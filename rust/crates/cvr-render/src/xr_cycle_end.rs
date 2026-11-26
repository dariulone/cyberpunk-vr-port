//! THE XR CYCLE'S COPIES AND ITS END (formerly src/Runtimes/OpenXRFrameLoop.cpp FrameThreadMain, from the submit
//! list's reset to the accepted frame's bookkeeping). The copies (cycle_copies): the list reset, the GPU stage
//! profile, the eyelids and the lobby's curtain sampled once, the eyes' pictures chosen, the last copy reused when the
//! same captured frame repeats, each eye copied into its swapchain image and its projection view composed, the depth
//! layer (depth_layer.rs), the copies submitted and the images released. The end (cycle_end): the
//! depth infos dropped when the depth images were not released, the frame's
//! layers -- the menu quad (a pause menu on its quad) or the projection, the HUD channels, the frame generation's
//! statistics panel, the VR menu's panel and its ray -- one layer only while the menu blinks or the lobby's curtain is
//! shut, xrEndFrame, and for an accepted frame: the last copy kept for reuse, the submission recorded for resubmits,
//! the cycle's counters, the frame generation told, the captured reads released, the frame-sync event set.
//!
//! The submit's resources and the cycle's records are xr_submit.rs's, which builds the cycle's input; the frame
//! generator's selection is the presenter's (framegen_presenter.rs), the captured reads' leases the capture state's
//! (xr_capture_state.rs).
use core::ffi::c_void;

use cvr_crt::{format, Arg};
use openxr_sys as xr;
use xr::Handle as _;

/// OpenXRManager::LastCopy: the last frame's copies into the swapchains, reused while the same captured frame repeats.
#[repr(C)]
pub struct LastCopy {
    pub valid: bool,
    pub serial: u64,
    pub source: [*mut c_void; 2],
    pub color: [u64; 2],
    pub depth_used: bool,
    pub depth_info: [xr::CompositionLayerDepthInfoKHR; 2],
}
const _: () = assert!(
    core::mem::size_of::<LastCopy>() == 184 && core::mem::offset_of!(LastCopy, depth_info) == 56
);

/// What the cycle's end needs of the cycle (pointers to the submit's records, xr_submit.rs).
#[repr(C)]
pub struct CycleEndIn {
    /// cvr_xr_submit_copies' results: the copies' images released, the depth images released.
    pub release_ok: bool,
    pub depth_ok: bool,
    pub use_depth_layer: bool,
    pub menu_rect_active: bool,
    pub head_located: bool,
    pub should_render: bool,
    pub reuse_images: bool,
    pub fg_selected: bool,
    pub fg_synthetic: bool,
    pub curtain_shut: bool,
    pub menu_blink: f32,
    pub view_count: u32,
    pub projection_views: *mut xr::CompositionLayerProjectionView,
    /// The head located this cycle (location.pose) and the submitted left view's capture orientation.
    pub head_pose: xr::Posef,
    pub mono_pose0: xr::Quaternionf,
    pub display_time: i64,
    pub display_period: i64,
    pub wait_enter_ms: f64,
    pub wait_ms: f64,
    pub mono_capture_ms: f64,
    pub predicted_period_ns: i64,
    pub session: u64,
    pub local_space: u64,
    pub view_space: u64,
    pub device: *mut c_void,
    pub queue: *mut c_void,
    /// m_eyeSwapchains[0]'s size, and the eyes' colour swapchains (0 past the view count).
    pub eye_width: i32,
    pub eye_height: i32,
    pub color: [u64; 2],
    pub acquired_eyes: [bool; 2],
    pub eye_sources: [*mut c_void; 2],
    pub present_serial: u64,
    pub mono_pose_ids: [u64; 2],
    pub quads: [*mut cvr_xr::hud_quad::Quad; 5],
    pub framegen_overlay: *mut cvr_xr::framegen_overlay::Overlay,
    pub settings_panel: *mut cvr_xr::vr_panel::Panel,
    pub tracking_origin: u64,
    pub last_copy: *mut LastCopy,
    pub last_submission: *mut cvr_xr::resubmit::LastSubmission,
    pub last_submitted_serial: *mut u64,
    pub frame_sync_event: *mut c_void,
    pub mono_source: *mut c_void,
    pub vrcam_eye: *mut c_void,
    pub captured_read_fence: u64,
    /// The queue the eye and depth copies run on (cvr::capture::SubmissionQueue, main's 0.2.4): the synthesis queue for
    /// a generated image, the runtime's for a finished capture, the game's otherwise. `queue` is the runtime's queue:
    /// the layers' own copies and the submit fence go there.
    pub copy_queue: *mut c_void,
    /// The presenter's selection and generation (ms), for the cycle trace.
    pub select_ms: f64,
}

