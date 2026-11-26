//! mods/cet/CyberpunkVRPort_TacticalBelt (init.lua, startup_clothing.lua; the grenade in the hand in belt_grenade.rs,
//! its flight in belt_flight.rs): the belt the player always wears and what hangs on it.
//!
//! The belt is a garment item (Items.vrp_belt, left-handed Items.vrp_belt_lh, or the Ezio variants) put into the port's
//! own slot AttachmentSlots.VRPortWaist (EquipmentEx's outfit slot for the Ezio belts) and named its appearance
//! ("<item>&Male|Female&TPP|FPP"); the player's vrp_belt* pieces switched per belt.cfg (the bare strap), the worn one's
//! chunk mask; the ucbelt* elements on the player switched by their rules -- the weapon in the slots or the hand and
//! its own magazine (ucbeltmag_<weapon>), the pump's shells, the equipped grenade's holder -- placed by the per-body
//! offsets, mirrored on the left-handed belt, the render plane parameter kept; all of it off in a scene playing the
//! body or in a vehicle. Every frame: the palms' distance to the weapon's magazine pouch and the shells (the reload
//! asks: VRBeltMagKey/MagDist/ShellDist/ReachRadius, VRBeltMagTaken/ShellTaken), the element a palm reaches lit (its
//! "_lit" appearance, a haptic tick, the Reach marker on it); the props in the belt slots (VRPortHoldItem/Scene, their
//! placement, their rim/glow mark by the hand's distance, re-attached across the render plane); the grenade grab.
//!
//! belt.cfg is read as the Lua module read it (merged over the defaults the generator took from init.lua's CFG, the
//! code-owned keys kept, the versioned upgrades) and read again when the CET overlay saves it; the overlay and its
//! offset files stay the Lua host's.
use super::belt_grenade::{self as grenade, Grenade};
use crate::script_call::{call_handle, call_native, invoke, invoke_handle, method, Value};
use cvr_host::log;
use red4ext_abi::cname::CName;
use red4ext_abi::handle::Handle;
use serde_json::Value as J;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

pub(super) type V3 = [f64; 3];
pub(super) type Q = [f64; 4];
type R<T> = Result<T, String>;

const DEFAULTS: &str = include_str!("belt_cfg_defaults.json");
const CFG_VARIANT_V: i64 = 1;
const CFG_VARIANT_DEFAULT: i64 = 4;
const THROW_CFG_VERSION: i64 = 4;
const GRAB_PLANE_CFG_VERSION: i64 = 1;
const OFFSETS_CFG_VERSION: i64 = 1;
const HIGHLIGHT_RADIUS_CFG_VERSION: i64 = 2;
const ATTACH_WAIT: f64 = 30.0;
const SWAP_SETTLE: f64 = 1.5;
const PIECE_READS_PER_FRAME: usize = 3;
const MASK_READ_FRAMES: i32 = 8;
const HIPS_T: V3 = [0.0, -0.0151072554, 0.96074301];
const HIPS_Q: Q = [0.701699018, -0.087263152, 0.701704204, -0.0872637928];

pub(super) const WEAPON_MAG: [&str; 33] = [
    "copperhead", "guillotine", "sidewinder", "silverhand", "palica", "satara", "lexington", "masamune", "overture", "saratoga",
    "tamayura", "yukimura", "crusher", "kenshin", "liberty", "ashura", "pozhar", "pulsar", "quasar", "senkoh", "burya", "kappa",
    "kyubi", "metel", "omaha", "ticon", "umbra", "unity", "chao", "dian", "grit", "nova", "nue",
];
const MAG_CENTRES: [(&str, V3); 33] = [
    ("ashura", [-0.1033, 0.1244, 1.0943]), ("burya", [-0.1347, 0.1031, 1.0796]), ("chao", [-0.1197, 0.1337, 1.0735]),
    ("copperhead", [-0.1114, 0.1124, 1.0636]), ("crusher", [-0.0874, 0.1305, 1.0416]), ("dian", [-0.1292, 0.0963, 1.0067]),
    ("grit", [-0.1391, 0.1038, 1.0276]), ("guillotine", [-0.0905, 0.1327, 1.0136]), ("kappa", [-0.1045, 0.1293, 1.0730]),
    ("kenshin", [-0.1124, 0.1152, 1.0636]), ("kyubi", [-0.1350, 0.0913, 1.0842]), ("lexington", [-0.1238, 0.1025, 1.0616]),
    ("liberty", [-0.0985, 0.1141, 1.0616]), ("masamune", [-0.1195, 0.1152, 1.0296]), ("metel", [-0.1062, 0.1197, 1.0250]),
    ("nova", [-0.1045, 0.1327, 1.0796]), ("nue", [-0.1086, 0.1191, 1.0496]), ("omaha", [-0.0981, 0.1155, 1.0450]),
    ("overture", [-0.1104, 0.1396, 1.0756]), ("palica", [-0.0907, 0.1363, 1.0951]), ("pozhar", [-0.1272, 0.0997, 1.0753]),
    ("pulsar", [-0.1222, 0.1155, 1.1099]), ("quasar", [-0.1055, 0.1283, 1.0672]), ("saratoga", [-0.1200, 0.1244, 1.0568]),
    ("satara", [-0.1040, 0.1080, 1.0700]), ("senkoh", [-0.1075, 0.1272, 1.0556]), ("sidewinder", [-0.1313, 0.1022, 1.0312]),
    ("silverhand", [-0.1248, 0.1145, 1.0891]), ("tamayura", [-0.1118, 0.1152, 1.0716]), ("ticon", [-0.1175, 0.1032, 1.0516]),
    ("umbra", [-0.1371, 0.1090, 1.0516]), ("unity", [-0.1121, 0.1051, 1.0676]), ("yukimura", [-0.1125, 0.1273, 1.0536]),
];
const MAG_PREFIX: &str = "ucbeltmag_";
const MIRRORED: [&str; 3] = ["ucbeltmag_", "ucbeltammo_shell", "ucbeltgrenade"];
const OURS: [&str; 3] = ["ucbeltmag_", "ucbeltgrenade", "ucbeltammo_shell"];
const SHELL_ROW: [&str; 3] = ["ucbeltammo_shell1", "ucbeltammo_shell2", "ucbeltammo_shell3"];
const GRENADE_KINDS: [&str; 9] = ["frag", "flash", "incendiary", "biohazard", "recon", "cutting", "smoke", "emp", "ozob"];
pub(super) const GRENADE_PROPS: [(&str, &str); 9] = [
    ("frag", "vrp_gren_frag"), ("flash", "vrp_gren_flash"), ("incendiary", "vrp_gren_incendiary"), ("biohazard", "vrp_gren_biohazard"),
    ("recon", "vrp_gren_recon"), ("cutting", "vrp_gren_cutting"), ("smoke", "vrp_gren_smoke"), ("emp", "vrp_gren_emp"), ("ozob", "vrp_gren_ozob"),
];
const GRENADE_ITEM: &str = "Items.VRPortGrenade";
const BELT_PIECES: [&str; 20] = [
    "strap", "back", "mag", "mag_l", "mag_r", "mag_back", "pouch_small", "pouch_l", "pouch_r", "bag_front_l", "bag_front_r", "bag_back_l",
    "bag_back_r", "radio", "radio_front", "torch", "torch_front", "gren_front", "gren_mid", "gren_back",
];

// ---------------------------------------------------------------- math
pub(super) fn qrot(q: Q, v: V3) -> V3 {
    let (i, j, k, r) = (q[0], q[1], q[2], q[3]);
    let (x, y, z) = (v[0], v[1], v[2]);
    let (tx, ty, tz) = (2.0 * (j * z - k * y), 2.0 * (k * x - i * z), 2.0 * (i * y - j * x));
    [x + r * tx + (j * tz - k * ty), y + r * ty + (k * tx - i * tz), z + r * tz + (i * ty - j * tx)]
}
pub(super) fn qmul(a: Q, b: Q) -> Q {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}
pub(super) fn conj(q: Q) -> Q {
    [-q[0], -q[1], -q[2], q[3]]
}
pub(super) fn dist(a: V3, b: V3) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}
/// The panel's degrees to the engine's quaternion (roll x, pitch y, yaw z).
pub(super) fn euler_quat(rx: f64, ry: f64, rz: f64) -> Q {
    let (hr, hp, hy) = (rx.to_radians() * 0.5, ry.to_radians() * 0.5, rz.to_radians() * 0.5);
    let (cr, sr, cp, sp, cy, sy) = (hr.cos(), hr.sin(), hp.cos(), hp.sin(), hy.cos(), hy.sin());
    [sr * cp * cy - cr * sp * sy, cr * sp * cy + sr * cp * sy, cr * cp * sy - sr * sp * cy, cr * cp * cy + sr * sp * sy]
}
fn to_hips_frame(p: V3) -> V3 {
    qrot(conj(HIPS_Q), [p[0] - HIPS_T[0], p[1] - HIPS_T[1], p[2] - HIPS_T[2]])
}
pub(super) fn f3(v: [f32; 4]) -> V3 {
    [f64::from(v[0]), f64::from(v[1]), f64::from(v[2])]
}
pub(super) fn f4(v: [f32; 4]) -> Q {
    [f64::from(v[0]), f64::from(v[1]), f64::from(v[2]), f64::from(v[3])]
}
pub(super) fn v4(v: V3, w: f32) -> [f32; 4] {
    [v[0] as f32, v[1] as f32, v[2] as f32, w]
}
pub(super) fn q4(q: Q) -> [f32; 4] {
    [q[0] as f32, q[1] as f32, q[2] as f32, q[3] as f32]
}

// ---------------------------------------------------------------- calls
pub(super) fn m<T: crate::script_call::Ret>(o: &Handle, name: &str, values: &[Value]) -> R<T> {
    if o.is_null() {
        return Err(format!("{name}: no object"));
    }
    // SAFETY: the caller's live object, held.
    unsafe { method::<T>(o.instance, name, values) }.ok_or_else(|| format!("{name} failed"))
}
pub(super) fn h(o: &Handle, name: &str, values: &[Value]) -> Handle {
    if o.is_null() {
        return Handle::null();
    }
    // SAFETY: as above.
    unsafe { call_handle(o.instance, name, values) }.unwrap_or_else(Handle::null)
}
pub(super) fn has(o: &Handle, name: &str) -> bool {
    // SAFETY: the object's class chain.
    !o.is_null() && unsafe { red4ext_abi::rtti::object_class(o.instance) }.is_some_and(|c| c.function(CName::hashed(name)).is_some())
}
pub(super) fn native<T: crate::script_call::Ret>(name: &str, values: &[Value]) -> Option<T> {
    // SAFETY: one of the plugin's own natives, called as the script VM calls it.
    unsafe { call_native::<T>(name, values) }
}
pub(super) fn st<T: crate::script_call::Ret>(class: &str, name: &str, values: &[Value]) -> Option<T> {
    // SAFETY: a static function (no context).
    crate::script_call::static_function_for(class, name, values).and_then(|f| unsafe { invoke::<T>(core::ptr::null_mut(), f, values) })
}
pub(super) fn st_handle(class: &str, name: &str, values: &[Value]) -> Handle {
    // SAFETY: as above.
    crate::script_call::static_function_for(class, name, values).and_then(|f| unsafe { invoke_handle(core::ptr::null_mut(), f, values) }).unwrap_or_else(Handle::null)
}
pub(super) fn cn(s: &str) -> Value {
    Value::U64(CName::hashed(s).0)
}
pub(super) fn tdb(s: &str) -> Value {
    Value::U64(red4ext_abi::tweakdbid::from_name(s))
}
pub(super) fn shared(i: i32) -> f32 {
    // SAFETY: the shared block's read.
    unsafe { cvr_rig::vrik::provider::shared_slot(i) }
}
fn tdb_id(s: &str) -> u64 {
    red4ext_abi::tweakdbid::from_name(s)
}
/// logDebug's gate: routine chatter only with the launcher's DEBUG box ticked (shared [156], main's c026279ef).
fn debug_log() -> bool {
    shared(156) > 0.5
}
pub(super) fn player() -> Handle {
    let mut p = Handle::null();
    // SAFETY: GetPlayer(GameInstance) into a handle of ours.
    unsafe { crate::anim_lookup::get_player(&mut p) };
    p
}
pub(super) fn entity_hash(o: &Handle) -> Option<u64> {
    m::<u64>(o, "GetEntityID", &[]).ok()
}
pub(super) fn transactions() -> Handle {
    super::game_system("GetTransactionSystem")
}
fn equip_system() -> Handle {
    h(&super::game_system("GetScriptableSystemsContainer"), "Get", &[cn("EquipmentSystem")])
}
pub(super) fn in_slot(pl: &Handle, slot: &str) -> Handle {
    h(&transactions(), "GetItemInSlot", &[Value::Handle(pl), tdb(slot)])
}
pub(super) fn item_id(o: &Handle) -> Option<[u64; 2]> {
    m::<[u64; 2]>(o, "GetItemID", &[]).ok()
}
pub(super) fn is_valid(id: &[u64; 2]) -> bool {
    st::<bool>("ItemID", "IsValid", &[Value::Ptr(id.as_ptr().cast())]).unwrap_or(false)
}
pub(super) fn record_name(tdbid: u64) -> Option<String> {
    super::weapon::tdbid_name(tdbid)
}
pub(super) fn components(e: &Handle) -> Vec<Handle> {
    crate::engine_objects::entity_components(e)
}
pub(super) fn comp_name(c: &Handle) -> String {
    m::<u64>(c, "GetName", &[]).map(|n| CName(n).text().to_string()).unwrap_or_default()
}
pub(super) fn class_name(o: &Handle) -> String {
    // SAFETY: a live scriptable, held.
    unsafe { red4ext_abi::rtti::object_class(o.instance) }.map(|c| c.class_name().text().to_string()).unwrap_or_default()
}
fn enabled(c: &Handle) -> Option<bool> {
    m::<bool>(c, "IsEnabled", &[]).ok()
}
pub(super) fn toggle(c: &Handle, on: bool) -> bool {
    m::<()>(c, "Toggle", &[Value::Bool(on)]).is_ok()
}
fn cname_prop(o: &Handle, prop: &str) -> Option<u64> {
    // SAFETY: the object's CName property, read in place.
    unsafe { crate::script_call::property(o.instance, prop) }.map(|p| unsafe { *(p as *const u64) })
}
pub(super) fn set_cname_prop(o: &Handle, prop: &str, v: u64) {
    // SAFETY: the object's CName property, written in place.
    if let Some(p) = unsafe { crate::script_call::property(o.instance, prop) } {
        unsafe { *(p as *mut u64) = v };
    }
}
fn mesh_appearance(c: &Handle) -> Option<String> {
    cname_prop(c, "meshAppearance").map(|n| CName(n).text().to_string())
}
pub(super) fn change_appearance(c: &Handle, name: &str, wait: bool) -> bool {
    crate::engine_objects::component_change_appearance(c, CName::hashed(name).0, wait)
}
pub(super) fn now() -> f64 {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f64()
}
pub(super) fn left_handed() -> bool {
    native::<bool>("VRLeftHanded", &[]) == Some(true)
}

