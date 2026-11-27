//! CyberpunkVRPort_Menu/modules/wheel.lua (+ wheel_art.lua): the weapon wheel -- the game's radial wheel without the
//! pause. Hold Y and a small wheel hangs in front of the body past an outstretched arm; point at a cell with either
//! hand (the ray, or the fingertip up close) and let go of Y. The plugin takes the hold from the game
//! (VRWeaponWheel: 0 none, 1 pressed and undecided, 2 a hold); the wheel arms only where the game's own would open
//! (WeaponWheelDecisions, a choice on Y, the menu closed). The cells are the game's: the three weapon slots as wedges
//! across the top, the arm cyberware (the fists without one) on the right, the holster on the left, the grenades and
//! the consumables as round cells below, their choices out on an arc; applied with the game's own requests.
use super::lobby_ink as ink;
use super::lobby_rtti::{self as rt, Val, A};
use super::lobby_spawn::{self as sp, add, cross, dot, mul, norm, sub, V3};
use super::menu_invdata::{self as idt, Item};
use super::menu_ink as k;
use super::menu_locale as loc;
use super::menu_rt::*;
use crate::script_call::Value;
use red4ext_abi::handle::Handle;
use std::collections::HashSet;
use std::sync::Mutex;

const ENT: &str = "vrport\\ui\\vr_menu.ent";
const BEAM_ENT: &str = "vrport\\ui\\vr_menu_beam.ent";
const DISC_ENT: &str = "vrport\\ui\\vr_menu_icon.ent";
const UI: &str = "vrp_menu_ui";
const SHADOW: &str = "base\\gameplay\\gui\\fullscreen\\inventory\\shadow_icons.inkatlas";
const ATLAS: &str = "vrport\\ui\\vr_wheel.inkatlas";
const CW: f64 = 3200.0;
const CH: f64 = 1600.0;
const PX: f64 = 3200.0;
const W: f64 = CW / PX;
const H: f64 = CH / PX;
const CX: f64 = CW / 2.0;
const CY: f64 = CH / 2.0;
const BEAM_LEN: f64 = 1.5;
const R_HUB: f64 = 135.0;
const R_RIM: f64 = 508.0;
const R_CELLS: f64 = 335.0;
const R_CELL: f64 = 76.0;
const R_ARC: f64 = 625.0;
const R_CHOICE: f64 = 64.0;
const ARC_STEP: f64 = 17.0;
const MAX_CHOICES: usize = 9;
const ARC_KEEP: f64 = 0.35;
const SMOKE_WAIT: f64 = 3.0;
const CONSUME_WAIT: f64 = 1.0;
const CIGARETTE: &str = "UIIcon.base_junk_item_cigarette_pack_a";
const GLASS_BACK: f64 = 0.006;
const REACH: f64 = 0.90;
const LIFT: f64 = 0.20;
/// THE WHEEL'S OWN FREE-LOOK CONE (main's 2026-11-07): the wheel keeps its own heading -- the body's when it opens -- and
/// stays while the head looks within this of it; past that it follows the excess only, a leash at the cone's edge (the
/// body turns after the head, and every look at a wedge used to turn the wheel away with it).
const FREE_LOOK_DEG: f64 = 10.0;
const SIDE_MIN: f64 = 0.10;
const SIDE_MAX: f64 = 0.35;
const SIDE_DEFAULT: f64 = 0.18;
const HOVER: f64 = 0.10;
const THROUGH: f64 = -0.05;
/// Seconds with a player in the world before the wheel's entities and inventory manager are made (ahead of the first
/// Y press, as the game's radial wheel spawns its widgets when the HUD starts).
const WARM_AFTER: f64 = 3.0;
/// How far below the body the entities wait until the wheel first opens (out of sight while the display comes on).
const HIDE_DEPTH: f64 = 40.0;
/// The inventory scan's time a frame.
const SCAN_BUDGET: std::time::Duration = std::time::Duration::from_micros(400);
/// An item came to the player or left (game_hooks.rs): the hotkey items are looked for again.
static INVENTORY_CHANGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub(super) fn inventory_changed() {
    INVENTORY_CHANGED.store(true, std::sync::atomic::Ordering::Relaxed);
}
const HAPTIC_CLICK: i32 = 20;

/// wheel_art.lua's parts: (name, w, h, ox, oy).
const PARTS: [(&str, f64, f64, f64, f64); 11] = [
    ("arm_band", 350.0, 297.0, 175.0, 466.0),
    ("arm_edge", 350.0, 297.0, 175.0, 466.0),
    ("arm_fill", 350.0, 297.0, 175.0, 466.0),
    ("disc_edge", 184.0, 184.0, 92.0, 92.0),
    ("disc_fill", 184.0, 184.0, 92.0, 92.0),
    ("holster_band", 273.0, 253.0, 136.5, 441.0),
    ("holster_edge", 273.0, 253.0, 136.5, 441.0),
    ("holster_fill", 273.0, 253.0, 136.5, 441.0),
    ("weapon_band", 435.0, 360.0, 217.5, 496.0),
    ("weapon_edge", 435.0, 360.0, 217.5, 496.0),
    ("weapon_fill", 435.0, 360.0, 217.5, 496.0),
];
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Weapon(i32),
    Cyberware,
    Misc(&'static str),
    Holster,
}
struct Sector {
    kind: Kind,
    shape: Option<&'static str>,
    angle: f64,
    empty: Option<&'static str>,
    inner: f64,
    outer: f64,
    half: f64,
}
const WEAPON: (f64, f64, f64) = (170.0, 480.0, 26.0);
const ARM: (f64, f64, f64) = (200.0, 450.0, 22.0);
const HOLSTER: (f64, f64, f64) = (215.0, 425.0, 18.0);
const NONE: (f64, f64, f64) = (0.0, 0.0, 0.0);
const fn sector(kind: Kind, shape: Option<&'static str>, g: (f64, f64, f64), angle: f64, empty: Option<&'static str>) -> Sector {
    Sector { kind, shape, angle, empty, inner: g.0, outer: g.1, half: g.2 }
}
static SECTORS: [Sector; 7] = [
    sector(Kind::Weapon(0), Some("weapon"), WEAPON, -52.0, Some("gun")),
    sector(Kind::Weapon(1), Some("weapon"), WEAPON, 0.0, Some("gun")),
    sector(Kind::Weapon(2), Some("weapon"), WEAPON, 52.0, Some("gun")),
    sector(Kind::Cyberware, Some("arm"), ARM, 108.0, Some("cyberware")),
    sector(Kind::Misc("RB"), None, NONE, 157.0, Some("granade")),
    sector(Kind::Misc("DPAD_UP"), None, NONE, 203.0, Some("consumable")),
    sector(Kind::Holster, Some("holster"), HOLSTER, 252.0, None),
];

#[derive(Clone)]
struct Choice {
    item: Option<Item>,
    smoke: bool,
    lit: bool,
    name: String,
    image: Option<super::menu_data::Icon>,
    key: String,
    quantity: f64,
    quality: String,
}
#[derive(Clone, Default)]
struct Entry {
    item: Option<Item>,
    blocked: bool,
    active: bool,
    fists: bool,
    hotkey: f64,
    choices: Vec<Choice>,
}
#[derive(Default)]
struct Surf {
    id: Option<u64>,
    entity: Handle,
    comp: Handle,
    root: Handle,
    built: bool,
    gpu: bool,
    shown: Option<bool>,
    /// The display switched on once (then the canvas's opacity hides it) and when it last showed.
    display_on: bool,
    shown_at: Option<f64>,
    scale: Option<f64>,
    strip: Vec<(usize, f64, f64, Handle)>,
    strip_px: f64,
}
struct Cell {
    fill: Handle,
    band: Handle,
    glow: Handle,
    edge: Vec<Handle>,
    content: Handle,
    cx: f64,
    cy: f64,
    mark: (f64, f64),
    empty: bool,
    blocked: bool,
    col: [f64; 3],
}
struct ChoiceCell {
    x: f64,
    y: f64,
    current: bool,
    glow: Handle,
    edge: Vec<Handle>,
}
#[derive(Default)]
struct Frame {
    warm: Handle,
    hub_glow: Handle,
    hub_ring: Vec<Handle>,
    hub_mark: Vec<Handle>,
    hub_name: Handle,
    hub_info: Handle,
    cells: Vec<Cell>,
    choice_layer: Handle,
    choice_cells: Vec<ChoiceCell>,
    cursor: Handle,
}
#[derive(Clone, Copy, PartialEq)]
enum Hover {
    Hub,
    Cell(usize),
    Choice(usize, usize),
}
struct Fit {
    widget: Handle,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    op: f64,
}
#[derive(Default)]
struct Wheel {
    time: f64,
    open: bool,
    screen: Surf,
    frame: Option<Frame>,
    beam: Surf,
    glass: Surf,
    choice_glass: Vec<Surf>,
    ctx: Option<idt::Ctx>,
    anchor: Option<(V3, V3, V3)>,
    /// The wheel's heading (world, radians): the body's at the opening, then the head's cone.
    yaw: Option<f64>,
    slots: Vec<Entry>,
    hover: Option<Hover>,
    hover_painted: bool,
    arc: Option<usize>,
    arc_shown: Option<Option<usize>>,
    arc_away: Option<f64>,
    /// Seconds with a player in the world.
    present: f64,
    /// The hotkey cells' items: the scan under way, and the last one finished (None: stale).
    scan: Option<idt::HotkeyScan>,
    hits: Option<Vec<(f64, Handle)>>,
    allowed_at: f64,
    allowed: bool,
    hand: Option<usize>,
    fits: Vec<Fit>,
    filled: bool,
    smoke_at: Option<f64>,
    consume_at: Option<(f64, String)>,
}
// SAFETY: the handles are made, used and dropped on the game's main thread, where the mechanics tick.
unsafe impl Send for Wheel {}
static WHEEL: Mutex<Option<Wheel>> = Mutex::new(None);

