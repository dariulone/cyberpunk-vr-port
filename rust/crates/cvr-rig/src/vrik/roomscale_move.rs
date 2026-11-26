//! The roomscale move (formerly src/Hooks/RoomscaleMove.cpp): the headset's displacement injected at the native
//! character controller's already-buffered move -- contacts, steps, gravity, backend velocity and entity propagation
//! stay the engine's -- the velocity feedback that keeps the solvers from integrating physical motion twice, and the
//! player identity and the gates the rest of the port asks.
//!
//! SEH is limited to foreign-memory reads (cvr_engine::code::read). Nothing here catches an exception raised by the
//! engine's move itself, and our lock is never held while engine or OpenXR code runs.
//!
//! The C++ half keeps the exports, the capsule tweak (src/Hooks/RoomscaleTweaks.cpp) and the hook's registration; the
//! namespaced API it calls (cvr::roomscale::*) forwards here (src/Hooks/RoomscaleRust.cpp).
use super::body_heading::{cvr_body_heading_reset, cvr_body_tracking_yaw};
use super::body_yaw_follow::cvr_body_yaw_follow_release;
use super::camera_link::{cvr_camera_anchor_recipe_read, HeadPose};
use super::camera_math::cvr_camera_world_scale;
use super::env::{g_VRBind, g_VRBoneCount, g_VRHeadBoneIdx, g_VRLocomotionState, g_isInVehicle, g_pSharedHands, log};
use super::weapon_arm::cvr_anim_is_head_aim_weapon_active;
use core::ffi::c_void;
use cvr_body::cmath::{atan2, max};
use cvr_body::policy::{is_vrik_body_movement_allowed, is_vrik_pose_tier_allowed};
use cvr_body::roomscale::{accepted_displacement, physical_velocity, LeadLimit, Movement, PhysicalWalkGate, Reason, Sample, Step};
use cvr_body::vec2::{dot, finite, rotate, v2, Vec2};
use cvr_camera::anchor::{compose_view_translation, AnchorRecipe, AnchorVector};
use cvr_engine::code::{expect, read, Pattern};
use cvr_engine::hooks::{install_chain, Detour};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{LazyLock, Mutex};

/// The POD snapshot for x64dbg and offline probes (CyberpunkVR_RoomscaleDebug); read while paused, or bracketed by
/// the even, unchanged sequence. Diagnostics never write game state.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct RoomscaleDebug {
    pub calls: u64,
    pub injected: u64,
    pub feedback_reads: u64,
    pub resets: u64,
    pub player: usize,
    pub movement: usize,
    pub backend: usize,
    pub pose_sequence: u64,
    pub origin: u64,
    /// native binding / CCT enabled / gameplay / tracking
    pub reason: u32,
    pub gates: u32,
    pub dt: f32,
    pub yaw: f32,
    pub native: [f32; 3],
    pub requested: [f32; 3],
    pub before: [f32; 3],
    pub after: [f32; 3],
    pub velocity: [f32; 3],
    pub consumed: [f32; 2],
    pub feedback: [f32; 2],
    /// The controller's share of the physical step; the view's lead over the capsule (m).
    pub accepted: [f32; 2],
    pub lead: [f32; 2],
    pub lead_radius: f32,
    pub eye_base: [f32; 2],
    /// Strict: the physical velocity the readers other than the solver see (m/s).
    pub reported: [f32; 2],
    pub strict: u32,
    pub walking: u32,
}
const _: () = assert!(core::mem::size_of::<RoomscaleDebug>() == 208);

extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static CyberpunkVR_HeadLeadsCapsule: i32;
    static CyberpunkVR_BodyStrictUnderHead: i32;
    static mut CyberpunkVR_RoomscaleDebug: RoomscaleDebug;
    static CyberpunkVR_RoomscaleDebugSeq: AtomicU32;
    static mut CyberpunkVR_RoomscaleHooksReady: i32;
    static mut CyberpunkVR_SwimmingFeedbackReads: [u64; 2];
    static mut CyberpunkVR_SwimmingPropertyFeedbackReads: u64;
    static mut CyberpunkVR_SwimmingRedirect: [f32; 3];
    static mut CyberpunkVR_EngineBodyYawValid: i32;
    static mut CyberpunkVR_PlayerEntityValid: i32;
    static g_VRIKClockQpc: Option<unsafe extern "C" fn() -> i64>;
    fn Get3DofMovement() -> i32;
    // src/Hooks/HookServices.cpp (and the camera's, src/Camera/CameraServices.cpp)
    fn cvr_cpp_xr_tracking_origin_serial() -> u64;
    fn cvr_xr_get_roomscale_sample(out: *mut Sample, frame_head: *mut HeadPose, frame_time: *mut i64) -> bool;
    fn cvr_xr_flush_hands(frame_head: *const HeadPose, frame_time: i64, consumed: *const Vec2);
    fn cvr_tweak_capsule_radius() -> f32; // cvr-ffi native_tweaks
    fn cvr_cpp_camera_player_changed();
    fn cvr_cpp_device_cam_active() -> bool;
    fn cvr_cpp_braindance_active() -> bool;
    fn cvr_cpp_menu_mode() -> i32;
    fn cvr_cpp_lobby_world_shown() -> bool;
    fn cvr_cpp_swimming_active() -> bool;
    fn cvr_cpp_swimming_read_steering(direction: *mut [f32; 3], state: *mut i32) -> bool;
    fn cvr_cpp_ladder_active() -> bool;
    fn cvr_cpp_vrik_fbt_body_active() -> bool;
    fn cvr_cpp_fbt_calibrating() -> bool;
    fn cvr_cpp_pose_scene_tier() -> i32;
    fn cvr_cpp_pose_effective_cutscene_tier() -> i32;
    fn cvr_cpp_pose_heading_ready() -> bool;
}

