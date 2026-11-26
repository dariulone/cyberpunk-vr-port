//! The colour blits (rust/crates/cvr-gpu/src/color_blit.rs), formerly its HLSL: the quad (VSMain), the blit of a part
//! of a picture (PSMain), the overlay with its blur and black point (OVERLAY_SHADER), the ported HUD composite
//! (HUD_SHADER) and the barrel dot (DOT_SHADER). Translated statement by statement: the same order of operations,
//! HLSL's lerp as a + t * (b - a), saturate as max then min.
use spirv_std::glam::{vec2, vec3, vec4, Vec2, Vec3, Vec4};
use spirv_std::num_traits::Float;
use spirv_std::{spirv, Image, Sampler};

type Picture = Image!(2D, type = f32, sampled);

fn saturate(x: f32) -> f32 {
    x.max(0.0).min(1.0)
}
fn saturate3(v: Vec3) -> Vec3 {
    vec3(saturate(v.x), saturate(v.y), saturate(v.z))
}
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + t * (b - a)
}
/// HLSL's smoothstep(a, b, x).
fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = saturate((x - a) / (b - a));
    t * t * (3.0 - 2.0 * t)
}
/// HLSL's step(edge, x).
fn step(edge: f32, x: f32) -> f32 {
    if x >= edge { 1.0 } else { 0.0 }
}
fn rgb(v: Vec4) -> Vec3 {
    v.truncate()
}

// ---------------------------------------------------------------- the quad and the blit
/// The four corners of a strip and their uv: (-1,-1) (0,1), (-1,1) (0,0), (1,-1) (1,1), (1,1) (1,0).
#[spirv(vertex)]
pub fn vs(#[spirv(vertex_index)] vid: i32, #[spirv(position)] out_position: &mut Vec4, out_uv: &mut Vec2) {
    let (p, uv) = match vid {
        0 => (vec2(-1.0, -1.0), vec2(0.0, 1.0)),
        1 => (vec2(-1.0, 1.0), vec2(0.0, 0.0)),
        2 => (vec2(1.0, -1.0), vec2(1.0, 1.0)),
        _ => (vec2(1.0, 1.0), vec2(1.0, 0.0)),
    };
    *out_position = vec4(p.x, p.y, 0.0, 1.0);
    *out_uv = uv;
}

/// b0: the part of the picture (u0, v0, u1, v1).
#[repr(C)]
#[derive(Copy, Clone)]
pub struct BlitParams {
    pub uv_rect: Vec4,
}

#[spirv(fragment)]
pub fn blit_ps(
    uv: Vec2,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] params: &BlitParams,
    #[spirv(descriptor_set = 0, binding = 2)] color: &Picture,
    #[spirv(descriptor_set = 0, binding = 3)] linear: &Sampler,
    out_color: &mut Vec4,
) {
    let a = vec2(params.uv_rect.x, params.uv_rect.y);
    let at = a + uv * (vec2(params.uv_rect.z, params.uv_rect.w) - a);
    let c: Vec4 = color.sample_by_lod(*linear, at, 0.0);
    *out_color = rgb(c).extend(1.0);
}

// ---------------------------------------------------------------- the overlay
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Tint {
    pub tint: Vec4,
}
/// b1: xy the blur's step, z > 0.5 blurs, w the black point.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Filter {
    pub filter: Vec4,
}

#[spirv(fragment)]
pub fn overlay_ps(
    uv: Vec2,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] tint: &Tint,
    #[spirv(uniform, descriptor_set = 0, binding = 1)] filter: &Filter,
    #[spirv(descriptor_set = 0, binding = 2)] color_t: &Picture,
    #[spirv(descriptor_set = 0, binding = 3)] linear: &Sampler,
    out_color: &mut Vec4,
) {
    let f = filter.filter;
    let mut color: Vec4 = color_t.sample_by_lod(*linear, uv, 0.0);
    if f.z > 0.5 {
        color *= 0.2270270270;
        // [unroll] for (int i = 1; i <= 4; ++i) color += weights[i-1] * (tap(+i) + tap(-i)), written out: a loop whose
        // result is read after it lost its last iteration in naga's HLSL (the A/B found it)
        let step = vec2(f.x, f.y);
        let pair = |i: f32| -> Vec4 {
            let o = step * i;
            let plus: Vec4 = color_t.sample_by_lod(*linear, uv + o, 0.0);
            let minus: Vec4 = color_t.sample_by_lod(*linear, uv - o, 0.0);
            plus + minus
        };
        color += 0.1945945946 * pair(1.0);
        color += 0.1216216216 * pair(2.0);
        color += 0.0540540541 * pair(3.0);
        color += 0.0162162162 * pair(4.0);
    }
    // A black point: light under it is dropped, the rest scaled back to full range and kept in its hue.
    if f.w > 0.0 {
        let peak = color.x.max(color.y.max(color.z));
        let k = saturate((peak - f.w) / (peak * (1.0 - f.w)).max(1e-5));
        color = (rgb(color) * k).extend(color.w);
    }
    *out_color = color * tint.tint;
}

