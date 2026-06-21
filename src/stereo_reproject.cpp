#include "stereo_reproject.h"

#include <cstdio>
#include <cmath>
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

// Depth-based stereo reprojection PS.
//
// Given a source eye color + depth, synthesize the OTHER eye by computing
// per-pixel horizontal disparity:
//
//   For a 3D point at world distance Z from camera with inter-pupillary
//   distance IPD, the horizontal pixel offset between left/right images is
//   d_px = focal_px * IPD / Z   where focal_px = (W/2) / tan(fov_h/2).
//   In UV units (divide by W) that's d_uv = (IPD / (2 * tan(fov_h/2))) / Z.
//
//   So: srcUv.x = dstUv.x + signLR * d_uv
//
// CP2077 uses reversed-Z infinite-far projection, so depth_sample d ∈ [0,1]
// maps to Z ≈ nearZ / d (d=1 → near, d=0 → far). We clamp d away from 0 to
// avoid infinite disparity on sky pixels.
constexpr char kPsSource[] = R"(
cbuffer ReprojectCB : register(b0) {
    float ipdOverFocal; // = IPD / (2 * tan(fov_h/2))  (precomputed host-side)
    float nearZ;        // projection near plane in meters
    float signLR;       // +1 = synthesize right from left, -1 opposite
    float padding;
}

Texture2D<float4> g_color : register(t0);
Texture2D<float>  g_depth : register(t1);
SamplerState g_linear : register(s0);

struct VSOut {
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

float4 PSMain(VSOut input) : SV_Target {
    float d = g_depth.SampleLevel(g_linear, input.uv, 0.0);
    // Clamp away from far (sky pixels) so disparity stays finite.
    float dClamped = max(d, 1.0e-4);
    float zWorld = nearZ / dClamped;        // reversed-Z infinite-far approximation
    float dispUv = (ipdOverFocal * signLR) / zWorld;
    float2 srcUv = input.uv + float2(dispUv, 0.0);
    // If the sample point would fall outside the source eye, clamp — at this
    // sign it shows the closest in-frame color (best simple fill). A future
    // refinement could use previous-frame actual data of the synthesized eye
    // to fill disocclusions, but clamp keeps the picture coherent.
    srcUv = saturate(srcUv);
    return g_color.SampleLevel(g_linear, srcUv, 0.0);
}
)";

bool CompileShader(const char* src, const char* entry, const char* target, ComPtr<ID3DBlob>& out) {
    UINT flags = D3DCOMPILE_ENABLE_STRICTNESS;
    ComPtr<ID3DBlob> errors;
    HRESULT hr = D3DCompile(src, strlen(src), nullptr, nullptr, nullptr,
        entry, target, flags, 0, &out, &errors);
    if (FAILED(hr)) {
        Log("StereoReproject: shader compile failed %s/%s hr=0x%08X %.*s\n",
            entry, target, static_cast<unsigned>(hr),
            errors ? static_cast<int>(errors->GetBufferSize()) : 0,
            errors ? static_cast<const char*>(errors->GetBufferPointer()) : "");
        return false;
    }
    return true;
}

}  // namespace

StereoReproject::~StereoReproject() { Shutdown(); }

void StereoReproject::Shutdown() {
    std::lock_guard<std::mutex> lock(m_mutex);
    m_pso.Reset();
    m_rootSig.Reset();
    m_srvHeap.Reset();
    m_rtvHeap.Reset();
    m_cb.Reset();
    m_device.Reset();
    m_colorFormat = DXGI_FORMAT_UNKNOWN;
    m_width = m_height = 0;
}

