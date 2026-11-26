//! mods/cet/CyberpunkVRPort_Weapon's tracked combat (the mod's combatFrame after the weapon frame, with cyberware.lua,
//! ordinary_melee.lua, physical_throw.lua and melee_tuning.lua), every frame:
//! - the iconic cyberware chord (VRCyberwareChordTake -> PlayerPuppet.VRActivateIconicCyberware);
//! - the arm implant in hand (PlayerPuppet.VRCyberwareKind per weapon entity: 1 Gorilla Arms, 2 Mantis Blades, 3
//!   Monowire, 4 Projectile Launcher; a plain melee weapon is the ordinary kind 5) to the plugin (VRCyberwareSelect),
//!   and which hands hold something other than fists or an implant (VRHandEquipment);
//! - the physical throw (VRMeleeThrowFrame): a grip press arms it, the release throws along the stroke's measured
//!   velocity (the best 20..65 ms segment ending within 100 ms of the release), its pitch mostly the head's and its
//!   heading leaning to the gaze the head had when the hand was last still (PlayerPuppet.VRMeleeThrowRelease);
//! - the implant's two swing histories (VRCyberwareFrame: the drawn geometry per hand, the controllers' poses): a
//!   swing begins at the hand's tracked speed past the threshold once re-armed by stillness or a reversal
//!   (VRCyberwareBeginSwing / UpdateSwing), the whoosh on the controllers' motion over 90 ms, the contacts swept
//!   between the frames' geometry (VRCyberwareContacts; the Monowire also collides its wire, VRCyberwareCollideWire);
//! - an ordinary melee weapon's swing from the grip's physical movement (VRMeleeInputFrame), the blade swept between
//!   frames (VRMeleeSweepHit).
use crate::script_call::{call_native, method, static_function, Value};
use cvr_host::log;
use red4ext_abi::cname::CName;
use red4ext_abi::handle::Handle;
use std::sync::Mutex;

// melee_tuning.lua
const SWING_SPEED: f64 = 3.0;
const WHOOSH_SPEED: f64 = 3.5;
const GORILLA_SWING_SPEED: f64 = 2.0;
const GORILLA_WHOOSH_SPEED: f64 = 2.0;
const REARM_SPEED: f64 = 1.0;
const FAST_SPEED: f64 = 4.2;
const MIN_GAP: f64 = 0.25;
const WINDOW: f64 = 0.09;
const MIN_SPAN: f64 = 0.04;
const TELEPORT: f64 = 0.30;
const EQUIP_MUTE: f64 = 0.8;

type V3 = [f64; 3];
type V4 = [f32; 4];
/// The points as f64 vectors (the swings' geometry in mechanics/melee.rs).
fn d3(p: &[V4]) -> Vec<V3> {
    p.iter().map(|x| v3(*x)).collect()
}
fn v3(v: V4) -> V3 {
    [f64::from(v[0]), f64::from(v[1]), f64::from(v[2])]
}
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn len(v: V3) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn scaled(a: V3, k: f64) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn v4w(v: V3, w: f32) -> V4 {
    [v[0] as f32, v[1] as f32, v[2] as f32, w]
}
/// The angle between two orientations (quaternions as x, y, z, w).
fn angle(a: V4, b: V4) -> f64 {
    let d = f64::from(a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]);
    2.0 * d.abs().min(1.0).acos()
}
/// The XR clock's milliseconds, modulo 2^24, as seconds between two stamps.
fn age(a: f32, b: f32) -> f64 {
    (f64::from(a) - f64::from(b)).rem_euclid(16_777_216.0) * 0.001
}

type R<T> = Result<T, String>;
fn m<T: crate::script_call::Ret>(o: &Handle, name: &str, values: &[Value]) -> R<T> {
    if o.is_null() {
        return Err(format!("{name}: no object"));
    }
    // SAFETY: the caller's live object, held.
    unsafe { method::<T>(o.instance, name, values) }.ok_or_else(|| format!("{name} failed"))
}
fn native<T: crate::script_call::Ret>(name: &str, values: &[Value]) -> Option<T> {
    // SAFETY: one of the plugin's own natives, called as the script VM calls it.
    unsafe { call_native::<T>(name, values) }
}
/// A native returning array<Vector4> (VRCyberwareFrame, VRMeleeInputFrame, VRMeleeThrowFrame).
fn native_frame(name: &str) -> Option<Vec<V4>> {
    let f = red4ext_abi::rtti::RttiSystem::get()?.get_function(CName::hashed(name))?;
    // SAFETY: a registered global function, no context; the array result kept while read.
    let owned = unsafe { crate::script_call::invoke_owned(core::ptr::null_mut(), &f.base, &[]) }?;
    // SAFETY: a DynArray of Vector4.
    let arr = unsafe { &*(owned.as_ptr() as *const red4ext_abi::dynarray::DynArray<V4>) };
    Some(arr.as_slice().to_vec())
}
fn shared(i: i32) -> f32 {
    // SAFETY: the shared block's read.
    unsafe { cvr_rig::vrik::provider::shared_slot(i) }
}
fn paused() -> bool {
    super::requests_handler().ok().filter(|r| !r.is_null()).is_some_and(|r| m::<bool>(&r, "IsGamePaused", &[]).unwrap_or(false))
}
fn entity_hash(o: &Handle) -> Option<u64> {
    m::<u64>(o, "GetEntityID", &[]).ok()
}
fn is_of_type(id: &[u64; 2], ty: &str) -> bool {
    let Some(t) = crate::script_call::enum_value("gamedataItemType", ty) else { return false };
    static_function("WeaponObject", "IsOfType")
        // SAFETY: a static function (no context).
        .and_then(|f| unsafe { crate::script_call::invoke::<bool>(core::ptr::null_mut(), f, &[Value::Ptr(id.as_ptr().cast()), Value::U64(t as u64)]) })
        .unwrap_or(false)
}
fn is_melee(w: &Handle) -> bool {
    let Ok(id) = m::<[u64; 2]>(w, "GetItemID", &[]) else { return false };
    static_function("WeaponObject", "IsMelee")
        // SAFETY: as above.
        .and_then(|f| unsafe { crate::script_call::invoke::<bool>(core::ptr::null_mut(), f, &[Value::Ptr(id.as_ptr().cast())]) })
        .unwrap_or(false)
}


