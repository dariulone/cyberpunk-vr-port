//! The optics' pixel shaders made from Rust (rust/shaders/passes/src/optics.rs) against the DXIL shipped before them
//! (src/Shaders/*.dxil, DXC's), on WARP: drawn with a vertex shader giving the engine pass's output signature (the
//! glass's SV_Position + TEXCOORD0..6, the sight's SV_Position + TEXCOORD0..4), the engine's constant buffers at b0, b1
//! and b4, every target byte compared -- the lens's VR marker and the sight's image scales set and not. And the two
//! source resolves (compute, their root signatures embedded: each made from its own DXIL) on HDR pictures with dark
//! dots and non-finite texels, at rest and moving, reset, clean and sharpened.
use crate::bench::{self, Arg, Bench};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

/// Root CBVs at b0, b1, b4 (parameters 0, 1, 2), as the engine binds its constant buffers.
fn root(device: &ID3D12Device) -> ID3D12RootSignature {
    let cbv = |register: u32| D3D12_ROOT_PARAMETER {
        ParameterType: D3D12_ROOT_PARAMETER_TYPE_CBV,
        Anonymous: D3D12_ROOT_PARAMETER_0 { Descriptor: D3D12_ROOT_DESCRIPTOR { ShaderRegister: register, RegisterSpace: 0 } },
        ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
    };
    let params = [cbv(0), cbv(1), cbv(4)];
    let desc = D3D12_ROOT_SIGNATURE_DESC {
        NumParameters: params.len() as u32,
        pParameters: params.as_ptr(),
        Flags: D3D12_ROOT_SIGNATURE_FLAG_NONE,
        ..Default::default()
    };
    let mut blob = None;
    // SAFETY: the description and its parameters on the stack.
    unsafe {
        D3D12SerializeRootSignature(&desc, D3D_ROOT_SIGNATURE_VERSION_1, &mut blob, None).unwrap();
        let blob = blob.unwrap();
        device.CreateRootSignature(0, core::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize())).unwrap()
    }
}

/// The bench's vertex shaders: a full-screen triangle carrying an engine pass's outputs, each varying over the screen
/// (the glass's current and previous clip positions with a w of their own, so the velocity divides by something).
const VERTEX: &str = r#"
struct Glass { float4 pos : SV_Position; float4 t0 : TEXCOORD0; float4 t1 : TEXCOORD1; float4 t2 : TEXCOORD2;
               float4 t3 : TEXCOORD3; float4 current : TEXCOORD4; float4 previous : TEXCOORD5; float2 t6 : TEXCOORD6; };
struct Sight { float4 pos : SV_Position; float4 t0 : TEXCOORD0; float4 t1 : TEXCOORD1; float4 t2 : TEXCOORD2;
               float4 t3 : TEXCOORD3; float4 t4 : TEXCOORD4; };
float2 Uv(uint id) { return float2((id << 1) & 2, id & 2); }
Glass GlassVs(uint id : SV_VertexID) {
    float2 uv = Uv(id); float2 ndc = uv * float2(2, -2) + float2(-1, 1);
    Glass o; o.pos = float4(ndc, 0.5, 1);
    o.t0 = float4(uv, 1 - uv); o.t1 = float4(uv.yx, 2, 3); o.t2 = float4(-uv, 0.25, 0.75); o.t3 = float4(uv * 7, 1, 0);
    float w = 1.5 + uv.x * 0.75;
    o.current = float4(ndc * w, 0.3, w);
    o.previous = float4((ndc + float2(0.02, -0.013) + uv * 0.01) * (w * 1.1), 0.31, w * 1.1);
    o.t6 = uv * 3;
    return o;
}
Sight SightVs(uint id : SV_VertexID) {
    float2 uv = Uv(id); float2 ndc = uv * float2(2, -2) + float2(-1, 1);
    Sight o; o.pos = float4(ndc, 0.5, 1);
    o.t0 = float4(uv, 0.5, 0); o.t1 = float4(1, uv, 1); o.t2 = float4(0, 0, 1, uv.x); o.t3 = float4(uv.y, 0, 0, 1); o.t4 = float4(uv * 9, 2, 1);
    return o;
}
"#;

/// The first differing 16-bit words of two outputs (index, reference, Rust), for a failure's message.
fn first_apart(a: &[u8], b: &[u8]) -> Vec<(usize, u16, u16)> {
    a.chunks(2)
        .zip(b.chunks(2))
        .enumerate()
        .filter(|(_, (x, y))| x != y)
        .take(6)
        .map(|(i, (x, y))| (i, u16::from_le_bytes([x[0], x[1]]), u16::from_le_bytes([y[0], y[1]])))
        .collect()
}

fn floats(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}
/// `rows` float4 rows from a pattern in [-1, 1), then the given rows set.
fn rows(seed: u32, count: usize, set: &[(usize, [f32; 4])]) -> Vec<f32> {
    let raw = bench::pattern(seed, count * 8);
    let mut v: Vec<f32> = (0..count * 4).map(|i| u16::from_le_bytes([raw[2 * i], raw[2 * i + 1]]) as f32 / 32768.0 - 1.0).collect();
    for &(row, value) in set {
        v[row * 4..row * 4 + 4].copy_from_slice(&value);
    }
    v
}