// ---------------------------------------------------------------- belt.cfg
/// The belt's config: init.lua's CFG with belt.cfg merged over it as loadCfg merges it.
pub(super) struct Cfg(J);
impl Cfg {
    pub(super) fn v(&self, k: &str) -> &J {
        self.0.get(k).unwrap_or(&J::Null)
    }
    pub(super) fn f(&self, k: &str, d: f64) -> f64 {
        self.v(k).as_f64().unwrap_or(d)
    }
    /// Lua truthiness: nil and false are false.
    pub(super) fn b(&self, k: &str) -> bool {
        !matches!(self.v(k), J::Null | J::Bool(false))
    }
    /// `CFG.k ~= false`: only an explicit false is false.
    pub(super) fn not_false(&self, k: &str) -> bool {
        !matches!(self.v(k), J::Bool(false))
    }
    pub(super) fn s(&self, k: &str) -> &str {
        self.v(k).as_str().unwrap_or("")
    }
    fn variant(&self) -> usize {
        let n = self.v("variants").as_array().map_or(0, Vec::len);
        let i = self.v("variant").as_i64().unwrap_or(1);
        if i < 1 || i as usize > n {
            1
        } else {
            i as usize
        }
    }
    fn variants(&self) -> Vec<String> {
        self.v("variants").as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default()
    }
}
fn cfg_path() -> std::path::PathBuf {
    cvr_host::paths::file(cvr_host::paths::Folder::Lua, "CyberpunkVRPort_TacticalBelt\\belt.cfg")
}
/// The code's own additions to CFG (the magazine pouches' rules and centres, the Overture's parts).
fn code_cfg() -> J {
    let mut c: J = serde_json::from_str(DEFAULTS).unwrap_or(J::Null);
    if let Some(rules) = c.get_mut("rules").and_then(J::as_object_mut) {
        for k in WEAPON_MAG {
            rules.insert(format!("ucbeltmag_{k}"), J::from(format!("weaponmag={k}")));
        }
        for i in 2..=7 {
            rules.insert(format!("ucbeltmag_overture_p{i}"), J::from("weaponmag=overture"));
        }
    }
    if let Some(centres) = c.get_mut("centres").and_then(J::as_object_mut) {
        for k in WEAPON_MAG {
            let p = MAG_CENTRES.iter().find(|e| e.0 == k).map(|e| e.1).unwrap_or([-0.1497, 0.0949, 1.0756]);
            centres.insert(format!("ucbeltmag_{k}"), J::from(p.to_vec()));
        }
    }
    c
}
fn same_type(a: &J, b: &J) -> bool {
    matches!((a, b), (J::Bool(_), J::Bool(_)) | (J::Number(_), J::Number(_)) | (J::String(_), J::String(_)))
        || (matches!(a, J::Array(_) | J::Object(_)) && matches!(b, J::Array(_) | J::Object(_)))
}
/// Bumped by every write of belt.cfg (the overlay's window saving its copy, a migration): the tick reads the file again
/// when it sees a new generation. In the Lua mod the window and the tick shared one CFG table; here the window edits a
/// copy, and this is how its save reaches the tick (it used to stat the file every frame for that).
static CFG_GENERATION: AtomicU64 = AtomicU64::new(0);
/// loadCfg and the upgrades: belt.cfg merged over the code's CFG.
fn load_cfg() -> Cfg {
    let mut cfg = code_cfg();
    let data: Option<J> = std::fs::read_to_string(cfg_path()).ok().and_then(|t| serde_json::from_str(&t).ok());
    if let (Some(J::Object(data)), Some(obj)) = (&data, cfg.as_object_mut()) {
        for (k, v) in data {
            if matches!(k.as_str(), "centres" | "variants" | "variant_names" | "slot" | "outfit_slot" | "props") {
                continue;
            }
            let Some(cur) = obj.get_mut(k) else { continue };
            if !same_type(cur, v) {
                continue;
            }
            if (k == "rules" || k == "offsets") && v.is_object() {
                if let (Some(dst), Some(src)) = (cur.as_object_mut(), v.as_object()) {
                    for (kk, vv) in src {
                        let mine = k == "rules" && (kk.starts_with("ucbeltgrenade") || kk.starts_with("ucbeltammo") || kk.starts_with("ucbeltmag_"));
                        if !mine {
                            dst.insert(kk.clone(), vv.clone());
                        }
                    }
                } else {
                    *cur = v.clone();
                }
            } else {
                *cur = v.clone();
            }
        }
        if data.get("belt_variant_v").and_then(J::as_i64) != Some(CFG_VARIANT_V) {
            obj.insert("variant".into(), J::from(CFG_VARIANT_DEFAULT));
        }
        if let (Some(J::Array(saved)), Some(J::Array(props))) = (data.get("props"), obj.get_mut("props")) {
            for p in props.iter_mut() {
                let slot = p.get("slot").cloned();
                let Some(sp) = saved.iter().find(|sp| sp.get("slot") == slot.as_ref() && slot.is_some()) else { continue };
                for f in ["on", "lit", "auto", "up", "back", "right", "rx", "ry", "rz"] {
                    if let (Some(v), Some(cur)) = (sp.get(f), p.get(f)) {
                        if same_type(v, cur) {
                            p[f] = v.clone();
                        }
                    }
                }
            }
        }
    }
    // The versioned upgrades (upgradeThrowCfg, upgradeGrabPlaneCfg, upgradeOffsetsCfg, upgradeHighlightRadiusCfg).
    let version = |c: &J, k: &str| c.get(k).and_then(J::as_i64);
    let keys = ["grenade_throw_v", "grenade_grab_plane_v", "offsets_v", "highlight_radius_v"];
    let before: Vec<Option<i64>> = keys.iter().map(|k| version(&cfg, k)).collect();
    if let Some(obj) = cfg.as_object_mut() {
        if obj.get("grenade_throw_v").and_then(J::as_i64) != Some(THROW_CFG_VERSION) {
            obj.insert("grenade_throw_v".into(), J::from(THROW_CFG_VERSION));
            obj.insert("grenade_throw_gain".into(), J::from(2.0));
            obj.insert("grenade_throw_consume".into(), J::from(true));
            obj.insert("grenade_throw_max".into(), J::from(45.0));
            obj.insert("grenade_throw_min".into(), J::from(1.5));
        }
        if obj.get("grenade_grab_plane_v").and_then(J::as_i64) != Some(GRAB_PLANE_CFG_VERSION) {
            obj.insert("grenade_grab_plane_v".into(), J::from(GRAB_PLANE_CFG_VERSION));
            obj.insert("grenade_grab_plane".into(), J::from(0));
        }
        if obj.get("offsets_v").and_then(J::as_i64) != Some(OFFSETS_CFG_VERSION) {
            obj.insert("offsets_v".into(), J::from(OFFSETS_CFG_VERSION));
            if let Some(off) = obj.get_mut("offsets").and_then(J::as_object_mut) {
                off.insert("ucbeltammo_shell1".into(), serde_json::json!({"x": 0.0180, "y": 0.0260, "z": 0.1020, "rx": 0.0, "ry": 0.0, "rz": 0.0}));
                off.insert("ucbeltmag_dian".into(), serde_json::json!({"x": 0.0240, "y": 0.0260, "z": 0.0580, "rx": 0.0, "ry": 0.0, "rz": 0.0}));
            }
        }
        // The reach radius: every belt.cfg written while the default was 0.14 carries 0.14 (v1), and every one written
        // before 2026-11-11 the crouched reach of 1.5 and the release factor (v2, main's 58a9afab9).
        let v = obj.get("highlight_radius_v").and_then(J::as_i64).unwrap_or(0);
        if v < HIGHLIGHT_RADIUS_CFG_VERSION {
            if v < 1 {
                obj.insert("highlight_radius_m".into(), J::from(0.17));
            }
            obj.insert("crouch_reach_scale".into(), J::from(1.2));
            obj.insert("highlight_release_m".into(), J::from(0.03));
            obj.insert("highlight_radius_v".into(), J::from(HIGHLIGHT_RADIUS_CFG_VERSION));
            log!("[belt] reach radius upgraded to v{HIGHLIGHT_RADIUS_CFG_VERSION} (from Rust)\n");
        }
    }
    // A migration is written back at once (main's 58a9afab9): belt.cfg was saved only when the window closed, so each
    // one ran -- and logged, and put its defaults over the sliders -- on every launch until the window was opened.
    if keys.iter().zip(&before).any(|(k, b)| version(&cfg, k) != *b) {
        write_cfg(&cfg);
    }
    Cfg(cfg)
}
/// OFFSET_GROUP: hosts that follow another's placement.
pub(super) fn offset_group(host: &str) -> Option<&'static str> {
    match host {
        "ucbeltammo_shell2" | "ucbeltammo_shell3" => Some("ucbeltammo_shell1"),
        _ if host.starts_with("ucbeltmag_overture_p") => Some("ucbeltmag_overture"),
        _ => None,
    }
}
fn group_lead(host: &str) -> String {
    offset_group(host).map(str::to_string).unwrap_or_else(|| host.to_string())
}
/// markFamily: a lead and every host that follows it.
fn mark_family(host: &str) -> Vec<String> {
    let lead = group_lead(host);
    let mut out = vec![lead.clone()];
    if lead == "ucbeltammo_shell1" {
        out.push("ucbeltammo_shell2".into());
        out.push("ucbeltammo_shell3".into());
    } else if lead == "ucbeltmag_overture" {
        out.extend((2..=7).map(|i| format!("ucbeltmag_overture_p{i}")));
    }
    out
}

// ---------------------------------------------------------------- the state
#[derive(Clone, Default)]
pub(super) struct BeltState {
    pub grenade: bool,
    pub weapon: bool,
    pub inhand: bool,
    pub tier: i32,
    pub gtype: Option<&'static str>,
    pub wclass: HashSet<&'static str>,
    pub wmag: Option<String>,
    pub gcount: Option<i32>,
}
#[derive(Default)]
struct PropState {
    obj: Handle,
    obj_at: Option<f64>,
    empty_slot: Option<String>,
    mesh: Handle,
    claim_at: Option<f64>,
    item: Option<String>,
    at: f64,
    placed: Option<String>,
    lit: Option<bool>,
    dist: Option<f64>,
    flip: bool,
    glow_comp: Handle,
    glow: Option<bool>,
    emis: Option<Option<i64>>,
    plane: Option<i32>,
    fresh: bool,
    want: Option<i32>,
}
/// startup_clothing.lua's job (EquipmentEx's outfit route only).
#[derive(Default)]
struct Startup {
    done: bool,
    planned: bool,
    belt_key: Option<u64>,
    due: Option<f64>,
    waits: u32,
    errors: u32,
    restored: u32,
    refresh_count: u32,
    refreshed: HashSet<u64>,
    plan: Option<Vec<([u64; 2], u64)>>,
    changed: bool,
}
pub(super) struct Belt {
    pub cfg: Cfg,
    /// the CFG_GENERATION this config was read at
    cfg_generation: u64,
    acc: f64,
    belt_hash: Option<u64>,
    pub comps: HashMap<String, Handle>,
    comps_for: Option<u64>,
    dumped: bool,
    last_equip_try: f64,
    attach_pending_at: Option<f64>,
    detached_at: Option<f64>,
    outfit_missing: bool,
    recheck: Option<f64>,
    settle_at: Option<f64>,
    dressed: HashMap<String, String>,
    lit_on: HashSet<String>,
    pub lit_hand: [Option<String>; 2],
    /// The grenade holder each hand is near (cosmetic only: the grip and the holsters go by lit_hand).
    grenade_hover: [Option<String>; 2],
    lit_from: HashMap<String, String>,
    ring_at: [Option<V3>; 2],
    pub grab: Grenade,
    hl_acc: f64,
    mag_host: Option<String>,
    mag_d: [Option<f64>; 2],
    mag_taken: bool,
    shell_d: [Option<f64>; 2],
    shell_near: [Option<String>; 2],
    shell_taken: Option<String>,
    offsets_at: f64,
    pub state: BeltState,
    belt_parts: Option<HashMap<String, Handle>>,
    belt_parts_for: Option<u64>,
    belt_parts_scan: f64,
    piece_on: HashMap<String, Option<bool>>,
    piece_map_gen: u64,
    piece_map_seen: u64,
    piece_read: usize,
    mask_comp: usize,
    mask_want: u64,
    mask_skip: i32,
    belt_mirrored: bool,
    app_said: Option<String>,
    app_asked: Option<String>,
    scene: bool,
    scene_flip: bool,
    hold_off: Option<f64>,
    to_dress: bool,
    crouch_at: Option<f64>,
    crouched: bool,
    body_tag: Option<&'static str>,
    slot_memo: HashMap<usize, (u64, String)>,
    psm: Handle,
    mag_by_ent: HashMap<String, Option<String>>,
    slots_comp: Handle,
    slots_at: Option<f64>,
    props: Vec<PropState>,
    plane_at: f64,
    plane_val: Option<i32>,
    mounted_at: f64,
    mounted: bool,
    startup: Startup,
    grenade_entity: Option<String>,
    allowed: HashSet<String>,
    frame: u64,
    hips_frame: u64,
    hips: Option<(V3, Q)>,
    palm_frame: [u64; 2],
    palm: [Option<V3>; 2],
    errors: HashSet<String>,
    reported: bool,
    late_at: Option<f64>,
    late_done: bool,
}
// SAFETY: the handles are made, used and dropped on the game's main thread, where the mechanics tick.
unsafe impl Send for Belt {}
static STATE: Mutex<Option<Belt>> = Mutex::new(None);
/// The reload's bridge: whether its magazine and which shell are in a hand (VRBeltMagTaken / VRBeltShellTaken).
static MAG_TAKEN: AtomicI32 = AtomicI32::new(-1);
static SHELL_TAKEN: AtomicI32 = AtomicI32::new(-2);
static ATTACHED: AtomicBool = AtomicBool::new(false);
/// What the reload reads (VRBeltMagKey / MagDist / ShellDist / ReachRadius), published every frame.
pub(super) struct Published {
    mag_key: i32,
    mag_d: [f32; 2],
    shell_d: [f32; 2],
    radius: f32,
}
static PUBLISHED: Mutex<Published> = Mutex::new(Published { mag_key: -1, mag_d: [-1.0; 2], shell_d: [-1.0; 2], radius: 0.17 });

impl Belt {
    fn new() -> Belt {
        let cfg = load_cfg();
        let n = cfg.v("props").as_array().map_or(0, Vec::len);
        Belt {
            cfg,
            cfg_generation: CFG_GENERATION.load(Ordering::Acquire),
            acc: 0.0,
            belt_hash: None,
            comps: HashMap::new(),
            comps_for: None,
            dumped: false,
            last_equip_try: 0.0,
            attach_pending_at: None,
            detached_at: None,
            outfit_missing: false,
            recheck: None,
            settle_at: None,
            dressed: HashMap::new(),
            lit_on: HashSet::new(),
            lit_hand: [None, None],
            grenade_hover: [None, None],
            lit_from: HashMap::new(),
            ring_at: [None, None],
            grab: Grenade::default(),
            hl_acc: 0.0,
            mag_host: None,
            mag_d: [None; 2],
            mag_taken: false,
            shell_d: [None; 2],
            shell_near: [None, None],
            shell_taken: None,
            offsets_at: 0.0,
            state: BeltState::default(),
            belt_parts: None,
            belt_parts_for: None,
            belt_parts_scan: -1.0,
            piece_on: HashMap::new(),
            piece_map_gen: 0,
            piece_map_seen: u64::MAX,
            piece_read: 0,
            mask_comp: 0,
            mask_want: 0,
            mask_skip: 0,
            belt_mirrored: false,
            app_said: None,
            app_asked: None,
            scene: false,
            scene_flip: false,
            hold_off: None,
            to_dress: false,
            crouch_at: None,
            crouched: false,
            body_tag: None,
            slot_memo: HashMap::new(),
            psm: Handle::null(),
            mag_by_ent: HashMap::new(),
            slots_comp: Handle::null(),
            slots_at: None,
            props: (0..n).map(|_| PropState::default()).collect(),
            plane_at: -1.0,
            plane_val: None,
            mounted_at: -1.0,
            mounted: false,
            startup: Startup::default(),
            grenade_entity: None,
            allowed: HashSet::new(),
            frame: 0,
            hips_frame: u64::MAX,
            hips: None,
            palm_frame: [u64::MAX; 2],
            palm: [None, None],
            errors: HashSet::new(),
            reported: false,
            late_at: None,
            late_done: false,
        }
    }
    fn body_tag(&mut self) -> &'static str {
        if self.body_tag.is_none() {
            let female = m::<u64>(&player(), "GetResolvedGenderName", &[]).is_ok_and(|n| CName(n).text().contains("Female"));
            self.body_tag = Some(if female { "wa" } else { "ma" });
        }
        self.body_tag.unwrap_or("ma")
    }
    fn offset_table(&mut self) -> &J {
        let female = self.body_tag() == "wa" && self.cfg.v("offsets_female").is_object();
        self.cfg.v(if female { "offsets_female" } else { "offsets" })
    }
    fn offset_for(&mut self, name: &str) -> Option<J> {
        let base = name.strip_suffix("_lit").unwrap_or(name).to_string();
        let lead = group_lead(&base);
        self.offset_table().get(&lead).cloned()
    }
    fn mirror_sign(&self, name: &str) -> f64 {
        if self.belt_mirrored && MIRRORED.iter().any(|p| name.starts_with(p)) {
            -1.0
        } else {
            1.0
        }
    }
    fn wanted_record(&self) -> Option<String> {
        let rec = self.cfg.variants().get(self.cfg.variant().saturating_sub(1)).cloned()?;
        if rec == "Items.vrp_belt" && left_handed() {
            return Some("Items.vrp_belt_lh".into());
        }
        Some(rec)
    }
    /// The slot the variant's record lists (the port's own), else EquipmentEx's outfit slot.
    fn slot(&mut self) -> (u64, String) {
        let i = self.cfg.variant();
        if let Some(s) = self.slot_memo.get(&i) {
            return s.clone();
        }
        let rec = self.cfg.variants().get(i - 1).cloned().unwrap_or_default();
        let slot = self.cfg.s("slot").to_string();
        let name = if lists_slot(&rec, &slot) { slot } else { self.cfg.s("outfit_slot").to_string() };
        let v = (red4ext_abi::tweakdbid::from_name(&name), name);
        self.slot_memo.insert(i, v.clone());
        v
    }
    fn slot_name(&mut self) -> String {
        self.slot().1
    }
    fn element_on(&self, host: &str) -> Option<J> {
        self.cfg
            .v("elements")
            .as_array()?
            .iter()
            .find(|el| el.get("host").and_then(J::as_str) == Some(host) && self.dressed.get(host).map(String::as_str) == el.get("name").and_then(J::as_str))
            .cloned()
    }
    fn rule_for(&self, host: &str) -> String {
        if let Some(el) = self.element_on(host) {
            return el.get("rule").and_then(J::as_str).unwrap_or("always").to_string();
        }
        self.cfg.v("rules").get(host).and_then(J::as_str).unwrap_or("always").to_string()
    }
}
fn lists_slot(rec: &str, slot: &str) -> bool {
    let want = red4ext_abi::tweakdbid::from_name(slot) & 0xFF_FFFF_FFFF;
    let db = red4ext_abi::tweakdb::get();
    let Some((ty, value)) = (!db.is_null()).then(|| crate::tweak_records::flat(db, red4ext_abi::tweakdbid::from_name(&format!("{rec}.placementSlots")))).flatten() else {
        return false;
    };
    // SAFETY: the flat's own array type and value.
    unsafe {
        let n = crate::tweak_records::rt_array_length(ty, value);
        (0..n).any(|i| (*(crate::tweak_records::rt_array_element(ty, value, i) as *const u64) & 0xFF_FFFF_FFFF) == want)
    }
}

