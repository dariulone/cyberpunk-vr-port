//! mods/redscript/CyberpunkVRPort_Basketball, the physics half (the input bridge is mechanics.rs's `basketball`): the
//! ball (vrbasketball\vr_basketball.ent, one entPhysicalMeshComponent authored to a real ball's 0.624 kg) spawned
//! through the plugin's spawn system, steered to the hand while held, and bounced as a basketball.
//!
//! The engine's materials give it e ~= 0.24 (rubber.physmat's 0.2 through PhysX's contact-pair combine), so the normal
//! component is topped up once per contact against an ENERGY target: on the frame the impulse lands the ball has
//! already slowed and risen, so it is the speed that still reaches the apex e(v_in) * v_in would have given --
//!     budget = (e * v_in)^2 / 2 - g_n * s,   J = m * (sqrt(2 * budget) - v_n)
//! and e(v) = clamp(0.88 - 0.0125 v, 0.72, 0.86), the NBA drop rule's line. The tangential half is PhysX's.
//!
//! A held ball stays a dynamic body, steered by a bounded impulse toward the palm (the hand's velocity fed forward plus
//! a proportional pull, the limits growing with the miss beyond 0.20 m), its orientation by angular velocity; released,
//! its velocity already is the throw. A rising grip edge within 1.6 m of the palm grabs a free ball.
//!
//! The PlayerPuppet fields of the script are this state, kept per player. The console's methods are the plugin's
//! natives VRBallToggle, VRBallSetCorrection, VRBallDropTest, VRBallTestThrow, VRBallReport and VRBallStatus.
use super::belt::{conj, dist, f3, f4, m, native, qmul, qrot, v4, Q, V3};
use super::lobby_spawn::{self as sp, add, dot, mul, sub};
use crate::script_call::Value;
use cvr_host::log;
use red4ext_abi::handle::Handle;
use std::sync::Mutex;

const MASS: f64 = 0.624;
const CONTACT_SPEED: f64 = 1.0;
const GRAB_REACH: f64 = 1.60;
const SIM_MASK1: u64 = 376836;
const SIM_MASK2: u64 = 5070;
const MAX_PALM_STEP: f64 = 0.35;
const LOST_DISTANCE: f64 = 80.0;
const CARRY_GAIN: f64 = 25.0;
const CARRY_MAX_SPEED: f64 = 9.0;
const CARRY_SPIN: f64 = 12.0;
const CARRY_ACCEL: f64 = 80.0;
const CARRY_RECOVER: f64 = 0.20;
const TEMPLATE: &str = "vrbasketball\\vr_basketball.ent";
const TAG: &str = "VRBasketball";
/// Wrist -> a held ball's centre in each hand's own frame (right, left): flesh centre + palm normal * (half-thickness
/// 0.020 + ball radius 0.119) + palm axis * 0.085, measured off the rig and the fists mesh. The frames are mirrored.
const PALM_OFF: [V3; 2] = [[-0.109, -0.022, 0.116], [0.084, 0.022, -0.139]];

/// The synthetic hand of VRBallTestThrow: phase 1 holding and moving, 2 released (watching the flight), 3 done.
#[derive(Default)]
struct Fake {
    on: bool,
    phase: i32,
    pos: V3,
    vel: V3,
    t: f64,
    hold: f64,
    rel_pos: V3,
    flight: f64,
    range: f64,
    apex: f64,
    land_range: f64,
    land_t: f64,
    rel_contacts: i32,
    gap: f64,
}

