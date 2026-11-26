//! mods/cet/CyberpunkVRPort_Reload/init.lua's onUpdate: every frame the dual wield first (which lets the hand reload
//! go while two guns are out), the plugin's capture and the hand-stop deadband once, the held gun (the active weapon,
//! kept ~30 frames while GetActiveWeapon blinks) and its slot component, the hand holding it (the hand slot the gun is
//! in, else the nearer tracked hand, latched), then the reload's frame (reload_frame.rs and its parts) unless the
//! game's own reload has the gun, the glow on the gun, and the wrist the reload owns this frame published
//! (SetVRReloadOwnedHand). The overlay's magazine tuner stays the Lua host's and reports to VRReloadTune.
use super::belt::{native, player, st_handle};
use super::reload_core::State;
use super::reload_data::{self as data, Lua};
use crate::script_call::Value;
use cvr_host::log;
use red4ext_abi::handle::Handle;
use std::sync::Mutex;

const DEAD: f32 = 0.002;
const HOLDER_DROP: f64 = 0.30;
const HOLDER_EDGE: f64 = 0.05;

struct Reload {
    s: State,
    glow: super::reload_glow::Glow,
    dual: super::reload_dual::Dual,
    cap_on: bool,
    dead_set: bool,
    last_w: Handle,
    /// The weapon is confirmed in a hand this frame (the active weapon, or one in a hand slot).
    held_confirmed: bool,
    slot_comp: Handle,
    slot_ent: Option<u64>,
    holder_side: Option<usize>,
    holder_ent: Option<u64>,
    slot_holder: Option<usize>,
    slot_holder_for: Option<u64>,
    dual_yielded: bool,
    last_owned: Option<usize>,
    log_acc: f64,
    note: String,
    reported: bool,
    /// The held weapon whose animated objects were last published to the rig pass, the time since, and them.
    held_for: usize,
    held_t: f64,
    held_objs: Vec<usize>,
}
// SAFETY: the handles are made, used and dropped on the game's main thread, where the mechanics tick.
unsafe impl Send for Reload {}
static RELOAD: Mutex<Option<Reload>> = Mutex::new(None);

