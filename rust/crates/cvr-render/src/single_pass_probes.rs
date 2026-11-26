//! The single-pass engine probes (formerly src/Stereo/Grading.cpp's BindCameraProbe, BindMaterialProbe and
//! RenderPacketsProbe with their gates and CVR_DETOUR_IF lines, and the source SRV setters' registrations): the bounded
//! camera binding (the source CBVs' annotations, early depth, paired geometry and the camera upload), the native
//! material binding, the current buffer SRV and cached SRV group (their asm thunks), and the bounded native geometry
//! packets (the stage replay's consumer, the packet capture, the preparation's draw scope, the GPU probe's group and
//! early depth). Registered by engine_detours.rs; the paired geometry and the families/native probe gates are C++,
//! called by their MSVC names.

use crate::early_depth_native::Group;
use crate::geometry_packets::Ticket;
use crate::gpu_probe_runtime::GroupContext;
use crate::source_cbv_observe::Annotation;
use core::ffi::c_void;
use core::ptr::null_mut;
use core::sync::atomic::Ordering::*;
use windows::core::Interface;

#[allow(non_upper_case_globals)]
unsafe extern "C" {
    #[link_name = "cvr_nsf2_enabled"]
    fn families_enabled() -> bool;
    /// The source SRV setters' thunks (asm_thunks/native_source_srv_binding_thunk.rs) and their originals
    /// (src/Render/NativeSourceSrvBinding.cpp).
    fn CvrSourceSrvIndividualDetour();
    fn CvrSourceSrvCachedDetour();
    static mut CvrSourceSrvIndividualOriginal: *mut c_void;
    static mut CvrSourceSrvCachedOriginal: *mut c_void;
}

/// The engine's own functions, written by the install pass.
pub(crate) static mut BIND_CAMERA_ORIGINAL: *mut c_void = null_mut();
pub(crate) static mut BIND_MATERIAL_ORIGINAL: *mut c_void = null_mut();
pub(crate) static mut RENDER_PACKETS_ORIGINAL: *mut c_void = null_mut();
type BindCamera = unsafe extern "system" fn(u32, u32, u32) -> i64;
type BindMaterial = unsafe extern "system" fn(u32, u32, u8) -> i64;
type RenderPackets = unsafe extern "system" fn(*mut c_void, *const c_void, *const c_void) -> i64;

fn base() -> usize {
    // SAFETY: set once at install.
    unsafe { crate::stereo_shared::g_exe_base as usize }
}
fn tls() -> &'static crate::render_tls::RenderTls {
    // SAFETY: this thread's block.
    unsafe { &*crate::render_tls::cvr_render_tls() }
}
/// source_cbv::ObserveBindMask / ObserveBindStage, each annotation's CBV written to the descriptor audit (its origin's
/// owner let go afterwards).
fn annotate(which: u32, reg: u32, handle: u32, stages: u32) {
    // SAFETY: plain data, all zero the empty annotation.
    let mut raw: [Annotation; 2] = unsafe { core::mem::zeroed() };
    // SAFETY: room for two annotations.
    let count = unsafe { crate::source_cbv_observe::cvr_scbo_bind(which, reg, handle, stages, raw.as_mut_ptr()) } as usize;
    for a in &raw[..count.min(2)] {
        // SAFETY: the annotation's view and origin.
        unsafe {
            crate::descriptor_audit::cvr_sda_audit_write(
                3,
                null_mut(),
                (&raw const a.view) as usize,
                a.cpu,
                a.origin.cookie,
                a.origin.id,
                a.origin.owner,
                null_mut(),
            )
        };
    }
    for a in &raw[..count.min(2)] {
        if !a.origin.owner.is_null() {
            // SAFETY: an owned reference the bind handed out.
            drop(unsafe { windows::core::IUnknown::from_raw(a.origin.owner) });
        }
    }
}

