//! NPCs react to a VR barrel aimed at them (main's mods/cet/CyberpunkVRPort_Weapon/aim_threat.lua with aim_dwell.lua, and
//! mods/redscript/CyberpunkVRPort_WeaponThreat: vrport_weapon_threat.reds, vrport_crowd_aim.reds; 84ebb57c0, c29e7eb3d,
//! b106160b2). The game's reactions to being aimed at need ADS and the camera's CurrentVisibleTarget: in VR the camera
//! is not the gun. Each hand's drawn, equipped, ranged gun casts its barrel 30 m; two seconds of continuous tracked aim
//! by the same weapon on the same NPC (cvr_body::aim_dwell) give that NPC a short lease (0.6 s, refreshed while the
//! barrel stays), which its reactions accept as the player aiming: ReactionManagerComponent.IsPlayerAiming, and the
//! crowd reaction's ADS / visible-target guard (OnCrowdReaction, the one changed branch: every fear phase, workspot
//! reaction, vehicle behaviour and police call native). Only transitions send the stimuli (AimingAt, StopedAiming):
//! repeated AimingAt would postpone an aggressive NPC's delayed reaction instead of letting it execute.
use super::lobby_rtti::{self as rt, Val, A};
use crate::script_call::Owned;
use crate::script_hooks::{Call, HookFn, Kind};
use cvr_body::aim_dwell::{Dwell, Hit};
use cvr_host::log;
use red4ext_abi::cname::CName;
use red4ext_abi::handle::{Handle, WeakHandle};
use std::collections::HashMap;
use std::sync::Mutex;

fn n(s: &str) -> A<'static> {
    A::Name(CName::hashed(s).0)
}
fn ev(e: &str, m: &str) -> f64 {
    rt::enum_value(e, m)
}
fn flag(o: &Handle, f: &str, args: &[A]) -> bool {
    super::menu_rt::b(o, f, args)
}
fn num(o: &Handle, f: &str, args: &[A]) -> f64 {
    rt::num(o, f, args).unwrap_or(0.0)
}
fn shared(i: i32) -> f32 {
    super::belt::shared(i)
}
fn entity_hash(e: &Handle) -> Option<u64> {
    rt::call(e, "GetEntityID", &[]).map(|o| {
        // SAFETY: an EntityID result (its hash first).
        unsafe { *(o.as_ptr() as *const u64) }
    })
}
/// A native returning array<Vector4>.
pub(super) fn native_frame(name: &str) -> Option<Vec<[f32; 4]>> {
    let f = red4ext_abi::rtti::RttiSystem::get()?.get_function(CName::hashed(name))?;
    // SAFETY: a registered global function, no context; the array result kept while read.
    let owned = unsafe { crate::script_call::invoke_owned(core::ptr::null_mut(), &f.base, &[]) }?;
    // SAFETY: a DynArray of Vector4.
    let arr = unsafe { &*(owned.as_ptr() as *const red4ext_abi::dynarray::DynArray<[f32; 4]>) };
    Some(arr.as_slice().to_vec())
}
/// VRMeleeInputFrame(): the support hand, then the primary, with w > 0 for fresh tracking.
pub(super) fn input_frame() -> Vec<[f32; 4]> {
    native_frame("VRMeleeInputFrame").unwrap_or_default()
}
/// EngineTime.ToFloat(GameInstance.GetSimTime()).
fn sim_time() -> f64 {
    super::melee::sim_time()
}

// ---------------------------------------------------------------- the NPC's lease (vrport_weapon_threat.reds)
/// NPCPuppet's m_vrWeaponAimActive / m_vrWeaponAimUntil / m_vrWeaponAimToken, by the NPC object.
struct Lease {
    npc: WeakHandle,
    active: bool,
    until: f64,
}
struct Leases(HashMap<usize, Lease>);
// SAFETY: the game thread's alone (the mechanics' frame and the reactions' script hooks).
unsafe impl Send for Leases {}
static LEASES: Mutex<Option<Leases>> = Mutex::new(None);

