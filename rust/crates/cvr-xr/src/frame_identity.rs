//! One identity per frame (OpenXRManager's frame-identity members, owned here; the C++ methods forward to these):
//!
//!   - the frame slots: every frame's per-eye poses and FOVs and the head, located together and keyed by the serial of
//!     the present that will show it (PublishFrameSlot / GetFrameSlot), plus the cycle's flat head latch;
//!   - the display-time fit: displayTime = k*serial + b, a rolling least-squares line (FitAddDisplayTime, ...);
//!   - the rendered-frame queues, MAIN's and VRCAM's: the XR sample each view was actually rendered with, read back
//!     from the engine and popped at capture (Push/PopRenderedFramePose, Push/PopVrcamRenderedFramePose);
//!   - the render-head ring and the frame's own pose (PushRenderHeadPose, GetRenderHeadPoseForPresent,
//!     GetFramePoseForSerial);
//!   - the per-eye render poses the camera hook stores, back in raw LOCAL space, and their 256-deep queue by the
//!     engine's sequence (StoreRenderEyePose), under the render-pose lock both halves take.
#![cfg_attr(test, allow(dead_code, unused_imports))]
use cvr_body::xr::{multiply_quat, rotate_vector, Quat, Vec3};
use openxr_sys as xr;
use std::sync::atomic::Ordering;
use std::sync::Mutex;

use crate::frame::FRAME;
use crate::srw::Guarded;
use crate::tracking::{origin_serial, HeadPose, HEAD};

const ZERO_POSE: xr::Posef =
    xr::Posef { orientation: xr::Quaternionf { x: 0.0, y: 0.0, z: 0.0, w: 0.0 }, position: xr::Vector3f { x: 0.0, y: 0.0, z: 0.0 } };
const ZERO_FOV: xr::Fovf = xr::Fovf { angle_left: 0.0, angle_right: 0.0, angle_up: 0.0, angle_down: 0.0 };

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

// ---- the frame slots

/// OpenXRManager::XrFrameSlot.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FrameSlot {
    /// The present that will show the frame built with this.
    pub serial: u64,
    /// What the locate was aimed at.
    pub display_time: i64,
    /// Per-eye pose, LOCAL space, as located for this frame.
    pub view_pose: [xr::Posef; 2],
    pub view_fov: [xr::Fovf; 2],
    /// The view space's origin in LOCAL space, same instant.
    pub head_pose_local: xr::Posef,
    pub valid: bool,
}
const _: () = assert!(core::mem::offset_of!(FrameSlot, view_fov) == 72 && core::mem::offset_of!(FrameSlot, head_pose_local) == 104);
const _: () = assert!(core::mem::offset_of!(FrameSlot, valid) == 132 && core::mem::size_of::<FrameSlot>() == 136);
const NO_SLOT: FrameSlot =
    FrameSlot { serial: 0, display_time: 0, view_pose: [ZERO_POSE; 2], view_fov: [ZERO_FOV; 2], head_pose_local: ZERO_POSE, valid: false };
const FRAME_SLOTS: usize = 8;
static SLOTS: Mutex<[FrameSlot; FRAME_SLOTS]> = Mutex::new([NO_SLOT; FRAME_SLOTS]);

/// A frame's slot, written whole.
pub fn publish_slot(slots: &mut [FrameSlot; FRAME_SLOTS], serial: u64, display_time: i64, views: &[xr::View], head: &xr::Posef) {
    let s = &mut slots[(serial % FRAME_SLOTS as u64) as usize];
    s.serial = serial;
    s.display_time = display_time;
    for eye in 0..2 {
        s.view_pose[eye] = views[eye].pose;
        s.view_fov[eye] = views[eye].fov;
    }
    s.head_pose_local = *head;
    s.valid = true;
}

/// The exact slot if it exists, else the newest one older than asked for (an interval without a locate of its own
/// was rendered with the last pose located); never a newer one. The bool says which.
pub fn find_slot(slots: &[FrameSlot; FRAME_SLOTS], serial: u64) -> Option<(FrameSlot, bool)> {
    let s = &slots[(serial % FRAME_SLOTS as u64) as usize];
    if s.valid && s.serial == serial {
        return Some((*s, true));
    }
    let mut best: Option<&FrameSlot> = None;
    for c in slots.iter() {
        if !c.valid || c.serial > serial {
            continue;
        }
        if best.is_none_or(|b| c.serial > b.serial) {
            best = Some(c);
        }
    }
    best.map(|b| (*b, false))
}

