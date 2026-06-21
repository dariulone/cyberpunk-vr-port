#include "depth_resolve.h"

#include <cstdio>
#include <d3dcompiler.h>

extern void Log(const char* fmt, ...);

namespace {
using Microsoft::WRL::ComPtr;

// Fullscreen-triangle VS (no vertex buffer).
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

// PS: sample depth plane 0 of R32G8X24_TYPELESS via R32_FLOAT_X8X24_TYPELESS
// SRV (D3D12 reads only the R channel = the 32-bit float depth). Write that
// value to SV_Depth so the output goes into the depth plane of the bound DSV.
// No color attachment.
constexpr char kPsSource[] = R"(
Texture2D<float> g_srcDepth : register(t0);
SamplerState g_pointClamp   : register(s0);

struct VSOut {
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

float PSMain(VSOut input) : SV_Depth {
    return g_srcDepth.SampleLevel(g_pointClamp, input.uv, 0.0);
}
)";

// Same sample, but written to a COLOR target (SV_Target) so the output is a plain
// R32_FLOAT texture CUDA can import (depth-stencil outputs cannot be imported).
constexpr char kPsColorSource[] = R"(
Texture2D<float> g_srcDepth : register(t0);
SamplerState g_pointClamp   : register(s0);

struct VSOut {
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

float PSMain(VSOut input) : SV_Target {
    return g_srcDepth.SampleLevel(g_pointClamp, input.uv, 0.0);
}
)";

bool CompileShader(const char* src, const char* entry, const char* target, ComPtr<ID3DBlob>& out) {
    UINT flags = D3DCOMPILE_ENABLE_STRICTNESS;
    ComPtr<ID3DBlob> errors;
    HRESULT hr = D3DCompile(src, strlen(src), nullptr, nullptr, nullptr,
        entry, target, flags, 0, &out, &errors);
    if (FAILED(hr)) {
        Log("DepthResolve: shader compile failed %s/%s hr=0x%08X %.*s\n",
            entry, target, static_cast<unsigned>(hr),
            errors ? static_cast<int>(errors->GetBufferSize()) : 0,
            errors ? static_cast<const char*>(errors->GetBufferPointer()) : "");
        return false;
    }
    return true;
}

}  // namespace

DepthResolve::~DepthResolve() { Shutdown(); }

void DepthResolve::Shutdown() {
    std::lock_guard<std::mutex> lock(m_mutex);
    m_pso.Reset();
    m_rootSig.Reset();
    m_srvHeap.Reset();
    m_dsvHeap.Reset();
    m_psoColor.Reset();
    m_srvHeapColor.Reset();
    m_rtvHeap.Reset();
    m_device.Reset();
    m_outputFormat = DXGI_FORMAT_UNKNOWN;
    m_width = m_height = 0;
}

