//! THE TRACES (trace=1): each drawn frame -- the drawn place, the body's as drawn and as PhysX has it, its velocity,
//! what it was carried by, tracking's target, the touch -- through a ring the worker empties into
//! log/physhands_trace.csv; and the final avatar wrist with the exact NPC publication its solve consumed, into
//! log/npc_grip_trace.csv (independently queried NPC and hand snapshots can differ simply because another animation job
//! ran between them).
use super::fingers::{SHOWN_BENDS, SHOWN_DEPTH};
use super::grab::read_person_attachment;
use super::math::qmul;
use super::stats::{last_track, now_s};
use super::tick_ms;
use super::types::{DrawnHand, MotorProbe, Offset, PoseInputs, Step, TouchInfo};
use core::sync::atomic::{AtomicU32, Ordering::*};
use cvr_host::paths::{file, Folder};
use cvr_physhands::person::PersonAttachment;
use std::io::Write as _;
use std::sync::Mutex;

#[derive(Clone, Copy, Default)]
pub struct TraceRow {
    t: f64,
    side: i32,
    drawn: [f32; 3],
    body: [f32; 3],
    raw: [f32; 3],
    vel: [f32; 3],
    lead: f32,
    track: [f32; 3],
    touching: u8,
    on_world: u8,
    /// the drawn hand's turn and its body's, world
    rot: [f32; 4],
    body_rot: [f32; 4],
    /// tracking's turn, world
    track_rot: [f32; 4],
    /// the latch, how much of tracking's move is added, each finger's bends
    pressed: u8,
    move_w: f32,
    bend: [f32; 5],
    /// each finger as drawn, how deep in what is near (m)
    inside: [f32; 5],
    /// the body's shapes where the hand is drawn: the skin's deepest (m)
    drawn_in: f32,
    /// the last step's drive: what the touch test met (kind, owner, index, depth), its flags, the turn to the target,
    /// the spin asked and given, and the spin PhysX left the body with
    touch: TouchInfo,
    drive_flags: u32,
    angle: f32,
    asked_spin: [f32; 3],
    given_spin: [f32; 3],
    spin: [f32; 3],
    /// the entity the frame draws in (place, turn), the step's entity, whether the step rode the body along
    entity: [f32; 3],
    entity_rot: [f32; 4],
    step_entity: [f32; 3],
    riding: u8,
}
const TRACE_RING: usize = 8192;
static TRACE: Mutex<Vec<TraceRow>> = Mutex::new(Vec::new());
/// trace=1: the body's shapes where the hand is drawn, the skin's deepest in what is near (f32 bits)
pub static DRAWN_IN: [AtomicU32; 2] = [AtomicU32::new(0), AtomicU32::new(0)];