/// OpenXRManager::PublishFrameSlot: the slot, and the cycle's head as the flat latch the camera hook reads.
///
/// # Safety
/// `views` holds `count` views; `head` is a pose.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_publish_frame_slot(serial: u64, display_time: i64, views: *const xr::View, count: u32, head: *const xr::Posef) {
    if views.is_null() || count < 2 || head.is_null() {
        return;
    }
    // SAFETY: the caller's views and pose.
    let (views, head) = unsafe { (core::slice::from_raw_parts(views, 2), &*head) };
    let mut slots = lock(&SLOTS);
    publish_slot(&mut slots, serial, display_time, views, head);
    let f = &FRAME;
    f.cycle_ori[0].store(head.orientation.x);
    f.cycle_ori[1].store(head.orientation.y);
    f.cycle_ori[2].store(head.orientation.z);
    f.cycle_ori[3].store(head.orientation.w);
    f.cycle_pos[0].store(head.position.x);
    f.cycle_pos[1].store(head.position.y);
    f.cycle_pos[2].store(head.position.z);
    f.cycle_valid.store(true, Ordering::Release);
}

/// The slot for `serial` (exact, or the newest older one) and whether it is exact.
pub fn slot_for(serial: u64) -> Option<(FrameSlot, bool)> {
    find_slot(&lock(&SLOTS), serial)
}

/// OpenXRManager::GetFrameSlot.
///
/// # Safety
/// `out` is an XrFrameSlot; `exact` is null or a bool.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_get_frame_slot(serial: u64, out: *mut FrameSlot, exact: *mut bool) -> bool {
    if out.is_null() {
        return false;
    }
    let Some((slot, is_exact)) = slot_for(serial) else { return false };
    // SAFETY: the caller's slot and flag.
    unsafe {
        *out = slot;
        if !exact.is_null() {
            *exact = is_exact;
        }
    }
    true
}

// ---- the display-time fit

/// The rolling least-squares line displayTime = ay + k * (serial - ax), re-anchored per window (raw magnitudes would
/// cancel catastrophically: serials reach six figures, XrTime ~1e18 ns).
#[derive(Clone, Copy, Default)]
pub struct Fit {
    anchored: bool,
    wx0: u64,
    wy0: i64,
    n: f64,
    sx: f64,
    sy: f64,
    sxx: f64,
    sxy: f64,
    ax: u64,
    ay: i64,
    k: f64,
    valid: bool,
}
impl Fit {
    pub const NEW: Fit = Fit { anchored: false, wx0: 0, wy0: 0, n: 0.0, sx: 0.0, sy: 0.0, sxx: 0.0, sxy: 0.0, ax: 0, ay: 0, k: 0.0, valid: false };

    pub fn add(&mut self, serial: u64, display_time: i64) {
        if !self.anchored {
            self.wx0 = serial;
            self.wy0 = display_time;
            self.anchored = true;
        }
        let x = serial.wrapping_sub(self.wx0) as i64 as f64;
        let y = display_time.wrapping_sub(self.wy0) as f64;
        self.n += 1.0;
        self.sx += x;
        self.sy += y;
        self.sxx += x * x;
        self.sxy += x * y;
        if self.n >= 8.0 {
            let den = self.n * self.sxx - self.sx * self.sx;
            if den > 1e-6 {
                let k = (self.n * self.sxy - self.sx * self.sy) / den;
                let b = (self.sy - k * self.sx) / self.n;
                // ns of display time per present: outside 1..200 ms is a broken sample set, not a frame rate.
                if k > 1.0e6 && k < 2.0e8 {
                    self.k = k;
                    self.ax = self.wx0;
                    self.ay = self.wy0.wrapping_add(b as i64);
                    self.valid = true;
                }
            }
        }
        if self.n >= 240.0 {
            // A new window so the fit tracks drift; the last line stays.
            self.anchored = false;
            self.n = 0.0;
            self.sx = 0.0;
            self.sy = 0.0;
            self.sxx = 0.0;
            self.sxy = 0.0;
        }
    }

