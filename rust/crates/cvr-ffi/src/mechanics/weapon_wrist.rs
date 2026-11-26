//! THE WRIST'S TURN PER WEAPON: the overlay's "VR weapon grip" window (beside the CET mods' windows, dev_windows.rs)
//! turns the hand that holds the drawn weapon -- and the weapon with it -- by pitch, yaw and roll, each with a slider
//! and buttons of 0.1 degree, kept per weapon model in config\weapon_wrist.json ({"Preset_Base_Lexington":
//! [pitch, yaw, roll]}). The weapon frame (weapon.rs) tells which weapon is drawn; the rig applies the turn after the
//! wrist calibration (cvr-rig vrik::weapon_wrist).
//!
//! THE BUILT-IN TURNS: weapon_wrist_defaults.json, built into the plugin, is every player's turn of a model; the
//! player's file holds only the turns that differ from it (a model turned back to nothing included), so a later
//! build's turns reach every model the player has not changed. A reset puts a model back to its built-in turn.
//!
//! A MODEL is the weapon records' base appearance preset (Item_Record.AppearanceResourceName): every quality of a
//! weapon and its iconic variants share it (Items.Preset_Lexington_Default and _Wilson: Preset_Base_Lexington), so one
//! turn serves them all. A turn kept under a record's name (the first 0.2.5 builds) moves to its model when that
//! weapon is drawn.
//!
//! THE WEAPON LIST: the game's weapon records (TweakDB, gamedataWeaponItem_Record) read in once the window asks,
//! a few dozen a frame, grouped by model and kind -- the hand-held weapons a player can carry (not the abstract
//! Items.Base_ records, the left-hand twins, the hidden ones, the vehicles' or the arm cyberware). Picking one draws
//! that model's plain preset into the hand: the player's own copy when there is one, a copy given for the test
//! otherwise. The game puts a weapon it draws into a weapon slot (the first empty one, or the active one), so the
//! slots are noted at the first pick; "Put my weapons back" equips them again, takes back the copies given, and draws
//! the weapon that was in hand then (or leaves the hands empty as they were).
use super::lobby_rtti::{self as rt, Val, A};
use crate::script_call::{Owned, Value};
use cvr_host::log;
use red4ext_abi::cname::CName;
use red4ext_abi::handle::Handle;
use std::collections::BTreeMap;
use std::sync::Mutex;

/// The turns' range (degrees) and the buttons' step.
const RANGE: f32 = 60.0;
const STEP: f32 = 0.1;
/// A change is written this long after the last one (and when the windows close).
const SAVE_AFTER: f64 = 2.0;
/// Weapon records read a frame while the list is being made.
const RECORDS_PER_FRAME: u32 = 64;
/// The weapon slots noted and put back (the three of the weapon wheel).
const SLOTS: usize = 3;

type Item = [u64; 2];
const NONE: Item = [0, 0];

/// One weapon model of the list.
#[derive(Clone)]
struct Model {
    /// The model's key: its base appearance preset.
    key: String,
    /// Its kind (the item type without "Wea_").
    kind: String,
    /// The name shown: the drawn record's own, localized.
    name: String,
    /// The record drawn when it is picked, its name, its display name's key and how well it suits.
    record: u64,
    record_name: String,
    name_key: u64,
    score: i32,
}

#[derive(Default)]
struct Catalog {
    built: bool,
    total: u32,
    /// The weapon records' ids, read once from the TweakDB's records by type.
    ids: Vec<u64>,
    cursor: u32,
    models: BTreeMap<String, Model>,
    /// The models in the list's order: by kind, then by name.
    order: Vec<String>,
}

/// The weapons the list drew: the weapon slots and the weapon in hand at the first pick, the copies given.
struct Session {
    slots: [Item; SLOTS],
    drawn: Item,
    given: Vec<Item>,
}

/// Putting the weapons back, a step at a time: the weapon put away, the slots equipped again, the copies taken back.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Restore {
    Holster,
    Slots,
    Finish,
}