/// The g_liveControls fields the roomscale and body-turn hooks read (src/Hooks/HookServices.cpp fills them).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct BodyControls {
    pub roomscale_movement: i32,
    pub cutscene_suspend_tier: i32,
    pub body_rotation_mode: i32,
    pub body_move_radius: f32,
    pub body_free_look_deg: f32,
    pub body_free_look_down_deg: f32,
    pub body_free_look_swim_deg: f32,
}
pub fn controls() -> BodyControls {
    let mut c = BodyControls::default();
    // SAFETY: fills the struct.
    unsafe { super::live_reads::body_controls(&mut c) };
    c
}

// ---- the sites (CP2077 2.31; docs/cp2077-native-vr-off-21236.md, roomscale_hook_abi_20260919.md)

pub const CCT_MOVE: usize = 0x1C75B4;
const CCT_MOVE_BYTES: &str = "48 8B C4 48 89 58 10 48 89 70 18 48 89 78 20 55 41 54 41 55 41 56 41 57 48 8D 68 A8 48 81 EC 30 01 00 00";
pub const VELOCITY: usize = 0x260881C;
const VELOCITY_BYTES: &str = "40 53 48 83 EC 20 48 8B DA 45 33 C0 8B 91 C4 00 00 00 48 8B CB E8";
const SOLVER_CALL: usize = 0x90B2EB;
const SOLVER_CALL_BYTES: &str = "E8 2C D5 CF 01";
pub const SOLVER_VELOCITY_RETURN: usize = 0x90B2F0;
pub const SWIMMING_DIVING_VELOCITY_RETURN: usize = 0x2EF1676;
pub const SWIMMING_SURFACE_VELOCITY_RETURN: usize = 0x2EF250A;
pub const PHYSICS_PROPERTY: usize = 0x27D19C;
const PHYSICS_PROPERTY_BYTES: &str = "48 8B C4 48 89 58 08 48 89 68 10 48 89 70 18 48 89 78 20 41 56 48 83 EC 50 41 8B E8 8B DA 44 8B";
pub const PHYSICAL_PROVIDER_VTABLE: usize = 0x3098448;
pub const PHYSICAL_ADAPTER_VTABLE: usize = 0x3098390;
pub const CCT_BACKEND_VTABLE: usize = 0x31EB5E8;

type MoveFn = unsafe extern "system" fn(*mut c_void, *mut f32, f32) -> usize;
type VelocityFn = unsafe extern "system" fn(*mut c_void, *mut f32) -> *mut f32;
type PropertyFn = unsafe extern "system" fn(*mut c_void, u32, u32, u32, u32, *mut c_void, u32) -> usize;

static mut ORIG_MOVE: *mut c_void = core::ptr::null_mut();
static mut ORIG_VELOCITY: *mut c_void = core::ptr::null_mut();
static mut ORIG_PROPERTY: *mut c_void = core::ptr::null_mut();
static BASE: AtomicUsize = AtomicUsize::new(0);
static PLAYER: AtomicUsize = AtomicUsize::new(0);
static MOVEMENT: AtomicUsize = AtomicUsize::new(0);
static BODY_FOLLOWING: AtomicBool = AtomicBool::new(false);

struct State {
    motion: Movement,
    water_mode: bool,
    walk_gate: PhysicalWalkGate,
    /// The last move was strict: the readers get `reported` in place of `feedback`.
    strict: bool,
    reported: Vec2,
    feedback: Vec2,
    feedback_z: f32,
    feedback_backend: usize,
    feedback_provider: usize,
    feedback_handle: u32,
    feedback_us: u64,
    /// The visible body's dead zones (main's 0.2.3; off by default) and the tracking origin they belong to.
    head_led_body: cvr_body::dead_zones::BodyDeadZones,
    body_zones_origin: u64,
    body_zones_active: bool,
}
static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| {
    Mutex::new(State {
        motion: Movement::default(),
        water_mode: false,
        walk_gate: PhysicalWalkGate::default(),
        strict: false,
        reported: Vec2::default(),
        feedback: Vec2::default(),
        feedback_z: 0.0,
        feedback_backend: 0,
        feedback_provider: 0,
        feedback_handle: 0,
        feedback_us: 0,
        head_led_body: cvr_body::dead_zones::BodyDeadZones::default(),
        body_zones_origin: 0,
        body_zones_active: false,
    })
});
fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}
fn diagnostics() -> bool {
    // SAFETY: a plain exported int.
    unsafe { CyberpunkVR_RuntimeDiagnostics != 0 }
}