#[derive(Clone)]
struct Sample {
    time: f64,
    points: Vec<V3>,
    control: V4,
    rotation: V4,
}
struct Hand {
    history: Vec<Sample>,
    sound: Vec<Sample>,
    armed: bool,
    mute: f64,
    last_whoosh: f64,
    episode: bool,
    epoch: Option<f32>,
    time: Option<f64>,
    rel: Option<Vec<V3>>,
    points: Option<Vec<V4>>,
    origin: Option<V3>,
    control: Option<V4>,
    rotation: Option<V4>,
    track_origin: Option<f32>,
    quiet_since: Option<f64>,
    direction: Option<V3>,
    strong: bool,
    whoosh: bool,
    last_begin: f64,
    wire_live_until: f64,
    began_on_hit: bool,
    peak: f64,
}
fn fresh(now: f64) -> Hand {
    Hand {
        history: Vec::new(),
        sound: Vec::new(),
        armed: false,
        mute: now + EQUIP_MUTE,
        last_whoosh: -10.0,
        episode: false,
        epoch: None,
        time: None,
        rel: None,
        points: None,
        origin: None,
        control: None,
        rotation: None,
        track_origin: None,
        quiet_since: None,
        direction: None,
        strong: false,
        whoosh: false,
        last_begin: -10.0,
        wire_live_until: -1.0,
        began_on_hit: false,
        peak: 0.0,
    }
}

/// ordinary_melee.lua's sample: the XR grip's sample and turn, the solved weapon's grip and tip, the player's origin.
#[derive(Clone)]
struct OSample {
    id: u64,
    control: V4,
    rotation: V4,
    position: V4,
    tip: V4,
    origin: V3,
    relative_grip: V3,
    relative_tip: V3,
    stamp: f32,
    epoch: f32,
    line: Option<V3>,
    time: f64,
}
struct Ordinary {
    sample: OSample,
    geometry: OSample,
    history: Vec<OSample>,
    mute: f64,
    armed: bool,
    sound_armed: bool,
    last_begin: f64,
    last_sound: f64,
    quiet: Option<f64>,
    direction: Option<V3>,
    strong: bool,
    episode: bool,
    began_on_hit: bool,
    peak: f64,
    live_until: f64,
    repeat_age: f64,
}

/// physical_throw.lua's state.
struct ThrowAim {
    t: f32,
    gaze: Option<V4>,
    eye: Option<V4>,
}
struct Throw {
    key: u64,
    origin: f32,
    seq: f32,
    stamp: f32,
    p: V3,
    down: bool,
    held: bool,
    started: f32,
    history: Vec<(V3, f32)>,
    aims: Vec<ThrowAim>,
}

struct State {
    cached_id: Option<Option<u64>>,
    cached_kind: i32,
    cached_ordinary: bool,
    hands: [Option<Hand>; 2],
    previous_stamp: Option<f32>,
    real_clock: f64,
    ordinary: Option<Ordinary>,
    ordinary_clock: f64,
    sample_ids: u64,
    throw: Option<Throw>,
    errors: Vec<String>,
    reported: bool,
    classified: bool,
}
// SAFETY: the mechanics tick on the game's main thread only.
unsafe impl Send for State {}
static STATE: Mutex<Option<State>> = Mutex::new(None);
/// CVR_MELEE_TRACE=1: the physical stroke's samples and sweeps logged (a melee hit followed from the controller's
/// motion to the blade's contact).
fn trace() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("CVR_MELEE_TRACE").is_ok_and(|v| v == "1"))
}

