// Phase 2b: motion-vector forward warp implementation.
#include "mv_warp.h"

#include <cstdio>
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
        float2(-1.0,  1.0),
        float2( 1.0,  1.0),
        float2(-1.0, -1.0),
        float2( 1.0, -1.0)
    };
    static const float2 uvs[4] = {
        float2(0.0, 0.0),
        float2(1.0, 0.0),
        float2(0.0, 1.0),
        float2(1.0, 1.0)
    };
    VSOut o;
    o.position = float4(positions[vid], 0.0, 1.0);
    o.uv = uvs[vid];
    return o;
}
)";

// Forward warp via engine motion vectors.
//
// The engine's motion-vector texture encodes per-pixel velocity in NDC space:
// `mv = posPrev - posCur` (or the reverse, sign convention varies). For
// "advance an old image forward by one frame": output(p) = prev(p - mv*scale)
// — i.e. we look back along the motion vector from the destination pixel to
// find which source pixel "moves to here this frame."
//
// We pass `mvScale` from the host so the same shader works with either NDC-
// space or texel-space MV regardless of engine convention. For CP2077's DLSS
// MV (R16G16_FLOAT in NDC) a reasonable starting scale is (1, 1) with sign
// adjusted from telemetry; the host will tune.
constexpr char kPsSource[] = R"(
cbuffer WarpCB : register(b0) {
    float2 mvScale;
    float2 padding;
}
Texture2D<float4> g_prevColor : register(t0);
Texture2D<float2> g_mv        : register(t1);
SamplerState g_linear         : register(s0);

struct VSOut {
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

float4 PSMain(VSOut input) : SV_Target {
    float2 mv = g_mv.SampleLevel(g_linear, input.uv, 0.0);
    float2 src = input.uv - mv * mvScale;
    src = saturate(src);
    return g_prevColor.SampleLevel(g_linear, src, 0.0);
}
)";

bool CompileShader(const char* src, const char* entry, const char* target, ComPtr<ID3DBlob>& out) {
    UINT flags = D3DCOMPILE_ENABLE_STRICTNESS;
    ComPtr<ID3DBlob> errors;
    HRESULT hr = D3DCompile(src, strlen(src), nullptr, nullptr, nullptr,
        entry, target, flags, 0, &out, &errors);
    if (FAILED(hr)) {
        Log("MotionVectorWarp: shader compile failed %s/%s hr=0x%08X %.*s\n",
            entry, target, static_cast<unsigned>(hr),
            errors ? static_cast<int>(errors->GetBufferSize()) : 0,
            errors ? static_cast<const char*>(errors->GetBufferPointer()) : "");
        return false;
    }
    return true;
}

}  // namespace

MotionVectorWarp::~MotionVectorWarp() {
    Shutdown();
}

void MotionVectorWarp::Shutdown() {
    std::lock_guard<std::mutex> lock(m_mutex);
    m_pso.Reset();
    m_rootSig.Reset();
    m_srvHeap.Reset();
    m_rtvHeap.Reset();
    m_cb.Reset();
    m_device.Reset();
    m_outputFormat = DXGI_FORMAT_UNKNOWN;
    m_outputWidth = m_outputHeight = 0;
}

