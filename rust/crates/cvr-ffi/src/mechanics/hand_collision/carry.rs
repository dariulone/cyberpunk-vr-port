//! CARRYING (main's HandCollision K.*): a grip that closes at a loose thing takes that body: held by its own body
//! (cvr-rig's heldbody: the thing's PhysX actor stays dynamic and is given, just before each PhysX step, the velocities
//! that bring it to where the drawn hand has it, as far as a hand's force can), else driven after the hand by velocity,
//! so it stops at what it runs into. What a hand can do is limited by how much the thing weighs, as the change of speed
//! it may give it per second; two hands count as much more than double. A throw is the peak of the swing. What was woken
//! goes back to sleep for good once it has lain still. A BODY LYING DEAD is taken by its limb: held by the hand as a
//! pistol is (VRHandHoldBody), the rest of it hanging from it and towed (VRCorpseHold); its ragdoll goes back to rest as
//! a prop does. A PERSON'S PART a physics hand took is pulled by that part's own bones (cvr-rig's npc_grip). WEIGHT
//! SLOWS THE PLAYER, and carrying a dead body is the game's own Carrying.
use super::people::NpcRef;
use super::{add, conj, data, len, len2, mul, nat, nat_handle, qmul, qrot, sub, v4, vp, Frame, State, Q, V3};
use crate::script_call::Value;
use cvr_host::log;
use red4ext_abi::cname::CName;
use red4ext_abi::handle::Handle;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

// ---------------------------------------------------------------- the numbers (main's K)
const WORLD_PRESETS: [&str; 3] = ["Sight Blocker", "Bullet", "Camera"];
const HV_TAU: f32 = 0.06;
const HF_TAU: f32 = 0.02;
/// m from the body's axis before a dynamic ray: the player's capsule is dynamic to a query
const FINGER_FROM_BODY: f32 = 0.30;
/// m, m round the hand, s before asked again, s the closed hand waits for it to appear
const DEAD_RANGE: f32 = 2.5;
const DEAD_NEAR: f32 = 1.2;
const DEAD_AGAIN: f64 = 4.0;
const DEAD_WAIT: f64 = 0.8;
const CORPSE_LOST: f32 = 3.0;
/// s between asking whether one at rest is still there
const RAGDOLL_REST_EVERY: f64 = 5.0;
/// m/s of its fastest limb; s between looks; s after it was told to sleep; s a ragdoll asked for and never taken stays
const RAGDOLL_SPEED: f32 = 0.1;
const RAGDOLL_EVERY: f64 = 0.25;
const RAGDOLL_CHECK: f64 = 0.6;
const RAGDOLL_ASKED_FOR: f64 = 6.0;
/// m past what the part can reach the grip gives, for this many seconds; and m from the person's chest in any case
const NPC_TEAR: f32 = 0.2;
const NPC_TEAR_FOR: f32 = 0.1;
const NPC_REACH: f32 = 1.6;
/// What a grip can take of a person: the part, the slot it is solved from, its name.
const NPC_PARTS: [(i32, &str, &str); 6] =
    [(0, "LeftHand", "left arm"), (1, "RightHand", "right arm"), (2, "LeftFoot", "left leg"), (3, "RightFoot", "right leg"), (4, "Head", "head"), (5, "Chest", "chest")];
/// m round a person's axis, m/s of the hand, s between bumps
const NPC_TOUCH: f32 = 0.45;
const NPC_MOVING: f32 = 0.5;
const NPC_EVERY: f64 = 1.5;
/// m/s: with the physics hands a touch is a bump only this fast
const NPC_BLOW: f32 = 2.5;
/// kg: what a grip takes at all, and what is pulled into the palm
const CARRY_MAX_MASS: f32 = 30.0;
const CARRY_EASY_MASS: f32 = 6.0;
/// m from the wrist
const CARRY_REACH: f32 = 0.17;
/// m from the palm: past the rays, what PhysX has this near is taken
const CARRY_NEAR: f32 = 0.1;
/// m a hand left alone with a thing can be off the place it took and just goes on holding it there
const REGRIP_MOST: f32 = 0.03;
/// calls between looks by a closed hand for what the other holds
const REACH_EVERY: u32 = 8;
/// s without the body moving before it is commanded anyway: it may be stuck
const STEP_MOST: f32 = 0.05;
/// HELD BY ITS OWN BODY (heldbody): false would carry everything by velocity
const HOLD_BODY: bool = true;
/// IN ONE HAND, A GRASP: held by its own body in one hand alone, the thing is seated for the grasp its shape makes
const GRASP_STREET: bool = true;
/// s before a thing comes to the palm: both hands closing on it together find it where it was
const PULL_WAIT: f32 = 0.12;
/// what is measured of a swing, from what hand speed, and how many a session
const TRACE_FRAMES: usize = 90;
const TRACE_SPEED: f32 = 2.0;
const TRACE_TIMES: u32 = 4;
/// m from the wrist along the fingers, until the knuckles have been seen; of the way from the wrist to the knuckles
const CARRY_PALM: f32 = 0.085;
const CARRY_PALM_ALONG: f32 = 0.55;
/// m out of the palm a thing is looked for, and from the hand's middle to the palm's skin
const CARRY_PALM_REACH: f32 = 0.13;
const CARRY_PALM_SKIN: f32 = 0.03;
/// m and s: the place on it that was taken comes to the palm
const CARRY_PULL_MOST: f32 = 0.25;
const CARRY_PULL_TIME: f32 = 0.15;
const CARRY_FOLLOW: f32 = 0.85;
const CARRY_SPIN: f32 = 25.0;
const CARRY_MAX_SPEED: f32 = 30.0;
/// s the wanted turn is smoothed over, rad/s at most
const TURN_TAU: f32 = 0.02;
const TURN_MOST: f32 = 40.0;
/// m/s^2: up to 3 kg a hand keeps up with any swing; past it the limit falls steeply. Two hands count as a fifth of the mass.
const CARRY_ACCEL: f32 = 500.0;
const CARRY_EASY: f32 = 3.0;
const CARRY_MASS_POWER: f32 = 5.4;
const CARRY_TWO_HANDS: f32 = 5.0;
/// m between the hands: under the first they turn it as one, past the second the line between them does
const TWO_TURN_FROM: f32 = 0.05;
const TWO_TURN_FULL: f32 = 0.15;
/// s for a drawn hand to go to the place it took
const HAND_TO_IT_TIME: f32 = 0.2;
/// m/s: slower than this, the bone and the arm's target are the same moment
const HAND_STILL: f32 = 0.15;
/// m/s and s: what was woken goes back to sleep once it has lain still that long
const SETTLE_SPEED: f32 = 0.05;
const SETTLE_FOR: f32 = 3.0;
const BLOCK_MISS: f32 = 0.03;
const BLOCK_FREE_MISS: f32 = 0.02;
const BLOCK_SLIP: f32 = 0.25;
const BLOCK_SPEED: f32 = 0.6;
const BLOCK_RAMP: f32 = 30.0;
const PICTURE_STEPS: f32 = 1.5;
/// m: no ray for less, what a thing measures if it does not say, and room kept from the wall
const PICTURE_GAP: f32 = 0.003;
const PICTURE_MEASURE: f32 = 0.05;
const PICTURE_MARGIN: f32 = 0.005;
/// m a free hand is near, s after letting go it stays hidden
const QUERY_HAND: f32 = 0.3;
const QUERY_AFTER: f64 = 0.3;
/// less of the told speed than this along it, this many steps running, and this many free steps end it
const BLOCK_ALONG: f32 = 0.5;
const BLOCK_STEPS: u32 = 3;
const BLOCK_FREE_STEPS: u32 = 6;
/// m the drawn hand ignores, and the most it stays behind
const HAND_SLACK: f32 = 0.02;
const HAND_TRAIL: f32 = 0.25;
/// m from the real hand, for that many seconds, and the hand lets go
const HAND_LOST_AT: f32 = 0.45;
const HAND_LOST_FOR: f32 = 0.6;
/// A THROW IS THE PEAK OF THE SWING, over a short window
const THROW_WINDOW: f64 = 0.15;
const THROW_GAIN: f32 = 1.25;
const THROW_GAIN_FROM: f32 = 1.0;
/// m over which the hand's limit worked on it: no thing leaves faster than that allows
const THROW_ARM: f32 = 0.6;
/// m/s from the wrist's turn, and rad/s kept
const THROW_TANGENT_MOST: f32 = 5.0;
const THROW_SPIN_MOST: f32 = 20.0;
/// s: both hands letting go within this is a two-handed throw
const THROW_TOGETHER: f64 = 0.2;
/// the grip's shared slots, left and right
const GRIP_SLOT: [i32; 2] = [155, 49];
/// THE FINGERS ON A BODY LYING DEAD: by NPC_BODY.bones, the grasp's style and the part's radius
const CORPSE_GRASP: [(i32, f32); 13] =
    [(4, 0.12), (2, 0.052), (4, 0.078), (1, 0.055), (1, 0.055), (1, 0.048), (1, 0.048), (1, 0.040), (1, 0.040), (1, 0.072), (1, 0.072), (1, 0.050), (1, 0.050)];
/// THE PART'S DRAWN SURFACE against the ragdoll's own shapes, by bone segment: the ragdoll's capsules are rounder and
/// fuller than the drawn body and its clothes
const CORPSE_CLOTH: [f32; 13] = [0.0; 13];
/// m: the drawn surface this far off the palm's skin; clearance for the finger pads
const CORPSE_PALM_GAP: f32 = 0.007;
const CORPSE_WRAP_WIDER: f32 = 0.0015;
/// a measured flesh this share of the part's own radius, or it is not its own
const CORPSE_FLESH_LEAST: f32 = 0.7;
const CORPSE_FLESH_MOST: f32 = 1.4;
/// s: the bones read again this often while a part is held; m the capsule given the fingers reaches along the bone
const CORPSE_CAP_EVERY: f64 = 0.25;
const CORPSE_CAP_HALF: f32 = 0.06;
/// N m: the grip a hand holds a dead body's part with (the trunk, a part)
const CORPSE_GRIP_TRUNK: f32 = 60.0;
const CORPSE_GRIP_PART: f32 = 25.0;

// ---------------------------------------------------------------- the records
/// What two hands hold together: the line between them as they took it, and the thing's turn then.
#[derive(Clone, Copy)]
pub(super) struct Two {
    u: V3,
    len: f32,
    q: Q,
}
#[derive(Clone, Copy)]
struct Turn {
    q: Q,
    w: V3,
}
/// A body the hands meet, kept by who it is (a hit lasts a frame, and a body is an allocation).
pub(super) struct BodyRec {
    pub body: Handle,
    pub kind: f32,
    pub proxy: u32,
    pub index: u32,
    pub mass: f32,
    massless: bool,
    checked: bool,
    settled: f32,
    query_off: Option<bool>,
    two: Option<Two>,
    parted: Option<f64>,
    kinematic: bool,
    since: f32,
    at: Option<V3>,
    bwas: Option<V3>,
    pictured: bool,
    went: Option<V3>,
    go: V3,
    turn: Option<Turn>,
    told: Option<V3>,
    slips: u32,
    clear: u32,
    blocked: Option<f32>,
    dims: Option<Option<V3>>,
    trace: Option<Vec<String>>,
    whole: Option<f32>,
    corpse_part: Option<&'static str>,
    limb_hang: bool,
}
pub(super) type RecRef = Rc<RefCell<BodyRec>>;
fn new_rec(body: Handle, kind: f32, proxy: u32, index: u32, w: f32) -> RecRef {
    Rc::new(RefCell::new(BodyRec {
        body,
        kind,
        proxy,
        index,
        mass: if w > 0.01 { w } else { 1.0 },
        massless: w <= 0.01,
        checked: false,
        settled: 0.0,
        query_off: None,
        two: None,
        parted: None,
        kinematic: false,
        since: 0.0,
        at: None,
        bwas: None,
        pictured: false,
        went: None,
        go: [0.0; 3],
        turn: None,
        told: None,
        slips: 0,
        clear: 0,
        blocked: None,
        dims: None,
        trace: None,
        whole: None,
        corpse_part: None,
        limb_hang: false,
    }))
}
/// A DEAD BODY'S PART HELD BY THE HAND AS A PISTOL IS (VRHandHoldBody): the bone point g of the body's frame at x of the
/// hand's, the body turned q; the grasp the seat had (style, the capsule c along a, its half and radius).
#[derive(Clone, Copy)]
struct Fast {
    x: V3,
    q: Q,
    g: V3,
    c: V3,
    a: V3,
    grip: f32,
    style: Option<i32>,
    half: f32,
    cr: f32,
}
/// What a hand carries.
pub(super) struct Hold {
    pub rec: RecRef,
    lost: f32,
    trail: Vec<(f64, V3, V3)>,
    age: f32,
    /// the real hand, world
    p: V3,
    q: Q,
    /// the place it took, in the thing's frame
    l: V3,
    /// that place in the hand's frame, and the thing's turn in it; the wrist in the thing's frame
    a: V3,
    r: Q,
    g: V3,
    /// where the place goes in the hand's frame
    t: V3,
    seat_on: bool,
    q0: Q,
    /// the drawn hand's offset (model space), None for none
    pub d: Option<V3>,
    said: bool,
    fast: Option<Fast>,
    dq: Option<Q>,
    cap_r: Option<f32>,
    cap_along: Option<f32>,
}
fn new_hold(rec: RecRef, p: V3, q: Q) -> Hold {
    Hold {
        rec,
        lost: 0.0,
        trail: Vec::new(),
        age: 0.0,
        p,
        q,
        l: [0.0; 3],
        a: [0.0; 3],
        r: [0.0, 0.0, 0.0, 1.0],
        g: [0.0; 3],
        t: [0.0; 3],
        seat_on: false,
        q0: [0.0, 0.0, 0.0, 1.0],
        d: None,
        said: false,
        fast: None,
        dq: None,
        cap_r: None,
        cap_along: None,
    }
}
#[derive(Clone, Copy)]
struct Seg {
    s: usize,
    a: V3,
    c: V3,
}
/// A ragdoll the hands have taken, by proxy.
struct Ragdoll {
    still: f64,
    next: f64,
    id: Option<u64>,
    frozen: bool,
    resting: bool,
    check: Option<f64>,
    looked: Option<f64>,
    seg: Option<Seg>,
    seg_body: usize,
    seg_at: f64,
    kind: Option<String>,
}
/// A PERSON'S PART HELD (npc_grip): who, which part, where it stood from the controller when the grip closed.
struct NpcGrip {
    npc: Handle,
    part: i32,
    o: V3,
    m: Q,
    torn: f32,
    age: f32,
    phys: bool,
    owner: i32,
    rec: Option<NpcRef>,
    native_anchor: bool,
    segment: i32,
    dw: Option<V3>,
    gap: Option<f32>,
    pub d: Option<V3>,
    said: bool,
}

