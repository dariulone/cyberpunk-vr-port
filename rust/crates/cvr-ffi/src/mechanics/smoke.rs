//! mods/redscript/CyberpunkVRPort_Smoking, the gesture half (the input bridge is mechanics.rs's `smoking`): a vanilla
//! cigarette (Items.cigarette_i_stick) in one hand and a lighter (Items.apparel_lighter_a) in the other -- left-handed
//! the other way round -- each hand's grip curled around its prop by the plugin's poses.
//!
//! The lighter's trigger strikes it (spark, then the held flame); the flame held on the cigarette's tip (6.5 cm along
//! its forward) within 8 cm for 1.5 s lights it, and the lighter goes. A grip at the lips (the plugin's hand-to-mouth
//! distance under 0.22 m, each grip by its own hand) puts the cigarette into the mouth -- Splinter, the WeaponRight1
//! leaf the plugin pins to the lips, both hands free -- or takes it out into that hand when it is free. Lit, the hand
//! at the mouth draws (ember, the inhale) and leaving it exhales (smoke at the mouth, the exhale); in the mouth it puffs
//! every 5 s by itself, the exhale 1.5 s after each draw. It burns 0.005 a second, three times that at the mouth, and
//! goes out at 0.4 of its length. A weapon, a gadget or a consumable taken into a hand ends it (only the lighter is put
//! away while the cigarette is in the mouth).
//!
//! The PlayerPuppet fields of the script are this state, kept per player; its delay callbacks (VRSmokePuffCallback,
//! VRSmokeExhaleCallback) are timers of the tick. Its console's slot experiments (VRSmokeTestSlot and the pins) are
//! not carried over.
use super::belt::{h, m, native, st, tdb, transactions};
use super::lobby_rtti::{self as rt, A};
use crate::script_call::Value;
use cvr_host::log;
use red4ext_abi::cname::CName;
use red4ext_abi::handle::Handle;
use std::sync::Mutex;

const CIG: &str = "Items.cigarette_i_stick";
const LIGHTER: &str = "Items.apparel_lighter_a";
const RIGHT: &str = "AttachmentSlots.WeaponRight";
const LEFT: &str = "AttachmentSlots.WeaponLeft";
const MOUTH: &str = "AttachmentSlots.Splinter";
/// The test slot the script's console could move the cigarette to (its default, AttachmentSlots.Head), cleared too.
const HEAD: &str = "AttachmentSlots.Head";
const PUFF_PERIOD: f64 = 5.0;
const EXHALE_AFTER: f64 = 1.5;

#[derive(Clone, Copy, PartialEq)]
enum Timer {
    Puff,
    Exhale,
}

#[derive(Default)]
struct Smoke {
    owner: usize,
    has: bool,
    lit: bool,
    mouth: bool,
    /// 1.0 full .. 0.0 spent.
    len: f64,
    /// Guards stale puffs and exhales.
    token: i32,
    igniting: bool,
    was_mouth: bool,
    grip_prev: [bool; 2],
    light_progress: f64,
    cig_base: f64,
    cig_base_known: bool,
    light_cd: f64,
    stick_cd: f64,
    last_light: f64,
    last_mouth: f64,
    last_mouth_l: f64,
    cig_id: [u64; 2],
    left_handed: bool,
    /// Where the cigarette is: 0 WeaponRight, 1 Splinter (the mouth), 2 WeaponLeft.
    cig_slot: i32,
    timers: Vec<(f64, Timer, i32)>,
}
// SAFETY: the state is used on the game's main thread only (the mechanics' tick, the script hooks).
unsafe impl Send for Smoke {}
static SMOKE: Mutex<Option<Smoke>> = Mutex::new(None);