/// cyberware.lua's M.kind: the implant in hand, asked once per weapon entity.
fn kind(s: &mut State, weapon: &Handle, player: &Handle) -> i32 {
    let id = if weapon.is_null() { None } else { entity_hash(weapon) };
    if s.cached_id != Some(id) {
        if s.cached_kind == 3 {
            super::cyberware::wire_effect(player, &Handle::null(), false);
        }
        (s.cached_id, s.cached_kind, s.hands, s.previous_stamp, s.cached_ordinary) = (Some(id), 0, [None, None], None, false);
        s.ordinary = None;
        s.classified = false;
    }
    // an entity can enter its slot before its equipment record is ready (a restore): the same entity retried
    if !s.classified && !weapon.is_null() && !player.is_null() && super::blade::record_ready(weapon) {
        s.cached_kind = super::melee::kind(weapon);
        s.cached_ordinary = s.cached_kind == 0 && (is_melee(weapon) || super::blade::kind(weapon) > 0);
        s.classified = true;
    }
    s.cached_kind
}
fn input_kind(s: &State) -> i32 {
    if s.cached_ordinary {
        5
    } else {
        s.cached_kind
    }
}
/// cyberware.lua's M.handEquipment: which hands hold something other than fists or an implant.
fn hand_equipment(player: &Handle) {
    if player.is_null() {
        native::<()>("VRHandEquipment", &[Value::Int32(3)]);
        return;
    }
    let ts = super::game_system("GetTransactionSystem");
    let mut mask = 0;
    for (bit, slot) in [(1, "AttachmentSlots.WeaponLeft"), (2, "AttachmentSlots.WeaponRight")] {
        if ts.is_null() {
            break;
        }
        // SAFETY: the system, held.
        let item = unsafe {
            crate::script_call::call_handle(ts.instance, "GetItemInSlot", &[Value::Handle(player), Value::U64(red4ext_abi::tweakdbid::from_name(slot))])
        }
        .unwrap_or_else(Handle::null);
        if item.is_null() {
            continue;
        }
        let Ok(id) = m::<[u64; 2]>(&item, "GetItemID", &[]) else { continue };
        let free = ["Wea_Fists", "Cyb_StrongArms", "Cyb_MantisBlades", "Cyb_NanoWires"].iter().any(|t| is_of_type(&id, t));
        if !free {
            mask += bit;
        }
    }
    native::<()>("VRHandEquipment", &[Value::Int32(mask)]);
}

