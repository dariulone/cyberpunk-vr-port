//! The services the Rust solve (cvr-rig) asks of its host, formerly src/Anim/CharacterRigRust.cpp's: the wheel's
//! tracking frame and its UI gate, the frame's head orientation, the cutscene tier, the pose identity's diagnostics,
//! the log, the snap-window trace and the grip and config paths. tools/vrik_tests keep their own copies
//! (rust_glue/CharacterRigRust.cpp), so these live in the plugin's build only.
use core::ffi::{c_char, CStr};
use core::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::io::Write;
use std::sync::Mutex;

use cvr_xr::tracking::HeadPose;

#[allow(non_upper_case_globals)]
unsafe extern "C" {
    /// volatile int g_menuModeValue (src/Core/VrCore.cpp, C linkage).
    static g_menuModeValue: i32;
    /// The runtime diagnostics switch (cvr::RuntimeDiagnosticsEnabled).
    static CyberpunkVR_RuntimeDiagnostics: i32;
    /// The pose identity's diagnostics (cvr-rig's state_globals.rs).
    static CyberpunkVR_PoseIdVrikSource: AtomicU64;
    static CyberpunkVR_PoseIdVrikInputChanged: AtomicU64;
    /// OverlayIsVisible (cpp_services.rs).
    fn cvr_cpp_overlay_visible() -> bool;
    /// OpenXRManager::AcquireFrameHeadSample (cvr-xr).
    fn cvr_xr_acquire_frame_head_sample(
        out: *mut HeadPose,
        sequence: *mut u64,
        sampled_us: *mut u64,
        frame_time: *mut i64,
        prefer_native: bool,
    ) -> bool;
    /// OpenXRManager::GetGestureHandFrame's publication (cvr-xr): 1 the gesture hands, 2 the raw melee ones.
    fn cvr_xr_published_hand_frame(head: *mut HeadPose, hands: *mut HeadPose, which: i32, sequence: *mut u64, stamp_us: *mut u64) -> bool;
    /// cvr-rig's vrik_arming.rs.
    fn VRIK_EffectiveCutsceneSuspendTier() -> i32;
    /// The plugin's log (cvr-ffi's host.rs).
    fn cvr_log_write(text: *const c_char, len: usize);
    /// The plugin's folder `folder` (Core/Paths.hpp order: 2 grips, 4 config), narrow, with its trailing separator.
    fn cvr_paths_dir_a(folder: u32) -> *const c_char;
}

fn diagnostics() -> bool {
    // SAFETY: an export read whole.
    unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_RuntimeDiagnostics) != 0 }
}

/// cvr::anim::WheelTrackingFrame (include/Anim/TrackedWheelSteering.hpp; cvr-rig's wheel.rs mirrors it too).
#[repr(C)]
pub struct WheelTrackingFrame {
    head_ori: [f32; 4],
    head_pos: [f32; 3],
    /// Head-relative, right then left.
    hands: [[f32; 3]; 2],
    sequence: u64,
    origin: u64,
    stamp_us: u64,
    valid_hands: u8,
    head_valid: bool,
}
const _: () = assert!(
    core::mem::size_of::<WheelTrackingFrame>() == 88
        && core::mem::offset_of!(WheelTrackingFrame, hands) == 28
        && core::mem::offset_of!(WheelTrackingFrame, sequence) == 56
        && core::mem::offset_of!(WheelTrackingFrame, valid_hands) == 80
);

/// The wheel stands down while a menu or the VR overlay is up.
#[no_mangle]
pub extern "C" fn cvr_cpp_wheel_ui_blocks() -> bool {
    // SAFETY: a C global read whole.
    unsafe { core::ptr::read_volatile(&raw const g_menuModeValue) != 0 || cvr_cpp_overlay_visible() }
}

/// The head, then both hands head-relative, right then left (tracking publishes left then right); the hands' valid
/// bits are added to the frame's.
///
/// # Safety
/// `frame` is the caller's frame.
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_wheel_tracking_frame(frame: *mut WheelTrackingFrame) -> bool {
    let mut head = HeadPose::EMPTY;
    let mut hands = [HeadPose::EMPTY; 2];
    let (mut sequence, mut stamp) = (0u64, 0u64);
    // SAFETY: local outputs; the caller's frame.
    unsafe {
        if !cvr_xr_published_hand_frame(&mut head, hands.as_mut_ptr(), 1, &mut sequence, &mut stamp) {
            return false;
        }
        let f = &mut *frame;
        f.head_ori = head.ori;
        f.head_pos = head.pos;
        f.sequence = sequence;
        f.origin = head.origin_serial;
        f.stamp_us = stamp;
        f.head_valid = head.valid;
        for (i, hand) in [&hands[1], &hands[0]].into_iter().enumerate() {
            f.hands[i] = hand.pos;
            if hand.valid {
                f.valid_hands |= 1 << i;
            }
        }
    }
    true
}

/// The head orientation PatchCamera composes with in this aim epoch (x y z w, OpenXR axes); false without one.
///
/// # Safety
/// `out` holds 4 floats.
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_vrik_frame_head_orientation(out: *mut f32) -> bool {
    let mut head = HeadPose::EMPTY;
    // SAFETY: a local output; the caller's four floats.
    unsafe {
        let null = core::ptr::null_mut::<u64>();
        if !cvr_xr_acquire_frame_head_sample(&mut head, null, null, core::ptr::null_mut(), true) || !head.valid {
            return false;
        }
        core::ptr::copy_nonoverlapping(head.ori.as_ptr(), out, 4);
    }
    true
}

