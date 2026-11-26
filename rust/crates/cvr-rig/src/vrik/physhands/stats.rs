//! The drawn hand frame to frame (animation thread, for the statistics): how far it leaves the line of its last two
//! frames (a hand moving steadily scores nothing; a jump scores its size), the same for tracking's own target as the
//! yardstick, and the drawn hand's offset from tracking frame to frame (in the air it is nil and stays so, on a wall it
//! grows and shrinks smoothly).
use super::math::qrot;
use super::qpc;
use core::sync::atomic::{AtomicU64, Ordering::*};
use std::cell::RefCell;

const Z: AtomicU64 = AtomicU64::new(0);
pub static DRAWN_JERK_UM: [AtomicU64; 2] = [Z; 2];
pub static DRAWN_JERK_MOST_UM: [AtomicU64; 2] = [Z; 2];
pub static DRAWN_JERK_N: [AtomicU64; 2] = [Z; 2];
pub static DRAWN_JUMPS: [AtomicU64; 2] = [Z; 2];
pub static TRACK_JERK_UM: [AtomicU64; 2] = [Z; 2];
pub static TRACK_JERK_MOST_UM: [AtomicU64; 2] = [Z; 2];
pub static TRACK_JERK_N: [AtomicU64; 2] = [Z; 2];
pub static SLIP_UM: [AtomicU64; 2] = [Z; 2];
pub static SLIP_MOST_UM: [AtomicU64; 2] = [Z; 2];
pub static SLIP_N: [AtomicU64; 2] = [Z; 2];
pub static SLIP_JUMPS: [AtomicU64; 2] = [Z; 2];
pub static AIR_SLIP_UM: [AtomicU64; 2] = [Z; 2];
pub static AIR_SLIP_N: [AtomicU64; 2] = [Z; 2];
/// tracking's place last frame, the game's world (the trace's)
pub static mut last_track: [[f32; 3]; 2] = [[0.0; 3]; 2];

/// NowS: seconds by the performance counter.
pub fn now_s() -> f64 {
    let (t, f) = qpc();
    t as f64 / f as f64
}

/// A point's track frame to frame.
#[derive(Clone, Copy, Default)]
pub struct Track {
    t1: f64,
    t2: f64,
    p1: [f32; 3],
    p2: [f32; 3],
    have: i32,
}
/// OffItsLine: how far this frame's point is off the line through the two before it, carried by the time between
/// (frames come unevenly; a point moving steadily scores nothing however they come); negative when there is none.
pub fn off_its_line(tr: &mut Track, world: &[f32; 3], now: f64) -> f32 {
    let mut off = -1.0f32;
    if now - tr.t1 < 0.004 {
        return -1.0; // a second solve of the same frame
    }
    if now - tr.t1 > 0.1 {
        tr.have = 0;
    }
    if tr.have >= 2 && tr.t1 > tr.t2 {
        let k = (now - tr.t1) / (tr.t1 - tr.t2);
        let mut d = 0.0f32;
        for i in 0..3 {
            let e = world[i] - (tr.p1[i] + (tr.p1[i] - tr.p2[i]) * k as f32);
            d += e * e;
        }
        off = d.sqrt();
    }
    tr.p2 = tr.p1;
    tr.p1 = *world;
    tr.t2 = tr.t1;
    tr.t1 = now;
    if tr.have < 2 {
        tr.have += 1;
    }
    off
}
fn tally(sum: &AtomicU64, most: &AtomicU64, n: &AtomicU64, off: f32) {
    let um = (off * 1e6) as u64;
    sum.fetch_add(um, Relaxed);
    n.fetch_add(1, Relaxed);
    most.fetch_max(um, Relaxed);
}
thread_local! {
    static DRAWN_TRACK: RefCell<[Track; 2]> = RefCell::new([Track::default(); 2]);
    static TRACKING_TRACK: RefCell<[Track; 2]> = RefCell::new([Track::default(); 2]);
    static SLIP_LAST: RefCell<([[f32; 3]; 2], [f64; 2])> = const { RefCell::new(([[0.0; 3]; 2], [0.0; 2])) };
}
pub fn note_drawn(side: usize, world: &[f32; 3]) {
    let off = DRAWN_TRACK.with(|t| off_its_line(&mut t.borrow_mut()[side], world, now_s()));
    if off < 0.0 {
        return;
    }
    tally(&DRAWN_JERK_UM[side], &DRAWN_JERK_MOST_UM[side], &DRAWN_JERK_N[side], off);
    if off > 0.003 {
        DRAWN_JUMPS[side].fetch_add(1, Relaxed);
    }
}
pub fn note_slip(side: usize, drawn: &[f32; 3], track: &[f32; 3], in_air: bool) {
    let now = now_s();
    let off = SLIP_LAST.with(|s| {
        let (last, last_at) = &mut *s.borrow_mut();
        if now - last_at[side] < 0.004 {
            return None; // a second solve of the same frame
        }
        let e = [drawn[0] - track[0], drawn[1] - track[1], drawn[2] - track[2]];
        let d2: f32 = (0..3).map(|k| (e[k] - last[side][k]) * (e[k] - last[side][k])).sum();
        let have = now - last_at[side] < 0.1;
        last[side] = e;
        last_at[side] = now;
        have.then(|| d2.sqrt())
    });
    let Some(off) = off else { return };
    tally(&SLIP_UM[side], &SLIP_MOST_UM[side], &SLIP_N[side], off);
    if off > 0.003 {
        SLIP_JUMPS[side].fetch_add(1, Relaxed);
    }
    if in_air {
        AIR_SLIP_UM[side].fetch_add((off * 1e6) as u64, Relaxed);
        AIR_SLIP_N[side].fetch_add(1, Relaxed);
    }
}
/// NoteTracking: tracking's target in the game's world, for the trace and the yardstick.
pub fn note_tracking(side: usize, model_pos: &[f32; 3]) {
    let (entity_position, entity_rotation) = super::pose::pose_entity_now();
    let r = qrot(&entity_rotation, model_pos);
    let w = [r[0] + entity_position[0], r[1] + entity_position[1], r[2] + entity_position[2]];
    // SAFETY: the animation thread's own copy.
    unsafe { last_track[side] = w };
    let off = TRACKING_TRACK.with(|t| off_its_line(&mut t.borrow_mut()[side], &w, now_s()));
    if off >= 0.0 {
        tally(&TRACK_JERK_UM[side], &TRACK_JERK_MOST_UM[side], &TRACK_JERK_N[side], off);
    }
}
