//! THE FRAME GENERATOR'S PRESENTER (formerly src/Framegen/Presenter.cpp, with include/Framegen/FramePolicy.hpp's Cadence
//! and CanInterpolate): which picture a display cycle shows -- the newest real capture, a generated midpoint between the
//! previous real capture and it, or the real endpoint a midpoint was shown ahead of -- and the generation itself, through
//! the generator (framegen_generator.rs).
//!
//! A generated midpoint is always followed by its real endpoint: the pending endpoint is never replaced with a newer
//! capture, or motion reverses or skips time. The selected image may be a midpoint or the pending real B while the newest
//! capture is already C: timing and identity follow the selected pixels.
//!
//! The frame each cycle hands it is the frame loop's newest captured pair (formerly OpenXRFrameLoop.cpp's fgFrame,
//! cvr_xr_framegen_select): both eyes' pictures, poses, FOVs and labels, the frame generator's inputs
//! (framegen_inputs.rs' records), and the pair's read lease (ReadLease, the captured pictures' reservations in the
//! capture state, xr_capture_state.rs).
use core::ffi::c_void;
use core::sync::atomic::{AtomicI32, AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};

use cvr_gpu::frame_capture::{CaptureEye, EyeCamera};
use cvr_gpu::framegen_core::{self as core_fg, Settings};
use openxr_sys as xr;
use windows::core::Interface as _;
use windows::Win32::Graphics::Direct3D12::{ID3D12Fence, ID3D12Resource};

use crate::framegen_inputs::{cvr_framegen_release_inputs, cvr_framegen_retain_inputs, Inputs};
use crate::ofxr::{BridgeCheck, BridgeStatus};

/// cvr::framegen::Identity: a frame's serial, tracking origin, engine frame, the eyes' pose ids, capture time, and
/// whether it is a stereo pair with matching inputs (and a camera reset).
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct Identity {
    pub serial: u64,
    pub origin: u64,
    pub native_frame: u32,
    pub pose: [u64; 2],
    pub captured_ms: f64,
    pub stereo: bool,
    pub inputs: bool,
    pub reset: bool,
}
const _: () = assert!(
    core::mem::size_of::<Identity>() == 56
        && core::mem::offset_of!(Identity, pose) == 24
        && core::mem::offset_of!(Identity, captured_ms) == 40
        && core::mem::offset_of!(Identity, stereo) == 48
);

/// FramePolicy.hpp CanInterpolate: two consecutive engine frames of one stereo pair line, inputs matching, no reset, both
/// poses labelled, at most 100 ms apart.
pub fn can_interpolate(a: &Identity, b: &Identity) -> bool {
    let dt = b.captured_ms - a.captured_ms;
    a.serial != 0
        && b.serial > a.serial
        && a.origin == b.origin
        && a.origin != 0
        && b.native_frame.wrapping_sub(a.native_frame) == 1
        && a.stereo
        && b.stereo
        && a.inputs
        && b.inputs
        && !b.reset
        && a.pose[0] != 0
        && a.pose[1] != 0
        && b.pose[0] != 0
        && b.pose[1] != 0
        && dt.is_finite()
        && dt > 0.0
        && dt <= 100.0
}

/// cvr::framegen::Cadence::Next.
#[repr(i32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Next {
    Repeat = 0,
    #[default]
    Real = 1,
    Midpoint = 2,
    PendingReal = 3,
    /// FSR3 extrapolation's frame shown on a cycle that would repeat the real one (main's 5fa0c8b57: counted as a
    /// generated frame, not as a repeat).
    Extrapolated = 4,
}
/// FramePolicy.hpp Cadence.
#[derive(Default)]
struct Cadence {
    pending: u64,
    displayed: u64,
}
impl Cadence {
    fn select(&self, serial: u64, pair_ready: bool, enabled: bool) -> Next {
        if self.pending != 0 {
            return Next::PendingReal;
        }
        if serial == 0 || serial == self.displayed {
            return Next::Repeat;
        }
        if enabled && pair_ready {
            Next::Midpoint
        } else {
            Next::Real
        }
    }
    fn accepted(&mut self, kind: Next, serial: u64) {
        if kind == Next::Midpoint {
            self.pending = serial;
        } else if kind == Next::Real || kind == Next::PendingReal {
            self.displayed = serial;
            self.pending = 0;
        }
    }
    fn reset(&mut self) {
        self.pending = 0;
        self.displayed = 0;
    }
}

/// A reference to the frame generator's inputs record (retained, released when dropped).
pub(crate) struct InputsRef(*const Inputs);
impl InputsRef {
    /// Takes over one reference.
    fn adopt(p: *const Inputs) -> Option<Self> {
        (!p.is_null()).then_some(Self(p))
    }
    pub(crate) fn get(&self) -> &Inputs {
        // SAFETY: a held reference.
        unsafe { &*self.0 }
    }
}
impl Clone for InputsRef {
    fn clone(&self) -> Self {
        // SAFETY: a held reference.
        Self(unsafe { cvr_framegen_retain_inputs(self.0) })
    }
}
impl Drop for InputsRef {
    fn drop(&mut self) {
        // SAFETY: the reference held, released once.
        unsafe { cvr_framegen_release_inputs(self.0) };
    }
}

