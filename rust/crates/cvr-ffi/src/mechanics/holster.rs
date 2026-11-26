//! mods/cet/CyberpunkVRPort_Holster (modules/holsters.lua, placements.lua, shells.lua, simple.lua; its data in
//! holster_data.rs): every weapon in the weapon slots that is not in a hand hangs on the body as a copy
//! (vrport\holster\vrp_holster_weapon.ent in that item's own appearance -- the one the engine's seed picks among the
//! .app's appearances carrying all of the record's visual tags) made a child of a bone slot (VRAttachToSlot): pistols at
//! the drawing hip then under the off arm, blades at the off hip then over the shoulders, long guns over the shoulders
//! and the lower back, knives on the chest, the thigh, the lower back -- or where the VR menu's inventory chose
//! (holster_places.json, read again when it changes); a dual-wielded pair on both hips. The port's own holster shells
//! (the leg holster, the shoulder harness with its underarm holster and knife sheath) hang round them. A copy is built
//! out of sight (under the floor, then 10 m under its bone) and shown once seated; hidden while its weapon is in the
//! hand, in a scene playing the body or in a vehicle, or when the menu hides the holsters.
//!
//! The drawing hand's grip near a copy takes it: a place with `pull` holds the wrist on the weapon's hold and draws it out
//! along the holster (the fingers on its grip through the plugin's grasp) until it comes free and rides in the hand
//! until the game's weapon is there; a place without draws at once. The same grip near its own place with the weapon
//! in hand puts it away: the copy takes the weapon's pose and goes home over the holster's mouth. A weapon's own hold
//! is learned once it has been steady in the hand. The grip belongs elsewhere with a cigarette, a hand at the wheel, a
//! lit belt grenade (the belt reports it: VRBeltGrenadeReach) or a throw wound up.
//!
//! The simple holsters (shared [23]): the earlier hand-to-zone equip -- back, drawing hip, other hip -- by the plugin's
//! distances (shared [20..22]).
//!
//! The menu's side (slotInfo, setPlace, hidden, setHidden) stays the Lua host's and writes holster_places.json; the
//! belt asks VRHolsterNear; the player's detach releases everything (VRHolsterRelease).
use super::holster_data::{Pose, BIND_CHEST_M, BIND_CHEST_W, HARNESS_M, HARNESS_PARTS, HARNESS_W, LEG, LOOKS};
use crate::script_call::{call_handle, call_native, invoke, invoke_handle, method, Value};
use cvr_host::log;
use red4ext_abi::cname::CName;
use red4ext_abi::handle::{Handle, WeakHandle};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Mutex;

const ENT: &str = "vrport\\holster\\vrp_holster_weapon.ent";
const SCAN_EVERY: f64 = 0.25;
const COOLDOWN: f64 = 0.6;
const STOW_TIME: f64 = 0.35;
const IN_HAND_WAIT: f64 = 3.0;
const RELEASE_TIME: f64 = 0.25;
const HOLD_REACHED: f64 = 3.0 * std::f64::consts::PI / 180.0;
const HOLD_WAIT: f64 = 0.25;
const BUILD_TIME: f64 = 1.0;
const BUILD_LIMIT: f64 = 15.0;
const SEAT_DELAY: f64 = 0.3;
const OUT_OF_SIGHT: V3 = [-10.0, 0.0, 0.0];
/// A seated copy's entity measured against its place once a second; farther than this it is seated again.
const SEAT_CHECK_EVERY: f64 = 1.0;
const SEAT_TOLERANCE: f64 = 0.25;
const LEARN_AFTER: f64 = 0.7;
const WRIST: [i32; 2] = [23, 24];

type V3 = [f64; 3];
type Q = [f64; 4];
type R<T> = Result<T, String>;

// ---------------------------------------------------------------- rotations ({ i, j, k, r })
fn qmul(a: Q, b: Q) -> Q {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}
fn qconj(a: Q) -> Q {
    [-a[0], -a[1], -a[2], a[3]]
}
fn qrot(q: Q, v: V3) -> V3 {
    let r = qmul(qmul(q, [v[0], v[1], v[2], 0.0]), qconj(q));
    [r[0], r[1], r[2]]
}
fn qnorm(q: Q) -> Q {
    let l = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if l < 1e-8 {
        [0.0, 0.0, 0.0, 1.0]
    } else {
        [q[0] / l, q[1] / l, q[2] / l, q[3] / l]
    }
}
fn qangle(a: Q, b: Q) -> f64 {
    2.0 * (a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]).abs().min(1.0).acos()
}
fn qslerp(a: Q, mut b: Q, t: f64) -> Q {
    let mut d = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    if d < 0.0 {
        b = [-b[0], -b[1], -b[2], -b[3]];
        d = -d;
    }
    if d > 0.9995 {
        return qnorm([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t, a[3] + (b[3] - a[3]) * t]);
    }
    let th = d.acos();
    let (s, sa, sb) = (th.sin(), ((1.0 - t) * th).sin(), (t * th).sin());
    [(a[0] * sa + b[0] * sb) / s, (a[1] * sa + b[1] * sb) / s, (a[2] * sa + b[2] * sb) / s, (a[3] * sa + b[3] * sb) / s]
}
fn vnorm(a: V3) -> V3 {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    [a[0] / l, a[1] / l, a[2] / l]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn lerp3(a: V3, b: V3, t: f64) -> V3 {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn scale(a: V3, k: f64) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn dist(a: V3, b: V3) -> f64 {
    let d = sub(a, b);
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}
/// The rotation whose columns are x, y, z.
fn from_axes(x: V3, y: V3, z: V3) -> Q {
    let (m00, m01, m02) = (x[0], y[0], z[0]);
    let (m10, m11, m12) = (x[1], y[1], z[1]);
    let (m20, m21, m22) = (x[2], y[2], z[2]);
    let tr = m00 + m11 + m22;
    if tr > 0.0 {
        let s = (tr + 1.0).sqrt() * 2.0;
        [(m21 - m12) / s, (m02 - m20) / s, (m10 - m01) / s, 0.25 * s]
    } else if m00 > m11 && m00 > m22 {
        let s = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
        [0.25 * s, (m01 + m10) / s, (m02 + m20) / s, (m21 - m12) / s]
    } else if m11 > m22 {
        let s = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
        [(m01 + m10) / s, 0.25 * s, (m12 + m21) / s, (m02 - m20) / s]
    } else {
        let s = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
        [(m02 + m20) / s, (m12 + m21) / s, 0.25 * s, (m10 - m01) / s]
    }
}
fn compose(a: Pose, b: Pose) -> Pose {
    let r = qrot(a.q, b.p);
    Pose { p: add(a.p, r), q: qmul(a.q, b.q) }
}
fn inverse(a: Pose) -> Pose {
    let iq = qconj(a.q);
    let r = qrot(iq, a.p);
    Pose { p: [-r[0], -r[1], -r[2]], q: iq }
}
const IDENTITY: Pose = Pose { p: [0.0; 3], q: [0.0, 0.0, 0.0, 1.0] };

// ---------------------------------------------------------------- placements.lua
/// One family's pose at a place, in body words (up, forward, right).
#[derive(Clone, Copy)]
struct Fam {
    pos: V3,
    dir: V3,
    face: V3,
}
struct Def {
    name: &'static str,
    at: (&'static str, &'static str),
    reach: f64,
    pull: Option<f64>,
    gun: Option<Fam>,
    melee: Option<Fam>,
    grab: Option<(f64, f64)>,
    mirror_of: Option<&'static str>,
}
const HIPS: (&str, &str) = ("slots", "hips");
const CHEST: (&str, &str) = ("ItemAttachmentSlots", "Chest");
const fn fam(pos: V3, dir: V3, face: V3) -> Option<Fam> {
    Some(Fam { pos, dir, face })
}
static RIGHT: [Def; 8] = [
    // A gun at the right hip sits flat against the belt and draws along `pull`, grip first; main's 2026-11-19 placements:
    // 3 cm down and 2 cm forward of the earlier belt pose, the grip canted 25 degrees forward about the hips' lateral
    // axis (the muzzle down and back); the shells and reach centres derive from the same pose. A blade keeps the earlier.
    Def {
        name: "hip_r",
        at: HIPS,
        reach: 0.15,
        pull: Some(0.13),
        gun: fam([0.14, 0.15, 0.207], [-1.0, -0.466_307_658_154_998_6, 0.0], [0.0, 1.0, 0.0]), // -tan(25 deg)
        melee: fam([0.17, 0.06, 0.23], [-1.0, 0.10, 0.0], [0.0, 1.0, 0.0]),
        grab: None,
        mirror_of: None,
    },
    Def {
        name: "hip_l",
        at: HIPS,
        reach: 0.13,
        pull: Some(0.22),
        gun: None,
        melee: fam([0.105, 0.147, -0.178], [-0.293, -0.944, -0.150], [-0.095, 0.185, -0.977]),
        grab: None,
        mirror_of: None,
    },
    Def { name: "underarm_l", at: CHEST, reach: 0.14, pull: Some(0.12), gun: fam([-0.10, 0.11, -0.20], [-0.10, -1.0, 0.0], [1.0, 0.0, 0.0]), melee: None, grab: None, mirror_of: None },
    Def { name: "chest_l", at: CHEST, reach: 0.12, pull: None, gun: None, melee: fam([-0.06, 0.179, -0.134], [-1.0, 0.0, 0.20], [0.0, -0.59, -0.81]), grab: None, mirror_of: None },
    // The places on the back are reached for unseen, so their reach is wider than the ones in view: 25 cm round the
    // shoulder's grab line, 22 cm round the lower back.
    Def {
        name: "shoulder_r",
        at: CHEST,
        reach: 0.25,
        pull: None,
        gun: fam([0.10, -0.125, 0.12], [-0.90, 0.0, -0.43], [0.43, 0.0, -0.90]),
        melee: fam([0.175, -0.12, 0.16], [-0.760, 0.053, -0.648], [0.649, 0.008, -0.761]),
        grab: Some((0.32, 0.22)),
        mirror_of: None,
    },
    Def {
        name: "lower_back",
        at: HIPS,
        reach: 0.22,
        pull: None,
        gun: fam([0.04, -0.15, 0.14], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0]),
        melee: fam([0.04, -0.15, 0.14], [0.0, 0.0, -1.0], [-1.0, 0.0, 0.0]),
        grab: None,
        mirror_of: None,
    },
    // A knife low on the right thigh, blade down, flat on the thigh, the edge forward; a pistol or revolver 3 cm lower
    // and 2 cm forward.
    Def {
        name: "thigh_r",
        at: HIPS,
        reach: 0.12,
        pull: None,
        gun: fam([-0.27, 0.067, 0.19], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        melee: fam([-0.24, 0.047, 0.19], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        grab: None,
        mirror_of: None,
    },
    Def { name: "shoulder_l", at: CHEST, reach: 0.25, pull: None, gun: None, melee: None, grab: Some((0.32, 0.22)), mirror_of: Some("shoulder_r") },
];
fn mirror_name(n: &str) -> Option<&'static str> {
    Some(match n {
        "hip_r" => "hip_l",
        "hip_l" => "hip_r",
        "underarm_l" => "underarm_r",
        "chest_l" => "chest_r",
        "shoulder_r" => "shoulder_l",
        "shoulder_l" => "shoulder_r",
        "thigh_r" => "thigh_l",
        "lower_back" => "lower_back",
        _ => return None,
    })
}
/// The place's pose for a family: its own one, or the other family's; a mirrored one turned over.
fn fam_pose(def: &Def, melee: bool) -> Option<Fam> {
    if let Some(src) = def.mirror_of.and_then(|n| RIGHT.iter().find(|d| d.name == n)) {
        let p = fam_pose(src, melee)?;
        let z = |v: V3| [v[0], v[1], -v[2]];
        return Some(Fam { pos: z(p.pos), dir: z(p.dir), face: z(p.face) });
    }
    if melee {
        def.melee.or(def.gun)
    } else {
        def.gun.or(def.melee)
    }
}
/// The weapon's axes in the bone's frame: y kept exactly, z made square to it.
fn quat_yz(y: V3, z: V3) -> Q {
    let y = vnorm(y);
    let x = vnorm(cross(y, z));
    let z = cross(x, y);
    from_axes(x, y, z)
}
/// A place as this player has it.
#[derive(Clone, Debug, PartialEq)]
struct Place {
    name: &'static str,
    comp: &'static str,
    slot: &'static str,
    pos: V3,
    rot: Q,
    reach: f64,
    pull: Option<f64>,
    pull_axis: Option<V3>,
    pull_local: V3,
    melee: bool,
    grab: Option<f64>,
}
/// P.get: the place `name` (a right-handed name) for a melee weapon or a gun, turned over for the left hand.
fn place_get(name: &str, left: bool, melee: bool) -> Option<Place> {
    let def = RIGHT.iter().find(|d| d.name == name)?;
    let s = if left { -1.0 } else { 1.0 };
    let p = fam_pose(def, melee)?;
    let v = |a: V3| [a[0], -a[1], a[2] * s];
    let (pos, dir, face) = (v(p.pos), v(p.dir), v(p.face));
    let nd = vnorm(dir);
    let rot = if melee { quat_yz(cross(cross(nd, face), nd), nd) } else { quat_yz(dir, face) };
    Some(Place {
        name: if left { mirror_name(def.name).unwrap_or(def.name) } else { def.name },
        comp: def.at.0,
        slot: def.at.1,
        pos,
        rot,
        reach: def.reach,
        pull: def.pull,
        pull_axis: def.pull.map(|_| [-nd[0], -nd[1], -nd[2]]),
        pull_local: if melee { [0.0, 0.0, -1.0] } else { [0.0, -1.0, 0.0] },
        melee,
        grab: def.grab.map(|g| if melee { g.1 } else { g.0 }),
    })
}
fn kind_of(type_name: &str) -> Option<&'static str> {
    Some(match type_name {
        "Wea_Handgun" | "Wea_Revolver" => "pistol",
        "Wea_Katana" | "Wea_Sword" | "Wea_LongBlade" | "Wea_Machete" | "Wea_Axe" | "Wea_OneHandedClub" | "Wea_Chainsword" => "blade",
        "Wea_Knife" | "Wea_ShortBlade" => "knife",
        "Wea_AssaultRifle" | "Wea_Rifle" | "Wea_PrecisionRifle" | "Wea_SniperRifle" | "Wea_Shotgun" | "Wea_ShotgunDual" | "Wea_SubmachineGun"
        | "Wea_LightMachineGun" | "Wea_HeavyMachineGun" | "Wea_TwoHandedClub" | "Wea_Hammer" => "long",
        _ => return None,
    })
}
fn is_melee_type(type_name: &str) -> bool {
    matches!(
        type_name,
        "Wea_Katana" | "Wea_Sword" | "Wea_LongBlade" | "Wea_Machete" | "Wea_Axe" | "Wea_OneHandedClub" | "Wea_Chainsword" | "Wea_Knife" | "Wea_ShortBlade"
            | "Wea_TwoHandedClub" | "Wea_Hammer"
    )
}
fn order(kind: &str) -> &'static [&'static str] {
    match kind {
        "pistol" => &["hip_r", "underarm_l"],
        "blade" => &["hip_l", "shoulder_r", "shoulder_l"],
        "long" => &["shoulder_r", "shoulder_l", "lower_back"],
        "knife" => &["chest_l", "thigh_r", "lower_back"],
        _ => &[],
    }
}
fn choices(kind: &str) -> &'static [&'static str] {
    match kind {
        "pistol" => &["hip_r", "thigh_r", "underarm_l"],
        "blade" => &["hip_l", "shoulder_r", "shoulder_l", "lower_back"],
        "long" => &["shoulder_r", "shoulder_l", "lower_back"],
        "knife" => &["chest_l", "thigh_r", "lower_back"],
        _ => &[],
    }
}
const HOLD_CLUB: Pose = Pose { p: [-0.0601, 0.0317, 0.0250], q: [-0.5199, 0.5508, 0.5681, 0.3219] };
const HOLD_PISTOL: Pose = Pose { p: [-0.0748, 0.0321, 0.0136], q: [-0.5061, 0.5157, 0.5756, 0.3829] };
const HOLD_EMPTY: Pose = Pose { p: [-0.0881, 0.0418, 0.0217], q: [-0.4709, 0.5140, 0.5078, 0.5061] };
fn hold_side(h: Pose, left: bool) -> Pose {
    if !left {
        return h;
    }
    Pose { p: [-h.p[0], -h.p[1], -h.p[2]], q: [h.q[3], h.q[2], -h.q[1], -h.q[0]] }
}
/// P.hold: the hold a weapon of this type takes in the hand (the left hand's mirrored).
fn type_hold(type_name: &str, melee: bool, left: bool) -> Pose {
    let h = match type_name {
        "Wea_Katana" => Pose { p: [-0.0647, 0.0320, 0.0294], q: [-0.6178, 0.3754, 0.6161, 0.3128] },
        "Wea_Machete" | "Wea_OneHandedClub" => HOLD_CLUB,
        "Wea_Handgun" => HOLD_PISTOL,
        "Wea_Revolver" => Pose { p: [-0.0761, 0.0468, 0.0113], q: [-0.4743, 0.5834, 0.4813, 0.4506] },
        _ if melee => HOLD_CLUB,
        _ => HOLD_PISTOL,
    };
    hold_side(h, left)
}

