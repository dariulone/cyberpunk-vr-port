//! FSR3's dilate and warp and Neural Rendering's codec made from Rust against main's HLSL compiled by FXC (as main's
//! D3DCompile did: cs_5_0, flags 0), on WARP: the same inputs, every output compared.
use cvr_gpu::bench::{self, Bench, Texture};
use windows::Win32::Graphics::Direct3D::ID3DBlob;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

fn serialize(device: &ID3D12Device, desc: &D3D12_ROOT_SIGNATURE_DESC) -> ID3D12RootSignature {
    let mut blob = None;
    // SAFETY: the description and its parameters alive for the call.
    unsafe {
        D3D12SerializeRootSignature(desc, D3D_ROOT_SIGNATURE_VERSION_1, &mut blob, None).unwrap();
        let blob: ID3DBlob = blob.unwrap();
        device.CreateRootSignature(0, core::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize())).unwrap()
    }
}
/// A root signature of 6 constants at b0 and one table (`srvs` SRVs, then `uavs` UAVs), with or without the linear
/// clamp sampler at s0: fsr3.rs's (2 + 5, a sampler) and neural_render.rs's codec (1 + 1).
fn root(device: &ID3D12Device, srvs: u32, uavs: u32, sampler: bool) -> ID3D12RootSignature {
    let ranges = [
        D3D12_DESCRIPTOR_RANGE { RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV, NumDescriptors: srvs, ..Default::default() },
        D3D12_DESCRIPTOR_RANGE {
            RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_UAV,
            NumDescriptors: uavs,
            OffsetInDescriptorsFromTableStart: srvs,
            ..Default::default()
        },
    ];
    let params = [
        D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS,
            Anonymous: D3D12_ROOT_PARAMETER_0 { Constants: D3D12_ROOT_CONSTANTS { ShaderRegister: 0, RegisterSpace: 0, Num32BitValues: 6 } },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
        },
        D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
            Anonymous: D3D12_ROOT_PARAMETER_0 {
                DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE { NumDescriptorRanges: 2, pDescriptorRanges: ranges.as_ptr() },
            },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
        },
    ];
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
        NumParameters: 2,
        pParameters: params.as_ptr(),
        NumStaticSamplers: u32::from(sampler),
        pStaticSamplers: if sampler { &linear } else { core::ptr::null() },
        Flags: D3D12_ROOT_SIGNATURE_FLAG_NONE,
    };
    serialize(device, &desc)
}

/// Floats in [lo, hi) from a byte pattern, as little-endian bytes.
fn floats(seed: u32, count: usize, lo: f32, hi: f32) -> Vec<f32> {
    let raw = bench::pattern(seed, count * 2);
    (0..count).map(|i| lo + u16::from_le_bytes([raw[2 * i], raw[2 * i + 1]]) as f32 / 65536.0 * (hi - lo)).collect()
}
fn bytes(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}
/// An f32 as an IEEE half (round to nearest even; the values here are finite and in the half's normal range or 0).
fn half(v: f32) -> u16 {
    let b = v.to_bits();
    let sign = ((b >> 16) & 0x8000) as u16;
    let exp = ((b >> 23) & 0xff) as i32 - 127 + 15;
    let mant = b & 0x7f_ffff;
    if v == 0.0 || exp <= 0 {
        return sign;
    }
    let mut h = ((exp as u32) << 10) | (mant >> 13);
    let rest = mant & 0x1fff;
    if rest > 0x1000 || (rest == 0x1000 && h & 1 != 0) {
        h += 1;
    }
    sign | h as u16
}
fn halves(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|&v| half(v).to_le_bytes()).collect()
}

/// The bytes of `a` and `b` that differ, and where the first is.
fn differ(a: &[u8], b: &[u8]) -> (usize, Option<usize>) {
    (a.iter().zip(b).filter(|(x, y)| x != y).count(), a.iter().zip(b).position(|(x, y)| x != y))
}

