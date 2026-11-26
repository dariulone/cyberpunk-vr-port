//! THE CAPTURE'S STATE (formerly OpenXRManager's members under m_presentMutex: m_monoPool, m_monoCapturedFrame,
//! m_captureLeases, the VRCAM eye pool with its serials and labels, the retired captured reads): what the game's
//! Present captures and the XR thread submits.
//!
//! A POOL OF THREE PER EYE, ROTATED BY THE PRODUCER. The producer writes the next buffer of its pool, never the one just
//! published, so it neither waits for the submit thread nor skips: skipping the capture while the consumer was busy kept
//! Present free but starved the headset (the submit thread holds the snapshot for a good part of every display frame).
//! A pointer is not a version of its pixels: readers and in-progress writers RESERVE a resource (the leases), and a
//! reader's reference outlives its GPU reads -- a read released before its fence passes is retired until it does.
//!
//! The published frame is one locked step: image, poses, FOVs, labels, serial and the frame generator's inputs become
//! visible together, and the buffer the consumer was reading before stays untouched.
use cvr_host::fast::FastMap;
use core::ffi::c_void;
use core::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::{LazyLock, Mutex, MutexGuard};

use openxr_sys as xr;
use windows::core::{Interface as _, HSTRING};
use windows::Win32::Graphics::Direct3D12::{
    ID3D12Device, ID3D12Fence, ID3D12Resource, D3D12_HEAP_FLAG_NONE, D3D12_HEAP_PROPERTIES,
    D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_DESC, D3D12_RESOURCE_DIMENSION_TEXTURE2D,
    D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET, D3D12_RESOURCE_STATES, D3D12_RESOURCE_STATE_COPY_DEST,
    D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_TEXTURE_LAYOUT_UNKNOWN,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT, DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC};

use crate::framegen_inputs::{cvr_framegen_release_inputs, cvr_framegen_retain_inputs, Inputs};

/// The second eye's slots.
pub const VRCAM_SLOTS: usize = 3;

extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static CyberpunkVR_DebugCapOk: AtomicU64;
}

/// One resource's use: readers, and a write in progress.
#[derive(Clone, Copy, Default)]
struct Use {
    readers: u32,
    writing: bool,
}
/// The leases on the pools' resources (cvr::capture::BufferLeases), by address.
#[derive(Default)]
struct Leases(FastMap<usize, Use>);
impl Leases {
    fn begin_write(&mut self, r: usize) -> bool {
        if r == 0 {
            return false;
        }
        let v = self.0.entry(r).or_default();
        if v.readers != 0 || v.writing {
            return false;
        }
        v.writing = true;
        true
    }
    fn end_write(&mut self, r: usize) {
        if let Some(v) = self.0.get_mut(&r) {
            v.writing = false;
            if v.readers == 0 {
                self.0.remove(&r);
            }
        }
    }
    fn pin(&mut self, r: usize) -> bool {
        if r == 0 {
            return false;
        }
        let v = self.0.entry(r).or_default();
        if v.writing {
            return false;
        }
        v.readers += 1;
        true
    }
    fn unpin(&mut self, r: usize) {
        if let Some(v) = self.0.get_mut(&r) {
            v.readers = v.readers.saturating_sub(1);
            if v.readers == 0 && !v.writing {
                self.0.remove(&r);
            }
        }
    }
    fn writing(&self, r: usize) -> bool {
        self.0.get(&r).is_some_and(|v| v.writing)
    }
}

/// The pose a second eye's picture was rendered at (OpenXRManager::CapturedEyeLabel).
#[derive(Clone, Copy)]
struct Label {
    pose: xr::Posef,
    fov: xr::Fovf,
    pose_id: u64,
    /// The image's generation (Camera/ImagePoseIdentity.hpp), kept with its label; nothing reads it back yet.
    #[allow(dead_code)]
    generation: u64,
    /// The tracking origin it was captured in (main's 8255f379b): a reused eye from before a recenter is not this one.
    origin: u64,
    valid: bool,
}
const NO_POSE: xr::Posef = xr::Posef {
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
};
const NO_FOV: xr::Fovf = xr::Fovf {
    angle_left: 0.0,
    angle_right: 0.0,
    angle_up: 0.0,
    angle_down: 0.0,
};
const NO_LABEL: Label = Label {
    pose: NO_POSE,
    fov: NO_FOV,
    pose_id: 0,
    generation: 0,
    origin: 0,
    valid: false,
};

/// The newest captured frame (OpenXRManager::CapturedMonoFrame).
#[derive(Clone, Copy)]
struct Captured {
    /// One of the pool's resources (the pool holds the reference).
    texture: *mut c_void,
    width: u32,
    height: u32,
    format: u32,
    serial: u64,
    poses: [xr::Posef; 2],
    fovs: [xr::Fovf; 2],
    has_view: [bool; 2],
    pose_ids: [u64; 2],
    generations: [u64; 2],
    /// The frame generator's inputs per eye: one reference each (framegen_inputs.rs), or null.
    inputs: [*const Inputs; 2],
    /// When the image and its poses were captured, on the QPC clock (XrDiagNowMs).
    capture_ms: f64,
    /// Presented under a loading screen: never shown, however often submitted.
    dark: bool,
    /// The dark with the boot's notice, nothing else: one picture drawn for the main eye, submitted to both eyes with
    /// the turn it was drawn for.
    one_turn: bool,
    /// The eye this picture is (under AFW it turns over every frame).
    main_eye: i8,
    /// The tracking origin its poses belong to, and the capture fence's value the GPU has written it at (main's 0.2.4:
    /// a reader on the runtime's own queue takes only a finished capture).
    origin: u64,
    capture_fence: u64,
}
const NO_CAPTURED: Captured = Captured {
    texture: core::ptr::null_mut(),
    width: 0,
    height: 0,
    format: 0,
    serial: 0,
    poses: [NO_POSE; 2],
    fovs: [NO_FOV; 2],
    has_view: [false; 2],
    pose_ids: [0; 2],
    generations: [0; 2],
    inputs: [core::ptr::null(); 2],
    capture_ms: 0.0,
    dark: false,
    one_turn: false,
    main_eye: 1,
    origin: 0,
    capture_fence: 0,
};

