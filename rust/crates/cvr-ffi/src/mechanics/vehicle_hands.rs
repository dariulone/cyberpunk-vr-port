//! The owned car's panels by hand (main's mods/cet/CyberpunkVRPort_Driving vehicle_hands.lua and vehicle_gesture.lua,
//! mods/redscript/CyberpunkVRPort_Driving vrport_vehicle_hands.reds), made physical: a hand closing its grip on a door,
//! the trunk or the hood (PhysX confirms the touch on this car, the panel by its mesh's bounds, VRVehiclePartNear)
//! takes it and swings it about its hinge as far as the hand pulls -- where the hand lets go it stays. Let go past 80 %
//! open it swings fully open, under 20 % it shuts; let go while moving it carries on (a flung door slams). A free hand
//! pushing a panel (the hands are PhysX bodies) swings it too. The panel's angle is written into the car's pose
//! (cvr-rig vehicle_doors); the game's own door state follows: opened natively as the hand first takes a shut panel
//! (its animation shows the hinge and its open angle, its sound plays), closed natively as it shuts (the slam's
//! sound). Ownership, quest locks and reservations are checked on this instance, as main's script did; a hand
//! holding a weapon, a body or a person takes no panel.
use super::lobby_rtti::{self as rt, Val, A};
use super::menu_rt as r;
use crate::script_call::Value;
use cvr_host::log;
use red4ext_abi::cname::CName;
use red4ext_abi::handle::{Handle, WeakHandle};
use std::sync::Mutex;

type V3 = [f32; 3];
type V4 = [f32; 4];
const DOOR_NAMES: [&str; 6] = ["seat_front_left", "seat_front_right", "seat_back_left", "seat_back_right", "trunk", "hood"];
const REACH: f32 = 0.10;
const OPEN_SHARE: f32 = 0.8;
const SHUT_SHARE: f32 = 0.2;
const SETTLE_SPEED: f32 = 2.6;
const FRICTION: f32 = 2.2;
const SLAM_SPEED: f32 = 0.35;

