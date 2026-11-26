//! The L/R pair probe (formerly src/Stereo/StereoPairProbe.cpp; include/Stereo/StereoPairProbe.hpp forwards here, the
//! file keeps the exported request, state, records and counters): how engine stereo sends the two eyes' draws. With
//! cvr_pair_probe.txt in the plugin's log folder at startup, the graphics root-parameter setters and the index buffer
//! setter of every hooked command-list vtable get pass-through detours that keep each list's bound state; asked for
//! whole frames, every draw inside one render-graph node is recorded with its pipeline, root signature, root parameters
//! (table handle, buffer address, or a hash of root constants), index and vertex buffers, viewport and arguments.
//! SINGLE-PASS FEASIBILITY: a right-eye draw whose left-eye partner is known gets that partner's tables for the root
//! parameters in CyberpunkVR_PairSubstituteMask (and its start instance), for the draw only.
#![allow(unsafe_op_in_unsafe_fn)]
use cvr_host::fast::FastMap;
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::*};
use std::cell::Cell;
use std::sync::Mutex;

use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;

const RECORD_PARAMS: usize = 40;
pub(crate) const CAPACITY: u32 = 16384;
const PARAMS: usize = 64;

/// One recorded draw (the exported records' layout).
#[repr(C)]
pub struct Record {
    list: u64,
    pso: u64,
    root: u64,
    ib: u64,
    vb: u64,
    params: [u64; RECORD_PARAMS],
    /// per parameter: 0 unset, 1 table, 2 CBV, 3 SRV, 4 UAV, 5 constants
    types: [u8; RECORD_PARAMS],
    node: u32,
    serial: u32,
    kind: u32,
    viewport_x: i32,
    viewport_width: u32,
    args: [u32; 5],
    /// the draw's place in its list
    sequence: u32,
    pad: u32,
}
const _: () = assert!(core::mem::size_of::<Record>() == 448);

#[allow(non_upper_case_globals)]
extern "C" {
    static CyberpunkVR_PairProbeRequest: AtomicU32;
    static CyberpunkVR_PairProbeState: AtomicU32;
    static CyberpunkVR_PairProbeNode: AtomicU32;
    static CyberpunkVR_PairProbeCount: AtomicU32;
    static mut CyberpunkVR_PairProbeRecords: [Record; CAPACITY as usize];
    static CyberpunkVR_PairSubstituteMask: AtomicU32;
    static CyberpunkVR_PairSubstituteMaskHigh: AtomicU32;
    static CyberpunkVR_PairSubstituteInstance: AtomicU32;
    static CyberpunkVR_PairSubstituteCounts: [AtomicU64; 4];
    static CyberpunkVR_PresentTiming: [AtomicU64; 8];
    /// The root signature's serialized blob (src/Stereo/DeviceHooks.cpp keeps them): its length, copied while it fits.
    fn cvr_cpp_rootsig_blob(root: *mut c_void, out: *mut u8, capacity: usize) -> usize;
}

type SigFn = unsafe extern "system" fn(*mut c_void, *mut c_void);
type RootFn = unsafe extern "system" fn(*mut c_void, u32, u64);
type ConstFn = unsafe extern "system" fn(*mut c_void, u32, u32, u32);
type ConstsFn = unsafe extern "system" fn(*mut c_void, u32, u32, *const c_void, u32);
type IbFn = unsafe extern "system" fn(*mut c_void, *const c_void);

#[derive(Clone, Copy)]
struct Originals {
    vtable: usize,
    signature: Option<SigFn>,
    table: Option<RootFn>,
    cbv: Option<RootFn>,
    srv: Option<RootFn>,
    uav: Option<RootFn>,
    constant: Option<ConstFn>,
    constants: Option<ConstsFn>,
    ib: Option<IbFn>,
}
const NO_ORIGINALS: Originals = Originals {
    vtable: 0,
    signature: None,
    table: None,
    cbv: None,
    srv: None,
    uav: None,
    constant: None,
    constants: None,
    ib: None,
};
static mut ORIGINALS: [Originals; 16] = [NO_ORIGINALS; 16];
static ORIGINAL_COUNT: AtomicU32 = AtomicU32::new(0);
static INSTALL: Mutex<()> = Mutex::new(());