/// SteamVR: which cycles take only a FINISHED capture on the runtime's queue (bit 0 with frame generation, bit 1
/// without); the others take the newest capture and order the runtime's queue after the game's. Measured 2026-11-13,
/// Quest 3, generation on, 30 s H10 sprint: the newest capture is 21-23 ms capture-to-submit against 33-39 ms, but
/// SteamVR then waits for the game's frame and drops display cycles, and the world jerked in locomotion two to five
/// times as often. Finished captures by default with generation as well as without it (main's 0.2.4).
#[no_mangle]
pub static mut CyberpunkVR_SteamVrFinishedCapture: i32 = 3;
/// SteamVR with generation from finished captures: one finished real frame kept in hand -- the older one not shown yet
/// is taken even when the newest is finished too, so frames go out in order on an even beat (0 = newest finished
/// first).
#[no_mangle]
pub static mut CyberpunkVR_SteamVrFrameBuffer: i32 = 1;

extern "C" {
    fn GetMenuRectMode() -> i32;
    fn GetMenuMode() -> i32;
    fn GetMenuFov() -> f32;
    fn tanf(x: f32) -> f32;
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static CyberpunkVR_DebugXrEndFailed: core::sync::atomic::AtomicU64;
}
extern "system" {
    fn SetEvent(event: *mut c_void) -> i32;
}
fn diagnostics() -> bool {
    // SAFETY: an export of the C++ half.
    unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_RuntimeDiagnostics) != 0 }
}
fn layer<T>(l: &T) -> *const xr::CompositionLayerBaseHeader {
    (l as *const T).cast()
}

/// A captured read ended (OpenXRManager::ReleaseCapturedRead): the cycle's reference handed over to the capture state,
/// released once the submit fence passed `fence`.
///
/// # Safety
/// `resource` carries the cycle's reference; `submit` is the live submit fence or null.
pub(crate) unsafe fn release_captured_read(submit: *mut c_void, resource: *mut c_void, fence: u64) {
    use windows::core::Interface as _;
    if resource.is_null() {
        return;
    }
    // SAFETY: the caller's live fence.
    let completed =
        unsafe { windows::Win32::Graphics::Direct3D12::ID3D12Fence::from_raw_borrowed(&submit) }
            .map_or(u64::MAX, |f| unsafe { f.GetCompletedValue() });
    // SAFETY: the reference handed over.
    unsafe { crate::xr_capture_state::cvr_xr_capture_release_read(resource, fence, completed) };
}

