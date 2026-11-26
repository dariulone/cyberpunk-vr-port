//! NIGHTCOOP: the VR body as partners see it (main's bd7adc549, src/Natives/NightCoopBridge.cpp). When NightCoop
//! (co-op) is installed, it is looked up from the main thread and given our solved rig in world space through its SDK's
//! pose source; nothing of it is linked (externals/nightcoop-sdk: the SDK's header, MIT, read for the layouts below).
//!
//! WHAT GOES OUT, AND IN WHICH CONVENTION (NightCoop 0.2.1's SDK and its own pose simulator, which feeds the same
//! poseSet): every transform in WORLD space (NCSDK_POSE_WORLD_SPACE); NightCoop turns it into our body's space with the
//! player's own transform at the call, and the partner puts it back on our mirror there.
//!   head    the head BONE's position, and the HMD's world rotation in the game's camera convention (identity looks
//!           along +Y with +Z up). NightCoop measures which local axis of the head bone looks forward in the rig's
//!           reference pose and turns the bone by this look; the position is what its neck reaches for.
//!   hands   the LeftHand / RightHand bones as the solved rig has them: the bone's own world rotation, not a
//!           controller frame -- NightCoop's arm IK puts the partner's hand bone exactly there.
//!   fingers the curl of each digit as V's hand draws it, 0 straight .. 1 fist, left thumb..little then right; -1 where
//!           the game's graph poses the hand (a weapon, a held magazine): the partner's game keeps its grip there.
//!   aim     the muzzle and its forward line while a ranged weapon is out, so the partner's copy of our shots leaves
//!           our barrel, not our view.
//!   crouch  the physical squat, 0 standing .. 1 at NightCoop's full squat (its simulator drops the head 0.45 m).
//!
//! WHERE IT COMES FROM: the solved-rig snapshot (g_VRFKSnap*, model space, published by the pose path while the diag
//! capture is on -- VRIK keeps it on whenever tracking drives the body) put in the world by the model-to-world pair the
//! menu's pointers use (the camera's model point and rotation against the view's world pose). A snapshot that stopped
//! moving (VRIK suspended by a scene, a menu, a load) is old: the source is taken off and the partners' game shows our
//! body by its own graph again.
use crate::math::{quat_conj, quat_mul, quat_rotate_vec, Q, V3};
use core::ffi::{c_char, c_void};
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering::*};
use std::sync::Mutex;
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::System::SystemInformation::GetTickCount64;

#[allow(non_upper_case_globals)]
extern "C" {
    static g_VRViewWorldPos: [f32; 3];
    static g_VRViewWorldRot: [f32; 4];
    static g_VRViewWorldValid: i32;
    static g_VRCamModelPos: [f32; 3];
    static g_VRCamModelRot: [f32; 4];
    static g_VRPalmModelValid: i32;
    static g_VRFKSnapCount: i32;
    static g_VRFKSnapPos: [[f32; 3]; super::MAX_BONES];
    static g_VRFKSnapRot: [[f32; 4]; super::MAX_BONES];
    static CyberpunkVR_PlayerEntityValid: i32;
    static CyberpunkVR_PhysicalCrouchDepth: f32;
    static CyberpunkVR_FingerBones: i32;
    static CyberpunkVR_GestureStraight: [[f32; 5]; 2];
}

