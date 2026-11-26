//! The holographic sight's reticle (src/Shaders/sight_reflex_ps.hlsl, formerly DXC's): the collimated reticle -- the
//! dot where the line of sight is parallel to the bore, at its authored angular size -- over the original's four layers
//! and its glitch, the camera scope's image surface (sampled by the viewing ray, its native LCD labels and marks over it),
//! and in the scope build (SIGHT_SCOPE_GLASS: `reflex_ps_scope`) the magnified scope's panels laid on the glass. Every
//! path as the HLSL has it, the same operations in the same order (the HLSL's comments have the whole reasoning, the
//! materials' markers and the measurements); compiled IEEE-strict like the other shaders DXC compiled before.
//!
//! The input signature is the native sight PS's (SV_Position, TEXCOORD0..4: the builder's SIGNATURES).
//! Bindings: set 0 -- 0 GlobalShaderConstants (b0), 4 ShaderSpecificConsts (b4), 1 the reticle atlas (t0), 2 its
//! sampler (s0); set 1 -- 0 the bindless textures (t0, space1: the noise map).
use crate::common::{dot2, dot3, fmax, fmin, length_of2, length_of3, normalize3};
use spirv_std::arch::{kill, Derivative};
use spirv_std::glam::{UVec2, Vec2, Vec3, Vec4, Vec4Swizzles};
#[cfg(target_arch = "spirv")]
use spirv_std::num_traits::Float;
use spirv_std::{spirv, Image, Sampler};

type Texture = Image!(2D, type = f32, sampled);

/// A constant buffer of N float4 rows.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Rows<const N: usize> {
    pub v: [Vec4; N],
}

// the build's constants: the HLSL's #define defaults, which the shipped DXIL was built with (the bench: within
// fast math's noise of it at SIGHT_SIZE_SCALE 1, most of the bytes apart at build_sight.ps1's 2)
const SIGHT_SIZE_SCALE: f32 = 1.0;
const SIGHT_RETICLE_DISTANCE: f32 = 1.0e6;
const SIGHT_RETICLE_RELIEF: f32 = 0.15;
const SIGHT_LABEL_GAIN: f32 = 1.5;
const SIGHT_COLLIMATOR_LIFT: f32 = 2.5;
const SIGHT_COLLIMATOR_GAIN: f32 = 2.5;
const SIGHT_COLLIMATOR_OPAQUE: f32 = 0.6;
const SIGHT_COLLIMATOR_COVER: f32 = 0.85;
// FIXED FRAME, COLLIMATED DOT (main's 92b43b88f, SIGHT_FRAME_FIXED=1): only the dot belongs to the bore -- the atlas is
// sampled at the collimated coordinate inside SIGHT_DOT_RADIUS of the rect's centre (atlas-rect units, soft over
// SIGHT_DOT_SOFT of it) and at the stock painted-on coordinate outside it; below SIGHT_DOT_FLOOR the collimated sample
// is dropped (the faint box and glows around a dot) and the rest rescaled to keep the dot's level.
const SIGHT_DOT_RADIUS: f32 = 0.12;
const SIGHT_DOT_SOFT: f32 = 0.25;
const SIGHT_DOT_FLOOR: f32 = 0.15;
const SIGHT_ZERO_U: f32 = 0.0;
const SIGHT_ZERO_V: f32 = 0.0;
const SIGHT_PANEL_FILL_MARGIN: f32 = 1.2;
const SIGHT_MARK_RING: f32 = 0.03;
const SIGHT_MARK_FLOOR: f32 = 0.6;
const SIGHT_MARK_BACKDROP: f32 = 2.5;
const SIGHT_MARK_PEAK: f32 = 4.0;
const SIGHT_MARK_OPAQUE: f32 = 1.0;
const SIGHT_MARK_COVER: f32 = 0.9;
const SIGHT_MARK_RIM: f32 = 0.6;
const SIGHT_MARK_TINT_LEVEL: f32 = 0.005;

const T47: [f32; 4] = [1.0, 0.5, 0.0, 0.0];
const T50: [f32; 4] = [1.0, 0.0, 1.0, 0.0];
const T52: [f32; 4] = [1.0, 1.0, 0.0, 1.0];

