//! vrport_blade_visible_cut.reds: the visible blade's poses in the animated frames while it is lodged, and the cut
//! along its edge clipped from them -- the chord of the native shape the edge's stroke really crossed, checked on the
//! solver's rail, the pin's frame and the shape's frame over the same window.
use super::*;

/// VrBladeObservedShape: a shape the shaft was in, as observed.
pub(in crate::mechanics) struct ObservedShape {
    pub name: u64,
    pub slot: u64,
    pub geometry: Handle,
    pub data: Handle,
    pub material: u64,
    pub capsule: (V4, V4),
    pub a: V4,
    pub b: V4,
    pub fraction: f32,
    pub interval: V4,
    pub frame: WorldTransform,
}
/// VrBladeVisiblePose: one sample of the lodged blade.
#[derive(Default)]
pub(in crate::mechanics) struct VisiblePose {
    pub serial: u32,
    pub stroke: u32,
    pub at: f64,
    pub a: V4,
    pub b: V4,
    pub goal: V4,
    pub advance: f32,
    pub rail_offset: V4,
    pub rail_axis: V4,
    pub world_a: V4,
    pub world_b: V4,
    pub rotation: V4,
    pub shapes: Vec<Rc<ObservedShape>>,
    pub counts: V4,
    pub preferred_fraction: f32,
}
/// VrBladeCutShape: the chord of a shape the edge cut.
pub(in crate::mechanics) struct CutShape {
    pub observation: Rc<ObservedShape>,
    pub name: u64,
    pub slot: u64,
    pub capsule: (V4, V4),
    pub reverse_query: bool,
    pub either_end: bool,
    pub window: V4,
    pub observed_frame: WorldTransform,
    pub shaft_from: V4,
    pub shaft_to: V4,
    pub from: V4,
    pub to: V4,
    pub old_a: V4,
    pub old_b: V4,
    pub end_a: V4,
    pub end_b: V4,
}
fn same_capsule(a: &(V4, V4), b: &(V4, V4)) -> bool {
    a.0 == b.0 && a.1 == b.1
}
/// Prior: the same shape (name, slot, material, hit data, capsule) in an earlier pose.
fn prior(pose: &VisiblePose, current: &ObservedShape) -> Option<Rc<ObservedShape>> {
    pose.shapes
        .iter()
        .find(|o| o.name == current.name && o.slot == current.slot && o.material == current.material && bn::equivalent_hit_data(o.data.instance.cast(), current.data.instance.cast()) && same_capsule(&o.capsule, &current.capsule))
        .cloned()
}
/// VRBladeKeepOccupied: a shape the shaft is in kept once per pose (the pin's fraction preferred within half its
/// depth).
fn keep(pose: &mut VisiblePose, o: bn::Occupied) -> bool {
    let reach = dist(&o.a, &o.b);
    if !(reach >= 0.04 && o.interval[3] > 0.5 && o.interval[1] > 0.001) {
        return false;
    }
    let fraction = if (pose.preferred_fraction - o.interval[0]).abs() * reach < o.interval[1] * 0.5 { pose.preferred_fraction } else { o.interval[0] };
    let obs = ObservedShape { name: o.name, slot: o.slot, geometry: o.geometry, data: o.data, material: o.material, capsule: (o.center, o.axis), a: o.a, b: o.b, fraction, interval: o.interval, frame: o.frame };
    if prior(pose, &obs).is_some() {
        return false;
    }
    pose.shapes.push(Rc::new(obs));
    true
}
/// WeaponObject.VRBladeRecordEdgePose: the blade's pose in the slot's frame at the edge stroke's sample (24 kept,
/// 150 ms), restarted on a new stroke, a regression or a gap.
#[allow(clippy::too_many_arguments)]
pub(super) fn record_edge_pose(rc: &Sh<Receipt>, pin: &Sh<SlotAttachment>, slot_frame: &WorldTransform, a: &V4, b_: &V4, edge: &V4, goal: &V4, side: i32) {
    let (serial, joint, last_stroke) = {
        let r = rc.borrow();
        (r.serial, r.joint_serial, r.last_edge_stroke)
    };
    let path = bn::slot_edge_sample(side, joint);
    let pin_slots = pin.borrow().slots.lock();
    if path.len() != 2 || path[0][3] < 1.0 || pin_slots.is_null() {
        pin.borrow_mut().edge_frames.clear();
        return;
    }
    let mut pose = VisiblePose { stroke: path[0][3] as u32, serial, at: real_time(), ..VisiblePose::default() };
    if pose.stroke == last_stroke {
        pin.borrow_mut().edge_frames.clear();
        return;
    }
    pose.rail_offset = with_w(path[0], 0.0);
    pose.rail_axis = with_w(path[1], 0.0);
    pose.advance = dot(&pose.rail_offset, &pose.rail_axis);
    pose.a = to_local(slot_frame, a);
    pose.b = to_local(slot_frame, b_);
    pose.world_a = with_w(*a, 1.0);
    pose.world_b = with_w(*b_, 1.0);
    pose.rotation = slot_frame.orientation();
    pose.goal = to_local(slot_frame, &with_w(*goal, 1.0));
    {
        let mut p = pin.borrow_mut();
        if let Some(last) = p.edge_frames.last() {
            if last.serial != serial || last.stroke != pose.stroke || last.at > pose.at || pose.at - last.at > 0.10 || pose.advance < last.advance {
                p.edge_frames.clear();
            } else if last.at == pose.at {
                return;
            }
        }
    }
    let (fraction, slot) = {
        let p = pin.borrow();
        (p.fraction, p.slot)
    };
    pose.preferred_fraction = fraction;
    let counts = bn::collect_occupied(pin_slots.instance.cast(), a, b_, edge, slot, slot_frame, |o| Some(keep(&mut pose, o)));
    pose.counts = counts;
    let mut p = pin.borrow_mut();
    while !p.edge_frames.is_empty() && (pose.at - p.edge_frames[0].at > 0.15 || p.edge_frames.len() >= 24) {
        p.edge_frames.remove(0);
    }
    p.edge_frames.push(Rc::new(pose));
}
/// UseCut: the impact swept along the cut's chord, its frames from the observed slot.
pub(super) fn use_cut(impact: &mut Impact, shape: &Rc<CutShape>) {
    impact.cut_shape = Some(shape.clone());
    impact.contact_metrics[0] = shape.window[3];
    let f = &shape.observed_frame;
    impact.sweep_from = to_world(f, &shape.from);
    impact.sweep_to = to_world(f, &shape.to);
    impact.previous_a = to_world(f, &shape.old_a);
    impact.previous_b = to_world(f, &shape.old_b);
    impact.end_a = to_world(f, &shape.end_a);
    impact.end_b = to_world(f, &shape.end_b);
}
/// BoxHalfExtent: the stock effect's cube.
pub(in crate::mechanics) fn box_half_extent() -> f32 {
    0.035
}
/// ExteriorEnd: which end's whole starting cube clears the observed capsule (1 a, -1 b, 3 both, 0 neither).
fn exterior_end(obs: &ObservedShape) -> i32 {
    let (mut center, mut axis) = obs.capsule;
    let clearance = center[3] + 3f32.sqrt() * box_half_extent();
    let height = axis[3];
    center[3] = 1.0;
    axis[3] = 0.0;
    let half = mul(&norm(&axis), height);
    let (lo, hi) = (sub(&center, &half), add(&center, &half));
    let clear_a = dist(&obs.a, &closest(&obs.a, &lo, &hi)) > clearance;
    let clear_b = dist(&obs.b, &closest(&obs.b, &lo, &hi)) > clearance;
    match (clear_a, clear_b) {
        (true, true) => 3,
        (true, false) => 1,
        (false, true) => -1,
        _ => 0,
    }
}
fn station(a: &V4, b_: &V4, f: f32) -> V4 {
    lerp(a, b_, f)
}
/// Window: x the solver's signed travel over the pair of samples, y and z the visible travel at the same shaft
/// fraction in the pin's and the shape's frames, w their shared, direction-checked length.
fn window(old: &VisiblePose, now: &VisiblePose, prior: &ObservedShape, current: &ObservedShape, pin_to_shape: &V4, intent: f32) -> V4 {
    let f = current.fraction;
    let physical = with_w(sub(&now.rail_offset, &old.rail_offset), 0.0);
    let mapped = now.rail_axis;
    let measured = dot(&physical, &mapped);
    let goal = with_w(sub(&now.goal, &old.goal), 0.0);
    let pin_motion = with_w(sub(&station(&now.a, &now.b, f), &station(&old.a, &old.b, f)), 0.0);
    let motion = with_w(sub(&station(&current.a, &current.b, f), &station(&prior.a, &prior.b, f)), 0.0);
    let distance = len(&goal);
    if !((0.0..=1.0).contains(&f) && (0.008..=0.35).contains(&measured) && (0.008..=0.35).contains(&distance)) {
        return [measured, 0.0, 0.0, 0.0];
    }
    let goal_travel = dot(&goal, &mapped);
    let at_pin = dot(&pin_motion, &mapped);
    let at_shape = dot(&motion, &rotate(pin_to_shape, &mapped));
    let (pin_length, length) = (len(&pin_motion), len(&motion));
    if !(measured >= 0.96 * len(&physical) && len(&physical) <= 0.35 && goal_travel >= 0.96 * distance && pin_length <= 0.35 && length <= 0.35 && at_pin >= 0.96 * pin_length && at_shape >= 0.96 * length) {
        return [measured, at_pin, at_shape, 0.0];
    }
    let credit = intent.min(measured.min(goal_travel.min(at_pin.min(at_shape))));
    [measured, at_pin, at_shape, if credit >= 0.008 { credit } else { 0.0 }]
}
/// WeaponObject.VRBladeCaptureCut: the edge's solver path (its stroke unused, at least 8 mm along the rail edge, the
/// controller along the seat's edge) clipped to the visible windows.
pub(super) fn capture_cut(w: &Handle, impact: &Sh<Impact>, motion: &V4) -> bool {
    let Some(rc) = receipt(w) else { return false };
    let (owner, joint, last_stroke, seat_edge) = {
        let r = rc.borrow();
        (r.owner(), r.joint_serial, r.last_edge_stroke, r.seat_edge)
    };
    let side = hand_side(&owner);
    let path = bn::edge_path(side, joint, true);
    if path.len() != 3 || path[2][3] != 5.0 {
        return rpg::follow_result(w, "NoSolverPath");
    }
    if rc.borrow().slots().is_null() {
        return rpg::follow_result(w, "NoHitSlots");
    }
    let stroke = path[0][3] as u32;
    if stroke == 0 || stroke == last_stroke {
        return rpg::follow_result(w, "UsedStroke");
    }
    let (start, end) = (with_w(path[0], 1.0), with_w(path[1], 1.0));
    let travel = with_w(sub(&end, &start), 0.0);
    let distance = len(&travel);
    let rail_edge = with_w(path[2], 0.0);
    let (ia, ib) = {
        let i = impact.borrow();
        (i.a, i.b)
    };
    let geometry = {
        let st = weapon(w);
        let mut s = st.borrow_mut();
        s.follow_geometry[0] = dist(&end, &closest(&end, &ia, &ib));
        s.follow_geometry[1] = dot(&travel, &rail_edge) / distance.max(0.000_001);
        s.follow_geometry[2] = dot(motion, &seat_edge) / len(motion).max(0.000_001);
        s.follow_geometry
    };
    if !(distance >= 0.008) {
        return rpg::follow_result(w, "SolverDistance");
    }
    if !(geometry[1] >= 0.96) {
        return rpg::follow_result(w, "SolverDirection");
    }
    if !(geometry[2] >= 0.96) {
        return rpg::follow_result(w, "ControllerDirection");
    }
    clip_stations(w, &rc, impact, stroke, dot(motion, &seat_edge))
}
/// WeaponObject.VRBladeClipStations: the longest chord, over the visible windows of each shape the blade is in, that
/// the solver, the pin and the shape all moved along -- the follow-through's cut.
fn clip_stations(w: &Handle, rc: &Sh<Receipt>, impact_rc: &Sh<Impact>, stroke: u32, intent: f32) -> bool {
    let (pin, slots_ptr, material) = {
        let r = rc.borrow();
        (r.attachment.clone(), r.slots_ptr, r.material)
    };
    let Some(pin) = pin.filter(|p| !p.borrow().slots.lock().is_null() && p.borrow().slots_ptr == slots_ptr) else { return rpg::follow_result(w, "NoVisiblePin") };
    let frames = pin.borrow().edge_frames.clone();
    let Some(now) = frames.last().cloned() else { return rpg::follow_result(w, "NoVisibleWindow") };
    let count = frames.len();
    let st = weapon(w);
    st.borrow_mut().follow_counts = now.counts;
    let age = real_time() - now.at;
    let parent = impact_rc.borrow().parent_serial;
    if now.serial != parent || now.stroke != stroke || !(0.0..=0.05).contains(&age) {
        return rpg::follow_result(w, "StaleVisibleWindow");
    }
    st.borrow_mut().follow_geometry[3] = now.shapes.len() as f32;
    if now.shapes.is_empty() {
        if now.counts[3] < 0.0 {
            return rpg::follow_result(w, "ObservationUnavailable");
        }
        if now.counts[0] > 0.0 {
            return rpg::follow_result(w, if now.counts[1] > 0.0 { "NoRetainedShape" } else { "NoResolvedShape" });
        }
        if now.counts[3] > 0.0 {
            return rpg::follow_result(w, "UnresolvedSlots");
        }
        return rpg::follow_result(w, "VisibleBladeClear");
    }
    let (latest_a, latest_b) = {
        let i = impact_rc.borrow();
        (with_w(i.a, 1.0), with_w(i.b, 1.0))
    };
    if dist(&latest_a, &now.world_a) > 0.001 || dist(&latest_b, &now.world_b) > 0.001 {
        return rpg::follow_result(w, "StaleVisibleWindow");
    }
    let mut best = 0.0f32;
    let mut gate = "NoVisibleWindow";
    for current in &now.shapes {
        let frame = current.frame;
        let exterior = exterior_end(current);
        let mut chord: Option<Rc<CutShape>> = None;
        let pin_to_shape = qmul(&bn::conj(&frame.orientation()), &now.rotation);
        let mut index = count as i32 - 2;
        if !rpg::valid_zone_data(&current.data) {
            index = -1;
        }
        while index >= 0 {
            let old = &frames[index as usize];
            let Some(prior) = prior(old, current) else { break };
            let win = window(old, &now, &prior, current, &pin_to_shape, intent);
            if best == 0.0 && win[0] >= st.borrow().follow_window[0] {
                st.borrow_mut().follow_window = win;
            }
            if win[3] > 0.0 {
                gate = "NoClippedInterval";
                let end = station(&current.a, &current.b, current.fraction);
                let was = station(&prior.a, &prior.b, current.fraction);
                let share = win[3] / dist(&was, &end);
                let start = add(&end, &mul(&sub(&was, &end), share));
                let shaft = norm(&sub(&current.b, &current.a));
                let interval = bn::capsule_result(&start, &end, &ZERO, &shaft, &current.capsule.0, &current.capsule.1, material, true);
                if interval[3] > 0.5 && interval[1] > 0.001 && chord.as_ref().is_none_or(|c| interval[1] > c.window[3]) {
                    if exterior == 0 {
                        gate = "NoExteriorStart";
                        break;
                    }
                    let half = interval[1] / (2.0 * win[3]);
                    let shaft_half = current.interval[1] / (2.0 * dist(&current.a, &current.b));
                    let first = (interval[0] - half).clamp(0.0, 1.0);
                    let last = (interval[0] + half).clamp(0.0, 1.0);
                    chord = Some(Rc::new(CutShape {
                        observation: current.clone(),
                        name: current.name,
                        slot: current.slot,
                        capsule: current.capsule,
                        reverse_query: exterior < 0,
                        either_end: exterior == 3,
                        window: [win[0], win[1], win[2], interval[1]],
                        observed_frame: frame,
                        shaft_from: station(&current.a, &current.b, (current.interval[0] - shaft_half).clamp(0.0, 1.0)),
                        shaft_to: station(&current.a, &current.b, (current.interval[0] + shaft_half).clamp(0.0, 1.0)),
                        from: station(&start, &end, first),
                        to: station(&start, &end, last),
                        old_a: station(&current.a, &prior.a, share * (1.0 - first)),
                        old_b: station(&current.b, &prior.b, share * (1.0 - first)),
                        end_a: station(&current.a, &prior.a, share * (1.0 - last)),
                        end_b: station(&current.b, &prior.b, share * (1.0 - last)),
                    }));
                }
            } else if gate == "NoVisibleWindow" {
                gate = "StationMotion";
            }
            index -= 1;
        }
        if let Some(chord) = chord {
            let mut i = impact_rc.borrow_mut();
            i.cut_candidates.push(chord.clone());
            if chord.window[3] > best {
                best = chord.window[3];
                st.borrow_mut().follow_window = chord.window;
                i.cut_shape = Some(chord);
            }
        }
    }
    if best <= 0.0 {
        return rpg::follow_result(w, gate);
    }
    let solver_cos = st.borrow().follow_geometry[1];
    let mut i = impact_rc.borrow_mut();
    i.a = now.world_a;
    i.b = now.world_b;
    i.edge_stroke = stroke;
    i.contact_metrics = [best, 0.0, solver_cos, 0.0];
    if let Some(c) = i.cut_shape.clone() {
        use_cut(&mut i, &c);
    }
    true
}
