//! The tank (formerly src/Hooks/PanzerSteering.cpp and PanzerWeaponAim.cpp): the driven tank steers where the external
//! main camera looks, and its weapons aim along the rendered main view.
//!
//!   PanzerAimUpdate   the native TPP camera helper FixedUpdate_PreSolve compares the chassis forward with: its planar
//!                     target (model+E8/EC/F0) becomes the external camera's direction. The yaw controller, turning
//!                     speed, forces and camera orbit stay the game's.
//!   PanzerWeaponAim   the weapon target update: its two camera getters (VehicleCameraPosition/Rotation, from their
//!                     own call sites only) answer with the rendered main view's pose for the frame; the native
//!                     collision query and the salvo latch run unchanged.
//!   VehicleWeaponLaunch  the cannon (slot 0) from the targeted launch call: converges from the native muzzle onto the
//!                     vehicle's collision-tested target point. Missile arcs and countermeasures stay native.
//!
//! The player's tank is recognised by RTTI (vehicleTankBaseObject), which stays C++ (cvr_cpp_is_tank) until the
//! engine's type system is reached from Rust.
use super::camera_sites::{
    site_matches, PANZER_AIM_CALL, PANZER_AIM_UPDATE, PANZER_WEAPON_AIM, PANZER_WEAPON_POSITION_CALL, PANZER_WEAPON_ROTATION_CALL,
    VEHICLE_CAMERA_POSITION, VEHICLE_CAMERA_ROTATION, VEHICLE_WEAPON_LAUNCH, VEHICLE_WEAPON_TARGETED_LAUNCH_CALL,
};
use super::pose_transport::{cvr_pose_transport_read_external_direction, cvr_pose_transport_read_main_aim, MainAimPose};
use core::ffi::c_void;
use cvr_camera::external_view::aim_at_point;
use cvr_body::xr::Vec3;
use cvr_engine::code::{image_base, read, write};
use cvr_engine::hooks::{install_chain, Detour};
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicUsize, Ordering};
use std::sync::Mutex;

/// The vehicle's weapon slots (DynArray of 104-byte entries, the weapon at +0x10) and its aim target (2.5 offsets).
const WEAPON_SLOTS: usize = 0xB20;
const WEAPON_SLOT_COUNT: usize = 0xB2C;
const AIM_TARGET: usize = 0xB34;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct PanzerSteeringDebug {
    pub calls: u64,
    pub applied: u64,
    pub model: u64,
    pub owner: u64,
    pub stamp_us: u64,
    pub pose_id: u64,
    pub native_target: [f32; 3],
    pub target: [f32; 3],
    pub reason: u32,
    pub reserved: u32,
}
const _: () = assert!(core::mem::size_of::<PanzerSteeringDebug>() == 80);
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct PanzerWeaponAimDebug {
    pub calls: u64,
    pub applied: u64,
    pub position_writes: u64,
    pub rotation_writes: u64,
    pub owner: u64,
    pub pose_id: u64,
    pub stamp_us: u64,
    pub native_position: [f32; 3],
    pub native_rotation: [f32; 4],
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub target: [f32; 3],
    pub reason: u32,
}
const _: () = assert!(core::mem::size_of::<PanzerWeaponAimDebug>() == 128);
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct PanzerCannonDebug {
    pub calls: u64,
    pub applied: u64,
    pub owner: u64,
    pub current_main_pose_id: u64,
    pub stamp_us: u64,
    pub position: [f32; 3],
    pub native_forward: [f32; 3],
    pub forward: [f32; 3],
    pub target: [f32; 3],
    pub reason: u32,
}
const _: () = assert!(core::mem::size_of::<PanzerCannonDebug>() == 96);

