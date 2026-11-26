//! mods/cet/CyberpunkVRPort_TacticalBelt/grenade/grenade.lua: the grenade in the hand. A fresh grip at a lit grenade
//! holder takes the equipped grenade's prop into that hand's own slot (VRPortGrenadeLeft/Right; never into a hand a weapon
//! is in, never from a seat); its parts are posed
//! from the hold (the wrist's frame, per type and lever state) every frame, the fingers from the recorded poses; the
//! free hand's pinch at the pin magnets the wrist there and a fast pull past the stop pulls it (a button grenade arms
//! by the trigger held); the trigger closes the lever (sounds, the game's trigger blocked while held). Let go with the
//! pin out: the release velocity (the window's peak, averaged with its neighbours) throws the real grenade through the
//! port's flight (belt_flight.rs) or the game's launch; a charge spent. Let go with the pin in: back on the belt.
//! Main's grenade work (c026279ef .. ed36cb209): with a pin grenade held, a click of that hand's stick (claimed from the
//! game: cvr-input's grenade_stick) starts the thumb's 1.27 s pin timeline (belt_pin_contact.rs): the thumb hooks the
//! ring and pulls it against its spring, the fingers follow the regrip, the ring flies off the hand and shrinks away;
//! the left hand's hold mirrored from the right's; the trigger blocked on the holding hand only; a change of the
//! rendering plane re-attaches the prop alone, the grip and its clock untouched.
use super::belt::{
    conj, dist, euler_quat, f3, f4, has, in_slot, m, native, player, player_plane, prop_allow_slot, prop_item_grenade, qmul, qrot, record_name, shared, st,
    tdb, tdb_str, transactions, Belt, V3, Q,
};
use super::belt_flight::Flights;
use super::belt_pin_contact::{self as contact, Contact, Phase, Profile};
use super::belt_poses::{Pose, POSES, TYPES};
use crate::script_call::Value;
use cvr_host::log;
use red4ext_abi::cname::CName;
use red4ext_abi::handle::Handle;
use serde_json::Value as J;

const GRIP_SLOT: [i32; 2] = [155, 49];
const HAND_SLOT: [&str; 2] = ["AttachmentSlots.VRPortGrenadeLeft", "AttachmentSlots.VRPortGrenadeRight"];
const TRIGGER_SLOT: [i32; 2] = [154, 160];
const WRIST_BONE: [i32; 2] = [23, 24];
const ANCHOR_BONE: [i32; 2] = [26, 28];
const THUMB_TIP: [i32; 2] = [55, 60];
const INDEX_TIP: [i32; 2] = [65, 69];
const HIST: usize = 24;
const THROW_SLOTS: [&str; 3] = ["AttachmentSlots.GrenadeLeft", "AttachmentSlots.GrenadeRight", "AttachmentSlots.VRPortGrenadeFlight"];
const PIN_P: V3 = [-0.0005, -0.0188, 0.0125];
const HOLD1_P: V3 = [-0.0767, 0.0493, 0.0254];
const HOLD1_Q: Q = [0.7317, -0.0221, -0.1596, -0.6623];
const HOLD_P: V3 = [-0.0830, 0.0516, 0.0267];
const HOLD_Q: Q = [-0.731265, -0.032951, 0.109304, 0.672472];

/// The pin ring's flight off the hand once the thumb has yanked it: when, and from where (its start, rotation and
/// velocity, model space).
#[derive(Default)]
struct RingFlight {
    at: Option<f64>,
    start: Option<(V3, Q, V3)>,
}
/// The stick's pin timeline (grenade.lua's `stick`).
#[derive(Default)]
struct Stick {
    sequence: Option<i32>,
    elapsed: Option<f64>,
    contact: Option<Contact>,
    hold: Option<(V3, Q, Option<Vec<(String, Q)>>)>,
    hold_lever: Option<f64>,
    profile: Option<&'static Profile>,
    flight: RingFlight,
    /// The holding thumb's three local rotations set this frame (for the tip's segment).
    thumb: Option<[Option<Q>; 3]>,
}
struct Thrown {
    slot: Option<String>,
    dir: V3,
    side: usize,
    speed: f64,
    at: f64,
    rec: String,
    armed_at: f64,
}
/// The grab's state (E.S.grab and the file's own tables).
#[derive(Default)]
pub(super) struct Grenade {
    pub side: Option<usize>,
    pub slot: Option<String>,
    pub host: Option<String>,
    plane: Option<i32>,
    kind: Option<String>,
    item: Option<String>,
    retry_plane_at: Option<f64>,
    stick: Stick,
    parts: Option<Vec<Handle>>,
    pin: Option<Vec<Handle>>,
    obj: Handle,
    pinned: bool,
    was: [bool; 2],
    spent: [bool; 2],
    last_dt: f64,
    // pinS
    pin_hand: Option<usize>,
    pin_blend: f64,
    pin_at: Option<V3>,
    pin_out: bool,
    magnet: Option<usize>,
    pin_pull: Option<V3>,
    item_p: Option<V3>,
    item_q: Option<Q>,
    pin_prev: Option<V3>,
    pin_speed: f64,
    grab_q: Option<Q>,
    // throwS
    t_prev: Option<V3>,
    tv: V3,
    hist: Vec<(V3, f64)>,
    hn: usize,
    armed: Option<String>,
    armed_at: f64,
    arm_slot: Option<String>,
    rec_by_slot: Vec<(String, String)>,
    pending_throw: Option<Thrown>,
    // leverS, poseS, trgS
    lever: f64,
    lever_shut: bool,
    lever_arm_at: Option<f64>,
    pose_hand: Option<usize>,
    pose_blend: f64,
    /// The trigger the hold blocks: the primary hand's (SetVRTriggerMode) or the other's (SetVRLeftTriggerBlock).
    trigger_writer: Option<&'static str>,
    mute_stow: bool,
    snd_at: Vec<(String, f64)>,
    flights: Flights,
}