/// XrDiagNowUs (QPC in microseconds, the multiplication wrapping as MSVC's does), or the tests' counter (1 MHz).
pub fn now_us() -> u64 {
    // SAFETY: a function pointer the tests set before a trace and clear after it.
    if let Some(clock) = unsafe { g_VRIKClockQpc } {
        return unsafe { clock() } as u64;
    }
    let (t, f) = super::env::qpc();
    if f == 0 {
        return 0;
    }
    (t.wrapping_mul(1_000_000) / f) as u64
}

fn rd<T: Copy>(address: usize, offset: usize) -> Option<T> {
    read::<T>(address.wrapping_add(offset))
}

#[derive(Clone, Copy, Default)]
struct NativeState {
    movement: usize,
    provider: usize,
    backend: usize,
    handle: u32,
    yaw: f32,
    position: [f32; 3],
    velocity: [f32; 3],
    enabled: bool,
}

/// The player's backend, its movement state, provider and adapter, all cross-checked; None for anything else.
fn read_native(backend: usize, out: &mut NativeState) -> bool {
    (|| -> Option<()> {
        let base = BASE.load(Ordering::Relaxed);
        let player = PLAYER.load(Ordering::Acquire);
        let state = MOVEMENT.load(Ordering::Acquire);
        if player == 0 || state == 0 || rd::<usize>(backend, 8)? != player || rd::<usize>(backend, 0)? != base + CCT_BACKEND_VTABLE {
            return None;
        }
        if rd::<usize>(state, 0x90)? != player {
            return None;
        }
        let provider = rd::<usize>(state, 0xA8)?;
        let adapter = rd::<usize>(state, 0x160)?;
        if provider == 0 || adapter == 0 {
            return None;
        }
        if rd::<usize>(provider, 0)? != base + PHYSICAL_PROVIDER_VTABLE
            || rd::<usize>(provider, 0x10)? != state
            || rd::<usize>(adapter, 0)? != base + PHYSICAL_ADAPTER_VTABLE
            || rd::<usize>(adapter, 0x18)? != state
        {
            return None;
        }
        let handle = rd::<u32>(backend, 0x30)?;
        if handle != rd::<u32>(provider, 0xC4)? || handle != rd::<u32>(adapter, 0x40)? {
            return None;
        }
        out.movement = state;
        out.provider = provider;
        out.backend = backend;
        out.handle = handle;
        out.enabled = rd::<u8>(backend, 0x185)? != 0 && rd::<u8>(state, 0x8B)? != 0;
        let (z, w) = (rd::<f32>(state, 0x1D8)?, rd::<f32>(state, 0x1DC)?);
        if !z.is_finite() || !w.is_finite() || z * z + w * w < 0.5 {
            return None;
        }
        out.yaw = 2.0 * atan2(z, w);
        for i in 0..3 {
            out.position[i] = rd::<f32>(backend, 0x108 + 4 * i)?;
            out.velocity[i] = rd::<f32>(backend, 0x114 + 4 * i)?;
            if !out.position[i].is_finite() || !out.velocity[i].is_finite() {
                return None;
            }
        }
        Some(())
    })()
    .is_some()
}

/// How far from the capsule's axis the eye may lead it at an obstacle: within the capsule's own footprint the
/// controller keeps space free, so the view cannot enter geometry; the clearance and the controller's 7 cm contact
/// offset keep the face and the near plane 12 cm off the surface the capsule touches.
const LEAD_CLEARANCE: f32 = 0.05;
const DEFAULT_CAPSULE_RADIUS: f32 = 0.25;
static CAPSULE_RADIUS: AtomicU32 = AtomicU32::new(0);
static CAPSULE_ATTEMPTS: AtomicI32 = AtomicI32::new(0);

fn head_lead_limit() -> LeadLimit {
    let mut r = f32::from_bits(CAPSULE_RADIUS.load(Ordering::Relaxed));
    if r <= 0.0 {
        // SAFETY: the TweakDB read, its own lock inside.
        r = if CAPSULE_ATTEMPTS.fetch_add(1, Ordering::Relaxed) < 600 { unsafe { cvr_tweak_capsule_radius() } } else { 0.0 };
        if r > 0.0 {
            CAPSULE_RADIUS.store(r.to_bits(), Ordering::Relaxed);
            log(&format!("[roomscale] capsule radius {:.3}\n", r));
        } else {
            r = DEFAULT_CAPSULE_RADIUS;
        }
    }
    let mut limit = LeadLimit { radius: max(r - LEAD_CLEARANCE, 0.0), ..LeadLimit::default() };
    // The view's offset from the entity before any tracking offset: the camera's calibration, as the view composes it.
    let mut recipe = AnchorRecipe::default();
    // SAFETY: writes the recipe.
    if unsafe { cvr_camera_anchor_recipe_read(&mut recipe) } {
        let base = compose_view_translation(AnchorVector::default(), &recipe, recipe.tracking_yaw, recipe.model_yaw);
        if base.x.is_finite() && base.y.is_finite() {
            limit.eye_base = v2(base.x, base.y);
        }
    }
    limit
}