#[allow(non_upper_case_globals)]
extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static CyberpunkVR_PanzerGazeSteering: i32;
    static mut CyberpunkVR_PanzerSteeringDebug: PanzerSteeringDebug;
    static CyberpunkVR_PanzerSteeringDebugSeq: AtomicU32;
    static mut CyberpunkVR_PanzerWeaponAimDebug: PanzerWeaponAimDebug;
    static CyberpunkVR_PanzerWeaponAimDebugSeq: AtomicU32;
    static mut CyberpunkVR_PanzerCannonDebug: PanzerCannonDebug;
    static CyberpunkVR_PanzerCannonDebugSeq: AtomicU32;
    static g_sceneTier: AtomicI32;
    fn cvr_cpp_is_driving() -> bool;
    fn cvr_cpp_menu_mode() -> i32;
    fn cvr_cpp_overlay_visible() -> bool;
    fn cvr_mounted_vehicle_identity() -> usize;
    fn cvr_cpp_is_tank(owner: usize) -> bool;
}

fn diagnostics() -> bool {
    // SAFETY: a plain exported int.
    unsafe { CyberpunkVR_RuntimeDiagnostics != 0 }
}
/// XrDiagNowUs: QPC microseconds (diagnostics stamps).
fn diag_now_us() -> u64 {
    let (now, freq) = super::env::qpc();
    if freq == 0 {
        return 0;
    }
    (now * 1_000_000 / freq) as u64
}
/// ReadFloatArraySafe: float by float, stopping at the first that cannot be read.
fn read_floats(at: usize, out: &mut [f32]) -> bool {
    for (i, v) in out.iter_mut().enumerate() {
        match read::<f32>(at + i * 4) {
            Some(x) => *v = x,
            None => return false,
        }
    }
    true
}
static DEBUG: Mutex<()> = Mutex::new(());
/// The seqlocked debug record: odd while it is written.
unsafe fn publish<T: Copy>(seq: &AtomicU32, slot: *mut T, value: T) {
    seq.fetch_add(1, Ordering::AcqRel);
    // SAFETY: the exported record, written under DEBUG.
    unsafe { *slot = value };
    seq.fetch_add(1, Ordering::Release);
}

// ---- the originals, the image, and the install state ----
static mut STEER_ORIG: *mut c_void = core::ptr::null_mut();
static mut UPDATE_ORIG: *mut c_void = core::ptr::null_mut();
static mut POSITION_ORIG: *mut c_void = core::ptr::null_mut();
static mut ROTATION_ORIG: *mut c_void = core::ptr::null_mut();
static mut LAUNCH_ORIG: *mut c_void = core::ptr::null_mut();
static EXE: AtomicUsize = AtomicUsize::new(0);
static HOOKS_READY: AtomicBool = AtomicBool::new(false);
/// The readers of the rendered main view (src/Hooks/CameraPoseTransport.cpp's, or the bench's when it binds them).
static MAIN_AIM: AtomicUsize = AtomicUsize::new(0);
static EXTERNAL_DIRECTION: AtomicUsize = AtomicUsize::new(0);
unsafe fn read_main_aim(out: &mut MainAimPose) -> bool {
    // SAFETY: the bound reader, or the transport's.
    unsafe {
        match MAIN_AIM.load(Ordering::Relaxed) {
            0 => cvr_pose_transport_read_main_aim(out),
            f => core::mem::transmute::<usize, unsafe extern "C" fn(*mut MainAimPose) -> bool>(f)(out),
        }
    }
}
unsafe fn read_external_direction(out: *mut f32, pose_id: *mut u64) -> bool {
    // SAFETY: as read_main_aim.
    unsafe {
        match EXTERNAL_DIRECTION.load(Ordering::Relaxed) {
            0 => cvr_pose_transport_read_external_direction(out, pose_id),
            f => core::mem::transmute::<usize, unsafe extern "C" fn(*mut f32, *mut u64) -> bool>(f)(out, pose_id),
        }
    }
}
type TargetFn = unsafe extern "C" fn(usize) -> usize;
type GetFn = unsafe extern "C" fn(usize, *mut f32) -> *mut f32;
type LaunchFn = unsafe extern "C" fn(usize, usize, *const f32, *const f32, *const f32, f32, u8) -> usize;

