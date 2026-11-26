//! The colour blitter (formerly src/Render/ColorBlit.cpp): a plain or cropped copy, the overlay blends (premultiplied
//! like the engine's HUD, opaque, RGBA replace, additive, straight alpha -- with pixel-exact placement, mip selection,
//! a separable blur and a black point), the engine's HUD composite ported from its PipelineState_576, and the barrel
//! dot. Every draw takes the next slot of one descriptor ring, because the submit path keeps several frames in flight
//! and a descriptor is read when the list executes, not when it is recorded. The shaders stay HLSL, compiled at first
//! use as the C++ did.
use core::ffi::c_void;
use std::sync::Mutex;

use cvr_host::log;
use windows::core::{Interface, PCSTR};
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::{ID3DBlob, D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

const D3DCOMPILE_ENABLE_STRICTNESS: u32 = 1 << 11;

/// ColorBlit::kSlots: 64 sprites plus the mip/blur/style passes of one fenced HUD panel slot.
pub const SLOTS: u32 = 96;
/// The HUD composite's root constants (ColorBlit::HudParams, 31 floats); targetW/targetH are filled here.
pub const HUD_PARAMS: usize = 33;
const HUD_TARGET_W: usize = 22;
const HUD_TARGET_H: usize = 23;

const VERTEX_SHADER: &str = r#"
struct VSOut {
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

VSOut VSMain(uint vid : SV_VertexID) {
    static const float2 positions[4] = {
        float2(-1.0, -1.0),
        float2(-1.0,  1.0),
        float2( 1.0, -1.0),
        float2( 1.0,  1.0)
    };
    static const float2 uvs[4] = {
        float2(0.0, 1.0),
        float2(0.0, 0.0),
        float2(1.0, 1.0),
        float2(1.0, 0.0)
    };
    VSOut o;
    o.position = float4(positions[vid], 0.0, 1.0);
    o.uv = uvs[vid];
    return o;
}
"#;
const PIXEL_SHADER: &str = r#"
Texture2D<float4> g_color : register(t0);
SamplerState g_linear : register(s0);
cbuffer BlitParams : register(b0) {
    float4 g_uvRect;
};

struct VSOut {
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

float4 PSMain(VSOut input) : SV_Target {
    float2 uv = lerp(g_uvRect.xy, g_uvRect.zw, input.uv);
    float4 c = g_color.SampleLevel(g_linear, uv, 0.0);
    return float4(c.rgb, 1.0);
}
"#;
const OVERLAY_SHADER: &str = r#"
Texture2D<float4> g_color : register(t0);
SamplerState g_linear : register(s0);
cbuffer Tint : register(b0) { float4 g_tint; };
cbuffer Filter : register(b1) { float4 g_filter; };

struct VSOut {
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

float4 PSMain(VSOut input) : SV_Target {
    float4 color=g_color.SampleLevel(g_linear,input.uv,0.0);
    if (g_filter.z > 0.5) {
        color *= 0.2270270270;
        const float weights[4]={0.1945945946,0.1216216216,0.0540540541,0.0162162162};
        [unroll] for(int i=1;i<=4;++i)
            color += weights[i-1]*(g_color.SampleLevel(g_linear,input.uv+g_filter.xy*i,0.0)+
                                   g_color.SampleLevel(g_linear,input.uv-g_filter.xy*i,0.0));
    }
    // A black point: light under it is dropped, the rest scaled back to full range and kept in its hue.
    if (g_filter.w > 0.0) {
        const float peak = max(color.r, max(color.g, color.b));
        color.rgb *= saturate((peak - g_filter.w) / max(peak * (1.0 - g_filter.w), 1e-5));
    }
    return color * g_tint;
}
"#;
const HUD_SHADER: &str = r#"
Texture2D<float4> g_scene   : register(t0);
Texture2D<float4> g_hud     : register(t1);
Texture2D<float4> g_hudBlur : register(t2);   // the engine's half-res 4-mip HUD pyramid
ByteAddressBuffer g_expo    : register(t3);   // FrameExposureData; dword 6 is the exposure
SamplerState g_linear : register(s0);

cbuffer Params : register(b0) {
    float2 curvature;
    float2 shadowUV;
    float2 mipOffset1;
    float2 mipOffset2;
    float2 mipOffset4;
    float  glowWeight1;
    float  glowWeight2;
    float  glowWeight4;
    float  aberration;
    float  hudGain;
    float  glowGain;
    float  shadowStrength;
    float  bloomGain;
    float  bloomLod;
    float  shadowMip;
    float  timeSec;
    float  flicker;
    float  targetW;
    float  targetH;
    float  debugMode;
    // Horizontal shift, in UV, applied to the HUD SAMPLE only. Zero puts the HUD at the same
    // pixel in both eyes, which is optical infinity -- and an icon at infinity doubles the moment
    // you converge on a world an arm's length away. See the note where it is computed.
    float  hudShiftU;
    float  layerOnly;
    float  panelBrightness;
    float  panelShadow;
    float  panelGlow;
    float  panelEngineCurve;
};

// The engine's per-frame constants; frameConst[0].x is the time the flicker runs on. Taking it
// from the engine rather than from a clock of ours is what keeps the pattern identical in both
// eyes -- the flicker is spatial, so a phase of our own would make them disagree pixel by pixel.
cbuffer FrameCB : register(b1) {
    float4 frameConst[30];
};

// The composite's own constants. hudConst[16].zw is its target size, which makes the binding
// self-checking: if that is not the surface we are writing, the buffer is not the one we meant
// and the values captured from a 1920x1080 trace are used instead.
cbuffer HudCB : register(b2) {
    float4 hudConst[32];
};

struct VSOut {
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

float4 PSMain(VSOut input) : SV_Target {
    float2 uv = input.uv;

    float2 tgt = float2(targetW, targetH);
    bool cbOk = all(abs(hudConst[16].zw - tgt) < 1.0);

    float2 kCurve   = cbOk ? hudConst[3].xy : curvature;
    float2 kShadow  = cbOk ? hudConst[3].zw : shadowUV;
    float2 kOff1    = cbOk ? hudConst[7].xy : mipOffset1;
    float2 kOff2    = cbOk ? hudConst[7].zw : mipOffset2;
    float2 kOff4    = cbOk ? hudConst[8].xy : mipOffset4;
    float  kW1      = cbOk ? hudConst[8].z  : glowWeight1;
    float  kW2      = cbOk ? hudConst[8].w  : glowWeight2;
    float  kW4      = cbOk ? hudConst[9].x  : glowWeight4;
    float  kAberr   = cbOk ? hudConst[6].w  : aberration;
    float  kSat     = cbOk ? hudConst[6].z  : 1.0;
    float  kGlow    = cbOk ? hudConst[1].x  : glowGain;
    float  kShadowS = cbOk ? hudConst[4].x  : shadowStrength;
    float  kShadowM = cbOk ? hudConst[4].y  : shadowMip;
    float  kBloomG  = cbOk ? hudConst[5].x  : bloomGain;
    float  kBloomL  = cbOk ? hudConst[5].y  : bloomLod;
    float  kVideo   = cbOk ? hudConst[1].z  : 1.0;
    if(layerOnly > 0.5) {kShadowS*=panelShadow;kGlow*=panelGlow;kBloomG*=panelGlow;}
    // The in-eye block's bend is zeroed under the asymmetric projection; the panel keeps the engine's.
    if(layerOnly > 0.5 && panelEngineCurve > 0.5) kCurve = curvature;

    // Bisection switches -- see HudParams::debugMode.
    int   dbg    = (int)(debugMode + 0.5);
    bool  noWarp  = (dbg == 1) || (dbg == 2);
    bool  noGlow  = (dbg == 1) || (dbg == 4);
    bool  noHalo  = (dbg == 1) || (dbg == 3);
    bool  noAberr = (dbg == 1) || (dbg == 5);
    if (noWarp)  { kCurve = float2(0.0, 0.0); }
    if (noAberr) { kAberr = 0.0; }
    if (noGlow)  { kW1 = 0.0; kW2 = 0.0; kW4 = 0.0; }
    if (noHalo)  { kBloomG = 0.0; }

    // _1487 / _1488: the HUD curvature.
    float cx  = uv.x - 0.5;
    float cy2 = (uv.y - 0.5) * 2.0;
    float2 d;
    d.x = uv.x - (cy2 * cy2) * cx  * kCurve.x;
    d.y = uv.y - ((cx * cx) * 2.0) * cy2 * kCurve.y;
    float2 c2 = (d - 0.5) * 2.0;

    // _1531 and _2347: the scanline flicker. Its mean is about 0.66 and it gates the glow, so
    // holding it at 1 is not neutral -- it makes the halo half again too strong.
    float t   = frameConst[0].x;
    float k   = t * 0.005;
    float st  = sin(t);
    float x1  = saturate(d.x) + 1.0;
    float yy  = (saturate(d.y) + 1.0) - k;
    float cy  = saturate(abs(cos(yy * 650.0)));
    float f   = saturate(((st * 0.2 + 0.54)
              + (saturate(abs(cos((x1 + k) * 550.0))) + saturate(abs(cos((x1 - k) * 250.0)))) * 0.18)
              * (cy * 0.4 + 0.6));
    f = lerp(1.0, f, flicker);
    float s2   = st * 0.01;
    float scan = (saturate((cos((s2 + d.y) * 1700.0) + 1.0) * 0.75) * 0.0085 + 0.0015)
               * ((s2 + 0.54 + st * 0.1)
                  + (saturate(abs(cos((x1 + k) * 350.0))) + saturate(abs(cos((x1 - k) * 350.0)))) * 0.18)
               + 0.99;
    scan = lerp(1.0, scan, flicker);

    // GIVE THE HUD A DISTANCE. Applied here, AFTER the flicker and scanline terms are derived
    // from the unshifted d: those are a screen-space pattern the engine lays over both eyes
    // alike, so moving them would only make the two eyes disagree pixel by pixel. Only what is
    // sampled OUT of the HUD surface moves.
    // su (the shadow tap) is derived from d further down, so it follows without a second shift.
    d.x += hudShiftU;

    // Chromatic aberration: R, G and B come from three different taps.
    float2 ao = kAberr * c2;
    float4 A = g_hud.SampleLevel(g_linear, d + ao, 0.0);
    float4 B = g_hud.SampleLevel(g_linear, d,      0.0);
    float4 C = g_hud.SampleLevel(g_linear, d - ao, 0.0);

    float4 G1 = g_hud.SampleLevel(g_linear, d + kOff1 * c2, 1.0);
    float4 G2 = g_hud.SampleLevel(g_linear, d + kOff2 * c2, 2.0);
    float4 G4 = g_hud.SampleLevel(g_linear, d + kOff4 * c2, 4.0);

    float cover = 1.0 - (A.w + B.w + C.w) * 0.3333333333;   // _2175
    float3 glow = saturate(f * (kW2 * G2.rgb + kW1 * G1.rgb + kW4 * G4.rgb)) * cover;
    float3 hud  = float3(A.x, B.y, C.z) * hudGain * scan;

    // _1472: the frame exposure every HUD term is scaled by.
    float expo = (kVideo * (asfloat(g_expo.Load(24)) - 1.0)) + 1.0;

    // _2318: the shadow the HUD casts, mixing its own alpha with the pyramid's.
    float2 su = d - kShadow;
    float aSharp = g_hud.SampleLevel(g_linear, su, 0.0).w;
    float aBlur  = g_hudBlur.SampleLevel(g_linear, su, max(kShadowM - 1.0, 0.0)).w;
    float aMix   = lerp(aSharp, aBlur, saturate(kShadowM));
    float sh = 1.0 - kShadowS * saturate(pow(saturate(aMix), 0.82));

    // Scene is loaded undistorted, exactly as the engine does (_8.Load at the raw pixel).
    float3 scene = layerOnly > 0.5 ? float3(0,0,0) : g_scene.SampleLevel(g_linear, uv, 0.0).rgb;
    glow *= 1.0 - dot(scene, float3(0.2126, 0.7152, 0.0722)) * 0.7;   // _2263

    float hudL  = dot(hud,  float3(0.2126, 0.7152, 0.0722));
    float glowL = dot(glow, float3(0.2126, 0.7152, 0.0722));
    hud  = (hud  - hudL)  * kSat + hudL;
    glow = (glow - glowL) * kSat + glowL;

    // _2381 * _2379: the wide HUD halo, added outside the coverage/shadow attenuation.
    float3 halo = g_hudBlur.SampleLevel(g_linear, d, kBloomL).rgb * (kBloomG * expo);

    hud *= expo;
    if(layerOnly > 0.5) {
        // Factor the native composite into premultiplied color + transmission.
        // The runtime supplies the scene behind this transparent HUD layer.
        float transmission=saturate(cover*sh);
        float3 color=(hud + glow*(kGlow*expo)*transmission + halo)*panelBrightness;
        return float4(color,1.0-transmission);
    }
    return float4(hud + (glow * (kGlow * expo) + scene) * cover * sh + halo, 1.0);
}
"#;
const DOT_SHADER: &str = r#"
cbuffer DotParams : register(b0) {
    float2 centerNdc;    // -1..1, +Y up
    float2 radiusNdc;    // half-extent per axis, already aspect-corrected by the caller
    float4 color;        // straight alpha
};

struct VSOut {
    float4 position : SV_Position;
    float2 local    : TEXCOORD0;   // -1..1 across the quad
};

VSOut VSMain(uint vid : SV_VertexID) {
    static const float2 corners[4] = {
        float2(-1.0, -1.0), float2(-1.0, 1.0), float2(1.0, -1.0), float2(1.0, 1.0)
    };
    VSOut o;
    o.local = corners[vid];
    o.position = float4(centerNdc + corners[vid] * radiusNdc, 0.0, 1.0);
    return o;
}

float4 PSMain(VSOut i) : SV_Target {
    const float d = length(i.local);
    // One-pixel feather from the quad's own derivatives. Safe here in a way it was not in the
    // sight shader: this pass runs on the finished image, after upscaling, so there is no
    // sub-pixel jitter to make the derivative wobble frame to frame.
    const float aa = max(fwidth(d), 1e-4);
    const float cov = 1.0 - smoothstep(1.0 - aa, 1.0, d);
    if (cov <= 0.0) discard;
    return float4(color.rgb, color.a * cov);
}
"#;

fn blob_text(b: &Option<ID3DBlob>) -> String {
    match b {
        // SAFETY: the blob's own buffer.
        Some(b) => unsafe { String::from_utf8_lossy(core::slice::from_raw_parts(b.GetBufferPointer() as *const u8, b.GetBufferSize())).into_owned() },
        None => String::new(),
    }
}
fn compile(source: &str, entry: &core::ffi::CStr, target: &core::ffi::CStr) -> Option<ID3DBlob> {
    let (mut code, mut errors): (Option<ID3DBlob>, Option<ID3DBlob>) = (None, None);
    // SAFETY: the source's bytes and length, NUL-terminated entry and target, out-blobs.
    let hr = unsafe {
        D3DCompile(
            source.as_ptr() as *const c_void,
            source.len(),
            PCSTR::null(),
            None,
            None,
            PCSTR(entry.as_ptr() as *const u8),
            PCSTR(target.as_ptr() as *const u8),
            D3DCOMPILE_ENABLE_STRICTNESS,
            0,
            &mut code,
            Some(&mut errors),
        )
    };
    if let Err(e) = hr {
        log!(
            "ColorBlit: shader compile failed {}/{} hr=0x{:08X} {}\n",
            entry.to_string_lossy(),
            target.to_string_lossy(),
            e.code().0 as u32,
            blob_text(&errors)
        );
        return None;
    }
    code
}

/// A root signature from its description; Err carries the serializer's message.
fn root_signature(device: &ID3D12Device, desc: &D3D12_ROOT_SIGNATURE_DESC) -> Result<ID3D12RootSignature, Option<ID3DBlob>> {
    let (mut blob, mut errors): (Option<ID3DBlob>, Option<ID3DBlob>) = (None, None);
    // SAFETY: the caller's description, whose arrays outlive the call; the live device.
    unsafe {
        if D3D12SerializeRootSignature(desc, D3D_ROOT_SIGNATURE_VERSION_1, &mut blob, Some(&mut errors)).is_err() {
            return Err(errors);
        }
        let blob = blob.ok_or(None)?;
        let bytes = core::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize());
        device.CreateRootSignature::<ID3D12RootSignature>(0, bytes).map_err(|_| None)
    }
}

fn bytecode(b: &ID3DBlob) -> D3D12_SHADER_BYTECODE {
    // SAFETY: the blob's own buffer, alive while the description is used.
    unsafe {
        D3D12_SHADER_BYTECODE {
            pShaderBytecode: b.GetBufferPointer(),
            BytecodeLength: b.GetBufferSize(),
        }
    }
}

/// The fullscreen-quad pipeline every variant starts from: no blend, no depth, solid, no culling. The description
/// borrows the root signature (no reference of its own).
fn pipeline(root: &ID3D12RootSignature, vs: D3D12_SHADER_BYTECODE, ps: D3D12_SHADER_BYTECODE, format: DXGI_FORMAT) -> D3D12_GRAPHICS_PIPELINE_STATE_DESC {
    let mut pso = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
        // SAFETY: the same pointer without an AddRef; the field is ManuallyDrop and never releases it.
        pRootSignature: unsafe { core::mem::transmute_copy(root) },
        VS: vs,
        PS: ps,
        PrimitiveTopologyType: D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE,
        NumRenderTargets: 1,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        SampleMask: u32::MAX,
        ..Default::default()
    };
    pso.RTVFormats[0] = format;
    pso.RasterizerState.FillMode = D3D12_FILL_MODE_SOLID;
    pso.RasterizerState.CullMode = D3D12_CULL_MODE_NONE;
    pso.BlendState.RenderTarget[0].RenderTargetWriteMask = D3D12_COLOR_WRITE_ENABLE_ALL.0 as u8;
    pso
}
fn blend(pso: &mut D3D12_GRAPHICS_PIPELINE_STATE_DESC, src: D3D12_BLEND, dst: D3D12_BLEND, src_alpha: D3D12_BLEND, dst_alpha: D3D12_BLEND) {
    let rt = &mut pso.BlendState.RenderTarget[0];
    rt.BlendEnable = true.into();
    rt.SrcBlend = src;
    rt.DestBlend = dst;
    rt.BlendOp = D3D12_BLEND_OP_ADD;
    rt.SrcBlendAlpha = src_alpha;
    rt.DestBlendAlpha = dst_alpha;
    rt.BlendOpAlpha = D3D12_BLEND_OP_ADD;
    rt.RenderTargetWriteMask = D3D12_COLOR_WRITE_ENABLE_ALL.0 as u8;
}
fn make_pso(device: &ID3D12Device, desc: &D3D12_GRAPHICS_PIPELINE_STATE_DESC) -> Option<ID3D12PipelineState> {
    // SAFETY: the live device and a description whose pointers outlive the call.
    unsafe { device.CreateGraphicsPipelineState(desc).ok() }
}

fn linear_clamp() -> D3D12_STATIC_SAMPLER_DESC {
    D3D12_STATIC_SAMPLER_DESC {
        Filter: D3D12_FILTER_MIN_MAG_MIP_LINEAR,
        AddressU: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
        AddressV: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
        AddressW: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
        ShaderRegister: 0,
        ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
        MaxLOD: D3D12_FLOAT32_MAX,
        ..Default::default()
    }
}
fn constants(register: u32, count: u32, visibility: D3D12_SHADER_VISIBILITY) -> D3D12_ROOT_PARAMETER {
    D3D12_ROOT_PARAMETER {
        ParameterType: D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS,
        Anonymous: D3D12_ROOT_PARAMETER_0 {
            Constants: D3D12_ROOT_CONSTANTS {
                ShaderRegister: register,
                RegisterSpace: 0,
                Num32BitValues: count,
            },
        },
        ShaderVisibility: visibility,
    }
}
fn table(range: &D3D12_DESCRIPTOR_RANGE) -> D3D12_ROOT_PARAMETER {
    D3D12_ROOT_PARAMETER {
        ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
        Anonymous: D3D12_ROOT_PARAMETER_0 {
            DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
                NumDescriptorRanges: 1,
                pDescriptorRanges: range,
            },
        },
        ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
    }
}
fn root_cbv(register: u32) -> D3D12_ROOT_PARAMETER {
    D3D12_ROOT_PARAMETER {
        ParameterType: D3D12_ROOT_PARAMETER_TYPE_CBV,
        Anonymous: D3D12_ROOT_PARAMETER_0 {
            Descriptor: D3D12_ROOT_DESCRIPTOR {
                ShaderRegister: register,
                RegisterSpace: 0,
            },
        },
        ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
    }
}
fn heap(device: &ID3D12Device, kind: D3D12_DESCRIPTOR_HEAP_TYPE, count: u32, visible: bool) -> Option<ID3D12DescriptorHeap> {
    let desc = D3D12_DESCRIPTOR_HEAP_DESC {
        Type: kind,
        NumDescriptors: count,
        Flags: if visible {
            D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE
        } else {
            D3D12_DESCRIPTOR_HEAP_FLAG_NONE
        },
        NodeMask: 0,
    };
    // SAFETY: the live device.
    unsafe { device.CreateDescriptorHeap(&desc).ok() }
}

/// The view a HUD-family texture is sampled through: the engine's R8G8B8A8_TYPELESS surfaces as sRGB.
fn hud_view(format: DXGI_FORMAT) -> DXGI_FORMAT {
    if format == DXGI_FORMAT_R8G8B8A8_TYPELESS {
        DXGI_FORMAT_R8G8B8A8_UNORM_SRGB
    } else {
        format
    }
}
fn texture_srv(format: DXGI_FORMAT, most_detailed: u32, levels: u32) -> D3D12_SHADER_RESOURCE_VIEW_DESC {
    D3D12_SHADER_RESOURCE_VIEW_DESC {
        Format: format,
        ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
        Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
        Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
            Texture2D: D3D12_TEX2D_SRV {
                MostDetailedMip: most_detailed,
                MipLevels: levels,
                ..Default::default()
            },
        },
    }
}
fn rtv(format: DXGI_FORMAT, mip: u32) -> D3D12_RENDER_TARGET_VIEW_DESC {
    D3D12_RENDER_TARGET_VIEW_DESC {
        Format: format,
        ViewDimension: D3D12_RTV_DIMENSION_TEXTURE2D,
        Anonymous: D3D12_RENDER_TARGET_VIEW_DESC_0 {
            Texture2D: D3D12_TEX2D_RTV { MipSlice: mip, PlaneSlice: 0 },
        },
    }
}
/// `size >> mip`, at least 1 (a shift past the width is 0, then 1).
fn mip_extent(size: u64, mip: u32) -> u64 {
    size.checked_shr(mip).unwrap_or(0).max(1)
}

struct State {
    device: ID3D12Device,
    root: ID3D12RootSignature,
    pso: ID3D12PipelineState,
    pso_overlay: Option<ID3D12PipelineState>,
    pso_overlay_copy: Option<ID3D12PipelineState>,
    pso_overlay_add: Option<ID3D12PipelineState>,
    pso_overlay_straight: Option<ID3D12PipelineState>,
    root_hud: Option<ID3D12RootSignature>,
    pso_hud: Option<ID3D12PipelineState>,
    root_dot: Option<ID3D12RootSignature>,
    pso_dot: Option<ID3D12PipelineState>,
    hud_srv_heap: Option<ID3D12DescriptorHeap>,
    /// CVR_SHADER_AB: the HLSL's pipelines, the takes, and the HUD composites recorded (their parity tells the eyes
    /// apart: each composites once a frame).
    reference: Option<Reference>,
    ab: crate::shader_ab::ShaderAb,
    hud_calls: u32,
    srv_heap: ID3D12DescriptorHeap,
    rtv_heap: ID3D12DescriptorHeap,
    slot: u32,
    srv_stride: u32,
    rtv_stride: u32,
    color_format: DXGI_FORMAT,
    width: u32,
    height: u32,
}
// SAFETY: free-threaded D3D12 objects, used under the blitter's mutex.
unsafe impl Send for State {}

impl State {
    /// CVR_SHADER_AB: this draw again by the HLSL reference and by the DXIL (`pso`) into a take's targets, then the
    /// caller's pipeline and target (`rtv`, the full viewport) bound again; whether a take was made.
    ///
    /// # Safety
    /// `list` records on the state's device with the draw's root signature, tables and constants set.
    unsafe fn take(
        &mut self,
        list: &ID3D12GraphicsCommandList,
        key: &str,
        reference: impl Fn(&Reference) -> Option<ID3D12PipelineState>,
        pso: &ID3D12PipelineState,
        rtv: D3D12_CPU_DESCRIPTOR_HANDLE,
    ) -> bool {
        if !self.ab.wants(key) {
            return false;
        }
        let Some(r) = self.reference.as_ref().and_then(reference) else { return false };
        let (device, w, h, format) = (self.device.clone(), self.width, self.height, self.color_format);
        // SAFETY: as the caller's; the take binds only its own targets.
        let made = unsafe {
            self.ab.take(&device, list, key, format, w, h, |replacement| {
                list.SetPipelineState(if replacement { pso } else { &r });
                list.DrawInstanced(4, 1, 0, 0);
            })
        };
        // SAFETY: the recording list.
        unsafe { list.SetPipelineState(pso) };
        self.target_full(list, rtv);
        made
    }
}

impl State {
    /// The next ring slot (every record function takes one).
    fn take_slot(&mut self) -> u32 {
        let slot = self.slot;
        self.slot = (self.slot + 1) % SLOTS;
        slot
    }
    fn srv_cpu(&self, heap: &ID3D12DescriptorHeap, index: u32) -> D3D12_CPU_DESCRIPTOR_HANDLE {
        // SAFETY: the live heap.
        let mut h = unsafe { heap.GetCPUDescriptorHandleForHeapStart() };
        h.ptr += index as usize * self.srv_stride as usize;
        h
    }
    fn srv_gpu(&self, heap: &ID3D12DescriptorHeap, index: u32) -> D3D12_GPU_DESCRIPTOR_HANDLE {
        // SAFETY: the live heap.
        let mut h = unsafe { heap.GetGPUDescriptorHandleForHeapStart() };
        h.ptr += index as u64 * self.srv_stride as u64;
        h
    }
    fn rtv_cpu(&self, slot: u32) -> D3D12_CPU_DESCRIPTOR_HANDLE {
        // SAFETY: the live heap.
        let mut h = unsafe { self.rtv_heap.GetCPUDescriptorHandleForHeapStart() };
        h.ptr += slot as usize * self.rtv_stride as usize;
        h
    }
    /// The full-target viewport and scissor and the render target, as most passes set them.
    fn target_full(&self, list: &ID3D12GraphicsCommandList, rtv_cpu: D3D12_CPU_DESCRIPTOR_HANDLE) {
        let vp = D3D12_VIEWPORT {
            TopLeftX: 0.0,
            TopLeftY: 0.0,
            Width: self.width as f32,
            Height: self.height as f32,
            MinDepth: 0.0,
            MaxDepth: 1.0,
        };
        // SAFETY: the recording list.
        unsafe {
            list.RSSetViewports(&[vp]);
            list.RSSetScissorRects(&[RECT {
                left: 0,
                top: 0,
                right: self.width as i32,
                bottom: self.height as i32,
            }]);
            list.OMSetRenderTargets(1, Some(&rtv_cpu), false, None);
        }
    }
}

/// A blitter's root signatures and pipelines for one device and render-target format, shared by every blitter: each HUD
/// panel slot owns one, and building all seven pipelines afresh for each new slot or canvas size cost ~50 ms on the
/// game's Present thread (three slots per channel -- 65-130 ms frames whenever a HUD channel first showed or an
/// interaction window came up in a new size; SteamVR freezes, 2026-11-26).
#[derive(Clone)]
struct Pipelines {
    device: ID3D12Device,
    color_format: DXGI_FORMAT,
    root: ID3D12RootSignature,
    pso: ID3D12PipelineState,
    pso_overlay: Option<ID3D12PipelineState>,
    pso_overlay_copy: Option<ID3D12PipelineState>,
    pso_overlay_add: Option<ID3D12PipelineState>,
    pso_overlay_straight: Option<ID3D12PipelineState>,
    root_hud: Option<ID3D12RootSignature>,
    pso_hud: Option<ID3D12PipelineState>,
    root_dot: Option<ID3D12RootSignature>,
    pso_dot: Option<ID3D12PipelineState>,
    ok: bool,
}
// SAFETY: free-threaded D3D12 objects, immutable once made.
unsafe impl Send for Pipelines {}
static PIPELINES: Mutex<Vec<Pipelines>> = Mutex::new(Vec::new());

/// Builds a blitter: its descriptor heaps, and the pipelines -- made once per device and format, then shared. The bool is
/// EnsureInitialized's answer (a failed RGBA-replace overlay pipeline answers false but keeps the rest, as the C++ did).
fn build(device: &ID3D12Device, color_format: DXGI_FORMAT, width: u32, height: u32) -> (Option<State>, bool) {
    let cached = PIPELINES
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .find(|p| p.device.as_raw() == device.as_raw() && p.color_format == color_format)
        .cloned();
    if let Some(p) = cached {
        let Some(srv_heap) = heap(device, D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, SLOTS, true) else {
            log!("ColorBlit: failed to create SRV heap\n");
            return (None, false);
        };
        let Some(rtv_heap) = heap(device, D3D12_DESCRIPTOR_HEAP_TYPE_RTV, SLOTS, false) else {
            log!("ColorBlit: failed to create RTV heap\n");
            return (None, false);
        };
        let hud_srv_heap = if p.root_hud.is_some() { heap(device, D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, SLOTS * 4, true) } else { None };
        // SAFETY: the live device.
        let (srv_stride, rtv_stride) = unsafe {
            (
                device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV),
                device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV),
            )
        };
        let mut state = State {
            device: device.clone(),
            root: p.root,
            pso: p.pso,
            pso_overlay: p.pso_overlay,
            pso_overlay_copy: p.pso_overlay_copy,
            pso_overlay_add: p.pso_overlay_add,
            pso_overlay_straight: p.pso_overlay_straight,
            root_hud: if hud_srv_heap.is_some() { p.root_hud } else { None },
            pso_hud: if hud_srv_heap.is_some() { p.pso_hud } else { None },
            root_dot: p.root_dot,
            pso_dot: p.pso_dot,
            hud_srv_heap,
            reference: None,
            ab: crate::shader_ab::ShaderAb::default(),
            hud_calls: 0,
            srv_heap,
            rtv_heap,
            slot: 0,
            srv_stride,
            rtv_stride,
            color_format,
            width,
            height,
        };
        if crate::shader_ab::enabled() && !crate::dxil::hlsl_requested() {
            state.reference = reference_psos(device, &state);
        }
        return (Some(state), p.ok);
    }
    let (state, ok) = build_fresh(device, color_format, width, height);
    if let Some(s) = state.as_ref() {
        PIPELINES.lock().unwrap_or_else(|p| p.into_inner()).push(Pipelines {
            device: s.device.clone(),
            color_format,
            root: s.root.clone(),
            pso: s.pso.clone(),
            pso_overlay: s.pso_overlay.clone(),
            pso_overlay_copy: s.pso_overlay_copy.clone(),
            pso_overlay_add: s.pso_overlay_add.clone(),
            pso_overlay_straight: s.pso_overlay_straight.clone(),
            root_hud: s.root_hud.clone(),
            pso_hud: s.pso_hud.clone(),
            root_dot: s.root_dot.clone(),
            pso_dot: s.pso_dot.clone(),
            ok,
        });
    }
    (state, ok)
}

