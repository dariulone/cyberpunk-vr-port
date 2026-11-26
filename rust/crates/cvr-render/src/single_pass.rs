//! SINGLE PASS: one view-instanced draw for both eyes of an engine-stereo RenderElements draw pair (formerly
//! src/Stereo/SinglePass.cpp; include/Stereo/SinglePass.hpp has the contract).
//!
//! Under engine stereo every RenderElements batch is drawn twice in one command list: the left eye's N draws (viewport
//! at x = 0), then the right eye's same N draws (viewport at x = its width), differing only in the camera tables (root
//! parameters 0 and 18). So a LEFT draw whose pipeline has a view-instanced twin (single_pass_pipelines) is kept, its
//! RIGHT partner is sent as ONE draw with the twin on the twin root -- the right eye's tables as the engine bound them,
//! the left eye's camera tables in parameters 36/37 -- and two viewports and scissors; anything else first sends every
//! kept left draw as it was (FLUSH: its own state, then the engine's state back). While left draws are kept, the
//! engine's state calls are held (CyberpunkVR_SinglePassLazy): the next draw gets the difference to what the list has.
//!
//! The command list methods that read or write what the draws produce are chained over the vtable (after the census'
//! own hooks): each flushes, then calls what the slot held. The occlusion culling prototype (CyberpunkVR_OcclusionCull,
//! off by default) wraps the view-instanced draws in binary occlusion queries and predicates draws found empty.
//!
//! src/Stereo/SinglePass.cpp keeps the exports, the C++ names as forwarders and the census' hook record lookup.
#![allow(unsafe_op_in_unsafe_fn)]
use cvr_host::fast::FastMap;
use crate::single_pass_pipelines as pipelines;
use core::cell::Cell;
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering::*};
use std::sync::{LazyLock, Mutex};
use windows::core::{Interface, HRESULT};
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D12::*;

#[allow(non_upper_case_globals)]
extern "C" {
    static CyberpunkVR_SinglePass: AtomicU32;
    static CyberpunkVR_SinglePassNode: AtomicU32;
    static CyberpunkVR_SinglePassCounts: [AtomicU64; 10];
    static CyberpunkVR_SinglePassLazy: AtomicU32;
    static CyberpunkVR_SinglePassFlushBy: [AtomicU64; 96];
    static CyberpunkVR_SinglePassMismatch: [AtomicU64; 12];
    static CyberpunkVR_OcclusionCull: AtomicU32;
    static CyberpunkVR_OcclusionCullRetest: AtomicU32;
    static CyberpunkVR_OcclusionCullCounts: [AtomicU64; 8];
    static CyberpunkVR_PresentTiming: [AtomicU64; 8];
    static CyberpunkVR_SinglePassPipelineCounts: [AtomicU64; 12];
    /// cvr::detail::command_list_hook_entry (src/Stereo/DeviceHooks.cpp): the census' record of a list's vtable.
    fn cvr_cpp_command_list_hook_entry(list: *mut c_void) -> *const Hook;
}
fn count(i: usize) -> &'static AtomicU64 {
    // SAFETY: an export of the C++ half.
    unsafe { &CyberpunkVR_SinglePassCounts[i] }
}
// THE COUNTS PER THREAD, added to the shared exports every 256 events of the thread (and before a report): every draw
// of every render worker incremented the same shared cache line -- the atomic increments were 1 ms a frame of the draw
// hook's own 2 ms (ETW stacks, RTX 5070 Ti). The exports lag by under 256 per thread.
thread_local! {
    static LOCAL: core::cell::RefCell<([u64; 10], [u64; 96], u32)> = const { core::cell::RefCell::new(([0; 10], [0; 96], 0)) };
}
fn flush_local(l: &mut ([u64; 10], [u64; 96], u32)) {
    for (i, v) in l.0.iter_mut().enumerate() {
        if *v != 0 {
            count(i).fetch_add(core::mem::take(v), Relaxed);
        }
    }
    for (i, v) in l.1.iter_mut().enumerate() {
        if *v != 0 {
            // SAFETY: an export of the C++ half.
            unsafe { CyberpunkVR_SinglePassFlushBy[i].fetch_add(core::mem::take(v), Relaxed) };
        }
    }
    l.2 = 0;
}
fn tally(f: impl FnOnce(&mut ([u64; 10], [u64; 96], u32))) {
    LOCAL.with(|l| {
        let mut l = l.borrow_mut();
        f(&mut l);
        l.2 += 1;
        if l.2 >= 256 {
            flush_local(&mut l);
        }
    });
}
/// count(i) += 1, through the thread's tally.
fn bump(i: usize) {
    tally(|l| l.0[i] += 1);
}
fn cause(i: usize) {
    tally(|l| l.1[i] += 1);
}
fn log(text: &str) {
    cvr_host::log::write(text);
}

const TABLES: usize = 36;
const ALL: usize = 38;
const VB: usize = 16;
const VIEWS: usize = D3D12_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE as usize;

// ---- the census' record of a list's originals (include/Stereo/StereoInternal.hpp CommandListVtableHook, its head)

type L = *mut c_void;
type RootFn = unsafe extern "system" fn(L, *mut c_void);
type TableFn = unsafe extern "system" fn(L, u32, D3D12_GPU_DESCRIPTOR_HANDLE);
type SetPsoFn = unsafe extern "system" fn(L, *mut c_void);
type IbFn = unsafe extern "system" fn(L, *const D3D12_INDEX_BUFFER_VIEW);
type VbFn = unsafe extern "system" fn(L, u32, u32, *const D3D12_VERTEX_BUFFER_VIEW);
type TopoFn = unsafe extern "system" fn(L, i32);
type StencilFn = unsafe extern "system" fn(L, u32);
type BlendFn = unsafe extern "system" fn(L, *const f32);
type RateFn = unsafe extern "system" fn(L, i32, *const i32);
type ImageFn = unsafe extern "system" fn(L, *mut c_void);
type ViewportsFn = unsafe extern "system" fn(L, u32, *const D3D12_VIEWPORT);
type ScissorFn = unsafe extern "system" fn(L, u32, *const RECT);
type DrawFn = unsafe extern "system" fn(L, u32, u32, u32, u32);
type DrawIdxFn = unsafe extern "system" fn(L, u32, u32, u32, i32, u32);
#[repr(C)]
pub(crate) struct Hook {
    vtable: usize,
    original: usize,
    barrier_original: usize,
    pub(crate) barrier_call: usize,
    pub(crate) copyres: usize,
    pub(crate) copytex: usize,
    indirect_original: usize,
    draw_original: Option<DrawFn>,
    drawidx_original: Option<DrawIdxFn>,
    pub(crate) cbr_original: usize,
    dispatch_original: usize,
    viewports_original: Option<ViewportsFn>,
    scissor_original: Option<ScissorFn>,
    reset_original: usize,
    setpso_original: Option<SetPsoFn>,
    iavb_original: Option<VbFn>,
    cleardsv_original: usize,
    crootsig_original: usize,
    crootdt_original: usize,
    groot_original: Option<RootFn>,
    grootdt_original: Option<TableFn>,
    grootcbv_original: usize,
    iaib_original: Option<IbFn>,
    predication_original: usize,
    heaps_original: usize,
    topology_original: Option<TopoFn>,
    stencil_original: Option<StencilFn>,
    shading_rate_original: Option<RateFn>,
    shading_image_original: Option<ImageFn>,
    shading_state_known: bool,
    close_original: usize,
    clear_state_original: usize,
    bundle_original: usize,
    discard_original: usize,
    blend_factor_original: Option<BlendFn>,
}
const _: () = assert!(
    core::mem::offset_of!(Hook, draw_original) == 56
        && core::mem::offset_of!(Hook, viewports_original) == 88
        && core::mem::offset_of!(Hook, setpso_original) == 112
        && core::mem::offset_of!(Hook, iavb_original) == 120
        && core::mem::offset_of!(Hook, groot_original) == 152
        && core::mem::offset_of!(Hook, grootdt_original) == 160
        && core::mem::offset_of!(Hook, iaib_original) == 176
        && core::mem::offset_of!(Hook, topology_original) == 200
        && core::mem::offset_of!(Hook, stencil_original) == 208
        && core::mem::offset_of!(Hook, shading_rate_original) == 216
        && core::mem::offset_of!(Hook, shading_image_original) == 224
        && core::mem::offset_of!(Hook, blend_factor_original) == 272
);
unsafe fn originals<'a>(list: L) -> Option<&'a Hook> {
    cvr_cpp_command_list_hook_entry(list).as_ref()
}

