//! The plugin's exported API functions whose C++ definitions only forwarded to Rust: exported under the same names
//! (the DLL's export table lists them through rust/exported-functions.txt), each calling the Rust function it called.
#![allow(non_snake_case)]

extern "C" {
    fn cvr_swapchain_plugin_bootstrap();
    fn cvr_view_key_main_recording() -> i32;
    fn cvr_view_key_hook_active() -> i32;
    fn cvr_srg_configure(enable: i32);
    fn cvr_srt_poll();
    fn cvr_capture_register_foreign_resource(r: *mut core::ffi::c_void);
    fn cvr_capture_get_vrcam_eye_texture() -> *mut core::ffi::c_void;
    fn cvr_capture_get_vrcam_eye_texture_fresh() -> *mut core::ffi::c_void;
    fn cvr_cull_dump_prepare_finalize_buckets();
    fn cvr_hud_get_vision_texture() -> *mut core::ffi::c_void;
    fn cvr_hud_get_hud_texture() -> *mut core::ffi::c_void;
    fn cvr_hud_get_slot_texture(slot0: u32) -> *mut core::ffi::c_void;
    fn cvr_hud_get_constant_buffer() -> *mut core::ffi::c_void;
    fn cvr_stereo_prof_publish();
    fn cvr_stereo_prof_node_name(rva: u32) -> *const core::ffi::c_char;
    fn cvr_stereo_node_cut_get(rva: u32, rtid: u32) -> i32;
    fn cvr_stereo_node_cut_poll();
    fn cvr_stereo_prof_dump_nodes();
    fn cvr_ss_bd_cam_name_hash() -> u64;
    fn cvr_ss_selected_resolution(w: *mut u32, h: *mut u32) -> i32;
}

