//! CameraPoseTransport (formerly src/Hooks/CameraPoseTransport.cpp): the pose identity carried across the engine's
//! own camera copies -- the director's blend and its finalize, the component serializers, the setup copy and default,
//! MAIN's read of its selected setup, the RTT camera build and refresh, the descriptor builder, the camera clone and the
//! render-view build -- so the label published at the locate site reaches the render node. And the EXTERNAL view: a
//! detached camera the director blends in as MAIN gets the head composed onto it (GenericSerialize), and the second
//! eye's RTT camera is built from that completed MAIN, the eye offset across (DescriptorBuild), with a lease that hands
//! the completed MAIN over (PublishExternalMain / ReadExternalMain) and the aim and steering readers.
//!
//! Three detours ask where they were called from (BlendFinalize, DescriptorBuild, RenderBuild): a naked entry passes
//! the return address on to the body as one more argument, and jumps, so the body returns straight to the engine. The
//! setup copy and default go through src/Hooks/CameraPoseLeafThunks.asm (their callers rely on a narrower clobber set
//! than the Win64 ABI's), which calls CvrPoseSetupCopyBefore / After and CvrPoseSetupDefaultBefore here.
//!
//! The second eye's component is held by a RED4ext weak reference, which stays C++ (src/Hooks/HookServices.cpp
//! cvr_cpp_external_vrcam_observe / notify) until the engine's handles are in Rust.
use super::camera_link::HeadPose;
use super::camera_math::{cvr_camera_desired_half_ipd, cvr_camera_world_scale};
use super::camera_sites::{
    site_matches, Site, BLEND, BLEND_FINALIZE, BLEND_FINALIZE_CALL, CAMERA_CLONE, DESCRIPTOR_BUILD, GENERIC_SERIALIZE, MAIN_READ, RENDER_BUILD,
    RENDER_CALL, RTT_BUILD, RTT_DESCRIPTOR_CALL, RTT_REFRESH, SERIALIZE, SETUP_COPY, SETUP_DEFAULT,
};
use super::pose_identity::{
    cvr_pose_capture, cvr_pose_invalidate, cvr_pose_next_identity, cvr_pose_publish_blended, cvr_pose_publish_serialized, cvr_pose_publish_setup,
    cvr_pose_transfer, PoseReceipt, WeightedPoseReceipt,
};
use super::roomscale_move::now_us;
use core::cell::Cell;
use core::ffi::c_void;
use cvr_body::xr::{Quat, Vec3};
use cvr_camera::external_view::{compose_external, external_eye, normalize_external, planar_external_forward, ExternalViewKey, ExternalViewLease, ExternalViewPose};
use cvr_camera::pose_ledger::Fingerprint;
use cvr_engine::code::{image_base, read, read_bytes, write};
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;

/// The last blend finalize, for the diagnostics overlay (CyberpunkVR_PoseBlendDebug).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct PoseBlendDebug {
    pub calls: u64,
    pub labelled: u64,
    pub rejected: u64,
    pub director: usize,
    pub count: u32,
    pub accepted: u32,
    pub weights: [f32; 8],
    pub pose_ids: [u64; 8],
    pub components: [usize; 8],
    pub inputs: [Fingerprint; 8],
    pub output: Fingerprint,
}
const _: () = assert!(core::mem::size_of::<PoseBlendDebug>() == 456);
/// The last external camera written per eye (CyberpunkVR_ExternalCameraDebug).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ExternalCameraDebug {
    pub writes: u64,
    pub stamp_us: u64,
    pub pose_id: u64,
    pub origin: u64,
    pub component: u64,
    pub position: [i32; 3],
    pub rotation: [f32; 4],
    pub head: [f32; 7],
    pub fov: f32,
}
const _: () = assert!(core::mem::size_of::<ExternalCameraDebug>() == 104);
/// cvr::camera::MainAimPose.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MainAimPose {
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub pose_id: u64,
}

extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static mut CyberpunkVR_PoseBlendDebug: PoseBlendDebug;
    static CyberpunkVR_PoseBlendDebugSeq: AtomicU32;
    static CyberpunkVR_PoseMainReadMissing: [AtomicU64; 2];
    static mut CyberpunkVR_ExternalCameraDebug: [ExternalCameraDebug; 2];
    static CyberpunkVR_ExternalCameraDebugSeq: [AtomicU32; 2];
    static CyberpunkVR_ExternalSelectionStates: [AtomicU64; 4];
    static CyberpunkVR_ExternalNotify: [AtomicU64; 3];
    static CyberpunkVR_IpdInWorldPos: i32;
    static CyberpunkVR_MainIsRightEye: i32;
    static CyberpunkVR_BdEditorAlign: i32;
    fn cvr_cpp_menu_mode() -> i32;
    fn cvr_cpp_lobby_world_shown() -> bool;
    fn cvr_cpp_device_cam_active() -> bool;
    fn cvr_cpp_braindance_active() -> bool;
    fn cvr_cpp_bd_scene_pose_valid() -> bool;
    fn cvr_cpp_cam_obj_main() -> usize;
    fn cvr_cpp_vrcam_comp() -> usize;
    fn cvr_cpp_xr_tracking_origin_serial() -> u64;
    fn cvr_cpp_xr_external_pose_reset_pending() -> bool;
    fn cvr_xr_acquire_camera_pose_frame(head: *mut HeadPose, consumed: *mut super::camera_link::Vec2, sequence: *mut u64) -> bool;
    fn cvr_cpp_refresh_player_camera_state();
    fn cvr_cpp_normal_fov_override() -> f32;
    fn cvr_cpp_external_vrcam_observe(component: usize);
    fn cvr_cpp_external_vrcam_notify(eye_camera: usize, notify: unsafe extern "C" fn(usize)) -> i32;
    fn cvr_cpp_ensure_camera_pose_copy_hook() -> bool;
    fn CvrPoseSetupCopyDetour();
    fn CvrPoseSetupDefaultDetour();
}