fn new_reload() -> Reload {
    Reload {
        s: State::new(),
        glow: Default::default(),
        dual: Default::default(),
        cap_on: false,
        dead_set: false,
        last_w: Handle::null(),
        held_confirmed: false,
        slot_comp: Handle::null(),
        slot_ent: None,
        holder_side: None,
        holder_ent: None,
        slot_holder: None,
        slot_holder_for: None,
        dual_yielded: false,
        last_owned: None,
        log_acc: 0.0,
        note: "-".into(),
        reported: false,
        held_for: 0,
        held_t: 0.0,
        held_objs: Vec::new(),
    }
}
fn active_weapon(pl: &Handle) -> Handle {
    st_handle("ScriptedPuppet", "GetActiveWeapon", &[Value::Handle(pl)])
}
/// heldWeapon: the active weapon, else the one the real hand slots hold, right first (main's 4b743a124: an entity can
/// answer GetWorldPosition for a second after holstering, which is no evidence its reload highlight belongs in the
/// hand). The Lua kept the last weapon for 30 frames only when the slot query itself failed; here it cannot.
fn held_weapon(r: &mut Reload) -> Handle {
    r.held_confirmed = false;
    let pl = player();
    if pl.is_null() {
        r.last_w = Handle::null();
        return Handle::null();
    }
    let mut w = active_weapon(&pl);
    if w.is_null() {
        w = super::belt::in_slot(&pl, "AttachmentSlots.WeaponRight");
        if w.is_null() {
            w = super::belt::in_slot(&pl, "AttachmentSlots.WeaponLeft");
        }
    }
    (r.last_w, r.held_confirmed) = (w.clone(), !w.is_null());
    w
}
/// readWeapon: the held gun's slot component (the one carrying the first of its capsules' slots); 0 without a gun.
fn read_weapon(r: &mut Reload) -> usize {
    let caps = data::data().g("weapon_capsules").arr();
    if caps.is_empty() || player().is_null() {
        return 0;
    }
    let w = held_weapon(r);
    if w.is_null() {
        (r.slot_comp, r.slot_ent) = (Handle::null(), None);
        return 0;
    }
    if super::reload_core::player_frame().is_none() {
        return 0;
    }
    let key = super::belt::entity_hash(&w);
    if key != r.slot_ent {
        (r.slot_comp, r.slot_ent) = (Handle::null(), key);
    }
    if r.slot_comp.is_null() {
        let first = caps[0].s("slot").unwrap_or("");
        r.slot_comp = super::belt::components(&w)
            .into_iter()
            .find(|c| super::belt::class_name(c).contains("SlotComponent") && super::reload_core::slot_world(c, first).is_some())
            .unwrap_or_else(Handle::null);
        if r.slot_comp.is_null() {
            return 0;
        }
    }
    usize::from(caps.iter().any(|c| c.s("slot").is_some_and(|sl| super::reload_core::slot_world(&r.slot_comp, sl).is_some())))
}
/// holderFromSlot: the hand slot the active weapon is in (right first), cached per weapon.
fn holder_from_slot(r: &mut Reload) -> Option<usize> {
    let pl = player();
    let w = if pl.is_null() { Handle::null() } else { active_weapon(&pl) };
    if w.is_null() {
        (r.slot_holder, r.slot_holder_for) = (None, None);
        return None;
    }
    let wid = super::belt::entity_hash(&w)?;
    if r.slot_holder_for == Some(wid) {
        return r.slot_holder;
    }
    let mut found = None;
    for (side, slot) in [(1, "AttachmentSlots.WeaponRight"), (0, "AttachmentSlots.WeaponLeft")] {
        let it = super::belt::in_slot(&pl, slot);
        if !it.is_null() && super::belt::entity_hash(&it) == Some(wid) {
            found = Some(side);
            break;
        }
    }
    if found.is_some() {
        (r.slot_holder, r.slot_holder_for) = (found, Some(wid));
    }
    found
}
fn holder_of(r: &mut Reload, rl: Option<[f32; 4]>, rr: Option<[f32; 4]>) -> Option<usize> {
    if let Some(s) = holder_from_slot(r) {
        (r.holder_side, r.holder_ent) = (Some(s), r.slot_ent);
        return Some(s);
    }
    if r.slot_ent != r.holder_ent {
        (r.holder_side, r.holder_ent) = (None, r.slot_ent);
    }
    let dist = |v: [f32; 4]| (f64::from(v[0]).powi(2) + f64::from(v[1]).powi(2) + f64::from(v[2]).powi(2)).sqrt();
    let dl = rl.map_or(-1.0, dist);
    let dr = rr.map_or(-1.0, dist);
    if let Some(hs) = r.holder_side {
        let d = if hs == 0 { dl } else { dr };
        if (0.0..=HOLDER_DROP).contains(&d) {
            return Some(hs);
        }
        r.holder_side = None;
    }
    r.holder_side = match (rl, rr) {
        (Some(_), Some(_)) => Some(if dl + HOLDER_EDGE < dr { 0 } else { 1 }),
        (None, Some(_)) => Some(1),
        (Some(_), None) => Some(0),
        _ => None,
    };
    r.holder_side
}
fn publish_owned_hand(r: &mut Reload, hand: Option<usize>) {
    if hand == r.last_owned {
        return;
    }
    r.last_owned = hand;
    native::<()>("SetVRReloadOwnedHand", &[Value::Int32(hand.map_or(-1, |h| h as i32))]);
}
fn update(r: &mut Reload, dt: f64) {
    super::reload_frame::tick_loose_rounds(&mut r.s, dt);
    let dual = super::reload_dual::frame(&mut r.dual, &mut r.s, dt);
    if !r.cap_on && native::<()>("SetVRDiagCapture", &[Value::Int32(1)]).is_some() {
        r.cap_on = true;
    }
    if !r.dead_set && native::<()>("VRHandStopDeadband", &[Value::Float(DEAD)]).is_some() {
        r.dead_set = true;
    }
    let raw = |side: i32| native::<[f32; 4]>("VRHandRawModel", &[Value::Int32(side)]).filter(|p| p[3] >= 0.5);
    let (rl, rr) = (raw(0), raw(1));
    if rl.is_none() && rr.is_none() {
        // no tracked hand: no valid place for a hand-mounted highlight
        super::reload_glow::hide(&mut r.glow);
        return;
    }
    let have_gun = read_weapon(r) > 0;
    let mut holder = None;
    if have_gun {
        holder = holder_of(r, rl, rr);
        if holder.is_some_and(|hs| if hs == 0 { rl.is_none() } else { rr.is_none() }) {
            holder = None;
        }
    }
    let weapon = r.last_w.clone();
    publish_held(r, have_gun, dt);
    if dual {
        if !r.dual_yielded {
            super::reload_frame::release(&mut r.s, &weapon);
        }
        r.dual_yielded = true;
        r.note = "dual wield".into();
    } else {
        r.dual_yielded = false;
        if super::reload_frame::game_reload(&mut r.s, &weapon) {
            r.note = "game reload".into();
        } else {
            let sc = r.slot_comp.clone();
            super::reload_frame::frame(&mut r.s, &weapon, &sc, holder, dt);
            r.note = "-".into();
        }
    }
    let mine = r.held_confirmed && have_gun && !dual && r.note == "-";
    let sc = r.slot_comp.clone();
    if mine {
        let hold = r.s.holder_ok.or(holder);
        super::reload_glow::frame(&mut r.glow, &mut r.s, &weapon, hold, &sc, dt);
    } else {
        super::reload_glow::frame(&mut r.glow, &mut r.s, &Handle::null(), None, &sc, dt);
    }
    let owned = r.s.owned_hand;
    publish_owned_hand(r, owned);
    super::reload_probe::row(&r.s, &r.note, dt);
    if !r.reported {
        r.reported = true;
        log!(
            "[Reload] from Rust (build {}): {} weapon configs, gun {}, holder {:?}, dual {dual} ({})\n",
            super::reload_core::BUILD,
            data::configs().len(),
            if have_gun { "yes" } else { "no" },
            holder,
            r.note
        );
    }
    r.log_acc += dt;
    if r.log_acc >= 1.0 && super::belt::shared(156) > 0.5 {
        r.log_acc = 0.0;
        log!(
            "[Reload] gun={} slots={} holder={:?} own={:?} {} (from Rust)\n",
            if have_gun { "yes" } else { "no" },
            if r.slot_comp.is_null() { "no" } else { "yes" },
            holder,
            r.s.owned_hand,
            r.note
        );
        log!("[Reload] {} {} (from Rust)\n", super::reload_shotgun::diag(&r.s), super::reload_revolver::diag(&r.s));
    }
}
/// The animated objects of the weapons in the hands (the held gun, a dual wield's second one in the other hand slot)
/// published to the rig pass, which then reads and writes only their rigs, not every copy of the same gun (holsters'
/// copies, the other guns, NPCs'): looked up again when the gun changes, then every 3 s (0.5 s while none is found).
fn publish_held(r: &mut Reload, have_gun: bool, dt: f64) {
    r.held_t += dt;
    let w = if have_gun && !r.last_w.is_null() { r.last_w.instance as usize } else { 0 };
    let due = w != r.held_for || r.held_t >= if r.held_objs.is_empty() { 0.5 } else { 3.0 };
    if !due {
        return;
    }
    r.held_t = 0.0;
    r.held_for = w;
    let mut objs = Vec::new();
    // CVR_RIG_HELD_FILTER=0: every copy of the gun read and written as before (an A/B switch)
    if w != 0 && std::env::var("CVR_RIG_HELD_FILTER").as_deref() != Ok("0") {
        let mut ents = vec![w];
        let pl = player();
        for slot in ["AttachmentSlots.WeaponLeft", "AttachmentSlots.WeaponRight"] {
            let e = super::belt::in_slot(&pl, slot);
            if !e.is_null() && !ents.contains(&(e.instance as usize)) {
                ents.push(e.instance as usize);
            }
        }
        for e in ents {
            for o in crate::anim_lookup::animated_objects_for_entity(e) {
                if !objs.contains(&o) {
                    objs.push(o);
                }
            }
        }
        objs.truncate(8);
    }
    if objs != r.held_objs {
        let stats = cvr_rig::vrik::weapon_rig::HELD_STATS.each_ref().map(|c| c.load(std::sync::atomic::Ordering::Relaxed));
        cvr_host::vlog!("[Reload] rig passes for the held gun: {} animated object(s) {:x?} (passes so far: held {}, others skipped {}, unscoped {}) (from Rust)\n", objs.len(), objs, stats[0], stats[1], stats[2]);
    }
    cvr_rig::vrik::weapon_rig::set_held_objects(&objs);
    r.held_objs = objs;
}
/// The mod's onUpdate.
pub(in crate::mechanics) fn tick(dt: f32) {
    let mut guard = RELOAD.lock().unwrap_or_else(|p| p.into_inner());
    let r = guard.get_or_insert_with(new_reload);
    update(r, f64::from(dt));
}
/// The mod's onShutdown.
pub(in crate::mechanics) fn shutdown() {
    let mut guard = RELOAD.lock().unwrap_or_else(|p| p.into_inner());
    let Some(r) = guard.as_mut() else { return };
    super::reload_shotgun::clear_loose_rounds(&mut r.s);
    publish_owned_hand(r, None);
    for side in 0..2 {
        super::reload_core::stop_model(side, false, [0.0; 3]);
        super::reload_core::stop_rot(side, 0, [0.0, 0.0, 0.0, 1.0]);
    }
    super::reload_dual::release(&mut r.dual);
    super::reload_glow::clear(&mut r.glow);
}
/// VRReloadTune: the overlay's magazine tuner (M.tune) as the Lua host's window has it.
#[allow(clippy::too_many_arguments)]
pub(in crate::mechanics) fn set_tune(hold: bool, d: [f32; 3], rot: [f32; 3], stage: f32, seat: f32, lead: f32, rev: i32) {
    let mut guard = RELOAD.lock().unwrap_or_else(|p| p.into_inner());
    let r = guard.get_or_insert_with(new_reload);
    let t = &mut r.s.tune;
    t.hold = hold;
    t.d = d.map(f64::from);
    t.r = rot.map(f64::from);
    t.stage = f64::from(stage);
    t.seat = f64::from(seat);
    t.lead = f64::from(lead);
    t.rev = rev;
}
/// VRReloadTuneJoint: one finger joint's weight (1..19).
pub(in crate::mechanics) fn set_tune_joint(i: i32, w: f32) {
    let mut guard = RELOAD.lock().unwrap_or_else(|p| p.into_inner());
    let r = guard.get_or_insert_with(new_reload);
    if let Some(slot) = usize::try_from(i).ok().and_then(|i| i.checked_sub(1)).and_then(|i| r.s.tune.w.get_mut(i)) {
        *slot = f64::from(w);
    }
}