    pub fn predict(&self, serial: u64) -> Option<i64> {
        if !self.valid {
            return None;
        }
        let dx = serial.wrapping_sub(self.ax) as i64 as f64;
        Some(self.ay.wrapping_add((self.k * dx) as i64))
    }

    /// A new origin or frame generation: the line and the window both start again.
    pub fn reset(&mut self) {
        self.anchored = false;
        self.valid = false;
        self.n = 0.0;
        self.sx = 0.0;
        self.sy = 0.0;
        self.sxx = 0.0;
        self.sxy = 0.0;
    }
}
static FIT: Mutex<Fit> = Mutex::new(Fit::NEW);

/// OpenXRManager::FitAddDisplayTime.
#[no_mangle]
pub extern "C" fn cvr_xr_fit_add_display_time(serial: u64, display_time: i64) {
    lock(&FIT).add(serial, display_time);
}
/// OpenXRManager::FitPredictDisplayTime.
///
/// # Safety
/// `out` is an XrTime.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_fit_predict_display_time(serial: u64, out: *mut i64) -> bool {
    if out.is_null() {
        return false;
    }
    let Some(t) = lock(&FIT).predict(serial) else { return false };
    // SAFETY: the caller's time.
    unsafe { *out = t };
    true
}
/// OpenXRManager::FitSlopeNs: ns of display time per present, 0 without a line.
#[no_mangle]
pub extern "C" fn cvr_xr_fit_slope_ns() -> f64 {
    let f = lock(&FIT);
    if f.valid {
        f.k
    } else {
        0.0
    }
}
/// The frame loop's reset on a new tracking origin or frame-generation mode.
#[no_mangle]
pub extern "C" fn cvr_xr_fit_reset() {
    lock(&FIT).reset();
}

// ---- the rendered-frame queues (MAIN's and VRCAM's)

const RENDERED_Q: usize = 8;
/// A view's read-back queue: the XR sample each rendered frame was built with, popped at capture.
pub struct RenderedQueue {
    q: [HeadPose; RENDERED_Q],
    head: u64,
    tail: u64,
}
impl RenderedQueue {
    pub const NEW: RenderedQueue = RenderedQueue { q: [HeadPose::EMPTY; RENDERED_Q], head: 0, tail: 0 };

    /// Bounded: a graph run without a present drops the oldest rather than drifting out of step for good.
    pub fn push(&mut self, pose: HeadPose) {
        self.q[(self.tail & (RENDERED_Q as u64 - 1)) as usize] = pose;
        self.tail += 1;
        if self.tail - self.head > 4 {
            self.head = self.tail - 4;
        }
    }
    pub fn pop(&mut self) -> Option<HeadPose> {
        if self.head == self.tail {
            return None;
        }
        let p = self.q[(self.head & (RENDERED_Q as u64 - 1)) as usize];
        self.head += 1;
        Some(p)
    }
    pub fn depth(&self) -> u32 {
        (self.tail - self.head) as u32
    }
}
static RENDERED: [Mutex<RenderedQueue>; 2] = [Mutex::new(RenderedQueue::NEW), Mutex::new(RenderedQueue::NEW)];

/// Push(Vrcam)RenderedFramePose: `vrcam` picks the second eye's queue.
///
/// # Safety
/// `pose` is an OpenXRHeadPose.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_push_rendered_frame_pose(vrcam: bool, pose: *const HeadPose) {
    // SAFETY: the caller's pose.
    if let Some(p) = unsafe { pose.as_ref() } {
        lock(&RENDERED[vrcam as usize]).push(*p);
    }
}
/// Pop(Vrcam)RenderedFramePose: the oldest entry, true when it is valid and of the current tracking origin.
///
/// # Safety
/// `out` is an OpenXRHeadPose.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_pop_rendered_frame_pose(vrcam: bool, out: *mut HeadPose) -> bool {
    if out.is_null() {
        return false;
    }
    let Some((p, usable)) = pop_rendered(vrcam) else { return false };
    // SAFETY: the caller's pose.
    unsafe { *out = p };
    usable
}
/// The oldest rendered-frame pose of a view's queue (whatever it holds) and whether it is usable: valid and of the
/// current tracking origin.
pub fn pop_rendered(vrcam: bool) -> Option<(HeadPose, bool)> {
    let p = lock(&RENDERED[vrcam as usize]).pop()?;
    Some((p, p.valid && p.origin_serial == origin_serial()))
}
/// (Vrcam)RenderedFrameQueueDepth.
#[no_mangle]
pub extern "C" fn cvr_xr_rendered_frame_queue_depth(vrcam: bool) -> u32 {
    lock(&RENDERED[vrcam as usize]).depth()
}

