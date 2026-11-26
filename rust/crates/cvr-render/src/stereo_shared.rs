//! THE STEREO HOOKS' SHARED OBJECTS that the capture, mirror and HUD halves read (formerly C++ globals of SyncStereo.cpp,
//! Mirror.cpp, Capture.cpp, Hud.cpp, NodeDispatch.cpp, LumaProbe.cpp and DeviceHooks.cpp), and the services those
//! halves took from the C++ through their runtime tables. Each object is owned here under the symbol its C++
//! definition had (MSVC-decorated, so the C++ declarations in include/Stereo/StereoInternal.hpp reach it unchanged),
//! with the same initial value; the five std::mutex are SRW locks (cvr::SrwMutex on the C++ side, extern "C").
#![allow(non_upper_case_globals)]
use core::ffi::{c_char, c_void};
use core::ptr::null_mut;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering::*};
use cvr_xr::srw::SrwLock;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::{ID3D12Resource, D3D12_RESOURCE_DESC};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT, DXGI_FORMAT_R8G8B8A8_UNORM_SRGB};


// ---- the second view's output and its stable snapshot (Capture.cpp, SyncStereo.cpp)
/// std::atomic<ID3D12Resource*> g_captured_vrcam_res{nullptr}: the second view's current output.
#[export_name = "?g_captured_vrcam_res@detail@cvr@@3U?$atomic@PEAUID3D12Resource@@@std@@A"]
pub static g_captured_vrcam_res: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
/// ID3D12Resource* g_stable_tex = nullptr; D3D12_RESOURCE_DESC g_stable_desc{}: the committed snapshot, under
/// g_stable_mtx.
#[export_name = "?g_stable_tex@detail@cvr@@3PEAUID3D12Resource@@EA"]
pub static mut g_stable_tex: *mut c_void = null_mut();
#[export_name = "?g_stable_desc@detail@cvr@@3UD3D12_RESOURCE_DESC@@A"]
// SAFETY: D3D12_RESOURCE_DESC is plain data; all zero is the C++'s value-initialisation.
pub static mut g_stable_desc: D3D12_RESOURCE_DESC = unsafe { core::mem::zeroed() };
/// The 2-MRT pass's first four RT0 handles and their RT1 partners, and the tonemap pass's RT0
/// (std::atomic<SIZE_T> g_2rt_seen_h0[4], g_2rt_seen_h1[4], g_tonemap_h0).
#[export_name = "?g_2rt_seen_h0@detail@cvr@@3PAU?$atomic@_K@std@@A"]
pub static g_2rt_seen_h0: [AtomicUsize; 4] = [const { AtomicUsize::new(0) }; 4];
#[export_name = "?g_2rt_seen_h1@detail@cvr@@3PAU?$atomic@_K@std@@A"]
pub static g_2rt_seen_h1: [AtomicUsize; 4] = [const { AtomicUsize::new(0) }; 4];
#[export_name = "?g_tonemap_h0@detail@cvr@@3U?$atomic@_K@std@@A"]
pub static g_tonemap_h0: AtomicUsize = AtomicUsize::new(0);
/// g_mirror_rtv_candidate_count (Capture.cpp's file static): the RTV map's candidates, which the mirror's starvation
/// line reads.
pub static MIRROR_RTV_CANDIDATE_COUNT: AtomicU32 = AtomicU32::new(0);