/// The cycle's end after the copies, for a frame whose copies were ready. True when the frame was accepted (the frame
/// loop continues with the next cycle); false when it was not (the loop goes on to its empty end). `fence` is the submit
/// fence, `depth` the cycle's depth layer choice (for its report).
///
/// # Safety
/// `c` is the cycle's record as documented: `projection_views` holds `view_count` views, the pointers are
/// OpenXRManager's members, the quads, overlay and panel the manager's Rust objects.
unsafe fn cycle_end(c: &CycleEndIn, fence: *mut c_void, depth: &crate::depth_layer::Cycle) -> bool {
    // SAFETY (whole body): as the caller promises.
    unsafe {
        let views = core::slice::from_raw_parts_mut(c.projection_views, c.view_count as usize);
        let last_copy = &mut *c.last_copy;
        let use_depth_layer = c.use_depth_layer && c.depth_ok;
        if !use_depth_layer {
            for v in views.iter_mut() {
                v.next = core::ptr::null();
            }
        }
        if !c.release_ok {
            last_copy.valid = false;
            return false;
        }

        // The HUD quads' Prepare below may recreate their swapchains: from here on the record of the last accepted
        // frame may name a destroyed one, until this cycle's own end succeeds and writes a new record.
        (*c.last_submission).valid = false;
        let mut layer_proj: xr::CompositionLayerProjection = core::mem::zeroed();
        layer_proj.ty = xr::StructureType::COMPOSITION_LAYER_PROJECTION;
        let mut layer_quad: xr::CompositionLayerQuad = core::mem::zeroed();
        layer_quad.ty = xr::StructureType::COMPOSITION_LAYER_QUAD;
        const LAYERS: usize = 1 + 2 * 6 + 3;
        let mut layers: [*const xr::CompositionLayerBaseHeader; LAYERS] =
            [core::ptr::null(); LAYERS];

        if c.menu_rect_active {
            layer_quad.space = xr::Space::from_raw(c.local_space);
            layer_quad.eye_visibility = xr::EyeVisibility::BOTH;
            layer_quad.sub_image = views[0].sub_image;
            cvr_xr::menu::cvr_xr_menu_quad_pose(
                c.head_located,
                (&c.head_pose as *const xr::Posef).cast(),
                c.display_time,
                (&mut layer_quad.pose as *mut xr::Posef).cast(),
            );
            let mut quad_width =
                2.0f32 * 1.5f32 * tanf(GetMenuFov() * 3.14159f32 / 180.0f32 * 0.5f32);
            let mut quad_height = quad_width;
            if crate::engine_stereo::cvr_engine_stereo_enabled() && c.eye_height != 0 {
                let window_w = 2.0f32 * c.eye_width as f32;
                let window_h = c.eye_height as f32;
                let wide = window_h * 16.0f32 / 9.0f32;
                let menu_w = if wide < window_w { wide } else { window_w };
                quad_height = quad_width * window_h / menu_w;
                quad_width *= window_w / menu_w;
            }
            layer_quad.size = xr::Extent2Df {
                width: quad_width,
                height: quad_height,
            };
            layers[0] = layer(&layer_quad);
        } else {
            layer_proj.space = xr::Space::from_raw(c.local_space);
            layer_proj.view_count = c.view_count;
            layer_proj.views = views.as_ptr();
            layers[0] = layer(&layer_proj);
        }

        let mut end_info: xr::FrameEndInfo = core::mem::zeroed();
        end_info.ty = xr::StructureType::FRAME_END_INFO;
        end_info.display_time = xr::Time::from_nanos(c.display_time);
        end_info.environment_blend_mode = xr::EnvironmentBlendMode::OPAQUE;
        end_info.layer_count = 1;
        let mut settings: cvr_gpu::framegen_core::Settings = core::mem::zeroed();
        cvr_gpu::framegen_core::cvr_framegen_get_settings(&mut settings);
        let hud_in = cvr_xr::hud_quad::HudLayersIn {
            head_located: c.head_located,
            menu_rect: c.menu_rect_active,
            framegen_overlay: settings.overlay,
            display_time: c.display_time,
            device: c.device,
            queue: c.queue,
            quads: c.quads,
            render_pose: views[0].pose,
            // HUD canvases are drawn on the game's queue; copied on the runtime's own queue they must be finished
            // first (the newest finished canvas whenever the runtime has a queue of its own, or under SteamVR).
            completed_source: cvr_xr::runtime_info::RUNTIME.steamvr.load(core::sync::atomic::Ordering::Relaxed)
                || cvr_xr::runtime_queue::has_own(),
        };
        let mut hud: cvr_xr::hud_quad::HudLayersOut = core::mem::zeroed();
        let capacity = (LAYERS - 3) as u32;
        end_info.layer_count += cvr_xr::hud_quad::cvr_xr_hud_layers(
            &hud_in,
            layers.as_mut_ptr().add(1),
            capacity - 1,
            &mut hud,
        );
        let hud_layer = hud.main;
        let hud_head = hud.head;
        // One picture without a menu (MaskNative): the panels are off but the game shows its HUD, so their slots must
        // not stay masked as for a menu.
        if c.menu_rect_active && GetMenuRectMode() == 0 && GetMenuMode() == 0 {
            for channel in 0..cvr_gpu::hud_panel::CHANNELS as u32 {
                cvr_gpu::hud_panel::cvr_hud_panel_drop_consumer(channel);
            }
        }
        let metrics = cvr_xr::framegen_overlay::cvr_xr_framegen_overlay_prepare(
            c.framegen_overlay,
            c.session,
            c.view_space,
            c.device,
            c.queue,
            c.local_space,
            if hud.tracked {
                &hud_head
            } else {
                core::ptr::null()
            },
            c.tracking_origin,
            c.display_time,
        );
        if !metrics.is_null() && !cvr_body::vr_overlay::cvr_vrui_visible() {
            layers[end_info.layer_count as usize] = metrics;
            end_info.layer_count += 1;
        }
        end_info.layers = layers.as_ptr();
        let panel = cvr_xr::vr_panel::cvr_vrui_panel_prepare(
            c.settings_panel,
            c.session,
            c.local_space,
            c.device,
            c.queue,
        );
        if !panel.is_null() {
            layers[end_info.layer_count as usize] = panel;
            end_info.layer_count += 1;
            let ray = cvr_xr::vr_panel::cvr_vrui_panel_ray(c.settings_panel);
            if !ray.is_null() {
                layers[end_info.layer_count as usize] = ray;
                end_info.layer_count += 1;
            }
        }
        if c.menu_blink > 0.0 || c.curtain_shut {
            end_info.layer_count = 1;
        }
        // Only the appended optional layers are filtered: the primary image at layers[0] can itself be a menu quad.
        end_info.layer_count = 1 + cvr_xr::hud_quad::filter_submittable_quads(layers.as_mut_ptr().add(1), end_info.layer_count - 1);
        crate::menu_lobby::trace_boot_text(|| format!(
            "end layers {} first {} curtain_shut {} blink {:.2}",
            end_info.layer_count,
            if c.menu_rect_active { "quad" } else { "projection" },
            c.curtain_shut,
            c.menu_blink
        ));
        let end_start = cvr_xr::tracking::now_ms();
        let end_res =
            cvr_xr::cycle::cvr_xr_end_frame(&end_info, c.mono_capture_ms, c.predicted_period_ns);
        let end_ms = cvr_xr::tracking::now_ms() - end_start;
        {
            let (kind, native_frame) =
                if c.fg_selected { crate::framegen_presenter::last_selection_trace() } else { (-1.0, 0.0) };
            cvr_xr::xr_trace::cycle([
                cvr_xr::tracking::now_ms(),
                c.display_time as f64,
                c.present_serial as f64,
                if c.fg_selected && c.fg_synthetic { 1.0 } else { 0.0 },
                kind,
                c.mono_capture_ms,
                if end_res >= 0 { 1.0 } else { 0.0 },
                c.display_period as f64,
                native_frame,
                c.wait_ms,
                end_ms,
                c.select_ms,
            ]);
        }
        if diagnostics() {
            crate::depth_layer::report(
                depth,
                use_depth_layer,
                end_res,
                c.present_serial,
                &c.mono_pose_ids,
            );
        }
        cvr_xr::xr_rates::cvr_xr_accumulate_cycle(c.wait_enter_ms, c.wait_ms, c.display_period);
        if end_res < 0 {
            last_copy.valid = false;
        } else if !c.reuse_images {
            // What went into the swapchains: a repeat of this captured frame may submit them again without copying.
            last_copy.valid = c.view_count == 2
                && c.acquired_eyes[0]
                && c.acquired_eyes[1]
                && c.menu_blink <= 0.0;
            last_copy.serial = c.present_serial;
            for eye in 0..2 {
                last_copy.source[eye] = c.eye_sources[eye];
                last_copy.color[eye] = if (eye as u32) < c.view_count {
                    c.color[eye]
                } else {
                    0
                };
            }
            let d0 = views[0].next as *const xr::CompositionLayerDepthInfoKHR;
            let d1 = if c.view_count > 1 {
                views[1].next as *const xr::CompositionLayerDepthInfoKHR
            } else {
                core::ptr::null()
            };
            last_copy.depth_used = use_depth_layer
                && !d0.is_null()
                && !d1.is_null()
                && (*d0).ty == xr::StructureType::COMPOSITION_LAYER_DEPTH_INFO_KHR
                && (*d1).ty == xr::StructureType::COMPOSITION_LAYER_DEPTH_INFO_KHR;
            if last_copy.depth_used {
                last_copy.depth_info = [*d0, *d1];
                last_copy.depth_info[0].next = core::ptr::null();
                last_copy.depth_info[1].next = core::ptr::null();
            }
        }
        if end_res >= 0 {
            let mut color_handles = [0u64; 2];
            for (eye, handle) in color_handles
                .iter_mut()
                .enumerate()
                .take(c.view_count.min(2) as usize)
            {
                *handle = c.color[eye];
            }
            cvr_xr::resubmit::cvr_xr_record_submission(
                c.last_submission,
                views.as_ptr(),
                c.view_count,
                c.menu_rect_active,
                c.local_space,
                c.display_time,
                color_handles.as_ptr(),
                &layer_quad,
                layers.as_ptr(),
                end_info.layer_count,
            );
            let real_submission = !c.fg_selected || !c.fg_synthetic;
            let last_submitted = *c.last_submitted_serial;
            let submitted = cvr_xr::cycle::Submitted {
                hud: !hud_layer.is_null() && !c.menu_rect_active,
                real: real_submission,
                should_render: c.should_render,
                depth: use_depth_layer,
                view_count: c.view_count,
                pose_ids: c.mono_pose_ids,
                period_ns: c.display_period,
                display_time: c.display_time,
                present_serial: c.present_serial,
                last_submitted,
                render_ori: core::mem::transmute(c.mono_pose0),
                live_ori: core::mem::transmute(c.head_pose.orientation),
            };
            cvr_xr::cycle::cvr_xr_after_submit(&submitted);
            if c.fg_selected {
                crate::framegen_presenter::cvr_framegen_presenter_submitted(
                    fence,
                    c.captured_read_fence,
                    true,
                );
            } else if cvr_gpu::framegen_core::cvr_framegen_metrics_enabled() {
                cvr_gpu::framegen_core::cvr_framegen_on_submitted(
                    c.present_serial,
                    false,
                    c.present_serial == last_submitted,
                    cvr_gpu::framegen_core::cvr_framegen_now_ms(),
                );
            }
            // A midpoint has B's endpoint serial, but it is not B's presentation: B is recorded only when its real
            // image reaches the compositor.
            if real_submission {
                *c.last_submitted_serial = c.present_serial;
                crate::xr_capture_state::LAST_SUBMITTED_SERIAL.store(c.present_serial, core::sync::atomic::Ordering::Relaxed);
            }
            release_captured_read(fence, c.mono_source, c.captured_read_fence);
            if !c.vrcam_eye.is_null() {
                release_captured_read(fence, c.vrcam_eye, c.captured_read_fence);
            }
            if !c.frame_sync_event.is_null() {
                SetEvent(c.frame_sync_event);
            }
            return true;
        }
        if diagnostics() {
            CyberpunkVR_DebugXrEndFailed.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        }
        // Not a bug in the counter: control falls through to the empty xrEndFrame at the cycle's tail, so a failed
        // submit is counted once here and once as an EMPTY end.
        cvr_host::log::write(
            &format(
                c"OpenXRManager: xrEndFrame mono submit failed (res=%d)\n",
                &[Arg::I(end_res)],
            )
            .to_string_lossy(),
        );
        false
    }
}