fn is_backend_player(backend: usize) -> bool {
    let player = PLAYER.load(Ordering::Acquire);
    player != 0 && rd::<usize>(backend, 8) == Some(player)
}

static LAST_LOG_US: AtomicU64 = AtomicU64::new(0);

/// The CCT move: the headset's step added to the native request, then the ledger told what the controller accepted.
#[no_mangle]
pub unsafe extern "C" fn cvr_roomscale_move(backend: *mut c_void, displacement: *mut f32, dt: f32) -> usize {
    let original: MoveFn = core::mem::transmute(ORIG_MOVE);
    let b = backend as usize;
    if !is_backend_player(b) {
        return original(backend, displacement, dt);
    }
    let player = PLAYER.load(Ordering::Acquire);
    let mut sample = Sample::default();
    let mut frame_head = HeadPose::default();
    let mut frame_time = 0i64;
    cvr_xr_get_roomscale_sample(&mut sample, &mut frame_head, &mut frame_time);
    let mut before = NativeState::default();
    let native_ok = read_native(b, &mut before);
    let allowed = native_ok && before.enabled && cvr_roomscale_gameplay_allowed();
    let yaw = cvr_body_tracking_yaw(before.yaw);
    let water_mode = cvr_cpp_swimming_active();
    // On land, and not under full-body tracking (the trackers place the body there).
    let strict = CyberpunkVR_BodyStrictUnderHead != 0 && !water_mode && !cvr_cpp_vrik_fbt_body_active();
    let step: Step = {
        let mut s = state();
        // Removing/reinstating the land hinge is a posture change, not a physical step: keep the camera ledger.
        if water_mode != s.water_mode {
            s.motion.suspend();
            s.water_mode = water_mode;
        }
        let radius = controls().body_move_radius;
        let step = s.motion.begin(&sample, now_us(), dt, cvr_camera_world_scale(), yaw, allowed, radius, strict);
        // The bend tracker's walking mode: in strict mode a walk is the gate's walk, not every move of the head.
        let following = if strict { s.walk_gate.walking() } else { s.motion.following() };
        BODY_FOLLOWING.store(following, Ordering::Relaxed);
        step
    };
    let native = [*displacement, *displacement.add(1), *displacement.add(2)];
    let mut swim_direction = [0.0f32; 3];
    let mut swim_state = 0i32;
    let steer = native_ok && before.enabled && cvr_cpp_swimming_read_steering(&mut swim_direction, &mut swim_state);
    let mut redirected = if steer { cvr_body::swim::redirect(native, swim_direction, yaw) } else { native };
    // THE HANDS PUSH THE BODY (physhands' body_push): a hand held off by a wall it is pushed into sends the player away
    // from it, as a stick's move would (not the headset's share: the ledger keeps the head where it is)
    if allowed && !water_mode {
        if let Some(push) = super::physhands::api::body_push().filter(|p| (p[0] + p[1]).is_finite()) {
            redirected[0] += push[0] * dt;
            redirected[1] += push[1] * dt;
        }
        // ...and the hands that hold on pull it after them, up as well (body_pull: climbing onto a railing, up out of a
        // crouch); holding on, the hands carry the body: the game's own fall is left out
        if let Some(pull) = super::physhands::api::body_pull().filter(|p| (p[0] + p[1] + p[2]).is_finite()) {
            redirected[0] += pull[0] * dt;
            redirected[1] += pull[1] * dt;
            redirected[2] = pull[2] * dt;
        }
    }
    let added = [
        redirected[0] - native[0] + step.world.x,
        redirected[1] - native[1] + step.world.y,
        redirected[2] - native[2] + 0.0,
    ];
    let mut combined = [native[0] + added[0], native[1] + added[1], native[2] + added[2]];
    if diagnostics() {
        CyberpunkVR_SwimmingRedirect = [redirected[0] - native[0], redirected[1] - native[1], redirected[2] - native[2]];
    }
    let result = original(backend, combined.as_mut_ptr(), dt);
    // Not under full-body tracking: there the trackers place the body, and a lead estimated from the capsule's
    // velocity kept growing on open floor -- the body trailed the head and slid after it.
    let head_leads = CyberpunkVR_HeadLeadsCapsule != 0 && !cvr_cpp_vrik_fbt_body_active();
    let mut lead_limit = if head_leads { head_lead_limit() } else { LeadLimit::default() };
    // The ledger walks the neck (eye_from_neck); the limit is the eye's, so the lever goes back on.
    if head_leads && finite(sample.eye_from_neck) {
        lead_limit.eye_base = lead_limit.eye_base + rotate(sample.eye_from_neck * cvr_camera_world_scale(), yaw);
    }
    let mut after = NativeState::default();
    let result_ok = native_ok && read_native(b, &mut after);
    let completed_consumed;
    {
        let mut s = state();
        if PLAYER.load(Ordering::Acquire) != player {
            return result;
        }
        // What the controller did with the headset's part; the stick's and the swim redirect's are the rest.
        let accepted = if result_ok && head_leads {
            accepted_displacement(v2(redirected[0], redirected[1]), step.world, v2(after.velocity[0] * dt, after.velocity[1] * dt))
        } else {
            step.world
        };
        s.motion.complete(&step, accepted, lead_limit);
        completed_consumed = s.motion.consumed(sample.origin);
        // THE VISIBLE BODY'S DEAD ZONES (off by default): it stays in the world while the neck -- the capsule -- moves
        // under it, and is placed again only when the head leaves its zones. The neck's move is what the controller
        // accepted: a step the wall refused moved neither the capsule nor the body.
        let body_zones = super::state_globals::CyberpunkVR_BodyDeadZones != 0
            && strict
            && result_ok
            && controls().body_rotation_mode == cvr_body::hybrid_yaw::RotationMode::HeadLed as i32;
        if !body_zones || sample.origin != s.body_zones_origin {
            s.head_led_body.reset();
            s.body_zones_origin = sample.origin;
        }
        if body_zones && step.reason == Reason::Applied && step.scale > 0.0 && frame_head.valid {
            let moved = rotate(accepted, -step.yaw) * (1.0 / step.scale);
            // tracking -> world turns by `yaw`, the body faces before.yaw
            let body_in_tracking = before.yaw - yaw;
            let forward = v2(-body_in_tracking.sin(), body_in_tracking.cos());
            let o = &frame_head.ori;
            let pitch = -cvr_body::look_down::head_down_degrees(o[0], o[1], o[2], o[3]);
            let omega = if super::state_globals::CyberpunkVR_BodyDeadZoneEase != 0 {
                cvr_body::dead_zones::BodyDeadZones::SHOW_OMEGA
            } else {
                0.0
            };
            let offset = s.head_led_body.step(moved, sample.eye_from_neck, forward, pitch, step.scale, step.dt, omega);
            let st = s.head_led_body.last();
            let shown = s.head_led_body.shown();
            super::state_globals::CyberpunkVR_DebugBodyZones = [
                offset.x,
                offset.y,
                f32::from(u8::from(st.looking_down)),
                f32::from(u8::from(st.breached)),
                f32::from(u8::from(st.inner)),
                st.distance,
                shown.x,
                shown.y,
            ];
        }
        s.body_zones_active = body_zones && s.head_led_body.valid();
        s.feedback = if result_ok {
            physical_velocity(v2(native[0], native[1]), v2(added[0], added[1]), v2(after.velocity[0], after.velocity[1]), dt)
        } else {
            Vec2::default()
        };
        // Pitch and buoyancy are already part of the native vertical solve.
        s.feedback_z = 0.0;
        if strict && result_ok {
            let fb = s.feedback;
            s.walk_gate.update(fb, dt);
        } else {
            s.walk_gate.reset();
        }
        s.strict = strict && result_ok;
        s.reported = s.walk_gate.reported();
        s.feedback_backend = if result_ok { before.backend } else { 0 };
        s.feedback_provider = if result_ok { before.provider } else { 0 };
        s.feedback_handle = if result_ok { before.handle } else { 0 };
        s.feedback_us = now_us();
        if diagnostics() {
            CyberpunkVR_RoomscaleDebugSeq.fetch_add(1, Ordering::AcqRel);
            let d = &mut *core::ptr::addr_of_mut!(CyberpunkVR_RoomscaleDebug);
            d.calls += 1;
            if dot(step.world, step.world) > 0.0 {
                d.injected += 1;
            }
            if step.reason == Reason::Baseline || step.reason == Reason::Discontinuity {
                d.resets += 1;
            }
            d.player = PLAYER.load(Ordering::Relaxed);
            d.movement = before.movement;
            d.backend = before.backend;
            d.pose_sequence = sample.sequence;
            d.origin = sample.origin;
            d.reason = step.reason as u32;
            d.dt = dt;
            d.yaw = yaw;
            d.gates = (native_ok as u32) | ((before.enabled as u32) << 1) | ((cvr_roomscale_gameplay_allowed() as u32) << 2) | ((sample.valid as u32) << 3);
            for i in 0..3 {
                d.native[i] = native[i];
                d.before[i] = before.position[i];
                d.after[i] = after.position[i];
                d.velocity[i] = after.velocity[i];
            }
            d.requested = [step.world.x, step.world.y, 0.0];
            let consumed = s.motion.consumed(sample.origin);
            d.consumed = [consumed.x, consumed.y];
            d.feedback = [s.feedback.x, s.feedback.y];
            d.reported = [s.reported.x, s.reported.y];
            d.strict = s.strict as u32;
            d.walking = s.walk_gate.walking() as u32;
            let lead = s.motion.pending() * cvr_camera_world_scale();
            d.accepted = [accepted.x, accepted.y];
            d.lead = [lead.x, lead.y];
            d.lead_radius = lead_limit.radius;
            d.eye_base = [lead_limit.eye_base.x, lead_limit.eye_base.y];
            CyberpunkVR_RoomscaleDebugSeq.fetch_add(1, Ordering::Release);
            if s.feedback_us.wrapping_sub(LAST_LOG_US.load(Ordering::Relaxed)) >= 1_000_000 {
                LAST_LOG_US.store(s.feedback_us, Ordering::Relaxed);
                log(&format!(
                    "[roomscale] ticks={} moves={} reason={} gates={:X} seq={} origin={} dt={:.4} request=({:.5},{:.5}) consumed=({:.3},{:.3}) velocity=({:.3},{:.3}) feedback=({:.3},{:.3})\n",
                    d.calls, d.injected, d.reason, d.gates, d.pose_sequence, d.origin, dt, step.world.x, step.world.y, consumed.x,
                    consumed.y, after.velocity[0], after.velocity[1], s.feedback.x, s.feedback.y
                ));
            }
        }
    }
    // Publish after complete: the head and the consumed displacement must name the same native move.
    if native_ok && before.enabled && cvr_roomscale_pose_frame_allowed() && sample.valid && frame_head.valid && frame_time > 0 {
        cvr_xr_flush_hands(&frame_head, frame_time, &completed_consumed);
    }
    result
}

