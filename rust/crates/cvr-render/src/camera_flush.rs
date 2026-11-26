//! THE CAMERA CBV DESCRIPTOR FLUSH STATE (include/Render/NativeCameraFlushState.hpp): the native recording state's
//! descriptor heap allocation (heap, base, capacity, stride, cursor), the CPU shadow table, the root layout and its
//! sampler / resource masks and per-stage visibility, the bound programs, the dirty and rebind words and the cached
//! GPU / CPU tables; for each requested stage the camera CBV's resource group -- its table entry (start, first,
//! count, root parameter, not a sampler table) and every CPU descriptor -- under exact bounds, the tables disjoint;
//! read twice and equal. Room: nothing dirty in the graphics groups, no rebind, room for two copies. SameAllocation,
//! Pending (one descriptor replaced in every table, dirty and uncached, the cursor unmoved) and Flushed (the cursor
//! advanced by the copies, the groups clean) compare snapshots. The header forwards to these in the Rust build;
//! tools/single_pass_tests/camera_flush_state_test runs its cases against them.
use core::ffi::c_void;

use crate::camera_binding::{Context, ReadState};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Table {
    pub group: u32,
    pub root_parameter: u32,
    pub start: u32,
    pub count: u32,
    pub first: u32,
    pub cached: i32,
    pub descriptors: Vec<u64>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub context: Context,
    pub heap: u64,
    pub layout: u64,
    pub cpu_table: u64,
    pub dirty: u64,
    pub rebind: u64,
    pub groups: u64,
    pub resource_mask: u64,
    pub sampler_mask: u64,
    pub graphics_mask: u64,
    pub visibility: [u64; 4],
    pub programs: [u32; 3],
    pub cached_cpu: u64,
    pub cached_gpu: u64,
    pub cpu_valid: u8,
    pub gpu_valid: u8,
    pub base: u32,
    pub capacity: u32,
    pub stride: u32,
    pub cursor: u32,
    pub count: u32,
    pub tables: Vec<Table>,
}

fn copy<T: Copy>(read: Option<ReadState>, at: u64, out: &mut T) -> bool {
    copy_bytes(
        read,
        at,
        out as *mut T as *mut c_void,
        core::mem::size_of::<T>(),
    )
}
fn copy_bytes(read: Option<ReadState>, at: u64, out: *mut c_void, bytes: usize) -> bool {
    match read {
        Some(read) => {
            at != 0 && at <= u64::MAX - bytes as u64 && unsafe { read(at as usize, out, bytes) }
        }
        None => false,
    }
}