// ---- the copies: from the submit list's reset to the copies' submission ----------------------------------------------

/// One eye's swapchains (OpenXRManager::EyeSwapchain's fields the copies use).
#[repr(C)]
pub struct EyeSwap {
    pub handle: u64,
    pub images: *const xr::SwapchainImageD3D12KHR,
    pub image_count: u32,
    /// D3D12_CPU_DESCRIPTOR_HANDLE[rtv_count].
    pub rtvs: *const usize,
    pub rtv_count: u32,
    pub width: i32,
    pub height: i32,
}

/// The cycle's copies: the end's record (what is known before the copies; the rest is written here), the submit
/// ring's allocator and list, the eyes' swapchains, the projection's inputs, and the depth layer's inputs.
#[repr(C)]
pub struct CycleCopiesIn {
    pub end: CycleEndIn,
    /// ID3D12CommandAllocator*, ID3D12GraphicsCommandList*, ID3D12Fence* of the submit ring.
    pub allocator: *mut c_void,
    pub list: *mut c_void,
    pub fence: *mut c_void,
    /// m_fenceValue and m_cmdAllocatorFence[m_cmdAllocatorIndex].
    pub fence_value: *mut u64,
    pub allocator_fence: *mut u64,
    pub allocator_index: u32,
    /// The eye MAIN's picture belongs to.
    pub frame_main_eye: u32,
    /// The frame was presented under a loading screen: the eyelids shut.
    pub mono_dark: bool,
    /// The swapchain does not declare the eye bytes as sRGB (m_convertForSwapchain with a pass).
    pub convert: bool,
    /// cvr::framegen::Enabled(); CyberpunkVR_XrReuseImages.
    pub framegen_enabled: bool,
    pub reuse_switch: bool,
    /// The frame generator's selected pictures (with `end.fg_selected`).
    pub fg_colors: [*mut c_void; 2],
    /// EyeSwap[end.view_count].
    pub swaps: *const EyeSwap,
    /// The sRGB decode (rust/crates/cvr-gpu/src/srgb_pass.rs), or null.
    pub srgb_pass: *const c_void,
    pub projection: cvr_xr::views::ProjectionInputs,
    pub depth: crate::depth_layer::DepthIn,
}

