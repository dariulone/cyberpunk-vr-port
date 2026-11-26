//! appearance.lua and appearance_view.lua (main's 0.2.3 walkable mirror): the mirror's appearance request, retained by
//! the vendor route's queue hook (MorphMenuUserData in MenuScenario_CharacterCustomizationMirror), edited in the world
//! menu on the mirror's double -- the game's own reflection NPC (Character.q110_tpp_*_v) -- instead of the game's
//! editor: the double wears the editor's head and arms (leased slots, given back), the customization system dresses it
//! (VRCreationTarget) from its own option resources (VRMirrorAppearanceOptions), each value applied at once; ACCEPT
//! finalizes, CANCEL undoes the preview; the mirror's icon hands the request to the game's own editor. The session ends
//! in steps: the double's own head back, the double rebuilt a second later, the system let go and the scene completed
//! (VRMirrorAppearanceFinish) or handed over (VRMirrorAppearanceNative) a second after that.
use super::lobby_rtti::{self as rt, Val, A};
use super::lobby_spawn::native;
use super::menu_core::*;
use super::menu_ink as k;
use super::menu_locale as loc;
use super::menu_rt::*;
use crate::script_call::Value;
use red4ext_abi::handle::Handle;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Seconds: after the editor head comes off (it strips the double), and after the double is rebuilt.
const STEP: f64 = 1.0;
/// Seconds the double's own customization controllers take to rebuild it after a finalize; the system's reset waits
/// that long (a reset in the finalize's frame cuts them off: a bald reflection).
const SETTLE: f64 = 2.0;
/// The game's mirror prompt icon (ChoiceIcons.MirrorIcon): a person before a mirror; its opaque box measured off
/// mappin_icons.xbm, alpha > 40.
const MIRROR: Glyph = (k::ATLAS_MAPPIN, "mirror", 64.0, 64.0, 2.0, 5.0, 61.0, 59.0, 1.0);