/// NPCPuppet.VRPortIsWeaponAimedAt: a live lease, the NPC attached and active.
fn is_weapon_aimed_at(npc: &Handle) -> bool {
    if npc.is_null() {
        return false;
    }
    let lease = LEASES.lock().unwrap_or_else(|p| p.into_inner());
    let live = lease.as_ref().and_then(|l| l.0.get(&(npc.instance as usize))).is_some_and(|l| l.active && sim_time() <= l.until);
    live && flag(npc, "IsAttached", &[]) && flag(npc, "IsActive", &[])
}
/// NPCPuppet.VRPortSetWeaponAim: the lease taken (or refreshed) or given up; the stimulus only on a transition.
fn set_weapon_aim(npc: &Handle, active: bool) {
    if npc.is_null() {
        return;
    }
    let player = super::belt::player();
    {
        let mut g = LEASES.lock().unwrap_or_else(|p| p.into_inner());
        let leases = &mut g.get_or_insert_with(|| Leases(HashMap::new())).0;
        let key = npc.instance as usize;
        if active {
            if !flag(npc, "IsAttached", &[]) || !flag(npc, "IsActive", &[]) || player.is_null() {
                return;
            }
            let until = sim_time() + 0.60;
            let lease = leases.entry(key).or_insert_with(|| Lease { npc: WeakHandle::from_handle(npc), active: false, until: 0.0 });
            lease.until = until;
            if lease.active {
                return;
            }
            lease.active = true;
        } else {
            match leases.get_mut(&key) {
                Some(l) if l.active => {
                    l.active = false;
                    l.until = 0.0;
                }
                _ => return,
            }
            leases.remove(&key);
        }
    }
    if player.is_null() {
        return;
    }
    let broadcaster = rt::obj(&player, "GetStimBroadcasterComponent", &[]);
    if broadcaster.is_null() {
        return;
    }
    if active {
        rt::call(&broadcaster, "SendDrirectStimuliToTarget", &[A::H(npc), A::N(ev("gamedataStimType", "AimingAt")), A::H(npc)]);
        if flag(npc, "IsCharacterCivilian", &[]) {
            rt::static_call("NPCPuppet", "TutorialAddIllegalActionFact", &[A::H(npc)]);
        }
    } else {
        let was = |f: &str| rt::field(npc, f).is_some_and(|v| v.bool());
        if !was("m_wasAimedAtLast") && !was("m_wasCWChargedAtLast") && !was("m_wasMeleeChargedAtLast") && !was("m_wasChargingGadgetAtLast") {
            rt::call(&broadcaster, "SendDrirectStimuliToTarget", &[A::H(npc), A::N(ev("gamedataStimType", "StopedAiming")), A::H(npc)]);
        }
    }
}
/// The lease's expiry (the scripts' VRWeaponAimExpiryEvent): a lease not refreshed for 0.6 s of game time ends.
fn expire_leases() {
    let now = sim_time();
    let expired: Vec<WeakHandle> = {
        let mut g = LEASES.lock().unwrap_or_else(|p| p.into_inner());
        let Some(l) = g.as_mut() else { return };
        // a despawned NPC's lease goes with it
        l.0.retain(|_, lease| !lease.npc.lock().is_null());
        l.0.values().filter(|lease| lease.active && now > lease.until).map(|lease| lease.npc.clone()).collect()
    };
    for npc in expired {
        set_weapon_aim(&npc.lock(), false);
    }
}

// ---------------------------------------------------------------- the barrels (aim_threat.lua)
struct State {
    dwell: Dwell<Handle>,
    next_poll: f64,
    next_refresh: f64,
    player_id: Option<u64>,
    traces: u64,
    hits: u64,
    error: Option<String>,
    start: std::time::Instant,
    reported: bool,
}
// SAFETY: the game thread's alone.
unsafe impl Send for State {}
static STATE: Mutex<Option<State>> = Mutex::new(None);

