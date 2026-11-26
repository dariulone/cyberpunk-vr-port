//! The draw, dispatch, copy and binding detours (formerly src/Stereo/CommandListCensus.cpp): hk_IASetVertexBuffers,
//! hk_SetComputeRootSignature, hk_SetComputeRootDescriptorTable, hk_SetPipelineState, hk_ClearDepthStencilView,
//! hk_DrawInstanced, hk_DrawIndexedInstanced (with the lens velocity and history mask redraws), hk_ExecuteIndirect,
//! hk_Dispatch (with the outline compute swap), probe_CopyResource, hk_/probe_CopyTextureRegion (with the lens source
//! copy), hk_CopyBufferRegion; the capture markers (and the game-facing wrapper vtable's under a capture layer), the
//! geometry cost probe's scissor, the shader census.

use super::*;
use core::cell::Cell;
use std::sync::Mutex;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;

extern "C" {
    fn cvr_census_light_note(num_bytes: u64, dst_size: u64, dst: *mut c_void, vrcam: bool);
    fn cvr_census_light_report();
    fn cvr_census_dispatch_note(rva: u32, x: u32, y: u32, z: u32, vrcam: bool);
    fn cvr_census_dispatch_report();
    fn cvr_census_light_content_note(src: *mut c_void, off: u64, n: u64, vrcam: bool);
    fn cvr_census_light_content_report();
    fn cvr_census_indirect_note(rva: u32, signature: *mut c_void, vrcam: bool);
    fn cvr_census_pso_ps(pso: *mut c_void) -> u64;
    fn cvr_census_draw_probe(pso: *mut c_void);
    fn cvr_census_sight_note(pso: *mut c_void, rva: u32, vrcam: bool) -> u64;
    fn cvr_census_filled_note(dst: *mut c_void, dst_off: u64, bytes: u64, cpu: *const u8);
    fn cvr_census_sight_axis_drain();
    fn cvr_census_sight_axis_readback(list: List, vrcam: bool, sinst: u32, va: u64);
    fn cvr_census_sight_axis_note(vrcam: bool, sinst: u32, inst_va: u64, inst_stride: u32);
    fn cvr_census_lut_src_bind(index: u32, handle: u64, side: i32, work: usize, base: usize) -> u64;
    fn cvr_census_draw_note(rva: u32, vrcam: bool);
    fn cvr_census_cascade_note(idx: u32, inst: u32, sinst: u32, vrcam: bool);
    fn cvr_census_cascade_report();
    fn cvr_census_cascade_draw_seen(work: usize, base: usize, side: i32);
    fn cvr_cl_volume_node_note(rva: u32, n: u32, vrcam: bool);
    fn cvr_dh_shader_blob(which: u32, size: *mut usize) -> *const u8;
    fn cvr_dh_vision_ready() -> bool;
}

use crate::declared_moved_exports as declared;
use crate::used_moved_exports as used;

/// RenderShadowCascade's work (include/Stereo/EngineRvas.hpp).
const CASCADE_NODE_RVA: u32 = 0x4E_DFE0;
/// ClearShadowCascades' work.
const CASCADE_CLEAR_NODE_RVA: u32 = 0x22A_3F80;
/// include/Stereo/StereoInternal.hpp.
const CLUSTERED_LIGHTS_CULL_RVA: u32 = 0x41_0F1C;
const RENDER_LIGHT_BUFFERS_RVA: u32 = 0x41_1B94;
const LIGHT_SNAP_MAX: u64 = 64 * 1024;
/// hidden_area::shaders::Kind.
const KIND_DISPATCH: u32 = 1;
const KIND_EXECUTE_INDIRECT: u32 = 2;
const KIND_DRAW: u32 = 3;
const KIND_DRAW_INDEXED: u32 = 4;
const KIND_DISPATCH_RAYS: u32 = 5;

thread_local! {
    /// t_current_pso: the engine's last pipeline on this recording thread.
    pub(super) static CURRENT_PSO: Cell<*mut c_void> = const { Cell::new(null_mut()) };
    /// t_vis_swap_node, t_vis_swap_ord.
    pub(super) static VIS_SWAP: Cell<(usize, i32)> = const { Cell::new((0, 0)) };
    /// t_inst_va, t_inst_stride: slot 7's instance stream.
    static INSTANCE: Cell<(u64, u32)> = const { Cell::new((0, 0)) };
    /// t_current_compute_rootsig.
    static COMPUTE_ROOT: Cell<*mut c_void> = const { Cell::new(null_mut()) };
    /// t_mark_real, t_mark_wrap.
    static MARK_REAL: Cell<Marker> = const { Cell::new(Marker::NONE) };
    static MARK_WRAP: Cell<Marker> = const { Cell::new(Marker::NONE) };
    /// t_geometry_cost_scissor.
    static COST_SCISSOR: Cell<Scissors> = const { Cell::new(Scissors { list: 0, count: 0, rects: [[0; 4]; 16] }) };
    /// t_geometry_cost_viewport_list, _x, _w.
    static COST_VIEWPORT: Cell<(usize, u32, u32)> = const { Cell::new((0, 0, 0)) };
}
/// g_last_compute_rootsig: the last compute root signature seen on any thread.
static LAST_COMPUTE_ROOT: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

fn current_pso() -> *mut c_void {
    CURRENT_PSO.with(Cell::get)
}

// ---- the shader census, the markers, the pair probe's draws ----

