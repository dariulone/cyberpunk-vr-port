//! Framegen's inputs: each eye's depth and motion vectors and the camera they were rendered with, copied while the
//! engine owns them and handed to the generator once the GPU has them (formerly src/Framegen/Inputs.cpp and
//! NativeInputs.cpp; the files keep the exports, the C++ names as forwarders, `AcquireInputs`' shared pointer over a
//! record here, and two adapters for the C++ half's thread-locals: the node pose and the render node this thread works).
//!
//! Two sources. With DLSS the upscaler, Streamline's tags (the tag hook calls `RecordTags`) and constants
//! (slSetConstants, hooked here) give the inputs and the camera, and slEvaluateFeature (hooked here, through engine
//! stereo's per-eye DLSS) completes a record. Otherwise the engine's TAA and temporal upscaler nodes (FSR 2, FSR 3 and
//! the FidelityFX API, XeSS) are observed: their depth and velocity handles as the node dispatcher binds them, the
//! resources behind them from the frame graph's resource table and their states from the command context's ledger, the
//! camera from the view itself; TAA and FSR 2 are captured at the node's next draw or dispatch, the libraries when
//! they are called.
//!
//! A record per eye and rendered frame, from two pools of six (three generation slots, the displayed endpoint, the
//! newest capture and one producer); the copies are recorded on the engine's list outside the lock and the record
//! completes when every list that wrote it has been submitted (`Submitted`, which also signals the record's fence).
//! Engine stereo renders both eyes into MAIN: its inputs are recorded as two, each half with its eye's camera.
#![allow(unsafe_op_in_unsafe_fn)]
use cvr_host::fast::FastMap;
use crate::asym_projection;
use crate::engine_stereo;
use crate::engine_stereo_dlss::{self, ConstantsFn, EvaluateFn};
use crate::streamline::{self as sl, SlResult};
use core::cell::{Cell, UnsafeCell};
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering::*};
use cvr_crt::{format, Arg};
use cvr_engine::code;
use cvr_gpu::frame_capture::CameraData;
use cvr_gpu::framegen_core;
use cvr_gpu::input_copy::{cvr_gpu_framegen_copy_input, InputSlots, InputTag};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, Weak};
use windows::core::{s, w, Interface};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};

#[allow(non_upper_case_globals)]
extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static CyberpunkVR_VrcamDlssViewport: i32;
    static CyberpunkVR_FramegenInputStages: [AtomicU64; 10];
    static mut CyberpunkVR_FramegenCameraDelta: [[f32; 6]; 2];
    static CyberpunkVR_FramegenNativeStages: [AtomicU64; 4];
    static CyberpunkVR_FramegenNativeDebug: [[AtomicU64; 8]; 2];
    static CyberpunkVR_FramegenNativeSource: AtomicU32;
    /// src/Framegen/Inputs.cpp: the node pose's fields read here (cvr::camera::CurrentNodePoseIdentity).
    fn cvr_cpp_framegen_node_pose(out: *mut NodePose);
    /// src/Framegen/Inputs.cpp: g_verboseLog.
    fn cvr_cpp_framegen_verbose() -> bool;
}
#[allow(non_snake_case)]
extern "system" {
    fn MH_CreateHook(target: *mut c_void, detour: *mut c_void, original: *mut *mut c_void) -> i32;
    fn MH_EnableHook(target: *mut c_void) -> i32;
    fn MH_DisableHook(target: *mut c_void) -> i32;
    fn MH_RemoveHook(target: *mut c_void) -> i32;
}

/// The node pose as the C++ adapter hands it over.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct NodePose {
    pub pose_id: u64,
    /// head.originSerial.
    pub origin: u64,
    pub view: u32,
    pub render_frame_id: u32,
    /// `bool(pose)`: poseId, writeId and head.valid.
    pub valid: bool,
    pub has_render_frame_id: bool,
}
fn node_pose() -> NodePose {
    let mut p = NodePose::default();
    // SAFETY: the C++ adapter writes the struct.
    unsafe { cvr_cpp_framegen_node_pose(&mut p) };
    p
}
fn diagnostics() -> bool {
    // SAFETY: an export of the C++ half.
    unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_RuntimeDiagnostics) != 0 }
}
/// CVR_DIAGNOSTIC(++CyberpunkVR_FramegenInputStages[i]).
fn stage(i: usize) {
    if diagnostics() {
        // SAFETY: an export of the C++ half.
        unsafe { CyberpunkVR_FramegenInputStages[i].fetch_add(1, Relaxed) };
    }
}
fn log(text: &str) {
    cvr_host::log::write(text);
}
fn capture_inputs_enabled() -> bool {
    framegen_core::cvr_framegen_capture_inputs_enabled()
}
fn motion_wanted() -> bool {
    framegen_core::cvr_framegen_motion_wanted()
}

/// include/Framegen/Inputs.hpp's Inputs (each `ComPtr` one owning pointer).
#[repr(C)]
#[derive(Clone)]
pub struct Inputs {
    pub depth: Option<ID3D12Resource>,
    pub motion: Option<ID3D12Resource>,
    pub ready_fence: Option<ID3D12Fence>,
    pub queue: *mut c_void,
    pub camera: CameraData,
    pub pose_id: u64,
    pub origin: u64,
    pub fence_value: u64,
    pub bytes: u64,
    pub frame_id: u32,
    pub view: u32,
    pub width: u32,
    pub height: u32,
    pub motion_width: u32,
    pub motion_height: u32,
    pub has_depth: bool,
    pub has_motion: bool,
    pub submitted: bool,
    pub evaluated: bool,
    pub native: bool,
}
const _: () = assert!(
    core::mem::size_of::<Inputs>() == 192
        && core::mem::offset_of!(Inputs, camera) == 32
        && core::mem::offset_of!(Inputs, pose_id) == 128
        && core::mem::offset_of!(Inputs, frame_id) == 160
        && core::mem::offset_of!(Inputs, has_depth) == 184
        && core::mem::offset_of!(Inputs, native) == 188
);
impl Default for Inputs {
    fn default() -> Self {
        Inputs {
            depth: None,
            motion: None,
            ready_fence: None,
            queue: core::ptr::null_mut(),
            camera: CameraData::default(),
            pose_id: 0,
            origin: 0,
            fence_value: 0,
            bytes: 0,
            frame_id: 0,
            view: 0,
            width: 0,
            height: 0,
            motion_width: 0,
            motion_height: 0,
            has_depth: false,
            has_motion: false,
            submitted: false,
            evaluated: false,
            native: false,
        }
    }
}

#[repr(C)]
#[derive(Default)]
struct RecordState {
    // First: a record's address is its inputs' (AcquireInputs hands the inputs out, the release takes them back).
    data: Inputs,
    generation: u64,
    expected: u8,
    submitted: u8,
    invalid: bool,
}
/// A record: written under STATE's lock only; the C++ half reads its inputs through the pointer AcquireInputs gave.
#[repr(C)]
#[derive(Default)]
pub struct Record(UnsafeCell<RecordState>);
// SAFETY: D3D12 objects are free-threaded; the record is written under STATE's lock.
unsafe impl Send for Record {}
unsafe impl Sync for Record {}
impl Record {
    #[allow(clippy::mut_from_ref)]
    fn get(&self) -> &mut RecordState {
        // SAFETY: called with STATE's lock held.
        unsafe { &mut *self.0.get() }
    }
}

