//! include/Natives/HeldGripContact.hpp: one physics step of the world scene, prepared and fetched once, and the held
//! point's distance from where the hand has it.
use crate::held_grip;

pub const INVALID: u64 = u64::MAX;
pub const MAXIMUM_MICROMETRES: u64 = 1_000_000_000_000;

/// A fetched step: its scene, its generation, and whether its preparation went through.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Fetch {
    pub scene: usize,
    pub generation: u64,
    pub valid: bool,
}
/// The world scene's step between BeforeSimulate and AfterFetch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepGate {
    pub scene: usize,
    pub generation: u64,
    pub prepared: bool,
    pub consumed: bool,
}
impl Default for StepGate {
    fn default() -> StepGate {
        StepGate { scene: 0, generation: 0, prepared: false, consumed: true }
    }
}
impl StepGate {
    /// Run before BeforeSimulate's early returns. Other scenes must not invalidate an outstanding world step, but a
    /// failed preparation of this scene must.
    pub fn begin(&mut self, incoming: usize, world: usize) {
        if incoming == 0 || (incoming != world && incoming != self.scene) {
            return;
        }
        self.scene = incoming;
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 {
            self.generation = 1;
        }
        self.prepared = false;
        self.consumed = false;
    }
    pub fn prepared(&mut self, incoming: usize) {
        if incoming == self.scene && self.generation != 0 {
            self.prepared = true;
        }
    }
    /// Consumed before AfterFetch's early returns too: a duplicate or failed fetch cannot later relabel the old contact
    /// target with a fresh publication time.
    pub fn take(&mut self, incoming: usize) -> Fetch {
        if incoming != self.scene || incoming == 0 {
            return Fetch { scene: incoming, generation: 0, valid: false };
        }
        let valid = self.prepared && !self.consumed;
        self.consumed = true;
        Fetch { scene: incoming, generation: self.generation, valid }
    }
}
/// WorldPoint: a point of the actor's frame in the game's world.
pub fn world_point(actor_position: &[f32; 3], actor_rotation: &[f32; 4], local_point: &[f32; 3], scene_origin: &[f32; 3]) -> Option<[f32; 3]> {
    let mut q = *actor_rotation;
    if !held_grip::normalize(&mut q) {
        return None;
    }
    let offset = held_grip::rotate(&q, local_point);
    let mut world = [0.0f32; 3];
    for k in 0..3 {
        world[k] = actor_position[k] + offset[k] + scene_origin[k];
        if !world[k].is_finite() {
            return None;
        }
    }
    Some(world)
}
/// ErrorMicrometres: both points refer to the same completed physics step. The target is kept in world space across an
/// origin rebase. No Lua reference body's point is used.
pub fn error_micrometres(actor_position: &[f32; 3], actor_rotation: &[f32; 4], actual_local_grip: &[f32; 3], current_origin: &[f32; 3], target_world: &[f32; 3], target_valid: bool) -> u64 {
    if !target_valid {
        return INVALID;
    }
    let Some(actual) = world_point(actor_position, actor_rotation, actual_local_grip, current_origin) else { return INVALID };
    let mut squared = 0.0f64;
    for k in 0..3 {
        let d = f64::from(actual[k]) - f64::from(target_world[k]);
        squared += d * d;
    }
    if !squared.is_finite() {
        return INVALID;
    }
    let micrometres = squared.sqrt() * 1_000_000.0;
    (micrometres.min(MAXIMUM_MICROMETRES as f64) + 0.5) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::held_grip::rotate;
    // main's tools/physics_frame_tests/held_contact_test.cpp
    #[test]
    fn one_prepared_step_is_fetched_once_and_only_from_its_scene() {
        let (a, b) = (0x1000usize, 0x2000usize);
        let mut gate = StepGate::default();
        gate.begin(a, a);
        gate.prepared(a);
        let first = gate.take(a);
        assert!(first.valid, "prepared scene step rejected");
        assert!(!gate.take(a).valid, "duplicate fetch refreshed old target");
        gate.begin(a, a);
        assert!(!gate.take(a).valid, "failed preparation reused old target");
        gate.begin(a, a);
        gate.prepared(a);
        gate.begin(b, a);
        assert!(!gate.take(b).valid, "foreign scene consumed world contact");
        assert!(gate.take(a).valid, "interleaved non-world scene lost world target");
        gate.begin(a, a);
        gate.prepared(a);
        assert!(!gate.take(b).valid, "new scene reused old-scene target");
        gate.begin(b, b);
        gate.prepared(b);
        let replacement = gate.take(b);
        assert!(replacement.valid && replacement.generation != first.generation, "new scene did not get a distinct prepared generation");
        gate.begin(b, b);
        gate.prepared(b);
        gate.take(b);
        assert!(!gate.take(b).valid, "failed fetch was reused with a new timestamp");
    }
    #[test]
    fn the_held_points_error_is_its_own_in_the_world() {
        let identity = [0.0f32, 0.0, 0.0, 1.0];
        let origin = [-2048.0f32, 1536.0, 0.0];
        let actual_grip = [0.025f32, 0.04, -0.02];
        let target = [-2280.75f32, 1704.5, 19.1];
        let mut actor = [0.0f32; 3];
        for k in 0..3 {
            actor[k] = target[k] - origin[k] - actual_grip[k];
        }
        assert!(error_micrometres(&actor, &identity, &actual_grip, &origin, &target, true) < 500, "initial actual-body contact not aligned");
        for degrees in [-170.0f32, -60.0, 0.0, 50.0, 170.0] {
            let h = degrees * 0.008_726_646;
            let q = [0.0, 0.0, h.sin(), h.cos()];
            let offset = rotate(&q, &actual_grip);
            for k in 0..3 {
                actor[k] = target[k] - origin[k] - offset[k];
            }
            assert!(error_micrometres(&actor, &q, &actual_grip, &origin, &target, true) < 500, "rotation about the actual anchor looked like contact loss");
            let unrelated = [target[0] + 1.0, target[1] - 0.7, target[2] + 0.3];
            assert!(error_micrometres(&actor, &q, &actual_grip, &origin, &unrelated, true) > 1_000_000, "fixture did not separate reference-joint motion");
        }
        let rebased = [-2304.0f32, 1792.0, 128.0];
        for k in 0..3 {
            actor[k] = target[k] - rebased[k] - actual_grip[k];
        }
        assert!(error_micrometres(&actor, &identity, &actual_grip, &rebased, &target, true) < 500, "origin rebase changed world contact");
        actor[2] += 0.02;
        let error = error_micrometres(&actor, &identity, &actual_grip, &rebased, &target, true);
        assert!(error > 19_500 && error < 20_500, "real 2cm contact error was not measured: {error}");
        let (a_grip, b_grip, zero) = ([-0.1f32, 0.0, 0.0], [0.1f32, 0.0, 0.0], [0.0f32; 3]);
        let (a_target, b_target) = ([-0.1f32, 0.0, 0.0], [0.15f32, 0.0, 0.0]);
        assert_eq!(error_micrometres(&zero, &identity, &a_grip, &zero, &a_target, true), 0, "first hand inherited the other hand's contact error");
        assert!(error_micrometres(&zero, &identity, &b_grip, &zero, &b_target, true) >= 49_999, "second hand's real contact error was hidden by midpoint");
        assert_eq!(error_micrometres(&zero, &identity, &a_grip, &zero, &a_target, false), INVALID, "missing target published zero success");
        let bad = [0.0f32, 0.0, f32::NAN, 1.0];
        assert_eq!(error_micrometres(&zero, &bad, &a_grip, &zero, &a_target, true), INVALID, "invalid actor pose published zero success");
        let no_target = [f32::INFINITY, 0.0, 0.0];
        assert_eq!(error_micrometres(&zero, &identity, &a_grip, &zero, &no_target, true), INVALID, "invalid target published contact success");
    }
}