/// The shared pipelines for `device` and `color_format` made now if they are not yet: the HUD panel asks at its first
/// capture call (the boot's or the main menu's frames), so no frame in the world pays for them.
pub fn prewarm(device: &ID3D12Device, color_format: DXGI_FORMAT) {
    let have = PIPELINES
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .any(|p| p.device.as_raw() == device.as_raw() && p.color_format == color_format);
    if !have {
        let span = cvr_host::hitch::start();
        drop(build(device, color_format, 16, 16));
        cvr_host::hitch::end(span, 0.0, || "ColorBlit pipelines prewarmed".to_string());
    }
}

/// Every pipeline made afresh (the first blitter of a device and format).
fn build_fresh(device: &ID3D12Device, color_format: DXGI_FORMAT, width: u32, height: u32) -> (Option<State>, bool) {
    let Some(srv_heap) = heap(device, D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, SLOTS, true) else {
        log!("ColorBlit: failed to create SRV heap\n");
        return (None, false);
    };
    let Some(rtv_heap) = heap(device, D3D12_DESCRIPTOR_HEAP_TYPE_RTV, SLOTS, false) else {
        log!("ColorBlit: failed to create RTV heap\n");
        return (None, false);
    };
    // SAFETY: the live device.
    let (srv_stride, rtv_stride) = unsafe {
        (
            device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV),
            device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV),
        )
    };
    let srv_range = D3D12_DESCRIPTOR_RANGE {
        RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
        NumDescriptors: 1,
        BaseShaderRegister: 0,
        ..Default::default()
    };
    let params = [
        table(&srv_range),
        constants(0, 4, D3D12_SHADER_VISIBILITY_PIXEL),
        constants(1, 4, D3D12_SHADER_VISIBILITY_PIXEL),
    ];
    let sampler = linear_clamp();
    let root_desc = D3D12_ROOT_SIGNATURE_DESC {
        NumParameters: 3,
        pParameters: params.as_ptr(),
        NumStaticSamplers: 1,
        pStaticSamplers: &sampler,
        Flags: D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT,
    };
    let root = match root_signature(device, &root_desc) {
        Ok(r) => r,
        Err(Some(errors)) => {
            log!("ColorBlit: root sig serialize failed {}\n", blob_text(&Some(errors)));
            return (None, false);
        }
        Err(None) => {
            log!("ColorBlit: failed to create root sig\n");
            return (None, false);
        }
    };
    // the shaders made from Rust (rust/shaders/passes/src/color_blit.rs); the HLSL above with CVR_SHADER_HLSL
    use crate::dxil::{pick, COLOR_BLIT_DOT_PS, COLOR_BLIT_DOT_VS, COLOR_BLIT_HUD_PS, COLOR_BLIT_OVERLAY_PS, COLOR_BLIT_PS, COLOR_BLIT_VS};
    let Some(vs) = pick(COLOR_BLIT_VS, || compile(VERTEX_SHADER, c"VSMain", c"vs_5_0")) else {
        return (None, false);
    };
    let Some(ps) = pick(COLOR_BLIT_PS, || compile(PIXEL_SHADER, c"PSMain", c"ps_5_0")) else {
        return (None, false);
    };
    let base = pipeline(&root, vs.bytecode(), ps.bytecode(), color_format);
    let Some(pso) = make_pso(device, &base) else {
        log!("ColorBlit: PSO create failed\n");
        return (None, false);
    };
    let mut state = State {
        device: device.clone(),
        root,
        pso,
        pso_overlay: None,
        pso_overlay_copy: None,
        pso_overlay_add: None,
        pso_overlay_straight: None,
        root_hud: None,
        pso_hud: None,
        root_dot: None,
        pso_dot: None,
        hud_srv_heap: None,
        reference: None,
        ab: crate::shader_ab::ShaderAb::default(),
        hud_calls: 0,
        srv_heap,
        rtv_heap,
        slot: 0,
        srv_stride,
        rtv_stride,
        color_format,
        width,
        height,
    };

    // Overlay variants: the same root signature and quad. RGBA replace first (the mip/blur passes), then the engine's
    // premultiplied HUD blend, its additive twin and the straight alpha of its final composite.
    if let Some(ps_overlay) = pick(COLOR_BLIT_OVERLAY_PS, || compile(OVERLAY_SHADER, c"PSMain", c"ps_5_0")) {
        let mut ov = pipeline(&state.root, vs.bytecode(), ps_overlay.bytecode(), color_format);
        state.pso_overlay_copy = make_pso(device, &ov);
        if state.pso_overlay_copy.is_none() {
            return (Some(state), false);
        }
        blend(&mut ov, D3D12_BLEND_ONE, D3D12_BLEND_INV_SRC_ALPHA, D3D12_BLEND_ONE, D3D12_BLEND_INV_SRC_ALPHA);
        state.pso_overlay = make_pso(device, &ov);
        if state.pso_overlay.is_none() {
            log!("ColorBlit: overlay PSO create failed -- second eye stays HUD-less\n");
        }
        blend(&mut ov, D3D12_BLEND_ONE, D3D12_BLEND_ONE, D3D12_BLEND_ONE, D3D12_BLEND_ONE);
        state.pso_overlay_add = make_pso(device, &ov);
        blend(
            &mut ov,
            D3D12_BLEND_SRC_ALPHA,
            D3D12_BLEND_INV_SRC_ALPHA,
            D3D12_BLEND_ONE,
            D3D12_BLEND_INV_SRC_ALPHA,
        );
        state.pso_overlay_straight = make_pso(device, &ov);
    }

    // The ported HUD composite: four SRVs in one table (scene, HUD, HUD blur pyramid, exposure), the parameters as
    // root constants, the engine's frame and composite constants as root CBVs straight off its own buffers.
    if let Some(hud_heap) = heap(device, D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, SLOTS * 4, true) {
        state.hud_srv_heap = Some(hud_heap);
        let range = D3D12_DESCRIPTOR_RANGE {
            RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
            NumDescriptors: 4,
            BaseShaderRegister: 0,
            ..Default::default()
        };
        let params = [
            table(&range),
            constants(0, HUD_PARAMS as u32, D3D12_SHADER_VISIBILITY_PIXEL),
            root_cbv(1),
            root_cbv(2),
        ];
        let sampler = linear_clamp();
        let desc = D3D12_ROOT_SIGNATURE_DESC {
            NumParameters: 4,
            pParameters: params.as_ptr(),
            NumStaticSamplers: 1,
            pStaticSamplers: &sampler,
            Flags: D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT,
        };
        let root = root_signature(device, &desc);
        let shader = if root.is_ok() { pick(COLOR_BLIT_HUD_PS, || compile(HUD_SHADER, c"PSMain", c"ps_5_0")) } else { None };
        match (root, shader) {
            (Ok(root_hud), Some(ps_hud)) => {
                let hp = pipeline(&root_hud, vs.bytecode(), ps_hud.bytecode(), color_format);
                state.pso_hud = make_pso(device, &hp);
                if state.pso_hud.is_none() {
                    log!("ColorBlit: HUD composite PSO create failed\n");
                }
                state.root_hud = Some(root_hud);
            }
            (root, _) => {
                let errors = root.err().flatten();
                log!("ColorBlit: HUD composite root signature/shader failed {}\n", blob_text(&errors));
            }
        }
    }

    // The barrel dot: root constants only (centre, radius, colour), placed by its own vertex shader.
    {
        let params = [constants(0, 8, D3D12_SHADER_VISIBILITY_ALL)];
        let desc = D3D12_ROOT_SIGNATURE_DESC {
            NumParameters: 1,
            pParameters: params.as_ptr(),
            Flags: D3D12_ROOT_SIGNATURE_FLAG_NONE,
            ..Default::default()
        };
        let root = root_signature(device, &desc);
        let shaders = if root.is_ok() {
            pick(COLOR_BLIT_DOT_VS, || compile(DOT_SHADER, c"VSMain", c"vs_5_0"))
                .and_then(|v| pick(COLOR_BLIT_DOT_PS, || compile(DOT_SHADER, c"PSMain", c"ps_5_0")).map(|p| (v, p)))
        } else {
            None
        };
        match (root, shaders) {
            (Ok(root_dot), Some((dvs, dps))) => {
                let mut dp = pipeline(&root_dot, dvs.bytecode(), dps.bytecode(), color_format);
                blend(
                    &mut dp,
                    D3D12_BLEND_SRC_ALPHA,
                    D3D12_BLEND_INV_SRC_ALPHA,
                    D3D12_BLEND_ONE,
                    D3D12_BLEND_INV_SRC_ALPHA,
                );
                state.pso_dot = make_pso(device, &dp);
                if state.pso_dot.is_none() {
                    log!("ColorBlit: dot PSO create failed -- second eye stays dotless\n");
                }
                state.root_dot = Some(root_dot);
            }
            (root, _) => {
                let errors = root.err().flatten();
                log!("ColorBlit: dot root signature/shader failed {}\n", blob_text(&errors));
            }
        }
    }
    if crate::shader_ab::enabled() && !crate::dxil::hlsl_requested() {
        state.reference = reference_psos(device, &state);
        log!("[color-blit] A/B: the HLSL reference made: {}\n", state.reference.is_some());
    }
    (Some(state), true)
}