fn saturate(x: f32) -> f32 {
    fmin(fmax(x, 0.0), 1.0)
}
fn saturate2(v: Vec2) -> Vec2 {
    Vec2::new(saturate(v.x), saturate(v.y))
}
fn saturate3(v: Vec3) -> Vec3 {
    Vec3::new(saturate(v.x), saturate(v.y), saturate(v.z))
}
fn max2(a: Vec2, b: Vec2) -> Vec2 {
    Vec2::new(fmax(a.x, b.x), fmax(a.y, b.y))
}
fn min2(a: Vec2, b: Vec2) -> Vec2 {
    Vec2::new(fmin(a.x, b.x), fmin(a.y, b.y))
}
fn max3(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(fmax(a.x, b.x), fmax(a.y, b.y), fmax(a.z, b.z))
}
fn min3(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(fmin(a.x, b.x), fmin(a.y, b.y), fmin(a.z, b.z))
}
fn clamp2(v: Vec2, lo: Vec2, hi: Vec2) -> Vec2 {
    min2(max2(v, lo), hi)
}
/// HLSL's lerp as DXC writes it: x + s * (y - x).
fn lerp(x: f32, y: f32, s: f32) -> f32 {
    x + s * (y - x)
}
fn lerp2(x: Vec2, y: Vec2, s: Vec2) -> Vec2 {
    x + s * (y - x)
}
fn lerp3(x: Vec3, y: Vec3, s: f32) -> Vec3 {
    x + s * (y - x)
}
fn lerp3v(x: Vec3, y: Vec3, s: Vec3) -> Vec3 {
    x + s * (y - x)
}
/// HLSL's smoothstep(a, b, x) as DXC lowers it: s = saturate((x - a) / (b - a)), s * (s * (3 - s * 2)); the division
/// kept one (crate::common::div) where the edges are constants.
fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = saturate(crate::common::div(x - a, b - a));
    t * (t * (3.0 - t * 2.0))
}
fn frac(x: f32) -> f32 {
    crate::common::fract(x)
}
fn max_of(c: Vec3) -> f32 {
    fmax(c.x, fmax(c.y, c.z))
}
fn outside(uv: Vec2) -> bool {
    uv.x < 0.0 || uv.y < 0.0 || uv.x > 1.0 || uv.y > 1.0
}
fn rgb(t: &Texture, s: &Sampler, uv: Vec2) -> Vec3 {
    let c: Vec4 = t.sample_by_lod(*s, uv, 0.0);
    c.xyz()
}
fn atlas_size(t: &Texture) -> Vec2 {
    let size: UVec2 = t.query_size_lod(0);
    Vec2::new(size.x as f32, size.y as f32)
}
/// HLSL's round: to nearest, ties to even (GLSL RoundEven).
#[cfg(target_arch = "spirv")]
fn round(x: f32) -> f32 {
    let mut result = 0.0f32;
    unsafe {
        core::arch::asm! {
            "%glsl = OpExtInstImport \"GLSL.std.450\"",
            "%x = OpLoad _ {x}",
            "%result = OpExtInst typeof*{result} %glsl 2 %x",
            "OpStore {result} %result",
            x = in(reg) &x,
            result = in(reg) &mut result,
        }
    }
    result
}
#[cfg(not(target_arch = "spirv"))]
fn round(x: f32) -> f32 {
    x.round_ties_even()
}

fn panel_fill_extent(panel: Vec2, glass: Vec2) -> Vec2 {
    let p = max2(panel, Vec2::splat(1e-5));
    p * fmax(glass.x / p.x, glass.y / p.y) * SIGHT_PANEL_FILL_MARGIN
}
fn panel_edge_fade(local: Vec2) -> f32 {
    let e = min2(local, Vec2::ONE - local);
    saturate(crate::common::div(fmin(e.x, e.y), 0.03))
}
/// A panel's canvas in its tile: scaled to the tile's width (or height, for a tall canvas), 0 = square.
fn canvas_extent(aspect: f32) -> Vec2 {
    if aspect >= 1.0 {
        Vec2::new(1.0, 1.0 / aspect)
    } else if aspect > 0.0 {
        Vec2::new(aspect, 1.0)
    } else {
        Vec2::ONE
    }
}