// ---- our own second-view target and the desktop mirror (Mirror.cpp, SyncStereo.cpp)
/// ID3D12Resource* g_own_target; ID3D12DescriptorHeap* g_own_rtv_heap; D3D12_CPU_DESCRIPTOR_HANDLE g_own_rtv: under
/// g_own_target_mtx.
#[export_name = "?g_own_target@detail@cvr@@3PEAUID3D12Resource@@EA"]
pub static mut g_own_target: *mut c_void = null_mut();
#[export_name = "?g_own_rtv_heap@detail@cvr@@3PEAUID3D12DescriptorHeap@@EA"]
pub static mut g_own_rtv_heap: *mut c_void = null_mut();
#[export_name = "?g_own_rtv@detail@cvr@@3UD3D12_CPU_DESCRIPTOR_HANDLE@@A"]
pub static mut g_own_rtv: usize = 0;
/// The desktop mirror's texture, whether it is a render target, its fence and its size and format (under g_d12_mtx).
#[export_name = "?g_d12_mtex@detail@cvr@@3PEAUID3D12Resource@@EA"]
pub static mut g_d12_mtex: *mut c_void = null_mut();
#[export_name = "?g_d12_mtex_is_rt@detail@cvr@@3_NA"]
pub static mut g_d12_mtex_is_rt: bool = false;
#[export_name = "?g_d12_fence@detail@cvr@@3PEAUID3D12Fence@@EA"]
pub static mut g_d12_fence: *mut c_void = null_mut();
#[export_name = "?g_d12_w@detail@cvr@@3IA"]
pub static mut g_d12_w: u32 = 0;
#[export_name = "?g_d12_h@detail@cvr@@3IA"]
pub static mut g_d12_h: u32 = 0;
#[export_name = "?g_d12_fmt@detail@cvr@@3W4DXGI_FORMAT@@A"]
pub static mut g_d12_fmt: DXGI_FORMAT = DXGI_FORMAT_R8G8B8A8_UNORM_SRGB;
/// std::atomic<ID3D12GraphicsCommandList*> g_mirror_pending_list{nullptr}.
#[export_name = "?g_mirror_pending_list@detail@cvr@@3U?$atomic@PEAUID3D12GraphicsCommandList@@@std@@A"]
pub static g_mirror_pending_list: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
/// The game-side copy list and its four allocators (ID3D12GraphicsCommandList* g_d12_copy_list;
/// ID3D12CommandAllocator* g_d12_copy_alloc[4]).
#[export_name = "?g_d12_copy_list@detail@cvr@@3PEAUID3D12GraphicsCommandList@@EA"]
pub static mut g_d12_copy_list: *mut c_void = null_mut();
#[export_name = "?g_d12_copy_alloc@detail@cvr@@3PAPEAUID3D12CommandAllocator@@A"]
pub static mut g_d12_copy_alloc: [*mut c_void; 4] = [null_mut(); 4];
/// The vision dump's readback and slot (LumaProbe.cpp's ID3D12Resource* g_visdump_rb; int g_visdump_slot = -1).
#[export_name = "?g_visdump_rb@detail@cvr@@3PEAUID3D12Resource@@EA"]
pub static mut g_visdump_rb: *mut c_void = null_mut();
#[export_name = "?g_visdump_slot@detail@cvr@@3HA"]
pub static mut g_visdump_slot: i32 = -1;
/// DeviceHooks.cpp's CreateRTVFn g_orig_CreateRTV (0 before the device hook is in).
#[export_name = "?g_orig_CreateRTV@detail@cvr@@3P6AXPEAUID3D12Device@@PEAUID3D12Resource@@PEBUD3D12_RENDER_TARGET_VIEW_DESC@@UD3D12_CPU_DESCRIPTOR_HANDLE@@@ZEA"]
pub static mut g_orig_CreateRTV: usize = 0;

// ---- the luma and composite probes' captures (SyncStereo.cpp)
/// The adapted-exposure accumulators, one per view (std::atomic<ID3D12Resource*> g_expo_vrcam, g_expo_main).
#[export_name = "?g_expo_vrcam@detail@cvr@@3U?$atomic@PEAUID3D12Resource@@@std@@A"]
pub static g_expo_vrcam: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
#[export_name = "?g_expo_main@detail@cvr@@3U?$atomic@PEAUID3D12Resource@@@std@@A"]
pub static g_expo_main: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
/// The luma probe's constant buffer and the last one seen (std::atomic<ID3D12Resource*> g_cb_res, g_cb_last_res).
#[export_name = "?g_cb_res@detail@cvr@@3U?$atomic@PEAUID3D12Resource@@@std@@A"]
pub static g_cb_res: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
#[export_name = "?g_cb_last_res@detail@cvr@@3U?$atomic@PEAUID3D12Resource@@@std@@A"]
pub static g_cb_last_res: AtomicPtr<c_void> = AtomicPtr::new(null_mut());