/// ReadOnce.
pub fn read_once(read: Option<ReadState>, context: Context, stages: u32) -> Option<Snapshot> {
    if !context.valid()
        || stages == 0
        || stages > 3
        || context.state > u64::MAX - 0x181
        || context.context > u64::MAX - 0x3c8
    {
        return None;
    }
    let mut s = Snapshot {
        context,
        ..Snapshot::default()
    };
    let (mut list, mut state, mut full_rebind) = (0u64, 0u64, 0u8);
    if !copy(read, context.context + 0x30, &mut list)
        || !copy(read, context.context + 0x60, &mut state)
        || list != context.list
        || state != context.state
        || !copy(read, state, &mut s.heap)
        || !copy(read, state + 8, &mut s.base)
        || !copy(read, state + 0xc, &mut s.capacity)
        || !copy(read, state + 0x10, &mut s.stride)
        || !copy(read, state + 0x14, &mut s.cursor)
        || !copy(read, state + 0x28, &mut s.cpu_table)
        || !copy(read, state + 0x68, &mut s.layout)
        || !copy(read, state + 0x70, &mut s.dirty)
        || !copy(read, state + 0x78, &mut s.rebind)
        || !copy(read, state + 0x50, &mut s.cached_cpu)
        || !copy(read, state + 0x58, &mut s.cached_gpu)
        || !copy(read, state + 0x60, &mut s.cpu_valid)
        || !copy(read, state + 0x61, &mut s.gpu_valid)
        || s.cpu_valid > 1
        || s.gpu_valid > 1
        || !copy(read, state + 0x180, &mut full_rebind)
        || full_rebind != 0
        || s.heap == 0
        || s.cpu_table == 0
        || s.layout == 0
        || s.layout > u64::MAX - 0x473
        || s.stride == 0
        || s.stride > 256
        || s.capacity == 0
        || s.cursor > s.capacity
    {
        return None;
    }
    let mut programs01 = [0u32; 2];
    if !copy(read, s.layout, &mut s.sampler_mask)
        || !copy(read, s.layout + 8, &mut s.resource_mask)
        || !copy(read, s.layout + 0x18, &mut s.visibility)
        || !copy(read, context.context + 0x3c0, &mut programs01)
        || !copy(read, context.context + 0x10c, &mut s.programs[2])
    {
        return None;
    }
    s.programs[0] = programs01[0];
    s.programs[1] = programs01[1];
    for stage in 0..3 {
        if s.programs[stage] != 0 && s.programs[stage] <= 0x7fff_ffff {
            s.graphics_mask |= s.visibility[stage];
        }
    }
    s.graphics_mask &= s.resource_mask | s.sampler_mask;
    for stage in 0..2u64 {
        if stages & (1 << stage) == 0 {
            continue;
        }
        let program = s.programs[stage as usize];
        if program == 0 || program > 0x7fff_ffff {
            return None;
        }
        let mut group = 0u8;
        if !copy(read, s.layout + 0x455 + stage * 0x1c, &mut group) || group >= 64 {
            return None;
        }
        let bit = 1u64 << group;
        if s.resource_mask & bit == 0
            || s.sampler_mask & bit != 0
            || s.visibility[stage as usize] & bit == 0
        {
            return None;
        }
        if s.groups & bit != 0 {
            continue;
        }
        if s.layout > u64::MAX - 0x38 - group as u64 * 16 - 16 {
            return None;
        }
        let entry = s.layout + 0x38 + group as u64 * 16;
        let (mut first, mut count, mut kind, mut parameter) = (0u16, 0u16, 0u8, 0u8);
        let mut table = Table {
            group: group as u32,
            ..Table::default()
        };
        if !copy(read, entry + 4, &mut table.start)
            || !copy(read, entry + 8, &mut first)
            || !copy(read, entry + 10, &mut count)
            || !copy(read, entry + 13, &mut kind)
            || kind == 2
            || !copy(read, entry + 14, &mut parameter)
            || parameter >= 64
            || count == 0
            || first > 1
            || (1 - first as u32) >= count as u32
            || table.start > 8192
            || count as u32 > 8192 - table.start
            || !copy(read, s.state_cached(group), &mut table.cached)
        {
            return None;
        }
        table.root_parameter = parameter as u32;
        table.first = first as u32;
        table.count = count as u32;
        table.descriptors = vec![0u64; count as usize];
        for previous in &s.tables {
            if previous.root_parameter == table.root_parameter
                || (previous.start < table.start + table.count
                    && table.start < previous.start + previous.count)
            {
                return None;
            }
        }
        let offset = table.start as u64 * 8;
        if s.cpu_table > u64::MAX - offset
            || !copy_bytes(
                read,
                s.cpu_table + offset,
                table.descriptors.as_mut_ptr().cast(),
                count as usize * 8,
            )
            || table.descriptors.iter().any(|&p| p == 0)
        {
            return None;
        }
        s.count += count as u32;
        s.groups |= bit;
        s.tables.push(table);
    }
    if s.count == 0
        || s.base as u64 + s.capacity as u64 > u32::MAX as u64
        || (s.base as u64 + s.cursor as u64 + s.count as u64 * 2) * s.stride as u64
            > u32::MAX as u64
    {
        return None;
    }
    Some(s)
}
impl Snapshot {
    /// The group's cached table index in the native state (+0x80 + group*4).
    fn state_cached(&self, group: u8) -> u64 {
        self.context.state + 0x80 + group as u64 * 4
    }
}
/// Read: twice and equal.
pub fn read(read: Option<ReadState>, context: Context, stages: u32) -> Option<Snapshot> {
    let a = read_once(read, context, stages)?;
    let b = read_once(read, context, stages)?;
    (a == b).then_some(a)
}
/// Room.
pub fn room(s: &Snapshot) -> bool {
    s.dirty & s.graphics_mask == 0
        && s.rebind == 0
        && s.count != 0
        && s.cursor <= s.capacity
        && s.count as u64 * 2 <= (s.capacity - s.cursor) as u64
}
/// SameAllocation.
pub fn same_allocation(a: &Snapshot, b: &Snapshot) -> bool {
    if a.context != b.context
        || a.heap != b.heap
        || a.layout != b.layout
        || a.cpu_table != b.cpu_table
        || a.base != b.base
        || a.capacity != b.capacity
        || a.stride != b.stride
        || a.groups != b.groups
        || a.count != b.count
        || a.tables.len() != b.tables.len()
        || a.resource_mask != b.resource_mask
        || a.sampler_mask != b.sampler_mask
        || a.graphics_mask != b.graphics_mask
        || a.visibility != b.visibility
        || a.programs != b.programs
    {
        return false;
    }
    if (a.cpu_valid != 0 && (b.cpu_valid == 0 || a.cached_cpu != b.cached_cpu))
        || (a.gpu_valid != 0 && (b.gpu_valid == 0 || a.cached_gpu != b.cached_gpu))
    {
        return false;
    }
    a.tables.iter().zip(&b.tables).all(|(x, y)| {
        x.group == y.group
            && x.root_parameter == y.root_parameter
            && x.start == y.start
            && x.count == y.count
            && x.first == y.first
    })
}
/// Pending: the replacement written into every table's camera slot, the groups dirty and uncached.
pub fn pending(original: &Snapshot, next: &Snapshot, replacement: u64) -> bool {
    if replacement == 0
        || !same_allocation(original, next)
        || next.cursor != original.cursor
        || next.dirty != (original.dirty | original.groups)
        || next.rebind != 0
    {
        return false;
    }
    original.tables.iter().zip(&next.tables).all(|(a, b)| {
        let slot = 1u32.wrapping_sub(a.first) as usize;
        b.cached == -1
            && a.descriptors.len() == b.descriptors.len()
            && b.descriptors.iter().enumerate().all(|(i, &d)| {
                d == if i == slot {
                    replacement
                } else {
                    a.descriptors[i]
                }
            })
    })
}
/// Flushed: the copies made at the cursor, the groups clean.
pub fn flushed(pending: &Snapshot, next: &Snapshot) -> bool {
    if !same_allocation(pending, next)
        || next.cursor != pending.cursor.wrapping_add(pending.count)
        || next.dirty != (pending.dirty & !pending.groups)
        || next.rebind != 0
    {
        return false;
    }
    pending
        .tables
        .iter()
        .zip(&next.tables)
        .all(|(p, n)| p.descriptors == n.descriptors && n.cached == -1)
}