// ---- a list's state

/// What a draw reads besides its viewports.
#[derive(Clone, Copy)]
struct Input {
    root: usize,
    tables: [u64; ALL],
    /// Bit i: table i holds tables[i].
    mask: u64,
    /// The bound pipeline (for the engine's view: what its SetPipelineState bound).
    pso: usize,
    ib: D3D12_INDEX_BUFFER_VIEW,
    vb: [D3D12_VERTEX_BUFFER_VIEW; VB],
    /// Slots below this were set.
    vb_used: u32,
    topo: i32,
    // The per-draw output state the engine sets between draws (stencil reference per material, its VRS rate).
    stencil_known: bool,
    blend_known: bool,
    rate_known: bool,
    image_known: bool,
    stencil: u32,
    blend: [f32; 4],
    rate: i32,
    combiners: [i32; 2],
    image: usize,
}
impl Default for Input {
    fn default() -> Self {
        // SAFETY: plain data; all zeros is the C++ `{}`.
        unsafe { core::mem::zeroed() }
    }
}
#[derive(Clone, Copy)]
struct Gfx {
    i: Input,
    vp_count: u32,
    vp: [D3D12_VIEWPORT; VIEWS],
    sc_count: u32,
    sc: [RECT; VIEWS],
}
impl Default for Gfx {
    fn default() -> Self {
        // SAFETY: plain data.
        unsafe { core::mem::zeroed() }
    }
}
#[derive(Clone, Copy)]
struct Pending {
    engine: usize,
    twin: usize,
    /// The engine's state at the left draw.
    input: Input,
    vp: D3D12_VIEWPORT,
    sc: RECT,
    kind: u32,
    args: [u32; 5],
    sent: bool,
}
impl Default for Pending {
    fn default() -> Self {
        // SAFETY: plain data.
        unsafe { core::mem::zeroed() }
    }
}
#[derive(Default)]
struct List {
    /// What the engine set, what the list has.
    eng: Gfx,
    act: Gfx,
    /// act may differ from eng.
    dirty: bool,
    /// The engine's own pipeline.
    eng_pso: usize,
    rec: usize,
    rec_pso: usize,
    pending: Vec<Pending>,
    cursor: usize,
    unsent: usize,
    /// 1 left batch, 2 right batch.
    phase: i32,
    /// The render-graph node of the batch.
    node: u32,
    broken: bool,
    // The bound render targets (an eye's batch binds the same ones again: nothing the kept draws need changes).
    targets_known: bool,
    target_count: u32,
    targets_contiguous: i32,
    targets: [usize; 8],
    depth_target: usize,
}

/// Bytes compared as memcmp does.
fn same<T>(a: &T, b: &T) -> bool {
    // SAFETY: plain-data views of two values of one padding-free type.
    unsafe {
        core::slice::from_raw_parts(a as *const T as *const u8, core::mem::size_of::<T>())
            == core::slice::from_raw_parts(b as *const T as *const u8, core::mem::size_of::<T>())
    }
}
fn same_n<T>(a: &[T], b: &[T], n: usize) -> bool {
    // SAFETY: as `same`, over the first n elements.
    unsafe {
        core::slice::from_raw_parts(a.as_ptr() as *const u8, core::mem::size_of::<T>() * n)
            == core::slice::from_raw_parts(b.as_ptr() as *const u8, core::mem::size_of::<T>() * n)
    }
}

struct Lists(FastMap<usize, Box<List>>);
// SAFETY: a list's record is used by the thread recording that list.
unsafe impl Send for Lists {}
static LISTS: LazyLock<Mutex<Lists>> = LazyLock::new(|| Mutex::new(Lists(cvr_host::fast::fast_map())));
thread_local! {
    static CURRENT: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
}
/// The list's record (made at its first use); the last one is remembered per thread.
#[allow(clippy::mut_from_ref)]
fn of<'a>(list: L) -> &'a mut List {
    let (cached, state) = CURRENT.with(|c| c.get());
    if cached == list as usize && state != 0 {
        // SAFETY: a boxed record that lives for the process, used by the thread recording the list.
        return unsafe { &mut *(state as *mut List) };
    }
    let mut lists = LISTS.lock().unwrap_or_else(|e| e.into_inner());
    let slot = lists.0.entry(list as usize).or_default();
    let p: *mut List = &mut **slot;
    CURRENT.with(|c| c.set((list as usize, p as usize)));
    // SAFETY: as above.
    unsafe { &mut *p }
}
fn hold(l: &List) -> bool {
    // SAFETY: exports of the C++ half.
    l.unsent != 0
        && unsafe {
            CyberpunkVR_SinglePassLazy.load(Relaxed) != 0
                && CyberpunkVR_SinglePass.load(Relaxed) != 0
        }
}
static ACTIVE: AtomicBool = AtomicBool::new(false);
fn active() -> bool {
    ACTIVE.load(Relaxed)
}

// ---- applying state