/// For tools/vrik_tests: the image the call sites are measured in, the five originals, the weapon-aim gate and the
/// two readers of the main view (0: the transport's).
#[no_mangle]
pub unsafe extern "C" fn cvr_panzer_bind(
    exe: usize,
    steer: usize,
    update: usize,
    position: usize,
    rotation: usize,
    launch: usize,
    ready: bool,
    main_aim: usize,
    external_direction: usize,
) {
    EXE.store(exe, Ordering::Relaxed);
    MAIN_AIM.store(main_aim, Ordering::Relaxed);
    EXTERNAL_DIRECTION.store(external_direction, Ordering::Relaxed);
    // SAFETY: the slots the detours read, set before any call.
    unsafe {
        STEER_ORIG = steer as *mut c_void;
        UPDATE_ORIG = update as *mut c_void;
        POSITION_ORIG = position as *mut c_void;
        ROTATION_ORIG = rotation as *mut c_void;
        LAUNCH_ORIG = launch as *mut c_void;
    }
    HOOKS_READY.store(ready, Ordering::Release);
}

// ---- the steering target ----
#[no_mangle]
pub unsafe extern "C" fn CvrPanzerSteerTarget(model: usize) -> usize {
    // SAFETY (whole body): the engine's original and its model through SEH reads and writes, the plugin's state.
    unsafe {
        let result = core::mem::transmute::<*mut c_void, TargetFn>(STEER_ORIG)(model);
        let mut frame = PanzerSteeringDebug { model: model as u64, reason: 1, ..Default::default() };
        'apply: {
            if CyberpunkVR_PanzerGazeSteering == 0
                || !cvr_cpp_is_driving()
                || cvr_cpp_menu_mode() != 0
                || g_sceneTier.load(Ordering::Relaxed) > 2
                || cvr_cpp_overlay_visible()
            {
                break 'apply;
            }
            frame.reason = 2;
            let owner = match read::<u64>(model + 0xE0) {
                Some(o) if o != 0 && o as usize == cvr_mounted_vehicle_identity() => o,
                _ => break 'apply,
            };
            frame.owner = owner;
            frame.reason = 3;
            if !read_external_direction(frame.target.as_mut_ptr(), &mut frame.pose_id) {
                break 'apply;
            }
            // CP2077 2.31: FixedUpdate_PreSolve calls this native TPP camera helper, then compares chassis forward
            // with model+E8/EC/F0. Replace its planar target only.
            if diagnostics() {
                read_floats(model + 0xE8, &mut frame.native_target);
            }
            frame.reason = 4;
            if !write(model + 0xE8, frame.target[0]) || !write(model + 0xEC, frame.target[1]) || !write(model + 0xF0, 0.0f32) {
                break 'apply;
            }
            frame.reason = 0;
        }
        if diagnostics() {
            frame.stamp_us = diag_now_us();
            let _guard = DEBUG.lock().unwrap_or_else(|p| p.into_inner());
            let mut next = frame;
            next.calls = CyberpunkVR_PanzerSteeringDebug.calls + 1;
            next.applied = CyberpunkVR_PanzerSteeringDebug.applied + (frame.reason == 0) as u64;
            publish(&CyberpunkVR_PanzerSteeringDebugSeq, &raw mut CyberpunkVR_PanzerSteeringDebug, next);
        }
        result
    }
}

// ---- the weapon aim ----
#[derive(Clone, Copy, Default)]
struct Context {
    pose: MainAimPose,
    debug: PanzerWeaponAimDebug,
    position_written: bool,
    rotation_written: bool,
}
thread_local! {
    /// cvr::ThreadScope<Context>: the frame the two getters answer with, while its update runs on this thread.
    static SCOPE: Cell<*mut Context> = const { Cell::new(core::ptr::null_mut()) };
}