/// The 19 joints a recorded pose carries, by finger (M.tune.joints: 1-based, the recorder's order).
const JOINTS: [(&str, &[usize], &[&str]); 5] = [
    ("thumb", &[1, 6, 7], &["meta", "base", "tip"]),
    ("index", &[2, 8, 9, 10], &["meta", "base", "mid", "tip"]),
    ("middle", &[3, 11, 12, 13], &["meta", "base", "mid", "tip"]),
    ("ring", &[4, 14, 15, 16], &["meta", "base", "mid", "tip"]),
    ("pinky", &[5, 17, 18, 19], &["meta", "base", "mid", "tip"]),
];
/// The mod's "VR magazine tuner" window (dev_windows.rs): M.tune edited in place.
pub(in crate::mechanics) fn dev_window(frame: [f32; 2]) {
    use super::dev_windows as w;
    let mut guard = RELOAD.lock().unwrap_or_else(|p| p.into_inner());
    let r = guard.get_or_insert_with(new_reload);
    let t = &mut r.s.tune;
    w::window("VR magazine tuner", [0.46, 0.04], [0.24, 0.91], frame, || {
        w::text("Hold a magazine and shape the hand. Print writes the numbers to the log.");
        w::checkbox("hold a magazine (no reload needed)", &mut t.hold);
        w::separator();
        w::text("WHERE IT SITS IN THE HAND  (added to the measured holdOffset, metres)");
        w::slider_f64("x  along the hand", &mut t.d[0], -0.15, 0.15);
        w::slider_f64("y", &mut t.d[1], -0.15, 0.15);
        w::slider_f64("z", &mut t.d[2], -0.15, 0.15);
        w::text("HOW IT IS TURNED  (degrees, on top of the measured rotation)");
        w::slider_f64("pitch", &mut t.r[0], -180.0, 180.0);
        w::slider_f64("yaw", &mut t.r[1], -180.0, 180.0);
        w::slider_f64("roll", &mut t.r[2], -180.0, 180.0);
        w::separator();
        w::text("FINGERS  0 = tracking, 1 = the recorded pose, above 1 curls HARDER than the animation");
        for (name, idx, labels) in JOINTS {
            w::tree(name, || {
                for (k, &j) in idx.iter().enumerate() {
                    if w::slider_f64(&format!("{}##{name}", labels[k]), &mut t.w[j - 1], 0.0, 2.5) {
                        t.rev += 1;
                    }
                }
                let mut all = idx.iter().map(|&j| t.w[j - 1]).sum::<f64>() / idx.len() as f64;
                if w::slider_f64(&format!("ALL##{name}"), &mut all, 0.0, 2.5) {
                    for &j in idx {
                        t.w[j - 1] = all;
                    }
                    t.rev += 1;
                }
            });
        }
        if w::button("every joint 1") {
            t.w = [1.0; 19];
            t.rev += 1;
        }
        w::same_line();
        if w::button("every joint 0") {
            t.w = [0.0; 19];
            t.rev += 1;
        }
        w::separator();
        w::text("POSE STAGE  (-1 follows the magazine, otherwise pins to that depth in metres)");
        w::slider_f64("stage depth", &mut t.stage, -1.0, 0.15);
        w::separator();
        w::text("WELL  (-1 keeps the weapon config, otherwise the fraction of the run at which it catches)");
        w::slider_f64("seat depth", &mut t.seat, -1.0, 1.0);
        w::separator();
        w::text("HELD ENTITY LEAD  (frames of the player's travel; 0 = off)");
        w::slider_f64("lead frames", &mut t.lead, 0.0, 4.0);
        w::separator();
        if w::button("PRINT to the log") {
            log!("[MagTune] holdOffset += {{ {:.5}, {:.5}, {:.5} }}   rot += {{ {:.2}, {:.2}, {:.2} }} deg\n", t.d[0], t.d[1], t.d[2], t.r[0], t.r[1], t.r[2]);
            let w: Vec<String> = t.w.iter().map(|x| format!("{x:.2}")).collect();
            log!("[MagTune] joints = {{ {} }}\n", w.join(", "));
            log!("[MagTune] seatAt = {}\n", if t.seat >= 0.0 { format!("{:.3}", t.seat) } else { "config".into() });
            log!("[MagTune] lead = {:.2} frames\n", t.lead);
        }
        w::same_line();
        if w::button("reset") {
            t.d = [0.0; 3];
            t.r = [0.0; 3];
            t.seat = -1.0;
            t.lead = 2.0;
            t.w = [1.0; 19];
            t.rev += 1;
            t.stage = -1.0;
        }
    });
}