/// The newest serial the XR submit sent (m_lastSubmittedSerial), for the capture's protection of the frame before the
/// newest (xr_cycle_end.rs stores it).
pub static LAST_SUBMITTED_SERIAL: AtomicU64 = AtomicU64::new(0);
/// The submit fence (ID3D12Fence, one reference; xr_submit.rs publishes it): a capture writing a slot a reader on the
/// runtime's queue retired but has not finished reading waits for it on the GPU.
pub static SUBMIT_FENCE: Mutex<Option<ID3D12Fence>> = Mutex::new(None);

/// CaptureFrontier.hpp's ProtectPrevious: a capture is published when queued, before its GPU fence completes. An
/// unpresented predecessor is kept until it completes or a newer completed capture can replace it -- otherwise a GPU
/// queue more than two frames deep makes BOTH selectable captures perpetually unfinished, even while the GPU advances.
pub fn protect_previous(previous: u64, presented: u64, origin: u64, current_origin: u64) -> bool {
    previous != 0 && previous > presented && origin != 0 && origin == current_origin
}
/// ReplacePrevious.
pub fn replace_previous(previous: u64, presented: u64, candidate_fence: u64, completed_fence: u64) -> bool {
    !protect_previous(previous, presented, 1, 1) || candidate_fence <= completed_fence
}

struct State {
    pool: [Option<ID3D12Resource>; 3],
    pool_slot: usize,
    captured: Captured,
    /// The frame published before the newest (m_monoPreviousFrame): a reader on the runtime's queue takes it while the
    /// newest is still on the GPU. It owns its frame generator's inputs (one reference each, moved from the newest when
    /// it became the previous frame), as main's copy of the shared pointers did.
    previous: Captured,
    /// The capture ring's fence (one reference), set by the capture's publication.
    capture_fence: Option<ID3D12Fence>,
    leases: Leases,
    vrcam: [Option<ID3D12Resource>; VRCAM_SLOTS],
    vrcam_serial: [u64; VRCAM_SLOTS],
    labels: [Label; VRCAM_SLOTS],
    vrcam_slot: usize,
    vrcam_size: (u32, u32, u32),
    /// The newest published serial with a second eye (for the log lines that report it); 0 none.
    vrcam_newest: u64,
    /// The tracking origin the capture being recorded belongs to (set when it takes its slot).
    capture_origin: u64,
    /// Reads released before their fence passed: the reader's reference, kept until it does.
    retired: Vec<(ID3D12Resource, u64)>,
}
// SAFETY: the raw pointers are the pool's own resources and the frame generator's reference-counted records, used
// under the state's lock only.
unsafe impl Send for State {}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| {
    Mutex::new(State {
        pool: [None, None, None],
        pool_slot: 0,
        captured: NO_CAPTURED,
        previous: NO_CAPTURED,
        capture_fence: None,
        leases: Leases::default(),
        vrcam: [None, None, None],
        vrcam_serial: [0; VRCAM_SLOTS],
        labels: [NO_LABEL; VRCAM_SLOTS],
        vrcam_slot: 0,
        vrcam_size: (0, 0, 0),
        vrcam_newest: 0,
        capture_origin: 0,
        retired: Vec::new(),
    })
});
fn state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}
fn raw(r: &Option<ID3D12Resource>) -> *mut c_void {
    r.as_ref().map_or(core::ptr::null_mut(), |r| r.as_raw())
}
/// A reference of one's own to `p` (AddRef), handed over raw.
fn add_ref(p: *mut c_void) -> *mut c_void {
    // SAFETY: a live resource of the pools.
    unsafe { ID3D12Resource::from_raw_borrowed(&p) }
        .map_or(core::ptr::null_mut(), |r| r.clone().into_raw())
}
fn log(text: &str) {
    cvr_host::log::write(text);
}
fn diagnostics() -> bool {
    // SAFETY: an export of the C++ half.
    unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_RuntimeDiagnostics) != 0 }
}

/// Three textures like `desc`, in `state`, named `name` with their index.
fn create_three(
    device: &ID3D12Device,
    desc: &D3D12_RESOURCE_DESC,
    state: D3D12_RESOURCE_STATES,
    name: &str,
) -> Result<[Option<ID3D12Resource>; 3], usize> {
    let heap = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_DEFAULT,
        ..Default::default()
    };
    let mut created: [Option<ID3D12Resource>; 3] = [None, None, None];
    for (i, slot) in created.iter_mut().enumerate() {
        let mut r: Option<ID3D12Resource> = None;
        // SAFETY: a live device; out-parameter.
        if unsafe {
            device.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, desc, state, None, &mut r)
        }
        .is_err()
            || r.is_none()
        {
            return Err(i);
        }
        if let Some(r) = &r {
            // SAFETY: a live resource.
            let _ = unsafe { r.SetName(&HSTRING::from(format!("{name}{i}").as_str())) };
        }
        *slot = r;
    }
    Ok(created)
}

/// FenceRetirement for the capture ring (main's 8255f379b): every write the capture queued is done -- or nothing was
/// queued yet. Polled only: an elapsed time never proves submitted GPU work done; a removed device (all ones) is not
/// ready either.
fn capture_writes_retired() -> bool {
    // SAFETY: cvr-xr's ring block; its fence a live object or null.
    let ring = unsafe { &*cvr_xr::capture_ring::cvr_xr_capture_ring() };
    if ring.fence_value == 0 {
        return true;
    }
    let fence = ring.fence;
    // SAFETY: as above.
    let Some(fence) = (unsafe { windows::Win32::Graphics::Direct3D12::ID3D12Fence::from_raw_borrowed(&fence) }) else { return false };
    // SAFETY: a plain query.
    let completed = unsafe { fence.GetCompletedValue() };
    completed != u64::MAX && completed >= ring.fence_value
}
/// CaptureFrontier.hpp ReusableEye: a second eye's picture shown for frame `requested` -- its own frame, or at most
/// `max_reuse` older, captured in the same tracking origin.
fn reusable_eye(source: u64, requested: u64, max_reuse: u32, origin: u64, current_origin: u64) -> bool {
    source != 0
        && source <= requested
        && origin != 0
        && origin == current_origin
        && (source == requested || (max_reuse != 0 && requested - source <= u64::from(max_reuse)))
}

