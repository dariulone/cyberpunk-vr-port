//! mods/cet/CyberpunkVRPort_Reach (what a hand reaches for, marked in the world, and the grip that takes it):
//! modules/world.lua -- the hand chooses, the game offers: of the things the targeting system knows round the player
//! (GetTargetParts: loot, containers, devices) the one nearest the palm, else the one its palm points at within a
//! short pull; the game's look-at ray is turned onto it (VRReachAim) and the game's own choice, prompt and highlight
//! do the rest; a grip closing on a thing in reach presses the interaction button for a moment (VRReachUse), one on a
//! thing pointed at holds it, and the palm snatched back pulls loot to the hand on a low arc (moved a frame at a time)
//! or uses a container or device where it stands; a weapon taken by the weapon hand flies into its hold and is drawn
//! (DrawItemRequest) -- and modules/marker.lua, the ring and brackets (vrport\ui\vr_marker.ent, spawned through
//! exEntitySpawner once the game is in play, turned to the eyes every frame, faded through its parts' appearances).
//! The other mods mark through the Lua host's shim (VRReachMark). The math is in f64, as Lua's numbers were.
use crate::script_call::{call_handle, method, native_class_function, Value};
use core::ffi::c_void;
use cvr_host::log;
use red4ext_abi::cname::CName;
use red4ext_abi::handle::Handle;
use red4ext_abi::rtti::{Class, IType, RttiKind};
use std::sync::Mutex;