#[derive(Clone, Copy, Default)]
struct Constants {
    camera: CameraData,
    frame: u32,
    valid: bool,
    pose: u64,
    origin: u64,
}
struct Link {
    record: Weak<Record>,
    generation: u64,
    mask: u8,
}
struct State {
    cameras: [[Constants; 8]; 2],
    // The native consumers' camera (TAA and the upscaler nodes), read from the view itself.
    view_cameras: [[Constants; 8]; 2],
    // Three GPU generation slots, the displayed endpoint, the newest capture and one producer slot. Allocation is
    // lazy; completed generation leases retire every display cycle, including when no new matched input has arrived.
    pools: [[Option<Arc<Record>>; 6]; 2],
    pending: FastMap<usize, Vec<Link>>,
    next_generation: u64,
}
static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| {
    Mutex::new(State {
        cameras: Default::default(),
        view_cameras: Default::default(),
        pools: Default::default(),
        pending: cvr_host::fast::fast_map(),
        next_generation: 0,
    })
});
fn lock() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}
static HAVE_PENDING: AtomicBool = AtomicBool::new(false);
static HOOKS_INSTALLED: AtomicBool = AtomicBool::new(false);
static LOGGED: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
static ORIGINAL_CONSTANTS: AtomicUsize = AtomicUsize::new(0);
static ORIGINAL_EVALUATE: AtomicUsize = AtomicUsize::new(0);

fn valid_camera(c: &CameraData) -> bool {
    if !c.camera_motion
        || c.motion3d
        || !c.near_plane.is_finite()
        || c.near_plane <= 0.0
        || !(c.far_plane > c.near_plane)
        || !c.vertical_fov.is_finite()
        || c.vertical_fov <= 0.0
        || c.vertical_fov >= 3.13
        || !c.aspect.is_finite()
        || c.aspect <= 0.0
        || !c.motion_scale[0].is_finite()
        || !c.motion_scale[1].is_finite()
        || !c.jitter[0].is_finite()
        || !c.jitter[1].is_finite()
    {
        return false;
    }
    (0..3).all(|i| {
        c.position[i].is_finite()
            && c.up[i].is_finite()
            && c.right[i].is_finite()
            && c.forward[i].is_finite()
    })
}

// Engine stereo: MAIN's inputs are recorded per eye (ENGINE STEREO: ONE FRAME, TWO EYES below).
fn split_eyes(pose: &NodePose) -> bool {
    pose.view == 1 && engine_stereo::enabled() && engine_stereo::rendering()
}

/// Streamline's constants as a camera, and the frame token they were given for (written once the structure checks
/// pass, as the C++ did).
unsafe fn camera(
    p: *const sl::Constants,
    token: *const c_void,
    out: &mut CameraData,
    frame: &mut u32,
) -> bool {
    if p.is_null() || token.is_null() {
        return false;
    }
    let Some(p) = code::read::<sl::Constants>(p as usize) else {
        return false;
    };
    if p.base.struct_type != sl::CONSTANTS || p.base.struct_version < 1 || p.base.struct_version > 2
    {
        return false;
    }
    *frame = engine_stereo_dlss::frame_value(token);
    let mut c = CameraData {
        jitter: [p.jitter_offset.x, p.jitter_offset.y],
        motion_scale: [p.mvec_scale.x, p.mvec_scale.y],
        position: [p.camera_pos.x, p.camera_pos.y, p.camera_pos.z],
        up: [p.camera_up.x, p.camera_up.y, p.camera_up.z],
        right: [p.camera_right.x, p.camera_right.y, p.camera_right.z],
        forward: [p.camera_fwd.x, p.camera_fwd.y, p.camera_fwd.z],
        near_plane: p.camera_near,
        far_plane: p.camera_far,
        vertical_fov: p.camera_fov,
        aspect: p.camera_aspect_ratio,
        ..Default::default()
    };
    let projection_y = p.camera_view_to_clip[1].y.abs();
    if projection_y.is_finite() && projection_y > 0.001 {
        c.vertical_fov = 2.0 * (1.0 / projection_y).atan();
    }
    c.inverted = p.depth_inverted == sl::TRUE;
    c.infinite = c.far_plane.is_infinite();
    c.jittered = p.motion_vectors_jittered == sl::TRUE;
    c.reset = p.reset == sl::TRUE;
    c.camera_motion = p.camera_motion_included == sl::TRUE;
    c.motion3d = p.motion_vectors_3d == sl::TRUE;
    const INVALID: u8 = 2;
    if p.depth_inverted == INVALID
        || p.reset == INVALID
        || p.orthographic_projection != sl::FALSE
        || !valid_camera(&c)
    {
        return false;
    }
    *out = c;
    true
}

unsafe extern "C" fn set_constants(
    input: *const sl::Constants,
    frame: *const c_void,
    view: *const sl::ViewportHandle,
) -> SlResult {
    engine_stereo_dlss::cvr_engine_stereo_dlss_record_constants(input, frame, view);
    let original: ConstantsFn = core::mem::transmute(ORIGINAL_CONSTANTS.load(Acquire));
    let result = original(input, frame, view);
    if !capture_inputs_enabled() {
        return result;
    }
    let viewport = (*view).value;
    let side: i32 =
        if viewport == core::ptr::read_volatile(&raw const CyberpunkVR_VrcamDlssViewport) as u32 {
            1
        } else if viewport == 0 {
            0
        } else {
            -1
        };
    let pose = node_pose();
    if result == sl::RESULT_OK
        && capture_inputs_enabled()
        && (0..2).contains(&side)
        && pose.valid
        && pose.has_render_frame_id
        && pose.view == (side + 1) as u32
    {
        let mut value = Constants::default();
        value.valid = camera(input, frame, &mut value.camera, &mut value.frame);
        static DIAGNOSTIC_COUNT: AtomicU32 = AtomicU32::new(0);
        if diagnostics() && DIAGNOSTIC_COUNT.fetch_add(1, SeqCst) < 4 {
            let i = &*input;
            let line = format(
                c"[framegen] constants viewport=%u valid=%d frame=%u version=%llu near=%.6f far=%.3f fov=%.5f py=%.5f motion=(%.6f,%.6f) flags=%d/%d/%d/%d\n",
                &[
                    Arg::U(viewport as u64),
                    Arg::I(value.valid as i32),
                    Arg::U(value.frame as u64),
                    Arg::U(i.base.struct_version as u64),
                    Arg::F(i.camera_near as f64),
                    Arg::F(i.camera_far as f64),
                    Arg::F(i.camera_fov as f64),
                    Arg::F(i.camera_view_to_clip[1].y as f64),
                    Arg::F(i.mvec_scale.x as f64),
                    Arg::F(i.mvec_scale.y as f64),
                    Arg::I(i.depth_inverted as i8 as i32),
                    Arg::I(i.camera_motion_included as i8 as i32),
                    Arg::I(i.motion_vectors_3d as i8 as i32),
                    Arg::I(i.reset as i8 as i32),
                ],
            );
            log(&line.to_string_lossy());
        }
        value.frame = pose.render_frame_id;
        value.pose = pose.pose_id;
        value.origin = pose.origin;
        if value.valid {
            let mut s = lock();
            // Detour_DlssConst records the native camera immediately before slSetConstants. The scalar SL adapter has
            // no centre-shift field; do not overwrite the same camera's asymmetric shift with zeros.
            let native = s.cameras[side as usize][(value.frame % 8) as usize];
            if native.valid
                && native.frame == value.frame
                && native.pose == value.pose
                && native.origin == value.origin
            {
                value.camera.shift = native.camera.shift;
            } else if asym_projection::active() {
                value.valid = false;
            }
            if value.valid {
                s.cameras[side as usize][(value.frame % 8) as usize] = value;
            }
        }
    }
    result
}