/// The bounded camera binding (0x337968): after the engine's bind, the source CBVs, early depth, paired geometry and
/// the camera upload's native bind.
unsafe extern "system" fn bind_camera_probe(binding: u32, handle: u32, stages: u32) -> i64 {
    // SAFETY: the original the install pass wrote.
    let original: BindCamera = unsafe { core::mem::transmute(BIND_CAMERA_ORIGINAL) };
    // SAFETY: the engine's own call.
    let result = unsafe { original(binding, handle, stages) };
    annotate(0, binding, handle, stages);
    crate::early_depth_native::cvr_ned_bound_camera(binding, handle, stages);
    let t = tls();
    // SAFETY: plain C++ calls.
    unsafe {
        paired_bound_camera(t.view_side, binding, handle, stages);
        if crate::early_depth_exports::CyberpunkVR_EarlyDepthNativeCamera.load(Relaxed) != 0 || paired_needs_camera() {
            crate::camera_upload::cvr_ncu_observe_bind(base() as u64, binding, handle, stages);
        }
    }
    if crate::declared_moved_exports::CyberpunkVR_SinglePassTwinUpload.load(Relaxed) != 0 {
        let b = base();
        crate::camera_upload::cvr_ncu_after_bind(
            b as u64,
            binding,
            handle,
            stages,
            t.view_side == 1 && t.current_node_work == b + 0x33DDA4,
        );
    }
    result
}
extern "C" fn want_native_camera_probe() -> bool {
    // SAFETY: a plain C++ query (families::Enabled); native_probe::Enabled is stereo_probe_creation.rs'.
    crate::stereo_probe_creation::enabled() || unsafe { families_enabled() }
}

/// The native material binding (0x336B54): after the engine's bind, the material binding and the source CBVs.
unsafe extern "system" fn bind_material_probe(binding: u32, handle: u32, stage: u8) -> i64 {
    // SAFETY: the original the install pass wrote.
    let original: BindMaterial = unsafe { core::mem::transmute(BIND_MATERIAL_ORIGINAL) };
    // SAFETY: the engine's own call.
    let result = unsafe { original(binding, handle, stage) };
    crate::material_observe::cvr_mbind_observe_bind(base() as u64, binding, handle, u32::from(stage));
    annotate(1, binding, handle, u32::from(stage));
    result
}
extern "C" fn want_native_material_probe() -> bool {
    // SAFETY: the process image's base.
    let game = unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(core::ptr::null()) } as u64;
    want_native_camera_probe() && crate::material_observe::cvr_mbind_compatible(game)
}
extern "C" fn want_native_source_srv() -> bool {
    let name: Vec<u16> = "CVR_STEREO_SOURCE_SELECTORS".encode_utf16().chain(Some(0)).collect();
    // SAFETY: a terminated name; the length only.
    let set = unsafe { windows_sys::Win32::System::Environment::GetEnvironmentVariableW(name.as_ptr(), null_mut(), 0) } != 0;
    // SAFETY: plain C++ query.
    set && crate::stereo_probe::cvr_observe_bindings()
        && unsafe { families_enabled() }
        && crate::source_srv_observe::cvr_ssrvo_compatible()
        && crate::source_srv_observe::cvr_ssrvo_configure_xstate()
}