#[derive(Default)]
struct Ball {
    /// The player the state belongs to (the script's fields were the player's).
    owner: usize,
    /// m_vrBallHas with m_vrBallId.
    id: Option<u64>,
    placed: bool,
    prev_pos: V3,
    prev_vel: V3,
    has_prev: bool,
    correct: bool,
    last_impact: f64,
    e_in: f64,
    e_out: f64,
    e_engine: f64,
    contacts: i32,
    test_drop: f64,
    test_rebound: f64,
    test_e: f64,
    /// The deferred correction: the clean closing speed, the contact normal, where it happened.
    pend: Option<(f64, V3, V3)>,
    /// 0 free, 1 the right hand, 2 the left.
    held: i32,
    palm: [Option<V3>; 2],
    palm_vel: [V3; 2],
    palm_has_prev: bool,
    grip_prev: [bool; 2],
    fake: Fake,
    last_throw: f64,
    test_phase: i32,
    test_start_z: f64,
    test_hit_z: f64,
    test_apex_z: f64,
    /// The entity the body was found on, and its physical body.
    body_of: usize,
    body: Handle,
}
// SAFETY: the handles are made, used and dropped on the game's main thread (the mechanics' tick, the natives).
unsafe impl Send for Ball {}

static BALL: Mutex<Option<Ball>> = Mutex::new(None);

/// The player's pose for a tick.
struct Pl {
    h: Handle,
    pos: V3,
    rot: Q,
}
impl Pl {
    fn now() -> Option<Pl> {
        let h = super::belt::player();
        let pos = f3(m::<[f32; 4]>(&h, "GetWorldPosition", &[]).ok()?);
        let rot = f4(m::<[f32; 4]>(&h, "GetWorldOrientation", &[]).ok()?);
        Some(Pl { h, pos, rot })
    }
}

fn with<T>(pl: &Pl, f: impl FnOnce(&mut Ball) -> T) -> T {
    let mut g = BALL.lock().unwrap_or_else(|p| p.into_inner());
    let owner = pl.h.instance as usize;
    if g.as_ref().is_none_or(|b| b.owner != owner) {
        *g = Some(Ball { owner, ..Ball::default() });
    }
    f(g.as_mut().expect("the ball's state"))
}

fn len(v: V3) -> f64 {
    dot(v, v).sqrt()
}
fn world_pos(e: &Handle) -> Option<V3> {
    m::<[f32; 4]>(e, "GetWorldPosition", &[]).ok().map(f3)
}
fn body_vel(b: &Handle) -> Option<V3> {
    native::<[f32; 4]>("VRBodyGetVel", &[Value::Handle(b)]).filter(|v| v[3] >= 0.5).map(f3)
}
fn impulse(b: &Handle, j: V3) {
    let v = v4(j, 0.0);
    super::lobby_ink::call(b, "AddLinearImpulse", &[Value::Ptr(v.as_ptr().cast()), Value::Bool(true)]);
}
/// e(v): rubber loses more the harder it is hit; anchored on the NBA drop (1.80 m -> 1.20..1.40 m).
fn restitution(vn: f64) -> f64 {
    (0.88 - 0.0125 * vn).clamp(0.72, 0.86)
}

impl Ball {
    fn entity(&self) -> Handle {
        match self.id {
            Some(id) => sp::native_handle("VRSpawnedEntity", &[Value::U64(id)]),
            None => Handle::null(),
        }
    }
    /// VRBallBody: the first entPhysicalMeshComponent's physical body (found again when the entity changes).
    fn body(&mut self, ent: &Handle) -> Handle {
        if ent.is_null() {
            return Handle::null();
        }
        let at = ent.instance as usize;
        if self.body_of == at && !self.body.is_null() {
            return self.body.clone();
        }
        self.body_of = at;
        self.body = Handle::null();
        let wanted = red4ext_abi::cname::CName::hashed("entPhysicalMeshComponent");
        // SAFETY: the live entity's components, read on the main thread; each a live component for the calls.
        for &[instance, _] in unsafe { crate::anim_lookup::components(at) } {
            // SAFETY: as above.
            if instance == 0 || !unsafe { red4ext_abi::rtti::object_class(instance as *mut core::ffi::c_void) }.is_some_and(|c| c.is_a(wanted)) {
                continue;
            }
            // SAFETY: as above.
            self.body = unsafe { crate::script_call::call_handle(instance as *mut core::ffi::c_void, "CreatePhysicalBodyInterface", &[Value::Int32(0)]) }
                .unwrap_or_else(Handle::null);
            break;
        }
        self.body.clone()
    }

