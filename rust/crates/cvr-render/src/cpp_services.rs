//! The C++ half's services that were only forwards into Rust, defined in Rust under the same C names: the Rust solve
//! (cvr-rig: the pose hook, the camera, the XInput merge, the menus, the remote camera), the plugin's entry
//! (cvr-ffi: the runtime paths, RenderDoc) and the OpenXR runtime (cvr-xr: the Vive trackers) call them as before.
//! Formerly src/Anim/CharacterRigRust.cpp, src/Camera/CameraServices.cpp, src/Hooks/HookServices.cpp,
//! src/Hooks/InputServices.cpp, src/Hooks/LeftHandedAnimation.cpp, src/Hooks/OsInput.cpp, src/Main.cpp,
//! src/Natives/AnimLookup.cpp, src/Natives/RemoteCamera.cpp, src/Natives/VrMenu.cpp and src/Runtimes/OpenXRManager.cpp,
//! each calling a C++ function whose body called the Rust export. tools/vrik_tests (built without rust-build) keeps
//! its own C forms (tools/vrik_tests/stubs.cpp).
use core::ffi::{c_char, c_void};
use core::sync::atomic::Ordering::Relaxed;

use crate::{camera_owner, camera_takeover, lobby_intro, menu_lobby, runtime_paths, ui_rec};

/// cvr::roomscale::Vec2 (rust/crates/cvr-rig/src/vrik/camera_link.rs's Vec2).
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct Vec2 {
    x: f32,
    y: f32,
}

// The Rust solve's exports (cvr-rig, which this crate does not depend on: resolved by the linker).
unsafe extern "C" {
    fn cvr_ladder_read_hand_kinds(kinds: *mut i32);
    fn cvr_ladder_constrain_hand(side: i32, target: *mut f32, rotation: *mut f32, position: *const f32, entity_rotation: *const f32);
    fn cvr_roomscale_camera_consumed() -> Vec2;
    fn cvr_roomscale_camera_rest_neck(out: *mut Vec2) -> bool;
    fn cvr_body_yaw_follow_active() -> bool;
    fn cvr_body_tracking_yaw(fallback: f32) -> f32;
    fn cvr_weapon_aim_ensure_pose_copy_hook() -> bool;
    fn cvr_scene_comfort_apply(raw: *const f32, out: *mut f32) -> bool;
    fn cvr_surveillance_view_yaw(lens: usize, owner: usize, origin: u64, owner_yaw: f32, yaw: *mut f32) -> bool;
    fn cvr_reach_aim_point(point: *mut f32) -> bool;
    fn cvr_swimming_read_steering(direction: *mut [f32; 3], state: *mut i32) -> bool;
    fn cvr_swimming_bind_native_input(locomotion: *mut c_void);
    safe fn cvr_action_reads_active() -> bool;
    safe fn cvr_quest_suspend_manual_clue_input();
    safe fn cvr_quest_consume_manual_clue_button(pressed: bool, allowed: bool) -> bool;
    safe fn cvr_swimming_reset_input();
    fn cvr_swimming_sprint_input(wanted: bool) -> bool;
    fn cvr_swimming_update_input(controllers: *const c_void, manual_back: bool, out: *mut c_void);
    safe fn cvr_ladder_reset_input();
    safe fn cvr_reach_use_held() -> bool;
    safe fn cvr_vrmenu_captures_buttons() -> bool;
    safe fn cvr_vrmenu_wheel_armed(now: u64) -> bool;
    safe fn cvr_vrmenu_publish_wheel_hold(pending: bool, held: bool);
    safe fn cvr_keypad_input_active() -> bool;
    safe fn cvr_keypad_publish_stick(x: f32, y: f32, deadzone: f32);
    safe fn cvr_in_braindance() -> bool;
    fn cvr_item_handling_bits(feature: *const c_void) -> u32;
    safe fn cvr_weapon_aim_install() -> bool;
    fn cvr_vrmenu_finger_world(side: i32, world: *mut f32) -> bool;
}