type R<T> = Result<T, String>;
type V3 = [f64; 3];

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn mul(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn len(a: V3) -> f64 {
    dot(a, a).sqrt()
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn norm(v: V3) -> Option<V3> {
    let l = dot(v, v).sqrt();
    (l > 1e-6).then(|| mul(v, 1.0 / l))
}
/// A quaternion (i, j, k, r) as CET's Quaternion carried it.
type Q = [f64; 4];
fn q64(q: [f32; 4]) -> Q {
    [q[0] as f64, q[1] as f64, q[2] as f64, q[3] as f64]
}
fn qrot(q: Q, v: V3) -> V3 {
    let (x, y, z) = (v[0], v[1], v[2]);
    let (tx, ty, tz) = (2.0 * (q[1] * z - q[2] * y), 2.0 * (q[2] * x - q[0] * z), 2.0 * (q[0] * y - q[1] * x));
    [x + q[3] * tx + (q[1] * tz - q[2] * ty), y + q[3] * ty + (q[2] * tx - q[0] * tz), z + q[3] * tz + (q[0] * ty - q[1] * tx)]
}
fn slerp(a: Q, b: Q, t: f64) -> Q {
    let mut d = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    let sign = if d < 0.0 { -1.0 } else { 1.0 };
    d = d.abs();
    let (mut wa, mut wb) = (1.0 - t, t);
    if d < 0.9995 {
        let th = d.acos();
        let sn = th.sin();
        (wa, wb) = (((1.0 - t) * th).sin() / sn, (t * th).sin() / sn);
    }
    wb *= sign;
    let q = [a[0] * wa + b[0] * wb, a[1] * wa + b[1] * wb, a[2] * wa + b[2] * wb, a[3] * wa + b[3] * wb];
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    [q[0] / n, q[1] / n, q[2] / n, q[3] / n]
}

fn h(o: &Handle, name: &str, values: &[Value]) -> R<Handle> {
    // SAFETY: the caller's live object, held.
    unsafe { call_handle(o.instance, name, values) }.ok_or_else(|| format!("{name} failed"))
}
fn m<T: crate::script_call::Ret>(o: &Handle, name: &str, values: &[Value]) -> R<T> {
    // SAFETY: as above.
    unsafe { method::<T>(o.instance, name, values) }.ok_or_else(|| format!("{name} failed"))
}
fn is_a(o: &Handle, class: &str) -> bool {
    !o.is_null() && m::<bool>(o, "IsA", &[Value::U64(CName::hashed(class).0)]).unwrap_or(false)
}
fn v3_of(v: [f32; 4]) -> V3 {
    [v[0] as f64, v[1] as f64, v[2] as f64]
}
fn player() -> Handle {
    let mut p = Handle::null();
    // SAFETY: GetPlayer(GameInstance) into a handle of ours.
    unsafe { crate::anim_lookup::get_player(&mut p) };
    p
}
fn entity_id(o: &Handle) -> R<u64> {
    m::<u64>(o, "GetEntityID", &[])
}
fn game() -> crate::script_call::GameInstance {
    crate::script_call::game_instance()
}
fn find_entity(id: u64) -> R<Handle> {
    let f = native_class_function("ScriptGameInstance", "FindEntityByID").ok_or("no FindEntityByID")?;
    let g = game();
    // SAFETY: the game instance's static.
    unsafe {
        crate::script_call::invoke_handle(core::ptr::null_mut(), f, &[Value::Ptr((&g as *const crate::script_call::GameInstance).cast()), Value::U64(id)])
    }
    .ok_or_else(|| "FindEntityByID failed".into())
}
/// A WorldTransform (position in the engine's fixed point, x * 131072 truncated; the orientation as given).
#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct WorldTransform([u8; 0x20]);
fn world_transform(p: V3, q: Q) -> WorldTransform {
    let mut t = WorldTransform([0; 0x20]);
    for k in 0..3 {
        t.0[k * 4..k * 4 + 4].copy_from_slice(&(((p[k] as f32) * 131072.0) as i32).to_le_bytes());
    }
    for k in 0..4 {
        t.0[0x10 + k * 4..0x14 + k * 4].copy_from_slice(&(q[k] as f32).to_le_bytes());
    }
    t
}
fn place(e: &Handle, p: V3, q: Q) {
    let t = world_transform(p, q);
    crate::engine_objects::entity_set_world_transform(e, &t.0);
}
/// A scripted object's or struct's field by name.
///
/// # Safety
/// `value` is a live value of `class`.
unsafe fn field_in(value: *mut c_void, class: &Class, name: &str) -> R<(*mut c_void, &'static IType)> {
    // SAFETY: the caller's.
    unsafe {
        let p = class.property_named(name).ok_or_else(|| format!("no {name}"))?;
        Ok((p.value_ptr::<c_void>(value), &*p.ty))
    }
}
fn write_enum(at: *mut c_void, ty: &IType, value: i64) {
    // SAFETY: an enum value of the type's own size.
    unsafe {
        match ty.size() {
            1 => *(at as *mut i8) = value as i8,
            2 => *(at as *mut i16) = value as i16,
            8 => *(at as *mut i64) = value,
            _ => *(at as *mut i32) = value as i32,
        }
    }
}

// ---- marker.lua ---------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(in crate::mechanics) enum Kind {
    Near,
    Aim,
    Lock,
    Pull,
}
/// One frame's mark of a hand: the thing's centre, its size (how far before the centre the ring hangs when `lift`
/// does not say), the kind, whether it is in reach (near), the hand (where the ring opens to).
#[derive(Clone, Copy, Debug)]
pub(in crate::mechanics) struct Mark {
    pub pos: V3,
    pub radius: Option<f64>,
    pub kind: Kind,
    pub ready: bool,
    pub from: Option<V3>,
    pub lift: Option<f64>,
}

const ENT: &str = "vrport\\ui\\vr_marker.ent";
const PARTS: [&str; 5] = ["ring", "brackets1", "brackets2", "brackets3", "brackets4"];
const SNAP: [f64; 4] = [1.35, 1.24, 1.12, 1.0];
const LEVELS: f64 = 8.0;
const RESPAWN: f64 = 5.0;
const LITTER: f64 = 30.0;
const ASK: f64 = 0.5;
const IN: f64 = 14.0;
const OUT: f64 = 7.0;
const MORPH: f64 = 11.0;

#[derive(Default)]
struct MHand {
    id: Option<u64>,
    entity: Handle,
    comps: Option<[Handle; 5]>,
    level: [Option<i32>; 5],
    shown: f64,
    morph: f64,
    at: Option<Mark>,
    eye: Option<V3>,
    want: Option<Mark>,
    spawned_at: Option<f64>,
    last_turn: f64,
}
struct Marker {
    time: f64,
    hands: [MHand; 2],
    litter: Vec<(u64, f64)>,
    play: bool,
    next_ask: f64,
    pinned: [Option<Mark>; 2],
    reported: bool,
}

fn approach(x: f64, target: f64, rate: f64, dt: f64) -> f64 {
    x + (target - x) * (rate * dt).min(1.0)
}
/// The rotation whose columns are the world images of the local X, Y and Z axes.
fn quat_from_basis(x: V3, y: V3, z: V3) -> Q {
    let (m00, m01, m02) = (x[0], y[0], z[0]);
    let (m10, m11, m12) = (x[1], y[1], z[1]);
    let (m20, m21, m22) = (x[2], y[2], z[2]);
    let tr = m00 + m11 + m22;
    let (qx, qy, qz, qw);
    if tr > 0.0 {
        let s = (tr + 1.0).sqrt() * 2.0;
        (qw, qx, qy, qz) = (0.25 * s, (m21 - m12) / s, (m02 - m20) / s, (m10 - m01) / s);
    } else if m00 > m11 && m00 > m22 {
        let s = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
        (qw, qx, qy, qz) = ((m21 - m12) / s, 0.25 * s, (m01 + m10) / s, (m02 + m20) / s);
    } else if m11 > m22 {
        let s = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
        (qw, qx, qy, qz) = ((m02 - m20) / s, (m01 + m10) / s, 0.25 * s, (m12 + m21) / s);
    } else {
        let s = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
        (qw, qx, qy, qz) = ((m10 - m01) / s, (m02 + m20) / s, (m12 + m21) / s, 0.25 * s);
    }
    [qx, qy, qz, qw]
}
/// The marker at C seen from the eyes along N: X the opening's way (R turned by `turn` toward U), -Y the eyes', Z up.
fn transform_for(c: V3, r: V3, u: V3, n: V3, turn: f64) -> (V3, Q) {
    let (co, si) = (turn.cos(), turn.sin());
    let x = add(mul(r, co), mul(u, si));
    let z = sub(mul(u, co), mul(r, si));
    (c, quat_from_basis(x, mul(n, -1.0), z))
}
/// Where the eyes are: the rendered view, else the game's camera; None with no world to look at.
fn eyes() -> Option<V3> {
    // SAFETY: the view pose's read.
    let wp = unsafe { cvr_rig::vrik::hand_body::view_world_pos() };
    if wp[3] > 0.5 {
        return Some(v3_of(wp));
    }
    super::view::camera().ok().flatten().map(|(o, _)| o)
}
/// In play: a game (not the main menu's), the player there, and not paused.
fn in_play() -> bool {
    let Ok(r) = super::requests_handler() else { return false };
    !r.is_null() && m::<bool>(&r, "IsPreGame", &[]) == Ok(false) && m::<bool>(&r, "IsGamePaused", &[]) == Ok(false) && !player().is_null()
}
fn reset(h: &mut MHand) {
    h.id = None;
    h.entity = Handle::null();
    h.comps = None;
    h.level = [None; 5];
    h.at = None;
    h.eye = None;
}
fn spawn(s: &mut Marker, hand: usize, p: V3) {
    let h = &mut s.hands[hand];
    reset(h);
    h.id = super::ent_spawn::spawn(ENT, p, [0.0, 0.0, 0.0, 1.0], 0);
    h.spawned_at = Some(s.time);
}
/// Despawned now if it is there.
fn despawn_id(id: u64) -> bool {
    let Ok(e) = find_entity(id) else { return false };
    if e.is_null() {
        return false;
    }
    super::ent_spawn::despawn(&e)
}
fn despawn(s: &mut Marker, hand: usize) {
    let time = s.time;
    if let Some(id) = s.hands[hand].id {
        if !despawn_id(id) {
            s.litter.push((id, time));
        }
    }
    reset(&mut s.hands[hand]);
}
fn sweep(s: &mut Marker) {
    let time = s.time;
    s.litter.retain(|&(id, at)| !(despawn_id(id) || time - at > LITTER));
}
/// The entity and its parts once the spawn has landed; gone again with the world it was in.
fn resolve(s: &mut Marker, hand: usize) -> bool {
    let Some(id) = s.hands[hand].id else { return false };
    if s.hands[hand].comps.is_some() {
        if find_entity(id).is_ok_and(|e| !e.is_null()) {
            return true;
        }
        reset(&mut s.hands[hand]);
        s.play = false;
        return false;
    }
    let Ok(e) = find_entity(id) else { return false };
    if e.is_null() {
        return false;
    }
    let mut comps: [Handle; 5] = Default::default();
    for (i, part) in PARTS.iter().enumerate() {
        match h(&e, "FindComponentByName", &[Value::U64(CName::hashed(part).0)]) {
            Ok(c) if !c.is_null() => comps[i] = c,
            _ => return false,
        }
    }
    let hd = &mut s.hands[hand];
    hd.entity = e;
    hd.comps = Some(comps);
    hd.level = [None; 5];
    true
}
fn show(hd: &mut MHand, part: usize, level: i32) {
    if hd.level[part] == Some(level) {
        return;
    }
    if let Some(comps) = &hd.comps {
        crate::engine_objects::component_change_appearance(&comps[part], CName::hashed(&format!("l{level}")).0, true);
        hd.level[part] = Some(level);
    }
}
fn level(v: f64) -> i32 {
    ((LEVELS * v + 0.5).floor()).clamp(0.0, LEVELS) as i32
}
fn marker_update(s: &mut Marker, dt: f64) {
    s.time += dt;
    if !s.litter.is_empty() {
        sweep(s);
    }
    let mut eye: Option<V3> = None;
    for hand in 0..2 {
        if let Some(p) = s.pinned[hand] {
            s.hands[hand].want = Some(p);
        }
        let t = s.hands[hand].want.take();
        if s.hands[hand].id.is_none() {
            if t.is_none() && s.time >= s.next_ask {
                s.next_ask = s.time + ASK;
                s.play = in_play();
            }
            if t.is_some() || s.play {
                eye = eye.or_else(eyes);
                if let Some(e) = eye {
                    spawn(s, hand, e);
                }
            }
        } else if s.hands[hand].comps.is_none() && s.time - s.hands[hand].spawned_at.unwrap_or(s.time) > RESPAWN {
            despawn(s, hand);
        }
        if resolve(s, hand) {
            if !s.reported {
                s.reported = true;
                log!("[mechanics] reach: marker spawned and resolved from Rust (hand {})\n", hand + 1);
            }
            let lock = if t.is_some_and(|t| matches!(t.kind, Kind::Lock | Kind::Pull)) { 1.0 } else { 0.0 };
            let goal = match t {
                None => 0.0,
                Some(t) if t.kind == Kind::Near && !t.ready => 0.45,
                Some(_) => 1.0,
            };
            let hd = &mut s.hands[hand];
            hd.shown = approach(hd.shown, goal, if goal > hd.shown { IN } else { OUT }, dt);
            hd.morph = approach(hd.morph, lock, MORPH, dt);
            if t.is_some() {
                eye = eye.or_else(eyes);
                hd.at = t;
                hd.eye = eye;
            }
            if let (Some(a), Some(e)) = (hd.at, hd.eye) {
                // Turned to the eyes, before the thing by its lift, the opening toward the hand.
                if let Some(n) = norm(sub(e, a.pos)) {
                    let u = norm(sub([0.0, 0.0, 1.0], mul(n, n[2]))).or_else(|| norm(cross(n, [1.0, 0.0, 0.0])));
                    if let Some(u) = u {
                        let r = cross(u, n);
                        let mut turn = hd.last_turn;
                        if let Some(from) = a.from {
                            let d = sub(from, a.pos);
                            let (x, y) = (dot(d, r), dot(d, u));
                            if x * x + y * y > 1e-6 {
                                turn = y.atan2(x);
                            }
                        }
                        hd.last_turn = turn;
                        // The brackets stand square to the eyes: the turn gives way as they come.
                        let c = add(a.pos, mul(n, a.lift.unwrap_or(a.radius.unwrap_or(0.05))));
                        let (p, q) = transform_for(c, r, u, n, turn * (1.0 - hd.morph));
                        place(&hd.entity, p, q);
                    }
                }
            }
            // The ring fades as the brackets come; the brackets snap in through their steps.
            let mo = hd.morph;
            let (mut snap, mut best) = (0, f64::INFINITY);
            for (i, sc) in SNAP.iter().enumerate() {
                let d = (sc - (1.35 - 0.35 * mo)).abs();
                if d < best {
                    (snap, best) = (i, d);
                }
            }
            show(hd, 0, level(hd.shown * (1.0 - mo)));
            let bl = level(hd.shown * mo);
            for i in 0..4 {
                show(hd, 1 + i, if i == snap { bl } else { 0 });
            }
        }
    }
}
fn marker_clear(s: &mut Marker) {
    for hand in 0..2 {
        despawn(s, hand);
        s.hands[hand] = MHand::default();
    }
}

// ---- world.lua ---------------------------------------------------------------------------------------------------

const NEAR: f64 = 0.32;
const PULL_RANGE: f64 = 1.6;
const POINT_CONE: f64 = 7.0 * core::f64::consts::PI / 180.0;
const RAY_RADIUS: f64 = 0.06;
const PALM_AT: [V3; 2] = [[0.0396, 0.0039, -0.0124], [-0.0396, -0.0039, 0.0124]];
const PALM_OUT: [V3; 2] = [[0.0775, -0.0077, -0.9970], [-0.0775, 0.0077, 0.9970]];
const GRIP_ON: f64 = 0.65;
const GRIP_OFF: f64 = 0.35;
const LIST_EVERY: f64 = 0.25;
const CONFIRM_WITHIN: f64 = 0.35;
const SNUB: f64 = 1.2;
const BESIDE: f64 = 0.6;
const USE_FOR: f32 = 0.12;
const FLICK: f64 = 1.1;
const VEL_SMOOTH: f64 = 0.06;
const FLIGHT_SPEED: f64 = 4.0;
const FLIGHT_MIN: f64 = 0.18;
const FLIGHT_MAX: f64 = 0.45;
const ARC: f64 = 0.12;
const TAKE_AT: f64 = 0.8;
const GIVE_UP: f64 = 0.5;
const ALIGN_BY: f64 = 0.85;
const DRAW_WITHIN: f64 = 1.0;
// FOOD AND DRINK (main's 0.2.3): what lies about to eat is loot without a body, so no hand's ray meets it. The grip
// holds it on the hand's slot, and where the hand takes it decides: at the mouth it is eaten or drunk through the
// game's own take; let go anywhere else it goes back where it lay. It is pulled from further than other loot, and
// chosen first.
const EDIBLE_RANGE: f64 = 6.0;
const EDIBLE_CONE: f64 = 16.0 * core::f64::consts::PI / 180.0;
const EDIBLE_FIRST: f64 = 0.4;
// LOOT LYING ABOUT comes the same way (far): the game's prompt reaches some 2 m and follows the eyes; its word waits
// for the thing to arrive. Only what lies dropped.
const LAND_ASK: f64 = 0.2;
// the game's own outline on this one entity (entRenderHighlightEvent): pointed at, held from afar; 0 off
const OUTLINE_AIM: i32 = 5;
const OUTLINE_LOCK: i32 = 6;
const MOUTH_BELOW: f64 = 0.09;
const MOUTH_NEAR: f64 = 0.17;
const EAT_WAIT: f64 = 1.5;
const ASK_WAIT: f64 = 0.6;
const HAND_SLOTS: [&str; 2] = ["WeaponLeft", "WeaponRight"];

#[derive(Clone, Copy)]
struct Edible {
    item: [u8; 16],
    drink: bool,
}
impl Edible {
    fn how(&self) -> &'static str {
        if self.drink {
            "drink"
        } else {
            "eat"
        }
    }
}
#[derive(Clone)]
struct Thing {
    id: u64,
    entity: Handle,
    item: bool,
    pos: Option<V3>,
    edible: Option<Edible>,
    far: bool,
}
/// What a hand holds to eat (S.eating).
struct Eating {
    hand: usize,
    thing: Thing,
    start: V3,
    q: Q,
    since: f64,
    attached: bool,
    lift: f64,
    said: bool,
    asked_at: Option<f64>,
}
/// What the game is to put in the inventory, then eat or drink (S.swallow).
struct Swallow {
    item: [u8; 16],
    drink: bool,
    till: f64,
}
#[derive(Default)]
struct WHand {
    pos: Option<V3>,
    ray: Option<(V3, V3)>,
    palm: Option<(V3, V3)>,
    rel: Option<V3>,
    vel: Option<V3>,
    grip: bool,
    grip_prev: bool,
    busy: bool,
}
struct Target {
    hand: usize,
    thing: Thing,
    since: f64,
    confirmed: bool,
    locked: bool,
    snatch: f64,
    /// how it is reached this frame: in reach, pointed at, or held from afar (main's T.how)
    how: Kind,
    /// what the grip would take (main's T.offered)
    offered: Option<Thing>,
    said: bool,
}
struct Flight {
    hand: usize,
    thing: Thing,
    start: V3,
    lift: f64,
    q: Q,
    since: f64,
    duration: f64,
    asked: bool,
    landed: Option<f64>,
    weapon: bool,
}
struct Draw {
    id: [u8; 16],
    till: f64,
}
struct World {
    time: f64,
    things: Vec<Thing>,
    list_at: f64,
    hands: [WHand; 2],
    target: Option<Target>,
    snubbed: std::collections::HashMap<u64, f64>,
    uses: u32,
    pulls: u32,
    draws: u32,
    last_aim: bool,
    flight: Option<Flight>,
    draw: Option<Draw>,
    held: Option<(V3, f64)>,
    slot_comp: Option<Handle>,
    slot_owner: Option<u64>,
    reported: bool,
    edible: std::collections::HashMap<u64, Option<Edible>>,
    eating: Option<Eating>,
    swallow: Option<Swallow>,
    lit: Option<(Thing, i32)>,
    said_outline: bool,
}

