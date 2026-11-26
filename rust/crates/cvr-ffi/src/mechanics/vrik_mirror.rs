//! The last of mods/cet/CyberpunkVRPort_VRIK's per-frame Lua (bit 0 of VRRustMechanics2): the mirror double and the
//! console's EquipCyberware.
//!
//! THE MIRROR DOUBLE (src/Natives/VrikArming.cpp). Looking into a mirror is a scene that spawns an NPC copy of V at the
//! mirror (Character.q110_tpp_male_v in V's bathroom); the reflection shows that copy, not the player. A puppet whose
//! record is a TPP V of the player's gender, within FIND_M while the mirror's own hub (title "Mirror", with a leave
//! choice) is up, is bound (VRMirrorDouble), and it takes the tracked pose. Only a mirror the player turns on: a story
//! scene that stands V at a mirror spawns the same double but shows no "Mirror" hub, and nothing is bound there; Johnny
//! in the mirror (fact johnny_mirror) is the game's. Once the mirror's hub is up the player is let out of the scene's
//! workspot and walks on; the double follows by an AI teleport command; X on the same prompt turns the mirror off again
//! (its highlight put on the leave choice once, for up to 3 s); further than OFF_M from where it started the mirror
//! turns itself off by its own leave choice, handed to the hub through VRInteractionAction.
//!
//! EQUIPCYBERWARE (GetMod("CyberpunkVRPort_VRIK").EquipCyberware(kind), the console's test helper; the Lua host keeps
//! the function and asks VRIKEquipCyberware / VRIKCyberwareStatus): gorilla / mantis / monowire / launcher -- the
//! legendary implant found in the inventory (given when missing), the weapons holstered, then once nothing is in the
//! hands and the arms' visual is attached the arms slot swapped, then the implant drawn (not the launcher).
use super::lobby_rtti::{self as rt, Val, A};
use super::menu_rt::{self as r, cname, ev, items, Id16};
use cvr_host::log;
use red4ext_abi::handle::Handle;
use std::sync::Mutex;

const HUB_TITLE: &str = "LocKey#46420";
/// Seconds of the mirror's hub on screen before the player is let out (when the prompt was not seen being taken).
const FREE_AFTER: f64 = 0.2;
/// Metres from where the mirror started: it turns itself off.
const OFF_M: f64 = 4.0;
/// Seconds between the hub actions of a leave.
const STEP: f64 = 0.3;
/// Metres around the player the double is looked for once the mirror's hub is up.
const FIND_M: f64 = 15.0;
/// VRIKMirrorDoubleFlags' default (bit 0: identify its passes by buffer A as well; the harness sets it in Lua).
const FLAGS: f64 = 0.0;

const RECORDS: [(&str, &str); 4] = [
    ("gorilla", "Items.AdvancedStrongArmsLegendary"),
    ("mantis", "Items.AdvancedMantisBladesLegendary"),
    ("monowire", "Items.AdvancedNanoWiresLegendary"),
    ("launcher", "Items.AdvancedProjectileLauncherLegendary"),
];

struct Mirror {
    id: u64,
    anchor: [f64; 3],
    hub_t: f64,
    freed: bool,
    step_t: f64,
    yaw: Option<f64>,
    taken: bool,
    homed: bool,
    home_t: f64,
}
#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Holstering,
    Equipping,
    Drawing,
}
struct Pending {
    owner: u64,
    id: Id16,
    kind: String,
    elapsed: f64,
    stable: f64,
    phase: Phase,
    uninstall: Handle,
    install: Handle,
}
struct State {
    timer: f64,
    seen_t: f64,
    bound: bool,
    mirror: Option<Mirror>,
    pending: Option<Pending>,
    status: String,
}
// SAFETY: the handles are made, used and dropped on the game's main thread, where the mechanics tick.
unsafe impl Send for State {}
static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with<T>(f: impl FnOnce(&mut State) -> T) -> T {
    let mut g = STATE.lock().unwrap_or_else(|p| p.into_inner());
    f(g.get_or_insert_with(|| State { timer: 0.0, seen_t: 99.0, bound: false, mirror: None, pending: None, status: "idle".into() }))
}

fn pos(e: &Handle) -> Option<[f64; 3]> {
    let o = rt::call(e, "GetWorldPosition", &[])?;
    let v = Val::of(&o);
    Some([r::fnum(&v, "X"), r::fnum(&v, "Y"), r::fnum(&v, "Z")])
}
fn hash(e: &Handle) -> u64 {
    super::belt::entity_hash(e).unwrap_or(0)
}
fn workspots() -> Handle {
    super::game_system("GetWorkspotSystem")
}
fn fast_exit(ws: &Handle, actor: &Handle) {
    let zero = r::new_struct("Vector3");
    let z = zero.as_ref().map_or(A::D, |o| A::P(r::ptr(o)));
    rt::call(ws, "SendFastExitSignal", &[A::H(actor), z, A::B(false), A::B(false), A::B(true), A::B(false)]);
}
fn hub_action(action: &str) {
    rt::global_call("VRInteractionAction", &[A::Name(cname(action))]);
}