unsafe fn apply_root(
    list: L,
    e: &Hook,
    l: &mut List,
    root: usize,
    tables: &[u64; ALL],
    mask: u64,
    n: usize,
) {
    if l.act.i.root != root {
        if let Some(f) = e.groot_original {
            f(list, root as *mut c_void);
        }
        l.act.i.root = root;
        l.act.i.mask = 0;
    }
    for i in 0..n {
        if (mask >> i) & 1 == 0 {
            continue;
        }
        if (l.act.i.mask >> i) & 1 != 0 && l.act.i.tables[i] == tables[i] {
            continue;
        }
        if let Some(f) = e.grootdt_original {
            f(
                list,
                i as u32,
                D3D12_GPU_DESCRIPTOR_HANDLE { ptr: tables[i] },
            );
        }
        l.act.i.tables[i] = tables[i];
        l.act.i.mask |= 1u64 << i;
    }
}
unsafe fn apply_input(list: L, e: &Hook, l: &mut List, t: &Input) {
    if t.pso != 0 && l.act.i.pso != t.pso {
        if let Some(f) = e.setpso_original {
            f(list, t.pso as *mut c_void);
        }
        l.act.i.pso = t.pso;
    }
    if !same(&l.act.i.ib, &t.ib) {
        if let Some(f) = e.iaib_original {
            f(list, &t.ib);
        }
        l.act.i.ib = t.ib;
    }
    let n = l.act.i.vb_used.max(t.vb_used) as usize;
    if n != 0 && !same_n(&l.act.i.vb, &t.vb, n) {
        if let Some(f) = e.iavb_original {
            f(list, 0, n as u32, t.vb.as_ptr());
        }
        l.act.i.vb[..n].copy_from_slice(&t.vb[..n]);
        l.act.i.vb_used = n as u32;
    }
    if l.act.i.topo != t.topo && t.topo != 0 {
        if let Some(f) = e.topology_original {
            f(list, t.topo);
        }
        l.act.i.topo = t.topo;
    }
    if t.stencil_known && (!l.act.i.stencil_known || l.act.i.stencil != t.stencil) {
        if let Some(f) = e.stencil_original {
            f(list, t.stencil);
            l.act.i.stencil = t.stencil;
            l.act.i.stencil_known = true;
        }
    }
    if t.blend_known && (!l.act.i.blend_known || !same(&l.act.i.blend, &t.blend)) {
        if let Some(f) = e.blend_factor_original {
            f(list, t.blend.as_ptr());
            l.act.i.blend = t.blend;
            l.act.i.blend_known = true;
        }
    }
    if t.rate_known
        && (!l.act.i.rate_known || l.act.i.rate != t.rate || l.act.i.combiners != t.combiners)
    {
        if let Some(f) = e.shading_rate_original {
            f(list, t.rate, t.combiners.as_ptr());
            l.act.i.rate = t.rate;
            l.act.i.combiners = t.combiners;
            l.act.i.rate_known = true;
        }
    }
    if t.image_known && (!l.act.i.image_known || l.act.i.image != t.image) {
        if let Some(f) = e.shading_image_original {
            f(list, t.image as *mut c_void);
            l.act.i.image = t.image;
            l.act.i.image_known = true;
        }
    }
}
unsafe fn apply_views(list: L, e: &Hook, l: &mut List, vp: &[D3D12_VIEWPORT], sc: &[RECT]) {
    let (vn, sn) = (vp.len(), sc.len());
    if vn != 0 && (l.act.vp_count as usize != vn || !same_n(&l.act.vp, vp, vn)) {
        if let Some(f) = e.viewports_original {
            f(list, vn as u32, vp.as_ptr());
        }
        l.act.vp_count = vn as u32;
        l.act.vp[..vn].copy_from_slice(vp);
    }
    if sn != 0 && (l.act.sc_count as usize != sn || !same_n(&l.act.sc, sc, sn)) {
        if let Some(f) = e.scissor_original {
            f(list, sn as u32, sc.as_ptr());
        }
        l.act.sc_count = sn as u32;
        l.act.sc[..sn].copy_from_slice(sc);
    }
}
/// The engine's state back on the list.
unsafe fn restore(list: L, l: &mut List) {
    if !l.dirty {
        return;
    }
    let Some(e) = originals(list) else { return };
    let eng = l.eng;
    if eng.i.root != 0 {
        apply_root(list, e, l, eng.i.root, &eng.i.tables, eng.i.mask, TABLES);
    }
    apply_input(list, e, l, &eng.i);
    apply_views(
        list,
        e,
        l,
        &eng.vp[..eng.vp_count as usize],
        &eng.sc[..eng.sc_count as usize],
    );
    l.dirty = false;
}
unsafe fn send(list: L, e: &Hook, kind: u32, a: &[u32; 5]) {
    if kind == 2 {
        if let Some(f) = e.drawidx_original {
            f(list, a[0], a[1], a[2], a[3] as i32, a[4]);
        }
    } else if let Some(f) = e.draw_original {
        f(list, a[0], a[1], a[2], a[3]);
    }
}
/// A kept left draw, as the engine would have sent it.
unsafe fn emit(list: L, e: &Hook, l: &mut List, index: usize) {
    l.dirty = true;
    let p = l.pending[index];
    apply_root(
        list,
        e,
        l,
        p.input.root,
        &p.input.tables,
        p.input.mask,
        TABLES,
    );
    apply_input(list, e, l, &p.input);
    apply_views(list, e, l, &[p.vp], &[p.sc]);
    send(list, e, p.kind, &p.args);
    l.pending[index].sent = true;
    l.unsent -= 1;
    bump(2);
}
unsafe fn flush_list(list: L, l: &mut List) {
    if l.unsent != 0 {
        if let Some(e) = originals(list) {
            for i in 0..l.pending.len() {
                if !l.pending[i].sent {
                    emit(list, e, l, i);
                }
            }
            bump(5);
        }
        l.unsent = 0;
    }
    restore(list, l);
}
/// Both eyes in one draw: the right eye's state as the engine bound it, the left eye's camera tables in 36/37.
unsafe fn instanced(list: L, e: &Hook, l: &mut List, index: usize, kind: u32, args: &[u32; 5]) {
    let p = l.pending[index];
    let twin_root = pipelines::cvr_single_pass_twin_root(l.eng.i.root as *mut c_void) as usize;
    l.dirty = true;
    let mut tables = [0u64; ALL];
    tables[..TABLES].copy_from_slice(&l.eng.i.tables[..TABLES]);
    tables[36] = p.input.tables[0];
    tables[37] = p.input.tables[18];
    if l.act.i.root != twin_root {
        bump(9);
    }
    let mask = l.eng.i.mask | (3u64 << 36);
    apply_root(list, e, l, twin_root, &tables, mask, ALL);
    let mut input = l.eng.i;
    input.pso = p.twin;
    apply_input(list, e, l, &input);
    let vp = [p.vp, l.eng.vp[0]];
    let sc = [p.sc, l.eng.sc[0]];
    apply_views(list, e, l, &vp, &sc);
    occ::draw(list, e, l, kind, args);
    l.pending[index].sent = true;
    l.unsent -= 1;
}
fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.5
}
fn mismatch(why: usize) -> bool {
    // SAFETY: an export of the C++ half.
    unsafe { CyberpunkVR_SinglePassMismatch[why].fetch_add(1, Relaxed) };
    false
}
unsafe fn matches(p: &Pending, l: &List, kind: u32, args: &[u32; 5]) -> bool {
    let a = &p.input;
    let b = &l.eng;
    if p.engine != l.eng_pso || a.pso != b.i.pso {
        return mismatch(0);
    }
    if p.kind != kind {
        return mismatch(1);
    }
    if a.root != b.i.root || a.mask != b.i.mask || a.topo != b.i.topo {
        return mismatch(2);
    }
    let instance_arg = if kind == 2 { 4 } else { 3 };
    for i in 0..5 {
        if i != instance_arg && p.args[i] != args[i] {
            return mismatch(1);
        }
    }
    // The index buffer of an indexed draw and the vertex buffers its input layout reads (other slots may hold anything
    // the batch bound before).
    if kind == 2 && !same(&a.ib, &b.i.ib) {
        return mismatch(3);
    }
    let slots = pipelines::cvr_single_pass_input_slots(l.rec as *const c_void);
    let none = D3D12_VERTEX_BUFFER_VIEW::default();
    for i in 0..VB {
        if (slots >> i) & 1 == 0 {
            continue;
        }
        let x = if (i as u32) < a.vb_used {
            &a.vb[i]
        } else {
            &none
        };
        let y = if (i as u32) < b.i.vb_used {
            &b.i.vb[i]
        } else {
            &none
        };
        if !same(x, y) {
            return mismatch(3);
        }
    }
    if a.stencil_known != b.i.stencil_known || a.stencil != b.i.stencil {
        return mismatch(4);
    }
    if a.blend_known != b.i.blend_known || !same(&a.blend, &b.i.blend) {
        return mismatch(8);
    }
    if a.rate_known != b.i.rate_known || a.rate != b.i.rate || a.combiners != b.i.combiners {
        return mismatch(9);
    }
    if a.image_known != b.i.image_known || a.image != b.i.image {
        return mismatch(10);
    }
    let (u, v) = (&p.vp, &b.vp[0]);
    if !near(u.Width, v.Width)
        || !near(u.Height, v.Height)
        || !near(u.TopLeftY, v.TopLeftY)
        || u.MinDepth != v.MinDepth
        || u.MaxDepth != v.MaxDepth
        || !near(v.TopLeftX, u.TopLeftX + u.Width)
    {
        return mismatch(5);
    }
    let (c, d) = (&p.sc, &b.sc[0]);
    if b.sc_count != 1
        || c.top != d.top
        || c.bottom != d.bottom
        || c.right - c.left != d.right - d.left
    {
        return mismatch(6);
    }
    if a.mask & 1 != 0 && (a.mask >> 18) & 1 != 0 {
        true
    } else {
        mismatch(7)
    }
}
fn eye_of(l: &List) -> i32 {
    if l.eng.vp_count != 1 || l.eng.sc_count != 1 {
        return -1;
    }
    let v = &l.eng.vp[0];
    if v.Width <= 0.0 {
        return -1;
    }
    if near(v.TopLeftX, 0.0) {
        return 0;
    }
    if near(v.TopLeftX, v.Width) {
        return 1;
    }
    -1
}