unsafe extern "C" fn evaluate(
    feature: sl::Feature,
    frame: *const c_void,
    inputs: *const *const sl::BaseStructure,
    count: u32,
    list: *mut c_void,
) -> SlResult {
    let original_evaluate: EvaluateFn = core::mem::transmute(ORIGINAL_EVALUATE.load(Acquire));
    let original_constants: ConstantsFn = core::mem::transmute(ORIGINAL_CONSTANTS.load(Acquire));
    // Engine stereo: DLSS per eye (cvr-render engine_stereo_dlss) instead of one evaluate over both eyes.
    let mut result: SlResult = 0;
    if !engine_stereo_dlss::cvr_engine_stereo_dlss_evaluate(
        feature,
        frame,
        inputs,
        count,
        list,
        Some(original_evaluate),
        Some(original_constants),
        &mut result,
    ) {
        result = engine_stereo_dlss::cvr_engine_stereo_dlss_evaluate_original(
            feature,
            frame,
            inputs,
            count,
            list,
            original_evaluate,
        );
    }
    // A camera-scope completion is not a completed headset eye.
    if crate::scope_dlss::is_scope_evaluation(feature as u32, engine_stereo_dlss::frame_value(frame), inputs.cast(), count, result as i32) {
        return result;
    }
    if !capture_inputs_enabled() {
        return result;
    }
    let pose = node_pose();
    if capture_inputs_enabled()
        && result == sl::RESULT_OK
        && (feature == sl::FEATURE_DLSS || feature == sl::FEATURE_DLSS_RR)
        && pose.valid
        && pose.has_render_frame_id
        && (1..=2).contains(&pose.view)
        && !list.is_null()
    {
        let mut guard = lock();
        let s = &mut *guard;
        // Engine stereo: MAIN's evaluate completes both eyes' records.
        let first = pose.view as usize - 1;
        let last = if split_eyes(&pose) { 1 } else { first };
        for side in first..=last {
            for item in s.pools[side].iter().flatten() {
                let r = item.get();
                if !r.invalid
                    && r.data.frame_id == pose.render_frame_id
                    && r.data.pose_id == pose.pose_id
                    && r.data.origin == pose.origin
                    && r.data.has_depth
                    && (!motion_wanted() || r.data.has_motion)
                    && !r.data.submitted
                    && !r.data.evaluated
                {
                    r.data.evaluated = true;
                    r.expected |= 4;
                    stage(7);
                    s.pending.entry(list as usize).or_default().push(Link {
                        record: Arc::downgrade(item),
                        generation: r.generation,
                        mask: 4,
                    });
                    HAVE_PENDING.store(true, Release);
                }
            }
        }
    }
    result
}

/// A Streamline tag as framegen's input tag: a 2D texture's depth (0) or motion (1), its state and rectangle.
fn read_tag(p: usize) -> Option<InputTag> {
    let tag = code::read::<sl::ResourceTag>(p)?;
    if tag.base.struct_type != sl::RESOURCE_TAG
        || tag.base.struct_version != 1
        || tag.resource.is_null()
    {
        return None;
    }
    let resource = code::read::<sl::Resource>(tag.resource as usize)?;
    if resource.base.struct_type != sl::RESOURCE
        || resource.base.struct_version != 1
        || resource.ty != 0
    {
        return None;
    }
    let out = InputTag {
        resource: resource.native,
        state: resource.state,
        kind: tag.ty,
        x: tag.extent.left,
        y: tag.extent.top,
        width: tag.extent.width,
        height: tag.extent.height,
    };
    (!out.resource.is_null() && (out.kind == 0 || out.kind == 1) && out.state != u32::MAX)
        .then_some(out)
}
fn empty_tag() -> InputTag {
    InputTag {
        resource: core::ptr::null_mut(),
        state: 0,
        kind: 0,
        x: 0,
        y: 0,
        width: 0,
        height: 0,
    }
}

/// ReadRenderFrameIndex: the native renderer's frame clock (incremented at RVA294055 and read by temporal passes,
/// e.g. RVA788104). The per-view +1A0 token Streamline uses is permanently zero for the RTT view, so it cannot
/// identify its texture history; the common clock is read, never modified.
///
/// # Safety
/// `index` is null or a u32.
#[no_mangle]
pub unsafe extern "C" fn cvr_framegen_read_render_frame_index(index: *mut u32) -> bool {
    let base = code::image_base();
    if index.is_null() || base == 0 {
        return false;
    }
    let Some(renderer) =
        code::read::<usize>(base + cvr_engine::addresses::rva::RENDERER_GLOBAL_RVA)
    else {
        return false;
    };
    if renderer == 0 {
        return false;
    }
    match code::read::<u32>(renderer + 0x4CC8) {
        Some(v) => {
            *index = v;
            true
        }
        None => false,
    }
}

/// The exact source fields native0x78933C copies into sl::Constants. Its last-frame gate skips slSetConstants when
/// VRCAM's native token stays at zero, though staging already contains that view's camera.
fn native_camera(state: usize, view: u32) -> Option<CameraData> {
    if state == 0 {
        return None;
    }
    let mut p = [0u8; 0x1F4];
    if !code::read_bytes(state, &mut p) {
        return None;
    }
    let f = |o: usize| f32::from_le_bytes([p[o], p[o + 1], p[o + 2], p[o + 3]]);
    let mut c = CameraData {
        position: [f(0x1A0), f(0x1A4), f(0x1A8)],
        up: [f(0x1AC), f(0x1B0), f(0x1B4)],
        right: [f(0x1B8), f(0x1BC), f(0x1C0)],
        forward: [f(0x1C4), f(0x1C8), f(0x1CC)],
        near_plane: f(0x1D0),
        far_plane: f(0x1D4),
        aspect: f(0x1DC),
        jitter: [f(0x1E0), f(0x1E4)],
        motion_scale: [f(0x1E8), f(0x1EC)],
        ..Default::default()
    };
    let projection_y = f(0xB4).abs();
    c.vertical_fov = if projection_y > 0.001 {
        2.0 * (1.0 / projection_y).atan()
    } else {
        0.0
    };
    // The view's off-centre window, as the producer gave it (eye 1 = MAIN = side 0).
    if let Some(window) = asym_projection::view_window_of(view as i32 - 1) {
        c.shift = [window.shift_x, window.shift_y];
    }
    c.inverted = p[0x1F1] != 0;
    c.camera_motion = p[0x1F3] != 0;
    c.reset = p[0x1F0] == 0;
    c.infinite = c.far_plane.is_infinite();
    c.motion3d = code::read::<u8>(code::image_base() + 0x3914B80)? != 0;
    valid_camera(&c).then_some(c)
}