/// A slot the double wears for the session: what it had there, what it was given.
struct Lease {
    slot: u64,
    old: Option<Id16>,
    item: Option<Id16>,
    had: bool,
}
/// One editable option: the option object, its key, its label and its values (each one's label).
#[derive(Clone)]
pub(super) struct OptRow {
    option: Handle,
    key: String,
    label: String,
    values: Vec<String>,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    Preparing,
    Ready,
    Building,
    Active,
    Failed,
    Restoring,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// The scene goes on here.
    Here,
    /// The request goes back to the game's own editor at once.
    Now,
    /// This session hands it to that editor.
    Native,
}
struct Session {
    phase: Phase,
    age: f64,
    panel: Handle,
    puppet: Handle,
    player: u64,
    leases: Vec<Lease>,
    leases_back: bool,
    chosen: HashMap<String, (i64, f64)>,
    anchor: [f64; 3],
    male: bool,
    target: bool,
    error: Option<String>,
    accepted: Option<bool>,
    final_: Option<HashMap<String, i64>>,
    reapplied: usize,
    rows: Option<Vec<OptRow>>,
    refresh_at: Option<f64>,
    stage: i32,
    stage_at: f64,
    end_reason: &'static str,
    end_mode: Mode,
}
#[derive(Default)]
struct Shared {
    session: Option<Session>,
    reset_in: Option<f64>,
    last: Option<String>,
}
thread_local! {
    static S: RefCell<Shared> = RefCell::new(Shared::default());
}
fn with<R>(f: impl FnOnce(&mut Shared) -> R) -> R {
    S.with(|s| f(&mut s.borrow_mut()))
}
fn system() -> Handle {
    super::game_system("GetCharacterCustomizationSystem")
}
fn identity(e: &Handle) -> u64 {
    rt::call(e, "GetEntityID", &[]).map_or(0, |o| unsafe { *(o.as_ptr() as *const u64) })
}
/// GetLocalizedTextByKey of a name by its hash: "" when it has none.
fn name_label(hash: u64) -> String {
    super::menu_data::loc_name(hash)
}
/// GetLocalizedTextByKey: the key's text, "" when it has none.
#[allow(dead_code)]
fn loc_key(key: &str) -> String {
    if key.is_empty() || key == "None" {
        return String::new();
    }
    let t = super::menu_data::loc_key_text(key);
    if t.is_empty() || t == key {
        String::new()
    } else {
        t
    }
}
fn fact(name: &str) -> f64 {
    let qs = super::game_system("GetQuestsSystem");
    let a = Arg::new(name);
    n(&qs, "GetFactStr", &[a.a()])
}
fn world_position(e: &Handle) -> [f64; 3] {
    let v = rt::call(e, "GetWorldPosition", &[]).map(|o| Val::of(&o));
    let f = |c: &str| v.as_ref().and_then(|v| v.f(c)).and_then(|x| x.num()).unwrap_or(0.0);
    [f("X"), f("Y"), f("Z")]
}
fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}
fn gender_name(e: &Handle) -> String {
    rt::call(e, "GetResolvedGenderName", &[]).map(|o| Val::of(&o).text()).unwrap_or_default()
}
/// The game's own reflection: the TPP double of the player's gender within 3 m (none while Johnny is in the mirror).
fn mirror_double() -> Handle {
    let pl = player();
    if pl.is_null() || fact("johnny_mirror") > 0.0 {
        return Handle::null();
    }
    let wanted = tdb(if gender_name(&pl) == "Female" { "Character.q110_tpp_female_v" } else { "Character.q110_tpp_male_v" });
    let Some(mut q) = new_struct("TargetSearchQuery") else { return Handle::null() };
    let qv = val(&q);
    set(&qv, "maxDistance", 3.0);
    set(&qv, "testedSet", ev("TargetingSet", "Complete"));
    let targeting = super::game_system("GetTargetingSystem");
    let Some((_, outs)) = rt::call_outs(&targeting, "GetTargetParts", &[A::H(&pl), A::P(q.as_mut_ptr().cast_const().cast())], 1) else { return Handle::null() };
    let Some(parts) = outs.first() else { return Handle::null() };
    for part in items(&Val::of(parts)) {
        let comp = rt::static_call("TS_TargetPartInfo", "GetComponent", &[A::P(part.ptr.cast_const())]).map(|o| Val::of(&o).handle()).unwrap_or_else(Handle::null);
        let e = o(&comp, "GetEntity", &[]);
        if !e.is_null() && is_a(&e, "NPCPuppet") && id(&e, "GetRecordID", &[]) == wanted {
            return e;
        }
    }
    Handle::null()
}
fn ts() -> Handle {
    super::game_system("GetTransactionSystem")
}
fn item_in_slot(puppet: &Handle, slot: u64) -> Option<Id16> {
    let it = o(&ts(), "GetItemInSlot", &[A::H(puppet), A::Name(slot)]);
    (!it.is_null()).then(|| id16(&it, "GetItemID", &[]))
}
/// The slot leased for the session: its own item's visual entity destroyed (keeping it alive left external garment
/// meshes on the double though the slot reads empty), the given item put on.
fn lease_slot(s: &mut Session, slot: u64, item: Option<Id16>) -> Result<(), String> {
    let ts = ts();
    let old = item_in_slot(&s.puppet, slot);
    let had = item.as_ref().is_some_and(|i| b(&ts, "HasItem", &[A::H(&s.puppet), i.a()]));
    s.leases.push(Lease { slot, old, item, had });
    if old.is_some() {
        rt::call(&ts, "RemoveItemFromSlot", &[A::H(&s.puppet), A::Name(slot), A::B(true)]);
    }
    if let Some(item) = item {
        if !had {
            rt::call(&ts, "GiveItem", &[A::H(&s.puppet), item.a(), A::N(1.0)]);
        }
        if !b(&ts, "AddItemToSlot", &[A::H(&s.puppet), A::Name(slot), item.a()]) {
            return Err("preview attachment refused".into());
        }
    }
    Ok(())
}
/// The double's own headwear, head and arms back on it. Once per session.
fn restore_leases(s: &mut Session) {
    if s.leases_back {
        return;
    }
    s.leases_back = true;
    let pl = player();
    if s.puppet.is_null() || (!pl.is_null() && identity(&pl) == identity(&s.puppet)) {
        return;
    }
    let ts = ts();
    for l in s.leases.iter().rev() {
        let mut current = item_in_slot(&s.puppet, l.slot);
        if let (Some(item), Some(cur)) = (l.item, current) {
            if cur == item {
                rt::call(&ts, "RemoveItemFromSlot", &[A::H(&s.puppet), A::Name(l.slot), A::B(true)]);
                current = None;
            }
        }
        if current.is_none() {
            if let Some(old) = l.old.filter(|old| b(&ts, "HasItem", &[A::H(&s.puppet), old.a()])) {
                rt::call(&ts, "AddItemToSlot", &[A::H(&s.puppet), A::Name(l.slot), old.a()]);
            }
        }
        if let (Some(item), false) = (l.item, l.had) {
            rt::call(&ts, "RemoveItem", &[A::H(&s.puppet), item.a(), A::N(1.0)]);
        }
    }
}
fn creation_target(e: &Handle) -> bool {
    native::<bool>("VRCreationTarget", &[Value::Handle(e)]) == Some(true)
}
fn restore(s: &mut Session) {
    restore_leases(s);
    if s.target {
        creation_target(&Handle::null());
        s.target = false;
    }
}
/// The game's reset first takes the session's parts off the system's target -- the mirror double, by resource, its own
/// copies too. VRCustomizationReset resets with no target and gives the game its target back.
fn reset_options() {
    if native::<bool>("VRCustomizationReset", &[]) == Some(true) {
        return;
    }
    rt::call(&system(), "ResetCustomizationOptionsResource", &[]);
}
fn reset_system() {
    with(|sh| sh.reset_in = None);
    reset_options();
}
fn options(c: &Handle) -> Vec<Handle> {
    rt::objs(c, "GetUnitedOptions", &[A::B(true), A::B(true), A::B(true)])
}
fn option_key(o: &Handle) -> String {
    rt::field(o, "info").and_then(|i| i.f("name")).map(|n| n.text()).unwrap_or_default()
}
fn cur_index(o: &Handle) -> i64 {
    rt::field(o, "currIndex").and_then(|v| v.num()).unwrap_or(0.0) as i64
}
fn flag(o: &Handle, f: &str) -> bool {
    rt::field(o, f).is_some_and(|v| v.bool())
}
fn apply(c: &Handle, o: &Handle, index: i64) -> bool {
    rt::call(c, "ApplyChangeToOption", &[A::H(o), A::N(index as f64), A::B(true)]).is_some()
}
/// ApplyChangeToOption ignores a value equal to the option's current one: with the current index cleared the game takes
/// the value as new -- loads its resources, puts its parts on the system's target, and removes nothing.
fn reapply_all(c: &Handle, mut final_: Option<&mut HashMap<String, i64>>) -> usize {
    let mut count = 0;
    for o in options(c) {
        if !flag(&o, "isActive") {
            continue;
        }
        let index = cur_index(&o);
        if let Some(f) = final_.as_deref_mut() {
            f.insert(option_key(&o), index);
        }
        hset(&o, "currIndex", 4294967295.0);
        apply(c, &o, index);
        if cur_index(&o) != index {
            hset(&o, "currIndex", index as f64);
        }
        count += 1;
    }
    count
}
/// The preview undone on the double: every value the panel offers back to the finalized one through the same change.
fn revert(s: &Session) {
    let (Some(final_), Some(rows)) = (s.final_.as_ref(), s.rows.as_ref()) else { return };
    let c = system();
    for row in rows {
        if let Some(&index) = final_.get(&row.key) {
            apply(&c, &row.option, index);
        }
    }
}
/// Taking the editor head off strips the double of its customization parts (hair, beard, piercings): the finalized
/// state put back on it, then those same values finalized -- the double's own controllers rebuild it from them.
fn rebuild() {
    let c = system();
    rt::call(&c, "InitializeOptionsFromFinalizedState", &[]);
    reapply_all(&c, None);
    rt::call(&c, "ReFinalizeState", &[]);
}
/// The session's end. The game's own editor shows an empty screen while the double's option resource is still held (its
/// SetCustomizationOptionsResource loads nothing): reset at once when the request goes to that editor, otherwise once the
/// double has settled. The mirror scene waits for the menu to return to Idle: handed to the game's editor, the request is
/// that editor's to complete; otherwise (or when it can no longer be handed over) it is completed here.
fn finish_session(reason: &str, mode: Mode) {
    let Some(mut s) = with(|sh| sh.session.take()) else { return };
    let touched = s.target;
    restore(&mut s);
    if touched {
        if mode != Mode::Here {
            reset_system();
        } else {
            with(|sh| sh.reset_in = Some(SETTLE));
        }
    }
    let mut handed = false;
    if mode == Mode::Native && !s.panel.is_null() {
        handed = native::<bool>("VRMirrorAppearanceNative", &[Value::Handle(&s.panel)]) == Some(true);
    }
    let mut completed = false;
    if !handed && !s.panel.is_null() {
        completed = native::<bool>("VRMirrorAppearanceFinish", &[Value::Handle(&s.panel)]) == Some(true);
    }
    let last = format!("closed:{reason}:error={}:accepted={:?}:sceneCompleted={completed}:native={handed}", s.error.as_deref().unwrap_or(""), s.accepted);
    cvr_host::log!("[VRMenu] appearance {last} (from Rust)\n");
    with(|sh| sh.last = Some(last));
}
/// The session ends in steps (poll): the double's own head back now, rebuilt a STEP later, then the system let go.
fn close_session(s: &mut Session, reason: &'static str, mode: Mode) {
    restore_leases(s);
    (s.phase, s.stage, s.stage_at, s.end_reason, s.end_mode) = (Phase::Restoring, 1, s.age + STEP, reason, mode);
}
/// The options the panel offers: editable, active, uncensored, with more than one value.
fn rows() -> Vec<OptRow> {
    let mut out = Vec::new();
    for o in options(&system()) {
        if !flag(&o, "isEditable") || !flag(&o, "isActive") || flag(&o, "isCensored") {
            continue;
        }
        let Some(info) = rt::field(&o, "info") else { continue };
        let field = match info.class_name().as_str() {
            "gameuiAppearanceInfo" => "definitions",
            "gameuiMorphInfo" => "morphNames",
            "gameuiSwitcherInfo" => "options",
            _ => continue,
        };
        let Some(values) = info.f(field) else { continue };
        if values.len() <= 1 {
            continue;
        }
        // the names localized by their hash (the Lua's locKey took the CName itself; by its text a name made from a
        // TweakDB string read "")
        let named = |v: Option<Val>| v.and_then(|v| v.f("localizedName")).map_or(String::new(), |n| name_label(n.name_hash()));
        let labels = (0..values.len()).map(|i| named(values.at(i))).collect();
        let label = named(Some(info.clone()));
        out.push(OptRow { option: o.clone(), key: info.f("name").map(|n| n.text()).unwrap_or_default(), label, values: labels });
    }
    out
}