// ---------------------------------------------------------------- the HUD composite
/// b0: the composite's parameters (33 root constants, HudParams in color_blit.rs), one float each so the layout is
/// HLSL's packing as it is.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct HudParams {
    pub curvature_x: f32,
    pub curvature_y: f32,
    pub shadow_u: f32,
    pub shadow_v: f32,
    pub mip_offset1_x: f32,
    pub mip_offset1_y: f32,
    pub mip_offset2_x: f32,
    pub mip_offset2_y: f32,
    pub mip_offset4_x: f32,
    pub mip_offset4_y: f32,
    pub glow_weight1: f32,
    pub glow_weight2: f32,
    pub glow_weight4: f32,
    pub aberration: f32,
    pub hud_gain: f32,
    pub glow_gain: f32,
    pub shadow_strength: f32,
    pub bloom_gain: f32,
    pub bloom_lod: f32,
    pub shadow_mip: f32,
    pub time_sec: f32,
    pub flicker: f32,
    pub target_w: f32,
    pub target_h: f32,
    pub debug_mode: f32,
    pub hud_shift_u: f32,
    pub layer_only: f32,
    pub panel_brightness: f32,
    pub panel_shadow: f32,
    pub panel_glow: f32,
    pub panel_engine_curve: f32,
    /// A vehicle's luminous map projection: a transparent backdrop and perspective into the windshield (main's
    /// 0.2.4), and the map's share of the canvas height (the quest text, unwarped, under it).
    pub windshield_map: f32,
    pub windshield_split: f32,
}
/// b1: the engine's per-frame constants (frameConst[0].x is the time the flicker runs on).
#[repr(C)]
#[derive(Copy, Clone)]
pub struct FrameCb {
    pub frame_const: [Vec4; 30],
}
/// b2: the engine composite's own constants (hudConst[16].zw its target size).
#[repr(C)]
#[derive(Copy, Clone)]
pub struct HudCb {
    pub hud_const: [Vec4; 32],
}

const LUMA: Vec3 = Vec3::new(0.2126, 0.7152, 0.0722);