/// The provider's velocity getter, given who asked: the solvers (land and both swims) get the physical part taken
/// off; every other reader -- the locomotion animation among them -- the full velocity, or in strict mode the
/// physical part only as a walk.
#[no_mangle]
pub unsafe extern "C" fn cvr_roomscale_velocity_from(provider: *mut c_void, output: *mut f32, caller: usize) -> *mut f32 {
    let original: VelocityFn = core::mem::transmute(ORIG_VELOCITY);
    let result = original(provider, output);
    // Water queries share the physics-property reader below: do not subtract twice.
    if cvr_cpp_swimming_active() {
        return result;
    }
    let base = BASE.load(Ordering::Relaxed);
    let swim_diving = caller == base + SWIMMING_DIVING_VELOCITY_RETURN;
    let swim_surface = caller == base + SWIMMING_SURFACE_VELOCITY_RETURN;
    let solver = caller == base + SOLVER_VELOCITY_RETURN || swim_diving || swim_surface;
    if !solver {
        if CyberpunkVR_BodyStrictUnderHead == 0 || !cvr_roomscale_is_player_provider(provider) {
            return result;
        }
        let now = now_us();
        let s = state();
        if !s.strict || s.feedback_backend == 0 || provider as usize != s.feedback_provider || now < s.feedback_us || now - s.feedback_us > Movement::MAX_AGE_US {
            return result;
        }
        *result += s.reported.x - s.feedback.x;
        *result.add(1) += s.reported.y - s.feedback.y;
        return result;
    }
    if !cvr_roomscale_is_player_provider(provider) {
        return result;
    }
    let now = now_us();
    let s = state();
    if s.feedback_backend == 0 || provider as usize != s.feedback_provider || now < s.feedback_us || now - s.feedback_us > Movement::MAX_AGE_US {
        return result;
    }
    *result -= s.feedback.x;
    *result.add(1) -= s.feedback.y;
    if swim_diving || swim_surface {
        *result.add(2) -= s.feedback_z;
        if diagnostics() {
            CyberpunkVR_SwimmingFeedbackReads[swim_surface as usize] += 1;
        }
    }
    if diagnostics() {
        CyberpunkVR_RoomscaleDebugSeq.fetch_add(1, Ordering::AcqRel);
        (*core::ptr::addr_of_mut!(CyberpunkVR_RoomscaleDebug)).feedback_reads += 1;
        CyberpunkVR_RoomscaleDebugSeq.fetch_add(1, Ordering::Release);
    }
    result
}