// ---- the chain: detours over whatever the vtable had

const SLOTS: usize = 81;
struct Chain {
    vtable: AtomicUsize,
    next: [AtomicUsize; SLOTS],
}
static CHAINS: [Chain; 16] = [const {
    Chain {
        vtable: AtomicUsize::new(0),
        next: [const { AtomicUsize::new(0) }; SLOTS],
    }
}; 16];
static CHAIN_COUNT: AtomicU32 = AtomicU32::new(0);
static CHAIN_MUTEX: Mutex<()> = Mutex::new(());
unsafe fn chain_of(this: L) -> Option<&'static Chain> {
    let vtable = *(this as *const usize);
    let n = CHAIN_COUNT.load(Acquire) as usize;
    CHAINS[..n]
        .iter()
        .find(|c| c.vtable.load(Relaxed) == vtable)
}
unsafe fn next(this: L, slot: usize) -> usize {
    chain_of(this).map_or(0, |c| c.next[slot].load(Relaxed))
}

// ---- the occlusion culling prototype (CyberpunkVR_OcclusionCull)

mod occ {
    use super::*;
    /// Query indices: two halves, even and odd frames.
    const CAPACITY: u32 = 1 << 17;
    const HALF: u32 = CAPACITY / 2;
    /// A list takes its indices in runs of 256: one resolve per run.
    const CHUNK: u32 = 256;
    /// Draws known (slots).
    const DRAWS: usize = 1 << 16;
    fn stat(i: usize) -> &'static AtomicU64 {
        // SAFETY: an export of the C++ half.
        unsafe { &CyberpunkVR_OcclusionCullCounts[i] }
    }
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
    struct Key {
        pso: u64,
        ib: u64,
        vb: u64,
        args: [u32; 4],
        kind: u32,
        k: u32,
    }
    #[derive(Clone, Copy)]
    struct Chunk {
        frame: u64,
        next: u32,
        end: u32,
    }
    impl Default for Chunk {
        fn default() -> Self {
            Chunk {
                frame: !0,
                next: 0,
                end: 0,
            }
        }
    }
    #[derive(Default)]
    struct State {
        heap: Option<ID3D12QueryHeap>,
        results: Option<ID3D12Resource>,
        failed: bool,
        slots: FastMap<Key, u32>,
        seen: FastMap<Key, u32>,
        frame: u64,
        last_use: Vec<u64>,
        streak: Vec<u8>,
        last_index: Vec<u32>,
        /// The next free index of this frame's half.
        frame_next: u32,
        chunks: FastMap<usize, Chunk>,
        /// Query indices a list ended (resolved at its Close).
        used: FastMap<usize, Vec<u32>>,
    }
    // SAFETY: the COM objects are free-threaded; the state is used under its mutex.
    unsafe impl Send for State {}
    static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| {
        Mutex::new(State {
            frame: !0,
            ..Default::default()
        })
    });

    /// The query heap and the buffer the results are resolved into and predicated from (kept in PREDICATION between
    /// resolves). Made once, never released.
    unsafe fn ready(s: &mut State, list: L) -> bool {
        if s.heap.is_some() {
            return true;
        }
        if s.failed {
            return false;
        }
        let made = (|| -> Option<(ID3D12QueryHeap, ID3D12Resource)> {
            let list = ID3D12GraphicsCommandList::from_raw_borrowed(&list)?;
            let mut device: Option<ID3D12Device> = None;
            list.GetDevice(&mut device).ok()?;
            let device = device?;
            let query = D3D12_QUERY_HEAP_DESC {
                Type: D3D12_QUERY_HEAP_TYPE_OCCLUSION,
                Count: CAPACITY,
                NodeMask: 0,
            };
            let mut heap: Option<ID3D12QueryHeap> = None;
            device.CreateQueryHeap(&query, &mut heap).ok()?;
            let properties = D3D12_HEAP_PROPERTIES {
                Type: D3D12_HEAP_TYPE_DEFAULT,
                ..Default::default()
            };
            let desc = D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
                Width: CAPACITY as u64 * 8,
                Height: 1,
                DepthOrArraySize: 1,
                MipLevels: 1,
                SampleDesc: windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
                ..Default::default()
            };
            let mut results: Option<ID3D12Resource> = None;
            device
                .CreateCommittedResource(
                    &properties,
                    D3D12_HEAP_FLAG_NONE,
                    &desc,
                    D3D12_RESOURCE_STATE_PREDICATION,
                    None,
                    &mut results,
                )
                .ok()?;
            Some((heap?, results?))
        })();
        let Some((heap, results)) = made else {
            s.failed = true;
            stat(5).fetch_add(1, Relaxed);
            log("[occlusion] query heap / predication buffer not created: prototype off\n");
            return false;
        };
        s.heap = Some(heap);
        s.results = Some(results);
        s.last_use = vec![!0u64; DRAWS];
        s.streak = vec![0u8; DRAWS];
        s.last_index = vec![0u32; DRAWS];
        log(&format!(
            "[occlusion] prototype on: {CAPACITY} query slots\n"
        ));
        true
    }
    type QueryFn = unsafe extern "system" fn(L, *mut c_void, i32, u32);
    type PredicateFn = unsafe extern "system" fn(L, *mut c_void, u64, i32);
    type ResolveFn = unsafe extern "system" fn(L, *mut c_void, i32, u32, u32, *mut c_void, u64);
    type BarrierFn = unsafe extern "system" fn(L, u32, *const D3D12_RESOURCE_BARRIER);
    pub(super) unsafe fn draw(list: L, e: &Hook, l: &List, kind: u32, a: &[u32; 5]) {
        let mode = CyberpunkVR_OcclusionCull.load(Relaxed);
        let chain = if mode != 0 { chain_of(list) } else { None };
        let Some(chain) = chain else {
            send(list, e, kind, a);
            return;
        };
        let (begin, end, predication) = (
            chain.next[52].load(Relaxed),
            chain.next[53].load(Relaxed),
            chain.next[55].load(Relaxed),
        );
        if begin == 0 || end == 0 || predication == 0 {
            send(list, e, kind, a);
            return;
        }
        let (query, previous, predicate, heap, results);
        {
            let mut guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
            let s = &mut *guard;
            if !ready(s, list) {
                drop(guard);
                send(list, e, kind, a);
                return;
            }
            let now = CyberpunkVR_PresentTiming[0].load(Relaxed);
            if now != s.frame {
                s.frame = now;
                s.seen.clear();
                s.frame_next = 0;
            }
            // This draw's query index: from the list's run in this frame's half.
            let frame_next = s.frame_next;
            let chunk = s.chunks.entry(list as usize).or_default();
            if chunk.frame != now || chunk.next == chunk.end {
                if frame_next + CHUNK > HALF {
                    drop(guard);
                    send(list, e, kind, a);
                    return;
                }
                chunk.frame = now;
                chunk.next = frame_next;
                chunk.end = frame_next + CHUNK;
                s.frame_next = frame_next + CHUNK;
            }
            let chunk = s
                .chunks
                .get_mut(&(list as usize))
                .unwrap_or_else(|| unreachable!());
            let q = (now & 1) as u32 * HALF + chunk.next;
            chunk.next += 1;
            let mut key = Key {
                pso: l.act.i.pso as u64,
                ib: if kind == 2 {
                    l.act.i.ib.BufferLocation
                } else {
                    0
                },
                vb: if l.act.i.vb_used != 0 {
                    l.act.i.vb[0].BufferLocation
                } else {
                    0
                },
                kind,
                ..Default::default()
            };
            key.args[..3].copy_from_slice(&a[..3]);
            // Not the instance offset: the engine's per-frame instance data moves.
            key.args[3] = if kind == 2 { a[3] } else { 0 };
            let seen = s.seen.entry(key).or_insert(0);
            key.k = *seen;
            *seen += 1;
            let slot = match s.slots.get(&key) {
                Some(&slot) => slot,
                None => {
                    if s.slots.len() >= DRAWS {
                        if let Some(c) = s.chunks.get_mut(&(list as usize)) {
                            c.next -= 1;
                        }
                        drop(guard);
                        send(list, e, kind, a);
                        return;
                    }
                    let slot = s.slots.len() as u32;
                    s.slots.insert(key, slot);
                    stat(2).fetch_add(1, Relaxed);
                    slot
                }
            } as usize;
            let last_frame = s.last_use[slot].wrapping_add(1) == now;
            if s.last_use[slot] != now {
                s.streak[slot] = if last_frame {
                    (s.streak[slot] as u32 + 1).min(255) as u8
                } else {
                    1
                };
                s.last_use[slot] = now;
            }
            let retest = CyberpunkVR_OcclusionCullRetest.load(Relaxed).max(1) as u64;
            // Drawn the frames before this one (its last result is resolved), and not a frame that tests everything
            // again.
            predicate = mode == 1 && last_frame && s.streak[slot] >= 3 && now % retest != 0;
            previous = s.last_index[slot];
            s.last_index[slot] = q;
            s.used.entry(list as usize).or_default().push(q);
            query = q;
            heap = s
                .heap
                .as_ref()
                .map_or(core::ptr::null_mut(), |h| h.as_raw());
            results = s
                .results
                .as_ref()
                .map_or(core::ptr::null_mut(), |r| r.as_raw());
        }
        let _internal = cvr_host::probe::Internal::new();
        let begin: QueryFn = core::mem::transmute(begin);
        let end: QueryFn = core::mem::transmute(end);
        let predication: PredicateFn = core::mem::transmute(predication);
        if predicate {
            predication(
                list,
                results,
                previous as u64 * 8,
                D3D12_PREDICATION_OP_EQUAL_ZERO.0,
            );
            stat(1).fetch_add(1, Relaxed);
        }
        begin(list, heap, D3D12_QUERY_TYPE_BINARY_OCCLUSION.0, query);
        send(list, e, kind, a);
        end(list, heap, D3D12_QUERY_TYPE_BINARY_OCCLUSION.0, query);
        if predicate {
            predication(
                list,
                core::ptr::null_mut(),
                0,
                D3D12_PREDICATION_OP_EQUAL_ZERO.0,
            );
        }
        stat(0).fetch_add(1, Relaxed);
    }
    /// At Close: this list's queries resolved into the predication buffer (outside any render pass), in runs of
    /// consecutive slots; the next frame's draws predicate on them.
    pub(super) unsafe fn close(list: L) {
        let (mut mine, heap, results) = {
            let mut s = STATE.lock().unwrap_or_else(|e| e.into_inner());
            let Some(used) = s.used.get_mut(&(list as usize)) else {
                return;
            };
            let mine = core::mem::take(used);
            (
                mine,
                s.heap
                    .as_ref()
                    .map_or(core::ptr::null_mut(), |h| h.as_raw()),
                s.results
                    .as_ref()
                    .map_or(core::ptr::null_mut(), |r| r.as_raw()),
            )
        };
        let Some(chain) = chain_of(list) else { return };
        let (barrier_fn, resolve_fn) = (chain.next[26].load(Relaxed), chain.next[54].load(Relaxed));
        if mine.is_empty() || results.is_null() || barrier_fn == 0 || resolve_fn == 0 {
            return;
        }
        mine.sort_unstable();
        mine.dedup();
        let _internal = cvr_host::probe::Internal::new();
        let barrier_fn: BarrierFn = core::mem::transmute(barrier_fn);
        let resolve_fn: ResolveFn = core::mem::transmute(resolve_fn);
        let transition =
            |before: D3D12_RESOURCE_STATES, after: D3D12_RESOURCE_STATES| D3D12_RESOURCE_BARRIER {
                Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
                Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
                Anonymous: D3D12_RESOURCE_BARRIER_0 {
                    Transition: core::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                        pResource: core::mem::transmute_copy(&results),
                        Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                        StateBefore: before,
                        StateAfter: after,
                    }),
                },
            };
        let to_copy = transition(
            D3D12_RESOURCE_STATE_PREDICATION,
            D3D12_RESOURCE_STATE_COPY_DEST,
        );
        barrier_fn(list, 1, &to_copy);
        let mut i = 0;
        while i < mine.len() {
            let mut j = i + 1;
            while j < mine.len() && mine[j] == mine[j - 1] + 1 {
                j += 1;
            }
            resolve_fn(
                list,
                heap,
                D3D12_QUERY_TYPE_BINARY_OCCLUSION.0,
                mine[i],
                (j - i) as u32,
                results,
                mine[i] as u64 * 8,
            );
            stat(3).fetch_add(1, Relaxed);
            i = j;
        }
        let back = transition(
            D3D12_RESOURCE_STATE_COPY_DEST,
            D3D12_RESOURCE_STATE_PREDICATION,
        );
        barrier_fn(list, 1, &back);
        stat(4).fetch_add(1, Relaxed);
    }
    pub(super) fn reset(list: L) {
        let mut s = STATE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(used) = s.used.get_mut(&(list as usize)) {
            if !used.is_empty() {
                stat(6).fetch_add(1, Relaxed);
                used.clear();
            }
        }
        s.chunks.remove(&(list as usize));
    }
}

