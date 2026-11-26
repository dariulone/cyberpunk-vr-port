//! The scanner / focus outline (src/Shaders/vision_highlight_cs.hlsl, formerly DXC's): the engine's
//! VisionMode_Highlight compute shader, swapped into the engine's own pipeline, as the capture's decompilation has it --
//! statement for statement, the decompiler's numbered values kept so the two read side by side -- with the port's two
//! changes: the flags taps' radius (VRP_THICKNESS, kept at 1) and the port's own white outline grown by VRP_WHITE_GROW
//! texels around what the port asked to outline. (The HLSL's comments have the whole reasoning and the measurements.)
//!
//! Bindings (descriptor set 0) at the engine's registers: 0 b0, 1 b1, 12 b12, 6 b6 (constant buffers); 20.. t0, t1, t2,
//! t4, t5 (single-channel depths and strengths), t6 (the highlight flags, int2), t7, t8; 40 u0, 41 u1; 50 s11.
use crate::common::{dot3, fmax, fmin};
use spirv_std::glam::{IVec2, IVec4, UVec2, UVec3, Vec2, Vec3, Vec4};
#[cfg(target_arch = "spirv")]
use spirv_std::num_traits::Float;
use spirv_std::{spirv, Image, Sampler};

type Texture = Image!(2D, type = f32, sampled);
type IntTexture = Image!(2D, type = i32, sampled);
type OutColour = Image!(2D, format = rgba16f, sampled = false);

/// A constant buffer of N float4 rows.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Rows<const N: usize> {
    pub v: [Vec4; N],
}

/// The tap radius multiplier: 1 reproduces the engine exactly (the HLSL's note: keep it at 1).
const VRP_THICKNESS: i32 = 1;
/// How many texels the port's WHITE outline grows by (0: an exact copy of the engine's behaviour).
const VRP_WHITE_GROW: i32 = 3;
/// The added band's opacity.
const VRP_GROW_ALPHA: f32 = 1.0;

