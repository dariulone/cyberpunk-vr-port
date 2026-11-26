//! THE ROAD WHEELS AGAINST THE CABIN WHEEL, for the car the player drives with a hand on the wheel (main's
//! src/Anim/VehicleSteerCurve.cpp).
//!
//! The hands turn the cabin wheel one to one (wheel_grab: the input is read off the cabin wheel's curve), and the car
//! turns its road wheels from that same input. Most drive models take the "alternative" turn update, which squares the
//! input -- a thumbstick's curve: on the 911 the hands at 20/40/80/120 degrees put the road wheels at 1.5/5.7/18.6/33.7
//! degrees, too little of a real wheel's middle. So this writes the car's live physics, only while a hand owns the
//! wheel of the car the player drives:
//!   road = maxWheelTurnDeg * speed taper * c1(|n|) * (share + (1 - share) * |n|)
//! (share = CyberpunkVR_WheelRoadLinearShare, 0.48: 20 degrees of wheel -> 5 of road on the 911, the full lock still
//! the car's own). The physics copies its drive model at spawn and its turn update (2.5: sub_14061F538) reads the copy
//! every step; in its linear branch the target is sign(n) * maxWheelTurnDeg * c0(speed) * c1(|n|), so the flag is
//! cleared and maxWheelTurnDeg rewritten each poll to M = maxWheelTurnDeg * taper(speed) * (share + (1 - share) |n|) /
//! c0(speed). The moment the hand lets go, the seat is left, a menu opens or the car changes, the copy gets its own
//! values back (the alternative update's smoothed input set where the road wheels are, so they do not jump). NPC
//! drivers and traffic never see it. Every offset is the 2.5 build's and checked against the code that uses it before
//! anything is written; the live block is identified by its two owner pointers back to the vehicle.
use cvr_engine::code;
use std::sync::Mutex;

/// Share of the turn that reaches the road wheels in proportion to the hands, 0..1.
#[no_mangle]
pub static mut CyberpunkVR_WheelRoadLinearShare: f32 = 0.48;
/// 0 = leave every car's steering exactly as the game has it.
#[no_mangle]
pub static mut CyberpunkVR_WheelRoadCurve: i32 = 1;
/// Test read-out: applied, maxWheelTurnDeg (own), written M, |n|, c0, taper, road angle, own alternative flag.
#[no_mangle]
pub static mut CyberpunkVR_WheelRoadDebug: [f32; 8] = [0.0; 8];
/// Test read-out: the mounted vehicle object, as the plugin last saw it (0 on foot).
#[no_mangle]
pub static mut CyberpunkVR_DebugMountedVehicle: u64 = 0;

extern "C" {
    fn cvr_mounted_vehicle_identity() -> usize;
}

// vehicle (vehicleBaseObject)
const VEH_PHYSICS: usize = 0x2D0;
const VEH_INPUT: usize = 0x278;
const VEH_STATE: usize = 0x5F0;
const VEH_CURVES: usize = 0x2E0;
// physics
const PHYS_OWNER_A: usize = 0x60;
const PHYS_OWNER_B: usize = 0x108;
const PHYS_ROAD: usize = 0xF4;
const PHYS_ALT_STATE: usize = 0xF8;
const MAX_DEG: usize = 0xC50;
const ALT: usize = 0xC5C;
const BASE: usize = 0xC60;
const MID: usize = 0xC64;
const TOP: usize = 0xC68;
const MID_MUL: usize = 0xC70;
const TOP_MUL: usize = 0xC78;
const STATE_SPEED: usize = 0xC8;
const STATE_SPEED_TENTH: usize = 0xCC;
// curve set: entries at +0x40, count at +0x4C, 0x40 bytes each, CName key first
const SET_ENTRIES: usize = 0x40;
const SET_COUNT: usize = 0x4C;
const ENTRY_STRIDE: usize = 0x40;
const CURVE_EVAL_RVA: usize = 0x61FB14;
const CURVE_VALUE_RVA: usize = 0x62008C;

/// The instructions that read each offset above, in the turn update and its helpers (2.5).
const SITES: [(usize, &str); 13] = [
    (0x61F5A1, "488b9308010000"),
    (0x61F5AF, "488bbaf0050000"),
    (0x61F5E5, "f3440f59a278020000"),
    (0x61F5F6, "f3440f109fcc000000"),
    (0x61F709, "488b4360"),
    (0x61F70D, "80b8410a000000"),
    (0x61F71E, "80bb5c0c000000"),
    (0x61F72B, "f3440f109fc8000000"),
    (0x61F965, "f30f10a3500c0000"),
    (0x61FAAB, "f30f1193f4000000"),
    (CURVE_EVAL_RVA, "4883ec28418bd0488d05"),
    (CURVE_VALUE_RVA, "4055488bec4883ec500fb65138"),
    (0x6E3964, "488b91e0020000"),
];

type CurveValueFn = unsafe extern "system" fn(usize, f32) -> f32;
struct Game {
    curve_value: CurveValueFn,
    curve_keys: usize,
}
static GAME: std::sync::OnceLock<Option<Game>> = std::sync::OnceLock::new();