    /// VRBallSpawnAt: `dist` m ahead of the player, `up` m above their feet.
    fn spawn_at(&mut self, pl: &Pl, ahead: f64, up: f64) {
        if self.id.is_some() {
            self.despawn();
        }
        let fwd = m::<[f32; 4]>(&pl.h, "GetWorldForward", &[]).ok().map(f3).unwrap_or([0.0, 1.0, 0.0]);
        let pos = [pl.pos[0] + fwd[0] * ahead, pl.pos[1] + fwd[1] * ahead, pl.pos[2] + up];
        self.id = Some(sp::spawn_tagged(TEMPLATE, "default", pos, [0.0, 0.0, 0.0, 1.0], &[TAG]).unwrap_or(0));
        self.placed = false;
        self.has_prev = false;
        self.test_phase = 0;
        self.held = 0;
        self.palm_has_prev = false;
        self.pend = None;
        self.body_of = 0;
        self.body = Handle::null();
        log!("[VRBall] spawned at ({:.3}, {:.3}, {:.3}) (from Rust)\n", pos[0], pos[1], pos[2]);
    }
    /// VRBallSpawn: the normal way to get a ball, the bounce correction on (the engine alone gives a beanbag).
    fn spawn(&mut self, pl: &Pl) {
        self.correct = true;
        self.spawn_at(pl, 1.5, 1.2);
    }
    fn despawn(&mut self) {
        let Some(id) = self.id else { return };
        let ent = self.entity();
        let b = self.body(&ent);
        if !b.is_null() {
            native::<bool>("VRBodySetSimMasks", &[Value::Handle(&b), Value::U64(SIM_MASK1), Value::U64(SIM_MASK2)]);
        }
        native::<bool>("VRSpawnDelete", &[Value::U64(id)]);
        self.id = None;
        self.has_prev = false;
        self.test_phase = 0;
        self.body_of = 0;
        self.body = Handle::null();
        log!("[VRBall] despawned (from Rust)\n");
    }

    /// VRBallHandWorldRot: playerOrientation * the solved hand's model rotation.
    fn hand_rot(pl: &Pl, right: bool) -> Q {
        let r = native::<[f32; 4]>("VRPalmModelRot", &[Value::Int32(i32::from(right))]).map(f4).unwrap_or([0.0, 0.0, 0.0, 1.0]);
        qmul(pl.rot, r)
    }
    /// VRBallPalmWorld: the palm (where a held ball's centre goes) in the world; the synthetic hand owns the right.
    fn palm_world(&self, pl: &Pl, right: bool) -> Option<V3> {
        if self.fake.on && right {
            return Some(self.fake.pos);
        }
        let pm = native::<[f32; 4]>("VRPalmModelPos", &[Value::Int32(i32::from(right))])?;
        if pm[3] < 0.5 {
            return None;
        }
        let w = qrot(pl.rot, f3(pm));
        let off = qrot(Self::hand_rot(pl, right), PALM_OFF[usize::from(!right)]);
        Some(add(add(pl.pos, w), off))
    }
    fn update_palms(&mut self, pl: &Pl, dt: f64) {
        let now = [self.palm_world(pl, true), self.palm_world(pl, false)];
        if self.palm_has_prev && dt > 0.0 {
            for i in 0..2 {
                if let (Some(n), Some(o)) = (now[i], self.palm[i]) {
                    // a step larger than a hand can make in a frame is a tracking jump, not motion
                    self.palm_vel[i] = if dist(n, o) > MAX_PALM_STEP { [0.0; 3] } else { mul(sub(n, o), 1.0 / dt) };
                }
            }
        }
        self.palm = now;
        self.palm_has_prev = true;
    }

