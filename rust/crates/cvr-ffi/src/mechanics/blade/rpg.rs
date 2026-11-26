//! vrport_blade_rpg.reds: what a confirmed blade hit is worth (VrBladeRPG), the receipt the health ledger publishes,
//! the follow-through along the lodged blade, the seat's interval and the joint to the person's body.
use super::*;
use crate::script_call::Value;

// ---------------------------------------------------------------- a hit's shape (gameShapeData, 0x70)
#[derive(Clone, Copy)]
pub(super) struct Shape(pub *mut u8);
struct ShapeLayout {
    result: usize,
    data: usize,
    material: usize,
    name: usize,
}
fn layout() -> Option<&'static ShapeLayout> {
    static AT: std::sync::OnceLock<Option<ShapeLayout>> = std::sync::OnceLock::new();
    AT.get_or_init(|| {
        let c = bn::class("gameShapeData")?;
        Some(ShapeLayout { result: bn::prop(c, "result")?, data: bn::prop(c, "userData")?, material: bn::prop(c, "physicsMaterial")?, name: bn::prop(c, "hitShapeName")? })
    })
    .as_ref()
}
impl Shape {
    fn at<T: Copy>(&self, offset: usize) -> T {
        // SAFETY: a live gameShapeData of the hit's array.
        unsafe { core::ptr::read_unaligned(self.0.add(offset) as *const T) }
    }
    pub(super) fn name(&self) -> u64 {
        layout().map_or(0, |l| self.at::<u64>(l.name))
    }
    pub(super) fn material(&self) -> u64 {
        layout().map_or(0, |l| self.at::<u64>(l.material))
    }
    pub(super) fn data(&self) -> Handle {
        // SAFETY: the user data handle in the struct, cloned.
        layout().map_or_else(Handle::null, |l| unsafe { (*(self.0.add(l.data) as *const Handle)).clone() })
    }
    pub(super) fn data_ptr(&self) -> *mut core::ffi::c_void {
        layout().map_or(core::ptr::null_mut(), |l| self.at::<*mut core::ffi::c_void>(l.data))
    }
    pub(super) fn enter(&self) -> V4 {
        layout().map_or(ZERO, |l| with_w(self.at::<V4>(l.result), 1.0))
    }
    pub(super) fn exit(&self) -> V4 {
        layout().map_or(ZERO, |l| with_w(self.at::<V4>(l.result + 16), 1.0))
    }
}
/// hitEvent.hitRepresentationResult.hitShapes.
pub(super) fn hit_shapes(hit: &Handle) -> Option<Val> {
    let result = rt::field(hit, "hitRepresentationResult")?;
    result.f("hitShapes").or_else(|| result.f("result").and_then(|q| q.f("hitShapes")))
}
pub(super) fn shapes_of(v: &Val) -> Vec<Shape> {
    (0..v.len()).filter_map(|i| v.at(i)).map(|e| Shape(e.ptr.cast())).collect()
}
/// Only the shape at `index` left in the hit's array (the others destructed in place).
pub(super) fn keep_only(v: &Val, index: usize) {
    let n = v.len();
    // SAFETY: the array value and its own type.
    unsafe {
        for i in (index + 1..n).rev() {
            v.ty.array_remove_at(v.ptr, i as i32);
        }
        for _ in 0..index {
            v.ty.array_remove_at(v.ptr, 0);
        }
    }
}

