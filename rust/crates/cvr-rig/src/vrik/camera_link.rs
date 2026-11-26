//! What the camera patch sites hand each other (formerly src/Camera/CameraLink.cpp; the ownership rule and the
//! single-publisher invariant are include/Camera/CameraLink.hpp's): the composed quaternion with the XR sample it
//! came from, the ring of writes the final camera identifies its frame by, the placed per-eye transforms, the main
//! eye's centre, the anchor recipe, and three one-instant packets (the located frame, the view frame, the barrel
//! frame) behind sequence locks so a reader never mixes two publications.
//!
//! The counters stay exports of src/Camera/CameraLinkState.cpp; what this side needs from the rest of the C++ half
//! (the tracking origin, the pose identities, the scene cameras) comes through src/Camera/CameraServices.cpp.
use super::env::log;
use cvr_body::cmath::atan2;
use cvr_camera::anchor::AnchorRecipe;
use cvr_camera::eye_centre::EyeCentreLedger;
use cvr_camera::history::{LabelPose, RenderedPoseHistory, RenderedPoseMatch};
use std::sync::atomic::{fence, AtomicU32, AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};

/// OpenXRHeadPose (include/Runtimes/OpenXRManager.hpp), field for field; CameraRust.cpp checks the layout.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeadPose {
    pub pos: [f32; 3],
    pub ori: [f32; 4],
    pub valid: bool,
    /// Recenter/reference-space generation of this exact sample.
    pub origin_serial: u64,
    /// cvr::body::BendSample: right, forward, angle.
    pub body_bend: [f32; 3],
    pub crouch_depth: f32,
    pub physical_crouch: bool,
    /// The AFW eye MAIN was placed at, 0 or 1; -1 not placed for AFW.
    pub afw_eye: i8,
}
impl Default for HeadPose {
    fn default() -> Self {
        HeadPose {
            pos: [0.0; 3],
            ori: [0.0; 4],
            valid: false,
            origin_serial: 0,
            body_bend: [0.0; 3],
            crouch_depth: 0.0,
            physical_crouch: false,
            afw_eye: -1,
        }
    }
}
impl LabelPose for HeadPose {
    fn valid(&self) -> bool {
        self.valid
    }
    fn position(&self) -> [f32; 3] {
        self.pos
    }
    fn orientation(&self) -> [f32; 4] {
        self.ori
    }
    fn origin_serial(&self) -> u64 {
        self.origin_serial
    }
}

/// cvr::roomscale::Vec2.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

/// cvr::camera::LocatedCameraFrame.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LocatedCameraFrame {
    pub world_pos: [f32; 3],
    pub world_quat: [f32; 4],
    pub sequence: u32,
    pub frame_epoch: u32,
    pub body_centre_known: u32,
    /// From the same component write, before VR translation/IPD.
    pub body_base_world: [f32; 3],
}
/// cvr::camera::ViewFrame.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ViewFrame {
    pub view_quat: [f32; 4],
    pub world_delta: [f32; 3],
    pub delta_semantics: f32,
    pub heading_rad: f32,
    pub head_ori: [f32; 4],
    pub stamp_ms: f32,
}
/// cvr::camera::BarrelFrame.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BarrelFrame {
    pub cam_quat: [f32; 4],
    pub muzzle_fwd: [f32; 3],
}

extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static CyberpunkVR_BodyCentreMatch: AtomicU64;
    static CyberpunkVR_BodyCentreMiss: AtomicU64;
    static CyberpunkVR_BodyLuaCentreMatch: AtomicU64;
    static CyberpunkVR_BodyLuaCentreRejected: AtomicU64;
    static CyberpunkVR_PlacedPoseMatch: AtomicU64;
    static CyberpunkVR_PlacedPoseMiss: AtomicU64;
    static CyberpunkVR_PlacedPoseAmbiguous: AtomicU64;
    static mut CyberpunkVR_DebugFinalExact: u64;
    static mut CyberpunkVR_DebugFinalApprox: u64;
    static mut CyberpunkVR_DebugFinalExactTies: u64;
    static CyberpunkVR_OneSamplePerFrame: i32;
    static CyberpunkVR_CamWriteInPatch: i32;
    static CyberpunkVR_CamComposeAtWrite: i32;
    static CyberpunkVR_CamWriteInFinal: i32;
    static CyberpunkVR_HeadTranslationInPatch: i32;
    static CyberpunkVR_VrcamHeadTranslation: i32;
    static CyberpunkVR_DeltaFromFreshSample: i32;
    static CyberpunkVR_VrcamPosFromMain: i32;
    static CyberpunkVR_EngineBodyYawValid: i32;
    static CyberpunkVR_EngineBodyYawZ: f32;
    static CyberpunkVR_EngineBodyYawW: f32;
    static g_isInVehicle: bool;
    // src/Camera/CameraServices.cpp
    fn cvr_cpp_xr_tracking_origin_serial() -> u64;
    fn cvr_cpp_next_pose_identity() -> u64;
    fn cvr_cpp_device_cam_active() -> bool;
    fn cvr_cpp_braindance_active() -> bool;
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