/// THE SNAPSHOT POOL at the presented frame's size and format (EnsureMonoCaptureResource's pool): three textures of the
/// backbuffer's description that a blit may also draw into, resting in COPY_DEST; a new size or format drops the
/// published frame. False when the description is empty or a texture could not be made.
///
/// # Safety
/// `device` is an ID3D12Device, `desc` the backbuffer's (eye's) description.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_capture_ensure_pool(
    device: *mut c_void,
    desc: *const D3D12_RESOURCE_DESC,
) -> bool {
    // SAFETY: the caller's objects.
    let (Some(device), Some(desc)) = (
        unsafe { ID3D12Device::from_raw_borrowed(&device) },
        unsafe { desc.as_ref() },
    ) else {
        return false;
    };
    let (width, height, format) = (desc.Width as u32, desc.Height, desc.Format.0 as u32);
    if width == 0 || height == 0 || format == 0 {
        return false;
    }
    {
        let s = state();
        let c = &s.captured;
        if s.pool[0].is_some() && c.width == width && c.height == height && c.format == format {
            return true;
        }
        // Readers' leases protect reads, not the producer's queued writes: a resize lets those finish before the pool
        // is given up (the next capture asks again; the game's Present is not held).
        if s.pool[0].is_some() && !capture_writes_retired() {
            return false;
        }
    }
    let mut capture = *desc;
    capture.Flags |= D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
    // every slot starts and ends in COPY_SOURCE: the capture moves it to COPY_DEST for its write each time
    let created = match create_three(
        device,
        &capture,
        D3D12_RESOURCE_STATE_COPY_SOURCE,
        "OpenXR_mono_snapshot_color",
    ) {
        Ok(created) => created,
        Err(i) => {
            log(&format!(
                "OpenXRManager: Failed to create mono captured texture {i}\n"
            ));
            return false;
        }
    };
    let _retired = {
        let mut s = state();
        let retired = core::mem::replace(&mut s.pool, created);
        s.pool_slot = 0;
        let first = raw(&s.pool[0]);
        let c = &mut s.captured;
        c.texture = first;
        (c.width, c.height, c.format, c.serial) = (width, height, format, 0);
        c.has_view = [false; 2];
        retired
    };
    log(&format!(
        "OpenXRManager: Mono snapshot resource ready. size={width}x{height} format={format}\n"
    ));
    true
}

/// THE SECOND EYE'S POOL (EnsureVrcamEyeTexture): the eye swapchain's exact format and size, so the submit copies it
/// with the same plain CopyResource it uses for MAIN; resting in COPY_SOURCE between captures, a blit drawing into it.
/// A new size or format drops the slots and their serials.
///
/// # Safety
/// `device` is an ID3D12Device.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_capture_ensure_eye_pool(
    device: *mut c_void,
    width: u32,
    height: u32,
    format: i32,
) -> bool {
    // SAFETY: the caller's device.
    let Some(device) = (unsafe { ID3D12Device::from_raw_borrowed(&device) }) else {
        return false;
    };
    if width == 0 || height == 0 || format == DXGI_FORMAT_UNKNOWN.0 {
        return false;
    }
    {
        let s = state();
        if s.vrcam[0].is_some() && s.vrcam_size == (width, height, format as u32) {
            return true;
        }
        if s.vrcam[0].is_some() && !capture_writes_retired() {
            return false;
        }
    }
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
        Width: u64::from(width),
        Height: height,
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: DXGI_FORMAT(format),
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
        // The colour blit draws into it.
        Flags: D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
        ..Default::default()
    };
    match create_three(
        device,
        &desc,
        D3D12_RESOURCE_STATE_COPY_SOURCE,
        "OpenXR_vrcam_eye_slot",
    ) {
        Ok(created) => {
            let retired = {
                let mut s = state();
                let retired = core::mem::replace(&mut s.vrcam, created);
                s.vrcam_serial = [0; VRCAM_SLOTS];
                s.labels = [NO_LABEL; VRCAM_SLOTS];
                s.vrcam_slot = 0;
                s.vrcam_newest = 0;
                s.vrcam_size = (width, height, format as u32);
                retired
            };
            // the driver's releases outside the lock the consumers take theirs under
            drop(retired);
        }
        Err(i) => {
            log(&format!("OpenXRManager: Failed to create stereo capture texture eye=1 slot={i} {width}x{height} fmt={format}\n"));
            return false;
        }
    }
    log(&format!("OpenXRManager: stereo capture pool ready, {VRCAM_SLOTS} slots. eye=1 {width}x{height} fmt={format}\n"));
    true
}

