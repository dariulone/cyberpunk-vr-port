#include "optical_flow_d3d12.h"

#include <cstdarg>
#include <cstring>
#include <cwchar>
#include <d3dcompiler.h>
#include <unordered_map>
#include <vector>
#include <wrl.h>

#include "NvOFD3D12.h"
#include "NvOFD3DCommon.h"

extern void Log(const char* fmt, ...);

using Microsoft::WRL::ComPtr;

namespace {

constexpr float kAERV2FrameGenBlendFactor = 0.5f;

void SetD3DName(ID3D12Object* object, const wchar_t* name) {
    if (object && name) {
        object->SetName(name);
    }
}

void SetD3DNamef(ID3D12Object* object, const wchar_t* format, ...) {
    if (!object || !format) {
        return;
    }
    wchar_t name[160]{};
    va_list args;
    va_start(args, format);
    _vsnwprintf_s(name, sizeof(name) / sizeof(name[0]), _TRUNCATE, format, args);
    va_end(args);
    object->SetName(name);
}

bool WaitForQueueIdleLocal(ID3D12CommandQueue* queue, ID3D12Fence* fence, HANDLE fenceEvent, UINT64& fenceValue) {
    if (!queue || !fence || !fenceEvent) {
        return false;
    }
    ++fenceValue;
    if (FAILED(queue->Signal(fence, fenceValue))) {
        return false;
    }
    if (fence->GetCompletedValue() < fenceValue) {
        if (FAILED(fence->SetEventOnCompletion(fenceValue, fenceEvent))) {
            return false;
        }
        WaitForSingleObject(fenceEvent, INFINITE);
    }
    return true;
}

// On DEVICE_REMOVED the fence may still signal but every subsequent D3D12
// call (Reset / Map / CreateView) will fail with the same removed-reason and
// the synth pass corrupts state. Detect immediately, log a single line with
// stage context + reason, and let DRED dump on Present catch the breadcrumb.
bool CheckDeviceRemovedAfterWork(ID3D12Device* device, const char* stage, int eyeIndex) {
    if (!device) return false;
    const HRESULT removed = device->GetDeviceRemovedReason();
    if (removed == S_OK) return false;
    Log("OpticalFlowD3D12: DEVICE_REMOVED detected after %s eye=%d reason=0x%08X\n",
        stage,
        eyeIndex,
        static_cast<unsigned>(removed));
    return true;
}

bool WaitForFencePoint(ID3D12Fence* fence, uint64_t value, HANDLE eventHandle) {
    if (!fence || !eventHandle) {
        return false;
    }
    if (fence->GetCompletedValue() < value) {
        if (FAILED(fence->SetEventOnCompletion(value, eventHandle))) {
            return false;
        }
        WaitForSingleObject(eventHandle, INFINITE);
    }
    return true;
}

bool CreateFencePoint(ID3D12Device* device, NV_OF_FENCE_POINT* fencePoint, const wchar_t* name) {
    if (!device || !fencePoint) {
        return false;
    }
    fencePoint->fence = nullptr;
    fencePoint->value = 0;
    if (FAILED(device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fencePoint->fence)))) {
        return false;
    }
    SetD3DName(fencePoint->fence, name);
    return true;
}

void ReleaseFencePoint(NV_OF_FENCE_POINT* fencePoint) {
    if (fencePoint && fencePoint->fence) {
        fencePoint->fence->Release();
        fencePoint->fence = nullptr;
        fencePoint->value = 0;
    }
}

NV_OF_BUFFER_FORMAT ChooseNvOFInputFormat(DXGI_FORMAT sourceFormat) {
    switch (sourceFormat) {
    case DXGI_FORMAT_R8G8B8A8_UNORM:
    case DXGI_FORMAT_B8G8R8A8_UNORM:
        return NV_OF_BUFFER_FORMAT_ABGR8;
    default:
        return NV_OF_BUFFER_FORMAT_UNDEFINED;
    }
}

DXGI_FORMAT ChooseConvertedInputFormat(DXGI_FORMAT sourceFormat) {
    switch (sourceFormat) {
    case DXGI_FORMAT_R8G8B8A8_UNORM:
    case DXGI_FORMAT_B8G8R8A8_UNORM:
        return DXGI_FORMAT_B8G8R8A8_UNORM;
    default:
        return DXGI_FORMAT_UNKNOWN;
    }
}

bool CompileShader(const char* source, const char* entryPoint, const char* target, ID3DBlob** outBlob) {
    UINT flags = D3DCOMPILE_ENABLE_STRICTNESS;
#if defined(_DEBUG)
    flags |= D3DCOMPILE_DEBUG | D3DCOMPILE_SKIP_OPTIMIZATION;
#endif
    ComPtr<ID3DBlob> shader;
    ComPtr<ID3DBlob> errors;
    const HRESULT hr = D3DCompile(source, strlen(source), nullptr, nullptr, nullptr, entryPoint, target, flags, 0, &shader, &errors);
    if (FAILED(hr)) {
        if (errors) {
            Log("OpticalFlowD3D12: Shader compile failed (%s/%s): %.*s\n",
                entryPoint,
                target,
                static_cast<int>(errors->GetBufferSize()),
                static_cast<const char*>(errors->GetBufferPointer()));
        } else {
            Log("OpticalFlowD3D12: Shader compile failed (%s/%s), hr=0x%08X\n",
                entryPoint,
                target,
                static_cast<unsigned>(hr));
        }
        return false;
    }
    *outBlob = shader.Detach();
    return true;
}

constexpr char kCopyVsSource[] = R"(
struct VSOut {
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

VSOut VSMain(uint vertexId : SV_VertexID) {
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
    VSOut outValue;
    outValue.position = float4(positions[vertexId], 0.0, 1.0);
    outValue.uv = uvs[vertexId];
    return outValue;
}
)";

constexpr char kCopyPsSource[] = R"(
Texture2D<float4> g_source : register(t0);
SamplerState g_sampler : register(s0);

struct VSOut {
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

float4 PSMain(VSOut input) : SV_Target {
    float4 c = g_source.SampleLevel(g_sampler, input.uv, 0.0);
    return float4(c.rgb, 1.0);
}
)";

constexpr char kInterpPsSource[] = R"(
cbuffer SynthCB : register(b0) {
    float2 invSize;
    float2 flowScale;
    float blendFactor;
    float3 padding;
}

Texture2D<float4> g_previous : register(t0);
Texture2D<float4> g_current : register(t1);
Texture2D<int2> g_flow : register(t2);
SamplerState g_sampler : register(s0);

struct VSOut {
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

float4 PSMain(VSOut input) : SV_Target {
    float2 pixel = input.uv / invSize - 0.5.xx;
    int2 flowCoord = int2(clamp(pixel * flowScale, 0.0.xx, 65535.0.xx));
    int2 flowRaw = g_flow.Load(int3(flowCoord, 0));
    float2 flowPixels = float2(flowRaw) / 32.0.xx;
    float2 prevUv = saturate((pixel - flowPixels * blendFactor + 0.5.xx) * invSize);
    float2 currUv = saturate((pixel + flowPixels * (1.0.xx - blendFactor) + 0.5.xx) * invSize);
    float4 prevColor = g_previous.SampleLevel(g_sampler, prevUv, 0.0);
    float4 currColor = g_current.SampleLevel(g_sampler, currUv, 0.0);
    return lerp(prevColor, currColor, blendFactor);
}
)";