/// The scene camera descriptor in a view (include/Render/ViewCameraLayout.hpp kCamera) and the history flag (kHistory).
const VIEW_CAMERA: usize = 0xE0;
const VIEW_HISTORY: usize = 0x1060;
/// The fields SetStreamlineConstants (native0x788A9C) copies from the view, read at their source: Streamline is not
/// initialised as an upscaler everywhere (AMD), and the native consumers must not depend on it.
fn view_camera(view: usize) -> Option<CameraData> {
    if view == 0 {
        return None;
    }
    let mut d = [0u8; 0x378];
    if !code::read_bytes(view + VIEW_CAMERA, &mut d) {
        return None;
    }
    let f = |o: usize| f32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]]);
    let i = |o: usize| i32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]]);
    let mut c = CameraData::default();
    for k in 0..3 {
        c.position[k] = i(4 * k) as f32 * (1.0 / 131072.0); // 1/131072 m units
    }
    c.right = [f(0x250), f(0x254), f(0x258)];
    c.forward = [f(0x260), f(0x264), f(0x268)];
    c.up = [f(0x270), f(0x274), f(0x278)];
    c.near_plane = f(0x40);
    c.far_plane = f(0x44);
    c.aspect = f(0x28);
    c.shift = [f(0x30), f(0x34)]; // the descriptor's own centre shift
    if !c.shift[0].is_finite()
        || !c.shift[1].is_finite()
        || c.shift[0].abs() >= 1.0
        || c.shift[1].abs() >= 1.0
    {
        c.shift = [0.0, 0.0];
    }
    let degrees = f(0x20);
    let jitter_y = f(0x374);
    c.vertical_fov = degrees * (3.14159265f32 / 180.0); // what FSR 3 is given too
    c.jitter = [f(0x370), -jitter_y];
    let base = code::image_base();
    c.motion_scale = [
        code::read::<f32>(base + 0x3BFA1E0)?,
        code::read::<f32>(base + 0x3BFA230)?,
    ];
    c.reset = code::read::<u8>(view + VIEW_HISTORY)? == 0;
    c.inverted = true;
    c.camera_motion = true;
    c.infinite = c.far_plane.is_infinite();
    c.motion3d = code::read::<u8>(base + 0x3914B80)? != 0;
    valid_camera(&c).then_some(c)
}
/// std::max's choice: the second only when the first is less.
fn cmax(a: f32, b: f32) -> f32 {
    if a < b {
        b
    } else {
        a
    }
}
fn largest(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b)
        .fold(0.0, |d, (x, y)| cmax(d, (x - y).abs()))
}
/// The view camera against Streamline's for the same eye and frame, when both exist (NVIDIA), into
/// CyberpunkVR_FramegenCameraDelta. Called with the lock held.
fn compare(s: &State, side: usize, frame: u32) {
    let a = &s.cameras[side][(frame % 8) as usize];
    let b = &s.view_cameras[side][(frame % 8) as usize];
    if !a.valid || !b.valid || a.frame != frame || b.frame != frame || a.pose != b.pose {
        return;
    }
    let (x, y) = (&a.camera, &b.camera);
    let out = [
        largest(&x.position, &y.position),
        cmax(
            cmax(largest(&x.up, &y.up), largest(&x.right, &y.right)),
            largest(&x.forward, &y.forward),
        ),
        cmax(
            (x.near_plane - y.near_plane).abs(),
            (x.far_plane - y.far_plane).abs(),
        ),
        (x.vertical_fov - y.vertical_fov).abs(),
        largest(&x.jitter, &y.jitter),
        largest(&x.motion_scale, &y.motion_scale),
    ];
    // SAFETY: an export of the C++ half, written under the lock.
    unsafe { (*(&raw mut CyberpunkVR_FramegenCameraDelta))[side] = out };
}

/// RecordNativeCamera: the camera Detour_DlssConst finds in the engine's staging for `view` (1 MAIN, 2 the second eye).
#[no_mangle]
pub extern "C" fn cvr_framegen_record_native_camera(state: *mut c_void, view: u32) {
    if !capture_inputs_enabled() || !(1..=2).contains(&view) {
        return;
    }
    let pose = node_pose();
    if !pose.valid || !pose.has_render_frame_id || pose.view != view {
        return;
    }
    let camera = native_camera(state as usize, view);
    let value = Constants {
        camera: camera.unwrap_or_default(),
        frame: pose.render_frame_id,
        valid: camera.is_some(),
        pose: pose.pose_id,
        origin: pose.origin,
    };
    if value.valid {
        let mut s = lock();
        s.cameras[view as usize - 1][(value.frame % 8) as usize] = value;
        compare(&s, view as usize - 1, value.frame);
    }
}
/// RecordViewCamera: the camera of the engine view a native consumer node renders (eye 1 MAIN, 2 the second eye).
#[no_mangle]
pub extern "C" fn cvr_framegen_record_view_camera(view: *const c_void, eye: u32) {
    if !capture_inputs_enabled() || !(1..=2).contains(&eye) {
        return;
    }
    let pose = node_pose();
    if !pose.valid || !pose.has_render_frame_id || pose.view != eye {
        return;
    }
    let camera = view_camera(view as usize);
    let value = Constants {
        camera: camera.unwrap_or_default(),
        frame: pose.render_frame_id,
        valid: camera.is_some(),
        pose: pose.pose_id,
        origin: pose.origin,
    };
    if value.valid {
        let mut s = lock();
        s.view_cameras[eye as usize - 1][(value.frame % 8) as usize] = value;
        compare(&s, eye as usize - 1, value.frame);
    }
}

// ENGINE STEREO: ONE FRAME, TWO EYES (include/Stereo/EngineStereo.hpp). MAIN's depth and motion hold both eyes side by
// side and its camera is the head centre's, while the headset shows each half as its own eye and framegen generates
// each eye from its inputs. MAIN's inputs are recorded as two -- the left half in the first pool, the right half in the
// second -- each with its eye's camera: placed sep/2 along the right vector, its own lens window, the motion scale of a
// half-width input and the jitter in its own pixels, as engine stereo gives each eye's DLSS.
unsafe fn half(tag: &InputTag, eye: u32) -> InputTag {
    let mut half = InputTag { ..*tag };
    let Some(resource) = ID3D12Resource::from_raw_borrowed(&tag.resource) else {
        return half;
    };
    let desc = resource.GetDesc(); // a native tag is the whole resource
    half.width = (if tag.width != 0 {
        tag.width
    } else {
        desc.Width as u32
    }) / 2;
    half.height = if tag.height != 0 {
        tag.height
    } else {
        desc.Height
    };
    half.x = tag.x + if eye != 0 { half.width } else { 0 };
    half
}
// Not from the tags: the engine tags one resource per call, and the record (and its camera) is made by the first.
unsafe fn camera_of_eye(mut c: CameraData, eye: u32) -> CameraData {
    let offset = (if eye != 0 { 0.5f32 } else { -0.5 }) * engine_stereo::eye_separation();
    for i in 0..3 {
        c.position[i] += c.right[i] * offset;
    }
    let (mut width, mut height) = (0u32, 0u32);
    let sized = engine_stereo::cvr_engine_stereo_eye_size(&mut width, &mut height);
    let mut window = core::mem::zeroed::<engine_stereo::EyeProjection>();
    if engine_stereo::cvr_engine_stereo_eye_window(eye as i32, &mut window) {
        c.aspect = window.aspect;
        c.shift = [window.shift_x, window.shift_y];
    } else if sized {
        c.aspect = width as f32 / height as f32; // the shared window, square pixels
    }
    c.motion_scale[0] *= 2.0;
    // The eye camera turns the jitter into NDC with its build's size (2 j / size), the eye's half has `width` pixels.
    let (mut jitter_width, mut jitter_height) = (0u32, 0u32);
    if sized
        && engine_stereo::cvr_engine_stereo_eye_jitter_size(&mut jitter_width, &mut jitter_height)
    {
        c.jitter[0] *= width as f32 / jitter_width as f32;
        c.jitter[1] *= height as f32 / jitter_height as f32;
    }
    c
}