/// The mirror's hub on screen: its id, the index of its leave choice (None when it has none), the active hub's id and
/// the highlighted index. None when no mirror hub is up.
fn mirror_hub() -> Option<(i64, Option<i64>, i64, i64)> {
    let defs = super::boards::defs().ok()?;
    let dd = super::boards::definition(&defs, "UIInteractions").ok()?;
    let bb = super::boards::system_board(dd).ok()?;
    if bb.is_null() {
        return None;
    }
    let f = super::boards::field(dd, "DialogChoiceHubs").ok()?;
    let v = rt::call(&bb, "GetVariant", &[A::P(f.cast_const())])?;
    let lists = r::variant_inner(&v)?;
    let hubs = lists.f("choiceHubs")?;
    for hub in items(&hubs) {
        if r::ftext(&hub, "title") != HUB_TITLE {
            continue;
        }
        let mut leave = None;
        if let Some(choices) = hub.f("choices") {
            for (i, c) in items(&choices).iter().enumerate() {
                let props = c.f("type").map_or(0.0, |t| r::fnum(&t, "properties")) as i64;
                if leave.is_none() && props % 2 == 1 {
                    leave = Some(i as i64);
                }
            }
        }
        let int = |name: &str| -> i64 {
            super::boards::field(dd, name).ok().and_then(|id| rt::call(&bb, "GetInt", &[A::P(id.cast_const())])).and_then(|o| Val::of(&o).num()).unwrap_or(-1.0) as i64
        };
        return Some((r::fnum(&hub, "id") as i64, leave, int("ActiveChoiceHubID"), int("SelectedIndex")));
    }
    None
}
/// One step of leaving the mirror by its leave choice: the highlight moved onto it, then applied.
fn step_leave() {
    let Some((id, Some(leave), active, selected)) = mirror_hub() else { return };
    if active != id {
        return;
    }
    hub_action(if selected > leave {
        "ChoiceScrollUp"
    } else if selected < leave {
        "ChoiceScrollDown"
    } else {
        "ChoiceApply"
    });
}
/// A puppet near the player whose record is a TPP V of the player's gender.
fn find_double(player: &Handle, within: f64) -> Option<Handle> {
    let mut q = r::new_struct("TargetSearchQuery")?;
    let qv = r::val(&q);
    r::set(&qv, "maxDistance", within);
    r::set(&qv, "testedSet", ev("TargetingSet", "Complete"));
    let filter = rt::global_call("TSF_All", &[A::N(ev("TSFMV", "Obj_Puppet"))])?;
    if let Some(sf) = qv.f("searchFilter") {
        // SAFETY: the filter field and the global's result of the same type.
        unsafe { sf.ty.assign(sf.ptr, filter.as_ptr().cast()) };
    }
    let targeting = super::game_system("GetTargetingSystem");
    let (_, outs) = rt::call_outs(&targeting, "GetTargetParts", &[A::H(player), A::P(q.as_mut_ptr().cast_const().cast())], 1)?;
    let parts = outs.first()?;
    let me = hash(player);
    let female = {
        let g = rt::call(player, "GetResolvedGenderName", &[]).map(|o| Val::of(&o).text()).unwrap_or_default();
        g == "Female"
    };
    for part in items(&Val::of(parts)) {
        let comp = rt::static_call("TS_TargetPartInfo", "GetComponent", &[A::P(part.ptr.cast_const())]).map(|o| Val::of(&o).handle()).unwrap_or_else(Handle::null);
        let e = r::o(&comp, "GetEntity", &[]);
        if e.is_null() || hash(&e) == me || !r::is_a(&e, "NPCPuppet") {
            continue;
        }
        let rec = r::id(&e, "GetRecordID", &[]);
        let name = super::weapon::tdbid_name(rec).unwrap_or_default().to_lowercase();
        if name.contains("tpp") && name.ends_with("_v") && name.contains("female") == female {
            return Some(e);
        }
    }
    None
}
fn bind(double: &Handle) -> bool {
    rt::global_call("VRMirrorDouble", &[A::H(double), A::N(FLAGS)]).and_then(|o| Val::of(&o).num()).is_some_and(|v| v != 0.0)
}
fn unbind() {
    rt::global_call("VRMirrorDoubleClear", &[]);
}