fn play(kind: &str) {
    let file = match kind {
        "open" => "open.wav",
        "close" => "close.wav",
        "click" => "click.wav",
        _ => return,
    };
    let f = sp::Str::new(&format!("CyberpunkVRPort_Menu/sounds/{file}"));
    sp::native::<bool>("VRMenuPlayWav", &[f.v(), Value::Float(super::menu_data::effects_volume() as f32)]);
}
fn haptic(hand: Option<usize>) {
    if let Some(h) = hand {
        sp::native::<()>("VRHapticEvent", &[Value::Int32(HAPTIC_CLICK), Value::Int32(if h == 1 { 1 } else { 2 })]);
    }
}
fn pos(angle: f64, r: f64) -> (f64, f64) {
    let a = angle.to_radians();
    (CX + a.sin() * r, CY - a.cos() * r)
}
fn turn(a: f64, b: f64) -> f64 {
    (b - a + 540.0).rem_euclid(360.0) - 180.0
}
fn quality_color(q: &str) -> Option<[f64; 3]> {
    if q.is_empty() {
        return None;
    }
    let base = q.replace("Plus", "");
    match base.as_str() {
        "Common" | "Uncommon" | "Rare" | "Epic" | "Legendary" | "Iconic" => Some(k::quality(&base)),
        _ => None,
    }
}

// ---------------------------------------------------------------- the game's conditions
fn tagged(pl: &Handle, tag: &str) -> bool {
    sb("StatusEffectSystem", "ObjectHasStatusEffectWithTag", &[A::H(pl), A::Name(cname(tag))])
}
fn bb_field(name: &str) -> Option<*mut std::ffi::c_void> {
    let defs = super::boards::defs().ok()?;
    let dd = super::boards::definition(&defs, "PlayerStateMachine").ok()?;
    super::boards::field(dd, name).ok()
}
fn psm(pl: &Handle) -> Handle {
    let Ok(defs) = super::boards::defs() else { return Handle::null() };
    let Ok(dd) = super::boards::definition(&defs, "PlayerStateMachine") else { return Handle::null() };
    let eid = id(pl, "GetEntityID", &[]);
    super::boards::local_board(eid, dd).unwrap_or_else(|_| Handle::null())
}
fn bb_int(bb: &Handle, f: &str) -> i32 {
    bb_field(f).and_then(|id| super::boards::get::<i32>(bb, "GetInt", id).ok()).unwrap_or(0)
}
fn bb_bool(bb: &Handle, f: &str) -> bool {
    bb_field(f).and_then(|id| super::boards::get::<bool>(bb, "GetBool", id).ok()).unwrap_or(false)
}
const Y_CHOICES: [&str; 4] = ["Choice2", "Choice2_Hold", "Choice2_Release", "PickUpBody"];
fn on_y(n: u64) -> bool {
    Y_CHOICES.iter().any(|c| cname(c) == n)
}
/// Y is also the interactions' second choice: while the interaction on screen offers a choice on it, Y stays the game's.
fn choice_on_y() -> bool {
    let Ok(defs) = super::boards::defs() else { return false };
    let Ok(dd) = super::boards::definition(&defs, "UIInteractions") else { return false };
    let Ok(bb) = super::boards::system_board(dd) else { return false };
    let read = |name: &str| -> Option<crate::script_call::Owned> {
        let f = super::boards::field(dd, name).ok()?;
        rt::call(&bb, "GetVariant", &[A::P(f.cast_const())])
    };
    let any = |list: Option<Val>, field: &str| list.is_some_and(|l| items(&l).iter().any(|c| on_y(fid(c, field))));
    if let Some(v) = read("InteractionChoiceHub") {
        if let Some(hub) = variant_inner(&v) {
            if fbool(&hub, "active") && any(hub.f("choices"), "inputAction") {
                return true;
            }
        }
    }
    if let Some(v) = read("DialogChoiceHubs") {
        if let Some(lists) = variant_inner(&v) {
            if let Some(hubs) = lists.f("choiceHubs") {
                if items(&hubs).iter().any(|h| any(h.f("choices"), "inputActionName")) {
                    return true;
                }
            }
        }
    }
    if let Some(v) = read("LootData") {
        if let Some(loot) = variant_inner(&v) {
            if fbool(&loot, "isActive") && any(loot.f("choices"), "inputAction") {
                return true;
            }
        }
    }
    false
}
fn on_ladder(bb: &Handle) -> bool {
    let v = f64::from(bb_int(bb, "LocomotionDetailed"));
    ["Ladder", "LadderSprint", "LadderSlide", "LadderJump"].iter().any(|n| ev("gamePSMDetailedLocomotionStates", n) == v)
}
fn allowed_now(pl: &Handle, menu_open: bool) -> bool {
    if menu_open || choice_on_y() || super::menu_main::paused() {
        return false;
    }
    if super::requests_handler().ok().is_some_and(|rh| b(&rh, "IsPreGame", &[])) {
        return false;
    }
    if ["NoRadialMenus", "NoWeaponWheel", "FocusModeLocomotion"].iter().any(|t| tagged(pl, t)) || b(pl, "IsReplacer", &[]) {
        return false;
    }
    let bb = psm(pl);
    if bb.is_null() {
        return false;
    }
    if bb_int(&bb, "SceneTier") >= 3 || bb_int(&bb, "Swimming") > 0 || on_ladder(&bb) || bb_int(&bb, "CombatGadget") > 0 {
        return false;
    }
    let vehicle = bb_int(&bb, "Vehicle");
    if vehicle == 4 {
        return false;
    }
    if bb_bool(&bb, "MountedToVehicle") && vehicle != 2 && !bb_bool(&bb, "MountedToCombatVehicle") {
        return false;
    }
    !(vehicle == 1 && !sb("VehicleTransition", "CanEnterDriverCombat", &[]))
}
fn weapons_blocked(pl: &Handle) -> bool {
    ["NoCombat", "FirearmsNoSwitch", "ShootingRangeCompetition", "Fists", "FirearmsNoUnequipNoSwitch"].iter().any(|t| tagged(pl, t))
}