#[spirv(fragment)]
#[allow(clippy::too_many_arguments)]
pub fn hud_ps(
    uv: Vec2,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] p: &HudParams,
    #[spirv(uniform, descriptor_set = 0, binding = 1)] frame: &FrameCb,
    #[spirv(uniform, descriptor_set = 0, binding = 4)] composite: &HudCb,
    #[spirv(descriptor_set = 0, binding = 5)] scene_t: &Picture,
    #[spirv(descriptor_set = 0, binding = 6)] hud_t: &Picture,
    #[spirv(descriptor_set = 0, binding = 7)] blur_t: &Picture,
    #[spirv(storage_buffer, descriptor_set = 0, binding = 8)] expo_buffer: &[u32],
    #[spirv(descriptor_set = 0, binding = 3)] linear: &Sampler,
    out_color: &mut Vec4,
) {
    if p.windshield_map > 0.5 {
        let split = p.windshield_split.max(0.05).min(1.0);
        if uv.y > split {
            // The quest text under the map: straight, in its own colours, with a dark rim around the letters (the
            // brightest of eight texels 2 px away) so they read over a bright street; no box around them.
            let texel: Vec4 = hud_t.sample_by_lod(*linear, uv, 0.0);
            let brightest = texel.x.max(texel.y.max(texel.z));
            let alpha = smoothstep(0.02, 0.18, brightest) * 0.97;
            let px = vec2(2.0 / p.target_w, 2.0 / p.target_h);
            // [unroll] for (int i = 0; i < 8; ++i) at angle 2*pi*i/8, written out (naga's HLSL loops, see the blur).
            let tap = |c: f32, s: f32| -> f32 {
                let t: Vec4 = hud_t.sample_by_lod(*linear, uv + vec2(c, s) * px, 0.0);
                smoothstep(0.02, 0.18, t.x.max(t.y.max(t.z)))
            };
            const D: f32 = 0.707_106_77;
            let rim = tap(1.0, 0.0)
                .max(tap(D, D))
                .max(tap(0.0, 1.0))
                .max(tap(-D, D))
                .max(tap(-1.0, 0.0))
                .max(tap(-D, -D))
                .max(tap(0.0, -1.0))
                .max(tap(D, -D));
            let backing = 0.6 * rim * step(split + px.y, uv.y);
            let c = rgb(texel) / brightest.max(0.001) * alpha * p.panel_brightness;
            *out_color = c.extend(alpha + backing * (1.0 - alpha));
            return;
        }
        // The map in the top `split` of the canvas, `my` its own height. An inverse trapezoid warp: distant streets
        // converge while nearby streets remain readable; the dark map backing replaced by a frosted one.
        let my = uv.y / split;
        let half_width = lerp(0.30, 0.49, my);
        let map_uv = vec2((uv.x - 0.5) / (2.0 * half_width) + 0.5, my);
        let inside = step(0.0, map_uv.x) * step(map_uv.x, 1.0);
        let texel: Vec4 = hud_t.sample_by_lod(*linear, vec2(map_uv.x, my * split), 0.0);
        let brightest = texel.x.max(texel.y.max(texel.z));
        let edge = smoothstep(0.0, 0.045, my) * smoothstep(0.0, 0.045, 1.0 - my);
        let alpha = smoothstep(0.010, 0.09, brightest) * 0.95 * edge * inside;
        let own = rgb(texel) / brightest.max(0.001);
        let light = vec3(lerp(0.20, own.x, 0.48), lerp(0.88, own.y, 0.48), lerp(0.90, own.z, 0.48));
        // Frosted combiner behind the symbols, soft on every side, as on a real HUD.
        let soft = smoothstep(0.0, 0.07, map_uv.x) * smoothstep(0.0, 0.07, 1.0 - map_uv.x) * smoothstep(0.0, 0.10, my)
            * smoothstep(0.0, 0.10, 1.0 - my);
        let backing = 0.5 * soft * inside;
        let frost = vec3(0.02, 0.06, 0.07) * backing;
        *out_color = ((light * alpha + frost * (1.0 - alpha)) * p.panel_brightness).extend(alpha + backing * (1.0 - alpha));
        return;
    }
    let hc = &composite.hud_const;
    let tgt = vec2(p.target_w, p.target_h);
    let cb_ok = (hc[16].z - tgt.x).abs() < 1.0 && (hc[16].w - tgt.y).abs() < 1.0;

    let mut k_curve = if cb_ok { vec2(hc[3].x, hc[3].y) } else { vec2(p.curvature_x, p.curvature_y) };
    let k_shadow = if cb_ok { vec2(hc[3].z, hc[3].w) } else { vec2(p.shadow_u, p.shadow_v) };
    let k_off1 = if cb_ok { vec2(hc[7].x, hc[7].y) } else { vec2(p.mip_offset1_x, p.mip_offset1_y) };
    let k_off2 = if cb_ok { vec2(hc[7].z, hc[7].w) } else { vec2(p.mip_offset2_x, p.mip_offset2_y) };
    let k_off4 = if cb_ok { vec2(hc[8].x, hc[8].y) } else { vec2(p.mip_offset4_x, p.mip_offset4_y) };
    let mut k_w1 = if cb_ok { hc[8].z } else { p.glow_weight1 };
    let mut k_w2 = if cb_ok { hc[8].w } else { p.glow_weight2 };
    let mut k_w4 = if cb_ok { hc[9].x } else { p.glow_weight4 };
    let mut k_aberr = if cb_ok { hc[6].w } else { p.aberration };
    let k_sat = if cb_ok { hc[6].z } else { 1.0 };
    let mut k_glow = if cb_ok { hc[1].x } else { p.glow_gain };
    let mut k_shadow_s = if cb_ok { hc[4].x } else { p.shadow_strength };
    let k_shadow_m = if cb_ok { hc[4].y } else { p.shadow_mip };
    let mut k_bloom_g = if cb_ok { hc[5].x } else { p.bloom_gain };
    let k_bloom_l = if cb_ok { hc[5].y } else { p.bloom_lod };
    let k_video = if cb_ok { hc[1].z } else { 1.0 };
    if p.layer_only > 0.5 {
        k_shadow_s *= p.panel_shadow;
        k_glow *= p.panel_glow;
        k_bloom_g *= p.panel_glow;
    }
    // The in-eye block's bend is zeroed under the asymmetric projection. The panel is flat unless asked to bend with the
    // engine's values (CyberpunkVR_HudPanelCurve): the live block may still carry them.
    if p.layer_only > 0.5 {
        k_curve = if p.panel_engine_curve > 0.5 { vec2(p.curvature_x, p.curvature_y) } else { vec2(0.0, 0.0) };
    }

    // Bisection switches -- see HudParams::debugMode.
    let dbg = (p.debug_mode + 0.5) as i32;
    let no_warp = dbg == 1 || dbg == 2;
    let no_glow = dbg == 1 || dbg == 4;
    let no_halo = dbg == 1 || dbg == 3;
    let no_aberr = dbg == 1 || dbg == 5;
    if no_warp {
        k_curve = vec2(0.0, 0.0);
    }
    if no_aberr {
        k_aberr = 0.0;
    }
    if no_glow {
        k_w1 = 0.0;
        k_w2 = 0.0;
        k_w4 = 0.0;
    }
    if no_halo {
        k_bloom_g = 0.0;
    }

    // _1487 / _1488: the HUD curvature.
    let cx = uv.x - 0.5;
    let cy2 = (uv.y - 0.5) * 2.0;
    let mut d = vec2(uv.x - (cy2 * cy2) * cx * k_curve.x, uv.y - ((cx * cx) * 2.0) * cy2 * k_curve.y);
    let c2 = (d - 0.5) * 2.0;

    // _1531 and _2347: the scanline flicker.
    let t = frame.frame_const[0].x;
    let k = t * 0.005;
    let st = t.sin();
    let x1 = saturate(d.x) + 1.0;
    let yy = (saturate(d.y) + 1.0) - k;
    let cy = saturate((yy * 650.0).cos().abs());
    let mut f = saturate(
        ((st * 0.2 + 0.54) + (saturate(((x1 + k) * 550.0).cos().abs()) + saturate(((x1 - k) * 250.0).cos().abs())) * 0.18)
            * (cy * 0.4 + 0.6),
    );
    f = lerp(1.0, f, p.flicker);
    let s2 = st * 0.01;
    let mut scan = (saturate((((s2 + d.y) * 1700.0).cos() + 1.0) * 0.75) * 0.0085 + 0.0015)
        * ((s2 + 0.54 + st * 0.1) + (saturate(((x1 + k) * 350.0).cos().abs()) + saturate(((x1 - k) * 350.0).cos().abs())) * 0.18)
        + 0.99;
    scan = lerp(1.0, scan, p.flicker);

    // GIVE THE HUD A DISTANCE: only what is sampled out of the HUD surface moves.
    d.x += p.hud_shift_u;

    // Chromatic aberration: R, G and B come from three different taps.
    let ao = k_aberr * c2;
    let a: Vec4 = hud_t.sample_by_lod(*linear, d + ao, 0.0);
    let b: Vec4 = hud_t.sample_by_lod(*linear, d, 0.0);
    let c: Vec4 = hud_t.sample_by_lod(*linear, d - ao, 0.0);

    let g1: Vec4 = hud_t.sample_by_lod(*linear, d + k_off1 * c2, 1.0);
    let g2: Vec4 = hud_t.sample_by_lod(*linear, d + k_off2 * c2, 2.0);
    let g4: Vec4 = hud_t.sample_by_lod(*linear, d + k_off4 * c2, 4.0);

    let cover = 1.0 - (a.w + b.w + c.w) * 0.3333333333; // _2175
    let mut glow = saturate3(f * (k_w2 * rgb(g2) + k_w1 * rgb(g1) + k_w4 * rgb(g4))) * cover;
    let mut hud = vec3(a.x, b.y, c.z) * p.hud_gain * scan;

    // _1472: the frame exposure every HUD term is scaled by (FrameExposureData's dword 6).
    let expo = (k_video * (f32::from_bits(expo_buffer[6]) - 1.0)) + 1.0;

    // _2318: the shadow the HUD casts, mixing its own alpha with the pyramid's.
    let su = d - k_shadow;
    let sharp: Vec4 = hud_t.sample_by_lod(*linear, su, 0.0);
    let blurred: Vec4 = blur_t.sample_by_lod(*linear, su, (k_shadow_m - 1.0).max(0.0));
    let a_mix = lerp(sharp.w, blurred.w, saturate(k_shadow_m));
    let sh = 1.0 - k_shadow_s * saturate(saturate(a_mix).powf(0.82));

    // The scene loaded undistorted, as the engine does.
    let scene = if p.layer_only > 0.5 {
        Vec3::ZERO
    } else {
        let s: Vec4 = scene_t.sample_by_lod(*linear, uv, 0.0);
        rgb(s)
    };
    glow *= 1.0 - scene.dot(LUMA) * 0.7; // _2263

    let hud_l = hud.dot(LUMA);
    let glow_l = glow.dot(LUMA);
    hud = (hud - hud_l) * k_sat + hud_l;
    glow = (glow - glow_l) * k_sat + glow_l;

    // _2381 * _2379: the wide HUD halo, added outside the coverage/shadow attenuation.
    let halo_s: Vec4 = blur_t.sample_by_lod(*linear, d, k_bloom_l);
    let halo = rgb(halo_s) * (k_bloom_g * expo);

    hud *= expo;
    if p.layer_only > 0.5 {
        // the native composite as premultiplied colour + transmission
        let transmission = saturate(cover * sh);
        let color = (hud + glow * (k_glow * expo) * transmission + halo) * p.panel_brightness;
        *out_color = color.extend(1.0 - transmission);
        return;
    }
    *out_color = (hud + (glow * (k_glow * expo) + scene) * cover * sh + halo).extend(1.0);
}

