//! mods/cet/CyberpunkVRPort_Reload/reload/reload.lua: the hand-driven reload of a held gun -- its slide, magazine,
//! hatch, hammer, cylinder and the free hand's finger poses -- written into the weapon's rig through the plugin's rig
//! natives (VRRigWrite*, VRRigTrackWrite), the magazine carried by a hand (a carrier component or an item), dropped
//! with its own contact physics, fed back from the belt's pouch. The game's own reload wins whenever it runs.
//!
//! Part 1: the module's state and helpers (reload.lua up to M.frame).
use super::belt::{conj, f3, f4, has, m, native, player, qmul, qrot, st, tdb_str, transactions, V3, Q};
use super::reload_data::{self as data, Lua, Pose};
use crate::script_call::Value;
use cvr_host::log;
use red4ext_abi::cname::CName;
use red4ext_abi::handle::Handle;
use serde_json::Value as J;
use std::collections::HashMap;

pub(super) const BUILD: &str = "2232-ammo-once";
pub(super) const PULL_MS: f64 = 180.0;
pub(super) const PULL_ZETA: f64 = 0.75;
const SIDE_STEPS: usize = 12;
pub(super) const PAD_WRIST: [i32; 2] = [23, 24];
pub(super) const PAD_OFF: [V3; 2] = [[0.1194, -0.0112, -0.0421], [-0.1166, 0.0052, 0.0257]];
const RAY_PRESETS: [&str; 3] = ["Sight Blocker", "Bullet", "Camera"];
const SLOT_BIND: [f64; 7] = [0.0873, -0.0421, -0.0209, 0.5064, 0.5079, -0.5138, 0.4707];

/// The dropped magazine's body and contact model.
pub(super) struct Drop {
    pub radius: f64,
    pub centre: V3,
    pub flat: f64,
    pub long: V3,
    pub thin: V3,
    pub probe: f64,
    pub arm: f64,
    pub inertia: f64,
    pub bounce: f64,
    pub friction: f64,
    pub spin_air: f64,
    pub drag: f64,
    pub rest_e: f64,
    pub spin_damp: f64,
    pub touch_rest: f64,
    pub rest_v: f64,
    pub settle: f64,
    pub max_v: f64,
    pub max_w: f64,
}
pub(super) const DROP: Drop = Drop {
    radius: 0.035,
    centre: [0.0, -0.021, -0.060],
    flat: 0.015,
    long: [0.0, 0.0, 1.0],
    thin: [1.0, 0.0, 0.0],
    probe: 0.028,
    arm: 0.050,
    inertia: 0.0022,
    bounce: 0.22,
    friction: 0.55,
    spin_air: 0.50,
    drag: 0.10,
    rest_e: 0.35,
    spin_damp: 0.10,
    touch_rest: 0.25,
    rest_v: 0.45,
    settle: 0.18,
    max_v: 8.0,
    max_w: 20.0,
};

// ---------------------------------------------------------------- the module's state (reload.lua's locals)
#[derive(Default)]
pub(super) struct Fade {
    pub hand: Option<usize>,
    pub key: Option<String>,
    pub blend: f64,
    pub dir: i32,
}
#[derive(Default)]
pub(super) struct Pull {
    pub key: Option<String>,
    pub x: f64,
    pub v: f64,
}
#[derive(Default)]
pub(super) struct Lead {
    pub p: V3,
    pub d: V3,
}
#[derive(Default)]
pub(super) struct HoldS {
    pub comp: [Handle; 2],
    pub mesh: [Option<String>; 2],
    pub skin: [Option<String>; 2],
    pub on: [bool; 2],
}
#[derive(Default)]
pub(super) struct ItemS {
    pub name: [Option<String>; 2],
    pub comps: [Option<Vec<Handle>>; 2],
}
#[derive(Default)]
pub(super) struct Press {
    pub t: f64,
    pub hand: Option<usize>,
    pub on: bool,
}
pub(super) struct State {
    pub fade: Fade,
    pub pull: Pull,
    pub lead: HashMap<&'static str, Lead>,
    pub hold: HoldS,
    pub item: ItemS,
    pub press: Press,
    pub case_q: Vec<(f64, Handle, String)>,
    pub litter: Vec<u64>,
    pub yield_on: bool,
    pub trg_mode: Option<i32>,
    pub own_tagged: bool,
    pub exit_at: &'static str,
    pub ammo_err: String,
    pub match_cache: HashMap<u64, Option<&'static J>>,
    pub ray_pick: usize,
    pub ray_log: u32,
    pub owned_hand: Option<usize>,
    pub holder_seen: Option<usize>,
    pub holder_for: f64,
    pub holder_ok: Option<usize>,
    pub th_mask_on: Option<i32>,
    pub sg_slide_on: bool,
    pub sg_lt_on: bool,
    pub tune: Tune,
    pub frame: super::reload_frame::FrameState,
    pub rev: super::reload_revolver::Cases,
    pub sg: super::reload_shotgun::Shotgun,
    /// A released pistol magazine directed by a flick of the controller (main's mag_flick.lua).
    pub flick: super::reload_flick::Flick,
}
/// M.tune: the overlay's magazine tuner (the Lua host's window reports it: VRReloadTune).
pub(super) struct Tune {
    pub seat: f64,
    pub hold: bool,
    pub d: V3,
    pub r: V3,
    pub rev: i32,
    pub stage: f64,
    pub lead: f64,
    pub w: [f64; 19],
}
impl Default for Tune {
    fn default() -> Tune {
        Tune { seat: -1.0, hold: false, d: [0.0; 3], r: [0.0; 3], rev: 0, stage: -1.0, lead: 1.0, w: [1.0; 19] }
    }
}
// SAFETY: the handles are made, used and dropped on the game's main thread, where the mechanics tick.
unsafe impl Send for State {}
impl State {
    pub(super) fn new() -> State {
        State {
            fade: Fade::default(),
            pull: Pull::default(),
            lead: HashMap::new(),
            hold: HoldS::default(),
            item: ItemS::default(),
            press: Press { t: -1.0, hand: None, on: false },
            case_q: Vec::new(),
            litter: Vec::new(),
            yield_on: false,
            trg_mode: None,
            own_tagged: false,
            exit_at: "-",
            ammo_err: "-".into(),
            match_cache: HashMap::new(),
            ray_pick: 1,
            ray_log: 0,
            owned_hand: None,
            holder_seen: None,
            holder_for: 0.0,
            holder_ok: None,
            th_mask_on: None,
            sg_slide_on: false,
            sg_lt_on: false,
            tune: Tune::default(),
            frame: super::reload_frame::FrameState::new(),
            rev: Default::default(),
            sg: Default::default(),
            flick: Default::default(),
        }
    }
}