// ---------------------------------------------------------------- shells.lua
fn side_of(name: &str) -> Option<&'static str> {
    Some(match name {
        "hip_r" | "underarm_r" | "chest_r" => "r",
        "hip_l" | "underarm_l" | "chest_l" => "l",
        _ => return None,
    })
}
fn design(name: &str) -> Option<&'static str> {
    Some(match name {
        "underarm_l" | "underarm_r" => "underarm",
        "chest_l" | "chest_r" => "cknife",
        _ => return None,
    })
}
fn bind_chest(body: char) -> Pose {
    if body == 'w' {
        BIND_CHEST_W
    } else {
        BIND_CHEST_M
    }
}
fn harness_part(part: &str, side: &str, body: char) -> Option<(&'static str, Pose)> {
    HARNESS_PARTS.iter().find(|e| e.0 == part && e.1 == side && e.2.starts_with(body)).map(|e| (e.3, e.4))
}
fn shifted(part: &str, side: &str, pose: Pose) -> Pose {
    if part != "underarm" {
        return pose;
    }
    let d = if side == "l" { [-0.03, -0.01, 0.0] } else { [0.03, -0.01, 0.0] };
    Pose { p: add(pose.p, d), q: pose.q }
}
/// SH.place: a harness place moves into its holster (the weapon's fit there in the Chest bone's frame).
fn shell_place(place: Place, body: char) -> Place {
    let (Some(part), Some(side)) = (design(place.name), side_of(place.name)) else { return place };
    let Some((_, fit)) = harness_part(part, side, body) else { return place };
    let pose = compose(inverse(bind_chest(body)), shifted(part, side, fit));
    let mut c = place;
    c.pos = pose.p;
    c.rot = pose.q;
    if c.pull.is_some() {
        c.pull_axis = Some(qrot(pose.q, if c.melee { [0.0, 0.0, -1.0] } else { [0.0, -1.0, 0.0] }));
    }
    c
}
fn leg_kind(look: &str, type_name: &str) -> &'static str {
    if look.contains("Overture") {
        "overture"
    } else if look.contains("Silverhand") || look.contains("Malorian") {
        "malorian"
    } else if type_name == "Wea_Revolver" {
        "revolver"
    } else {
        "pistol"
    }
}
struct WantShell {
    ent: &'static str,
    comp: &'static str,
    slot: &'static str,
    pose: Pose,
}
/// SH.wanted: the shells the weapons on the body need.
fn shells_wanted(copies: &HashMap<u64, Copy>, body: char) -> HashMap<String, WantShell> {
    let mut out = HashMap::new();
    let mut harness = false;
    for c in copies.values() {
        let name = c.place.name;
        let side = side_of(name);
        if name == "hip_r" || name == "hip_l" {
            if c.kind == "pistol" {
                let (Some(side), kind) = (side, leg_kind(&c.look, &c.type_name)) else { continue };
                if let Some(spec) = LEG.iter().find(|e| e.0 == side && e.1 == kind && e.2.starts_with(body)) {
                    out.insert(format!("leg_{side}"), WantShell { ent: spec.3, comp: c.place.comp, slot: c.place.slot, pose: compose(Pose { p: c.place.pos, q: c.place.rot }, inverse(spec.4)) });
                }
            }
        } else if let (Some(part), Some(side)) = (design(name), side) {
            if let Some((ent, _)) = harness_part(part, side, body) {
                out.insert(
                    format!("{part}_{side}"),
                    WantShell { ent, comp: "ItemAttachmentSlots", slot: "Chest", pose: compose(inverse(bind_chest(body)), shifted(part, side, IDENTITY)) },
                );
                harness = true;
            }
        }
    }
    if harness {
        out.insert(
            "harness".into(),
            WantShell { ent: if body == 'w' { HARNESS_W } else { HARNESS_M }, comp: "ItemAttachmentSlots", slot: "Chest", pose: inverse(bind_chest(body)) },
        );
    }
    out
}

// ---------------------------------------------------------------- the state
#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    Place,
    Hand,
    Stow,
    Homing,
}
struct Copy {
    key: u64,
    id: Option<u64>,
    item: [u64; 2],
    look: String,
    place: Place,
    kind: &'static str,
    type_name: String,
    entity: Handle,
    attached: bool,
    shown: Option<bool>,
    mode: Option<Mode>,
    found_at: f64,
    built_by: Option<f64>,
    on_bone_at: Option<f64>,
    root: Handle,
    root_of: usize,
    hand_side: Option<i32>,
    pair: bool,
    draw_item: Option<[u64; 2]>,
    hold_key: (u64, bool),
    side: i32,
    until: f64,
    hold: Option<Pose>,
    real_at: Option<f64>,
    stow_from: Pose,
    stow_mouth: V3,
    stow_at: f64,
    pair_wait: bool,
    /// when its seat is measured again (seat_error), and whether the last measure found it away from its place
    check_at: f64,
    seat_off: bool,
    /// a melee weapon's handle from its grip to the pommel (melee_handle), and whether it was measured on the copy
    handle: f64,
    handle_measured: bool,
}
struct Shell {
    ent: &'static str,
    comp: &'static str,
    slot: &'static str,
    pose: Pose,
    id: Option<u64>,
    entity: Handle,
    attached: bool,
    root: Handle,
    root_of: usize,
    scene_hidden: Option<HashSet<u64>>,
}
struct Pull {
    key: u64,
    side: i32,
    h0: V3,
    s: f64,
    rel: Option<Pose>,
}
struct Release {
    side: i32,
    p: V3,
    q: Q,
    at: f64,
}
struct HiddenReal {
    id: Option<u64>,
    names: HashSet<u64>,
}
struct Prefs {
    slots: [Option<String>; 3],
    hidden: bool,
    /// read from holster_places.json (once, as holsters.lua's loadPrefs at load)
    loaded: bool,
}
#[derive(Default)]
struct Simple {
    cache_l: Option<&'static str>,
    cache_r: Option<&'static str>,
    since_scan: f64,
    grip_prev: bool,
    cooldown: f64,
}
struct State {
    t: f64,
    scan_at: f64,
    cooldown: f64,
    copies: HashMap<u64, Copy>,
    shells: HashMap<String, Shell>,
    pull: Option<Pull>,
    grasp: [f64; 2],
    grasp_want: [f64; 2],
    magnet: Option<i32>,
    held: Option<(V3, Q)>,
    release: Option<Release>,
    holds: HashMap<(u64, bool), Pose>,
    in_hand_at: HashMap<u64, f64>,
    hold_read: HashMap<u64, Option<Pose>>,
    grip_prev: [bool; 2],
    looks: HashMap<u64, String>,
    hidden_real: HashMap<u64, HiddenReal>,
    scene: bool,
    mounted_at: f64,
    mounted: bool,
    prefs: Prefs,
    simple: Simple,
    error_at: f64,
    reported: bool,
}
// SAFETY: the handles are made, used and dropped on the game's main thread, where the mechanics tick.
unsafe impl Send for State {}
static STATE: Mutex<Option<State>> = Mutex::new(None);
/// The belt's lit grenade holder per hand (VRBeltGrenadeReach): the grip is the grenade's.
static BELT_REACH: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
/// VRHolsterRelease asked (the player detaching).
static RELEASE_ASKED: AtomicU8 = AtomicU8::new(0);
static SCAN_REPORT: super::PerPlayer = super::PerPlayer::new();
static DIAG_PD: super::PerPlayer = super::PerPlayer::new();
static DIAG_EMPTY: super::PerPlayer = super::PerPlayer::new();