/// The grab trace (CFG.grab_trace: grab_trace.log beside the mod).
pub(super) fn trace_line(line: &str) {
    if !TRACE.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    use std::io::Write;
    let path = cvr_host::paths::file(cvr_host::paths::Folder::Lua, "CyberpunkVRPort_TacticalBelt\\grab_trace.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{line}");
    }
}
static TRACE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn pose(name: &str) -> Option<Pose> {
    POSES.iter().find(|p| p.0 == name).map(|p| p.1)
}
fn custom_for(s: &Belt, kind: Option<&str>) -> bool {
    kind.is_some_and(|k| s.cfg.v("grenade_custom").get(k) == Some(&J::Bool(true)))
}
fn type_info(s: &Belt) -> Option<super::belt_poses::TypeInfo> {
    let k = s.grab.kind.as_deref();
    if !custom_for(s, k) {
        return None;
    }
    TYPES.iter().find(|t| Some(t.0) == k).map(|t| t.1)
}
fn arm_mode(s: &Belt) -> &'static str {
    if let Some(ti) = type_info(s) {
        return if ti.pin { "pin" } else { "button" };
    }
    match s.grab.kind.as_deref() {
        Some("recon" | "cutting" | "emp" | "dronelure") => "button",
        _ => "pin",
    }
}
fn pose_for(s: &Belt, which: &str, fallback: Option<Pose>) -> Option<Pose> {
    let Some(k) = s.grab.kind.as_deref() else { return fallback };
    if which != "thumb" && !custom_for(s, Some(k)) {
        return fallback;
    }
    pose(&format!("{k}_{which}")).or(fallback)
}
/// The thumb's two joints turned by `deg` about their z (a press).
fn press_pose(base: Option<Pose>, deg: f64) -> Option<Vec<(String, Q)>> {
    let base = base?;
    let half = deg.to_radians() * 0.5;
    let (sz, cz) = (half.sin(), half.cos());
    Some(
        base.iter()
            .map(|(nm, q)| {
                let q = if *nm == "LeftHandThumb1" || *nm == "LeftHandThumb2" {
                    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
                    [x * cz + y * sz, y * cz - x * sz, z * cz + w * sz, w * cz - z * sz]
                } else {
                    *q
                };
                (nm.to_string(), q)
            })
            .collect(),
    )
}
fn owned(p: Option<Pose>) -> Option<Vec<(String, Q)>> {
    p.map(|p| p.iter().map(|(n, q)| (n.to_string(), *q)).collect())
}
/// holdNow: the hold in force (lever open .. shut, interpolated) and the finger pose with it.
fn hold_now(s: &Belt) -> (V3, Q, Option<Vec<(String, Q)>>) {
    if let (Some(h), Some(t)) = (&s.grab.stick.hold, s.grab.stick.elapsed) {
        if t < 0.95 {
            return h.clone();
        }
    }
    let grenade_pose = pose("grenade_hold");
    let pinch_pose = pose("grenade_pinch");
    let (open_p, open_q, open_pose, shut_p, shut_q, shut_pose);
    if let Some(ti) = type_info(s) {
        let base = pose_for(s, "hold", grenade_pose);
        if ti.pin {
            (open_p, open_q, open_pose) = (ti.p2, ti.q2, owned(base));
            (shut_p, shut_q, shut_pose) = (ti.p1, ti.q1, owned(pose_for(s, "pinch", pinch_pose)));
        } else {
            let mode = s.cfg.v("grenade_press_mode").get(s.grab.kind.as_deref().unwrap_or("")).and_then(J::as_str).unwrap_or("lift").to_string();
            if mode == "swap" {
                (open_p, open_q, open_pose) = (ti.p1, ti.q1, owned(pose_for(s, "thumb", None).or(base)));
                (shut_p, shut_q, shut_pose) = (ti.p2, ti.q2, owned(base));
            } else {
                (open_p, open_q, open_pose) = (ti.p2, ti.q2, press_pose(base, s.cfg.f("grenade_button_lift_deg", 22.0)).or_else(|| owned(base)));
                (shut_p, shut_q, shut_pose) = (ti.p2, ti.q2, owned(base));
            }
        }
    } else {
        (open_p, open_q, open_pose) = (HOLD_P, HOLD_Q, owned(grenade_pose));
        if arm_mode(s) == "button" {
            (shut_p, shut_q) = (HOLD_P, HOLD_Q);
            shut_pose = owned(pose_for(s, "thumb", None)).or_else(|| press_pose(grenade_pose, s.cfg.f("grenade_button_curl_deg", 22.0))).or_else(|| owned(grenade_pose));
        } else {
            (shut_p, shut_q, shut_pose) = (HOLD1_P, HOLD1_Q, owned(pinch_pose));
        }
    }
    let o = s.cfg.f("grenade_lever_open", 0.0);
    let t = o + (1.0 - o) * s.grab.lever;
    if t.abs() < 0.001 {
        return (open_p, open_q, open_pose);
    }
    if t >= 0.999 {
        return (shut_p, shut_q, shut_pose);
    }
    let p = [0, 1, 2].map(|i| open_p[i] + (shut_p[i] - open_p[i]) * t);
    let d: f64 = (0..4).map(|i| open_q[i] * shut_q[i]).sum();
    let sg = if d >= 0.0 { 1.0 } else { -1.0 };
    let mut q = [0, 1, 2, 3].map(|i| open_q[i] + (shut_q[i] * sg - open_q[i]) * t);
    let n = q.iter().map(|c| c * c).sum::<f64>().sqrt().max(1e-6);
    q = q.map(|c| c / n);
    let lever = match (&open_pose, &shut_pose) {
        (Some(a), Some(b)) if a.len() == b.len() => Some(
            a.iter()
                .zip(b)
                .map(|((nm, p), (_, q))| {
                    let d = p[0] * q[0] + p[1] * q[1] + p[2] * q[2] + p[3] * q[3];
                    let sg = if d >= 0.0 { 1.0 } else { -1.0 };
                    let mut v = [0, 1, 2, 3].map(|i| p[i] + (q[i] * sg - p[i]) * t);
                    let n = v.iter().map(|c| c * c).sum::<f64>().sqrt();
                    let n = if n < 1e-6 { 1.0 } else { n };
                    v = v.map(|c| c / n);
                    (nm.clone(), v)
                })
                .collect(),
        ),
        _ => shut_pose.or(open_pose),
    };
    (p, q, lever)
}
fn bone_pos(i: i32) -> Option<V3> {
    native::<[f32; 4]>("VRBoneModelPos", &[Value::Int32(i)]).map(f3)
}
/// The bone's model position with its w (0 when the plugin has none).
fn bone_pos4(i: i32) -> Option<[f64; 4]> {
    native::<[f32; 4]>("VRBoneModelPos", &[Value::Int32(i)]).map(|v| v.map(f64::from))
}
fn bone_rot(i: i32) -> Option<Q> {
    native::<[f32; 4]>("VRBoneModelRot", &[Value::Int32(i)]).map(f4)
}
fn pinch_point(hand: usize) -> Option<V3> {
    let (a, b) = (bone_pos(THUMB_TIP[hand])?, bone_pos(INDEX_TIP[hand])?);
    Some([(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5, (a[2] + b[2]) * 0.5])
}
fn raw_hand(hand: usize) -> Option<V3> {
    native::<[f32; 4]>("VRHandRawModel", &[Value::Int32(hand as i32)]).map(f3)
}
fn grip_down(side: usize) -> bool {
    shared(GRIP_SLOT[side]) > 0.5
}
fn trigger_amount(side: Option<usize>) -> f64 {
    let Some(side) = side else { return 0.0 };
    let v = f64::from(shared(TRIGGER_SLOT[side]));
    if v.is_nan() || !(0.0..=1.0).contains(&v) {
        0.0
    } else {
        v
    }
}
fn claim_hand(hand: Option<usize>) {
    native::<()>("SetVRReloadOwnedHand", &[Value::Int32(hand.map_or(-1, |h| h as i32))]);
}
fn magnet_set(s: &mut Belt, hand: usize, p: V3, q: Option<Q>) {
    claim_hand(Some(hand));
    let v = [p[0] as f32, p[1] as f32, p[2] as f32, 1.0];
    native::<bool>("VRHandStopModel", &[Value::Int32(hand as i32), Value::Bool(true), Value::Ptr(v.as_ptr().cast())]);
    if let Some(q) = q {
        stop_rot(hand, 1, q);
    }
    s.grab.magnet = Some(hand);
}
fn stop_rot(hand: usize, on: i32, q: Q) {
    native::<bool>(
        "VRHandStopRot",
        &[Value::Int32(hand as i32), Value::Int32(on), Value::Float(q[0] as f32), Value::Float(q[1] as f32), Value::Float(q[2] as f32), Value::Float(q[3] as f32)],
    );
}
fn magnet_clear(s: &mut Belt) {
    let Some(h) = s.grab.magnet.take() else { return };
    claim_hand(None);
    let z = [0f32; 4];
    native::<bool>("VRHandStopModel", &[Value::Int32(h as i32), Value::Bool(false), Value::Ptr(z.as_ptr().cast())]);
    stop_rot(h, 0, [0.0, 0.0, 0.0, 1.0]);
}
/// The holding hand owns its trigger; the other keeps its fire or aim. Written every frame while blocking (the slot
/// has other writers), cleared once when the writer changes or the hold ends.
fn set_trigger_block(s: &mut Belt, on: bool) {
    let writer = on.then(|| {
        let primary = if super::belt::left_handed() { 0 } else { 1 };
        if s.grab.side == Some(primary) { "SetVRTriggerMode" } else { "SetVRLeftTriggerBlock" }
    });
    if let Some(old) = s.grab.trigger_writer.filter(|w| Some(*w) != writer) {
        native::<()>(old, &[Value::Int32(0)]);
    }
    s.grab.trigger_writer = writer;
    if let Some(w) = writer {
        native::<()>(w, &[Value::Int32(1)]);
    }
}
fn hand_bone(hand: usize, name: &str) -> String {
    if hand == 1 {
        if let Some(rest) = name.strip_prefix("Left") {
            return format!("Right{rest}");
        }
    } else if let Some(rest) = name.strip_prefix("Right") {
        return format!("Left{rest}");
    }
    name.to_string()
}
fn pose_apply(s: &mut Belt, hand: usize, blend: f64, p: Option<Vec<(String, Q)>>) {
    let p = p.or_else(|| hold_now(s).2);
    let Some(p) = p else { return };
    native::<i32>("VRReloadFingerClear", &[Value::Int32(hand as i32)]);
    let held = s.grab.side == Some(hand);
    if held {
        s.grab.stick.thumb = Some([None; 3]);
    }
    let elapsed = s.grab.stick.elapsed;
    let flick = if held { elapsed.map(contact::flick_keys) } else { None };
    let timeline = match (flick, s.grab.stick.profile, elapsed) {
        (Some(_), Some(pr), Some(t)) => Some((pr, t, contact::sample(pr, t))),
        _ => None,
    };
    // the hold the fingers bend from: frozen with the timeline's start until 0.95 s, live after
    let lever = match (elapsed, s.grab.stick.hold_lever) {
        (Some(t), Some(l)) if t < 0.95 => l,
        _ => s.grab.lever,
    };
    let idle = if held { contact::profile(hand, s.grab.kind.as_deref()) } else { None };
    let released_at = s.grab.stick.contact.as_ref().and_then(|c| c.released_at);
    for (nm, q) in &p {
        let left = hand_bone(0, nm);
        let mut x = *q;
        if let Some(iq) = idle.and_then(|pr| contact::idle_finger(pr, &left, s.grab.lever)) {
            x = iq;
        }
        if let (Some((pr, t, (ca, cb, cu))), Some((fa, fb, fu))) = (timeline, flick) {
            x = contact::finger(pr, t, &left, x, ca, cb, cu, released_at, lever).unwrap_or_else(|| contact::flick_finger(&left, x, fa, fb, fu));
        } else if let Some((fa, fb, fu)) = flick {
            x = contact::flick_finger(&left, x, fa, fb, fu);
        }
        let bone = tdb_str(&hand_bone(hand, nm));
        native::<i32>(
            "VRReloadFingerSet",
            &[Value::Int32(hand as i32), bone.value(), Value::Float(x[0] as f32), Value::Float(x[1] as f32), Value::Float(x[2] as f32), Value::Float(x[3] as f32)],
        );
        if held {
            if let (Some(i), Some(th)) = (contact::thumb_index(&left), s.grab.stick.thumb.as_mut()) {
                th[i - 1] = Some(x);
            }
        }
    }
    native::<i32>("VRReloadFingerBlend", &[Value::Int32(hand as i32), Value::Float(blend as f32)]);
    native::<i32>("VRReloadFingerApply", &[Value::Int32(hand as i32), Value::Int32(1)]);
}
fn pose_clear(s: &mut Belt) {
    let Some(h) = s.grab.pose_hand.take() else { return };
    native::<i32>("VRReloadFingerApply", &[Value::Int32(h as i32), Value::Int32(0)]);
    s.grab.pose_blend = 0.0;
}
fn pose_tick(s: &mut Belt, hand: Option<usize>, dt: f64) {
    if !s.cfg.b("grenade_grab_pose") {
        if s.grab.pose_hand.is_some() {
            pose_clear(s);
        }
        return;
    }
    let Some(hand) = hand else {
        if let Some(ph) = s.grab.pose_hand {
            s.grab.pose_blend -= dt * 6.0;
            if s.grab.pose_blend <= 0.0 {
                pose_clear(s);
            } else {
                pose_apply(s, ph, s.grab.pose_blend, None);
            }
        }
        return;
    };
    if s.grab.pose_hand.is_some_and(|h| h != hand) {
        pose_clear(s);
    }
    s.grab.pose_hand = Some(hand);
    s.grab.pose_blend = (s.grab.pose_blend + dt * 6.0).min(1.0);
    pose_apply(s, hand, s.grab.pose_blend, None);
}
fn pin_pose(s: &Belt) -> Option<Vec<(String, Q)>> {
    owned(pose_for(s, if arm_mode(s) == "pin" { "pin" } else { "press" }, pose("grenade_pin_left")))
}
fn pin_pose_tick(s: &mut Belt, hand: Option<usize>, want: f64, dt: f64) {
    match hand.filter(|_| want > 0.0) {
        None => {
            if let Some(ph) = s.grab.pin_hand {
                s.grab.pin_blend -= dt * 6.0;
                if s.grab.pin_blend <= 0.0 {
                    native::<i32>("VRReloadFingerApply", &[Value::Int32(ph as i32), Value::Int32(0)]);
                    (s.grab.pin_hand, s.grab.pin_blend) = (None, 0.0);
                } else {
                    pose_apply(s, ph, s.grab.pin_blend, pin_pose(s));
                }
            }
        }
        Some(hand) => {
            if let Some(ph) = s.grab.pin_hand.filter(|h| *h != hand) {
                native::<i32>("VRReloadFingerApply", &[Value::Int32(ph as i32), Value::Int32(0)]);
                s.grab.pin_blend = 0.0;
            }
            s.grab.pin_hand = Some(hand);
            let step = dt * 6.0;
            s.grab.pin_blend = if s.grab.pin_blend < want { want.min(s.grab.pin_blend + step) } else { want.max(s.grab.pin_blend - step) };
            pose_apply(s, hand, s.grab.pin_blend, pin_pose(s));
        }
    }
}
fn play_snd(s: &mut Belt, which: &str) {
    if s.cfg.v("grenade_sound") == &J::Bool(false) || (which == "stow" && s.grab.mute_stow) {
        return;
    }
    let default = match which {
        "grab" => "grenade_equip",
        "stow" => "grenade_unequip",
        "throw" => "grenade_throw",
        "pin" => "grenade_arm_pin",
        "button" => "grenade_arm_button",
        "lever" => "w_expl_frag_grenade_foley_raise_cover",
        "leverOff" => "w_expl_frag_grenade_foley_lower_cover",
        _ => "",
    };
    let name = s.cfg.v(&format!("grenade_snd_{which}")).as_str().unwrap_or(default).to_string();
    if name.is_empty() {
        return;
    }
    let id = if s.grab.obj.is_null() { super::belt::entity_hash(&player()) } else { super::belt::entity_hash(&s.grab.obj) };
    let Some(id) = id else { return };
    let t = super::belt::now();
    let gap = if which == "lever" || which == "leverOff" { s.cfg.f("grenade_snd_gap_lever", 0.45) } else { s.cfg.f("grenade_snd_gap", 0.12) };
    if let Some(e) = s.grab.snd_at.iter_mut().find(|e| e.0 == which) {
        if t - e.1 < gap {
            return;
        }
        e.1 = t;
    } else {
        s.grab.snd_at.push((which.to_string(), t));
    }
    let emitter = s.cfg.v("grenade_snd_emitter").get(s.grab.kind.as_deref().unwrap_or("")).and_then(J::as_str).unwrap_or("").to_string();
    let audio = super::game_system("GetAudioSystem");
    let _ = m::<()>(&audio, "Play", &[Value::U64(CName::hashed(&name).0), Value::U64(id), Value::U64(if emitter.is_empty() { 0 } else { CName::hashed(&emitter).0 })]);
}
fn lever_tick(s: &mut Belt, dt: f64) {
    let Some(side) = s.grab.side.filter(|_| s.cfg.b("grenade_lever")) else {
        (s.grab.lever, s.grab.lever_shut) = (0.0, false);
        return;
    };
    let want = if s.grab.pin_out { 1.0 } else { trigger_amount(Some(side)) };
    let step = dt * s.cfg.f("grenade_lever_speed", 8.0);
    s.grab.lever = if s.grab.lever < want { want.min(s.grab.lever + step) } else { want.max(s.grab.lever - step) };
    let has_lever = arm_mode(s) == "pin";
    if !s.grab.lever_shut && s.grab.lever >= 0.7 {
        s.grab.lever_shut = true;
        if arm_mode(s) == "button" && !s.grab.pin_out {
            s.grab.lever_arm_at = Some(super::belt::now());
        }
        if has_lever {
            play_snd(s, "lever");
        }
    } else if s.grab.lever_shut && s.grab.lever <= 0.3 {
        s.grab.lever_shut = false;
        s.grab.lever_arm_at = None;
        if has_lever {
            play_snd(s, "leverOff");
        }
    }
    if arm_mode(s) == "button" && !s.grab.pin_out && s.grab.lever_arm_at.is_some_and(|a| super::belt::now() - a >= s.cfg.f("grenade_button_hold_s", 0.35)) {
        (s.grab.pin_out, s.grab.lever_arm_at) = (true, None);
        play_snd(s, "button");
        log!("[belt] grenade: armed by button (from Rust)\n");
    }
}
/// The prop's parts (its meshes that can be placed) and the pin's, once it exists.
fn grenade_parts(s: &mut Belt) -> Option<Vec<Handle>> {
    if let Some(p) = &s.grab.parts {
        return Some(p.clone());
    }
    let slot = s.grab.slot.clone()?;
    let e = in_slot(&player(), &slot);
    if e.is_null() {
        return None;
    }
    let comps = super::belt::components(&e);
    let list: Vec<Handle> = comps.iter().filter(|c| super::belt::class_name(c).contains("Mesh") && has(c, "SetLocalTransform")).cloned().collect();
    if list.is_empty() {
        return None;
    }
    s.grab.parts = Some(list.clone());
    s.grab.obj = e;
    s.grab.pin = None;
    let mut idx = None;
    let mut seen = 0;
    for c in &comps {
        if super::belt::class_name(c).contains("SkinnedMesh") {
            if super::belt::comp_name(c).to_lowercase().contains("safe_pin") {
                idx = Some(seen);
                break;
            }
            seen += 1;
        }
    }
    if let Some(idx) = idx {
        let want = [if idx == 0 { "mag_mesh".to_string() } else { format!("mag_mesh_{idx:02}") }, format!("vrp_lit_{idx}")];
        let pins: Vec<Handle> = list
            .iter()
            .filter(|c| {
                let n = super::belt::comp_name(c);
                want.contains(&n) || n.to_lowercase().contains("safe_pin")
            })
            .cloned()
            .collect();
        if !pins.is_empty() {
            s.grab.pin = Some(pins);
        }
    }
    let shown = !s.grab.pin_out || s.grab.stick.elapsed.is_some();
    for c in s.grab.pin.iter().flatten() {
        super::belt::toggle(c, shown);
    }
    Some(list)
}
fn grenade_hold_pose(s: &mut Belt) {
    let Some(parts) = grenade_parts(s) else { return };
    let Some(side) = s.grab.side else { return };
    let (Some(wp), Some(wq), Some(ap), Some(aq)) = (bone_pos(WRIST_BONE[side]), bone_rot(WRIST_BONE[side]), bone_pos(ANCHOR_BONE[side]), bone_rot(ANCHOR_BONE[side])) else {
        return;
    };
    let (mut x, mut hq, _) = hold_now(s);
    // Behaviour-mirrored wrists have opposite local translations; the item also takes a half-turn about its own X to keep
    // its visible orientation (the hold was recorded against the right wrist).
    if side == 0 {
        x = x.map(|c| -c);
        hq = [hq[3], hq[2], -hq[1], -hq[0]];
    }
    if let Some(sh) = contact::profile(side, s.grab.kind.as_deref()).and_then(|p| p.idle_shift) {
        x = [x[0] + sh[0], x[1] + sh[1], x[2] + sh[2]];
    }
    if let (Some(t), Some(pr)) = (s.grab.stick.elapsed, s.grab.stick.profile) {
        let (a, b, u) = contact::sample(pr, t);
        let (r, d) = contact::grip(pr, a, b, u);
        let (php, phq) = pr.hold;
        let d = qrot(phq, d);
        let r = qmul(phq, r);
        (x, hq) = contact::blend_grip(t, (x, hq), ([php[0] + d[0], php[1] + d[1], php[2] + d[2]], r));
    }
    let r = qrot(wq, x);
    let hpos = [wp[0] + r[0], wp[1] + r[1], wp[2] + r[2]];
    let mq = qmul(wq, hq);
    (s.grab.item_p, s.grab.item_q) = (Some(hpos), Some(mq));
    let mut lq = qmul(conj(aq), mq);
    let mut lp = qrot(conj(aq), [hpos[0] - ap[0], hpos[1] - ap[1], hpos[2] - ap[2]]);
    let mut o = s.cfg.v("grenade_hold").clone();
    if let Some(k) = s.grab.kind.as_deref() {
        if let Some(by) = s.cfg.v("grenade_hold_kind").get(k) {
            o = by.clone();
        }
    }
    let g = |k: &str| o.get(k).and_then(J::as_f64).unwrap_or(0.0);
    lq = qmul(lq, euler_quat(g("rx"), g("ry"), g("rz")));
    lp = [lp[0] + g("x"), lp[1] + g("y"), lp[2] + g("z")];

    // THE THUMB ON THE PIN: its tip's segment, in the item's frame, drives the constrained pin.
    let elapsed = s.grab.stick.elapsed;
    let profile = s.grab.stick.profile;
    if let (Some(t), Some(pr), Some(mut c)) = (elapsed, profile, s.grab.stick.contact.take()) {
        let mut a = bone_pos4(THUMB_TIP[side]);
        let mut tq = bone_rot(THUMB_TIP[side]);
        // The local rotations scheduled for this rendered frame: native FK snapshots still hold the previous one.
        if let Some([Some(q1), Some(q2), Some(q3)]) = s.grab.stick.thumb {
            let (mut p, mut qq) = (wp, wq);
            for (joint, jq) in [q1, q2, q3].iter().enumerate() {
                let o = qrot(qq, pr.offsets[joint]);
                p = [p[0] + o[0], p[1] + o[1], p[2] + o[2]];
                qq = qmul(qq, *jq);
            }
            (a, tq) = (Some([p[0], p[1], p[2], 1.0]), Some(qq));
        }
        if let (Some(a4), Some(tq)) = (a, tq) {
            if a4[3] != 0.0 {
                let av = [a4[0], a4[1], a4[2]];
                let d = qrot(tq, pr.tip);
                let b = [av[0] + d[0], av[1] + d[1], av[2] + d[2]];
                let item_point = |p: V3| {
                    let x = qrot(conj(aq), [p[0] - ap[0], p[1] - ap[1], p[2] - ap[2]]);
                    qrot(conj(lq), [x[0] - lp[0], x[1] - lp[1], x[2] - lp[2]])
                };
                let dt = if s.grab.last_dt > 0.0 { s.grab.last_dt } else { 0.016 };
                if contact::step(&mut c, pr, item_point(av), item_point(b), dt) {
                    s.grab.stick.contact = Some(c.clone());
                    finish_pin(s, true);
                    s.grab.stick.flight.at = Some(t);
                    c.released_at = Some(t);
                    native::<()>("VRHapticEvent", &[Value::Int32(2), Value::Int32(side as i32 + 1)]);
                    log!("[belt] grenade: thumb constraint released at {t:.2} force {:.1} (from Rust)\n", c.force);
                } else if c.force > 1.0 && !c.feedback {
                    c.feedback = true;
                    native::<()>("VRHapticEvent", &[Value::Int32(3), Value::Int32(side as i32 + 1)]);
                }
            }
        }
        s.grab.stick.contact = Some(c);
    }

    // THE PIN IS NOT WRITTEN WITH THE BODY while a hand or the thumb is on it: it gets a transform of its own.
    let pins: Vec<usize> = s.grab.pin.iter().flatten().map(|c| c.instance as usize).collect();
    let pulling = s.grab.pin_pull.is_some();
    let released = s.grab.stick.contact.as_ref().is_some_and(|c| c.phase == Phase::Released);
    let constrained = s.grab.stick.contact.as_ref().and_then(|c| c.along.map(|_| c.offset));
    let set = |c: &Handle, p: V3, q: Q| {
        let (pv, qv) = ([p[0] as f32, p[1] as f32, p[2] as f32, 1.0], super::belt::q4(q));
        let _ = m::<()>(c, "SetLocalTransform", &[Value::Ptr(pv.as_ptr().cast()), Value::Ptr(qv.as_ptr().cast())]);
    };
    for c in &parts {
        if !(pins.contains(&(c.instance as usize)) && (pulling || elapsed.is_some())) {
            set(c, lp, lq);
        }
    }
    let (mut pin_p, mut pin_q) = (lp, lq);
    if let (Some(_), Some(t), Some(pr)) = (&s.grab.pin, elapsed, profile) {
        let r = contact::pin_rotation(pr, t);
        let h = pr.hinge;
        let hr = qrot(r, h);
        let d = qrot(lq, [h[0] - hr[0], h[1] - hr[1], h[2] - hr[2]]);
        (pin_p, pin_q) = ([lp[0] + d[0], lp[1] + d[1], lp[2] + d[2]], qmul(lq, r));
    }
    if let (Some(pin), true, Some(t), Some(pr)) = (s.grab.pin.clone(), released, elapsed, profile) {
        // THE RING FLIES OFF: from where the thumb let it go, along its launch, falling, spinning and shrinking away.
        let pivot = pr.original_centre;
        if s.grab.stick.flight.start.is_none() {
            let c = s.grab.stick.contact.as_ref().expect("released");
            let fr = qmul(aq, pin_q);
            let o = qrot(aq, pin_p);
            let pv = qrot(fr, pivot);
            let bq = qmul(aq, lq);
            let d = qrot(bq, c.offset);
            let start = [ap[0] + o[0] + pv[0] + d[0], ap[1] + o[1] + pv[1] + d[1], ap[2] + o[2] + pv[2] + d[2]];
            s.grab.stick.flight.start = Some((start, fr, qrot(bq, c.velocity)));
        }
        let (start, fr, v) = s.grab.stick.flight.start.expect("set above");
        let time = (t - s.grab.stick.flight.at.unwrap_or(t)).max(0.0);
        let dp = [v[0] * time, v[1] * time, v[2] * time - 1.4 * time * time];
        let spin = if side == 0 { -1.0 } else { 1.0 } * time * 12.0;
        let tilt = time * 5.0;
        let scale = (1.0 - (time - 0.25).max(0.0) * 4.0).max(0.05);
        let mut r = qmul([0.0, 0.0, (spin / 2.0).sin(), (spin / 2.0).cos()], [0.0, (tilt / 2.0).sin(), 0.0, (tilt / 2.0).cos()]);
        r = qmul(r, fr);
        r = qmul(conj(aq), r);
        let a = qrot(conj(aq), [start[0] + dp[0] - ap[0], start[1] + dp[1] - ap[1], start[2] + dp[2] - ap[2]]);
        let pp = qrot(r, pivot.map(|c| c * scale));
        ring_scale(s, scale);
        for c in &pin {
            set(c, [a[0] - pp[0], a[1] - pp[1], a[2] - pp[2]], r);
        }
    } else if let (Some(pin), Some(_)) = (&s.grab.pin, elapsed) {
        let off = constrained.unwrap_or([0.0; 3]);
        let d = qrot(lq, off);
        for c in pin {
            set(c, [pin_p[0] + d[0], pin_p[1] + d[1], pin_p[2] + d[2]], pin_q);
        }
    } else if let (Some(pin), Some(pull)) = (&s.grab.pin, s.grab.pin_pull) {
        let w = qrot(mq, pull);
        let a = qrot(conj(aq), w);
        for c in pin {
            set(c, [lp[0] + a[0], lp[1] + a[1], lp[2] + a[2]], lq);
        }
    }
}
fn pin_axis(s: &Belt) -> V3 {
    let by = s.grab.kind.as_deref().and_then(|k| s.cfg.v("grenade_pin_axis_kind").get(k).cloned());
    let a = by.unwrap_or_else(|| s.cfg.v("grenade_pin_axis").clone());
    let g = |k: &str, d: f64| a.get(k).and_then(J::as_f64).unwrap_or(d);
    let (x, y, z) = (g("x", -0.0005), g("y", -0.0188), g("z", 0.0125));
    let n = (x * x + y * y + z * z).sqrt();
    if n < 1e-6 {
        [0.0, -1.0, 0.0]
    } else {
        [x / n, y / n, z / n]
    }
}
fn update_pin(s: &mut Belt, dt: f64) {
    if arm_mode(s) == "button" {
        pin_pose_tick(s, None, 0.0, dt);
        magnet_clear(s);
        return;
    }
    let held = s.grab.side.unwrap_or(1);
    let free = if held == 1 { 0 } else { 1 };
    let pl = player();
    let free_armed = s.grab.slot.is_some() && player_plane(s, &pl, super::belt::now()) == Some(2) && super::belt::weapon_in_hands(&pl, Some(free));
    if !s.cfg.b("grenade_pin") || s.grab.slot.is_none() || s.grab.pin_out || s.grab.stick.elapsed.is_some() || free_armed {
        pin_pose_tick(s, None, 0.0, dt);
        magnet_clear(s);
        return;
    }
    let (Some(ip), Some(iq)) = (s.grab.item_p, s.grab.item_q) else {
        pin_pose_tick(s, None, 0.0, dt);
        return;
    };
    let (Some(fp), Some(c)) = (bone_pos(WRIST_BONE[free]), pinch_point(free)) else {
        pin_pose_tick(s, None, 0.0, dt);
        magnet_clear(s);
        return;
    };
    let off = s.grab.kind.as_deref().and_then(|k| s.cfg.v("grenade_pin_off_kind").get(k).cloned()).unwrap_or_else(|| s.cfg.v("grenade_pin_off").clone());
    let g = |k: &str| off.get(k).and_then(J::as_f64).unwrap_or(0.0);
    let mut d3 = [0.0; 3];
    if let (Some(ti), Some(base)) = (type_info(s), TYPES.iter().find(|t| t.0 == "frag").map(|t| t.1)) {
        d3 = [0, 1, 2].map(|i| ti.pin_p[i] - base.pin_p[i]);
    }
    let pp_l = qrot(iq, [PIN_P[0] + g("x") + d3[0], PIN_P[1] + g("y") + d3[1], PIN_P[2] + g("z") + d3[2]]);
    let pp = [ip[0] + pp_l[0], ip[1] + pp_l[1], ip[2] + pp_l[2]];
    let d = dist(c, pp);
    let grip = s.cfg.f("grenade_pin_grip_m", 0.05);
    let prev = s.cfg.f("grenade_pin_preview_m", 0.12);
    if d > prev && s.grab.magnet.is_none() {
        (s.grab.pin_at, s.grab.pin_pull, s.grab.pin_prev, s.grab.pin_speed, s.grab.grab_q) = (None, None, None, 0.0, None);
        pin_pose_tick(s, Some(free), 0.0, dt);
        return;
    }
    let mut want = 1.0;
    if d > grip {
        want = (1.0 - (d - grip) / (prev - grip).max(1e-3)).max(0.0);
    }
    pin_pose_tick(s, Some(free), want, dt);
    let wrist_off = [c[0] - fp[0], c[1] - fp[1], c[2] - fp[2]];
    if s.grab.magnet.is_none() {
        if d <= grip && grip_down(free) {
            let w = raw_hand(free).unwrap_or(c);
            s.grab.pin_at = Some(qrot(conj(iq), [w[0] - ip[0], w[1] - ip[1], w[2] - ip[2]]));
            (s.grab.pin_pull, s.grab.pin_prev, s.grab.pin_speed) = (Some([0.0; 3]), None, 0.0);
            let fq = bone_rot(WRIST_BONE[free]);
            magnet_set(s, free, [pp[0] - wrist_off[0], pp[1] - wrist_off[1], pp[2] - wrist_off[2]], fq);
            s.grab.grab_q = fq;
            trace_line(&format!("pin: magnet on, pinch {d:.3} from ring"));
        }
        return;
    }
    if !grip_down(free) {
        magnet_clear(s);
        (s.grab.pin_at, s.grab.pin_pull, s.grab.pin_prev, s.grab.pin_speed, s.grab.grab_q) = (None, None, None, 0.0, None);
        trace_line("pin: released without pulling");
        return;
    }
    let mut axial = 0.0;
    if let Some(at) = s.grab.pin_at {
        let w = raw_hand(free).unwrap_or(c);
        let r = qrot(conj(iq), [w[0] - ip[0], w[1] - ip[1], w[2] - ip[2]]);
        let mv = [r[0] - at[0], r[1] - at[1], r[2] - at[2]];
        let u = pin_axis(s);
        axial = mv[0] * u[0] + mv[1] * u[1] + mv[2] * u[2];
        let along = axial.max(0.0).min(s.cfg.f("grenade_pin_slide_m", 0.012));
        s.grab.pin_pull = Some([u[0] * along, u[1] * along, u[2] * along]);
        if let Some(pv) = s.grab.pin_prev {
            let iv = 1.0 / if dt > 1e-3 { dt } else { 0.016 };
            let v = dist(r, pv) * iv;
            s.grab.pin_speed += (v - s.grab.pin_speed) * 0.45;
        }
        s.grab.pin_prev = Some(r);
    }
    claim_hand(s.grab.magnet);
    let mut t = pp;
    if let Some(pull) = s.grab.pin_pull {
        let w = qrot(iq, pull);
        t = [t[0] + w[0], t[1] + w[1], t[2] + w[2]];
    }
    if let Some(mh) = s.grab.magnet {
        let v = [(t[0] - wrist_off[0]) as f32, (t[1] - wrist_off[1]) as f32, (t[2] - wrist_off[2]) as f32, 1.0];
        native::<bool>("VRHandStopModel", &[Value::Int32(mh as i32), Value::Bool(true), Value::Ptr(v.as_ptr().cast())]);
        if let Some(q) = s.grab.grab_q {
            stop_rot(mh, 1, q);
        }
    }
    if s.grab.pin_at.is_some() {
        let past = axial - s.cfg.f("grenade_pin_slide_m", 0.012);
        if past >= s.cfg.f("grenade_pin_pull_m", 0.06) && s.grab.pin_speed >= s.cfg.f("grenade_pin_speed", 2.0) {
            finish_pin(s, false);
            native::<()>("VRHapticEvent", &[Value::Int32(2), Value::Int32(if free == 1 { 2 } else { 1 })]);
            native::<()>("VRHapticEvent", &[Value::Int32(3), Value::Int32(if held == 1 { 2 } else { 1 })]);
            log!("[belt] grenade: pin pulled ({:.0} mm past the stop at {:.1} m/s) (from Rust)\n", past * 1000.0, s.grab.pin_speed);
        }
    }
}
fn throw_sample(s: &mut Belt, side: Option<usize>, dt: f64) {
    let Some(side) = side else {
        (s.grab.t_prev, s.grab.tv, s.grab.hist, s.grab.hn) = (None, [0.0; 3], Vec::new(), 0);
        return;
    };
    let Some(p) = raw_hand(side) else { return };
    if let Some(prev) = s.grab.t_prev {
        let iv = 1.0 / if dt > 1e-3 { dt } else { 0.016 };
        for i in 0..3 {
            s.grab.tv[i] += ((p[i] - prev[i]) * iv - s.grab.tv[i]) * 0.45;
        }
    }
    s.grab.t_prev = Some(p);
    s.grab.hn += 1;
    let e = (s.grab.tv, super::belt::now());
    let slot = (s.grab.hn - 1) % HIST;
    if s.grab.hist.len() <= slot {
        s.grab.hist.push(e);
    } else {
        s.grab.hist[slot] = e;
    }
}
fn release_velocity(s: &Belt) -> V3 {
    let t = super::belt::now();
    let win = s.cfg.f("grenade_throw_window", 0.20);
    let n = s.grab.hn.min(HIST);
    let seq: Vec<V3> = (0..n)
        .filter_map(|k| s.grab.hist.get((s.grab.hn + HIST * 2 - 1 - k) % HIST))
        .filter(|q| t - q.1 <= win)
        .map(|q| q.0)
        .collect();
    if seq.is_empty() {
        return s.grab.tv;
    }
    let len = |v: &V3| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let (mut peak, mut peak_len) = (0, -1.0);
    for (i, q) in seq.iter().enumerate() {
        if len(q) > peak_len {
            (peak_len, peak) = (len(q), i);
        }
    }
    let p = seq[peak];
    if peak > 0 && peak + 1 < seq.len() {
        let (a, b) = (seq[peak - 1], seq[peak + 1]);
        return [0, 1, 2].map(|i| (a[i] + p[i] + b[i]) / 3.0);
    }
    p
}
fn hand_forward(side: usize) -> Option<V3> {
    let w = bone_pos(WRIST_BONE[side])?;
    let c = pinch_point(side)?;
    let d = [c[0] - w[0], c[1] - w[1], c[2] - w[2]];
    let n = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    (n >= 1e-4).then(|| d.map(|v| v / n))
}
fn consume(s: &Belt, pl: &Handle, rec: Option<&str>) {
    if !s.cfg.b("grenade_throw_consume") {
        return;
    }
    match s.cfg.v("grenade_consume_mode").as_str().unwrap_or("charge") {
        "charge" => {
            super::hold::spend_grenade_charge(pl);
        }
        "item" => {
            if let Some(rec) = rec {
                if let Some(id) = st::<[u64; 2]>("ItemID", "FromTDBID", &[tdb(rec)]) {
                    let _ = m::<bool>(&transactions(), "RemoveItem", &[Value::Handle(pl), Value::Ptr(id.as_ptr().cast()), Value::Int32(1)]);
                }
            }
        }
        _ => {}
    }
}
fn active_grenade(pl: &Handle) -> Option<String> {
    let pd = super::belt::h(&super::belt::h(&super::game_system("GetScriptableSystemsContainer"), "Get", &[super::belt::cn("EquipmentSystem")]), "GetPlayerData", &[Value::Handle(pl)]);
    let id = m::<[u64; 2]>(&pd, "GetActiveGadget", &[]).ok().filter(super::belt::is_valid)?;
    let name = record_name(id[0]).filter(|n| !n.is_empty())?;
    let t = super::weapon::flat_u64(&format!("{name}.itemType"));
    if t.and_then(record_name).is_some_and(|n| !n.contains("Gad_Grenade")) {
        return None;
    }
    Some(name)
}
fn throw_arm(s: &mut Belt, pl: &Handle) {
    if s.grab.armed.is_some() {
        return;
    }
    let Some(rec) = active_grenade(pl) else { return };
    if let Some(id) = st::<[u64; 2]>("ItemID", "FromTDBID", &[tdb(&rec)]) {
        if m::<i32>(&transactions(), "GetItemQuantity", &[Value::Handle(pl), Value::Ptr(id.as_ptr().cast())]).is_ok_and(|n| n <= 0) {
            return;
        }
    }
    let busy = s.grab.flights.slots_in_use();
    let slot = THROW_SLOTS.iter().find(|c| {
        if busy.iter().any(|b| b == **c) {
            return false;
        }
        !super::hold::slot_filled(pl, c)
    });
    let Some(slot) = slot.map(|s| s.to_string()) else {
        trace_line(&format!("arm: no free slot ({} in the air)", s.grab.flights.count()));
        return;
    };
    prop_allow_slot(s, &rec, &slot);
    let ok = super::hold::hold_item(pl, &rec, &slot, false);
    if !ok {
        trace_line(&format!("arm: {rec} would not attach"));
        return;
    }
    (s.grab.armed, s.grab.armed_at, s.grab.arm_slot) = (Some(rec.clone()), super::belt::now(), Some(slot.clone()));
    s.grab.rec_by_slot.retain(|e| e.0 != slot);
    s.grab.rec_by_slot.push((slot.clone(), rec));
}
/// M.disarm: the throw slot's real grenade taken away.
pub(super) fn disarm(s: &mut Belt, pl: &Handle) {
    if s.grab.armed.is_none() {
        return;
    }
    let slot = s.grab.arm_slot.take();
    s.grab.armed = None;
    let Some(slot) = slot else { return };
    if !pl.is_null() {
        super::hold::drop_item(pl, &slot);
    }
}
fn model_to_world(pl: &Handle, v: V3) -> V3 {
    match m::<[f32; 4]>(pl, "GetWorldOrientation", &[]) {
        Ok(q) => qrot(f4(q), v),
        Err(_) => v,
    }
}
fn grenade_throw(s: &mut Belt, pl: &Handle, side: usize) -> bool {
    let v = release_velocity(s);
    let sp = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let dir = if sp > s.cfg.f("grenade_throw_dir_min", 0.30) {
        v.map(|c| c / sp)
    } else {
        match hand_forward(side) {
            Some(d) => d,
            None => return false,
        }
    };
    let speed = (sp * s.cfg.f("grenade_throw_gain", 1.0)).clamp(s.cfg.f("grenade_throw_min", 1.5), s.cfg.f("grenade_throw_max", 28.0));
    let Some(rec) = s.grab.armed.clone() else {
        log!("[belt] grenade: nothing armed to throw\n");
        return false;
    };
    let w = model_to_world(pl, dir);
    let from = (!s.grab.obj.is_null()).then(|| (m::<[f32; 4]>(&s.grab.obj, "GetWorldPosition", &[]).ok(), m::<[f32; 4]>(&s.grab.obj, "GetWorldOrientation", &[]).ok()));
    s.grab.mute_stow = true;
    release(s, pl);
    s.grab.mute_stow = false;
    let mut flew = false;
    if let Some((Some(p), q)) = from {
        if s.cfg.not_false("grenade_throw_own") {
            let ent = prop_item_grenade(s).map(|e| e.1).unwrap_or_else(|| "vrp_gren_frag".into());
            let path = format!("{}{ent}.ent", s.cfg.v("grenade_prop_dir").as_str().unwrap_or("base\\vrport\\"));
            let carry = s.cfg.f("grenade_throw_carry", 1.0);
            let mut cv = [0.0; 3];
            if carry > 0.0 {
                if let Ok(pv) = m::<[f32; 4]>(pl, "GetVelocity", &[]) {
                    cv = f3(pv).map(|c| c * carry);
                }
            }
            let q = q.map(f4).unwrap_or([0.0, 0.0, 0.0, 1.0]);
            let cfg = &s.cfg;
            flew = s.grab.flights.launch(cfg, &path, s.grab.arm_slot.clone(), f3(p), q, [w[0] * speed + cv[0], w[1] * speed + cv[1], w[2] * speed + cv[2]]);
        }
    }
    if !flew {
        s.grab.pending_throw = Some(Thrown { slot: s.grab.arm_slot.clone(), dir: w, side, speed, at: super::belt::now(), rec: rec.clone(), armed_at: s.grab.armed_at });
    }
    if s.cfg.v("grenade_throw_consume_at").as_str().unwrap_or("throw") == "throw" {
        consume(s, pl, Some(&rec));
    }
    (s.grab.armed, s.grab.arm_slot) = (None, None);
    log!("[belt] grenade: thrown {rec} at {speed:.1} m/s ({}) (from Rust)\n", if flew { "the port's flight" } else { "the game's launch" });
    true
}
fn throw_tick(s: &mut Belt, pl: &Handle) {
    let done = {
        let dt = s.grab.last_dt;
        let cfg = &s.cfg;
        s.grab.flights.tick(cfg, dt)
    };
    for d in done {
        let mut ok = false;
        if let Some(slot) = &d.slot {
            ok = super::hold::detonate_grenade_at(pl, slot, [f64::from(d.at[0] as f32), f64::from(d.at[1] as f32), f64::from(d.at[2] as f32)]);
        }
        log!("[belt] grenade: detonated at {:.1} {:.1} {:.1} -> {ok}\n", d.at[0], d.at[1], d.at[2]);
        if let Some(slot) = &d.slot {
            if s.cfg.v("grenade_throw_consume_at").as_str() == Some("detonate") {
                let rec = s.grab.rec_by_slot.iter().find(|e| e.0 == *slot).map(|e| e.1.clone());
                consume(s, pl, rec.as_deref());
            }
            s.grab.rec_by_slot.retain(|e| e.0 != *slot);
            if !ok {
                super::hold::drop_item(pl, slot);
            }
        }
    }
    if s.grab.slot.is_some() && s.grab.pin_out && s.cfg.not_false("grenade_throw") {
        throw_arm(s, pl);
    } else if s.grab.pending_throw.is_none() && s.grab.slot.is_none() && !s.grab.flights.flying() {
        disarm(s, pl);
    }
    let Some(p) = &s.grab.pending_throw else { return };
    let slot = p.slot.clone().unwrap_or_default();
    let filled = super::hold::slot_filled(pl, &slot);
    let ready = super::belt::now() - p.armed_at >= s.cfg.f("grenade_throw_ready", 0.15);
    if filled && ready {
        let hand = if p.side == 1 { s.cfg.v("grenade_throw_hand_r").as_str().unwrap_or("RightHand") } else { s.cfg.v("grenade_throw_hand_l").as_str().unwrap_or("LeftHand") }.to_string();
        let (dx, dy, dz, speed) = (p.dir[0] as f32, p.dir[1] as f32, p.dir[2] as f32, p.speed as f32);
        let dir = [f64::from(dx), f64::from(dy), f64::from(dz)];
        let ok = super::hold::throw_grenade_from(pl, &slot, Some(&hand), dir, f64::from(speed));
        let rec = p.rec.clone();
        log!("[belt] grenade: thrown {rec} at {speed:.1} m/s -> {ok}\n");
        if ok {
            play_snd(s, "throw");
            if s.cfg.v("grenade_throw_consume_at").as_str() == Some("detonate") {
                consume(s, pl, Some(&rec));
            }
        }
        s.grab.pending_throw = None;
    } else if super::belt::now() - p.at > 1.0 {
        log!("[belt] grenade: the real one never resolved in {slot} -- throw abandoned\n");
        super::hold::drop_item(pl, &slot);
        s.grab.pending_throw = None;
    }
}
/// M.pin: one put in the hand and left there, from the overlay's window (`host` the belt element it is taken from,
/// "" for none).
pub(super) fn pin(s: &mut Belt, side: usize, host: &str) -> bool {
    let pl = player();
    if pl.is_null() {
        return false;
    }
    if s.grab.slot.is_some() {
        release(s, &pl);
    }
    let ok = grenade_attach(s, &pl, side, host, super::belt::now());
    s.grab.pinned = ok;
    ok
}
/// M.unpin.
pub(super) fn unpin(s: &mut Belt) {
    let pl = player();
    s.grab.pinned = false;
    if !pl.is_null() {
        release(s, &pl);
    }
}
impl Grenade {
    /// The kind of the grenade in the hand (S.grab.kind).
    pub(super) fn kind(&self) -> Option<&str> {
        self.kind.as_deref()
    }
    pub(super) fn is_pinned(&self) -> bool {
        self.pinned
    }
}
/// desiredPlane: the grab plane the panel forces (1 scene, 2 weapon), else the player's.
fn desired_plane(s: &mut Belt, pl: &Handle, t: f64) -> i32 {
    match s.cfg.f("grenade_grab_plane", 0.0) as i64 {
        1 => 0,
        2 => 2,
        _ => player_plane(s, pl, t).unwrap_or(0),
    }
}
fn attach_prop(pl: &Handle, item: &str, slot: &str, plane: i32) -> bool {
    super::hold::hold_item(pl, item, slot, plane == 2)
}
/// The pin ring's components scaled (its flight shrinks it away; 1 again for a fresh hold).
fn ring_scale(s: &Belt, scale: f64) {
    let v = [scale as f32; 3];
    for c in s.grab.pin.iter().flatten() {
        // SAFETY: the component, held; its visualScale a Vector3 written in place.
        if let Some(p) = unsafe { crate::script_call::property(c.instance, "visualScale") } {
            unsafe { *(p as *mut [f32; 3]) = v };
        }
    }
}
fn hide_ring(s: &Belt) {
    for c in s.grab.pin.iter().flatten() {
        super::belt::toggle(c, false);
    }
}
fn reset_prop_ring(s: &Belt) {
    ring_scale(s, 1.0);
    for c in s.grab.pin.iter().flatten() {
        super::belt::toggle(c, true);
    }
}
/// finishPin: the pin out (the ring hidden unless the thumb's flight is still to show it).
fn finish_pin(s: &mut Belt, keep_visible: bool) {
    (s.grab.pin_out, s.grab.pin_pull, s.grab.pin_at) = (true, None, None);
    magnet_clear(s);
    if !keep_visible {
        hide_ring(s);
    }
    play_snd(s, "pin");
}
/// VRGrenadeStick: the hand whose stick click the grenade claims (-1 none), and the clicks claimed so far.
fn grenade_stick(side: i32) -> i32 {
    use core::sync::atomic::Ordering::Relaxed;
    cvr_input::grenade_stick::GRENADE_STICK_HAND.store(if side == 0 || side == 1 { side } else { -1 }, Relaxed);
    cvr_input::grenade_stick::GRENADE_STICK_CLICKS.load(Relaxed) as i32
}
/// VRGrenadeStick(side) -> Int32, the native (main's Input/GrenadeStick.hpp).
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRGrenadeStick(_context: *mut core::ffi::c_void, frame: *mut red4ext_abi::stack::StackFrame, out: *mut core::ffi::c_void, _a4: i64) {
    // SAFETY: the VM's frame of this native (one Int32).
    let mut a = unsafe { crate::native_args::args(frame) };
    let side = a.get::<i32>();
    a.finish();
    let clicks = grenade_stick(side);
    if !out.is_null() {
        // SAFETY: the VM's Int32 result.
        unsafe { *(out as *mut i32) = clicks };
    }
}
/// stickTick: a fresh click of the holding hand's stick starts the pin timeline (with the grip closed and the pin in);
/// the hand clip, the ornament and the haptics share its clock; letting go of the grip ends it.
fn stick_tick(s: &mut Belt, dt: f64) {
    let (Some(seq0), Some(side)) = (s.grab.stick.sequence, s.grab.side) else { return };
    let seq = grenade_stick(side as i32);
    if s.cfg.b("grenade_pin") && !s.grab.pin_out && seq != seq0 && s.grab.stick.elapsed.is_none() && grip_down(side) {
        s.grab.stick.hold = Some(hold_now(s));
        s.grab.stick.hold_lever = Some(s.grab.lever);
        s.grab.stick.profile = contact::profile(side, s.grab.kind.as_deref());
        s.grab.stick.contact = Some(Contact::new());
        s.grab.stick.elapsed = Some(0.0);
        s.grab.stick.flight = RingFlight::default();
        (s.grab.pin_pull, s.grab.pin_at) = (None, None);
        magnet_clear(s);
    }
    s.grab.stick.sequence = Some(seq);
    let Some(e) = s.grab.stick.elapsed else { return };
    let end = |s: &mut Belt| {
        (s.grab.stick.elapsed, s.grab.stick.contact, s.grab.stick.hold, s.grab.stick.hold_lever) = (None, None, None, None);
    };
    if !grip_down(side) || !s.cfg.b("grenade_pin") {
        end(s);
        if s.grab.pin_out {
            hide_ring(s);
        }
        ring_scale(s, 1.0);
        return;
    }
    let t = e + dt;
    s.grab.stick.elapsed = Some(t);
    if t >= contact::DURATION {
        if s.grab.pin_out {
            hide_ring(s);
        } else {
            ring_scale(s, 1.0);
        }
        end(s);
    }
}
fn grenade_attach(s: &mut Belt, pl: &Handle, side: usize, host: &str, t: f64) -> bool {
    let Some((item, entity)) = prop_item_grenade(s) else { return false };
    // never into a hand a weapon is in
    if super::belt::weapon_in_hands(pl, Some(side)) {
        return false;
    }
    let slot = HAND_SLOT[side];
    prop_allow_slot(s, &item, slot);
    let plane = desired_plane(s, pl, t);
    if !attach_prop(pl, &item, slot, plane) {
        return false;
    }
    s.grab.kind = entity.strip_prefix("vrp_gren_").map(str::to_string);
    (s.grab.side, s.grab.slot, s.grab.host) = (Some(side), Some(slot.to_string()), Some(host.to_string()));
    s.grab.plane = Some(plane);
    s.grab.item = Some(item);
    (s.grab.parts, s.grab.pin, s.grab.obj) = (None, None, Handle::null());
    (s.grab.pin_out, s.grab.pin_at, s.grab.pin_pull) = (false, None, None);
    (s.grab.lever, s.grab.lever_shut) = (0.0, false);
    (s.grab.item_p, s.grab.item_q, s.grab.lever_arm_at) = (None, None, None);
    if s.cfg.b("grenade_pin") && arm_mode(s) == "pin" {
        s.grab.stick.sequence = Some(grenade_stick(side as i32));
    }
    s.lit_hand = [None, None];
    play_snd(s, "grab");
    log!("[belt] grenade -> {} hand ({entity}, plane {plane}) (from Rust)\n", if side == 1 { "right" } else { "left" });
    true
}
/// M.release: the grenade back on the belt.
pub(super) fn release(s: &mut Belt, pl: &Handle) {
    let Some(slot) = s.grab.slot.clone() else { return };
    if s.grab.stick.sequence.is_some() {
        grenade_stick(-1);
    }
    let st = &mut s.grab.stick;
    (st.sequence, st.elapsed, st.contact, st.hold, st.thumb) = (None, None, None, None, None);
    // THE PIN GOES BACK IN: the entity outlives the hold.
    reset_prop_ring(s);
    pose_clear(s);
    magnet_clear(s);
    set_trigger_block(s, false);
    (s.grab.lever, s.grab.lever_shut) = (0.0, false);
    play_snd(s, "stow");
    if !pl.is_null() {
        super::hold::drop_item(pl, &slot);
    }
    log!("[belt] grenade released from {slot}\n");
    (s.grab.side, s.grab.slot, s.grab.host, s.grab.plane) = (None, None, None, None);
    (s.grab.parts, s.grab.pinned, s.grab.obj) = (None, false, Handle::null());
    (s.grab.pin, s.grab.item, s.grab.retry_plane_at) = (None, None, None);
}
/// updatePropPlane: only the drawn item changes with the rendering plane -- the input, the animation clock and the
/// equipped grenade stay the same uninterrupted grip; a weapon come into that hand ends the hold.
fn update_prop_plane(s: &mut Belt, pl: &Handle, t: f64) {
    let plane = desired_plane(s, pl, t);
    if Some(plane) == s.grab.plane || s.grab.retry_plane_at.is_some_and(|r| t < r) {
        return;
    }
    let (Some(side), Some(slot), Some(item)) = (s.grab.side, s.grab.slot.clone(), s.grab.item.clone()) else { return };
    if super::belt::weapon_in_hands(pl, Some(side)) {
        release(s, pl);
        return;
    }
    reset_prop_ring(s);
    super::hold::drop_item(pl, &slot);
    (s.grab.parts, s.grab.pin, s.grab.obj) = (None, None, Handle::null());
    if attach_prop(pl, &item, &slot, plane) {
        trace_line(&format!("replane {:?} -> {plane}", s.grab.plane));
        (s.grab.plane, s.grab.retry_plane_at) = (Some(plane), None);
        return;
    }
    if !attach_prop(pl, &item, &slot, s.grab.plane.unwrap_or(0)) {
        release(s, pl);
        return;
    }
    s.grab.retry_plane_at = Some(t + 0.20);
}
/// updateGrenadeGrab: every frame.
pub(super) fn tick(s: &mut Belt, t: f64, dt: f64) {
    TRACE.store(s.cfg.b("grab_trace"), std::sync::atomic::Ordering::Relaxed);
    let pl = player();
    if pl.is_null() {
        return;
    }
    s.grab.last_dt = dt;
    lever_tick(s, dt);
    stick_tick(s, if dt > 0.0 { dt } else { 0.016 });
    throw_sample(s, s.grab.slot.as_ref().and(s.grab.side), dt);
    throw_tick(s, &pl);
    let block = s.grab.slot.is_some() && s.cfg.not_false("grenade_trigger_block");
    set_trigger_block(s, block);
    s.grab.stick.thumb = None;
    pose_tick(s, s.grab.slot.as_ref().and(s.grab.side), dt);
    update_pin(s, dt);
    if !s.cfg.b("grenade_grab") {
        if s.grab.slot.is_some() {
            release(s, &pl);
        }
        return;
    }
    if s.grab.slot.is_some() {
        let side = s.grab.side.unwrap_or(1);
        if s.grab.pinned {
            grenade_hold_pose(s);
            s.lit_hand = [None, None];
            return;
        }
        if !grip_down(side) {
            if s.grab.pin_out && s.cfg.not_false("grenade_throw") && grenade_throw(s, &pl, side) {
                s.lit_hand = [None, None];
                s.grab.was = [grip_down(0), grip_down(1)];
                return;
            }
            release(s, &pl);
        } else {
            grenade_hold_pose(s);
            s.lit_hand = [None, None];
            update_prop_plane(s, &pl, t);
        }
        s.grab.was = [grip_down(0), grip_down(1)];
        return;
    }
    let closed_takes = native::<bool>("VRGripFromFingers", &[]) == Some(true);
    for side in 0..2usize {
        let down = grip_down(side);
        let edge = down && ((closed_takes && !s.grab.spent[side]) || !s.grab.was[side]);
        s.grab.was[side] = down;
        if !down {
            s.grab.spent[side] = false;
        }
        if edge && s.grab.slot.is_none() {
            // Not from a seat: the belt is under the hands on the wheel, and a squeeze there took a grenade.
            if let Some(host) = s.lit_hand[side].clone().filter(|h| h.starts_with("ucbeltgrenade")) {
                if !super::belt::in_vehicle(s) && !super::holster::weapons_forbidden(&pl) {
                    grenade_attach(s, &pl, side, &host, t);
                }
            }
        }
    }
}