/// shader_census.
fn shader_census(ctx: &Context, kind: u32, x: u32, y: u32, z: u32) {
    // SAFETY: exported controls.
    if unsafe { get(addr_of!(used::CyberpunkVR_ProbeLayoutNode)) } == 0 && crate::shader_patch_exports::CyberpunkVR_ShaderCensus.load(Relaxed) == 0 {
        return;
    }
    let node = ctx.node();
    let pso = if kind == KIND_DISPATCH_RAYS { null_mut() } else { current_pso() };
    // SAFETY: plain arguments.
    unsafe {
        cvr_ssr_merge_probe_layout(node);
        cvr_shader_patches_command(node, ctx.side, pso, kind, x, y, z);
    }
}
/// census_dispatch_rays.
pub(super) fn census_dispatch_rays(desc: *const D3D12_DISPATCH_RAYS_DESC) {
    // SAFETY: the engine's description.
    if let Some(d) = unsafe { desc.as_ref() } {
        if !internal() {
            shader_census(&Context::read(), KIND_DISPATCH_RAYS, d.Width, d.Height, d.Depth);
        }
    }
}
#[derive(Clone, Copy)]
struct Marker {
    list: usize,
    node: u32,
    side: i32,
    cascade: i32,
}
impl Marker {
    const NONE: Marker = Marker { list: 0, node: u32::MAX, side: -2, cascade: -2 };
}
static SAID_MARKER: AtomicBool = AtomicBool::new(false);
/// GetModuleHandleExA(FROM_ADDRESS) and the file's base name, "(unknown)" when not found.
fn module_base_name(address: usize) -> String {
    use windows::Win32::System::LibraryLoader::{GetModuleFileNameA, GetModuleHandleExA, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT};
    let mut module = windows::Win32::Foundation::HMODULE::default();
    // SAFETY: an address lookup into a local handle.
    let found = address != 0
        && unsafe {
            GetModuleHandleExA(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, windows::core::PCSTR(address as *const u8), &mut module)
        }
        .is_ok()
        && !module.is_invalid();
    if !found {
        return "(unknown)".into();
    }
    let mut full = [0u8; 260];
    // SAFETY: a local buffer.
    let n = unsafe { GetModuleFileNameA(Some(module), &mut full) } as usize;
    if n == 0 {
        return "(unknown)".into();
    }
    let full = &full[..n];
    let name = full.iter().rposition(|&b| b == b'\\').map_or(full, |at| &full[at + 1..]);
    String::from_utf8_lossy(name).into_owned()
}
/// maybe_marker_in: a PIX marker naming the node, the view and the cascade when they change on the list.
fn marker_in(list: List, which: &'static std::thread::LocalKey<Cell<Marker>>, ctx: &Context) {
    // SAFETY: an exported control.
    if unsafe { get(addr_of!(used::CyberpunkVR_CaptureMarkers)) } == 0 || list.is_null() || ctx.base == 0 {
        return;
    }
    if ctx.work <= ctx.base {
        return;
    }
    let rva = (ctx.work - ctx.base) as u32;
    let previous = which.with(Cell::get);
    if list as usize == previous.list && rva == previous.node && ctx.side == previous.side && ctx.cascade == previous.cascade {
        return;
    }
    which.with(|m| m.set(Marker { list: list as usize, node: rva, side: ctx.side, cascade: ctx.cascade }));
    let side = match ctx.side {
        0 => "MAIN",
        1 => "VRCAM",
        _ => "neither-eye",
    };
    // SAFETY: the profiler's static names.
    let node = unsafe { CyberpunkVR_ProfNodeName(rva).as_ref() }.map(|p| unsafe { core::ffi::CStr::from_ptr(p) }.to_string_lossy().into_owned());
    let node = node.as_deref().unwrap_or("?");
    let mut label = if ctx.cascade >= 0 { format!("{side} | {node} [cascade {}]", ctx.cascade) } else { format!("{side} | {node}") };
    label.truncate(159);
    if !SAID_MARKER.swap(true, SeqCst) {
        // SAFETY: a live list's vtable and its first slot.
        let table = unsafe { *(list as *const usize) };
        let first = if table != 0 { unsafe { *(table as *const usize) } } else { 0 };
        log(&format!(
            "[cascmark] first marker: list={} vtable={} vtable[0] belongs to {} | label='{}'",
            pointer(list as usize),
            pointer(table),
            module_base_name(first),
            label
        ));
    }
    let mut text = label.into_bytes();
    text.push(0);
    // SAFETY: SetMarker (slot 56) through the list's vtable, under SEH as the .cpp's __try did.
    let set_marker = unsafe { *((*(list as *const usize)) as *const usize).add(56) };
    // SAFETY: the list, the metadata, the terminated label and its size.
    let done = unsafe { cvr_engine::code::guarded_call4(set_marker, list, 1usize as *mut c_void, text.as_mut_ptr().cast(), text.len() as *mut c_void) };
    if done.is_none() {
        return;
    }
    if diagnostics() {
        // SAFETY: an exported counter.
        unsafe { add(addr_of_mut!(used::CyberpunkVR_DebugCaptureMarkers), 1) };
    }
}
/// The game-facing wrapper vtable once a capture layer's was hooked (g_wrapper_vtable).
static WRAPPER_VTABLE: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
/// The wrapper vtable's originals: DrawInstanced, DrawIndexedInstanced, Dispatch, ClearDepthStencilView.
static WRAP: [core::sync::atomic::AtomicUsize; 4] = [const { core::sync::atomic::AtomicUsize::new(0) }; 4];
/// maybe_marker.
fn marker(list: List, ctx: &Context, raster: bool) {
    // SAFETY: the profiler's flag.
    if unsafe { (*cvr_scene_profile_active()).load(Relaxed) } {
        let node = ctx.node();
        let optics_view = crate::vrcam_config::cname_hash(b"vr_optics_feed");
        let side = if ctx.side >= 0 {
            ctx.side
        } else if ctx.active_view_known && ctx.active_view_key == optics_view {
            2
        } else {
            -1
        };
        let detail = if node == CASCADE_NODE_RVA {
            (ctx.cascade + 1) as u32
        } else if raster {
            // SAFETY: the engine's list.
            unsafe { cvr_scene_profile_draw_detail(list) }
        } else {
            0
        };
        // SAFETY: the engine's list.
        unsafe { cvr_scene_profile_mark(list, node, side, current_pso(), detail) };
    }
    if declared::CyberpunkVR_SinglePassTraceState.load(Relaxed) == 1 && ctx.base != 0 && ctx.work > ctx.base {
        // SAFETY: plain arguments.
        unsafe { cvr_sp_trace_list((ctx.work - ctx.base) as u32, ctx.side, list) };
    }
    // SAFETY: an exported control.
    if WRAPPER_VTABLE.load(Relaxed) != 0 || unsafe { get(addr_of!(used::CyberpunkVR_CaptureMarkers)) } == 0 {
        return;
    }
    marker_in(list, &MARK_REAL, ctx);
}
/// pair_draw.
fn pair_draw(list: List, ctx: &Context, kind: u32, args: [u32; 5]) {
    // SAFETY: plain query.
    if !unsafe { cvr_pair_probe_tracking() } || internal() {
        return;
    }
    // SAFETY: the engine's list and the call's arguments.
    unsafe { cvr_pair_probe_draw(list, ctx.node(), current_pso(), kind, &args) };
}

// ---- the geometry cost probe ----