// ---------------------------------------------------------------- the exports (main's names, read by the overlay)
/// 1 = the VR body goes to NightCoop as a pose source (live). 0 = the partners see the game's own animation of V.
#[no_mangle]
pub static mut CyberpunkVR_NightCoopPose: i32 = 1;
/// 1 = in a vehicle the hands go too; 0 (default) = the head and the aim only: NightCoop's own seat pose keeps the
/// partner's copy of our hands on its mirror of the wheel, which our wheel hands would not match.
#[no_mangle]
pub static mut CyberpunkVR_NightCoopVehicleHands: i32 = 0;
/// 0 NightCoop not found (yet), 1 found, its SDK table taken, 2 registered, -1 refused / its SDK switched off, -2
/// NightCoop unloaded.
#[no_mangle]
pub static CyberpunkVR_NightCoopLink: AtomicI32 = AtomicI32::new(0);
/// The bits of the last pose given (NCSDK_POSE_*), the answer of the last poseSet, how many poseSet calls were taken.
#[no_mangle]
pub static CyberpunkVR_NightCoopPoseBits: AtomicI32 = AtomicI32::new(0);
#[no_mangle]
pub static CyberpunkVR_NightCoopPoseResult: AtomicI32 = AtomicI32::new(0);
#[no_mangle]
pub static CyberpunkVR_NightCoopPoseSets: AtomicU32 = AtomicU32::new(0);
/// NightCoop's frame calls seen, and what stopped the last one short of poseSet: 0 none (sent), 1 switched off, 2 no VR
/// session, 3 the VR arms not bound, 4 the solved rig not published lately, 5 no model-to-world frame, 6 no head or
/// hand.
#[no_mangle]
pub static CyberpunkVR_NightCoopFrames: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_NightCoopGate: AtomicI32 = AtomicI32::new(-1);

