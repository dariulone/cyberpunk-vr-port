//! The game's functions the blade wraps: the damage pipeline (only the traced person and the blade's one shape),
//! the blade's damage rule before ApplyDamage, the dismemberment the stock reaction may choose, the reaction's
//! severity for a physical light stroke, and the holds and receipts let go when a person or the weapon leaves.
use super::*;
use crate::script_hooks::{Call, HookFn, Kind};

fn hit_data(hit: &Handle) -> Handle {
    rt::field(hit, "attackData").map(|v| v.handle()).unwrap_or_else(Handle::null)
}
fn hit_target(hit: &Handle) -> Handle {
    rt::field(hit, "target").map(|v| v.handle()).unwrap_or_else(Handle::null)
}
fn projection(hit: &Handle) -> bool {
    rt::field(hit, "projectionPipeline").is_some_and(|v| v.bool())
}
fn is_melee(data: &Handle) -> bool {
    let t = rt::call(data, "GetAttackType", &[]).and_then(|o| Val::of(&o).num()).unwrap_or(-1.0);
    sb("AttackData", "IsMelee", &[A::N(t)])
}
/// A shape that is a protection layer (DamageSystemHelper.IsProtectionLayer, or HitData_Base's own type).
fn protection(shape: rpg::Shape) -> bool {
    if sb("DamageSystemHelper", "IsProtectionLayer", &[A::P(shape.0.cast_const().cast())]) {
        return true;
    }
    let data = shape.data();
    r::is_a(&data, "HitData_Base") && rt::field(&data, "m_hitShapeType").and_then(|v| v.num()) == Some(ev("HitShape_Type", "ProtectionLayer"))
}