#[derive(Clone, Copy)]
struct Scissors {
    list: usize,
    count: u32,
    rects: [[i32; 4]; 16],
}
const COST_ACTION_MASK: u32 = 3;
const COST_SIDE0: u32 = 0x10;
const COST_SIDE1: u32 = 0x20;
const COST_LEFT_HALF: u32 = 0x40;
const COST_RIGHT_HALF: u32 = 0x80;
const COST_KNOWN: u32 = COST_ACTION_MASK | COST_SIDE0 | COST_SIDE1 | COST_LEFT_HALF | COST_RIGHT_HALF;
const RENDER_ELEMENTS_NODE: u32 = 0x33_DDA4;
#[derive(Clone, Copy, PartialEq, Eq)]
enum CostAction {
    Forward,
    EmptyScissor,
    Skip,
}
/// geometry_cost::EyeAllows.
fn eye_allows(control: u32, known: bool, x: u32, width: u32) -> bool {
    if control & (COST_LEFT_HALF | COST_RIGHT_HALF) == 0 {
        return true;
    }
    if !known {
        return false;
    }
    let right = x > 0 && (if x > width { x - width } else { width - x }) <= 1;
    let left = x == 0;
    (control & COST_LEFT_HALF != 0 && left) || (control & COST_RIGHT_HALF != 0 && right)
}
/// geometry_cost::Decide.
fn decide(control: u32, filter: u32, node: u32, side: i32) -> CostAction {
    if control == 0 || control & !COST_KNOWN != 0 {
        return CostAction::Forward;
    }
    let action = control & COST_ACTION_MASK;
    if action == 0 || action == 3 || (side != 0 && side != 1) || control & (if side != 0 { COST_SIDE1 } else { COST_SIDE0 }) == 0 {
        return CostAction::Forward;
    }
    let wanted = if filter != 0 { filter } else { RENDER_ELEMENTS_NODE };
    if node == 0 || node != wanted {
        return CostAction::Forward;
    }
    if action == 1 {
        CostAction::EmptyScissor
    } else {
        CostAction::Skip
    }
}
/// CyberpunkVR_GeometryCostTrackViewport (Dlss.cpp's RSSetViewports).
///
/// # Safety
/// `viewports` holds `count` viewports or is null.
#[no_mangle]
pub unsafe extern "C" fn CyberpunkVR_GeometryCostTrackViewport(list: List, count: u32, viewports: *const D3D12_VIEWPORT) {
    if used::CyberpunkVR_GeometryCostProbe.load(Relaxed) == 0 {
        return;
    }
    if count == 0 || viewports.is_null() {
        COST_VIEWPORT.with(|v| v.set((0, 0, 0)));
        return;
    }
    // SAFETY: the caller's first viewport.
    let first = unsafe { &*viewports };
    COST_VIEWPORT.with(|v| v.set((list as usize, (first.TopLeftX + 0.5) as u32, (first.Width + 0.5) as u32)));
}
/// CyberpunkVR_GeometryCostTrackScissor (Dlss.cpp's RSSetScissorRects).
///
/// # Safety
/// `rects` holds `count` rectangles or is null.
#[no_mangle]
pub unsafe extern "C" fn CyberpunkVR_GeometryCostTrackScissor(list: List, count: u32, rects: *const [i32; 4]) {
    if used::CyberpunkVR_GeometryCostProbe.load(Relaxed) == 0 {
        return;
    }
    COST_SCISSOR.with(|s| {
        let mut state = s.get();
        if !rects.is_null() && count != 0 {
            state.list = list as usize;
            state.count = count.min(16);
            for i in 0..state.count as usize {
                // SAFETY: the caller's rectangles.
                state.rects[i] = unsafe { *rects.add(i) };
            }
        } else if state.list == list as usize {
            state.list = 0;
            state.count = 0;
        }
        s.set(state);
    });
}
/// geometry_cost_action.
fn cost_action(list: List, ctx: &Context) -> CostAction {
    let control = used::CyberpunkVR_GeometryCostProbe.load(Relaxed);
    if control == 0 {
        return CostAction::Forward;
    }
    let mut action = decide(control, used::CyberpunkVR_GeometryCostNode.load(Relaxed), ctx.node(), ctx.side);
    if action != CostAction::Forward {
        let pso = used::CyberpunkVR_GeometryCostPso.load(Relaxed);
        let (viewport_list, x, width) = COST_VIEWPORT.with(Cell::get);
        if (pso != 0 && pso != current_pso() as u64) || !eye_allows(control, viewport_list == list as usize, x, width) {
            action = CostAction::Forward;
        }
    }
    if action != CostAction::Forward {
        used::CyberpunkVR_GeometryCostHits.fetch_add(1, Relaxed);
    }
    action
}
type ScissorFn = unsafe extern "system" fn(List, u32, *const [i32; 4]);
/// GeometryCostScissor: an empty scissor for the draw, the engine's restored after it.
struct CostScissor {
    list: List,
    scissor: Option<ScissorFn>,
}
impl CostScissor {
    fn new(list: List, e: &Entry, action: CostAction) -> Self {
        let scissor = original!(e.head[head::SCISSOR], fn(List, u32, *const [i32; 4]));
        if action != CostAction::EmptyScissor || scissor.is_none() {
            return CostScissor { list, scissor: None };
        }
        let empty = [0i32; 4];
        // SAFETY: the original RSSetScissorRects.
        unsafe { scissor.expect("checked")(list, 1, &empty) };
        CostScissor { list, scissor }
    }
}
impl Drop for CostScissor {
    fn drop(&mut self) {
        let Some(scissor) = self.scissor else { return };
        let state = COST_SCISSOR.with(Cell::get);
        if state.list == self.list as usize && state.count != 0 {
            // SAFETY: the original RSSetScissorRects with the engine's rectangles.
            unsafe { scissor(self.list, state.count, state.rects.as_ptr()) };
        } else {
            // D3D12_VIEWPORT_BOUNDS_MAX.
            let wide = [0, 0, 32767, 32767];
            // SAFETY: the original RSSetScissorRects.
            unsafe { scissor(self.list, 1, &wide) };
        }
    }
}
/// optics::FullRateDraw: the glass draw shaded at 1x1 with passthrough combiners, the engine's rate handed back after.
struct FullRate {
    list: List,
    set: Option<unsafe extern "system" fn(List, i32, *const i32)>,
    rate: i32,
    combiners: [i32; 2],
}
impl FullRate {
    fn new(list: List, pso: *mut c_void, set_rate: usize) -> Self {
        let mut out = FullRate { list, set: None, rate: 0, combiners: [0; 2] };
        if list.is_null() || pso.is_null() || set_rate == 0 {
            return out;
        }
        let (mut rate, mut combiners) = (0i32, [0i32; 2]);
        // SAFETY: local outputs.
        if !unsafe { cvr_optics_full_rate(list, pso, &mut rate, combiners.as_mut_ptr()) } {
            return out;
        }
        let set = original!(set_rate, fn(List, i32, *const i32)).expect("checked");
        out.set = Some(set);
        out.rate = rate;
        out.combiners = combiners;
        let passthrough = [0i32; 2];
        // SAFETY: the original RSSetShadingRate: 1x1, both combiners passthrough.
        unsafe { set(list, 0, passthrough.as_ptr()) };
        out
    }
}
impl Drop for FullRate {
    fn drop(&mut self) {
        if let Some(set) = self.set {
            // SAFETY: the original RSSetShadingRate with the engine's state.
            unsafe { set(self.list, self.rate, self.combiners.as_ptr()) };
        }
    }
}

// ---- the detours ----