// ---------------------------------------------------------------- calls
fn m<T: crate::script_call::Ret>(o: &Handle, name: &str, values: &[Value]) -> R<T> {
    if o.is_null() {
        return Err(format!("{name}: no object"));
    }
    // SAFETY: the caller's live object, held.
    unsafe { method::<T>(o.instance, name, values) }.ok_or_else(|| format!("{name} failed"))
}
fn h(o: &Handle, name: &str, values: &[Value]) -> Handle {
    if o.is_null() {
        return Handle::null();
    }
    // SAFETY: as above.
    unsafe { call_handle(o.instance, name, values) }.unwrap_or_else(Handle::null)
}
fn native<T: crate::script_call::Ret>(name: &str, values: &[Value]) -> Option<T> {
    // SAFETY: one of the plugin's own natives, called as the script VM calls it.
    unsafe { call_native::<T>(name, values) }
}
fn st<T: crate::script_call::Ret>(class: &str, name: &str, values: &[Value]) -> Option<T> {
    // SAFETY: a static function (no context).
    crate::script_call::static_function_for(class, name, values).and_then(|f| unsafe { invoke::<T>(core::ptr::null_mut(), f, values) })
}
fn st_handle(class: &str, name: &str, values: &[Value]) -> Handle {
    // SAFETY: as above.
    crate::script_call::static_function_for(class, name, values).and_then(|f| unsafe { invoke_handle(core::ptr::null_mut(), f, values) }).unwrap_or_else(Handle::null)
}
fn shared(i: i32) -> f32 {
    // SAFETY: the shared block's read.
    unsafe { cvr_rig::vrik::provider::shared_slot(i) }
}
fn cn(s: &str) -> Value {
    Value::U64(CName::hashed(s).0)
}
fn v4(v: V3, w: f32) -> [f32; 4] {
    [v[0] as f32, v[1] as f32, v[2] as f32, w]
}
fn f3(v: [f32; 4]) -> V3 {
    [f64::from(v[0]), f64::from(v[1]), f64::from(v[2])]
}
fn f4(v: [f32; 4]) -> Q {
    [f64::from(v[0]), f64::from(v[1]), f64::from(v[2]), f64::from(v[3])]
}
fn player() -> Handle {
    let mut p = Handle::null();
    // SAFETY: GetPlayer(GameInstance) into a handle of ours.
    unsafe { crate::anim_lookup::get_player(&mut p) };
    p
}
fn is_valid(id: &[u64; 2]) -> bool {
    st::<bool>("ItemID", "IsValid", &[Value::Ptr(id.as_ptr().cast())]).unwrap_or(false)
}
fn item_key(id: &[u64; 2]) -> u64 {
    st::<u64>("ItemID", "GetCombinedHash", &[Value::Ptr(id.as_ptr().cast())]).unwrap_or(id[0])
}
fn draw_left() -> bool {
    native::<bool>("VRLeftHanded", &[]) == Some(true)
}
fn record_name(id: &[u64; 2]) -> String {
    super::weapon::tdbid_name(id[0]).unwrap_or_else(|| format!("<{:#x}>", id[0]))
}
fn components(e: &Handle) -> Vec<Handle> {
    crate::engine_objects::entity_components(e)
}
fn is_a(c: &Handle, class: &str) -> bool {
    m::<bool>(c, "IsA", &[cn(class)]).unwrap_or(false)
}
fn visual(c: &Handle) -> bool {
    is_a(c, "entIVisualComponent") || is_a(c, "entLightComponent")
}
fn comp_name(c: &Handle) -> u64 {
    m::<u64>(c, "GetName", &[]).unwrap_or(0)
}
fn entity_id(o: &Handle) -> Option<u64> {
    m::<u64>(o, "GetEntityID", &[]).ok()
}

// ---------------------------------------------------------------- prefs (holster_places.json, the menu's)
fn prefs_path() -> std::path::PathBuf {
    cvr_host::paths::file(cvr_host::paths::Folder::Lua, "CyberpunkVRPort_Holster\\holster_places.json")
}
/// holsters.lua's loadPrefs: the file read once, at the first use, as the mod read it at load. The menu changes the
/// prefs in memory and writes them through (set_place, set_hidden); nothing else writes the file, so it is not looked
/// at again (a stat of it every frame was game-thread time for nothing).
fn load_prefs(p: &mut Prefs) {
    if p.loaded {
        return;
    }
    p.loaded = true;
    let Ok(text) = std::fs::read_to_string(prefs_path()) else { return };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else { return };
    p.slots = [None, None, None];
    if let Some(slots) = json.get("slots").and_then(|s| s.as_object()) {
        for (k, v) in slots {
            if let (Ok(i), Some(v)) = (k.parse::<usize>(), v.as_str()) {
                if (1..=3).contains(&i) {
                    p.slots[i - 1] = Some(v.to_string());
                }
            }
        }
    }
    p.hidden = json.get("hidden").and_then(|v| v.as_bool()) == Some(true);
}
/// The place chosen for a weapon slot's weapon of `kind`: a place, "none", or None for the automatic one.
fn chosen(p: &Prefs, slot: i32, kind: &str) -> Option<&'static str> {
    let c = p.slots.get(slot as usize)?.as_deref()?;
    if c == "auto" {
        return None;
    }
    if c == "none" {
        return Some("none");
    }
    choices(kind).iter().find(|n| **n == c).copied()
}

// ---------------------------------------------------------------- the body's frames
#[repr(C, align(16))]
struct WorldTransform([u32; 8]);
/// A slot's pose in the world.
fn slot_world(pl: &Handle, comp: &str, slot: &str) -> Option<(V3, Q)> {
    let c = h(pl, "FindComponentByName", &[cn(comp)]);
    let mut wt = WorldTransform([0; 8]);
    if m::<bool>(&c, "GetSlotTransform", &[cn(slot), Value::Out((&mut wt as *mut WorldTransform).cast())]) != Ok(true) {
        return None;
    }
    let p = [0, 1, 2].map(|i| f64::from(wt.0[i] as i32) / 131072.0);
    let q = [4, 5, 6, 7].map(|i| f64::from(f32::from_bits(wt.0[i])));
    Some((p, qnorm(q)))
}
fn hand_slot_name(side: i32) -> &'static str {
    if side == 0 {
        "WeaponLeft"
    } else {
        "WeaponRight"
    }
}
fn world_to_model(p: V3) -> Option<V3> {
    let v = v4(p, 1.0);
    native::<[f32; 4]>("VRWorldToModel", &[Value::Ptr(v.as_ptr().cast())]).map(f3)
}
/// A world rotation in the player's model space: its axes carried over.
fn model_q(q: Q) -> Option<Q> {
    let mut ax = [[0.0; 3]; 3];
    for (i, a) in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]].iter().enumerate() {
        let w = v4(qrot(q, *a), 0.0);
        ax[i] = vnorm(f3(native::<[f32; 4]>("VRWorldDirToModel", &[Value::Ptr(w.as_ptr().cast())])?));
    }
    Some(qnorm(from_axes(ax[0], ax[1], ax[2])))
}
/// The hand slot's (or `slot`'s) pose in the wrist's frame.
fn slot_in_wrist(pl: &Handle, side: i32, slot: Option<&str>) -> Option<Pose> {
    let (sp, sq) = slot_world(pl, "ItemAttachmentSlots", slot.unwrap_or(hand_slot_name(side)))?;
    let wp = f3(native::<[f32; 4]>("VRBoneModelPos", &[Value::Int32(WRIST[side as usize])])?);
    let wq = qnorm(f4(native::<[f32; 4]>("VRBoneModelRot", &[Value::Int32(WRIST[side as usize])])?));
    let mp = world_to_model(sp)?;
    let smq = model_q(sq)?;
    Some(Pose { p: qrot(qconj(wq), sub(mp, wp)), q: qmul(qconj(wq), smq) })
}
/// A copy's pose in the world, `s` metres drawn out of its place.
fn copy_world(pl: &Handle, c: &Copy, s: f64) -> Option<(V3, Q)> {
    let (bp, bq) = slot_world(pl, c.place.comp, c.place.slot)?;
    let mut lp = c.place.pos;
    if let (true, Some(ax)) = (s > 0.0, c.place.pull_axis) {
        lp = add(lp, scale(ax, s));
    }
    Some((add(bp, qrot(bq, lp)), qmul(bq, c.place.rot)))
}

// ---------------------------------------------------------------- the wrist and the fingers
fn claim(side: Option<i32>) {
    native::<()>("SetVRReloadOwnedHand", &[Value::Int32(side.unwrap_or(-1))]);
}
fn stop_model(side: i32, on: bool, p: V3) {
    let v = v4(p, if on { 1.0 } else { 0.0 });
    native::<bool>("VRHandStopModel", &[Value::Int32(side), Value::Bool(on), Value::Ptr(v.as_ptr().cast())]);
}
fn stop_rot(side: i32, on: i32, q: Q) {
    native::<bool>("VRHandStopRot", &[Value::Int32(side), Value::Int32(on), Value::Float(q[0] as f32), Value::Float(q[1] as f32), Value::Float(q[2] as f32), Value::Float(q[3] as f32)]);
}
/// The wrist where it holds a weapon whose root is at (p, q) in the world.
fn magnet_to(s: &mut State, side: i32, rel: Pose, p: V3, q: Q) {
    let (Some(m), Some(mq)) = (world_to_model(p), model_q(q)) else { return };
    let wq = qmul(mq, qconj(rel.q));
    let mp = sub(m, qrot(wq, rel.p));
    claim(Some(side));
    stop_model(side, true, mp);
    stop_rot(side, 1, wq);
    (s.magnet, s.held) = (Some(side), Some((mp, wq)));
}
fn magnet_off(s: &mut State) {
    s.release = None;
    let Some(side) = s.magnet.take() else { return };
    claim(None);
    stop_model(side, false, [0.0; 3]);
    stop_rot(side, 0, [0.0, 0.0, 0.0, 1.0]);
}
fn release_wrist(s: &mut State, side: i32) {
    match (s.magnet, s.held) {
        (Some(_), Some((p, q))) => s.release = Some(Release { side, p, q, at: s.t }),
        _ => magnet_off(s),
    }
}
fn release_tick(s: &mut State) {
    let Some(r) = &s.release else { return };
    let (side, rp, rq, at) = (r.side, r.p, r.q, r.at);
    let mut u = (s.t - at) / RELEASE_TIME;
    let p = native::<[f32; 4]>("VRHandRawModel", &[Value::Int32(side)]);
    let q = native::<[f32; 4]>("VRHandRawRot", &[Value::Int32(side)]);
    let (Some(p), Some(q)) = (p, q) else { return magnet_off(s) };
    if u >= 1.0 || p[3] < 0.5 {
        return magnet_off(s);
    }
    u = u * u * (3.0 - 2.0 * u);
    let pos = lerp3(rp, f3(p), u);
    let rot = qslerp(rq, qnorm(f4(q)), u);
    claim(Some(side));
    stop_model(side, true, pos);
    stop_rot(side, 1, rot);
}
/// The fingers round the grip: the plugin's grasp on a bar in the hand's slot, set in the frame of the rig's own slot.
fn grasp_on(s: &mut State, side: i32, melee: bool, hold: Option<Pose>, now: Pose) {
    let (mut lp, mut lq) = ([0.0, 0.0, -0.02], [0.0, 0.0, 0.0, 1.0]);
    let (mut hx, mut hy, mut hz) = (0.014, 0.014, 0.05);
    if !melee {
        (lp, lq) = ([0.0, -0.012, -0.035], [-0.139, 0.0, 0.0, 0.990]);
        (hx, hy, hz) = (0.013, 0.02, 0.045);
    }
    if let Some(hold) = hold {
        let iq = qconj(now.q);
        let r = qrot(hold.q, lp);
        lp = qrot(iq, add(sub(hold.p, now.p), r));
        lq = qnorm(qmul(qmul(iq, hold.q), lq));
    }
    let (p, q) = (v4(lp, 1.0), [lq[0] as f32, lq[1] as f32, lq[2] as f32, lq[3] as f32]);
    native::<bool>(
        "VRGraspSet",
        &[Value::Int32(side), Value::Int32(0), Value::Float(hx), Value::Float(hy), Value::Float(hz), Value::Ptr(p.as_ptr().cast()), Value::Ptr(q.as_ptr().cast())],
    );
    native::<bool>("VRGraspPin", &[Value::Int32(side), Value::Bool(false)]);
    s.grasp_want[side as usize] = 1.0;
}
fn grasp_off(s: &mut State, side: i32) {
    s.grasp_want[side as usize] = 0.0;
}
fn grasp_tick(s: &mut State, dt: f64) {
    for side in 0..2usize {
        let (b, w) = (s.grasp[side], s.grasp_want[side]);
        if b != w {
            let step = dt / if w > b { 0.10 } else { 0.18 };
            let b = if w > b { w.min(b + step) } else { w.max(b - step) };
            s.grasp[side] = b;
            native::<bool>("VRGraspBlend", &[Value::Int32(side as i32), Value::Float(b as f32)]);
            if b <= 0.0 {
                native::<bool>("VRGraspClear", &[Value::Int32(side as i32)]);
            }
        }
    }
}

