//! The Osprey's screen hinge, its checks (main's include/Anim/OpticHinge.hpp, 0aa233e0a): the native rig folds the
//! scope screen 180 degrees about scope_hinge only in idlepose_ironsight, so while the VR port aims the Osprey through
//! its screen (the optics' camera path, CyberpunkVRPort_Optics' hinge request) the bone is written open after the
//! native pose copy -- for the one animated object the request resolved, on a fresh request (200 ms), never as a
//! fallback to the bind pose of an omitted bone. Disabling leaves the next native copy untouched: no stale restore.

/// The hinge's bone in the Osprey's rig and the rig's bone count.
pub const BONE: u32 = 8;
pub const BONE_COUNT: u32 = 13;
/// The native Osprey rig's referencePoseLS[8] (osprey_hinge_audit): the one rotation idlepose_ironsight makes the
/// identity -- (i, j, k, r).
pub const HIP_ROTATION: [f32; 4] = [0.0, 0.0, 1.0, 0.0];
/// A request is fresh this long (ms).
pub const FRESH_MS: u64 = 200;

const fn fnv1a64(text: &[u8]) -> u64 {
    let mut h = 14_695_981_039_346_656_037u64;
    let mut i = 0;
    while i < text.len() {
        h = (h ^ text[i] as u64).wrapping_mul(1_099_511_628_211);
        i += 1;
    }
    h
}
/// CName("scope_hinge").
pub const BONE_NAME_HASH: u64 = fnv1a64(b"scope_hinge");

/// What a request resolved: the player, the weapon and its Barrel component, the animated object, its pose descriptor
/// and track buffer, the rig and the meta rig, the graph's component and the control binding (0 when the Barrel is its
/// own graph).
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Identity {
    pub player: usize,
    pub weapon: usize,
    pub component: usize,
    pub object: usize,
    pub descriptor: usize,
    pub rig: usize,
    pub meta_rig: usize,
    pub track: usize,
    pub meta_rig_id: u64,
    pub graph_component: usize,
    pub control_binding: usize,
}
impl Identity {
    pub fn valid(&self) -> bool {
        self.player != 0
            && self.weapon != 0
            && self.component != 0
            && self.object != 0
            && self.descriptor != 0
            && self.rig != 0
            && self.meta_rig != 0
            && self.track != 0
            && self.graph_component != 0
    }
}
/// The request in force: what it resolved and when (GetTickCount64).
#[derive(Clone, Copy, Default, Debug)]
pub struct Target {
    pub identity: Identity,
    pub stamp: u64,
    pub enabled: bool,
}
/// One pose pass: the binding read again now and the pass's own pointers.
#[derive(Clone, Copy, Default, Debug)]
pub struct Pass {
    pub current: Identity,
    pub object: usize,
    pub descriptor: usize,
    pub rig: usize,
    pub track: usize,
    pub player: usize,
    pub meta_rig: usize,
    pub now: u64,
    pub context: bool,
    pub weak_alive: bool,
    pub legacy_conflict: bool,
}

/// Matches: the request fresh and enabled, the context and the handles alive, and the pass the request's own.
pub fn matches(target: &Target, pass: &Pass) -> bool {
    target.enabled
        && target.identity.valid()
        && target.stamp != 0
        && pass.context
        && pass.weak_alive
        && !pass.legacy_conflict
        && pass.now >= target.stamp
        && pass.now - target.stamp < FRESH_MS
        && target.identity == pass.current
        && pass.player == target.identity.player
        && pass.object == target.identity.object
        && pass.descriptor == target.identity.descriptor
        && pass.rig == target.identity.rig
        && pass.track == target.identity.track
        && pass.meta_rig == target.identity.meta_rig
}