extern "C" {
    fn cvr_vrmenu_blink_closure() -> f32;
    static CyberpunkVR_XrReusedCycles: core::sync::atomic::AtomicU64;
    static CyberpunkVR_XrProjectionCycles: core::sync::atomic::AtomicU64;
    static CyberpunkVR_XrRepeatCycles: core::sync::atomic::AtomicU64;
}
/// cvr::gpu::profile::Stage::SubmitColor.
const STAGE_SUBMIT_COLOR: u32 = 3;

unsafe extern "C" fn decode(
    pass: *mut c_void,
    list: *mut c_void,
    source: *mut c_void,
    target: *mut c_void,
    slot: u32,
) -> bool {
    // SAFETY: the cycle's pass, list and resources.
    unsafe { cvr_gpu::srgb_pass::cvr_gpu_srgb_pass_record(pass.cast(), list, source, target, slot) }
}
/// A SubmitColor scope on the cycle's profile batch (cvr::gpu::profile::Scope).
unsafe extern "C" fn scope_open(batch: *mut c_void) -> *mut c_void {
    let batch = batch as *const cvr_gpu::stage_profile::Batch;
    // SAFETY: the cycle's live batch.
    let index =
        unsafe { cvr_gpu::stage_profile::cvr_gpu_stage_scope_begin(batch, STAGE_SUBMIT_COLOR) };
    if index == u32::MAX {
        return core::ptr::null_mut();
    }
    Box::into_raw(Box::new((batch, index))).cast()
}
unsafe extern "C" fn scope_close(scope: *mut c_void) {
    // SAFETY: a scope from scope_open, closed once.
    let (batch, index) =
        *unsafe { Box::from_raw(scope as *mut (*const cvr_gpu::stage_profile::Batch, u32)) };
    // SAFETY: the cycle's batch, alive until the cycle ends.
    unsafe { cvr_gpu::stage_profile::cvr_gpu_stage_scope_end(batch, index) };
}
unsafe extern "C" fn profile_submitted(
    batch: *mut c_void,
    queue: *mut c_void,
    fence: *mut c_void,
    value: u64,
) {
    // SAFETY: the cycle's batch, the queue and fence of the submission.
    unsafe {
        cvr_gpu::stage_profile::cvr_gpu_stage_submitted(batch as *const _, queue, fence, value)
    }
}