fn with<T>(pl: &Handle, f: impl FnOnce(&mut Smoke) -> T) -> T {
    let mut g = SMOKE.lock().unwrap_or_else(|p| p.into_inner());
    let owner = pl.instance as usize;
    if g.as_ref().is_none_or(|s| s.owner != owner) {
        *g = Some(Smoke { owner, len: 1.0, last_light: 999.0, ..Smoke::default() });
    }
    f(g.as_mut().expect("the smoke's state"))
}
fn set(native_name: &str, on: bool) {
    native::<i32>(native_name, &[Value::Int32(i32::from(on))]);
}
fn cig_slot_name(s: i32) -> &'static str {
    match s {
        1 => MOUTH,
        2 => LEFT,
        _ => RIGHT,
    }
}
fn item_in(pl: &Handle, slot: &str) -> Handle {
    h(&transactions(), "GetItemInSlot", &[Value::Handle(pl), tdb(slot)])
}
/// VRSmokeRemoveProp (main's 0.2.3): ONLY THE SMOKING PROPS COME OFF A SLOT. A cigarette in the mouth leaves both hands
/// free, and a weapon drawn meanwhile sits in WeaponRight: burnt out, the old removal took WeaponRight / WeaponLeft off
/// with the TransactionSystem -- the weapon with them, behind the EquipmentSystem's back, which still had it active, so
/// no draw, holster or swap went through again. Every removal here takes the item off only when it is the cigarette or
/// the lighter.
fn remove(pl: &Handle, slot: &str, from_inventory: bool) {
    let item = item_in(pl, slot);
    if item.is_null() {
        return;
    }
    let id = rt::call(&item, "GetItemID", &[]).map(|o| {
        // SAFETY: an ItemID's leading TweakDBID.
        unsafe { *(o.as_ptr() as *const u64) }
    });
    let prop = [CIG, LIGHTER].iter().any(|n| id == Some(red4ext_abi::tweakdbid::from_name(n)));
    if prop {
        let _ = m::<bool>(&transactions(), "RemoveItemFromSlot", &[Value::Handle(pl), tdb(slot), Value::Bool(from_inventory)]);
    }
}
fn add(pl: &Handle, slot: &str, id: &[u64; 2]) -> bool {
    m::<bool>(&transactions(), "AddItemToSlot", &[Value::Handle(pl), tdb(slot), Value::Ptr(id.as_ptr().cast()), Value::Bool(true)]).unwrap_or(false)
}
/// AddItemToSlot(.., true, null, RPl_Scene, false, true): the placement slots ignored (Splinter is none of the cig's).
fn add_anywhere(pl: &Handle, slot: &str, id: &[u64; 2]) -> bool {
    let null = Handle::null();
    let plane = rt::enum_value("ERenderingPlane", "RPl_Scene");
    rt::call(
        &transactions(),
        "AddItemToSlot",
        &[A::H(pl), A::Name(red4ext_abi::tweakdbid::from_name(slot)), A::P(id.as_ptr().cast()), A::B(true), A::H(&null), A::N(plane), A::B(false), A::B(true)],
    )
    .is_some_and(|o| rt::Val::of(&o).bool())
}
fn effect(obj: &Handle, name: &str, start: bool, persist: bool) {
    let n = A::Name(CName::hashed(name).0);
    if start {
        rt::static_call("GameObjectEffectHelper", "StartEffectEvent", &[A::H(obj), n, A::B(persist)]);
    } else {
        rt::static_call("GameObjectEffectHelper", "StopEffectEvent", &[A::H(obj), n]);
    }
}
fn sound(obj: &Handle, name: &str) {
    rt::static_call("GameObject", "PlaySound", &[A::H(obj), A::Name(CName::hashed(name).0)]);
}
fn world_pos(o: &Handle) -> Option<[f64; 3]> {
    m::<[f32; 4]>(o, "GetWorldPosition", &[]).ok().map(super::belt::f3)
}

