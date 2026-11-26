//! include/Natives/ClimbSupport.hpp: what the hand's grip closed on, from the contacts of one static snapshot -- a top
//! (a railing's, a ledge's), an edge the fingers curl over, or neither (a wall's face is not climbed).
use crate::handspace::{length_squared, Vector};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Feature {
    None,
    Top,
    Edge,
}
/// A contact: its point and its unit normal (the minimum translation's direction), the shape it is on -- an index local
/// to one physics-thread query of one static snapshot -- and whether it is close.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub point: Vector,
    pub normal: Vector,
    pub shape: i32,
    pub close: bool,
}
pub const MAX_HITS: usize = 24;
/// A small collision seam allowed between adjacent pieces.
pub const SEAM_RADIUS: f32 = 0.012;
/// A small pad just inside the side the hand touches.
pub const CAP_RADIUS: f32 = 0.016;
pub fn dot(a: Vector, b: Vector) -> f32 {
    a.x * b.x + a.y * b.y + a.z * b.z
}
/// Classify: `shares_seam(a, b, seam)` tests a small ball at the intersection of the two measured planes against BOTH
/// shapes (nearby bounds alone do not establish a common corner); `has_cap(shape, pad)` a pad on that shape's top.
pub fn classify(hits: &[Hit], mut shares_seam: impl FnMut(i32, i32, Vector) -> bool, mut has_cap: impl FnMut(i32, Vector) -> bool) -> Feature {
    for hi in hits {
        if hi.normal.z < 0.45 {
            continue;
        }
        if hits.iter().any(|hj| hj.close && hi.shape == hj.shape) {
            return Feature::Top;
        }
    }
    for (i, a) in hits.iter().enumerate() {
        for b in &hits[i + 1..] {
            if !a.close && !b.close {
                continue;
            }
            let d = b.point - a.point;
            let c = dot(a.normal, b.normal);
            if c >= 0.77 || length_squared(d) > 0.18 * 0.18 {
                continue;
            }
            let (ab, ba) = (dot(d, a.normal), -dot(d, b.normal));
            // Exterior: one point wraps behind the other plane, neither is clearly in front.
            if (ab >= -0.015 && ba >= -0.015) || ab > 0.012 || ba > 0.012 {
                continue;
            }
            if a.shape == b.shape {
                return Feature::Edge; // the opposite sides of a narrow rail too
            }
            if c < -0.95 {
                continue; // facing sheets have no well-defined local seam
            }
            // The midpoint projected onto both planes (the normals are unit MTD directions).
            let (u, v) = (-0.5 * ab, -0.5 * ba);
            let inv = 1.0 / (1.0 - c * c);
            let seam = a.point + d * 0.5 + a.normal * ((u - c * v) * inv) + b.normal * ((v - c * u) * inv);
            if length_squared(seam - a.point) <= 0.12 * 0.12 && length_squared(seam - b.point) <= 0.12 * 0.12 && shares_seam(a.shape, b.shape, seam) {
                return Feature::Edge;
            }
        }
    }
    // A finger over a rail can curl in the drawn pose while its physical capsule is still on the near face: just behind
    // the highest close side contact, on that same shape only. Two pad heights cover a lip up to ~4.5 cm; the scene is
    // never scanned.
    let mut highest: Option<&Hit> = None;
    for h in hits {
        if h.close && h.normal.z.abs() < 0.35 && highest.is_none_or(|t| h.point.z > t.point.z) {
            highest = Some(h);
        }
    }
    if let Some(h) = highest {
        let inside = h.point - h.normal * 0.009;
        for rise in [0.020f32, 0.045] {
            if has_cap(h.shape, inside + Vector::new(0.0, 0.0, rise)) {
                return Feature::Top;
            }
        }
    }
    Feature::None
}