// ---------------------------------------------------------------- the belt's components
fn belt_components(s: &mut Belt) -> bool {
    let pl = player();
    let (slot, _) = s.slot();
    let obj = h(&transactions(), "GetItemInSlot", &[Value::Handle(&pl), Value::U64(slot)]);
    if pl.is_null() {
        return false;
    }
    if obj.is_null() {
        (s.belt_hash, s.comps) = (None, HashMap::new());
        s.belt_mirrored = false;
        return false;
    }
    let bh = entity_hash(&obj);
    let who = entity_hash(&pl);
    let t = now();
    let stale = bh != s.belt_hash || who != s.comps_for || s.comps.is_empty() || t - s.recheck.unwrap_or(0.0) > 5.0;
    if !stale {
        return true;
    }
    // The garment's meshes come from the equipped record, also when a wardrobe manages it or forced appearance changes
    // are off (main's c026279ef).
    s.belt_mirrored = item_id(&obj).is_some_and(|id| id[0] & 0xFF_FFFF_FFFF == tdb_id("Items.vrp_belt_lh") & 0xFF_FFFF_FFFF);
    if who != s.comps_for {
        s.comps_for = who;
        s.dressed.clear();
        s.mag_by_ent.clear();
    }
    (s.belt_hash, s.recheck, s.settle_at) = (bh, Some(t), Some(t));
    let prefix = s.cfg.s("prefix").to_string();
    let mut found = HashMap::new();
    let mut parts = HashMap::new();
    for c in components(&pl) {
        let nm = comp_name(&c);
        if nm.contains(&prefix) {
            found.insert(nm.clone(), c.clone());
        }
        if nm.starts_with("vrp_belt") {
            parts.insert(nm, c);
        }
    }
    (s.belt_parts, s.belt_parts_for, s.belt_parts_scan) = (Some(parts), who, t);
    s.piece_map_gen += 1;
    s.comps = found;
    s.to_dress = true;
    if !s.comps.is_empty() && !s.dumped {
        s.dumped = true;
        let mut names: Vec<&String> = s.comps.keys().collect();
        names.sort();
        if debug_log() {
            log!("[belt] belt elements on the player: {} -- {} (from Rust)\n", names.len(), names.iter().map(|n| n.as_str()).collect::<Vec<_>>().join(", "));
        }
    }
    true
}
fn attach_direct(s: &mut Belt, pl: &Handle) -> bool {
    let ts = transactions();
    let Some(rec) = s.wanted_record() else { return false };
    let Some(rid) = st::<[u64; 2]>("ItemID", "FromTDBID", &[tdb(&rec)]) else { return false };
    let Ok(pid) = m::<[u64; 2]>(&ts, "CreatePreviewItemID", &[Value::Ptr(rid.as_ptr().cast())]) else { return false };
    let _ = m::<bool>(&ts, "GivePreviewItemByItemID", &[Value::Handle(pl), Value::Ptr(rid.as_ptr().cast())]);
    let (slot, _) = s.slot();
    m::<bool>(&ts, "AddItemToSlot", &[Value::Handle(pl), Value::U64(slot), Value::Ptr(pid.as_ptr().cast()), Value::Bool(true)]).unwrap_or(false)
}
fn dress_elements(s: &mut Belt) {
    if !s.to_dress || s.settle_at.is_none_or(|t| now() - t < 0.25) {
        return;
    }
    s.to_dress = false;
    let elements = s.cfg.v("elements").as_array().cloned().unwrap_or_default();
    for el in elements {
        let (Some(name), Some(hn)) = (el.get("name").and_then(J::as_str), el.get("host").and_then(J::as_str)) else { continue };
        let Some(host) = s.comps.get(hn).cloned() else {
            log!("[belt] element {name} wants host {hn}, which the belt does not have\n");
            continue;
        };
        let mesh = el.get("mesh").and_then(J::as_str).unwrap_or("");
        if mesh.is_empty() {
            continue;
        }
        let cur = mesh_appearance(&host);
        let app = el.get("appearance").and_then(J::as_str);
        let marked = cur.as_deref().is_some_and(|c| c.starts_with("lit"));
        if cur.is_some() && app.is_some() && (cur.as_deref() == app || marked) {
            s.dressed.insert(hn.to_string(), name.to_string());
            continue;
        }
        let path = mesh.replace("{g}", s.body_tag());
        let ok_mesh = native::<bool>("VRComponentChangeResource", &[Value::Handle(&host), Value::Ptr(engine_str(&path).as_ptr()), Value::Bool(false)]);
        let ok_app = app.filter(|a| !a.is_empty()).map(|a| change_appearance(&host, a, false));
        if ok_mesh == Some(true) {
            s.dressed.insert(hn.to_string(), name.to_string());
        }
        let o = [el.get("ox").and_then(J::as_f64).unwrap_or(0.0), el.get("oy").and_then(J::as_f64).unwrap_or(0.0), el.get("oz").and_then(J::as_f64).unwrap_or(0.0)];
        let p = v4(o, 1.0);
        let _ = m::<()>(&host, "SetLocalPosition", &[Value::Ptr(p.as_ptr().cast())]);
        log!("[belt] element {name} dressed on {hn}: mesh={ok_mesh:?} appearance={ok_app:?} (was {cur:?})\n");
    }
}
/// An engine String holding `text`, kept while the call reads it.
pub(super) struct EngineStr(red4ext_abi::cstring::CString, #[allow(dead_code)] std::ffi::CString);
impl EngineStr {
    pub(super) fn as_ptr(&self) -> *const core::ffi::c_void {
        (&self.0 as *const red4ext_abi::cstring::CString).cast()
    }
}
pub(super) fn engine_str(text: &str) -> EngineStr {
    let c = std::ffi::CString::new(text).unwrap_or_default();
    let e = red4ext_abi::cstring::CString::new(&c).unwrap_or_else(|| red4ext_abi::cstring::CString::new(c"").expect("empty string"));
    EngineStr(e, c)
}
fn force_belt_appearance(s: &mut Belt, pl: &Handle) {
    if !s.cfg.b("force_appearance") {
        return;
    }
    let (slot, _) = s.slot();
    let o = h(&transactions(), "GetItemInSlot", &[Value::Handle(pl), Value::U64(slot)]);
    if o.is_null() {
        return;
    }
    let rec = s.wanted_record().unwrap_or_default();
    let Some(stem) = rec.strip_prefix("Items.") else { return };
    let body = if s.body_tag() == "wa" { "Female" } else { "Male" };
    let now_app = m::<u64>(&o, "GetCurrentAppearanceName", &[]).map(|n| CName(n).text().to_string()).unwrap_or_default();
    let want = format!("{stem}_&{body}&TPP");
    if now_app == want {
        if s.app_said.as_deref() != Some(now_app.as_str()) {
            log!("[belt] belt appearance is {now_app} (from Rust)\n");
            s.app_said = Some(now_app);
        }
        return;
    }
    let mut w = want.clone();
    if s.app_asked.as_deref() == Some(w.as_str()) && !now_app.is_empty() && now_app != "default" {
        w = format!("{stem}_&{body}&FPP");
    }
    s.app_asked = Some(w.clone());
    let _ = m::<()>(&o, "ScheduleAppearanceChange", &[cn(&w)]);
    if debug_log() {
        log!("[belt] belt appearance: было {now_app}, прошу {w}\n");
    }
}
fn belt_all_in_one(s: &mut Belt) -> bool {
    let k = if s.body_tag() == "wa" { "belt_all_in_one_female" } else { "belt_all_in_one" };
    s.cfg.not_false(k)
}
fn piece_on(s: &mut Belt, piece: &str) -> bool {
    let female = s.body_tag() == "wa" && s.cfg.v("belt_pieces_female").is_object();
    s.cfg.v(if female { "belt_pieces_female" } else { "belt_pieces" }).get(piece) == Some(&J::Bool(true))
}
fn belt_chunk_mask(s: &mut Belt) -> u64 {
    let female = s.body_tag() == "wa" && s.cfg.v("belt_chunk_off_female").is_array();
    let off = s.cfg.v(if female { "belt_chunk_off_female" } else { "belt_chunk_off" }).as_array().cloned().unwrap_or_default();
    (0..32).filter(|i| off.get(*i) != Some(&J::Bool(true))).fold(0u64, |m, i| m | (1u64 << i))
}
fn belt_parts(s: &mut Belt, pl: &Handle, who: Option<u64>) {
    if s.belt_parts.is_some() && s.belt_parts_for == who && who.is_some() {
        let map = s.belt_parts.as_ref();
        let empty = map.is_some_and(HashMap::is_empty);
        if !empty || now() - s.belt_parts_scan < 1.0 {
            return;
        }
    }
    let mut map = HashMap::new();
    for c in components(pl) {
        let nm = comp_name(&c);
        if nm.starts_with("vrp_belt") {
            map.insert(nm, c);
        }
    }
    (s.belt_parts, s.belt_parts_for, s.belt_parts_scan) = (Some(map), who, now());
    s.piece_map_gen += 1;
}
/// The belt's own pieces switched per the body's table; the worn one returned for its chunk mask.
fn apply_belt_pieces(s: &mut Belt, pl: &Handle, who: Option<u64>) -> Option<(Handle, String)> {
    belt_parts(s, pl, who);
    if s.piece_map_seen != s.piece_map_gen {
        (s.piece_map_seen, s.piece_on) = (s.piece_map_gen, HashMap::new());
    }
    let mut names = vec!["vrp_belt".to_string()];
    names.extend(BELT_PIECES.iter().map(|k| format!("vrp_belt_{k}")));
    let count = names.len();
    let first = s.piece_read;
    s.piece_read = (first + PIECE_READS_PER_FRAME) % count;
    let mut worn = None;
    for (i, nm) in names.iter().enumerate() {
        let mut on = if i == 0 { belt_all_in_one(s) } else { piece_on(s, BELT_PIECES[i - 1]) };
        if s.scene {
            on = false;
        }
        let Some(c) = s.belt_parts.as_ref().and_then(|m| m.get(nm)).cloned() else { continue };
        let mut is = s.piece_on.get(nm).copied().flatten();
        let due = (i as i64 - first as i64).rem_euclid(count as i64) < PIECE_READS_PER_FRAME as i64;
        if is.is_none() || due {
            is = enabled(&c);
        }
        let mut now_on = is;
        if is != Some(on) && toggle(&c, on) && is.is_some() {
            now_on = Some(on);
        }
        s.piece_on.insert(nm.clone(), now_on);
        if now_on == Some(true) && worn.is_none() {
            worn = Some((c, nm.clone()));
        }
    }
    worn
}
fn apply_belt_chunks(s: &mut Belt, worn: Option<(Handle, String)>) {
    let Some((c, _)) = worn else { return };
    let want = belt_chunk_mask(s);
    let ptr = c.instance as usize;
    if ptr == s.mask_comp && want == s.mask_want && s.mask_skip > 0 {
        s.mask_skip -= 1;
        return;
    }
    (s.mask_comp, s.mask_want, s.mask_skip) = (ptr, want, MASK_READ_FRAMES - 1);
    // SAFETY: the mesh component's chunk mask (a uint64 bitfield), read and written in place.
    let Some(p) = (unsafe { crate::script_call::property(c.instance, "chunkMask") }) else { return };
    unsafe {
        if *(p as *const u64) == want {
            return;
        }
        *(p as *mut u64) = want;
    }
    toggle(&c, false);
    toggle(&c, true);
}
fn outfit_system(s: &mut Belt) -> Handle {
    let o = h(&super::game_system("GetScriptableSystemsContainer"), "Get", &[cn("EquipmentEx.OutfitSystem")]);
    if o.is_null() {
        if !s.outfit_missing {
            s.outfit_missing = true;
            log!("[belt] EquipmentEx is not installed -- the port's own belt goes into its own slot, the Ezio belts cannot be worn (from Rust)\n");
        }
    } else {
        s.outfit_missing = false;
    }
    o
}
fn drop_outfit_duplicate(s: &mut Belt, pl: &Handle, want: &str) {
    let outfit_slot = s.cfg.s("outfit_slot").to_string();
    if s.slot_name() == outfit_slot {
        return;
    }
    let o = in_slot(pl, &outfit_slot);
    let Some(id) = item_id(&o) else { return };
    if record_name(id[0]).as_deref() != Some(want) {
        return;
    }
    let outfit = outfit_system(s);
    let off = m::<bool>(&outfit, "UnequipSlot", &[tdb(&outfit_slot)]).unwrap_or(false);
    if !off {
        let _ = m::<bool>(&transactions(), "RemoveItemFromSlot", &[Value::Handle(pl), tdb(&outfit_slot), Value::Bool(true)]);
    }
    log!("[belt] {want} was also in {outfit_slot} (an EquipmentEx outfit from before the port's own slot) -- taken off there\n");
}
fn ensure_equipped(s: &mut Belt, now_t: f64) {
    if !s.cfg.b("auto_equip") || now_t - s.last_equip_try < 2.0 {
        return;
    }
    s.last_equip_try = now_t;
    let pl = player();
    if pl.is_null() {
        return;
    }
    let (slot, slot_name) = s.slot();
    let have = h(&transactions(), "GetItemInSlot", &[Value::Handle(&pl), Value::U64(slot)]);
    let want = s.wanted_record().unwrap_or_default();
    if !have.is_null() {
        s.attach_pending_at = None;
        let worn = item_id(&have).and_then(|id| record_name(id[0]));
        if worn.is_none() || worn.as_deref() == Some(want.as_str()) {
            force_belt_appearance(s, &pl);
            drop_outfit_duplicate(s, &pl, &want);
            return;
        }
        log!("[belt] waist holds {worn:?}, wanted {want} -- swapping\n");
        let mut off = false;
        if slot_name == s.cfg.s("outfit_slot") {
            let o2 = outfit_system(s);
            off = m::<bool>(&o2, "UnequipSlot", &[Value::U64(slot)]).unwrap_or(false);
            if !off {
                if let Some(id) = item_id(&have) {
                    off = m::<bool>(&o2, "UnequipItem", &[Value::Ptr(id.as_ptr().cast())]).unwrap_or(false);
                }
            }
        }
        if !off {
            let _ = m::<bool>(&transactions(), "RemoveItemFromSlot", &[Value::Handle(&pl), Value::U64(slot), Value::Bool(false)]);
        }
        s.detached_at = Some(now_t);
        return;
    }
    if let Some(at) = s.detached_at {
        if now_t - at < SWAP_SETTLE {
            return;
        }
        s.detached_at = None;
    }
    if s.attach_pending_at.is_some_and(|at| now_t - at < ATTACH_WAIT) {
        return;
    }
    if s.cfg.s("route") != "outfit" && attach_direct(s, &pl) {
        s.attach_pending_at = Some(now_t);
        if debug_log() {
            log!("[belt] queued direct attachment of {want} into {slot_name} (from Rust)\n");
        }
        return;
    }
    if slot_name != s.cfg.s("outfit_slot") {
        return;
    }
    let outfit = outfit_system(s);
    if outfit.is_null() {
        return;
    }
    let iid = inventory_item_id(&pl, &want).or_else(|| {
        // Game.AddToInventory(rec, 1): the host's global function.
        static CACHE: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
        if let Some(f) = crate::script_call::global_by_short_name("AddToInventory", &CACHE) {
            let r = tdb_str(&want);
            // SAFETY: a global function (no context) with its String and Int32.
            unsafe { invoke::<()>(core::ptr::null_mut(), f, &[r.value(), Value::Int32(1)]) };
        }
        inventory_item_id(&pl, &want)
    });
    let Some(iid) = iid else {
        log!("[belt] no {want} in the inventory and AddToInventory did not make one -- cannot equip\n");
        return;
    };
    let ok = m::<bool>(&outfit, "EquipItem", &[Value::Ptr(iid.as_ptr().cast()), Value::U64(slot)]);
    let eq = m::<bool>(&outfit, "IsEquipped", &[Value::Ptr(iid.as_ptr().cast())]);
    log!("[belt] equip {want} -> {ok:?}, EquipmentEx counts it as equipped: {eq:?}\n");
    force_belt_appearance(s, &pl);
}
fn inventory_item_id(pl: &Handle, rec: &str) -> Option<[u64; 2]> {
    let ts = transactions();
    // SAFETY: the system, held; GetItemList's out array kept while read.
    let f = unsafe { crate::script_call::find_function(ts.instance, "GetItemList", 2) }?;
    let ty: &'static red4ext_abi::rtti::IType = unsafe { &*(**f.params.as_slice().get(1)?).ty };
    let mut out = crate::script_call::Owned::of_type(ty)?;
    m::<bool>(&ts, "GetItemList", &[Value::Handle(pl), Value::Out(out.as_mut_ptr().cast())]).ok()?;
    handles(out.as_ptr()).iter().find_map(|d| {
        let id = m::<[u64; 2]>(d, "GetID", &[]).ok()?;
        (record_name(id[0]).as_deref() == Some(rec)).then_some(id)
    })
}

// ---------------------------------------------------------------- what the player has
/// The handles of an array<ref<T>> value, each held.
pub(super) fn handles(array: *const u8) -> Vec<Handle> {
    // SAFETY: a DynArray of handles (two pointers each), read in place and cloned.
    unsafe {
        let arr = &*(array as *const red4ext_abi::dynarray::DynArray<[usize; 2]>);
        arr.as_slice().iter().map(|e| (*(e as *const [usize; 2] as *const Handle)).clone()).collect()
    }
}
fn is_grenade(id: &[u64; 2]) -> bool {
    let Some(name) = record_name(id[0]) else { return true };
    let Some(t) = super::weapon::flat_u64(&format!("{name}.itemType")) else { return true };
    record_name(t).is_none_or(|n| n.contains("Gad_Grenade"))
}
fn weapon_class(type_name: &str) -> Option<&'static str> {
    Some(match type_name {
        "Wea_Handgun" | "Wea_Revolver" => "pistol",
        "Wea_Shotgun" | "Wea_ShotgunDual" => "shotgun",
        "Wea_AssaultRifle" | "Wea_Rifle" | "Wea_PrecisionRifle" | "Wea_SniperRifle" | "Wea_SubmachineGun" | "Wea_LightMachineGun" | "Wea_HeavyMachineGun" => "rifle",
        _ => return None,
    })
}
pub(super) fn mag_key_in(name: &str) -> Option<String> {
    let low = name.to_lowercase();
    if ["carnage", "tactician"].iter().any(|k| low.contains(k)) {
        return Some("pump".into());
    }
    if let Some(k) = WEAPON_MAG.iter().find(|k| low.contains(*k)) {
        return Some((*k).to_string());
    }
    [("igla", "satara"), ("testera", "satara")].iter().find(|(k, _)| low.contains(k)).map(|(_, to)| to.to_string())
}
fn mag_key_of_weapon(s: &mut Belt, obj: &Handle) -> Option<String> {
    let rec = item_id(obj).and_then(|id| record_name(id[0]));
    if let Some(k) = rec.as_ref().and_then(|r| s.mag_by_ent.get(r)) {
        return k.clone();
    }
    let cs = components(obj);
    let names: Vec<String> = cs.iter().map(comp_name).collect();
    let key = names.iter().filter(|n| n.contains("__mag_")).find_map(|n| mag_key_in(n)).or_else(|| names.iter().find_map(|n| mag_key_in(n)));
    if let (Some(k), Some(r)) = (&key, rec) {
        s.mag_by_ent.insert(r, Some(k.clone()));
    }
    key
}
fn weapon_classes(id: &[u64; 2], mk: Option<&str>) -> HashSet<&'static str> {
    let mut cls = HashSet::new();
    let rec = st_handle("TweakDBInterface", "GetItemRecord", &[Value::U64(id[0])]);
    if let Ok(t) = m::<i32>(&h(&rec, "ItemType", &[]), "Type", &[]) {
        if let Some(c) = crate::script_call::enum_name("gamedataItemType", i64::from(t)).as_deref().and_then(weapon_class) {
            cls.insert(c);
        }
    }
    if mk == Some("pump") {
        cls.insert("pump");
    }
    cls
}
fn read_state(s: &mut Belt) -> BeltState {
    let mut st_ = BeltState::default();
    let pl = player();
    if pl.is_null() {
        return st_;
    }
    let es = equip_system();
    if !es.is_null() {
        let area = |a: &str| Value::U64(crate::script_call::enum_value("gamedataEquipmentArea", a).unwrap_or(0) as u64);
        if let Ok(id) = m::<[u64; 2]>(&es, "GetItemInEquipSlot", &[Value::Handle(&pl), area("Gadget"), Value::Int32(0)]) {
            st_.grenade = is_valid(&id) && is_grenade(&id);
        }
        let pd = h(&es, "GetPlayerData", &[Value::Handle(&pl)]);
        if let Ok(id) = m::<[u64; 2]>(&pd, "GetActiveGadget", &[]) {
            if is_valid(&id) {
                if let Some(nm) = record_name(id[0]) {
                    let low = nm.to_lowercase();
                    st_.gtype = GRENADE_KINDS.iter().find(|k| low.contains(*k)).copied();
                }
            }
        }
        st_.gcount = m::<i32>(&pl, "GetGrenadeCharges", &[]).ok();
        if !st_.grenade {
            st_.grenade = m::<i32>(&transactions(), "GetItemQuantityByTag", &[Value::Handle(&pl), cn("Grenade")]).is_ok_and(|n| n > 0);
        }
        // where weapons may not be drawn the grenades are neither shown nor taken
        if super::holster::weapons_forbidden(&pl) {
            st_.grenade = false;
        }
        for slot in 0..3 {
            if st_.weapon {
                break;
            }
            if let Ok(id) = m::<[u64; 2]>(&es, "GetItemInEquipSlot", &[Value::Handle(&pl), area("Weapon"), Value::Int32(slot)]) {
                if is_valid(&id) {
                    st_.weapon = true;
                    st_.wmag = record_name(id[0]).and_then(|n| mag_key_in(&n));
                    st_.wclass = weapon_classes(&id, st_.wmag.as_deref());
                }
            }
        }
    }
    let mut w = in_slot(&pl, "AttachmentSlots.WeaponRight");
    if w.is_null() {
        w = in_slot(&pl, "AttachmentSlots.WeaponLeft");
    }
    st_.inhand = !w.is_null();
    if let Some(id) = item_id(&w) {
        let k = mag_key_of_weapon(s, &w).or_else(|| record_name(id[0]).and_then(|n| mag_key_in(&n)));
        st_.wclass = weapon_classes(&id, k.as_deref());
        st_.wmag = k;
    }
    if s.psm.is_null() {
        if let Ok(defs) = super::boards::defs() {
            if let Ok(d) = super::boards::definition(&defs, "PlayerStateMachine") {
                s.psm = super::boards::local_board(entity_hash(&pl).unwrap_or(0), d).unwrap_or_else(|_| Handle::null());
            }
        }
    }
    if !s.psm.is_null() {
        if let Ok(defs) = super::boards::defs() {
            if let Ok(d) = super::boards::definition(&defs, "PlayerStateMachine") {
                if let Ok(f) = super::boards::field(d, "SceneTier") {
                    st_.tier = super::boards::get::<i32>(&s.psm, "GetInt", f).unwrap_or(0);
                }
            }
        }
    }
    st_
}
fn wanted(rule: &str, st_: &BeltState, mag_taken: bool) -> bool {
    let none = st_.gcount.is_some_and(|n| n <= 0);
    if let Some(kind) = rule.strip_prefix("grenade=") {
        return !none && st_.grenade && st_.gtype == Some(kind);
    }
    if let Some(cls) = rule.strip_prefix("weapon=") {
        return st_.wclass.contains(cls);
    }
    if let Some(mk) = rule.strip_prefix("weaponmag=") {
        return !mag_taken && st_.wmag.as_deref() == Some(mk);
    }
    match rule {
        "always" => true,
        "never" => false,
        "grenade" => !none && st_.grenade,
        "weapon" => st_.weapon,
        "inhand" => st_.inhand,
        _ => true,
    }
}
pub(super) fn in_vehicle(s: &mut Belt) -> bool {
    let t = now();
    if t - s.mounted_at >= 0.25 || t < s.mounted_at {
        s.mounted_at = t;
        static CACHE: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
        let pl = player();
        s.mounted = !pl.is_null()
            && crate::script_call::global_by_short_name("GetMountedVehicle", &CACHE)
                // SAFETY: a global script function (no context).
                .and_then(|f| unsafe { invoke_handle(core::ptr::null_mut(), f, &[Value::Handle(&pl)]) })
                .is_some_and(|v| !v.is_null());
    }
    s.mounted
}
fn scene_hides(s: &mut Belt) -> bool {
    if s.cfg.b("hide_in_vehicles") && in_vehicle(s) {
        return true;
    }
    if !s.cfg.b("hide_in_scenes") {
        return false;
    }
    match native::<bool>("VRSceneOwnsBody", &[]) {
        Some(v) => v,
        None => f64::from(s.state.tier) >= s.cfg.f("cutscene_tier", 3.0),
    }
}
fn apply_elements(s: &mut Belt) {
    if !belt_components(s) {
        return;
    }
    let hide_all = s.scene;
    let names: Vec<String> = s.comps.keys().cloned().collect();
    let plane_param = s.cfg.s("render_plane_param").to_string();
    for name in names {
        let Some(comp) = s.comps.get(&name).cloned() else { continue };
        let twin = name.ends_with("_lit");
        let rule = s.rule_for(&name);
        let mut want = wanted(&rule, &s.state, s.mag_taken);
        if hide_all {
            want = false;
        }
        if let Some(gh) = &s.grab.host {
            if name == *gh || name == format!("{gh}_lit") {
                want = false;
            }
        }
        if let Some(sh) = &s.shell_taken {
            if name == *sh || name == format!("{sh}_lit") {
                want = false;
            }
        }
        let is = enabled(&comp);
        if (!twin || hide_all) && is.is_some_and(|v| v != want) {
            toggle(&comp, want);
        }
        if let Some(off) = s.offset_for(&name) {
            let sx = s.mirror_sign(&name);
            let g = |k: &str| off.get(k).and_then(J::as_f64).unwrap_or(0.0);
            let w = [sx * g("x"), g("y"), g("z")];
            let cur = m::<[f32; 4]>(&comp, "GetLocalPosition", &[]).ok().map(f3);
            if cur.is_none_or(|c| (0..3).any(|i| (c[i] - w[i]).abs() > 0.0002)) {
                let p = v4(w, 1.0);
                let _ = m::<()>(&comp, "SetLocalPosition", &[Value::Ptr(p.as_ptr().cast())]);
            }
            let q = euler_quat(g("rx"), sx * g("ry"), sx * g("rz"));
            let cq = m::<[f32; 4]>(&comp, "GetLocalOrientation", &[]).ok().map(f4);
            if cq.is_none_or(|c| (0..4).any(|i| (c[i] - q[i]).abs() > 0.0004)) {
                let qv = q4(q);
                let _ = m::<()>(&comp, "SetLocalOrientation", &[Value::Ptr(qv.as_ptr().cast())]);
            }
        }
        if let Some(el) = s.element_on(&name) {
            let g = |k: &str| el.get(k).and_then(J::as_f64).unwrap_or(0.0);
            let w = [g("ox"), g("oy"), g("oz")];
            let cur = m::<[f32; 4]>(&comp, "GetLocalPosition", &[]).ok().map(f3);
            if cur.is_none_or(|c| (0..3).any(|i| (c[i] - w[i]).abs() > 0.0002)) {
                let p = v4(w, 1.0);
                let _ = m::<()>(&comp, "SetLocalPosition", &[Value::Ptr(p.as_ptr().cast())]);
            }
        }
        if !plane_param.is_empty() {
            if let Some(cur) = cname_prop(&comp, "renderingPlaneAnimationParam") {
                if cur != CName::hashed(&plane_param).0 {
                    set_cname_prop(&comp, "renderingPlaneAnimationParam", CName::hashed(&plane_param).0);
                }
            }
        }
    }
}