impl Smoke {
    fn cig(&self, pl: &Handle) -> Handle {
        item_in(pl, cig_slot_name(self.cig_slot))
    }
    fn lighter_slot(&self) -> &'static str {
        if self.left_handed {
            RIGHT
        } else {
            LEFT
        }
    }
    fn lighter(&self, pl: &Handle) -> Handle {
        item_in(pl, self.lighter_slot())
    }
    /// The lighter left the hand holding it: that hand lets go.
    fn release_lighter_hand(&self) {
        if self.left_handed {
            set("SetVRSmokeFingers", false);
            set("SetVRSmokeRightLighter", false);
        } else {
            set("SetVRSmokeFingersL", false);
        }
    }
    fn spawn(&mut self, pl: &Handle) {
        let ts = transactions();
        let Some(cig) = st::<[u64; 2]>("ItemID", "FromTDBID", &[tdb(CIG)]) else { return };
        let Some(lt) = st::<[u64; 2]>("ItemID", "FromTDBID", &[tdb(LIGHTER)]) else { return };
        self.cig_id = cig;
        self.left_handed = native::<bool>("VRLeftHanded", &[]).unwrap_or(false);
        // it starts in the hand that does not hold the lighter
        self.cig_slot = if self.left_handed { 2 } else { 0 };
        rt::call(&ts, "GiveItem", &[A::H(pl), A::P(cig.as_ptr().cast()), A::N(1.0)]);
        add(pl, cig_slot_name(self.cig_slot), &cig);
        rt::call(&ts, "GiveItem", &[A::H(pl), A::P(lt.as_ptr().cast()), A::N(1.0)]);
        add(pl, self.lighter_slot(), &lt);
        self.has = true;
        self.lit = false;
        self.mouth = false;
        self.len = 1.0;
        // the authored full length, forced: a re-spawned mesh can still carry the last burned-down scale
        self.cig_base = 1.3;
        self.cig_base_known = true;
        set("SetVRSmokeRightLighter", self.left_handed);
        set("SetVRSmokeLeftCig", self.left_handed);
        set("SetVRSmokeFingers", true);
        set("SetVRSmokeFingersL", true);
        log!("[Smoke] cigarette and lighter in hand, left-handed {} (from Rust)\n", self.left_handed);
    }
    fn extinguish(&mut self, pl: &Handle) {
        self.token += 1;
        let cig = self.cig(pl);
        if !cig.is_null() {
            effect(&cig, "smoke_trail", false, false);
            effect(&cig, "ember_passive", false, false);
        }
        for slot in [RIGHT, LEFT, MOUTH, HEAD] {
            remove(pl, slot, true);
        }
        self.cig_slot = 0;
        self.has = false;
        self.lit = false;
        self.mouth = false;
        self.len = 1.0;
        self.cig_base_known = false;
        for n in ["SetVRSmokeMouthAnchor", "SetVRSmokeFingers", "SetVRSmokeFingersL", "SetVRSmokeLeftCig", "SetVRSmokeRightLighter"] {
            set(n, false);
        }
        log!("[Smoke] put out (from Rust)\n");
    }
    /// The lighter's spark (the flint you press for), then its held flame.
    fn ignite(&self, pl: &Handle) {
        let lt = self.lighter(pl);
        if lt.is_null() {
            return;
        }
        effect(&lt, "ignition", true, false);
        effect(&lt, "ignition_loop", true, true);
        sound(&lt, "cmn_generic_item_lighter");
    }
    fn douse(&self, pl: &Handle) {
        let lt = self.lighter(pl);
        if !lt.is_null() {
            effect(&lt, "ignition_loop", false, false);
            effect(&lt, "ignition", false, false);
        }
    }
    /// The flame caught on the tip: the ember and the smoke on, the lighter gone.
    fn light(&mut self, pl: &Handle) {
        if !self.has || self.lit {
            return;
        }
        let cig = self.cig(pl);
        if cig.is_null() {
            return;
        }
        self.lit = true;
        effect(&cig, "ember", true, false);
        effect(&cig, "ember_passive", true, true);
        effect(&cig, "smoke_trail", true, true);
        sound(&cig, "cmn_generic_male_inhale_smoking");
        self.douse(pl);
        remove(pl, self.lighter_slot(), true);
        self.release_lighter_hand();
        log!("[Smoke] lit (from Rust)\n");
        if self.mouth {
            self.schedule(Timer::Puff, PUFF_PERIOD, self.token);
        }
    }
    fn inhale(&self, pl: &Handle) {
        if !self.lit {
            return;
        }
        let cig = self.cig(pl);
        if !cig.is_null() {
            effect(&cig, "ember", true, false);
            sound(&cig, "cmn_generic_male_inhale_smoking");
        }
    }
    fn exhale(&self, pl: &Handle) {
        if !self.lit {
            return;
        }
        let cig = self.cig(pl);
        if !cig.is_null() {
            self.mouth_puff(pl);
            sound(&cig, "cmn_generic_male_exhale_smoking");
        }
    }
    /// VRSmokeMouthPuff: the exhale's smoke (the cigarette's ResourceLibrary "mouth_smoke") spawned at the plugin's
    /// HMD-tracked mouth pose.
    fn mouth_puff(&self, pl: &Handle) {
        let cig = self.cig(pl);
        let lib = super::lobby_ink::child(&cig, "FindComponentByName", &[Value::U64(CName::hashed("ResourceLibrary").0)]);
        let r = (|| -> Result<String, &str> {
            let fx = rt::call(&lib, "GetResource", &[A::Name(CName::hashed("mouth_smoke").0)]).ok_or("no ResourceLibrary")?;
            if !rt::static_call("FxResource", "IsValid", &[A::P(fx.as_ptr().cast())]).is_some_and(|o| rt::Val::of(&o).bool()) {
                return Err("no mouth_smoke resource");
            }
            let p = native::<[f32; 4]>("VRSmokeMouthWorldPos", &[]).filter(|p| p[3] >= 0.5).ok_or("no mouth pose")?;
            let q = native::<[f32; 4]>("VRSmokeMouthWorldRot", &[]).map_or([0.0, 0.0, 0.0, 1.0], super::belt::f4);
            let xf = super::reload_core::world_bytes(super::belt::f3(p), q);
            let fx_system = super::game_system("GetFxSystem");
            let made = rt::call(&fx_system, "SpawnEffect", &[A::P(fx.as_ptr().cast()), A::P(xf.as_ptr().cast())])
                .is_some_and(|o| !rt::Val::of(&o).handle().is_null());
            Ok(format!("spawned at ({:.2}, {:.2}, {:.2}), an instance {made}", p[0], p[1], p[2]))
        })();
        static SAID: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !SAID.swap(true, std::sync::atomic::Ordering::Relaxed) {
            log!("[Smoke] the mouth's smoke: {} (from Rust)\n", r.unwrap_or_else(|e| format!("not spawned ({e})")));
        }
    }
    /// VRSmokeStickToggle: into the mouth from the hand holding it, or out into the hand whose grip fired
    /// (0 right, 2 left) when that hand is free.
    fn stick_toggle(&mut self, pl: &Handle, dest: i32) {
        if self.mouth {
            if dest == 2 {
                if !item_in(pl, LEFT).is_null() {
                    return;
                }
                remove(pl, MOUTH, false);
                set("SetVRSmokeLeftCig", true);
                add(pl, LEFT, &self.cig_id);
                set("SetVRSmokeFingersL", true);
                self.cig_slot = 2;
            } else {
                if !item_in(pl, RIGHT).is_null() {
                    return;
                }
                remove(pl, MOUTH, false);
                add(pl, RIGHT, &self.cig_id);
                set("SetVRSmokeFingers", true);
                self.cig_slot = 0;
            }
            set("SetVRSmokeMouthAnchor", false);
            self.mouth = false;
            self.token += 1;
            log!("[Smoke] out of the mouth into the {} hand (from Rust)\n", if dest == 2 { "left" } else { "right" });
        } else {
            if !self.has {
                return;
            }
            remove(pl, cig_slot_name(self.cig_slot), false);
            add_anywhere(pl, MOUTH, &self.cig_id);
            if self.cig_slot == 2 {
                set("SetVRSmokeFingersL", false);
                set("SetVRSmokeLeftCig", false);
            } else {
                set("SetVRSmokeFingers", false);
            }
            self.cig_slot = 1;
            set("SetVRSmokeMouthAnchor", true);
            self.mouth = true;
            log!("[Smoke] into the mouth (from Rust)\n");
            if self.lit {
                self.inhale(pl);
                self.schedule(Timer::Exhale, EXHALE_AFTER, self.token);
                self.schedule(Timer::Puff, PUFF_PERIOD, self.token);
            }
        }
    }
    fn schedule(&mut self, t: Timer, after: f64, token: i32) {
        self.timers.push((after, t, token));
    }
    fn auto_puff(&mut self, pl: &Handle, token: i32) {
        if !self.mouth || !self.lit || token != self.token {
            return;
        }
        self.inhale(pl);
        self.schedule(Timer::Exhale, EXHALE_AFTER, token);
        self.schedule(Timer::Puff, PUFF_PERIOD, self.token);
    }
    fn auto_exhale(&self, pl: &Handle, token: i32) {
        if !self.mouth || !self.lit || token != self.token {
            return;
        }
        let cig = self.cig(pl);
        if !cig.is_null() {
            self.mouth_puff(pl);
            sound(&cig, "cmn_generic_male_exhale_smoking");
        }
    }
    /// VRSmokeFreeHands: with the cigarette in the mouth only the lighter, if a hand still holds it, is put away.
    fn free_hands(&self, pl: &Handle) {
        let lighter = self.lighter(pl);
        if lighter.is_null() {
            return;
        }
        let id = rt::call(&lighter, "GetItemID", &[]).map(|o| {
            // SAFETY: an ItemID's leading TweakDBID.
            unsafe { *(o.as_ptr() as *const u64) }
        });
        if id == Some(red4ext_abi::tweakdbid::from_name(LIGHTER)) {
            effect(&lighter, "ignition_loop", false, false);
            remove(pl, self.lighter_slot(), true);
            self.release_lighter_hand();
        }
    }

    /// VRSmokeTick: the gestures, once a frame.
    fn tick(&mut self, pl: &Handle, lt: f64, rtrig: f64, grip: [bool; 2], dt: f64) {
        // the timers first (the script's delay callbacks)
        let mut due = Vec::new();
        self.timers.retain_mut(|(t, kind, token)| {
            *t -= dt;
            if *t <= 0.0 {
                due.push((*kind, *token));
                false
            } else {
                true
            }
        });
        for (kind, token) in due {
            match kind {
                Timer::Puff => self.auto_puff(pl, token),
                Timer::Exhale => self.auto_exhale(pl, token),
            }
        }
        if !self.has {
            self.igniting = false;
            self.was_mouth = false;
            return;
        }
        self.light_cd -= dt;
        self.stick_cd -= dt;
        let cig = self.cig(pl);
        if cig.is_null() {
            return;
        }
        // the burn-down: the authored full length (learned once) times what is left
        if !self.cig_base_known {
            let b = native::<f32>("GetVRSmokeCigVisualScaleY", &[Value::Handle(&cig)]).map_or(0.0, f64::from);
            if b > 0.01 {
                self.cig_base = b;
                self.cig_base_known = true;
            }
        }
        if self.cig_base_known {
            native::<i32>("SetVRSmokeCigVisualScale", &[Value::Handle(&cig), Value::Float((self.cig_base * self.len) as f32)]);
        }
        let fwd = m::<[f32; 4]>(&cig, "GetWorldForward", &[]).ok().map(super::belt::f3).unwrap_or([0.0; 3]);
        let tip = world_pos(&cig).map(|p| [p[0] + fwd[0] * 0.065, p[1] + fwd[1] * 0.065, p[2] + fwd[2] * 0.065]);
        // hand at the lips ~0.17, chest ~0.4, arm down ~0.6 (the plugin's HMD-local distance, per hand)
        self.last_mouth = native::<f32>("VRSmokeMouthDist", &[]).map_or(999.0, f64::from);
        self.last_mouth_l = native::<f32>("VRSmokeMouthDistL", &[]).map_or(999.0, f64::from);
        self.last_light = 999.0;
        let lighter = self.lighter(pl);
        if let (Some(t), Some(l)) = (tip, world_pos(&lighter)) {
            self.last_light = super::belt::dist(t, l);
        }
        // a grip at the lips, each by its own hand (rising edge): which fired decides where it goes out to
        let fire_r = grip[0] && !self.grip_prev[0] && self.last_mouth < 0.22;
        let fire_l = grip[1] && !self.grip_prev[1] && self.last_mouth_l < 0.22;
        if (fire_r || fire_l) && self.stick_cd <= 0.0 {
            let dest = if self.mouth && !fire_r { 2 } else { 0 };
            self.stick_toggle(pl, dest);
            self.stick_cd = 0.5;
        }
        self.grip_prev = grip;
        if !self.lit {
            let t = if self.left_handed { rtrig } else { lt };
            if t >= 0.95 && !self.igniting {
                self.ignite(pl);
                self.igniting = true;
            } else if t < 0.85 && self.igniting {
                self.douse(pl);
                self.igniting = false;
            }
            // the flame has to heat the tip for 1.5 s; pulled off, it starts again
            if self.igniting && self.last_light < 0.08 {
                self.light_progress += dt;
                if self.light_progress >= 1.5 && self.light_cd <= 0.0 {
                    self.light(pl);
                    self.igniting = false;
                    self.light_progress = 0.0;
                    self.light_cd = 0.5;
                }
            } else {
                self.light_progress = 0.0;
            }
        } else {
            self.igniting = false;
            // hysteresis: held at the mouth to 0.30, entered under 0.22
            let at_mouth = self.last_mouth < if self.was_mouth { 0.30 } else { 0.22 };
            if at_mouth && !self.was_mouth {
                self.inhale(pl);
            } else if !at_mouth && self.was_mouth {
                self.exhale(pl);
            }
            self.was_mouth = at_mouth;
            // ~2 min passive, three times faster drawing; out at 0.4 (a stub remains)
            self.len -= (if at_mouth { 0.015 } else { 0.005 }) * dt;
            if self.len <= 0.4 {
                self.extinguish(pl);
            }
        }
    }
}