struct State {
    loaded: bool,
    turns: BTreeMap<String, [f32; 3]>,
    /// The drawn weapon: its entity, its model's key, its record's name.
    entity: Option<u64>,
    key: Option<String>,
    record: Option<String>,
    dirty: bool,
    since_change: f64,
    /// The list (made on the game thread when the window asks for it), the window's filter.
    catalog: Catalog,
    want_catalog: bool,
    filter: String,
    /// A record picked in the window, drawn by the next game frame; what happened last.
    pick: Option<u64>,
    status: String,
    session: Option<Session>,
    restore: Option<(Restore, f64)>,
    clock: f64,
}
static STATE: Mutex<State> = Mutex::new(State {
    loaded: false,
    turns: BTreeMap::new(),
    entity: None,
    key: None,
    record: None,
    dirty: false,
    since_change: 0.0,
    catalog: Catalog { built: false, total: 0, ids: Vec::new(), cursor: 0, models: BTreeMap::new(), order: Vec::new() },
    want_catalog: false,
    filter: String::new(),
    pick: None,
    status: String::new(),
    session: None,
    restore: None,
    clock: 0.0,
});

fn path() -> std::path::PathBuf {
    cvr_host::paths::file(cvr_host::paths::Folder::Config, "weapon_wrist.json")
}
/// The turns of a {"model": [pitch, yaw, roll]} object, each within the range.
fn turns_of(json: &serde_json::Value) -> BTreeMap<String, [f32; 3]> {
    let mut out = BTreeMap::new();
    if let Some(map) = json.as_object() {
        for (k, v) in map {
            let a: Vec<f32> = v.as_array().map(|a| a.iter().filter_map(|x| x.as_f64()).map(|x| x as f32).collect()).unwrap_or_default();
            if a.len() == 3 && a.iter().all(|x| x.is_finite()) {
                out.insert(k.clone(), [a[0].clamp(-RANGE, RANGE), a[1].clamp(-RANGE, RANGE), a[2].clamp(-RANGE, RANGE)]);
            }
        }
    }
    out
}
/// The built-in turns (weapon_wrist_defaults.json).
fn defaults() -> &'static BTreeMap<String, [f32; 3]> {
    static DEFAULTS: std::sync::OnceLock<BTreeMap<String, [f32; 3]>> = std::sync::OnceLock::new();
    DEFAULTS.get_or_init(|| turns_of(&serde_json::from_str(include_str!("weapon_wrist_defaults.json")).unwrap_or(serde_json::Value::Null)))
}
/// A model's built-in turn (none: not turned).
fn default_of(key: &str) -> [f32; 3] {
    defaults().get(key).copied().unwrap_or([0.0; 3])
}
fn load(s: &mut State) {
    if s.loaded {
        return;
    }
    s.loaded = true;
    s.turns = defaults().clone();
    let own = match std::fs::read_to_string(path()) {
        Ok(text) => match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(json) => turns_of(&json),
            Err(_) => {
                log!("[weapon-wrist] config\\weapon_wrist.json is not JSON: kept aside, not read\n");
                BTreeMap::new()
            }
        },
        Err(_) => BTreeMap::new(),
    };
    let changed = own.iter().filter(|(k, t)| **t != default_of(k)).count();
    // entries equal to the built-in turn are not the player's own: written out again without them, or a later build's
    // turn for that model would be held at this one
    if changed != own.len() {
        s.dirty = true;
        s.since_change = 0.0;
    }
    s.turns.extend(own);
    log!("[weapon-wrist] {} built-in wrist turn(s), {changed} of the player's own\n", defaults().len());
}
fn save(s: &mut State) {
    if !s.dirty {
        return;
    }
    s.dirty = false;
    // only what differs from the built-in turns: the rest follows the build
    let map: serde_json::Map<String, serde_json::Value> = s
        .turns
        .iter()
        .filter(|(k, t)| **t != default_of(k))
        .map(|(k, t)| (k.clone(), serde_json::json!([round(t[0]), round(t[1]), round(t[2])])))
        .collect();
    let text = serde_json::to_string_pretty(&serde_json::Value::Object(map)).unwrap_or_default();
    match std::fs::write(path(), text) {
        Ok(()) => log!("[weapon-wrist] config\\weapon_wrist.json written\n"),
        Err(e) => log!("[weapon-wrist] config\\weapon_wrist.json not written: {e}\n"),
    }
}
fn round(v: f32) -> f64 {
    (f64::from(v) * 10.0).round() / 10.0
}
fn publish(s: &State) {
    let t = s.key.as_ref().and_then(|k| s.turns.get(k)).copied().unwrap_or([0.0; 3]);
    cvr_rig::vrik::weapon_wrist::set_degrees(t[0], t[1], t[2]);
}

