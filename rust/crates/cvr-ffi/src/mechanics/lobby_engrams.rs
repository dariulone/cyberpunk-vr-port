//! modules/engrams.lua, construct.lua (with construct_list.lua) and clothes.lua. In the lobby the player is a
//! construct: the body's meshes in the construct's appearance (the arms through their own copies), everything else
//! hidden. On the table lie engrams, one per playthrough's newest save (its look read by the plugin: VRLobbySaveLooks);
//! one brought to the neck goes in -- the save's body, appearance and clothes put on the player through the creation's
//! steps, the construct staged so the change is not seen -- and Continue then loads that save; a grip at the neck
//! takes it out again.
use super::belt::{player, qrot, transactions};
use super::lobby::Lobby;
use super::lobby_hands::len;
use super::lobby_items::{self as items, Entry, EntryRef};
use super::lobby_ink as ink;
use super::lobby_rtti::{self as rt, A};
use super::lobby_spawn::{self as sp, add, mul, native, sub, V3};
use crate::script_call::Value;
use red4ext_abi::cname::CName;
use red4ext_abi::handle::Handle;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

// ---------------------------------------------------------------- the looks
pub(super) struct App {
    pub group: i64,
    pub option: String,
    pub appearance: String,
}
pub(super) struct Morph {
    pub group: i64,
    pub region: String,
    pub morph: String,
}
pub(super) struct Look {
    pub playthrough: String,
    pub folder: String,
    pub life: String,
    pub body: String,
    pub brain: String,
    pub level: f64,
    pub time: f64,
    pub apps: Vec<App>,
    pub morphs: Vec<Morph>,
    pub wear: Vec<(String, String)>,
    pub outfit: Vec<(String, String)>,
}
/// M.looks: the saves' looks, newest first.
pub(super) fn looks() -> Vec<Look> {
    let text = sp::native_string("VRLobbySaveLooks", &[]).unwrap_or_default();
    let mut out: Vec<Look> = Vec::new();
    for line in text.split('\n').filter(|l| !l.is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        let g = |i: usize| f.get(i).copied().unwrap_or("").to_string();
        let n = |i: usize| f.get(i).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
        match f.first().copied() {
            Some("look") => out.push(Look {
                playthrough: g(1),
                folder: g(2),
                life: g(3),
                body: g(4),
                brain: g(5),
                level: n(6),
                time: n(7),
                apps: Vec::new(),
                morphs: Vec::new(),
                wear: Vec::new(),
                outfit: Vec::new(),
            }),
            Some("app") => {
                if let Some(c) = out.last_mut() {
                    c.apps.push(App { group: n(1) as i64, option: g(2), appearance: g(3) });
                }
            }
            Some("morph") => {
                if let Some(c) = out.last_mut() {
                    c.morphs.push(Morph { group: n(1) as i64, region: g(2), morph: g(3) });
                }
            }
            Some("wear") => {
                if let Some(c) = out.last_mut() {
                    c.wear.push((g(1), g(2)));
                }
            }
            Some("outfit") => {
                if let Some(c) = out.last_mut() {
                    c.outfit.push((g(1), g(2)));
                }
            }
            _ => {}
        }
    }
    out.sort_by(|a, b| b.time.partial_cmp(&a.time).unwrap_or(std::cmp::Ordering::Equal));
    out
}