/// One record of pool `side` for the pose's frame, found or allocated, its tags copied outside the lock. Takes the
/// lock and gives it back.
#[allow(clippy::too_many_arguments)]
unsafe fn record_side(
    mut guard: MutexGuard<'static, State>,
    side: usize,
    pose: &NodePose,
    camera: &CameraData,
    tags: &[InputTag],
    list: &ID3D12GraphicsCommandList,
    native: bool,
) -> (MutexGuard<'static, State>, bool) {
    let s = &mut *guard;
    let mut record: Option<Arc<Record>> = None;
    for item in s.pools[side].iter().flatten() {
        let r = item.get();
        if !r.invalid
            && !r.data.submitted
            && r.data.frame_id == pose.render_frame_id
            && r.data.pose_id == pose.pose_id
            && r.data.origin == pose.origin
        {
            record = Some(item.clone());
            break;
        }
    }
    if record.is_none() {
        for slot in s.pools[side].iter_mut() {
            if let Some(item) = slot.as_ref() {
                let r = item.get();
                if Arc::strong_count(item) != 1
                    || r.expected != r.submitted
                    || r.data
                        .ready_fence
                        .as_ref()
                        .is_some_and(|f| f.GetCompletedValue() < r.data.fence_value)
                {
                    continue;
                }
            }
            let item = slot.get_or_insert_with(|| Arc::new(Record::default()));
            let r = item.get();
            let data = &mut r.data;
            data.pose_id = pose.pose_id;
            data.origin = pose.origin;
            data.frame_id = pose.render_frame_id;
            data.view = side as u32 + 1;
            data.camera = *camera;
            data.queue = core::ptr::null_mut();
            data.has_depth = false;
            data.has_motion = false;
            data.submitted = false;
            data.evaluated = false;
            data.native = native;
            s.next_generation += 1;
            r.generation = s.next_generation;
            r.expected = 0;
            r.submitted = 0;
            r.invalid = false;
            record = Some(item.clone());
            break;
        }
    }
    let Some(record) = record else {
        framegen_core::cvr_framegen_on_skip();
        return (guard, false);
    };
    // Resource/barrier hooks may enter other renderer locks. Do GPU recording outside the input registry lock, then
    // publish only the fields we wrote. A prior component's queue/fence publication must not be overwritten.
    let mut copy = record.get().data.clone();
    drop(guard);
    let mut copied = 0u8;
    for tag in tags {
        if tag.resource.is_null() {
            continue;
        }
        if tag.kind == 1 && !motion_wanted() {
            continue;
        }
        stage(5);
        if (tag.kind == 0 && copy.has_depth) || (tag.kind == 1 && copy.has_motion) {
            continue;
        }
        let slots = InputSlots {
            depth: &mut copy.depth,
            motion: &mut copy.motion,
            ready_fence: &mut copy.ready_fence,
            width: &mut copy.width,
            height: &mut copy.height,
            motion_width: &mut copy.motion_width,
            motion_height: &mut copy.motion_height,
            has_depth: &mut copy.has_depth,
            has_motion: &mut copy.has_motion,
            bytes: &mut copy.bytes,
        };
        if cvr_gpu_framegen_copy_input(list.as_raw(), tag, &slots) {
            stage(6);
            copied |= if tag.kind == 0 { 1 } else { 2 };
        }
    }
    let mut guard = lock();
    let s = &mut *guard;
    let r = record.get();
    if copied != 0 {
        let data = &mut r.data;
        data.depth = copy.depth.take();
        data.motion = copy.motion.take();
        data.width = copy.width;
        data.height = copy.height;
        data.motion_width = copy.motion_width;
        data.motion_height = copy.motion_height;
        data.has_depth = copy.has_depth;
        data.has_motion = copy.has_motion;
        data.bytes = copy.bytes;
        if data.ready_fence.is_none() {
            data.ready_fence = copy.ready_fence.take();
        }
        r.expected |= copied;
        let links = s.pending.entry(list.as_raw() as usize).or_default();
        links.push(Link {
            record: Arc::downgrade(&record),
            generation: r.generation,
            mask: copied,
        });
        if native
            && r.data.has_depth
            && (!motion_wanted() || r.data.has_motion)
            && !r.data.evaluated
        {
            r.data.evaluated = true;
            r.expected |= 4;
            links.push(Link {
                record: Arc::downgrade(&record),
                generation: r.generation,
                mask: 4,
            });
        }
        HAVE_PENDING.store(true, Release);
    }
    let complete = r.data.has_depth && (!motion_wanted() || r.data.has_motion);
    (guard, complete)
}

unsafe fn record_inputs(tags: &[InputTag], command_list: *mut c_void, native: bool) -> bool {
    if !capture_inputs_enabled() || command_list.is_null() || tags.len() > 32 {
        return false;
    }
    stage(1);
    let pose = node_pose();
    if !pose.valid || !(1..=2).contains(&pose.view) {
        return false;
    }
    stage(2);
    if !pose.has_render_frame_id {
        return false;
    }
    stage(3);
    let Some(list) = ID3D12GraphicsCommandList::from_raw_borrowed(&command_list) else {
        return false;
    };
    if list.GetType() != D3D12_COMMAND_LIST_TYPE_DIRECT {
        return false;
    }
    let guard = lock();
    let constants = (if native {
        &guard.view_cameras
    } else {
        &guard.cameras
    })[pose.view as usize - 1][(pose.render_frame_id % 8) as usize];
    if !constants.valid
        || constants.frame != pose.render_frame_id
        || constants.pose != pose.pose_id
        || constants.origin != pose.origin
    {
        return false;
    }
    stage(4);
    if !split_eyes(&pose) {
        return record_side(
            guard,
            pose.view as usize - 1,
            &pose,
            &constants.camera,
            tags,
            list,
            native,
        )
        .1;
    }
    let mut guard = guard;
    let mut complete = true;
    for eye in 0..2u32 {
        let halves: Vec<InputTag> = tags.iter().map(|t| half(t, eye)).collect();
        let (g, ok) = record_side(
            guard,
            eye as usize,
            &pose,
            &camera_of_eye(constants.camera, eye),
            &halves,
            list,
            native,
        );
        guard = g;
        complete = ok && complete;
    }
    complete
}