// ---------------------------------------------------------------- the game's side
fn ret_u64(o: Option<Owned>) -> u64 {
    // SAFETY: an 8-byte result read as such.
    o.filter(|o| o.ty().size() == 8).map_or(0, |o| unsafe { *(o.as_ptr() as *const u64) })
}
fn ret_item(o: Option<Owned>) -> Item {
    // SAFETY: an ItemID result (16 bytes).
    o.filter(|o| o.ty().size() == 16).map_or(NONE, |o| unsafe { *(o.as_ptr() as *const Item) })
}
fn ret_bool(o: Option<Owned>) -> bool {
    o.is_some_and(|o| Val::of(&o).bool())
}
fn valid(i: &Item) -> bool {
    i[0] != 0
}
/// ItemID equality: the record, the seed and the counter.
fn same(a: &Item, b: &Item) -> bool {
    a[0] == b[0] && (a[1] & 0xFFFF_FFFF_FFFF) == (b[1] & 0xFFFF_FFFF_FFFF)
}
fn ip(i: &Item) -> A<'_> {
    A::P(i.as_ptr().cast())
}
fn name_text(hash: u64) -> String {
    let t = CName(hash).text().to_string();
    if t == "None" {
        String::new()
    } else {
        t
    }
}
/// The game's text for a localization key (a display name's CName), by GetLocalizedTextByKey.
fn localized(key: u64) -> Option<String> {
    if key == 0 {
        return None;
    }
    let f = red4ext_abi::rtti::RttiSystem::get()?.get_function(CName::hashed("GetLocalizedTextByKey"))?;
    // SAFETY: the global function with its CName by value; its String result read while it is held.
    let r = unsafe { crate::script_call::invoke_owned(core::ptr::null_mut(), &f.base, &[Value::U64(key)]) }?;
    if r.ty().name().text() != "String" {
        return None;
    }
    // SAFETY: a String result.
    let t = unsafe { (*(r.as_ptr() as *const red4ext_abi::cstring::CString)).as_str().to_string() };
    (!t.is_empty()).then_some(t)
}
fn weapon_record(id: u64) -> Handle {
    rt::static_obj("TweakDBInterface", "GetWeaponItemRecord", &[A::Name(id)])
}
/// The weapon model a record belongs to (its base appearance preset); none for a record without one.
fn model_of(record: &Handle) -> Option<String> {
    let key = name_text(ret_u64(rt::call(record, "AppearanceResourceName", &[])));
    (!key.is_empty()).then_some(key)
}
fn equipment() -> Handle {
    rt::obj(&super::game_system("GetScriptableSystemsContainer"), "Get", &[A::Name(CName::hashed("EquipmentSystem").0)])
}
fn queue(req: &Handle, pl: &Handle) {
    super::menu_main::set_handle_prop(req, "owner", pl);
    rt::call(&equipment(), "QueueRequest", &[A::H(req)]);
}
fn set_item(req: &Handle, field: &str, item: &Item) {
    if let Some(f) = rt::field(req, field) {
        // SAFETY: an ItemID field, assigned from an ItemID of the same type.
        unsafe { f.ty.assign(f.ptr, item.as_ptr().cast()) };
    }
}
fn slot_item(pl: &Handle, index: usize) -> Item {
    let area = rt::enum_value("gamedataEquipmentArea", "Weapon");
    ret_item(rt::call(&equipment(), "GetItemInEquipSlot", &[A::H(pl), A::N(area), A::N(index as f64)]))
}
fn draw(pl: &Handle, item: &Item) {
    let req = super::lobby_ink::new("DrawItemRequest");
    if req.is_null() {
        return;
    }
    set_item(&req, "itemID", item);
    queue(&req, pl);
}
fn in_hand(wpn: &Handle) -> Item {
    if wpn.is_null() {
        return NONE;
    }
    // SAFETY: the weapon, held.
    unsafe { crate::script_call::method::<Item>(wpn.instance, "GetItemID", &[]) }.unwrap_or(NONE)
}

/// TweakDBInterface.GetRecord of a record's id.
fn record_by_id(id: u64) -> Handle {
    let values = [Value::U64(id)];
    crate::script_call::static_function_for("TweakDBInterface", "GetRecord", &values)
        // SAFETY: a static returning a handle (no context).
        .and_then(|f| unsafe { crate::script_call::invoke_handle(core::ptr::null_mut(), f, &values) })
        .unwrap_or_else(Handle::null)
}

