#include "Render/SrgbToLinearPass.hpp"

#if !CVR_RUST_VRIK
// The C++ build's pass; the Rust build's is rust/crates/cvr-gpu/src/srgb_pass.rs (the header calls it).
#include <cstring>
#include <d3dcompiler.h>

extern void Log(const char* fmt, ...);

namespace {
using Microsoft::WRL::ComPtr;
constexpr uint32_t kDescriptorCount = 6; // Three submit command lists, two eyes each.

constexpr char kVertexShader[] = R"(
struct VSOut {
    float4 position : SV_Position;
};

VSOut VSMain(uint vertexId : SV_VertexID) {
    static const float2 positions[3] = {
        float2(-1.0,  1.0),
        float2( 3.0,  1.0),
        float2(-1.0, -3.0)
    };
    VSOut output;
    output.position = float4(positions[vertexId], 0.0, 1.0);
    return output;
}
)";

constexpr char kPixelShader[] = R"(
cbuffer ConversionParams : register(b0) {
    uint manuallyDecodeSrgb;
};
Texture2D<float4> sourceTexture : register(t0);

float3 SrgbToLinear(float3 value) {
    const float3 low = value / 12.92;
    const float3 high = pow((value + 0.055) / 1.055, 2.4);
    return lerp(low, high, step(0.04045, value));
}

float4 PSMain(float4 position : SV_Position) : SV_Target {
    const float4 encoded = sourceTexture.Load(int3(position.xy, 0));
    const float3 linearColor = manuallyDecodeSrgb != 0
        ? SrgbToLinear(encoded.rgb)
        : encoded.rgb;
    return float4(linearColor, encoded.a);
}
)";

bool CompileShader(const char* source, const char* entry, const char* target,
                   ComPtr<ID3DBlob>& bytecode) {
    ComPtr<ID3DBlob> errors;
    const HRESULT hr = D3DCompile(source, std::strlen(source), nullptr, nullptr, nullptr,
        entry, target, D3DCOMPILE_ENABLE_STRICTNESS, 0, &bytecode, &errors);
    if (FAILED(hr)) {
        Log("SrgbToLinearPass: shader compile failed %s/%s hr=0x%08X %.*s\n",
            entry, target, static_cast<unsigned>(hr),
            errors ? static_cast<int>(errors->GetBufferSize()) : 0,
            errors ? static_cast<const char*>(errors->GetBufferPointer()) : "");
        return false;
    }
    return true;
}

struct SourceView {
    DXGI_FORMAT format = DXGI_FORMAT_UNKNOWN;
    bool manuallyDecodeSrgb = false;
};

SourceView GetSourceView(DXGI_FORMAT resourceFormat) {
    switch (resourceFormat) {
        case DXGI_FORMAT_R8G8B8A8_TYPELESS:
        case DXGI_FORMAT_R8G8B8A8_UNORM:
            return {DXGI_FORMAT_R8G8B8A8_UNORM, true};
        case DXGI_FORMAT_R8G8B8A8_UNORM_SRGB:
            return {DXGI_FORMAT_R8G8B8A8_UNORM_SRGB, false};
        case DXGI_FORMAT_B8G8R8A8_TYPELESS:
        case DXGI_FORMAT_B8G8R8A8_UNORM:
            return {DXGI_FORMAT_B8G8R8A8_UNORM, true};
        case DXGI_FORMAT_B8G8R8A8_UNORM_SRGB:
            return {DXGI_FORMAT_B8G8R8A8_UNORM_SRGB, false};
        default:
            return {};
    }
}

} // namespace

SrgbToLinearPass::~SrgbToLinearPass() {
    Shutdown();
}

void SrgbToLinearPass::Shutdown() {
    std::lock_guard<std::mutex> lock(m_mutex);
    m_pipelineState.Reset();
    m_rootSignature.Reset();
    m_srvHeap.Reset();
    m_rtvHeap.Reset();
    m_device.Reset();
    m_outputFormat = DXGI_FORMAT_UNKNOWN;
    m_srvDescriptorSize = 0;
    m_rtvDescriptorSize = 0;
    m_width = 0;
    m_height = 0;
}