fn reference(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../src/Shaders").join(format!("{name}.dxil"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The reference and the Rust pixel shader drawn alike into zeroed targets; their bytes, and whether anything was drawn.
#[allow(clippy::too_many_arguments)]
fn both(
    b: &mut Bench,
    root: &ID3D12RootSignature,
    vs: &[u8],
    name: &str,
    rust: &'static [u8],
    format: DXGI_FORMAT,
    cbvs: [&ID3D12Resource; 3],
    what: &str,
) -> bool {
    let target = b.target(format, 48, 32);
    let reference = reference(name);
    let mut results = Vec::new();
    for ps in [bench::code(&reference), bench::dxil(rust)] {
        b.upload(&target, &vec![0u8; (target.width * target.height * target.texel) as usize]);
        let pso = b.graphics(root, bench::code(vs), ps, format);
        b.draw_with(root, &pso, &target, &[Arg::Cbv(0, cbvs[0]), Arg::Cbv(1, cbvs[1]), Arg::Cbv(2, cbvs[2])], 3);
        results.push(b.read(&target));
    }
    let differ = results[0].iter().zip(&results[1]).filter(|(x, y)| x != y).count();
    assert_eq!(differ, 0, "{what}: {differ} of {} bytes differ (first at {:?})", results[0].len(), results[0].iter().zip(&results[1]).position(|(x, y)| x != y));
    results[1].iter().any(|&x| x != 0)
}

#[test]
fn optics_pixel_shaders_alike() {
    let mut b = Bench::warp();
    let root = root(&b.device);
    let glass_vs = bench::dxc(VERTEX, "GlassVs", "vs_6_0");
    let sight_vs = bench::dxc(VERTEX, "SightVs", "vs_6_0");
    for (n, (marker, alternate)) in [(713.25f32, 0u32), (713.25, 1), (12.5, 0)].into_iter().enumerate() {
        let mut globals = vec![0u32; 48];
        globals[47] = alternate;
        globals[3] = 0x1234_5678;
        let globals: Vec<u8> = globals.iter().flat_map(|v| v.to_le_bytes()).collect();
        let camera = rows(31 + n as u32, 53, &[(51, [0.0012, -0.0007, 0.0, 0.0])]);
        let material = rows(57 + n as u32, 32, &[(31, [0.1, 0.2, 0.3, marker]), (17, [4.0 + n as f32, 0.0, 0.0, 0.0])]);
        let cbvs = [b.constants(&globals), b.constants(&floats(&camera)), b.constants(&floats(&material))];
        let cb = [&cbvs[0], &cbvs[1], &cbvs[2]];
        let drawn = both(&mut b, &root, &glass_vs, "lens_mask_ps", crate::dxil::LENS_MASK_PS, DXGI_FORMAT_R8_UNORM, cb, &format!("case {n} lens_mask"));
        assert_eq!(drawn, n == 0, "case {n}: the lens mask drawn only on a VR lens on the main projection path");
        let drawn = both(&mut b, &root, &glass_vs, "lens_velocity_ps", crate::dxil::LENS_VELOCITY_PS, DXGI_FORMAT_R32G32_FLOAT, cb, &format!("case {n} lens_velocity"));
        assert_eq!(drawn, n == 0, "case {n}: the lens velocity drawn only on a VR lens");
    }
    for (n, scale) in [-713.25f32, -714.25, -716.25, -717.25, -715.25, 2.0].into_iter().enumerate() {
        let sight = rows(91 + n as u32, 21, &[(6, [scale, 0.5, 0.5, 1.0])]);
        let cbvs = [b.constants(&[0u8; 16]), b.constants(&[0u8; 16]), b.constants(&floats(&sight))];
        let drawn = both(
            &mut b,
            &root,
            &sight_vs,
            "sight_mask_ps",
            crate::dxil::SIGHT_MASK_PS,
            DXGI_FORMAT_R8_UNORM,
            [&cbvs[0], &cbvs[1], &cbvs[2]],
            &format!("scale {scale} sight_mask"),
        );
        assert_eq!(drawn, n < 4, "scale {scale}: the sight mask drawn only on a camera scope's surface");
    }
}

/// A float picture in [lo, hi), with a dark dot every 37 texels and (`specials`) a NaN and an infinity in it.
fn picture(seed: u32, texels: usize, lo: f32, hi: f32, specials: bool) -> Vec<u8> {
    let raw = bench::pattern(seed, texels * 8);
    let mut v: Vec<f32> = (0..texels * 4).map(|i| lo + (u16::from_le_bytes([raw[2 * i], raw[2 * i + 1]]) as f32 / 65536.0) * (hi - lo)).collect();
    for t in (5..texels).step_by(37) {
        v[t * 4] = 0.0005;
        v[t * 4 + 1] = 0.0004;
        v[t * 4 + 2] = 0.0006;
    }
    if specials {
        v[4 * 11] = f32::NAN;
        v[4 * 23 + 1] = f32::INFINITY;
    }
    floats(&v)
}

/// A compute shader that embeds its root signature, run as the reference and as the Rust DXIL on the same inputs
/// (SRVs at slots 0.., UAVs after them, one table), every UAV compared.
#[allow(clippy::too_many_arguments)]
fn resolve_both(
    b: &mut Bench,
    name: &str,
    rust: &'static [u8],
    srvs: &[&bench::Texture],
    uavs: &[&bench::Texture],
    constants: &[u32],
    groups: [u32; 3],
    what: &str,
) {
    // the HLSL compiled IEEE-strict, as the builder compiles the Rust (bit for bit), and the fast-math DXIL shipped
    // before (its distance reported)
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../src/Shaders").join(format!("{name}.hlsl"));
    let strict = bench::dxc_with(&std::fs::read_to_string(&path).unwrap(), "main", "cs_6_0", &["-Gis"]);
    let shipped = reference(name);
    let mut results = Vec::new();
    for code in [bench::code(&strict), bench::dxil(rust), bench::code(&shipped)] {
        // SAFETY: the shader's own bytes, its embedded root signature read by the device.
        let root: ID3D12RootSignature =
            unsafe { b.device.CreateRootSignature(0, core::slice::from_raw_parts(code.pShaderBytecode as *const u8, code.BytecodeLength)) }
                .unwrap_or_else(|e| panic!("{what}: no root signature in the shader: {e}"));
        for (i, t) in srvs.iter().enumerate() {
            b.srv(i as u32, Some(t), DXGI_FORMAT_UNKNOWN);
        }
        for (i, t) in uavs.iter().enumerate() {
            b.uav((srvs.len() + i) as u32, Some(t), DXGI_FORMAT_UNKNOWN);
            b.upload(t, &vec![0u8; (t.width * t.height * t.texel) as usize]);
        }
        let pso = b.pipeline(&root, code);
        b.dispatch_with(&root, &pso, &[Arg::Constants(0, constants), Arg::Table(1, 0)], groups);
        results.push(uavs.iter().map(|t| b.read(t)).collect::<Vec<_>>());
    }
    for (i, (reference, rust)) in results[0].iter().zip(&results[1]).enumerate() {
        let differ = reference.iter().zip(rust).filter(|(x, y)| x != y).count();
        assert_eq!(differ, 0, "{what}: UAV {i}: {differ} of {} bytes differ (first at {:?})", reference.len(), reference.iter().zip(rust).position(|(x, y)| x != y));
        assert!(rust.iter().any(|&x| x != 0), "{what}: UAV {i} left empty");
        let shipped = &results[2][i];
        let apart = shipped.iter().zip(rust).filter(|(x, y)| x != y).count();
        println!("{what}: UAV {i}: the shipped fast-math DXIL {apart} of {} bytes apart", rust.len());
    }
}

#[test]
fn source_resolves_alike() {
    let mut b = Bench::warp();
    let (w, h) = (40u32, 28u32);
    let groups = [w.div_ceil(8), h.div_ceil(8), 1];
    let words = |v: &[f32]| v.iter().map(|x| x.to_bits()).collect::<Vec<u32>>();
    let current = b.texture(DXGI_FORMAT_R32G32B32A32_FLOAT, w, h, false);
    b.upload(&current, &picture(3, (w * h) as usize, -0.1, 3.0, true));
    let history = b.texture(DXGI_FORMAT_R32G32B32A32_FLOAT, w, h, false);
    b.upload(&history, &picture(4, (w * h) as usize, 0.0, 2.5, false));
    let history_out = b.texture(DXGI_FORMAT_R16G16B16A16_FLOAT, w, h, true);
    // the camera: a small rotation's homography, at rest and reset
    for (n, reset) in [0u32, 1].into_iter().enumerate() {
        let mut k = vec![w, h, (1.0 / w as f32).to_bits(), (1.0 / h as f32).to_bits()];
        k.extend(words(&[0.1, 0.5, 1.25]));
        k.push(reset);
        k.extend(words(&[0.998, 0.02, 0.01, 0.0, -0.02, 0.999, -0.015, 0.0, 0.0, 0.0, 1.0, 0.0]));
        resolve_both(&mut b, "camera_source_resolve_cs", crate::dxil::CAMERA_SOURCE_RESOLVE_CS, &[&current, &history], &[&history_out], &k, groups, &format!("camera case {n}"));
    }
    // the lens: the engine's velocity, jittered, plain, clean and sharpened
    let velocity = b.texture(DXGI_FORMAT_R32G32_FLOAT, w, h, false);
    let raw = bench::pattern(9, (w * h * 4) as usize);
    let motion: Vec<f32> = (0..(w * h * 2) as usize).map(|i| (raw[2 * i] as f32 - 128.0) / 128.0 * 0.03).collect();
    b.upload(&velocity, &floats(&motion));
    let output = b.texture(DXGI_FORMAT_R11G11B10_FLOAT, w, h, true);
    for (n, (reset, clean, sharpen)) in [(0u32, 0u32, 0.0f32), (0, 1, 0.0), (0, 1, 0.6), (1, 1, 0.6)].into_iter().enumerate() {
        let mut k = vec![w, h, (1.0 / w as f32).to_bits(), (1.0 / h as f32).to_bits()];
        k.extend(words(&[0.15, 1.1]));
        k.extend([reset, clean]);
        k.extend(words(&[0.31, -0.27, sharpen]));
        resolve_both(
            &mut b,
            "lens_source_resolve_cs",
            crate::dxil::LENS_SOURCE_RESOLVE_CS,
            &[&current, &velocity, &history],
            &[&history_out, &output],
            &k,
            groups,
            &format!("lens case {n}"),
        );
    }
}

/// The world markers' vertex shaders (rust/shaders/passes/src/world_markers.rs) against src/Render/WorldMarkers.hlsl
/// compiled by DXC IEEE-strict, as the builder compiles them: six vertices of every kind (plain, depth-tagged, world,
/// world behind the eye) through the engine's ink layout, the outputs taken by stream output and compared bit for bit
/// -- every variant, the three eyes, a matching and a missing marker camera, plain, text-depth and world-text objects.
#[test]
fn world_marker_vertex_shaders_alike() {
    use crate::bench::{Attribute, Captured};
    let mut b = Bench::warp();
    let root = {
        let cbv = |register: u32| D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_CBV,
            Anonymous: D3D12_ROOT_PARAMETER_0 { Descriptor: D3D12_ROOT_DESCRIPTOR { ShaderRegister: register, RegisterSpace: 0 } },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
        };
        let params = [cbv(1), cbv(5), cbv(7)];
        let desc = D3D12_ROOT_SIGNATURE_DESC {
            NumParameters: params.len() as u32,
            pParameters: params.as_ptr(),
            Flags: D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT | D3D12_ROOT_SIGNATURE_FLAG_ALLOW_STREAM_OUTPUT,
            ..Default::default()
        };
        let mut blob = None;
        // SAFETY: the description and its parameters on the stack.
        unsafe {
            D3D12SerializeRootSignature(&desc, D3D_ROOT_SIGNATURE_VERSION_1, &mut blob, None).unwrap();
            let blob = blob.unwrap();
            b.device.CreateRootSignature::<ID3D12RootSignature>(0, core::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize())).unwrap()
        }
    };
    let layout = [
        Attribute { semantic: c"POSITION", index: 0, format: DXGI_FORMAT_R32G32B32_FLOAT, offset: 0 },
        Attribute { semantic: c"TEXCOORD", index: 0, format: DXGI_FORMAT_R32G32_FLOAT, offset: 12 },
        Attribute { semantic: c"COLOR", index: 0, format: DXGI_FORMAT_R32G32B32A32_FLOAT, offset: 20 },
    ];
    // plain, depth-tagged (2.5 and the range's end), world, world behind the eye, plain
    let vertices: [[f32; 9]; 6] = [
        [12.5, -7.25, 0.375, 0.25, 0.75, 0.9, 0.1, 0.2, 0.8],
        [-30.0, 44.0, -65536.0 - 4096.0 * 2.5, 0.5, 0.5, 0.3, 0.6, 0.9, 1.0],
        [104.0, -12.0, 7.5, 0.125, 0.875, 0.4, 0.5, 0.6, -0.7],
        [3.0, 9.0, -147456.0, 1.0, 0.0, 0.2, 0.2, 0.2, 0.5],
        [96.0, -40.0, -3.0, 0.0, 1.0, 0.7, 0.8, 0.9, -1.0],
        [-0.5, 0.25, 1.5, 0.3, 0.6, 1.0, 0.0, 0.5, 0.25],
    ];
    let vertex_bytes = floats(&vertices.concat());
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../src/Render/WorldMarkers.hlsl");
    let source = std::fs::read_to_string(&path).unwrap();
    let variants: [(&str, &'static [u8], u8); 6] = [
        ("SpriteMain", crate::dxil::WORLD_MARKER_SPRITE_VS, 3),
        ("ProceduralMain", crate::dxil::WORLD_MARKER_PROCEDURAL_VS, 4),
        ("SpriteStereoMain", crate::dxil::WORLD_MARKER_SPRITE_STEREO_VS, 3),
        ("TextStereoMain", crate::dxil::WORLD_MARKER_TEXT_STEREO_VS, 3),
        ("SpriteStereoZMain", crate::dxil::WORLD_MARKER_SPRITE_STEREO_Z_VS, 3),
        ("SpriteStereoOneMain", crate::dxil::WORLD_MARKER_SPRITE_STEREO_ONE_VS, 3),
    ];
    let objects: [[f32; 4]; 3] = [[0.25, -0.5, 2.0, 1.5], [0.0, 0.0, 1.0, -65536.0 - 4096.0 * 3.0], [0.1, 0.2, -16384.0, 4.0]];
    for (entry, rust, colour) in variants {
        let strict = bench::dxc_with(&source, entry, "vs_6_0", &["-Gis"]);
        let fast = bench::dxc(&source, entry, "vs_6_0");
        let captured = [
            Captured { semantic: c"SV_Position", index: 0, start: 0, count: 4 },
            Captured { semantic: c"TEXCOORD", index: 0, start: 0, count: 4 },
            Captured { semantic: c"TEXCOORD", index: 1, start: 0, count: colour },
        ];
        let mut apart = 0;
        let mut total = 0;
        for eye in [0i32, 1, 2] {
            for (n, row2) in objects.iter().enumerate() {
                for marker in [538012.0f32, 1.0] {
                    // CameraShaderConsts: the 53 engine rows, the eye, padding, the marker camera
                    let mut camera = rows(201 + n as u32, 53, &[(0, [1.2, 0.05, 0.02, 0.3]), (1, [0.03, 1.6, -0.01, -0.2]), (2, [0.0, 0.01, 0.4, 0.5]), (3, [0.0, 0.0, 0.05, 1.0])]);
                    camera.push(f32::from_bits(eye as u32));
                    camera.extend([0.0; 3]);
                    camera.extend([1.0, -2.0, 0.5, marker, 0.99, 0.1, 0.0, 0.0, -0.1, 0.99, 0.05, 0.0, 0.02, 0.0, 0.999, 0.0, 1.4, 1.9, 0.01, -0.02]);
                    let object = rows(301 + n as u32, 7, &[(0, [1.1, 0.1, 0.0, 3.0]), (1, [-0.1, 0.9, 0.0, -2.0]), (2, *row2)]);
                    let material = rows(401 + eye as u32, 32, &[(1, [0.045, 0.6, 0.025, 0.0]), (2, [-0.03, 0.0, 0.0, 0.0])]);
                    let cbvs = [b.constants(&floats(&camera)), b.constants(&floats(&object)), b.constants(&floats(&material))];
                    let args = [Arg::Cbv(0, &cbvs[0]), Arg::Cbv(1, &cbvs[1]), Arg::Cbv(2, &cbvs[2])];
                    let mut results = Vec::new();
                    for code in [bench::code(&strict), bench::dxil(rust), bench::code(&fast)] {
                        results.push(b.stream_out(&root, code, &layout, &vertex_bytes, 36, &captured, &args));
                    }
                    let what = format!("{entry}, eye {eye}, object {n}, marker {marker}");
                    let differ = results[0].iter().zip(&results[1]).filter(|(x, y)| x != y).count();
                    assert_eq!(differ, 0, "{what}: {differ} of {} bytes differ (first at {:?})", results[0].len(), results[0].iter().zip(&results[1]).position(|(x, y)| x != y));
                    apart += results[2].iter().zip(&results[1]).filter(|(x, y)| x != y).count();
                    total += results[1].len();
                }
            }
        }
        println!("{entry}: the fast-math DXIL {apart} of {total} bytes apart");
    }
}

