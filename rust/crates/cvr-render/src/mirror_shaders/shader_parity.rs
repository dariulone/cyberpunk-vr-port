//! The mirror's present conversion made from Rust (rust/shaders/passes/src/mirror.rs) against its HLSL compiled by FXC
//! as the plugin compiled it (vs_5_0 / ps_5_0, no flags), on WARP: an HDR picture -- negative, linear-segment, mid and
//! over-range values -- encoded and dithered into an 8-bit target at several times, every byte compared.
use cvr_gpu::bench::{self, Bench};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

/// mirror.rs's root signature: the SRV table t0, one constant at b0, the linear clamp sampler s0.
fn root(device: &ID3D12Device) -> ID3D12RootSignature {
    let range = D3D12_DESCRIPTOR_RANGE { RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV, NumDescriptors: 1, ..Default::default() };
    let params = [
        D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
            Anonymous: D3D12_ROOT_PARAMETER_0 { DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE { NumDescriptorRanges: 1, pDescriptorRanges: &range } },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
        },
        D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS,
            Anonymous: D3D12_ROOT_PARAMETER_0 { Constants: D3D12_ROOT_CONSTANTS { ShaderRegister: 0, RegisterSpace: 0, Num32BitValues: 1 } },
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
        device.CreateRootSignature(0, core::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize())).unwrap()
    }
}

#[test]
fn mirror_conversion_fxc_and_rust_alike() {
    let mut b = Bench::warp();
    let root = root(&b.device);
    let (w, h) = (64u32, 48u32);
    // the HDR picture: [-0.2, 1.6) in every channel, a band of the linear segment (below 0.0031308) in the first rows
    let raw = bench::pattern(5, (w * h * 4 * 2) as usize);
    let mut texels = Vec::with_capacity((w * h * 16) as usize);
    for i in 0..(w * h * 4) as usize {
        let unit = u16::from_le_bytes([raw[2 * i], raw[2 * i + 1]]) as f32 / 65536.0;
        let row = i / 4 / w as usize;
        let v = if row < 6 { unit * 0.005 - 0.001 } else { unit * 1.8 - 0.2 };
        texels.extend_from_slice(&v.to_le_bytes());
    }
    let picture = b.texture(DXGI_FORMAT_R32G32B32A32_FLOAT, w, h, false);
    b.upload(&picture, &texels);
    b.srv(0, Some(&picture), DXGI_FORMAT_UNKNOWN);
    let target = b.target(DXGI_FORMAT_R8G8B8A8_UNORM, w, h);
    let fxc_vs = bench::fxc(super::CONVERT_HLSL, c"VSMain", c"vs_5_0", 0);
    let fxc_ps = bench::fxc(super::CONVERT_HLSL, c"PSMain", c"ps_5_0", 0);
    let reference = b.graphics(&root, bench::bytecode(&fxc_vs), bench::bytecode(&fxc_ps), DXGI_FORMAT_R8G8B8A8_UNORM);
    let rust = b.graphics(&root, bench::dxil(cvr_gpu::dxil::MIRROR_VS), bench::dxil(cvr_gpu::dxil::MIRROR_PS), DXGI_FORMAT_R8G8B8A8_UNORM);
    for time in [0.0f32, 1.2345, 100.5, 7777.25] {
        let mut results = Vec::new();
        for pso in [&reference, &rust] {
            b.upload(&target, &vec![0u8; (w * h * 4) as usize]);
            b.draw(&root, pso, &target, &[(0, 0)], Some((1, &[time.to_bits()])), 3);
            results.push(b.read(&target));
        }
        let differ = results[0].iter().zip(&results[1]).filter(|(x, y)| x != y).count();
        assert_eq!(differ, 0, "time {time}: {differ} of {} bytes differ (first at {:?})", results[0].len(), results[0].iter().zip(&results[1]).position(|(x, y)| x != y));
        assert!(results[1].chunks(4).all(|p| p[3] == 255), "time {time}: every pixel drawn");
    }
}