// ---- the C ABI: the C++ snapshot's records, borrowed, and a read's result -------------------------------------------

/// flush_state::Table, borrowed.
#[repr(C)]
pub struct TableView {
    pub group: u32,
    pub root_parameter: u32,
    pub start: u32,
    pub count: u32,
    pub first: u32,
    pub cached: i32,
    pub descriptors: *const u64,
    pub descriptor_count: usize,
}
/// flush_state::Snapshot, borrowed.
#[repr(C)]
pub struct SnapshotView {
    pub context: Context,
    pub heap: u64,
    pub layout: u64,
    pub cpu_table: u64,
    pub dirty: u64,
    pub rebind: u64,
    pub groups: u64,
    pub resource_mask: u64,
    pub sampler_mask: u64,
    pub graphics_mask: u64,
    pub visibility: [u64; 4],
    pub programs: [u32; 3],
    pub cached_cpu: u64,
    pub cached_gpu: u64,
    pub cpu_valid: u8,
    pub gpu_valid: u8,
    pub base: u32,
    pub capacity: u32,
    pub stride: u32,
    pub cursor: u32,
    pub count: u32,
    pub tables: *const TableView,
    pub table_count: usize,
}
const _: () = assert!(
    core::mem::size_of::<TableView>() == 40
        && core::mem::size_of::<SnapshotView>() == 200
        && core::mem::offset_of!(SnapshotView, cached_cpu) == 144
        && core::mem::offset_of!(SnapshotView, tables) == 184
);