fn log(text: &str) {
    super::env::log(text);
}

fn sites_match() -> Option<Game> {
    let exe = code::image_base();
    if exe == 0 {
        return None;
    }
    for (rva, bytes) in SITES {
        let want: Vec<u8> = (0..bytes.len() / 2).filter_map(|i| u8::from_str_radix(&bytes[i * 2..i * 2 + 2], 16).ok()).collect();
        let mut have = vec![0u8; want.len()];
        if !code::read_bytes(exe + rva, &mut have) || have != want {
            log(&format!("[steer-curve] the game's turn update is not the one this was written for (rva {rva:X}): cars keep their own steering\n"));
            return None;
        }
    }
    // lea rax,[rip+rel32] at +7: the table of the four curve names the turn update looks up
    let rel = code::read::<i32>(exe + CURVE_EVAL_RVA + 10)?;
    Some(Game {
        // SAFETY: the curve function whose first bytes were just matched.
        curve_value: unsafe { core::mem::transmute::<usize, CurveValueFn>(exe + CURVE_VALUE_RVA) },
        curve_keys: (exe + CURVE_EVAL_RVA + 14).wrapping_add_signed(rel as isize),
    })
}
fn game() -> Option<&'static Game> {
    GAME.get_or_init(sites_match).as_ref()
}

fn read_f(address: usize, fallback: f32) -> f32 {
    code::read::<f32>(address).filter(|v| v.is_finite()).unwrap_or(fallback)
}

/// The car's curve `index` (0 speed taper, 1 input) at x, as the turn update reads it; None without that curve.
fn car_curve(g: &Game, set: usize, index: usize, x: f32) -> Option<f32> {
    let key = code::read::<u64>(g.curve_keys + 8 * index)?;
    let entries = code::read::<usize>(set + SET_ENTRIES)?;
    let count = code::read::<u32>(set + SET_COUNT)?;
    if entries == 0 || count > 64 {
        return None;
    }
    for i in 0..count as usize {
        if code::read::<u64>(entries + ENTRY_STRIDE * i) == Some(key) {
            // The engine's own curve function on an entry whose key was just read (main calls it under SEH; the guarded
            // call here takes pointer arguments only, and this one takes a float).
            // SAFETY: a live curve entry of the car's set.
            let v = unsafe { (g.curve_value)(entries + ENTRY_STRIDE * i, x) };
            return v.is_finite().then(|| v.clamp(0.0, 1.0));
        }
    }
    None
}

/// sub_14061CAE4: a to b across [lo, hi], clamped; the bounds are put in order first.
fn remap(x: f32, mut lo: f32, mut hi: f32, mut a: f32, mut b: f32) -> f32 {
    if hi < lo {
        core::mem::swap(&mut lo, &mut hi);
        core::mem::swap(&mut a, &mut b);
    }
    if x <= lo {
        a
    } else if x < hi {
        (x - lo) / (hi - lo) * (b - a) + a
    } else {
        b
    }
}
/// The alternative turn update's speed taper, from the copied thresholds.
fn speed_taper(phys: usize, speed: f32) -> f32 {
    let (base, mid, top) = (read_f(phys + BASE, 0.0), read_f(phys + MID, 0.0), read_f(phys + TOP, 0.0));
    let (mid_mul, top_mul) = (read_f(phys + MID_MUL, 1.0), read_f(phys + TOP_MUL, 1.0));
    if mid == 0.0 {
        remap(speed, base, top, 1.0, top_mul)
    } else if speed > mid {
        remap(speed, mid, top, mid_mul, top_mul)
    } else {
        remap(speed, base, mid, 1.0, mid_mul)
    }
}

#[derive(Default)]
struct Patch {
    vehicle: usize,
    phys: usize,
    own_max: f32,
    written: f32,
    taper: f32,
    own_alt: u8,
    applied: bool,
    refused: bool,
    logged: bool,
}
static PATCH: Mutex<Option<Patch>> = Mutex::new(None);

