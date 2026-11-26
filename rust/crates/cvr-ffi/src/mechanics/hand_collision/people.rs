//! PEOPLE (main's NPC_BODY): their posed anatomy published to the native PhysX hand bodies. Listed four times a second
//! and placed every frame.
//!
//! FROM THEIR BONES. An NPC's slot component binds slots to real bones (Item_Attachment_Slot: Head -> Head, Chest ->
//! Spine3, LegLeft -> LeftLeg, and so on), and those answer with the live pose, so a capsule between two of them sits,
//! crouches and swings its arms with the person. The radii are the ragdoll's own (man_base.rig, ragdollDesc), wider on
//! the trunk and legs: clothes and flesh stand further off a bone than its ragdoll does. One person's slots are read per
//! frame, in turn (the nearest every frame); without that component, three capsules from the feet up.
use super::{entity_hash, len, qrot, slot_transform, world_fwd, world_pos, State, Q, V3};
use red4ext_abi::cname::CName;
use red4ext_abi::handle::Handle;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

const NPC_RANGE: f32 = 1.6;
pub(super) const NPC_MAX: usize = 4;
const NPC_EVERY: f64 = 0.25;
/// legs, body, head: radius, from, to (m above the feet)
const NPC_PARTS: [[f32; 3]; 3] = [[0.12, 0.10, 0.85], [0.15, 0.90, 1.42], [0.11, 1.56, 1.66]];
/// m: a joint stays put until it has moved this far (a breath), then follows at that distance.
const NPC_SWAY: f32 = 0.006;
pub(super) const SLOTS: [&str; 14] =
    ["Head", "Chest", "Hips", "LeftArm", "RightArm", "LeftForeArm", "RightForeArm", "LeftHand", "RightHand", "LegLeft", "LegRight", "LeftFoot", "RightFoot", "BroadPhase"];
/// joint, joint (1-based slots), radius, and how far the capsule's end sits above the joint
pub(super) const BONES: [(usize, usize, f32); 13] = [
    (3, 2, 0.145),
    (2, 1, 0.060),
    (1, 1, 0.095),
    (2, 4, 0.070),
    (2, 5, 0.070),
    (4, 6, 0.060),
    (5, 7, 0.060),
    (6, 8, 0.050),
    (7, 9, 0.050),
    (3, 10, 0.105),
    (3, 11, 0.105),
    (10, 12, 0.080),
    (11, 13, 0.080),
];
/// The most capsule places a person has (npcPart).
const MOST_PARTS: usize = 40;

/// Where a person's hair is (VRHairPoints), read once: their chains in the head's frame, or none (tried again later).
#[derive(Default)]
pub(super) struct HairChains {
    pub n: usize,
    pub l: Vec<V3>,
    /// each particle's parent, 1-based (0 none)
    pub p: Vec<usize>,
    pub w: Vec<V3>,
    pub s: Vec<f32>,
}
pub(super) struct NpcRec {
    pub e: Handle,
    pub uid: i32,
    /// how many slots answered at the last read
    pub n: usize,
    pub kind: Option<String>,
    comp: Handle,
    pub no_comp: bool,
    /// each slot's place, world space, kept back by NPC_SWAY
    pub at: [Option<V3>; 14],
    /// each slot's turn (kept from the last read that had it)
    pub q: [Option<Q>; 14],
    /// where the person stood and faced as the slots were read
    pub e_at: Option<V3>,
    pub e_fwd: Option<[f32; 2]>,
    /// a hand holds a part of this person
    pub held: bool,
    /// by capsule place (1-based): the part of the body each is pulled by, the grasp the fingers take, the limb's segment
    pub kinds: [Option<i32>; MOST_PARTS],
    pub grips: [Option<i32>; MOST_PARTS],
    pub segments: [Option<i32>; MOST_PARTS],
    pub hair_pts: Option<Option<HairChains>>,
    pub hair_retry: f32,
    pub hair_strands: Option<String>,
    pub hair_force: Option<V3>,
}
pub(super) type NpcRef = Rc<RefCell<NpcRec>>;
impl NpcRec {
    pub fn comp(&self) -> &Handle {
        &self.comp
    }
}