/// cvr::framegen::ReadLease: a captured pair's read reservation -- each picture pinned in the capture state with a
/// reference of the lease's own, held while a selection, the pending real endpoint or a generation reads it, and released
/// through the capture state (ReleaseCapturedRead) once the GPU passed the fence its last read ends at.
struct ReadLease {
    resources: [*mut c_void; 2],
    fence: AtomicU64,
    /// The submit fence the reads are counted on (a reference of the lease's own).
    submit: Option<ID3D12Fence>,
}
// SAFETY: the pictures' references are the lease's own; the capture state is thread-safe.
unsafe impl Send for ReadLease {}
unsafe impl Sync for ReadLease {}
impl Drop for ReadLease {
    fn drop(&mut self) {
        let fence = self.fence.load(Relaxed);
        for &resource in &self.resources {
            if resource.is_null() {
                continue;
            }
            // SAFETY: the lease's fence reference.
            let completed = self
                .submit
                .as_ref()
                .map_or(u64::MAX, |f| unsafe { f.GetCompletedValue() });
            // SAFETY: the lease's own reference, handed over.
            unsafe {
                crate::xr_capture_state::cvr_xr_capture_release_read(resource, fence, completed)
            };
        }
    }
}
/// A reference to a read lease (shared_ptr<ReadLease>).
#[derive(Clone)]
pub(crate) struct Lease(Arc<ReadLease>);
impl Lease {
    fn raise(&self, value: u64) {
        self.0.fence.fetch_max(value, Relaxed);
    }
}

/// cvr::framegen::Frame.
#[derive(Clone)]
struct Frame {
    identity: Identity,
    color: [usize; 2],
    pose: [xr::Posef; 2],
    fov: [xr::Fovf; 2],
    inputs: [Option<InputsRef>; 2],
    lease: Option<Lease>,
}
/// The zeroed pose (the C++ `{}`).
fn zero_pose() -> xr::Posef {
    xr::Posef {
        orientation: xr::Quaternionf {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 0.0,
        },
        position: xr::Vector3f {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
    }
}
impl Default for Frame {
    fn default() -> Self {
        let pose = zero_pose();
        let fov = xr::Fovf {
            angle_left: 0.0,
            angle_right: 0.0,
            angle_up: 0.0,
            angle_down: 0.0,
        };
        Self {
            identity: Identity::default(),
            color: [0; 2],
            pose: [pose; 2],
            fov: [fov; 2],
            inputs: [None, None],
            lease: None,
        }
    }
}

/// What the frame loop gets back of the cycle's selection: the pictures it shows and whether they are synthetic.
#[repr(C)]
pub struct SelectedOut {
    pub color: [*mut c_void; 2],
    pub synthetic: bool,
}
const _: () = assert!(core::mem::size_of::<SelectedOut>() == 24);

/// The selected frame as the depth layer reads it (depth_layer.rs): synthetic or not, its identity, the eyes' poses and
/// the frame generator's inputs (references of its own).
pub(crate) struct DepthSource {
    pub synthetic: bool,
    pub identity: Identity,
    pub pose: [xr::Posef; 2],
    inputs: [Option<InputsRef>; 2],
}
impl DepthSource {
    pub fn input(&self, eye: usize) -> Option<&Inputs> {
        self.inputs[eye].as_ref().map(InputsRef::get)
    }
}
/// The selection handed out last, for the depth layer (none before a select or after its submission).
pub(crate) fn depth_source() -> Option<DepthSource> {
    let p = presenter();
    p.last.as_ref().map(|s| DepthSource {
        synthetic: s.synthetic,
        identity: s.frame.identity,
        pose: s.frame.pose,
        inputs: s.frame.inputs.clone(),
    })
}

struct Selection {
    frame: Frame,
    kind: Next,
    /// Sources read by generation even when its displayed image is synthetic.
    generated_from: Option<Lease>,
    synthetic: bool,
}

struct Presenter {
    external_bridge: BridgeCheck,
    cadence: Cadence,
    generator: *mut crate::framegen_generator::Generator,
    retire_generator: bool,
    pending: Frame,
    previous: Frame,
    primed: bool,
    generated_serial: u64,
    sdk_frame: u64,
    /// The selection handed out last: its leases' fences are raised when it is submitted.
    last: Option<Selection>,
    /// The backend the cycles ran with: a change starts over.
    active_backend: i32,
    /// FSR3 extrapolation: the capture to predict from on the next cycle that still has it, and the one predicted from
    /// (its pose half a frame ahead).
    predict_next: u64,
    predicted_serial: u64,
    predicted_pose: [xr::Posef; 2],
}
// SAFETY: the XR thread's own presenter, used under the lock.
unsafe impl Send for Presenter {}
static PRESENTER: LazyLock<Mutex<Presenter>> = LazyLock::new(|| {
    Mutex::new(Presenter {
        external_bridge: BridgeCheck::default(),
        cadence: Cadence::default(),
        generator: crate::framegen_generator::cvr_framegen_generator_new(),
        retire_generator: false,
        pending: Frame::default(),
        previous: Frame::default(),
        primed: false,
        generated_serial: 0,
        sdk_frame: 0,
        last: None,
        active_backend: 1,
        predict_next: 0,
        predicted_serial: 0,
        predicted_pose: [zero_pose(); 2],
    })
});
fn presenter() -> MutexGuard<'static, Presenter> {
    PRESENTER.lock().unwrap_or_else(|e| e.into_inner())
}

extern "C" {
    static CyberpunkVR_FramegenCapture: AtomicI32;
}