bool MotionVectorWarp::EnsureInitialized(ID3D12Device* device,
                                        DXGI_FORMAT outputFormat,
                                        uint32_t outputWidth,
                                        uint32_t outputHeight) {
    std::lock_guard<std::mutex> lock(m_mutex);
    if (!device || !outputWidth || !outputHeight || outputFormat == DXGI_FORMAT_UNKNOWN) {
        return false;
    }
    if (m_device.Get() == device &&
        m_outputFormat == outputFormat &&
        m_outputWidth == outputWidth &&
        m_outputHeight == outputHeight &&
        m_pso) {
        return true;
    }

    // Reset any stale state from a prior (device, fmt, size) config.
    m_pso.Reset();
    m_rootSig.Reset();
    m_srvHeap.Reset();
    m_rtvHeap.Reset();
    m_cb.Reset();

    m_device = device;
    m_outputFormat = outputFormat;
    m_outputWidth = outputWidth;
    m_outputHeight = outputHeight;
    m_srvStride = device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);

    // SRV heap: 2 SRVs (prevColor t0, mv t1). Shader-visible.
    D3D12_DESCRIPTOR_HEAP_DESC srvHeapDesc{};
    srvHeapDesc.NumDescriptors = 2;
    srvHeapDesc.Type = D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV;
    srvHeapDesc.Flags = D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE;
    if (FAILED(device->CreateDescriptorHeap(&srvHeapDesc, IID_PPV_ARGS(&m_srvHeap)))) {
        Log("MotionVectorWarp: failed to create SRV heap\n");
        return false;
    }
    m_srvHeap->SetName(L"MVWarp_SRV_heap");

    D3D12_DESCRIPTOR_HEAP_DESC rtvHeapDesc{};
    rtvHeapDesc.NumDescriptors = 1;
    rtvHeapDesc.Type = D3D12_DESCRIPTOR_HEAP_TYPE_RTV;
    if (FAILED(device->CreateDescriptorHeap(&rtvHeapDesc, IID_PPV_ARGS(&m_rtvHeap)))) {
        Log("MotionVectorWarp: failed to create RTV heap\n");
        return false;
    }
    m_rtvHeap->SetName(L"MVWarp_RTV_heap");

    // Constant buffer (just mvScale + padding = 16 bytes, but D3D12 wants 256-aligned).
    D3D12_HEAP_PROPERTIES uploadHeap{};
    uploadHeap.Type = D3D12_HEAP_TYPE_UPLOAD;
    D3D12_RESOURCE_DESC cbDesc{};
    cbDesc.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
    cbDesc.Width = 256;
    cbDesc.Height = 1;
    cbDesc.DepthOrArraySize = 1;
    cbDesc.MipLevels = 1;
    cbDesc.SampleDesc.Count = 1;
    cbDesc.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    if (FAILED(device->CreateCommittedResource(&uploadHeap, D3D12_HEAP_FLAG_NONE,
            &cbDesc, D3D12_RESOURCE_STATE_GENERIC_READ, nullptr, IID_PPV_ARGS(&m_cb)))) {
        Log("MotionVectorWarp: failed to create CB\n");
        return false;
    }
    m_cb->SetName(L"MVWarp_CB");

    // Root signature: root CBV b0 + descriptor table for SRVs t0,t1 + static sampler.
    D3D12_DESCRIPTOR_RANGE srvRange{};
    srvRange.RangeType = D3D12_DESCRIPTOR_RANGE_TYPE_SRV;
    srvRange.NumDescriptors = 2;
    srvRange.BaseShaderRegister = 0;
    srvRange.RegisterSpace = 0;
    srvRange.OffsetInDescriptorsFromTableStart = 0;

    D3D12_ROOT_PARAMETER params[2] = {};
    params[0].ParameterType = D3D12_ROOT_PARAMETER_TYPE_CBV;
    params[0].Descriptor.ShaderRegister = 0;
    params[0].ShaderVisibility = D3D12_SHADER_VISIBILITY_PIXEL;
    params[1].ParameterType = D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;
    params[1].DescriptorTable.NumDescriptorRanges = 1;
    params[1].DescriptorTable.pDescriptorRanges = &srvRange;
    params[1].ShaderVisibility = D3D12_SHADER_VISIBILITY_PIXEL;

    D3D12_STATIC_SAMPLER_DESC sampler{};
    sampler.Filter = D3D12_FILTER_MIN_MAG_MIP_LINEAR;
    sampler.AddressU = D3D12_TEXTURE_ADDRESS_MODE_CLAMP;
    sampler.AddressV = D3D12_TEXTURE_ADDRESS_MODE_CLAMP;
    sampler.AddressW = D3D12_TEXTURE_ADDRESS_MODE_CLAMP;
    sampler.ShaderRegister = 0;
    sampler.RegisterSpace = 0;
    sampler.ShaderVisibility = D3D12_SHADER_VISIBILITY_PIXEL;
    sampler.MaxLOD = D3D12_FLOAT32_MAX;

    D3D12_ROOT_SIGNATURE_DESC rootDesc{};
    rootDesc.NumParameters = 2;
    rootDesc.pParameters = params;
    rootDesc.NumStaticSamplers = 1;
    rootDesc.pStaticSamplers = &sampler;
    rootDesc.Flags = D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT;

    ComPtr<ID3DBlob> rootBlob, rootErrors;
    if (FAILED(D3D12SerializeRootSignature(&rootDesc, D3D_ROOT_SIGNATURE_VERSION_1,
            &rootBlob, &rootErrors))) {
        Log("MotionVectorWarp: root signature serialize failed %.*s\n",
            rootErrors ? static_cast<int>(rootErrors->GetBufferSize()) : 0,
            rootErrors ? static_cast<const char*>(rootErrors->GetBufferPointer()) : "");
        return false;
    }
    if (FAILED(device->CreateRootSignature(0, rootBlob->GetBufferPointer(),
            rootBlob->GetBufferSize(), IID_PPV_ARGS(&m_rootSig)))) {
        Log("MotionVectorWarp: failed to create root sig\n");
        return false;
    }
    m_rootSig->SetName(L"MVWarp_RootSig");

    ComPtr<ID3DBlob> vsBlob, psBlob;
    if (!CompileShader(kVsSource, "VSMain", "vs_5_0", vsBlob) ||
        !CompileShader(kPsSource, "PSMain", "ps_5_0", psBlob)) {
        return false;
    }

    D3D12_GRAPHICS_PIPELINE_STATE_DESC psoDesc{};
    psoDesc.pRootSignature = m_rootSig.Get();
    psoDesc.VS = { vsBlob->GetBufferPointer(), vsBlob->GetBufferSize() };
    psoDesc.PS = { psBlob->GetBufferPointer(), psBlob->GetBufferSize() };
    psoDesc.PrimitiveTopologyType = D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE;
    psoDesc.NumRenderTargets = 1;
    psoDesc.RTVFormats[0] = outputFormat;
    psoDesc.SampleDesc.Count = 1;
    psoDesc.SampleMask = UINT_MAX;
    psoDesc.RasterizerState.FillMode = D3D12_FILL_MODE_SOLID;
    psoDesc.RasterizerState.CullMode = D3D12_CULL_MODE_NONE;
    psoDesc.BlendState.RenderTarget[0].RenderTargetWriteMask = D3D12_COLOR_WRITE_ENABLE_ALL;
    psoDesc.DepthStencilState.DepthEnable = FALSE;
    psoDesc.DepthStencilState.StencilEnable = FALSE;
    if (FAILED(device->CreateGraphicsPipelineState(&psoDesc, IID_PPV_ARGS(&m_pso)))) {
        Log("MotionVectorWarp: PSO create failed\n");
        return false;
    }
    m_pso->SetName(L"MVWarp_PSO");

    Log("MotionVectorWarp: initialized device=%p out=%ux%u fmt=%u\n",
        device, outputWidth, outputHeight, static_cast<unsigned>(outputFormat));
    return true;
}