/// Before a chained command: what is kept goes first (the slot counted as the cause).
unsafe fn flush_for(list: L, slot: usize) {
    if active() {
        let l = of(list);
        if l.unsent != 0 {
            cause(slot);
        }
        if l.unsent != 0 || l.dirty {
            flush_list(list, l);
        }
    }
    if slot == 9 {
        // Close: the occlusion prototype's queries resolved.
        occ::close(list);
    }
}
macro_rules! flushing {
    ($name:ident, $slot:expr, ($($a:ident: $t:ty),*) $(-> $r:ty)?) => {
        unsafe extern "system" fn $name(this: L $(, $a: $t)*) $(-> $r)? {
            flush_for(this, $slot);
            let f: unsafe extern "system" fn(L $(, $t)*) $(-> $r)? = core::mem::transmute(next(this, $slot));
            f(this $(, $a)*)
        }
    };
}
type P = *mut c_void;
flushing!(close_detour, 9, () -> HRESULT);
flushing!(dispatch_detour, 14, (a: u32, b: u32, c: u32));
flushing!(copy_buffer_region, 15, (a: P, b: u64, c: P, d: u64, e: u64));
flushing!(copy_texture_region, 16, (a: P, b: u32, c: u32, d: u32, e: P, f: P));
flushing!(copy_resource, 17, (a: P, b: P));
flushing!(copy_tiles, 18, (a: P, b: P, c: P, d: P, e: u64, f: i32));
flushing!(resolve_subresource, 19, (a: P, b: u32, c: P, d: u32, e: i32));
flushing!(resource_barrier, 26, (a: u32, b: P));
flushing!(execute_bundle, 27, (a: P));
flushing!(set_descriptor_heaps, 28, (a: u32, b: P));
flushing!(root_constant, 34, (a: u32, b: u32, c: u32));
flushing!(root_constants, 36, (a: u32, b: u32, c: P, d: u32));
flushing!(root_cbv, 38, (a: u32, b: u64));
flushing!(root_srv, 40, (a: u32, b: u64));
flushing!(root_uav, 42, (a: u32, b: u64));
flushing!(so_set_targets, 45, (a: u32, b: u32, c: P));
flushing!(clear_depth_stencil, 47, (a: usize, b: i32, c: f32, d: u8, e: u32, f: P));
flushing!(clear_render_target, 48, (a: usize, b: P, c: u32, d: P));
flushing!(clear_uav_uint, 49, (a: u64, b: usize, c: P, d: P, e: u32, f: P));
flushing!(clear_uav_float, 50, (a: u64, b: usize, c: P, d: P, e: u32, f: P));
flushing!(discard_resource, 51, (a: P, b: P));
flushing!(begin_query, 52, (a: P, b: i32, c: u32));
flushing!(end_query, 53, (a: P, b: i32, c: u32));
flushing!(resolve_query, 54, (a: P, b: i32, c: u32, d: u32, e: P, f: u64));
flushing!(set_predication, 55, (a: P, b: u64, c: i32));
flushing!(execute_indirect, 59, (a: P, b: u32, c: P, d: u64, e: P, f: u64));
flushing!(atomic_copy32, 60, (a: P, b: u64, c: P, d: u64, e: u32, f: P, g: P));
flushing!(atomic_copy64, 61, (a: P, b: u64, c: P, d: u64, e: u32, f: P, g: P));
flushing!(depth_bounds, 62, (a: f32, b: f32));
flushing!(sample_positions, 63, (a: u32, b: u32, c: P));
flushing!(resolve_region, 64, (a: P, b: u32, c: u32, d: u32, e: P, f: u32, g: P, h: i32, i: i32));
flushing!(view_instance_mask, 65, (a: u32));
flushing!(write_immediate, 66, (a: u32, b: P, c: P));
flushing!(begin_render_pass, 68, (a: u32, b: P, c: P, d: i32));
flushing!(end_render_pass, 69, ());
flushing!(initialize_meta, 70, (a: P, b: P, c: usize));
flushing!(execute_meta, 71, (a: P, b: P, c: usize));
flushing!(build_raytracing, 72, (a: P, b: u32, c: P));
flushing!(emit_raytracing, 73, (a: P, b: u32, c: P));
flushing!(copy_raytracing, 74, (a: u64, b: u64, c: i32));
flushing!(dispatch_rays, 76, (a: P));
flushing!(dispatch_mesh, 79, (a: u32, b: u32, c: u32));
flushing!(enhanced_barrier, 80, (a: u32, b: P));
/// The flushing detours by slot.
fn flush_slots() -> [(usize, usize); 43] {
    [
        (9, close_detour as *const () as usize),
        (14, dispatch_detour as *const () as usize),
        (15, copy_buffer_region as *const () as usize),
        (16, copy_texture_region as *const () as usize),
        (17, copy_resource as *const () as usize),
        (18, copy_tiles as *const () as usize),
        (19, resolve_subresource as *const () as usize),
        (26, resource_barrier as *const () as usize),
        (27, execute_bundle as *const () as usize),
        (28, set_descriptor_heaps as *const () as usize),
        (34, root_constant as *const () as usize),
        (36, root_constants as *const () as usize),
        (38, root_cbv as *const () as usize),
        (40, root_srv as *const () as usize),
        (42, root_uav as *const () as usize),
        (45, so_set_targets as *const () as usize),
        (47, clear_depth_stencil as *const () as usize),
        (48, clear_render_target as *const () as usize),
        (49, clear_uav_uint as *const () as usize),
        (50, clear_uav_float as *const () as usize),
        (51, discard_resource as *const () as usize),
        (52, begin_query as *const () as usize),
        (53, end_query as *const () as usize),
        (54, resolve_query as *const () as usize),
        (55, set_predication as *const () as usize),
        (59, execute_indirect as *const () as usize),
        (60, atomic_copy32 as *const () as usize),
        (61, atomic_copy64 as *const () as usize),
        (62, depth_bounds as *const () as usize),
        (63, sample_positions as *const () as usize),
        (64, resolve_region as *const () as usize),
        (65, view_instance_mask as *const () as usize),
        (66, write_immediate as *const () as usize),
        (68, begin_render_pass as *const () as usize),
        (69, end_render_pass as *const () as usize),
        (70, initialize_meta as *const () as usize),
        (71, execute_meta as *const () as usize),
        (72, build_raytracing as *const () as usize),
        (73, emit_raytracing as *const () as usize),
        (74, copy_raytracing as *const () as usize),
        (76, dispatch_rays as *const () as usize),
        (79, dispatch_mesh as *const () as usize),
        (80, enhanced_barrier as *const () as usize),
    ]
}

