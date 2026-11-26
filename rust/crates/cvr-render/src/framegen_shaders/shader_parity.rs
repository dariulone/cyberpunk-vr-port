//! The frame generator's passes made from Rust (rust/shaders/passes/src/framegen.rs) against their HLSL compiled by
//! FXC, on WARP: the same inputs, every output byte for byte -- the bench the plugin's other passes have in tools/.
use cvr_gpu::bench::{self, Bench, Texture};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

/// framegen_generator's root signature: SRV t0..t2, UAV u0..u2 (a table each), 6 constants at b0, linear clamp s0.
fn root(device: &ID3D12Device) -> ID3D12RootSignature {
    let ranges = [
        D3D12_DESCRIPTOR_RANGE { RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV, NumDescriptors: 3, ..Default::default() },
        D3D12_DESCRIPTOR_RANGE { RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_UAV, NumDescriptors: 3, ..Default::default() },
    ];
    let table = |range: &D3D12_DESCRIPTOR_RANGE| D3D12_ROOT_PARAMETER {
        ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
        Anonymous: D3D12_ROOT_PARAMETER_0 {
            DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE { NumDescriptorRanges: 1, pDescriptorRanges: range },
        },
        ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
    };
    let params = [
        table(&ranges[0]),
        table(&ranges[1]),
        D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS,
            Anonymous: D3D12_ROOT_PARAMETER_0 { Constants: D3D12_ROOT_CONSTANTS { ShaderRegister: 0, RegisterSpace: 0, Num32BitValues: 6 } },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
        },
    ];
    let sampler = D3D12_STATIC_SAMPLER_DESC {
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
        NumParameters: 3,
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
        device.CreateRootSignature(0, core::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize())).unwrap()
    }
}

/// Floats in [lo, hi) from a byte pattern (finite: never a NaN from random bits).
fn floats(seed: u32, count: usize, lo: f32, hi: f32) -> Vec<u8> {
    let raw = bench::pattern(seed, count * 2);
    (0..count)
        .flat_map(|i| {
            let unit = u16::from_le_bytes([raw[2 * i], raw[2 * i + 1]]) as f32 / 65536.0;
            (lo + unit * (hi - lo)).to_le_bytes()
        })
        .collect()
}

struct Case {
    source: (u32, u32),
    dest: (u32, u32),
    motion: (u32, u32),
    reset: u32,
    clear_scd: u32,
}

/// One pass, both ways: the outputs zeroed, the FXC pipeline run and read, zeroed again, the DXIL's run and read.
#[allow(clippy::too_many_arguments)]
fn both(
    b: &mut Bench,
    root: &ID3D12RootSignature,
    fxc: &windows::Win32::Graphics::Direct3D::ID3DBlob,
    dxil: &'static [u8],
    outputs: &[&Texture],
    constants: &[u32; 6],
    groups: [u32; 3],
    what: &str,
    may_be_empty: bool,
) {
    let mut results = Vec::new();
    for code in [bench::bytecode(fxc), bench::dxil(dxil)] {
        for t in outputs {
            b.upload(t, &vec![0u8; (t.width * t.height * t.texel) as usize]);
        }
        let pso = b.pipeline(root, code);
        b.dispatch(root, &pso, &[(0, 0), (1, 3)], Some((2, constants)), groups);
        results.push(outputs.iter().map(|t| b.read(t)).collect::<Vec<_>>());
    }
    for (i, (fxc, rust)) in results[0].iter().zip(&results[1]).enumerate() {
        let differ = fxc.iter().zip(rust).filter(|(a, b)| a != b).count();
        assert_eq!(differ, 0, "{what}: output {i}: {differ} of {} bytes differ (first at {:?})", fxc.len(), fxc.iter().zip(rust).position(|(a, b)| a != b));
        // every image output written (the scene cut's three words may be zero)
        assert!(may_be_empty || rust.iter().any(|&x| x != 0) || outputs[i].width * outputs[i].height <= 3, "{what}: output {i} left empty");
    }
}

