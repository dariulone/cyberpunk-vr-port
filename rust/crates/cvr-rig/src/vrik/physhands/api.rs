//! The physics hands' outward face: what the other threads and modules ask of them (the body's push and pull for the
//! player's move, the held thing's filter for HeldBody, the gun's boxes and the people's capsules from the hand
//! collision, the test stack), and the natives the scripts call (VRPhysHand*).
use super::body::{hand_filter, hand_filter_static};
use super::census::filter_ready;
use super::grab::{grabbed_person, set_grab_style, BODY_PULLS, BODY_PUSHES};
use super::loose::{taken_from_stack, StackAsk, STACK_ASKS};
use super::math::qnorm;
use super::people::{self_shape, set_people, set_person, set_self_pad};
use super::px::{self, P};
use super::settings;
use super::shapes::{weapon_staged, WEAPONS};
use super::tick_ms;
use core::sync::atomic::{AtomicI32, AtomicU32, Ordering::*};
use cvr_settings::lc;
use red4ext_abi::dynarray::DynArray;
use red4ext_abi::stack::StackFrame;

/// Taken: HeldBody, on the physics thread, as a hand starts to steer a loose thing by its body -- what stood on it
/// comes down.
pub unsafe fn taken(a: P) {
    taken_from_stack(a);
}
/// FilterAgain: after changing an actor's filter (physics thread, between steps), its pairs filtered again.
pub unsafe fn filter_again(sc: P, a: P) {
    if px::prove() && !sc.is_null() && !a.is_null() {
        px::reset_filtering(sc, a);
    }
}
/// HeldAlone: the filter a held thing's shape gets while held (from its own) -- the hands' own group, which no mask in
/// the game has, and the world's mask: no loose thing reaches for it, it reaches for none, and the hands do not meet
/// it; the game's callback bits kept, so it still sounds and breaks on the world. None: leave it as it is.
pub unsafe fn held_alone(original: &[u32; 4]) -> Option<[u32; 4]> {
    let s = settings::current();
    if s.held_alone == 0 || !filter_ready {
        return None;
    }
    Some([hand_filter[0], hand_filter[1], hand_filter_static[2], hand_filter_static[3] | (original[3] & 0xC000_0000)])
}
static STACK_SERIAL: AtomicU32 = AtomicU32::new(0);
/// TestStack: a test tool -- the few light loose things nearest this point of the game's world stood one on another
/// (next step); `dump` logs the loose things near instead, `dump_shapes` every shape near.
pub fn test_stack(world_at: &[f32; 3], dump: bool, dump_shapes: bool) {
    STACK_ASKS.write(&StackAsk { at: *world_at, at_ms: tick_ms(), serial: STACK_SERIAL.fetch_add(1, Relaxed) + 1, dump, shapes: dump_shapes });
}
static WEAPON_SAID: AtomicI32 = AtomicI32::new(0);
/// SetWeapon: the gun in a hand -- `count` boxes (each set before by set_weapon_part, corners in the gun's frame), the
/// gun's frame in the hand bone's (rotation x y z w, then position). Count 0: none. Script thread.
pub unsafe fn set_weapon(side: usize, count: i32, frame: Option<&[f32; 7]>) -> bool {
    if side > 1 {
        return false;
    }
    let w = &mut weapon_staged[side];
    let n = count.clamp(0, super::body::MOST_WEAPON_BOXES as i32);
    if n != w.n && WEAPON_SAID.fetch_add(1, Relaxed) < 24 {
        cvr_host::vlog!(
            "[physhands] the gun in the {} hand: {} boxes, its grip at ({:.3} {:.3} {:.3}) in the hand bone's frame\n",
            if side == 1 { "right" } else { "left" },
            count,
            frame.map_or(0.0, |f| f[4]),
            frame.map_or(0.0, |f| f[5]),
            frame.map_or(0.0, |f| f[6])
        );
    }
    w.n = n;
    if let Some(f) = frame {
        w.frame = *f;
    }
    let mut q = [w.frame[0], w.frame[1], w.frame[2], w.frame[3]];
    if w.n > 0 && !qnorm(&mut q) {
        w.n = 0;
    } else if w.n > 0 {
        w.frame[..4].copy_from_slice(&q);
    }
    w.at = tick_ms();
    WEAPONS[side].write(w);
    true
}
/// SetWeaponPart: one box of the gun in that hand, its corners in the gun's own frame.
pub unsafe fn set_weapon_part(side: usize, index: usize, lo: &[f32; 3], hi: &[f32; 3]) -> bool {
    if side > 1 || index >= super::body::MOST_WEAPON_BOXES {
        return false;
    }
    for k in 0..3 {
        if !(lo[k] + hi[k]).is_finite() || hi[k] < lo[k] || hi[k] - lo[k] > 2.0 {
            return false;
        }
        weapon_staged[side].lo[index][k] = lo[k];
        weapon_staged[side].hi[index][k] = hi[k];
    }
    true
}
/// Tracing: trace=1 (the finger layer's diagnostics).
pub fn tracing() -> bool {
    settings::read().is_some_and(|s| s.trace != 0)
}
/// FingerHingeOnly: physhands.ini finger_hinge_only -- the fingers' joints drawn as hinges while they lie on something.
pub fn finger_hinge_only() -> bool {
    settings::read().is_none_or(|s| s.finger_hinge_only != 0)
}
fn hands_live() -> bool {
    lc!(xrHandPhysics) != 0 && lc!(xrHandCollision) != 0
}
/// BodyPull: the velocity the hands that hold on (climb) give the player's body so that tracking comes back to where they
/// took hold (world x y z, m/s). None when no hand holds on. Any thread.
pub fn body_pull() -> Option<[f32; 3]> {
    if !hands_live() || (!super::on(0) && !super::on(1)) {
        return None;
    }
    let p = BODY_PULLS.read()?;
    (p.on && tick_ms().wrapping_sub(p.at) <= 100 && (p.v[0] + p.v[1] + p.v[2]).is_finite()).then_some(p.v)
}
/// BodyPush: the velocity the hands held off by the world give the player's body (world x y, m/s; Bonelab's -F on the
/// body). None when there is none. Any thread.
pub fn body_push() -> Option<[f32; 2]> {
    if !hands_live() || (!super::on(0) && !super::on(1)) {
        return None;
    }
    let p = BODY_PUSHES.read()?;
    (tick_ms().wrapping_sub(p.at) <= 100 && p.v[0].abs() + p.v[1].abs() > 1e-4 && (p.v[0] + p.v[1]).is_finite()).then_some(p.v)
}

