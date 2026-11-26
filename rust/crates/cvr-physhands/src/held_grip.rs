//! include/Natives/HeldGripPose.hpp: a held ragdoll part driven toward the hand. Quaternions x y z w.

/// DriveAngular: numerical mass scaling makes the held ragdoll stable against its connected bodies; it must not weaken
/// the grip. The acceleration is bounded by the part's original inertia, and the angular chase damped before the target
/// is reached.
pub fn drive_angular(error: &[f32; 3], velocity: &[f32; 3], original_inertia: &[f32; 3], torque: f32, step: f32, max_spin: f32) -> [f32; 3] {
    let inertia = original_inertia[0].max(original_inertia[1]).max(original_inertia[2]).max(1e-3);
    let response = 1.0 - (-48.0 * step).exp();
    let mut dw = [0.0f32; 3];
    let mut n2 = 0.0;
    for k in 0..3 {
        dw[k] = (error[k] * 12.0 - velocity[k]) * response;
        n2 += dw[k] * dw[k];
    }
    let n = n2.sqrt();
    let limit = torque.max(0.0) / inertia * step;
    let scale = if n > limit { limit / n } else { 1.0 };
    let mut out = [0.0f32; 3];
    let mut speed2 = 0.0;
    for k in 0..3 {
        out[k] = velocity[k] + dw[k] * scale;
        speed2 += out[k] * out[k];
    }
    let speed = speed2.sqrt();
    if speed > max_spin {
        for o in &mut out {
            *o *= max_spin / speed;
        }
    }
    out
}
/// PinnedForearmLength: a held body keeps the forearm the length the hold wants.
pub fn pinned_forearm_length(wanted: f32, minimum: f32, maximum: f32, held_body: bool) -> f32 {
    if held_body {
        wanted
    } else {
        wanted.clamp(minimum, maximum)
    }
}
pub fn multiply(a: &[f32; 4], b: &[f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}
pub fn normalize(q: &mut [f32; 4]) -> bool {
    let n: f32 = q.iter().map(|v| v * v).sum();
    if !n.is_finite() || n < 1e-10 {
        return false;
    }
    let n = 1.0 / n.sqrt();
    for v in q.iter_mut() {
        *v *= n;
    }
    true
}
pub fn rotate(q: &[f32; 4], v: &[f32; 3]) -> [f32; 3] {
    let t = [2.0 * (q[1] * v[2] - q[2] * v[1]), 2.0 * (q[2] * v[0] - q[0] * v[2]), 2.0 * (q[0] * v[1] - q[1] * v[0])];
    [
        v[0] + q[3] * t[0] + q[1] * t[2] - q[2] * t[1],
        v[1] + q[3] * t[1] + q[2] * t[0] - q[0] * t[2],
        v[2] + q[3] * t[2] + q[0] * t[1] - q[1] * t[0],
    ]
}
/// DriveTarget: the visual wrist may already be rotated by the body's weight. Its reachable grip point is kept, but the
/// controller's independent angular goal retained; feeding the corrected wrist back would cancel the torque that should
/// turn the held part toward the controller. (position, rotation) or None.
pub fn drive_target(drawn_p: &[f32; 3], drawn_q: &[f32; 4], control_q: &[f32; 4], body_p: &[f32; 3], body_q: &[f32; 4], grip: &[f32; 3]) -> Option<([f32; 3], [f32; 4])> {
    let (mut draw, mut control, mut relation) = (*drawn_q, *control_q, *body_q);
    if !normalize(&mut draw) || !normalize(&mut control) || !normalize(&mut relation) {
        return None;
    }
    let mut in_hand = rotate(&relation, grip);
    for k in 0..3 {
        in_hand[k] += body_p[k];
    }
    let point = rotate(&draw, &in_hand);
    let mut out_q = multiply(&control, &relation);
    if !normalize(&mut out_q) {
        return None;
    }
    let rotated_grip = rotate(&out_q, grip);
    let mut out_p = [0.0f32; 3];
    for k in 0..3 {
        out_p[k] = drawn_p[k] + point[k] - rotated_grip[k];
        if !out_p[k].is_finite() {
            return None;
        }
    }
    Some((out_p, out_q))
}
/// TwoHandTarget: two separated grips determine the body's swing as well as its translation. The average wrist twist is
/// kept, then the line through the body's grips aligned with the line through the hands. Averaging wrist rotations
/// alone left a body horizontal when one hand was raised, despite both hands holding it. (position, rotation, the grips'
/// centre in the body's frame) or None.
#[allow(clippy::too_many_arguments)]
pub fn two_hand_target(a_p: &[f32; 3], a_q: &[f32; 4], a_grip: &[f32; 3], b_p: &[f32; 3], b_q: &[f32; 4], b_grip: &[f32; 3]) -> Option<([f32; 3], [f32; 4], [f32; 3])> {
    let (mut qa, mut qb) = (*a_q, *b_q);
    if !normalize(&mut qa) || !normalize(&mut qb) {
        return None;
    }
    let mut dot: f32 = (0..4).map(|k| qa[k] * qb[k]).sum();
    let sign = if dot < 0.0 { -1.0 } else { 1.0 };
    let mut out_q = [0.0f32; 4];
    for k in 0..4 {
        out_q[k] = qa[k] + sign * qb[k];
    }
    if !normalize(&mut out_q) {
        return None;
    }
    let mut ga = rotate(&qa, a_grip);
    let mut gb = rotate(&qb, b_grip);
    let (mut centre, mut mid, mut local, mut world) = ([0.0f32; 3], [0.0f32; 3], [0.0f32; 3], [0.0f32; 3]);
    let (mut local2, mut world2) = (0.0f32, 0.0f32);
    for k in 0..3 {
        ga[k] += a_p[k];
        gb[k] += b_p[k];
        centre[k] = 0.5 * (a_grip[k] + b_grip[k]);
        mid[k] = 0.5 * (ga[k] + gb[k]);
        local[k] = b_grip[k] - a_grip[k];
        world[k] = gb[k] - ga[k];
        if !centre[k].is_finite() || !mid[k].is_finite() {
            return None;
        }
        local2 += local[k] * local[k];
        world2 += world[k] * world[k];
    }
    if !local2.is_finite() || !world2.is_finite() {
        return None;
    }
    // Coincident or near-coincident grips cannot reliably define an axis.
    if local2 >= 0.0001 && world2 >= 0.0001 {
        let mut from = rotate(&out_q, &local);
        let (il, iw) = (1.0 / local2.sqrt(), 1.0 / world2.sqrt());
        dot = 0.0;
        for k in 0..3 {
            from[k] *= il;
            world[k] *= iw;
            dot += from[k] * world[k];
        }
        let mut swing = [
            from[1] * world[2] - from[2] * world[1],
            from[2] * world[0] - from[0] * world[2],
            from[0] * world[1] - from[1] * world[0],
            1.0 + dot.clamp(-1.0, 1.0),
        ];
        if dot < -0.999999 {
            // A half turn has no unique shortest axis: a stable axis perpendicular to the grip line, from its
            // least-aligned basis.
            let mut axis = 0;
            for k in 1..3 {
                if from[k].abs() < from[axis].abs() {
                    axis = k;
                }
            }
            let basis = [f32::from(u8::from(axis == 0)), f32::from(u8::from(axis == 1)), f32::from(u8::from(axis == 2))];
            swing = [from[1] * basis[2] - from[2] * basis[1], from[2] * basis[0] - from[0] * basis[2], from[0] * basis[1] - from[1] * basis[0], 0.0];
        }
        if !normalize(&mut swing) {
            return None;
        }
        out_q = multiply(&swing, &out_q);
        if !normalize(&mut out_q) {
            return None;
        }
    }
    let offset = rotate(&out_q, &centre);
    let out_p = [mid[0] - offset[0], mid[1] - offset[1], mid[2] - offset[2]];
    Some((out_p, out_q, centre))
}
/// WristRotation: the held part's measured rotation and its fixed hand-relative rotation determine the wrist -- each
/// hand's own relation, not a two-hand average's error or an angle-limited correction.
pub fn wrist_rotation(body: &[f32; 4], body_in_hand: &[f32; 4]) -> Option<[f32; 4]> {
    let inv = [-body_in_hand[0], -body_in_hand[1], -body_in_hand[2], body_in_hand[3]];
    let mut wrist = multiply(body, &inv);
    normalize(&mut wrist).then_some(wrist)
}
/// SeatRotation: rotate about the point held, keeping the latest controller-driven grip position.
pub fn seat_rotation(position: &mut [f32; 3], rotation: &mut [f32; 4], measured: &[f32; 4], seat: &[f32; 3], blend: f32) -> bool {
    let d: f32 = (0..4).map(|i| rotation[i] * measured[i]).sum();
    let sign = if d < 0.0 { -1.0 } else { 1.0 };
    let w = blend.clamp(0.0, 1.0);
    let mut q = [0.0f32; 4];
    for i in 0..4 {
        q[i] = rotation[i] * (1.0 - w) + measured[i] * sign * w;
    }
    if !normalize(&mut q) {
        return false;
    }
    let before = rotate(rotation, seat);
    let after = rotate(&q, seat);
    for i in 0..3 {
        position[i] += before[i] - after[i];
    }
    *rotation = q;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    fn yaw(degrees: f32) -> [f32; 4] {
        let h = degrees * 0.008_726_646;
        [0.0, 0.0, h.sin(), h.cos()]
    }
    // main's tools/physics_frame_tests/grip_drive_test.cpp
    #[test]
    fn the_held_drive_keeps_the_reachable_grip_and_the_controllers_turn() {
        let wrist = [-1042.0f32, 1337.0, 6.6];
        let relation_p = [0.08f32, -0.06, 0.04];
        let grip = [0.03f32, 0.1, -0.02];
        for visual in [-170.0f32, -65.0, 0.0, 70.0, 179.0] {
            for wanted in [-160.0f32, -40.0, 45.0, 150.0] {
                for relation in [-35.0f32, 0.0, 80.0] {
                    let (draw, control, body_q) = (yaw(visual), yaw(wanted), yaw(relation));
                    let (out_p, out_q) = drive_target(&wrist, &draw, &control, &relation_p, &body_q, &grip).expect("valid drive rejected");
                    let mut local = rotate(&body_q, &grip);
                    for k in 0..3 {
                        local[k] += relation_p[k];
                    }
                    let before = rotate(&draw, &local);
                    let after = rotate(&out_q, &grip);
                    for k in 0..3 {
                        assert!(((out_p[k] + after[k]) - (wrist[k] + before[k])).abs() < 0.00025, "visual grip point moved when angular drive changed");
                    }
                    let expected = multiply(&control, &body_q);
                    let dot: f32 = (0..4).map(|k| out_q[k] * expected[k]).sum();
                    assert!(dot.abs() > 0.99999, "load-corrected wrist cancelled controller torque");
                }
            }
        }
        let (still, mut turn) = (yaw(0.0), yaw(90.0));
        let (p, q) = drive_target(&wrist, &still, &turn, &relation_p, &still, &grip).expect("held rotation test");
        assert!(q[2].abs() > 0.7, "stationary visual wrist swallowed a ninety-degree controller turn");
        let origin = [-1024.0f32, 1024.0, 256.0];
        let local_wrist = [wrist[0] - origin[0], wrist[1] - origin[1], wrist[2] - origin[2]];
        let (local_p, _) = drive_target(&local_wrist, &still, &turn, &relation_p, &still, &grip).expect("rebased drive rejected");
        for k in 0..3 {
            assert!((local_p[k] + origin[k] - p[k]).abs() < 0.00025, "scene origin rebase moved held grip");
        }
        turn[0] = f32::NAN;
        assert!(drive_target(&wrist, &still, &turn, &relation_p, &still, &grip).is_none(), "NaN control accepted");
    }
    #[test]
    fn two_hands_turn_the_body_by_the_line_through_their_grips() {
        let wrist = [-1042.0f32, 1337.0, 6.6];
        let still = yaw(0.0);
        let (a_grip, b_grip) = ([-0.2f32, 0.03, 0.02], [0.2f32, 0.03, 0.02]);
        for angle in [-179.0f32, -90.0, -30.0, 0.0, 40.0, 90.0, 179.0, 180.0] {
            let expected = yaw(angle);
            let (mut ga, mut gb) = (rotate(&expected, &a_grip), rotate(&expected, &b_grip));
            let (mut ap, mut bp) = ([0.0f32; 3], [0.0f32; 3]);
            for k in 0..3 {
                ga[k] += wrist[k];
                gb[k] += wrist[k];
                ap[k] = ga[k] - a_grip[k];
                bp[k] = gb[k] - b_grip[k];
            }
            let (out_p, out_q, _) = two_hand_target(&ap, &still, &a_grip, &bp, &still, &b_grip).expect("two-hand target rejected");
            let (got_a, got_b) = (rotate(&out_q, &a_grip), rotate(&out_q, &b_grip));
            for k in 0..3 {
                assert!((out_p[k] + got_a[k] - ga[k]).abs() < 0.0005, "first grip missed when hands turned body");
                assert!((out_p[k] + got_b[k] - gb[k]).abs() < 0.0005, "second grip missed when hands turned body");
            }
            let (sp, sq, _) = two_hand_target(&bp, &still, &b_grip, &ap, &still, &a_grip).expect("swapped hands rejected");
            for k in 0..3 {
                assert!((sp[k] - out_p[k]).abs() < 0.0005, "hand order moved body");
            }
            let d: f32 = (0..4).map(|k| sq[k] * out_q[k]).sum();
            assert!(d.abs() > 0.99999, "hand order changed body turn");
        }
        // Unequal hand separation cannot stretch a rigid body.
        let (ap, bp) = ([0.2f32, -0.4, 0.0], [-0.2f32, 0.4, 0.0]);
        let (p, q, _) = two_hand_target(&ap, &still, &a_grip, &bp, &still, &b_grip).expect("stretched hand span rejected");
        let (ca, cb) = (rotate(&q, &a_grip), rotate(&q, &b_grip));
        assert!((cb[0] - ca[0]).abs() < 1e-6 && (cb[1] - ca[1] - 0.4).abs() < 1e-6, "two-hand span changed rigid body length");
        for k in 0..3 {
            assert!((p[k] + 0.5 * (ca[k] + cb[k]) - 0.5 * (ap[k] + a_grip[k] + bp[k] + b_grip[k])).abs() < 1e-6, "unequal span lost midpoint");
        }
        let (zero, same, roll) = ([0.0f32; 3], [0.05f32, 0.0, 0.0], [0.5f32, 0.0, 0.0, 0.866_025_4]);
        let (_, q, _) = two_hand_target(&zero, &roll, &same, &zero, &roll, &same).expect("coincident grips rejected");
        assert!((q[0] - 0.5).abs() < 1e-6, "coincident grips lost wrist twist");
        let reversed = [-0.5f32, 0.0, 0.0, -0.866_025_4];
        let (_, q, _) = two_hand_target(&zero, &roll, &same, &zero, &reversed, &same).expect("antipodal quaternion rejected");
        assert!((q[0] - 0.5).abs() < 1e-6, "antipodal quaternion lost wrist twist");
        // A vertical lift with both wrists rolled 60 degrees: the grip line supplies the swing, the roll stays.
        let (ga_l, gb_l) = ([-0.2f32, 0.0, 0.0], [0.2f32, 0.0, 0.0]);
        let (lifted_a, lifted_b) = ([0.2f32, 0.0, -0.2], [-0.2f32, 0.0, 0.2]);
        let (_, q, _) = two_hand_target(&lifted_a, &roll, &ga_l, &lifted_b, &roll, &gb_l).expect("vertical two-hand lift rejected");
        let (axis, across) = (rotate(&q, &[1.0, 0.0, 0.0]), rotate(&q, &[0.0, 1.0, 0.0]));
        assert!(axis[0].abs() < 1e-6 && (axis[2] - 1.0).abs() < 1e-6, "vertical lift did not swing body");
        assert!((across[0] + 0.866_025_4).abs() < 1e-6 && (across[1] - 0.5).abs() < 1e-6, "vertical lift discarded wrist twist");
        let bad = [f32::INFINITY, 0.0, 0.0];
        assert!(two_hand_target(&bad, &still, &a_grip, &bp, &still, &b_grip).is_none(), "infinite two-hand position accepted");
    }
    #[test]
    fn the_angular_drive_is_bounded_by_the_parts_own_inertia_and_the_spin() {
        let w = drive_angular(&[1.0, 0.0, 0.0], &[0.0; 3], &[0.5, 0.5, 0.5], 1.0, 0.02, 100.0);
        assert!((w[0] - 1.0 / 0.5 * 0.02).abs() < 1e-6, "{w:?}");
        let w = drive_angular(&[100.0, 0.0, 0.0], &[0.0; 3], &[1e-6; 3], 1e6, 0.02, 3.0);
        assert!((w[0] - 3.0).abs() < 1e-4);
        assert_eq!(pinned_forearm_length(0.5, 0.2, 0.3, true), 0.5);
        assert_eq!(pinned_forearm_length(0.5, 0.2, 0.3, false), 0.3);
    }
}