fn status(text: &core::ffi::CStr, active: bool) {
    // SAFETY: a NUL-terminated text.
    unsafe { core_fg::cvr_framegen_status(text.as_ptr(), active) }
}
pub(crate) fn diagnostics() -> bool {
    extern "C" {
        static CyberpunkVR_RuntimeDiagnostics: i32;
    }
    // SAFETY: an export of the C++ half.
    unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_RuntimeDiagnostics) != 0 }
}

/// The pose half way between two (positions averaged, orientations by the shorter arc, normalised).
fn middle(a: &xr::Posef, b: &xr::Posef) -> xr::Posef {
    let (x, y) = (&a.orientation, &b.orientation);
    let sign = if x.x * y.x + x.y * y.y + x.z * y.z + x.w * y.w < 0.0 {
        -1.0
    } else {
        1.0
    };
    let mut q = xr::Quaternionf {
        x: x.x + sign * y.x,
        y: x.y + sign * y.y,
        z: x.z + sign * y.z,
        w: x.w + sign * y.w,
    };
    let norm = (q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w).sqrt();
    if norm > 0.0 {
        q = xr::Quaternionf {
            x: q.x / norm,
            y: q.y / norm,
            z: q.z / norm,
            w: q.w / norm,
        };
    } else {
        q = xr::Quaternionf {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        };
    }
    xr::Posef {
        orientation: q,
        position: xr::Vector3f {
            x: (a.position.x + b.position.x) * 0.5,
            y: (a.position.y + b.position.y) * 0.5,
            z: (a.position.z + b.position.z) * 0.5,
        },
    }
}
fn multiply(p: &xr::Quaternionf, q: &xr::Quaternionf) -> xr::Quaternionf {
    xr::Quaternionf {
        x: p.w * q.x + p.x * q.w + p.y * q.z - p.z * q.y,
        y: p.w * q.y - p.x * q.z + p.y * q.w + p.z * q.x,
        z: p.w * q.z + p.x * q.y - p.y * q.x + p.z * q.w,
        w: p.w * q.w - p.x * q.x - p.y * q.y - p.z * q.z,
    }
}
/// `b` moved on past it by `f` of the step from `a` to `b`: position b + f(b - a), and the rotation from a to b applied
/// again, f of it, on top of b (FSR3 extrapolation's pose).
fn ahead(a: &xr::Posef, b: &xr::Posef, f: f32) -> xr::Posef {
    let (p, q) = (&a.position, &b.position);
    let position = xr::Vector3f {
        x: q.x + (q.x - p.x) * f,
        y: q.y + (q.y - p.y) * f,
        z: q.z + (q.z - p.z) * f,
    };
    let bo = &b.orientation;
    let mut from = a.orientation;
    if from.x * bo.x + from.y * bo.y + from.z * bo.z + from.w * bo.w < 0.0 {
        from = xr::Quaternionf {
            x: -from.x,
            y: -from.y,
            z: -from.z,
            w: -from.w,
        };
    }
    // b = step * a
    let mut step = multiply(
        bo,
        &xr::Quaternionf {
            x: -from.x,
            y: -from.y,
            z: -from.z,
            w: from.w,
        },
    );
    if step.w < 0.0 {
        step = xr::Quaternionf {
            x: -step.x,
            y: -step.y,
            z: -step.z,
            w: -step.w,
        };
    }
    let s = (1.0 - step.w * step.w).max(0.0).sqrt();
    let mut orientation = *bo;
    if s > 1e-6 {
        let half = step.w.min(1.0).acos() * f;
        let k = half.sin() / s;
        let q = multiply(
            &xr::Quaternionf {
                x: step.x * k,
                y: step.y * k,
                z: step.z * k,
                w: half.cos(),
            },
            bo,
        );
        let norm = (q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w).sqrt();
        orientation = if norm > 0.0 {
            xr::Quaternionf {
                x: q.x / norm,
                y: q.y / norm,
                z: q.z / norm,
                w: q.w / norm,
            }
        } else {
            *bo
        };
    }
    xr::Posef { orientation, position }
}
fn camera(pose: &xr::Posef, fov: &xr::Fovf) -> EyeCamera {
    let q = &pose.orientation;
    EyeCamera {
        orientation: [q.x, q.y, q.z, q.w],
        fov: [
            fov.angle_left,
            fov.angle_right,
            fov.angle_up,
            fov.angle_down,
        ],
    }
}
fn same_projection(a: &Frame, b: &Frame) -> bool {
    for eye in 0..2 {
        let (x, y) = (&a.fov[eye], &b.fov[eye]);
        if (x.angle_left - y.angle_left).abs() > 0.001
            || (x.angle_right - y.angle_right).abs() > 0.001
            || (x.angle_up - y.angle_up).abs() > 0.001
            || (x.angle_down - y.angle_down).abs() > 0.001
        {
            return false;
        }
        let (q, r) = (&a.pose[eye].orientation, &b.pose[eye].orientation);
        if (q.x * r.x + q.y * r.y + q.z * r.z + q.w * r.w).abs() < 0.95 {
            return false;
        }
    }
    true
}
/// FrameCapture.cpp CaptureFrame: the frame's pictures, inputs and cameras to the capture tool, while it is armed.
fn capture_frame(
    device: *mut c_void,
    queue: *mut c_void,
    f: &Frame,
    cameras: &[[EyeCamera; 2]; 2],
) {
    // SAFETY: the C++ half's export.
    if unsafe { CyberpunkVR_FramegenCapture.load(Relaxed) } != 1 {
        return;
    }
    let (Some(a), Some(b)) = (&f.inputs[0], &f.inputs[1]) else {
        return;
    };
    let eye = |i: &Inputs, e: usize| CaptureEye {
        depth: i
            .depth
            .as_ref()
            .map_or(core::ptr::null_mut(), windows::core::Interface::as_raw),
        motion: i
            .motion
            .as_ref()
            .map_or(core::ptr::null_mut(), windows::core::Interface::as_raw),
        camera: &i.camera,
        eye: &cameras[e][1],
        pose_id: i.pose_id,
        frame_id: i.frame_id,
        has_motion: i.has_motion,
    };
    let eyes = [eye(a.get(), 0), eye(b.get(), 1)];
    let colors = [f.color[0] as *mut c_void, f.color[1] as *mut c_void];
    // SAFETY: the C++ half's export, live objects.
    unsafe {
        cvr_gpu::frame_capture::cvr_gpu_framegen_capture_frame(
            &raw const CyberpunkVR_FramegenCapture as *mut AtomicI32,
            device,
            queue,
            colors.as_ptr(),
            eyes.as_ptr(),
        )
    };
}