// CAS (Contrast Adaptive Sharpening) shader
// #08 (DXBC, decompiled): 3x3 neighborhood, sharpness peak = -1/(8 - 3*x), final
// lerp(center, sharpened, mix). filterParam.x = FilterSharpness, .y = FilterSharpmix.
constexpr char kSharpenPsSource[] = R"(
cbuffer FilterCB : register(b0) {
    float4 filterParam; // x=sharpness(0..1), y=mix(0..1), z,w=unused
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

    // 3x3 neighborhood (a b c / d e f / g h i), e = center.
    float3 a = tap(uv, float2(-inv.x, -inv.y));
    float3 b = tap(uv, float2(    0.0, -inv.y));
    float3 c = tap(uv, float2( inv.x, -inv.y));
    float3 d = tap(uv, float2(-inv.x,    0.0));
    float3 e = tap(uv, float2(    0.0,    0.0));
    float3 f = tap(uv, float2( inv.x,    0.0));
    float3 g = tap(uv, float2(-inv.x,  inv.y));
    float3 hh= tap(uv, float2(    0.0,  inv.y));
    float3 i = tap(uv, float2( inv.x,  inv.y));

    // CAS: min/max over the cross + the corners (the "+=" ring accumulation).
    float3 mn = min(min(min(d, e), min(f, b)), hh);
    mn += min(mn, min(min(a, c), min(g, i)));
    float3 mx = max(max(max(d, e), max(f, b)), hh);
    mx += max(mx, max(max(a, c), max(g, i)));

    float3 rcpMx = rcp(max(mx, 1e-5));
    float3 amp = saturate(min(mn, 2.0 - mx) * rcpMx);
    amp = sqrt(amp);

    // sharpness peak: -1/(8 - 3*sharpness).
    float peak = -1.0 / (8.0 - 3.0 * saturate(filterParam.x));
    float3 wt = amp * peak;
    float3 rcpW = rcp(1.0 + 4.0 * wt);
    float3 sharp = saturate((b * wt + d * wt + f * wt + hh * wt + e) * rcpW);

    float3 outc = lerp(e, sharp, saturate(filterParam.y));
    return float4(outc, 1.0);
}
)";

}  // namespace

struct OpticalFlowD3D12::Impl {
    ComPtr<ID3D12Device> device;
    NvOFObj flow;
    uint32_t width = 0;
    uint32_t height = 0;
    DXGI_FORMAT sourceFormat = DXGI_FORMAT_UNKNOWN;
    DXGI_FORMAT inputFormat = DXGI_FORMAT_UNKNOWN;
    uint32_t gridSize = 0;
    uint32_t flowWidth = 0;
    uint32_t flowHeight = 0;
    ComPtr<ID3D12CommandQueue> convertQueue;
    ComPtr<ID3D12CommandAllocator> convertAllocator;
    ComPtr<ID3D12GraphicsCommandList> convertList;
    ComPtr<ID3D12Fence> convertFence;
    HANDLE convertFenceEvent = nullptr;
    UINT64 convertFenceValue = 0;
    ComPtr<ID3D12Resource> convertTarget;
    D3D12_RESOURCE_STATES convertTargetState = D3D12_RESOURCE_STATE_COMMON;
    NV_OF_FENCE_POINT readyFence{};
    NV_OF_FENCE_POINT executeFence{};
    HANDLE executeFenceEvent = nullptr;
    ComPtr<ID3D12DescriptorHeap> srvHeap;
    ComPtr<ID3D12DescriptorHeap> rtvHeap;
    ComPtr<ID3D12RootSignature> rootSignature;
    ComPtr<ID3D12PipelineState> copyPipeline;
    ComPtr<ID3D12RootSignature> interpRootSignature;
    ComPtr<ID3D12PipelineState> interpPipeline;
    ComPtr<ID3D12Resource> interpConstantBuffer;
    // CAS sharpen pass (shares interpRootSignature: CBV b0 + SRV table; only t0 used).
    ComPtr<ID3D12PipelineState> sharpenPipeline;
    ComPtr<ID3D12Resource> sharpenConstantBuffer;
    ComPtr<ID3D12Resource> interpolatedTextures[2];
    bool interpolatedValid[2] = {false, false};
    std::unordered_map<ID3D12Resource*, NvOFBufferObj> registeredInputs;
    std::vector<ComPtr<ID3D12Resource>> outputResources;
    std::vector<NvOFBufferObj> outputBuffers;
    // Set once any D3D12 work in this OF pipeline triggers DEVICE_REMOVED.
    // All subsequent ExecuteFlow/SynthesizeMidpoint calls fast-bail so we
    // don't pile additional Reset/Map failures on top of the original crash;
    // the FIRST crash is what DRED has captured.
    bool deviceLost = false;
};

OpticalFlowD3D12::OpticalFlowD3D12() = default;

OpticalFlowD3D12::~OpticalFlowD3D12() {
    Shutdown();
}

bool OpticalFlowD3D12::MatchesFailedAttemptLocked(ID3D12Device* device, uint32_t width, uint32_t height, DXGI_FORMAT format) const {
    return m_lastAttemptFailed &&
        m_lastAttemptDevice == device &&
        m_lastAttemptWidth == width &&
        m_lastAttemptHeight == height &&
        m_lastAttemptFormat == format;
}