/// The list made a few dozen records a frame.
fn catalog_step(c: &mut Catalog) {
    if c.total == 0 {
        // the weapon records straight from the TweakDB's records by type: TweakXL's GetRecordCount and
        // GetRecordByIndex, which the list read before, are not in a clean game
        let db = red4ext_abi::tweakdb::get();
        c.ids = match crate::script_call::class_any("gamedataWeaponItem_Record") {
            // SAFETY: the live TweakDB and a record class.
            Some(class) if !db.is_null() => unsafe {
                red4ext_abi::tweakdb::record_ids_of_type(db, (class as *const red4ext_abi::rtti::Class).cast())
            },
            _ => Vec::new(),
        };
        c.total = c.ids.len() as u32;
        if c.total == 0 {
            c.built = true;
            log!("[weapon-wrist] the game has no weapon records to list\n");
            return;
        }
    }
    let tag = |r: &Handle, t: &str| ret_bool(rt::call(r, "TagsContains", &[A::Name(CName::hashed(t).0)]));
    let end = (c.cursor + RECORDS_PER_FRAME).min(c.total);
    for i in c.cursor..end {
        let r = record_by_id(c.ids[i as usize]);
        if r.is_null() {
            continue;
        }
        let id = ret_u64(rt::call(&r, "GetID", &[]));
        let Some(name) = super::weapon::tdbid_name(id) else { continue };
        if !name.starts_with("Items.") || name.starts_with("Items.Base_") {
            continue;
        }
        let kind = name_text(ret_u64(rt::call(&rt::obj(&r, "ItemType", &[]), "Name", &[])));
        let Some(kind) = kind.strip_prefix("Wea_") else { continue };
        if matches!(kind, "Fists" | "VehicleMissileLauncher" | "VehiclePowerWeapon") {
            continue;
        }
        if tag(&r, "HideInUI") || tag(&r, "IgnoreInventory") || tag(&r, "Left_Hand") {
            continue;
        }
        let Some(key) = model_of(&r) else { continue };
        // the record drawn for the model: a plain preset of it, not an iconic, not an enemy's or a crafted copy
        let mut score = 0;
        if name.starts_with("Items.Preset_") {
            score += 4;
        }
        if name.ends_with("_Default") {
            score += 8;
        }
        if !tag(&r, "IconicWeapon") {
            score += 2;
        }
        if name.contains("Hard_") || name.contains("Craftable") {
            score -= 6;
        }
        let better = c.models.get(&key).is_none_or(|m| score > m.score || score == m.score && name.len() < m.record_name.len());
        if better {
            let name_key = ret_u64(rt::call(&r, "DisplayName", &[]));
            c.models.insert(
                key.clone(),
                Model { key, kind: kind.to_string(), name: String::new(), record: id, record_name: name, name_key, score },
            );
        }
    }
    c.cursor = end;
    if c.cursor >= c.total {
        for m in c.models.values_mut() {
            m.name = localized(m.name_key).unwrap_or_else(|| m.record_name.trim_start_matches("Items.").to_string());
        }
        // two models of one name (a quest's own look of a weapon): each told by its record
        let mut seen: BTreeMap<String, u32> = BTreeMap::new();
        for m in c.models.values() {
            *seen.entry(m.name.clone()).or_default() += 1;
        }
        for m in c.models.values_mut() {
            if seen.get(&m.name).copied().unwrap_or(0) > 1 {
                m.name = format!("{} ({})", m.name, m.record_name.trim_start_matches("Items.").trim_start_matches("Preset_"));
            }
        }
        let mut order: Vec<&Model> = c.models.values().collect();
        order.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        c.order = order.iter().map(|m| m.key.clone()).collect();
        c.built = true;
        log!("[weapon-wrist] the weapon list: {} models from {} weapon records\n", c.models.len(), c.total);
    }
}

