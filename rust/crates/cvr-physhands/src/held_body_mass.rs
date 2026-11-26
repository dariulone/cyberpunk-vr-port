//! include/Natives/HeldBodyMass.hpp: the held actor's identity and the mass it had before the solver's override.

/// SameActor: one native body -- the actor, its proxy and its body index all the same (a recycled actor pointer under
/// another proxy or body is another one).
pub fn same_actor(a: usize, proxy_a: u32, body_a: u32, b: usize, proxy_b: u32, body_b: u32) -> bool {
    a == b && proxy_a == proxy_b && body_a == body_b
}
/// Original: the 100 kg / 20x solver mass is not the inventory's or the carry's weight. A completed hold publishes the
/// mass captured before its first override; both hands on one actor publish the same shared mass. Fresh within 300 ms.
#[allow(clippy::too_many_arguments)]
pub fn original(wanted_proxy: u32, wanted_body: u32, held_proxy: u32, held_body: u32, original_mass: f32, active: bool, at: u64, now: u64) -> Option<f32> {
    if !active
        || at == 0
        || now < at
        || now - at >= 300
        || wanted_proxy != held_proxy
        || wanted_body != held_body
        || !original_mass.is_finite()
        || original_mass <= 0.0
    {
        return None;
    }
    Some(original_mass)
}

#[cfg(test)]
mod tests {
    use super::*;
    // main's tools/physics_frame_tests/held_mass_test.cpp
    struct Snapshot {
        proxy: u32,
        body: u32,
        mass: f32,
        on: bool,
        at: u64,
    }
    fn measure(proxy: u32, body: u32, solver_mass: f32, hands: &[Snapshot; 2], now: u64) -> f32 {
        hands.iter().find_map(|s| original(proxy, body, s.proxy, s.body, s.mass, s.on, s.at, now)).unwrap_or(solver_mass)
    }
    fn same() -> [Snapshot; 2] {
        [Snapshot { proxy: 68334, body: 18, mass: 5.0, on: true, at: 1000 }, Snapshot { proxy: 68334, body: 18, mass: 5.0, on: true, at: 1000 }]
    }
    #[test]
    fn the_held_mass_is_the_parts_own_shared_fresh_and_by_identity() {
        let actor = 0x10000usize;
        assert!(same_actor(actor, 68334, 18, actor, 68334, 18), "same native body not shared by both hands");
        assert!(!same_actor(actor, 68335, 18, actor, 68334, 18), "recycled actor pointer inherited old mass ownership");
        assert!(!same_actor(actor, 68334, 19, actor, 68334, 18), "new body index reused old original mass");
        assert_eq!(measure(68334, 18, 100.0, &same(), 1016), 5.0, "same-body second hand published overridden mass");
        let mut first_released = same();
        first_released[0].on = false;
        assert_eq!(measure(68334, 18, 100.0, &first_released, 1016), 5.0, "remaining hand lost the original mass after first hand released");
        assert_eq!(measure(68334, 7, 5.0, &same(), 1016), 5.0, "reference body inherited another body's mass");
        let (mut before, mut after) = (0.0f32, 0.0f32);
        for body in 0..19 {
            before += 5.0;
            after += measure(68334, body, if body == 18 { 100.0 } else { 5.0 }, &same(), 1016);
        }
        assert!(before == 95.0 && after == before, "anchor change inflated whole-corpse weight after native mass override");
        let mut different = same();
        different[0] = Snapshot { proxy: 68334, body: 7, mass: 5.0, on: true, at: 1000 };
        after = 0.0;
        for body in 0..19 {
            after += measure(68334, body, if body == 7 || body == 18 { 100.0 } else { 5.0 }, &different, 1016);
        }
        assert_eq!(after, 95.0, "two different held parts inflated ragdoll mass");
        assert_eq!(measure(68335, 18, 12.0, &same(), 1016), 12.0, "foreign proxy matched original held mass");
        assert_eq!(measure(68334, 18, 100.0, &same(), 1300), 100.0, "stale snapshot overrode measured mass");
        assert_eq!(measure(68334, 18, 100.0, &same(), 999), 100.0, "future snapshot accepted");
        let mut inactive = same();
        for s in &mut inactive {
            s.on = false;
        }
        assert_eq!(measure(68334, 18, 7.0, &inactive, 1016), 7.0, "released hold changed measured mass");
        let mut invalid = same();
        for s in &mut invalid {
            s.mass = f32::NAN;
        }
        assert_eq!(measure(68334, 18, 7.0, &invalid, 1016), 7.0, "non-finite original mass accepted");
    }
}
