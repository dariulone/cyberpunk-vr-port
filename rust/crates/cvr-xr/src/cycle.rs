//! FrameThreadMain's per-cycle steps that only glue the parts here together: the hands and the controllers of one XR
//! cycle (formerly inline in the frame loop) -- the actions synced and the vibrations played, under the hand lock the
//! hand sample's head stamped, the Vive trackers located, the hands located and the controllers read for the game.
#![cfg_attr(test, allow(dead_code, unused_imports))]
#![allow(non_upper_case_globals)]
use core::ffi::c_void;
use core::sync::atomic::{AtomicU32, Ordering};

use cvr_body::xr::{multiply_quat, rotate_vector, Quat, Vec3};

use crate::hands::Pose;
use crate::input::{ControllerState, InputActions, InputFrame, UiHand};
use std::sync::atomic::AtomicU64;

/// What the cycle's hands need from the frame loop.
#[repr(C)]
pub struct CycleHandsIn {
    /// The frame's predicted display time (the controllers and the overlay's aims are read at it).
    pub display_time: i64,
    /// The hands' locate instant.
    pub hand_time: i64,
    pub base: Pose,
    pub base_inv: Quat,
    /// The base-relative head through this cycle's filter.
    pub filtered: Pose,
    /// The head at the hands' instant (LOCAL space).
    pub hand_head: Pose,
    pub psvr2: bool,
    pub simulator: bool,
    /// The C++ overlay's cvr::vrui::Tracking for this cycle and its hands.
    pub ui_tracking: *mut c_void,
    pub ui_hands: *mut UiHand,
}

#[cfg(not(test))]
extern "C" {
    static CyberpunkVR_HandRelToFilteredHead: i32;
    fn GetInputActionsEnabled() -> i32;
    fn Get3DofMovement() -> i32;
    fn cvr_cpp_verbose_log() -> i32;
    /// The tactile envelopes played on this thread (rust/crates/cvr-rig, in the same library).
    fn cvr_haptics_tick();
}
#[cfg(not(test))]
extern "system" {
    fn xrSyncActions(session: openxr_sys::Session, info: *const openxr_sys::ActionsSyncInfo) -> openxr_sys::Result;
}

/// One frame in 120 logs the hands when the log is verbose.
static HAND_LOG: AtomicU32 = AtomicU32::new(0);