pub(super) struct Carry {
    pub coll_on: bool,
    bodies: HashMap<u64, RecRef>,
    pub hold: [Option<Hold>; 2],
    woken: Vec<RecRef>,
    query_back: Vec<(RecRef, f64)>,
    hv: [V3; 2],
    hp: [Option<V3>; 2],
    hf: [V3; 2],
    grip_was: [bool; 2],
    native: [(u32, Option<[[f32; 4]; 9]>); 2],
    native_palm: [Option<V3>; 2],
    pub palm_local: [Option<V3>; 2],
    rd: [Option<V3>; 2],
    carry_lead: f32,
    ragdolls: HashMap<u32, Ragdoll>,
    dead_asked: HashMap<u64, f64>,
    dead_prune_at: f64,
    dead_loose: HashMap<u64, f64>,
    forced: [Option<f64>; 2],
    npc_grip: [Option<NpcGrip>; 2],
    corpse_grasped: [bool; 2],
    fast: [bool; 2],
    corpse_drive: (u32, Vec<(u32, i32, f32)>),
    massless_said: u32,
    block_said: u32,
    trace_done: u32,
    reach_turn: u32,
    npc_next: f64,
    npc_said: u32,
    carry_want: bool,
    carry_set: bool,
    weight_at: Option<f32>,
    weight_mod: Handle,
    taken_by: i32,
    world_preset: usize,
    world_ever: bool,
}
impl Carry {
    pub fn new() -> Carry {
        Carry {
            coll_on: false,
            bodies: HashMap::new(),
            hold: [None, None],
            woken: Vec::new(),
            query_back: Vec::new(),
            hv: [[0.0; 3]; 2],
            hp: [None; 2],
            hf: [[0.0; 3]; 2],
            grip_was: [false; 2],
            native: [(u32::MAX, None); 2],
            native_palm: [None; 2],
            palm_local: [None; 2],
            rd: [None; 2],
            carry_lead: 0.03,
            ragdolls: HashMap::new(),
            dead_asked: HashMap::new(),
            dead_prune_at: 0.0,
            dead_loose: HashMap::new(),
            forced: [None; 2],
            npc_grip: [None, None],
            corpse_grasped: [false; 2],
            fast: [false; 2],
            corpse_drive: (u32::MAX, Vec::new()),
            massless_said: 0,
            block_said: 0,
            trace_done: 0,
            reach_turn: 0,
            npc_next: 0.0,
            npc_said: 0,
            carry_want: false,
            carry_set: false,
            weight_at: None,
            weight_mod: Handle::null(),
            taken_by: 0,
            world_preset: 0,
            world_ever: false,
        }
    }
}

// ---------------------------------------------------------------- the natives on a body
fn bh(b: &Handle) -> Value {
    Value::Handle(b)
}
fn body_pos(b: &Handle) -> Option<V3> {
    nat::<[f32; 4]>("VRBodyGetPos", &[bh(b)]).filter(|p| p[3] > 0.5).map(|p| [p[0], p[1], p[2]])
}
fn body_rot(b: &Handle) -> Option<Q> {
    nat::<[f32; 4]>("VRBodyGetRot", &[bh(b)])
}
fn body_vel(b: &Handle) -> Option<V3> {
    nat::<[f32; 4]>("VRBodyGetVel", &[bh(b)]).filter(|v| v[3] > 0.5).map(|v| [v[0], v[1], v[2]])
}
fn body_set_vel(b: &Handle, v: &V3) {
    nat::<bool>("VRBodySetVel", &[bh(b), vp(&v4(v, 0.0))]);
}
fn body_set_ang_vel(b: &Handle, v: &V3) {
    nat::<bool>("VRBodySetAngVel", &[bh(b), vp(&v4(v, 0.0))]);
}
fn body_info(proxy: u32, index: u32) -> Option<[f32; 4]> {
    nat::<[f32; 4]>("VRBodyInfo", &[Value::Int32(proxy as i32), Value::Int32(index as i32)])
}
fn body_surface(proxy: u32, index: u32, at: &V3) -> Option<[f32; 4]> {
    nat::<[f32; 4]>("VRBodySurface", &[Value::Int32(proxy as i32), Value::Int32(index as i32), vp(&v4(at, 1.0))])
}
fn debug_probe(proxy: u32, index: u32, at: &V3) {
    nat::<bool>("VRDebugProbeBody", &[Value::Int32(proxy as i32), Value::Int32(index as i32), vp(&v4(at, 1.0))]);
}
fn pulse(side: usize) {
    nat::<()>("VRHapticEvent", &[Value::Int32(20), Value::Int32(if side == 0 { 1 } else { 2 })]);
}
fn key(proxy: u32, index: u32) -> u64 {
    u64::from(proxy) * 4096 + u64::from(index)
}
fn side_name(side: usize) -> &'static str {
    if side == 0 {
        "left"
    } else {
        "right"
    }
}

// ---------------------------------------------------------------- the world's rays
/// physicsTraceResult, 0x60 bytes: its position first; the material by its property.
#[repr(C, align(16))]
struct Trace([u8; 0x60]);
impl Trace {
    fn pos(&self) -> V3 {
        // SAFETY: a Vector3 at the start of the result.
        unsafe { *(self.0.as_ptr() as *const V3) }
    }
    fn material(&self) -> String {
        static AT: std::sync::OnceLock<Option<usize>> = std::sync::OnceLock::new();
        let at = *AT.get_or_init(|| crate::script_call::class_any("physicsTraceResult").and_then(|c| c.property(CName::hashed("material"))).map(|p| p.value_offset as usize));
        at.filter(|a| a + 8 <= 0x60).map(|a| super::name_text(u64::from_le_bytes(self.0[a..a + 8].try_into().expect("8 bytes")))).unwrap_or_default()
    }
    /// The trace's proxy and body as the game converted its PhysX hit (+0x48, +0x4C).
    fn ids(&self) -> (u32, u32) {
        let u = |o: usize| u32::from_le_bytes(self.0[o..o + 4].try_into().expect("4 bytes"));
        (u(0x48), u(0x4C))
    }
}
/// castRay: a ray of the spatial queries system by a preset; the hit drawn while the collision view is on.
fn cast_ray(s: &State, a: &V3, b: &V3, preset: &str, static_only: bool, dynamic_only: bool) -> Option<Trace> {
    let sq = super::super::game_system("GetSpatialQueriesSystem");
    if sq.is_null() {
        return None;
    }
    let (a4, b4) = (v4(a, 1.0), v4(b, 1.0));
    let mut t = Trace([0; 0x60]);
    let hit = super::m::<bool>(
        &sq,
        "SyncRaycastByQueryPreset",
        &[vp(&a4), vp(&b4), Value::U64(CName::hashed(preset).0), Value::Out((&mut t as *mut Trace).cast()), Value::Bool(static_only), Value::Bool(dynamic_only)],
    ) == Some(true);
    if s.view.on {
        if hit {
            nat::<bool>("VRDebugRay", &[vp(&a4), vp(&v4(&t.pos(), 1.0)), Value::Int32(2)]);
        } else {
            nat::<bool>("VRDebugRay", &[vp(&a4), vp(&b4), Value::Int32(1)]);
        }
    }
    hit.then_some(t)
}
/// A wall answers one preset or another (some are Sight Blockers, some only stop a Bullet), so a static ray that
/// missed asks the second before it counts as clear.
fn static_cast(s: &State, a: &V3, b: &V3) -> Option<Trace> {
    let c = &s.carry;
    if let Some(t) = cast_ray(s, a, b, WORLD_PRESETS[c.world_preset], true, false) {
        return Some(t);
    }
    if c.world_ever {
        let other = if c.world_preset == 1 { 0 } else { 1 };
        return cast_ray(s, a, b, WORLD_PRESETS[other], true, false);
    }
    None
}
fn ray_distance(s: &State, a: &V3, b: &V3, from: &V3, dynamic_too: bool) -> Option<f32> {
    let t = static_cast(s, a, b).or_else(|| if dynamic_too { cast_ray(s, a, b, WORLD_PRESETS[s.carry.world_preset], false, true) } else { None })?;
    Some(len(&sub(&t.pos(), from)))
}
/// bodyOf: the body behind a hit, or none for what does not move.
fn body_of(s: &mut State, t: &Trace) -> Option<RecRef> {
    let Some(info) = nat::<[f32; 4]>("VRTraceInfo", &[Value::Ptr(t.0.as_ptr().cast())]) else {
        s.say_once("trace call failed");
        return None;
    };
    if info[1] == 0.0 {
        s.say_once("trace without a proxy");
        return None;
    }
    if ![1.0, 3.0, 10.0, 12.0].contains(&info[0]) {
        return None;
    }
    let (proxy, index) = t.ids();
    let k = key(proxy, index);
    if let Some(r) = s.carry.bodies.get(&k) {
        return Some(r.clone());
    }
    let body = nat_handle("VRTraceBody", &[Value::Ptr(t.0.as_ptr().cast())]);
    if body.is_null() {
        s.say_once("no body from a trace");
        return None;
    }
    if s.carry.bodies.len() >= 128 {
        s.carry.bodies.clear();
    }
    let rec = new_rec(body, info[0], proxy, index, info[3]);
    s.carry.bodies.insert(k, rec.clone());
    Some(rec)
}
fn body_wake(s: &mut State, rec: &RecRef) {
    let mut r = rec.borrow_mut();
    if r.checked {
        return;
    }
    r.checked = true;
    if r.kind == 1.0 {
        return;
    }
    let kin = nat::<i32>("VRBodyIsKinematic", &[bh(&r.body)]);
    drop(r);
    s.say_once(&format!("body kinematic answer {}", kin.map_or("false".into(), |k| k.to_string())));
    if kin == Some(1) {
        let mut r = rec.borrow_mut();
        nat::<bool>("VRBodySetKinematic", &[bh(&r.body), Value::Bool(false)]);
        r.settled = 0.0;
        drop(r);
        if !s.carry.woken.iter().any(|w| Rc::ptr_eq(w, rec)) {
            s.carry.woken.push(rec.clone());
        }
    }
}

// ---------------------------------------------------------------- the hands
/// One completed native pose per script frame (VRPhysHandFrame's rows): the solved wrist, the independent drive target
/// and the measured palm geometry. None without the physics hands' frame (false: not the native path at all).
fn native_frame(s: &mut State, side: usize, fresh: bool) -> (Option<[[f32; 4]; 9]>, bool) {
    if !s.phys {
        return (None, false);
    }
    if fresh || s.carry.native[side].0 != s.gen {
        let rows = cvr_rig::vrik::physhands::pose::script_hand_frame(side).filter(|r| r[0][3] > 0.5);
        if let Some(r) = rows {
            s.carry.native_palm[side] = Some(super::f3(&r[4]));
        }
        s.carry.native[side] = (s.gen, rows);
    }
    (s.carry.native[side].1, true)
}
fn bone_of_hand(s: &mut State, side: usize) -> Option<(V3, Q)> {
    let h = data::data().hand[side]?;
    s.bone(data::data().caps[h].index)
}
/// handWorld: the drawn hand, world space.
pub(super) fn hand_world(s: &mut State, side: usize, f: &Frame) -> Option<(V3, Q)> {
    let (frame, native) = native_frame(s, side, false);
    if native {
        let r = frame?;
        return Some((super::f3(&r[0]), r[1]));
    }
    let (hp, hq) = bone_of_hand(s, side)?;
    // where this frame's solve puts the wrist: the bone is where the last frame's did
    let p = s.px[side].unwrap_or(hp);
    Some((f.to_world(&p), qmul(&f.i, &hq)))
}
/// handReal: the real hand, world space: what a carried thing goes by, since the drawn one goes by the carried thing.
fn hand_real(s: &mut State, side: usize, f: &Frame) -> Option<(V3, Q)> {
    let (frame, native) = native_frame(s, side, false);
    if native {
        let r = frame?;
        return Some((super::f3(&r[2]), r[3]));
    }
    let r = s.raw[side]?;
    let (_, hq) = bone_of_hand(s, side)?;
    let m = match s.carry.rd[side] {
        Some(d) => add(&r, &qrot(&hq, &d)),
        None => r,
    };
    Some((f.to_world(&m), qmul(&f.i, &hq)))
}
/// handTracked: THE CONTROLLER ALONE, world space -- its tracked place and turn, nothing of the drawn hand (the drawn
/// hand is the physics hand, kept on the part it holds).
fn hand_tracked(s: &mut State, side: usize, f: &Frame) -> Option<(V3, Q)> {
    let (frame, native) = native_frame(s, side, false);
    if native {
        let r = frame?;
        return Some((super::f3(&r[2]), r[3]));
    }
    let r = s.raw[side]?;
    let q = nat::<[f32; 4]>("VRHandRawRot", &[Value::Int32(side as i32)])?;
    Some((f.to_world(&r), qmul(&f.i, &q)))
}
/// handMeasure: how the arm's target and the wrist it arrives at differ, in the hand's frame, while nothing holds the
/// hand (without the physics hands' own frame).
fn hand_measure(s: &mut State, side: usize) {
    if s.phys {
        s.carry.rd[side] = None;
        return;
    }
    let Some(r) = s.raw[side] else { return };
    if s.carry.hold[side].is_some() || s.skip[side] {
        return;
    }
    if len2(&s.carry.hv[side]) > HAND_STILL * HAND_STILL {
        return;
    }
    let Some((hp, hq)) = bone_of_hand(s, side) else { return };
    let x = qrot(&conj(&hq), &sub(&hp, &r));
    if len2(&x) > 0.04 {
        return;
    }
    s.carry.rd[side] = Some(match s.carry.rd[side] {
        None => x,
        Some(d) => add(&d, &mul(&sub(&x, &d), 0.05)),
    });
}
/// A fingertip and the second joint it is measured from, model space.
pub(super) fn finger_ends(s: &mut State, f: &data::Finger) -> Option<(V3, V3)> {
    let d = data::data();
    let (mid, tip) = (f.mid?, f.tip?);
    let (mp, _) = s.bone(d.caps[mid].index)?;
    let (tp, _) = s.bone(d.caps[tip].index)?;
    let (a, c) = s.endpoints(tip)?;
    // the capsule's end further from its bone is the fingertip
    let end = if len2(&sub(&a, &tp)) > len2(&sub(&c, &tp)) { a } else { c };
    Some((mp, end))
}
/// The way a hand's palm faces, model space, worked out once from the hand itself: across the knuckles and along the
/// fingers gives the axis, and the fingers' resting curl says which way along it is the palm.
pub(super) fn palm_normal(s: &mut State, side: usize) -> Option<V3> {
    let d = data::data();
    let (hp, hq) = bone_of_hand(s, side)?;
    if s.carry.palm_local[side].is_none() {
        let (fi, fp) = (d.fingers[side][1]?, d.fingers[side][4]?);
        let (ip, _) = s.bone(d.caps[fi.mid?].index)?;
        let (pp, _) = s.bone(d.caps[fp.mid?].index)?;
        let ax = sub(&pp, &ip);
        let fx = sub(&mul(&add(&ip, &pp), 0.5), &hp);
        let mut n = [ax[1] * fx[2] - ax[2] * fx[1], ax[2] * fx[0] - ax[0] * fx[2], ax[0] * fx[1] - ax[1] * fx[0]];
        let l = len(&n);
        if l < 1e-5 {
            return None;
        }
        n = mul(&n, 1.0 / l);
        let mut vote = 0.0;
        for digit in 2..5 {
            if let Some(fd) = d.fingers[side][digit] {
                if let Some((m, c)) = finger_ends(s, &fd) {
                    vote += super::dot(&sub(&c, &m), &n);
                }
            }
        }
        // a flat hand does not say: next frame
        if vote.abs() < 0.004 {
            return None;
        }
        if vote < 0.0 {
            n = mul(&n, -1.0);
        }
        s.carry.palm_local[side] = Some(qrot(&conj(&hq), &n));
    }
    Some(qrot(&hq, &s.carry.palm_local[side].expect("set")))
}
/// The middle of the palm in the hand's frame: part of the way from the wrist to the knuckles.
fn palm_spot(side: usize) -> V3 {
    // the knuckles' local places (main's baseLocal) are never measured: the hand's own default
    let d = data::data();
    let sign = d.hand[side].map_or(1.0, |h| if d.caps[h].c[0] >= 0.0 { 1.0 } else { -1.0 });
    let _ = CARRY_PALM_ALONG;
    [sign * CARRY_PALM, 0.0, 0.0]
}
/// And its skin: where a thing in the hand touches it (the hand's frame).
pub(super) fn palm_skin(s: &mut State, side: usize) -> V3 {
    let (frame, native) = native_frame(s, side, false);
    if native {
        // Rig dimensions remain usable when tracking is lost, especially while releasing a two-hand hold. Only world
        // poses must fail closed.
        if let Some(p) = frame.map(|r| super::f3(&r[4])).or(s.carry.native_palm[side]) {
            return p;
        }
    }
    let p = palm_spot(side);
    match s.carry.palm_local[side] {
        Some(n) => add(&p, &mul(&n, CARRY_PALM_SKIN)),
        None => p,
    }
}
/// trackHands: the hands' speed, world space (smoothed, and barely smoothed for a throw and for carrying); and the
/// carry lead's setting.
pub(super) fn track_hands(s: &mut State, dt: f32, f: &Frame) {
    if let Some(m) = nat::<f32>("VRCarryLead", &[]) {
        s.carry.carry_lead = m;
    }
    let k = 1.0 - (-dt.max(0.001) / HV_TAU).exp();
    let kf = 1.0 - (-dt.max(0.001) / HF_TAU).exp();
    for side in 0..2 {
        let r = s.raw[side];
        if let (Some(r), Some(last)) = (r, s.carry.hp[side]) {
            if dt > 0.0005 {
                let v = qrot(&conj(&f.i), &mul(&sub(&r, &last), 1.0 / dt));
                let h = &mut s.carry.hv[side];
                *h = add(h, &mul(&sub(&v, h), k));
                let fast = &mut s.carry.hf[side];
                *fast = add(fast, &mul(&sub(&v, fast), kf));
            }
        }
        s.carry.hp[side] = r;
    }
}
fn carry_accel(mass: f32, hands: usize) -> f32 {
    let m = if hands >= 2 { mass / CARRY_TWO_HANDS } else { mass };
    if m <= CARRY_EASY {
        CARRY_ACCEL
    } else {
        CARRY_ACCEL * (CARRY_EASY / m).powf(CARRY_MASS_POWER)
    }
}
/// How a hand holds the thing, from how they stand now: the place it took (l, in the thing's frame) is at a in the
/// hand's, and that is one point: a thing turned in the hand turns about it. g is the wrist, in the thing's frame.
fn carry_base(h: &mut Hold, pos: &V3, rot: &Q) {
    let c = conj(&h.q);
    let w = qrot(rot, &h.l);
    h.a = qrot(&c, &sub(&add(pos, &w), &h.p));
    h.r = qmul(&c, rot);
    h.g = qrot(&conj(rot), &sub(&h.p, pos));
}
/// From here on this hand has it alone, and a thing it can manage comes to its palm (a heavy thing too, only not from far).
fn carry_alone(s: &mut State, h: &mut Hold, side: usize, pos: &V3, rot: &Q) {
    carry_base(h, pos, rot);
    h.seat_on = false;
    let p = palm_skin(s, side);
    let o = sub(&h.a, &p);
    h.t = if h.rec.borrow().mass <= CARRY_EASY_MASS || len2(&o) <= CARRY_PULL_MOST * CARRY_PULL_MOST { p } else { h.a };
}