/// A development check (CVR_AMMO_TEST): 44 s into the loaded save the first weapon slot is drawn; at 52 s the
/// magazine's count is read (mag_ammo -- WeaponObject.GetMagazineAmmoCount, a static called on the weapon as CET
/// calls it) and set one lower through setMagAmmo, and 2 s later read again; then filled as a reload fills it (the
/// capacity, at most the reserve) and read again 2 s later.
pub(in crate::mechanics) fn ammo_test(dt: f64) {
    thread_local! {
        static T: std::cell::Cell<(f64, u32, usize, i32)> = const { std::cell::Cell::new((0.0, 0, 0, -1)) };
    }
    if std::env::var_os("CVR_AMMO_TEST").is_none() {
        return;
    }
    let pl = player();
    let (t, step, first, before) = T.with(|c| c.get());
    if pl.is_null() || step >= 4 {
        return;
    }
    let p = pl.instance as usize;
    let first = if first == 0 { p } else { first };
    if p == first || native::<bool>("VRLobbyActive", &[]) == Some(true) {
        T.with(|c| c.set((t, step, first, before)));
        return;
    }
    let t = t + dt;
    let (mut step, mut before) = (step, before);
    if step == 0 && t > 44.0 {
        super::holster::equip_request(&pl, "RequestWeaponSlot1");
        // the weapon records as the weapon grip window lists them (the TweakDB's records by type, no TweakXL)
        let db = red4ext_abi::tweakdb::get();
        let n = match crate::script_call::class_any("gamedataWeaponItem_Record") {
            // SAFETY: the live TweakDB and a record class.
            Some(class) if !db.is_null() => unsafe { red4ext_abi::tweakdb::record_ids_of_type(db, (class as *const red4ext_abi::rtti::Class).cast()) }.len(),
            _ => 0,
        };
        log!("[Reload] ammo test: {n} weapon records in the TweakDB (from Rust)\n");
        step = 1;
    } else if step == 1 && t > 52.0 {
        let weapon = active_weapon(&pl);
        let cur = super::reload_core::mag_ammo(&weapon);
        let mut guard = RELOAD.lock().unwrap_or_else(|p| p.into_inner());
        let r = guard.get_or_insert_with(new_reload);
        let ok = cur.is_some_and(|c| super::reload_core::set_mag_ammo(&mut r.s, &weapon, c - 1));
        log!("[Reload] ammo test: weapon {} count {cur:?}, set one lower: {ok} ({}) (from Rust)\n", !weapon.is_null(), r.s.ammo_err);
        before = cur.unwrap_or(-1);
        step = 2;
    } else if step == 2 && t > 54.0 {
        let weapon = active_weapon(&pl);
        let after = super::reload_core::mag_ammo(&weapon);
        log!("[Reload] ammo test 2 s later: count {after:?} (was {before}) (from Rust)\n");
        // the magazine filled as a reload fills it: the capacity, at most the reserve
        let (cap, reserve) = (super::reload_core::mag_capacity(&weapon), super::reload_core::reserve_ammo(&weapon));
        let mut guard = RELOAD.lock().unwrap_or_else(|p| p.into_inner());
        let r = guard.get_or_insert_with(new_reload);
        let ok = super::reload_core::set_mag_ammo(&mut r.s, &weapon, cap.min(reserve));
        log!("[Reload] ammo test fill: capacity {cap}, reserve {reserve}, set {}: {ok} ({}) (from Rust)\n", cap.min(reserve), r.s.ammo_err);
        step = 3;
    } else if step == 3 && t > 56.0 {
        let after = super::reload_core::mag_ammo(&active_weapon(&pl));
        log!("[Reload] ammo test fill 2 s later: count {after:?} (from Rust)\n");
        step = 4;
    }
    T.with(|c| c.set((t, step, first, before)));
}