/// The hands and the controllers of one XR cycle with a located head: xrSyncActions and the vibrations (on success),
/// then under the hand lock the hand sample's stamp and head, the Vive trackers, the hands (head-relative to the head
/// at their instant, or to the filtered head under xr_hand_rel_filtered) and the controllers. Nothing without actions.
///
/// # Safety
/// `input` is valid; its tracking pointers are the C++ overlay's for this cycle.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_cycle_hands(input: *const CycleHandsIn) {
    use openxr_sys as xr;
    use xr::Handle;
    // SAFETY: the caller's input.
    let Some(inp) = (unsafe { input.as_ref() }) else { return };
    let a = crate::actions::actions();
    if a.action_set == 0 {
        return;
    }
    let session = crate::frame::SESSION.load(Ordering::Acquire);
    let local = crate::frame::LOCAL_SPACE.load(Ordering::Acquire);
    let view = crate::frame::VIEW_SPACE.load(Ordering::Acquire);
    // SAFETY (whole body): the session's live handles; plain C getters; the hand lock as the C++ half takes it.
    unsafe {
        let verbose = cvr_cpp_verbose_log() != 0;
        let log = verbose && HAND_LOG.fetch_add(1, Ordering::Relaxed) % 120 == 0;
        let set = xr::ActionSet::from_raw(a.action_set);
        let active = xr::ActiveActionSet { action_set: set, subaction_path: xr::Path::NULL };
        let sync = xr::ActionsSyncInfo { ty: xr::StructureType::ACTIONS_SYNC_INFO, next: core::ptr::null(), count_active_action_sets: 1, active_action_sets: &active };
        let sync_res = xrSyncActions(xr::Session::from_raw(session), &sync);
        if sync_res.into_raw() >= 0 {
            cvr_haptics_tick();
        }
        if log {
            cvr_host::log!("OpenXRManager[Hands]: syncRes={} sessionState={}\n", sync_res.into_raw(), crate::events::session_state().into_raw());
        }
        let gameplay_active = GetInputActionsEnabled() != 0 && a.thumbstick != 0;
        let mut ctrl: ControllerState = core::mem::zeroed();
        let mut pose_valid = [false; 2];
        let mut scanner_hand = [0.0f32; 3];
        let hands = &crate::tracking::HANDS;
        let guard = hands.lock.lock();
        {
            // The hand sample's stamp and head (the coherent view anchor's reference).
            let ms = crate::tracking::now_ms();
            crate::published::PUBLISHED.sample_ms.store((ms % 100000.0) as f32);
            let d = crate::published::PUBLISHED.data.get(&guard);
            let dof3 = Get3DofMovement() != 0;
            let p = &crate::tracking::HEAD.pos;
            let h = [p[0].load(), p[1].load(), p[2].load()];
            d.sample_head_pos = if dof3 { [0.0; 3] } else { h };
            d.sample_head_valid = true;
            d.sample_origin = crate::tracking::origin_serial();
        }
        // The head the hands are made relative to.
        let mut head = inp.hand_head;
        if core::ptr::read_volatile(core::ptr::addr_of!(CyberpunkVR_HandRelToFilteredHead)) != 0 {
            let fp = rotate_vector(inp.base.orientation, inp.filtered.position);
            head.position = Vec3 { x: inp.base.position.x + fp.x, y: inp.base.position.y + fp.y, z: inp.base.position.z + fp.z };
            head.orientation = multiply_quat(inp.base.orientation, inp.filtered.orientation);
        }
        // The Vive trackers (trackers.rs); the head as an XrPosef (orientation, then position).
        crate::trackers::cvr_trackers_locate(local, view, inp.hand_time, &head as *const Pose as *const openxr_sys::Posef);
        let (grip_spaces, aim_spaces) = *crate::session::HAND_SPACES.lock().unwrap_or_else(|p| p.into_inner());
        let frame = crate::hands::HandsFrame {
            session,
            local_space: local,
            hand_pose_action: a.hand_pose,
            hand_paths: a.hand_paths,
            hand_spaces: grip_spaces,
            hand_aim_spaces: aim_spaces,
            time: inp.hand_time,
            head,
            raw_head: inp.hand_head,
            base_inv: inp.base_inv,
            origin: crate::tracking::origin_serial(),
            gameplay_active,
            simulator: inp.simulator,
            log,
        };
        let out = crate::hands::HandsOut {
            hands: {
                let hp = hands.hands.get(&guard).as_mut_ptr();
                [hp.cast(), hp.add(1).cast()]
            },
            motion: hands.motion.get(&guard).as_mut_ptr(),
            aim_filter: hands.aim_filter.get(&guard).as_mut_ptr(),
            yaw_rel: hands.yaw_rel.as_ptr().cast(),
            yaw_valid: hands.yaw_valid.as_ptr(),
            ctrl: &mut ctrl,
            pose_valid: pose_valid.as_mut_ptr(),
            scanner_hand: &mut scanner_hand,
        };
        crate::hands::cvr_xr_locate_hands(&frame, &out);
        let actions = InputActions {
            session,
            local_space: local,
            hand_paths: a.hand_paths,
            hand_aim_spaces: aim_spaces,
            thumbstick: a.thumbstick,
            trigger: a.trigger,
            grip: a.grip,
            grip_force: a.grip_force,
            thumbstick_click: a.thumbstick_click,
            primary: a.primary_button,
            secondary: a.secondary_button,
            menu: a.menu_button,
            psvr2_triangle_touch: a.psvr2_triangle_touch,
            steam_frame: a.steam_frame,
            thumbrest_touch: a.thumbrest_touch,
        };
        let input_frame = InputFrame {
            time: inp.display_time,
            pose_valid,
            gameplay_active,
            psvr2: inp.psvr2,
            sync_ok: sync_res == xr::Result::SUCCESS,
            scanner_hand,
        };
        crate::input::cvr_xr_sample_controllers(&actions, &input_frame, &mut ctrl, inp.ui_tracking, inp.ui_hands);
        drop(guard);
    }
}

// ---- the cycle's wait and begin

/// What xrWaitFrame gave the cycle, and how long it waited (CyberpunkVR_XrRateLog only).
#[repr(C)]
pub struct CycleFrame {
    pub display_time: i64,
    pub display_period: i64,
    pub should_render: bool,
    pub wait_enter_ms: f64,
    pub wait_ms: f64,
}