// ---- the pose hook (src/Anim/CharacterRigRust.cpp)
#[no_mangle]
pub extern "C" fn cvr_cpp_pose_device_cam_active() -> bool {
    camera_takeover::cvr_device_cam_active()
}
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_ladder_read_hand_kinds(kinds: *mut i32) {
    // SAFETY: the caller's two ints.
    unsafe { cvr_ladder_read_hand_kinds(kinds) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_ladder_constrain_hand(side: i32, target: *mut f32, rotation: *mut f32, position: *const f32,
                                                       entity_rotation: *const f32) {
    // SAFETY: the caller's vectors, passed on.
    unsafe { cvr_ladder_constrain_hand(side, target, rotation, position, entity_rotation) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_roomscale_camera_consumed(out: *mut f32) {
    // SAFETY: the solve's state; the caller's two floats.
    unsafe {
        let v = cvr_roomscale_camera_consumed();
        *out = v.x;
        *out.add(1) = v.y;
    }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_roomscale_camera_rest_neck(out: *mut f32) -> bool {
    let mut v = Vec2::default();
    // SAFETY: a local; the caller's two floats.
    unsafe {
        let ok = cvr_roomscale_camera_rest_neck(&mut v);
        *out = v.x;
        *out.add(1) = v.y;
        ok
    }
}
#[no_mangle]
pub extern "C" fn cvr_cpp_body_yaw_follow_active() -> bool {
    // SAFETY: the solve's state.
    unsafe { cvr_body_yaw_follow_active() }
}
#[no_mangle]
pub extern "C" fn cvr_cpp_vrik_body_tracking_yaw(native_yaw: f32) -> f32 {
    // SAFETY: the solve's state.
    unsafe { cvr_body_tracking_yaw(native_yaw) }
}

// ---- the camera (src/Camera/CameraServices.cpp, src/Hooks/HookServices.cpp)
#[no_mangle]
pub extern "C" fn cvr_cpp_device_cam_active() -> bool {
    camera_takeover::cvr_device_cam_active()
}
#[no_mangle]
pub extern "C" fn cvr_cpp_lobby_world_shown() -> bool {
    menu_lobby::cvr_menu_lobby_world_shown()
}
#[no_mangle]
pub extern "C" fn cvr_cpp_ensure_camera_pose_copy_hook() -> bool {
    // SAFETY: the weapon aim's hook, installed once under its lock.
    unsafe { cvr_weapon_aim_ensure_pose_copy_hook() }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_scene_comfort_apply(raw: *const f32, out: *mut f32) -> bool {
    // SAFETY: the caller's vectors, passed on (null-checked there).
    unsafe { cvr_scene_comfort_apply(raw, out) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_surveillance_view_yaw(lens: usize, owner: usize, origin: u64, owner_yaw: f32, yaw: *mut f32) -> bool {
    // SAFETY: the caller's float.
    !yaw.is_null() && unsafe { cvr_surveillance_view_yaw(lens, owner, origin, owner_yaw, yaw) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_reach_aim_point(point: *mut f32) -> bool {
    // SAFETY: the caller's three floats.
    !point.is_null() && unsafe { cvr_reach_aim_point(point) }
}
#[no_mangle]
pub extern "C" fn cvr_cpp_lobby_holds_render(menu_state: usize) -> bool {
    menu_lobby::cvr_menu_lobby_holds_render(menu_state as *const c_void)
}
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_swimming_read_steering(direction: *mut f32, state: *mut i32) -> bool {
    let mut d = [0.0f32; 3];
    // SAFETY: a local; the caller's three floats and int.
    unsafe {
        let ok = cvr_swimming_read_steering(&mut d, state);
        core::ptr::copy_nonoverlapping(d.as_ptr(), direction, 3);
        ok
    }
}
#[no_mangle]
pub extern "C" fn cvr_cpp_shot_shared() -> *mut f32 {
    camera_takeover::cvr_get_shot_shared()
}
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_swimming_bind_native_input(locomotion: *mut c_void) {
    // SAFETY: the engine's locomotion object, passed on.
    unsafe { cvr_swimming_bind_native_input(locomotion) }
}
#[no_mangle]
pub extern "C" fn cvr_cpp_swimming_script_hooks_active() -> bool {
    cvr_action_reads_active()
}

// ---- the XInput merge (src/Hooks/InputServices.cpp)
#[no_mangle]
pub extern "C" fn cvr_cpp_xr_set_shared_slot(slot: i32, value: f32) {
    let p = cvr_xr::switches::SWITCHES.shared_hands.load(Relaxed);
    if !p.is_null() && (0..256).contains(&slot) {
        // SAFETY: the present's 256-float block.
        unsafe { *p.add(slot as usize) = value }
    }
}
#[no_mangle]
pub extern "C" fn cvr_cpp_xr_get_shared_slot(slot: i32) -> f32 {
    let p = cvr_xr::switches::SWITCHES.shared_hands.load(Relaxed);
    if !p.is_null() && (0..256).contains(&slot) {
        // SAFETY: the present's 256-float block.
        unsafe { *p.add(slot as usize) }
    } else {
        0.0
    }
}
#[no_mangle]
pub extern "C" fn cvr_cpp_quest_suspend_manual_clue_input() {
    cvr_quest_suspend_manual_clue_input()
}
#[no_mangle]
pub extern "C" fn cvr_cpp_quest_consume_manual_clue_button(pressed: bool, allowed: bool) -> bool {
    cvr_quest_consume_manual_clue_button(pressed, allowed)
}
#[no_mangle]
pub extern "C" fn cvr_cpp_swimming_reset_input() {
    cvr_swimming_reset_input()
}
/// The swim strokes' input from the controllers (cvr::swimming::UpdateInput's C entry): the XInput merge goes through
/// this service so vrik_tests' bench records the call (tools/vrik_tests/stubs.cpp, the input parity's call 104).
///
/// # Safety
/// `controllers` is a VRControllerState, `out` a cvr::swimming::Input.
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_swimming_update_input(controllers: *const c_void, manual_back: bool, out: *mut c_void) {
    // SAFETY: the caller's.
    unsafe { cvr_swimming_update_input(controllers, manual_back, out) }
}
#[no_mangle]
pub extern "C" fn cvr_cpp_swimming_sprint_input(wanted: bool) -> bool {
    // SAFETY: the swimming input's state.
    unsafe { cvr_swimming_sprint_input(wanted) }
}
#[no_mangle]
pub extern "C" fn cvr_cpp_ladder_reset_input() {
    cvr_ladder_reset_input()
}
#[no_mangle]
pub extern "C" fn cvr_cpp_vrui_captures_input() -> bool {
    cvr_body::vr_overlay::cvr_vrui_captures_input()
}
#[no_mangle]
pub extern "C" fn cvr_cpp_reach_use_held() -> bool {
    cvr_reach_use_held()
}
#[no_mangle]
pub extern "C" fn cvr_cpp_vrmenu_captures_buttons() -> bool {
    cvr_vrmenu_captures_buttons()
}
#[no_mangle]
pub extern "C" fn cvr_cpp_vrmenu_wheel_armed(now: u64) -> bool {
    cvr_vrmenu_wheel_armed(now)
}
#[no_mangle]
pub extern "C" fn cvr_cpp_vrmenu_publish_wheel_hold(pending: bool, held: bool) {
    cvr_vrmenu_publish_wheel_hold(pending, held)
}
#[no_mangle]
pub extern "C" fn cvr_cpp_keypad_input_active() -> bool {
    cvr_keypad_input_active()
}
#[no_mangle]
pub extern "C" fn cvr_cpp_keypad_publish_stick(x: f32, y: f32, deadzone: f32) {
    cvr_keypad_publish_stick(x, y, deadzone)
}
#[no_mangle]
pub extern "C" fn cvr_cpp_lobby_curtain() -> f32 {
    menu_lobby::cvr_menu_lobby_curtain()
}
#[no_mangle]
pub extern "C" fn cvr_cpp_lobby_intro_running() -> bool {
    lobby_intro::cvr_intro_running()
}
#[no_mangle]
pub extern "C" fn cvr_cpp_in_braindance() -> bool {
    cvr_in_braindance()
}
#[no_mangle]
pub extern "C" fn cvr_cpp_scene_stick_yaw_armed() -> bool {
    camera_takeover::cvr_scene_stick_yaw_armed()
}

// ---- the left-handed doors, the OS input, the plugin's entry, the natives (LeftHandedAnimation.cpp, OsInput.cpp,
// Main.cpp, AnimLookup.cpp, RemoteCamera.cpp, VrMenu.cpp)
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_item_handling_bits(feature: *const c_void) -> u32 {
    // SAFETY: the engine's feature, passed on (null-checked there).
    unsafe { cvr_item_handling_bits(feature) }
}
#[no_mangle]
pub extern "C" fn cvr_cpp_launcher_open() -> bool {
    cvr_settings::launcher_dialog::cvr_launcher_is_open() != 0
}
#[no_mangle]
pub extern "C" fn cvr_cpp_init_runtime_paths() {
    runtime_paths::cvr_init_runtime_paths()
}
#[no_mangle]
pub extern "C" fn cvr_cpp_renderdoc_allow_nvapi() {
    // SAFETY: RenderDoc's API, when its module is loaded.
    unsafe { cvr_gpu::renderdoc_bridge::cvr_renderdoc_allow_nvapi() };
}
#[no_mangle]
pub extern "C" fn cvr_cpp_install_weapon_aim_hooks() -> bool {
    cvr_weapon_aim_install()
}
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_vrmenu_finger_world(side: i32, world: *mut f32) -> bool {
    // SAFETY: the caller's three floats.
    unsafe { cvr_vrmenu_finger_world(side, world) }
}
#[no_mangle]
pub extern "C" fn cvr_cpp_takeover_lens_release() {
    camera_takeover::cvr_takeover_lens_release()
}
#[no_mangle]
pub extern "C" fn cvr_cpp_braindance_camera_release() {
    camera_takeover::cvr_braindance_camera_release()
}
#[no_mangle]
pub extern "C" fn cvr_cpp_patch_fast_disarm() {
    camera_owner::cvr_patch_fast_disarm()
}
#[no_mangle]
pub extern "C" fn cvr_cpp_device_cam_restore_fov() {
    camera_takeover::cvr_device_cam_restore_fov()
}
#[no_mangle]
pub extern "C" fn cvr_cpp_vrui_consume_menu_summon() -> bool {
    cvr_body::vr_overlay::cvr_vrui_consume_menu_summon()
}
#[no_mangle]
pub extern "C" fn cvr_cpp_vrui_visible() -> bool {
    cvr_body::vr_overlay::cvr_vrui_visible()
}
#[no_mangle]
pub extern "C" fn cvr_cpp_vrui_toggle() {
    cvr_body::vr_overlay::cvr_vrui_toggle()
}
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_uirec_queue_edit(page: i32, id: *const c_char, value: f64) {
    // SAFETY: the caller's string, passed on (null-checked there).
    unsafe { ui_rec::cvr_uirec_queue_edit(page, id, value) }
}

// ---- the Vive trackers (src/Runtimes/OpenXRManager.cpp)
#[no_mangle]
pub extern "C" fn cvr_cpp_xr_on_instance_created(instance: u64) {
    // SAFETY: the runtime's instance handle.
    unsafe { cvr_xr::trackers::cvr_trackers_on_instance_created(instance) }
}
#[no_mangle]
pub extern "C" fn cvr_cpp_xr_create_tracker_actions(instance: u64, set: u64) {
    // SAFETY: the runtime's instance and action set handles.
    unsafe { cvr_xr::trackers::cvr_trackers_create_actions(instance, set) }
}

// ---- the C names the C++ half kept over Rust (B459): the launch window, the log, the depth capture, the GPU
// diagnostics, body follow, the swapchain's DRED and scene depth, the RTV swapchain, the upload maps, the bounds trace
// and the scene update's thunks
unsafe extern "C" {
    safe fn cvr_launch_window_mode_apply();
    fn cvr_log_text(text: *const c_char);
    fn cvr_depth_wait_on_all_game_signals(consumer: *mut c_void);
    fn cvr_gpu_diag_before_device();
    fn cvr_gpu_diag_device(raw: *mut c_void);
    fn cvr_body_yaw_follow_release();
    fn cvr_body_yaw_follow_step(observed_offset: f32) -> f32;
    fn cvr_body_yaw_follow_tick(engine_z: f32, engine_w: f32, engine_pos: *const f32);
    safe fn cvr_dred_enable_once();
    safe fn cvr_depth_resource() -> *mut c_void;
    safe fn cvr_depth_acquire() -> *mut c_void;
    safe fn cvr_depth_writer_queue() -> *mut c_void;
    safe fn cvr_depth_state() -> u32;
    safe fn cvr_depth_width() -> u32;
    safe fn cvr_depth_height() -> u32;
    safe fn cvr_depth_format() -> u32;
}
/// src/Core/FirstLaunch.cpp.
#[no_mangle]
pub extern "C" fn ApplyLaunchWindowMode() {
    cvr_launch_window_mode_apply()
}
/// src/Core/Log.cpp: one line into the plugin's log.
#[no_mangle]
pub unsafe extern "C" fn CvrLogText(text: *const c_char) {
    // SAFETY: the caller's string.
    unsafe { cvr_log_text(text) }
}
/// src/Hooks/DepthCapture.cpp.
#[no_mangle]
pub unsafe extern "C" fn CyberpunkVRPort_WaitOnAllGameSignals(consumer_queue: *mut c_void) {
    // SAFETY: the caller's queue.
    unsafe { cvr_depth_wait_on_all_game_signals(consumer_queue) }
}
/// src/Hooks/Dred.cpp.
#[no_mangle]
pub extern "C" fn CyberpunkVRPort_GpuDiagBeforeDevice() {
    // SAFETY: d3d12's debug interfaces, before the device.
    unsafe { cvr_gpu_diag_before_device() }
}
#[no_mangle]
pub unsafe extern "C" fn CyberpunkVRPort_GpuDiagDevice(device: *mut c_void) {
    // SAFETY: the game's device.
    unsafe { cvr_gpu_diag_device(device) }
}
/// src/Hooks/RoomscaleRust.cpp: body follow's C names.
#[no_mangle]
pub extern "C" fn BodyYawFollowRelease() {
    // SAFETY: the follower's state.
    unsafe { cvr_body_yaw_follow_release() }
}
#[no_mangle]
pub extern "C" fn BodyYawFollowStep(observed_offset: f32) -> f32 {
    // SAFETY: the follower's state.
    unsafe { cvr_body_yaw_follow_step(observed_offset) }
}
#[no_mangle]
pub unsafe extern "C" fn BodyYawFollowTick(engine_z: f32, engine_w: f32, engine_pos: *const f32) {
    // SAFETY: the caller's position.
    unsafe { cvr_body_yaw_follow_tick(engine_z, engine_w, engine_pos) }
}
/// src/Hooks/SwapChain.cpp: DRED once, and the scene depth for the capture (one owned reference from the acquire).
#[no_mangle]
pub extern "C" fn CyberpunkVRPort_EnableDredOnce() {
    cvr_dred_enable_once()
}
#[no_mangle]
pub extern "C" fn OmoGetSceneDepthResource() -> *mut c_void {
    cvr_depth_resource()
}
#[no_mangle]
pub extern "C" fn OmoAcquireSceneDepthResource() -> *mut c_void {
    cvr_depth_acquire()
}
#[no_mangle]
pub extern "C" fn OmoGetSceneDepthWriterQueue() -> *mut c_void {
    cvr_depth_writer_queue()
}
#[no_mangle]
pub extern "C" fn OmoGetSceneDepthState() -> u32 {
    cvr_depth_state()
}
#[no_mangle]
pub extern "C" fn OmoGetSceneDepthWidth() -> u32 {
    cvr_depth_width()
}
#[no_mangle]
pub extern "C" fn OmoGetSceneDepthHeight() -> u32 {
    cvr_depth_height()
}
#[no_mangle]
pub extern "C" fn OmoGetSceneDepthFormat() -> u32 {
    cvr_depth_format()
}
/// src/Stereo/Capture.cpp.
#[no_mangle]
pub unsafe extern "C" fn CyberpunkVR_RegisterRtvSwapchain(swap_chain: *mut c_void) {
    // SAFETY: the caller's swapchain.
    unsafe { crate::capture_rtv::cvr_capture_register_rtv_swapchain(swap_chain) }
}
unsafe extern "C-unwind" {
    /// The upload maps' CPU view of a GPU address (upload_maps.rs).
    safe fn cvr_upload_cpu_for_va(va: u64, need: u64) -> *const u8;
}
/// src/Stereo/ViewConstants.cpp.
#[no_mangle]
pub extern "C" fn cvr_cpp_upload_cpu_for_va(va: u64, need: u64) -> *const u8 {
    cvr_upload_cpu_for_va(va, need)
}
/// src/Stereo/NativeBoundsTrace.cpp: the bounds and particle thunks' C names.
#[no_mangle]
pub extern "C" fn CvrBoundsBefore(pool: *mut c_void, indices: *mut c_void, bounds: *const c_void, caller: usize) {
    crate::bounds_trace::cvr_bt_observe(pool as u64, indices as u64, bounds as u64, caller as u64)
}
#[no_mangle]
pub extern "C" fn CvrParticleBefore(system: *mut c_void, context: *mut c_void, emitter: *mut c_void, stack: *const c_void) {
    crate::bounds_trace::cvr_bt_before_particle(system as u64, context as u64, emitter as u64, stack as u64)
}
/// src/Stereo/NativeSceneUpdate.cpp: the scene update thunks' C names.
#[no_mangle]
pub unsafe extern "C" fn CvrSceneUpdateBefore(event: u32, frame: *const crate::scene_update::ThunkFrame) -> u64 {
    // SAFETY: the thunk's frame.
    unsafe { crate::scene_update::cvr_su_before(event, frame) }
}
#[no_mangle]
pub unsafe extern "C" fn CvrSceneUpdateAfter(event: u32, cookie: u64, frame: *const crate::scene_update::ThunkFrame) {
    // SAFETY: the thunk's frame.
    unsafe { crate::scene_update::cvr_su_after(event, cookie, frame) }
}

// ---- the C++ half's services over its globals (B457): each read or written here by the global's own name
#[allow(non_upper_case_globals)]
mod globals {
    use core::ffi::c_void;
    use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicUsize};
    unsafe extern "C" {
        #[link_name = "?g_isDriving@@3U?$atomic@_N@std@@A"]
        pub static g_isDriving: AtomicBool;
        pub static g_sceneTier: AtomicI32;
        #[link_name = "?g_vehicleState@@3U?$atomic@H@std@@A"]
        pub static g_vehicleState: AtomicI32;
        #[link_name = "?g_gameModuleBase@@3_KA"]
        pub static g_gameModuleBase: usize;
        #[link_name = "?g_gameModuleSize@@3_KA"]
        pub static g_gameModuleSize: usize;
        pub static g_bdActive: AtomicI32;
        #[link_name = "?g_verboseLog@@3HC"]
        pub static g_verboseLog: i32;
        #[link_name = "?g_launcherWidth@@3HA"]
        pub static g_launcherWidth: i32;
        #[link_name = "?g_launcherHeight@@3HA"]
        pub static g_launcherHeight: i32;
        pub static g_menuModeValue: i32;
        pub static g_camObjMain: AtomicUsize;
        pub static g_bdScenePoseValid: AtomicI32;
        #[link_name = "?g_vrcam_comp@detail@cvr@@3U?$atomic@_K@std@@A"]
        pub static g_vrcam_comp: AtomicUsize;
        pub static g_normalFovOverrideValue: f32;
        #[link_name = "?g_remoteCamOn@@3U?$atomic@H@std@@A"]
        pub static g_remoteCamOn: AtomicI32;
        #[link_name = "?g_takeoverEntityId@@3U?$atomic@_K@std@@A"]
        pub static g_takeoverEntityId: AtomicU64;
        #[link_name = "?g_uiPopupOpen@@3U?$atomic@H@std@@A"]
        pub static g_uiPopupOpen: AtomicI32;
        #[link_name = "?g_uiPopupClosedMs@@3U?$atomic@_K@std@@A"]
        pub static g_uiPopupClosedMs: AtomicU64;
        #[link_name = "?g_bdSceneFov@@3U?$atomic@M@std@@A"]
        pub static g_bdSceneFov: AtomicU32;
        #[link_name = "?g_gameHwnd@@3PEAUHWND__@@EA"]
        pub static g_gameHwnd: *mut c_void;
        #[link_name = "?g_pendingSnapYawDeltaBits@@3JC"]
        pub static g_pendingSnapYawDeltaBits: AtomicI32;
        pub static g_lookStickX: AtomicU32;
    }
}
use core::sync::atomic::Ordering::Acquire;
use globals::*;
/// g_PlayerPoseStateLock (include/Anim/VrikState.hpp's inline SRWLOCK, emitted by no C++ unit of the Rust build any
/// more): the player pose state's lock, under its MSVC name.
#[export_name = "?g_PlayerPoseStateLock@@3U_RTL_SRWLOCK@@A"]
#[allow(non_upper_case_globals)]
pub static mut g_PlayerPoseStateLock: windows_sys::Win32::System::Threading::SRWLOCK =
    windows_sys::Win32::System::Threading::SRWLOCK_INIT;

fn read<T: Copy>(at: *const T) -> T {
    // SAFETY: a plain global of the C++ half (or Rust's), read volatile as the C++ half's volatile or plain reads.
    unsafe { core::ptr::read_volatile(at) }
}
#[no_mangle]
pub extern "C" fn cvr_cpp_wheel_driving() -> bool {
    // SAFETY: the global's atomic.
    unsafe { g_isDriving.load(Relaxed) }
}
#[no_mangle]
pub extern "C" fn cvr_cpp_is_driving() -> bool {
    // SAFETY: the global's atomic.
    unsafe { g_isDriving.load(Relaxed) }
}
#[no_mangle]
pub extern "C" fn cvr_cpp_pose_scene_tier() -> i32 {
    // SAFETY: the global's atomic.
    unsafe { g_sceneTier.load(Relaxed) }
}
#[no_mangle]
pub extern "C" fn cvr_cpp_pose_vehicle_state() -> i32 {
    // SAFETY: the global's atomic.
    unsafe { g_vehicleState.load(Relaxed) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_game_module_range(base: *mut usize, size: *mut usize) {
    // SAFETY: the caller's two outputs; the module's range, set at the plugin's start.
    unsafe {
        *base = read(&raw const g_gameModuleBase);
        *size = read(&raw const g_gameModuleSize);
    }
}
#[no_mangle]
pub extern "C" fn cvr_cpp_braindance_active() -> bool {
    // SAFETY: the global's atomic.
    unsafe { g_bdActive.load(Relaxed) != 0 }
}
#[no_mangle]
pub extern "C" fn cvr_cpp_framegen_verbose() -> bool {
    read(&raw const g_verboseLog) != 0
}
#[no_mangle]
pub extern "C" fn cvr_cpp_verbose_log() -> i32 {
    read(&raw const g_verboseLog)
}
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_launcher_size(width: *mut i32, height: *mut i32) {
    // SAFETY: the caller's two outputs.
    unsafe {
        *width = read(&raw const g_launcherWidth);
        *height = read(&raw const g_launcherHeight);
    }
}
#[no_mangle]
pub extern "C" fn cvr_cpp_menu_mode() -> i32 {
    read(&raw const g_menuModeValue)
}
#[no_mangle]
pub extern "C" fn cvr_cpp_cam_obj_main() -> usize {
    // SAFETY: the global's atomic.
    unsafe { g_camObjMain.load(Acquire) }
}
#[no_mangle]
pub extern "C" fn cvr_cpp_bd_scene_pose_valid() -> bool {
    // SAFETY: the global's atomic.
    unsafe { g_bdScenePoseValid.load(Acquire) != 0 }
}
#[no_mangle]
pub extern "C" fn cvr_cpp_vrcam_comp() -> usize {
    // SAFETY: the global's atomic.
    unsafe { g_vrcam_comp.load(Acquire) }
}
#[no_mangle]
pub extern "C" fn cvr_cpp_normal_fov_override() -> f32 {
    read(&raw const g_normalFovOverrideValue)
}
#[no_mangle]
pub extern "C" fn cvr_cpp_remote_cam_on() -> bool {
    // SAFETY: the global's atomic.
    unsafe { g_remoteCamOn.load(Acquire) != 0 }
}
#[no_mangle]
pub extern "C" fn cvr_cpp_takeover_entity_id() -> u64 {
    // SAFETY: the global's atomic.
    unsafe { g_takeoverEntityId.load(Acquire) }
}
#[no_mangle]
pub extern "C" fn cvr_cpp_pose_lock() {
    // SAFETY: the player pose state's SRW lock.
    unsafe { windows_sys::Win32::System::Threading::AcquireSRWLockExclusive(&raw mut g_PlayerPoseStateLock) }
}
#[no_mangle]
pub extern "C" fn cvr_cpp_pose_unlock() {
    // SAFETY: as above, held by this thread.
    unsafe { windows_sys::Win32::System::Threading::ReleaseSRWLockExclusive(&raw mut g_PlayerPoseStateLock) }
}
#[no_mangle]
pub extern "C" fn cvr_cpp_ui_popup_open() -> i32 {
    // SAFETY: the global's atomic.
    unsafe { g_uiPopupOpen.load(Relaxed) }
}
#[no_mangle]
pub extern "C" fn cvr_cpp_ui_popup_closed_ms() -> u64 {
    // SAFETY: the global's atomic.
    unsafe { g_uiPopupClosedMs.load(Relaxed) }
}
#[no_mangle]
pub extern "C" fn cvr_cpp_bd_scene_fov() -> f32 {
    // SAFETY: the global's atomic (a float's bits).
    f32::from_bits(unsafe { g_bdSceneFov.load(Relaxed) })
}
#[no_mangle]
pub extern "C" fn cvr_cpp_game_hwnd() -> *mut c_void {
    read(&raw const g_gameHwnd)
}
#[no_mangle]
pub extern "C" fn cvr_cpp_cursor_clipped() -> *mut bool {
    (&raw const crate::moved_globals::g_cursorClipped).cast_mut().cast()
}
#[no_mangle]
pub extern "C" fn cvr_cpp_set_pending_snap_yaw(bits: i32) {
    // SAFETY: InterlockedExchange on the global.
    unsafe { g_pendingSnapYawDeltaBits.swap(bits, core::sync::atomic::Ordering::SeqCst) };
}
#[no_mangle]
pub extern "C" fn cvr_cpp_set_look_stick_x(x: f32) {
    // SAFETY: the global's atomic (a float's bits).
    unsafe { g_lookStickX.store(x.to_bits(), Relaxed) }
}
#[no_mangle]
pub extern "C" fn cvr_cpp_game_window_in_front() -> bool {
    // SAFETY: a plain query.
    unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow() as *mut c_void == read(&raw const g_gameHwnd) }
}
#[no_mangle]
pub extern "C" fn cvr_cpp_cutscene_suspend_tier() -> i32 {
    cvr_settings::lc!(xrCutsceneSuspendTier)
}
/// PersistLiveControlsUiState(MakeLiveControlsUiState()).
#[no_mangle]
pub extern "C" fn cvr_cpp_persist_live_controls() {
    // SAFETY: a zeroed snapshot (as the C++ half default-constructed it), filled whole by the make.
    let mut state: cvr_settings::live_controls_ui_state::LiveControlsUiState = unsafe { core::mem::zeroed() };
    // SAFETY: a local snapshot.
    unsafe {
        crate::live_controls_ui::cvr_make_live_controls_ui_state(&mut state);
        crate::live_controls_ui::cvr_persist_live_controls_ui_state(&state);
    }
}

// ---- the C++ half's services over OpenXRManager's getters, read from cvr-xr's blocks and functions directly (B458)
#[no_mangle]
pub extern "C" fn cvr_cpp_xr_tracking_origin_serial() -> u64 {
    // SAFETY: cvr-xr's head block, for the process.
    unsafe { (*cvr_xr::tracking::cvr_xr_head_shared()).tracking_origin_serial.load(Acquire) }
}
#[no_mangle]
pub extern "C" fn cvr_cpp_xr_runtime_ipd() -> f32 {
    cvr_xr::runtime_info::RUNTIME.ipd.load()
}
#[no_mangle]
pub extern "C" fn cvr_cpp_xr_runtime_hfov() -> f32 {
    cvr_xr::runtime_info::RUNTIME.horizontal_fov_deg.load()
}
#[no_mangle]
pub extern "C" fn cvr_cpp_xr_runtime_psvr2() -> bool {
    cvr_xr::runtime_info::RUNTIME.psvr2.load(Relaxed)
}
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_xr_resolve_rendered_pose(head: *const c_void, out: *mut c_void) -> bool {
    // SAFETY: the caller's pose and output.
    !head.is_null() && !out.is_null() && unsafe { cvr_xr::frame::cvr_xr_resolve_rendered_pose(head.cast(), out.cast()) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_xr_eye_fov(eye: i32, out: *mut c_void) -> bool {
    // SAFETY: the caller's XrFovf.
    unsafe { cvr_xr::views::cvr_xr_eye_fov(eye, out.cast()) }
}
/// OpenXRManager::GetAsymmetricEyeWindows(out) (fitTarget true): AsymEyeWindows is the two windows.
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_xr_asym_eye_windows(out: *mut cvr_camera::projection::EyeWindow) -> bool {
    // SAFETY: the caller's two windows.
    !out.is_null() && unsafe { crate::eye_windows::cvr_xr_asym_eye_windows(out, true) }
}
#[no_mangle]
pub extern "C" fn cvr_cpp_xr_set_physical_crouch_bridge_ready(ready: bool) {
    cvr_xr::posture::cvr_xr_set_physical_crouch_bridge_ready(ready)
}
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_xr_hand_motion(hand: i32, linear: *mut f32, angular: *mut f32) -> bool {
    // SAFETY: the caller's two vectors.
    unsafe { cvr_xr::tracking::cvr_xr_get_hand_motion(hand, linear, angular) }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_xr_raw_controller_state(out: *mut c_void) -> bool {
    // SAFETY: the caller's VRControllerState.
    unsafe { cvr_xr::input::cvr_xr_controller_state(out.cast(), true) }
}

// ---- the HUD's texture leases (formerly cvr::hud::TextureLease, a std::shared_ptr the C++ half held for the HUD
// natives, the HUD panel's sprites and the optics probe; every holder in this build is Rust's) (B459)
/// An engine colour target held for one of the port's reads: its pool handle referenced and its resource owned.
struct TextureLease {
    handle: u32,
    resource: *mut c_void,
}
impl Drop for TextureLease {
    fn drop(&mut self) {
        // the handle first, then the resource (the C++ destructor's body, then its member)
        if self.handle != 0 {
            cvr_gpu::texture_lease::cvr_hud_texture_release(self.handle);
        }
        if !self.resource.is_null() {
            // SAFETY: the reference the lease took.
            unsafe { release_unknown(self.resource) };
        }
    }
}
/// IUnknown::Release.
unsafe fn release_unknown(object: *mut c_void) {
    type Release = unsafe extern "system" fn(*mut c_void) -> u32;
    // SAFETY: a live COM object: its vtable's third slot.
    unsafe {
        let vtable = *(object as *const *const usize);
        let release: Release = core::mem::transmute(*vtable.add(2));
        release(object);
    }
}
/// cvr::hud::Lease(texture, idle): a held lease and its resource, null when the texture cannot be leased.
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_hud_lease(texture: *mut c_void, idle: bool, resource: *mut *mut c_void) -> *mut c_void {
    let (mut handle, mut held) = (0u32, core::ptr::null_mut());
    // SAFETY: the engine's texture; two locals.
    if !unsafe { cvr_gpu::texture_lease::cvr_hud_texture_lease(texture, idle, &mut handle, &mut held) } {
        // SAFETY: the caller's output.
        unsafe { *resource = core::ptr::null_mut() };
        return core::ptr::null_mut();
    }
    // SAFETY: the caller's output.
    unsafe { *resource = held };
    Box::into_raw(Box::new(TextureLease { handle, resource: held })).cast()
}
/// A lease handed back (the HUD natives' sprites).
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_hud_lease_release(lease: *mut c_void) {
    if !lease.is_null() {
        // SAFETY: a lease cvr_cpp_hud_lease made.
        drop(unsafe { Box::from_raw(lease as *mut TextureLease) });
    }
}
/// A lease handed back (the HUD panel's and the optics probe's).
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_texture_lease_release(lease: *mut c_void) {
    // SAFETY: as above.
    unsafe { cvr_cpp_hud_lease_release(lease) }
}
/// cvr::optics::probe::Request(lease): takes the lease; false (and the lease let go) without one or its resource.
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_optics_probe_request(lease: *mut c_void) -> bool {
    if lease.is_null() {
        return false;
    }
    // SAFETY: a lease cvr_cpp_hud_lease made, owned from here.
    let held = unsafe { Box::from_raw(lease as *mut TextureLease) };
    if held.resource.is_null() {
        return false;
    }
    let (resource, handle) = (held.resource, held.handle);
    // SAFETY: the probe takes the lease (and gives it back through cvr_cpp_texture_lease_release).
    unsafe { cvr_gpu::optics_probe::cvr_optics_probe_request(Box::into_raw(held).cast(), resource, handle) }
}

// ---- the services that reached Rust through a C++ forward (B461): Main.cpp's installs, CharacterRigRust.cpp's and
// InputServices.cpp's ladder, swimming and cyberware chord, CameraServices.cpp's pose identity and VrMenu.cpp's UI
// recording request, straight to the Rust they ended in
mod forwarded {
    use core::ffi::{c_char, c_void};
    unsafe extern "C" {
        // cvr-rig
        pub safe fn cvr_ladder_active() -> bool;
        pub safe fn cvr_swimming_active() -> bool;
        pub safe fn cvr_cyberware_chord_dispatch();
        pub safe fn cvr_pose_next_identity() -> u64;
        pub fn cvr_ladder_update_input(controllers: *const c_void);
        // cvr-ffi
        pub safe fn cvr_input_config_apply();
        pub fn cvr_tweaks_install(handle: *mut c_void, sdk: *const c_void);
        pub fn cvr_archive_install(handle: *mut c_void, sdk: *const c_void);
        pub fn cvr_archive_add_fixup(type_name: *const c_char, fix: Option<unsafe extern "C" fn(*mut c_void)>);
    }
}
/// The stereo module's log for the Rust half (formerly src/Stereo/Profiler.cpp's, through include/Utils/StereoLog.hpp's
/// cvr::log): the live switch CyberpunkVR_StereoLog, a cap of 200 lines a second (the next window says how many were
/// suppressed), the text cut to 1023 bytes and tagged [stereo].
///
/// # Safety
/// `text` is a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_stereo_log(text: *const c_char) {
    // SAFETY: the exported switch, written by the overlay.
    if unsafe { core::ptr::read_volatile(&raw const crate::declared_moved_exports::CyberpunkVR_StereoLog) } == 0
        || !stereo_log_rate_ok()
        || text.is_null()
    {
        return;
    }
    // SAFETY: the caller's string.
    let bytes = unsafe { core::ffi::CStr::from_ptr(text) }.to_bytes();
    let bytes = &bytes[..bytes.len().min(1023)];
    cvr_host::log::write(&format!("[stereo] {}\n", String::from_utf8_lossy(bytes)));
}
fn stereo_log_rate_ok() -> bool {
    struct Window {
        start: u64,
        lines: u32,
        announced: bool,
    }
    static WINDOW: std::sync::Mutex<Window> = std::sync::Mutex::new(Window { start: 0, lines: 0, announced: false });
    let mut window = WINDOW.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    // SAFETY: plain Win32 call.
    let now = unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() };
    if now.wrapping_sub(window.start) >= 1000 {
        if window.announced {
            cvr_host::log::write(&format!(
                "[stereo] (log rate cap hit, {} lines suppressed in the last window)\n",
                window.lines.saturating_sub(200)
            ));
            window.announced = false;
        }
        window.start = now;
        window.lines = 0;
    }
    window.lines = window.lines.wrapping_add(1);
    if window.lines > 200 {
        window.announced = true;
        return false;
    }
    true
}
/// VRIK_ReadCurrentBodyFrame (formerly src/Hooks/RoomscaleRust.cpp's forward): the body's current world frame.
///
/// # Safety
/// The three out-pointers are writable (a position, a quaternion, a yaw).
#[no_mangle]
pub unsafe extern "C" fn VRIK_ReadCurrentBodyFrame(p: *mut f32, q: *mut f32, tracking_yaw: *mut f32) -> bool {
    unsafe extern "C" {
        fn cvr_body_frame_read(p: *mut f32, q: *mut f32, tracking_yaw: *mut f32) -> bool;
    }
    // SAFETY: the caller's out-pointers, passed on.
    unsafe { cvr_body_frame_read(p, q, tracking_yaw) }
}
/// VRIK_ReadNativeTransformSnapshot (formerly src/Hooks/RoomscaleRust.cpp's forward): the native transform pair.
///
/// # Safety
/// `out` is a writable VrikTransformSnapshot.
#[no_mangle]
pub unsafe extern "C" fn VRIK_ReadNativeTransformSnapshot(out: *mut c_void) -> bool {
    unsafe extern "C" {
        fn cvr_body_native_pair_read(out: *mut c_void) -> bool;
    }
    // SAFETY: the caller's snapshot, passed on.
    unsafe { cvr_body_native_pair_read(out) }
}
/// cvr::ladder::Active().
#[no_mangle]
pub extern "C" fn cvr_cpp_ladder_active() -> bool {
    forwarded::cvr_ladder_active()
}
/// cvr::swimming::Active().
#[no_mangle]
pub extern "C" fn cvr_cpp_swimming_active() -> bool {
    forwarded::cvr_swimming_active()
}
/// cvr::ladder::UpdateInput(*controllers).
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_ladder_update_input(controllers: *const c_void) {
    // SAFETY: the caller's controller state.
    unsafe { forwarded::cvr_ladder_update_input(controllers) }
}
/// cvr::input::DispatchCyberwareChord().
#[no_mangle]
pub extern "C" fn cvr_cpp_cyberware_chord_dispatch() {
    forwarded::cvr_cyberware_chord_dispatch()
}
/// cvr::camera::NextPoseIdentity().
#[no_mangle]
pub extern "C" fn cvr_cpp_next_pose_identity() -> u64 {
    forwarded::cvr_pose_next_identity()
}
/// cvr::uirec::Request(page).
#[no_mangle]
pub extern "C" fn cvr_cpp_uirec_request(page: i32) {
    crate::ui_rec::cvr_uirec_request(page)
}
/// cvr::input_config::Apply(): the port's input bindings, before the engine reads its input configuration.
#[no_mangle]
pub extern "C" fn cvr_cpp_input_config_apply() {
    forwarded::cvr_input_config_apply()
}
/// cvr::tweak_records::Install(handle, sdk): the port's TweakDB records, applied as the engine loads its TweakDB.
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_tweak_records_install(handle: *mut c_void, sdk: *const c_void) {
    // SAFETY: RED4ext's plugin handle and SDK.
    unsafe { forwarded::cvr_tweaks_install(handle, sdk) }
}
/// cvr::archive::Install(handle, sdk): the port's archive, mounted from the plugin's folder, and its manifest.
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_archive_install(handle: *mut c_void, sdk: *const c_void) {
    // SAFETY: as above.
    unsafe { forwarded::cvr_archive_install(handle, sdk) }
}
/// cvr::lobby::notice::Install(): the boot's notice in the game's language, set as the game's ink settings load.
#[no_mangle]
pub extern "C" fn cvr_cpp_lobby_notice_install() {
    // SAFETY: a terminated type name and the notice's fixup of the resource's signature.
    unsafe { forwarded::cvr_archive_add_fixup(c"inkGameSettingsResource".as_ptr(), Some(crate::notice::cvr_notice_set_boot_video)) }
}

// ---- the C++ forwards into Rust (B463): each defined here under the forward's own symbol, the C++ callers unchanged
mod cpp_forwards {
#![allow(non_snake_case, clippy::missing_safety_doc, clippy::unnecessary_cast, clippy::useless_conversion)]
use core::ffi::c_void;
#[allow(clashing_extern_declarations, improper_ctypes)]
unsafe extern "C" {
    fn cvr_apply_settings_resolution_override(a0: usize);
    fn cvr_census_buf_note(a0: *mut c_void, a1: u64, a2: u64);
    fn cvr_census_filled_cpu_for_va(a0: u64, a1: u64) -> *const c_void;
    fn cvr_device_cam_active() -> bool;
    fn cvr_dh_descriptor_hooks_ready(a0: *mut c_void) -> bool;
    fn cvr_dh_install_descriptor_hooks(a0: *mut c_void);
    fn cvr_engine_stereo_enabled() -> bool;
    fn cvr_engine_stereo_loading_shown() -> bool;
    fn cvr_engine_stereo_rendering() -> bool;
    fn cvr_ensure_live_control_file_exists();
    fn cvr_forced_render_size(a0: bool) -> u32;
    fn cvr_foveated_dlss_reset_list(a0: *mut c_void);
    fn cvr_foveated_dlss_shutdown();
    fn cvr_framegen_reset_list(a0: *mut c_void);
    fn cvr_game_build_supported() -> bool;
    fn cvr_get_shot_shared() -> *mut c_void;
    fn cvr_grading_optics_material_snapshot(a0: u32, a1: *const c_void);
    fn cvr_image_pose_reset_list(a0: *mut c_void);
    fn cvr_init_runtime_paths();
    fn cvr_ladder_active() -> bool;
    fn cvr_ladder_debug_value(a0: i32) -> f32;
    fn cvr_ladder_native_action() -> f32;
    fn cvr_ladder_reset_input();
    fn cvr_ladder_update_input(a0: *const c_void);
    fn cvr_list_audit_list_reset(a0: *mut c_void, a1: *mut c_void);
    fn cvr_menu_lobby_active() -> bool;
    fn cvr_menu_surface_reset_list(a0: *mut c_void);
    fn cvr_mirror_submit_copy(a0: *mut c_void);
    fn cvr_ndt_describe_current(a0: *mut c_void) -> bool;
    fn cvr_optics_probe_capture(a0: *mut c_void, a1: *mut c_void);
    fn cvr_patch_fast_disarm();
    fn cvr_poll_live_controls();
    fn cvr_rp_observe_fog_constants(a0: u32, a1: *const c_void);
    fn cvr_stamp_device_cam();
    fn cvr_swimming_active() -> bool;
    fn cvr_swimming_debug_value(a0: i32) -> f32;
    fn cvr_swimming_native_action(a0: i32) -> f32;
    fn cvr_swimming_reset_input();
    fn cvr_trackers_locate_count() -> u64;
    fn cvr_trackers_on_session_destroying();
    fn cvr_trackers_read(a0: i32, a1: *mut c_void) -> bool;
    fn cvr_trackers_read_hand(a0: i32, a1: *mut c_void) -> bool;
    fn cvr_trackers_read_head_stage(a0: *mut c_void) -> bool;
    fn cvr_trackers_running() -> bool;
    fn cvr_vrik_read_transform_snapshot(a0: *mut c_void) -> bool;
    fn cvr_worker_thread_main() -> u32;
    fn cvr_xr_remove_simulator_recenter();
}
#[allow(clashing_extern_declarations, improper_ctypes)]
unsafe extern "C-unwind" {
    fn cvr_fbt_switch(a0: u32) -> bool;
    fn cvr_sfo_last_boundary_failure() -> u32;
    fn cvr_srt_active() -> bool;
    fn cvr_upload_cpu_for_va(a0: u64, a1: u64) -> *const c_void;
}
/// Fbt/Game/Body.cpp: `bool LegsEnabled() { return cvr_fbt_switch(0); }`
#[export_name = "?LegsEnabled@game@fbt@cvr@@YA_NXZ"]
pub unsafe extern "C" fn fwd_cvr__fbt__game__legsenabled() -> bool {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_fbt_switch(0 as u32) }
}
/// Fbt/Game/Body.cpp: `bool SpineEnabled() { return cvr_fbt_switch(1); }`
#[export_name = "?SpineEnabled@game@fbt@cvr@@YA_NXZ"]
pub unsafe extern "C" fn fwd_cvr__fbt__game__spineenabled() -> bool {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_fbt_switch(1 as u32) }
}
/// Fbt/Game/Body.cpp: `bool ArmsEnabled() { return cvr_fbt_switch(2); }`
#[export_name = "?ArmsEnabled@game@fbt@cvr@@YA_NXZ"]
pub unsafe extern "C" fn fwd_cvr__fbt__game__armsenabled() -> bool {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_fbt_switch(2 as u32) }
}
/// Stereo/Grading.cpp: `void optics_material_snapshot(uint32_t size, const void* source) { cvr_grading_optics_material_snapshot(size, source); }`
#[export_name = "?optics_material_snapshot@detail@cvr@@YAXIPEBX@Z"]
pub unsafe extern "C" fn fwd_cvr__detail__optics_material_snapshot(p_size: u32, p_source: *mut c_void) {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_grading_optics_material_snapshot(p_size as u32, p_source as *const c_void); }
}
/// Render/NativeSubmissionFrontier.cpp: `uint32_t LastBoundaryFailure()noexcept{return cvr_sfo_last_boundary_failure();}`
#[export_name = "?LastBoundaryFailure@submission_frontier@stereo@cvr@@YAIXZ"]
pub unsafe extern "C" fn fwd_cvr__stereo__submission_frontier__lastboundaryfailure() -> u32 {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_sfo_last_boundary_failure() }
}
/// Core/LiveControlsPoll.cpp: `void PollLiveControls() { cvr_poll_live_controls(); }`
#[export_name = "?PollLiveControls@@YAXXZ"]
pub unsafe extern "C" fn fwd_polllivecontrols() {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_poll_live_controls(); }
}
/// Stereo/NativeDefinitionTracking.cpp: `bool DescribeCurrent(Identity& output)noexcept{return cvr_ndt_describe_current(&output);}`
#[export_name = "?DescribeCurrent@scene_child@stereo@cvr@@YA_NAEAUIdentity@123@@Z"]
pub unsafe extern "C" fn fwd_cvr__stereo__scene_child__describecurrent(p_output: *mut c_void) -> bool {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_ndt_describe_current(p_output as *mut c_void) }
}
/// Natives/CameraPair.cpp: `bool VRIK_ReadTransformSnapshot(VrikTransformSnapshot* out) { return cvr_vrik_read_transform_snapshot(out); }`
#[no_mangle]
pub unsafe extern "C" fn VRIK_ReadTransformSnapshot(p_out: *mut c_void) -> bool {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_vrik_read_transform_snapshot(p_out as *mut c_void) }
}
/// Core/VrCore.cpp: `UINT GetForcedRenderWidthValue() { return cvr_forced_render_size(true); }`
#[export_name = "?GetForcedRenderWidthValue@@YAIXZ"]
pub unsafe extern "C" fn fwd_getforcedrenderwidthvalue() -> u32 {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_forced_render_size(true) }
}
/// Core/VrCore.cpp: `UINT GetForcedRenderHeightValue() { return cvr_forced_render_size(false); }`
#[export_name = "?GetForcedRenderHeightValue@@YAIXZ"]
pub unsafe extern "C" fn fwd_getforcedrenderheightvalue() -> u32 {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_forced_render_size(false) }
}
/// Core/VrCore.cpp: `void ApplySettingsResolutionOverride(uintptr_t settingsPtr) { cvr_apply_settings_resolution_override(settingsPtr); }`
#[export_name = "?ApplySettingsResolutionOverride@@YAX_K@Z"]
pub unsafe extern "C" fn fwd_applysettingsresolutionoverride(p_settingsPtr: u64) {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_apply_settings_resolution_override(p_settingsPtr as usize); }
}
/// Core/VrCore.cpp: `void PatchFastDisarm() { cvr_patch_fast_disarm(); }`
#[export_name = "?PatchFastDisarm@@YAXXZ"]
pub unsafe extern "C" fn fwd_patchfastdisarm() {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_patch_fast_disarm(); }
}
/// Core/VrCore.cpp: `float* GetShotShared() { return cvr_get_shot_shared(); }`
#[export_name = "?GetShotShared@@YAPEAMXZ"]
pub unsafe extern "C" fn fwd_getshotshared() -> *mut c_void {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_get_shot_shared() }
}
/// Core/VrCore.cpp: `void StampDeviceCam() { cvr_stamp_device_cam(); }`
#[export_name = "?StampDeviceCam@@YAXXZ"]
pub unsafe extern "C" fn fwd_stampdevicecam() {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_stamp_device_cam(); }
}
/// Core/VrCore.cpp: `bool DeviceCamActive() { return cvr_device_cam_active(); }`
#[export_name = "?DeviceCamActive@@YA_NXZ"]
pub unsafe extern "C" fn fwd_devicecamactive() -> bool {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_device_cam_active() }
}
/// Stereo/CommandListCensus.cpp: `void buf_note(ID3D12Resource* res, uint64_t va, uint64_t size) { cvr_census_buf_note(res, va, size); }`
#[export_name = "?buf_note@detail@cvr@@YAXPEAUID3D12Resource@@_K1@Z"]
pub unsafe extern "C" fn fwd_cvr__detail__buf_note(p_res: *mut c_void, p_va: u64, p_size: u64) {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_census_buf_note(p_res as *mut c_void, p_va as u64, p_size as u64); }
}
/// Stereo/CommandListCensus.cpp: `const uint8_t* filled_cpu_for_va(uint64_t va, uint64_t need) { return cvr_census_filled_cpu_for_va(va, need); }`
#[export_name = "?filled_cpu_for_va@detail@cvr@@YAPEBE_K0@Z"]
pub unsafe extern "C" fn fwd_cvr__detail__filled_cpu_for_va(p_va: u64, p_need: u64) -> *mut c_void {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_census_filled_cpu_for_va(p_va as u64, p_need as u64) as *mut c_void }
}
/// Core/LauncherConfig.cpp: `void InitRuntimePaths() { cvr_init_runtime_paths(); }`
#[export_name = "?InitRuntimePaths@@YAXXZ"]
pub unsafe extern "C" fn fwd_initruntimepaths() {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_init_runtime_paths(); }
}
/// Core/LauncherConfig.cpp: `void EnsureLiveControlFileExists() { cvr_ensure_live_control_file_exists(); }`
#[export_name = "?EnsureLiveControlFileExists@@YAXXZ"]
pub unsafe extern "C" fn fwd_ensurelivecontrolfileexists() {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_ensure_live_control_file_exists(); }
}
/// Hooks/DepthCapture.cpp: `bool DescriptorMutationHooksReady(ID3D12Device* device){return cvr_dh_descriptor_hooks_ready(device);}`
#[export_name = "?DescriptorMutationHooksReady@@YA_NPEAUID3D12Device@@@Z"]
pub unsafe extern "C" fn fwd_descriptormutationhooksready(p_device: *mut c_void) -> bool {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_dh_descriptor_hooks_ready(p_device as *mut c_void) }
}
/// Hooks/DepthCapture.cpp: `void InstallDepthCaptureHooks(ID3D12Device* device) { cvr_dh_install_descriptor_hooks(device); }`
#[export_name = "?InstallDepthCaptureHooks@@YAXPEAUID3D12Device@@@Z"]
pub unsafe extern "C" fn fwd_installdepthcapturehooks(p_device: *mut c_void) {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_dh_install_descriptor_hooks(p_device as *mut c_void); }
}
/// Stereo/RenderParity.cpp: `void ObserveFogConstants(uint32_t size,const void* source){cvr_rp_observe_fog_constants(size,source);}`
#[export_name = "?ObserveFogConstants@stereo@cvr@@YAXIPEBX@Z"]
pub unsafe extern "C" fn fwd_cvr__stereo__observefogconstants(p_size: u32, p_source: *mut c_void) {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_rp_observe_fog_constants(p_size as u32, p_source as *const c_void); }
}
/// Framegen/Inputs.cpp: `void ResetList(ID3D12GraphicsCommandList* list) { cvr_framegen_reset_list(list); }`
#[export_name = "?ResetList@framegen@cvr@@YAXPEAUID3D12GraphicsCommandList@@@Z"]
pub unsafe extern "C" fn fwd_cvr__framegen__resetlist(p_list: *mut c_void) {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_framegen_reset_list(p_list as *mut c_void); }
}
/// Fbt/Game/Trackers.cpp: `void OnSessionDestroying() { cvr_trackers_on_session_destroying(); }`
#[export_name = "?OnSessionDestroying@trackers@cvr@@YAXXZ"]
pub unsafe extern "C" fn fwd_cvr__trackers__onsessiondestroying() {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_trackers_on_session_destroying(); }
}
/// Fbt/Game/Trackers.cpp: `bool Read(Role role, Pose* out) { return cvr_trackers_read(role, out); }`
#[export_name = "?Read@trackers@cvr@@YA_NW4Role@12@PEAUPose@12@@Z"]
pub unsafe extern "C" fn fwd_cvr__trackers__read(p_role: i32, p_out: *mut c_void) -> bool {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_trackers_read(p_role as i32, p_out as *mut c_void) }
}
/// Fbt/Game/Trackers.cpp: `bool ReadHeadStage(XrPosef* out) { return cvr_trackers_read_head_stage(out); }`
#[export_name = "?ReadHeadStage@trackers@cvr@@YA_NPEAUXrPosef@@@Z"]
pub unsafe extern "C" fn fwd_cvr__trackers__readheadstage(p_out: *mut c_void) -> bool {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_trackers_read_head_stage(p_out as *mut c_void) }
}
/// Fbt/Game/Trackers.cpp: `unsigned long long LocateCount() { return cvr_trackers_locate_count(); }`
#[export_name = "?LocateCount@trackers@cvr@@YA_KXZ"]
pub unsafe extern "C" fn fwd_cvr__trackers__locatecount() -> u64 {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_trackers_locate_count() }
}
/// Fbt/Game/Trackers.cpp: `bool ReadHand(int hand, Pose* out) { return cvr_trackers_read_hand(hand, out); }`
#[export_name = "?ReadHand@trackers@cvr@@YA_NHPEAUPose@12@@Z"]
pub unsafe extern "C" fn fwd_cvr__trackers__readhand(p_hand: i32, p_out: *mut c_void) -> bool {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_trackers_read_hand(p_hand as i32, p_out as *mut c_void) }
}
/// Fbt/Game/Trackers.cpp: `bool Running() { return cvr_trackers_running(); }`
#[export_name = "?Running@trackers@cvr@@YA_NXZ"]
pub unsafe extern "C" fn fwd_cvr__trackers__running() -> bool {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_trackers_running() }
}
/// Hooks/WaterLadderRust.cpp: `bool Active() { return cvr_ladder_active(); }`
#[export_name = "?Active@ladder@cvr@@YA_NXZ"]
pub unsafe extern "C" fn fwd_cvr__ladder__active() -> bool {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_ladder_active() }
}
/// Hooks/WaterLadderRust.cpp: `void UpdateInput(const VRControllerState& controllers) { cvr_ladder_update_input(&controllers); }`
#[export_name = "?UpdateInput@ladder@cvr@@YAXAEBUVRControllerState@@@Z"]
pub unsafe extern "C" fn fwd_cvr__ladder__updateinput(p_controllers: *mut c_void) {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_ladder_update_input(p_controllers as *const c_void); }
}
/// Hooks/WaterLadderRust.cpp: `void ResetInput() { cvr_ladder_reset_input(); }`
#[export_name = "?ResetInput@ladder@cvr@@YAXXZ"]
pub unsafe extern "C" fn fwd_cvr__ladder__resetinput() {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_ladder_reset_input(); }
}
/// Hooks/WaterLadderRust.cpp: `float NativeAction() { return cvr_ladder_native_action(); }`
#[export_name = "?NativeAction@ladder@cvr@@YAMXZ"]
pub unsafe extern "C" fn fwd_cvr__ladder__nativeaction() -> f32 {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_ladder_native_action() }
}
/// Hooks/WaterLadderRust.cpp: `float DebugValue(int index) { return cvr_ladder_debug_value(index); }`
#[export_name = "?DebugValue@ladder@cvr@@YAMH@Z"]
pub unsafe extern "C" fn fwd_cvr__ladder__debugvalue(p_index: i32) -> f32 {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_ladder_debug_value(p_index as i32) }
}
/// Hooks/WaterLadderRust.cpp: `bool Active() { return cvr_swimming_active(); }`
#[export_name = "?Active@swimming@cvr@@YA_NXZ"]
pub unsafe extern "C" fn fwd_cvr__swimming__active() -> bool {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_swimming_active() }
}
/// Hooks/WaterLadderRust.cpp: `float NativeAction(int action) { return cvr_swimming_native_action(action); }`
#[export_name = "?NativeAction@swimming@cvr@@YAMH@Z"]
pub unsafe extern "C" fn fwd_cvr__swimming__nativeaction(p_action: i32) -> f32 {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_swimming_native_action(p_action as i32) }
}
/// Hooks/WaterLadderRust.cpp: `void ResetInput() { cvr_swimming_reset_input(); }`
#[export_name = "?ResetInput@swimming@cvr@@YAXXZ"]
pub unsafe extern "C" fn fwd_cvr__swimming__resetinput() {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_swimming_reset_input(); }
}
/// Hooks/WaterLadderRust.cpp: `float DebugValue(int index) { return cvr_swimming_debug_value(index); }`
#[export_name = "?DebugValue@swimming@cvr@@YAMH@Z"]
pub unsafe extern "C" fn fwd_cvr__swimming__debugvalue(p_index: i32) -> f32 {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_swimming_debug_value(p_index as i32) }
}
/// Render/StageReplay.cpp: `bool Active(){return cvr_srt_active();}`
#[export_name = "?Active@stage_replay@stereo@cvr@@YA_NXZ"]
pub unsafe extern "C" fn fwd_cvr__stereo__stage_replay__active() -> bool {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_srt_active() }
}
/// Core/WorkerThread.cpp: `DWORD WINAPI WorkerThread(LPVOID) { return cvr_worker_thread_main(); }`
#[export_name = "?WorkerThread@@YAKPEAX@Z"]
pub unsafe extern "C" fn fwd_workerthread(_p_LPVOID: *mut c_void) -> u32 {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_worker_thread_main() }
}
/// Stereo/ViewConstants.cpp: `const uint8_t* upload_cpu_for_va(uint64_t va, uint64_t need) { return cvr_upload_cpu_for_va(va, need); }`
#[export_name = "?upload_cpu_for_va@detail@cvr@@YAPEBE_K0@Z"]
pub unsafe extern "C" fn fwd_cvr__detail__upload_cpu_for_va(p_va: u64, p_need: u64) -> *mut c_void {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_upload_cpu_for_va(p_va as u64, p_need as u64) as *mut c_void }
}
/// Stereo/Mirror.cpp: `void d12_submit_mirror_copy(ID3D12CommandQueue* queue) { cvr_mirror_submit_copy(queue); }`
#[export_name = "?d12_submit_mirror_copy@detail@cvr@@YAXPEAUID3D12CommandQueue@@@Z"]
pub unsafe extern "C" fn fwd_cvr__detail__d12_submit_mirror_copy(p_queue: *mut c_void) {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_mirror_submit_copy(p_queue as *mut c_void); }
}
/// Camera/ImagePoseIdentity.cpp: `void ResetImagePoseList(ID3D12GraphicsCommandList* list) { cvr_image_pose_reset_list(list); }`
#[export_name = "?ResetImagePoseList@camera@cvr@@YAXPEAUID3D12GraphicsCommandList@@@Z"]
pub unsafe extern "C" fn fwd_cvr__camera__resetimageposelist(p_list: *mut c_void) {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_image_pose_reset_list(p_list as *mut c_void); }
}
/// Stereo/EngineStereo.cpp: `bool Enabled() { return cvr_engine_stereo_enabled(); }`
#[export_name = "?Enabled@engine_stereo@cvr@@YA_NXZ"]
pub unsafe extern "C" fn fwd_cvr__engine_stereo__enabled() -> bool {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_engine_stereo_enabled() }
}
/// Stereo/EngineStereo.cpp: `bool LoadingShown() { return cvr_engine_stereo_loading_shown(); }`
#[export_name = "?LoadingShown@engine_stereo@cvr@@YA_NXZ"]
pub unsafe extern "C" fn fwd_cvr__engine_stereo__loadingshown() -> bool {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_engine_stereo_loading_shown() }
}
/// Stereo/EngineStereo.cpp: `bool Rendering() { return cvr_engine_stereo_rendering(); }`
#[export_name = "?Rendering@engine_stereo@cvr@@YA_NXZ"]
pub unsafe extern "C" fn fwd_cvr__engine_stereo__rendering() -> bool {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_engine_stereo_rendering() }
}
/// Render/FoveatedDlss.cpp: `void ResetList(ID3D12GraphicsCommandList* list) { cvr_foveated_dlss_reset_list(list); }`
#[export_name = "?ResetList@foveated@cvr@@YAXPEAUID3D12GraphicsCommandList@@@Z"]
pub unsafe extern "C" fn fwd_cvr__foveated__resetlist(p_list: *mut c_void) {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_foveated_dlss_reset_list(p_list as *mut c_void); }
}
/// Render/FoveatedDlss.cpp: `void Shutdown() { cvr_foveated_dlss_shutdown(); }`
#[export_name = "?Shutdown@foveated@cvr@@YAXXZ"]
pub unsafe extern "C" fn fwd_cvr__foveated__shutdown() {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_foveated_dlss_shutdown(); }
}
/// Natives/MenuSurface.cpp: `void ResetList(ID3D12GraphicsCommandList* list) { cvr_menu_surface_reset_list(list); }`
#[export_name = "?ResetList@menu_surface@cvr@@YAXPEAUID3D12GraphicsCommandList@@@Z"]
pub unsafe extern "C" fn fwd_cvr__menu_surface__resetlist(p_list: *mut c_void) {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_menu_surface_reset_list(p_list as *mut c_void); }
}
/// Lobby/MenuLobby.cpp: `bool Active() { return cvr_menu_lobby_active(); }`
#[export_name = "?Active@lobby@cvr@@YA_NXZ"]
pub unsafe extern "C" fn fwd_cvr__lobby__active() -> bool {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_menu_lobby_active() }
}
/// Hooks/ListAudit.cpp: `void ListReset(ID3D12GraphicsCommandList* list, ID3D12CommandAllocator* allocator) { cvr_list_audit_list_reset(list, allocator); }`
#[export_name = "?ListReset@list_audit@cvr@@YAXPEAUID3D12GraphicsCommandList@@PEAUID3D12CommandAllocator@@@Z"]
pub unsafe extern "C" fn fwd_cvr__list_audit__listreset(p_list: *mut c_void, p_allocator: *mut c_void) {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_list_audit_list_reset(p_list as *mut c_void, p_allocator as *mut c_void); }
}
/// Render/OpticsProbe.cpp: `void Capture(ID3D12Device* device, ID3D12CommandQueue* queue) { cvr_optics_probe_capture(device, queue); }`
#[export_name = "?Capture@probe@optics@cvr@@YAXPEAUID3D12Device@@PEAUID3D12CommandQueue@@@Z"]
pub unsafe extern "C" fn fwd_cvr__optics__probe__capture(p_device: *mut c_void, p_queue: *mut c_void) {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_optics_probe_capture(p_device as *mut c_void, p_queue as *mut c_void); }
}
/// Core/GameBuild.cpp: `bool Supported() { return cvr_game_build_supported(); }`
#[export_name = "?Supported@game_build@cvr@@YA_NXZ"]
pub unsafe extern "C" fn fwd_cvr__game_build__supported() -> bool {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_game_build_supported() }
}
/// Runtimes/SimulatorRecenter.cpp: `void RemoveSimulatorRecenterHook() { cvr_xr_remove_simulator_recenter(); }`
#[export_name = "?RemoveSimulatorRecenterHook@@YAXXZ"]
pub unsafe extern "C" fn fwd_removesimulatorrecenterhook() {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_xr_remove_simulator_recenter(); }
}
}

