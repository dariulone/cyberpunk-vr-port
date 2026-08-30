#include "Render/ColorBlit.hpp"

#include <cstdio>
#include <cstring>
#include <d3dcompiler.h>

extern void Log(const char* fmt, ...);

namespace {
using Microsoft::WRL::ComPtr;

constexpr char kVsSource[] = R"(
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
)";

constexpr char kPsSource[] = R"(
Texture2D<float4> g_color : register(t0);
SamplerState g_linear : register(s0);

struct VSOut {
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

float4 PSMain(VSOut input) : SV_Target {
    float4 c = g_color.SampleLevel(g_linear, input.uv, 0.0);
    return float4(c.rgb, 1.0);
}
)";

// The overlay keeps alpha, because the blend state consumes it. The source view is the HUD
// surface's own _UNORM_SRGB format, so the sample is decoded to linear by the hardware, and the
// destination RTV is _UNORM_SRGB too, so the blend happens in linear and the store re-encodes --
// the same arrangement the engine uses when it composites the HUD for the flat screen.
constexpr char kPsOverlaySource[] = R"(
Texture2D<float4> g_color : register(t0);
SamplerState g_linear : register(s0);

struct VSOut {
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

float4 PSMain(VSOut input) : SV_Target {
    return g_color.SampleLevel(g_linear, input.uv, 0.0);
}
)";

// The engine's HUD composite, ported from PipelineState_576 (the indirect compute dispatch that
// writes the final colour). Only the HUD-relevant part is kept: with the game's live constants
// the glitch branch (_43_m0[0].x/.y), the film-grain branch (_43_m0[16].x), the lens overlay
// (_43_m0[1].w) and the fullscreen-video layer (1x1 and 16x16 dummies) are all inert, and the
// scene bloom add belongs to the scene, not the HUD.
//
// What is kept, and why each matters:
//   * curvature  -- the barrel warp that makes the HUD sit on a curved screen
//   * x2 gain    -- the engine stores the HUD at half intensity; without this the text is thin
//   * glow       -- mips 1/2/4, offset and weighted, gated by the scanline flicker
//   * aberration -- R/G/B sampled at +off/0/-off
//   * shadow     -- the scene is attenuated under the HUD by (1 - mean alpha) and a blurred alpha
constexpr char kPsHudSource[] = R"(
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
    float3 scene = g_scene.SampleLevel(g_linear, uv, 0.0).rgb;
    glow *= 1.0 - dot(scene, float3(0.2126, 0.7152, 0.0722)) * 0.7;   // _2263

    float hudL  = dot(hud,  float3(0.2126, 0.7152, 0.0722));
    float glowL = dot(glow, float3(0.2126, 0.7152, 0.0722));
    hud  = (hud  - hudL)  * kSat + hudL;
    glow = (glow - glowL) * kSat + glowL;

    // _2381 * _2379: the wide HUD halo, added outside the coverage/shadow attenuation.
    float3 halo = g_hudBlur.SampleLevel(g_linear, d, kBloomL).rgb * (kBloomG * expo);

    hud *= expo;
    return float4(hud + (glow * (kGlow * expo) + scene) * cover * sh + halo, 1.0);
}
)";


// A radial laser spot generated entirely from SV_VertexID. This uses the same ring locations
// and quantised vertex colours as the HMD ImGui mesh in OverlayDebugDraw, so the desktop
// second-eye mirror retains the same source appearance.
constexpr char kDotSource[] = R"(
cbuffer DotParams : register(b0) {
    float2 centerNdc;    // -1..1, +Y up
    float2 radiusNdc;    // 2.5x-core halo extent, already aspect-corrected by the caller
    float4 color;        // straight alpha
};

struct VSOut {
    float4 position : SV_Position;
    float4 color    : COLOR0;
};

static const uint kSegments = 64;
static const uint kRingCount = 9;
static const float kOuterCoreRatio = 2.5;
static const float kRingCoreDistances[9] = {
    0.25, 0.50, 0.75, 1.00, 1.30, 1.65, 2.00, 2.25, 2.50
};

float4 SpotColor(float coreDistance) {
    const float intensity = exp(-0.69314718 * coreDistance * coreDistance);
    const float outerFade = 1.0 - smoothstep(2.25, 2.5, coreDistance);
    const float hotCore = exp(-4.0 * coreDistance * coreDistance);
    const float3 hotColor = float3(1.0, 0.82, 0.68);
    const float3 rgb = lerp(color.rgb, hotColor, hotCore);
    // ImGui stores these samples as ImU32. Match that 8-bit quantisation before interpolation.
    return round(saturate(float4(rgb, color.a * intensity * outerFade)) * 255.0) / 255.0;
}

float2 RingPoint(float coreDistance, uint segment) {
    const float angle = 6.28318530718 * float(segment % kSegments) / float(kSegments);
    const float radius = coreDistance / kOuterCoreRatio;
    return float2(cos(angle), sin(angle)) * radius;
}

VSOut MakeVertex(float coreDistance, uint segment) {
    VSOut o;
    const float2 local = coreDistance > 0.0 ? RingPoint(coreDistance, segment) : float2(0.0, 0.0);
    o.position = float4(centerNdc + local * radiusNdc, 0.0, 1.0);
    o.color = SpotColor(coreDistance);
    return o;
}

