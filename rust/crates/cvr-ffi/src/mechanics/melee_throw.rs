//! mods/redscript/CyberpunkVRPort_Melee/vrport_physical_throw.reds: a throwable melee weapon let go of in a throwing
//! motion is thrown -- from where it is, along the hand's throw (aim-assisted: the enemy the head looks at within 20
//! degrees from the eye, else the one within 15 degrees of the throw or the stroke, in sight, 1..30 m), at the hand's
//! speed (4..45 m/s) on a near-straight line (gravity -1.5), curved onto the assisted target. The melee machine's throw
//! state takes the request (its targeting state left, no aim-assist record for it), spawns the projectile through the
//! game's ProjectileLaunchHelper from the request's origin and direction, the weapon hidden until the throw's reload
//! ends; the projectile claims the request as it is set up and launches by it. The request, the weapon's and the
//! projectile's references to it are maps here (the scripts' fields).
use super::lobby_rtti::{self as rt, Val, A};
use super::melee::{add, dist, dot, len, n, norm, real_time, sub, v4, world_pos, V3};
use super::menu_rt as r;
use crate::script_call::Value;
use crate::script_hooks::{Call, HookFn, Kind};
use red4ext_abi::cname::CName;
use red4ext_abi::handle::{Handle, WeakHandle};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

#[derive(Clone)]
struct Throw {
    id: u64,
    weapon: usize,
    origin: V3,
    direction: V3,
    stroke: V3,
    gaze: V3,
    eye: V3,
    speed: f64,
    gravity: f64,
    stamp: f64,
    launched: bool,
    claimed: bool,
    target: Option<WeakHandle>,
}
#[derive(Default)]
struct Throws {
    owner: usize,
    serial: u64,
    request: Option<Throw>,
    /// The weapon's request (by the weapon object) and the projectile's (by the projectile object).
    by_weapon: HashMap<usize, Throw>,
    by_projectile: HashMap<usize, Throw>,
    hidden: HashSet<usize>,
    shots: i32,
    hits: i32,
}
// SAFETY: the weak handles are used on the game's main thread only.
unsafe impl Send for Throws {}
static THROWS: Mutex<Option<Throws>> = Mutex::new(None);
fn with<T>(f: impl FnOnce(&mut Throws) -> T) -> T {
    let mut g = THROWS.lock().unwrap_or_else(|p| p.into_inner());
    let owner = super::belt::player().instance as usize;
    if g.as_ref().is_none_or(|t| t.owner != owner) {
        *g = Some(Throws { owner, ..Throws::default() });
    }
    f(g.as_mut().expect("the throws"))
}
fn enabled() -> bool {
    super::belt::native::<bool>("VRMeleeThrowEnabled", &[]) == Some(true)
}
fn throwable(weapon: &Handle) -> bool {
    !weapon.is_null() && r::b(weapon, "WeaponHasTag", &[n("Throwable")])
}
fn psm_int(pl: &Handle, field: &str) -> Option<f64> {
    let defs = super::boards::defs().ok()?;
    let def = super::boards::definition(&defs, "PlayerStateMachine").ok()?;
    let board = super::boards::local_board(super::melee::entity_id(pl), def).ok()?;
    let id = super::boards::field(def, field).ok()?;
    rt::num(&board, "GetInt", &[A::P(id)])
}