/// A camera scope's native panel, readable over the image: its glyphs over the local backdrop at the panel's emission,
/// covering the image with a dark rim, the backdrop a faint tint (and the glass filter by the backdrop's hue).
#[allow(clippy::too_many_arguments)]
fn native_optic_marks(
    slope: Vec2,
    axial: f32,
    projection: Vec4,
    tile: Vec2,
    aspect: f32,
    size: f32,
    fill: Vec2,
    tint: f32,
    filter: &mut Vec3,
    atlas: &Texture,
    s: &Sampler,
) -> Vec4 {
    *filter = Vec3::ONE;
    if projection.w <= 0.0 {
        return Vec4::ZERO;
    }
    let local = if fill.x > 0.0 {
        0.5 + slope * axial / fill
    } else {
        0.5 + slope * fmax(axial + projection.z, 1e-5) / max2(projection.xy() * size, Vec2::splat(1e-5))
    };
    if outside(local) {
        return Vec4::ZERO;
    }
    let canvas = canvas_extent(aspect);
    let lo = tile * 0.5;
    let hi = (tile + canvas) * 0.5;
    let uv = lo + local * canvas * 0.5;
    let texel = 1.0 / atlas_size(atlas);
    let px = max2(uv.fwidth(), texel);
    let in_lo = lo + texel * 0.5;
    let in_hi = hi - texel * 0.5;
    let mut ink = Vec3::ZERO;
    let mut x = -1i32;
    while x <= 1 {
        let mut y = -1i32;
        while y <= 1 {
            ink = max3(ink, rgb(atlas, s, clamp2(uv + Vec2::new(x as f32, y as f32) * px * 0.45, in_lo, in_hi)));
            y += 1;
        }
        x += 1;
    }
    ink = max3(ink, Vec3::ZERO);
    let ring = SIGHT_MARK_RING * 0.5;
    let mut backdrop = ink;
    let mut i = 0i32;
    while i < 8 {
        let angle = i as f32 * 0.7853981634;
        let (sn, cs) = (angle.sin(), angle.cos());
        backdrop = min3(backdrop, max3(rgb(atlas, s, clamp2(uv + Vec2::new(cs, sn) * ring, in_lo, in_hi)), Vec3::ZERO));
        i += 1;
    }
    let floor_level = backdrop * (1.0 + SIGHT_MARK_FLOOR);
    let mut glyph = max3(ink - floor_level, Vec3::ZERO) * projection.w;
    glyph *= fmin(1.0, SIGHT_MARK_PEAK / fmax(max_of(glyph), 1e-6));
    let mut rim = 0.0f32;
    let mut j = 0i32;
    while j < 4 {
        let o = Vec2::new(
            if j == 0 {
                -px.x
            } else if j == 1 {
                px.x
            } else {
                0.0
            },
            if j == 2 {
                -px.y
            } else if j == 3 {
                px.y
            } else {
                0.0
            },
        ) * 1.5;
        rim = fmax(rim, max_of(max3(rgb(atlas, s, clamp2(uv + o, in_lo, in_hi)) - floor_level, Vec3::ZERO)));
        j += 1;
    }
    let fade = if fill.x > 0.0 { panel_edge_fade(local) } else { 1.0 };
    if tint > 0.0 {
        let level = max_of(backdrop);
        *filter = lerp3(Vec3::ONE, backdrop / fmax(level, 1e-6), saturate(tint) * saturate(crate::common::div(level, SIGHT_MARK_TINT_LEVEL)) * fade);
    }
    let color = (backdrop * SIGHT_MARK_BACKDROP + glyph) * fade;
    let cover = saturate(max_of(glyph) / SIGHT_MARK_OPAQUE) * SIGHT_MARK_COVER;
    let edge = saturate(rim * projection.w / SIGHT_MARK_OPAQUE) * SIGHT_MARK_RIM;
    Vec4::new(color.x, color.y, color.z, fmax(cover, edge) * fade)
}

/// The sight's inputs: uv.xy, view z (TEXCOORD0); the glass size and bore (TEXCOORD1); normal.xyz, bitangent.x
/// (TEXCOORD2); bitangent.yz, tangent.xy (TEXCOORD3); tangent.z, the eye-to-pixel ray (TEXCOORD4).
struct In {
    texcoord: Vec4,
    color: Vec4,
    nrm_bx: Vec4,
    byz_txy: Vec4,
    tz_wpos: Vec4,
}

/// The port's built-in sight surfaces (main's 36d046482, ImageScale -718.25 and the integrated camera displays'
/// -717.25): the ink sized in view angles, so leaning along the glass neither resizes nor duplicates the mark. `style`
/// (row 8.x): 0 amber, 1 a smaller dot, 2 the smart sights' corner brackets (apart from the game's lock-on HUD),
/// 3 Senkoh LX's four rounded ivory corners (in angular space: the native atlas' dot mask cannot cut them).
fn integrated_reticle(i: &In, style: f32) -> Vec4 {
    let t = normalize3(Vec3::new(i.byz_txy.z, i.byz_txy.w, i.tz_wpos.x));
    let b = normalize3(Vec3::new(i.nrm_bx.w, i.byz_txy.x, i.byz_txy.y));
    let ray = Vec3::new(i.tz_wpos.y, i.tz_wpos.z, i.tz_wpos.w);
    let axial = dot3(ray, normalize3(i.nrm_bx.xyz()));
    let slope = (Vec2::new(dot3(ray, t), dot3(ray, b)) - Vec2::new(axial * i.color.z, axial * i.color.w)) / fmax(axial.abs(), 1e-5);
    let a = Vec2::new(slope.x.abs(), slope.y.abs());
    let radius = length_of2(slope);
    let px = fmax(fmax(slope.x.fwidth(), slope.y.fwidth()), 1e-5);
    let dot_edge = radius - if style == 1.0 { 0.0016 } else { 0.0021 };
    // four open arcs around a quiet centre
    let mut frame = fmax((radius - 0.013).abs() - 0.0011, 0.0035 - fmin(a.x, a.y));
    if style == 2.0 {
        frame = fmax((fmax(a.x, a.y) - 0.012).abs() - 0.0011, 0.006 - fmin(a.x, a.y));
    }
    if style == 3.0 {
        let q = Vec2::new(a.x - 0.009, a.y - 0.009);
        let rounded = length_of2(max2(q, Vec2::ZERO)) + fmin(fmax(q.x, q.y), 0.0) - 0.003;
        frame = fmax(rounded.abs() - 0.0013, 0.003 - fmin(a.x, a.y));
    }
    let edge = fmin(dot_edge, frame);
    let ink = saturate(0.5 - edge / px);
    let coverage = saturate(0.5 - (edge - 0.8 * px) / px);
    let color = if style == 3.0 {
        Vec3::new(1.26, 1.13, 0.75)
    } else if style == 0.0 {
        Vec3::new(1.4, 0.72, 0.27)
    } else {
        Vec3::new(0.46, 1.22, 1.03)
    };
    Vec4::new(color.x * ink, color.y * ink, color.z * ink, coverage)
}