VSOut VSMain(uint vid : SV_VertexID) {
    const uint triangleIndex = vid / 3;
    const uint corner = vid % 3;

    // Center fan: center, first-ring current, first-ring next.
    if (triangleIndex < kSegments) {
        if (corner == 0) return MakeVertex(0.0, 0);
        if (corner == 1) return MakeVertex(kRingCoreDistances[0], triangleIndex);
        return MakeVertex(kRingCoreDistances[0], triangleIndex + 1);
    }

    // Every later band has two triangles per angular segment.
    const uint bandTriangle = triangleIndex - kSegments;
    const uint band = bandTriangle / (kSegments * 2);
    const uint withinBand = bandTriangle % (kSegments * 2);
    const uint segment = withinBand / 2;
    const bool secondTriangle = (withinBand % 2) != 0;
    const float innerDistance = kRingCoreDistances[band];
    const float outerDistance = kRingCoreDistances[band + 1];

    if (!secondTriangle) {
        if (corner == 0) return MakeVertex(innerDistance, segment);
        if (corner == 1) return MakeVertex(outerDistance, segment);
        return MakeVertex(outerDistance, segment + 1);
    }
    if (corner == 0) return MakeVertex(innerDistance, segment);
    if (corner == 1) return MakeVertex(outerDistance, segment + 1);
    return MakeVertex(innerDistance, segment + 1);
}

float4 PSMain(VSOut i) : SV_Target {
    return i.color;
}
)";

constexpr UINT kDotMeshSegments = 64;
constexpr UINT kDotMeshRings = 9;
constexpr UINT kDotMeshVertexCount =
    kDotMeshSegments * (1 + 2 * (kDotMeshRings - 1)) * 3;

DXGI_FORMAT ToNonSrgbViewFormat(DXGI_FORMAT format) {
    switch (format) {
        case DXGI_FORMAT_R8G8B8A8_UNORM_SRGB: return DXGI_FORMAT_R8G8B8A8_UNORM;
        case DXGI_FORMAT_B8G8R8A8_UNORM_SRGB: return DXGI_FORMAT_B8G8R8A8_UNORM;
        case DXGI_FORMAT_B8G8R8X8_UNORM_SRGB: return DXGI_FORMAT_B8G8R8X8_UNORM;
        default: return format;
    }
}

bool CompileShader(const char* src, const char* entry, const char* target, ComPtr<ID3DBlob>& out) {
    UINT flags = D3DCOMPILE_ENABLE_STRICTNESS;
    ComPtr<ID3DBlob> errors;
    HRESULT hr = D3DCompile(src, std::strlen(src), nullptr, nullptr, nullptr,
        entry, target, flags, 0, &out, &errors);
    if (FAILED(hr)) {
        Log("ColorBlit: shader compile failed %s/%s hr=0x%08X %.*s\n",
            entry, target, static_cast<unsigned>(hr),
            errors ? static_cast<int>(errors->GetBufferSize()) : 0,
            errors ? static_cast<const char*>(errors->GetBufferPointer()) : "");
        return false;
    }
    return true;
}
}

ColorBlit::~ColorBlit() { Shutdown(); }

void ColorBlit::Shutdown() {
    std::lock_guard<std::mutex> lock(m_mutex);
    m_pso.Reset();
    m_psoOverlay.Reset();
    m_psoOverlayAdd.Reset();
    m_psoOverlayStraight.Reset();
    m_psoHud.Reset();
    m_rootSigHud.Reset();
    m_psoDot.Reset();
    m_rootSigDot.Reset();
    m_hudSrvHeap.Reset();
    m_rootSig.Reset();
    m_srvHeap.Reset();
    m_rtvHeap.Reset();
    m_device.Reset();
    m_colorFormat = DXGI_FORMAT_UNKNOWN;
    m_width = m_height = 0;
}

