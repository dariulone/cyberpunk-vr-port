//! mods/redscript/CyberpunkVRPort_Cyberware, the launcher's and the monowire's half (the swings are melee.rs's):
//!
//! - The projectile launcher kept open as a weapon: chosen on the weapon wheel's cycle (CycleWeapon steps through the
//!   wheel's slots and the installed launcher in the last one, when the player may cycle to it), the hand decision for
//!   it (GetItemIDfromEquipmentManipulationAction -- the left hand's cyberware, the selection's flags), kept open after
//!   its shot (the state VrLauncherReady, its classes and decisions: launcher_ready.rs, whose vrLauncher* flags of the
//!   player are written here), unequipped when cycled off;
//!   aimed by the controller (AimSnap's aim start, DetachProjectile from the plugin's muzzle points along their line, a
//!   rejected shot draining no charge).
//! - The monowire: its idle effect while out, its points' rays against the world (three presets in turn, the planes to
//!   the plugin: VRCyberwareWirePlanes).
//! - The iconic cyberware chord (not with the scanner up in a vehicle behind a mask).
//!
//! The left hand's decision (left_handed.rs) runs inside the hand decision's hook, right after the original, as the
//! LeftHanded wrapper sat inside this module's.
use super::lobby_rtti::{self as rt, Val, A};
use super::melee::{add, dist, dot, f3, mul, n, norm, same_item, sub, v4, V3};
use super::menu_rt as r;
use crate::script_call::{Owned, Value};
use crate::script_hooks::{Call, HookFn, Kind};
use red4ext_abi::handle::{Handle, WeakHandle};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

type Item = [u64; 2];
const NONE: Item = [0, 0];

fn owner(si: &Handle) -> Handle {
    rt::field(si, "executionOwner").map(|v| v.handle()).unwrap_or_else(Handle::null)
}
fn player_of(o: &Handle) -> Handle {
    if r::is_a(o, "PlayerPuppet") {
        o.clone()
    } else {
        Handle::null()
    }
}
fn flag(pl: &Handle, f: &str) -> bool {
    super::launcher_ready::flag(pl, f)
}
fn set_flag(pl: &Handle, f: &str, v: bool) {
    super::launcher_ready::set_flag(pl, f, v);
}
fn item_of(o: &Owned) -> Item {
    // SAFETY: an ItemID result.
    unsafe { *(o.as_ptr() as *const Item) }
}
fn valid(i: &Item) -> bool {
    i[0] != 0
}
fn ip(i: &Item) -> A<'_> {
    A::P(i.as_ptr().cast())
}
fn is_type(i: &Item, t: &str) -> bool {
    rt::static_call("WeaponObject", "IsOfType", &[ip(i), A::N(rt::enum_value("gamedataItemType", t))]).is_some_and(|o| Val::of(&o).bool())
}
fn ev(en: &str, m: &str) -> f64 {
    rt::enum_value(en, m)
}
fn native<T: crate::script_call::Ret>(name: &str, values: &[Value]) -> Option<T> {
    super::belt::native::<T>(name, values)
}

// ---------------------------------------------------------------- the launcher's equip events
fn launcher_equipped(si: &Handle) -> bool {
    rt::static_call("LeftHandCyberwareHelper", "IsProjectileLauncherEquipped", &[A::H(si)]).is_some_and(|o| Val::of(&o).bool())
}
fn equip_enter(call: &mut Call) {
    let si = call.arg_handle(1);
    let pl = player_of(&owner(&si));
    set_flag(&pl, "vrLauncherShotRejected", false);
    set_flag(&pl, "vrLauncherShotQueued", false);
    set_flag(&pl, "vrLauncherKeepOpen", launcher_equipped(&si));
}
fn equip_exit(call: &mut Call) {
    let si = call.arg_handle(1);
    set_flag(&player_of(&owner(&si)), "vrLauncherKeepOpen", launcher_equipped(&si));
}
fn cleanup(call: &mut Call) {
    let pl = player_of(&owner(&call.arg_handle(1)));
    for f in ["vrLauncherKeepOpen", "vrLauncherSelectedAsWeapon", "vrLauncherFromWeaponCycle"] {
        set_flag(&pl, f, false);
    }
}
/// EquipmentSystemPlayerData's vrCyberwareHandlingRequest: a weapon request is being handled.
static HANDLING: AtomicBool = AtomicBool::new(false);
fn manipulation_request(call: &mut Call) {
    let data = call.this();
    let pl = player_of(&rt::field(&data, "m_owner").map(|v| v.handle()).unwrap_or_else(Handle::null));
    let req = call.arg_handle(0);
    if !pl.is_null() && rt::field(&req, "requestType").and_then(|v| v.num()) == Some(ev("EquipmentManipulationAction", "UnequipAll")) {
        set_flag(&pl, "vrLauncherKeepOpen", false);
        set_flag(&pl, "vrLauncherSelectedAsWeapon", false);
    }
    let was = HANDLING.swap(true, Ordering::Relaxed);
    call.original();
    HANDLING.store(was, Ordering::Relaxed);
}