// ---------------------------------------------------------------- what is in the hands
fn player_data(pl: &Handle) -> Handle {
    let es = h(&super::game_system("GetScriptableSystemsContainer"), "Get", &[cn("EquipmentSystem")]);
    h(&es, "GetPlayerData", &[Value::Handle(pl)])
}
/// The items the hands hold (their weapon objects), and the ones the equipment system is putting there.
fn in_hands(pl: &Handle, pd: &Handle) -> (HashSet<u64>, HashMap<u64, Handle>) {
    let (mut out, mut real) = (HashSet::new(), HashMap::new());
    let ts = super::game_system("GetTransactionSystem");
    for slot in ["AttachmentSlots.WeaponRight", "AttachmentSlots.WeaponLeft"] {
        let obj = h(&ts, "GetItemInSlot", &[Value::Handle(pl), Value::U64(red4ext_abi::tweakdbid::from_name(slot))]);
        if let Ok(id) = m::<[u64; 2]>(&obj, "GetItemID", &[]) {
            let k = item_key(&id);
            out.insert(k);
            real.insert(k, obj);
        }
    }
    for side in ["Right", "Left"] {
        let Some(v) = crate::script_call::enum_value("EquipmentManipulationRequestSlot", side) else { continue };
        if let Ok(id) = m::<[u64; 2]>(pd, "GetSlotActiveItem", &[Value::U64(v as u64)]) {
            if is_valid(&id) {
                out.insert(item_key(&id));
            }
        }
    }
    (out, real)
}
/// Which of n appearances an item's seed picks: the engine's PCG output step.
fn seed_pick(seed: u32, n: usize) -> usize {
    let v = 0x5851_F42D_4C95_7F2Du64.wrapping_mul(u64::from(seed)).wrapping_add(1);
    let a = (v >> 27) as u32;
    let b = (v >> 45) as u32;
    let r = (v >> 59) as u32;
    let u = a ^ b;
    let rot = (u >> r) | u.checked_shl(31 - r).unwrap_or(0);
    rot as usize % n
}
fn item_record(id: &[u64; 2]) -> Handle {
    st_handle("TweakDBInterface", "GetItemRecord", &[Value::U64(id[0])])
}
/// The copy's appearance: "<factory key>__<appearance>".
fn look_for(s: &mut State, id: &[u64; 2], k: u64) -> Option<String> {
    if let Some(l) = s.looks.get(&k) {
        return Some(l.clone());
    }
    let rec = item_record(id);
    if rec.is_null() {
        return None;
    }
    let res = super::weapon::flat_u64(&format!("{}.appearanceResourceName", record_name(id))).map(|n| CName(n).text().to_string())?;
    let rows = LOOKS.iter().find(|e| e.0 == res)?.1;
    // SAFETY: the record, held; the array result kept while read.
    let tags: Vec<String> = unsafe { crate::script_call::method_owned(rec.instance, "VisualTags", &[]) }
        .map(|o| {
            // SAFETY: an array of CName.
            let arr = unsafe { &*(o.as_ptr() as *const red4ext_abi::dynarray::DynArray<u64>) };
            arr.as_slice().iter().map(|n| CName(*n).text().to_string()).collect()
        })
        .unwrap_or_default();
    let fit: Vec<&str> = rows.iter().filter(|r| tags.iter().all(|t| r.1.contains(&t.as_str()))).map(|r| r.0).collect();
    let mut name = "default";
    if !fit.is_empty() {
        let seed = st::<u32>("ItemID", "GetRngSeed", &[Value::Ptr(id.as_ptr().cast())]).unwrap_or(0);
        name = fit[seed_pick(seed, fit.len())];
    }
    let look = format!("{res}__{name}");
    s.looks.insert(k, look.clone());
    Some(look)
}
fn item_type_name(id: &[u64; 2]) -> Option<String> {
    let t = m::<i32>(&h(&item_record(id), "ItemType", &[]), "Type", &[]).ok()?;
    crate::script_call::enum_name("gamedataItemType", i64::from(t))
}

// ---------------------------------------------------------------- the copies
fn find_entity(id: Option<u64>) -> Handle {
    id.and_then(|id| super::find_entity(id).ok()).unwrap_or_else(Handle::null)
}
fn despawn_id(id: Option<u64>) {
    let e = find_entity(id);
    if e.is_null() {
        return;
    }
    native::<bool>("VRDetachFromSlot", &[Value::Handle(&e)]);
    super::ent_spawn::despawn(&e);
}
/// The entity's visual and light components switched: how many it has (none until its appearance has loaded).
fn show_entity(e: &Handle, on: bool) -> usize {
    let mut n = 0;
    for c in components(e) {
        if visual(&c) {
            let _ = m::<()>(&c, "Toggle", &[Value::Bool(on)]);
            n += 1;
        }
    }
    n
}
fn hide_real(s: &mut State, k: u64, obj: &Handle) {
    let id = entity_id(obj);
    let rec = s.hidden_real.entry(k).or_insert_with(|| HiddenReal { id, names: HashSet::new() });
    if rec.id != id {
        *rec = HiddenReal { id, names: HashSet::new() };
    }
    for c in components(obj) {
        if visual(&c) && m::<bool>(&c, "IsEnabled", &[]).unwrap_or(false) {
            rec.names.insert(comp_name(&c));
            let _ = m::<()>(&c, "Toggle", &[Value::Bool(false)]);
        }
    }
}
fn unhide_real(rec: &HiddenReal, obj: &Handle) {
    if rec.id.is_none() || entity_id(obj) != rec.id {
        return;
    }
    for c in components(obj) {
        if rec.names.contains(&comp_name(&c)) {
            let _ = m::<()>(&c, "Toggle", &[Value::Bool(true)]);
        }
    }
}
fn hide_kept(e: &Handle) -> HashSet<u64> {
    let mut names = HashSet::new();
    for c in components(e) {
        if visual(&c) && m::<bool>(&c, "IsEnabled", &[]).unwrap_or(false) {
            names.insert(comp_name(&c));
            let _ = m::<()>(&c, "Toggle", &[Value::Bool(false)]);
        }
    }
    names
}
fn show_kept(e: &Handle, names: &HashSet<u64>) {
    for c in components(e) {
        if names.contains(&comp_name(&c)) {
            let _ = m::<()>(&c, "Toggle", &[Value::Bool(true)]);
        }
    }
}
fn show(c: &mut Copy, on: bool) {
    if c.entity.is_null() || c.shown == Some(on) {
        return;
    }
    show_entity(&c.entity, on);
    c.shown = Some(on);
}
/// The entity's root, as the engine picks it: its placeholder, else its only placed component without a parent.
fn root_of(e: &Handle) -> Handle {
    let r = h(e, "FindComponentByName", &[cn("entPlaceholderComponent")]);
    if !r.is_null() {
        return r;
    }
    let mut found = Handle::null();
    for c in components(e) {
        // SAFETY: the component's handle property, read in place.
        let no_parent = unsafe { crate::script_call::property(c.instance, "parentTransform") }.is_none_or(|p| unsafe { (*(p as *const Handle)).is_null() });
        if is_a(&c, "entIPlacedComponent") && no_parent {
            if !found.is_null() {
                return Handle::null();
            }
            found = c;
        }
    }
    found
}
fn set_local_on(entity: &Handle, root: &mut Handle, root_for: &mut usize, p: V3, q: Q) -> bool {
    if *root_for != entity.instance as usize || root.is_null() {
        *root = root_of(entity);
        *root_for = entity.instance as usize;
    }
    if root.is_null() {
        return false;
    }
    let (pv, qv) = (v4(p, 1.0), [q[0] as f32, q[1] as f32, q[2] as f32, q[3] as f32]);
    m::<()>(root, "SetLocalTransform", &[Value::Ptr(pv.as_ptr().cast()), Value::Ptr(qv.as_ptr().cast())]).is_ok()
}
fn set_local(c: &mut Copy, p: V3, q: Q) -> bool {
    set_local_on(&c.entity.clone(), &mut c.root, &mut c.root_of, p, q)
}
/// The copy where its place puts it, `s` metres drawn out along the pull axis.
fn seat(c: &mut Copy, s: f64) -> bool {
    let mut p = c.place.pos;
    if let (true, Some(ax)) = (s > 0.0, c.place.pull_axis) {
        p = add(p, scale(ax, s));
    }
    let rot = c.place.rot;
    set_local(c, p, rot)
}
fn attach_to(entity: &Handle, comp: &str, slot: &str) -> bool {
    let pl = player();
    let c = h(&pl, "FindComponentByName", &[cn(comp)]);
    if c.is_null() {
        return false;
    }
    native::<bool>("VRDetachFromSlot", &[Value::Handle(entity)]);
    native::<bool>("VRAttachToSlot", &[Value::Handle(entity), Value::Handle(&c), cn(slot)]) == Some(true)
}
fn attach(c: &mut Copy) -> bool {
    if !attach_to(&c.entity, c.place.comp, c.place.slot) {
        return false;
    }
    c.mode = Some(Mode::Place);
    seat(c, 0.0)
}
/// Spawned out of sight, under the floor, until it is on its bone.
fn spawn_entity(template: &str, look: Option<&str>) -> Option<u64> {
    let pl = player();
    let p = m::<[f32; 4]>(&pl, "GetWorldPosition", &[]).ok()?;
    let at = [f64::from(p[0]), f64::from(p[1]), f64::from(p[2]) - 10.0];
    let look = look.map(|l| CName::hashed(l).0).unwrap_or(0);
    super::ent_spawn::spawn(template, at, [0.0, 0.0, 0.0, 1.0], look)
}
fn spawn_copy(c: &mut Copy) {
    c.id = spawn_entity(ENT, Some(&c.look));
    (c.entity, c.attached, c.shown, c.mode) = (Handle::null(), false, None, None);
}