/// CVR_SHADER_AB: the pipelines again from the HLSL the Rust shaders replace (FXC, as before), with the same
/// descriptions -- the reference each take draws beside the DXIL's.
struct Reference {
    pso: ID3D12PipelineState,
    overlay: Option<ID3D12PipelineState>,
    overlay_copy: Option<ID3D12PipelineState>,
    overlay_add: Option<ID3D12PipelineState>,
    overlay_straight: Option<ID3D12PipelineState>,
    hud: Option<ID3D12PipelineState>,
    dot: Option<ID3D12PipelineState>,
}
fn reference_psos(device: &ID3D12Device, state: &State) -> Option<Reference> {
    let format = state.color_format;
    let vs = compile(VERTEX_SHADER, c"VSMain", c"vs_5_0")?;
    let ps = compile(PIXEL_SHADER, c"PSMain", c"ps_5_0")?;
    let pso = make_pso(device, &pipeline(&state.root, bytecode(&vs), bytecode(&ps), format))?;
    let mut r = Reference { pso, overlay: None, overlay_copy: None, overlay_add: None, overlay_straight: None, hud: None, dot: None };
    if let Some(ps_overlay) = compile(OVERLAY_SHADER, c"PSMain", c"ps_5_0") {
        let mut ov = pipeline(&state.root, bytecode(&vs), bytecode(&ps_overlay), format);
        r.overlay_copy = make_pso(device, &ov);
        blend(&mut ov, D3D12_BLEND_ONE, D3D12_BLEND_INV_SRC_ALPHA, D3D12_BLEND_ONE, D3D12_BLEND_INV_SRC_ALPHA);
        r.overlay = make_pso(device, &ov);
        blend(&mut ov, D3D12_BLEND_ONE, D3D12_BLEND_ONE, D3D12_BLEND_ONE, D3D12_BLEND_ONE);
        r.overlay_add = make_pso(device, &ov);
        blend(&mut ov, D3D12_BLEND_SRC_ALPHA, D3D12_BLEND_INV_SRC_ALPHA, D3D12_BLEND_ONE, D3D12_BLEND_INV_SRC_ALPHA);
        r.overlay_straight = make_pso(device, &ov);
    }
    if let (Some(root_hud), Some(ps_hud)) = (&state.root_hud, compile(HUD_SHADER, c"PSMain", c"ps_5_0")) {
        r.hud = make_pso(device, &pipeline(root_hud, bytecode(&vs), bytecode(&ps_hud), format));
    }
    if let (Some(root_dot), Some(dvs), Some(dps)) =
        (&state.root_dot, compile(DOT_SHADER, c"VSMain", c"vs_5_0"), compile(DOT_SHADER, c"PSMain", c"ps_5_0"))
    {
        let mut dp = pipeline(root_dot, bytecode(&dvs), bytecode(&dps), format);
        blend(&mut dp, D3D12_BLEND_SRC_ALPHA, D3D12_BLEND_INV_SRC_ALPHA, D3D12_BLEND_ONE, D3D12_BLEND_INV_SRC_ALPHA);
        r.dot = make_pso(device, &dp);
    }
    Some(r)
}