fn diagnostics() -> bool {
    // SAFETY: a plain exported int.
    unsafe { CyberpunkVR_RuntimeDiagnostics != 0 }
}
fn count(counter: &AtomicU64) {
    if diagnostics() {
        counter.fetch_add(1, Ordering::Relaxed);
    }
}
fn capture(address: usize) -> PoseReceipt {
    let mut out = PoseReceipt::default();
    // SAFETY: writes the local.
    unsafe { cvr_pose_capture(address, &mut out) };
    out
}

// ---- the originals (and the image), set at install or by the tests' bind ----
static EXE: AtomicUsize = AtomicUsize::new(0);
static ORIGINALS: [AtomicUsize; 10] = [const { AtomicUsize::new(0) }; 10];
const O_BLEND: usize = 0;
const O_BLEND_FINALIZE: usize = 1;
const O_SERIALIZE: usize = 2;
const O_GENERIC_SERIALIZE: usize = 3;
const O_MAIN_READ: usize = 4;
const O_RTT: usize = 5;
const O_RTT_REFRESH: usize = 6;
const O_DESCRIPTOR: usize = 7;
const O_CAMERA_CLONE: usize = 8;
const O_RENDER: usize = 9;
/// The asm thunks jump through these (CameraPoseLeafThunks.asm).
#[no_mangle]
pub static mut CvrPoseSetupCopyOriginal: *mut c_void = core::ptr::null_mut();
#[no_mangle]
pub static mut CvrPoseSetupDefaultOriginal: *mut c_void = core::ptr::null_mut();
fn original(k: usize) -> usize {
    ORIGINALS[k].load(Ordering::Acquire)
}

/// The tests' stand-ins for the engine: the image base and the ten originals, in the detours' order.
#[no_mangle]
pub unsafe extern "C" fn CvrPoseTransportBind(exe: usize, originals: *const usize) {
    EXE.store(exe, Ordering::Release);
    for (k, o) in ORIGINALS.iter().enumerate() {
        // SAFETY: the caller's ten entries.
        o.store(if originals.is_null() { 0 } else { unsafe { *originals.add(k) } }, Ordering::Release);
    }
}

// ---- thread state ----
#[derive(Clone, Copy, Default)]
struct ExternalHead {
    head: HeadPose,
    id: u64,
}
#[derive(Clone, Copy, Default)]
struct ExternalSelection {
    main: ExternalViewPose,
    sample: ExternalHead,
    fov: f32,
    external_view: bool,
}
thread_local! {
    static T_SOURCE: Cell<PoseReceipt> = Cell::new(PoseReceipt::default());
    static T_BLEND_DIRECTOR: Cell<usize> = const { Cell::new(0) };
    static T_RTT_COMPONENT: Cell<usize> = const { Cell::new(0) };
    static T_SERIALIZING_COMPONENT: Cell<usize> = const { Cell::new(0) };
    static T_EXTERNAL_HEAD: Cell<ExternalHead> = Cell::new(ExternalHead::default());
    static T_EXTERNAL_RTT: Cell<*const ExternalSelection> = const { Cell::new(core::ptr::null()) };
    static T_PUSHING: Cell<bool> = const { Cell::new(false) };
}
struct Leases {
    main: ExternalViewLease<ExternalSelection>,
    aim: ExternalViewLease<ExternalSelection>,
}
static LEASES: Mutex<Leases> = Mutex::new(Leases {
    main: ExternalViewLease { value: SELECTION_ZERO, key: KEY_ZERO, stamp: 0 },
    aim: ExternalViewLease { value: SELECTION_ZERO, key: KEY_ZERO, stamp: 0 },
});
const KEY_ZERO: ExternalViewKey = ExternalViewKey { player_camera: 0, eye_camera: 0, origin: 0 };
const SELECTION_ZERO: ExternalSelection = ExternalSelection {
    main: ExternalViewPose { position: [0; 3], rotation: Quat::IDENTITY },
    sample: ExternalHead { head: HEAD_ZERO, id: 0 },
    fov: 0.0,
    external_view: false,
};
const HEAD_ZERO: HeadPose = HeadPose {
    pos: [0.0; 3],
    ori: [0.0; 4],
    valid: false,
    origin_serial: 0,
    body_bend: [0.0; 3],
    crouch_depth: 0.0,
    physical_crouch: false,
    afw_eye: -1,
};
static BLEND_DEBUG: Mutex<()> = Mutex::new(());
static EXTERNAL_DEBUG: [Mutex<()>; 2] = [Mutex::new(()), Mutex::new(())];
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