/// The bounded native geometry packets (0x334CC0): the deferred present waited, then the stage replay's consumer (which
/// may skip a replayed stage with its probes) or the probes around the engine's consumer.
unsafe extern "system" fn render_packets_probe(renderer: *mut c_void, arguments: *const c_void, span: *const c_void) -> i64 {
    crate::submit_chain::CyberpunkVR_WaitDeferredPresentForPackets();
    if crate::stage_replay_runtime::cvr_srt_active() {
        let t = tls();
        let b = base();
        let node = if b != 0 && t.current_node_work > b { (t.current_node_work - b) as u32 } else { 0 };
        // SAFETY: the engine's arguments; the inner consumer below.
        return unsafe { crate::stage_replay_runtime::cvr_srt_consume(t.view_side, node, renderer, arguments, span, render_packets_inner) };
    }
    // SAFETY: as above.
    unsafe { render_packets_inner(renderer, arguments, span) }
}
unsafe extern "C" fn render_packets_inner(renderer: *mut c_void, arguments: *const c_void, span: *const c_void) -> i64 {
    // SAFETY: the original the install pass wrote.
    let original: RenderPackets = unsafe { core::mem::transmute(RENDER_PACKETS_ORIGINAL) };
    let group = crate::native_state_exports::CyberpunkVR_StereoGpuProbeGroup.load(Relaxed) != 0;
    let pairing = crate::native_state_exports::CyberpunkVR_PreparationState.load(Relaxed) == 1
        || crate::native_state_exports::CyberpunkVR_PreparationRunState.load(Acquire) == 1;
    if crate::declared_moved_exports::CyberpunkVR_GeometryPacketState.load(Relaxed) != 1 && !group && !pairing {
        // SAFETY: the engine's own call.
        return unsafe { original(renderer, arguments, span) };
    }
    let t = tls();
    let side = t.view_side;
    let b = base();
    let node = if b != 0 && t.current_node_work > b { (t.current_node_work - b) as u32 } else { 0 };
    let mut ticket = Ticket { epoch: 0, index: u32::MAX };
    // SAFETY: the engine's arguments.
    unsafe { crate::geometry_packets::cvr_gp_begin(node, side, renderer, arguments, span, &mut ticket) };
    // The preparation's draw scope (its members' defaults, as the C++ class's), left in place until it is closed below.
    let mut scope = crate::preparation_trace::DrawScope {
        previous: null_mut(),
        source: crate::preparation_trace::Record::default(),
        capture_epoch: 0,
        lists: [0; 64],
        draws: [0; 64],
        count: 0,
        valid: false,
    };
    // SAFETY: a scope that stays put until it is closed below.
    unsafe { crate::preparation_trace::cvr_prep_draw_scope(0, &mut scope, node, side, arguments as u64) };
    // SAFETY: plain data; written by the group's start.
    let mut previous: GroupContext = unsafe { core::mem::zeroed() };
    if group {
        let mut plane = u32::MAX;
        let mut read = 0usize;
        if !arguments.is_null() && node == 0x33DDA4 {
            // SAFETY: a read the process may refuse.
            unsafe {
                windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory(
                    windows_sys::Win32::System::Threading::GetCurrentProcess(),
                    arguments.cast::<u8>().add(0x10).cast(),
                    (&raw mut plane).cast(),
                    4,
                    &mut read,
                )
            };
        }
        // SAFETY: a local context.
        unsafe { crate::gpu_probe_runtime::cvr_sgpw_begin_group(if read == 4 { side } else { -1 }, plane, &mut previous) };
    }
    let mut early = Group::default();
    // SAFETY: the engine's arguments; a local group.
    unsafe { crate::early_depth_native::cvr_ned_begin(node, side, renderer as u64, arguments as u64, span as u64, &mut early) };
    // SAFETY: the engine's own call.
    let result = unsafe { original(renderer, arguments, span) };
    // SAFETY: the group, context and scope opened above.
    unsafe {
        crate::early_depth_native::cvr_ned_end(&early);
        if group {
            crate::gpu_probe_runtime::cvr_sgpw_end_group(&previous);
        }
    }
    crate::geometry_packets::cvr_gp_end(ticket.epoch, ticket.index);
    // SAFETY: as above.
    unsafe { crate::preparation_trace::cvr_prep_draw_scope(1, &mut scope, 0, 0, 0) };
    result
}
extern "C" fn want_native_packets_probe() -> bool {
    // SAFETY: the process image's base.
    let game = unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(core::ptr::null()) } as u64;
    let compatible = crate::code_tables::cvr_packets_prologue(game);
    crate::declared_moved_exports::CyberpunkVR_GeometryPacketHookCompatible.store(if compatible { 1 } else { 2 }, Release);
    compatible
}

/// The probes' registrations (engine_detours.rs).
pub(crate) fn register(detour: fn(&'static str, usize, *const (), *mut c_void, Option<cvr_engine::detours::WantedFn>)) {
    detour(
        "[single-pass] bounded camera binding probe",
        0x337968,
        bind_camera_probe as *const (),
        (&raw mut BIND_CAMERA_ORIGINAL).cast(),
        Some(want_native_camera_probe),
    );
    detour(
        "[single-pass] native material binding",
        0x336B54,
        bind_material_probe as *const (),
        (&raw mut BIND_MATERIAL_ORIGINAL).cast(),
        Some(want_native_material_probe),
    );
    detour(
        "[single-pass] current buffer SRV",
        0x338FC0,
        CvrSourceSrvIndividualDetour as *const (),
        (&raw mut CvrSourceSrvIndividualOriginal).cast(),
        Some(want_native_source_srv),
    );
    detour(
        "[single-pass] current cached SRV group",
        0x338ED4,
        CvrSourceSrvCachedDetour as *const (),
        (&raw mut CvrSourceSrvCachedOriginal).cast(),
        Some(want_native_source_srv),
    );
    detour(
        "[single-pass] bounded native geometry packets",
        0x334CC0,
        render_packets_probe as *const (),
        (&raw mut RENDER_PACKETS_ORIGINAL).cast(),
        Some(want_native_packets_probe),
    );
}

// paired_geometry::BoundCamera / NeedsCamera (formerly src/Render/NativePairedGeometry.cpp): paired_geometry_native.rs.
unsafe fn paired_bound_camera(side: i32, binding: u32, handle: u32, stages: u32) {
    crate::paired_geometry_native::bound_camera(side, binding, handle, stages);
}
unsafe fn paired_needs_camera() -> bool {
    crate::paired_geometry_native::needs_camera()
}
