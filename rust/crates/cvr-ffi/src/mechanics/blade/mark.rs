//! vrport_blade_surface_mark.reds: the marks a confirmed blade leaves on the struck person's own skinned mesh -- the
//! physics contact names the slot, the finite blade rays select the triangles (the blade mesh, crate::blade_mesh);
//! a lodged blade drawn along the surface traces it on.
use super::*;

/// One of the SlotComponent's slots: its bone, its offset and turn from the bone.
struct SlotBone {
    bone: u64,
    position: V4,
    rotation: V4,
}
/// The slot's entry of the hit representation (SlotComponent.slots: slotName, boneName, relativePosition,
/// relativeRotation).
fn slot_bone(slots: &Handle, slot: u64) -> Option<SlotBone> {
    let list = rt::field(slots, "slots")?;
    for i in 0..list.len() {
        let e = list.at(i)?;
        let name = e.f("slotName")?;
        // SAFETY: a CName value.
        if unsafe { *(name.ptr as *const u64) } != slot {
            continue;
        }
        // SAFETY: the entry's CName, Vector3 and Quaternion values.
        unsafe {
            let bone = *(e.f("boneName")?.ptr as *const u64);
            let p = *(e.f("relativePosition")?.ptr as *const [f32; 3]);
            let q = *(e.f("relativeRotation")?.ptr as *const [f32; 4]);
            return Some(SlotBone { bone, position: [p[0], p[1], p[2], 0.0], rotation: q });
        }
    }
    None
}
/// VRBladePaintLocal: where the mesh's paint landed (its point and normal), in the slot's frame.
fn paint_local(r: &Receipt) -> Vec<V4> {
    let target = r.target();
    let Some(sb) = slot_bone(&r.slots(), r.slot) else { return Vec::new() };
    let sample = crate::blade_mesh::paint_point(&target, sb.bone, r.serial);
    if sample.len() != 2 {
        return Vec::new();
    }
    let inverse = bn::conj(&sb.rotation);
    let p = with_w(rotate(&inverse, &sub(&sample[0], &sb.position)), sample[0][3]);
    let n = with_w(rotate(&inverse, &sample[1]), sample[1][3]);
    vec![p, n]
}
/// VRBladeCutSurface: a stroke on the mesh, in the slot's bone (kind 1 a slash, 2 a puncture, 3 a trace).
#[allow(clippy::too_many_arguments)]
fn cut_surface(r: &Receipt, start: &V4, end: &V4, normal: &V4, depth: f32, blade_a: &V4, blade_b: &V4, kind: i32, shell_depth: f32) -> u32 {
    let Some(sb) = slot_bone(&r.slots(), r.slot) else { return 0 };
    let t = |p: &V4| add(&rotate(&sb.rotation, p), &sb.position);
    let bone_normal = with_w(rotate(&sb.rotation, normal), shell_depth);
    crate::blade_mesh::queue_surface_mark(&r.target(), sb.bone, &t(start), &t(end), &bone_normal, depth, r.serial, &t(&r.mesh_start), &t(&r.mesh_end), &t(blade_a), &t(blade_b), kind)
}
/// VRBladeCapsuleNormal: the entry's outward direction from the confirmed capsule's spine (its distance in w).
fn capsule_normal(r: &Receipt) -> V4 {
    let Some((center, axis)) = r.capsule else { return ZERO };
    let half_height = axis[3];
    let mut axis = with_w(axis, 0.0);
    let mut radial = with_w(sub(&r.part_point, &center), 0.0);
    if half_height > 0.0 {
        if len(&axis) < 0.001 {
            return ZERO;
        }
        axis = norm(&axis);
        radial = sub(&radial, &mul(&axis, dot(&radial, &axis).clamp(-half_height, half_height)));
    }
    let distance = len(&radial);
    if distance < 0.001 {
        return ZERO;
    }
    with_w(mul(&radial, 1.0 / distance), distance)
}
/// The receipt's motion for its mark (publish: the edge's chord or the tip's travel; a cut shape's own frames).
fn mark_motion(rc: &Receipt, impact: &mut Impact) -> V4 {
    let mut motion = if impact.edge_stroke != 0 { sub(&impact.sweep_to, &impact.sweep_from) } else { sub(&impact.end_b, &impact.previous_b) };
    if let Some(c) = impact.cut_shape.clone() {
        if let Some(frame) = bn::slot_frame(rc.slots_ptr as *mut core::ffi::c_void, rc.slot).filter(|_| !rc.slots().is_null()) {
            impact.previous_a = to_world(&frame, &c.old_a);
            impact.previous_b = to_world(&frame, &c.old_b);
            impact.end_a = to_world(&frame, &c.end_a);
            impact.end_b = to_world(&frame, &c.end_b);
            motion = rotate(&frame.orientation(), &sub(&c.to, &c.from));
        }
    }
    motion
}
/// VRBladeAddSurfaceMark: a flesh hit marks the mesh at the entry -- 3 cm across the blade's motion, on the
/// capsule's shell.
pub(super) fn add_surface_mark(target: &Handle, shape: rpg::Shape, rc: &Sh<Receipt>, impact_rc: &Sh<Impact>) -> bool {
    let data = shape.data();
    let motion = mark_motion(&rc.borrow(), &mut impact_rc.borrow_mut());
    let mut r = rc.borrow_mut();
    if target.is_null() || !b(target, "IsAttached") || !r::is_a(&data, "HitShapeUserDataBase") || rpg::shape_type(&data) != ev("EHitShapeType", "Flesh") || r.slots().is_null() {
        return false;
    }
    let Some(frame) = bn::slot_frame(r.slots_ptr as *mut core::ffi::c_void, r.slot) else { return false };
    let inverse = bn::conj(&frame.orientation());
    let mut normal = rotate(&inverse, &with_w(sub(&shape.enter(), &shape.exit()), 0.0));
    let capsule = capsule_normal(&r);
    let shell_depth = capsule[3];
    if shell_depth > 0.0 {
        normal = with_w(capsule, 0.0);
    } else {
        if len(&normal) < 0.001 || len(&normal) > 1.25 {
            normal = with_w(r.part_point, 0.0);
        }
        if len(&normal) < 0.001 {
            normal = [0.0, 1.0, 0.0, 0.0];
        }
        normal = norm(&normal);
    }
    let mut tangent = rotate(&inverse, &with_w(motion, 0.0));
    tangent = sub(&tangent, &mul(&normal, dot(&tangent, &normal)));
    if len(&tangent) < 0.001 {
        tangent = if normal[2].abs() < 0.8 { [-normal[1], normal[0], 0.0, 0.0] } else { [0.0, -normal[2], normal[1], 0.0] };
    }
    let tangent = norm(&tangent);
    let impact = impact_rc.borrow();
    r.mesh_start = to_local(&frame, &impact.previous_a);
    r.mesh_end = to_local(&frame, &impact.previous_b);
    let (end_a, end_b) = (to_local(&frame, &impact.end_a), to_local(&frame, &impact.end_b));
    drop(impact);
    r.trail_point = closest(&r.part_point, &end_a, &end_b);
    r.trail_normal = normal;
    r.trail_ready = true;
    let part = r.part_point;
    let kind = if r.puncture { 2 } else { 1 };
    r.mesh_requests = cut_surface(&r, &sub(&part, &mul(&tangent, 0.015)), &add(&part, &mul(&tangent, 0.015)), &normal, 0.015, &end_a, &end_b, kind, shell_depth);
    r.mesh_start = end_a;
    r.mesh_end = end_b;
    r.mesh_requests > 0
}
/// VRBladeContinueTrail: a follow-through on the same person, slot and shape continues the trail.
pub(super) fn continue_trail(current: &mut Receipt, previous: &Receipt) {
    if current.target_id != previous.target_id || current.slot != previous.slot || current.shape_name != previous.shape_name {
        return;
    }
    current.trail_point = previous.trail_point;
    current.trail_normal = previous.trail_normal;
    current.trail_deadline = previous.trail_deadline;
    current.trail_segments = previous.trail_segments;
    current.trail_ready = previous.trail_ready;
    current.mesh_start = previous.mesh_start;
    current.mesh_end = previous.mesh_end;
    current.trail_on_mesh = previous.trail_on_mesh;
}
/// WeaponObject.VRBladeTraceSurface: the lodged blade drawn along the surface paints a segment (each 1.8 cm across
/// the normal, at most each 25 ms; a jump over 15 cm restarts the trail).
pub(super) fn trace_surface(w: &Handle, serial: u32, a: &V4, b_: &V4) -> bool {
    let Some(rc) = receipt(w) else { return false };
    let (owner, target, ok) = {
        let r = rc.borrow();
        (r.owner(), r.target(), r.serial == serial && r.trail_ready)
    };
    if !ok || owner.is_null() || target.is_null() || !b(&target, "IsAttached") || !allowed(&owner, w) || rc.borrow().slots().is_null() {
        return false;
    }
    let Some(frame) = bn::slot_frame(rc.borrow().slots_ptr as *mut core::ffi::c_void, rc.borrow().slot) else { return false };
    let now = sim_time();
    {
        let r = rc.borrow();
        if now < r.time || now < r.trail_deadline {
            return false;
        }
    }
    let (local_a, local_b) = (to_local(&frame, a), to_local(&frame, b_));
    let fresh = {
        let r = rc.borrow();
        !r.trail_on_mesh && r.trail_segments == 0
    };
    if fresh {
        let sample = paint_local(&rc.borrow());
        if sample.len() == 2 {
            let mut r = rc.borrow_mut();
            r.trail_point = with_w(sample[0], 1.0);
            r.trail_normal = with_w(sample[1], 0.0);
            r.trail_on_mesh = true;
        }
    }
    let (trail, normal) = {
        let r = rc.borrow();
        (r.trail_point, r.trail_normal)
    };
    let point = closest(&trail, &local_a, &local_b);
    let delta = with_w(sub(&point, &trail), 0.0);
    let tangent = sub(&delta, &mul(&normal, dot(&normal, &delta)));
    if len(&tangent) < 0.018 {
        return false;
    }
    rc.borrow_mut().trail_deadline = now + 0.025;
    if len(&delta) > 0.15 {
        let mut r = rc.borrow_mut();
        r.trail_point = point;
        r.mesh_start = local_a;
        r.mesh_end = local_b;
        return false;
    }
    let world_motion = rotate(&frame.orientation(), &delta);
    let depth = rpg::measure(w, serial, a, b_, &world_motion)[0].clamp(0.012, 0.15);
    let mut r = rc.borrow_mut();
    let requests = cut_surface(&r, &trail, &point, &normal, depth, &local_a, &local_b, 3, 0.0);
    r.mesh_requests = requests;
    r.trail_point = point;
    r.mesh_start = local_a;
    r.mesh_end = local_b;
    if requests == 0 {
        return false;
    }
    r.trail_segments += 1;
    true
}
