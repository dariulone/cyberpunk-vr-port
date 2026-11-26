//! THE DRAWN SHAPE OF A ROUND PROP (main's src/Natives/HeldBody.cpp: MeshProfile, ProfileOf, ProfilePoints). A bottle's
//! collider is a capsule round the whole of it (a can's, a cup's too): it has no neck, and it is thicker than the glass.
//! Where the neck is and how thin, and how thick the body really is, are the drawn mesh's: their radius profiles, made
//! from the game's cooked meshes by tools/grip_generator into src/Anim/GraspProfiles.inc, read here as it is -- one
//! source for both builds -- by the mesh's depot path's hash.

/// One mesh's profile: its axis (0 x, 1 y, 2 z), the profile's extent along it (lo, hi), the axis's place across it (cb,
/// cc: the other two coordinates, in axis order), and the radius of 16 equal slices from lo to hi.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshProfile {
    pub hash: u64,
    pub axis: usize,
    pub lo: f32,
    pub hi: f32,
    pub cb: f32,
    pub cc: f32,
    pub r: [f32; 16],
}

const PROFILES_TEXT: &str = include_str!("../../../../src/Anim/GraspProfiles.inc");

/// One line of the generator's table: `{0x...ull, axis, lo f, hi f, cb f, cc f, {r0 f, ... r15 f}}, // path`.
pub fn parse(line: &str) -> Option<MeshProfile> {
    let body = line.trim().strip_prefix('{')?;
    let body = &body[..body.find("}}")?];
    let mut it = body.split([',', '{']).map(str::trim).filter(|s| !s.is_empty());
    let hash = u64::from_str_radix(it.next()?.trim_start_matches("0x").trim_end_matches("ull"), 16).ok()?;
    let axis: usize = it.next()?.parse().ok()?;
    let mut f = || -> Option<f32> { it.next()?.trim_end_matches('f').parse().ok() };
    let (lo, hi, cb, cc) = (f()?, f()?, f()?, f()?);
    let mut r = [0.0f32; 16];
    for v in r.iter_mut() {
        *v = f()?;
    }
    (axis < 3).then_some(MeshProfile { hash, axis, lo, hi, cb, cc, r })
}

/// The whole table, by hash (read once).
pub fn table() -> &'static [MeshProfile] {
    static TABLE: std::sync::OnceLock<Vec<MeshProfile>> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let mut v: Vec<MeshProfile> = PROFILES_TEXT.lines().filter(|l| l.trim_start().starts_with('{')).filter_map(parse).collect();
        // The generated table is sorted and unique; kept so whatever it holds.
        v.sort_by_key(|p| p.hash);
        v.dedup_by_key(|p| p.hash);
        v
    })
}

/// ProfileOf: the profile of a mesh by its path's hash (0: none). The table covers thousands of props, so a lookup does
/// not scan it.
pub fn profile_of(hash: u64) -> Option<&'static MeshProfile> {
    if hash == 0 {
        return None;
    }
    let t = table();
    t.binary_search_by_key(&hash, |p| p.hash).ok().map(|i| &t[i])
}

pub const PROFILE_RING: usize = 8;
/// The most points profile_points writes: 16 rings of PROFILE_RING.
pub const PROFILE_POINTS: usize = 16 * PROFILE_RING;

fn qrot(q: &[f32; 4], v: &[f32; 3]) -> [f32; 3] {
    let t = [2.0 * (q[1] * v[2] - q[2] * v[1]), 2.0 * (q[2] * v[0] - q[0] * v[2]), 2.0 * (q[0] * v[1] - q[1] * v[0])];
    [v[0] + q[3] * t[0] + q[1] * t[2] - q[2] * t[1], v[1] + q[3] * t[1] + q[2] * t[0] - q[0] * t[2], v[2] + q[3] * t[2] + q[0] * t[1] - q[1] * t[0]]
}

/// What profile_points made: the points' count, the profile's middle along the cylinder's axis (`mz`), its half length,
/// and its widest radius.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rings {
    pub count: usize,
    pub mz: f32,
    pub half_length: f32,
    pub widest: f32,
}