/// RecordTags: called by the Streamline tag hook while the supplied list owns the resources.
///
/// # Safety
/// Streamline's arguments as slSetTag received them.
#[no_mangle]
pub unsafe extern "C" fn cvr_framegen_record_tags(
    viewport: *const c_void,
    tags: *const c_void,
    count: u32,
    command_list: *mut c_void,
) {
    stage(0);
    // Scope lenses write their velocity into this target (independent of framegen).
    if !tags.is_null() && count <= 32 {
        for i in 0..count as usize {
            if let Some(tag) = read_tag(tags as usize + i * core::mem::size_of::<sl::ResourceTag>())
            {
                if tag.kind == 1 {
                    cvr_gpu::optics::cvr_optics_motion_vectors_tagged(
                        tag.resource,
                        cvr_cpp_framegen_verbose(),
                    );
                }
            }
        }
    }
    if crate::scope_dlss::is_scope_viewport(viewport) {
        return;
    }
    if !capture_inputs_enabled() || tags.is_null() || count > 32 {
        return;
    }
    let decoded: Vec<InputTag> = (0..count as usize)
        .map(|i| {
            read_tag(tags as usize + i * core::mem::size_of::<sl::ResourceTag>())
                .unwrap_or_else(empty_tag)
        })
        .collect();
    record_inputs(&decoded, command_list, false);
}
/// RecordNativeInputs: a native consumer's depth and motion (two tags).
///
/// # Safety
/// `tags` points to two tags; `list` is an open command list or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_framegen_record_native_inputs(
    tags: *const InputTag,
    list: *mut c_void,
) -> bool {
    if tags.is_null() {
        return false;
    }
    record_inputs(core::slice::from_raw_parts(tags, 2), list, true)
}

/// InstallInputHooks: slSetConstants and slEvaluateFeature, once sl.interposer.dll is loaded, then engine stereo's
/// options hook.
#[no_mangle]
pub extern "C" fn cvr_framegen_install_input_hooks() -> bool {
    if HOOKS_INSTALLED.load(Acquire) {
        return true;
    }
    // SAFETY: plain Win32 and MinHook calls on Streamline's exports.
    unsafe {
        let Ok(module) = GetModuleHandleW(w!("sl.interposer.dll")) else {
            return false;
        };
        let (Some(constants), Some(evaluate_fn)) = (
            GetProcAddress(module, s!("slSetConstants")),
            GetProcAddress(module, s!("slEvaluateFeature")),
        ) else {
            return false;
        };
        let constants = constants as *mut c_void;
        let evaluate_fn = evaluate_fn as *mut c_void;
        let mut original: *mut c_void = core::ptr::null_mut();
        if MH_CreateHook(constants, set_constants as *mut c_void, &mut original) != 0 {
            return false;
        }
        ORIGINAL_CONSTANTS.store(original as usize, Release);
        let mut original: *mut c_void = core::ptr::null_mut();
        if MH_CreateHook(evaluate_fn, evaluate as *mut c_void, &mut original) != 0 {
            MH_RemoveHook(constants);
            return false;
        }
        ORIGINAL_EVALUATE.store(original as usize, Release);
        if MH_EnableHook(constants) != 0 || MH_EnableHook(evaluate_fn) != 0 {
            MH_DisableHook(constants);
            MH_DisableHook(evaluate_fn);
            MH_RemoveHook(constants);
            MH_RemoveHook(evaluate_fn);
            return false;
        }
    }
    HOOKS_INSTALLED.store(true, Release);
    log("[framegen] native Streamline input hooks installed\n");
    engine_stereo_dlss::cvr_engine_stereo_dlss_install_options_hook();
    true
}

/// Submitted: the lists a queue ran; the records they wrote are signalled on the record's fence and complete once
/// every part (depth 1, motion 2, evaluate 4) is in.
///
/// # Safety
/// `lists` holds `count` command lists.
#[no_mangle]
pub unsafe extern "C" fn cvr_framegen_inputs_submitted(
    queue: *mut c_void,
    count: u32,
    lists: *const *mut c_void,
) {
    if !HAVE_PENDING.load(Acquire) {
        return;
    }
    let mut guard = lock();
    let s = &mut *guard;
    let Some(queue_ref) = ID3D12CommandQueue::from_raw_borrowed(&queue) else {
        return;
    };
    for i in 0..count as usize {
        let Some(links) = s.pending.remove(&(*lists.add(i) as usize)) else {
            continue;
        };
        for link in &links {
            let Some(record) = link.record.upgrade() else {
                continue;
            };
            let r = record.get();
            if r.generation != link.generation {
                continue;
            }
            let data = &mut r.data;
            if !data.queue.is_null() && data.queue != queue {
                r.invalid = true;
            }
            data.queue = queue;
            r.submitted |= link.mask;
            if let Some(fence) = data.ready_fence.as_ref() {
                data.fence_value += 1;
                if queue_ref.Signal(fence, data.fence_value).is_err() {
                    r.invalid = true;
                }
            }
            if !r.invalid
                && (r.submitted & 5) == 5
                && r.submitted == r.expected
                && data.evaluated
                && !data.submitted
            {
                data.submitted = true;
                framegen_core::cvr_framegen_on_input(data.view - 1, data.width, data.height);
                if diagnostics() && !LOGGED[data.view as usize - 1].swap(true, SeqCst) {
                    let line = format(
                        c"[framegen] view=%u frame=%u pose=%llu depth=%ux%u mv=%ux%u scale=(%.6f,%.6f) near=%.6f far=%.3f fov=%.5f inverted=%d\n",
                        &[
                            Arg::U(data.view as u64),
                            Arg::U(data.frame_id as u64),
                            Arg::U(data.pose_id),
                            Arg::U(data.width as u64),
                            Arg::U(data.height as u64),
                            Arg::U(data.motion_width as u64),
                            Arg::U(data.motion_height as u64),
                            Arg::F(data.camera.motion_scale[0] as f64),
                            Arg::F(data.camera.motion_scale[1] as f64),
                            Arg::F(data.camera.near_plane as f64),
                            Arg::F(data.camera.far_plane as f64),
                            Arg::F(data.camera.vertical_fov as f64),
                            Arg::I(data.camera.inverted as i32),
                        ],
                    );
                    log(&line.to_string_lossy());
                }
                stage(8);
            }
        }
    }
    HAVE_PENDING.store(!s.pending.is_empty(), Release);
}
/// ResetList: a list reset before it was submitted; the records it wrote are given up.
#[no_mangle]
pub extern "C" fn cvr_framegen_reset_list(list: *mut c_void) {
    if !HAVE_PENDING.load(Acquire) {
        return;
    }
    let mut guard = lock();
    let s = &mut *guard;
    let Some(links) = s.pending.remove(&(list as usize)) else {
        return;
    };
    for link in &links {
        if let Some(record) = link.record.upgrade() {
            let r = record.get();
            if r.generation == link.generation {
                r.invalid = true;
                r.submitted |= link.mask;
            }
        }
    }
    HAVE_PENDING.store(!s.pending.is_empty(), Release);
}
/// AcquireInputs: the completed inputs of `view`'s frame and pose on `queue`, held until cvr_framegen_release_inputs.
#[no_mangle]
pub extern "C" fn cvr_framegen_acquire_inputs(
    view: u32,
    frame: u32,
    pose: u64,
    origin: u64,
    queue: *mut c_void,
) -> *const Inputs {
    if !(1..=2).contains(&view) {
        return core::ptr::null();
    }
    let s = lock();
    for record in s.pools[view as usize - 1].iter().flatten() {
        let r = record.get();
        if !r.invalid
            && r.data.submitted
            && r.data.frame_id == frame
            && r.data.pose_id == pose
            && r.data.origin == origin
            && r.data.queue == queue
        {
            stage(9);
            return Arc::into_raw(record.clone()) as *const Inputs;
        }
    }
    core::ptr::null()
}
/// The release of what cvr_framegen_acquire_inputs gave.
///
/// # Safety
/// `inputs` came from cvr_framegen_acquire_inputs and is released once.
#[no_mangle]
pub unsafe extern "C" fn cvr_framegen_release_inputs(inputs: *const Inputs) {
    if !inputs.is_null() {
        drop(Arc::from_raw(inputs as *const Record));
    }
}
/// One more reference to what cvr_framegen_acquire_inputs gave (released with cvr_framegen_release_inputs); the same
/// pointer, or null.
///
/// # Safety
/// `inputs` came from cvr_framegen_acquire_inputs (or this) and is still held, or is null.
#[no_mangle]
pub unsafe extern "C" fn cvr_framegen_retain_inputs(inputs: *const Inputs) -> *const Inputs {
    if !inputs.is_null() {
        Arc::increment_strong_count(inputs as *const Record);
    }
    inputs
}
/// InputVram: the bytes of every record's copies.
#[no_mangle]
pub extern "C" fn cvr_framegen_input_vram() -> u64 {
    let s = lock();
    s.pools
        .iter()
        .flatten()
        .flatten()
        .map(|r| r.get().data.bytes)
        .sum()
}
/// ReleaseInputs: the records nobody holds and the GPU is done with.
#[no_mangle]
pub extern "C" fn cvr_framegen_release_all_inputs() {
    let mut guard = lock();
    for slot in guard.pools.iter_mut().flatten() {
        let done = slot.as_ref().is_some_and(|item| {
            let r = item.get();
            Arc::strong_count(item) == 1
                && r.expected == r.submitted
                // SAFETY: a live fence.
                && r.data.ready_fence.as_ref().is_none_or(|f| unsafe { f.GetCompletedValue() } >= r.data.fence_value)
        });
        if done {
            *slot = None;
        }
    }
}