/// The scanner outline (rust/shaders/passes/src/vision.rs) against src/Shaders/vision_highlight_cs.hlsl compiled by DXC
/// IEEE-strict, as the builder compiles it: depths with silhouettes, strengths, highlight flags of every kind (fill and
/// outline branches, every colour, the through-walls bit, the port's white), at rest and with the halo, both outputs
/// compared bit for bit.
#[test]
fn vision_highlight_alike() {
    let mut b = Bench::warp();
    let (w, h) = (40u32, 24u32);
    // the engine's signature for the pass: b0, b1, b12, b6 root CBVs, t0..t8 and u0..u1 tables, s11 linear clamp
    let root = {
        let cbv = |register: u32| D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_CBV,
            Anonymous: D3D12_ROOT_PARAMETER_0 { Descriptor: D3D12_ROOT_DESCRIPTOR { ShaderRegister: register, RegisterSpace: 0 } },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
        };
        let ranges = [
            D3D12_DESCRIPTOR_RANGE { RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV, NumDescriptors: 9, ..Default::default() },
            D3D12_DESCRIPTOR_RANGE { RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_UAV, NumDescriptors: 2, ..Default::default() },
        ];
        let table = |range: &D3D12_DESCRIPTOR_RANGE| D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
            Anonymous: D3D12_ROOT_PARAMETER_0 { DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE { NumDescriptorRanges: 1, pDescriptorRanges: range } },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
        };
        let params = [cbv(0), cbv(1), cbv(12), cbv(6), table(&ranges[0]), table(&ranges[1])];
        let sampler = D3D12_STATIC_SAMPLER_DESC {
            Filter: D3D12_FILTER_MIN_MAG_MIP_LINEAR,
            AddressU: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
            AddressV: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
            AddressW: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
            MaxAnisotropy: 1,
            ComparisonFunc: D3D12_COMPARISON_FUNC_ALWAYS,
            MaxLOD: D3D12_FLOAT32_MAX,
            ShaderRegister: 11,
            ..Default::default()
        };
        let desc = D3D12_ROOT_SIGNATURE_DESC {
            NumParameters: params.len() as u32,
            pParameters: params.as_ptr(),
            NumStaticSamplers: 1,
            pStaticSamplers: &sampler,
            Flags: D3D12_ROOT_SIGNATURE_FLAG_NONE,
        };
        let mut blob = None;
        // SAFETY: the description and its parameters on the stack.
        unsafe {
            D3D12SerializeRootSignature(&desc, D3D_ROOT_SIGNATURE_VERSION_1, &mut blob, None).unwrap();
            let blob = blob.unwrap();
            b.device.CreateRootSignature::<ID3D12RootSignature>(0, core::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize())).unwrap()
        }
    };
    let texels = (w * h) as usize;
    let single = |b: &mut Bench, seed: u32, lo: f32, hi: f32| {
        let t = b.texture(DXGI_FORMAT_R32_FLOAT, w, h, false);
        let raw = bench::pattern(seed, texels * 2);
        let v: Vec<f32> = (0..texels).map(|i| lo + (u16::from_le_bytes([raw[2 * i], raw[2 * i + 1]]) as f32 / 65536.0) * (hi - lo)).collect();
        b.upload(&t, &floats(&v));
        t
    };
    let t0 = single(&mut b, 11, 0.0, 0.2);
    let t1 = single(&mut b, 12, 0.0, 0.6);
    let t2 = single(&mut b, 13, 0.001, 0.9);
    // the scene depth with a near box in the middle: silhouettes
    let t4 = b.texture(DXGI_FORMAT_R32_FLOAT, w, h, false);
    let depth: Vec<f32> = (0..texels).map(|i| if (12..28).contains(&(i as u32 % w)) && (6..18).contains(&(i as u32 / w)) { 0.8 } else { 0.05 + (i % 7) as f32 * 0.01 }).collect();
    b.upload(&t4, &floats(&depth));
    let t5 = single(&mut b, 15, 0.2, 1.0);
    // the flags (int2: .y read): outlined boxes of every colour on both branches, a through-walls bit, the port's white
    let t6 = b.texture(DXGI_FORMAT_R32G32_SINT, w, h, false);
    let raw = bench::pattern(16, texels);
    let mut flags = Vec::with_capacity(texels * 8);
    for i in 0..texels {
        let (x, y) = (i as u32 % w, i as u32 / w);
        let inside = (12..28).contains(&x) && (6..18).contains(&y);
        let v: u32 = if !inside {
            0
        } else if x < 20 {
            1 | (7 << 2) // the port's white, the outline branch
        } else {
            1 | ((raw[i] as u32 & 1) << 1) | ((raw[i] as u32 >> 1 & 7) << 2) | ((raw[i] as u32 >> 4 & 1) << 6)
        };
        flags.extend_from_slice(&0i32.to_le_bytes());
        flags.extend_from_slice(&(v as i32).to_le_bytes());
    }
    b.upload(&t6, &flags);
    let t7 = b.texture(DXGI_FORMAT_R32G32B32A32_FLOAT, w, h, false);
    b.upload(&t7, &picture(17, texels, -0.02, 0.02, false));
    let t8 = b.texture(DXGI_FORMAT_R32G32B32A32_FLOAT, w, h, false);
    b.upload(&t8, &picture(18, texels, 0.0, 1.0, false));
    let u0 = b.texture(DXGI_FORMAT_R16G16B16A16_FLOAT, w, h, true);
    let u1 = b.texture(DXGI_FORMAT_R16G16B16A16_FLOAT, w, h, true);
    let srvs = [Some(&t0), Some(&t1), Some(&t2), None, Some(&t4), Some(&t5), Some(&t6), Some(&t7), Some(&t8)];
    for (i, t) in srvs.iter().enumerate() {
        b.srv(i as u32, *t, DXGI_FORMAT_R32_FLOAT);
    }
    b.uav(9, Some(&u0), DXGI_FORMAT_UNKNOWN);
    b.uav(10, Some(&u1), DXGI_FORMAT_UNKNOWN);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../src/Shaders/vision_highlight_cs.hlsl");
    let strict = bench::dxc_with(&std::fs::read_to_string(&path).unwrap(), "main", "cs_6_0", &["-Gis"]);
    let shipped = reference("vision_highlight_cs");
    for (n, (halo, bits)) in [(0.0f32, 0u32), (0.6, 0), (0.6, 1)].into_iter().enumerate() {
        let k28 = rows(501, 30, &[(0, [0.0, 0.0, 12.75, 0.0])]);
        let k33 = rows(502, 53, &[(36, [1.0, -2.0, 0.5, 1.0])]);
        let k38 = rows(
            503,
            99,
            &[(79, [1.0, 1.0, 1.0, 1.0]), (27, [w as f32, h as f32, 0.0, 0.0]), (26, [1.0, 0.0, 0.0, 0.0]), (25, [-0.95, 1.0, 0.0, 0.0]), (0, [3.0, 4.0, 5.0, 0.0])],
        );
        let k43 = rows(504, 3, &[(1, [0.0, 0.0, 1.0 / w as f32, 1.0 / h as f32]), (2, [7.25, halo, f32::from_bits(bits), 0.0])]);
        let cbvs = [b.constants(&floats(&k28)), b.constants(&floats(&k33)), b.constants(&floats(&k38)), b.constants(&floats(&k43))];
        let args = [Arg::Cbv(0, &cbvs[0]), Arg::Cbv(1, &cbvs[1]), Arg::Cbv(2, &cbvs[2]), Arg::Cbv(3, &cbvs[3]), Arg::Table(4, 0), Arg::Table(5, 9)];
        let mut results = Vec::new();
        for code in [bench::code(&strict), bench::dxil(crate::dxil::VISION_HIGHLIGHT_CS), bench::code(&shipped)] {
            b.upload(&u0, &vec![0u8; texels * 8]);
            b.upload(&u1, &vec![0u8; texels * 8]);
            let pso = b.pipeline(&root, code);
            b.dispatch_with(&root, &pso, &args, [w.div_ceil(8), h.div_ceil(8), 1]);
            results.push([b.read(&u0), b.read(&u1)]);
        }
        for i in 0..2 {
            let (strict, rust, shipped) = (&results[0][i], &results[1][i], &results[2][i]);
            if std::env::var_os("CVR_BENCH_PIXELS").is_some() {
                let px: Vec<(u32, u32)> = strict.chunks(8).zip(rust.chunks(8)).enumerate().filter(|(_, (a, b))| a != b).map(|(p, _)| (p as u32 % w, p as u32 / w)).collect();
                eprintln!("case {n} u{i}: {} pixels apart: {:?}", px.len(), px);
            }
            let differ = strict.iter().zip(rust).filter(|(x, y)| x != y).count();
            if std::env::var_os("CVR_BENCH_PIXELS").is_some() {
                eprintln!("case {n} u{i}: {differ} bytes apart: {:?}", first_apart(strict, rust));
                continue;
            }
            assert_eq!(differ, 0, "case {n} u{i}: {differ} of {} bytes differ: {:?}", strict.len(), first_apart(strict, rust));
            assert!(rust.iter().any(|&x| x != 0), "case {n} u{i} left empty");
            let apart = shipped.iter().zip(rust).filter(|(x, y)| x != y).count();
            println!("case {n} u{i}: the shipped fast-math DXIL {apart} of {} bytes apart", rust.len());
        }
    }
}