#[test]
fn fsr3_dilate_main_and_rust_alike() {
    let mut b = Bench::warp();
    let root = root(&b.device, 2, 5, true);
    let fxc = [c"Clear", c"ReconstructAndDilate", c"Resolve"].map(|e| bench::fxc(super::DILATE, e, c"cs_5_0", 0));
    let rust = [cvr_gpu::dxil::FSR3_DILATE_CLEAR_CS, cvr_gpu::dxil::FSR3_DILATE_CS, cvr_gpu::dxil::FSR3_DILATE_RESOLVE_CS];
    // (size, motion scale, inverted)
    let cases = [((37u32, 23u32), [1.0f32, 1.0], 1u32), ((37, 23), [1.0, 1.0], 0), ((64, 40), [0.5, -0.5], 1), ((19, 31), [2.0, 1.0], 0)];
    for (n, &((w, h), scale, inverted)) in cases.iter().enumerate() {
        let seed = 3 + n as u32 * 13;
        let depth = b.texture(DXGI_FORMAT_R32_FLOAT, w, h, false);
        b.upload(&depth, &bytes(&floats(seed, (w * h) as usize, 0.0, 1.0)));
        let motion = b.texture(DXGI_FORMAT_R32G32_FLOAT, w, h, false);
        // UV offsets of up to a few pixels, some below the 0.1-pixel cut
        b.upload(&motion, &bytes(&floats(seed + 1, (w * h * 2) as usize, -4.0 / w as f32, 4.0 / w as f32)));
        let dilated_depth = b.texture(DXGI_FORMAT_R32_FLOAT, w, h, true);
        let dilated_motion = b.texture(DXGI_FORMAT_R16G16_FLOAT, w, h, true);
        let scratch = b.texture(DXGI_FORMAT_R32_UINT, w, h, true);
        let previous = b.texture(DXGI_FORMAT_R32_FLOAT, w, h, true);
        b.srv(0, Some(&depth), DXGI_FORMAT_UNKNOWN);
        b.srv(1, Some(&motion), DXGI_FORMAT_UNKNOWN);
        b.uav(2, Some(&dilated_depth), DXGI_FORMAT_UNKNOWN);
        b.uav(3, Some(&dilated_motion), DXGI_FORMAT_UNKNOWN);
        b.uav(4, Some(&scratch), DXGI_FORMAT_UNKNOWN);
        b.uav(5, Some(&previous), DXGI_FORMAT_UNKNOWN);
        b.uav(6, Some(&scratch), DXGI_FORMAT_UNKNOWN);
        let clear = if inverted != 0 { 0.0f32 } else { 1.0 };
        let k = [w, h, scale[0].to_bits(), scale[1].to_bits(), inverted, clear.to_bits()];
        let outputs = [&dilated_depth, &dilated_motion, &scratch, &previous];
        let mut results: Vec<Vec<Vec<u8>>> = Vec::new();
        for variant in 0..2 {
            for t in outputs {
                b.upload(t, &vec![0u8; (t.width * t.height * t.texel) as usize]);
            }
            for pass in 0..3 {
                let code = if variant == 0 { bench::bytecode(&fxc[pass]) } else { bench::dxil(rust[pass]) };
                let pso = b.pipeline(&root, code);
                b.dispatch(&root, &pso, &[(1, 0)], Some((0, &k)), [w.div_ceil(8), h.div_ceil(8), 1]);
            }
            results.push(outputs.iter().map(|t| b.read(t)).collect());
        }
        for (i, (main, ours)) in results[0].iter().zip(&results[1]).enumerate() {
            let (count, first) = differ(main, ours);
            assert_eq!(count, 0, "case {n} (inverted {inverted}): output {i}: {count} of {} bytes differ (first at {first:?})", main.len());
            assert!(ours.iter().any(|&x| x != 0), "case {n}: output {i} left empty");
        }
        // the scatter touched texels other than the far value
        let far = clear.to_bits().to_le_bytes();
        assert!(results[1][2].chunks(4).any(|c| c != far), "case {n}: nothing scattered");
    }
}

#[test]
fn fsr3_warp_main_and_rust_alike() {
    let mut b = Bench::warp();
    let root = root(&b.device, 2, 5, true);
    let fxc = bench::fxc(super::WARP, c"Warp", c"cs_5_0", 0);
    // (display, render, factor)
    let cases = [((50u32, 34u32), (25u32, 17u32), 0.5f32), ((64, 48), (64, 48), 0.5), ((33, 21), (40, 30), 1.0)];
    for (n, &((dw, dh), (rw, rh), factor)) in cases.iter().enumerate() {
        let seed = 41 + n as u32 * 7;
        let color = b.texture(DXGI_FORMAT_R8G8B8A8_UNORM, dw, dh, false);
        b.upload(&color, &bench::pattern(seed, (dw * dh * 4) as usize));
        let motion = b.texture(DXGI_FORMAT_R32G32_FLOAT, rw, rh, false);
        b.upload(&motion, &bytes(&floats(seed + 1, (rw * rh * 2) as usize, -0.08, 0.08)));
        let output = b.texture(DXGI_FORMAT_R8G8B8A8_UNORM, dw, dh, true);
        b.srv(0, Some(&color), DXGI_FORMAT_UNKNOWN);
        b.srv(1, Some(&motion), DXGI_FORMAT_UNKNOWN);
        b.uav(2, Some(&output), DXGI_FORMAT_UNKNOWN);
        for slot in 3..7 {
            b.uav(slot, None, DXGI_FORMAT_R32_FLOAT);
        }
        let k = [dw, dh, rw, rh, factor.to_bits(), 0];
        let mut results = Vec::new();
        for code in [bench::bytecode(&fxc), bench::dxil(cvr_gpu::dxil::FSR3_WARP_CS)] {
            b.upload(&output, &vec![0u8; (dw * dh * 4) as usize]);
            let pso = b.pipeline(&root, code);
            b.dispatch(&root, &pso, &[(1, 0)], Some((0, &k)), [dw.div_ceil(8), dh.div_ceil(8), 1]);
            results.push(b.read(&output));
        }
        let (count, first) = differ(&results[0], &results[1]);
        assert_eq!(count, 0, "case {n}: {count} of {} bytes differ (first at {first:?})", results[0].len());
        assert!(results[1].iter().any(|&x| x != 0), "case {n}: left empty");
    }
}