// ---- the native consumers (formerly src/Framegen/NativeInputs.cpp)
//
// The engine nodes whose depth and motion framegen reuses when Streamline is not the upscaler. TAA keeps the two SRVs
// it was proven on. Every temporal upscaler node is handed the same two view-salted resources instead -- depth
// (view<<24 ^ 0x61F178D4) and velocity (view<<24 ^ 0x15EAB19C), the ones sub_141D4E150 passes to FSR 3 as "depth" and
// "motionVectors" -- so one observer covers FSR 2 (the engine's own passes), FSR 3.0, FSR 3.1/4 and XeSS. The last
// three are libraries that move the inputs into their own states, so they are captured when the library is called
// (cvr_framegen_capture_upscaler_inputs), not at the next draw or dispatch.

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
enum Consumer {
    None,
    Taa,
    Fsr2,
    #[allow(dead_code)]
    Fsr3,
    FfxApi,
    Xess,
}
fn current_consumer() -> Consumer {
    let base = code::image_base();
    if !capture_inputs_enabled() || cvr_host::probe::internal() || base == 0 {
        return Consumer::None;
    }
    // SAFETY: the C++ adapter.
    match crate::render_tls::current_node_work().wrapping_sub(base) {
        0x557204 => Consumer::Taa,  // CRenderNode_ApplyTXAA
        0x41CE60 => Consumer::Fsr2, // CRenderNode_ApplyFSR2
        // CRenderNode_ApplyFSR3_0 (2.31: 0x1D4E8B0, ffx_fsr3_x64.dll) is gone in 2.5 with the ffx_* DLLs; FSR 3.x now
        // runs through ApplyFSR3 and amd_fidelityfx_loader_dx12.dll.
        0x22873B0 => Consumer::FfxApi, // CRenderNode_ApplyFSR3, amd_fidelityfx_dx12.dll
        0xD6C0C0 => Consumer::Xess,    // CRenderNode_ApplyXESS
        _ => Consumer::None,
    }
}
fn library(c: Consumer) -> bool {
    matches!(c, Consumer::Fsr3 | Consumer::FfxApi | Consumer::Xess)
}
fn input_type(c: Consumer, key: u32) -> i32 {
    // ApplyTXAA's contiguous SRVs: current color, history, velocity (917D8BC9), resolved depth (DEBF0C27), mask. The
    // raw G-buffer velocity is not sampled.
    let (depth, motion) = if c == Consumer::Taa {
        (0xBF0C27, 0x7D8BC9)
    } else {
        (0xF178D4, 0xEAB19C)
    };
    if key == depth {
        0
    } else if key == motion {
        1
    } else {
        -1
    }
}
#[derive(Clone, Copy, Default)]
struct Candidate {
    frame: u32,
    view: u32,
    handles: [u32; 2],
    pose: u64,
    node: usize,
    copied: bool,
}
thread_local! {
    static CANDIDATE: Cell<Candidate> = Cell::new(Candidate::default());
}
fn native_debug(kind: u32, i: usize, value: u64) {
    // SAFETY: an export of the C++ half.
    unsafe { CyberpunkVR_FramegenNativeDebug[kind as usize][i].store(value, SeqCst) };
}
fn native_stage(i: usize) {
    // SAFETY: an export of the C++ half.
    unsafe { CyberpunkVR_FramegenNativeStages[i].fetch_add(1, SeqCst) };
}
/// The resource behind a physical handle, as a framegen input in `state`.
fn describe(pool: usize, handle: u32, kind: u32, state: u32) -> Option<InputTag> {
    native_debug(kind, 4, state as u64);
    let resource = code::read::<usize>(pool + 0x2F1D8 + (handle as usize).wrapping_sub(1) * 0xB0)?;
    native_debug(kind, 5, resource as u64);
    let raw = resource as *mut c_void;
    // SAFETY: the frame graph's resource, as the C++ called it.
    let d = unsafe { ID3D12Resource::from_raw_borrowed(&raw)?.GetDesc() };
    native_debug(kind, 6, d.Format.0 as u32 as u64);
    native_debug(kind, 7, (d.Width << 32) | d.Height as u64);
    if d.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
        || d.Width == 0
        || d.Height == 0
        || d.Width > 16384
        || d.Height > 16384
        || d.MipLevels != 1
        || d.DepthOrArraySize != 1
    {
        return None;
    }
    Some(InputTag {
        resource: raw,
        state,
        kind,
        x: 0,
        y: 0,
        width: d.Width as u32,
        height: d.Height,
    })
}
/// The resource and state behind a handle: the 2.31 resource table and the command context's state ledger, the same
/// records consumed by native SRV binding +1FABD0 and barrier tracking +1F41F8. No remembered D3D pointer is used;
/// the only assumed state is a library's input state (`unused`) for a resource this context has not touched yet. Zero
/// refuses such a resource.
fn read_handle(handle: u32, kind: u32, unused: u32) -> Option<InputTag> {
    native_debug(kind, 0, handle as u64);
    let base = code::image_base();
    // SAFETY: the engine's current command context, called through the SEH adapter (no arguments).
    let context = unsafe {
        code::guarded_call4(
            base + 0x337230,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
        )
    }? as usize;
    native_debug(kind, 1, context as u64);
    let pool = code::read::<usize>(base + 0x3BCDCA8)?;
    if context == 0 || pool == 0 || handle == 0 || handle > 0x100000 {
        return None;
    }
    let table = context + 0x538;
    let count = code::read::<u32>(table + 8)?;
    let buckets = code::read::<u32>(table + 12)?;
    let stride = code::read::<u32>(table + 28)?;
    native_debug(kind, 2, count as u64);
    native_debug(kind, 3, ((buckets as u64) << 32) | stride as u64);
    if count == 0 {
        return if unused != 0 {
            describe(pool, handle, kind, unused)
        } else {
            None
        };
    }
    if buckets == 0 || buckets > 0x100000 || !(20..=128).contains(&stride) {
        return None;
    }
    let heads = code::read::<usize>(table)?;
    let entries = code::read::<usize>(table + 16)?;
    if heads == 0 || entries == 0 {
        return None;
    }
    let mut index = code::read::<i32>(heads + 4 * (handle % buckets) as usize)?;
    let mut visited = 0;
    while (0..0x100000).contains(&index) && visited < 1024 {
        let entry = entries + index as usize * stride as usize;
        if code::read::<u32>(entry + 4)? == handle && code::read::<u32>(entry + 8)? == handle {
            let mut state = code::read::<u32>(entry + 16)?;
            if state == 0x8000_0000 {
                let first = code::read::<u16>(entry + 12)?;
                let states = code::read::<usize>(table + 96)?;
                if first == 0xFFFF || states == 0 || code::read::<u16>(entry + 14)? == 0 {
                    return None;
                }
                state = code::read::<u32>(states + 4 * first as usize)?;
            }
            const READS: u32 = (D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE.0
                | D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE.0
                | D3D12_RESOURCE_STATE_DEPTH_READ.0) as u32;
            native_debug(kind, 4, state as u64);
            if state & (READS & !(D3D12_RESOURCE_STATE_DEPTH_READ.0 as u32)) == 0
                || state & !READS != 0
            {
                return None;
            }
            return describe(pool, handle, kind, state);
        }
        index = code::read::<i32>(entry)?;
        visited += 1;
    }
    if unused != 0 {
        return describe(pool, handle, kind, unused);
    }
    None
}

