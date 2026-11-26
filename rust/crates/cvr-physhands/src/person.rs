//! include/Natives/PersonGrip.hpp and PersonCollisionFilter.hpp: an NPC's part held by a hand -- the snapshots that
//! cross the physics, the player's animation and the NPC's animation threads without keeping game objects (world
//! coordinates), the arm's reach on the held point, and the filter of an NPC's capsules per hand.

/// GripControl: the controller's place for a held person's part and the arm that reaches it.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct GripControl {
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub shoulder: [f32; 3],
    pub reach: f32,
    pub at: u64,
}
impl Default for GripControl {
    fn default() -> GripControl {
        GripControl { position: [0.0; 3], rotation: [0.0, 0.0, 0.0, 1.0], shoulder: [0.0; 3], reach: 0.0, at: 0 }
    }
}
/// PersonContact: where a hand met a person's capsule.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct PersonContact {
    pub owner: i32,
    pub serial: u64,
    pub at: u64,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub capsule: [f32; 3],
    pub capsule_rotation: [f32; 4],
    pub radius: f32,
    pub half: f32,
    pub ready: bool,
}
impl Default for PersonContact {
    fn default() -> PersonContact {
        PersonContact { owner: 0, serial: 0, at: 0, position: [0.0; 3], rotation: [0.0, 0.0, 0.0, 1.0], capsule: [0.0; 3], capsule_rotation: [0.0, 0.0, 0.0, 1.0], radius: 0.0, half: 0.0, ready: false }
    }
}
/// PersonAttachment: the hold on a person's part as applied.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct PersonAttachment {
    pub owner: i32,
    pub serial: u64,
    pub at: u64,
    pub revision: u64,
    pub control_at: u64,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub capsule: [f32; 3],
    pub capsule_rotation: [f32; 4],
    pub radius: f32,
    pub half: f32,
    pub on: bool,
    pub release: bool,
}
impl Default for PersonAttachment {
    fn default() -> PersonAttachment {
        PersonAttachment {
            owner: 0,
            serial: 0,
            at: 0,
            revision: 0,
            control_at: 0,
            position: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            capsule: [0.0; 3],
            capsule_rotation: [0.0, 0.0, 0.0, 1.0],
            radius: 0.0,
            half: 0.0,
            on: false,
            release: false,
        }
    }
}
/// LimitGripReach: the held point no farther from the shoulder than the arm reaches; false without a usable reach.
pub fn limit_grip_reach(control: &GripControl, point: &mut [f32; 3]) -> bool {
    let mut distance2 = 0.0f32;
    for k in 0..3 {
        let d = point[k] - control.shoulder[k];
        distance2 += d * d;
    }
    // !(reach > .01) catches a NaN reach too
    if !distance2.is_finite() || control.reach.is_nan() || control.reach <= 0.01 || !control.reach.is_finite() {
        return false;
    }
    let distance = distance2.sqrt();
    if distance > control.reach {
        for k in 0..3 {
            point[k] = control.shoulder[k] + (point[k] - control.shoulder[k]) * (control.reach / distance);
        }
    }
    true
}

/// PersonCollisionFilter: an NPC capsule's filter words (group 0-1, mask 2-3) and the hands it meets (bits).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PersonCollisionFilter {
    pub words: [u32; 4],
    pub hands: u8,
}
/// FilterPersonHands: the game accepts a pair when either group's bits occur in the other's mask. NPC capsules use
/// dedicated hand identity bits, separate from the hand and weapon contact bits, so holding with one hand does not
/// disable the other hand.
pub const fn filter_person_hands(common: u64, left: u64, right: u64, requested: u8) -> PersonCollisionFilter {
    let requested = requested & 3;
    if left != 0 && right != 0 && (left & right) == 0 && (common & (left | right)) == 0 {
        let mask = (if requested & 1 != 0 { left } else { 0 }) | (if requested & 2 != 0 { right } else { 0 });
        return PersonCollisionFilter { words: [0, 0, mask as u32, (mask >> 32) as u32], hands: requested };
    }
    // Without distinct identities a partial pair cannot be expressed: the conservative physical filter mirrored in
    // the query mask as well.
    if common != 0 && requested == 3 {
        return PersonCollisionFilter { words: [common as u32, (common >> 32) as u32, common as u32, (common >> 32) as u32], hands: 3 };
    }
    PersonCollisionFilter { words: [0; 4], hands: 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    // main's tools/physics_frame_tests/person_filter_test.cpp
    fn pair(group: u64, mask: u64, p: &PersonCollisionFilter) -> bool {
        let pg = u64::from(p.words[0]) | (u64::from(p.words[1]) << 32);
        let pm = u64::from(p.words[2]) | (u64::from(p.words[3]) << 32);
        (group & pm) != 0 || (pg & mask) != 0
    }
    #[test]
    fn each_hand_meets_a_person_by_its_own_identity() {
        let (common, left, right, contact_l, contact_r) = (1u64 << 60, 1u64 << 54, 1u64 << 53, 1u64 << 58, 1u64 << 57);
        for held in 0u8..4 {
            let wanted = 3 & !held;
            let p = filter_person_hands(common, left, right, wanted);
            for side in 0..2 {
                for hand_mask in [0u64, 0x5c004, contact_l | contact_r] {
                    let expected = (wanted >> side) & 1 != 0;
                    let actual = pair(common | if side == 1 { right } else { left }, hand_mask, &p);
                    assert_eq!(actual, expected, "one hand's grip changed the other hand's physical contacts");
                    assert_eq!(actual, (p.hands >> side) & 1 != 0, "query eligibility disagrees with the physical pair filter");
                }
            }
            assert!(!pair(0x5c004, 0, &p), "NPC capsule started colliding with the static world");
            assert!(!pair(contact_l, 0, &p) && !pair(contact_r, 0, &p), "hand/weapon contact identities were reused for NPC filtering");
        }
        for wanted in 0u8..4 {
            let p = filter_person_hands(common, 0, 0, wanted);
            for group in [common, common | left, common | right] {
                assert_eq!(pair(group, 0, &p), p.hands != 0, "fallback simulation and query disagree");
            }
            assert_eq!(p.hands, if wanted == 3 { 3 } else { 0 }, "fallback pretended to isolate a hand without distinct identities");
        }
        let old_physical = filter_person_hands(common, 0, 0, 1);
        assert!(!pair(common | left, 0, &old_physical), "old shared filter no longer exposes loss of the nonholding hand");
        assert_eq!(old_physical.hands, 0, "disabled collision remained a query obstacle");
        assert_eq!(filter_person_hands(common, left, left, 1).hands, 0, "aliased identities were accepted");
        assert_eq!(filter_person_hands(0, 0, 0, 3).hands, 0, "empty physical filter claimed contacts");
    }
    #[test]
    fn the_held_point_stays_within_the_arms_reach() {
        let c = GripControl { shoulder: [0.0, 0.0, 1.4], reach: 0.6, ..Default::default() };
        let mut p = [1.2f32, 0.0, 1.4];
        assert!(limit_grip_reach(&c, &mut p));
        assert!((p[0] - 0.6).abs() < 1e-6);
        let mut q = [0.3f32, 0.0, 1.4];
        assert!(limit_grip_reach(&c, &mut q) && q[0] == 0.3);
        assert!(!limit_grip_reach(&GripControl { reach: f32::NAN, ..c }, &mut q));
        assert!(!limit_grip_reach(&GripControl { reach: 0.005, ..c }, &mut q));
    }
}