// ---------------------------------------------------------------- what hangs where
struct Item {
    id: [u64; 2],
    slot: i32,
}
/// EquipmentSystemPlayerData.GetItemInEquipSlot(areaType, slotIndex): the public overload, by its area's type -- a
/// private GetItemInEquipSlot(equipAreaIndex: Int32, slotIndex: Int32) has as many parameters.
pub(super) fn equip_slot_item(pd: &Handle, area: u64, slot: i32) -> Option<[u64; 2]> {
    if pd.is_null() {
        return None;
    }
    // SAFETY: the player data, held.
    unsafe { crate::script_call::method_typed::<[u64; 2]>(pd.instance, "GetItemInEquipSlot", &["gamedataEquipmentArea"], &[Value::U64(area), Value::Int32(slot)]) }
}
fn equipped(pd: &Handle) -> Vec<Item> {
    let area = crate::script_call::enum_value("gamedataEquipmentArea", "Weapon").unwrap_or(0) as u64;
    (0..4)
        .filter_map(|i| equip_slot_item(pd, area, i).filter(is_valid).map(|id| Item { id, slot: i }))
        .collect()
}
fn dual_perk(pl: &Handle) -> bool {
    let data = st_handle("PlayerDevelopmentSystem", "GetData", &[Value::Handle(pl)]);
    let perk = crate::script_call::enum_value("gamedataNewPerkType", "Cool_Central_Milestone_2").unwrap_or(-1);
    m::<bool>(&data, "IsNewPerkBoughtAnyLevel", &[Value::U64(perk as u64)]).unwrap_or(false)
}
fn body(pl: &Handle) -> char {
    if m::<u64>(pl, "GetResolvedGenderName", &[]).is_ok_and(|n| CName(n).text() == "Female") {
        'w'
    } else {
        'm'
    }
}
struct Assigned {
    id: [u64; 2],
    place: Place,
    kind: &'static str,
    type_name: String,
    side: Option<i32>,
    pair: bool,
    draw: Option<[u64; 2]>,
}
/// The pair first, then the places chosen in the menu, then every other weapon its kind's first free one.
fn assign(prefs: &Prefs, items: &[Item], left: bool, body: char, perk: bool) -> Vec<Assigned> {
    let mut taken: HashSet<&str> = HashSet::new();
    let mut out: Vec<Assigned> = Vec::new();
    struct By {
        id: [u64; 2],
        slot: i32,
        type_name: String,
        kind: Option<&'static str>,
    }
    let by: Vec<By> = items
        .iter()
        .map(|it| {
            let type_name = item_type_name(&it.id).unwrap_or_default();
            By { id: it.id, slot: it.slot, kind: kind_of(&type_name), type_name }
        })
        .collect();
    let find = |slot: i32| by.iter().find(|b| b.slot == slot);
    let put = |out: &mut Vec<Assigned>, a: &By, place: Option<Place>, side: Option<i32>, pair: bool, draw: Option<[u64; 2]>| {
        if let (Some(place), Some(kind)) = (place, a.kind) {
            out.push(Assigned { id: a.id, place: shell_place(place, body), kind, type_name: a.type_name.clone(), side, pair, draw });
        }
    };
    let (r, l) = (find(0), find(3));
    let pair = perk && matches!((r, l), (Some(r), Some(l)) if r.kind == Some("pistol") && l.kind == Some("pistol"));
    if pair {
        let (r, l) = (r.unwrap_or(&by[0]), l.unwrap_or(&by[0]));
        taken.insert("hip_r");
        taken.insert("hip_l");
        put(&mut out, r, place_get("hip_r", false, false), Some(1), true, Some(r.id));
        put(&mut out, l, place_get("hip_r", true, false), Some(0), true, Some(r.id));
    }
    let mut auto: Vec<&By> = Vec::new();
    for it in items {
        let Some(a) = find(it.slot) else { continue };
        let Some(kind) = a.kind else { continue };
        if it.slot <= 2 && !(pair && it.slot == 0) && !order(kind).is_empty() {
            match chosen(prefs, it.slot, kind) {
                Some("none") => {}
                Some(c) if !taken.contains(c) => {
                    taken.insert(c);
                    put(&mut out, a, place_get(c, left, is_melee_type(&a.type_name)), None, false, None);
                }
                _ => auto.push(a),
            }
        }
    }
    for a in auto {
        for name in order(a.kind.unwrap_or("")) {
            if !taken.contains(name) {
                taken.insert(name);
                put(&mut out, a, place_get(name, left, is_melee_type(&a.type_name)), None, false, None);
                break;
            }
        }
    }
    // The chest knife sheath and the underarm holster are one spot on the harness: the knife goes to the other side.
    let (under, chest) = if left { ("underarm_r", "chest_r") } else { ("underarm_l", "chest_l") };
    let gun = out.iter().any(|a| a.place.name == under);
    if gun {
        if let Some(knife) = out.iter_mut().find(|a| a.place.name == chest) {
            if let Some(p) = place_get("chest_l", !left, true) {
                knife.place = shell_place(p, body);
            }
        }
    }
    out
}
fn scan_shells(s: &mut State, body: char) {
    let want = if s.prefs.hidden { HashMap::new() } else { shells_wanted(&s.copies, body) };
    let names: Vec<String> = s.shells.keys().cloned().collect();
    for name in names {
        let keep = match (want.get(&name), s.shells.get_mut(&name)) {
            (Some(w), Some(sh)) if w.ent == sh.ent && w.slot == sh.slot => {
                if w.pose != sh.pose {
                    (sh.pose, sh.attached) = (w.pose, false);
                }
                true
            }
            _ => false,
        };
        if !keep {
            if let Some(sh) = s.shells.remove(&name) {
                despawn_id(sh.id);
            }
        }
    }
    for (name, w) in want {
        if !s.shells.contains_key(&name) {
            let id = spawn_entity(w.ent, None);
            s.shells.insert(name, Shell { ent: w.ent, comp: w.comp, slot: w.slot, pose: w.pose, id, entity: Handle::null(), attached: false, root: Handle::null(), root_of: 0, scene_hidden: None });
        }
    }
}
fn settle_shells(s: &mut State) {
    let scene = s.scene;
    for sh in s.shells.values_mut() {
        if sh.entity.is_null() && sh.id.is_some() {
            sh.entity = find_entity(sh.id);
        }
        if !sh.entity.is_null() && !sh.attached {
            let e = sh.entity.clone();
            sh.attached = attach_to(&e, sh.comp, sh.slot) && set_local_on(&e, &mut sh.root, &mut sh.root_of, sh.pose.p, sh.pose.q);
        }
        if !sh.entity.is_null() {
            if scene && sh.scene_hidden.is_none() {
                sh.scene_hidden = Some(hide_kept(&sh.entity));
            } else if !scene {
                if let Some(names) = sh.scene_hidden.take() {
                    show_kept(&sh.entity, &names);
                }
            }
        }
    }
}
fn scan(s: &mut State, pl: &Handle, pd: &Handle, enabled: bool) {
    let left = draw_left();
    let body = body(pl);
    struct Want {
        id: [u64; 2],
        look: String,
        place: Place,
        kind: &'static str,
        type_name: String,
        side: Option<i32>,
        pair: bool,
        draw: Option<[u64; 2]>,
    }
    let mut want: HashMap<u64, Want> = HashMap::new();
    if enabled {
        let items = equipped(pd);
        let assigned = assign(&s.prefs, &items, left, body, dual_perk(pl));
        let mut seen: Vec<String> = Vec::new();
        for a in assigned {
            let k = item_key(&a.id);
            let look = look_for(s, &a.id, k);
            seen.push(format!("{} {} -> {}", record_name(&a.id), a.type_name, look.as_deref().unwrap_or("no look")));
            if let Some(look) = look {
                want.insert(k, Want { id: a.id, look, place: a.place, kind: a.kind, type_name: a.type_name, side: a.side, pair: a.pair, draw: a.draw });
            }
        }
        // Once per player: what the scan found (the menu's puppet, then the save's player).
        if items.is_empty() && DIAG_EMPTY.first() {
            let area = crate::script_call::enum_value("gamedataEquipmentArea", "Weapon");
            let first = equip_slot_item(pd, area.unwrap_or(0) as u64, 0);
            log!("[mechanics] holsters: scan from Rust -- no weapon in the equip slots (area {area:?}, slot 0 reads {first:?})\n");
        }
        if !items.is_empty() && SCAN_REPORT.first() {
            log!("[mechanics] holsters: scan from Rust -- {} weapon(s) equipped, {} carried: {}\n", items.len(), seen.len(), seen.join("; "));
        }
    }
    let keys: Vec<u64> = s.copies.keys().copied().collect();
    for k in keys {
        let Some(c) = s.copies.get(&k) else { continue };
        let pulling = s.pull.as_ref().is_some_and(|p| p.key == k);
        let busy = matches!(c.mode, Some(Mode::Hand | Mode::Stow | Mode::Homing)) || pulling;
        let drop = match want.get(&k) {
            None => true,
            Some(w) => !busy && (w.look != c.look || w.place != c.place),
        };
        if drop {
            if pulling {
                s.pull = None;
                magnet_off(s);
            }
            if let Some(c) = s.copies.remove(&k) {
                despawn_id(c.id);
            }
        }
    }
    for (k, w) in want {
        let c = s.copies.entry(k).or_insert_with(|| {
            let mut c = Copy {
                key: k,
                id: None,
                item: w.id,
                look: w.look.clone(),
                place: w.place.clone(),
                kind: w.kind,
                type_name: w.type_name.clone(),
                entity: Handle::null(),
                attached: false,
                shown: None,
                mode: None,
                found_at: 0.0,
                built_by: None,
                on_bone_at: None,
                root: Handle::null(),
                root_of: 0,
                hand_side: None,
                pair: false,
                draw_item: None,
                hold_key: (k, false),
                side: 1,
                until: 0.0,
                hold: None,
                real_at: None,
                stow_from: IDENTITY,
                stow_mouth: [0.0; 3],
                stow_at: 0.0,
                pair_wait: false,
                check_at: 0.0,
                seat_off: false,
                handle: type_handle(&w.type_name),
                handle_measured: false,
            };
            spawn_copy(&mut c);
            c
        });
        (c.hand_side, c.pair, c.draw_item) = (w.side, w.pair, w.draw);
        c.hold_key = (k, w.pair);
    }
    scan_shells(s, body);
}

// ---------------------------------------------------------------- the copy in the hand and on its way home
fn into_hand(c: &mut Copy, side: i32, hold: Option<Pose>, t: f64) -> bool {
    let bone = if side == 0 { "LeftHand" } else { "RightHand" };
    if !attach_to(&c.entity, "ItemAttachmentSlots", bone) {
        return false;
    }
    let b = slot_in_wrist(&player(), side, Some(bone));
    match (b, hold) {
        (Some(b), Some(hold)) => {
            let iq = qconj(b.q);
            set_local(c, qrot(iq, sub(hold.p, b.p)), qmul(iq, hold.q));
        }
        _ => {
            set_local(c, [0.0; 3], [0.0, 0.0, 0.0, 1.0]);
        }
    }
    (c.mode, c.side, c.until, c.hold, c.real_at) = (Some(Mode::Hand), side, t + IN_HAND_WAIT, hold, None);
    show(c, true);
    true
}
fn hold_of(s: &State, c: &Copy, left: bool) -> Pose {
    s.holds.get(&c.hold_key).copied().unwrap_or_else(|| type_hold(&c.type_name, c.place.melee, left))
}
fn side_of_copy(c: &Copy, draw_side: i32) -> i32 {
    c.hand_side.unwrap_or(draw_side)
}
fn partner_of(s: &State, c: &Copy) -> Option<u64> {
    if !c.pair {
        return None;
    }
    s.copies.values().find(|o| o.key != c.key && o.pair).map(|o| o.key)
}
fn in_hand(c: &Copy, held: &HashSet<u64>, real: &HashMap<u64, Handle>) -> bool {
    if c.pair {
        real.contains_key(&c.key)
    } else {
        held.contains(&c.key)
    }
}
fn learn_holds(s: &mut State, pl: &Handle, real: &HashMap<u64, Handle>, draw_side: i32) {
    let keys: Vec<u64> = s.copies.keys().copied().collect();
    for k in keys {
        let Some(c) = s.copies.get(&k) else { continue };
        let (hk, side, pull, mode) = (c.hold_key, side_of_copy(c, draw_side), c.place.pull.is_some(), c.mode);
        if !real.contains_key(&k) {
            s.in_hand_at.remove(&k);
            s.hold_read.remove(&k);
        } else if pull && !s.holds.contains_key(&hk) && mode == Some(Mode::Place) {
            let at = *s.in_hand_at.entry(k).or_insert(s.t);
            if s.t - at >= LEARN_AFTER {
                let r = slot_in_wrist(pl, side, None);
                let before = s.hold_read.get(&k).copied().flatten();
                if let (Some(r), Some(before)) = (r, before) {
                    if qangle(r.q, before.q) < 0.5f64.to_radians() {
                        s.holds.insert(hk, r);
                    }
                }
                s.hold_read.insert(k, r);
            }
        }
    }
}
fn home(c: &mut Copy) {
    if attach(c) {
        show(c, false);
    }
    c.mode = Some(Mode::Place);
}
/// Put away: the copy takes the weapon's pose in the hand and goes home over the holster's mouth.
fn stow(s: &mut State, pl: &Handle, k: u64, side: i32, weapon: Option<&Handle>) -> bool {
    let Some(c) = s.copies.get(&k) else { return false };
    let (Some((hp, hq)), Some((bp, bq))) = (slot_world(pl, "ItemAttachmentSlots", hand_slot_name(side)), slot_world(pl, c.place.comp, c.place.slot)) else {
        return false;
    };
    let iq = qconj(bq);
    let lp = qrot(iq, sub(hp, bp));
    let lq = qmul(iq, hq);
    let over = c.place.pull.unwrap_or(0.10);
    let mut mouth = c.place.pos;
    if let Some(ax) = c.place.pull_axis {
        mouth = add(mouth, scale(ax, over));
    }
    let t = s.t;
    if let Some(c) = s.copies.get_mut(&k) {
        (c.stow_from, c.stow_mouth, c.stow_at, c.mode) = (Pose { p: lp, q: lq }, mouth, t, Some(Mode::Stow));
        set_local(c, lp, lq);
        show(c, true);
    }
    if let Some(w) = weapon {
        hide_real(s, k, w);
    }
    true
}
fn reveal_real(s: &mut State, real: &HashMap<u64, Handle>) {
    let keys: Vec<u64> = s.hidden_real.keys().copied().collect();
    for k in keys {
        let c = s.copies.get(&k);
        let busy = c.is_some_and(|c| matches!(c.mode, Some(Mode::Stow | Mode::Homing | Mode::Hand)));
        if let (Some(obj), false) = (real.get(&k), busy) {
            if let Some(rec) = s.hidden_real.remove(&k) {
                unhide_real(&rec, obj);
            }
        } else if c.is_none() && !real.contains_key(&k) {
            s.hidden_real.remove(&k);
        }
    }
}
fn stow_tick(c: &mut Copy, t: f64) {
    let u = ((t - c.stow_at) / STOW_TIME).min(1.0);
    let rot = c.place.rot;
    let (pos, q) = if u < 0.6 {
        let a = u / 0.6;
        let a = a * a * (3.0 - 2.0 * a);
        (lerp3(c.stow_from.p, c.stow_mouth, a), qslerp(c.stow_from.q, rot, a))
    } else {
        let a = (u - 0.6) / 0.4;
        (lerp3(c.stow_mouth, c.place.pos, a), rot)
    };
    set_local(c, pos, q);
    if u >= 1.0 {
        c.mode = Some(Mode::Homing);
    }
}
/// Each copy found once spawned, put on its bone, then shown or hidden by what is in the hands.
fn settle(s: &mut State, held: &HashSet<u64>, real: &HashMap<u64, Handle>) {
    let keys: Vec<u64> = s.copies.keys().copied().collect();
    for k in keys {
        let (t, scene, hidden) = (s.t, s.scene, s.prefs.hidden);
        let pulling = s.pull.as_ref().is_some_and(|p| p.key == k);
        let mut graspoff: Option<i32> = None;
        let mut hide: Option<Handle> = None;
        {
            let Some(c) = s.copies.get_mut(&k) else { continue };
            if c.entity.is_null() && c.id.is_some() {
                c.entity = find_entity(c.id);
                c.found_at = t;
            }
            if !c.entity.is_null() && !c.attached {
                let n = show_entity(&c.entity, false);
                c.shown = Some(false);
                match c.on_bone_at {
                    None => {
                        if n > 0 && c.built_by.is_none() {
                            c.built_by = Some(t + BUILD_TIME);
                        }
                        if (c.built_by.is_some_and(|b| t >= b) || t - c.found_at > BUILD_LIMIT)
                            && attach_to(&c.entity, c.place.comp, c.place.slot)
                            && set_local(c, OUT_OF_SIGHT, [0.0, 0.0, 0.0, 1.0])
                        {
                            c.on_bone_at = Some(t);
                        }
                    }
                    Some(at) if t - at >= SEAT_DELAY => {
                        if c.place.melee && !c.handle_measured {
                            c.handle_measured = true;
                            if let Some(h) = super::hand_collision::melee_handle(&c.entity) {
                                c.handle = h;
                            }
                        }
                        c.mode = Some(Mode::Place);
                        c.attached = seat(c, 0.0);
                        // the entity's world position follows the seat a frame or more later
                        (c.check_at, c.seat_off) = (t + SEAT_CHECK_EVERY, false);
                    }
                    _ => {}
                }
            }
            if !c.entity.is_null() && c.attached {
                match c.mode {
                    Some(Mode::Hand) => {
                        if let Some(obj) = real.get(&k) {
                            let real_at = *c.real_at.get_or_insert(t);
                            graspoff = Some(c.side);
                            let now = slot_in_wrist(&player(), c.side, None);
                            let reached = match (now, c.hold) {
                                (Some(n), Some(hd)) => qangle(n.q, hd.q) < HOLD_REACHED,
                                _ => true,
                            };
                            if reached || t - real_at > HOLD_WAIT {
                                home(c);
                            } else {
                                hide = Some(obj.clone());
                            }
                        } else if t > c.until {
                            graspoff = Some(c.side);
                            home(c);
                        }
                    }
                    Some(Mode::Stow) => {
                        stow_tick(c, t);
                        show(c, !scene);
                        hide = real.get(&k).cloned();
                    }
                    Some(Mode::Homing) => {
                        show(c, !scene);
                        hide = real.get(&k).cloned();
                        if !(held.contains(&k) || in_hand(c, held, real)) || t - c.stow_at > IN_HAND_WAIT {
                            c.mode = Some(Mode::Place);
                        }
                    }
                    _ => {
                        let on = !scene && (pulling || (!in_hand(c, held, real) && !hidden));
                        show(c, on);
                        if !pulling && t >= c.check_at {
                            c.check_at = t + SEAT_CHECK_EVERY;
                            reseat_if_off(c);
                            reshow_if_off(c);
                        }
                    }
                }
                trace_copy(c, t, in_hand(c, held, real), scene, hidden);
            }
        }
        if let Some(side) = graspoff {
            grasp_off(s, side);
        }
        if let Some(obj) = hide {
            hide_real(s, k, &obj);
        }
    }
}

