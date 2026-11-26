//! The physical body's heading from the HMD and the controllers (formerly src/Hooks/BodyYawFollow.cpp): the solver
//! runs at the native motion provider (body_heading.rs feeds each request to the movement adapter and keeps only the
//! committed heading), in one of three modes -- the head cone, the tracked hands, or the hybrid of both. The view's
//! tracking has a separate reference and never cancels this output. Also the body's own frame, published from the
//! body-yaw store site for the pose path, and the native camera/entity pair the solve converts with.
//!
//! The exports stay in src/Hooks/BodyYawFollow.cpp; its C++ API, and the C names the solve and the hooks call (VRIK_*,
//! BodyYawFollow*), forward here from src/Hooks/RoomscaleRust.cpp -- tools/vrik_tests stands in for those names.
use super::body_heading::{cvr_body_heading_offset, cvr_body_heading_ready, cvr_body_tracking_yaw};
use super::camera_link::HeadPose;
use super::env::{steady_us, VrikTransformSnapshot};
use super::gestures::now_ms;
use super::roomscale_move::{controls, cvr_roomscale_physical_body_allowed, cvr_roomscale_physical_body_heading_owned, cvr_roomscale_player_identity};
use cvr_body::cmath::{atan2, cos, remainder, sin};
use cvr_body::head_led::{turn_clamp_axis, turn_delta_angle, HeadLedBodyTurn, DEFAULT_BODY_TURN_CONFIG, LOOK_DOWN_ANGLE_DEG};
use cvr_body::hybrid_yaw::{needs_body_tracking, HybridBodyYaw, HybridInput, RotationMode};
use cvr_body::look_down::{body_free_look_cone, head_down_degrees};
use cvr_body::roomscale::head_yaw;
use cvr_body::tracked_yaw::{TrackedBodyYaw, TrackingFrame, YawEstimate};
use cvr_body::yaw_follower::BodyYawFollower;
use cvr_camera::frames::{camera_packet_status, make_camera_entity_sample, CameraPacketState};
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{LazyLock, Mutex};

extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static mut CyberpunkVR_BodyYawFollow: i32;
    static mut CyberpunkVR_DebugVrikNativePairPublished: u64;
    static mut CyberpunkVR_DebugVrikNativePairRejected: u64;
    static mut CyberpunkVR_BodyYawFollowDeadDeg: f32;
    static mut CyberpunkVR_DebugHybridTrackedWeight: f32;
    static mut CyberpunkVR_DebugHybridHandsDeg: f32;
    static mut CyberpunkVR_DebugHybridNeckPullDeg: f32;
    static mut CyberpunkVR_DebugHybridAlignDeg: f32;
    static mut CyberpunkVR_DebugHybridEstimate: i32;
    static mut CyberpunkVR_DebugTrackedTrace: [f32; 8];
    static mut CyberpunkVR_DebugTrackedGates: u32;
    static mut CyberpunkVR_DebugHeadLedTurn: [f32; 4];
    static CyberpunkVR_BodyTurnSpeedDps: i32;
    static CyberpunkVR_BodyUprightConeDeg: i32;
    static mut CyberpunkVR_DebugBodyFollowErrDeg: f32;
    static mut CyberpunkVR_DebugBodyFollowCalls: u64;
    static mut CyberpunkVR_DebugBodyFollowApplied: u64;
    static mut CyberpunkVR_BodyYawFinalRad: f32;
    static mut CyberpunkVR_BodyYawFinalValid: i32;
    static mut CyberpunkVR_PlayerEntityPos: [f32; 3];
    static mut CyberpunkVR_PlayerEntityQuat: [f32; 4];
    static mut CyberpunkVR_PlayerEntityValid: i32;
    static g_VrikFrameEpoch: AtomicU32;
    fn cvr_cpp_xr_tracking_origin_serial() -> u64;
    fn cvr_xr_body_tracking_frame(out: *mut TrackingFrame) -> bool;
    fn cvr_cpp_swimming_active() -> bool;
    fn cvr_cpp_fbt_calibrating() -> bool;
}

fn diagnostics() -> bool {
    // SAFETY: a plain exported int.
    unsafe { CyberpunkVR_RuntimeDiagnostics != 0 }
}

