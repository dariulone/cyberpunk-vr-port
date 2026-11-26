//! mods/cet/CyberpunkVRPort_Weapon's weapon frame (the mod's updateWeapon; the tracked combat -- arm cyberware, ordinary
//! melee swings, the physical throw -- stays the Lua host's for now), with a player, every frame:
//! - the hand-over (carryTick): right grip with the left grip held hands a long gun to the left hand (WeaponLeft, its
//!   slot entry set to where the gun was against PropLeft's frame, the item object and the weapon plane passed); right
//!   grip again within the plugin's reach takes it back; an empty left slot left displaced is reset (re-checked every
//!   2 s once clean). Not with two guns, an arm implant, a pistol, or left-handed;
//! - the aim instrument and the weapon-aim hooks installed once, 3 s in (InstallVRProvInstrument, InstallWeaponAimHook);
//! - the muzzle (SetVRMuzzleQuat, SetVRMuzzlePos) and where its line hits the static world (SetVRAimHit);
//! - the recoil profile per drawn weapon (SetVRWeaponName/Kick/Class: the family from the record and its friendlyName,
//!   the game's peak kick table, the class from itemType, melee by the game's own predicate) and the camera kick
//!   removed at the weapon (Multiplier 0 on the item's stats object, one set per object); the same for the left gun
//!   (SetVRLeftWeaponKick), and the guns that may be drawn into the left hand prepared while holstered (every 0.5 s);
//! - handedness: a change of the setting puts the weapon away and draws it again into the other hand; a loaded save's
//!   weapon is restored into the hand the controls use (watched for 120 s after the attach: VRWeaponAttached);
//! - the draw sound (VREquipSound) on a new weapon entity, the camera zoom (SetVRZoomLevel);
//! - the blade guard: IsBlocking / IsDeflecting stat modifiers on the player while a melee blade is held outside the
//!   forward thrust cone (a parry window of 0.25 s on entry); taken off otherwise and at the shutdown.
use crate::script_call::{call_handle, call_native, invoke, invoke_handle, method, static_function, Value};
use cvr_host::log;
use red4ext_abi::cname::CName;
use red4ext_abi::handle::Handle;
use red4ext_abi::tweakdbid::from_name as tdbid;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

const GUARD_THRUST_DOT: f64 = 0.50;
const GUARD_PARRY_WINDOW: f64 = 0.25;
const CAMERA_KICK_SCALE: f32 = 0.0;
const RECOIL_STATS: [&str; 6] = ["RecoilKickMin", "RecoilKickMax", "RecoilKickMinADS", "RecoilKickMaxADS", "RecoilAngle", "RecoilAngleADS"];
const LEFT_PARTNER_SLOT: i32 = 3;
const RESTORE_WATCH: f64 = 120.0;
const RESTORE_SETTLE: f64 = 1.5;
const REHAND_WAIT: f64 = 3.0;
const CARRY_BACK_DIST_NOTE: f64 = 0.18; // the plugin answers the reach (xr_carry_radius); kept for the record
const PLANE_WEAPON: u64 = 2;
/// The game's peak camera kick per weapon family (TweakDB's max RecoilKickMax across the family).
const RECOIL_KICK_BY_FAMILY: [(&str, f64); 48] = [
    ("achilles", 2.0), ("ajax", 1.3), ("ashura", 1.0), ("buck", 8.0), ("burya", 9.0), ("carnage", 12.0), ("chao", 0.4),
    ("copperhead", 0.9), ("cpo", 4.0), ("crusher", 5.6), ("defender", 0.466), ("dian", 0.24), ("grad", 8.0),
    ("guillotine", 0.65), ("hmg", 0.8), ("igla", 8.0), ("kappa", 0.24), ("kenshin", 0.8), ("kolac", 3.8), ("kyubi", 1.4),
    ("lexington", 1.0), ("liberty", 2.0), ("ma70", 0.6), ("masamune", 1.2), ("nekomata", 7.2), ("nova", 3.5),
    ("nue", 3.2), ("omaha", 2.8), ("overture", 4.0), ("palica", 5.6), ("pozhar", 5.4), ("pulsar", 0.85), ("quasar", 1.2),
    ("rocketlauncher", 15.0), ("saratoga", 0.6), ("satara", 9.6), ("senkoh", 0.8), ("shingen", 0.28),
    ("sidewinder", 0.56), ("silverhand", 1.9), ("slaughtomatic", 1.3), ("sor22", 3.5), ("tactician", 11.0),
    ("testera", 10.4), ("umbra", 1.05), ("unity", 2.25), ("yukimura", 0.7), ("zhuo", 2.8),
];
/// Felt recoil where the game's number came back wrong in the headset.
const RECOIL_KICK_OVERRIDE: [(&str, f64); 1] = [("unity", 0.87)];
/// Hold names for weapons the kick table does not name (the two-hand hold is kept per name).
const HOLD_NAMES: [&str; 29] = [
    "ticon", "tamayura", "osprey", "rasetsu", "hercules", "warden", "katana", "machete", "kukri", "knife", "tanto",
    "tomahawk", "chainsword", "axe", "baton", "sledgehammer", "hammer", "kanabo", "crowbar", "pipe_wrench", "iron_pipe",
    "tire_iron", "bat", "wrench", "pipe", "mantis", "monowire", "gorilla", "projectilelauncher",
];

/// The player attached (VRWeaponAttached): the loaded save's weapon is watched for.
static ATTACHED: AtomicBool = AtomicBool::new(false);