// ---------------------------------------------------------------- the hand's reach
fn slots_component(s: &mut Belt) -> Handle {
    let t = now();
    if !s.slots_comp.is_null() && s.slots_at.is_some_and(|a| t - a < 5.0) {
        return s.slots_comp.clone();
    }
    s.slots_at = Some(t);
    s.slots_comp = h(&player(), "FindComponentByName", &[cn("slots")]);
    s.slots_comp.clone()
}
#[repr(C, align(16))]
pub(super) struct WorldTransform(pub [u32; 8]);
/// The player's position and its world -> model rotation (the conjugate of its orientation).
pub(super) fn model_basis(pl: &Handle) -> Option<(V3, Q)> {
    let b = f3(m::<[f32; 4]>(pl, "GetWorldPosition", &[]).ok()?);
    let q = f4(m::<[f32; 4]>(pl, "GetWorldOrientation", &[]).ok()?);
    Some((b, conj(q)))
}
fn hips_model_frame_now(s: &mut Belt) -> Option<(V3, Q)> {
    let pl = player();
    let sc = slots_component(s);
    if pl.is_null() || sc.is_null() {
        return None;
    }
    let (b, c) = model_basis(&pl)?;
    let mut wt = WorldTransform([0; 8]);
    if m::<bool>(&sc, "GetSlotTransform", &[cn("hips"), Value::Out((&mut wt as *mut WorldTransform).cast())]) != Ok(true) {
        return None;
    }
    let v = [0, 1, 2].map(|i| f64::from(wt.0[i] as i32) / 131072.0);
    let q = [4, 5, 6, 7].map(|i| f64::from(f32::from_bits(wt.0[i])));
    let p = qrot(c, [v[0] - b[0], v[1] - b[1], v[2] - b[2]]);
    Some((p, qmul(c, q)))
}
fn hips_model_frame(s: &mut Belt) -> Option<(V3, Q)> {
    if s.hips_frame != s.frame {
        s.hips_frame = s.frame;
        s.hips = hips_model_frame_now(s);
    }
    s.hips
}
fn element_centre(s: &mut Belt, host: &str) -> Option<V3> {
    let sx = s.mirror_sign(host);
    if let Some(el) = s.element_on(host) {
        if let (Some(cx), Some(cy), Some(cz)) = (el.get("cx").and_then(J::as_f64), el.get("cy").and_then(J::as_f64), el.get("cz").and_then(J::as_f64)) {
            let g = |k: &str| el.get(k).and_then(J::as_f64).unwrap_or(0.0);
            return Some([sx * (cx + g("ox")), cy + g("oy"), cz + g("oz")]);
        }
    }
    let c = s.cfg.v("centres").get(host)?.as_array()?.iter().filter_map(J::as_f64).collect::<Vec<_>>();
    if c.len() < 3 {
        return None;
    }
    match s.offset_for(host) {
        None => Some([sx * c[0], c[1], c[2]]),
        Some(o) => {
            let g = |k: &str| o.get(k).and_then(J::as_f64).unwrap_or(0.0);
            Some([sx * (c[0] + g("x")), c[1] + g("y"), c[2] + g("z")])
        }
    }
}
fn ours(host: &str) -> bool {
    OURS.iter().any(|p| host.starts_with(p))
}
fn lit_appearance_for(s: &Belt, host: &str, cur: Option<&str>) -> Option<String> {
    let marked = |n: Option<&str>| n.filter(|n| !n.is_empty()).map(|n| if n.ends_with("_lit") { n.to_string() } else { format!("{n}_lit") });
    // The flash garment asks for militech, which its mesh renders with its default skin; the garment's own name comes
    // back when the hand leaves.
    if host == "ucbeltgrenade_flash" && cur == Some("militech") {
        return Some("default_lit".into());
    }
    if ours(host) {
        return marked(cur);
    }
    if let Some(el) = s.element_on(host) {
        if let Some(hl) = el.get("hl").and_then(J::as_str).filter(|h| !h.is_empty()) {
            return if hl.starts_with("lit") { marked(cur) } else { Some(hl.to_string()) };
        }
    }
    Some(s.cfg.s("highlight_appearance").to_string())
}
fn markable(s: &Belt, host: &str) -> bool {
    if s.cfg.v("highlight_only_ours") == &J::Bool(false) {
        return true;
    }
    if host.starts_with(MAG_PREFIX) {
        return !(s.mag_taken && s.mag_host.as_deref() == Some(host));
    }
    ours(host) || s.element_on(host).is_some()
}
/// The element lit (its "_lit" appearance) or put back.
fn mark_set(s: &mut Belt, host: &str, on: bool) -> bool {
    let pl = player();
    if pl.is_null() {
        return false;
    }
    let mut complete = true;
    for mname in mark_family(host) {
        let c = h(&pl, "FindComponentByName", &[cn(&mname)]);
        if c.is_null() {
            if on {
                complete = false;
            }
            continue;
        }
        if on {
            let cur = mesh_appearance(&c);
            let want = lit_appearance_for(s, &mname, cur.as_deref());
            if let (Some(cur), Some(want)) = (cur, want) {
                if cur != want {
                    s.lit_from.insert(mname.clone(), cur);
                    if !change_appearance(&c, &want, true) {
                        complete = false;
                    }
                }
            }
        } else if let Some(back) = s.lit_from.get(&mname).cloned() {
            if change_appearance(&c, &back, true) {
                s.lit_from.remove(&mname);
            } else {
                complete = false;
            }
        }
    }
    complete
}
fn apply_lit(s: &mut Belt, want: &HashSet<String>) {
    let on: Vec<String> = s.lit_on.iter().cloned().collect();
    for host in on {
        if !want.contains(&host) && mark_set(s, &host, false) {
            s.lit_on.remove(&host);
        }
    }
    for host in want {
        if !s.lit_on.contains(host) && mark_set(s, host, true) {
            s.lit_on.insert(host.clone());
        }
    }
}
fn palm_point_now(s: &Belt, side: usize) -> Option<V3> {
    let p = native::<[f32; 4]>("VRPalmModelPos", &[Value::Int32(side as i32)])?;
    if p[3] == 0.0 {
        return None;
    }
    let mut pt = f3(p);
    let ho = s.cfg.v("highlight_hand_off");
    let g = |k: &str| ho.get(k).and_then(J::as_f64).unwrap_or(0.0);
    let off = [g("x"), g("y"), g("z")];
    if off.iter().map(|v| v.abs()).sum::<f64>() > 1e-6 {
        if let Some(q) = native::<[f32; 4]>("VRPalmModelRot", &[Value::Int32(side as i32)]) {
            let o = qrot(f4(q), off);
            pt = [pt[0] + o[0], pt[1] + o[1], pt[2] + o[2]];
        }
    }
    Some(pt)
}
fn palm_point(s: &mut Belt, side: usize) -> Option<V3> {
    if s.palm_frame[side] != s.frame {
        s.palm_frame[side] = s.frame;
        s.palm[side] = palm_point_now(s, side);
    }
    s.palm[side]
}
fn reach_radius(s: &mut Belt) -> f64 {
    let mut r = s.cfg.f("highlight_radius_m", 0.17);
    let t = now();
    if s.crouch_at.is_none_or(|a| t - a > 0.25) {
        s.crouch_at = Some(t);
        s.crouched = m::<bool>(&h(&player(), "GetPuppetPS", &[]), "IsCrouch", &[]).unwrap_or(false);
    }
    if s.crouched {
        r *= s.cfg.f("crouch_reach_scale", 1.0);
    }
    r
}
fn update_shell_reach(s: &mut Belt, hp: V3, hq: Q) {
    s.shell_d = [None; 2];
    s.shell_near = [None, None];
    if !s.state.wclass.contains("shotgun") {
        return;
    }
    for side in 0..2usize {
        let Some(p) = palm_point(s, side) else { continue };
        for host in SHELL_ROW {
            if !s.comps.contains_key(host) || s.shell_taken.as_deref() == Some(host) {
                continue;
            }
            let Some(e) = element_centre(s, host) else { continue };
            let w = qrot(hq, to_hips_frame(e));
            let d = dist([hp[0] + w[0], hp[1] + w[1], hp[2] + w[2]], p);
            if s.shell_d[side].is_none_or(|b| d < b) {
                (s.shell_d[side], s.shell_near[side]) = (Some(d), Some(host.to_string()));
            }
        }
    }
}
fn update_mag_reach(s: &mut Belt) {
    s.mag_d = [None; 2];
    let host = s.state.wmag.as_ref().map(|k| format!("ucbeltmag_{k}"));
    s.mag_host = host.filter(|h| s.comps.contains_key(h));
    let Some((hp, hq)) = hips_model_frame(s) else { return };
    update_shell_reach(s, hp, hq);
    let Some(host) = s.mag_host.clone() else { return };
    let Some(e) = element_centre(s, &host) else { return };
    let w = qrot(hq, to_hips_frame(e));
    let target = [hp[0] + w[0], hp[1] + w[1], hp[2] + w[2]];
    for side in 0..2usize {
        if let Some(p) = palm_point(s, side) {
            s.mag_d[side] = Some(dist(target, p));
        }
    }
}
fn mark_reach(s: &mut Belt) {
    if s.ring_at.iter().all(Option::is_none) {
        return;
    }
    let Some((hp, hq)) = hips_model_frame(s) else { return };
    let Some((b, c)) = model_basis(&player()) else { return };
    for side in 0..2usize {
        let (Some(a), true) = (s.ring_at[side], s.lit_hand[side].is_some()) else { continue };
        let mm = qrot(hq, a);
        let w = qrot(conj(c), [hp[0] + mm[0], hp[1] + mm[1], hp[2] + mm[2]]);
        let from = palm_point(s, side).map(|pp| {
            let f = qrot(conj(c), pp);
            [b[0] + f[0], b[1] + f[1], b[2] + f[2]]
        });
        super::reach::mark(
            side + 1,
            super::reach::Mark { pos: [b[0] + w[0], b[1] + w[1], b[2] + w[2]], radius: Some(0.03), kind: super::reach::Kind::Near, ready: true, from, lift: Some(0.03) },
        );
    }
}
fn update_highlight(s: &mut Belt, dt: f64) {
    let mut frame_at = None;
    if s.cfg.b("highlight") {
        s.hl_acc += dt;
        if s.hl_acc < 1.0 / s.cfg.f("highlight_rate_hz", 20.0).max(1.0) {
            return;
        }
        s.hl_acc = 0.0;
        if !s.comps.is_empty() {
            frame_at = hips_model_frame(s);
        }
    }
    let Some((hp, hq)) = frame_at else {
        (s.grenade_hover, s.lit_hand, s.ring_at) = ([None, None], [None, None], [None, None]);
        if !s.lit_on.is_empty() {
            apply_lit(s, &HashSet::new());
        }
        return;
    };
    let enter = reach_radius(s);
    // A margin past the reach, not a factor (main's 58a9afab9: 1.35 times the crouched reach let go 34 cm away).
    let leave = enter + s.cfg.f("highlight_release_m", 0.03);
    let pl = player();
    // A hand with a weapon still lights the garment; only a free hand can grab.
    let weapon_drawn = !pl.is_null() && player_plane(s, &pl, now()) == Some(2);
    let before = s.lit_hand.clone();
    let mut targets: Option<Vec<(String, V3, V3)>> = None;
    for side in 0..2usize {
        let Some(p) = palm_point(s, side) else {
            s.lit_hand[side] = None;
            s.grenade_hover[side] = None;
            s.ring_at[side] = None;
            continue;
        };
        let no_grenades = weapon_drawn && weapon_in_hands(&pl, Some(side));
        if targets.is_none() {
            let hosts: Vec<(String, Handle)> = s.comps.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            let mut t = Vec::new();
            for (host, comp) in hosts {
                let Some(e) = element_centre(s, &host) else { continue };
                if markable(s, &host) && enabled(&comp) == Some(true) {
                    let a = to_hips_frame(e);
                    let w = qrot(hq, a);
                    t.push((host, [hp[0] + w[0], hp[1] + w[1], hp[2] + w[2]], a));
                }
            }
            targets = Some(t);
        }
        let ts = targets.as_ref().map(Vec::as_slice).unwrap_or(&[]);
        let (mut best, mut best_d) = (None::<String>, None::<f64>);
        let (mut hover, mut hover_d) = (None::<String>, None::<f64>);
        let mut held = s.lit_hand[side].clone();
        let mut held_grenade_d = None::<f64>;
        for (host, at, _) in ts {
            let d = dist(*at, p);
            let grenade = host.starts_with("ucbeltgrenade");
            if grenade {
                if held.as_deref() == Some(host.as_str()) {
                    held_grenade_d = Some(d);
                }
                let radius = if s.grenade_hover[side].as_deref() == Some(host.as_str()) { leave } else { enter };
                if d <= radius && hover_d.is_none_or(|h| d < h) {
                    (hover, hover_d) = (Some(host.clone()), Some(d));
                }
            }
            if !(no_grenades && grenade) && best_d.is_none_or(|b| d < b) {
                (best, best_d) = (Some(host.clone()), Some(d));
            }
        }
        s.grenade_hover[side] = hover;
        best = best.map(|b| group_lead(&b));
        // A grenade this hand had lit is let go by its own distance, even with another pouch nearest.
        if held.as_deref().is_some_and(|h| h.starts_with("ucbeltgrenade")) && (no_grenades || held_grenade_d.is_none_or(|d| d > leave)) {
            s.lit_hand[side] = None;
            held = None;
        }
        if best.as_deref().is_some_and(|b| b.starts_with("ucbeltgrenade")) {
            let hol = super::holster::near(p[0] as f32, p[1] as f32, p[2] as f32, side as i32);
            if hol >= 0.0 && best_d.is_some_and(|b| f64::from(hol) <= b) {
                (best, best_d) = (None, None);
                s.lit_hand[side] = None;
                held = None;
            }
        }
        match (&best, best_d) {
            (Some(b), Some(d)) => {
                if held.as_deref() == Some(b.as_str()) {
                    if d > leave {
                        s.lit_hand[side] = None;
                    }
                } else if d <= enter {
                    s.lit_hand[side] = Some(b.clone());
                }
            }
            _ => s.lit_hand[side] = None,
        }
        let mut ring: Option<(V3, f64)> = None;
        if let Some(lit) = &s.lit_hand[side] {
            for (host, at, a) in ts {
                if group_lead(host) == *lit {
                    let d = dist(*at, p);
                    if ring.is_none_or(|r| d < r.1) {
                        ring = Some((*a, d));
                    }
                }
            }
        }
        s.ring_at[side] = ring.map(|r| r.0);
    }
    for side in 0..2usize {
        if let Some(hn) = &s.lit_hand[side] {
            if before[side].as_ref() != Some(hn) {
                native::<()>("VRHapticEvent", &[Value::Int32(1), Value::Int32(if side == 1 { 2 } else { 1 })]);
            }
        }
    }
    // A lit grenade holder shows by the hover alone; a hand's lit grenade still owns the grip.
    let mut want: HashSet<String> = s.lit_hand.iter().flatten().filter(|h| !h.starts_with("ucbeltgrenade")).cloned().collect();
    want.extend(s.grenade_hover.iter().flatten().cloned());
    apply_lit(s, &want);
}