// ---- the anchor recipe

static ANCHOR: Mutex<Option<AnchorRecipe>> = Mutex::new(None);

#[no_mangle]
pub unsafe extern "C" fn cvr_camera_anchor_recipe_publish(recipe: *const AnchorRecipe) {
    *ANCHOR.lock().unwrap_or_else(|e| e.into_inner()) = Some(*recipe);
}
/// Forget the published recipe: reads fail until the next publication.
#[no_mangle]
pub extern "C" fn cvr_camera_anchor_recipe_clear() {
    *ANCHOR.lock().unwrap_or_else(|e| e.into_inner()) = None;
}
#[no_mangle]
pub unsafe extern "C" fn cvr_camera_anchor_recipe_read(recipe: *mut AnchorRecipe) -> bool {
    if recipe.is_null() {
        return false;
    }
    match *ANCHOR.lock().unwrap_or_else(|e| e.into_inner()) {
        Some(r) => {
            *recipe = r;
            true
        }
        None => false,
    }
}

// ---- the main eye's centre

static EYE_CENTRES: LazyLock<EyeCentreLedger> = LazyLock::new(EyeCentreLedger::default);

#[no_mangle]
pub unsafe extern "C" fn cvr_camera_main_eye_centre_publish(eye: *const [i32; 3], centre: *const [i32; 3], body_base: *const [i32; 3]) {
    EYE_CENTRES.publish(*eye, *centre, *body_base);
}
#[no_mangle]
pub unsafe extern "C" fn cvr_camera_main_eye_centre_read(eye: *const [i32; 3], centre: *mut [i32; 3], body_base: *mut [i32; 3]) -> bool {
    let Some((value, base)) = EYE_CENTRES.read(*eye) else {
        count(&CyberpunkVR_BodyCentreMiss);
        return false;
    };
    count(&CyberpunkVR_BodyCentreMatch);
    *centre = value;
    if !body_base.is_null() {
        *body_base = base;
    }
    true
}
#[no_mangle]
pub unsafe extern "C" fn cvr_camera_main_eye_centre_read_world(eye_or_centre: *const [f32; 3], centre: *mut [f32; 3], body_base: *mut [f32; 3]) -> bool {
    let Some((value, base)) = EYE_CENTRES.read_world(*eye_or_centre, !body_base.is_null()) else {
        count(&CyberpunkVR_BodyLuaCentreRejected);
        return false;
    };
    *centre = value;
    if !body_base.is_null() {
        *body_base = base;
    }
    count(&CyberpunkVR_BodyLuaCentreMatch);
    true
}

// ---- the composed quaternion and its source head pose

struct CamWrite {
    known: bool,
    quat: [f32; 4],
    pose: HeadPose,
    consumed: Vec2,
    consumed_known: bool,
    pose_id: u64,
}
static CAM_WRITE: LazyLock<Mutex<CamWrite>> = LazyLock::new(|| {
    Mutex::new(CamWrite {
        known: false,
        quat: [0.0, 0.0, 0.0, 1.0],
        pose: HeadPose::default(),
        consumed: Vec2::default(),
        consumed_known: false,
        pose_id: 0,
    })
});