// ---- the render-head ring and the frame's own pose

/// The poses the camera injection used, stamped with the present interval they were injected in, and the last one
/// bound to the frame the next present will show.
pub struct RenderRing {
    ring: [HeadPose; 16],
    stamp: [u64; 16],
    head: u64,
    valid: bool,
    frame_pose: HeadPose,
    frame_pose_serial: u64,
}
impl RenderRing {
    pub const NEW: RenderRing =
        RenderRing { ring: [HeadPose::EMPTY; 16], stamp: [0; 16], head: 0, valid: false, frame_pose: HeadPose::EMPTY, frame_pose_serial: 0 };

    /// `present` is the interval being built in: the frame it belongs to is shown by the next present, and the last
    /// write before that present wins.
    pub fn push(&mut self, pose: HeadPose, present: u64) {
        let slot = (self.head & 15) as usize;
        self.ring[slot] = pose;
        self.stamp[slot] = present;
        self.head += 1;
        self.valid = true;
        self.frame_pose = pose;
        self.frame_pose_serial = if pose.valid { present + 1 } else { 0 };
    }
    /// The newest pose stamped at or before `present - 1 - lag` (lag in intervals, 0..8).
    pub fn for_present(&self, present: u64, lag: i32) -> Option<HeadPose> {
        if !self.valid {
            return None;
        }
        let want = lag.clamp(0, 8) as u64 + 1;
        if present < want {
            return None;
        }
        let cutoff = present - want;
        let n = self.head.min(16);
        for i in 1..=n {
            let slot = (self.head.wrapping_sub(i) & 15) as usize;
            if self.stamp[slot] <= cutoff {
                return Some(self.ring[slot]);
            }
        }
        None
    }
    /// The pose bound to `serial`, if that is the frame it was bound to.
    pub fn for_serial(&self, serial: u64) -> Option<HeadPose> {
        (self.frame_pose_serial != 0 && self.frame_pose_serial == serial).then_some(self.frame_pose)
    }
}
static RING: Mutex<RenderRing> = Mutex::new(RenderRing::NEW);

/// OpenXRManager::PushRenderHeadPose (`present` = m_presentCount now).
///
/// # Safety
/// `pose` is an OpenXRHeadPose.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_push_render_head_pose(pose: *const HeadPose, present: u64) {
    // SAFETY: the caller's pose.
    if let Some(p) = unsafe { pose.as_ref() } {
        lock(&RING).push(*p, present);
    }
}
/// GetFramePoseForSerial, then GetRenderHeadPoseForPresent with `lag`: the first usable answer (valid, current origin).
pub fn bound_or_ring_pose(present: u64, lag: i32) -> Option<HeadPose> {
    let usable = |p: &HeadPose| p.valid && p.origin_serial == origin_serial();
    let ring = lock(&RING);
    if let Some(p) = ring.for_serial(present).filter(usable) {
        return Some(p);
    }
    ring.for_present(present, lag).filter(usable)
}

/// OpenXRManager::GetRenderHeadPoseForPresent.
///
/// # Safety
/// `out` is an OpenXRHeadPose.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_render_head_pose_for_present(present: u64, lag: i32, out: *mut HeadPose) -> bool {
    if out.is_null() {
        return false;
    }
    let Some(p) = lock(&RING).for_present(present, lag) else { return false };
    // SAFETY: the caller's pose.
    unsafe { *out = p };
    p.valid && p.origin_serial == origin_serial()
}
/// OpenXRManager::GetFramePoseForSerial.
///
/// # Safety
/// `out` is an OpenXRHeadPose.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_frame_pose_for_serial(serial: u64, out: *mut HeadPose) -> bool {
    if out.is_null() {
        return false;
    }
    let Some(p) = lock(&RING).for_serial(serial) else { return false };
    // SAFETY: the caller's pose.
    unsafe { *out = p };
    p.valid && p.origin_serial == origin_serial()
}

// ---- the per-eye render poses (under the render-pose lock)