/// ClearState: a flush, then nothing of the old state holds.
unsafe extern "system" fn clear_state(this: L, pso: P) {
    if active() {
        let l = of(this);
        flush_list(this, l);
        l.eng = Gfx::default();
        l.act = Gfx::default();
        l.eng_pso = 0;
        l.dirty = false;
    }
    let f: unsafe extern "system" fn(L, P) = core::mem::transmute(next(this, 11));
    f(this, pso);
}
/// SetPipelineState1 (a ray-tracing state object) takes the pipeline slot.
unsafe extern "system" fn state_object(this: L, object: P) {
    if active() {
        let l = of(this);
        flush_list(this, l);
        l.eng.i.pso = 0;
        l.act.i.pso = 0;
        l.eng_pso = 0;
    }
    let f: unsafe extern "system" fn(L, P) = core::mem::transmute(next(this, 75));
    f(this, object);
}
unsafe extern "system" fn topology(this: L, t: i32) {
    if active() {
        let l = of(this);
        if hold(l) {
            l.eng.i.topo = t;
            l.dirty = true;
            return;
        }
        l.eng.i.topo = t;
        l.act.i.topo = t;
    }
    let f: unsafe extern "system" fn(L, i32) = core::mem::transmute(next(this, 20));
    f(this, t);
}
unsafe extern "system" fn root(this: L, root: P) {
    if active() {
        let l = of(this);
        let r = root as usize;
        if l.unsent != 0 && r != l.eng.i.root {
            // The kept draws need the material root.
            cause(30);
            flush_list(this, l);
        }
        // Setting the bound root again keeps its arguments; another root starts without any.
        if r != l.act.i.root {
            l.act.i.mask = 0;
            if r == l.eng.i.root && l.eng.i.mask != 0 {
                l.dirty = true;
            }
        }
        if r != l.eng.i.root {
            l.eng.i.mask = 0;
        }
        l.eng.i.root = r;
        l.act.i.root = r;
    }
    let f: unsafe extern "system" fn(L, P) = core::mem::transmute(next(this, 30));
    f(this, root);
}
unsafe extern "system" fn table(this: L, index: u32, handle: D3D12_GPU_DESCRIPTOR_HANDLE) {
    if active() && (index as usize) < TABLES {
        let l = of(this);
        let i = index as usize;
        l.eng.i.tables[i] = handle.ptr;
        l.eng.i.mask |= 1u64 << i;
        if hold(l) && l.act.i.root == l.eng.i.root {
            l.dirty = true;
            return;
        }
        l.act.i.tables[i] = handle.ptr;
        l.act.i.mask |= 1u64 << i;
    }
    let f: unsafe extern "system" fn(L, u32, D3D12_GPU_DESCRIPTOR_HANDLE) =
        core::mem::transmute(next(this, 32));
    f(this, index, handle);
}
unsafe extern "system" fn targets(
    this: L,
    count: u32,
    rt: *const usize,
    contiguous: i32,
    ds: *const usize,
) {
    if active() {
        let l = of(this);
        let n = if rt.is_null() { 0 } else { count.min(8) };
        let listed = if contiguous != 0 { n.min(1) } else { n };
        let depth = if ds.is_null() { 0 } else { *ds };
        let mut same = l.targets_known
            && count == l.target_count
            && contiguous == l.targets_contiguous
            && depth == l.depth_target;
        let mut i = 0;
        while same && i < listed {
            same = *rt.add(i as usize) == l.targets[i as usize];
            i += 1;
        }
        if !same {
            if l.unsent != 0 {
                cause(46);
            }
            if l.unsent != 0 || l.dirty {
                flush_list(this, l);
            }
        }
        l.targets_known = count <= 8;
        l.target_count = count;
        l.targets_contiguous = contiguous;
        l.depth_target = depth;
        for i in 0..listed as usize {
            l.targets[i] = *rt.add(i);
        }
    }
    let f: unsafe extern "system" fn(L, u32, *const usize, i32, *const usize) =
        core::mem::transmute(next(this, 46));
    f(this, count, rt, contiguous, ds);
}
unsafe extern "system" fn stencil_ref(this: L, value: u32) {
    if active() {
        let l = of(this);
        l.eng.i.stencil = value;
        l.eng.i.stencil_known = true;
        if hold(l) {
            l.dirty = true;
            return;
        }
        l.act.i.stencil = value;
        l.act.i.stencil_known = true;
    }
    let f: unsafe extern "system" fn(L, u32) = core::mem::transmute(next(this, 24));
    f(this, value);
}
unsafe extern "system" fn blend_factor(this: L, factor: *const f32) {
    let f: unsafe extern "system" fn(L, *const f32) = core::mem::transmute(next(this, 23));
    f(this, factor);
    if active() {
        let l = of(this);
        let v = if factor.is_null() {
            [1.0f32; 4]
        } else {
            *(factor as *const [f32; 4])
        };
        l.eng.i.blend = v;
        l.act.i.blend = v;
        l.eng.i.blend_known = true;
        l.act.i.blend_known = true;
    }
}
unsafe extern "system" fn shading_rate(this: L, rate: i32, combiners: *const i32) {
    let f: unsafe extern "system" fn(L, i32, *const i32) = core::mem::transmute(next(this, 77));
    f(this, rate, combiners);
    if active() {
        let l = of(this);
        let passthrough = D3D12_SHADING_RATE_COMBINER_PASSTHROUGH.0;
        let c = if combiners.is_null() {
            [passthrough; 2]
        } else {
            [*combiners, *combiners.add(1)]
        };
        l.eng.i.rate = rate;
        l.act.i.rate = rate;
        l.eng.i.combiners = c;
        l.act.i.combiners = c;
        l.eng.i.rate_known = true;
        l.act.i.rate_known = true;
    }
}
unsafe extern "system" fn shading_image(this: L, image: P) {
    let f: unsafe extern "system" fn(L, P) = core::mem::transmute(next(this, 78));
    f(this, image);
    if active() {
        let l = of(this);
        l.eng.i.image = image as usize;
        l.act.i.image = image as usize;
        l.eng.i.image_known = true;
        l.act.i.image_known = true;
    }
}
unsafe extern "system" fn index_buffer(this: L, view: *const D3D12_INDEX_BUFFER_VIEW) {
    if active() {
        let l = of(this);
        l.eng.i.ib = view.as_ref().copied().unwrap_or_default();
        if hold(l) {
            l.dirty = true;
            return;
        }
        l.act.i.ib = l.eng.i.ib;
    }
    let f: unsafe extern "system" fn(L, *const D3D12_INDEX_BUFFER_VIEW) =
        core::mem::transmute(next(this, 43));
    f(this, view);
}