// ---------------------------------------------------------------- what is on it
fn in_hand(pl: &Handle) -> HashSet<String> {
    let ts = super::game_system("GetTransactionSystem");
    let mut keys = HashSet::new();
    for slot in ["AttachmentSlots.WeaponRight", "AttachmentSlots.WeaponLeft"] {
        let item = o(&ts, "GetItemInSlot", &[A::H(pl), A::Name(tdb(slot))]);
        if !item.is_null() {
            keys.insert(idt::key_of(&id16(&item, "GetItemID", &[])));
        }
    }
    keys
}
fn choice_of(it: Item) -> Choice {
    Choice { name: it.name.clone(), image: it.image.clone(), key: it.key.clone(), quantity: it.quantity, quality: it.quality.clone(), smoke: false, lit: false, item: Some(it) }
}
/// A hotkey's scope (Hotkey.GetScope: its item types in order).
fn scope(hotkey: &str) -> Vec<f64> {
    rt::static_call("Hotkey", "GetScope", &[A::N(ev("EHotkey", hotkey))]).map(|s| items(&Val::of(&s)).iter().filter_map(|k| k.num()).collect()).unwrap_or_default()
}
/// Every hotkey cell's types: what the inventory scan looks for.
fn hotkey_kinds() -> Vec<f64> {
    let mut kinds = Vec::new();
    for s in SECTORS.iter() {
        if let Kind::Misc(hk) = s.kind {
            kinds.extend(scope(hk));
        }
    }
    kinds
}
fn hotkey_entry(ctx: &mut idt::Ctx, e: &mut Entry, hotkey: &str, hits: &[(f64, Handle)]) {
    e.hotkey = ev("EHotkey", hotkey);
    let mut seen = HashSet::new();
    // RadialWheelController.GetValidItemsForMiscSlot: the scope's types in order, each type's items in the inventory's
    for kind in scope(hotkey) {
        for (_, d) in hits.iter().filter(|(ty, _)| *ty == kind) {
            if let Some(it) = idt::hotkey_item(ctx, kind, d) {
                if seen.insert(it.key.clone()) {
                    e.choices.push(choice_of(it));
                }
            }
        }
    }
    let current = rt::call(&ctx.manager, "GetHotkeyItemData", &[A::N(e.hotkey)]).and_then(|d| idt::item_kept(ctx, d));
    if let Some(cur) = current {
        e.item = e.choices.iter().find(|c| c.key == cur.key).and_then(|c| c.item.clone());
    }
    if e.item.is_none() {
        e.item = e.choices.first().and_then(|c| c.item.clone());
    }
}
fn smoke_choice(pl: &Handle) -> Option<Choice> {
    let has = super::smoke::has_cig(pl);
    Some(Choice {
        item: None,
        smoke: true,
        lit: has,
        name: if has { loc::tr("Убрать сигарету", "Put the cigarette away") } else { loc::tr("Сигарета", "Cigarette") },
        image: super::menu_gpdata::image(tdb(CIGARETTE)),
        key: "vrsmoke".into(),
        quantity: 0.0,
        quality: String::new(),
    })
}
fn base_fists(ctx: &mut idt::Ctx, pl: &Handle) -> Option<Item> {
    let ids = rt::static_call("EquipmentSystem", "GetItemsInArea", &[A::H(pl), A::N(idt::area("BaseFists"))])?;
    let first = Val::of(&ids).at(0).map(|v| Id16::of(&v))?;
    if let Some(it) = idt::kept(ctx, &first) {
        return Some(it);
    }
    let data = o(&super::game_system("GetTransactionSystem"), "GetItemData", &[A::H(pl), first.a()]);
    if data.is_null() {
        return None;
    }
    let d = rt::call(&ctx.manager, "GetInventoryItemData", &[A::H(pl), A::H(&data), A::B(false), A::B(true)])?;
    idt::item_kept(ctx, d)
}
fn gather(ctx: &mut idt::Ctx, pl: &Handle, hits: &[(f64, Handle)]) -> Vec<Entry> {
    let held = in_hand(pl);
    let blocked = weapons_blocked(pl);
    let mut out = Vec::new();
    for s in SECTORS.iter() {
        let mut e = Entry::default();
        match s.kind {
            Kind::Weapon(slot) => {
                e.item = idt::equipped_kept(ctx, "Weapon", slot);
                e.blocked = blocked;
            }
            Kind::Cyberware => {
                let mut cw = idt::equipped_kept(ctx, "ArmsCW", 0);
                if cw.as_ref().is_some_and(|c| c.ty == ev("gamedataItemType", "Cyb_Launcher")) {
                    cw = None;
                }
                let strong = cw.as_ref().is_some_and(|c| c.ty == ev("gamedataItemType", "Cyb_StrongArms"));
                let mut item = cw.clone();
                if cw.is_none() || (!strong && cw.as_ref().is_some_and(|c| held.contains(&c.key))) {
                    item = base_fists(ctx, pl).or(cw.clone());
                }
                if let Some(it) = item {
                    e.fists = cw.as_ref().is_none_or(|c| c.key != it.key);
                    e.blocked = blocked || (!e.fists && tagged(pl, "NoArmsCW") && !strong);
                    e.item = Some(it);
                }
            }
            Kind::Holster => e.blocked = tagged(pl, "FirearmsNoUnequip"),
            Kind::Misc(hk) => {
                hotkey_entry(ctx, &mut e, hk, hits);
                if hk == "DPAD_UP" {
                    if let Some(sm) = smoke_choice(pl) {
                        e.choices.insert(0, sm);
                    }
                }
            }
        }
        e.active = e.item.as_ref().is_some_and(|i| held.contains(&i.key));
        out.push(e);
    }
    out
}

// ---------------------------------------------------------------- applying
fn equipment() -> Handle {
    o(&super::game_system("GetScriptableSystemsContainer"), "Get", &[A::Name(cname("EquipmentSystem"))])
}
fn manipulation(pl: &Handle, action: &str) {
    let r = ink::new("EquipmentSystemWeaponManipulationRequest");
    super::menu_main::set_handle_prop(&r, "owner", pl);
    super::menu_main::set_enum_prop(&r, "requestType", "EquipmentManipulationAction", action);
    super::menu_main::set_enum_prop(&r, "equipAnimType", "gameEquipAnimationType", "Default");
    rt::call(&equipment(), "QueueRequest", &[A::H(&r)]);
}
fn has_cig(pl: &Handle) -> bool {
    super::smoke::has_cig(pl)
}
fn smoke(w: &mut Wheel, pl: &Handle) -> bool {
    if has_cig(pl) {
        super::smoke::toggle(pl);
    } else if !in_hand(pl).is_empty() {
        manipulation(pl, "UnequipAll");
        w.smoke_at = Some(w.time);
    } else {
        super::smoke::toggle(pl);
    }
    true
}
fn consume(pl: &Handle) {
    let up = ev("EHotkey", "DPAD_UP");
    if static_gi("PlayerGameplayRestrictions", "IsHotkeyRestricted", &[A::N(up)]).is_some_and(|o| Val::of(&o).bool()) {
        let n0 = ink::new("UIInGameNotificationEvent");
        hset(&n0, "m_notificationType", ev("UIInGameNotificationType", "ActionRestriction"));
        rt::call(&super::game_system("GetUISystem"), "QueueEvent", &[A::H(&n0)]);
        return;
    }
    let evd = ink::new("DPADActionPerformed");
    hset(&evd, "action", up);
    hset(&evd, "successful", 1.0);
    rt::call(&super::game_system("GetUISystem"), "QueueEvent", &[A::H(&evd)]);
    manipulation(pl, "RequestConsumable");
}
fn apply(w: &mut Wheel, hover: Option<Hover>) -> bool {
    let pl = player();
    let (Some(hv), false) = (hover, pl.is_null()) else { return false };
    let (index, choice) = match hv {
        Hover::Hub => return false,
        Hover::Cell(i) => (i, None),
        Hover::Choice(i, c) => (i, Some(c)),
    };
    let Some(e) = w.slots.get(index).cloned() else { return false };
    let s = &SECTORS[index];
    if let Kind::Misc(hk) = s.kind {
        let Some(c) = choice.and_then(|c| e.choices.get(c).cloned()) else { return false };
        if c.smoke {
            return smoke(w, &pl);
        }
        let consumable = hk == "DPAD_UP";
        let Some(it) = c.item.clone() else { return false };
        if e.item.as_ref().is_none_or(|cur| c.key != cur.key) {
            let r = so("HotkeyAssignmentRequest", "Construct", &[it.id.a(), A::N(e.hotkey), A::H(&pl), A::N(ev("EHotkeyRequestType", "Assign"))]);
            if r.is_null() || !b(&r, "IsValid", &[]) {
                return false;
            }
            rt::call(&equipment(), "QueueRequest", &[A::H(&r)]);
        } else if !consumable {
            return false;
        }
        if consumable {
            w.consume_at = Some((w.time, idt::key_of(&it.id)));
        }
        return true;
    }
    if e.blocked {
        return false;
    }
    if s.kind == Kind::Holster {
        manipulation(&pl, "UnequipAll");
        return true;
    }
    let Some(it) = e.item.as_ref().filter(|_| !e.active) else { return false };
    let r = ink::new("DrawItemRequest");
    super::menu_main::set_handle_prop(&r, "owner", &pl);
    idt::hset_id(&r, "itemID", &it.id);
    rt::call(&equipment(), "QueueRequest", &[A::H(&r)]);
    true
}