bool StereoReproject::EnsureInitialized(ID3D12Device* device,
                                        DXGI_FORMAT colorFormat,
                                        uint32_t width,
                                        uint32_t height) {
    std::lock_guard<std::mutex> lock(m_mutex);
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
    m_cb.Reset();

    m_device = device;
    m_colorFormat = colorFormat;
    m_width = width;
    m_height = height;
    m_srvStride = device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);

    D3D12_DESCRIPTOR_HEAP_DESC srvHeapDesc{};
    srvHeapDesc.NumDescriptors = 2;
    srvHeapDesc.Type = D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV;
    srvHeapDesc.Flags = D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE;
    if (FAILED(device->CreateDescriptorHeap(&srvHeapDesc, IID_PPV_ARGS(&m_srvHeap)))) {
        Log("StereoReproject: failed to create SRV heap\n");
        return false;
    }
    m_srvHeap->SetName(L"StereoReproject_SRV_heap");

    D3D12_DESCRIPTOR_HEAP_DESC rtvHeapDesc{};
    rtvHeapDesc.NumDescriptors = 1;
    rtvHeapDesc.Type = D3D12_DESCRIPTOR_HEAP_TYPE_RTV;
    if (FAILED(device->CreateDescriptorHeap(&rtvHeapDesc, IID_PPV_ARGS(&m_rtvHeap)))) {
        Log("StereoReproject: failed to create RTV heap\n");
        return false;
    }
    m_rtvHeap->SetName(L"StereoReproject_RTV_heap");

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
        Log("StereoReproject: failed to create CB\n");
        return false;
    }
    m_cb->SetName(L"StereoReproject_CB");

    D3D12_DESCRIPTOR_RANGE srvRange{};
    srvRange.RangeType = D3D12_DESCRIPTOR_RANGE_TYPE_SRV;
    srvRange.NumDescriptors = 2;
    srvRange.BaseShaderRegister = 0;

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
        Log("StereoReproject: root sig serialize failed %.*s\n",
            rsErrors ? static_cast<int>(rsErrors->GetBufferSize()) : 0,
            rsErrors ? static_cast<const char*>(rsErrors->GetBufferPointer()) : "");
        return false;
    }
    if (FAILED(device->CreateRootSignature(0, rsBlob->GetBufferPointer(),
            rsBlob->GetBufferSize(), IID_PPV_ARGS(&m_rootSig)))) {
        Log("StereoReproject: failed to create root sig\n");
        return false;
    }
    m_rootSig->SetName(L"StereoReproject_RootSig");

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
        Log("StereoReproject: PSO create failed\n");
        return false;
    }
    m_pso->SetName(L"StereoReproject_PSO");
    Log("StereoReproject: initialized %ux%u colorFmt=%u\n",
        width, height, static_cast<unsigned>(colorFormat));
    return true;
}

bool StereoReproject::RecordReproject(ID3D12GraphicsCommandList* cmdList,
                                     ID3D12Resource* srcColor,
                                     ID3D12Resource* srcDepth,
                                     ID3D12Resource* dstColor,
                                     float ipdMeters,
                                     float horizFovRad,
                                     float nearZ,
                                     float signLR) {
    std::lock_guard<std::mutex> lock(m_mutex);
    if (!cmdList || !srcColor || !srcDepth || !dstColor || !m_pso) return false;

    // Host-side precompute the disparity coefficient: ipd / (2 * tan(fov/2)).
    const float ipdOverFocal = ipdMeters / (2.0f * std::tan(0.5f * horizFovRad));

    struct CB {
        float ipdOverFocal;
        float nearZ;
        float signLR;
        float pad;
    } cb { ipdOverFocal, nearZ, signLR, 0.0f };
    void* mapped = nullptr;
    if (FAILED(m_cb->Map(0, nullptr, &mapped))) {
        Log("StereoReproject: failed to map CB\n");
        return false;
    }
    memcpy(mapped, &cb, sizeof(cb));
    m_cb->Unmap(0, nullptr);

    auto srvCpu = m_srvHeap->GetCPUDescriptorHandleForHeapStart();
    D3D12_SHADER_RESOURCE_VIEW_DESC srv0{};
    srv0.Format = srcColor->GetDesc().Format;
    srv0.ViewDimension = D3D12_SRV_DIMENSION_TEXTURE2D;
    srv0.Shader4ComponentMapping = D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;
    srv0.Texture2D.MipLevels = 1;
    m_device->CreateShaderResourceView(srcColor, &srv0, srvCpu);
    srvCpu.ptr += m_srvStride;

    // Depth source is D32_FLOAT (typed) or R32_TYPELESS (we view as R32_FLOAT).
    DXGI_FORMAT depthFmt = srcDepth->GetDesc().Format;
    D3D12_SHADER_RESOURCE_VIEW_DESC srv1{};
    if (depthFmt == DXGI_FORMAT_D32_FLOAT) {
        srv1.Format = DXGI_FORMAT_R32_FLOAT;
    } else if (depthFmt == DXGI_FORMAT_R32_TYPELESS) {
        srv1.Format = DXGI_FORMAT_R32_FLOAT;
    } else {
        srv1.Format = depthFmt;
    }
    srv1.ViewDimension = D3D12_SRV_DIMENSION_TEXTURE2D;
    srv1.Shader4ComponentMapping = D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;
    srv1.Texture2D.MipLevels = 1;
    m_device->CreateShaderResourceView(srcDepth, &srv1, srvCpu);

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
    cmdList->SetGraphicsRootConstantBufferView(0, m_cb->GetGPUVirtualAddress());
    cmdList->SetGraphicsRootDescriptorTable(1, m_srvHeap->GetGPUDescriptorHandleForHeapStart());
    cmdList->IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    cmdList->DrawInstanced(3, 1, 0, 0);
    return true;
}