bool OpticalFlowD3D12::EnsureInitialized(ID3D12Device* device, uint32_t width, uint32_t height, DXGI_FORMAT format) {
    std::lock_guard<std::mutex> lock(m_mutex);
    if (!device || width == 0 || height == 0 || format == DXGI_FORMAT_UNKNOWN) {
        return false;
    }

    if (m_impl &&
        m_impl->device.Get() == device &&
        m_impl->width == width &&
        m_impl->height == height &&
        m_impl->sourceFormat == format) {
        return true;
    }
    if (MatchesFailedAttemptLocked(device, width, height, format)) {
        return false;
    }

    m_impl.reset();
    m_lastAttemptDevice = device;
    m_lastAttemptWidth = width;
    m_lastAttemptHeight = height;
    m_lastAttemptFormat = format;
    m_lastAttemptFailed = false;

    const DXGI_FORMAT convertedFormat = ChooseConvertedInputFormat(format);
    const NV_OF_BUFFER_FORMAT inputFormat = ChooseNvOFInputFormat(format);
    if (convertedFormat == DXGI_FORMAT_UNKNOWN || inputFormat == NV_OF_BUFFER_FORMAT_UNDEFINED) {
        Log("OpticalFlowD3D12: Unsupported source format %u. Need an additional conversion path before AER V2 can run.\n",
            static_cast<unsigned>(format));
        m_lastAttemptFailed = true;
        return false;
    }

    D3D12_COMMAND_QUEUE_DESC queueDesc{};
    queueDesc.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
    ComPtr<ID3D12CommandQueue> convertQueue;
    ComPtr<ID3D12CommandAllocator> convertAllocator;
    ComPtr<ID3D12GraphicsCommandList> convertList;
    ComPtr<ID3D12Fence> convertFence;
    if (FAILED(device->CreateCommandQueue(&queueDesc, IID_PPV_ARGS(&convertQueue))) ||
        FAILED(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&convertAllocator))) ||
        FAILED(device->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, convertAllocator.Get(), nullptr, IID_PPV_ARGS(&convertList))) ||
        FAILED(device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&convertFence)))) {
        Log("OpticalFlowD3D12: Failed to create conversion command objects.\n");
        m_lastAttemptFailed = true;
        return false;
    }
    SetD3DName(convertQueue.Get(), L"AERV2_OF_work_queue");
    SetD3DName(convertAllocator.Get(), L"AERV2_OF_work_allocator");
    SetD3DName(convertList.Get(), L"AERV2_OF_work_command_list");
    SetD3DName(convertFence.Get(), L"AERV2_OF_work_fence");
    convertList->Close();

    HANDLE convertFenceEvent = CreateEventA(nullptr, FALSE, FALSE, nullptr);
    if (!convertFenceEvent) {
        Log("OpticalFlowD3D12: Failed to create conversion fence event.\n");
        m_lastAttemptFailed = true;
        return false;
    }

    ComPtr<ID3D12DescriptorHeap> srvHeap;
    D3D12_DESCRIPTOR_HEAP_DESC srvHeapDesc{};
    srvHeapDesc.NumDescriptors = 3;
    srvHeapDesc.Type = D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV;
    srvHeapDesc.Flags = D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE;
    if (FAILED(device->CreateDescriptorHeap(&srvHeapDesc, IID_PPV_ARGS(&srvHeap)))) {
        CloseHandle(convertFenceEvent);
        Log("OpticalFlowD3D12: Failed to create SRV heap.\n");
        m_lastAttemptFailed = true;
        return false;
    }
    SetD3DName(srvHeap.Get(), L"AERV2_OF_srv_heap");

    ComPtr<ID3D12DescriptorHeap> rtvHeap;
    D3D12_DESCRIPTOR_HEAP_DESC rtvHeapDesc{};
    rtvHeapDesc.NumDescriptors = 1;
    rtvHeapDesc.Type = D3D12_DESCRIPTOR_HEAP_TYPE_RTV;
    if (FAILED(device->CreateDescriptorHeap(&rtvHeapDesc, IID_PPV_ARGS(&rtvHeap)))) {
        CloseHandle(convertFenceEvent);
        Log("OpticalFlowD3D12: Failed to create RTV heap.\n");
        m_lastAttemptFailed = true;
        return false;
    }
    SetD3DName(rtvHeap.Get(), L"AERV2_OF_rtv_heap");

    ComPtr<ID3DBlob> vsBlob;
    ComPtr<ID3DBlob> psBlob;
    ComPtr<ID3DBlob> interpPsBlob;
    if (!CompileShader(kCopyVsSource, "VSMain", "vs_5_0", &vsBlob) ||
        !CompileShader(kCopyPsSource, "PSMain", "ps_5_0", &psBlob) ||
        !CompileShader(kInterpPsSource, "PSMain", "ps_5_0", &interpPsBlob)) {
        CloseHandle(convertFenceEvent);
        m_lastAttemptFailed = true;
        return false;
    }

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
    sampler.AddressU = D3D12_TEXTURE_ADDRESS_MODE_CLAMP;
    sampler.AddressV = D3D12_TEXTURE_ADDRESS_MODE_CLAMP;
    sampler.AddressW = D3D12_TEXTURE_ADDRESS_MODE_CLAMP;
    sampler.ShaderRegister = 0;
    sampler.RegisterSpace = 0;
    sampler.ShaderVisibility = D3D12_SHADER_VISIBILITY_PIXEL;
    sampler.MaxLOD = D3D12_FLOAT32_MAX;

    D3D12_ROOT_SIGNATURE_DESC rootDesc{};
    rootDesc.NumParameters = 1;
    rootDesc.pParameters = &param;
    rootDesc.NumStaticSamplers = 1;
    rootDesc.pStaticSamplers = &sampler;
    rootDesc.Flags = D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT;

    ComPtr<ID3DBlob> rootBlob;
    ComPtr<ID3DBlob> rootErrors;
    if (FAILED(D3D12SerializeRootSignature(&rootDesc, D3D_ROOT_SIGNATURE_VERSION_1, &rootBlob, &rootErrors))) {
        if (rootErrors) {
            Log("OpticalFlowD3D12: Root signature compile failed: %.*s\n",
                static_cast<int>(rootErrors->GetBufferSize()),
                static_cast<const char*>(rootErrors->GetBufferPointer()));
        }
        CloseHandle(convertFenceEvent);
        m_lastAttemptFailed = true;
        return false;
    }

    ComPtr<ID3D12RootSignature> rootSignature;
    if (FAILED(device->CreateRootSignature(0, rootBlob->GetBufferPointer(), rootBlob->GetBufferSize(), IID_PPV_ARGS(&rootSignature)))) {
        CloseHandle(convertFenceEvent);
        Log("OpticalFlowD3D12: Failed to create root signature.\n");
        m_lastAttemptFailed = true;
        return false;
    }
    SetD3DName(rootSignature.Get(), L"AERV2_OF_copy_root_signature");

    D3D12_RENDER_TARGET_BLEND_DESC rtBlend{};
    rtBlend.RenderTargetWriteMask = D3D12_COLOR_WRITE_ENABLE_ALL;
    D3D12_BLEND_DESC blendDesc{};
    blendDesc.RenderTarget[0] = rtBlend;

    D3D12_RASTERIZER_DESC rasterizer{};
    rasterizer.FillMode = D3D12_FILL_MODE_SOLID;
    rasterizer.CullMode = D3D12_CULL_MODE_NONE;
    rasterizer.DepthClipEnable = TRUE;

    D3D12_DEPTH_STENCIL_DESC depthStencil{};
    depthStencil.DepthEnable = FALSE;
    depthStencil.StencilEnable = FALSE;

    D3D12_GRAPHICS_PIPELINE_STATE_DESC psoDesc{};
    psoDesc.pRootSignature = rootSignature.Get();
    psoDesc.VS = {vsBlob->GetBufferPointer(), vsBlob->GetBufferSize()};
    psoDesc.PS = {psBlob->GetBufferPointer(), psBlob->GetBufferSize()};
    psoDesc.BlendState = blendDesc;
    psoDesc.SampleMask = UINT_MAX;
    psoDesc.RasterizerState = rasterizer;
    psoDesc.DepthStencilState = depthStencil;
    psoDesc.PrimitiveTopologyType = D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE;
    psoDesc.NumRenderTargets = 1;
    psoDesc.RTVFormats[0] = convertedFormat;
    psoDesc.SampleDesc.Count = 1;

    ComPtr<ID3D12PipelineState> copyPipeline;
    if (FAILED(device->CreateGraphicsPipelineState(&psoDesc, IID_PPV_ARGS(&copyPipeline)))) {
        CloseHandle(convertFenceEvent);
        Log("OpticalFlowD3D12: Failed to create conversion graphics pipeline.\n");
        m_lastAttemptFailed = true;
        return false;
    }
    SetD3DName(copyPipeline.Get(), L"AERV2_OF_copy_pipeline");

    D3D12_DESCRIPTOR_RANGE interpRanges[3] = {};
    for (UINT i = 0; i < 3; ++i) {
        interpRanges[i].RangeType = D3D12_DESCRIPTOR_RANGE_TYPE_SRV;
        interpRanges[i].NumDescriptors = 1;
        interpRanges[i].BaseShaderRegister = i;
        interpRanges[i].OffsetInDescriptorsFromTableStart = (i == 0) ? 0 : D3D12_DESCRIPTOR_RANGE_OFFSET_APPEND;
    }
    D3D12_ROOT_PARAMETER interpParams[2] = {};
    interpParams[0].ParameterType = D3D12_ROOT_PARAMETER_TYPE_CBV;
    interpParams[0].Descriptor.ShaderRegister = 0;
    interpParams[0].ShaderVisibility = D3D12_SHADER_VISIBILITY_PIXEL;
    interpParams[1].ParameterType = D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;
    interpParams[1].DescriptorTable.NumDescriptorRanges = 3;
    interpParams[1].DescriptorTable.pDescriptorRanges = interpRanges;
    interpParams[1].ShaderVisibility = D3D12_SHADER_VISIBILITY_PIXEL;

    D3D12_ROOT_SIGNATURE_DESC interpRootDesc{};
    interpRootDesc.NumParameters = 2;
    interpRootDesc.pParameters = interpParams;
    interpRootDesc.NumStaticSamplers = 1;
    interpRootDesc.pStaticSamplers = &sampler;
    interpRootDesc.Flags = D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT;

    ComPtr<ID3DBlob> interpRootBlob;
    ComPtr<ID3DBlob> interpRootErrors;
    if (FAILED(D3D12SerializeRootSignature(&interpRootDesc, D3D_ROOT_SIGNATURE_VERSION_1, &interpRootBlob, &interpRootErrors))) {
        if (interpRootErrors) {
            Log("OpticalFlowD3D12: Interp root signature compile failed: %.*s\n",
                static_cast<int>(interpRootErrors->GetBufferSize()),
                static_cast<const char*>(interpRootErrors->GetBufferPointer()));
        }
        CloseHandle(convertFenceEvent);
        m_lastAttemptFailed = true;
        return false;
    }

    ComPtr<ID3D12RootSignature> interpRootSignature;
    if (FAILED(device->CreateRootSignature(0, interpRootBlob->GetBufferPointer(), interpRootBlob->GetBufferSize(), IID_PPV_ARGS(&interpRootSignature)))) {
        CloseHandle(convertFenceEvent);
        Log("OpticalFlowD3D12: Failed to create interpolation root signature.\n");
        m_lastAttemptFailed = true;
        return false;
    }
    SetD3DName(interpRootSignature.Get(), L"AERV2_OF_interp_root_signature");

    D3D12_GRAPHICS_PIPELINE_STATE_DESC interpPsoDesc = psoDesc;
    interpPsoDesc.pRootSignature = interpRootSignature.Get();
    interpPsoDesc.PS = {interpPsBlob->GetBufferPointer(), interpPsBlob->GetBufferSize()};
    interpPsoDesc.RTVFormats[0] = format;

    ComPtr<ID3D12PipelineState> interpPipeline;
    if (FAILED(device->CreateGraphicsPipelineState(&interpPsoDesc, IID_PPV_ARGS(&interpPipeline)))) {
        CloseHandle(convertFenceEvent);
        Log("OpticalFlowD3D12: Failed to create interpolation graphics pipeline.\n");
        m_lastAttemptFailed = true;
        return false;
    }
    SetD3DName(interpPipeline.Get(), L"AERV2_OF_interp_pipeline");

    // CAS sharpen pipeline: same root signature (CBV b0 + SRV table; shader uses t0
    // only), same fullscreen VS, sharpen PS, RT format = eye format.
    ComPtr<ID3D12PipelineState> sharpenPipeline;
    {
        ComPtr<ID3DBlob> sharpenPsBlob;
        if (!CompileShader(kSharpenPsSource, "PSMain", "ps_5_0", &sharpenPsBlob)) {
            CloseHandle(convertFenceEvent);
            Log("OpticalFlowD3D12: Failed to compile CAS sharpen pixel shader.\n");
            m_lastAttemptFailed = true;
            return false;
        }
        D3D12_GRAPHICS_PIPELINE_STATE_DESC sharpenPsoDesc = interpPsoDesc;
        sharpenPsoDesc.PS = {sharpenPsBlob->GetBufferPointer(), sharpenPsBlob->GetBufferSize()};
        if (FAILED(device->CreateGraphicsPipelineState(&sharpenPsoDesc, IID_PPV_ARGS(&sharpenPipeline)))) {
            CloseHandle(convertFenceEvent);
            Log("OpticalFlowD3D12: Failed to create CAS sharpen graphics pipeline.\n");
            m_lastAttemptFailed = true;
            return false;
        }
        SetD3DName(sharpenPipeline.Get(), L"AERV2_OF_sharpen_pipeline");
    }

    ComPtr<ID3D12Resource> convertTarget;
    if (convertedFormat != format) {
        D3D12_HEAP_PROPERTIES targetHeapProps{};
        targetHeapProps.Type = D3D12_HEAP_TYPE_DEFAULT;
        D3D12_RESOURCE_DESC targetDesc{};
        targetDesc.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
        targetDesc.Width = width;
        targetDesc.Height = height;
        targetDesc.DepthOrArraySize = 1;
        targetDesc.MipLevels = 1;
        targetDesc.Format = convertedFormat;
        targetDesc.SampleDesc.Count = 1;
        targetDesc.Layout = D3D12_TEXTURE_LAYOUT_UNKNOWN;
        targetDesc.Flags = D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
        D3D12_CLEAR_VALUE targetClear{};
        targetClear.Format = convertedFormat;
        if (FAILED(device->CreateCommittedResource(
                &targetHeapProps,
                D3D12_HEAP_FLAG_NONE,
                &targetDesc,
                D3D12_RESOURCE_STATE_COMMON,
                &targetClear,
                IID_PPV_ARGS(&convertTarget)))) {
            CloseHandle(convertFenceEvent);
            Log("OpticalFlowD3D12: Failed to create conversion staging target.\n");
            m_lastAttemptFailed = true;
            return false;
        }
        SetD3DName(convertTarget.Get(), L"AERV2_OF_convert_target");
    }

    ComPtr<ID3D12Resource> interpConstantBuffer;
    D3D12_HEAP_PROPERTIES cbHeapProps{};
    cbHeapProps.Type = D3D12_HEAP_TYPE_UPLOAD;
    D3D12_RESOURCE_DESC cbDesc{};
    cbDesc.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
    cbDesc.Width = 256;
    cbDesc.Height = 1;
    cbDesc.DepthOrArraySize = 1;
    cbDesc.MipLevels = 1;
    cbDesc.SampleDesc.Count = 1;
    cbDesc.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    if (FAILED(device->CreateCommittedResource(&cbHeapProps, D3D12_HEAP_FLAG_NONE, &cbDesc, D3D12_RESOURCE_STATE_GENERIC_READ, nullptr, IID_PPV_ARGS(&interpConstantBuffer)))) {
        CloseHandle(convertFenceEvent);
        Log("OpticalFlowD3D12: Failed to create interpolation constant buffer.\n");
        m_lastAttemptFailed = true;
        return false;
    }
    SetD3DName(interpConstantBuffer.Get(), L"AERV2_OF_interp_constant_buffer");

    ComPtr<ID3D12Resource> sharpenConstantBuffer;
    if (FAILED(device->CreateCommittedResource(&cbHeapProps, D3D12_HEAP_FLAG_NONE, &cbDesc, D3D12_RESOURCE_STATE_GENERIC_READ, nullptr, IID_PPV_ARGS(&sharpenConstantBuffer)))) {
        CloseHandle(convertFenceEvent);
        Log("OpticalFlowD3D12: Failed to create CAS sharpen constant buffer.\n");
        m_lastAttemptFailed = true;
        return false;
    }
    SetD3DName(sharpenConstantBuffer.Get(), L"AERV2_OF_sharpen_constant_buffer");

    try {
        NvOFObj flow = NvOFD3D12::Create(device, width, height, inputFormat, NV_OF_MODE_OPTICALFLOW, NV_OF_PERF_LEVEL_SLOW);

        uint32_t gridSize = NV_OF_OUTPUT_VECTOR_GRID_SIZE_4;
        if (!flow->CheckGridSize(gridSize)) {
            uint32_t fallbackGrid = 0;
            if (!flow->GetNextMinGridSize(gridSize, fallbackGrid)) {
                Log("OpticalFlowD3D12: No supported optical-flow grid size for %ux%u.\n", width, height);
                CloseHandle(convertFenceEvent);
                m_lastAttemptFailed = true;
                return false;
            }
            gridSize = fallbackGrid;
        }

        flow->Init(gridSize);

        NV_OF_FENCE_POINT readyFence{};
        NV_OF_FENCE_POINT executeFence{};
        if (!CreateFencePoint(device, &readyFence, L"AERV2_OF_ready_fence") ||
            !CreateFencePoint(device, &executeFence, L"AERV2_OF_execute_fence")) {
            ReleaseFencePoint(&readyFence);
            ReleaseFencePoint(&executeFence);
            CloseHandle(convertFenceEvent);
            Log("OpticalFlowD3D12: Failed to create NvOF fence points.\n");
            m_lastAttemptFailed = true;
            return false;
        }
        HANDLE executeFenceEvent = CreateEventA(nullptr, FALSE, FALSE, nullptr);
        if (!executeFenceEvent) {
            ReleaseFencePoint(&readyFence);
            ReleaseFencePoint(&executeFence);
            CloseHandle(convertFenceEvent);
            Log("OpticalFlowD3D12: Failed to create NvOF execute fence event.\n");
            m_lastAttemptFailed = true;
            return false;
        }

        const uint32_t flowWidth = (width + gridSize - 1) / gridSize;
        const uint32_t flowHeight = (height + gridSize - 1) / gridSize;
        if (flowWidth == 0 || flowHeight == 0) {
            ReleaseFencePoint(&readyFence);
            ReleaseFencePoint(&executeFence);
            CloseHandle(executeFenceEvent);
            CloseHandle(convertFenceEvent);
            Log("OpticalFlowD3D12: Invalid flow output dimensions.\n");
            m_lastAttemptFailed = true;
            return false;
        }

        NV_OF_BUFFER_DESCRIPTOR outputDesc{};
        outputDesc.width = flowWidth;
        outputDesc.height = flowHeight;
        outputDesc.bufferUsage = NV_OF_BUFFER_USAGE_OUTPUT;
        outputDesc.bufferFormat = NV_OF_BUFFER_FORMAT_SHORT2;
        D3D12_HEAP_PROPERTIES outputHeapProps{};
        outputHeapProps.Type = D3D12_HEAP_TYPE_DEFAULT;
        D3D12_RESOURCE_DESC outputResourceDesc{};
        outputResourceDesc.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
        outputResourceDesc.Width = flowWidth;
        outputResourceDesc.Height = flowHeight;
        outputResourceDesc.DepthOrArraySize = 1;
        outputResourceDesc.MipLevels = 1;
        outputResourceDesc.Format = DXGI_FORMAT_R16G16_SINT;
        outputResourceDesc.SampleDesc.Count = 1;
        outputResourceDesc.Layout = D3D12_TEXTURE_LAYOUT_UNKNOWN;
        outputResourceDesc.Flags = D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;

        std::vector<ComPtr<ID3D12Resource>> outputResources;
        std::vector<NvOFBufferObj> outputBuffers;
        outputResources.reserve(2);
        outputBuffers.reserve(2);
        for (int eye = 0; eye < 2; ++eye) {
            ComPtr<ID3D12Resource> outputResource;
            if (FAILED(device->CreateCommittedResource(
                    &outputHeapProps,
                    D3D12_HEAP_FLAG_NONE,
                    &outputResourceDesc,
                    D3D12_RESOURCE_STATE_COMMON,
                    nullptr,
                    IID_PPV_ARGS(&outputResource)))) {
                ReleaseFencePoint(&readyFence);
                ReleaseFencePoint(&executeFence);
                CloseHandle(executeFenceEvent);
                CloseHandle(convertFenceEvent);
                Log("OpticalFlowD3D12: Failed to create preallocated flow output for eye %d.\n", eye);
                m_lastAttemptFailed = true;
                return false;
            }
            SetD3DNamef(outputResource.Get(), L"AERV2_OF_flow_output_eye%d", eye);
            ++executeFence.value;
            const uint64_t outputRegisterFenceValue = executeFence.value;
            outputBuffers.emplace_back(flow->RegisterPreAllocBuffers(outputDesc, outputResource.Get(), &readyFence, &executeFence));
            if (!WaitForFencePoint(executeFence.fence, outputRegisterFenceValue, executeFenceEvent)) {
                ReleaseFencePoint(&readyFence);
                ReleaseFencePoint(&executeFence);
                CloseHandle(executeFenceEvent);
                CloseHandle(convertFenceEvent);
                Log("OpticalFlowD3D12: Failed waiting for NvOF output registration. eye=%d\n", eye);
                m_lastAttemptFailed = true;
                return false;
            }
            outputResources.emplace_back(std::move(outputResource));
        }

        ComPtr<ID3D12Resource> interpolatedTextures[2];
        D3D12_HEAP_PROPERTIES texHeapProps{};
        texHeapProps.Type = D3D12_HEAP_TYPE_DEFAULT;
        D3D12_RESOURCE_DESC interpDesc{};
        interpDesc.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
        interpDesc.Width = width;
        interpDesc.Height = height;
        interpDesc.DepthOrArraySize = 1;
        interpDesc.MipLevels = 1;
        interpDesc.Format = format;
        interpDesc.SampleDesc.Count = 1;
        interpDesc.Layout = D3D12_TEXTURE_LAYOUT_UNKNOWN;
        interpDesc.Flags = D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
        D3D12_CLEAR_VALUE interpClear{};
        interpClear.Format = format;
        for (int eye = 0; eye < 2; ++eye) {
            if (FAILED(device->CreateCommittedResource(
                    &texHeapProps,
                    D3D12_HEAP_FLAG_NONE,
                    &interpDesc,
                    D3D12_RESOURCE_STATE_COMMON,
                    &interpClear,
                    IID_PPV_ARGS(&interpolatedTextures[eye])))) {
                ReleaseFencePoint(&readyFence);
                ReleaseFencePoint(&executeFence);
                CloseHandle(executeFenceEvent);
                CloseHandle(convertFenceEvent);
                Log("OpticalFlowD3D12: Failed to create interpolated texture for eye %d.\n", eye);
                m_lastAttemptFailed = true;
                return false;
            }
            SetD3DNamef(interpolatedTextures[eye].Get(), L"AERV2_OF_interpolated_eye%d", eye);
        }

        std::unique_ptr<Impl> impl = std::make_unique<Impl>();
        impl->device = device;
        impl->flow = std::move(flow);
        impl->width = width;
        impl->height = height;
        impl->sourceFormat = format;
        impl->inputFormat = convertedFormat;
        impl->gridSize = gridSize;
        impl->flowWidth = flowWidth;
        impl->flowHeight = flowHeight;
        impl->convertQueue = std::move(convertQueue);
        impl->convertAllocator = std::move(convertAllocator);
        impl->convertList = std::move(convertList);
        impl->convertFence = std::move(convertFence);
        impl->convertFenceEvent = convertFenceEvent;
        impl->convertTarget = std::move(convertTarget);
        impl->convertTargetState = D3D12_RESOURCE_STATE_COMMON;
        impl->readyFence = readyFence;
        impl->executeFence = executeFence;
        impl->executeFenceEvent = executeFenceEvent;
        impl->srvHeap = std::move(srvHeap);
        impl->rtvHeap = std::move(rtvHeap);
        impl->rootSignature = std::move(rootSignature);
        impl->copyPipeline = std::move(copyPipeline);
        impl->interpRootSignature = std::move(interpRootSignature);
        impl->interpPipeline = std::move(interpPipeline);
        impl->interpConstantBuffer = std::move(interpConstantBuffer);
        impl->sharpenPipeline = std::move(sharpenPipeline);
        impl->sharpenConstantBuffer = std::move(sharpenConstantBuffer);
        impl->interpolatedTextures[0] = std::move(interpolatedTextures[0]);
        impl->interpolatedTextures[1] = std::move(interpolatedTextures[1]);
        impl->outputResources = std::move(outputResources);
        impl->outputBuffers = std::move(outputBuffers);
        m_impl = std::move(impl);

        Log("OpticalFlowD3D12: Initialized. size=%ux%u sourceFormat=%u inputFormat=%u grid=%u flow=%ux%u\n",
            width,
            height,
            static_cast<unsigned>(format),
            static_cast<unsigned>(convertedFormat),
            gridSize,
            flowWidth,
            flowHeight);
        return true;
    } catch (const NvOFException& e) {
        Log("OpticalFlowD3D12: NvOF init failed: %s\n", e.what());
    } catch (const std::exception& e) {
        Log("OpticalFlowD3D12: Init failed: %s\n", e.what());
    }

    CloseHandle(convertFenceEvent);
    m_lastAttemptFailed = true;
    return false;
}

