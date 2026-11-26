//! The port's shaders from Rust to DXIL (stage 8 of docs/rust-migration-plan.md), run by `cargo xtask shaders`:
//!
//!   1. ../passes (Rust, rust-gpu) -> one SPIR-V module per entry point;
//!   2. naga reads each (D3D12's conventions kept: no y flip) and validates it;
//!   3. naga writes HLSL with the pass's resources at the registers of its root signature (the table below), the
//!      samplers rewritten from naga's sampler heap to plain registers -- the passes bind static samplers;
//!   4. DXC compiles it to DXIL (validated: dxil.dll signs only a valid module), and the bindings the DXIL declares are
//!      checked against the table from DXC's own listing;
//!   5. the DXIL lands in ../crates/cvr-gpu/shaders/<name>.dxil with MANIFEST.txt (each file's size and stage, the
//!      tools' versions).
//!
//! `--check` builds into the target folder and compares with the committed files instead.
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A register of the pass's root signature.
#[derive(Clone, Copy)]
enum Reg {
    B(u32),
    T(u32),
    /// A table of `count` SRVs from the register on (an array of resources).
    TArray(u32, u32),
    /// An array of `count` SRVs at the register in a space of its own (register, space, count: the engine's bindless
    /// textures at t0, space1); the Rust shader binds it in descriptor set `space`.
    TSpace(u32, u32, u32),
    U(u32),
    S(u32),
}
impl Reg {
    fn number(self) -> u32 {
        match self {
            Reg::B(n) | Reg::T(n) | Reg::TArray(n, _) | Reg::TSpace(n, _, _) | Reg::U(n) | Reg::S(n) => n,
        }
    }
    /// As DXC's listing names it ("cb0", "t0", "s1").
    fn listed(self) -> String {
        match self {
            Reg::B(n) => format!("cb{n}"),
            Reg::T(n) | Reg::TArray(n, _) => format!("t{n}"),
            Reg::TSpace(n, space, _) => format!("t{n},space{space}"),
            Reg::U(n) => format!("u{n}"),
            Reg::S(n) => format!("s{n}"),
        }
    }
}

struct Shader {
    /// The rust-gpu entry point (module path :: function).
    entry: &'static str,
    /// The DXIL's file name.
    out: &'static str,
    /// (binding in descriptor set 0, its register): exactly what the DXIL must declare.
    bindings: &'static [(u32, Reg)],
}