#[derive(Default)]
struct State {
    installed: bool,
    install_timer: f64,
    recoil_killed: Option<u64>,
    kick_mods: HashMap<u64, Vec<Handle>>,
    last_class: i32,
    left_killed: Option<u64>,
    left_prep_clock: f64,
    equip_init: bool,
    equip_id: Option<u64>,
    guard_clock: f64,
    guard_parry_until: f64,
    guard_was_on: bool,
    guard_block: Handle,
    guard_parry: Handle,
    melee_for: Option<u64>,
    melee_was: bool,
    kind_for: Option<u64>,
    kind: i32,
    handed_was: Option<bool>,
    rehand_left: Option<f64>,
    restore_watch: Option<f64>,
    restore_held: f64,
    carry_on: bool,
    carry_item: Option<[u64; 2]>,
    carry_rgrip_was: bool,
    left_clean: bool,
    left_checked_at: f64,
    clock: f64,
    reported_muzzle: bool,
    reported_hit: bool,
    /// The weapon in hand's one-handed and supported spring accents (hand_kick.rs), the hand-pose smoothing the accent
    /// blends over, and the blend toward the supported one (0 one hand, 1 two).
    active_solo: Option<f64>,
    active_supported: Option<f64>,
    support_time: f64,
    support_blend: f64,
    /// The smart-gun answer for the drawn weapon entity (only an answer is remembered: a weapon just drawn can have no
    /// record yet).
    smart_key: Option<u64>,
    smart_is: bool,
}
// SAFETY: the handles are made, used and dropped on the game's main thread, where the mechanics tick.
unsafe impl Send for State {}
static STATE: Mutex<Option<State>> = Mutex::new(None);

type R<T> = Result<T, String>;
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
fn stat_static<T: crate::script_call::Ret>(class: &str, name: &str, values: &[Value]) -> R<T> {
    let f = static_function(class, name).ok_or_else(|| format!("no {class}.{name}"))?;
    // SAFETY: a static function (no context).
    unsafe { invoke::<T>(core::ptr::null_mut(), f, values) }.ok_or_else(|| format!("{class}.{name} failed"))
}
fn static_handle(class: &str, name: &str, values: &[Value]) -> Handle {
    // SAFETY: a static function (no context).
    static_function(class, name).and_then(|f| unsafe { invoke_handle(core::ptr::null_mut(), f, values) }).unwrap_or_else(Handle::null)
}
fn shared(i: i32) -> f32 {
    // SAFETY: the shared block's read.
    unsafe { cvr_rig::vrik::provider::shared_slot(i) }
}
fn player() -> Handle {
    let mut p = Handle::null();
    // SAFETY: GetPlayer(GameInstance) into a handle of ours.
    unsafe { crate::anim_lookup::get_player(&mut p) };
    p
}
fn active_weapon(p: &Handle) -> Handle {
    static_handle("ScriptedPuppet", "GetActiveWeapon", &[Value::Handle(p)])
}
fn entity_hash(o: &Handle) -> Option<u64> {
    m::<u64>(o, "GetEntityID", &[]).ok()
}
fn enum_v(name: &str, member: &str) -> u64 {
    crate::script_call::enum_value(name, member).unwrap_or(0) as u64
}
/// A TweakDB flat's 8-byte value (a CName, a TweakDBID) by its name.
pub(super) fn flat_u64(name: &str) -> Option<u64> {
    let db = red4ext_abi::tweakdb::get();
    if db.is_null() {
        return None;
    }
    // SAFETY: the database's own flat lookup: a FlatValue object, its value through its own GetValue (8 bytes: a
    // CName, a TweakDBID, a ResRef).
    unsafe {
        let flat = red4ext_abi::tweakdb::flat_value(db, tdbid(name));
        if flat.is_null() {
            return None;
        }
        let v = red4ext_abi::tweakdb::flat_stack_value(flat);
        (!v.value.is_null()).then(|| *(v.value as *const u64))
    }
}
/// A record's name: one the plugin built an id from (tdb_names.rs: the port's own records), else TDBID.ToStringDEBUG
/// (the plugin's names, or Cyber Engine Tweaks' when it is installed), when the game knows it.
pub(super) fn tdbid_name(id: u64) -> Option<String> {
    if let Some(name) = crate::tdb_names::name_of(id) {
        return Some(name);
    }
    let why = |what: &'static str| {
        crate::audit::once(crate::audit::key(&[0x5444, CName::hashed(what).0]), || format!("TDBID.ToStringDEBUG: {what}"));
    };
    let Some(f) = static_function("TDBID", "ToStringDEBUG") else {
        why("no such static");
        return None;
    };
    // SAFETY: a static returning a String, kept while read.
    let Some(s) = (unsafe { crate::script_call::invoke_owned(core::ptr::null_mut(), f, &[Value::U64(id)]) }) else {
        why("the call did not run");
        return None;
    };
    // SAFETY: the String result.
    let text = unsafe { (*(s.as_ptr() as *const red4ext_abi::cstring::CString)).as_str().to_string() };
    if text.is_empty() {
        why("an empty name");
    }
    (!text.is_empty()).then_some(text)
}
fn log_probe(line: &str) {
    use std::io::Write;
    let path = cvr_host::paths::file(cvr_host::paths::Folder::Lua, "CyberpunkVRPort_Weapon\\recoil_probe.txt");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{line}");
    }
}