/// CyberpunkVRPort_PluginBootstrap (formerly src/Hooks/SwapChain.cpp).
#[no_mangle]
pub unsafe extern "C" fn CyberpunkVRPort_PluginBootstrap() {
    // SAFETY: the caller's arguments, passed on.
    unsafe { cvr_swapchain_plugin_bootstrap() }
}
/// CyberpunkVR_IsMainViewRecording (formerly src/Hooks/ViewKey.cpp).
#[no_mangle]
pub unsafe extern "C" fn CyberpunkVR_IsMainViewRecording() -> i32 {
    // SAFETY: the caller's arguments, passed on.
    unsafe { cvr_view_key_main_recording() }
}
/// CyberpunkVR_ViewKeyHookActive (formerly src/Hooks/ViewKey.cpp).
#[no_mangle]
pub unsafe extern "C" fn CyberpunkVR_ViewKeyHookActive() -> i32 {
    // SAFETY: the caller's arguments, passed on.
    unsafe { cvr_view_key_hook_active() }
}
/// CyberpunkVR_StageReplayConfigure (formerly src/Render/StageReplay.cpp).
#[no_mangle]
pub unsafe extern "C" fn CyberpunkVR_StageReplayConfigure(enable: i32) {
    // SAFETY: the caller's arguments, passed on.
    unsafe { cvr_srg_configure(enable) }
}
/// CyberpunkVR_StageReplayPoll (formerly src/Render/StageReplay.cpp).
#[no_mangle]
pub unsafe extern "C" fn CyberpunkVR_StageReplayPoll() {
    // SAFETY: the caller's arguments, passed on.
    unsafe { cvr_srt_poll() }
}
/// CyberpunkVR_RegisterForeignResource (formerly src/Stereo/Capture.cpp).
#[no_mangle]
pub unsafe extern "C" fn CyberpunkVR_RegisterForeignResource(r: *mut core::ffi::c_void) {
    // SAFETY: the caller's arguments, passed on.
    unsafe { cvr_capture_register_foreign_resource(r) }
}
/// CyberpunkVR_GetVrcamEyeTexture (formerly src/Stereo/Capture.cpp).
#[no_mangle]
pub unsafe extern "C" fn CyberpunkVR_GetVrcamEyeTexture() -> *mut core::ffi::c_void {
    // SAFETY: the caller's arguments, passed on.
    unsafe { cvr_capture_get_vrcam_eye_texture() }
}
/// CyberpunkVR_GetVrcamEyeTextureFresh (formerly src/Stereo/Capture.cpp).
#[no_mangle]
pub unsafe extern "C" fn CyberpunkVR_GetVrcamEyeTextureFresh() -> *mut core::ffi::c_void {
    // SAFETY: the caller's arguments, passed on.
    unsafe { cvr_capture_get_vrcam_eye_texture_fresh() }
}
/// CyberpunkVR_DumpPrepareFinalizeBuckets (formerly src/Stereo/CullVisibility.cpp).
#[no_mangle]
pub unsafe extern "C" fn CyberpunkVR_DumpPrepareFinalizeBuckets() {
    // SAFETY: the caller's arguments, passed on.
    unsafe { cvr_cull_dump_prepare_finalize_buckets() }
}
/// CyberpunkVR_GetVisionTexture (formerly src/Stereo/Hud.cpp).
#[no_mangle]
pub unsafe extern "C" fn CyberpunkVR_GetVisionTexture() -> *mut core::ffi::c_void {
    // SAFETY: the caller's arguments, passed on.
    unsafe { cvr_hud_get_vision_texture() }
}
/// CyberpunkVR_GetHudTexture (formerly src/Stereo/Hud.cpp).
#[no_mangle]
pub unsafe extern "C" fn CyberpunkVR_GetHudTexture() -> *mut core::ffi::c_void {
    // SAFETY: the caller's arguments, passed on.
    unsafe { cvr_hud_get_hud_texture() }
}
/// CyberpunkVR_GetHudBlurTexture (formerly src/Stereo/Hud.cpp).
#[no_mangle]
pub unsafe extern "C" fn CyberpunkVR_GetHudBlurTexture() -> *mut core::ffi::c_void {
    // SAFETY: the caller's arguments, passed on.
    unsafe { cvr_hud_get_slot_texture(1) }
}
/// CyberpunkVR_GetMainOutTexture (formerly src/Stereo/Hud.cpp).
#[no_mangle]
pub unsafe extern "C" fn CyberpunkVR_GetMainOutTexture() -> *mut core::ffi::c_void {
    // SAFETY: the caller's arguments, passed on.
    unsafe { cvr_hud_get_slot_texture(2) }
}
/// CyberpunkVR_GetMainSceneTexture (formerly src/Stereo/Hud.cpp).
#[no_mangle]
pub unsafe extern "C" fn CyberpunkVR_GetMainSceneTexture() -> *mut core::ffi::c_void {
    // SAFETY: the caller's arguments, passed on.
    unsafe { cvr_hud_get_slot_texture(3) }
}
/// CyberpunkVR_GetHudConstantBuffer (formerly src/Stereo/Hud.cpp).
#[no_mangle]
pub unsafe extern "C" fn CyberpunkVR_GetHudConstantBuffer() -> *mut core::ffi::c_void {
    // SAFETY: the caller's arguments, passed on.
    unsafe { cvr_hud_get_constant_buffer() }
}
/// CyberpunkVR_ProfPublish (formerly src/Stereo/Profiler.cpp).
#[no_mangle]
pub unsafe extern "C" fn CyberpunkVR_ProfPublish() {
    // SAFETY: the caller's arguments, passed on.
    unsafe { cvr_stereo_prof_publish() }
}
/// CyberpunkVR_ProfNodeName (formerly src/Stereo/Profiler.cpp).
#[no_mangle]
pub unsafe extern "C" fn CyberpunkVR_ProfNodeName(rva: u32) -> *const core::ffi::c_char {
    // SAFETY: the caller's arguments, passed on.
    unsafe { cvr_stereo_prof_node_name(rva) }
}
/// CyberpunkVR_NodeCutGet (formerly src/Stereo/Profiler.cpp).
#[no_mangle]
pub unsafe extern "C" fn CyberpunkVR_NodeCutGet(rva: u32, rtid: u32) -> i32 {
    // SAFETY: the caller's arguments, passed on.
    unsafe { cvr_stereo_node_cut_get(rva, rtid) }
}
/// CyberpunkVR_NodeCutPoll (formerly src/Stereo/Profiler.cpp).
#[no_mangle]
pub unsafe extern "C" fn CyberpunkVR_NodeCutPoll() {
    // SAFETY: the caller's arguments, passed on.
    unsafe { cvr_stereo_node_cut_poll() }
}
/// CyberpunkVR_ProfDumpNodes (formerly src/Stereo/Profiler.cpp).
#[no_mangle]
pub unsafe extern "C" fn CyberpunkVR_ProfDumpNodes() {
    // SAFETY: the caller's arguments, passed on.
    unsafe { cvr_stereo_prof_dump_nodes() }
}
/// CyberpunkVR_VrcamBdCamNameHash (formerly src/Stereo/SyncStereo.cpp).
#[no_mangle]
pub unsafe extern "C" fn CyberpunkVR_VrcamBdCamNameHash() -> u64 {
    // SAFETY: the caller's arguments, passed on.
    unsafe { cvr_ss_bd_cam_name_hash() }
}
/// CyberpunkVR_GetSelectedResolution (formerly src/Stereo/SyncStereo.cpp).
#[no_mangle]
pub unsafe extern "C" fn CyberpunkVR_GetSelectedResolution(w: *mut u32, h: *mut u32) -> i32 {
    // SAFETY: the caller's arguments, passed on.
    unsafe { cvr_ss_selected_resolution(w, h) }
}
