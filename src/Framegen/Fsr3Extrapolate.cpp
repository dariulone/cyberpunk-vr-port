// The extrapolation pass -- see Framegen/Fsr3Extrapolate.hpp. Built like Framegen/Fsr3Dilate.cpp: compiled at
// first use with D3DCompile, one root signature, its own shader-visible descriptor ring.
#include "Framegen/Fsr3Extrapolate.hpp"

#include <windows.h>
#include <d3d12.h>
#include <d3dcompiler.h>

#include <cstdint>
#include <cstring>

extern void Log(const char* fmt, ...);

namespace {

const char* kWarpHLSL = R"HLSL(
cbuffer K : register(b0) {
    uint2 DisplaySize;
    uint2 RenderSize;
    float Factor;       // how far ahead of the capture, in game frames
};
Texture2D<float4> Color   : register(t0);   // the capture, display size
Texture2D<float2> Motion  : register(t1);   // dilated, render size, UV offset to the previous frame
RWTexture2D<float4> Output : register(u0);
SamplerState Linear : register(s0);

[numthreads(8,8,1)]
void Warp(uint3 id : SV_DispatchThreadID) {
    if (id.x >= DisplaySize.x || id.y >= DisplaySize.y) return;
    const float2 q = (float2(id.xy) + 0.5) / float2(DisplaySize);
    // The source pixel s that lands on q: s - Factor * mv(s) = q. Point-sampled vectors, so an object's edge
    // keeps one motion instead of a blend of its own and the background's.
    float2 s = q;
    [unroll]
    for (int i = 0; i < 3; ++i) {
        const int2 m = clamp(int2(s * float2(RenderSize)), int2(0, 0), int2(RenderSize) - 1);
        s = q + Factor * Motion.Load(int3(m, 0));
    }
    Output[id.xy] = Color.SampleLevel(Linear, saturate(s), 0);
}
)HLSL";

ID3D12RootSignature* g_rootSig = nullptr;
ID3D12PipelineState* g_pso = nullptr;
ID3D12DescriptorHeap* g_heap = nullptr;
UINT g_descSize = 0;
UINT g_ring = 0;
constexpr UINT kDescsPerDispatch = 3;   // t0 color, t1 motion, u0 output
constexpr UINT kRingSlots = 64;
bool g_ready = false;

}  // namespace

bool Fsr3Extrapolate_Init(ID3D12Device* device) {
    if (g_ready) return true;
    if (!device) return false;

    D3D12_DESCRIPTOR_RANGE ranges[2] = {};
    ranges[0].RangeType = D3D12_DESCRIPTOR_RANGE_TYPE_SRV;
    ranges[0].NumDescriptors = 2;
    ranges[0].OffsetInDescriptorsFromTableStart = 0;
    ranges[1].RangeType = D3D12_DESCRIPTOR_RANGE_TYPE_UAV;
    ranges[1].NumDescriptors = 1;
    ranges[1].OffsetInDescriptorsFromTableStart = 2;
    D3D12_ROOT_PARAMETER params[2] = {};
    params[0].ParameterType = D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS;
    params[0].Constants.ShaderRegister = 0;
    params[0].Constants.Num32BitValues = 5;   // uint2 + uint2 + float
    params[0].ShaderVisibility = D3D12_SHADER_VISIBILITY_ALL;
    params[1].ParameterType = D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;
    params[1].DescriptorTable.NumDescriptorRanges = 2;
    params[1].DescriptorTable.pDescriptorRanges = ranges;
    params[1].ShaderVisibility = D3D12_SHADER_VISIBILITY_ALL;
    D3D12_STATIC_SAMPLER_DESC sampler{};
    sampler.Filter = D3D12_FILTER_MIN_MAG_MIP_LINEAR;
    sampler.AddressU = sampler.AddressV = sampler.AddressW = D3D12_TEXTURE_ADDRESS_MODE_CLAMP;
    sampler.MaxLOD = D3D12_FLOAT32_MAX;
    sampler.ShaderRegister = 0;
    sampler.ShaderVisibility = D3D12_SHADER_VISIBILITY_ALL;
    D3D12_ROOT_SIGNATURE_DESC rs{};
    rs.NumParameters = 2;
    rs.pParameters = params;
    rs.NumStaticSamplers = 1;
    rs.pStaticSamplers = &sampler;
    ID3DBlob* rsBlob = nullptr;
    ID3DBlob* rsErr = nullptr;
    if (FAILED(D3D12SerializeRootSignature(&rs, D3D_ROOT_SIGNATURE_VERSION_1, &rsBlob, &rsErr))) {
        Log("[fsr3-extrapolate] root signature serialize failed %s\n",
            rsErr ? static_cast<const char*>(rsErr->GetBufferPointer()) : "");
        if (rsErr) rsErr->Release();
        return false;
    }
    const HRESULT rsHr = device->CreateRootSignature(0, rsBlob->GetBufferPointer(), rsBlob->GetBufferSize(),
                                                     IID_PPV_ARGS(&g_rootSig));
    rsBlob->Release();
    if (FAILED(rsHr)) {
        Log("[fsr3-extrapolate] CreateRootSignature 0x%08X\n", static_cast<unsigned>(rsHr));
        return false;
    }

    ID3DBlob* cs = nullptr;
    ID3DBlob* err = nullptr;
    const HRESULT compileHr = D3DCompile(kWarpHLSL, std::strlen(kWarpHLSL), nullptr, nullptr, nullptr, "Warp",
                                         "cs_5_0", 0, 0, &cs, &err);
    if (FAILED(compileHr)) {
        Log("[fsr3-extrapolate] compile failed 0x%08X %.*s\n", static_cast<unsigned>(compileHr),
            err ? static_cast<int>(err->GetBufferSize()) : 0, err ? static_cast<const char*>(err->GetBufferPointer()) : "");
        if (err) err->Release();
        return false;
    }
    D3D12_COMPUTE_PIPELINE_STATE_DESC pd{};
    pd.pRootSignature = g_rootSig;
    pd.CS = {cs->GetBufferPointer(), cs->GetBufferSize()};
    const HRESULT psoHr = device->CreateComputePipelineState(&pd, IID_PPV_ARGS(&g_pso));
    cs->Release();
    if (FAILED(psoHr)) {
        Log("[fsr3-extrapolate] PSO create failed 0x%08X\n", static_cast<unsigned>(psoHr));
        return false;
    }

    D3D12_DESCRIPTOR_HEAP_DESC hd{};
    hd.Type = D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV;
    hd.NumDescriptors = kRingSlots * kDescsPerDispatch;
    hd.Flags = D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE;
    if (FAILED(device->CreateDescriptorHeap(&hd, IID_PPV_ARGS(&g_heap)))) {
        Log("[fsr3-extrapolate] descriptor heap create failed\n");
        return false;
    }
    g_descSize = device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
    g_ready = true;
    Log("[fsr3-extrapolate] ready (motion-vector gather, three fixed-point steps)\n");
    return true;
}

