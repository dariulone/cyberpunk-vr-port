//! Co-op (NightCoop): a street prop held in a VR hand, shown in the partner's hand.
//!
//! A carried prop (heldbody.rs) is a body of the world's instanced physics, moved on this machine only: NightCoop syncs
//! entities, not those bodies, and the same spot holds other bodies on the partner's machine (or none), so the body
//! itself cannot be named to them. What goes instead is what the partner has to see: the prop's mesh (its resource
//! path's hash) and where it is drawn in the world, 20 times a second per hand on the bridge's channel (the latest only),
//! and a reliable word when the hand lets go. The partner's game spawns a mesh entity of its own for each of our hands
//! (a one-mesh template of the port's, its mesh swapped for the prop's by the hash), eases it to each pose given and
//! takes it away when we let go, or when nothing came for a while (we left, the session ended).
//!
//! The words: version 1, kind (1 held, 2 let go), hand (0 left, 1 right), 0, the mesh hash (u64), the pose (7 f32:
//! x y z, quaternion x y z w); little-endian, 40 bytes; channel 1 the left hand, 2 the right.
use super::lobby_spawn as sp;
use cvr_host::log;
use cvr_rig::vrik::nightcoop;
use cvr_rig::vrik::physhands::heldbody;
use std::sync::Mutex;
use std::time::Instant;

const VERSION: u8 = 1;
const HELD: u8 = 1;
const LET_GO: u8 = 2;
const WORD: usize = 40;
const SEND_EVERY: f64 = 0.05;
/// s without a word: the mirror goes (a partner that left, a lost let-go word).
const STALE: f64 = 1.5;
const TAG: &str = "VRCoopProp";
/// A one-mesh template of the port's: the entity a partner's prop is drawn with (its mesh swapped by hash).
const CARRIER: &str = "vrport\\holster\\shell\\vrp_shell_harness_m.ent";
const CARRIER_MESH: &str = "shell";

struct Mirror {
    peer: u32,
    hand: u8,
    mesh: u64,
    id: Option<u64>,
    dressed: bool,
    from: [f32; 7],
    to: [f32; 7],
    got: Instant,
}
struct State {
    sent: [Option<Instant>; 2],
    holding: [bool; 2],
    mirrors: Vec<Mirror>,
    logged: bool,
}
static STATE: Mutex<State> = Mutex::new(State { sent: [None, None], holding: [false, false], mirrors: Vec::new(), logged: false });

fn word(kind: u8, hand: u8, mesh: u64, pose: &[f32; 7]) -> [u8; WORD] {
    let mut w = [0u8; WORD];
    w[0] = VERSION;
    w[1] = kind;
    w[2] = hand;
    w[4..12].copy_from_slice(&mesh.to_le_bytes());
    for (k, v) in pose.iter().enumerate() {
        w[12 + 4 * k..16 + 4 * k].copy_from_slice(&v.to_le_bytes());
    }
    w
}
fn read(w: &[u8]) -> Option<(u8, u8, u64, [f32; 7])> {
    if w.len() < WORD || w[0] != VERSION || w[2] > 1 {
        return None;
    }
    let mesh = u64::from_le_bytes(w[4..12].try_into().ok()?);
    let mut pose = [0f32; 7];
    for (k, v) in pose.iter_mut().enumerate() {
        *v = f32::from_le_bytes(w[12 + 4 * k..16 + 4 * k].try_into().ok()?);
    }
    pose.iter().all(|v| v.is_finite()).then_some((w[1], w[2], mesh, pose))
}

/// Our carried props out to the partners (the bridge sends nothing outside a session or without NightCoop).
fn send_ours(s: &mut State) {
    let carried = heldbody::carried_now();
    for hand in 0..2u8 {
        let c = carried.iter().find(|c| c.left == (hand == 0) && c.mesh != 0);
        let channel = u16::from(hand) + 1;
        match c {
            Some(c) => {
                if s.sent[hand as usize].is_some_and(|t| t.elapsed().as_secs_f64() < SEND_EVERY) {
                    continue;
                }
                let r = nightcoop::send(channel, &word(HELD, hand, c.mesh, &c.pose), nightcoop::SEND_LATEST_ONLY);
                if r.is_some() {
                    s.sent[hand as usize] = Some(Instant::now());
                    if !s.holding[hand as usize] {
                        log!("[coop-props] our {} hand holds a prop (mesh {:#018x}): sent to the partners ({r:?})\n", if hand == 0 { "left" } else { "right" }, c.mesh);
                    }
                    s.holding[hand as usize] = true;
                }
            }
            None if s.holding[hand as usize] => {
                nightcoop::send(channel, &word(LET_GO, hand, 0, &[0.0; 7]), nightcoop::SEND_RELIABLE);
                s.holding[hand as usize] = false;
                s.sent[hand as usize] = None;
            }
            None => {}
        }
    }
}