/// Neural Rendering's codec: the encode (HDR into the SDR proxy) and the decode (the model's result back onto the game's
/// output, which it also loads), RGBA16F as the game's DLSS output before 2.5.
#[test]
fn nr_codec_main_and_rust_alike() {
    let mut b = Bench::warp();
    let root = root(&b.device, 1, 1, false);
    let encode = bench::fxc(super::CODEC, c"Encode", c"cs_5_0", 0);
    let decode = bench::fxc(super::CODEC, c"Decode", c"cs_5_0", 0);
    // (area, offset, paper white, colour strength)
    let cases = [((24u32, 16u32), (0u32, 0u32), 1.0f32, 1.0f32), ((20, 12), (5, 3), 2.5, 0.6), ((17, 9), (2, 7), 0.5, 1.7)];
    for (n, &((w, h), (ox, oy), paper_white, strength)) in cases.iter().enumerate() {
        let seed = 91 + n as u32 * 5;
        let (gw, gh) = (w + ox + 3, h + oy + 2);
        // the game's output: HDR, with some negative and some black pixels
        let mut game = floats(seed, (gw * gh * 4) as usize, -0.5, 12.0);
        for (i, v) in game.iter_mut().enumerate() {
            if i % 37 == 0 {
                *v = 0.0;
            }
        }
        let game_bytes = halves(&game);
        let source = b.texture(DXGI_FORMAT_R16G16B16A16_FLOAT, gw, gh, false);
        b.upload(&source, &game_bytes);
        let proxy = b.texture(DXGI_FORMAT_R16G16B16A16_FLOAT, w, h, true);
        let k = [w, h, paper_white.to_bits(), strength.to_bits(), ox, oy];

        // ENCODE: the game's area -> the proxy
        b.srv(0, Some(&source), DXGI_FORMAT_UNKNOWN);
        b.uav(1, Some(&proxy), DXGI_FORMAT_UNKNOWN);
        let mut encoded = Vec::new();
        for code in [bench::bytecode(&encode), bench::dxil(cvr_gpu::dxil::NR_ENCODE_CS)] {
            b.upload(&proxy, &vec![0u8; (w * h * 8) as usize]);
            let pso = b.pipeline(&root, code);
            b.dispatch(&root, &pso, &[(1, 0)], Some((0, &k)), [w.div_ceil(8), h.div_ceil(8), 1]);
            encoded.push(b.read(&proxy));
        }
        let (count, first) = differ(&encoded[0], &encoded[1]);
        assert_eq!(count, 0, "case {n} Encode: {count} of {} bytes differ (first at {first:?})", encoded[0].len());

        // DECODE: a model result (the proxy, some of it near 1 and some black) -> back onto the game's output
        let mut result = floats(seed + 2, (w * h * 4) as usize, 0.0, 1.0);
        for (i, v) in result.iter_mut().enumerate() {
            if i % 29 == 0 {
                *v = 0.9999;
            } else if i % 31 < 4 {
                *v = 0.0;
            }
        }
        let model = b.texture(DXGI_FORMAT_R16G16B16A16_FLOAT, w, h, false);
        b.upload(&model, &halves(&result));
        let target = b.texture(DXGI_FORMAT_R16G16B16A16_FLOAT, gw, gh, true);
        b.srv(0, Some(&model), DXGI_FORMAT_UNKNOWN);
        b.uav(1, Some(&target), DXGI_FORMAT_UNKNOWN);
        let mut decoded = Vec::new();
        for code in [bench::bytecode(&decode), bench::dxil(cvr_gpu::dxil::NR_DECODE_CS)] {
            b.upload(&target, &game_bytes);
            let pso = b.pipeline(&root, code);
            b.dispatch(&root, &pso, &[(1, 0)], Some((0, &k)), [w.div_ceil(8), h.div_ceil(8), 1]);
            decoded.push(b.read(&target));
        }
        let (count, first) = differ(&decoded[0], &decoded[1]);
        assert_eq!(count, 0, "case {n} Decode: {count} of {} bytes differ (first at {first:?})", decoded[0].len());
        assert_ne!(decoded[1], game_bytes, "case {n}: the decode wrote nothing");
    }
}

#[test]
fn halves_round_to_nearest_even() {
    assert_eq!(half(1.0), 0x3c00);
    assert_eq!(half(-2.0), 0xc000);
    assert_eq!(half(0.5), 0x3800);
    assert_eq!(half(65504.0), 0x7bff);
    assert_eq!(half(0.0), 0);
    // 1 + 2^-11 is halfway between 1 and the next half: to even (1)
    assert_eq!(half(1.0 + 1.0 / 2048.0), 0x3c00);
}