impl Presenter {
    /// Throw away both endpoints. The next valid pair primes the backend with the reset flag.
    fn reset_history(&mut self, why: &str, keep_anr: bool) {
        if self.primed {
            // rate-limited: the first eight, then every 500th (a GPU-bound frame generator resets on each refused cycle)
            static RESETS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
            let n = RESETS.fetch_add(1, Relaxed);
            if n < 8 || n % 500 == 0 {
                let action = if keep_anr { "ANR history reset, resources retained" } else { "generation reset" };
                cvr_host::log::write(&format!("[framegen] {action} ({}): {why}\n", n + 1));
            }
        }
        self.predict_next = 0;
        self.predicted_serial = 0;
        core_fg::cvr_framegen_pacing_ready(false);
        self.cadence.reset();
        self.pending = Frame::default();
        self.previous = Frame::default();
        self.primed = false;
        self.generated_serial = 0;
    }

    fn reset(&mut self, why: &str) {
        self.reset_history(why, false);
        if crate::fsr3::is_backend(self.active_backend) {
            crate::fsr3::reset_history();
            crate::fsr3::release_resources();
        }
        self.retire_generator = true;
        self.poll_retirement();
    }

    fn poll_retirement(&mut self) {
        // SAFETY: the presenter's generator; the backend waits for all its fences before dropping buffers.
        if self.retire_generator && unsafe { crate::framegen_generator::cvr_framegen_generator_reset(self.generator) } {
            self.retire_generator = false;
            if !core_fg::cvr_framegen_depth_capture_enabled() {
                crate::framegen_inputs::cvr_framegen_release_all_inputs();
            }
            if core_fg::cvr_framegen_metrics_enabled() {
                core_fg::cvr_framegen_set_vram(crate::framegen_inputs::cvr_framegen_input_vram());
            }
        }
    }

    fn pause(&mut self, why: &str, settings: &Settings) {
        use crate::framegen_generator::{has_resident_anr, ANR};
        // SAFETY: the presenter's generator, under its lock.
        if settings.enabled && settings.backend == ANR && !self.retire_generator && unsafe { has_resident_anr(self.generator) } {
            self.reset_history(why, true);
        } else {
            self.reset(why);
        }
    }

