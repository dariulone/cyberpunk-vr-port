//! THE XR FRAME LOOP'S CYCLE (formerly src/Runtimes/OpenXRFrameLoop.cpp OpenXRManager::FrameThreadMain): the session's
//! events and the auto calibration, the SteamVR startup wait for the first presented backbuffer, the old pacing wait
//! for a new captured frame (behind CyberpunkVR_XrPaceByRuntime = 0), xrWaitFrame and xrBeginFrame, the locate
//! target, the simulator's recenter, the menu rect and its follow reset, the submit's resources, the views, the head
//! and the hands, then the cycle's submission (xr_submit.rs), the cycle's rates and the frame-sync event.
//!
//! OpenXRManager's members come in by pointer (the host), read where the cycle reads them -- those the Rust half owns
//! (the session's and its spaces' handles, the submission's state, the presented frame under the present lock, what
//! the runtime reported, the tracking origin, the external reset's serial, the gaze centre) are read and written in
//! cvr-xr's storage, the session's events are polled in cvr-xr's events.rs, the submit's resources are ensured in
//! xr_submit.rs, the asymmetric projection's window parameters taken in asym_projection.rs and the world and eye scales
//! read in the live controls; what is still C++ -- the PSVR2 haptic heartbeat -- comes in through a callback.
use core::ffi::c_void;
use core::sync::atomic::Ordering::*;

use cvr_gpu::framegen_core as core_fg;
use cvr_host::log;
use cvr_xr::input::UiHand;
use openxr_sys as xr;
use windows::Win32::Foundation::{HANDLE, WAIT_TIMEOUT};
use windows::Win32::System::Threading::{SetEvent, Sleep, WaitForSingleObject};

use crate::depth_layer::DepthIn;
use crate::xr_submit::CycleSubmitIn;

/// OpenXRManager's members and the C++ half's callbacks for the cycle.
#[repr(C)]
pub struct FrameHost {
    pub ctx: *mut c_void,
    /// g_verboseLog.
    pub verbose: *const i32,
    pub quads: [*mut c_void; 5],
    pub framegen_overlay: *mut c_void,
    pub settings_panel: *mut c_void,
    /// The PSVR2 Toolkit bridge's haptic channel heartbeat (cvr::psvr2::Haptics().Heartbeat()).
    pub heartbeat: extern "C" fn(ctx: *mut c_void),
}
const _: () = assert!(
    core::mem::size_of::<FrameHost>() == 80
        && core::mem::offset_of!(FrameHost, quads) == 16
        && core::mem::offset_of!(FrameHost, heartbeat) == 72
);

/// cvr::vrui::Tracking (include/Overlay/VrInteraction.hpp): this cycle's head and hands for the overlay.
#[repr(C)]
struct UiTracking {
    head: xr::Posef,
    hands: [UiHand; 2],
    valid: bool,
    origin: u64,
    time: i64,
}
const _: () = assert!(
    core::mem::size_of::<UiTracking>() == 136 && core::mem::offset_of!(UiTracking, valid) == 116
);
const IDENTITY: xr::Posef = xr::Posef {
    orientation: xr::Quaternionf {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 1.0,
    },
    position: xr::Vector3f {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    },
};

extern "C" {
    fn GetXrRuntimeMode() -> i32;
    fn GetMenuRectMode() -> i32;
    fn GetMenuMode() -> i32;
    static CyberpunkVR_XrPaceByRuntime: i32;
    static CyberpunkVR_AsymProjection: i32;
}
fn read<T: Copy>(v: *const T) -> T {
    // SAFETY: a live export or member, read as it changes.
    unsafe { core::ptr::read_volatile(v) }
}
fn set_event(event: *mut c_void) {
    if !event.is_null() {
        // SAFETY: the manager's event.
        let _ = unsafe { SetEvent(HANDLE(event)) };
    }
}
/// cvr::engine_stereo::OnePicture: engine stereo is on but draws one picture (not rendering, or a loading screen).
fn one_picture() -> bool {
    crate::engine_stereo::cvr_engine_stereo_enabled()
        && (!crate::engine_stereo::cvr_engine_stereo_rendering()
            || crate::engine_stereo::cvr_engine_stereo_loading_shown())
}

