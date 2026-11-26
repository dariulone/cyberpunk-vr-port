//! vendor_visit.lua and stash_visit.lua: a shop's or a ripperdoc's visit taken over from the game's own menu -- the
//! request intercepted before its fullscreen widgets exist (VendorRoute's native queue, UISystem.RequestVendorMenu), a
//! scene-driven visit's native hub initialised hidden and closed through its own event queue, the menu layer's
//! presentation suppressed meanwhile and restored after, the UI_Vendor board's open / close signals for the quest
//! dialogue; a storage container's stash the same way (OnOpenStorage). The game's hooks stay in the Lua host
//! (CyberpunkVRPort_Menu/init.lua) and report here through VRMenuHook; the state is the main thread's own, not the
//! menu's (a hook can fire inside a menu call -- RequestVendorMenu from TRADE).
use super::lobby_ink as ink;
use super::lobby_rtti::{self as rt, A};
use super::menu_core::Menu;
use super::menu_rt::*;
use red4ext_abi::handle::Handle;
use std::cell::RefCell;

#[derive(Default)]
pub(super) struct Purchase {
    pub record: u64,
    pub slot: i32,
    pub area: String,
    pub before: std::collections::HashSet<String>,
    pub at: f64,
    pub polled: Option<f64>,
}
#[derive(Default)]
pub(super) struct Visit {
    pub vendor: Handle,
    pub manager: Handle,
    pub panel: Handle,
    scenario: Handle,
    pub pending: bool,
    pub active: bool,
    pub ripperdoc: bool,
    /// A Glamdoc (NPCPuppet.IsGlamdoc): its section is the world menu's Glamdoc.
    pub glamdoc: bool,
    started: f64,
    native_handoff: bool,
    controller: Handle,
    hub_ready_at: f64,
    close_queued: bool,
    hub_closed: bool,
    board_open: bool,
    transferring: bool,
    pub purchase: Option<Purchase>,
}
#[derive(Default)]
pub(super) struct StashVisit {
    pub object: Handle,
    pub manager: Handle,
    pending: bool,
    pub active: bool,
    close_native: bool,
    transferring: bool,
}
#[derive(Default)]
struct VShared {
    menu_controller: Handle,
    ripper: Option<Visit>,
    handoff: bool,
    presentation: Option<(Handle, f64)>,
    presentation_early: bool,
    trade_opening: Option<(String, bool)>,
    live_hub: Option<(Handle, String)>,
    stash: Option<StashVisit>,
    clock: f64,
}
thread_local! {
    static VS: RefCell<VShared> = RefCell::new(VShared::default());
}
fn with<R>(f: impl FnOnce(&mut VShared) -> R) -> R {
    VS.with(|v| f(&mut v.borrow_mut()))
}
/// availability.lua: one gate for direct opens and the native vendor / storage interception (xr_vr_interface).
pub(super) fn interface_enabled() -> bool {
    cvr_settings::lc!(xrVrInterface) != 0
}
/// V.section: the menu section a visit opens.
fn section_of(ripperdoc: bool, glamdoc: bool) -> &'static str {
    if glamdoc {
        "glamdoc"
    } else if !ripperdoc {
        "trade"
    } else {
        "cyberware"
    }
}
/// The section the active visit opens, when there is one within reach.
pub(super) fn current_section(m: &Menu) -> Option<&'static str> {
    current_ripperdoc(m)?;
    with(|v| v.ripper.as_ref().map(|r| section_of(r.ripperdoc, r.glamdoc)))
}
/// The active Glamdoc visit within reach: its vendor and manager.
pub(super) fn glamdoc_visit() -> Option<(Handle, Handle)> {
    let (active, glamdoc, vendor, manager) = with(|v| v.ripper.as_ref().map(|r| (r.active, r.glamdoc, r.vendor.clone(), r.manager.clone())))?;
    (active && glamdoc && !manager.is_null() && near(&vendor)).then_some((vendor, manager))
}
/// The in-game menu's controller, process-wide as main's shared.menuController was (its OnInitialize hook and the
/// menu's tick need not run on one thread: kept in the thread's own VShared, the tick's copy stayed null -- every game
/// tooltip fell back to the menu's card, without the weapon's stats).
struct SharedController(Handle);
// SAFETY: a handle the game's threads share as the Lua's table did; read and replaced under the lock.
unsafe impl Send for SharedController {}
static MENU_CONTROLLER: std::sync::Mutex<Option<SharedController>> = std::sync::Mutex::new(None);
fn set_menu_controller(h: &Handle, from: i32) {
    let mut g = MENU_CONTROLLER.lock().unwrap_or_else(|p| p.into_inner());
    let same = g.as_ref().is_some_and(|c| c.0.instance == h.instance);
    if !same {
        cvr_host::log!(
            "[VRMenu] the in-game menu's controller from hook {from} on thread {:?} (null {}) (from Rust)\n",
            std::thread::current().id(),
            h.is_null()
        );
    }
    *g = Some(SharedController(h.clone()));
}
pub(super) fn menu_controller() -> Handle {
    MENU_CONTROLLER.lock().unwrap_or_else(|p| p.into_inner()).as_ref().map_or_else(Handle::null, |c| c.0.clone())
}
/// The ripperdoc visit, borrowed for `f` (no game call that can reach a hook may be made inside).
pub(super) fn with_visit<R>(f: impl FnOnce(Option<&mut Visit>) -> R) -> R {
    with(|v| f(v.ripper.as_mut()))
}
pub(super) fn with_stash<R>(f: impl FnOnce(Option<&mut StashVisit>) -> R) -> R {
    with(|v| f(v.stash.as_mut()))
}
fn hash_of(e: &Handle) -> String {
    id(e, "GetEntityID", &[]).to_string()
}
fn near(a: &Handle) -> bool {
    let p = player();
    if p.is_null() || a.is_null() {
        return false;
    }
    let (Ok(pp), Ok(vp)) = (super::belt::m::<[f32; 4]>(&p, "GetWorldPosition", &[]), super::belt::m::<[f32; 4]>(a, "GetWorldPosition", &[])) else { return false };
    let d: f32 = (0..3).map(|i| (pp[i] - vp[i]).powi(2)).sum();
    d <= 64.0
}

