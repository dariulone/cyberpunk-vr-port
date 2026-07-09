#include "warp_pass.h"

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

// Rotational reprojection (RealVR reproject_frame_rotational equivalent).
// XR conventions: -Z forward, +X right, +Y up. fov tan vectors are
// (tanLeft, tanRight, tanUp, tanDown) with tanLeft/tanDown typically negative.
// uv.x: 0 = left edge, uv.y: 0 = TOP edge (D3D texture space).
constexpr char kPsSource[] = R"(
cbuffer WarpCB : register(b0) {
    float4 rotRow0;   // world-agnostic delta rotation matrix, row 0 (dst ray -> src ray)
    float4 rotRow1;
    float4 rotRow2;
    float4 srcTan;    // content projection:  (tanL, tanR, tanU, tanD)
    float4 dstTan;    // declared output fov: (tanL, tanR, tanU, tanD)
}
Texture2D<float4> g_source : register(t0);
SamplerState g_sampler : register(s0);

struct VSOut {
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

float4 PSMain(VSOut input) : SV_Target {
    // Ray through this output pixel in the DECLARED (live lens) frustum.
    float tx = lerp(dstTan.x, dstTan.y, input.uv.x);
    float ty = lerp(dstTan.z, dstTan.w, input.uv.y);   // uv.y 0 = top -> tanUp
    float3 rayDst = float3(tx, ty, -1.0);

    // Rotate into the RENDER frame.
    float3 raySrc = float3(dot(rotRow0.xyz, rayDst),
                           dot(rotRow1.xyz, rayDst),
                           dot(rotRow2.xyz, rayDst));

    // Behind the source camera -> nothing to show.
    if (raySrc.z >= -1e-5) {
        return float4(0.0, 0.0, 0.0, 1.0);
    }

    // Project through the CONTENT's true frustum.
    float invZ = -1.0 / raySrc.z;
    float sx = raySrc.x * invZ;
    float sy = raySrc.y * invZ;
    float u = (sx - srcTan.x) / (srcTan.y - srcTan.x);
    float v = (srcTan.z - sy) / (srcTan.z - srcTan.w);  // top(v=0) = tanUp

    if (u < 0.0 || u > 1.0 || v < 0.0 || v > 1.0) {
        return float4(0.0, 0.0, 0.0, 1.0);
    }
    return float4(g_source.SampleLevel(g_sampler, float2(u, v), 0.0).rgb, 1.0);
}
)";

bool CompileShader(const char* src, const char* entry, const char* target, ComPtr<ID3DBlob>& out) {
    UINT flags = D3DCOMPILE_ENABLE_STRICTNESS;
    ComPtr<ID3DBlob> errors;
    HRESULT hr = D3DCompile(src, strlen(src), nullptr, nullptr, nullptr,
        entry, target, flags, 0, &out, &errors);
    if (FAILED(hr)) {
        Log("WarpPass: shader compile failed %s/%s hr=0x%08X %.*s\n",
            entry, target, static_cast<unsigned>(hr),
            errors ? static_cast<int>(errors->GetBufferSize()) : 0,
            errors ? static_cast<const char*>(errors->GetBufferPointer()) : "");
        return false;
    }
    return true;
}

struct WarpCB {
    float rotRow0[4];
    float rotRow1[4];
    float rotRow2[4];
    float srcTan[4];
    float dstTan[4];
};

// q -> 3x3 rotation matrix rows (row-vector convention chosen so the shader can
// do row·ray directly for  v' = R * v).
void QuatToRows(const XrQuaternionf& q, WarpCB& cb) {
    const float x = q.x, y = q.y, z = q.z, w = q.w;
    cb.rotRow0[0] = 1.0f - 2.0f * (y * y + z * z);
    cb.rotRow0[1] = 2.0f * (x * y - z * w);
    cb.rotRow0[2] = 2.0f * (x * z + y * w);
    cb.rotRow1[0] = 2.0f * (x * y + z * w);
    cb.rotRow1[1] = 1.0f - 2.0f * (x * x + z * z);
    cb.rotRow1[2] = 2.0f * (y * z - x * w);
    cb.rotRow2[0] = 2.0f * (x * z - y * w);
    cb.rotRow2[1] = 2.0f * (y * z + x * w);
    cb.rotRow2[2] = 1.0f - 2.0f * (x * x + y * y);
}