/// The weapon's recoil profile off its record: the record key, the family, the kick and the class.
/// The recoil profile: (the record key, the family, the spring kick, the class, the item type, the VR presentation).
type Profile = (Option<String>, Option<&'static str>, f64, i32, Option<String>, Option<cvr_body::hand_kick::HandKick>);
fn recoil_profile(wpn: &Handle) -> Profile {
    let item = m::<[u64; 2]>(wpn, "GetItemID", &[]).ok();
    let key = item.and_then(|i| tdbid_name(i[0]));
    let fname = key.as_ref().and_then(|k| flat_u64(&format!("{k}.friendlyName"))).map(|n| CName(n).text().to_string());
    let low = key.as_ref().map(|k| format!("{k} {}", fname.as_deref().unwrap_or("nil")).to_lowercase());
    let mut kick: Option<f64> = None;
    let mut fam: Option<&'static str> = None;
    if let Some(low) = &low {
        let mut best = 0;
        for &(f, v) in &RECOIL_KICK_BY_FAMILY {
            if f.len() > best && low.contains(f) {
                (kick, fam, best) = (Some(v), Some(f), f.len());
            }
        }
        if fam.is_none() {
            let mut best2 = 0;
            for w in HOLD_NAMES {
                if w.len() > best2 && low.contains(w) {
                    (fam, best2) = (Some(w), w.len());
                }
            }
        }
    }
    if let Some(f) = fam {
        if let Some(&(_, v)) = RECOIL_KICK_OVERRIDE.iter().find(|e| e.0 == f) {
            kick = Some(v);
        }
    }
    let kick = kick.filter(|k| *k > 0.0).unwrap_or(1.0);
    let mut cls = 0;
    let it_name = key.as_ref().and_then(|k| flat_u64(&format!("{k}.itemType"))).and_then(tdbid_name);
    if key.is_some() {
        let low = it_name.as_deref().unwrap_or("nil").to_lowercase();
        let any = |ts: &[&str]| ts.iter().any(|t| low.contains(t));
        cls = if low.contains("shotgun") {
            3
        } else if any(&["handgun", "revolver"]) {
            1
        } else if any(&["sniperrifle", "precisionrifle"]) {
            4
        } else if any(&["rifle", "machinegun", "submachine"]) {
            2
        } else if any(&["katana", "blade", "knife", "machete", "axe", "hammer", "club", "chainsword", "sword", "fists", "melee"]) {
            5
        } else {
            0
        };
    }
    if let Some(i) = item {
        if stat_static::<bool>("WeaponObject", "IsMelee", &[Value::Ptr(i.as_ptr().cast())]).unwrap_or(false) {
            cls = 5;
        }
    }
    // THE ORDINARY VR PRESENTATION (hand_kick.rs) takes precedence over the legacy family kick after the type lookup:
    // the audio family within the item type, a weapon with a trigger mode.
    let kick_profile = key.as_ref().and_then(|k| {
        let audio = flat_u64(&format!("{k}.audioName")).map(|n| CName(n).text().to_string());
        let mode = |property: &str| {
            flat_u64(&format!("{k}{property}")).and_then(tdbid_name).is_some_and(|n| {
                n.strip_prefix("TriggerMode.").is_some_and(|rest| !rest.is_empty() && rest.starts_with(|c: char| c.is_alphanumeric()))
            })
        };
        let modes = mode(".primaryTriggerMode") || mode(".secondaryTriggerMode");
        cvr_body::hand_kick::profile(k, it_name.as_deref().unwrap_or(""), audio.as_deref(), modes)
    });
    let kick = kick_profile.map_or(kick, |p| p.solo);
    (key, fam, kick, cls, it_name, kick_profile)
}
/// The supported accent blended in while a second hand holds the gun (GetVRTwoHandActive: the native side excludes
/// dual wielding and owns the handedness), over the reference's hand-pose smoothing.
fn update_support_accent(s: &mut State, dt: f64) {
    let (Some(solo), Some(supported)) = (s.active_solo, s.active_supported) else { return };
    let target = if native::<bool>("GetVRTwoHandActive", &[]) == Some(true) { 1.0 } else { 0.0 };
    if target == s.support_blend {
        return;
    }
    let time = if s.support_time > 0.0 { s.support_time } else { 0.03 };
    let mut next = target + (s.support_blend - target) * (-dt.max(0.0) / time).exp();
    if (next - target).abs() < 0.005 {
        next = target;
    }
    s.support_blend = next;
    native::<()>("SetVRWeaponKick", &[Value::Float((solo + (supported - solo) * next) as f32)]);
}
/// A smart gun (its record's evolution), asked once per drawn weapon entity.
fn is_smart_weapon(s: &mut State, wpn: &Handle) -> bool {
    let key = entity_hash(wpn);
    if key.is_none() || key != s.smart_key {
        let record = h(wpn, "GetWeaponRecord", &[]);
        if record.is_null() {
            return false;
        }
        let evolution = h(&record, "Evolution", &[]);
        let smart = if evolution.is_null() {
            false
        } else {
            let ty = super::lobby_rtti::num(&evolution, "Type", &[]);
            let Some(ty) = ty else { return false };
            ty == super::lobby_rtti::enum_value("gamedataWeaponEvolution", "Smart")
        };
        s.smart_key = key;
        s.smart_is = smart;
    }
    s.smart_is
}
/// The camera kick off one weapon: a Multiplier of CAMERA_KICK_SCALE on the item's stats object (else on `fallback`,
/// the gun entity), one set per object (the previous set taken off first).
fn zero_camera_kick_item(s: &mut State, p: &Handle, item: [u64; 2], fallback: Option<u64>, wid: Option<u64>) -> R<()> {
    let ts = super::game_system("GetTransactionSystem");
    let data = h(&ts, "GetItemData", &[Value::Handle(p), Value::Ptr(item.as_ptr().cast())]);
    let sid = m::<[u64; 2]>(&data, "GetStatsObjectID", &[]).ok().filter(|s| s[0] != 0);
    let target = match (sid, fallback) {
        (Some(s), _) => s,
        (None, Some(e)) => [e, 0],
        (None, None) => return Err("no stats object".into()),
    };
    let ss = super::game_system("GetStatsSystem");
    let key = sid.map(|s| s[0]).or(wid).unwrap_or(target[0]);
    for old in s.kick_mods.remove(&key).unwrap_or_default() {
        let _ = m::<bool>(&ss, "RemoveModifier", &[Value::Ptr(target.as_ptr().cast()), Value::Handle(&old)]);
    }
    let mut mods = Vec::new();
    let mult = enum_v("gameStatModifierType", "Multiplier");
    for name in RECOIL_STATS {
        let Some(st) = crate::script_call::enum_value("gamedataStatType", name) else { continue };
        let md = static_handle("RPGManager", "CreateStatModifier", &[Value::U64(st as u64), Value::U64(mult), Value::Float(CAMERA_KICK_SCALE)]);
        if md.is_null() {
            return Err("CreateStatModifier failed".into());
        }
        m::<bool>(&ss, "AddModifier", &[Value::Ptr(target.as_ptr().cast()), Value::Handle(&md)])?;
        mods.push(md);
    }
    s.kick_mods.insert(key, mods);
    Ok(())
}
fn kill_camera_recoil(s: &mut State, p: &Handle, wpn: &Handle, wid: Option<u64>) {
    if s.recoil_killed == wid {
        return;
    }
    let (key, fam, kick, cls, it_name, kick_profile) = recoil_profile(wpn);
    let supported = kick_profile.map(|p| p.supported);
    s.active_solo = Some(kick);
    s.active_supported = supported.filter(|v| *v != kick);
    s.support_time = kick_profile.map_or(0.0, |p| p.blend_time);
    s.support_blend = 0.0;
    let name = fam.map(str::to_string).or_else(|| key.as_ref().map(|k| k.to_lowercase())).unwrap_or_default();
    if let Some(text) = std::ffi::CString::new(name).ok().and_then(|c| red4ext_abi::cstring::CString::new(&c)) {
        native::<()>("SetVRWeaponName", &[Value::Ptr((&text as *const red4ext_abi::cstring::CString).cast())]);
    }
    native::<()>("SetVRWeaponKick", &[Value::Float(kick as f32)]);
    native::<()>("SetVRWeaponClass", &[Value::Int32(cls)]);
    s.last_class = cls;
    let (key_s, it_s) = (key.as_deref().unwrap_or("nil").to_string(), it_name.as_deref().unwrap_or("nil").to_string());
    log_probe(&format!("class={cls} type={it_s} key={key_s}"));
    cvr_host::vlog!("[CyberpunkVRPort_Weapon] recoil: itemType={it_s} class={cls} (from Rust)\n");
    cvr_host::vlog!(
        "[CyberpunkVRPort_Weapon] recoil: key={key_s} spring={kick} supported={} grip blend={}s manual tap scale={} reference={} (from Rust)\n",
        supported.map_or("nil".into(), |v| v.to_string()),
        kick_profile.map_or("nil".into(), |p| p.blend_time.to_string()),
        kick_profile.map_or("nil".into(), |p| p.tap_scale.to_string()),
        kick_profile.map_or("nil", |p| p.reference)
    );
    let wid_s = wid.map(|w| w.to_string()).unwrap_or_else(|| "nil".into());
    log_probe(&format!("key={key_s} kick={kick} wid={wid_s} cls={cls} type={it_s}"));
    let item = m::<[u64; 2]>(wpn, "GetItemID", &[]);
    match item.and_then(|i| zero_camera_kick_item(s, p, i, entity_hash(wpn), wid)) {
        Ok(()) => s.recoil_killed = wid,
        Err(e) => log!("[CyberpunkVRPort_Weapon] recoil: kill failed: {e}\n"),
    }
}
fn left_gun_frame(s: &mut State, p: &Handle, dt: f64, primary_wid: Option<u64>) -> R<()> {
    let ts = super::game_system("GetTransactionSystem");
    let gun = h(&ts, "GetItemInSlot", &[Value::Handle(p), Value::U64(tdbid("AttachmentSlots.WeaponLeft"))]);
    // The left gun's installed parts (attachment_recoil.rs): only a partner, never the primary seen through this slot.
    let gun_wid = if gun.is_null() { None } else { entity_hash(&gun) };
    let partner = (!gun.is_null() && gun_wid != primary_wid).then_some(&gun);
    super::attachment_recoil::update(0, p, partner, partner.and(gun_wid), dt);
    if gun.is_null() {
        if s.left_killed.take().is_some() {
            native::<()>("SetVRLeftWeaponKick", &[Value::Float(0.0), Value::Int32(0)]);
        }
        s.left_prep_clock += dt;
        if s.left_prep_clock >= 0.5 {
            s.left_prep_clock = 0.0;
            let data = static_handle("EquipmentSystem", "GetData", &[Value::Handle(p)]);
            let area = enum_v("gamedataEquipmentArea", "Weapon");
            let first = if native::<bool>("VRLeftHanded", &[]) == Some(true) { 0 } else { LEFT_PARTNER_SLOT };
            let last = LEFT_PARTNER_SLOT.min(m::<i32>(&data, "GetNumberOfSlots", &[Value::U64(area), Value::Bool(true)])? - 1);
            for slot in first..=last {
                let id = super::holster::equip_slot_item(&data, area, slot).ok_or("GetItemInEquipSlot failed")?;
                if stat_static::<bool>("ItemID", "IsValid", &[Value::Ptr(id.as_ptr().cast())]).unwrap_or(false) {
                    let _ = zero_camera_kick_item(s, p, id, None, None);
                }
            }
        }
        return Ok(());
    }
    s.left_prep_clock = 0.5;
    let wid = entity_hash(&gun);
    if wid == s.left_killed {
        return Ok(());
    }
    let (key, _, kick, cls, _, _) = recoil_profile(&gun);
    native::<()>("SetVRLeftWeaponKick", &[Value::Float(kick as f32), Value::Int32(cls)]);
    match m::<[u64; 2]>(&gun, "GetItemID", &[]).and_then(|i| zero_camera_kick_item(s, p, i, wid, wid)) {
        Ok(()) => s.left_killed = wid,
        Err(e) => log!("[CyberpunkVRPort_Weapon] recoil: left kill failed: {e}\n"),
    }
    log!("[CyberpunkVRPort_Weapon] recoil: left key={} kick={kick} class={cls} (from Rust)\n", key.as_deref().unwrap_or("nil"));
    Ok(())
}
/// The guard's two stat modifiers on the player, matching the wanted phase.
fn guard_stats(s: &mut State, p: &Handle, parry: bool, block: bool) {
    let ss = super::game_system("GetStatsSystem");
    let Some(id) = entity_hash(p) else { return };
    let target = [id, 0u64];
    let add = enum_v("gameStatModifierType", "Additive");
    for (want, slot, stat) in [(parry, &mut s.guard_parry, "IsDeflecting"), (block, &mut s.guard_block, "IsBlocking")] {
        if want && slot.is_null() {
            let md = static_handle("RPGManager", "CreateStatModifier", &[Value::U64(enum_v("gamedataStatType", stat)), Value::U64(add), Value::Float(1.0)]);
            if !md.is_null() && m::<bool>(&ss, "AddModifier", &[Value::Ptr(target.as_ptr().cast()), Value::Handle(&md)]).is_ok() {
                *slot = md;
            }
        } else if !want && !slot.is_null() {
            let _ = m::<bool>(&ss, "RemoveModifier", &[Value::Ptr(target.as_ptr().cast()), Value::Handle(slot)]);
            *slot = Handle::null();
        }
    }
}

#[repr(C, align(16))]
struct WorldTransform([u32; 8]);
#[repr(C, align(16))]
struct TraceResult([f32; 24]);
/// The muzzle's world position out of GetMuzzleSlotWorldTransform's result. On 2.5 its WorldPosition words hold the
/// plain float bits of the world coordinates (CET reads the same words as fixed point and gets -7590, 8783, 8418 for a
/// muzzle at -1433, 1264, 24), so they are read as floats; a genuine 17.15 fixed-point value shows as a float that is
/// huge or denormal, and is then read as fixed point.
fn muzzle_position(xf: &WorldTransform) -> [f32; 3] {
    muzzle_words_position(&xf.0)
}
/// [`muzzle_position`] for any copy of GetMuzzleSlotWorldTransform's eight words (the aim threat, the dual reload, the
/// shooting range read it too).
pub(super) fn muzzle_words_position(words: &[u32; 8]) -> [f32; 3] {
    let float = [0, 1, 2].map(|i| f32::from_bits(words[i]));
    let plausible = |v: f32| v.is_finite() && v.abs() < 100_000.0 && (v == 0.0 || v.abs() > 1e-20);
    if float.iter().all(|v| plausible(*v)) {
        float
    } else {
        [0, 1, 2].map(|i| words[i] as i32 as f32 / 131072.0)
    }
}
fn update_muzzle(s: &mut State, wpn: &Handle) {
    // SAFETY: the weapon, held.
    let Some(f) = (unsafe { crate::script_call::find_function(wpn.instance, "GetMuzzleSlotWorldTransform", 0) }) else { return };
    let mut xf = WorldTransform([0; 8]);
    // SAFETY: the weapon, held; the transform is a plain value written into our storage.
    if !unsafe { crate::script_call::invoke_into(wpn.instance, f, &[], (&mut xf as *mut WorldTransform).cast()) } {
        return;
    }
    let q = [f32::from_bits(xf.0[4]), f32::from_bits(xf.0[5]), f32::from_bits(xf.0[6]), f32::from_bits(xf.0[7])];
    native::<()>("SetVRMuzzleQuat", &[Value::Float(q[0]), Value::Float(q[1]), Value::Float(q[2]), Value::Float(q[3])]);
    // Smart guns lock targets in a cone around the targeting ray; the plugin points that ray down the barrel while this
    // reports a smart gun, refreshed every frame with the muzzle.
    let smart = is_smart_weapon(s, wpn);
    native::<()>("SetVRSmartWeapon", &[Value::Bool(smart)]);
    let pos = muzzle_position(&xf);
    native::<()>("SetVRMuzzlePos", &[Value::Float(pos[0]), Value::Float(pos[1]), Value::Float(pos[2])]);
    if !s.reported_muzzle {
        s.reported_muzzle = true;
        log!("[CyberpunkVRPort_Weapon] muzzlePos OK: ({:.4}, {:.4}, {:.4}) from Rust\n", pos[0], pos[1], pos[2]);
    }
    // Where that line hits the static world (the forward is the quaternion's +Y).
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    let fwd = [2.0 * (x * y - w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z + w * x)];
    let sq = super::game_system("GetSpatialQueriesSystem");
    if sq.is_null() {
        return;
    }
    let o = [pos[0], pos[1], pos[2], 1.0];
    let far = [pos[0] + fwd[0] * 200.0, pos[1] + fwd[1] * 200.0, pos[2] + fwd[2] * 200.0, 1.0];
    let mut trace = TraceResult([0.0; 24]);
    let hit = m::<bool>(
        &sq,
        "SyncRaycastByCollisionGroup",
        &[
            Value::Ptr(o.as_ptr().cast()),
            Value::Ptr(far.as_ptr().cast()),
            Value::U64(CName::hashed("Static").0),
            Value::Out((&mut trace as *mut TraceResult).cast()),
            Value::Bool(false),
            Value::Bool(false),
        ],
    );
    if hit == Ok(true) {
        let t = &trace.0;
        native::<()>("SetVRAimHit", &[Value::Float(t[0]), Value::Float(t[1]), Value::Float(t[2]), Value::Float(t[3]), Value::Float(t[4]), Value::Float(t[5])]);
        if !s.reported_hit {
            s.reported_hit = true;
            log!("[CyberpunkVRPort_Weapon] aimHit OK: ({:.3}, {:.3}, {:.3}) n=({:.2}, {:.2}, {:.2}) from Rust\n", t[0], t[1], t[2], t[3], t[4], t[5]);
        }
    }
}

/// The player's ItemAttachmentSlots entry whose slot name contains `name`: (the entry, the component held).
fn slot_entry(p: &Handle, name: &str) -> Option<(*mut u8, Handle)> {
    let c = h(p, "FindComponentByName", &[Value::U64(CName::hashed("ItemAttachmentSlots").0)]);
    if c.is_null() {
        return None;
    }
    // SAFETY: the component's `slots` array of entSlot (0x30 each: slotName 0x00, relativePosition 0x10,
    // relativeRotation 0x20).
    let arr = unsafe { crate::script_call::property(c.instance, "slots") }? as *const red4ext_abi::dynarray::DynArray<[u128; 3]>;
    let entries = unsafe { (*arr).as_slice() };
    let e = entries.iter().find(|e| CName(unsafe { *(*e as *const [u128; 3] as *const u64) }).text().contains(name))?;
    Some((e as *const [u128; 3] as *mut u8, c))
}
fn slot_get_rel(p: &Handle, name: &str) -> Option<[f32; 3]> {
    let (e, _held) = slot_entry(p, name)?;
    // SAFETY: the entry's relativePosition.
    Some(unsafe { *(e.add(0x10) as *const [f32; 3]) })
}
fn slot_set_rel(p: &Handle, name: &str, pos: [f32; 3], rot: [f32; 4]) -> bool {
    let Some((e, _held)) = slot_entry(p, name) else { return false };
    // SAFETY: the entry's relativePosition and relativeRotation, written in place when they differ.
    unsafe {
        let (pp, rp) = (e.add(0x10) as *mut [f32; 3], e.add(0x20) as *mut [f32; 4]);
        let same = (0..3).all(|i| ((*pp)[i] - pos[i]).abs() < 1e-5) && (0..4).all(|i| ((*rp)[i] - rot[i]).abs() < 1e-5);
        if !same {
            *pp = pos;
            *rp = rot;
        }
    }
    true
}
/// A slot's world frame (origin and basis) from four slot position providers.
fn slot_frame(p: &Handle, name: &str) -> Option<([f32; 3], [[f32; 3]; 3])> {
    let at = |x: f32, y: f32, z: f32| -> Option<[f32; 3]> {
        let off = [x, y, z];
        let pp = static_handle("IPositionProvider", "CreateSlotPositionProvider", &[Value::Handle(p), Value::U64(CName::hashed(name).0), Value::Ptr(off.as_ptr().cast())]);
        let mut out = [0.0f32; 4];
        (m::<bool>(&pp, "CalculatePosition", &[Value::Out(out.as_mut_ptr().cast())]) == Ok(true)).then_some([out[0], out[1], out[2]])
    };
    let (o, x, y, z) = (at(0.0, 0.0, 0.0)?, at(1.0, 0.0, 0.0)?, at(0.0, 1.0, 0.0)?, at(0.0, 0.0, 1.0)?);
    let d = |v: [f32; 3]| [v[0] - o[0], v[1] - o[1], v[2] - o[2]];
    Some((o, [d(x), d(y), d(z)]))
}
/// A quaternion from a world basis (columns X, Y, Z), Shepperd's method.
fn quat_from_basis(b: [[f32; 3]; 3]) -> [f32; 4] {
    let m = [[b[0][0], b[1][0], b[2][0]], [b[0][1], b[1][1], b[2][1]], [b[0][2], b[1][2], b[2][2]]];
    let tr = m[0][0] + m[1][1] + m[2][2];
    if tr > 0.0 {
        let s = (tr + 1.0).sqrt() * 2.0;
        [(m[2][1] - m[1][2]) / s, (m[0][2] - m[2][0]) / s, (m[1][0] - m[0][1]) / s, 0.25 * s]
    } else if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
        let s = (1.0 + m[0][0] - m[1][1] - m[2][2]).sqrt() * 2.0;
        [0.25 * s, (m[0][1] + m[1][0]) / s, (m[0][2] + m[2][0]) / s, (m[2][1] - m[1][2]) / s]
    } else if m[1][1] > m[2][2] {
        let s = (1.0 + m[1][1] - m[0][0] - m[2][2]).sqrt() * 2.0;
        [(m[0][1] + m[1][0]) / s, 0.25 * s, (m[1][2] + m[2][1]) / s, (m[0][2] - m[2][0]) / s]
    } else {
        let s = (1.0 + m[2][2] - m[0][0] - m[1][1]).sqrt() * 2.0;
        [(m[0][2] + m[2][0]) / s, (m[1][2] + m[2][1]) / s, 0.25 * s, (m[1][0] - m[0][1]) / s]
    }
}
fn cyber_kind(s: &mut State, _p: &Handle, wpn: &Handle) -> i32 {
    let id = entity_hash(wpn);
    if s.kind_for != id || id.is_none() {
        s.kind_for = id;
        s.kind = if wpn.is_null() { 0 } else { super::melee::kind(wpn) };
    }
    s.kind
}
const IDENTITY: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
/// THE RIGHT GRIP NO LONGER HANDS A SUPPORTED WEAPON TO THE LEFT HAND (main's 0.2.4 CARRY_LEFT = false); a carry
/// already active keeps its recovery.
const CARRY_LEFT: bool = false;
/// The hand-over between the hands (the mod's carryTick).
fn carry_tick(s: &mut State, p: &Handle) -> R<()> {
    if !CARRY_LEFT && !s.carry_on {
        return Ok(());
    }
    if !s.carry_on && native::<bool>("VRWeaponInLeftHand", &[]) == Some(true) {
        return Ok(());
    }
    if stat_static::<bool>("GameObject", "IsDualWielding", &[Value::Handle(p)]).unwrap_or(false) {
        return Ok(());
    }
    let wpn = active_weapon(p);
    if cyber_kind(s, p, &wpn) != 0 {
        return Ok(());
    }
    let ts = super::game_system("GetTransactionSystem");
    if ts.is_null() {
        return Ok(());
    }
    let (right, left) = (tdbid("AttachmentSlots.WeaponRight"), tdbid("AttachmentSlots.WeaponLeft"));
    let (rg, lg) = (shared(49), shared(155));
    let r_down = rg > 0.5;
    let r_fresh = r_down && !s.carry_rgrip_was;
    s.carry_rgrip_was = r_down;
    let in_slot = |slot: u64| h(&ts, "GetItemInSlot", &[Value::Handle(p), Value::U64(slot)]);
    if !r_fresh {
        if s.left_clean && s.clock - s.left_checked_at < 2.0 {
            return Ok(());
        }
        if in_slot(left).is_null() {
            let cur = slot_get_rel(p, "WeaponLeft");
            if cur.is_some_and(|c| c[0].abs() + c[1].abs() + c[2].abs() > 0.0005) {
                slot_set_rel(p, "WeaponLeft", [0.0; 3], IDENTITY);
                log!("[CyberpunkVRPort_Weapon] carry: left slot empty but displaced -- reset (from Rust)\n");
            }
            (s.left_clean, s.left_checked_at) = (cur.is_some(), s.clock);
        }
        return Ok(());
    }
    let item_obj = |w: &Handle| m::<[u64; 2]>(w, "GetItemID", &[]);
    let move_to = |from: u64, to: u64, id: &[u64; 2], w: &Handle| -> (bool, bool) {
        let r = m::<bool>(&ts, "RemoveItemFromSlot", &[Value::Handle(p), Value::U64(from), Value::Bool(false)]).unwrap_or(false);
        let a = m::<bool>(&ts, "AddItemToSlot", &[Value::Handle(p), Value::U64(to), Value::Ptr(id.as_ptr().cast()), Value::Bool(true), Value::Handle(w), Value::U64(PLANE_WEAPON)])
            .unwrap_or(false);
        (r, a)
    };
    if !s.carry_on {
        if s.last_class == 1 || lg <= 0.5 {
            return Ok(());
        }
        let w = in_slot(right);
        if w.is_null() {
            return Ok(());
        }
        let id = item_obj(&w)?;
        let frame = slot_frame(p, "PropLeft");
        let wp = m::<[f32; 4]>(&w, "GetWorldPosition", &[]).ok();
        let wq = m::<[f32; 4]>(&w, "GetWorldOrientation", &[]).ok();
        let (Some((o, b)), Some(wp), Some(wq)) = (frame, wp, wq) else {
            log!("[CyberpunkVRPort_Weapon] carry: cannot measure (frame={} pos={} rot={})\n", frame.is_some(), wp.is_some(), wq.is_some());
            return Ok(());
        };
        let d = [wp[0] - o[0], wp[1] - o[1], wp[2] - o[2]];
        let off = [0, 1, 2].map(|k| d[0] * b[k][0] + d[1] * b[k][1] + d[2] * b[k][2]);
        let bq = quat_from_basis(b);
        let (ci, cj, ck, cr) = (-bq[0], -bq[1], -bq[2], bq[3]);
        let rot = [
            cr * wq[0] + ci * wq[3] + cj * wq[2] - ck * wq[1],
            cr * wq[1] - ci * wq[2] + cj * wq[3] + ck * wq[0],
            cr * wq[2] + ci * wq[1] - cj * wq[0] + ck * wq[3],
            cr * wq[3] - ci * wq[0] - cj * wq[1] - ck * wq[2],
        ];
        s.left_clean = false;
        let wrote = slot_set_rel(p, "WeaponLeft", off, rot);
        let (ok_r, ok_a) = move_to(right, left, &id, &w);
        if !ok_a {
            log!("[CyberpunkVRPort_Weapon] carry: add to the left slot refused (remove={ok_r})\n");
        }
        s.carry_on = ok_a;
        s.carry_item = Some(id);
        if ok_a {
            native::<()>("SetVRCarryLeft", &[Value::Int32(1)]);
        } else {
            move_to(left, right, &id, &w);
            slot_set_rel(p, "WeaponLeft", [0.0; 3], IDENTITY);
        }
        log!(
            "[CyberpunkVRPort_Weapon] carry: -> LEFT pos ({:.3} {:.3} {:.3}) rot ({:.3} {:.3} {:.3} {:.3}) wrote={wrote} ok={} (from Rust)\n",
            off[0], off[1], off[2], rot[0], rot[1], rot[2], rot[3], s.carry_on
        );
    } else {
        s.left_clean = false;
        let w = in_slot(left);
        if !w.is_null() && native::<i32>("GetVRCarryNear", &[]).unwrap_or(-1) == 0 {
            log!("[CyberpunkVRPort_Weapon] carry: right hand too far to take it back (from Rust)\n");
            return Ok(());
        }
        let id = s.carry_item.or_else(|| item_obj(&w).ok());
        let mut ok_a = false;
        if let Some(id) = id {
            ok_a = move_to(left, right, &id, &w).1;
            if !ok_a {
                log!("[CyberpunkVRPort_Weapon] carry: add to the right slot refused\n");
            }
        }
        s.carry_on = !ok_a;
        if ok_a {
            s.carry_item = None;
        }
        slot_set_rel(p, "WeaponLeft", [0.0; 3], IDENTITY);
        native::<()>("SetVRCarryLeft", &[Value::Int32(0)]);
        log!("[CyberpunkVRPort_Weapon] carry: -> RIGHT ok={ok_a} (from Rust)\n");
    }
    let _ = CARRY_BACK_DIST_NOTE;
    Ok(())
}