// ---------------------------------------------------------------- clothes.lua
const EMPTY: &str = "empty_appearance_default";
const CLOTH_SLOTS: [(&str, &str); 9] = [
    ("Head", "AttachmentSlots.Head"),
    ("Face", "AttachmentSlots.Eyes"),
    ("OuterChest", "AttachmentSlots.Torso"),
    ("InnerChest", "AttachmentSlots.Chest"),
    ("Legs", "AttachmentSlots.Legs"),
    ("Feet", "AttachmentSlots.Feet"),
    ("Outfit", "AttachmentSlots.Outfit"),
    ("UnderwearTop", "AttachmentSlots.UnderwearTop"),
    ("UnderwearBottom", "AttachmentSlots.UnderwearBottom"),
];
#[derive(Default)]
pub(super) struct Clothes {
    worn: Vec<(u64, [u64; 2])>,
    before: HashMap<u64, [u64; 2]>,
    pub error: Option<String>,
}
/// A TweakDBID from the 16 hex digits the plugin writes (length in digits 7..8, the hash in 9..16).
fn tweak(hex: &str) -> Option<u64> {
    if hex.len() != 16 {
        return None;
    }
    let ln = u64::from_str_radix(&hex[6..8], 16).ok()?;
    let hash = u64::from_str_radix(&hex[8..16], 16).ok()?;
    Some(hash | (ln << 32))
}
fn item_from(tdb: u64) -> Option<[u64; 2]> {
    super::belt::st::<[u64; 2]>("ItemID", "FromTDBID", &[Value::U64(tdb)])
}
fn record_exists(id: u64) -> bool {
    let db = red4ext_abi::tweakdb::get();
    // SAFETY: the database's own record lookup.
    !db.is_null() && unsafe { red4ext_abi::tweakdb::record_exists(db, id) }
}
fn take_off(c: &mut Clothes, pl: &Handle, ts: &Handle) {
    for (slot, item) in std::mem::take(&mut c.worn) {
        rt::call(ts, "RemoveItemFromSlot", &[A::H(pl), A::Name(slot), A::B(true)]);
        rt::call(ts, "RemoveItem", &[A::H(pl), A::P(item.as_ptr().cast()), A::N(1.0)]);
    }
}
/// M.dress: the look's clothes on the player (its outfit, else its pieces); true when it covers the feet.
pub(super) fn dress(c: &mut Clothes, look: &Look) -> bool {
    let (pl, ts) = (player(), transactions());
    if pl.is_null() || ts.is_null() {
        return false;
    }
    take_off(c, &pl, &ts);
    let mut pieces: Vec<(Option<u64>, Option<u64>, bool)> = Vec::new();
    if !look.outfit.is_empty() {
        for (slot, item) in &look.outfit {
            pieces.push((tweak(slot), tweak(item), false));
        }
    } else {
        for (area, item) in &look.wear {
            if let Some((_, slot)) = CLOTH_SLOTS.iter().find(|(a, _)| a == area) {
                let feet = *slot == "AttachmentSlots.Feet" || *slot == "AttachmentSlots.Outfit";
                pieces.push((Some(red4ext_abi::tweakdbid::from_name(slot)), tweak(item), feet));
            }
        }
    }
    let mut feet = false;
    for (slot, item, pf) in pieces {
        let (Some(slot), Some(item)) = (slot, item) else { continue };
        if !record_exists(item) {
            continue;
        }
        let Some(id) = item_from(item) else {
            c.error = Some("ItemID.FromTDBID failed".into());
            continue;
        };
        let held = rt::obj(&ts, "GetItemInSlot", &[A::H(&pl), A::Name(slot)]);
        if !held.is_null() && !c.before.contains_key(&slot) {
            if let Some(hid) = super::belt::item_id(&held) {
                c.before.insert(slot, hid);
            }
        }
        rt::call(&ts, "GiveItem", &[A::H(&pl), A::P(id.as_ptr().cast()), A::N(1.0)]);
        rt::call(&ts, "AddItemToSlot", &[A::H(&pl), A::Name(slot), A::P(id.as_ptr().cast()), A::B(true)]);
        c.worn.push((slot, id));
        feet = feet || pf;
    }
    feet
}
pub(super) fn undress(c: &mut Clothes) {
    let (pl, ts) = (player(), transactions());
    if !pl.is_null() && !ts.is_null() {
        take_off(c, &pl, &ts);
        for (slot, item) in c.before.drain() {
            rt::call(&ts, "AddItemToSlot", &[A::H(&pl), A::Name(slot), A::P(item.as_ptr().cast()), A::B(true)]);
        }
        for (_, slot) in CLOTH_SLOTS {
            let held = rt::obj(&ts, "GetItemInSlot", &[A::H(&pl), A::Name(red4ext_abi::tweakdbid::from_name(slot))]);
            let Some(id) = (!held.is_null()).then(|| super::belt::item_id(&held)).flatten() else { continue };
            let app = rt::call(&ts, "GetItemAppearance", &[A::H(&pl), A::P(id.as_ptr().cast())]).map(|o| rt::Val::of(&o).text()).unwrap_or_default();
            if app == EMPTY {
                rt::call(&ts, "ResetItemAppearance", &[A::H(&pl), A::P(id.as_ptr().cast())]);
            }
        }
    }
    (c.worn, c.before) = (Vec::new(), HashMap::new());
}

