//! The peer eye's camera predicted at each traced camera upload (formerly src/Stereo/Grading.cpp's
//! PredictPeerCameraInput and TraceCameraUpload): for the stage-3 consumer's 848-byte camera upload, the other view's
//! camera input -- in a verified pair, MAIN's own current camera and its decoded history (the shader camera built from
//! them); otherwise VRCAM's rebuilt at MAIN's pose with MAIN's R2 jitter (the engine's rebuild between guard bands, its
//! asymmetric window when the projection is on), its shader camera built against MAIN's previous input, and the
//! twin's upload prepared when asked for --; the trace's camera row, and the early depth's and paired geometry's own
//! rows with their native inputs.
//!
//! The .cpp keeps the uploader detours and lends the stereo state, the native-input capture (its SEH unwind) and the
//! early depth's and paired geometry's camera hooks through the runtime.

use crate::single_pass_trace::Record;
use core::ffi::c_void;
use core::sync::atomic::{AtomicI32, AtomicPtr, AtomicU32, Ordering::*};

const CAMERA_BYTES: usize = 904;
const SHADER_CAMERA_BYTES: usize = 848;
/// view_layout: the camera input and its history at the view.
const VIEW_CAMERA: u64 = 0xE0;
const VIEW_HISTORY: u64 = VIEW_CAMERA + 4 * 0x3E0;
const HISTORY_BYTES: usize = 0x184;
/// CameraWordMask::All (212 words).
const ALL_WORDS: [u64; 4] = [u64::MAX, u64::MAX, u64::MAX, (1 << 20) - 1];