bool OpticalFlowD3D12::ConvertToInputTexture(ID3D12Resource* source, ID3D12Resource* converted,
                                             ID3D12Fence* waitFence, uint64_t waitValue,
                                             uint64_t* outSignalValue) {
    std::lock_guard<std::mutex> lock(m_mutex);
    if (!m_impl || !source || !converted) {
        return false;
    }
    if (m_impl->deviceLost) {
        return false;
    }

    if (m_impl->inputFormat == m_impl->sourceFormat) {
        if (FAILED(m_impl->convertAllocator->Reset()) || FAILED(m_impl->convertList->Reset(m_impl->convertAllocator.Get(), nullptr))) {
            Log("OpticalFlowD3D12: Failed to reset conversion command list for direct copy.\n");
            return false;
        }
        D3D12_RESOURCE_BARRIER barrier{};
        barrier.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        barrier.Transition.pResource = converted;
        barrier.Transition.StateBefore = D3D12_RESOURCE_STATE_COMMON;
        barrier.Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_DEST;
        barrier.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
        m_impl->convertList->ResourceBarrier(1, &barrier);
        m_impl->convertList->CopyResource(converted, source);
        barrier.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_DEST;
        barrier.Transition.StateAfter = D3D12_RESOURCE_STATE_COMMON;
        m_impl->convertList->ResourceBarrier(1, &barrier);
        m_impl->convertList->Close();
        // GPU-side ordering against the external producer (AER capture copy) so we
        // never read `source` before its copy lands — no CPU stall here.
        if (waitFence) {
            m_impl->convertQueue->Wait(waitFence, waitValue);
        }
        ID3D12CommandList* cmdLists[] = {m_impl->convertList.Get()};
        m_impl->convertQueue->ExecuteCommandLists(1, cmdLists);
        if (outSignalValue) {
            // Fire-and-forget: signal the convert fence and return immediately. The
            // consumer GPU-Waits on this value before reading `converted`. No CPU
            // stall here — safe on the game/present thread.
            ++m_impl->convertFenceValue;
            m_impl->convertQueue->Signal(m_impl->convertFence.Get(), m_impl->convertFenceValue);
            *outSignalValue = m_impl->convertFenceValue;
            return true;
        }
        const bool waitOk = WaitForQueueIdleLocal(m_impl->convertQueue.Get(), m_impl->convertFence.Get(), m_impl->convertFenceEvent, m_impl->convertFenceValue);
        if (CheckDeviceRemovedAfterWork(m_impl->device.Get(), "ConvertToInputTexture(direct copy)", -1)) {
            m_impl->deviceLost = true;
            return false;
        }
        return waitOk;
    }

    if (m_impl->sourceFormat != DXGI_FORMAT_R8G8B8A8_UNORM || m_impl->inputFormat != DXGI_FORMAT_B8G8R8A8_UNORM || !m_impl->convertTarget) {
        Log("OpticalFlowD3D12: No conversion path for source format %u -> input format %u.\n",
            static_cast<unsigned>(m_impl->sourceFormat),
            static_cast<unsigned>(m_impl->inputFormat));
        return false;
    }

    if (FAILED(m_impl->convertAllocator->Reset()) || FAILED(m_impl->convertList->Reset(m_impl->convertAllocator.Get(), m_impl->copyPipeline.Get()))) {
        Log("OpticalFlowD3D12: Failed to reset conversion command list.\n");
        return false;
    }

    D3D12_SHADER_RESOURCE_VIEW_DESC srvDesc{};
    srvDesc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    srvDesc.ViewDimension = D3D12_SRV_DIMENSION_TEXTURE2D;
    srvDesc.Shader4ComponentMapping = D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;
    srvDesc.Texture2D.MipLevels = 1;
    m_impl->device->CreateShaderResourceView(source, &srvDesc, m_impl->srvHeap->GetCPUDescriptorHandleForHeapStart());

    const D3D12_CPU_DESCRIPTOR_HANDLE rtvHandle = m_impl->rtvHeap->GetCPUDescriptorHandleForHeapStart();
    m_impl->device->CreateRenderTargetView(m_impl->convertTarget.Get(), nullptr, rtvHandle);

    D3D12_RESOURCE_BARRIER barriers[2] = {};
    barriers[0].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barriers[0].Transition.pResource = source;
    barriers[0].Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
    barriers[0].Transition.StateAfter = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
    barriers[0].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    barriers[1].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barriers[1].Transition.pResource = m_impl->convertTarget.Get();
    barriers[1].Transition.StateBefore = m_impl->convertTargetState;
    barriers[1].Transition.StateAfter = D3D12_RESOURCE_STATE_RENDER_TARGET;
    barriers[1].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    m_impl->convertList->ResourceBarrier(2, barriers);

    D3D12_VIEWPORT viewport{};
    viewport.Width = static_cast<float>(m_impl->width);
    viewport.Height = static_cast<float>(m_impl->height);
    viewport.MaxDepth = 1.0f;
    D3D12_RECT scissor{0, 0, static_cast<LONG>(m_impl->width), static_cast<LONG>(m_impl->height)};
    m_impl->convertList->RSSetViewports(1, &viewport);
    m_impl->convertList->RSSetScissorRects(1, &scissor);
    m_impl->convertList->OMSetRenderTargets(1, &rtvHandle, FALSE, nullptr);
    const FLOAT clearColor[4] = {0.0f, 0.0f, 0.0f, 0.0f};
    m_impl->convertList->ClearRenderTargetView(rtvHandle, clearColor, 0, nullptr);

    ID3D12DescriptorHeap* heaps[] = {m_impl->srvHeap.Get()};
    m_impl->convertList->SetDescriptorHeaps(1, heaps);
    m_impl->convertList->SetGraphicsRootSignature(m_impl->rootSignature.Get());
    m_impl->convertList->SetGraphicsRootDescriptorTable(0, m_impl->srvHeap->GetGPUDescriptorHandleForHeapStart());
    m_impl->convertList->IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
    m_impl->convertList->DrawInstanced(4, 1, 0, 0);

    D3D12_RESOURCE_BARRIER afterDraw[3] = {};
    afterDraw[0].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    afterDraw[0].Transition.pResource = source;
    afterDraw[0].Transition.StateBefore = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
    afterDraw[0].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
    afterDraw[0].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    afterDraw[1].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    afterDraw[1].Transition.pResource = m_impl->convertTarget.Get();
    afterDraw[1].Transition.StateBefore = D3D12_RESOURCE_STATE_RENDER_TARGET;
    afterDraw[1].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
    afterDraw[1].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    afterDraw[2].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    afterDraw[2].Transition.pResource = converted;
    afterDraw[2].Transition.StateBefore = D3D12_RESOURCE_STATE_COMMON;
    afterDraw[2].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_DEST;
    afterDraw[2].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    m_impl->convertList->ResourceBarrier(3, afterDraw);

    m_impl->convertList->CopyResource(converted, m_impl->convertTarget.Get());

    D3D12_RESOURCE_BARRIER afterCopy{};
    afterCopy.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    afterCopy.Transition.pResource = converted;
    afterCopy.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_DEST;
    afterCopy.Transition.StateAfter = D3D12_RESOURCE_STATE_COMMON;
    afterCopy.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    m_impl->convertList->ResourceBarrier(1, &afterCopy);

    m_impl->convertList->Close();
    // GPU-side ordering against the external producer (AER capture copy) so we
    // never read `source` before its copy lands — no CPU stall here.
    if (waitFence) {
        m_impl->convertQueue->Wait(waitFence, waitValue);
    }
    ID3D12CommandList* cmdLists[] = {m_impl->convertList.Get()};
    m_impl->convertQueue->ExecuteCommandLists(1, cmdLists);
    // The recorded list leaves convertTarget in COPY_SOURCE regardless of when the
    // GPU drains, so update the tracked state now (both sync and async modes).
    m_impl->convertTargetState = D3D12_RESOURCE_STATE_COPY_SOURCE;
    if (outSignalValue) {
        // Fire-and-forget: signal the convert fence and return immediately. The
        // consumer GPU-Waits on this value before reading `converted`. No CPU
        // stall here — safe on the game/present thread.
        ++m_impl->convertFenceValue;
        m_impl->convertQueue->Signal(m_impl->convertFence.Get(), m_impl->convertFenceValue);
        *outSignalValue = m_impl->convertFenceValue;
        return true;
    }
    const bool ok = WaitForQueueIdleLocal(m_impl->convertQueue.Get(), m_impl->convertFence.Get(), m_impl->convertFenceEvent, m_impl->convertFenceValue);
    if (CheckDeviceRemovedAfterWork(m_impl->device.Get(), "ConvertToInputTexture(BGRA convert)", -1)) {
        m_impl->deviceLost = true;
        return false;
    }
    return ok;
}