unsafe fn originals_of(list: *mut c_void) -> Option<&'static Originals> {
    let vtable = *(list as *const usize);
    let n = ORIGINAL_COUNT.load(Acquire) as usize;
    let all: &'static [Originals; 16] = &*(&raw const ORIGINALS);
    all[..n].iter().find(|o| o.vtable == vtable)
}

#[derive(Clone)]
struct Partner {
    pso: u64,
    ib: u64,
    vb: u64,
    kind: u32,
    start_instance: u32,
    tables: [u64; PARAMS],
    table_mask: u64,
}
struct State {
    root: usize,
    params: [u64; RECORD_PARAMS],
    types: [u8; RECORD_PARAMS],
    /// every table parameter's handle as the engine bound it
    tables: [u64; PARAMS],
    /// which of them are tables
    table_mask: u64,
    /// the left batch just recorded
    left: Vec<Partner>,
    right_index: u32,
    /// 0 left, 1 right, -1 other
    last_eye: i32,
    /// tables AfterDraw must put back
    restore_mask: u64,
    constants: [[u32; 64]; RECORD_PARAMS],
    ib: u64,
    vb: u64,
    viewport_x: i32,
    viewport_width: u32,
    sequence: u32,
}
impl Default for State {
    fn default() -> State {
        State {
            root: 0,
            params: [0; RECORD_PARAMS],
            types: [0; RECORD_PARAMS],
            tables: [0; PARAMS],
            table_mask: 0,
            left: Vec::new(),
            right_index: 0,
            last_eye: -1,
            restore_mask: 0,
            constants: [[0; 64]; RECORD_PARAMS],
            ib: 0,
            vb: 0,
            viewport_x: -1,
            viewport_width: 0,
            sequence: 0,
        }
    }
}
static STATES: Mutex<Option<FastMap<usize, Box<State>>>> = Mutex::new(None);
thread_local! {
    static LAST: Cell<(usize, *mut State)> = const { Cell::new((0, core::ptr::null_mut())) };
}
/// The list's state: kept for good (a list's box never moves), cached per thread.
unsafe fn state_of(list: *mut c_void) -> &'static mut State {
    let (cached_list, cached) = LAST.get();
    if cached_list == list as usize && !cached.is_null() {
        return &mut *cached;
    }
    let mut states = STATES.lock().unwrap_or_else(|p| p.into_inner());
    let slot = states
        .get_or_insert_with(cvr_host::fast::fast_map)
        .entry(list as usize)
        .or_insert_with(|| Box::new(State::default()));
    let p: *mut State = &mut **slot;
    LAST.set((list as usize, p));
    &mut *p
}

static WANTED: Mutex<Option<bool>> = Mutex::new(None);
static TRACKING: AtomicBool = AtomicBool::new(false);
static WINDOW: Mutex<(u64, u64)> = Mutex::new((0, 0));

fn fnv(data: &[u8], mut h: u64) -> u64 {
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    h
}
const FNV_START: u64 = 1469598103934665603;
unsafe fn bytes_of<T>(v: *const T, count: usize) -> &'static [u8] {
    core::slice::from_raw_parts(v as *const u8, core::mem::size_of::<T>() * count)
}

