//! BAKED GRASPS (main's GraspPose.cpp): grips/CyberpunkVR_MeshGrasps.bin, made by tools/grip_generator/pose_batch.cpp.
//! For each hand-sized cooked prop mesh (by its depot path's hash): the primitives the game takes it by (its bounding
//! box, or a round mesh's cylinder and its parts) and, for each hand, the grasps the fitter accepted: the production seat
//! on one of them moved and tilted to fit the drawn surface against the FPP hand's skin, kept as the MESH's pose against
//! the palm, and the fingers the production solver closed on the drawn surface (not on the primitive).
use super::*;
use cvr_body::cmath::acos;
use cvr_host::log;
use std::sync::OnceLock;

const MAGIC: u32 = 0x4752_5643;
const VERSION: u32 = 2;
const HEADER: usize = 20;
const MESH_ROW: usize = 20;
const TARGET_ROW: usize = 44;
const GRASP_ROW: usize = 40 + SLOTS * 6;
#[derive(Clone, Copy, Debug)]
struct MeshRow {
    hash: u64,
    first_grasp: u32,
    first_target: u32,
    grasps: u16,
    targets: u8,
    round_axis: i8,
}
#[derive(Clone, Copy, Debug)]
struct TargetRow {
    shape: u8,
    ends: u8,
    part: u8,
    half: V3,
    p: V3,
    q: Q,
}
#[derive(Clone, Copy, Debug)]
struct GraspRow {
    side: u8,
    kind: u8,
    target: u8,
    palm_d: f32,
    palm_t: f32,
    pos: V3,
    rel: Q,
    delta: [[i16; 3]; SLOTS],
}
#[derive(Default)]
struct Baked {
    meshes: Vec<MeshRow>,
    targets: Vec<TargetRow>,
    grasps: Vec<GraspRow>,
}
static BAKED: OnceLock<Baked> = OnceLock::new();

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}
impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let v = self.b.get(self.at..self.at + N)?.try_into().ok()?;
        self.at += N;
        Some(v)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take::<1>().map(|v| v[0])
    }
    fn u16(&mut self) -> Option<u16> {
        self.take().map(u16::from_le_bytes)
    }
    fn i16(&mut self) -> Option<i16> {
        self.take().map(i16::from_le_bytes)
    }
    fn u32(&mut self) -> Option<u32> {
        self.take().map(u32::from_le_bytes)
    }
    fn u64(&mut self) -> Option<u64> {
        self.take().map(u64::from_le_bytes)
    }
    fn f32(&mut self) -> Option<f32> {
        self.take().map(f32::from_le_bytes)
    }
    fn v3(&mut self) -> Option<V3> {
        Some([self.f32()?, self.f32()?, self.f32()?])
    }
    fn q4(&mut self) -> Option<Q> {
        Some([self.f32()?, self.f32()?, self.f32()?, self.f32()?])
    }
}
/// The file read and checked whole: its header (magic, version 2, sane counts), its rows, the meshes by hash and every
/// row's references inside the tables. None: the file is missing or not this version.
fn parse(bytes: &[u8]) -> Option<Baked> {
    let mut r = Reader { b: bytes, at: 0 };
    let (magic, version, meshes, targets, grasps) = (r.u32()?, r.u32()?, r.u32()?, r.u32()?, r.u32()?);
    if magic != MAGIC || version != VERSION || meshes >= 200_000 || targets >= 2_000_000 || grasps >= 4_000_000 {
        return None;
    }
    debug_assert_eq!(r.at, HEADER);
    let (meshes, targets, grasps) = (meshes as usize, targets as usize, grasps as usize);
    if bytes.len() < HEADER + meshes * MESH_ROW + targets * TARGET_ROW + grasps * GRASP_ROW {
        return None;
    }
    let mut out = Baked { meshes: Vec::with_capacity(meshes), targets: Vec::with_capacity(targets), grasps: Vec::with_capacity(grasps) };
    for _ in 0..meshes {
        out.meshes.push(MeshRow { hash: r.u64()?, first_grasp: r.u32()?, first_target: r.u32()?, grasps: r.u16()?, targets: r.u8()?, round_axis: r.u8()? as i8 });
    }
    for _ in 0..targets {
        let (shape, ends, part, _pad) = (r.u8()?, r.u8()?, r.u8()?, r.u8()?);
        out.targets.push(TargetRow { shape, ends, part, half: r.v3()?, p: r.v3()?, q: r.q4()? });
    }
    for _ in 0..grasps {
        let (side, kind, target, _pad) = (r.u8()?, r.u8()?, r.u8()?, r.u8()?);
        let (palm_d, palm_t, pos, rel) = (r.f32()?, r.f32()?, r.v3()?, r.q4()?);
        let mut delta = [[0i16; 3]; SLOTS];
        for d in delta.iter_mut() {
            *d = [r.i16()?, r.i16()?, r.i16()?];
        }
        out.grasps.push(GraspRow { side, kind, target, palm_d, palm_t, pos, rel, delta });
    }
    for (i, m) in out.meshes.iter().enumerate() {
        let ok = (i == 0 || out.meshes[i - 1].hash < m.hash)
            && m.first_grasp as usize + m.grasps as usize <= out.grasps.len()
            && m.first_target as usize + m.targets as usize <= out.targets.len()
            && m.round_axis < 3
            && (m.first_grasp..m.first_grasp + u32::from(m.grasps)).all(|g| out.grasps[g as usize].target < m.targets && out.grasps[g as usize].side < 2);
        if !ok {
            return None;
        }
    }
    Some(out)
}
/// GraspLoadBaked: loaded once (the count of meshes; 0 when the file is missing or not this version).
pub fn grasp_load_baked(path: &std::path::Path) -> i32 {
    BAKED.get_or_init(|| std::fs::read(path).ok().and_then(|b| parse(&b)).unwrap_or_default()).meshes.len() as i32
}
fn baked_row(hash: u64) -> Option<(&'static Baked, &'static MeshRow)> {
    let b = BAKED.get()?;
    let i = b.meshes.binary_search_by_key(&hash, |m| m.hash).ok()?;
    Some((b, &b.meshes[i]))
}
/// GraspBakedHas: whether a mesh has any baked grasp.
pub fn grasp_baked_has(mesh: u64) -> bool {
    mesh != 0 && baked_row(mesh).is_some()
}
/// How far the seat the game gave this grab may be from a baked one, its turn and its place against the palm (a round
/// mesh: its axis's way only, it may be turned about it).
const BAKED_MATCH_DEG: f32 = 25.0;
const BAKED_MATCH_M: f32 = 0.035;
/// The palm's axes as one rotation (hand frame): d, n made square to it, d x n -- the fitter's construction.
fn palm_basis(pm: &Palm) -> (Q, V3, V3, V3) {
    let mut d = pm.d;
    unit(&mut d);
    let along = dot(&pm.n, &d);
    let mut n = [0.0f32; 3];
    for k in 0..3 {
        n[k] = pm.n[k] - d[k] * along;
    }
    unit(&mut n);
    let t = cross(&d, &n);
    let m = [[d[0], n[0], t[0]], [d[1], n[1], t[1]], [d[2], n[2], t[2]]];
    let tr = m[0][0] + m[1][1] + m[2][2];
    let mut q = if tr > 0.0 {
        let s = sqrt(tr + 1.0) * 2.0;
        [(m[2][1] - m[1][2]) / s, (m[0][2] - m[2][0]) / s, (m[1][0] - m[0][1]) / s, 0.25 * s]
    } else if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
        let s = sqrt(1.0 + m[0][0] - m[1][1] - m[2][2]) * 2.0;
        [0.25 * s, (m[0][1] + m[1][0]) / s, (m[0][2] + m[2][0]) / s, (m[2][1] - m[1][2]) / s]
    } else if m[1][1] > m[2][2] {
        let s = sqrt(1.0 + m[1][1] - m[0][0] - m[2][2]) * 2.0;
        [(m[0][1] + m[1][0]) / s, 0.25 * s, (m[1][2] + m[2][1]) / s, (m[0][2] - m[2][0]) / s]
    } else {
        let s = sqrt(1.0 + m[2][2] - m[0][0] - m[1][1]) * 2.0;
        [(m[0][2] + m[2][0]) / s, (m[1][2] + m[2][1]) / s, 0.25 * s, (m[1][0] - m[0][1]) / s]
    };
    qnorm(&mut q);
    (q, d, n, t)
}
/// What grasp_take_baked took: the mesh's pose in the hand bone's frame, the grasp's kind, and how far the game's seat
/// was from it (degrees, metres).
#[derive(Clone, Copy, Debug)]
pub struct BakedTake {
    pub p: V3,
    pub q: Q,
    pub kind: i32,
    pub deg: f32,
    pub dist: f32,
}
/// GraspTakeBaked: a hand taking a mesh by the baked grasp nearest the seat the game gave it -- `mesh_p`, `mesh_q` the
/// MESH's pose in the hand bone's frame at that seat (a held body's frame is its mesh's). None: none near enough (25
/// degrees, 3.5 cm), keep the game's seat. Some: the baked mesh pose, and the baked fingers from the next pass.
pub fn grasp_take_baked(side: usize, mesh: u64, mesh_p: &V3, mesh_q: &Q) -> Option<BakedTake> {
    if side > 1 || mesh == 0 {
        return None;
    }
    let (baked, m) = baked_row(mesh)?;
    let pm = {
        let st = lock(side);
        if !st.palm {
            return None;
        }
        st.pm
    };
    let (basis, bd, bn, bt) = palm_basis(&pm);
    let basis_inv = conj(&basis);
    let mut q0 = *mesh_q;
    qnorm(&mut q0);
    let mut rel = qmul(&basis_inv, &q0);
    qnorm(&mut rel);
    let off = sub(mesh_p, &pm.skin);
    let at = [dot(&off, &bd), dot(&off, &bn), dot(&off, &bt)];
    let mut axis = [0.0f32; 3];
    let mut axis_now = [0.0f32; 3];
    if m.round_axis >= 0 {
        axis[m.round_axis as usize] = 1.0;
        axis_now = rot(&rel, &axis);
    }
    let (mut best, mut best_cost, mut best_deg, mut best_dist) = (None::<&GraspRow>, 1e30f32, 0.0f32, 0.0f32);
    let (mut nearest, mut nearest_cost, mut nearest_deg, mut nearest_dist) = (None::<&GraspRow>, 1e30f32, 0.0f32, 0.0f32);
    for g in &baked.grasps[m.first_grasp as usize..m.first_grasp as usize + m.grasps as usize] {
        if g.side as usize != side {
            continue;
        }
        let deg = if m.round_axis >= 0 {
            let b = rot(&g.rel, &axis);
            acos(clamp(dot(&b, &axis_now), -1.0, 1.0)) / K_DEG
        } else {
            let d = (rel[0] * g.rel[0] + rel[1] * g.rel[1] + rel[2] * g.rel[2] + rel[3] * g.rel[3]).abs();
            2.0 * acos(min(1.0, d)) / K_DEG
        };
        let sd = if g.palm_d > 1e-4 { pm.half_d / g.palm_d } else { 1.0 };
        let st = if g.palm_t > 1e-4 { pm.half_t / g.palm_t } else { 1.0 };
        let dp = [at[0] - g.pos[0] * sd, at[1] - g.pos[1], at[2] - g.pos[2] * st];
        let dist = len(&dp);
        let cost = deg / BAKED_MATCH_DEG + dist / BAKED_MATCH_M;
        if cost < nearest_cost {
            nearest_cost = cost;
            nearest = Some(g);
            nearest_deg = deg;
            nearest_dist = dist;
        }
        if deg > BAKED_MATCH_DEG || dist > BAKED_MATCH_M {
            continue;
        }
        if cost < best_cost {
            best_cost = cost;
            best = Some(g);
            best_deg = deg;
            best_dist = dist;
        }
    }
    let Some(best) = best else {
        log!(
            "[grasp] {} hand: mesh {:016x} at the game's seat (palm basis) pos {:.3} {:.3} {:.3} turn {:.3} {:.3} {:.3} {:.3}; nearest baked: {} target {} kind {}, {:.1} deg, {:.1} mm\n",
            if side != 0 { "right" } else { "left" },
            mesh,
            at[0],
            at[1],
            at[2],
            rel[0],
            rel[1],
            rel[2],
            rel[3],
            if nearest.is_some() { "" } else { "none" },
            nearest.map_or(-1, |g| i32::from(g.target)),
            nearest.map_or(-1, |g| i32::from(g.kind)),
            nearest_deg,
            nearest_dist * 1000.0
        );
        return None;
    };
    // the baked pose, for this palm: along it and across it as much bigger as the palm is, off it as baked
    let sd = if best.palm_d > 1e-4 { pm.half_d / best.palm_d } else { 1.0 };
    let st = if best.palm_t > 1e-4 { pm.half_t / best.palm_t } else { 1.0 };
    let mut mesh_in_hand = POSE0;
    for c in 0..3 {
        mesh_in_hand.p[c] = pm.skin[c] + bd[c] * best.pos[0] * sd + bn[c] * best.pos[1] + bt[c] * best.pos[2] * st;
    }
    mesh_in_hand.q = qmul(&basis, &best.rel);
    qnorm(&mut mesh_in_hand.q);
    let tg = baked.targets[m.first_target as usize + best.target as usize];
    if m.round_axis >= 0 {
        // A round mesh is the same all round its axis: it keeps the turn about its axis it was taken with (a mug does
        // not spin in the hand to the baked one's), only its axis goes to the baked axis -- round that axis's line where
        // the baked grasp has it (the primitive's centre is on it).
        let b = rot(&mesh_in_hand.q, &axis);
        let now = rot(&q0, &axis);
        let c = cross(&now, &b);
        let mut swing = [c[0], c[1], c[2], 1.0 + dot(&now, &b)];
        qnorm(&mut swing);
        let mut q = qmul(&swing, &q0);
        qnorm(&mut q);
        let c0 = rot(&mesh_in_hand.q, &tg.p);
        let c1 = rot(&q, &tg.p);
        for k in 0..3 {
            mesh_in_hand.p[k] += c0[k] - c1[k];
        }
        mesh_in_hand.q = q;
    }
    // its primitive, for the request (what the hand holds, in the slot's frame)
    let mut in_mesh = Pose { p: tg.p, q: tg.q };
    qnorm(&mut in_mesh.q);
    let prim = compose(&mesh_in_hand, &in_mesh);
    let mut prim_slot = Pose { p: to_frame(&pm.slot, &prim.p), q: qmul(&conj(&pm.slot.q), &prim.q) };
    qnorm(&mut prim_slot.q);
    let mut side_state = lock(side);
    side_state.req = REQUEST0;
    side_state.req.shape = i32::from(tg.shape);
    side_state.req.half = tg.half;
    side_state.req.obj = prim_slot;
    side_state.req.ends = i32::from(tg.ends);
    side_state.req.part = tg.part != 0;
    side_state.req.baked = true;
    side_state.req.baked_kind = i32::from(best.kind);
    for s in 0..SLOTS {
        let q = &mut side_state.req.baked_delta[s];
        for c in 0..3 {
            q[c] = f32::from(best.delta[s][c]) / 32767.0;
        }
        q[3] = sqrt(max(0.0, 1.0 - q[0] * q[0] - q[1] * q[1] - q[2] * q[2]));
        qnorm(q);
    }
    side_state.active = true;
    side_state.pin = true;
    side_state.rev = side_state.rev.wrapping_add(1);
    Some(BakedTake { p: mesh_in_hand.p, q: mesh_in_hand.q, kind: i32::from(best.kind), deg: best_deg, dist: best_dist })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_shipped_file_parses() {
        let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../mods/config/CyberpunkVR_MeshGrasps.bin")).expect("the shipped file");
        let b = parse(&bytes).expect("version 2");
        assert!(!b.meshes.is_empty());
        assert_eq!(bytes.len(), HEADER + b.meshes.len() * MESH_ROW + b.targets.len() * TARGET_ROW + b.grasps.len() * GRASP_ROW);
    }
}