type Views = *const D3D12_VERTEX_BUFFER_VIEW;
/// hk_IASetVertexBuffers.
pub unsafe extern "system" fn vertex_buffers(list: List, start: u32, count: u32, views: Views) {
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.head[head::VERTEX_BUFFERS], fn(List, u32, u32, Views)) else { return };
    // SAFETY: the engine's arguments throughout.
    unsafe {
        cvr_pair_probe_vertex_buffers(list, start, count, views.cast());
        if probe_state_on() {
            cvr_lh_probe(1, list, start as u64, count as u64, views as u64);
        }
        if get(addr_of!(used::CyberpunkVR_SightAxisProbe)) != 0 && !views.is_null() && count >= 1 && start == 7 {
            INSTANCE.with(|i| i.set(((*views).BufferLocation, (*views).StrideInBytes)));
        }
        if !cvr_lh_single_pass(0, list, start as u64, count as u64, views.cast()) {
            original(list, start, count, views);
        }
    }
}
/// hk_SetComputeRootSignature.
pub unsafe extern "system" fn compute_root(list: List, root: *mut c_void) {
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.head[head::COMPUTE_ROOT], fn(List, *mut c_void)) else { return };
    COMPUTE_ROOT.with(|c| c.set(root));
    if !root.is_null() {
        LAST_COMPUTE_ROOT.store(root as usize, Release);
    }
    // SAFETY: the engine's arguments.
    unsafe {
        original(list, root);
        if recording() {
            cvr_scene_compute_root(list as usize, root as usize);
        }
    }
}
/// hk_SetComputeRootDescriptorTable.
pub unsafe extern "system" fn compute_table(list: List, index: u32, handle: u64) {
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.head[head::COMPUTE_TABLE], fn(List, u32, u64)) else { return };
    let ctx = Context::read();
    // SAFETY: the engine's arguments.
    unsafe {
        let handle = cvr_census_lut_src_bind(index, handle, ctx.side, ctx.work, ctx.base);
        original(list, index, handle);
        if recording() {
            cvr_scene_compute_table(list as usize, index, handle);
        }
    }
}
/// hk_SetPipelineState.
pub unsafe extern "system" fn set_pipeline(list: List, pso: *mut c_void) {
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.head[head::SET_PSO], fn(List, *mut c_void)) else { return };
    // SAFETY: the engine's arguments throughout.
    unsafe {
        if internal() {
            original(list, pso);
            return;
        }
        CURRENT_PSO.with(|c| c.set(pso));
        cvr_lh_families(0, list, pso, 0, 0);
        cvr_vrs_pipeline(list, pso);
        if probe_state_on() {
            cvr_lh_probe(0, list, pso as u64, 0, 0);
        }
        let ctx = Context::read();
        let bound = crate::pso_hooks::sight_twin(0, cvr_shader_patches_bind(pso, ctx.side));
        if !cvr_lh_single_pass(1, list, pso as u64, bound as u64, null()) {
            original(list, bound);
        }
    }
}
/// hk_ClearDepthStencilView.
pub unsafe extern "system" fn clear_depth(list: List, dsv: usize, flags: i32, depth: f32, stencil: u8, count: u32, rects: *const c_void) {
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.head[head::CLEAR_DSV], fn(List, usize, i32, f32, u8, u32, *const c_void)) else { return };
    let ctx = Context::read();
    flush_engine(list, 5);
    early_unsupported(list, 1);
    marker(list, &ctx, false);
    let cascade_clear = ctx.base != 0 && ctx.work == ctx.base + CASCADE_CLEAR_NODE_RVA as usize;
    // SAFETY: the engine's arguments throughout.
    unsafe {
        if cvr_mono_cascades_clear(list, dsv, flags, depth, stencil, count, ctx.side, cascade_clear) {
            return;
        }
        handoff_clear_view(list, dsv, true);
        access_clear_view(list, dsv, true);
        original(list, dsv, flags, depth, stencil, count, rects);
        cvr_lh_hidden_clear(list, dsv, flags as u32, depth, count, ctx.side, ctx.node(), e.head[head::CLEAR_DSV]);
    }
}
/// hk_DrawInstanced.
pub unsafe extern "system" fn draw_instanced(list: List, vertices: u32, instances: u32, start_vertex: u32, start_instance: u32) {
    copy_windows_unknown_draw(list);
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.head[head::DRAW], fn(List, u32, u32, u32, u32)) else { return };
    let ctx = Context::read();
    let node = ctx.node();
    flush_engine(list, 7);
    early_unsupported(list, 2);
    // SAFETY: the engine's arguments throughout.
    unsafe {
        if cvr_mono_cascades_skip_draw(list) {
            return;
        }
        if crate::native_state_exports::CyberpunkVR_StereoFamilyObserve.load(Relaxed) != 0 {
            cvr_lh_families(1, list, null_mut(), node, ctx.side);
        }
        pair_draw(list, &ctx, 1, [vertices, instances, start_vertex, start_instance, 0]);
        let pair = [vertices, instances, start_vertex, start_instance, 0];
        let start_instance = cvr_pair_probe_before_draw(list, node, current_pso(), 1, &pair, start_instance);
        marker(list, &ctx, true);
        if !internal() {
            shader_census(&ctx, KIND_DRAW, vertices, instances, 0);
        }
        cvr_census_cascade_draw_seen(ctx.work, ctx.base, ctx.side);
        cvr_vrs_prepare_draw(list, node, ctx.side, e.head[head::SHADING_RATE], e.head[head::SHADING_IMAGE]);
        handoff_draw(list);
        access_shader(list, true);
        let action = cost_action(list, &ctx);
        let single = [vertices, instances, start_vertex, start_instance, 0];
        if action != CostAction::Skip && !cvr_lh_single_pass(2, list, node as u64, 1, single.as_ptr().cast()) {
            let _cost = CostScissor::new(list, e, action);
            cvr_scene_profile_draw_begin(list);
            original(list, vertices, instances, start_vertex, start_instance);
            cvr_scene_profile_draw_end(list);
        }
        draw_recorded(list);
        cvr_pair_probe_after_draw(list);
        capture_native_inputs(list);
        if get(addr_of!(used::CyberpunkVR_DrawCensus)) != 0 {
            draw_census_note(&ctx);
        }
    }
}
/// draw_census_note.
fn draw_census_note(ctx: &Context) {
    if ctx.base == 0 || ctx.work <= ctx.base {
        return;
    }
    // SAFETY: plain arguments.
    unsafe { cvr_census_draw_note((ctx.work - ctx.base) as u32, ctx.vrcam) };
}
type DrawIndexed = unsafe extern "system" fn(List, u32, u32, u32, i32, u32);
/// lens_velocity: after a glass draw, the lens redrawn into the G-buffer velocity target.
fn lens_velocity(list: List, e: &Entry, ctx: &Context, draw: DrawIndexed, args: (u32, u32, u32, i32, u32)) {
    let pso = current_pso();
    let (Some(barrier_call), Some(set_pso)) =
        (original!(e.head[head::BARRIER_CALL], fn(List, u32, *const Barrier)), original!(e.head[head::SET_PSO], fn(List, *mut c_void)))
    else {
        return;
    };
    // SAFETY: the bound pipeline.
    if pso.is_null() || !unsafe { cvr_optics_glass_pipeline(pso) } {
        return;
    }
    let mut pass = LensVelocityPass::default();
    // SAFETY: a local output; its target's reference is this function's.
    if !unsafe { cvr_optics_prepare_lens_velocity(list, pso, &mut pass) } {
        return;
    }
    let target = Owned(pass.target as *mut c_void);
    let Some(om) = original!(e.head[head::OM], fn(List, u32, *const usize, i32, *const usize)) else { return };
    if !keep(list, &[target.0]) {
        return;
    }
    let read = 0x40 | 0x80;
    let mut barrier = Barrier { kind: 0, flags: 0, resource: target.0, subresource: ALL_SUBRESOURCES, before: read, after: 4, pad: 0 };
    let t = &pass.targets;
    let depth = if t.has_depth { &t.depth as *const usize } else { null() };
    // SAFETY: the originals with the pass' and the engine's state.
    unsafe {
        barrier_call(list, 1, &barrier);
        om(list, 1, &pass.rtv, 0, depth);
        set_pso(list, pass.pipeline as *mut c_void);
        draw(list, args.0, args.1, args.2, args.3, args.4);
        set_pso(list, cvr_shader_patches_bind(pso, ctx.side));
        om(list, t.count, if t.count != 0 { t.handles.as_ptr() } else { null() }, t.contiguous, depth);
        core::mem::swap(&mut barrier.before, &mut barrier.after);
        barrier_call(list, 1, &barrier);
    }
}
/// A COM reference this code holds (ComPtr::Attach).
struct Owned(*mut c_void);
impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the held reference.
            drop(unsafe { windows::core::IUnknown::from_raw(self.0) });
        }
    }
}
/// lens_mask: the glass (or the sight) redrawn into the eye's upscaler history mask.
fn lens_mask(list: List, e: &Entry, ctx: &Context, draw: DrawIndexed, args: (u32, u32, u32, i32, u32)) {
    let pso = current_pso();
    let Some(set_pso) = original!(e.head[head::SET_PSO], fn(List, *mut c_void)) else { return };
    // SAFETY: a plain query.
    if pso.is_null() || ctx.side < 0 || ctx.side > 1 || !unsafe { cvr_optics_mask_enabled() } {
        return;
    }
    // SAFETY: the bound pipeline.
    let mut twin = unsafe { cvr_optics_glass_mask_pipeline(pso) };
    if twin.is_null() {
        twin = crate::pso_hooks::sight_twin(1, pso);
    }
    if twin.is_null() {
        return;
    }
    let (mut raw, mut rtv) = (null_mut(), 0usize);
    // SAFETY: local outputs; the mask's reference is this function's.
    if !unsafe { cvr_optics_mask_target(ctx.side, &mut raw, &mut rtv) } {
        return;
    }
    let mask = Owned(raw);
    let mut t = BoundTargets::default();
    // SAFETY: a local output.
    if !unsafe { cvr_optics_bound_targets(list, &mut t) } {
        return;
    }
    let Some(om) = original!(e.head[head::OM], fn(List, u32, *const usize, i32, *const usize)) else { return };
    if !keep(list, &[mask.0]) {
        return;
    }
    let depth = if t.has_depth { &t.depth as *const usize } else { null() };
    // SAFETY: the originals with the mask's and the engine's state.
    unsafe {
        om(list, 1, &rtv, 0, depth);
        set_pso(list, twin);
        draw(list, args.0, args.1, args.2, args.3, args.4);
        set_pso(list, crate::pso_hooks::sight_twin(0, cvr_shader_patches_bind(pso, ctx.side)));
        om(list, t.count, if t.count != 0 { t.handles.as_ptr() } else { null() }, t.contiguous, depth);
        cvr_optics_mask_drawn(ctx.side);
    }
}
/// hk_DrawIndexedInstanced.
pub unsafe extern "system" fn draw_indexed(list: List, indices: u32, instances: u32, first: u32, base_vertex: i32, start_instance: u32) {
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.head[head::DRAW_INDEXED], fn(List, u32, u32, u32, i32, u32)) else { return };
    // SAFETY: the engine's arguments throughout.
    unsafe {
        if internal() {
            copy_windows_unknown_draw(list);
            handoff_draw(list);
            access_shader(list, true);
            original(list, indices, instances, first, base_vertex, start_instance);
            return;
        }
        let ctx = Context::read();
        let node = ctx.node();
        if !probe_state_on() && retained_active() {
            retained_flush(list, 10);
        }
        if cvr_mono_cascades_skip_draw(list) {
            return;
        }
        if crate::native_state_exports::CyberpunkVR_StereoFamilyObserve.load(Relaxed) != 0 {
            cvr_lh_families(1, list, null_mut(), node, ctx.side);
        }
        let mut retained_observed = false;
        if probe_state_on() {
            cvr_vrs_restore(list);
            let _full_rate = FullRate::new(list, current_pso(), e.head[head::SHADING_RATE]);
            if cvr_lh_probe_draw(list, current_pso(), node, ctx.side, indices, instances, first, base_vertex, start_instance, &mut retained_observed) {
                return;
            }
        }
        if !retained_observed && retained_active() {
            retained_flush(list, 17);
        }
        if copy_windows_active() {
            let mut pipeline = 0u64;
            if !cvr_lh_probe_pipeline(list, &mut pipeline) || families_status(pipeline as *mut c_void) != 1 {
                copy_windows_unknown_draw(list);
            }
        }
        pair_draw(list, &ctx, 2, [indices, instances, first, base_vertex as u32, start_instance]);
        let pair = [indices, instances, first, base_vertex as u32, start_instance];
        let start_instance = cvr_pair_probe_before_draw(list, node, current_pso(), 2, &pair, start_instance);
        marker(list, &ctx, true);
        shader_census(&ctx, KIND_DRAW_INDEXED, indices, instances, 0);
        cvr_census_cascade_draw_seen(ctx.work, ctx.base, ctx.side);
        let sight_shape = indices == 6 && instances == 1 && first == 0 && base_vertex == 0 && start_instance != 0;
        if get(addr_of!(used::CyberpunkVR_SightAxisProbe)) != 0 && sight_shape {
            let pso = current_pso();
            if !pso.is_null() && cvr_census_pso_ps(pso) == get(addr_of!(used::CyberpunkVR_SightPsHash)) {
                let (va, stride) = INSTANCE.with(Cell::get);
                cvr_census_sight_axis_note(ctx.vrcam, start_instance, va, stride);
                cvr_census_sight_axis_readback(list, ctx.vrcam, start_instance, va.wrapping_add(start_instance as u64 * stride as u64));
                cvr_census_sight_axis_drain();
            }
        }
        if get(addr_of!(used::CyberpunkVR_PsoProbe)) != 0 && sight_shape {
            let base = ctx.base;
            let rva = if base != 0 && ctx.work > base { (ctx.work - base) as u32 } else { 0 };
            let ps = cvr_census_sight_note(current_pso(), rva, ctx.vrcam);
            if ps != 0 && ps == get(addr_of!(used::CyberpunkVR_SightSkipPs)) {
                return;
            }
        }
        let probe = used::CyberpunkVR_DrawProbeIndices.load(Relaxed);
        if probe != 0 && indices == probe && !current_pso().is_null() {
            cvr_census_draw_probe(current_pso());
        }
        {
            cvr_vrs_prepare_draw(list, node, ctx.side, e.head[head::SHADING_RATE], e.head[head::SHADING_IMAGE]);
            handoff_draw(list);
            access_shader(list, true);
            let action = cost_action(list, &ctx);
            let _full_rate = FullRate::new(list, current_pso(), e.head[head::SHADING_RATE]);
            let single = [indices, instances, first, base_vertex as u32, start_instance];
            if action != CostAction::Skip && !cvr_lh_single_pass(2, list, node as u64, 2, single.as_ptr().cast()) {
                {
                    let _cost = CostScissor::new(list, e, action);
                    cvr_scene_profile_draw_begin(list);
                    original(list, indices, instances, first, base_vertex, start_instance);
                    cvr_scene_profile_draw_end(list);
                }
                let args = (indices, instances, first, base_vertex, start_instance);
                lens_velocity(list, e, &ctx, original, args);
                lens_mask(list, e, &ctx, original, args);
            }
            draw_recorded(list);
        }
        cvr_pair_probe_after_draw(list);
        capture_native_inputs(list);
        if get(addr_of!(used::CyberpunkVR_DrawCensus)) != 0 {
            draw_census_note(&ctx);
        }
        if get(addr_of!(used::CyberpunkVR_CascadeCensus)) != 0 && ctx.base != 0 && ctx.work == ctx.base + CASCADE_NODE_RVA as usize {
            cvr_census_cascade_note(indices, instances, start_instance, ctx.vrcam);
            cvr_census_cascade_report();
        }
    }
}
/// hk_ExecuteIndirect.
pub unsafe extern "system" fn execute_indirect(list: List, signature: *mut c_void, max: u32, arguments: *mut c_void, offset: u64, counter: *mut c_void, counter_offset: u64) {
    let original = entry(list).and_then(|e| original!(e.head[head::INDIRECT], fn(List, *mut c_void, u32, *mut c_void, u64, *mut c_void, u64)));
    let Some(original) = original else {
        // SAFETY: an exported counter; the list's vtable pointer.
        let n = unsafe { bump(addr_of_mut!(used::CyberpunkVR_DebugEiNoEntry)) };
        if n == 1 || n % 1000 == 0 {
            let table = if list.is_null() { 0 } else { unsafe { *(list as *const usize) } };
            log(&format!(
                "[EI-DIAG] ExecuteIndirect with no hook entry -- DRAW DROPPED (list={} vtable={} count={n})",
                pointer(list as usize),
                pointer(table)
            ));
        }
        return;
    };
    let e = entry(list).expect("checked");
    // SAFETY: the engine's arguments throughout.
    unsafe {
        let operands = |stride: u32| {
            cvr_lh_operand(list, arguments, false, 9, u32::MAX, offset, if stride != 0 { stride as u64 * max as u64 } else { u64::MAX });
            if !counter.is_null() {
                cvr_lh_operand(list, counter, false, 9, u32::MAX, counter_offset, 4);
            }
        };
        let observe = |stride: u32| {
            if stride != 0 {
                handoff_draw(list);
                access_shader(list, true);
            } else {
                handoff_unknown(list, 9);
                access_unknown(list, 9);
            }
        };
        if internal() {
            copy_windows_unknown_draw(list);
            let stride = pure_stride(signature);
            if stride != 0 {
                handoff_draw(list);
            } else {
                handoff_unknown(list, 9);
            }
            if stride != 0 {
                access_shader(list, true);
            } else {
                access_unknown(list, 9);
            }
            operands(stride);
            original(list, signature, max, arguments, offset, counter, counter_offset);
            return;
        }
        let ctx = Context::read();
        if arguments.is_null() {
            if retained_active() {
                retained_flush(list, 9);
            }
            let n = bump(addr_of_mut!(used::CyberpunkVR_DebugEiNullArgs));
            if n == 1 || n % 500 == 0 {
                log(&format!(
                    "[EI-DIAG] skipped ExecuteIndirect with NULL argument buffer (list={} sig={} maxCount={max} argOff={offset} vrcamNode={} count={n})",
                    pointer(list as usize),
                    pointer(signature as usize),
                    ctx.vrcam as i32
                ));
            }
            return;
        }
        if retained_active() && cvr_lh_retained_indirect(list, signature, max, arguments, offset, counter, counter_offset) {
            draw_recorded(list);
            return;
        }
        if copy_windows_active() {
            let mut pipeline = 0u64;
            if pure_stride(signature) == 0 || !cvr_lh_probe_pipeline(list, &mut pipeline) || families_status(pipeline as *mut c_void) != 1 {
                copy_windows_unknown_draw(list);
            }
        }
        early_unsupported(list, 3);
        {
            cvr_vrs_prepare_draw(list, ctx.node(), ctx.side, e.head[head::SHADING_RATE], e.head[head::SHADING_IMAGE]);
            if !cvr_mono_cascades_skip_indirect(list, signature) {
                let stride = pure_stride(signature);
                observe(stride);
                operands(stride);
                if stride == 0 {
                    shader_census(&ctx, KIND_EXECUTE_INDIRECT, max, 0, 0);
                }
                pair_draw(list, &ctx, 3, [max, stride, offset as u32, 0, 0]);
                marker(list, &ctx, stride != 0);
                let action = if stride != 0 { cost_action(list, &ctx) } else { CostAction::Forward };
                if action != CostAction::Skip {
                    let _cost = CostScissor::new(list, e, action);
                    original(list, signature, max, arguments, offset, counter, counter_offset);
                }
                draw_recorded(list);
            }
        }
        capture_native_inputs(list);
        if get(addr_of!(used::CyberpunkVR_IndirectCensus)) == 0 || ctx.base == 0 || ctx.work <= ctx.base {
            return;
        }
        cvr_census_indirect_note((ctx.work - ctx.base) as u32, signature, ctx.vrcam);
    }
}
/// The outline compute pipeline built against the engine's root signature (g_vision_cs_pso, _rs).
static VISION_PSO: Mutex<(usize, usize)> = Mutex::new((0, 0));
static VISION_SAID: AtomicBool = AtomicBool::new(false);
/// vision_cs_pso_get.
fn vision_pso(list: List) -> *mut c_void {
    // SAFETY: an exported control.
    if unsafe { get(addr_of!(used::CyberpunkVR_VisionCsSwap)) } == 0 || list.is_null() {
        return null_mut();
    }
    let root = COMPUTE_ROOT.with(Cell::get);
    // SAFETY: a plain query.
    if root.is_null() || !unsafe { cvr_dh_vision_ready() } {
        return null_mut();
    }
    let mut cached = VISION_PSO.lock().unwrap_or_else(|p| p.into_inner());
    if cached.0 != 0 && cached.1 == root as usize {
        return cached.0 as *mut c_void;
    }
    // SAFETY: the engine's live list.
    let Some(l) = (unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) }) else { return null_mut() };
    // SAFETY: the list's device.
    let mut device: Option<ID3D12Device> = None;
    // SAFETY: the list's device into a local.
    if unsafe { l.GetDevice(&mut device) }.is_err() {
        return null_mut();
    }
    let Some(device) = device else { return null_mut() };
    if cached.0 != 0 {
        // SAFETY: the held pipeline's reference.
        drop(unsafe { ID3D12PipelineState::from_raw(cached.0 as *mut c_void) });
        cached.0 = 0;
    }
    let mut size = 0usize;
    // SAFETY: the outline compute shader's static blob.
    let code = unsafe { cvr_dh_shader_blob(3, &mut size) };
    let mut desc = D3D12_COMPUTE_PIPELINE_STATE_DESC::default();
    // SAFETY: the description borrows the root signature (ManuallyDrop: no release).
    desc.pRootSignature = unsafe { core::mem::transmute_copy::<*mut c_void, core::mem::ManuallyDrop<Option<ID3D12RootSignature>>>(&root) };
    desc.CS = D3D12_SHADER_BYTECODE { pShaderBytecode: code.cast(), BytecodeLength: size };
    // SAFETY: a complete description.
    let made = unsafe { device.CreateComputePipelineState::<ID3D12PipelineState>(&desc) };
    drop(device);
    let pso = match made {
        Ok(p) => p,
        Err(error) => {
            if !VISION_SAID.swap(true, Relaxed) {
                log(&format!(
                    "[pso] outline CS pipeline REFUSED hr=0x{:08X} rs={} -- engine's own kept",
                    error.code().0 as u32,
                    pointer(root as usize)
                ));
            }
            return null_mut();
        }
    };
    let raw = pso.into_raw();
    *cached = (raw as usize, root as usize);
    log(&format!("[pso] outline CS pipeline built against the engine's root signature rs={}", pointer(root as usize)));
    raw
}
/// SetPipelineState through the list's vtable (the port's own detour first).
fn set_pipeline_through(list: List, pso: *mut c_void) {
    // SAFETY: the list's slot 25.
    let f = unsafe { *((*(list as *const usize)) as *const usize).add(25) };
    if let Some(f) = original!(f, fn(List, *mut c_void)) {
        // SAFETY: the list's own SetPipelineState.
        unsafe { f(list, pso) };
    }
}
/// hk_Dispatch.
pub unsafe extern "system" fn dispatch(list: List, x: u32, y: u32, z: u32) {
    copy_windows_unknown(list);
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.head[head::DISPATCH], fn(List, u32, u32, u32)) else { return };
    let ctx = Context::read();
    flush_engine(list, 8);
    marker(list, &ctx, false);
    if !internal() {
        shader_census(&ctx, KIND_DISPATCH, x, y, z);
    }
    let mut swapped = false;
    let previous = current_pso();
    // SAFETY: exported controls and the engine's arguments throughout.
    unsafe {
        if get(addr_of!(used::CyberpunkVR_VisionCsSwap)) != 0 && ctx.base != 0 {
            let rva = ctx.node();
            let dim = get(addr_of!(used::CyberpunkVR_VisionCsDim));
            let dim_ok = dim <= 0 || (x == dim as u32 && y == dim as u32);
            let wanted = get(addr_of!(used::CyberpunkVR_VisionCsNode));
            let node_ok = wanted == u32::MAX || rva == wanted;
            if node_ok && dim_ok {
                let ordinal = VIS_SWAP.with(|v| {
                    let (mut node, mut ordinal) = v.get();
                    if ctx.work != node {
                        node = ctx.work;
                        ordinal = 0;
                    }
                    v.set((node, ordinal + 1));
                    ordinal
                });
                let wanted_ordinal = get(addr_of!(used::CyberpunkVR_VisionCsOrd));
                if (wanted_ordinal < 0 || ordinal == wanted_ordinal) && get(addr_of!(used::CyberpunkVR_VisionCsSwap)) != 0 {
                    let mine = vision_pso(list);
                    if !mine.is_null() {
                        set_pipeline_through(list, mine);
                        swapped = true;
                        if diagnostics() {
                            add(addr_of_mut!(used::CyberpunkVR_DebugVisionCsSwaps), 1);
                        }
                    }
                }
            }
        }
        handoff_unknown(list, 10);
        access_shader(list, false);
        original(list, x, y, z);
        capture_native_inputs(list);
        if swapped && !previous.is_null() {
            set_pipeline_through(list, previous);
        }
        cvr_lh_last_dispatch(x, y, z);
        if get(addr_of!(used::CyberpunkVR_DispatchCensus)) != 0 && ctx.base != 0 && ctx.work > ctx.base {
            cvr_census_dispatch_note((ctx.work - ctx.base) as u32, x, y, z, ctx.vrcam);
            cvr_census_dispatch_report();
        }
        if get(addr_of!(used::CyberpunkVR_VolumeNodeProbe)) != 0 && ctx.base != 0 && x == y && y == z && (2..=16).contains(&x) {
            cvr_cl_volume_node_note(ctx.node(), x, ctx.vrcam);
        }
        if get(addr_of!(used::CyberpunkVR_LightContent)) != 0 {
            cvr_census_light_content_report();
        }
    }
}
/// `(EarlyDepthWholeTarget || early_depth::RunTracking())` and a depth-stencil operand: early depth cannot follow.
fn depth_copy_unsupported(list: List, destination: *mut c_void, source: *mut c_void, reason: u32) {
    if internal() {
        return;
    }
    if crate::early_depth_exports::CyberpunkVR_EarlyDepthWholeTarget.load(Relaxed) == 0 && !early_run_tracking() {
        return;
    }
    if (!destination.is_null() && depth_stencil(destination)) || (!source.is_null() && depth_stencil(source)) {
        early_unsupported(list, reason);
    }
}
/// probe_CopyResource.
pub unsafe extern "system" fn copy_resource(list: List, destination: *mut c_void, source: *mut c_void) {
    resources_engine(list, source, destination, 4);
    buffer_written(list, destination);
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.head[head::COPY_RESOURCE], fn(List, *mut c_void, *mut c_void)) else { return };
    depth_copy_unsupported(list, destination, source, 4);
    // SAFETY: the engine's arguments throughout.
    unsafe {
        cvr_mono_cascades_write(destination);
        early_instance_write(list, destination);
        texture_written(list, destination);
        cvr_lh_constants(1, list, destination, 0, source, 0, 0);
        if !internal() {
            cvr_upload_shadow_invalidate_resource(destination);
        }
        handoff_written(list, destination, 4);
        access_read(list, source, 4, ALL_SUBRESOURCES, 0, u64::MAX);
        access_written(list, destination, 4, ALL_SUBRESOURCES, 0, u64::MAX);
        original(list, destination, source);
    }
    menu_surface_written(list, destination);
}
type CopyTexture = unsafe extern "system" fn(List, *const CopyLocation, u32, u32, u32, *const CopyLocation, *const c_void);
/// optics_scene_copy: the lens source resolve's image copied over the glass pass' scene copy.
fn optics_scene_copy(list: List, copy: CopyTexture, destination: *const CopyLocation, x: u32, y: u32, z: u32, source: *const CopyLocation, area: *const c_void) {
    if internal() {
        return;
    }
    // SAFETY: the engine's locations.
    let target = unsafe { CopyLocation::resource_of(destination) };
    if !destination.is_null() {
        menu_surface_written(list, target);
    }
    // SAFETY: plain queries and the engine's arguments.
    unsafe {
        if !destination.is_null() && cvr_optics_camera_node_active() {
            cvr_optics_camera_copied(list, target);
        }
        let side = Context::read().side;
        let resolved = cvr_optics_source_scene_copied(list, side, destination, x, y, z, source, !area.is_null());
        if resolved.is_null() {
            return;
        }
        if !keep(list, &[resolved]) {
            cvr_optics_source_list_reset(list);
            return;
        }
        let from = CopyLocation { resource: resolved, kind: 0, pad: 0, subresource: 0 };
        let mut whole = [0u8; 80];
        core::ptr::copy_nonoverlapping((&from as *const CopyLocation).cast::<u8>(), whole.as_mut_ptr(), core::mem::size_of::<CopyLocation>());
        let _internal = cvr_host::probe::Internal::new();
        copy(list, destination, 0, 0, 0, whole.as_ptr().cast(), null());
    }
}
/// hk_CopyTextureRegion (without the native probe).
pub unsafe extern "system" fn copy_texture(list: List, destination: *const CopyLocation, x: u32, y: u32, z: u32, source: *const CopyLocation, area: *const c_void) {
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.head[head::COPY_TEXTURE], fn(List, *const CopyLocation, u32, u32, u32, *const CopyLocation, *const c_void)) else { return };
    // SAFETY: the engine's arguments.
    unsafe { original(list, destination, x, y, z, source, area) };
    optics_scene_copy(list, original, destination, x, y, z, source, area);
}
/// probe_CopyTextureRegion (with the native probe).
pub unsafe extern "system" fn probe_copy_texture(list: List, destination: *const CopyLocation, x: u32, y: u32, z: u32, source: *const CopyLocation, area: *const c_void) {
    // SAFETY: the engine's locations throughout.
    let (target, from) = unsafe { (CopyLocation::resource_of(destination), CopyLocation::resource_of(source)) };
    resources_engine(list, from, target, 4);
    if !destination.is_null() {
        buffer_written(list, target);
    }
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.head[head::COPY_TEXTURE], fn(List, *const CopyLocation, u32, u32, u32, *const CopyLocation, *const c_void)) else { return };
    if !destination.is_null() {
        early_instance_write(list, target);
        texture_written(list, target);
    }
    depth_copy_unsupported(list, target, from, 5);
    handoff_written(list, target, 4);
    // SAFETY: the engine's locations.
    unsafe {
        access_read(list, from, 4, CopyLocation::subresource_of(source), 0, u64::MAX);
        access_written(list, target, 4, CopyLocation::subresource_of(destination), 0, u64::MAX);
        original(list, destination, x, y, z, source, area);
    }
    optics_scene_copy(list, original, destination, x, y, z, source, area);
}
/// hk_CopyBufferRegion.
pub unsafe extern "system" fn copy_buffer(list: List, destination: *mut c_void, destination_offset: u64, source: *mut c_void, source_offset: u64, bytes: u64) {
    resources_engine(list, source, destination, 14);
    buffer_written(list, destination);
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.head[head::COPY_BUFFER], fn(List, *mut c_void, u64, *mut c_void, u64, u64)) else { return };
    // SAFETY: the engine's arguments throughout.
    unsafe {
        early_instance_write(list, destination);
        cvr_lh_constants(0, list, destination, destination_offset, source, source_offset, bytes);
        if !internal() {
            cvr_upload_shadow_observe(destination, destination_offset, source, source_offset, bytes);
        }
        handoff_written(list, destination, 4);
        access_read(list, source, 4, u32::MAX, source_offset, bytes);
        access_written(list, destination, 4, u32::MAX, destination_offset, bytes);
        original(list, destination, destination_offset, source, source_offset, bytes);
        let ctx = Context::read();
        let light_node = ctx.base != 0 && ctx.work > ctx.base && {
            let rva = (ctx.work - ctx.base) as u32;
            rva == CLUSTERED_LIGHTS_CULL_RVA || rva == RENDER_LIGHT_BUFFERS_RVA
        };
        if get(addr_of!(used::CyberpunkVR_LightContent)) != 0 && !source.is_null() && (4096..=LIGHT_SNAP_MAX).contains(&bytes) && light_node {
            cvr_census_light_content_note(source, source_offset, bytes, ctx.vrcam);
        }
        if get(addr_of!(declared::CyberpunkVR_LightCensus)) != 0 && bytes != 0 && light_node {
            let (total, count) = if ctx.vrcam {
                (addr_of_mut!(declared::CyberpunkVR_DebugLightBytesVrcam), addr_of_mut!(declared::CyberpunkVR_DebugLightUploadsVrcam))
            } else {
                (addr_of_mut!(declared::CyberpunkVR_DebugLightBytesMain), addr_of_mut!(declared::CyberpunkVR_DebugLightUploadsMain))
            };
            add(total, bytes);
            add(count, 1);
            let size = if destination.is_null() { 0 } else { cvr_lh_resource_width(destination) };
            cvr_census_light_note(bytes, size, destination, ctx.vrcam);
            cvr_census_light_report();
        }
        if get(addr_of!(used::CyberpunkVR_SightAxisProbe)) != 0 && !destination.is_null() && !source.is_null() && bytes >= 64 {
            let mapped = cvr_upload_map_read(source);
            if !mapped.is_null() {
                cvr_census_filled_note(destination, destination_offset, bytes, mapped.wrapping_add(source_offset as usize));
            }
        }
        cvr_lh_constant_buffers(destination, destination_offset, source, source_offset, bytes);
    }
}

