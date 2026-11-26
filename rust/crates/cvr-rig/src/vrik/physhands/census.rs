//! THE FILTER. The game's shader (Cyberpunk2077.exe+0x320794, every scene's) lets two shapes meet when the group of
//! either (filter words 0-1) has a bit in the other's mask (words 2-3), and sends a pair to the game's own callback
//! when either mask has bit 62 or 63. So a hand's group is a bit no mask around it has (nothing reaches for it), and its
//! mask is what it may touch, chosen group by group (choose_mask): the world, light loose bodies and kinematic ones
//! (people, doors), never a group of the player's own and not one of heavy bodies (cars). A census of the shapes near
//! the player finds them, a slice a frame, before the hands are made.
//!
//! Each group value seen, with how many shapes of each kind carry it. The groups OR'd together say nothing: every
//! kind's union covered the statics' bits (2026-11-08: static 5c00c, kinematic 5d04f, callback 5d058), so the hand's
//! mask is chosen from the groups one by one.
use super::body::{actor, hand_filter, hand_filter_static};
use super::px::{self, P};
use super::{settings, tick_ms};
use cvr_host::log;

pub const KIND_STATIC: usize = 0;
pub const KIND_LIGHT: usize = 1;
pub const KIND_KINEMATIC: usize = 2;
pub const KIND_HEAVY: usize = 3;
pub const KIND_CALLBACK: usize = 4;
pub const KIND_PLAYER: usize = 5;
pub const KIND_COUNT: usize = 6;

#[derive(Clone, Copy, Default)]
pub struct GroupSeen {
    pub group: u64,
    pub n: [i32; KIND_COUNT],
    /// kinematic capsules, and of them the thin short ones a body's limbs are
    pub capsules: i32,
    pub limbs: i32,
}
pub const MOST_GROUPS: usize = 48;
pub struct Census {
    /// 0 not begun, 1 actors, 2 aggregates, 3 done
    pub phase: i32,
    pub count: u32,
    pub next: u32,
    pub mask_all: u64,
    pub group_static: u64,
    pub group_light: u64,
    pub group_kinematic: u64,
    pub group_heavy: u64,
    pub group_callback: u64,
    pub shapes: i32,
    pub statics: i32,
    pub lights: i32,
    pub kinematics: i32,
    pub heavies: i32,
    pub callbacks: i32,
    pub universal: i32,
    pub overflow: i32,
    pub player_said: i32,
    pub seen: [GroupSeen; MOST_GROUPS],
    pub groups: i32,
    pub at: [f32; 3],
    /// the player's entity where it was taken (world, not the scene's origin)
    pub from: [f32; 3],
    pub steady_since: u64,
}
impl Census {
    pub const NEW: Census = Census {
        phase: 0,
        count: 0,
        next: 0,
        mask_all: 0,
        group_static: 0,
        group_light: 0,
        group_kinematic: 0,
        group_heavy: 0,
        group_callback: 0,
        shapes: 0,
        statics: 0,
        lights: 0,
        kinematics: 0,
        heavies: 0,
        callbacks: 0,
        universal: 0,
        overflow: 0,
        player_said: 0,
        seen: [GroupSeen { group: 0, n: [0; KIND_COUNT], capsules: 0, limbs: 0 }; MOST_GROUPS],
        groups: 0,
        at: [0.0; 3],
        from: [0.0; 3],
        steady_since: 0,
    };
}
pub static mut census: Census = Census::NEW;
/// A CENSUS TAKEN WHERE ALMOST NOTHING STOOD is taken again: a load's spawn point is steady for seconds before the
/// world streams in round it, and a census there saw one shape -- no group worth meeting, no hands for the whole
/// session (2026-11-15, the player then put 1554 m away at the save). Again once the player is elsewhere, or the world
/// has had time to come in round the same spot (a few times there); the hands, if any, keep stepping with the old
/// filters meanwhile.
pub const THIN_CENSUS: i32 = 20;
pub const CENSUS_REDO_MOVE: f32 = 20.0;
pub const CENSUS_REDO_WAIT_MS: u64 = 10_000;
pub const CENSUS_REDO_TRIES: i32 = 3;
pub static mut census_done_at: u64 = 0;
pub static mut census_tries_here: i32 = 0;
pub const MOST_LISTED: usize = 40_000;
pub static mut listed: [P; MOST_LISTED] = [core::ptr::null_mut(); MOST_LISTED];
pub const CENSUS_RADIUS: f32 = 15.0;
pub const HEAVY: f32 = 50.0;
pub const NEAR_PLAYER: f32 = 0.5;
pub static mut filter_ready: bool = false;
/// BITS NO SHAPE ROUND HERE HAS in its group or its mask (the census): the left hand's and the right hand's -- each
/// hand's palm and fingers carry their own in their group and the other's in their mask while the two meet (either
/// word lets a pair meet, so both or neither) -- and the parts of V's own body each hand meets (in that part's group;
/// the hand's mask has its own while it meets them).
pub static mut bit_left: u64 = 0;
pub static mut bit_right: u64 = 0;
pub static mut bit_self_left: u64 = 0;
pub static mut bit_self_right: u64 = 0;
pub static mut bit_person_left: u64 = 0;
pub static mut bit_person_right: u64 = 0;
pub static mut self_bits_ready: bool = false;