// ---------------------------------------------------------------- the launcher on the weapon wheel
/// VrInstalledLauncher: the arms' cyberware when it is the launcher.
fn installed_launcher(data: &Handle) -> Item {
    let item = rt::call(data, "GetActiveItem", &[A::N(ev("gamedataEquipmentArea", "ArmsCW"))]).map_or(NONE, |o| item_of(&o));
    if valid(&item) && is_type(&item, "Cyb_Launcher") {
        item
    } else {
        NONE
    }
}
/// VrCanCycleLauncher: no status effect forbidding weapons, arms or combat, the RB hotkey not restricted.
fn can_cycle_launcher(data: &Handle) -> bool {
    let owner_obj = rt::field(data, "m_owner").map(|v| v.handle()).unwrap_or_else(Handle::null);
    for tag in ["NoWeapons", "NoArmsCW", "NoCombat", "Melee", "Fists", "OneHandedFirearms", "DriverCombatFirearms", "DriverCombatBikeWeapons"] {
        if rt::static_call("StatusEffectSystem", "ObjectHasStatusEffectWithTag", &[A::H(&owner_obj), n(tag)]).is_some_and(|o| Val::of(&o).bool()) {
            return false;
        }
    }
    !r::static_gi("PlayerGameplayRestrictions", "IsHotkeyRestricted", &[A::N(ev("EHotkey", "RB"))]).is_some_and(|o| Val::of(&o).bool())
}
fn slot_item(data: &Handle, left: bool) -> Item {
    rt::call(data, "GetSlotActiveItem", &[A::N(ev("EquipmentManipulationRequestSlot", if left { "Left" } else { "Right" }))]).map_or(NONE, |o| item_of(&o))
}
/// EquipmentSystemPlayerData.CycleWeapon: the wheel's slots stepped through with the launcher in the last one.
fn cycle_weapon(call: &mut Call) {
    let data = call.this();
    let (next, only_check) = (call.arg_bool(0), call.arg_bool(1));
    let launcher = installed_launcher(&data);
    let pl = rt::field(&data, "m_owner").map(|v| v.handle()).unwrap_or_else(Handle::null);
    if !valid(&launcher) || !r::is_a(&pl, "PlayerPuppet") || !r::b(&pl, "ShouldAllowCycleToFistCyberware", &[]) {
        return;
    }
    let area = rt::num(&data, "GetEquipAreaIndex", &[A::N(ev("gamedataEquipmentArea", "WeaponWheel"))]).unwrap_or(-1.0) as i64;
    let Some(areas) = rt::field(&data, "m_equipment").and_then(|e| e.f("equipAreas")) else { return };
    let Some(a) = (area >= 0).then(|| areas.at(area as usize)).flatten() else { return };
    let Some(slots_v) = a.f("equipSlots") else { return };
    let slots = slots_v.len() as i64;
    if !(2..=16).contains(&slots) {
        return;
    }
    let mut active = rt::call(&data, "GetLastUsedWeaponItemID", &[]).map_or(NONE, |o| item_of(&o));
    let mut current = a.f("activeIndex").and_then(|v| v.num()).unwrap_or(0.0) as i64;
    if same_item(&slot_item(&data, true), &launcher) {
        active = launcher;
        current = slots - 1;
    }
    let dir = if next { 1 } else { -1 };
    for _ in 0..slots {
        current = (current + dir + slots) % slots;
        let candidate =
            if current == slots - 1 { launcher } else { slots_v.at(current as usize).and_then(|s| s.f("itemID")).map_or(NONE, |v| unsafe { *(v.ptr as *const Item) }) };
        if !valid(&candidate) || same_item(&candidate, &active) {
            continue;
        }
        let allowed = if same_item(&candidate, &launcher) { can_cycle_launcher(&data) } else { r::b(&data, "CheckWeaponAgainstGameplayRestrictions", &[ip(&candidate), A::B(true)]) };
        if allowed {
            if !only_check {
                if let Some(idx) = a.f("activeIndex") {
                    // SAFETY: the area's Int32 active index in place.
                    unsafe { *(idx.ptr as *mut i32) = current as i32 };
                }
            }
            call.set_result_from(candidate.as_ptr().cast());
            return;
        }
    }
    call.set_result_from(NONE.as_ptr().cast());
}
/// The SSlotActiveItems result's two hands.
fn result_hands(call: &Call) -> Option<(Item, Item)> {
    let (p, ty) = call.result()?;
    let v = Val { ptr: p, ty };
    Some((v.f("rightHandItem").map_or(NONE, |x| unsafe { *(x.ptr as *const Item) }), v.f("leftHandItem").map_or(NONE, |x| unsafe { *(x.ptr as *const Item) })))
}
/// GetItemIDfromEquipmentManipulationAction: the weapon wheel's cycle through the launcher, the left hand's decision
/// right after the original, the launcher's selection flags while a request is handled.
fn decision(call: &mut Call) {
    let data = call.this();
    let action = call.arg(0).and_then(|(p, ty)| Val { ptr: p, ty }.num()).unwrap_or(-1.0);
    let pl = player_of(&rt::field(&data, "m_owner").map(|v| v.handle()).unwrap_or_else(Handle::null));
    let launcher = installed_launcher(&data);
    let request_cw = ev("EquipmentManipulationAction", "RequestLeftHandCyberware");
    let item;
    if action == ev("EquipmentManipulationAction", "CycleWeaponWheelItem") && valid(&launcher) {
        let cycled = rt::call(&data, "CycleWeapon", &[A::B(true), A::B(false)]).map_or(NONE, |o| item_of(&o));
        if same_item(&cycled, &launcher) {
            if let Some((p, ty)) = call.arg(0) {
                // SAFETY: the action enum argument, by its size.
                unsafe {
                    match ty.size() {
                        1 => *(p as *mut i8) = request_cw as i8,
                        8 => *(p as *mut i64) = request_cw as i64,
                        _ => *(p as *mut i32) = request_cw as i32,
                    }
                }
            }
            call.original();
            super::left_handed::decide(call);
        } else if !valid(&cycled) {
            if let Some((p, _)) = call.arg(1) {
                call.set_result_from(p.cast_const());
            }
            return;
        } else {
            // a fresh state with only the right hand's item
            let Some((p, ty)) = call.result() else { return };
            let Some(mut fresh) = Owned::of_type(ty) else { return };
            if let Some(f) = (Val { ptr: fresh.as_mut_ptr().cast(), ty }).f("rightHandItem") {
                // SAFETY: the fresh struct's ItemID field.
                unsafe { *(f.ptr as *mut Item) = cycled };
            }
            let _ = p;
            call.set_result_from(fresh.as_ptr().cast());
        }
        item = cycled;
    } else {
        call.original();
        super::left_handed::decide(call);
        let (right, left) = result_hands(call).unwrap_or((NONE, NONE));
        item = if action == request_cw || !valid(&right) { left } else { right };
    }
    if HANDLING.load(Ordering::Relaxed) && !pl.is_null() && valid(&item) && !r::b(&data, "IsEquipmentManipulationAnUnequipRequest", &[A::N(action)]) {
        if is_type(&item, "Cyb_Launcher") {
            // B's draw (arm_b.rs) asks the left hand's cyberware as a weapon of its own.
            let selected = action != request_cw || flag(&pl, "vrArmBLauncherPending");
            set_flag(&pl, "vrArmBLauncherPending", false);
            set_flag(&pl, "vrLauncherSelectedAsWeapon", selected);
            if selected {
                set_flag(&pl, "vrLauncherFromWeaponCycle", true);
            }
        } else if !r::b(&data, "CheckCyberwareItemForActivatedAction", &[ip(&item)]) {
            set_flag(&pl, "vrLauncherKeepOpen", false);
            set_flag(&pl, "vrLauncherSelectedAsWeapon", false);
        }
    }
}
fn equip_to_unequip(call: &mut Call) {
    let pl = player_of(&owner(&call.arg_handle(1)));
    if flag(&pl, "vrLauncherFromWeaponCycle") && !flag(&pl, "vrLauncherSelectedAsWeapon") {
        call.set_result_bool(true);
    }
}
fn quick_action_enter(call: &mut Call) {
    let (sc, si) = (call.arg_handle(0), call.arg_handle(1));
    if flag(&player_of(&owner(&si)), "vrLauncherFromWeaponCycle") && r::b(&sc, "IsStateActive", &[n("LeftHandCyberware"), n("leftHandCyberwareEquip")]) {
        call.set_result_bool(false);
    }
}