// ---------------------------------------------------------------- the props in the belt slots
fn active_gadget(pl: &Handle) -> Option<[u64; 2]> {
    let pd = h(&equip_system(), "GetPlayerData", &[Value::Handle(pl)]);
    m::<[u64; 2]>(&pd, "GetActiveGadget", &[]).ok().filter(is_valid)
}
/// What the equipped grenade's prop is: its own record per type (data: Items.VRPortGrenade_<kind>).
pub(super) fn prop_item_grenade(s: &mut Belt) -> Option<(String, String)> {
    let pl = player();
    let id = active_gadget(&pl)?;
    let nm = record_name(id[0]).filter(|n| !n.is_empty())?;
    if !is_grenade(&id) {
        return None;
    }
    let low = nm.to_lowercase();
    let (kind, entity) = GRENADE_PROPS.iter().find(|(k, _)| low.contains(k)).copied().unwrap_or(("frag", "vrp_gren_frag"));
    let rec = format!("{GRENADE_ITEM}_{kind}");
    if crate::tweak_records::record_exists(&rec) {
        return Some((rec, entity.to_string()));
    }
    if s.grenade_entity.as_deref() != Some(entity) && crate::tweak_records::set_cname_flat(GRENADE_ITEM, "entityName", CName::hashed(entity).0) {
        s.grenade_entity = Some(entity.to_string());
        log!("[belt] prop: grenade -> {entity}\n");
    }
    Some((GRENADE_ITEM.to_string(), entity.to_string()))
}
fn prop_item(s: &mut Belt, p: &J) -> Option<(String, String)> {
    match p.get("follow").and_then(J::as_str) {
        None => p.get("item").and_then(J::as_str).map(|i| (i.to_string(), i.to_string())),
        Some("grenade") => prop_item_grenade(s),
        Some(_) => p.get("item").and_then(J::as_str).map(|i| (i.to_string(), i.to_string())),
    }
}
/// propAllowSlot: the item's placementSlots gain the slot once (a whitelist the engine enforces).
pub(super) fn prop_allow_slot(s: &mut Belt, item: &str, slot: &str) {
    let key = format!("{item}|{slot}");
    if !s.allowed.insert(key) {
        return;
    }
    if crate::tweak_records::allow_slot(item, slot) == Some(true) {
        log!("[belt] prop: allowed {slot} on {item}\n");
    }
}
/// Whether a weapon is in a hand slot (`side` 0 the left, 1 the right, None either): presence only -- the held
/// grenade props and the throws have slots of their own (main's c026279ef).
pub(super) fn weapon_in_hands(pl: &Handle, side: Option<usize>) -> bool {
    let has = |slot: &str| !in_slot(pl, slot).is_null();
    match side {
        Some(0) => has("AttachmentSlots.WeaponLeft"),
        Some(_) => has("AttachmentSlots.WeaponRight"),
        None => has("AttachmentSlots.WeaponRight") || has("AttachmentSlots.WeaponLeft"),
    }
}
/// The player's plane: 2 with a weapon in a hand, 0 without (read five times a second).
pub(super) fn player_plane(s: &mut Belt, pl: &Handle, t: f64) -> Option<i32> {
    if t - s.plane_at < 0.2 {
        return s.plane_val;
    }
    s.plane_at = t;
    s.plane_val = Some(if weapon_in_hands(pl, None) { 2 } else { 0 });
    s.plane_val
}
fn is_mesh(c: &Handle) -> bool {
    class_name(c).contains("MeshComponent")
}
fn prop_place(p: &J, obj: &Handle) -> bool {
    let g = |k: &str| p.get(k).and_then(J::as_f64).unwrap_or(0.0);
    let q = q4(euler_quat(g("rx"), g("ry"), g("rz")));
    let pos = v4([g("up"), g("back"), g("right")], 1.0);
    let mut n = 0;
    for c in components(obj) {
        let cls = class_name(&c);
        if cls.contains("MeshComponent") {
            let _ = m::<()>(&c, "SetLocalPosition", &[Value::Ptr(pos.as_ptr().cast())]);
            let _ = m::<()>(&c, "SetLocalOrientation", &[Value::Ptr(q.as_ptr().cast())]);
            if cls.contains("Skinned") {
                set_cname_prop(&c, "renderingPlaneAnimationParam", CName::hashed("renderPlane").0);
            }
            n += 1;
        }
    }
    n > 0
}
/// A FocusForcedHighlightData for the prop's claim (the engine's outline).
fn highlight_data(s: &Belt, name: &str, out: f32, prop: bool) -> Handle {
    let Some(class) = crate::script_call::class_any("FocusForcedHighlightData") else { return Handle::null() };
    // SAFETY: a new instance owned by the handle made for it; its fields written in place through the RTTI.
    let d = unsafe { Handle::from_instance(class.create_instance()) };
    let hud_class = crate::script_call::class_any("HighlightInstance");
    let set = |prop_name: &str, write: &dyn Fn(*mut u8)| {
        // SAFETY: the instance's own property.
        if let Some(p) = unsafe { crate::script_call::property(d.instance, prop_name) } {
            write(p as *mut u8);
        }
    };
    let ctx = s.cfg.f(if prop { "prop_context" } else { "highlight_context" }, 1.0) as i32;
    let ty = s.cfg.f(if prop { "prop_type" } else { "highlight_type" }, 0.0) as i32;
    let outline = s.cfg.f(if prop { "prop_outline" } else { "highlight_outline" }, 8.0) as i32;
    let reveal = s.cfg.b("highlight_through_walls");
    // SAFETY: plain values of the properties' own types.
    unsafe {
        set("isSavable", &|p| *(p as *mut bool) = false);
        if let Some(hc) = hud_class {
            let hud = Handle::from_instance(hc.create_instance());
            if let Some(hp) = crate::script_call::property(hud.instance, "context") {
                *(hp as *mut i32) = ctx;
            }
            if let Some(hp) = crate::script_call::property(hud.instance, "instant") {
                *(hp as *mut bool) = true;
            }
            set("hudData", &|p| {
                let old = std::ptr::replace(p as *mut Handle, hud.clone());
                drop(old);
            });
        }
        set("highlightType", &|p| *(p as *mut i32) = ty);
        set("outlineType", &|p| *(p as *mut i32) = outline);
        set("inTransitionTime", &|p| *(p as *mut f32) = 0.0);
        set("outTransitionTime", &|p| *(p as *mut f32) = out);
        let abs = crate::script_call::enum_value("EPriority", "Absolute").unwrap_or(0) as i32;
        set("priority", &|p| *(p as *mut i32) = abs);
        set("sourceName", &|p| *(p as *mut u64) = CName::hashed(name).0);
        set("isRevealed", &|p| *(p as *mut bool) = reveal);
    }
    d
}
fn prop_light(s: &Belt, obj: &Handle, base: &str, on: bool, flip: bool) {
    if obj.is_null() {
        return;
    }
    let (live, dead) = if flip { (format!("{base}_a"), format!("{base}_b")) } else { (format!("{base}_b"), format!("{base}_a")) };
    let dd = highlight_data(s, &dead, 0.0, true);
    if !dd.is_null() {
        let _ = m::<()>(obj, "CancelForcedVisionAppearance", &[Value::Handle(&dd)]);
    }
    let dl = highlight_data(s, &live, 0.0, true);
    if dl.is_null() {
        return;
    }
    let _ = m::<()>(obj, if on { "ForceVisionAppearance" } else { "CancelForcedVisionAppearance" }, &[Value::Handle(&dl)]);
}
fn comp_tag(c: &Handle) -> String {
    comp_name(c)
}
fn prop_emissive(s: &Belt, obj: &Handle, on: bool) -> bool {
    if obj.is_null() {
        return false;
    }
    let level = s.cfg.f("prop_glow_level", 1.0) as usize;
    let names = ["lit", "lit2", "lit3", "lit4", "lit5", "lit6", "lit7", "lit8"];
    let want = names.get(level.wrapping_sub(1)).copied().unwrap_or("lit");
    let k = s.cfg.f("prop_rim_scale", 1.05) as f32;
    let mut n = 0;
    for c in components(obj) {
        if comp_tag(&c).starts_with("vrp_lit_") {
            if on {
                change_appearance(&c, want, false);
                // SAFETY: the component's Vector3 property.
                if let Some(p) = unsafe { crate::script_call::property(c.instance, "visualScale") } {
                    unsafe { *(p as *mut [f32; 3]) = [k, k, k] };
                }
            }
            toggle(&c, on);
            n += 1;
        }
    }
    n > 0
}
fn prop_glow(s: &mut Belt, i: usize, obj: &Handle, on: bool) -> bool {
    if obj.is_null() {
        return false;
    }
    if s.props[i].glow_comp.is_null() {
        s.props[i].glow_comp = components(obj).into_iter().find(|x| comp_tag(x) == "vrp_glow").unwrap_or_else(Handle::null);
    }
    let c = s.props[i].glow_comp.clone();
    if c.is_null() {
        return false;
    }
    if on {
        // SAFETY: the light component's Float properties.
        unsafe {
            if let Some(p) = crate::script_call::property(c.instance, "intensity") {
                *(p as *mut f32) = s.cfg.f("prop_glow_lm", 60.0) as f32;
            }
            if let Some(p) = crate::script_call::property(c.instance, "radius") {
                *(p as *mut f32) = s.cfg.f("prop_glow_r", 0.35) as f32;
            }
        }
    }
    toggle(&c, on)
}
fn prop_obj(s: &mut Belt, i: usize, p: &J, t: f64) -> Handle {
    let ps = &s.props[i];
    let slot = p.get("slot").and_then(J::as_str).unwrap_or("").to_string();
    let fresh = t - ps.obj_at.unwrap_or(-1.0) < 0.5;
    let on = p.get("on") == Some(&J::Bool(true));
    if fresh && (!ps.obj.is_null() || (!on && ps.empty_slot.as_deref() == Some(slot.as_str()))) {
        return ps.obj.clone();
    }
    let pl = player();
    if pl.is_null() {
        return Handle::null();
    }
    let o = in_slot(&pl, &slot);
    let ps = &mut s.props[i];
    ps.empty_slot = o.is_null().then_some(slot);
    if o.instance != ps.obj.instance {
        (ps.mesh, ps.glow_comp) = (Handle::null(), Handle::null());
        if !o.is_null() {
            ps.fresh = true;
        }
    }
    (ps.obj, ps.obj_at) = (o.clone(), Some(t));
    o
}
fn prop_unmark(s: &mut Belt, i: usize, obj: &Handle) {
    if !obj.is_null() {
        prop_emissive(s, obj, false);
        prop_glow(s, i, obj, false);
        prop_light(s, obj, &format!("vrpProp{}", i + 1), false, true);
        prop_light(s, obj, &format!("vrpProp{}", i + 1), false, false);
    }
    let ps = &mut s.props[i];
    (ps.lit, ps.emis, ps.glow, ps.claim_at) = (None, None, None, None);
}
fn update_prop_distances(s: &mut Belt, t: f64) {
    let props = s.cfg.v("props").as_array().cloned().unwrap_or_default();
    let active = |p: &J| p.get("on") == Some(&J::Bool(true)) && p.get("auto") == Some(&J::Bool(true));
    if !props.iter().any(active) {
        for ps in &mut s.props {
            ps.dist = None;
        }
        return;
    }
    let pw: Vec<V3> = (0..2).filter_map(|side| native::<[f32; 4]>("VRPalmWorldPos", &[Value::Int32(side)]).filter(|p| p[3] != 0.0).map(f3)).collect();
    for (i, p) in props.iter().enumerate() {
        let mut best: Option<f64> = None;
        if active(p) && !pw.is_empty() {
            let obj = prop_obj(s, i, p, t);
            if !obj.is_null() {
                if s.props[i].mesh.is_null() {
                    // On a grenade prop the skinned source comes before the plain mesh actually drawn; only that one
                    // carries the belt placement in the world (main's c026279ef).
                    s.props[i].mesh = if p.get("follow").and_then(J::as_str) == Some("grenade") {
                        h(&obj, "FindComponentByName", &[cn("mag_mesh")])
                    } else {
                        components(&obj).into_iter().find(is_mesh).unwrap_or_else(Handle::null)
                    };
                }
                if let Ok(l2w) = m::<[[f32; 4]; 4]>(&s.props[i].mesh, "GetLocalToWorld", &[]) {
                    let w = [f64::from(l2w[3][0]), f64::from(l2w[3][1]), f64::from(l2w[3][2])];
                    for palm in &pw {
                        let d = dist(*palm, w);
                        if best.is_none_or(|b| d < b) {
                            best = Some(d);
                        }
                    }
                }
            }
        }
        if let Some(ps) = s.props.get_mut(i) {
            ps.dist = best;
        }
    }
}
fn drop_item(pl: &Handle, slot: &str) {
    super::hold::drop_item(pl, slot);
}
/// A String argument kept alive for the call.
pub(super) struct ArgStr(EngineStr);
impl ArgStr {
    pub(super) fn value(&self) -> Value {
        Value::Ptr(self.0.as_ptr())
    }
}
pub(super) fn tdb_str(text: &str) -> ArgStr {
    ArgStr(engine_str(text))
}
fn update_props(s: &mut Belt, t: f64) {
    let props = s.cfg.v("props").as_array().cloned().unwrap_or_default();
    let pl = player();
    if pl.is_null() {
        return;
    }
    if s.props.len() != props.len() {
        s.props = (0..props.len()).map(|_| PropState::default()).collect();
    }
    for (i, p) in props.iter().enumerate() {
        let mut obj = prop_obj(s, i, p, t);
        let on = p.get("on") == Some(&J::Bool(true));
        if !on && obj.is_null() {
            continue;
        }
        let slot = p.get("slot").and_then(J::as_str).unwrap_or("").to_string();
        let want = prop_item(s, p);
        let want_tag = want.as_ref().map(|w| w.1.clone());
        if want_tag != s.props[i].item && !obj.is_null() {
            prop_unmark(s, i, &obj);
            drop_item(&pl, &slot);
            let ps = &mut s.props[i];
            (ps.obj, ps.mesh, ps.obj_at, ps.placed) = (Handle::null(), Handle::null(), None, None);
            obj = Handle::null();
        }
        s.props[i].item = want_tag;
        let plane = player_plane(s, &pl, t);
        if !obj.is_null() && plane.is_some() && s.props[i].plane.is_some() && plane != s.props[i].plane {
            prop_unmark(s, i, &obj);
            drop_item(&pl, &slot);
            let ps = &mut s.props[i];
            (ps.obj, ps.mesh, ps.obj_at, ps.placed) = (Handle::null(), Handle::null(), None, None);
            (ps.at, ps.plane, ps.want) = (0.0, None, plane);
            obj = Handle::null();
        }
        match (on, want) {
            (true, Some((item, _))) => {
                if obj.is_null() {
                    if s.props[i].at + 1.0 < t {
                        s.props[i].at = t;
                        s.props[i].placed = None;
                        (s.props[i].obj, s.props[i].mesh, s.props[i].obj_at) = (Handle::null(), Handle::null(), None);
                        prop_allow_slot(s, &item, &slot);
                        let wplane = s.props[i].want.or(plane).unwrap_or(0);
                        let ps = &mut s.props[i];
                        (ps.plane, ps.want) = (Some(wplane), None);
                        (ps.glow, ps.glow_comp, ps.emis) = (None, Handle::null(), None);
                        super::hold::hold_item(&pl, &item, &slot, wplane == 2);
                    }
                } else {
                    if s.props[i].fresh {
                        s.props[i].fresh = false;
                        prop_unmark(s, i, &obj);
                        s.props[i].plane = None;
                    }
                    let g = |k: &str| p.get(k).and_then(J::as_f64).unwrap_or(0.0);
                    let key = format!("{:.4}/{:.4}/{:.4}/{:.2}/{:.2}/{:.2}", g("up"), g("back"), g("right"), g("rx"), g("ry"), g("rz"));
                    if s.props[i].placed.as_deref() != Some(key.as_str()) && prop_place(p, &obj) {
                        s.props[i].placed = Some(key);
                    }
                    let mut want_lit = p.get("lit") == Some(&J::Bool(true));
                    if p.get("auto") == Some(&J::Bool(true)) {
                        let enter = s.cfg.f("prop_radius_m", 0.06);
                        let leave = enter * s.cfg.f("prop_release", 1.4);
                        want_lit = match s.props[i].dist {
                            None => false,
                            Some(d) if s.props[i].lit == Some(true) => d <= leave,
                            Some(d) => d <= enter,
                        };
                        if p.get("lit") == Some(&J::Bool(true)) {
                            want_lit = true;
                        }
                    }
                    let changed = Some(want_lit) != s.props[i].lit;
                    let mark = s.cfg.f("prop_mark", 0.0) as i64;
                    let base = format!("vrpProp{}", i + 1);
                    if mark == 0 || mark == 2 {
                        let due = t - s.props[i].claim_at.unwrap_or(-1.0) >= s.cfg.f("prop_renew_s", 0.35);
                        if changed || due {
                            s.props[i].flip = !s.props[i].flip;
                            prop_light(s, &obj, &base, want_lit, s.props[i].flip);
                            s.props[i].claim_at = Some(t);
                        }
                    } else if s.props[i].claim_at.is_some() {
                        prop_light(s, &obj, &base, false, s.props[i].flip);
                        prop_light(s, &obj, &base, false, !s.props[i].flip);
                        s.props[i].claim_at = None;
                    }
                    let want_emis = (mark == 1 || mark == 2) && want_lit;
                    let emis_key = want_emis.then(|| s.cfg.f("prop_glow_level", 1.0) as i64);
                    if Some(emis_key) != s.props[i].emis && prop_emissive(s, &obj, want_emis) {
                        s.props[i].emis = Some(emis_key);
                    }
                    let want_glow = mark == 3 && want_lit;
                    if Some(want_glow) != s.props[i].glow && prop_glow(s, i, &obj, want_glow) {
                        s.props[i].glow = Some(want_glow);
                    }
                    s.props[i].lit = Some(want_lit);
                }
            }
            _ => {
                if !obj.is_null() {
                    s.props[i].placed = None;
                    prop_unmark(s, i, &obj);
                    drop_item(&pl, &slot);
                }
            }
        }
    }
}