/// PlayerPuppet.VRMeleeThrowReady: a throwable weapon in the held hand's slot the machine can throw, in its throw-aim
/// state (MeleeWeapon 9).
pub(super) fn ready(pl: &Handle, weapon: &Handle) -> bool {
    if !enabled() || !throwable(weapon) || r::b(pl, "IsDead", &[]) || super::melee::holster_intent(pl) {
        return false;
    }
    let ts = super::belt::transactions();
    let held = rt::obj(&ts, "GetItemInSlot", &[A::H(pl), A::Name(super::left_handed::held_slot(pl))]);
    if held.instance != weapon.instance || !rt::static_call("MeleeTransition", "CanThrowWeaponObject", &[A::H(pl), A::H(weapon)]).is_some_and(|o| Val::of(&o).bool()) {
        return false;
    }
    psm_int(pl, "MeleeWeapon") == Some(9.0)
}
pub(super) fn pending(weapon: &Handle) -> bool {
    let (w, now) = (weapon.instance as usize, real_time());
    with(|t| t.request.as_ref().is_some_and(|q| !q.launched && q.weapon == w && now - q.stamp < 0.30))
}
pub(super) fn cancel() {
    with(|t| {
        if t.request.as_ref().is_some_and(|q| !q.launched) {
            t.request = None;
        }
    });
}
pub(super) fn recovering(weapon: &Handle) -> bool {
    !weapon.is_null() && with(|t| t.hidden.contains(&(weapon.instance as usize)))
}
/// VRMeleeThrowAssist: the enemy the head looks at, else the one nearest the throw, in sight.
fn assist(pl: &Handle, q: &mut Throw) {
    let (mut best, mut best_look) = (0.965926, 0.939693);
    let (mut best_dir, mut look_dir) = ([0.0; 3], [0.0; 3]);
    let (mut best_target, mut look_target): (Option<Handle>, Option<Handle>) = (None, None);
    let queries = super::game_system("GetSpatialQueriesSystem");
    let Some(mut qs) = r::new_struct("TargetSearchQuery") else { return };
    let qv = r::val(&qs);
    r::set(&qv, "testedSet", r::ev("TargetingSet", "Complete"));
    r::set(&qv, "maxDistance", 30.0);
    r::set(&qv, "filterObjectByDistance", 1.0);
    r::set(&qv, "ignoreInstigator", 1.0);
    if let (Some(filter), Some(sf)) = (rt::global_call("TSF_NPC", &[]), qv.f("searchFilter")) {
        // SAFETY: the filter field and the global's result of the same type.
        unsafe { sf.ty.assign(sf.ptr, filter.as_ptr().cast()) };
    }
    let targeting = super::game_system("GetTargetingSystem");
    let Some((_, outs)) = rt::call_outs(&targeting, "GetTargetParts", &[A::H(pl), A::P(qs.as_mut_ptr().cast_const().cast())], 1) else { return };
    let Some(parts) = outs.first() else { return };
    for part in r::items(&Val::of(parts)) {
        let comp = rt::static_call("TS_TargetPartInfo", "GetComponent", &[A::P(part.ptr.cast_const())]).map(|o| Val::of(&o).handle()).unwrap_or_else(Handle::null);
        let target = r::o(&comp, "GetEntity", &[]);
        if comp.is_null() || !r::is_a(&target, "ScriptedPuppet") || !r::b(&target, "IsActive", &[]) || r::b(&target, "IsDead", &[]) || r::b(&target, "IsCrowd", &[]) {
            continue;
        }
        if rt::static_call("GameObject", "IsFriendlyTowardsPlayer", &[A::H(&target)]).is_some_and(|o| Val::of(&o).bool()) {
            continue;
        }
        let Some(point) = rt::static_call("ProjectileTargetingHelper", "GetTargetingComponentsWorldPosition", &[A::H(&comp)]).map(|o| {
            // SAFETY: a Vector4 result.
            super::melee::f3(unsafe { *(o.as_ptr() as *const [f32; 4]) })
        }) else {
            continue;
        };
        let delta = sub(point, q.origin);
        if len(delta) < 1.0 || len(delta) > 30.0 {
            continue;
        }
        let dir = norm(delta);
        let score = dot(q.direction, dir).max(dot(q.stroke, dir));
        let look = dot(q.gaze, norm(sub(point, q.eye)));
        if look > best_look || (look_target.is_none() && score > best) {
            // in sight: nothing blocks the way, or what blocks it is at the target
            let blocked = match super::view::raycast(queries.instance, "SyncRaycastByQueryPreset", q.origin, point, "Sight Blocker", &[Value::Bool(true), Value::Bool(false)]) {
                Ok(Some(hit)) => dist(hit, point) >= 0.35,
                _ => false,
            };
            if !blocked {
                if look > best_look {
                    (best_look, look_dir, look_target) = (look, dir, Some(comp.clone()));
                }
                if score > best {
                    (best, best_dir, best_target) = (score, dir, Some(comp.clone()));
                }
            }
        }
    }
    if let Some(t) = look_target {
        q.direction = look_dir;
        q.target = Some(WeakHandle::from_handle(&t));
    } else if let Some(t) = best_target {
        q.direction = best_dir;
        q.target = Some(WeakHandle::from_handle(&t));
    }
}
/// PlayerPuppet.VRMeleeThrowRelease: the throw asked for (true when accepted).
pub(super) fn release(pl: &Handle, weapon: &Handle, direction: V3, speed: f64, hand_speed: f64, stroke: V3, gaze: V3, eye_offset: V3) -> bool {
    if !ready(pl, weapon) || pending(weapon) || len(direction) < 0.50 {
        return false;
    }
    let origin = world_pos(weapon).unwrap_or([0.0; 3]);
    let dir = norm(direction);
    let mut q = Throw {
        id: 0,
        weapon: weapon.instance as usize,
        origin,
        direction: dir,
        stroke: if len(stroke) > 0.50 { norm(stroke) } else { dir },
        gaze: if len(gaze) > 0.50 { norm(gaze) } else { dir },
        eye: add(origin, eye_offset),
        speed: speed.clamp(4.0, 45.0),
        gravity: -1.50,
        stamp: real_time(),
        launched: false,
        claimed: false,
        target: None,
    };
    if hand_speed >= 1.0 {
        assist(pl, &mut q);
    }
    with(|t| {
        t.serial += 1;
        q.id = t.serial;
        t.request = Some(q);
    });
    true
}