/// A root signature of root CBVs at `cbvs`, then descriptor tables of SRVs (`tables`: register, space, count -- u32::MAX
/// unbounded), with an optional linear clamp sampler at s0, and `flags`.
fn signature(device: &ID3D12Device, cbvs: &[u32], tables: &[(u32, u32, u32)], sampler: bool, flags: D3D12_ROOT_SIGNATURE_FLAGS) -> ID3D12RootSignature {
    let ranges: Vec<D3D12_DESCRIPTOR_RANGE> = tables
        .iter()
        .map(|&(register, space, count)| D3D12_DESCRIPTOR_RANGE {
            RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
            NumDescriptors: count,
            BaseShaderRegister: register,
            RegisterSpace: space,
            OffsetInDescriptorsFromTableStart: 0,
        })
        .collect();
    let mut params: Vec<D3D12_ROOT_PARAMETER> = cbvs
        .iter()
        .map(|&register| D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_CBV,
            Anonymous: D3D12_ROOT_PARAMETER_0 { Descriptor: D3D12_ROOT_DESCRIPTOR { ShaderRegister: register, RegisterSpace: 0 } },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
        })
        .collect();
    for range in &ranges {
        params.push(D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
            Anonymous: D3D12_ROOT_PARAMETER_0 { DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE { NumDescriptorRanges: 1, pDescriptorRanges: range } },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
        });
    }
    let linear = D3D12_STATIC_SAMPLER_DESC {
        Filter: D3D12_FILTER_MIN_MAG_MIP_LINEAR,
        AddressU: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
        AddressV: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
        AddressW: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
        MaxAnisotropy: 1,
        ComparisonFunc: D3D12_COMPARISON_FUNC_ALWAYS,
        MaxLOD: D3D12_FLOAT32_MAX,
        ..Default::default()
    };
    let desc = D3D12_ROOT_SIGNATURE_DESC {
        NumParameters: params.len() as u32,
        pParameters: params.as_ptr(),
        NumStaticSamplers: sampler as u32,
        pStaticSamplers: &linear,
        Flags: flags,
    };
    let mut blob = None;
    let mut error = None;
    // SAFETY: the description and its parameters on the stack.
    unsafe {
        if D3D12SerializeRootSignature(&desc, D3D_ROOT_SIGNATURE_VERSION_1, &mut blob, Some(&mut error)).is_err() {
            let e = error.unwrap();
            panic!("root signature: {}", String::from_utf8_lossy(core::slice::from_raw_parts(e.GetBufferPointer() as *const u8, e.GetBufferSize())));
        }
        let blob = blob.unwrap();
        device.CreateRootSignature(0, core::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize())).unwrap()
    }
}