// ---- the C++ forwards into Rust (B463): each defined here under the forward's own symbol, the C++ callers unchanged
mod cpp_forwards_b463 {
#![allow(non_snake_case, clippy::missing_safety_doc, clippy::unnecessary_cast, clippy::useless_conversion)]
use core::ffi::c_void;
#[allow(clashing_extern_declarations, improper_ctypes)]
unsafe extern "C" {
    fn cvr_aftermath_observe_resource_creation(a0: *mut c_void, a1: *const c_void);
    fn cvr_hidden_area_dsv_created(a0: *mut c_void, a1: *const c_void, a2: usize);
    fn cvr_hud_register_rtv(a0: *mut c_void, a1: *const c_void, a2: usize);
    fn cvr_image_pose_commit(a0: *mut c_void, a1: u32, a2: *mut c_void);
    fn cvr_image_pose_record(a0: *mut c_void, a1: *mut c_void, a2: *const c_void, a3: bool);
    fn cvr_menu_surface_barriers(a0: *mut c_void, a1: u32, a2: *const c_void);
    fn cvr_optics_probe_stage_barriers(a0: *mut c_void, a1: u32, a2: *const c_void);
    fn cvr_overlay_set_device_and_queue(a0: *mut c_void, a1: *mut c_void);
    fn cvr_single_pass_graphics_created(a0: *mut c_void, a1: *const c_void, a2: *mut c_void, a3: *mut c_void);
    fn cvr_single_pass_root_created(a0: *mut c_void, a1: *mut c_void, a2: *const c_void, a3: usize);
    fn cvr_single_pass_stream_created(a0: *mut c_void, a1: *const c_void, a2: *mut c_void, a3: *mut c_void);
}
/// Camera/ImagePoseIdentity.cpp: `void RecordImagePose(ID3D12GraphicsCommandList* list, ID3D12Resource* resource, const PoseIdentity& pose, bool stable) { cvr_image_pose_record(list, r`
#[export_name = "?RecordImagePose@camera@cvr@@YAXPEAUID3D12GraphicsCommandList@@PEAUID3D12Resource@@AEBUPoseIdentity@12@_N@Z"]
pub unsafe extern "C" fn fwd_cvr__camera__recordimagepose(p_list: *mut c_void, p_resource: *mut c_void, p_pose: *mut c_void, p_stable: bool) {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_image_pose_record(p_list as *mut c_void, p_resource as *mut c_void, p_pose as *const c_void, p_stable); }
}
/// Camera/ImagePoseIdentity.cpp: `void CommitImagePoseLists(ID3D12CommandQueue* queue, UINT count, ID3D12CommandList* const* lists) { cvr_image_pose_commit(queue, count, lists); }`
#[export_name = "?CommitImagePoseLists@camera@cvr@@YAXPEAUID3D12CommandQueue@@IPEBQEAUID3D12CommandList@@@Z"]
pub unsafe extern "C" fn fwd_cvr__camera__commitimageposelists(p_queue: *mut c_void, p_count: u32, p_lists: *mut c_void) {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_image_pose_commit(p_queue as *mut c_void, p_count as u32, p_lists as *mut c_void); }
}
/// Hooks/AftermathDiagnostics.cpp: `void cvr::diagnostics::ObserveResourceCreation(ID3D12Resource* resource, const D3D12_RESOURCE_DESC& desc) { cvr_aftermath_observe_resource_creation(re`
#[export_name = "?ObserveResourceCreation@diagnostics@cvr@@YAXPEAUID3D12Resource@@AEBUD3D12_RESOURCE_DESC@@@Z"]
pub unsafe extern "C" fn fwd_cvr__diagnostics__observeresourcecreation(p_resource: *mut c_void, p_desc: *mut c_void) {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_aftermath_observe_resource_creation(p_resource as *mut c_void, p_desc as *const c_void); }
}
/// Natives/MenuSurface.cpp: `void Barriers(ID3D12GraphicsCommandList* list,UINT count,const D3D12_RESOURCE_BARRIER* barriers) { cvr_menu_surface_barriers(list,count,barriers); }`
#[export_name = "?Barriers@menu_surface@cvr@@YAXPEAUID3D12GraphicsCommandList@@IPEBUD3D12_RESOURCE_BARRIER@@@Z"]
pub unsafe extern "C" fn fwd_cvr__menu_surface__barriers(p_list: *mut c_void, p_count: u32, p_barriers: *mut c_void) {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_menu_surface_barriers(p_list as *mut c_void, p_count as u32, p_barriers as *const c_void); }
}
/// Overlay/ImGuiOverlay.cpp: `void OverlaySetDeviceAndQueue(ID3D12Device* device, ID3D12CommandQueue* queue) { cvr_overlay_set_device_and_queue(device, queue); }`
#[export_name = "?OverlaySetDeviceAndQueue@@YAXPEAUID3D12Device@@PEAUID3D12CommandQueue@@@Z"]
pub unsafe extern "C" fn fwd_overlaysetdeviceandqueue(p_device: *mut c_void, p_queue: *mut c_void) {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_overlay_set_device_and_queue(p_device as *mut c_void, p_queue as *mut c_void); }
}
/// Render/HiddenArea.cpp: `void DsvCreated(ID3D12Resource* resource,const D3D12_DEPTH_STENCIL_VIEW_DESC* desc,D3D12_CPU_DESCRIPTOR_HANDLE handle){ cvr_hidden_area_dsv_created(re`
#[export_name = "?DsvCreated@hidden_area@cvr@@YAXPEAUID3D12Resource@@PEBUD3D12_DEPTH_STENCIL_VIEW_DESC@@UD3D12_CPU_DESCRIPTOR_HANDLE@@@Z"]
pub unsafe extern "C" fn fwd_cvr__hidden_area__dsvcreated(p_resource: *mut c_void, p_desc: *mut c_void, p_handle: u64) {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_hidden_area_dsv_created(p_resource as *mut c_void, p_desc as *const c_void, p_handle as usize); }
}
/// Render/OpticsProbe.cpp: `void StageBarriers(ID3D12GraphicsCommandList* list, UINT count, const D3D12_RESOURCE_BARRIER* barriers) { cvr_optics_probe_stage_barriers(list, count,`
#[export_name = "?StageBarriers@probe@optics@cvr@@YAXPEAUID3D12GraphicsCommandList@@IPEBUD3D12_RESOURCE_BARRIER@@@Z"]
pub unsafe extern "C" fn fwd_cvr__optics__probe__stagebarriers(p_list: *mut c_void, p_count: u32, p_barriers: *mut c_void) {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_optics_probe_stage_barriers(p_list as *mut c_void, p_count as u32, p_barriers as *const c_void); }
}
/// Render/SinglePassPipelines.cpp: `void RootCreated(ID3D12Device* device, ID3D12RootSignature* root, const void* blob, size_t bytes) { cvr_single_pass_root_created(device, root, blob, b`
#[export_name = "?RootCreated@single_pass@cvr@@YAXPEAUID3D12Device@@PEAUID3D12RootSignature@@PEBX_K@Z"]
pub unsafe extern "C" fn fwd_cvr__single_pass__rootcreated(p_device: *mut c_void, p_root: *mut c_void, p_blob: *mut c_void, p_bytes: u64) {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_single_pass_root_created(p_device as *mut c_void, p_root as *mut c_void, p_blob as *const c_void, p_bytes as usize); }
}
/// Render/SinglePassPipelines.cpp: `void GraphicsCreated(ID3D12Device* device, const D3D12_GRAPHICS_PIPELINE_STATE_DESC& desc, ID3D12PipelineState* pso, CreateStreamFn create) { cvr_sing`
#[export_name = "?GraphicsCreated@single_pass@cvr@@YAXPEAUID3D12Device@@AEBUD3D12_GRAPHICS_PIPELINE_STATE_DESC@@PEAUID3D12PipelineState@@P6AJ0PEBUD3D12_PIPELINE_STATE_STREAM_DESC@@AEBU_GUID@@PEAPEAX@Z@Z"]
pub unsafe extern "C" fn fwd_cvr__single_pass__graphicscreated(p_device: *mut c_void, p_desc: *mut c_void, p_pso: *mut c_void, p_create: *mut c_void) {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_single_pass_graphics_created(p_device as *mut c_void, p_desc as *const c_void, p_pso as *mut c_void, p_create as *mut c_void); }
}
/// Render/SinglePassPipelines.cpp: `void StreamCreated(ID3D12Device* device, const D3D12_PIPELINE_STATE_STREAM_DESC& desc, ID3D12PipelineState* pso, CreateStreamFn create) { cvr_single_p`
#[export_name = "?StreamCreated@single_pass@cvr@@YAXPEAUID3D12Device@@AEBUD3D12_PIPELINE_STATE_STREAM_DESC@@PEAUID3D12PipelineState@@P6AJ0PEBU4@AEBU_GUID@@PEAPEAX@Z@Z"]
pub unsafe extern "C" fn fwd_cvr__single_pass__streamcreated(p_device: *mut c_void, p_desc: *mut c_void, p_pso: *mut c_void, p_create: *mut c_void) {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_single_pass_stream_created(p_device as *mut c_void, p_desc as *const c_void, p_pso as *mut c_void, p_create as *mut c_void); }
}
/// Stereo/Hud.cpp: `void hud_register_rtv(ID3D12Resource* res, const D3D12_RENDER_TARGET_VIEW_DESC* vd, D3D12_CPU_DESCRIPTOR_HANDLE h) { cvr_hud_register_rtv(res, vd, h.p`
#[export_name = "?hud_register_rtv@detail@cvr@@YAXPEAUID3D12Resource@@PEBUD3D12_RENDER_TARGET_VIEW_DESC@@UD3D12_CPU_DESCRIPTOR_HANDLE@@@Z"]
pub unsafe extern "C" fn fwd_cvr__detail__hud_register_rtv(p_res: *mut c_void, p_vd: *mut c_void, p_h: u64) {
    // SAFETY: the C++ caller's arguments, passed on as the forward did.
    unsafe { cvr_hud_register_rtv(p_res as *mut c_void, p_vd as *const c_void, p_h as usize); }
}
}