unsafe fn slice<'a, T>(data: *const T, count: usize) -> &'a [T] {
    if count == 0 || data.is_null() {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(data, count) }
    }
}
pub(crate) unsafe fn snapshot_of(v: &SnapshotView) -> Snapshot {
    unsafe {
        Snapshot {
            context: v.context,
            heap: v.heap,
            layout: v.layout,
            cpu_table: v.cpu_table,
            dirty: v.dirty,
            rebind: v.rebind,
            groups: v.groups,
            resource_mask: v.resource_mask,
            sampler_mask: v.sampler_mask,
            graphics_mask: v.graphics_mask,
            visibility: v.visibility,
            programs: v.programs,
            cached_cpu: v.cached_cpu,
            cached_gpu: v.cached_gpu,
            cpu_valid: v.cpu_valid,
            gpu_valid: v.gpu_valid,
            base: v.base,
            capacity: v.capacity,
            stride: v.stride,
            cursor: v.cursor,
            count: v.count,
            tables: slice(v.tables, v.table_count)
                .iter()
                .map(|t| Table {
                    group: t.group,
                    root_parameter: t.root_parameter,
                    start: t.start,
                    count: t.count,
                    first: t.first,
                    cached: t.cached,
                    descriptors: slice(t.descriptors, t.descriptor_count).to_vec(),
                })
                .collect(),
        }
    }
}

/// A read's snapshot with its tables' views (borrowed until cvr_fs_free).
pub struct Captured {
    snapshot: Snapshot,
    tables: Vec<TableView>,
}

/// ReadOnce (`twice` false) or Read (true): the snapshot (its fields through `out`, free it with cvr_fs_free) or
/// null.
///
/// # Safety
/// `context` is valid; `out` is writable; `read` follows camera_upload::ReadState.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_fs_read(
    read_state: Option<ReadState>,
    context: *const Context,
    stages: u32,
    twice: bool,
    out: *mut SnapshotView,
) -> *mut Captured {
    unsafe {
        let snapshot = if twice {
            read(read_state, *context, stages)
        } else {
            read_once(read_state, *context, stages)
        };
        let Some(snapshot) = snapshot else {
            return core::ptr::null_mut();
        };
        let tables = snapshot
            .tables
            .iter()
            .map(|t| TableView {
                group: t.group,
                root_parameter: t.root_parameter,
                start: t.start,
                count: t.count,
                first: t.first,
                cached: t.cached,
                descriptors: t.descriptors.as_ptr(),
                descriptor_count: t.descriptors.len(),
            })
            .collect::<Vec<_>>();
        let captured = Box::into_raw(Box::new(Captured { snapshot, tables }));
        let s = &(*captured).snapshot;
        *out = SnapshotView {
            context: s.context,
            heap: s.heap,
            layout: s.layout,
            cpu_table: s.cpu_table,
            dirty: s.dirty,
            rebind: s.rebind,
            groups: s.groups,
            resource_mask: s.resource_mask,
            sampler_mask: s.sampler_mask,
            graphics_mask: s.graphics_mask,
            visibility: s.visibility,
            programs: s.programs,
            cached_cpu: s.cached_cpu,
            cached_gpu: s.cached_gpu,
            cpu_valid: s.cpu_valid,
            gpu_valid: s.gpu_valid,
            base: s.base,
            capacity: s.capacity,
            stride: s.stride,
            cursor: s.cursor,
            count: s.count,
            tables: (*captured).tables.as_ptr(),
            table_count: (*captured).tables.len(),
        };
        captured
    }
}

/// # Safety
/// `captured` comes from cvr_fs_read (or is null) and is not used again.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_fs_free(captured: *mut Captured) {
    if !captured.is_null() {
        drop(unsafe { Box::from_raw(captured) });
    }
}

/// Room (`check` 0, `a`), SameAllocation (1, `a`, `b`), Pending (2, original `a`, next `b`, `replacement`) or Flushed
/// (3, pending `a`, next `b`).
///
/// # Safety
/// The views are valid and hold their counts (`b` for checks 1-3).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_fs_check(
    check: u32,
    a: *const SnapshotView,
    b: *const SnapshotView,
    replacement: u64,
) -> bool {
    unsafe {
        let a = snapshot_of(&*a);
        if check == 0 {
            return room(&a);
        }
        let b = snapshot_of(&*b);
        match check {
            1 => same_allocation(&a, &b),
            2 => pending(&a, &b, replacement),
            _ => flushed(&a, &b),
        }
    }
}