#[cfg(not(test))]
extern "C" {
    static CyberpunkVR_XrRateLog: i32;
    static mut CyberpunkVR_DebugXrCycles: u64;
    static mut CyberpunkVR_DebugMonoSubmits: u64;
    static CyberpunkVR_DebugXrWaits: core::sync::atomic::AtomicU64;
    static CyberpunkVR_DebugXrWaitsReturned: core::sync::atomic::AtomicU64;
    static CyberpunkVR_DebugXrWaitFailed: core::sync::atomic::AtomicU64;
    static CyberpunkVR_DebugXrBegins: core::sync::atomic::AtomicU64;
    static CyberpunkVR_DebugXrBeginDiscarded: core::sync::atomic::AtomicU64;
}
#[cfg(not(test))]
extern "system" {
    fn xrWaitFrame(session: openxr_sys::Session, info: *const openxr_sys::FrameWaitInfo, state: *mut openxr_sys::FrameState) -> openxr_sys::Result;
    fn xrBeginFrame(session: openxr_sys::Session, info: *const openxr_sys::FrameBeginInfo) -> openxr_sys::Result;
}

/// The cycle's xrWaitFrame (FrameThreadMain): the display cycle counted against the submissions (with the pacing line
/// every 600 cycles), the wait marked and timed. False when the wait failed (counted); the caller then signals the
/// frame sync and sleeps.
///
/// # Safety
/// `out` is valid; the session is running.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_wait_frame(out: *mut CycleFrame) -> bool {
    use openxr_sys as xr;
    use xr::Handle;
    // SAFETY (whole body): the caller's output; the session's handle; the C++ half's counters.
    unsafe {
        let Some(out) = out.as_mut() else { return false };
        // Each count checks the switch itself, as CVR_DIAGNOSTIC does per statement.
        let diag = crate::present_diag_enabled;
        if diag() {
            let p = core::ptr::addr_of_mut!(CyberpunkVR_DebugXrCycles);
            let cycles = core::ptr::read_volatile(p).wrapping_add(1);
            core::ptr::write_volatile(p, cycles);
            if cycles % 600 == 0 {
                let submits = core::ptr::read_volatile(core::ptr::addr_of!(CyberpunkVR_DebugMonoSubmits));
                let missed = cycles.wrapping_sub(submits);
                cvr_host::log!(
                    "XR pacing: cycles={} submits={} missed={} ({:.1}%)\n",
                    cycles,
                    submits,
                    missed,
                    if cycles != 0 { 100.0 * missed as f64 / cycles as f64 } else { 0.0 }
                );
            }
        }
        crate::xr_rates::cvr_xr_mark(b'W');
        if diag() {
            CyberpunkVR_DebugXrWaits.fetch_add(1, Ordering::Relaxed);
        }
        let timed = core::ptr::read_volatile(core::ptr::addr_of!(CyberpunkVR_XrRateLog)) != 0;
        out.wait_enter_ms = if timed { crate::tracking::now_ms() } else { 0.0 };
        let info = xr::FrameWaitInfo { ty: xr::StructureType::FRAME_WAIT_INFO, next: core::ptr::null() };
        let mut state: xr::FrameState = core::mem::zeroed();
        state.ty = xr::StructureType::FRAME_STATE;
        let res = xrWaitFrame(xr::Session::from_raw(crate::frame::SESSION.load(Ordering::Acquire)), &info, &mut state);
        out.wait_ms = if timed { crate::tracking::now_ms() - out.wait_enter_ms } else { 0.0 };
        if diag() {
            CyberpunkVR_DebugXrWaitsReturned.fetch_add(1, Ordering::Relaxed);
        }
        if res.into_raw() < 0 {
            if diag() {
                CyberpunkVR_DebugXrWaitFailed.fetch_add(1, Ordering::Relaxed);
            }
            return false;
        }
        out.display_time = state.predicted_display_time.as_nanos();
        out.display_period = state.predicted_display_period.as_nanos();
        out.should_render = state.should_render.into();
        true
    }
}