/// How far the copy's entity stands from where its place puts it on its bone (None when either is not known).
fn seat_error(c: &Copy) -> Option<f64> {
    let at = f3(m::<[f32; 4]>(&c.entity, "GetWorldPosition", &[]).ok()?);
    let (bp, bq) = slot_world(&player(), c.place.comp, c.place.slot)?;
    Some(dist(at, add(bp, qrot(bq, c.place.pos))))
}
/// A copy seated on its bone that is not there: the root its seat moved was taken while the appearance was still
/// being built (a revolver's 40-odd components), so the entity itself stayed out of sight and the holster hung empty
/// while the mechanics took the copy as shown. Off at two measures in a row (the entity's position follows a seat a
/// frame or more later), the root is looked up again and the copy seated anew.
fn reseat_if_off(c: &mut Copy) {
    let Some(off) = seat_error(c) else { return };
    if off <= SEAT_TOLERANCE {
        c.seat_off = false;
        return;
    }
    if !c.seat_off {
        c.seat_off = true;
        return;
    }
    c.seat_off = false;
    let before = c.root.instance as usize;
    (c.root, c.root_of) = (Handle::null(), 0);
    let seated = attach_to(&c.entity, c.place.comp, c.place.slot) && seat(c, 0.0);
    log!(
        "[mechanics] holsters: {} stood {off:.2} m from its place at {}: seated again (root {:#x} -> {:#x}, {seated})\n",
        c.look,
        c.place.name,
        before,
        c.root.instance as usize
    );
}

/// A copy taken as shown whose visual components are not all on (show() toggles only on a change of the wanted state,
/// so a component switched off or made later off stayed so): switched on again, the ones found off named in the log.
fn reshow_if_off(c: &mut Copy) {
    if c.shown != Some(true) {
        return;
    }
    let off: Vec<Handle> = components(&c.entity).into_iter().filter(|x| visual(x) && !m::<bool>(x, "IsEnabled", &[]).unwrap_or(true)).collect();
    if off.is_empty() {
        return;
    }
    let names: Vec<String> = off.iter().map(|x| CName(comp_name(x)).text().to_string()).collect();
    for x in &off {
        let _ = m::<()>(x, "Toggle", &[Value::Bool(true)]);
    }
    log!("[mechanics] holsters: {} at {} shown with {} visual component(s) off ({}): switched on\n", c.look, c.place.name, off.len(), names.join(", "));
}

/// CVR_HOLSTER_TRACE=1: each seated copy once, 2 s after it went on its bone -- where it is against its slot, its
/// appearance, its visual components (each one's class and whether it is enabled) and why it is shown or hidden.
fn trace_copy(c: &Copy, t: f64, in_hand: bool, scene: bool, hidden: bool) {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ON.get_or_init(|| std::env::var("CVR_HOLSTER_TRACE").is_ok_and(|v| v == "1")) {
        return;
    }
    if !c.on_bone_at.is_some_and(|at| t - at > 2.0) {
        return;
    }
    thread_local! {
        static SAID: std::cell::RefCell<HashSet<(u64, usize)>> = std::cell::RefCell::new(HashSet::new());
    }
    if !SAID.with(|s| s.borrow_mut().insert((c.key, c.entity.instance as usize))) {
        return;
    }
    let at = m::<[f32; 4]>(&c.entity, "GetWorldPosition", &[]).ok().map(f3);
    let slot = slot_world(&player(), c.place.comp, c.place.slot).map(|(p, _)| p);
    let apart = match (at, slot) {
        (Some(a), Some(b)) => format!("{:.3} m", dist(a, b)),
        _ => "?".into(),
    };
    let look = m::<u64>(&c.entity, "GetCurrentAppearanceName", &[]).map(|n| CName(n).text().to_string()).unwrap_or_default();
    let mut parts = Vec::new();
    let (mut shown_n, mut total) = (0, 0);
    for comp in components(&c.entity) {
        let class = red4ext_abi::rtti::RttiSystem::get()
            .and(unsafe { red4ext_abi::rtti::object_class(comp.instance) })
            .map(|k| k.class_name().text().to_string())
            .unwrap_or_default();
        let enabled = m::<bool>(&comp, "IsEnabled", &[]).unwrap_or(false);
        if visual(&comp) {
            total += 1;
            shown_n += usize::from(enabled);
        }
        parts.push(format!("{class}{}", if enabled { "" } else { " (off)" }));
    }
    log!(
        "[holster-trace] {} at {}: the copy at {:?}, its slot at {:?} ({} apart, {:?} m from its place), appearance {:?}, visual {}/{} enabled; mode {:?} shown {:?} in hand {} scene {} hidden {}; components: {}\n",
        c.look,
        c.place.name,
        at,
        slot,
        apart,
        seat_error(c),
        look,
        shown_n,
        total,
        c.mode,
        c.shown,
        in_hand,
        scene,
        hidden,
        parts.join(", ")
    );
}