#[cfg(test)]
mod tests {
    use super::*;
    // main's tools/physics_frame_tests/climb_support_test.cpp
    #[derive(Clone, Copy)]
    struct Box3 {
        lo: Vector,
        hi: Vector,
    }
    impl Box3 {
        fn top_under_pad(&self, p: Vector) -> bool {
            p.x >= self.lo.x && p.x <= self.hi.x && p.y >= self.lo.y && p.y <= self.hi.y && p.z >= self.hi.z && p.z - self.hi.z < CAP_RADIUS
        }
        fn touches(&self, p: Vector) -> bool {
            let c = Vector::new(p.x.clamp(self.lo.x, self.hi.x), p.y.clamp(self.lo.y, self.hi.y), p.z.clamp(self.lo.z, self.hi.z));
            length_squared(c - p) < SEAM_RADIUS * SEAM_RADIUS
        }
    }
    fn v(x: f32, y: f32, z: f32) -> Vector {
        Vector::new(x, y, z)
    }
    fn hit(p: Vector, n: Vector, shape: i32, close: bool) -> Hit {
        Hit { point: p, normal: n, shape, close }
    }
    #[test]
    fn tops_rails_and_connected_corners_hold_and_walls_do_not() {
        let mut boxes = [Box3 { lo: v(9.99, 19.0, 0.0), hi: v(10.0, 20.0, 2.0) }, Box3 { lo: v(9.0, 19.99, 0.0), hi: v(10.0, 20.0, 2.0) }];
        let mut seams = 0;
        let mut classify_with = |boxes: &[Box3; 2], hits: &[Hit]| {
            classify(
                hits,
                |a, b, seam| {
                    seams += 1;
                    boxes[a as usize].touches(seam) && boxes[b as usize].touches(seam)
                },
                |i, pad| boxes[i as usize].top_under_pad(pad),
            )
        };
        let face = hit(v(10.0, 19.95, 1.0), v(1.0, 0.0, 0.0), 0, true);
        let mut side = hit(v(9.97, 20.0, 1.04), v(0.0, 1.0, 0.0), 1, false);
        assert!(classify_with(&boxes, &[face, side]) == Feature::Edge && classify_with(&boxes, &[side, face]) == Feature::Edge, "split exterior corner in either order");
        boxes[1].hi.x -= 0.006;
        assert_eq!(classify_with(&boxes, &[face, side]), Feature::Edge, "small cooked seam still supported");
        boxes[1].hi.x -= 0.020;
        assert_eq!(classify_with(&boxes, &[face, side]), Feature::None, "disconnected side behind wall cannot supply a corner");
        side.shape = 0;
        assert_eq!(classify_with(&boxes, &[face, side]), Feature::Edge, "one-piece exterior corner");
        side.point = v(10.05, 20.0, 1.0);
        assert_eq!(classify_with(&boxes, &[face, side]), Feature::None, "one-sided behind-plane evidence rejected");
        let mut inside = face;
        inside.point.y = 20.04;
        assert_eq!(classify_with(&boxes, &[inside, side]), Feature::None, "inside corner");
        let mut back = hit(v(9.95, 19.95, 1.0), v(-1.0, 0.0, 0.0), 0, true);
        assert_eq!(classify_with(&boxes, &[face, back]), Feature::Edge, "opposite faces of a narrow rail");
        back.shape = 1;
        assert_eq!(classify_with(&boxes, &[face, back]), Feature::None, "opposite faces on disconnected sheets");
        let mut top = hit(v(10.0, 19.95, 1.02), v(0.0, 0.0, 1.0), 1, true);
        assert_eq!(classify_with(&boxes, &[top]), Feature::Top, "ledge or rail top without finger contacts");
        top.close = false;
        assert_eq!(classify_with(&boxes, &[face, top]), Feature::None, "separate near top cannot borrow wall contact");
        top.shape = 0;
        assert_eq!(classify_with(&boxes, &[face, top]), Feature::Top, "rail side within contact range with top within reach");
        assert!(classify_with(&boxes, &[face]) == Feature::None && classify_with(&boxes, &[]) == Feature::None, "no previous hold carried into later smooth or empty query");
        let mut parallel = face;
        parallel.point.y += 0.07;
        assert_eq!(classify_with(&boxes, &[face, parallel]), Feature::None, "many points on a smooth wall");
        boxes[0].hi.z = 1.033;
        assert_eq!(classify_with(&boxes, &[face]), Feature::Top, "side-only touch just below the rail's top with fingers over the lip");
        boxes[0].hi.z = 1.08;
        assert_eq!(classify_with(&boxes, &[face]), Feature::None, "distant top not within hooking reach");
        boxes[1] = boxes[0];
        boxes[1].hi.z = 1.032;
        assert_eq!(classify_with(&boxes, &[face]), Feature::None, "neighboring rail cannot prove a cap on the wall");
        drop(classify_with);
        assert!(seams >= 4, "two-piece fixtures did not actually query seams");
    }
}