const SHADERS: &[Shader] = &[
    // space_quad.rs: b0 the 28 root constants, t0 the picture, s0 the footprint (anisotropic), s1 linear
    Shader { entry: "space_quad::vs", out: "space_quad_vs", bindings: &[(0, Reg::B(0))] },
    Shader { entry: "space_quad::fill", out: "space_quad_fill", bindings: &[(0, Reg::B(0))] },
    Shader { entry: "space_quad::over", out: "space_quad_over", bindings: &[(0, Reg::B(0)), (1, Reg::T(0)), (2, Reg::S(0))] },
    Shader { entry: "space_quad::copy", out: "space_quad_copy", bindings: &[(1, Reg::T(0)), (3, Reg::S(1))] },
    Shader { entry: "space_quad::light", out: "space_quad_light", bindings: &[(0, Reg::B(0)), (1, Reg::T(0)), (2, Reg::S(0))] },
    // srgb_pass.rs: b0 one root constant, t0
    Shader { entry: "fullscreen::srgb_vs", out: "srgb_vs", bindings: &[] },
    Shader { entry: "fullscreen::srgb_ps", out: "srgb_ps", bindings: &[(0, Reg::B(0)), (1, Reg::T(0))] },
    // sharpen_pass.rs (b0 four root constants, t0, s0 point) and depth_resolve.rs (t0, s0 point): one vertex shader
    Shader { entry: "fullscreen::uv_vs", out: "fullscreen_uv_vs", bindings: &[] },
    Shader { entry: "fullscreen::sharpen_ps", out: "sharpen_ps", bindings: &[(0, Reg::B(0)), (1, Reg::T(0)), (2, Reg::S(0))] },
    Shader { entry: "fullscreen::depth_resolve_ps", out: "depth_resolve_ps", bindings: &[(1, Reg::T(0)), (2, Reg::S(0))] },
    // group_resolve.rs: b0 the eye, t0-t2 the colour slices, t3 the depth, t4 the stencil
    Shader { entry: "group_resolve::vs", out: "group_resolve_vs", bindings: &[] },
    Shader {
        entry: "group_resolve::ps",
        out: "group_resolve_ps",
        bindings: &[(0, Reg::B(0)), (1, Reg::T(0)), (2, Reg::T(1)), (3, Reg::T(2)), (4, Reg::T(3)), (5, Reg::T(4))],
    },
    Shader { entry: "group_resolve::depth_ps", out: "group_resolve_depth_ps", bindings: &[(0, Reg::B(0)), (4, Reg::T(3)), (5, Reg::T(4))] },
    // color_blit.rs: the quad; the blit (b0 the uv rect, t0, s0 linear); the overlay (b0 tint, b1 filter, t0, s0); the
    // HUD composite (b0 its 33 constants, b1 the engine's frame CB, b2 its composite CB, t0 scene, t1 HUD, t2 the HUD
    // pyramid, t3 the exposure buffer, s0); the barrel dot (b0)
    Shader { entry: "color_blit::vs", out: "color_blit_vs", bindings: &[] },
    Shader { entry: "color_blit::blit_ps", out: "color_blit_ps", bindings: &[(0, Reg::B(0)), (2, Reg::T(0)), (3, Reg::S(0))] },
    Shader { entry: "color_blit::overlay_ps", out: "color_blit_overlay_ps", bindings: &[(0, Reg::B(0)), (1, Reg::B(1)), (2, Reg::T(0)), (3, Reg::S(0))] },
    Shader {
        entry: "color_blit::hud_ps",
        out: "color_blit_hud_ps",
        bindings: &[
            (0, Reg::B(0)),
            (1, Reg::B(1)),
            (4, Reg::B(2)),
            (5, Reg::T(0)),
            (6, Reg::T(1)),
            (7, Reg::T(2)),
            (8, Reg::T(3)),
            (3, Reg::S(0)),
        ],
    },
    Shader { entry: "color_blit::dot_vs", out: "color_blit_dot_vs", bindings: &[(0, Reg::B(0))] },
    Shader { entry: "color_blit::dot_ps", out: "color_blit_dot_ps", bindings: &[(0, Reg::B(0))] },
    // foveated_gpu.rs: b0 the constants, t0-t4 Color, Depth, Motion, Center, Peripheral, u0-u3 ColorOut, DepthOut,
    // MotionOut, Output, s0 linear clamp
    Shader {
        entry: "foveated::prepare",
        out: "foveated_prepare_cs",
        bindings: &[
            (0, Reg::B(0)),
            (1, Reg::T(0)),
            (2, Reg::T(1)),
            (3, Reg::T(2)),
            (6, Reg::U(0)),
            (7, Reg::U(1)),
            (8, Reg::U(2)),
            (10, Reg::S(0)),
        ],
    },
    Shader {
        entry: "foveated::composite",
        out: "foveated_composite_cs",
        bindings: &[(0, Reg::B(0)), (4, Reg::T(3)), (5, Reg::T(4)), (9, Reg::U(3)), (10, Reg::S(0))],
    },
    // stereo_depth.rs: b0 the parameters, t0 the source depth, t1 B's motion vectors (a midpoint's), u0 the output
    Shader { entry: "stereo_depth::main", out: "stereo_depth_cs", bindings: &[(0, Reg::B(0)), (1, Reg::T(0)), (2, Reg::T(1)), (3, Reg::U(0))] },
    // depth_commit_gate.rs: DEPTH_SHADER (t0 Depth, t1 Stencil, u0 Decision); BUFFER_SHADER (b0, t0 Main, t1 Peer, u0)
    Shader { entry: "single_pass::depth_gate", out: "depth_commit_gate_depth_cs", bindings: &[(1, Reg::T(0)), (2, Reg::T(1)), (3, Reg::U(0))] },
    Shader {
        entry: "single_pass::buffer_gate",
        out: "depth_commit_gate_buffer_cs",
        bindings: &[(0, Reg::B(0)), (3, Reg::U(0)), (4, Reg::T(0)), (5, Reg::T(1))],
    },
    // geometry_compare.rs: b0 the batch, t0 left, t1 right, t2 permission, t3 ranges, u0 results
    Shader {
        entry: "single_pass::geometry_initialize",
        out: "geometry_compare_initialize_cs",
        bindings: &[(0, Reg::B(0)), (3, Reg::T(2)), (5, Reg::U(0))],
    },
    Shader {
        entry: "single_pass::geometry_compare",
        out: "geometry_compare_cs",
        bindings: &[(0, Reg::B(0)), (1, Reg::T(0)), (2, Reg::T(1)), (3, Reg::T(2)), (4, Reg::T(3)), (5, Reg::U(0))],
    },
    // multilayer_membership.rs (cvr-render): b0, t0 b4, t1 t5, t2 membership, t3 producer, t4 metadata, u0 predicates,
    // u1 results
    Shader {
        entry: "single_pass::membership_check",
        out: "multilayer_membership_cs",
        bindings: &[
            (0, Reg::B(0)),
            (1, Reg::T(0)),
            (2, Reg::T(1)),
            (3, Reg::T(2)),
            (4, Reg::T(3)),
            (5, Reg::T(4)),
            (6, Reg::U(0)),
            (7, Reg::U(1)),
        ],
    },
    // finer_vrs.rs: b0 the parameters, t0..t511 the inputs, u0 the output rate, u1 the status (a root UAV)
    Shader { entry: "finer_vrs::initialize", out: "finer_vrs_initialize_cs", bindings: &[(3, Reg::U(1))] },
    Shader {
        entry: "finer_vrs::reduce",
        out: "finer_vrs_reduce_cs",
        bindings: &[(0, Reg::B(0)), (1, Reg::TArray(0, 512)), (2, Reg::U(0)), (3, Reg::U(1))],
    },
    // osr.rs: b0 the constants (a root CBV); t0..t5 SrcColor, SrcDepth, HistColor, HistDepth, ResColor, ResDepth;
    // u0..u5 StereoT, HistT, OutColor, OutDepth, Final, NearD; s0 the linear clamp sampler
    Shader {
        entry: "osr::clear",
        out: "osr_clear_cs",
        bindings: &[(0, Reg::B(0)), (2, Reg::T(1)), (7, Reg::U(0)), (8, Reg::U(1)), (12, Reg::U(5))],
    },
    Shader {
        entry: "osr::splat",
        out: "osr_splat_cs",
        bindings: &[(0, Reg::B(0)), (2, Reg::T(1)), (4, Reg::T(3)), (7, Reg::U(0)), (8, Reg::U(1))],
    },
    Shader {
        entry: "osr::resolve",
        out: "osr_resolve_cs",
        bindings: &[
            (0, Reg::B(0)),
            (1, Reg::T(0)),
            (2, Reg::T(1)),
            (3, Reg::T(2)),
            (4, Reg::T(3)),
            (7, Reg::U(0)),
            (8, Reg::U(1)),
            (9, Reg::U(2)),
            (10, Reg::U(3)),
            (12, Reg::U(5)),
            (13, Reg::S(0)),
        ],
    },
    Shader {
        entry: "osr::inpaint",
        out: "osr_inpaint_cs",
        bindings: &[(0, Reg::B(0)), (1, Reg::T(0)), (5, Reg::T(4)), (6, Reg::T(5)), (11, Reg::U(4)), (13, Reg::S(0))],
    },
    // framegen.rs: b0 the parameters (6 root constants); t0..t2, u0..u2 (one table each); s0 linear clamp
    Shader {
        entry: "framegen::pack",
        out: "framegen_pack_cs",
        bindings: &[(0, Reg::B(0)), (1, Reg::T(0)), (4, Reg::U(0)), (7, Reg::S(0))],
    },
    Shader {
        entry: "framegen::flow",
        out: "framegen_flow_cs",
        bindings: &[(0, Reg::B(0)), (2, Reg::T(1)), (5, Reg::U(1)), (6, Reg::U(2))],
    },
    Shader {
        entry: "framegen::scene_cut",
        out: "framegen_scene_cut_cs",
        bindings: &[(0, Reg::B(0)), (1, Reg::T(0)), (3, Reg::T(2)), (6, Reg::U(2)), (7, Reg::S(0))],
    },
    Shader {
        entry: "framegen::prepare_inputs",
        out: "framegen_prepare_inputs_cs",
        bindings: &[(0, Reg::B(0)), (1, Reg::T(0)), (2, Reg::T(1)), (4, Reg::U(0)), (5, Reg::U(1))],
    },
    // fsr3.rs (main's 2e123a4a3): dilate -- b0 the parameters (6 root constants), t0 depth, t1 motion, u0 dilated depth,
    // u1 dilated motion, u2 the scatter's scratch (InterlockedMax), u3 the previous depth, u4 the scratch again
    // (InterlockedMin); warp -- b0 (5), t0 colour, t1 motion, u0, s0
    Shader { entry: "fsr3::dilate_clear", out: "fsr3_dilate_clear_cs", bindings: &[(0, Reg::B(0)), (5, Reg::U(2))] },
    Shader {
        entry: "fsr3::dilate",
        out: "fsr3_dilate_cs",
        bindings: &[(0, Reg::B(0)), (1, Reg::T(0)), (2, Reg::T(1)), (3, Reg::U(0)), (4, Reg::U(1)), (5, Reg::U(2)), (7, Reg::U(4))],
    },
    Shader { entry: "fsr3::dilate_resolve", out: "fsr3_dilate_resolve_cs", bindings: &[(0, Reg::B(0)), (5, Reg::U(2)), (6, Reg::U(3))] },
    Shader {
        entry: "fsr3::warp",
        out: "fsr3_warp_cs",
        bindings: &[(0, Reg::B(0)), (1, Reg::T(0)), (2, Reg::T(1)), (3, Reg::U(0)), (4, Reg::S(0))],
    },
    // neural_render.rs (main's 2e123a4a3): b0 the parameters (6 root constants), t0 the source, u0 the destination
    Shader { entry: "neural_render::encode", out: "nr_encode_cs", bindings: &[(0, Reg::B(0)), (1, Reg::T(0)), (2, Reg::U(0))] },
    Shader { entry: "neural_render::decode", out: "nr_decode_cs", bindings: &[(0, Reg::B(0)), (1, Reg::T(0)), (2, Reg::U(0))] },
    // mirror.rs: b0 the time (one root constant), t0 the mirror texture (s0 static, unused by the pass)
    Shader { entry: "mirror::vs", out: "mirror_vs", bindings: &[] },
    Shader { entry: "mirror::ps", out: "mirror_ps", bindings: &[(0, Reg::B(0)), (1, Reg::T(0))] },
    // optics.rs: the engine's constant buffers at its registers (b0 globals, b1 camera, b4 material / sight constants)
    Shader { entry: "optics::lens_mask", out: "lens_mask_ps", bindings: &[(0, Reg::B(0)), (4, Reg::B(4))] },
    Shader { entry: "optics::lens_velocity", out: "lens_velocity_ps", bindings: &[(0, Reg::B(0)), (1, Reg::B(1)), (4, Reg::B(4))] },
    Shader { entry: "optics::sight_mask", out: "sight_mask_ps", bindings: &[(4, Reg::B(4))] },
    // optics_resolve.rs: b0 20 root constants, t0..t1 and u0 one table, s0 static (the root signature embedded)
    Shader {
        entry: "optics_resolve::lens",
        out: "lens_source_resolve_cs",
        bindings: &[(0, Reg::B(0)), (1, Reg::T(0)), (2, Reg::T(1)), (3, Reg::T(2)), (4, Reg::U(0)), (5, Reg::U(1)), (6, Reg::S(0))],
    },
    Shader {
        entry: "optics_resolve::camera",
        out: "camera_source_resolve_cs",
        bindings: &[(0, Reg::B(0)), (1, Reg::T(0)), (2, Reg::T(1)), (3, Reg::U(0)), (4, Reg::S(0))],
    },
    // world_markers.rs: the engine's constant buffers at its registers (b1 camera, b5 object, b7 material)
    Shader { entry: "world_markers::sprite_main", out: "world_marker_sprite_vs", bindings: &[(1, Reg::B(1)), (5, Reg::B(5))] },
    Shader {
        entry: "world_markers::procedural_main",
        out: "world_marker_procedural_vs",
        bindings: &[(1, Reg::B(1)), (5, Reg::B(5)), (7, Reg::B(7))],
    },
    Shader {
        entry: "world_markers::sprite_stereo_main",
        out: "world_marker_sprite_stereo_vs",
        bindings: &[(1, Reg::B(1)), (5, Reg::B(5)), (7, Reg::B(7))],
    },
    Shader {
        entry: "world_markers::text_stereo_main",
        out: "world_marker_text_stereo_vs",
        bindings: &[(1, Reg::B(1)), (5, Reg::B(5)), (7, Reg::B(7))],
    },
    Shader {
        entry: "world_markers::sprite_stereo_z_main",
        out: "world_marker_sprite_stereo_z_vs",
        bindings: &[(1, Reg::B(1)), (5, Reg::B(5)), (7, Reg::B(7))],
    },
    Shader {
        entry: "world_markers::sprite_stereo_one_main",
        out: "world_marker_sprite_stereo_one_vs",
        bindings: &[(1, Reg::B(1)), (5, Reg::B(5)), (7, Reg::B(7))],
    },
    // vision.rs: the engine's VisionMode_Highlight registers (b0, b1, b6, b12; t0..t2, t4..t8; u0, u1; s11)
    Shader {
        entry: "vision::main",
        out: "vision_highlight_cs",
        bindings: &[
            (0, Reg::B(0)),
            (1, Reg::B(1)),
            (6, Reg::B(6)),
            (12, Reg::B(12)),
            (20, Reg::T(0)),
            (21, Reg::T(1)),
            (22, Reg::T(2)),
            (24, Reg::T(4)),
            (25, Reg::T(5)),
            (26, Reg::T(6)),
            (27, Reg::T(7)),
            (28, Reg::T(8)),
            (40, Reg::U(0)),
            (41, Reg::U(1)),
            (50, Reg::S(11)),
        ],
    },
    // sight.rs: the engine's reticle registers (b7, b0, b1, b5; the skinning buffer t10)
    Shader {
        entry: "sight::reflex_vs",
        out: "sight_reflex_vs",
        bindings: &[(7, Reg::B(7)), (0, Reg::B(0)), (1, Reg::B(1)), (5, Reg::B(5)), (10, Reg::T(10))],
    },
    // sight_ps.rs: b0 globals, b4 the sight's constants, t0 the reticle atlas, s0; the bindless noise maps at t0, space1
    Shader {
        entry: "sight_ps::reflex_ps",
        out: "sight_reflex_ps",
        bindings: &[(0, Reg::B(0)), (4, Reg::B(4)), (1, Reg::T(0)), (0, Reg::TSpace(0, 1, 32768)), (2, Reg::S(0))],
    },
    Shader {
        entry: "sight_ps::reflex_ps_scope",
        out: "sight_reflex_ps_scope",
        bindings: &[(0, Reg::B(0)), (4, Reg::B(4)), (1, Reg::T(0)), (0, Reg::TSpace(0, 1, 32768)), (2, Reg::S(0))],
    },
];