// ---------------------------------------------------------------- the hand
fn hand_pos(left: bool) -> Option<V3> {
    let p = native::<[f32; 4]>("VRHandRawWorld", &[Value::Int32(if left { 0 } else { 1 })])?;
    (p[3] >= 0.5).then(|| f3(p))
}
fn reachable(c: &Copy) -> bool {
    !c.entity.is_null() && c.attached && matches!(c.mode, None | Some(Mode::Place))
}
/// A melee weapon's handle by its type, from the grip to the pommel, where its mesh family has no measured boxes
/// (melee_handle: the katana 0.312, the knives 0.10..0.114, the baton 0.213 m); the others by their kind's length.
fn type_handle(type_name: &str) -> f64 {
    match type_name {
        "Wea_Katana" | "Wea_Sword" | "Wea_LongBlade" => 0.31,
        "Wea_Knife" | "Wea_ShortBlade" => 0.11,
        "Wea_OneHandedClub" => 0.21,
        "Wea_Machete" => 0.15,
        "Wea_Chainsword" => 0.25,
        "Wea_Axe" => 0.30,
        "Wea_TwoHandedClub" => 0.40,
        "Wea_Hammer" => 0.45,
        _ => 0.15,
    }
}
/// Where a hand meets a copy: its origin, and the far end of what the hand may take it by -- a place's `grab` (the
/// stock or the pommel over a shoulder), else a melee weapon's whole handle: a grip anywhere on it takes the weapon,
/// the wrist then brought onto the weapon's hold (grab: magnet_to) and the draw pulled from there.
fn grab_span(pl: &Handle, c: &Copy) -> Option<(V3, Option<V3>)> {
    let o = f3(m::<[f32; 4]>(&c.entity, "GetWorldPosition", &[]).ok()?);
    let grab = c.place.grab.or_else(|| c.place.melee.then_some(c.handle));
    let Some(g) = grab else { return Some((o, None)) };
    let Some((_, wq)) = copy_world(pl, c, 0.0) else { return Some((o, None)) };
    let d = qrot(wq, c.place.pull_local);
    Some((o, Some(add(o, scale(d, g)))))
}
fn span_dist(p: V3, a: V3, b: Option<V3>) -> f64 {
    let Some(b) = b else { return dist(p, a) };
    let ab = sub(b, a);
    let l2 = ab[0] * ab[0] + ab[1] * ab[1] + ab[2] * ab[2];
    let mut t = 0.0;
    if l2 > 1e-9 {
        t = (((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1] + (p[2] - a[2]) * ab[2]) / l2).clamp(0.0, 1.0);
    }
    dist(p, add(a, scale(ab, t)))
}
fn draw(pl: &Handle, c: &Copy) {
    let es = h(&super::game_system("GetScriptableSystemsContainer"), "Get", &[cn("EquipmentSystem")]);
    let Some(class) = crate::script_call::class_any("DrawItemRequest") else { return };
    // SAFETY: a new instance owned by the handle made for it; its native properties written in place.
    let r = unsafe { Handle::from_instance(class.create_instance()) };
    let item = c.draw_item.unwrap_or(c.item);
    unsafe {
        if let Some(p) = crate::script_call::property(r.instance, "owner") {
            let old = std::ptr::replace(p as *mut WeakHandle, WeakHandle::from_handle(pl));
            drop(old);
        }
        if let Some(p) = crate::script_call::property(r.instance, "itemID") {
            *(p as *mut [u64; 2]) = item;
        }
    }
    let _ = m::<()>(&es, "QueueRequest", &[Value::Handle(&r)]);
    log!("[Holster] draw {} from {}{} (from Rust)\n", record_name(&item), c.place.name, if c.pair { " (the pair)" } else { "" });
}
fn active_weapon(pl: &Handle) -> Handle {
    st_handle("ScriptedPuppet", "GetActiveWeapon", &[Value::Handle(pl)])
}
/// The grip belongs elsewhere: a cigarette, a hand at the wheel, a lit belt grenade, a throw wound up.
fn grip_owned_elsewhere(pl: &Handle, left: bool) -> bool {
    if super::smoke::has_cig(pl) {
        return true;
    }
    let wheel = shared(163);
    if ((f64::from(wheel) / if left { 2.0 } else { 1.0 }).floor() as i64) % 2 == 1 {
        return true;
    }
    if BELT_REACH[usize::from(!left)].load(Ordering::Relaxed) {
        return true;
    }
    let w = active_weapon(pl);
    !w.is_null() && super::melee_throw::ready(pl, &w)
}
fn holster_held(pl: &Handle) {
    let w = active_weapon(pl);
    let melee = !w.is_null() && m::<bool>(&w, "IsMelee", &[]).unwrap_or(false);
    equip_helper(pl, if melee { "VRHolsterMelee" } else { "VRUnequipWeapon" });
    log!("[Holster] put away (from Rust)\n");
}
fn take_partner(s: &mut State, k: u64, side: i32, waits: bool) -> bool {
    let left = side == 0;
    let Some(c) = s.copies.get(&k) else { return false };
    let hold = hold_of(s, c, left);
    let t = s.t;
    let Some(c) = s.copies.get_mut(&k) else { return false };
    if !into_hand(c, side, Some(hold), t) {
        return false;
    }
    c.pair_wait = waits;
    let item = c.item;
    grasp_on(s, side, false, Some(hold), hold_side(HOLD_EMPTY, left));
    log!(
        "[Holster] the pair's {} into the {} hand{}\n",
        record_name(&item),
        if left { "left" } else { "right" },
        if waits { " (the other hand still pulling)" } else { "" }
    );
    true
}
fn end_pull(s: &mut State, k: Option<u64>) {
    if let Some(c) = k.and_then(|k| s.copies.get_mut(&k)) {
        if !c.entity.is_null() {
            seat(c, 0.0);
        }
    }
    if let Some(p) = s.pull.take() {
        grasp_off(s, p.side);
    }
    magnet_off(s);
    let keys: Vec<u64> = s.copies.keys().copied().collect();
    for k in keys {
        let mut off = None;
        if let Some(o) = s.copies.get_mut(&k) {
            if o.pair_wait {
                o.pair_wait = false;
                if o.mode == Some(Mode::Hand) {
                    off = Some(o.side);
                    home(o);
                }
            }
        }
        if let Some(side) = off {
            grasp_off(s, side);
        }
    }
}
fn pull_tick(s: &mut State, pl: &Handle, grip: bool) {
    let Some(p) = &s.pull else { return };
    let (side, key, h0, rel) = (p.side, p.key, p.h0, p.rel);
    let hp = hand_pos(side == 0);
    let alive = s.copies.get(&key).is_some_and(|c| !c.entity.is_null());
    let (Some(hp), true, true) = (hp, alive, grip) else { return end_pull(s, Some(key)) };
    let Some(c) = s.copies.get(&key) else { return end_pull(s, Some(key)) };
    let ps = s.pull.as_ref().map(|p| p.s).unwrap_or(0.0);
    let Some((_, wq)) = copy_world(pl, c, ps) else { return end_pull(s, Some(key)) };
    let d = qrot(wq, c.place.pull_local);
    let full = c.place.pull.unwrap_or(0.0);
    let sv = ((hp[0] - h0[0]) * d[0] + (hp[1] - h0[1]) * d[1] + (hp[2] - h0[2]) * d[2]).clamp(0.0, full);
    if let Some(p) = s.pull.as_mut() {
        p.s = sv;
    }
    if let Some(c) = s.copies.get_mut(&key) {
        seat(c, sv);
    }
    let Some(c) = s.copies.get(&key) else { return };
    if let (Some((wp, wq)), Some(rel)) = (copy_world(pl, c, sv), rel) {
        magnet_to(s, side, rel, wp, wq);
    }
    if sv >= full * 0.95 {
        s.pull = None;
        release_wrist(s, side);
        let t = s.t;
        let ok = s.copies.get_mut(&key).is_some_and(|c| into_hand(c, side, rel, t));
        if !ok {
            grasp_off(s, side);
            if let Some(c) = s.copies.get_mut(&key) {
                seat(c, 0.0);
            }
        }
        for o in s.copies.values_mut() {
            if o.pair_wait {
                (o.pair_wait, o.until) = (false, t + IN_HAND_WAIT);
            }
        }
        if let Some(c) = s.copies.get(&key) {
            draw(pl, c);
        }
        s.cooldown = COOLDOWN;
    }
}
/// One hand's new grip: the copy nearest to it that this hand takes and puts back.
fn grab(s: &mut State, pl: &Handle, held: &HashSet<u64>, real: &HashMap<u64, Handle>, side: i32, draw_side: i32) {
    let left = side == 0;
    let Some(hp) = hand_pos(left) else { return };
    if grip_owned_elsewhere(pl, left) {
        return;
    }
    let mut nearest: Option<(u64, f64)> = None;
    let trace = std::env::var("CVR_HOLSTER_TRACE").is_ok_and(|v| v == "1");
    for c in s.copies.values() {
        if reachable(c) && side_of_copy(c, draw_side) == side {
            if let Some((a, b)) = grab_span(pl, c) {
                let d = span_dist(hp, a, b);
                if trace {
                    log!(
                        "[holster-trace] grip {side} at {hp:?}: {} at {} spans {a:?} .. {b:?}: {d:.3} m from it ({:.3} from its origin), reach {:.2}\n",
                        c.look,
                        c.place.name,
                        dist(hp, a),
                        c.place.reach
                    );
                }
                if d < c.place.reach && nearest.is_none_or(|n| d < n.1) {
                    nearest = Some((c.key, d));
                }
            }
        }
    }
    let Some((nk, _)) = nearest else { return };
    let Some(n) = s.copies.get(&nk) else { return };
    if n.pair {
        let other = partner_of(s, n);
        if let Some(p) = &s.pull {
            if other == Some(p.key) {
                take_partner(s, nk, side, true);
            }
            return;
        }
        if held.contains(&nk) && !in_hand(n, held, real) {
            take_partner(s, nk, side, false);
            return;
        }
    } else if s.pull.is_some() {
        return;
    }
    if s.cooldown > 0.0 {
        return;
    }
    let Some(n) = s.copies.get(&nk) else { return };
    if in_hand(n, held, real) {
        if !held.contains(&nk) {
            return;
        }
        let other = partner_of(s, n);
        stow(s, pl, nk, side, real.get(&nk));
        if let Some(ok) = other {
            if let Some(o) = s.copies.get(&ok) {
                if reachable(o) && real.contains_key(&ok) {
                    let os = side_of_copy(o, draw_side);
                    stow(s, pl, ok, os, real.get(&ok));
                }
            }
        }
        holster_held(pl);
        s.cooldown = COOLDOWN;
        return;
    }
    if n.place.pull.is_some() {
        let rel = Some(hold_of(s, n, left));
        let melee = n.place.melee;
        s.pull = Some(Pull { key: nk, side, h0: hp, s: 0.0, rel });
        grasp_on(s, side, melee, rel, hold_side(HOLD_EMPTY, left));
        if let (Some(c), Some(rel)) = (s.copies.get(&nk), rel) {
            if let Some((wp, wq)) = copy_world(pl, c, 0.0) {
                magnet_to(s, side, rel, wp, wq);
            }
        }
        return;
    }
    if let Some(c) = s.copies.get_mut(&nk) {
        show(c, false);
    }
    if let Some(c) = s.copies.get(&nk) {
        draw(pl, c);
    }
    s.cooldown = COOLDOWN;
}
fn hand(s: &mut State, pl: &Handle, held: &HashSet<u64>, real: &HashMap<u64, Handle>, enabled: bool) {
    let draw_side = if draw_left() { 0 } else { 1 };
    let mut grips = [false; 2];
    let mut edges = [false; 2];
    for side in 0..2usize {
        let g = shared(if side == 0 { 155 } else { 49 }) > 0.5;
        (grips[side], edges[side]) = (g, g && !s.grip_prev[side]);
        s.grip_prev[side] = g;
    }
    // SAMPLED EVEN WHILE SUSPENDED (main 0.2.4): neither a held press nor a missed release may be carried across a
    // scene, a mode switch or the player's detach.
    if !enabled {
        if let Some(k) = s.pull.as_ref().map(|p| p.key) {
            end_pull(s, Some(k));
        }
        return;
    }
    if let Some(p) = &s.pull {
        let g = grips[p.side as usize];
        pull_tick(s, pl, g);
    }
    for side in [draw_side, 1 - draw_side] {
        if edges[side as usize] && s.pull.as_ref().is_none_or(|p| p.side != side) {
            grab(s, pl, held, real, side, draw_side);
        }
    }
}
fn in_vehicle(s: &mut State, pl: &Handle) -> bool {
    if s.t - s.mounted_at >= 0.25 || s.t < s.mounted_at {
        s.mounted_at = s.t;
        static CACHE: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
        s.mounted = crate::script_call::global_by_short_name("GetMountedVehicle", &CACHE)
            // SAFETY: a global script function (no context).
            .and_then(|f| unsafe { invoke_handle(core::ptr::null_mut(), f, &[Value::Handle(pl)]) })
            .is_some_and(|v| !v.is_null());
    }
    s.mounted
}
/// Weapons may not be drawn here (the game's own DefaultTransition.IsNoCombatActionsForced: a NoCombat restriction,
/// not fast-forwarding, or a vehicle scene; and the state machine's safe zone): the holsters and the belt's grenades
/// are hidden and taken by no hand. Asked at most each quarter second.
pub(in crate::mechanics) fn weapons_forbidden(pl: &Handle) -> bool {
    use std::sync::atomic::AtomicU64;
    static AT: AtomicU64 = AtomicU64::new(0);
    static ANSWER: AtomicBool = AtomicBool::new(false);
    static SAID: AtomicU8 = AtomicU8::new(2);
    if pl.is_null() {
        return false;
    }
    // SAFETY: a plain OS call.
    let now = unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() };
    if now.saturating_sub(AT.load(Ordering::Relaxed)) < 250 {
        return ANSWER.load(Ordering::Relaxed);
    }
    AT.store(now, Ordering::Relaxed);
    let tag = |t: &str| super::blade::has_effect_tag(pl, t);
    let safe = super::blade::psm_int(pl, "Zones") == Some(super::lobby_rtti::enum_value("gamePSMZones", "Safe"));
    let forbidden = (tag("NoCombat") && !tag("FastForward")) || tag("VehicleScene") || safe;
    ANSWER.store(forbidden, Ordering::Relaxed);
    if SAID.swap(u8::from(forbidden), Ordering::Relaxed) != u8::from(forbidden) {
        log!("[Holster] weapons {} here: the holsters and grenades {} (from Rust)\n", if forbidden { "forbidden" } else { "allowed" }, if forbidden { "hidden" } else { "shown" });
    }
    forbidden
}
/// H.update.
fn update(s: &mut State, dt: f64, enabled: bool) -> R<()> {
    s.t += dt;
    s.cooldown = (s.cooldown - dt).max(0.0);
    release_tick(s);
    let pl = player();
    if pl.is_null() {
        hand(s, &pl, &HashSet::new(), &HashMap::new(), false);
        return Ok(());
    }
    let pd = player_data(&pl);
    if pd.is_null() {
        hand(s, &pl, &HashSet::new(), &HashMap::new(), false);
        if DIAG_PD.first() {
            let es = h(&super::game_system("GetScriptableSystemsContainer"), "Get", &[cn("EquipmentSystem")]);
            log!("[mechanics] holsters: no EquipmentSystem player data (the system {})\n", if es.is_null() { "not found" } else { "found" });
        }
        return Ok(());
    }
    load_prefs(&mut s.prefs);
    let (held, real) = in_hands(&pl, &pd);
    s.scene = native::<bool>("VRSceneOwnsBody", &[]) == Some(true) || in_vehicle(s, &pl) || weapons_forbidden(&pl);
    if s.t - s.scan_at >= SCAN_EVERY {
        s.scan_at = s.t;
        scan(s, &pl, &pd, enabled);
    }
    settle(s, &held, &real);
    reveal_real(s, &real);
    learn_holds(s, &pl, &real, if draw_left() { 0 } else { 1 });
    settle_shells(s);
    let enabled = enabled && !s.scene;
    hand(s, &pl, &held, &real, enabled);
    grasp_tick(s, dt);
    if !s.reported && s.copies.values().any(|c| c.attached) {
        s.reported = true;
        let on: Vec<String> = s.copies.values().map(|c| format!("{} at {}", c.look, c.place.name)).collect();
        log!("[mechanics] holsters: {} on the body from Rust ({}), {} shell(s)\n", s.copies.len(), on.join(", "), s.shells.len());
    }
    Ok(())
}
/// H.release: everything let go (the player detaching, the shutdown).
fn release(s: &mut State) {
    if let Some(k) = s.pull.as_ref().map(|p| p.key) {
        end_pull(s, Some(k));
    }
    let pl = player();
    if !pl.is_null() {
        let pd = player_data(&pl);
        if !pd.is_null() {
            let (_, real) = in_hands(&pl, &pd);
            for (k, rec) in &s.hidden_real {
                if let Some(obj) = real.get(k) {
                    unhide_real(rec, obj);
                }
            }
        }
    }
    s.hidden_real.clear();
    magnet_off(s);
    for side in 0..2usize {
        if s.grasp[side] > 0.0 {
            native::<bool>("VRGraspClear", &[Value::Int32(side as i32)]);
        }
        (s.grasp[side], s.grasp_want[side]) = (0.0, 0.0);
    }
    for c in s.copies.values() {
        despawn_id(c.id);
    }
    for sh in s.shells.values() {
        despawn_id(sh.id);
    }
    s.copies.clear();
    s.shells.clear();
    s.pull = None;
}

