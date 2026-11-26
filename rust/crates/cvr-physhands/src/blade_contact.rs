//! include/Anim/BladeContactState.hpp and src/Anim/BladeContactState.cpp (the contact's measures; the rail helpers are
//! blade.rs): how deep a blade segment is in a capsule and at what angle, the segment swept between two poses through a
//! capsule, whether an edge is usable on a shaft, and which of a hit representation's capsules a name and its user data
//! select.
const EPSILON: f32 = 1.0e-6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum Material {
    Flesh = 0,
    Cyberware = 1,
    Metal = 2,
    Armor = 3,
    Unknown = 4,
}
impl Material {
    pub fn from_i32(v: i32) -> Material {
        match v {
            0 => Material::Flesh,
            1 => Material::Cyberware,
            2 => Material::Metal,
            3 => Material::Armor,
            _ => Material::Unknown,
        }
    }
    fn resistance(self) -> f32 {
        match self {
            Material::Flesh => 1.0,
            Material::Cyberware => 2.6,
            Material::Metal => 4.5,
            Material::Armor => 6.0,
            Material::Unknown => 3.0,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct ContactInput {
    pub entry: [f32; 3],
    pub exit: [f32; 3],
    pub blade_start: [f32; 3],
    pub blade_end: [f32; 3],
    pub motion_start: [f32; 3],
    pub motion_end: [f32; 3],
    pub surface_normal: [f32; 3],
    pub edge_direction: [f32; 3],
    pub material: Material,
}
impl Default for ContactInput {
    fn default() -> ContactInput {
        ContactInput {
            entry: [0.0; 3],
            exit: [0.0; 3],
            blade_start: [0.0; 3],
            blade_end: [0.0; 3],
            motion_start: [0.0; 3],
            motion_end: [0.0; 3],
            surface_normal: [0.0; 3],
            edge_direction: [0.0; 3],
            material: Material::Unknown,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ContactMetrics {
    pub penetration_depth: f32,
    pub center_fraction: f32,
    pub incidence: f32,
    pub edge_alignment: f32,
    pub resistance: f32,
    pub valid: bool,
}
fn dot(a: &[f32; 3], b: &[f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn length(v: &[f32; 3]) -> f32 {
    dot(v, v).sqrt()
}
fn normalized(v: &[f32; 3]) -> Option<[f32; 3]> {
    let l = length(v);
    if !l.is_finite() || l <= EPSILON {
        return None;
    }
    Some([v[0] / l, v[1] / l, v[2] / l])
}
fn project_edge(shaft: &[f32; 3], input: &[f32; 3]) -> Option<[f32; 3]> {
    let axial = dot(input, shaft);
    normalized(&[input[0] - axial * shaft[0], input[1] - axial * shaft[1], input[2] - axial * shaft[2]])
}
/// HasContactEdge: an edge with a direction across the shaft from a to b.
pub fn has_contact_edge(a: &[f32; 3], b: &[f32; 3], edge: &[f32; 3]) -> bool {
    normalized(&[b[0] - a[0], b[1] - a[1], b[2] - a[2]]).is_some_and(|shaft| project_edge(&shaft, edge).is_some())
}
/// EvaluateContact: the blade's depth between its entry and exit (both on its line), where along it, at what incidence
/// to the surface, how well its motion runs along the edge, and the material's resistance for that.
pub fn evaluate_contact(input: &ContactInput) -> ContactMetrics {
    let mut result = ContactMetrics::default();
    let blade = [input.blade_end[0] - input.blade_start[0], input.blade_end[1] - input.blade_start[1], input.blade_end[2] - input.blade_start[2]];
    let reach = length(&blade);
    if reach > 1.5 {
        return result;
    }
    let Some(blade) = normalized(&blade) else { return result };
    let Some(normal) = normalized(&input.surface_normal) else { return result };
    let Some(edge) = project_edge(&blade, &input.edge_direction) else { return result };
    let project = |point: &[f32; 3]| -> Option<f32> {
        let mut d = [point[0] - input.blade_start[0], point[1] - input.blade_start[1], point[2] - input.blade_start[2]];
        let distance = dot(&d, &blade);
        for k in 0..3 {
            d[k] -= distance * blade[k];
        }
        (distance.is_finite() && dot(&d, &d) <= 0.000025).then_some(distance)
    };
    let (Some(enter_at), Some(exit_at)) = (project(&input.entry), project(&input.exit)) else { return result };
    if exit_at < enter_at {
        return result;
    }
    let depth = reach.min(exit_at) - enter_at.max(0.0);
    if !(depth > EPSILON) {
        return result;
    }
    let motion = [input.motion_end[0] - input.motion_start[0], input.motion_end[1] - input.motion_start[1], input.motion_end[2] - input.motion_start[2]];
    if !dot(&motion, &motion).is_finite() {
        return result;
    }
    let moving = normalized(&motion);
    result.penetration_depth = depth;
    result.center_fraction = (enter_at.max(0.0) + depth * 0.5) / reach;
    result.incidence = dot(&blade, &normal).abs().clamp(0.0, 1.0);
    result.edge_alignment = moving.map_or(0.0, |m| dot(&m, &edge).clamp(0.0, 1.0));
    let depth_factor = 1.0 + (depth / 0.12).clamp(0.0, 4.0);
    let angle_factor = 0.35 + 0.65 * result.incidence;
    result.resistance = input.material.resistance() * depth_factor * angle_factor;
    result.valid = result.resistance.is_finite();
    result
}
/// EvaluateCapsule: the blade's line through a capsule (centre, axis, radius, half its height), measured as a contact.
pub fn evaluate_capsule(input: &ContactInput, center: &[f32; 3], axis: &[f32; 3], radius: f32, half_height: f32) -> ContactMetrics {
    if !(radius > 0.0 && radius <= 1.0 && (0.0..=1.0).contains(&half_height)) {
        return ContactMetrics::default();
    }
    let mut direction = [0.0f32; 3];
    let mut origin = [0.0f32; 3];
    for i in 0..3 {
        direction[i] = input.blade_end[i] - input.blade_start[i];
        origin[i] = input.blade_start[i] - center[i];
    }
    let (Some(direction), Some(up)) = (normalized(&direction), normalized(axis)) else { return ContactMetrics::default() };
    if !dot(&origin, &origin).is_finite() {
        return ContactMetrics::default();
    }
    let axial_speed = dot(&direction, &up);
    let axial_origin = dot(&origin, &up);
    let (mut first, mut last) = (f32::INFINITY, f32::NEG_INFINITY);
    let mut intersect = |shift: f32, cap: i32| {
        let mut local = [0.0f32; 3];
        let mut slope = [0.0f32; 3];
        for i in 0..3 {
            local[i] = origin[i] - up[i] * if cap != 0 { shift } else { axial_origin };
            slope[i] = direction[i] - if cap != 0 { 0.0 } else { axial_speed * up[i] };
        }
        let squared = dot(&slope, &slope);
        let linear = dot(&slope, &local);
        let discriminant = linear * linear - squared * (dot(&local, &local) - radius * radius);
        if squared < 1e-10 || !(discriminant >= 0.0) {
            return;
        }
        let root = discriminant.sqrt();
        for dist in [(-linear - root) / squared, (-linear + root) / squared] {
            let along = axial_origin + dist * axial_speed;
            let outside = if cap != 0 { (cap as f32) * (along - shift) < -1e-5 } else { along.abs() > half_height + 1e-5 };
            if outside {
                continue;
            }
            first = first.min(dist);
            last = last.max(dist);
        }
    };
    intersect(0.0, 0);
    intersect(-half_height, -1);
    intersect(half_height, 1);
    if !(first < last) {
        return ContactMetrics::default();
    }
    let mut measured = *input;
    let closest = (axial_origin + first * axial_speed).clamp(-half_height, half_height);
    for i in 0..3 {
        measured.entry[i] = input.blade_start[i] + first * direction[i];
        measured.exit[i] = input.blade_start[i] + last * direction[i];
        measured.surface_normal[i] = origin[i] + first * direction[i] - closest * up[i];
    }
    evaluate_contact(&measured)
}
/// A swept segment's first contact: when along the sweep, where along the blade, how deep.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SweepContact {
    pub time: f32,
    pub fraction: f32,
    pub depth: f32,
}
/// SweepCapsule: the blade from (old_a, old_b) to (a, b), stepped by a quarter of the capsule's radius (at least 5 mm,
/// at most 256 steps), and its first pose inside the capsule. Nothing for a travel over 4 m or a capsule out of reach.
#[allow(clippy::too_many_arguments)]
pub fn sweep_capsule(old_a: &[f32; 3], old_b: &[f32; 3], a: &[f32; 3], b: &[f32; 3], center: &[f32; 3], up: &[f32; 3], radius: f32, half_height: f32) -> SweepContact {
    let da = [a[0] - old_a[0], a[1] - old_a[1], a[2] - old_a[2]];
    let db = [b[0] - old_b[0], b[1] - old_b[1], b[2] - old_b[2]];
    let travel = length(&da).max(length(&db));
    if !(travel <= 4.0 && radius > 0.0 && radius <= 1.0) {
        return SweepContact::default();
    }
    for i in 0..3 {
        let bound = half_height + radius;
        let lo = old_a[i].min(old_b[i]).min(a[i]).min(b[i]);
        let hi = old_a[i].max(old_b[i]).max(a[i]).max(b[i]);
        if lo > center[i] + bound || hi < center[i] - bound {
            return SweepContact::default();
        }
    }
    let steps = ((travel / (radius * 0.25).max(0.005)).ceil() as i32).clamp(1, 256);
    let mut input = ContactInput::default();
    for step in 0..=steps {
        let t = step as f32 / steps as f32;
        let mut direction = [0.0f32; 3];
        for i in 0..3 {
            input.blade_start[i] = old_a[i] + t * da[i];
            input.blade_end[i] = old_b[i] + t * db[i];
            direction[i] = input.blade_end[i] - input.blade_start[i];
        }
        // Only the finite interval is needed: a non-axial basis for the contact metric's edge projection, including
        // vertical weapons.
        input.edge_direction[0] = if direction[0].abs() < direction[2].abs() { 1.0 } else { 0.0 };
        input.edge_direction[1] = 0.0;
        input.edge_direction[2] = 1.0 - input.edge_direction[0];
        let hit = evaluate_capsule(&input, center, up, radius, half_height);
        if hit.valid {
            return SweepContact { time: t, fraction: hit.center_fraction, depth: hit.penetration_depth };
        }
    }
    SweepContact::default()
}
/// SegmentInBox: the part of the segment start..end inside the box centred at the origin with half extents `half`
/// (its own frame), as fractions along the segment; None when it misses.
pub fn segment_in_box(start: &[f32; 3], end: &[f32; 3], half: &[f32; 3]) -> Option<(f32, f32)> {
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for i in 0..3 {
        let d = end[i] - start[i];
        if d.abs() < 1e-7 {
            if start[i].abs() > half[i] {
                return None;
            }
            continue;
        }
        let (t1, t2) = ((-half[i] - start[i]) / d, (half[i] - start[i]) / d);
        lo = lo.max(t1.min(t2));
        hi = hi.min(t1.max(t2));
        if lo > hi {
            return None;
        }
    }
    Some((lo, hi))
}
/// SweepBox: the blade from (old_a, old_b) to (a, b) in the box's own frame (centred, axis-aligned, half extents `half`
/// -- a mech's hit shapes are boxes), stepped by a quarter of its smallest half extent (at least 5 mm, at most 256
/// steps), and its first pose inside it: when along the sweep, the middle of the blade's part inside, and how deep that
/// point is under the nearest face. Nothing for a travel over 4 m or a box beyond reach.
pub fn sweep_box(old_a: &[f32; 3], old_b: &[f32; 3], a: &[f32; 3], b: &[f32; 3], half: &[f32; 3]) -> SweepContact {
    let da = [a[0] - old_a[0], a[1] - old_a[1], a[2] - old_a[2]];
    let db = [b[0] - old_b[0], b[1] - old_b[1], b[2] - old_b[2]];
    let travel = length(&da).max(length(&db));
    if !(travel <= 4.0 && half.iter().all(|h| *h > 0.0 && *h <= 4.0)) {
        return SweepContact::default();
    }
    for i in 0..3 {
        let lo = old_a[i].min(old_b[i]).min(a[i]).min(b[i]);
        let hi = old_a[i].max(old_b[i]).max(a[i]).max(b[i]);
        if lo > half[i] || hi < -half[i] {
            return SweepContact::default();
        }
    }
    let smallest = half[0].min(half[1]).min(half[2]);
    let steps = ((travel / (smallest * 0.25).max(0.005)).ceil() as i32).clamp(1, 256);
    for step in 0..=steps {
        let t = step as f32 / steps as f32;
        let mut s = [0.0f32; 3];
        let mut e = [0.0f32; 3];
        for i in 0..3 {
            s[i] = old_a[i] + t * da[i];
            e[i] = old_b[i] + t * db[i];
        }
        if let Some((enter, exit)) = segment_in_box(&s, &e, half) {
            let fraction = 0.5 * (enter + exit);
            let depth = (0..3).map(|i| half[i] - (s[i] + fraction * (e[i] - s[i])).abs()).fold(f32::MAX, f32::min);
            if depth > 0.0 {
                return SweepContact { time: t, fraction, depth };
            }
        }
    }
    SweepContact::default()
}
/// CapsuleLookup: a matching hit-data handle is preferred; if the engine cloned it, all named candidates must agree on
/// the same local capsule.
#[derive(Clone, Copy, Debug, Default)]
pub struct CapsuleLookup {
    pub geometry: [f32; 8],
    pub found: bool,
    pub ambiguous: bool,
}
impl CapsuleLookup {
    /// The candidate considered; true (stop) for the exact handle.
    pub fn consider(&mut self, candidate: [f32; 8], exact_handle: bool) -> bool {
        self.ambiguous = !exact_handle && (self.ambiguous || (self.found && self.geometry != candidate));
        self.geometry = candidate;
        self.found = true;
        exact_handle
    }
}
pub fn uses_shape_resource(use_resource_data: bool, has_local_shapes: bool) -> bool {
    use_resource_data || !has_local_shapes
}
/// CallbackShapeMatch: a hit callback identifies a shape by name and user data, without its slot. A cloned data handle
/// is usable only when every same-name candidate identifies the observed container; an exact data handle takes
/// precedence over clones.
#[derive(Clone, Copy, Debug, Default)]
pub struct CallbackShapeMatch {
    pub observed: bool,
    pub other: bool,
    pub exact_observed: bool,
    pub exact_other: bool,
}
impl CallbackShapeMatch {
    pub fn consider(&mut self, same_container: bool, exact_data: bool) {
        if same_container {
            self.observed = true;
            self.exact_observed |= exact_data;
        } else {
            self.other = true;
            self.exact_other |= exact_data;
        }
    }
    pub fn matches(&self) -> bool {
        !self.exact_other && (self.exact_observed || (self.observed && !self.other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn straight_through_a_capsule() {
        let input = ContactInput { blade_start: [-0.6, 0.0, 0.0], blade_end: [0.6, 0.0, 0.0], edge_direction: [0.0, 0.0, 1.0], material: Material::Flesh, ..ContactInput::default() };
        let m = evaluate_capsule(&input, &[0.0, 0.0, 0.0], &[0.0, 0.0, 1.0], 0.1, 0.2);
        assert!(m.valid);
        assert!((m.penetration_depth - 0.2).abs() < 1e-4, "{m:?}");
        assert!((m.center_fraction - 0.5).abs() < 1e-4);
        assert!((m.incidence - 1.0).abs() < 1e-4);
    }
    #[test]
    fn sweep_finds_the_first_pose_inside() {
        let c = sweep_capsule(&[0.5, -0.6, 0.0], &[0.5, 0.6, 0.0], &[-0.5, -0.6, 0.0], &[-0.5, 0.6, 0.0], &[0.0, 0.0, 0.0], &[0.0, 0.0, 1.0], 0.1, 0.3);
        assert!(c.depth > 0.0);
        assert!(c.time > 0.35 && c.time < 0.5, "{c:?}");
        let miss = sweep_capsule(&[0.5, -1.0, 2.0], &[0.5, 1.0, 2.0], &[-0.5, -1.0, 2.0], &[-0.5, 1.0, 2.0], &[0.0, 0.0, 0.0], &[0.0, 0.0, 1.0], 0.1, 0.3);
        assert_eq!(miss, SweepContact::default());
    }
    #[test]
    fn lookups() {
        let mut l = CapsuleLookup::default();
        assert!(!l.consider([1.0; 8], false));
        assert!(!l.consider([2.0; 8], false));
        assert!(l.ambiguous);
        assert!(l.consider([3.0; 8], true));
        assert!(!l.ambiguous);
        let mut m = CallbackShapeMatch::default();
        m.consider(true, false);
        assert!(m.matches());
        m.consider(false, false);
        assert!(!m.matches());
        m.consider(true, true);
        assert!(m.matches());
        assert!(has_contact_edge(&[0.0; 3], &[0.0, 0.0, 1.0], &[1.0, 0.0, 0.0]));
        assert!(!has_contact_edge(&[0.0; 3], &[0.0, 0.0, 1.0], &[0.0, 0.0, 1.0]));
    }
    #[test]
    fn box_sweep_finds_the_first_pose_inside() {
        let half = [0.5, 1.0, 0.25];
        // a blade swept along x into the box: it enters at x = -0.5, 1.5 m into the 4 m from -2 to 2
        let c = sweep_box(&[-2.0, -0.2, 0.0], &[-2.0, 0.2, 0.0], &[2.0, -0.2, 0.0], &[2.0, 0.2, 0.0], &half);
        assert!(c.depth > 0.0 && (c.time - 1.5 / 4.0).abs() < 0.02, "{c:?}");
        // a travel over 4 m: nothing
        assert_eq!(sweep_box(&[-2.0, -0.2, 0.0], &[-2.0, 0.2, 0.0], &[2.5, -0.2, 0.0], &[2.5, 0.2, 0.0], &half), SweepContact::default());
        assert!((c.fraction - 0.5).abs() < 1e-3);
        // passing beside it: nothing
        let miss = sweep_box(&[-2.0, 0.0, 0.4], &[-2.0, 0.4, 0.4], &[2.0, 0.0, 0.4], &[2.0, 0.4, 0.4], &half);
        assert_eq!(miss, SweepContact::default());
        // a blade already half inside: at once, the middle of its inner part
        let inside = sweep_box(&[0.0, 0.0, 0.0], &[0.0, 0.0, 1.0], &[0.0, 0.0, 0.0], &[0.0, 0.0, 1.0], &half);
        assert!(inside.time == 0.0 && (inside.fraction - 0.125).abs() < 1e-3 && inside.depth > 0.0, "{inside:?}");
        assert_eq!(segment_in_box(&[2.0, 0.0, 0.0], &[3.0, 0.0, 0.0], &half), None);
    }
}