/// THE NEXT SNAPSHOT SLOT: the pool's next buffer that is not the one just published and that no reader holds, its write
/// lease taken (`writes[0]`) and one reference of the caller's own; `previous` the published frame's serial. Null when
/// every buffer is in use.
///
/// # Safety
/// `previous` and `writes` (two entries) are valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_capture_begin_snapshot(
    previous: *mut u64,
    writes: *mut usize,
) -> *mut c_void {
    let mut s = state();
    let mut snapshot = core::ptr::null_mut();
    let mut dropped = [core::ptr::null(); 2];
    // The unpresented frontier is kept even when every other slot is leased: one skipped capture is better than
    // destroying the only image whose fence can become ready (main's 0.2.4; only with the runtime's own queue).
    let own = cvr_xr::runtime_queue::has_own();
    let origin = cvr_xr::tracking::origin_serial();
    let protect = own
        && protect_previous(s.previous.serial, LAST_SUBMITTED_SERIAL.load(Relaxed), s.previous.origin, origin);
    'passes: for pass in (if own { 0 } else { 1 })..2 {
    for step in 1..=3 {
        let slot = (s.pool_slot + step) % 3;
        let candidate = raw(&s.pool[slot]);
        if candidate == s.captured.texture {
            continue;
        }
        if (pass == 0 || protect) && candidate == s.previous.texture {
            continue;
        }
        if s.leases.begin_write(candidate as usize) {
            s.pool_slot = slot;
            snapshot = candidate;
            if candidate == s.previous.texture {
                dropped = core::mem::replace(&mut s.previous, NO_CAPTURED).inputs;
            }
            // the tracking origin this capture's poses belong to: a recenter before its publication cancels it
            s.capture_origin = cvr_xr::tracking::origin_serial();
            // SAFETY: the caller's write list.
            unsafe { *writes = candidate as usize };
            break 'passes;
        }
    }
    }
    // SAFETY: the caller's out-parameter.
    unsafe { *previous = s.captured.serial };
    let snapshot = add_ref(snapshot);
    drop(s);
    for inputs in dropped {
        // SAFETY: the previous frame's own references, released once.
        unsafe { cvr_framegen_release_inputs(inputs) };
    }
    snapshot
}

/// THE SECOND EYE'S SLOT a capture writes: the next one of the pool that no submit is reading and that the frame just
/// published does not pair with, its write lease taken (`writes[1]`). Null when every one is in use.
///
/// # Safety
/// `writes` holds two entries.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_capture_take_eye_slot(writes: *mut usize) -> *mut c_void {
    let mut s = state();
    let own = cvr_xr::runtime_queue::has_own();
    let protect = own
        && protect_previous(
            s.previous.serial,
            LAST_SUBMITTED_SERIAL.load(Relaxed),
            s.previous.origin,
            cvr_xr::tracking::origin_serial(),
        );
    for pass in (if own { 0 } else { 1 })..2 {
    for step in 1..=VRCAM_SLOTS {
        let slot = (s.vrcam_slot + step) % VRCAM_SLOTS;
        if s.vrcam_serial[slot] == s.captured.serial && s.captured.serial != 0 {
            continue;
        }
        if (pass == 0 || protect) && s.previous.serial != 0 && s.vrcam_serial[slot] == s.previous.serial {
            continue;
        }
        let candidate = raw(&s.vrcam[slot]);
        if s.leases.begin_write(candidate as usize) {
            // The queued write can succeed even when a recenter later cancels its publication: the slot's old serial
            // and label never outlive its changed pixels (main's 8255f379b).
            s.vrcam_serial[slot] = 0;
            s.labels[slot] = NO_LABEL;
            s.vrcam_slot = slot;
            // SAFETY: the caller's write list.
            unsafe { *writes.add(1) = candidate as usize };
            return candidate;
        }
    }
    }
    core::ptr::null_mut()
}

/// The second eye's slot the capture wrote last (borrowed), or null.
#[no_mangle]
pub extern "C" fn cvr_xr_capture_eye_slot() -> *mut c_void {
    let s = state();
    raw(&s.vrcam[s.vrcam_slot])
}

/// The capture's write leases ended (a capture that stopped, or one published), and the list cleared.
///
/// # Safety
/// `writes` holds two entries.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_capture_end_writes(writes: *mut usize) {
    // SAFETY: the caller's write list.
    let writes = unsafe { &mut *writes.cast::<[usize; 2]>() };
    if writes.iter().all(|&w| w == 0) {
        return;
    }
    let mut s = state();
    for w in writes.iter_mut() {
        if *w != 0 {
            s.leases.end_write(*w);
        }
        *w = 0;
    }
}

/// A captured frame for its publication.
#[repr(C)]
pub struct Publish {
    pub snapshot: *mut c_void,
    pub serial: u64,
    pub poses: [xr::Posef; 2],
    pub fovs: [xr::Fovf; 2],
    pub has_view: [bool; 2],
    pub dark: bool,
    pub one_turn: bool,
    pub main_eye: u32,
    pub pose_ids: [u64; 2],
    pub generations: [u64; 2],
    /// The frame generator's inputs: the caller's references, handed over.
    pub inputs: [*const Inputs; 2],
    /// The second eye's slot holds this frame's second eye.
    pub second_eye: bool,
}

const _: () = assert!(
    core::mem::size_of::<Publish>() == 168
        && core::mem::offset_of!(Publish, poses) == 16
        && core::mem::offset_of!(Publish, pose_ids) == 112
        && core::mem::offset_of!(Publish, second_eye) == 160
);