/// The MinHook detour: the getter's return address names its caller.
#[unsafe(naked)]
unsafe extern "system" fn velocity_entry(_provider: *mut c_void, _output: *mut f32) -> *mut f32 {
    core::arch::naked_asm!("mov r8, [rsp]", "jmp {body}", body = sym cvr_roomscale_velocity_from)
}

/// The physics-property reader: Property 4 is the resolved linear velocity (12 bytes), shared by native swimming and
/// the script state-vector readers -- the water solver's direct and cached reads had escaped the provider sites.
#[no_mangle]
pub unsafe extern "C" fn cvr_roomscale_property(registry: *mut c_void, handle: u32, flags: u32, mode: u32, property: u32, output: *mut c_void, bytes: u32) -> usize {
    let original: PropertyFn = core::mem::transmute(ORIG_PROPERTY);
    let result = original(registry, handle, flags, mode, property, output, bytes);
    if result & 0xff == 0 || property != 4 || bytes != 12 || output.is_null() || !cvr_cpp_swimming_active() {
        return result;
    }
    let now = now_us();
    let s = state();
    if s.feedback_backend == 0 || handle != s.feedback_handle || now < s.feedback_us || now - s.feedback_us > Movement::MAX_AGE_US {
        return result;
    }
    let v = output as *mut f32;
    if !(*v).is_finite() || !(*v.add(1)).is_finite() || !(*v.add(2)).is_finite() {
        return result;
    }
    *v -= s.feedback.x;
    *v.add(1) -= s.feedback.y;
    *v.add(2) -= s.feedback_z;
    if diagnostics() {
        CyberpunkVR_SwimmingPropertyFeedbackReads += 1;
    }
    result
}