// ---------------------------------------------------------------- entities
fn spawn(path: &str, c: V3, r: V3, u: V3) -> Surf {
    Surf { id: super::reload_core::spawn_at(path, c, sp::quad_pose(r, u)), ..Default::default() }
}
fn despawn(h: &mut Surf) {
    if let Some(id) = h.id.take() {
        super::reload_core::despawn(id);
    }
    *h = Surf::default();
}
fn resolve(h: &mut Surf, component: &str) -> bool {
    if h.id.is_none() {
        return false;
    }
    let done = |h: &Surf| !h.comp.is_null() && (component != UI || !h.root.is_null());
    if done(h) {
        return true;
    }
    h.entity = sp::find_entity(h.id);
    if !h.entity.is_null() {
        h.comp = sp::find_comp(&h.entity, component);
    }
    if !h.comp.is_null() && component == UI {
        h.root = ink::child(&h.comp, "GetWidget", &[]);
    }
    done(h)
}
fn place(h: &Surf, c: V3, r: V3, u: V3) {
    if !h.entity.is_null() {
        sp::set_world(&h.entity, c, sp::quad_pose(r, u));
    }
}
/// The screen's display, on only after the plugin reports a finished GPU write of the built canvas. Once on it STAYS on
/// and the canvas's opacity hides the wheel (main's 612bd89fc): switched off and on, the display built a new render
/// proxy that starts from the material's defaults -- an opaque black plane the size of the screen -- until the next
/// update binds the canvas (Natives/MenuSurface.cpp, Rebind), the dark window a second opening sometimes began with.
fn display(h: &mut Surf, on: bool, time: f64) {
    if h.entity.is_null() || h.shown == Some(on) {
        return;
    }
    if on && !h.display_on {
        super::belt::toggle(&sp::find_comp(&h.entity, "display"), true);
        h.display_on = true;
    }
    if !h.root.is_null() {
        ink::opacity(&h.root, if on { 1.0 } else { 0.0 });
    }
    h.shown = Some(on);
    h.shown_at = on.then_some(time);
}
fn show_glass(h: &mut Surf, on: bool, scale: Option<f64>) {
    if !resolve(h, "display") {
        return;
    }
    if let Some(sc) = scale.filter(|s| h.scale != Some(*s)) {
        // SAFETY: the mesh component's Vector3 property.
        if let Some(p) = unsafe { crate::script_call::property(h.comp.instance, "visualScale") } {
            unsafe { *(p as *mut [f32; 3]) = [sc as f32, sc as f32, 1.0] };
        }
        super::belt::toggle(&h.comp, false);
        (h.scale, h.shown) = (Some(sc), Some(false));
    }
    if h.shown != Some(on) {
        super::belt::toggle(&h.comp, on);
        h.shown = Some(on);
    }
}
fn release(w: &mut Wheel) {
    despawn(&mut w.screen);
    despawn(&mut w.beam);
    despawn(&mut w.glass);
    for g in w.choice_glass.iter_mut() {
        despawn(g);
    }
    w.choice_glass.clear();
    if let Some(c) = w.ctx.take() {
        idt::close(c);
    }
    w.fits.clear();
    w.frame = None;
    (w.open, w.hover, w.arc, w.anchor, w.yaw) = (false, None, None, None, None);
}

// ---------------------------------------------------------------- the body's frame
fn body() -> Option<(V3, [f64; 4])> {
    let pl = player();
    if pl.is_null() {
        return None;
    }
    let p = super::belt::m::<[f32; 4]>(&pl, "GetWorldPosition", &[]).ok()?;
    let q = super::belt::m::<[f32; 4]>(&pl, "GetWorldOrientation", &[]).ok()?;
    Some(([f64::from(p[0]), f64::from(p[1]), f64::from(p[2])], q.map(f64::from)))
}
fn dominant() -> usize {
    if sp::native::<bool>("VRLeftHanded", &[]) == Some(true) {
        1
    } else {
        2
    }
}
fn anchor_for(b: &(V3, [f64; 4])) -> Option<(V3, V3, V3)> {
    let (view, _) = sp::view_world()?;
    let eye = super::belt::qrot(super::belt::conj(b.1), sub(view, b.0));
    let chest = sp::native::<[f32; 4]>("VRBodyBonePos", &[Value::Int32(2)]).filter(|c| c[3] > 0.5).map(|c| [f64::from(c[0]), f64::from(c[1]), f64::from(c[2])]).unwrap_or([0.0, eye[1] - 0.05, eye[2] - 0.35]);
    let side = if dominant() == 2 { 1.0 } else { -1.0 };
    let x = sp::native::<[f32; 4]>("VRHandRawModel", &[Value::Int32(if dominant() == 2 { 1 } else { 0 })]).filter(|h| h[3] > 0.5).map_or(SIDE_DEFAULT, |h| ((f64::from(h[0]) - chest[0]) * side).clamp(SIDE_MIN, SIDE_MAX));
    let c = [chest[0] + x * side, chest[1] + REACH, chest[2] + LIFT];
    let nn = norm(sub(eye, c));
    let r = norm(sub([1.0, 0.0, 0.0], mul(nn, nn[0])));
    Some((c, r, cross(nn, r)))
}
/// The heading of a horizontal forward (the game's yaw: forward = (-sin, cos)).
fn yaw_of(f: V3) -> f64 {
    (-f[0]).atan2(f[1])
}
fn yaw_quat(y: f64) -> [f64; 4] {
    [0.0, 0.0, (y / 2.0).sin(), (y / 2.0).cos()]
}
fn wrap_angle(a: f64) -> f64 {
    (a + std::f64::consts::PI).rem_euclid(2.0 * std::f64::consts::PI) - std::f64::consts::PI
}
/// The wheel's heading starts as the body's (the anchor is taken in the body's frame then)...
fn heading_from_body(w: &mut Wheel, b: &(V3, [f64; 4])) {
    let f = super::belt::qrot(b.1, [0.0, 1.0, 0.0]);
    w.yaw = Some(yaw_of(norm([f[0], f[1], 0.0])));
}
/// ...and then follows the head only past the cone.
fn follow_head(w: &mut Wheel) {
    let (Some(yaw), Some((_, q))) = (w.yaw, sp::view_world()) else { return };
    let f = super::belt::qrot(q, [0.0, 1.0, 0.0]);
    let d = wrap_angle(yaw_of(f) - yaw);
    let cone = FREE_LOOK_DEG.to_radians();
    if d > cone {
        w.yaw = Some(wrap_angle(yaw + d - cone));
    } else if d < -cone {
        w.yaw = Some(wrap_angle(yaw + d + cone));
    }
}
fn frame_of(w: &Wheel, b: &(V3, [f64; 4])) -> Option<(V3, V3, V3)> {
    let (c, r, u) = w.anchor?;
    let q = w.yaw.map(yaw_quat).unwrap_or(b.1);
    Some((add(b.0, super::belt::qrot(q, c)), super::belt::qrot(q, r), super::belt::qrot(q, u)))
}