#[allow(clippy::too_many_arguments)]
fn shade(i: &In, scope: bool, g: &Rows<30>, k: &Rows<21>, atlas: &Texture, noise: &[Texture; 32768], s: &Sampler) -> Vec4 {
    if k.v[6].x == -718.25 {
        return integrated_reticle(i, k.v[8].x);
    }
    if scope {
        // MAGNIFIED SCOPE PANELS (optics path 'shader'): the port's reticle surfaces carry each native panel's tile.
        if k.v[6].x == -715.25 {
            let lens_t = normalize3(Vec3::new(i.byz_txy.z, i.byz_txy.w, i.tz_wpos.x));
            let lens_b = normalize3(Vec3::new(i.nrm_bx.w, i.byz_txy.x, i.byz_txy.y));
            let lens_n = normalize3(i.nrm_bx.xyz());
            let eye_ray = Vec3::new(i.tz_wpos.y, i.tz_wpos.z, i.tz_wpos.w);
            let axial = dot3(eye_ray, lens_n).abs();
            if axial < 1e-5 {
                kill();
            }
            let panel = k.v[8];
            // the division and the product kept as the HLSL has them (spirv-opt folds `(x / y) * y` into x)
            let slope = Vec2::new(crate::common::div(dot3(eye_ray, lens_t), axial), crate::common::div(dot3(eye_ray, lens_b), axial));
            let fill = ((round(k.v[7].x) as u32) & 2) != 0;
            let local = if fill {
                0.5 + Vec2::new(crate::common::mul(slope.x, axial), crate::common::mul(slope.y, axial)) / panel_fill_extent(panel.xy(), i.color.xy())
            } else {
                0.5 + slope * fmax(axial + panel.z, 1e-5) / max2(panel.xy(), Vec2::splat(1e-5))
            };
            if outside(local) {
                kill();
            }
            let fade = if fill { panel_edge_fade(local) } else { 1.0 };
            let mut uv = lerp2(k.v[12].xy(), k.v[12].zw(), local);
            if k.v[15].x != 0.0 {
                uv.y = 1.0 - uv.y;
            }
            let rear = panel.w > 0.5;
            let mut ink = max3(rgb(atlas, s, uv), Vec3::ZERO);
            let mut outline = 0.0f32;
            if rear {
                // the brightest sample over the pixel's footprint: thin hairs never fall between samples
                let px = max2(uv.fwidth(), 1.0 / atlas_size(atlas));
                let mut x = -2i32;
                while x <= 2 {
                    let mut y = -2i32;
                    while y <= 2 {
                        ink = max3(ink, rgb(atlas, s, uv + Vec2::new(x as f32, y as f32) * px * 0.25));
                        y += 1;
                    }
                    x += 1;
                }
                let border = max3(
                    max3(rgb(atlas, s, uv - Vec2::new(px.x, 0.0)), rgb(atlas, s, uv + Vec2::new(px.x, 0.0))),
                    max3(rgb(atlas, s, uv - Vec2::new(0.0, px.y)), rgb(atlas, s, uv + Vec2::new(0.0, px.y))),
                );
                outline = saturate(fmax(border.x, fmax(border.y, border.z)) * k.v[2].x) * 0.8 * fade;
            }
            let color = ink * fmax(k.v[2].x, 0.0) * SIGHT_LABEL_GAIN * fade;
            let coverage = saturate(fmax(color.x, fmax(color.y, color.z)));
            // Premultiplied over (ONE, INV_SRC_ALPHA): alpha 0 adds, alpha 1 replaces.
            return Vec4::new(color.x, color.y, color.z, if rear { fmax(outline, coverage) } else { 0.0 });
        }
    } else if k.v[6].x == -715.25 {
        // the native reticle panels' old meshes must not blend a second time
        kill();
    }
    // The dedicated camera-image surface (ImageScale -713.25 / -714.25 / -716.25; -717.25 an integrated display).
    let integrated_camera = k.v[6].x == -717.25;
    if k.v[6].x == -713.25 || k.v[6].x == -714.25 || k.v[6].x == -716.25 || integrated_camera {
        let composite = k.v[6].x == -714.25;
        let marks = k.v[6].x == -716.25;
        let lens_t = normalize3(Vec3::new(i.byz_txy.z, i.byz_txy.w, i.tz_wpos.x));
        let lens_b = normalize3(Vec3::new(i.nrm_bx.w, i.byz_txy.x, i.byz_txy.y));
        let lens_n = normalize3(i.nrm_bx.xyz());
        let eye_ray = Vec3::new(i.tz_wpos.y, i.tz_wpos.z, i.tz_wpos.w);
        let axial = dot3(eye_ray, lens_n).abs();
        // tan(80deg/2), optics/policy.lua: the image sampled by the viewing ray
        let image_half_slope = 0.8390996312f32;
        let local = 0.5 + Vec2::new(dot3(eye_ray, lens_t), dot3(eye_ray, lens_b)) / (2.0 * image_half_slope * fmax(axial, 1e-5));
        let ray_visible = axial >= 1e-5 && !outside(local);
        if !ray_visible && !composite {
            return Vec4::new(0.0, 0.0, 0.0, 1.0);
        }
        let safe = saturate2(local);
        let camera_uv = if composite { Vec2::new(safe.x * 0.5, safe.y) } else { safe };
        let mut uv = lerp2(k.v[12].xy(), k.v[12].zw(), camera_uv);
        if k.v[15].x != 0.0 {
            uv.y = 1.0 - uv.y;
        }
        // the ink stores the camera image at half intensity: undone
        let mut scene = max3(rgb(atlas, s, uv), Vec3::ZERO) * 2.0;
        if !ray_visible {
            scene = Vec3::ZERO;
        }
        if integrated_camera {
            let r = integrated_reticle(i, k.v[8].x);
            let s = scene * (1.0 - r.w) + r.xyz();
            return Vec4::new(s.x, s.y, s.z, 1.0);
        }
        if composite {
            // the native LCD labels on the other atlas half, on the physical screen
            let mut labels_uv = Vec2::new(0.5 + saturate(i.texcoord.x) * 0.5, 1.0 - saturate(i.texcoord.y));
            labels_uv = lerp2(k.v[12].xy(), k.v[12].zw(), labels_uv);
            if k.v[15].x != 0.0 {
                labels_uv.y = 1.0 - labels_uv.y;
            }
            let sampled: Vec4 = atlas.sample(*s, labels_uv);
            let labels = max3(sampled.xyz(), Vec3::ZERO) * k.v[8].y * SIGHT_LABEL_GAIN;
            let coverage = saturate(fmax(labels.x, fmax(labels.y, labels.z)));
            scene = scene * (1.0 - coverage) + labels;
            // the aim mark, in the image's own angular units
            if ray_visible {
                let sl = Vec2::new(dot3(eye_ray, lens_t), dot3(eye_ray, lens_b)) / fmax(axial, 1e-5);
                let px = fmax(fmax(sl.x.fwidth(), sl.y.fwidth()), 1e-6);
                let a = Vec2::new(sl.x.abs(), sl.y.abs());
                let half_w = fmax(0.0012, px * 0.7);
                let (gap, arm, dot_r) = (0.008f32, 0.050f32, fmax(0.0016, px));
                let mut d_line = 1e9f32;
                if a.x >= gap && a.x <= arm {
                    d_line = fmin(d_line, a.y);
                }
                if a.y >= gap && a.y <= arm {
                    d_line = fmin(d_line, a.x);
                }
                let d_dot = length_of2(sl);
                let ink = fmax(saturate((half_w - d_line) / px + 0.5), saturate((dot_r - d_dot) / px + 0.5));
                let edge = 1.5 * px;
                let halo = fmax(saturate((half_w + edge - d_line) / px + 0.5), saturate((dot_r + edge - d_dot) / px + 0.5));
                scene *= 1.0 - 0.8 * halo;
                scene = lerp3(scene, Vec3::new(0.80, 1.0, 0.95) * 3.0, ink);
            }
        }
        if marks {
            let slope = Vec2::new(dot3(eye_ray, lens_t), dot3(eye_ray, lens_b)) / fmax(axial, 1e-5);
            let front_p = k.v[8];
            let rear_p = Vec4::new(k.v[13].x, k.v[16].x, k.v[18].x, k.v[20].x);
            let front_fill = front_p.z <= -0.5;
            let rear_fill = rear_p.z <= -0.5;
            let mut front_tint = Vec3::ONE;
            let mut rear_tint = Vec3::ONE;
            let front = native_optic_marks(
                slope,
                axial,
                front_p,
                Vec2::new(1.0, 0.0),
                if front_fill { -front_p.z } else { 1.0 },
                1.0,
                if front_fill { panel_fill_extent(front_p.xy(), i.color.xy()) } else { Vec2::ZERO },
                k.v[3].x,
                &mut front_tint,
                atlas,
                s,
            );
            let rear = native_optic_marks(
                slope,
                axial,
                rear_p,
                Vec2::new(1.0, 1.0),
                if rear_fill { -rear_p.z } else { 1.0 },
                1.0,
                if rear_fill { panel_fill_extent(rear_p.xy(), i.color.xy()) } else { Vec2::ZERO },
                k.v[4].x,
                &mut rear_tint,
                atlas,
                s,
            );
            // the glass tint first, then premultiplied: the glyphs cover the image, the backdrop adds to it
            scene *= front_tint * rear_tint;
            scene = scene * (1.0 - front.w) + front.xyz();
            scene = scene * (1.0 - rear.w) + rear.xyz();
        }
        return Vec4::new(scene.x, scene.y, scene.z, 1.0);
    }
    // ---- the collimator
    let t = Vec3::new(i.byz_txy.z, i.byz_txy.w, i.tz_wpos.x);
    let b = Vec3::new(i.nrm_bx.w, i.byz_txy.x, i.byz_txy.y);
    // eye -> pixel, made by the paired VS before interpolating large world coordinates
    let d3 = Vec3::new(i.tz_wpos.y, i.tz_wpos.z, i.tz_wpos.w);
    let dist = fmax(length_of3(d3), 1e-6);
    let v = d3 / dist;
    let v_t = dot3(v, t);
    let v_b = k.v[18].x * dot3(v, b);
    // the atlas rect, exactly as the original
    let v120 = 1.0 - i.texcoord.y;
    let v121 = k.v[12].z - k.v[12].x;
    let v122 = k.v[12].w - k.v[12].y;
    let v126 = (v121 * 0.01) + k.v[12].x;
    let v127 = (v122 * 0.01) + k.v[12].y;
    let v131 = (v121 * 0.99) + k.v[12].x;
    let v132 = (v122 * 0.99) + k.v[12].y;
    // collimation: the glass size from the vertex shader, along the sight's optical axis
    let size_u = i.color.x * SIGHT_SIZE_SCALE;
    let size_v = i.color.y * SIGHT_SIZE_SCALE;
    let ucoord = i.texcoord.x;
    let vcoord = v120;
    let n = i.nrm_bx.xyz();
    let dn = dot3(d3, n);
    let bore = Vec2::new(i.color.z, i.color.w);
    let angular = if scope { 1.0 } else { SIGHT_RETICLE_RELIEF / fmax(dn.abs(), 1e-4) };
    let f_t = (dot3(d3, t) - dn * bore.x) * angular;
    let f_b = (dot3(d3, b) - dn * bore.y) * k.v[18].x * angular;
    let k_coll = SIGHT_RETICLE_DISTANCE / (dist + SIGHT_RETICLE_DISTANCE);
    let mut uv_c = Vec2::new(
        if size_u > 1e-5 { 0.5 + f_t / size_u + SIGHT_ZERO_U } else { ucoord },
        if size_v > 1e-5 { 0.5 + f_b / size_v + SIGHT_ZERO_V } else { vcoord },
    );
    uv_c = lerp2(Vec2::new(ucoord, vcoord), uv_c, Vec2::splat(k_coll));
    let v135 = (v121 * uv_c.x) + k.v[12].x;
    let v136 = (v122 * uv_c.y) + k.v[12].y;
    let (v146, v147, v148) = if k.v[15].x != 0.0 { (1.0 - v136, 1.0 - v132, 1.0 - v127) } else { (v136, v127, v132) };
    let v155 = fmin(fmax(k.v[7].x * v121, 0.0), 0.5);
    let crisp = Vec2::new(saturate(fmin(fmax(v135, v126), v131)), saturate(fmin(fmax(v146, v147), v148)));
    let mut v229: Vec4 = atlas.sample(*s, crisp);
    let mut dot_mask_c = 0.0f32;
    if !scope {
        // the crisp layer at the brightest sample over the pixel's footprint
        let fp = max2(crisp.fwidth(), 1.0 / atlas_size(atlas)) * 0.45;
        let mut x = -1i32;
        while x <= 1 {
            let mut y = -1i32;
            while y <= 1 {
                let at = Vec2::new(
                    saturate(fmin(fmax(crisp.x + x as f32 * fp.x, v126), v131)),
                    saturate(fmin(fmax(crisp.y + y as f32 * fp.y, v147), v148)),
                );
                let c: Vec4 = atlas.sample_by_lod(*s, at, 0.0);
                v229 = Vec4::new(fmax(v229.x, c.x), fmax(v229.y, c.y), fmax(v229.z, c.z), fmax(v229.w, c.w));
                y += 1;
            }
            x += 1;
        }
        // the dot: the collimated sample inside the radius; the frame: the stock painted-on sample outside it
        dot_mask_c = 1.0 - smoothstep(SIGHT_DOT_RADIUS * (1.0 - SIGHT_DOT_SOFT), SIGHT_DOT_RADIUS, length_of2(uv_c - 0.5));
        let uv_s = Vec2::new(ucoord, vcoord);
        let frame_mask = smoothstep(SIGHT_DOT_RADIUS * (1.0 - SIGHT_DOT_SOFT), SIGHT_DOT_RADIUS, length_of2(uv_s - 0.5));
        let sx = (v121 * uv_s.x) + k.v[12].x;
        let sy = (v122 * uv_s.y) + k.v[12].y;
        let sy2 = if k.v[15].x != 0.0 { 1.0 - sy } else { sy };
        let uv_stock = Vec2::new(saturate(fmin(fmax(sx, v126), v131)), saturate(fmin(fmax(sy2, v147), v148)));
        let mut stock: Vec4 = atlas.sample(*s, uv_stock);
        let sfp = max2(uv_stock.fwidth(), 1.0 / atlas_size(atlas)) * 0.45;
        let mut x = -1i32;
        while x <= 1 {
            let mut y = -1i32;
            while y <= 1 {
                let at = Vec2::new(
                    saturate(fmin(fmax(uv_stock.x + x as f32 * sfp.x, v126), v131)),
                    saturate(fmin(fmax(uv_stock.y + y as f32 * sfp.y, v147), v148)),
                );
                let c: Vec4 = atlas.sample_by_lod(*s, at, 0.0);
                stock = Vec4::new(fmax(stock.x, c.x), fmax(stock.y, c.y), fmax(stock.z, c.z), fmax(stock.w, c.w));
                y += 1;
            }
            x += 1;
        }
        let dot = |c: f32, st: f32| fmax(crate::common::div(fmax(c - SIGHT_DOT_FLOOR, 0.0), 1.0 - SIGHT_DOT_FLOOR) * dot_mask_c, st * frame_mask);
        v229 = Vec4::new(dot(v229.x, stock.x), dot(v229.y, stock.y), dot(v229.z, stock.z), dot(v229.w, stock.w));
    }
    let layer = |m: f32| -> Vec4 {
        atlas.sample(
            *s,
            Vec2::new(saturate(fmin(fmax(((v_t * m) * v155) + v135, v126), v131)), saturate(fmin(fmax(((v_b * m) * v155) + v146, v147), v148))),
        )
    };
    let v235: Vec4 = atlas.sample(
        *s,
        Vec2::new(saturate(fmin(fmax((v155 * v_t) + v135, v126), v131)), saturate(fmin(fmax((v155 * v_b) + v146, v147), v148))),
    );
    let v241 = layer(2.0);
    let v247 = layer(3.0);
    let (v235, v241, v247) = if scope {
        (v235, v241, v247)
    } else {
        // the smear layers keep the dot alone, its faint background dropped
        let f = |c: Vec4| {
            Vec4::new(
                fmax(c.x - SIGHT_DOT_FLOOR, 0.0) * dot_mask_c,
                fmax(c.y - SIGHT_DOT_FLOOR, 0.0) * dot_mask_c,
                fmax(c.z - SIGHT_DOT_FLOOR, 0.0) * dot_mask_c,
                fmax(c.w - SIGHT_DOT_FLOOR, 0.0) * dot_mask_c,
            )
        };
        (f(v235), f(v241), f(v247))
    };
    // ---- everything below is the original, unchanged
    let v259 = fmin(fmax(k.v[11].x, 0.75), 1.0);
    let noise_index = k.v[1].x.to_bits();
    let noise_sample: Vec4 = noise[noise_index as usize].sample(
        *s,
        Vec2::new((k.v[9].z * g.v[0].x) + (k.v[9].x * i.texcoord.x), (k.v[9].w * g.v[0].x) + (k.v[9].y * i.texcoord.y)),
    );
    let v262 = ((1.0 - v259) * noise_sample.x) + v259;
    let blend = |c235: f32, c229: f32, c241: f32, c247: f32| -> f32 {
        1.0 - ((((1.0 - ((k.v[8].y * c235) * v262)) * (1.0 - (k.v[8].x * c229))) * (1.0 - ((v262 * c241) * k.v[8].z))) * (1.0 - ((v262 * c247) * k.v[8].w)))
    };
    let v326 = blend(v235.x, v229.x, v241.x, v247.x);
    let v327 = blend(v235.y, v229.y, v241.y, v247.y);
    let v328 = blend(v235.z, v229.z, v241.z, v247.z);
    let v329 = blend(v235.w, v229.w, v241.w, v247.w);
    let time = g.v[0].x;
    let v338 = (frac(time + 0.375) * 6.0) + 2.0;
    let v346 = frac((((time + 0.35747999) * 60.0).floor()) * 0.1031);
    let v349 = dot3(Vec3::splat(v346), Vec3::splat(v346 + 33.33));
    let v369 = frac(frac(((v346 + v346) + (v349 * 2.0)) * (v349 + v346)) + i.texcoord.x)
        / ((saturate(frac((((v338 * i.texcoord.x).floor() / v338) + 1.0) * time)) * 3.0) + 1.0);
    let v378 = frac(((frac(time) * ((v369 * v338).floor() / v338)) + (-0.16249999)) * 0.1031);
    let v380 = dot3(Vec3::splat(v378), Vec3::splat(v378 + 33.33));
    let v391 = (frac(((v378 + v378) + (v380 * 2.0)) * (v380 + v378)) * 4.0) as i32 as u32;
    let v400 = frac(time * 0.1031);
    let v402 = dot3(Vec3::splat(v400), Vec3::splat(v400 + 33.33));
    let v410 = frac(((v400 + v400) + (v402 * 2.0)) * (v402 + v400));
    let v417 = if v410 < 0.69999999 { 0.0 } else { 1.0 };
    let v433 = frac((time * 10.0).floor() * 0.1031);
    let v435 = v433 * (v433 + 33.33);
    let v436 = v435 * v435;
    let (v445, v447, v449, v451);
    if frac(v436 * 2.0) < (1.0 - k.v[10].x) {
        (v445, v447, v449, v451) = (v326, v327, v328, v329);
    } else if (i.texcoord.x > 0.77999997 && i.texcoord.x < 0.85000002)
        || ((i.texcoord.x > 0.019999999 && i.texcoord.x < 0.11999999) || (i.texcoord.x > 0.28000000 && i.texcoord.x < 0.54299998))
    {
        let v544: Vec4 = atlas.sample(*s, Vec2::splat(v369));
        let v555: Vec4 = atlas.sample(
            *s,
            Vec2::new(
                ((v417 * ((v410 * 0.024999998) + (-0.019999999))) + 0.0099999998) + v135,
                ((v417 * ((v410 * 0.029999999) + (-0.0099999998))) + (-0.0099999998)) + v146,
            ),
        );
        v445 = (v555.x * 0.40000000) + (v544.x * T47[v391 as usize]);
        v447 = (v555.y * 0.40000000) + (v544.y * T50[v391 as usize]);
        v449 = (v555.z * 0.40000000) + (v544.z * T52[v391 as usize]);
        v451 = (v555.w * 0.40000000) + v544.w;
    } else {
        (v445, v447, v449, v451) = (v326, v327, v328, v329);
    }
    let v461 = saturate((((i.texcoord.y * 4.0) * v120) + 1.0) - k.v[16].x);
    let v473 = ((v461 * v445) * k.v[17].x) * k.v[2].x;
    let v476 = ((v461 * v447) * k.v[17].y) * k.v[2].x;
    let v479 = ((v461 * v449) * k.v[17].z) * k.v[2].x;
    let v485 = saturate(fmax(fmax(fmin(1.0, v473), fmin(1.0, v476)), fmin(1.0, v479)));
    let v491 = (k.v[0].x * (v451 - v485)) + v485;
    let v498 = ((1.0 - v491) * k.v[19].x) + v491;
    let channel = |c: f32| fmin(fmax((-0.0) - fmin((-0.0) - (c * v498), 0.0), 0.0), 65000.0);
    let mut o = Vec4::new(channel(v473), channel(v476), channel(v479), saturate(v498 * k.v[0].x));
    if scope {
        o = Vec4::new(o.x * SIGHT_LABEL_GAIN, o.y * SIGHT_LABEL_GAIN, o.z * SIGHT_LABEL_GAIN, o.w);
    } else {
        let m = max_of(o.xyz());
        if m > 1e-5 {
            let lifted = fmax(m, fmin(m * SIGHT_COLLIMATOR_GAIN, SIGHT_COLLIMATOR_LIFT));
            let ratio = lifted / m;
            o = Vec4::new(o.x * ratio, o.y * ratio, o.z * ratio, fmax(o.w, saturate(crate::common::div(lifted, SIGHT_COLLIMATOR_OPAQUE)) * SIGHT_COLLIMATOR_COVER));
        }
    }
    o
}