// ---------------------------------------------------------------- the route, the poll
/// A.route: the retained mirror request -- Some(true) taken (the world editor builds), Some(false) given back to the
/// game's editor, None kept until the world editor is ready.
pub(super) fn route(panel: &Handle) -> Option<bool> {
    if with(|sh| sh.session.is_none()) {
        // whoever takes this request -- the world editor or the game's own -- starts from a clean system
        if with(|sh| sh.reset_in.is_some()) {
            reset_system();
        }
        if !super::menu_vendor::interface_enabled() {
            return Some(false);
        }
        let puppet = mirror_double();
        let pl = player();
        if puppet.is_null() || identity(&puppet) == identity(&pl) {
            return Some(false);
        }
        let male = gender_name(&pl) == "Male";
        let mut s = Session {
            phase: Phase::Preparing,
            age: 0.0,
            panel: panel.clone(),
            puppet,
            player: identity(&pl),
            leases: Vec::new(),
            leases_back: false,
            chosen: HashMap::new(),
            anchor: world_position(&pl),
            male,
            target: false,
            error: None,
            accepted: None,
            final_: None,
            reapplied: 0,
            rows: None,
            refresh_at: None,
            stage: 0,
            stage_at: 0.0,
            end_reason: "",
            end_mode: Mode::Here,
        };
        let prepared = (|| -> Result<(), String> {
            lease_slot(&mut s, tdb("AttachmentSlots.Head"), None)?;
            lease_slot(&mut s, tdb("AttachmentSlots.Eyes"), None)?;
            for path in [if male { "Items.CharacterCustomizationMaHead" } else { "Items.CharacterCustomizationWaHead" }, "Items.CharacterCustomizationArms"] {
                let item = rt::static_call("ItemID", "FromTDBID", &[A::Name(tdb(path))]).map(|o| Id16::of(&Val::of(&o))).unwrap_or_default();
                let slot = rt::static_call("EquipmentSystem", "GetPlacementSlot", &[item.a()]).map_or(0, |o| unsafe { *(o.as_ptr() as *const u64) });
                lease_slot(&mut s, slot, Some(item))?;
            }
            Ok(())
        })();
        let failed = prepared.err();
        s.error = failed.clone();
        with(|sh| sh.session = Some(s));
        if failed.is_some() {
            finish_session("prepare-failed", Mode::Here);
            return Some(false);
        }
    }
    let (phase, age, target) = with(|sh| sh.session.as_ref().map(|s| (s.phase, s.age, s.target))).unwrap_or((Phase::Failed, 0.0, false));
    if phase == Phase::Ready {
        with(|sh| {
            if let Some(s) = sh.session.as_mut() {
                s.phase = Phase::Building;
            }
        });
        return Some(true);
    }
    if phase == Phase::Failed || age > 8.0 {
        if target {
            rt::call(&system(), "CancelFinalizedStateUpdate", &[]);
        }
        finish_session("native-fallback", Mode::Now);
        return Some(false);
    }
    // the native request kept until the world editor is ready
    None
}
/// A.poll: the session's frame; true while the world editor waits to be shown.
pub(super) fn poll(dt: f64) -> bool {
    let dt = dt.max(0.0);
    if with(|sh| sh.session.is_none()) {
        let due = with(|sh| {
            sh.reset_in = sh.reset_in.map(|t| t - dt);
            sh.reset_in.is_some_and(|t| t <= 0.0)
        });
        if due {
            reset_system();
        }
        return false;
    }
    let pl = player();
    let (phase, age, who) = with(|sh| {
        let s = sh.session.as_mut().expect("session");
        s.age += dt;
        (s.phase, s.age, s.player)
    });
    if pl.is_null() || identity(&pl) != who {
        finish_session("player-changed", Mode::Here);
        return false;
    }
    if phase == Phase::Restoring {
        let step = with(|sh| sh.session.as_ref().map(|s| (s.stage, s.stage_at, s.target, s.puppet.clone(), s.end_reason, s.end_mode)));
        if let Some((stage, stage_at, target, puppet, reason, mode)) = step {
            if age >= stage_at {
                if stage == 1 && target && !puppet.is_null() {
                    rebuild();
                    with(|sh| {
                        if let Some(s) = sh.session.as_mut() {
                            (s.stage, s.stage_at) = (2, s.age + STEP);
                        }
                    });
                } else {
                    finish_session(reason, mode);
                }
            }
        }
        return false;
    }
    let (puppet, anchor) = with(|sh| sh.session.as_ref().map(|s| (s.puppet.clone(), s.anchor))).unwrap_or_default();
    if puppet.is_null() || distance(world_position(&pl), anchor) > 4.0 {
        cancel();
        return false;
    }
    if phase == Phase::Preparing && age >= 0.35 {
        prepare();
    }
    with(|sh| sh.session.as_ref().is_some_and(|s| s.phase == Phase::Building))
}
/// The double dressed by the system: its leased items on, the system's target, its option resources, the editor's tag;
/// every option applied again (the editor head and arms are new items every session -- unchanged options would put
/// nothing on them: no face, no hands); the finalized values kept for the cancel.
fn prepare() {
    let Some((puppet, male, leases)) = with(|sh| sh.session.as_ref().map(|s| (s.puppet.clone(), s.male, s.leases.iter().filter_map(|l| l.item.map(|_| l.slot)).collect::<Vec<_>>()))) else { return };
    if leases.iter().any(|&slot| item_in_slot(&puppet, slot).is_none()) {
        return;
    }
    let c = system();
    let result = (|| -> Result<(HashMap<String, i64>, usize, Vec<OptRow>), String> {
        if !creation_target(&puppet) {
            return Err("mirror target rejected".into());
        }
        with(|sh| {
            if let Some(s) = sh.session.as_mut() {
                s.target = true;
            }
        });
        reset_options();
        if native::<bool>("VRMirrorAppearanceOptions", &[Value::Handle(&puppet), Value::Bool(male)]) != Some(true) {
            return Err("mirror option resources not ready".into());
        }
        rt::call(&c, "InitializeOptionsFromFinalizedState", &[]);
        rt::call(&c, "ApplyEditTag", &[A::N(ev("gameuiCharacterCustomizationEditTag", "HairDresser"))]);
        let mut final_ = HashMap::new();
        let reapplied = reapply_all(&c, Some(&mut final_));
        let rows = rows();
        if rows.len() < 10 {
            return Err("incomplete appearance options".into());
        }
        Ok((final_, reapplied, rows))
    })();
    with(|sh| {
        let Some(s) = sh.session.as_mut() else { return };
        match result {
            Ok((final_, reapplied, rows)) => {
                (s.final_, s.reapplied, s.rows, s.phase) = (Some(final_), reapplied, Some(rows), Phase::Ready);
            }
            Err(e) => {
                (s.error, s.phase) = (Some(e), Phase::Failed);
            }
        }
    });
}
/// A.value: the row's shown index (a change just made, until the game reports it) and its label.
fn value(row: &OptRow) -> (i64, String) {
    let mut index = cur_index(&row.option);
    with(|sh| {
        if let Some(s) = sh.session.as_mut() {
            if let Some(&(chosen, at)) = s.chosen.get(&row.key) {
                if index != chosen && s.age - at < 2.0 {
                    index = chosen;
                } else {
                    s.chosen.remove(&row.key);
                }
            }
        }
    });
    if index < 0 || index as usize >= row.values.len() {
        index = 0;
    }
    let label = row.values.get(index as usize).cloned().unwrap_or_default();
    let text = format!("{}{}/{}", if label.is_empty() { String::new() } else { format!("{label}  ") }, index + 1, row.values.len());
    (index, text)
}
/// A.change: applied at once (the game's own deferred change, 0.3 s, could land after the session ended).
fn change(row: &OptRow, step: i64) -> bool {
    if !with(|sh| sh.session.as_ref().is_some_and(|s| s.phase == Phase::Active)) {
        return false;
    }
    let count = row.values.len() as i64;
    let index = (value(row).0 + step).rem_euclid(count);
    let ok = apply(&system(), &row.option, index);
    with(|sh| {
        if let Some(s) = sh.session.as_mut() {
            if ok {
                s.chosen.insert(row.key.clone(), (index, s.age));
                s.refresh_at = Some(s.age + 0.25);
            } else {
                s.error = Some("ApplyChangeToOption failed".into());
            }
        }
    });
    ok
}
/// A.action: ACCEPT finalizes, CANCEL undoes the preview; either ends the session in steps.
pub(super) fn act(accept: bool) -> bool {
    let Some((phase, target)) = with(|sh| sh.session.as_ref().map(|s| (s.phase, s.target))) else { return false };
    if phase == Phase::Restoring || (accept && phase != Phase::Active) {
        return false;
    }
    let ok = if accept {
        rt::call(&system(), "ReFinalizeState", &[]).is_none_or(|o| Val::of(&o).bool())
    } else if target {
        with(|sh| sh.session.as_ref().map(revert));
        rt::call(&system(), "CancelFinalizedStateUpdate", &[]).is_none_or(|o| Val::of(&o).bool())
    } else {
        true
    };
    with(|sh| {
        let Some(s) = sh.session.as_mut() else { return };
        if !ok {
            s.error = Some("the customization system refused".into());
            return;
        }
        s.accepted = Some(accept);
        close_session(s, if accept { "accepted" } else { "cancelled" }, Mode::Here);
    });
    ok
}
pub(super) fn cancel() -> bool {
    act(false)
}
/// A.native: the game's own editor instead -- the preview undone, the double rebuilt, then the retained request opens
/// that editor as if it had never been taken.
fn to_native() -> bool {
    if !with(|sh| sh.session.as_ref().is_some_and(|s| s.phase == Phase::Active)) {
        return false;
    }
    with(|sh| sh.session.as_ref().map(revert));
    rt::call(&system(), "CancelFinalizedStateUpdate", &[]);
    with(|sh| {
        if let Some(s) = sh.session.as_mut() {
            close_session(s, "native", Mode::Native);
        }
    });
    true
}
/// A.release: the menu's release -- the preview undone and the request given back at once.
pub(super) fn release() {
    let live = with(|sh| sh.session.as_ref().map(|s| (s.target, s.phase)));
    let Some((target, phase)) = live else {
        if with(|sh| sh.reset_in.is_some()) {
            reset_system();
        }
        return;
    };
    if target && phase != Phase::Restoring {
        with(|sh| sh.session.as_ref().map(revert));
        rt::call(&system(), "CancelFinalizedStateUpdate", &[]);
    }
    finish_session("released", Mode::Now);
}
/// A.present: the panel shown (building -> active); the rows read again after a change.
pub(super) fn present(m: &mut Menu) {
    if !m.menu.as_ref().is_some_and(|s| s.placed) {
        return;
    }
    let refresh = with(|sh| {
        let Some(s) = sh.session.as_mut() else { return false };
        if s.phase == Phase::Building {
            s.phase = Phase::Active;
        }
        if s.refresh_at.is_some_and(|t| s.age >= t) {
            s.refresh_at = None;
            return true;
        }
        false
    });
    if refresh {
        let r = rows();
        with(|sh| {
            if let Some(s) = sh.session.as_mut() {
                s.rows = Some(r);
            }
        });
        super::menu_main::build_panels(m, false, false);
    }
}
/// A.current: a session is on.
pub(super) fn current() -> bool {
    with(|sh| sh.session.is_some())
}