// ---------------------------------------------------------------- the finger poses
fn qscale(q: Q, w: f64) -> Q {
    if (w - 1.0).abs() < 0.001 {
        return q;
    }
    if w <= 0.0 {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2]).sqrt();
    if n < 1e-6 {
        return q;
    }
    let a = 2.0 * n.atan2(q[3]);
    let b = a * w * 0.5;
    let sn = b.sin();
    [q[0] / n * sn, q[1] / n * sn, q[2] / n * sn, b.cos()]
}
/// The tuner's per-joint curl over a pose.
pub(super) fn curl_pose(s: &State, pose: &Pose) -> Pose {
    if s.tune.w.iter().all(|w| (w - 1.0).abs() <= 0.001) {
        return pose.clone();
    }
    pose.iter().enumerate().map(|(i, (n, q))| (n.clone(), qscale(*q, s.tune.w.get(i).copied().unwrap_or(1.0)))).collect()
}
pub(super) fn for_hand(hand: usize, name: &str) -> String {
    if hand == 1 {
        if let Some(r) = name.strip_prefix("Left") {
            return format!("Right{r}");
        }
    } else if let Some(r) = name.strip_prefix("Right") {
        return format!("Left{r}");
    }
    name.to_string()
}
pub(super) fn mix_pose(pa: Option<&Pose>, pb: Option<&Pose>, t: f64) -> Option<Pose> {
    let Some(pa) = pa else { return pb.cloned() };
    let Some(pb) = pb.filter(|pb| t > 0.001 && pb.len() == pa.len()) else { return Some(pa.clone()) };
    if t >= 0.999 {
        return Some(pb.clone());
    }
    Some(
        pa.iter()
            .zip(pb)
            .map(|((n, x), (_, y))| {
                let dp = x[0] * y[0] + x[1] * y[1] + x[2] * y[2] + x[3] * y[3];
                let sg = if dp < 0.0 { -1.0 } else { 1.0 };
                let mut q = [0, 1, 2, 3].map(|i| x[i] + (y[i] * sg - x[i]) * t);
                let l = q.iter().map(|c| c * c).sum::<f64>().sqrt();
                let l = if l < 1e-6 { 1.0 } else { l };
                q = q.map(|c| c / l);
                (n.clone(), q)
            })
            .collect(),
    )
}
pub(super) fn apply_grip(hand: usize, pose: &Pose, alpha0: f64) {
    native::<i32>("VRReloadFingerClear", &[Value::Int32(hand as i32)]);
    for (n, q) in pose {
        let b = tdb_str(&for_hand(hand, n));
        native::<i32>(
            "VRReloadFingerSet",
            &[Value::Int32(hand as i32), b.value(), Value::Float(q[0] as f32), Value::Float(q[1] as f32), Value::Float(q[2] as f32), Value::Float(q[3] as f32)],
        );
    }
    native::<i32>("VRReloadFingerBlend", &[Value::Int32(hand as i32), Value::Float(alpha0 as f32)]);
    native::<i32>("VRReloadFingerApply", &[Value::Int32(hand as i32), Value::Int32(1)]);
}
pub(super) fn clear_grip(hand: usize) {
    native::<i32>("VRReloadFingerApply", &[Value::Int32(hand as i32), Value::Int32(0)]);
}
/// fadeIn: a grip's pose (by name) faded in on `hand`.
pub(super) fn fade_in(s: &mut State, hand: usize, pose_name: Option<&str>) {
    let Some(name) = pose_name else { return };
    if s.fade.hand != Some(hand) || s.fade.key.as_deref() != Some(name) {
        let Some(pose) = data::pose(name) else { return };
        if let Some(fh) = s.fade.hand.filter(|h| *h != hand) {
            clear_grip(fh);
        }
        if s.fade.hand != Some(hand) {
            s.fade.blend = 0.0;
        }
        apply_grip(hand, &curl_pose(s, &pose), s.fade.blend);
        (s.fade.hand, s.fade.key) = (Some(hand), Some(name.to_string()));
    }
    s.fade.dir = 1;
}
/// fadeInTable: a pose given as a table, under its own key.
pub(super) fn fade_in_table(s: &mut State, hand: usize, key: &str, pose: Option<&Pose>) {
    let Some(pose) = pose else { return };
    if s.fade.hand != Some(hand) || s.fade.key.as_deref() != Some(key) {
        if let Some(fh) = s.fade.hand.filter(|h| *h != hand) {
            clear_grip(fh);
        }
        if s.fade.hand != Some(hand) {
            s.fade.blend = 0.0;
        }
        apply_grip(hand, &curl_pose(s, pose), s.fade.blend);
        (s.fade.hand, s.fade.key) = (Some(hand), Some(key.to_string()));
    }
    s.fade.dir = 1;
}
pub(super) fn fade_out(s: &mut State) {
    if s.fade.hand.is_some() {
        s.fade.dir = -1;
    }
}
pub(super) fn fade_reset(s: &mut State) {
    if let Some(h) = s.fade.hand {
        clear_grip(h);
    }
    s.fade = Fade::default();
}
pub(super) fn fade_tick(s: &mut State, dt: f64, t: Option<f64>) {
    let Some(h) = s.fade.hand.filter(|_| s.fade.dir != 0) else { return };
    s.fade.blend += f64::from(s.fade.dir) * (dt / t.unwrap_or(0.15));
    if s.fade.blend >= 1.0 {
        (s.fade.blend, s.fade.dir) = (1.0, 0);
    } else if s.fade.blend <= 0.0 {
        fade_reset(s);
        return;
    }
    native::<i32>("VRReloadFingerBlend", &[Value::Int32(h as i32), Value::Float(s.fade.blend as f32)]);
}
pub(super) fn pull_in_reset(s: &mut State) {
    s.pull = Pull::default();
}
/// pullIn: a critically-damped spring from 0 to 1 over PULL_MS (the hand arriving).
pub(super) fn pull_in(s: &mut State, key: &str, dt: f64) -> f64 {
    if s.pull.key.as_deref() != Some(key) {
        s.pull = Pull { key: Some(key.to_string()), x: 0.0, v: 0.0 };
    }
    let wn = 6.283185307 / (PULL_MS / 1000.0);
    let d = dt.clamp(0.0, 0.1);
    let steps = ((d / 0.002).floor() as usize + 1).min(64);
    let sdt = d / steps as f64;
    for _ in 0..steps {
        let a = -wn * wn * (s.pull.x - 1.0) - 2.0 * PULL_ZETA * wn * s.pull.v;
        s.pull.v += a * sdt;
        s.pull.x += s.pull.v * sdt;
    }
    s.pull.x = s.pull.x.clamp(0.0, 1.0);
    s.pull.x
}