/// One person's slots carried to this frame (world space): where each slot is now and its three axes.
#[derive(Default)]
pub(super) struct Carried {
    pub w: [Option<V3>; 14],
    pub x: [Option<V3>; 14],
    pub y: [Option<V3>; 14],
    pub z: [Option<V3>; 14],
}
pub(super) struct People {
    pub list: Vec<Handle>,
    at_time: f64,
    clock: f64,
    pub recs: HashMap<u64, NpcRef>,
    /// the person behind each place of the list
    pub at: [Option<NpcRef>; NPC_MAX],
    turn: usize,
    seen: bool,
    next_uid: i32,
    odd: bool,
    /// how many were placed this frame, where each stands and which way each faces (world)
    pub n: usize,
    pub nw: [Option<V3>; NPC_MAX],
    pub nf: [Option<[f32; 2]>; NPC_MAX],
    /// someone a hand holds: read every frame
    pub held_rec: [Option<NpcRef>; 2],
    pub carried: Carried,
    /// what went to the physics hands last frame, kept while the collision view is on (it draws them)
    pub drawn: Vec<[f32; 8]>,
    pub drawn_on: bool,
    // npcSend's own: the person and the place counted, the part and the grasp of what is sent next
    uid: i32,
    part: usize,
    kind: i32,
    grip: i32,
    sent: usize,
}
impl People {
    pub fn new() -> People {
        People {
            list: Vec::new(),
            at_time: -1.0,
            clock: 0.0,
            recs: HashMap::new(),
            at: Default::default(),
            turn: 0,
            seen: false,
            next_uid: 1,
            odd: false,
            n: 0,
            nw: [None; NPC_MAX],
            nf: [None; NPC_MAX],
            held_rec: [None, None],
            carried: Carried::default(),
            drawn: Vec::new(),
            drawn_on: false,
            uid: 0,
            part: 0,
            kind: 0,
            grip: 0,
            sent: 0,
        }
    }
    /// npcFind: the person by their id.
    pub fn find(&self, uid: i32) -> Option<NpcRef> {
        self.recs.values().find(|r| r.borrow().uid == uid).cloned()
    }
}

