//! mods/cet/CyberpunkVRPort_Weapon/blade_contact_pose.lua: the confirmed blade's contact offsets the virtual wrist's
//! servo target (never the raw XR sample) -- the lodged blade's joint kept (held at depth, slid in, loaded, the edge
//! driven along a stroke), the release on a pull or a separation, the surface traced, a held ragdoll pushed, and a
//! deliberate stroke along the edge sent as the follow-through.
use super::*;
use crate::script_call::Value;

struct Sample {
    at: f64,
    rel: V4,
    track: V4,
    joint: bool,
    start: V4,
    tip: V4,
}
struct Seat {
    weapon: Handle,
    serial: u32,
    track_at: V4,
    joint_serial: u32,
    raw_from_anchor: V4,
    last_track: V4,
    axial: f32,
    last_slide: f32,
    input_axis: V4,
    input_edge: V4,
    input_basis: V4,
    backout: f32,
    travel_time: f64,
    slide_until: f64,
    samples: Vec<Sample>,
    dispatched: bool,
    measure_at: Option<f64>,
    trail_at: Option<f64>,
    trail_track: Option<V4>,
    load_at: Option<f64>,
    retry_at: Option<f64>,
    pull_minimum: Option<f32>,
    edge_track: Option<V4>,
    edge_until: Option<f64>,
    edge_from: Option<V4>,
    joint: bool,
    resistance: f32,
    clock: f64,
}
struct Waiting {
    weapon: Handle,
    serial: u32,
    track: V4,
}
#[derive(Default)]
struct State {
    serial: Option<u32>,
    weapon_id: Option<u64>,
    side: Option<i32>,
    seat: Option<Seat>,
    waiting: Option<Waiting>,
    sent: bool,
    phase: &'static str,
    binds: u32,
    requests: u32,
    reported: bool,
}
thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}
fn v(x: f32, y: f32, z: f32) -> V4 {
    [x, y, z, 0.0]
}
/// Tracking space (Y up) to the world by the body's heading basis (x, y).
fn world_motion(delta: &V4, basis: &V4) -> V4 {
    v(basis[0] * delta[0] + basis[1] * delta[2], basis[1] * delta[0] - basis[0] * delta[2], delta[1])
}
fn finite(a: &V4) -> bool {
    a[0].is_finite() && a[1].is_finite() && a[2].is_finite() && a[0].abs() + a[1].abs() + a[2].abs() < 1e7
}
fn native_v4(name: &str, values: &[Value]) -> V4 {
    super::super::belt::native::<V4>(name, values).unwrap_or([f32::NAN; 4])
}
fn paused() -> bool {
    super::super::requests_handler().ok().filter(|r| !r.is_null()).is_some_and(|r| super::super::belt::m::<bool>(&r, "IsGamePaused", &[]).unwrap_or(false))
}
fn pose_offset(side: i32, d: V4) -> bool {
    cvr_rig::vrik::blade_pose::publish(side, d)
}
fn clear(s: &mut State, phase: &'static str) {
    let side = s.side.unwrap_or(1);
    if let Some(seat) = s.seat.take() {
        release_contact(&seat.weapon, seat.serial);
        bn::clear_joint(side);
    } else if let Some(w) = s.waiting.take() {
        release_contact(&w.weapon, w.serial);
    }
    if s.sent {
        pose_offset(side, ZERO);
        s.sent = false;
    }
    s.waiting = None;
    s.phase = phase;
}
/// M.reset: the contact let go, the pose's state forgotten.
pub(in crate::mechanics) fn reset() {
    let _g = lock();
    STATE.with(|st| {
        let mut s = st.borrow_mut();
        clear(&mut s, "idle");
        s.serial = None;
        s.weapon_id = None;
        s.side = None;
    });
}
/// M.update: one frame of the blade in hand (its grip, its direction, the XR sample of the hand).
pub(in crate::mechanics) fn update(dt: f64, pl: &Handle, w: &Handle, position: V4, axis: V4, tracked: Option<V4>) {
    let _g = lock();
    STATE.with(|st| {
        let mut s = st.borrow_mut();
        frame(&mut s, dt, pl, w, position, axis, tracked);
        if !s.reported && s.binds > 0 {
            s.reported = true;
            cvr_host::log!("[blade] the confirmed blade's contact bound to the hand (phase {}) from Rust\n", s.phase);
        }
    });
}
#[allow(clippy::too_many_arguments)]
fn frame(s: &mut State, dt: f64, pl: &Handle, w: &Handle, position: V4, axis: V4, tracked: Option<V4>) {
    let id = eid(w);
    let hand = hand_side(pl);
    let n = confirmed_serial(w);
    if s.weapon_id != Some(id) || s.side != Some(hand) {
        clear(s, "idle");
        s.weapon_id = Some(id);
        s.side = Some(hand);
        s.serial = Some(n);
        return;
    }
    let fresh = s.serial != Some(n);
    s.serial = Some(n);
    if fresh {
        s.waiting = None;
    }
    let rc = receipt(w);
    if fresh && rc.as_ref().is_some_and(|r| r.borrow().released) {
        clear(s, "released");
        return;
    }
    if !fresh && s.seat.is_none() && s.waiting.is_none() {
        return;
    }
    let Some(rc) = rc else {
        clear(s, "released");
        return;
    };
    let tracked = tracked.unwrap_or([f32::NAN; 4]);
    if !finite(&tracked) || tracked[3] <= 0.0 || s.seat.as_ref().is_some_and(|seat| tracked[3] != seat.track_at[3]) || dt <= 0.0 || !finite(&axis) || !finite(&position) || !allowed(pl, w) || paused() {
        clear(s, "unavailable");
        return;
    }
    let (puncture_kind, receipt_joint) = {
        let r = rc.borrow();
        (r.puncture || r.lodged, r.joint_serial)
    };
    if dt > 0.12 {
        if s.seat.is_none() || fresh || !puncture_kind {
            clear(s, "unavailable");
            return;
        }
        // a late frame can expire the solver's lease: keep the confirmed entry, drop the stale gesture history
        if let Some(seat) = s.seat.as_mut() {
            seat.samples.clear();
            seat.edge_until = None;
            seat.edge_track = None;
            seat.edge_from = None;
        }
    }
    let age = confirmed_age(w);
    if age < 0.0 {
        clear(s, "released");
        return;
    }
    if (fresh || s.waiting.is_some()) && age > 0.20 {
        clear(s, "released");
        return;
    }
    let continuation = fresh && rc.borrow().follow;
    if continuation && s.seat.as_ref().is_none_or(|seat| !seat.dispatched || seat.joint_serial != receipt_joint) {
        clear(s, "released");
        return;
    }
    let target = rc.borrow().target();
    if target.is_null() || !b(&target, "IsAttached") {
        clear(s, "target gone");
        return;
    }
    let puncture = puncture_kind;
    let a = rpg::anchor_world(w, puncture);
    let raw = native_v4("VRHandRawWorld", &[Value::Int32(hand)]);
    if !finite(&a) || a[3] < 0.5 || !finite(&raw) || raw[3] < 0.5 {
        clear(s, "tracking lost");
        return;
    }
    let start = position;
    let reach = melee::reach(w) as f32;
    let tip = with_w(add(&start, &mul(&axis, reach)), 1.0);
    let measure = rpg::measure(w, n, &start, &tip, &ZERO);
    let named_depth = if finite(&measure) { measure[0] } else { 0.0 };
    let mut depth = named_depth;
    let edge0 = v4_of(w, "GetWorldForward");
    if !finite(&edge0) {
        clear(s, "no edge");
        return;
    }
    if puncture && depth <= 0.0 {
        let interval = rpg::seat_interval(w, n, &start, &tip, &edge0);
        if finite(&interval) && interval[3] > 0.5 {
            depth = interval[1];
        }
    }
    let basis = bn::tracking_basis();
    if !finite(&basis) || basis[3] < 0.5 {
        clear(s, "tracking basis lost");
        return;
    }
    let (seat_axis, seat_edge) = {
        let r = rc.borrow();
        (r.seat_axis, r.seat_edge)
    };
    let entry_axis = if finite(&seat_axis) && len(&seat_axis) >= 0.9 { seat_axis } else { axis };
    let entry_edge = if finite(&seat_edge) && len(&seat_edge) >= 0.9 { seat_edge } else { edge0 };
    if fresh || s.waiting.is_some() {
        if !continuation {
            if fresh {
                clear(s, "idle");
            }
            if let Some(wt) = &s.waiting {
                if wt.track[3] != tracked[3] || (depth <= 0.0 && dot(&world_motion(&sub(&tracked, &wt.track), &basis), &entry_axis) < -0.025) {
                    clear(s, "released");
                    return;
                }
            }
            if depth <= 0.0 {
                if puncture {
                    if s.waiting.is_none() {
                        s.waiting = Some(Waiting { weapon: w.clone(), serial: n, track: tracked });
                    }
                    s.phase = "awaiting overlap";
                    return;
                }
                s.phase = "outside contact";
                return;
            }
            let last_track = s.waiting.as_ref().map_or(tracked, |wt| wt.track);
            s.seat = Some(Seat {
                weapon: w.clone(),
                serial: n,
                track_at: tracked,
                joint_serial: receipt_joint,
                raw_from_anchor: sub(&raw, &a),
                last_track,
                axial: 0.0,
                last_slide: 0.0,
                input_axis: v(entry_axis[0], entry_axis[1], entry_axis[2]),
                input_edge: v(entry_edge[0], entry_edge[1], entry_edge[2]),
                input_basis: [basis[0], basis[1], basis[2], 1.0],
                backout: 0.0,
                travel_time: 0.0,
                slide_until: 0.09,
                samples: Vec::new(),
                dispatched: false,
                measure_at: None,
                trail_at: None,
                trail_track: None,
                load_at: None,
                retry_at: None,
                pull_minimum: None,
                edge_track: None,
                edge_until: None,
                edge_from: None,
                joint: false,
                resistance: 0.0,
                clock: 0.0,
            });
            s.waiting = None;
            s.binds += 1;
        }
        if let Some(seat) = s.seat.as_mut() {
            seat.serial = n;
            seat.samples.clear();
            seat.dispatched = false;
            seat.measure_at = None;
            seat.trail_at = None;
            seat.load_at = None;
            seat.retry_at = None;
        }
        s.phase = "bound";
    }
    let Some(seat) = s.seat.as_mut() else { return };
    let d = sub(&sub(&raw, &a), &seat.raw_from_anchor);
    if puncture {
        lodge_contact(w, n);
    }
    let tracked_travel = len(&sub(&tracked, &seat.track_at));
    let rail_point = bn::joint_point(hand, seat.joint_serial);
    let rail = finite(&rail_point) && rail_point[3] > 0.5;
    let mut slot_following = false;
    if puncture {
        let goal = rpg::follow_slot(w, hand, n, &start, &tip, &edge0, &tracked);
        if goal[3] < 0.0 {
            clear(s, "released");
            return;
        }
        if rc.borrow().attachment.is_some() && finite(&goal) && goal[3] > 1.5 {
            slot_following = true;
        }
    }
    let anchor = if rail { rail_point } else { a };
    let along = dot(&sub(&anchor, &start), &axis);
    let gap = len(&sub(&anchor, &add(&start, &mul(&axis, along.clamp(0.0, reach)))));
    let on_rail = rail && along >= 0.0 && along <= reach && gap <= 0.10;
    seat.travel_time += dt;
    if puncture {
        // small XR steps accumulated, the withdrawal reset on renewed insertion; animation and the solver add nothing
        let delta = world_motion(&sub(&tracked, &seat.last_track), &seat.input_basis);
        let mut step = dot(&delta, &seat.input_axis);
        seat.last_track = v(tracked[0], tracked[1], tracked[2]);
        if step.abs() < 0.7 * len(&delta) {
            step = 0.0;
        }
        seat.axial += step;
        seat.backout = (seat.backout - step).max(0.0);
        // a deliberate pull keeps the shaft free while the solved prop catches up; only a signed reinsertion cancels it
        if let Some(min) = seat.pull_minimum {
            let min = min.min(seat.axial);
            seat.pull_minimum = Some(min);
            if depth > 0.0 && seat.axial - min >= 0.025 {
                seat.pull_minimum = None;
                seat.backout = 0.0;
            }
        } else if seat.backout >= 0.025 {
            seat.pull_minimum = Some(seat.axial);
        }
        if (seat.axial - seat.last_slide).abs() > 0.004 {
            seat.last_slide = seat.axial;
            seat.slide_until = seat.travel_time + if step < 0.0 { 0.09 } else { 0.025 };
        }
        if seat.pull_minimum.is_none() && depth > 0.0 && seat.travel_time >= seat.slide_until {
            seat.backout = 0.0;
        }
    }
    if depth <= 0.0 && (!on_rail || slot_following) && (!puncture || seat.pull_minimum.is_some()) {
        let actual = world_motion(&sub(&tracked, &seat.track_at), &basis);
        let why = if puncture || (along >= reach && dot(&actual, &axis) < 0.0) { "retracted" } else { "separation" };
        clear(s, why);
        return;
    }
    let Some(seat) = s.seat.as_mut() else { return };
    // a visual sample uses the solved prop and a fresh XR displacement: an idle controller paints no passing target
    if named_depth > 0.0 && tracked_travel > 0.015 && age >= seat.trail_at.unwrap_or(0.0) {
        let last = seat.trail_track.unwrap_or(seat.track_at);
        if len(&sub(&tracked, &last)) > 0.006 {
            seat.trail_at = Some(age + 0.028);
            seat.trail_track = Some(tracked);
            mark::trace_surface(w, n, &start, &with_w(add(&start, &mul(&axis, reach)), 1.0));
        }
    }
    let mut edge = sub(&edge0, &mul(&axis, dot(&edge0, &axis)));
    let edge_length = len(&edge);
    if edge_length < 0.01 {
        clear(s, "no edge");
        return;
    }
    let Some(seat) = s.seat.as_mut() else { return };
    edge = with_w(mul(&edge, 1.0 / edge_length), 0.0);
    seat.resistance = if named_depth > 0.0 {
        measure[3]
    } else if depth > 0.0 || (puncture && rail) {
        1.0
    } else {
        0.0
    };
    let hold_depth = puncture && seat.pull_minimum.is_none() && seat.travel_time >= seat.slide_until;
    if puncture && rail && seat.pull_minimum.is_none() {
        let et = *seat.edge_track.get_or_insert(v(tracked[0], tracked[1], tracked[2]));
        let delta = world_motion(&sub(&tracked, &et), &seat.input_basis);
        let excursion = len(&delta);
        if excursion > 0.002 {
            if dot(&delta, &seat.input_edge) >= 0.96 * excursion {
                if seat.edge_until.is_none_or(|u| seat.travel_time > u) {
                    seat.edge_from = Some(et);
                }
                seat.edge_until = Some(seat.travel_time + 0.065);
            } else {
                seat.edge_until = None;
                seat.edge_from = None;
            }
            seat.edge_track = Some(v(tracked[0], tracked[1], tracked[2]));
        }
    } else {
        seat.edge_track = None;
        seat.edge_until = None;
        seat.edge_from = None;
    }
    let cutting = seat.edge_until.is_some_and(|u| seat.travel_time <= u);
    let mut edge_travel = 0.0f32;
    seat.joint = false;
    if rail && (on_rail || depth > 0.0 || hold_depth || (puncture && seat.backout < 0.004)) {
        if cutting {
            let from = seat.edge_from.unwrap_or(tracked);
            let advance = dot(&world_motion(&sub(&tracked, &from), &seat.input_basis), &seat.input_edge);
            let command = advance.clamp(0.0, 0.35);
            seat.joint = bn::drive_edge(hand, seat.joint_serial, &p3(&local_end(w)), command, seat.resistance);
            let path = bn::edge_path(hand, seat.joint_serial, true);
            if path.len() == 3 && path[0][3] > 0.0 {
                edge_travel = dot(&sub(&path[1], &path[0]), &path[2]);
            }
        } else if hold_depth {
            seat.joint = bn::joint_load(hand, seat.joint_serial, seat.resistance, true, false);
        } else if puncture && seat.pull_minimum.is_none() && seat.backout < 0.004 {
            seat.joint = bn::joint_load(hand, seat.joint_serial, seat.resistance, false, true);
        } else {
            seat.joint = bn::joint_load(hand, seat.joint_serial, seat.resistance, false, false);
        }
    }
    if depth > 0.0 && seat.pull_minimum.is_none() && !seat.joint && seat.travel_time >= seat.measure_at.unwrap_or(0.0) {
        seat.measure_at = Some(seat.travel_time + 0.025);
        seat.joint = rpg::update_joint(w, hand, n, &start, &tip, &edge);
    }
    if puncture && depth <= 0.0 {
        s.phase = "recovering contact";
    } else if s.phase == "recovering contact" {
        s.phase = "bound";
    }
    let Some(seat) = s.seat.as_mut() else { return };
    let resistance = seat.resistance;
    let yield_ = if resistance > 0.0 { 1.0 / (1.0 + resistance) } else { 0.0 };
    let mut load = sub(&d, &mul(&edge, dot(&d, &edge).max(0.0) * yield_));
    // the shaft free to back out with no dynamic body to hold it
    if !puncture || seat.pull_minimum.is_some() || seat.backout > 0.004 {
        load = sub(&load, &mul(&axis, dot(&load, &axis).min(0.0)));
    }
    let load_length = len(&load);
    let stiffness = if resistance > 0.0 { resistance / (resistance + 0.35) } else { 0.72 };
    let mut amount = ((load_length - 0.006) * stiffness).clamp(0.0, 0.080) * ((age / 0.075).min(1.0) as f32);
    let mut offset = if load_length > 0.0001 { mul(&load, -amount / load_length) } else { ZERO };
    if seat.joint {
        offset = ZERO;
        amount = 0.0;
    }
    let model = native_v4("VRWorldDirToModel", &[Value::Ptr(offset.as_ptr().cast())]);
    if !finite(&model) || model[3] < 0.5 {
        clear(s, "invalid transform");
        return;
    }
    s.sent = pose_offset(hand, [model[0], model[1], model[2], 1.0]);
    let sent = s.sent;
    let Some(seat) = s.seat.as_mut() else { return };
    // only an already simulated ragdoll takes this feedback: an animated person is never switched to show it
    if named_depth > 0.0 && !seat.joint && sent && amount > 0.006 && age >= seat.load_at.unwrap_or(0.05) {
        seat.load_at = Some(age + 0.05);
        rpg::load_ragdoll(w, n, offset);
    }
    // the input is displacement relative to the target, not the stopped hand, the camera or the person walking past;
    // +Y is the item's authored edge. XR, animation and the script publish apart: both streams over one short window
    let relative = sub(&raw, &a);
    let at = seat.clock + dt;
    seat.clock = at;
    let mut score = 0.0f32;
    let mut motion: Option<V4> = None;
    let mut before = false;
    let joint_now = seat.joint;
    let gesture_basis = if puncture { seat.input_basis } else { basis };
    let gesture_edge = if puncture { seat.input_edge } else { edge };
    seat.samples.retain(|o| at - o.at <= 0.15);
    for old in seat.samples.iter().rev() {
        let r_ = sub(&relative, &old.rel);
        let hand_motion = world_motion(&sub(&tracked, &old.track), &gesture_basis);
        let projected = dot(&r_, &edge);
        let hand_edge = dot(&hand_motion, &gesture_edge);
        // the solver can carry the target with the wrist: on the same live joint, signed XR travel is the input
        let coupled = old.joint && joint_now;
        if hand_edge > 0.0 && hand_edge * hand_edge >= dot(&hand_motion, &hand_motion) * 0.9216 && (coupled || (projected > 0.0 && projected * projected >= dot(&r_, &r_) * 0.9216)) {
            let candidate = if coupled { hand_edge } else { hand_edge.min(projected) };
            if candidate > score {
                score = candidate;
                motion = Some(hand_motion);
                before = true;
            }
        }
    }
    if seat.samples.len() >= 24 {
        seat.samples.remove(0);
    }
    seat.samples.push(Sample { at, rel: relative, track: tracked, joint: joint_now, start: with_w(start, 1.0), tip: with_w(tip, 1.0) });
    let _ = seat.samples.last().map(|x| (x.start, x.tip));
    let can_cut = (puncture && depth > 0.0 && cutting && seat.joint && edge_travel >= 0.008) || (!puncture && named_depth > 0.0);
    if age >= 0.09 && can_cut && resistance > 0.0 && score > 0.045 && age >= seat.retry_at.unwrap_or(0.0) && before {
        seat.retry_at = Some(age + 0.095);
        let dispatched = rpg::try_follow_through(pl, w, n, &start, &tip, motion.unwrap_or(ZERO));
        if dispatched {
            if let Some(seat) = s.seat.as_mut() {
                seat.dispatched = true;
                seat.samples.clear();
            }
            s.requests += 1;
            s.phase = "requested";
        }
    }
}