    /// VRBallFakeTick: the synthetic hand advanced, the flight recorded.
    fn fake_tick(&mut self, dt: f64, ball: V3) {
        let f = &mut self.fake;
        if !f.on {
            return;
        }
        if f.phase == 1 {
            f.pos = add(f.pos, mul(f.vel, dt));
            f.gap = f.gap.max(dist(ball, f.pos));
            f.t += dt;
            if f.t >= f.hold {
                f.phase = 2;
                f.rel_pos = ball;
                f.rel_contacts = self.contacts;
                f.land_range = -1.0;
            }
            return;
        }
        if f.phase == 2 {
            f.flight += dt;
            let (dx, dy) = (ball[0] - f.rel_pos[0], ball[1] - f.rel_pos[1]);
            f.range = (dx * dx + dy * dy).sqrt();
            f.apex = f.apex.max(ball[2] - f.rel_pos[2]);
            // the distance is the flight's only up to the first bounce
            if f.land_range < 0.0 && self.contacts > f.rel_contacts {
                f.land_range = f.range;
                f.land_t = f.flight;
            }
            if f.flight > 4.0 {
                f.phase = 3;
            }
        }
    }

    /// VRBallGripTick: grab, carry, release.
    fn grip_tick(&mut self, pl: &Pl, grip: [bool; 2], ent: &Handle, ball: V3, dt: f64) {
        let body = self.body(ent);
        if body.is_null() {
            return;
        }
        if self.held != 0 {
            let hi = usize::from(self.held != 1);
            if let (true, Some(palm)) = (grip[hi], self.palm[hi]) {
                super::lobby_ink::call(&body, "SetIsKinematic", &[Value::Bool(false)]);
                // invisible to scene queries while carried (the character controller sweeps), still simulated
                super::lobby_ink::call(&body, "SetIsQueryable", &[Value::Bool(false)]);
                let cur = world_pos(ent).unwrap_or(ball);
                let to = sub(palm, cur);
                let mut want = add(add(self.palm_vel[hi], mul(to, CARRY_GAIN)), [0.0, 0.0, 9.81 * dt]);
                // both limits grow with the miss beyond a hand's reach: nothing is held there to protect
                let miss = len(to);
                let far = if miss > CARRY_RECOVER { (miss / CARRY_RECOVER).min(12.0) } else { 1.0 };
                let (sp_, max_sp) = (len(want), CARRY_MAX_SPEED * far);
                if sp_ > max_sp {
                    want = mul(want, max_sp / sp_);
                }
                // the hand pulls with a bounded force; it does not dictate the velocity
                match body_vel(&body) {
                    Some(v) => {
                        let mut dv = sub(want, v);
                        let (dl, max_dv) = (len(dv), CARRY_ACCEL * far * dt);
                        if dl > max_dv && dl > 0.0001 {
                            dv = mul(dv, max_dv / dl);
                        }
                        impulse(&body, mul(dv, MASS));
                    }
                    None => {
                        let w = v4(want, 0.0);
                        native::<bool>("VRBodySetVel", &[Value::Handle(&body), Value::Ptr(w.as_ptr().cast())]);
                    }
                }
                // orientation by angular velocity: q_err = q_target * conj(q_current), the short way round
                if let Ok(qc) = m::<[f32; 4]>(ent, "GetWorldOrientation", &[]) {
                    let qe = qmul(Self::hand_rot(pl, self.held == 1), conj(f4(qc)));
                    let s = if qe[3] < 0.0 { -1.0 } else { 1.0 };
                    let k = 2.0 * CARRY_SPIN * s;
                    let w = v4([qe[0] * k, qe[1] * k, qe[2] * k], 0.0);
                    native::<bool>("VRBodySetAngVel", &[Value::Handle(&body), Value::Ptr(w.as_ptr().cast())]);
                }
                return;
            }
            // released: the ball's velocity already is the throw
            self.last_throw = body_vel(&body).map_or(0.0, len);
            self.held = 0;
            self.has_prev = false;
            self.pend = None;
            return;
        }
        // a free ball: a rising grip edge within reach grabs it, without bending down for one on the ground
        let d = |i: usize| self.palm[i].map_or(9999.0, |p| dist(p, ball));
        let (dr, dl) = (d(0), d(1));
        let r_grab = grip[0] && !self.grip_prev[0] && dr < GRAB_REACH;
        let l_grab = grip[1] && !self.grip_prev[1] && dl < GRAB_REACH;
        if r_grab || l_grab {
            self.held = if r_grab && dr <= dl {
                1
            } else if l_grab {
                2
            } else {
                1
            };
            self.pend = None;
        }
    }