/// One cycle of the frame loop (FrameThreadMain); 0.
///
/// # Safety
/// `host` is valid: its pointers are OpenXRManager's members, alive for the manager.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_frame_thread_main(host: *const FrameHost) -> u32 {
    // SAFETY: the caller's host.
    let Some(h) = (unsafe { host.as_ref() }) else {
        return 0;
    };
    cvr_xr::calibration::cvr_xr_frame_loop_startup();
    let shared = cvr_xr::events::cvr_xr_session_shared_ref();
    if shared.stop_frame_thread.load(Relaxed) {
        return 0;
    }
    // SAFETY (whole body): the host's members and callbacks.
    unsafe { cycle(h, shared) };
    0
}

unsafe fn cycle(h: &FrameHost, shared: &cvr_xr::events::SessionShared) {
    // SAFETY (whole body): as cvr_xr_frame_thread_main.
    unsafe {
        let (instance, _) = cvr_xr::view_configs::instance_and_system();
        cvr_xr::events::cvr_xr_poll_events(instance, cvr_xr::frame::session_handles().0);
        let (session, _, view_space) = cvr_xr::frame::session_handles();
        cvr_xr::calibration::cvr_xr_tick_auto_calibration(session, view_space);

        if !shared.running.load(Relaxed) {
            Sleep(10);
            return;
        }

        // SteamVR: the frame loop waits for the first present's backbuffer.
        if GetXrRuntimeMode() == 1 {
            let size = {
                let presented = &cvr_xr::presented::PRESENTED;
                let _held = presented.lock.lock();
                [*presented.width.get(), *presented.height.get(), *presented.format.get()]
            };
            if size.contains(&0) {
                // The frame loop's counter starts again every cycle: every waiting cycle says so.
                if read(h.verbose) != 0 {
                    log!(
                        "OpenXRManager: SteamVR startup wait. Deferring frame loop until first present provides a backbuffer. width={} height={} format={}\n",
                        size[0],
                        size[1],
                        size[2]
                    );
                }
                Sleep(1);
                return;
            }
        }

        // The old pacing wait, kept behind the flag (CyberpunkVR_XrPaceByRuntime = 0): a cycle only for a new captured
        // frame. Never at startup -- until the first submit the loop must run so the views are located and the
        // Present hook can capture its first frame -- and never on the event alone, which can be an already consumed
        // frame's: only a changed serial goes on.
        let submission = &cvr_xr::submit_state::SUBMIT;
        let runtime = &cvr_xr::runtime_info::RUNTIME;
        let mono_present_event = submission.mono_present_event();
        if read(&raw const CyberpunkVR_XrPaceByRuntime) == 0
            && !core_fg::cvr_framegen_enabled()
            && submission.mono_submit_enabled.load(Relaxed)
            && !mono_present_event.is_null()
        {
            let mut latest = crate::xr_capture_state::cvr_xr_capture_serial();
            let last_submitted = crate::xr_submit::cvr_xr_submit_last_serial();
            if last_submitted != 0 && latest == last_submitted {
                while !shared.stop_frame_thread.load(Relaxed) {
                    let waited = WaitForSingleObject(HANDLE(mono_present_event), 10);
                    latest = crate::xr_capture_state::cvr_xr_capture_serial();
                    if latest != 0 && latest != last_submitted {
                        break;
                    }
                    if !shared.running.load(Relaxed) {
                        break;
                    }
                    if waited == WAIT_TIMEOUT {
                        Sleep(1);
                    }
                }
                if latest == 0 || latest == last_submitted {
                    set_event(cvr_xr::submit_state::SUBMIT.frame_sync_event());
                    return;
                }
            }
        }

        let mut frame: cvr_xr::cycle::CycleFrame = core::mem::zeroed();
        if !cvr_xr::cycle::cvr_xr_wait_frame(&mut frame) {
            set_event(cvr_xr::submit_state::SUBMIT.frame_sync_event());
            Sleep(10);
            return;
        }
        cvr_xr::xr_trace::stage(cvr_xr::xr_trace::STAGE_WAITED);
        // The PSVR2 Toolkit bridge reads the port's haptic channel only while it beats.
        (h.heartbeat)(h.ctx);
        if frame.display_period > 0 {
            core_fg::cvr_framegen_display_period(frame.display_period, cvr_xr::runtime_info::RUNTIME.steamvr.load(Relaxed));
            submission.predicted_display_period_ns.store(frame.display_period, Relaxed);
            // The headset's refresh and the game's FPS lock target for the VRIK mechanic (shared [225], [226]).
            crate::cpp_services::cvr_cpp_xr_set_shared_slot(225, core_fg::display_hz() as f32);
            crate::cpp_services::cvr_cpp_xr_set_shared_slot(226, core_fg::target_game_hz() as f32);
        }
        cvr_xr::cycle::cvr_xr_begin_frame();
        cvr_xr::xr_trace::stage(cvr_xr::xr_trace::STAGE_BEGUN);

        let target = cvr_xr::frame::cvr_xr_cycle_locate_target(
            frame.display_time,
            frame.display_period,
            cvr_xr::presented::PRESENTED.present_count.load(Relaxed),
            core_fg::cvr_framegen_enabled(),
        );
        let locate_time = target.time;
        let hand_locate_time = locate_time;

        cvr_xr::simulator_recenter::cvr_xr_poll_simulator_recenter();
        let external_reset_serial = cvr_xr::published::RESET.gate.serial();
        let mut view_count = 0u32;
        let mono_enabled = submission.mono_submit_enabled.load(Relaxed);
        // The boot's notice is one picture in space, the projection's: not the menu quad.
        let notice_frame = crate::xr_capture_state::cvr_xr_capture_one_turn();
        let menu_rect_active = (GetMenuRectMode() != 0 || GetMenuMode() != 0 || one_picture())
            && !crate::menu_lobby::cvr_menu_lobby_world_shown()
            && !crate::menu_lobby::cvr_menu_lobby_loading_in_space()
            && !notice_frame;
        crate::menu_lobby::trace_boot_text(|| format!(
            "xr menu_rect {menu_rect_active} (rect {} menu {} one {}) world_shown {} loading_in_space {} notice_frame {notice_frame}",
            GetMenuRectMode(),
            GetMenuMode(),
            one_picture(),
            crate::menu_lobby::cvr_menu_lobby_world_shown(),
            crate::menu_lobby::cvr_menu_lobby_loading_in_space()
        ));
        if !menu_rect_active {
            let menu = &mut *cvr_xr::menu::cvr_xr_menu_shared();
            menu.follow.valid = false;
            menu.follow.following = false;
            menu.follow.reset_delay();
            menu.clock.last = 0;
            menu.clock.elapsed = 0.0;
            menu.eye_anchor_valid = false;
        }
        let mono_ready =
            mono_enabled && crate::xr_submit::ensure_mono_submit() && crate::xr_submit::cvr_xr_submit_eye_count() != 0;
        let views_len = *(*cvr_xr::views::cvr_xr_views_shared()).count.get();
        let asym_active = read(&raw const CyberpunkVR_AsymProjection) == 1;
        if mono_ready && views_len != 0 {
            let (guard, window_target) = crate::asym_projection::window_params(true);
            let located =
                cvr_xr::views::cvr_xr_locate_views(locate_time, asym_active, guard, window_target);
            view_count = located.count;
            cvr_xr::xr_trace::stage(cvr_xr::xr_trace::STAGE_LOCATED);
            if located.stats {
                runtime.store_view_stats(located.hfov_deg, located.vfov_deg, located.ipd);
                cvr_xr::gaze::publish(located.gaze[0], located.gaze[1]);
            }
        }

        // THE HEAD at this cycle's target, the base, the filter and the published samples (cvr_xr::head).
        let head_in = cvr_xr::head::CycleHeadIn {
            locate_time,
            hand_locate_time,
            reset_serial: external_reset_serial,
            building_serial: target.building_serial,
            session_state: shared.state.load(Relaxed),
        };
        let mut head: cvr_xr::head::CycleHead = core::mem::zeroed();
        cvr_xr::head::cvr_xr_locate_cycle_head(&head_in, &mut head);
        let head_pose: xr::Posef = core::mem::transmute(head.location);
        if head.located {
            // THE HANDS and the overlay's tracking (cvr_xr::cycle).
            let mut ui = UiTracking {
                head: head_pose,
                hands: core::array::from_fn(|_| UiHand {
                    aim: IDENTITY,
                    valid: false,
                    stick_click: false,
                    trigger: 0.0,
                    grip: 0.0,
                    stick_y: 0.0,
                }),
                valid: true,
                origin: cvr_xr::tracking::origin_serial(),
                time: frame.display_time,
            };
            let hands_ptr = ui.hands.as_mut_ptr();
            cvr_xr::xr_trace::stage(cvr_xr::xr_trace::STAGE_INPUT);
            let hands_in = cvr_xr::cycle::CycleHandsIn {
                display_time: frame.display_time,
                hand_time: hand_locate_time,
                base: head.base,
                base_inv: head.base_inv,
                filtered: head.filtered,
                hand_head: head.hand_head,
                psvr2: runtime.psvr2(),
                simulator: runtime.simulator(),
                ui_tracking: (&raw mut ui).cast(),
                ui_hands: hands_ptr,
            };
            cvr_xr::cycle::cvr_xr_cycle_hands(&hands_in);
            const LINEAR: u64 = 1;
            const ANGULAR: u64 = 2;
            cvr_xr::tracking::cvr_xr_publish_head_velocity(
                head.base_inv,
                &head.angular_velocity,
                &head.linear_velocity,
                head.velocity_flags & ANGULAR != 0 && head.velocity_flags & LINEAR != 0,
            );
        }

        // THE CYCLE'S SUBMISSION (xr_submit.rs): an accepted frame ends the cycle.
        let depth = DepthIn::live();
        let (session, local_space, view_space) = cvr_xr::frame::session_handles();
        let submit = CycleSubmitIn {
            session,
            local_space,
            view_space,
            device: read(submission.device.get()),
            queue: read(submission.queue.get()),
            display_time: frame.display_time,
            display_period: frame.display_period,
            predicted_period_ns: submission.predicted_display_period_ns.load(Relaxed),
            wait_enter_ms: frame.wait_enter_ms,
            wait_ms: frame.wait_ms,
            head_pose,
            tracking_origin: cvr_xr::tracking::origin_serial(),
            quads: h.quads,
            framegen_overlay: h.framegen_overlay,
            settings_panel: h.settings_panel,
            frame_sync_event: submission.frame_sync_event(),
            view_count,
            views_len,
            runtime_ipd: runtime.ipd.load(),
            depth,
            mono_ready,
            mono_enabled,
            should_render: frame.should_render,
            menu_rect_active,
            head_located: head.located,
            gameplay: !menu_rect_active && shared.running.load(Relaxed),
            asym_active,
        };
        if crate::xr_submit::cvr_xr_cycle_submit(&submit) {
            return;
        }
        cvr_xr::xr_rates::cvr_xr_accumulate_cycle(
            frame.wait_enter_ms,
            frame.wait_ms,
            frame.display_period,
        );
        set_event(cvr_xr::submit_state::SUBMIT.frame_sync_event());
    }
}