extern "C" {
    static CyberpunkVR_PlayerEntityValid: i32;
    static CyberpunkVR_PlayerEntityPos: [f32; 3];
    static g_menuModeValue: i32;
}
pub fn player_valid() -> bool {
    unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_PlayerEntityValid) != 0 }
}
/// THE SEAT'S PLACE, seated (the pose hook's, from the entity's own transform): the movement's body frame keeps a
/// mounted player at the world's origin, so a census taken in a car looked round (0, 0, 0) and found nothing to meet.
static SEAT_POS: [core::sync::atomic::AtomicU32; 3] =
    [core::sync::atomic::AtomicU32::new(0), core::sync::atomic::AtomicU32::new(0), core::sync::atomic::AtomicU32::new(0)];
static SEAT_AT: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
pub fn note_seat(p: &[f32; 3]) {
    use core::sync::atomic::Ordering::Relaxed;
    for k in 0..3 {
        SEAT_POS[k].store(p[k].to_bits(), Relaxed);
    }
    SEAT_AT.store(super::tick_ms(), core::sync::atomic::Ordering::Release);
}
extern "C" {
    static g_isInVehicle: bool;
}
pub fn player_pos() -> [f32; 3] {
    use core::sync::atomic::Ordering::{Acquire, Relaxed};
    // SAFETY: plain reads of the plugin's flag and the exported floats.
    unsafe {
        if core::ptr::read_volatile(&raw const g_isInVehicle) && super::tick_ms().wrapping_sub(SEAT_AT.load(Acquire)) < 500 {
            return [0, 1, 2].map(|k| f32::from_bits(SEAT_POS[k].load(Relaxed)));
        }
        core::ptr::read_volatile(&raw const CyberpunkVR_PlayerEntityPos)
    }
}
pub fn menu_mode() -> i32 {
    unsafe { core::ptr::read_volatile(&raw const g_menuModeValue) }
}

fn out_of(c: f32, lo: f32, hi: f32) -> f32 {
    if c < lo {
        lo - c
    } else if c > hi {
        c - hi
    } else {
        0.0
    }
}