unsafe fn is_player_tank(owner: usize) -> bool {
    // SAFETY: C++ services and the plugin's state.
    unsafe {
        HOOKS_READY.load(Ordering::Acquire)
            && owner != 0
            && cvr_cpp_is_driving()
            && cvr_cpp_menu_mode() == 0
            && g_sceneTier.load(Ordering::Relaxed) <= 2
            && !cvr_cpp_overlay_visible()
            && owner == cvr_mounted_vehicle_identity()
            && cvr_cpp_is_tank(owner)
    }
}

/// The cannon launch, `ret` the address it returns to.
#[no_mangle]
pub unsafe extern "C" fn CvrPanzerLaunchAt(
    owner: usize,
    weapon: usize,
    position: *const f32,
    forward: *const f32,
    target: *const f32,
    spread: f32,
    count: u8,
    ret: usize,
) -> usize {
    // SAFETY (whole body): the engine's original with its own arguments, the vehicle through SEH reads.
    unsafe {
        let original = core::mem::transmute::<*mut c_void, LaunchFn>(LAUNCH_ORIG);
        let native = || original(owner, weapon, position, forward, target, spread, count);
        if ret != EXE.load(Ordering::Relaxed) + VEHICLE_WEAPON_TARGETED_LAUNCH_CALL.rva + 5
            || !is_player_tank(owner)
            || position.is_null()
            || forward.is_null()
            || target.is_null()
        {
            return native();
        }
        let Some(slots) = read::<u64>(owner + WEAPON_SLOTS).filter(|&s| s != 0) else { return native() };
        let Some(_) = read::<u32>(owner + WEAPON_SLOT_COUNT).filter(|&n| (1..=8).contains(&n)) else { return native() };
        match read::<u64>(slots as usize + 0x10) {
            Some(cannon) if cannon as usize == weapon => {}
            _ => return native(),
        }
        // Converge from the native muzzle onto the collision-tested target point. Slot 0 is the cannon.
        let mut current = MainAimPose::default();
        let mut direction = Vec3 { x: 0.0, y: 0.0, z: 0.0 };
        let apply = read_main_aim(&mut current)
            && match aim_at_point(
                Vec3 { x: *position, y: *position.add(1), z: *position.add(2) },
                Vec3 { x: *target, y: *target.add(1), z: *target.add(2) },
            ) {
                Some(d) => {
                    direction = d;
                    true
                }
                None => false,
            };
        let redirected = [direction.x, direction.y, direction.z];
        let result = original(owner, weapon, position, if apply { redirected.as_ptr() } else { forward }, target, spread, count);
        if diagnostics() {
            let v3 = |p: *const f32| [*p, *p.add(1), *p.add(2)];
            let mut next = PanzerCannonDebug {
                owner: owner as u64,
                current_main_pose_id: current.pose_id,
                stamp_us: diag_now_us(),
                reason: if apply { 0 } else { 1 },
                position: v3(position),
                native_forward: v3(forward),
                forward: if apply { redirected } else { v3(forward) },
                target: v3(target),
                ..Default::default()
            };
            let _guard = DEBUG.lock().unwrap_or_else(|p| p.into_inner());
            next.calls = CyberpunkVR_PanzerCannonDebug.calls + 1;
            next.applied = CyberpunkVR_PanzerCannonDebug.applied + apply as u64;
            publish(&CyberpunkVR_PanzerCannonDebugSeq, &raw mut CyberpunkVR_PanzerCannonDebug, next);
        }
        result
    }
}
/// The camera position getter, `ret` the address it returns to.
#[no_mangle]
pub unsafe extern "C" fn CvrPanzerPositionAt(manager: usize, out: *mut f32, ret: usize) -> *mut f32 {
    // SAFETY: the engine's original, the update's frame on this thread.
    unsafe {
        let result = core::mem::transmute::<*mut c_void, GetFn>(POSITION_ORIG)(manager, out);
        let scope = SCOPE.with(Cell::get);
        if !scope.is_null() && result == out && !out.is_null() && ret == EXE.load(Ordering::Relaxed) + PANZER_WEAPON_POSITION_CALL.rva + 5 {
            let frame = &mut *scope;
            if diagnostics() {
                core::ptr::copy_nonoverlapping(out, frame.debug.native_position.as_mut_ptr(), 3);
            }
            core::ptr::copy_nonoverlapping(frame.pose.position.as_ptr(), out, 3);
            frame.position_written = true;
        }
        result
    }
}
/// The camera rotation getter, `ret` the address it returns to.
#[no_mangle]
pub unsafe extern "C" fn CvrPanzerRotationAt(manager: usize, out: *mut f32, ret: usize) -> *mut f32 {
    // SAFETY: as CvrPanzerPositionAt.
    unsafe {
        let result = core::mem::transmute::<*mut c_void, GetFn>(ROTATION_ORIG)(manager, out);
        let scope = SCOPE.with(Cell::get);
        if !scope.is_null() && result == out && !out.is_null() && ret == EXE.load(Ordering::Relaxed) + PANZER_WEAPON_ROTATION_CALL.rva + 5 {
            let frame = &mut *scope;
            if diagnostics() {
                core::ptr::copy_nonoverlapping(out, frame.debug.native_rotation.as_mut_ptr(), 4);
            }
            core::ptr::copy_nonoverlapping(frame.pose.rotation.as_ptr(), out, 4);
            frame.rotation_written = true;
        }
        result
    }
}
unsafe fn record(frame: &Context, owner: usize) {
    let mut next = frame.debug;
    next.owner = owner as u64;
    next.pose_id = frame.pose.pose_id;
    next.stamp_us = diag_now_us();
    next.position = frame.pose.position;
    next.rotation = frame.pose.rotation;
    if frame.position_written && frame.rotation_written {
        next.reason = 0;
        read_floats(owner + AIM_TARGET, &mut next.target);
    }
    let _guard = DEBUG.lock().unwrap_or_else(|p| p.into_inner());
    // SAFETY: the exported record, under DEBUG.
    unsafe {
        let d = CyberpunkVR_PanzerWeaponAimDebug;
        next.calls = d.calls + 1;
        next.applied = d.applied + (next.reason == 0) as u64;
        next.position_writes = d.position_writes + frame.position_written as u64;
        next.rotation_writes = d.rotation_writes + frame.rotation_written as u64;
        publish(&CyberpunkVR_PanzerWeaponAimDebugSeq, &raw mut CyberpunkVR_PanzerWeaponAimDebug, next);
    }
}
/// The weapon target update: the native collision query and salvo latch run with the two getters answering the
/// rendered main view's pose; no shared camera is modified.
#[no_mangle]
pub unsafe extern "C" fn CvrPanzerAimUpdate(owner: usize) -> usize {
    let mut frame = Context::default();
    frame.debug.reason = 1;
    let mut apply = false;
    // SAFETY (whole body): the engine's original, the plugin's state.
    unsafe {
        if is_player_tank(owner) {
            frame.debug.reason = 3;
            if read_main_aim(&mut frame.pose) {
                frame.debug.reason = 4;
                apply = true;
            }
        }
        let previous = SCOPE.with(|s| s.replace(if apply { &mut frame } else { core::ptr::null_mut() }));
        let result = core::mem::transmute::<*mut c_void, TargetFn>(UPDATE_ORIG)(owner);
        SCOPE.with(|s| s.set(previous));
        if diagnostics() {
            record(&frame, owner);
        }
        result
    }
}

