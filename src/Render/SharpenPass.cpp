#include "Render/SharpenPass.hpp"
#if !CVR_RUST_VRIK
// The C++ build's pass; the Rust build's is rust/crates/cvr-gpu/src/sharpen_pass.rs (the header calls it).
#include "Render/CommandResources.hpp"

#include <algorithm>
#include <cmath>
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
    static const float2 positions[3] = {
        float2(-1.0,  1.0),
        float2( 3.0,  1.0),
        float2(-1.0, -3.0)
    };
    static const float2 uvs[3] = {
        float2(0.0, 0.0),
        float2(2.0, 0.0),
        float2(0.0, 2.0)
    };
    VSOut o;
    o.position = float4(positions[vid], 0.0, 1.0);
    o.uv = uvs[vid];
    return o;
}
)";

// CAS pixel shader.
//   sharpness peak = -1 / (8 - 3*filterParam.x)
//   out = saturate(lerp(center, sharpened, 4*amount))
constexpr char kPsSource[] = R"(
cbuffer FilterCB : register(b0) {
    float4 filterParam; // x=Emphasis, y=4*amount, z=typed sRGB RTV
}
Texture2D<float4> g_source : register(t0);
SamplerState g_sampler : register(s0);

struct VSOut {
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

float3 tap(float2 uv, float2 off) {
    return g_source.SampleLevel(g_sampler, uv + off, 0.0).rgb;
}

float4 PSMain(VSOut input) : SV_Target {
    float w, h;
    g_source.GetDimensions(w, h);
    float2 inv = float2(1.0 / w, 1.0 / h);
    float2 uv = input.uv;

    float3 a = tap(uv, float2(-inv.x, -inv.y));
    float3 b = tap(uv, float2(    0.0, -inv.y));
    float3 c = tap(uv, float2( inv.x, -inv.y));
    float3 d = tap(uv, float2(-inv.x,    0.0));
    float3 e = tap(uv, float2(    0.0,    0.0));
    float3 f = tap(uv, float2( inv.x,    0.0));
    float3 g = tap(uv, float2(-inv.x,  inv.y));
    float3 hh= tap(uv, float2(    0.0,  inv.y));
    float3 i = tap(uv, float2( inv.x,  inv.y));

    float3 mn = min(min(min(d, e), min(f, b)), hh);
    mn += min(mn, min(min(a, c), min(g, i)));
    float3 mx = max(max(max(d, e), max(f, b)), hh);
    mx += max(mx, max(max(a, c), max(g, i)));

    float3 rcpMx = rcp(max(mx, 1e-5));
    float3 amp = saturate(min(mn, 2.0 - mx) * rcpMx);
    amp = sqrt(amp);

    float peak = -1.0 / (8.0 - 3.0 * saturate(filterParam.x));
    float3 wt = amp * peak;
    float3 rcpW = rcp(1.0 + 4.0 * wt);
    float3 sharp = saturate((b * wt + d * wt + f * wt + hh * wt + e) * rcpW);

    float3 outc = saturate(lerp(e, sharp, filterParam.y));
    if (filterParam.z > 0.5) {
        outc = float3(outc.r <= 0.04045 ? outc.r / 12.92 : pow((outc.r + 0.055) / 1.055, 2.4),
                      outc.g <= 0.04045 ? outc.g / 12.92 : pow((outc.g + 0.055) / 1.055, 2.4),
                      outc.b <= 0.04045 ? outc.b / 12.92 : pow((outc.b + 0.055) / 1.055, 2.4));
    }
    return float4(outc, 1.0);
}
)";