/// THE HAND-OFF: the buffer just filled published as the current frame -- image, poses, FOVs, labels, serial, the frame
/// generator's inputs -- in one locked step; the second eye's slot stamped with the SAME serial as the colour it belongs
/// to (the submit requires the match, so a frame without a second eye falls back to MAIN's in that eye instead of
/// pairing mismatched images), per slot, so a submit asks for the frame it carries; the write leases ended.
///
/// # Safety
/// `frame` and `writes` (two entries) are valid; the snapshot is the pool's.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_capture_publish(frame: *const Publish, writes: *mut usize) {
    // SAFETY: as the caller vouches.
    unsafe { publish_fenced(frame, writes, None, 0) }
}
/// cvr_xr_capture_publish with the capture's fence and the value the GPU has written it at (the Rust capture's own
/// call): the newest's predecessor kept as the previous frame for a reader on the runtime's queue.
///
/// # Safety
/// As cvr_xr_capture_publish.
pub unsafe fn publish_fenced(frame: *const Publish, writes: *mut usize, fence: Option<&ID3D12Fence>, value: u64) {
    // SAFETY: the caller's record.
    let Some(f) = (unsafe { frame.as_ref() }) else {
        return;
    };
    let mut released = [core::ptr::null(); 2];
    let mut dropped_previous: [*const Inputs; 2] = [core::ptr::null(); 2];
    {
        let mut s = state();
        // A recenter (a new tracking origin) or a runtime pose reset while the capture was recorded: its GPU work stays
        // the pool's, its stale poses are not published (main's 8255f379b).
        if s.capture_origin != cvr_xr::tracking::origin_serial() || cvr_xr::published::external_reset_pending() {
            // SAFETY: the caller's write list.
            let writes = unsafe { &mut *writes.cast::<[usize; 2]>() };
            for w in writes.iter_mut() {
                if *w != 0 {
                    s.leases.end_write(*w);
                }
                *w = 0;
            }
            drop(s);
            for input in f.inputs {
                // SAFETY: the caller's references, handed over.
                unsafe { crate::framegen_inputs::cvr_framegen_release_inputs(input) };
            }
            return;
        }
        let origin = s.capture_origin;
        if s.previous.origin != origin {
            dropped_previous = core::mem::replace(&mut s.previous, NO_CAPTURED).inputs;
        }
        if let Some(fence) = fence {
            if s.capture_fence.as_ref().map(|f| f.as_raw()) != Some(fence.as_raw()) {
                s.capture_fence = Some(fence.clone());
            }
        }
        if cvr_xr::runtime_queue::has_own()
            && s.captured.origin == origin
            && !s.captured.texture.is_null()
            && s.captured.texture != f.snapshot
            && s.captured.serial != 0
        {
            // SAFETY: the ring's live fence.
            let completed = s.capture_fence.as_ref().map_or(0, |f| unsafe { f.GetCompletedValue() });
            if replace_previous(s.previous.serial, LAST_SUBMITTED_SERIAL.load(Relaxed), s.captured.capture_fence, completed) {
                // the newest becomes the previous frame with its inputs (moved: the newest's are replaced below)
                let newest = s.captured;
                let old = core::mem::replace(&mut s.previous, newest);
                s.captured.inputs = [core::ptr::null(); 2];
                for (eye, inputs) in old.inputs.into_iter().enumerate() {
                    if !inputs.is_null() {
                        dropped_previous[eye] = inputs;
                    }
                }
            }
        }
        let c = &mut s.captured;
        c.texture = f.snapshot;
        c.serial = f.serial;
        c.origin = origin;
        c.capture_fence = value;
        if let Some(fence) = fence {
            // SAFETY: the ring's live fence.
            unsafe { cvr_xr::xr_trace::watch_capture(fence.as_raw(), f.serial, cvr_xr::tracking::now_ms(), value) };
        }
        c.capture_ms = cvr_xr::tracking::now_ms();
        c.dark = f.dark;
        c.one_turn = f.one_turn;
        c.main_eye = f.main_eye as i8;
        if diagnostics() {
            // SAFETY: the C++ half's counter.
            unsafe { CyberpunkVR_DebugCapOk.fetch_add(1, Relaxed) };
        }
        for eye in 0..2 {
            c.poses[eye] = f.poses[eye];
            c.pose_ids[eye] = f.pose_ids[eye];
            c.generations[eye] = f.generations[eye];
            released[eye] = core::mem::replace(&mut c.inputs[eye], f.inputs[eye]);
            c.fovs[eye] = f.fovs[eye];
            c.has_view[eye] = f.has_view[eye];
        }
        // SAFETY: the pool's resource.
        if let Some(t) = unsafe { ID3D12Resource::from_raw_borrowed(&c.texture) } {
            // SAFETY: a live resource.
            let _ = unsafe {
                t.SetName(&HSTRING::from(
                    format!("OpenXR_mono_snapshot_serial{}", f.serial).as_str(),
                ))
            };
        }
        s.vrcam_newest = if f.second_eye { f.serial } else { 0 };
        if f.second_eye {
            let slot = s.vrcam_slot;
            let eye = (f.main_eye as usize & 1) ^ 1;
            s.vrcam_serial[slot] = f.serial;
            s.labels[slot] = Label {
                pose: f.poses[eye],
                fov: f.fovs[eye],
                pose_id: f.pose_ids[eye],
                generation: f.generations[eye],
                origin: s.capture_origin,
                valid: true,
            };
        }
        // SAFETY: the caller's write list.
        let writes = unsafe { &mut *writes.cast::<[usize; 2]>() };
        for w in writes.iter_mut() {
            if *w != 0 {
                s.leases.end_write(*w);
            }
            *w = 0;
        }
    }
    for inputs in released.into_iter().chain(dropped_previous) {
        // SAFETY: the record's own reference, released once.
        unsafe { cvr_framegen_release_inputs(inputs) };
    }
}

/// What the submit takes of the newest captured frame.
#[repr(C)]
pub struct Taken {
    /// The image, with one reference and a read lease of the caller's own (ReleaseCapturedRead ends both).
    pub texture: *mut c_void,
    pub serial: u64,
    pub capture_ms: f64,
    pub dark: bool,
    pub main_eye: u32,
    pub poses: [xr::Posef; 2],
    pub fovs: [xr::Fovf; 2],
    pub has_view: [bool; 2],
    pub pose_ids: [u64; 2],
    /// The frame generator's inputs: one reference each of the caller's own (cvr_framegen_release_inputs), or null.
    pub inputs: [*const Inputs; 2],
    /// The second eye's picture of the same frame (or a reused one), reference and read lease of the caller's own;
    /// its own pose, FOV and pose id are in the eye's entries above.
    pub second: *mut c_void,
    pub second_reused: bool,
}

const _: () = assert!(
    core::mem::size_of::<Taken>() == 176
        && core::mem::offset_of!(Taken, poses) == 32
        && core::mem::offset_of!(Taken, pose_ids) == 128
        && core::mem::offset_of!(Taken, second) == 160
);