fn sub(a: &V3, b: &V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: &V3, b: &V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn dot(a: &V3, b: &V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: &V3, b: &V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn len(a: &V3) -> f32 {
    dot(a, a).sqrt()
}
fn mul(a: &V3, k: f32) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn shared(i: i32) -> f32 {
    super::belt::shared(i)
}
/// The self-test's hand (CVR_DOOR_TEST): a side, where it is and whether it grips, in place of the tracked one; and
/// the test's car, taken as the player's own.
static TEST_HAND: Mutex<Option<(usize, V3, bool)>> = Mutex::new(None);
static TEST_VEHICLE: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
fn test_hand(side: usize) -> Option<(V3, bool)> {
    TEST_HAND.lock().ok()?.filter(|t| t.0 == side).map(|t| (t.1, t.2))
}
/// The player's own car (or the self-test's).
fn owned(v: &Handle) -> bool {
    r::b(v, "IsPlayerVehicle", &[]) || {
        let t = TEST_VEHICLE.load(core::sync::atomic::Ordering::Relaxed);
        t != 0 && super::melee::entity_id(v) == t
    }
}
fn native<T: crate::script_call::Ret>(name: &str, values: &[Value]) -> Option<T> {
    super::belt::native::<T>(name, values)
}

/// A panel in hand or swinging.
struct Panel {
    vehicle: WeakHandle,
    id: u64,
    part: i32,
    slot: usize,
    angle: f32,
    omega: f32,
    /// the hand that holds it, the hand's angle about the hinge when it closed and the panel's then
    held_by: Option<usize>,
    grab_vector: V3,
    grab_angle: f32,
    last_hand: Option<f32>,
    /// the game asked to close it, waiting for its own pose to be shut before the hand's pose lets go
    closing: bool,
    opened_native: bool,
    since: f64,
    /// THE HAND ON THE HANDLE: the drawn wrist as it took the panel, in the car's frame
    hold: Option<Hold>,
}
/// The drawn wrist where the hand closed on the panel -- its palm on the face PhysX stopped it at, turned as the player
/// held it -- in the car's frame, and the panel's angle then. While the hand holds the panel the wrist is kept there and
/// so, turned with the panel about its hinge: not where tracking goes on to (through the door), nor however the
/// controller turns after the grip.
#[derive(Clone, Copy)]
struct Hold {
    p: V3,
    q: V4,
    angle: f32,
}
#[derive(Default)]
struct Hand {
    grip: bool,
    blocked: bool,
    pressed_at: f64,
    last: Option<(V3, f64)>,
    velocity: V3,
}
#[derive(Default)]
struct State {
    hands: [Hand; 2],
    panels: Vec<Panel>,
    next_scan: [f64; 2],
    actions: u32,
    /// the hands held on a panel's handle (VRHandStopModel on)
    magnet: [bool; 2],
    /// the fingers on the handle's grasp: how far (0 tracking .. 1 the grasp) and where they are going
    grasp: [f64; 2],
    grasp_want: [f64; 2],
}
/// THE FINGERS CLOSE ON THE HANDLE while the hand is held on it: the grasp solver's box (as the holsters' grip) across
/// the palm -- a door handle's bar, 2 cm deep, 2.4 cm high, 11 cm long -- eased in over 0.1 s and out over 0.18 s.
fn handle_grasp(s: &mut State, side: usize, on: bool) {
    if on {
        let (p, q) = ([0.0f32, -0.004, -0.02, 1.0], [0.0f32, 0.0, 0.0, 1.0]);
        native::<bool>(
            "VRGraspSet",
            &[Value::Int32(side as i32), Value::Int32(0), Value::Float(0.010), Value::Float(0.012), Value::Float(0.055), Value::Ptr(p.as_ptr().cast()), Value::Ptr(q.as_ptr().cast())],
        );
        native::<bool>("VRGraspPin", &[Value::Int32(side as i32), Value::Bool(false)]);
    }
    s.grasp_want[side] = if on { 1.0 } else { 0.0 };
}
fn grasp_tick(s: &mut State, dt: f64) {
    for side in 0..2usize {
        let (b, w) = (s.grasp[side], s.grasp_want[side]);
        if b == w {
            continue;
        }
        let step = dt / if w > b { 0.10 } else { 0.18 };
        let b = if w > b { w.min(b + step) } else { w.max(b - step) };
        s.grasp[side] = b;
        native::<bool>("VRGraspBlend", &[Value::Int32(side as i32), Value::Float(b as f32)]);
        if b <= 0.0 {
            native::<bool>("VRGraspClear", &[Value::Int32(side as i32)]);
        }
    }
}
// SAFETY: the game thread's alone (the mechanics' frame).
unsafe impl Send for State {}
static STATE: Mutex<Option<State>> = Mutex::new(None);

fn now() -> f64 {
    super::melee::real_time()
}
fn paused(pl: &Handle) -> bool {
    if pl.is_null() {
        return true;
    }
    let Ok(rh) = super::requests_handler() else { return true };
    if rh.is_null() || r::b(&rh, "IsPreGame", &[]) || r::b(&rh, "IsGamePaused", &[]) || shared(81) > 0.5 {
        return true;
    }
    if super::blade::psm_int(pl, "SceneTier").is_none_or(|t| t >= 3.0) {
        return true;
    }
    false
}
/// A hand that may take a panel: tracked, fists or nothing (or Gorilla Arms) in its weapon slot, carrying no body.
fn free(pl: &Handle, side: usize) -> bool {
    busy(pl, side).is_none()
}
/// What keeps the hand from the car: None when it is free.
fn busy(pl: &Handle, side: usize) -> Option<&'static str> {
    if test_hand(side).is_some() {
        return None;
    }
    if shared(side as i32 * 8) <= 0.5 {
        return Some("the hand is not tracked (its shared slot)");
    }
    let item = super::belt::in_slot(pl, if side == 0 { "AttachmentSlots.WeaponLeft" } else { "AttachmentSlots.WeaponRight" });
    if !item.is_null() {
        let id = super::melee::item_id(&item);
        let is = |t: &str| rt::static_call("WeaponObject", "IsOfType", &[A::P(id.as_ptr().cast()), A::N(rt::enum_value("gamedataItemType", t))]).is_some_and(|o| Val::of(&o).bool());
        if !(is("Wea_Fists") || is("Cyb_StrongArms")) {
            return Some("a weapon in that hand's slot");
        }
    }
    super::hand_collision::holding(side).then_some("the hand holds a thing")
}
/// CVR_DOOR_TRACE=1: a grip's way to a door logged (the hand, why it is busy, each contact ray, the magnet).
fn trace() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("CVR_DOOR_TRACE").is_ok_and(|v| v == "1"))
}
/// Where tracking alone puts the hand, world space.
fn raw_world(side: usize) -> Option<V3> {
    let p = native::<V4>("VRHandRawWorld", &[Value::Int32(side as i32)])?;
    (p[3] > 0.5).then_some([p[0], p[1], p[2]])
}
/// `v` turned by `a` radians about the unit `axis` (Rodrigues).
fn rotate(v: &V3, axis: &V3, a: f32) -> V3 {
    let (s, c) = a.sin_cos();
    let k = cross(axis, v);
    let d = dot(axis, v) * (1.0 - c);
    [v[0] * c + k[0] * s + axis[0] * d, v[1] * c + k[1] * s + axis[1] * d, v[2] * c + k[2] * s + axis[2] * d]
}
fn qmul(a: &V4, b: &V4) -> V4 {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}
fn qnorm(q: V4) -> V4 {
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if n < 1e-6 {
        [0.0, 0.0, 0.0, 1.0]
    } else {
        [q[0] / n, q[1] / n, q[2] / n, q[3] / n]
    }
}
/// The turn by `a` radians about the unit `axis`, as `rotate` turns a point.
fn q_axis(axis: &V3, a: f32) -> V4 {
    let (s, c) = (a * 0.5).sin_cos();
    [axis[0] * s, axis[1] * s, axis[2] * s, c]
}
/// The rotation whose columns are the three axes.
fn from_axes(x: &V3, y: &V3, z: &V3) -> V4 {
    let (m00, m01, m02) = (x[0], y[0], z[0]);
    let (m10, m11, m12) = (x[1], y[1], z[1]);
    let (m20, m21, m22) = (x[2], y[2], z[2]);
    let tr = m00 + m11 + m22;
    qnorm(if tr > 0.0 {
        let s = (tr + 1.0).sqrt() * 2.0;
        [(m21 - m12) / s, (m02 - m20) / s, (m10 - m01) / s, 0.25 * s]
    } else if m00 > m11 && m00 > m22 {
        let s = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
        [0.25 * s, (m01 + m10) / s, (m02 + m20) / s, (m21 - m12) / s]
    } else if m11 > m22 {
        let s = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
        [(m01 + m10) / s, 0.25 * s, (m12 + m21) / s, (m02 - m20) / s]
    } else {
        let s = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
        [(m02 + m20) / s, (m12 + m21) / s, 0.25 * s, (m10 - m01) / s]
    })
}
/// The world -> model rotation of the frame pair VRWorldToModel uses (the world's axes as it carries them over).
fn world_to_model_rot() -> Option<V4> {
    let mut ax = [[0.0f32; 3]; 3];
    for (i, a) in [[1.0f32, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]].iter().enumerate() {
        let w = [a[0], a[1], a[2], 0.0f32];
        let m = native::<V4>("VRWorldDirToModel", &[Value::Ptr(w.as_ptr().cast())])?;
        if m[3] < 0.5 {
            return None;
        }
        ax[i] = [m[0], m[1], m[2]];
    }
    Some(from_axes(&ax[0], &ax[1], &ax[2]))
}
/// The drawn wrist in the world: the physics hands' solved pose, else the drawn point with tracking's turn.
fn drawn_wrist(side: usize, p: &V3) -> Option<(V3, V4)> {
    if let Some((p, q)) = cvr_rig::vrik::physhands::pose::solved_world_pose(side) {
        return Some((p, qnorm(q)));
    }
    let raw = native::<V4>("VRHandRawRot", &[Value::Int32(side as i32)])?;
    let w2m = world_to_model_rot()?;
    Some((*p, qnorm(qmul(&crate::blade_natives::conj(&w2m), &raw))))
}
/// THE WRIST'S ONE WRITER while it is on a handle (shared slot 162, as the reload's, the holsters' and the grenade's
/// holds): HandCollision writes a stop on every wrist it does not see claimed and clears it again, and the arm solve
/// draws a script's stop and rotation lock over the hand's own PhysX body only for the claimed wrist -- unclaimed, the
/// handle's hold was overwritten each frame and the hand left to the body pressing on the door.
fn claim(side: usize, on: bool) {
    if on {
        native::<()>("SetVRReloadOwnedHand", &[Value::Int32(side as i32)]);
    } else if shared(162) == side as f32 {
        native::<()>("SetVRReloadOwnedHand", &[Value::Int32(-1)]);
    }
}
fn magnet_off(side: usize) {
    let z = [0f32; 4];
    native::<bool>("VRHandStopModel", &[Value::Int32(side as i32), Value::Bool(false), Value::Ptr(z.as_ptr().cast())]);
    native::<bool>("VRHandStopRot", &[Value::Int32(side as i32), Value::Int32(0), Value::Float(0.0), Value::Float(0.0), Value::Float(0.0), Value::Float(1.0)]);
    claim(side, false);
}
fn hand_world(side: usize) -> Option<V3> {
    if let Some((p, _)) = test_hand(side) {
        return Some(p);
    }
    let p = native::<V4>("VRVehicleHandWorld", &[Value::Int32(side as i32)])?;
    (p[3] > 0.5).then_some([p[0], p[1], p[2]])
}
fn vehicle_frame(v: &Handle) -> Option<(V3, V4)> {
    let p = super::belt::m::<V4>(v, "GetWorldPosition", &[]).ok()?;
    let q = super::belt::m::<V4>(v, "GetWorldOrientation", &[]).ok()?;
    Some(([p[0], p[1], p[2]], q))
}
/// localPoint: a world point in the vehicle's frame (its model space).
fn local_point(v: &Handle, p: &V3) -> Option<V3> {
    let (b, q) = vehicle_frame(v)?;
    Some(crate::blade_natives::qrot(&crate::blade_natives::conj(&q), &sub(p, &b)))
}
fn ps(v: &Handle) -> Handle {
    rt::obj(v, "GetVehiclePS", &[])
}
fn door_state(v: &Handle, part: i32) -> f64 {
    rt::num(&ps(v), "GetDoorState", &[A::N(f64::from(part))]).unwrap_or(-1.0)
}
/// VehicleObject.VRPortCanTouchPanel: the player's own car, the panel neither detached, quest locked nor reserved.
fn can_touch(v: &Handle, part: i32) -> bool {
    if !owned(v) || !(0..6).contains(&part) {
        return false;
    }
    let ps = ps(v);
    if ps.is_null() {
        return false;
    }
    let interaction = rt::num(&ps, "GetDoorInteractionState", &[A::N(f64::from(part))]).unwrap_or(-1.0);
    door_state(v, part) != rt::enum_value("VehicleDoorState", "Detached")
        && interaction != rt::enum_value("VehicleDoorInteractionState", "QuestLocked")
        && interaction != rt::enum_value("VehicleDoorInteractionState", "Reserved")
}
/// VehicleComponent.OpenDoor / CloseDoor on the panel's mounting slot.
fn native_door(v: &Handle, part: i32, open: bool) -> bool {
    let Some(mut slot) = r::new_struct("gameMountingSlotId").or_else(|| r::new_struct("MountingSlotId")) else {
        log!("[vehicle-hands] no MountingSlotId type: the game's door untouched\n");
        return false;
    };
    if let Some(f) = r::val(&slot).f("id") {
        // SAFETY: a CName field.
        unsafe { *(f.ptr as *mut u64) = CName::hashed(DOOR_NAMES[part as usize]).0 };
    }
    let p = slot.as_mut_ptr().cast_const().cast();
    let ok = if open {
        rt::static_call("VehicleComponent", "OpenDoor", &[A::H(v), A::P(p), A::N(0.0)])
    } else {
        rt::static_call("VehicleComponent", "CloseDoor", &[A::H(v), A::P(p)])
    };
    if ok.is_none() {
        log!("[vehicle-hands] VehicleComponent.{} could not be called\n", if open { "OpenDoor" } else { "CloseDoor" });
    }
    ok.is_some_and(|o| Val::of(&o).bool())
}
/// contact: PhysX's touch on an owned car at the palm (rays 12 cm along each axis), its panel at the touch.
/// The rays a grip looks for the car along, from the drawn wrist `p`: TOWARD TRACKING FIRST -- the physical hand stops on
/// the surface it touches while tracking goes on, so that surface lies between the drawn wrist and the tracked one,
/// whatever its angle (5 cm past the tracked wrist) -- then main's six along the world's axes, 12 cm each way. Those
/// alone reach 12 cm only along an axis: a door at 45 degrees with the wrist 9-10 cm off its face (the palm on it) was
/// hit by none of them, and the grip took nothing.
fn contact_rays(p: &V3, raw: Option<V3>) -> Vec<(&'static str, V3)> {
    let mut rays = Vec::with_capacity(7);
    if let Some(r) = raw {
        let d = sub(&r, p);
        let n = len(&d);
        if n > 0.01 {
            rays.push(("toward tracking", add(p, &mul(&d, (n + 0.05) / n))));
        }
    }
    const AXES: [(&str, usize, f32); 6] = [("-x", 0, -1.0), ("+x", 0, 1.0), ("-y", 1, -1.0), ("+y", 1, 1.0), ("-z", 2, -1.0), ("+z", 2, 1.0)];
    for (name, axis, sign) in AXES {
        let mut end = *p;
        end[axis] += 0.12 * sign;
        rays.push((name, end));
    }
    rays
}
fn contact(side: usize, p: &V3, raw: Option<V3>, others: &[(u64, i32)]) -> Option<(Handle, i32, u64)> {
    let filter = rt::static_call("QueryFilter", "ALL", &[])?;
    let sq = super::game_system("GetSpatialQueriesSystem");
    let testing = test_hand(side).is_some() || trace();
    for (ray, end) in contact_rays(p, raw) {
        let (a, b) = ([p[0], p[1], p[2], 1.0f32], [end[0], end[1], end[2], 1.0f32]);
        let Some((found, outs)) = rt::method_outs(&sq, "SyncRaycastByQueryFilter", &[A::P(a.as_ptr().cast()), A::P(b.as_ptr().cast()), A::P(filter.as_ptr().cast()), A::B(false), A::B(false)], 1) else {
            if testing {
                log!("[vehicle-hands] ray {ray}: the call failed\n");
            }
            continue;
        };
        if !found.is_some_and(|o| Val::of(&o).bool()) {
            continue;
        }
        let Some(trace) = outs.first() else { continue };
        // SAFETY: a TraceResult (0x60 bytes).
        let v = crate::blade_natives::trace_entity(unsafe { &*(trace.as_ptr() as *const [u8; 0x60]) });
        if testing {
            let class = if v.is_null() { String::from("nothing") } else { super::belt::class_name(&v) };
            log!("[vehicle-hands] ray {ray}: hit {class} (owned {})\n", !v.is_null() && owned(&v));
        }
        if !r::is_a(&v, "vehicleCarBaseObject") || !owned(&v) {
            continue;
        }
        // SAFETY: the trace's position.
        let hit = unsafe { *(trace.as_ptr() as *const [f32; 3]) };
        let (part, slot) = crate::vehicle_parts::part_near(&v, &[hit[0], hit[1], hit[2], 1.0], REACH);
        let n = part[0] as i32;
        if testing {
            log!("[vehicle-hands] ray {ray}: panel {:?} at {:?}, touchable {}\n", part, hit, part[3] > 0.5 && can_touch(&v, n));
        }
        if part[3] > 0.5 && can_touch(&v, n) {
            let id = super::melee::entity_id(&v);
            if !others.contains(&(id, n)) {
                return Some((v, n, slot));
            }
        }
    }
    None
}
/// The panel's bone in the car's pose: the slot's bone (any slot component has it), in the animated object that has it.
fn door_slot(v: &Handle, slot: u64) -> Option<usize> {
    let mut bone = 0u64;
    for c in crate::engine_objects::entity_components(v) {
        if !r::is_a(&c, "entSlotComponent") {
            continue;
        }
        let Some(list) = rt::field(&c, "slots") else { continue };
        for i in 0..list.len() {
            let Some(e) = list.at(i) else { continue };
            // SAFETY: the slot's CNames.
            let (name, b) = unsafe { (*(e.f("slotName")?.ptr as *const u64), *(e.f("boneName")?.ptr as *const u64)) };
            if name == slot {
                bone = b;
                break;
            }
        }
        if bone != 0 {
            break;
        }
    }
    if bone == 0 {
        return None;
    }
    for object in crate::anim_lookup::animated_objects_for_entity(v.instance as usize) {
        let b = cvr_rig::vrik::vehicle_doors::bone_of(object, bone);
        if b >= 0 {
            return cvr_rig::vrik::vehicle_doors::register(object, b);
        }
    }
    None
}
/// The hand's angle about the panel's hinge (in the model's frame), signed by the hinge's opening axis.
fn hand_angle(v: &Handle, door: &cvr_rig::vrik::vehicle_doors::Door, p: &V3, reference: &V3) -> Option<(f32, V3)> {
    if !door.have_model {
        return None;
    }
    let local = local_point(v, p)?;
    let axis = door.model_axis;
    let n = len(&axis);
    if n < 0.5 {
        return None;
    }
    let axis = mul(&axis, 1.0 / n);
    let rel = sub(&local, &door.pivot);
    let perp = sub(&rel, &mul(&axis, dot(&rel, &axis)));
    if len(&perp) < 0.05 {
        return None;
    }
    if len(reference) < 0.01 {
        return Some((0.0, perp));
    }
    let angle = dot(&axis, &cross(reference, &perp)).atan2(dot(reference, &perp));
    Some((angle, perp))
}