// ---------------------------------------------------------------- the visit's signals and the native shell
fn vendor_board(active: bool) {
    let Ok(defs) = super::boards::defs() else { return };
    let Ok(dd) = super::boards::definition(&defs, "UI_Vendor") else { return };
    let (Ok(bb), Ok(f)) = (super::boards::system_board(dd), super::boards::field(dd, "VendorData")) else { return };
    let Some(data) = new_struct("VendorData") else { return };
    set(&val(&data), "isActive", if active { 1.0 } else { 0.0 });
    let Some(var) = variant_of(&data) else { return };
    rt::call(&bb, "SetVariant", &[A::P(f.cast_const()), A::P(var.as_ptr().cast()), A::B(true)]);
}
fn open_board(v: &mut Visit) -> bool {
    if v.board_open {
        return false;
    }
    v.board_open = true;
    true
}
fn hide_presentation() {
    let have = with(|v| v.presentation.as_ref().map(|p| p.0.clone()));
    let root = match have {
        Some(r) => r,
        None => {
            let layer = super::lobby_spawn::native_handle("VRInkLayer", &[super::lobby_spawn::cn("inkMenuLayer")]);
            let root = ink::child(&layer, "GetVirtualWindow", &[]);
            if root.is_null() {
                return;
            }
            let op = ink::get::<f32>(&root, "GetOpacity", &[]).map_or(1.0, f64::from);
            with(|v| v.presentation = Some((root.clone(), op)));
            root
        }
    };
    ink::opacity(&root, 0.001);
}
fn restore_presentation_vs() {
    let p = with(|v| {
        v.presentation_early = false;
        v.presentation.take()
    });
    if let Some((root, op)) = p {
        ink::opacity(&root, op);
    }
}
fn cancel_handoff() {
    with(|v| {
        if v.handoff {
            v.ripper = None;
        }
        v.handoff = false;
    });
    restore_presentation_vs();
}
fn hide_hub(controller: &Handle) {
    let root = o(controller, "GetRootWidget", &[]);
    if !root.is_null() {
        ink::opacity(&root, 0.001);
        ink::visible(&root, true);
    }
}
fn panel_entity_hash(panel: &Handle) -> Option<String> {
    let d = pf(panel, "data")?;
    let e = d.f("entityID")?;
    Some(fid(&e, "hash").to_string())
}
fn capture_hub(controller: &Handle, panel: &Handle) {
    if panel.is_null() || controller.is_null() {
        return;
    }
    let Some(eh) = panel_entity_hash(panel) else { return };
    let clock = with(|v| {
        v.live_hub = Some((controller.clone(), eh.clone()));
        v.clock
    });
    let take = with(|v| {
        let handoff = v.handoff;
        match v.ripper.as_mut() {
            Some(vis) if handoff && hash_of(&vis.vendor) == eh => {
                vis.controller = controller.clone();
                vis.hub_ready_at = clock;
                true
            }
            _ => false,
        }
    });
    if take {
        hide_hub(controller);
    }
}
fn eligible(panel: &Handle) -> Option<(Handle, bool, bool)> {
    let d = pf(panel, "data")?;
    let e = d.f("entityID")?;
    let eid = Id16::of(&e);
    let g = rt::game();
    let gi = A::P((&g as *const crate::script_call::GameInstance).cast());
    let mut object = rt::global_obj("FindEntityByID", &[A::P((&g as *const crate::script_call::GameInstance).cast()), eid.a()]);
    if object.is_null() {
        object = so("ScriptGameInstance", "FindEntityByID", &[gi, eid.a()]);
    }
    if object.is_null() {
        return None;
    }
    let ripper = b(&object, "IsRipperdoc", &[]);
    let glam = b(&object, "IsGlamdoc", &[]);
    let qs = super::game_system("GetQuestsSystem");
    let fact = |n: &str| {
        let a = Arg::new(n);
        super::menu_rt::n(&qs, "GetFactStr", &[a.a()])
    };
    let vendor_id = rt::static_call("MarketSystem", "GetVendorID", &[A::H(&object)]).map_or(0, |o| unsafe { *(o.as_ptr() as *const u64) });
    if vendor_id == 0 || !sb("TDBID", "IsValid", &[A::Name(vendor_id)]) {
        return None;
    }
    if ripper && vendor_id == tdb("Vendors.wat_lch_ripperdoc_01") && fact("q001_ripperdoc_done") == 0.0 {
        return None;
    }
    let tutorial = (fact("mq048_active") > 0.0 && fact("mq048_done") == 0.0) || fact("ep1_standalone") > 0.0;
    if ripper && tutorial && fact("ep1_ripperdoc_tutorial_seen") == 0.0 {
        return None;
    }
    Some((object, ripper, glam))
}
/// V.begin: a visit made of the panel's vendor (false: the game's own menu goes on).
fn begin(panel: &Handle, scenario: &Handle) -> bool {
    if !interface_enabled() {
        return false;
    }
    let Some((vendor, ripper, glamdoc)) = eligible(panel) else { return false };
    let vh = hash_of(&vendor);
    if with(|v| v.trade_opening.as_ref().is_some_and(|t| Some(&t.0) == panel_entity_hash(panel).as_ref())) {
        return false;
    }
    let scen = !scenario.is_null();
    if scen && with(|v| v.handoff && v.ripper.as_ref().is_some_and(|r| hash_of(&r.vendor) == vh)) {
        return false;
    }
    if scen {
        hide_presentation();
    }
    let manager = ink::new("VendorDataManager");
    let veid = id16(&vendor, "GetEntityID", &[]);
    rt::call_typed(&manager, "Initialize", &["whandle:GameObject", "entEntityID"], &[A::H(&player()), veid.a()]);
    let started = with(|v| v.clock);
    let visit = Visit { vendor, manager, panel: panel.clone(), scenario: scenario.clone(), pending: true, ripperdoc: ripper, glamdoc, started, native_handoff: scen, ..Default::default() };
    let hub = with(|v| {
        v.ripper = Some(visit);
        if scen {
            v.handoff = true;
            v.live_hub.clone().filter(|h| h.1 == vh)
        } else {
            None
        }
    });
    if let Some((ctrl, _)) = hub {
        capture_hub(&ctrl, panel);
    }
    true
}
fn poll_native() {
    let pl = player();
    let ui = super::game_system("GetUISystem");
    let panel = super::lobby_spawn::native_handle("VRMenuVendorPoll", &[crate::script_call::Value::Bool(interface_enabled() && !pl.is_null()), crate::script_call::Value::Handle(&ui)]);
    if panel.is_null() {
        return;
    }
    // the mirror's appearance request (MorphMenuUserData) goes to the world editor, which may keep it until it is ready
    let appearance = super::belt::class_name(&panel) == "MorphMenuUserData";
    let accepted = if appearance {
        match super::menu_appearance::route(&panel) {
            None => return,
            Some(a) => a,
        }
    } else {
        begin(&panel, &Handle::null())
    };
    let resolved = super::lobby_spawn::native::<bool>("VRMenuVendorResolve", &[crate::script_call::Value::Handle(&panel), crate::script_call::Value::Bool(accepted)]) == Some(true);
    if !resolved && appearance {
        super::menu_appearance::release();
    }
    if !resolved {
        with(|v| {
            if v.ripper.as_ref().is_some_and(|r| r.panel.instance == panel.instance) {
                v.ripper = None;
            }
        });
    }
}
fn in_menu() -> bool {
    super::boards::defs().ok().and_then(|d| super::boards::in_menu(&d).ok().flatten()).unwrap_or(false)
}
/// V.advance: the native hand-off's progress; true once the world menu may open.
fn advance(now: f64) -> bool {
    let in_menu = in_menu();
    let native = with(|v| v.ripper.as_ref().is_some_and(|r| r.native_handoff));
    if !native {
        return !in_menu;
    }
    hide_presentation();
    let (controller, hub_closed, close_queued, ready_at, started, scenario) = with(|v| {
        let r = v.ripper.as_ref().expect("visit");
        (r.controller.clone(), r.hub_closed, r.close_queued, r.hub_ready_at, r.started, r.scenario.clone())
    });
    if !controller.is_null() && !hub_closed {
        hide_hub(&controller);
    }
    if !controller.is_null() && !close_queued && now - ready_at >= 0.08 {
        with(|v| {
            if let Some(r) = v.ripper.as_mut() {
                r.close_queued = true;
            }
        });
        rt::call(&scenario, "OnVendorClose", &[]);
    }
    if hub_closed && !in_menu {
        with(|v| {
            v.handoff = false;
            if let Some(r) = v.ripper.as_mut() {
                r.native_handoff = false;
                r.scenario = Handle::null();
            }
        });
        restore_presentation_vs();
        false
    } else if now - started > 4.0 {
        let panel = with(|v| {
            v.handoff = false;
            v.ripper.take().map(|r| r.panel).unwrap_or_else(Handle::null)
        });
        restore_presentation_vs();
        if !controller.is_null() {
            let root = o(&controller, "GetRootWidget", &[]);
            if !root.is_null() {
                ink::opacity(&root, 1.0);
                ink::visible(&root, true);
            }
            let data = ink::new("VendorUserData");
            super::menu_main::set_handle_prop(&data, "vendorData", &panel);
            rt::call(&scenario, "OnSwitchToVendor", &[A::H(&data)]);
        } else {
            rt::call(&scenario, "OpenMenu", &[A::Name(cname("vendor_hub_menu")), A::H(&panel), A::N(ev("ScreenDisplayContext", "Vendor"))]);
        }
        cvr_host::log!("[VRMenu] vendor handoff timed out; restored native shop (from Rust)\n");
        false
    } else {
        false
    }
}

