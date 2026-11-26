//! An item held in a hand, a grenade thrown or set off (formerly the PlayerPuppet methods of
//! mods/redscript/CyberpunkVRPort_Hold, called by the belt and the reload; the Rust build leaves the module out of the
//! script compilation).
//!
//! - hold_item: the item given when the player does not carry it, then put in the slot by the transaction system's
//!   AddItemToSlot with highPriority, the rendering plane decided by the attach itself (the weapon plane for what is
//!   held before the gun, the scene plane for a belt element: it outlines there).
//! - drop_item: RemoveItemFromSlot, the item left in the inventory; slot_filled (VRPortHasItem / VRPortSlotFilled):
//!   GetItemInSlot.
//! - throw_grenade_from: the item taken out of its slot and launched by a gameprojectileSetUpEvent (owner and weapon the
//!   player, a parabolic trajectory at the grenade's own gravity and the given speed) and a
//!   gameprojectileSetUpAndLaunchEvent from the hand's slot, oriented along the world direction, the player's velocity
//!   carried; throw_grenade the same from the item's own position.
//! - detonate_grenade_at: launched from a static point at a hair's speed and forced to activate there.
//! - spend_grenade_charge: the throw's clean cost taken from the GrenadesCharges pool.
use super::lobby_rtti::{self as rt, Val, A};
use super::menu_rt::{self as r, Id16};
use red4ext_abi::handle::Handle;

fn ts() -> Handle {
    super::game_system("GetTransactionSystem")
}
fn tdb(name: &str) -> u64 {
    red4ext_abi::tweakdbid::from_name(name)
}
fn item_in_slot(pl: &Handle, slot: &str) -> Handle {
    rt::obj(&ts(), "GetItemInSlot", &[A::H(pl), A::Name(tdb(slot))])
}