fn mirror_frame(s: &mut State, player: &Handle, dt: f64) {
    let ws = workspots();
    let Some(m) = s.mirror.as_mut() else {
        s.timer += dt;
        s.seen_t += dt;
        if s.timer < 0.1 {
            return;
        }
        s.timer = 0.0;
        let hub = mirror_hub();
        if hub.is_some() {
            s.seen_t = 0.0;
        }
        let in_workspot = r::b(&ws, "IsActorInWorkspot", &[A::H(player)]);
        let mut double = None;
        if hub.is_some_and(|h| h.1.is_some()) {
            let quests = super::game_system("GetQuestsSystem");
            let fact = r::Arg::new("johnny_mirror");
            if r::n(&quests, "GetFactStr", &[fact.a()]) <= 0.0 {
                double = find_double(player, FIND_M);
            }
        }
        if let Some(d) = double {
            if bind(&d) {
                s.bound = true;
                // a mirror's scene (its prompt was just taken, or its hub is up): no workspot from the first frame
                s.mirror = Some(Mirror {
                    id: hash(&d),
                    anchor: pos(player).unwrap_or_default(),
                    hub_t: 0.0,
                    freed: !in_workspot,
                    step_t: 0.0,
                    yaw: None,
                    taken: s.seen_t < 3.0,
                    homed: false,
                    home_t: 0.0,
                });
                log!("[mechanics] vrik: mirror double bound from Rust ({:#x})\n", hash(&d));
            }
        }
        return;
    };
    let double = super::find_entity(m.id).unwrap_or_else(|_| Handle::null());
    if double.is_null() {
        unbind();
        s.bound = false;
        s.mirror = None;
        return;
    }
    s.timer += dt;
    if s.timer >= 0.25 {
        s.timer = 0.0;
        bind(&double); // cheap for the same entity; rebinds a rebuilt rig
    }
    // in the scene's workspot (at the start, or put back by a hub branch): let out once the mirror's hub is up
    if r::b(&ws, "IsActorInWorkspot", &[A::H(player)]) {
        m.freed = false;
        if mirror_hub().is_some() {
            m.hub_t += dt;
        } else {
            m.hub_t = 0.0;
        }
        if m.taken || m.hub_t >= FREE_AFTER {
            m.taken = false;
            fast_exit(&ws, player);
            m.freed = true;
            m.hub_t = 0.0;
        }
        return;
    }
    if !m.freed {
        return;
    }
    // X on the same prompt turns it off: the highlight onto the leave choice, once, for up to 3 s
    if !m.homed {
        m.home_t += dt;
        m.step_t -= dt;
        if m.home_t > 3.0 {
            m.homed = true;
        } else if m.step_t <= 0.0 {
            m.step_t = STEP;
            if let Some((id, Some(leave), active, selected)) = mirror_hub() {
                if active == id {
                    if selected == leave {
                        m.homed = true;
                    } else {
                        hub_action(if selected > leave { "ChoiceScrollUp" } else { "ChoiceScrollDown" });
                    }
                }
            }
        }
    }
    // the double where the player is
    if r::b(&ws, "IsActorInWorkspot", &[A::H(&double)]) {
        fast_exit(&ws, &double);
    }
    let (Some(p), Some(dp)) = (pos(player), pos(&double)) else { return };
    let yaw = r::n(player, "GetWorldYaw", &[]);
    let (dx, dy, dz) = (p[0] - dp[0], p[1] - dp[1], p[2] - dp[2]);
    if dx * dx + dy * dy + dz * dz > 1e-4 || m.yaw.is_none_or(|y| (yaw - y).abs() > 0.5) {
        let cmd = super::lobby_ink::new("AITeleportCommand");
        if let Some(f) = rt::field(&cmd, "position") {
            for (i, axis) in ["X", "Y", "Z"].iter().enumerate() {
                r::set(&f, axis, p[i]);
            }
            r::set(&f, "W", 1.0);
        }
        r::hset(&cmd, "rotation", yaw);
        r::hset(&cmd, "doNavTest", 0.0);
        let ai = r::o(&double, "GetAIControllerComponent", &[]);
        rt::call(&ai, "SendCommand", &[A::H(&cmd)]);
        m.yaw = Some(yaw);
    }
    // far from the mirror: its own leave choice
    let (ax, ay, az) = (p[0] - m.anchor[0], p[1] - m.anchor[1], p[2] - m.anchor[2]);
    if (ax * ax + ay * ay + az * az).sqrt() > OFF_M {
        m.step_t -= dt;
        if m.step_t <= 0.0 {
            m.step_t = STEP;
            step_leave();
        }
    }
}