// ---------------------------------------------------------------- a body lying dead
/// The held anchor's exact contact error (VRPhysHeldBody's fifth field), µm: None with the data supported but not this
/// body's (the bool), None and false without it.
fn corpse_contact_micrometres(side: usize, proxy: u32) -> (Option<f64>, bool) {
    let n = cvr_rig::vrik::physhands::adjust::held_body_state(side);
    let (age, distance) = (n[3], n[4]);
    if n[0] != 1 || n[1] != u64::from(proxy) || age >= 200 || distance > 1_000_000_000_000 {
        return (None, true);
    }
    (Some(distance as f64), true)
}
fn corpse_tick(s: &mut State, rec: &RecRef, pos: &V3, dt: f32) {
    let proxy = rec.borrow().proxy;
    let mut held: Option<i32> = None;
    let mut other = -1i32;
    for side in 0..2 {
        let Some(h) = s.carry.hold[side].as_ref() else { continue };
        if h.rec.borrow().proxy != proxy {
            continue;
        }
        let mut index = h.rec.borrow().index as i32;
        if h.fast.is_some() {
            // The selected PhysX body can differ from the script's reference body. While that native request is
            // pending, do not tow from the reference or accidentally command the part about to be held.
            let n = cvr_rig::vrik::physhands::adjust::held_body_state(side);
            if n[3] >= 200 || n[0] != 1 || n[1] != u64::from(proxy) {
                return;
            }
            index = n[2] as i32;
            if index < 0 {
                return;
            }
        }
        if Rc::ptr_eq(&h.rec, rec) && held.is_none() {
            held = Some(index);
        } else {
            other = index;
        }
    }
    let Some(held) = held else { return };
    // Two Lua reference bodies can resolve to one actual held actor: two fixed slots, no duplicate drive.
    if s.carry.corpse_drive.0 != s.gen {
        s.carry.corpse_drive = (s.gen, Vec::new());
    }
    if let Some(&(_, _, whole)) = s.carry.corpse_drive.1.iter().find(|d| d.0 == proxy && d.1 == held) {
        if whole > 0.0 {
            rec.borrow_mut().whole = Some(whole);
        }
        return;
    }
    // A limb hangs the body without lift assistance: its second held part encoded as -(body + 3), so floor towing never
    // drives that hand's anchor too.
    let mode = if rec.borrow().limb_hang { if other >= 0 { -other - 3 } else { -2 } } else { other };
    if let Some(whole) = nat::<f32>("VRCorpseHold", &[Value::Int32(proxy as i32), Value::Int32(held), Value::Int32(mode), Value::Float(pos[2]), Value::Float(dt)]) {
        if whole > 0.0 {
            rec.borrow_mut().whole = Some(whole);
        }
        if s.carry.corpse_drive.1.len() < 2 {
            s.carry.corpse_drive.1.push((proxy, held, whole));
        }
    }
}
fn find_entity(id: u64) -> Handle {
    let Some(find) = crate::script_call::native_class_function("ScriptGameInstance", "FindEntityByID") else { return Handle::null() };
    let game = crate::script_call::game_instance();
    // SAFETY: the game instance's static with an EntityID by value.
    unsafe { crate::script_call::invoke_handle(core::ptr::null_mut(), find, &[Value::Ptr((&game as *const crate::script_call::GameInstance).cast()), Value::U64(id)]) }
        .unwrap_or_else(Handle::null)
}
/// The dead person nearest a place (in xy), within DEAD_NEAR.
fn ragdoll_person(p: &V3) -> Handle {
    let pl = super::player();
    let mut best = Handle::null();
    let mut best_d = DEAD_NEAR * DEAD_NEAR;
    for e in super::targets_near(&pl, DEAD_RANGE) {
        if !super::is_a(&e, "NPCPuppet") || super::m::<bool>(&e, "IsDead", &[]) != Some(true) {
            continue;
        }
        if let Some(at) = super::world_pos(&e) {
            let d = (at[0] - p[0]) * (at[0] - p[0]) + (at[1] - p[1]) * (at[1] - p[1]);
            if d < best_d {
                best = e;
                best_d = d;
            }
        }
    }
    best
}
/// A ragdoll event queued on the entity (Forced activation, or put to sleep).
fn ragdoll_event(id: Option<u64>, name: &str, forced: bool) -> bool {
    let Some(id) = id else { return false };
    let e = find_entity(id);
    if e.is_null() {
        return false;
    }
    let ev = super::super::lobby_ink::new(name);
    if ev.is_null() {
        return false;
    }
    if forced {
        if let Some(d) = super::rt::field(&ev, "data") {
            super::super::menu_rt::set(&d, "type", super::super::menu_rt::ev("entragdollActivationRequestType", "Forced"));
        }
    }
    super::rt::call(&e, "QueueEvent", &[super::A::H(&ev)]);
    true
}
/// The ragdoll's bones by its attachment slots (people::SLOTS), world positions.
fn corpse_slots(r: &Ragdoll) -> Option<[Option<V3>; 14]> {
    let e = find_entity(r.id?);
    if e.is_null() {
        return None;
    }
    let comp = crate::engine_objects::entity_components(&e).into_iter().find(|c| super::component_name(c) == "Item_Attachment_Slot")?;
    let mut p = [None; 14];
    for (i, slot) in super::people::SLOTS.iter().enumerate() {
        p[i] = super::slot_transform(&comp, CName::hashed(slot)).map(|t| t.0);
    }
    Some(p)
}
/// The bone segment (people::BONES) nearest a place, how far off it is, and where along it (0..1); the trunk favoured
/// 5 cm, for the pelvis and the chest sit where the thighs and the arms begin.
fn corpse_nearest(p: &[Option<V3>; 14], x: &V3) -> Option<(usize, f32, f32)> {
    let mut best = None;
    let mut best_d = 1e9f32;
    for (s, b) in super::people::BONES.iter().enumerate() {
        let (Some(a), Some(c)) = (p[b.0 - 1], p[b.1 - 1]) else { continue };
        let u = sub(&c, &a);
        let uu = len2(&u);
        let t = if uu > 1e-6 { (super::dot(&sub(x, &a), &u) / uu).clamp(0.0, 1.0) } else { 0.0 };
        let d = len(&sub(&add(&a, &mul(&u, t)), x)) - if s == 0 { 0.05 } else { 0.0 };
        if d < best_d {
            best = Some((s, d, t));
            best_d = d;
        }
    }
    best
}
fn corpse_radius(s: usize, r: Option<&mut Ragdoll>, along: Option<f32>) -> f32 {
    let Some(&(_, base)) = CORPSE_GRASP.get(s) else { return 0.06 };
    let kind = match r {
        Some(r) => {
            if r.kind.is_none() {
                let e = r.id.map(find_entity).unwrap_or_else(Handle::null);
                r.kind = Some(super::m::<u64>(&e, "GetBodyType", &[]).map(super::name_text).unwrap_or_default());
            }
            r.kind.clone().unwrap_or_default()
        }
        None => String::new(),
    };
    let scale = if kind.contains("Woman") {
        if s == 0 {
            0.92
        } else if s == 2 {
            0.95
        } else {
            0.88
        }
    } else if kind.contains("Fat") {
        if s == 0 {
            1.3
        } else if s == 2 {
            1.0
        } else {
            1.1
        }
    } else if kind.contains("Big") || kind.contains("Large") {
        if s == 2 {
            1.05
        } else {
            1.15
        }
    } else {
        1.0
    };
    // A forearm narrows toward the wrist: a uniform elbow-sized capsule leaves a small distal arm floating inside the
    // fingers even when the solver is in perfect contact with that capsule.
    let radius = match along {
        Some(a) if s == 7 || s == 8 => 0.038 + (0.018 - 0.038) * a.clamp(0.0, 1.0),
        _ => base,
    };
    radius * scale
}
fn corpse_unhold(s: &mut State, side: usize) {
    if !s.carry.fast[side] {
        return;
    }
    s.carry.fast[side] = false;
    if let Some(h) = s.carry.hold[side].as_mut() {
        h.fast = None;
    }
    let zero = [0f32; 4];
    nat::<bool>("VRHandHoldBody", &[Value::Int32(side as i32), Value::Int32(-1), Value::Int32(0), vp(&zero), vp(&[0.0, 0.0, 0.0, 1.0]), vp(&zero)]);
}
fn corpse_let_go(s: &mut State, side: usize) {
    s.carry.corpse_grasped[side] = false;
    corpse_unhold(s, side);
    // every argument given: a native called with one fewer fails, and the fingers stayed closed on nothing
    let zero = [0f32; 4];
    nat::<[f32; 4]>("VRGraspCapsule", &[Value::Int32(side as i32), Value::Int32(0), vp(&zero), vp(&zero), Value::Float(0.0), Value::Float(0.0), Value::Float(-1.0)]);
}
fn hold_body_native(side: usize, rec: &BodyRec, f: &Fast) -> bool {
    nat::<bool>(
        "VRHandHoldBody",
        &[Value::Int32(side as i32), Value::Int32(rec.proxy as i32), Value::Int32(rec.index as i32), vp(&v4(&f.x, f.grip)), vp(&f.q), vp(&v4(&f.g, 0.0))],
    ) == Some(true)
}
/// corpseHoldFast: the dead body's part held by the hand as a pistol from now on -- the bone under the palm at the seat,
/// the body turned as the grasp's seat turns it.
#[allow(clippy::too_many_arguments)]
fn corpse_hold_fast(s: &mut State, side: usize, rot: &Q, seg: usize, n: &V3, p: &V3, u: &V3) {
    let Some(h) = s.carry.hold[side].as_mut() else { return };
    h.fast = None;
    let dq = h.dq.unwrap_or(h.q);
    let mut q = qmul(&conj(&dq), rot);
    if let Some(t) = nat::<[f32; 4]>("VRGraspSeatTurn", &[Value::Int32(side as i32)]) {
        q = qmul(&t, &q);
    }
    let o = qrot(&q, n);
    let a = qrot(&q, u);
    let f = Fast { x: sub(p, &o), q, g: *n, c: *p, a, grip: if seg == 0 { CORPSE_GRIP_TRUNK } else { CORPSE_GRIP_PART }, style: None, half: 0.0, cr: 0.0 };
    let held = hold_body_native(side, &h.rec.borrow(), &f);
    if held {
        h.fast = Some(f);
    }
    s.carry.fast[side] = held;
    log!(
        "[HandColl] the {} hand {} the dead body's bone {} at ({:.3} {:.3} {:.3}) of its frame, as a pistol\n",
        side_name(side),
        if held { "holds" } else { "cannot hold (carried by velocity)" },
        seg + 1,
        p[0],
        p[1],
        p[2]
    );
}
/// corpseCloth: how far the clothes stand off the bone toward the palm -- the flesh as the ragdoll's own shapes have it
/// in that way (asked from 40 cm out along it, so a palm already in the limb still finds its surface). b a point of the
/// bone and u its way (world); sk the palm's skin in the drawn hand's frame (hp, hq).
#[allow(clippy::too_many_arguments)]
fn corpse_cloth(s: &mut State, side: usize, rec: &BodyRec, seg: usize, hp: &V3, hq: &Q, b: &V3, u: &V3, sk: &V3, along_cap: Option<f32>) -> Option<f32> {
    let w = add(hp, &qrot(hq, sk));
    // the way from the bone to the palm, across the bone
    let along = super::dot(&sub(&w, b), u);
    let a = add(b, &mul(u, along));
    let o = sub(&w, &a);
    let on = len(&o);
    if on < 1e-3 {
        return None;
    }
    let o = mul(&o, 1.0 / on);
    let c = body_surface(rec.proxy, rec.index, &add(&a, &mul(&o, 0.4))).filter(|c| c[3] >= 0.0)?;
    // the flesh's distance from the bone, across it
    let cc = sub(&super::f3(&c), &a);
    let ca = super::dot(&cc, u);
    let flesh = len(&sub(&cc, &mul(u, ca)));
    // the body under the palm is not always the bone's own (a hand at an ankle has the foot's box)
    let base = corpse_radius(seg, s.carry.ragdolls.get_mut(&rec.proxy), along_cap);
    if flesh < base * CORPSE_FLESH_LEAST || flesh > base * CORPSE_FLESH_MOST {
        log!(
            "[HandColl] the dead body's bone {} toward the {} palm: a surface {:.3} m off it is not its own ({:.3} expected): {:.3} taken\n",
            seg + 1,
            side_name(side),
            flesh,
            base,
            base + CORPSE_CLOTH[seg]
        );
        return None;
    }
    // Ragdoll collision volumes are deliberately fuller than the rendered limb: they may refine a narrower anatomical
    // profile, but must not inflate it and leave the palm hovering over the visible skin.
    let r = flesh.min(base) + CORPSE_CLOTH[seg];
    log!("[HandColl] the dead body's bone {} toward the {} palm: flesh {:.3} m off it, clothes to {:.3}\n", seg + 1, side_name(side), flesh, r);
    Some(r)
}
/// corpseGrasp: THE FINGERS ON A BODY LYING DEAD (VRGraspCapsule): the part in the hand is a capsule about its bone, read
/// from the ragdoll's own slots now and then and kept in the held body's frame; the fingers take the grasp a living
/// person's part would get, and the hand is seated on it.
fn corpse_grasp(s: &mut State, side: usize, rec_ref: &RecRef, pos: &V3, rot: &Q, now: f64, f: &Frame) {
    if let Some(fast) = s.carry.hold[side].as_ref().and_then(|h| h.fast) {
        let rec = rec_ref.borrow();
        if !hold_body_native(side, &rec, &fast) {
            drop(rec);
            if let Some(h) = s.carry.hold[side].as_mut() {
                h.fast = None;
            }
            s.say_once("a dead body held by the hand was let go by the plugin: false");
        }
    }
    let fast = s.carry.hold[side].as_ref().and_then(|h| h.fast);
    if let Some(fs) = fast.filter(|f| f.style.is_some()) {
        // held as a pistol: the grasp the seat had, in the hand's frame, every frame
        if let Some(seat) = nat::<[f32; 4]>(
            "VRGraspCapsule",
            &[Value::Int32(side as i32), Value::Int32(fs.style.unwrap_or(0)), vp(&v4(&fs.c, 1.0)), vp(&v4(&fs.a, 0.0)), Value::Float(fs.half), Value::Float(fs.cr), Value::Float(-1.0)],
        ) {
            s.carry.corpse_grasped[side] = seat[3] > -0.5;
        }
        return;
    }
    let (proxy, body_id, index) = {
        let r = rec_ref.borrow();
        (r.proxy, r.body.instance as usize, r.index)
    };
    let _ = index;
    if !s.carry.ragdolls.contains_key(&proxy) {
        return;
    }
    let Some(hl) = s.carry.hold[side].as_ref().map(|h| h.l) else { return };
    let ci = conj(rot);
    let stale = {
        let r = &s.carry.ragdolls[&proxy];
        r.seg.is_none() || r.seg_body != body_id || now - r.seg_at >= CORPSE_CAP_EVERY
    };
    if stale {
        if let Some(p) = corpse_slots(&s.carry.ragdolls[&proxy]) {
            let w = qrot(rot, &hl);
            if let Some((sg, _, _)) = corpse_nearest(&p, &add(pos, &w)) {
                let b = super::people::BONES[sg];
                let (a, c) = (p[b.0 - 1].expect("nearest has both"), p[b.1 - 1].expect("nearest has both"));
                let r = s.carry.ragdolls.get_mut(&proxy).expect("checked");
                r.seg = Some(Seg { s: sg, a: qrot(&ci, &sub(&a, pos)), c: qrot(&ci, &sub(&c, pos)) });
                r.seg_body = body_id;
                r.seg_at = now;
            }
        }
    }
    let Some(seg) = s.carry.ragdolls[&proxy].seg else { return };
    let Some(&(style, _)) = CORPSE_GRASP.get(seg.s) else { return };
    // the place taken projected onto the bone, in the body's frame, and a short capsule about it along the bone
    let mut u = sub(&seg.c, &seg.a);
    let uu = len2(&u);
    let (mut n, mut half) = (seg.a, 0.0f32);
    if uu > 1e-6 {
        let un = uu.sqrt();
        u = mul(&u, 1.0 / un);
        half = CORPSE_CAP_HALF.min(0.5 * un);
        let t = half.max((un - half).min(super::dot(&sub(&hl, &seg.a), &u)));
        if let Some(h) = s.carry.hold[side].as_mut() {
            h.cap_along = Some(t / un);
        }
        n = add(&seg.a, &mul(&u, t));
    } else {
        u = [0.0, 0.0, 1.0];
    }
    // into the hand's frame: the bone's way as it lies now. THE DRAWN HAND'S frame, the one the plugin holds the body in.
    let (hp, hq) = match hand_world(s, side, f) {
        Some(x) => x,
        None => {
            let h = s.carry.hold[side].as_ref().expect("held");
            (h.p, h.q)
        }
    };
    let w = qrot(rot, &n);
    let hi = conj(&hq);
    let mut p = qrot(&hi, &sub(&add(pos, &w), &hp));
    let dw = qrot(rot, &u);
    let ax = qrot(&hi, &dw);
    let e0 = p;
    let (seat_on, two, cap_along) = {
        let h = s.carry.hold[side].as_ref().expect("held");
        (h.seat_on, rec_ref.borrow().two.is_some(), h.cap_along)
    };
    if seat_on {
        // where the hold brings it: the fingers keep to that, solved again only as the part turns in the hand
        p = s.carry.hold[side].as_ref().expect("held").t;
    } else if !two {
        // where it is to lie: its clothes on the palm's skin, the bone that far out along the palm's normal
        if let Some(nl) = s.carry.palm_local[side] {
            let sk = palm_skin(s, side);
            let r = {
                let rec = rec_ref.borrow();
                corpse_cloth(s, side, &rec, seg.s, &hp, &hq, &add(&w, pos), &dw, &sk, cap_along)
            }
            .unwrap_or_else(|| corpse_radius(seg.s, s.carry.ragdolls.get_mut(&proxy), cap_along) + CORPSE_CLOTH[seg.s]);
            if let Some(h) = s.carry.hold[side].as_mut() {
                h.cap_r = Some(r);
            }
            // this palm's estimate: the grasp seats it on the palm the fingers are fitted to
            let out = r + CORPSE_PALM_GAP;
            p = add(&sk, &mul(&nl, out));
        }
    }
    let cap_held = s.carry.hold[side].as_ref().and_then(|h| h.cap_r);
    let mut cap_r = match (seat_on, two, cap_held) {
        (true, _, Some(r)) => r,
        (false, false, Some(r)) => r,
        _ => corpse_radius(seg.s, s.carry.ragdolls.get_mut(&proxy), cap_along),
    };
    // held round, a little wider than the part: the fingers lie round its clothes, not in them
    if style != 4 {
        cap_r += CORPSE_WRAP_WIDER;
    }
    let gap = if !seat_on && !two { CORPSE_PALM_GAP } else { -1.0 };
    let Some(seat) =
        nat::<[f32; 4]>("VRGraspCapsule", &[Value::Int32(side as i32), Value::Int32(style), vp(&v4(&p, 1.0)), vp(&v4(&ax, 0.0)), Value::Float(half), Value::Float(cap_r), Value::Float(gap)])
    else {
        return;
    };
    s.carry.corpse_grasped[side] = seat[3] > -0.5;
    if seat[3] > -0.5 && !seat_on && !two {
        // SEATED: the place the hand took is the bone under it, and it goes to where the grasp seats it
        if seat[3] > 0.5 {
            p = super::f3(&seat);
        }
        {
            let h = s.carry.hold[side].as_mut().expect("held");
            h.l = n;
            carry_base(h, pos, rot);
            h.t = p;
            h.seat_on = true;
            // THE BODY HELD BY THE HAND from now on, as a pistol: its turn in the drawn hand's frame, as its place
            h.dq = Some(hq);
        }
        corpse_hold_fast(s, side, rot, seg.s, &n, &p, &u);
        if let Some(fs) = s.carry.hold[side].as_mut().and_then(|h| h.fast.as_mut()) {
            fs.style = Some(style);
            fs.half = half;
            fs.cr = cap_r;
        }
        let e = sub(&p, &e0);
        log!(
            "[HandColl] the {} hand seats on the dead body's {} (bone {}, r {:.3} over its clothes) for its grasp: the bone comes {:.3} m to it{}\n",
            side_name(side),
            rec_ref.borrow().corpse_part.unwrap_or("nil"),
            seg.s + 1,
            cap_r,
            len(&e),
            if seat[3] > 0.5 { "" } else { " (the grasp held as it was)" }
        );
    }
}
/// ragdollTaken: a ragdoll a hand took woken (or found), the part taken said.
fn ragdoll_taken(s: &mut State, rec: &RecRef, p: &V3, at: Option<&V3>) {
    let proxy = rec.borrow().proxy;
    if !s.carry.ragdolls.contains_key(&proxy) {
        let e = ragdoll_person(p);
        let id = super::entity_hash(&e);
        s.carry.ragdolls.insert(proxy, Ragdoll { still: 0.0, next: 0.0, id, frozen: false, resting: false, check: None, looked: None, seg: None, seg_body: 0, seg_at: -1.0, kind: None });
    }
    let r = s.carry.ragdolls.get_mut(&proxy).expect("inserted");
    if r.frozen {
        nat::<bool>("VRCorpseFreeze", &[Value::Int32(proxy as i32), Value::Bool(false)]);
        r.frozen = false;
    }
    if r.resting {
        let ok = ragdoll_event(r.id, "entRagdollActivationRequestEvent", true);
        log!("[HandColl] a ragdoll at rest woken by a grip: {ok}\n");
        r.resting = false;
        r.check = None;
    }
    r.still = 0.0;
    r.next = 0.0;
    if let Some(id) = r.id {
        s.carry.dead_loose.remove(&id);
    }
    // the part taken: the trunk is lifted whole, the rest hang from the hand
    let found = at.and_then(|x| corpse_slots(&s.carry.ragdolls[&proxy]).and_then(|ps| corpse_nearest(&ps, x)));
    let part = found.map(|(sg, _, _)| match sg {
        0 => "trunk",
        1 | 2 => "head",
        3..=8 => "arm",
        _ => "leg",
    });
    let mut r = rec.borrow_mut();
    r.corpse_part = part;
    r.limb_hang = part.is_some_and(|p| p != "trunk");
    log!(
        "[HandColl] a body lying dead taken by the {} ({} m off its bones), {:.1} kg in the hand: {}\n",
        part.unwrap_or("unknown part"),
        found.map_or("?".into(), |f| format!("{:.2}", f.1)),
        r.mass,
        if r.limb_hang { "the rest hangs from it" } else { "lifted whole" }
    );
}
/// ragdollTick: A RAGDOLL GOES BACK TO REST as a prop does -- three seconds without a hand on it and with no limb moving.
/// First the game's own event; if its limbs go on moving after it, kinematic where they lie.
fn ragdoll_tick(s: &mut State) {
    let now = super::clock();
    if now >= s.carry.dead_prune_at {
        s.carry.dead_prune_at = now + 1.0;
        s.carry.dead_asked.retain(|_, asked| now - *asked <= DEAD_AGAIN);
    }
    let loose: Vec<u64> = s.carry.dead_loose.iter().filter(|(_, at)| now > **at + RAGDOLL_ASKED_FOR).map(|(k, _)| *k).collect();
    for id in loose {
        s.carry.dead_loose.remove(&id);
        ragdoll_event(Some(id), "entRagdollPutToSleepEvent", false);
    }
    let held_proxy = |s: &State, proxy: u32| s.carry.hold.iter().flatten().any(|h| h.rec.borrow().proxy == proxy);
    let proxies: Vec<u32> = s.carry.ragdolls.keys().copied().collect();
    for proxy in proxies {
        let held = held_proxy(s, proxy);
        let Some(r) = s.carry.ragdolls.get_mut(&proxy) else { continue };
        if now < r.next {
            continue;
        }
        let since = now - r.looked.unwrap_or(now);
        r.looked = Some(now);
        r.next = now + RAGDOLL_EVERY;
        if r.resting && r.check.is_none() && !held {
            r.next = now + RAGDOLL_REST_EVERY;
            if body_info(proxy, 0).is_none_or(|i| i[0] != 12.0) {
                s.carry.ragdolls.remove(&proxy);
            }
        } else if !held {
            let speed = nat::<f32>("VRCorpseSpeed", &[Value::Int32(proxy as i32)]);
            match speed {
                None => {
                    s.carry.ragdolls.remove(&proxy);
                }
                Some(sp) if sp < 0.0 => {
                    s.carry.ragdolls.remove(&proxy);
                }
                Some(sp) => {
                    if let Some(check) = r.check {
                        if now >= check {
                            r.check = None;
                            let mut frozen = false;
                            if sp > 0.01 {
                                frozen = nat::<bool>("VRCorpseFreeze", &[Value::Int32(proxy as i32), Value::Bool(true)]) == Some(true);
                                r.frozen = frozen;
                            }
                            log!("[HandColl] a ragdoll told to rest: fastest limb {:.3} m/s after it{}\n", sp, if frozen { ", so made kinematic" } else { "" });
                        }
                    } else if !r.resting {
                        if sp > RAGDOLL_SPEED {
                            r.still = 0.0;
                        } else {
                            r.still += since;
                            if r.still >= f64::from(SETTLE_FOR) {
                                let sent = ragdoll_event(r.id, "entRagdollPutToSleepEvent", false);
                                r.resting = true;
                                r.check = Some(now + RAGDOLL_CHECK);
                                log!("[HandColl] a ragdoll lay still {:.1} s: asked it to sleep, {}\n", r.still, sent);
                            }
                        }
                    }
                }
            }
        } else {
            r.still = 0.0;
        }
    }
}
/// deadForce: A BODY WITHOUT ANY RAGDOLL -- a person killed by an animation lies there with no ragdoll in the physics,
/// and nothing for a hand to hold. A grip on it asks the game for the ragdoll.
fn dead_force(s: &mut State, side: usize, f: &Frame) {
    let Some((p, _)) = hand_world(s, side, f) else { return };
    let pl = super::player();
    let now = super::clock();
    for e in super::targets_near(&pl, DEAD_RANGE) {
        if !super::is_a(&e, "NPCPuppet") || super::m::<bool>(&e, "IsDead", &[]) != Some(true) {
            continue;
        }
        let Some(at) = super::world_pos(&e) else { continue };
        if (at[0] - p[0]) * (at[0] - p[0]) + (at[1] - p[1]) * (at[1] - p[1]) >= DEAD_NEAR * DEAD_NEAR {
            continue;
        }
        let Some(id) = super::entity_hash(&e) else { continue };
        if now - s.carry.dead_asked.get(&id).copied().unwrap_or(-100.0) > DEAD_AGAIN {
            s.carry.dead_asked.insert(id, now);
            let ok = ragdoll_event(Some(id), "entRagdollActivationRequestEvent", true);
            log!("[HandColl] a body lying dead with no ragdoll in reach: {}\n", if ok { "asked for one" } else { "no entity" });
            s.carry.dead_loose.insert(id, now);
        }
        s.carry.forced[side] = Some(now + DEAD_WAIT);
        return;
    }
}