    fn select(
        &mut self,
        device: *mut c_void,
        queue: *mut c_void,
        current: Frame,
        gameplay: bool,
        display_period_ms: f64,
    ) -> Selection {
        // SAFETY (whole body): the presenter's generator and the caller's live objects.
        unsafe { crate::framegen_generator::cvr_framegen_generator_poll(self.generator) };
        self.poll_retirement();
        crate::fsr3::poll();
        let mut settings = Settings::default();
        core_fg::cvr_framegen_get_settings(&mut settings);
        crate::fsr3::sync(&settings);
        if settings.backend != self.active_backend {
            self.reset("the backend changed");
            self.active_backend = settings.backend;
        }
        if !crate::fsr3::is_backend(self.active_backend) || !settings.enabled {
            crate::fsr3::release_resources();
        }
        let (c, p) = (&current.identity, &self.previous.identity);
        let gap = c.native_frame.wrapping_sub(p.native_frame);
        let pair_gap = self.primed
            && self.cadence.pending == 0
            && c.inputs
            && !c.reset
            && c.origin == p.origin
            && c.serial > p.serial
            && gap > 1
            && gap <= 4
            && c.captured_ms - p.captured_ms <= 100.0
            && same_projection(&self.previous, &current);
        core_fg::cvr_framegen_on_presentation_cycle(pair_gap);
        if !settings.enabled {
            self.reset("generation disabled");
        } else if !gameplay || (self.pending.identity.serial != 0 && self.pending.identity.origin != current.identity.origin) {
            self.pause(if gameplay { "the tracking origin changed" } else { "not gameplay (a menu or an untracked view)" }, &settings);
        }
        if self.cadence.pending != 0 {
            return Selection {
                frame: self.pending.clone(),
                kind: Next::PendingReal,
                generated_from: None,
                synthetic: false,
            };
        }
        let kind = self.cadence.select(current.identity.serial, false, false);
        let mut result = Selection {
            frame: current.clone(),
            kind,
            generated_from: None,
            synthetic: false,
        };
        if !settings.enabled
            && gameplay
            && current.identity.stereo
            && current.color[0] != 0
            && current.color[1] != 0
            && current
                .inputs
                .iter()
                .all(|i| i.as_ref().is_some_and(|i| i.get().has_depth))
            && current.inputs[0].as_ref().map(|i| i.get().frame_id)
                == current.inputs[1].as_ref().map(|i| i.get().frame_id)
        {
            let cams = [
                [
                    camera(&current.pose[0], &current.fov[0]),
                    camera(&current.pose[0], &current.fov[0]),
                ],
                [
                    camera(&current.pose[1], &current.fov[1]),
                    camera(&current.pose[1], &current.fov[1]),
                ],
            ];
            capture_frame(device, queue, &current, &cams);
        }
        if !settings.enabled || !gameplay {
            status(
                if settings.enabled {
                    c"Paused in menus or untracked views"
                } else {
                    c"Disabled"
                },
                false,
            );
            return result;
        }
        if self.retire_generator {
            status(c"Waiting for previous frame generation resources", false);
            return result;
        }
        match self.external_bridge.status() {
            BridgeStatus::Absent => {}
            BridgeStatus::Loaded => {
                self.reset("the external OFXR bridge is loaded");
                status(
                    c"External OFXR is loaded: disarm its tray and restart before native generation",
                    false,
                );
                return result;
            }
            BridgeStatus::Unverified => {
                status(
                    c"Could not verify external OFXR. Restart the game before using native frame generation",
                    false,
                );
                return result;
            }
        }
        if !current.identity.inputs || !current.identity.stereo || current.lease.is_none() {
            self.primed = false;
            self.predict_next = 0;
            self.predicted_serial = 0;
            core_fg::cvr_framegen_pacing_ready(false);
            status(
                c"Waiting for matching depth and motion vectors in both eyes",
                false,
            );
            return result;
        }
        if crate::fsr3::is_backend(self.active_backend) {
            return self.select_fsr3(device, queue, current, self.active_backend, display_period_ms);
        }
        if current.identity.serial == self.generated_serial {
            return result;
        }
        let projection = same_projection(&self.previous, &current);
        let interpolate = self.primed
            && can_interpolate(&self.previous.identity, &current.identity)
            && projection;
        if self.primed && !interpolate && diagnostics() {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            // SAFETY: a plain Win32 call.
            let now = unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() };
            if now >= NEXT.load(Relaxed) {
                NEXT.store(now + 1000, Relaxed);
                cvr_host::log::write(&format!(
                    "[framegen-pair] prev={}/{} now={}/{} dt={:.3} reset={} projection={}\n",
                    self.previous.identity.serial,
                    self.previous.identity.native_frame,
                    current.identity.serial,
                    current.identity.native_frame,
                    current.identity.captured_ms - self.previous.identity.captured_ms,
                    i32::from(current.identity.reset),
                    i32::from(projection)
                ));
            }
        }
        let milliseconds = if interpolate {
            current.identity.captured_ms - self.previous.identity.captured_ms
        } else {
            1000.0 / 45.0
        };
        let mut generated = false;
        let cams = [
            [
                camera(&self.previous.pose[0], &self.previous.fov[0]),
                camera(&current.pose[0], &current.fov[0]),
            ],
            [
                camera(&self.previous.pose[1], &self.previous.fov[1]),
                camera(&current.pose[1], &current.fov[1]),
            ],
        ];
        let colors = [
            current.color[0] as *mut c_void,
            current.color[1] as *mut c_void,
        ];
        let inputs = [
            current.inputs[0]
                .as_ref()
                .map_or(core::ptr::null(), |i| i.0),
            current.inputs[1]
                .as_ref()
                .map_or(core::ptr::null(), |i| i.0),
        ];
        self.sdk_frame += 1;
        // SAFETY: the presenter's generator, the caller's objects, the frame's held inputs.
        let ok = unsafe {
            crate::framegen_generator::cvr_framegen_generator_generate(
                self.generator,
                device,
                queue,
                colors.as_ptr(),
                inputs.as_ptr(),
                &cams,
                &settings,
                milliseconds,
                !interpolate,
                self.sdk_frame,
                &mut generated,
            )
        };
        if !ok {
            self.primed = false;
            core_fg::cvr_framegen_on_skip();
            return result;
        }
        self.generated_serial = current.identity.serial;
        core_fg::cvr_framegen_pacing_ready(true);
        if interpolate && generated {
            result.kind = Next::Midpoint;
            result.synthetic = true;
            result.generated_from = current.lease.clone();
            result.frame.identity.captured_ms =
                (self.previous.identity.captured_ms + current.identity.captured_ms) * 0.5;
            for eye in 0..2 {
                // SAFETY: the presenter's generator.
                result.frame.color[eye] = unsafe {
                    crate::framegen_generator::cvr_framegen_generator_output(
                        self.generator,
                        eye as u32,
                    )
                } as usize;
                result.frame.pose[eye] = middle(&self.previous.pose[eye], &current.pose[eye]);
                // Synthetic pixels have a blended pose, not B's receipt.
                result.frame.identity.pose[eye] = 0;
            }
            self.pending = current.clone();
            status(c"Generating with engine motion vectors and depth", true);
        } else {
            result.generated_from = current.lease.clone();
            status(c"Priming frame history", false);
        }
        let mut previous = current;
        previous.lease = None;
        previous.inputs = [None, None];
        previous.color = [0, 0];
        self.previous = previous;
        self.primed = true;
        result
    }

    /// Presenter::SelectFsr3 (main's 2e123a4a3, 5fa0c8b57). INTERPOLATION: a capture that follows the previous one and
    /// leaves the time for it (longer apart than a display period, and than twice the generation's GPU time) gets the
    /// frame between them first, its real endpoint pending after; any other only primes the history. EXTRAPOLATION: the
    /// real capture is shown at once; a display cycle that would repeat it shows it half a frame ahead instead, its pose
    /// moved as far.
    fn select_fsr3(
        &mut self,
        device: *mut c_void,
        producer: *mut c_void,
        current: Frame,
        backend: i32,
        display_period_ms: f64,
    ) -> Selection {
        let serial = current.identity.serial;
        let kind = self.cadence.select(serial, false, false);
        let mut result = Selection {
            frame: current.clone(),
            kind,
            generated_from: None,
            synthetic: false,
        };
        core_fg::cvr_framegen_publish_active_backend(backend);
        if !crate::fsr3::enabled() {
            status(crate::fsr3::status().text, false);
            core_fg::cvr_framegen_pacing_ready(false);
            return result;
        }
        if serial == self.generated_serial {
            if backend != crate::fsr3::EXTRAPOLATION || result.kind != Next::Repeat {
                return result;
            }
            if self.predict_next == serial {
                self.predict_next = 0;
                if crate::fsr3::generate(device, producer, current.color, &current.inputs, serial, 0.0, current.lease.clone()) {
                    self.predicted_serial = serial;
                    if crate::fsr3::show_generated() {
                        result.kind = Next::Extrapolated;
                    }
                }
            }
            if self.predicted_serial == serial && crate::fsr3::show_generated() {
                for eye in 0..2 {
                    result.frame.color[eye] = crate::fsr3::output(eye, serial) as usize;
                    result.frame.pose[eye] = self.predicted_pose[eye];
                    result.frame.identity.pose[eye] = 0;
                }
                result.synthetic = true;
                result.generated_from = current.lease.clone();
            }
            return result;
        }
        self.predict_next = 0;
        self.predicted_serial = 0;
        let consecutive = self.primed
            && can_interpolate(&self.previous.identity, &current.identity)
            && same_projection(&self.previous, &current);
        let interval = if consecutive {
            current.identity.captured_ms - self.previous.identity.captured_ms
        } else {
            0.0
        };
        let fits = consecutive
            && interval > display_period_ms
            && interval > 2.0 * crate::fsr3::generation_ms();
        if backend == crate::fsr3::INTERPOLATION {
            // A new history starts with a real frame. The matching endpoint stays pinned until xrEndFrame accepts it.
            if !consecutive {
                crate::fsr3::reset_history();
            }
            if !consecutive || fits {
                let made = crate::fsr3::generate(
                    device,
                    producer,
                    current.color,
                    &current.inputs,
                    serial,
                    interval,
                    current.lease.clone(),
                );
                if made && fits && crate::fsr3::show_generated() {
                    result.kind = Next::Midpoint;
                    result.synthetic = true;
                    result.frame.identity.captured_ms =
                        (self.previous.identity.captured_ms + current.identity.captured_ms) * 0.5;
                    for eye in 0..2 {
                        result.frame.color[eye] = crate::fsr3::output(eye, serial) as usize;
                        result.frame.pose[eye] =
                            middle(&self.previous.pose[eye], &current.pose[eye]);
                        result.frame.identity.pose[eye] = 0;
                    }
                    self.pending = current.clone();
                }
            }
        } else if fits {
            // The real capture now; a prediction only if the next display cycle still has that capture.
            for eye in 0..2 {
                self.predicted_pose[eye] = ahead(&self.previous.pose[eye], &current.pose[eye], 0.5);
            }
            self.predict_next = serial;
        }
        self.primed = true;
        self.generated_serial = serial;
        self.previous.identity = current.identity;
        self.previous.pose = current.pose;
        self.previous.fov = current.fov;
        let st = crate::fsr3::status();
        core_fg::cvr_framegen_pacing_ready(
            st.ready || (backend == crate::fsr3::EXTRAPOLATION && fits),
        );
        status(st.text, st.ready && !st.faulted);
        result
    }

    fn submitted(&mut self, fence: *mut c_void, value: u64, accepted: bool, retain_endpoint: bool) {
        if let Some(result) = &self.last {
            if let Some(lease) = &result.frame.lease {
                lease.raise(value);
            }
            if let Some(lease) = &result.generated_from {
                lease.raise(value);
            }
        }
        // SAFETY: the presenter's generator and the submit fence.
        unsafe {
            crate::framegen_generator::cvr_framegen_generator_consumer_fence(
                self.generator,
                fence,
                value,
            )
        };
        if crate::fsr3::is_backend(self.active_backend) {
            crate::fsr3::consumer_fence(fence, value);
        }
        let Some(result) = self.last.take() else {
            return;
        };
        // A transient swapchain/submit failure must not discard B after its midpoint was accepted: the pinned endpoint is
        // kept for the next cycle; a recenter, a menu transition or an explicit reset still invalidates it (main's 0.2.4).
        if !accepted {
            if !retain_endpoint || result.kind != Next::PendingReal {
                let mut settings = Settings::default();
                core_fg::cvr_framegen_get_settings(&mut settings);
                self.pause(if retain_endpoint { "a frame not accepted (not a pending real one)" } else { "a frame not accepted" }, &settings);
            }
            return;
        }
        self.cadence
            .accepted(result.kind, result.frame.identity.serial);
        if core_fg::cvr_framegen_metrics_enabled() {
            core_fg::cvr_framegen_on_submitted(
                result.frame.identity.serial,
                result.synthetic,
                result.kind == Next::Repeat,
                core_fg::cvr_framegen_now_ms(),
            );
        }
        if result.kind == Next::PendingReal {
            self.pending = Frame::default();
        }
    }
}