// ---------------------------------------------------------------- startup clothing (EquipmentEx's outfit route)
fn startup_tick(s: &mut Belt, t: f64, pl: &Handle) -> Option<(bool, bool, bool, String)> {
    if s.startup.done {
        return None;
    }
    s.startup.changed = false;
    match startup_step(s, t, pl) {
        Ok(r) => {
            let changed = s.startup.changed;
            match r {
                Some(done) => Some((true, done, changed, String::new())),
                None if changed => Some((true, false, true, String::new())),
                None => None,
            }
        }
        Err(e) => {
            s.startup.errors += 1;
            s.startup.due = Some(t + 1.0);
            if s.startup.errors >= 3 {
                s.startup.done = true;
            }
            Some((false, s.startup.done, s.startup.changed, e))
        }
    }
}
fn tdb_array(o: &Handle, name: &str) -> Vec<u64> {
    // SAFETY: the object, held; the array result kept while read.
    unsafe { crate::script_call::method_owned(o.instance, name, &[]) }
        .map(|a| unsafe { (*(a.as_ptr() as *const red4ext_abi::dynarray::DynArray<u64>)).as_slice().to_vec() })
        .unwrap_or_default()
}
fn startup_step(s: &mut Belt, t: f64, pl: &Handle) -> R<Option<bool>> {
    let outfit = outfit_system(s);
    let ts = transactions();
    if outfit.is_null() || ts.is_null() {
        return Ok(None);
    }
    let (belt_slot, _) = s.slot();
    let belt = h(&ts, "GetItemInSlot", &[Value::Handle(pl), Value::U64(belt_slot)]);
    let want = s.wanted_record();
    if belt.is_null() || item_id(&belt).and_then(|id| record_name(id[0])) != want {
        (s.startup.belt_key, s.startup.due) = (None, None);
        return Ok(None);
    }
    let bk = entity_hash(&belt);
    if s.startup.belt_key != bk {
        (s.startup.belt_key, s.startup.due) = (bk, Some(t + 1.0));
    }
    if s.startup.due.is_some_and(|d| t < d) {
        return Ok(None);
    }
    s.startup.waits += 1;
    if s.startup.waits > 120 {
        s.startup.done = true;
        return Err("clothing attachments did not become ready".into());
    }
    if m::<bool>(&outfit, "IsBlocked", &[])? {
        return Ok(None);
    }
    let belt_name = record_name(belt_slot).unwrap_or_default();
    let clothing = |slot: u64| {
        let n = record_name(slot).unwrap_or_default();
        n != belt_name && n != "OutfitSlots.HandPropLeft" && n != "OutfitSlots.HandPropRight"
    };
    let parts = || -> Vec<Handle> {
        // SAFETY: the outfit system, held; the array of handles kept while read.
        unsafe { crate::script_call::method_owned(outfit.instance, "GetEquipmentParts", &[]) }
            .map(|a| handles(a.as_ptr()))
            .unwrap_or_default()
    };
    if !s.startup.planned {
        if s.startup.plan.is_none() {
            let mut plan = Vec::new();
            let has_clothes = tdb_array(&outfit, "GetUsedSlots").into_iter().any(clothing);
            if m::<bool>(&outfit, "IsActive", &[])? && !has_clothes {
                for part in parts() {
                    let item = m::<[u64; 2]>(&part, "GetItemID", &[])?;
                    let slot = m::<u64>(&outfit, "GetItemSlot", &[Value::Ptr(item.as_ptr().cast())])?;
                    if m::<bool>(&outfit, "IsOutfitSlot", &[Value::U64(slot)])? && clothing(slot) {
                        plan.push((item, slot));
                    }
                }
            }
            s.startup.plan = Some(plan);
        }
        for (item, slot) in s.startup.plan.clone().unwrap_or_default() {
            if !m::<bool>(&outfit, "IsOccupied", &[Value::U64(slot)])? {
                s.startup.changed = true;
                if m::<bool>(&outfit, "EquipItem", &[Value::Ptr(item.as_ptr().cast()), Value::U64(slot)])? != true {
                    return Err(format!("cannot restore equipped clothing in {}", record_name(slot).unwrap_or_default()));
                }
                s.startup.restored += 1;
            }
        }
        s.startup.planned = true;
        if s.startup.restored > 0 {
            s.startup.due = Some(t + 0.5);
            return Ok(None);
        }
    }
    let slots: Vec<u64> = if m::<bool>(&outfit, "IsActive", &[])? {
        tdb_array(&outfit, "GetUsedSlots")
    } else {
        parts().iter().filter_map(|p| m::<u64>(p, "GetSlotID", &[]).ok()).collect()
    };
    if slots.iter().any(|sl| clothing(*sl) && h(&ts, "GetItemInSlot", &[Value::Handle(pl), Value::U64(*sl)]).is_null()) {
        return Ok(None);
    }
    for sl in slots {
        if clothing(sl) && s.startup.refreshed.insert(sl) {
            s.startup.changed = true;
            let _ = m::<()>(&ts, "RefreshAttachment", &[Value::Handle(pl), Value::U64(sl)]);
            s.startup.refresh_count += 1;
        }
    }
    s.startup.done = true;
    Ok(Some(true))
}

// ---------------------------------------------------------------- the frame
fn clear_garment_cache(s: &mut Belt) {
    (s.belt_hash, s.comps, s.dumped) = (None, HashMap::new(), false);
    (s.belt_parts, s.belt_parts_for) = (None, None);
    s.piece_map_gen += 1;
    s.dressed.clear();
    (s.lit_on, s.lit_from, s.lit_hand, s.grenade_hover) = (HashSet::new(), HashMap::new(), [None, None], [None, None]);
    (s.to_dress, s.settle_at, s.recheck) = (false, None, None);
    s.belt_mirrored = false;
}
fn memo_drop(s: &mut Belt) {
    s.body_tag = None;
    s.slot_memo.clear();
    s.psm = Handle::null();
    s.mag_by_ent.clear();
    s.slots_comp = Handle::null();
    s.startup = Startup::default();
    s.comps_for = None;
    clear_garment_cache(s);
}
fn drop_if_player_changed(s: &mut Belt, pl: &Handle, who: Option<u64>) {
    if who.is_none() || who == s.comps_for {
        return;
    }
    s.startup = Startup::default();
    s.comps_for = who;
    s.hold_off = Some(now() + 1.0);
    grenade::release(s, pl);
    s.comps.clear();
    s.dressed.clear();
    (s.belt_parts, s.belt_parts_for) = (None, None);
    s.piece_map_gen += 1;
    (s.lit_on, s.lit_from, s.lit_hand, s.grenade_hover) = (HashSet::new(), HashMap::new(), [None, None], [None, None]);
    s.belt_hash = None;
    s.mag_by_ent.clear();
}
fn publish(s: &mut Belt) {
    let mag_key = s.mag_host.as_deref().and_then(|h| h.strip_prefix(MAG_PREFIX)).and_then(|k| WEAPON_MAG.iter().position(|w| *w == k)).map_or(-1, |i| i as i32);
    let r = reach_radius(s) as f32;
    let mut p = PUBLISHED.lock().unwrap_or_else(|e| e.into_inner());
    p.mag_key = mag_key;
    p.mag_d = s.mag_d.map(|d| d.map_or(-1.0, |v| v as f32));
    p.shell_d = s.shell_d.map(|d| d.map_or(-1.0, |v| v as f32));
    p.radius = r;
    // The holster's question: a lit grenade holder (or a grenade in that hand) owns the grip.
    let reach = |side: usize| {
        s.cfg.b("grenade_grab")
            && match &s.grab.slot {
                Some(_) => s.grab.side == Some(side),
                None => s.lit_hand[side].as_deref().is_some_and(|h| h.starts_with("ucbeltgrenade")),
            }
    };
    super::holster::belt_reach(reach(0), reach(1));
}
fn frame(s: &mut Belt, dt: f64) -> R<()> {
    s.frame += 1;
    let generation = CFG_GENERATION.load(Ordering::Acquire);
    if generation != s.cfg_generation {
        // the generation read before the file: a save landing meanwhile is seen at the next frame
        s.cfg_generation = generation;
        s.cfg = load_cfg();
        s.slot_memo.clear();
    }
    if !s.cfg.b("enabled") {
        return Ok(());
    }
    let handler = super::requests_handler().ok().filter(|r| !r.is_null());
    if handler.as_ref().is_some_and(|r| m::<bool>(r, "IsPreGame", &[]).unwrap_or(false)) {
        return Ok(());
    }
    let pl = player();
    if pl.is_null() {
        memo_drop(s);
        return Ok(());
    }
    if handler.as_ref().is_some_and(|r| m::<bool>(r, "IsGamePaused", &[]).unwrap_or(false)) {
        return Ok(());
    }
    if ATTACHED.swap(false, Ordering::Relaxed) {
        memo_drop(s);
    }
    let who = entity_hash(&pl);
    drop_if_player_changed(s, &pl, who);
    if let Some(at) = s.hold_off {
        if now() < at {
            return Ok(());
        }
        s.hold_off = None;
    }
    // The reload's bridge.
    match MAG_TAKEN.swap(-1, Ordering::Relaxed) {
        -1 => {}
        v => mag_taken(s, v == 1),
    }
    match SHELL_TAKEN.swap(-2, Ordering::Relaxed) {
        -2 => {}
        -1 => s.shell_taken = None,
        side => {
            if s.shell_taken.is_none() {
                s.shell_taken = s.shell_near[side.clamp(0, 1) as usize].clone();
            }
        }
    }
    let scene = scene_hides(s);
    if scene != s.scene {
        (s.scene, s.scene_flip, s.belt_parts) = (scene, true, None);
    }
    let worn = apply_belt_pieces(s, &pl, who);
    apply_belt_chunks(s, worn);
    update_mag_reach(s);
    update_highlight(s, dt);
    mark_reach(s);
    let t = now();
    grenade::tick(s, t, dt);
    update_prop_distances(s, t);
    update_props(s, t);
    publish(s);
    s.acc += dt;
    let period = 1.0 / s.cfg.f("rate_hz", 4.0).max(1.0);
    if s.acc < period && !s.scene_flip {
        return Ok(());
    }
    (s.acc, s.scene_flip) = (0.0, false);
    ensure_equipped(s, t);
    if s.cfg.b("auto_equip") && !s.startup.done && s.slot_name() == s.cfg.s("outfit_slot") {
        if let Some((ok, done, changed, err)) = startup_tick(s, t, &pl) {
            if ok && done {
                log!("[belt] startup clothing refreshed once: restored={} slots={}\n", s.startup.restored, s.startup.refresh_count);
            } else if !ok {
                log!("[belt] startup clothing refresh {}: {err}\n", if done { "stopped" } else { "deferred" });
            }
            if changed {
                clear_garment_cache(s);
                s.hold_off = Some(now() + 1.0);
                return Ok(());
            }
        }
    }
    dress_elements(s);
    if s.cfg.b("garment_offsets") && t - s.offsets_at > 5.0 {
        s.offsets_at = t;
        let _ = st::<()>("ArchiveXL", "EnableGarmentOffsets", &[]);
    }
    s.state = read_state(s);
    apply_elements(s);
    if !s.reported && !s.comps.is_empty() {
        s.reported = true;
        let shown: Vec<&String> = s.comps.iter().filter(|(_, c)| enabled(c) == Some(true)).map(|(n, _)| n).collect();
        log!(
            "[mechanics] belt: worn from Rust ({} elements, {} shown, state weapon {} grenade {} mag {:?}, scene {})\n",
            s.comps.len(),
            shown.len(),
            s.state.weapon,
            s.state.grenade,
            s.state.wmag,
            s.scene
        );
    }
    // Eight seconds into the session the loaded equipment is in: what the belt shows by then.
    if s.reported && !s.late_done {
        let at = *s.late_at.get_or_insert(t);
        if t - at >= 8.0 {
            s.late_done = true;
            let mut shown: Vec<&String> = s.comps.iter().filter(|(_, c)| enabled(c) == Some(true)).map(|(n, _)| n).collect();
            shown.sort();
            let mut cls: Vec<&&str> = s.state.wclass.iter().collect();
            cls.sort();
            log!(
                "[mechanics] belt: after 8 s from Rust -- shown {:?}; weapon {} grenade {} ({:?}, {:?} charges) mag {:?} classes {:?}; pieces {}, props {}\n",
                shown,
                s.state.weapon,
                s.state.grenade,
                s.state.gtype,
                s.state.gcount,
                s.state.wmag,
                cls,
                s.piece_on.values().filter(|v| **v == Some(true)).count(),
                s.props.iter().filter(|p| !p.obj.is_null()).count()
            );
        }
    }
    Ok(())
}
fn mag_taken(s: &mut Belt, on: bool) {
    if on == s.mag_taken {
        return;
    }
    s.mag_taken = on;
    if !on {
        return;
    }
    let Some(host) = s.mag_host.clone() else { return };
    for side in 0..2 {
        if s.lit_hand[side].as_deref() == Some(host.as_str()) {
            s.lit_hand[side] = None;
        }
    }
    if s.lit_on.contains(&host) {
        mark_set(s, &host, false);
        s.lit_on.remove(&host);
    }
}

