//! PhysHands (main's src/Natives/PhysHands.cpp, include/Natives/PhysHands.hpp) -- each hand a rigid body of its own in
//! the game's PhysX world, steered toward the controller with a hand's force, and the drawn hand where that body is. A
//! wall stops it because PhysX stops it; a cup it meets is pushed by it; a crate does not move for it. Nothing here asks
//! a ray.
//!
//! THREADS. Nothing heavy runs on a game thread:
//!   * the animation thread (VRIK_ApplyHandStop) publishes tracking and reads back an offset: a few copies; the finger
//!     layer publishes the fingers as drawn and turns their joints by what comes back;
//!   * the physics thread, in the PhysX step's hooks (physx_step), writes two velocities before each step and reads two
//!     poses after it (the bodies are made there once, the filter census runs there a slice a frame, and so does the
//!     list of the world's shapes round the player that the fingers meet);
//!   * a worker thread of its own turns each step's result into the drawn hand's offset, solves the fingers against
//!     that list (PxGeometryQuery, no scene access), decides when the hands are on (vehicles, menus, ladders, water),
//!     reloads config/physhands.ini when it changes, and keeps the statistics.
//! Physics and render outputs use sequence-counted snapshots (seq.rs). The primary avatar's complete input pose is
//! copied together under a short mutex, once per step.
//!
//! The pure pieces (frames, the held grip's drive and contact, the climb's support, the skin) are cvr-physhands'.
#![allow(dead_code, clippy::too_many_arguments, clippy::needless_range_loop)]
pub mod adjust;
pub mod api;
pub mod blade;
pub mod body;
pub mod cabin;
pub mod census;
pub mod fingers;
pub mod grab;
pub mod heldbody;
pub mod hold;
pub mod loose;
pub mod near;
pub mod people;
pub mod pose;
pub mod px;
pub mod seq;
pub mod settings;
pub mod shapes;
pub mod stats;
pub mod step;
pub mod touch;
pub mod trace;
pub mod types;
pub mod worker;

/// On: the body of this hand is in the world and steered (0 left, 1 right).
pub fn on(side: usize) -> bool {
    side <= 1 && body::LIVE[side].load(core::sync::atomic::Ordering::Acquire) && scanner_accepts(body::LIVE_SCANNER_EPOCH.load(core::sync::atomic::Ordering::Acquire))
}
/// The scanner's (and the world's slow motion's) gate on hand physics: work begun in this epoch is still good.
pub fn scanner_accepts(epoch: u64) -> bool {
    cvr_input::scanner::SCANNER_HAND_PHYSICS.accepts(epoch)
}
/// The worker is up (it starts on the first fetch).
pub fn worker_up() -> bool {
    worker::worker_up()
}
/// The world scene's origin, when `sc` is the hands' world scene and its origin reads (the hands live in it only).
pub unsafe fn scene_origin_of(sc: px::P) -> Option<[f32; 3]> {
    if sc.is_null() || sc != body::scene {
        return None;
    }
    px::origin(sc)
}
/// SceneOrigin: physics-thread access to this step's world origin. Held bodies must not keep the origin from their
/// initial pickup after the scene rebases.
pub unsafe fn scene_origin(sc: px::P) -> Option<[f32; 3]> {
    if sc.is_null() || !px::prove() {
        return None;
    }
    px::origin(sc)
}

/// THE SCENE PLAYS THE BODY: the user's cutscene tier (and the vehicle's, mounted), a grapple, a carried body, a
/// finisher, a surveillance camera -- whatever hands VRIK's arms to the animation (the pose hook's early-outs) takes the
/// hands' bodies out of the world too: an animated arm swung through a table would shove what is on it.
pub mod scene {
    use cvr_body::policy::{should_suspend_vrik_for_body_carry, should_suspend_vrik_for_finisher, should_suspend_vrik_for_takedown, should_suspend_vrik_in};
    use cvr_settings::lc;
    extern "C" {
        static g_VRTakedownState: i32;
        static g_VRBodyCarryingState: i32;
        static g_VRBodyDisposalState: i32;
        static g_VRLocomotionState: i32;
        static g_VRFinisherActive: i32;
        static g_VRFinisherSinceMs: u64;
        static CyberpunkVR_VrikTakedownSuspend: i32;
        static CyberpunkVR_VrikBodyCarryHandOff: i32;
        static CyberpunkVR_VrikFinisherSuspend: i32;
        fn cvr_cpp_pose_scene_tier() -> i32;
        fn cvr_cpp_pose_effective_cutscene_tier() -> i32;
        fn cvr_cpp_device_cam_active() -> bool;
    }
    pub unsafe fn scene_owns_arms() -> bool {
        use core::ptr::read_volatile as rv;
        let mounted = super::super::snapshot::SharedPose(31) > 0.5;
        should_suspend_vrik_in(cvr_cpp_pose_scene_tier(), mounted, cvr_cpp_pose_effective_cutscene_tier(), lc!(xrVehicleCutsceneSuspendTier))
            || should_suspend_vrik_for_takedown(rv(&raw const g_VRTakedownState), rv(&raw const CyberpunkVR_VrikTakedownSuspend))
            || should_suspend_vrik_for_body_carry(rv(&raw const g_VRBodyCarryingState), rv(&raw const g_VRBodyDisposalState), rv(&raw const g_VRLocomotionState), rv(&raw const CyberpunkVR_VrikBodyCarryHandOff))
            || should_suspend_vrik_for_finisher(rv(&raw const g_VRFinisherActive), super::tick_ms().wrapping_sub(rv(&raw const g_VRFinisherSinceMs)), rv(&raw const CyberpunkVR_VrikFinisherSuspend))
            || cvr_cpp_device_cam_active()
    }
}