#[no_mangle]
pub unsafe extern "C" fn cvr_camera_cam_write_quat_publish(x: f32, y: f32, z: f32, w: f32, pose: *const HeadPose, consumed: *const Vec2, source_id: u64) {
    let mut s = CAM_WRITE.lock().unwrap_or_else(|e| e.into_inner());
    s.quat = [x, y, z, w];
    s.pose = *pose;
    s.pose_id = if (*pose).valid {
        if source_id != 0 {
            source_id
        } else {
            cvr_cpp_next_pose_identity()
        }
    } else {
        0
    };
    s.consumed_known = !consumed.is_null();
    if !consumed.is_null() {
        s.consumed = *consumed;
    }
    s.known = true;
}
/// False when nothing was published; `out` is then untouched.
#[no_mangle]
pub unsafe extern "C" fn cvr_camera_cam_write_quat_read(out: *mut [f32; 4], pose: *mut HeadPose, consumed: *mut Vec2, consumed_known: *mut bool, pose_id: *mut u64) -> bool {
    let s = CAM_WRITE.lock().unwrap_or_else(|e| e.into_inner());
    if !s.known {
        return false;
    }
    *out = s.quat;
    if !pose.is_null() {
        *pose = s.pose;
    }
    if !consumed.is_null() {
        *consumed = s.consumed;
    }
    if !consumed_known.is_null() {
        *consumed_known = s.consumed_known;
    }
    if !pose_id.is_null() {
        *pose_id = s.pose_id;
    }
    true
}

// ---- the write ring: what we wrote, so the final camera can recognise it

const RING: u64 = 16;
#[derive(Clone, Copy, Default)]
struct Record {
    quat: [f32; 4],
    pose: HeadPose,
    /// Monotonic write index; ordering is what disambiguates a tie.
    id: u64,
    valid: bool,
}
struct Ring {
    records: [Record; RING as usize],
    head: u64,
}
static WRITE_RING: LazyLock<Mutex<Ring>> = LazyLock::new(|| Mutex::new(Ring { records: [Record::default(); RING as usize], head: 0 }));
static PLACED: LazyLock<RenderedPoseHistory<HeadPose>> = LazyLock::new(RenderedPoseHistory::default);
static APPROX_LAST_REPORT: AtomicU64 = AtomicU64::new(0);

#[no_mangle]
pub unsafe extern "C" fn cvr_camera_cam_write_record_push(q: *const [f32; 4], pose: *const HeadPose) {
    let mut ring = WRITE_RING.lock().unwrap_or_else(|e| e.into_inner());
    let id = ring.head;
    ring.records[(id % RING) as usize] = Record { quat: *q, pose: *pose, id, valid: true };
    ring.head = id + 1;
}