/// ColorBlit.
#[derive(Default)]
pub struct Blit {
    state: Mutex<Option<State>>,
}

impl Blit {
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<State>> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// EnsureInitialized: every pipeline for `device`, the typed render-target format and the size; multi-call safe.
    /// A typeless format is refused loudly: an RTV cannot be typeless, and the GPU answers one with a hung device.
    pub fn ensure(&self, device: &ID3D12Device, color_format: DXGI_FORMAT, width: u32, height: u32) -> bool {
        let mut s = self.lock();
        if width == 0 || height == 0 || color_format == DXGI_FORMAT_UNKNOWN {
            return false;
        }
        if matches!(
            color_format,
            DXGI_FORMAT_R8G8B8A8_TYPELESS
                | DXGI_FORMAT_B8G8R8A8_TYPELESS
                | DXGI_FORMAT_R10G10B10A2_TYPELESS
                | DXGI_FORMAT_R16G16B16A16_TYPELESS
                | DXGI_FORMAT_R32G32B32A32_TYPELESS
        ) {
            log!("ColorBlit: refusing typeless RTV format {} -- pass the typed view format\n", color_format.0);
            return false;
        }
        if let Some(st) = s.as_mut() {
            if st.device.as_raw() == device.as_raw() && st.color_format == color_format {
                // A new size alone keeps every pipeline and heap: the size is only the viewport, the scissor and the
                // constants made from it at each draw (the C++ kept them too, c3d7ca74).
                st.width = width;
                st.height = height;
                return true;
            }
        }
        *s = None;
        let (state, ok) = build(device, color_format, width, height);
        *s = state;
        ok
    }