struct State {
    marker: Marker,
    world: World,
}
// SAFETY: the handles are made, used and dropped on the game's main thread, where the mechanics tick.
unsafe impl Send for State {}
static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with<T>(f: impl FnOnce(&mut State) -> T) -> T {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let s = guard.get_or_insert_with(|| State {
        marker: Marker { time: 0.0, hands: Default::default(), litter: Vec::new(), play: false, next_ask: 0.0, pinned: [None, None], reported: false },
        world: World {
            time: 0.0,
            things: Vec::new(),
            list_at: -1.0,
            hands: Default::default(),
            target: None,
            snubbed: Default::default(),
            uses: 0,
            pulls: 0,
            draws: 0,
            last_aim: false,
            flight: None,
            draw: None,
            held: None,
            slot_comp: None,
            slot_owner: None,
            reported: false,
            edible: Default::default(),
            eating: None,
            swallow: None,
            lit: None,
            said_outline: false,
        },
    });
    f(s)
}

/// The ItemID a call returns (16 bytes).
fn item_id(o: &Handle) -> R<[u8; 16]> {
    let v = m::<[u64; 2]>(o, "GetItemID", &[])?;
    let mut out = [0u8; 16];
    out[..8].copy_from_slice(&v[0].to_le_bytes());
    out[8..].copy_from_slice(&v[1].to_le_bytes());
    Ok(out)
}
/// A hand with something in it takes nothing: an item in its weapon slot (bare fists and Gorilla Arms are a free hand),
/// or what the port put there (VRHandHolds).
fn holds(pl: &Handle, hand: usize) -> bool {
    if cvr_rig::vrik::reach::hand_holds(hand as i32 - 1) {
        return true;
    }
    let ts_held = super::game_system("GetTransactionSystem");
    let ts = ts_held.instance;
    if ts.is_null() {
        return false;
    }
    let slot = red4ext_abi::tweakdbid::from_name(if hand == 1 { "AttachmentSlots.WeaponLeft" } else { "AttachmentSlots.WeaponRight" });
    // SAFETY: the transaction system; the item held by its handle.
    let Some(item) = (unsafe { call_handle(ts, "GetItemInSlot", &[Value::Handle(pl), Value::U64(slot)]) }).filter(|i| !i.is_null()) else {
        return false;
    };
    let free = (|| -> R<bool> {
        let id = item_id(&item)?;
        let f = native_class_function("WeaponObject", "IsOfType")
            .or_else(|| crate::script_call::static_function("WeaponObject", "IsOfType"))
            .ok_or("no WeaponObject.IsOfType")?;
        let is = |t: &str| -> R<bool> {
            let v = crate::script_call::enum_value("gamedataItemType", t).ok_or("no gamedataItemType")?;
            // SAFETY: the static with the ItemID in place.
            unsafe { crate::script_call::invoke::<bool>(core::ptr::null_mut(), f, &[Value::Ptr(id.as_ptr().cast()), Value::U64(v as u64)]) }
                .ok_or_else(|| "IsOfType failed".into())
        };
        Ok(is("Wea_Fists")? || is("Cyb_StrongArms")?)
    })()
    .unwrap_or(false);
    !free
}
/// The hand the game draws a weapon into: the left one in the left-handed mode.
fn weapon_hand() -> usize {
    // SAFETY: the pose controls' read.
    if unsafe { cvr_rig::vrik::cyberware_natives::left_handed() } {
        1
    } else {
        2
    }
}
/// Where a weapon sits in hand `hand`: that hand's hold slot of the player's ItemAttachmentSlots.
fn hold_slot(w: &mut World, pl: &Handle, hand: usize) -> Option<(V3, Q)> {
    let owner = entity_id(pl).ok();
    if owner != w.slot_owner {
        w.slot_comp = None;
        w.slot_owner = owner;
    }
    if w.slot_comp.is_none() {
        for c in crate::engine_objects::entity_components(pl) {
            if m::<u64>(&c, "GetName", &[]).is_ok_and(|n| CName(n).text().contains("ItemAttachmentSlots")) {
                w.slot_comp = Some(c);
                break;
            }
        }
    }
    let comp = w.slot_comp.clone()?;
    let mut t = WorldTransform([0; 0x20]);
    let name = CName::hashed(if hand == 1 { "WeaponLeft" } else { "WeaponRight" });
    // SAFETY: the slot component, held; the transform's storage for the out parameter.
    let found = unsafe { method::<bool>(comp.instance, "GetSlotTransform", &[Value::U64(name.0), Value::Out(t.0.as_mut_ptr().cast())]) };
    match found {
        None => {
            w.slot_comp = None;
            None
        }
        Some(false) => None,
        Some(true) => {
            let fixed = |k: usize| i32::from_le_bytes(t.0[k * 4..k * 4 + 4].try_into().expect("4 bytes")) as f32 / 131072.0;
            let q = |k: usize| f32::from_le_bytes(t.0[0x10 + k * 4..0x14 + k * 4].try_into().expect("4 bytes")) as f64;
            Some(([fixed(0) as f64, fixed(1) as f64, fixed(2) as f64], [q(0), q(1), q(2), q(3)]))
        }
    }
}
fn blocked_now(pl: &Handle) -> R<bool> {
    if cvr_render::menu_lobby::active() {
        return Ok(true);
    }
    let r = super::requests_handler()?;
    if r.is_null() || m::<bool>(&r, "IsGamePaused", &[])? {
        return Ok(!r.is_null());
    }
    // SAFETY: a global script function with the player held.
    let vehicle = unsafe { crate::script_call::call_global_handle(core::ptr::null_mut(), "GetMountedVehicle", &[Value::Handle(pl)]) };
    if vehicle.is_some_and(|v| !v.is_null()) {
        return Ok(true);
    }
    let defs = super::boards::defs()?;
    let d = super::boards::definition(&defs, "PlayerStateMachine")?;
    let psm = h(pl, "GetPlayerStateMachineBlackboard", &[])?;
    if psm.is_null() {
        return Err("no state machine board".into());
    }
    use super::boards::{field, get};
    // The scanner up, a body carried, a grapple or a takedown: both hands are taken.
    Ok(get::<i32>(&psm, "GetInt", field(d, "Vision")?)? != 0
        || get::<bool>(&psm, "GetBool", field(d, "Carrying")?)?
        || get::<i32>(&psm, "GetInt", field(d, "Takedown")?)? != 0)
}
/// Whether the game offers a choice hub now (an NPC's or a device's choices on the UIInteractions board).
pub(super) fn choice_hub_up() -> bool {
    offers().1
}
/// What the game offers now: the owner of its loot prompt, and whether a choice hub is up.
fn offers() -> (Option<u64>, bool) {
    let (mut owner, mut hub) = (None, false);
    let _ = (|| -> R<()> {
        let defs = super::boards::defs()?;
        let d = super::boards::definition(&defs, "UIInteractions")?;
        let board = super::boards::system_board(d)?;
        if board.is_null() {
            return Err("no board".into());
        }
        if let Some((active, id)) = super::boards::variant_struct(&board, super::boards::field(d, "LootData")?, |data, class| {
            // SAFETY: the LootData value's fields.
            unsafe {
                let active = *(field_in(data, class, "isActive").ok()?.0 as *const bool);
                let id = *(field_in(data, class, "ownerId").ok()?.0 as *const u64);
                Some((active, id))
            }
        })? {
            if active {
                owner = Some(id);
            }
        }
        if let Some((active, choices)) = super::boards::variant_struct(&board, super::boards::field(d, "InteractionChoiceHub")?, |data, class| {
            // SAFETY: the InteractionChoiceHubData value's fields.
            unsafe {
                let active = *(field_in(data, class, "active").ok()?.0 as *const bool);
                let choices = *((field_in(data, class, "choices").ok()?.0 as *const u8).add(0x0C) as *const u32);
                Some((active, choices))
            }
        })? {
            hub = active && choices > 0;
        }
        Ok(())
    })();
    (owner, hub)
}
/// Loot, containers and devices: what a hand can take or use.
fn usable(e: &Handle) -> bool {
    is_a(e, "gameItemDropObject") || is_a(e, "gameItemObject") || is_a(e, "gameLootContainerBase") || is_a(e, "Device")
}
fn refresh(w: &mut World, pl: &Handle) {
    let mut list = Vec::new();
    let result = (|| -> R<(bool, u32)> {
        let qt: &'static IType = unsafe { &*(crate::script_call::class_any("TargetSearchQuery").ok_or("no TargetSearchQuery")? as *const Class as *const IType) };
        let mut query = crate::script_call::Owned::of_type(qt).ok_or("no query storage")?;
        // SAFETY: the query constructed by its own type; its fields by RTTI.
        unsafe {
            let class = &*(qt as *const IType as *const Class);
            let at = query.as_mut_ptr().cast::<c_void>();
            *(field_in(at, class, "maxDistance")?.0 as *mut f32) = (EDIBLE_RANGE + 1.0) as f32;
            let (set, set_ty) = field_in(at, class, "testedSet")?;
            write_enum(set, set_ty, crate::script_call::enum_value("TargetingSet", "Complete").ok_or("no TargetingSet.Complete")?);
            *(field_in(at, class, "ignoreInstigator")?.0 as *mut bool) = true;
        }
        let targeting_held = super::game_system("GetTargetingSystem");
        let targeting = targeting_held.instance;
        if targeting.is_null() {
            return Err("no targeting system".into());
        }
        // SAFETY: the targeting system; the parts array constructed by its type for the out parameter.
        let class = unsafe { red4ext_abi::rtti::object_class(targeting) }.ok_or("no class")?;
        let f = class.function(CName::hashed("GetTargetParts")).ok_or("no GetTargetParts")?;
        let parts_ty: &IType = unsafe { &*f.params.as_slice().get(2).and_then(|p| p.as_ref()).ok_or("no parts parameter")?.ty };
        let mut parts = crate::script_call::Owned::of_type(parts_ty).ok_or("no parts storage")?;
        let ok = unsafe {
            crate::script_call::invoke::<bool>(targeting, f, &[Value::Handle(pl), Value::Ptr(query.as_ptr().cast()), Value::Out(parts.as_mut_ptr().cast())])
        }
        .ok_or("GetTargetParts failed")?;
        if !ok {
            return Ok((false, 0));
        }
        let get = native_class_function("TS_TargetPartInfo", "GetComponent").ok_or("no TS_TargetPartInfo.GetComponent")?;
        let mut seen = std::collections::HashSet::new();
        let count;
        // SAFETY: the array's elements by its own type.
        unsafe {
            let storage = parts.as_mut_ptr().cast::<c_void>();
            count = parts_ty.array_length(storage);
            for i in 0..count {
                let part = parts_ty.array_element(storage, i);
                let c = crate::script_call::invoke_handle(core::ptr::null_mut(), get, &[Value::Ptr(part)]).ok_or("GetComponent failed")?;
                if c.is_null() {
                    continue;
                }
                let e = h(&c, "GetEntity", &[])?;
                if e.is_null() {
                    continue;
                }
                let id = entity_id(&e)?;
                if seen.insert(id) && usable(&e) {
                    let item = is_a(&e, "gameItemDropObject") || is_a(&e, "gameItemObject");
                    let mut thing = Thing { id, entity: e, item, pos: None, edible: None, far: false };
                    if item {
                        if !w.edible.contains_key(&id) {
                            if let Some(e) = edible_of(&thing.entity) {
                                w.edible.insert(id, e);
                            }
                        }
                        thing.edible = w.edible.get(&id).copied().flatten();
                        thing.far = thing.edible.is_some() || is_a(&thing.entity, "gameItemDropObject");
                    }
                    list.push(thing);
                }
            }
        }
        Ok((true, count))
    })();
    // Reported once per player when it answers (the first one in a world can come before the targeting system is
    // there), a failure each time it changes.
    static REFRESH_REPORTED: super::PerPlayer = super::PerPlayer::new();
    static LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);
    match &result {
        Ok((ok, parts)) => {
            if REFRESH_REPORTED.first() {
                log!("[mechanics] reach: GetTargetParts from Rust (ok={ok}, {parts} parts, {} usable things)\n", list.len());
            }
        }
        Err(e) => {
            let mut last = LAST_ERROR.lock().unwrap_or_else(|p| p.into_inner());
            if last.as_deref() != Some(e.as_str()) {
                log!("[mechanics] reach: the targeting query failed: {e}\n");
                *last = Some(e.clone());
            }
        }
    }
    w.things = list;
    let time = w.time;
    w.snubbed.retain(|_, till| *till >= time);
}
/// What an item lying about is to eat or drink (ItemActionsHelper's eat and drink actions): None before its item
/// object answers, Some(None) for neither.
fn edible_of(e: &Handle) -> Option<Option<Edible>> {
    let io = if is_a(e, "gameItemDropObject") { h(e, "GetItemObject", &[]).ok()? } else { e.clone() };
    if !is_a(&io, "gameItemObject") {
        return None;
    }
    let item = item_id(&io).ok()?;
    let action = |name: &str| {
        let f = crate::script_call::static_function("ItemActionsHelper", name)?;
        // SAFETY: a static with the ItemID in place.
        unsafe { crate::script_call::invoke_handle(core::ptr::null_mut(), f, &[Value::Ptr(item.as_ptr().cast())]) }
    };
    let drink = action("GetDrinkAction").is_some_and(|a| !a.is_null());
    let eat = action("GetEatAction").is_some_and(|a| !a.is_null());
    Some((drink || eat).then_some(Edible { item, drink }))
}
/// Where a thing is this frame: its origin, nudged up a little for loot (a drop's origin is its bottom).
fn where_of(t: &Thing) -> Option<V3> {
    let p = v3_of(m::<[f32; 4]>(&t.entity, "GetWorldPosition", &[]).ok()?);
    Some([p[0], p[1], p[2] + if t.item { 0.02 } else { 0.0 }])
}
fn hands(w: &mut World, pl: &Handle, dt: f64) -> R<()> {
    let wp = m::<[f32; 4]>(pl, "GetWorldPosition", &[])?;
    // SAFETY: the menu pointer's read.
    let hits = unsafe {
        cvr_rig::vrik::vr_menu::menu_pointer([wp[0], wp[1], wp[2], 1.0], [1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [1.0, 1.0, 0.0, 0.0])
    };
    let hits = (hits.len() >= 8).then_some(hits);
    let (pp, pq) = (v3_of(wp), q64(m::<[f32; 4]>(pl, "GetWorldOrientation", &[])?));
    for hand in 1..=2 {
        let busy = holds(pl, hand);
        let hd = &mut w.hands[hand - 1];
        hd.pos = None;
        hd.palm = None;
        hd.ray = None;
        // SAFETY: the palm's read.
        let mp = unsafe { cvr_rig::vrik::hand_body::palm_model_pos(hand == 2) };
        if mp[3] > 0.5 {
            hd.pos = Some(add(pp, qrot(pq, v3_of(mp))));
        }
        // The ray things are pointed at with: out of the middle of the palm.
        if let Some(pos) = hd.pos {
            // SAFETY: as above.
            let r = q64(unsafe { cvr_rig::vrik::hand_body::palm_model_rot(hand == 2) });
            hd.palm = Some((add(pos, qrot(pq, qrot(r, PALM_AT[hand - 1]))), qrot(pq, qrot(r, PALM_OUT[hand - 1]))));
        }
        let mut grip = 0.0;
        let was = hd.rel.take();
        if let Some(hits) = &hits {
            let (o, e) = (hits[4 * hand - 2], hits[4 * hand - 1]);
            grip = e[3] as f64;
            let d = sub(v3_of(e), v3_of(o));
            let l = len(d);
            if l > 1e-3 {
                hd.ray = Some((v3_of(o), mul(d, 1.0 / l)));
            }
            hd.rel = Some(sub(v3_of(o), pp));
        }
        // The controller's velocity against the body, smoothed: what a snatch is read from.
        match (hd.rel, was) {
            (Some(rel), Some(was)) if dt > 1e-4 => {
                let v = mul(sub(rel, was), 1.0 / dt);
                let k = (dt / VEL_SMOOTH).min(1.0);
                hd.vel = Some(match hd.vel {
                    Some(old) => add(old, mul(sub(v, old), k)),
                    None => v,
                });
            }
            _ => hd.vel = None,
        }
        hd.grip_prev = hd.grip;
        hd.grip = grip >= GRIP_ON || (hd.grip && grip > GRIP_OFF);
        hd.busy = busy;
    }
    Ok(())
}
/// The thing a hand reaches for: nearest the palm within NEAR, else the one its palm points at within PULL_RANGE.
fn choose(w: &World, hd: &WHand) -> Option<(usize, Kind, f64)> {
    let pos = hd.pos.filter(|_| !hd.busy)?;
    let fresh = |t: &Thing| w.snubbed.get(&t.id).copied().unwrap_or(-1.0) < w.time;
    let mut best: Option<(usize, f64)> = None;
    for (i, t) in w.things.iter().enumerate() {
        if let Some(tp) = t.pos.filter(|_| fresh(t)) {
            let d = len(sub(tp, pos));
            if d < NEAR && best.is_none_or(|(_, bd)| d < bd) {
                best = Some((i, d));
            }
        }
    }
    if let Some((i, d)) = best {
        return Some((i, Kind::Near, d));
    }
    let (o, dir) = hd.palm?;
    let mut best_a: Option<(usize, f64)> = None;
    let angle = |v: V3, d: V3| {
        let along = dot(v, d);
        (along > 0.1).then(|| (len(sub(v, mul(d, along))) - RAY_RADIUS).max(0.0).atan2(along))
    };
    for (i, t) in w.things.iter().enumerate() {
        if let Some(tp) = t.pos.filter(|tp| fresh(t) && len(sub(*tp, pos)) <= if t.far { EDIBLE_RANGE } else { PULL_RANGE }) {
            if let Some(mut a) = angle(sub(tp, o), dir) {
                if a < if t.far { EDIBLE_CONE } else { POINT_CONE } {
                    if t.far {
                        a *= EDIBLE_FIRST;
                    }
                    if best_a.is_none_or(|(_, ba)| a < ba) {
                        best_a = Some((i, a));
                    }
                }
            }
            // these are also what the fingers point at
            if let (true, Some((po, pd))) = (t.far, hd.ray) {
                if let Some(a) = angle(sub(tp, po), pd).filter(|a| *a < EDIBLE_CONE) {
                    let a = a * EDIBLE_FIRST;
                    if best_a.is_none_or(|(_, ba)| a < ba) {
                        best_a = Some((i, a));
                    }
                }
            }
        }
    }
    best_a.map(|(i, a)| (i, Kind::Aim, a))
}
fn aim(w: &mut World, p: Option<V3>) {
    match p {
        Some(p) => {
            cvr_rig::vrik::reach::set_aim_point([p[0] as f32, p[1] as f32, p[2] as f32, 1.0]);
            w.last_aim = true;
        }
        None if w.last_aim => {
            cvr_rig::vrik::reach::set_aim_point([0.0; 4]);
            w.last_aim = false;
        }
        None => {}
    }
}
fn use_(w: &mut World) {
    cvr_rig::vrik::reach::set_use_seconds(USE_FOR);
    w.uses += 1;
}
/// The weapon a thing on the ground is (its ItemID), or None.
fn weapon_in(t: &Thing) -> Option<[u8; 16]> {
    let io = if is_a(&t.entity, "gameItemDropObject") { h(&t.entity, "GetItemObject", &[]).ok()? } else { t.entity.clone() };
    if is_a(&io, "gameweaponObject") {
        item_id(&io).ok()
    } else {
        None
    }
}
/// What hand `hand` takes: the interaction button pressed on it, and a weapon taken by the weapon hand drawn into it.
fn take(w: &mut World, hand: usize, t: &Thing) {
    use_(w);
    if hand == weapon_hand() {
        if let Some(id) = weapon_in(t) {
            w.draw = Some(Draw { id, till: w.time + DRAW_WITHIN });
        }
    }
}
/// The game's take-and-equip: a DrawItemRequest, made at once -- the weapon is in the hand already.
fn draw_taken(w: &mut World, pl: &Handle) {
    let Some(d) = w.draw.as_ref() else { return };
    let ts_held = super::game_system("GetTransactionSystem");
    let ts = ts_held.instance;
    // SAFETY: the transaction system with the ItemID in place.
    let has = !ts.is_null() && unsafe { method::<bool>(ts, "HasItem", &[Value::Handle(pl), Value::Ptr(d.id.as_ptr().cast())]) }.unwrap_or(false);
    if has {
        let id = d.id;
        w.draw = None;
        let ok = (|| -> R<()> {
            let class = crate::script_call::class_any("DrawItemRequest").ok_or("no DrawItemRequest")?;
            // SAFETY: a new request owned by its handle; its fields by RTTI.
            let r = unsafe { Handle::from_instance(class.create_instance()) };
            unsafe {
                let (owner, owner_ty) = field_in(r.instance, class, "owner")?;
                if owner_ty.kind() == RttiKind::WeakHandle as u8 {
                    let weak = red4ext_abi::handle::WeakHandle::from_handle(pl);
                    owner_ty.assign(owner, (&weak as *const red4ext_abi::handle::WeakHandle).cast());
                } else {
                    owner_ty.assign(owner, (pl as *const Handle).cast());
                }
                let (item, _) = field_in(r.instance, class, "itemID")?;
                core::ptr::copy_nonoverlapping(id.as_ptr(), item as *mut u8, 16);
                let (anim, anim_ty) = field_in(r.instance, class, "equipAnimationType")?;
                write_enum(anim, anim_ty, crate::script_call::enum_value("gameEquipAnimationType", "Instant").ok_or("no Instant")?);
            }
            let f = native_class_function("ScriptGameInstance", "GetScriptableSystemsContainer").ok_or("no container")?;
            let g = game();
            // SAFETY: the game instance's static; the equipment system held for the call.
            unsafe {
                let container = crate::script_call::invoke_handle(core::ptr::null_mut(), f, &[Value::Ptr((&g as *const crate::script_call::GameInstance).cast())])
                    .filter(|c| !c.is_null())
                    .ok_or("no container")?;
                let system = call_handle(container.instance, "Get", &[Value::U64(CName::hashed("EquipmentSystem").0)]).filter(|s| !s.is_null()).ok_or("no EquipmentSystem")?;
                method::<()>(system.instance, "QueueRequest", &[Value::Handle(&r)]).ok_or("QueueRequest failed")?;
            }
            Ok(())
        })();
        if ok.is_ok() {
            w.draws += 1;
        }
    } else if w.time > d.till {
        w.draw = None;
    }
}
fn alive(e: &Handle) -> bool {
    !e.is_null() && m::<bool>(e, "IsAttached", &[]).unwrap_or(false)
}
/// Loot snatched by hand `hand`: it leaves from where it lies toward the palm -- a weapon for that hand toward its hold.
fn pull(w: &mut World, hand: usize, t: &Thing) -> bool {
    let Some(from) = where_of(t) else { return false };
    let (Ok(p), Ok(q)) = (m::<[f32; 4]>(&t.entity, "GetWorldPosition", &[]), m::<[f32; 4]>(&t.entity, "GetWorldOrientation", &[])) else { return false };
    let Some(hp) = w.hands[hand - 1].pos else { return false };
    let d = len(sub(hp, from));
    w.flight = Some(Flight {
        hand,
        thing: t.clone(),
        start: v3_of(p),
        lift: from[2] - p[2] as f64,
        q: q64(q),
        since: w.time,
        duration: (d / FLIGHT_SPEED).clamp(FLIGHT_MIN, FLIGHT_MAX),
        asked: false,
        landed: None,
        weapon: hand == weapon_hand() && weapon_in(t).is_some(),
    });
    w.pulls += 1;
    let what = t.edible.map(|e| e.how()).unwrap_or(if weapon_in(t).is_some() { "a weapon" } else { "loot" });
    log!("[VRReach] pulled {what} from {d:.2} m (from Rust)\n");
    true
}
/// A frame of the flight.
fn fly(w: &mut World, pl: &Handle, marks: &mut [Option<Mark>; 2]) {
    let Some(f) = w.flight.as_ref() else { return };
    let (hand, e) = (f.hand, f.thing.entity.clone());
    if !alive(&e) {
        w.flight = None;
        return;
    }
    let s = ((w.time - f.since) / f.duration).min(1.0);
    let k = s * s * (3.0 - 2.0 * s);
    let (mut to, mut q, mut take_at) = (None, f.q, TAKE_AT);
    if f.weapon {
        let (fq, _) = (f.q, ());
        if let Some((hp, hq)) = hold_slot(w, pl, hand) {
            let r = (s / ALIGN_BY).min(1.0);
            (to, q, take_at) = (Some(hp), slerp(fq, hq, r * r * (3.0 - 2.0 * r)), 1.0);
        }
    }
    let f = w.flight.as_ref().expect("checked");
    let hand_pos = w.hands[hand - 1].pos;
    let to = to.or_else(|| hand_pos.map(|p| sub(p, [0.0, 0.0, f.lift]))).unwrap_or(f.start);
    let p = add(add(f.start, mul(sub(to, f.start), k)), [0.0, 0.0, ARC * 4.0 * k * (1.0 - k)]);
    place(&e, p, q);
    let at = [p[0], p[1], p[2] + f.lift];
    aim(w, Some(at));
    let f = w.flight.as_mut().expect("checked");
    if s >= 1.0 && f.landed.is_none() {
        f.landed = Some(w.time);
    }
    if f.thing.edible.is_some() {
        if s >= 1.0 {
            let (thing, start, fq) = (f.thing.clone(), f.start, f.q);
            w.flight = None;
            if w.hands[hand - 1].grip {
                hold_food(w, hand, &thing, Some((start, fq)));
            } else {
                place(&e, start, fq);
            }
        }
        if w.flight.is_some() {
            marks[hand - 1] = Some(Mark { pos: at, radius: Some(0.05), kind: Kind::Lock, ready: true, from: hand_pos, lift: Some(0.06) });
        }
        return;
    }
    if s >= take_at && !f.asked && offers().0 == Some(f.thing.id) {
        f.asked = true;
        let t = f.thing.clone();
        take(w, hand, &t);
        log!("[VRReach] flying loot: the game offered it, taken (from Rust)\n");
    }
    // it has arrived, the ray is on it, and the prompt stayed on something beside it
    let f = w.flight.as_mut().expect("checked");
    if f.landed.is_some_and(|l| w.time - l > LAND_ASK) && !f.asked {
        if let Some(owner) = offers().0 {
            f.asked = true;
            let t = f.thing.clone();
            take(w, hand, &t);
            log!("[VRReach] flying loot: the prompt is on {owner}, not on it; pressed anyway (from Rust)\n");
        }
    }
    let f = w.flight.as_ref().expect("checked");
    if f.landed.is_some_and(|l| w.time - l > GIVE_UP) {
        log!("[VRReach] flying loot: the game did not take it, back where it lay; asked {} (from Rust)\n", f.asked);
        let (start, fq, id) = (f.start, f.q, f.thing.id);
        place(&e, start, fq);
        w.snubbed.insert(id, w.time + SNUB);
        w.flight = None;
        return;
    }
    marks[hand - 1] = Some(Mark { pos: at, radius: Some(0.05), kind: Kind::Lock, ready: true, from: hand_pos, lift: Some(0.06) });
}

// ---- food and drink in the hand
fn placed_now(t: &Thing) -> Option<(V3, Q)> {
    let p = m::<[f32; 4]>(&t.entity, "GetWorldPosition", &[]).ok()?;
    let q = m::<[f32; 4]>(&t.entity, "GetWorldOrientation", &[]).ok()?;
    Some((v3_of(p), q64(q)))
}
fn haptic(hand: usize) {
    // SAFETY: the plugin's own native.
    unsafe { crate::script_call::call_native::<()>("VRHapticEvent", &[Value::Int32(20), Value::Int32(hand as i32)]) };
}
/// holdFood: the grip holds it -- lifted from where it is now (after a flight, not where it lay).
fn hold_food(w: &mut World, hand: usize, t: &Thing, from: Option<(V3, Q)>) {
    let now = placed_now(t);
    let (start, q) = from.or(now).unwrap_or(([0.0; 3], [0.0, 0.0, 0.0, 1.0]));
    w.eating = Some(Eating { hand, thing: t.clone(), start, q, since: w.time, attached: false, lift: if t.item { 0.02 } else { 0.0 }, said: false, asked_at: None });
    log!("[VRReach] holding {} {} (from Rust)\n", t.edible.map(|e| e.how()).unwrap_or("?"), t.id);
    haptic(hand);
}
fn detach(e: &Handle) {
    // SAFETY: the plugin's own native.
    unsafe { crate::script_call::call_native::<bool>("VRDetachFromSlot", &[Value::Handle(e)]) };
}
fn let_food_go(w: &mut World, back: bool) {
    let Some(eat) = w.eating.take() else { return };
    if eat.attached {
        detach(&eat.thing.entity);
    }
    if back && alive(&eat.thing.entity) {
        place(&eat.thing.entity, eat.start, eat.q);
    }
    w.snubbed.insert(eat.thing.id, w.time + SNUB);
}
/// eatFrame: on the hand's slot once it is at the palm; at the mouth the game's take, a little later its eating.
fn eat_frame(w: &mut World, pl: &Handle) {
    let Some(eat) = w.eating.as_ref() else { return };
    let (hand, e) = (eat.hand, eat.thing.entity.clone());
    if !alive(&e) {
        w.eating = None;
        return;
    }
    let (grip, pos) = (w.hands[hand - 1].grip, w.hands[hand - 1].pos);
    let Some(pos) = pos.filter(|_| grip) else {
        let_food_go(w, true);
        return;
    };
    let time = w.time;
    let eat = w.eating.as_mut().expect("checked");
    if !eat.attached {
        place(&e, sub(pos, [0.0, 0.0, eat.lift]), eat.q);
        if time - eat.since > 0.03 {
            let slots = h(pl, "FindComponentByName", &[Value::U64(CName::hashed("ItemAttachmentSlots").0)]).unwrap_or_else(|_| Handle::null());
            // SAFETY: the plugin's own native.
            eat.attached = !slots.is_null() && unsafe { crate::script_call::call_native::<bool>("VRAttachToSlot", &[Value::Handle(&e), Value::Handle(&slots), Value::U64(CName::hashed(HAND_SLOTS[hand - 1]).0)]) } == Some(true);
            if !eat.said {
                eat.said = true;
                let from = placed_now(&eat.thing).map_or(-1.0, |(p, _)| len(sub(p, pos)));
                log!("[VRReach] food on the slot: {}, {from:.3} m from the palm (from Rust)\n", eat.attached);
            }
        }
    }
    let thing = eat.thing.clone();
    let at = where_of(&thing).unwrap_or(pos);
    aim(w, Some(at));
    let to_mouth = eyes().is_some_and(|y| len(sub(pos, [y[0], y[1], y[2] - MOUTH_BELOW])) <= MOUTH_NEAR);
    let eat = w.eating.as_mut().expect("checked");
    if !to_mouth {
        eat.asked_at = None;
        return;
    }
    let asked = *eat.asked_at.get_or_insert(time);
    let (owner, hub) = offers();
    if owner == Some(thing.id) || hub || time - asked > ASK_WAIT {
        if eat.attached {
            detach(&e);
        }
        let Some(edible) = thing.edible else { return };
        use_(w);
        w.swallow = Some(Swallow { item: edible.item, drink: edible.drink, till: time + EAT_WAIT });
        log!("[VRReach] {}: taken, the game offered {} (from Rust)\n", edible.how(), owner == Some(thing.id) || hub);
        haptic(hand);
        w.eating = None;
    }
}
/// swallowFrame: once the game has it in the inventory, eaten or drunk by its own action.
fn swallow_frame(w: &mut World, pl: &Handle) {
    let Some(sw) = w.swallow.as_ref() else { return };
    let ts = super::game_system("GetTransactionSystem");
    // SAFETY: the transaction system with the ItemID in place.
    let has = !ts.is_null() && unsafe { method::<bool>(ts.instance, "HasItem", &[Value::Handle(pl), Value::Ptr(sw.item.as_ptr().cast())]) }.unwrap_or(false);
    if has {
        let (item, drink) = (sw.item, sw.drink);
        w.swallow = None;
        let ok = crate::script_call::static_function("ItemActionsHelper", if drink { "DrinkItem" } else { "EatItem" })
            // SAFETY: the static with the player and the ItemID.
            .and_then(|f| unsafe { crate::script_call::invoke::<()>(core::ptr::null_mut(), f, &[Value::Handle(pl), Value::Ptr(item.as_ptr().cast()), Value::Bool(true)]) })
            .is_some();
        log!("[VRReach] {}: {} (from Rust)\n", if drink { "drink" } else { "eat" }, if ok { "done" } else { "failed" });
    } else if w.time > sw.till {
        log!("[VRReach] {}: the game did not take it (from Rust)\n", if sw.drink { "drink" } else { "eat" });
        w.swallow = None;
    }
}

// ---- the outline
fn outline(w: &mut World, t: &Thing, index: i32) {
    let on = |e: &Handle| -> bool {
        let ev = super::lobby_ink::new("entRenderHighlightEvent");
        if ev.is_null() {
            return false;
        }
        super::menu_rt::hset(&ev, "fillIndex", 0.0);
        super::menu_rt::hset(&ev, "outlineIndex", f64::from(index));
        super::menu_rt::hset(&ev, "seeThroughWalls", 0.0);
        super::menu_rt::hset(&ev, "opacity", 1.0);
        super::lobby_rtti::call(e, "QueueEvent", &[super::lobby_rtti::A::H(&ev)]).is_some()
    };
    let mut ok = on(&t.entity);
    // a drop is the thing it holds: that is what is drawn
    if is_a(&t.entity, "gameItemDropObject") {
        if let Ok(io) = h(&t.entity, "GetItemObject", &[]) {
            if !io.is_null() {
                ok &= on(&io);
            }
        }
    }
    if !ok || !w.said_outline {
        w.said_outline = true;
        log!("[VRReach] outline {index} on {}: {} (from Rust)\n", t.id, if ok { "asked" } else { "failed" });
    }
}
/// What is outlined follows what is chosen: the renderer asked only when that changes.
fn outline_frame(w: &mut World) {
    let want = if let Some(f) = w.flight.as_ref() {
        Some((f.thing.clone(), OUTLINE_LOCK))
    } else {
        w.target.as_ref().and_then(|t| t.offered.clone().map(|o| (o, if t.locked { OUTLINE_LOCK } else { OUTLINE_AIM })))
    };
    if let Some((lit, index)) = w.lit.clone() {
        if want.as_ref().is_none_or(|(t, i)| t.id != lit.id || *i != index) {
            if alive(&lit.entity) {
                outline(w, &lit, 0);
            }
            w.lit = None;
        }
    }
    if let (Some((t, index)), None) = (want, w.lit.as_ref()) {
        if alive(&t.entity) {
            outline(w, &t, index);
            w.lit = Some((t, index));
        }
    }
}

fn world_update(w: &mut World, dt: f64, marks: &mut [Option<Mark>; 2]) {
    w.time += dt;
    if let Some((point, till)) = w.held {
        if w.time < till {
            aim(w, Some(point));
            return;
        }
        w.held = None;
    }
    let pl = player();
    if !pl.is_null() {
        draw_taken(w, &pl);
        if w.swallow.is_some() {
            swallow_frame(w, &pl);
        }
    }
    if pl.is_null() || blocked_now(&pl).unwrap_or(false) {
        aim(w, None);
        w.target = None;
        return;
    }
    if w.time - w.list_at >= LIST_EVERY {
        refresh(w, &pl);
        w.list_at = w.time;
        if !w.reported && !w.things.is_empty() {
            w.reported = true;
            log!("[mechanics] reach: things round the player from Rust ({} of them)\n", w.things.len());
        }
    }
    for i in 0..w.things.len() {
        let p = where_of(&w.things[i]);
        w.things[i].pos = p;
    }
    if hands(w, &pl, dt).is_err() {
        return;
    }
    if w.eating.is_some() {
        eat_frame(w, &pl);
        return;
    }
    if w.flight.is_some() {
        fly(w, &pl, marks);
        return;
    }
    // A hold keeps its hand and its thing while the grip is closed; otherwise each free hand chooses.
    let mut chosen: Option<(usize, Thing, Kind)> = None;
    let held = w.target.as_ref().filter(|t| t.locked && w.hands[t.hand - 1].grip && !w.hands[t.hand - 1].busy);
    if let Some(t) = held {
        // The held thing as the list has it this frame (the mod held the list's own table, its position updated every
        // frame), else as it was last seen.
        let mut thing = w.things.iter().find(|x| x.id == t.thing.id).cloned().unwrap_or_else(|| t.thing.clone());
        if thing.pos.is_none() {
            thing.pos = where_of(&thing);
        }
        chosen = Some((t.hand, thing, Kind::Lock));
    } else {
        let mut score: Option<f64> = None;
        for hand in 1..=2 {
            let hd = &w.hands[hand - 1];
            if !(hd.grip && hd.grip_prev) {
                if let Some((i, kind, by)) = choose(w, hd) {
                    let sc = if kind == Kind::Near { 0.0 } else { 10.0 } + by;
                    if score.is_none_or(|s| sc < s) {
                        chosen = Some((hand, w.things[i].clone(), kind));
                        score = Some(sc);
                    }
                }
            }
        }
    }
    let Some((hand, thing, how)) = chosen else {
        w.target = None;
        aim(w, None);
        return;
    };
    // a distant hover must not lock the very grip edge that takes a prop at the palm (Reach may run before the hands)
    let hd = &w.hands[hand - 1];
    if how != Kind::Near && hd.grip && !hd.grip_prev && super::hand_collision::can_grab_near(hand - 1) {
        w.target = None;
        aim(w, None);
        return;
    }
    if w.target.as_ref().is_none_or(|t| t.thing.id != thing.id || t.hand != hand) {
        w.target = Some(Target { hand, thing: thing.clone(), since: w.time, confirmed: false, locked: false, snatch: 0.0, how, offered: None, said: false });
    } else if let Some(t) = w.target.as_mut() {
        t.thing = thing.clone();
        t.how = how;
    }
    if let (Some(e), Some(t)) = (thing.edible, w.target.as_mut()) {
        if how == Kind::Aim && !t.said {
            t.said = true;
            log!("[VRReach] pointed at {} {} (from Rust)\n", e.how(), thing.id);
        }
    }
    aim(w, thing.pos);
    // The game's word on it, a frame after the ray is on it.
    let (owner, hub) = offers();
    let targeting_held = super::game_system("GetTargetingSystem");
    let targeting = targeting_held.instance;
    let looked = if targeting.is_null() {
        None
    } else {
        // SAFETY: the targeting system with the player held.
        unsafe { call_handle(targeting, "GetLookAtObject", &[Value::Handle(&pl), Value::Bool(false), Value::Bool(false)]) }
            .filter(|o| !o.is_null())
            .and_then(|o| entity_id(&o).ok())
    };
    // What the grip would take: this thing, or the one beside it the game keeps its loot prompt on.
    let offered = if owner == Some(thing.id) || (hub && looked == Some(thing.id)) {
        Some(thing.clone())
    } else if let Some(owner) = owner {
        w.things.iter().find(|t| t.id == owner).filter(|o| o.pos.zip(thing.pos).is_some_and(|(a, b)| len(sub(a, b)) <= BESIDE)).cloned()
    } else {
        None
    };
    // food and far loot: the game's word waits for the take
    let offered = offered.or_else(|| (thing.edible.is_some() || (thing.far && how != Kind::Near)).then(|| thing.clone()));
    let t = w.target.as_mut().expect("set above");
    t.offered = offered.clone();
    t.confirmed = offered.is_some();
    let Some(offered) = offered else {
        if w.time - t.since > CONFIRM_WITHIN {
            w.snubbed.insert(thing.id, w.time + SNUB);
            w.target = None;
        }
        return;
    };
    let hd_grip = w.hands[hand - 1].grip;
    let hd_prev = w.hands[hand - 1].grip_prev;
    if how == Kind::Near {
        // In reach: the grip closing on it takes it.
        if hd_grip && !hd_prev {
            if offered.edible.is_some() {
                hold_food(w, hand, &offered, None);
                w.target = None;
                return;
            }
            take(w, hand, &offered);
        }
    } else {
        // Pointed at: the grip closing on it holds it from afar; held, the palm snatched away pulls loot to the hand and
        // uses a container or a device where it stands.
        let t = w.target.as_mut().expect("set above");
        if hd_grip && !hd_prev {
            t.locked = true;
        }
        if !hd_grip {
            t.locked = false;
        }
        let (pos, vel) = (w.hands[hand - 1].pos, w.hands[hand - 1].vel);
        if let (true, Some(vel), Some(pos), Some(op)) = (t.locked, vel, pos, offered.pos) {
            let away = sub(pos, op);
            let l = len(away);
            let speed = if l > 1e-3 { dot(vel, mul(away, 1.0 / l)) } else { 0.0 };
            t.snatch = t.snatch.max(speed);
            if speed >= FLICK {
                t.locked = false;
                w.target = None;
                if let Some(e) = offered.edible {
                    log!("[VRReach] pulled {} {} (from Rust)\n", e.how(), offered.id);
                }
                if !(offered.item && pull(w, hand, &offered)) {
                    take(w, hand, &offered);
                }
                return;
            }
        }
    }
    let locked = w.target.as_ref().is_some_and(|t| t.locked);
    let kind = if how == Kind::Near { Kind::Near } else if locked { Kind::Lock } else { Kind::Aim };
    if let Some(pos) = offered.pos {
        marks[hand - 1] = Some(Mark {
            pos,
            radius: Some(if offered.item { 0.05 } else { 0.07 }),
            kind,
            ready: true,
            from: w.hands[hand - 1].pos,
            lift: Some(if offered.item { 0.06 } else { 0.04 }),
        });
    }
}

/// The mod's onUpdate: the world's choice (marking its hands), then the markers.
pub(in crate::mechanics) fn tick(dt: f32) {
    let dt = f64::from(dt);
    with(|s| {
        let mut marks = [None, None];
        world_update(&mut s.world, dt, &mut marks);
        outline_frame(&mut s.world);
        for (hand, mark) in marks.into_iter().enumerate() {
            if let Some(mark) = mark {
                s.marker.hands[hand].want = Some(mark);
            }
        }
        marker_update(&mut s.marker, dt);
    });
}
/// Another mod's mark (the Lua host's shim of M.mark): a hand's mark for this frame.
pub(in crate::mechanics) fn mark(hand: usize, mark: Mark) {
    if (1..=2).contains(&hand) {
        with(|s| s.marker.hands[hand - 1].want = Some(mark));
    }
}
/// M.pin (tests): a mark held every frame until pinned again with none.
pub(in crate::mechanics) fn pin(hand: usize, mark: Option<Mark>) {
    if (1..=2).contains(&hand) {
        with(|s| s.marker.pinned[hand - 1] = mark);
    }
}
/// M.hold (tests): the look-at ray kept on a point for that many seconds, no hand choosing.
pub(in crate::mechanics) fn hold(point: V3, seconds: f64) {
    with(|s| {
        s.world.held = Some((point, s.world.time + seconds));
        s.world.target = None;
    });
}
/// The hand collision's K.reachBusy (main's GetMod('CyberpunkVRPort_Reach').world()): this mechanic has something for
/// that hand (1 left, 2 right) -- a thing flying to it, or one it holds from afar or that the game offers it. A confirmed
/// distant hover has not taken the hand: a physical thing at the palm gets the first grip edge (`near_pickup`), ranged
/// physics pulls still yield.
pub(in crate::mechanics) fn hand_busy(hand: usize, near_pickup: bool) -> bool {
    with(|s| {
        let w = &s.world;
        if w.flight.as_ref().is_some_and(|f| f.hand == hand) {
            return true;
        }
        let Some(t) = w.target.as_ref().filter(|t| t.hand == hand) else { return false };
        if t.locked {
            return true;
        }
        t.confirmed && (!near_pickup || t.how != Kind::Aim)
    })
}
/// The mod's onShutdown: both markers out of the world.
pub(in crate::mechanics) fn shutdown() {
    with(|s| marker_clear(&mut s.marker));
}
