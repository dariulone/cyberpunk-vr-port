//! The descriptor audit's draw binding capture (src/Render/StereoDescriptorAudit.cpp, `descriptors::`): for each draw
//! snapshotted while recording, every range of its supported root whose parameter the draw (or the list's tracked
//! tables, when they are the draw's root) binds -- its GPU address from the table base and offset, its range identity
//! in the store (heap, version, count) and, for at most 128 descriptors, the exported canonical value of each (values
//! deduplicated, at most 1048576 of them); a draw is complete when every bound range resolves (and a root past 32
//! parameters has its tables tracked). The draw stays pending for its list until submission re-checks every range's
//! identity (bit 2 submitted, bit 4 changed: no longer complete) or a Reset abandons it (8). At most 1048576 bindings,
//! 8 Mi ids and the probe's draw capacity.
//!
//! In the Rust build (`CVR_RUST_VRIK`) the capture state, Snapshot's walk, Export, BeforeSubmit's re-check and Reset's
//! abandonment forward to `cvr_sda_capture_*`; the roots, the tracked tables and the store stay the .cpp's, lent per call.
//! `tools/single_pass_tests/descriptor_audit_test` runs its cases against it in its Rust variant.

use cvr_host::fast::FastMap;
use std::sync::{Mutex, MutexGuard};

use crate::descriptor_store::{Store, Value, VALUE_LIMIT};
use crate::source_layout::Binding;

const BINDING_LIMIT: usize = 1048576;
const ID_LIMIT: usize = 8 * 1048576;
const SAMPLER: u32 = 3;

/// DrawBindings.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DrawBindings {
    pub sequence: u32,
    pub offset: u32,
    pub count: u32,
    pub complete: u32,
}
/// The .cpp's Tables: one list's tracked root and its bound tables.
#[repr(C)]
pub struct Tables {
    pub root: u64,
    pub mask: u64,
    pub addresses: [u64; 64],
}
/// One snapshotted draw's identity and bindings, lent.
#[repr(C)]
pub struct Draw {
    pub sequence: u64,
    pub list: u64,
    pub root: u64,
    pub table_mask: u32,
    pub tables: *const u64,
}
/// The draw's root layout, lent (null ranges when the root is unknown).
#[repr(C)]
pub struct Layout {
    pub ranges: *const Binding,
    pub count: usize,
    pub supported: bool,
    pub parameters: u32,
}
/// The capture's arrays for the .cpp's exports.
#[repr(C)]
pub struct View {
    pub draws: *const DrawBindings,
    pub draw_count: usize,
    pub bindings: *const Binding,
    pub binding_count: usize,
    pub ids: *const u32,
    pub id_count: usize,
    pub values: *const Value,
    pub value_count: usize,
}
const _: () = assert!(
    core::mem::size_of::<DrawBindings>() == 16
        && core::mem::size_of::<Binding>() == 64
        && core::mem::size_of::<Tables>() == 528
        && core::mem::size_of::<Draw>() == 40
        && core::mem::size_of::<Layout>() == 24
        && core::mem::size_of::<View>() == 64
);

struct Capture {
    draws: Vec<DrawBindings>,
    bindings: Vec<Binding>,
    ids: Vec<u32>,
    values: Vec<Value>,
    exported: FastMap<Value, u32>,
    pending: FastMap<u64, Vec<u32>>,
}
impl Default for Capture {
    fn default() -> Self {
        Capture { draws: Vec::new(), bindings: Vec::new(), ids: Vec::new(), values: vec![[0; 20]], exported: cvr_host::fast::fast_map(), pending: cvr_host::fast::fast_map() }
    }
}
static CAPTURE: Mutex<Option<Capture>> = Mutex::new(None);
fn capture() -> MutexGuard<'static, Option<Capture>> {
    CAPTURE.lock().unwrap_or_else(|e| e.into_inner())
}

impl Capture {
    /// Export: the descriptor's canonical value's index among the exported values (0 for none).
    fn export(&mut self, store: &Store, id: u32) -> u32 {
        if id == 0 {
            return 0;
        }
        let Some(value) = store.get(id) else {
            return 0;
        };
        if self.values.len() >= VALUE_LIMIT {
            return 0;
        }
        if let Some(&index) = self.exported.get(value) {
            return index;
        }
        let index = self.values.len() as u32;
        self.values.push(*value);
        self.exported.insert(*value, index);
        index
    }
}

/// The outcome of one Snapshot: the capture failure to report (0 for none) and whether recording stops.
pub struct Outcome {
    pub failure: u32,
    pub stop: bool,
}