// ---------------------------------------------------------------- the SDK's layouts (nightcoop_sdk.h, version 1)
const EXPORT_NAME: &[u8] = b"NightCoopSdkGet\0";
const SDK_VERSION: u32 = 1;
const OK: i32 = 0;
const LOG_INFO: i32 = 1;
const MOD_USES_POSE: u32 = 1 << 11;
const MOD_USES_CHANNELS: u32 = 1 << 8;
/// send(): every partner that has this mod; the latest message of a channel only (a newer one replaces a queued one).
const TO_ALL: u32 = 0xFFFF_FFFE;
pub const SEND_RELIABLE: u32 = 1 << 0;
pub const SEND_LATEST_ONLY: u32 = 1 << 1;
const POSE_HEAD: u32 = 1 << 0;
const POSE_LEFT_HAND: u32 = 1 << 1;
const POSE_RIGHT_HAND: u32 = 1 << 2;
const POSE_FINGERS: u32 = 1 << 3;
const POSE_AIM: u32 = 1 << 4;
const POSE_CROUCH: u32 = 1 << 5;
const POSE_WORLD_SPACE: u32 = 1 << 31;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Vec3 {
    x: f32,
    y: f32,
    z: f32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Quat {
    i: f32,
    j: f32,
    k: f32,
    r: f32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Transform {
    pos: Vec3,
    rot: Quat,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Pose {
    size: u32,
    valid: u32,
    head: Transform,
    left_hand: Transform,
    right_hand: Transform,
    fingers: [f32; 10],
    aim_origin: Vec3,
    aim_dir: Vec3,
    physical_crouch: f32,
}
type Hook = *const c_void;
/// NcSdkHooks: its size, the context, and the callbacks in the header's order (frame, event, message, stateChanged,
/// stateRequest, entityState, partnerPose, sharedSpawn, shutdown, npcRequest, npcResult, coopEvent, menuItem,
/// voteResult, worldRequest, spawnLeader, reward); the ones not set are null.
#[repr(C)]
struct Hooks {
    size: u32,
    context: *mut c_void,
    callbacks: [Hook; 17],
}
const HOOK_FRAME: usize = 0;
const HOOK_EVENT: usize = 1;
const HOOK_MESSAGE: usize = 2;
const HOOK_PARTNER_POSE: usize = 6;
const HOOK_SHUTDOWN: usize = 8;
#[repr(C)]
struct ModInfo {
    size: u32,
    flags: u32,
    id: *const c_char,
    name: *const c_char,
    version: *const c_char,
    protocol: u32,
    reserved: u32,
    hooks: *const Hooks,
}
/// NcSdkApi's fields this bridge calls: size (u32 at 0), then -- after version, protocol, reserved and the build
/// string (16) -- registerMod at 24 and each function 8 further in the header's order.
const API_REGISTER_MOD: usize = 24;
const API_UNREGISTER_MOD: usize = 32;
const API_LOG: usize = 40;
const API_POSE_SET: usize = 208;
const API_POSE_CLEAR: usize = 216;
const API_HOOK_COOP_USES: usize = 448;
const API_SEND: usize = 112;
type SendFn = unsafe extern "C" fn(u32, u32, u16, *const c_void, usize, u32) -> i32;
type HookCoopUses = unsafe extern "C" fn(*const c_char, *const c_char, u64) -> i32;
type GetFn = unsafe extern "C" fn(u32) -> *const u8;
type RegisterMod = unsafe extern "C" fn(*const ModInfo) -> u32;
type ModFn = unsafe extern "C" fn(u32);
type LogFn = unsafe extern "C" fn(u32, i32, *const c_char);
type PoseSet = unsafe extern "C" fn(u32, *const Pose) -> i32;

const MOD_ID: &[u8] = b"cyberpunkvrport.vr\0"; // stable forever: partners' NightCoop keys the source by it
const MOD_NAME: &[u8] = b"CyberpunkVRPort (VR body)\0";
const MOD_VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");
const PROTOCOL: u32 = 1;
/// NightCoop's simulator drops the head by up to 0.45 m at a crouch of 1: the same scale here.
const FULL_SQUAT_METRES: f32 = 0.45;
/// A relaxed hand, neither pulled straight nor closed: what the gesture model's rest pose is worth as a curl.
const REST_CURL: f32 = 0.25;
const STALE_MS: u64 = 250;
const LOOKUP_EVERY_MS: u64 = 1000;
const REFUSED_RETRY_MS: u64 = 30000;
const WEAPON_FLAG: i32 = 144; // Utils/SharedSlots.hpp vrshared::kWeaponFlag

struct Bridge {
    api: usize,
    module: u32,
    next_lookup_ms: u64,
    snap_seq: u32,
    snap_moved_ms: u64,
    muzzle_seq: u32,
    muzzle_moved_ms: u64,
    logged_result: i32,
    logged_bits: u32,
}
static BRIDGE: Mutex<Bridge> = Mutex::new(Bridge {
    api: 0,
    module: 0,
    next_lookup_ms: 0,
    snap_seq: 0,
    snap_moved_ms: 0,
    muzzle_seq: 0,
    muzzle_moved_ms: 0,
    logged_result: 1,
    logged_bits: u32::MAX,
});
/// NightCoop said it is unloading: never looked for again this run.
static GONE: AtomicBool = AtomicBool::new(false);
/// A source of ours is set at NightCoop.
static POSE_ON: AtomicBool = AtomicBool::new(false);
static LAST_FRAME_MS: AtomicU64 = AtomicU64::new(0);

fn log(text: &str) {
    super::env::log(text);
}
/// The table's function at `offset`, when this NightCoop's table has it (NCSDK_HAS).
unsafe fn api_fn(api: usize, offset: usize) -> usize {
    if api == 0 {
        return 0;
    }
    let size = core::ptr::read_unaligned(api as *const u32) as usize;
    if size < offset + 8 {
        return 0;
    }
    core::ptr::read_unaligned((api + offset) as *const usize)
}
fn finite(v: &[f32]) -> bool {
    v.iter().all(|x| x.is_finite())
}
fn normalized(q: &mut Q) -> bool {
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if !n.is_finite() || !(0.5..=1.5).contains(&n) {
        return false;
    }
    for c in q.iter_mut() {
        *c /= n;
    }
    true
}

/// The model-to-world pair of the solved rig: what the VR menu's pointers and the hand collision use.
struct ModelFrame {
    rot: Q,
    cam_model: V3,
    cam_world: V3,
}
unsafe fn model_frame() -> Option<ModelFrame> {
    if core::ptr::read_volatile(&raw const g_VRPalmModelValid) == 0 || core::ptr::read_volatile(&raw const g_VRViewWorldValid) == 0 {
        return None;
    }
    let cam = core::ptr::read_volatile(&raw const g_VRCamModelRot);
    let view = core::ptr::read_volatile(&raw const g_VRViewWorldRot);
    let mut rot = quat_mul(&view, &quat_conj(&cam));
    if !normalized(&mut rot) {
        return None;
    }
    let cam_model = core::ptr::read_volatile(&raw const g_VRCamModelPos);
    let cam_world = core::ptr::read_volatile(&raw const g_VRViewWorldPos);
    (finite(&cam_model) && finite(&cam_world)).then_some(ModelFrame { rot, cam_model, cam_world })
}
/// A bone of the snapshot in the world.
unsafe fn bone_world(f: &ModelFrame, bone: i32) -> Option<Transform> {
    let count = core::ptr::read_volatile(&raw const g_VRFKSnapCount);
    if bone < 0 || bone >= count || bone as usize >= super::MAX_BONES {
        return None;
    }
    let p = core::ptr::read_volatile(&raw const g_VRFKSnapPos[bone as usize]);
    let mut q = core::ptr::read_volatile(&raw const g_VRFKSnapRot[bone as usize]);
    let rel = [p[0] - f.cam_model[0], p[1] - f.cam_model[1], p[2] - f.cam_model[2]];
    if !finite(&rel) || !normalized(&mut q) {
        return None;
    }
    let w = quat_rotate_vec(&f.rot, &rel);
    let mut wq = quat_mul(&f.rot, &q);
    if !normalized(&mut wq) {
        return None;
    }
    Some(Transform {
        pos: Vec3 { x: f.cam_world[0] + w[0], y: f.cam_world[1] + w[1], z: f.cam_world[2] + w[2] },
        rot: Quat { i: wq[0], j: wq[1], k: wq[2], r: wq[3] },
    })
}
/// One hand's five curls: tracked fingers, else the gesture model where it owns the hand, else the game's graph (-1).
unsafe fn hand_curls(side: i32, out: &mut [f32]) -> bool {
    let mut tracked = [0.0f32; 5];
    if core::ptr::read_volatile(&raw const CyberpunkVR_FingerBones) != 0 && super::fingergrip::cvr_fingergrip_tracked_curl(side, tracked.as_mut_ptr()) {
        let t = crate::gestures::from_tracking(&tracked);
        for d in 0..5 {
            out[d] = t.closed[d].clamp(0.0, 1.0);
        }
        return true;
    }
    if !super::gestures::owns_hand(side) {
        out[..5].fill(-1.0);
        return false;
    }
    let straight = core::ptr::read_volatile(&raw const CyberpunkVR_GestureStraight);
    for d in 0..5 {
        let closed = super::gestures::finger_closed(side, d as i32).clamp(0.0, 1.0);
        let s = straight[side as usize][d].clamp(0.0, 1.0);
        out[d] = closed + (1.0 - closed) * (1.0 - s) * REST_CURL;
    }
    true
}
/// The muzzle while a ranged weapon is out: its point and its forward line (shared[24..26]), fresh only.
unsafe fn muzzle_aim(b: &mut Bridge, now: u64) -> Option<(Vec3, Vec3)> {
    let (p, seq) = super::provider::muzzle_world();
    if seq != b.muzzle_seq {
        b.muzzle_seq = seq;
        b.muzzle_moved_ms = now;
    }
    let shared = super::provider::shared_slot;
    if seq == 0 || now - b.muzzle_moved_ms > STALE_MS || shared(WEAPON_FLAG) <= 0.5 || shared(27) <= 0.5 {
        return None;
    }
    let d = [shared(24), shared(25), shared(26)];
    let n = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if !finite(&p) || !n.is_finite() || n < 0.5 {
        return None;
    }
    Some((Vec3 { x: p[0], y: p[1], z: p[2] }, Vec3 { x: d[0] / n, y: d[1] / n, z: d[2] / n }))
}
unsafe fn clear_pose(b: &mut Bridge, gate: i32, why: &str) {
    CyberpunkVR_NightCoopGate.store(gate, Relaxed);
    if !POSE_ON.swap(false, AcqRel) {
        return;
    }
    CyberpunkVR_NightCoopPoseBits.store(0, Relaxed);
    b.logged_bits = u32::MAX;
    let clear = api_fn(b.api, API_POSE_CLEAR);
    if clear != 0 && b.module != 0 {
        core::mem::transmute::<usize, ModFn>(clear)(b.module);
    }
    log(&format!("[nightcoop] pose source off: {why}\n"));
}

/// NightCoop's frame (the game thread, after its own tick).
unsafe extern "C" fn on_frame(_context: *mut c_void, _dt: f32) {
    let Ok(mut b) = BRIDGE.try_lock() else { return };
    if b.api == 0 || b.module == 0 {
        return;
    }
    CyberpunkVR_NightCoopFrames.fetch_add(1, Relaxed);
    let now = GetTickCount64();
    LAST_FRAME_MS.store(now, Relaxed);
    let seq = super::shared_globals::g_VRFKSnapSeq.load(Acquire);
    if seq != b.snap_seq {
        b.snap_seq = seq;
        b.snap_moved_ms = now;
    }
    if core::ptr::read_volatile(&raw const CyberpunkVR_NightCoopPose) == 0 {
        return clear_pose(&mut b, 1, "switched off (CyberpunkVR_NightCoopPose 0)");
    }
    if !super::xr_calls::session_running() {
        return clear_pose(&mut b, 2, "no VR session");
    }
    if core::ptr::read_volatile(&raw const CyberpunkVR_PlayerEntityValid) == 0 || core::ptr::read_volatile(&raw const super::env::g_VRBind) != 4 {
        return clear_pose(&mut b, 3, "the VR arms are not bound");
    }
    if seq == 0 || now - b.snap_moved_ms > STALE_MS {
        return clear_pose(&mut b, 4, "the solved rig is not published (VRIK suspended)");
    }
    let Some(frame) = model_frame() else {
        return clear_pose(&mut b, 5, "no model-to-world frame");
    };
    let mut pose = Pose { size: core::mem::size_of::<Pose>() as u32, ..Pose::default() };
    let mut valid = 0u32;
    let mut view = core::ptr::read_volatile(&raw const g_VRViewWorldRot);
    if let Some(mut head) = bone_world(&frame, core::ptr::read_volatile(&raw const super::env::g_VRHeadBoneIdx)) {
        if normalized(&mut view) {
            head.rot = Quat { i: view[0], j: view[1], k: view[2], r: view[3] };
            pose.head = head;
            valid |= POSE_HEAD;
        }
    }
    let vehicle = core::ptr::read_volatile(&raw const super::env::g_isInVehicle);
    if !vehicle || core::ptr::read_volatile(&raw const CyberpunkVR_NightCoopVehicleHands) != 0 {
        if let Some(t) = bone_world(&frame, core::ptr::read_volatile(&raw const super::env::g_VRLeftBoneIdx)) {
            pose.left_hand = t;
            valid |= POSE_LEFT_HAND;
        }
        if let Some(t) = bone_world(&frame, core::ptr::read_volatile(&raw const super::env::g_VRRightBoneIdx)) {
            pose.right_hand = t;
            valid |= POSE_RIGHT_HAND;
        }
        let mut any = false;
        for side in 0..2 {
            any |= hand_curls(side, &mut pose.fingers[side as usize * 5..side as usize * 5 + 5]);
        }
        if any {
            valid |= POSE_FINGERS;
        }
    } else {
        pose.fingers = [-1.0; 10];
    }
    if let Some((origin, dir)) = muzzle_aim(&mut b, now) {
        pose.aim_origin = origin;
        pose.aim_dir = dir;
        valid |= POSE_AIM;
    }
    if !vehicle {
        let depth = core::ptr::read_volatile(&raw const CyberpunkVR_PhysicalCrouchDepth);
        pose.physical_crouch = if depth.is_finite() { (depth / FULL_SQUAT_METRES).clamp(0.0, 1.0) } else { 0.0 };
        valid |= POSE_CROUCH;
    }
    if valid & (POSE_HEAD | POSE_LEFT_HAND | POSE_RIGHT_HAND) == 0 {
        return clear_pose(&mut b, 6, "no head or hand in the snapshot");
    }
    pose.valid = valid | POSE_WORLD_SPACE;
    CyberpunkVR_NightCoopGate.store(0, Relaxed);
    let set = api_fn(b.api, API_POSE_SET);
    if set == 0 {
        return;
    }
    let result = core::mem::transmute::<usize, PoseSet>(set)(b.module, &pose);
    CyberpunkVR_NightCoopPoseResult.store(result, Relaxed);
    CyberpunkVR_NightCoopPoseBits.store(valid as i32, Relaxed);
    if result == OK {
        CyberpunkVR_NightCoopPoseSets.fetch_add(1, Relaxed);
        POSE_ON.store(true, Release);
    }
    // a changed result always; the valid bits' changes (every tracking flicker) only with the verbose diagnostics
    if result != b.logged_result || (cvr_host::log::verbose() && result == OK && valid != b.logged_bits) {
        log(&format!("[nightcoop] poseSet bits {valid:#x}{} -> {result}\n", if vehicle { " (in a vehicle)" } else { "" }));
        b.logged_result = result;
        b.logged_bits = valid;
    }
}
unsafe extern "C" fn on_event(_context: *mut c_void, event: i32, peer: u32) {
    let what = match event {
        1 => "session started",
        2 => "session ended",
        3 => "a player joined",
        4 => "a player left",
        5 => "a player is in our world",
        6 => "a player left our world",
        _ => return,
    };
    log(&format!("[nightcoop] {what} (peer {peer}); our pose source {}\n", if POSE_ON.load(Acquire) { "on" } else { "off" }));
}
/// Partners' channel messages, kept for the game thread's next look (take_messages); the oldest go past MAX_QUEUED.
static INBOX: Mutex<Vec<(u32, u16, Vec<u8>)>> = Mutex::new(Vec::new());
const MAX_QUEUED: usize = 256;
unsafe extern "C" fn on_message(_context: *mut c_void, from: u32, channel: u16, data: *const u8, size: usize) {
    if data.is_null() || size == 0 || size > 4096 {
        return;
    }
    let bytes = core::slice::from_raw_parts(data, size).to_vec();
    let mut q = INBOX.lock().unwrap_or_else(|p| p.into_inner());
    if q.len() >= MAX_QUEUED {
        q.remove(0);
    }
    q.push((from, channel, bytes));
}
/// The partners' messages that came since the last call: (peer, channel, bytes).
pub fn take_messages() -> Vec<(u32, u16, Vec<u8>)> {
    core::mem::take(&mut *INBOX.lock().unwrap_or_else(|p| p.into_inner()))
}
/// Our message on `channel` to every partner with this mod (NCSDK_SEND_* flags); NightCoop's answer, or None without a
/// registered bridge. Game thread.
pub fn send(channel: u16, data: &[u8], flags: u32) -> Option<i32> {
    let b = BRIDGE.try_lock().ok()?;
    if b.api == 0 || b.module == 0 {
        return None;
    }
    // SAFETY: the table's send, by the header's layout; the bytes live for the call.
    unsafe {
        let f = api_fn(b.api, API_SEND);
        (f != 0).then(|| core::mem::transmute::<usize, SendFn>(f)(b.module, TO_ALL, channel, data.as_ptr().cast(), data.len(), flags))
    }
}
/// Partners' sources: NightCoop lays them on their bodies itself. Taken here only so they do not queue for polling.
unsafe extern "C" fn on_partner_pose(_context: *mut c_void, _peer: u32, _pose: *const Pose) {}
unsafe extern "C" fn on_shutdown(_context: *mut c_void) {
    log("[nightcoop] NightCoop is unloading: the bridge stops\n");
    let mut b = BRIDGE.lock().unwrap_or_else(|p| p.into_inner());
    b.api = 0;
    b.module = 0;
    POSE_ON.store(false, Release);
    GONE.store(true, Release);
    CyberpunkVR_NightCoopLink.store(-2, Relaxed);
}

unsafe fn try_connect(b: &mut Bridge, now: u64) {
    b.next_lookup_ms = now + LOOKUP_EVERY_MS;
    let module = module();
    if module.is_null() {
        return; // not installed, or not loaded yet
    }
    let get = GetProcAddress(module, EXPORT_NAME.as_ptr());
    let api = match get {
        Some(f) => core::mem::transmute::<unsafe extern "system" fn() -> isize, GetFn>(f)(SDK_VERSION) as usize,
        None => 0,
    };
    if api == 0 || api_fn(api, API_REGISTER_MOD) == 0 || api_fn(api, API_POSE_SET) == 0 || api_fn(api, API_POSE_CLEAR) == 0 {
        if CyberpunkVR_NightCoopLink.load(Relaxed) != -1 {
            log(&format!(
                "[nightcoop] NightCoop.dll is loaded but gives no SDK table of version {SDK_VERSION} with poseSet (its SDK switched off?): the VR body stays local\n"
            ));
        }
        CyberpunkVR_NightCoopLink.store(-1, Relaxed);
        b.next_lookup_ms = now + REFUSED_RETRY_MS;
        return;
    }
    CyberpunkVR_NightCoopLink.store(1, Relaxed);
    let mut callbacks: [Hook; 17] = [core::ptr::null(); 17];
    callbacks[HOOK_FRAME] = on_frame as *const c_void;
    callbacks[HOOK_EVENT] = on_event as *const c_void;
    callbacks[HOOK_MESSAGE] = on_message as *const c_void;
    callbacks[HOOK_PARTNER_POSE] = on_partner_pose as *const c_void;
    callbacks[HOOK_SHUTDOWN] = on_shutdown as *const c_void;
    // the hooks stay where NightCoop was told they are for the rest of the run (it copies the info, not the hooks)
    let hooks: &'static Hooks = Box::leak(Box::new(Hooks { size: core::mem::size_of::<Hooks>() as u32, context: core::ptr::null_mut(), callbacks }));
    // Partners need nothing of ours to see the pose (NightCoop sends and lays it), so not NEEDED_BY_ALL.
    let info = ModInfo {
        size: core::mem::size_of::<ModInfo>() as u32,
        // the pose source, and the channel the props held in a hand go by (cvr-ffi mechanics/coop_props.rs)
        flags: MOD_USES_POSE | MOD_USES_CHANNELS,
        id: MOD_ID.as_ptr().cast(),
        name: MOD_NAME.as_ptr().cast(),
        version: MOD_VERSION.as_ptr().cast(),
        protocol: PROTOCOL,
        reserved: 0,
        hooks,
    };
    let module_id = core::mem::transmute::<usize, RegisterMod>(api_fn(api, API_REGISTER_MOD))(&info);
    if module_id == 0 {
        log(&format!(
            "[nightcoop] NightCoop refused the registration of cyberpunkvrport.vr (see NightCoop's log): retrying in {} s\n",
            REFUSED_RETRY_MS / 1000
        ));
        CyberpunkVR_NightCoopLink.store(-1, Relaxed);
        b.next_lookup_ms = now + REFUSED_RETRY_MS;
        return;
    }
    b.api = api;
    b.module = module_id;
    CyberpunkVR_NightCoopLink.store(2, Relaxed);
    log(&format!(
        "[nightcoop] registered as cyberpunkvrport.vr {} (SDK table {} bytes): the VR head, hands, fingers, aim and crouch go to the partners as NightCoop's pose source\n",
        MOD_VERSION.trim_end_matches('\0'),
        core::ptr::read_unaligned(api as *const u32)
    ));
    let log_fn = api_fn(api, API_LOG);
    if log_fn != 0 {
        core::mem::transmute::<usize, LogFn>(log_fn)(
            module_id,
            LOG_INFO,
            c"VR body: head, hands (solved rig), fingers, muzzle aim and physical crouch as the pose source".as_ptr(),
        );
    }
}

fn module() -> windows_sys::Win32::Foundation::HMODULE {
    let name: Vec<u16> = "NightCoop.dll\0".encode_utf16().collect();
    // SAFETY: a module lookup by a zero-terminated name.
    unsafe { GetModuleHandleW(name.as_ptr()) }
}
/// NightCoop is in the process (RED4ext loads it before the game's states run).
pub fn loaded() -> bool {
    !GONE.load(Acquire) && !module().is_null()
}
/// NightCoop's own folder (red4ext/plugins/NightCoop: its name.txt, logs, handoff), from its DLL's path.
pub fn folder() -> Option<std::path::PathBuf> {
    let module = module();
    if module.is_null() {
        return None;
    }
    let mut buf = [0u16; 1024];
    // SAFETY: the loaded module's path into our buffer.
    let n = unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleFileNameW(module, buf.as_mut_ptr(), buf.len() as u32) } as usize;
    if n == 0 || n >= buf.len() {
        return None;
    }
    let path = std::path::PathBuf::from(String::from_utf16_lossy(&buf[..n]));
    path.parent().map(|p| p.to_path_buf())
}
/// NightCoop's hookCoopUses for a script function: the mask of co-op areas that rely on it now (0 none), its error
/// code (NCSDK_E_*, negative), None without NightCoop or that SDK function. Game thread.
pub fn coop_uses(class: &core::ffi::CStr, function: &core::ffi::CStr) -> Option<i32> {
    let module = module();
    if module.is_null() || GONE.load(Acquire) {
        return None;
    }
    // SAFETY: NightCoop's exported table getter and the table's own function, by the header's layout.
    unsafe {
        let get = GetProcAddress(module, EXPORT_NAME.as_ptr())?;
        let api = core::mem::transmute::<unsafe extern "system" fn() -> isize, GetFn>(get)(SDK_VERSION) as usize;
        let uses = api_fn(api, API_HOOK_COOP_USES);
        (uses != 0).then(|| core::mem::transmute::<usize, HookCoopUses>(uses)(class.as_ptr(), function.as_ptr(), 0))
    }
}

/// The Running state's update: NightCoop looked up once a second until found (or refused: every 30 s).
#[no_mangle]
pub unsafe extern "C" fn cvr_nightcoop_update(_app: *mut c_void) -> bool {
    if GONE.load(Acquire) {
        return false;
    }
    let Ok(mut b) = BRIDGE.try_lock() else { return false };
    if b.api != 0 {
        return false;
    }
    let now = GetTickCount64();
    if now >= b.next_lookup_ms {
        try_connect(&mut b, now);
    }
    false
}
/// The plugin's unload: the source cleared and the mod unregistered.
#[no_mangle]
pub unsafe extern "C" fn cvr_nightcoop_shutdown() {
    let mut b = BRIDGE.lock().unwrap_or_else(|p| p.into_inner());
    if b.api == 0 || b.module == 0 {
        return;
    }
    let clear = api_fn(b.api, API_POSE_CLEAR);
    if POSE_ON.load(Acquire) && clear != 0 {
        core::mem::transmute::<usize, ModFn>(clear)(b.module);
    }
    let unregister = api_fn(b.api, API_UNREGISTER_MOD);
    if unregister != 0 {
        core::mem::transmute::<usize, ModFn>(unregister)(b.module);
    }
    POSE_ON.store(false, Release);
    b.api = 0;
    b.module = 0;
}

// nightcoop_sdk.h's sizes: NcSdkPose 160 bytes (the fingers at 92, the crouch at 156); NcSdkHooks a size word and 18
// pointers; NcSdkModInfo 48 bytes.
const _: () = assert!(core::mem::size_of::<Pose>() == 160);
const _: () = assert!(core::mem::offset_of!(Pose, fingers) == 92);
const _: () = assert!(core::mem::offset_of!(Pose, physical_crouch) == 156);
const _: () = assert!(core::mem::size_of::<Hooks>() == 8 + 18 * 8);
const _: () = assert!(core::mem::size_of::<ModInfo>() == 48);
