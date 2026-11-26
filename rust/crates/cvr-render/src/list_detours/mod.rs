//! The command lists' detours (formerly src/Stereo/CommandListCensus.cpp's draw, dispatch, copy and binding hooks and
//! src/Stereo/DeviceHooks.cpp's observers): every engine call through a hooked list vtable slot -- what it tells the
//! port's subsystems before and after the original runs, the draws the port adds beside it (the lens velocity and
//! history mask redraws, the outline compute swap), the capture markers and the censuses.
//!
//! The .cpp's slot table names these functions; src/Stereo/ListDetourHost.cpp lends the render node's thread state
//! and the subsystems still in C++.

use core::ffi::{c_char, c_void};
use core::ptr::{addr_of, addr_of_mut, null, null_mut};
use windows::core::Interface;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering::*};

use crate::list_vtable::Entry;

mod cpp_host;
mod draws;
mod host;
mod observers;
mod targets;
pub(crate) use cpp_host::*;
pub(crate) use host::*;

pub(crate) type List = *mut c_void;

/// The render node this thread records (ListDetourHost.cpp's CvrLhContext).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct Context {
    pub base: usize,
    pub work: usize,
    pub active_view_key: u64,
    pub side: i32,
    pub cascade: i32,
    pub vrcam: bool,
    pub active_view_known: bool,
}
const _: () = assert!(core::mem::size_of::<Context>() == 40);
impl Context {
    pub fn read() -> Self {
        let mut out = Context::default();
        // SAFETY: a plain output.
        unsafe { cvr_lh_context(&mut out) };
        out
    }
    /// The node's work function as an RVA, 0 outside a node.
    pub fn node(&self) -> u32 {
        if self.base != 0 && self.work > self.base {
            (self.work - self.base) as u32
        } else {
            0
        }
    }
}

/// D3D12_RESOURCE_BARRIER with a transition.
#[repr(C)]
pub(crate) struct Barrier {
    pub kind: u32,
    pub flags: u32,
    pub resource: *mut c_void,
    pub subresource: u32,
    pub before: u32,
    pub after: u32,
    pub pad: u32,
}
const _: () = assert!(core::mem::size_of::<Barrier>() == 32);
/// cvr::optics::BoundTargets.
#[repr(C)]
#[derive(Default)]
pub(crate) struct BoundTargets {
    pub count: u32,
    pub contiguous: i32,
    pub handles: [usize; 8],
    pub depth: usize,
    pub has_depth: bool,
    pub valid: bool,
}
const _: () = assert!(core::mem::size_of::<BoundTargets>() == 88);
/// cvr_optics_lens_velocity_pass.
#[repr(C)]
#[derive(Default)]
pub(crate) struct LensVelocityPass {
    pub pipeline: usize,
    pub target: usize,
    pub rtv: usize,
    pub targets: BoundTargets,
}
/// D3D12_TEXTURE_COPY_LOCATION's head: the resource, the copy type and (for a subresource index) the index.
#[repr(C)]
pub(crate) struct CopyLocation {
    pub resource: *mut c_void,
    pub kind: u32,
    pub pad: u32,
    pub subresource: u32,
}
pub(crate) const ALL_SUBRESOURCES: u32 = u32::MAX;
impl CopyLocation {
    /// The location's resource (null for no location).
    ///
    /// # Safety
    /// `at` is null or the caller's location.
    pub unsafe fn resource_of(at: *const CopyLocation) -> *mut c_void {
        // SAFETY: the caller's location.
        unsafe { at.as_ref() }.map_or(null_mut(), |l| l.resource)
    }
    /// The location's subresource index, all subresources for a footprint or no location.
    ///
    /// # Safety
    /// `at` is null or the caller's location.
    pub unsafe fn subresource_of(at: *const CopyLocation) -> u32 {
        // SAFETY: the caller's location.
        match unsafe { at.as_ref() } {
            Some(l) if l.kind == 0 => l.subresource,
            _ => ALL_SUBRESOURCES,
        }
    }
}