// ---------------------------------------------------------------- taking and letting go
fn release(s: &mut State, side: usize, keep_speed: bool) {
    let Some(h) = s.carry.hold[side].take() else { return };
    let rec_ref = h.rec.clone();
    if s.carry.corpse_grasped[side] || s.carry.fast[side] {
        corpse_let_go(s, side);
    }
    let other_same = s.carry.hold[1 - side].as_ref().is_some_and(|o| Rc::ptr_eq(&o.rec, &rec_ref));
    if other_same && rec_ref.borrow().kind == 12.0 {
        // a dead body's part both held: the other hand holds it on, as it does
        log!("[HandColl] the other hand holds the dead body on\n");
        return;
    }
    if other_same {
        // the other hand has it now, as it is
        {
            let mut r = rec_ref.borrow_mut();
            r.two = None;
            r.parted = Some(super::clock());
        }
        let (pos, rot) = {
            let r = rec_ref.borrow();
            (body_pos(&r.body), body_rot(&r.body))
        };
        if let (Some(pos), Some(rot)) = (pos, rot) {
            let mut other = s.carry.hold[1 - side].take().expect("checked");
            // IT STAYS WHERE IT IS, for a hand on it. A hand that took it from a little way off has the place it took
            // come into its palm.
            let sk = palm_skin(s, 1 - side);
            let w = qrot(&other.q, &sk);
            let n = qrot(&conj(&rot), &sub(&add(&other.p, &w), &pos));
            if len2(&sub(&n, &other.l)) < REGRIP_MOST * REGRIP_MOST {
                other.l = n;
            }
            carry_alone(s, &mut other, 1 - side, &pos, &rot);
            s.carry.hold[1 - side] = Some(other);
        }
        log!("[HandColl] the other hand has it\n");
        return;
    }
    let (kinematic, proxy, index, query_off, mass, parted, body) = {
        let r = rec_ref.borrow();
        (r.kinematic, r.proxy, r.index, r.query_off, r.mass, r.parted, r.body.clone())
    };
    // a body the plugin holds goes dynamic again at the next PhysX step, with the throw below
    if !kinematic {
        nat::<bool>("VRHeldPictureLetGo", &[Value::Int32(proxy as i32), Value::Int32(index as i32)]);
    }
    if query_off.is_some() {
        s.carry.query_back.retain(|(r, _)| !Rc::ptr_eq(r, &rec_ref));
        s.carry.query_back.push((rec_ref.clone(), super::clock() + QUERY_AFTER));
    }
    // w 0: it goes on as it went
    let (mut throw_v, mut throw_w) = ([0f32; 4], [0f32; 4]);
    let mut speed = 0.0f32;
    if keep_speed {
        let (mut v, mut w) = ([0f32; 3], [0f32; 3]);
        let mut best = 0.0f32;
        for (_, tv, tw) in &h.trail {
            let s2 = len2(tv);
            if s2 > best {
                best = s2;
                v = *tv;
                w = *tw;
            }
        }
        speed = best.sqrt();
        let mut gain = if speed > THROW_GAIN_FROM { THROW_GAIN } else { 1.0 };
        let hands = if parted.is_some_and(|t| super::clock() - t < THROW_TOGETHER) { 2 } else { 1 };
        let most = (2.0 * carry_accel(mass, hands) * THROW_ARM).sqrt();
        if speed * gain > most {
            gain = most / speed.max(1e-3);
        }
        v = mul(&v, gain);
        speed *= gain;
        if let Some(pos) = body_pos(&body) {
            // the wrist's turn carries a thing held away from it
            let r = sub(&pos, &h.p);
            let mut t = [w[1] * r[2] - w[2] * r[1], w[2] * r[0] - w[0] * r[2], w[0] * r[1] - w[1] * r[0]];
            let tl = len(&t);
            if tl > THROW_TANGENT_MOST {
                t = mul(&t, THROW_TANGENT_MOST / tl);
            }
            v = add(&v, &t);
        }
        let wl = len(&w);
        if wl > THROW_SPIN_MOST {
            w = mul(&w, THROW_SPIN_MOST / wl);
        }
        if kinematic {
            throw_v = v4(&v, 1.0);
            throw_w = v4(&w, 0.0);
        } else {
            body_set_vel(&body, &v);
            body_set_ang_vel(&body, &w);
        }
    }
    if kinematic {
        nat::<bool>("VRHeldBodyLetGo", &[Value::Int32(proxy as i32), Value::Int32(index as i32), vp(&throw_v), vp(&throw_w)]);
        rec_ref.borrow_mut().kinematic = false;
    }
    log!("[HandColl] let go: {speed:.1} m/s\n");
}
/// carryFind: what a closing hand takes -- rays from the palm and the wrist (none close to the body, where a ray meets
/// the player's own capsule), else what PhysX has at the palm. `only`: the thing the other hand has, for a hand already
/// closed that comes to it. `probe_only`: selection only.
fn carry_find(s: &mut State, side: usize, f: &Frame, only: Option<&RecRef>, probe_only: bool) -> bool {
    if data::data().hand[side].is_none() || super::super::game_system("GetSpatialQueriesSystem").is_null() {
        return false;
    }
    let (frame, native) = native_frame(s, side, probe_only);
    let model_wrist = if native {
        let Some(r) = frame else { return false };
        super::f3(&r[7])
    } else {
        let Some((hp, _)) = bone_of_hand(s, side) else { return false };
        hp
    };
    // CLOSE TO THE BODY a ray meets the player's own capsule (dynamic to a query), so none is cast there; only a ragdoll
    // PhysX has at the palm is taken.
    let close = model_wrist[0] * model_wrist[0] + model_wrist[1] * model_wrist[1] < FINGER_FROM_BODY * FINGER_FROM_BODY;
    let Some((wrist, _)) = hand_world(s, side, f) else { return false };
    let (fd, nd, palm) = if native {
        let r = frame.expect("checked");
        (super::f3(&r[6]), super::f3(&r[5]), super::f3(&r[8]))
    } else {
        let (hp, hq) = bone_of_hand(s, side).expect("checked");
        let sign = data::data().hand[side].map_or(1.0, |h| if data::data().caps[h].c[0] >= 0.0 { 1.0 } else { -1.0 });
        let fx = qrot(&hq, &[sign, 0.0, 0.0]);
        let nx = palm_normal(s, side).unwrap_or([0.0, 0.0, -1.0]);
        let _ = hp;
        let sp = qrot(&hq, &palm_spot(side));
        (fx, nx, add(&wrist, &f.dir_to_world(&sp)))
    };
    let rays: Vec<(V3, V3, f32)> = if close {
        Vec::new()
    } else {
        vec![
            (palm, nd, CARRY_PALM_REACH),
            (palm, add(&fd, &nd), CARRY_PALM_REACH),
            (palm, sub(&nd, &fd), CARRY_PALM_REACH),
            (wrist, fd, CARRY_REACH),
            (wrist, sub(&fd, &nd), CARRY_REACH),
        ]
    };
    let mut rec: Option<RecRef> = None;
    let mut point = [0f32; 3];
    for (i, (a, d, reach)) in rays.iter().enumerate() {
        let l = len(d);
        let mut w = mul(d, reach / l);
        if !native {
            w = f.dir_to_world(&w);
        }
        let Some(t) = cast_ray(s, a, &add(a, &w), WORLD_PRESETS[s.carry.world_preset], false, true) else { continue };
        if t.material().contains("_car") {
            continue;
        }
        let Some(r) = body_of(s, &t) else { continue };
        s.carry.taken_by = i as i32 + 1;
        if r.borrow().massless {
            // WHAT IS IT? No mass: a character's or a device's collider, not a loose thing. Said a few times.
            s.carry.massless_said += 1;
            if s.carry.massless_said <= 6 {
                let (kind, proxy, index, body) = {
                    let b = r.borrow();
                    (b.kind, b.proxy, b.index, b.body.clone())
                };
                let pp = body_pos(&body).unwrap_or([0.0; 3]);
                let me = super::world_pos(&super::player()).unwrap_or([0.0; 3]);
                let best = s.people.list.iter().filter_map(super::world_pos).map(|e| ((e[0] - pp[0]).powi(2) + (e[1] - pp[1]).powi(2)).sqrt()).fold(999.0f32, f32::min);
                log!(
                    "[HandColl] a hand closed on a thing without mass: kind {:.0}, proxy {}, {:.2} m from the player, {:.2} m from a person, {} people near; material {}\n",
                    kind,
                    proxy,
                    ((me[0] - pp[0]).powi(2) + (me[1] - pp[1]).powi(2)).sqrt(),
                    best,
                    s.people.list.len(),
                    t.material()
                );
                debug_probe(proxy, index, &[0.0; 3]);
            }
        } else if r.borrow().mass <= CARRY_MAX_MASS {
            point = t.pos();
            rec = Some(r);
            break;
        }
    }
    // A ray through a limb can hit the torso behind it. Prefer a closer surface of that same ragdoll, without allowing
    // an unrelated nearby object to steal the ray's selection.
    let ray_rec = rec.clone();
    if rec.as_ref().is_none_or(|r| r.borrow().kind == 12.0) {
        let info = nat::<[f32; 4]>("VRNearBody", &[vp(&v4(&palm, 1.0)), Value::Float(CARRY_NEAR), Value::Float(CARRY_MAX_MASS)]);
        let mut prefer = rec.is_none();
        if let (Some(r), Some(i)) = (rec.as_ref(), info) {
            let b = r.borrow();
            if i[0] == 12.0 && i[1] as u32 == b.proxy && i[2] as u32 != b.index {
                let a = body_surface(b.proxy, b.index, &palm);
                let c = body_surface(i[1] as u32, i[2] as u32, &palm);
                prefer = matches!((a, c), (Some(a), Some(c)) if a[3] >= 0.0 && c[3] >= 0.0 && c[3] + 0.01 < a[3]);
            }
        }
        if let (true, Some(i)) = (prefer, info) {
            if i[1] != 0.0 && (!close || i[0] == 12.0) {
                let (proxy, index) = (i[1] as u32, i[2] as u32);
                let k = key(proxy, index);
                rec = s.carry.bodies.get(&k).cloned();
                if rec.is_none() {
                    let body = nat_handle("VRBodyOf", &[Value::Int32(proxy as i32), Value::Int32(index as i32)]);
                    if !body.is_null() {
                        if s.carry.bodies.len() >= 128 {
                            s.carry.bodies.clear();
                        }
                        let r = new_rec(body, i[0], proxy, index, i[3]);
                        // (main's record from a near body has no massless flag)
                        r.borrow_mut().massless = false;
                        s.carry.bodies.insert(k, r.clone());
                        rec = Some(r);
                    }
                }
                if let Some(r) = rec.clone() {
                    let body = r.borrow().body.clone();
                    match body_pos(&body) {
                        Some(pos) => {
                            // WHERE THE PALM IS, when no ray meets it, as with a palm already inside it
                            point = palm;
                            s.carry.taken_by = 0;
                            if let Some(t) = cast_ray(s, &palm, &pos, WORLD_PRESETS[s.carry.world_preset], false, true) {
                                if body_of(s, &t).is_some_and(|b| Rc::ptr_eq(&b, &r)) {
                                    point = t.pos();
                                    s.carry.taken_by = -1;
                                }
                            }
                            s.say_once("a hand took a thing PhysX found, where no ray met it");
                        }
                        None => rec = None,
                    }
                }
            }
        }
    }
    if rec.is_none() {
        rec = ray_rec;
    }
    let Some(rec) = rec else { return false };
    if only.is_some_and(|o| !Rc::ptr_eq(o, &rec)) {
        return false;
    }
    if probe_only {
        return true;
    }
    body_wake(s, &rec);
    let body = rec.borrow().body.clone();
    let (pos, rot) = (body_pos(&body), body_rot(&body));
    let real = hand_real(s, side, f);
    let (Some(pos), Some(rot), Some((rp, rq))) = (pos, rot, real) else { return false };
    let mut h = new_hold(rec.clone(), rp, rq);
    h.l = qrot(&conj(&rot), &sub(&point, &pos));
    let (kind, mass, proxy, index) = {
        let r = rec.borrow();
        (r.kind, r.mass, r.proxy, r.index)
    };
    let second = s.carry.hold[1 - side].as_ref().is_some_and(|o| Rc::ptr_eq(&o.rec, &rec)) && kind != 12.0;
    if second {
        // the second hand: from here on both turn it, as they turned since now, and the line between them does
        let Some((op, oq)) = hand_real(s, 1 - side, f) else { return false };
        let other = s.carry.hold[1 - side].as_mut().expect("checked");
        other.p = op;
        other.q = oq;
        carry_base(other, &pos, &rot);
        carry_base(&mut h, &pos, &rot);
        h.t = h.a;
        let (p0, p1) = if side == 0 { (h.p, other.p) } else { (other.p, h.p) };
        let u = sub(&p1, &p0);
        let ul = len(&u);
        rec.borrow_mut().two = Some(Two { u: mul(&u, 1.0 / ul.max(1e-3)), len: ul, q: rot });
        h.q0 = rq;
        other.q0 = other.q;
    } else {
        carry_alone(s, &mut h, side, &pos, &rot);
    }
    s.carry.hold[side] = Some(h);
    pulse(side);
    if mass > CARRY_EASY {
        pulse(side);
    }
    if kind == 12.0 {
        ragdoll_taken(s, &rec, &wrist, Some(&pos));
    }
    debug_probe(proxy, index, &pos);
    let by = match s.carry.taken_by {
        0 => "where the palm is".to_string(),
        -1 => "a ray to its middle".to_string(),
        n => format!("ray {n}"),
    };
    log!(
        "[HandColl] took a thing: mass {:.2}, kind {:.0}, {}, by {}, the place {:.3} m from the palm\n",
        mass,
        kind,
        if second { "second hand" } else { "one hand" },
        by,
        len(&sub(&point, &palm))
    );
    true
}