    /// VRBallOnContact: the engine's own bounce recorded, the correction deferred to the next clean frame.
    fn on_contact(&mut self, v_in: f64, v_out: f64, n: V3, p: V3, dt: f64) {
        self.e_in = v_in;
        self.e_out = v_out;
        self.e_engine = v_out / v_in;
        self.contacts += 1;
        if self.test_phase == 1 {
            self.test_hit_z = p[2];
            self.test_apex_z = p[2];
            self.test_phase = 2;
            log!("[VRBall] contact: v_in={v_in:.3} m/s  v_out(engine)={v_out:.3} m/s  -> e_engine={:.3} (from Rust)\n", v_out / v_in);
        }
        if !self.correct {
            return;
        }
        // half a frame of gravity on top of the frame before's average
        self.pend = Some((v_in + 4.905 * dt, n, p));
    }
    /// VRBallTestWatch: the apex after the drop test's first hit.
    fn test_watch(&mut self, z: f64) {
        if self.test_phase != 2 {
            return;
        }
        // `>=`: the contact's own frame reads the hit's height again (the script's `>` settled the test right there, a
        // rebound of 0)
        if z >= self.test_apex_z {
            self.test_apex_z = z;
            return;
        }
        let h_in = self.test_start_z - self.test_hit_z;
        let h_out = self.test_apex_z - self.test_hit_z;
        if h_in > 0.05 {
            let e = (h_out / h_in).max(0.0).sqrt();
            (self.test_drop, self.test_rebound, self.test_e) = (h_in, h_out, e);
            log!("[VRBall] RESULT drop={h_in:.3} m  rebound={h_out:.3} m  e_effective={e:.3}  (NBA wants 0.82..0.88 from 1.8 m) (from Rust)\n");
        }
        self.test_phase = 0;
    }

    /// VRBallTick: once a frame while the player has a ball.
    fn tick(&mut self, pl: &Pl, dt: f64, grip_r: bool, grip_l: bool) {
        if self.id.is_none() || dt <= 0.0 {
            return;
        }
        let ent = self.entity();
        let Some(p) = world_pos(&ent) else { return };
        // a ball just entered reads the world origin until its transform lands: nothing counts before it is placed
        if !self.placed {
            if dist(p, pl.pos) > LOST_DISTANCE {
                return;
            }
            self.placed = true;
        }
        self.fake_tick(dt, p);
        let grip = if self.fake.on { [self.fake.phase == 1, false] } else { [grip_r, grip_l] };
        self.update_palms(pl, dt);
        // invisible to scene queries for its whole life: the character controller would climb it
        let qb = self.body(&ent);
        if !qb.is_null() {
            super::lobby_ink::call(&qb, "SetIsQueryable", &[Value::Bool(false)]);
        }
        // a lost ball (one fell past z = -7070) recalled
        if self.held == 0 && dist(p, pl.pos) > LOST_DISTANCE {
            self.spawn_at(pl, 1.5, 1.2);
            return;
        }
        if self.held != 0 {
            self.grip_tick(pl, grip, &ent, p, dt);
            self.grip_prev = grip;
            self.prev_pos = p;
            self.has_prev = false;
            return;
        }
        if !self.has_prev {
            self.prev_pos = p;
            self.prev_vel = [0.0; 3];
            self.has_prev = true;
            return;
        }
        let v = mul(sub(p, self.prev_pos), 1.0 / dt);
        let dv = sub(v, self.prev_vel);
        // the pending correction, now that the ball has separated and v is clean again
        if let Some((v_in, n, at)) = self.pend.take() {
            let s = dot(sub(p, at), n);
            let budget = (restitution(v_in) * v_in).powi(2) * 0.5 - 9.81 * n[2] * s;
            if budget > 0.0 {
                let need = (2.0 * budget).sqrt() - dot(v, n);
                let body = self.body(&ent);
                if need > 0.0 && !body.is_null() {
                    impulse(&body, mul(n, MASS * need));
                }
            }
        }
        // only a contact changes the velocity faster than gravity; the step is its impulse, its direction the normal
        let dl = len(dv);
        if dl > CONTACT_SPEED {
            let n = mul(dv, 1.0 / dl);
            let (v_in, v_out) = (-dot(self.prev_vel, n), dot(v, n));
            if v_in > 0.0 && v_out >= 0.0 {
                self.last_impact = v_in;
                self.on_contact(v_in, v_out, n, p, dt);
            }
        }
        self.grip_tick(pl, grip, &ent, p, dt);
        self.grip_prev = grip;
        self.prev_pos = p;
        self.prev_vel = v;
        self.test_watch(p[2]);
    }