/// The sight's vertex shader (rust/shaders/passes/src/sight.rs) against src/Shaders/sight_reflex_vs.hlsl compiled by DXC
/// IEEE-strict: four bones read as raw dwords from a typed Buffer<uint4> over R32_UINT, the instance placement in
/// fixed point, the shadow pass's collapse, every output by stream output -- the slab's thin axis along each axis.
#[test]
fn sight_vertex_shader_alike() {
    use crate::bench::{Attribute, Captured};
    let mut b = Bench::warp();
    let root = signature(
        &b.device,
        &[7, 0, 1, 5],
        &[(10, 0, 1)],
        false,
        D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT | D3D12_ROOT_SIGNATURE_FLAG_ALLOW_STREAM_OUTPUT,
    );
    let layout = [
        Attribute { semantic: c"POSITION", index: 0, format: DXGI_FORMAT_R32G32B32_FLOAT, offset: 0 },
        Attribute { semantic: c"BLENDINDICES", index: 0, format: DXGI_FORMAT_R32G32B32A32_UINT, offset: 12 },
        Attribute { semantic: c"BLENDWEIGHT", index: 0, format: DXGI_FORMAT_R32G32B32A32_FLOAT, offset: 28 },
        Attribute { semantic: c"TEXCOORD", index: 0, format: DXGI_FORMAT_R32G32_FLOAT, offset: 44 },
        Attribute { semantic: c"NORMAL", index: 0, format: DXGI_FORMAT_R32G32B32_FLOAT, offset: 52 },
        Attribute { semantic: c"TANGENT", index: 0, format: DXGI_FORMAT_R32G32B32A32_FLOAT, offset: 64 },
        Attribute { semantic: c"COLOR", index: 0, format: DXGI_FORMAT_R32G32B32A32_FLOAT, offset: 80 },
        Attribute { semantic: c"TEXCOORD", index: 1, format: DXGI_FORMAT_R32G32_FLOAT, offset: 96 },
        Attribute { semantic: c"INSTANCE_TRANSFORM", index: 0, format: DXGI_FORMAT_R32G32B32A32_FLOAT, offset: 104 },
        Attribute { semantic: c"INSTANCE_TRANSFORM", index: 1, format: DXGI_FORMAT_R32G32B32A32_FLOAT, offset: 120 },
        Attribute { semantic: c"INSTANCE_TRANSFORM", index: 2, format: DXGI_FORMAT_R32G32B32A32_FLOAT, offset: 136 },
        Attribute { semantic: c"INSTANCE_SKINNING_DATA", index: 0, format: DXGI_FORMAT_R32G32B32A32_UINT, offset: 152 },
    ];
    let rebase = [0x4000_0000u32, 0x3fff_0000, 0x4000_8000, 0];
    let mut vertex = Vec::new();
    let raw = bench::pattern(77, 6 * 64);
    for v in 0..6usize {
        let f = |i: usize| raw[v * 64 + i] as f32 / 255.0;
        let put = |vertex: &mut Vec<u8>, x: &[f32]| x.iter().for_each(|x| vertex.extend_from_slice(&x.to_le_bytes()));
        put(&mut vertex, &[f(0) * 2.0 - 1.0, f(1) * 2.0 - 1.0, f(2) * 2.0 - 1.0]);
        for i in 0..4u32 {
            vertex.extend_from_slice(&((v as u32 + i) % 4).to_le_bytes());
        }
        put(&mut vertex, &[0.4 + f(3) * 0.2, 0.3, 0.2 + f(4) * 0.1, 0.1]);
        put(&mut vertex, &[f(5), f(6)]);
        put(&mut vertex, &[f(7), f(8), 0.9]);
        put(&mut vertex, &[f(9), 0.2, f(10), if v % 2 == 0 { 1.0 } else { 0.0 }]);
        put(&mut vertex, &[0.5, 0.5, 0.5, 1.0]);
        put(&mut vertex, &[0.25, 0.75]);
        let t = |k: u32, row: usize| f32::from_bits(rebase[row].wrapping_add(131072 * k + v as u32 * 977));
        put(&mut vertex, &[0.99, 0.05, -0.02, t(3, 0)]);
        put(&mut vertex, &[-0.05, 0.98, 0.1, t(5, 1)]);
        put(&mut vertex, &[0.03, -0.1, 0.97, t(7, 2)]);
        for x in [16u32, 48, 0, 0] {
            vertex.extend_from_slice(&x.to_le_bytes());
        }
    }
    // the bones: 4 matrices of 3 rows, 48 bytes apart from byte 16
    let mut bones: Vec<f32> = vec![0.0; 4 + 4 * 12];
    for bone in 0..4 {
        let at = 4 + bone * 12;
        let s = 1.0 + bone as f32 * 0.05;
        bones[at..at + 12].copy_from_slice(&[s, 0.01, 0.0, 0.1 * bone as f32, -0.01, s, 0.02, 0.05, 0.0, -0.02, s, -0.03]);
    }
    let bone_bytes = floats(&bones);
    let buffer = b.constants(&bone_bytes);
    b.buffer_srv(0, &buffer, DXGI_FORMAT_R32_UINT, (bone_bytes.len() / 4) as u32);
    let captured = [
        Captured { semantic: c"SV_Position", index: 0, start: 0, count: 4 },
        Captured { semantic: c"TEXCOORD", index: 0, start: 0, count: 4 },
        Captured { semantic: c"TEXCOORD", index: 1, start: 0, count: 4 },
        Captured { semantic: c"TEXCOORD", index: 2, start: 0, count: 4 },
        Captured { semantic: c"TEXCOORD", index: 3, start: 0, count: 4 },
        Captured { semantic: c"TEXCOORD", index: 4, start: 0, count: 4 },
        Captured { semantic: c"SV_ClipDistance", index: 0, start: 0, count: 1 },
    ];
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../src/Shaders/sight_reflex_vs.hlsl");
    let strict = bench::dxc_with(&std::fs::read_to_string(&path).unwrap(), "main", "vs_6_0", &["-Gis"]);
    let shipped = reference("sight_reflex_vs");
    for (n, (boxes, shadow)) in [([0.0128f32, 0.0001, 0.0133], false), ([0.0002, 0.012, 0.013], false), ([0.012, 0.013, 0.0001], true)].into_iter().enumerate() {
        let k15 = rows(601, 32, &[(1, [if shadow { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0])]);
        let k20 = rows(602, 33, &[(31, [0.0, f32::from_bits(2), 0.0, 0.0])]);
        let mut k25 = rows(603, 53, &[(36, [1.5, -0.25, 0.75, 0.0]), (37, [1.6, -0.2, 0.7, 0.0]), (50, [0.0, 0.0, 1.0, 0.5])]);
        for (i, r) in rebase.iter().enumerate() {
            k25[38 * 4 + i] = f32::from_bits(*r);
        }
        let k30 = rows(604, 7, &[(4, [boxes[0], boxes[1], boxes[2], 0.0]), (5, [0.001, -0.002, 0.026, 0.0])]);
        let cbvs = [b.constants(&floats(&k15)), b.constants(&floats(&k20)), b.constants(&floats(&k25)), b.constants(&floats(&k30))];
        let args = [Arg::Cbv(0, &cbvs[0]), Arg::Cbv(1, &cbvs[1]), Arg::Cbv(2, &cbvs[2]), Arg::Cbv(3, &cbvs[3]), Arg::Table(4, 0)];
        let mut results = Vec::new();
        for code in [bench::code(&strict), bench::dxil(crate::dxil::SIGHT_REFLEX_VS), bench::code(&shipped)] {
            results.push(b.stream_out(&root, code, &layout, &vertex, 168, &captured, &args));
        }
        let differ = results[0].iter().zip(&results[1]).filter(|(x, y)| x != y).count();
        assert_eq!(differ, 0, "case {n}: {differ} of {} bytes differ (first at {:?})", results[0].len(), results[0].iter().zip(&results[1]).position(|(x, y)| x != y));
        let apart = results[2].iter().zip(&results[1]).filter(|(x, y)| x != y).count();
        println!("sight VS case {n}: the shipped fast-math DXIL {apart} of {} bytes apart", results[1].len());
    }
}

/// The sight's pixel shaders (rust/shaders/passes/src/sight_ps.rs: the collimator build and the scope build) against
/// src/Shaders/sight_reflex_ps.hlsl compiled by DXC IEEE-strict with the build's defines, drawn with a vertex shader
/// giving the sight VS's outputs: the collimator with and without the glitch and flipped, the camera surfaces (image,
/// composite with its labels and aim mark, marks with front and rear panels, filled and projected), and in the scope
/// build the scope panels (front and rear, filled and projected).
#[test]
fn sight_pixel_shaders_alike() {
    let mut b = Bench::warp();
    let root = signature(&b.device, &[0, 4], &[(0, 0, 1), (0, 1, u32::MAX)], true, D3D12_ROOT_SIGNATURE_FLAG_NONE);
    let vertex = r#"
struct O { float4 pos : SV_Position; float4 t0 : TEXCOORD0; float4 t1 : TEXCOORD1; float4 t2 : TEXCOORD2;
           float4 t3 : TEXCOORD3; float4 t4 : TEXCOORD4; };
O main(uint id : SV_VertexID) {
    float2 uv = float2((id << 1) & 2, id & 2); float2 ndc = uv * float2(2, -2) + float2(-1, 1);
    O o; o.pos = float4(ndc, 0.5, 1);
    o.t0 = float4(uv * 0.5, 0.3, 0);
    o.t1 = float4(0.0256, 0.0265, 0.004, -0.003);
    o.t2 = float4(0.02, -0.01, 0.9997, 0.01);
    o.t3 = float4(0.999, -0.01, 0.9998, 0.015);
    o.t4 = float4(-0.02, (uv.x - 0.25) * 0.04 + 0.002, (uv.y - 0.25) * 0.04 - 0.001, 0.08 + uv.x * 0.01);
    return o;
}
"#;
    let vs = bench::dxc(vertex, "main", "vs_6_0");
    let atlas = b.texture(DXGI_FORMAT_R8G8B8A8_UNORM, 64, 64, false);
    // the reticle atlas: a soft dot and hairlines in each quarter, a dim backdrop, glyph strokes
    let mut texels = Vec::with_capacity(64 * 64 * 4);
    let noise = bench::pattern(91, 64 * 64);
    for y in 0..64 {
        for x in 0..64 {
            let (u, v) = ((x % 32) as f32 - 15.5, (y % 32) as f32 - 15.5);
            let dot = (1.0 - (u * u + v * v) / 30.0).max(0.0);
            let hair = if u.abs() < 0.8 || v.abs() < 0.8 { 0.7 } else { 0.0 };
            let level = (dot + hair + noise[y * 64 + x] as f32 / 255.0 * 0.08).min(1.0);
            texels.extend_from_slice(&[(level * 255.0) as u8, (level * 180.0) as u8, (level * 60.0 + 10.0) as u8, (level * 255.0) as u8]);
        }
    }
    b.upload(&atlas, &texels);
    let noise_map = b.texture(DXGI_FORMAT_R8G8B8A8_UNORM, 32, 32, false);
    b.upload(&noise_map, &bench::pattern(92, 32 * 32 * 4));
    b.srv(0, Some(&atlas), DXGI_FORMAT_UNKNOWN);
    b.srv(1, Some(&noise_map), DXGI_FORMAT_UNKNOWN);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../src/Shaders/sight_reflex_ps.hlsl");
    let source = std::fs::read_to_string(&path).unwrap();
    let target = b.target(DXGI_FORMAT_R32G32B32A32_FLOAT, 48, 32);
    for (scope, rust, shipped, cases) in [
        (false, crate::dxil::SIGHT_REFLEX_PS, "sight_reflex_ps", &[1.0f32, -715.25, -713.25, -714.25, -716.25, -717.25, -718.25][..]),
        (true, crate::dxil::SIGHT_REFLEX_PS_SCOPE, "sight_reflex_ps_scope", &[1.0f32, -715.25, -713.25, -716.25, -717.25, -718.25][..]),
    ] {
        let defines: &[&str] = if scope { &["-Gis", "-DSIGHT_SIZE_SCALE=1.000000f", "-DSIGHT_SCOPE_GLASS=1"] } else { &["-Gis", "-DSIGHT_SIZE_SCALE=1.000000f"] };
        let strict = bench::dxc_with(&source, "main", "ps_6_0", defines);
        let shipped = reference(shipped);
        for &scale in cases {
            for (variant, (glitch, flip, fill, rear)) in [(0.0f32, 0.0f32, 0.0f32, 1.0f32), (1.0, 1.0, 2.0, 0.0)].into_iter().enumerate() {
                let g = rows(701, 30, &[(0, [0.43, 0.0, 0.0, 0.0])]);
                let k = rows(
                    702,
                    21,
                    &[
                        (0, [0.9, 0.0, 0.0, 0.0]),
                        (1, [f32::from_bits(0), 0.0, 0.0, 0.0]),
                        (2, [3.0, 0.0, 0.0, 0.0]),
                        (3, [0.4, 0.0, 0.0, 0.0]),
                        (4, [0.0, 0.0, 0.0, 0.0]),
                        (6, [scale, 0.5, 0.5, 1.0]),
                        (7, [if scope { fill } else { 0.1 }, 0.0, 0.0, 0.0]),
                        // the integrated sights' styles: 0 and 3 (-718.25), 2 and 1 (-717.25)
                        (8, if scale == -718.25 { [3.0 * variant as f32, 0.1, 0.075, 0.05] } else if scale == -717.25 { [2.0 - variant as f32, 0.1, 0.075, 0.05] } else if scale == -716.25 { [0.03, 0.02, if fill > 0.0 { -1.5 } else { 0.01 }, 2.0] } else if scope { [0.03, 0.02, 0.01, rear] } else { [1.0, 0.1, 0.075, 0.05] }),
                        (9, [0.5, 0.5, 0.25, 0.75]),
                        (10, [glitch, 0.0, 0.0, 0.0]),
                        (11, [0.8, 0.0, 0.0, 0.0]),
                        (12, [0.5, 0.0, 1.0, 0.5]),
                        (13, [0.025, 0.0, 0.0, 0.0]),
                        (15, [flip, 0.0, 0.0, 0.0]),
                        (16, [0.02, 0.0, 0.0, 0.0]),
                        (17, [1.0, 0.6, 0.3, 0.0]),
                        (18, [if scale == -716.25 { -1.2 } else { 1.0 }, 0.0, 0.0, 0.0]),
                        (19, [0.2, 0.0, 0.0, 0.0]),
                        (20, [1.5, 0.0, 0.0, 0.0]),
                    ],
                );
                let cbvs = [b.constants(&floats(&g)), b.constants(&floats(&k))];
                let args = [Arg::Cbv(0, &cbvs[0]), Arg::Cbv(1, &cbvs[1]), Arg::Table(2, 0), Arg::Table(3, 1)];
                let mut results = Vec::new();
                for ps in [bench::code(&strict), bench::dxil(rust), bench::code(&shipped)] {
                    b.upload(&target, &vec![0u8; 48 * 32 * 16]);
                    let pso = b.graphics(&root, bench::code(&vs), ps, DXGI_FORMAT_R32G32B32A32_FLOAT);
                    b.draw_with(&root, &pso, &target, &args, 3);
                    results.push(b.read(&target));
                }
                let what = format!("scope {scope}, scale {scale}, variant {variant}");
                if std::env::var_os("CVR_BENCH_PIXELS").is_some() {
                    let apart: Vec<(usize, usize, usize, f32, f32)> = results[0]
                        .chunks(4)
                        .zip(results[1].chunks(4))
                        .enumerate()
                        .filter(|(_, (a, b))| a != b)
                        .map(|(i, (a, b))| (i / 4 % 48, i / 4 / 48, i % 4, f32::from_le_bytes(a.try_into().unwrap()), f32::from_le_bytes(b.try_into().unwrap())))
                        .collect();
                    eprintln!("{what}: {apart:?}");
                    continue;
                }
                let differ = results[0].iter().zip(&results[1]).filter(|(x, y)| x != y).count();
                assert_eq!(differ, 0, "{what}: {differ} of {} bytes differ: {:?}", results[0].len(), first_apart(&results[0], &results[1]));
                let apart = results[2].iter().zip(&results[1]).filter(|(x, y)| x != y).count();
                println!("{what}: the shipped fast-math DXIL {apart} of {} bytes apart", results[1].len());
            }
        }
    }
}