// ---------------------------------------------------------------- grips (the recorded hold, per hand and side)
/// A grip mirrored for the right hand across axis `across` (1-based, as the configs number axes).
pub(super) fn hand_grip(g: &J, hand: usize, across: usize) -> J {
    if hand != 1 || g.is_null() {
        return g.clone();
    }
    let mut m = g.clone();
    let flip = |v: &J| -> J {
        let mut o = v.clone();
        if let Some(a) = o.as_array_mut() {
            if let Some(c) = a.get_mut(across - 1) {
                *c = J::from(-c.as_f64().unwrap_or(0.0));
            }
        }
        o
    };
    if let Some(o) = m.as_object_mut() {
        for k in ["off", "previewOff"] {
            if let Some(v) = g.get(k) {
                o.insert(k.into(), flip(v));
            }
        }
        if let Some(r) = g.v4("rot") {
            let mut hv = [0.0; 3];
            hv[across - 1] = 1.0;
            let q = qmul([hv[0], hv[1], hv[2], 0.0], r);
            o.insert("rot".into(), J::from(q.to_vec()));
        }
    }
    m
}
fn reflect_grip(g: &J, up: usize, across: usize, phi: f64) -> J {
    let (p, q) = (across % 3 + 1, (across + 1) % 3 + 1);
    let (c, sn) = (phi.cos(), phi.sin());
    let mv = |v: &J| -> J {
        let Some(a) = v.as_array() else { return v.clone() };
        let mut o: Vec<f64> = a.iter().map(|x| x.as_f64().unwrap_or(0.0)).collect();
        if o.len() < 3 {
            return v.clone();
        }
        o[up - 1] = -o[up - 1];
        let (a1, b1) = (o[p - 1], o[q - 1]);
        o[p - 1] = c * a1 - sn * b1;
        o[q - 1] = sn * a1 + c * b1;
        J::from(o)
    };
    let mut m = g.clone();
    if let Some(o) = m.as_object_mut() {
        for k in ["off", "previewOff"] {
            if let Some(v) = g.get(k) {
                o.insert(k.into(), mv(v));
            }
        }
        if let Some(r) = g.v4("rot") {
            let mut hv = [0.0; 3];
            hv[up - 1] = 1.0;
            let r1 = qmul([hv[0], hv[1], hv[2], 0.0], r);
            let mut t = [0.0; 3];
            t[across - 1] = (phi * 0.5).sin();
            let q2 = qmul([t[0], t[1], t[2], (phi * 0.5).cos()], r1);
            o.insert("rot".into(), J::from(q2.to_vec()));
        }
    }
    m
}
/// handGrips: the grips a hand may take a part by -- the recorded one, its mirror, or turned round the part.
pub(super) fn hand_grips(g: &J, hand: usize, weapon_basis: bool, side: Option<&str>) -> Vec<J> {
    if g.is_null() {
        return Vec::new();
    }
    let side = if hand != 1 {
        "recorded"
    } else {
        match side {
            Some(s @ ("left" | "right" | "both")) => s,
            _ => "mirror",
        }
    };
    let (across, up) = if weapon_basis { (1, 3) } else { (3, 2) };
    match side {
        "recorded" => vec![g.clone()],
        "mirror" => vec![hand_grip(g, hand, across)],
        _ => {
            let mut list = Vec::new();
            if side == "both" {
                list.push(hand_grip(g, hand, across));
            }
            for k in 0..SIDE_STEPS {
                list.push(reflect_grip(g, up, across, k as f64 * 2.0 * std::f64::consts::PI / SIDE_STEPS as f64));
            }
            list
        }
    }
}
pub(super) fn hand_hold(hand: usize, o: Option<V3>) -> Option<V3> {
    if hand != 1 {
        return o;
    }
    o.map(|o| [-o[0], o[1], o[2]])
}
pub(super) fn hand_hold_rot(hand: usize, k: Q) -> Q {
    if hand != 1 {
        k
    } else {
        [k[0], -k[1], -k[2], k[3]]
    }
}
pub(super) fn aim_hand() -> usize {
    usize::from(native::<bool>("VRLeftHanded", &[]) == Some(true))
}
pub(super) fn support_hand() -> usize {
    usize::from(native::<bool>("VRWeaponInLeftHand", &[]) == Some(true))
}
/// A quaternion from the basis columns f, d, s (Shepperd).
pub(super) fn basis_quat(f: V3, d: V3, s: V3) -> Q {
    let (fx, fy, fz, dx, dy, dz, sx, sy, sz) = (f[0], f[1], f[2], d[0], d[1], d[2], s[0], s[1], s[2]);
    let tr = fx + dy + sz;
    if tr > 0.0 {
        let k = (tr + 1.0).sqrt() * 2.0;
        [(dz - sy) / k, (sx - fz) / k, (fy - dx) / k, 0.25 * k]
    } else if fx > dy && fx > sz {
        let k = (1.0 + fx - dy - sz).sqrt() * 2.0;
        [0.25 * k, (dx + fy) / k, (sx + fz) / k, (dz - sy) / k]
    } else if dy > sz {
        let k = (1.0 + dy - fx - sz).sqrt() * 2.0;
        [(dx + fy) / k, 0.25 * k, (sy + dz) / k, (sx - fz) / k]
    } else {
        let k = (1.0 + sz - fx - dy).sqrt() * 2.0;
        [(sx + fz) / k, (sy + dz) / k, 0.25 * k, (fy - dx) / k]
    }
}
/// The player's position and its world -> model rotation.
pub(super) fn player_frame() -> Option<(V3, Q)> {
    super::belt::model_basis(&player())
}
#[repr(C, align(16))]
pub(super) struct WorldTransform(pub [u32; 8]);
/// A world transform's bytes: the position in the engine's fixed point, the orientation from 0x10.
pub(super) fn world_bytes(p: V3, q: Q) -> [u8; 0x20] {
    let mut b = [0u8; 0x20];
    for k in 0..3 {
        b[k * 4..k * 4 + 4].copy_from_slice(&(((p[k] * 131072.0) as i32) as u32).to_le_bytes());
    }
    for k in 0..4 {
        b[0x10 + k * 4..0x14 + k * 4].copy_from_slice(&(q[k] as f32).to_le_bytes());
    }
    b
}
/// The template spawned at the place (mechanics/ent_spawn.rs): the new entity's id.
pub(super) fn spawn_at(path: &str, p: V3, q: Q) -> Option<u64> {
    super::ent_spawn::spawn(path, p, q, 0)
}
/// Game.VREntitySetWorldTransform on an entity.
pub(super) fn place_ent(e: &Handle, p: V3, q: Q) -> bool {
    !e.is_null() && crate::engine_objects::entity_set_world_transform(e, &world_bytes(p, q))
}
/// The same on a spawned entity by its id.
pub(super) fn place_id(id: u64, p: V3, q: Q) -> bool {
    super::find_entity(id).is_ok_and(|e| place_ent(&e, p, q))
}
/// A slot of the weapon's slot component in the world: (position, orientation).
pub(super) fn slot_world(slot_comp: &Handle, name: &str) -> Option<(V3, Q)> {
    let mut wt = WorldTransform([0; 8]);
    if m::<bool>(slot_comp, "GetSlotTransform", &[Value::U64(CName::hashed(name).0), Value::Out((&mut wt as *mut WorldTransform).cast())]) != Ok(true) {
        return None;
    }
    let p = [0, 1, 2].map(|i| f64::from(wt.0[i] as i32) / 131072.0);
    let q = [4, 5, 6, 7].map(|i| f64::from(f32::from_bits(wt.0[i])));
    Some((p, q))
}
/// slotModel: a slot's position in the player's model space.
pub(super) fn slot_model(slot_comp: &Handle, name: Option<&str>, b: V3, c: Q) -> Option<V3> {
    let (v, _) = slot_world(slot_comp, name?)?;
    Some(qrot(c, [v[0] - b[0], v[1] - b[1], v[2] - b[2]]))
}
/// seatedPose: where the magazine sits in the gun, in the world (the slot, the weapon's own orientation turned by
/// seatRot, moved by originOffset).
pub(super) fn seated_pose(weapon: &Handle, slot_comp: &Handle, mc: &J) -> Option<(V3, Q)> {
    let (p, _) = slot_world(slot_comp, mc.s("slot")?)?;
    let wq = f4(m::<[f32; 4]>(weapon, "GetWorldOrientation", &[]).ok()?);
    let mut sq = wq;
    if let Some(sr) = mc.v4("seatRot") {
        sq = qmul(sq, sr);
    }
    if let Some(oo) = mc.v3("originOffset") {
        let o = qrot(sq, oo);
        return Some(([p[0] + o[0], p[1] + o[1], p[2] + o[2]], sq));
    }
    Some((p, sq))
}
pub(super) fn bone_pos(i: i32) -> Option<[f32; 4]> {
    native::<[f32; 4]>("VRBoneModelPos", &[Value::Int32(i)])
}
pub(super) fn bone_rot(i: i32) -> Option<Q> {
    native::<[f32; 4]>("VRBoneModelRot", &[Value::Int32(i)]).map(f4)
}
pub(super) fn pad_model(side: usize) -> Option<V3> {
    let p = bone_pos(PAD_WRIST[side])?;
    let q = bone_rot(PAD_WRIST[side])?;
    if p[3] <= 0.5 {
        return None;
    }
    let d = qrot(q, PAD_OFF[side]);
    let p = f3(p);
    Some([p[0] + d[0], p[1] + d[1], p[2] + d[2]])
}
pub(super) fn raw_hand(side: usize) -> Option<[f32; 4]> {
    native::<[f32; 4]>("VRHandRawModel", &[Value::Int32(side as i32)])
}
pub(super) fn hand_model(side: usize) -> Option<V3> {
    raw_hand(side).map(f3)
}
pub(super) fn len3(v: V3) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}
pub(super) fn qslerp(a: Q, b: Q, t: f64) -> Q {
    let mut d = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    let s = if d < 0.0 { -1.0 } else { 1.0 };
    d *= s;
    let (w1, w2) = if d > 0.9995 {
        (1.0 - t, t * s)
    } else {
        let th = d.acos();
        let sn = th.sin();
        (((1.0 - t) * th).sin() / sn, s * (t * th).sin() / sn)
    };
    let q = [0, 1, 2, 3].map(|i| a[i] * w1 + b[i] * w2);
    let l = q.iter().map(|c| c * c).sum::<f64>().sqrt();
    if l < 1e-9 {
        a
    } else {
        q.map(|c| c / l)
    }
}
#[repr(C, align(16))]
struct TraceResult([f32; 24]);
/// rayHit: the first hit along the segment by the query presets (the last one that hit tried first): position, normal.
pub(super) fn ray_hit(s: &mut State, a: V3, b: V3) -> Option<(V3, V3)> {
    let sq = super::game_system("GetSpatialQueriesSystem");
    if sq.is_null() {
        return None;
    }
    let (a4, b4) = (super::belt::v4(a, 1.0), super::belt::v4(b, 1.0));
    for n in 0..RAY_PRESETS.len() {
        let i = (s.ray_pick + RAY_PRESETS.len() - 1 + n) % RAY_PRESETS.len();
        let mut tr = TraceResult([0.0; 24]);
        let hit = m::<bool>(
            &sq,
            "SyncRaycastByQueryPreset",
            &[Value::Ptr(a4.as_ptr().cast()), Value::Ptr(b4.as_ptr().cast()), Value::U64(CName::hashed(RAY_PRESETS[i]).0), Value::Out((&mut tr as *mut TraceResult).cast()), Value::Bool(true)],
        );
        if hit == Ok(true) {
            s.ray_pick = i + 1;
            let t = &tr.0;
            if s.ray_log < 4 {
                s.ray_log += 1;
                log!("[MagDrop] hit via {} at z={:.3} n=({:.2},{:.2},{:.2}) (from Rust)\n", RAY_PRESETS[i], t[2], t[3], t[4], t[5]);
            }
            return Some(([f64::from(t[0]), f64::from(t[1]), f64::from(t[2])], [f64::from(t[3]), f64::from(t[4]), f64::from(t[5])]));
        }
    }
    None
}
pub(super) fn play_snd(weapon: &Handle, name: Option<&str>) {
    let (Some(name), false) = (name, weapon.is_null()) else { return };
    let Some(id) = super::belt::entity_hash(weapon) else { return };
    let audio = super::game_system("GetAudioSystem");
    let _ = m::<()>(&audio, "Play", &[Value::U64(CName::hashed(name).0), Value::U64(id), Value::U64(0)]);
}
pub(super) fn play_fx(weapon: &Handle, name: Option<&str>) {
    let (Some(name), false) = (name, weapon.is_null()) else { return };
    let _ = st::<()>("GameObjectEffectHelper", "StartEffectEvent", &[Value::Handle(weapon), Value::U64(CName::hashed(name).0)]);
}
/// caseLand: the case's floor sound 0.45 s after it leaves (small / large / medium by the weapon's item type).
pub(super) fn case_land(s: &mut State, weapon: &Handle, sc_case_snd: &mut Option<String>) {
    if weapon.is_null() {
        return;
    }
    if sc_case_snd.is_none() {
        let data = super::belt::h(weapon, "GetItemData", &[]);
        let t = m::<i32>(&data, "GetItemType", &[]).ok().and_then(|v| crate::script_call::enum_name("gamedataItemType", i64::from(v))).unwrap_or_default();
        let n = if t.contains("Handgun") || t.contains("Revolver") || t.contains("SubmachineGun") {
            "w_casings_small_hit_floor"
        } else if t.contains("MachineGun") || t.contains("SniperRifle") {
            "w_casings_large_hit_floor"
        } else {
            "w_casings_medium_hit_floor"
        };
        *sc_case_snd = Some(n.to_string());
    }
    if let Some(n) = sc_case_snd {
        s.case_q.push((super::belt::now() + 0.45, weapon.clone(), n.clone()));
    }
}
pub(super) fn case_tick(s: &mut State) {
    let t = super::belt::now();
    let due: Vec<(Handle, String)> = s.case_q.iter().filter(|q| t >= q.0).map(|q| (q.1.clone(), q.2.clone())).collect();
    s.case_q.retain(|q| t < q.0);
    for (w, n) in due {
        play_snd(&w, Some(&n));
    }
}
fn handed(weapon: &Handle, name: &str) -> String {
    let left = m::<bool>(weapon, "IsEquippedInLeftHand", &[]).unwrap_or(false);
    let h = if left { "Left" } else { "Right" };
    match name {
        "Reload" => format!("{h}Reload"),
        "InterruptReload" => format!("Interrupt{h}Reload"),
        "ReloadData" => format!("{h}ReloadData"),
        _ => name.to_string(),
    }
}
pub(super) fn push_anim_event(weapon: &Handle, name: &str) {
    if weapon.is_null() {
        return;
    }
    let _ = st::<bool>("AnimationControllerComponent", "PushEvent", &[Value::Handle(weapon), Value::U64(CName::hashed(&handed(weapon, name)).0)]);
}
/// holdReloadStage: the weapon's reload stage animation held (the ReloadData feature and the Reload event).
pub(super) fn hold_reload_stage(weapon: &Handle) {
    if weapon.is_null() {
        return;
    }
    if let Some(class) = crate::script_call::class_any("AnimFeature_WeaponReload") {
        // SAFETY: a new instance owned by its handle; its fields written in place.
        let f = unsafe { Handle::from_instance(class.create_instance()) };
        unsafe {
            if let Some(p) = crate::script_call::property(f.instance, "emptyReload") {
                *(p as *mut bool) = false;
            }
            if let Some(p) = crate::script_call::property(f.instance, "amountToReload") {
                *(p as *mut i32) = 1;
            }
            if let Some(p) = crate::script_call::property(f.instance, "continueLoop") {
                *(p as *mut bool) = false;
            }
        }
        let _ = st::<()>("AnimationControllerComponent", "ApplyFeature", &[Value::Handle(weapon), Value::U64(CName::hashed(&handed(weapon, "ReloadData")).0), Value::Handle(&f)]);
    }
    push_anim_event(weapon, "Reload");
}
pub(super) fn mag_ammo(weapon: &Handle) -> Option<i32> {
    // the game's Uint32, read as its own type
    m::<u32>(weapon, "GetMagazineAmmoCount", &[]).ok().map(|n| n.min(i32::MAX as u32) as i32)
}
fn queue_event(weapon: &Handle, class: &str, fields: &[(&str, Value)]) -> bool {
    let Some(c) = crate::script_call::class_any(class) else { return false };
    // SAFETY: a new event instance owned by its handle; its fields written in place by their types.
    let e = unsafe { Handle::from_instance(c.create_instance()) };
    for (name, v) in fields {
        // SAFETY: as above.
        if let Some(p) = unsafe { crate::script_call::property(e.instance, name) } {
            unsafe {
                match v {
                    Value::Int32(i) => *(p as *mut i32) = *i,
                    Value::U64(u) => *(p as *mut u64) = *u,
                    Value::Ptr(src) => std::ptr::copy_nonoverlapping(*src as *const u8, p as *mut u8, 16),
                    _ => {}
                }
            }
        }
    }
    m::<()>(weapon, "QueueEvent", &[Value::Handle(&e)]).is_ok()
}
pub(super) fn consume_ammo(weapon: &Handle, n: i32) -> bool {
    queue_event(weapon, "WeaponConsumeMagazineAmmoEvent", &[("amount", Value::Int32(n))])
}
fn ammo_type(weapon: &Handle) -> Option<[u64; 2]> {
    st::<[u64; 2]>("WeaponObject", "GetAmmoType", &[Value::Handle(weapon)])
}
/// setMagAmmo: the magazine's count set (spent down by consume events, raised by SetAmmoCountEvent).
pub(super) fn set_mag_ammo(s: &mut State, weapon: &Handle, n: i32) -> bool {
    if weapon.is_null() {
        s.ammo_err = "no weapon".into();
        return false;
    }
    let Some(cur) = mag_ammo(weapon) else {
        s.ammo_err = "no count".into();
        return false;
    };
    let n = n.max(0);
    if n == cur {
        s.ammo_err = format!("at{n}");
        return true;
    }
    if n < cur {
        let ok = consume_ammo(weapon, cur - n);
        s.ammo_err = if ok { format!("consume{}({cur}->{n})", cur - n) } else { "consume FAILED".into() };
        return ok;
    }
    let Some(at) = ammo_type(weapon) else {
        s.ammo_err = "no ammo type".into();
        return false;
    };
    let ok = queue_event(weapon, "SetAmmoCountEvent", &[("ammoTypeID", Value::Ptr(at.as_ptr().cast())), ("count", Value::Int32(n))]);
    s.ammo_err = if ok { format!("set{n}({cur}->{n})") } else { "FAIL SetAmmoCountEvent".into() };
    ok
}
pub(super) fn w0_is_empty(weapon: &Handle) -> bool {
    if weapon.is_null() {
        return false;
    }
    m::<bool>(weapon, "IsMagazineEmpty", &[]).unwrap_or_else(|_| mag_ammo(weapon).unwrap_or(1) <= 0)
}
pub(super) fn mag_capacity(weapon: &Handle) -> i32 {
    st::<u32>("WeaponObject", "GetMagazineCapacity", &[Value::Handle(weapon)]).map_or(0, |n| n.min(i32::MAX as u32) as i32)
}
pub(super) fn reserve_ammo(weapon: &Handle) -> i32 {
    let Some(id) = ammo_type(weapon) else { return 0 };
    let Some(q) = st::<[u64; 2]>("ItemID", "CreateQuery", &[Value::U64(id[0])]) else { return 0 };
    m::<i32>(&transactions(), "GetItemQuantity", &[Value::Handle(&player()), Value::Ptr(q.as_ptr().cast())]).unwrap_or(0)
}
pub(super) fn give_reserve(weapon: &Handle, n: i32) {
    if weapon.is_null() || n <= 0 {
        return;
    }
    let Some(id) = ammo_type(weapon) else { return };
    let _ = m::<bool>(&transactions(), "GiveItem", &[Value::Handle(&player()), Value::Ptr(id.as_ptr().cast()), Value::Int32(n)]);
}
/// cfgFor: the weapon's config, by a component name carrying its match (the __base part first), cached per entity.
pub(super) fn cfg_for(s: &mut State, weapon: &Handle) -> Option<&'static J> {
    let key = super::belt::entity_hash(weapon).unwrap_or(0);
    if let Some(c) = s.match_cache.get(&key) {
        if c.is_some() {
            return *c;
        }
    }
    let names: Vec<String> = super::belt::components(weapon).iter().map(|c| super::belt::comp_name(c).to_lowercase()).collect();
    let configs = data::configs();
    for pass in 1..=2 {
        for nm in &names {
            if pass == 2 || (nm.contains("__base") && !nm.contains("decal")) {
                if let Some(cfg) = configs.iter().find(|c| c.s("match").is_some_and(|mt| nm.contains(mt))) {
                    s.match_cache.insert(key, Some(cfg));
                    return Some(cfg);
                }
            }
        }
    }
    None
}
pub(super) fn set_trg(s: &mut State, v: i32) {
    if s.trg_mode == Some(v) {
        return;
    }
    native::<()>("SetVRTriggerMode", &[Value::Int32(v)]);
    s.trg_mode = Some(v);
}
/// The belt's pouch: the palm's distance to this weapon's magazine there (None: no pouch for it).
pub(super) fn belt_mag_dist(side: usize) -> Option<f64> {
    if super::belt::mag_key() < 0 {
        return None;
    }
    let d = super::belt::mag_dist(side as i32);
    Some(if d < 0.0 { 99.0 } else { f64::from(d) })
}
pub(super) fn belt_mag_radius() -> f64 {
    f64::from(super::belt::reach_radius_published())
}
pub(super) fn belt_mag_taken(on: bool) {
    super::belt::set_mag_taken(on);
}
/// bodyMagReach: the plugin's body-magazine zone for that hand (shared [0..19], under its sequence [127]).
pub(super) fn body_mag_reach(side: usize) -> bool {
    let sh = |i: i32| f64::from(super::belt::shared(i));
    let seq = sh(127);
    if seq % 2.0 != 0.0 {
        return false;
    }
    let b = if side == 0 { 0 } else { 8 };
    if sh(b) < 0.5 {
        return false;
    }
    let (x, y, z) = (sh(b + 1), sh(b + 2), sh(b + 3));
    let (i, j, k, r) = (sh(16), sh(17), sh(18), sh(19));
    if ![x, y, z, i, j, k, r].iter().all(|v| v.is_finite()) || sh(127) != seq {
        return false;
    }
    let n = (i * i + j * j + k * k + r * r).sqrt();
    if !(0.5..=1.5).contains(&n) {
        return false;
    }
    let p = qrot([i / n, j / n, k / n, r / n], [x, y, z]);
    p[1] <= -0.18 && p[1] >= -1.10 && p[0] * p[0] + p[2] * p[2] <= 0.42 * 0.42
}
/// leadWorld: a held thing led by the player's own step (`tune.lead` frames of it), the step smoothed.
pub(super) fn lead_world(s: &mut State, key: &'static str, p: V3) -> (V3, f64) {
    let k = s.tune.lead;
    let Some(l) = s.lead.get_mut(key) else {
        s.lead.insert(key, Lead { p, d: [0.0; 3] });
        return (p, 0.0);
    };
    let r = [p[0] - l.p[0], p[1] - l.p[1], p[2] - l.p[2]];
    if r[0] * r[0] + r[1] * r[1] + r[2] * r[2] > 4.0 {
        l.d = [0.0; 3];
    } else {
        for i in 0..3 {
            l.d[i] += (r[i] - l.d[i]) * 0.65;
        }
    }
    l.p = p;
    let step = len3(l.d);
    if k <= 0.0 {
        return (p, step);
    }
    ([p[0] + l.d[0] * k, p[1] + l.d[1] * k, p[2] + l.d[2] * k], step)
}
pub(super) fn lead_reset(s: &mut State, key: &str) {
    s.lead.remove(key);
}
/// freshDelta: the engine's player position this frame against the model base (metres; -1/-2 when unknown/far).
pub(super) fn fresh_delta(b: V3) -> (V3, f64) {
    let Some(p) = native::<[f32; 4]>("VRPlayerEnginePos", &[]).filter(|p| p[3] >= 0.5) else { return ([0.0; 3], -1.0) };
    let d = [f64::from(p[0]) - b[0], f64::from(p[1]) - b[1], f64::from(p[2]) - b[2]];
    let d2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
    if d2 > 1.0 {
        return ([0.0; 3], -2.0);
    }
    (d, d2.sqrt())
}