unsafe fn set_param(list: *mut c_void, index: u32, value: u64, kind: u64) {
    let s = state_of(list);
    if (index as usize) < PARAMS {
        if kind == 1 {
            s.tables[index as usize] = value;
            s.table_mask |= 1u64 << index;
        } else {
            s.table_mask &= !(1u64 << index);
        }
    }
    if index as usize >= RECORD_PARAMS {
        return;
    }
    s.params[index as usize] = value;
    s.types[index as usize] = kind as u8;
}
unsafe extern "system" fn signature(list: *mut c_void, root: *mut c_void) {
    if let Some(f) = originals_of(list).and_then(|o| o.signature) {
        f(list, root);
    }
    let s = state_of(list);
    s.root = root as usize;
    s.params = [0; RECORD_PARAMS];
    s.types = [0; RECORD_PARAMS];
    s.tables = [0; PARAMS];
    s.table_mask = 0;
}
unsafe extern "system" fn table(list: *mut c_void, index: u32, handle: u64) {
    if let Some(f) = originals_of(list).and_then(|o| o.table) {
        f(list, index, handle);
    }
    set_param(list, index, handle, 1);
}
unsafe extern "system" fn cbv(list: *mut c_void, index: u32, address: u64) {
    if let Some(f) = originals_of(list).and_then(|o| o.cbv) {
        f(list, index, address);
    }
    set_param(list, index, address, 2);
}
unsafe extern "system" fn srv(list: *mut c_void, index: u32, address: u64) {
    if let Some(f) = originals_of(list).and_then(|o| o.srv) {
        f(list, index, address);
    }
    set_param(list, index, address, 3);
}
unsafe extern "system" fn uav(list: *mut c_void, index: u32, address: u64) {
    if let Some(f) = originals_of(list).and_then(|o| o.uav) {
        f(list, index, address);
    }
    set_param(list, index, address, 4);
}
unsafe fn constants_into(list: *mut c_void, index: u32, count: u32, values: *const u32, offset: u32) {
    if index as usize >= RECORD_PARAMS {
        return;
    }
    let s = state_of(list);
    let mut i = 0u32;
    while i < count && offset.wrapping_add(i) < 64 {
        s.constants[index as usize][(offset + i) as usize] = *values.add(i as usize);
        i += 1;
    }
    let hash = fnv(bytes_of(s.constants[index as usize].as_ptr(), 64), FNV_START);
    set_param(list, index, hash, 5);
}
unsafe extern "system" fn constant(list: *mut c_void, index: u32, value: u32, offset: u32) {
    if let Some(f) = originals_of(list).and_then(|o| o.constant) {
        f(list, index, value, offset);
    }
    constants_into(list, index, 1, &value, offset);
}
unsafe extern "system" fn constants(list: *mut c_void, index: u32, count: u32, values: *const c_void, offset: u32) {
    if let Some(f) = originals_of(list).and_then(|o| o.constants) {
        f(list, index, count, values, offset);
    }
    if !values.is_null() {
        constants_into(list, index, count, values as *const u32, offset);
    }
}
unsafe extern "system" fn index_buffer(list: *mut c_void, view: *const c_void) {
    if let Some(f) = originals_of(list).and_then(|o| o.ib) {
        f(list, view);
    }
    state_of(list).ib = if view.is_null() {
        0
    } else {
        fnv(bytes_of(view as *const D3D12_INDEX_BUFFER_VIEW, 1), FNV_START)
    };
}

/// The layout of each root signature the probed node draws with, once (eight at most), into the plugin's log.
unsafe fn describe_root(root: usize) {
    static DESCRIBED: Mutex<Vec<usize>> = Mutex::new(Vec::new());
    if root == 0 {
        return;
    }
    {
        let mut d = DESCRIBED.lock().unwrap_or_else(|p| p.into_inner());
        if d.len() >= 8 || d.contains(&root) {
            return;
        }
        d.push(root);
    }
    let length = cvr_cpp_rootsig_blob(root as *mut c_void, core::ptr::null_mut(), 0);
    let mut blob = vec![0u8; length];
    if length == 0 || cvr_cpp_rootsig_blob(root as *mut c_void, blob.as_mut_ptr(), length) != length {
        cvr_host::log!("[pair-probe] root {:016X}: blob not kept\n", root);
        return;
    }
    let mut raw: *mut c_void = core::ptr::null_mut();
    if D3D12CreateVersionedRootSignatureDeserializer(
        blob.as_ptr() as *const c_void,
        blob.len(),
        &ID3D12VersionedRootSignatureDeserializer::IID,
        &mut raw,
    )
    .is_err()
        || raw.is_null()
    {
        return;
    }
    let des = ID3D12VersionedRootSignatureDeserializer::from_raw(raw);
    let Ok(v) = des.GetRootSignatureDescAtVersion(D3D_ROOT_SIGNATURE_VERSION_1_1) else {
        return;
    };
    let Some(v) = v.as_ref() else { return };
    let d = &v.Anonymous.Desc_1_1;
    cvr_host::log!(
        "[pair-probe] root {:016X}: {} params, {} static samplers, flags 0x{:X}\n",
        root,
        d.NumParameters,
        d.NumStaticSamplers,
        d.Flags.0 as u32
    );
    for i in 0..d.NumParameters {
        let p = &*d.pParameters.add(i as usize);
        let mut line = format!(
            "[pair-probe]   [{:2}] type {} vis {}:",
            i, p.ParameterType.0 as u32, p.ShaderVisibility.0 as u32
        );
        if p.ParameterType == D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE {
            let t = &p.Anonymous.DescriptorTable;
            for k in 0..t.NumDescriptorRanges {
                if line.len() >= 540 {
                    break;
                }
                let r = &*t.pDescriptorRanges.add(k as usize);
                let kind = match r.RangeType {
                    D3D12_DESCRIPTOR_RANGE_TYPE_CBV => "b",
                    D3D12_DESCRIPTOR_RANGE_TYPE_SRV => "t",
                    D3D12_DESCRIPTOR_RANGE_TYPE_UAV => "u",
                    _ => "s",
                };
                line.push_str(&format!(
                    " {}{}+{}/sp{}@{} f{:X}",
                    kind, r.BaseShaderRegister, r.NumDescriptors as i32, r.RegisterSpace, r.OffsetInDescriptorsFromTableStart as i32, r.Flags.0 as u32
                ));
            }
        } else if p.ParameterType == D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS {
            let c = &p.Anonymous.Constants;
            line.push_str(&format!(" b{}/sp{} x{}", c.ShaderRegister, c.RegisterSpace, c.Num32BitValues));
        } else {
            let r = &p.Anonymous.Descriptor;
            line.push_str(&format!(" r{}/sp{}", r.ShaderRegister, r.RegisterSpace));
        }
        cvr_host::log!("{}\n", line);
    }
}