// ---- the natives
type Frame = StackFrame;
macro_rules! params {
    ($frame:expr $(, $value:expr)*) => {
        if let Some(frame) = $frame.as_mut() {
            $(frame.get_parameter(&mut $value);)*
            frame.finish_params();
        }
    };
}
unsafe fn put<T>(out: *mut T, value: T) {
    if !out.is_null() {
        *out = value;
    }
}
/// VRPhysHandWeapon(side, count, at, turn) -> Bool: the gun in that hand (0 left, 1 right) is `count` boxes (given before
/// by VRPhysHandWeaponPart), its frame at `at` (x y z) turned by `turn` (x y z w) in the hand bone's frame; count 0: none.
/// Said again at least every second while it is held: a gun not heard of for 1.5 s is gone.
#[no_mangle]
pub unsafe extern "C" fn VRPhysHandWeapon(_this: *mut core::ffi::c_void, frame: *mut Frame, out: *mut bool, _: i64) {
    let (mut side, mut count, mut at, mut turn) = (0i32, 0i32, [0.0f32; 4], [0.0f32; 4]);
    params!(frame, side, count, at, turn);
    let f = [turn[0], turn[1], turn[2], turn[3], at[0], at[1], at[2]];
    put(out, side >= 0 && set_weapon(side as usize, count, Some(&f)));
}
/// VRPhysHandsPerson(index, a, b) -> Bool: one capsule of the people near the hands, between a and b (the game's world),
/// its radius in a.w; b.w the script's id for this part of this person (person * 32 + part), 0 for a part not to be
/// taken hold of. VRPhysHandsPeople(count) hands the first `count` over.
#[no_mangle]
pub unsafe extern "C" fn VRPhysHandsPerson(_this: *mut core::ffi::c_void, frame: *mut Frame, out: *mut bool, _: i64) {
    let (mut index, mut a, mut b) = (0i32, [0.0f32; 4], [0.0f32; 4]);
    params!(frame, index, a, b);
    let owner = if b[3].is_finite() && b[3] > 0.5 && b[3] < 1.6e7 { (b[3] + 0.5) as i32 } else { 0 };
    put(out, index >= 0 && set_person(index as usize, &[a[0], a[1], a[2]], &[b[0], b[1], b[2]], a[3], owner));
}
/// VRPhysHandsSelfShape(index, end) -> Vector4: V's own body as the hands meet it, capsule `index` (the game's world):
/// end 0 its first end with its radius in w, end 1 its other end with the hands that meet it in w; w -1 past the last.
#[no_mangle]
pub unsafe extern "C" fn VRPhysHandsSelfShape(_this: *mut core::ffi::c_void, frame: *mut Frame, out: *mut [f32; 4], _: i64) {
    let (mut index, mut end) = (0i32, 0i32);
    params!(frame, index, end);
    let mut v = [0.0, 0.0, 0.0, -1.0];
    if let Some((a, b, r, meets)) = (index >= 0).then(|| self_shape(index as usize)).flatten() {
        let p = if end != 0 { b } else { a };
        v = [p[0], p[1], p[2], if end != 0 { meets as f32 } else { r }];
    }
    put(out, v);
}
/// VRPhysHandsGrab(side) -> Vector4: the person's part that hand holds (the script's id in w, 0 none) and the
/// controller's place off the hold (x y z, the game's world).
#[no_mangle]
pub unsafe extern "C" fn VRPhysHandsGrab(_this: *mut core::ffi::c_void, frame: *mut Frame, out: *mut [f32; 4], _: i64) {
    let mut side = 0i32;
    params!(frame, side);
    let (owner, pull) = if side >= 0 { grabbed_person(side as usize) } else { (0, [0.0; 3]) };
    put(out, [pull[0], pull[1], pull[2], owner as f32]);
}
/// VRPhysHandsSelfPad(metres) -> Bool: how much V's clothes add round V's own trunk for the hands, 0 to 4 cm.
#[no_mangle]
pub unsafe extern "C" fn VRPhysHandsSelfPad(_this: *mut core::ffi::c_void, frame: *mut Frame, out: *mut bool, _: i64) {
    let mut metres = 0.0f32;
    params!(frame, metres);
    set_self_pad(metres);
    put(out, true);
}
/// VRPhysHandsGrabStyle(side, owner, style) -> Bool: the grasp the person's part `owner` asks for while that hand holds
/// it: 1 a can, 2 a can with the fingers wide, 3 gently, 4 the palm; 0 the fingers as they lie.
#[no_mangle]
pub unsafe extern "C" fn VRPhysHandsGrabStyle(_this: *mut core::ffi::c_void, frame: *mut Frame, out: *mut bool, _: i64) {
    let (mut side, mut owner, mut style) = (-1i32, 0i32, 0i32);
    params!(frame, side, owner, style);
    if side == 0 || side == 1 {
        set_grab_style(side as usize, owner, style);
    }
    put(out, side == 0 || side == 1);
}
/// VRPhysHandFrame(side) -> array<Vector4>: the script's rows of the drawn wrist (script_hand_frame).
#[no_mangle]
pub unsafe extern "C" fn VRPhysHandFrame(_this: *mut core::ffi::c_void, frame: *mut Frame, out: *mut DynArray<[f32; 4]>, _: i64) {
    let mut side = -1i32;
    params!(frame, side);
    let Some(out) = out.as_mut() else { return };
    out.clear();
    if let Some(rows) = (side >= 0).then(|| super::pose::script_hand_frame(side as usize)).flatten() {
        for row in rows {
            out.push(row);
        }
    }
}
/// VRPhysHeldBody(side) -> array<Uint64>: [active, stable proxy ID, body index, age ms, exact anchor error micrometres];
/// u64::MAX is invalid contact data.
#[no_mangle]
pub unsafe extern "C" fn VRPhysHeldBody(_this: *mut core::ffi::c_void, frame: *mut Frame, out: *mut DynArray<u64>, _: i64) {
    let mut side = -1i32;
    params!(frame, side);
    let Some(out) = out.as_mut() else { return };
    out.clear();
    let fields = if side >= 0 { super::adjust::held_body_state(side as usize) } else { [0, 0, 0, 0, cvr_physhands::held_grip_contact::INVALID] };
    for v in fields {
        out.push(v);
    }
}
/// VRPhysHandsPeople(count) -> Bool: the first `count` people's capsules handed over.
#[no_mangle]
pub unsafe extern "C" fn VRPhysHandsPeople(_this: *mut core::ffi::c_void, frame: *mut Frame, out: *mut bool, _: i64) {
    let mut count = 0i32;
    params!(frame, count);
    set_people(count);
    put(out, true);
}
/// VRPhysHandsTestStack(at) -> Bool: a test tool -- the few light loose things nearest `at` stood one on another and put
/// to sleep, the next PhysX step (w < 0: the loose things near logged instead; w <= -2: every shape).
#[no_mangle]
pub unsafe extern "C" fn VRPhysHandsTestStack(_this: *mut core::ffi::c_void, frame: *mut Frame, out: *mut bool, _: i64) {
    let mut at = [0.0f32; 4];
    params!(frame, at);
    test_stack(&[at[0], at[1], at[2]], at[3] < 0.0, at[3] <= -2.0);
    put(out, true);
}
/// VRPhysHandWeaponPart(side, index, lo, hi) -> Bool: one box of the gun in that hand, its corners in the gun's own frame.
#[no_mangle]
pub unsafe extern "C" fn VRPhysHandWeaponPart(_this: *mut core::ffi::c_void, frame: *mut Frame, out: *mut bool, _: i64) {
    let (mut side, mut index, mut lo, mut hi) = (0i32, 0i32, [0.0f32; 4], [0.0f32; 4]);
    params!(frame, side, index, lo, hi);
    put(out, side >= 0 && index >= 0 && set_weapon_part(side as usize, index as usize, &[lo[0], lo[1], lo[2]], &[hi[0], hi[1], hi[2]]));
}