/// VehicleHands' lease (main's src/Natives/VehicleHands.cpp): a short script-owned lease consumes grip input, never the
/// hand's collisions.
pub mod vehicle_hands {
    use core::sync::atomic::{AtomicU64, Ordering::Relaxed};
    static UNTIL: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];
    pub fn busy(side: usize) -> bool {
        side <= 1 && super::tick_ms() < UNTIL[side].load(Relaxed)
    }
    pub fn claim(side: usize, on: bool) {
        if side <= 1 {
            UNTIL[side].store(if on { super::tick_ms() + 250 } else { 0 }, Relaxed);
        }
    }
}

pub use cvr_physhands::hand_skin::{skin_for, HandSkin};

/// GetTickCount64.
pub fn tick_ms() -> u64 {
    // SAFETY: a plain OS call.
    unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }
}
/// QueryPerformanceCounter and its frequency.
pub fn qpc() -> (i64, i64) {
    let (mut now, mut freq) = (0i64, 0i64);
    // SAFETY: plain OS calls into locals.
    unsafe {
        windows_sys::Win32::System::Performance::QueryPerformanceCounter(&mut now);
        windows_sys::Win32::System::Performance::QueryPerformanceFrequency(&mut freq);
    }
    (now, freq.max(1))
}
/// V's body: 1 a woman's, 0 a man's, -1 not known yet (the rig policy, g_VRBodyFemale).
pub fn body_female() -> i32 {
    super::shared_globals::g_VRBodyFemale.load(core::sync::atomic::Ordering::Relaxed)
}

/// Quaternions x y z w (PhysHands.cpp's math).
pub mod math {
    pub fn qmul(a: &[f32; 4], b: &[f32; 4]) -> [f32; 4] {
        [
            a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
            a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
            a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
            a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
        ]
    }
    pub fn qrot(q: &[f32; 4], v: &[f32; 3]) -> [f32; 3] {
        let tx = 2.0 * (q[1] * v[2] - q[2] * v[1]);
        let ty = 2.0 * (q[2] * v[0] - q[0] * v[2]);
        let tz = 2.0 * (q[0] * v[1] - q[1] * v[0]);
        [v[0] + q[3] * tx + q[1] * tz - q[2] * ty, v[1] + q[3] * ty + q[2] * tx - q[0] * tz, v[2] + q[3] * tz + q[0] * ty - q[1] * tx]
    }
    pub fn qconj(q: &[f32; 4]) -> [f32; 4] {
        [-q[0], -q[1], -q[2], q[3]]
    }
    pub fn qnorm(q: &mut [f32; 4]) -> bool {
        let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
        if !n.is_finite() || n < 1e-4 {
            return false;
        }
        for c in q.iter_mut() {
            *c /= n;
        }
        true
    }
    pub fn cross(a: &[f32; 3], b: &[f32; 3]) -> [f32; 3] {
        [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
    }
    pub fn dot(a: &[f32; 3], b: &[f32; 3]) -> f32 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }
    /// Compose: the transform b in the frame a (rotation, position).
    pub fn compose(aq: &[f32; 4], ap: &[f32; 3], bq: &[f32; 4], bp: &[f32; 3]) -> ([f32; 4], [f32; 3]) {
        let r = qrot(aq, bp);
        let op = [ap[0] + r[0], ap[1] + r[1], ap[2] + r[2]];
        let mut oq = qmul(aq, bq);
        qnorm(&mut oq);
        (oq, op)
    }
    pub fn axis_angle(axis: &[f32; 3], a: f32) -> [f32; 4] {
        let s = (0.5 * a).sin();
        [axis[0] * s, axis[1] * s, axis[2] * s, (0.5 * a).cos()]
    }
    pub fn q4(p: &[f32]) -> [f32; 4] {
        [p[0], p[1], p[2], p[3]]
    }
    pub fn v3(p: &[f32]) -> [f32; 3] {
        [p[0], p[1], p[2]]
    }
}