/// Apply's choice: the pose slot the hinge's rotation goes to -- the one destination the LOD's remap `pairs` (source,
/// destination) gives the hinge bone, within the pose's `slots` and named scope_hinge in the meta rig's names (`names`
/// of them, `name_of` reading one); None when the pass is not the request's or the remap is not the rig's.
pub fn destination(target: &Target, pass: &Pass, pairs: &[u32], slots: u32, names: u32, name_of: impl Fn(u32) -> Option<u64>) -> Option<u32> {
    if !matches(target, pass) || pairs.len() % 2 != 0 || pairs.len() > BONE_COUNT as usize * 2 {
        return None;
    }
    let mut found: Option<u32> = None;
    for pair in pairs.chunks_exact(2) {
        if pair[0] == BONE {
            if found.is_some() {
                return None;
            }
            found = Some(pair[1]);
        }
    }
    let d = found?;
    (d < slots && d < names && name_of(d) == Some(BONE_NAME_HASH)).then_some(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> Identity {
        Identity { player: 1, weapon: 2, component: 3, object: 4, descriptor: 5, rig: 6, meta_rig: 7, track: 8, meta_rig_id: 9, graph_component: 10, control_binding: 0 }
    }
    fn pass(now: u64) -> Pass {
        let i = identity();
        Pass { current: i, object: 4, descriptor: 5, rig: 6, track: 8, player: 1, meta_rig: 7, now, context: true, weak_alive: true, legacy_conflict: false }
    }

    #[test]
    fn the_hash_is_the_cname() {
        assert_eq!(BONE_NAME_HASH, fnv1a64(b"scope_hinge"));
        // FNV-1a 64 of the empty string is the offset basis
        assert_eq!(fnv1a64(b""), 14_695_981_039_346_656_037);
    }

    #[test]
    fn only_a_fresh_request_for_this_pass() {
        let t = Target { identity: identity(), stamp: 1000, enabled: true };
        assert!(matches(&t, &pass(1000)));
        assert!(matches(&t, &pass(1199)));
        assert!(!matches(&t, &pass(1200)), "200 ms old");
        assert!(!matches(&t, &pass(999)), "before the request");
        assert!(!matches(&Target { enabled: false, ..t }, &pass(1000)));
        assert!(!matches(&Target { stamp: 0, ..t }, &pass(0)));
        let mut p = pass(1000);
        p.object = 44;
        assert!(!matches(&t, &p), "another object");
        let mut p = pass(1000);
        p.current.meta_rig_id = 1;
        assert!(!matches(&t, &p), "the binding changed");
        let mut p = pass(1000);
        p.legacy_conflict = true;
        assert!(!matches(&t, &p));
        let mut bad = identity();
        bad.track = 0;
        assert!(!matches(&Target { identity: bad, stamp: 1000, enabled: true }, &Pass { current: bad, track: 0, ..pass(1000) }));
    }

    #[test]
    fn the_hinge_slot_from_the_remap() {
        let t = Target { identity: identity(), stamp: 1000, enabled: true };
        let p = pass(1010);
        let mut names = [0u64; 13];
        names[3] = BONE_NAME_HASH;
        let destination = |t: &Target, p: &Pass, pairs: &[u32], slots: u32, names: &[u64]| {
            destination(t, p, pairs, slots, names.len() as u32, |i| names.get(i as usize).copied())
        };
        // (source, destination) pairs: bone 8 to slot 3
        let pairs = [0, 0, 1, 1, 8, 3];
        assert_eq!(destination(&t, &p, &pairs, 13, &names), Some(3));
        // the slot out of the pose, or not named scope_hinge
        assert_eq!(destination(&t, &p, &pairs, 3, &names), None);
        assert_eq!(destination(&t, &p, &[8, 4], 13, &names), None);
        // the bone twice, an odd remap, or one longer than the rig
        assert_eq!(destination(&t, &p, &[8, 3, 8, 3], 13, &names), None);
        assert_eq!(destination(&t, &p, &[8, 3, 1], 13, &names), None);
        assert_eq!(destination(&t, &p, &[0u32; 28], 13, &names), None);
        // the bone omitted: no write (never a fallback to LOD0)
        assert_eq!(destination(&t, &p, &[0, 0, 1, 1], 13, &names), None);
        // an expired request
        assert_eq!(destination(&t, &pass(1300), &pairs, 13, &names), None);
    }
}