/// What StoreRenderEyePose keeps: each eye's last render pose and MAIN's by the engine's sequence, in raw LOCAL space.
pub struct RenderEyes {
    pose: [xr::Posef; 2],
    valid: [bool; 2],
    queue: [xr::Posef; 256],
    queue_frame: [u32; 256],
}
static RENDER_EYES: Guarded<RenderEyes> =
    Guarded::new(RenderEyes { pose: [ZERO_POSE; 2], valid: [false; 2], queue: [ZERO_POSE; 256], queue_frame: [0; 256] });

/// A base-relative pose put back in raw LOCAL space, the space the projection layer is submitted in: raw = base ∘ rel.
pub fn unrecenter(base: Option<&crate::hands::Pose>, rel_ori: Quat, rel_pos: Vec3) -> xr::Posef {
    let (o, p) = match base {
        Some(b) => {
            let r = rotate_vector(b.orientation, rel_pos);
            (multiply_quat(b.orientation, rel_ori), Vec3 { x: b.position.x + r.x, y: b.position.y + r.y, z: b.position.z + r.z })
        }
        None => (rel_ori, rel_pos),
    };
    xr::Posef { orientation: xr::Quaternionf { x: o.x, y: o.y, z: o.z, w: o.w }, position: xr::Vector3f { x: p.x, y: p.y, z: p.z } }
}

/// OpenXRManager::StoreRenderEyePose: the pose the camera hook rendered an eye with, back in raw LOCAL space.
///
/// # Safety
/// `pose` is an OpenXRHeadPose.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_store_render_eye_pose(eye: i32, pose: *const HeadPose, seq: u32) {
    // SAFETY: the caller's pose.
    let Some(p) = (unsafe { pose.as_ref() }) else { return };
    if !(0..=1).contains(&eye) || !p.valid {
        return;
    }
    let rel_ori = Quat { x: p.ori[0], y: p.ori[1], z: p.ori[2], w: p.ori[3] };
    let rel_pos = Vec3 { x: p.pos[0], y: p.pos[1], z: p.pos[2] };
    let guard = HEAD.render_pose_lock.lock();
    let base = if *HEAD.base_pose_set.get(&guard) { Some(*HEAD.base_pose.get(&guard)) } else { None };
    let raw = unrecenter(base.as_ref(), rel_ori, rel_pos);
    let eyes = RENDER_EYES.get(&guard);
    if eye == 0 && seq > 0 {
        let idx = (seq % 256) as usize;
        eyes.queue[idx] = raw;
        eyes.queue_frame[idx] = seq;
    }
    eyes.pose[eye as usize] = raw;
    eyes.valid[eye as usize] = true;
}