fn owned(p: &Patch) -> bool {
    p.phys != 0
        && code::read::<usize>(p.phys + PHYS_OWNER_A) == Some(p.vehicle)
        && code::read::<usize>(p.phys + PHYS_OWNER_B) == Some(p.vehicle)
}
/// Our values still in the block: nothing has replaced or freed it since the last write.
fn still_ours(p: &Patch) -> bool {
    owned(p) && code::read::<u8>(p.phys + ALT) == Some(0) && code::read::<f32>(p.phys + MAX_DEG).map(f32::to_bits) == Some(p.written.to_bits())
}
fn restore(p: &mut Patch) {
    if !p.applied {
        return;
    }
    p.applied = false;
    // SAFETY: the read-out export.
    unsafe { CyberpunkVR_WheelRoadDebug[0] = 0.0 };
    if !still_ours(p) {
        log(&format!("[steer-curve] {:#x}: the car's physics changed under the curve; nothing put back\n", p.vehicle));
        return;
    }
    if p.own_alt != 0 {
        // The alternative branch steers from its own slewed input: start it where the road wheels are now.
        let road = read_f(p.phys + PHYS_ROAD, 0.0);
        code::write::<f32>(p.phys + PHYS_ALT_STATE, (road / (p.own_max * p.taper.max(0.05))).clamp(-1.0, 1.0));
    }
    code::write::<f32>(p.phys + MAX_DEG, p.own_max);
    code::write::<u8>(p.phys + ALT, p.own_alt);
}
fn take(p: &mut Patch) -> bool {
    let Some(phys) = code::read::<usize>(p.vehicle + VEH_PHYSICS).filter(|&a| a != 0) else { return false };
    p.phys = phys;
    let own = code::read::<f32>(phys + MAX_DEG);
    let alt = code::read::<u8>(phys + ALT);
    let (Some(own), Some(alt)) = (own, alt) else {
        p.phys = 0;
        return false;
    };
    if !owned(p) || !own.is_finite() || !(5.0..=120.0).contains(&own) || alt > 1 {
        log(&format!(
            "[steer-curve] {:#x}: no drive model where expected (physics {phys:#x}); this car keeps its own steering\n",
            p.vehicle
        ));
        p.phys = 0;
        return false;
    }
    p.own_max = own;
    p.own_alt = alt;
    if !p.logged {
        p.logged = true;
        log(&format!("[steer-curve] {:#x}: wheel curve on (maxWheelTurnDeg {own:.1}, alternative turn update {alt})\n", p.vehicle));
    }
    true
}

/// VehicleSteerCurveTick, once per XInput poll: `held` the player drives a car with a hand owning its wheel.
pub fn tick(held: bool) {
    let Ok(mut guard) = PATCH.try_lock() else { return };
    let p = guard.get_or_insert_with(Patch::default);
    // SAFETY: a plain call and the read-out exports.
    let mounted = unsafe { cvr_mounted_vehicle_identity() };
    unsafe { CyberpunkVR_DebugMountedVehicle = mounted as u64 };
    let enabled = unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_WheelRoadCurve) } != 0;
    let vehicle = if held && enabled && game().is_some() { mounted } else { 0 };
    if p.vehicle != 0 && p.vehicle != vehicle {
        restore(p);
    }
    if vehicle == 0 {
        if p.vehicle != mounted {
            *p = Patch::default(); // keep the refusal while seated in the same car
        }
        return;
    }
    if p.vehicle != vehicle {
        *p = Patch { vehicle, ..Patch::default() };
    }
    if p.refused {
        return;
    }
    if !p.applied {
        if !take(p) {
            p.refused = true;
            return;
        }
        p.applied = true;
    } else if !still_ours(p) {
        if !owned(p) {
            log(&format!("[steer-curve] {:#x}: the car's physics changed under the curve; letting go of it\n", p.vehicle));
            p.applied = false;
            p.refused = true;
            return;
        }
        // The game set its drive model again (our maxWheelTurnDeg gone): those are the car's own values now.
        if code::read::<f32>(p.phys + MAX_DEG).is_some_and(|m| m.to_bits() != p.written.to_bits()) && !take(p) {
            p.applied = false;
            p.refused = true;
            return;
        }
    }
    let n = read_f(vehicle + VEH_INPUT, 0.0).abs().clamp(0.0, 1.0);
    let (mut speed, mut tenth) = (0.0f32, 0.0f32);
    if let Some(state) = code::read::<usize>(vehicle + VEH_STATE).filter(|&s| s != 0) {
        speed = read_f(state + STATE_SPEED, 0.0).abs();
        tenth = read_f(state + STATE_SPEED_TENTH, 0.0).clamp(-1.0, 1.0);
    }
    let mut c0 = 1.0f32;
    if let (Some(g), Some(holder)) = (game(), code::read::<usize>(vehicle + VEH_CURVES).filter(|&h| h != 0)) {
        if let Some(set) = code::read::<usize>(holder + 8).filter(|&s| s != 0) {
            if let Some(v) = car_curve(g, set, 0, tenth) {
                c0 = v;
            }
        }
    }
    // SAFETY: the export.
    let share = unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_WheelRoadLinearShare) }.clamp(0.0, 1.0);
    p.taper = speed_taper(p.phys, speed);
    let m = (p.own_max * p.taper * (share + (1.0 - share) * n) / c0.max(0.05)).clamp(0.5, p.own_max * 8.0);
    code::write::<u8>(p.phys + ALT, 0);
    code::write::<f32>(p.phys + MAX_DEG, m);
    p.written = m;
    // SAFETY: the read-out export.
    unsafe {
        CyberpunkVR_WheelRoadDebug = [1.0, p.own_max, m, n, c0, p.taper, read_f(p.phys + PHYS_ROAD, 0.0), p.own_alt as f32];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_remap_orders_its_bounds() {
        assert_eq!(remap(5.0, 0.0, 10.0, 1.0, 0.5), 0.75);
        assert_eq!(remap(5.0, 10.0, 0.0, 0.5, 1.0), 0.75);
        assert_eq!(remap(-1.0, 0.0, 10.0, 1.0, 0.5), 1.0);
        assert_eq!(remap(11.0, 0.0, 10.0, 1.0, 0.5), 0.5);
    }
}