fn equip_system() -> Handle {
    let c = super::game_system("GetScriptableSystemsContainer");
    r::o(&c, "Get", &[A::Name(cname("EquipmentSystem"))])
}
fn slot_item(ts: &Handle, player: &Handle, slot: &str) -> Handle {
    r::o(ts, "GetItemInSlot", &[A::H(player), A::Name(red4ext_abi::tweakdbid::from_name(slot))])
}
/// The implant of `record` in the player's inventory.
fn find_item(ts: &Handle, player: &Handle, record: u64) -> Option<Id16> {
    let (_, outs) = rt::call_outs(ts, "GetItemList", &[A::H(player)], 1)?;
    let list = outs.first()?;
    items(&Val::of(list)).iter().map(|d| r::id16(&d.handle(), "GetID", &[])).find(|id| id.tdbid() == record)
}
fn new_request(class: &str, player: &Handle) -> Handle {
    let req = super::lobby_ink::new(class);
    super::menu_invdata::hset_handle(&req, "owner", player);
    req
}

/// EquipCyberware(kind): Ok(record) when the change began, else the reason.
pub(crate) fn equip_cyberware(kind: &str) -> Result<String, String> {
    if with(|s| s.pending.is_some()) {
        return Err("A cyberware change is already in progress".into());
    }
    let record = RECORDS.iter().find(|(k, _)| *k == kind).map(|(_, rec)| *rec);
    let player = r::player();
    let (Some(record), false) = (record, player.is_null()) else {
        return Err("Use gorilla, mantis, monowire or launcher after loading a save".into());
    };
    let ts = super::game_system("GetTransactionSystem");
    let rec_id = r::tdb(record);
    let mut id = find_item(&ts, &player, rec_id);
    if id.is_none() {
        if let Some(fresh) = rt::static_call("ItemID", "FromTDBID", &[A::Name(rec_id)]) {
            let fresh = Id16::of(&Val::of(&fresh));
            rt::call(&ts, "GiveItem", &[A::H(&player), fresh.a(), A::N(1.0)]);
        }
        id = find_item(&ts, &player, rec_id);
    }
    let Some(id) = id else { return Err("The implant could not be found in the inventory".into()) };
    let equipment = equip_system();
    let holster = new_request("EquipmentSystemWeaponManipulationRequest", &player);
    let uninstall = new_request("UnequipRequest", &player);
    // the native RTTI name, not the REDscript EquipRequest alias
    let install = new_request("gameEquipRequest", &player);
    if equipment.is_null() || holster.is_null() || uninstall.is_null() || install.is_null() {
        return Err("Native equipment requests unavailable".into());
    }
    r::hset(&holster, "requestType", ev("EquipmentManipulationAction", "UnequipAll"));
    r::hset(&uninstall, "areaType", ev("gamedataEquipmentArea", "ArmsCW"));
    r::hset(&uninstall, "slotIndex", 0.0);
    super::menu_invdata::hset_id(&install, "itemID", &id);
    r::hset(&install, "slotIndex", 0.0);
    r::hset(&install, "addToInventory", 0.0);
    let owner = hash(&player);
    with(|s| {
        s.pending = Some(Pending { owner, id, kind: kind.to_string(), elapsed: 0.0, stable: 0.0, phase: Phase::Holstering, uninstall, install });
        s.status = "holstering".into();
    });
    rt::call(&equipment, "QueueRequest", &[A::H(&holster)]);
    log!("[mechanics] vrik: EquipCyberware({kind}) from Rust -- {record}\n");
    Ok(record.to_string())
}
pub(crate) fn cyberware_status() -> String {
    with(|s| s.status.clone())
}
fn finish(s: &mut State, status: &str) {
    s.pending = None;
    s.status = status.to_string();
}
fn cyberware_frame(s: &mut State, dt: f64) {
    let Some(p) = s.pending.as_mut() else { return };
    p.elapsed += dt;
    let player = r::player();
    if player.is_null() || hash(&player) != p.owner {
        return finish(s, "cancelled: player changed");
    }
    if p.elapsed > 8.0 {
        return finish(s, "error: equipment transition timed out");
    }
    let equipment = equip_system();
    let data = r::o(&equipment, "GetPlayerData", &[A::H(&player)]);
    let ts = super::game_system("GetTransactionSystem");
    let right = slot_item(&ts, &player, "AttachmentSlots.WeaponRight");
    let left = slot_item(&ts, &player, "AttachmentSlots.WeaponLeft");
    let arms_cw = ev("gamedataEquipmentArea", "ArmsCW");
    let active = || rt::call_typed(&data, "GetActiveItem", &["gamedataEquipmentArea"], &[A::N(arms_cw)]).map(|o| Id16::of(&Val::of(&o))).unwrap_or_default();
    match p.phase {
        Phase::Holstering => {
            // A queued holster is not a completed holster: installing while the old melee cyberware is still drawn
            // leaves its additional appearances on the player. Wait for the native detach and the holstered visual.
            let arms = slot_item(&ts, &player, "AttachmentSlots.RightArm");
            let pending = r::b(&data, "IsEquipPending", &[]);
            let attached = !arms.is_null() && r::b(&arms, "IsVisualSpawnAttached", &[]);
            if !right.is_null() || !left.is_null() || pending || !attached {
                if p.elapsed > 7.9 && p.elapsed - dt <= 7.9 {
                    let worn: Vec<&str> = ["Torso", "Chest", "Legs", "Feet", "Head", "LeftArm", "RightArm", "ArmsCW"]
                        .into_iter()
                        .filter(|sl| !slot_item(&ts, &player, &format!("AttachmentSlots.{sl}")).is_null())
                        .collect();
                    log!("[mechanics] vrik: slots holding an item: {worn:?} (from Rust)\n");
                    log!(
                        "[mechanics] vrik: holstering waits: right {} left {} pending {pending} arms {} attached {attached} (from Rust)\n",
                        !right.is_null(),
                        !left.is_null(),
                        !arms.is_null()
                    );
                }
                p.stable = 0.0;
                return;
            }
            p.stable += dt;
            if p.stable < 0.25 {
                return;
            }
            if active() != p.id {
                rt::call(&equipment, "QueueRequest", &[A::H(&p.uninstall)]);
                rt::call(&equipment, "QueueRequest", &[A::H(&p.install)]);
            }
            p.phase = Phase::Equipping;
            p.stable = 0.0;
            s.status = "equipping".into();
        }
        Phase::Equipping => {
            if r::b(&data, "IsEquipPending", &[]) || active() != p.id {
                p.stable = 0.0;
                return;
            }
            p.stable += dt;
            if p.stable < 0.25 {
                return;
            }
            if p.kind == "launcher" {
                return finish(s, "ready");
            }
            let draw = new_request("EquipmentSystemWeaponManipulationRequest", &player);
            r::hset(&draw, "requestType", ev("EquipmentManipulationAction", "RequestActiveMeleeware"));
            rt::call(&equipment, "QueueRequest", &[A::H(&draw)]);
            p.phase = Phase::Drawing;
            s.status = "drawing".into();
        }
        Phase::Drawing => {
            if !right.is_null() && r::id16(&right, "GetItemID", &[]) == p.id && r::b(&right, "IsVisualSpawnAttached", &[]) {
                finish(s, "ready");
            }
        }
    }
}