/// The glass pass's input signature: the native glass PS's, SV_Position and TEXCOORD0..6.
const GLASS_INPUT: &[(&str, &str)] = &[
    ("SV_Position", "float4"),
    ("TEXCOORD0", "float4"),
    ("TEXCOORD1", "float4"),
    ("TEXCOORD2", "float4"),
    ("TEXCOORD3", "float4"),
    ("TEXCOORD4", "float4"),
    ("TEXCOORD5", "float4"),
    ("TEXCOORD6", "float2"),
];
/// The sight pass's input signature: the native sight PS's, SV_Position and TEXCOORD0..4.
const SIGHT_INPUT: &[(&str, &str)] = &[
    ("SV_Position", "float4"),
    ("TEXCOORD0", "float4"),
    ("TEXCOORD1", "float4"),
    ("TEXCOORD2", "float4"),
    ("TEXCOORD3", "float4"),
    ("TEXCOORD4", "float4"),
];
/// The pixel shaders drawn with the engine's vertex shaders: their input struct written as the engine's signature
/// (entry point, the elements in order: semantic, HLSL type). naga names a location LOCn (TEXCOORDn here) and leaves out
/// what the shader does not read; the engine's VS output and this PS's input must match element for element.
const SIGNATURES: &[(&str, &[(&str, &str)])] = &[
    ("sight_ps::reflex_ps", SIGHT_INPUT),
    ("sight_ps::reflex_ps_scope", SIGHT_INPUT),
    ("optics::lens_mask", GLASS_INPUT),
    ("optics::lens_velocity", GLASS_INPUT),
    ("optics::sight_mask", SIGHT_INPUT),
    // the mirror's own pair: the pixel shader reads the position only, naga would drop the uv and the registers
    // would no longer meet the vertex shader's (D3D12 refuses the pipeline: found by the bench)
    ("mirror::ps", MIRROR_SIGNATURE),
];
/// The mirror's conversion pair: SV_Position, TEXCOORD0 (float2), as its HLSL had them.
const MIRROR_SIGNATURE: &[(&str, &str)] = &[("SV_Position", "float4"), ("TEXCOORD0", "float2")];

/// The fragment entry point `name`'s input struct in `hlsl` written as `signature`: naga's members kept (their names, a
/// location's semantic made TEXCOORDn, the type checked), the elements it left out declared unread, in the signature's
/// order; an entry point naga gave no input takes the struct as its parameter.
fn apply_signature(hlsl: &str, name: &str, signature: &[(&str, &str)]) -> Result<String, String> {
    let header = format!("struct FragmentInput_{name} {{\n");
    let mut members: Vec<(String, String, String)> = Vec::new(); // (type, name, semantic)
    let mut out = hlsl.to_string();
    if let Some(start) = hlsl.find(&header) {
        let end = start + hlsl[start..].find("};\n").ok_or("the input struct does not end")? + 3;
        for line in hlsl[start + header.len()..end - 3].lines() {
            let line = line.trim().trim_end_matches(';');
            let (decl, semantic) = line.split_once(" : ").ok_or_else(|| format!("an input member without a semantic: {line}"))?;
            let mut words = decl.split_whitespace().collect::<Vec<_>>();
            let member = words.pop().ok_or("an empty member")?.to_string();
            let ty = words.pop().ok_or("a member without a type")?.to_string();
            if !words.is_empty() {
                return Err(format!("an input member with a modifier ({line}): not in the engine's signature"));
            }
            let semantic = match semantic.strip_prefix("LOC") {
                Some(n) => format!("TEXCOORD{n}"),
                None => semantic.to_string(),
            };
            members.push((ty, member, semantic));
        }
        let mut body = header.clone();
        for (i, &(semantic, ty)) in signature.iter().enumerate() {
            match members.iter().find(|m| m.2 == semantic) {
                Some(m) if m.0 == ty => body.push_str(&format!("    {} {} : {};\n", m.0, m.1, semantic)),
                Some(m) => return Err(format!("{semantic}: naga declares {}, the engine's signature {ty}", m.0)),
                None => body.push_str(&format!("    {ty} unread_{i} : {semantic};\n")),
            }
        }
        if let Some(m) = members.iter().find(|m| !signature.iter().any(|s| s.0 == m.2)) {
            return Err(format!("{}: read by the shader, not in the engine's signature", m.2));
        }
        body.push_str("};\n");
        out.replace_range(start..end, &body);
    } else {
        let mut body = header.clone();
        for (i, &(semantic, ty)) in signature.iter().enumerate() {
            body.push_str(&format!("    {ty} unread_{i} : {semantic};\n"));
        }
        body.push_str("};\n\n");
        let entry = format!(" {name}()");
        let at = out.find(&entry).ok_or_else(|| format!("the entry point {name} without parameters not found"))?;
        out.replace_range(at..at + entry.len(), &format!(" {name}(FragmentInput_{name} unread)"));
        let first = out.find(&format!("{name}(FragmentInput_")).unwrap();
        let line_start = out[..first].rfind("\n").map_or(0, |i| i + 1);
        out.insert_str(line_start, &body);
    }
    Ok(out)
}

