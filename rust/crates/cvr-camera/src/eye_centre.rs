//! The eye-to-centre ledger (formerly include/Camera/EyeCentreLedger.hpp): the last 32 published eye positions in the
//! engine's fixed point (1/131072 m) with their optical centre and body base, looked up by an exact eye, or by the
//! float metres Lua sees -- accepting only the representation's own uncertainty (one float ULP, at most 0.25 mm).
use std::sync::Mutex;

pub type Position = [i32; 3];
pub type WorldPosition = [f32; 3];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Record {
    eye: Position,
    centre: Position,
    body_base: Position,
}

#[derive(Default)]
struct State {
    records: [Record; 32],
    next: u64,
}

#[derive(Default)]
pub struct EyeCentreLedger {
    state: Mutex<State>,
}

pub fn world(p: Position) -> WorldPosition {
    [p[0] as f32 / 131072.0, p[1] as f32 / 131072.0, p[2] as f32 / 131072.0]
}

fn same_body_offset(a: &Record, b: &Record) -> bool {
    (0..3).all(|k| {
        ((a.centre[k] as i64 - a.body_base[k] as i64) - (b.centre[k] as i64 - b.body_base[k] as i64)).abs() <= 1
    })
}

fn same_float_centre(a: &WorldPosition, b: &WorldPosition, query: &WorldPosition) -> bool {
    for k in 0..3 {
        if a[k] == b[k] {
            continue;
        }
        let x = query[k].abs();
        let ulp = x.next_up() - x;
        if !ulp.is_finite() || (a[k] - b[k]).abs() > cvr_body::cmath::min(ulp, 0.00025) {
            return false;
        }
    }
    true
}

impl EyeCentreLedger {
    pub fn publish(&self, eye: Position, centre: Position, body_base: Position) {
        let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let i = (s.next % 32) as usize;
        s.records[i] = Record { eye, centre, body_base };
        s.next += 1;
    }

    pub fn read(&self, eye: Position) -> Option<(Position, Position)> {
        let s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let n = s.next.min(32);
        for age in 1..=n {
            let r = &s.records[((s.next - age) % 32) as usize];
            if r.eye == eye {
                return Some((r.centre, r.body_base));
            }
        }
        None
    }

    /// (centre, body base) for an eye or centre given in float metres; None when absent or ambiguous.
    pub fn read_world(&self, eye_or_centre: WorldPosition, want_base: bool) -> Option<(WorldPosition, WorldPosition)> {
        let s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let n = s.next.min(32);
        let mut found = false;
        let (mut candidate, mut candidate_base) = ([0.0f32; 3], [0.0f32; 3]);
        let mut candidate_record = Record::default();
        for age in 1..=n {
            let r = &s.records[((s.next - age) % 32) as usize];
            let (eye, head) = (world(r.eye), world(r.centre));
            if eye_or_centre != eye && eye_or_centre != head {
                continue;
            }
            if found && !same_float_centre(&candidate, &head, &eye_or_centre) {
                return None;
            }
            let base = world(r.body_base);
            if found && want_base && candidate_base != base && !same_body_offset(&candidate_record, r) {
                return None;
            }
            if !found {
                candidate = head;
                candidate_base = base;
                candidate_record = *r;
                found = true;
            }
        }
        found.then_some((candidate, candidate_base))
    }
}