bool Fsr3Extrapolate_Run(ID3D12GraphicsCommandList* list, ID3D12Resource* color, ID3D12Resource* motion,
                         ID3D12Resource* output, int displayWidth, int displayHeight, int renderWidth,
                         int renderHeight, float factor) {
    if (!g_ready || !list || !color || !motion || !output || displayWidth <= 0 || displayHeight <= 0 ||
        renderWidth <= 0 || renderHeight <= 0)
        return false;
    ID3D12Device* dev = nullptr;
    if (FAILED(list->GetDevice(IID_PPV_ARGS(&dev))) || !dev) return false;

    const UINT slot = g_ring++ % kRingSlots;
    D3D12_CPU_DESCRIPTOR_HANDLE cpu = g_heap->GetCPUDescriptorHandleForHeapStart();
    D3D12_GPU_DESCRIPTOR_HANDLE gpu = g_heap->GetGPUDescriptorHandleForHeapStart();
    cpu.ptr += static_cast<SIZE_T>(slot) * kDescsPerDispatch * g_descSize;
    gpu.ptr += static_cast<UINT64>(slot) * kDescsPerDispatch * g_descSize;

    D3D12_SHADER_RESOURCE_VIEW_DESC srv{};
    srv.Shader4ComponentMapping = D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;
    srv.ViewDimension = D3D12_SRV_DIMENSION_TEXTURE2D;
    srv.Texture2D.MipLevels = 1;
    srv.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    D3D12_CPU_DESCRIPTOR_HANDLE h = cpu;
    dev->CreateShaderResourceView(color, &srv, h);
    h.ptr += g_descSize;
    srv.Format = DXGI_FORMAT_R16G16_FLOAT;
    dev->CreateShaderResourceView(motion, &srv, h);
    h.ptr += g_descSize;
    D3D12_UNORDERED_ACCESS_VIEW_DESC uav{};
    uav.ViewDimension = D3D12_UAV_DIMENSION_TEXTURE2D;
    uav.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    dev->CreateUnorderedAccessView(output, nullptr, &uav, h);
    dev->Release();

    struct { uint32_t dw, dh, rw, rh; float factor; } k{static_cast<uint32_t>(displayWidth),
        static_cast<uint32_t>(displayHeight), static_cast<uint32_t>(renderWidth), static_cast<uint32_t>(renderHeight),
        factor};
    ID3D12DescriptorHeap* heaps[] = {g_heap};
    list->SetDescriptorHeaps(1, heaps);
    list->SetComputeRootSignature(g_rootSig);
    list->SetComputeRoot32BitConstants(0, 5, &k, 0);
    list->SetComputeRootDescriptorTable(1, gpu);
    list->SetPipelineState(g_pso);
    list->Dispatch((static_cast<UINT>(displayWidth) + 7) / 8, (static_cast<UINT>(displayHeight) + 7) / 8, 1);
    return true;
}

void Fsr3Extrapolate_Shutdown() {
    if(g_heap)g_heap->Release();
    if(g_pso)g_pso->Release();
    if(g_rootSig)g_rootSig->Release();
    g_heap=nullptr;g_pso=nullptr;g_rootSig=nullptr;g_ring=0;g_ready=false;
}