/// The cycle's xrBeginFrame, counted (XR_FRAME_DISCARDED, a success code, on its own count: nothing branches on it).
#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn cvr_xr_begin_frame() {
    use openxr_sys as xr;
    use xr::Handle;
    let diag = crate::present_diag_enabled;
    // SAFETY: the session's handle; the C++ half's counters.
    unsafe {
        if diag() {
            CyberpunkVR_DebugXrBegins.fetch_add(1, Ordering::Relaxed);
        }
        let info = xr::FrameBeginInfo { ty: xr::StructureType::FRAME_BEGIN_INFO, next: core::ptr::null() };
        let res = xrBeginFrame(xr::Session::from_raw(crate::frame::SESSION.load(Ordering::Acquire)), &info);
        if res == xr::Result::FRAME_DISCARDED && diag() {
            CyberpunkVR_DebugXrBeginDiscarded.fetch_add(1, Ordering::Relaxed);
        }
    }
}

// ---- the cycle's end with a picture, and what follows it

#[cfg(not(test))]
extern "C" {
    static CyberpunkVR_XrDeepDiag: i32;
    static CyberpunkVR_DebugXrEnds: core::sync::atomic::AtomicU64;
    static CyberpunkVR_DebugXrEndsWithLayer: core::sync::atomic::AtomicU64;
    static CyberpunkVR_DebugSubmitAgeCount: core::sync::atomic::AtomicU64;
    static CyberpunkVR_DebugSubmitAgeSumUs: core::sync::atomic::AtomicU64;
    static CyberpunkVR_DebugSubmitAgeMinUs: core::sync::atomic::AtomicU64;
    static CyberpunkVR_DebugSubmitAgeMaxUs: core::sync::atomic::AtomicU64;
    static CyberpunkVR_DebugSubmitAgeBuckets: [core::sync::atomic::AtomicU64; 4];
    static CyberpunkVR_HudSubmittedFrames: core::sync::atomic::AtomicU64;
    static CyberpunkVR_PoseIdLastSubmit: [core::sync::atomic::AtomicU64; 2];
    static mut CyberpunkVR_DebugFreshSubmits: u64;
    static CyberpunkVR_DebugPoseFromWrite: u64;
    static CyberpunkVR_DebugPoseFromSlot: u64;
    static CyberpunkVR_DebugFinalExact: u64;
    static CyberpunkVR_DebugFinalApprox: u64;
    static CyberpunkVR_DebugFinalExactTies: u64;
    static CyberpunkVR_DebugFinalNoMatch: u64;
    static CyberpunkVR_DebugFinalAge: u32;
    static CyberpunkVR_DebugPoseReadBack: u64;
    static CyberpunkVR_PlacedPoseMatch: core::sync::atomic::AtomicU64;
    static CyberpunkVR_PlacedPoseMiss: core::sync::atomic::AtomicU64;
    static CyberpunkVR_PlacedPoseAmbiguous: core::sync::atomic::AtomicU64;
}
#[cfg(not(test))]
extern "system" {
    #[link_name = "xrEndFrame"]
    fn xr_end_frame(session: openxr_sys::Session, info: *const openxr_sys::FrameEndInfo) -> openxr_sys::Result;
}