// ---------------------------------------------------------------- carrying, once a frame per body
fn drive(s: &mut State, rec_ref: &RecRef, dt: f32, f: &Frame) {
    let body = rec_ref.borrow().body.clone();
    let (Some(pos), Some(rot)) = (body_pos(&body), body_rot(&body)) else {
        s.say_once("a carried body stopped answering");
        for side in 0..2 {
            if s.carry.hold[side].as_ref().is_some_and(|h| Rc::ptr_eq(&h.rec, rec_ref)) {
                s.carry.hold[side] = None;
                if s.carry.corpse_grasped[side] {
                    corpse_let_go(s, side);
                }
            }
        }
        return;
    };
    // THE SCRIPT IS CALLED TWICE A FRAME: one command for each time the body has moved, reckoned over the time it took
    // (a body the plugin holds is moved by the plugin: it is stepped every call)
    let step = {
        let mut r = rec_ref.borrow_mut();
        r.since += dt;
        if !r.kinematic && r.at == Some(pos) && r.since < STEP_MOST {
            return;
        }
        r.at = Some(pos);
        let step = r.since.min(STEP_MOST).max(1.0 / 120.0);
        r.since = 0.0;
        step
    };
    let dt = step;
    let now = super::clock();
    let mut hands: Vec<usize> = Vec::new();
    let mut v = [0f32; 3];
    // the player's own speed, for what leaves the hand
    let pv = {
        let mut r = rec_ref.borrow_mut();
        let pv = r.bwas.map_or([0.0; 3], |w| mul(&sub(&f.b, &w), 1.0 / step));
        r.bwas = Some(f.b);
        pv
    };
    for side in 0..2 {
        if !s.carry.hold[side].as_ref().is_some_and(|h| Rc::ptr_eq(&h.rec, rec_ref)) {
            continue;
        }
        let Some((p, q)) = hand_real(s, side, f) else { return };
        let hf = s.carry.hf[side];
        let h = s.carry.hold[side].as_mut().expect("checked");
        // how fast the wrist turns, world space
        let d = qmul(&q, &conj(&h.q));
        let sign = if d[3] < 0.0 { -2.0 } else { 2.0 };
        let w = [d[0] * sign / step, d[1] * sign / step, d[2] * sign / step];
        h.p = p;
        h.q = q;
        h.age += dt;
        h.trail.push((now, add(&hf, &pv), w));
        while h.trail.len() > 1 && now - h.trail[0].0 > THROW_WINDOW {
            h.trail.remove(0);
        }
        v = add(&v, &hf);
        hands.push(side);
        if rec_ref.borrow().kind == 12.0 {
            corpse_grasp(s, side, rec_ref, &pos, &rot, now, f);
        }
    }
    let n = hands.len();
    if n == 0 {
        return;
    }
    // Keep carried bodies out of player movement queries, but visible to a nearby second hand.
    {
        let mut off = true;
        if n == 1 {
            if let Some((op, _)) = hand_real(s, 1 - hands[0], f) {
                off = len2(&sub(&op, &pos)) > QUERY_HAND * QUERY_HAND;
            }
        }
        s.carry.query_back.retain(|(r, _)| !Rc::ptr_eq(r, rec_ref));
        let mut r = rec_ref.borrow_mut();
        if r.query_off != Some(off) {
            r.query_off = Some(off);
            nat::<bool>("VRBodySetQueryable", &[bh(&r.body), Value::Bool(!off)]);
        }
    }
    // The native weighted hold is the only driver once every hand has registered its fixed grip: the velocity drive
    // below targets the previous drawn pose and fights that hold.
    let kind = rec_ref.borrow().kind;
    let native_corpse = kind == 12.0 && hands.iter().all(|&sd| s.carry.hold[sd].as_ref().is_some_and(|h| h.fast.is_some()));
    if native_corpse {
        rec_ref.borrow_mut().pictured = false;
        let proxy = rec_ref.borrow().proxy;
        for &side in &hands {
            let (distance, supported) = corpse_contact_micrometres(side, proxy);
            let h = s.carry.hold[side].as_mut().expect("held");
            h.d = None;
            if supported {
                if let Some(d) = distance {
                    h.lost = if d > f64::from(HAND_LOST_AT) * 1_000_000.0 { h.lost + dt } else { 0.0 };
                }
            }
            if h.lost > CORPSE_LOST {
                release(s, side, false);
            }
        }
        corpse_tick(s, rec_ref, &pos, dt);
        return;
    }
    v = mul(&v, 1.0 / n as f32);
    // where it should be, and how it should stand
    let two = rec_ref.borrow().two;
    let limb_hang = rec_ref.borrow().limb_hang;
    let (mut t, mut tq);
    if n == 1 || two.is_none() {
        let h = s.carry.hold[hands[0]].as_mut().expect("held");
        let k = (step / CARRY_PULL_TIME).min(1.0);
        if h.age >= PULL_WAIT {
            h.a = add(&h.a, &mul(&sub(&h.t, &h.a), k));
        }
        tq = qmul(&h.q, &h.r);
        let a = qrot(&h.q, &h.a);
        // a dead body's arm, leg or head turns as it hangs, about where it was taken
        let l = if limb_hang { qrot(&rot, &h.l) } else { qrot(&tq, &h.l) };
        t = sub(&add(&h.p, &a), &l);
    } else {
        let two = two.expect("checked");
        let (a, b) = (s.carry.hold[0].as_ref().expect("both"), s.carry.hold[1].as_ref().expect("both"));
        // how the two hands have turned since they took it, together
        let aq = qmul(&a.q, &conj(&a.q0));
        let mut bq = qmul(&b.q, &conj(&b.q0));
        if aq[0] * bq[0] + aq[1] * bq[1] + aq[2] * bq[2] + aq[3] * bq[3] < 0.0 {
            bq = bq.map(|x| -x);
        }
        let mut d = [aq[0] + bq[0], aq[1] + bq[1], aq[2] + bq[2], aq[3] + bq[3]];
        let mut dn = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2] + d[3] * d[3]).sqrt();
        if dn < 1e-4 {
            d = [0.0, 0.0, 0.0, 1.0];
            dn = 1.0;
        }
        d = d.map(|x| x / dn);
        // far apart, the line between them is what carries it, and of their turn only the part about that line
        let mut u = sub(&b.p, &a.p);
        let ul = len(&u);
        let far = if two.len > TWO_TURN_FROM { ((ul - TWO_TURN_FROM) / (TWO_TURN_FULL - TWO_TURN_FROM)).clamp(0.0, 1.0) } else { 0.0 };
        if far > 0.0 {
            u = mul(&u, 1.0 / ul);
            let dt_ = super::dot(&two.u, &u);
            if dt_ > -0.99 {
                let cx = [two.u[1] * u[2] - two.u[2] * u[1], two.u[2] * u[0] - two.u[0] * u[2], two.u[0] * u[1] - two.u[1] * u[0]];
                let mut c = [cx[0], cx[1], cx[2], 1.0 + dt_];
                let cl = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2] + c[3] * c[3]).sqrt();
                c = c.map(|x| x / cl);
                let along = d[0] * two.u[0] + d[1] * two.u[1] + d[2] * two.u[2];
                let mut sw = [two.u[0] * along, two.u[1] * along, two.u[2] * along, d[3]];
                let mut sn = (sw[0] * sw[0] + sw[1] * sw[1] + sw[2] * sw[2] + sw[3] * sw[3]).sqrt();
                if sn < 1e-4 {
                    sw = [0.0, 0.0, 0.0, 1.0];
                    sn = 1.0;
                }
                let mut l = qmul(&c, &sw.map(|x| x / sn));
                if l[0] * d[0] + l[1] * d[1] + l[2] * d[2] + l[3] * d[3] < 0.0 {
                    l = l.map(|x| -x);
                }
                for k in 0..4 {
                    d[k] += (l[k] - d[k]) * far;
                }
                let dn = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2] + d[3] * d[3]).sqrt();
                d = d.map(|x| x / dn);
            }
        }
        tq = qmul(&d, &two.q);
        let g = qrot(&tq, &mul(&add(&a.g, &b.g), 0.5));
        t = sub(&mul(&add(&a.p, &b.p), 0.5), &g);
    }
    // THE PICTURE (heldbody): where the thing sits in the first hand's frame, for drawing it on that hand as drawn.
    let (pa_p, pa_q, pa_d) = {
        let pa = s.carry.hold[hands[0]].as_ref().expect("held");
        (pa.p, pa.q, pa.d)
    };
    // the picture goes on the hand as drawn, so where that hand is drawn off the real one is taken out
    let dw = if n == 2 { pa_d.map_or([0.0; 3], |d| f.dir_to_world(&d)) } else { [0.0; 3] };
    let pg = qrot(&conj(&pa_q), &sub(&sub(&t, &pa_p), &dw));
    let wtq = tq;
    let ptq = qmul(&conj(&pa_q), &tq);
    let pt = t;
    // HOW FAST THE TARGET GOES, asked of the target: the hand, the player's own walking and turning, the swing of a grip
    // point held off the thing's middle, two hands alike.
    {
        let mut r = rec_ref.borrow_mut();
        match r.went {
            Some(w) => {
                let k = (step / TURN_TAU).min(1.0);
                let go = mul(&sub(&t, &w), 1.0 / step);
                r.go = add(&r.go, &mul(&sub(&go, &r.go), k));
                r.went = Some(t);
            }
            None => {
                r.went = Some(t);
                r.go = v;
            }
        }
        v = r.go;
    }
    // HOW THE TARGET TURNS, asked of the target itself
    let mut w = [0f32; 3];
    {
        let mut r = rec_ref.borrow_mut();
        match r.turn.as_mut() {
            Some(tr) => {
                let d = qmul(&tq, &conj(&tr.q));
                let sign = if d[3] < 0.0 { -2.0 } else { 2.0 };
                let k = (step / TURN_TAU).min(1.0);
                for i in 0..3 {
                    tr.w[i] += (d[i] * sign / step - tr.w[i]) * k;
                }
                tr.q = tq;
                let wl = len(&tr.w);
                let c = if wl > TURN_MOST { TURN_MOST / wl } else { 1.0 };
                w = mul(&tr.w, c);
            }
            None => r.turn = Some(Turn { q: tq, w: [0.0; 3] }),
        }
    }
    // and how it will stand, as where it will be
    {
        let a = s.carry.carry_lead;
        let ai = mul(&w, a * 0.5);
        let an = (len2(&ai) + 1.0).sqrt();
        tq = qmul(&[ai[0] / an, ai[1] / an, ai[2] / an, 1.0 / an], &tq);
    }
    // WHERE IT WILL BE, going as it goes, for comparing with where the hand will be.
    let vel = body_vel(&body);
    let ahead = step + s.carry.carry_lead;
    let goes = vel.map_or(pos, |vv| add(&pos, &mul(&vv, ahead)));
    t = add(&t, &mul(&v, s.carry.carry_lead));
    // The player's own walking turns round no faster than legs do: that part of the lead is a step.
    t = add(&t, &mul(&pv, (step - s.carry.carry_lead).max(0.0)));
    let miss = len(&sub(&t, &goes));
    // Is it going as it was told?
    {
        let mut r = rec_ref.borrow_mut();
        let mut slipped = false;
        if let (Some(vv), Some(told)) = (vel, r.told) {
            let sd = sub(&told, &vv);
            let tn = len2(&told);
            slipped = len2(&sd) > BLOCK_SLIP * BLOCK_SLIP && (tn < 1e-4 || super::dot(&vv, &told) / tn < BLOCK_ALONG);
        }
        r.slips = if slipped { r.slips + 1 } else { 0 };
        r.clear = if slipped { 0 } else { r.clear + 1 };
        if let Some(b) = r.blocked {
            if miss < BLOCK_FREE_MISS || r.clear >= BLOCK_FREE_STEPS {
                r.blocked = None;
            } else if slipped {
                r.blocked = Some(BLOCK_SPEED);
            } else {
                r.blocked = Some(b + BLOCK_RAMP * dt);
            }
        } else if r.kind != 12.0 && miss > BLOCK_MISS && r.slips >= BLOCK_STEPS {
            r.blocked = Some(BLOCK_SPEED);
            s.carry.block_said += 1;
            if s.carry.block_said <= 20 {
                let told = r.told.unwrap_or([0.0; 3]);
                let vv = vel.unwrap_or([0.0; 3]);
                log!(
                    "[HandColl] a carried thing ran into something: {:.3} m off, told ({:.2} {:.2} {:.2}), going ({:.2} {:.2} {:.2}), target going ({:.2} {:.2} {:.2})\n",
                    miss,
                    told[0],
                    told[1],
                    told[2],
                    vv[0],
                    vv[1],
                    vv[2],
                    v[0],
                    v[1],
                    v[2]
                );
            }
        }
    }
    let (proxy, index) = {
        let r = rec_ref.borrow();
        (r.proxy, r.index)
    };
    let hand_bone = data::data().hand[hands[0]].map_or(-1, |h| data::data().caps[h].index);
    {
        let mut r = rec_ref.borrow_mut();
        r.pictured = false;
        let was_held = r.kinematic;
        // a dead body's part held by a hand is the plugin's: nothing is told it here
        r.kinematic = r.kind == 12.0 && s.carry.hold.iter().flatten().any(|h| Rc::ptr_eq(&h.rec, rec_ref) && h.fast.is_some());
        if HOLD_BODY && r.kind != 12.0 {
            r.kinematic = nat::<bool>("VRHeldBody", &[Value::Int32(proxy as i32), Value::Int32(index as i32), Value::Int32(hand_bone), vp(&v4(&pg, 0.0)), vp(&ptq)]) == Some(true);
            if r.kinematic {
                nat::<bool>("VRHeldBodyGrasp", &[Value::Int32(proxy as i32), Value::Int32(index as i32), Value::Bool(GRASP_STREET && n == 1)]);
            }
            if r.kinematic != was_held {
                log!("[HandColl] proxy {} body {}: {}\n", proxy, index, if r.kinematic { "held by its own body, kinematic in the hand" } else { "carried by velocity" });
            }
        }
    }
    let kinematic = rec_ref.borrow().kinematic;
    if !kinematic {
        // A WALL AHEAD: along the way from the body to where the picture goes, how much room is there, less what the
        // thing measures in that direction?
        let mut reach = -1.0f32;
        let g = sub(&pt, &pos);
        let gap = len(&g);
        if gap > PICTURE_GAP && !super::super::game_system("GetSpatialQueriesSystem").is_null() {
            let dims = {
                let mut r = rec_ref.borrow_mut();
                if r.dims.is_none() {
                    r.dims = Some(nat::<[f32; 4]>("VRBodyGetDims", &[bh(&body)]).filter(|d| d[3] > 0.5).map(|d| super::f3(&d)));
                }
                r.dims.flatten()
            };
            let mut ex = PICTURE_MEASURE;
            if let Some(d) = dims {
                let l = qrot(&conj(&rot), &mul(&g, 1.0 / gap));
                ex = l[0].abs() * d[0] + l[1].abs() * d[1] + l[2].abs() * d[2];
            }
            // the body will have gone on by the time this is used
            ex += vel.map_or(0.0, |vv| len(&vv) * step * PICTURE_STEPS);
            let k = (gap + ex + PICTURE_MARGIN) / gap;
            if let Some(d) = ray_distance(s, &pos, &add(&pos, &mul(&g, k)), &pos, false) {
                reach = (d - ex - PICTURE_MARGIN).max(0.0);
            }
        }
        let blocked = rec_ref.borrow().blocked.is_some();
        let on = nat::<bool>("VRHeldPicture", &[Value::Int32(proxy as i32), Value::Int32(index as i32), Value::Int32(hand_bone), vp(&v4(&pg, reach)), vp(&ptq), Value::Bool(blocked)]);
        // held short of the wall, the hand stays on it too
        rec_ref.borrow_mut().pictured = on == Some(true) && !blocked && (reach < 0.0 || reach > gap * 0.5);
        if on == Some(false) {
            s.say_once(&format!("a carried thing whose picture stays on its body: kind {kind}"));
        }
    }
    // the pull, as far as these hands can give it
    let (mass, blocked) = {
        let r = rec_ref.borrow();
        (r.mass, r.blocked)
    };
    let accel = if kind == 12.0 { CARRY_ACCEL } else { carry_accel(mass, n) };
    let follow = CARRY_FOLLOW / step;
    let mut e = mul(&sub(&t, &pos), follow);
    if let Some(b) = blocked {
        v = [0.0; 3];
        let el = len(&e);
        if el > b {
            e = mul(&e, b / el);
        }
    }
    let mut want = add(&v, &e);
    want[2] += 9.81 * dt;
    let sl = len(&want);
    if sl > CARRY_MAX_SPEED {
        want = mul(&want, CARRY_MAX_SPEED / sl);
    }
    if let Some(vv) = vel {
        let dv = sub(&want, &vv);
        let dl = len(&dv);
        let most = accel * step;
        if dl > most {
            want = add(&vv, &mul(&dv, most / dl));
        }
    }
    if !kinematic {
        body_set_vel(&body, &want);
    }
    if kind == 12.0 {
        corpse_tick(s, rec_ref, &pos, dt);
    }
    // THE MEASUREMENT: a swing, frame by frame. Only reads. It goes to the log in one piece at its end.
    {
        let mut r = rec_ref.borrow_mut();
        if r.trace.is_none() && s.carry.trace_done < TRACE_TIMES && n == 1 && (len2(&v) > TRACE_SPEED * TRACE_SPEED || len2(&w) > 36.0) {
            s.carry.trace_done += 1;
            r.trace = Some(Vec::new());
        }
        if let Some(rows) = r.trace.as_mut().filter(|rows| rows.len() < TRACE_FRAMES) {
            let px = nat::<[f32; 4]>("VRBodyPhysXPos", &[Value::Int32(proxy as i32), Value::Int32(index as i32)]).unwrap_or([0.0; 4]);
            let rw = nat::<[f32; 4]>("VRBodyGetAngVel", &[bh(&body)]).unwrap_or([0.0; 4]);
            let ei = qmul(&tq, &conj(&rot));
            let vv = vel.unwrap_or([0.0; 3]);
            rows.push(format!(
                "{:.4} t {:.3} {:.3} {:.3} hand {:.3} {:.3} told {:.2} {:.2} {:.2} read {:.2} {:.2} {:.2} game {:.3} {:.3} {:.3} px {:.3} {:.3} {:.3} turn {:.2} {:.2} {:.2} spun {:.2} {:.2} {:.2} off {:.3}",
                now,
                t[0],
                t[1],
                t[2],
                v[0],
                v[1],
                want[0],
                want[1],
                want[2],
                vv[0],
                vv[1],
                vv[2],
                pos[0],
                pos[1],
                pos[2],
                px[0],
                px[1],
                px[2],
                w[0],
                w[1],
                w[2],
                rw[0],
                rw[1],
                rw[2],
                2.0 * (ei[0] * ei[0] + ei[1] * ei[1] + ei[2] * ei[2]).sqrt().atan2(ei[3].abs())
            ));
            if rows.len() == TRACE_FRAMES {
                for row in rows.iter() {
                    log!("[trace] {row}\n");
                }
            }
        }
        // what it was told, less the fall a step of gravity takes off it again; a body held by the plugin is told nothing
        if r.kinematic {
            r.told = None;
            r.blocked = None;
        } else {
            r.told = Some([want[0], want[1], want[2] - 9.81 * dt]);
        }
        let blocked = r.blocked.is_some();
        let spin = CARRY_SPIN * (accel / 60.0).min(1.0) * if blocked { 0.3 } else { 1.0 };
        let ei = qmul(&tq, &conj(&rot));
        let sign = if ei[3] < 0.0 { -1.0 } else { 1.0 };
        let ff = if blocked { 0.0 } else { 1.0 };
        // (a dead body's limb is not turned: set every frame, its turn would hold the whole body up on it as on a lever)
        if !r.limb_hang && !r.kinematic {
            body_set_ang_vel(&r.body, &[w[0] * ff + 2.0 * spin * ei[0] * sign, w[1] * ff + 2.0 * spin * ei[1] * sign, w[2] * ff + 2.0 * spin * ei[2] * sign]);
        }
    }
    // each hand: drawn on the thing
    let pictured = rec_ref.borrow().pictured;
    for (ai, &side) in hands.iter().enumerate() {
        let (seat_on, ht) = {
            let h = s.carry.hold[side].as_ref().expect("held");
            (h.seat_on, h.t)
        };
        // (seated by a grasp, the place taken is a bone, and it lies at its seat, not on the palm's skin)
        let sk = if seat_on { ht } else { palm_skin(s, side) };
        let hv = s.carry.hf[side];
        let view_on = s.view.on;
        let lead = s.carry.carry_lead;
        let first_say = {
            let h = s.carry.hold[side].as_ref().expect("held");
            !h.said && h.age > 0.5
        };
        if first_say {
            // WHAT WAS MEASURED, once: how far the place it took is from the palm as drawn
            let hb = bone_of_hand(s, side);
            let ps = palm_skin(s, side);
            let nl = s.carry.palm_local[side];
            let rd = s.carry.rd[side];
            let h = s.carry.hold[side].as_mut().expect("held");
            h.said = true;
            if let Some((hp, _)) = hb {
                let l = qrot(&rot, &h.l);
                let m2 = f.to_world(&hp);
                let r2 = qrot(&h.q, &ps);
                let g2 = sub(&add(&pos, &l), &add(&m2, &r2));
                let l2 = qrot(&conj(&h.q), &g2);
                let nlv = nl.unwrap_or([0.0; 3]);
                let d = rd.unwrap_or([0.0; 3]);
                log!(
                    "[HandColl] in the hand: place taken is ({:.3} {:.3} {:.3}) from the palm, {:.3} out of it; palm normal ({:.2} {:.2} {:.2}), wrist off target ({:.3} {:.3} {:.3}), {}, {} hands\n",
                    l2[0],
                    l2[1],
                    l2[2],
                    if nl.is_some() { super::dot(&l2, &nlv) } else { 0.0 },
                    nlv[0],
                    nlv[1],
                    nlv[2],
                    d[0],
                    d[1],
                    d[2],
                    if blocked.is_some() { "blocked" } else { "free" },
                    n
                );
            }
        }
        let h = s.carry.hold[side].as_mut().expect("held");
        let l = qrot(&rot, &h.l);
        let skw = qrot(&h.q, &sk);
        // and where this hand is going
        let mut dv = sub(&sub(&add(&goes, &l), &skw), &add(&h.p, &mul(&hv, lead)));
        let mut dev = len(&dv);
        if view_on {
            // the place taken, and from it to the palm, as it should be with the hand where it really is
            let p = add(&goes, &l);
            let to = add(&h.p, &skw);
            nat::<bool>("VRDebugPoint", &[vp(&v4(&p, 1.0)), Value::Float(0.02), Value::Int32(7)]);
            nat::<bool>("VRDebugLine", &[vp(&v4(&p, 1.0)), vp(&v4(&to, 1.0)), Value::Int32(7)]);
        }
        h.lost = if dev > HAND_LOST_AT { h.lost + dt } else { 0.0 };
        // TWO HANDS, ONE TAKEN FROM A LITTLE WAY OFF: drawn on the place it took, on the picture
        let on_picture = pictured && n == 2;
        if on_picture {
            let pl = qrot(&wtq, &h.l);
            dv = sub(&sub(&add(&pt, &pl), &skw), &h.p);
            dev = len(&dv);
        }
        // With its picture on the hand, the hand does not go to the body; nor does the hand a body held by its own actor
        // is steered to (that body goes where this hand is drawn). Only another hand on it is drawn on it.
        let steered = kinematic && (ai == 0 || kind == 12.0);
        h.d = if dev > HAND_SLACK && (on_picture || !pictured) && !steered {
            let k = (HAND_TRAIL.min(dev - HAND_SLACK)) / dev * (h.age / HAND_TO_IT_TIME).min(1.0);
            Some(f.to_model(&add(&f.b, &mul(&dv, k))))
        } else {
            None
        };
    }
    for &side in &hands {
        let lost = s.carry.hold[side].as_ref().is_some_and(|h| h.lost > if kind == 12.0 { CORPSE_LOST } else { HAND_LOST_FOR });
        if lost {
            release(s, side, false);
        }
    }
}