/// A development check (CVR_VRIK_TEST=<kind>, inherited by the game from the probe session): 20 s into the world the
/// change is asked for as the console would, and each status it passes through is logged.
fn self_test(dt: f64) {
    thread_local! {
        static T: std::cell::Cell<(f64, bool)> = const { std::cell::Cell::new((0.0, false)) };
        static LAST: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
    }
    let Ok(kind) = std::env::var("CVR_VRIK_TEST") else { return };
    let (t, asked) = T.with(|c| c.get());
    let t = t + dt;
    T.with(|c| c.set((t, asked)));
    if !asked && t > 20.0 {
        T.with(|c| c.set((t, true)));
        let r = equip_cyberware(&kind);
        log!("[mechanics] vrik: self-test EquipCyberware({kind}) -> {r:?} (from Rust)\n");
    }
    if asked {
        let now = cyberware_status();
        let changed = LAST.with(|l| {
            let mut l = l.borrow_mut();
            let c = *l != now;
            *l = now.clone();
            c
        });
        if changed {
            log!("[mechanics] vrik: cyberware status {now} at {t:.1} s (from Rust)\n");
        }
    }
}

/// Every frame in the world (the mechanics' tick): the cyberware change, then the mirror double.
pub(super) fn tick(dt: f64) {
    let player = r::player();
    if !player.is_null() {
        self_test(dt);
    }
    with(|s| {
        cyberware_frame(s, dt);
        if player.is_null() {
            return;
        }
        mirror_frame(s, &player, dt);
    });
}
/// The plugin's exit: the double let go.
pub(super) fn shutdown() {
    with(|s| {
        if s.bound {
            unbind();
        }
        s.bound = false;
        s.mirror = None;
        s.pending = None;
        s.status = "idle".into();
    });
}