// ---------------------------------------------------------------- drawing
fn picture(w: &mut Wheel, parent: &Handle, image: Option<(u64, u64)>, x: f64, y: f64, pw: f64, ph: f64, op: f64, col: [f64; 3]) {
    let Some((atlas, part)) = image else { return };
    let pic = k::image_raw(parent, x, y, 1.0, 1.0, atlas, part, Some(col), 0.001);
    ink::call(&pic, "SetFitToContent", &[Value::Bool(true)]);
    ink::v2(&pic, "SetRenderTransformPivot", 0.0, 0.0);
    ink::v2(&pic, "SetScale", 0.001, 0.001);
    w.fits.push(Fit { widget: pic, x, y, w: pw, h: ph, op });
}
fn icon_img(i: &Option<super::menu_data::Icon>) -> Option<(u64, u64)> {
    i.as_ref().map(|i| (i.atlas, i.part))
}
fn shadow(part: &str) -> Option<(u64, u64)> {
    Some((red4ext_abi::resource::resource_path(SHADOW), cname(part)))
}
fn fit_pictures(w: &mut Wheel) {
    w.fits.retain(|f| {
        let Some(o0) = rt::call(&f.widget, "GetDesiredSize", &[]) else { return true };
        let v = Val::of(&o0);
        let (sx, sy) = (v.f("X").and_then(|x| x.num()).unwrap_or(0.0), v.f("Y").and_then(|x| x.num()).unwrap_or(0.0));
        if sx <= 1.0 || sy <= 1.0 {
            return true;
        }
        let kk = (f.w / sx).min(f.h / sy);
        ink::call(&f.widget, "SetFitToContent", &[Value::Bool(false)]);
        ink::size(&f.widget, sx * kk, sy * kk);
        ink::v2(&f.widget, "SetScale", 1.0, 1.0);
        ink::move_to(&f.widget, f.x - sx * kk / 2.0, f.y - sy * kk / 2.0);
        ink::opacity(&f.widget, f.op);
        false
    });
}
fn label(parent: &Handle, cx: f64, cy: f64, w: f64, text: &str, size: f64, col: [f64; 3]) -> Handle {
    k::text(parent, cx - w / 2.0, cy - size, w, size * 2.0, text, size, col, "center", false)
}
fn art(parent: &Handle, name: &str, angle: f64, cx: f64, cy: f64, col: [f64; 3], op: f64, scale: f64) -> Handle {
    let Some(&(_, pw, ph, ox, oy)) = PARTS.iter().find(|p| p.0 == name) else { return Handle::null() };
    let (w, h) = (pw * scale, ph * scale);
    let (dx, dy) = ((pw / 2.0 - ox) * scale, (ph / 2.0 - oy) * scale);
    let a = angle.to_radians();
    let (c, s) = (a.cos(), a.sin());
    let img = k::image(parent, cx + dx * c - dy * s - w / 2.0, cy + dx * s + dy * c - h / 2.0, w, h, ATLAS, name, Some(col), op);
    if angle != 0.0 {
        ink::v2(&img, "SetRenderTransformPivot", 0.5, 0.5);
        ink::rotation(&img, angle);
    }
    img
}
fn badge(parent: &Handle, cx: f64, cy: f64, value: f64) {
    let s = super::menu_gpdata::fmt_g(value);
    let (w, h) = (28.0 + 13.0 * s.len() as f64, 34.0);
    k::round_fill(parent, cx - w / 2.0, cy - h / 2.0, w, h, k::CYAN, 0.2, Some(12.0));
    k::round_edge(parent, cx - w / 2.0, cy - h / 2.0, w, h, k::CYAN, 0.85, Some(12.0));
    k::text(parent, cx - w / 2.0, cy - h / 2.0, w, h, &s, 24.0, k::WHITE, "center", false);
}
fn attachments(item: &Item) -> Vec<bool> {
    let Some(d) = item.data.as_ref() else { return Vec::new() };
    let Some(arr) = rt::static_call("InventoryItemData", "GetAttachments", &[A::P(ptr(d))]) else { return Vec::new() };
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for part in items(&Val::of(&arr)) {
        if seen.insert(fid(&part, "SlotID")) {
            let filled = part.f("ItemData").is_some_and(|d| !sb("InventoryItemData", "IsEmpty", &[A::P(d.ptr.cast_const())]));
            out.push(filled);
        }
    }
    out
}
fn rest(cell: &Cell) {
    let col = cell.col;
    let fill_op = if cell.empty { 0.1 } else if !cell.band.is_null() { 0.5 } else { 0.4 };
    k::tint(std::slice::from_ref(&cell.fill), if cell.empty { k::GREY } else { col }, Some(fill_op));
    if !cell.band.is_null() {
        k::tint(std::slice::from_ref(&cell.band), col, Some(if cell.empty { 0.0 } else { 0.9 }));
    }
    let ecol = if cell.empty { k::GREY } else if cell.blocked { k::RED } else { k::CYAN };
    k::tint(&cell.edge, ecol, Some(if cell.empty { 0.28 } else { 0.55 }));
    ink::opacity(&cell.glow, 0.0);
}
fn build_frame(h: &Surf) -> Frame {
    let root = &h.root;
    ink::remove_all(root);
    let mut f = Frame { warm: k::rect(root, 0.0, 0.0, 1.0, 1.0, k::BLACK, 1.0), ..Default::default() };
    let w = ink::canvas(root, 0.0, 0.0, CW, CH);
    art(&w, "disc_fill", 0.0, CX, CY, k::CYAN, 0.1, (R_RIM + 6.0) / R_CELL);
    let mut y = -R_RIM + 18.0;
    while y <= R_RIM - 18.0 {
        let half = (R_RIM * R_RIM - y * y).sqrt() - 10.0;
        k::rect(&w, CX - half, CY + y, 2.0 * half, 1.0, k::CYAN, 0.045);
        y += 18.0;
    }
    let hub = R_HUB / R_CELL;
    f.hub_glow = art(&w, "disc_fill", 0.0, CX, CY, k::CYAN, 0.12, hub);
    f.hub_ring = vec![art(&w, "disc_edge", 0.0, CX, CY, k::CYAN, 0.5, hub)];
    f.hub_mark = vec![k::slash(&w, CX, CY, 64.0, 6.0, k::GREY, 0.7), k::slash(&w, CX, CY, 64.0, 6.0, k::GREY, 0.7)];
    ink::rotation(&f.hub_mark[1], 45.0);
    f.hub_name = label(&w, CX, CY - 18.0, 250.0, "", 30.0, k::TEXT);
    f.hub_info = label(&w, CX, CY + 30.0, 250.0, "", 24.0, k::GREY);
    for s in SECTORS.iter() {
        if let Some(shape) = s.shape {
            let (cx, cy) = pos(s.angle, (s.inner + s.outer) / 2.0 + 10.0);
            f.cells.push(Cell {
                fill: art(&w, &format!("{shape}_fill"), s.angle, CX, CY, k::CYAN, 0.0, 1.0),
                band: art(&w, &format!("{shape}_band"), s.angle, CX, CY, k::CYAN, 0.0, 1.0),
                glow: art(&w, &format!("{shape}_fill"), s.angle, CX, CY, k::YELLOW, 0.0, 1.0),
                edge: vec![art(&w, &format!("{shape}_edge"), s.angle, CX, CY, k::CYAN, 0.55, 1.0)],
                content: ink::canvas(&w, 0.0, 0.0, CW, CH),
                cx,
                cy,
                mark: pos(s.angle, s.inner + 24.0),
                empty: false,
                blocked: false,
                col: k::CYAN,
            });
        } else {
            let (x, y) = pos(s.angle, R_CELLS);
            let c = ink::canvas(&w, x - R_CELL, y - R_CELL, 2.0 * R_CELL, 2.0 * R_CELL);
            f.cells.push(Cell {
                fill: art(&c, "disc_fill", 0.0, R_CELL, R_CELL, k::CYAN, 0.0, 1.0),
                band: Handle::null(),
                glow: art(&c, "disc_fill", 0.0, R_CELL, R_CELL, k::YELLOW, 0.0, 1.0),
                edge: vec![art(&c, "disc_edge", 0.0, R_CELL, R_CELL, k::CYAN, 0.55, 1.0)],
                content: ink::canvas(&c, 0.0, 0.0, 2.0 * R_CELL, 2.0 * R_CELL),
                cx: R_CELL,
                cy: R_CELL,
                mark: (R_CELL, 18.0),
                empty: false,
                blocked: false,
                col: k::CYAN,
            });
        }
    }
    f.choice_layer = ink::canvas(&w, 0.0, 0.0, CW, CH);
    let top = ink::canvas(root, 0.0, 0.0, CW, CH);
    f.cursor = ink::canvas(&top, 0.0, 0.0, 64.0, 64.0);
    k::ring(&f.cursor, 32.0, 32.0, 17.0, 2.0, k::YELLOW, 0.95, 28, None);
    k::image(&f.cursor, 29.0, 29.0, 6.0, 6.0, k::ATLAS_PHONE, "circle", Some(k::WHITE), 1.0);
    ink::opacity(&f.cursor, 0.0);
    f
}
fn text_of(i: usize, e: &Entry) -> String {
    if SECTORS[i].kind == Kind::Holster {
        return loc::tr("Убрать оружие", "Holster");
    }
    e.item.as_ref().map_or(String::new(), |it| it.name.clone())
}
fn fill_cells(w: &mut Wheel) {
    w.fits.clear();
    let slots = w.slots.clone();
    let Some(mut f) = w.frame.take() else { return };
    for (i, e) in slots.iter().enumerate() {
        let s = &SECTORS[i];
        let cell = &mut f.cells[i];
        ink::remove_all(&cell.content);
        let (x, y) = (cell.cx, cell.cy);
        let content = cell.content.clone();
        let empty = e.item.is_none() && s.kind != Kind::Holster;
        let dim = if e.blocked { 0.35 } else { 1.0 };
        match (&e.item, s.kind) {
            (_, Kind::Holster) => picture(w, &content, shadow("unequip"), x, y, 130.0, 70.0, if e.blocked { 0.3 } else { 0.9 }, k::TEXT),
            (None, _) => {
                if let Some(em) = s.empty {
                    let (pw, ph) = match s.kind {
                        Kind::Weapon(_) => (200.0, 80.0),
                        Kind::Cyberware => (130.0, 70.0),
                        _ => (96.0, 70.0),
                    };
                    picture(w, &content, shadow(em), x, y, pw, ph, 0.4, k::GREY);
                }
            }
            (Some(it), kind) if it.image.is_some() => match kind {
                Kind::Weapon(_) => {
                    let sl = attachments(it);
                    if !sl.is_empty() {
                        let step = 22.0;
                        let x0 = x - (sl.len() as f64 - 1.0) * step / 2.0;
                        for (n0, filled) in sl.iter().enumerate() {
                            let xx = x0 + n0 as f64 * step;
                            k::image(&content, xx - 7.0, y - 90.0 - 7.0, 14.0, 14.0, k::ATLAS_PHONE, "circle", Some(k::RED), 1.0);
                            if !filled {
                                k::image(&content, xx - 4.0, y - 90.0 - 4.0, 8.0, 8.0, k::ATLAS_PHONE, "circle", Some(k::BLACK), 1.0);
                            }
                        }
                    }
                    picture(w, &content, icon_img(&it.image), x, y - 22.0, 220.0, 104.0, dim, k::WHITE);
                    let mut below = y + 44.0;
                    if !it.melee && it.ammo >= 0.0 {
                        let st = super::menu_gpdata::fmt_g(it.ammo);
                        let ax = x - (16.0 + 13.0 * st.len() as f64) / 2.0;
                        k::rect(&content, ax, below - 7.0, 3.0, 14.0, k::CYAN, 1.0);
                        k::rect(&content, ax + 6.0, below - 7.0, 3.0, 14.0, k::CYAN, 1.0);
                        k::text(&content, ax + 16.0, below - 14.0, 13.0 * st.len() as f64 + 12.0, 28.0, &st, 24.0, k::CYAN, "left", false);
                        below += 32.0;
                    }
                    label(&content, x, below + 11.0, 240.0, &k::clip(&it.name, 18), 22.0, k::TEXT);
                }
                Kind::Cyberware => picture(w, &content, icon_img(&it.image), x, y, 150.0, 120.0, dim, k::WHITE),
                _ => picture(w, &content, icon_img(&it.image), x, y - 4.0, 96.0, 96.0, dim, k::WHITE),
            },
            (Some(_), _) => {
                label(&content, x, y, if s.shape.is_some() { 240.0 } else { 2.0 * R_CELL - 20.0 }, &text_of(i, e), 26.0, k::TEXT);
            }
        }
        if let (Kind::Misc(_), Some(it)) = (s.kind, &e.item) {
            if it.quantity > 0.0 {
                badge(&content, x, y + R_CELL - 6.0, it.quantity);
            }
        }
        if e.active {
            k::image(&content, cell.mark.0 - 8.0, cell.mark.1 - 8.0, 16.0, 16.0, k::ATLAS_PHONE, "circle", Some(k::YELLOW), 1.0);
        }
        (cell.empty, cell.blocked) = (empty, e.blocked);
        cell.col = e.item.as_ref().and_then(|it| quality_color(&it.quality)).unwrap_or(k::CYAN);
        rest(cell);
    }
    w.frame = Some(f);
}
fn choice_angle(i: usize, n: usize, kk: usize) -> f64 {
    SECTORS[i].angle + (kk as f64 + 1.0 - (n as f64 + 1.0) / 2.0) * ARC_STEP
}
fn draw_choices(w: &mut Wheel, index: Option<usize>) {
    let Some(mut f) = w.frame.take() else { return };
    ink::remove_all(&f.choice_layer);
    f.choice_cells.clear();
    if let Some(i) = index {
        let e = w.slots[i].clone();
        let n0 = e.choices.len().min(MAX_CHOICES);
        let k0 = R_CHOICE / R_CELL;
        for kk in 0..n0 {
            let c = &e.choices[kk];
            let (x, y) = pos(choice_angle(i, n0, kk), R_ARC);
            let canvas = ink::canvas(&f.choice_layer, x - R_CHOICE, y - R_CHOICE, 2.0 * R_CHOICE, 2.0 * R_CHOICE);
            let current = (c.smoke && c.lit) || (!c.smoke && e.item.as_ref().is_some_and(|it| it.key == c.key));
            art(&canvas, "disc_fill", 0.0, R_CHOICE, R_CHOICE, quality_color(&c.quality).unwrap_or(k::CYAN), 0.4, k0);
            let glow = art(&canvas, "disc_fill", 0.0, R_CHOICE, R_CHOICE, k::YELLOW, 0.0, k0);
            let edge = vec![art(&canvas, "disc_edge", 0.0, R_CHOICE, R_CHOICE, if current { k::YELLOW } else { k::CYAN }, if current { 0.9 } else { 0.6 }, k0)];
            f.choice_cells.push(ChoiceCell { x, y, current, glow, edge });
            picture(w, &canvas, icon_img(&c.image), R_CHOICE, R_CHOICE - 4.0, 84.0, 84.0, 1.0, k::WHITE);
            if c.quantity > 0.0 {
                badge(&canvas, R_CHOICE, 2.0 * R_CHOICE - 4.0, c.quantity);
            }
        }
    }
    w.frame = Some(f);
}