// Early depth's dispatch (cpp_services.rs, headset_and_early_depth).
extern "C" {
    pub(crate) fn cvr_lh_early_depth(op: u32, list: List, resource: *mut c_void, reason: u32) -> bool;
}
// The Rust halves the .cpp's inline forwarders called.
extern "C" {
    pub(crate) fn cvr_vrs_reset(list: List, known: bool, pso: *mut c_void);
    pub(crate) fn cvr_vrs_invalidate(list: List);
    pub(crate) fn cvr_vrs_pipeline(list: List, pso: *mut c_void);
    pub(crate) fn cvr_vrs_shading_rate(list: List, rate: i32, combiners: *const i32);
    pub(crate) fn cvr_vrs_shading_image(list: List, image: *mut c_void);
    pub(crate) fn cvr_vrs_prepare_draw(list: List, node: u32, eye: i32, set_rate: usize, set_image: usize);
    pub(crate) fn cvr_vrs_restore(list: List);
    pub(crate) fn cvr_mono_cascades_skip_draw(list: List) -> bool;
    pub(crate) fn cvr_mono_cascades_skip_indirect(list: List, signature: *mut c_void) -> bool;
    pub(crate) fn cvr_mono_cascades_clear(list: List, dsv: usize, flags: i32, depth: f32, stencil: u8, rects: u32, side: i32, clear_node: bool) -> bool;
    pub(crate) fn cvr_mono_cascades_discard(resource: *mut c_void) -> bool;
    pub(crate) fn cvr_mono_cascades_write(resource: *mut c_void);
    pub(crate) fn cvr_mono_cascades_close(list: List);
    pub(crate) fn cvr_scene_profile_active() -> *const AtomicBool;
    pub(crate) fn cvr_scene_profile_draw_detail(list: List) -> u32;
    pub(crate) fn cvr_scene_profile_mark(list: List, node: u32, side: i32, pso: *mut c_void, detail: u32);
    pub(crate) fn cvr_scene_profile_close(list: List);
    pub(crate) fn cvr_scene_profile_draw_begin(list: List);
    pub(crate) fn cvr_scene_profile_draw_end(list: List);
    pub(crate) fn cvr_scene_compute_root(list: usize, root: usize);
    pub(crate) fn cvr_scene_compute_table(list: usize, index: u32, handle: u64);
    pub(crate) fn cvr_scene_heaps(list: usize, count: u32, heaps: *const usize);
    pub(crate) fn cvr_scene_topology(list: usize, topology: i32);
    pub(crate) fn cvr_scene_stencil(list: usize, stencil: u32);
    pub(crate) fn cvr_scene_blend_factor(list: usize, factors: *const f32);
    pub(crate) fn cvr_scene_shading_rate(list: usize, rate: i32, combiners: *const i32);
    pub(crate) fn cvr_scene_shading_rate_image(list: usize, image: usize);
    pub(crate) fn cvr_sp_trace_list(node: u32, side: i32, list: *const c_void);
    pub(crate) fn cvr_pair_probe_tracking() -> bool;
    pub(crate) fn cvr_pair_probe_vertex_buffers(list: List, start: u32, count: u32, views: *const c_void);
    pub(crate) fn cvr_pair_probe_draw(list: List, node: u32, pso: *mut c_void, kind: u32, args: *const [u32; 5]);
    pub(crate) fn cvr_pair_probe_before_draw(list: List, node: u32, pso: *mut c_void, kind: u32, args: *const [u32; 5], start_instance: u32) -> u32;
    pub(crate) fn cvr_pair_probe_after_draw(list: List);
    pub(crate) fn cvr_shader_patches_bind(pso: *mut c_void, side: i32) -> *mut c_void;
    pub(crate) fn cvr_shader_patches_command(node: u32, side: i32, pso: *mut c_void, kind: u32, x: u32, y: u32, z: u32);
    pub(crate) fn cvr_upload_shadow_invalidate_resource(destination: *mut c_void);
    pub(crate) fn cvr_upload_shadow_observe(destination: *mut c_void, offset: u64, source: *mut c_void, source_offset: u64, bytes: u64);
    pub(crate) fn cvr_ssr_merge_probe_layout(node: u32);
    pub(crate) fn cvr_upload_map_read(res: *mut c_void) -> *const u8;
    pub(crate) fn cvr_gpu_keep_command_resources(list: List, objects: *const *mut c_void, count: usize) -> bool;
    pub(crate) fn cvr_optics_glass_pipeline(pso: *mut c_void) -> bool;
    pub(crate) fn cvr_optics_glass_mask_pipeline(pso: *mut c_void) -> *mut c_void;
    pub(crate) fn cvr_optics_shading_rate_set(list: List, rate: i32, combiners: *const i32);
    pub(crate) fn cvr_optics_full_rate(list: List, pso: *mut c_void, rate: *mut i32, combiners: *mut i32) -> bool;
    pub(crate) fn cvr_optics_bound_targets(list: List, out: *mut BoundTargets) -> bool;
    pub(crate) fn cvr_optics_list_reset(list: List);
    pub(crate) fn cvr_optics_prepare_lens_velocity(list: List, pso: *mut c_void, out: *mut LensVelocityPass) -> bool;
    pub(crate) fn cvr_optics_mask_enabled() -> bool;
    pub(crate) fn cvr_optics_mask_target(side: i32, resource: *mut *mut c_void, handle: *mut usize) -> bool;
    pub(crate) fn cvr_optics_mask_drawn(side: i32);
    pub(crate) fn cvr_optics_camera_node_active() -> bool;
    pub(crate) fn cvr_optics_camera_copied(list: List, destination: *mut c_void);
    pub(crate) fn cvr_optics_source_scene_copied(list: List, side: i32, destination: *const CopyLocation, x: u32, y: u32, z: u32, source: *const CopyLocation, has_box: bool) -> *mut c_void;
    pub(crate) fn cvr_optics_source_list_reset(list: List);
    pub(crate) fn CyberpunkVR_ProfNodeName(rva: u32) -> *const c_char;
    static CyberpunkVR_RuntimeDiagnostics: i32;
}