#[test]
fn framegen_passes_fxc_and_rust_alike() {
    let mut b = Bench::warp();
    let root = root(&b.device);
    let names = [c"Pack", c"Flow", c"SceneCut"];
    let fxc_main: Vec<_> = names.iter().map(|e| bench::fxc(super::SHADER, e, c"cs_5_0", 0)).collect();
    let fxc_prepare = bench::fxc(super::PREPARE_INPUTS_SHADER, c"PrepareInputs", c"cs_5_0", 0);
    let cases = [
        Case { source: (40, 24), dest: (20, 12), motion: (40, 24), reset: 0, clear_scd: 1 },
        Case { source: (40, 24), dest: (64, 40), motion: (20, 12), reset: 0, clear_scd: 0 },
        Case { source: (37, 23), dest: (19, 11), motion: (37, 23), reset: 1, clear_scd: 1 },
        Case { source: (33, 17), dest: (33, 17), motion: (66, 34), reset: 1, clear_scd: 0 },
    ];
    for (n, c) in cases.iter().enumerate() {
        let (sw, sh) = c.source;
        let (dw, dh) = c.dest;
        let seed = 7 + n as u32 * 11;
        let color = b.texture(DXGI_FORMAT_R8G8B8A8_UNORM, sw, sh, false);
        b.upload(&color, &bench::pattern(seed, (sw * sh * 4) as usize));
        let previous = b.texture(DXGI_FORMAT_R8G8B8A8_UNORM, sw, sh, false);
        // the second case's previous frame the colour inverted: a scene cut
        let mut before = bench::pattern(seed + 1, (sw * sh * 4) as usize);
        if n == 1 {
            before = bench::pattern(seed, (sw * sh * 4) as usize).iter().map(|x| 255 - x).collect();
        }
        b.upload(&previous, &before);
        let raw_flow = b.texture(DXGI_FORMAT_R16G16_SINT, sw, sh, false);
        b.upload(&raw_flow, &bench::pattern(seed + 2, (sw * sh * 4) as usize));
        let packed = b.texture(DXGI_FORMAT_R8G8B8A8_UNORM, dw, dh, true);
        let flow = b.texture(DXGI_FORMAT_R16G16_SINT, dw, dh, true);
        let scd = b.texture(DXGI_FORMAT_R32_UINT, 3, 1, true);
        b.srv(0, Some(&color), DXGI_FORMAT_UNKNOWN);
        b.srv(1, Some(&raw_flow), DXGI_FORMAT_UNKNOWN);
        b.srv(2, Some(&previous), DXGI_FORMAT_UNKNOWN);
        b.uav(3, Some(&packed), DXGI_FORMAT_UNKNOWN);
        b.uav(4, Some(&flow), DXGI_FORMAT_UNKNOWN);
        b.uav(5, Some(&scd), DXGI_FORMAT_UNKNOWN);
        let k = [sw, sh, dw, dh, c.reset, c.clear_scd];
        let groups = [dw.div_ceil(8), dh.div_ceil(8), 1];
        both(&mut b, &root, &fxc_main[0], cvr_gpu::dxil::FRAMEGEN_PACK_CS, &[&packed], &k, groups, &format!("case {n} Pack"), false);
        both(&mut b, &root, &fxc_main[1], cvr_gpu::dxil::FRAMEGEN_FLOW_CS, &[&flow, &scd], &k, groups, &format!("case {n} Flow"), c.reset != 0);
        both(&mut b, &root, &fxc_main[2], cvr_gpu::dxil::FRAMEGEN_SCENE_CUT_CS, &[&scd], &k, [1, 1, 1], &format!("case {n} SceneCut"), false);
        let cut = b.read(&scd);
        let expected_cut = if c.reset != 0 || n == 1 { 15 } else { 0 };
        assert_eq!(u32::from_le_bytes([cut[0], cut[1], cut[2], cut[3]]), expected_cut, "case {n}: the scene cut");

        // PrepareInputs: the depth (inverted in the odd cases) and the motion at another size
        let (mw, mh) = c.motion;
        let depth = b.texture(DXGI_FORMAT_R32_FLOAT, sw, sh, false);
        b.upload(&depth, &floats(seed + 3, (sw * sh) as usize, 0.0, 1.0));
        let motion = b.texture(DXGI_FORMAT_R32G32_FLOAT, mw, mh, false);
        b.upload(&motion, &floats(seed + 4, (mw * mh * 2) as usize, -40.0, 40.0));
        let target_depth = b.texture(DXGI_FORMAT_R32_FLOAT, dw, dh, true);
        let target_motion = b.texture(DXGI_FORMAT_R16G16_FLOAT, dw, dh, true);
        b.srv(0, Some(&depth), DXGI_FORMAT_UNKNOWN);
        b.srv(1, Some(&motion), DXGI_FORMAT_UNKNOWN);
        b.srv(2, None, DXGI_FORMAT_R8G8B8A8_UNORM);
        b.uav(3, Some(&target_depth), DXGI_FORMAT_UNKNOWN);
        b.uav(4, Some(&target_motion), DXGI_FORMAT_UNKNOWN);
        b.uav(5, None, DXGI_FORMAT_R32_UINT);
        let k = [sw, sh, dw, dh, (n % 2) as u32, 0];
        both(
            &mut b,
            &root,
            &fxc_prepare,
            cvr_gpu::dxil::FRAMEGEN_PREPARE_INPUTS_CS,
            &[&target_depth, &target_motion],
            &k,
            groups,
            &format!("case {n} PrepareInputs"),
            false,
        );
    }
}
