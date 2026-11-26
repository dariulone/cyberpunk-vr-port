//! vrport_npc_melee_reaction.reds' VrBladeMovementHold: a person a blade is lodged in holds their position (an
//! AIHoldPositionCommand of their own controller) while the seat lasts; checked each half second (the script's delay
//! callback), a seat re-measured after a long frame may take it again once.
use super::*;

pub(super) struct Hold {
    weapon: WeakHandle,
    weapon_id: u64,
    joint_serial: u32,
    command: Handle,
    next_check: f64,
}
fn ai(npc: &Handle) -> Handle {
    rt::obj(npc, "GetAIControllerComponent", &[])
}
/// WeaponObject.VRBeginBladeMovementHold.
pub(super) fn begin(w: &Handle, rc: &Sh<Receipt>) {
    let (joint, target, owner) = {
        let r = rc.borrow();
        (r.joint_serial, r.target(), r.owner())
    };
    if joint == 0 || target.is_null() || owner.is_null() || !b(&target, "IsAttached") || !rpg::special_target(&owner, &target) {
        return;
    }
    let controller = ai(&target);
    if controller.is_null() {
        return;
    }
    clear(&target);
    // a scripted movement order keeps its actor
    if !rt::obj(&controller, "GetEnqueuedOrExecutingCommand", &[A::Name(CName::hashed("AIMoveCommand").0), A::B(true)]).is_null() {
        return;
    }
    let command = super::super::lobby_ink::new("AIHoldPositionCommand");
    if command.is_null() {
        return;
    }
    r::hset(&command, "duration", -1.0);
    if !rt::call(&controller, "SendCommand", &[A::H(&command)]).is_some_and(|o| Val::of(&o).bool()) {
        return;
    }
    let id = eid(&target);
    let hold = Hold { weapon: WeakHandle::from_handle(w), weapon_id: eid(w), joint_serial: joint, command, next_check: sim_time() + 0.50 };
    store(|s| s.npcs.entry(id).or_default().hold = Some(hold));
}
/// WeaponObject.VREndBladeMovementHold: this weapon's hold on the receipt's person ended.
pub(super) fn end(w: &Handle, rc: &Sh<Receipt>) {
    let (target, joint) = {
        let r = rc.borrow();
        (r.target(), r.joint_serial)
    };
    if target.is_null() {
        return;
    }
    let (id, wid) = (eid(&target), eid(w));
    let mine = store(|s| s.npcs.get(&id).and_then(|n| n.hold.as_ref()).is_some_and(|h| h.weapon_id == wid && h.joint_serial == joint));
    if mine {
        clear(&target);
    }
}
/// NPCPuppet.VRClearBladeMovementHold: the command stopped or cancelled.
pub(super) fn clear(npc: &Handle) {
    let id = eid(npc);
    let Some(hold) = store(|s| s.npcs.get_mut(&id).and_then(|n| n.hold.take())) else { return };
    let controller = ai(npc);
    if controller.is_null() {
        return;
    }
    let state = rt::field(&hold.command, "state").and_then(|v| v.num()).unwrap_or(-1.0);
    if state == ev("AICommandState", "Executing") {
        rt::call(&controller, "StopExecutingCommand", &[A::H(&hold.command), A::B(true)]);
    } else if state == ev("AICommandState", "Enqueued") {
        rt::call(&controller, "CancelCommand", &[A::H(&hold.command)]);
    }
}
/// The holds due their check (VrBladeMovementHold.Call): kept while the seat is fresh and lodged; past half a second
/// a later valid lodge sample may take it again.
pub(super) fn tick() {
    let now = sim_time();
    let due: Vec<(u64, WeakHandle, u32)> = store(|s| s.npcs.iter().filter_map(|(id, n)| n.hold.as_ref().filter(|h| now >= h.next_check).map(|h| (*id, h.weapon.clone(), h.joint_serial))).collect());
    for (id, weapon, joint) in due {
        let w = weapon.lock();
        let rc = if w.is_null() { None } else { receipt(&w) };
        let target = rc.as_ref().map(|r| r.borrow().target()).unwrap_or_else(Handle::null);
        let mut keep = false;
        if let Some(rc) = &rc {
            let (valid, owner, lodged_at) = {
                let r = rc.borrow();
                (r.joint_serial == joint && r.target_id == id && r.lodged && !r.released, r.owner(), r.lodged_at)
            };
            if valid && !target.is_null() && !owner.is_null() && allowed(&owner, &w) && b(&target, "IsActive") && !b(&target, "IsRagdolling") && !b(&target, "IsIncapacitated") {
                if (0.0..0.50).contains(&(real_time() - lodged_at)) {
                    keep = true;
                } else {
                    // a long frame may re-measure this seat: its next valid lodge sample takes the hold again once
                    weapon_resume(&w, joint);
                }
            }
        }
        if keep {
            store(|s| {
                if let Some(h) = s.npcs.get_mut(&id).and_then(|n| n.hold.as_mut()) {
                    h.next_check = now + 0.50;
                }
            });
            continue;
        }
        // the person: from the receipt, else the hold is dropped where it was kept
        if !target.is_null() {
            clear(&target);
        } else {
            store(|s| {
                if let Some(n) = s.npcs.get_mut(&id) {
                    n.hold = None;
                }
            });
        }
    }
}
fn weapon_resume(w: &Handle, joint: u32) {
    weapon(w).borrow_mut().resume_hold_joint = joint;
}
/// NPCPuppet.OnDetach (before): the weapon whose seat this person held lets go; the hold cleared.
pub(super) fn npc_detached(npc: &Handle) {
    let id = eid(npc);
    let hold = store(|s| s.npcs.get(&id).and_then(|n| n.hold.as_ref().map(|h| (h.weapon.clone(), h.joint_serial))));
    if let Some((weapon, joint)) = hold {
        let w = weapon.lock();
        if let Some(rc) = (!w.is_null()).then(|| receipt(&w)).flatten() {
            let (serial, same) = {
                let r = rc.borrow();
                (r.serial, r.joint_serial == joint && r.target_id == id)
            };
            if same {
                release_contact(&w, serial);
            }
        }
    }
    clear(npc);
    store(|s| s.npcs.remove(&id));
}