/// The image base and the three originals: the install's, or the tests' fakes.
#[no_mangle]
pub unsafe extern "C" fn cvr_roomscale_bind(base: usize, move_fn: *mut c_void, velocity: *mut c_void, property: *mut c_void) {
    BASE.store(base, Ordering::Relaxed);
    ORIG_MOVE = move_fn;
    ORIG_VELOCITY = velocity;
    ORIG_PROPERTY = property;
}

/// Installed from src/Hooks/RoomscaleMove.cpp's registration (Boot, 77).
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_roomscale_install() -> bool {
    let base = cvr_engine::code::image_base();
    BASE.store(base, Ordering::Relaxed);
    let site = |rva: usize, bytes: &str| Pattern::parse(bytes).is_some_and(|p| expect(base + rva, &p));
    if base == 0
        || !site(CCT_MOVE, CCT_MOVE_BYTES)
        || !site(VELOCITY, VELOCITY_BYTES)
        || !site(SOLVER_CALL, SOLVER_CALL_BYTES)
        || !site(PHYSICS_PROPERTY, PHYSICS_PROPERTY_BYTES)
    {
        log("Roomscale: native ABI signature mismatch; hooks not installed.\n");
        return false;
    }
    let detours = [
        Detour {
            name: "CctMove",
            target: CCT_MOVE,
            absolute: false,
            detour: cvr_roomscale_move as *const c_void,
            original: core::ptr::addr_of_mut!(ORIG_MOVE),
            expect: Some(CCT_MOVE_BYTES),
        },
        Detour {
            name: "Velocity",
            target: VELOCITY,
            absolute: false,
            detour: velocity_entry as *const c_void,
            original: core::ptr::addr_of_mut!(ORIG_VELOCITY),
            expect: Some(VELOCITY_BYTES),
        },
        Detour {
            name: "PhysicsProperty",
            target: PHYSICS_PROPERTY,
            absolute: false,
            detour: cvr_roomscale_property as *const c_void,
            original: core::ptr::addr_of_mut!(ORIG_PROPERTY),
            expect: Some(PHYSICS_PROPERTY_BYTES),
        },
    ];
    if install_chain("roomscale", &detours).is_err() {
        return false;
    }
    CyberpunkVR_RoomscaleHooksReady = 1;
    log("Roomscale: native CCT displacement + solver-only velocity feedback installed (2.31).\n");
    true
}

// ---- the player, and the gates