    fn report(&self) -> String {
        format!(
            "contacts={} vIn={:.3} vOut={:.3} e_engine={:.3} | drop={:.3} rebound={:.3} e_test={:.3} | correction={} | held={} lastThrow={:.3}",
            self.contacts, self.e_in, self.e_out, self.e_engine, self.test_drop, self.test_rebound, self.test_e, self.correct, self.held, self.last_throw
        )
    }
    fn throw_report(&mut self) -> String {
        let ent = self.entity();
        let b = self.body(&ent);
        let bv = body_vel(&b).unwrap_or([0.0; 3]);
        let f = &self.fake;
        format!(
            "bodyVel=({:.3}, {:.3}, {:.3}) phase={} landRange={:.3} landT={:.3} worstCarryGap={:.3} | flight={:.2}s range={:.3}m apexRise={:.3} lastThrow={:.3}",
            bv[0], bv[1], bv[2], f.phase, f.land_range, f.land_t, f.gap, f.flight, f.range, f.apex, self.last_throw
        )
    }
    fn status(&mut self) -> String {
        let ent = self.entity();
        let Some(p) = world_pos(&ent) else { return "no ball".into() };
        let b = self.body(&ent);
        let kin = b.is_null() || m::<bool>(&b, "IsKinematic", &[]).unwrap_or(true);
        let sim = !b.is_null() && m::<bool>(&b, "IsSimulated", &[]).unwrap_or(false);
        let v = self.prev_vel;
        format!(
            "pos=({:.3}, {:.3}, {:.3}) body={} kinematic={kin} simulated={sim} vel=({:.3}, {:.3}, {:.3})",
            p[0], p[1], p[2], !b.is_null(), v[0], v[1], v[2]
        )
    }
}

// ---------------------------------------------------------------- the bridge's calls
/// The player has a ball (VRBallHas).
pub(super) fn has() -> bool {
    let p = super::belt::player().instance as usize;
    BALL.lock().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|b| b.owner == p && b.id.is_some())
}
pub(super) fn last_impact() -> f64 {
    BALL.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map_or(0.0, |b| b.last_impact)
}
pub(super) fn tick(dt: f64, grip_r: bool, grip_l: bool) {
    if let Some(pl) = Pl::now() {
        with(&pl, |b| b.tick(&pl, dt, grip_r, grip_l));
    }
    self_test(dt);
}
/// A frame without a ball.
pub(super) fn idle(dt: f64) {
    self_test(dt);
}