/// The frame's pose from the quaternion the engine renders with: the bit-identical write when there is one (the
/// normal path), else the nearest record inside a loose gate, counted and reported as the fallback it is.
#[no_mangle]
pub unsafe extern "C" fn cvr_camera_cam_write_record_find(q: *const [f32; 4], out: *mut HeadPose, out_age: *mut u32, out_ties: *mut u32) -> bool {
    let q = *q;
    let ring = WRITE_RING.lock().unwrap_or_else(|e| e.into_inner());
    let head = ring.head;
    let n = head.min(RING);
    let (mut have_best, mut best_d, mut best_id, mut best_pose, mut ties) = (false, 0.0f64, 0u64, HeadPose::default(), 0u32);
    let (mut exact_count, mut have_exact, mut exact_id, mut exact_pose) = (0u32, false, 0u64, HeadPose::default());
    for i in 1..=n {
        let r = &ring.records[(head.wrapping_sub(i) % RING) as usize];
        if !r.valid {
            continue;
        }
        if r.quat[0] == q[0] && r.quat[1] == q[1] && r.quat[2] == q[2] && r.quat[3] == q[3] {
            exact_count += 1;
            if !have_exact {
                have_exact = true;
                exact_id = r.id;
                exact_pose = r.pose;
            }
        }
        let dot = r.quat[0] as f64 * q[0] as f64 + r.quat[1] as f64 * q[1] as f64 + r.quat[2] as f64 * q[2] as f64 + r.quat[3] as f64 * q[3] as f64;
        // q and -q are the same rotation; align before differencing.
        let s = if dot < 0.0 { -1.0 } else { 1.0 };
        if dot * s <= 0.999999 {
            continue; // gate: obviously not this one
        }
        ties += 1;
        let mut d = 0.0f64;
        for k in 0..4 {
            let e = s * r.quat[k] as f64 - q[k] as f64;
            d += e * e;
        }
        if !have_best || d < best_d {
            best_d = d;
            best_id = r.id;
            best_pose = r.pose;
            have_best = true;
        }
    }
    // The exact hit decides whenever there is one: it is the identification, not an estimate.
    if have_exact {
        if diagnostics() {
            CyberpunkVR_DebugFinalExact += 1;
        }
        if exact_count > 1 && diagnostics() {
            CyberpunkVR_DebugFinalExactTies += 1;
        }
        if !out.is_null() {
            *out = exact_pose;
        }
        if !out_age.is_null() {
            *out_age = ((head - 1) - exact_id) as u32;
        }
        if !out_ties.is_null() {
            *out_ties = exact_count;
        }
        return true;
    }
    if !have_best {
        return false;
    }
    if diagnostics() {
        CyberpunkVR_DebugFinalApprox += 1;
    }
    let approx = CyberpunkVR_DebugFinalApprox;
    if approx.wrapping_sub(APPROX_LAST_REPORT.load(Ordering::Relaxed)) >= 600 {
        APPROX_LAST_REPORT.store(approx, Ordering::Relaxed);
        log(&format!(
            "POSEDIAG: WARNING -- the render camera quaternion is no longer a verbatim copy (approx={} exact={}). Frame identification has degraded to nearest-neighbour; expect the pose label to be within {:.3} deg rather than exact.\n",
            approx, CyberpunkVR_DebugFinalExact, 0.16
        ));
    }
    if !out.is_null() {
        *out = best_pose;
    }
    if !out_age.is_null() {
        *out_age = ((head - 1) - best_id) as u32;
    }
    if !out_ties.is_null() {
        *out_ties = ties;
    }
    true
}

/// The complete component path, on foot: position, orientation and eye identify the rendered transform.
#[no_mangle]
pub unsafe extern "C" fn cvr_camera_placed_pose_enabled() -> bool {
    CyberpunkVR_OneSamplePerFrame != 0
        && CyberpunkVR_CamWriteInPatch != 0
        && CyberpunkVR_CamComposeAtWrite != 0
        && CyberpunkVR_CamWriteInFinal == 0
        && CyberpunkVR_HeadTranslationInPatch != 0
        && CyberpunkVR_VrcamHeadTranslation != 0
        && CyberpunkVR_DeltaFromFreshSample != 0
        && CyberpunkVR_VrcamPosFromMain != 3
        && !g_isInVehicle
        && !cvr_cpp_device_cam_active()
        && !cvr_cpp_braindance_active()
}
#[no_mangle]
pub unsafe extern "C" fn cvr_camera_placed_pose_push(view: u32, q: *const [f32; 4], position: *const [i32; 3], pose: *const HeadPose) {
    PLACED.push(view, &*q, &*position, &*pose);
}
#[no_mangle]
pub unsafe extern "C" fn cvr_camera_placed_pose_find(view: u32, q: *const [f32; 4], position: *const [i32; 3], out: *mut HeadPose, age: *mut u32, ties: *mut u32) -> bool {
    let origin = cvr_cpp_xr_tracking_origin_serial();
    let (found, detail) = PLACED.find(view, &*q, &*position, origin);
    if let Some(d) = &detail {
        if !ties.is_null() {
            *ties = d.ties;
        }
    }
    match found {
        RenderedPoseMatch::Exact => {
            if let Some(d) = detail {
                if !out.is_null() {
                    *out = d.pose;
                }
                if !age.is_null() {
                    *age = d.age;
                }
            }
            count(&CyberpunkVR_PlacedPoseMatch);
            true
        }
        RenderedPoseMatch::Ambiguous => {
            count(&CyberpunkVR_PlacedPoseAmbiguous);
            false
        }
        RenderedPoseMatch::Missing => {
            count(&CyberpunkVR_PlacedPoseMiss);
            false
        }
    }
}

// ---- the one-instant packets, behind sequence locks: odd = a write in progress, the reader retries; even and
// unchanged across the read = the copy is whole