/// The frame loop's newest captured pair for the presenter (formerly OpenXRFrameLoop.cpp's fgFrame): the main eye's
/// picture and the second eye's, the eyes' poses, FOVs and labels, the serial and capture time (written back with the
/// selected frame's), and the frame generator's inputs.
#[repr(C)]
pub struct CapturedSelectIn {
    pub device: *mut c_void,
    pub queue: *mut c_void,
    /// The submit fence (ID3D12Fence) the reads are counted on.
    pub submit_fence: *mut c_void,
    /// The main eye's picture and the second eye's (the frame loop's pinned references).
    pub color: *mut c_void,
    pub second: *mut c_void,
    pub poses: *mut [xr::Posef; 2],
    pub fovs: *mut [xr::Fovf; 2],
    pub pose_ids: *mut [u64; 2],
    pub serial: *mut u64,
    pub captured_ms: *mut f64,
    /// The inputs references, handed over (null when none).
    pub inputs: [*const Inputs; 2],
    /// The tracking origin's serial.
    pub origin: u64,
    pub view_count: u32,
    pub main_eye: u32,
    pub gameplay: bool,
}
const _: () = assert!(
    core::mem::size_of::<CapturedSelectIn>() == 120
        && core::mem::offset_of!(CapturedSelectIn, inputs) == 80
        && core::mem::offset_of!(CapturedSelectIn, gameplay) == 112
);