// ---------------------------------------------------------------- the console's
pub(super) fn toggle() {
    if let Some(pl) = Pl::now() {
        with(&pl, |b| if b.id.is_some() { b.despawn() } else { b.spawn(&pl) });
    }
}
pub(super) fn set_correction(on: bool) {
    if let Some(pl) = Pl::now() {
        with(&pl, |b| b.correct = on);
        log!("[VRBall] bounce correction = {on} (from Rust)\n");
    }
}
/// VRBallDropTest: dropped from `height` m, the effective restitution reported from the rebound.
pub(super) fn drop_test(height: f64) {
    let Some(pl) = Pl::now() else { return };
    with(&pl, |b| {
        b.spawn_at(&pl, 1.5, height);
        b.test_phase = 1;
        b.test_start_z = pl.pos[2] + height;
        log!("[VRBall] drop test from {height:.2} m, correction={} (from Rust)\n", b.correct);
    });
}
/// VRBallTestThrow: a ball grabbed by a synthetic hand that moves at `v` (world axes) for `hold` s, then lets go.
pub(super) fn test_throw(v: V3, hold: f64) {
    let Some(pl) = Pl::now() else { return };
    let fwd = m::<[f32; 4]>(&pl.h, "GetWorldForward", &[]).ok().map(f3).unwrap_or([0.0, 1.0, 0.0]);
    with(&pl, |b| {
        b.despawn();
        b.fake = Fake { on: true, phase: 1, pos: [pl.pos[0] + fwd[0] * 0.4, pl.pos[1] + fwd[1] * 0.4, pl.pos[2] + 1.3], vel: v, hold, apex: -9999.0, ..Fake::default() };
        b.correct = true;
        // spawned clear of the player and steered in, as a grab from a distance
        b.spawn_at(&pl, 1.4, 1.3);
        b.held = 1;
    });
}
pub(super) fn report() -> String {
    Pl::now().map(|pl| with(&pl, |b| format!("{} || {}", b.report(), b.throw_report()))).unwrap_or_default()
}
pub(super) fn status() -> String {
    Pl::now().map(|pl| with(&pl, |b| b.status())).unwrap_or_default()
}

/// A development check (CVR_BALL_TEST, inherited by the game from the probe session): 45 s into the loaded save (a
/// player other than the first one seen -- the main menu's puppet -- with the lobby down), a 1.8 m drop test with the
/// correction on; at 51 s a throw by the synthetic hand (5 m/s along the player's forward, 3 m/s up, 0.4 s); at 59 s
/// the reports, then the ball despawned.
fn self_test(dt: f64) {
    thread_local! {
        static T: std::cell::Cell<(f64, u32)> = const { std::cell::Cell::new((0.0, 0)) };
        static FIRST: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }
    if std::env::var_os("CVR_BALL_TEST").is_none() {
        return;
    }
    let p = super::belt::player().instance as usize;
    if p == 0 {
        return;
    }
    if FIRST.with(|c| c.get()) == 0 {
        FIRST.with(|c| c.set(p));
    }
    if p == FIRST.with(|c| c.get()) || native::<bool>("VRLobbyActive", &[]) == Some(true) {
        return;
    }
    let (t, step) = T.with(|c| c.get());
    let t = t + dt;
    let mut next = step;
    match step {
        0 if t > 45.0 => {
            set_correction(true);
            drop_test(1.8);
            next = 1;
        }
        1 if t > 51.0 => {
            log!("[VRBall] self-test after the drop: {} | {} (from Rust)\n", report(), status());
            let fwd = Pl::now().and_then(|pl| m::<[f32; 4]>(&pl.h, "GetWorldForward", &[]).ok()).map(f3).unwrap_or([0.0, 1.0, 0.0]);
            test_throw([fwd[0] * 5.0, fwd[1] * 5.0, 3.0], 0.4);
            next = 2;
        }
        2 if t > 59.0 => {
            log!("[VRBall] self-test after the throw: {} | {} (from Rust)\n", report(), status());
            if let Some(pl) = Pl::now() {
                with(&pl, |b| {
                    b.fake = Fake::default();
                    b.despawn();
                });
            }
            log!("[VRBall] self-test done, has={} (from Rust)\n", has());
            next = 3;
        }
        _ => {}
    }
    T.with(|c| c.set((t, next)));
}