// ---------------------------------------------------------------- people
/// npcGrabFollow: A PERSON'S PART HELD BY A PHYSICS HAND (VRPhysHandsGrab names it) is pulled by that part's own bones,
/// kept where they stood from the controller as the grip closed. The physics hand stays on the part, so it follows what
/// it pulls; past what the part can reach the controller draws away from it, and the physics hand lets go.
fn npc_grab_follow(s: &mut State, side: usize, dt: f32, f: &Frame) {
    let (owner, _) = cvr_rig::vrik::physhands::grab::grabbed_person(side);
    let owner = owner.max(0);
    let stale = s.carry.npc_grip[side].as_ref().is_some_and(|g| !g.phys || g.owner != owner);
    if stale {
        let g = s.carry.npc_grip[side].take().expect("checked");
        cvr_rig::vrik::physhands::grab::set_grab_style(side, 0, 0);
        npc_grip_call(&g.npc, side, g.part, &[0.0; 4], &[0.0, 0.0, 0.0, 1.0]);
        if let Some(r) = g.rec {
            r.borrow_mut().held = false;
        }
        s.people.held_rec[side] = None;
    }
    if owner == 0 {
        return;
    }
    if s.carry.npc_grip[side].is_none() {
        let Some(rec) = s.people.find(owner / 32) else { return };
        let place = (owner % 32 + 1) as usize;
        let (kind, e, comp, style0, segment) = {
            let r = rec.borrow();
            (r.kinds.get(place).copied().flatten(), r.e.clone(), r.comp().clone(), r.grips.get(place).copied().flatten().unwrap_or(0), r.segments.get(place).copied().flatten())
        };
        let Some(&(part, slot, name)) = kind.and_then(|k| NPC_PARTS.get(k as usize)) else { return };
        if e.is_null() || comp.is_null() {
            return;
        }
        let Some((p, q)) = hand_tracked(s, side, f) else { return };
        let Some((wp, _)) = super::slot_transform(&comp, CName::hashed(slot)) else { return };
        let Some(eq) = super::world_rot(&e) else { return };
        let o = qrot(&conj(&q), &sub(&wp, &p));
        let mq = qmul(&conj(&eq), &q);
        let native_anchor = part < 4 && segment.is_some();
        rec.borrow_mut().held = true;
        s.people.held_rec[side] = Some(rec.clone());
        // THE GRASP: the part's own; the hips by their side (the hand out past their width: round it) or by the buttocks
        // or the belly (the palm), a thigh round it but by the buttock (behind it, at its top: the palm). The person's
        // frame: X to their right, Y ahead, Z up from their feet.
        let mut style = style0;
        if style == 6 || style == 7 {
            match super::world_pos(&e) {
                Some(en) => {
                    let l = qrot(&conj(&eq), &sub(&p, &en));
                    style = if style == 6 {
                        if l[0].abs() > 0.12 {
                            1
                        } else {
                            4
                        }
                    } else if l[1] < -0.02 && l[2] > 0.62 {
                        4
                    } else {
                        1
                    };
                }
                None => style = if style == 6 { 4 } else { 1 },
            }
        }
        if style > 0 {
            cvr_rig::vrik::physhands::grab::set_grab_style(side, owner, style);
        }
        const STYLES: [&str; 4] = ["round it like a can", "round it, wide", "on it gently", "the palm on it"];
        log!(
            "[HandColl] a physics hand holds a person by the {} (capsule {}): pulled by its bones, the fingers {}\n",
            name,
            owner % 32,
            STYLES.get((style - 1).max(0) as usize).filter(|_| style >= 1).copied().unwrap_or("as they lie")
        );
        s.carry.npc_grip[side] = Some(NpcGrip {
            npc: e,
            part,
            o,
            m: mq,
            torn: 0.0,
            age: 0.0,
            phys: true,
            owner,
            rec: Some(rec),
            native_anchor,
            segment: segment.unwrap_or(-1),
            dw: None,
            gap: None,
            d: None,
            said: false,
        });
        return;
    }
    npc_grip_tick(s, side, true, dt, f);
}
fn npc_grip_call(npc: &Handle, side: usize, part: i32, target: &[f32; 4], turn: &[f32; 4]) -> f32 {
    cvr_rig::vrik::npc_grip::grip(npc, side as i32, part, target, turn, &|| crate::npc_grip_natives::root_object(npc))
}
fn npc_grip_tick(s: &mut State, side: usize, grip: bool, dt: f32, f: &Frame) {
    let Some(g) = s.carry.npc_grip[side].as_ref() else { return };
    if g.native_anchor {
        if grip {
            // The native solver owns the bone-local contact and reads current paired tracking itself.
            let miss = cvr_rig::vrik::npc_grip::grip_bind(&g.npc, side as i32, g.part, g.segment, g.owner, &|| crate::npc_grip_natives::root_object(&g.npc));
            if miss >= 0.0 {
                return;
            }
        }
        npc_grip_call(&g.npc, side, g.part, &[0.0; 4], &[0.0, 0.0, 0.0, 1.0]);
        s.carry.npc_grip[side] = None;
        return;
    }
    let (phys, npc, part) = (g.phys, g.npc.clone(), g.part);
    let hand = if phys { hand_tracked(s, side, f) } else { hand_real(s, side, f) };
    let (e, eq) = (super::world_pos(&npc), super::world_rot(&npc));
    let sk = palm_skin(s, side);
    let g = s.carry.npc_grip[side].as_mut().expect("checked");
    let mut ok = grip && hand.is_some() && e.is_some() && eq.is_some();
    let (mut m, mut t) = ([0f32; 3], [0.0, 0.0, 0.0, 1.0]);
    if ok {
        let ((p, q), e, eq) = (hand.expect("checked"), e.expect("checked"), eq.expect("checked"));
        // A HEAD OR A CHEST GOES WHERE THE PALM GOES; the turn itself is passed on below
        let w = if g.part >= 4 { qrot(&q, &sk) } else { qrot(&q, &g.o) };
        m = qrot(&conj(&eq), &sub(&add(&p, &w), &e));
        ok = m[0] * m[0] + m[1] * m[1] + (m[2] - 1.3) * (m[2] - 1.3) < NPC_REACH * NPC_REACH;
        let nq = qmul(&conj(&eq), &q);
        t = qmul(&nq, &conj(&g.m));
    }
    if ok {
        let answer = npc_grip_call(&npc, side, part, &v4(&m, 1.0), &t);
        if answer < 0.0 {
            s.carry.npc_grip[side] = None;
            s.say_once(&format!("a person could not be held: {answer}"));
            return;
        }
        // solved once on a pose, a neck gives some 11 cm: a head or a chest is let go of a little further off
        g.torn = if answer > if g.part >= 4 { 0.25 } else { NPC_TEAR } { g.torn + dt } else { 0.0 };
        // a physics hand's hold lets go by itself (the controller drawn grab_release from it)
        ok = g.phys || g.torn < NPC_TEAR_FOR;
        if !ok {
            pulse(side);
        }
    }
    if !ok {
        npc_grip_call(&npc, side, part, &[0.0; 4], &[0.0, 0.0, 0.0, 1.0]);
        s.carry.npc_grip[side] = None;
    } else if let Some(dwv) = g.dw {
        g.age += dt;
        let k = (g.age / HAND_TO_IT_TIME).min(1.0);
        let (_, q) = hand.expect("ok");
        let w = if g.part >= 4 { qrot(&eq.expect("ok"), &dwv) } else { qrot(&q, &dwv) };
        g.d = Some(qrot(&f.i, &mul(&w, k)));
        if !g.said && g.age > 0.5 {
            g.said = true;
            log!("[HandColl] the hand on a person: to go {:.3} m, asked {:.3} m\n", g.gap.unwrap_or(0.0), len(&w));
        }
    }
}
/// npcNear: a living person whose cylinder the hand is within.
fn npc_near(s: &mut State, side: usize, f: &Frame) -> Option<Handle> {
    let (p, _) = hand_world(s, side, f)?;
    for n in &s.people.list {
        let Some(e) = super::world_pos(n) else { continue };
        let d = sub(&p, &e);
        if d[0] * d[0] + d[1] * d[1] < NPC_TOUCH * NPC_TOUCH && d[2] > 0.0 && d[2] < 2.0 && super::m::<bool>(n, "IsDead", &[]) != Some(true) {
            return Some(n.clone());
        }
    }
    None
}
/// npcTick: WITH THE PHYSICS HANDS the hands rest on people, take hold of them and stroke their hair: a bump each time
/// had a passer-by turn on V within a few touches. Only a blow is one: the hand at NPC_BLOW or faster.
fn npc_tick(s: &mut State, side: usize, edge: bool, f: &Frame) {
    let now = super::clock();
    let v = s.carry.hf[side];
    if s.phys {
        if edge || len2(&v) < NPC_BLOW * NPC_BLOW || now < s.carry.npc_next {
            return;
        }
    } else if !edge && (len2(&v) < NPC_MOVING * NPC_MOVING || now < s.carry.npc_next) {
        return;
    }
    let Some(n) = npc_near(s, side, f) else { return };
    s.carry.npc_next = now + NPC_EVERY;
    let pl = super::player();
    let broadcaster = super::rt::obj(&pl, "GetStimBroadcasterComponent", &[]);
    let sent = super::rt::call(&broadcaster, "SendDrirectStimuliToTarget", &[super::A::H(&pl), super::A::N(super::super::menu_rt::ev("gamedataStimType", "Bump")), super::A::H(&n)]).is_some();
    s.carry.npc_said += 1;
    if s.carry.npc_said <= 6 {
        log!("[HandColl] a person touched: bump stimulus, {}\n", if sent { "sent" } else { "failed" });
    }
    pulse(side);
}