// ---- the C++ names' targets

/// Install: chains the detours over slots 9..last_slot of a command-list vtable (Stereo/DeviceHooks.cpp, after its own
/// slots are written and while they are writable).
#[no_mangle]
pub unsafe extern "C" fn cvr_single_pass_install(vtable: *mut usize, last_slot: usize) {
    let _guard = CHAIN_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let n = CHAIN_COUNT.load(Relaxed) as usize;
    if CHAINS[..n]
        .iter()
        .any(|c| c.vtable.load(Relaxed) == vtable as usize)
        || n >= CHAINS.len()
    {
        return;
    }
    let c = &CHAINS[n];
    c.vtable.store(vtable as usize, Relaxed);
    for i in 9..=last_slot.min(SLOTS - 1) {
        c.next[i].store(*vtable.add(i), Relaxed);
    }
    CHAIN_COUNT.store(n as u32 + 1, Release);
    let put = |slot: usize, detour: usize| {
        if slot <= last_slot {
            (*(vtable.add(slot) as *const AtomicUsize)).swap(detour, SeqCst);
        }
    };
    for (slot, detour) in flush_slots() {
        put(slot, detour);
    }
    put(11, clear_state as *const () as usize);
    put(75, state_object as *const () as usize);
    put(20, topology as *const () as usize);
    put(30, root as *const () as usize);
    put(32, table as *const () as usize);
    put(43, index_buffer as *const () as usize);
    put(46, targets as *const () as usize);
    put(23, blend_factor as *const () as usize);
    put(24, stencil_ref as *const () as usize);
    put(77, shading_rate as *const () as usize);
    put(78, shading_image as *const () as usize);
    log(&format!(
        "[single-pass] command-list vtable {:016X} chained (slots 9..{last_slot})\n",
        vtable as usize
    ));
}
/// Activate: the first material root exists: from now on the lists' state is kept.
#[no_mangle]
pub extern "C" fn cvr_single_pass_activate() {
    ACTIVE.store(true, Release);
}
#[no_mangle]
pub unsafe extern "C" fn cvr_single_pass_reset(list: L) {
    occ::reset(list);
    if !active() {
        return;
    }
    let l = of(list);
    // Keep the allocation.
    let mut pending = core::mem::take(&mut l.pending);
    pending.clear();
    *l = List {
        pending,
        ..Default::default()
    };
}
#[no_mangle]
pub unsafe extern "C" fn cvr_single_pass_pipeline_set(list: L, engine: P, bound: P) -> bool {
    if !active() {
        return false;
    }
    let l = of(list);
    l.eng_pso = engine as usize;
    l.eng.i.pso = bound as usize;
    if hold(l) {
        l.dirty = true;
        return true;
    }
    l.act.i.pso = bound as usize;
    false
}
#[no_mangle]
pub unsafe extern "C" fn cvr_single_pass_viewports(
    list: L,
    count: u32,
    vp: *const D3D12_VIEWPORT,
) -> bool {
    if !active() {
        return false;
    }
    let l = of(list);
    let n = if vp.is_null() {
        0
    } else {
        (count as usize).min(VIEWS)
    };
    l.eng.vp_count = n as u32;
    if n != 0 {
        l.eng.vp[..n].copy_from_slice(core::slice::from_raw_parts(vp, n));
    }
    if hold(l) {
        l.dirty = true;
        return true;
    }
    l.act.vp_count = n as u32;
    if n != 0 {
        l.act.vp[..n].copy_from_slice(core::slice::from_raw_parts(vp, n));
    }
    false
}
#[no_mangle]
pub unsafe extern "C" fn cvr_single_pass_scissors(list: L, count: u32, sc: *const RECT) -> bool {
    if !active() {
        return false;
    }
    let l = of(list);
    let n = if sc.is_null() {
        0
    } else {
        (count as usize).min(VIEWS)
    };
    l.eng.sc_count = n as u32;
    if n != 0 {
        l.eng.sc[..n].copy_from_slice(core::slice::from_raw_parts(sc, n));
    }
    if hold(l) {
        l.dirty = true;
        return true;
    }
    l.act.sc_count = n as u32;
    if n != 0 {
        l.act.sc[..n].copy_from_slice(core::slice::from_raw_parts(sc, n));
    }
    false
}
#[no_mangle]
pub unsafe extern "C" fn cvr_single_pass_vertex_buffers(
    list: L,
    start: u32,
    count: u32,
    views: *const D3D12_VERTEX_BUFFER_VIEW,
) -> bool {
    if !active() {
        return false;
    }
    let l = of(list);
    let (start, count) = (start as usize, count as usize);
    for i in 0..count {
        if start + i >= VB {
            break;
        }
        l.eng.i.vb[start + i] = if views.is_null() {
            D3D12_VERTEX_BUFFER_VIEW::default()
        } else {
            *views.add(i)
        };
    }
    if count != 0 && start < VB {
        l.eng.i.vb_used = l.eng.i.vb_used.max((start + count).min(VB) as u32);
    }
    if hold(l) && start + count <= VB {
        l.dirty = true;
        return true;
    }
    for i in 0..count {
        if start + i >= VB {
            break;
        }
        l.act.i.vb[start + i] = l.eng.i.vb[start + i];
    }
    if count != 0 && start < VB {
        l.act.i.vb_used = l.act.i.vb_used.max((start + count).min(VB) as u32);
    }
    false
}
#[no_mangle]
pub unsafe extern "C" fn cvr_single_pass_flush(list: L) {
    if !active() {
        return;
    }
    let l = of(list);
    if l.unsent != 0 {
        cause(88);
    }
    if l.unsent != 0 || l.dirty {
        flush_list(list, l);
    }
}