// ---------------------------------------------------------------- pointing
fn hit(w: &Wheel, x: f64, y: f64) -> Option<Hover> {
    let (dx, dy) = (x - CX, y - CY);
    let r = (dx * dx + dy * dy).sqrt();
    if r < R_HUB {
        return Some(Hover::Hub);
    }
    let a = dx.atan2(-dy).to_degrees().rem_euclid(360.0);
    if let Some(i) = w.arc {
        if r > R_CELLS + R_CELL {
            let n0 = w.slots[i].choices.len().min(MAX_CHOICES);
            if turn(SECTORS[i].angle, a).abs() <= (n0 as f64 + 1.0) / 2.0 * ARC_STEP {
                if r > R_ARC + R_CHOICE + 40.0 {
                    return None;
                }
                if r > R_ARC - R_CHOICE - 20.0 {
                    for kk in 0..n0 {
                        if turn(choice_angle(i, n0, kk), a).abs() <= ARC_STEP / 2.0 {
                            return Some(Hover::Choice(i, kk));
                        }
                    }
                }
                return Some(Hover::Cell(i));
            }
        }
    }
    if r > R_RIM + 60.0 {
        return None;
    }
    let (mut nearest, mut best) = (None, 1e9);
    for (i, s) in SECTORS.iter().enumerate() {
        let d = turn(s.angle, a).abs();
        if s.shape.is_some() {
            if d <= s.half && r >= s.inner - 20.0 {
                return Some(Hover::Cell(i));
            }
        } else {
            let (cx, cy) = pos(s.angle, R_CELLS);
            if (x - cx).powi(2) + (y - cy).powi(2) <= (R_CELL + 18.0).powi(2) {
                return Some(Hover::Cell(i));
            }
        }
        if d < best {
            (nearest, best) = (Some(i), d);
        }
    }
    if r >= R_HUB + 30.0 {
        return nearest.map(Hover::Cell);
    }
    None
}
struct Point {
    hand: usize,
    x: f64,
    y: f64,
    beam: bool,
    origin: V3,
    at: V3,
    length: f64,
}
fn point(c: V3, r: V3, u: V3) -> Option<Point> {
    let vs = [[c[0] as f32, c[1] as f32, c[2] as f32, 1.0], [r[0] as f32, r[1] as f32, r[2] as f32, 0.0], [u[0] as f32, u[1] as f32, u[2] as f32, 0.0], [W as f32, H as f32, 0.0, 0.0]];
    let out = sp::native_v4s("VRMenuPointer", &[Value::Ptr(vs[0].as_ptr().cast()), Value::Ptr(vs[1].as_ptr().cast()), Value::Ptr(vs[2].as_ptr().cast()), Value::Ptr(vs[3].as_ptr().cast())]);
    if out.len() < 8 {
        return None;
    }
    let first = dominant();
    for hand in [first, 3 - first] {
        let base = (hand - 1) * 4;
        let (tip, ray, origin, at) = (out[base], out[base + 1], out[base + 2], out[base + 3]);
        let f = f64::from;
        let (uv, beam) = if tip[3] > 0.5 && f(tip[2]) < HOVER && f(tip[2]) > THROUGH {
            (Some((f(tip[0]), f(tip[1]))), false)
        } else if ray[3] > 0.5 {
            (Some((f(ray[0]), f(ray[1]))), true)
        } else {
            (None, false)
        };
        if let Some((uu, vv)) = uv.filter(|(a, b)| (0.0..=1.0).contains(a) && (0.0..=1.0).contains(b)) {
            let (x, y) = (uu * CW, vv * CH);
            if (x - CX).powi(2) + (y - CY).powi(2) < (R_ARC + R_CHOICE + 60.0).powi(2) {
                return Some(Point { hand, x, y, beam, origin: [f(origin[0]), f(origin[1]), f(origin[2])], at: [f(at[0]), f(at[1]), f(at[2])], length: f(ray[2]) });
            }
        }
    }
    None
}
fn build_beam(h: &mut Surf) {
    ink::remove_all(&h.root);
    h.strip.clear();
    for kk in 1..=7usize {
        let f = if kk <= 6 { kk as f64 / 7.0 } else { 1.0 };
        h.strip.push((kk, 0.0, 16.0, k::rect(&h.root, 0.0, 0.0, 16.0, 0.0, k::CYAN, 0.25 * f)));
        h.strip.push((kk, 5.0, 6.0, k::rect(&h.root, 5.0, 0.0, 6.0, 0.0, k::WHITE, 0.9 * f)));
    }
    h.strip_px = -1.0;
    h.built = true;
}
fn beam_length(h: &mut Surf, length: f64) {
    let per_m = 2048.0 / BEAM_LEN;
    let px = length.clamp(0.0, BEAM_LEN) * per_m;
    if (px - h.strip_px).abs() < 1.0 {
        return;
    }
    h.strip_px = px;
    let (hide, step) = (0.12 * per_m, 0.12 * per_m / 6.0);
    for (kk, x, wd, r) in &h.strip {
        let top = hide + (*kk as f64 - 1.0) * step;
        let bottom = if *kk <= 6 { top + step } else { px };
        ink::move_to(r, *x, top);
        ink::size(r, *wd, (bottom.min(px) - top).max(0.0));
    }
}
/// The beam's surface resolved, built and written once by the GPU.
fn beam_ready(h: &mut Surf) -> bool {
    if !resolve(h, UI) {
        return false;
    }
    if !h.built {
        build_beam(h);
    }
    if !h.gpu {
        h.gpu = sp::native::<bool>("VRMenuSurfaceReady", &[Value::Handle(&h.comp), Value::Handle(&h.root)]) == Some(true);
    }
    h.gpu
}
fn show_beam(w: &mut Wheel, p: Option<&Point>, view: Option<V3>) {
    let time = w.time;
    let h = &mut w.beam;
    if !beam_ready(h) {
        return;
    }
    let (Some(p), Some(view)) = (p.filter(|p| p.beam), view) else {
        display(h, false, time);
        return;
    };
    let dir = norm(sub(p.at, p.origin));
    let to_cam = sub(view, p.origin);
    let n0 = sub(to_cam, mul(dir, dot(to_cam, dir)));
    if dot(n0, n0) < 1e-12 {
        return;
    }
    let n0 = norm(n0);
    place(h, add(p.origin, mul(dir, BEAM_LEN / 2.0)), cross(n0, dir), mul(dir, -1.0));
    beam_length(h, p.length);
    display(h, true, time);
}
fn paint_hover(w: &Wheel, hover: Option<Hover>) {
    let Some(f) = w.frame.as_ref() else { return };
    for (i, cell) in f.cells.iter().enumerate() {
        if hover == Some(Hover::Cell(i)) && !cell.empty {
            ink::opacity(&cell.glow, if cell.blocked { 0.15 } else if !cell.band.is_null() { 0.45 } else { 0.5 });
            k::tint(&cell.edge, if cell.blocked { k::RED } else { k::YELLOW }, Some(1.0));
        } else {
            rest(cell);
        }
    }
    for (kk, cell) in f.choice_cells.iter().enumerate() {
        let on = w.arc.is_some_and(|a| hover == Some(Hover::Choice(a, kk)));
        ink::opacity(&cell.glow, if on { 0.5 } else { 0.0 });
        k::tint(&cell.edge, if on || cell.current { k::YELLOW } else { k::CYAN }, Some(if on { 1.0 } else if cell.current { 0.9 } else { 0.6 }));
    }
    let hub = hover == Some(Hover::Hub);
    match hover {
        Some(Hover::Cell(i)) | Some(Hover::Choice(i, _)) => {
            let e = &w.slots[i];
            let choice = if let Some(Hover::Choice(_, c)) = hover { e.choices.get(c) } else { None };
            let name = choice.map_or_else(|| text_of(i, e), |c| c.name.clone());
            ink::set_text(&f.hub_name, &k::clip(&name, 18));
            let mut kind = choice.and_then(|c| c.item.as_ref()).or(e.item.as_ref()).map_or(String::new(), |it| it.type_name.clone());
            if kind == "None" {
                kind.clear();
            }
            ink::set_text(&f.hub_info, &if e.blocked { loc::tr("Недоступно", "Unavailable") } else { kind });
            k::tint(&f.hub_mark, k::GREY, Some(0.0));
        }
        _ => {
            ink::set_text(&f.hub_name, "");
            ink::set_text(&f.hub_info, "");
            k::tint(&f.hub_mark, if hub { k::YELLOW } else { k::GREY }, Some(if hub { 1.0 } else { 0.7 }));
        }
    }
    k::tint(&f.hub_ring, if hub { k::YELLOW } else { k::CYAN }, Some(if hub { 1.0 } else { 0.5 }));
    ink::opacity(&f.hub_glow, if hub { 0.35 } else { 0.12 });
}