// ---- the headset's default FOV and the list detours' early depth dispatch (formerly OpenXRManager.cpp's and
// ListDetourHost.cpp's) (B462)
mod headset_and_early_depth {
    use core::ffi::{c_char, c_void};
    unsafe extern "C-unwind" {
        /// vrcam_names.rs's (its private abi module): the measured default for a headset name, with its one log line.
        fn cvr_xr_headset_default_fov(name: *const c_char) -> f32;
    }
    /// CyberpunkVR_HeadsetDefaultFovDeg: the runtime system's measured default horizontal FOV, 0 when the headset is not in
    /// the table (or no system is known yet).
    #[no_mangle]
    pub extern "C" fn CyberpunkVR_HeadsetDefaultFovDeg() -> f32 {
        let core = cvr_xr::core_handles::cvr_xr_core();
        // SAFETY: cvr-xr's core block, which lives for the process.
        let name = unsafe { core::ptr::addr_of!((*core).system_name) }.cast::<c_char>();
        // SAFETY: the system name's terminated bytes.
        if unsafe { *name } == 0 {
            return 0.0;
        }
        // SAFETY: as above.
        unsafe { cvr_xr_headset_default_fov(name) }
    }
    /// The list detours' early depth: 0 Unsupported(reason), 1 InstanceWrite(resource), 2 Closing, 4 Drawing() ||
    /// MaterialPending(), 5 InstanceBarrier(resource), otherwise RunTracking.
    #[no_mangle]
    pub extern "C" fn cvr_lh_early_depth(op: u32, list: *mut c_void, resource: *mut c_void, reason: u32) -> bool {
        use crate::early_depth_native as e;
        match op {
            0 => {
                e::cvr_ned_unsupported(list, reason);
                true
            }
            1 => {
                e::cvr_ned_instance_write(list, resource);
                true
            }
            2 => {
                e::cvr_ned_closing(list);
                true
            }
            4 => e::cvr_ned_drawing() || e::cvr_ned_material_pending(),
            5 => {
                e::cvr_ned_instance_barrier(list, resource);
                true
            }
            _ => e::cvr_ned_run_tracking(),
        }
    }
}