/// The world markers' input: the engine's ink vertex layout, by location (POSITION, TEXCOORD, COLOR).
const INK_INPUT: &[(&str, &str)] = &[("POSITION0", "float3"), ("TEXCOORD0", "float2"), ("COLOR0", "float4")];
/// The procedural quad's: the ink layout and the vertex id.
const INK_INPUT_ID: &[(&str, &str)] = &[("POSITION0", "float3"), ("TEXCOORD0", "float2"), ("COLOR0", "float4"), ("SV_VertexID", "uint")];
/// The reticle quad's input: the engine's 13 elements, POSITION0 .. SV_VertexID0 (by location, the system value last).
const SIGHT_VS_INPUT: &[(&str, &str)] = &[
    ("POSITION0", "float3"),
    ("BLENDINDICES0", "uint4"),
    ("BLENDWEIGHT0", "float4"),
    ("TEXCOORD0", "float2"),
    ("NORMAL0", "float3"),
    ("TANGENT0", "float4"),
    ("COLOR0", "float4"),
    ("TEXCOORD1", "float2"),
    ("INSTANCE_TRANSFORM0", "float4"),
    ("INSTANCE_TRANSFORM1", "float4"),
    ("INSTANCE_TRANSFORM2", "float4"),
    ("INSTANCE_SKINNING_DATA0", "uint4"),
    ("SV_VertexID", "uint"),
];
/// The reticle quad's output: the sight PS's input and the clip distance.
const SIGHT_VS_OUTPUT: &[(&str, &str)] = &[
    ("SV_Position", "float4"),
    ("TEXCOORD0", "float4"),
    ("TEXCOORD1", "float4"),
    ("TEXCOORD2", "float4"),
    ("TEXCOORD3", "float4"),
    ("TEXCOORD4", "float4"),
    ("SV_ClipDistance", "float"),
];
/// The world markers' sprite output: the ink pixel shaders' input, SV_Position, TEXCOORD0 (float4), TEXCOORD1 (float3).
const SPRITE_OUTPUT: &[(&str, &str)] = &[("SV_Position", "float4"), ("TEXCOORD0", "float4"), ("TEXCOORD1", "float3")];
/// The procedural quad's output: SV_Position, TEXCOORD0, TEXCOORD1 (float4 each).
const PROCEDURAL_OUTPUT: &[(&str, &str)] = &[("SV_Position", "float4"), ("TEXCOORD0", "float4"), ("TEXCOORD1", "float4")];
/// The vertex shaders drawn with the engine's input layouts and pixel shaders: (entry point, the input semantic of each
/// location, the output elements in order). naga names a location LOCn and puts the position last; the engine's
/// layout finds its elements by semantic, the engine's pixel shader its inputs by register.
const VERTEX_SIGNATURES: &[(&str, &[(&str, &str)], &[(&str, &str)])] = &[
    ("mirror::vs", &[("SV_VertexID", "uint")], MIRROR_SIGNATURE),
    ("world_markers::sprite_main", INK_INPUT, SPRITE_OUTPUT),
    ("world_markers::procedural_main", INK_INPUT_ID, PROCEDURAL_OUTPUT),
    ("sight::reflex_vs", SIGHT_VS_INPUT, SIGHT_VS_OUTPUT),
    ("world_markers::sprite_stereo_main", INK_INPUT, SPRITE_OUTPUT),
    ("world_markers::text_stereo_main", INK_INPUT, SPRITE_OUTPUT),
    ("world_markers::sprite_stereo_z_main", INK_INPUT, SPRITE_OUTPUT),
    ("world_markers::sprite_stereo_one_main", INK_INPUT, SPRITE_OUTPUT),
];

/// A struct's members in `hlsl` from `header` on: (start, end, [(type, member, semantic)]).
fn struct_members(hlsl: &str, header: &str) -> Result<(usize, usize, Vec<(String, String, String)>), String> {
    let start = hlsl.find(header).ok_or_else(|| format!("{} not found", header.trim()))?;
    let end = start + hlsl[start..].find("};\n").ok_or("the struct does not end")? + 3;
    let mut members = Vec::new();
    for line in hlsl[start + header.len()..end - 3].lines() {
        let line = line.trim().trim_end_matches(';');
        let (decl, semantic) = line.split_once(" : ").ok_or_else(|| format!("a member without a semantic: {line}"))?;
        let words = decl.split_whitespace().collect::<Vec<_>>();
        let [ty, member] = words.as_slice() else {
            return Err(format!("a member with a modifier ({line})"));
        };
        members.push((ty.to_string(), member.to_string(), semantic.to_string()));
    }
    Ok((start, end, members))
}

/// The vertex entry point `name`'s input locations given the engine's semantics, and its output struct (and the
/// initializer naga returns it with) put in the engine's order with TEXCOORDn for LOCn.
fn apply_vertex_signature(hlsl: &str, name: &str, inputs: &[(&str, &str)], outputs: &[(&str, &str)]) -> Result<String, String> {
    let mut out = hlsl.to_string();
    // the inputs: naga's entry point takes them as parameters (`float3 param : LOC0, ...`), leaving out the unread;
    // written in the engine's order, a location by its index, a system value by its name, the unread declared
    let entry = out.find(&format!(" {name}(")).ok_or_else(|| format!("the entry point {name} not found"))? + name.len() + 2;
    let close = entry + out[entry..].find(')').ok_or("the entry point's parameters do not end")?;
    let mut given: Vec<(String, String, String, bool)> = Vec::new(); // (type, name, semantic, taken)
    for p in out[entry..close].split(", ").filter(|p| !p.trim().is_empty()) {
        let (decl, semantic) = p.split_once(" : ").ok_or_else(|| format!("a parameter without a semantic: {p}"))?;
        let (ty, member) = decl.trim().rsplit_once(' ').ok_or_else(|| format!("a parameter without a type: {p}"))?;
        given.push((ty.to_string(), member.to_string(), semantic.trim().to_string(), false));
    }
    let mut parameters = Vec::new();
    for (i, &(semantic, ty)) in inputs.iter().enumerate() {
        let naga = if semantic.starts_with("SV_") { semantic.to_string() } else { format!("LOC{i}") };
        match given.iter_mut().find(|g| g.2 == naga) {
            Some(g) => {
                if g.0 != ty {
                    return Err(format!("{semantic}: naga declares {}, the engine's layout {ty}", g.0));
                }
                g.3 = true;
                parameters.push(format!("{} {} : {semantic}", g.0, g.1));
            }
            None => parameters.push(format!("{ty} unread_{i} : {semantic}")),
        }
    }
    if let Some(g) = given.iter().find(|g| !g.3) {
        return Err(format!("{}: read by the shader, not in the engine's layout", g.2));
    }
    out.replace_range(entry..close, &parameters.join(", "));
    let header = format!("struct VertexOutput_{name} {{\n");
    let (start, end, members) = struct_members(&out, &header)?;
    let named: Vec<(String, String, String)> = members
        .into_iter()
        .map(|(ty, member, semantic)| {
            let semantic = semantic.strip_prefix("LOC").map_or(semantic.clone(), |n| format!("TEXCOORD{n}"));
            (ty, member, semantic)
        })
        .collect();
    if named.len() != outputs.len() {
        return Err(format!("{} outputs, the engine's signature has {}", named.len(), outputs.len()));
    }
    let mut order = Vec::new();
    let mut body = header.clone();
    for &(semantic, ty) in outputs {
        let m = named.iter().find(|m| m.2 == semantic).ok_or_else(|| format!("{semantic} not written"))?;
        if m.0 != ty {
            return Err(format!("{semantic}: naga declares {}, the engine's signature {ty}", m.0));
        }
        body.push_str(&format!("    {} {} : {};\n", m.0, m.1, semantic));
        order.push(m.1.clone());
    }
    body.push_str("};\n");
    out.replace_range(start..end, &body);
    // the initializer: `const VertexOutput_<name> <v> = { <a>.member_1, <a>.member };`
    let init = format!("const VertexOutput_{name} ");
    let at = out.find(&init).ok_or("the output's initializer not found")?;
    let open = at + out[at..].find("= { ").ok_or("the initializer's brace")? + 4;
    let close = open + out[open..].find(" };").ok_or("the initializer's end")?;
    let elements: Vec<String> = out[open..close].split(", ").map(str::to_string).collect();
    let source = elements.first().and_then(|e| e.split_once('.')).map(|(v, _)| v.to_string()).ok_or("an empty initializer")?;
    let mut list = Vec::new();
    for member in &order {
        let element = format!("{source}.{member}");
        if !elements.contains(&element) {
            return Err(format!("the initializer does not set {member}"));
        }
        list.push(element);
    }
    out.replace_range(open..close, &list.join(", "));
    Ok(out)
}

/// The shaders DXC compiled before (src/Shaders, src/Render/WorldMarkers.hlsl) compiled IEEE-strict (`-Gis`): DXC's
/// default is fast math, whose reassociation follows the IR's shape -- the same arithmetic written by naga came out
/// one rounding apart from the HLSL's (the camera resolve: one half-float step on one texel). Strict, each operation
/// is rounded as written, so the shader's bits are the HLSL's own semantics (the bench compares them with the HLSL
/// compiled `-Gis` too, and reports the distance from the fast-math DXIL shipped before).
const IEEE_STRICT: &[&str] = &[
    "optics_resolve::camera",
    "optics_resolve::lens",
    "world_markers::sprite_main",
    "world_markers::procedural_main",
    "world_markers::sprite_stereo_main",
    "world_markers::text_stereo_main",
    "world_markers::sprite_stereo_z_main",
    "world_markers::sprite_stereo_one_main",
    "vision::main",
    "sight::reflex_vs",
    "sight_ps::reflex_ps",
    "sight_ps::reflex_ps_scope",
];