// ---------------------------------------------------------------- the barrel dot
/// b0: the centre (NDC, +y up), the half extent per axis (aspect-corrected), the colour (straight alpha).
#[repr(C)]
#[derive(Copy, Clone)]
pub struct DotParams {
    pub centre: Vec2,
    pub radius: Vec2,
    pub color: Vec4,
}

#[spirv(vertex)]
pub fn dot_vs(
    #[spirv(vertex_index)] vid: i32,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] dot: &DotParams,
    #[spirv(position)] out_position: &mut Vec4,
    out_local: &mut Vec2,
) {
    let corner = match vid {
        0 => vec2(-1.0, -1.0),
        1 => vec2(-1.0, 1.0),
        2 => vec2(1.0, -1.0),
        _ => vec2(1.0, 1.0),
    };
    *out_local = corner;
    let p = dot.centre + corner * dot.radius;
    *out_position = vec4(p.x, p.y, 0.0, 1.0);
}

/// The disc with a one-pixel feather from its own derivatives: 1 - smoothstep(1 - aa, 1, d), discarded where 0.
#[spirv(fragment)]
pub fn dot_ps(local: Vec2, #[spirv(uniform, descriptor_set = 0, binding = 0)] dot: &DotParams, out_color: &mut Vec4) {
    use spirv_std::arch::Derivative;
    let d = local.length();
    let aa = d.fwidth().max(1e-4);
    // HLSL's smoothstep(e0, e1, x): t = saturate((x - e0) / (e1 - e0)); t * t * (3 - 2 * t)
    let e0 = 1.0 - aa;
    let s = saturate((d - e0) / (1.0 - e0));
    let cov = 1.0 - s * s * (3.0 - 2.0 * s);
    if cov <= 0.0 {
        spirv_std::arch::kill();
    }
    *out_color = rgb(dot.color).extend(dot.color.w * cov);
}