ID3D12Fence* OpticalFlowD3D12::GetConvertFence() const {
    std::lock_guard<std::mutex> lock(m_mutex);
    return m_impl ? m_impl->convertFence.Get() : nullptr;
}

bool OpticalFlowD3D12::ExecuteFlow(ID3D12Resource* previous, ID3D12Resource* current, int eyeIndex) {
    std::lock_guard<std::mutex> lock(m_mutex);
    if (!m_impl || !previous || !current || eyeIndex < 0 || eyeIndex >= static_cast<int>(m_impl->outputBuffers.size())) {
        return false;
    }
    if (m_impl->deviceLost) {
        return false;
    }

    NV_OF_BUFFER_DESCRIPTOR inputDesc{};
    inputDesc.width = m_impl->width;
    inputDesc.height = m_impl->height;
    inputDesc.bufferUsage = NV_OF_BUFFER_USAGE_INPUT;
    inputDesc.bufferFormat = NV_OF_BUFFER_FORMAT_ABGR8;
    SetD3DNamef(previous, L"AERV2_OF_previous_eye%d", eyeIndex);
    SetD3DNamef(current, L"AERV2_OF_current_eye%d", eyeIndex);

    auto ensureRegistered = [&](ID3D12Resource* resource) -> NvOFBuffer* {
        auto it = m_impl->registeredInputs.find(resource);
        if (it != m_impl->registeredInputs.end()) {
            return it->second.get();
        }
        m_impl->executeFence.value++;
        const uint64_t registerFenceValue = m_impl->executeFence.value;
        NvOFBufferObj buffer = m_impl->flow->RegisterPreAllocBuffers(inputDesc, resource, &m_impl->readyFence, &m_impl->executeFence);
        if (!WaitForFencePoint(m_impl->executeFence.fence, registerFenceValue, m_impl->executeFenceEvent)) {
            Log("OpticalFlowD3D12: Failed waiting for NvOF input registration.\n");
            return nullptr;
        }
        NvOFBuffer* raw = buffer.get();
        m_impl->registeredInputs.emplace(resource, std::move(buffer));
        return raw;
    };

    try {
        NvOFBuffer* previousBuffer = ensureRegistered(previous);
        NvOFBuffer* currentBuffer = ensureRegistered(current);
        if (!previousBuffer || !currentBuffer) {
            return false;
        }
        m_impl->executeFence.value++;
        // One NvOF session processes two independent eye streams; do not feed
        // left-eye temporal hints into the right-eye execute.
        const NV_OF_BOOL disableTemporalHints = NV_OF_TRUE;
        m_impl->flow->Execute(
            previousBuffer,
            currentBuffer,
            m_impl->outputBuffers[eyeIndex].get(),
            nullptr,
            nullptr,
            0,
            nullptr,
            &m_impl->readyFence,
            1,
            &m_impl->executeFence,
            disableTemporalHints);
        const bool waitOk = WaitForFencePoint(m_impl->executeFence.fence, m_impl->executeFence.value, m_impl->executeFenceEvent);
        if (CheckDeviceRemovedAfterWork(m_impl->device.Get(), "NvOF Execute", eyeIndex)) {
            m_impl->deviceLost = true;
            return false;
        }
        return waitOk;
    } catch (const NvOFException& e) {
        Log("OpticalFlowD3D12: ExecuteFlow failed eye=%d: %s\n", eyeIndex, e.what());
    } catch (const std::exception& e) {
        Log("OpticalFlowD3D12: ExecuteFlow failed eye=%d: %s\n", eyeIndex, e.what());
    }
    return false;
}