// Lazy-build the color (R32_FLOAT RTV) pipeline. Reuses the existing root sig
// (SRV table + point sampler) and VS; only the PS (SV_Target) and PSO differ.
bool DepthResolve::EnsureColorPipeline() {
    if (m_psoColor) return true;
    if (!m_device || !m_rootSig) return false;

    D3D12_DESCRIPTOR_HEAP_DESC srvDesc{};
    srvDesc.NumDescriptors = 1;
    srvDesc.Type = D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV;
    srvDesc.Flags = D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE;
    if (FAILED(m_device->CreateDescriptorHeap(&srvDesc, IID_PPV_ARGS(&m_srvHeapColor)))) {
        Log("DepthResolve: color SRV heap failed\n");
        return false;
    }
    m_srvHeapColor->SetName(L"DepthResolve_SRV_color");

    D3D12_DESCRIPTOR_HEAP_DESC rtvDesc{};
    rtvDesc.NumDescriptors = 1;
    rtvDesc.Type = D3D12_DESCRIPTOR_HEAP_TYPE_RTV;
    if (FAILED(m_device->CreateDescriptorHeap(&rtvDesc, IID_PPV_ARGS(&m_rtvHeap)))) {
        Log("DepthResolve: RTV heap failed\n");
        return false;
    }
    m_rtvHeap->SetName(L"DepthResolve_RTV");

    ComPtr<ID3DBlob> vsBlob, psBlob;
    if (!CompileShader(kVsSource, "VSMain", "vs_5_0", vsBlob) ||
        !CompileShader(kPsColorSource, "PSMain", "ps_5_0", psBlob)) {
        return false;
    }
    D3D12_GRAPHICS_PIPELINE_STATE_DESC pso{};
    pso.pRootSignature = m_rootSig.Get();
    pso.VS = { vsBlob->GetBufferPointer(), vsBlob->GetBufferSize() };
    pso.PS = { psBlob->GetBufferPointer(), psBlob->GetBufferSize() };
    pso.PrimitiveTopologyType = D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE;
    pso.NumRenderTargets = 1;
    pso.RTVFormats[0] = DXGI_FORMAT_R32_FLOAT;
    pso.SampleDesc.Count = 1;
    pso.SampleMask = UINT_MAX;
    pso.RasterizerState.FillMode = D3D12_FILL_MODE_SOLID;
    pso.RasterizerState.CullMode = D3D12_CULL_MODE_NONE;
    pso.BlendState.RenderTarget[0].RenderTargetWriteMask = D3D12_COLOR_WRITE_ENABLE_ALL;
    pso.DepthStencilState.DepthEnable = FALSE;
    pso.DepthStencilState.StencilEnable = FALSE;
    if (FAILED(m_device->CreateGraphicsPipelineState(&pso, IID_PPV_ARGS(&m_psoColor)))) {
        Log("DepthResolve: color PSO create failed\n");
        return false;
    }
    m_psoColor->SetName(L"DepthResolve_PSO_color");
    return true;
}

bool DepthResolve::EnsureInitialized(ID3D12Device* device,
                                      DXGI_FORMAT depthOutputFormat,
                                      uint32_t width,
                                      uint32_t height) {
    std::lock_guard<std::mutex> lock(m_mutex);
    if (!device || !width || !height) return false;
    if (m_device.Get() == device &&
        m_outputFormat == depthOutputFormat &&
        m_width == width && m_height == height && m_pso) {
        return true;
    }
    m_pso.Reset();
    m_rootSig.Reset();
    m_srvHeap.Reset();
    m_dsvHeap.Reset();
    m_device = device;
    m_outputFormat = depthOutputFormat;
    m_width = width;
    m_height = height;

    D3D12_DESCRIPTOR_HEAP_DESC srvDesc{};
    srvDesc.NumDescriptors = 1;
    srvDesc.Type = D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV;
    srvDesc.Flags = D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE;
    if (FAILED(device->CreateDescriptorHeap(&srvDesc, IID_PPV_ARGS(&m_srvHeap)))) {
        Log("DepthResolve: failed to create SRV heap\n");
        return false;
    }
    m_srvHeap->SetName(L"DepthResolve_SRV_heap");

    D3D12_DESCRIPTOR_HEAP_DESC dsvDesc{};
    dsvDesc.NumDescriptors = 1;
    dsvDesc.Type = D3D12_DESCRIPTOR_HEAP_TYPE_DSV;
    if (FAILED(device->CreateDescriptorHeap(&dsvDesc, IID_PPV_ARGS(&m_dsvHeap)))) {
        Log("DepthResolve: failed to create DSV heap\n");
        return false;
    }
    m_dsvHeap->SetName(L"DepthResolve_DSV_heap");

    // Root sig: SRV table t0 + static point sampler.
    D3D12_DESCRIPTOR_RANGE range{};
    range.RangeType = D3D12_DESCRIPTOR_RANGE_TYPE_SRV;
    range.NumDescriptors = 1;
    range.BaseShaderRegister = 0;
    D3D12_ROOT_PARAMETER param{};
    param.ParameterType = D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;
    param.DescriptorTable.NumDescriptorRanges = 1;
    param.DescriptorTable.pDescriptorRanges = &range;
    param.ShaderVisibility = D3D12_SHADER_VISIBILITY_PIXEL;

    D3D12_STATIC_SAMPLER_DESC sampler{};
    sampler.Filter = D3D12_FILTER_MIN_MAG_MIP_POINT;
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
        Log("DepthResolve: root sig serialize failed %.*s\n",
            rsErrors ? static_cast<int>(rsErrors->GetBufferSize()) : 0,
            rsErrors ? static_cast<const char*>(rsErrors->GetBufferPointer()) : "");
        return false;
    }
    if (FAILED(device->CreateRootSignature(0, rsBlob->GetBufferPointer(),
            rsBlob->GetBufferSize(), IID_PPV_ARGS(&m_rootSig)))) {
        Log("DepthResolve: failed to create root sig\n");
        return false;
    }
    m_rootSig->SetName(L"DepthResolve_RootSig");

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
    pso.NumRenderTargets = 0;  // depth-only
    pso.DSVFormat = depthOutputFormat;
    pso.SampleDesc.Count = 1;
    pso.SampleMask = UINT_MAX;
    pso.RasterizerState.FillMode = D3D12_FILL_MODE_SOLID;
    pso.RasterizerState.CullMode = D3D12_CULL_MODE_NONE;
    // No color writes, all depth writes — shader returns SV_Depth.
    pso.BlendState.RenderTarget[0].RenderTargetWriteMask = 0;
    pso.DepthStencilState.DepthEnable = TRUE;
    pso.DepthStencilState.DepthWriteMask = D3D12_DEPTH_WRITE_MASK_ALL;
    // Always write the depth coming from the shader (sample) — overwrites any
    // initial state in the destination.
    pso.DepthStencilState.DepthFunc = D3D12_COMPARISON_FUNC_ALWAYS;
    pso.DepthStencilState.StencilEnable = FALSE;
    if (FAILED(device->CreateGraphicsPipelineState(&pso, IID_PPV_ARGS(&m_pso)))) {
        Log("DepthResolve: PSO create failed\n");
        return false;
    }
    m_pso->SetName(L"DepthResolve_PSO");
    Log("DepthResolve: initialized %ux%u outputFmt=%u\n", width, height,
        static_cast<unsigned>(depthOutputFormat));
    return true;
}