// ---------------------------------------------------------------- VrBladeRPG
pub(super) fn material_id(material: u64) -> i32 {
    let name = CName(material).text().to_string();
    if name.contains("armor") || name.contains("armour") {
        3
    } else if name.contains("metal") || name.contains("steel") {
        2
    } else if name.contains("cyber") {
        1
    } else if name.contains("flesh") || name.contains("skin") {
        0
    } else {
        4
    }
}
fn data_static(name: &str, data: &Handle) -> bool {
    sb("HitShapeUserDataBase", name, &[A::H(data)])
}
pub(super) fn shape_type(data: &Handle) -> f64 {
    rt::num(data, "GetShapeType", &[]).unwrap_or(-1.0)
}
pub(super) fn valid_zone_data(data: &Handle) -> bool {
    !data.is_null()
        && r::is_a(data, "HitShapeUserDataBase")
        && !data_static("IsProtectionLayer", data)
        && shape_type(data) != ev("EHitShapeType", "None")
        && (data_static("IsHitReactionZoneHead", data) || data_static("IsHitReactionZoneTorso", data) || data_static("IsHitReactionZoneLimb", data))
}
pub(super) fn valid_zone(shape: Shape) -> bool {
    valid_zone_data(&shape.data())
}
pub(super) fn zone_of(data: &Handle) -> f64 {
    rt::static_num("HitShapeUserDataBase", "GetHitReactionZone", &[A::H(data)]).unwrap_or(-1.0)
}
fn stealth_region(name: u64) -> bool {
    name == CName::hashed("Neck").0
}
/// PortContact: the shape is the neck's, or (a rig without one) the port lies along the blade within the native
/// contact interval of a Head, Chest or LeftTorso shape.
fn port_contact(impact: &Impact, shape: Shape) -> bool {
    let (entry, exit) = (shape.enter(), shape.exit());
    let name = shape.name();
    if stealth_region(name) {
        return true;
    }
    if impact.has_neck_slot {
        return false;
    }
    let port = impact.port_at_start;
    if port[3] < 0.50 || ![CName::hashed("Head").0, CName::hashed("Chest").0, CName::hashed("LeftTorso").0].contains(&name) {
        return false;
    }
    let along = closest(&port, &impact.a, &impact.b);
    if dist(&port, &along) > 0.06 {
        return false;
    }
    let a = closest(&entry, &impact.a, &impact.b);
    let b_ = if dist(&entry, &exit) <= 1.25 { closest(&exit, &impact.a, &impact.b) } else { a };
    (dist(&along, &closest(&along, &a, &b_)) - 0.06).max(0.0) <= 0.06
}
/// Behind: the player is behind the person's facing.
pub(super) fn behind(pl: &Handle, npc: &Handle) -> bool {
    let rel = sub(&v4_of(pl, "GetWorldPosition"), &v4_of(npc, "GetWorldPosition"));
    let facing = v4_of(npc, "GetWorldForward");
    rel[0] * facing[0] + rel[1] * facing[1] < 0.0
}
fn god(id: u64, mode: &str) -> bool {
    let g = super::super::game_system("GetGodModeSystem");
    !g.is_null() && rt::call(&g, "HasGodMode", &[A::P((&id as *const u64).cast()), A::N(ev("gameGodModeType", mode))]).is_some_and(|o| Val::of(&o).bool())
}
fn attitude(npc: &Handle, pl: &Handle) -> f64 {
    rt::static_num("GameObject", "GetAttitudeTowards", &[A::H(npc), A::H(pl)]).unwrap_or(-1.0)
}
/// WoundTarget: an active hostile or neutral human the blade may wound (no quest person, vendor, crowd or civilian,
/// nobody invulnerable).
pub(super) fn wound_target(pl: &Handle, npc: &Handle) -> bool {
    let id = eid(npc);
    let stats = super::super::game_system("GetStatsSystem");
    let sid = [id, 0u64];
    !stats.is_null()
        && b(npc, "IsActive")
        && !b(npc, "IsDead")
        && !b(npc, "IsIncapacitated")
        && !b(npc, "IsAboutToDieOrDefeated")
        && !sb("ScriptedPuppet", "IsDefeated", &[A::H(npc)])
        && !b(npc, "IsQuest")
        && !b(npc, "IsVendor")
        && !b(npc, "IsCrowd")
        && !b(npc, "IsCharacterCivilian")
        && attitude(npc, pl) != ev("EAIAttitude", "AIA_Friendly")
        && rt::num(npc, "GetNPCType", &[]) == Some(ev("gamedataNPCType", "Human"))
        && !god(id, "Invulnerable")
        && !god(id, "Immortal")
        && !rt::call(&stats, "GetStatBoolValue", &[A::P(sid.as_ptr().cast()), A::N(ev("gamedataStatType", "IsInvulnerable"))]).is_some_and(|o| Val::of(&o).bool())
}
fn in_workspot(o: &Handle) -> bool {
    let ws = super::super::game_system("GetWorkspotSystem");
    !ws.is_null() && rt::call(&ws, "IsActorInWorkspot", &[A::H(o)]).is_some_and(|x| Val::of(&x).bool())
}
/// SpecialTarget: a hostile ordinary enemy (trash to elite) the player could take down right now.
pub(super) fn special_target(pl: &Handle, npc: &Handle) -> bool {
    if !wound_target(pl, npc) || psm_board_null(pl) {
        return false;
    }
    let rarity = rt::num(npc, "GetNPCRarity", &[]).unwrap_or(-1.0);
    let record = rt::call(npc, "GetRecordID", &[]).map_or(0u64, |o| {
        // SAFETY: a TweakDBID result.
        unsafe { *(o.as_ptr() as *const u64) }
    });
    let id = eid(npc);
    let ses = super::super::game_system("GetStatusEffectSystem");
    psm_int(pl, "Takedown") == Some(ev("gamePSMTakedown", "Default"))
        && !b(pl, "GetIsInFastFinisher")
        && !b(pl, "GetIsInWorkspotFinisher")
        && !in_workspot(pl)
        && !in_workspot(npc)
        && !b(npc, "IsRagdolling")
        && !b(npc, "IsBoss")
        && !b(npc, "IsMassive")
        && attitude(npc, pl) == ev("EAIAttitude", "AIA_Hostile")
        && ["Trash", "Weak", "Normal", "Elite"].iter().any(|m| rarity == ev("gamedataNPCRarity", m))
        && !b(npc, "IsResistantToTakedown")
        && !sb("NPCManager", "HasTag", &[A::P((&record as *const u64).cast()), A::Name(CName::hashed("ImmuneToTakedown").0)])
        && !sb("NPCManager", "HasTag", &[A::P((&record as *const u64).cast()), A::Name(CName::hashed("MiniBoss").0)])
        && !(!ses.is_null()
            && rt::call(&ses, "HasStatusEffect", &[A::P((&id as *const u64).cast()), A::P((&red4ext_abi::tweakdbid::from_name("BaseStatusEffect.BlockGrapple") as *const u64).cast())])
                .is_some_and(|o| Val::of(&o).bool()))
        && rt::static_num("RPGManager", "CalculatePowerDifferential", &[A::H(npc)]) != Some(ev("gameEPowerDifferential", "IMPOSSIBLE"))
}
fn psm_board_null(pl: &Handle) -> bool {
    rt::obj(pl, "GetPlayerStateMachineBlackboard", &[]).is_null()
}
/// FollowMatches: a follow-through's hit is the receipt's own -- the same player, person, slot and shape (or, for an
/// edge cut, its live stroke on the same hit representation).
pub(super) fn follow_matches(hit: &Handle, impact_rc: &Sh<Impact>, shape: Shape) -> bool {
    let data = rt::field(hit, "attackData").map(|v| v.handle()).unwrap_or_else(Handle::null);
    let w = r::o(&data, "GetWeapon", &[]);
    let pl = r::o(&data, "GetInstigator", &[]);
    let npc = rt::field(hit, "target").map(|v| v.handle()).unwrap_or_else(Handle::null);
    if w.is_null() || !r::is_a(&pl, "PlayerPuppet") || !r::is_a(&npc, "NPCPuppet") {
        return false;
    }
    let Some(rc) = receipt(&w) else { return false };
    let impact = impact_rc.borrow();
    {
        let r = rc.borrow();
        let pending_is = r.pending.as_ref().is_some_and(|p| Rc::ptr_eq(p, impact_rc));
        if !allowed(&pl, &w) || r.serial != impact.parent_serial || r.owner().instance != pl.instance || r.target().instance != npc.instance || !pending_is || impact.cancelled || r.released || r.spent {
            return false;
        }
    }
    if !special_target(&pl, &npc) {
        return false;
    }
    let age = confirmed_age(&w);
    if age < 0.0 || sim_time() - impact.created > 0.20 || impact.contact_metrics[0] <= 0.0 || impact.contact_metrics[2] < 0.96 {
        return false;
    }
    let r = rc.borrow();
    if impact.edge_stroke != 0 {
        return r.lodged && impact.edge_stroke != r.last_edge_stroke && r.slots_ptr == hit_representation(&npc).instance as usize && bn::edge_stroke_live(hand_side(&pl), r.joint_serial, impact.edge_stroke);
    }
    r.zone == zone_of(&shape.data()) && r.slot != 0 && r.shape_name == shape.name() && same_shape_data(&r, shape) && impact.overlap_point[3] > 0.50
}
/// SameShapeData: the receipt's user data, or a clone of the same material and capsule.
fn same_shape_data(r: &Receipt, shape: Shape) -> bool {
    let data = shape.data();
    if r.shape_data.is_null() || data.is_null() {
        return false;
    }
    if r.shape_data.instance == data.instance {
        return true;
    }
    let Some(cap) = r.capsule else { return false };
    if material_id(shape.material()) != r.material {
        return false;
    }
    let Some(g) = bn::read_capsule(r.slots_ptr as *mut core::ffi::c_void, shape.name(), data.instance.cast(), None) else { return false };
    dist(&g.0, &cap.0) < 0.0001 && dist(&g.1, &cap.1) < 0.0001 && (g.0[3] - cap.0[3]).abs() < 0.0001 && (g.1[3] - cap.1[3]).abs() < 0.0001
}
fn budget(current: f32, maximum: f32, floor: f32, elite: bool) -> f32 {
    let available = (current - floor.max(0.0)).max(0.0);
    if elite {
        available.min(0.80 * maximum)
    } else {
        available
    }
}
fn follow_factor(strength: f32) -> f32 {
    1.75 + 0.25 * ((strength - 3.0) / 17.0).clamp(0.0, 1.0)
}
fn flag(data: &Handle, f: &str) -> bool {
    rt::call(data, "HasFlag", &[A::N(ev("hitFlag", f))]).is_some_and(|o| Val::of(&o).bool())
}
/// Rejected: blocked, deflected, dodged, evaded, nullified or otherwise dealing nothing.
pub(super) fn rejected(data: &Handle) -> bool {
    b(data, "WasBlocked")
        || b(data, "WasDeflectedAny")
        || ["WasDodged", "WasEvaded", "DealNoDamage", "DamageNullified", "FriendlyFireIgnored", "ImmortalTarget", "TargetWasAlreadyDeadNoStatPool"].iter().any(|f| flag(data, f))
}
fn pool(pools: &Handle, id: u64, f: &str, extra: bool) -> f32 {
    let sid = [id, 0u64];
    let args: Vec<A> = if extra { vec![A::P(sid.as_ptr().cast()), A::N(ev("gamedataStatPoolType", "Health")), A::B(false)] } else { vec![A::P(sid.as_ptr().cast()), A::N(ev("gamedataStatPoolType", "Health"))] };
    rt::num(pools, f, &args).unwrap_or(0.0) as f32
}
fn pool_of(pools: &Handle, id: u64, t: &str) -> f32 {
    let sid = [id, 0u64];
    rt::num(pools, "GetStatPoolValue", &[A::P(sid.as_ptr().cast()), A::N(ev("gamedataStatPoolType", t)), A::B(false)]).unwrap_or(0.0) as f32
}
fn physical(computed: &Handle) -> f32 {
    rt::num(computed, "GetAttackValue", &[A::N(ev("gamedataDamageType", "Physical"))]).unwrap_or(0.0) as f32
}
fn set_physical(computed: &Handle, v: f32) {
    rt::call(computed, "SetAttackValue", &[A::N(f64::from(v)), A::N(ev("gamedataDamageType", "Physical"))]);
}
/// ApplyRule: the blade's damage for the exact contact about to be applied -- a follow-through x1.75..2 by Strength,
/// a thrust x1.5 and bleeding, an unaware enemy thrust from behind (or through the neck) its remaining health.
pub(super) fn apply_rule(hit: &Handle, impact_rc: &Sh<Impact>, pl: &Handle, npc: &Handle, w: &Handle) {
    let data = rt::field(hit, "attackData").map(|v| v.handle()).unwrap_or_else(Handle::null);
    let computed = rt::field(hit, "attackComputed").map(|v| v.handle()).unwrap_or_else(Handle::null);
    let stats = super::super::game_system("GetStatsSystem");
    let pools = super::super::game_system("GetStatPoolsSystem");
    let id = eid(npc);
    let sid = [id, 0u64];
    let Some(first) = hit_shapes(hit).and_then(|v| shapes_of(&v).first().copied()) else { return };
    let shape = first.data();
    let shape_ok = r::is_a(&shape, "HitShapeUserDataBase");
    let phys = physical(&computed);
    {
        let mut i = impact_rc.borrow_mut();
        i.physical_before = phys;
        i.physical_after = phys;
    }
    if pools.is_null() {
        return;
    }
    let hp = pool(&pools, id, "GetStatPoolValue", true);
    let max_hp = pool(&pools, id, "GetStatPoolMaxPointValue", false);
    let floor = pool(&pools, id, "GetStatPoolValueCustomLimit", true);
    let min_pct = rt::num(&data, "GetMinimumHealthPercent", &[]).unwrap_or(0.0) as f32;
    impact_rc.borrow_mut().available_health = hp - floor.max(max_hp * min_pct * 0.01);
    let (thrust, stealth_intent, parent) = {
        let i = impact_rc.borrow();
        (i.thrust, i.stealth_intent, i.parent_serial)
    };
    let stat = |name: &str| rt::num(&stats, "GetStatValue", &[A::P(sid.as_ptr().cast()), A::N(ev("gamedataStatType", name))]).unwrap_or(0.0);
    let damage_mod = if shape_ok { rt::field(&shape, "m_hitShapeDamageMod").and_then(|v| v.num()).unwrap_or(0.0) } else { 0.0 };
    let total = rt::num(&computed, "GetTotalAttackValue", &[A::N(ev("gamedataStatPoolType", "Health"))]).unwrap_or(0.0) as f32;
    if (!thrust && !stealth_intent && parent == 0)
        || stats.is_null()
        || !wound_target(pl, npc)
        || !allowed(pl, w)
        || rejected(&data)
        || ["Nonlethal", "ReduceDamage", "WasMitigated", "CannotModifyDamage", "DeterministicDamage", "DoNotTriggerFinisher", "Kill", "FinisherTriggered", "IgnoreImmortalityModes", "IgnoreStatPoolCustomLimit"]
            .iter()
            .any(|f| flag(&data, f))
        || min_pct > 0.0
        || rt::num(&data, "GetHitType", &[]) == Some(ev("gameuiHitType", "Glance"))
        || !shape_ok
        || shape_type(&shape) != ev("EHitShapeType", "Flesh")
        || (damage_mod > 0.0 && damage_mod < 1.0)
        || stat("Armor") > 0.0
        || stat("PhysicalResistance") > 0.0
        || stat("MeleeResistance") > 0.0
        || stat("MaxPercentDamageTakenPerHit") > 0.0
        || pool_of(&pools, id, "Overshield") > 0.0
        || pool_of(&pools, id, "CPO_Armor") > 0.0
        || !(phys > 0.0)
        || phys != total
    {
        return;
    }
    if parent != 0 {
        if !special_target(pl, npc) {
            return;
        }
        let pid = [eid(pl), 0u64];
        let strength = rt::num(&stats, "GetStatValue", &[A::P(pid.as_ptr().cast()), A::N(ev("gamedataStatType", "Strength"))]).unwrap_or(0.0) as f32;
        let after = phys * follow_factor(strength);
        {
            let mut i = impact_rc.borrow_mut();
            i.strength_reaction = !i.no_dismember && strength >= 12.0;
            i.physical_after = after;
        }
        set_physical(&computed, after);
        return;
    }
    if thrust {
        let after = phys * 1.50;
        {
            let mut i = impact_rc.borrow_mut();
            i.physical_after = after;
            i.bleed_on_hit = true;
        }
        set_physical(&computed, after);
    }
    if !stealth_intent || !special_target(pl, npc) {
        return;
    }
    let port = {
        let mut i = impact_rc.borrow_mut();
        i.stealth_stage = 1;
        let m = port_contact(&i, first);
        i.port_matched = m;
        m
    };
    let rear_thrust = thrust && (data_static("IsHitReactionZoneTorso", &shape) || data_static("IsHitReactionZoneHead", &shape));
    if kind(w) <= 0 || !impact_rc.borrow().rear_at_start || !(rear_thrust || port) {
        return;
    }
    impact_rc.borrow_mut().stealth_stage = 2;
    let board = rt::obj(npc, "GetPuppetStateBlackboard", &[]);
    if !flag(&data, "StealthHit")
        || sb("AttackData", "IsPlayerInCombat", &[A::H(&data)])
        || board.is_null()
        || rt::num(npc, "GetHighLevelStateFromBlackboard", &[]) != Some(ev("gamedataNPCHighLevelState", "Relaxed"))
        || rt::num(npc, "GetDetectionPercentage", &[]).unwrap_or(1.0) != 0.0
    {
        return;
    }
    let budget = budget(hp, max_hp, floor, b(npc, "IsElite"));
    // sub-point channels round up to 1 in ApplyDamage: no smaller cap promised then
    if !(hp > 0.0 && max_hp >= hp && budget >= 1.0) {
        return;
    }
    set_physical(&computed, budget);
    let mut i = impact_rc.borrow_mut();
    i.physical_after = budget;
    i.stealth_applied = true;
    i.bleed_on_hit = false;
}