bool ColorBlit::EnsureInitialized(ID3D12Device* device,
                                  DXGI_FORMAT colorFormat,
                                  uint32_t width,
                                  uint32_t height) {
    std::lock_guard<std::mutex> lock(m_mutex);
    if (!device || !width || !height || colorFormat == DXGI_FORMAT_UNKNOWN) return false;

    // Refuse a TYPELESS render-target format, loudly.
    //
    // A render target view cannot be typeless, and the runtime hands out XR swapchain images
    // as typeless resources precisely so the app picks the view. Passing the RESOURCE format
    // straight through here therefore builds an illegal RTV, and the answer is not an error
    // return -- it is DXGI_ERROR_DEVICE_HUNG on the first draw. That cost two crashes to find,
    // so it fails here instead of on the GPU. Callers must pass the typed format.
    switch (colorFormat) {
        case DXGI_FORMAT_R8G8B8A8_TYPELESS:
        case DXGI_FORMAT_B8G8R8A8_TYPELESS:
        case DXGI_FORMAT_R10G10B10A2_TYPELESS:
        case DXGI_FORMAT_R16G16B16A16_TYPELESS:
        case DXGI_FORMAT_R32G32B32A32_TYPELESS:
            Log("ColorBlit: refusing typeless RTV format %u -- pass the typed view format\n",
                static_cast<unsigned>(colorFormat));
            return false;
        default:
            break;
    }
    if (m_device.Get() == device && m_colorFormat == colorFormat && m_width == width && m_height == height && m_pso) {
        return true;
    }

    m_pso.Reset();
    m_psoOverlay.Reset();
    m_psoOverlayAdd.Reset();
    m_psoOverlayStraight.Reset();
    m_psoHud.Reset();
    m_rootSigHud.Reset();
    m_psoDot.Reset();
    m_rootSigDot.Reset();
    m_hudSrvHeap.Reset();
    m_rootSig.Reset();
    m_srvHeap.Reset();
    m_rtvHeap.Reset();
    m_device = device;
    m_colorFormat = colorFormat;
    m_width = width;
    m_height = height;

    D3D12_DESCRIPTOR_HEAP_DESC srvHeapDesc{};
    srvHeapDesc.NumDescriptors = kSlots;
    srvHeapDesc.Type = D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV;
    srvHeapDesc.Flags = D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE;
    if (FAILED(device->CreateDescriptorHeap(&srvHeapDesc, IID_PPV_ARGS(&m_srvHeap)))) {
        Log("ColorBlit: failed to create SRV heap\n");
        return false;
    }

    D3D12_DESCRIPTOR_HEAP_DESC rtvHeapDesc{};
    rtvHeapDesc.NumDescriptors = kSlots;
    rtvHeapDesc.Type = D3D12_DESCRIPTOR_HEAP_TYPE_RTV;
    if (FAILED(device->CreateDescriptorHeap(&rtvHeapDesc, IID_PPV_ARGS(&m_rtvHeap)))) {
        Log("ColorBlit: failed to create RTV heap\n");
        return false;
    }
    m_srvStride = device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
    m_rtvStride = device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV);
    m_slot = 0;

    D3D12_DESCRIPTOR_RANGE srvRange{};
    srvRange.RangeType = D3D12_DESCRIPTOR_RANGE_TYPE_SRV;
    srvRange.NumDescriptors = 1;
    srvRange.BaseShaderRegister = 0;

    D3D12_ROOT_PARAMETER param{};
    param.ParameterType = D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;
    param.DescriptorTable.NumDescriptorRanges = 1;
    param.DescriptorTable.pDescriptorRanges = &srvRange;
    param.ShaderVisibility = D3D12_SHADER_VISIBILITY_PIXEL;

    D3D12_STATIC_SAMPLER_DESC sampler{};
    sampler.Filter = D3D12_FILTER_MIN_MAG_MIP_LINEAR;
    sampler.AddressU = sampler.AddressV = sampler.AddressW = D3D12_TEXTURE_ADDRESS_MODE_CLAMP;
    sampler.ShaderRegister = 0;
    sampler.ShaderVisibility = D3D12_SHADER_VISIBILITY_PIXEL;
    sampler.MaxLOD = D3D12_FLOAT32_MAX;

    D3D12_ROOT_SIGNATURE_DESC rsDesc{};
    rsDesc.NumParameters = 1;
    rsDesc.pParameters = &param;
    rsDesc.NumStaticSamplers = 1;
    rsDesc.pStaticSamplers = &sampler;
    rsDesc.Flags = D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT;

    ComPtr<ID3DBlob> rsBlob, rsErrors;
    if (FAILED(D3D12SerializeRootSignature(&rsDesc, D3D_ROOT_SIGNATURE_VERSION_1, &rsBlob, &rsErrors))) {
        Log("ColorBlit: root sig serialize failed %.*s\n",
            rsErrors ? static_cast<int>(rsErrors->GetBufferSize()) : 0,
            rsErrors ? static_cast<const char*>(rsErrors->GetBufferPointer()) : "");
        return false;
    }
    if (FAILED(device->CreateRootSignature(0, rsBlob->GetBufferPointer(), rsBlob->GetBufferSize(), IID_PPV_ARGS(&m_rootSig)))) {
        Log("ColorBlit: failed to create root sig\n");
        return false;
    }

    ComPtr<ID3DBlob> vsBlob, psBlob;
    if (!CompileShader(kVsSource, "VSMain", "vs_5_0", vsBlob) ||
        !CompileShader(kPsSource, "PSMain", "ps_5_0", psBlob)) {
        return false;
    }

    D3D12_GRAPHICS_PIPELINE_STATE_DESC pso{};
    pso.pRootSignature = m_rootSig.Get();
    pso.VS = { vsBlob->GetBufferPointer(), vsBlob->GetBufferSize() };
    pso.PS = { psBlob->GetBufferPointer(), psBlob->GetBufferSize() };
    pso.PrimitiveTopologyType = D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE;
    pso.NumRenderTargets = 1;
    pso.RTVFormats[0] = colorFormat;
    pso.SampleDesc.Count = 1;
    pso.SampleMask = UINT_MAX;
    pso.RasterizerState.FillMode = D3D12_FILL_MODE_SOLID;
    pso.RasterizerState.CullMode = D3D12_CULL_MODE_NONE;
    pso.BlendState.RenderTarget[0].RenderTargetWriteMask = D3D12_COLOR_WRITE_ENABLE_ALL;
    pso.DepthStencilState.DepthEnable = FALSE;
    pso.DepthStencilState.StencilEnable = FALSE;
    if (FAILED(device->CreateGraphicsPipelineState(&pso, IID_PPV_ARGS(&m_pso)))) {
        Log("ColorBlit: PSO create failed\n");
        return false;
    }

    // Overlay variant: same root signature, same fullscreen quad, premultiplied-alpha blend.
    // Read out of the game's own HUD pipelines (SrcBlend ONE, DestBlend INV_SRC_ALPHA, add) so
    // the second eye gets exactly the composite the first eye already has baked in.
    ComPtr<ID3DBlob> psOverlayBlob;
    if (CompileShader(kPsOverlaySource, "PSMain", "ps_5_0", psOverlayBlob)) {
        D3D12_GRAPHICS_PIPELINE_STATE_DESC ov = pso;
        ov.PS = { psOverlayBlob->GetBufferPointer(), psOverlayBlob->GetBufferSize() };
        auto& rt = ov.BlendState.RenderTarget[0];
        rt.BlendEnable = TRUE;
        rt.SrcBlend = D3D12_BLEND_ONE;
        rt.DestBlend = D3D12_BLEND_INV_SRC_ALPHA;
        rt.BlendOp = D3D12_BLEND_OP_ADD;
        rt.SrcBlendAlpha = D3D12_BLEND_ONE;
        rt.DestBlendAlpha = D3D12_BLEND_INV_SRC_ALPHA;
        rt.BlendOpAlpha = D3D12_BLEND_OP_ADD;
        rt.RenderTargetWriteMask = D3D12_COLOR_WRITE_ENABLE_ALL;
        if (FAILED(device->CreateGraphicsPipelineState(&ov, IID_PPV_ARGS(&m_psoOverlay)))) {
            // Not fatal: the eye still gets its image, just without the HUD.
            Log("ColorBlit: overlay PSO create failed -- second eye stays HUD-less\n");
            m_psoOverlay.Reset();
        }
        // Additive twin. A layer the engine consumes by its own arithmetic need not carry a
        // meaningful alpha, and a premultiplied blend of alpha-zero pixels is a no-op -- which
        // looks exactly like "nothing was drawn". Having both available makes that testable.
        rt.DestBlend = D3D12_BLEND_ONE;
        rt.DestBlendAlpha = D3D12_BLEND_ONE;
        if (FAILED(device->CreateGraphicsPipelineState(&ov, IID_PPV_ARGS(&m_psoOverlayAdd))))
            m_psoOverlayAdd.Reset();
        // STRAIGHT alpha. Read out of PipelineState_1216, the engine's own final composite: with
        // one overlay bound its arithmetic reduces to
        //     out = a*(overlay.rgb - scene) + scene   ==   lerp(scene, overlay.rgb, a)
        // so the layer's colour is NOT premultiplied. The distinction is the whole difference
        // between "only an outline" and "an outline plus a wash": measured on a dumped frame, a
        // premultiplied blend matches exactly at the outline core (alpha 255) and is 28x too
        // bright across the faint halo (alpha 2..15), which is where the wash comes from.
        rt.SrcBlend = D3D12_BLEND_SRC_ALPHA;
        rt.DestBlend = D3D12_BLEND_INV_SRC_ALPHA;
        rt.SrcBlendAlpha = D3D12_BLEND_ONE;
        rt.DestBlendAlpha = D3D12_BLEND_INV_SRC_ALPHA;
        if (FAILED(device->CreateGraphicsPipelineState(&ov, IID_PPV_ARGS(&m_psoOverlayStraight))))
            m_psoOverlayStraight.Reset();
    }

    // ---- the ported HUD composite ------------------------------------------------------------
    // Its own root signature: two SRVs in one table plus the constants as root constants, so
    // there is no per-frame constant buffer to allocate, upload or keep alive.
    {
        m_hudSrvHeap.Reset();
        D3D12_DESCRIPTOR_HEAP_DESC hd{};
        hd.NumDescriptors = kSlots * 4;   // scene, hud, hud-blur, exposure per slot
        hd.Type = D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV;
        hd.Flags = D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE;
        if (SUCCEEDED(device->CreateDescriptorHeap(&hd, IID_PPV_ARGS(&m_hudSrvHeap)))) {
            D3D12_DESCRIPTOR_RANGE range{};
            range.RangeType = D3D12_DESCRIPTOR_RANGE_TYPE_SRV;
            range.NumDescriptors = 4;                   // scene, hud, hud-blur, exposure
            range.BaseShaderRegister = 0;

            D3D12_ROOT_PARAMETER params[4]{};
            params[0].ParameterType = D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;
            params[0].DescriptorTable.NumDescriptorRanges = 1;
            params[0].DescriptorTable.pDescriptorRanges = &range;
            params[0].ShaderVisibility = D3D12_SHADER_VISIBILITY_PIXEL;
            params[1].ParameterType = D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS;
            params[1].Constants.ShaderRegister = 0;
            params[1].Constants.Num32BitValues = sizeof(HudParams) / 4;
            params[1].ShaderVisibility = D3D12_SHADER_VISIBILITY_PIXEL;
            // b1: the engine's frame constants, straight off its own buffer. A root CBV, because
            // that buffer rests in VERTEX_AND_CONSTANT_BUFFER -- the state a CBV read wants, so
            // nothing of the engine's has to be transitioned to read it.
            params[2].ParameterType = D3D12_ROOT_PARAMETER_TYPE_CBV;
            params[2].Descriptor.ShaderRegister = 1;
            params[2].ShaderVisibility = D3D12_SHADER_VISIBILITY_PIXEL;
            // b2: the composite's own constants, likewise off the engine's buffer.
            params[3].ParameterType = D3D12_ROOT_PARAMETER_TYPE_CBV;
            params[3].Descriptor.ShaderRegister = 2;
            params[3].ShaderVisibility = D3D12_SHADER_VISIBILITY_PIXEL;

            D3D12_STATIC_SAMPLER_DESC ss{};
            ss.Filter = D3D12_FILTER_MIN_MAG_MIP_LINEAR;
            ss.AddressU = ss.AddressV = ss.AddressW = D3D12_TEXTURE_ADDRESS_MODE_CLAMP;
            ss.ShaderRegister = 0;
            ss.ShaderVisibility = D3D12_SHADER_VISIBILITY_PIXEL;
            ss.MaxLOD = D3D12_FLOAT32_MAX;

            D3D12_ROOT_SIGNATURE_DESC rd{};
            rd.NumParameters = 4;
            rd.pParameters = params;
            rd.NumStaticSamplers = 1;
            rd.pStaticSamplers = &ss;
            rd.Flags = D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT;

            ComPtr<ID3DBlob> hb, he, hps;
            if (SUCCEEDED(D3D12SerializeRootSignature(&rd, D3D_ROOT_SIGNATURE_VERSION_1, &hb, &he)) &&
                SUCCEEDED(device->CreateRootSignature(0, hb->GetBufferPointer(),
                                                      hb->GetBufferSize(), IID_PPV_ARGS(&m_rootSigHud))) &&
                CompileShader(kPsHudSource, "PSMain", "ps_5_0", hps)) {
                D3D12_GRAPHICS_PIPELINE_STATE_DESC hp = pso;
                hp.pRootSignature = m_rootSigHud.Get();
                hp.PS = { hps->GetBufferPointer(), hps->GetBufferSize() };
                hp.BlendState.RenderTarget[0].BlendEnable = FALSE;   // full composite, not a blend
                if (FAILED(device->CreateGraphicsPipelineState(&hp, IID_PPV_ARGS(&m_psoHud)))) {
                    Log("ColorBlit: HUD composite PSO create failed\n");
                    m_psoHud.Reset();
                }
            } else {
                Log("ColorBlit: HUD composite root signature/shader failed %.*s\n",
                    he ? static_cast<int>(he->GetBufferSize()) : 0,
                    he ? static_cast<const char*>(he->GetBufferPointer()) : "");
                m_rootSigHud.Reset();
            }
        }
    }
    // ---- the barrel dot ----------------------------------------------------------------------
    // Root constants only: no descriptor table, no sampler, nothing to keep alive per frame.
    {
        D3D12_ROOT_PARAMETER dp{};
        dp.ParameterType = D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS;
        dp.Constants.ShaderRegister = 0;
        dp.Constants.Num32BitValues = 8;          // centre xy, radius xy, colour rgba
        dp.ShaderVisibility = D3D12_SHADER_VISIBILITY_ALL;   // the VS places the quad

        D3D12_ROOT_SIGNATURE_DESC dsig{};
        dsig.NumParameters = 1;
        dsig.pParameters = &dp;
        dsig.Flags = D3D12_ROOT_SIGNATURE_FLAG_NONE;

        ComPtr<ID3DBlob> db, de, dvs, dps;
        if (SUCCEEDED(D3D12SerializeRootSignature(&dsig, D3D_ROOT_SIGNATURE_VERSION_1, &db, &de)) &&
            SUCCEEDED(device->CreateRootSignature(0, db->GetBufferPointer(),
                                                  db->GetBufferSize(), IID_PPV_ARGS(&m_rootSigDot))) &&
            CompileShader(kDotSource, "VSMain", "vs_5_0", dvs) &&
            CompileShader(kDotSource, "PSMain", "ps_5_0", dps)) {
            D3D12_GRAPHICS_PIPELINE_STATE_DESC dpso = pso;
            dpso.pRootSignature = m_rootSigDot.Get();
            dpso.VS = { dvs->GetBufferPointer(), dvs->GetBufferSize() };
            dpso.PS = { dps->GetBufferPointer(), dps->GetBufferSize() };
            // MAIN's ImGui dot is blended numerically into the game's UNORM backbuffer. The eye
            // texture is typeless but its scene/HUD composite normally uses an sRGB RTV; reusing
            // that view here would encode the faint halo after blending and make it much brighter
            // than MAIN. Use the matching non-sRGB view only for this final dot pass.
            dpso.RTVFormats[0] = ToNonSrgbViewFormat(colorFormat);
            auto& drt = dpso.BlendState.RenderTarget[0];
            drt.BlendEnable = TRUE;
            drt.SrcBlend = D3D12_BLEND_SRC_ALPHA;
            drt.DestBlend = D3D12_BLEND_INV_SRC_ALPHA;
            drt.BlendOp = D3D12_BLEND_OP_ADD;
            drt.SrcBlendAlpha = D3D12_BLEND_ONE;
            drt.DestBlendAlpha = D3D12_BLEND_INV_SRC_ALPHA;
            drt.BlendOpAlpha = D3D12_BLEND_OP_ADD;
            drt.RenderTargetWriteMask = D3D12_COLOR_WRITE_ENABLE_ALL;
            if (FAILED(device->CreateGraphicsPipelineState(&dpso, IID_PPV_ARGS(&m_psoDot)))) {
                Log("ColorBlit: dot PSO create failed -- desktop second-eye mirror stays dotless\n");
                m_psoDot.Reset();
            }
        } else {
            Log("ColorBlit: dot root signature/shader failed %.*s\n",
                de ? static_cast<int>(de->GetBufferSize()) : 0,
                de ? static_cast<const char*>(de->GetBufferPointer()) : "");
            m_rootSigDot.Reset();
        }
    }
    return true;
}