bool OpticalFlowD3D12::SynthesizeMidpoint(ID3D12Resource* previous, ID3D12Resource* current, int eyeIndex) {
    std::lock_guard<std::mutex> lock(m_mutex);
    if (!m_impl || !previous || !current || eyeIndex < 0 || eyeIndex >= 2 || !m_impl->interpolatedTextures[eyeIndex]) {
        return false;
    }
    if (m_impl->deviceLost) {
        return false;
    }

    auto* flowBuffer = dynamic_cast<NvOFBufferD3D12<RWPolicyDeviceAndHost>*>(m_impl->outputBuffers[eyeIndex].get());
    ID3D12Resource* flowResource = flowBuffer ? flowBuffer->getD3D12ResourceHandle() : nullptr;
    if (!flowResource) {
        return false;
    }
    SetD3DNamef(previous, L"AERV2_synth_previous_eye%d", eyeIndex);
    SetD3DNamef(current, L"AERV2_synth_current_eye%d", eyeIndex);
    SetD3DNamef(flowResource, L"AERV2_synth_flow_eye%d", eyeIndex);
    SetD3DNamef(m_impl->interpolatedTextures[eyeIndex].Get(), L"AERV2_synth_interpolated_eye%d", eyeIndex);

    if (FAILED(m_impl->convertAllocator->Reset()) || FAILED(m_impl->convertList->Reset(m_impl->convertAllocator.Get(), m_impl->interpPipeline.Get()))) {
        Log("OpticalFlowD3D12: Failed to reset interpolation command list.\n");
        return false;
    }

    D3D12_CPU_DESCRIPTOR_HANDLE srvCpu = m_impl->srvHeap->GetCPUDescriptorHandleForHeapStart();
    const UINT srvStep = m_impl->device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
    D3D12_SHADER_RESOURCE_VIEW_DESC colorSrv{};
    colorSrv.Format = m_impl->inputFormat;
    colorSrv.ViewDimension = D3D12_SRV_DIMENSION_TEXTURE2D;
    colorSrv.Shader4ComponentMapping = D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;
    colorSrv.Texture2D.MipLevels = 1;
    m_impl->device->CreateShaderResourceView(previous, &colorSrv, srvCpu);
    srvCpu.ptr += srvStep;
    m_impl->device->CreateShaderResourceView(current, &colorSrv, srvCpu);
    srvCpu.ptr += srvStep;
    D3D12_SHADER_RESOURCE_VIEW_DESC flowSrv{};
    flowSrv.Format = DXGI_FORMAT_R16G16_SINT;
    flowSrv.ViewDimension = D3D12_SRV_DIMENSION_TEXTURE2D;
    flowSrv.Shader4ComponentMapping = D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;
    flowSrv.Texture2D.MipLevels = 1;
    m_impl->device->CreateShaderResourceView(flowResource, &flowSrv, srvCpu);

    const D3D12_CPU_DESCRIPTOR_HANDLE rtvHandle = m_impl->rtvHeap->GetCPUDescriptorHandleForHeapStart();
    m_impl->device->CreateRenderTargetView(m_impl->interpolatedTextures[eyeIndex].Get(), nullptr, rtvHandle);

    struct SynthConstants {
        float invSize[2];
        float flowScale[2];
        float blendFactor;
        float padding[3];
    } constants{};
    constants.invSize[0] = 1.0f / static_cast<float>(m_impl->width);
    constants.invSize[1] = 1.0f / static_cast<float>(m_impl->height);
    constants.flowScale[0] = static_cast<float>(m_impl->flowWidth) / static_cast<float>(m_impl->width);
    constants.flowScale[1] = static_cast<float>(m_impl->flowHeight) / static_cast<float>(m_impl->height);
    constants.blendFactor = kAERV2FrameGenBlendFactor;
    void* mapped = nullptr;
    if (FAILED(m_impl->interpConstantBuffer->Map(0, nullptr, &mapped))) {
        Log("OpticalFlowD3D12: Failed to map interpolation constant buffer.\n");
        return false;
    }
    memcpy(mapped, &constants, sizeof(constants));
    m_impl->interpConstantBuffer->Unmap(0, nullptr);

    D3D12_RESOURCE_BARRIER barriers[4] = {};
    barriers[0].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barriers[0].Transition.pResource = previous;
    barriers[0].Transition.StateBefore = D3D12_RESOURCE_STATE_COMMON;
    barriers[0].Transition.StateAfter = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
    barriers[0].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    barriers[1].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barriers[1].Transition.pResource = current;
    barriers[1].Transition.StateBefore = D3D12_RESOURCE_STATE_COMMON;
    barriers[1].Transition.StateAfter = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
    barriers[1].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    barriers[2].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barriers[2].Transition.pResource = flowResource;
    barriers[2].Transition.StateBefore = D3D12_RESOURCE_STATE_COMMON;
    barriers[2].Transition.StateAfter = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
    barriers[2].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    barriers[3].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barriers[3].Transition.pResource = m_impl->interpolatedTextures[eyeIndex].Get();
    barriers[3].Transition.StateBefore = m_impl->interpolatedValid[eyeIndex] ? D3D12_RESOURCE_STATE_COPY_SOURCE : D3D12_RESOURCE_STATE_COMMON;
    barriers[3].Transition.StateAfter = D3D12_RESOURCE_STATE_RENDER_TARGET;
    barriers[3].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    m_impl->convertList->ResourceBarrier(4, barriers);

    D3D12_VIEWPORT viewport{};
    viewport.Width = static_cast<float>(m_impl->width);
    viewport.Height = static_cast<float>(m_impl->height);
    viewport.MaxDepth = 1.0f;
    D3D12_RECT scissor{0, 0, static_cast<LONG>(m_impl->width), static_cast<LONG>(m_impl->height)};
    m_impl->convertList->RSSetViewports(1, &viewport);
    m_impl->convertList->RSSetScissorRects(1, &scissor);
    m_impl->convertList->OMSetRenderTargets(1, &rtvHandle, FALSE, nullptr);
    const FLOAT clearColor[4] = {0.0f, 0.0f, 0.0f, 0.0f};
    m_impl->convertList->ClearRenderTargetView(rtvHandle, clearColor, 0, nullptr);

    ID3D12DescriptorHeap* heaps[] = {m_impl->srvHeap.Get()};
    m_impl->convertList->SetDescriptorHeaps(1, heaps);
    m_impl->convertList->SetGraphicsRootSignature(m_impl->interpRootSignature.Get());
    m_impl->convertList->SetGraphicsRootConstantBufferView(0, m_impl->interpConstantBuffer->GetGPUVirtualAddress());
    m_impl->convertList->SetGraphicsRootDescriptorTable(1, m_impl->srvHeap->GetGPUDescriptorHandleForHeapStart());
    m_impl->convertList->IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
    m_impl->convertList->DrawInstanced(4, 1, 0, 0);

    barriers[0].Transition.pResource = m_impl->interpolatedTextures[eyeIndex].Get();
    barriers[0].Transition.StateBefore = D3D12_RESOURCE_STATE_RENDER_TARGET;
    barriers[0].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
    barriers[1].Transition.pResource = previous;
    barriers[1].Transition.StateBefore = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
    barriers[1].Transition.StateAfter = D3D12_RESOURCE_STATE_COMMON;
    barriers[2].Transition.pResource = current;
    barriers[2].Transition.StateBefore = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
    barriers[2].Transition.StateAfter = D3D12_RESOURCE_STATE_COMMON;
    barriers[3].Transition.pResource = flowResource;
    barriers[3].Transition.StateBefore = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
    barriers[3].Transition.StateAfter = D3D12_RESOURCE_STATE_COMMON;
    m_impl->convertList->ResourceBarrier(4, barriers);

    m_impl->convertList->Close();
    ID3D12CommandList* cmdLists[] = {m_impl->convertList.Get()};
    m_impl->convertQueue->ExecuteCommandLists(1, cmdLists);
    const bool ok = WaitForQueueIdleLocal(m_impl->convertQueue.Get(), m_impl->convertFence.Get(), m_impl->convertFenceEvent, m_impl->convertFenceValue);
    if (CheckDeviceRemovedAfterWork(m_impl->device.Get(), "SynthesizeMidpoint", eyeIndex)) {
        m_impl->deviceLost = true;
        return false;
    }
    if (ok) {
        m_impl->interpolatedValid[eyeIndex] = true;
    }
    return ok;
}