// ---------------------------------------------------------------- the launcher aimed by the controller
fn controller_aim() -> bool {
    native::<bool>("VRLauncherControllerAim", &[]) == Some(true)
}
fn aim_snap(call: &mut Call) {
    let si = call.arg_handle(0);
    let pl = player_of(&owner(&si));
    if !pl.is_null() && flag(&pl, "vrLauncherKeepOpen") && controller_aim() {
        let t = rt::obj(&si, "GetTargetingSystem", &[]);
        if !t.is_null() {
            rt::call(&t, "OnAimStartBegin", &[A::H(&pl)]);
            rt::call(&t, "OnAimStartEnd", &[A::H(&pl)]);
        }
        call.skip();
    }
}
fn drain(call: &mut Call) {
    let pl = player_of(&owner(&call.arg_handle(0)));
    if !pl.is_null() && flag(&pl, "vrLauncherShotRejected") && !flag(&pl, "vrLauncherShotQueued") {
        call.set_arg_float(1, 0.0);
    }
}
fn point(side: i32, index: i32) -> [f32; 4] {
    native::<[f32; 4]>("VRCyberwarePoint", &[Value::Int32(side), Value::Int32(index)]).unwrap_or([0.0; 4])
}
/// DetachProjectile: the launcher's shot from the plugin's muzzle along its points (the angle offset turned about its
/// right), the installed projectile's template.
fn detach(call: &mut Call) {
    let this = call.this();
    let si = call.arg_handle(0);
    let angle = call.arg_number(1).unwrap_or(0.0);
    let pl = player_of(&owner(&si));
    let weapon = rt::obj(&this, "GetLeftHandWeaponObject", &[A::H(&si)]);
    if pl.is_null() || !flag(&pl, "vrLauncherKeepOpen") || weapon.is_null() || !is_type(&super::melee::item_id(&weapon), "Cyb_Launcher") || !controller_aim() {
        return;
    }
    call.skip();
    let (m, e) = (point(0, 0), point(0, 1));
    if m[3] == 0.0 || e[3] == 0.0 || dist(f3(m), f3(e)) < 0.01 {
        set_flag(&pl, "vrLauncherShotRejected", true);
        return;
    }
    let mut dir = norm(sub(f3(e), f3(m)));
    let build = |d: V3| {
        let v = v4(d, 0.0);
        rt::static_call("Quaternion", "BuildFromDirectionVector", &[A::P(v.as_ptr().cast())])
    };
    let Some(mut rot) = build(dir) else { return };
    if angle != 0.0 {
        let right = rt::static_call("Quaternion", "GetRight", &[A::P(rot.as_ptr().cast())]).map_or([0.0; 3], |o| f3(unsafe { *(o.as_ptr() as *const [f32; 4]) }));
        dir = add(mul(dir, angle.cos()), mul(right, angle.sin()));
        let Some(r2) = build(dir) else { return };
        rot = r2;
    }
    let installed = rt::call_outs(&this, "GetCurrentlyInstalledProjectile", &[A::H(&si)], 1)
        .and_then(|(_, outs)| outs.first().map(item_of))
        .unwrap_or(NONE);
    let template_id = red4ext_abi::tweakdbid::derive(installed[0], ".projectileTemplateName");
    let template = rt::static_call("TweakDBInterface", "GetCName", &[A::Name(template_id), n("None")]).map_or(0, |o| unsafe { *(o.as_ptr() as *const u64) });
    if template == 0 || template == red4ext_abi::cname::CName::hashed("None").0 {
        set_flag(&pl, "vrLauncherShotRejected", true);
        return;
    }
    let launch = super::lobby_ink::new("gameprojectileSpawnerLaunchEvent");
    super::menu_main::set_handle_prop(&launch, "owner", &pl);
    for (f, v) in [("templateName", template), ("appearance", red4ext_abi::cname::CName::hashed("None").0)] {
        if let Some(x) = rt::field(&launch, f) {
            // SAFETY: a CName field.
            unsafe { *(x.ptr as *mut u64) = v };
        }
    }
    let Some(wp) = r::new_struct("WorldPosition") else { return };
    // SAFETY: the WorldPosition's three FixedPoint words (17 fractional bits).
    unsafe {
        let bits = r::ptr(&wp) as *mut i32;
        for k in 0..3 {
            *bits.add(k) = (f64::from(m[k]) * 131072.0) as i32;
        }
    }
    if let Some(lp) = rt::field(&launch, "launchParams") {
        r::set_h(&lp, "logicalPositionProvider", &r::so("IPositionProvider", "CreateStaticPositionProvider", &[A::P(r::ptr(&wp))]));
        r::set_h(&lp, "logicalOrientationProvider", &r::so("IOrientationProvider", "CreateStaticOrientationProvider", &[A::P(rot.as_ptr().cast())]));
    }
    if let Some(pp) = rt::field(&launch, "projectileParams") {
        r::set(&pp, "ignoreMountedVehicleCollision", 1.0);
    }
    rt::call(&weapon, "QueueEvent", &[A::H(&launch)]);
    set_flag(&pl, "vrLauncherShotQueued", true);
    set_flag(&pl, "vrLauncherShotRejected", false);
    let telemetry = super::game_system("GetTelemetrySystem");
    let wid = super::melee::item_id(&weapon);
    rt::call(&telemetry, "LogActiveCyberwareUsed", &[A::H(&pl), ip(&wid)]);
}