fn clear(s: &mut State) {
    for (_, target) in s.dwell.clear() {
        set_weapon_aim(&target, false);
    }
    s.next_poll = 0.0;
    s.next_refresh = 0.0;
    s.player_id = None;
}
/// The player's state machine board while the gameplay goes on (no menu, no pause, no scene of tier 3 or more, no
/// temporary unequip or forced empty hands); None otherwise.
fn gameplay_state(player: &Handle) -> Option<Handle> {
    if player.is_null() || !flag(player, "IsAttached", &[]) || flag(player, "IsDead", &[]) {
        return None;
    }
    let requests = super::requests_handler().ok().filter(|h| !h.is_null())?;
    if flag(&requests, "IsPreGame", &[]) || flag(&requests, "IsGamePaused", &[]) || shared(81) > 0.5 {
        return None;
    }
    let defs = super::boards::defs().ok()?;
    if super::boards::in_menu(&defs).ok()? != Some(false) {
        return None;
    }
    let psm = rt::obj(player, "GetPlayerStateMachineBlackboard", &[]);
    if psm.is_null() {
        return None;
    }
    if psm_int(&psm, "SceneTier") >= 3 {
        return None;
    }
    let upper = psm_int(&psm, "UpperBody");
    if upper == 4 || upper == 5 {
        return None; // TemporaryUnequip / ForceEmptyHands
    }
    Some(psm)
}
fn psm_int(board: &Handle, field: &str) -> i64 {
    let Ok(defs) = super::boards::defs() else { return 0 };
    let Ok(def) = super::boards::definition(&defs, "PlayerStateMachine") else { return 0 };
    let Ok(id) = super::boards::field(def, field) else { return 0 };
    super::boards::get::<i32>(board, "GetInt", id).map_or(0, i64::from)
}
fn item_of(o: &Owned) -> [u64; 2] {
    // SAFETY: an ItemID result.
    unsafe { *(o.as_ptr() as *const [u64; 2]) }
}
fn combined_hash(item: &[u64; 2]) -> f64 {
    rt::static_num("ItemID", "GetCombinedHash", &[A::P(item.as_ptr().cast())]).unwrap_or(0.0)
}
fn valid_item(item: &[u64; 2]) -> bool {
    rt::static_call("ItemID", "IsValid", &[A::P(item.as_ptr().cast())]).is_some_and(|o| Val::of(&o).bool())
}
/// PlayerPuppet.VRPortHasEquippedAimWeapon: the weapon is one of the two the player sets on equip (a lingering
/// attachment can still have a valid item and a muzzle).
fn has_equipped_aim_weapon(player: &Handle, weapon: &Handle) -> bool {
    ["m_equippedLeftHandWeapon", "m_equippedRightHandWeapon", "equippedLeftHandWeapon", "equippedRightHandWeapon"]
        .iter()
        .filter_map(|f| rt::field(player, f))
        .any(|v| v.handle().instance == weapon.instance)
}
/// A drawn, equipped, ranged gun in this hand's equipment slot.
fn drawn_gun(player: &Handle, psm: &Handle, weapon: &Handle, side: i32, slot_side: i32) -> bool {
    if weapon.is_null()
        || !super::menu_rt::is_a(weapon, "gameweaponObject")
        || !flag(weapon, "IsAttached", &[])
        || !has_equipped_aim_weapon(player, weapon)
        || !flag(weapon, "IsRanged", &[])
    {
        return false;
    }
    let item = rt::call(weapon, "GetItemID", &[]).map_or([0; 2], |o| item_of(&o));
    let slot = |left: bool| {
        rt::static_call("EquipmentSystem", "GetSlotActiveItem", &[A::H(player), A::N(ev("EquipmentManipulationRequestSlot", if left { "Left" } else { "Right" }))])
            .map_or([0; 2], |o| item_of(&o))
    };
    let mut left = side == 0;
    let mut active = slot(left);
    // A lone left primary can still be exposed through WeaponRight: its logical equipment state then.
    if side != slot_side && !valid_item(&active) {
        left = false;
        active = slot(false);
    }
    if psm_int(psm, if left { "EquipmentLeft" } else { "EquipmentRight" }) != 0 {
        return false; // equipmentStates.Equipped
    }
    if !valid_item(&active) || !valid_item(&item) || combined_hash(&active) != combined_hash(&item) {
        return false;
    }
    let kind = rt::static_num("WeaponObject", "GetWeaponType", &[A::P(item.as_ptr().cast())]).unwrap_or(-1.0);
    rt::static_call("UIInventoryItemsManager", "IsItemTypeRangedWeapon", &[A::N(kind)]).is_some_and(|o| Val::of(&o).bool())
        || rt::static_call("WeaponObject", "IsOfType", &[A::P(item.as_ptr().cast()), A::N(ev("gamedataItemType", "Wea_GrenadeLauncher"))])
            .is_some_and(|o| Val::of(&o).bool())
}
/// The barrel's hit: an attached, active NPC within 30 m along the muzzle.
fn hit(s: &mut State, weapon: &Handle) -> Option<Hit<Handle>> {
    #[repr(C)]
    struct WorldTransform([u32; 8]);
    // SAFETY: the weapon, held.
    let f = unsafe { crate::script_call::find_function(weapon.instance, "GetMuzzleSlotWorldTransform", 0) }?;
    let mut xf = WorldTransform([0; 8]);
    // SAFETY: the transform is a plain value written into our storage.
    if !unsafe { crate::script_call::invoke_into(weapon.instance, f, &[], (&mut xf as *mut WorldTransform).cast()) } {
        return None;
    }
    let q = [4, 5, 6, 7].map(|i| f32::from_bits(xf.0[i]));
    if q.iter().any(|v| !v.is_finite()) || (q.iter().map(|v| v * v).sum::<f32>() - 1.0).abs() > 0.01 {
        return None;
    }
    let p = super::weapon::muzzle_words_position(&xf.0);
    if p.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    let f = [2.0 * (x * y - w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z + w * x)];
    let origin = [p[0], p[1], p[2], 1.0];
    let end = [p[0] + f[0] * 30.0, p[1] + f[1] * 30.0, p[2] + f[2] * 30.0, 1.0];
    let filter = rt::static_call("QueryFilter", "ALL", &[])?;
    let sq = super::game_system("GetSpatialQueriesSystem");
    s.traces += 1;
    let (found, outs) = rt::method_outs(
        &sq,
        "SyncRaycastByQueryFilter",
        &[A::P(origin.as_ptr().cast()), A::P(end.as_ptr().cast()), A::P(filter.as_ptr().cast()), A::B(false), A::B(false)],
        1,
    )?;
    if !found.is_some_and(|o| Val::of(&o).bool()) {
        return None;
    }
    let trace = outs.first()?;
    // SAFETY: a TraceResult (0x60 bytes).
    let target = crate::blade_natives::trace_entity(unsafe { &*(trace.as_ptr() as *const [u8; 0x60]) });
    if target.is_null() || !super::menu_rt::is_a(&target, "NPCPuppet") || !flag(&target, "IsAttached", &[]) || !flag(&target, "IsActive", &[]) {
        return None;
    }
    s.hits += 1;
    let id = entity_hash(&target)?;
    Some(Hit::new(id, target, entity_hash(weapon).unwrap_or(0), 0.0, 0))
}
/// One hand's observation: this equipped barrel owns the hand even when the ray misses (true).
#[allow(clippy::too_many_arguments)]
fn observe(s: &mut State, obs: &mut [Option<Hit<Handle>>; 2], player: &Handle, psm: &Handle, side: i32, slot_side: i32, weapon: &Handle, hand_w: f32) -> bool {
    if !(hand_w > 0.0 && shared(side * 8) > 0.5) {
        return false;
    }
    if !drawn_gun(player, psm, weapon, side, slot_side) {
        return false;
    }
    if let Some(mut h) = hit(s, weapon) {
        h.origin = hand_w;
        h.slot_side = slot_side;
        obs[side as usize] = Some(h);
    }
    true
}