// ---- the HUD (Hud.cpp, SyncStereo.cpp)
/// ID3D12Resource* g_hud_res (AddRef'd): the HUD surface.
#[export_name = "?g_hud_res@detail@cvr@@3PEAUID3D12Resource@@EA"]
pub static mut g_hud_res: *mut c_void = null_mut();
/// D3D12_RESOURCE_DESC g_hud_snap_desc{}: the HUD snapshot's description, under g_hud_snap_mtx.
#[export_name = "?g_hud_snap_desc@detail@cvr@@3UD3D12_RESOURCE_DESC@@A"]
// SAFETY: D3D12_RESOURCE_DESC is plain data; all zero is the C++'s value-initialisation.
pub static mut g_hud_snap_desc: D3D12_RESOURCE_DESC = unsafe { core::mem::zeroed() };
/// The engine's per-frame constants (b0 in the composite: 30 float4, the first the time the HUD flicker runs on),
/// captured by their unique 480-byte upload (std::atomic<ID3D12Resource*> g_frame_cb).
#[export_name = "?g_frame_cb@detail@cvr@@3U?$atomic@PEAUID3D12Resource@@@std@@A"]
pub static g_frame_cb: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
/// The composite's own constants (b6): std::atomic<ID3D12Resource*> g_hud_cb, and our own 512-byte copy
/// (ID3D12Resource* g_hud_cb_copy; uint8_t* g_hud_cb_copy_ptr).
#[export_name = "?g_hud_cb@detail@cvr@@3U?$atomic@PEAUID3D12Resource@@@std@@A"]
pub static g_hud_cb: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
#[export_name = "?g_hud_cb_copy@detail@cvr@@3PEAUID3D12Resource@@EA"]
pub static mut g_hud_cb_copy: *mut c_void = null_mut();
#[export_name = "?g_hud_cb_copy_ptr@detail@cvr@@3PEAEEA"]
pub static mut g_hud_cb_copy_ptr: *mut u8 = null_mut();
/// Profiler.cpp's HUD_REQUIRED_MASK_RVA (word_143C1C8A0).
pub static HUD_REQUIRED_MASK_RVA: usize = 0x3C1C8A0;

// ---- the node dispatcher, the frame graph and the producer skips (Mirror.cpp, Profiler.cpp, FrameGraph.cpp,
// SyncStereo.cpp, Dlss.cpp)
/// The second view's mirror copy: armed, and the serials of the second view's output and of the arming
/// (std::atomic<bool> g_mirror_copy_armed; std::atomic<uint64_t> g_mirror_vrcam_serial, g_mirror_armed_serial).
#[export_name = "?g_mirror_copy_armed@detail@cvr@@3U?$atomic@_N@std@@A"]
pub static g_mirror_copy_armed: AtomicBool = AtomicBool::new(false);
#[export_name = "?g_mirror_vrcam_serial@detail@cvr@@3U?$atomic@_K@std@@A"]
pub static g_mirror_vrcam_serial: AtomicU64 = AtomicU64::new(0);
#[export_name = "?g_mirror_armed_serial@detail@cvr@@3U?$atomic@_K@std@@A"]
pub static g_mirror_armed_serial: AtomicU64 = AtomicU64::new(0);
/// The HUD view-data getter (HudViewDataFn g_hud_viewdata_get, resolved on first use) and MAIN's draw-block list
/// captured at MAIN's own DrawHUD (void* g_hud_block_main).
#[export_name = "?g_hud_viewdata_get@detail@cvr@@3P6APEAXPEAX@ZEA"]
pub static mut g_hud_viewdata_get: usize = 0;
#[export_name = "?g_hud_block_main@detail@cvr@@3PEAXEA"]
pub static mut g_hud_block_main: *mut c_void = null_mut();
/// The engine's refcounted handle assign, sub_1407CDAE4 (HandleAssignFn g_handle_assign, set at install).
#[export_name = "?g_handle_assign@detail@cvr@@3P6APEAXPEAX0@ZEA"]
pub static mut g_handle_assign: usize = 0;
/// MAIN's draw-block list for the light-volume lend (void* g_main_block_v5; nothing writes it any more).
#[export_name = "?g_main_block_v5@detail@cvr@@3PEAXEA"]
pub static mut g_main_block_v5: *mut c_void = null_mut();
/// The second view's DLSS render and output sizes (volatile int32_t g_vrcam_dlss_rw, _rh, _ow, _oh).
#[export_name = "?g_vrcam_dlss_rw@detail@cvr@@3HC"]
pub static g_vrcam_dlss_rw: AtomicI32 = AtomicI32::new(0);
#[export_name = "?g_vrcam_dlss_rh@detail@cvr@@3HC"]
pub static g_vrcam_dlss_rh: AtomicI32 = AtomicI32::new(0);
#[export_name = "?g_vrcam_dlss_ow@detail@cvr@@3HC"]
pub static g_vrcam_dlss_ow: AtomicI32 = AtomicI32::new(0);
#[export_name = "?g_vrcam_dlss_oh@detail@cvr@@3HC"]
pub static g_vrcam_dlss_oh: AtomicI32 = AtomicI32::new(0);
/// Profiler.cpp's DRAWHUD_WORK_RVA: CRenderNode_DrawHUD.
pub static DRAWHUD_WORK_RVA: u32 = 0x33E388;