bool OpticalFlowD3D12::ApplySharpen(ID3D12Resource* source, ID3D12Resource* dest,
                                    float sharpness, float mix) {
    std::lock_guard<std::mutex> lock(m_mutex);
    if (!m_impl || !source || !dest || !m_impl->sharpenPipeline) {
        return false;
    }
    if (m_impl->deviceLost) {
        return false;
    }
    if (FAILED(m_impl->convertAllocator->Reset()) ||
        FAILED(m_impl->convertList->Reset(m_impl->convertAllocator.Get(), m_impl->sharpenPipeline.Get()))) {
        Log("OpticalFlowD3D12: Failed to reset sharpen command list.\n");
        return false;
    }

    // One SRV (source @ t0) into the shared srv heap.
    const D3D12_CPU_DESCRIPTOR_HANDLE srvCpu = m_impl->srvHeap->GetCPUDescriptorHandleForHeapStart();
    D3D12_SHADER_RESOURCE_VIEW_DESC srv{};
    srv.Format = m_impl->inputFormat;
    srv.ViewDimension = D3D12_SRV_DIMENSION_TEXTURE2D;
    srv.Shader4ComponentMapping = D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;
    srv.Texture2D.MipLevels = 1;
    m_impl->device->CreateShaderResourceView(source, &srv, srvCpu);

    const D3D12_CPU_DESCRIPTOR_HANDLE rtvHandle = m_impl->rtvHeap->GetCPUDescriptorHandleForHeapStart();
    m_impl->device->CreateRenderTargetView(dest, nullptr, rtvHandle);

    struct FilterConstants {
        float filterParam[4];
    } constants{};
    constants.filterParam[0] = sharpness;
    constants.filterParam[1] = mix;
    void* mapped = nullptr;
    if (FAILED(m_impl->sharpenConstantBuffer->Map(0, nullptr, &mapped))) {
        Log("OpticalFlowD3D12: Failed to map sharpen constant buffer.\n");
        return false;
    }
    memcpy(mapped, &constants, sizeof(constants));
    m_impl->sharpenConstantBuffer->Unmap(0, nullptr);

    D3D12_RESOURCE_BARRIER barriers[2] = {};
    barriers[0].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barriers[0].Transition.pResource = source;
    barriers[0].Transition.StateBefore = D3D12_RESOURCE_STATE_COMMON;
    barriers[0].Transition.StateAfter = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
    barriers[0].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    barriers[1].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barriers[1].Transition.pResource = dest;
    barriers[1].Transition.StateBefore = D3D12_RESOURCE_STATE_COMMON;
    barriers[1].Transition.StateAfter = D3D12_RESOURCE_STATE_RENDER_TARGET;
    barriers[1].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    m_impl->convertList->ResourceBarrier(2, barriers);

    D3D12_VIEWPORT viewport{};
    viewport.Width = static_cast<float>(m_impl->width);
    viewport.Height = static_cast<float>(m_impl->height);
    viewport.MaxDepth = 1.0f;
    D3D12_RECT scissor{0, 0, static_cast<LONG>(m_impl->width), static_cast<LONG>(m_impl->height)};
    m_impl->convertList->RSSetViewports(1, &viewport);
    m_impl->convertList->RSSetScissorRects(1, &scissor);
    m_impl->convertList->OMSetRenderTargets(1, &rtvHandle, FALSE, nullptr);

    ID3D12DescriptorHeap* heaps[] = {m_impl->srvHeap.Get()};
    m_impl->convertList->SetDescriptorHeaps(1, heaps);
    m_impl->convertList->SetGraphicsRootSignature(m_impl->interpRootSignature.Get());
    m_impl->convertList->SetGraphicsRootConstantBufferView(0, m_impl->sharpenConstantBuffer->GetGPUVirtualAddress());
    m_impl->convertList->SetGraphicsRootDescriptorTable(1, m_impl->srvHeap->GetGPUDescriptorHandleForHeapStart());
    m_impl->convertList->IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
    m_impl->convertList->DrawInstanced(4, 1, 0, 0);

    barriers[0].Transition.pResource = source;
    barriers[0].Transition.StateBefore = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
    barriers[0].Transition.StateAfter = D3D12_RESOURCE_STATE_COMMON;
    barriers[1].Transition.pResource = dest;
    barriers[1].Transition.StateBefore = D3D12_RESOURCE_STATE_RENDER_TARGET;
    barriers[1].Transition.StateAfter = D3D12_RESOURCE_STATE_COMMON;
    m_impl->convertList->ResourceBarrier(2, barriers);

    m_impl->convertList->Close();
    ID3D12CommandList* cmdLists[] = {m_impl->convertList.Get()};
    m_impl->convertQueue->ExecuteCommandLists(1, cmdLists);
    const bool ok = WaitForQueueIdleLocal(m_impl->convertQueue.Get(), m_impl->convertFence.Get(),
                                          m_impl->convertFenceEvent, m_impl->convertFenceValue);
    if (CheckDeviceRemovedAfterWork(m_impl->device.Get(), "ApplySharpen", 0)) {
        m_impl->deviceLost = true;
        return false;
    }
    return ok;
}