// ---- the C++ half's simple globals, defined here under their own symbols with their initial values (B465)
#[allow(non_upper_case_globals)]
mod cpp_globals {
/// Core/LiveControlsPoll.cpp: `g_submitChainRev = 0`
#[export_name = "?g_submitChainRev@@3HA"]
pub static mut g_submitChainRev: i32 = 0;
/// Core/LiveControlsPoll.cpp: `g_viveTrackersSetting = 0`
#[export_name = "?g_viveTrackersSetting@@3HA"]
pub static mut g_viveTrackersSetting: i32 = 0;
/// Stereo/Dlss.cpp: `g_vrcam_dlss_cache_valid = false`
#[export_name = "?g_vrcam_dlss_cache_valid@detail@cvr@@3_NA"]
pub static mut g_vrcam_dlss_cache_valid: bool = false;
/// Natives/OrientationProvider.cpp: `g_provMuzzleSeq = 0`
#[no_mangle]
pub static mut g_provMuzzleSeq: u32 = 0;
/// Natives/OrientationProvider.cpp: `g_provRecoilSeqSeen = -1`
#[no_mangle]
pub static mut g_provRecoilSeqSeen: i32 = -1;
/// Natives/CameraPair.cpp: `g_VRCamPosX = 0.0f`
#[no_mangle]
pub static mut g_VRCamPosX: f32 = 0.0;
/// Natives/CameraPair.cpp: `g_VRCamPosY = 0.0f`
#[no_mangle]
pub static mut g_VRCamPosY: f32 = 0.0;
/// Natives/CameraPair.cpp: `g_VRCamPosZ = 0.0f`
#[no_mangle]
pub static mut g_VRCamPosZ: f32 = 0.0;
/// Natives/CameraPair.cpp: `g_VREntityPosX = 0.0f`
#[no_mangle]
pub static mut g_VREntityPosX: f32 = 0.0;
/// Natives/CameraPair.cpp: `g_VREntityPosY = 0.0f`
#[no_mangle]
pub static mut g_VREntityPosY: f32 = 0.0;
/// Natives/CameraPair.cpp: `g_VREntityPosZ = 0.0f`
#[no_mangle]
pub static mut g_VREntityPosZ: f32 = 0.0;
/// Natives/CameraPair.cpp: `g_VRCamPairLocalX = 0.0f`
#[no_mangle]
pub static mut g_VRCamPairLocalX: f32 = 0.0;
/// Natives/CameraPair.cpp: `g_VRCamPairLocalY = 0.0f`
#[no_mangle]
pub static mut g_VRCamPairLocalY: f32 = 0.0;
/// Natives/CameraPair.cpp: `g_VRCamPairLocalZ = 0.0f`
#[no_mangle]
pub static mut g_VRCamPairLocalZ: f32 = 0.0;
/// Natives/CameraPair.cpp: `g_VREntityQI = 0.0f`
#[no_mangle]
pub static mut g_VREntityQI: f32 = 0.0;
/// Natives/CameraPair.cpp: `g_VREntityQJ = 0.0f`
#[no_mangle]
pub static mut g_VREntityQJ: f32 = 0.0;
/// Natives/CameraPair.cpp: `g_VREntityQK = 0.0f`
#[no_mangle]
pub static mut g_VREntityQK: f32 = 0.0;
/// Natives/CameraPair.cpp: `g_VREntityQR = 1.0f`
#[no_mangle]
pub static mut g_VREntityQR: f32 = 1.0;
/// Core/VrCore.cpp: `g_dbgEntryYaw = 0.0f`
#[no_mangle]
pub static mut g_dbgEntryYaw: f32 = 0.0;
/// Core/VrCore.cpp: `g_dbgEntryPosX = 0.0f`
#[no_mangle]
pub static mut g_dbgEntryPosX: f32 = 0.0;
/// Core/VrCore.cpp: `g_dbgEntryPosY = 0.0f`
#[no_mangle]
pub static mut g_dbgEntryPosY: f32 = 0.0;
/// Core/VrCore.cpp: `g_dbgEntryPosZ = 0.0f`
#[no_mangle]
pub static mut g_dbgEntryPosZ: f32 = 0.0;
/// Stereo/LumaProbe.cpp: `g_visdump_map = nullptr`
#[no_mangle]
pub static mut g_visdump_map: *mut core::ffi::c_void = core::ptr::null_mut();
/// Stereo/LumaProbe.cpp: `g_visdump_w = 0`
#[no_mangle]
pub static mut g_visdump_w: u32 = 0;
/// Stereo/Profiler.cpp: `ProfPass g_prof_scenepass[256]` (full 0..255: 0xFF is a legitimate rtId value)
#[no_mangle]
pub static g_prof_scenepass: [crate::stereo_profiler::ProfPass; 256] = [const { crate::stereo_profiler::ProfPass::zero() }; 256];
/// Stereo/SyncStereo.cpp: `g_orig_CreateDescriptorHeap = nullptr` (the device creation's original; device_creation.rs
/// writes it)
#[export_name = "?g_orig_CreateDescriptorHeap@detail@cvr@@3P6AJPEAUID3D12Device@@PEBUD3D12_DESCRIPTOR_HEAP_DESC@@AEBU_GUID@@PEAPEAX@ZEA"]
pub static mut g_orig_CreateDescriptorHeap: *const core::ffi::c_void = core::ptr::null();
/// Stereo/LumaProbe.cpp: `g_visdump_h = 0`
#[no_mangle]
pub static mut g_visdump_h: u32 = 0;
/// Stereo/LumaProbe.cpp: `g_visdump_pitch = 0`
#[no_mangle]
pub static mut g_visdump_pitch: u32 = 0;
/// Stereo/ViewConstants.cpp: `g_cc_big{0}`
#[no_mangle]
pub static g_cc_big: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
/// Hooks/CameraFov.cpp: `g_dbgLastOriginalFov = 0.0f`
#[export_name = "?g_dbgLastOriginalFov@@3MC"]
pub static mut g_dbgLastOriginalFov: f32 = 0.0;
/// Natives/UiPopup.cpp: `g_uiPopupOpen{0}`
#[export_name = "?g_uiPopupOpen@@3U?$atomic@H@std@@A"]
pub static g_uiPopupOpen: core::sync::atomic::AtomicI32 = core::sync::atomic::AtomicI32::new(0);
/// Natives/UiPopup.cpp: `g_uiPopupClosedMs{0}`
#[export_name = "?g_uiPopupClosedMs@@3U?$atomic@_K@std@@A"]
pub static g_uiPopupClosedMs: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
/// src/Natives/UiPopup.cpp's services (the natives are cvr-rig's ui_popup.rs): the popup flag swapped, its close time stored.
#[no_mangle]
pub extern "C" fn cvr_cpp_ui_popup_exchange(open: i32) -> i32 {
    g_uiPopupOpen.swap(open, core::sync::atomic::Ordering::Relaxed)
}
#[no_mangle]
pub extern "C" fn cvr_cpp_ui_popup_closed_store(ms: u64) {
    g_uiPopupClosedMs.store(ms, core::sync::atomic::Ordering::Relaxed)
}
}