bool MotionVectorWarp::RecordWarp(ID3D12GraphicsCommandList* cmdList,
                                  ID3D12Resource* prevColor,
                                  ID3D12Resource* mvTexture,
                                  ID3D12Resource* dstColor,
                                  float mvScaleX, float mvScaleY) {
    std::lock_guard<std::mutex> lock(m_mutex);
    if (!cmdList || !prevColor || !mvTexture || !dstColor || !m_pso) return false;

    // Update CB.
    struct CB { float scale[2]; float pad[2]; } cb { { mvScaleX, mvScaleY }, { 0, 0 } };
    void* mapped = nullptr;
    if (FAILED(m_cb->Map(0, nullptr, &mapped))) {
        Log("MotionVectorWarp: failed to map CB\n");
        return false;
    }
    memcpy(mapped, &cb, sizeof(cb));
    m_cb->Unmap(0, nullptr);

    // Bind SRVs.
    auto srvCpu = m_srvHeap->GetCPUDescriptorHandleForHeapStart();
    D3D12_SHADER_RESOURCE_VIEW_DESC srv0{};
    srv0.Format = prevColor->GetDesc().Format;
    srv0.ViewDimension = D3D12_SRV_DIMENSION_TEXTURE2D;
    srv0.Shader4ComponentMapping = D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;
    srv0.Texture2D.MipLevels = 1;
    m_device->CreateShaderResourceView(prevColor, &srv0, srvCpu);
    srvCpu.ptr += m_srvStride;
    D3D12_SHADER_RESOURCE_VIEW_DESC srv1{};
    // Engine MV is R16G16_FLOAT (per Streamline capture); shader declares Texture2D<float2>.
    srv1.Format = mvTexture->GetDesc().Format;
    srv1.ViewDimension = D3D12_SRV_DIMENSION_TEXTURE2D;
    srv1.Shader4ComponentMapping = D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;
    srv1.Texture2D.MipLevels = 1;
    m_device->CreateShaderResourceView(mvTexture, &srv1, srvCpu);

    // Bind RTV.
    auto rtvCpu = m_rtvHeap->GetCPUDescriptorHandleForHeapStart();
    D3D12_RENDER_TARGET_VIEW_DESC rtvDesc{};
    rtvDesc.Format = m_outputFormat;
    rtvDesc.ViewDimension = D3D12_RTV_DIMENSION_TEXTURE2D;
    m_device->CreateRenderTargetView(dstColor, &rtvDesc, rtvCpu);

    D3D12_VIEWPORT vp{};
    vp.Width = static_cast<float>(m_outputWidth);
    vp.Height = static_cast<float>(m_outputHeight);
    vp.MaxDepth = 1.0f;
    D3D12_RECT scissor{0, 0, static_cast<LONG>(m_outputWidth), static_cast<LONG>(m_outputHeight)};
    cmdList->RSSetViewports(1, &vp);
    cmdList->RSSetScissorRects(1, &scissor);
    cmdList->OMSetRenderTargets(1, &rtvCpu, FALSE, nullptr);

    cmdList->SetGraphicsRootSignature(m_rootSig.Get());
    cmdList->SetPipelineState(m_pso.Get());
    ID3D12DescriptorHeap* heaps[] = { m_srvHeap.Get() };
    cmdList->SetDescriptorHeaps(1, heaps);
    cmdList->SetGraphicsRootConstantBufferView(0, m_cb->GetGPUVirtualAddress());
    cmdList->SetGraphicsRootDescriptorTable(1, m_srvHeap->GetGPUDescriptorHandleForHeapStart());
    cmdList->IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
    cmdList->DrawInstanced(4, 1, 0, 0);
    return true;
}