// ---------------------------------------------------------------- open / close
fn prepare(w: &mut Wheel, b: &(V3, [f64; 4])) {
    if w.screen.id.is_some() {
        return;
    }
    w.anchor = anchor_for(b);
    heading_from_body(w, b);
    let Some((c, r, u)) = frame_of(w, b) else { return };
    // below the body until the first opening places them: the display comes on there (prewarm)
    let c = add(c, [0.0, 0.0, -HIDE_DEPTH]);
    w.screen = spawn(ENT, c, r, u);
    w.beam = spawn(BEAM_ENT, c, r, u);
    w.glass = spawn(DISC_ENT, c, r, u);
    w.choice_glass.clear();
    w.frame = None;
    sp::native::<()>("SetVRDiagCapture", &[Value::Int32(1)]);
}
/// The wheel's inventory context: the game's own radial wheel's manager as soon as the HUD has made it (warm since its
/// OnLateInit), taking over from one of ours; one of ours only for an opening before the HUD's (its cache built then).
fn ensure_ctx(w: &mut Wheel, opening: bool) {
    if w.ctx.as_ref().is_some_and(idt::Ctx::borrowed) {
        return;
    }
    if let Some(m) = game_radial_manager() {
        if let Some(old) = w.ctx.take() {
            idt::close(old);
        }
        w.ctx = Some(idt::open_on(m));
    } else if w.ctx.is_none() && opening {
        let c = idt::open();
        idt::warm(&c);
        w.ctx = Some(c);
    }
}
/// The game's radial wheel (RadialWheelController, a HUD controller made with the HUD), held weakly from its
/// OnInitialize to its OnUninitialize (game_hooks.rs).
struct Radial(red4ext_abi::handle::WeakHandle);
// SAFETY: set and read on the game's main thread (the HUD's callbacks, the mechanics' tick).
unsafe impl Send for Radial {}
static RADIAL: Mutex<Option<Radial>> = Mutex::new(None);
pub(super) fn radial_controller(this: Option<&Handle>) {
    *RADIAL.lock().unwrap_or_else(|p| p.into_inner()) = this.map(|h| Radial(red4ext_abi::handle::WeakHandle::from_handle(h)));
}
fn game_radial_manager() -> Option<Handle> {
    let c = RADIAL.lock().unwrap_or_else(|p| p.into_inner()).as_ref().map(|r| r.0.lock())?;
    let m = rt::field(&c, "inventoryManager").map(|v| v.handle())?;
    (!m.is_null()).then_some(m)
}
/// The screen's display switched on while the wheel is still hidden (the canvas at zero opacity): a display switched
/// on builds its render proxy from the material's defaults -- an opaque black plane the size of the screen -- until the
/// canvas binds, the black window an opening began with when that happened in front of the eyes.
fn prewarm(h: &mut Surf) {
    if h.entity.is_null() || h.root.is_null() || h.display_on {
        return;
    }
    ink::opacity(&h.root, 0.0);
    super::belt::toggle(&sp::find_comp(&h.entity, "display"), true);
    h.display_on = true;
    h.shown = Some(false);
}
fn open(w: &mut Wheel, pl: &Handle, b: &(V3, [f64; 4])) -> bool {
    w.anchor = anchor_for(b);
    heading_from_body(w, b);
    ensure_ctx(w, true);
    if w.anchor.is_none() || w.ctx.is_none() {
        return false;
    }
    loc::set(&super::menu_data::language());
    // the inventory scan finished now if an opening came before it did
    if w.hits.is_none() {
        let mut sc = w.scan.take().unwrap_or_else(|| idt::HotkeyScan::start(hotkey_kinds()));
        sc.step(None);
        w.hits = Some(sc.hits);
    }
    let hits = w.hits.clone().unwrap_or_default();
    let mut ctx = w.ctx.take().expect("ctx");
    w.slots = gather(&mut ctx, pl, &hits);
    w.ctx = Some(ctx);
    (w.open, w.hover, w.arc, w.arc_away, w.hand, w.filled) = (true, None, None, None, None, false);
    play("open");
    true
}
fn close(w: &mut Wheel, applied: bool) {
    if !w.open {
        return;
    }
    w.open = false;
    display(&mut w.screen, false, w.time);
    if let Some(f) = &w.frame {
        ink::opacity(&f.cursor, 0.0);
    }
    display(&mut w.beam, false, w.time);
    show_glass(&mut w.glass, false, None);
    for g in w.choice_glass.iter_mut() {
        show_glass(g, false, None);
    }
    if applied {
        play("click");
        haptic(w.hand);
    } else {
        play("close");
    }
    (w.hover, w.arc) = (None, None);
}
fn place_glass(w: &mut Wheel, c: V3, r: V3, u: V3) {
    // not before the screen has shown for a tick: dark glass with no light on it reads as a black window
    if w.screen.shown_at.is_none_or(|at| w.time <= at) {
        return;
    }
    let n0 = cross(r, u);
    if resolve(&mut w.glass, "display") {
        place(&w.glass, add(c, mul(n0, -GLASS_BACK)), r, u);
        show_glass(&mut w.glass, true, Some(2.0 * (R_RIM + 10.0) / PX));
    }
    let spots: Vec<(f64, f64)> = w.frame.as_ref().map(|f| f.choice_cells.iter().map(|cc| (cc.x, cc.y)).collect()).unwrap_or_default();
    for kk in 0..spots.len().max(w.choice_glass.len()) {
        match spots.get(kk) {
            Some(&(x, y)) => {
                if w.choice_glass.len() <= kk {
                    w.choice_glass.push(spawn(DISC_ENT, c, r, u));
                }
                let g = &mut w.choice_glass[kk];
                if resolve(g, "display") {
                    let at = add(add(add(c, mul(r, (x - CX) / PX)), mul(u, (CY - y) / PX)), mul(n0, -GLASS_BACK));
                    place(g, at, r, u);
                    show_glass(g, true, Some(2.0 * (R_CHOICE + 6.0) / PX));
                }
            }
            None => show_glass(&mut w.choice_glass[kk], false, None),
        }
    }
}