/// Sample: one actor's shapes counted under their groups, when its bounds are within the census radius.
pub unsafe fn sample(a: P, mut kind_of: i32) {
    // PxActorType: 0 static, 1 dynamic, 4 an articulation's link (a rigid body too); particles and cloth have no shapes
    if kind_of != 0 && kind_of != 1 && kind_of != 4 {
        return;
    }
    if kind_of == 4 {
        kind_of = 1;
    }
    let b = px::world_bounds(a);
    let mut d2 = 0.0f32;
    for k in 0..3 {
        let e = out_of(census.at[k], b.lo[k], b.hi[k]);
        d2 += e * e;
    }
    if !(d2 <= CENSUS_RADIUS * CENSUS_RADIUS) {
        return;
    }
    if a == actor[0] || a == actor[1] || super::people::own_body(a) || super::cabin::own_cabin(a) {
        return;
    }
    // a body in the player's own column (half a metre round the feet point, from the feet to over the head): the
    // player's controller capsule and any body of the player's own that rides with it
    let mut h2 = 0.0f32;
    for k in 0..2 {
        let e = out_of(census.at[k], b.lo[k], b.hi[k]);
        h2 += e * e;
    }
    let by_player = h2 <= NEAR_PLAYER * NEAR_PLAYER && b.hi[2] >= census.at[2] - 0.2 && b.lo[2] <= census.at[2] + 2.1;
    let (mut kinematic, mut heavy) = (false, false);
    if kind_of == 1 {
        kinematic = px::rigid_body_flags(a) & 1 != 0;
        heavy = px::mass(a) > HEAVY;
    }
    let mut list = [core::ptr::null_mut(); 8];
    let n = px::shapes(a, &mut list);
    for &sh in list.iter().take(n.min(8) as usize) {
        let w = px::sim_filter(sh);
        let group = u64::from(w[0]) | (u64::from(w[1]) << 32);
        let mask = u64::from(w[2]) | (u64::from(w[3]) << 32);
        // a shape of every group meeting every mask (the world's floor plane under the map) says nothing about anyone
        if group == !0 || mask == !0 {
            census.universal += 1;
            continue;
        }
        census.shapes += 1;
        census.mask_all |= mask;
        if mask >> 62 != 0 {
            census.group_callback |= group;
            census.callbacks += 1;
        }
        let kind = if kind_of == 0 {
            census.group_static |= group;
            census.statics += 1;
            KIND_STATIC
        } else if kinematic {
            census.group_kinematic |= group;
            census.kinematics += 1;
            KIND_KINEMATIC
        } else if heavy {
            census.group_heavy |= group;
            census.heavies += 1;
            KIND_HEAVY
        } else {
            census.group_light |= group;
            census.lights += 1;
            KIND_LIGHT
        };
        let mut g: Option<usize> = (0..census.groups as usize).find(|&j| census.seen[j].group == group);
        if g.is_none() && (census.groups as usize) < MOST_GROUPS {
            let j = census.groups as usize;
            census.groups += 1;
            census.seen[j] = GroupSeen { group, ..Default::default() };
            g = Some(j);
        }
        let Some(j) = g else {
            census.overflow += 1;
            continue;
        };
        census.seen[j].n[kind] += 1;
        if mask >> 62 != 0 {
            census.seen[j].n[KIND_CALLBACK] += 1;
        }
        let geometry = px::geometry_type(sh);
        if kinematic && geometry == px::GEOM_CAPSULE {
            if let Some(c) = px::capsule(sh) {
                census.seen[j].capsules += 1;
                if c.radius < 0.15 && c.half_height < 0.4 {
                    census.seen[j].limbs += 1;
                }
            }
        }
        if by_player && kinematic {
            census.seen[j].n[KIND_PLAYER] += 1;
            if census.player_said < 8 {
                census.player_said += 1;
                log!(
                    "[physhands]   the player's? a kinematic shape, geometry {}, bounds {:.2} x {:.2} x {:.2} m from {:.2} m over the feet, group {:016x} mask {:016x}\n",
                    geometry,
                    b.hi[0] - b.lo[0],
                    b.hi[1] - b.lo[1],
                    b.hi[2] - b.lo[2],
                    b.lo[2] - census.at[2],
                    group,
                    mask
                );
            }
        }
    }
}

