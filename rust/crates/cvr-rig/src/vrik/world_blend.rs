//! WorldSpaceBlendCamera (formerly src/Hooks/WorldSpaceBlendCamera.cpp): the world-space blend camera's notification
//! (2.5 sub at 0xEBE7F6, `call [rax+240h]`) after it stored the pose it blended from MAIN's placed camera.
//!
//! The verified native update reads exactly MAIN's source (+0x1E0) before smoothing it, and that source already holds
//! the shared VR composition and MAIN's eye offset. So the completed write is COPIED -- position and rotation bytes,
//! and its pose label with them -- never a newer head located again, never the HMD added twice. The call site becomes
//! NOP + CALL into a relay that jumps to src/Hooks/CameraPoseLeafThunks.asm's CvrWorldBlendNotifyDetour, which saves
//! the volatile registers, calls CvrWorldBlendBeforeNotify here and then the original vtable+240 notification with
//! RAX, RCX and RDX unchanged; the engine rebuilds matrices and dirty state from these bytes.
use super::camera_sites::{site_matches, WORLD_BLEND_NOTIFY_CALL, WORLD_BLEND_SOURCE_READ, WORLD_BLEND_STORE};
use super::pose_identity::{cvr_pose_capture, cvr_pose_invalidate, cvr_pose_publish_serialized, PoseReceipt};
use super::roomscale_move::cvr_roomscale_player_identity;
use core::ffi::c_void;
use cvr_engine::code::{image_base, read, write};
use std::sync::atomic::{AtomicU64, Ordering};

extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static CyberpunkVR_WorldBlendPoseCalls: AtomicU64;
    static CyberpunkVR_WorldBlendPoseApplied: AtomicU64;
    static CyberpunkVR_WorldBlendPoseMissing: AtomicU64;
    static CyberpunkVR_WorldBlendPoseChanged: AtomicU64;
    fn cvr_cpp_menu_mode() -> i32;
    fn cvr_cpp_device_cam_active() -> bool;
    fn cvr_cpp_braindance_active() -> bool;
    fn cvr_cpp_xr_external_pose_reset_pending() -> bool;
    fn cvr_cpp_xr_tracking_origin_serial() -> u64;
    fn cvr_cpp_cam_obj_main() -> usize;
    fn CvrWorldBlendNotifyDetour();
}
#[link(name = "kernel32")]
extern "system" {
    fn VirtualProtect(address: *mut c_void, size: usize, protect: u32, old: *mut u32) -> i32;
    fn FlushInstructionCache(process: *mut c_void, address: *const c_void, size: usize) -> i32;
    fn GetCurrentProcess() -> *mut c_void;
}

fn count(counter: &AtomicU64) {
    // SAFETY: a plain exported int.
    if unsafe { CyberpunkVR_RuntimeDiagnostics } != 0 {
        counter.fetch_add(1, Ordering::Relaxed);
    }
}

/// The blend camera `component` is about to be notified of its new pose.
#[no_mangle]
pub unsafe extern "C" fn CvrWorldBlendBeforeNotify(component: usize) {
    // SAFETY: exported counters, C++ services and SEH reads and writes of the engine's camera.
    unsafe {
        count(&CyberpunkVR_WorldBlendPoseCalls);
        if component < 0x10000 {
            return;
        }
        cvr_pose_invalidate(component + 0xE0);
        if cvr_cpp_menu_mode() != 0 || cvr_cpp_device_cam_active() || cvr_cpp_braindance_active() || cvr_cpp_xr_external_pose_reset_pending() {
            return;
        }
        let player = cvr_roomscale_player_identity();
        if player == 0 {
            return;
        }
        let Some(source) = read::<usize>(component + 0x1E0) else { return };
        if source == 0 || source != cvr_cpp_cam_obj_main() {
            return;
        }
        if read::<usize>(component + 0x50) != Some(player) || read::<usize>(source + 0x50) != Some(player) {
            return;
        }
        let mut receipt = PoseReceipt::default();
        cvr_pose_capture(source + 0xE0, &mut receipt);
        if !receipt.valid()
            || receipt.payload.component != source
            || receipt.payload.view != 1
            || receipt.payload.head.origin_serial != cvr_cpp_xr_tracking_origin_serial()
        {
            count(&CyberpunkVR_WorldBlendPoseMissing);
            return;
        }
        // position at +0xE0, rotation at +0xF0: the fingerprint IS those bytes
        if !write(component + 0xE0, receipt.fingerprint.position) || !write(component + 0xF0, receipt.fingerprint.rotation) {
            return;
        }
        cvr_pose_publish_serialized(component + 0xE0, &receipt);
        let mut written = PoseReceipt::default();
        cvr_pose_capture(component + 0xE0, &mut written);
        if written.valid() && written.payload.write_id == receipt.payload.write_id {
            count(&CyberpunkVR_WorldBlendPoseApplied);
        } else {
            count(&CyberpunkVR_WorldBlendPoseChanged);
        }
    }
}

/// Installed from src/Hooks/WorldSpaceBlendCamera.cpp's registration (Boot, 82).
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_world_blend_install() -> bool {
    let exe = image_base();
    if exe == 0
        || !site_matches(exe, &WORLD_BLEND_NOTIFY_CALL)
        || !site_matches(exe, &WORLD_BLEND_SOURCE_READ)
        || !site_matches(exe, &WORLD_BLEND_STORE)
    {
        return false;
    }
    let call = exe + WORLD_BLEND_NOTIFY_CALL.rva;
    let Some(relay) = cvr_engine::trampoline::allocate(call, 14) else { return false };
    // jmp [rip+0] / the detour: an indirect jump keeps RAX, which holds the native vtable
    let mut bytes = [0u8; 14];
    bytes[..6].copy_from_slice(&[0xFF, 0x25, 0, 0, 0, 0]);
    bytes[6..].copy_from_slice(&(CvrWorldBlendNotifyDetour as *const () as u64).to_le_bytes());
    let distance = relay as i64 - (call as i64 + 6);
    if distance < i32::MIN as i64 || distance > i32::MAX as i64 {
        return false;
    }
    // SAFETY: arena memory, then the six-byte call site, made writable for the patch and restored.
    unsafe {
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), relay as *mut u8, bytes.len());
        let mut previous = 0u32;
        if VirtualProtect(call as *mut c_void, 6, 0x40, &mut previous) == 0 {
            return false;
        }
        // NOP + CALL keeps the native six-byte call's return address intact.
        let p = call as *mut u8;
        *p = 0x90;
        *p.add(1) = 0xE8;
        core::ptr::copy_nonoverlapping((distance as i32).to_le_bytes().as_ptr(), p.add(2), 4);
        let mut unused = 0u32;
        VirtualProtect(call as *mut c_void, 6, previous, &mut unused);
        FlushInstructionCache(GetCurrentProcess(), relay as *const c_void, 14);
        FlushInstructionCache(GetCurrentProcess(), call as *const c_void, 6);
    }
    true
}