// ---------------------------------------------------------------- simple.lua
fn simple_classify(name: &str) -> Option<&'static str> {
    let lc = name.to_lowercase();
    if ["katana", "sword", "blade", "_eqh", "s10_military", "knife", "machete"].iter().any(|k| lc.contains(k)) {
        Some("melee")
    } else if ["pistol", "revolver", "holster"].iter().any(|k| lc.contains(k)) {
        Some("ranged")
    } else {
        None
    }
}
fn simple_classify_side(pl: &Handle, slots: &[&str]) -> Option<&'static str> {
    let ts = super::game_system("GetTransactionSystem");
    for slot in slots {
        let item = h(&ts, "GetItemInSlot", &[Value::Handle(pl), Value::U64(red4ext_abi::tweakdbid::from_name(slot))]);
        if let Ok(id) = m::<[u64; 2]>(&item, "GetItemID", &[]) {
            if let Some(c) = simple_classify(&record_name(&id)) {
                return Some(c);
            }
        }
    }
    None
}
fn simple_rescan(s: &mut Simple, pl: &Handle) {
    let l = simple_classify_side(pl, &["OutfitSlots.ThighLeft", "OutfitSlots.KneeLeft", "OutfitSlots.AnkleLeft"]);
    let r = simple_classify_side(pl, &["OutfitSlots.ThighRight", "OutfitSlots.KneeRight", "OutfitSlots.AnkleRight"]);
    (s.cache_l, s.cache_r) = if draw_left() { (r, l) } else { (l, r) };
}
/// The simple holsters: the hand-to-zone equip.
fn simple_update(s: &mut Simple, dt: f64) {
    s.cooldown = (s.cooldown - dt).max(0.0);
    let pl = player();
    if pl.is_null() || super::smoke::has_cig(&pl) {
        return;
    }
    let left = draw_left();
    let grip_slot = if left { 155 } else { 49 };
    if ((f64::from(shared(163)) / if left { 2.0 } else { 1.0 }).floor() as i64) % 2 == 1 {
        s.grip_prev = shared(grip_slot) > 0.5;
        return;
    }
    s.since_scan += dt;
    if s.since_scan >= 1.0 {
        s.since_scan = 0.0;
        simple_rescan(s, &pl);
    }
    let g = shared(grip_slot) > 0.5;
    let edge = g && !s.grip_prev;
    s.grip_prev = g;
    if !edge || s.cooldown > 0.0 {
        return;
    }
    if BELT_REACH[usize::from(!left)].load(Ordering::Relaxed) {
        return;
    }
    let w = active_weapon(&pl);
    if !w.is_null() && super::melee_throw::ready(&pl, &w) {
        return;
    }
    let d = |i: i32| {
        let v = f64::from(shared(i));
        if v < 0.0 {
            1e9
        } else {
            v
        }
    };
    let (dr, dl, db) = (d(20), d(21), d(22));
    let zone = if db < 0.25 && db + 0.03 < dl.min(dr) {
        "B"
    } else if dl < 0.40 && dl <= dr {
        "L"
    } else if dr < 0.25 {
        "R"
    } else {
        return;
    };
    let ts = super::game_system("GetTransactionSystem");
    let slot = if left { "AttachmentSlots.WeaponLeft" } else { "AttachmentSlots.WeaponRight" };
    let right_item = h(&ts, "GetItemInSlot", &[Value::Handle(&pl), Value::U64(red4ext_abi::tweakdbid::from_name(slot))]);
    let melee_in_hand = || m::<[u64; 2]>(&right_item, "GetItemID", &[]).ok().and_then(|id| simple_classify(&record_name(&id))) == Some("melee");
    let call = |name: &str| equip_helper(&pl, name);
    if shared(23) > 0.5 {
        log!("[Holster] simple grip zone={zone} dR={dr:.2} dL={dl:.2} dB={db:.2} hasWpn={} (from Rust)\n", !right_item.is_null());
        if !right_item.is_null() {
            call(if melee_in_hand() { "VRHolsterMelee" } else { "VRUnequipWeapon" });
            log!("[Holster] simple -> HOLSTER\n");
        } else {
            call(match zone {
                "B" => "VREquipPrimaryWeapon",
                "R" => "VREquipWeaponSlot2",
                _ => "VREquipWeaponSlot3",
            });
            log!("[Holster] simple -> equip slot for zone {zone}\n");
        }
        s.cooldown = 0.6;
        return;
    }
    simple_rescan(s, &pl);
    let cls = match zone {
        "B" => Some("primary"),
        "L" => s.cache_l,
        _ => s.cache_r,
    };
    let Some(cls) = cls else { return };
    log!("[Holster] grip zone={zone} cls={cls} dR={dr:.2} dL={dl:.2} dB={db:.2} hasWpn={}\n", !right_item.is_null());
    if !right_item.is_null() {
        call(if melee_in_hand() { "VRHolsterMelee" } else { "VRUnequipWeapon" });
    } else {
        call(match cls {
            "melee" => "VREquipMeleeWeapon",
            "primary" => "VREquipPrimaryWeapon",
            _ => "VREquipOneHandedRangedWeapon",
        });
    }
    s.cooldown = 0.6;
}

fn new_state() -> State {
    State {
        t: 0.0,
        scan_at: -1.0,
        cooldown: 0.0,
        copies: HashMap::new(),
        shells: HashMap::new(),
        pull: None,
        grasp: [0.0; 2],
        grasp_want: [0.0; 2],
        magnet: None,
        held: None,
        release: None,
        holds: HashMap::new(),
        in_hand_at: HashMap::new(),
        hold_read: HashMap::new(),
        grip_prev: [false; 2],
        looks: HashMap::new(),
        hidden_real: HashMap::new(),
        scene: false,
        mounted_at: -1.0,
        mounted: false,
        prefs: Prefs { slots: [None, None, None], hidden: false, loaded: false },
        simple: Simple::default(),
        error_at: -10.0,
        reported: false,
    }
}

/// The mod's onUpdate.
pub(in crate::mechanics) fn tick(dt: f32) {
    let dt = f64::from(dt);
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let s = guard.get_or_insert_with(new_state);
    if RELEASE_ASKED.swap(0, Ordering::Relaxed) != 0 {
        release(s);
    }
    let simple = shared(23) > 0.5;
    if let Err(e) = update(s, dt, !simple) {
        if s.t - s.error_at > 5.0 {
            s.error_at = s.t;
            log!("[Holster] {e} (from Rust)\n");
        }
    }
    if simple {
        simple_update(&mut s.simple, dt);
    }
}
/// VRHolsterRelease(): the player detaching -- the copies and shells let go.
pub(in crate::mechanics) fn ask_release() {
    RELEASE_ASKED.store(1, Ordering::Relaxed);
}
/// VRBeltGrenadeReach(left, right): the belt's lit grenade holders.
pub(in crate::mechanics) fn belt_reach(left: bool, right: bool) {
    BELT_REACH[0].store(left, Ordering::Relaxed);
    BELT_REACH[1].store(right, Ordering::Relaxed);
}
/// VRHolsterNear(x, y, z, side): H.near -- the distance from a model-space point to the nearest holstered weapon hand
/// `side` could take or put back there, within that place's reach; negative when none.
pub(in crate::mechanics) fn near(x: f32, y: f32, z: f32, side: i32) -> f32 {
    let guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let Some(s) = guard.as_ref() else { return -1.0 };
    let pl = player();
    if pl.is_null() {
        return -1.0;
    }
    let draw_side = if draw_left() { 0 } else { 1 };
    let p = [f64::from(x), f64::from(y), f64::from(z)];
    let mut best: Option<f64> = None;
    for c in s.copies.values() {
        if reachable(c) && side_of_copy(c, draw_side) == side {
            let Some((a, b)) = grab_span(&pl, c) else { continue };
            let (Some(ma), mb) = (world_to_model(a), b.and_then(world_to_model)) else { continue };
            let d = span_dist(p, ma, mb);
            if d < c.place.reach && best.is_none_or(|b| d < b) {
                best = Some(d);
            }
        }
    }
    best.map(|b| b as f32).unwrap_or(-1.0)
}
/// The mod's onShutdown.
pub(in crate::mechanics) fn shutdown() {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(s) = guard.as_mut() {
        release(s);
    }
}

// ---------------------------------------------------------------- the VR menu's side (holsters.lua H.slotInfo ...)
/// A weapon slot's holster as the menu's inventory shows it: its kind's family (the icons), the choice, the place it
/// hangs at now, the places its kind can take (with their names as shown, turned over for the left hand).
pub(super) struct SlotInfo {
    pub family: &'static str,
    pub choice: String,
    pub place: Option<String>,
    pub choices: Vec<(&'static str, String)>,
    pub auto: Option<String>,
    pub pair: bool,
}
fn with_prefs<R>(f: impl FnOnce(&mut Prefs, &mut f64) -> R) -> Option<R> {
    let mut g = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let s = g.as_mut()?;
    load_prefs(&mut s.prefs);
    let State { prefs, scan_at, .. } = s;
    Some(f(prefs, scan_at))
}
/// The immersive holsters' menu side is offered (shared [23] is 1 for the simple ones).
pub(super) fn menu_active() -> bool {
    f64::from(super::belt::shared(23)) <= 0.5 && STATE.lock().map(|g| g.is_some()).unwrap_or(false)
}
pub(super) fn slot_info(slot: i32) -> Option<SlotInfo> {
    let pl = super::belt::player();
    if pl.is_null() {
        return None;
    }
    let pd = player_data(&pl);
    let area = crate::script_call::enum_value("gamedataEquipmentArea", "Weapon").unwrap_or(0) as u64;
    let id = equip_slot_item(&pd, area, slot).filter(is_valid)?;
    let type_name = item_type_name(&id).unwrap_or_default();
    let kind = kind_of(&type_name)?;
    if choices(kind).is_empty() {
        return None;
    }
    let left = draw_left();
    let shown = |n: &str| -> String { if left { mirror_name(n).unwrap_or(n).to_string() } else { n.to_string() } };
    let (items, perk, body) = (equipped(&pd), dual_perk(&pl), body(&pl));
    with_prefs(|prefs, _| {
        let mut place = None;
        let mut pair = false;
        for a in assign(prefs, &items, left, body, perk) {
            if a.id == id {
                (place, pair) = (Some(a.place.name.to_string()), a.pair);
            }
        }
        let family = match kind {
            "pistol" => "gun",
            "blade" => "blade",
            "long" => "long",
            _ => "knife",
        };
        SlotInfo {
            family,
            choice: prefs.slots.get(slot as usize).cloned().flatten().unwrap_or_else(|| "auto".into()),
            place,
            choices: choices(kind).iter().map(|n| (*n, shown(n))).collect(),
            auto: order(kind).first().map(|n| shown(n)),
            pair,
        }
    })
}
fn save_prefs(p: &Prefs) -> bool {
    let mut slots = serde_json::Map::new();
    for i in 0..3 {
        slots.insert((i + 1).to_string(), serde_json::Value::String(p.slots[i].clone().unwrap_or_else(|| "auto".into())));
    }
    let v = serde_json::json!({ "slots": slots, "hidden": p.hidden });
    std::fs::write(prefs_path(), v.to_string()).is_ok()
}
/// A slot's choice ("auto", "none" or a place of its kind's); the copies move at the next scan.
pub(super) fn set_place(slot: i32, name: &str) -> bool {
    if !(0..=2).contains(&slot) {
        return false;
    }
    with_prefs(|p, scan_at| {
        p.slots[slot as usize] = Some(if name.is_empty() { "auto".into() } else { name.to_string() });
        *scan_at = -1.0;
        save_prefs(p)
    })
    .unwrap_or(false)
}
pub(super) fn hidden() -> bool {
    with_prefs(|p, _| p.hidden).unwrap_or(false)
}
pub(super) fn set_hidden(on: bool) -> bool {
    with_prefs(|p, scan_at| {
        p.hidden = on;
        *scan_at = -1.0;
        save_prefs(p)
    })
    .unwrap_or(false)
}

/// PlayerPuppet's equip helpers (formerly mods/redscript/CyberpunkVRPort_Holster: VREquipMeleeWeapon,
/// VREquipRangedWeapon, VREquipOneHandedRangedWeapon, VREquipPrimaryWeapon, VREquipWeaponSlot2 / 3, VRUnequipWeapon):
/// the action's EquipmentSystemWeaponManipulationRequest queued on the equipment system; VRHolsterMelee (formerly
/// CyberpunkVRPort_Engine's vrport_holster_shared.reds) also marks the holster's intent, which the melee reads.
pub(in crate::mechanics) fn equip_request(pl: &Handle, action: &str) {
    use super::lobby_rtti::{self as rt, A};
    let c = super::game_system("GetScriptableSystemsContainer");
    let equipment = rt::obj(&c, "Get", &[A::Name(red4ext_abi::cname::CName::hashed("EquipmentSystem").0)]);
    let req = super::lobby_ink::new("EquipmentSystemWeaponManipulationRequest");
    if equipment.is_null() || req.is_null() {
        return;
    }
    super::menu_rt::hset(&req, "requestType", super::menu_rt::ev("EquipmentManipulationAction", action));
    super::menu_main::set_handle_prop(&req, "owner", pl);
    rt::call(&equipment, "QueueRequest", &[A::H(&req)]);
}
/// PlayerPuppet's m_vrHolsterIntentTime: the sim time of the last holstering (bits of an f64).
static HOLSTER_INTENT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// PlayerPuppet.VRHolsterMelee: the holster's intent marked, the weapon put away.
pub(in crate::mechanics) fn holster_melee(pl: &Handle) {
    HOLSTER_INTENT.store(super::melee::sim_time().to_bits(), std::sync::atomic::Ordering::Relaxed);
    equip_request(pl, "UnequipWeapon");
}
/// PlayerPuppet.VRHolsterIntentActive(window): the weapon was holstered less than `window` seconds ago.
pub(in crate::mechanics) fn holster_intent_active(window: f64) -> bool {
    let since = super::melee::sim_time() - f64::from_bits(HOLSTER_INTENT.load(std::sync::atomic::Ordering::Relaxed));
    (0.0..window).contains(&since)
}
/// The player detached: its intent went with it.
pub(in crate::mechanics) fn forget_holster_intent() {
    HOLSTER_INTENT.store(0, std::sync::atomic::Ordering::Relaxed);
}
/// One of the former script helpers by its name, as its request.
pub(in crate::mechanics) fn equip_helper(pl: &Handle, name: &str) {
    let action = match name {
        "VRHolsterMelee" => {
            holster_melee(pl);
            return;
        }
        "VREquipMeleeWeapon" => "RequestFirstMeleeWeapon",
        "VREquipRangedWeapon" => "RequestLastUsedOrFirstAvailableRangedWeapon",
        "VREquipOneHandedRangedWeapon" => "RequestLastUsedOrFirstAvailableOneHandedRangedWeapon",
        "VREquipPrimaryWeapon" => "RequestWeaponSlot1",
        "VREquipWeaponSlot2" => "RequestWeaponSlot2",
        "VREquipWeaponSlot3" => "RequestWeaponSlot3",
        "VRUnequipWeapon" => "UnequipWeapon",
        _ => {
            let _ = m::<()>(pl, name, &[]);
            return;
        }
    };
    equip_request(pl, action);
}