/// cyberware.lua's M.update: the implant's two swing histories.
fn cyber_update(s: &mut State, dt: f64, player: &Handle, weapon: &Handle, kind: i32) -> R<()> {
    let threshold = if kind == 1 { GORILLA_SWING_SPEED } else { SWING_SPEED };
    let sound_threshold = if kind == 1 { GORILLA_WHOOSH_SPEED } else { WHOOSH_SPEED };
    if kind == 4 {
        return Ok(());
    }
    if dt <= 0.0 || dt > 0.1 || paused() {
        s.hands = [None, None];
        return Ok(());
    }
    let Some(data) = native_frame("VRCyberwareFrame") else {
        s.hands = [None, None];
        return Ok(());
    };
    if data.len() < 6 || data[0][0] as i32 != kind {
        s.hands = [None, None];
        return Ok(());
    }
    let (header, origin4) = (data[0], data[1]);
    let (left_n, right_n) = (header[1].max(0.0) as usize, header[2].max(0.0) as usize);
    let tracking = 6 + left_n + right_n; // 0-based: Lua's 7 + y + z
    if data.len() < tracking + 4 {
        s.hands = [None, None];
        return Ok(());
    }
    if let Some(prev) = s.previous_stamp {
        s.real_clock += age(origin4[3], prev);
    }
    s.previous_stamp = Some(origin4[3]);
    let now = s.real_clock;
    if kind == 3 {
        super::cyberware::wire_effect(player, weapon, left_n + right_n > 0);
    }
    let origin = v3(origin4);
    let mut offset = 6;
    for side in 0..2usize {
        let count = if side == 0 { left_n } else { right_n };
        let points: Vec<V4> = data[offset..offset + count].to_vec();
        offset += count;
        if s.hands[side].is_none() {
            s.hands[side] = Some(fresh(now));
        }
        if count < 2 || data[tracking + side][3] <= 0.0 {
            s.hands[side] = Some(fresh(now));
            continue;
        }
        let mut h = s.hands[side].take().unwrap_or_else(|| fresh(now));
        if h.epoch == Some(header[3]) || h.time.is_some_and(|t| now - t < 0.004) {
            s.hands[side] = Some(h);
            continue;
        }
        h.epoch = Some(header[3]);
        if kind == 3 {
            let prev = h.points.clone().unwrap_or_default();
            let span = h.time.map(|t| now - t).unwrap_or(dt);
            super::cyberware::collide_wire(player, side as i32, header[3] as i32, &d3(&points), &d3(&prev), span);
        }
        let rel: Vec<V3> = points.iter().map(|p| sub(v3(*p), origin)).collect();
        let (control, rotation) = (data[tracking + side], data[tracking + 2 + side]);
        let mut teleport = h.origin.is_some_and(|o| len(sub(origin, o)) > 0.6);
        if h.track_origin.is_some_and(|t| t != control[3]) {
            teleport = true;
        }
        if h.time.is_some_and(|t| now - t > 0.1) {
            teleport = true;
        }
        if let (Some(c), Some(t)) = (h.control, h.time) {
            if len(sub(v3(control), v3(c))) > TELEPORT.max((now - t) * 20.0) {
                teleport = true;
            }
        }
        let (mut instant, mut fastest) = (0.0f64, [0.0; 3]);
        if let (Some(prev), Some(t)) = (&h.rel, h.time) {
            if prev.len() == count {
                for i in 0..count {
                    let mv = sub(rel[i], prev[i]);
                    let d = len(mv);
                    if kind != 3 && d > 0.65f64.max((now - t) * 25.0) {
                        teleport = true;
                    }
                    let speed = d / (now - t).max(0.001);
                    if speed > instant {
                        (instant, fastest) = (speed, mv);
                    }
                }
            }
        }
        if teleport {
            h = fresh(now);
        }
        let wire_motion = instant;
        let lever = match kind {
            2 => 0.65,
            3 => 0.35,
            _ => 0.10,
        };
        if let (Some(c), Some(r), Some(t)) = (h.control, h.rotation, h.time) {
            let span = (now - t).max(0.001);
            let tracked = len(sub(v3(control), v3(c))).max(angle(rotation, r) * lever) / span;
            instant = instant.min(tracked);
        }
        h.history.push(Sample { time: now, points: rel.clone(), control, rotation });
        let window = if kind == 3 { 0.06 } else { 0.035 };
        while h.history.len() > 2 && now - h.history[0].time > window {
            h.history.remove(0);
        }
        let (mut speed, mut velocity) = (0.0f64, [0.0; 3]);
        if let Some(first) = h.history.first() {
            if now - first.time >= 0.008 && first.points.len() == count {
                for i in 0..count {
                    let mv = sub(rel[i], first.points[i]);
                    let value = len(mv) / (now - first.time);
                    if value > speed {
                        (speed, velocity) = (value, mv);
                    }
                }
                let tracked = len(sub(v3(control), v3(first.control))).max(angle(rotation, first.rotation) * lever) / (now - first.time);
                speed = speed.min(tracked);
            }
        }
        if instant > speed {
            (speed, velocity) = (instant, fastest);
        }
        // the solved item over the audio window, with tracked motion too: a passive wire's oscillation stays silent
        if let Some(last) = h.history.last().cloned() {
            h.sound.push(last);
        }
        while h.sound.first().is_some_and(|e| now - e.time > WINDOW) {
            h.sound.remove(0);
        }
        let mut sound_speed = 0.0;
        if let Some(oldest) = h.sound.first() {
            if oldest.points.len() == count && now - oldest.time >= MIN_SPAN {
                let tracked = len(sub(v3(control), v3(oldest.control))).max(angle(rotation, oldest.rotation) * lever);
                let solved = (0..count).map(|i| len(sub(rel[i], oldest.points[i]))).fold(0.0, f64::max);
                sound_speed = instant.min(tracked.min(solved) / (now - oldest.time));
            }
        }
        if kind == 1 {
            velocity = sub(v3(control), v3(h.control.unwrap_or(control)));
        }
        if speed < REARM_SPEED {
            let q = *h.quiet_since.get_or_insert(now);
            if now - q >= 0.08 {
                (h.armed, h.episode, h.whoosh) = (true, false, false);
            }
        } else if !h.armed && h.direction.is_some() && speed >= threshold && len(velocity) > 0.0001 {
            h.quiet_since = None;
            let dir = h.direction.unwrap_or_default();
            if dot(velocity, dir) < -0.1 * len(velocity) * len(dir) {
                h.armed = true;
            }
        } else {
            h.quiet_since = None;
        }
        if h.armed && speed >= threshold && now >= h.mute && now - h.last_begin >= 0.12 {
            h.strong = shared(if side == 0 { 154 } else { 160 }) > 0.5;
            (h.episode, h.began_on_hit, h.peak) = (true, false, speed);
            h.last_begin = now;
            if kind == 3 && h.episode {
                h.wire_live_until = now + 0.45;
            }
            (h.armed, h.direction, h.whoosh) = (false, Some(velocity), false);
        }
        if h.episode {
            h.peak = h.peak.max(speed);
        }
        if h.episode && sound_speed >= sound_threshold && !h.whoosh && now - h.last_whoosh >= MIN_GAP {
            super::melee::whoosh(weapon, sound_speed >= FAST_SPEED, h.strong);
            (h.whoosh, h.last_whoosh) = (true, now);
        }
        if let Some(prev) = h.points.clone().filter(|p| p.len() == count) {
            let active = (h.episode && instant >= threshold) || (kind == 3 && now < h.wire_live_until && wire_motion >= threshold);
            let radius = if kind == 3 { 0.035 } else { 0.08 };
            let peak = if h.peak > 0.0 { h.peak } else { speed };
            let hit = super::melee::contacts(player, weapon, side as i32, &d3(&points), &d3(&prev), radius, !active, !h.began_on_hit, h.strong, peak);
            if hit > 0 {
                h.began_on_hit = true;
            }
        }
        (h.rel, h.points, h.origin, h.time) = (Some(rel), Some(points), Some(origin), Some(now));
        (h.control, h.rotation, h.track_origin) = (Some(control), Some(rotation), Some(control[3]));
        s.hands[side] = Some(h);
    }
    Ok(())
}