const T52: [f32; 16] = [1.5, 0.0, 0.0, 1.5, 1.5, 1.5, 0.800000011920928955078125, 1.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
const T59: [f32; 16] = [5.0, 3.5, 3.5, 5.0, 5.0, 5.0, 1.0, 5.0, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5];
const T67: [f32; 16] = [1000.0, 1000.0, 20.0, 20.0, 30.0, 20.0, 40.0, 60.0, 1000.0, 50.0, 50.0, 20.0, 50.0, 1000.0, 20.0, 20.0];
const T73: [f32; 16] = [1001.0, 1001.0, 35.0, 35.0, 65.0, 35.0, 50.0, 75.0, 1001.0, 65.0, 65.0, 35.0, 65.0, 1001.0, 35.0, 35.0];
const T78: [f32; 16] = [
    1.0,
    0.20000000298023223876953125,
    0.20000000298023223876953125,
    0.100000001490116119384765625,
    0.20000000298023223876953125,
    0.100000001490116119384765625,
    0.20000000298023223876953125,
    0.25,
    0.20000000298023223876953125,
    1.0,
    1.0,
    1.0,
    1.0,
    0.20000000298023223876953125,
    1.0,
    1.0,
];
/// c_offsets, signed: the centre, then the eight neighbours at radius one.
const OFFSETS: [i32; 18] = [0, 0, -1, 0, 1, 0, 0, -1, 0, 1, -1, 1, 1, -1, -1, -1, 1, 1];
/// Highlights.1.v: two palettes of eight (fill, outline), RGBA.
const PALETTE: [f32; 64] = [
    0.0, 0.0, 0.0, 1.0, 0.941176474094390869140625, 0.70980393886566162109375, 0.21568627655506134033203125, 1.0,
    0.3529411852359771728515625, 0.4823529422283172607421875, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.113725490868091583251953125, 1.0,
    0.19607843458652496337890625, 1.0, 0.12549020349979400634765625, 0.4823529422283172607421875, 1.0, 1.0, 1.0,
    0.19607843458652496337890625, 0.0, 1.0, 1.0, 0.090196080505847930908203125, 0.0431372560560703277587890625, 1.0, 0.0, 0.0, 0.0,
    1.0, 0.113725490868091583251953125, 1.0, 0.19607843458652496337890625, 1.0, 1.0, 0.0588235296308994293212890625,
    0.01960784383118152618408203125, 1.0, 0.3529411852359771728515625, 0.4823529422283172607421875, 1.0, 1.0, 0.99215686321258544921875,
    0.22745098173618316650390625, 0.17647059261798858642578125, 1.0, 0.941176474094390869140625, 0.70980393886566162109375,
    0.21568627655506134033203125, 1.0, 0.12549020349979400634765625, 0.4823529422283172607421875, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0,
];

fn clamp01(x: f32) -> f32 {
    fmin(fmax(x, 0.0), 1.0)
}
fn frac(x: f32) -> f32 {
    crate::common::fract(x)
}
fn load(t: &Texture, x: u32, y: u32) -> f32 {
    let v: Vec4 = t.fetch(IVec2::new(x as i32, y as i32));
    v.x
}
fn flags(t: &IntTexture, x: u32, y: u32) -> u32 {
    let v: IVec4 = t.fetch(IVec2::new(x as i32, y as i32));
    v.y as u32
}
/// 1 / max(((k[26].x * d + k[26].y) * k[25].x) + k[25].y, 1e-7): the view distance of a depth.
fn distance(d: f32, k: &Rows<99>) -> f32 {
    1.0 / fmax((((k.v[26].x * d) + k.v[26].y) * k.v[25].x) + k.v[25].y, 1.0000000116860974230803549289703e-07)
}
/// The flags tap at thread + offset[i], clamped to the view (the offset scaled by VRP_THICKNESS).
fn tap(id: UVec3, i: u32, limit_x: f32, limit_y: f32) -> (u32, u32) {
    let x = (id.x as i32 + OFFSETS[(i * 2) as usize] * VRP_THICKNESS) as f32;
    let y = (id.y as i32 + OFFSETS[(1 + i * 2) as usize] * VRP_THICKNESS) as f32;
    (fmin(fmax(x, 0.0), limit_x) as i32 as u32, fmin(fmax(y, 0.0), limit_y) as i32 as u32)
}
/// Whether a surface stands out from the background (the silhouette's depth step).
fn steps(near: f32, far: f32) -> bool {
    (near - far) > ((clamp01(far * 0.00999999977648258209228515625) * 0.1500000059604644775390625) + 0.0500000007450580596923828125)
}

#[spirv(compute(threads(8, 8, 1)))]
// the decompiler's form kept: its initial values, and _303 (read only by a choice between equal values)
#[allow(clippy::too_many_arguments, unused_assignments, unused_variables)]
pub fn main(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] k28: &Rows<30>,
    #[spirv(uniform, descriptor_set = 0, binding = 1)] k33: &Rows<53>,
    #[spirv(uniform, descriptor_set = 0, binding = 12)] k38: &Rows<99>,
    #[spirv(uniform, descriptor_set = 0, binding = 6)] k43: &Rows<3>,
    #[spirv(descriptor_set = 0, binding = 20)] t8: &Texture,
    #[spirv(descriptor_set = 0, binding = 21)] t9: &Texture,
    #[spirv(descriptor_set = 0, binding = 22)] t10: &Texture,
    #[spirv(descriptor_set = 0, binding = 24)] t11: &Texture,
    #[spirv(descriptor_set = 0, binding = 25)] t12: &Texture,
    #[spirv(descriptor_set = 0, binding = 26)] t16: &IntTexture,
    #[spirv(descriptor_set = 0, binding = 27)] t17: &Texture,
    #[spirv(descriptor_set = 0, binding = 28)] t18: &Texture,
    #[spirv(descriptor_set = 0, binding = 40)] u21: &OutColour,
    #[spirv(descriptor_set = 0, binding = 41)] u22: &OutColour,
    #[spirv(descriptor_set = 0, binding = 50)] s46: &Sampler,
) {
    let v145 = k38.v[79].z * k38.v[27].x;
    let v146 = k38.v[79].w * k38.v[27].y;
    let v147 = v146 + (-1.0);
    let v149 = v145 + (-1.0);
    // the first scan: the nine flags taps
    let (mut v150, mut v152, mut v154, mut v156, mut v158, mut v160, mut v162, mut v164) = (0u32, 0u32, 0.0f32, 0.0f32, 0.0f32, 0u32, 0u32, 0u32);
    let (mut v166, mut v169, mut v171, mut v173, mut v175, mut v177) = (0u32, 0u32, 0u32, 0u32, 0u32, 0u32);
    let (mut v298, mut v299, mut v300, mut v301, mut v302, mut v303) = (0u32, 0u32, 0.0f32, 0.0f32, 0.0f32, 0u32);
    let (mut v304, mut v305, mut v306, mut v307, mut v308, mut v309, mut v310, mut v311) = (0u32, 0u32, 0u32, 0u32, 0u32, 0u32, 0u32, 0u32);
    loop {
        let (v198, v199) = tap(id, v164, v149, v147);
        let v203 = flags(t16, v198, v199);
        let v216 = v203 & 3;
        let v218 = (v203 >> 2) & 7;
        let v222 = (v203 >> 5) & 1;
        let v242 = distance(load(t11, v198, v199), k38);
        let v251 = distance(
            load(
                t10,
                (k38.v[79].x * (v198 as i32) as f32).ceil() as i32 as u32,
                (k38.v[79].y * (v199 as i32) as f32).ceil() as i32 as u32,
            ),
            k38,
        );
        let (v254, v256) = if v216 == 0 {
            (0u32, 0u32)
        } else {
            let v269 = steps(v242, v251);
            (v269 as u32, ((v203 & 64) != 0 || !v269) as u32)
        };
        let (fp0, fp1, fp2, fp3, fp4, fp5, fp6, fp7, fp8, fp9, fp10, fp11, fp12);
        if v256 == 0 {
            (fp0, fp1, fp2, fp3, fp4, fp5) = (v150, v171, v169, v166, v162, v160);
            (fp6, fp7, fp8, fp9, fp10, fp11, fp12) = (v158, v156, v154, v152, v175, v177, v173);
        } else {
            let v284 = (v203 & 2) != 0;
            let v285 = if v284 { v218 } else { 0 };
            let v286 = v285;
            let v289 = v285;
            let v274 = if k43.v[2].z.to_bits() == 0 {
                ((1.0
                    - (clamp01((v242 - T52[v286 as usize]) / (T59[v285 as usize] - T52[v286 as usize]))
                        * (1.0 - clamp01((v242 - T67[v289 as usize]) / (T73[v285 as usize] - T67[v289 as usize])))))
                    * (T78[v285 as usize] + (-1.0)))
                    + 1.0
            } else {
                1.0
            };
            let v273 = load(t12, v198, v199) * v274;
            let (v272, v275, v276, v277, v278, v279);
            if v150 == 0 && (v203 & 1) != 0 {
                if v152 != 0 {
                    (v298, v299, v300, v301, v302, v303) = (1, v152, v154, v156, v273, v160);
                    (v304, v305, v306, v307, v308) = (v254, v164, v218, v218, v216);
                    (v309, v310, v311) = (v173, v175, v177);
                    break;
                }
                (v272, v275, v276, v277, v278, v279) = (1, v273, v254, v218, v218, v216);
            } else {
                (v272, v275, v276, v277, v278, v279) = (v150, v158, v162, v166, v169, v171);
            }
            let (l0, l1, l2, l3, l4, l5, l6, l7, l8, l9, l10, l11, l12);
            if v152 == 0 && v284 {
                if v272 != 0 {
                    (v298, v299, v300, v301, v302, v303) = (v272, 1, v273, v274, v275, v254);
                    (v304, v305, v306, v307, v308) = (v276, v164, v277, v278, v279);
                    (v309, v310, v311) = (v222, v218, v216);
                    break;
                }
                (l0, l1, l2, l3, l4, l5) = (0, v279, v278, v277, v276, v254);
                (l6, l7, l8, l9, l10, l11, l12) = (v275, v274, v273, 1, v218, v216, v222);
            } else {
                (l0, l1, l2, l3, l4, l5) = (v272, v279, v278, v277, v276, v160);
                (l6, l7, l8, l9, l10, l11, l12) = (v275, v156, v154, v152, v175, v177, v173);
            }
            (fp0, fp1, fp2, fp3, fp4, fp5) = (l0, l1, l2, l3, l4, l5);
            (fp6, fp7, fp8, fp9, fp10, fp11, fp12) = (l6, l7, l8, l9, l10, l11, l12);
        }
        let (v151, v172, v170, v168, v163, v161) = (fp0, fp1, fp2, fp3, fp4, fp5);
        let (v159, v157, v155, v153, v176, v178, v174) = (fp6, fp7, fp8, fp9, fp10, fp11, fp12);
        let v165 = v164 + 1;
        if v165 < 9 {
            (v150, v152, v154, v156, v158, v160) = (v151, v153, v155, v157, v159, v161);
            (v162, v164, v166, v169, v171) = (v163, v165, v168, v170, v172);
            (v173, v175, v177) = (v174, v176, v178);
            continue;
        } else {
            (v298, v299, v300, v301, v302, v303) = (v151, v153, v155, v157, v159, v161);
            (v304, v305, v306, v307, v308) = (v163, v165, v168, v170, v172);
            (v309, v310, v311) = (v174, v176, v178);
            break;
        }
    }
    let v313 = 1.0 / v146;
    let v314 = (id.x as i32) as f32;
    let v315 = (id.y as i32) as f32;
    let v316 = (1.0 / v145) * v314;
    let v317 = v313 * v315;
    let v318 = v316 + (-0.5);
    let v321 = (v317 + (-0.5)) * 2.0;
    let v326 = (v321 * v321) * (v318 * (-0.0089999996125698089599609375));
    let v330 = ((v318 * v318) * (-0.17000000178813934326171875)) * v321;
    let v333 = id.x.wrapping_add(4294967295);
    let v334 = id.y.wrapping_add(4294967295);
    let v351 = distance(load(t11, v333, v334), k38);
    let v352 = (-0.0) - v351;
    let v362 = distance(load(t11, id.x, v334), k38);
    let v364 = id.x.wrapping_add(1);
    let v373 = distance(load(t11, v364, v334), k38);
    let v384 = distance(load(t11, v333, id.y), k38);
    let v396 = distance(load(t11, v364, id.y), k38);
    let v400 = id.y.wrapping_add(1);
    let v409 = distance(load(t11, v333, v400), k38);
    let v419 = distance(load(t11, id.x, v400), k38);
    let v430 = distance(load(t11, v364, v400), k38);
    let v447 = distance(load(t11, id.x, id.y), k38);
    let v451 = (((((((v351 - v373) + (v384 * 2.0)) - (v396 * 2.0)) + v409) - v430) / v447) * 0.5) * k38.v[25].x;
    let v453 = ((((((v352 - v373) + v409) + v430) + ((v419 - v362) * 2.0)) / v447) * 0.5) * k38.v[25].x;
    let v474 = ((v146 * 400.0) * v313) * (v330 + v317);
    let v475 = v474.floor();
    let v477 = (v475 * 0.5) + (((v145 * 400.0) * v313) * (v326 + v316));
    let v484 = frac(v477.floor() * 0.103100001811981201171875);
    let v485 = frac(v475 * 0.103100001811981201171875);
    let v488 = v484 + 33.3300018310546875;
    let v489 = dot3(Vec3::new(v484, v485, v484), Vec3::new(v485 + 33.3300018310546875, v488, v488));
    let v509 = frac(v475 * (-0.103100001811981201171875));
    let v511 = v509 * (v509 + 33.3300018310546875);
    let v512 = v511 * v511;
    let v521 = (frac((k43.v[2].x * 5.0) + frac(v512 * 2.0)).log2() * 0.300000011920928955078125).exp2();
    let v522 = frac(v477) + (-0.5);
    let v523 = frac(v474) + (-0.5);
    let v528 = (((frac(((v485 + v484) + (v489 * 2.0)) * (v489 + v484)) * 40.0 * k43.v[2].x).sin() * 0.0500000007450580596923828125)
        + 0.89999997615814208984375)
        - ((v523 * v523) + (v522 * v522)).sqrt();
    let (v559, v561, v563, v565, v567, v569, v571);
    if v298 == 0 {
        (v559, v561, v563) = (v447, v314, v315);
        (v565, v567, v569, v571) = (0.0, 0.0, 0.0, 0.0);
    } else {
        let (v560, v562, v564, v794, v796, v798);
        if v305 < 9 {
            // the second scan, from where the first stopped: the nearest outlined neighbour
            let (mut v931, mut v933, mut v934, mut v935) = (v305, v306, v307, v308);
            let (mut v936, mut v937, mut v938) = (v447, v314, v315);
            let (v795, v797, v799, v800, v801, v802);
            loop {
                let (v955, v956) = tap(id, v931, v149, v147);
                let v958 = k38.v[79];
                let v961 = flags(t16, v955, v956);
                let v963 = (v961 >> 2) & 7;
                let v965 = (v961 & 64) != 0;
                let v967 = (v961 & 1) == 0;
                let (q0, q1, q2, q3, q4, q5);
                if v967 {
                    (q0, q1, q2, q3, q4, q5) = (v938, v937, v935, v934, v933, v936);
                } else {
                    let v1029 = v961 & 3;
                    let v1030 = (v956 as i32) as f32;
                    let v1035 = (v955 as i32) as f32;
                    let v1056 = distance(load(t11, v955, v956), k38);
                    let v1065 = distance(
                        load(t10, (v958.x * v1035).ceil() as i32 as u32, (v1030 * v958.y).ceil() as i32 as u32),
                        k38,
                    );
                    let v1161 = if v1029 == 0 { 0u32 } else { (v965 || !steps(v1056, v1065)) as u32 };
                    let v1165 = v1161 != 0 && v1056 < v936;
                    q0 = if v1165 { v1030 } else { v938 };
                    q1 = if v1165 { v1035 } else { v937 };
                    q2 = if v1165 { v1029 } else { v935 };
                    q3 = if v1165 { v963 } else { v934 };
                    q4 = if v1165 { v963 } else { v933 };
                    q5 = if v1165 { v1056 } else { v936 };
                }
                let v932 = v931 + 1;
                if v932 == 9 {
                    (v802, v801, v799, v797, v795, v800) = (q0, q1, q2, q3, q4, q5);
                    break;
                }
                (v931, v933, v934, v935) = (v932, q4, q3, q2);
                (v936, v937, v938) = (q5, q1, q0);
            }
            (v794, v796, v798) = (v795, v797, v799);
            (v560, v562, v564) = (v800, v801, v802);
        } else {
            (v794, v796, v798) = (v306, v307, v308);
            (v560, v562, v564) = (v447, v314, v315);
        }
        let v804 = clamp01((((((((v352 - v362) - v373) - v384) - v396) - v409) - v419) - v430) + (v447 * 8.0)) * v302;
        let (g0, g1, g2, g3);
        if v804 > 0.00999999977648258209228515625 {
            let base = if (v798 & 2) == 0 { (v794 + 8) * 4 } else { v796 * 4 };
            let (v1173, v1174, v1175, v1176) =
                (PALETTE[base as usize], PALETTE[(1 + base) as usize], PALETTE[(2 + base) as usize], PALETTE[(3 + base) as usize]);
            let (v1177, v1178, v1179, v1180) = (v1173, v1174, v1175, v1176);
            let v1181 = v304 != 0;
            g0 = (if v1181 { v1175 * v528 } else { v1179 }) * 1.2000000476837158203125;
            g1 = clamp01(
                (((v302 * 1.2000000476837158203125) * ((v521 * 0.20000000298023223876953125) + 0.800000011920928955078125)) * v804)
                    * (if v1181 { v1176 * v528 } else { v1180 }),
            );
            g2 = (if v1181 { v1174 * v528 } else { v1178 }) * 1.2000000476837158203125;
            g3 = (if v1181 { v1173 * v528 } else { v1177 }) * 1.2000000476837158203125;
        } else {
            (g0, g1, g2, g3) = (0.0, 0.0, 0.0, 0.0);
        }
        (v559, v561, v563, v565, v567, v569, v571) = (v560, v562, v564, g3, g2, g0, g1);
    }
    let v583 = ((((((v314 as i32 as u32) as i32) as f32) + 0.5) * 2.0) / v145) + (-1.0);
    let v585 = (-0.0) - ((((((v315 as i32 as u32) as i32) as f32 + 0.5) * 2.0) / v146) + (-1.0));
    let v632 = (((k38.v[24].z + k38.v[23].z) + k38.v[22].z.mul_add(v585, k38.v[21].z * v583)) * v559) + k38.v[0].z;
    let v639 = (k38.v[0].x + (((k38.v[22].x.mul_add(v585, k38.v[21].x * v583) + k38.v[23].x) + k38.v[24].x) * v559)) - k33.v[36].x;
    let v640 = (k38.v[0].y + (((k38.v[24].y + k38.v[23].y) + k38.v[22].y.mul_add(v585, k38.v[21].y * v583)) * v559)) - k33.v[36].y;
    let v641 = v632 - k33.v[36].z;
    let v653 = ((((v639 * v639) + (v640 * v640)) + (v641 * v641)).sqrt() * 0.20000000298023223876953125) - (k28.v[0].z * 0.5);
    let v662 = clamp01(fmin(frac(v653) * 1.111111164093017578125, frac((-0.0) - v653) * 9.9999980926513671875));
    let v667 = (v662 * v662) * (3.0 - (v662 * 2.0));
    let (v673, v675, v677, v679);
    if v299 == 0 {
        (v673, v675, v677, v679) = (0.0, 0.0, 0.0, 0.0);
    } else {
        let v702 = if (v311 & 2) != 0 { v310 } else { 0 };
        let v674 = PALETTE[(v702 * 4) as usize] * 1.75;
        let v676 = PALETTE[(1 + v702 * 4) as usize] * 1.75;
        let v678 = PALETTE[(2 + v702 * 4) as usize] * 1.75;
        let rim = 1.0
            - ((1.0 - fmax(0.0, v453 * inverse_sqrt(dot3(Vec3::new(v451, v453, 1.0), Vec3::new(v451, v453, 1.0)))))
                * (1.0 - fmin(fmin(1.0, ((v453 * v453) + (v451 * v451)).sqrt()), 0.5)));
        let v764 = clamp01(
            1.0 - ((1.0 - (clamp01((v559 + (-30.0)) * 0.014285714365541934967041015625) * 0.5))
                * (1.0 - ((1.0 - (clamp01(v559 * 0.02500000037252902984619140625) * 0.699999988079071044921875)) * rim.log2()).exp2())),
        );
        let v769 = (v528 * 0.5) + 0.5;
        let strength = if v304 != 0 {
            clamp01(v764 * ((v528 * 1.2000000476837158203125) + (-0.20000000298023223876953125)))
        } else {
            fmax(clamp01(((load(t8, id.x, id.y) * (-64.0)).exp2() + load(t9, id.x, id.y)) * v769), v764 * v769)
                * (if v702 == 1 { 0.75 } else { 0.5 })
        };
        let v792 = ((v301 * v300) * ((v521 * 0.25) + 0.75)) * clamp01(strength);
        let (h0, h1, h2, h3);
        if v309 == 0 {
            let v901 = (k28.v[0].z * 3.0) + (v632 * 0.25);
            let v913 = clamp01((fmin(frac(v901) * 10.0, frac((-0.0) - v901) * 1.111111164093017578125) + (-0.60000002384185791015625)) * 2.5000002384185791015625);
            let v918 = flags(t16, id.x, id.y);
            let v921 = (v918 >> 2) & 7;
            let (v970, v973);
            if (v918 & 3) == 0 {
                (v970, v973) = (false, false);
            } else {
                let v1015 = distance(
                    load(t10, (k38.v[79].x * v314).ceil() as u32, (k38.v[79].y * v315).ceil() as u32),
                    k38,
                );
                let v971 = steps(distance(load(t11, id.x, id.y), k38), v1015);
                v970 = v971;
                v973 = (v918 & 64) != 0 || !v971;
            }
            let v1150 = if (v918 & 1) != 0 && v973 {
                let base = if (v918 & 2) == 0 { (v921 + 8) * 4 } else { v921 * 4 };
                let v1216 = PALETTE[base as usize];
                let v1217 = PALETTE[base as usize];
                (if v970 { v1216 } else { v1217 }) * 8.0
            } else {
                0.0
            };
            h0 = v674;
            h1 = (clamp01(v1150) + 1.0) * clamp01((((v913 * v913) * (v792 * 0.3499999940395355224609375)) * (3.0 - (v913 * 2.0))) + v792);
            h2 = v676;
            h3 = v678;
        } else {
            h0 = v674;
            h1 = v792 * (if v702 != 1 { (v667 * 0.5) + 0.5 } else { (v667 * 0.125) + 0.875 });
            h2 = v676;
            h3 = v678;
        }
        (v673, v675, v677, v679) = (h0, h2, h3, h1);
    }
    let v690 = ((v565 - v673) * v571) + v673;
    let v691 = ((v567 - v675) * v571) + v675;
    let v692 = ((v569 - v677) * v571) + v677;
    let v693 = ((v571 - v679) * v571) + v679;
    let (mut v885, mut v886, mut v887, mut v888);
    if v306 != 2 && k43.v[2].y > 0.0 {
        let v817: Vec4 = t17.fetch(IVec2::new((k38.v[79].x * v561).ceil() as i32, (k38.v[79].y * v563).ceil() as i32));
        let v821 = v817.x * v145;
        let v822 = v817.y * v146;
        let v832 = (k43.v[2].y * 0.800000011920928955078125) / (((((v821 * v821) + (v822 * v822)).sqrt()) * 0.0199999995529651641845703125) + 1.0);
        let v834 = (v299 | v298) != 0;
        let v836 = if v834 { 1.5 } else { 3.0 };
        let v837 = v836 * (v326 + 1.0);
        let v838 = v836 * v330;
        let v839 = if v834 { v832 * 0.75 } else { v832 };
        let v840 = v314 + 0.5;
        let v841 = v315 + 0.5;
        let v854: Vec4 = t18.sample_by_lod(*s46, Vec2::new(k43.v[1].z * ((v840 - v837) + v821), k43.v[1].w * ((v841 - v838) + v822)), 0.0);
        let v867: Vec4 = t18.sample_by_lod(*s46, Vec2::new(k43.v[1].z * ((v840 + v837) + v821), k43.v[1].w * ((v841 + v838) + v822)), 0.0);
        v885 = fmax(v690, fmax(v854.x, v867.x) * v839);
        v886 = fmax(v691, fmax(v854.y, v867.y) * v839);
        v887 = fmax(v692, fmax(v854.z, v867.z) * v839);
        v888 = fmax(v693, fmax(v854.w, v867.w) * v839);
    } else {
        (v885, v886, v887, v888) = (v690, v691, v692, v693);
    }
    // THE PORT'S OWN LINE, THICKER -- and only ours (white: palette entry 15, which the game never picks).
    if VRP_WHITE_GROW > 0 {
        let own = flags(t16, id.x, id.y);
        if (own & 1) == 0 {
            let mut white_near = 0.0f32;
            let mut gy = -VRP_WHITE_GROW;
            while gy <= VRP_WHITE_GROW {
                let mut gx = -VRP_WHITE_GROW;
                while gx <= VRP_WHITE_GROW {
                    // round, not square: a square neighbourhood thickens the diagonals half again as much
                    if !(gx == 0 && gy == 0) && gx * gx + gy * gy <= VRP_WHITE_GROW * VRP_WHITE_GROW {
                        let nx = fmin(fmax((id.x as i32 + gx) as f32, 0.0), v149) as i32 as u32;
                        let ny = fmin(fmax((id.y as i32 + gy) as f32, 0.0), v147) as i32 as u32;
                        let nf = flags(t16, nx, ny);
                        if (nf & 1) != 0 && (nf & 2) == 0 && ((nf >> 2) & 7) == 7 {
                            white_near = 1.0;
                        }
                    }
                    gx += 1;
                }
                gy += 1;
            }
            if white_near > 0.0 && VRP_GROW_ALPHA > v888 {
                (v885, v886, v887) = (1.0, 1.0, 1.0);
                v888 = VRP_GROW_ALPHA;
            }
        }
    }
    let at = UVec2::new(id.x, id.y);
    unsafe {
        u22.write(at, Vec4::new(v885, v886, v887, v888));
        u21.write(at, Vec4::new(v888 * v885, v888 * v886, v888 * v887, v888));
    }
}

/// HLSL's rsqrt (GLSL InverseSqrt).
#[cfg(target_arch = "spirv")]
fn inverse_sqrt(x: f32) -> f32 {
    let mut result = 0.0f32;
    unsafe {
        core::arch::asm! {
            "%glsl = OpExtInstImport \"GLSL.std.450\"",
            "%x = OpLoad _ {x}",
            "%result = OpExtInst typeof*{result} %glsl 32 %x",
            "OpStore {result} %result",
            x = in(reg) &x,
            result = in(reg) &mut result,
        }
    }
    result
}
#[cfg(not(target_arch = "spirv"))]
fn inverse_sqrt(x: f32) -> f32 {
    1.0 / x.sqrt()
}