/// A model picked: its record in the hand -- the player's own copy, or one given for the test.
fn pick(s: &mut State, pl: &Handle, wpn: &Handle, record: u64) {
    let label = s.catalog.models.values().find(|m| m.record == record).map_or_else(|| format!("{record:#x}"), |m| m.name.clone());
    if s.restore.is_some() {
        s.status = String::from("Your weapons are being put back: pick again in a moment.");
        return;
    }
    let ts = super::belt::transactions();
    if s.session.is_none() {
        let slots = core::array::from_fn(|i| slot_item(pl, i));
        s.session = Some(Session { slots, drawn: in_hand(wpn), given: Vec::new() });
        log!("[weapon-wrist] the weapon slots noted before the list's first draw\n");
    }
    let data = rt::obj(&ts, "GetItemDataByTDBID", &[A::H(pl), A::Name(record)]);
    let own = if data.is_null() { NONE } else { ret_item(rt::call(&data, "GetID", &[])) };
    let item = if valid(&own) {
        own
    } else {
        let id = ret_item(rt::static_call("ItemID", "FromTDBID", &[A::Name(record)]));
        if !valid(&id) || !ret_bool(rt::call(&ts, "GiveItem", &[A::H(pl), ip(&id), A::N(1.0)])) {
            s.status = format!("{label}: the game did not give it");
            log!("[weapon-wrist] {label}: GiveItem refused\n");
            return;
        }
        if let Some(session) = &mut s.session {
            session.given.push(id);
        }
        id
    };
    draw(pl, &item);
    s.status = format!("{label}: drawn{}", if valid(&own) { " (your own)" } else { " (given for the test)" });
    log!("[weapon-wrist] {label} drawn from the list ({})\n", if valid(&own) { "the player's own" } else { "given" });
}

/// Putting the weapons back, one step a call.
fn restore_step(s: &mut State, pl: &Handle) {
    let Some((step, since)) = s.restore else { return };
    let Some(session) = &s.session else {
        s.restore = None;
        return;
    };
    let t = s.clock;
    match step {
        Restore::Holster => {
            super::holster::equip_request(pl, "UnequipWeapon");
            s.restore = Some((Restore::Slots, t));
        }
        Restore::Slots if t - since > 1.2 => {
            for (i, want) in session.slots.iter().enumerate() {
                let now = slot_item(pl, i);
                if same(&now, want) {
                    continue;
                }
                if valid(want) {
                    let req = super::lobby_ink::new("GameplayEquipRequest");
                    set_item(&req, "itemID", want);
                    super::menu_rt::hset(&req, "slotIndex", i as f64);
                    queue(&req, pl);
                } else {
                    let req = super::lobby_ink::new("UnequipRequest");
                    super::menu_main::set_enum_prop(&req, "areaType", "gamedataEquipmentArea", "Weapon");
                    super::menu_rt::hset(&req, "slotIndex", i as f64);
                    queue(&req, pl);
                }
            }
            s.restore = Some((Restore::Finish, t));
        }
        Restore::Finish if t - since > 0.8 => {
            let ts = super::belt::transactions();
            let kept: Vec<Item> = (0..SLOTS).map(|i| slot_item(pl, i)).collect();
            let mut taken = 0;
            for id in &session.given {
                if kept.iter().any(|k| same(k, id)) {
                    continue;
                }
                if ret_bool(rt::call(&ts, "RemoveItem", &[A::H(pl), ip(id), A::N(1.0)])) {
                    taken += 1;
                }
            }
            // the hands as they were: the weapon held then drawn again; empty, the weapon the game drew as its slot came
            // back put away
            if valid(&session.drawn) {
                draw(pl, &session.drawn);
            } else {
                super::holster::equip_request(pl, "UnequipWeapon");
            }
            s.status = format!("Your weapon slots are as they were; {taken} weapon(s) given for the test taken back.");
            log!("[weapon-wrist] the weapons put back: the slots equipped again, {taken} copy(ies) taken back\n");
            s.session = None;
            s.restore = None;
        }
        _ => {}
    }
}