fn weapon_trigger() -> bool {
    shared(if native::<bool>("VRWeaponInLeftHand", &[]) == Some(true) { 154 } else { 160 }) > 0.5
}
/// controllerTravel: the grip's travel, the line's turn at the lever, the turn's chord at the item's end.
fn controller_travel(a: &OSample, b: &OSample, reach: f64) -> f64 {
    let mut travel = len(sub(v3(a.control), v3(b.control)));
    if let (Some(la), Some(lb)) = (a.line, b.line) {
        travel = travel.max(dot(la, lb).clamp(-1.0, 1.0).acos() * (reach * 0.5).max(0.10));
    }
    let (u, r) = (a.rotation, b.rotation);
    let cosine = f64::from(u[0] * r[0] + u[1] * r[1] + u[2] * r[2] + u[3] * r[3]).abs();
    // a stationary grip can still turn the prop: the world check rejects pure roll
    travel.max(2.0 * reach * (1.0 - cosine.min(1.0).powi(2)).max(0.0).sqrt())
}
/// The grip's physical travel: the controller's own movement, and with both hands on the weapon the turn of the line
/// between them at a lever of half the reach. A stroke is the arm's: a turn of the wrist alone (or a roll) gives the
/// blade's end a sword-length lever and was taken for a swing -- the air's whoosh and a full blow on every wrist flick.
fn grip_travel(a: &OSample, b: &OSample, lever: f64) -> f64 {
    let mut travel = len(sub(v3(a.control), v3(b.control)));
    if let (Some(la), Some(lb)) = (a.line, b.line) {
        travel = travel.max(dot(la, lb).clamp(-1.0, 1.0).acos() * lever);
    }
    travel
}
/// solvedMotion: the solved item's larger end motion relative to the player.
fn solved_motion(a: &OSample, b: &OSample) -> V3 {
    let grip = sub(a.relative_grip, b.relative_grip);
    let tip = sub(a.relative_tip, b.relative_tip);
    if dot(tip, tip) > dot(grip, grip) {
        tip
    } else {
        grip
    }
}
/// contactMotion: a short stroke along the blade or across its edge, the controller moving too.
fn contact_motion(a: &OSample, b: &OSample, axis: V3, edge: Option<V3>, reach: f64) -> bool {
    if controller_travel(a, b, reach) < 0.022 {
        return false;
    }
    let d = solved_motion(a, b);
    let magnitude = len(d);
    if magnitude < 0.020 {
        return false;
    }
    let along = dot(d, axis);
    let across = edge.map_or(0.0, |e| dot(d, e) - along * dot(e, axis));
    along.max(across.abs()) >= 0.65 * magnitude
}
/// trackedWorldMotion: the XR grip's motion in the world by the tracking basis.
fn tracked_world_motion(a: &OSample, b: &OSample) -> V4 {
    let d = sub(v3(a.control), v3(b.control));
    let basis = crate::blade_natives::tracking_basis();
    if basis[3] < 0.5 {
        return [0.0; 4];
    }
    let (bx, by) = (f64::from(basis[0]), f64::from(basis[1]));
    [(bx * d[0] + by * d[2]) as f32, (by * d[0] - bx * d[2]) as f32, d[1] as f32, 0.0]
}
/// sweep: the latest solved interval swept (VRMeleeSweepHit) -- probed in a stroke, or a short contact stroke of a
/// blade; the stroke begun on its first contact.
#[allow(clippy::too_many_arguments)]
fn o_sweep(o: &mut Ordinary, player: &Handle, weapon: &Handle, sample: &OSample, forward: V3, reach: f64, active: bool, allow_short: bool, motion: V4, dt: f64) {
    let old = std::mem::replace(&mut o.geometry, sample.clone());
    let travel = len(solved_motion(sample, &old));
    let edge = m::<V4>(weapon, "GetWorldForward", &[]).ok().map(v3);
    let short = allow_short && super::blade::kind(weapon) > 0 && o.history.first().is_some_and(|first| contact_motion(sample, first, forward, edge, reach));
    let probe = travel > 0.001 && (active || short);
    let strong = probe && ((active && o.strong) || (!active && weapon_trigger()));
    let speed = if active { o.peak } else { travel / dt.max(1e-6) };
    let hit = super::melee::sweep_hit(player, weapon, sample.position, sample.tip, old.position, old.tip, 0.08, !probe, motion, !(active && o.began_on_hit), strong, speed);
    if trace() && (probe || hit != 0) {
        log!(
            "[melee-trace] sweep: travel {travel:.3} m, active {active}, short {short}, probe {probe}, strong {strong}, speed {speed:.2} -> {hit}\n"
        );
    }
    if hit > 0 && active {
        o.began_on_hit = true;
    }
}
/// ordinary_melee.lua's M.update: the solved item's swept points, the tracking confirming the player's motion;
/// the XR grip's sample for the blade's pose (before any virtual resistance).
fn ordinary_update(s: &mut State, dt: f64, player: &Handle, weapon: &Handle, position: V4, forward: V4, origin: V3) -> R<Option<V4>> {
    let input = native_frame("VRMeleeInputFrame");
    let Some(input) = input.filter(|i| i.len() == 5 && i[2][3] > 0.0 && dt > 0.0 && dt <= 0.25 && !paused()) else {
        s.ordinary = None;
        return Ok(None);
    };
    let reach = super::melee::reach(weapon).min(1.25);
    let (p3, f3) = (v3(position), v3(forward));
    let tip = v4w([p3[0] + f3[0] * reach, p3[1] + f3[1] * reach, p3[2] + f3[2] * reach], 1.0);
    s.sample_ids += 1;
    let mut sample = OSample {
        id: s.sample_ids,
        control: input[2],
        rotation: input[4],
        position,
        tip,
        origin,
        relative_grip: sub(p3, origin),
        relative_tip: sub(v3(tip), origin),
        stamp: input[0][0],
        epoch: input[0][1],
        line: None,
        time: 0.0,
    };
    if input[0][2] > 0.5 && input[1][3] > 0.0 {
        let line = sub(v3(input[1]), v3(input[2]));
        let n = len(line);
        if n > 0.03 {
            sample.line = Some(scaled(line, 1.0 / n));
        }
    }
    let tracking = Some(input[2]);
    let mut previous = s.ordinary.as_ref().map(|o| o.sample.clone());
    let repeated = previous.as_ref().is_some_and(|p| p.epoch == sample.epoch);
    let mut span = previous.as_ref().map(|p| age(sample.stamp, p.stamp)).unwrap_or(0.0);
    if repeated {
        span = 0.0;
        let o = s.ordinary.as_mut().ok_or("no ordinary state")?;
        o.repeat_age += dt;
        if len(sub(origin, o.geometry.origin)) > 0.6 {
            s.ordinary = None;
            return Ok(tracking);
        }
        if o.repeat_age > 0.06 {
            // the passive contacts still polled; a stale input interval never replayed
            o_sweep(o, player, weapon, &sample, f3, reach, false, false, [0.0; 4], dt);
            return Ok(tracking);
        }
    } else if previous.is_some() && span < 0.004 {
        return Ok(tracking);
    }
    s.ordinary_clock += span.min(0.25);
    let clock = s.ordinary_clock;
    sample.time = clock;
    if let Some(p) = &previous {
        if span > 0.1
            || p.control[3] != sample.control[3]
            || len(sub(v3(sample.control), v3(p.control))) > TELEPORT.max(span * 15.0)
            || len(sub(origin, p.origin)) > 0.6
        {
            s.ordinary = None;
            previous = None;
        }
    }
    let (Some(o), Some(mut previous)) = (s.ordinary.as_mut(), previous) else {
        s.ordinary = Some(Ordinary {
            sample: sample.clone(),
            geometry: sample.clone(),
            history: vec![sample],
            mute: clock + EQUIP_MUTE,
            armed: false,
            sound_armed: false,
            last_begin: -10.0,
            last_sound: -10.0,
            quiet: None,
            direction: None,
            strong: false,
            episode: false,
            began_on_hit: false,
            peak: 0.0,
            live_until: -1.0,
            repeat_age: 0.0,
        });
        return Ok(tracking);
    };
    if previous.line.is_some() != sample.line.is_some() {
        // a change of grip is not motion: the stroke and the mute kept
        o.history.clear();
        previous = sample.clone();
        o.geometry = sample.clone();
    }
    if repeated && !o.history.is_empty() {
        let last = o.history.len() - 1;
        o.history[last] = sample.clone();
    } else {
        o.history.push(sample.clone());
    }
    while o.history.len() > 1 && clock - o.history[0].time > 0.18 {
        o.history.remove(0);
    }
    // the stroke's speed is the grip's physical movement (grip_travel); the blade's solved motion only the direction
    let (mut speed, mut velocity) = (0.0f64, [0.0; 3]);
    let second_last = o.history.len().checked_sub(2).map(|i| o.history[i].id);
    let extra = if repeated { o.repeat_age } else { 0.0 };
    let lever = (reach * 0.5).max(0.10);
    for old in &o.history {
        let seconds = clock - old.time;
        if seconds >= 0.004 && (seconds <= 0.05 || Some(old.id) == second_last) {
            let travel = grip_travel(&sample, old, lever);
            let value = travel / (seconds + extra);
            // a one-sample twitch rejected by its excursion, a short real stroke not averaged over standing still
            if travel >= 0.05 && value > speed {
                speed = value;
                velocity = sub(v3(sample.control), v3(old.control));
                if sample.line.is_some() && len(velocity) < 0.005 {
                    velocity = solved_motion(&sample, old);
                }
            }
        }
    }
    if speed < REARM_SPEED {
        let q = *o.quiet.get_or_insert(clock);
        if clock - q >= 0.06 {
            (o.armed, o.sound_armed) = (true, true);
        }
    } else {
        o.quiet = None;
        if let Some(dir) = o.direction {
            if dot(velocity, dir) < -0.2 * len(velocity) * len(dir) && clock - o.last_begin >= 0.12 {
                (o.armed, o.sound_armed) = (true, true);
            }
        }
    }
    if clock >= o.mute && speed >= SWING_SPEED && o.armed && clock - o.last_begin >= 0.12 {
        o.strong = weapon_trigger();
        (o.episode, o.began_on_hit, o.peak) = (true, false, speed);
        (o.armed, o.last_begin, o.direction) = (false, clock, Some(velocity));
    }
    if o.episode && speed >= SWING_SPEED {
        o.live_until = clock + 0.06;
        o.peak = o.peak.max(speed);
    }
    // the air's sound is the physical stroke's: one whoosh a stroke or reversal
    if clock >= o.mute && speed >= WHOOSH_SPEED && o.sound_armed && clock - o.last_sound >= 0.12 {
        super::melee::whoosh(weapon, speed >= FAST_SPEED, weapon_trigger());
        (o.sound_armed, o.last_sound) = (false, clock);
    }
    let active = o.episode && clock <= o.live_until;
    if trace() && (speed >= REARM_SPEED || active) {
        log!("[melee-trace] stroke: speed {speed:.2} m/s, armed {}, episode {}, active {active}, peak {:.2}\n", o.armed, o.episode, o.peak);
    }
    if !active {
        o.episode = false;
    }
    if !repeated {
        o.repeat_age = 0.0;
    }
    let short = !repeated && clock >= o.mute && controller_travel(&sample, &previous, reach) > 0.0015;
    let motion = tracked_world_motion(&sample, &previous);
    o_sweep(o, player, weapon, &sample, f3, reach, active, short, motion, if repeated { dt } else { span });
    o.sample = sample;
    Ok(tracking)
}