/// The typed buffers a shader reads (entry point, register, the HLSL type): written in Rust as a read-only storage
/// buffer of u32 (naga has no texel buffers: a ByteAddressBuffer), its declaration made the engine's typed view and each
/// `Load(i*4+0)` of a dword element `i` the view's `Load(i).x` -- the sight's skinning matrices, read as raw dwords from
/// a Buffer<uint4> over R32_UINT exactly as the engine's shader does.
const TYPED_BUFFERS: &[(&str, &str, &str)] = &[("sight::reflex_vs", "t10", "Buffer<uint4>")];

/// The buffer at `register` in `hlsl` made `ty`, its loads by dword index.
fn typed_buffer(hlsl: &str, register: &str, ty: &str) -> Result<String, String> {
    let marker = format!(" : register({register});");
    let at = hlsl.find(&marker).ok_or_else(|| format!("no resource at {register}"))?;
    let line = hlsl[..at].rfind('\n').map_or(0, |i| i + 1);
    let decl = &hlsl[line..at];
    let name = decl.strip_prefix("ByteAddressBuffer ").ok_or_else(|| format!("{register} is not a ByteAddressBuffer: {decl}"))?.to_string();
    let mut out = String::with_capacity(hlsl.len());
    out.push_str(&hlsl[..line]);
    out.push_str(&format!("{ty} {name}"));
    let mut rest = &hlsl[at..];
    let call = format!("{name}.Load(");
    let mut loads = 0;
    while let Some(i) = rest.find(&call) {
        out.push_str(&rest[..i + call.len()]);
        rest = &rest[i + call.len()..];
        // the argument, to the matching parenthesis
        let mut depth = 1;
        let end = rest
            .char_indices()
            .find(|&(_, c)| {
                depth += match c {
                    '(' => 1,
                    ')' => -1,
                    _ => 0,
                };
                depth == 0
            })
            .map(|(i, _)| i)
            .ok_or("a load without its end")?;
        let argument = rest[..end].strip_suffix("*4+0").ok_or_else(|| format!("a load not of a dword element: {}", &rest[..end]))?;
        out.push_str(argument);
        out.push_str(").x");
        rest = &rest[end + 1..];
        loads += 1;
    }
    out.push_str(rest);
    if loads == 0 {
        return Err(format!("no load from {register}"));
    }
    Ok(out)
}

/// The root signatures embedded in a shader's DXIL (the plugin makes the signature from the shader's bytes): entry point,
/// the signature in HLSL's root signature language -- the one the HLSL declared with [RootSignature(...)].
const ROOT_SIGNATURES: &[(&str, &str)] = &[
    (
        "optics_resolve::lens",
        "RootConstants(num32BitConstants=11, b0),\
         DescriptorTable(SRV(t0, numDescriptors=3), UAV(u0, numDescriptors=2)),\
         StaticSampler(s0, filter=FILTER_MIN_MAG_MIP_LINEAR, addressU=TEXTURE_ADDRESS_CLAMP, addressV=TEXTURE_ADDRESS_CLAMP)",
    ),
    (
    "optics_resolve::camera",
    "RootConstants(num32BitConstants=20, b0),\
     DescriptorTable(SRV(t0, numDescriptors=2), UAV(u0, numDescriptors=1)),\
     StaticSampler(s0, filter=FILTER_MIN_MAG_MIP_LINEAR, addressU=TEXTURE_ADDRESS_CLAMP, addressV=TEXTURE_ADDRESS_CLAMP)",
    ),
];

/// `[RootSignature("...")]` set before the entry point `name`'s attributes in `hlsl`.
fn embed_root_signature(hlsl: &str, name: &str, signature: &str) -> Result<String, String> {
    let entry = hlsl.find(&format!(" {name}(")).ok_or_else(|| format!("the entry point {name} not found"))?;
    let line = hlsl[..entry].rfind('\n').map_or(0, |i| i + 1);
    // the attributes on the lines before the entry's (naga writes [numthreads(...)] on its own line)
    let mut at = line;
    while let Some(previous) = hlsl[..at.saturating_sub(1)].rfind('\n').map(|i| i + 1).or(if at > 0 { Some(0) } else { None }) {
        if !hlsl[previous..at].trim_start().starts_with('[') {
            break;
        }
        at = previous;
        if at == 0 {
            break;
        }
    }
    let mut out = hlsl.to_string();
    out.insert_str(at, &format!("[RootSignature(\"{}\")]\n", signature.split_whitespace().collect::<Vec<_>>().join(" ")));
    Ok(out)
}

/// The image stores the builder turns into InterlockedMax: (entry point, its bindings). Every store to these storage
/// images in that entry point is the atomic (spirv-std has no image atomic; rust-gpu's inline SPIR-V gives an
/// OpImageTexelPointer's result the Function storage class, which its own validation refuses) -- osr::splat's
/// StereoT and HistT, the nearest depth kept per cell.
const ATOMIC_MAX_STORES: &[(&str, &[u32])] = &[("osr::splat", &[7, 8]), ("fsr3::dilate", &[5])];
/// The same, made InterlockedMin -- fsr3::dilate's scatter for a depth that is not inverted (main's
/// Fsr3Dilate.cpp: the nearest is the smallest), the scratch bound a second time at u4.
const ATOMIC_MIN_STORES: &[(&str, &[u32])] = &[("fsr3::dilate", &[7])];

/// The entry points whose HLSL DXC compiles as HLSL 2018 (as wgpu does naga's): naga writes a vector condition of ?:
/// as a select, which 2018 takes and 2021 refuses. The others stay 2021, DXC's default: there naga's scalar ?: is
/// short-circuiting (branches), in 2018 both sides are evaluated -- the same values, other code.
const HLSL_2018: &[&str] = &["color_blit::hud_ps", "foveated::composite"];

fn dxc() -> PathBuf {
    if let Some(p) = std::env::var_os("CVR_DXC") {
        return PathBuf::from(p);
    }
    let kits = Path::new(r"C:\Program Files (x86)\Windows Kits\10\bin");
    let mut found: Vec<PathBuf> = std::fs::read_dir(kits)
        .map(|d| d.flatten().map(|e| e.path().join("x64").join("dxc.exe")).filter(|p| p.exists()).collect())
        .unwrap_or_default();
    found.sort();
    found.pop().expect("dxc.exe: set CVR_DXC or install the Windows SDK")
}

/// naga writes every sampler through its sampler heap (an index buffer per group into `nagaSamplerHeap`); the passes
/// bind static samplers at plain registers. Each `static const SamplerState X = nagaSamplerHeap[nagaGroupNSamplerIndexArray[R]];`
/// becomes `SamplerState X : register(sR);` (R is the register the binding map gave), the heap and index buffer
/// declarations go; anything of the heap left is an error.
fn plain_samplers(hlsl: &str) -> Result<String, String> {
    let mut out = String::with_capacity(hlsl.len());
    for line in hlsl.lines() {
        let t = line.trim();
        if (t.starts_with("SamplerState nagaSamplerHeap[") || t.starts_with("SamplerComparisonState nagaComparisonSamplerHeap["))
            || (t.starts_with("StructuredBuffer<uint> nagaGroup") && t.contains("SamplerIndexArray"))
        {
            continue;
        }
        let rewritten = ["SamplerState", "SamplerComparisonState"].iter().find_map(|kind| {
            let rest = t.strip_prefix("static const ")?.strip_prefix(kind)?.strip_prefix(' ')?;
            let (name, value) = rest.split_once(" = ")?;
            let heap = if *kind == "SamplerState" { "nagaSamplerHeap[" } else { "nagaComparisonSamplerHeap[" };
            let index = value.strip_prefix(heap)?.strip_suffix("]];")?;
            let (array, register) = index.split_once('[')?;
            (array.starts_with("nagaGroup") && array.ends_with("SamplerIndexArray")).then_some(())?;
            let register: u32 = register.parse().ok()?;
            Some(format!("{kind} {name} : register(s{register});"))
        });
        out.push_str(rewritten.as_deref().unwrap_or(line));
        out.push('\n');
    }
    if out.contains("nagaSamplerHeap") || out.contains("nagaComparisonSamplerHeap") || out.contains("SamplerIndexArray") {
        return Err("naga's sampler heap is still referenced after the rewrite".into());
    }
    Ok(out)
}