// ---- the entries: the return address as one more argument ----
#[unsafe(naked)]
unsafe extern "C" fn position_entry() {
    core::arch::naked_asm!("mov r8, [rsp]", "jmp {body}", body = sym CvrPanzerPositionAt);
}
#[unsafe(naked)]
unsafe extern "C" fn rotation_entry() {
    core::arch::naked_asm!("mov r8, [rsp]", "jmp {body}", body = sym CvrPanzerRotationAt);
}
// The launch has seven arguments, so the return address goes eighth on a frame of its own: the entry copies the
// three stack arguments down, adds the return address after them and calls the body. The frame has unwind data
// (.seh_*), so a stack walk through the engine's original sees through it.
core::arch::global_asm!(
    ".globl cvr_panzer_launch_entry",
    ".def cvr_panzer_launch_entry",
    ".scl 2",
    ".type 32",
    ".endef",
    ".p2align 4",
    "cvr_panzer_launch_entry:",
    ".seh_proc cvr_panzer_launch_entry",
    "sub rsp, 0x48",
    ".seh_stackalloc 0x48",
    ".seh_endprologue",
    "mov rax, qword ptr [rsp + 0x48]",
    "mov r10, qword ptr [rsp + 0x70]",
    "mov qword ptr [rsp + 0x20], r10",
    "mov r10, qword ptr [rsp + 0x78]",
    "mov qword ptr [rsp + 0x28], r10",
    "mov r10, qword ptr [rsp + 0x80]",
    "mov qword ptr [rsp + 0x30], r10",
    "mov qword ptr [rsp + 0x38], rax",
    "call {body}",
    "add rsp, 0x48",
    "ret",
    ".seh_endproc",
    body = sym CvrPanzerLaunchAt,
);
extern "C" {
    fn cvr_panzer_launch_entry();
}