/// ObserveNativeInput: a consumer node's logical input bound to its physical handle.
#[no_mangle]
pub extern "C" fn cvr_framegen_observe_native_input(logical: u32, physical: u32) {
    let consumer = current_consumer();
    if consumer == Consumer::None {
        return;
    }
    let kind = input_type(consumer, logical & 0xFF_FFFF);
    if kind < 0 {
        return;
    }
    let pose = node_pose();
    if !pose.valid || !pose.has_render_frame_id || !(1..=2).contains(&pose.view) {
        return;
    }
    // SAFETY: the C++ adapter.
    let node = crate::render_tls::current_node_work();
    CANDIDATE.with(|cell| {
        let mut c = cell.get();
        if c.frame != pose.render_frame_id
            || c.view != pose.view
            || c.pose != pose.pose_id
            || c.node != node
        {
            c = Candidate {
                frame: pose.render_frame_id,
                view: pose.view,
                handles: [0, 0],
                pose: pose.pose_id,
                node,
                copied: false,
            };
        }
        c.handles[kind as usize] = physical;
        cell.set(c);
    });
    native_stage(0);
}
/// ObserveNativeView: the node dispatcher's work context before a consumer node runs; records that view's camera.
#[no_mangle]
pub extern "C" fn cvr_framegen_observe_native_view(work_context: *const c_void) {
    if current_consumer() == Consumer::None || work_context.is_null() {
        return;
    }
    let pose = node_pose();
    if !pose.valid || !pose.has_render_frame_id || !(1..=2).contains(&pose.view) {
        return;
    }
    // The view a node renders, at work context +0x18 (as every node reads it).
    if let Some(view) = code::read::<usize>(work_context as usize + 0x18).filter(|v| *v != 0) {
        cvr_framegen_record_view_camera(view as *const c_void, pose.view);
    }
}
unsafe fn capture(list: *mut c_void, from_library: bool) {
    let consumer = current_consumer();
    if consumer == Consumer::None || library(consumer) != from_library {
        return;
    }
    let candidate = CANDIDATE.with(|c| c.get());
    if candidate.copied
        || candidate.handles[0] == 0
        || (motion_wanted() && candidate.handles[1] == 0)
        || candidate.node != crate::render_tls::current_node_work()
    {
        return;
    }
    let pose = node_pose();
    if !pose.valid
        || !pose.has_render_frame_id
        || candidate.frame != pose.render_frame_id
        || candidate.view != pose.view
        || candidate.pose != pose.pose_id
    {
        return;
    }
    native_stage(1);
    // A library is promised its inputs readable by pixel and compute shaders: the engine hands FSR 3 FFX state 12,
    // PIXEL_COMPUTE_READ (sub_141D4F220). An input this command context has not touched yet is in that state from an
    // earlier context.
    let unused = if from_library {
        (D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE.0
            | D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE.0) as u32
    } else {
        0
    };
    let mut tags = [empty_tag(), empty_tag()];
    match read_handle(candidate.handles[0], 0, unused) {
        Some(t) => tags[0] = t,
        None => {
            native_stage(2);
            return;
        }
    }
    if motion_wanted() {
        match read_handle(candidate.handles[1], 1, unused) {
            Some(t) => tags[1] = t,
            None => {
                native_stage(2);
                return;
            }
        }
    }
    let recorded = {
        let _internal = cvr_host::probe::Internal::new();
        record_inputs(&tags, list, true)
    };
    if !recorded {
        native_stage(2);
        return;
    }
    CANDIDATE.with(|c| {
        let mut v = c.get();
        v.copied = true;
        c.set(v);
    });
    native_stage(3);
    // SAFETY: an export of the C++ half.
    CyberpunkVR_FramegenNativeSource.store(consumer as u32, Relaxed);
    static LOGS: AtomicU32 = AtomicU32::new(0);
    if LOGS.fetch_add(1, SeqCst) < 4 {
        log(&format!(
            "[framegen-native] source={} view={} frame={} depth={}x{} state=0x{:X} mv={}x{} state=0x{:X}\n",
            consumer as u32,
            pose.view,
            pose.render_frame_id,
            tags[0].width,
            tags[0].height,
            tags[0].state,
            tags[1].width,
            tags[1].height,
            tags[1].state
        ));
    }
}
/// CaptureNativeInputs: TAA and FSR 2 record engine passes, captured at the node's next draw or dispatch.
///
/// # Safety
/// `list` is the engine's open command list.
#[no_mangle]
pub unsafe extern "C" fn cvr_framegen_capture_native_inputs(list: *mut c_void) {
    capture(list, false);
}
/// CaptureUpscalerInputs: FSR 3 and XeSS are libraries, captured when the library is called, before it touches the
/// inputs.
///
/// # Safety
/// As cvr_framegen_capture_native_inputs.
#[no_mangle]
pub unsafe extern "C" fn cvr_framegen_capture_upscaler_inputs(list: *mut c_void) {
    capture(list, true);
}