DXGI_FORMAT ColorViewFormat(DXGI_FORMAT format) {
    switch (format) {
    case DXGI_FORMAT_R8G8B8A8_TYPELESS:
    case DXGI_FORMAT_R8G8B8A8_UNORM:
    case DXGI_FORMAT_R8G8B8A8_UNORM_SRGB: return DXGI_FORMAT_R8G8B8A8_UNORM;
    case DXGI_FORMAT_B8G8R8A8_TYPELESS:
    case DXGI_FORMAT_B8G8R8A8_UNORM:
    case DXGI_FORMAT_B8G8R8A8_UNORM_SRGB: return DXGI_FORMAT_B8G8R8A8_UNORM;
    case DXGI_FORMAT_R10G10B10A2_TYPELESS:
    case DXGI_FORMAT_R10G10B10A2_UNORM: return DXGI_FORMAT_R10G10B10A2_UNORM;
    default: return DXGI_FORMAT_UNKNOWN;
    }
}

bool IsSrgb(DXGI_FORMAT format) {
    return format == DXGI_FORMAT_R8G8B8A8_UNORM_SRGB || format == DXGI_FORMAT_B8G8R8A8_UNORM_SRGB;
}

bool Compatible(const D3D12_RESOURCE_DESC& desc, DXGI_FORMAT format, uint32_t width, uint32_t height) {
    return desc.Dimension == D3D12_RESOURCE_DIMENSION_TEXTURE2D &&
        ColorViewFormat(desc.Format) == format && desc.Width == width && desc.Height == height &&
        desc.DepthOrArraySize == 1 && desc.MipLevels == 1 && desc.SampleDesc.Count == 1;
}

bool CompileShader(const char* src, const char* entry, const char* target, ComPtr<ID3DBlob>& out) {
    UINT flags = D3DCOMPILE_ENABLE_STRICTNESS;
    ComPtr<ID3DBlob> errors;
    HRESULT hr = D3DCompile(src, strlen(src), nullptr, nullptr, nullptr,
        entry, target, flags, 0, &out, &errors);
    if (FAILED(hr)) {
        Log("SharpenPass: shader compile failed %s/%s hr=0x%08X %.*s\n",
            entry, target, static_cast<unsigned>(hr),
            errors ? static_cast<int>(errors->GetBufferSize()) : 0,
            errors ? static_cast<const char*>(errors->GetBufferPointer()) : "");
        return false;
    }
    return true;
}

}  // namespace

SharpenPass::~SharpenPass() { Shutdown(); }

void SharpenPass::Shutdown() {
    std::lock_guard<std::mutex> lock(m_mutex);
    m_pso.Reset();
    m_rootSig.Reset();
    m_srvHeap.Reset();
    m_rtvHeap.Reset();
    m_scratch.Reset();
    m_device.Reset();
    m_colorFormat = DXGI_FORMAT_UNKNOWN;
    m_width = m_height = 0;
}