/// VRIK_EffectiveCutsceneSuspendTier.
#[no_mangle]
pub extern "C" fn cvr_cpp_pose_effective_cutscene_tier() -> i32 {
    // SAFETY: cvr-rig's own function.
    unsafe { VRIK_EffectiveCutsceneSuspendTier() }
}

/// The pose identity's diagnostics: the pose the solve read (0 when its input changed) and the input changes counted.
#[no_mangle]
pub extern "C" fn cvr_cpp_pose_id_note(same_input: bool, pose_id: u64) {
    // SAFETY: exports of cvr-rig, atomics.
    unsafe {
        if diagnostics() {
            CyberpunkVR_PoseIdVrikSource.store(if same_input { pose_id } else { 0 }, Relaxed);
        }
        if !same_input && diagnostics() {
            CyberpunkVR_PoseIdVrikInputChanged.fetch_add(1, Relaxed);
        }
    }
}

/// One line to the plugin's log, as Log("%s", text) wrote it.
///
/// # Safety
/// `text` is NUL-terminated or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_vrik_log(text: *const c_char) {
    // SAFETY: as documented; printf's "%s" of a null pointer prints "(null)".
    unsafe {
        let bytes = if text.is_null() { b"(null)".as_slice() } else { CStr::from_ptr(text).to_bytes() };
        cvr_log_write(bytes.as_ptr().cast(), bytes.len());
    }
}

/// The snap-window trace, log\cyberpunkvr_snapwin.log: truncated per session, written in text mode (LF as CRLF), flushed
/// per line; opening is tried again on the next line when it failed.
static SNAP_TRACE: Mutex<Option<std::fs::File>> = Mutex::new(None);

/// One line of the snap-window trace (HandSnapshot.cpp's VRIK_SnapTraceLog).
///
/// # Safety
/// `line` is NUL-terminated or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_vrik_snap_trace(line: *const c_char) {
    let mut trace = SNAP_TRACE.lock().unwrap_or_else(|p| p.into_inner());
    if trace.is_none() {
        let path = cvr_host::paths::file(cvr_host::paths::Folder::Log, "cyberpunkvr_snapwin.log");
        *trace = std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(path).ok();
    }
    let Some(file) = trace.as_mut() else { return };
    if line.is_null() {
        return;
    }
    // SAFETY: as documented.
    let bytes = unsafe { CStr::from_ptr(line) }.to_bytes();
    let mut text = Vec::with_capacity(bytes.len() + 8);
    for &b in bytes {
        if b == b'\n' {
            text.push(b'\r');
        }
        text.push(b);
    }
    let _ = file.write_all(&text);
    let _ = file.flush();
}

/// The plugin folder's file `name` (cvr::paths::FileA: the folder, then the name), NUL-terminated into `out`; false
/// (and `out` emptied) when it does not fit.
///
/// # Safety
/// `name` is NUL-terminated; `out` holds `size` bytes or is null.
unsafe fn folder_file(folder: u32, name: *const c_char, out: *mut c_char, size: usize) -> bool {
    if out.is_null() || size == 0 {
        return false;
    }
    // SAFETY: as documented; the folder's narrow name lives for the process.
    unsafe {
        let dir = CStr::from_ptr(cvr_paths_dir_a(folder)).to_bytes();
        let name = CStr::from_ptr(name).to_bytes();
        let length = dir.len() + name.len();
        if length == 0 || length + 1 > size {
            *out = 0;
            return false;
        }
        let out = out.cast::<u8>();
        core::ptr::copy_nonoverlapping(dir.as_ptr(), out, dir.len());
        core::ptr::copy_nonoverlapping(name.as_ptr(), out.add(dir.len()), name.len());
        *out.add(length) = 0;
    }
    true
}

/// A recorded grip's file (VRGripPath: the grips folder), NUL-terminated into `out`; false when it does not fit.
///
/// # Safety
/// As folder_file's.
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_vrik_grip_path(name: *const c_char, out: *mut c_char, size: usize) -> bool {
    // SAFETY: as documented.
    unsafe { folder_file(2, name, out, size) }
}

/// The config folder's file `name` (cvr::paths::FileA), NUL-terminated into `out`; false when it does not fit.
///
/// # Safety
/// As folder_file's.
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_vrik_config_file(name: *const c_char, out: *mut c_char, size: usize) -> bool {
    // SAFETY: as documented.
    unsafe { folder_file(4, name, out, size) }
}

/// The player pose's heading is ready (the head bone resolved by the arming; include/Anim/VrikState.hpp's
/// g_PlayerPoseHeadingReady in the reference build): set by cvr-rig's vrik_arming.rs, read by its roomscale_move.rs.
static POSE_HEADING_READY: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

#[no_mangle]
pub extern "C" fn cvr_cpp_set_pose_heading_ready(ready: bool) {
    POSE_HEADING_READY.store(ready, core::sync::atomic::Ordering::Release);
}

#[no_mangle]
pub extern "C" fn cvr_cpp_pose_heading_ready() -> bool {
    POSE_HEADING_READY.load(core::sync::atomic::Ordering::Acquire)
}