    /// RecordDot: a straight-alpha disc at an NDC point (+Y up) over `dst` (in RENDER_TARGET); false off screen.
    // NaN radius and coordinates are refused, as the C++ tests were written: !(x > 0), not x <= 0.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn record_dot(&self, list: &ID3D12GraphicsCommandList, dst: &ID3D12Resource, ndc_x: f32, ndc_y: f32, radius_px: f32, color: [f32; 4]) -> bool {
        let mut s = self.lock();
        let Some(st) = s.as_mut() else { return false };
        let (Some(root), Some(pso)) = (st.root_dot.clone(), st.pso_dot.clone()) else {
            return false;
        };
        if !(radius_px > 0.0) {
            return false;
        }
        if !(ndc_x > -1.5 && ndc_x < 1.5 && ndc_y > -1.5 && ndc_y < 1.5) {
            return false;
        }
        let slot = st.take_slot();
        let rtv_cpu = st.rtv_cpu(slot);
        let k = [
            ndc_x,
            ndc_y,
            (2.0 * radius_px) / st.width as f32,
            (2.0 * radius_px) / st.height as f32,
            color[0],
            color[1],
            color[2],
            color[3],
        ];
        // SAFETY: the live objects and the recording list.
        unsafe {
            st.device.CreateRenderTargetView(dst, Some(&rtv(st.color_format, 0)), rtv_cpu);
            st.target_full(list, rtv_cpu);
            list.SetGraphicsRootSignature(&root);
            list.SetPipelineState(&pso);
            list.SetGraphicsRoot32BitConstants(0, 8, k.as_ptr() as *const c_void, 0);
            list.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
            let key = format!("color_blit.dot format {}", st.color_format.0);
            st.take(list, &key, |r| r.dot.clone(), &pso, rtv_cpu);
            list.DrawInstanced(4, 1, 0, 0);
        }
        true
    }

    /// RecordBlit: `src` (its own format, mip 0) with the sub-rectangle `uv` stretched over `dst`.
    pub fn record_blit(&self, list: &ID3D12GraphicsCommandList, src: &ID3D12Resource, dst: &ID3D12Resource, uv: [f32; 4]) -> bool {
        let mut s = self.lock();
        let Some(st) = s.as_mut() else { return false };
        let slot = st.take_slot();
        let (srv_cpu, srv_gpu, rtv_cpu) = (st.srv_cpu(&st.srv_heap, slot), st.srv_gpu(&st.srv_heap, slot), st.rtv_cpu(slot));
        // SAFETY: the live objects and the recording list.
        unsafe {
            st.device.CreateShaderResourceView(src, Some(&texture_srv(src.GetDesc().Format, 0, 1)), srv_cpu);
            st.device.CreateRenderTargetView(dst, Some(&rtv(st.color_format, 0)), rtv_cpu);
            st.target_full(list, rtv_cpu);
            list.SetGraphicsRootSignature(&st.root);
            list.SetPipelineState(&st.pso);
            list.SetDescriptorHeaps(&[Some(st.srv_heap.clone())]);
            list.SetGraphicsRootDescriptorTable(0, srv_gpu);
            list.SetGraphicsRoot32BitConstants(1, 4, uv.as_ptr() as *const c_void, 0);
            list.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
            let (key, pso) = (format!("color_blit.blit format {}", st.color_format.0), st.pso.clone());
            st.take(list, &key, |r| Some(r.pso.clone()), &pso, rtv_cpu);
            list.DrawInstanced(4, 1, 0, 0);
        }
        true
    }

    /// RecordHudComposite: scene + HUD into `dst` (in RENDER_TARGET) through the engine's own arithmetic on its own
    /// data (HUD mip chain, blur pyramid, exposure, frame and composite constants).
    #[allow(clippy::too_many_arguments)]
    pub fn record_hud_composite(
        &self,
        list: &ID3D12GraphicsCommandList,
        scene: &ID3D12Resource,
        hud: &ID3D12Resource,
        hud_blur: &ID3D12Resource,
        exposure: &ID3D12Resource,
        frame_cb: &ID3D12Resource,
        hud_cb: &ID3D12Resource,
        dst: &ID3D12Resource,
        params: &[f32; HUD_PARAMS],
    ) -> bool {
        let mut s = self.lock();
        let Some(st) = s.as_mut() else { return false };
        let (Some(pso), Some(heap), Some(root)) = (st.pso_hud.clone(), st.hud_srv_heap.clone(), st.root_hud.clone()) else {
            return false;
        };
        let slot = st.take_slot();
        let srv_cpu = st.srv_cpu(&heap, slot * 4);
        let srv_gpu = st.srv_gpu(&heap, slot * 4);
        let rtv_cpu = st.rtv_cpu(slot);
        let at = |i: u32| D3D12_CPU_DESCRIPTOR_HANDLE {
            ptr: srv_cpu.ptr + i as usize * st.srv_stride as usize,
        };
        let mut p = *params;
        p[HUD_TARGET_W] = st.width as f32;
        p[HUD_TARGET_H] = st.height as f32;
        // SAFETY: the live objects and the recording list.
        unsafe {
            // t0 the scene in its own format; t1 the HUD and t2 its blur pyramid with their whole mip chains (the
            // glow reads mips 1/2/4, the halo lod 1.8); t3 FrameExposureData raw, for its dword 6.
            st.device
                .CreateShaderResourceView(scene, Some(&texture_srv(hud_view(scene.GetDesc().Format), 0, 1)), at(0));
            let levels = |d: &D3D12_RESOURCE_DESC| if d.MipLevels != 0 { d.MipLevels as u32 } else { 1 };
            let hd = hud.GetDesc();
            st.device
                .CreateShaderResourceView(hud, Some(&texture_srv(hud_view(hd.Format), 0, levels(&hd))), at(1));
            let bd = hud_blur.GetDesc();
            st.device
                .CreateShaderResourceView(hud_blur, Some(&texture_srv(hud_view(bd.Format), 0, levels(&bd))), at(2));
            let ev = D3D12_SHADER_RESOURCE_VIEW_DESC {
                Format: DXGI_FORMAT_R32_TYPELESS,
                ViewDimension: D3D12_SRV_DIMENSION_BUFFER,
                Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
                Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                    Buffer: D3D12_BUFFER_SRV {
                        FirstElement: 0,
                        NumElements: (exposure.GetDesc().Width / 4) as u32,
                        StructureByteStride: 0,
                        Flags: D3D12_BUFFER_SRV_FLAG_RAW,
                    },
                },
            };
            st.device.CreateShaderResourceView(exposure, Some(&ev), at(3));
            st.device.CreateRenderTargetView(dst, Some(&rtv(st.color_format, 0)), rtv_cpu);
            st.target_full(list, rtv_cpu);
            list.SetGraphicsRootSignature(&root);
            list.SetPipelineState(&pso);
            list.SetDescriptorHeaps(&[Some(heap.clone())]);
            list.SetGraphicsRootDescriptorTable(0, srv_gpu);
            list.SetGraphicsRoot32BitConstants(1, HUD_PARAMS as u32, p.as_ptr() as *const c_void, 0);
            list.SetGraphicsRootConstantBufferView(2, frame_cb.GetGPUVirtualAddress());
            list.SetGraphicsRootConstantBufferView(3, hud_cb.GetGPUVirtualAddress());
            list.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
            st.hud_calls = st.hud_calls.wrapping_add(1);
            let key = format!("color_blit.hud call parity {} format {}", st.hud_calls % 2, st.color_format.0);
            st.take(list, &key, |r| r.hud.clone(), &pso, rtv_cpu);
            list.DrawInstanced(4, 1, 0, 0);
        }
        true
    }

    /// RecordOverlay: `src` (one mip, HUD view) blended over `dst` (in RENDER_TARGET, one mip). mode 0 premultiplied,
    /// 1 opaque copy (diagnostic), 2 additive, 3 straight alpha, 4 RGBA replace; 2 and 3 fall back to 0 when their
    /// pipeline is missing. `pixel_exact` sizes the viewport to the source (or `size`) at `offset`, so texels map one
    /// to one; `blur_axis` 1/2 runs the separable blur along x/y; `black_point` drops light under it.
    #[allow(clippy::too_many_arguments)]
    pub fn record_overlay(
        &self,
        list: &ID3D12GraphicsCommandList,
        src: &ID3D12Resource,
        dst: &ID3D12Resource,
        mode: i32,
        pixel_exact: bool,
        offset: [f32; 2],
        size: [f32; 2],
        tint: Option<[f32; 4]>,
        source_mip: u32,
        target_mip: u32,
        blur_axis: i32,
        black_point: f32,
    ) -> bool {
        let mut s = self.lock();
        let Some(st) = s.as_mut() else { return false };
        let pso = match mode {
            1 => Some(st.pso.clone()),
            4 => st.pso_overlay_copy.clone(),
            2 if st.pso_overlay_add.is_some() => st.pso_overlay_add.clone(),
            3 if st.pso_overlay_straight.is_some() => st.pso_overlay_straight.clone(),
            _ => st.pso_overlay.clone(),
        };
        let Some(pso) = pso else { return false };
        let slot = st.take_slot();
        let (srv_cpu, srv_gpu, rtv_cpu) = (st.srv_cpu(&st.srv_heap, slot), st.srv_gpu(&st.srv_heap, slot), st.rtv_cpu(slot));
        // SAFETY: the live objects and the recording list.
        unsafe {
            let sd = src.GetDesc();
            st.device
                .CreateShaderResourceView(src, Some(&texture_srv(hud_view(sd.Format), source_mip, 1)), srv_cpu);
            st.device.CreateRenderTargetView(dst, Some(&rtv(st.color_format, target_mip)), rtv_cpu);
            let source_w = mip_extent(sd.Width, source_mip);
            let source_h = mip_extent(sd.Height as u64, source_mip);
            let vp = if pixel_exact {
                D3D12_VIEWPORT {
                    TopLeftX: offset[0],
                    TopLeftY: offset[1],
                    Width: if size[0] > 0.0 { size[0] } else { source_w as f32 },
                    Height: if size[1] > 0.0 { size[1] } else { source_h as f32 },
                    MinDepth: 0.0,
                    MaxDepth: 1.0,
                }
            } else {
                D3D12_VIEWPORT {
                    TopLeftX: 0.0,
                    TopLeftY: 0.0,
                    Width: st.width as f32,
                    Height: st.height as f32,
                    MinDepth: 0.0,
                    MaxDepth: 1.0,
                }
            };
            let dd = dst.GetDesc();
            let scissor = RECT {
                left: 0,
                top: 0,
                right: mip_extent(dd.Width, target_mip) as i32,
                bottom: mip_extent(dd.Height as u64, target_mip) as i32,
            };
            list.RSSetViewports(&[vp]);
            list.RSSetScissorRects(&[scissor]);
            list.OMSetRenderTargets(1, Some(&rtv_cpu), false, None);
            list.SetGraphicsRootSignature(&st.root);
            list.SetPipelineState(&pso);
            list.SetDescriptorHeaps(&[Some(st.srv_heap.clone())]);
            list.SetGraphicsRootDescriptorTable(0, srv_gpu);
            // b0 is the tint for the overlay shaders but the uv rectangle for the blit one (mode 1).
            let b0 = if pso.as_raw() == st.pso.as_raw() {
                [0.0, 0.0, 1.0, 1.0]
            } else {
                tint.unwrap_or([1.0; 4])
            };
            list.SetGraphicsRoot32BitConstants(1, 4, b0.as_ptr() as *const c_void, 0);
            let filter = [
                if blur_axis == 1 { 1.0 / source_w as f32 } else { 0.0 },
                if blur_axis == 2 { 1.0 / source_h as f32 } else { 0.0 },
                if blur_axis != 0 { 1.0 } else { 0.0 },
                black_point,
            ];
            list.SetGraphicsRoot32BitConstants(2, 4, filter.as_ptr() as *const c_void, 0);
            list.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
            let key = format!("color_blit.overlay mode {mode} blur {blur_axis} format {}", st.color_format.0);
            let pick_ref = move |r: &Reference| match mode {
                1 => Some(r.pso.clone()),
                4 => r.overlay_copy.clone(),
                2 if r.overlay_add.is_some() => r.overlay_add.clone(),
                3 if r.overlay_straight.is_some() => r.overlay_straight.clone(),
                _ => r.overlay.clone(),
            };
            if st.take(list, &key, pick_ref, &pso, rtv_cpu) {
                list.RSSetViewports(&[vp]);
                list.RSSetScissorRects(&[scissor]);
            }
            list.DrawInstanced(4, 1, 0, 0);
        }
        true
    }

    /// Shutdown: every object released.
    pub fn shutdown(&self) {
        *self.lock() = None;
    }
}