struct Follow {
    follower: BodyYawFollower,
    tracked: TrackedBodyYaw,
    mode: RotationMode,
    hybrid: HybridBodyYaw,
    head_led: HeadLedBodyTurn,
}
static FOLLOW: LazyLock<Mutex<Follow>> = LazyLock::new(|| {
    Mutex::new(Follow {
        follower: BodyYawFollower::default(),
        tracked: TrackedBodyYaw::default(),
        mode: RotationMode::Off,
        hybrid: HybridBodyYaw::default(),
        head_led: HeadLedBodyTurn::default(),
    })
});
fn follow() -> std::sync::MutexGuard<'static, Follow> {
    FOLLOW.lock().unwrap_or_else(|e| e.into_inner())
}
fn mode_of(value: i32) -> RotationMode {
    match value {
        1 => RotationMode::HeadCone,
        2 => RotationMode::Tracked,
        3 => RotationMode::Hybrid,
        4 => RotationMode::HeadLed,
        _ => RotationMode::Off,
    }
}

// ---- the body's current frame, from the store site

struct BodyFrame {
    position: [f32; 3],
    rotation: [f32; 4],
    tracking_yaw: f32,
    owner: usize,
    stamp: u64,
}
static BODY_FRAME: Mutex<BodyFrame> = Mutex::new(BodyFrame { position: [0.0; 3], rotation: [0.0, 0.0, 0.0, 1.0], tracking_yaw: 0.0, owner: 0, stamp: 0 });

#[no_mangle]
pub unsafe extern "C" fn cvr_body_frame_read(p: *mut f32, q: *mut f32, tracking_yaw: *mut f32) -> bool {
    let f = BODY_FRAME.lock().unwrap_or_else(|e| e.into_inner());
    let now = now_ms();
    if f.owner == 0 || f.owner != cvr_roomscale_player_identity() || now < f.stamp || now - f.stamp > 250 {
        return false;
    }
    core::ptr::copy_nonoverlapping(f.position.as_ptr(), p, 3);
    core::ptr::copy_nonoverlapping(f.rotation.as_ptr(), q, 4);
    *tracking_yaw = f.tracking_yaw;
    true
}

// ---- the native camera/entity pair, behind a sequence lock

struct NativePair {
    seq: AtomicU32,
    cam_quat: [AtomicU32; 4],
    entity_quat: [AtomicU32; 4],
    camera_minus_entity: [AtomicU32; 3],
    valid: AtomicU32,
    /// Diagnostic only.
    consumer_epoch: AtomicU32,
    body_camera_minus_entity: [AtomicU32; 3],
    unavailable: AtomicU32,
    owner: AtomicUsize,
    origin: AtomicU64,
    stamp_ms: AtomicU64,
}
const Z: AtomicU32 = AtomicU32::new(0);
static PAIR: NativePair = NativePair {
    seq: AtomicU32::new(0),
    cam_quat: [Z; 4],
    entity_quat: [Z; 4],
    camera_minus_entity: [Z; 3],
    valid: AtomicU32::new(0),
    consumer_epoch: AtomicU32::new(0),
    body_camera_minus_entity: [Z; 3],
    unavailable: AtomicU32::new(0),
    owner: AtomicUsize::new(0),
    origin: AtomicU64::new(0),
    stamp_ms: AtomicU64::new(0),
};
static PAIR_WRITER: Mutex<()> = Mutex::new(());
fn st(a: &AtomicU32, v: f32) {
    a.store(v.to_bits(), Ordering::Relaxed);
}
fn ld(a: &AtomicU32) -> f32 {
    f32::from_bits(a.load(Ordering::Relaxed))
}