bool DepthResolve::RecordResolve(ID3D12GraphicsCommandList* cmdList,
                                 ID3D12Resource* src,
                                 ID3D12Resource* dst) {
    std::lock_guard<std::mutex> lock(m_mutex);
    if (!cmdList || !src || !dst || !m_pso) return false;

    // SRV: typed view of plane 0 (depth) of typeless source. For
    // R32G8X24_TYPELESS that's R32_FLOAT_X8X24_TYPELESS.
    auto srvCpu = m_srvHeap->GetCPUDescriptorHandleForHeapStart();
    D3D12_SHADER_RESOURCE_VIEW_DESC srv{};
    const DXGI_FORMAT srcFmt = src->GetDesc().Format;
    if (srcFmt == DXGI_FORMAT_R32G8X24_TYPELESS) {
        srv.Format = DXGI_FORMAT_R32_FLOAT_X8X24_TYPELESS;
    } else if (srcFmt == DXGI_FORMAT_R24G8_TYPELESS) {
        srv.Format = DXGI_FORMAT_R24_UNORM_X8_TYPELESS;
    } else {
        // R32_TYPELESS, D32_FLOAT, R32_FLOAT — already 32bpp, sample directly.
        srv.Format = DXGI_FORMAT_R32_FLOAT;
    }
    srv.ViewDimension = D3D12_SRV_DIMENSION_TEXTURE2D;
    srv.Shader4ComponentMapping = D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;
    srv.Texture2D.MipLevels = 1;
    m_device->CreateShaderResourceView(src, &srv, srvCpu);

    // DSV on output.
    auto dsvCpu = m_dsvHeap->GetCPUDescriptorHandleForHeapStart();
    D3D12_DEPTH_STENCIL_VIEW_DESC dsv{};
    dsv.Format = m_outputFormat;
    dsv.ViewDimension = D3D12_DSV_DIMENSION_TEXTURE2D;
    m_device->CreateDepthStencilView(dst, &dsv, dsvCpu);

    D3D12_VIEWPORT vp{};
    vp.Width = static_cast<float>(m_width);
    vp.Height = static_cast<float>(m_height);
    vp.MaxDepth = 1.0f;
    D3D12_RECT scissor{0, 0, static_cast<LONG>(m_width), static_cast<LONG>(m_height)};
    cmdList->RSSetViewports(1, &vp);
    cmdList->RSSetScissorRects(1, &scissor);
    cmdList->OMSetRenderTargets(0, nullptr, FALSE, &dsvCpu);

    cmdList->SetGraphicsRootSignature(m_rootSig.Get());
    cmdList->SetPipelineState(m_pso.Get());
    ID3D12DescriptorHeap* heaps[] = { m_srvHeap.Get() };
    cmdList->SetDescriptorHeaps(1, heaps);
    cmdList->SetGraphicsRootDescriptorTable(0, m_srvHeap->GetGPUDescriptorHandleForHeapStart());
    cmdList->IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    cmdList->DrawInstanced(3, 1, 0, 0);
    return true;
}