bool SharpenPass::EnsureInitialized(ID3D12Device* device,
                                    DXGI_FORMAT colorFormat,
                                    uint32_t width,
                                    uint32_t height) {
    std::lock_guard<std::mutex> lock(m_mutex);
    const auto scratchFormat = ColorViewFormat(colorFormat);
    if (!IsSrgb(colorFormat)) colorFormat = scratchFormat;
    if (!device || !width || !height || colorFormat == DXGI_FORMAT_UNKNOWN) return false;
    if (m_device.Get() == device &&
        m_colorFormat == colorFormat &&
        m_width == width && m_height == height && m_pso) {
        return true;
    }
    m_pso.Reset();
    m_rootSig.Reset();
    m_srvHeap.Reset();
    m_rtvHeap.Reset();
    m_scratch.Reset();

    m_device = device;
    m_colorFormat = colorFormat;
    m_width = width;
    m_height = height;

    D3D12_DESCRIPTOR_HEAP_DESC srvHeapDesc{};
    srvHeapDesc.NumDescriptors = 1;
    srvHeapDesc.Type = D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV;
    srvHeapDesc.Flags = D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE;
    if (FAILED(device->CreateDescriptorHeap(&srvHeapDesc, IID_PPV_ARGS(&m_srvHeap)))) {
        Log("SharpenPass: failed to create SRV heap\n");
        return false;
    }
    m_srvHeap->SetName(L"SharpenPass_SRV_heap");

    D3D12_DESCRIPTOR_HEAP_DESC rtvHeapDesc{};
    rtvHeapDesc.NumDescriptors = 1;
    rtvHeapDesc.Type = D3D12_DESCRIPTOR_HEAP_TYPE_RTV;
    if (FAILED(device->CreateDescriptorHeap(&rtvHeapDesc, IID_PPV_ARGS(&m_rtvHeap)))) {
        Log("SharpenPass: failed to create RTV heap\n");
        return false;
    }
    m_rtvHeap->SetName(L"SharpenPass_RTV_heap");

    D3D12_HEAP_PROPERTIES heap{};
    heap.Type = D3D12_HEAP_TYPE_DEFAULT;
    D3D12_RESOURCE_DESC scratch{};
    scratch.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
    scratch.Width = width;
    scratch.Height = height;
    scratch.DepthOrArraySize = 1;
    scratch.MipLevels = 1;
    scratch.Format = scratchFormat;
    scratch.SampleDesc.Count = 1;
    if (FAILED(device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE,
            &scratch, D3D12_RESOURCE_STATE_COPY_DEST, nullptr, IID_PPV_ARGS(&m_scratch)))) {
        Log("SharpenPass: failed to create SRV scratch\n");
        return false;
    }
    m_scratch->SetName(L"CAS_Sharpening_SRV_scratch");
    D3D12_SHADER_RESOURCE_VIEW_DESC srv{};
    srv.Format = scratchFormat;
    srv.ViewDimension = D3D12_SRV_DIMENSION_TEXTURE2D;
    srv.Shader4ComponentMapping = D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;
    srv.Texture2D.MipLevels = 1;
    device->CreateShaderResourceView(m_scratch.Get(), &srv, m_srvHeap->GetCPUDescriptorHandleForHeapStart());

    D3D12_DESCRIPTOR_RANGE srvRange{};
    srvRange.RangeType = D3D12_DESCRIPTOR_RANGE_TYPE_SRV;
    srvRange.NumDescriptors = 1;
    srvRange.BaseShaderRegister = 0;

    D3D12_ROOT_PARAMETER params[2] = {};
    params[0].ParameterType = D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS;
    params[0].Constants.ShaderRegister = 0;
    params[0].Constants.Num32BitValues = 4;
    params[0].ShaderVisibility = D3D12_SHADER_VISIBILITY_PIXEL;
    params[1].ParameterType = D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;
    params[1].DescriptorTable.NumDescriptorRanges = 1;
    params[1].DescriptorTable.pDescriptorRanges = &srvRange;
    params[1].ShaderVisibility = D3D12_SHADER_VISIBILITY_PIXEL;

    D3D12_STATIC_SAMPLER_DESC sampler{};
    sampler.Filter = D3D12_FILTER_MIN_MAG_MIP_POINT;
    sampler.AddressU = sampler.AddressV = sampler.AddressW = D3D12_TEXTURE_ADDRESS_MODE_CLAMP;
    sampler.ShaderRegister = 0;
    sampler.ShaderVisibility = D3D12_SHADER_VISIBILITY_PIXEL;
    sampler.MaxLOD = D3D12_FLOAT32_MAX;

    D3D12_ROOT_SIGNATURE_DESC rsDesc{};
    rsDesc.NumParameters = 2;
    rsDesc.pParameters = params;
    rsDesc.NumStaticSamplers = 1;
    rsDesc.pStaticSamplers = &sampler;
    rsDesc.Flags = D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT;

    ComPtr<ID3DBlob> rsBlob, rsErrors;
    if (FAILED(D3D12SerializeRootSignature(&rsDesc, D3D_ROOT_SIGNATURE_VERSION_1, &rsBlob, &rsErrors))) {
        Log("SharpenPass: root sig serialize failed %.*s\n",
            rsErrors ? static_cast<int>(rsErrors->GetBufferSize()) : 0,
            rsErrors ? static_cast<const char*>(rsErrors->GetBufferPointer()) : "");
        return false;
    }
    if (FAILED(device->CreateRootSignature(0, rsBlob->GetBufferPointer(),
            rsBlob->GetBufferSize(), IID_PPV_ARGS(&m_rootSig)))) {
        Log("SharpenPass: failed to create root sig\n");
        return false;
    }
    m_rootSig->SetName(L"SharpenPass_RootSig");

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
        Log("SharpenPass: PSO create failed\n");
        return false;
    }
    m_pso->SetName(L"SharpenPass_PSO");
    Log("SharpenPass: initialized %ux%u colorFmt=%u\n",
        width, height, static_cast<unsigned>(colorFormat));
    return true;
}