bool SrgbToLinearPass::EnsureInitialized(ID3D12Device* device,
                                         DXGI_FORMAT outputFormat,
                                         uint32_t width,
                                         uint32_t height) {
    std::lock_guard<std::mutex> lock(m_mutex);
    if (!device || outputFormat == DXGI_FORMAT_UNKNOWN || width == 0 || height == 0) {
        return false;
    }
    if (m_device.Get() == device && m_outputFormat == outputFormat &&
        m_width == width && m_height == height && m_pipelineState) {
        return true;
    }

    m_pipelineState.Reset();
    m_rootSignature.Reset();
    m_srvHeap.Reset();
    m_rtvHeap.Reset();
    m_device = device;
    m_outputFormat = outputFormat;
    m_width = width;
    m_height = height;

    D3D12_DESCRIPTOR_HEAP_DESC srvHeapDesc{};
    srvHeapDesc.Type = D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV;
    srvHeapDesc.NumDescriptors = kDescriptorCount;
    srvHeapDesc.Flags = D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE;
    if (FAILED(device->CreateDescriptorHeap(&srvHeapDesc, IID_PPV_ARGS(&m_srvHeap)))) {
        Log("SrgbToLinearPass: failed to create SRV heap\n");
        return false;
    }
    m_srvDescriptorSize = device->GetDescriptorHandleIncrementSize(
        D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);

    D3D12_DESCRIPTOR_HEAP_DESC rtvHeapDesc{};
    rtvHeapDesc.Type = D3D12_DESCRIPTOR_HEAP_TYPE_RTV;
    rtvHeapDesc.NumDescriptors = kDescriptorCount;
    if (FAILED(device->CreateDescriptorHeap(&rtvHeapDesc, IID_PPV_ARGS(&m_rtvHeap)))) {
        Log("SrgbToLinearPass: failed to create RTV heap\n");
        return false;
    }
    m_rtvDescriptorSize = device->GetDescriptorHandleIncrementSize(
        D3D12_DESCRIPTOR_HEAP_TYPE_RTV);

    D3D12_DESCRIPTOR_RANGE srvRange{};
    srvRange.RangeType = D3D12_DESCRIPTOR_RANGE_TYPE_SRV;
    srvRange.NumDescriptors = 1;
    srvRange.BaseShaderRegister = 0;

    D3D12_ROOT_PARAMETER rootParameters[2]{};
    rootParameters[0].ParameterType = D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS;
    rootParameters[0].Constants.ShaderRegister = 0;
    rootParameters[0].Constants.Num32BitValues = 1;
    rootParameters[0].ShaderVisibility = D3D12_SHADER_VISIBILITY_PIXEL;
    rootParameters[1].ParameterType = D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;
    rootParameters[1].DescriptorTable.NumDescriptorRanges = 1;
    rootParameters[1].DescriptorTable.pDescriptorRanges = &srvRange;
    rootParameters[1].ShaderVisibility = D3D12_SHADER_VISIBILITY_PIXEL;

    D3D12_ROOT_SIGNATURE_DESC rootDesc{};
    rootDesc.NumParameters = 2;
    rootDesc.pParameters = rootParameters;
    rootDesc.Flags = D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT;

    ComPtr<ID3DBlob> rootBlob;
    ComPtr<ID3DBlob> rootErrors;
    if (FAILED(D3D12SerializeRootSignature(&rootDesc, D3D_ROOT_SIGNATURE_VERSION_1,
            &rootBlob, &rootErrors))) {
        Log("SrgbToLinearPass: root signature serialization failed %.*s\n",
            rootErrors ? static_cast<int>(rootErrors->GetBufferSize()) : 0,
            rootErrors ? static_cast<const char*>(rootErrors->GetBufferPointer()) : "");
        return false;
    }
    if (FAILED(device->CreateRootSignature(0, rootBlob->GetBufferPointer(),
            rootBlob->GetBufferSize(), IID_PPV_ARGS(&m_rootSignature)))) {
        Log("SrgbToLinearPass: failed to create root signature\n");
        return false;
    }

    ComPtr<ID3DBlob> vertexShader;
    ComPtr<ID3DBlob> pixelShader;
    if (!CompileShader(kVertexShader, "VSMain", "vs_5_0", vertexShader) ||
        !CompileShader(kPixelShader, "PSMain", "ps_5_0", pixelShader)) {
        return false;
    }

    D3D12_GRAPHICS_PIPELINE_STATE_DESC pso{};
    pso.pRootSignature = m_rootSignature.Get();
    pso.VS = {vertexShader->GetBufferPointer(), vertexShader->GetBufferSize()};
    pso.PS = {pixelShader->GetBufferPointer(), pixelShader->GetBufferSize()};
    pso.PrimitiveTopologyType = D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE;
    pso.NumRenderTargets = 1;
    pso.RTVFormats[0] = outputFormat;
    pso.SampleDesc.Count = 1;
    pso.SampleMask = UINT_MAX;
    pso.RasterizerState.FillMode = D3D12_FILL_MODE_SOLID;
    pso.RasterizerState.CullMode = D3D12_CULL_MODE_NONE;
    pso.BlendState.RenderTarget[0].RenderTargetWriteMask = D3D12_COLOR_WRITE_ENABLE_ALL;
    pso.DepthStencilState.DepthEnable = FALSE;
    pso.DepthStencilState.StencilEnable = FALSE;
    if (FAILED(device->CreateGraphicsPipelineState(&pso, IID_PPV_ARGS(&m_pipelineState)))) {
        Log("SrgbToLinearPass: failed to create pipeline state for format %u\n",
            static_cast<unsigned>(outputFormat));
        return false;
    }

    Log("SrgbToLinearPass: initialized %ux%u outputFmt=%u\n",
        width, height, static_cast<unsigned>(outputFormat));
    return true;
}