// ---- the executable (NodeDispatch.cpp)
/// uint8_t* g_exe_base: the game module's base, set by the stereo install.
#[export_name = "?g_exe_base@detail@cvr@@3PEAEEA"]
pub static mut g_exe_base: *mut u8 = null_mut();

// ---- the locks (formerly std::mutex)
#[no_mangle]
pub static g_stable_mtx: SrwLock = SrwLock::new();
#[no_mangle]
pub static g_own_target_mtx: SrwLock = SrwLock::new();
#[no_mangle]
pub static g_d12_mtx: SrwLock = SrwLock::new();
#[no_mangle]
pub static g_d12_copy_mtx: SrwLock = SrwLock::new();
#[no_mangle]
pub static g_hud_snap_mtx: SrwLock = SrwLock::new();

/// A lock taken (true) or released (false) across a call boundary, as the tables' adapters did with lock()/unlock().
fn hold(lock: &SrwLock, take: bool) {
    if take {
        core::mem::forget(lock.lock());
    } else {
        // SAFETY: the caller took it with hold(lock, true) on this thread.
        unsafe { lock.force_unlock() };
    }
}
pub extern "C" fn stable_lock(take: bool) {
    hold(&g_stable_mtx, take);
}
pub extern "C" fn own_target_lock(take: bool) {
    hold(&g_own_target_mtx, take);
}
pub extern "C" fn d12_lock(take: bool) {
    hold(&g_d12_mtx, take);
}
pub extern "C" fn hud_snap_lock(take: bool) {
    hold(&g_hud_snap_mtx, take);
}
pub extern "C" fn hud_snap_take() {
    hold(&g_hud_snap_mtx, true);
}
pub extern "C" fn hud_snap_release() {
    hold(&g_hud_snap_mtx, false);
}
/// g_d12_copy_mtx: tried (true, answers whether it was taken) or released (false).
pub extern "C" fn copy_lock(take: bool) -> bool {
    if take {
        return g_d12_copy_mtx.try_lock().map(core::mem::forget).is_some();
    }
    // SAFETY: taken by copy_lock(true) on this thread.
    unsafe { g_d12_copy_mtx.force_unlock() };
    true
}

