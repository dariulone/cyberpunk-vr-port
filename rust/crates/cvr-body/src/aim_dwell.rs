//! The barrel's dwell on an NPC (main's mods/cet/CyberpunkVRPort_Weapon/aim_dwell.lua): each physical hand earns its
//! own; only mature observations -- two seconds of continuous, tracked aim by the same weapon from the same tracking
//! origin -- are merged by NPC. `T` is what the caller notifies (the NPC's handle).
use std::vec::Vec;

/// The tracking sample's stamp wraps at 2^24 milliseconds (VRMeleeInputFrame publishes it modulo 2^24).
const STAMP_MODULUS: f64 = 16_777_216.0;

/// One hand's observation of a barrel on an NPC.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit<T> {
    /// The NPC's entity hash.
    pub id: u64,
    pub target: T,
    /// The weapon's entity hash.
    pub weapon: u64,
    /// The hand's tracking origin (the frame's w) and the attachment slot the weapon was found on.
    pub origin: f32,
    pub slot_side: i32,
    pub since: f64,
    pub tracked_ms: f64,
    pub mature: bool,
}
impl<T> Hit<T> {
    pub fn new(id: u64, target: T, weapon: u64, origin: f32, slot_side: i32) -> Self {
        Hit { id, target, weapon, origin, slot_side, since: 0.0, tracked_ms: 0.0, mature: false }
    }
}

/// The dwell's session.
#[derive(Clone, Debug)]
pub struct Dwell<T> {
    /// By hand side (0 left, 1 right).
    pub candidates: [Option<Hit<T>>; 2],
    /// The NPCs whose dwell matured, by id.
    pub active: Vec<(u64, T)>,
    last: Option<f64>,
    stamp: Option<f64>,
    last_sample_at: Option<f64>,
}
impl<T: Clone> Default for Dwell<T> {
    fn default() -> Self {
        Dwell { candidates: [None, None], active: Vec::new(), last: None, stamp: None, last_sample_at: None }
    }
}
impl<T: Clone> Dwell<T> {
    /// M.clear: everything forgotten; the NPCs that were active, to be told it stopped.
    pub fn clear(&mut self) -> Vec<(u64, T)> {
        let stopped = core::mem::take(&mut self.active);
        *self = Dwell::default();
        stopped
    }
    pub fn last(&self) -> Option<f64> {
        self.last
    }
    pub fn stamp(&self) -> Option<f64> {
        self.stamp
    }
    pub fn has_candidates(&self) -> bool {
        self.candidates.iter().any(Option::is_some)
    }
    /// M.step: this update's observations (by hand) at `now` (s) with the tracking sample's `stamp`; (the active
    /// NPCs, those that stopped).
    pub fn step(&mut self, now: f64, stamp: Option<f64>, mut observations: [Option<Hit<T>>; 2]) -> (Vec<(u64, T)>, Vec<(u64, T)>) {
        let previous_active = core::mem::take(&mut self.active);
        let continuous = self.last.is_some_and(|l| now >= l && now - l <= 0.30);
        let stale = self.last_sample_at.is_some_and(|at| now - at > 0.25);
        if stamp.is_none() || (stamp == self.stamp && stale) {
            observations = [None, None];
        }
        if stamp != self.stamp || !continuous {
            self.last_sample_at = Some(now);
        }
        let mut active: Vec<(u64, T)> = Vec::new();
        for (side, slot) in observations.iter_mut().enumerate() {
            let Some(hit) = slot.as_mut() else { continue };
            hit.since = now;
            hit.tracked_ms = 0.0;
            let prev = if continuous && !stale { self.candidates[side].as_ref() } else { None };
            if let (Some(prev), Some(stamp), Some(previous_stamp)) = (prev, stamp, self.stamp) {
                if prev.weapon == hit.weapon && prev.slot_side == hit.slot_side && prev.id == hit.id && prev.origin == hit.origin {
                    let elapsed_ms = (stamp - previous_stamp).rem_euclid(STAMP_MODULUS);
                    if elapsed_ms <= 250.0 {
                        hit.since = prev.since;
                        hit.tracked_ms = (prev.tracked_ms + elapsed_ms).min(2000.0);
                        hit.mature = prev.mature || (elapsed_ms > 0.0 && now - hit.since >= 2.0 && hit.tracked_ms >= 2000.0);
                    }
                }
            }
            if hit.mature && !active.iter().any(|(id, _)| *id == hit.id) {
                active.push((hit.id, hit.target.clone()));
            }
        }
        let stopped: Vec<(u64, T)> = previous_active.into_iter().filter(|(id, _)| !active.iter().any(|(a, _)| a == id)).collect();
        self.candidates = observations;
        self.active = active.clone();
        self.last = Some(now);
        self.stamp = stamp;
        (active, stopped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(id: u64) -> [Option<Hit<u64>>; 2] {
        [None, Some(Hit::new(id, id, 7, 1.0, 1))]
    }

    #[test]
    fn two_seconds_of_tracked_aim_mature() {
        let mut d: Dwell<u64> = Dwell::default();
        let (mut t, mut stamp) = (10.0, 1000.0);
        let mut matured_at = None;
        for _ in 0..40 {
            let (active, stopped) = d.step(t, Some(stamp), hit(42));
            assert!(stopped.is_empty());
            if !active.is_empty() && matured_at.is_none() {
                matured_at = Some(t);
            }
            t += 0.1;
            stamp += 100.0;
        }
        let m = matured_at.expect("matured");
        assert!((m - 12.0).abs() < 0.11, "{m}");
        // the barrel leaves: stopped once
        let (active, stopped) = d.step(t, Some(stamp), [None, None]);
        assert!(active.is_empty());
        assert_eq!(stopped, std::vec![(42, 42)]);
    }

    #[test]
    fn a_frozen_sample_or_another_weapon_restarts() {
        let mut d: Dwell<u64> = Dwell::default();
        let (mut t, stamp) = (0.0, 500.0);
        // the same stamp while time passes: stale after 0.25 s, the observations dropped
        for _ in 0..30 {
            let (active, _) = d.step(t, Some(stamp), hit(1));
            assert!(active.is_empty());
            t += 0.1;
        }
        // another weapon in between restarts the count
        let mut d: Dwell<u64> = Dwell::default();
        let (mut t, mut s) = (0.0, 0.0);
        for i in 0..40 {
            let mut o = hit(1);
            if i == 15 {
                o[1].as_mut().unwrap().weapon = 8;
            }
            let (active, _) = d.step(t, Some(s), o);
            assert!(active.is_empty() || i >= 36, "matured too early at {i}");
            t += 0.1;
            s += 100.0;
        }
        // the stamp wraps at 2^24
        let mut d: Dwell<u64> = Dwell::default();
        let (mut t, mut s) = (0.0, STAMP_MODULUS - 1000.0);
        let mut matured = false;
        for _ in 0..25 {
            matured |= !d.step(t, Some(s), hit(3)).0.is_empty();
            t += 0.1;
            s = (s + 100.0) % STAMP_MODULUS;
        }
        assert!(matured);
        assert_eq!(d.clear(), std::vec![(3, 3)]);
    }
}