struct SeqLock<const W: usize> {
    seq: AtomicU32,
    words: [AtomicU32; W],
}
impl<const W: usize> SeqLock<W> {
    const fn new() -> Self {
        SeqLock { seq: AtomicU32::new(0), words: [const { AtomicU32::new(0) }; W] }
    }
    fn publish(&self, words: &[u32; W]) {
        let s = self.seq.load(Ordering::Relaxed).wrapping_add(1); // odd
        self.seq.store(s, Ordering::Relaxed);
        fence(Ordering::Release);
        for (slot, w) in self.words.iter().zip(words) {
            slot.store(*w, Ordering::Relaxed);
        }
        fence(Ordering::Release);
        self.seq.store(s.wrapping_add(1), Ordering::Relaxed); // even = complete
    }
    fn read(&self) -> Option<[u32; W]> {
        for _ in 0..8 {
            let s0 = self.seq.load(Ordering::Acquire);
            if s0 == 0 {
                return None; // nothing published yet
            }
            if s0 & 1 != 0 {
                continue; // write in progress
            }
            let mut out = [0u32; W];
            for (o, slot) in out.iter_mut().zip(&self.words) {
                *o = slot.load(Ordering::Relaxed);
            }
            fence(Ordering::Acquire);
            if self.seq.load(Ordering::Relaxed) == s0 {
                return Some(out);
            }
        }
        None
    }
}
/// A repr(C) packet of 4-byte fields as its words.
unsafe fn to_words<T: Copy, const W: usize>(v: &T) -> [u32; W] {
    debug_assert_eq!(core::mem::size_of::<T>(), W * 4);
    core::mem::transmute_copy(v)
}
unsafe fn from_words<T: Copy, const W: usize>(w: &[u32; W]) -> T {
    core::mem::transmute_copy(w)
}
const LOCATED_WORDS: usize = core::mem::size_of::<LocatedCameraFrame>() / 4;
const VIEW_WORDS: usize = core::mem::size_of::<ViewFrame>() / 4;
const BARREL_WORDS: usize = core::mem::size_of::<BarrelFrame>() / 4;
static LOCATED: SeqLock<LOCATED_WORDS> = SeqLock::new();
static VIEW: SeqLock<VIEW_WORDS> = SeqLock::new();
static BARREL: SeqLock<BARREL_WORDS> = SeqLock::new();

#[no_mangle]
pub unsafe extern "C" fn cvr_camera_located_frame_publish(f: *const LocatedCameraFrame) {
    LOCATED.publish(&to_words(&*f));
}
#[no_mangle]
pub unsafe extern "C" fn cvr_camera_located_frame_read(out: *mut LocatedCameraFrame) -> bool {
    if out.is_null() {
        return false;
    }
    match LOCATED.read() {
        Some(w) => {
            *out = from_words(&w);
            true
        }
        None => false,
    }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_camera_view_frame_publish(f: *const ViewFrame) {
    VIEW.publish(&to_words(&*f));
}
#[no_mangle]
pub unsafe extern "C" fn cvr_camera_view_frame_read(out: *mut ViewFrame) -> bool {
    if out.is_null() {
        return false;
    }
    match VIEW.read() {
        Some(w) => {
            *out = from_words(&w);
            true
        }
        None => false,
    }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_camera_barrel_frame_publish(f: *const BarrelFrame) {
    BARREL.publish(&to_words(&*f));
}
#[no_mangle]
pub unsafe extern "C" fn cvr_camera_barrel_frame_read(out: *mut BarrelFrame) -> bool {
    if out.is_null() {
        return false;
    }
    match BARREL.read() {
        Some(w) => {
            *out = from_words(&w);
            true
        }
        None => false,
    }
}

/// The engine's body yaw for the hand anchor (formerly CameraMath.cpp): its quaternion's Z/W, or `fallback` in a
/// vehicle, without one, or for a non-unit reading.
#[no_mangle]
pub unsafe extern "C" fn cvr_camera_body_anchor_yaw(fallback: f32) -> f32 {
    if g_isInVehicle || CyberpunkVR_EngineBodyYawValid == 0 {
        return fallback;
    }
    let (z, w) = (CyberpunkVR_EngineBodyYawZ, CyberpunkVR_EngineBodyYawW);
    if !z.is_finite() || !w.is_finite() || z * z + w * w < 0.5 {
        return fallback;
    }
    2.0 * atan2(z, w)
}