// ---- the HUD quad's GPU profile hold (formerly src/Runtimes/HudQuad.cpp's ProfileHold: a batch and the HudSubmit
// scope over it, cvr-gpu's stage profile handles) (B467)
mod hud_profile {
    use core::ffi::c_void;
    use cvr_gpu::stage_profile::{
        cvr_gpu_stage_begin, cvr_gpu_stage_clone, cvr_gpu_stage_free, cvr_gpu_stage_scope_begin, cvr_gpu_stage_scope_end,
        cvr_gpu_stage_submitted, Batch,
    };
    /// cvr::gpu::profile::Stage::HudSubmit.
    const HUD_SUBMIT: u32 = 10;
    const NO_SCOPE: u32 = !0;
    struct Hold {
        batch: *const Batch,
        scope: u32,
    }
    impl Hold {
        /// The scope's end (its destructor: only when it began).
        fn end_scope(&mut self) {
            if self.scope != NO_SCOPE {
                // SAFETY: the hold's batch, which the scope began on.
                unsafe { cvr_gpu_stage_scope_end(self.batch, self.scope) };
                self.scope = NO_SCOPE;
            }
        }
    }
    impl Drop for Hold {
        /// The members in reverse order: the scope, then the batch.
        fn drop(&mut self) {
            self.end_scope();
            // SAFETY: the hold's own batch reference (or null).
            unsafe { cvr_gpu_stage_free(self.batch) };
        }
    }
    /// A batch begun on the list with its HudSubmit scope open.
    #[no_mangle]
    pub unsafe extern "C" fn cvr_cpp_gpu_profile_hud_begin(list: *mut c_void) -> *mut c_void {
        // SAFETY: the caller's open command list.
        let batch = unsafe { cvr_gpu_stage_begin(list) };
        let scope = if batch.is_null() {
            NO_SCOPE
        } else {
            // SAFETY: the batch just begun.
            unsafe { cvr_gpu_stage_scope_begin(batch, HUD_SUBMIT) }
        };
        Box::into_raw(Box::new(Hold { batch, scope })).cast()
    }
    /// The HudSubmit scope closed.
    #[no_mangle]
    pub unsafe extern "C" fn cvr_cpp_gpu_profile_hud_end(hold: *mut c_void) {
        // SAFETY: a hold cvr_cpp_gpu_profile_hud_begin made.
        unsafe { &mut *(hold as *mut Hold) }.end_scope();
    }
    /// Submitted (through a copy of the batch, as the C++ by-value call made), then the hold let go.
    #[no_mangle]
    pub unsafe extern "C" fn cvr_cpp_gpu_profile_submitted(hold: *mut c_void, queue: *mut c_void, fence: *mut c_void, value: u64) {
        // SAFETY: a hold cvr_cpp_gpu_profile_hud_begin made, owned from here.
        let held = unsafe { Box::from_raw(hold as *mut Hold) };
        // SAFETY: the hold's batch (or null) and the caller's queue and fence.
        unsafe {
            let copy = if held.batch.is_null() { core::ptr::null() } else { cvr_gpu_stage_clone(held.batch) };
            cvr_gpu_stage_submitted(copy, queue, fence, value);
            cvr_gpu_stage_free(copy);
        }
        drop(held);
    }
    /// The hold let go unsubmitted.
    #[no_mangle]
    pub unsafe extern "C" fn cvr_cpp_gpu_profile_release(hold: *mut c_void) {
        if !hold.is_null() {
            // SAFETY: a hold cvr_cpp_gpu_profile_hud_begin made.
            drop(unsafe { Box::from_raw(hold as *mut Hold) });
        }
    }
}