extern "C" {
    static CyberpunkVR_PlayerEntityQuat: [f32; 4];
}
fn positive(mut q: [f32; 4]) -> [f32; 4] {
    if q[3] < 0.0 {
        for c in &mut q {
            *c = -*c;
        }
    }
    q
}
/// TraceFrame: one drawn frame into the ring (the animation thread), dropped when the ring is full.
pub fn trace_frame(side: usize, at: &[f32; 3], o: &Offset, lead: f32, model_rot: Option<&[f32; 4]>, move_w: f32, track_model_rot: Option<&[f32; 4]>) {
    // SAFETY: the exported quaternion, read.
    let entity = unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_PlayerEntityQuat) };
    let mut r = TraceRow {
        t: now_s(),
        side: side as i32,
        drawn: *at,
        body: o.body,
        raw: o.raw,
        vel: o.vel,
        // SAFETY: the animation thread's own copy.
        track: unsafe { last_track[side] },
        lead,
        touching: o.touching,
        on_world: o.on_world,
        rot: positive(model_rot.map_or([0.0, 0.0, 0.0, 1.0], |m| qmul(&entity, m))),
        body_rot: positive(o.body_rot),
        track_rot: positive(track_model_rot.map_or([0.0, 0.0, 0.0, 1.0], |m| qmul(&entity, m))),
        pressed: u8::from(o.pressed),
        move_w,
        drawn_in: f32::from_bits(DRAWN_IN[side].load(Relaxed)),
        ..Default::default()
    };
    if let Some(st) = super::worker::STEPS.read().filter(|st: &Step| st.valid[side]) {
        let d = &st.motor[side];
        r.touch = d.touch;
        r.drive_flags = d.flags;
        r.angle = d.angle;
        r.asked_spin = d.asked_spin;
        r.given_spin = d.given_spin;
        r.spin = st.spin[side];
    }
    let (ep, eq) = super::pose::pose_entity_now();
    r.entity = ep;
    r.entity_rot = eq;
    r.step_entity = o.sent_entity;
    r.riding = u8::from(o.riding);
    for f in 0..5 {
        r.bend[f] = (0..3).map(|j| f32::from_bits(SHOWN_BENDS[side][f][j].load(Relaxed))).sum();
        r.inside[f] = f32::from_bits(SHOWN_DEPTH[side][f].load(Relaxed));
    }
    let mut ring = TRACE.lock().unwrap_or_else(|e| e.into_inner());
    if ring.len() < TRACE_RING {
        ring.push(r);
    }
}
static TRACE_FILE: Mutex<Option<std::fs::File>> = Mutex::new(None);
/// The trace, appended (the worker): the rows the ring holds, the file closed once trace is off and the ring empty.
pub fn flush_trace(enabled: bool) {
    let rows: Vec<TraceRow> = core::mem::take(&mut *TRACE.lock().unwrap_or_else(|e| e.into_inner()));
    let mut f = TRACE_FILE.lock().unwrap_or_else(|e| e.into_inner());
    if rows.is_empty() {
        if f.is_some() && !enabled {
            *f = None;
        }
        return;
    }
    if f.is_none() {
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(file(Folder::Log, "physhands_trace.csv")) {
            let _ = file.write_all(
                b"t,side,drawn_x,drawn_y,drawn_z,body_x,body_y,body_z,raw_x,raw_y,raw_z,vel_x,vel_y,vel_z,move,track_x,track_y,track_z,touching,on_world,rot_x,rot_y,rot_z,rot_w,brot_x,brot_y,brot_z,brot_w,pressed,move_w,bend_0,bend_1,bend_2,bend_3,bend_4,in_0,in_1,in_2,in_3,in_4,trot_x,trot_y,trot_z,trot_w,drawn_in,tkind,towner,tindex,tdepth,dflags,angle,ask_wx,ask_wy,ask_wz,give_wx,give_wy,give_wz,spin_x,spin_y,spin_z,en_x,en_y,en_z,eq_x,eq_y,eq_z,eq_w,se_x,se_y,se_z,riding\r\n",
            );
            *f = Some(file);
        }
    }
    if let Some(file) = f.as_mut() {
        let mut text = String::with_capacity(rows.len() * 300);
        for r in &rows {
            text.push_str(&format!(
                "{:.5},{},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.3},{:.3},{:.3},{:.4},{:.4},{:.4},{:.4},{},{},{:.5},{:.5},{:.5},{:.5},{:.5},{:.5},{:.5},{:.5},{},{:.3},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.5},{:.5},{:.5},{:.5},{:.2},{},{},{},{:.2},{},{:.4},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.4},{:.4},{:.4},{:.5},{:.5},{:.5},{:.5},{:.4},{:.4},{:.4},{}\r\n",
                r.t, r.side, r.drawn[0], r.drawn[1], r.drawn[2], r.body[0], r.body[1], r.body[2], r.raw[0], r.raw[1], r.raw[2], r.vel[0], r.vel[1], r.vel[2], r.lead, r.track[0], r.track[1], r.track[2],
                r.touching, r.on_world, r.rot[0], r.rot[1], r.rot[2], r.rot[3], r.body_rot[0], r.body_rot[1], r.body_rot[2], r.body_rot[3], r.pressed, r.move_w, r.bend[0], r.bend[1], r.bend[2],
                r.bend[3], r.bend[4], 1000.0 * r.inside[0], 1000.0 * r.inside[1], 1000.0 * r.inside[2], 1000.0 * r.inside[3], 1000.0 * r.inside[4], r.track_rot[0], r.track_rot[1], r.track_rot[2],
                r.track_rot[3], 1000.0 * r.drawn_in, r.touch.kind, r.touch.owner, r.touch.index, 1000.0 * r.touch.depth, r.drive_flags, r.angle,
                r.asked_spin[0], r.asked_spin[1], r.asked_spin[2], r.given_spin[0], r.given_spin[1], r.given_spin[2], r.spin[0], r.spin[1], r.spin[2],
                r.entity[0], r.entity[1], r.entity[2], r.entity_rot[0], r.entity_rot[1], r.entity_rot[2], r.entity_rot[3], r.step_entity[0], r.step_entity[1], r.step_entity[2], r.riding
            ));
        }
        let _ = file.write_all(text.as_bytes());
        let _ = file.flush();
    }
}