/// Wanted: the marker in the log folder, looked for once.
#[no_mangle]
pub extern "C" fn cvr_pair_probe_wanted() -> bool {
    let mut w = WANTED.lock().unwrap_or_else(|p| p.into_inner());
    *w.get_or_insert_with(|| {
        let found = cvr_host::paths::file(cvr_host::paths::Folder::Log, "cvr_pair_probe.txt").exists();
        if found {
            cvr_host::log!("[pair-probe] marker found: root-parameter detours will be installed\n");
        }
        found
    })
}
/// Install: with the vtable writable, after the census hooks -- the originals of slots 30, 32, 34, 36, 38, 40, 42, 43
/// saved for this vtable and the detours put in.
///
/// # Safety
/// `vtable` is an ID3D12GraphicsCommandList vtable, writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_pair_probe_install(vtable: *mut *mut c_void) {
    let _held = INSTALL.lock().unwrap_or_else(|p| p.into_inner());
    let n = ORIGINAL_COUNT.load(Relaxed) as usize;
    let all = &mut *(&raw mut ORIGINALS);
    if all[..n].iter().any(|o| o.vtable == vtable as usize) || n >= all.len() {
        return;
    }
    let slot = |i: usize| *vtable.add(i);
    all[n] = Originals {
        vtable: vtable as usize,
        signature: core::mem::transmute::<*mut c_void, Option<SigFn>>(slot(30)),
        table: core::mem::transmute::<*mut c_void, Option<RootFn>>(slot(32)),
        constant: core::mem::transmute::<*mut c_void, Option<ConstFn>>(slot(34)),
        constants: core::mem::transmute::<*mut c_void, Option<ConstsFn>>(slot(36)),
        cbv: core::mem::transmute::<*mut c_void, Option<RootFn>>(slot(38)),
        srv: core::mem::transmute::<*mut c_void, Option<RootFn>>(slot(40)),
        uav: core::mem::transmute::<*mut c_void, Option<RootFn>>(slot(42)),
        ib: core::mem::transmute::<*mut c_void, Option<IbFn>>(slot(43)),
    };
    ORIGINAL_COUNT.store(n as u32 + 1, Release);
    let detours: [(usize, *const ()); 8] = [
        (30, signature as *const ()),
        (32, table as *const ()),
        (34, constant as *const ()),
        (36, constants as *const ()),
        (38, cbv as *const ()),
        (40, srv as *const ()),
        (42, uav as *const ()),
        (43, index_buffer as *const ()),
    ];
    for (index, detour) in detours {
        (*(vtable.add(index) as *const core::sync::atomic::AtomicPtr<c_void>)).swap(detour as *mut c_void, SeqCst);
    }
    TRACKING.store(true, Release);
    cvr_host::log!("[pair-probe] installed on vtable {:016X}\n", vtable as usize);
}
/// Tracking: the marker was found (state is kept).
#[no_mangle]
pub extern "C" fn cvr_pair_probe_tracking() -> bool {
    TRACKING.load(Relaxed)
}
/// Reset: a list's state started over (the list was reset).
///
/// # Safety
/// `list` is a live command list.
#[no_mangle]
pub unsafe extern "C" fn cvr_pair_probe_reset(list: *mut c_void) {
    if !cvr_pair_probe_tracking() {
        return;
    }
    *state_of(list) = State::default();
}
/// Viewports: the first viewport's left edge and width, rounded.
///
/// # Safety
/// `viewports` holds `count` viewports or is null.
#[no_mangle]
pub unsafe extern "C" fn cvr_pair_probe_viewports(list: *mut c_void, count: u32, viewports: *const D3D12_VIEWPORT) {
    if !cvr_pair_probe_tracking() {
        return;
    }
    let s = state_of(list);
    if count == 0 || viewports.is_null() {
        s.viewport_x = -1;
        s.viewport_width = 0;
        return;
    }
    let v = &*viewports;
    s.viewport_x = (v.TopLeftX + 0.5) as i32;
    s.viewport_width = (v.Width + 0.5) as u32;
}
/// VertexBuffers: a hash of the start slot, the count and the views.
///
/// # Safety
/// `views` holds `count` views or is null.
#[no_mangle]
pub unsafe extern "C" fn cvr_pair_probe_vertex_buffers(list: *mut c_void, start: u32, count: u32, views: *const D3D12_VERTEX_BUFFER_VIEW) {
    if !cvr_pair_probe_tracking() {
        return;
    }
    let s = state_of(list);
    s.vb = fnv(bytes_of(&start, 1), fnv(bytes_of(&count, 1), FNV_START));
    if !views.is_null() && count != 0 {
        s.vb = fnv(bytes_of(views, count as usize), s.vb);
    }
}
/// Draw: the draw recorded while a window is open and the node is the probed one (kind 1 DrawInstanced, 2
/// DrawIndexedInstanced, 3 ExecuteIndirect).
///
/// # Safety
/// `args` holds five arguments.
#[no_mangle]
pub unsafe extern "C" fn cvr_pair_probe_draw(list: *mut c_void, node: u32, pso: *mut c_void, kind: u32, args: *const [u32; 5]) {
    if !cvr_pair_probe_tracking() {
        return;
    }
    let s = state_of(list);
    let sequence = s.sequence;
    s.sequence = s.sequence.wrapping_add(1);
    if CyberpunkVR_PairProbeState.load(Relaxed) != 2 {
        return;
    }
    let wanted_node = CyberpunkVR_PairProbeNode.load(Relaxed);
    if wanted_node != 0 && node != wanted_node {
        return;
    }
    let serial = CyberpunkVR_PresentTiming[0].load(Relaxed);
    let (start, end) = *WINDOW.lock().unwrap_or_else(|p| p.into_inner());
    if serial <= start || serial > end {
        return;
    }
    let index = CyberpunkVR_PairProbeCount.fetch_add(1, Relaxed);
    if index >= CAPACITY {
        return;
    }
    let r = &mut (*(&raw mut CyberpunkVR_PairProbeRecords))[index as usize];
    r.list = list as u64;
    r.pso = pso as u64;
    r.root = s.root as u64;
    r.ib = s.ib;
    r.vb = s.vb;
    r.params = s.params;
    r.types = s.types;
    describe_root(s.root);
    r.node = node;
    r.serial = serial as u32;
    r.kind = kind;
    r.viewport_x = s.viewport_x;
    r.viewport_width = s.viewport_width;
    r.args = *args;
    r.sequence = sequence;
}
/// BeforeDraw: the single-pass feasibility substitution; the start instance to draw with.
///
/// # Safety
/// As cvr_pair_probe_draw.
#[no_mangle]
pub unsafe extern "C" fn cvr_pair_probe_before_draw(
    list: *mut c_void,
    node: u32,
    pso: *mut c_void,
    kind: u32,
    _args: *const [u32; 5],
    start_instance: u32,
) -> u32 {
    let mask = CyberpunkVR_PairSubstituteMask.load(Relaxed) as u64 | (CyberpunkVR_PairSubstituteMaskHigh.load(Relaxed) as u64) << 32;
    let instance = CyberpunkVR_PairSubstituteInstance.load(Relaxed) != 0;
    if (mask == 0 && !instance) || !cvr_pair_probe_tracking() || kind > 2 {
        return start_instance;
    }
    let wanted_node = CyberpunkVR_PairProbeNode.load(Relaxed);
    if wanted_node != 0 && node != wanted_node {
        return start_instance;
    }
    let counts = &*(&raw const CyberpunkVR_PairSubstituteCounts);
    let s = state_of(list);
    let eye = if s.viewport_x == 0 {
        0
    } else if s.viewport_x > 0 && s.viewport_x as u32 == s.viewport_width {
        1
    } else {
        -1
    };
    if eye == 0 {
        if s.last_eye != 0 {
            s.left.clear();
            s.right_index = 0;
        }
        s.left.push(Partner {
            pso: pso as u64,
            ib: s.ib,
            vb: s.vb,
            kind,
            start_instance,
            tables: s.tables,
            table_mask: s.table_mask,
        });
        s.last_eye = 0;
        counts[2].fetch_add(1, Relaxed);
        return start_instance;
    }
    if eye != 1 {
        s.last_eye = -1;
        return start_instance;
    }
    if s.last_eye != 1 {
        s.right_index = 0;
    }
    s.last_eye = 1;
    let k = s.right_index;
    s.right_index = s.right_index.wrapping_add(1);
    let set_table = originals_of(list).and_then(|o| o.table);
    let (Some(set_table), Some(p)) = (set_table, s.left.get(k as usize).cloned()) else {
        counts[1].fetch_add(1, Relaxed);
        return start_instance;
    };
    if p.pso != pso as u64 || p.ib != s.ib || p.vb != s.vb || p.kind != kind {
        counts[1].fetch_add(1, Relaxed);
        return start_instance;
    }
    let substitute = mask & p.table_mask & s.table_mask;
    for i in 0..PARAMS {
        if substitute >> i & 1 == 0 || p.tables[i] == s.tables[i] {
            continue;
        }
        set_table(list, i as u32, p.tables[i]);
        s.restore_mask |= 1u64 << i;
        counts[3].fetch_add(1, Relaxed);
    }
    counts[0].fetch_add(1, Relaxed);
    if instance {
        p.start_instance
    } else {
        start_instance
    }
}
/// AfterDraw: the engine's tables put back.
///
/// # Safety
/// `list` is a live command list.
#[no_mangle]
pub unsafe extern "C" fn cvr_pair_probe_after_draw(list: *mut c_void) {
    if !cvr_pair_probe_tracking() {
        return;
    }
    let s = state_of(list);
    if s.restore_mask == 0 {
        return;
    }
    let set_table = originals_of(list).and_then(|o| o.table);
    for i in 0..PARAMS {
        if s.restore_mask >> i & 1 != 0 {
            if let Some(f) = set_table {
                f(list, i as u32, s.tables[i]);
            }
        }
    }
    s.restore_mask = 0;
}
/// Poll: once per Present -- the frame window armed from a request, and closed after its frames.
#[no_mangle]
pub extern "C" fn cvr_pair_probe_poll() {
    // SAFETY: the C++ request and state.
    unsafe {
        let state = CyberpunkVR_PairProbeState.load(Relaxed);
        let mut window = WINDOW.lock().unwrap_or_else(|p| p.into_inner());
        if state == 2 {
            if CyberpunkVR_PresentTiming[0].load(Relaxed) > window.1 + 1 {
                CyberpunkVR_PairProbeState.store(3, SeqCst);
            }
            return;
        }
        let frames = CyberpunkVR_PairProbeRequest.load(Relaxed);
        if frames == 0 {
            return;
        }
        CyberpunkVR_PairProbeRequest.store(0, SeqCst);
        if !cvr_pair_probe_tracking() {
            CyberpunkVR_PairProbeState.store(4, SeqCst);
            return;
        }
        CyberpunkVR_PairProbeCount.store(0, SeqCst);
        let start = CyberpunkVR_PresentTiming[0].load(Relaxed);
        *window = (start, start + frames.min(8) as u64);
        CyberpunkVR_PairProbeState.store(2, SeqCst);
    }
}