/// The cycle with a captured frame and a free allocator: the submit list reset, the eyes copied into their swapchain
/// images (or the last copy reused while the same captured frame repeats), their projection views, the depth layer,
/// the copies submitted -- then the cycle's end (cycle_end). True when the frame was accepted (the frame loop
/// goes on with its next cycle); false when the list could not be reset, a copy failed or the end was not accepted
/// (the loop goes on to its empty end, with `end.captured_read_fence` the fence the captured pictures' reads end at).
///
/// # Safety
/// `c` as documented: live objects; `swaps` and `end.projection_views` hold `end.view_count`.
pub(crate) unsafe fn cycle_copies(c: &mut CycleCopiesIn) -> bool {
    use windows::core::Interface as _;
    use windows::Win32::Graphics::Direct3D12::{
        ID3D12CommandAllocator, ID3D12GraphicsCommandList, ID3D12PipelineState,
    };
    // SAFETY (whole body): as the caller promises.
    unsafe {
        let (Some(allocator), Some(list)) = (
            ID3D12CommandAllocator::from_raw_borrowed(&c.allocator),
            ID3D12GraphicsCommandList::from_raw_borrowed(&c.list),
        ) else {
            return false;
        };
        if allocator.Reset().is_err()
            || list.Reset(allocator, None::<&ID3D12PipelineState>).is_err()
        {
            return false;
        }
        let profile = cvr_gpu::stage_profile::cvr_gpu_stage_begin(c.list);
        let accepted = copies(c, profile.cast());
        cvr_gpu::stage_profile::cvr_gpu_stage_free(profile);
        accepted
    }
}