// ---------------------------------------------------------------- the callers'
/// VRSmokeHasCig.
pub(super) fn has_cig(pl: &Handle) -> bool {
    !pl.is_null() && with(pl, |s| s.has)
}
/// VRSmokeToggle: spawned, or put out.
pub(super) fn toggle(pl: &Handle) {
    if !pl.is_null() {
        with(pl, |s| if s.has { s.extinguish(pl) } else { s.spawn(pl) });
    }
}
/// The bridge's tick: (lit, the tip-to-lighter distance, the right and left hand-to-mouth distances) for its log.
pub(super) fn tick(pl: &Handle, lt: f64, rtrig: f64, grip: [bool; 2], dt: f64) -> (bool, f64, f64, f64) {
    with(pl, |s| {
        s.tick(pl, lt, rtrig, grip, dt);
        (s.lit, s.last_light, s.last_mouth, s.last_mouth_l)
    })
}
/// EquipmentSystemPlayerData.OnEquipmentSystemWeaponManipulationRequest, before the request: a weapon, a gadget or a
/// consumable taken into a hand, or the weapons put away, ends the smoke (the end of a left-hand action does not);
/// with the cigarette in the mouth only the lighter goes.
fn weapon_request(call: &mut crate::script_hooks::Call) {
    let data = call.this();
    let pl = rt::field(&data, "m_owner").map(|v| v.handle()).unwrap_or_else(Handle::null);
    if !super::menu_rt::is_a(&pl, "PlayerPuppet") || !has_cig(&pl) {
        return;
    }
    let req = call.arg_handle(0);
    let action = rt::field(&req, "requestType").and_then(|v| v.num()).unwrap_or(-1.0);
    let spares = ["Undefined", "UnequipConsumable", "UnequipGadget", "UnequipLeftHandCyberware"];
    if spares.iter().any(|a| action == rt::enum_value("EquipmentManipulationAction", a)) {
        return;
    }
    with(&pl, |s| if s.mouth { s.free_hands(&pl) } else { s.extinguish(&pl) });
}
pub(super) const HOOKS: [(&str, &str, crate::script_hooks::Kind, crate::script_hooks::HookFn); 1] =
    [("EquipmentSystemPlayerData", "OnEquipmentSystemWeaponManipulationRequest", crate::script_hooks::Kind::Before, weapon_request)];