// physical_throw.lua
const AIM_PITCH_HAND: f64 = 0.2;
const AIM_YAW_HAND: f64 = 0.35;
const AIM_STILL: f64 = 1.0;
const LAUNCH_BASE: f64 = 10.0;
const LAUNCH_PER_HAND: f64 = 5.0;
const LAUNCH_MIN: f64 = 10.0;
const LAUNCH_MAX: f64 = 45.0;
fn heading(v: V3) -> f64 {
    v[0].atan2(-v[2])
}
fn pitch(v: V3) -> f64 {
    (v[1] / len(v).max(1e-6)).clamp(-1.0, 1.0).asin()
}
fn wrap(a: f64) -> f64 {
    (a + std::f64::consts::PI).rem_euclid(2.0 * std::f64::consts::PI) - std::f64::consts::PI
}
fn flat(v: V3) -> f64 {
    (v[0] * v[0] + v[2] * v[2]).sqrt() / len(v).max(1e-6)
}
fn along(yaw: f64, p: f64) -> V3 {
    [yaw.sin() * p.cos(), p.sin(), -yaw.cos() * p.cos()]
}
/// Tracking space (Y up, -Z forward) to the world by the body's heading `b` (x, y).
fn world(v: V3, b: V4) -> V4 {
    let (bx, by) = (f64::from(b[0]), f64::from(b[1]));
    v4w([bx * v[0] + by * v[2], by * v[0] - bx * v[2], v[1]], 0.0)
}
fn aimed(d: V3, gaze: Option<V3>) -> (V3, V3) {
    let Some(gaze) = gaze.filter(|g| len(*g) >= 0.5) else { return (d, d) };
    let gaze_yaw = if flat(gaze) > 0.2 { heading(gaze) } else { heading(d) };
    let own = if flat(d) > 0.35 { heading(d) } else { gaze_yaw };
    let off = wrap(own - gaze_yaw).abs();
    let (window, fade) = (30f64.to_radians(), 60f64.to_radians());
    let share = if off <= window { AIM_YAW_HAND } else { AIM_YAW_HAND + (1.0 - AIM_YAW_HAND) * ((off - window) / (fade - window)).min(1.0) };
    let p = pitch(gaze) + AIM_PITCH_HAND * (pitch(d) - pitch(gaze));
    (along(gaze_yaw + wrap(own - gaze_yaw) * share, p), along(own, p))
}
fn throw_reset(s: &mut State, player: &Handle) {
    s.throw = None;
    if !player.is_null() {
        super::melee_throw::cancel();
    }
}
/// physical_throw.lua's M.update; true while the throw owns the weapon (no melee swing then).
fn throw_update(s: &mut State, dt: f64, player: &Handle, weapon: &Handle) -> R<bool> {
    let ready = !player.is_null() && !weapon.is_null() && super::melee_throw::ready(player, weapon);
    if !ready || dt <= 0.0 || dt > 0.1 || paused() {
        throw_reset(s, player);
        return Ok(!player.is_null() && super::melee_throw::recovering(weapon));
    }
    let Some(frame) = native_frame("VRMeleeThrowFrame").filter(|f| f.len() >= 4) else {
        throw_reset(s, player);
        return Ok(true);
    };
    let (hd, p, b) = (frame[0], v3(frame[1]), frame[2]);
    let (gaze_now, eye_now) = (frame.get(4).copied(), frame.get(5).copied());
    let key = entity_hash(weapon).unwrap_or(0);
    let new = s.throw.as_ref().is_none_or(|t| t.key != key || t.origin != hd[2]);
    if new {
        s.throw = Some(Throw { key, origin: hd[2], seq: hd[1], stamp: hd[0], p, down: b[3] > 0.35, held: false, started: hd[0], history: Vec::new(), aims: Vec::new() });
        return Ok(false);
    }
    let Some(t) = s.throw.as_mut() else { return Ok(true) };
    if hd[1] != t.seq {
        let span = age(hd[0], t.stamp);
        if span > 0.1 || len(sub(p, t.p)) > 0.45f64.max(span * 16.0) {
            throw_reset(s, player);
            return Ok(true);
        }
        let moved = if span > 0.0 { len(sub(p, t.p)) / span } else { 0.0 };
        (t.seq, t.stamp, t.p) = (hd[1], hd[0], p);
        if t.held {
            if moved < AIM_STILL {
                t.aims.push(ThrowAim { t: hd[0], gaze: gaze_now, eye: eye_now });
                if t.aims.len() > 128 {
                    t.aims.remove(0);
                }
            }
            t.history.push((p, hd[0]));
            while t.history.len() > 1 && age(hd[0], t.history[0].1) > 0.20 {
                t.history.remove(0);
            }
        }
    }
    let down = (t.down && b[3] > 0.35) || (!t.down && b[3] >= 0.65);
    let releasing = !down && t.down && t.held;
    if down && !t.down {
        (t.held, t.started) = (true, hd[0]);
        t.history = vec![(p, hd[0])];
        t.aims = vec![ThrowAim { t: hd[0], gaze: gaze_now, eye: eye_now }];
    } else if !down && t.down && t.held {
        t.held = false;
        let (mut best, mut velocity, mut began) = (0.0f64, None::<V3>, hd[0]);
        for (i, finish) in t.history.iter().enumerate() {
            let recency = age(hd[0], finish.1);
            if recency > 0.10 {
                continue;
            }
            for start in &t.history[i.saturating_sub(6)..i] {
                let seconds = age(finish.1, start.1);
                if (0.02..=0.065).contains(&seconds) {
                    let delta = sub(finish.0, start.0);
                    let travel = len(delta);
                    let speed = travel / seconds;
                    let score = speed * (1.0 - 0.25 * recency / 0.10);
                    if travel >= 0.035 && speed <= 16.0 && score > best {
                        (best, velocity, began) = (score, Some(scaled(delta, 1.0 / seconds)), start.1);
                    }
                }
            }
        }
        let speed = velocity.map(len).unwrap_or(0.0);
        let mut aim = t.aims.first();
        for a in &t.aims {
            if age(began, a.t) < 60.0 {
                aim = Some(a);
            }
        }
        let gaze = aim.and_then(|a| a.gaze).or(gaze_now).map(v3);
        let eye = aim.and_then(|a| a.eye).or(eye_now).map(v3);
        let base = if speed >= 0.7 { scaled(velocity.unwrap_or_default(), 1.0 / speed) } else { v3(frame[3]) };
        let (direction, own) = aimed(base, gaze);
        let look = match gaze.filter(|g| len(*g) > 0.5) {
            Some(g) => world(g, b),
            None => world(direction, b),
        };
        let eye_offset = eye.map(|e| world(sub(e, p), b)).unwrap_or([0.0; 4]);
        let launch = (LAUNCH_BASE + speed * LAUNCH_PER_HAND).clamp(LAUNCH_MIN, LAUNCH_MAX);
        if age(hd[0], t.started) >= 0.04 {
            let (dw, ow) = (world(direction, b), world(own, b));
            let accepted = super::melee_throw::release(player, weapon, v3(dw), f64::from(launch as f32), f64::from(speed as f32), v3(ow), v3(look), v3(eye_offset));
            log!("[CyberpunkVRPort_Weapon] throw: {} (hand {speed:.2} m/s, launch {launch:.1} m/s) from Rust\n", if accepted { "released" } else { "rejected" });
        }
        t.history.clear();
    }
    t.down = down;
    // aiming a throwable item is also a melee state: the input reserved only for an armed throw, its release frame or
    // a pending transition
    let held = t.held;
    Ok(held || releasing || super::melee_throw::pending(weapon))
}