// ---------------------------------------------------------------- construct.lua (with construct_list.lua)
const APPEARANCE: &str = "vrport_construct";
const WRIST: &str = "vrp_latd_screen";
const SETTLE: f64 = 2.0;
const MASK: u64 = 0xFFFF_FFFF_FFFF_FFF7;
struct Worn {
    path: &'static str,
    own: Option<&'static str>,
    mask: Option<u64>,
}
fn worn_entry(key: u64) -> Option<Worn> {
    let w = |path, own, mask| Some(Worn { path, own, mask });
    match key {
        17351060424129901860 => w(r"base\characters\common\player_base_bodies\player_female_average\arms_hq\a0_000_pwa_base_hq__l.mesh", Some(r"vrport\construct\a0_000_pwa_base_hq__l.mesh"), Some(MASK)),
        6127661484476729462 => w(r"base\characters\common\player_base_bodies\player_female_average\arms_hq\a0_000_pwa_base_hq__r.mesh", Some(r"vrport\construct\a0_000_pwa_base_hq__r.mesh"), None),
        3424520411910737910 => w(r"base\characters\common\player_base_bodies\player_female_average\fpp\n0_000_pwa_fpp__neck.mesh", None, None),
        4386203188796562796 => w(r"base\characters\common\player_base_bodies\player_female_average\l0_000_pwa_base__cs_flat.mesh", None, None),
        10998646327038800579 => w(r"base\characters\common\player_base_bodies\player_female_average\t0_000_pwa_base__full.morphtarget", None, None),
        6123990083473767804 => w(r"base\characters\common\player_base_bodies\player_man_average\arms_hq\a0_000_pma_base_hq__l.mesh", Some(r"vrport\construct\a0_000_pma_base_hq__l.mesh"), Some(MASK)),
        3536741777257648318 => w(r"base\characters\common\player_base_bodies\player_man_average\arms_hq\a0_000_pma_base_hq__r.mesh", Some(r"vrport\construct\a0_000_pma_base_hq__r.mesh"), None),
        10166971776422861834 => w(r"base\characters\common\player_base_bodies\player_man_average\fpp\n0_000_pma_fpp__neck.mesh", None, None),
        3781668235714414901 => w(r"base\characters\common\player_base_bodies\player_man_average\t0_000_pma_base__full.mesh", None, None),
        17923313006320215944 => w(r"base\characters\garment\player_equipment\feet\s1_010_shoe__elegant\s1_010_pma_shoe__elegant.mesh", None, None),
        _ => None,
    }
}
fn own_key(key: u64) -> bool {
    matches!(key, 4348962040142790935 | 13207257604266595469 | 14785570617216477197 | 5927275053092672663)
}
struct WornWas {
    app: String,
    key: u64,
    mask: u64,
}
pub(super) struct Construct {
    worn: HashMap<String, WornWas>,
    hidden: HashSet<String>,
    clock: f64,
    settle: f64,
    enabled: bool,
    own_body: bool,
    resolved: bool,
    engram: HashSet<u64>,
    covered: HashSet<u64>,
    staging: bool,
    before: HashMap<u64, bool>,
    staged: HashSet<u64>,
    restyled: HashMap<u64, String>,
}
impl Default for Construct {
    fn default() -> Construct {
        Construct {
            worn: HashMap::new(),
            hidden: HashSet::new(),
            clock: 0.0,
            settle: 0.0,
            enabled: true,
            own_body: false,
            resolved: false,
            engram: HashSet::new(),
            covered: HashSet::new(),
            staging: false,
            before: HashMap::new(),
            staged: HashSet::new(),
            restyled: HashMap::new(),
        }
    }
}
fn u64_prop(c: &Handle, name: &str) -> Option<u64> {
    // SAFETY: the component's 8-byte property (a resource reference's hash, a chunk mask).
    unsafe { crate::script_call::property(c.instance, name) }.map(|p| unsafe { *(p as *const u64) })
}
fn set_u64_prop(c: &Handle, name: &str, v: u64) {
    // SAFETY: as above.
    if let Some(p) = unsafe { crate::script_call::property(c.instance, name) } {
        unsafe { *(p as *mut u64) = v };
    }
}
fn resource_of(c: &Handle) -> Option<u64> {
    u64_prop(c, "mesh").or_else(|| u64_prop(c, "morphResource"))
}
fn key_of(c: &Handle) -> u64 {
    native::<u64>("VRComponentKey", &[Value::Handle(c)]).unwrap_or(0)
}
fn meshes(pl: &Handle) -> Vec<Handle> {
    sp::native_owned("VREntitySkinnedMeshes", &[Value::Handle(pl)]).map(|o| super::belt::handles(o.as_ptr())).unwrap_or_default()
}
fn name(c: &Handle) -> String {
    super::belt::comp_name(c)
}
fn app(c: &Handle) -> String {
    u64_prop(c, "meshAppearance").map(|n| CName(n).text().to_string()).unwrap_or_default()
}
fn enabled(c: &Handle) -> bool {
    ink::get::<bool>(c, "IsEnabled", &[]) == Some(true)
}
fn toggle(c: &Handle, on: bool) {
    super::belt::toggle(c, on);
}
fn change_resource(c: &Handle, path: &str) {
    let s = sp::Str::new(path);
    native::<bool>("VRComponentChangeResource", &[Value::Handle(c), s.v(), Value::Bool(false)]);
}
fn change_appearance(c: &Handle, a: &str) {
    native::<bool>("VRComponentChangeAppearance", &[Value::Handle(c), sp::cn(a), Value::Bool(false)]);
}
fn to_construct(c: &Handle, e: &Worn) {
    if let Some(own) = e.own {
        if let Some(m) = e.mask {
            set_u64_prop(c, "chunkMask", m);
        }
        set_u64_prop(c, "meshAppearance", CName::hashed(APPEARANCE).0);
        change_resource(c, own);
    } else if app(c) != APPEARANCE {
        change_appearance(c, APPEARANCE);
    }
}
fn apply(s: &mut Construct, pl: &Handle) {
    for c in meshes(pl) {
        let nm = name(&c);
        if nm == WRIST {
            if enabled(&c) {
                toggle(&c, false);
            }
        } else if !nm.starts_with("vrp_") {
            let key = resource_of(&c).unwrap_or(0);
            match worn_entry(key) {
                Some(e) if !s.engram.contains(&key_of(&c)) => {
                    s.worn.entry(nm.clone()).or_insert_with(|| WornWas { app: app(&c), key, mask: u64_prop(&c, "chunkMask").unwrap_or(0) });
                    to_construct(&c, &e);
                }
                _ => {
                    if !own_key(key) && app(&c) != APPEARANCE && enabled(&c) {
                        s.hidden.insert(nm);
                        toggle(&c, false);
                    }
                }
            }
        }
    }
}
fn hold_new(s: &mut Construct, pl: &Handle) {
    for c in meshes(pl) {
        let key = key_of(&c);
        let was_on = s.before.get(&key).copied();
        if was_on.is_none() {
            s.engram.insert(key);
        }
        let nm = if enabled(&c) { Some(name(&c)) } else { None };
        let Some(nm) = nm.filter(|n| !n.starts_with("vrp_")) else { continue };
        if s.engram.contains(&key) {
            s.staged.insert(key);
            toggle(&c, false);
        } else if app(&c) != APPEARANCE {
            if let Some(e) = s.worn.get(&nm).and_then(|w| worn_entry(w.key)) {
                s.restyled.insert(key, app(&c));
                to_construct(&c, &e);
            } else {
                s.staged.insert(key);
                toggle(&c, false);
            }
        } else if was_on != Some(true) {
            toggle(&c, false);
        }
    }
}
pub(super) fn construct_update(s: &mut Construct, dt: f64) {
    if s.staging {
        let pl = player();
        if !pl.is_null() {
            hold_new(s, &pl);
        }
        return;
    }
    if !s.enabled || s.own_body {
        return;
    }
    s.clock -= dt;
    s.settle -= dt;
    if s.clock > 0.0 && s.settle <= 0.0 {
        return;
    }
    s.clock = 1.0;
    let pl = player();
    if !pl.is_null() {
        apply(s, &pl);
    }
}
fn undo(s: &mut Construct, body_only: bool) {
    let pl = player();
    if !pl.is_null() {
        for c in meshes(&pl) {
            let nm = name(&c);
            let key = key_of(&c);
            if s.engram.contains(&key) {
                continue;
            }
            if let Some(was) = s.worn.get(&nm) {
                let now = app(&c);
                let a = s.restyled.get(&key).cloned().unwrap_or_else(|| if now != APPEARANCE { now.clone() } else { was.app.clone() });
                if let Some(e) = worn_entry(was.key) {
                    if e.own.is_some() {
                        if e.mask.is_some() {
                            set_u64_prop(&c, "chunkMask", was.mask);
                        }
                        set_u64_prop(&c, "meshAppearance", CName::hashed(&a).0);
                        change_resource(&c, e.path);
                    } else if now != a {
                        change_appearance(&c, &a);
                    }
                }
            }
            if s.hidden.contains(&nm) && !body_only {
                toggle(&c, true);
            }
        }
    }
    (s.worn, s.clock, s.restyled) = (HashMap::new(), 0.0, HashMap::new());
    if !body_only {
        s.hidden.clear();
    }
}
pub(super) fn resolve_as(s: &mut Construct, male: Option<bool>) {
    if male.is_none() && !s.resolved {
        return;
    }
    s.resolved = male.is_some();
    native::<()>("VRLobbyResolveAs", &[Value::Bool(male != Some(false))]);
}
fn unwear(s: &mut Construct, pl: &Handle) {
    for c in meshes(pl) {
        let key = key_of(&c);
        if s.engram.contains(&key) {
            if enabled(&c) {
                toggle(&c, false);
            }
        } else if s.covered.contains(&key) {
            toggle(&c, true);
            s.hidden.remove(&name(&c));
        }
    }
    (s.covered, s.staged, s.staging, s.restyled) = (HashSet::new(), HashSet::new(), false, HashMap::new());
}
pub(super) fn restore(s: &mut Construct) {
    (s.own_body, s.staging) = (false, false);
    resolve_as(s, None);
    (s.covered, s.staged, s.before, s.restyled) = (HashSet::new(), HashSet::new(), HashMap::new(), HashMap::new());
    if !s.worn.is_empty() || !s.hidden.is_empty() {
        undo(s, false);
    }
    s.engram.clear();
}
fn head_part(nm: &str, a: &str) -> bool {
    let h = nm.strip_prefix('h').is_some_and(|r| {
        let lower = r.chars().take_while(|c| c.is_ascii_lowercase()).count();
        r[lower..].starts_with('_')
    });
    if h || nm.starts_with("beard") || nm.starts_with("i1_") {
        return true;
    }
    nm.starts_with("MorphTargetSkinnedMesh") && !a.contains("personal_link") && !a.contains("nails")
}
fn underwear(nm: &str) -> bool {
    nm.contains("boxers") || nm.contains("underwear") || nm.contains("panties")
}
fn of_body(nm: &str, male: bool) -> bool {
    if male {
        nm.contains("pma") || nm.contains("_ma_")
    } else {
        nm.contains("pwa") || nm.contains("_wa_")
    }
}
fn spawned_male(pl: &Handle) -> Option<bool> {
    for c in meshes(pl) {
        if super::belt::class_name(&c) == "entGarmentSkinnedMeshComponent" {
            match name(&c).as_str() {
                "t0_000_pma_base__full" => return Some(true),
                "t0_000_pwa_base__full" => return Some(false),
                _ => {}
            }
        }
    }
    None
}
fn cover(s: &mut Construct, c: &Handle) {
    s.hidden.insert(name(c));
    let key = key_of(c);
    if !s.engram.contains(&key) && worn_entry(resource_of(c).unwrap_or(0)).is_some() {
        s.covered.insert(key);
    }
    toggle(c, false);
}
/// t0_000_p<l>a_base__full
fn body_full(nm: &str) -> bool {
    nm.len() == "t0_000_pma_base__full".len() && nm.starts_with("t0_000_p") && nm.ends_with("a_base__full") && nm.as_bytes()[8].is_ascii_lowercase()
}
/// M.dressBody: the engram's body over the construct -- the head's parts, the duplicate meshes and the other body's
/// covered, the neck and the genitals in the body's skin, the underwear shown.
pub(super) fn dress_body(s: &mut Construct, male: Option<bool>) -> usize {
    let pl = player();
    if pl.is_null() {
        return 0;
    }
    let own = spawned_male(&pl);
    let cross = matches!((male, own), (Some(m), Some(o)) if m != o);
    let mut last: HashMap<String, u64> = HashMap::new();
    let mut all = Vec::new();
    for c in meshes(&pl) {
        let key = if enabled(&c) { resource_of(&c) } else { None };
        if let Some(k) = key.filter(|k| *k != 0) {
            let key = format!("{k}:{}", u64_prop(&c, "chunkMask").unwrap_or(0));
            last.insert(key.clone(), c.instance as u64);
            all.push((c, key));
        }
    }
    let (mut n, mut skin) = (0, None);
    for (c, key) in &all {
        let (nm, a) = (name(c), app(c));
        if last.get(key) != Some(&(c.instance as u64)) || head_part(&nm, &a) || (cross && own.is_some_and(|o| of_body(&nm, o)) && !nm.starts_with("n0_")) {
            cover(s, c);
            n += 1;
        } else if body_full(&nm) {
            skin = Some(a);
        }
    }
    for c in meshes(&pl) {
        let nm = name(&c);
        let skinned = nm.starts_with("n0_") || (nm.starts_with("i0_000_p") && nm.get(9..).is_some_and(|r| r.starts_with("a_base__genitals_none")));
        if let Some(sk) = &skin {
            if skinned && &app(&c) != sk {
                change_appearance(&c, sk);
            }
        }
        if underwear(&nm) && !enabled(&c) && !(cross && own.is_some_and(|o| of_body(&nm, o))) {
            toggle(&c, true);
        }
    }
    n
}
pub(super) fn hide_shoes(s: &mut Construct) {
    let pl = player();
    for c in meshes(&pl) {
        if enabled(&c) && name(&c).starts_with("s1_") && !s.engram.contains(&key_of(&c)) {
            cover(s, &c);
        }
    }
}
pub(super) fn own_body(s: &mut Construct, on: bool) {
    if s.own_body == on {
        return;
    }
    s.own_body = on;
    if on {
        undo(s, true);
    } else {
        resolve_as(s, None);
        let pl = player();
        if !pl.is_null() {
            unwear(s, &pl);
            if s.enabled {
                apply(s, &pl);
            }
        }
        (s.clock, s.settle) = (1.0, SETTLE);
    }
}
pub(super) fn stage(s: &mut Construct) {
    let pl = player();
    if pl.is_null() {
        return;
    }
    own_body(s, false);
    if s.enabled {
        apply(s, &pl);
    }
    (s.before, s.staged, s.restyled) = (HashMap::new(), HashSet::new(), HashMap::new());
    let mut engram = HashSet::new();
    for c in meshes(&pl) {
        let key = key_of(&c);
        s.before.insert(key, enabled(&c));
        if s.engram.contains(&key) {
            engram.insert(key);
        }
    }
    s.engram = engram;
    s.staging = true;
}
pub(super) fn reveal(s: &mut Construct, male: bool, shoes: bool) {
    let pl = player();
    if pl.is_null() {
        return;
    }
    hold_new(s, &pl);
    s.staging = false;
    own_body(s, true);
    for c in meshes(&pl) {
        if s.staged.contains(&key_of(&c)) {
            toggle(&c, true);
        }
    }
    s.staged.clear();
    dress_body(s, Some(male));
    if shoes {
        hide_shoes(s);
    }
}
pub(super) fn unstage(s: &mut Construct) {
    s.staging = false;
    s.own_body = true;
    own_body(s, false);
}
pub(super) fn enable(s: &mut Construct, on: bool) {
    s.enabled = on;
    if !on {
        restore(s);
    }
}