/// # Safety
/// As cycle_copies; `profile` the cycle's batch or null.
unsafe fn copies(c: &mut CycleCopiesIn, profile: *const c_void) -> bool {
    use core::sync::atomic::Ordering::Relaxed;
    // SAFETY (whole body): as the caller promises.
    unsafe {
        let view_count = c.end.view_count as usize;
        let swaps = core::slice::from_raw_parts(c.swaps, view_count);
        let views = core::slice::from_raw_parts_mut(c.end.projection_views, view_count);
        let last_copy = &mut *c.end.last_copy;
        let mut copy_ready = true;
        // Sampled once for both eyes, after frame generation: a repeated or generated picture must not freeze the
        // eyelids or interpolate a black capture into the world. A frame presented under a loading screen shuts them.
        // The lobby's curtain, and its intro's picture in that dark, are in the frame itself: with the curtain shut
        // only the separate quads go.
        let curtain_shut = crate::menu_lobby::curtain() >= 1.0;
        let menu_blink = if c.mono_dark {
            1.0
        } else {
            cvr_vrmenu_blink_closure()
        };
        let mut acquired_eyes = vec![false; view_count];
        let mut acquired_depth = vec![false; view_count];
        for v in views.iter_mut() {
            *v = core::mem::zeroed();
            v.ty = xr::StructureType::COMPOSITION_LAYER_PROJECTION_VIEW;
        }

        // THE SAME CAPTURED FRAME AS LAST CYCLE: its pictures are the eye swapchains' last released images already
        // (LastCopy). Not with frame generation (its output textures change content under one pointer) and not while
        // the eyelids are drawn.
        let second = (c.frame_main_eye ^ 1) as usize;
        let second_picture = !c.end.vrcam_eye.is_null();
        let source_of = |eye: usize| {
            if c.end.fg_selected {
                c.fg_colors[eye]
            } else if eye == second && second_picture {
                c.end.vrcam_eye
            } else {
                c.end.mono_source
            }
        };
        let eye_sources = [source_of(0), source_of(1)];
        let serial = c.end.present_serial;
        let repeat = last_copy.valid && serial != 0 && last_copy.serial == serial;
        let reuse_images = c.reuse_switch
            && repeat
            && view_count == 2
            && !c.framegen_enabled
            && !(c.end.fg_selected && c.end.fg_synthetic)
            && menu_blink <= 0.0
            && eye_sources == last_copy.source
            && last_copy.color[0] == swaps[0].handle
            && last_copy.color[1] == swaps[1].handle;
        if reuse_images {
            CyberpunkVR_XrReusedCycles.fetch_add(1, Relaxed);
        }
        CyberpunkVR_XrProjectionCycles.fetch_add(1, Relaxed);
        if repeat {
            CyberpunkVR_XrRepeatCycles.fetch_add(1, Relaxed);
        }
        for eye in 0..view_count {
            let swap = &swaps[eye];
            if !reuse_images {
                let copy = cvr_xr::swapchains::EyeCopy {
                    swapchain: swap.handle,
                    images: swap.images,
                    image_count: swap.image_count,
                    rtvs: swap.rtvs,
                    rtv_count: swap.rtv_count,
                    width: swap.width,
                    height: swap.height,
                    list: c.list,
                    source: source_of(eye),
                    convert: c.convert,
                    decode_slot: c.allocator_index * 2 + eye as u32,
                    blink: menu_blink,
                    second_eye: eye == second && second_picture,
                    decode: Some(decode),
                    decode_ctx: c.srgb_pass as *mut c_void,
                    scope_open: Some(scope_open),
                    scope_close: Some(scope_close),
                    profile: profile as *mut c_void,
                };
                let copied = cvr_xr::swapchains::cvr_xr_copy_eye(&copy, eye as u32);
                if copied != cvr_xr::swapchains::EYE_NOT_ACQUIRED {
                    acquired_eyes[eye] = true;
                }
                if copied != cvr_xr::swapchains::EYE_COPIED {
                    copy_ready = false;
                    break;
                }
            }
            cvr_xr::views::cvr_xr_compose_projection_view(
                &c.projection,
                eye as u32,
                swap.handle,
                swap.width as u32,
                swap.height as u32,
                &mut views[eye],
            );
        }

        // The depth infos the views point at, alive until the end has gone out.
        let mut depth_infos: [xr::CompositionLayerDepthInfoKHR; 2] = core::mem::zeroed();
        let mut depth = crate::depth_layer::Cycle::default();
        let target = crate::depth_layer::Target {
            device: c.end.device,
            list: c.list,
            queue: c.end.copy_queue,
            session: c.end.session,
            slot: c.allocator_index,
            fence: c.fence,
            value: *c.fence_value + 1,
            profile: profile.cast(),
        };
        let use_depth_layer = crate::depth_layer::cycle(
            &c.depth,
            &target,
            reuse_images,
            copy_ready,
            c.end.menu_rect_active,
            c.end.fg_selected,
            last_copy,
            views,
            &mut depth_infos,
            &mut acquired_depth,
            &mut depth,
        );
        if !use_depth_layer {
            for v in views.iter_mut() {
                v.next = core::ptr::null();
            }
        }

        let mut submit = cvr_xr::swapchains::SubmitCopies {
            copy_ready,
            view_count: c.end.view_count.min(4),
            acquired: [false; 4],
            acquired_depth: [false; 4],
            color: [0; 4],
            depth: [0; 4],
            list: c.list,
            queue: c.end.copy_queue,
            fence_queue: c.end.queue,
            fence: c.fence,
            fence_value: c.fence_value,
            allocator_fence: c.allocator_fence,
            submitted: Some(profile_submitted),
            profile: profile as *mut c_void,
        };
        for eye in 0..submit.view_count as usize {
            submit.acquired[eye] = acquired_eyes[eye];
            submit.acquired_depth[eye] = acquired_depth[eye];
            submit.color[eye] = swaps[eye].handle;
            submit.depth[eye] = crate::depth_layer::cvr_xr_depth_swapchain(eye as u32);
        }
        let mut submitted = cvr_xr::swapchains::SubmittedCopies {
            read_fence: 0,
            release_ok: false,
            depth_ok: false,
        };
        // a generated frame copied here is FSR3's compute queue's work: the copies wait for its last generation
        if c.end.fg_selected && crate::fsr3::enabled() {
            crate::fsr3::queue_wait_for_last(c.end.copy_queue);
        }
        cvr_xr::swapchains::cvr_xr_submit_copies(&submit, &mut submitted);
        c.end.captured_read_fence = submitted.read_fence;

        if !copy_ready {
            // An image released here may hold nothing of this frame: never reuse what is in the swapchains.
            last_copy.valid = false;
            return false;
        }
        c.end.release_ok = submitted.release_ok;
        c.end.depth_ok = submitted.depth_ok;
        c.end.use_depth_layer = use_depth_layer;
        c.end.reuse_images = reuse_images;
        c.end.curtain_shut = curtain_shut;
        c.end.menu_blink = menu_blink;
        for eye in 0..2 {
            c.end.acquired_eyes[eye] = eye < view_count && acquired_eyes[eye];
        }
        c.end.eye_sources = eye_sources;
        cycle_end(&c.end, c.fence, &depth)
    }
}