// ---------------------------------------------------------------- the receipt
/// PlayerPuppet.VRBladePublishHit: the health the blade's hit took publishes its receipt (false: the hit is not the
/// blade's own, nothing confirmed).
pub(in crate::mechanics) fn publish_hit(pl: &Handle, hit: &Handle, health_loss: f64, ticket: &melee::TicketView) -> bool {
    let _g = lock();
    let data = rt::field(hit, "attackData").map(|v| v.handle()).unwrap_or_else(Handle::null);
    let w = r::o(&data, "GetWeapon", &[]);
    let blade = r::o(&data, "GetAttackDefinition", &[]);
    let npc = rt::field(hit, "target").map(|v| v.handle()).unwrap_or_else(Handle::null);
    let projection = rt::field(hit, "projectionPipeline").is_some_and(|v| v.bool());
    let instigator = r::o(&data, "GetInstigator", &[]);
    if !(health_loss > 0.0)
        || projection
        || !r::is_a(&npc, "NPCPuppet")
        || w.is_null()
        || blade.is_null()
        || !same_attack(&blade, &ticket.attack)
        || !melee::same_item(&melee::item_id(&w), &ticket.weapon)
        || ticket.target != eid(&npc)
        || instigator.instance != pl.instance
        || is_client()
        || !in_hand(pl, &w)
    {
        return false;
    }
    let Some(impact_rc) = attack_impact(&blade) else { return false };
    let Some(shapes) = hit_shapes(hit) else { return false };
    let list = shapes_of(&shapes);
    {
        let i = impact_rc.borrow();
        if !i.claimed || i.cancelled || i.published || i.target != ticket.target || list.len() != 1 || rejected(&data) || !(i.available_health > 0.0) {
            return false;
        }
    }
    let shape = list[0];
    let parent = impact_rc.borrow().parent_serial;
    let previous = receipt(&w);
    let confirmed = previous.as_ref().map_or(0, |r| r.borrow().serial);
    if !valid_zone(shape) || (parent != 0 && parent != confirmed) {
        return false;
    }
    impact_rc.borrow_mut().published = true;
    let mut serial = confirmed.wrapping_add(1);
    if serial == 0 {
        serial = 1;
    }
    let npc_xf = world_transform(&npc).unwrap_or_default();
    let impact = impact_rc.borrow();
    let entry_point = with_w(if parent != 0 { impact.overlap_point } else { shape.enter() }, 1.0);
    let slots = hit_representation(&npc);
    let mut rc = Receipt {
        serial,
        time: sim_time(),
        owner: WeakHandle::from_handle(pl),
        target: WeakHandle::from_handle(&npc),
        target_id: eid(&npc),
        point: to_local(&npc_xf, &entry_point),
        slots: WeakHandle::from_handle(&slots),
        slots_ptr: slots.instance as usize,
        shape_name: shape.name(),
        shape_data: shape.data(),
        ..Receipt::default()
    };
    let neck = CName::hashed("Neck").0;
    let binding = if impact.edge_stroke != 0 { impact.cut_slot } else if rc.shape_name == neck { CName::hashed("Head").0 } else { rc.shape_name };
    let sp = slots.instance.cast();
    if binding != 0 {
        if let Some(frame) = bn::slot_frame(sp, binding) {
            rc.slot = binding;
            rc.part_point = to_local(&frame, &entry_point);
            if impact.edge_stroke != 0 {
                rc.part_point = impact.cut_point;
                rc.capsule = impact.cut_shape.as_ref().map(|c| c.capsule);
            } else {
                rc.capsule = bn::read_capsule(sp, rc.shape_name, shape.data_ptr(), None);
            }
            rc.mesh_start = to_local(&frame, &impact.a);
            rc.mesh_end = to_local(&frame, &impact.b);
        }
    }
    rc.zone = zone_of(&shape.data());
    rc.material = material_id(shape.material());
    rc.follow = parent != 0;
    rc.stealth = impact.stealth_applied;
    rc.health_loss = (health_loss as f32).min(impact.available_health);
    rc.entry = if rc.follow { previous.as_ref().and_then(|p| p.borrow().entry.clone()) } else { impact.entry.clone() };
    rc.puncture = impact.thrust || impact.stealth_applied;
    let disable_vfx = flag(&data, "DisableVFX");
    if rc.follow {
        if let Some(prev) = &previous {
            let p = prev.borrow();
            rc.joint_serial = p.joint_serial;
            rc.seat_point = p.seat_point;
            rc.seat_slot = p.seat_slot;
            rc.seat_part_point = p.seat_part_point;
            rc.seat_axis = p.seat_axis;
            rc.seat_edge = p.seat_edge;
            rc.attachment = p.attachment.clone();
            rc.lodged = p.lodged;
            rc.lodged_at = p.lodged_at;
            rc.requested_bodies = p.requested_bodies;
        }
        rc.last_edge_stroke = impact.edge_stroke;
        if impact.edge_stroke == 0 && !disable_vfx {
            if let Some(prev) = &previous {
                super::mark::continue_trail(&mut rc, &prev.borrow());
            }
        }
    } else {
        rc.seat_point = rc.point;
        rc.seat_slot = rc.slot;
        rc.seat_part_point = rc.part_point;
        rc.released = impact.entry.as_ref().is_some_and(|e| e.borrow().ended_clear);
        if rc.puncture && !rc.released {
            rc.seat_axis = norm(&sub(&impact.b, &impact.a));
            let along = dot(&impact.attack_edge, &rc.seat_axis);
            rc.seat_edge = norm(&sub(&impact.attack_edge, &mul(&rc.seat_axis, along)));
            rc.awaiting_seat = true;
            rc.lodged_at = real_time();
        }
        rc.joint_serial = next_joint_serial();
    }
    let follow = rc.follow;
    let seated_reaction = rc.awaiting_seat || (rc.follow && rc.lodged);
    let (bleed, swing, edge_stroke) = (impact.bleed_on_hit, impact.swing.clone(), impact.edge_stroke);
    drop(impact);
    if !follow {
        if let Some(prev) = &previous {
            let lodged = prev.borrow().lodged && !prev.borrow().target().is_null();
            if lodged {
                super::hold::end(&w, prev);
            }
        }
    }
    set_seated(hit, seated_reaction);
    let rc = sh(rc);
    weapon(&w).borrow_mut().receipt = Some(rc.clone());
    let bleeding = red4ext_abi::tweakdbid::from_name("BaseStatusEffect.Bleeding");
    if bleed && !b(&npc, "IsDead") && !sb("StatusEffectSystem", "ObjectHasStatusEffect", &[A::H(&npc), A::P((&bleeding as *const u64).cast())]) {
        let pid = eid(pl);
        rt::static_call("StatusEffectHelper", "ApplyStatusEffect", &[A::H(&npc), A::P((&bleeding as *const u64).cast()), A::P((&pid as *const u64).cast())]);
    }
    if !follow {
        if let Some((side, s)) = &swing {
            melee::chain_landed_once(pl, *side, s);
        }
    }
    if (!follow || edge_stroke != 0) && !disable_vfx {
        super::mark::add_surface_mark(&npc, shape, &rc, &impact_rc);
    }
    true
}
fn is_client() -> bool {
    let info = r::static_gi("GameInstance", "GetRuntimeInfo", &[]).map(|o| Val::of(&o).handle()).unwrap_or_else(Handle::null);
    b(&info, "IsClient")
}
/// WeaponObject.VRBladeAnchorWorld: the lodged point in the world (its seat slot's frame; with allowRoot, else the
/// person's root).
pub(super) fn anchor_world(w: &Handle, allow_root: bool) -> V4 {
    let Some(rc) = receipt(w) else { return ZERO };
    let r = rc.borrow();
    let target = r.target();
    if target.is_null() || !b(&target, "IsAttached") {
        return ZERO;
    }
    if r.seat_slot != 0 {
        if let Some(frame) = bn::slot_frame(r.slots_ptr as *mut core::ffi::c_void, r.seat_slot).filter(|_| !r.slots().is_null()) {
            return to_world(&frame, &r.seat_part_point);
        }
        if !allow_root {
            return ZERO;
        }
    }
    let xf = world_transform(&target).unwrap_or_default();
    to_world(&xf, &r.seat_point)
}
/// WeaponObject.VRBladeLoadRagdoll: the hand's reaction pushes an already simulated ragdoll at the anchor (the equal
/// and opposite side of the soft hand constraint), at most each 50 ms.
pub(super) fn load_ragdoll(w: &Handle, serial: u32, wrist: V4) -> bool {
    let Some(rc) = receipt(w) else { return false };
    let (owner, target, ok) = {
        let r = rc.borrow();
        (r.owner(), r.target(), serial != 0 && r.serial == serial && !r.follow && !r.spent && r.slot != 0)
    };
    if !ok || owner.is_null() || target.is_null() || !b(&target, "IsRagdolling") || !allowed(&owner, w) {
        return false;
    }
    let wrist = with_w(wrist, 0.0);
    let magnitude = len(&wrist);
    if !(magnitude > 0.006 && magnitude <= 0.081) {
        return false;
    }
    let now = sim_time();
    let dt = {
        let r = rc.borrow();
        now - r.time.max(r.load_time)
    };
    if dt < 0.05 {
        return false;
    }
    let anchor = anchor_world(w, false);
    if anchor[3] < 0.50 {
        return false;
    }
    rc.borrow_mut().load_time = now;
    let impulse = mul(&wrist, -160.0 * dt.min(0.08) as f32);
    let ev = rt::global_obj("CreateRagdollApplyImpulseEvent", &[A::P(anchor.as_ptr().cast()), A::P(impulse.as_ptr().cast()), A::N(0.12)]);
    if !ev.is_null() {
        rt::call(&target, "QueueEvent", &[A::H(&ev)]);
    }
    true
}
/// VRBladeFollowResult: the gate the last follow-through stopped at (true only for a dispatched one).
pub(super) fn follow_result(w: &Handle, gate: &'static str) -> bool {
    weapon(w).borrow_mut().follow_gate = gate;
    gate == "Dispatched"
}
/// PlayerPuppet.VRBladeTryFollowThrough: a deliberate stroke along the lodged (or touching) blade continues the
/// confirmed hit as a new native effect on the same shape.
#[allow(clippy::too_many_arguments)]
pub(super) fn try_follow_through(pl: &Handle, w: &Handle, serial: u32, a: &V4, b_: &V4, motion: V4) -> bool {
    if w.is_null() {
        return false;
    }
    {
        let st = weapon(w);
        let mut s = st.borrow_mut();
        s.follow_serial = serial;
        s.follow_attempt = s.follow_attempt.wrapping_add(1);
        s.follow_counts = [0.0, 0.0, 0.0, -1.0];
        s.follow_geometry = ZERO;
        s.follow_window = ZERO;
    }
    if serial == 0 || confirmed_serial(w) != serial {
        return follow_result(w, "ReceiptChanged");
    }
    if !allowed(pl, w) || melee::holster_intent(pl) {
        return follow_result(w, "WeaponUnavailable");
    }
    let Some(rc) = receipt(w) else { return follow_result(w, "ReceiptInvalid") };
    let age = confirmed_age(w);
    let (owner, target) = {
        let r = rc.borrow();
        (r.owner(), r.target())
    };
    if owner.instance != pl.instance || age < 0.0 || target.is_null() {
        return follow_result(w, "ReceiptInvalid");
    }
    let (spent, released, lodged, puncture) = {
        let r = rc.borrow();
        (r.spent, r.released, r.lodged, r.puncture)
    };
    if spent || released {
        return follow_result(w, "SpentOrReleased");
    }
    if !special_target(pl, &target) {
        return follow_result(w, if b(&target, "IsRagdolling") { "TargetRagdoll" } else { "TargetRules" });
    }
    if rc.borrow().pending.as_ref().is_some_and(|p| sim_time() - p.borrow().created < 0.085) {
        return follow_result(w, "Pending");
    }
    let l = dist(a, b_);
    if !(l >= 0.04 && l <= melee::reach(w) as f32 + 0.02 && dist(a, &v4_of(w, "GetWorldPosition")) <= 0.15) {
        return follow_result(w, "WeaponPose");
    }
    let motion = with_w(motion, 0.0);
    if !(len(&motion) >= 0.045 && len(&motion) <= 0.35) {
        return follow_result(w, "ControllerDistance");
    }
    let impact = sh(Impact { parent_serial: serial, target: eid(&target), a: *a, b: *b_, ..Impact::default() });
    if lodged || puncture {
        if !lodged {
            return follow_result(w, "NotLodged");
        }
        if !super::cut::capture_cut(w, &impact, &motion) {
            return false;
        }
    } else {
        let measured = measure(w, serial, a, b_, &ZERO);
        if !(measured[0] > 0.0 && measured[2] >= 0.96) {
            return follow_result(w, "EntryShapeMotion");
        }
        let anchor = overlap_point(w, a, b_, &v4_of(w, "GetWorldForward"));
        if anchor[3] < 0.50 {
            return follow_result(w, "EntryShapeOverlap");
        }
        let mut i = impact.borrow_mut();
        i.overlap_point = anchor;
        i.contact_metrics = measured;
    }
    {
        let mut i = impact.borrow_mut();
        i.created = sim_time();
        i.no_dismember = no_dismember(w);
    }
    if let Some(p) = rc.borrow_mut().pending.replace(impact.clone()) {
        p.borrow_mut().cancelled = true;
    }
    note_probe(w, &impact);
    if melee::ordinary_contact(pl, w, &target, a, b_, false, Some(impact.clone())) > 0 {
        return follow_result(w, "Dispatched");
    }
    impact.borrow_mut().cancelled = true;
    rc.borrow_mut().pending = None;
    follow_result(w, "EffectStart")
}
/// WeaponObject.VRBladeCutGap: a native entry on the occupied shaft (in the slot's frame now, or as observed).
pub(super) fn cut_gap(w: &Handle, shape: &super::cut::CutShape, hit: Shape) -> f32 {
    let Some(rc) = receipt(w) else { return 100_000.0 };
    let r = rc.borrow();
    if r.slots().is_null() || shape.name != hit.name() {
        return 100_000.0;
    }
    let sp = r.slots_ptr as *mut core::ffi::c_void;
    let Some(frame) = bn::slot_frame(sp, shape.slot) else { return 100_000.0 };
    let obs = &shape.observation;
    if !bn::matches_occupied(sp, hit.name(), hit.data_ptr(), hit.material(), shape.slot, obs.geometry.instance.cast(), obs.data.instance.cast(), obs.material, &shape.capsule.0, &shape.capsule.1) {
        return 100_000.0;
    }
    let entry = hit.enter();
    let here = to_local(&frame, &entry);
    let then = to_local(&shape.observed_frame, &entry);
    dist(&here, &closest(&here, &shape.shaft_from, &shape.shaft_to)).min(dist(&then, &closest(&then, &shape.shaft_from, &shape.shaft_to)))
}
/// WeaponObject.VRBladeMeasure: the blade against the receipt's capsule, in its slot's frame (VRBladeCapsuleMetrics).
pub(super) fn measure(w: &Handle, serial: u32, a: &V4, b_: &V4, motion: &V4) -> V4 {
    let Some(rc) = receipt(w) else { return ZERO };
    let r = rc.borrow();
    let (target, owner) = (r.target(), r.owner());
    let Some(cap) = r.capsule else { return ZERO };
    if serial == 0 || r.serial != serial || r.slots().is_null() || target.is_null() || !b(&target, "IsAttached") || owner.is_null() || dist(a, b_) > melee::reach(w) as f32 + 0.02 {
        return ZERO;
    }
    let Some(frame) = bn::slot_frame(r.slots_ptr as *mut core::ffi::c_void, r.slot) else { return ZERO };
    let (material, slot_q) = (r.material, frame.orientation());
    drop(r);
    if !allowed(&owner, w) {
        return ZERO;
    }
    let inverse = bn::conj(&slot_q);
    bn::capsule_result(&to_local(&frame, a), &to_local(&frame, b_), &rotate(&inverse, motion), &rotate(&inverse, &v4_of(w, "GetWorldForward")), &cap.0, &cap.1, material, false)
}
fn receipt_interval(w: &Handle, a: &V4, b_: &V4, edge: &V4) -> V4 {
    let Some(rc) = receipt(w) else { return ZERO };
    let r = rc.borrow();
    let Some(cap) = r.capsule else { return ZERO };
    if r.slots().is_null() {
        return ZERO;
    }
    let Some(frame) = bn::slot_frame(r.slots_ptr as *mut core::ffi::c_void, r.slot) else { return ZERO };
    bn::capsule_result(&to_local(&frame, a), &to_local(&frame, b_), &ZERO, &rotate(&bn::conj(&frame.orientation()), edge), &cap.0, &cap.1, r.material, true)
}
/// WeaponObject.VRBladeOverlapPoint: where the blade enters the receipt's capsule.
pub(super) fn overlap_point(w: &Handle, a: &V4, b_: &V4, edge: &V4) -> V4 {
    let interval = receipt_interval(w, a, b_, edge);
    if interval[3] < 0.5 || interval[1] <= 0.0 {
        return ZERO;
    }
    with_w(lerp(a, b_, interval[0]), 1.0)
}
/// WeaponObject.VRBladeSeatInterval: the blade's interval in the receipt's shape, else (a puncture) in any shape.
pub(super) fn seat_interval(w: &Handle, serial: u32, a: &V4, b_: &V4, edge: &V4) -> V4 {
    let Some(rc) = receipt(w) else { return ZERO };
    let (target, owner, ok, lodgedish) = {
        let r = rc.borrow();
        (r.target(), r.owner(), r.serial == serial && !r.released, r.puncture || r.lodged)
    };
    if !ok || target.is_null() || !b(&target, "IsAttached") || owner.is_null() || !allowed(&owner, w) || dist(a, b_) > melee::reach(w) as f32 + 0.02 {
        return ZERO;
    }
    let interval = receipt_interval(w, a, b_, edge);
    if lodgedish && interval[3] < 0.5 {
        let shapes = hit_representation(&target);
        let body = bn::swept_contact(shapes.instance.cast(), &[p3(a), p3(b_), p3(a), p3(b_)], None);
        if body[3] > 0.5 {
            return [body[1], body[2], 0.0, 1.0];
        }
    }
    interval
}
/// WeaponObject.VRBladeBindSlot: the material point the blade holds -- in the receipt's capsule, else the deepest
/// occupied shape.
fn bind_slot(rc: &Sh<Receipt>, component: &Handle, a: &V4, b_: &V4, edge: &V4) -> Option<SlotAttachment> {
    let cp = component.instance.cast::<core::ffi::c_void>();
    let mut hit = SlotAttachment { slots: WeakHandle::from_handle(component), slots_ptr: component.instance as usize, ..SlotAttachment::default() };
    let r = rc.borrow();
    let mut frame = WorldTransform::default();
    let mut interval = ZERO;
    if component.instance as usize == r.slots_ptr {
        if let (Some(cap), Some(f)) = (r.capsule, bn::slot_frame(cp, r.slot)) {
            frame = f;
            interval = bn::capsule_result(&to_local(&f, a), &to_local(&f, b_), &ZERO, &rotate(&bn::conj(&f.orientation()), edge), &cap.0, &cap.1, r.material, true);
        }
    }
    if interval[3] > 0.5 {
        hit.shape = r.shape_name;
        hit.slot = r.slot;
        hit.capsule = r.capsule.unwrap_or((ZERO, ZERO));
        hit.fraction = interval[0];
        hit.depth = interval[1];
    } else {
        let pin_frame = frame;
        bn::collect_occupied(cp, a, b_, edge, CName::hashed("None").0, &pin_frame, |o| {
            if o.interval[1] > hit.depth {
                hit.shape = o.name;
                hit.slot = o.slot;
                hit.capsule = (o.center, o.axis);
                hit.fraction = o.interval[0];
                hit.depth = o.interval[1];
                frame = o.frame;
            }
            Some(true)
        });
    }
    if hit.depth <= 0.0 {
        return None;
    }
    hit.local_point = to_local(&frame, &lerp(a, b_, hit.fraction));
    Some(hit)
}
/// WeaponObject.VRBladeFollowSlot: the lodged blade's material point followed in its animated slot (the goal the
/// hand's attachment drives to; w 3 once it holds), the slide's pulses and the edge's poses recorded.
pub(super) fn follow_slot(w: &Handle, side: i32, serial: u32, a: &V4, b_: &V4, edge: &V4, tracked_grip: &V4) -> V4 {
    let Some(rc) = receipt(w) else { return ZERO };
    let (target, owner, ok) = {
        let r = rc.borrow();
        (r.target(), r.owner(), serial != 0 && serial == r.serial && r.lodged && !r.released)
    };
    if !ok || target.is_null() || !b(&target, "IsAttached") || owner.is_null() || !allowed(&owner, w) || dist(a, &v4_of(w, "GetWorldPosition")) > 0.05 || dist(a, b_) > melee::reach(w) as f32 + 0.02 {
        return ZERO;
    }
    let component = hit_representation(&target);
    {
        let mut r = rc.borrow_mut();
        if r.attachment.as_ref().is_some_and(|p| component.is_null() || p.borrow().slots_ptr != component.instance as usize) {
            r.attachment = None;
        }
    }
    if component.is_null() {
        return ZERO;
    }
    let existing = rc.borrow().attachment.clone();
    let bind = existing.is_none();
    let pin = match existing {
        Some(p) => p,
        None => match bind_slot(&rc, &component, a, b_, edge) {
            Some(p) => sh(p),
            None => return ZERO,
        },
    };
    pin.borrow_mut().depth = 0.0;
    let cp = component.instance.cast::<core::ffi::c_void>();
    let (slot, capsule, fraction, local_point) = {
        let p = pin.borrow();
        (p.slot, p.capsule, p.fraction, p.local_point)
    };
    let Some(frame) = bn::slot_frame(cp, slot) else { return ZERO };
    let (material, joint) = {
        let r = rc.borrow();
        (r.material, r.joint_serial)
    };
    let interval = bn::capsule_result(&to_local(&frame, a), &to_local(&frame, b_), &ZERO, &rotate(&bn::conj(&frame.orientation()), edge), &capsule.0, &capsule.1, material, true);
    if interval[3] > 0.5 {
        pin.borrow_mut().depth = interval[1];
    }
    let station = lerp(a, b_, fraction);
    let animated = to_world(&frame, &local_point);
    if bind && pin.borrow().depth <= 0.0 {
        return ZERO;
    }
    let goal = bn::animated_attachment(side, joint, &p3(&animated), &frame.orientation(), &p3(&station), bind);
    if goal[3] > 0.5 {
        rc.borrow_mut().attachment = Some(pin.clone());
    }
    let depth = pin.borrow().depth;
    if goal[3] > 2.5 && depth > 0.001 && tracked_grip[3] > 0.0 {
        let pulse = pin.borrow_mut().slide_feedback(&to_local(&frame, &station), tracked_grip, real_time());
        if pulse {
            // one borrow: an assignment's right side is evaluated first and its Ref lives to the statement's end, so
            // `st.borrow_mut().x = st.borrow().x + 1` panicked on every pulse of a blade sliding in a body
            let st = weapon(w);
            {
                let mut s = st.borrow_mut();
                s.slide_ticks = s.slide_ticks.wrapping_add(1);
            }
            super::super::belt::native::<()>("VRHapticEvent", &[Value::Int32(16), Value::Int32(if side == 0 { 1 } else { 2 })]);
        }
    } else {
        pin.borrow_mut().slide_tracking = false;
    }
    if goal[3] > 2.5 {
        super::cut::record_edge_pose(&rc, &pin, &frame, a, b_, edge, &goal, side);
    } else {
        pin.borrow_mut().edge_frames.clear();
    }
    goal
}
/// WeaponObject.VRBladeUpdateJoint: the joint between the hand and the person's body found inside the measured shaft
/// interval (three presets, three stations in turn; the person's ragdoll asked for once), else the puppet's own.
pub(super) fn update_joint(w: &Handle, side: i32, serial: u32, a: &V4, b_: &V4, edge: &V4) -> bool {
    let Some(rc) = receipt(w) else { return false };
    let (target, owner, ok) = {
        let r = rc.borrow();
        (r.target(), r.owner(), r.serial == serial && !r.released)
    };
    if !ok || target.is_null() || !b(&target, "IsAttached") || owner.is_null() || !allowed(&owner, w) || dist(a, &v4_of(w, "GetWorldPosition")) > 0.05 || dist(a, b_) > melee::reach(w) as f32 + 0.02 {
        bn::clear_joint(side);
        return false;
    }
    let interval = seat_interval(w, serial, a, b_, edge);
    if interval[3] < 0.5 {
        return false;
    }
    let sq = queries();
    let axis = norm(&sub(b_, a));
    let middle = lerp(a, b_, interval[0]);
    let now = real_time();
    let (index, joint) = {
        let mut r = rc.borrow_mut();
        if r.joint_probe_until == 0.0 {
            r.joint_probe_until = now + 0.15;
        }
        if now >= r.joint_probe_until {
            r.joint_probe_index = (r.joint_probe_index + 1) % 6;
            r.joint_probe_until = now + 0.15;
        }
        (r.joint_probe_index, r.joint_serial)
    };
    let section = index % 3;
    let offset = match section {
        1 => -0.4,
        2 => 0.4,
        _ => 0.0,
    };
    let anchor = with_w(add(&middle, &mul(&axis, offset * interval[1])), 1.0);
    let start = sub(&anchor, &mul(&axis, 0.09));
    let end = add(&anchor, &mul(&axis, 0.09));
    let (requested, spent, puncture, lodged) = {
        let r = rc.borrow();
        (r.requested_bodies, r.spent, r.puncture, r.lodged)
    };
    if !requested && !spent && !b(&target, "IsRagdollEnabled") && b(&target, "CanRagdoll") && special_target(&owner, &target) && !has_effect_tag(&target, "DisableRagdoll") {
        rc.borrow_mut().requested_bodies = true;
        let ev = rt::global_obj(
            "CreateRagdollActivationRequestEvent",
            &[A::N(ev("entragdollActivationRequestType", "Animated")), A::Name(CName::hashed("None").0), A::B(false), A::B(false), A::Name(CName::hashed("VRBladeContact").0)],
        );
        if !ev.is_null() {
            rt::call(&target, "QueueEvent", &[A::H(&ev)]);
        }
    }
    // an asynchronous trace gets a solver step to bind; on alternate windows the owned body's own lookup
    if index % 2 == 0 {
        for preset in ["Melee", "Sight Blocker", "Bullet"] {
            let mut trace = Trace::zeroed();
            if raycast(&sq, &start, &end, preset, true, &mut trace) && trace.entity().instance == target.instance {
                let kind = bn::trace_kind(&trace.0);
                rc.borrow_mut().joint_trace_kind = kind;
                if kind == 12 || (kind == 1 && (puncture || lodged || b(&target, "IsRagdolling"))) {
                    let entry = trace.position();
                    let on_blade = add(&middle, &mul(&axis, dot(&sub(&entry, &middle), &axis).clamp(-0.5 * interval[1], 0.5 * interval[1])));
                    if dist(&entry, &on_blade) <= 0.035 {
                        return bn::submit_joint(side, joint, &trace.0, &p3(&on_blade), &p3(&axis));
                    }
                }
            }
        }
    }
    bn::find_puppet_joint(&target, side, joint, &p3(&anchor), &p3(&axis))
}