/// The weapon frame, every frame with a player: the player, the weapon in hand (`wpn` null for none) and its entity's
/// hash.
pub(super) fn frame(pl: &Handle, wpn: &Handle, entity: Option<u64>, dt: f64) {
    let mut g = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let s = &mut *g;
    load(s);
    s.clock += dt;
    if s.want_catalog && !s.catalog.built {
        catalog_step(&mut s.catalog);
    }
    if let Some(record) = s.pick.take() {
        pick(s, pl, wpn, record);
    }
    restore_step(s, pl);
    if s.entity != entity {
        s.entity = entity;
        let item = in_hand(wpn);
        if !wpn.is_null() && !valid(&item) {
            crate::audit::once(crate::audit::key(&[0x5757, 1]), || "weapon wrist: the weapon's GetItemID did not answer".to_string());
        }
        // the record's name; its id when the game has no name for it
        s.record = valid(&item).then(|| super::weapon::tdbid_name(item[0]).unwrap_or_else(|| format!("TDBID:{:010X}", item[0] & 0xFF_FFFF_FFFF)));
        s.key = if valid(&item) { model_of(&weapon_record(item[0])).or_else(|| s.record.clone()) } else { None };
        // a turn kept under the record's name moves to its model
        if let (Some(k), Some(r)) = (&s.key, &s.record) {
            if k != r && s.turns.get(k).is_none_or(|t| *t == default_of(k)) {
                if let Some(t) = s.turns.remove(r) {
                    s.turns.insert(k.clone(), t);
                    s.dirty = true;
                    s.since_change = 0.0;
                    log!("[weapon-wrist] {r}'s turn now its model's, {k}\n");
                }
            }
        }
        publish(s);
        if let Some(k) = &s.key {
            if let Some(t) = s.turns.get(k) {
                cvr_host::vlog!("[weapon-wrist] {k}: the wrist turned pitch {:.1} yaw {:.1} roll {:.1}\n", t[0], t[1], t[2]);
            }
        }
    }
    if s.dirty {
        s.since_change += dt;
        if s.since_change >= SAVE_AFTER {
            save(s);
        }
    }
}

/// The windows closed: a change is written now.
pub(super) fn closed() {
    let mut g = STATE.lock().unwrap_or_else(|p| p.into_inner());
    save(&mut g);
}

// ---------------------------------------------------------------- the window
/// One turn's row: a slider and the step buttons (held, they repeat).
fn row(label: &str, id: &str, v: &mut f32, width: f32) -> bool {
    use cvr_render::imgui;
    let mut changed = false;
    let c = |s: &str| std::ffi::CString::new(s).unwrap_or_default();
    imgui::push_id_str(&c(id));
    imgui::push_button_repeat(true);
    if imgui::button(c"-", [0.0, 0.0]) {
        *v -= STEP;
        changed = true;
    }
    imgui::same_line(0.0, -1.0);
    imgui::set_next_item_width(width);
    if imgui::slider_float(c"##v", v, -RANGE, RANGE, c"%.1f", 0) {
        changed = true;
    }
    imgui::same_line(0.0, -1.0);
    if imgui::button(c"+", [0.0, 0.0]) {
        *v += STEP;
        changed = true;
    }
    imgui::pop_button_repeat();
    imgui::same_line(0.0, -1.0);
    imgui::text_unformatted(&c(label));
    imgui::pop_id();
    if changed {
        // the buttons' tenths kept exact
        *v = ((*v * 10.0).round() / 10.0).clamp(-RANGE, RANGE);
    }
    changed
}