/// THE HAND'S MASK, a bit for each group worth meeting. A group is worth it when what it holds is the world, light
/// props or kinematic bodies (people's, doors: they push the hand, it cannot push them) -- the world alone for a hand
/// holding something -- none of it the player's own and its heavy bodies a tenth of the rest at most. A bit meets every
/// group that has it, so each wanted group gets the one of its bits that meets the fewest shapes not wanted; a bit any
/// of the player's groups has is never taken. A group whose kinematic capsules are mostly fat and tall is characters'
/// controllers: a hand would stop a hand's width short of the body, so those are not met; the thin short ones are
/// limbs, and a hand stops on a limb.
pub fn worth_meeting(g: &GroupSeen, statics: bool) -> bool {
    // THE WORLD ALONE (a holding hand, the fingers, a held thing): a group of mostly static shapes.
    if statics {
        return g.n[KIND_STATIC] > 2 * (g.n[KIND_LIGHT] + g.n[KIND_KINEMATIC] + g.n[KIND_HEAVY]) && g.n[KIND_PLAYER] == 0;
    }
    let good = g.n[KIND_STATIC] + g.n[KIND_LIGHT] + g.n[KIND_KINEMATIC];
    let controllers = g.capsules - g.limbs > g.limbs && g.n[KIND_STATIC] + g.n[KIND_LIGHT] == 0;
    good > 0 && g.n[KIND_PLAYER] == 0 && g.n[KIND_HEAVY] * 10 <= good && !controllers
}
pub unsafe fn choose_mask(statics: bool, group: u64) -> (u64, &'static str) {
    let seen = &census.seen[..census.groups as usize];
    let player = seen.iter().filter(|g| g.n[KIND_PLAYER] != 0).fold(0u64, |a, g| a | g.group);
    let allowed = !(3u64 << 62) & !group & !player;
    let mut mask = 0u64;
    let mut missed = 0;
    for g in seen {
        if !worth_meeting(g, statics) || (g.group & mask) != 0 {
            continue;
        }
        let (mut best_bit, mut best_cost) = (-1i32, 0i32);
        for bit in 0..62 {
            if (g.group >> bit) & 1 == 0 || (allowed >> bit) & 1 == 0 {
                continue;
            }
            let mut cost = 0;
            for o in seen {
                if (o.group >> bit) & 1 != 0 && !worth_meeting(o, statics) {
                    cost += o.n[KIND_STATIC] + o.n[KIND_LIGHT] + o.n[KIND_KINEMATIC] + o.n[KIND_HEAVY];
                }
            }
            if best_bit < 0 || cost < best_cost {
                best_bit = bit;
                best_cost = cost;
            }
        }
        if best_bit < 0 {
            missed += 1;
        } else {
            mask |= 1u64 << best_bit;
        }
    }
    (mask, if missed != 0 { "some wanted groups have only the player's bits" } else { "a bit a wanted group" })
}
pub unsafe fn meets(mask: u64, kind: usize) -> i32 {
    census.seen[..census.groups as usize].iter().filter(|g| g.group & mask != 0).map(|g| g.n[kind]).sum()
}