#[no_mangle]
pub unsafe extern "C" fn cvr_body_native_pair_publish(centre: *const i32, body_base: *const i32, entity: *const i32, camera_quat: *const f32,
    entity_quat: *const f32, owner: usize, origin: u64, owner_stable: bool) {
    let c = [*centre, *centre.add(1), *centre.add(2)];
    let b = [*body_base, *body_base.add(1), *body_base.add(2)];
    let e = [*entity, *entity.add(1), *entity.add(2)];
    let cq = [*camera_quat, *camera_quat.add(1), *camera_quat.add(2), *camera_quat.add(3)];
    let eq = [*entity_quat, *entity_quat.add(1), *entity_quat.add(2), *entity_quat.add(3)];
    let pair = make_camera_entity_sample(&c, &b, &e, &cq, &eq);
    let _w = PAIR_WRITER.lock().unwrap_or_else(|p| p.into_inner());
    PAIR.seq.fetch_add(1, Ordering::AcqRel);
    for k in 0..4 {
        st(&PAIR.cam_quat[k], pair.camera_quat[k]);
        st(&PAIR.entity_quat[k], pair.entity_quat[k]);
    }
    for k in 0..3 {
        st(&PAIR.camera_minus_entity[k], pair.camera_minus_entity[k]);
        st(&PAIR.body_camera_minus_entity[k], pair.body_minus_entity[k]);
    }
    PAIR.valid.store((owner_stable && pair.valid) as u32, Ordering::Relaxed);
    PAIR.unavailable.store((!owner_stable) as u32, Ordering::Relaxed);
    PAIR.consumer_epoch.store(g_VrikFrameEpoch.load(Ordering::Relaxed), Ordering::Relaxed);
    PAIR.owner.store(owner, Ordering::Relaxed);
    PAIR.origin.store(origin, Ordering::Relaxed);
    PAIR.stamp_ms.store(now_ms(), Ordering::Relaxed);
    PAIR.seq.fetch_add(1, Ordering::Release);
    if diagnostics() {
        if owner_stable && pair.valid {
            CyberpunkVR_DebugVrikNativePairPublished += 1;
        } else {
            CyberpunkVR_DebugVrikNativePairRejected += 1;
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn cvr_body_native_pair_read(out: *mut VrikTransformSnapshot) -> bool {
    if out.is_null() {
        return false;
    }
    for _ in 0..4 {
        let before = PAIR.seq.load(Ordering::Acquire);
        if before == 0 {
            return false;
        }
        if before & 1 != 0 {
            continue;
        }
        let mut v: VrikTransformSnapshot = core::mem::zeroed();
        for k in 0..4 {
            v.camQuat[k] = ld(&PAIR.cam_quat[k]);
            v.entityQuat[k] = ld(&PAIR.entity_quat[k]);
        }
        for k in 0..3 {
            v.cameraMinusEntity[k] = ld(&PAIR.camera_minus_entity[k]);
            v.bodyCameraMinusEntity[k] = ld(&PAIR.body_camera_minus_entity[k]);
        }
        v.valid = PAIR.valid.load(Ordering::Relaxed);
        v.unavailable = PAIR.unavailable.load(Ordering::Relaxed);
        let owner = PAIR.owner.load(Ordering::Relaxed);
        let origin = PAIR.origin.load(Ordering::Relaxed);
        let stamp = PAIR.stamp_ms.load(Ordering::Relaxed);
        if before != PAIR.seq.load(Ordering::Acquire) {
            continue;
        }
        let state = camera_packet_status(owner, origin, stamp, cvr_roomscale_player_identity(), cvr_cpp_xr_tracking_origin_serial(), now_ms(),
            v.valid != 0, v.unavailable != 0);
        v.valid = (state == CameraPacketState::Valid) as u32;
        v.unavailable = (state == CameraPacketState::Unavailable) as u32;
        *out = v;
        return true;
    }
    false
}

// ---- the follower

/// The HMD's yaw relative to the recenter base, about the XR vertical: the same input latch the CCT move reads,
/// not the completed previous move camera and IK consumers still own.
struct Head {
    yaw: f32,
    down_degrees: f32,
    bend: f32,
    origin: u64,
    yaw_defined: bool,
}
unsafe fn head_yaw_rel_base(h: &mut Head) -> bool {
    let mut hp = HeadPose::default();
    if !super::xr_calls::acquire_frame_head(&mut hp) || !hp.valid {
        return false;
    }
    let [x, y, z, w] = hp.ori;
    h.origin = hp.origin_serial;
    h.down_degrees = head_down_degrees(x, y, z, w);
    h.bend = hp.body_bend[2];
    match head_yaw(x, y, z, w) {
        Some(v) => {
            h.yaw = v;
            h.yaw_defined = true;
        }
        None => h.yaw_defined = false,
    }
    let norm = x * x + y * y + z * z + w * w;
    norm.is_finite() && norm > 0.5 && norm < 1.5
}

unsafe fn publish_follower_state(f: &Follow) {
    CyberpunkVR_BodyYawFollow = f.follower.enabled() as i32;
}
unsafe fn refresh_follower_state(f: &mut Follow, observed_offset: f32) {
    let mode = mode_of(controls().body_rotation_mode);
    f.follower.set_enabled(cvr_body_heading_ready() && mode != RotationMode::Off && cvr_roomscale_physical_body_heading_owned());
    if mode != f.mode || !f.follower.enabled() {
        f.tracked.reset(observed_offset, mode != RotationMode::Hybrid);
        f.hybrid.reset();
        f.head_led.reset();
        f.mode = mode;
    }
    publish_follower_state(f);
}

/// Animation can ask before the next native move; only that move owns the committed offset that seeds a mode
/// transition, so this query does not reset the estimator.
#[no_mangle]
pub unsafe extern "C" fn cvr_body_yaw_follow_active() -> bool {
    cvr_body_heading_ready() && mode_of(controls().body_rotation_mode) != RotationMode::Off && cvr_roomscale_physical_body_heading_owned()
}
#[no_mangle]
pub unsafe extern "C" fn cvr_body_yaw_follow_offset() -> f32 {
    cvr_body_heading_offset()
}
#[no_mangle]
pub unsafe extern "C" fn cvr_body_yaw_follow_release() {
    let mut f = follow();
    f.follower.reset();
    let hybrid = f.mode != RotationMode::Hybrid;
    f.tracked.reset(0.0, hybrid);
    f.hybrid.reset();
    f.head_led.reset();
    publish_follower_state(&f);
}

/// One native move's body turn: the observed offset is feedback from the adapter's persistent quaternion, so a
/// skipped call cannot accumulate phantom yaw.
#[no_mangle]
pub unsafe extern "C" fn cvr_body_yaw_follow_step(observed_offset: f32) -> f32 {
    // XR state before the follower lock: camera consumers may read the offset while publishing that same XR frame.
    let mut head = Head { yaw: 0.0, down_degrees: 0.0, bend: 0.0, origin: 0, yaw_defined: false };
    let have_head = head_yaw_rel_base(&mut head);
    let c = controls();
    let mut body_frame = TrackingFrame::default();
    let have_body = needs_body_tracking(mode_of(c.body_rotation_mode)) && cvr_xr_body_tracking_frame(&mut body_frame);
    let mut f = follow();
    if diagnostics() {
        CyberpunkVR_DebugBodyFollowCalls += 1;
    }
    refresh_follower_state(&mut f, observed_offset);
    // Held while calibrating full-body tracking: the player turns the head to look at V beside them.
    if !f.follower.enabled() || !cvr_roomscale_physical_body_allowed() || !have_head || cvr_cpp_fbt_calibrating() {
        f.tracked.suspend();
        f.hybrid.suspend(false);
        return 0.0;
    }
    f.follower.observe(observed_offset, head.origin);
    let now = steady_us() as u64;
    if diagnostics() {
        CyberpunkVR_DebugBodyFollowErrDeg =
            if head.yaw_defined { remainder(head.yaw - f.follower.offset(), 6.283_185_307_18) * 57.295_779_5 } else { 0.0 };
    }
    let mut result = YawEstimate::default();
    if needs_body_tracking(f.mode) {
        if have_body && body_frame.origin == head.origin {
            f.tracked.trace.bits = 0;
            result = f.tracked.update(&body_frame);
            let t = f.tracked.trace;
            CyberpunkVR_DebugTrackedTrace = [t.delta, t.wrist_mean, t.head_delta, t.head_lead, t.accepted, t.evidence, t.residual, t.target];
            CyberpunkVR_DebugTrackedGates = t.bits;
            if result.valid {
                result = f.tracked.predict(now);
            }
        } else {
            f.tracked.suspend();
        }
    }
    let applied = |step: f32| {
        if step != 0.0 && diagnostics() {
            CyberpunkVR_DebugBodyFollowApplied += 1;
        }
    };
    if f.mode == RotationMode::HeadLed {
        // The head-led turn (cvr_body::head_led): the head alone decides; no follow zone, no controllers.
        CyberpunkVR_BodyYawFollowDeadDeg = 0.0;
        if !head.yaw_defined {
            return 0.0;
        }
        let body_deg = f.follower.offset() * 57.295_779_5;
        // Started from the body as it stands; started again if the body was turned from outside (a reset, a
        // teleport): a missed apply is not that -- the model's own heading carries on and the next request catches
        // the body up.
        if !f.head_led.seeded() || turn_delta_angle(f.head_led.output(), body_deg).abs() > 20.0 {
            f.head_led.seed(body_deg);
            CyberpunkVR_DebugHeadLedTurn[3] += 1.0;
        }
        let since = f.follower.since_last(now);
        let dt = if 0.25 < since { 0.25 } else if since < 0.0 { 0.0 } else { since };
        let pitch = -head.down_degrees; // up positive
        let mut config = DEFAULT_BODY_TURN_CONFIG;
        let speed = CyberpunkVR_BodyTurnSpeedDps;
        if speed > 0 && speed <= 3600 {
            config.upper_speed_dps = speed as f32;
            config.lower_speed_dps = speed as f32;
        }
        let cone = CyberpunkVR_BodyUprightConeDeg;
        if cone > 0 && cone <= 90 {
            config.upper_tolerance_deg = cone as f32;
        }
        let out = f.head_led.step(head.yaw * 57.295_779_5, pitch, dt, &config, LOOK_DOWN_ANGLE_DEG);
        CyberpunkVR_DebugHeadLedTurn[0] = if pitch < LOOK_DOWN_ANGLE_DEG { 1.0 } else { 0.0 };
        CyberpunkVR_DebugHeadLedTurn[1] = turn_clamp_axis(head.yaw * 57.295_779_5);
        CyberpunkVR_DebugHeadLedTurn[2] = out;
        let step = f.follower.set(out * 0.017_453_292_52, now, head.origin);
        publish_follower_state(&f);
        applied(step);
        return step;
    }
    if f.mode == RotationMode::Hybrid {
        let input = HybridInput {
            head_yaw: head.yaw,
            head_down_degrees: head.down_degrees,
            body_bend_radians: head.bend,
            body_offset: f.follower.offset(),
            cone_degrees: c.body_free_look_deg,
            estimate: result,
            yaw_defined: head.yaw_defined,
            estimate_ready: result.valid && f.tracked.ready(),
            stamp_us: now,
            origin: head.origin,
            lowered_cone_degrees: c.body_free_look_down_deg,
        };
        let command = f.hybrid.step(&input);
        CyberpunkVR_BodyYawFollowDeadDeg = command.cone_degrees;
        CyberpunkVR_DebugHybridTrackedWeight = command.tracked_weight;
        CyberpunkVR_DebugHybridHandsDeg = command.hands_yaw * 57.295_779_5;
        CyberpunkVR_DebugHybridNeckPullDeg = command.neck_pull * 57.295_779_5;
        CyberpunkVR_DebugHybridAlignDeg = command.alignment * 57.295_779_5;
        CyberpunkVR_DebugHybridEstimate = command.estimate as i32;
        let step = f.follower.step(command.target_yaw, 0.0, now, head.origin);
        publish_follower_state(&f);
        applied(step);
        return step;
    }
    if f.mode == RotationMode::Tracked {
        CyberpunkVR_BodyYawFollowDeadDeg = 0.0; // no standing, look-down or swimming cone in this mode
        let target = if result.valid { result.yaw } else { f.follower.offset() };
        let step = f.follower.step(target, 0.0, now, head.origin);
        publish_follower_state(&f);
        applied(step);
        return step;
    }
    if !head.yaw_defined {
        return 0.0;
    }
    CyberpunkVR_BodyYawFollowDeadDeg =
        body_free_look_cone(cvr_cpp_swimming_active(), c.body_free_look_swim_deg, head.down_degrees, c.body_free_look_deg, c.body_free_look_down_deg, head.bend);
    let step = f.follower.step(head.yaw, CyberpunkVR_BodyYawFollowDeadDeg * 0.017_453_292_52, now, head.origin);
    publish_follower_state(&f);
    applied(step);
    step
}

/// From the body-yaw store site (body_yaw_census.rs), once per frame for the player: the body's own transform for
/// the pose path, the position from the engine in the same instant as the yaw. Writes nothing into the game.
#[no_mangle]
pub unsafe extern "C" fn cvr_body_yaw_follow_tick(engine_z: f32, engine_w: f32, engine_pos: *const f32) {
    let (mut wz, mut ww) = (engine_z, engine_w);
    if ww < 0.0 {
        wz = -wz;
        ww = -ww;
    }
    if wz == 0.0 && ww == 0.0 {
        return;
    }
    if engine_pos.is_null() {
        return;
    }
    let pos = [*engine_pos, *engine_pos.add(1), *engine_pos.add(2)];
    let yaw = 2.0 * atan2(wz, ww);
    let h = yaw * 0.5;
    let quat = [0.0f32, 0.0, sin(h), cos(h)];
    {
        let mut f = BODY_FRAME.lock().unwrap_or_else(|e| e.into_inner());
        f.position = pos;
        f.rotation = quat;
        f.tracking_yaw = cvr_body_tracking_yaw(yaw);
        f.owner = cvr_roomscale_player_identity();
        f.stamp = now_ms();
    }
    CyberpunkVR_BodyYawFinalRad = yaw;
    CyberpunkVR_BodyYawFinalValid = 1;
    CyberpunkVR_PlayerEntityPos = pos;
    CyberpunkVR_PlayerEntityQuat = [0.0, 0.0, quat[2], quat[3]];
    CyberpunkVR_PlayerEntityValid = 1;
}