/// The cycle's xrEndFrame with its layers (FrameThreadMain): counted, and behind CyberpunkVR_XrDeepDiag the submitted
/// image's age (from its capture, `capture_ms` on tracking::now_ms's clock) into the count, sum, min, max and the
/// display-period buckets. Returns the XrResult.
///
/// # Safety
/// `end` is a complete XrFrameEndInfo; the session is live.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_end_frame(end: *const openxr_sys::FrameEndInfo, capture_ms: f64, period_ns: i64) -> i32 {
    use openxr_sys as xr;
    use xr::Handle;
    let diag = crate::present_diag_enabled;
    // SAFETY (whole body): the C++ half's counters; the caller's end info; the session's handle.
    unsafe {
        if diag() {
            CyberpunkVR_DebugXrEnds.fetch_add(1, Ordering::Relaxed);
        }
        if diag() {
            CyberpunkVR_DebugXrEndsWithLayer.fetch_add(1, Ordering::Relaxed);
        }
        if core::ptr::read_volatile(core::ptr::addr_of!(CyberpunkVR_XrDeepDiag)) != 0 && capture_ms > 0.0 {
            let age_ms = crate::tracking::now_ms() - capture_ms;
            if (0.0..1000.0).contains(&age_ms) {
                let age_us = (age_ms * 1000.0) as u64;
                if diag() {
                    CyberpunkVR_DebugSubmitAgeCount.fetch_add(1, Ordering::Relaxed);
                }
                if diag() {
                    CyberpunkVR_DebugSubmitAgeSumUs.fetch_add(age_us, Ordering::Relaxed);
                }
                CyberpunkVR_DebugSubmitAgeMaxUs.fetch_max(age_us, Ordering::Relaxed);
                let _ = CyberpunkVR_DebugSubmitAgeMinUs.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |prev| {
                    (prev == 0 || age_us < prev).then_some(age_us)
                });
                let period_ms = if period_ns > 0 { period_ns as f64 / 1.0e6 } else { 13.89 };
                let bucket = ((age_ms / period_ms) as i32).clamp(0, 3) as usize;
                if diag() {
                    CyberpunkVR_DebugSubmitAgeBuckets[bucket].fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        crate::xr_trace::stage(crate::xr_trace::STAGE_END);
        xr_end_frame(xr::Session::from_raw(crate::frame::SESSION.load(Ordering::Acquire)), end).into_raw()
    }
}

/// What a successful submit leaves for the accounting.
#[repr(C)]
pub struct Submitted {
    /// The HUD's main layer went out outside a menu.
    pub hud: bool,
    pub real: bool,
    pub should_render: bool,
    pub depth: bool,
    pub view_count: u32,
    pub pose_ids: [u64; 2],
    pub period_ns: i64,
    pub display_time: i64,
    pub present_serial: u64,
    /// The serial the last real submission carried (before this one).
    pub last_submitted: u64,
    /// The submitted left view's orientation and the head located this cycle.
    pub render_ori: Quat,
    pub live_ori: Quat,
}

/// The render->live gap's window (POSEDIAG): min, max, sum over up to 120 submits.
struct GapWindow {
    min: f32,
    max: f32,
    sum: f32,
    n: u32,
}
static GAP: std::sync::Mutex<GapWindow> = std::sync::Mutex::new(GapWindow { min: 1e9, max: -1e9, sum: 0.0, n: 0 });

/// LOG_THROTTLED's rule for one call site: nothing without the diagnostics or a verbose log; every call when verbose;
/// else once per 30 seconds.
#[cfg(not(test))]
fn throttle_due(last: &AtomicU64) -> bool {
    // SAFETY: a plain C getter.
    let verbose = unsafe { cvr_cpp_verbose_log() } != 0;
    if !crate::present_diag_enabled() && !verbose {
        return false;
    }
    if verbose {
        return true;
    }
    // SAFETY: a plain clock read.
    let now = unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() };
    let prev = last.load(Ordering::Relaxed);
    if prev != 0 && now - prev < 30000 {
        return false;
    }
    last.store(now, Ordering::Relaxed);
    true
}