/// naga's SPIR-V reader (naga 30, front/spv/image.rs) looks up the 2-component vector of an arrayed image's integer
/// coordinate type (its xy apart from the layer) and panics when the module has none: rust-gpu builds a fetch's
/// 3-component coordinate straight from scalars. Each 32-bit integer type with a 3- or 4-component vector and no
/// 2-component one gets that declaration, right after the integer type (unused otherwise: the module means the same).
fn declare_int2(spv: &[u8]) -> Vec<u8> {
    const OP_TYPE_INT: u32 = 21;
    const OP_TYPE_VECTOR: u32 = 23;
    let mut words: Vec<u32> = spv.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    let mut ints: Vec<(u32, usize)> = Vec::new(); // (id, the word after its instruction)
    let mut vectors: Vec<(u32, u32)> = Vec::new(); // (component type, count)
    let mut at = 5;
    while at < words.len() {
        let (count, op) = ((words[at] >> 16) as usize, words[at] & 0xFFFF);
        if count == 0 {
            break;
        }
        if op == OP_TYPE_INT && words[at + 2] == 32 {
            ints.push((words[at + 1], at + count));
        }
        if op == OP_TYPE_VECTOR {
            vectors.push((words[at + 2], words[at + 3]));
        }
        at += count;
    }
    let mut insertions: Vec<(usize, [u32; 4])> = Vec::new();
    for &(id, after) in &ints {
        let wider = vectors.iter().any(|&(c, n)| c == id && n >= 3);
        let has2 = vectors.iter().any(|&(c, n)| c == id && n == 2);
        if wider && !has2 {
            let new_id = words[3];
            words[3] += 1;
            insertions.push((after, [(4 << 16) | OP_TYPE_VECTOR, new_id, id, 2]));
        }
    }
    insertions.sort_by(|a, b| b.0.cmp(&a.0));
    for (after, ins) in insertions {
        words.splice(after..after, ins);
    }
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// Every `ImageStore` to the storage images at `bindings` (descriptor set 0) made an `ImageAtomic` max (ATOMIC_MAX_STORES),
/// the images given ATOMIC access. The number of stores turned.
fn atomic_max_stores(module: &mut naga::Module, bindings: &[u32], fun: naga::AtomicFunction) -> usize {
    use naga::{ImageClass, Span, StorageAccess, Type, TypeInner};
    let mut images = std::collections::HashSet::new();
    for (handle, global) in module.global_variables.iter_mut() {
        let Some(rb) = &global.binding else { continue };
        if rb.group != 0 || !bindings.contains(&rb.binding) {
            continue;
        }
        let (name, inner) = (module.types[global.ty].name.clone(), module.types[global.ty].inner.clone());
        let TypeInner::Image { dim, arrayed, class: ImageClass::Storage { format, access } } = inner else {
            panic!("binding {}: not a storage image ({inner:?})", rb.binding);
        };
        let class = ImageClass::Storage { format, access: access | StorageAccess::ATOMIC };
        global.ty = module.types.insert(Type { name, inner: TypeInner::Image { dim, arrayed, class } }, Span::UNDEFINED);
        images.insert(handle);
    }
    let mut turned = 0;
    let mut fix = |f: &mut naga::Function| {
        let mut body = std::mem::take(&mut f.body);
        stores_to_atomics(&mut body, f, &images, &mut turned, fun);
        f.body = body;
    };
    for (_, f) in module.functions.iter_mut() {
        fix(f);
    }
    for ep in module.entry_points.iter_mut() {
        fix(&mut ep.function);
    }
    turned
}

fn stores_to_atomics(
    block: &mut naga::Block,
    f: &naga::Function,
    images: &std::collections::HashSet<naga::Handle<naga::GlobalVariable>>,
    turned: &mut usize,
    fun: naga::AtomicFunction,
) {
    use naga::{Expression, Statement};
    for statement in block.iter_mut() {
        match statement {
            Statement::Block(b) => stores_to_atomics(b, f, images, turned, fun),
            Statement::If { accept, reject, .. } => {
                stores_to_atomics(accept, f, images, turned, fun);
                stores_to_atomics(reject, f, images, turned, fun);
            }
            Statement::Switch { cases, .. } => {
                for c in cases.iter_mut() {
                    stores_to_atomics(&mut c.body, f, images, turned, fun);
                }
            }
            Statement::Loop { body, continuing, .. } => {
                stores_to_atomics(body, f, images, turned, fun);
                stores_to_atomics(continuing, f, images, turned, fun);
            }
            &mut Statement::ImageStore { image, coordinate, array_index, value } => {
                if matches!(f.expressions[image], Expression::GlobalVariable(g) if images.contains(&g)) {
                    // naga's store takes a vec4 (a scalar write splatted); the atomic, the scalar
                    let value = match f.expressions[value] {
                        Expression::Splat { value, .. } => value,
                        ref other => panic!("an image store's value not a splatted scalar: {other:?}"),
                    };
                    *statement = Statement::ImageAtomic { image, coordinate, array_index, fun, value };
                    *turned += 1;
                }
            }
            _ => {}
        }
    }
}

/// A `Select` on a splatted scalar (rust-gpu's SPIR-V selects a vector by a vector of one condition) selects by the
/// scalar: naga writes `(c).xx ? a : b`, which HLSL 2021 refuses (a short-circuiting `?:` takes a scalar condition);
/// `c ? a : b` is the same choice. The number of conditions made scalar. (The two shaders that predate this compile
/// as HLSL 2018 instead: HLSL_2018.)
fn scalar_select_conditions(module: &mut naga::Module) -> usize {
    use naga::Expression;
    let mut count = 0;
    let mut fix = |f: &mut naga::Function| {
        let handles: Vec<_> = f.expressions.iter().map(|(h, _)| h).collect();
        for h in handles {
            let Expression::Select { condition, .. } = f.expressions[h] else { continue };
            let Expression::Splat { value, .. } = f.expressions[condition] else { continue };
            if let Expression::Select { condition, .. } = &mut f.expressions[h] {
                *condition = value;
                count += 1;
            }
        }
    };
    for (_, f) in module.functions.iter_mut() {
        fix(f);
    }
    for ep in module.entry_points.iter_mut() {
        fix(&mut ep.function);
    }
    count
}

/// naga's HLSL writes a loop's `continuing` at the head of the next iteration -- after the stores that move the loop's
/// phi variables on -- and evaluates its `break if` there anew, from those moved variables: a `while` left one
/// iteration early, an accumulation read after the loop lost its last term (the overlay blur, found by the A/B; the
/// SPIR-V is right: the condition is a value the body computed before the phis move). Each such condition is pinned:
/// stored into a local of its own where the loop body emits it (a `LogicalNot` naga put around it in `continuing` kept
/// there), and `break if` reads the local. The number of loops pinned; a condition not emitted at the top of the body
/// is left as it is (and counted apart).
fn pin_break_ifs(module: &mut naga::Module) -> (usize, usize) {
    use naga::{Expression, Scalar, Span, Type, TypeInner};
    let bool_ty = module.types.insert(Type { name: None, inner: TypeInner::Scalar(Scalar::BOOL) }, Span::UNDEFINED);
    let mut counts = (0, 0);
    let mut fix = |f: &mut naga::Function| {
        let mut body = std::mem::take(&mut f.body);
        pin_in_block(&mut body, f, bool_ty, &mut counts);
        f.body = body;
    };
    for (_, f) in module.functions.iter_mut() {
        fix(f);
    }
    for ep in module.entry_points.iter_mut() {
        fix(&mut ep.function);
    }
    let _ = Expression::LocalVariable;
    counts
}

fn pin_in_block(block: &mut naga::Block, f: &mut naga::Function, bool_ty: naga::Handle<naga::Type>, counts: &mut (usize, usize)) {
    use naga::{Expression, Span, Statement, UnaryOperator};
    for i in 0..block.len() {
        match &mut block[i] {
            Statement::Block(b) => pin_in_block(b, f, bool_ty, counts),
            Statement::If { accept, reject, .. } => {
                pin_in_block(accept, f, bool_ty, counts);
                pin_in_block(reject, f, bool_ty, counts);
            }
            Statement::Switch { cases, .. } => {
                for c in cases.iter_mut() {
                    pin_in_block(&mut c.body, f, bool_ty, counts);
                }
            }
            Statement::Loop { body, continuing, break_if } => {
                pin_in_block(body, f, bool_ty, counts);
                pin_in_block(continuing, f, bool_ty, counts);
                let Some(cond) = *break_if else { continue };
                // the value the body computed, and whether naga negated it in `continuing`
                let (value, negated) = match f.expressions[cond] {
                    Expression::Unary { op: UnaryOperator::LogicalNot, expr } if emitted_at(body, expr).is_some() => (expr, true),
                    _ => (cond, false),
                };
                let Some(at) = emitted_at(body, value) else {
                    counts.1 += 1;
                    continue;
                };
                let local = f.local_variables.append(
                    naga::LocalVariable { name: Some(format!("break_if_{}", counts.0)), ty: bool_ty, init: None },
                    Span::UNDEFINED,
                );
                let pointer = f.expressions.append(Expression::LocalVariable(local), Span::UNDEFINED);
                body.splice(at + 1..at + 1, naga::Block::from_vec(vec![Statement::Store { pointer, value }]));
                let load = f.expressions.append(Expression::Load { pointer }, Span::UNDEFINED);
                let mut tail = load;
                if negated {
                    tail = f.expressions.append(Expression::Unary { op: UnaryOperator::LogicalNot, expr: load }, Span::UNDEFINED);
                }
                continuing.push(Statement::Emit(naga::Range::new_from_bounds(load, tail)), Span::UNDEFINED);
                *break_if = Some(tail);
                counts.0 += 1;
            }
            _ => {}
        }
    }
}

/// naga's HLSL writes a loop's `continuing` at the head of the next iteration, and an expression of the body that
/// `continuing` uses is written out again there -- reading the phi variables as they are then. naga's `continuing` moves
/// the loop's phis on (the header's, read by the next iteration) before the statements that make the loop's exit
/// values, so those read the NEXT iteration's phis (vision_highlight: the second break's flags read one tap late, found
/// by the bench; the family of pin_break_ifs). SPIR-V's continue block reads every phi as this iteration's: the stores
/// to the header's phis (the phi variables the body loads) are moved, in their order, after the continuing's other
/// statements. The number of loops changed.
fn defer_header_phi_stores(module: &mut naga::Module) -> usize {
    let mut count = 0;
    let mut fix = |f: &mut naga::Function| {
        let mut body = std::mem::take(&mut f.body);
        defer_in_block(&mut body, f, &mut count);
        f.body = body;
    };
    for (_, f) in module.functions.iter_mut() {
        fix(f);
    }
    for ep in module.entry_points.iter_mut() {
        fix(&mut ep.function);
    }
    count
}

fn defer_in_block(block: &mut naga::Block, f: &mut naga::Function, count: &mut usize) {
    use naga::{Expression, Statement};
    for statement in block.iter_mut() {
        match statement {
            Statement::Block(b) => defer_in_block(b, f, count),
            Statement::If { accept, reject, .. } => {
                defer_in_block(accept, f, count);
                defer_in_block(reject, f, count);
            }
            Statement::Switch { cases, .. } => {
                for c in cases.iter_mut() {
                    defer_in_block(&mut c.body, f, count);
                }
            }
            Statement::Loop { body, continuing, .. } => {
                defer_in_block(body, f, count);
                defer_in_block(continuing, f, count);
                // the phi variables the body loads: the loop's header phis
                let mut emitted = Vec::new();
                emitted_in(body, &mut emitted);
                let header: Vec<naga::Handle<naga::LocalVariable>> = emitted
                    .into_iter()
                    .filter_map(|h| match f.expressions[h] {
                        Expression::Load { pointer } => match f.expressions[pointer] {
                            Expression::LocalVariable(v) if f.local_variables[v].name.as_deref().is_some_and(|n| n.starts_with("phi_")) => Some(v),
                            _ => None,
                        },
                        _ => None,
                    })
                    .collect();
                let moves = |s: &Statement| match *s {
                    Statement::Store { pointer, .. } => matches!(f.expressions[pointer], Expression::LocalVariable(v) if header.contains(&v)),
                    _ => false,
                };
                // only when a statement that is not such a store follows one
                let statements: Vec<_> = std::mem::take(continuing).span_into_iter().collect();
                let first = statements.iter().position(|(s, _)| moves(s));
                let later = first.is_some_and(|i| statements[i..].iter().any(|(s, _)| !moves(s)));
                let (mut kept, mut deferred) = (naga::Block::new(), naga::Block::new());
                for (s, span) in statements {
                    if later && moves(&s) {
                        deferred.push(s, span);
                    } else {
                        kept.push(s, span);
                    }
                }
                if later {
                    *count += 1;
                }
                for (s, span) in deferred.span_into_iter() {
                    kept.push(s, span);
                }
                *continuing = kept;
            }
            _ => {}
        }
    }
}

/// Every expression an `Emit` in `block` (and the blocks in it) makes.
fn emitted_in(block: &naga::Block, out: &mut Vec<naga::Handle<naga::Expression>>) {
    use naga::Statement;
    for statement in block.iter() {
        match statement {
            Statement::Emit(range) => out.extend(range.clone()),
            Statement::Block(b) => emitted_in(b, out),
            Statement::If { accept, reject, .. } => {
                emitted_in(accept, out);
                emitted_in(reject, out);
            }
            Statement::Switch { cases, .. } => cases.iter().for_each(|c| emitted_in(&c.body, out)),
            Statement::Loop { body, continuing, .. } => {
                emitted_in(body, out);
                emitted_in(continuing, out);
            }
            _ => {}
        }
    }
}

/// The index, in `block`'s top level, of the `Emit` that makes `expr`.
fn emitted_at(block: &naga::Block, expr: naga::Handle<naga::Expression>) -> Option<usize> {
    block.iter().position(|s| matches!(s, naga::Statement::Emit(r) if r.index_range().contains(&(expr.index() as u32))))
}

/// The "HLSL Bind" column of DXC's listing ("; Resource Bindings:").
fn listed_bindings(listing: &str) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    let (mut inside, mut header) = (false, false);
    for line in listing.lines() {
        let t = line.trim_start_matches(';').trim();
        if t.starts_with("Resource Bindings:") {
            inside = true;
            continue;
        }
        if !inside {
            continue;
        }
        let cols: Vec<&str> = t.split_whitespace().collect();
        if cols.first() == Some(&"Name") {
            header = true;
            continue;
        }
        // the table ends at the first empty line after its header (an empty table has no row at all)
        if t.is_empty() {
            if header {
                break;
            }
            continue;
        }
        if cols.iter().all(|c| c.chars().all(|ch| ch == '-')) {
            continue;
        }
        // Name Type Format Dim ID HLSL-Bind Count -- DXC runs a long type into its format ("UAVunorm_f32" for an
        // RWTexture2D<unorm float4>): six columns then, the register still the one before the count
        let register = |c: &str| {
            let digits = c.trim_start_matches(|ch: char| ch.is_ascii_alphabetic());
            !digits.is_empty() && digits.chars().next().is_some_and(|ch| ch.is_ascii_digit())
        };
        if cols.len() >= 6 && register(cols[cols.len() - 2]) {
            set.insert(cols[cols.len() - 2].to_string());
        }
    }
    set
}