/// Where kept draws went, now and then (every 4096th batch).
fn report() {
    LOCAL.with(|l| flush_local(&mut l.borrow_mut()));
    // SAFETY: exports of the C++ half.
    unsafe {
        let mut line = String::new();
        for i in 0..96 {
            if line.len() >= 640 {
                break;
            }
            let n = CyberpunkVR_SinglePassFlushBy[i].load(Relaxed);
            if n != 0 {
                line.push_str(&format!(" {i}:{n}"));
            }
        }
        let c = |i: usize| CyberpunkVR_SinglePassPipelineCounts[i].load(SeqCst);
        line.push_str(&format!(
            "; pipelines registered {} queued {} built {} failed {} ({} ms), unknown replacement {}, owned {} MB",
            c(1),
            c(2),
            c(3),
            c(4),
            c(10),
            c(11),
            c(9) >> 20
        ));
        line.push_str("; mismatch");
        for i in 0..12 {
            if line.len() >= 1150 {
                break;
            }
            line.push_str(&format!(
                " {}",
                CyberpunkVR_SinglePassMismatch[i].load(Relaxed)
            ));
        }
        if !cvr_host::log::verbose() {
            return;
        }
        log(&format!(
            "[single-pass] kept {}, instanced {}, flushed {}; flushes by{}\n",
            count(0).load(SeqCst),
            count(1).load(SeqCst),
            count(2).load(SeqCst),
            if line.is_empty() {
                " (none)"
            } else {
                line.as_str()
            }
        ));
    }
}

/// Draw: true when single pass took the draw (kept, or drawn for both eyes) and the caller must not send it; false
/// after the list's state is the engine's again. kind 1 DrawInstanced (vertices, instances, start vertex, start
/// instance), 2 DrawIndexedInstanced (indices, instances, start index, base vertex, start instance).
#[no_mangle]
pub unsafe extern "C" fn cvr_single_pass_draw(
    list: L,
    node: u32,
    kind: u32,
    args: *const [u32; 5],
) -> bool {
    if !active() {
        return false;
    }
    let args = &*args;
    let l = of(list);
    let mode = CyberpunkVR_SinglePass.load(Relaxed);
    let native = |l: &mut List, why: usize| -> bool {
        if l.unsent != 0 {
            cause(why);
        }
        if l.unsent != 0 || l.dirty {
            flush_list(list, l);
        }
        false
    };
    let wanted = CyberpunkVR_SinglePassNode.load(Relaxed);
    if mode == 0 || (wanted != 0 && node != wanted) || !(1..=2).contains(&kind) {
        if l.phase != 0 {
            l.phase = 0;
        }
        return native(l, 90);
    }
    // A batch never spans two nodes: whatever is kept goes before another node's draws.
    if node != l.node {
        if l.unsent != 0 || l.dirty {
            if l.unsent != 0 {
                cause(90);
            }
            flush_list(list, l);
        }
        l.phase = 0;
        l.node = node;
    }
    let eye = eye_of(l);
    if eye < 0 {
        bump(7);
        l.phase = 0;
        return native(l, 91);
    }
    let Some(e) = originals(list) else {
        return native(l, 89);
    };
    if e.groot_original.is_none()
        || e.grootdt_original.is_none()
        || e.iaib_original.is_none()
        || e.topology_original.is_none()
    {
        return native(l, 89);
    }
    if eye == 0 {
        if l.phase != 1 {
            if l.unsent != 0 {
                cause(95);
                flush_list(list, l);
            }
            l.pending.clear();
            l.cursor = 0;
            l.phase = 1;
            l.broken = false;
            if (count(8).fetch_add(1, Relaxed) + 1) % 4096 == 0 {
                report();
            }
        }
        if l.rec_pso != l.eng_pso {
            l.rec = pipelines::cvr_single_pass_find(l.eng_pso as *mut c_void) as usize;
            l.rec_pso = l.eng_pso;
        }
        // Every left draw of the material root is kept. One whose twin is not ready (yet) is sent just in front of its
        // partner: the left eye's draws keep their order either way. (Without DXC no twin can come: then nothing is
        // kept, the engine's draws go as they are.)
        let material = l.rec != 0
            && pipelines::cvr_single_pass_root_of(l.rec as *const c_void) as usize == l.eng.i.root
            && !pipelines::cvr_single_pass_twin_root(l.eng.i.root as *mut c_void).is_null()
            && l.eng.i.mask & 1 != 0
            && (l.eng.i.mask >> 18) & 1 != 0
            && (mode == 2 || pipelines::cvr_single_pass_available());
        if !material {
            if l.unsent != 0 {
                cause(92);
            }
            if l.unsent != 0 || l.dirty {
                flush_list(list, l);
            }
            // Its partner goes as it is.
            l.pending.push(Pending {
                engine: l.eng_pso,
                sent: true,
                kind,
                ..Default::default()
            });
            bump(4);
            return false;
        }
        let twin = if mode == 1 {
            pipelines::cvr_single_pass_twin(
                l.rec as *mut c_void,
                l.eng_pso as *mut c_void,
                l.eng.i.pso as *mut c_void,
            ) as usize
        } else {
            0
        };
        l.pending.push(Pending {
            engine: l.eng_pso,
            twin,
            input: l.eng.i,
            vp: l.eng.vp[0],
            sc: l.eng.sc[0],
            kind,
            args: *args,
            sent: false,
        });
        l.unsent += 1;
        bump(0);
        return true;
    }
    if l.phase == 1 {
        l.phase = 2;
        l.cursor = 0;
    }
    if l.phase != 2 || l.broken || l.cursor >= l.pending.len() {
        bump(3);
        return native(l, 93);
    }
    let index = l.cursor;
    l.cursor += 1;
    if l.pending[index].sent {
        bump(3);
        return native(l, 93);
    }
    // A partner that differs (its own vertex buffers, say): that left draw alone goes first, the next pairs go on.
    // Every earlier left draw was sent already, so the left eye's draws keep their order.
    let p = l.pending[index];
    if !matches(&p, l, kind, args) {
        bump(6);
        emit(list, e, l, index);
        restore(list, l);
        return false;
    }
    if mode == 2 || p.twin == 0 {
        // Replay test, or no twin: the kept left draw just before its partner.
        emit(list, e, l, index);
        restore(list, l);
        return false;
    }
    instanced(list, e, l, index, kind, args);
    bump(1);
    true
}