bool ColorBlit::RecordDot(ID3D12GraphicsCommandList* cmdList,
                          ID3D12Resource* dstColor,
                          float ndcX, float ndcY,
                          float radiusPx,
                          float r, float g, float b, float a) {
    std::lock_guard<std::mutex> lock(m_mutex);
    if (!cmdList || !dstColor || !m_psoDot || !m_rootSigDot || !m_width || !m_height) return false;
    if (!(radiusPx > 0.0f)) return false;
    // Off-screen: nothing to draw, and saying so lets the caller count misses.
    if (!(ndcX > -1.5f && ndcX < 1.5f && ndcY > -1.5f && ndcY < 1.5f)) return false;

    const uint32_t slot = m_slot;
    m_slot = (m_slot + 1) % kSlots;

    D3D12_CPU_DESCRIPTOR_HANDLE rtvCpu = m_rtvHeap->GetCPUDescriptorHandleForHeapStart();
    rtvCpu.ptr += static_cast<SIZE_T>(slot) * m_rtvStride;
    D3D12_RENDER_TARGET_VIEW_DESC rtv{};
    rtv.Format = ToNonSrgbViewFormat(m_colorFormat);
    rtv.ViewDimension = D3D12_RTV_DIMENSION_TEXTURE2D;
    m_device->CreateRenderTargetView(dstColor, &rtv, rtvCpu);

    D3D12_VIEWPORT vp{};
    vp.Width = static_cast<float>(m_width);
    vp.Height = static_cast<float>(m_height);
    vp.MaxDepth = 1.0f;
    D3D12_RECT scissor{0, 0, static_cast<LONG>(m_width), static_cast<LONG>(m_height)};
    cmdList->RSSetViewports(1, &vp);
    cmdList->RSSetScissorRects(1, &scissor);
    cmdList->OMSetRenderTargets(1, &rtvCpu, FALSE, nullptr);

    const float k[8] = {
        ndcX, ndcY,
        (2.0f * radiusPx) / static_cast<float>(m_width),
        (2.0f * radiusPx) / static_cast<float>(m_height),
        r, g, b, a
    };
    cmdList->SetGraphicsRootSignature(m_rootSigDot.Get());
    cmdList->SetPipelineState(m_psoDot.Get());
    cmdList->SetGraphicsRoot32BitConstants(0, 8, k, 0);
    cmdList->IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    cmdList->DrawInstanced(kDotMeshVertexCount, 1, 0, 0);
    return true;
}