macro_rules! sight_entry {
    ($name:ident, $scope:expr) => {
        #[spirv(fragment)]
        #[allow(clippy::too_many_arguments)]
        pub fn $name(
            #[spirv(frag_coord)] _pos: Vec4,
            texcoord: Vec4,
            color: Vec4,
            nrm_bx: Vec4,
            byz_txy: Vec4,
            tz_wpos: Vec4,
            #[spirv(uniform, descriptor_set = 0, binding = 0)] g: &Rows<30>,
            #[spirv(uniform, descriptor_set = 0, binding = 4)] k: &Rows<21>,
            #[spirv(descriptor_set = 0, binding = 1)] atlas: &Texture,
            #[spirv(descriptor_set = 1, binding = 0)] noise: &[Texture; 32768],
            #[spirv(descriptor_set = 0, binding = 2)] s: &Sampler,
            out: &mut Vec4,
        ) {
            let i = In { texcoord, color, nrm_bx, byz_txy, tz_wpos };
            *out = shade(&i, $scope, g, k, atlas, noise, s);
        }
    };
}
sight_entry!(reflex_ps, false);
sight_entry!(reflex_ps_scope, true);

#[allow(dead_code)]
fn unused(v: Vec3, w: Vec3) -> Vec3 {
    let _ = dot2(Vec2::ZERO, Vec2::ZERO);
    lerp3v(v, w, w) + saturate3(v) + Vec3::splat(lerp(0.0, 1.0, 0.5))
}