// ---------------------------------------------------------------- the state of things
/// settleTick: kinematic again, what has lain still long enough; the queries back on what was let go a moment ago.
fn settle_tick(s: &mut State, dt: f32) {
    ragdoll_tick(s);
    let now = super::clock();
    let back: Vec<RecRef> = s.carry.query_back.iter().filter(|(_, at)| now >= *at).map(|(r, _)| r.clone()).collect();
    s.carry.query_back.retain(|(_, at)| now < *at);
    for rec in back {
        let mut r = rec.borrow_mut();
        r.query_off = None;
        if body_pos(&r.body).is_some() {
            nat::<bool>("VRBodySetQueryable", &[bh(&r.body), Value::Bool(true)]);
        }
    }
    let woken = std::mem::take(&mut s.carry.woken);
    for rec in woken {
        if s.carry.hold.iter().flatten().any(|h| Rc::ptr_eq(&h.rec, &rec)) {
            rec.borrow_mut().settled = 0.0;
            s.carry.woken.push(rec);
            continue;
        }
        let body = rec.borrow().body.clone();
        match body_vel(&body) {
            None => {}
            Some(v) if len2(&v) > SETTLE_SPEED * SETTLE_SPEED => {
                rec.borrow_mut().settled = 0.0;
                s.carry.woken.push(rec);
            }
            Some(_) => {
                let settled = {
                    let mut r = rec.borrow_mut();
                    r.settled += dt;
                    r.settled
                };
                if settled >= SETTLE_FOR {
                    nat::<bool>("VRBodySetKinematic", &[bh(&body), Value::Bool(true)]);
                    rec.borrow_mut().checked = false;
                    s.say_once("a thing lay still and went back to being kinematic");
                } else {
                    s.carry.woken.push(rec);
                }
            }
        }
    }
}
/// carryState: CARRYING A BODY, AS THE GAME KNOWS IT -- while a hand holds a dead body the player state machine's
/// Carrying is set and the CarryBody stimulus is on the player, as the game's own carry sets them. Only what this set is
/// taken off again.
pub(super) fn carry_state(s: &mut State, on: bool) {
    if on == s.carry.carry_want {
        return;
    }
    s.carry.carry_want = on;
    let pl = super::player();
    let mut failed = None;
    if !pl.is_null() {
        let psm = super::rt::obj(&pl, "GetPlayerStateMachineBlackboard", &[]);
        let id = super::super::boards::defs().and_then(|d| super::super::boards::definition(&d, "PlayerStateMachine").and_then(|def| super::super::boards::field(def, "Carrying")));
        let b = super::rt::obj(&pl, "GetStimBroadcasterComponent", &[]);
        match (id, psm.is_null()) {
            (Ok(id), false) => {
                let carry_body = super::super::menu_rt::ev("gamedataStimType", "CarryBody");
                if on {
                    if super::super::boards::get::<bool>(&psm, "GetBool", id) != Ok(true) {
                        super::rt::call(&psm, "SetBool", &[super::A::P(id), super::A::B(true)]);
                        if !b.is_null() {
                            super::rt::call(&b, "AddActiveStimuli", &[super::A::H(&pl), super::A::N(carry_body), super::A::N(-1.0)]);
                        }
                        s.carry.carry_set = true;
                    }
                } else if s.carry.carry_set {
                    super::rt::call(&psm, "SetBool", &[super::A::P(id), super::A::B(false)]);
                    if !b.is_null() {
                        super::rt::call(&b, "RemoveActiveStimuliByName", &[super::A::H(&pl), super::A::N(carry_body)]);
                    }
                    s.carry.carry_set = false;
                }
            }
            (Err(e), _) => failed = Some(e),
            (_, true) => failed = Some("no state machine blackboard".into()),
        }
    }
    if let Some(e) = failed {
        s.say_once(&format!("the player state machine's Carrying failed: {e}"));
    }
    log!("[HandColl] carrying a dead body: {}{}\n", if on { "yes" } else { "no" }, if s.carry.carry_set { " (set)" } else { "" });
}
/// weightTick: WEIGHT SLOWS THE PLAYER, from 3 kg on, to half at 30. A plain modifier: it is not saved, and it is taken
/// off again with the last thing let go.
fn weight_tick(s: &mut State) {
    let mut mass = 0.0f32;
    let mut seen: Vec<(bool, u32, usize)> = Vec::new();
    for side in 0..2 {
        let Some(h) = s.carry.hold[side].as_ref() else { continue };
        let r = h.rec.borrow();
        // a dead body is one, by whatever parts the hands have it
        let key = if r.kind == 12.0 { (true, r.proxy, 0) } else { (false, 0, Rc::as_ptr(&h.rec) as usize) };
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        let both = s.carry.hold[1 - side].as_ref().is_some_and(|o| Rc::ptr_eq(&o.rec, &h.rec) || (r.kind == 12.0 && o.rec.borrow().kind == 12.0 && o.rec.borrow().proxy == r.proxy));
        mass += r.whole.unwrap_or(r.mass) / if both { CARRY_TWO_HANDS } else { 1.0 };
    }
    let want = (1.0 - (mass - 3.0).max(0.0) / 54.0).max(0.5);
    if s.carry.weight_at.is_some_and(|w| (w - want).abs() < 0.02) {
        return;
    }
    s.carry.weight_at = Some(want);
    let pl = super::player();
    let Some(id) = super::entity_hash(&pl) else { return };
    let stats = super::super::game_system("GetStatsSystem");
    let target = [id, 0u64];
    if !s.carry.weight_mod.is_null() {
        super::m::<bool>(&stats, "RemoveModifier", &[Value::Ptr(target.as_ptr().cast()), Value::Handle(&s.carry.weight_mod)]);
        s.carry.weight_mod = Handle::null();
    }
    if want < 0.999 {
        let st = crate::script_call::enum_value("gamedataStatType", "MaxSpeed").unwrap_or(0) as u64;
        let mult = crate::script_call::enum_value("gameStatModifierType", "Multiplier").unwrap_or(0) as u64;
        let md = crate::script_call::static_function("RPGManager", "CreateStatModifier")
            // SAFETY: a static function (no context).
            .and_then(|f| unsafe { crate::script_call::invoke_handle(core::ptr::null_mut(), f, &[Value::U64(st), Value::U64(mult), Value::Float(want)]) })
            .unwrap_or_else(Handle::null);
        if md.is_null() || super::m::<bool>(&stats, "AddModifier", &[Value::Ptr(target.as_ptr().cast()), Value::Handle(&md)]).is_none() {
            s.carry.weight_at = Some(1.0);
            s.say_once("no slowing the player down: CreateStatModifier failed");
            return;
        }
        s.carry.weight_mod = md;
    }
}
/// The Reach mechanic's claim on this hand: where it has something for this hand, this one stands back.
fn reach_busy(side: usize, near_pickup: bool) -> bool {
    super::super::reach::hand_busy(side + 1, near_pickup)
}
fn vehicle_hand_busy(side: usize) -> bool {
    nat::<bool>("VRVehicleHandBusy", &[Value::Int32(side as i32)]) == Some(true)
}