bool ColorBlit::RecordBlit(ID3D12GraphicsCommandList* cmdList,
                           ID3D12Resource* srcColor,
                           ID3D12Resource* dstColor) {
    std::lock_guard<std::mutex> lock(m_mutex);
    if (!cmdList || !srcColor || !dstColor || !m_pso) return false;

    const uint32_t slot = m_slot;
    m_slot = (m_slot + 1) % kSlots;

    D3D12_CPU_DESCRIPTOR_HANDLE srvCpu = m_srvHeap->GetCPUDescriptorHandleForHeapStart();
    srvCpu.ptr += static_cast<SIZE_T>(slot) * m_srvStride;
    D3D12_GPU_DESCRIPTOR_HANDLE srvGpu = m_srvHeap->GetGPUDescriptorHandleForHeapStart();
    srvGpu.ptr += static_cast<UINT64>(slot) * m_srvStride;
    D3D12_CPU_DESCRIPTOR_HANDLE rtvCpu = m_rtvHeap->GetCPUDescriptorHandleForHeapStart();
    rtvCpu.ptr += static_cast<SIZE_T>(slot) * m_rtvStride;

    D3D12_SHADER_RESOURCE_VIEW_DESC srv{};
    srv.Format = srcColor->GetDesc().Format;
    srv.ViewDimension = D3D12_SRV_DIMENSION_TEXTURE2D;
    srv.Shader4ComponentMapping = D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;
    srv.Texture2D.MipLevels = 1;
    m_device->CreateShaderResourceView(srcColor, &srv, srvCpu);

    D3D12_RENDER_TARGET_VIEW_DESC rtv{};
    rtv.Format = m_colorFormat;
    rtv.ViewDimension = D3D12_RTV_DIMENSION_TEXTURE2D;
    m_device->CreateRenderTargetView(dstColor, &rtv, rtvCpu);

    D3D12_VIEWPORT vp{};
    vp.Width = static_cast<float>(m_width);
    vp.Height = static_cast<float>(m_height);
    vp.MaxDepth = 1.0f;
    D3D12_RECT scissor{0, 0, static_cast<LONG>(m_width), static_cast<LONG>(m_height)};
    cmdList->RSSetViewports(1, &vp);
    cmdList->RSSetScissorRects(1, &scissor);
    cmdList->OMSetRenderTargets(1, &rtvCpu, FALSE, nullptr);

    cmdList->SetGraphicsRootSignature(m_rootSig.Get());
    cmdList->SetPipelineState(m_pso.Get());
    ID3D12DescriptorHeap* heaps[] = { m_srvHeap.Get() };
    cmdList->SetDescriptorHeaps(1, heaps);
    cmdList->SetGraphicsRootDescriptorTable(0, srvGpu);
    cmdList->IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
    cmdList->DrawInstanced(4, 1, 0, 0);
    return true;
}