// ---------------------------------------------------------------- the panel (appearance_view.lua)
/// `x` is the panel's left edge on the menu canvas. The real mirror is the preview: the menu opens centred on it, so
/// the panel stands beside it.
pub(super) fn draw(m: &mut Menu, p: &Handle, x: f64) {
    let Some(rows) = with(|sh| sh.session.as_ref().and_then(|s| s.rows.clone())) else { return };
    let y = P1Y;
    glass(m, p, 820.0, 1140.0, Some((x, y)));
    k::text(p, 24.0, 8.0, 620.0, 70.0, &loc::tr("ВНЕШНОСТЬ", "APPEARANCE"), 40.0, k::CYAN, "left", false);
    // the game's own character editor, for what the panel does not offer
    let (nx, ny, ns) = (676.0, 10.0, 130.0);
    let nbg = k::round_fill(p, nx, ny, ns, ns, k::ICE, 0.12, Some(12.0));
    let icon = glyph(p, nx + ns / 2.0, ny + ns / 2.0, ns * 1.25, MIRROR, k::CYAN, 0.85);
    let b = button(
        m,
        "p1",
        &at(x, y),
        nx,
        ny,
        ns,
        ns,
        Some(Rc::new(|m: &mut Menu, _| {
            if to_native() {
                super::menu_main::close(m);
            }
        })),
        Some(Rc::new(move |h, _| {
            super::lobby_ink::opacity(&nbg, if h { 0.35 } else { 0.12 });
            super::lobby_ink::opacity(&icon, if h { 1.0 } else { 0.85 });
        })),
    );
    *b.key.borrow_mut() = Some("appearanceNative".into());
    let mut list_rows = Vec::new();
    for entry in rows {
        let (_, shown) = value(&entry);
        let entry = Rc::new(entry);
        list_rows.push(Row {
            h: 116.0,
            build: Rc::new(move |m: &mut Menu, c: &Handle, it: RowCtx| {
                k::text(c, 18.0, 0.0, it.cw - 36.0, 44.0, &k::clip(&entry.label, 38), 30.0, k::TEXT, "left", false);
                k::text(c, 80.0, 48.0, it.cw - 160.0, 48.0, &k::clip(&shown, 31), 28.0, k::CYAN, "center", false);
                for direction in [-1i64, 1] {
                    let bx = if direction < 0 { 12.0 } else { it.cw - 72.0 };
                    let bg = k::round_fill(c, bx, 48.0, 60.0, 48.0, k::ICE, 0.12, Some(6.0));
                    k::text(c, bx, 48.0, 60.0, 48.0, if direction < 0 { "<" } else { ">" }, 32.0, k::CYAN, "center", false);
                    let e2 = entry.clone();
                    let b = button(
                        m,
                        "p1",
                        &At { ox: it.vx, oy: it.y, view: Some(it.view.clone()) },
                        bx,
                        48.0,
                        60.0,
                        48.0,
                        Some(Rc::new(move |m: &mut Menu, _| {
                            change(&e2, direction);
                            super::menu_main::build_panels(m, false, false);
                        })),
                        Some(Rc::new(move |h, _| super::lobby_ink::opacity(&bg, if h { 0.35 } else { 0.12 }))),
                    );
                    *b.key.borrow_mut() = Some(format!("appearance:{}:{direction}", entry.key));
                }
            }),
        });
    }
    let keep = m.keep_scroll;
    list(m, p, "p1", "appearance", x, y, 150.0, 792.0, list_rows, 830.0, keep);
    k::text(p, 24.0, 980.0, 772.0, 40.0, &loc::tr("Изменения видны в зеркале", "Changes are shown in the mirror"), 24.0, k::TEXT, "left", false);
    for (bx, label, accept, key) in [(25.0, loc::tr("ОТМЕНА", "CANCEL"), false, "appearanceCancel"), (425.0, loc::tr("ПРИНЯТЬ", "ACCEPT"), true, "appearanceAccept")] {
        let b = action(
            m,
            p,
            "p1",
            &at(x, y),
            bx,
            1030.0,
            370.0,
            76.0,
            &label,
            true,
            Rc::new(move |m: &mut Menu, _| {
                if act(accept) {
                    super::menu_main::close(m);
                }
            }),
            Some(k::CYAN),
            None,
        );
        *b.key.borrow_mut() = Some(key.into());
    }
}