void OpticalFlowD3D12::Shutdown() {
    std::lock_guard<std::mutex> lock(m_mutex);
    if (m_impl && m_impl->convertFenceEvent) {
        CloseHandle(m_impl->convertFenceEvent);
        m_impl->convertFenceEvent = nullptr;
    }
    if (m_impl && m_impl->executeFenceEvent) {
        CloseHandle(m_impl->executeFenceEvent);
        m_impl->executeFenceEvent = nullptr;
    }
    if (m_impl) {
        ReleaseFencePoint(&m_impl->readyFence);
        ReleaseFencePoint(&m_impl->executeFence);
    }
    m_impl.reset();
    m_lastAttemptDevice = nullptr;
    m_lastAttemptWidth = 0;
    m_lastAttemptHeight = 0;
    m_lastAttemptFormat = DXGI_FORMAT_UNKNOWN;
    m_lastAttemptFailed = false;
}

bool OpticalFlowD3D12::IsReady() const {
    std::lock_guard<std::mutex> lock(m_mutex);
    return m_impl != nullptr;
}

DXGI_FORMAT OpticalFlowD3D12::GetInputTextureFormat() const {
    std::lock_guard<std::mutex> lock(m_mutex);
    return m_impl ? m_impl->inputFormat : DXGI_FORMAT_UNKNOWN;
}

ID3D12Resource* OpticalFlowD3D12::GetFlowResource(int eyeIndex) const {
    std::lock_guard<std::mutex> lock(m_mutex);
    if (!m_impl || eyeIndex < 0 || eyeIndex >= static_cast<int>(m_impl->outputBuffers.size())) {
        return nullptr;
    }
    auto* buffer = dynamic_cast<NvOFBufferD3D12<RWPolicyDeviceAndHost>*>(m_impl->outputBuffers[eyeIndex].get());
    return buffer ? buffer->getD3D12ResourceHandle() : nullptr;
}

ID3D12Resource* OpticalFlowD3D12::GetInterpolatedResource(int eyeIndex) const {
    std::lock_guard<std::mutex> lock(m_mutex);
    if (!m_impl || eyeIndex < 0 || eyeIndex >= 2 || !m_impl->interpolatedValid[eyeIndex]) {
        return nullptr;
    }
    return m_impl->interpolatedTextures[eyeIndex].Get();
}