use crate::list_vtable::{Slot, BINDING_COUNT, LIST5, NO_PROBE, PAIR_INSTALL, PROBE, SAME1, SAME2, SAME4, SAME6, SAME7};
struct Table([Slot; 56]);
// SAFETY: immutable; the rows name process-lifetime functions.
unsafe impl Sync for Table {}
macro_rules! row {
    ($index:expr, $conditions:expr, $detour:expr) => {
        Slot { index: $index, conditions: $conditions, detour: $detour as *const c_void }
    };
}
/// The detour table (formerly DeviceHooks.cpp's kListSlots): rows apply in order, a later row on a slot wins when its
/// conditions hold; the pair probe installs over the rows before it.
static SLOTS: Table = Table([
    row!(46, 0, targets::set_targets),
    row!(26, 0, targets::resource_barrier),
    row!(21, 0, targets::viewports),
    row!(22, 0, targets::scissors),
    row!(10, 0, targets::reset),
    row!(9, 0, observers::close),
    row!(11, 0, observers::clear_state),
    row!(27, 0, observers::execute_bundle),
    row!(51, 0, observers::discard),
    row!(16, 0, observers::cascade_copy_texture),
    row!(17, 0, draws::copy_resource),
    row!(19, 0, observers::resolve),
    row!(48, 0, observers::clear_rtv),
    row!(15, 0, draws::copy_buffer),
    row!(59, 0, draws::execute_indirect),
    row!(14, 0, draws::dispatch),
    row!(12, 0, draws::draw_instanced),
    row!(13, 0, draws::draw_indexed),
    row!(44, 0, draws::vertex_buffers),
    row!(29, 0, draws::compute_root),
    row!(31, 0, draws::compute_table),
    row!(25, 0, draws::set_pipeline),
    row!(47, 0, draws::clear_depth),
    row!(16, PROBE, draws::probe_copy_texture),
    row!(30, PROBE, observers::graphics_root),
    row!(32, PROBE, observers::graphics_table),
    row!(38, PROBE, observers::graphics_cbv),
    row!(43, PROBE, observers::index_buffer),
    row!(55, PROBE, observers::predication),
    row!(28, PROBE, observers::heaps),
    row!(20, PROBE, observers::topology),
    row!(24, PROBE, observers::stencil),
    row!(23, PROBE, observers::blend_factor),
    row!(16, NO_PROBE, draws::copy_texture),
    Slot { index: BINDING_COUNT, conditions: PROBE, detour: core::ptr::null() },
    Slot { index: PAIR_INSTALL, conditions: NO_PROBE, detour: core::ptr::null() },
    row!(64, SAME1, observers::resolve_region),
    row!(68, SAME4, observers::begin_render_pass),
    row!(69, SAME4, observers::end_render_pass),
    row!(76, SAME4 | NO_PROBE, observers::census_rays),
    row!(70, PROBE | SAME4, observers::initialize_meta),
    row!(71, PROBE | SAME4, observers::execute_meta),
    row!(72, PROBE | SAME4, observers::build_ray),
    row!(73, PROBE | SAME4, observers::emit_ray),
    row!(74, PROBE | SAME4, observers::copy_ray),
    row!(76, PROBE | SAME4, observers::dispatch_rays),
    row!(79, PROBE | SAME6, observers::dispatch_mesh),
    row!(18, PROBE, observers::copy_tiles),
    row!(45, PROBE, observers::stream_output),
    row!(54, PROBE, observers::resolve_query),
    row!(49, PROBE, observers::clear_uav_uint),
    row!(50, PROBE, observers::clear_uav_float),
    row!(60, PROBE | SAME1, observers::atomic_copy32),
    row!(61, PROBE | SAME1, observers::atomic_copy64),
    row!(66, PROBE | SAME2, observers::immediate_write),
    row!(80, PROBE | SAME7, observers::enhanced_barrier),
]);
static LIST5_SLOTS: Table2 = Table2([row!(77, LIST5, observers::shading_rate), row!(78, LIST5, observers::shading_image)]);
struct Table2([Slot; 2]);
// SAFETY: immutable; the rows name process-lifetime functions.
unsafe impl Sync for Table2 {}
/// The Rust rows, in the table's order.
pub(crate) fn slots() -> impl Iterator<Item = &'static Slot> {
    SLOTS.0.iter().chain(LIST5_SLOTS.0.iter())
}