// ---------------------------------------------------------------- engrams.lua
const ENT: &str = "vrport\\engram\\vr_engram_";
const LOOKS: [&str; 4] = ["default", "b", "c", "d"];
const RADIUS: f64 = 0.023;
const MASS: f64 = 0.02;
const SHARD_HALF: V3 = [0.011, 0.0232, 0.002];
const SPACING: f64 = 0.09;
const PER_ROW: usize = 3;
const NECK_DROP: f64 = 0.11;
const NECK_BACK: f64 = 0.09;
const INSERT: f64 = 0.08;
const TAKE_OUT: f64 = 0.14;
const AWAY: f64 = 0.25;
#[derive(Default)]
pub(super) struct Engrams {
    entries: Vec<EntryRef>,
    inserted: Option<EntryRef>,
    wearing: bool,
    pub error: Option<String>,
    out: Option<EntryRef>,
}
/// M.items: the scene's items with the shards replaced by the engrams laid out where they were.
pub(super) fn items(l: &mut Lobby, list: Vec<EntryRef>) -> Vec<EntryRef> {
    let (mut out, mut shards) = (Vec::new(), Vec::new());
    for e in list {
        if e.name.starts_with("item shard") {
            shards.push(e);
        } else {
            out.push(e);
        }
    }
    l.engrams.entries.clear();
    let Some(first) = shards.first().cloned() else { return out };
    let p0 = first.position;
    let mut step = shards.get(1).map_or([0.0, 1.0, 0.0], |s| sub(s.position, p0));
    step[2] = 0.0;
    step = mul(step, SPACING / len(step).max(1e-6));
    let mut back = [-step[1], step[0], 0.0];
    if sp::dot(back, [p0[0], p0[1], 0.0]) < 0.0 {
        back = mul(back, -1.0);
    }
    for (i, look) in looks().into_iter().enumerate() {
        let (col, row) = (i % PER_ROW, i / PER_ROW);
        let p = add(add(p0, mul(step, col as f64)), mul(back, row as f64));
        let e = Rc::new(Entry {
            name: format!("engram {}", i + 1),
            ent: format!("{ENT}{}.ent", LOOKS[i % LOOKS.len()]),
            position: p,
            rotation: first.rotation,
            radius: RADIUS,
            mass: MASS,
            shape: Some("box".into()),
            half: Some(SHARD_HALF),
            grasp_half: None,
            grasp_at: None,
            centre: None,
            look: Some(Rc::new(look)),
        });
        out.push(e.clone());
        l.engrams.entries.push(e);
    }
    out
}
fn neck() -> Option<V3> {
    let (pp, pq) = super::lobby_hands::player_pose()?;
    let c = native::<[f32; 4]>("VRCamModelPos", &[]).filter(|c| c[3] >= 0.5)?;
    let head = add(pp, qrot(pq, [f64::from(c[0]), f64::from(c[1]), f64::from(c[2])]));
    let mut back = [0.0; 3];
    if let Some(f) = super::belt::m::<[f32; 4]>(&super::game_system("GetCameraSystem"), "GetActiveCameraForward", &[]).ok() {
        let (fx, fy) = (f64::from(f[0]), f64::from(f[1]));
        let l = (fx * fx + fy * fy).sqrt();
        if l > 1e-3 {
            back = [-fx / l * NECK_BACK, -fy / l * NECK_BACK, 0.0];
        }
    }
    Some([head[0] + back[0], head[1] + back[1], head[2] - NECK_DROP])
}
fn feedback(hand: usize) {
    let f = sp::Str::new("CyberpunkVRPort_Menu/sounds/open.wav");
    native::<bool>("VRMenuPlayWav", &[f.v(), Value::Float(0.9)]);
    sp::haptic(hand, 20);
}
fn wear(l: &mut Lobby, entry: EntryRef) -> bool {
    let Some(look) = entry.look.clone() else { return false };
    let male = look.body != "Female";
    let shoes = Rc::new(std::cell::Cell::new(false));
    let (look2, sh1, sh2, e2) = (look.clone(), shoes.clone(), shoes, entry.clone());
    let cb = super::lobby_creation::WearCb {
        stage: Rc::new(move |l| {
            stage(&mut l.construct);
            resolve_as(&mut l.construct, Some(male));
            let feet = dress(&mut l.clothes, &look2);
            sh1.set(feet);
            if feet {
                hide_shoes(&mut l.construct);
            }
        }),
        ready: Rc::new(move |l| reveal(&mut l.construct, male, sh2.get())),
        done: Rc::new(move |l, done| {
            l.engrams.wearing = false;
            if done {
                return;
            }
            l.engrams.error = Some("the look did not go on".into());
            unstage(&mut l.construct);
            undress(&mut l.clothes);
            if l.engrams.inserted.as_ref().is_some_and(|i| Rc::ptr_eq(i, &e2)) {
                l.engrams.inserted = None;
                items::restore(l, &e2);
            }
        }),
    };
    if !super::lobby_creation::wear(l, look, cb) {
        return false;
    }
    let previous = l.engrams.inserted.replace(entry);
    (l.engrams.wearing, l.engrams.error) = (true, None);
    if let Some(p) = previous {
        items::restore(l, &p);
    }
    true
}
fn insert(l: &mut Lobby, hand: Option<usize>, entry: EntryRef) {
    if super::lobby_creation::busy(l) || super::lobby_creation::is_open(l) {
        return;
    }
    if !wear(l, entry.clone()) {
        l.engrams.error = Some("busy".into());
        return;
    }
    match hand {
        None => {
            items::take_entry(l, &entry);
        }
        Some(h) => {
            items::take(l, h);
            feedback(h);
        }
    }
}
fn take_out(l: &mut Lobby, hand: usize) {
    let Some(entry) = l.engrams.inserted.clone() else { return };
    if l.engrams.wearing || !items::give(l, &entry, hand) {
        return;
    }
    (l.engrams.inserted, l.engrams.out) = (None, Some(entry));
    own_body(&mut l.construct, false);
    undress(&mut l.clothes);
    feedback(hand);
}
pub(super) fn update(l: &mut Lobby, _dt: f64) {
    if l.engrams.entries.is_empty() {
        return;
    }
    let Some(n) = neck() else { return };
    for hand in 1..=2 {
        match items::held(l, hand) {
            Some((entry, p)) if entry.look.is_some() => {
                let d = len(sub(p, n));
                if l.engrams.out.as_ref().is_some_and(|o| Rc::ptr_eq(o, &entry)) {
                    if l.hands.h(hand).pos.is_some_and(|hp| len(sub(hp, n)) > AWAY) {
                        l.engrams.out = None;
                    }
                } else if d < INSERT {
                    insert(l, Some(hand), entry);
                }
            }
            _ => {
                if l.engrams.inserted.is_some() && l.hands.taking(hand) {
                    if l.hands.h(hand).pos.is_some_and(|hp| len(sub(hp, n)) < TAKE_OUT) {
                        take_out(l, hand);
                    }
                }
            }
        }
    }
}
/// M.inserted: the save folder of the engram in the neck.
pub(super) fn inserted_folder(l: &Lobby) -> Option<String> {
    l.engrams.inserted.as_ref().and_then(|e| e.look.as_ref()).map(|lk| lk.folder.clone())
}
pub(super) fn close(l: &mut Lobby) {
    resolve_as(&mut l.construct, None);
    undress(&mut l.clothes);
    l.engrams = Engrams::default();
}
pub(super) fn state(l: &Lobby) -> String {
    format!(
        "engrams {} inserted {} wearing {} error {}",
        l.engrams.entries.len(),
        inserted_folder(l).unwrap_or_else(|| "-".into()),
        l.engrams.wearing,
        l.engrams.error.clone().unwrap_or_else(|| "-".into())
    )
}