// ---- the haptics' device call (formerly src/Runtimes/HapticsDevice.cpp and OpenXRManager::ApplyHaptic) (B468)
mod haptics_device {
    use core::sync::atomic::Ordering::Relaxed;
    #[allow(non_upper_case_globals)]
    unsafe extern "C" {
        /// cvr::psvr2::Haptics().Pulse(hand, amplitude, milliseconds) (src/Runtimes/HapticsDevice.cpp).
        safe fn cvr_cpp_psvr2_pulse(hand: i32, amplitude: f32, milliseconds: i32);
    }
    static LAST_RESULT: [core::sync::atomic::AtomicI32; 2] =
        [core::sync::atomic::AtomicI32::new(123_456), core::sync::atomic::AtomicI32::new(123_456)];
    static BINDING_LOGGED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
    /// OpenXRManager::ApplyHaptic: a vibration on one hand. With the Toolkit bridge chosen and found, its channel is
    /// the one actuator owner (OpenXR is the default for every headset, PSVR2 included); else OpenXR, the bound outputs
    /// logged once (a saved SteamVR binding may lack the haptic action) and each hand's result when it changes.
    #[no_mangle]
    pub extern "C" fn cvr_cpp_haptics_play(hand: i32, amplitude: f32, seconds: f32) -> i32 {
        let running = cvr_xr::events::cvr_xr_session_shared_ref().running.load(Relaxed);
        if cvr_settings::haptic_route::HAPTIC_ROUTE.toolkit() {
            if !running || !(0..=1).contains(&hand) || !amplitude.is_finite() || !seconds.is_finite() || amplitude <= 0.0 || seconds <= 0.0 {
                return -1;
            }
            let ms = ((if seconds > 0.5 { 0.5 } else { seconds }) * 1000.0) as i32;
            cvr_cpp_psvr2_pulse(hand, amplitude.min(1.0), ms.max(1));
            return 0;
        }
        // SAFETY: cvr-xr's core block, alive for the process.
        let core = unsafe { &*cvr_xr::core_handles::cvr_xr_core() };
        if core.haptic_action == 0 || core.session == 0 || !running {
            return -1;
        }
        if !(0..=1).contains(&hand) || !amplitude.is_finite() || !seconds.is_finite() || amplitude <= 0.0 || seconds <= 0.0 {
            return -1;
        }
        if !BINDING_LOGGED.swap(true, Relaxed) {
            // SAFETY: the live instance, session and action.
            unsafe { cvr_xr::teardown::cvr_xr_log_haptic_bindings(core.instance, core.session, core.haptic_action) };
        }
        let path = core.hand_paths[hand as usize];
        // SAFETY: the session's handles.
        let r = unsafe {
            cvr_xr::teardown::cvr_xr_apply_haptic(core.session, core.haptic_action, path, running, hand, amplitude, seconds)
        };
        if LAST_RESULT[hand as usize].swap(r, Relaxed) != r {
            cvr_host::log::write(&format!(
                "OpenXRManager[Haptics]: OpenXR hand={hand} apply result={r} (accepted call is not proof of physical vibration).\n"
            ));
        }
        r
    }
}