/// The item in the slot (given first when not carried); the attach's own answer.
pub(super) fn hold_item(pl: &Handle, item: &str, slot: &str, weapon_plane: bool) -> bool {
    let t = ts();
    let Some(id) = rt::static_call("ItemID", "FromTDBID", &[A::Name(tdb(item))]).map(|o| Id16::of(&Val::of(&o))) else { return false };
    if !rt::static_call("ItemID", "IsValid", &[id.a()]).is_some_and(|o| Val::of(&o).bool()) {
        return false;
    }
    if !r::b(&t, "HasItem", &[A::H(pl), id.a()]) && !r::b(&t, "GiveItem", &[A::H(pl), id.a(), A::N(1.0)]) {
        return false;
    }
    let plane = r::ev("ERenderingPlane", if weapon_plane { "RPl_Weapon" } else { "RPl_Scene" });
    r::b(&t, "AddItemToSlot", &[A::H(pl), A::Name(tdb(slot)), id.a(), A::B(true), A::H(&Handle::null()), A::N(plane), A::B(false), A::B(false)])
}
pub(super) fn drop_item(pl: &Handle, slot: &str) {
    rt::call(&ts(), "RemoveItemFromSlot", &[A::H(pl), A::Name(tdb(slot)), A::B(false)]);
}
pub(super) fn slot_filled(pl: &Handle, slot: &str) -> bool {
    !item_in_slot(pl, slot).is_null()
}
/// A grenade's gravity (BaseGrenade.GetAccelerationZ), else the world's.
fn gravity(item: &Handle) -> f64 {
    let g = if r::is_a(item, "BaseGrenade") { rt::num(item, "GetAccelerationZ", &[]).unwrap_or(0.0) } else { 0.0 };
    if g.abs() > 0.001 {
        g
    } else {
        -9.81
    }
}
fn vector4(x: f64, y: f64, z: f64, w: f64) -> Option<crate::script_call::Owned> {
    let o = r::new_struct("Vector4")?;
    let v = r::val(&o);
    for (f, x) in [("X", x), ("Y", y), ("Z", z), ("W", w)] {
        r::set(&v, f, x);
    }
    Some(o)
}
fn trajectory(item: &Handle, speed: f64) -> Handle {
    let accel = vector4(0.0, 0.0, gravity(item), 0.0);
    let a = accel.as_ref().map_or(A::D, |o| A::P(r::ptr(o)));
    r::so("ParabolicTrajectoryParams", "GetAccelVelParabolicParams", &[a, A::N(speed)])
}
fn orientation(q: &crate::script_call::Owned) -> Handle {
    r::so("IOrientationProvider", "CreateStaticOrientationProvider", &[A::P(r::ptr(q))])
}
/// The launch event with its providers and trajectory, owned by the player.
fn launch_event(pl: &Handle, from: &Handle, q: &crate::script_call::Owned, trajectory: &Handle, velocity: bool) -> Handle {
    let ev = super::lobby_ink::new("gameprojectileSetUpAndLaunchEvent");
    if let Some(lp) = rt::field(&ev, "launchParams") {
        for f in ["logicalPositionProvider", "visualPositionProvider"] {
            r::set_h(&lp, f, from);
        }
        for f in ["logicalOrientationProvider", "visualOrientationProvider"] {
            r::set_h(&lp, f, &orientation(q));
        }
        if velocity {
            r::set_h(&lp, "ownerVelocityProvider", &r::so("MoveComponentVelocityProvider", "CreateMoveComponentVelocityProvider", &[A::H(pl)]));
        }
    }
    super::menu_main::set_handle_prop(&ev, "trajectoryParams", trajectory);
    super::menu_main::set_handle_prop(&ev, "owner", pl);
    ev
}
fn set_up(pl: &Handle, item: &Handle, trajectory: &Handle) {
    let su = super::lobby_ink::new("gameprojectileSetUpEvent");
    super::menu_main::set_handle_prop(&su, "owner", pl);
    super::menu_main::set_handle_prop(&su, "weapon", pl);
    super::menu_main::set_handle_prop(&su, "trajectoryParams", trajectory);
    rt::call(item, "QueueEvent", &[A::H(&su)]);
}
fn direction(dir: [f64; 3]) -> Option<crate::script_call::Owned> {
    if (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt() < 0.001 {
        return None;
    }
    let d = vector4(dir[0], dir[1], dir[2], 0.0)?;
    rt::static_call("Quaternion", "BuildFromDirectionVector", &[A::P(r::ptr(&d))])
}
/// The grenade in `slot` launched from the hand's slot `hand` along the world direction at `speed`.
pub(super) fn throw_grenade_from(pl: &Handle, slot: &str, hand: Option<&str>, dir: [f64; 3], speed: f64) -> bool {
    let item = item_in_slot(pl, slot);
    if item.is_null() {
        return false;
    }
    let Some(q) = direction(dir) else { return false };
    let traj = trajectory(&item, speed);
    drop_item(pl, slot);
    let from = match hand {
        Some(hand) => {
            set_up(pl, &item, &traj);
            r::so("IPositionProvider", "CreateSlotPositionProvider", &[A::H(pl), A::Name(r::cname(hand))])
        }
        None => r::so("IPositionProvider", "CreateEntityPositionProvider", &[A::H(&item)]),
    };
    let ev = launch_event(pl, &from, &q, &trajectory(&item, speed), true);
    rt::call(&item, "QueueEvent", &[A::H(&ev)]);
    true
}
/// The grenade in `slot` set off at the world point `at`.
pub(super) fn detonate_grenade_at(pl: &Handle, slot: &str, at: [f64; 3]) -> bool {
    let item = item_in_slot(pl, slot);
    if item.is_null() {
        return false;
    }
    // WorldPosition: three FixedPoints, 17 fractional bits
    let Some(wp) = r::new_struct("WorldPosition") else { return false };
    // SAFETY: the WorldPosition's three FixedPoint words.
    unsafe {
        let bits = r::ptr(&wp) as *mut i32;
        for (k, v) in at.iter().enumerate() {
            *bits.add(k) = (v * 131072.0) as i32;
        }
    }
    let from = r::so("IPositionProvider", "CreateStaticPositionProvider", &[A::P(r::ptr(&wp))]);
    let Some(down) = r::new_struct("Quaternion") else { return false };
    r::set(&r::val(&down), "r", 1.0);
    let traj = trajectory(&item, 0.01);
    drop_item(pl, slot);
    set_up(pl, &item, &traj);
    let ev = launch_event(pl, &from, &down, &trajectory(&item, 0.01), false);
    rt::call(&item, "QueueEvent", &[A::H(&ev)]);
    rt::call(&item, "QueueEvent", &[A::H(&super::lobby_ink::new("gameprojectileForceActivationEvent"))]);
    true
}
/// One throw's cost from the grenade charges; false when the throw costs nothing.
pub(super) fn spend_grenade_charge(pl: &Handle) -> bool {
    let cost = rt::num(pl, "GetGrenadeThrowCostClean", &[]).unwrap_or(0.0);
    if cost <= 0.0 {
        return false;
    }
    let id = [super::belt::entity_hash(pl).unwrap_or(0), 0u64];
    let pools = super::game_system("GetStatPoolsSystem");
    rt::call(
        &pools,
        "RequestChangingStatPoolValue",
        &[A::P(id.as_ptr().cast()), A::N(r::ev("gamedataStatPoolType", "GrenadesCharges")), A::N(-cost), A::H(pl), A::B(false), A::B(false)],
    );
    true
}


/// A development check (CVR_HOLD_TEST, inherited by the game from the probe session): 45 s into the loaded save (a
/// player other than the first, the lobby down) a frag grenade is put in the left hand's slot by Rust and read back
/// 2 s later; then (=compare, while the module is still compiled) the same by the script's VRPortHoldItemScene; then a
/// Rust hold again thrown from the left hand and the slot read once more.
pub(super) fn self_test(dt: f64) {
    thread_local! {
        static T: std::cell::Cell<(f64, u8, usize)> = const { std::cell::Cell::new((0.0, 0, 0)) };
    }
    let Ok(mode) = std::env::var("CVR_HOLD_TEST") else { return };
    let pl = r::player();
    let (t, step, first) = T.with(|c| c.get());
    if pl.is_null() || step >= 9 {
        return;
    }
    let p = pl.instance as usize;
    let first = if first == 0 { p } else { first };
    if p == first || super::belt::native::<bool>("VRLobbyActive", &[]) == Some(true) {
        T.with(|c| c.set((t, step, first)));
        return;
    }
    let t = t + dt;
    // a prop the hand's slot keeps (the smoking's cigarette in WeaponRight): a frag grenade in WeaponLeft is read back
    // empty for the script's method as well -- the belt first allows its slot in the record (prop_allow_slot)
    const SLOT: &str = "AttachmentSlots.WeaponRight";
    const ITEM: &str = "Items.cigarette_i_stick";
    let compare = mode == "compare";
    let mut next = step;
    match step {
        0 if t > 45.0 => {
            let ok = hold_item(&pl, ITEM, SLOT, false);
            cvr_host::log!("[hold] self-test: Rust hold_item -> {ok}, slot filled at once {} (from Rust)\n", slot_filled(&pl, SLOT));
            next = 1;
        }
        1 if t > 47.0 => {
            cvr_host::log!("[hold] self-test: 2 s after Rust's hold the slot holds {} (from Rust)\n", if slot_filled(&pl, SLOT) { "the prop" } else { "nothing" });
            drop_item(&pl, SLOT);
            next = if compare { 2 } else { 4 };
        }
        2 if t > 48.0 => {
            let it = super::lobby_spawn::Str::new(ITEM);
            let sl = super::lobby_spawn::Str::new(SLOT);
            let ok = super::belt::m::<bool>(&pl, "VRPortHoldItemScene", &[it.v(), sl.v()]);
            cvr_host::log!("[hold] self-test: the script's VRPortHoldItemScene -> {ok:?} (from Rust)\n");
            next = 3;
        }
        3 if t > 50.0 => {
            cvr_host::log!("[hold] self-test: 2 s after the script's hold the slot holds {} (from Rust)\n", if slot_filled(&pl, SLOT) { "the prop" } else { "nothing" });
            drop_item(&pl, SLOT);
            next = 4;
        }
        4 if t > 51.0 => {
            let ok = hold_item(&pl, ITEM, SLOT, false);
            cvr_host::log!("[hold] self-test: Rust hold_item again -> {ok} (from Rust)\n");
            next = 5;
        }
        5 if t > 53.0 => {
            cvr_host::log!("[hold] self-test: 2 s after Rust's second hold the slot holds {} (from Rust)\n", if slot_filled(&pl, SLOT) { "the prop" } else { "nothing" });
            drop_item(&pl, SLOT);
            next = 6;
        }
        6 if t > 55.0 => {
            cvr_host::log!("[hold] self-test done: slot filled {} (from Rust)\n", slot_filled(&pl, SLOT));
            next = 9;
        }
        _ => {}
    }
    T.with(|c| c.set((t, next, first)));
}