/// The mod's onUpdate.
pub(in crate::mechanics) fn tick(dt: f32) {
    let actions = std::mem::take(&mut WINDOW.lock().unwrap_or_else(|p| p.into_inner()).actions);
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let s = guard.get_or_insert_with(Belt::new);
    for a in actions {
        window_action(s, a);
    }
    if let Err(e) = frame(s, f64::from(dt)) {
        if s.errors.insert(e.clone()) {
            log!("[belt] {e} (from Rust)\n");
        }
    }
}
/// The mod's onShutdown: the grenade let go, the throw slot cleared.
pub(in crate::mechanics) fn shutdown() {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(s) = guard.as_mut() {
        let pl = player();
        grenade::release(s, &pl);
        grenade::disarm(s, &pl);
    }
}
/// The Lua observer's OnGameAttached (memoDrop).
pub(in crate::mechanics) fn attached() {
    ATTACHED.store(true, Ordering::Relaxed);
}
pub(in crate::mechanics) fn set_mag_taken(on: bool) {
    MAG_TAKEN.store(i32::from(on), Ordering::Relaxed);
}
pub(in crate::mechanics) fn set_shell_taken(on: bool, side: i32) {
    SHELL_TAKEN.store(if on { side.clamp(0, 1) } else { -1 }, Ordering::Relaxed);
}
pub(in crate::mechanics) fn mag_key() -> i32 {
    PUBLISHED.lock().unwrap_or_else(|e| e.into_inner()).mag_key
}
pub(in crate::mechanics) fn mag_dist(side: i32) -> f32 {
    PUBLISHED.lock().unwrap_or_else(|e| e.into_inner()).mag_d[side.clamp(0, 1) as usize]
}
pub(in crate::mechanics) fn shell_dist(side: i32) -> f32 {
    PUBLISHED.lock().unwrap_or_else(|e| e.into_inner()).shell_d[side.clamp(0, 1) as usize]
}
pub(in crate::mechanics) fn reach_radius_published() -> f32 {
    PUBLISHED.lock().unwrap_or_else(|e| e.into_inner()).radius
}