// ---- Hooks/HookServices.cpp's services the Rust hooks (cvr-rig) take: the player's replacement, the foreground and
// SendInput calls, the hooks' telemetry line, the fast path's block, the pending snap yaw, the overlay's visibility and
// the simulator (B471)
mod hook_services {
    use core::ffi::c_void;
    use core::ptr::{addr_of_mut, read_volatile, write_volatile};
    use core::sync::atomic::Ordering::{Release, SeqCst};

    /// TelemetryData (include/Core/Telemetry.hpp): one block, whose fields the trampolines' immediates address.
    #[repr(C)]
    struct Telemetry {
        locate_hits: u32,
        _pad1: u32,
        locate_rbx: usize,
        locate_xmm0: f32,
        _pad2: [u32; 3],
        patch_hits: u32,
        _pad3: u32,
        patch_rdx: usize,
        patch_rsi: usize,
        patch_xmm0: [f32; 4],
        final_hits: u32,
        _pad4: u32,
        final_rsi: usize,
        delta_head_hits: u32,
        _pad5: u32,
        delta_head_rcx: usize,
        delta_head_xmm0: f32,
        _pad6: [u32; 3],
        move_xy_hits: u32,
        _pad7: u32,
        move_xy_rsi: usize,
        move_xy_xmm0: f32,
        _pad8: [u32; 3],
        free_delta_hits: u32,
        _pad9: u32,
        free_delta_rsi: usize,
        free_delta_xmm3: f32,
        _pad10: [u32; 3],
    }
    const _: () = assert!(
        core::mem::size_of::<Telemetry>() == 184
            && core::mem::offset_of!(Telemetry, patch_hits) == 32
            && core::mem::offset_of!(Telemetry, final_hits) == 72
            && core::mem::offset_of!(Telemetry, delta_head_hits) == 88
            && core::mem::offset_of!(Telemetry, move_xy_hits) == 120
            && core::mem::offset_of!(Telemetry, free_delta_hits) == 152
    );
    /// kPatchTelemetryOffset and kFreeDeltaTelemetryOffset.
    const PATCH_OFFSET: usize = 32;
    const FREE_DELTA_OFFSET: usize = 152;
    #[allow(non_upper_case_globals)]
    unsafe extern "C" {
        /// TelemetryData* g_telemetry (src/Core/VrCore.cpp; the worker allocates it).
        #[link_name = "?g_telemetry@@3PEAUTelemetryData@@EA"]
        static g_telemetry: *mut Telemetry;
        static CyberpunkVR_RuntimeDiagnostics: i32;
    }
    fn telemetry() -> *mut Telemetry {
        // SAFETY: a pointer the worker sets once.
        unsafe { g_telemetry }
    }
    fn diagnostics() -> bool {
        // SAFETY: the C++ half's switch.
        unsafe { CyberpunkVR_RuntimeDiagnostics != 0 }
    }

    /// A player replacement: a previous puppet can keep animating, its camera counters still advancing.
    #[no_mangle]
    pub extern "C" fn cvr_cpp_camera_player_changed() {
        // SAFETY: the C-linkage atomic.
        unsafe { super::globals::g_camObjMain.store(0, Release) };
        crate::camera_owner::cvr_patch_fast_disarm();
    }
    /// The foreground window is this process's: the cyberware chord's key reaches no other app.
    #[no_mangle]
    pub extern "C" fn cvr_cpp_foreground_in_process() -> bool {
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
        let mut process = 0u32;
        // SAFETY: plain Win32 calls.
        unsafe {
            GetWindowThreadProcessId(GetForegroundWindow(), &mut process);
            process == windows_sys::Win32::System::Threading::GetCurrentProcessId()
        }
    }
    /// FinalCamera's telemetry line.
    #[no_mangle]
    pub extern "C" fn cvr_cpp_telemetry_final(hits: u32, rsi: usize) {
        let t = telemetry();
        if !t.is_null() {
            // SAFETY: the block's fields.
            unsafe {
                write_volatile(addr_of_mut!((*t).final_hits), hits);
                write_volatile(addr_of_mut!((*t).final_rsi), rsi);
            }
        }
    }
    /// The fast path's block the trampoline bakes in.
    #[no_mangle]
    pub extern "C" fn cvr_cpp_patch_fast_path() -> usize {
        (&raw mut crate::camera_owner::g_patchFast) as usize
    }
    /// PatchCamera's telemetry line, or 0 without the block.
    #[no_mangle]
    pub extern "C" fn cvr_cpp_telemetry_patch_hits() -> usize {
        let t = telemetry();
        if t.is_null() { 0 } else { t as usize + PATCH_OFFSET }
    }
    /// LocateCamera's telemetry line.
    #[no_mangle]
    pub extern "C" fn cvr_cpp_telemetry_locate(hits: u32, rbx: u64, xmm0: f32) {
        let t = telemetry();
        if t.is_null() {
            return;
        }
        // SAFETY: the block's fields.
        unsafe {
            write_volatile(addr_of_mut!((*t).locate_hits), hits);
            write_volatile(addr_of_mut!((*t).locate_rbx), rbx as usize);
            write_volatile(addr_of_mut!((*t).locate_xmm0), xmm0);
        }
    }
    /// The free-delta hook's telemetry line (the trampoline bakes the address in, the block present or not).
    #[no_mangle]
    pub extern "C" fn cvr_cpp_telemetry_free_delta() -> usize {
        telemetry() as usize + FREE_DELTA_OFFSET
    }
    /// The on-foot move hook's telemetry, counted while the runtime diagnostics are on.
    #[no_mangle]
    pub extern "C" fn cvr_cpp_telemetry_move_xy(move_struct: *mut c_void) {
        let t = telemetry();
        if t.is_null() {
            return;
        }
        // SAFETY: the block's fields.
        unsafe {
            if diagnostics() {
                write_volatile(addr_of_mut!((*t).move_xy_hits), read_volatile(addr_of_mut!((*t).move_xy_hits)).wrapping_add(1));
            }
            if diagnostics() {
                write_volatile(addr_of_mut!((*t).move_xy_rsi), move_struct as usize);
            }
        }
    }
    /// The on-foot delta head hook's telemetry.
    #[no_mangle]
    pub extern "C" fn cvr_cpp_telemetry_delta_head(delta_head: *mut f32) {
        let t = telemetry();
        if t.is_null() {
            return;
        }
        // SAFETY: the block's fields.
        unsafe {
            write_volatile(addr_of_mut!((*t).delta_head_hits), read_volatile(addr_of_mut!((*t).delta_head_hits)).wrapping_add(1));
            write_volatile(addr_of_mut!((*t).delta_head_rcx), delta_head as usize);
        }
    }
    /// The snap yaw the stick queued, taken.
    #[no_mangle]
    pub extern "C" fn cvr_cpp_take_pending_snap_yaw() -> i32 {
        // SAFETY: the C++ half's interlocked long.
        unsafe { super::globals::g_pendingSnapYawDeltaBits.swap(0, SeqCst) }
    }
    /// The keys the XInput merge types (the scanner's zoom and list).
    ///
    /// # Safety
    /// `inputs` holds `count` INPUT records of `size` bytes.
    #[no_mangle]
    pub unsafe extern "C" fn cvr_cpp_send_input(count: u32, inputs: *const c_void, size: i32) -> u32 {
        // SAFETY: the caller's records.
        unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::SendInput(count, inputs.cast(), size) }
    }
    /// OverlayIsVisible: the VR overlay is up.
    #[no_mangle]
    pub extern "C" fn cvr_cpp_overlay_visible() -> bool {
        cvr_body::vr_overlay::cvr_vrui_visible()
    }
    /// The OpenXR simulator is the runtime (its DLL is loaded).
    #[no_mangle]
    pub extern "C" fn cvr_cpp_simulator_loaded() -> bool {
        let name: Vec<u16> = "openxr_simulator.dll".encode_utf16().chain(Some(0)).collect();
        // SAFETY: a terminated wide name.
        !unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(name.as_ptr()) }.is_null()
    }
}

// ---- the first launch's entry (formerly src/Core/FirstLaunch.cpp), VrCore.cpp's GetRenderedCameraSeq and GetMenuMode (B473)
mod first_launch_entry {
    /// cvr-ffi's first_launch.rs Runtime: the live controls' first_launch and their preparation.
    #[repr(C)]
    struct Runtime {
        first_launch: *mut i32,
        prepare: Option<unsafe extern "C" fn()>,
    }
    unsafe extern "C" {
        /// cvr-ffi's first_launch.rs.
        fn cvr_first_launch_apply(runtime: *const Runtime);
        /// The runtime paths, the live controls' file and their poll.
        fn cvr_init_runtime_paths();
        fn cvr_ensure_live_control_file_exists();
        fn cvr_poll_live_controls();
    }
    /// InitRuntimePaths, EnsureLiveControlFileExists and PollLiveControls.
    unsafe extern "C" fn prepare() {
        // SAFETY: the plugin's own preparation.
        unsafe {
            cvr_init_runtime_paths();
            cvr_ensure_live_control_file_exists();
            cvr_poll_live_controls();
        }
    }
    /// ApplyFirstLaunchGameSettings: the shipped preset merged into the player's settings, once.
    #[no_mangle]
    pub extern "C" fn ApplyFirstLaunchGameSettings() {
        let block = cvr_settings::live_controls_block::block();
        if block.is_null() {
            return;
        }
        // SAFETY: the plugin's block, alive for the process.
        let runtime = Runtime { first_launch: unsafe { &raw mut (*block).xrFirstLaunch }, prepare: Some(prepare) };
        // SAFETY: a runtime the call uses while it runs.
        unsafe { cvr_first_launch_apply(&runtime) };
    }
    /// GetRenderedCameraSeq: the locate sequence of the camera the last rendered frame used (FinalCamera's).
    #[no_mangle]
    pub extern "C" fn GetRenderedCameraSeq() -> u32 {
        crate::vrcore_exports::g_renderedSeq.load(core::sync::atomic::Ordering::Relaxed)
    }
    /// GetMenuMode: the game's menu mode (g_menuModeValue).
    #[no_mangle]
    pub extern "C" fn GetMenuMode() -> i32 {
        super::cvr_cpp_menu_mode()
    }
}