// ---- the external view ----
/// The existing device-lens and BD playback paths already own those views. The main menu shown in its world is
/// head-tracked as a scene is.
fn external_allowed() -> bool {
    // SAFETY: C++ services.
    unsafe {
        (cvr_cpp_menu_mode() == 0 || cvr_cpp_lobby_world_shown())
            && super::xr_calls::session_running()
            && !cvr_cpp_device_cam_active()
            && !(cvr_cpp_braindance_active() && cvr_cpp_bd_scene_pose_valid())
    }
}
fn external_head_read(sample: &mut ExternalHead, includes_fpp: bool) -> bool {
    if includes_fpp {
        // SAFETY: a C++ service.
        let component = unsafe { cvr_cpp_cam_obj_main() };
        let source = if component != 0 { capture(component + 0xE0) } else { PoseReceipt::default() };
        // SAFETY: as above.
        if !source.valid() || source.payload.view != 1 || !source.payload.head.valid || source.payload.head.origin_serial != unsafe { cvr_cpp_xr_tracking_origin_serial() } {
            return false;
        }
        *sample = ExternalHead { head: source.payload.head, id: source.payload.pose_id };
        return true;
    }
    // A dormant FPP component may carry an old, still valid label. Use it only when the director is actually blending
    // that component into this view.
    let (mut sequence, mut consumed) = (0u64, super::camera_link::Vec2::default());
    // SAFETY: a C++ service writing locals.
    if !unsafe { cvr_xr_acquire_camera_pose_frame(&mut sample.head, &mut consumed, &mut sequence) } || !sample.head.valid {
        return false;
    }
    sample.id = if sequence != 0 { sequence } else { cvr_pose_next_identity() };
    true
}
/// Called inside the native serializer loop, which already holds its lock.
fn selection_includes_fpp(director: usize) -> bool {
    let (Some(table), Some(count)) = (read::<u64>(director + 0x48), read::<u8>(director + 0x54)) else { return false };
    if table == 0 || count > 64 {
        return false;
    }
    // SAFETY: a C++ service.
    let main = unsafe { cvr_cpp_cam_obj_main() } as u64;
    (0..count as u64).any(|i| {
        read::<u64>((table + i * 0x20) as usize) == Some(main) && read::<f32>((table + i * 0x20 + 0x10) as usize).is_some_and(|w| w > 0.0)
    })
}
fn read_external_pose(address: usize) -> Option<ExternalViewPose> {
    let mut pose = ExternalViewPose::default();
    for i in 0..3 {
        pose.position[i] = read::<u32>(address + i * 4)? as i32;
    }
    let r = read::<[f32; 4]>(address + 16)?;
    pose.rotation = Quat { x: r[0], y: r[1], z: r[2], w: r[3] };
    normalize_external(&mut pose.rotation).then_some(pose)
}
fn write_external_pose(address: usize, pose: &ExternalViewPose) -> bool {
    for i in 0..3 {
        if !write(address + i * 4, pose.position[i] as u32) {
            return false;
        }
    }
    let r = [pose.rotation.x, pose.rotation.y, pose.rotation.z, pose.rotation.w];
    (0..4).all(|i| write(address + 16 + i * 4, r[i]))
}
fn record_external(eye: usize, address: usize, component: usize, sample: &ExternalHead) {
    let Some(pose) = read_external_pose(address) else { return };
    let Some(fov) = read::<f32>(address + 0x20) else { return };
    let _guard = lock(&EXTERNAL_DEBUG[eye]);
    // SAFETY: the exported debug record, written under its lock between two sequence steps.
    unsafe {
        CyberpunkVR_ExternalCameraDebugSeq[eye].fetch_add(1, Ordering::AcqRel);
        let out = &mut (*(&raw mut CyberpunkVR_ExternalCameraDebug))[eye];
        out.writes += 1;
        out.stamp_us = now_us();
        out.pose_id = sample.id;
        out.origin = sample.head.origin_serial;
        out.component = component as u64;
        out.position = pose.position;
        out.rotation = [pose.rotation.x, pose.rotation.y, pose.rotation.z, pose.rotation.w];
        let h = &sample.head;
        out.head = [h.pos[0], h.pos[1], h.pos[2], h.ori[0], h.ori[1], h.ori[2], h.ori[3]];
        out.fov = fov;
        CyberpunkVR_ExternalCameraDebugSeq[eye].fetch_add(1, Ordering::Release);
    }
}
fn main_eye_sign() -> f32 {
    // SAFETY: a plain exported int.
    if unsafe { CyberpunkVR_MainIsRightEye } != 0 { 1.0 } else { -1.0 }
}
fn ipd_in_world() -> bool {
    // SAFETY: a plain exported int.
    unsafe { CyberpunkVR_IpdInWorldPos != 0 }
}
fn compose_external_main(base: &ExternalViewPose, head: &ExternalHead) -> Option<ExternalViewPose> {
    let h = &head.head;
    let eye = if ipd_in_world() { cvr_camera_desired_half_ipd() * main_eye_sign() } else { 0.0 };
    compose_external(
        base,
        Quat { x: h.ori[0], y: h.ori[1], z: h.ori[2], w: h.ori[3] },
        Vec3 { x: h.pos[0], y: h.pos[1], z: h.pos[2] },
        cvr_camera_world_scale(),
        eye,
    )
}
fn external_key() -> ExternalViewKey {
    // SAFETY: C++ services.
    unsafe { ExternalViewKey { player_camera: cvr_cpp_cam_obj_main(), eye_camera: cvr_cpp_vrcam_comp(), origin: cvr_cpp_xr_tracking_origin_serial() } }
}
/// This existing scene/editor route already positions VRCAM from MAIN. Carry the completed MAIN receipt with that
/// transform as well, instead of losing it at the legacy component rewrite (and disabling framegen).
fn scene_main_handoff() -> bool {
    // SAFETY: an exported int and C++ services.
    unsafe { CyberpunkVR_BdEditorAlign != 0 && cvr_cpp_braindance_active() && !cvr_cpp_bd_scene_pose_valid() }
}
/// The component's own notification (vtable +0x240), with its transform at +0x100; not re-entered.
unsafe extern "C" fn notify_external_vrcam(component: usize) {
    if T_PUSHING.with(|p| p.get()) || component == 0 {
        return;
    }
    let Some(table) = read::<u64>(component).filter(|&t| t != 0) else { return };
    let Some(notify) = read::<u64>(table as usize + 0x240).filter(|&n| n != 0) else { return };
    T_PUSHING.with(|p| p.set(true));
    // SAFETY: the engine's notification, with the component the C++ adapter holds alive through the call.
    unsafe { core::mem::transmute::<usize, unsafe extern "C" fn(usize, usize)>(notify as usize)(component, component + 0x100) };
    T_PUSHING.with(|p| p.set(false));
}
fn publish_external_main(address: usize, receipt: &PoseReceipt) {
    if !receipt.valid() {
        return; // missing observation does not mean an FPP transition
    }
    let key = external_key();
    if !external_allowed() {
        let mut l = lock(&LEASES);
        l.main.clear();
        l.aim.clear();
        return;
    }
    if receipt.payload.head.origin_serial != key.origin {
        return;
    }
    let Some(main) = read_external_pose(address) else { return };
    let Some(fov) = read::<f32>(address + 0x20) else { return };
    let frame = ExternalSelection {
        main,
        sample: ExternalHead { head: receipt.payload.head, id: receipt.payload.pose_id },
        fov,
        external_view: receipt.payload.external_view,
    };
    let mirror = frame.external_view || scene_main_handoff();
    {
        let mut l = lock(&LEASES);
        let now = now_us();
        l.aim.publish(frame, key, now);
        if mirror {
            l.main.publish(frame, key, now);
        } else {
            l.main.clear();
        }
    }
    if !mirror {
        return;
    }
    // Refresh from this completed MAIN, without one-frame lag. The address is only an identity comparison: locking
    // the engine weak reference is what proves lifetime, including if the old component has already been freed.
    // SAFETY: the C++ adapter holds the component through the notification.
    // SAFETY: exported counters.
    match unsafe { cvr_cpp_external_vrcam_notify(key.eye_camera, notify_external_vrcam) } {
        0 => count(unsafe { &CyberpunkVR_ExternalNotify[0] }),
        1 => count(unsafe { &CyberpunkVR_ExternalNotify[1] }),
        _ => count(unsafe { &CyberpunkVR_ExternalNotify[2] }),
    }
}
fn read_external_main() -> Option<ExternalSelection> {
    let key = external_key();
    let mut l = lock(&LEASES);
    // SAFETY: C++ service; exported counters.
    unsafe {
        if !external_allowed() || cvr_cpp_xr_external_pose_reset_pending() {
            l.main.clear();
            count(&CyberpunkVR_ExternalSelectionStates[2]);
            return None;
        }
        let now = now_us();
        if let Some(output) = l.main.read(key, now) {
            if !output.external_view && !scene_main_handoff() {
                l.main.clear();
                return None;
            }
            count(&CyberpunkVR_ExternalSelectionStates[if now - l.main.stamp > 50000 { 1 } else { 0 }]);
            return Some(output);
        }
        count(&CyberpunkVR_ExternalSelectionStates[3]);
    }
    None
}