bool SharpenPass::RecordSharpen(ID3D12GraphicsCommandList* cmdList,
                                ID3D12Resource* srcColor,
                                ID3D12Resource* dstColor,
                                float sharpness,
                                float amount) {
    std::lock_guard<std::mutex> lock(m_mutex);
    if (!cmdList || !srcColor || !dstColor || !m_pso ||
        !std::isfinite(sharpness) || !std::isfinite(amount) ||
        !Compatible(srcColor->GetDesc(), ColorViewFormat(m_colorFormat), m_width, m_height) ||
        !Compatible(dstColor->GetDesc(), ColorViewFormat(m_colorFormat), m_width, m_height) ||
        (IsSrgb(dstColor->GetDesc().Format) && dstColor->GetDesc().Format != m_colorFormat) ||
        !(dstColor->GetDesc().Flags & D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET)) return false;

    if (!cvr::gpu::KeepCommandResources(cmdList, {srcColor, dstColor, m_scratch.Get(),
            m_srvHeap.Get(), m_rtvHeap.Get(), m_rootSig.Get(), m_pso.Get()})) {
        return false;
    }
    const float constants[4]{std::clamp(sharpness, 0.0f, 1.0f),
        4.0f * std::clamp(amount, 0.0f, 1.0f), IsSrgb(m_colorFormat) ? 1.0f : 0.0f, 0.0f};

    // Only the private texture changes state; capture/framegen sources keep
    // their shared COPY_SOURCE state. The immutable SRV is safe across eyes.
    cmdList->CopyResource(m_scratch.Get(), srcColor);
    D3D12_RESOURCE_BARRIER barrier{};
    barrier.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Transition.pResource = m_scratch.Get();
    barrier.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    barrier.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_DEST;
    barrier.Transition.StateAfter = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
    cmdList->ResourceBarrier(1, &barrier);

    D3D12_RESOURCE_BARRIER targetBarrier{};
    if (srcColor == dstColor) {
        targetBarrier.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        targetBarrier.Transition = {dstColor, D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
            D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_RENDER_TARGET};
        cmdList->ResourceBarrier(1, &targetBarrier);
    }

    auto rtvCpu = m_rtvHeap->GetCPUDescriptorHandleForHeapStart();
    D3D12_RENDER_TARGET_VIEW_DESC rtvDesc{};
    rtvDesc.Format = m_colorFormat;
    rtvDesc.ViewDimension = D3D12_RTV_DIMENSION_TEXTURE2D;
    m_device->CreateRenderTargetView(dstColor, &rtvDesc, rtvCpu);

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
    cmdList->SetGraphicsRoot32BitConstants(0, 4, constants, 0);
    cmdList->SetGraphicsRootDescriptorTable(1, m_srvHeap->GetGPUDescriptorHandleForHeapStart());
    cmdList->IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    cmdList->DrawInstanced(3, 1, 0, 0);
    if (srcColor == dstColor) {
        std::swap(targetBarrier.Transition.StateBefore, targetBarrier.Transition.StateAfter);
        cmdList->ResourceBarrier(1, &targetBarrier);
    }
    std::swap(barrier.Transition.StateBefore, barrier.Transition.StateAfter);
    cmdList->ResourceBarrier(1, &barrier);
    return true;
}
#endif