/// What the .cpp lends.
#[repr(C)]
pub struct Runtime {
    pub exe_base: extern "C" fn() -> u64,
    /// g_main_view_ctx.
    pub main_view: extern "C" fn() -> u64,
    /// early_depth::NeedsCamera, paired_geometry::NeedsCamera.
    pub early_needs_camera: extern "C" fn() -> bool,
    pub paired_needs_camera: extern "C" fn() -> bool,
    /// trace::CaptureNativeInputs (the guarded unwind).
    pub capture_native_inputs: extern "C" fn(*mut Record),
    /// early_depth::Camera, paired_geometry::Camera.
    pub early_camera: extern "C" fn(*const Record),
    pub paired_camera: extern "C" fn(*const Record),
    pub trace_state: *const AtomicU32,
    pub twin_upload: *const AtomicU32,
    /// CyberpunkVR_AsymProjection (projection::Active is 1).
    pub asym_projection: *const AtomicI32,
}
// SAFETY: immutable; its pointers name process-lifetime exports and functions.
unsafe impl Sync for Runtime {}
/// The plugin's table, built here (formerly Stereo/Grading.cpp's); a test program attaches its own.
#[cfg(feature = "rust-build")]
mod plugin {
    use super::{Record, Runtime};
    use core::sync::atomic::Ordering::Acquire;
    /// paired_geometry::NeedsCamera / Camera (paired_geometry_native.rs).
    extern "C" fn cvr_cpp_paired_needs_camera() -> bool {
        crate::paired_geometry_native::needs_camera()
    }
    extern "C" fn cvr_cpp_paired_camera(row: *const Record) {
        // SAFETY: the trace's row, valid for the call.
        if let Some(row) = unsafe { row.as_ref() } {
            crate::paired_geometry_native::camera(row);
        }
    }
    extern "C" fn exe_base() -> u64 {
        // SAFETY: the game module base, set once at the stereo install.
        unsafe { core::ptr::read_volatile(&raw const crate::stereo_shared::g_exe_base) as u64 }
    }
    extern "C" fn main_view() -> u64 {
        crate::moved_globals::g_main_view_ctx.load(Acquire) as u64
    }
    extern "C" fn capture_native_inputs(row: *mut Record) {
        // SAFETY: the caller's row.
        unsafe { crate::single_pass_trace::cvr_sp_trace_native_inputs(row) }
    }
    /// early_depth::Camera: the row's camera and its shader prediction.
    extern "C" fn early_camera(row: *const Record) {
        // SAFETY: the caller's row.
        let Some(r) = (unsafe { row.as_ref() }) else { return };
        // SAFETY: the row's two 848-byte cameras.
        unsafe {
            crate::early_depth_native::cvr_ned_camera(r.node, r.input_valid, r.index, r.side, r.view, r.reserved,
                                                      r.shader_prediction_valid, &r.camera, &r.shader_prediction)
        }
    }
    pub(super) static RT: Runtime = Runtime {
        exe_base,
        main_view,
        early_needs_camera: crate::early_depth_native::cvr_ned_needs_camera,
        paired_needs_camera: cvr_cpp_paired_needs_camera,
        capture_native_inputs,
        early_camera,
        paired_camera: cvr_cpp_paired_camera,
        trace_state: &crate::declared_moved_exports::CyberpunkVR_SinglePassTraceState,
        twin_upload: &crate::declared_moved_exports::CyberpunkVR_SinglePassTwinUpload,
        asym_projection: (&raw const crate::declared_moved_exports::CyberpunkVR_AsymProjection).cast(),
    };
}
#[cfg(feature = "rust-build")]
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new((&raw const plugin::RT).cast_mut());
#[cfg(not(feature = "rust-build"))]
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new(core::ptr::null_mut());
fn rt() -> Option<&'static Runtime> {
    // SAFETY: attached once with a static runtime.
    unsafe { RUNTIME.load(Acquire).as_ref() }
}
fn read(address: u64, out: &mut [u8]) -> bool {
    address != 0 && crate::geometry_packets::read(address, out)
}
fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}
/// PeekR2Jitter: REDengine's R2 branch without advancing its native counter (each product rounded before the add).
pub fn peek_r2_jitter(counter: u32, width: u32, height: u32) -> crate::camera_input::JitterSample {
    if width == 0 || height == 0 {
        return Default::default();
    }
    let component = |multiplier: f32| {
        let product = core::hint::black_box(counter as f32 * multiplier);
        let shifted = core::hint::black_box(product + 0.5);
        shifted.fract() - 0.5
    };
    let (pixel_x, pixel_y) = (component(0.754_877_686_500_549_3), component(0.569_840_312_004_089_4));
    crate::camera_input::JitterSample { pixel_x, pixel_y, clip_x: (pixel_x * 2.0) / width as f32, clip_y: (pixel_y * 2.0) / height as f32, phase: counter & 15 }
}
fn inverse(base: u64) -> crate::shader_camera::InvertShaderMatrix {
    // SAFETY: the engine's matrix inverse at its RVA.
    unsafe { core::mem::transmute((base + 0x297A1C) as usize) }
}
fn exclude(words: &mut [u64; 4], word: u32) {
    if word < 212 {
        words[(word / 64) as usize] &= !(1 << (word % 64));
    }
}
#[repr(C, align(16))]
struct Guarded {
    before: [u8; 32],
    camera: [u8; 912],
    after: [u8; 32],
}
/// PredictPeerCameraInput.
pub fn predict(row: &mut Record) {
    let Some(r) = rt() else { return };
    if row.node != 0x33DDA4 || row.input_valid == 0 {
        return;
    }
    let paired = (crate::preparation_trace::camera_trace() != 0 || (r.early_needs_camera)()) && crate::preparation_trace::pairing() && crate::preparation_trace::pair_state() == 2;
    if if paired { row.side != 0 } else { row.side != 1 } {
        return;
    }
    row.prediction_peer_side = paired.into();
    let peer = if paired { crate::preparation_trace::pair_view(3) } else { (r.main_view)() };
    let base = (r.exe_base)();
    if peer == 0 || base == 0 || peer == row.view || (paired && row.view != crate::preparation_trace::pair_view(4)) {
        row.reserved = 1;
        return;
    }
    if paired {
        let (mut current, mut check) = ([0u8; CAMERA_BYTES], [0u8; CAMERA_BYTES]);
        let (mut history, mut history_check) = ([0u8; HISTORY_BYTES], [0u8; HISTORY_BYTES]);
        if !read(peer + VIEW_CAMERA, &mut current)
            || !read(peer + VIEW_HISTORY, &mut history)
            || !read(peer + VIEW_CAMERA, &mut check)
            || check != current
            || !read(peer + VIEW_HISTORY, &mut history_check)
            || history_check != history
        {
            row.reserved = 13;
            return;
        }
        if u32_at(&current, 0x378) != row.width || u32_at(&current, 0x37C) != row.height || current[0x20..0x48] != row.input[0x20..0x48] {
            row.reserved = 14;
            return;
        }
        let (mut previous, mut weight) = ([0u8; CAMERA_BYTES], 0f32);
        // SAFETY: our buffers.
        if !unsafe { crate::shader_camera::cvr_scam_decode(current.as_ptr(), CAMERA_BYTES, &history, previous.as_mut_ptr(), &mut weight) } {
            row.reserved = 15;
            return;
        }
        // SAFETY: our buffers and the engine's inverse.
        if !unsafe { crate::shader_camera::cvr_scam_build(&row.camera, current.as_ptr(), previous.as_ptr(), CAMERA_BYTES, weight, Some(inverse(base)), &mut row.shader_prediction) } {
            row.reserved = 16;
            return;
        }
        (row.prediction, row.previous_input, row.prediction_counter) = (current, previous, u32_at(&current, 0x380));
        (row.prediction_valid, row.previous_input_valid, row.shader_prediction_valid) = (1, 1, 1);
        row.valid_shader_words = ALL_WORDS;
        row.reserved = 12;
        return;
    }
    let (mut flags, mut size, mut counter, mut halton) = ([0u8; 8], [0u8; 8], [0u8; 4], [[0u8; 1]; 3]);
    if !read(row.view.wrapping_add(0x25B8), &mut flags)
        || !read(peer + 0x34, &mut size)
        || !read(peer + 0x2450, &mut counter)
        || !read(peer + 0x2C50, &mut row.peer_source_pose)
        || !read(base + 0x392C5E8, &mut halton[0])
        || !read(base + 0x392C620, &mut halton[1])
        || !read(base + 0x392C658, &mut halton[2])
    {
        row.reserved = 2;
        return;
    }
    let flags = u64::from_le_bytes(flags);
    if flags & 0x40 == 0 || flags & 0x5700 != 0 || halton.iter().any(|h| h[0] != 0) {
        row.reserved = 3;
        return;
    }
    let (width, height, counter) = (u32_at(&size, 0), u32_at(&size, 4), u32::from_le_bytes(counter));
    if width != row.width || height != row.height {
        row.reserved = 4;
        return;
    }
    let jitter = peek_r2_jitter(counter, width, height);
    // SAFETY: the engine's camera rebuild at its RVA.
    let rebuild: crate::camera_input::RebuildCameraInput = unsafe { core::mem::transmute((base + 0x296EA0) as usize) };
    let mut local = Guarded { before: [0xA5; 32], camera: [0; 912], after: [0xA5; 32] };
    let (mut aspect, mut shift) = ([0u8; 4], [0u8; 8]);
    let asym = r.asym_projection.is_null() || unsafe { (*r.asym_projection).load(Relaxed) } == 1;
    let own = !r.asym_projection.is_null() && asym && read(peer + 0x2C78, &mut aspect) && read(peer + 0x2C80, &mut shift);
    let projection = crate::camera_input::PeerProjection { aspect: f32::from_le_bytes(aspect), shift_x: f32::from_le_bytes(shift[..4].try_into().unwrap()), shift_y: f32::from_le_bytes(shift[4..].try_into().unwrap()) };
    let camera: &mut [u8; CAMERA_BYTES] = (&mut local.camera[..CAMERA_BYTES]).try_into().unwrap();
    // SAFETY: the input's 904 bytes, the pose's 32, the engine's rebuild over our guarded camera.
    let prepared = unsafe { crate::camera_input::prepare_camera_at_pose(row.input.as_ptr(), row.peer_source_pose.as_ptr(), &jitter, width, height, Some(rebuild), camera, own.then_some(&projection)) };
    if !prepared {
        row.reserved = 5;
        return;
    }
    if local.before.iter().chain(&local.after).any(|&b| b != 0xA5) {
        row.reserved = 6;
        return;
    }
    row.prediction.copy_from_slice(&local.camera[..CAMERA_BYTES]);
    (row.prediction_counter, row.prediction_valid) = (counter, 1);
    let (mut previous, mut history_valid) = ([0u8; CAMERA_BYTES], [0u8; 1]);
    if !read(peer + VIEW_CAMERA, &mut previous) || !read(peer + VIEW_HISTORY, &mut history_valid) {
        row.reserved = 7;
        return;
    }
    (row.previous_input, row.previous_input_valid) = (previous, 1);
    if history_valid[0] == 0 {
        row.reserved = 8;
        return;
    }
    // SAFETY: our buffers and the engine's inverse.
    if unsafe { crate::shader_camera::cvr_scam_build(&row.camera, local.camera.as_ptr(), previous.as_ptr(), CAMERA_BYTES, 0.0, Some(inverse(base)), &mut row.shader_prediction) } {
        row.valid_shader_words = ALL_WORDS;
        exclude(&mut row.valid_shader_words, 147);
        row.shader_prediction[588..592].copy_from_slice(&0x7FC0_0000u32.to_le_bytes());
        row.shader_prediction_valid = 1;
        row.reserved = 11;
        // SAFETY: the runtime's export.
        let twin = !r.twin_upload.is_null() && unsafe { (*r.twin_upload).load(Relaxed) } != 0;
        if row.index != u32::MAX && twin {
            let first: &[u8; SHADER_CAMERA_BYTES] = &row.camera;
            let uploaded = crate::camera_upload::prepare(base, row.index, first, &row.shader_prediction);
            (row.twin_cpu_descriptor, row.twin_gpu_address, row.twin_bytes) = (uploaded.cpu_descriptor, uploaded.gpu_address, uploaded.bytes);
        }
    } else {
        row.reserved = 10;
    }
}
extern "C" fn predict_c(row: *mut Record) {
    // SAFETY: the trace's row.
    if let Some(row) = unsafe { row.as_mut() } {
        predict(row);
    }
}
fn row_of(node: u32, side: i32, index: u32, bytes: &[u8; SHADER_CAMERA_BYTES]) -> Box<Record> {
    // SAFETY: plain record.
    let mut row: Box<Record> = Box::new(unsafe { core::mem::zeroed() });
    (row.node, row.side, row.index) = (node, side, index);
    row.camera = *bytes;
    row
}
/// TraceCameraUpload: an 848-byte camera upload traced, and handed to the early depth and the paired geometry.
///
/// # Safety
/// `src` holds `size` readable bytes.
pub unsafe fn trace_camera_upload(index: u32, size: u32, src: *const c_void) {
    let Some(r) = rt() else { return };
    if size != 848 || src.is_null() {
        return;
    }
    let early = (r.early_needs_camera)();
    let paired = (r.paired_needs_camera)();
    // SAFETY: the runtime's export.
    let trace = !r.trace_state.is_null() && unsafe { (*r.trace_state).load(Relaxed) } == 1;
    if !early && !paired && !trace {
        return;
    }
    let base = (r.exe_base)();
    let work = crate::render_tls::current_node_work() as u64;
    let node = if base != 0 && work > base { (work - base) as u32 } else { 0 };
    let side = crate::render_tls::view_side();
    // SAFETY: as documented.
    let bytes = unsafe { &*src.cast::<[u8; SHADER_CAMERA_BYTES]>() };
    crate::single_pass_trace::camera(node, side, index, Some(bytes), Some(predict_c));
    if early {
        let mut row = row_of(node, side, index, bytes);
        (r.capture_native_inputs)(&mut *row);
        if row.input_valid != 0 {
            predict(&mut row);
        }
        (r.early_camera)(&*row);
    }
    if paired {
        let mut row = row_of(node, side, index, bytes);
        (r.capture_native_inputs)(&mut *row);
        (r.paired_camera)(&*row);
    }
}