/// THE LISTS ARE ASKED FOR AGAIN EACH STEP, and only this step's slice of them read. A list kept from one step to the
/// next held actors the game released in between, on the way in from a load, and the census called through one of them
/// (2026-11-08 22:02). A list that changed under the walk only moves which actors a slice sees.
pub unsafe fn census_slice(sc: P, origin: &[f32; 3]) {
    const SLICE: u32 = 400;
    if census.phase == 0 {
        // the player in a world that has stepped steadily for three seconds: not a menu's, not a loading screen's
        let now = tick_ms();
        if !player_valid() || menu_mode() != 0 {
            census.steady_since = 0;
            return;
        }
        if census.steady_since == 0 {
            census.steady_since = now;
        }
        if now - census.steady_since < 3000 {
            return;
        }
        let p = player_pos();
        for k in 0..3 {
            census.at[k] = p[k] - origin[k];
            census.from[k] = p[k];
        }
        census.next = 0;
        census.phase = 1;
        return;
    }
    if census.phase == 1 {
        census.count = px::scene_actors(sc, 1 | 2, &mut listed, 0);
        let end = (census.next + SLICE).min(census.count);
        for i in census.next..end {
            let a = listed[i as usize];
            sample(a, px::actor_type(a));
        }
        census.next = end;
        if end >= census.count {
            census.next = 0;
            census.phase = 2;
        }
        return;
    }
    if census.phase == 2 {
        census.count = px::scene_aggregates(sc, &mut listed, 0);
        let end = (census.next + SLICE / 8).min(census.count);
        for i in census.next..end {
            let mut members = [core::ptr::null_mut(); 64];
            let n = px::aggregate_actors(listed[i as usize], &mut members);
            for &m in members.iter().take(n.min(64) as usize) {
                sample(m, px::actor_type(m));
            }
        }
        census.next = end;
        if end < census.count {
            return;
        }
        census.phase = 3;
        census_done_at = tick_ms();
        let s = settings::current();
        // the hand's group: the highest bit under 62 no mask round here has
        let mut group = 0u64;
        for bit in (32..=61).rev() {
            if (census.mask_all >> bit) & 1 == 0 {
                group = 1u64 << bit;
                break;
            }
        }
        if s.group_override != 0 {
            group = s.group_override;
        }
        let (mut mask, rule) = choose_mask(false, group);
        let (mut mask_static, rule_static) = choose_mask(true, group);
        if s.mask_override != 0 {
            mask = s.mask_override;
            mask_static = s.mask_override & census.group_static;
        }
        hand_filter = [group as u32, (group >> 32) as u32, mask as u32, (mask >> 32) as u32];
        hand_filter_static = hand_filter;
        hand_filter_static[2] = mask_static as u32;
        hand_filter_static[3] = (mask_static >> 32) as u32;
        filter_ready = group != 0 && mask != 0;
        {
            let mut groups = census.group_static | census.group_light | census.group_kinematic | census.group_heavy | census.group_callback;
            for g in &census.seen[..census.groups as usize] {
                groups |= g.group;
            }
            let used = census.mask_all | groups | group | mask | mask_static | (3u64 << 62);
            let mut bits = [0u64; 6];
            let mut found = 0usize;
            for bit in (0..=61).rev() {
                if found >= 6 {
                    break;
                }
                if (used >> bit) & 1 == 0 {
                    bits[found] = 1u64 << bit;
                    found += 1;
                }
            }
            self_bits_ready = found >= 4 && filter_ready;
            bit_left = if self_bits_ready { bits[0] } else { 0 };
            bit_right = if self_bits_ready { bits[1] } else { 0 };
            bit_self_left = if self_bits_ready { bits[2] } else { 0 };
            bit_self_right = if self_bits_ready { bits[3] } else { 0 };
            bit_person_left = if found >= 6 && filter_ready { bits[4] } else { 0 };
            bit_person_right = if found >= 6 && filter_ready { bits[5] } else { 0 };
            log!("[physhands] NPC contact identities: left {:016x}, right {:016x}\n", bit_person_left, bit_person_right);
            log!(
                "[physhands] the hands' own bits: left {:016x}, right {:016x}, V's body for the left {:016x}, for the right {:016x}{}\n",
                bit_left,
                bit_right,
                bit_self_left,
                bit_self_right,
                if self_bits_ready { "" } else { " (too few free: the hands meet neither V nor each other)" }
            );
        }
        log!(
            "[physhands] census of {} shapes within {:.0} m (and {} of every group): static {} (groups {:016x}), light {} ({:016x}), kinematic {} ({:016x}), heavy {} ({:016x}), to the game's callback {} (groups {:016x}); every mask {:016x}. The hands: group {:016x}, mask {:016x} (holding: {:016x}){}\n",
            census.shapes,
            CENSUS_RADIUS,
            census.universal,
            census.statics,
            census.group_static,
            census.lights,
            census.group_light,
            census.kinematics,
            census.group_kinematic,
            census.heavies,
            census.group_heavy,
            census.callbacks,
            census.group_callback,
            census.mask_all,
            group,
            mask,
            mask_static,
            if filter_ready { "" } else { ": nothing to meet, no hands" }
        );
        for g in &census.seen[..census.groups as usize] {
            cvr_host::vlog!(
                "[physhands]   group {:016x}: static {}, light {}, kinematic {} (capsules {}, limb-sized {}), heavy {}; to the callback {}; the player's {}\n",
                g.group,
                g.n[KIND_STATIC],
                g.n[KIND_LIGHT],
                g.n[KIND_KINEMATIC],
                g.capsules,
                g.limbs,
                g.n[KIND_HEAVY],
                g.n[KIND_CALLBACK],
                g.n[KIND_PLAYER]
            );
        }
        cvr_host::vlog!(
            "[physhands] the mask ({}) meets static {}, light {}, kinematic {}, heavy {}, the player's {}; holding ({}) static {}, light {}{}\n",
            rule,
            meets(mask, KIND_STATIC),
            meets(mask, KIND_LIGHT),
            meets(mask, KIND_KINEMATIC),
            meets(mask, KIND_HEAVY),
            meets(mask, KIND_PLAYER),
            rule_static,
            meets(mask_static, KIND_STATIC),
            meets(mask_static, KIND_LIGHT),
            if census.overflow != 0 { " (more groups than kept)" } else { "" }
        );
    }
}