/// HC.carry: the hands' carrying, every frame.
pub(super) fn carry(s: &mut State, allowed: bool, holder: Option<i32>, dt: f32, f: &Frame) {
    s.pf("before carrying");
    settle_tick(s, dt);
    s.pf("settle");
    for side in 0..2 {
        hand_measure(s, side);
        let grip = shared(GRIP_SLOT[side]) > 0.5 && !vehicle_hand_busy(side);
        let edge = grip && !s.carry.grip_was[side];
        s.carry.grip_was[side] = grip;
        let mine = allowed && holder != Some(side as i32) && !s.skip[side];
        if s.phys {
            // THE PHYSICS HANDS TAKE HOLD: what they hold is pulled by its bones
            npc_grab_follow(s, side, dt, f);
        } else if s.carry.npc_grip[side].is_some() {
            npc_grip_tick(s, side, grip && mine, dt, f);
        }
        // (not with the grip closed on the physics hands: the bump came as the hand took hold, and the person flinched)
        if mine && s.carry.npc_grip[side].is_none() && !(s.phys && grip) {
            npc_tick(s, side, edge, f);
        }
        s.pf("people");
        let held_dead = s.carry.hold[side].as_ref().is_some_and(|h| h.rec.borrow().kind == 12.0);
        if s.carry.corpse_grasped[side] && !held_dead {
            corpse_let_go(s, side);
        }
        if s.carry.hold[side].is_some() {
            if !mine {
                release(s, side, false);
            } else if !grip {
                release(s, side, true);
            }
        } else if edge && mine && !reach_busy(side, true) {
            carry_find(s, side, f, None, false);
            if s.carry.hold[side].is_none() {
                dead_force(s, side, f);
            }
        } else if grip && mine && s.carry.forced[side].is_some() {
            if super::clock() > s.carry.forced[side].expect("checked") {
                s.carry.forced[side] = None;
            } else {
                carry_find(s, side, f, None, false);
            }
        } else if grip && mine && s.carry.npc_grip[side].is_none() {
            if let Some(other) = s.carry.hold[1 - side].as_ref().map(|h| h.rec.clone()) {
                // closed already, and coming to what the other hand has: it takes hold when it gets there
                s.carry.reach_turn += 1;
                if s.carry.reach_turn % REACH_EVERY == 0 {
                    carry_find(s, side, f, Some(&other), false);
                }
            }
        }
        s.pf("taking");
    }
    weight_tick(s);
    s.pf("weight");
    let first = s.carry.hold[0].as_ref().map(|h| h.rec.clone());
    let second = s.carry.hold[1].as_ref().map(|h| h.rec.clone());
    if let Some(r) = first.clone() {
        drive(s, &r, dt, f);
    }
    if let Some(r) = second {
        if first.as_ref().is_none_or(|a| !Rc::ptr_eq(a, &r)) {
            drive(s, &r, dt, f);
        }
    }
    let dead = s.carry.hold.iter().flatten().any(|h| h.rec.borrow().kind == 12.0);
    carry_state(s, dead);
    s.pf("carrying");
}
fn shared(i: i32) -> f32 {
    super::shared(i)
}
/// HC.suspend: everything held let go, every person's part, every hair, the queries given back.
pub(super) fn suspend(s: &mut State) {
    for side in 0..2 {
        release(s, side, false);
        s.carry.forced[side] = None;
        if s.carry.corpse_grasped[side] || s.carry.fast[side] {
            corpse_let_go(s, side);
        }
        if let Some(g) = s.carry.npc_grip[side].take() {
            npc_grip_call(&g.npc, side, g.part, &[0.0; 4], &[0.0, 0.0, 0.0, 1.0]);
            if let Some(r) = g.rec {
                r.borrow_mut().held = false;
            }
            s.people.held_rec[side] = None;
        }
    }
    for (rec, _) in std::mem::take(&mut s.carry.query_back) {
        let mut r = rec.borrow_mut();
        nat::<bool>("VRBodySetQueryable", &[bh(&r.body), Value::Bool(true)]);
        r.query_off = None;
    }
    super::hair::release_all(s);
    carry_state(s, false);
    weight_tick(s);
}
/// carryFind's probe (probeOnly): whether this hand would find a body to grip, nothing taken.
pub(super) fn probe(s: &mut State, side: usize, f: &Frame) -> bool {
    carry_find(s, side, f, None, true)
}
