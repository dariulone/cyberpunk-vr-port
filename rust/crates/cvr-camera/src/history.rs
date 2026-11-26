//! Rendered pose history (formerly include/Camera/RenderedPoseHistory.hpp): the completed per-eye camera transform
//! beside the XR label it was rendered with. The quaternion alone cannot identify a translation-only frame; a
//! collision that repeats the whole transform with different labels is rejected as ambiguous rather than attaching the
//! newest label to old pixels.
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderedPoseMatch {
    Missing,
    Exact,
    Ambiguous,
}

/// The pose label the history carries (OpenXRHeadPose's fields it reads).
pub trait LabelPose: Copy + Default {
    fn valid(&self) -> bool;
    fn position(&self) -> [f32; 3];
    fn orientation(&self) -> [f32; 4];
    fn origin_serial(&self) -> u64;
}

/// Engine matrix->quaternion round trips change component bits (live VRCAM: 0.0004-0.0014 degrees): the same
/// rotation within 0.002 degrees, either sign.
pub fn same_rendered_rotation(a: &[f32; 4], b: &[f32; 4]) -> bool {
    let (mut aa, mut bb, mut dot) = (0.0f64, 0.0f64, 0.0f64);
    for k in 0..4 {
        aa += a[k] as f64 * a[k] as f64;
        bb += b[k] as f64 * b[k] as f64;
        dot += a[k] as f64 * b[k] as f64;
    }
    if !(aa + bb + dot).is_finite() || aa < 0.5 || aa > 1.5 || bb < 0.5 || bb > 1.5 {
        return false;
    }
    let ai = 1.0 / aa.sqrt();
    let bi = (if dot < 0.0 { -1.0 } else { 1.0 }) / bb.sqrt();
    let mut error = 0.0f64;
    for k in 0..4 {
        let d = a[k] as f64 * ai - b[k] as f64 * bi;
        error += d * d;
    }
    error <= 3.0461742e-10
}

/// CCT positions pass through world floats before the camera's fixed-point write: one float ULP per axis, the vector
/// bounded to 0.01..0.25 mm.
fn label_position_tolerance(position: &[i32; 3]) -> f64 {
    let mut squared = 0.0f64;
    for k in 0..3 {
        let world = (position[k] as f32 / 131072.0).abs();
        let ulp = (world.next_up() - world) as f64;
        squared += ulp * ulp;
    }
    squared.sqrt().clamp(0.00001, 0.00025)
}

fn same_label<P: LabelPose>(a: &P, b: &P, position: &[i32; 3]) -> bool {
    let tolerance = label_position_tolerance(position);
    let (pa, pb) = (a.position(), b.position());
    let x = pa[0] as f64 - pb[0] as f64;
    let y = pa[1] as f64 - pb[1] as f64;
    let z = pa[2] as f64 - pb[2] as f64;
    if x * x + y * y + z * z > tolerance * tolerance {
        return false;
    }
    same_rendered_rotation(&a.orientation(), &b.orientation())
}

#[derive(Clone, Copy, Debug, Default)]
struct Entry<P> {
    quat: [f32; 4],
    position: [i32; 3],
    pose: P,
    id: u64,
    view: u32,
}

pub struct RenderedPoseHistory<P: LabelPose, const N: usize = 64> {
    state: Mutex<(Vec<Entry<P>>, u64)>,
}

impl<P: LabelPose, const N: usize> Default for RenderedPoseHistory<P, N> {
    fn default() -> Self {
        RenderedPoseHistory { state: Mutex::new((vec![Entry::default(); N], 0)) }
    }
}

pub struct Found<P> {
    pub pose: P,
    pub age: u32,
    pub ties: u32,
}

impl<P: LabelPose, const N: usize> RenderedPoseHistory<P, N> {
    pub fn push(&self, view: u32, q: &[f32; 4], position: &[i32; 3], pose: &P) {
        let p = pose.position();
        let o = pose.orientation();
        if !pose.valid() || !p.iter().chain(o.iter()).all(|v| v.is_finite()) {
            return;
        }
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let head = s.1;
        let entry = &mut s.0[(head % N as u64) as usize];
        entry.quat = *q;
        entry.position = *position;
        entry.view = view;
        entry.pose = *pose;
        entry.id = head;
        s.1 = head + 1;
    }

    /// (match, the pose when exact, its age in pushes, the number of matching entries).
    pub fn find(&self, view: u32, q: &[f32; 4], position: &[i32; 3], origin: u64) -> (RenderedPoseMatch, Option<Found<P>>) {
        let s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let head = s.1;
        let count = head.min(N as u64);
        let mut best: Option<&Entry<P>> = None;
        let mut matches = 0u32;
        let mut ambiguous = false;
        for i in 1..=count {
            let entry = &s.0[((head - i) % N as u64) as usize];
            if entry.view != view || entry.pose.origin_serial() != origin {
                continue;
            }
            if entry.position != *position || !same_rendered_rotation(&entry.quat, q) {
                continue;
            }
            matches += 1;
            match best {
                None => best = Some(entry),
                Some(b) if !same_label(&b.pose, &entry.pose, position) => ambiguous = true,
                _ => {}
            }
        }
        if ambiguous {
            return (RenderedPoseMatch::Ambiguous, Some(Found { pose: P::default(), age: 0, ties: matches }));
        }
        match best {
            None => (RenderedPoseMatch::Missing, Some(Found { pose: P::default(), age: 0, ties: matches })),
            Some(b) => (RenderedPoseMatch::Exact, Some(Found { pose: b.pose, age: (head - 1 - b.id) as u32, ties: matches })),
        }
    }
}