/// After a successful xrEndFrame with a picture (FrameThreadMain): the HUD and pose-id counters, the event ring and the
/// cadence, POSEDIAG (the render->live gap over 120 submits, the read-back and placed-camera counts, each line at most
/// once per 30 s), the "Mono frame submitted" line every 300th serial, the submit count, and for a real frame with a
/// new serial the display-time fit's sample.
///
/// # Safety
/// `s` is valid.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_after_submit(s: *const Submitted) {
    // SAFETY (whole body): the caller's record; the C++ half's counters, as the C++ did.
    unsafe {
        let Some(s) = s.as_ref() else { return };
        let diag = crate::present_diag_enabled;
        if s.hud && diag() {
            CyberpunkVR_HudSubmittedFrames.fetch_add(1, Ordering::Relaxed);
        }
        for eye in 0..2 {
            if diag() {
                CyberpunkVR_PoseIdLastSubmit[eye].store(s.pose_ids[eye], Ordering::Relaxed);
            }
        }
        crate::xr_rates::cvr_xr_mark(b'E');
        crate::xr_rates::cvr_xr_bucket_cadence(s.period_ns);
        if diag() {
            let (a, b) = (s.render_ori, s.live_ori);
            let dot = (a.x * b.x + a.y * b.y + a.z * b.z + a.w * b.w).abs().min(1.0);
            let gap_deg = 2.0 * cvr_body::cmath::acos(dot) * 180.0 / 3.1415926535f32;
            let mut g = GAP.lock().unwrap_or_else(|p| p.into_inner());
            if gap_deg < g.min {
                g.min = gap_deg;
            }
            if gap_deg > g.max {
                g.max = gap_deg;
            }
            g.sum += gap_deg;
            let disp_ms = s.period_ns as f64 / 1.0e6;
            let game_ms = crate::frame_identity::cvr_xr_fit_slope_ns() / 1.0e6;
            g.n += 1;
            if g.n >= 120 {
                static T1: AtomicU64 = AtomicU64::new(0);
                static T2: AtomicU64 = AtomicU64::new(0);
                static T3: AtomicU64 = AtomicU64::new(0);
                if throttle_due(&T1) {
                    cvr_host::log!(
                        "POSEDIAG: render->live gap over {} frames: avg {:.2} deg, min {:.2}, max {:.2}, spread {:.2} | display {:.2} ms ({:.1} Hz), game {:.2} ms ({:.1} fps), ratio {:.2} | poseFromWrite={} fromSlot={}\n",
                        g.n,
                        (g.sum / g.n as f32) as f64,
                        g.min as f64,
                        g.max as f64,
                        (g.max - g.min) as f64,
                        disp_ms,
                        if disp_ms > 0.0 { 1000.0 / disp_ms } else { 0.0 },
                        game_ms,
                        if game_ms > 0.0 { 1000.0 / game_ms } else { 0.0 },
                        if disp_ms > 0.0 { game_ms / disp_ms } else { 0.0 },
                        core::ptr::read_volatile(core::ptr::addr_of!(CyberpunkVR_DebugPoseFromWrite)),
                        core::ptr::read_volatile(core::ptr::addr_of!(CyberpunkVR_DebugPoseFromSlot))
                    );
                }
                if throttle_due(&T2) {
                    cvr_host::log!(
                        "POSEDIAG: readback exact={} approx={} exactTies={} nomatch={} age={} qdepth={} used={}\n",
                        core::ptr::read_volatile(core::ptr::addr_of!(CyberpunkVR_DebugFinalExact)),
                        core::ptr::read_volatile(core::ptr::addr_of!(CyberpunkVR_DebugFinalApprox)),
                        core::ptr::read_volatile(core::ptr::addr_of!(CyberpunkVR_DebugFinalExactTies)),
                        core::ptr::read_volatile(core::ptr::addr_of!(CyberpunkVR_DebugFinalNoMatch)),
                        core::ptr::read_volatile(core::ptr::addr_of!(CyberpunkVR_DebugFinalAge)),
                        crate::frame_identity::cvr_xr_rendered_frame_queue_depth(false),
                        core::ptr::read_volatile(core::ptr::addr_of!(CyberpunkVR_DebugPoseReadBack))
                    );
                }
                if throttle_due(&T3) {
                    cvr_host::log!(
                        "POSEDIAG: placed-camera match={} miss={} ambiguous={}\n",
                        CyberpunkVR_PlacedPoseMatch.load(Ordering::Relaxed),
                        CyberpunkVR_PlacedPoseMiss.load(Ordering::Relaxed),
                        CyberpunkVR_PlacedPoseAmbiguous.load(Ordering::Relaxed)
                    );
                }
                *g = GapWindow { min: 1e9, max: -1e9, sum: 0.0, n: 0 };
            }
        }
        if diag() && s.present_serial % 300 == 1 {
            cvr_host::log!(
                "OpenXRManager: Mono frame submitted. serial={} fresh={} views={} shouldRender={} depth={}\n",
                s.present_serial,
                (s.present_serial != s.last_submitted) as i32,
                s.view_count,
                s.should_render as i32,
                s.depth as i32
            );
        }
        if diag() {
            let p = core::ptr::addr_of_mut!(CyberpunkVR_DebugMonoSubmits);
            core::ptr::write_volatile(p, core::ptr::read_volatile(p).wrapping_add(1));
        }
        if s.real && s.present_serial != s.last_submitted {
            crate::frame_identity::cvr_xr_fit_add_display_time(s.present_serial, s.display_time);
            if diag() {
                let p = core::ptr::addr_of_mut!(CyberpunkVR_DebugFreshSubmits);
                core::ptr::write_volatile(p, core::ptr::read_volatile(p).wrapping_add(1));
            }
        }
    }
}