XrQuaternionf QuatConjugate(const XrQuaternionf& q) {
    return XrQuaternionf{-q.x, -q.y, -q.z, q.w};
}

XrQuaternionf QuatMul(const XrQuaternionf& a, const XrQuaternionf& b) {
    XrQuaternionf r;
    r.w = a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z;
    r.x = a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y;
    r.y = a.w * b.y - a.x * b.z + a.y * b.w + a.z * b.x;
    r.z = a.w * b.z + a.x * b.y - a.y * b.x + a.z * b.w;
    return r;
}

void QuatNormalizeInPlace(XrQuaternionf& q) {
    const float n = sqrtf(q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w);
    if (n > 1e-6f) {
        const float inv = 1.0f / n;
        q.x *= inv; q.y *= inv; q.z *= inv; q.w *= inv;
    } else {
        q = XrQuaternionf{0.0f, 0.0f, 0.0f, 1.0f};
    }
}

}  // namespace

WarpPass::~WarpPass() { Shutdown(); }

void WarpPass::Shutdown() {
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

bool WarpPass::EnsureInitialized(ID3D12Device* device,
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
    m_srvDescSize = device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
    m_rtvDescSize = device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV);

    D3D12_DESCRIPTOR_HEAP_DESC srvHeapDesc{};
    srvHeapDesc.NumDescriptors = 2;   // per-eye slots
    srvHeapDesc.Type = D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV;
    srvHeapDesc.Flags = D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE;
    if (FAILED(device->CreateDescriptorHeap(&srvHeapDesc, IID_PPV_ARGS(&m_srvHeap)))) {
        Log("WarpPass: failed to create SRV heap\n");
        return false;
    }
    m_srvHeap->SetName(L"WarpPass_SRV_heap");

    D3D12_DESCRIPTOR_HEAP_DESC rtvHeapDesc{};
    rtvHeapDesc.NumDescriptors = 2;   // per-eye slots
    rtvHeapDesc.Type = D3D12_DESCRIPTOR_HEAP_TYPE_RTV;
    if (FAILED(device->CreateDescriptorHeap(&rtvHeapDesc, IID_PPV_ARGS(&m_rtvHeap)))) {
        Log("WarpPass: failed to create RTV heap\n");
        return false;
    }
    m_rtvHeap->SetName(L"WarpPass_RTV_heap");

    D3D12_HEAP_PROPERTIES uploadHeap{};
    uploadHeap.Type = D3D12_HEAP_TYPE_UPLOAD;
    D3D12_RESOURCE_DESC cbDesc{};
    cbDesc.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
    cbDesc.Width = 512;   // 2 x 256-byte eye slots
    cbDesc.Height = 1;
    cbDesc.DepthOrArraySize = 1;
    cbDesc.MipLevels = 1;
    cbDesc.SampleDesc.Count = 1;
    cbDesc.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    if (FAILED(device->CreateCommittedResource(&uploadHeap, D3D12_HEAP_FLAG_NONE,
            &cbDesc, D3D12_RESOURCE_STATE_GENERIC_READ, nullptr, IID_PPV_ARGS(&m_cb)))) {
        Log("WarpPass: failed to create CB\n");
        return false;
    }
    m_cb->SetName(L"WarpPass_CB");

    D3D12_DESCRIPTOR_RANGE srvRange{};
    srvRange.RangeType = D3D12_DESCRIPTOR_RANGE_TYPE_SRV;
    srvRange.NumDescriptors = 1;
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
    sampler.AddressU = sampler.AddressV = sampler.AddressW = D3D12_TEXTURE_ADDRESS_MODE_BORDER;
    sampler.BorderColor = D3D12_STATIC_BORDER_COLOR_OPAQUE_BLACK;
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
        Log("WarpPass: root sig serialize failed %.*s\n",
            rsErrors ? static_cast<int>(rsErrors->GetBufferSize()) : 0,
            rsErrors ? static_cast<const char*>(rsErrors->GetBufferPointer()) : "");
        return false;
    }
    if (FAILED(device->CreateRootSignature(0, rsBlob->GetBufferPointer(),
            rsBlob->GetBufferSize(), IID_PPV_ARGS(&m_rootSig)))) {
        Log("WarpPass: failed to create root sig\n");
        return false;
    }
    m_rootSig->SetName(L"WarpPass_RootSig");

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
        Log("WarpPass: PSO create failed\n");
        return false;
    }
    m_pso->SetName(L"WarpPass_PSO");
    Log("WarpPass: initialized %ux%u colorFmt=%u\n",
        width, height, static_cast<unsigned>(colorFormat));
    return true;
}