// ---- installation ----
/// PanzerSteering (Boot, 83): the steering target, while both its sites are the ones the port was written against.
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_panzer_steering_install() -> bool {
    let exe = image_base();
    if exe == 0 || !site_matches(exe, &PANZER_AIM_UPDATE) || !site_matches(exe, &PANZER_AIM_CALL) {
        return false;
    }
    EXE.store(exe, Ordering::Relaxed);
    let detour = Detour {
        name: "PanzerAimUpdate",
        target: PANZER_AIM_UPDATE.rva,
        absolute: false,
        detour: CvrPanzerSteerTarget as *const c_void,
        original: &raw mut STEER_ORIG,
        expect: None,
    };
    // SAFETY: the detour has the target's signature and reads its trampoline from STEER_ORIG.
    unsafe { install_chain("PanzerSteering", &[detour]) }.is_ok()
}
/// PanzerWeaponAim (Boot, 84): the update, the two getters and the launch, together.
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_panzer_weapon_aim_install() -> bool {
    let exe = image_base();
    if exe == 0 {
        return false;
    }
    let verified = [
        &PANZER_WEAPON_AIM,
        &VEHICLE_CAMERA_POSITION,
        &VEHICLE_CAMERA_ROTATION,
        &PANZER_WEAPON_POSITION_CALL,
        &PANZER_WEAPON_ROTATION_CALL,
        &VEHICLE_WEAPON_LAUNCH,
        &VEHICLE_WEAPON_TARGETED_LAUNCH_CALL,
    ];
    if let Some(site) = verified.iter().find(|s| !site_matches(exe, s)) {
        super::env::log(&format!("[hooks] PanzerWeaponAim: the code at {:#x} is not what the port was written against
", site.rva));
        return false;
    }
    EXE.store(exe, Ordering::Relaxed);
    let d = |name, target: usize, detour: *const c_void, original: *mut *mut c_void| Detour { name, target, absolute: false, detour, original, expect: None };
    let detours = [
        d("VehicleCameraPosition", VEHICLE_CAMERA_POSITION.rva, position_entry as *const c_void, &raw mut POSITION_ORIG),
        d("VehicleCameraRotation", VEHICLE_CAMERA_ROTATION.rva, rotation_entry as *const c_void, &raw mut ROTATION_ORIG),
        d("PanzerWeaponAim", PANZER_WEAPON_AIM.rva, CvrPanzerAimUpdate as *const c_void, &raw mut UPDATE_ORIG),
        d("VehicleWeaponLaunch", VEHICLE_WEAPON_LAUNCH.rva, cvr_panzer_launch_entry as *const c_void, &raw mut LAUNCH_ORIG),
    ];
    // SAFETY: each detour has its target's signature (the entries pass the return address on and jump).
    let ok = unsafe { install_chain("PanzerWeaponAim", &detours) }.is_ok();
    HOOKS_READY.store(ok, Ordering::Release);
    ok
}