// ---- the game-facing wrapper vtable (a capture layer above the port) ----

unsafe extern "system" fn wrap_draw(list: List, a: u32, b: u32, c: u32, d: u32) {
    marker_in(list, &MARK_WRAP, &Context::read());
    if let Some(f) = original!(WRAP[0].load(Acquire), fn(List, u32, u32, u32, u32)) {
        // SAFETY: the wrapper's own DrawInstanced.
        unsafe { f(list, a, b, c, d) };
    }
}
unsafe extern "system" fn wrap_draw_indexed(list: List, a: u32, b: u32, c: u32, d: i32, f5: u32) {
    marker_in(list, &MARK_WRAP, &Context::read());
    if let Some(f) = original!(WRAP[1].load(Acquire), fn(List, u32, u32, u32, i32, u32)) {
        // SAFETY: the wrapper's own DrawIndexedInstanced.
        unsafe { f(list, a, b, c, d, f5) };
    }
}
unsafe extern "system" fn wrap_dispatch(list: List, x: u32, y: u32, z: u32) {
    marker_in(list, &MARK_WRAP, &Context::read());
    if let Some(f) = original!(WRAP[2].load(Acquire), fn(List, u32, u32, u32)) {
        // SAFETY: the wrapper's own Dispatch.
        unsafe { f(list, x, y, z) };
    }
}
unsafe extern "system" fn wrap_clear_depth(list: List, dsv: usize, flags: i32, depth: f32, stencil: u8, count: u32, rects: *const c_void) {
    marker_in(list, &MARK_WRAP, &Context::read());
    if let Some(f) = original!(WRAP[3].load(Acquire), fn(List, usize, i32, f32, u8, u32, *const c_void)) {
        // SAFETY: the wrapper's own ClearDepthStencilView.
        unsafe { f(list, dsv, flags, depth, stencil, count, rects) };
    }
}
/// patch_vtable_slot.
///
/// # Safety
/// `table` is a live vtable with the slot.
unsafe fn patch_slot(table: *mut usize, slot: usize, hook: usize, saved: &core::sync::atomic::AtomicUsize) -> bool {
    use windows::Win32::System::Memory::{VirtualProtect, PAGE_PROTECTION_FLAGS, PAGE_READWRITE};
    // SAFETY: the caller's vtable.
    let at = unsafe { table.add(slot) };
    let mut old = PAGE_PROTECTION_FLAGS::default();
    // SAFETY: the slot's own page.
    if unsafe { VirtualProtect(at.cast(), 8, PAGE_READWRITE, &mut old) }.is_err() {
        return false;
    }
    // SAFETY: the now writable slot.
    unsafe {
        saved.store(*at, Release);
        *at = hook;
    }
    let mut junk = PAGE_PROTECTION_FLAGS::default();
    // SAFETY: the protection put back.
    let _ = unsafe { VirtualProtect(at.cast(), 8, old, &mut junk) };
    true
}
/// RegisterGameFacingListVtable (SwapChain.cpp, swapchain.rs): under RenderDoc the game records through the capture
/// layer's wrapper lists, so the markers go there too.
///
/// # Safety
/// `device` is null or a live device.
#[no_mangle]
pub unsafe extern "C" fn RegisterGameFacingListVtable(device: *mut c_void) {
    if WRAPPER_VTABLE.load(Acquire) != 0 {
        return;
    }
    // SAFETY: the caller's device.
    let Some(device) = (unsafe { ID3D12Device::from_raw_borrowed(&device) }) else { return };
    // SAFETY: a direct allocator and list through the device (the port's own list hooks see it).
    let Ok(allocator) = (unsafe { device.CreateCommandAllocator::<ID3D12CommandAllocator>(D3D12_COMMAND_LIST_TYPE_DIRECT) }) else { return };
    let Ok(list) = (unsafe { device.CreateCommandList::<_, _, ID3D12GraphicsCommandList>(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None) }) else {
        return;
    };
    // SAFETY: the new list.
    let _ = unsafe { list.SetName(windows::core::w!("CVR_census_probe_list")) };
    // SAFETY: the list's vtable and its first slot.
    let table = unsafe { *(list.as_raw() as *const usize) };
    let first = if table != 0 { unsafe { *(table as *const usize) } } else { 0 };
    let module = module_base_name(first);
    if table != 0 && module.eq_ignore_ascii_case("renderdoc.dll") {
        let t = table as *mut usize;
        // SAFETY: the wrapper's vtable.
        let ok = unsafe {
            patch_slot(t, 12, wrap_draw as *const () as usize, &WRAP[0])
                && patch_slot(t, 13, wrap_draw_indexed as *const () as usize, &WRAP[1])
                && patch_slot(t, 14, wrap_dispatch as *const () as usize, &WRAP[2])
                && patch_slot(t, 47, wrap_clear_depth as *const () as usize, &WRAP[3])
        };
        if ok {
            WRAPPER_VTABLE.store(table, Release);
        }
        log(&format!(
            "[cascmark] game-facing list vtable={} owned by {module} -- marker hooks {}",
            pointer(table),
            if ok { "INSTALLED, markers now land in the capture" } else { "FAILED to patch" }
        ));
    } else {
        log(&format!(
            "[cascmark] game-facing list vtable={} owned by {module} -- no capture layer above us, wrapper hooks not needed",
            pointer(table)
        ));
    }
}