bool DepthResolve::RecordResolveColor(ID3D12GraphicsCommandList* cmdList,
                                      ID3D12Resource* src,
                                      ID3D12Resource* dst) {
    std::lock_guard<std::mutex> lock(m_mutex);
    if (!cmdList || !src || !dst || !m_pso) return false;
    if (!EnsureColorPipeline()) return false;

    // SRV: typed view of plane 0 (depth) of the typeless source (same as the
    // depth-output path).
    auto srvCpu = m_srvHeapColor->GetCPUDescriptorHandleForHeapStart();
    D3D12_SHADER_RESOURCE_VIEW_DESC srv{};
    const DXGI_FORMAT srcFmt = src->GetDesc().Format;
    if (srcFmt == DXGI_FORMAT_R32G8X24_TYPELESS) {
        srv.Format = DXGI_FORMAT_R32_FLOAT_X8X24_TYPELESS;
    } else if (srcFmt == DXGI_FORMAT_R24G8_TYPELESS) {
        srv.Format = DXGI_FORMAT_R24_UNORM_X8_TYPELESS;
    } else {
        srv.Format = DXGI_FORMAT_R32_FLOAT;
    }
    srv.ViewDimension = D3D12_SRV_DIMENSION_TEXTURE2D;
    srv.Shader4ComponentMapping = D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;
    srv.Texture2D.MipLevels = 1;
    m_device->CreateShaderResourceView(src, &srv, srvCpu);

    auto rtvCpu = m_rtvHeap->GetCPUDescriptorHandleForHeapStart();
    D3D12_RENDER_TARGET_VIEW_DESC rtv{};
    rtv.Format = DXGI_FORMAT_R32_FLOAT;
    rtv.ViewDimension = D3D12_RTV_DIMENSION_TEXTURE2D;
    m_device->CreateRenderTargetView(dst, &rtv, rtvCpu);

    D3D12_VIEWPORT vp{};
    vp.Width = static_cast<float>(m_width);
    vp.Height = static_cast<float>(m_height);
    vp.MaxDepth = 1.0f;
    D3D12_RECT scissor{0, 0, static_cast<LONG>(m_width), static_cast<LONG>(m_height)};
    cmdList->RSSetViewports(1, &vp);
    cmdList->RSSetScissorRects(1, &scissor);
    cmdList->OMSetRenderTargets(1, &rtvCpu, FALSE, nullptr);

    cmdList->SetGraphicsRootSignature(m_rootSig.Get());
    cmdList->SetPipelineState(m_psoColor.Get());
    ID3D12DescriptorHeap* heaps[] = { m_srvHeapColor.Get() };
    cmdList->SetDescriptorHeaps(1, heaps);
    cmdList->SetGraphicsRootDescriptorTable(0, m_srvHeapColor->GetGPUDescriptorHandleForHeapStart());
    cmdList->IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    cmdList->DrawInstanced(3, 1, 0, 0);
    return true;
}