fn update(s: &mut State, player: &Handle) {
    let now = s.start.elapsed().as_secs_f64();
    // Polled for acquisition at 10 Hz; once a barrel has hit, every update, so a one-frame holster, obstruction or
    // tracking loss breaks the dwell.
    if !s.dwell.has_candidates() && s.dwell.last().is_some_and(|l| now >= l) && now < s.next_poll {
        return;
    }
    s.next_poll = now + 0.1;
    let Some(psm) = gameplay_state(player) else {
        clear(s);
        return;
    };
    let id = entity_hash(player);
    if s.player_id.is_some() && s.player_id != id {
        clear(s);
    }
    s.player_id = id;
    // The native frame rejects stale tracking and the head-aim fallback; its positions the support hand first, the
    // weapon hand second, each's w the tracking origin.
    let primary_left = super::belt::native::<bool>("VRWeaponInLeftHand", &[]) == Some(true);
    let frame: Vec<[f32; 4]> = native_frame("VRMeleeInputFrame").unwrap_or_default();
    let mut obs: [Option<Hit<Handle>>; 2] = [None, None];
    let stamp = frame.first().map(|v| f64::from(v[0])).filter(|v| v.is_finite());
    if frame.len() >= 3 && stamp.is_some() && primary_left == (super::belt::native::<bool>("VRWeaponInLeftHand", &[]) == Some(true)) {
        let ts = super::game_system("GetTransactionSystem");
        let in_slot = |slot: &str| rt::obj(&ts, "GetItemInSlot", &[A::H(player), A::Name(red4ext_abi::tweakdbid::from_name(slot))]);
        let (left, right) = (in_slot("AttachmentSlots.WeaponLeft"), in_slot("AttachmentSlots.WeaponRight"));
        if primary_left {
            // a single physical left primary: its tracking is frame[2] and shared [0] whichever attachment exposed it
            if !observe(s, &mut obs, player, &psm, 0, 0, &left, frame[2][3]) {
                observe(s, &mut obs, player, &psm, 0, 1, &right, frame[2][3]);
            }
        } else {
            // both lookups can expose one entity during a reattach: the support controller makes no second dwell
            let same = !left.is_null() && !right.is_null() && entity_hash(&left) == entity_hash(&right);
            if !left.is_null() && !same {
                observe(s, &mut obs, player, &psm, 0, 0, &left, frame[1][3]);
            }
            observe(s, &mut obs, player, &psm, 1, 1, &right, frame[2][3]);
        }
    }
    let previous: Vec<u64> = s.dwell.active.iter().map(|(id, _)| *id).collect();
    if s.dwell.last().is_some_and(|l| now < l) {
        s.next_refresh = 0.0;
    }
    let refresh = stamp != s.dwell.stamp() && now >= s.next_refresh;
    let (active, stopped) = s.dwell.step(now, stamp, obs);
    for (_, target) in stopped {
        set_weapon_aim(&target, false);
    }
    if refresh {
        s.next_refresh = now + 0.1;
    }
    for (id, target) in active {
        if refresh || !previous.contains(&id) {
            set_weapon_aim(&target, true);
            if !s.reported {
                s.reported = true;
                log!("[mechanics] weapon threat: a barrel held on an NPC for two seconds -- its reactions take it as aimed (from Rust)\n");
            }
        }
    }
}