/// ProfilePoints: a profile's rings (PROFILE_RING points round each slice's middle) in the cylinder's frame (`local`: its
/// pose in the body's, that is the mesh's: x y z, x y z w), each taken `mz` along the cylinder's axis toward the
/// profile's own middle. Count 0 when the mesh's axis is not the cylinder's or it is not as thick (another scale, another
/// thing). `out` holds 3 * PROFILE_POINTS floats.
pub fn profile_points(m: &MeshProfile, local: &[f32; 7], half: &[f32; 3], out: &mut [f32]) -> Rings {
    let inv = [-local[3], -local[4], -local[5], local[6]];
    let mut axis = [0.0f32; 3];
    axis[m.axis] = 1.0;
    let dir = qrot(&inv, &axis);
    if dir[2].abs() < 0.95 {
        return Rings::default();
    }
    let widest = m.r.iter().copied().fold(0.0f32, f32::max);
    let capsule = half[0].max(half[1]);
    if !(widest > 0.004) || widest > 1.4 * capsule || widest < 0.6 * capsule {
        return Rings { widest, ..Rings::default() };
    }
    let (b, c) = ((m.axis + 1) % 3, (m.axis + 2) % 3);
    let mut centre = [0.0f32; 3];
    centre[m.axis] = 0.5 * (m.lo + m.hi);
    centre[b] = m.cb;
    centre[c] = m.cc;
    let mid = qrot(&inv, &[centre[0] - local[0], centre[1] - local[1], centre[2] - local[2]]);
    if mid[0].hypot(mid[1]) > 0.01 {
        // its axis is not the cylinder's
        return Rings { widest, ..Rings::default() };
    }
    let mz = mid[2];
    let half_length = 0.5 * (m.hi - m.lo);
    let w = (m.hi - m.lo) / 16.0;
    let mut n = 0;
    for k in 0..16 {
        for a in 0..PROFILE_RING {
            let t = 6.283_185_3f32 * a as f32 / PROFILE_RING as f32;
            let mut p = [0.0f32; 3];
            p[m.axis] = m.lo + w * (k as f32 + 0.5);
            p[b] = m.cb + m.r[k] * t.cos();
            p[c] = m.cc + m.r[k] * t.sin();
            let mut q = qrot(&inv, &[p[0] - local[0], p[1] - local[1], p[2] - local[2]]);
            q[2] -= mz;
            out[3 * n..3 * n + 3].copy_from_slice(&q);
            n += 1;
        }
    }
    Rings { count: n, mz, half_length, widest }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_whole_table_parses() {
        let lines = PROFILES_TEXT.lines().filter(|l| l.trim_start().starts_with('{')).count();
        let t = table();
        assert!(lines > 1000);
        assert_eq!(t.len(), lines);
        assert!(t.windows(2).all(|w| w[0].hash < w[1].hash));
        let first = t[0];
        assert_eq!(first.hash, 0x0003_097c_6f11_73f0);
        assert_eq!(first.axis, 0);
        assert_eq!(first.lo, -2.584_145_777e-2);
        assert_eq!(first.cc, 1.629_850_984);
        assert_eq!(first.r[15], 7.730_476_093e-3);
        assert_eq!(profile_of(first.hash), Some(&t[0]));
        assert_eq!(profile_of(0), None);
        assert_eq!(profile_of(1), None);
    }

    #[test]
    fn rings_of_a_bottle_on_its_cylinder() {
        // a bottle along the mesh's Z, its axis through x 0 y 0, from 0 to 0.3: base radius 0.035, the neck 0.012
        let mut r = [0.035f32; 16];
        for v in r.iter_mut().skip(11) {
            *v = 0.012;
        }
        let m = MeshProfile { hash: 1, axis: 2, lo: 0.0, hi: 0.3, cb: 0.0, cc: 0.0, r };
        // the cylinder's frame is the body's, its middle 0.15 up
        let local = [0.0, 0.0, 0.15, 0.0, 0.0, 0.0, 1.0];
        let mut out = vec![0.0f32; 3 * PROFILE_POINTS];
        let rings = profile_points(&m, &local, &[0.036, 0.036, 0.15], &mut out);
        assert_eq!(rings.count, PROFILE_POINTS);
        assert!(rings.mz.abs() < 1e-6);
        assert!((rings.half_length - 0.15).abs() < 1e-6);
        assert_eq!(rings.widest, 0.035);
        // the first ring: radius 0.035 round the axis, at the first slice's middle below the cylinder's middle
        assert!((out[0] - 0.035).abs() < 1e-6 && out[1].abs() < 1e-6 && (out[2] - (0.3 / 32.0 - 0.15)).abs() < 1e-6);
        // a thing much thicker than its collider is another thing
        assert_eq!(profile_points(&m, &local, &[0.1, 0.1, 0.15], &mut out).count, 0);
        // the cylinder lying across the mesh's axis
        let across = [0.0, 0.0, 0.15, 0.0, 0.707_106_8, 0.0, 0.707_106_8];
        assert_eq!(profile_points(&m, &across, &[0.036, 0.036, 0.15], &mut out).count, 0);
    }
}
