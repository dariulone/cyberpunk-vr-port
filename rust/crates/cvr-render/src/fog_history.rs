//! The render-parity data model and the fog history's arithmetic (formerly include/Stereo/RenderParity.hpp and
//! include/Stereo/FogHistory.hpp): who owns the render (a sample from MAIN is only good for the same owner, at most
//! one native frame old), MAIN's AA mode and sky radiance as samples, the fog-history borrow's eligibility, the fog
//! camera's projection without the temporal jitter, and the graph-cache key split for the second view.
//!
//! Pure functions over plain data; render_parity.rs applies them to the engine. The checks are those of
//! tools/render_parity_tests (main.cpp, sky.cpp, fog.cpp), run as this file's tests.

/// The volumetric-fog node's camera constants (212 floats).
pub type FogCamera = [f32; 212];
pub type FogProjection = [f32; 16];
pub type FogGrid = [f32; 16];
pub type SkyRadiance = [f32; 4];

/// camera[51].xy: the NDC jitter alone (the native VS removes it this way, see lens_velocity_ps.hlsl).
pub const FOG_CAMERA_JITTER: usize = 204;
/// Anything larger is not a sub-pixel jitter.
pub const FOG_MAX_JITTER: f32 = 0.05;

/// Recovered from dabinn TE6 highlight (docs/te6-testing2-render-review-20260926.md): the graph-cache key of the
/// second view, salted by its view name.
pub fn vrcam_graph_hash(mut hash: u64, mut view_name: u64) -> u64 {
    let mut value: u64 = 0x9EDC_B08A_284E_FDDF;
    for _ in 0..8 {
        value ^= hash & 0xFF;
        value = value.wrapping_mul(0x0000_0100_0000_01B3);
        hash >>= 8;
    }
    for _ in 0..8 {
        value ^= view_name & 0xFF;
        value = value.wrapping_mul(0x0000_0100_0000_01B3);
        view_name >>= 8;
    }
    value
}
pub fn preserve_view_bit33(desired: u64, native: u64) -> u64 {
    const BIT: u64 = 1 << 33;
    (desired & !BIT) | (native & BIT)
}
/// Native bit 33 is useful only when computed under the agreed AA mode; a missing MAIN observation keeps the prior
/// complete behaviour.
pub fn merge_view_flags(desired: u64, native: u64, ready: bool) -> u64 {
    if ready {
        preserve_view_bit33(desired, native)
    } else {
        desired
    }
}
pub fn is_main_aa_context(context: usize, name: u64, main_context: usize) -> bool {
    context != 0 && main_context != 0 && context == main_context && name == 0
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderOwner {
    pub renderer: usize,
    pub player: usize,
    pub main_camera: usize,
    pub eye_camera: usize,
    pub origin: u64,
    pub eye_name: u64,
    pub width: u32,
    pub height: u32,
}
impl RenderOwner {
    pub fn valid(&self) -> bool {
        self.renderer != 0
            && self.player != 0
            && self.main_camera != 0
            && self.eye_camera != 0
            && self.origin != 0
            && self.eye_name != 0
            && self.width != 0
            && self.height != 0
    }
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct AaSample {
    pub owner: RenderOwner,
    pub frame: u32,
    pub mode: u32,
    pub valid: bool,
}
impl AaSample {
    /// The second view can build before MAIN. AA is a setting, not a GPU handle, so the immediately preceding native
    /// frame is valid for this observation.
    pub fn read(&self, now: &RenderOwner, next: u32) -> Option<u32> {
        (self.valid && now.valid() && self.owner == *now && next.wrapping_sub(self.frame) <= 1)
            .then_some(self.mode)
    }
}
pub fn valid_sky_radiance(value: &SkyRadiance) -> bool {
    value.iter().all(|c| c.is_finite() && *c >= 0.0)
        && (0..3).all(|i| (value[i] * value[3]).is_finite())
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SkyRadianceSample {
    pub owner: RenderOwner,
    pub color: SkyRadiance,
    pub frame: u32,
    pub valid: bool,
}
impl SkyRadianceSample {
    pub fn read(&self, now: &RenderOwner, next: u32) -> Option<SkyRadiance> {
        (self.valid
            && now.valid()
            && self.owner == *now
            && next.wrapping_sub(self.frame) <= 1
            && valid_sky_radiance(&self.color))
        .then_some(self.color)
    }
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FogAllocation {
    pub pool: usize,
    pub resource: usize,
    pub object: usize,
    pub srv: usize,
}
impl FogAllocation {
    pub fn valid(&self) -> bool {
        self.pool != 0 && self.resource != 0 && self.object != 0 && self.srv != 0
    }
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct FogSample {
    pub owner: RenderOwner,
    pub allocation: FogAllocation,
    pub registry: usize,
    pub pose: u64,
    pub stamp_us: u64,
    pub frame: u32,
    pub handle: u32,
}
impl FogSample {
    /// Live 2.31 trace: the second view runs first; three persistent input buffers rotate between views. This is
    /// HISTORY, so the preceding MAIN frame is the intended source. A new pose is normal then; same-frame pairs must
    /// agree.
    pub fn eligible(
        &self,
        now: &RenderOwner,
        registry: usize,
        next: u32,
        next_pose: u64,
        time_us: u64,
    ) -> bool {
        let age = next.wrapping_sub(self.frame);
        now.valid()
            && self.owner == *now
            && self.registry == registry
            && registry != 0
            && age <= 1
            && self.pose != 0
            && next_pose != 0
            && (age != 0 || self.pose == next_pose)
            && self.handle != 0
            && self.handle <= 0x7fff_ffff
            && self.allocation.valid()
            && self.stamp_us != 0
            && time_us >= self.stamp_us
            && time_us - self.stamp_us <= 100_000
    }
}

/// The froxel grid's slicing out of the prepare-scene fog constants (rows 25-26 and 43-44).
pub fn read_fog_grid(constants: &[u8]) -> Option<FogGrid> {
    if constants.len() < 43 * 16 + 32 {
        return None;
    }
    let float = |at: usize| {
        f32::from_ne_bytes([
            constants[at],
            constants[at + 1],
            constants[at + 2],
            constants[at + 3],
        ])
    };
    let mut value: FogGrid = [0.0; 16];
    for i in 0..8 {
        value[i] = float(25 * 16 + 4 * i);
        value[8 + i] = float(43 * 16 + 4 * i);
    }
    if !value.iter().all(|f| f.is_finite()) {
        return None;
    }
    if (9..12).any(|i| value[i] < 1.0 || value[i] > 2048.0) || (12..16).any(|i| value[i] <= 0.0) {
        return None;
    }
    Some(value)
}
fn close(a: &[f32; 16], b: &[f32; 16], tolerance: f32) -> bool {
    a.iter().zip(b).all(|(&x, &y)| {
        x.is_finite()
            && y.is_finite()
            && (x - y).abs() <= tolerance * 1.0f32.max(x.abs().max(y.abs()))
    })
}
pub fn same_fog_grid(a: &FogGrid, b: &FogGrid) -> bool {
    close(a, b, 1e-5)
}
/// MAIN's fog history stands in for the second view's only when both froxel grids span the same frustum.
pub fn same_fog_frustum(a: &FogProjection, b: &FogProjection) -> bool {
    close(a, b, 1e-4)
}
fn jitter_ok(camera: &FogCamera) -> bool {
    let jitter = &camera[FOG_CAMERA_JITTER..FOG_CAMERA_JITTER + 2];
    jitter
        .iter()
        .all(|j| j.is_finite() && j.abs() <= FOG_MAX_JITTER)
}
/// The projection alone with the jitter taken out: the frustum the view's froxel grid spans.
pub fn fog_frustum(camera: &FogCamera) -> Option<FogProjection> {
    let projection = &camera[32..48];
    if !projection.iter().all(|f| f.is_finite()) || !jitter_ok(camera) {
        return None;
    }
    let mut p: FogProjection = [0.0; 16];
    p.copy_from_slice(projection);
    p[2] -= camera[FOG_CAMERA_JITTER];
    p[6] -= camera[FOG_CAMERA_JITTER + 1];
    Some(p)
}
/// CameraShaderConsts stores transposed matrices. Fog froxels exclude the temporal projection jitter, as does the
/// native previous-VP matrix -- but NOT the projection's own off-centre shift (AsymProjection), which P20/P21 carry
/// next to the jitter: the jitter is subtracted and the shift kept.
pub fn fog_current_projection(camera: &FogCamera) -> Option<FogProjection> {
    let projection = &camera[32..48];
    let view = &camera[80..96];
    if !projection.iter().chain(view).all(|f| f.is_finite()) {
        return None;
    }
    if projection[0] <= 0.0
        || projection[5] <= 0.0
        || (projection[14] - 1.0).abs() > 1e-4
        || projection[15].abs() > 1e-4
        || (view[15] - 1.0).abs() > 1e-4
        || !jitter_ok(camera)
    {
        return None;
    }
    let p = fog_frustum(camera)?;
    let mut result: FogProjection = [0.0; 16];
    for r in 0..4 {
        for c in 0..4 {
            let mut sum = 0.0f32;
            for k in 0..4 {
                sum += p[r * 4 + k] * view[k * 4 + c];
            }
            if !sum.is_finite() {
                return None;
            }
            result[r * 4 + c] = sum;
        }
    }
    Some(result)
}
/// The native camera with MAIN's projection in its previous-VP slot (camera[48..64]).
pub fn apply_fog_history_projection(
    native: &FogCamera,
    source: &FogProjection,
) -> Option<FogCamera> {
    fog_current_projection(native)?;
    if !source.iter().all(|f| f.is_finite()) {
        return None;
    }
    let mut output = *native;
    output[48..64].copy_from_slice(source);
    Some(output)
}
#[allow(clippy::too_many_arguments)]
pub fn can_borrow_fog(
    sample: &FogSample,
    owner: &RenderOwner,
    registry: usize,
    frame: u32,
    pose: u64,
    time: u64,
    live: &FogAllocation,
    scattering_output: &FogAllocation,
    integrated_output: &FogAllocation,
    source_grid: &FogGrid,
    current_grid: &FogGrid,
) -> bool {
    sample.eligible(owner, registry, frame, pose, time)
        && *live == sample.allocation
        && scattering_output.valid()
        && integrated_output.valid()
        && live.resource != scattering_output.resource
        && live.resource != integrated_output.resource
        && same_fog_grid(source_grid, current_grid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    const VIEW: u64 = 0x77AD_6D68_7165_0500;
    fn owner() -> RenderOwner {
        RenderOwner {
            renderer: 1,
            player: 2,
            main_camera: 3,
            eye_camera: 4,
            origin: 5,
            eye_name: VIEW,
            width: 2560,
            height: 2560,
        }
    }
    fn changed(field: usize) -> RenderOwner {
        let mut o = owner();
        match field {
            0 => o.renderer += 1,
            1 => o.player += 1,
            2 => o.main_camera += 1,
            3 => o.eye_camera += 1,
            4 => o.origin += 1,
            5 => o.eye_name += 1,
            6 => o.width += 1,
            _ => o.height += 1,
        }
        o
    }

    #[test]
    fn cache_partition_aa_flags_and_fog_lifetime() {
        assert!(
            is_main_aa_context(100, 0, 100),
            "MAIN is identified by its camera context"
        );
        assert!(
            !is_main_aa_context(200, 0, 100),
            "other key-zero view is not MAIN"
        );
        assert!(
            !is_main_aa_context(100, 1, 100),
            "named view cannot masquerade as MAIN"
        );
        assert!(
            !is_main_aa_context(0, 0, 0),
            "missing MAIN context stays unclaimed"
        );
        let mut hashes = HashSet::new();
        for i in 0..4096u64 {
            let salted = vrcam_graph_hash(i, VIEW);
            assert_ne!(salted, i, "VRCAM does not reuse MAIN's key");
            assert!(
                hashes.insert(salted),
                "distinct graph keys stay distinct in tested range"
            );
            assert_ne!(
                salted,
                vrcam_graph_hash(i, VIEW + 1),
                "resolution/name change partitions graph cache"
            );
            let native = i.wrapping_mul(0x1_0001_0001);
            let desired = !native;
            let merged = preserve_view_bit33(desired, native);
            assert_eq!(
                merged & !(1 << 33),
                desired & !(1 << 33),
                "keep other forced feature bits"
            );
            assert_eq!(
                merged & (1 << 33),
                native & (1 << 33),
                "preserve native view's bit 33"
            );
        }
        let o = owner();
        let mut aa = AaSample {
            owner: o,
            frame: 500,
            mode: 4,
            valid: true,
        };
        assert_eq!(aa.read(&o, 500), Some(4), "MAIN-first AA mode");
        assert_eq!(
            aa.read(&o, 501),
            Some(4),
            "VRCAM-first AA uses preceding observed setting"
        );
        assert_eq!(aa.read(&o, 502), None, "AA publication stops");
        assert_eq!(aa.read(&o, 499), None, "AA frame reset");
        assert_eq!(aa.read(&changed(3), 501), None, "AA rebind invalidation");
        aa.frame = 0xffff_ffff;
        assert_eq!(aa.read(&o, 0), Some(4), "AA native frame wrap");
        let alloc = FogAllocation {
            pool: 10,
            resource: 11,
            object: 12,
            srv: 13,
        };
        let mut fog = FogSample {
            owner: o,
            allocation: alloc,
            registry: 20,
            pose: 30,
            stamp_us: 1_000_000,
            frame: 500,
            handle: 40,
        };
        assert!(
            fog.eligible(&o, 20, 500, 30, 1_000_010),
            "same-pair fog input"
        );
        assert!(
            fog.eligible(&o, 20, 501, 30, 1_000_010),
            "VRCAM-first reads preceding MAIN history"
        );
        assert!(
            fog.eligible(&o, 20, 501, 31, 1_000_010),
            "head motion does not invalidate preceding history"
        );
        assert!(
            !fog.eligible(&o, 20, 502, 30, 1_000_010),
            "never carry history across a skipped frame"
        );
        assert!(
            !fog.eligible(&o, 20, 499, 30, 1_000_010),
            "never reuse future frame"
        );
        assert!(
            !fog.eligible(&o, 21, 500, 30, 1_000_010),
            "registry replacement"
        );
        assert!(
            !fog.eligible(&o, 20, 500, 31, 1_000_010),
            "head pose pair mismatch"
        );
        assert!(
            !fog.eligible(&o, 20, 500, 30, 1_100_001),
            "long stall expires handle"
        );
        assert!(
            !fog.eligible(&o, 20, 500, 30, 999_999),
            "clock reset expires handle"
        );
        for field in 0..8 {
            assert!(
                !fog.eligible(&changed(field), 20, 500, 30, 1_000_010),
                "load/rebind/recenter/resize invalidation"
            );
        }
        fog.handle = 0;
        assert!(
            !fog.eligible(&o, 20, 500, 30, 1_000_010),
            "missing GPU handle"
        );
        fog.handle = 0xffff_ffff;
        assert!(
            !fog.eligible(&o, 20, 500, 30, 1_000_010),
            "invalid GPU handle sentinel"
        );
        fog.handle = 40;
        fog.allocation = FogAllocation::default();
        assert!(
            !fog.eligible(&o, 20, 500, 30, 1_000_010),
            "missing allocation"
        );
        assert_ne!(
            alloc,
            FogAllocation {
                resource: 21,
                ..alloc
            },
            "recycled D3D resource cannot use the cached handle"
        );
        assert_ne!(
            alloc,
            FogAllocation {
                object: 22,
                ..alloc
            },
            "recycled engine wrapper rejected"
        );
        assert_ne!(
            alloc,
            FogAllocation { srv: 23, ..alloc },
            "recycled SRV rejected"
        );
    }

    #[test]
    fn sky_radiance_weather_and_lifetime() {
        let o = owner();
        let mut source = SkyRadianceSample {
            owner: o,
            color: [30.000004, 60.146496, 93.7846, 1.0],
            frame: 100,
            valid: true,
        };
        assert_eq!(
            source.read(&o, 100),
            Some(source.color),
            "MAIN-first radiance"
        );
        assert_eq!(
            source.read(&o, 101),
            Some(source.color),
            "VRCAM-first uses preceding MAIN"
        );
        assert_eq!(source.read(&o, 102), None, "skipped frame");
        assert_eq!(source.read(&o, 99), None, "future observation rejected");
        for field in 0..8 {
            assert_eq!(
                source.read(&changed(field), 101),
                None,
                "load, recenter, camera and resolution changes invalidate sample"
            );
        }
        source.frame = 0xffff_ffff;
        assert!(source.read(&o, 0).is_some(), "native frame counter wrap");
        source.valid = false;
        assert!(source.read(&o, 0).is_none(), "no MAIN observation");
        source.valid = true;
        assert!(
            source.read(&RenderOwner::default(), 0).is_none(),
            "missing live stereo owner"
        );
        for color in [
            [0.0, 0.0, 0.0, 1.0],
            [0.7, 1.3, 2.1, 137.5],
            [100000.0, 240000.0, 190000.0, 0.6],
            [1.0, 1.0, 1.0, 0.0],
        ] {
            source.color = color;
            assert_eq!(
                source.read(&o, 0),
                Some(color),
                "current authored radiance and zero remain valid"
            );
        }
        for i in 0..4 {
            for bad in [-1.0, f32::INFINITY, f32::NAN] {
                source.color = [1.0, 2.0, 3.0, 1.0];
                source.color[i] = bad;
                assert!(source.read(&o, 0).is_none(), "invalid channels fail");
            }
        }
        source.color = [f32::MAX, 1.0, 1.0, 2.0];
        assert!(
            source.read(&o, 0).is_none(),
            "overflowing radiance rejected"
        );
    }

    fn camera(yaw: f32, x: f32, y: f32, z: f32) -> FogCamera {
        let mut c: FogCamera = core::array::from_fn(|i| i as f32 * 0.01);
        c[32..48].fill(0.0);
        c[80..96].fill(0.0);
        c[32] = 0.7815386;
        c[37] = 0.7815386;
        c[42] = -1.25e-6;
        c[43] = 0.020000025;
        c[46] = 1.0;
        // P20/P21 carry the jitter; camera[51].xy holds it alone.
        c[34] = 0.0006;
        c[204] = 0.0006;
        c[38] = -0.0004;
        c[205] = -0.0004;
        let (co, si) = (yaw.cos(), yaw.sin());
        c[80] = co;
        c[81] = si;
        c[83] = -co * x - si * y;
        c[86] = 1.0;
        c[87] = -z;
        c[88] = si;
        c[89] = -co;
        c[91] = -si * x + co * y;
        c[95] = 1.0;
        c
    }
    fn project(m: &FogProjection, p: [f32; 4]) -> [f32; 4] {
        let mut out = [0.0f32; 4];
        for r in 0..4 {
            for k in 0..4 {
                out[r] += m[r * 4 + k] * p[k];
            }
        }
        out
    }

    #[test]
    fn fog_reprojection_and_resource_pairing() {
        let mut source = camera(0.7, -1359.0, -1798.0, 7.69);
        let source_projection = fog_current_projection(&source).expect("MAIN projection extracted");
        let mut unjittered = source;
        for i in [34, 38, 204, 205] {
            unjittered[i] = 0.0;
        }
        assert_eq!(
            fog_current_projection(&unjittered),
            Some(source_projection),
            "source froxels exclude temporal jitter"
        );
        let mut shifted = source;
        shifted[34] += 0.03;
        shifted[38] += 0.1776;
        let mut shifted_clean = unjittered;
        shifted_clean[34] = 0.03;
        shifted_clean[38] = 0.1776;
        let with_shift = fog_current_projection(&shifted).expect("shifted projection extracted");
        let clean_shift =
            fog_current_projection(&shifted_clean).expect("shifted projection extracted");
        for i in 0..16 {
            assert!(
                (with_shift[i] - clean_shift[i]).abs() <= 1e-6 * 1.0f32.max(clean_shift[i].abs()),
                "jitter removed, shift kept"
            );
        }
        assert_ne!(
            with_shift, source_projection,
            "the shift reaches the borrowed projection"
        );
        let point = [-1355.0, -1792.0, 8.0, 1.0];
        let (a, b) = (
            project(&with_shift, point),
            project(&source_projection, point),
        );
        assert!(
            ((a[1] / a[3] - b[1] / b[3]) - 0.1776).abs() < 1e-4
                && ((a[0] / a[3] - b[0] / b[3]) - 0.03).abs() < 1e-4,
            "NDC moves by exactly the shift"
        );
        let mut rejittered = shifted_clean;
        rejittered[34] += 0.0004;
        rejittered[204] += 0.0004;
        let mut mirrored = shifted_clean;
        mirrored[34] = -0.03;
        let (fa, fb, fc) = (
            fog_frustum(&shifted_clean).unwrap(),
            fog_frustum(&rejittered).unwrap(),
            fog_frustum(&mirrored).unwrap(),
        );
        assert!(same_fog_frustum(&fa, &fb), "jitter alone keeps the frustum");
        assert!(
            !same_fog_frustum(&fa, &fc),
            "a mirrored window is another frustum"
        );
        let mut wild = source;
        wild[204] = 0.5;
        assert!(
            fog_current_projection(&wild).is_none(),
            "a jitter slot holding anything but a sub-pixel jitter is refused"
        );
        for degree in -180..=180 {
            let native = camera(degree as f32 * 0.017_453_292, -1359.065, -1798.0, 7.69);
            let patched = apply_fog_history_projection(&native, &source_projection)
                .expect("head turn accepts valid source");
            for i in 0..patched.len() {
                assert_eq!(
                    patched[i],
                    if (48..64).contains(&i) {
                        source_projection[i - 48]
                    } else {
                        native[i]
                    },
                    "only previous VP changes"
                );
            }
        }
        source[32] = f32::NAN;
        assert!(
            apply_fog_history_projection(&source, &source_projection).is_none(),
            "invalid camera refused"
        );
        let mut invalid = source_projection;
        invalid[3] = f32::INFINITY;
        assert!(
            apply_fog_history_projection(&unjittered, &invalid).is_none(),
            "invalid source refused"
        );
        let mut shared = [0.0f32; 396];
        shared[43 * 4 + 1] = 96.0;
        shared[43 * 4 + 2] = 96.0;
        shared[43 * 4 + 3] = 64.0;
        shared[44 * 4] = 2.0;
        shared[44 * 4 + 1] = 0.5;
        shared[44 * 4 + 2] = 250.0;
        shared[44 * 4 + 3] = 0.004;
        let bytes: Vec<u8> = shared.iter().flat_map(|f| f.to_ne_bytes()).collect();
        let grid = read_fog_grid(&bytes).expect("native fog layout");
        let o = owner();
        let sample = FogSample {
            owner: o,
            allocation: FogAllocation {
                pool: 10,
                resource: 11,
                object: 12,
                srv: 13,
            },
            registry: 20,
            pose: 30,
            stamp_us: 1_000_000,
            frame: 500,
            handle: 40,
        };
        let scattering = FogAllocation {
            pool: 10,
            resource: 21,
            object: 22,
            srv: 23,
        };
        let integrated = FogAllocation {
            pool: 10,
            resource: 31,
            object: 32,
            srv: 33,
        };
        let borrow = |live: FogAllocation,
                      raw: FogAllocation,
                      fin: FogAllocation,
                      current: FogGrid,
                      frame: u32| {
            can_borrow_fog(
                &sample, &o, 20, frame, 31, 1_001_000, &live, &raw, &fin, &grid, &current,
            )
        };
        assert!(
            borrow(sample.allocation, scattering, integrated, grid, 501),
            "MAIN previous-frame source with matching layout"
        );
        assert!(
            !borrow(sample.allocation, sample.allocation, integrated, grid, 501),
            "reject scattering UAV alias"
        );
        assert!(
            !borrow(sample.allocation, scattering, sample.allocation, grid, 501),
            "reject integration UAV alias"
        );
        assert!(
            !borrow(
                FogAllocation {
                    resource: 99,
                    ..sample.allocation
                },
                scattering,
                integrated,
                grid,
                501
            ),
            "reject recycled source"
        );
        assert!(
            !borrow(sample.allocation, scattering, integrated, grid, 502),
            "reject missed frame"
        );
        for i in 0..16 {
            let mut changed = grid;
            changed[i] += 1.0;
            assert!(
                !borrow(sample.allocation, scattering, integrated, changed, 501),
                "reject changed depth/slicing layout"
            );
            changed = grid;
            changed[i] = f32::NAN;
            assert!(
                !borrow(sample.allocation, scattering, integrated, changed, 501),
                "reject nonfinite grid"
            );
        }
        shared[43 * 4 + 1] = 0.0;
        let bytes: Vec<u8> = shared.iter().flat_map(|f| f.to_ne_bytes()).collect();
        assert!(
            read_fog_grid(&bytes).is_none(),
            "reject empty fog dimensions"
        );
    }
}