pub(super) fn tick() {
    let mut g = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let s = g.get_or_insert_with(|| State {
        dwell: Dwell::default(),
        next_poll: 0.0,
        next_refresh: 0.0,
        player_id: None,
        traces: 0,
        hits: 0,
        error: None,
        start: std::time::Instant::now(),
        reported: false,
    });
    let player = super::belt::player();
    update(s, &player);
    drop(g);
    expire_leases();
}
pub(super) fn shutdown() {
    let mut g = STATE.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(s) = g.as_mut() {
        clear(s);
        let _ = (s.traces, s.hits, s.error.take());
    }
}

// ---------------------------------------------------------------- the reactions
fn owner_npc(component: &Handle) -> Handle {
    let o = rt::obj(component, "GetOwner", &[]);
    if super::menu_rt::is_a(&o, "NPCPuppet") {
        o
    } else {
        Handle::null()
    }
}
/// ReactionManagerComponent.IsPlayerAiming: the leased NPC accepts the VR barrel; the ADS and visible-target rule
/// otherwise.
fn is_player_aiming(call: &mut Call) {
    if is_weapon_aimed_at(&owner_npc(&call.this())) {
        call.set_result_bool(true);
    }
}
/// A field written from an owned value of its own type.
fn set_from(obj: &Handle, field: &str, value: &Owned) {
    if let Some(f) = rt::field(obj, field) {
        // SAFETY: the field's own type assigns a value of its kind.
        unsafe { f.ty.assign(f.ptr, value.as_ptr().cast()) };
    }
}
fn set_num(obj: &Handle, field: &str, x: f64) {
    super::menu_rt::hset(obj, field, x);
}
fn delay_event(owner: &Handle, class: &str, delay: f64) -> Option<Owned> {
    let evt = super::lobby_ink::new(class);
    let ds = super::menu_rt::static_gi("GameInstance", "GetDelaySystem", &[]).map(|o| Val::of(&o).handle()).unwrap_or_else(Handle::null);
    rt::call(&ds, "DelayEvent", &[A::H(owner), A::H(&evt), A::N(delay)])
}
fn play_fear_beg(owner: &Handle, context: &str) {
    rt::static_call("GameObject", "PlayVoiceOver", &[A::H(owner), n("fear_beg"), n(context)]);
}
/// ReactionManagerComponent.OnCrowdReaction for a crowd member the VR barrel holds (the game's 2.5 function as main's
/// vrport_crowd_aim.reds replaces it): the ADS / visible-target guard of an AimingAt stimulus is met by the lease, and
/// everything after it runs as the game's own. Any other call runs the game's function.
fn crowd_reaction(call: &mut Call) {
    let this = call.this();
    let e = call.arg_handle(0);
    let owner = rt::obj(&this, "GetOwner", &[]);
    let stim_event = rt::field(&e, "stimEvent").map(|v| v.handle()).unwrap_or_else(Handle::null);
    if owner.is_null() || stim_event.is_null() {
        return;
    }
    let stim_type = num(&stim_event, "GetStimType", &[]);
    let vehicle_fear_phase = rt::field(&e, "vehicleFearPhase").and_then(|v| v.num()).unwrap_or(0.0) as i64;
    if stim_type != ev("gamedataStimType", "AimingAt") || vehicle_fear_phase == 3 || !is_weapon_aimed_at(&owner) {
        return; // the game's own
    }
    call.set_result_bool(false);
    if !rt::static_call("ScriptedPuppet", "IsActive", &[A::H(&owner)]).is_some_and(|o| Val::of(&o).bool()) {
        return;
    }
    // (an AimingAt stimulus: never HijackVehicle, never Bump)
    if rt::field(&this, "m_beignHijacked").is_some_and(|v| v.bool()) {
        return;
    }
    let gi = |getter: &str| super::menu_rt::static_gi("GameInstance", getter, &[]).map(|o| Val::of(&o).handle()).unwrap_or_else(Handle::null);
    let reaction_system = gi("GetReactionSystem");
    let crowd = rt::obj(&rt::obj(&this, "GetOwnerPuppet", &[]), "GetCrowdMemberComponent", &[]);
    rt::call(&crowd, "OnCrowdReaction", &[A::N(stim_type)]);
    // (the guard: met by the lease)
    if let (Some(ds), Some(id)) = (Some(gi("GetDelaySystem")), rt::field(&this, "m_callingPoliceID")) {
        rt::call(&ds, "CancelDelay", &[A::P(id.ptr)]);
    }
    let mut fear_phase = num(&this, "GetFearReactionPhase", &[A::H(&stim_event)]) as i64;
    if vehicle_fear_phase > 0 {
        fear_phase = vehicle_fear_phase;
    }
    let workspot = gi("GetWorkspotSystem");
    let local_player = rt::obj(&rt::obj(&this, "GetPlayerSystem", &[]), "GetLocalPlayerMainGameObject", &[]);
    let player_pos = rt::call(&local_player, "GetWorldPosition", &[]);
    if let Some(p) = player_pos.as_ref() {
        rt::call(&crowd, "SetThreatLastKnownPosition", &[A::P(p.as_ptr().cast())]);
    }
    rt::call(&crowd, "AllowWorkspotsUsage", &[A::B(false)]);
    rt::static_call("NPCPuppet", "ChangeHighLevelState", &[A::H(&owner), A::N(ev("gamedataNPCHighLevelState", "Fear"))]);
    if !super::menu_rt::static_gi("VehicleComponent", "IsMountedToVehicle", &[A::H(&owner)]).is_some_and(|o| Val::of(&o).bool()) {
        if let Some(id) = rt::call(&owner, "GetEntityID", &[]) {
            rt::call(&reaction_system, "MarkDespawnCandidate", &[A::P(id.as_ptr().cast())]);
        }
    }
    rt::call(&this, "DeactiveLookAt", &[]);
    rt::call(&this, "ResetFacial", &[A::N(0.0)]);
    rt::call(&this, "TriggerFearFacial", &[A::N(fear_phase as f64)]);
    if rt::field(&this, "m_previousFearPhase").and_then(|v| v.num()).unwrap_or(0.0) as i64 != 2 {
        let wrapper = rt::call(&this, "GetFearAnimWrapper", &[A::N(fear_phase as f64)]).map(|o| {
            // SAFETY: a CName result.
            unsafe { *(o.as_ptr() as *const u64) }
        });
        if let Some(w) = wrapper {
            rt::static_call("AnimationControllerComponent", "SetAnimWrapperWeightOnOwnerAndItems", &[A::H(&owner), A::Name(w), A::N(1.0)]);
        }
        if fear_phase != 0 && (!rt::field(&this, "m_fearLocomotionWrapper").is_some_and(|v| v.bool()) || fear_phase == 3) {
            let loco = rt::call(&this, "GetRandomFearLocomotionAnimWrapper", &[A::N(fear_phase as f64), A::N(stim_type)]).map(|o| {
                // SAFETY: a CName result.
                unsafe { *(o.as_ptr() as *const u64) }
            });
            if let Some(w) = loco {
                rt::static_call("AnimationControllerComponent", "SetAnimWrapperWeightOnOwnerAndItems", &[A::H(&owner), A::Name(w), A::N(1.0)]);
            }
        }
        set_num(&this, "m_fearLocomotionWrapper", 1.0);
    }
    let stage = match fear_phase {
        1 => "Stressed",
        2 => "Alarmed",
        3 => "Panic",
        _ => "Relaxed",
    };
    set_num(&this, "m_crowdFearStage", ev("gameFearStage", stage));
    if fear_phase == 3 {
        if let Some(p) = player_pos.as_ref() {
            rt::call(&crowd, "SetThreatLastKnownPosition", &[A::P(p.as_ptr().cast())]);
        }
    }
    let skip_initial = rt::field(&stim_event, "stimInvestigateData")
        .and_then(|d| d.f("skipInitialAnimation"))
        .is_some_and(|v| v.bool());
    rt::call(&crowd, "ChangeFearStage", &[A::N(ev("gameFearStage", stage)), A::B(!skip_initial)]);
    let source = rt::field(&stim_event, "sourceObject").map(|v| v.handle()).unwrap_or_else(Handle::null);
    let reaction_available = || flag(&workspot, "IsReactionAvailable", &[A::H(&owner), n("Fear")]);
    let fear_in_workspot = |this: &Handle| {
        rt::call(this, "ActivateReactionLookAt", &[A::H(&source), A::B(false), A::B(false), A::N(0.0), A::B(true)]);
        rt::call(&workspot, "SendReactionSignal", &[A::H(&owner), n("Fear")]);
        set_num(this, "m_inReactionSequence", 1.0);
        play_fear_beg(&owner, "Scripts:OnDelayedReaction");
    };
    let exit_workspot_later = |this: &Handle| {
        if let Some(id) = delay_event(&owner, "ExitWorkspotSequenceEvent", 3.0) {
            set_from(this, "m_exitWorkspotSequenceEventId", &id);
        }
    };
    let crowd_behavior = |this: &Handle, output: &str, a: bool, b: Option<bool>| {
        let mut args = vec![A::H(&stim_event), A::N(ev("gamedataOutput", output)), A::B(a)];
        if let Some(b) = b {
            args.push(A::B(b));
        }
        rt::call(this, "TriggerReactionBehaviorForCrowd", &args);
    };
    let run_away = |this: &Handle| {
        rt::call(this, "SetCrowdRunningAwayAnimFeature", &[A::N(stim_type)]);
    };
    let facial = |this: &Handle| {
        rt::call(this, "TriggerFacialLookAtReaction", &[A::B(true), A::B(true)]);
    };
    let in_reaction_sequence = rt::field(&this, "m_inReactionSequence").is_some_and(|v| v.bool());
    let previous_fear_phase = rt::field(&this, "m_previousFearPhase").and_then(|v| v.num()).unwrap_or(0.0) as i64;
    let near_point = || {
        let pos = rt::call(&owner, "GetWorldPosition", &[]);
        let nav = gi("GetAINavigationSystem");
        pos.and_then(|p| rt::call(&nav, "FindPointInSphereForCharacter", &[A::P(p.as_ptr().cast()), A::N(0.5), A::H(&owner)]))
            .and_then(|r| Val::of(&r).f("status").and_then(|s| s.num()))
            == Some(ev("worldNavigationRequestStatus", "OK"))
    };
    if flag(&workspot, "IsActorInWorkspot", &[A::H(&owner)]) {
        let owner_id = rt::call(&owner, "GetEntityID", &[]);
        let immovable = owner_id.as_ref().is_some_and(|id| flag(&reaction_system, "IsInImmovableWorkspot", &[A::P(id.as_ptr().cast())]));
        if immovable {
            if fear_phase > 1 && reaction_available() {
                fear_in_workspot(&this);
            } else {
                facial(&this);
            }
        } else {
            match fear_phase {
                0 => facial(&this),
                1 => {
                    let found = near_point();
                    let close = flag(&this, "IsTargetClose", &[A::H(&source), A::N(3.0)]) && flag(&this, "IsTargetInFront", &[A::H(&source)]);
                    if close && reaction_available() {
                        fear_in_workspot(&this);
                        exit_workspot_later(&this);
                    } else if found && flag(&workspot, "HasExitNodes", &[A::H(&owner), A::B(true), A::B(false), A::B(true)]) {
                        crowd_behavior(&this, "WalkAway", false, None);
                    } else if reaction_available() {
                        fear_in_workspot(&this);
                        exit_workspot_later(&this);
                    } else {
                        facial(&this);
                    }
                }
                2 => {
                    if reaction_available() {
                        fear_in_workspot(&this);
                        exit_workspot_later(&this);
                    } else if super::menu_rt::static_gi("VehicleComponent", "IsMountedToVehicle", &[A::H(&owner)]).is_some_and(|o| Val::of(&o).bool()) {
                        rt::call(&this, "ActivateReactionLookAt", &[A::H(&source), A::B(false), A::B(false), A::N(0.0), A::B(true), A::B(true)]);
                        let no_answer = [0u64; 1];
                        rt::static_call(
                            "GameObject",
                            "PlayVoiceOver",
                            &[A::H(&owner), n("fear_beg"), n("Scripts:OnCrowdReaction"), A::N(0.0), A::P(no_answer.as_ptr().cast()), A::B(true)],
                        );
                        if let Some(id) = delay_event(&owner, "DeescalateFearInVehicle", 3.0) {
                            set_from(&this, "m_exitFearInVehicleEventId", &id);
                        }
                    } else if !in_reaction_sequence {
                        if fear_phase == 3 {
                            run_away(&this);
                            crowd_behavior(&this, "Flee", false, None);
                        } else {
                            crowd_behavior(&this, "Surrender", false, None);
                        }
                    }
                }
                3 => {
                    if flag(&workspot, "IsWorkspotEnabled", &[A::H(&owner)])
                        && !flag(&workspot, "HasExitNodes", &[A::H(&owner), A::B(true), A::B(false), A::B(true)])
                        && (reaction_available() || in_reaction_sequence)
                    {
                        fear_in_workspot(&this);
                        exit_workspot_later(&this);
                    } else {
                        run_away(&this);
                        let flee_far = previous_fear_phase > 1 || flag(&workspot, "HasExitNodes", &[A::H(&owner), A::B(true), A::B(false)]);
                        crowd_behavior(&this, "Flee", flee_far, None);
                    }
                }
                _ => {}
            }
        }
    } else if rt::field(&this, "m_inTrafficLane").is_some_and(|v| v.bool()) {
        let ai = rt::obj(&rt::obj(&this, "GetOwnerPuppet", &[]), "GetAIControllerComponent", &[]);
        if let Some(f) = rt::field(&stim_event, "sourceObject") {
            if let Some(mut value) = Owned::of_type(f.ty) {
                // SAFETY: the field's own type copies a value of its kind.
                unsafe { f.ty.assign(value.as_mut_ptr().cast(), f.ptr) };
                if let Some(variant) = super::menu_rt::variant_of(&value) {
                    rt::call(&ai, "SetBehaviorArgument", &[n("StimTarget"), A::P(variant.as_ptr().cast())]);
                }
            }
        }
        match fear_phase {
            0 => facial(&this),
            1 => crowd_behavior(&this, "WalkAway", false, Some(true)),
            2 => {
                rt::call(&crowd, "TryStopTrafficMovement", &[]);
                crowd_behavior(&this, "Surrender", false, Some(true));
            }
            3 => {
                run_away(&this);
                crowd_behavior(&this, "Flee", false, Some(true));
            }
            _ => {}
        }
    } else {
        match fear_phase {
            0 => facial(&this),
            1 => {
                if near_point() {
                    crowd_behavior(&this, "WalkAway", false, None);
                } else {
                    facial(&this);
                }
            }
            2 => crowd_behavior(&this, "Surrender", true, None),
            3 => {
                run_away(&this);
                crowd_behavior(&this, "Flee", false, (previous_fear_phase > 1).then_some(true));
            }
            _ => {}
        }
    }
    let preset = rt::field(&this, "m_reactionPreset").map(|v| v.handle()).unwrap_or_else(Handle::null);
    let preset_child = !preset.is_null() && num(&preset, "Type", &[]) == ev("gamedataReactionPresetType", "Child");
    if !preset_child && source.instance == local_player.instance && !flag(&stim_event, "IsTagInStimuli", &[n("NoFearSpread")]) {
        rt::call(&this, "SpreadFear", &[A::N(fear_phase as f64)]);
    }
    set_num(&this, "m_desiredFearPhase", -1.0);
}

pub(super) const HOOKS: [(&str, &str, Kind, HookFn); 2] = [
    ("ReactionManagerComponent", "IsPlayerAiming", Kind::Override, is_player_aiming),
    ("ReactionManagerComponent", "OnCrowdReaction", Kind::Override, crowd_reaction),
];