bool SrgbToLinearPass::Record(ID3D12GraphicsCommandList* cmdList,
                              ID3D12Resource* source,
                              ID3D12Resource* destination,
                              uint32_t descriptorIndex) {
    std::lock_guard<std::mutex> lock(m_mutex);
    if (!cmdList || !source || !destination ||
        descriptorIndex >= kDescriptorCount || !m_pipelineState) {
        return false;
    }

    const SourceView sourceView = GetSourceView(source->GetDesc().Format);
    if (sourceView.format == DXGI_FORMAT_UNKNOWN) {
        Log("SrgbToLinearPass: unsupported source format %u\n",
            static_cast<unsigned>(source->GetDesc().Format));
        return false;
    }

    D3D12_SHADER_RESOURCE_VIEW_DESC srv{};
    srv.Format = sourceView.format;
    srv.ViewDimension = D3D12_SRV_DIMENSION_TEXTURE2D;
    srv.Shader4ComponentMapping = D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;
    srv.Texture2D.MipLevels = 1;
    D3D12_CPU_DESCRIPTOR_HANDLE srvCpu = m_srvHeap->GetCPUDescriptorHandleForHeapStart();
    srvCpu.ptr += static_cast<SIZE_T>(descriptorIndex) * m_srvDescriptorSize;
    m_device->CreateShaderResourceView(source, &srv, srvCpu);

    D3D12_RENDER_TARGET_VIEW_DESC rtv{};
    rtv.Format = m_outputFormat;
    rtv.ViewDimension = D3D12_RTV_DIMENSION_TEXTURE2D;
    D3D12_CPU_DESCRIPTOR_HANDLE rtvCpu = m_rtvHeap->GetCPUDescriptorHandleForHeapStart();
    rtvCpu.ptr += static_cast<SIZE_T>(descriptorIndex) * m_rtvDescriptorSize;
    m_device->CreateRenderTargetView(destination, &rtv, rtvCpu);

    D3D12_VIEWPORT viewport{};
    viewport.Width = static_cast<float>(m_width);
    viewport.Height = static_cast<float>(m_height);
    viewport.MaxDepth = 1.0f;
    const D3D12_RECT scissor{0, 0, static_cast<LONG>(m_width), static_cast<LONG>(m_height)};
    cmdList->RSSetViewports(1, &viewport);
    cmdList->RSSetScissorRects(1, &scissor);
    cmdList->OMSetRenderTargets(1, &rtvCpu, FALSE, nullptr);
    cmdList->SetGraphicsRootSignature(m_rootSignature.Get());
    cmdList->SetPipelineState(m_pipelineState.Get());
    ID3D12DescriptorHeap* heaps[] = {m_srvHeap.Get()};
    cmdList->SetDescriptorHeaps(1, heaps);
    D3D12_GPU_DESCRIPTOR_HANDLE srvGpu = m_srvHeap->GetGPUDescriptorHandleForHeapStart();
    srvGpu.ptr += static_cast<UINT64>(descriptorIndex) * m_srvDescriptorSize;
    cmdList->SetGraphicsRoot32BitConstant(0, sourceView.manuallyDecodeSrgb ? 1u : 0u, 0);
    cmdList->SetGraphicsRootDescriptorTable(1, srvGpu);
    cmdList->IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    cmdList->DrawInstanced(3, 1, 0, 0);
    return true;
}
#endif