// ---------------------------------------------------------------- the CET overlay's window (dev_windows.rs)
/// What the mod's "VR tactical belt" window edits and asks for: a copy of the belt's config (written to belt.cfg when
/// the overlay closes -- the mod's saveCfg on onOverlayClose; the tick reads the file again, as after any change), the
/// offsets' filter, and what only the main thread's tick may do.
struct Window {
    doc: Option<J>,
    filter: String,
    actions: Vec<Action>,
}
enum Action {
    Equip,
    Play(String),
    Pin(usize),
    Unpin,
}
static WINDOW: Mutex<Window> = Mutex::new(Window { doc: None, filter: String::new(), actions: Vec::new() });
fn window_action(s: &mut Belt, a: Action) {
    match a {
        Action::Equip => {
            s.last_equip_try = 0.0;
            ensure_equipped(s, now());
        }
        Action::Play(name) => {
            if let Some(id) = entity_hash(&player()) {
                let audio = super::game_system("GetAudioSystem");
                let _ = m::<()>(&audio, "Play", &[Value::U64(CName::hashed(&name).0), Value::U64(id), Value::U64(0)]);
            }
        }
        Action::Pin(side) => {
            let host = s.grab.host.clone().unwrap_or_default();
            grenade::pin(s, side, &host);
        }
        Action::Unpin => grenade::unpin(s),
    }
}
/// belt.cfg as the mod's saveCfg writes it: the whole config.
fn write_cfg(doc: &J) {
    if let Ok(text) = serde_json::to_string(doc) {
        let _ = std::fs::write(cfg_path(), text);
    }
    CFG_GENERATION.fetch_add(1, Ordering::Release);
}
/// writeOffsetFile: offsets.json / offsets_female.json (tools/bake_belt_offsets.py reads them), names sorted.
fn write_offset_file(name: &str, table: &J) -> usize {
    let Some(t) = table.as_object() else { return 0 };
    let mut names: Vec<&String> = t.keys().collect();
    names.sort();
    let f = |o: &J, k: &str| o.get(k).and_then(J::as_f64).unwrap_or(0.0);
    let parts: Vec<String> = names
        .iter()
        .map(|nm| {
            let o = &t[*nm];
            format!("  \"{nm}\": {{ \"x\": {:.5}, \"y\": {:.5}, \"z\": {:.5}, \"rx\": {:.3}, \"ry\": {:.3}, \"rz\": {:.3} }}", f(o, "x"), f(o, "y"), f(o, "z"), f(o, "rx"), f(o, "ry"), f(o, "rz"))
        })
        .collect();
    let path = cvr_host::paths::file(cvr_host::paths::Folder::Lua, &format!("CyberpunkVRPort_TacticalBelt\\{name}"));
    let _ = std::fs::write(path, format!("{{\n{}\n}}\n", parts.join(",\n")));
    names.len()
}
/// The doc's value at a JSON pointer, made (objects along the way) when missing.
fn slot_at<'a>(doc: &'a mut J, ptr: &str) -> &'a mut J {
    let mut cur = doc;
    for part in ptr.split('/').skip(1) {
        if !cur.is_object() {
            *cur = J::Object(serde_json::Map::new());
        }
        cur = cur.as_object_mut().map(|o| o.entry(part.to_string()).or_insert(J::Null)).expect("an object");
    }
    cur
}
fn jf(doc: &J, ptr: &str, d: f64) -> f64 {
    doc.pointer(ptr).and_then(J::as_f64).unwrap_or(d)
}
fn jdrag(doc: &mut J, label: &str, ptr: &str, d: f64, speed: f32, lo: f32, hi: f32, fmt: &str) {
    let mut v = jf(doc, ptr, d) as f32;
    if super::dev_windows::drag(label, &mut v, speed, lo, hi, fmt) {
        *slot_at(doc, ptr) = J::from(f64::from(v));
    }
}
fn jint(doc: &mut J, label: &str, ptr: &str, d: i64, lo: i32, hi: i32) {
    let mut v = doc.pointer(ptr).and_then(J::as_i64).unwrap_or(d) as i32;
    if super::dev_windows::drag_int(label, &mut v, lo, hi) {
        *slot_at(doc, ptr) = J::from(v);
    }
}
/// CFG.k = ImGui.Checkbox(label, CFG.k): Lua's truth (nil and false are false).
fn jcheck(doc: &mut J, label: &str, ptr: &str) {
    let mut b = !matches!(doc.pointer(ptr), None | Some(J::Null) | Some(J::Bool(false)));
    if super::dev_windows::checkbox(label, &mut b) {
        *slot_at(doc, ptr) = J::Bool(b);
    }
}
fn opt(v: Option<f64>, digits: usize) -> String {
    v.map_or("-".into(), |x| format!("{x:.digits$}"))
}
/// A per-kind copy of a shared table, seeded from it the first time (CFG.grenade_hold_kind[kind] and its kin).
fn per_kind(doc: &mut J, kind: Option<&str>, table: &str, shared: &str, keys: &[&str]) -> String {
    let Some(kind) = kind else { return format!("/{shared}") };
    let ptr = format!("/{table}/{kind}");
    if doc.pointer(&ptr).map_or(true, J::is_null) {
        let mut seed = serde_json::Map::new();
        for k in keys {
            seed.insert((*k).into(), J::from(jf(doc, &format!("/{shared}/{k}"), 0.0)));
        }
        *slot_at(doc, &ptr) = J::Object(seed);
    }
    ptr
}
/// The mod's "VR tactical belt" window: its controls over the config, its read-outs of the belt's state.
pub(in crate::mechanics) fn dev_window(frame: [f32; 2]) {
    use super::dev_windows as w;
    let mut win = WINDOW.lock().unwrap_or_else(|p| p.into_inner());
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    w::window("VR tactical belt", [0.21, 0.04], [0.24, 0.91], frame, || {
        let Some(s) = guard.as_mut() else {
            w::text("waiting for the player");
            return;
        };
        let win = &mut *win;
        let doc = win.doc.get_or_insert_with(|| s.cfg.0.clone());
        let mut save_now = false;
        w::text("runs in Rust (mechanics/belt.rs): settings apply when the overlay closes");
        jcheck(doc, "enabled", "/enabled");
        jcheck(doc, "keep it equipped", "/auto_equip");
        w::separator();
        let slot = s.slot_memo.get(&s.cfg.variant()).map_or_else(|| s.cfg.s("slot").to_string(), |v| v.1.clone());
        w::text(&format!("slot: {slot}"));
        w::text(&format!("belt object:   {}", if s.belt_hash.is_some() { "yes" } else { "no" }));
        w::text(&format!("render plane param: {}", doc.get("render_plane_param").map(|v| v.to_string()).unwrap_or_default()));
        w::text(&format!("grenade: {}   weapon: {}   in hand: {}   SceneTier: {}", s.state.grenade, s.state.weapon, s.state.inhand, s.state.tier));
        w::separator();
        w::text("colour");
        let variants: Vec<String> = doc.get("variants").and_then(J::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
        let names: Vec<String> = doc.get("variant_names").and_then(J::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
        let current = doc.get("variant").and_then(J::as_i64).unwrap_or(1);
        for (i, v) in variants.iter().enumerate() {
            let label = names.get(i).unwrap_or(v);
            if w::radio(&format!("{label}##colour{i}"), current == i as i64 + 1) {
                *slot_at(doc, "/variant") = J::from(i as i64 + 1);
                save_now = true;
            }
            if i + 1 < variants.len() {
                w::same_line();
            }
        }
        w::separator();
        w::text("elements");
        let mut comps: Vec<String> = s.comps.keys().cloned().collect();
        comps.sort();
        if comps.is_empty() {
            w::text("  no components yet -- the belt is not on");
        }
        for nm in &comps {
            let worn = s.dressed.get(nm).cloned();
            // the element worn there (its rule is its own), else the component's rule
            let el_index = doc.get("elements").and_then(J::as_array).and_then(|a| {
                a.iter().position(|el| el.get("host").and_then(J::as_str) == Some(nm.as_str()) && worn.as_deref() == el.get("name").and_then(J::as_str))
            });
            let rule = match el_index {
                Some(i) => format!("{} (el)", doc.pointer(&format!("/elements/{i}/rule")).and_then(J::as_str).unwrap_or("always")),
                None => doc.pointer(&format!("/rules/{nm}")).and_then(J::as_str).unwrap_or("always").to_string(),
            };
            w::text(&format!("  {nm:<18} rule={rule:<8} {}", worn.map(|x| format!("[{x}]")).unwrap_or_default()));
            for (k, r) in ["always", "never", "grenade", "weapon", "inhand"].iter().enumerate() {
                if k > 0 {
                    w::same_line();
                }
                if w::small(&format!("{r}##{nm}")) {
                    let ptr = match el_index {
                        Some(i) => format!("/elements/{i}/rule"),
                        None => format!("/rules/{nm}"),
                    };
                    *slot_at(doc, &ptr) = J::from(*r);
                }
            }
        }
        w::separator();
        jcheck(doc, "take the grenade off the belt on grip", "/grenade_grab");
        let mut plane = doc.get("grenade_grab_plane").and_then(J::as_i64).unwrap_or(0) as i32;
        if w::slider_int("plane: 0 follow / 1 scene / 2 weapon##gg", &mut plane, 0, 2) {
            *slot_at(doc, "/grenade_grab_plane") = J::from(plane);
        }
        jcheck(doc, "recorded grip: the lever under the thumb", "/grenade_grab_pose");
        jcheck(doc, "trace every grab step to grab_trace.log", "/grab_trace");
        w::text(&format!(
            "   in hand: {}  slot {}",
            s.grab.host.as_deref().unwrap_or("-"),
            s.grab.slot.as_deref().unwrap_or("-")
        ));
        if w::header("как граната сидит в руке") {
            let kind = s.grab.kind().map(str::to_string);
            let hold = per_kind(doc, kind.as_deref(), "grenade_hold_kind", "grenade_hold", &["x", "y", "z", "rx", "ry", "rz"]);
            w::text(&format!("   правится: {}", kind.as_deref().unwrap_or("общая (ничего в руке)")));
            if w::button("в правую руку") {
                win.actions.push(Action::Pin(1));
            }
            w::same_line();
            if w::button("в левую") {
                win.actions.push(Action::Pin(0));
            }
            w::same_line();
            if w::button("убрать") {
                win.actions.push(Action::Unpin);
            }
            w::text(&format!("   закреплена: {}", if s.grab.is_pinned() { "да" } else { "нет" }));
            jcheck(doc, "чека выдёргивается свободной рукой", "/grenade_pin");
            jdrag(doc, "preview, м##pin", "/grenade_pin_preview_m", 0.15, 0.005, 0.05, 0.50, "%.3f");
            jdrag(doc, "захват, м##pin", "/grenade_pin_grip_m", 0.08, 0.005, 0.02, 0.40, "%.3f");
            jdrag(doc, "выход чеки, м##pin", "/grenade_pin_slide_m", 0.01, 0.001, 0.000, 0.05, "%.4f");
            let axis = per_kind(doc, kind.as_deref(), "grenade_pin_axis_kind", "grenade_pin_axis", &["x", "y", "z"]);
            w::text(&format!("   направление выхода (куда едет чека) -- {}", kind.as_deref().unwrap_or("по умолчанию")));
            for c in ["x", "y", "z"] {
                jdrag(doc, &format!("выход {c}##pin"), &format!("{axis}/{c}"), 0.0, 0.01, -1.0, 1.0, "%.3f");
            }
            w::separator();
            jcheck(doc, "триггер прожимает рычаг", "/grenade_lever");
            jcheck(doc, "не стрелять, пока граната в руке", "/grenade_trigger_block");
            jdrag(doc, "скорость прожатия##lev", "/grenade_lever_speed", 8.0, 0.5, 1.0, 30.0, "%.1f");
            jdrag(doc, "раскрытие рычага##lev", "/grenade_lever_open", -1.0, 0.05, -2.0, 0.0, "%.2f");
            w::separator();
            jcheck(doc, "бросок: отпустить грип с выдернутой чекой", "/grenade_throw");
            w::text("   1.0 = ровно как кисть,  3.5 = как кидает игра (19-25 м/с)");
            jdrag(doc, "сила броска##thr", "/grenade_throw_gain", 1.0, 0.05, 0.2, 5.0, "%.2f");
            jdrag(doc, "мин, м/с##thr", "/grenade_throw_min", 0.0, 0.1, 0.0, 10.0, "%.1f");
            jdrag(doc, "макс, м/с##thr", "/grenade_throw_max", 30.0, 0.5, 5.0, 60.0, "%.1f");
            jdrag(doc, "окно пика, с##thr", "/grenade_throw_window", 0.2, 0.01, 0.05, 0.60, "%.2f");
            jcheck(doc, "списывать гранату из инвентаря", "/grenade_throw_consume");
            w::text(&format!("   гранат в инвентаре: {}", s.state.gcount.map_or("-".into(), |c| c.to_string())));
            w::separator();
            jcheck(doc, "звуки гранаты", "/grenade_sound");
            for k in ["grab", "pin", "lever", "leverOff", "throw", "stow"] {
                let key = format!("/grenade_snd_{k}");
                let mut name = doc.pointer(&key).and_then(J::as_str).unwrap_or("").to_string();
                if w::input(&format!("{k}##snd"), &mut name, 64) {
                    *slot_at(doc, &key) = J::from(name.clone());
                }
                w::same_line();
                if w::small(&format!("play##{k}")) {
                    win.actions.push(Action::Play(name));
                }
            }
            jdrag(doc, "рывок сверх упора, м##pin", "/grenade_pin_pull_m", 0.05, 0.005, 0.01, 0.30, "%.3f");
            jdrag(doc, "рывок, м/с##pin", "/grenade_pin_speed", 0.5, 0.05, 0.0, 6.0, "%.2f");
            let off = per_kind(doc, kind.as_deref(), "grenade_pin_off_kind", "grenade_pin_off", &["x", "y", "z"]);
            w::text(&format!("   точка хвата (где встаёт рука) -- {}", kind.as_deref().unwrap_or("по умолчанию")));
            for c in ["x", "y", "z"] {
                jdrag(doc, &format!("хват {c}##pin"), &format!("{off}/{c}"), 0.0, 0.002, -0.10, 0.10, "%.4f");
            }
            let line = |doc: &J, name: &str, off: &str, axis: &str| {
                format!(
                    "pin {name}  хват {:.4} {:.4} {:.4}   выход {:.3} {:.3} {:.3}",
                    jf(doc, &format!("{off}/x"), 0.0),
                    jf(doc, &format!("{off}/y"), 0.0),
                    jf(doc, &format!("{off}/z"), 0.0),
                    jf(doc, &format!("{axis}/x"), 0.0),
                    jf(doc, &format!("{axis}/y"), 0.0),
                    jf(doc, &format!("{axis}/z"), 0.0)
                )
            };
            if w::small("в лог##pin") {
                log!("[belt] {} (from Rust)\n", line(doc, kind.as_deref().unwrap_or("default"), &off, &axis));
            }
            w::same_line();
            if w::small("все в лог##pin") {
                let kinds: Vec<String> = doc.get("grenade_pin_off_kind").and_then(J::as_object).map(|o| o.keys().cloned().collect()).unwrap_or_default();
                for k in kinds {
                    let a = if doc.pointer(&format!("/grenade_pin_axis_kind/{k}")).is_some() { format!("/grenade_pin_axis_kind/{k}") } else { "/grenade_pin_axis".into() };
                    log!("[belt] {} (from Rust)\n", line(doc, &k, &format!("/grenade_pin_off_kind/{k}"), &a));
                }
            }
            for (c, label, speed, lo, hi, fmt) in [
                ("x", "x##gh", 0.002, -0.4, 0.4, "%.4f"),
                ("y", "y##gh", 0.002, -0.4, 0.4, "%.4f"),
                ("z", "z##gh", 0.002, -0.4, 0.4, "%.4f"),
                ("rx", "roll X##gh", 0.5, -180.0, 180.0, "%.1f"),
                ("ry", "pitch Y##gh", 0.5, -180.0, 180.0, "%.1f"),
                ("rz", "yaw Z##gh", 0.5, -180.0, 180.0, "%.1f"),
            ] {
                jdrag(doc, label, &format!("{hold}/{c}"), 0.0, speed, lo, hi, fmt);
            }
            if w::button("0##gh") {
                for c in ["x", "y", "z", "rx", "ry", "rz"] {
                    *slot_at(doc, &format!("{hold}/{c}")) = J::from(0.0);
                }
            }
            w::same_line();
            if w::button("в лог##gh") {
                let g = |c: &str| jf(doc, &format!("{hold}/{c}"), 0.0);
                log!("[belt] grenade_hold  {:.4} {:.4} {:.4}   {:.1} {:.1} {:.1} (from Rust)\n", g("x"), g("y"), g("z"), g("rx"), g("ry"), g("rz"));
            }
        }
        jcheck(doc, "light the element the hand reaches for", "/highlight");
        jcheck(doc, "PROBE: re-assert engine outline on nearby devices", "/probe_devices");
        w::text(&format!(
            "nearest: {} / {}   radius {:.2}",
            s.lit_hand[0].as_deref().unwrap_or("-"),
            s.lit_hand[1].as_deref().unwrap_or("-"),
            jf(doc, "/highlight_radius_m", 0.17)
        ));
        jdrag(doc, "radius##hl", "/highlight_radius_m", 0.17, 0.005, 0.03, 0.40, "%.3f");
        jdrag(doc, "x crouched##hl", "/crouch_reach_scale", 1.5, 0.05, 1.0, 3.0, "%.2f");
        w::text("точка на руке: костяшка среднего пальца + смещение в осях ладони (м)");
        jdrag(doc, "поперёк ладони##hoff", "/highlight_hand_off/x", 0.0, 0.002, -0.20, 0.20, "%.3f");
        jdrag(doc, "вдоль пальцев##hoff", "/highlight_hand_off/y", 0.0, 0.002, -0.20, 0.20, "%.3f");
        jdrag(doc, "из ладони##hoff", "/highlight_hand_off/z", 0.0, 0.002, -0.20, 0.20, "%.3f");
        if w::small("0##hoff") {
            for c in ["x", "y", "z"] {
                *slot_at(doc, &format!("/highlight_hand_off/{c}")) = J::from(0.0);
            }
        }
        w::separator();
        w::text("element offsets (metres, entity space: X across, Y fore-aft, Z up)");
        let elements = doc.get("elements").and_then(J::as_array).map_or(0, Vec::len);
        for i in 0..elements {
            let name = doc.pointer(&format!("/elements/{i}/name")).and_then(J::as_str).unwrap_or("").to_string();
            let host = doc.pointer(&format!("/elements/{i}/host")).and_then(J::as_str).unwrap_or("").to_string();
            w::text(&format!("{name} on {host}"));
            for c in ["ox", "oy", "oz"] {
                let axis = c[1..].to_uppercase();
                jdrag(doc, &format!("{axis}##{name}"), &format!("/elements/{i}/{c}"), 0.0, 0.002, -0.5, 0.5, "%.4f");
            }
            if w::small(&format!("zero##{name}")) {
                for c in ["ox", "oy", "oz"] {
                    *slot_at(doc, &format!("/elements/{i}/{c}")) = J::from(0.0);
                }
            }
            w::same_line();
            if w::small(&format!("save##{name}")) {
                save_now = true;
                let g = |c: &str| jf(doc, &format!("/elements/{i}/{c}"), 0.0);
                log!("[belt] {name} offset saved: {:.4} {:.4} {:.4} (from Rust)\n", g("ox"), g("oy"), g("oz"));
            }
        }
        w::separator();
        let fem = s.body_tag == Some("wa");
        w::text(if fem { "тело: женское (i1_016_wa_belt__police_eq)" } else { "тело: мужское (куски i1_016_ma_belt__police*)" });
        let aio_key = if fem { "/belt_all_in_one_female" } else { "/belt_all_in_one" };
        let mut aio = doc.pointer(aio_key) != Some(&J::Bool(false));
        if w::checkbox("пояс целиком (с подсумками)", &mut aio) {
            *slot_at(doc, aio_key) = J::Bool(aio);
        }
        w::text("отдельные куски (галка = показать)");
        let pieces = if fem { "/belt_pieces_female" } else { "/belt_pieces" };
        if !doc.pointer(pieces).is_some_and(J::is_object) {
            *slot_at(doc, pieces) = J::Object(serde_json::Map::new());
        }
        let mut shown = 0;
        for (i, k) in BELT_PIECES.iter().enumerate() {
            if i % 3 != 0 {
                w::same_line();
            }
            let ptr = format!("{pieces}/{k}");
            let mut on = doc.pointer(&ptr) == Some(&J::Bool(true));
            if w::checkbox(&format!("{k}##piece"), &mut on) {
                *slot_at(doc, &ptr) = J::Bool(on);
            }
            shown += usize::from(on);
        }
        w::text(&format!("   включено кусков: {shown}   компонентов найдено: {}", s.belt_parts.as_ref().map_or(0, HashMap::len)));
        if w::small("голый ремень##piece") {
            for k in BELT_PIECES {
                *slot_at(doc, &format!("{pieces}/{k}")) = J::Bool(false);
            }
            if fem {
                *slot_at(doc, "/belt_all_in_one_female") = J::Bool(true);
            } else {
                *slot_at(doc, "/belt_all_in_one") = J::Bool(false);
                *slot_at(doc, &format!("{pieces}/strap")) = J::Bool(true);
                *slot_at(doc, &format!("{pieces}/back")) = J::Bool(true);
            }
        }
        w::same_line();
        if w::small("вернуть как было##piece") {
            for k in BELT_PIECES {
                *slot_at(doc, &format!("{pieces}/{k}")) = J::Bool(false);
            }
            *slot_at(doc, aio_key) = J::Bool(true);
        }
        w::same_line();
        if w::small("в лог##piece") {
            let on: Vec<&str> = BELT_PIECES.iter().copied().filter(|k| doc.pointer(&format!("{pieces}/{k}")) == Some(&J::Bool(true))).collect();
            log!("[belt] belt pieces: целиком={}, куски [{}] (from Rust)\n", doc.pointer("/belt_all_in_one") != Some(&J::Bool(false)), on.join(","));
        }
        w::separator();
        w::text("куски пояса (галка = спрятать)");
        let chunks = if fem { "/belt_chunk_off_female" } else { "/belt_chunk_off" };
        if !doc.pointer(chunks).is_some_and(J::is_array) {
            *slot_at(doc, chunks) = J::Array(Vec::new());
        }
        let mut hidden = [false; 32];
        if let Some(a) = doc.pointer(chunks).and_then(J::as_array) {
            for (i, v) in a.iter().take(32).enumerate() {
                hidden[i] = v == &J::Bool(true);
            }
        }
        let mut changed = false;
        for (i, h) in hidden.iter_mut().enumerate().take(16) {
            if i % 4 != 0 {
                w::same_line();
            }
            changed |= w::checkbox(&format!("{i}##chunk"), h);
        }
        let mask: u64 = (0..32).filter(|&i| !hidden[i]).map(|i| 1u64 << i).sum();
        w::text(&format!("   маска: {mask}"));
        if w::small("все видны##chunk") {
            hidden = [false; 32];
            changed = true;
        }
        w::same_line();
        if w::small("в лог##chunk") {
            let hid: Vec<String> = (0..32).filter(|&i| hidden[i]).map(|i| i.to_string()).collect();
            log!("[belt] belt chunks: маска {mask}, спрятаны [{}] (from Rust)\n", hid.join(","));
        }
        if changed {
            *slot_at(doc, chunks) = J::Array(hidden.iter().map(|&h| J::Bool(h)).collect());
        }
        w::separator();
        jcheck(doc, "ArchiveXL garment offsets", "/garment_offsets");
        let route = doc.get("color_route").and_then(J::as_str).unwrap_or("item").to_string();
        if w::small(&format!("colour route: {route}")) {
            *slot_at(doc, "/color_route") = J::from(if route == "item" { "component" } else { "item" });
        }
        jcheck(doc, "hide in scenes", "/hide_in_scenes");
        jcheck(doc, "log every toggle", "/verbose");
        if w::button("equip now") {
            win.actions.push(Action::Equip);
        }
        w::separator();
        if w::header("belt props (outlineable elements)") {
            w::wrapped(
                "An element that can be outlined has to be an ITEM in a slot, not a mesh on the belt: a mesh there is a component on the player and the engine outline never reaches a component. Sliders are metres, in the item's own frame -- up, back, right.",
            );
            let props = doc.get("props").and_then(J::as_array).map_or(0, Vec::len);
            for i in 0..props {
                let p = format!("/props/{i}");
                let slot = doc.pointer(&format!("{p}/slot")).and_then(J::as_str).unwrap_or("").replace("AttachmentSlots.", "");
                let follow = doc.pointer(&format!("{p}/follow")).and_then(J::as_str).map(|f| format!("  <- {f}")).unwrap_or_default();
                jcheck(doc, &format!("{slot}{follow}##prop{i}"), &format!("{p}/on"));
                w::same_line();
                jcheck(doc, &format!("hand lights it##prop{i}"), &format!("{p}/auto"));
                w::same_line();
                jcheck(doc, &format!("always lit##prop{i}"), &format!("{p}/lit"));
                if doc.pointer(&format!("{p}/on")) == Some(&J::Bool(true)) {
                    if let Some(ps) = s.props.get(i) {
                        w::text(&format!("   hand: {}   outline: {}", opt(ps.dist, 3), if ps.lit == Some(true) { "ON" } else { "off" }));
                    }
                    for (c, speed, lo, hi, fmt) in [("up", 0.002, -0.60, 0.60, "%.3f"), ("back", 0.002, -0.60, 0.60, "%.3f"), ("right", 0.002, -0.60, 0.60, "%.3f")] {
                        jdrag(doc, &format!("{c}##{i}"), &format!("{p}/{c}"), 0.0, speed, lo, hi, fmt);
                    }
                    for (c, label) in [("rx", "turn X (roll)"), ("ry", "turn Y (pitch)"), ("rz", "turn Z (yaw)")] {
                        jdrag(doc, &format!("{label}##{i}"), &format!("{p}/{c}"), 0.0, 0.5, -180.0, 180.0, "%.1f");
                    }
                    let q = euler_quat(jf(doc, &format!("{p}/rx"), 0.0), jf(doc, &format!("{p}/ry"), 0.0), jf(doc, &format!("{p}/rz"), 0.0));
                    w::text(&format!("quat  i {:.5}  j {:.5}  k {:.5}  r {:.5}", q[0], q[1], q[2], q[3]));
                    if w::small(&format!("zero pos##{i}")) {
                        for c in ["up", "back", "right"] {
                            *slot_at(doc, &format!("{p}/{c}")) = J::from(0.0);
                        }
                    }
                    w::same_line();
                    if w::small(&format!("zero turn##{i}")) {
                        for c in ["rx", "ry", "rz"] {
                            *slot_at(doc, &format!("{p}/{c}")) = J::from(0.0);
                        }
                    }
                }
                w::separator();
            }
            w::text(&format!("plane: player={}", s.plane_val.map_or("nil".into(), |v| v.to_string())));
            if w::header("положение элементов пояса") {
                let mut names: Vec<String> = s.comps.keys().filter(|nm| !nm.ends_with("_lit") && offset_group(nm).is_none()).cloned().collect();
                names.sort();
                // what is worn first (the belt's dressed hosts), then the rest
                let (mut live, rest): (Vec<String>, Vec<String>) = names.into_iter().partition(|nm| s.dressed.contains_key(nm));
                let live_count = live.len();
                live.extend(rest);
                let names = live;
                let count = |p: &str| names.iter().filter(|x| x.starts_with(p)).count();
                w::text(&format!(
                    "в списке {}: гранат {}, магазинов {}, патронов {}, на теле {live_count}",
                    names.len(),
                    count("ucbeltgrenade"),
                    count("ucbeltmag_"),
                    count("ucbeltammo_")
                ));
                w::input("фильтр##off", &mut win.filter, 64);
                for (label, f) in [("всё##offfilter", "ucbelt"), ("на теле##offfilter", ""), ("маг##offfilter", "ucbeltmag_"), ("гран##offfilter", "ucbeltgrenade"), ("патр##offfilter", "ucbeltammo_")] {
                    if f != "ucbelt" {
                        w::same_line();
                    }
                    if w::small(label) {
                        win.filter = f.to_string();
                    }
                }
                w::text(&format!("фильтр = [{}]", win.filter));
                if w::small("в лог##offlist") {
                    log!("[belt] offsets list: {} (from Rust)\n", names.join(", "));
                }
                let table = if fem && doc.get("offsets_female").is_some_and(J::is_object) { "/offsets_female" } else { "/offsets" };
                for (i, nm) in names.iter().enumerate() {
                    let open = if win.filter.is_empty() { i < live_count } else { nm.contains(win.filter.as_str()) };
                    if !open {
                        continue;
                    }
                    let o = format!("{table}/{nm}");
                    let tuned = ["x", "y", "z", "rx", "ry", "rz"].iter().map(|c| jf(doc, &format!("{o}/{c}"), 0.0).abs()).sum::<f64>() > 1e-6;
                    w::text(&format!("{nm}{}{}", if i < live_count { "  <- на теле" } else { "" }, if tuned { "  *" } else { "" }));
                    for (c, label, speed, lo, hi, fmt) in [
                        ("x", "x", 0.002, -0.6, 0.6, "%.4f"),
                        ("y", "y", 0.002, -0.6, 0.6, "%.4f"),
                        ("z", "z", 0.002, -0.6, 0.6, "%.4f"),
                        ("rx", "roll X", 0.5, -180.0, 180.0, "%.1f"),
                        ("ry", "pitch Y", 0.5, -180.0, 180.0, "%.1f"),
                        ("rz", "yaw Z", 0.5, -180.0, 180.0, "%.1f"),
                    ] {
                        jdrag(doc, &format!("{label}##{nm}"), &format!("{o}/{c}"), 0.0, speed, lo, hi, fmt);
                    }
                    if w::button(&format!("0##{nm}")) {
                        for c in ["x", "y", "z", "rx", "ry", "rz"] {
                            *slot_at(doc, &format!("{o}/{c}")) = J::from(0.0);
                        }
                    }
                    w::separator();
                }
                if w::button("записать offsets.json") {
                    let n = write_offset_file("offsets.json", doc.get("offsets").unwrap_or(&J::Null));
                    let nf = write_offset_file("offsets_female.json", doc.get("offsets_female").unwrap_or(&J::Null));
                    log!("[belt] offsets written ({n} male, {nf} female) (from Rust)\n");
                }
                w::same_line();
                if w::button("напечатать всё в лог") {
                    for nm in &names {
                        let g = |c: &str| jf(doc, &format!("/offsets/{nm}/{c}"), 0.0);
                        let q = euler_quat(g("rx"), g("ry"), g("rz"));
                        log!(
                            "[belt] offset {nm:<20} pos {:.4} {:.4} {:.4}  deg {:.1} {:.1} {:.1}  quat {:.6} {:.6} {:.6} {:.6} (from Rust)\n",
                            g("x"),
                            g("y"),
                            g("z"),
                            g("rx"),
                            g("ry"),
                            g("rz"),
                            q[0],
                            q[1],
                            q[2],
                            q[3]
                        );
                    }
                }
            }
            w::text("mark");
            jint(doc, "mark: 0 outline  1 material  2 both  3 light", "/prop_mark", 1, 0, 3);
            jint(doc, "colour: 1 magenta 2 white 3 cyan 4 green 5 amber 6 red 7 blue 8 violet", "/prop_glow_level", 1, 1, 8);
            jdrag(doc, "rim thickness (twin scale)", "/prop_rim_scale", 1.05, 0.002, 1.0, 1.3, "%.3f");
            jdrag(doc, "glow, lumens", "/prop_glow_lm", 60.0, 1.0, 0.0, 600.0, "%.0f");
            jdrag(doc, "glow radius, m", "/prop_glow_r", 0.35, 0.01, 0.05, 3.0, "%.2f");
            w::separator();
            jdrag(doc, "light-up radius", "/prop_radius_m", 0.1, 0.002, 0.01, 0.50, "%.3f");
            jdrag(doc, "release factor", "/prop_release", 1.3, 0.01, 1.0, 2.5, "%.2f");
            jdrag(doc, "claim renew, s", "/prop_renew_s", 0.5, 0.01, 0.05, 2.0, "%.2f");
            jint(doc, "outline type (8 = white, ours)", "/prop_outline", 8, 0, 9);
            jint(doc, "context (1 = line only)", "/prop_context", 1, 0, 3);
            jint(doc, "highlight type", "/prop_type", 0, 0, 9);
            w::wrapped("Values are written to belt.cfg when the overlay closes, so a tuned position survives a restart.");
        }
        if save_now {
            write_cfg(doc);
        }
    });
}
/// The overlay closed: the window's config written (the tick reads it again), the copy dropped for the next open.
pub(in crate::mechanics) fn dev_window_closed() {
    if let Some(doc) = WINDOW.lock().unwrap_or_else(|p| p.into_inner()).doc.take() {
        write_cfg(&doc);
        log!("[belt] settings written to belt.cfg as the overlay closed (from Rust)\n");
    }
}