/// THE NEWEST CAPTURED FRAME FOR THE SUBMIT, when it has both views and no capture is writing it: its image pinned and
/// referenced, its poses, FOVs and labels, the frame generator's inputs (dropped from the frame while the generator
/// takes none), and -- with the stereo submit -- the second eye's picture of the same serial (or one at most
/// `reuse_max` frames older, which keeps its own label). The boot's notice gives both eyes the main eye's turn: the
/// compositor then holds it where it hangs through every display frame the game gives no new one for. False when there
/// is none.
///
/// # Safety
/// `out` is valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_capture_take(
    stereo: bool,
    reuse_max: u32,
    keep_inputs: bool,
    out: *mut Taken,
) -> bool {
    // SAFETY: as the caller vouches.
    unsafe { take(stereo, reuse_max, keep_inputs, Finished::default(), out) }
}

/// What a reader on the runtime's own queue asks of the take (main's 0.2.4 OpenXRFrameLoop.cpp).
#[derive(Default, Clone, Copy)]
pub struct Finished {
    /// Only a capture the game's queue has run: the newest when finished, else the frame before it when that is
    /// finished and not sent yet, else none (the last frame goes again).
    pub only: bool,
    /// Frame generation under SteamVR: one finished real frame in hand -- the older one not shown yet even when the
    /// newest is finished too (CyberpunkVR_SteamVrFrameBuffer).
    pub buffer: bool,
}

/// Cycles whose newest capture the game's queue had not run yet (the last frame sent again).
#[no_mangle]
pub static CyberpunkVR_XrCaptureNotDone: AtomicU64 = AtomicU64::new(0);
/// ...of which the frame before the newest, finished, went out instead.
#[no_mangle]
pub static CyberpunkVR_XrCapturePrevious: AtomicU64 = AtomicU64::new(0);
/// Cycles that took the older finished frame with the newest finished too (the frame buffer).
#[no_mangle]
pub static CyberpunkVR_XrCaptureBuffered: AtomicU64 = AtomicU64::new(0);

/// cvr_xr_capture_take with the runtime queue's rules for which frame.
///
/// # Safety
/// As cvr_xr_capture_take.
pub unsafe fn take(stereo: bool, reuse_max: u32, keep_inputs: bool, finished: Finished, out: *mut Taken) -> bool {
    // SAFETY: the caller's record.
    let Some(o) = (unsafe { out.as_mut() }) else {
        return false;
    };
    let mut s = state();
    let st = &mut *s;
    let origin = cvr_xr::tracking::origin_serial();
    let last = LAST_SUBMITTED_SERIAL.load(Relaxed);
    // SAFETY: the ring's live fence.
    let completed = st.capture_fence.as_ref().map(|f| unsafe { f.GetCompletedValue() });
    let mut use_previous = false;
    if finished.only {
        if let Some(completed) = completed {
            let newest = &st.captured;
            let prev = &st.previous;
            let previous_ready = !prev.texture.is_null()
                && prev.origin == origin
                && prev.serial != 0
                && prev.texture != newest.texture
                && prev.serial > last
                && completed >= prev.capture_fence;
            let running = newest.serial != 0 && newest.serial != last && completed < newest.capture_fence;
            if running {
                CyberpunkVR_XrCaptureNotDone.fetch_add(1, Relaxed);
                if !previous_ready {
                    return false;
                }
                use_previous = true;
                CyberpunkVR_XrCapturePrevious.fetch_add(1, Relaxed);
            } else if finished.buffer && previous_ready {
                use_previous = true;
                CyberpunkVR_XrCaptureBuffered.fetch_add(1, Relaxed);
            }
        }
    }
    let c = if use_previous { &mut st.previous } else { &mut st.captured };
    if c.texture.is_null()
        || c.serial == 0
        || c.origin != origin && c.origin != 0
        || !c.has_view[0]
        || !c.has_view[1]
        || !st.leases.pin(c.texture as usize)
    {
        return false;
    }
    o.texture = add_ref(c.texture);
    o.serial = c.serial;
    o.capture_ms = c.capture_ms;
    o.dark = c.dark;
    o.main_eye = u32::from(c.main_eye != 0);
    for eye in 0..2 {
        if !keep_inputs {
            let dropped = core::mem::replace(&mut c.inputs[eye], core::ptr::null());
            // SAFETY: the frame's own reference, released once.
            unsafe { cvr_framegen_release_inputs(dropped) };
        }
        o.poses[eye] = c.poses[eye];
        o.pose_ids[eye] = c.pose_ids[eye];
        o.fovs[eye] = c.fovs[eye];
        o.has_view[eye] = c.has_view[eye];
        // SAFETY: the frame's reference, held.
        o.inputs[eye] = unsafe { cvr_framegen_retain_inputs(c.inputs[eye]) };
    }
    o.second = core::ptr::null_mut();
    o.second_reused = false;
    let main = o.main_eye as usize;
    let one_turn = c.one_turn;
    if stereo {
        let serial = c.serial;
        let eye = main ^ 1;
        let mut best: Option<usize> = None;
        let mut best_serial = 0u64;
        let origin = cvr_xr::tracking::origin_serial();
        for i in 0..VRCAM_SLOTS {
            let source_serial = st.vrcam_serial[i];
            let source = raw(&st.vrcam[i]);
            if source.is_null()
                || !st.labels[i].valid
                || !reusable_eye(source_serial, serial, reuse_max, st.labels[i].origin, origin)
                || st.leases.writing(source as usize)
            {
                continue;
            }
            if source_serial > best_serial {
                best = Some(i);
                best_serial = source_serial;
            }
        }
        if let Some(i) = best {
            let resource = raw(&st.vrcam[i]);
            if st.leases.pin(resource as usize) {
                o.second = add_ref(resource);
                let label = st.labels[i];
                o.poses[eye] = label.pose;
                o.fovs[eye] = label.fov;
                o.pose_ids[eye] = label.pose_id;
                o.second_reused = best_serial != serial;
            }
        }
    }
    if one_turn {
        let turn = o.poses[main].orientation;
        for pose in o.poses.iter_mut() {
            pose.orientation = turn;
        }
    }
    true
}