/// The head pose the frame the engine rendered as `seq` was drawn with (MAIN's queue), else MAIN's last.
///
/// # Safety
/// `out` is an XrPosef.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_render_head_pose_for_seq(seq: u32, out: *mut xr::Posef) -> bool {
    if out.is_null() {
        return false;
    }
    let Some(p) = render_head_for_seq(seq) else { return false };
    // SAFETY: the caller's pose.
    unsafe { *out = p };
    true
}
/// cvr_xr_render_head_pose_for_seq's body.
pub fn render_head_for_seq(seq: u32) -> Option<xr::Posef> {
    let guard = HEAD.render_pose_lock.lock();
    let eyes = RENDER_EYES.get(&guard);
    let idx = (seq % 256) as usize;
    if seq > 0 && eyes.queue_frame[idx] == seq {
        Some(eyes.queue[idx])
    } else if eyes.valid[0] {
        Some(eyes.pose[0])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn views(x: f32) -> [xr::View; 2] {
        let mut v = [xr::View { ty: xr::StructureType::VIEW, next: core::ptr::null_mut(), pose: ZERO_POSE, fov: ZERO_FOV }; 2];
        v[0].pose.position.x = x;
        v[1].pose.position.x = x + 0.064;
        v
    }

    #[test]
    fn slots_exact_else_newest_older_never_newer() {
        let mut s = [NO_SLOT; FRAME_SLOTS];
        assert!(find_slot(&s, 5).is_none());
        for serial in [10u64, 11, 13] {
            publish_slot(&mut s, serial, serial as i64 * 100, &views(serial as f32), &ZERO_POSE);
        }
        let (a, exact) = find_slot(&s, 11).unwrap();
        assert!(exact && a.serial == 11 && a.view_pose[0].position.x == 11.0);
        let (b, exact) = find_slot(&s, 12).unwrap();
        assert!(!exact && b.serial == 11);
        let (c, exact) = find_slot(&s, 20).unwrap();
        assert!(!exact && c.serial == 13);
        assert!(find_slot(&s, 9).is_none());
        // A slot overwritten by serial + 8 no longer answers its old serial exactly.
        publish_slot(&mut s, 18, 0, &views(18.0), &ZERO_POSE);
        assert!(find_slot(&s, 10).is_none());
        assert!(find_slot(&s, 18).unwrap().1);
    }

    #[test]
    fn fit_recovers_the_line_and_rejects_nonsense() {
        let mut f = Fit::NEW;
        let period = 13_888_889i64;
        let t0 = 1_700_000_000_000_000_000i64;
        for i in 0..7u64 {
            f.add(1000 + i, t0 + i as i64 * period);
        }
        assert!(f.predict(2000).is_none());
        f.add(1007, t0 + 7 * period);
        let p = f.predict(1100).unwrap();
        assert!((p - (t0 + 100 * period)).abs() < 1000, "{p}");
        // 0.5 ms per present is no frame rate: the line stays.
        let mut g = Fit::NEW;
        for i in 0..10u64 {
            g.add(i, i as i64 * 500_000);
        }
        assert!(g.predict(3).is_none());
        // The window restarts at 240 samples and keeps the line.
        for i in 8..240u64 {
            f.add(1000 + i, t0 + i as i64 * period);
        }
        assert!(f.predict(1300).is_some() && !f.anchored);
        f.reset();
        assert!(f.predict(1300).is_none());
    }

    #[test]
    fn rendered_queue_keeps_the_newest_four() {
        let mut q = RenderedQueue::NEW;
        assert!(q.pop().is_none());
        for i in 0..6 {
            let mut p = HeadPose::EMPTY;
            p.pos[0] = i as f32;
            q.push(p);
        }
        assert_eq!(q.depth(), 4);
        assert_eq!(q.pop().unwrap().pos[0], 2.0);
        assert_eq!(q.depth(), 3);
    }

    #[test]
    fn render_ring_picks_by_interval_and_binds_the_next_present() {
        let mut r = RenderRing::NEW;
        assert!(r.for_present(10, 0).is_none());
        for present in [5u64, 6, 6, 8] {
            let mut p = HeadPose::EMPTY;
            p.valid = true;
            p.pos[0] = present as f32;
            r.push(p, present);
        }
        assert_eq!(r.for_present(9, 0).unwrap().pos[0], 8.0);
        assert_eq!(r.for_present(8, 0).unwrap().pos[0], 6.0);
        assert_eq!(r.for_present(8, 1).unwrap().pos[0], 6.0);
        assert_eq!(r.for_present(7, 1).unwrap().pos[0], 5.0);
        assert!(r.for_present(5, 0).is_none());
        assert_eq!(r.for_serial(9).unwrap().pos[0], 8.0);
        assert!(r.for_serial(8).is_none());
        // An invalid pose unbinds the frame.
        r.push(HeadPose::EMPTY, 9);
        assert!(r.for_serial(10).is_none());
    }

    #[test]
    fn unrecenter_applies_the_base() {
        let rel_ori = Quat { x: 0.0, y: 0.0, z: 0.0, w: 1.0 };
        let rel_pos = Vec3 { x: 1.0, y: 0.0, z: 0.0 };
        let raw = unrecenter(None, rel_ori, rel_pos);
        assert_eq!(raw.position.x, 1.0);
        // A base turned 90 degrees about Y, at (0, 1, 0).
        let h = core::f32::consts::FRAC_1_SQRT_2;
        let base = crate::hands::Pose { orientation: Quat { x: 0.0, y: h, z: 0.0, w: h }, position: Vec3 { x: 0.0, y: 1.0, z: 0.0 } };
        let raw = unrecenter(Some(&base), rel_ori, rel_pos);
        assert!((raw.position.x).abs() < 1e-6 && (raw.position.y - 1.0).abs() < 1e-6 && (raw.position.z + 1.0).abs() < 1e-6);
        assert!((raw.orientation.y - h).abs() < 1e-7);
    }
}