// ---------------------------------------------------------------- the menu's side
/// V.current: the active visit while V is within 8 m of the vendor: Some(ripperdoc).
pub(super) fn current_ripperdoc(_m: &Menu) -> Option<bool> {
    let (active, vendor, has_manager, ripper) = with(|v| v.ripper.as_ref().map(|r| (r.active, r.vendor.clone(), !r.manager.is_null(), r.ripperdoc)))?;
    (active && has_manager && near(&vendor)).then_some(ripper)
}
/// The ripperdoc's session for the cyberware page (cyberware_data D.session): active, a ripperdoc, near.
pub(super) fn ripper_session() -> bool {
    with(|v| v.ripper.as_ref().map(|r| (r.active, r.vendor.clone(), !r.manager.is_null(), r.ripperdoc))).is_some_and(|(a, vend, m, r)| a && m && r && near(&vend))
}
pub(super) fn purchasing(_m: &Menu) -> bool {
    with(|v| v.ripper.as_ref().is_some_and(|r| r.purchase.is_some()))
}
/// The visit's vendor and data manager (for the trade and cyberware pages).
pub(super) fn visit_parts() -> Option<(Handle, Handle)> {
    with(|v| v.ripper.as_ref().map(|r| (r.vendor.clone(), r.manager.clone())))
}
pub(super) fn stash_parts() -> Option<(Handle, Handle)> {
    with(|v| v.stash.as_ref().map(|s| (s.object.clone(), s.manager.clone())))
}
/// V.trade: the vendor's own trade menu requested (the hub's tabs set to Trade by the SetupMenuTabs hook).
pub(super) fn trade() -> bool {
    let Some((vendor, panel)) = with(|v| v.ripper.as_ref().map(|r| (r.vendor.clone(), r.panel.clone()))) else { return false };
    if vendor.is_null() {
        return false;
    }
    let panel = if panel.is_null() {
        let p = ink::new("VendorPanelData");
        if let (Some(d), Some(f)) = (new_struct("VendorData"), rt::field(&p, "data")) {
            let dv = val(&d);
            if let Some(e) = dv.f("entityID") {
                let eid = id16(&vendor, "GetEntityID", &[]);
                // SAFETY: an EntityID (8 bytes).
                unsafe { *(e.ptr as *mut u64) = eid.0[0] };
            }
            set(&dv, "isActive", 1.0);
            // SAFETY: the same struct type assigns its value.
            unsafe { f.ty.assign(f.ptr, d.as_ptr().cast()) };
        }
        p
    } else {
        panel
    };
    let vh = hash_of(&vendor);
    with(|v| v.trade_opening = Some((vh, true)));
    rt::call(&super::game_system("GetUISystem"), "RequestVendorMenu", &[A::H(&panel), A::D]);
    true
}
pub(super) fn close_sessions(_m: &mut Menu) {
    let close_board = with(|v| {
        let active = v.ripper.as_ref().is_some_and(|r| r.active);
        let board = active && v.ripper.as_mut().is_some_and(|r| std::mem::replace(&mut r.board_open, false));
        if active {
            v.ripper = None;
        }
        if v.stash.as_ref().is_some_and(|s| s.active) {
            v.stash = None;
        }
        board
    });
    if close_board {
        vendor_board(false);
    }
}
pub(super) fn cancel_all(_m: &mut Menu) {
    cancel_handoff();
    with(|v| {
        v.ripper = None;
        v.stash = None;
    });
}
pub(super) fn restore_presentation(_m: &mut Menu) {
    restore_presentation_vs();
}
fn stash_current() -> bool {
    with(|v| v.stash.as_ref().map(|s| (s.active, s.object.clone()))).is_some_and(|(a, o)| a && near(&o))
}
pub(super) fn stash_current_m(_m: &Menu) -> bool {
    stash_current()
}
/// The visits' part of M.update; true when the frame ends there (the menu closed for a hand-over).
pub(super) fn poll(m: &mut Menu) -> bool {
    with(|v| v.clock = m.time);
    m.shared.menu_controller = menu_controller();
    if m.shared.menu_controller.is_null() {
        static SAID: std::sync::Once = std::sync::Once::new();
        SAID.call_once(|| cvr_host::log!("[VRMenu] no in-game menu controller yet on the menu's thread {:?} (from Rust)\n", std::thread::current().id()));
    }
    poll_native();
    let pl = player();
    let stash = with(|v| v.stash.as_ref().map(|s| (s.active, s.pending, s.transferring, s.close_native)));
    if let Some((active, pending, transferring, close_native)) = stash {
        if active && !stash_current() {
            with(|v| v.stash = None);
            if m.open {
                super::menu_main::open_section(m, Some("inventory"), true);
            }
        } else if pending && !pl.is_null() {
            if !transferring {
                if m.open {
                    super::menu_main::close(m);
                }
                with(|v| {
                    v.ripper = None;
                    if let Some(s) = v.stash.as_mut() {
                        s.transferring = true;
                        s.close_native = false;
                    }
                });
                if close_native {
                    let ctrl = menu_controller();
                    super::menu_data::menu_event(&ctrl, "OnCloseHubMenuRequest");
                }
                return true;
            }
            if !super::menu_main::paused() {
                if !m.open {
                    super::menu_main::open_centred(m, None);
                }
                if m.menu.as_ref().is_some_and(|s| s.built) {
                    with(|v| {
                        if let Some(s) = v.stash.as_mut() {
                            (s.active, s.pending) = (true, false);
                        }
                    });
                    super::menu_main::open_section(m, Some("stash"), true);
                }
            }
        }
    }
    let visit = with(|v| v.ripper.as_ref().map(|r| (r.pending, r.transferring, section_of(r.ripperdoc, r.glamdoc))));
    if let Some((true, transferring, section)) = visit {
        if !pl.is_null() {
            if !transferring {
                if m.open {
                    super::menu_main::close(m);
                }
                with(|v| {
                    if let Some(r) = v.ripper.as_mut() {
                        r.transferring = true;
                    }
                });
                return true;
            }
            if advance(m.time) && !super::menu_main::paused() && !m.open {
                with(|v| {
                    if let Some(r) = v.ripper.as_mut() {
                        r.active = true;
                    }
                });
                // the requested shop built before the surface's first GPU-ready check
                if super::menu_main::open_centred(m, Some(section)) {
                    let board = with(|v| {
                        v.ripper.as_mut().is_some_and(|r| {
                            r.pending = false;
                            r.controller = Handle::null();
                            open_board(r)
                        })
                    });
                    if board {
                        vendor_board(true);
                    }
                } else {
                    with(|v| {
                        if let Some(r) = v.ripper.as_mut() {
                            r.active = false;
                        }
                    });
                }
            }
        }
    }
    false
}