/// A read lease on a pool's resource (the frame generator's hold on the pictures it keeps); whether it was taken.
#[no_mangle]
pub extern "C" fn cvr_xr_capture_pin(resource: *mut c_void) -> bool {
    state().leases.pin(resource as usize)
}

/// A captured read ended (ReleaseCapturedRead): the lease dropped, and the reader's reference released once the GPU
/// passed `fence` (`completed` is the submit fence's value now) -- held among the retired reads until then; the retired
/// reads it has passed released with it.
///
/// # Safety
/// `resource` carries one reference of the caller's own, handed over.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_capture_release_read(
    resource: *mut c_void,
    fence: u64,
    completed: u64,
) {
    if resource.is_null() {
        return;
    }
    // SAFETY: the caller's reference, taken over.
    let owned = unsafe { ID3D12Resource::from_raw(resource) };
    let mut release = Vec::new();
    {
        let mut s = state();
        s.leases.unpin(resource as usize);
        if fence != 0 && completed < fence {
            s.retired.push((owned, fence));
        } else {
            release.push(owned);
        }
        let mut kept = Vec::with_capacity(s.retired.len());
        for (r, f) in s.retired.drain(..) {
            if f <= completed {
                release.push(r);
            } else {
                kept.push((r, f));
            }
        }
        s.retired = kept;
    }
    drop(release);
}

/// The submit fence's value the retired reads of `writes` end at (0 none): a capture about to write those resources on
/// the game's queue waits for it there while the runtime's own queue may still be reading them (main's 0.2.4).
pub fn retired_reads_of(writes: &[usize; 2]) -> u64 {
    let s = state();
    let mut pending = 0u64;
    for (r, f) in &s.retired {
        let raw = r.as_raw() as usize;
        if writes.iter().any(|&w| w != 0 && w == raw) && *f > pending {
            pending = *f;
        }
    }
    pending
}

/// The newest captured frame's serial (0 none).
#[no_mangle]
pub extern "C" fn cvr_xr_capture_serial() -> u64 {
    state().captured.serial
}
/// The newest captured frame is the boot's notice in the dark (one turn for both eyes).
#[no_mangle]
pub extern "C" fn cvr_xr_capture_one_turn() -> bool {
    state().captured.one_turn
}
/// The newest published serial with a second eye (0 none).
#[no_mangle]
pub extern "C" fn cvr_xr_capture_second_eye_serial() -> u64 {
    state().vrcam_newest
}

/// The published frame forgotten (the submit switched on or off): nothing is submitted until the next capture.
#[no_mangle]
pub extern "C" fn cvr_xr_capture_forget() {
    let mut s = state();
    s.captured.serial = 0;
    s.captured.has_view = [false; 2];
}

/// The pools released (the manager's shutdown, before its device): the snapshots, the second eye's slots and their
/// serials, the menu window and the blits; the published frame emptied.
#[no_mangle]
pub extern "C" fn cvr_xr_capture_shutdown() {
    let (pool, vrcam, previous) = {
        let mut s = state();
        let previous = core::mem::replace(&mut s.previous, NO_CAPTURED).inputs;
        let pool = core::mem::replace(&mut s.pool, [None, None, None]);
        let vrcam = core::mem::replace(&mut s.vrcam, [None, None, None]);
        s.vrcam_serial = [0; VRCAM_SLOTS];
        s.vrcam_slot = 0;
        s.vrcam_size = (0, 0, 0);
        s.vrcam_newest = 0;
        let c = &mut s.captured;
        c.texture = core::ptr::null_mut();
        (c.width, c.height, c.format, c.serial) = (0, 0, 0, 0);
        c.has_view = [false; 2];
        (pool, vrcam, previous)
    };
    drop(pool);
    drop(vrcam);
    for inputs in previous {
        // SAFETY: the previous frame's own references, released once.
        unsafe { cvr_framegen_release_inputs(inputs) };
    }
    release_resources();
}

// ---- the capture's own resources -----------------------------------------------------------------------------------------

/// The engine stereo menu window (the whole backbuffer, resting in COPY_DEST), the blit that squeezes it into one eye's
/// snapshot, the blit the second eye's picture is drawn with (rust/crates/cvr-gpu/src/color_blit.rs), and the CAS
/// sharpening the real captures go through (rust/crates/cvr-gpu/src/sharpen_pass.rs).
struct Resources {
    menu_window: Option<ID3D12Resource>,
    menu_blit: *mut cvr_gpu::color_blit::Blit,
    eye_blit: *mut cvr_gpu::color_blit::Blit,
    sharpen: *mut cvr_gpu::sharpen_pass::Pass,
}
// SAFETY: the blits are the capture's own, used under the lock or by the capture thread alone.
unsafe impl Send for Resources {}
static RESOURCES: Mutex<Resources> = Mutex::new(Resources {
    menu_window: None,
    menu_blit: core::ptr::null_mut(),
    eye_blit: core::ptr::null_mut(),
    sharpen: core::ptr::null_mut(),
});
fn resources() -> MutexGuard<'static, Resources> {
    RESOURCES.lock().unwrap_or_else(|e| e.into_inner())
}