/// A development check (CVR_SMOKE_TEST, inherited by the game from the probe session): 45 s into the loaded save the
/// cigarette and lighter are spawned, at 48 s the cigarette goes into the mouth, at 50 s it is lit (as the flame would
/// light it), at 58 s -- past one auto-puff -- it comes out into the right hand, at 61 s a weapon is requested (the
/// hook ends the smoke when the equipment system handles it), at 64 s the state is logged.
pub(super) fn self_test(dt: f64) {
    thread_local! {
        static T: std::cell::Cell<(f64, u32, usize)> = const { std::cell::Cell::new((0.0, 0, 0)) };
    }
    if std::env::var_os("CVR_SMOKE_TEST").is_none() {
        return;
    }
    let pl = super::belt::player();
    let (t, step, first) = T.with(|c| c.get());
    if pl.is_null() || step >= 6 {
        return;
    }
    let p = pl.instance as usize;
    let first = if first == 0 { p } else { first };
    if p == first || native::<bool>("VRLobbyActive", &[]) == Some(true) {
        T.with(|c| c.set((t, step, first)));
        return;
    }
    let t = t + dt;
    let state = |s: &Smoke| format!("has={} lit={} mouth={} slot={} len={:.3} token={} timers={}", s.has, s.lit, s.mouth, s.cig_slot, s.len, s.token, s.timers.len());
    let mut next = step;
    match step {
        0 if t > 45.0 => {
            toggle(&pl);
            let held = (!item_in(&pl, RIGHT).is_null(), !item_in(&pl, LEFT).is_null());
            log!("[Smoke] self-test spawned: {} ; right hand {} left hand {} (from Rust)\n", with(&pl, |s| state(s)), held.0, held.1);
            next = 1;
        }
        1 if t > 48.0 => {
            log!("[Smoke] self-test 3 s after the spawn: right hand {} left hand {} (from Rust)\n", !item_in(&pl, RIGHT).is_null(), !item_in(&pl, LEFT).is_null());
            with(&pl, |s| s.stick_toggle(&pl, 0));
            log!("[Smoke] self-test in the mouth: {} ; Splinter {} (from Rust)\n", with(&pl, |s| state(s)), !item_in(&pl, MOUTH).is_null());
            next = 2;
        }
        2 if t > 50.0 => {
            log!("[Smoke] self-test 2 s in the mouth: Splinter {} right hand {} (from Rust)\n", !item_in(&pl, MOUTH).is_null(), !item_in(&pl, RIGHT).is_null());
            with(&pl, |s| s.light(&pl));
            log!("[Smoke] self-test lit: {} ; lighter {} (from Rust)\n", with(&pl, |s| state(s)), !with(&pl, |s| s.lighter(&pl)).is_null());
            next = 3;
        }
        3 if t > 58.0 => {
            with(&pl, |s| s.stick_toggle(&pl, 0));
            log!("[Smoke] self-test out of the mouth: {} ; right hand {} (from Rust)\n", with(&pl, |s| state(s)), !item_in(&pl, RIGHT).is_null());
            next = 4;
        }
        4 if t > 61.0 => {
            log!("[Smoke] self-test 3 s out of the mouth: right hand {} Splinter {} (from Rust)\n", !item_in(&pl, RIGHT).is_null(), !item_in(&pl, MOUTH).is_null());
            super::holster::equip_request(&pl, "RequestWeaponSlot1");
            log!("[Smoke] self-test weapon requested: {} (from Rust)\n", with(&pl, |s| state(s)));
            next = 5;
        }
        5 if t > 64.0 => {
            // the request is handled by the equipment system later: the hook ended the smoke there, or not
            log!("[Smoke] self-test after the weapon request: {} (from Rust)\n", with(&pl, |s| state(s)));
            if with(&pl, |s| s.has) {
                toggle(&pl);
            }
            log!("[Smoke] self-test done (from Rust)\n");
            next = 6;
        }
        _ => {}
    }
    T.with(|c| c.set((t, next, first)));
}