bool WarpPass::RecordWarp(ID3D12GraphicsCommandList* cmdList,
                          ID3D12Resource* srcColor,
                          ID3D12Resource* dstColor,
                          int eyeIndex,
                          const XrQuaternionf& renderOrientation,
                          const XrQuaternionf& liveOrientation,
                          const XrFovf& srcFov,
                          const XrFovf& dstFov) {
    std::lock_guard<std::mutex> lock(m_mutex);
    if (!cmdList || !srcColor || !dstColor || !m_pso) return false;
    if (eyeIndex < 0 || eyeIndex > 1) return false;

    // dst-ray -> src-ray rotation: p_src = conj(q_render) * q_live * p_dst.
    XrQuaternionf qRel = QuatMul(QuatConjugate(renderOrientation), liveOrientation);
    QuatNormalizeInPlace(qRel);

    WarpCB cb{};
    QuatToRows(qRel, cb);
    cb.srcTan[0] = tanf(srcFov.angleLeft);
    cb.srcTan[1] = tanf(srcFov.angleRight);
    cb.srcTan[2] = tanf(srcFov.angleUp);
    cb.srcTan[3] = tanf(srcFov.angleDown);
    cb.dstTan[0] = tanf(dstFov.angleLeft);
    cb.dstTan[1] = tanf(dstFov.angleRight);
    cb.dstTan[2] = tanf(dstFov.angleUp);
    cb.dstTan[3] = tanf(dstFov.angleDown);

    // Per-eye CB slot (256-byte aligned) so two eye draws recorded into the same
    // command list keep distinct constants (CBVs are read at EXECUTION time).
    const UINT cbOffset = static_cast<UINT>(eyeIndex) * 256u;
    uint8_t* mapped = nullptr;
    D3D12_RANGE readRange{0, 0};
    if (FAILED(m_cb->Map(0, &readRange, reinterpret_cast<void**>(&mapped)))) {
        Log("WarpPass: failed to map CB\n");
        return false;
    }
    memcpy(mapped + cbOffset, &cb, sizeof(cb));
    m_cb->Unmap(0, nullptr);

    D3D12_CPU_DESCRIPTOR_HANDLE srvCpu = m_srvHeap->GetCPUDescriptorHandleForHeapStart();
    srvCpu.ptr += static_cast<SIZE_T>(eyeIndex) * m_srvDescSize;
    D3D12_SHADER_RESOURCE_VIEW_DESC srv0{};
    srv0.Format = srcColor->GetDesc().Format;
    srv0.ViewDimension = D3D12_SRV_DIMENSION_TEXTURE2D;
    srv0.Shader4ComponentMapping = D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;
    srv0.Texture2D.MipLevels = 1;
    m_device->CreateShaderResourceView(srcColor, &srv0, srvCpu);

    D3D12_CPU_DESCRIPTOR_HANDLE rtvCpu = m_rtvHeap->GetCPUDescriptorHandleForHeapStart();
    rtvCpu.ptr += static_cast<SIZE_T>(eyeIndex) * m_rtvDescSize;
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
    cmdList->SetGraphicsRootConstantBufferView(0, m_cb->GetGPUVirtualAddress() + cbOffset);
    D3D12_GPU_DESCRIPTOR_HANDLE srvGpu = m_srvHeap->GetGPUDescriptorHandleForHeapStart();
    srvGpu.ptr += static_cast<UINT64>(eyeIndex) * m_srvDescSize;
    cmdList->SetGraphicsRootDescriptorTable(1, srvGpu);
    cmdList->IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    cmdList->DrawInstanced(3, 1, 0, 0);
    return true;
}