/// Unlike the body trace above, this records the FINAL avatar FK wrist and the exact NPC publication consumed by that
/// solve.
#[derive(Clone, Copy, Default)]
struct GripTraceRow {
    at: u64,
    avatar_at: u64,
    physics_at: u64,
    physics_serial: u64,
    physics_grip: u64,
    side: i32,
    weight: f32,
    physical: bool,
    drawn: DrawnHand,
    applied: PersonAttachment,
    latest: PersonAttachment,
    /// quaternion, then world position
    body: [f32; 7],
    motor: [f32; 7],
    drive: MotorProbe,
    actual_spin: [f32; 3],
}
const GRIP_TRACE_RING: usize = 2048;
struct GripTrace {
    rows: std::collections::VecDeque<GripTraceRow>,
    dropped: u64,
}
static GRIP_TRACE: Mutex<GripTrace> = Mutex::new(GripTrace { rows: std::collections::VecDeque::new(), dropped: 0 });
/// TraceFinalGrip: the completed wrist and the NPC grip it consumed, when trace=1 and a grip is on.
pub fn trace_final_grip(side: usize, pose: &PoseInputs) {
    if super::settings::read().is_none_or(|s| s.trace == 0) {
        return;
    }
    let applied = pose.applied_grip[side];
    let latest = read_person_attachment(side).unwrap_or_default();
    if !applied.on && !latest.on {
        return;
    }
    let mut row = GripTraceRow { applied, latest, at: tick_ms(), avatar_at: pose.at, side: side as i32, drawn: pose.drawn[side], weight: pose.applied_grip_weight[side], ..Default::default() };
    if let Some(physical) = super::worker::STEPS.read().filter(|st: &Step| st.valid[side]) {
        row.physical = true;
        row.physics_at = physical.at;
        row.physics_serial = physical.serial;
        row.physics_grip = physical.grip_revision[side];
        row.drive = physical.motor[side];
        row.actual_spin = physical.spin[side];
        row.body = physical.pose[side];
        row.motor = physical.sent[side];
        for k in 0..3 {
            row.body[k + 4] += physical.origin[k];
            row.motor[k + 4] += physical.origin[k];
        }
    }
    let mut g = GRIP_TRACE.lock().unwrap_or_else(|e| e.into_inner());
    if g.rows.len() == GRIP_TRACE_RING {
        g.dropped += 1;
        return;
    }
    g.rows.push_back(row);
}
static GRIP_FILE: Mutex<Option<std::fs::File>> = Mutex::new(None);
/// FlushGripTrace: the grip rows appended (the worker); the file closed once the trace is off.
pub fn flush_grip_trace(enabled: bool) {
    let mut file_g = GRIP_FILE.lock().unwrap_or_else(|e| e.into_inner());
    loop {
        let (row, dropped) = {
            let mut g = GRIP_TRACE.lock().unwrap_or_else(|e| e.into_inner());
            match g.rows.pop_front() {
                Some(r) => (r, g.dropped),
                None => break,
            }
        };
        if file_g.is_none() {
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(file(Folder::Log, "npc_grip_trace.csv")) {
                let _ = f.write_all(b"at,avatar_at,side,weight,owner,serial,used_rev,used_at,control_at,latest_rev,latest_at,physics_at,physics_serial,physics_grip,physical,dropped,hand_x,hand_y,hand_z,hand_qx,hand_qy,hand_qz,hand_qw,used_x,used_y,used_z,used_qx,used_qy,used_qz,used_qw,latest_x,latest_y,latest_z,latest_qx,latest_qy,latest_qz,latest_qw,body_qx,body_qy,body_qz,body_qw,body_x,body_y,body_z,motor_qx,motor_qy,motor_qz,motor_qw,motor_x,motor_y,motor_z,drive_flags,touch_kind,touch_owner,touch_index,touch_mask,touch_depth,drive_dt,drive_angle,drive_gain,drive_budget,pre_qx,pre_qy,pre_qz,pre_qw,pre_wx,pre_wy,pre_wz,raw_wx,raw_wy,raw_wz,ask_wx,ask_wy,ask_wz,give_wx,give_wy,give_wz,actual_wx,actual_wy,actual_wz\r\n");
                *file_g = Some(f);
            }
        }
        let Some(f) = file_g.as_mut() else { continue };
        let (a, n) = (&row.applied, &row.latest);
        let mut s = format!(
            "{},{},{},{:.7},{},{},{},{},{},{},{},{},{},{},{},{}",
            row.at, row.avatar_at, row.side, row.weight, a.owner, a.serial, a.revision, a.at, a.control_at, n.revision, n.at, row.physics_at, row.physics_serial, row.physics_grip, i32::from(row.physical), dropped
        );
        let mut values = |v: &[f32]| {
            for x in v {
                s.push_str(&format!(",{}", cvr_physhands_g9(*x)));
            }
        };
        values(&row.drawn.p);
        values(&row.drawn.q);
        values(&a.position);
        values(&a.rotation);
        values(&n.position);
        values(&n.rotation);
        values(&row.body);
        values(&row.motor);
        let d = &row.drive;
        s.push_str(&format!(
            ",{},{},{},{},{},{},{},{},{},{}",
            d.flags,
            d.touch.kind,
            d.touch.owner,
            d.touch.index,
            d.touch.simulates,
            cvr_physhands_g9(d.touch.depth),
            cvr_physhands_g9(d.dt),
            cvr_physhands_g9(d.angle),
            cvr_physhands_g9(d.gain),
            cvr_physhands_g9(d.budget)
        ));
        for v in [&d.before[..], &d.before_spin[..], &d.raw_spin[..], &d.asked_spin[..], &d.given_spin[..], &row.actual_spin[..]] {
            for x in v {
                s.push_str(&format!(",{}", cvr_physhands_g9(*x)));
            }
        }
        s.push_str("\r\n");
        let _ = f.write_all(s.as_bytes());
    }
    if let Some(f) = file_g.as_mut() {
        let _ = f.flush();
        if !enabled {
            *file_g = None;
        }
    }
}
/// printf's %.9g: nine significant digits, the shorter of the fixed and exponent forms, trailing zeros dropped.
pub fn cvr_physhands_g9(x: f32) -> String {
    let x = f64::from(x);
    if x == 0.0 || !x.is_finite() {
        return if x.is_nan() {
            "nan".into()
        } else if x.is_infinite() {
            if x > 0.0 { "inf".into() } else { "-inf".into() }
        } else {
            "0".into()
        };
    }
    let exp = x.abs().log10().floor() as i32;
    if !(-5..9).contains(&exp) {
        let s = format!("{:.8e}", x);
        let (m, e) = s.split_once('e').unwrap_or((&s, "0"));
        let m = m.trim_end_matches('0').trim_end_matches('.');
        let e: i32 = e.parse().unwrap_or(0);
        return format!("{}e{}{:02}", m, if e < 0 { '-' } else { '+' }, e.abs());
    }
    let decimals = (8 - exp).max(0) as usize;
    let s = format!("{:.*}", decimals, x);
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}