#[no_mangle]
pub unsafe extern "C" fn cvr_roomscale_set_player(player: usize) {
    if PLAYER.swap(player, Ordering::AcqRel) == player {
        return;
    }
    // A previous puppet can keep animating after a player replacement; its camera counters still advance.
    cvr_cpp_camera_player_changed();
    MOVEMENT.store(0, Ordering::Release);
    let mut s = state();
    s.motion.reset();
    s.feedback = Vec2::default();
    s.feedback_z = 0.0;
    s.feedback_backend = 0;
    s.feedback_provider = 0;
    s.feedback_us = 0;
    CyberpunkVR_EngineBodyYawValid = 0;
    CyberpunkVR_PlayerEntityValid = 0;
    cvr_body_yaw_follow_release();
    cvr_body_heading_reset();
    drop(s);
    log(&format!("Roomscale: player identity -> {:016X}, tracking baseline reset.\n", player));
}
#[no_mangle]
pub extern "C" fn cvr_roomscale_player_identity() -> usize {
    PLAYER.load(Ordering::Acquire)
}
#[no_mangle]
pub extern "C" fn cvr_roomscale_is_player_movement(state: *const c_void) -> bool {
    let player = PLAYER.load(Ordering::Acquire);
    player != 0 && !state.is_null() && rd::<usize>(state as usize, 0x90) == Some(player)
}
#[no_mangle]
pub extern "C" fn cvr_roomscale_is_player_provider(provider: *const c_void) -> bool {
    let p = provider as usize;
    if p == 0 || rd::<usize>(p, 0) != Some(BASE.load(Ordering::Relaxed) + PHYSICAL_PROVIDER_VTABLE) {
        return false;
    }
    let Some(state) = rd::<usize>(p, 0x10) else { return false };
    cvr_roomscale_is_player_movement(state as *const c_void) && rd::<usize>(state, 0xA8) == Some(p)
}
#[no_mangle]
pub extern "C" fn cvr_roomscale_observe_movement(state: *mut c_void) {
    if cvr_roomscale_is_player_movement(state) {
        MOVEMENT.store(state as usize, Ordering::Release);
    }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_roomscale_gameplay_allowed() -> bool {
    let loco = g_VRLocomotionState;
    // Not while full-body tracking calibrates: V stays where it stands and the player walks into it.
    controls().roomscale_movement != 0 && cvr_roomscale_physical_body_allowed() && !cvr_cpp_fbt_calibrating() && loco != 6 && loco != 9 && loco != 10 && loco != 11
}
/// The lobby is a place: its body walks and turns through the native move, so its view takes that move's frame.
unsafe fn on_foot_view_allowed() -> bool {
    (cvr_cpp_menu_mode() == 0 || cvr_cpp_lobby_world_shown()) && !cvr_cpp_device_cam_active() && !cvr_cpp_braindance_active()
}
unsafe fn physical_body_scope_enabled() -> bool {
    !cvr_cpp_ladder_active()
        && !g_pSharedHands.is_null()
        && !cvr_cpp_device_cam_active()
        && !cvr_cpp_braindance_active()
        && is_vrik_body_movement_allowed(g_VRBind, cvr_anim_is_head_aim_weapon_active(), g_isInVehicle, cvr_cpp_pose_scene_tier(), controls().cutscene_suspend_tier)
}
#[no_mangle]
pub unsafe extern "C" fn cvr_roomscale_physical_body_enabled() -> bool {
    physical_body_scope_enabled() && g_VRBoneCount > 0 && g_VRHeadBoneIdx >= 0
}
/// Retains same-frame view cancellation while the rig rebinds: the half-built metadata is not observed.
#[no_mangle]
pub unsafe extern "C" fn cvr_roomscale_physical_body_heading_owned() -> bool {
    physical_body_scope_enabled() && cvr_cpp_pose_heading_ready()
}
#[no_mangle]
pub unsafe extern "C" fn cvr_roomscale_physical_body_allowed() -> bool {
    (cvr_cpp_menu_mode() == 0 || cvr_cpp_lobby_world_shown()) && cvr_roomscale_physical_body_enabled()
}
#[no_mangle]
pub unsafe extern "C" fn cvr_roomscale_pose_frame_allowed() -> bool {
    on_foot_view_allowed() && !g_isInVehicle && is_vrik_pose_tier_allowed(cvr_cpp_pose_scene_tier(), cvr_cpp_pose_effective_cutscene_tier())
}
/// Keeps the offset when movement is disabled: dropping it would apply all distance already travelled a second time.
#[no_mangle]
pub unsafe extern "C" fn cvr_roomscale_camera_consumed_at(origin: u64) -> Vec2 {
    let mut s = state();
    if !cvr_roomscale_gameplay_allowed() {
        s.motion.suspend();
    }
    s.motion.consumed(origin)
}
#[no_mangle]
pub unsafe extern "C" fn cvr_roomscale_camera_consumed() -> Vec2 {
    cvr_roomscale_camera_consumed_at(cvr_cpp_xr_tracking_origin_serial())
}
#[no_mangle]
pub unsafe extern "C" fn cvr_roomscale_camera_rest_neck_at(origin: u64, out: *mut Vec2) -> bool {
    if Get3DofMovement() != 0 {
        return false;
    }
    match state().motion.rest_neck(origin) {
        Some(v) => {
            if !out.is_null() {
                *out = v;
            }
            true
        }
        None => false,
    }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_roomscale_camera_rest_neck(out: *mut Vec2) -> bool {
    cvr_roomscale_camera_rest_neck_at(cvr_cpp_xr_tracking_origin_serial(), out)
}
/// StrictTickEye: the eye as the capsule's last move saw it, while the last move was strict (the visible body under the
/// rendered head, pose_hook.rs).
pub unsafe fn strict_tick_eye() -> Option<Vec2> {
    if Get3DofMovement() != 0 || super::state_globals::CyberpunkVR_BodyUnderRenderedHead == 0 {
        return None;
    }
    let origin = cvr_cpp_xr_tracking_origin_serial();
    let s = state();
    if !s.strict {
        return None;
    }
    s.motion.tick_eye(origin)
}
/// BodyDeadZoneOffset: the shown body less the neck while the dead zones are on.
pub unsafe fn body_dead_zone_offset() -> Option<Vec2> {
    if Get3DofMovement() != 0 {
        return None;
    }
    let origin = cvr_cpp_xr_tracking_origin_serial();
    let s = state();
    if !s.body_zones_active || origin != s.body_zones_origin || !s.head_led_body.valid() {
        return None;
    }
    let shown = s.head_led_body.shown();
    finite(shown).then_some(shown)
}
#[no_mangle]
pub extern "C" fn cvr_roomscale_body_following() -> bool {
    BODY_FOLLOWING.load(Ordering::Relaxed)
}