// ---- the C ABI (include/Render/ColorBlit.hpp)

#[no_mangle]
pub extern "C" fn cvr_gpu_color_blit_new() -> *mut Blit {
    Box::into_raw(Box::default())
}
/// # Safety
/// `blit` came from cvr_gpu_color_blit_new (freed once) or is null.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_color_blit_free(blit: *mut Blit) {
    if !blit.is_null() {
        // SAFETY: the caller's blitter, freed once.
        drop(unsafe { Box::from_raw(blit) });
    }
}
/// # Safety
/// `blit` is live; `device` an ID3D12Device or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_color_blit_ensure(blit: *const Blit, device: *mut c_void, format: i32, width: u32, height: u32) -> bool {
    // SAFETY: the caller's blitter and device.
    unsafe {
        match (blit.as_ref(), ID3D12Device::from_raw_borrowed(&device)) {
            (Some(b), Some(d)) => b.ensure(d, DXGI_FORMAT(format), width, height),
            _ => false,
        }
    }
}
/// # Safety
/// `blit` is live; the others are a recording ID3D12GraphicsCommandList and ID3D12Resources, or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_color_blit_record_blit(
    blit: *const Blit,
    list: *mut c_void,
    src: *mut c_void,
    dst: *mut c_void,
    u0: f32,
    v0: f32,
    u1: f32,
    v1: f32,
) -> bool {
    // SAFETY: the caller's objects.
    unsafe {
        match (
            blit.as_ref(),
            ID3D12GraphicsCommandList::from_raw_borrowed(&list),
            ID3D12Resource::from_raw_borrowed(&src),
            ID3D12Resource::from_raw_borrowed(&dst),
        ) {
            (Some(b), Some(l), Some(s), Some(d)) => b.record_blit(l, s, d, [u0, v0, u1, v1]),
            _ => false,
        }
    }
}
/// # Safety
/// As cvr_gpu_color_blit_record_blit; `tint` is four floats or null.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn cvr_gpu_color_blit_record_overlay(
    blit: *const Blit,
    list: *mut c_void,
    src: *mut c_void,
    dst: *mut c_void,
    mode: i32,
    pixel_exact: bool,
    off_x: f32,
    off_y: f32,
    width: f32,
    height: f32,
    tint: *const [f32; 4],
    source_mip: u32,
    target_mip: u32,
    blur_axis: i32,
    black_point: f32,
) -> bool {
    // SAFETY: the caller's objects.
    unsafe {
        match (
            blit.as_ref(),
            ID3D12GraphicsCommandList::from_raw_borrowed(&list),
            ID3D12Resource::from_raw_borrowed(&src),
            ID3D12Resource::from_raw_borrowed(&dst),
        ) {
            (Some(b), Some(l), Some(s), Some(d)) => b.record_overlay(
                l,
                s,
                d,
                mode,
                pixel_exact,
                [off_x, off_y],
                [width, height],
                tint.as_ref().copied(),
                source_mip,
                target_mip,
                blur_axis,
                black_point,
            ),
            _ => false,
        }
    }
}
/// A borrowed resource behind a C pointer (None for null).
///
/// # Safety
/// `p` points at an ID3D12Resource or is null.
unsafe fn r(p: &*mut c_void) -> Option<&ID3D12Resource> {
    // SAFETY: the caller's pointer.
    unsafe { ID3D12Resource::from_raw_borrowed(p) }
}
/// # Safety
/// As cvr_gpu_color_blit_record_blit; `params` is ColorBlit::HudParams (31 floats).
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn cvr_gpu_color_blit_record_hud_composite(
    blit: *const Blit,
    list: *mut c_void,
    scene: *mut c_void,
    hud: *mut c_void,
    hud_blur: *mut c_void,
    exposure: *mut c_void,
    frame_cb: *mut c_void,
    hud_cb: *mut c_void,
    dst: *mut c_void,
    params: *const [f32; HUD_PARAMS],
) -> bool {
    // SAFETY: the caller's objects.
    unsafe {
        match (blit.as_ref(), ID3D12GraphicsCommandList::from_raw_borrowed(&list), params.as_ref()) {
            (Some(b), Some(l), Some(p)) => match (r(&scene), r(&hud), r(&hud_blur), r(&exposure), r(&frame_cb), r(&hud_cb), r(&dst)) {
                (Some(s), Some(h), Some(hb), Some(e), Some(f), Some(hc), Some(d)) => b.record_hud_composite(l, s, h, hb, e, f, hc, d, p),
                _ => false,
            },
            _ => false,
        }
    }
}
/// # Safety
/// As cvr_gpu_color_blit_record_blit.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn cvr_gpu_color_blit_record_dot(
    blit: *const Blit,
    list: *mut c_void,
    dst: *mut c_void,
    ndc_x: f32,
    ndc_y: f32,
    radius_px: f32,
    r: f32,
    g: f32,
    b: f32,
    a: f32,
) -> bool {
    // SAFETY: the caller's objects.
    unsafe {
        match (
            blit.as_ref(),
            ID3D12GraphicsCommandList::from_raw_borrowed(&list),
            ID3D12Resource::from_raw_borrowed(&dst),
        ) {
            (Some(bl), Some(l), Some(d)) => bl.record_dot(l, d, ndc_x, ndc_y, radius_px, [r, g, b, a]),
            _ => false,
        }
    }
}
/// # Safety
/// `blit` is live or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_color_blit_shutdown(blit: *const Blit) {
    // SAFETY: the caller's blitter.
    if let Some(b) = unsafe { blit.as_ref() } {
        b.shutdown();
    }
}
