//! include/Anim/BladeAttachment.hpp and the rail helpers of src/Anim/BladeContactState.cpp: a blade held in a hand and
//! lodged in a body by a D6 joint whose local X is the one free (or locked) slide axis -- the shaft or the authored
//! cutting edge -- and the visible weapon pinned to the animated slot it is in.
use crate::handspace::{conjugate, finite, length_squared, multiply, rotate, Rotation, Vector};

/// LocalCutEdge: +Y is the asset's cutting edge, its component along the authored grip-to-tip line removed in item space
/// before either the hand or the target turns.
pub fn local_cut_edge(shaft: Vector) -> Option<Vector> {
    let n = length_squared(shaft);
    if !(n > 1e-6 && n.is_finite()) {
        return None;
    }
    let edge = Vector::new(0.0, 1.0, 0.0) - shaft * (shaft.y / n);
    let e = length_squared(edge);
    (e > 0.01).then(|| edge * (1.0 / e.sqrt()))
}
/// EdgeDriveWrist: the actuator has to start at the solver contact, including when a puppet reaction left the raw
/// controller a long way from its held virtual wrist. Both local D6 frames coincide at zero commanded travel. (the wrist,
/// its rotation)
pub fn edge_drive_wrist(body_point: Vector, body_frame: Rotation, hand_anchor: Vector, hand_frame: Rotation, advance: f32) -> (Vector, Rotation) {
    let wrist_rotation = multiply(body_frame, conjugate(hand_frame));
    (body_point + rotate(body_frame, Vector::new(advance, 0.0, 0.0)) - rotate(wrist_rotation, hand_anchor), wrist_rotation)
}
/// RailTravel: both samples in the coupled actor's frame; common body motion drops out. Only movement along the D6's
/// currently free translation is carried into the animated slot: locked-axis solver error must not move the pin.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RailTravel {
    pub last: Vector,
    pub total: Vector,
}
impl RailTravel {
    pub fn sample(&mut self, current: Vector, axis: Vector, sliding: bool) {
        let step = current - self.last;
        if sliding {
            self.total = self.total + axis * (step.x * axis.x + step.y * axis.y + step.z * axis.z);
        }
        self.last = current;
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BladeAttachment {
    pub wrist_local: Vector,
    pub initial_travel: Vector,
    pub prior_slip: Vector,
    pub slot_slip: Vector,
    pub body_to_slot: Rotation,
}
impl BladeAttachment {
    pub fn bind_wrist(&mut self, station: Vector, wrist: Vector, rotation: Rotation) {
        self.wrist_local = rotate(conjugate(rotation), station - wrist);
    }
    pub fn bind_rail(&mut self, travel: Vector, body: Rotation, slot: Rotation) {
        self.initial_travel = travel;
        self.prior_slip = self.slot_slip;
        self.body_to_slot = multiply(conjugate(slot), body);
    }
    pub fn goal(&mut self, animated_point: Vector, slot: Rotation, travel: Vector) -> Vector {
        self.slot_slip = self.prior_slip + rotate(self.body_to_slot, travel - self.initial_travel);
        animated_point + rotate(slot, self.slot_slip)
    }
    pub fn wrist(&self, goal: Vector, rotation: Rotation) -> Vector {
        goal - rotate(rotation, self.wrist_local)
    }
}
/// BladeWristLeash: displacement along the tracked wrist's travel permitted; only the remaining displacement of the
/// virtual wrist since this contact was seated is bound.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BladeWristLeash {
    pub seated: Vector,
    pub controller: Vector,
}
impl BladeWristLeash {
    pub fn limit(&self, tracked: Vector, wrist: &mut Vector) -> bool {
        const SLACK: f32 = 0.12;
        const RELEASE: f32 = 0.48;
        const FREE_PLAY: f32 = 0.012;
        let travel = *wrist - self.seated;
        let input = tracked - self.controller;
        if !finite(input) || !finite(travel) {
            return false;
        }
        let input_squared = length_squared(input);
        let along = input.x * travel.x + input.y * travel.y + input.z * travel.z;
        let fraction = if input_squared > 1e-8 { (along / input_squared).clamp(0.0, 1.0) } else { 0.0 };
        let supported = self.seated + input * fraction;
        let offset = *wrist - supported;
        let gap = length_squared(offset).sqrt();
        if gap > FREE_PLAY {
            let give = SLACK.min(FREE_PLAY + 0.25 * (gap - FREE_PLAY));
            *wrist = supported + offset * (give / gap);
        }
        gap <= RELEASE
    }
}
/// ReleaseWrist: from the physics body's displaced place back to the free hand over t 0..1 (smoothstep).
pub fn release_wrist(physics: Vector, controller: Vector, last_offset: Vector, t: f32) -> Vector {
    let t = t.clamp(0.0, 1.0);
    let fade = t * t * (3.0 - 2.0 * t);
    let held = controller + last_offset;
    held + (physics - held) * fade
}

fn dot(a: &[f32; 3], b: &[f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn normalized(source: &[f32; 3]) -> Option<[f32; 3]> {
    let length = dot(source, source).sqrt();
    if !length.is_finite() || length <= 1.0e-6 {
        return None;
    }
    Some([source[0] / length, source[1] / length, source[2] / length])
}
/// InsideWeaponBox: a point within 3.5 cm of a weapon box (its half extents).
pub fn inside_weapon_box(point: &[f32; 3], half_extents: &[f32; 3]) -> bool {
    let mut gap_squared = 0.0f32;
    for i in 0..3 {
        if !point[i].is_finite() || !half_extents[i].is_finite() || half_extents[i] < 0.0 {
            return false;
        }
        let outside = (point[i].abs() - half_extents[i]).max(0.0);
        gap_squared += outside * outside;
    }
    gap_squared <= 0.035 * 0.035
}
/// SelectRailAxis: the free axis -- the shaft, or for a cut the authored edge with its shaft component removed (none
/// when the edge is too near the shaft).
pub fn select_rail_axis(shaft: &[f32; 3], cutting_edge: Option<&[f32; 3]>) -> Option<[f32; 3]> {
    let axis = normalized(shaft)?;
    let Some(cutting_edge) = cutting_edge else { return Some(axis) };
    let mut edge = normalized(cutting_edge)?;
    let axial = dot(&edge, &axis);
    if axial.abs() > 0.12 {
        return None;
    }
    for i in 0..3 {
        edge[i] -= axis[i] * axial;
    }
    normalized(&edge)
}
/// ShaftPoint: the body's point brought onto the shaft's line through the hand's; its slide along the shaft.
pub fn shaft_point(hand: &[f32; 3], body: &[f32; 3], shaft: &[f32; 3]) -> (f32, [f32; 3]) {
    let delta = [body[0] - hand[0], body[1] - hand[1], body[2] - hand[2]];
    let slide = dot(&delta, shaft);
    (slide, [hand[0] + slide * shaft[0], hand[1] + slide * shaft[1], hand[2] + slide * shaft[2]])
}
/// ConfigureRailTranslations: D6 enum values Locked = 0, Free = 2; the frame builder maps local X onto either the shaft
/// or the authored cutting edge. True when the readback agrees.
pub fn configure_rail_translations(hold: bool, mut set: impl FnMut(i32, i32), mut read: impl FnMut(i32) -> i32) -> bool {
    let x = if hold { 0 } else { 2 };
    set(0, x);
    set(1, 0);
    set(2, 0);
    read(0) == x && read(1) == 0 && read(2) == 0
}
/// DampSlide: relative velocity on the D6's free axis damped at the given rate (per second). A kinematic target
/// contributes its animation velocity and receives no impulse.
#[allow(clippy::too_many_arguments)]
pub fn damp_slide(hand_velocity: &mut [f32; 3], body_velocity: &mut [f32; 3], axis: &[f32; 3], hand_mass: f32, body_mass: f32, damping_rate: f32, step: f32, kinematic_body: bool) -> bool {
    if !(hand_mass > 0.0
        && hand_mass.is_finite()
        && (kinematic_body || (body_mass > 0.0 && (hand_mass + body_mass).is_finite()))
        && damping_rate > 0.0
        && damping_rate.is_finite()
        && step > 0.0
        && step <= 0.1)
    {
        return false;
    }
    let Some(direction) = normalized(axis) else { return false };
    let relative = dot(hand_velocity, &direction) - dot(body_velocity, &direction);
    if !relative.is_finite() || relative.abs() < 1e-4 {
        return false;
    }
    let reduction = relative * (-(-(damping_rate.min(60.0)) * step).exp_m1());
    let hand_share = if kinematic_body { 1.0 } else { body_mass / (hand_mass + body_mass) };
    for i in 0..3 {
        hand_velocity[i] -= direction[i] * reduction * hand_share;
        if !kinematic_body {
            body_velocity[i] += direction[i] * reduction * (1.0 - hand_share);
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    // main's tools/physics_frame_tests/blade_contact_state_test.cpp, the parts of it that are this file's
    #[test]
    fn the_pin_carries_the_visible_point_with_the_slot_and_the_free_axis_only() {
        let identity = Rotation::default();
        let quarter_turn = Rotation::new(0.0, 0.0, 0.707_106_77, 0.707_106_77);
        let (origin, local_station) = (Vector::default(), Vector::new(0.02, 0.0, 0.0));
        let initial_wrist = Vector::new(-0.03, 0.04, -0.55);
        let mut pin = BladeAttachment::default();
        pin.bind_wrist(local_station, initial_wrist, identity);
        pin.bind_rail(origin, identity, identity);
        let mut slide = RailTravel::default();
        let target = Vector::new(0.82, -0.38, 0.025);
        let animated = target + rotate(quarter_turn, local_station);
        slide.sample(Vector::new(0.13, 0.0, 0.0), Vector::new(0.0, 0.0, 1.0), false);
        let goal = pin.goal(animated, quarter_turn, slide.total);
        let wrist = pin.wrist(goal, quarter_turn);
        assert!(length_squared(wrist + rotate(quarter_turn, pin.wrist_local) - animated) < 1e-10, "slot and visible material point diverged under rotation");
        slide.sample(Vector::new(0.13, 0.04, 0.01), Vector::new(0.0, 1.0, 0.0), true);
        let goal = pin.goal(animated, quarter_turn, slide.total);
        assert!(length_squared(goal - animated - rotate(quarter_turn, Vector::new(0.0, 0.04, 0.0))) < 1e-10, "flat or shaft solver error entered the edge offset");
        pin.bind_rail(origin, quarter_turn, quarter_turn);
        assert!(length_squared(pin.goal(animated, quarter_turn, origin) - goal) < 1e-10, "rebind reset the occupied station");
    }
    #[test]
    fn the_edge_actuator_starts_at_the_contact_and_moves_along_the_edge() {
        let local = local_cut_edge(Vector::new(0.0, 0.05, 0.75)).expect("off-axis marker");
        assert!(local.y > 0.0 && (0.05 * local.y + 0.75 * local.z).abs() < 1e-5, "off-axis marker changed cutting side");
        assert!(local_cut_edge(Vector::new(0.0, 1.0, 0.0)).is_none(), "parallel shaft and edge created a cut frame");
        let shaft = Vector::new(0.0, 0.0, 1.0);
        let local_edge = local_cut_edge(shaft).expect("missing +Y item edge");
        let (quarter_x, quarter_z) = (Rotation::new(0.707_106_77, 0.0, 0.0, 0.707_106_77), Rotation::new(0.0, 0.0, 0.707_106_77, 0.707_106_77));
        let (physical_shaft, physical_edge) = (rotate(quarter_x, shaft), rotate(quarter_x, local_edge));
        assert!(physical_edge.z > 0.999 && physical_shaft.y < -0.999, "rotated edge was compared to a stale world shaft");
        let hand_frame = quarter_z;
        let body_frame = multiply(quarter_x, hand_frame);
        let (body_point, hand_anchor) = (Vector::new(2.0, 0.9, -0.1), Vector::new(-0.14, 0.22, 0.57));
        for advance in [0.0f32, 0.008, 0.093] {
            let (target, wrist) = edge_drive_wrist(body_point, body_frame, hand_anchor, hand_frame, advance);
            let station = target + rotate(wrist, hand_anchor);
            assert!(length_squared(station - body_point - physical_edge * advance) < 1e-10, "wrist goal released old tracking error or commanded the wrong edge sign");
        }
        let moved_frame = multiply(quarter_z, body_frame);
        let moved_point = body_point + Vector::new(0.75, -0.3, 0.1);
        let (target, wrist) = edge_drive_wrist(moved_point, moved_frame, hand_anchor, hand_frame, 0.093);
        let relative = rotate(conjugate(moved_frame), target + rotate(wrist, hand_anchor) - moved_point);
        assert!(length_squared(relative - Vector::new(0.093, 0.0, 0.0)) < 1e-10, "moving target generated a different relative command");
    }
    #[test]
    fn the_leash_gives_and_lets_go_at_48_cm() {
        let leash = BladeWristLeash { seated: Vector::default(), controller: Vector::new(0.0, 0.05, 0.0) };
        let cap = 0.12f32;
        let mut pose = Vector::new(0.012, 0.0, 0.0);
        assert!(leash.limit(leash.controller, &mut pose) && pose.x == 0.012, "small idle motion lost");
        pose = Vector::new(0.13, 0.0, 0.0);
        assert!(leash.limit(leash.controller, &mut pose) && pose.x > 0.012 && pose.x < cap, "NPC reaction dragged a still wrist");
        for gap in [0.479f32, 0.48] {
            pose = Vector::new(gap, 0.0, 0.0);
            assert!(leash.limit(leash.controller, &mut pose) && (pose.x - cap).abs() < 1e-6, "capped wrist should stay attached through 48 cm");
        }
        pose = Vector::new(0.481, 0.0, 0.0);
        assert!(!leash.limit(leash.controller, &mut pose) && (pose.x - cap).abs() < 1e-6, "wrist should release beyond 48 cm with travel capped");
        pose = Vector::new(0.14, 0.46, 0.0);
        assert!(leash.limit(Vector::new(0.14, 0.05, 0.0), &mut pose) && pose.x == 0.14 && (pose.y - cap).abs() < 1e-6, "input on one axis excused transverse drag");
        pose = Vector::new(0.12, 0.0, 0.0);
        assert!(leash.limit(Vector::new(0.30, 0.05, 0.0), &mut pose) && pose.x == 0.12, "insertion resistance was pulled up to raw tracking");
        pose = Vector::new(0.5, 0.0, 0.0);
        assert!(leash.limit(Vector::new(0.5, 0.05, 0.0), &mut pose) && pose.x == 0.5, "common travel released the contact");
        assert_eq!(release_wrist(Vector::new(0.3, 0.0, 0.0), Vector::new(0.1, 0.0, 0.0), Vector::new(-0.05, 0.0, 0.0), 0.0).x, 0.05, "release starts at the physics body's displaced position");
        assert!((release_wrist(Vector::new(0.3, 0.0, 0.0), Vector::new(0.1, 0.0, 0.0), Vector::new(-0.05, 0.0, 0.0), 1.0).x - 0.3).abs() < 1e-6, "release did not return to the free hand");
    }
    #[test]
    fn the_rail_axis_modes_shaft_point_box_and_drag() {
        let (shaft, edge) = ([0.0f32, 0.0, 1.0], [0.0f32, 1.0, 0.04]);
        let free = select_rail_axis(&shaft, Some(&edge)).expect("edge mode");
        assert!(free[1] > 0.9999 && free[2] == 0.0, "edge mode must lock insertion depth, including a slightly skewed authored edge");
        assert_eq!(select_rail_axis(&shaft, None).map(|a| a[2]), Some(1.0), "withdrawal must restore shaft motion");
        assert!(select_rail_axis(&shaft, Some(&shaft)).is_none(), "parallel shaft and edge enabled cutting");
        let modes = core::cell::RefCell::new([2i32, 2, 2]);
        let set = |i: i32, v: i32| modes.borrow_mut()[i as usize] = v;
        let read = |i: i32| modes.borrow()[i as usize];
        assert!(configure_rail_translations(false, set, read) && *modes.borrow() == [2, 0, 0], "slide axes");
        assert!(configure_rail_translations(true, set, read) && *modes.borrow() == [0, 0, 0], "depth lock axes");
        assert!(!configure_rail_translations(false, set, |i| if i == 2 { 2 } else { modes.borrow()[i as usize] }), "bad flat-axis readback");
        let (h, reaction) = ([1.0f32, 2.0, 3.0], [1.10f32, 2.07, 3.02]);
        let (slide, point) = shaft_point(&h, &reaction, &shaft);
        assert!((slide - 0.02).abs() < 1e-5 && point[0] == 1.0 && point[1] == 2.0, "transverse puppet reaction counted as withdrawal");
        let blade_half = [0.02f32, 0.01, 0.35];
        assert!(inside_weapon_box(&[0.0, 0.0, 0.30], &blade_half), "partial withdrawal detached");
        assert!(!inside_weapon_box(&[0.0, 0.0, 0.40], &blade_half), "shaft slid completely off its joint anchor");
        assert!(!inside_weapon_box(&[0.07, 0.0, 0.10], &blade_half), "separated actor kept joint");
        let slide_axis = [0.0f32, 1.0, 0.0];
        let (mut hand_speed, mut body_speed) = ([2.0f32, 1.0, 4.0], [3.0f32, 0.0, 5.0]);
        assert!(!damp_slide(&mut hand_speed, &mut body_speed, &slide_axis, 2.0, 5.0, 0.0, 0.016, false), "free rail applied drag");
        assert!(damp_slide(&mut hand_speed, &mut body_speed, &slide_axis, 2.0, 5.0, 4.0, 0.016, false), "active rail applied no drag");
        assert!(hand_speed[1] < 1.0 && body_speed[1] > 0.0 && (2.0 * hand_speed[1] + 5.0 * body_speed[1] - 2.0).abs() < 1e-6, "bilateral rail momentum");
        assert!(hand_speed[0] == 2.0 && hand_speed[2] == 4.0 && body_speed[0] == 3.0 && body_speed[2] == 5.0, "drag changed a transverse velocity");
        for hz in [30, 60, 90, 144] {
            let (mut moving, mut animated) = ([1.0f32, 3.0, 5.0], [4.0f32, 1.0, 6.0]);
            for _ in 0..hz {
                damp_slide(&mut moving, &mut animated, &slide_axis, 2.0, 0.0, 4.0, 1.0 / hz as f32, true);
            }
            assert!((moving[1] - (1.0 + 2.0 * (-4.0f32).exp())).abs() < 1e-5, "animated rail damping changed with physics rate");
            assert!(animated[1] == 1.0 && moving[0] == 1.0 && moving[2] == 5.0, "animated rail modified unrelated motion");
            moving[1] = 1.0;
            assert!(!damp_slide(&mut moving, &mut animated, &slide_axis, 2.0, 0.0, 4.0, 1.0 / hz as f32, true), "co-moving hand acquired drag");
        }
    }
}