bool ColorBlit::RecordHudComposite(ID3D12GraphicsCommandList* cmdList,
                                   ID3D12Resource* srcScene,
                                   ID3D12Resource* srcHud,
                                   ID3D12Resource* srcHudBlur,
                                   ID3D12Resource* srcExposure,
                                   ID3D12Resource* srcFrameCB,
                                   ID3D12Resource* srcHudCB,
                                   ID3D12Resource* dstColor,
                                   const HudParams& params) {
    std::lock_guard<std::mutex> lock(m_mutex);
    if (!cmdList || !srcScene || !srcHud || !srcHudBlur || !srcExposure || !srcFrameCB || !srcHudCB ||
        !dstColor || !m_psoHud || !m_hudSrvHeap) {
        return false;
    }

    const uint32_t slot = m_slot;
    m_slot = (m_slot + 1) % kSlots;

    D3D12_CPU_DESCRIPTOR_HANDLE srvCpu = m_hudSrvHeap->GetCPUDescriptorHandleForHeapStart();
    srvCpu.ptr += static_cast<SIZE_T>(slot) * 4 * m_srvStride;
    D3D12_GPU_DESCRIPTOR_HANDLE srvGpu = m_hudSrvHeap->GetGPUDescriptorHandleForHeapStart();
    srvGpu.ptr += static_cast<UINT64>(slot) * 4 * m_srvStride;
    D3D12_CPU_DESCRIPTOR_HANDLE rtvCpu = m_rtvHeap->GetCPUDescriptorHandleForHeapStart();
    rtvCpu.ptr += static_cast<SIZE_T>(slot) * m_rtvStride;

    // t0 = scene, in whatever format it already carries (VRCAM's is R11G11B10 linear, which is
    // the same kind of value the engine composites against).
    const D3D12_RESOURCE_DESC sceneDesc = srcScene->GetDesc();
    D3D12_SHADER_RESOURCE_VIEW_DESC sv{};
    sv.Format = sceneDesc.Format;
    sv.ViewDimension = D3D12_SRV_DIMENSION_TEXTURE2D;
    sv.Shader4ComponentMapping = D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;
    sv.Texture2D.MipLevels = 1;
    m_device->CreateShaderResourceView(srcScene, &sv, srvCpu);

    // t1 = HUD, with its WHOLE mip chain: the glow taps read mips 1, 2 and 4.
    const D3D12_RESOURCE_DESC hudDesc = srcHud->GetDesc();
    D3D12_CPU_DESCRIPTOR_HANDLE hudCpu = srvCpu;
    hudCpu.ptr += m_srvStride;
    D3D12_SHADER_RESOURCE_VIEW_DESC hv{};
    hv.Format = (hudDesc.Format == DXGI_FORMAT_R8G8B8A8_TYPELESS)
                    ? DXGI_FORMAT_R8G8B8A8_UNORM_SRGB : hudDesc.Format;
    hv.ViewDimension = D3D12_SRV_DIMENSION_TEXTURE2D;
    hv.Shader4ComponentMapping = D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;
    hv.Texture2D.MipLevels = hudDesc.MipLevels ? hudDesc.MipLevels : 1;
    m_device->CreateShaderResourceView(srcHud, &hv, hudCpu);

    // t2 = the engine's blurred-HUD pyramid, whole chain (the halo samples lod 1.8).
    const D3D12_RESOURCE_DESC blurDesc = srcHudBlur->GetDesc();
    D3D12_CPU_DESCRIPTOR_HANDLE blurCpu = hudCpu;
    blurCpu.ptr += m_srvStride;
    D3D12_SHADER_RESOURCE_VIEW_DESC bv = hv;
    bv.Format = (blurDesc.Format == DXGI_FORMAT_R8G8B8A8_TYPELESS)
                    ? DXGI_FORMAT_R8G8B8A8_UNORM_SRGB : blurDesc.Format;
    bv.Texture2D.MipLevels = blurDesc.MipLevels ? blurDesc.MipLevels : 1;
    m_device->CreateShaderResourceView(srcHudBlur, &bv, blurCpu);

    // t3 = FrameExposureData, read raw so the shader can take dword 6 exactly as the engine does.
    D3D12_CPU_DESCRIPTOR_HANDLE expoCpu = blurCpu;
    expoCpu.ptr += m_srvStride;
    D3D12_SHADER_RESOURCE_VIEW_DESC ev{};
    ev.Format = DXGI_FORMAT_R32_TYPELESS;
    ev.ViewDimension = D3D12_SRV_DIMENSION_BUFFER;
    ev.Shader4ComponentMapping = D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;
    ev.Buffer.NumElements = static_cast<UINT>(srcExposure->GetDesc().Width / 4);
    ev.Buffer.Flags = D3D12_BUFFER_SRV_FLAG_RAW;
    m_device->CreateShaderResourceView(srcExposure, &ev, expoCpu);

    D3D12_RENDER_TARGET_VIEW_DESC rtv{};
    rtv.Format = m_colorFormat;
    rtv.ViewDimension = D3D12_RTV_DIMENSION_TEXTURE2D;
    m_device->CreateRenderTargetView(dstColor, &rtv, rtvCpu);

    D3D12_VIEWPORT vp{};
    vp.Width = static_cast<float>(m_width);
    vp.Height = static_cast<float>(m_height);
    vp.MaxDepth = 1.0f;
    D3D12_RECT scissor{0, 0, static_cast<LONG>(m_width), static_cast<LONG>(m_height)};
    cmdList->RSSetViewports(1, &vp);
    cmdList->RSSetScissorRects(1, &scissor);
    cmdList->OMSetRenderTargets(1, &rtvCpu, FALSE, nullptr);

    cmdList->SetGraphicsRootSignature(m_rootSigHud.Get());
    cmdList->SetPipelineState(m_psoHud.Get());
    ID3D12DescriptorHeap* heaps[] = { m_hudSrvHeap.Get() };
    cmdList->SetDescriptorHeaps(1, heaps);
    HudParams p = params;
    p.targetW = static_cast<float>(m_width);
    p.targetH = static_cast<float>(m_height);
    cmdList->SetGraphicsRootDescriptorTable(0, srvGpu);
    cmdList->SetGraphicsRoot32BitConstants(1, sizeof(HudParams) / 4, &p, 0);
    cmdList->SetGraphicsRootConstantBufferView(2, srcFrameCB->GetGPUVirtualAddress());
    cmdList->SetGraphicsRootConstantBufferView(3, srcHudCB->GetGPUVirtualAddress());
    cmdList->IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
    cmdList->DrawInstanced(4, 1, 0, 0);
    return true;
}