/// The record of the list's vtable and its originals.
pub(crate) fn entry(list: List) -> Option<&'static Entry> {
    crate::list_vtable::entry(list)
}
// The originals' places in the record (include/Stereo/StereoInternal.hpp's CommandListVtableHook).
pub(crate) mod head {
    pub const OM: usize = 1;
    pub const BARRIER_ORIGINAL: usize = 2;
    pub const BARRIER_CALL: usize = 3;
    pub const COPY_RESOURCE: usize = 4;
    pub const COPY_TEXTURE: usize = 5;
    pub const INDIRECT: usize = 6;
    pub const DRAW: usize = 7;
    pub const DRAW_INDEXED: usize = 8;
    pub const COPY_BUFFER: usize = 9;
    pub const DISPATCH: usize = 10;
    pub const VIEWPORTS: usize = 11;
    pub const SCISSOR: usize = 12;
    pub const RESET: usize = 13;
    pub const SET_PSO: usize = 14;
    pub const VERTEX_BUFFERS: usize = 15;
    pub const CLEAR_DSV: usize = 16;
    pub const COMPUTE_ROOT: usize = 17;
    pub const COMPUTE_TABLE: usize = 18;
    pub const GRAPHICS_ROOT: usize = 19;
    pub const GRAPHICS_TABLE: usize = 20;
    pub const GRAPHICS_CBV: usize = 21;
    pub const INDEX_BUFFER: usize = 22;
    pub const PREDICATION: usize = 23;
    pub const HEAPS: usize = 24;
    pub const TOPOLOGY: usize = 25;
    pub const STENCIL: usize = 26;
    pub const SHADING_RATE: usize = 27;
    pub const SHADING_IMAGE: usize = 28;
}
pub(crate) mod tail {
    pub const CLOSE: usize = 0;
    pub const CLEAR_STATE: usize = 1;
    pub const BUNDLE: usize = 2;
    pub const DISCARD: usize = 3;
    pub const BLEND_FACTOR: usize = 4;
    pub const RESOLVE_QUERY: usize = 5;
    pub const WRITE_IMMEDIATE: usize = 6;
    pub const ENHANCED_BARRIER: usize = 7;
    pub const STREAM_OUTPUT: usize = 8;
    pub const COPY_TILES: usize = 9;
    pub const ATOMIC_COPY32: usize = 10;
    pub const ATOMIC_COPY64: usize = 11;
    pub const CLEAR_RTV: usize = 12;
    pub const RESOLVE: usize = 13;
    pub const RESOLVE_REGION: usize = 14;
    pub const BEGIN_RENDER_PASS: usize = 15;
    pub const END_RENDER_PASS: usize = 16;
    pub const CLEAR_UAV_UINT: usize = 17;
    pub const CLEAR_UAV_FLOAT: usize = 18;
    pub const INITIALIZE_META: usize = 19;
    pub const EXECUTE_META: usize = 20;
    pub const BUILD_RAY: usize = 21;
    pub const EMIT_RAY: usize = 22;
    pub const COPY_RAY: usize = 23;
    pub const DISPATCH_RAY: usize = 24;
    pub const DISPATCH_MESH: usize = 25;
}
/// An original as a function of the given type, None when the slot was never hooked.
macro_rules! original {
    ($address:expr, fn($($t:ty),*) $(-> $r:ty)?) => {{
        let address: usize = $address;
        if address == 0 {
            None
        } else {
            // SAFETY: the vtable's own function, of the slot's type.
            #[allow(unused_unsafe)]
            let f = unsafe { core::mem::transmute::<usize, unsafe extern "system" fn($($t),*) $(-> $r)?>(address) };
            Some(f)
        }
    }};
}
pub(crate) use original;