/// Mirror actors occupy the player's location or are holographic reflections, not touchable people: publishing their
/// capsules as NPCs lets the player's hands push their own visual double.
fn alive(e: &Handle) -> bool {
    if !super::is_a(e, "NPCPuppet") || !super::super::menu_rt::sb("ScriptedPuppet", "IsAlive", &[super::A::H(e)]) {
        return false;
    }
    let record = super::m::<u64>(e, "GetRecordID", &[]).unwrap_or(0) & 0xFF_FFFF_FFFF;
    let mirror = |n: &str| red4ext_abi::tweakdbid::from_name(n) & 0xFF_FFFF_FFFF;
    record != mirror("Character.q110_tpp_male_v") && record != mirror("Character.q110_tpp_female_v") && record != mirror("Character.johnny_mirror")
}
fn refresh(s: &mut State) {
    let pl = super::player();
    let mut list = Vec::new();
    if !pl.is_null() {
        for e in super::targets_near(&pl, NPC_RANGE) {
            if alive(&e) && list.len() < NPC_MAX {
                list.push(e);
            }
        }
    }
    // what is known of each person is kept by who they are, since the list is rebuilt four times a second
    let b = &mut s.people;
    let mut recs = HashMap::new();
    for (i, e) in list.iter().enumerate() {
        let id = entity_hash(e).unwrap_or(0);
        let rec = match b.recs.get(&id) {
            Some(r) => r.clone(),
            None => {
                let uid = b.next_uid;
                b.next_uid = b.next_uid % 500000 + 1;
                Rc::new(RefCell::new(NpcRec {
                    e: e.clone(),
                    uid,
                    n: 0,
                    kind: None,
                    comp: Handle::null(),
                    no_comp: false,
                    at: [None; 14],
                    q: [None; 14],
                    e_at: None,
                    e_fwd: None,
                    held: false,
                    kinds: [None; MOST_PARTS],
                    grips: [None; MOST_PARTS],
                    segments: [None; MOST_PARTS],
                    hair_pts: None,
                    hair_retry: 0.0,
                    hair_strands: None,
                    hair_force: None,
                }))
            }
        };
        rec.borrow_mut().e = e.clone();
        b.at[i] = Some(rec.clone());
        recs.insert(id, rec);
    }
    for i in list.len()..NPC_MAX {
        b.at[i] = None;
    }
    b.recs = recs;
    b.list = list;
}
/// The slots of one person, world space.
fn read_bones(s: &mut State, rec: &NpcRef) {
    let mut r = rec.borrow_mut();
    // where the person stood and faced as the slots were read: the slots are carried by the person's step and turn
    // from here until they are read again
    r.e_at = world_pos(&r.e);
    r.e_fwd = world_fwd(&r.e).map(|f| [f[0], f[1]]);
    if r.kind.is_none() {
        r.kind = Some(super::m::<u64>(&r.e, "GetBodyType", &[]).map(super::name_text).unwrap_or_default());
    }
    if r.comp.is_null() {
        if r.no_comp {
            return;
        }
        for c in crate::engine_objects::entity_components(&r.e) {
            if super::component_name(&c) == "Item_Attachment_Slot" {
                r.comp = c;
                break;
            }
        }
        if r.comp.is_null() {
            r.no_comp = true;
            return;
        }
    }
    let mut n = 0;
    // (none kept back for someone a hand holds: what is held is pulled, and the hand on it follows it exactly)
    let sway = if r.held { 0.0 } else { NPC_SWAY };
    for i in 0..SLOTS.len() {
        match slot_transform(&r.comp, CName::hashed(SLOTS[i])) {
            Some((wp, q)) => {
                n += 1;
                r.q[i] = Some(q);
                // A joint stays put until it has moved more than the sway of someone standing still, and then follows
                // at that distance: a hand resting on a head was carried about by every breath otherwise.
                r.at[i] = Some(match r.at[i] {
                    Some(o) => {
                        let d = super::sub(&wp, &o);
                        let dl = len(&d);
                        if dl > sway {
                            super::add(&o, &super::mul(&d, 1.0 - sway / dl))
                        } else {
                            o
                        }
                    }
                    None => wp,
                });
            }
            None => r.at[i] = None,
        }
    }
    r.n = n;
    if !s.people.seen && n > 0 {
        s.people.seen = true;
        cvr_host::log!("[HandColl] npc bones: {} of {} slots answer\n", n, SLOTS.len());
    }
}
/// npcPlace: who is near, re-listed four times a second; where they stand this frame. EVERY FRAME the person nearest the
/// player is read, and every other frame one of the others in turn as well: the one the hands are on moves with its own
/// bones, not a frame behind them.
pub(super) fn place(s: &mut State, dt: f32) {
    s.people.clock += f64::from(dt);
    if s.people.clock - s.people.at_time >= NPC_EVERY {
        s.people.at_time = s.people.clock;
        refresh(s);
    }
    s.people.n = 0;
    if s.people.list.is_empty() {
        return;
    }
    let Some(f) = s.frame() else { return };
    s.people.odd = !s.people.odd;
    let list = s.people.list.clone();
    let mut pick = None;
    let mut best = 1e9f32;
    for (i, e) in list.iter().enumerate() {
        if let Some(p) = world_pos(e) {
            let d = (p[0] - f.b[0]) * (p[0] - f.b[0]) + (p[1] - f.b[1]) * (p[1] - f.b[1]);
            if d < best {
                best = d;
                pick = Some(i);
            }
        }
    }
    if let Some(rec) = pick.and_then(|i| s.people.at[i].clone()) {
        read_bones(s, &rec);
    }
    if s.people.odd && list.len() > 1 {
        s.people.turn = s.people.turn % list.len() + 1;
        let t = s.people.turn - 1;
        if Some(t) != pick {
            if let Some(rec) = s.people.at[t].clone() {
                read_bones(s, &rec);
            }
        }
    }
    let pick = pick.unwrap_or(0);
    // someone a hand holds: every frame, whoever else is read
    for side in 0..2 {
        if let Some(h) = s.people.held_rec[side].clone() {
            let same = s.people.at[pick].as_ref().is_some_and(|a| Rc::ptr_eq(a, &h));
            if !same {
                read_bones(s, &h);
            }
        }
    }
    let b = &mut s.people;
    for (i, e) in list.iter().enumerate() {
        if let Some(p) = world_pos(e) {
            let n = b.n;
            b.nw[n] = Some(p);
            b.nf[n] = world_fwd(e).map(|f| [f[0], f[1]]);
            b.at[n] = b.at[i].clone();
            b.n += 1;
        }
    }
}
/// npcCarry: one person's slots carried to this frame (world space) -- where each slot is now and its three axes, from
/// where it was read moved by the person's step and turn since (the slots are read a frame in two to four, the person
/// walks every frame).
pub(super) fn carry_slots(c: &mut Carried, rec: &NpcRec, nw: &V3, nf: Option<[f32; 2]>) {
    let e = rec.e_at.unwrap_or(*nw);
    let (mut cs, mut sn) = (1.0f32, 0.0f32);
    if let (Some(ef), Some(nf)) = (rec.e_fwd, nf) {
        if ef[0] * ef[0] + ef[1] * ef[1] > 0.01 && nf[0] * nf[0] + nf[1] * nf[1] > 0.01 {
            let t = nf[1].atan2(nf[0]) - ef[1].atan2(ef[0]);
            cs = t.cos();
            sn = t.sin();
        }
    }
    let turn = |v: V3| [cs * v[0] - sn * v[1], sn * v[0] + cs * v[1], v[2]];
    for i in 0..SLOTS.len() {
        match rec.at[i] {
            Some(x) => {
                let (dx, dy) = (x[0] - e[0], x[1] - e[1]);
                c.w[i] = Some([nw[0] + cs * dx - sn * dy, nw[1] + sn * dx + cs * dy, nw[2] + x[2] - e[2]]);
                match rec.q[i] {
                    Some(q) => {
                        c.x[i] = Some(turn(qrot(&q, &[1.0, 0.0, 0.0])));
                        c.y[i] = Some(turn(qrot(&q, &[0.0, 1.0, 0.0])));
                        c.z[i] = Some(turn(qrot(&q, &[0.0, 0.0, 1.0])));
                    }
                    None => c.x[i] = None,
                }
            }
            None => c.w[i] = None,
        }
    }
}
/// npcPt: a point of the carried slots: t of the way from slot i to slot j (1-based), then u v w along slot i's own axes.
/// None when a slot did not answer.
pub(super) fn point(c: &Carried, i: usize, j: usize, t: f32, u: f32, v: f32, w: f32) -> Option<V3> {
    let (a, b) = (c.w[i - 1]?, c.w[j - 1]?);
    let mut p = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
    if u != 0.0 || v != 0.0 || w != 0.0 {
        let x = c.x[i - 1]?;
        let (y, z) = (c.y[i - 1].unwrap_or([0.0; 3]), c.z[i - 1].unwrap_or([0.0; 3]));
        for k in 0..3 {
            p[k] += x[k] * u + y[k] * v + z[k] * w;
        }
    }
    Some(p)
}
/// The capsules sent this frame: each with the id the physics hands keep a hold by -- the person's (uid) times 32 plus
/// the capsule's place among theirs, 0 for a part not to be taken hold of (the shins and the feet).
fn send(b: &mut People, a: V3, c: V3, r: f32, nogrip: bool) {
    let owner = if b.uid > 0 && !nogrip && (1..=32).contains(&b.part) { b.uid * 32 + b.part as i32 - 1 } else { 0 };
    if b.sent < 96 {
        // SAFETY: the physics hands' script-side staging.
        unsafe { cvr_rig::vrik::physhands::people::set_person(b.sent, &a, &c, r, owner) };
        b.sent += 1;
        if b.drawn_on {
            b.drawn.push([a[0], a[1], a[2], c[0], c[1], c[2], r, owner as f32]);
        }
    }
}
/// npcSend: a capsule between two points of the carried slots. The place is counted for every capsule tried, sent or
/// not: counted only for those sent, a slot that did not answer for a frame moved every capsule after it down one.
#[allow(clippy::too_many_arguments)]
fn send_pts(b: &mut People, rec: &mut NpcRec, i1: usize, j1: usize, t1: f32, u1: f32, v1: f32, w1: f32, i2: usize, j2: usize, t2: f32, u2: f32, v2: f32, w2: f32, r: f32) {
    b.part += 1;
    let part = b.part;
    if part < MOST_PARTS {
        rec.kinds[part] = Some(b.kind);
        rec.grips[part] = Some(b.grip);
        rec.segments[part] = match b.kind {
            0 | 1 => match i1 {
                4 | 5 => Some(0),
                6 | 7 => Some(1),
                8 | 9 => Some(2),
                _ => None,
            },
            2 | 3 => Some(0),
            _ => None,
        };
    }
    let Some(a) = point(&b.carried, i1, j1, t1, u1, v1, w1) else { return };
    let Some(c) = point(&b.carried, i2, j2, t2, u2, v2, w2) else { return };
    send(b, a, c, r, false);
}
/// npcFoot: from the heel the way the foot points, level (the heel's Y forward on the left, its -Y on the right; the
/// person's own when the foot stands on its toes). The heel's place.
fn foot(b: &mut People, heel: usize, right: bool, nf: Option<[f32; 2]>, r: f32) -> Option<V3> {
    b.part += 1;
    let x = b.carried.w[heel - 1]?;
    let (mut fx, mut fy) = b.carried.y[heel - 1].map_or((0.0, 0.0), |y| (y[0], y[1]));
    if right {
        fx = -fx;
        fy = -fy;
    }
    let mut l = (fx * fx + fy * fy).sqrt();
    if l < 0.3 {
        let nf = nf?;
        fx = nf[0];
        fy = nf[1];
        l = (fx * fx + fy * fy).sqrt();
        if l < 0.1 {
            return None;
        }
    }
    fx /= l;
    fy /= l;
    send(b, [x[0], x[1], x[2] + 0.045], [x[0] + fx * 0.17, x[1] + fy * 0.17, x[2] + 0.035], r, true);
    Some(x)
}
/// npcBody: THE PEOPLE FOR THE PHYSICS HANDS -- each person's capsules, world space, handed over every frame. The hands
/// stop on them and the fingers lie on them, and nobody is pushed. Each shape is put by the bones it lies on, turned with
/// them (the rig's frames: the spine's and the head's X up their bone, Y to the back, Z to the right; an arm's and a
/// leg's X down to the next joint on the left and up from it on the right). A woman's are slighter, a big man's and a
/// fat one's wider; a woman's chest stands out further.
///
/// kind: the part of the body each capsule is pulled by when a physics hand holds it (npc_grip: 0 1 the arms, 2 3 the
/// legs, 4 the head, 5 the trunk); grip: the grasp the fingers take on it (1 round it like a can, 2 so with the fingers
/// wide, 3 gently, 4 the palm on it, 6 the hips by their side, 7 a thigh).
fn body(b: &mut People, rec: &NpcRef, nw: &V3, nf: Option<[f32; 2]>) {
    let mut r = rec.borrow_mut();
    carry_slots(&mut b.carried, &r, nw, nf);
    let kind = r.kind.clone().unwrap_or_default();
    let woman = kind.contains("Woman");
    let (mut ts, mut ls, mut hs) = (1.0f32, 1.0f32, 1.0f32);
    if woman {
        (ts, ls, hs) = (0.92, 0.88, 0.95);
    } else if kind.contains("Fat") {
        (ts, ls) = (1.3, 1.1);
    } else if kind.contains("Big") || kind.contains("Large") {
        (ts, ls, hs) = (1.15, 1.15, 1.05);
    }
    let r = &mut *r;
    // 1 Head, 2 Chest, 3 Hips, 4 5 the arms, 6 7 the forearms, 8 9 the hands, 10 11 the knees, 12 13 the heels, 14 the spine
    b.kind = 4;
    b.grip = 4;
    send_pts(b, r, 1, 1, 0.0, 0.09, 0.005, 0.0, 1, 1, 0.0, 0.09, -0.045, 0.0, 0.078 * hs); // skull
    send_pts(b, r, 1, 1, 0.0, 0.05, -0.07, 0.0, 1, 1, 0.0, -0.035, -0.075, 0.0, 0.050 * hs); // face, jaw
    b.grip = 2;
    send_pts(b, r, 2, 2, 0.0, 0.15, 0.01, 0.0, 1, 1, 0.0, 0.02, -0.01, 0.0, 0.052 * ls); // neck
    // the trunk: as wide as a man's hips and chest (34-36 cm) and as deep (22-24 cm), the spine 5-6 cm under the back;
    // the shoulders from the top of the chest out to the arm's joint
    b.kind = 5;
    for side in [-1.0f32, 1.0] {
        let arm = if side < 0.0 { 4 } else { 5 };
        b.grip = 6;
        send_pts(b, r, 3, 3, 0.0, -0.04, -0.02, side * 0.075 * ts, 14, 14, 0.0, 0.0, -0.05, side * 0.065 * ts, 0.10 * ts);
        b.grip = 4;
        send_pts(b, r, 14, 14, 0.0, 0.0, -0.05, side * 0.065 * ts, 2, 2, 0.0, 0.0, -0.055, side * 0.075 * ts, 0.10 * ts);
        send_pts(b, r, 2, 2, 0.0, 0.0, -0.055, side * 0.075 * ts, 2, 2, 0.0, 0.11, -0.045, side * 0.08 * ts, 0.095 * ts);
        b.grip = 1;
        send_pts(b, r, 2, 2, 0.0, 0.13, -0.03, side * 0.11 * ts, arm, arm, 0.0, 0.0, 0.0, 0.0, 0.055 * ls);
    }
    if woman {
        b.grip = 4;
        send_pts(b, r, 2, 2, 0.0, 0.02, -0.13, -0.06, 2, 2, 0.0, 0.02, -0.13, 0.06, 0.06); // breasts
    }
    // the arms: the shoulder to the elbow, the elbow to the wrist, the hand on from the wrist
    b.grip = 1;
    b.kind = 0;
    send_pts(b, r, 4, 4, 0.0, 0.0, 0.0, 0.0, 6, 6, 0.0, 0.0, 0.0, 0.0, 0.048 * ls);
    b.kind = 1;
    send_pts(b, r, 5, 5, 0.0, 0.0, 0.0, 0.0, 7, 7, 0.0, 0.0, 0.0, 0.0, 0.048 * ls);
    b.kind = 0;
    send_pts(b, r, 6, 6, 0.0, 0.0, 0.0, 0.0, 8, 8, 0.0, 0.0, 0.0, 0.0, 0.04 * ls);
    b.kind = 1;
    send_pts(b, r, 7, 7, 0.0, 0.0, 0.0, 0.0, 9, 9, 0.0, 0.0, 0.0, 0.0, 0.04 * ls);
    b.grip = 3;
    b.kind = 0;
    send_pts(b, r, 8, 8, 0.0, 0.02, 0.0, 0.0, 8, 8, 0.0, 0.11, 0.0, 0.0, 0.032 * ls);
    b.kind = 1;
    send_pts(b, r, 9, 9, 0.0, -0.02, 0.0, 0.0, 9, 9, 0.0, -0.11, 0.0, 0.0, 0.032 * ls);
    // the legs: the hip joint (beside the hips bone, a little under it) to the knee, the knee to the ankle, the foot
    for side in [-1.0f32, 1.0] {
        let (knee, heel) = if side < 0.0 { (10, 12) } else { (11, 13) };
        b.kind = if side < 0.0 { 2 } else { 3 };
        b.grip = 7;
        send_pts(b, r, 3, 3, 0.0, -0.03, 0.0, side * 0.09 * ts, knee, knee, 0.0, 0.0, 0.0, 0.0, 0.072 * ls);
        let h = foot(b, heel, side > 0.0, nf, 0.045 * ls);
        let k = point(&b.carried, knee, knee, 0.0, 0.0, 0.0, 0.0);
        b.part += 1;
        if let (Some(h), Some(k)) = (h, k) {
            send(b, k, [h[0], h[1], h[2] + 0.075], 0.05 * ls, true);
        }
    }
}
/// npcPublish: every person placed this frame handed to the physics hands (people far from the hands too: they decide
/// who they meet).
pub(super) fn publish(s: &mut State) {
    let b = &mut s.people;
    b.drawn.clear();
    b.sent = 0;
    for i in 0..b.n {
        let rec = b.at[i].clone();
        b.uid = rec.as_ref().map_or(0, |r| r.borrow().uid);
        b.part = 0;
        let Some(nw) = b.nw[i] else { continue };
        let nf = b.nf[i];
        match rec {
            Some(r) if r.borrow().n >= 8 => body(b, &r, &nw, nf),
            _ => {
                for (n, part) in NPC_PARTS.iter().enumerate() {
                    b.part = n + 1;
                    send(b, [nw[0], nw[1], nw[2] + part[1]], [nw[0], nw[1], nw[2] + part[2]], part[0], false);
                }
            }
        }
    }
    // SAFETY: the physics hands' script-side staging.
    unsafe { cvr_rig::vrik::physhands::people::set_people(b.sent as i32) };
}
