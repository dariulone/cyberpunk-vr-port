//! The bodies a hand reaches (main's src/Natives/HandBody.cpp, its 2026-11 additions): the body a ray hit, what a hand
//! closing here can take, what it points at, a body lying dead carried in one call, a body held at a point by a
//! physics hand, the grasp on a part's capsule, and a broken thing's body refused (debug_view's body_gone).
//!
//! THE BODY A RAY HIT. physicsTraceResult is 0x60 bytes and script sees three fields of it. The rest, read off the
//! game's own conversion from a PhysX hit (sub_1403DDE38 in 2.5): +0x48 the proxy, +0x4C the body within it, +0x50 the
//! shape, +0x54 the distance, +0x59 the proxy's kind. And entPhysicalBodyInterface is nothing but that pair: its natives
//! read +0x40 as the proxy and +0x44 as the body and look the rest up. So one instance, filled in from a hit, IS the body
//! it hit, and every VRBody* native works on it. Small props are kind 10 (aggregate), 1 (physical system) or 3
//! (destruction); kind 7 is the world's baked collision, mass 0.
use super::*;
use std::sync::Mutex;

/// AND NEVER ON A BROKEN THING. Every call on an entPhysicalBodyInterface made by new_body has +0x40 and +0x44 the proxy
/// and the body. A destructible's body whose actor the game has released still reaches that freed actor through this
/// interface; a velocity written there put it back into the scene's queries, and the player's controller crashed on it.
pub(super) unsafe fn body_broken(body: *const c_void) -> bool {
    if body.is_null() {
        return true;
    }
    let raw = body as *const u8;
    crate::vrik::debug_view::body_gone(*(raw.add(0x40) as *const u32), *(raw.add(0x44) as *const u32))
}
unsafe fn ids(body: *const c_void) -> (u32, u32) {
    let raw = body as *const u8;
    (*(raw.add(0x40) as *const u32), *(raw.add(0x44) as *const u32))
}
unsafe fn set_ids(body: *mut c_void, proxy: u32, part: u32) {
    let raw = body as *mut u8;
    *(raw.add(0x40) as *mut u32) = proxy;
    *(raw.add(0x44) as *mut u32) = part;
}