/// One more reference to a live picture.
fn retain(resource: usize) {
    let p = resource as *mut c_void;
    // SAFETY: a live picture of the caller's.
    if let Some(r) = unsafe { ID3D12Resource::from_raw_borrowed(&p) } {
        core::mem::forget(r.clone());
    }
}

/// THE CYCLE'S SELECTION (formerly OpenXRFrameLoop.cpp's fgFrame and Presenter::Select): with both eyes' pictures of a
/// two-view frame, the frame built -- its identity's inputs those of one engine frame with depth and motion, each eye's
/// labelled with its pose and the tracking origin, a camera reset carried --, with generation on and such inputs the
/// pair's read lease taken (both pictures pinned, or none), and the presenter's choice made: the poses, FOVs, labels,
/// serial and capture time written back with the selected frame's, `out` its pictures -> true. Without a pair, the presenter reset -> false. The inputs references are taken over either way.
///
/// # Safety
/// `input` and `out` are valid with their pointers; the device, queue, fence and pictures live.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_framegen_select(
    input: *const CapturedSelectIn,
    out: *mut SelectedOut,
) -> bool {
    // SAFETY: the caller's records.
    let (Some(i), Some(out)) = (unsafe { input.as_ref() }, unsafe { out.as_mut() }) else {
        return false;
    };
    let inputs = [InputsRef::adopt(i.inputs[0]), InputsRef::adopt(i.inputs[1])];
    if i.color.is_null() || i.second.is_null() || i.view_count != 2 {
        // Under SteamVR a pending endpoint outlives a cycle without a pair (main's 0.2.4); a menu or a stopped session
        // resets it there too.
        if !cvr_xr::runtime_info::RUNTIME.steamvr.load(Relaxed) || !i.gameplay {
            let mut p = presenter();
            p.last = None;
            let mut settings = Settings::default();
            core_fg::cvr_framegen_get_settings(&mut settings);
            p.pause(if i.gameplay { "a cycle without a stereo pair" } else { "a cycle without a pair outside gameplay" }, &settings);
        }
        drop(inputs);
        return false;
    }
    // SAFETY: the caller's locals.
    let (poses, fovs, ids, serial, captured_ms) = unsafe {
        (
            &mut *i.poses,
            &mut *i.fovs,
            &mut *i.pose_ids,
            &mut *i.serial,
            &mut *i.captured_ms,
        )
    };
    let main = (i.main_eye & 1) as usize;
    let mut frame = Frame::default();
    frame.color[main] = i.color as usize;
    frame.color[main ^ 1] = i.second as usize;
    let id = &mut frame.identity;
    id.serial = *serial;
    id.origin = i.origin;
    id.captured_ms = *captured_ms;
    id.stereo = true;
    id.inputs = match (&inputs[0], &inputs[1]) {
        (Some(a), Some(b)) => a.get().frame_id == b.get().frame_id,
        _ => false,
    };
    for eye in 0..2 {
        frame.pose[eye] = poses[eye];
        frame.fov[eye] = fovs[eye];
        let id = &mut frame.identity;
        id.pose[eye] = ids[eye];
        if let Some(r) = &inputs[eye] {
            let r = r.get();
            id.native_frame = r.frame_id;
            id.inputs = id.inputs
                && r.has_depth
                && r.has_motion
                && r.pose_id == ids[eye]
                && r.origin == id.origin;
            id.reset = id.reset || r.camera.reset;
        }
    }
    frame.inputs = inputs;
    if core_fg::cvr_framegen_enabled() && frame.identity.inputs {
        let fence = i.submit_fence;
        let mut lease = ReadLease {
            resources: [core::ptr::null_mut(); 2],
            fence: AtomicU64::new(0),
            // SAFETY: the caller's live fence.
            submit: unsafe { ID3D12Fence::from_raw_borrowed(&fence) }.cloned(),
        };
        for eye in 0..2 {
            if crate::xr_capture_state::cvr_xr_capture_pin(frame.color[eye] as *mut c_void) {
                retain(frame.color[eye]);
                lease.resources[eye] = frame.color[eye] as *mut c_void;
            }
        }
        if !lease.resources[0].is_null() && !lease.resources[1].is_null() {
            frame.lease = Some(Lease(Arc::new(lease)));
        }
    }
    let mut p = presenter();
    // the headset's display period, as the frame loop's last xrWaitFrame predicted it
    let display_period_ms =
        cvr_xr::submit_state::SUBMIT.predicted_display_period_ns.load(Relaxed) as f64 * 1e-6;
    let selection = p.select(i.device, i.queue, frame, i.gameplay, display_period_ms);
    for eye in 0..2 {
        poses[eye] = selection.frame.pose[eye];
        fovs[eye] = selection.frame.fov[eye];
        ids[eye] = selection.frame.identity.pose[eye];
    }
    // The selected image may be a midpoint or the pending real B while the newest capture is already C: timing and
    // identity follow the selected pixels too.
    *serial = selection.frame.identity.serial;
    *captured_ms = selection.frame.identity.captured_ms;
    *out = SelectedOut {
        color: [
            selection.frame.color[0] as *mut c_void,
            selection.frame.color[1] as *mut c_void,
        ],
        synthetic: selection.synthetic,
    };
    p.last = Some(selection);
    true
}
/// Presenter::Submitted for the selection handed out last: its leases' fences raised to `value`, the generator told the
/// consumer's fence; accepted, the cadence moves on, else everything resets.
///
/// # Safety
/// `fence` is the submit's ID3D12Fence or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_framegen_presenter_submitted(
    fence: *mut c_void,
    value: u64,
    accepted: bool,
) {
    presenter().submitted(fence, value, accepted, false);
}
/// Presenter::Submitted with retainEndpoint: a refused cycle under SteamVR keeps the real endpoint a midpoint led to.
///
/// # Safety
/// As cvr_framegen_presenter_submitted.
pub unsafe fn submitted_retaining(fence: *mut c_void, value: u64, accepted: bool, retain_endpoint: bool) {
    presenter().submitted(fence, value, accepted, retain_endpoint);
}