/// What the load left in the hands (the mod's restoreFinding).
fn restore_finding(p: &Handle, left: bool) -> Option<&'static str> {
    let r = super::requests_handler().ok().filter(|r| !r.is_null())?;
    if m::<bool>(&r, "IsPreGame", &[]).unwrap_or(true) || m::<bool>(&r, "IsGamePaused", &[]).unwrap_or(true) {
        return None;
    }
    let rw = !static_handle("GameObject", "GetRightActiveWeapon", &[Value::Handle(p)]).is_null();
    let lw = !static_handle("GameObject", "GetLeftActiveWeapon", &[Value::Handle(p)]).is_null();
    if (left && rw && !lw) || (!left && lw && !rw) {
        return Some("rehand");
    }
    if rw || lw {
        return Some("done");
    }
    super::left_handed::restore_pending(p).then_some("draw")
}
fn handedness_frame(s: &mut State, p: &Handle, dt: f64) {
    let Some(now) = native::<bool>("VRLeftHanded", &[]) else { return };
    if s.handed_was.is_some_and(|w| w != now) {
        let away = super::left_handed::rehand(p);
        s.rehand_left = away.then_some(REHAND_WAIT);
        log!("[CyberpunkVRPort_Weapon] handedness: left-handed={now}, weapon put away to change hands={away} (from Rust)\n");
    }
    s.handed_was = Some(now);
    if let (Some(watch), None) = (s.restore_watch, s.rehand_left) {
        let watch = watch - dt;
        s.restore_watch = Some(watch);
        if watch <= 0.0 {
            s.restore_watch = None;
            log!("[CyberpunkVRPort_Weapon] handedness: nothing to restore after the load\n");
        } else {
            let f = restore_finding(p, now);
            if f == Some("done") {
                s.restore_watch = None;
            } else {
                s.restore_held = if f.is_some() { s.restore_held + dt } else { 0.0 };
                if let (Some(f), true) = (f, s.restore_held >= RESTORE_SETTLE) {
                    s.restore_watch = None;
                    if f == "draw" {
                        let drawn = super::left_handed::draw_restored(p);
                        log!("[CyberpunkVRPort_Weapon] handedness: the save's left-hand weapon drawn={drawn}\n");
                    } else {
                        let away = super::left_handed::rehand(p);
                        s.rehand_left = away.then_some(REHAND_WAIT);
                        log!("[CyberpunkVRPort_Weapon] handedness: the save's weapon came up in the other hand, put away to change hands={away}\n");
                    }
                }
            }
        }
    }
    let Some(left) = s.rehand_left else { return };
    let left = left - dt;
    s.rehand_left = Some(left);
    if super::left_handed::hands_free(p) {
        s.rehand_left = None;
        log!("[CyberpunkVRPort_Weapon] handedness: drawn again={}\n", super::left_handed::redraw(p));
    } else if left <= 0.0 {
        s.rehand_left = None;
        log!("[CyberpunkVRPort_Weapon] handedness: the hands did not empty, no redraw\n");
    }
}
/// The mod's weaponFrame.
fn weapon_frame(s: &mut State, p: &Handle, dt: f64) {
    let wpn = active_weapon(p);
    if !wpn.is_null() {
        update_muzzle(s, &wpn);
    }
    let cur = if wpn.is_null() { None } else { entity_hash(&wpn) };
    // the drawn weapon's own wrist turn (the overlay's "VR weapon grip")
    super::weapon_wrist::frame(p, &wpn, cur, dt);
    if !wpn.is_null() {
        kill_camera_recoil(s, p, &wpn, cur);
        if s.active_supported.is_some() {
            update_support_accent(s, dt);
        }
    } else {
        s.active_solo = None;
        s.active_supported = None;
        s.recoil_killed = None;
    }
    super::attachment_recoil::update(1, p, (!wpn.is_null()).then_some(&wpn), cur, dt);
    if let Err(e) = left_gun_frame(s, p, dt, cur) {
        let _ = e;
    }
    handedness_frame(s, p, dt);
    if !s.equip_init {
        s.equip_init = true;
        s.equip_id = cur;
    } else if cur != s.equip_id {
        s.equip_id = cur;
        s.recoil_killed = None;
        if !wpn.is_null() {
            super::melee::equip_sound(&wpn);
        }
    }
    if let Ok(z) = m::<f32>(&h(p, "GetFPPCameraComponent", &[]), "GetZoom", &[]) {
        if z > 0.0 {
            native::<()>("SetVRZoomLevel", &[Value::Float(z)]);
        }
    }
    s.guard_clock += dt;
    if cyber_kind(s, p, &wpn) != 0 {
        guard_stats(s, p, false, false);
        s.guard_was_on = false;
        return;
    }
    if wpn.is_null() {
        guard_stats(s, p, false, false);
        s.guard_was_on = false;
        return;
    }
    let is_melee = if cur.is_some() && s.melee_for == cur {
        s.melee_was
    } else {
        let v = m::<[u64; 2]>(&wpn, "GetItemID", &[])
            .and_then(|i| stat_static::<bool>("WeaponObject", "IsMelee", &[Value::Ptr(i.as_ptr().cast())]))
            .unwrap_or(false);
        (s.melee_for, s.melee_was) = (cur, v);
        v
    };
    if !is_melee {
        guard_stats(s, p, false, false);
        s.guard_was_on = false;
        return;
    }
    let (Ok(_wp), Ok(_pp), Ok(fwd)) = (
        m::<[f32; 4]>(&wpn, "GetWorldPosition", &[]),
        m::<[f32; 4]>(p, "GetWorldPosition", &[]),
        Ok::<[f32; 4], String>(super::melee::direction(&wpn)),
    ) else {
        return;
    };
    let mut guard_on = false;
    if let Ok(pf) = m::<[f32; 4]>(p, "GetWorldForward", &[]) {
        let (pfx, pfy) = (f64::from(pf[0]), f64::from(pf[1]));
        let pfl = (pfx * pfx + pfy * pfy).sqrt();
        let (bx, by, bz) = (f64::from(fwd[0]), f64::from(fwd[1]), f64::from(fwd[2]));
        let bfl = (bx * bx + by * by + bz * bz).sqrt();
        if pfl > 0.001 && bfl > 0.001 {
            guard_on = (bx * pfx + by * pfy) / (pfl * bfl) < GUARD_THRUST_DOT;
        }
    }
    if guard_on && !s.guard_was_on {
        s.guard_parry_until = s.guard_clock + GUARD_PARRY_WINDOW;
    }
    s.guard_was_on = guard_on;
    if guard_on {
        let parry = s.guard_clock < s.guard_parry_until;
        guard_stats(s, p, parry, !parry);
    } else {
        guard_stats(s, p, false, false);
    }
}