// ---- the services
/// The render-mask table and its report (render_mask_report.rs).
pub use crate::render_mask_report::abi::{cvr_cpp_render_mask_count, cvr_cpp_render_mask_desc, cvr_cpp_render_mask_report};
thread_local! {
    /// t_vrcam_dlss_post (formerly DeviceHooks.cpp's): this command-list thread in the post-DLSS phase.
    static DLSS_POST: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
}
/// The current node's pose identity's view (formerly Dlss.cpp's cvr_cpp_node_view).
pub extern "C" fn cvr_cpp_node_view() -> u32 {
    let mut identity = core::mem::MaybeUninit::<crate::xr_capture::PoseIdentity>::zeroed();
    // SAFETY: an identity to fill (plain data, all zero the empty one).
    unsafe {
        cvr_pose_node_current(identity.as_mut_ptr().cast());
        identity.assume_init().view
    }
}
/// The post-DLSS phase marker set (formerly Dlss.cpp's cvr_cpp_set_dlss_post).
pub extern "C" fn cvr_cpp_set_dlss_post(on: bool) {
    DLSS_POST.with(|p| p.set(on));
}
unsafe extern "C" {
    /// The C++ half's services the tables hand on: Dlss.cpp's post-DLSS marker and node view, FrameGraph.cpp's
    /// upscaler gate.
    pub safe fn cvr_cpp_select_dlss(selected: bool);
    /// DeviceHooks.cpp's: the game's D3D12 device.
    pub static g_game_device: *mut c_void;
}
extern "C" {
    /// Profiler.cpp's: the stereo module's log (its switch and rate cap are one for the whole module).
    fn cvr_cpp_stereo_log(text: *const c_char);
    /// cvr-rig's image poses and pose identity: a resource's image pose recorded, this render thread's node pose.
    fn cvr_image_pose_record(a0: *mut c_void, a1: *mut c_void, a2: *const c_void, a3: bool);
    fn cvr_pose_node_current(out: *mut c_void);
    /// cvr-ffi's host.rs: the plugin's main log.
    fn cvr_log_write(text: *const c_char, len: usize);
    static CyberpunkVR_MirrorOutput: AtomicU32;
    static CyberpunkVR_StereoEyeCapture: AtomicI32;
}
pub extern "C" fn stereo_log(text: *const c_char) {
    // SAFETY: a NUL-terminated string from the caller.
    unsafe { cvr_cpp_stereo_log(text) }
}
/// `Log("%s", text)`: the plugin's main log, where [eyefin] has always gone.
pub extern "C" fn plain_log(text: *const c_char) {
    if text.is_null() {
        return;
    }
    // SAFETY: a NUL-terminated string from the caller.
    unsafe { cvr_log_write(text, core::ffi::CStr::from_ptr(text).to_bytes().len()) }
}
/// The stable snapshot's image pose: the given pose, or this render thread's node pose (formerly Capture.cpp's
/// cvr_cpp_record_stable_pose over camera::RecordImagePose).
pub extern "C" fn record_stable_pose(list: *mut c_void, stable: *mut c_void, pose: *const c_void) {
    /// cvr::camera::PoseIdentity's storage (136 bytes).
    #[repr(C, align(8))]
    struct Pose([u8; 136]);
    let mut current = Pose([0; 136]);
    let pose = if pose.is_null() {
        // SAFETY: a local the node pose is copied into.
        unsafe { cvr_pose_node_current((&raw mut current).cast()) };
        (&raw const current).cast()
    } else {
        pose
    };
    // SAFETY: the caller's live list and resource; a pose that outlives the call.
    unsafe { cvr_image_pose_record(list, stable, pose, true) }
}
/// stereo_eye_capture_wanted(): the desktop mirror or the OpenXR submit wants the second view's snapshot.
pub extern "C" fn capture_wanted() -> bool {
    // SAFETY: exports the plugin defines.
    unsafe { CyberpunkVR_MirrorOutput.load(Relaxed) != 0 || CyberpunkVR_StereoEyeCapture.load(Relaxed) != 0 }
}
/// The finished-frame slot (the render thread's final_res / final_state / final_list): read, or cleared.
pub extern "C" fn final_slot(clear: bool, res: *mut *mut c_void, state: *mut u32, list: *mut *mut c_void) {
    // SAFETY: this thread's block; the out pointers are the caller's locals.
    unsafe {
        let t = &mut *crate::render_tls::cvr_render_tls();
        if clear {
            t.final_res = null_mut();
            t.final_state = 0;
            t.final_list = null_mut();
            return;
        }
        *res = t.final_res;
        *state = t.final_state;
        *list = t.final_list;
    }
}
unsafe extern "C" fn read_desc(resource: *mut c_void, out: *mut c_void, _: *mut c_void) -> bool {
    // SAFETY: under the guarded call: a resource the caller names and its out block.
    unsafe {
        let Some(resource) = ID3D12Resource::from_raw_borrowed(&resource) else {
            return false;
        };
        *(out as *mut D3D12_RESOURCE_DESC) = resource.GetDesc();
    }
    true
}
/// mirror_get_resource_desc: the resource's description, read under SEH (false for null or a fault).
pub extern "C" fn get_desc(resource: *mut c_void, out: *mut D3D12_RESOURCE_DESC) -> bool {
    if resource.is_null() || out.is_null() {
        return false;
    }
    // SAFETY: read_desc under SEH; a fault answers None.
    unsafe { cvr_engine::code::guarded_call3(read_desc as *const () as usize, resource, out.cast(), null_mut()) == Some(true) }
}
/// The second view's output, published (publication.rs; main's 8255f379b): its identity kept in g_captured_vrcam_res.
pub static MIRROR_OUTPUT: crate::publication::Published = crate::publication::Published::new(&g_captured_vrcam_res);
/// The tonemap input capture (publication.rs; main's c41b2e23d).
pub static TM_INPUTS: crate::publication::TmInputs = crate::publication::TmInputs::new();
pub use crate::publication::TmEntry;

/// command_queue_original_execute(queue): the queue's own ExecuteCommandLists, past our hook.
pub extern "C" fn queue_original_execute(queue: *mut c_void) -> usize {
    // SAFETY: the caller's live queue.
    unsafe { crate::queue_dispatch::cvr_ecl_original(queue) }
}
/// hud_composite_params() into the blit's parameter block (33 floats).
pub extern "C" fn hud_params(out: *mut [f32; crate::xr_capture::HUD_PARAMS]) {
    // SAFETY: the caller's block, the size of HudParams.
    unsafe { crate::hud_composite::cvr_hud_composite_params(out.cast()) }
}
/// cname_hash("vr_optics_feed"): the optics auxiliary view, which may never become a source.
pub const OPTICS_FEED_HASH: u64 = {
    let s = b"vr_optics_feed";
    let mut h = 0xCBF2_9CE4_8422_2325u64;
    let mut i = 0;
    while i < s.len() {
        h = (h ^ s[i] as u64).wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    h
};
