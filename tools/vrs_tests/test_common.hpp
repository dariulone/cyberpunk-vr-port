#pragma once
#include <d3d12.h>
#include <d3d12sdklayers.h>
#include <d3dcompiler.h>
#include <dxgi1_6.h>
#include <wrl/client.h>
#include <array>
#include <cstring>
#include <iostream>
#include <mutex>
#include <source_location>
#include <sstream>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

namespace vrs_test {
using Microsoft::WRL::ComPtr;
inline std::mutex g_logMutex;
inline ComPtr<ID3D12InfoQueue> g_debugInfo;

inline void Check(bool ok, const char* text, std::source_location where = std::source_location::current()) {
    if (ok) return;
    std::lock_guard lock(g_logMutex);
    std::cerr << where.line() << ": Check failed: " << text << '\n';
    throw std::runtime_error(text);
}
inline void DumpInfoQueue(ID3D12InfoQueue* info, bool throwOnWarning) {
    if (!info) return;
    for (UINT64 i = 0, n = info->GetNumStoredMessages(); i < n; ++i) {
        SIZE_T bytes = 0; info->GetMessage(i, nullptr, &bytes);
        if (!bytes) continue;
        std::vector<char> storage(bytes);
        auto* m = reinterpret_cast<D3D12_MESSAGE*>(storage.data());
        if (SUCCEEDED(info->GetMessage(i, m, &bytes)) && m->pDescription) {
            if (!throwOnWarning || m->Severity <= D3D12_MESSAGE_SEVERITY_WARNING) std::cerr << m->pDescription << '\n';
            if (throwOnWarning && m->Severity <= D3D12_MESSAGE_SEVERITY_WARNING) throw std::runtime_error("D3D12 validation warning or error");
        }
    }
}
inline void Hr(HRESULT hr, std::source_location where = std::source_location::current()) {
    if (SUCCEEDED(hr)) return;
    std::lock_guard lock(g_logMutex);
    std::ostringstream oss; oss << where.line() << ": HRESULT 0x" << std::hex << unsigned(hr) << std::dec;
    DumpInfoQueue(g_debugInfo.Get(), false);
    throw std::runtime_error(oss.str());
}
struct UniqueHandle {
    HANDLE handle{};
    UniqueHandle() = default;
    explicit UniqueHandle(HANDLE h) : handle(h) {}
    ~UniqueHandle() { if (handle) CloseHandle(handle); }
    UniqueHandle(const UniqueHandle&) = delete;
    UniqueHandle& operator=(const UniqueHandle&) = delete;
    UniqueHandle(UniqueHandle&& o) noexcept : handle(std::exchange(o.handle, nullptr)) {}
    UniqueHandle& operator=(UniqueHandle&& o) noexcept {
        if (this != &o) { if (handle) CloseHandle(handle); handle = std::exchange(o.handle, nullptr); }
        return *this;
    }
};
inline bool InitDebugLayerOrSkip() {
    char env[8]{};
    const bool req = GetEnvironmentVariableA("CVR_REQUIRE_D3D12_DEBUG", env, sizeof(env)) == 1 && env[0] == '1';
    ComPtr<ID3D12Debug> debug;
    const HRESULT hr = D3D12GetDebugInterface(IID_PPV_ARGS(&debug));
    if (FAILED(hr)) {
        if (req) Hr(hr);
        std::cout << "SKIP D3D12 debug layer unavailable (HRESULT 0x" << std::hex << unsigned(hr) << std::dec << ")\n";
        return false;
    }
    debug->EnableDebugLayer();
    return true;
}
struct DeviceSelection { ComPtr<ID3D12Device> device; D3D12_FEATURE_DATA_D3D12_OPTIONS6 options6{}; D3D12_FEATURE_DATA_D3D12_OPTIONS3 options3{}; };
inline DeviceSelection FindTier2Device(bool requireViewInstancing) {
    ComPtr<IDXGIFactory6> factory; Hr(CreateDXGIFactory1(IID_PPV_ARGS(&factory)));
    DeviceSelection sel;
    for (UINT i = 0;; ++i) {
        ComPtr<IDXGIAdapter1> adapter;
        if (factory->EnumAdapterByGpuPreference(i, DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE, IID_PPV_ARGS(&adapter)) == DXGI_ERROR_NOT_FOUND) break;
        DXGI_ADAPTER_DESC1 desc{};
        if (FAILED(adapter->GetDesc1(&desc)) || (desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE)) continue;
        ComPtr<ID3D12Device> dev;
        if (FAILED(D3D12CreateDevice(adapter.Get(), D3D_FEATURE_LEVEL_12_0, IID_PPV_ARGS(&dev)))) continue;
        D3D12_FEATURE_DATA_D3D12_OPTIONS6 o6{}; D3D12_FEATURE_DATA_D3D12_OPTIONS3 o3{}; D3D12_FEATURE_DATA_SHADER_MODEL sm{D3D_SHADER_MODEL_6_1};
        const bool okVrs = SUCCEEDED(dev->CheckFeatureSupport(D3D12_FEATURE_D3D12_OPTIONS6, &o6, sizeof(o6))) &&
                           o6.VariableShadingRateTier >= D3D12_VARIABLE_SHADING_RATE_TIER_2 && o6.ShadingRateImageTileSize > 0;
        const bool okVi = !requireViewInstancing || (SUCCEEDED(dev->CheckFeatureSupport(D3D12_FEATURE_D3D12_OPTIONS3, &o3, sizeof(o3))) &&
                           o3.ViewInstancingTier >= D3D12_VIEW_INSTANCING_TIER_1 &&
                           SUCCEEDED(dev->CheckFeatureSupport(D3D12_FEATURE_SHADER_MODEL, &sm, sizeof(sm))) && sm.HighestShaderModel >= D3D_SHADER_MODEL_6_1);
        if (okVrs && okVi) { sel = {std::move(dev), o6, o3}; Hr(sel.device.As(&g_debugInfo)); break; }
    }
    return sel;
}
inline ComPtr<ID3D12Resource> MakeResource(ID3D12Device* dev, D3D12_RESOURCE_DESC d, D3D12_HEAP_TYPE type,
                                           D3D12_RESOURCE_STATES state, const D3D12_CLEAR_VALUE* clear = nullptr) {
    D3D12_HEAP_PROPERTIES h{}; h.Type = type; ComPtr<ID3D12Resource> r;
    Hr(dev->CreateCommittedResource(&h, D3D12_HEAP_FLAG_NONE, &d, state, clear, IID_PPV_ARGS(&r)));
    return r;
}
inline ComPtr<ID3D12Resource> MakeBuffer(ID3D12Device* dev, UINT64 bytes, D3D12_HEAP_TYPE type, D3D12_RESOURCE_STATES state) {
    D3D12_RESOURCE_DESC d{}; d.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER; d.Width = bytes;
    d.Height = 1; d.DepthOrArraySize = 1; d.MipLevels = 1; d.SampleDesc.Count = 1; d.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    return MakeResource(dev, d, type, state);
}
inline ComPtr<ID3D12Resource> MakeTexture2D(ID3D12Device* dev, UINT w, UINT h, DXGI_FORMAT fmt, D3D12_RESOURCE_FLAGS flags,
                                            D3D12_RESOURCE_STATES state, const D3D12_CLEAR_VALUE* clear = nullptr) {
    D3D12_RESOURCE_DESC d{}; d.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D; d.Width = w; d.Height = h;
    d.DepthOrArraySize = 1; d.MipLevels = 1; d.Format = fmt; d.SampleDesc.Count = 1; d.Flags = flags;
    return MakeResource(dev, d, D3D12_HEAP_TYPE_DEFAULT, state, clear);
}
struct TargetSet {
    std::array<ComPtr<ID3D12Resource>, 3> colors;
    ComPtr<ID3D12Resource> depth;
    ComPtr<ID3D12DescriptorHeap> rtvHeap, dsvHeap;
    D3D12_CPU_DESCRIPTOR_HANDLE rtv{}, dsv{};
    UINT rtvStride{};
};
inline TargetSet CreateTargetSet(ID3D12Device* dev, UINT w, UINT h) {
    TargetSet t;
    D3D12_CLEAR_VALUE rtClear{DXGI_FORMAT_R8G8B8A8_UNORM, {{0.f, 0.f, 0.f, 1.f}}};
    D3D12_CLEAR_VALUE dsClear{}; dsClear.Format = DXGI_FORMAT_D32_FLOAT; dsClear.DepthStencil.Depth = 1.f;
    for (auto& c : t.colors) c = MakeTexture2D(dev, w, h, DXGI_FORMAT_R8G8B8A8_UNORM, D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET, D3D12_RESOURCE_STATE_RENDER_TARGET, &rtClear);
    t.depth = MakeTexture2D(dev, w, h, DXGI_FORMAT_D32_FLOAT, D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL, D3D12_RESOURCE_STATE_DEPTH_WRITE, &dsClear);
    D3D12_DESCRIPTOR_HEAP_DESC hd{D3D12_DESCRIPTOR_HEAP_TYPE_RTV, 3}; Hr(dev->CreateDescriptorHeap(&hd, IID_PPV_ARGS(&t.rtvHeap)));
    hd = {D3D12_DESCRIPTOR_HEAP_TYPE_DSV, 1}; Hr(dev->CreateDescriptorHeap(&hd, IID_PPV_ARGS(&t.dsvHeap)));
    t.rtvStride = dev->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV);
    t.rtv = t.rtvHeap->GetCPUDescriptorHandleForHeapStart();
    for (UINT i = 0; i < 3; ++i) dev->CreateRenderTargetView(t.colors[i].Get(), nullptr, {t.rtv.ptr + SIZE_T(i) * t.rtvStride});
    t.dsv = t.dsvHeap->GetCPUDescriptorHandleForHeapStart();
    dev->CreateDepthStencilView(t.depth.Get(), nullptr, t.dsv);
    return t;
}
struct Shaders { ComPtr<ID3DBlob> vs, ps; };
inline Shaders CompileTestShaders() {
    static constexpr const char* kSrc = "float4 vs(uint id:SV_VertexID):SV_Position{return float4(id==2?3:-1,id==1?3:-1,.5,1);}\n"
        "float4 ps(float4 p:SV_Position):SV_Target{return float4(frac(p.x/31),frac(p.y/29),frac((p.x+p.y)/23),1);}\n";
    Shaders s; ComPtr<ID3DBlob> err;
    Hr(D3DCompile(kSrc, std::strlen(kSrc), nullptr, nullptr, nullptr, "vs", "vs_5_0", D3DCOMPILE_OPTIMIZATION_LEVEL3, 0, &s.vs, &err));
    Hr(D3DCompile(kSrc, std::strlen(kSrc), nullptr, nullptr, nullptr, "ps", "ps_5_0", D3DCOMPILE_OPTIMIZATION_LEVEL3, 0, &s.ps, &err));
    return s;
}
inline D3D12_GRAPHICS_PIPELINE_STATE_DESC Make3MrtPsoDesc(ID3D12RootSignature* root, D3D12_SHADER_BYTECODE vs, D3D12_SHADER_BYTECODE ps, bool blended = false) {
    D3D12_GRAPHICS_PIPELINE_STATE_DESC pd{}; pd.pRootSignature = root; pd.VS = vs; pd.PS = ps; pd.SampleMask = UINT_MAX; pd.SampleDesc.Count = 1;
    for (auto& b : pd.BlendState.RenderTarget) {
        b.SrcBlend = b.SrcBlendAlpha = D3D12_BLEND_ONE; b.DestBlend = b.DestBlendAlpha = D3D12_BLEND_ZERO;
        b.BlendOp = b.BlendOpAlpha = D3D12_BLEND_OP_ADD; b.LogicOp = D3D12_LOGIC_OP_NOOP; b.RenderTargetWriteMask = D3D12_COLOR_WRITE_ENABLE_ALL;
    }
    if (blended) { pd.BlendState.RenderTarget[0].BlendEnable = TRUE; pd.BlendState.RenderTarget[0].SrcBlend = D3D12_BLEND_SRC_ALPHA; pd.BlendState.RenderTarget[0].DestBlend = D3D12_BLEND_INV_SRC_ALPHA; }
    pd.RasterizerState.FillMode = D3D12_FILL_MODE_SOLID; pd.RasterizerState.CullMode = D3D12_CULL_MODE_NONE; pd.RasterizerState.DepthClipEnable = TRUE;
    pd.DepthStencilState.DepthEnable = TRUE; pd.DepthStencilState.DepthWriteMask = D3D12_DEPTH_WRITE_MASK_ALL; pd.DepthStencilState.DepthFunc = D3D12_COMPARISON_FUNC_ALWAYS;
    pd.PrimitiveTopologyType = D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE; pd.NumRenderTargets = 3; pd.DSVFormat = DXGI_FORMAT_D32_FLOAT;
    for (UINT i = 0; i < 3; ++i) pd.RTVFormats[i] = DXGI_FORMAT_R8G8B8A8_UNORM;
    return pd;
}
inline void Barrier(ID3D12GraphicsCommandList* l, ID3D12Resource* r, D3D12_RESOURCE_STATES from, D3D12_RESOURCE_STATES to) {
    D3D12_RESOURCE_BARRIER b{}; b.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION; b.Transition = {r, 0, from, to}; l->ResourceBarrier(1, &b);
}
inline void VerifyInfoQueueClean(ID3D12Device* dev) {
    ComPtr<ID3D12InfoQueue> info; Hr(dev->QueryInterface(IID_PPV_ARGS(&info))); DumpInfoQueue(info.Get(), true);
}
} // namespace vrs_test