/// THE MENU WINDOW under engine stereo (EnsureMenuWindow): the whole window of `width` x `height` in `format`, made
/// anew when the backbuffer changes, and its blit ready for one eye's half. The window (borrowed) and the blit, or None.
pub fn menu_window(
    device: &ID3D12Device,
    width: u64,
    height: u32,
    format: DXGI_FORMAT,
) -> Option<(*mut c_void, *const c_void)> {
    if width < 2 || height == 0 {
        return None;
    }
    let mut r = resources();
    // SAFETY: a live resource.
    let have = r.menu_window.as_ref().map(|w| unsafe { w.GetDesc() });
    if have.is_none_or(|d| d.Width != width || d.Height != height || d.Format != format) {
        r.menu_window = None;
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Width: width,
            Height: height,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
            ..Default::default()
        };
        let heap = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_DEFAULT,
            ..Default::default()
        };
        let mut made: Option<ID3D12Resource> = None;
        // SAFETY: a live device; out-parameter.
        let ok = unsafe {
            device.CreateCommittedResource(
                &heap,
                D3D12_HEAP_FLAG_NONE,
                &desc,
                D3D12_RESOURCE_STATE_COPY_DEST,
                None,
                &mut made,
            )
        }
        .is_ok();
        let Some(window) = made.filter(|_| ok) else {
            log(&format!("OpenXRManager: Failed to create the engine stereo menu window {width}x{height} fmt={}\n", format.0 as u32));
            return None;
        };
        // SAFETY: a live resource.
        let _ = unsafe { window.SetName(&HSTRING::from("OpenXR_engine_stereo_menu_window")) };
        log(&format!(
            "OpenXRManager: engine stereo menu window ready {width}x{height} fmt={}\n",
            format.0 as u32
        ));
        r.menu_window = Some(window);
    }
    if r.menu_blit.is_null() {
        r.menu_blit = cvr_gpu::color_blit::cvr_gpu_color_blit_new();
    }
    // SAFETY: the blit just made or kept, and a live device.
    let ready = unsafe {
        cvr_gpu::color_blit::cvr_gpu_color_blit_ensure(
            r.menu_blit,
            device.as_raw(),
            format.0,
            (width / 2) as u32,
            height,
        )
    };
    ready.then(|| (raw(&r.menu_window), r.menu_blit as *const c_void))
}

/// The second eye's blit (made on first use).
pub fn eye_blit() -> *const c_void {
    let mut r = resources();
    if r.eye_blit.is_null() {
        r.eye_blit = cvr_gpu::color_blit::cvr_gpu_color_blit_new();
    }
    r.eye_blit as *const c_void
}

/// The CAS pass (made on first use).
pub fn sharpen_pass() -> *const c_void {
    let mut r = resources();
    if r.sharpen.is_null() {
        r.sharpen = cvr_gpu::sharpen_pass::cvr_gpu_sharpen_pass_new();
    }
    r.sharpen as *const c_void
}

fn release_resources() {
    let mut guard = resources();
    let r = &mut *guard;
    for blit in [&mut r.eye_blit, &mut r.menu_blit] {
        if !blit.is_null() {
            // SAFETY: the capture's blit, released once.
            unsafe {
                cvr_gpu::color_blit::cvr_gpu_color_blit_shutdown(*blit);
                cvr_gpu::color_blit::cvr_gpu_color_blit_free(*blit);
            }
            *blit = core::ptr::null_mut();
        }
    }
    r.menu_window = None;
    if !r.sharpen.is_null() {
        // SAFETY: the capture's pass, released once.
        unsafe { cvr_gpu::sharpen_pass::cvr_gpu_sharpen_pass_free(r.sharpen) };
        r.sharpen = core::ptr::null_mut();
    }
}

/// THE CAPTURE'S RING (EnsureMonoCaptureResource): three allocators and their lists (closed), a fence and its event, each
/// made when its slot is empty. False when one could not be made (the slots made so far are kept).
///
/// # Safety
/// `device` is an ID3D12Device; `allocators` and `lists` hold three slots, `fence` and `event` one each.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_capture_ensure_ring(
    device: *mut c_void,
    allocators: *mut *mut c_void,
    lists: *mut *mut c_void,
    fence: *mut *mut c_void,
    event: *mut *mut c_void,
) -> bool {
    use windows::Win32::Graphics::Direct3D12::{
        ID3D12CommandAllocator, ID3D12Fence, ID3D12GraphicsCommandList, ID3D12PipelineState,
        D3D12_COMMAND_LIST_TYPE_DIRECT, D3D12_FENCE_FLAG_NONE,
    };
    // SAFETY (whole body): the caller's device and slots.
    unsafe {
        let Some(device) = ID3D12Device::from_raw_borrowed(&device) else {
            return false;
        };
        if (*allocators).is_null() {
            for i in 0..3 {
                match device.CreateCommandAllocator::<ID3D12CommandAllocator>(
                    D3D12_COMMAND_LIST_TYPE_DIRECT,
                ) {
                    Ok(a) => {
                        let _ = a.SetName(&HSTRING::from("OpenXR_capture_allocator"));
                        *allocators.add(i) = a.into_raw();
                    }
                    Err(_) => {
                        log(&format!(
                            "OpenXRManager: Failed to create mono capture command allocator {i}\n"
                        ));
                        return false;
                    }
                }
            }
        }
        if (*lists).is_null() {
            for i in 0..3 {
                let allocator = *allocators.add(i);
                let Some(a) = ID3D12CommandAllocator::from_raw_borrowed(&allocator) else {
                    return false;
                };
                match device.CreateCommandList::<_, _, ID3D12GraphicsCommandList>(
                    0,
                    D3D12_COMMAND_LIST_TYPE_DIRECT,
                    a,
                    None::<&ID3D12PipelineState>,
                ) {
                    Ok(list) => {
                        let _ = list.SetName(&HSTRING::from("OpenXR_capture_command_list"));
                        let _ = list.Close();
                        *lists.add(i) = list.into_raw();
                    }
                    Err(_) => {
                        log(&format!(
                            "OpenXRManager: Failed to create mono capture command list {i}\n"
                        ));
                        return false;
                    }
                }
            }
        }
        if (*fence).is_null() {
            match device.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) {
                Ok(f) => {
                    let _ = f.SetName(&HSTRING::from("OpenXR_capture_fence"));
                    *fence = f.into_raw();
                }
                Err(_) => {
                    log("OpenXRManager: Failed to create mono capture fence\n");
                    return false;
                }
            }
        }
        if (*event).is_null() {
            let e = windows_sys::Win32::System::Threading::CreateEventA(
                core::ptr::null(),
                0,
                0,
                core::ptr::null(),
            );
            if e.is_null() {
                log("OpenXRManager: Failed to create mono capture fence event\n");
                return false;
            }
            *event = e;
        }
        true
    }
}