/// Snapshot: the draw's bindings captured (see the module documentation).
#[allow(clippy::too_many_arguments)]
pub fn snapshot(store: &Store, draw: &Draw, tables: &[u64; 32], layout: Option<(&[Binding], bool, u32)>, tracked: Option<&Tables>, resource_stride: u32, sampler_stride: u32, draw_capacity: usize) -> Outcome {
    let mut guard = capture();
    let c = guard.get_or_insert_with(Capture::default);
    let mut outcome = Outcome { failure: 0, stop: false };
    let mut result = DrawBindings { sequence: draw.sequence as u32, offset: c.bindings.len() as u32, count: 0, complete: 0 };
    if let Some((ranges, true, parameters)) = layout {
        if !store.overflow() {
            let own = tracked.filter(|t| t.root == draw.root);
            result.complete = (parameters <= 32 || own.is_some()) as u32;
            for range in ranges {
                let mut b = *range;
                let mask = own.map_or(draw.table_mask as u64, |t| t.mask);
                if mask & (1u64 << b.parameter) == 0 {
                    continue;
                }
                let stride = if b.kind == SAMPLER { sampler_stride } else { resource_stride };
                let base = match own {
                    Some(t) => t.addresses[b.parameter as usize],
                    None if b.parameter < 32 => tables[b.parameter as usize],
                    None => 0,
                };
                if stride == 0 || base > u64::MAX - b.table_offset as u64 * stride as u64 {
                    result.complete = 0;
                    continue;
                }
                b.gpu = base + b.table_offset as u64 * stride as u64;
                let identity = store.range(b.gpu, b.count);
                if identity.valid {
                    b.heap = identity.heap;
                    b.version = identity.version;
                    b.status = 1;
                    b.count = identity.count;
                    if b.count <= 128 {
                        b.value_offset = c.ids.len() as u32;
                        b.value_count = b.count;
                        b.status = 2;
                        for i in 0..b.count {
                            let id = c.export(store, store.at(identity.cpu + i as u64 * stride as u64));
                            c.ids.push(id);
                            if id == 0 {
                                b.status = 1;
                            }
                        }
                    }
                } else {
                    result.complete = 0;
                }
                if c.bindings.len() >= BINDING_LIMIT || c.ids.len() > ID_LIMIT {
                    outcome = Outcome { failure: 5, stop: true };
                    break;
                }
                c.bindings.push(b);
                result.count += 1;
            }
        }
    }
    if c.draws.len() < draw_capacity {
        c.pending.entry(draw.list).or_default().push(c.draws.len() as u32);
        c.draws.push(result);
    } else {
        outcome.failure = 5;
    }
    outcome
}
/// BeforeSubmit: the submitted lists' pending draws re-checked against the store; the pending lists left.
pub fn submit(store: &Store, lists: &[u64]) -> usize {
    let mut guard = capture();
    let c = guard.get_or_insert_with(Capture::default);
    for list in lists {
        let Some(indices) = c.pending.remove(list) else {
            continue;
        };
        for index in indices {
            let draw = &mut c.draws[index as usize];
            draw.complete |= 2;
            for b in &c.bindings[draw.offset as usize..(draw.offset + draw.count) as usize] {
                if b.status == 0 {
                    continue;
                }
                let current = store.range(b.gpu, b.count);
                if !current.valid || current.heap != b.heap || current.version != b.version {
                    draw.complete = (draw.complete & !1) | 4;
                }
            }
        }
    }
    c.pending.len()
}
/// Reset's abandonment of the list's pending draws: the pending lists left, or None when it had none.
pub fn reset_list(list: u64) -> Option<usize> {
    let mut guard = capture();
    let c = guard.get_or_insert_with(Capture::default);
    let indices = c.pending.remove(&list)?;
    for index in indices {
        c.draws[index as usize].complete = 8;
    }
    Some(c.pending.len())
}

// ---- the C ABI

/// `which` 0: BeginCapture's clear; 1: the pending lists; 2: the view into `view`.
///
/// # Safety
/// For 2, `view` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_capture_state(which: u32, view: *mut View) -> usize {
    let mut guard = capture();
    let c = guard.get_or_insert_with(Capture::default);
    match which {
        0 => {
            *c = Capture::default();
            0
        }
        1 => c.pending.len(),
        _ => {
            unsafe {
                *view = View {
                    draws: c.draws.as_ptr(),
                    draw_count: c.draws.len(),
                    bindings: c.bindings.as_ptr(),
                    binding_count: c.bindings.len(),
                    ids: c.ids.as_ptr(),
                    id_count: c.ids.len(),
                    values: c.values.as_ptr(),
                    value_count: c.values.len(),
                };
            }
            0
        }
    }
}
/// Snapshot: the failure (0 none, 5 capacity) in the low bits, bit 8 when recording stops.
///
/// # Safety
/// `store` is the .cpp's Rust store; `draw` and its tables are valid; `layout` is valid or null; `tracked` is valid or
/// null.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_sda_capture_snapshot(
    store: *const Store,
    draw: *const Draw,
    layout: *const Layout,
    tracked: *const Tables,
    resource_stride: u32,
    sampler_stride: u32,
    draw_capacity: usize,
) -> u32 {
    let (store, draw) = unsafe { (&*store, &*draw) };
    let tables = unsafe { &*(draw.tables as *const [u64; 32]) };
    let layout = unsafe { layout.as_ref() }.filter(|l| !l.ranges.is_null() || l.count == 0).map(|l| {
        let ranges = if l.count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(l.ranges, l.count) } };
        (ranges, l.supported, l.parameters)
    });
    let outcome = snapshot(store, draw, tables, layout, unsafe { tracked.as_ref() }, resource_stride, sampler_stride, draw_capacity);
    outcome.failure | if outcome.stop { 1 << 8 } else { 0 }
}
/// BeforeSubmit's re-check: the pending lists left.
///
/// # Safety
/// `store` is the .cpp's Rust store; `lists` holds `count` lists.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_capture_submit(store: *const Store, lists: *const u64, count: usize) -> usize {
    let lists = if lists.is_null() || count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(lists, count) } };
    submit(unsafe { &*store }, lists)
}
/// Reset's abandonment: the pending lists left, or usize::MAX when the list had none.
///
/// # Safety
/// None.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_capture_reset(list: u64) -> usize {
    reset_list(list).unwrap_or(usize::MAX)
}