// ---------------------------------------------------------------- the monowire
#[derive(Default)]
struct Wire {
    owner: usize,
    preset: usize,
    planes: [Vec<[f32; 4]>; 2],
    effect_weapon: Option<WeakHandle>,
    effect_on: bool,
    pub ray_hits: i32,
    pub ray_tests: i32,
}
// SAFETY: the weak handle is used on the game's main thread only.
unsafe impl Send for Wire {}
static WIRE: Mutex<Option<Wire>> = Mutex::new(None);
fn wire<T>(pl: &Handle, f: impl FnOnce(&mut Wire) -> T) -> T {
    let mut g = WIRE.lock().unwrap_or_else(|p| p.into_inner());
    let owner = pl.instance as usize;
    if g.as_ref().is_none_or(|w| w.owner != owner) {
        *g = Some(Wire { owner, ..Wire::default() });
    }
    f(g.as_mut().expect("the wire"))
}
#[repr(C, align(16))]
struct TraceResult([f32; 24]);
/// A ray with its hit's position and normal (SyncRaycastByQueryPreset, static only).
fn ray(sq: &Handle, a: V3, b: V3, preset: &str) -> Option<(V3, V3)> {
    let (a, b) = (v4(a, 1.0), v4(b, 1.0));
    let mut trace = TraceResult([0.0; 24]);
    let values = [
        Value::Ptr(a.as_ptr().cast()),
        Value::Ptr(b.as_ptr().cast()),
        Value::U64(red4ext_abi::cname::CName::hashed(preset).0),
        Value::Out((&mut trace as *mut TraceResult).cast()),
        Value::Bool(true),
        Value::Bool(false),
    ];
    // SAFETY: the spatial queries system; the trace result's storage outlives the call.
    let hit = unsafe { crate::script_call::method::<bool>(sq.instance, "SyncRaycastByQueryPreset", &values) }?;
    let t = &trace.0;
    hit.then(|| ([f64::from(t[0]), f64::from(t[1]), f64::from(t[2])], [f64::from(t[3]), f64::from(t[4]), f64::from(t[5])]))
}
/// VrWireTrace: the three presets in turn, from the one that hit last.
fn wire_trace(pl: &Handle, sq: &Handle, a: V3, b: V3) -> Option<(V3, V3)> {
    let start = wire(pl, |w| w.preset);
    for attempt in 0..3 {
        let i = (start + attempt) % 3;
        wire(pl, |w| w.ray_tests += 1);
        if let Some(h) = ray(sq, a, b, ["Bullet", "Camera", "Sight Blocker"][i]) {
            wire(pl, |w| w.preset = i);
            return Some(h);
        }
    }
    None
}
/// PlayerPuppet.VRCyberwareCollideWire: each of the wire's 46 points swept from the last frame (a little ahead, kept
/// 2 cm off the plane it touched), else a short drop through it; the planes to the plugin.
pub(super) fn collide_wire(pl: &Handle, side: i32, epoch: i32, points: &[V3], previous: &[V3], dt: f64) {
    let sq = super::game_system("GetSpatialQueriesSystem");
    if points.len() != 46 || sq.is_null() {
        return;
    }
    let old = wire(pl, |w| w.planes[usize::from(side != 0)].clone());
    let mut planes: Vec<[f32; 4]> = Vec::with_capacity(92);
    for i in 0..points.len() {
        let mut hit = None;
        if i > 0 {
            let (mut start, mut end) = (points[0], points[i]);
            if previous.len() == points.len() {
                start = previous[i];
                let delta = sub(points[i], previous[i]);
                end = add(points[i], mul(delta, (0.04 / dt.max(0.001)).min(2.0)));
                if old.len() == 92 && old[i * 2][3] > 0.5 {
                    let (contact, normal) = (f3(old[i * 2]), f3(old[i * 2 + 1]));
                    let separation = dot(sub(start, contact), normal);
                    if separation < 0.02 {
                        start = add(start, mul(normal, 0.02 - separation));
                    }
                }
            }
            if dist(start, end) > 0.002 {
                hit = wire_trace(pl, &sq, start, end);
            }
            if hit.is_none() {
                hit = wire_trace(pl, &sq, add(points[i], [0.0, 0.0, 0.035]), sub(points[i], [0.0, 0.0, 0.07]));
            }
        }
        match hit {
            Some((p, nrm)) => {
                wire(pl, |w| w.ray_hits += 1);
                planes.push(v4(p, 1.0));
                planes.push(v4(nrm, 0.0));
            }
            None => {
                planes.push([0.0; 4]);
                planes.push([0.0; 4]);
            }
        }
    }
    wire(pl, |w| w.planes[usize::from(side != 0)] = planes.clone());
    // the native takes an array<Vector4>: made by its own parameter type
    let Some(f) = red4ext_abi::rtti::RttiSystem::get().and_then(|r| r.get_function(red4ext_abi::cname::CName::hashed("VRCyberwareWirePlanes"))) else { return };
    let Some(pty) = f.base.params.as_slice().get(2).and_then(|p| unsafe { p.as_ref() }).and_then(|p| unsafe { p.ty.as_ref() }) else { return };
    let Some(mut arr) = Owned::of_type(pty) else { return };
    // SAFETY: the array value resized and filled by its own type.
    unsafe {
        let at = arr.as_mut_ptr().cast();
        if !pty.array_resize(at, planes.len() as u32) {
            return;
        }
        for (k, v) in planes.iter().enumerate() {
            *(pty.array_element(at, k as u32) as *mut [f32; 4]) = *v;
        }
    }
    native::<()>("VRCyberwareWirePlanes", &[Value::Int32(side), Value::Int32(epoch), Value::Ptr(arr.as_ptr().cast())]);
}
/// PlayerPuppet.VRCyberwareWireEffect: the monowire's idle effect on while it is out.
pub(super) fn wire_effect(pl: &Handle, weapon: &Handle, active: bool) {
    let (changed, old) = wire(pl, |w| {
        let same = w.effect_weapon.as_ref().is_some_and(|x| x.lock().instance == weapon.instance);
        if same {
            (false, Handle::null())
        } else {
            let old = if w.effect_on { w.effect_weapon.as_ref().map(WeakHandle::lock).unwrap_or_else(Handle::null) } else { Handle::null() };
            w.effect_weapon = (!weapon.is_null()).then(|| WeakHandle::from_handle(weapon));
            w.effect_on = false;
            (true, old)
        }
    });
    let effect = |o: &Handle, start: bool| {
        rt::static_call("GameObjectEffectHelper", if start { "StartEffectEvent" } else { "StopEffectEvent" }, &[A::H(o), n("monowire_idle")]);
    };
    if changed && !old.is_null() {
        effect(&old, false);
    }
    if weapon.is_null() || super::melee::kind(weapon) != 3 {
        return;
    }
    if wire(pl, |w| w.effect_on) != active {
        effect(weapon, active);
        wire(pl, |w| w.effect_on = active);
    }
}