/// The window's first size, as fractions of the frame.
const SIZE: [f32; 2] = [0.27, 0.62];
/// The name a model's key is shown by: the list's when it is made.
fn shown(s: &State, key: &str) -> String {
    s.catalog.models.get(key).map_or_else(|| key.to_string(), |m| m.name.clone())
}
/// The window, on the render thread while the overlay is open.
pub(super) fn dev_window(frame: [f32; 2]) {
    super::dev_windows::window("VR weapon grip", [0.71, 0.05], SIZE, frame, || {
        let mut g = STATE.lock().unwrap_or_else(|p| p.into_inner());
        let s = &mut *g;
        load(s);
        s.want_catalog = true;
        if let Some(key) = s.key.clone() {
            super::dev_windows::text(&format!("{}  ({key})", shown(s, &key)));
            super::dev_windows::wrapped("The hand that holds this weapon, turned after the wrist calibration (degrees). Kept for every weapon of this model.");
            let mut t = s.turns.get(&key).copied().unwrap_or([0.0; 3]);
            let mut changed = false;
            // the sliders as wide as the window leaves beside the two buttons and the label
            let font = cvr_render::imgui::font_size();
            let width = (cvr_render::imgui::content_region_avail()[0] - font * 8.0).max(font * 8.0);
            changed |= row("Pitch", "pitch", &mut t[0], width);
            changed |= row("Yaw", "yaw", &mut t[1], width);
            changed |= row("Roll", "roll", &mut t[2], width);
            if super::dev_windows::button("Reset this weapon") {
                t = default_of(&key);
                changed = true;
            }
            if changed {
                s.turns.insert(key, t);
                s.dirty = true;
                s.since_change = 0.0;
                publish(s);
            }
        } else {
            super::dev_windows::wrapped("Pick a weapon below (or draw one): its wrist turn is set here (pitch, yaw, roll; 0.1 degree a press).");
        }
        weapon_list(s);
        others(s);
    });
}
/// Every weapon model: picked, it is drawn into the hand.
fn weapon_list(s: &mut State) {
    use cvr_render::imgui;
    super::dev_windows::separator();
    if !s.catalog.built {
        let (done, total) = (s.catalog.cursor, s.catalog.total);
        super::dev_windows::text(&format!("Reading the game's weapons... {done} / {total}"));
        return;
    }
    super::dev_windows::text(&format!("Weapons ({} models): pick one to hold it", s.catalog.order.len()));
    imgui::set_next_item_width(imgui::content_region_avail()[0]);
    super::dev_windows::input("##filter", &mut s.filter, 48);
    let filter = s.filter.trim().to_lowercase();
    // the list fills the window but for the status and the button under it
    let below = imgui::frame_height() * if s.session.is_some() { 2.6 } else { 1.4 };
    imgui::begin_child(c"##weapons", [0.0, -below], 1, 0);
    let mut picked = None;
    let mut kind = String::new();
    let mut open = true;
    for key in &s.catalog.order {
        let Some(m) = s.catalog.models.get(key) else { continue };
        if !filter.is_empty() {
            let hay = format!("{} {} {} {}", m.name, m.key, m.record_name, m.kind).to_lowercase();
            if !hay.contains(&filter) {
                continue;
            }
        } else if m.kind != kind {
            // a tree per kind while nothing is typed
            if !kind.is_empty() && open {
                imgui::tree_pop();
            }
            kind = m.kind.clone();
            open = imgui::tree_node(&std::ffi::CString::new(kind.clone()).unwrap_or_default());
        }
        if filter.is_empty() && !open {
            continue;
        }
        let turn = s.turns.get(key).filter(|t| t.iter().any(|v| *v != 0.0));
        let label = match turn {
            Some(t) => format!("{}   [{:.1} {:.1} {:.1}]##{}", m.name, t[0], t[1], t[2], m.key),
            None => format!("{}##{}", m.name, m.key),
        };
        let held = s.key.as_deref() == Some(key.as_str());
        let c = std::ffi::CString::new(label.replace('\0', "")).unwrap_or_default();
        if imgui::selectable(c.as_ptr(), held, 0, [0.0, 0.0]) {
            picked = Some(m.record);
        }
    }
    if filter.is_empty() && !kind.is_empty() && open {
        imgui::tree_pop();
    }
    imgui::end_child();
    if let Some(record) = picked {
        s.pick = Some(record);
    }
    if !s.status.is_empty() {
        super::dev_windows::text(&s.status);
    }
    if let Some(session) = &s.session {
        let label = format!("Put my weapons back ({} given for the test)", session.given.len());
        if s.restore.is_none() && super::dev_windows::button(&label) {
            s.restore = Some((Restore::Holster, s.clock));
            s.status = String::from("Putting your weapons back...");
        }
    }
}
/// The other weapons turned otherwise than built in, each with its reset to the built-in turn.
fn others(s: &mut State) {
    let others: Vec<(String, [f32; 3])> =
        s.turns.iter().filter(|(k, t)| Some(*k) != s.key.as_ref() && **t != default_of(k)).map(|(k, t)| (k.clone(), *t)).collect();
    if others.is_empty() {
        return;
    }
    super::dev_windows::separator();
    let mut reset = None;
    super::dev_windows::tree(&format!("Other weapons changed from the built-in turn ({})", others.len()), || {
        for (k, t) in &others {
            super::dev_windows::text(&format!("{}: pitch {:.1}, yaw {:.1}, roll {:.1}", shown(s, k), t[0], t[1], t[2]));
            super::dev_windows::same_line();
            if super::dev_windows::small(&format!("Reset##{k}")) {
                reset = Some(k.clone());
            }
        }
    });
    if let Some(k) = reset {
        s.turns.insert(k.clone(), default_of(&k));
        s.dirty = true;
        s.since_change = 0.0;
    }
}