fn combat_frame(s: &mut State, dt: f64) -> R<()> {
    let mut pl = Handle::null();
    // SAFETY: GetPlayer(GameInstance) into a handle of ours.
    unsafe { crate::anim_lookup::get_player(&mut pl) };
    let wpn = if pl.is_null() {
        Handle::null()
    } else {
        static_function("ScriptedPuppet", "GetActiveWeapon")
            // SAFETY: a static function (no context).
            .and_then(|f| unsafe { crate::script_call::invoke_handle(core::ptr::null_mut(), f, &[Value::Handle(&pl)]) })
            .unwrap_or_else(Handle::null)
    };
    let mut wpn = wpn;
    if !pl.is_null() {
        let held = super::blade::physical_weapon(&pl);
        if !held.is_null() && super::blade::kind(&held) > 0 {
            wpn = held;
        }
    }
    if native::<bool>("VRCyberwareChordTake", &[]) == Some(true) {
        super::cyberware::activate_iconic(&pl);
    }
    let k = kind(s, &wpn, &pl);
    native::<()>("VRCyberwareSelect", &[Value::Int32(input_kind(s))]);
    hand_equipment(&pl);
    let throwing = throw_update(s, dt, &pl, &wpn)?;
    let mut blade_frame = false;
    if k != 0 {
        cyber_update(s, dt, &pl, &wpn, k)?;
    } else if input_kind(s) == 5 && !throwing {
        let pos = m::<V4>(&wpn, "GetWorldPosition", &[])?;
        let fwd = super::melee::direction(&wpn);
        let origin = v3(m::<V4>(&pl, "GetWorldPosition", &[])?);
        let tracking = ordinary_update(s, dt, &pl, &wpn, pos, fwd, origin)?;
        blade_frame = true;
        super::blade::pose_update(dt, &pl, &wpn, pos, fwd, tracking);
    }
    if !blade_frame {
        super::blade::pose_reset();
    }
    if !s.reported && !pl.is_null() {
        s.reported = true;
        log!("[mechanics] weapon combat: from Rust (kind {k}, input kind {}, throwing {throwing})\n", input_kind(s));
    }
    Ok(())
}

/// The mod's combatFrame after the weapon frame, every frame.
pub(in crate::mechanics) fn tick(dt: f32) {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let s = guard.get_or_insert_with(|| State {
        cached_id: None,
        cached_kind: 0,
        cached_ordinary: false,
        hands: [None, None],
        previous_stamp: None,
        real_clock: 0.0,
        ordinary: None,
        ordinary_clock: 0.0,
        sample_ids: 0,
        throw: None,
        errors: Vec::new(),
        reported: false,
        classified: false,
    });
    crate::blade_mesh::poll();
    super::blade::tick();
    if let Err(e) = combat_frame(s, f64::from(dt)) {
        super::blade::pose_reset();
        if !s.errors.contains(&e) {
            log!("[CyberpunkVRPort_Weapon] cyberware combat: {e} (from Rust)\n");
            s.errors.push(e);
        }
    }
}