/// stereo::probe::internalCommands.
pub(crate) fn internal() -> bool {
    cvr_host::probe::internal()
}
/// scene_state's detail::Recording: the native probe records and the commands are the engine's.
pub(crate) fn recording() -> bool {
    !internal() && crate::declared_moved_exports::CyberpunkVR_NativeStereoProbeState.load(Relaxed) == 1
}
pub(crate) fn probe_state_on() -> bool {
    crate::declared_moved_exports::CyberpunkVR_NativeStereoProbeState.load(Relaxed) == 1
}
/// CVR_DIAGNOSTIC: runtime diagnostics on.
pub(crate) fn diagnostics() -> bool {
    // SAFETY: an exported control.
    unsafe { core::ptr::read_volatile(addr_of!(CyberpunkVR_RuntimeDiagnostics)) != 0 }
}
/// A plain exported control.
///
/// # Safety
/// `p` names one of the port's exports.
pub(crate) unsafe fn get<T: Copy>(p: *const T) -> T {
    // SAFETY: the caller's export.
    unsafe { core::ptr::read_volatile(p) }
}
/// `++export` on a plain 64-bit counter, as the .cpp did (unsynchronised); the new value.
///
/// # Safety
/// `p` names one of the port's exports.
pub(crate) unsafe fn bump(p: *mut u64) -> u64 {
    // SAFETY: the caller's export.
    unsafe {
        let n = core::ptr::read_volatile(p).wrapping_add(1);
        core::ptr::write_volatile(p, n);
        n
    }
}
/// InterlockedExchangeAdd64 on a plain exported counter.
///
/// # Safety
/// `p` names one of the port's 8-byte aligned exports.
pub(crate) unsafe fn add(p: *mut u64, value: u64) {
    // SAFETY: the caller's aligned export.
    unsafe { (*(p as *const AtomicU64)).fetch_add(value, SeqCst) };
}
pub(crate) fn log(text: &str) {
    let mut bytes = Vec::with_capacity(text.len() + 1);
    bytes.extend(text.bytes().filter(|&b| b != 0));
    bytes.push(0);
    // SAFETY: a terminated string.
    unsafe { cvr_lh_log(bytes.as_ptr().cast()) };
}
/// `%p` as MSVC prints it.
pub(crate) fn pointer(p: usize) -> String {
    format!("{p:016X}")
}
/// The address the hook returns to (`_ReturnAddress()`): frame 0 is the hook, frame 1 its caller.
#[inline(always)]
pub(crate) fn return_address() -> usize {
    let mut frames = [null_mut::<c_void>(); 1];
    // SAFETY: a back trace into a local buffer.
    let n = unsafe { windows::Win32::System::Diagnostics::Debug::RtlCaptureStackBackTrace(1, &mut frames, None) };
    if n == 0 {
        0
    } else {
        frames[0] as usize
    }
}