fn frame(s: &mut State, pl: &Handle, dt: f32) {
    let t = now();
    let others: Vec<(u64, i32)> = s.panels.iter().filter(|p| p.held_by.is_some()).map(|p| (p.id, p.part)).collect();
    for side in 0..2 {
        let grip = test_hand(side).map_or(shared(if side == 0 { 155 } else { 49 }) > 0.5, |t| t.1);
        let hand_free = free(pl, side);
        let pos = if hand_free { hand_world(side) } else { None };
        let h = &mut s.hands[side];
        // the hand's velocity (for a push)
        if let Some(p) = pos {
            if let Some((lp, lt)) = h.last {
                let span = (t - lt) as f32;
                if span > 1e-3 && span < 0.2 {
                    let v = mul(&sub(&p, &lp), 1.0 / span);
                    let k = (span / 0.05).min(1.0);
                    h.velocity = [h.velocity[0] + (v[0] - h.velocity[0]) * k, h.velocity[1] + (v[1] - h.velocity[1]) * k, h.velocity[2] + (v[2] - h.velocity[2]) * k];
                }
            }
            h.last = Some((p, t));
        } else {
            h.last = None;
            h.velocity = [0.0; 3];
        }
        if !grip {
            h.blocked = false;
            if h.grip {
                // let go: the panel keeps its swing
                for panel in s.panels.iter_mut().filter(|p| p.held_by == Some(side)) {
                    panel.held_by = None;
                }
            }
            h.grip = false;
            native::<()>("VRVehicleHandClaim", &[Value::Int32(side as i32), Value::Bool(false)]);
            continue;
        }
        let pressed = !h.grip;
        h.grip = true;
        if pressed {
            h.pressed_at = t;
            // a grip closed before reaching the car takes nothing until it opens again (main's gesture.blocked)
            h.blocked = pos.is_none();
            if trace() {
                log!(
                    "[vehicle-hands] grip {side} pressed: hand at {:?}, busy {:?}\n",
                    hand_world(side),
                    busy(pl, side)
                );
            }
        }
        if !hand_free {
            h.blocked = true;
            for panel in s.panels.iter_mut().filter(|p| p.held_by == Some(side)) {
                panel.held_by = None;
            }
            native::<()>("VRVehicleHandClaim", &[Value::Int32(side as i32), Value::Bool(false)]);
            continue;
        }
        let holding = s.panels.iter().any(|p| p.held_by == Some(side));
        let (blocked, pressed_at) = (h.blocked, h.pressed_at);
        if !holding && !blocked && t - pressed_at <= 0.6 && t >= s.next_scan[side] {
            s.next_scan[side] = t + 0.05;
            let raw = if test_hand(side).is_some() { None } else { raw_world(side) };
            if let Some((v, part, slot)) = pos.and_then(|p| contact(side, &p, raw, &others)) {
                take(s, pl, side, &v, part, slot, pos.unwrap_or([0.0; 3]), t);
            }
        }
        let holding = s.panels.iter().any(|p| p.held_by == Some(side));
        native::<()>("VRVehicleHandClaim", &[Value::Int32(side as i32), Value::Bool(holding)]);
    }
    // each panel: driven by its hand, else swinging, pushed, settling
    let held: [bool; 2] = [0, 1].map(|side| s.panels.iter().any(|p| p.held_by == Some(side)));
    let hands_now: [(Option<V3>, V3, bool); 2] = [0, 1].map(|side| {
        // a hand holding a panel turns it from where tracking puts it: its drawn hand is held on the handle
        let at = if held[side] { raw_world(side).or_else(|| hand_world(side)) } else { hand_world(side) };
        (at.filter(|_| free(pl, side)), s.hands[side].velocity, s.hands[side].grip)
    });
    let mut magnet_want: [Option<(V3, V4)>; 2] = [None, None];
    let mut done = Vec::new();
    for (i, panel) in s.panels.iter_mut().enumerate() {
        let v = panel.vehicle.lock();
        let Some(door) = cvr_rig::vrik::vehicle_doors::get(panel.slot) else {
            done.push(i);
            continue;
        };
        if v.is_null() || !r::b(&v, "IsAttached", &[]) || !owned(&v) {
            done.push(i);
            continue;
        }
        // the game closed it itself (the player getting in, a scene): its pose its own again
        let open = rt::enum_value("VehicleDoorState", "Open");
        if !panel.closing && panel.opened_native && door_state(&v, panel.part) != open && t - panel.since > 1.0 {
            done.push(i);
            continue;
        }
        let max = door.open_max.max(0.0);
        let before = panel.angle;
        if let Some(side) = panel.held_by {
            match hands_now[side].0.and_then(|p| hand_angle(&v, &door, &p, &panel.grab_vector).map(|a| (a, p))) {
                Some(((a, perp), p)) if len(&panel.grab_vector) < 0.01 => {
                    // the hinge has just become known: the hand's direction from it is the reference
                    panel.grab_vector = perp;
                    panel.grab_angle = panel.angle;
                    let _ = (a, p);
                }
                Some(((a, _), _)) => {
                    let wanted = (panel.grab_angle + a).clamp(0.0, if max > 0.0 { max } else { 0.0 });
                    panel.angle = wanted;
                    if dt > 1e-4 {
                        let w = (panel.angle - before) / dt;
                        panel.omega += (w - panel.omega) * (dt / 0.06).min(1.0);
                    }
                    panel.last_hand = Some(a);
                }
                None => {
                    if hands_now[side].0.is_none() {
                        panel.held_by = None;
                    }
                }
            }
        } else {
            // a free hand pushing it: the hand's speed along the panel's swing at its distance from the hinge
            for side in 0..2 {
                let (Some(p), vel, grip) = hands_now[side] else { continue };
                if grip {
                    continue;
                }
                let (part, _) = crate::vehicle_parts::part_near(&v, &[p[0], p[1], p[2], 1.0], 0.05);
                if part[3] < 0.5 || part[0] as i32 != panel.part || !door.have_model {
                    continue;
                }
                let (Some(local), Some((_, q))) = (local_point(&v, &p), vehicle_frame(&v)) else { continue };
                let axis = door.model_axis;
                let rel = sub(&local, &door.pivot);
                let radius = len(&sub(&rel, &mul(&axis, dot(&rel, &axis))));
                if radius < 0.1 {
                    continue;
                }
                let lv = crate::blade_natives::qrot(&crate::blade_natives::conj(&q), &vel);
                // the swing's direction at the hand: axis x radial
                let swing = cross(&axis, &rel);
                let sl = len(&swing);
                if sl < 1e-3 {
                    continue;
                }
                let along = dot(&lv, &mul(&swing, 1.0 / sl));
                if along.abs() > 0.25 {
                    let w = along / radius;
                    // a push only adds: the hand pressing on the panel's face drives it its way
                    if w * panel.omega <= 0.0 || w.abs() > panel.omega.abs() {
                        panel.omega = w;
                    }
                }
            }
            // where it was left: 80 % open swings it fully open, 20 % shuts it
            if max > 0.0 {
                let share = panel.angle / max;
                if share >= OPEN_SHARE && panel.omega >= -0.2 {
                    panel.omega = panel.omega.max(SETTLE_SPEED);
                } else if share <= SHUT_SHARE && panel.omega <= 0.2 && !panel.closing {
                    panel.omega = panel.omega.min(-SETTLE_SPEED);
                }
            }
            panel.angle += panel.omega * dt;
            panel.omega *= (-FRICTION * dt).exp();
            if panel.omega.abs() < 0.02 {
                panel.omega = 0.0;
            }
        }
        if panel.angle >= max && max > 0.0 {
            panel.angle = max;
            panel.omega = panel.omega.min(0.0);
        }
        if panel.angle <= 0.0 {
            panel.angle = 0.0;
            let slam = panel.omega < -SLAM_SPEED;
            panel.omega = 0.0;
            if panel.held_by.is_none() && !panel.closing {
                // shut: the game's own close (its slam), the pose held shut until the game's is
                if native_door(&v, panel.part, false) || slam {
                    s.actions += 1;
                }
                panel.closing = true;
                log!("[vehicle-hands] panel {} shut{} (from Rust)\n", panel.part, if slam { " with a slam" } else { "" });
            }
        } else if panel.closing && panel.angle > 0.01 {
            // grabbed again while closing: open once more
            panel.closing = false;
            panel.opened_native = native_door(&v, panel.part, true);
            panel.since = t;
        }
        if panel.closing && door.graph_angle < 1f32.to_radians() {
            done.push(i);
            continue;
        }
        cvr_rig::vrik::vehicle_doors::set(panel.slot, panel.angle, true);
        // the holding hand drawn on the handle, turned with the panel about its hinge
        if let (Some(side), Some(hold), true) = (panel.held_by, panel.hold, door.have_model) {
            let n = len(&door.model_axis);
            if n > 0.5 {
                let axis = mul(&door.model_axis, 1.0 / n);
                let turn = panel.angle - hold.angle;
                let local = add(&door.pivot, &rotate(&sub(&hold.p, &door.pivot), &axis, turn));
                if let Some((b, q)) = vehicle_frame(&v) {
                    let w = add(&b, &crate::blade_natives::qrot(&q, &local));
                    magnet_want[side] = Some((w, qnorm(qmul(&q, &qmul(&q_axis(&axis, turn), &hold.q)))));
                }
            }
        }
    }
    for side in 0..2 {
        match magnet_want[side] {
            Some((world, wq)) => {
                let w = [world[0], world[1], world[2], 1.0f32];
                let (Some(m), Some(w2m)) = (native::<V4>("VRWorldToModel", &[Value::Ptr(w.as_ptr().cast())]), world_to_model_rot()) else { continue };
                if m[3] < 0.5 {
                    continue;
                }
                let mq = qnorm(qmul(&w2m, &wq));
                let mv = [m[0], m[1], m[2], 1.0f32];
                claim(side, true);
                native::<bool>("VRHandStopModel", &[Value::Int32(side as i32), Value::Bool(true), Value::Ptr(mv.as_ptr().cast())]);
                native::<bool>("VRHandStopRot", &[Value::Int32(side as i32), Value::Int32(1), Value::Float(mq[0]), Value::Float(mq[1]), Value::Float(mq[2]), Value::Float(mq[3])]);
                if !s.magnet[side] {
                    log!("[vehicle-hands] hand {side} held on the handle at {world:?}, turned {wq:?} (from Rust)\n");
                    handle_grasp(s, side, true);
                }
                s.magnet[side] = true;
            }
            None if s.magnet[side] => {
                magnet_off(side);
                s.magnet[side] = false;
                handle_grasp(s, side, false);
            }
            None => {}
        }
    }
    grasp_tick(s, f64::from(dt));
    for &i in done.iter().rev() {
        let p = s.panels.remove(i);
        cvr_rig::vrik::vehicle_doors::set(p.slot, 0.0, false);
        cvr_rig::vrik::vehicle_doors::release(p.slot);
    }
}
#[allow(clippy::too_many_arguments)]
fn take(s: &mut State, _pl: &Handle, side: usize, v: &Handle, part: i32, slot: u64, p: V3, t: f64) {
    let id = super::melee::entity_id(v);
    let hold_at = |angle: f32| -> Option<Hold> {
        let (wp, wq) = drawn_wrist(side, &p)?;
        let (b, q) = vehicle_frame(v)?;
        let iq = crate::blade_natives::conj(&q);
        Some(Hold { p: crate::blade_natives::qrot(&iq, &sub(&wp, &b)), q: qnorm(qmul(&iq, &wq)), angle })
    };
    if let Some(panel) = s.panels.iter_mut().find(|x| x.id == id && x.part == part) {
        panel.held_by = Some(side);
        panel.grab_vector = [0.0; 3];
        panel.omega = 0.0;
        panel.hold = hold_at(panel.angle);
        haptic(side, 0.18, 20.0);
        return;
    }
    let Some(door_slot) = door_slot(v, slot) else {
        log!("[vehicle-hands] panel {part}: no bone for slot {} (from Rust)\n", CName(slot).text());
        return;
    };
    let open = rt::enum_value("VehicleDoorState", "Open");
    let was_open = door_state(v, part) == open;
    let door = cvr_rig::vrik::vehicle_doors::get(door_slot);
    let start = if was_open { door.map_or(0.0, |d| d.graph_angle) } else { 0.0 };
    let opened_native = was_open || native_door(v, part, true);
    log!("[vehicle-hands] the game's door {part}: was open {was_open}, opened natively {opened_native}, its state now {}
", door_state(v, part));
    s.panels.push(Panel {
        vehicle: WeakHandle::from_handle(v),
        id,
        part,
        slot: door_slot,
        angle: start,
        omega: 0.0,
        held_by: Some(side),
        grab_vector: [0.0; 3],
        grab_angle: start,
        last_hand: None,
        closing: false,
        opened_native,
        since: t,
        hold: hold_at(start),
    });
    cvr_rig::vrik::vehicle_doors::set(door_slot, start, true);
    s.actions += 1;
    haptic(side, 0.4, 35.0);
    log!("[vehicle-hands] panel {part} ({}) taken by hand {side}, {} (from Rust)\n", CName(slot).text(), if was_open { "open" } else { "opening" });
}
fn haptic(side: usize, strength: f32, ms: f32) {
    native::<()>("SetVRHapticPulse", &[Value::Int32(side as i32), Value::Float(strength), Value::Float(ms)]);
}
/// The mechanics' frame (the Driving mod's onUpdate).
pub(in crate::mechanics) fn tick(dt: f32) {
    let mut g = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let s = g.get_or_insert_with(State::default);
    let pl = super::belt::player();
    if paused(&pl) || r::b(&pl, "IsDead", &[]) || super::lobby_rtti::static_call("VehicleComponent", "IsMountedToVehicle", &[A::P((&rt::game() as *const crate::script_call::GameInstance).cast()), A::H(&pl)]).is_some_and(|o| Val::of(&o).bool()) {
        clear(s);
        return;
    }
    frame(s, &pl, dt.clamp(0.0, 0.1));
    let summary: Vec<(i32, f32, f32, bool, bool)> = s
        .panels
        .iter()
        .map(|p| (p.part, p.angle, cvr_rig::vrik::vehicle_doors::get(p.slot).map_or(0.0, |d| d.open_max), p.held_by.is_some(), p.closing))
        .collect();
    drop(g);
    self_test(&pl, &summary);
}
fn clear(s: &mut State) {
    for side in 0..2 {
        s.hands[side] = Hand::default();
        s.next_scan[side] = 0.0;
        native::<()>("VRVehicleHandClaim", &[Value::Int32(side as i32), Value::Bool(false)]);
        if std::mem::take(&mut s.magnet[side]) {
            magnet_off(side);
        }
        // the fingers given back at once: a load screen or getting in leaves no ease to run
        if std::mem::take(&mut s.grasp[side]) > 0.0 {
            native::<bool>("VRGraspClear", &[Value::Int32(side as i32)]);
        }
        s.grasp_want[side] = 0.0;
    }
    for p in s.panels.drain(..) {
        cvr_rig::vrik::vehicle_doors::set(p.slot, 0.0, false);
        cvr_rig::vrik::vehicle_doors::release(p.slot);
    }
}
/// The Driving mod's onShutdown.
pub(in crate::mechanics) fn shutdown() {
    let mut g = STATE.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(s) = g.as_mut() {
        clear(s);
    }
}

/// The doors' self-test (CVR_DOOR_TEST=1): a car spawned beside the player (taken as the player's own), its front left
/// door taken by a hand at its outer face, swung 40 degrees about its hinge and let go (it must stay), then swung back
/// fast and let go (it must slam shut and be given back to the game); each step logged, the car deleted.
struct DoorTest {
    frames: u32,
    stage: u8,
    spawned: u64,
    wait: u32,
    start: Option<(V3, f32)>,
    hand: V3,
    log_at: u32,
}
static DOOR_TEST: Mutex<DoorTest> = Mutex::new(DoorTest { frames: 0, stage: 0, spawned: 0, wait: 0, start: None, hand: [0.0; 3], log_at: 0 });
fn set_test_hand(h: Option<(usize, V3, bool)>) {
    if let Ok(mut t) = TEST_HAND.lock() {
        *t = h;
    }
}
fn self_test(pl: &Handle, panels: &[(i32, f32, f32, bool, bool)]) {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ON.get_or_init(|| std::env::var("CVR_DOOR_TEST").is_ok_and(|v| v == "1")) || pl.is_null() || !r::b(pl, "IsAttached", &[]) {
        return;
    }
    let mut t = DOOR_TEST.lock().unwrap_or_else(|p| p.into_inner());
    t.frames += 1;
    let car = if t.spawned != 0 { crate::spawn_system::spawned(t.spawned) } else { Handle::null() };
    let door = panels.iter().find(|p| p.0 == 0).copied();
    match t.stage {
        0 if t.frames >= 600 => {
            let at = super::belt::m::<V4>(pl, "GetWorldPosition", &[]).unwrap_or([0.0; 4]);
            let f = super::belt::m::<V4>(pl, "GetWorldForward", &[]).unwrap_or([0.0, 1.0, 0.0, 0.0]);
            let right = [f[1], -f[0]];
            let pos = [at[0] + right[0] * 2.6 + f[0] * 0.5, at[1] + right[1] * 2.6 + f[1] * 0.5, at[2], 1.0];
            let yaw = (-f[0]).atan2(f[1]);
            let q = [0.0, 0.0, (yaw * 0.5).sin(), (yaw * 0.5).cos()];
            t.spawned = crate::spawn_system::spawn_record(red4ext_abi::tweakdbid::from_name("Vehicle.v_standard2_archer_hella_player"), &pos, &q);
            log!("[door-test] a car spawned: {}\n", t.spawned);
            t.stage = 1;
        }
        1 => {
            t.wait += 1;
            if car.is_null() || t.wait < 500 {
                if t.wait > 1500 {
                    log!("[door-test] no car: stopped\n");
                    t.stage = 9;
                }
                return;
            }
            TEST_VEHICLE.store(super::melee::entity_id(&car), core::sync::atomic::Ordering::Relaxed);
            let Some((c, n, half)) = crate::vehicle_parts::part_face(&car, 0) else {
                log!("[door-test] the car has no front left door panel: stopped\n");
                t.stage = 9;
                return;
            };
            // the hand 3 cm off the door's outer face, gripping
            let hand = [c[0] + n[0] * (half + 0.03), c[1] + n[1] * (half + 0.03), c[2] + n[2] * (half + 0.03)];
            let probe = crate::vehicle_parts::part_near(&car, &[hand[0], hand[1], hand[2], 1.0], REACH);
            log!("[door-test] the door's face at {:?} (normal {:?}, half {half:.3}); the panel there: {:?}\n", c, n, probe.0);
            t.hand = hand;
            set_test_hand(Some((1, hand, true)));
            t.stage = 2;
            t.wait = 0;
        }
        2 => {
            t.wait += 1;
            let Some((_, angle, max, held, _)) = door else {
                if t.wait > 120 {
                    log!("[door-test] the hand took no panel: stopped\n");
                    set_test_hand(None);
                    t.stage = 9;
                }
                return;
            };
            let Some(d) = cvr_rig::vrik::vehicle_doors::get(0).filter(|d| d.have_model).or_else(|| (1..12).find_map(|i| cvr_rig::vrik::vehicle_doors::get(i).filter(|d| d.have_model))) else { return };
            // swing the hand 40 degrees about the hinge over a second, once the hinge is known
            let Some((b, q)) = vehicle_frame(&car) else { return };
            let hand0 = t.hand;
            let start = *t.start.get_or_insert_with(|| (crate::blade_natives::qrot(&crate::blade_natives::conj(&q), &sub(&hand0, &b)), 0.0));
            let progress = ((t.wait as f32) / 90.0).min(1.0);
            let a = 40f32.to_radians() * progress;
            let axis = d.model_axis;
            let rel = sub(&start.0, &d.pivot);
            // Rodrigues about the hinge
            let (sn, cs) = a.sin_cos();
            let k = dot(&axis, &rel);
            let c3 = cross(&axis, &rel);
            let turned = [0, 1, 2].map(|i| rel[i] * cs + c3[i] * sn + axis[i] * k * (1.0 - cs));
            let local = [d.pivot[0] + turned[0], d.pivot[1] + turned[1], d.pivot[2] + turned[2]];
            let r3 = crate::blade_natives::qrot(&q, &local);
            let world = [r3[0] + b[0], r3[1] + b[1], r3[2] + b[2]];
            set_test_hand(Some((1, world, progress < 1.0)));
            if t.wait % 30 == 1 {
                log!("[door-test] swinging: hand at {:.0} deg, door {:.1} of {:.1} deg, held {held}\n", a.to_degrees(), angle.to_degrees(), max.to_degrees());
            }
            if progress >= 1.0 {
                t.hand = world;
                t.stage = 3;
                t.wait = 0;
            }
        }
        3 => {
            t.wait += 1;
            set_test_hand(Some((1, t.hand, false)));
            if t.wait == 90 {
                if let Some((_, angle, max, held, closing)) = door {
                    log!("[door-test] let go 1.5 s ago: door {:.1} of {:.1} deg (stays: {}), held {held}, closing {closing}\n", angle.to_degrees(), max.to_degrees(), angle > 0.15 * max && angle < 0.85 * max);
                } else {
                    log!("[door-test] let go 1.5 s ago: the panel is gone\n");
                }
                // grab it again where it is and fling it shut
                set_test_hand(Some((1, t.hand, true)));
                t.stage = 4;
                t.wait = 0;
                t.start = None;
            }
        }
        4 => {
            t.wait += 1;
            let Some(d) = (0..12).find_map(|i| cvr_rig::vrik::vehicle_doors::get(i).filter(|d| d.have_model)) else { return };
            let Some((b, q)) = vehicle_frame(&car) else { return };
            let hand0 = t.hand;
            let start = *t.start.get_or_insert_with(|| (crate::blade_natives::qrot(&crate::blade_natives::conj(&q), &sub(&hand0, &b)), 0.0));
            let progress = ((t.wait as f32) / 9.0).min(1.0);
            let a = -30f32.to_radians() * progress;
            let axis = d.model_axis;
            let rel = sub(&start.0, &d.pivot);
            let (sn, cs) = a.sin_cos();
            let k = dot(&axis, &rel);
            let c3 = cross(&axis, &rel);
            let turned = [0, 1, 2].map(|i| rel[i] * cs + c3[i] * sn + axis[i] * k * (1.0 - cs));
            let local = [d.pivot[0] + turned[0], d.pivot[1] + turned[1], d.pivot[2] + turned[2]];
            let r3 = crate::blade_natives::qrot(&q, &local);
            set_test_hand(Some((1, [r3[0] + b[0], r3[1] + b[1], r3[2] + b[2]], progress < 1.0)));
            if progress >= 1.0 {
                t.stage = 5;
                t.wait = 0;
            }
        }
        5 => {
            t.wait += 1;
            if t.wait % 20 == 1 || t.wait == 120 {
                log!("[door-test] flung shut, {} frames: {:?} (door state {})\n", t.wait, door.map(|d| (d.1.to_degrees(), d.4)), door_state(&car, 0));
            }
            if t.wait >= 120 {
                set_test_hand(None);
                log!("[door-test] the car deleted: {}\n", crate::spawn_system::delete_spawned(t.spawned));
                log!("[door-test] done\n");
                t.stage = 9;
            }
        }
        _ => {}
    }
    let _ = t.log_at;
}