// ---- the detours ----
unsafe fn call1(k: usize, a: usize) -> usize {
    // SAFETY: the engine function (or the tests' stand-in) at the original pointer.
    unsafe { core::mem::transmute::<usize, unsafe extern "C" fn(usize) -> usize>(original(k))(a) }
}
unsafe fn call2(k: usize, a: usize, b: usize) -> usize {
    // SAFETY: as above.
    unsafe { core::mem::transmute::<usize, unsafe extern "C" fn(usize, usize) -> usize>(original(k))(a, b) }
}
unsafe fn call3(k: usize, a: usize, b: usize, c: usize) -> usize {
    // SAFETY: as above.
    unsafe { core::mem::transmute::<usize, unsafe extern "C" fn(usize, usize, usize) -> usize>(original(k))(a, b, c) }
}

#[no_mangle]
pub unsafe extern "C" fn CvrPtBlend(director: usize) -> usize {
    let previous = T_BLEND_DIRECTOR.with(|t| t.get());
    let previous_head = T_EXTERNAL_HEAD.with(|t| t.replace(ExternalHead::default()));
    T_BLEND_DIRECTOR.with(|t| t.set(director));
    // SAFETY: the engine's blend.
    let result = unsafe { call1(O_BLEND, director) };
    T_BLEND_DIRECTOR.with(|t| t.set(previous));
    T_EXTERNAL_HEAD.with(|t| t.set(previous_head));
    result
}
/// The native blend is finished, but its serialized inputs still exist. This call site is under our Blend scope;
/// unrelated array frees never acquire a camera label. The actual entries are read, not a table sampled before the
/// director took its own lock.
#[no_mangle]
pub unsafe extern "C" fn CvrPtBlendFinalizeAt(array: usize, return_address: usize) {
    let director = T_BLEND_DIRECTOR.with(|t| t.get());
    if director != 0 && return_address == EXE.load(Ordering::Acquire) + 0x5FBE2B {
        let diagnostic_on = diagnostics();
        let mut diagnostic = PoseBlendDebug { director, ..Default::default() };
        let output = director + 0x500; // 2.31: +0x4C0
        let mut accepted = false;
        // ReadU64Safe(array) && ReadU32Safe(array+12): the count is read only after the entries
        let entries = read::<u64>(array);
        let count = if entries.is_some() { read::<u32>(array + 12) } else { None };
        let n;
        match (entries, count) {
            (Some(entries), Some(c)) if c > 0 && c <= 64 => {
                n = c;
                let mut inputs = Vec::with_capacity(c as usize);
                let mut readable = true;
                for i in 0..c as usize {
                    let entry = entries as usize + i * 0xA0;
                    let Some(weight) = read::<f32>(entry + 0x90) else {
                        readable = false;
                        break;
                    };
                    let receipt = capture(entry);
                    inputs.push(WeightedPoseReceipt { source: receipt, weight });
                    if diagnostic_on && i < 8 {
                        diagnostic.weights[i] = weight;
                        diagnostic.pose_ids[i] = receipt.payload.pose_id;
                        diagnostic.components[i] = receipt.payload.component;
                        if receipt.valid() {
                            diagnostic.inputs[i] = receipt.fingerprint;
                        }
                    }
                }
                if readable {
                    // SAFETY: the local inputs.
                    accepted = unsafe { cvr_pose_publish_blended(output, inputs.as_ptr(), inputs.len()) };
                } else {
                    cvr_pose_invalidate(output);
                }
            }
            (_, c) => {
                n = c.unwrap_or(0);
                cvr_pose_invalidate(output);
            }
        }
        if accepted {
            publish_external_main(output, &capture(output));
        }
        if diagnostic_on {
            diagnostic.count = n;
            diagnostic.accepted = accepted as u32;
            let receipt = capture(output);
            if receipt.valid() {
                diagnostic.output = receipt.fingerprint;
            }
            let _guard = lock(&BLEND_DEBUG);
            // SAFETY: the exported debug record, written under its lock between two sequence steps.
            unsafe {
                let last = &raw mut CyberpunkVR_PoseBlendDebug;
                diagnostic.calls = (*last).calls + 1;
                diagnostic.labelled = (*last).labelled + accepted as u64;
                diagnostic.rejected = (*last).rejected + (!accepted) as u64;
                CyberpunkVR_PoseBlendDebugSeq.fetch_add(1, Ordering::AcqRel);
                *last = diagnostic;
                CyberpunkVR_PoseBlendDebugSeq.fetch_add(1, Ordering::Release);
            }
        }
    }
    // SAFETY: the engine's finalize.
    unsafe { call1(O_BLEND_FINALIZE, array) };
}
#[no_mangle]
pub unsafe extern "C" fn CvrPtSerialize(interface: usize, destination: usize) {
    let component = interface.wrapping_sub(0x120);
    let source = capture(component.wrapping_add(0xE0));
    cvr_pose_invalidate(destination);
    let previous = T_SERIALIZING_COMPONENT.with(|t| t.replace(component));
    // SAFETY: the engine's serializer (it includes the LocateCamera hook).
    unsafe { call2(O_SERIALIZE, interface, destination) };
    T_SERIALIZING_COMPONENT.with(|t| t.set(previous));
    // Locate owns takeover transforms and publishes their actual written pose. The destination was invalidated before
    // this call, so only a new, still matching publication can survive here. Ordinary serialization inherits the
    // component receipt as before.
    if !capture(destination).valid() {
        // SAFETY: the local receipt.
        unsafe { cvr_pose_publish_serialized(destination, &source) };
    }
}
#[no_mangle]
pub unsafe extern "C" fn CvrPtGenericSerialize(interface: usize, destination: usize) {
    cvr_pose_invalidate(destination);
    // SAFETY: the engine's serializer.
    unsafe { call2(O_GENERIC_SERIALIZE, interface, destination) };
    let director = T_BLEND_DIRECTOR.with(|t| t.get());
    if director == 0 || !external_allowed() {
        return;
    }
    // SAFETY: a C++ service.
    unsafe { cvr_cpp_refresh_player_camera_state() };
    let mut head = T_EXTERNAL_HEAD.with(|t| t.get());
    if head.id == 0 {
        let ok = external_head_read(&mut head, selection_includes_fpp(director));
        T_EXTERNAL_HEAD.with(|t| t.set(head));
        if !ok {
            return;
        }
    }
    let Some(base) = read_external_pose(destination) else { return };
    let Some(composed) = compose_external_main(&base, &head) else { return };
    if !write_external_pose(destination, &composed) {
        return;
    }
    // SAFETY: a C++ service.
    let fov = unsafe { cvr_cpp_normal_fov_override() };
    if fov.is_finite() && fov > 1.0 && fov < 179.0 {
        write(destination + 0x20, fov);
    }
    let component = interface.wrapping_sub(0x120);
    // SAFETY: the local sample.
    unsafe { cvr_pose_publish_setup(destination, component, 1, head.id, &head.head, true) };
    if diagnostics() {
        record_external(0, destination, component, &head);
    }
}
#[no_mangle]
pub unsafe extern "C" fn CvrPtMainRead(director: usize, destination: usize, setup: usize) -> usize {
    // Bind the actual selected setup argument. Reading director+500 here would mislabel an override chosen by the
    // director after an earlier observation.
    let receipt = capture(setup);
    let previous = T_SOURCE.with(|t| t.replace(receipt));
    if !receipt.valid() {
        // SAFETY: exported counters.
        count(unsafe { &CyberpunkVR_PoseMainReadMissing[if setup == director.wrapping_add(0x500) { 0 } else { 1 }] });
    }
    cvr_pose_invalidate(destination);
    // SAFETY: the engine's read.
    let result = unsafe { call3(O_MAIN_READ, director, destination, setup) };
    let source = T_SOURCE.with(|t| t.get());
    // SAFETY: the local receipt.
    unsafe { cvr_pose_transfer(&source, destination) };
    publish_external_main(destination, &capture(destination));
    T_SOURCE.with(|t| t.set(previous));
    result
}
#[no_mangle]
pub unsafe extern "C" fn CvrPtRttBuild(component: usize, destination: usize, force: i8) -> f64 {
    // D29AA8 reads this component's +E0/+F0 before publishing its named view. Capture BEFORE the read, including
    // when the resulting coordinates repeat.
    let receipt = capture(component.wrapping_add(0xE0));
    let previous_source = T_SOURCE.with(|t| t.replace(receipt));
    // SAFETY: C++ services.
    let vrcam = unsafe { cvr_cpp_vrcam_comp() };
    if component != 0 && component == vrcam {
        // SAFETY: as above.
        unsafe { cvr_cpp_external_vrcam_observe(component) };
    }
    // SAFETY: as above.
    let selection = if component == unsafe { cvr_cpp_vrcam_comp() } { read_external_main() } else { None };
    let external = selection.is_some();
    let held = selection.unwrap_or(SELECTION_ZERO);
    let previous_selection = T_EXTERNAL_RTT.with(|t| t.replace(if external { &held } else { core::ptr::null() }));
    let previous = T_RTT_COMPONENT.with(|t| t.replace(component));
    // SAFETY: the engine's RTT build.
    let result = unsafe {
        core::mem::transmute::<usize, unsafe extern "C" fn(usize, usize, i8) -> f64>(original(O_RTT))(component, destination, if external { 1 } else { force })
    };
    T_RTT_COMPONENT.with(|t| t.set(previous));
    T_EXTERNAL_RTT.with(|t| t.set(previous_selection));
    T_SOURCE.with(|t| t.set(previous_source));
    result
}
#[no_mangle]
pub unsafe extern "C" fn CvrPtRttRefresh(component: usize) {
    // SAFETY: C++ services.
    if component != 0 && component == unsafe { cvr_cpp_vrcam_comp() } {
        // SAFETY: as above.
        unsafe { cvr_cpp_external_vrcam_observe(component) };
        if read_external_main().is_some() {
            // SAFETY: the component the RTT callback owns.
            unsafe { notify_external_vrcam(component) };
        }
    }
    // SAFETY: the engine's refresh.
    unsafe { call1(O_RTT_REFRESH, component) };
}
fn descriptor_matches(descriptor: usize, receipt: &PoseReceipt) -> bool {
    if !receipt.valid() || descriptor < 0x10000 {
        return false;
    }
    let mut bytes = [0u8; 28];
    if !read_bytes(descriptor, &mut bytes) {
        return false;
    }
    let mut rotation = [0u8; 16];
    let mut position = [0u8; 12];
    for (i, v) in receipt.fingerprint.rotation.iter().enumerate() {
        rotation[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    for (i, v) in receipt.fingerprint.position.iter().enumerate() {
        position[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    bytes[..16] == rotation && bytes[16..] == position
}
#[no_mangle]
pub unsafe extern "C" fn CvrPtDescriptorBuildAt(destination: usize, descriptor: usize, return_address: usize) -> usize {
    let rtt = T_RTT_COMPONENT.with(|t| t.get());
    // SAFETY: a C++ service.
    if rtt != 0 && rtt == unsafe { cvr_cpp_vrcam_comp() } && return_address == EXE.load(Ordering::Acquire) + 0xD29C8C {
        let selection = T_EXTERNAL_RTT.with(|t| t.get());
        if !selection.is_null() {
            // SAFETY: the RttBuild frame's selection, alive for this nested call.
            let source = unsafe { *selection };
            let mut opposite = source.main;
            let offset = if ipd_in_world() { cvr_camera_desired_half_ipd() * 2.0 * -main_eye_sign() } else { 0.0 };
            if external_eye(&mut opposite, offset) {
                // This is the native builder's temporary descriptor, not the player's placed component. Matrices and
                // culling are constructed by the original from these fields; no late camera rewrite. 2.5 reads the
                // input up to +98; the caller's descriptor is a frame local, so the wider read stays inside its frame.
                #[repr(C, align(16))]
                struct Copy([u8; 0xA0]);
                let mut copy = Copy([0; 0xA0]);
                // SAFETY: the engine's descriptor, a frame local of at least 0xA0 bytes.
                unsafe { core::ptr::copy_nonoverlapping(descriptor as *const u8, copy.0.as_mut_ptr(), 0xA0) };
                let r = [opposite.rotation.x, opposite.rotation.y, opposite.rotation.z, opposite.rotation.w];
                for (i, v) in r.iter().enumerate() {
                    copy.0[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
                }
                for (i, v) in opposite.position.iter().enumerate() {
                    copy.0[0x10 + i * 4..0x14 + i * 4].copy_from_slice(&v.to_le_bytes());
                }
                if source.fov.is_finite() && source.fov > 1.0 && source.fov < 179.0 {
                    copy.0[0x38..0x3C].copy_from_slice(&source.fov.to_le_bytes());
                }
                cvr_pose_invalidate(destination);
                // SAFETY: the engine's builder, with the local copy.
                let result = unsafe { call2(O_DESCRIPTOR, destination, copy.0.as_ptr() as usize) };
                // SAFETY: the local sample.
                unsafe { cvr_pose_publish_setup(destination, rtt, 2, source.sample.id, &source.sample.head, source.external_view) };
                if diagnostics() {
                    record_external(1, destination, rtt, &source.sample);
                }
                return result;
            }
        }
    }
    let current = T_SOURCE.with(|t| t.get());
    let source = if descriptor_matches(descriptor, &current) { current } else { PoseReceipt::default() };
    cvr_pose_invalidate(destination);
    // SAFETY: the engine's builder.
    let result = unsafe { call2(O_DESCRIPTOR, destination, descriptor) };
    // SAFETY: the local receipt.
    unsafe { cvr_pose_transfer(&source, destination) };
    result
}
#[no_mangle]
pub unsafe extern "C" fn CvrPtCameraClone(destination: usize, source: usize) -> usize {
    let receipt = capture(source);
    // SAFETY: the engine's clone.
    let result = unsafe { call2(O_CAMERA_CLONE, destination, source) };
    // SAFETY: the local receipt.
    unsafe { cvr_pose_transfer(&receipt, destination) };
    result
}
/// The two verified calls (4248F6, 424951) pass context+2ED0; their descriptor is made from context+2C50, populated
/// by the registered-camera copy.
#[no_mangle]
pub unsafe extern "C" fn CvrPtRenderBuildAt(destination: usize, descriptor: usize, clip_planes: usize, return_address: usize) -> usize {
    let exe = EXE.load(Ordering::Acquire);
    let mut source = PoseReceipt::default();
    if (return_address == exe + 0x4248FB || return_address == exe + 0x424956) && clip_planes > 0x2ED0 {
        let context = clip_planes - 0x2ED0;
        source = capture(context + 0x2C50);
        if !descriptor_matches(descriptor, &source) {
            source = PoseReceipt::default();
        }
    }
    cvr_pose_invalidate(destination);
    // SAFETY: the engine's render-view build.
    let result = unsafe { call3(O_RENDER, destination, descriptor, clip_planes) };
    // SAFETY: the local receipt.
    unsafe { cvr_pose_transfer(&source, destination) };
    result
}

// The return address as one more argument: the naked entries load it from [rsp] and jump, so the body returns
// straight to the engine.
#[unsafe(naked)]
unsafe extern "C" fn blend_finalize_entry() {
    core::arch::naked_asm!("mov rdx, [rsp]", "jmp {body}", body = sym CvrPtBlendFinalizeAt);
}
#[unsafe(naked)]
unsafe extern "C" fn descriptor_build_entry() {
    core::arch::naked_asm!("mov r8, [rsp]", "jmp {body}", body = sym CvrPtDescriptorBuildAt);
}
#[unsafe(naked)]
unsafe extern "C" fn render_build_entry() {
    core::arch::naked_asm!("mov r9, [rsp]", "jmp {body}", body = sym CvrPtRenderBuildAt);
}

// ---- the asm thunks' halves ----
#[no_mangle]
pub unsafe extern "C" fn CvrPoseSetupDefaultBefore(destination: usize) {
    cvr_pose_invalidate(destination);
}
/// `storage`: the thunk's 512-byte per-call slot for the receipt.
#[no_mangle]
pub unsafe extern "C" fn CvrPoseSetupCopyBefore(storage: *mut PoseReceipt, source: usize) {
    // SAFETY: the thunk's slot (0x200 bytes, 16-aligned) holds one receipt.
    unsafe { storage.write(capture(source)) };
}
#[no_mangle]
pub unsafe extern "C" fn CvrPoseSetupCopyAfter(receipt: *const PoseReceipt, destination: usize) {
    // SAFETY: the receipt CvrPoseSetupCopyBefore wrote into the thunk's slot.
    unsafe { cvr_pose_transfer(receipt, destination) };
}

// ---- the readers ----
#[no_mangle]
pub extern "C" fn cvr_pose_transport_serialized_component() -> usize {
    T_SERIALIZING_COMPONENT.with(|t| t.get())
}
#[no_mangle]
pub unsafe extern "C" fn cvr_pose_transport_read_main_aim(out: *mut MainAimPose) -> bool {
    // SAFETY: a C++ service.
    if out.is_null() || !external_allowed() || unsafe { cvr_cpp_xr_external_pose_reset_pending() } {
        return false;
    }
    let key = external_key();
    let now = now_us();
    let mut frame = {
        let l = lock(&LEASES);
        match l.aim.read(key, now) {
            Some(f) if now - l.aim.stamp <= 100000 => f,
            _ => return false,
        }
    };
    let offset = if ipd_in_world() { cvr_camera_desired_half_ipd() * -main_eye_sign() } else { 0.0 };
    if !external_eye(&mut frame.main, offset) {
        return false;
    }
    let r = frame.main.rotation;
    // SAFETY: the caller's pose.
    unsafe {
        *out = MainAimPose {
            position: [frame.main.position[0] as f32 / 131072.0, frame.main.position[1] as f32 / 131072.0, frame.main.position[2] as f32 / 131072.0],
            rotation: [r.x, r.y, r.z, r.w],
            pose_id: frame.sample.id,
        }
    };
    true
}
#[no_mangle]
pub unsafe extern "C" fn cvr_pose_transport_read_external_direction(out: *mut f32, pose_id: *mut u64) -> bool {
    // SAFETY: a C++ service.
    if out.is_null() || !external_allowed() || unsafe { cvr_cpp_xr_external_pose_reset_pending() } {
        return false;
    }
    let key = external_key();
    let now = now_us();
    // Steering needs a recent observation even though a missing MAIN update is never a reason to move the rendered
    // second eye back into the cabin.
    let frame = {
        let l = lock(&LEASES);
        match l.main.read(key, now) {
            Some(f) if f.external_view && now - l.main.stamp <= 100000 => f,
            _ => return false,
        }
    };
    let Some(forward) = planar_external_forward(frame.main.rotation) else { return false };
    // SAFETY: the caller's three floats and id.
    unsafe {
        *out = forward.x;
        *out.add(1) = forward.y;
        *out.add(2) = 0.0;
        if !pose_id.is_null() {
            *pose_id = frame.sample.id;
        }
    }
    true
}

// ---- the install ----
/// Installed from src/Hooks/CameraPoseTransport.cpp's registration (Boot, 81).
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_pose_transport_install() -> bool {
    use cvr_engine::hooks::{install_chain, Detour};
    let exe = image_base();
    if exe == 0 {
        return false;
    }
    EXE.store(exe, Ordering::Release);
    let sites: [(&str, &Site, *const (), *mut usize); 12] = [
        ("Blend", &BLEND, CvrPtBlend as *const (), ORIGINALS[O_BLEND].as_ptr()),
        ("BlendFinalize", &BLEND_FINALIZE, blend_finalize_entry as *const (), ORIGINALS[O_BLEND_FINALIZE].as_ptr()),
        ("Serialize", &SERIALIZE, CvrPtSerialize as *const (), ORIGINALS[O_SERIALIZE].as_ptr()),
        ("GenericSerialize", &GENERIC_SERIALIZE, CvrPtGenericSerialize as *const (), ORIGINALS[O_GENERIC_SERIALIZE].as_ptr()),
        ("SetupCopy", &SETUP_COPY, CvrPoseSetupCopyDetour as *const (), (&raw mut CvrPoseSetupCopyOriginal).cast()),
        ("SetupDefault", &SETUP_DEFAULT, CvrPoseSetupDefaultDetour as *const (), (&raw mut CvrPoseSetupDefaultOriginal).cast()),
        ("MainRead", &MAIN_READ, CvrPtMainRead as *const (), ORIGINALS[O_MAIN_READ].as_ptr()),
        ("RttBuild", &RTT_BUILD, CvrPtRttBuild as *const (), ORIGINALS[O_RTT].as_ptr()),
        ("RttRefresh", &RTT_REFRESH, CvrPtRttRefresh as *const (), ORIGINALS[O_RTT_REFRESH].as_ptr()),
        ("DescriptorBuild", &DESCRIPTOR_BUILD, descriptor_build_entry as *const (), ORIGINALS[O_DESCRIPTOR].as_ptr()),
        ("CameraClone", &CAMERA_CLONE, CvrPtCameraClone as *const (), ORIGINALS[O_CAMERA_CLONE].as_ptr()),
        ("RenderBuild", &RENDER_BUILD, render_build_entry as *const (), ORIGINALS[O_RENDER].as_ptr()),
    ];
    if sites.iter().any(|(_, site, _, _)| !site_matches(exe, site))
        || !site_matches(exe, &RENDER_CALL)
        || !site_matches(exe, &BLEND_FINALIZE_CALL)
        || !site_matches(exe, &RTT_DESCRIPTOR_CALL)
    {
        return false;
    }
    // SAFETY: a C++ service (WeaponAim's pose-copy hook).
    if !unsafe { cvr_cpp_ensure_camera_pose_copy_hook() } {
        return false;
    }
    let detours: Vec<Detour> = sites
        .iter()
        .map(|&(name, site, hook, original)| Detour {
            name,
            target: site.rva,
            absolute: false,
            detour: hook as *const c_void,
            original: original.cast(),
            expect: None,
        })
        .collect();
    // SAFETY: the sites match the bytes the port was written against; all or none (the chain logs either way).
    unsafe { install_chain("pose-transport", &detours) }.is_ok()
}