fn main() {
    let check = std::env::args().any(|a| a == "--check");
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let shaders_root = here.parent().unwrap();
    let committed = shaders_root.join("..").join("crates").join("cvr-gpu").join("shaders");
    let work = shaders_root.join("target").join("dxil");
    std::fs::create_dir_all(&work).unwrap();
    let out_dir = if check { work.clone() } else { committed.clone() };
    std::fs::create_dir_all(&out_dir).unwrap();

    // 1. Rust -> SPIR-V, one module per entry point
    let mut builder = spirv_builder::SpirvBuilder::new(shaders_root.join("passes"), "spirv-unknown-vulkan1.1");
    builder.build_script.defaults = false;
    builder.multimodule = true;
    // GetDimensions (OpImageQuerySizeLod)
    builder.capabilities.push(spirv_builder::Capability::ImageQuery);
    // the storage images in the extended formats (rg16f, r32f) the compute passes write
    builder.capabilities.push(spirv_builder::Capability::StorageImageExtendedFormats);
    let result = builder.build().expect("rust-gpu: the passes did not build");
    let spirv_builder::ModuleResult::MultiModule(modules) = &result.module else {
        panic!("rust-gpu: expected one module per entry point")
    };
    let dxc = dxc();
    let dxc_version = Command::new(&dxc).arg("--version").output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    let mut manifest = format!(
        "The port's shaders in DXIL, made from rust/shaders/passes by `cargo xtask shaders` -- not edited by hand.\n\
         rust-gpu 0.11.0 (spirv-builder), naga 30, {dxc_version}\n\n"
    );
    let mut failures = 0;
    for shader in SHADERS {
        let spv_path = modules.get(shader.entry).unwrap_or_else(|| panic!("no entry point {} in the passes (have: {:?})", shader.entry, modules.keys()));
        let spv = declare_int2(&std::fs::read(spv_path).unwrap());
        // 2. SPIR-V -> naga, D3D12's conventions as the Rust source writes them
        let options = naga::front::spv::Options { adjust_coordinate_space: false, ..Default::default() };
        let mut module = naga::front::spv::parse_u8_slice(&spv, &options).unwrap_or_else(|e| panic!("{}: naga could not read it: {e:?}", shader.entry));
        if let Some((_, bindings)) = ATOMIC_MAX_STORES.iter().find(|(entry, _)| *entry == shader.entry) {
            let turned = atomic_max_stores(&mut module, bindings, naga::AtomicFunction::Max);
            println!("[naga] {}: {turned} image store(s) made InterlockedMax", shader.entry);
            assert!(turned > 0, "{}: no image store to turn into InterlockedMax", shader.entry);
        }
        if let Some((_, bindings)) = ATOMIC_MIN_STORES.iter().find(|(entry, _)| *entry == shader.entry) {
            let turned = atomic_max_stores(&mut module, bindings, naga::AtomicFunction::Min);
            println!("[naga] {}: {turned} image store(s) made InterlockedMin", shader.entry);
            assert!(turned > 0, "{}: no image store to turn into InterlockedMin", shader.entry);
        }
        if !HLSL_2018.contains(&shader.entry) {
            let scalar = scalar_select_conditions(&mut module);
            if scalar > 0 {
                println!("[naga] {}: {scalar} vector select(s) by a scalar condition", shader.entry);
            }
        }
        let deferred = defer_header_phi_stores(&mut module);
        if deferred > 0 {
            println!("[naga] {}: {deferred} loop(s) moving their phis on after the continuing's other statements", shader.entry);
        }
        let (pinned, left) = pin_break_ifs(&mut module);
        if left > 0 {
            panic!("{}: {left} loop condition(s) not made at the top of the loop body: naga's HLSL would read them late", shader.entry);
        }
        if pinned > 0 {
            println!("[naga] {}: {pinned} loop condition(s) pinned", shader.entry);
        }
        let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
            .validate(&module)
            .unwrap_or_else(|e| panic!("{}: naga validation: {e:?}", shader.entry));
        let [ep] = module.entry_points.as_slice() else { panic!("{}: one entry point expected", shader.entry) };
        // 3. naga -> HLSL with the root signature's registers
        let mut binding_map = naga::back::hlsl::BindingMap::default();
        for &(binding, reg) in shader.bindings {
            let (group, size) = match reg {
                Reg::TArray(_, count) => (0, Some(count)),
                Reg::TSpace(_, space, count) => (space, Some(count)),
                _ => (0, None),
            };
            binding_map.insert(
                naga::ResourceBinding { group, binding },
                naga::back::hlsl::BindTarget { space: group as u8, register: reg.number(), binding_array_size: size, ..Default::default() },
            );
        }
        let mut sampler_buffers = naga::back::hlsl::SamplerIndexBufferBindingMap::default();
        sampler_buffers.insert(
            naga::back::hlsl::SamplerIndexBufferKey { group: 0 },
            naga::back::hlsl::BindTarget { space: 255, register: 0, ..Default::default() },
        );
        let hlsl_options = naga::back::hlsl::Options {
            shader_model: naga::back::hlsl::ShaderModel::V6_0,
            binding_map,
            sampler_buffer_binding_map: sampler_buffers,
            fake_missing_bindings: false,
            ..Default::default()
        };
        let mut hlsl = String::new();
        let pipeline = naga::back::hlsl::PipelineOptions::default();
        let reflection = naga::back::hlsl::Writer::new(&mut hlsl, &hlsl_options, &pipeline)
            .write(&module, &info, None)
            .unwrap_or_else(|e| panic!("{}: naga could not write HLSL: {e:?}", shader.entry));
        let hlsl = plain_samplers(&hlsl).unwrap_or_else(|e| panic!("{}: {e}", shader.entry));
        let name = reflection.entry_point_names[0].as_ref().unwrap_or_else(|e| panic!("{}: {e:?}", shader.entry)).clone();
        let hlsl = match SIGNATURES.iter().find(|(entry, _)| *entry == shader.entry) {
            Some((_, signature)) => apply_signature(&hlsl, &name, signature).unwrap_or_else(|e| panic!("{}: {e}", shader.entry)),
            None => hlsl,
        };
        let hlsl = match VERTEX_SIGNATURES.iter().find(|(entry, _, _)| *entry == shader.entry) {
            Some((_, inputs, outputs)) => {
                apply_vertex_signature(&hlsl, &name, inputs, outputs).unwrap_or_else(|e| panic!("{}: {e}", shader.entry))
            }
            None => hlsl,
        };
        let hlsl = match TYPED_BUFFERS.iter().find(|(entry, _, _)| *entry == shader.entry) {
            Some((_, register, ty)) => typed_buffer(&hlsl, register, ty).unwrap_or_else(|e| panic!("{}: {e}", shader.entry)),
            None => hlsl,
        };
        let hlsl = match ROOT_SIGNATURES.iter().find(|(entry, _)| *entry == shader.entry) {
            Some((_, signature)) => embed_root_signature(&hlsl, &name, signature).unwrap_or_else(|e| panic!("{}: {e}", shader.entry)),
            None => hlsl,
        };
        let hlsl_path = work.join(format!("{}.hlsl", shader.out));
        std::fs::write(&hlsl_path, &hlsl).unwrap();
        // 4. HLSL -> DXIL, validated; the declared bindings checked
        let profile = match ep.stage {
            naga::ShaderStage::Vertex => "vs_6_0",
            naga::ShaderStage::Fragment => "ps_6_0",
            naga::ShaderStage::Compute => "cs_6_0",
            other => panic!("{}: stage {other:?}", shader.entry),
        };
        let binary = out_dir.join(format!("{}.dxil", shader.out));
        let listing = work.join(format!("{}.lst", shader.out));
        let o = Command::new(&dxc)
            .args(["-T", profile, "-E", &name, "-HV", if HLSL_2018.contains(&shader.entry) { "2018" } else { "2021" }])
            .args(["-O3", "-Wno-parentheses-equality"])
            .args(if IEEE_STRICT.contains(&shader.entry) { &["-Gis"][..] } else { &[][..] })
            .arg("-Fo")
            .arg(&binary)
            .arg("-Fc")
            .arg(&listing)
            .arg(&hlsl_path)
            .output()
            .expect("dxc could not be started");
        if !o.status.success() {
            panic!("{}: DXC failed:\n{}{}", shader.entry, String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
        }
        let declared = listed_bindings(&std::fs::read_to_string(&listing).unwrap());
        let expected: BTreeSet<String> = shader.bindings.iter().map(|&(_, r)| r.listed()).collect();
        if declared != expected {
            panic!("{}: the DXIL declares {declared:?}, the root signature has {expected:?}", shader.entry);
        }
        let bytes = std::fs::read(&binary).unwrap();
        if bytes.get(..4) != Some(b"DXBC") || bytes.get(4..20).is_some_and(|d| d.iter().all(|&b| b == 0)) {
            panic!("{}: the DXIL container is not signed (no validator digest)", shader.entry);
        }
        writeln!(manifest, "{}.dxil  {profile}  {} bytes  {}  {:?}", shader.out, bytes.len(), shader.entry, expected).unwrap();
        if check {
            let mine = committed.join(format!("{}.dxil", shader.out));
            let same = std::fs::read(&mine).is_ok_and(|c| c == bytes);
            println!("[check] {}.dxil {}", shader.out, if same { "the same" } else { "DIFFERS from the committed file" });
            failures += usize::from(!same);
        } else {
            println!("[dxil] {}.dxil {profile} {} bytes, bindings {:?}", shader.out, bytes.len(), expected);
        }
    }
    if check {
        let same = std::fs::read_to_string(committed.join("MANIFEST.txt")).is_ok_and(|m| m == manifest);
        println!("[check] MANIFEST.txt {}", if same { "the same" } else { "DIFFERS" });
        failures += usize::from(!same);
        if failures > 0 {
            std::process::exit(1);
        }
    } else {
        std::fs::write(committed.join("MANIFEST.txt"), manifest).unwrap();
    }
}