/// The real endpoint a midpoint already accepted by the runtime owns (Presenter::PendingFrame), for a cycle whose new
/// capture is not ready or lacks its pair: both pictures pinned and referenced (the caller's, ReleaseCapturedRead ends
/// them), its inputs retained (handed to the selection).
pub struct Owed {
    pub color: [*mut c_void; 2],
    pub serial: u64,
    pub captured_ms: f64,
    pub poses: [xr::Posef; 2],
    pub fovs: [xr::Fovf; 2],
    pub pose_ids: [u64; 2],
    pub inputs: [*const Inputs; 2],
}
/// The owed endpoint of the current tracking origin, or None.
pub fn pending_endpoint(origin: u64) -> Option<Owed> {
    let p = presenter();
    if p.cadence.pending == 0 {
        return None;
    }
    let owed = &p.pending;
    if owed.identity.origin != origin || owed.lease.is_none() || owed.color[0] == 0 || owed.color[1] == 0 {
        return None;
    }
    let mut color = [core::ptr::null_mut(); 2];
    for eye in 0..2 {
        let c = owed.color[eye] as *mut c_void;
        if crate::xr_capture_state::cvr_xr_capture_pin(c) {
            retain(owed.color[eye]);
            color[eye] = c;
        }
    }
    let inputs = [0, 1].map(|eye| {
        owed.inputs[eye].as_ref().map_or(core::ptr::null(), |r| {
            // SAFETY: a held reference, one more taken for the caller.
            unsafe { cvr_framegen_retain_inputs(r.0) }
        })
    });
    Some(Owed {
        color,
        serial: owed.identity.serial,
        captured_ms: owed.identity.captured_ms,
        poses: owed.pose,
        fovs: owed.fov,
        pose_ids: owed.identity.pose,
        inputs,
    })
}
/// Presenter::Reset.
#[no_mangle]
pub extern "C" fn cvr_framegen_presenter_reset() {
    presenter_reset("an explicit reset (Presenter::Reset)");
}
fn presenter_reset(why: &str) {
    let mut p = presenter();
    p.last = None;
    p.reset(why);
}
/// The cycle trace's view of the selection handed out last: its cadence kind and native frame ((-1, 0) without one).
pub fn last_selection_trace() -> (f64, f64) {
    let p = presenter();
    p.last.as_ref().map_or((-1.0, 0.0), |s| (s.kind as i32 as f64, s.frame.identity.native_frame as f64))
}
/// Presenter::Pending: a real endpoint waits after a midpoint.
#[no_mangle]
pub extern "C" fn cvr_framegen_presenter_pending() -> bool {
    presenter().cadence.pending != 0
}
/// Presenter::WantsFsr3Cycle: FSR3 has a frame for one more display cycle -- the real endpoint after an interpolated
/// frame, or the extrapolated frame after the real one (the inline frame loop runs a second cycle for it).
pub fn wants_fsr3_cycle() -> bool {
    let p = presenter();
    (p.active_backend == crate::fsr3::INTERPOLATION && p.cadence.pending != 0) || p.predict_next != 0
}