const PROXY_PHYSICAL_SYSTEM: i32 = 1;
const PROXY_DESTRUCTION: i32 = 3;
const PROXY_AGGREGATE: i32 = 10;
/// What a hand can move: 12 is a ragdoll (a body lying dead can be taken by the limb).
fn trace_moves(kind: i32) -> bool {
    kind == PROXY_PHYSICAL_SYSTEM || kind == PROXY_DESTRUCTION || kind == PROXY_AGGREGATE || kind == 12
}
/// A new entPhysicalBodyInterface for (proxy, body).
unsafe fn new_body(proxy: u32, body: u32) -> *mut c_void {
    let Some(rtti) = RttiSystem::get() else { return core::ptr::null_mut() };
    let Some(class) = body_class(rtti) else { return core::ptr::null_mut() };
    let instance = class.create_instance();
    if !instance.is_null() {
        set_ids(instance, proxy, body);
    }
    instance
}
/// One kept instance per use, set to each body asked of in turn (main's static scratch bodies).
struct Scratch(usize);
impl Scratch {
    unsafe fn at(&mut self, proxy: u32, body: u32) -> *mut c_void {
        if self.0 == 0 {
            self.0 = new_body(proxy, body) as usize;
        }
        let b = self.0 as *mut c_void;
        if !b.is_null() {
            set_ids(b, proxy, body);
        }
        b
    }
}
static TRACE_SCRATCH: Mutex<Scratch> = Mutex::new(Scratch(0));
static NEAR_SCRATCH: Mutex<Scratch> = Mutex::new(Scratch(0));
static PICK_SCRATCH: Mutex<Scratch> = Mutex::new(Scratch(0));
static INFO_SCRATCH: Mutex<Scratch> = Mutex::new(Scratch(0));
fn scratch(s: &'static Mutex<Scratch>) -> std::sync::MutexGuard<'static, Scratch> {
    s.lock().unwrap_or_else(|e| e.into_inner())
}

/// Once per proxy in the log: the numbers the next test is read from.
static SEEN: Mutex<([u32; 256], usize)> = Mutex::new(([0; 256], 0));
fn trace_note(proxy: u32, body: u32, shape: u32, kind: u8, mass: f32) {
    let mut s = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    if s.0.contains(&(proxy + 1)) {
        return;
    }
    let at = s.1 % 256;
    s.0[at] = proxy + 1;
    s.1 += 1;
    cvr_host::log!("[HandColl] touched proxy={} body={} shape={} kind={} mass={:.2}\n", proxy, body, shape, kind, mass);
}
/// physicsTraceResult, 0x60 bytes.
#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct TraceResult([u8; 0x60]);
impl Default for TraceResult {
    fn default() -> Self {
        TraceResult([0; 0x60])
    }
}
fn read_trace(t: &TraceResult) -> (u32, u32, u32, u8) {
    let u = |o: usize| u32::from_le_bytes([t.0[o], t.0[o + 1], t.0[o + 2], t.0[o + 3]]);
    (u(0x48), u(0x4C), u(0x50), t.0[0x59])
}
unsafe fn mass_by_call(b: *mut c_void) -> f32 {
    let mut mass = 0.0f32;
    body_call_get(b, "GetMass", c"Float", as_bytes(&mut mass));
    mass
}

/// VRTraceInfo(trace) -> Vector4: x the kind, y the proxy, z the body, w the mass. All zero when the trace carries no
/// proxy.
#[no_mangle]
pub unsafe extern "C" fn VRTraceInfo(_this: *mut c_void, frame: *mut Frame, out: *mut Vector4, _unused: i64) {
    let mut trace = TraceResult::default();
    params!(frame, trace);
    let Some(out) = out.as_mut() else { return };
    *out = ZERO4;
    let (proxy, body, shape, kind) = read_trace(&trace);
    if proxy == 0 {
        return;
    }
    let mut mass = 0.0f32;
    if trace_moves(i32::from(kind)) {
        let b = scratch(&TRACE_SCRATCH).at(proxy, body);
        if !b.is_null() {
            mass = mass_by_call(b);
        }
    }
    trace_note(proxy, body, shape, kind, mass);
    *out = v4(f32::from(kind), proxy as f32, body as f32, mass);
}
unsafe fn put_handle(out: *mut c_void, instance: *mut c_void) {
    let out = out.cast::<Handle>();
    if !out.is_null() && !instance.is_null() {
        drop(core::ptr::replace(out, Handle::from_instance(instance)));
    }
}
/// VRTraceBody(trace) -> the body, for the VRBody* natives. Null for a kind that does not move.
#[no_mangle]
pub unsafe extern "C" fn VRTraceBody(_this: *mut c_void, frame: *mut Frame, out: *mut c_void, _unused: i64) {
    let mut trace = TraceResult::default();
    params!(frame, trace);
    let (proxy, body, _, kind) = read_trace(&trace);
    if proxy == 0 || !trace_moves(i32::from(kind)) {
        return;
    }
    put_handle(out, new_body(proxy, body));
}

/// WHAT A BODY WEIGHS, kept: a look at what a hand points at spent up to 33 ms on the bodies it found, not on finding
/// them, and a body's mass stays what it is. A body held now reads its own mass, not the solver's (the hold raises it).
struct MassSeen {
    seen: [(u32, u32, f32, u64); 256],
    ms: f64,
}
static MASSES: Mutex<MassSeen> = Mutex::new(MassSeen { seen: [(0, 0, 0.0, 0); 256], ms: 0.0 });
unsafe fn mass_of(b: *mut c_void, proxy: u32, body: u32) -> f32 {
    let now = crate::vrik::physhands::tick_ms();
    let slot = ((proxy.wrapping_mul(31).wrapping_add(body)) % 256) as usize;
    if let Some(original) = crate::vrik::physhands::adjust::original_held_mass(proxy, body) {
        // Replace even an earlier cached solver mass. After release this cache must still describe the body's real
        // mass, not its temporary 100 kg.
        MASSES.lock().unwrap_or_else(|e| e.into_inner()).seen[slot] = (proxy, body, original, now);
        return original;
    }
    {
        let m = MASSES.lock().unwrap_or_else(|e| e.into_inner());
        let s = m.seen[slot];
        if s.3 != 0 && s.0 == proxy && s.1 == body && now - s.3 < 60000 {
            return s.2;
        }
    }
    let (a, f) = crate::vrik::physhands::qpc();
    let mut mass = mass_by_call(b);
    if let Some(original) = crate::vrik::physhands::adjust::original_held_mass(proxy, body) {
        mass = original;
    }
    let (t, _) = crate::vrik::physhands::qpc();
    let mut m = MASSES.lock().unwrap_or_else(|e| e.into_inner());
    m.ms += (t - a) as f64 * 1000.0 / f as f64;
    m.seen[slot] = (proxy, body, mass, now);
    mass
}

/// VRNearBody(center, radius, most mass) -> Vector4 as VRTraceInfo: what a hand closing here can take, nearest first,
/// from PhysX's own list. All zero if nothing is.
#[no_mangle]
pub unsafe extern "C" fn VRNearBody(_this: *mut c_void, frame: *mut Frame, out: *mut Vector4, _unused: i64) {
    let (mut center, mut radius, mut most) = (Vector4::default(), 0.1f32, 30.0f32);
    params!(frame, center, radius, most);
    let Some(out) = out.as_mut() else { return };
    *out = ZERO4;
    let found = crate::vrik::debug_view::pick::find_near(&[center.x, center.y, center.z], radius, 16);
    let mut s = scratch(&NEAR_SCRATCH);
    for n in found {
        let kind = crate::vrik::debug_view::pick::proxy_kind(n.proxy);
        if !trace_moves(kind) {
            continue;
        }
        let b = s.at(n.proxy, n.body);
        if b.is_null() {
            return;
        }
        let mass = mass_of(b, n.proxy, n.body);
        // no mass: not the simulation's to move; exactly 1 kg of kind 1: the hand's own capsule
        if mass <= 0.01 || mass > most || (kind == 1 && mass == 1.0) {
            continue;
        }
        *out = v4(kind as f32, n.proxy as f32, n.body as f32, mass);
        return;
    }
}

static LOOK_SAID: core::sync::atomic::AtomicI32 = core::sync::atomic::AtomicI32::new(0);
/// VRPickCone(from, a, b, cone, range, most mass, keep proxy, keep body) -> Vector4 as VRTraceInfo: what a hand points
/// at, along either direction. All zero if nothing.
#[no_mangle]
pub unsafe extern "C" fn VRPickCone(_this: *mut c_void, frame: *mut Frame, out: *mut Vector4, _unused: i64) {
    let (mut from, mut a, mut b) = (Vector4::default(), Vector4::default(), Vector4::default());
    let (mut cone, mut range, mut most, mut keep_proxy, mut keep_body) = (0.15f32, 6.0f32, 6.0f32, 0i32, 0i32);
    params!(frame, from, a, b, cone, range, most, keep_proxy, keep_body);
    let Some(out) = out.as_mut() else { return };
    *out = ZERO4;
    // A LONG LOOK, in the log: what of it took so long. Frames of 40 to 70 ms were measured in the pointing.
    let (start, frequency) = crate::vrik::physhands::qpc();
    MASSES.lock().unwrap_or_else(|e| e.into_inner()).ms = 0.0;
    let found = crate::vrik::debug_view::pick::pick_cone(&[from.x, from.y, from.z], &[a.x, a.y, a.z], &[b.x, b.y, b.z], cone, range, keep_proxy as u32, keep_body as u32, 16);
    let (walked, _) = crate::vrik::physhands::qpc();
    let count = found.len();
    let mut s = scratch(&PICK_SCRATCH);
    for n in found {
        let kind = crate::vrik::debug_view::pick::proxy_kind(n.proxy);
        if !trace_moves(kind) || kind == 12 {
            continue;
        }
        let body = s.at(n.proxy, n.body);
        if body.is_null() {
            break;
        }
        let mass = mass_of(body, n.proxy, n.body);
        if mass <= 0.01 || mass > most || (kind == 1 && mass == 1.0) {
            continue;
        }
        *out = v4(kind as f32, n.proxy as f32, n.body as f32, mass);
        break;
    }
    let (end, _) = crate::vrik::physhands::qpc();
    let unit = 1000.0 / frequency as f64;
    let whole = (end - start) as f64 * unit;
    if whole > 8.0 && LOOK_SAID.fetch_add(1, core::sync::atomic::Ordering::Relaxed) < 60 {
        cvr_host::log!(
            "[pick] a look took {:.1} ms: {:.1} walking the actors, {:.1} on {} of them, of that {:.1} asking masses\n",
            whole,
            (walked - start) as f64 * unit,
            (end - walked) as f64 * unit,
            count,
            MASSES.lock().unwrap_or_else(|e| e.into_inner()).ms
        );
    }
}

// ---- A BODY LYING DEAD, IN ONE CALL. Done from the script it was a call per limb and per thing asked of it: some sixty
// a frame through CET, and 6 ms measured. Its limbs keep their weight: the limb in the hand goes where it goes and the
// rest hangs from it, each no further below the hand than it lay from that limb. Only what would hang lower than that is
// held up, gently, so a hand near the floor drags a body with all its weight and a hand raised lifts it from the top
// down. Limbs at the same spot are one.
const CORPSE_LIMBS: usize = 64;
const CORPSES: usize = 16;
const LIMB_LONGEST: f32 = 1.7;
const LIMB_LIFT: f32 = 14.0;
const LIMB_PULL: f32 = 30.0;
const LIMB_CALM: f32 = 0.96;
const LIMB_RISE: f32 = 1.6;
/// Towed along the floor after a limb in the hand (VRCorpseHold, other -2): past how far it lay from it plus TOW_SLACK,
/// a limb is drawn toward it at TOW_PULL per metre past (1/s^2), at most TOW_MOST m/s^2 and TOW_SPEED m/s that way.
const TOW_SLACK: f32 = 0.05;
const TOW_PULL: f32 = 40.0;
const TOW_MOST: f32 = 20.0;
const TOW_SPEED: f32 = 2.0;
#[derive(Clone, Copy)]
struct CorpseLimb {
    body: u16,
    hang: f32,
}
#[derive(Clone, Copy)]
struct Corpse {
    proxy: u32,
    bodies: i32,
    count: usize,
    limbs: [CorpseLimb; CORPSE_LIMBS],
    whole: f32,
    held_when_measured: i32,
}
const CORPSE0: Corpse = Corpse { proxy: 0, bodies: 0, count: 0, limbs: [CorpseLimb { body: 0, hang: 0.0 }; CORPSE_LIMBS], whole: 0.0, held_when_measured: 0 };
struct Corpses {
    c: [Corpse; CORPSES],
    next: usize,
    scratch: Scratch,
}
static CORPSE_STATE: Mutex<Corpses> = Mutex::new(Corpses { c: [CORPSE0; CORPSES], next: 0, scratch: Scratch(0) });
impl Corpses {
    fn find(&self, proxy: u32, held: i32) -> Option<usize> {
        self.c.iter().position(|c| c.proxy == proxy && (held < 0 || c.held_when_measured == held))
    }
    unsafe fn transform(&mut self, proxy: u32, body: u32) -> Option<Transform> {
        let b = self.scratch.at(proxy, body);
        let mut t = Transform::default();
        (!b.is_null() && body_call_get(b, "GetTransform", c"Transform", as_bytes(&mut t))).then_some(t)
    }
    unsafe fn velocity(&mut self, proxy: u32, body: u32) -> Option<Vector4> {
        let b = self.scratch.at(proxy, body);
        let mut v = Vector4::default();
        (!b.is_null() && body_call_get(b, "GetLinearVelocity", c"Vector4", as_bytes(&mut v))).then_some(v)
    }
    unsafe fn set_velocity(&mut self, proxy: u32, body: u32, mut v: Vector4) {
        let b = self.scratch.at(proxy, body);
        if !b.is_null() {
            body_call_set(b, "SetLinearVelocity", c"Vector4", &mut v);
        }
    }
    unsafe fn mass(&mut self, proxy: u32, body: u32) -> f32 {
        let b = self.scratch.at(proxy, body);
        if b.is_null() {
            0.0
        } else {
            mass_of(b, proxy, body)
        }
    }
    /// `held` is what the distances are from. Each held limb needs its own reference lengths: two hands on different
    /// parts used to replace one proxy-wide entry every call, re-reading the entire ragdoll and gradually adopting its
    /// stretched pose as the baseline.
    unsafe fn measure(&mut self, proxy: u32, held: i32) -> usize {
        let i = match self.find(proxy, held) {
            Some(i) => i,
            None => {
                let i = self.next;
                self.next = (self.next + 1) % CORPSES;
                i
            }
        };
        let bodies = crate::vrik::debug_view::pick::proxy_bodies(proxy);
        let mut c = CORPSE0;
        c.proxy = proxy;
        c.held_when_measured = held;
        c.bodies = bodies.min(CORPSE_LIMBS as i32);
        let Some(from) = self.transform(proxy, held as u32) else {
            self.c[i] = c;
            return i;
        };
        let mut seen: Vec<[f32; 3]> = vec![[from.position.x, from.position.y, from.position.z]];
        c.whole += self.mass(proxy, held as u32);
        for b in 0..c.bodies {
            if b == held {
                continue;
            }
            let Some(t) = self.transform(proxy, b as u32) else { continue };
            let p = [t.position.x, t.position.y, t.position.z];
            if seen.iter().any(|s| {
                let d = [p[0] - s[0], p[1] - s[1], p[2] - s[2]];
                d[0] * d[0] + d[1] * d[1] + d[2] * d[2] < 1e-6
            }) {
                continue;
            }
            seen.push(p);
            let d = [p[0] - from.position.x, p[1] - from.position.y, p[2] - from.position.z];
            let dist = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            c.limbs[c.count] = CorpseLimb { body: b as u16, hang: dist.min(LIMB_LONGEST) };
            c.count += 1;
            c.whole += self.mass(proxy, b as u32);
        }
        cvr_host::log!("[corpse] proxy {}: {} bodies, {} more places, {:.0} kg\n", proxy, c.bodies, c.count, c.whole);
        self.c[i] = c;
        i
    }
}
/// VRBodySurface(proxy, body, point) -> the point of the body's own shapes nearest `point` (the game's world), w how far
/// (0 inside); w -1 when the body has no actor or no shape that is measured.
#[no_mangle]
pub unsafe extern "C" fn VRBodySurface(_this: *mut c_void, frame: *mut Frame, out: *mut Vector4, _unused: i64) {
    let (mut proxy, mut body, mut point) = (0i32, 0i32, Vector4::default());
    params!(frame, proxy, body, point);
    let Some(out) = out.as_mut() else { return };
    *out = v4(0.0, 0.0, 0.0, -1.0);
    if proxy <= 0 {
        return;
    }
    let Some((actor, _, origin)) = crate::vrik::debug_view::rigid_dynamic_of(proxy as u32, body as u32) else { return };
    if crate::vrik::debug_view::body_gone(proxy as u32, body as u32) {
        return;
    }
    if let Some((c, d)) = crate::vrik::physhands::hold::body_surface(actor, &origin, &[point.x, point.y, point.z]) {
        *out = v4(c[0], c[1], c[2], d);
    }
}

static SEAT_TURN: Mutex<[[f32; 4]; 2]> = Mutex::new([[0.0, 0.0, 0.0, 1.0]; 2]);
/// VRGraspCapsule(side, style, center, axis, halfLength, radius, palmGap) -> the part's seat in the hand's frame; w 1
/// seated this call, 0 held as it was, -1 refused (another grasp has the hand, or no palm model yet) or let go. The grasp
/// the fingers take on a capsule given in the hand bone's frame (its axis a unit vector there), as grasp_person gives a
/// living person's part: the script's carry of a body lying dead asks for it each frame a hand holds a part of it, and
/// moves the place it took to the seat. Style 0 lets go.
#[no_mangle]
pub unsafe extern "C" fn VRGraspCapsule(_this: *mut c_void, frame: *mut Frame, out: *mut Vector4, _unused: i64) {
    let (mut side, mut style, mut center, mut axis) = (0i32, 0i32, Vector4::default(), Vector4::default());
    let (mut half, mut radius, mut palm_gap) = (0.0f32, 0.0f32, -1.0f32);
    params!(frame, side, style, center, axis, half, radius, palm_gap);
    let Some(out) = out.as_mut() else { return };
    *out = v4(0.0, 0.0, 0.0, -1.0);
    if !(0..=1).contains(&side) {
        return;
    }
    let sd = side as usize;
    if style <= 0 {
        crate::vrik::grasp::grasp_person(sd, 0, None, None, 0.0, 0.0);
        return;
    }
    // the capsule's axis as its local z: the shortest turn from z to the axis given
    let mut a = [axis.x, axis.y, axis.z];
    let n = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    if n < 1e-4 {
        return;
    }
    for v in a.iter_mut() {
        *v /= n;
    }
    let q = if a[2] > 0.9999 {
        [0.0, 0.0, 0.0, 1.0]
    } else if a[2] < -0.9999 {
        // a half turn about x
        [1.0, 0.0, 0.0, 0.0]
    } else {
        // (z x a, 1 + z.a)
        let q = [-a[1], a[0], 0.0, 1.0 + a[2]];
        let m = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
        [q[0] / m, q[1] / m, q[2] / m, q[3] / m]
    };
    let p = [center.x, center.y, center.z];
    let Some((seat_p, seat_q, seated)) = crate::vrik::grasp::grasp_person_gap(sd, style, Some(&p), Some(&q), half.max(0.0), radius.max(0.005), palm_gap) else { return };
    let seat_p = if seated { seat_p } else { [0.0; 3] };
    *out = v4(seat_p[0], seat_p[1], seat_p[2], if seated { 1.0 } else { 0.0 });
    // the turn the seat gives the part, in the hand's frame (VRGraspSeatTurn): seat * capsule^-1
    let mut t = [0.0, 0.0, 0.0, 1.0];
    if seated {
        let qi = [-q[0], -q[1], -q[2], q[3]];
        let a2 = seat_q;
        t = [
            a2[3] * qi[0] + a2[0] * qi[3] + a2[1] * qi[2] - a2[2] * qi[1],
            a2[3] * qi[1] - a2[0] * qi[2] + a2[1] * qi[3] + a2[2] * qi[0],
            a2[3] * qi[2] + a2[0] * qi[1] - a2[1] * qi[0] + a2[2] * qi[3],
            a2[3] * qi[3] - a2[0] * qi[0] - a2[1] * qi[1] - a2[2] * qi[2],
        ];
        let m = (t[0] * t[0] + t[1] * t[1] + t[2] * t[2] + t[3] * t[3]).sqrt();
        if m > 1e-6 && m.is_finite() {
            for v in t.iter_mut() {
                *v /= m;
            }
        } else {
            t = [0.0, 0.0, 0.0, 1.0];
        }
    }
    SEAT_TURN.lock().unwrap_or_else(|e| e.into_inner())[sd] = t;
}
/// VRGraspSeatTurn(side) -> Quaternion: the turn the last VRGraspCapsule that seated the part gave it, in the hand
/// bone's frame: the caller turns the part by it. Identity when that call did not seat it.
#[no_mangle]
pub unsafe extern "C" fn VRGraspSeatTurn(_this: *mut c_void, frame: *mut Frame, out: *mut Quaternion, _unused: i64) {
    let mut side = 0i32;
    params!(frame, side);
    let Some(out) = out.as_mut() else { return };
    *out = q4([0.0, 0.0, 0.0, 1.0]);
    if side != 0 && side != 1 {
        return;
    }
    *out = q4(SEAT_TURN.lock().unwrap_or_else(|e| e.into_inner())[side as usize]);
}

/// THE PART'S OWN BODY, found once for each part taken (a look walks every scene: 0.3 to 2 ms, and the script asks every
/// frame): per hand the body taken, the part held, its scene, and what they were asked as.
#[derive(Clone, Copy)]
struct Known {
    known: usize,
    part: usize,
    scene: usize,
    proxy: u32,
    body: u32,
    part_body: u32,
    grip: [f32; 3],
}
static KNOWN: Mutex<[Known; 2]> = Mutex::new([Known { known: 0, part: 0, scene: 0, proxy: 0, body: 0, part_body: 0, grip: [0.0; 3] }; 2]);
/// VRHandHoldBody(side, proxy, body, pos, rot, grip) -> Bool: the body held by the hand at a point of it, as a pistol is
/// (physhands' hold_body): pos xyz, rot its pose in the hand bone's frame, pos w the grip's torque (N m); grip xyz the
/// point held in the body's frame. A proxy of 0 or less lets go. The body the script took is the one nearest the palm when
/// the hand closed, and that is not always the bone's under it (a palm on a shin had a neighbour 25 cm off it): the body
/// whose own shapes are nearest the point held is held instead, the script's pose carried onto it. False when the body
/// has no actor of the PhysX this plugin reads, or none in the hands' scene.
#[no_mangle]
pub unsafe extern "C" fn VRHandHoldBody(_this: *mut c_void, frame: *mut Frame, out: *mut bool, _unused: i64) {
    let (mut side, mut proxy, mut body) = (0i32, 0i32, 0i32);
    let (mut pos, mut rot, mut grip) = (Vector4::default(), Quaternion::default(), Vector4::default());
    params!(frame, side, proxy, body, pos, rot, grip);
    let mut ok = false;
    if side == 0 || side == 1 {
        let sd = side as usize;
        let mut all = KNOWN.lock().unwrap_or_else(|e| e.into_inner());
        let k = &mut all[sd];
        if proxy <= 0 {
            k.known = 0;
            k.part = 0;
            ok = crate::vrik::physhands::hold::hold_body(sd, core::ptr::null_mut(), core::ptr::null_mut(), 0, 0, None, None, None, core::ptr::null_mut(), 0);
        } else {
            let (p, b) = (proxy as u32, body as u32);
            if k.known == 0 || k.proxy != p || k.body != b || k.grip != [grip.x, grip.y, grip.z] || crate::vrik::debug_view::body_gone(p, b) || crate::vrik::debug_view::body_gone(p, k.part_body) {
                let found = crate::vrik::debug_view::rigid_dynamic_of(p, b);
                let (actor, scene, origin) = found.map_or((core::ptr::null_mut(), core::ptr::null_mut(), [0.0; 3]), |f| f);
                k.known = actor as usize;
                k.part = actor as usize;
                k.scene = scene as usize;
                k.part_body = b;
                k.proxy = p;
                k.body = b;
                k.grip = [grip.x, grip.y, grip.z];
                if !actor.is_null() {
                    crate::vrik::debug_view::note_actor(p, b, actor as usize);
                    if let Some(world) = crate::vrik::physhands::hold::actor_point(actor, &origin, &k.grip) {
                        if let Some(n) = crate::vrik::debug_view::pick::nearest_body_of(p, &world, 0.25) {
                            if !n.actor.is_null() && n.body != b && n.scene == scene {
                                cvr_host::log!("[corpse] proxy {}: the point held is {:.3} m off body {}'s shapes, body {}'s own; that one held\n", p, n.distance, n.body, b);
                                k.part = n.actor as usize;
                                k.part_body = n.body;
                                crate::vrik::debug_view::note_actor(p, n.body, n.actor as usize);
                            }
                        }
                    }
                }
            }
            if k.known != 0 && k.part != 0 {
                let pp = [pos.x, pos.y, pos.z, pos.w];
                let q = [rot.i, rot.j, rot.k, rot.r];
                let g = [grip.x, grip.y, grip.z];
                ok = crate::vrik::physhands::hold::hold_body(sd, k.part as *mut c_void, k.scene as *mut c_void, p, k.part_body, Some(&pp), Some(&q), Some(&g), k.known as *mut c_void, b);
            }
        }
    }
    put(out, ok);
}

/// VRCorpseHold(proxy, the limb in a hand, the limb in the other or -1, how high the first is, dt) -> the whole body's
/// mass. Measured anew each time a limb is taken (held changed). The other as -2: the hand has an arm, a leg or the head,
/// and the body hangs from it by its own weight -- nothing is held up; -(otherBody + 3) the same mode, the other hand's
/// held part left out. It is towed along the floor instead: a limb farther along the floor from the one in the hand than
/// it lay from it (plus TOW_SLACK) is drawn toward it along the floor.
#[no_mangle]
pub unsafe extern "C" fn VRCorpseHold(_this: *mut c_void, frame: *mut Frame, out: *mut f32, _unused: i64) {
    let (mut proxy, mut held, mut other, mut z, mut dt) = (0i32, 0i32, -1i32, 0.0f32, 0.016f32);
    params!(frame, proxy, held, other, z, dt);
    put(out, 0.0);
    let p = proxy as u32;
    let mut st = CORPSE_STATE.lock().unwrap_or_else(|e| e.into_inner());
    let i = match st.find(p, held) {
        Some(i) => i,
        None => st.measure(p, held),
    };
    let c = st.c[i];
    put(out, c.whole);
    let Some(h) = st.transform(p, held as u32) else { return };
    // The script pose can belong to its old reference body. Both towing and lifting are anchored to the actor actually
    // held by the native driver.
    let _ = z;
    let z = h.position.z;
    if other <= -2 {
        let other_held: i64 = if other < -2 { -i64::from(other) - 3 } else { -1 };
        let (hx, hy) = (h.position.x, h.position.y);
        for limb in &c.limbs[..c.count] {
            if i32::from(limb.body) == held || i64::from(limb.body) == other_held {
                continue;
            }
            let Some(t) = st.transform(p, u32::from(limb.body)) else { continue };
            let (dx, dy) = (hx - t.position.x, hy - t.position.y);
            let along = (dx * dx + dy * dy).sqrt();
            let past = along - limb.hang - TOW_SLACK;
            if past <= 0.0 || along < 1e-3 {
                continue;
            }
            let Some(v) = st.velocity(p, u32::from(limb.body)) else { continue };
            let (ux, uy) = (dx / along, dy / along);
            let a = if past * TOW_PULL < TOW_MOST { past * TOW_PULL } else { TOW_MOST };
            let toward = v.x * ux + v.y * uy;
            if toward >= TOW_SPEED {
                continue;
            }
            let add = (a * dt).min(TOW_SPEED - toward);
            st.set_velocity(p, u32::from(limb.body), v4(v.x + ux * add, v.y + uy * add, v.z, 0.0));
        }
        return;
    }
    for limb in &c.limbs[..c.count] {
        if i32::from(limb.body) == held || i32::from(limb.body) == other {
            continue;
        }
        let Some(t) = st.transform(p, u32::from(limb.body)) else { continue };
        let low = z - limb.hang - t.position.z;
        if low <= 0.0 {
            continue;
        }
        let Some(v) = st.velocity(p, u32::from(limb.body)) else { continue };
        let pull = low * LIMB_PULL;
        let up = if pull < LIMB_LIFT { pull } else { LIMB_LIFT };
        let vz = v.z + 9.81 * dt + up * dt;
        st.set_velocity(p, u32::from(limb.body), v4(v.x * LIMB_CALM, v.y * LIMB_CALM, if vz < LIMB_RISE { vz } else { LIMB_RISE }, 0.0));
    }
}
/// VRCorpseSpeed(proxy) -> the fastest of its limbs, m/s; below 0 when it is no ragdoll any more.
#[no_mangle]
pub unsafe extern "C" fn VRCorpseSpeed(_this: *mut c_void, frame: *mut Frame, out: *mut f32, _unused: i64) {
    let mut proxy = 0i32;
    params!(frame, proxy);
    put(out, -1.0);
    let p = proxy as u32;
    if crate::vrik::debug_view::pick::proxy_kind(p) != 12 {
        return;
    }
    let mut st = CORPSE_STATE.lock().unwrap_or_else(|e| e.into_inner());
    let i = match st.find(p, -1) {
        Some(i) => i,
        None => st.measure(p, 0),
    };
    let c = st.c[i];
    let mut most = 0.0f32;
    let mut checked = 0u64;
    let mut sample = |st: &mut Corpses, body: u32| {
        if body as usize >= CORPSE_LIMBS || (checked & (1u64 << body)) != 0 {
            return;
        }
        checked |= 1u64 << body;
        let Some(v) = st.velocity(p, body) else { return };
        most = most.max(v.x * v.x + v.y * v.y + v.z * v.z);
    };
    // The measured anchor was omitted from limbs; it can still be the fastest moving part after release and must
    // prevent premature ragdoll freezing.
    sample(&mut st, c.held_when_measured as u32);
    for limb in &c.limbs[..c.count] {
        sample(&mut st, u32::from(limb.body));
    }
    // Other actual anchors may have coincided with a different shape when the first cache entry was measured. Include
    // them even after the hands release.
    let anchors: Vec<i32> = st.c.iter().filter(|a| a.proxy == p && a.held_when_measured >= 0).map(|a| a.held_when_measured).collect();
    for a in anchors {
        sample(&mut st, a as u32);
    }
    put(out, most.sqrt());
}
/// VRCorpseFreeze(proxy, frozen): every body of it kinematic where it lies, or back in the simulation.
#[no_mangle]
pub unsafe extern "C" fn VRCorpseFreeze(_this: *mut c_void, frame: *mut Frame, out: *mut bool, _unused: i64) {
    let (mut proxy, mut frozen) = (0i32, true);
    params!(frame, proxy, frozen);
    put(out, false);
    let p = proxy as u32;
    let mut st = CORPSE_STATE.lock().unwrap_or_else(|e| e.into_inner());
    let Some(i) = st.find(p, -1) else { return };
    if body_func("SetIsKinematic").is_none() {
        return;
    }
    let bodies = st.c[i].bodies;
    for b in 0..bodies {
        let body = st.scratch.at(p, b as u32);
        if body.is_null() {
            return;
        }
        let mut v = frozen;
        body_call_set(body, "SetIsKinematic", c"Bool", &mut v);
    }
    put(out, true);
}
/// VRBodyInfo(proxy, body) -> Vector4 as VRTraceInfo: its kind, ids and mass. All zero if it is no thing that moves.
#[no_mangle]
pub unsafe extern "C" fn VRBodyInfo(_this: *mut c_void, frame: *mut Frame, out: *mut Vector4, _unused: i64) {
    let (mut proxy, mut body) = (0i32, 0i32);
    params!(frame, proxy, body);
    let Some(out) = out.as_mut() else { return };
    *out = ZERO4;
    let kind = crate::vrik::debug_view::pick::proxy_kind(proxy as u32);
    if !trace_moves(kind) {
        return;
    }
    let b = scratch(&INFO_SCRATCH).at(proxy as u32, body as u32);
    if b.is_null() {
        return;
    }
    let mass = mass_of(b, proxy as u32, body as u32);
    *out = v4(kind as f32, proxy as f32, body as f32, mass);
}
/// VRBodySolverMass(proxy, body) -> Float: diagnostic only, the current solver mass, past mass_of's original-mass
/// normalization and cache. Negative: the actor disappeared or refused.
#[no_mangle]
pub unsafe extern "C" fn VRBodySolverMass(_this: *mut c_void, frame: *mut Frame, out: *mut f32, _unused: i64) {
    let (mut proxy, mut body) = (0i32, -1i32);
    params!(frame, proxy, body);
    put(out, -1.0);
    if proxy <= 0 || body < 0 || body >= crate::vrik::debug_view::pick::proxy_bodies(proxy as u32) || crate::vrik::debug_view::body_gone(proxy as u32, body as u32) {
        return;
    }
    if let Some((actor, _, _)) = crate::vrik::debug_view::rigid_dynamic_of(proxy as u32, body as u32) {
        put(out, crate::vrik::physhands::hold::body_solver_mass(actor, proxy as u32, body as u32));
    }
}
/// VRBodyOf(proxy, body) -> the body, for the VRBody* natives.
#[no_mangle]
pub unsafe extern "C" fn VRBodyOf(_this: *mut c_void, frame: *mut Frame, out: *mut c_void, _unused: i64) {
    let (mut proxy, mut body) = (0i32, 0i32);
    params!(frame, proxy, body);
    put_handle(out, new_body(proxy as u32, body as u32));
}
/// VRBodyGetRot(body) -> Quaternion: the body's orientation, beside VRBodyGetPos.
#[no_mangle]
pub unsafe extern "C" fn VRBodyGetRot(_this: *mut c_void, frame: *mut Frame, out: *mut Quaternion, _unused: i64) {
    let mut body = Handle::null();
    params!(frame, body);
    let Some(out) = out.as_mut() else { return };
    *out = q4([0.0, 0.0, 0.0, 1.0]);
    let mut t = Transform::default();
    if body_call_get(body.instance as *mut c_void, "GetTransform", c"Transform", as_bytes(&mut t)) {
        *out = t.orientation;
    }
}
/// VRBodySetQueryable(body, queryable) -> Bool: queries off hides a body from the player's movement, and from rays; it
/// still collides. A body with a PhysX actor of its own is switched at the PhysX step on that actor while it lives
/// (heldbody's query_wanted); any other through the game's native, queued for the body's proxy.
#[no_mangle]
pub unsafe extern "C" fn VRBodySetQueryable(_this: *mut c_void, frame: *mut Frame, out: *mut bool, _unused: i64) {
    let (mut body, mut queryable) = (Handle::null(), true);
    params!(frame, body, queryable);
    put(out, false);
    let instance = body.instance as *mut c_void;
    if instance.is_null() || body_broken(instance) {
        return;
    }
    let (proxy, part) = ids(instance);
    if crate::vrik::physhands::heldbody::query_wanted(proxy, part, queryable) != 0 {
        put(out, true);
        return;
    }
    let mut v = queryable;
    put(out, body_call_set(instance, "SetIsQueryable", c"Bool", &mut v));
}