fn drop_mirror(m: &Mirror) {
    sp::delete(m.id);
}

/// The partners' words in: a mirror per (partner, hand) made, moved, taken away.
fn take_theirs(s: &mut State) {
    for (peer, channel, bytes) in nightcoop::take_messages() {
        if !(1..=2).contains(&channel) {
            continue;
        }
        let Some((kind, hand, mesh, pose)) = read(&bytes) else { continue };
        let at = s.mirrors.iter().position(|m| m.peer == peer && m.hand == hand);
        if kind == LET_GO {
            if let Some(i) = at {
                drop_mirror(&s.mirrors.remove(i));
            }
            continue;
        }
        if kind != HELD || mesh == 0 {
            continue;
        }
        match at {
            Some(i) if s.mirrors[i].mesh == mesh => {
                let m = &mut s.mirrors[i];
                m.from = current(m);
                m.to = pose;
                m.got = Instant::now();
            }
            other => {
                if let Some(i) = other {
                    drop_mirror(&s.mirrors.remove(i));
                }
                let p = [f64::from(pose[0]), f64::from(pose[1]), f64::from(pose[2])];
                let q = [f64::from(pose[3]), f64::from(pose[4]), f64::from(pose[5]), f64::from(pose[6])];
                let id = sp::spawn_tagged(CARRIER, "", p, q, &[TAG]);
                if !s.logged {
                    s.logged = true;
                    log!("[coop-props] a partner (peer {peer}) holds a prop (mesh {mesh:#018x}): its mirror spawned {}\n", id.is_some());
                }
                s.mirrors.push(Mirror { peer, hand, mesh, id, dressed: false, from: pose, to: pose, got: Instant::now() });
            }
        }
    }
}

/// Where a mirror is drawn now: eased from the previous pose to the latest over one send period.
fn current(m: &Mirror) -> [f32; 7] {
    let k = (m.got.elapsed().as_secs_f64() / SEND_EVERY).clamp(0.0, 1.0) as f32;
    let mut out = [0f32; 7];
    for i in 0..3 {
        out[i] = m.from[i] + (m.to[i] - m.from[i]) * k;
    }
    // the shorter way round, then normalised (nlerp)
    let dot: f32 = (3..7).map(|i| m.from[i] * m.to[i]).sum();
    let sign = if dot < 0.0 { -1.0 } else { 1.0 };
    for i in 3..7 {
        out[i] = m.from[i] + (m.to[i] * sign - m.from[i]) * k;
    }
    let n = (3..7).map(|i| out[i] * out[i]).sum::<f32>().sqrt();
    if n > 1e-6 {
        for v in &mut out[3..7] {
            *v /= n;
        }
    } else {
        out[3..7].copy_from_slice(&m.to[3..7]);
    }
    out
}

fn place(s: &mut State) {
    s.mirrors.retain(|m| {
        let keep = m.got.elapsed().as_secs_f64() < STALE;
        if !keep {
            drop_mirror(m);
        }
        keep
    });
    for m in &mut s.mirrors {
        let e = sp::find_entity(m.id);
        if e.is_null() {
            continue;
        }
        if !m.dressed {
            let c = sp::find_comp(&e, CARRIER_MESH);
            if c.is_null() {
                continue;
            }
            super::belt::toggle(&c, false);
            let swapped = crate::engine_objects::component_change_resource_hash(&c, m.mesh, false);
            super::belt::change_appearance(&c, "default", false);
            super::belt::toggle(&c, true);
            m.dressed = true;
            log!("[coop-props] the mirror wears the partner's prop mesh {:#018x}: {swapped}\n", m.mesh);
        }
        let p = current(m);
        sp::set_world(&e, [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])], [f64::from(p[3]), f64::from(p[4]), f64::from(p[5]), f64::from(p[6])]);
    }
}

/// Every frame with a player (the mechanics' update).
pub(super) fn tick() {
    if !cvr_rig::vrik::nightcoop::loaded() {
        return;
    }
    let mut s = STATE.lock().unwrap_or_else(|e| e.into_inner());
    send_ours(&mut s);
    take_theirs(&mut s);
    place(&mut s);
}

/// The player goes (a load, the main menu): every mirror taken away.
pub(super) fn forget() {
    let mut s = STATE.lock().unwrap_or_else(|e| e.into_inner());
    for m in s.mirrors.drain(..) {
        drop_mirror(&m);
    }
    s.holding = [false, false];
    s.sent = [None, None];
    sp::clear(TAG);
}