/// DamageSystem.ProcessPipeline: a VR melee attack enters the damage pipeline only for the person it was made for,
/// once; a blade impact only with the native hit shape at the blade (nearest the swept segment, the follow-through's
/// own, the edge cut's proven chord), no protection layer in front of it -- that one shape is all the hit keeps.
fn process_pipeline(call: &mut Call) {
    let _g = lock();
    let hit = call.arg_handle(0);
    let data = hit_data(&hit);
    if data.is_null() || !is_melee(&data) {
        return;
    }
    // CVR_MELEE_TRACE=1: why a VR melee hit left the pipeline
    let tracing = std::env::var("CVR_MELEE_TRACE").is_ok_and(|v| v == "1");
    let why = |what: &str| {
        if tracing {
            cvr_host::log!("[melee-trace] pipeline: {what}
");
        }
    };
    let blade = r::o(&data, "GetAttackDefinition", &[]);
    if blade.is_null() {
        return;
    }
    let impact = attack_impact(&blade);
    let shapes = rpg::hit_shapes(&hit);
    let count = shapes.as_ref().map_or(0, |v| v.len());
    let target = hit_target(&hit);
    if let Some(contact) = attack_target(&blade).filter(|t| *t != 0) {
        if projection(&hit) || target.is_null() || eid(&target) != contact {
            why("not the swing's person (or a projection)");
            call.skip();
            return;
        }
        if impact.is_none() {
            if count == 0 || !claim_contact(&blade) {
                why(&format!("a contact hit with {count} shape(s), claimed before: skipped"));
                call.skip();
            } else {
                why(&format!("a contact hit with {count} shape(s): kept"));
            }
            return;
        }
    }
    let Some(impact_rc) = impact else { return };
    let skip = |call: &mut Call| call.skip();
    let (cancelled, claimed, entry_done, impact_target, parent, edge, a, b_, sweep_from, sweep_to, overlap_prev) = {
        let mut i = impact_rc.borrow_mut();
        i.events += 1;
        i.last_shapes = count as i32;
        let entry_done = i.entry.as_ref().is_some_and(|e| e.borrow().claimed || e.borrow().expired);
        (i.cancelled, i.claimed, entry_done, i.target, i.parent_serial, i.edge_stroke, i.a, i.b, i.sweep_from, i.sweep_to, i.overlap_point)
    };
    if cancelled || claimed || entry_done || target.is_null() || eid(&target) != impact_target || count == 0 {
        why(&format!("a blade impact: cancelled {cancelled}, claimed {claimed}, entry done {entry_done}, {count} shape(s)"));
        return skip(call);
    }
    let w = r::o(&data, "GetWeapon", &[]);
    if parent == 0 {
        let pl = r::o(&data, "GetInstigator", &[]);
        let pl = if r::is_a(&pl, "PlayerPuppet") { pl } else { Handle::null() };
        if w.is_null() || !can_claim_entry(&w, &impact_rc.borrow(), &pl) {
            return skip(call);
        }
    }
    let Some(shapes) = shapes else { return skip(call) };
    let list = rpg::shapes_of(&shapes);
    let mut follow_gap = 0.0;
    if parent != 0 && edge == 0 {
        if w.is_null() || confirmed_serial(&w) != parent {
            return skip(call);
        }
        let overlap = rpg::overlap_point(&w, &a, &b_, &v4_of(&w, "GetWorldForward"));
        if overlap[3] < 0.50 {
            return skip(call);
        }
        follow_gap = dist(&overlap, &overlap_prev);
    }
    let mut nearest = 100_000.0f32;
    let mut index = 0usize;
    let mut chosen: Option<Rc<cut::CutShape>> = None;
    let (candidates, reverse) = {
        let i = impact_rc.borrow();
        (i.cut_candidates.clone(), i.cut_shape.as_ref().is_some_and(|c| c.reverse_query))
    };
    if edge != 0 {
        impact_rc.borrow_mut().cut_proof = [candidates.len() as f32, 0.0, 0.0, 0.0];
    }
    for (i, shape) in list.iter().enumerate() {
        let point = shape.enter();
        let mut gap = dist(&point, &closest(&point, &a, &b_));
        let mut row_cut = None;
        if sweep_from[3] > 0.5 {
            gap = gap.min(dist(&point, &closest(&point, &sweep_from, &sweep_to)));
        }
        if parent != 0 && edge == 0 {
            gap = follow_gap;
        }
        if edge != 0 {
            if w.is_null() {
                return skip(call);
            }
            gap = 100_000.0;
            let mut proof = [0f32; 2];
            for candidate in &candidates {
                if (candidate.either_end || candidate.reverse_query == reverse) && candidate.name == shape.name() {
                    proof[0] += 1.0;
                    let distance = rpg::cut_gap(&w, candidate, *shape);
                    if distance < gap {
                        gap = distance;
                        row_cut = Some(candidate.clone());
                    }
                }
            }
            if gap < 100_000.0 {
                proof[1] += 1.0;
            }
            let mut im = impact_rc.borrow_mut();
            im.cut_proof[1] += proof[0];
            im.cut_proof[2] += proof[1];
        }
        // a sustained contact keeps its animated slot: a nearer neighbour must not mask the original shape
        if gap < nearest && (parent == 0 || (rpg::valid_zone(*shape) && rpg::follow_matches(&hit, &impact_rc, *shape))) {
            nearest = gap;
            index = i;
            if edge != 0 && gap <= 0.06 {
                // the first proven surface in stock order keeps every protection layer before it
                chosen = row_cut;
                impact_rc.borrow_mut().cut_proof[3] = 1.0;
                break;
            }
        }
    }
    impact_rc.borrow_mut().last_distance = nearest;
    // a widened sweep (a mech's boxes) meets its shape up to the widening away from the blade
    let reach = if impact_rc.borrow().wide { 0.06 + super::melee::WIDE_HALF as f32 } else { 0.06 };
    if nearest > reach {
        why(&format!("a blade impact: the nearest of {} shape(s) {nearest:.3} m from the blade", list.len()));
        return skip(call);
    }
    let selected = list[index];
    if !r::is_a(&target, "NPCPuppet") || projection(&hit) {
        return skip(call);
    }
    // an intervening protection layer is never dropped when the sweep is reduced to one local hit
    if list[..=index].iter().any(|s| protection(*s)) {
        why("a blade impact: a protection layer in front");
        return skip(call);
    }
    if !rpg::valid_zone(selected) {
        why("a blade impact: not a valid zone");
        return skip(call);
    }
    why(&format!("a blade impact on {}: kept", CName(selected.name()).text()));
    if parent != 0 {
        if !rpg::follow_matches(&hit, &impact_rc, selected) {
            return skip(call);
        }
        if edge != 0 {
            let rc = receipt(&w);
            let frame = match (&chosen, &rc) {
                (Some(c), Some(rc)) => bn::slot_frame(rc.borrow().slots_ptr as *mut core::ffi::c_void, c.slot),
                _ => None,
            };
            let (Some(c), Some(frame)) = (chosen, frame) else { return skip(call) };
            let mut i = impact_rc.borrow_mut();
            cut::use_cut(&mut i, &c);
            i.cut_slot = c.slot;
            i.cut_point = with_w(c.to, 1.0);
            i.overlap_point = to_world(&frame, &i.cut_point);
        }
        if let Some(rc) = receipt(&w) {
            rc.borrow_mut().spent = true;
        }
    }
    let name = selected.name();
    rpg::keep_only(&shapes, index);
    let mut i = impact_rc.borrow_mut();
    i.accepted_shape = name;
    i.claimed = true; // the shape reserved: not yet a hit, nor a combo step
    if let Some(e) = &i.entry {
        e.borrow_mut().claimed = true;
    }
}
/// DamageSystem.DealDamages (before): the blade's rule adjusts only the exact contact about to enter ApplyDamage.
fn deal_damages(call: &mut Call) {
    let _g = lock();
    let hit = call.arg_handle(0);
    let Some(ticket) = melee::ticket_view(&hit) else { return };
    let data = hit_data(&hit);
    let computed = rt::field(&hit, "attackComputed").map(|v| v.handle()).unwrap_or_else(Handle::null);
    if ticket.confirmed || projection(&hit) || data.is_null() || computed.is_null() {
        return;
    }
    let blade = r::o(&data, "GetAttackDefinition", &[]);
    if blade.is_null() || !same_attack(&blade, &ticket.attack) {
        return;
    }
    let Some(impact) = attack_impact(&blade) else { return };
    if !impact.borrow().claimed || impact.borrow().cancelled {
        return;
    }
    let pl = r::o(&data, "GetInstigator", &[]);
    let npc = hit_target(&hit);
    let w = r::o(&data, "GetWeapon", &[]);
    if !r::is_a(&pl, "PlayerPuppet") || !r::is_a(&npc, "NPCPuppet") || w.is_null() {
        return;
    }
    let shapes = rpg::hit_shapes(&hit).map(|v| rpg::shapes_of(&v)).unwrap_or_default();
    let client = {
        let info = r::static_gi("GameInstance", "GetRuntimeInfo", &[]).map(|o| Val::of(&o).handle()).unwrap_or_else(Handle::null);
        b(&info, "IsClient")
    };
    if melee::same_item(&melee::item_id(&w), &ticket.weapon) && eid(&npc) == ticket.target && ticket.target == impact.borrow().target && !client && shapes.len() == 1 && rpg::valid_zone(shapes[0]) {
        rpg::apply_rule(&hit, &impact, &pl, &npc, &w);
    }
}
/// HitReactionComponent's attack's blade impact (VRLocalBladeImpact).
fn local_impact(hrc: &Handle) -> Option<Sh<Impact>> {
    let data = rt::field(hrc, "m_attackData").map(|v| v.handle()).unwrap_or_else(Handle::null);
    if data.is_null() {
        return None;
    }
    let attack = r::o(&data, "GetAttackDefinition", &[]);
    if attack.is_null() {
        None
    } else {
        attack_impact(&attack)
    }
}
fn reaction_shape(hrc: &Handle) -> u64 {
    rt::field(hrc, "m_hitShapeData").map_or(0, |v| rpg::Shape(v.ptr.cast()).name())
}
fn owner_id(hrc: &Handle) -> u64 {
    eid(&r::o(hrc, "GetOwner", &[]))
}
/// HitReactionComponent.DismembermentConditions: a small or unmarked blade never; a paid follow-through on its own
/// shape with Strength 12 always (the stock reaction picks its part and effects); else the stock answer.
fn dismemberment(call: &mut Call) {
    let _g = lock();
    let hrc = call.this();
    let Some(impact) = local_impact(&hrc) else { return };
    let i = impact.borrow();
    if i.no_dismember {
        call.set_result_bool(false);
        return;
    }
    if i.parent_serial != 0 {
        let ok = i.strength_reaction && i.published && i.target == owner_id(&hrc) && i.accepted_shape != 0 && reaction_shape(&hrc) == i.accepted_shape;
        call.set_result_bool(ok);
    }
}
/// HitReactionComponent.WoundedCyberConditions: a blade's hit detaches no cybernetic part unless dismembering.
fn wounded_cyber(call: &mut Call) {
    let _g = lock();
    let Some(impact) = local_impact(&call.this()) else { return };
    let i = impact.borrow();
    if i.no_dismember || (i.parent_serial != 0 && !call.arg_bool(0)) {
        call.set_result_bool(false);
    }
}
/// HitReactionComponent.IsValidBodyPerkDismemberAttack: never for a traced blade (the perk picks random parts).
fn body_perk(call: &mut Call) {
    let _g = lock();
    if local_impact(&call.this()).is_some() {
        call.set_result_bool(false);
    }
}
/// HitReactionComponent.NotifyAboutDismembermentInstigated (before): the follow-through's part noted.
fn dismember_noted(call: &mut Call) {
    let _g = lock();
    let hrc = call.this();
    let Some(impact) = local_impact(&hrc) else { return };
    let owner = owner_id(&hrc);
    let shape = reaction_shape(&hrc);
    let mut i = impact.borrow_mut();
    if i.parent_serial != 0 && i.published && i.target == owner && shape == i.accepted_shape {
        i.part_notified = true;
    }
}
/// VrPhysicalMeleeReaction.SeverityCap: a confirmed controller hit's highest reaction -- a seated blade's twitch (0),
/// an ordinary light combo stroke's stock impact (1); -1 to leave the stock selection alone.
fn severity_cap(hit: &Handle, npc: &Handle) -> i32 {
    if hit.is_null() || npc.is_null() || hit_target(hit).instance != npc.instance || projection(hit) {
        return -1;
    }
    let Some(ticket) = melee::ticket_view(hit) else { return -1 };
    let data = hit_data(hit);
    if !ticket.confirmed || data.is_null() || !is_melee(&data) {
        return -1;
    }
    let attack = r::o(&data, "GetAttackDefinition", &[]);
    let w = r::o(&data, "GetWeapon", &[]);
    let flag = |f: &str| rt::call(&data, "HasFlag", &[A::N(ev("hitFlag", f))]).is_some_and(|o| Val::of(&o).bool());
    if attack.is_null()
        || !same_attack(&attack, &ticket.attack)
        || ticket.target != eid(npc)
        || attack_target(&attack) != Some(ticket.target)
        || w.is_null()
        || !melee::same_item(&melee::item_id(&w), &ticket.weapon)
        || !r::is_a(&r::o(&data, "GetInstigator", &[]), "PlayerPuppet")
        || rpg::rejected(&data)
        || flag("ForceKnockdown")
        || flag("HighSpeedMelee")
        || flag("StunApplied")
        || !b(npc, "IsActive")
        || b(npc, "IsAboutToBeDefeated")
        || b(npc, "IsIncapacitated")
        || b(npc, "IsRagdolling")
    {
        return -1;
    }
    // a retained entry or its lodged follow-through: the graph's brief twitch
    if seated(hit) {
        return 0;
    }
    let t = rt::call(&data, "GetAttackType", &[]).and_then(|o| Val::of(&o).num()).unwrap_or(-1.0);
    if !attack_light_chain(&attack) || !sb("AttackData", "IsLightMelee", &[A::N(t)]) {
        return -1;
    }
    let record = r::o(&attack, "GetRecord", &[]);
    let subtype = r::o(&record, "AttackSubtype", &[]);
    if !r::is_a(&record, "Attack_Melee_Record") || subtype.is_null() || rt::num(&subtype, "Type", &[]) != Some(ev("gamedataAttackSubtype", "ComboAttack")) {
        return -1;
    }
    1
}
/// HitReactionComponent.GetReactionType: the severity capped before the stock selector records its winner (a
/// rewrite afterwards would leave a phantom stagger); an impact or twitch is a light hit.
fn reaction_type(call: &mut Call) {
    let _g = lock();
    let hrc = call.this();
    let hit = call.arg_handle(1);
    let npc = rt::field(&hrc, "m_ownerNPC").map(|v| v.handle()).unwrap_or_else(Handle::null);
    let cap = severity_cap(&hit, &npc);
    let alive = rt::field(&hrc, "m_isAlive").is_some_and(|v| v.bool());
    if cap < 0 || !alive {
        return;
    }
    let data = hit_data(&hit);
    let maximum = rt::num(&data, "GetHitReactionSeverityMax", &[]).unwrap_or(-1.0) as i32;
    let capped = if maximum < 0 { cap } else { maximum.min(cap) };
    rt::call(&data, "SetHitReactionSeverityMax", &[A::N(f64::from(capped))]);
    call.original();
    rt::call(&data, "SetHitReactionSeverityMax", &[A::N(f64::from(maximum))]);
    let reaction = call.result().and_then(|(p, ty)| Val { ptr: p, ty }.num()).unwrap_or(-1.0);
    if reaction == ev("animHitReactionType", "Impact") || reaction == ev("animHitReactionType", "Twitch") {
        r::hset(&hrc, "m_hitIntensity", ev("EAIHitIntensity", "Light"));
    }
}
/// NPCPuppet.OnIncapacitated / OnRagdollEnabledEvent (before): the hold cleared.
fn npc_hold_off(call: &mut Call) {
    let _g = lock();
    hold::clear(&call.this());
}
/// NPCPuppet.OnDetach (before).
fn npc_detach(call: &mut Call) {
    let _g = lock();
    hold::npc_detached(&call.this());
}
/// WeaponObject.OnDetach (before): the receipt's contact released.
fn weapon_detach(call: &mut Call) {
    let _g = lock();
    weapon_detached(&call.this());
}

/// The blade combat self-test's look at a blade impact's hit after the pipeline (CVR_BLADE_COMBAT_TEST): its flags'
/// sources, its computed health damage, the shapes it kept.
fn pipeline_seen(call: &mut Call) {
    let _g = lock();
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ON.get_or_init(|| std::env::var("CVR_BLADE_COMBAT_TEST").is_ok_and(|v| v == "1")) {
        return;
    }
    let hit = call.arg_handle(0);
    let data = hit_data(&hit);
    let blade = r::o(&data, "GetAttackDefinition", &[]);
    let Some(impact) = (!blade.is_null()).then(|| attack_impact(&blade)).flatten() else {
        static SAID: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
        let instigator = r::o(&data, "GetInstigator", &[]);
        if r::is_a(&instigator, "PlayerPuppet") && SAID.fetch_add(1, core::sync::atomic::Ordering::Relaxed) < 4 {
            let now = sim_time();
            let noted: Vec<String> = store(|s| s.attacks.iter().map(|(k, a)| format!("{k:#x} record {:#x} {:.2} s ago", a.record, now - a.at)).collect());
            cvr_host::log!("[blade-combat] the pipeline's attack's record {:#x}\n", record_of(&blade));
            let class = if blade.is_null() { String::from("none") } else { super::super::belt::class_name(&blade) };
            cvr_host::log!("[blade-combat] the pipeline's player hit: its attack {:#x} ({class}) is not one noted ({})\n", blade.instance as usize, noted.join(" "));
        }
        return;
    };
    let computed = rt::field(&hit, "attackComputed").map(|v| v.handle()).unwrap_or_else(Handle::null);
    let health = rt::num(&computed, "GetTotalAttackValue", &[A::N(ev("gamedataStatPoolType", "Health"))]).unwrap_or(-1.0);
    let mut flags = Vec::new();
    if let Some(list) = rt::field(&data, "flags") {
        for i in 0..list.len() {
            if let Some(e) = list.at(i) {
                let f = e.f("flag").and_then(|v| v.num()).unwrap_or(-1.0);
                let source = e.f("source").map(|v| v.text()).unwrap_or_default();
                flags.push(format!("{f}:{source}"));
            }
        }
    }
    let shapes = rpg::hit_shapes(&hit).map_or(0, |v| v.len());
    let i = impact.borrow();
    cvr_host::log!(
        "[blade-combat] the pipeline's hit: health damage {health:.1}, flags [{}], {shapes} shapes; the impact claimed {} cancelled {} events {} nearest {:.3}\n",
        flags.join(" "),
        i.claimed,
        i.cancelled,
        i.events,
        i.last_distance
    );
}

pub(in crate::mechanics) const HOOKS: [(&str, &str, Kind, HookFn); 12] = [
    ("DamageSystem", "ProcessPipeline", Kind::Override, process_pipeline),
    ("DamageSystem", "ProcessPipeline", Kind::After, pipeline_seen),
    ("DamageSystem", "DealDamages", Kind::Before, deal_damages),
    ("HitReactionComponent", "DismembermentConditions", Kind::Override, dismemberment),
    ("HitReactionComponent", "WoundedCyberConditions", Kind::Override, wounded_cyber),
    ("HitReactionComponent", "IsValidBodyPerkDismemberAttack", Kind::Override, body_perk),
    ("HitReactionComponent", "NotifyAboutDismembermentInstigated", Kind::Before, dismember_noted),
    ("HitReactionComponent", "GetReactionType", Kind::Override, reaction_type),
    ("NPCPuppet", "OnIncapacitated", Kind::Before, npc_hold_off),
    ("NPCPuppet", "OnRagdollEnabledEvent", Kind::Before, npc_hold_off),
    ("NPCPuppet", "OnDetach", Kind::Before, npc_detach),
    ("WeaponObject", "OnDetach", Kind::Before, weapon_detach),
];