/// VRWeaponAttached(): the player attached -- the loaded save's weapon is watched for.
pub(in crate::mechanics) fn attached() {
    ATTACHED.store(true, Ordering::Relaxed);
}

/// The mod's updateWeapon, with a player.
pub(in crate::mechanics) fn tick(dt: f32) {
    let dt = f64::from(dt);
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let s = guard.get_or_insert_with(|| State { left_prep_clock: 0.5, left_checked_at: -10.0, guard_parry_until: -1.0, ..Default::default() });
    s.clock += dt;
    if ATTACHED.swap(false, Ordering::Relaxed) {
        (s.restore_watch, s.restore_held) = (Some(RESTORE_WATCH), 0.0);
    }
    let p = player();
    if p.is_null() {
        // The modifiers belonged to the session that ended.
        s.guard_parry = Handle::null();
        s.guard_block = Handle::null();
        super::attachment_recoil::reset();
        return;
    }
    if let Err(e) = carry_tick(s, &p) {
        let _ = e;
    }
    if !s.installed {
        s.install_timer += dt;
        if s.install_timer > 3.0 {
            let r = native::<i32>("InstallVRProvInstrument", &[]).unwrap_or(0);
            let a = native::<i32>("InstallWeaponAimHook", &[]);
            log!("[CyberpunkVRPort_Weapon] InstallWeaponAimHook = {a:?}, InstallVRProvInstrument = {r} (from Rust)\n");
            s.installed = true;
        }
    }
    weapon_frame(s, &p, dt);
}

/// The mod's onShutdown: the guard's modifiers never outlive the session.
pub(in crate::mechanics) fn shutdown() {
    super::attachment_recoil::reset();
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(s) = guard.as_mut() {
        let p = player();
        if !p.is_null() {
            guard_stats(s, &p, false, false);
        }
    }
}