// ---- the C ABI

/// # Safety
/// `runtime` lives for the process.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_pcam_attach(runtime: *const Runtime) {
    RUNTIME.store(runtime.cast_mut(), Release);
}
/// # Safety
/// As trace_camera_upload.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_pcam_trace(index: u32, size: u32, src: *const c_void) {
    unsafe { trace_camera_upload(index, size, src) };
}

#[cfg(test)]
mod tests {
    //! The R2 jitter's samples and phase, the shader word mask's exclusion, and the prediction's refusals before any
    //! engine read (another node, no input, the wrong side, no peer).
    use super::*;

    #[test]
    fn jitter_words_and_refusals() {
        let j = peek_r2_jitter(3, 1920, 1080);
        assert!((j.pixel_x - ((3.0f32 * 0.754_877_686_500_549_3 + 0.5).fract() - 0.5)).abs() < 1e-7 && j.phase == 3);
        assert!((j.clip_y - j.pixel_y * 2.0 / 1080.0).abs() < 1e-9);
        assert_eq!(peek_r2_jitter(5, 0, 1080).phase, 0, "no size: no sample");
        let mut words = ALL_WORDS;
        exclude(&mut words, 147);
        assert_eq!(words[2], !(1u64 << 19));
        exclude(&mut words, 300);
        assert_eq!(words[3], (1 << 20) - 1, "past the mask: ignored");
        extern "C" fn yes() -> bool {
            true
        }
        extern "C" fn no() -> bool {
            false
        }
        extern "C" fn zero() -> u64 {
            0
        }
        extern "C" fn none(_: *mut Record) {}
        extern "C" fn none_const(_: *const Record) {}
        static RT: Runtime = Runtime {
            exe_base: zero,
            main_view: zero,
            early_needs_camera: no,
            paired_needs_camera: yes,
            capture_native_inputs: none,
            early_camera: none_const,
            paired_camera: none_const,
            trace_state: core::ptr::null(),
            twin_upload: core::ptr::null(),
            asym_projection: core::ptr::null(),
        };
        unsafe { cvr_pcam_attach(&RT) };
        // The render thread's state: the second view, no node.
        // SAFETY: this thread's block.
        unsafe {
            (*crate::render_tls::cvr_render_tls()).view_side = 1;
            (*crate::render_tls::cvr_render_tls()).current_node_work = 0;
        }
        let mut row: Box<Record> = Box::new(unsafe { core::mem::zeroed() });
        (row.node, row.input_valid, row.side) = (0x1234, 1, 1);
        predict(&mut row);
        assert_eq!((row.reserved, row.prediction_peer_side), (0, 0), "another node: untouched");
        row.node = 0x33DDA4;
        row.side = 0;
        predict(&mut row);
        assert_eq!(row.reserved, 0, "unpaired: only the second view predicts");
        row.side = 1;
        predict(&mut row);
        assert_eq!(row.reserved, 1, "no MAIN view to read");
    }
}