// ---------------------------------------------------------------- the iconic cyberware
/// PlayerPuppet.VRActivateIconicCyberware: not with a mask on in a vehicle while the scanner shows.
pub(super) fn activate_iconic(pl: &Handle) {
    let vehicle = rt::field(pl, "m_vehicleState").and_then(|v| v.num()).unwrap_or(0.0);
    if r::b(pl, "HasCWMask", &[]) && vehicle != ev("gamePSMVehicle", "Default") {
        let shown = (|| {
            let defs = super::boards::defs().ok()?;
            let def = super::boards::definition(&defs, "UI_Scanner").ok()?.clone();
            let board = super::boards::system_board(&def).ok()?;
            let id = super::boards::field(&def, "UIVisible").ok()?;
            Some(r::b(&board, "GetBool", &[A::P(id)]))
        })()
        .unwrap_or(false);
        if shown {
            return;
        }
    }
    rt::call(pl, "ActivateIconicCyberware", &[]);
}

pub(super) const HOOKS: [(&str, &str, Kind, HookFn); 11] = [
    ("LeftHandCyberwareEquipEvents", "OnEnter", Kind::After, equip_enter),
    ("LeftHandCyberwareEquipEvents", "OnExit", Kind::After, equip_exit),
    ("LeftHandCyberwareEventsTransition", "CleanUpLeftHandCyberwareState", Kind::Before, cleanup),
    ("EquipmentSystemPlayerData", "OnEquipmentSystemWeaponManipulationRequest", Kind::Override, manipulation_request),
    ("EquipmentSystemPlayerData", "CycleWeapon", Kind::Override, cycle_weapon),
    ("EquipmentSystemPlayerData", "GetItemIDfromEquipmentManipulationAction", Kind::Override, decision),
    ("LeftHandCyberwareEquipDecisions", "ToLeftHandCyberwareUnequip", Kind::Override, equip_to_unequip),
    ("LeftHandCyberwareQuickActionDecisions", "EnterCondition", Kind::Override, quick_action_enter),
    ("LeftHandCyberwareTransition", "AimSnap", Kind::Override, aim_snap),
    ("LeftHandCyberwareTransition", "DrainLeftHandWeaponCharge", Kind::Before, drain),
    ("LeftHandCyberwareTransition", "DetachProjectile", Kind::Override, detach),
];