bool ColorBlit::RecordOverlay(ID3D12GraphicsCommandList* cmdList,
                              ID3D12Resource* srcOverlay,
                              ID3D12Resource* dstColor,
                              int mode,
                              bool pixelExact,
                              float offX,
                              float offY) {
    std::lock_guard<std::mutex> lock(m_mutex);
    if (!cmdList || !srcOverlay || !dstColor) return false;
    ID3D12PipelineState* pso = m_psoOverlay.Get();
    if (mode == 1) pso = m_pso.Get();                 // opaque replace, diagnostic
    else if (mode == 2 && m_psoOverlayAdd) pso = m_psoOverlayAdd.Get();
    else if (mode == 3 && m_psoOverlayStraight) pso = m_psoOverlayStraight.Get();
    if (!pso) return false;

    const uint32_t slot = m_slot;
    m_slot = (m_slot + 1) % kSlots;

    D3D12_CPU_DESCRIPTOR_HANDLE srvCpu = m_srvHeap->GetCPUDescriptorHandleForHeapStart();
    srvCpu.ptr += static_cast<SIZE_T>(slot) * m_srvStride;
    D3D12_GPU_DESCRIPTOR_HANDLE srvGpu = m_srvHeap->GetGPUDescriptorHandleForHeapStart();
    srvGpu.ptr += static_cast<UINT64>(slot) * m_srvStride;
    D3D12_CPU_DESCRIPTOR_HANDLE rtvCpu = m_rtvHeap->GetCPUDescriptorHandleForHeapStart();
    rtvCpu.ptr += static_cast<SIZE_T>(slot) * m_rtvStride;

    // Mip 0 only. The HUD surface carries a glow mip chain the engine builds after the draws;
    // sampling anything but the top level would smear the HUD.
    const D3D12_RESOURCE_DESC sd = srcOverlay->GetDesc();
    D3D12_SHADER_RESOURCE_VIEW_DESC srv{};
    srv.Format = (sd.Format == DXGI_FORMAT_R8G8B8A8_TYPELESS)
                     ? DXGI_FORMAT_R8G8B8A8_UNORM_SRGB : sd.Format;
    srv.ViewDimension = D3D12_SRV_DIMENSION_TEXTURE2D;
    srv.Shader4ComponentMapping = D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;
    srv.Texture2D.MostDetailedMip = 0;
    srv.Texture2D.MipLevels = 1;
    m_device->CreateShaderResourceView(srcOverlay, &srv, srvCpu);

    D3D12_RENDER_TARGET_VIEW_DESC rtv{};
    rtv.Format = m_colorFormat;
    rtv.ViewDimension = D3D12_RTV_DIMENSION_TEXTURE2D;
    m_device->CreateRenderTargetView(dstColor, &rtv, rtvCpu);

    D3D12_VIEWPORT vp{};
    vp.MaxDepth = 1.0f;
    if (pixelExact) {
        // The quad always spans the VIEWPORT with uv 0..1, so sizing the viewport to the SOURCE
        // makes destination texel (x,y) sample source texel (x,y) exactly, and the scissor clips
        // whatever hangs off the target. No shader constant, no root-signature change.
        vp.TopLeftX = offX;
        vp.TopLeftY = offY;
        vp.Width  = static_cast<float>(sd.Width);
        vp.Height = static_cast<float>(sd.Height);
    } else {
        vp.Width = static_cast<float>(m_width);
        vp.Height = static_cast<float>(m_height);
    }
    D3D12_RECT scissor{0, 0, static_cast<LONG>(m_width), static_cast<LONG>(m_height)};
    cmdList->RSSetViewports(1, &vp);
    cmdList->RSSetScissorRects(1, &scissor);
    cmdList->OMSetRenderTargets(1, &rtvCpu, FALSE, nullptr);

    cmdList->SetGraphicsRootSignature(m_rootSig.Get());
    cmdList->SetPipelineState(pso);
    ID3D12DescriptorHeap* heaps[] = { m_srvHeap.Get() };
    cmdList->SetDescriptorHeaps(1, heaps);
    cmdList->SetGraphicsRootDescriptorTable(0, srvGpu);
    cmdList->IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
    cmdList->DrawInstanced(4, 1, 0, 0);
    return true;
}