// ---------------------------------------------------------------- the hooks (VRMenuHook from the Lua host)
fn stash_begin(data: &Handle, close_native: bool) -> bool {
    if !interface_enabled() {
        return false;
    }
    let pl = player();
    let object = pf(data, "storageObject").map(|v| v.handle()).unwrap_or_else(Handle::null);
    if pl.is_null() || object.is_null() {
        return false;
    }
    let manager = ink::new("VendorDataManager");
    let eid = id16(&object, "GetEntityID", &[]);
    rt::call_typed(&manager, "Initialize", &["whandle:GameObject", "entEntityID"], &[A::H(&pl), eid.a()]);
    with(|v| v.stash = Some(StashVisit { object, manager, pending: true, close_native, ..Default::default() }));
    true
}
/// One of the game's hooks, as the Lua host reports it: `a` the hooked object, `b` its argument, `name` a scenario's
/// name; the answer for an Override (true: the hook's own result instead of the game's).
pub(crate) fn hook(kind: i32, a: &Handle, b: &Handle, name: u64) -> bool {
    let is = |n: &str| name == cname(n);
    match kind {
        1 => {
            // Idle.ShowMenus(true) reveals the common background before Vendor.OnEnter
            if interface_enabled() && is("MenuScenario_Vendor") {
                hide_presentation();
                with(|v| v.presentation_early = true);
            }
            false
        }
        2 => {
            if with(|v| v.presentation_early && !v.handoff) {
                restore_presentation_vs();
            }
            false
        }
        3 => {
            if with(|v| v.handoff) && !is("MenuScenario_Idle") {
                cancel_handoff();
            }
            with(|v| v.trade_opening = None);
            false
        }
        4 => begin(b, &Handle::null()),
        5 => {
            let panel = if pf(b, "data").is_some() { b.clone() } else { pf(b, "vendorData").map(|v| v.handle()).unwrap_or_else(Handle::null) };
            begin(&panel, a);
            false
        }
        6 => with(|v| v.handoff),
        7 => {
            if with(|v| v.trade_opening.as_ref().is_some_and(|t| t.1) || v.handoff) {
                return true;
            }
            let panel = pf(b, "vendorData").map(|v| v.handle()).unwrap_or_else(Handle::null);
            !b.is_null() && begin(&panel, a)
        }
        8 => {
            if with(|v| v.handoff) {
                hide_hub(a);
            }
            false
        }
        9 => {
            capture_hub(a, b);
            false
        }
        10 => {
            with(|v| {
                v.live_hub = None;
                let handoff = v.handoff;
                if let Some(r) = v.ripper.as_mut().filter(|r| handoff && r.close_queued) {
                    r.hub_closed = true;
                }
            });
            false
        }
        11 => {
            let trade = with(|v| v.trade_opening.clone());
            if let Some((hash, _)) = trade {
                let mgr = pf(a, "VendorDataManager").map(|v| v.handle()).unwrap_or_else(Handle::null);
                let inst = o(&mgr, "GetVendorInstance", &[]);
                if !inst.is_null() && hash_of(&inst) == hash {
                    let tabs = pf(a, "tabRoot").map(|v| v.handle()).unwrap_or_else(Handle::null);
                    rt::call(&tabs, "Toggle", &[A::N(0.0)]);
                    rt::call(a, "OnValueChanged", &[A::H(&tabs), A::N(0.0)]);
                }
            }
            false
        }
        12 => {
            with(|v| v.trade_opening = None);
            false
        }
        13 => {
            set_menu_controller(a, 13);
            stash_begin(b, false)
        }
        14 => {
            set_menu_controller(a, 14);
            false
        }
        // MenuScenario_Vendor.OnSwitchToGlamdoc: the Glamdoc's own switch (main's vendor_visit.lua)
        15 => {
            if with(|v| v.handoff) {
                return true;
            }
            let panel = pf(b, "vendorData").map(|v| v.handle()).unwrap_or_else(Handle::null);
            !b.is_null() && begin(&panel, a)
        }
        _ => false,
    }
}