// ---------------------------------------------------------------- the magazine carriers
/// holdComp: the player's carrier component for that hand (vrp_hold_left / right), on the weapon plane at LOD 0.
pub(super) fn hold_comp(s: &mut State, side: usize) -> Handle {
    if !s.hold.comp[side].is_null() {
        return s.hold.comp[side].clone();
    }
    let want = if side == 0 { "vrp_hold_left" } else { "vrp_hold_right" };
    let Some(c) = super::belt::components(&player()).into_iter().find(|c| super::belt::comp_name(c).contains(want)) else { return Handle::null() };
    if let Some(v) = crate::script_call::enum_value("ERenderingPlane", "RPl_Weapon") {
        // SAFETY: the component's enum property (one byte suffices for the plane's values).
        if let Some(p) = unsafe { crate::script_call::property(c.instance, "renderingPlane") } {
            unsafe { *(p as *mut u8) = v as u8 };
        }
    }
    // SAFETY: the component's LOD byte.
    if let Some(p) = unsafe { crate::script_call::property(c.instance, "forceLODLevel") } {
        unsafe { *(p as *mut i8) = 0 };
    }
    s.hold.comp[side] = c.clone();
    c
}
pub(super) fn item_hold(s: &mut State, side: usize, item: &str, slot: &str) -> bool {
    let pl = player();
    if pl.is_null() {
        return false;
    }
    if s.item.name[side].as_deref() == Some(item) {
        return true;
    }
    if !super::hold::hold_item(&pl, item, slot, true) {
        return false;
    }
    s.item.name[side] = Some(item.to_string());
    s.item.comps[side] = None;
    true
}
pub(super) fn item_parts(s: &mut State, side: usize, slot: &str) -> Option<Vec<Handle>> {
    if let Some(c) = &s.item.comps[side] {
        return Some(c.clone());
    }
    let e = super::belt::in_slot(&player(), slot);
    if e.is_null() {
        return None;
    }
    let list: Vec<Handle> = super::belt::components(&e)
        .into_iter()
        .filter(|c| super::belt::class_name(c).contains("Mesh") && has(c, "SetLocalTransform"))
        .filter(|c| {
            // SAFETY: the component's Bool property.
            unsafe { crate::script_call::property(c.instance, "isEnabled") }.is_none_or(|p| unsafe { *(p as *const bool) })
        })
        .collect();
    if list.is_empty() {
        return None;
    }
    s.item.comps[side] = Some(list.clone());
    Some(list)
}
pub(super) fn set_local(c: &Handle, p: V3, q: Q) -> bool {
    let (pv, qv) = (super::belt::v4(p, 1.0), super::belt::q4(q));
    m::<()>(c, "SetLocalTransform", &[Value::Ptr(pv.as_ptr().cast()), Value::Ptr(qv.as_ptr().cast())]).is_ok()
}
pub(super) fn item_place(s: &mut State, side: usize, slot: &str, p: V3, q: Q) -> bool {
    let Some(parts) = item_parts(s, side, slot) else { return false };
    for c in &parts {
        set_local(c, p, q);
    }
    true
}
pub(super) fn item_drop(s: &mut State, side: usize, slot: &str) {
    if s.item.name[side].is_none() {
        return;
    }
    let pl = player();
    if !pl.is_null() {
        super::hold::drop_item(&pl, slot);
    }
    (s.item.name[side], s.item.comps[side]) = (None, None);
}
pub(super) fn hold_show(s: &mut State, side: usize, mesh: &str, appearance: Option<&str>) -> bool {
    let c = hold_comp(s, side);
    if c.is_null() {
        return false;
    }
    if s.hold.mesh[side].as_deref() != Some(mesh) {
        let p = super::belt::engine_str(mesh);
        if native::<bool>("VRComponentChangeResource", &[Value::Handle(&c), Value::Ptr(p.as_ptr()), Value::Bool(true)]).is_none() {
            return false;
        }
        s.hold.mesh[side] = Some(mesh.to_string());
        s.hold.skin[side] = None;
    }
    let app = appearance.unwrap_or("default");
    if s.hold.skin[side].as_deref() != Some(app) {
        super::belt::change_appearance(&c, app, true);
        s.hold.skin[side] = Some(app.to_string());
    }
    if !s.hold.on[side] {
        if !super::belt::toggle(&c, true) {
            return false;
        }
        s.hold.on[side] = true;
    }
    true
}
pub(super) fn hold_place(s: &mut State, side: usize, p: V3, q: Q) -> bool {
    let c = hold_comp(s, side);
    !c.is_null() && set_local(&c, p, q)
}
pub(super) fn hold_hide(s: &mut State, side: Option<usize>) {
    for k in 0..2usize {
        if side.is_none_or(|sd| sd == k) && s.hold.on[k] {
            let c = hold_comp(s, k);
            if !c.is_null() {
                super::belt::toggle(&c, false);
            }
            s.hold.on[k] = false;
        }
    }
}
pub(super) fn despawn(id: u64) -> bool {
    let Ok(e) = super::find_entity(id) else { return false };
    if e.is_null() {
        return false;
    }
    super::ent_spawn::despawn(&e);
    true
}
pub(super) fn mag_litter_sweep(s: &mut State) {
    s.litter.retain(|id| !despawn(*id));
}
pub(super) fn stop_model(h: usize, on: bool, p: V3) {
    let v = super::belt::v4(p, if on { 1.0 } else { 0.0 });
    native::<bool>("VRHandStopModel", &[Value::Int32(h as i32), Value::Bool(on), Value::Ptr(v.as_ptr().cast())]);
}
pub(super) fn stop_rot(h: usize, on: i32, q: Q) {
    native::<bool>(
        "VRHandStopRot",
        &[Value::Int32(h as i32), Value::Int32(on), Value::Float(q[0] as f32), Value::Float(q[1] as f32), Value::Float(q[2] as f32), Value::Float(q[3] as f32)],
    );
}
pub(super) fn rig_write_clear() {
    native::<()>("VRRigWriteClear", &[]);
}
pub(super) fn track_write(which: i32, track: i32, v: f64, mode: i32) {
    native::<()>("VRRigTrackWrite", &[Value::Int32(which), Value::Int32(track), Value::Float(v as f32), Value::Int32(mode)]);
}
/// M.slotBind: the left hand slot's bind (the rig's) when the measured one is off it by more than 25 mm / ~8 degrees.
pub(super) fn slot_bind(side: usize, r: V3, e: Q) -> Option<[f64; 7]> {
    if side != 0 {
        return None;
    }
    let b = SLOT_BIND;
    let d = [r[0] - b[0], r[1] - b[1], r[2] - b[2]];
    let dot = (e[0] * b[3] + e[1] * b[4] + e[2] * b[5] + e[3] * b[6]).abs();
    if d[0] * d[0] + d[1] * d[1] + d[2] * d[2] < 0.025 * 0.025 && dot > 0.99 {
        return None;
    }
    Some(b)
}
/// M.thMask: the two-hand grip suppressed for those hands.
pub(super) fn th_mask(s: &mut State, mask: i32) {
    if s.th_mask_on == Some(mask) {
        return;
    }
    s.th_mask_on = Some(mask);
    native::<()>("SetVRTwoHandSuppress", &[Value::Int32(mask)]);
}
pub(super) fn wheel_owns(h: usize) -> bool {
    let mk = f64::from(super::belt::shared(163)).floor() as i64;
    if mk <= 0 {
        return false;
    }
    let bit = if h == 0 { 2 } else { 1 };
    mk % (bit * 2) >= bit
}
pub(super) fn grip_of(h: usize) -> bool {
    !wheel_owns(h) && super::belt::shared(if h == 0 { 155 } else { 49 }) > 0.5
}
pub(super) fn trig_of(h: usize) -> bool {
    super::belt::shared(if h == 0 { 154 } else { 160 }) > 0.5
}
/// M.squeeze: the hand's grip (or trigger) as the config asks.
pub(super) fn squeeze(how: Option<&str>, h: usize) -> bool {
    if how == Some("trigger") {
        trig_of(h)
    } else {
        grip_of(h)
    }
}
/// M.reachMark: the Reach marker on a model-space point for that hand, from the palm.
pub(super) fn reach_mark(side: usize, p: V3, from: Option<V3>) {
    let Some((b, c)) = player_frame() else { return };
    let w = qrot(conj(c), p);
    let from = from.map(|h| {
        let f = qrot(conj(c), h);
        [b[0] + f[0], b[1] + f[1], b[2] + f[2]]
    });
    super::reach::mark(
        side + 1,
        super::reach::Mark { pos: [b[0] + w[0], b[1] + w[1], b[2] + w[2]], radius: Some(0.03), kind: super::reach::Kind::Near, ready: true, from, lift: Some(0.02) },
    );
}
pub(super) fn rig_bone(which: i32, bone: i32, i: i32) -> Option<f64> {
    native::<f32>("VRRigBone", &[Value::Int32(which), Value::Int32(bone), Value::Int32(i)]).map(f64::from).filter(|v| v.is_finite())
}
pub(super) fn rig_write(which: i32, bone: i32, d: V3) {
    native::<()>("VRRigWrite", &[Value::Int32(which), Value::Int32(bone), Value::Float(d[0] as f32), Value::Float(d[1] as f32), Value::Float(d[2] as f32)]);
}
pub(super) fn rig_write_rot(which: i32, bone: i32, deg: f64, ax: V3) {
    native::<()>(
        "VRRigWriteRot",
        &[Value::Int32(which), Value::Int32(bone), Value::Float(deg as f32), Value::Float(ax[0] as f32), Value::Float(ax[1] as f32), Value::Float(ax[2] as f32)],
    );
}
pub(super) fn rig_write_abs(which: i32, bone: i32, p: V3, q: Q, mode: i32) -> Option<i32> {
    native::<i32>(
        "VRRigWriteAbs",
        &[
            Value::Int32(which),
            Value::Int32(bone),
            Value::Float(p[0] as f32),
            Value::Float(p[1] as f32),
            Value::Float(p[2] as f32),
            Value::Float(q[0] as f32),
            Value::Float(q[1] as f32),
            Value::Float(q[2] as f32),
            Value::Float(q[3] as f32),
            Value::Int32(mode),
        ],
    )
}
pub(super) fn rig_write_off(which: i32, bone: i32) {
    native::<()>("VRRigWriteOff", &[Value::Int32(which), Value::Int32(bone)]);
}
pub(super) fn rig_write_scale(which: i32, bone: i32, v: f64) {
    native::<()>("VRRigWriteScale", &[Value::Int32(which), Value::Int32(bone), Value::Float(v as f32)]);
}