// The host's operations by name.
pub(crate) fn retained_active() -> bool {
    // SAFETY: plain arguments.
    unsafe { cvr_lh_retained(0, null_mut(), null_mut(), null_mut(), 0) }
}
pub(crate) fn retained_flush(list: List, reason: u32) {
    // SAFETY: the engine's list.
    unsafe { cvr_lh_retained(1, list, null_mut(), null_mut(), reason) };
}
pub(crate) fn retained_resources(list: List, source: *mut c_void, destination: *mut c_void, reason: u32) {
    // SAFETY: the engine's list and resources.
    unsafe { cvr_lh_retained(2, list, source, destination, reason) };
}
/// `if(!internal && retained_geometry::Active())retained_geometry::Flush(list,reason)`.
pub(crate) fn flush_engine(list: List, reason: u32) {
    if !internal() && retained_active() {
        retained_flush(list, reason);
    }
}
/// `if(!internal && retained_geometry::Active())retained_geometry::BeforeResources(...)`.
pub(crate) fn resources_engine(list: List, source: *mut c_void, destination: *mut c_void, reason: u32) {
    if !internal() && retained_active() {
        retained_resources(list, source, destination, reason);
    }
}
pub(crate) fn early_unsupported(list: List, reason: u32) {
    // SAFETY: the engine's list.
    unsafe { cvr_lh_early_depth(0, list, null_mut(), reason) };
}
pub(crate) fn early_instance_write(list: List, resource: *mut c_void) {
    // SAFETY: the engine's list and resource.
    unsafe { cvr_lh_early_depth(1, list, resource, 0) };
}
pub(crate) fn early_run_tracking() -> bool {
    // SAFETY: plain arguments.
    unsafe { cvr_lh_early_depth(3, null_mut(), null_mut(), 0) }
}
pub(crate) fn handoff_draw(list: List) {
    // SAFETY: the engine's list.
    unsafe { cvr_lh_handoff(0, list, 0, 0) };
}
pub(crate) fn handoff_unknown(list: List, kind: u32) {
    // SAFETY: the engine's list.
    unsafe { cvr_lh_handoff(1, list, 0, kind) };
}
pub(crate) fn handoff_written(list: List, resource: *mut c_void, kind: u32) {
    // SAFETY: the engine's list and resource.
    unsafe { cvr_lh_handoff(2, list, resource as u64, kind) };
}
pub(crate) fn handoff_clear_view(list: List, handle: usize, depth: bool) {
    // SAFETY: the engine's list.
    unsafe { cvr_lh_handoff(3, list, handle as u64, depth as u32) };
}
pub(crate) fn handoff_close(list: List, result: i32) {
    // SAFETY: the engine's list.
    unsafe { cvr_lh_handoff(4, list, 0, result as u32) };
}
pub(crate) fn access_shader(list: List, graphics: bool) {
    // SAFETY: the engine's list.
    unsafe { cvr_lh_access(0, list, null_mut(), graphics as u32, 0, 0, 0) };
}
pub(crate) fn access_unknown(list: List, kind: u32) {
    // SAFETY: the engine's list.
    unsafe { cvr_lh_access(1, list, null_mut(), kind, 0, 0, 0) };
}
pub(crate) fn access_read(list: List, resource: *mut c_void, kind: u32, subresource: u32, offset: u64, bytes: u64) {
    // SAFETY: the engine's list and resource.
    unsafe { cvr_lh_access(2, list, resource, kind, subresource, offset, bytes) };
}
pub(crate) fn access_written(list: List, resource: *mut c_void, kind: u32, subresource: u32, offset: u64, bytes: u64) {
    // SAFETY: the engine's list and resource.
    unsafe { cvr_lh_access(3, list, resource, kind, subresource, offset, bytes) };
}
pub(crate) fn access_clear_view(list: List, handle: usize, depth: bool) {
    // SAFETY: the engine's list.
    unsafe { cvr_lh_access(4, list, null_mut(), depth as u32, 0, handle as u64, 0) };
}
pub(crate) fn access_close(list: List, result: i32) {
    // SAFETY: the engine's list.
    unsafe { cvr_lh_access(5, list, null_mut(), result as u32, 0, 0, 0) };
}
pub(crate) fn copy_windows_unknown_draw(list: List) {
    // SAFETY: the engine's list.
    unsafe { cvr_lh_copy_windows(0, list) };
}
pub(crate) fn copy_windows_unknown(list: List) {
    // SAFETY: the engine's list.
    unsafe { cvr_lh_copy_windows(1, list) };
}
pub(crate) fn copy_windows_written_all() {
    // SAFETY: plain arguments.
    unsafe { cvr_lh_copy_windows(2, null_mut()) };
}
pub(crate) fn copy_windows_active() -> bool {
    // SAFETY: plain arguments.
    unsafe { cvr_lh_copy_windows(3, null_mut()) }
}
pub(crate) fn families_status(pso: *mut c_void) -> u32 {
    // SAFETY: the pipeline the list holds.
    unsafe { cvr_lh_families(2, null_mut(), pso, 0, 0) }
}
pub(crate) fn families_reset(list: List, pso: *mut c_void) {
    // SAFETY: the engine's list.
    unsafe { cvr_lh_families(3, list, pso, 0, 0) };
}
/// The native probe's list observations (formerly ListDetourHost.cpp's cvr_lh_probe, through native_probe's C++
/// facades to stereo_probe_state.rs): the shim's numbering -- 0 pipeline, 1 vertex buffers, 2 heaps, 3 root signature,
/// 4 root table, 5 root CBV, 6 index buffer, 7 predication, 9 viewports, 10 scissors, else reset -- onto the list's.
///
/// # Safety
/// The engine's list and arguments.
pub(crate) unsafe fn cvr_lh_probe(op: u32, list: List, a: u64, b: u64, c: u64) {
    use crate::stereo_probe_state::cvr_nsps_list as observe;
    let p = |v: u64| v as usize as *mut c_void;
    // SAFETY: as documented.
    unsafe {
        match op {
            0 => observe(2, list, 0, 0, p(a), false, false, 0),
            1 => observe(3, list, a as u32, b as u32, p(c), false, false, 0),
            2 => observe(1, list, 0, a as u32, p(b), false, false, 0),
            3 => observe(5, list, 0, 0, p(a), false, false, 0),
            4 => observe(6, list, a as u32, 0, null_mut(), false, false, b),
            5 => observe(7, list, a as u32, 0, null_mut(), false, false, b),
            6 => observe(4, list, 0, 0, p(a), false, false, 0),
            7 => observe(8, list, 0, 0, p(a), false, false, 0),
            9 => observe(10, list, 0, a as u32, p(b), false, false, 0),
            10 => observe(11, list, 0, a as u32, p(b), false, false, 0),
            _ => observe(0, list, 0, 0, p(a), b != 0, c != 0, 0),
        }
    }
}
/// native_probe::Draw (formerly ListDetourHost.cpp's cvr_lh_probe_draw).
///
/// # Safety
/// The engine's list and draw.
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn cvr_lh_probe_draw(list: List, pso: *mut c_void, node: u32, side: i32, indices: u32, instances: u32, first: u32, base: i32, start_instance: u32, retained: *mut bool) -> bool {
    // SAFETY: as documented.
    unsafe { crate::stereo_probe_state::cvr_nsps_draw(list, pso, node, side, indices, instances, first, base, start_instance, retained) }
}
/// The list's current pipeline from native_probe::CurrentState (formerly ListDetourHost.cpp's cvr_lh_probe_pipeline).
///
/// # Safety
/// The engine's list; `pipeline` is writable.
pub(crate) unsafe fn cvr_lh_probe_pipeline(list: List, pipeline: *mut u64) -> bool {
    // SAFETY: plain data, zero valid.
    let mut current: crate::geometry_commands::DrawRecord = unsafe { core::mem::zeroed() };
    // SAFETY: as documented.
    if !unsafe { crate::stereo_probe_state::cvr_nsps_current(list, &mut current) } {
        return false;
    }
    // SAFETY: as documented.
    unsafe { *pipeline = current.pipeline };
    true
}
/// descriptors::HeapsBound when the descriptor audit observes (formerly ListDetourHost.cpp's cvr_lh_heaps_bound): the
/// list's device and the bound heaps.
///
/// # Safety
/// The engine's list; `heaps` holds `count` heaps.
pub(crate) unsafe fn cvr_lh_heaps_bound(list: List, count: u32, heaps: *const *mut c_void) {
    if !crate::stereo_probe::cvr_observe_bindings() || cvr_host::probe::internal() {
        return;
    }
    // SAFETY: the engine's list.
    let Some(recording) = (unsafe { windows::Win32::Graphics::Direct3D12::ID3D12GraphicsCommandList::from_raw_borrowed(&list) }) else { return };
    let mut device: Option<windows::Win32::Graphics::Direct3D12::ID3D12Device> = None;
    // SAFETY: a live list.
    if unsafe { recording.GetDevice(&mut device) }.is_err() {
        return;
    }
    let Some(device) = device else { return };
    // SAFETY: the list's device and the engine's heaps.
    unsafe { crate::descriptor_audit::cvr_sda_audit_write(7, device.as_raw(), heaps as usize, count as u64, 0, 0, null_mut(), null_mut()) };
}
pub(crate) fn probe_reset(list: List, pso: *mut c_void, rate_known: bool, new_recording: bool) {
    // SAFETY: the engine's list.
    unsafe { cvr_lh_probe(8, list, pso as u64, rate_known as u64, new_recording as u64) };
}
pub(crate) fn draw_recorded(list: List) {
    // SAFETY: the engine's list.
    unsafe { cvr_lh_list(0, list, null_mut()) };
}
pub(crate) fn capture_native_inputs(list: List) {
    // SAFETY: the engine's list.
    unsafe { cvr_lh_list(1, list, null_mut()) };
}
pub(crate) fn menu_surface_written(list: List, resource: *mut c_void) {
    // SAFETY: the engine's list and resource.
    unsafe { cvr_lh_list(4, list, resource) };
}
/// buffer_read::Written (formerly through ListDetourHost.cpp's cvr_lh_reads): the open copy windows' write, then the
/// buffer reads'.
pub(crate) fn buffer_written(list: List, resource: *mut c_void) {
    if crate::copy_windows::cvr_cw_active_windows.load(core::sync::atomic::Ordering::Relaxed) != 0 {
        crate::copy_windows::cvr_cw_global(0, resource as u64, 0);
    }
    // SAFETY: the engine's list (or null) and resource.
    unsafe { crate::buffer_read::cvr_buffer_read_written(list, resource) };
}
/// texture_read::Written (formerly through cvr_lh_reads): the texture receipts'.
pub(crate) fn texture_written(list: List, resource: *mut c_void) {
    // SAFETY: the engine's list (or null) and resource.
    unsafe { crate::texture_receipts::cvr_tr_written(list, resource) };
}
/// buffer_read::WrittenAddress (formerly through cvr_lh_reads): the open copy windows' range, then the buffer reads'.
pub(crate) fn buffer_written_address(address: u64, bytes: u64) {
    if crate::copy_windows::cvr_cw_active_windows.load(core::sync::atomic::Ordering::Relaxed) != 0 {
        crate::copy_windows::cvr_cw_global(1, address, bytes);
    }
    crate::buffer_read::cvr_buffer_read_written_address(address, bytes);
}
/// buffer_read::ObserverUnavailable (formerly through cvr_lh_reads).
pub(crate) fn buffer_read_unavailable() {
    crate::buffer_read::cvr_buffer_read_unavailable();
}
/// The target access', texture reads' and buffer reads' observers unavailable (ListDetourHost.cpp: the target
/// access is C++'s).
pub(crate) fn read_observation_unavailable() {
    // SAFETY: no arguments.
    unsafe { cvr_lh_reads_unavailable() };
}
pub(crate) fn constants_invalidate(list: List, resource: *mut c_void) {
    // SAFETY: the engine's list and resource.
    unsafe { cvr_lh_constants(2, list, resource, 0, null_mut(), 0, 0) };
}
/// indirect_policy::PureDrawStride (formerly ListDetourHost.cpp's cvr_lh_pure_stride): the signature's receipt.
pub(crate) fn pure_stride(signature: *mut c_void) -> u32 {
    let mut receipt = crate::indirect_policy::Receipt::default();
    // SAFETY: the engine's signature into a local receipt.
    unsafe {
        crate::indirect_policy::cvr_indirect_describe(signature, &mut receipt);
        crate::indirect_policy::cvr_indirect_receipt_pure_stride(&receipt)
    }
}
pub(crate) fn keep(list: List, objects: &[*mut c_void]) -> bool {
    // SAFETY: the engine's list and live objects.
    unsafe { cvr_gpu_keep_command_resources(list, objects.as_ptr(), objects.len()) }
}
/// The resource's D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL.
pub(crate) fn depth_stencil(resource: *mut c_void) -> bool {
    // SAFETY: the engine's live resource.
    unsafe { windows::Win32::Graphics::Direct3D12::ID3D12Resource::from_raw_borrowed(&resource) }
        .is_some_and(|r| unsafe { r.GetDesc() }.Flags.0 & 2 != 0)
}

/// census_pipeline_reset and the .cpp's thread's current pipeline.
pub(crate) fn pipeline_reset(pso: *mut c_void) {
    if !internal() {
        draws::CURRENT_PSO.with(|c| c.set(pso));
    }
}
/// The .cpp's census_pipeline_reset (Dlss.cpp's list reset).
#[no_mangle]
pub extern "C" fn cvr_ld_pipeline_reset(pso: *mut c_void) {
    pipeline_reset(pso);
}
/// The .cpp's vision_swap_list_reset: a list Reset starts a new recording.
#[no_mangle]
pub extern "C" fn cvr_ld_vision_swap_reset() {
    draws::VIS_SWAP.with(|v| v.set((0, 0)));
}