// ---------------------------------------------------------------- the machine
fn owner(si: &Handle) -> Handle {
    rt::field(si, "executionOwner").map(|v| v.handle()).unwrap_or_else(Handle::null)
}
fn machine_weapon(si: &Handle) -> Handle {
    rt::static_obj("MeleeTransition", "GetWeaponObject", &[A::H(si)])
}
fn throw_enter(call: &mut Call) {
    let si = call.arg_handle(1);
    let (pl, weapon) = (owner(&si), machine_weapon(&si));
    if r::is_a(&pl, "PlayerPuppet") && enabled() && throwable(&weapon) {
        let ok = pending(&weapon)
            && r::b(&call.this(), "HasAttackRecord", &[A::H(&si)])
            && rt::static_call("MeleeTransition", "CanThrowWeaponObject", &[A::H(&pl), A::H(&weapon)]).is_some_and(|o| Val::of(&o).bool());
        call.set_result_bool(ok);
    }
}
fn targeting_exit(call: &mut Call) {
    let si = call.arg_handle(1);
    if r::is_a(&owner(&si), "PlayerPuppet") && enabled() && pending(&machine_weapon(&si)) {
        call.set_result_bool(true);
    }
}
fn no_aim_assist(call: &mut Call) {
    let si = call.arg_handle(0);
    if enabled() && r::is_a(&owner(&si), "PlayerPuppet") && throwable(&machine_weapon(&si)) {
        let null = Handle::null();
        call.set_result_from((&null as *const Handle).cast());
    }
}
/// MeleeThrowAttackEvents.OnEnter: a physical throw spawns the projectile at once and hides the weapon.
fn throw_attack_enter(call: &mut Call) {
    let (sc, si) = (call.arg_handle(0), call.arg_handle(1));
    let (pl, weapon) = (owner(&si), machine_weapon(&si));
    let physical = r::is_a(&pl, "PlayerPuppet") && enabled() && pending(&weapon);
    call.original();
    if !physical {
        return;
    }
    let this = call.this();
    r::hset(&this, "m_projectileThrown", 1.0);
    rt::call(&this, "SetIsThrowReloading", &[A::H(&sc), A::B(true)]);
    rt::call(&this, "SendAnimFeatureData", &[A::H(&sc), A::H(&si)]);
    rt::call(&this, "SpawnMeleeWeaponProjectile", &[A::H(&si)]);
    rt::call(&this, "ApplyThrowableCooldown", &[A::H(&sc), A::H(&si)]);
    with(|t| t.hidden.insert(weapon.instance as usize));
    rt::call(&weapon, "SetVisible", &[A::B(false)]);
}
fn throw_reload_exit(call: &mut Call) {
    let w = machine_weapon(&call.arg_handle(1));
    with(|t| t.hidden.remove(&(w.instance as usize)));
}
/// ProjectileLaunchHelper::SpawnProjectile (a scripted static): a pending physical throw launches from its own origin
/// and direction, tracking its assisted target.
fn spawn_projectile(call: &mut Call) {
    let (owner_obj, item) = (call.arg_handle(0), call.arg_handle(3));
    let (template, appearance) = (call.arg_u64(1), call.arg_u64(2));
    if !r::is_a(&owner_obj, "PlayerPuppet") || !r::is_a(&item, "WeaponObject") || !enabled() || !pending(&item) {
        return;
    }
    let Some(q) = with(|t| t.request.clone()) else { return };
    let launch = super::lobby_ink::new("gameprojectileSpawnerLaunchEvent");
    let Some(lp) = rt::field(&launch, "launchParams") else { return };
    let Some(wp) = r::new_struct("WorldPosition") else { return };
    // SAFETY: the WorldPosition's three FixedPoint words (17 fractional bits).
    unsafe {
        let bits = r::ptr(&wp) as *mut i32;
        for (k, v) in q.origin.iter().enumerate() {
            *bits.add(k) = (v * 131072.0) as i32;
        }
    }
    let pos = r::so("IPositionProvider", "CreateStaticPositionProvider", &[A::P(r::ptr(&wp))]);
    let d = v4(q.direction, 0.0);
    let Some(rot) = rt::static_call("Quaternion", "BuildFromDirectionVector", &[A::P(d.as_ptr().cast())]) else { return };
    let ori = r::so("IOrientationProvider", "CreateStaticOrientationProvider", &[A::P(rot.as_ptr().cast())]);
    for (f, h) in [("logicalPositionProvider", &pos), ("visualPositionProvider", &pos), ("logicalOrientationProvider", &ori), ("visualOrientationProvider", &ori)] {
        r::set_h(&lp, f, h);
    }
    for (f, v) in [("templateName", template), ("appearance", appearance)] {
        if let Some(x) = rt::field(&launch, f) {
            // SAFETY: a CName field.
            unsafe { *(x.ptr as *mut u64) = v };
        }
    }
    super::menu_main::set_handle_prop(&launch, "owner", &owner_obj);
    if let Some(pp) = rt::field(&launch, "projectileParams") {
        r::set(&pp, "ignoreMountedVehicleCollision", 1.0);
        if let Some(target) = q.target.as_ref().map(WeakHandle::lock).filter(|h| !h.is_null()) {
            r::set_h(&pp, "trackedTargetComponent", &target);
            if let (Some(tp), Some(o)) = (pp.f("targetPosition"), rt::static_call("ProjectileTargetingHelper", "GetTargetingComponentsWorldPosition", &[A::H(&target)])) {
                // SAFETY: a Vector4 field assigned from a Vector4 result.
                unsafe { tp.ty.assign(tp.ptr, o.as_ptr().cast()) };
            }
        }
    }
    let wid = item.instance as usize;
    with(|t| {
        if let Some(r) = t.request.as_mut() {
            r.launched = true;
            let mut copy = r.clone();
            copy.launched = true;
            t.by_weapon.insert(wid, copy);
        }
    });
    rt::call(&item, "QueueEvent", &[A::H(&launch)]);
    call.set_result_bool(true);
}
/// MeleeProjectile.OnProjectileInitialize: the projectile claims its weapon's launched request (within a second).
fn projectile_init(call: &mut Call) {
    let ev = call.arg_handle(0);
    let weapon = rt::field(&ev, "weapon").map(|v| v.handle()).unwrap_or_else(Handle::null);
    let owner_obj = rt::field(&ev, "owner").map(|v| v.handle()).unwrap_or_else(Handle::null);
    call.original();
    let me = call.this().instance as usize;
    let now = real_time();
    with(|t| {
        t.by_projectile.remove(&me);
        if !owner_obj.is_null() && super::menu_rt::is_a(&owner_obj, "PlayerPuppet") && !weapon.is_null() {
            if let Some(q) = t.by_weapon.remove(&(weapon.instance as usize)) {
                if q.launched && !q.claimed && now - q.stamp < 1.0 {
                    let mut q = q;
                    q.claimed = true;
                    if t.by_projectile.len() > 32 {
                        t.by_projectile.clear();
                    }
                    t.by_projectile.insert(me, q);
                }
            }
        }
    });
}
/// VRPhysicalLaunch: the projectile launched by its request -- curved onto the live target, else a parabola.
fn physical_launch(call: &mut Call) {
    let this = call.this();
    let Some(q) = with(|t| t.by_projectile.get(&(this.instance as usize)).cloned()) else { return };
    let ev = call.arg_handle(0);
    let target = q.target.as_ref().map(WeakHandle::lock).unwrap_or_else(Handle::null);
    let puppet = if target.is_null() { Handle::null() } else { r::o(&target, "GetEntity", &[]) };
    rt::call(&this, "GeneralLaunchSetup", &[A::H(&ev)]);
    r::hset(&this, "m_initialLaunchVelocity", q.speed);
    if r::is_a(&puppet, "ScriptedPuppet") && r::b(&puppet, "IsActive", &[]) && !r::b(&puppet, "IsDead", &[]) {
        rt::call(&this, "CurvedLaunchToTarget", &[A::H(&ev), A::H(&puppet), A::H(&target)]);
    } else {
        let name = super::menu_rt::Arg::new("energyLossFactorAfterCollision");
        let loss = rt::num(&this, "GetProjectileTweakDBFloatParameter", &[name.a()]).unwrap_or(0.0);
        rt::call(&this, "ParabolicLaunch", &[A::H(&ev), A::N(q.gravity), A::N(q.speed), A::N(loss)]);
    }
    if r::is_a(&rt::field(&this, "m_user").map(|v| v.handle()).unwrap_or_else(Handle::null), "PlayerPuppet") {
        with(|t| t.shots += 1);
    }
    call.set_result_bool(true);
}
fn projectile_collision(call: &mut Call) {
    let this = call.this();
    let user = rt::field(&this, "m_user").map(|v| v.handle()).unwrap_or_else(Handle::null);
    if r::is_a(&user, "PlayerPuppet") {
        with(|t| {
            if t.by_projectile.contains_key(&(this.instance as usize)) {
                t.hits += 1;
            }
        });
    }
}
/// (shots launched, hits) -- the self-test's.
pub(super) fn counts() -> (i32, i32) {
    with(|t| (t.shots, t.hits))
}
#[allow(dead_code)]
fn unused(_: Value, _: CName) {}

pub(super) const HOOKS: [(&str, &str, Kind, HookFn); 9] = [
    ("MeleeThrowAttackDecisions", "EnterCondition", Kind::Override, throw_enter),
    ("MeleeTargetingDecisions", "ExitCondition", Kind::Override, targeting_exit),
    ("MeleeThrowAttackEvents", "OnEnter", Kind::Override, throw_attack_enter),
    ("MeleeThrowReloadEvents", "OnExit", Kind::After, throw_reload_exit),
    ("MeleeThrowReloadEvents", "OnForcedExit", Kind::After, throw_reload_exit),
    ("MeleeProjectile", "OnProjectileInitialize", Kind::Override, projectile_init),
    ("MeleeProjectile", "OnShoot", Kind::Override, physical_launch),
    ("MeleeProjectile", "OnShootTarget", Kind::Override, physical_launch),
    ("MeleeProjectile", "OnCollision", Kind::Before, projectile_collision),
];
pub(super) const STATIC_HOOKS: [(&str, &str, Kind, HookFn); 2] = [
    ("MeleeTransition", "GetAimAssistMeleeRecord", Kind::Override, no_aim_assist),
    ("ProjectileLaunchHelper", "SpawnProjectile", Kind::Override, spawn_projectile),
];