// ---------------------------------------------------------------- the frame
fn tick(w: &mut Wheel, menu_open: bool) {
    let pl = player();
    if w.time - w.allowed_at > 0.1 {
        w.allowed_at = w.time;
        w.allowed = allowed_now(&pl, menu_open);
    }
    let allowed = w.allowed;
    let hold = sp::native::<i32>("VRWeaponWheel", &[Value::Bool(allowed || w.open)]).unwrap_or(0);
    ensure_ctx(w, false);
    if let Some((at0, key)) = w.consume_at.clone() {
        let on_key = rt::call(&equipment(), "GetItemIDFromHotkey", &[A::H(&pl), A::N(ev("EHotkey", "DPAD_UP"))]).map(|o0| idt::key_of(&Id16::of(&Val::of(&o0)))) == Some(key);
        if on_key {
            w.consume_at = None;
            consume(&pl);
        } else if w.time - at0 > CONSUME_WAIT {
            w.consume_at = None;
        }
    }
    if let Some(at0) = w.smoke_at {
        if in_hand(&pl).is_empty() {
            w.smoke_at = None;
            if !has_cig(&pl) {
                super::smoke::toggle(&pl);
            }
        } else if w.time - at0 > SMOKE_WAIT {
            w.smoke_at = None;
        }
    }
    let Some(b) = body() else { return };
    if w.open {
        follow_head(w);
    }
    // made ahead (the entities below the body, the inventory manager): kept for the session like the game's radial
    // wheel's widgets, so no Y press spawns them; a press before that makes them then
    if w.screen.id.is_none() && ((w.present > WARM_AFTER && !menu_open) || (hold >= 1 && allowed)) {
        prepare(w, &b);
    }
    if resolve(&mut w.screen, UI) {
        if w.frame.is_none() {
            w.frame = Some(build_frame(&w.screen));
            w.screen.built = true;
        }
        if !w.screen.gpu {
            w.screen.gpu = sp::native::<bool>("VRMenuSurfaceReady", &[Value::Handle(&w.screen.comp), Value::Handle(&w.screen.root)]) == Some(true);
        }
        if w.screen.gpu && !w.open {
            prewarm(&mut w.screen);
        }
    }
    if hold == 2 && !w.open && allowed {
        open(w, &pl, &b);
    }
    if w.open && (!allowed || hold == 0) {
        let applied = hold == 0 && allowed && {
            let hv = w.hover;
            apply(w, hv)
        };
        close(w, applied);
    }
    if !w.open {
        // the beam's display likewise comes on hidden, once the screen's has
        if w.screen.display_on && !w.beam.display_on && beam_ready(&mut w.beam) {
            prewarm(&mut w.beam);
        }
        // the hotkey items looked for a little per frame: after a load, and again whenever an item comes or goes
        if INVENTORY_CHANGED.swap(false, std::sync::atomic::Ordering::Relaxed) {
            (w.hits, w.scan) = (None, None);
        }
        if w.hits.is_none() && w.present > WARM_AFTER {
            let sc = w.scan.get_or_insert_with(|| idt::HotkeyScan::start(hotkey_kinds()));
            if sc.step(Some(SCAN_BUDGET)) {
                w.hits = w.scan.take().map(|sc| sc.hits);
            }
        }
        return;
    }
    if w.frame.is_none() {
        return;
    }
    if !w.filled {
        fill_cells(w);
        w.filled = true;
        w.arc_shown = None;
    }
    fit_pictures(w);
    let Some((c, r, u)) = frame_of(w, &b) else { return };
    place(&w.screen, c, r, u);
    if !w.screen.gpu {
        return;
    }
    if let Some(f) = w.frame.as_mut() {
        if !f.warm.is_null() {
            ink::call(&w.screen.root, "RemoveChild", &[Value::Handle(&f.warm)]);
            f.warm = Handle::null();
        }
    }
    display(&mut w.screen, true, w.time);
    let view = sp::view_world().map(|v| v.0);
    let p = point(c, r, u);
    let hover = p.as_ref().and_then(|p| hit(w, p.x, p.y));
    match hover {
        Some(Hover::Cell(i)) if Some(i) != w.arc => {
            let away = *w.arc_away.get_or_insert(w.time);
            if w.arc.is_none() || w.time - away >= ARC_KEEP {
                let e = &w.slots[i];
                let more = e.choices.len() > usize::from(e.item.is_some());
                w.arc = (matches!(SECTORS[i].kind, Kind::Misc(_)) && more).then_some(i);
                w.arc_away = None;
            }
        }
        _ => {
            w.arc_away = None;
            if hover == Some(Hover::Hub) {
                w.arc = None;
            }
        }
    }
    if w.arc_shown != Some(w.arc) {
        let arc = w.arc;
        draw_choices(w, arc);
        w.arc_shown = Some(arc);
        w.hover_painted = false;
    }
    if hover != w.hover || !w.hover_painted {
        paint_hover(w, hover);
        w.hover = hover;
        w.hover_painted = true;
    }
    if let Some(pp) = &p {
        w.hand = Some(pp.hand);
    }
    if let Some(f) = &w.frame {
        match &p {
            Some(pp) => {
                ink::move_to(&f.cursor, pp.x - 32.0, pp.y - 32.0);
                ink::opacity(&f.cursor, 1.0);
            }
            None => ink::opacity(&f.cursor, 0.0),
        }
    }
    place_glass(w, c, r, u);
    show_beam(w, p.as_ref(), view);
}
/// Wheel.update: the frame, the menu's open state given (the wheel stands down while it is open).
pub(super) fn update(dt: f64, menu_open: bool) {
    let mut g = WHEEL.lock().unwrap_or_else(|p| p.into_inner());
    let w = g.get_or_insert_with(Wheel::default);
    w.time += dt;
    if player().is_null() {
        w.present = 0.0;
        (w.hits, w.scan) = (None, None);
        sp::native::<i32>("VRWeaponWheel", &[Value::Bool(false)]);
        if w.screen.id.is_some() || w.ctx.is_some() {
            (w.screen, w.beam, w.glass) = (Surf::default(), Surf::default(), Surf::default());
            w.choice_glass.clear();
            w.ctx = None;
            w.frame = None;
            (w.open, w.hover, w.arc) = (false, None, None);
        }
        return;
    }
    w.present += dt;
    tick(w, menu_open);
}
pub(super) fn release_all() {
    sp::native::<i32>("VRWeaponWheel", &[Value::Bool(false)]);
    let mut g = WHEEL.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(w) = g.as_mut() {
        close(w, false);
        release(w);
    }
}
pub(super) fn is_open() -> bool {
    WHEEL.lock().map(|g| g.as_ref().is_some_and(|w| w.open)).unwrap_or(false)
}
