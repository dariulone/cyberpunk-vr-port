//! reload/glow.lua: what the reload wants of the held gun lit on it -- the magazine's glow copy over the seated
//! magazine (pulsing when the gun is empty, a steady ghost while the magazine is out), or the glow copy of the part the
//! hand must pull (the bolt, the slide, the hammer) on that part's slot, drawn by the player's glow carrier for the
//! holding hand (vrp_glow_left / right); a gun with no copy of its part has the reach marker blink on its slide slot.
use super::belt::{conj, h, native, player, qmul, qrot, Q, V3};
use super::reload_core::{slot_world, State};
use super::reload_data::{self as data, Lua};
use crate::script_call::Value;
use red4ext_abi::cname::CName;
use red4ext_abi::handle::Handle;

const CARRIER: [&str; 2] = ["vrp_glow_left", "vrp_glow_right"];
const SLOT: [&str; 2] = ["WeaponLeft", "WeaponRight"];
const PULSE: &str = "vrp_glow";
const GHOST: &str = "vrp_glow_ghost";
const PERIOD: f64 = 3.0;
const RACK_ON: f64 = 0.55;
const RETRY: f64 = 2.0;

#[derive(Default)]
pub(super) struct Glow {
    comp: [Handle; 2],
    mesh: [Option<String>; 2],
    look: [Option<&'static str>; 2],
    slots: Handle,
    time: f64,
    missed: [Option<f64>; 3],
    forced: Option<(Option<&'static str>, f64)>,
}
// SAFETY: the handles are made, used and dropped on the game's main thread, where the mechanics tick.
unsafe impl Send for Glow {}

fn carrier(g: &mut Glow, side: usize) -> Handle {
    if !g.comp[side].is_null() {
        return g.comp[side].clone();
    }
    if g.time - g.missed[side].unwrap_or(-RETRY) < RETRY {
        return Handle::null();
    }
    let found = h(&player(), "FindComponentByName", &[Value::U64(CName::hashed(CARRIER[side]).0)]);
    if found.is_null() {
        g.missed[side] = Some(g.time);
        return found;
    }
    if let Some(v) = crate::script_call::enum_value("ERenderingPlane", "RPl_Weapon") {
        // SAFETY: the component's enum property (one byte suffices for the plane's values).
        if let Some(p) = unsafe { crate::script_call::property(found.instance, "renderingPlane") } {
            unsafe { *(p as *mut u8) = v as u8 };
        }
    }
    // SAFETY: the component's LOD byte.
    if let Some(p) = unsafe { crate::script_call::property(found.instance, "forceLODLevel") } {
        unsafe { *(p as *mut i8) = 0 };
    }
    g.comp[side] = found.clone();
    found
}
fn slot_comp(g: &mut Glow) -> Handle {
    if !g.slots.is_null() {
        return g.slots.clone();
    }
    if g.time - g.missed[2].unwrap_or(-RETRY) < RETRY {
        return Handle::null();
    }
    g.missed[2] = Some(g.time);
    if let Some(c) = super::belt::components(&player()).into_iter().find(|c| super::belt::comp_name(c).contains("ItemAttachmentSlots")) {
        g.slots = c.clone();
        return c;
    }
    Handle::null()
}
fn show(g: &mut Glow, side: usize, glow_mesh: Option<&str>, look: Option<&'static str>) -> bool {
    let c = carrier(g, side);
    if c.is_null() {
        return false;
    }
    let Some(look) = look else {
        if g.look[side].is_some() {
            super::belt::toggle(&c, false);
        }
        g.look[side] = None;
        return true;
    };
    let Some(mesh) = glow_mesh else { return false };
    if g.mesh[side].as_deref() != Some(mesh) {
        let p = super::belt::engine_str(mesh);
        if native::<bool>("VRComponentChangeResource", &[Value::Handle(&c), Value::Ptr(p.as_ptr()), Value::Bool(true)]).is_none() {
            return false;
        }
        (g.mesh[side], g.look[side]) = (Some(mesh.to_string()), None);
        super::belt::toggle(&c, false);
    }
    if g.look[side] != Some(look) {
        super::belt::change_appearance(&c, look, true);
        if g.look[side].is_none() {
            super::belt::toggle(&c, true);
        }
        g.look[side] = Some(look);
    }
    true
}
/// The carrier's local transform that puts its mesh at the world pose (relative to the player's hand slot).
fn carry(g: &mut Glow, side: usize, mp: V3, mq: Q) -> bool {
    let sc = slot_comp(g);
    let c = carrier(g, side);
    if sc.is_null() || c.is_null() {
        return false;
    }
    let Some((sp, sq)) = slot_world(&sc, SLOT[side]) else {
        g.slots = Handle::null();
        return false;
    };
    let ci = conj(sq);
    let l = qrot(ci, [mp[0] - sp[0], mp[1] - sp[1], mp[2] - sp[2]]);
    super::reload_core::set_local(&c, l, qmul(ci, mq))
}
fn place_magazine(g: &mut Glow, s: &mut State, side: usize, weapon: &Handle, slots: &Handle) -> bool {
    let Some((mp, mq)) = super::reload_parts::seated(s, weapon, slots) else { return false };
    carry(g, side, mp, mq)
}
fn place_part(g: &mut Glow, side: usize, slots: &Handle, slot: &str) -> bool {
    let Some((p, q)) = slot_world(slots, slot) else { return false };
    carry(g, side, p, q)
}
fn off(g: &mut Glow) {
    for side in 0..2 {
        if g.look[side].is_some() {
            let m = g.mesh[side].clone();
            show(g, side, m.as_deref(), None);
        }
    }
}
/// M.hide: the highlight off without dropping the component caches (a transient tracking or equip miss).
pub(super) fn hide(g: &mut Glow) {
    off(g);
}
fn mark_rack(side: usize, slots: &Handle, slot: Option<&str>, on: bool) {
    let (true, Some(slot)) = (on, slot) else { return };
    let Some((p, _)) = slot_world(slots, slot) else { return };
    super::reach::mark(side + 1, super::reach::Mark { pos: p, radius: Some(0.035), kind: super::reach::Kind::Lock, ready: true, from: None, lift: Some(0.0) });
}
/// M.frame: the glow for the holder's gun.
pub(super) fn frame(g: &mut Glow, s: &mut State, weapon: &Handle, holder: Option<usize>, slots: &Handle, dt: f64) {
    g.time += dt;
    let (Some(holder), false) = (holder.filter(|h| *h < 2), weapon.is_null()) else {
        off(g);
        return;
    };
    // THE SWITCH (Settings > Controls > Reload highlight, xr_reload_glow): published inverted to shared slot [174], 1 =
    // off, so an unset slot leaves the highlight on; off draws nothing and the reload itself is untouched
    if super::belt::shared(174) > 0.5 && !g.forced.is_some_and(|(_, till)| g.time < till) {
        off(g);
        return;
    }
    let w = super::reload_parts::wants(s, weapon);
    let (mut want, mesh, slide_slot) = match w {
        Some((want, mesh, slide)) => (want, Some(mesh), slide),
        None => (None, None, None),
    };
    if let Some((fw, till)) = g.forced {
        if g.time < till {
            want = fw;
        }
    }
    let glow = data::data().g("glow");
    let mut glow_mesh = mesh.and_then(|m| glow.g("meshes").s(m));
    let other = 1 - holder;
    if g.look[other].is_some() {
        let m = g.mesh[other].clone();
        show(g, other, m.as_deref(), None);
    }
    let part = if want == Some("rack") { mesh.map(|m| glow.g("bolts").g(m)).filter(|p| p.present()) } else { None };
    if want == Some("rack") && part.is_none() {
        mark_rack(holder, slots, slide_slot, (g.time % PERIOD) < RACK_ON * PERIOD);
    }
    let mut look = None;
    if part.is_some() || (glow_mesh.is_some() && want == Some("empty")) {
        look = Some(PULSE);
    } else if glow_mesh.is_some() && want == Some("out") {
        look = Some(GHOST);
    }
    if let Some(part) = part {
        glow_mesh = part.s("mesh");
        if look.is_some() && !part.s("slot").is_some_and(|sl| place_part(g, holder, slots, sl)) {
            look = None;
        }
    } else if look.is_some() && !place_magazine(g, s, holder, weapon, slots) {
        look = None;
    }
    show(g, holder, glow_mesh, look);
}
pub(super) fn clear(g: &mut Glow) {
    off(g);
    let t = g.time;
    *g = Glow { time: t, ..Glow::default() };
}
#[allow(dead_code)]
/// M.force: a want shown for a while (the overlay's test buttons).
pub(super) fn force(g: &mut Glow, want: Option<&'static str>, seconds: f64) {
    g.forced = Some((want, g.time + seconds));
}
