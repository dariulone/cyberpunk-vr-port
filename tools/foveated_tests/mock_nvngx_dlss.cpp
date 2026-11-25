#ifndef MOCK_NVNGX_EXPORTS
#define MOCK_NVNGX_EXPORTS
#endif
#include "mock_nvngx_dlss.h"
#include <wrl/client.h>
#include <algorithm>
#include <mutex>
#include <vector>

struct cvr::foveated::ngx::Handle { uint64_t id = 0; };

namespace {
using Microsoft::WRL::ComPtr;
using namespace cvr::foveated::ngx;

constexpr const char* kPresetKeys[6]{"DLSS.Hint.Render.Preset.DLAA", "DLSS.Hint.Render.Preset.Quality",
    "DLSS.Hint.Render.Preset.Balanced", "DLSS.Hint.Render.Preset.Performance",
    "DLSS.Hint.Render.Preset.UltraPerformance", "DLSS.Hint.Render.Preset.UltraQuality"};
std::mutex g_mutex;
MockNgxState g_state;
std::vector<Handle*> g_aliveHandles;
std::vector<ComPtr<ID3D12DescriptorHeap>> g_keptHeaps;

DXGI_FORMAT ViewFormat(DXGI_FORMAT format) {
    switch (format) {
    case DXGI_FORMAT_R32_TYPELESS: case DXGI_FORMAT_D32_FLOAT: return DXGI_FORMAT_R32_FLOAT;
    case DXGI_FORMAT_R24G8_TYPELESS: case DXGI_FORMAT_D24_UNORM_S8_UINT: return DXGI_FORMAT_R24_UNORM_X8_TYPELESS;
    case DXGI_FORMAT_R16G16_TYPELESS: return DXGI_FORMAT_R16G16_FLOAT;
    case DXGI_FORMAT_R16G16B16A16_TYPELESS: return DXGI_FORMAT_R16G16B16A16_FLOAT;
    case DXGI_FORMAT_R8G8B8A8_TYPELESS: return DXGI_FORMAT_R8G8B8A8_UNORM;
    default: return format;
    }
}

void ComputeClearColor(uint64_t id, float rgba[4]) {
    // Power-of-two fractions store losslessly in R16G16B16A16_FLOAT.
    rgba[0] = 0.125f + 0.0625f * static_cast<float>(id & 7u);
    rgba[1] = 0.250f + 0.0625f * static_cast<float>((id >> 3u) & 7u);
    rgba[2] = 0.875f - 0.0625f * static_cast<float>(id & 7u);
    rgba[3] = 1.0f;
}

__declspec(noinline) void ReadPresets(const Parameters* p, unsigned presets[6]) {
    for (unsigned i = 0; i < 6; ++i) presets[i] = Integer(p, kPresetKeys[i]);
}

bool IsAliveLocked(const Handle* handle) {
    return handle && std::any_of(g_aliveHandles.begin(), g_aliveHandles.end(),
                                 [handle](const Handle* entry) { return entry == handle; });
}

__declspec(noinline) void ClearOutputUav(ID3D12GraphicsCommandList* list, ID3D12Resource* output, uint64_t id) {
    if (!list || !output) return;
    const auto desc = output->GetDesc();
    if (desc.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D || !(desc.Flags & D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS)) return;
    ComPtr<ID3D12Device> device;
    if (FAILED(list->GetDevice(IID_PPV_ARGS(&device))) || !device) return;
    D3D12_DESCRIPTOR_HEAP_DESC hd{D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, 1, D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE, 0};
    ComPtr<ID3D12DescriptorHeap> gpuHeap, cpuHeap;
    if (FAILED(device->CreateDescriptorHeap(&hd, IID_PPV_ARGS(&gpuHeap)))) return;
    hd.Flags = D3D12_DESCRIPTOR_HEAP_FLAG_NONE;
    if (FAILED(device->CreateDescriptorHeap(&hd, IID_PPV_ARGS(&cpuHeap)))) return;
    D3D12_UNORDERED_ACCESS_VIEW_DESC uav{ViewFormat(desc.Format), D3D12_UAV_DIMENSION_TEXTURE2D, {}};
    device->CreateUnorderedAccessView(output, nullptr, &uav, gpuHeap->GetCPUDescriptorHandleForHeapStart());
    device->CreateUnorderedAccessView(output, nullptr, &uav, cpuHeap->GetCPUDescriptorHandleForHeapStart());
    ID3D12DescriptorHeap* heaps[]{gpuHeap.Get()}; list->SetDescriptorHeaps(1, heaps);
    float rgba[4]{}; ComputeClearColor(id, rgba);
    list->ClearUnorderedAccessViewFloat(gpuHeap->GetGPUDescriptorHandleForHeapStart(),
                                        cpuHeap->GetCPUDescriptorHandleForHeapStart(), output, rgba, 0, nullptr);
    D3D12_RESOURCE_BARRIER b{}; b.Type = D3D12_RESOURCE_BARRIER_TYPE_UAV; b.UAV.pResource = output;
    list->ResourceBarrier(1, &b);
    g_keptHeaps.push_back(std::move(gpuHeap)); g_keptHeaps.push_back(std::move(cpuHeap));
}

__declspec(noinline) Result EvaluateFeatureImpl(
    ID3D12GraphicsCommandList* list, const Handle* handle, const Parameters* p, void* callback, bool isCRoute) {
    (void)callback;
    MockNgxEvaluateRecord rec{}; rec.handle = handle;
    if (p) {
        rec.width = Integer(p, "Width", Integer(p, "DLSS.Render.Subrect.Dimensions.Width"));
        rec.height = Integer(p, "Height", Integer(p, "DLSS.Render.Subrect.Dimensions.Height"));
        rec.outWidth = Integer(p, "OutWidth"); rec.outHeight = Integer(p, "OutHeight");
        rec.perfQualityValue = Integer(p, "PerfQualityValue"); rec.createFlags = Integer(p, "DLSS.Feature.Create.Flags");
        rec.reset = Integer(p, "Reset");
        rec.mvScaleX = Read<float>(p, "MV.Scale.X"); rec.mvScaleY = Read<float>(p, "MV.Scale.Y");
        rec.jitterOffsetX = Read<float>(p, "Jitter.Offset.X"); rec.jitterOffsetY = Read<float>(p, "Jitter.Offset.Y");
        rec.color = Read<ID3D12Resource*>(p, "Color"); rec.depth = Read<ID3D12Resource*>(p, "Depth");
        rec.motionVectors = Read<ID3D12Resource*>(p, "MotionVectors"); rec.output = Read<ID3D12Resource*>(p, "Output");
        ReadPresets(p, rec.presets);
    }
    std::lock_guard<std::mutex> lock(g_mutex);
    if (isCRoute) ++g_state.evaluateCCount;
    if (g_state.failNextEvaluate || !p || !IsAliveLocked(handle)) {
        g_state.failNextEvaluate = false; rec.result = 0xBAD00005u;
    } else {
        ClearOutputUav(list, rec.output, handle->id); rec.result = 1u;
    }
    ++g_state.evaluateCount;
    if (g_state.evaluateRecordCount < MockNgxState::kMaxRecords) g_state.evaluates[g_state.evaluateRecordCount++] = rec;
    return rec.result;
}
}  // namespace

extern "C" {
MOCK_NVNGX_API __declspec(noinline) Result NVSDK_NGX_D3D12_CreateFeature(
    ID3D12GraphicsCommandList* list, unsigned feature, Parameters* p, Handle** outHandle) {
    (void)list;
    MockNgxCreateRecord rec{}; rec.feature = feature;
    if (p) {
        rec.width = Integer(p, "Width", Integer(p, "DLSS.Render.Subrect.Dimensions.Width"));
        rec.height = Integer(p, "Height", Integer(p, "DLSS.Render.Subrect.Dimensions.Height"));
        rec.outWidth = Integer(p, "OutWidth"); rec.outHeight = Integer(p, "OutHeight");
        rec.perfQualityValue = Integer(p, "PerfQualityValue"); rec.createFlags = Integer(p, "DLSS.Feature.Create.Flags");
        ReadPresets(p, rec.presets);
    }
    std::lock_guard<std::mutex> lock(g_mutex);
    if (outHandle) *outHandle = nullptr;
    if (!outHandle || !p || g_state.failNextCreate) {
        g_state.failNextCreate = false; rec.result = 0xBAD00005u;
    } else {
        auto* created = new Handle{g_state.nextHandleId++};
        g_aliveHandles.push_back(created); g_state.aliveHandles = g_aliveHandles.size();
        rec.handle = created; rec.result = 1u; *outHandle = created;
    }
    ++g_state.createCount;
    if (g_state.createRecordCount < MockNgxState::kMaxRecords) g_state.creates[g_state.createRecordCount++] = rec;
    return rec.result;
}

MOCK_NVNGX_API __declspec(noinline) Result NVSDK_NGX_D3D12_EvaluateFeature(
    ID3D12GraphicsCommandList* list, const Handle* handle, const Parameters* p, void* callback) {
    // Distinct non-trivial prologue so MinHook/FarInlineHook hooks cleanly and /OPT:ICF cannot fold routes.
    volatile uint64_t routeTag = 0x1111u;
    if (!handle && !p && routeTag == 0u) return 0xBAD00005u;
    return EvaluateFeatureImpl(list, handle, p, callback, false);
}

MOCK_NVNGX_API __declspec(noinline) Result NVSDK_NGX_D3D12_EvaluateFeature_C(
    ID3D12GraphicsCommandList* list, const Handle* handle, const Parameters* p, void* callback) {
    volatile uint64_t routeTag = 0x2222u;
    if (!handle && !p && routeTag == 0u) return 0xBAD00006u;
    return EvaluateFeatureImpl(list, handle, p, callback, true);
}

MOCK_NVNGX_API __declspec(noinline) Result NVSDK_NGX_D3D12_ReleaseFeature(Handle* handle) {
    std::lock_guard<std::mutex> lock(g_mutex);
    ++g_state.releaseCount;
    if (g_state.releaseRecordCount < MockNgxState::kMaxRecords) g_state.releasedHandles[g_state.releaseRecordCount++] = handle;
    const auto it = handle ? std::find(g_aliveHandles.begin(), g_aliveHandles.end(), handle) : g_aliveHandles.end();
    if (it == g_aliveHandles.end()) return 0xBAD00005u;
    g_aliveHandles.erase(it); delete handle; g_state.aliveHandles = g_aliveHandles.size();
    return 1u;
}

MOCK_NVNGX_API void MockNgxReset() {
    std::lock_guard<std::mutex> lock(g_mutex);
    for (Handle* h : g_aliveHandles) delete h;
    g_aliveHandles.clear(); g_keptHeaps.clear(); g_state = MockNgxState{};
}
MOCK_NVNGX_API bool MockNgxReadState(MockNgxState* out) {
    if (!out) return false; std::lock_guard<std::mutex> lock(g_mutex); *out = g_state; return true;
}
MOCK_NVNGX_API void MockNgxSetFailNextEvaluate(bool fail) { std::lock_guard<std::mutex> lock(g_mutex); g_state.failNextEvaluate = fail; }
MOCK_NVNGX_API void MockNgxSetFailNextCreate(bool fail) { std::lock_guard<std::mutex> lock(g_mutex); g_state.failNextCreate = fail; }
MOCK_NVNGX_API bool MockNgxIsHandleAlive(const Handle* handle) { std::lock_guard<std::mutex> lock(g_mutex); return IsAliveLocked(handle); }
MOCK_NVNGX_API bool MockNgxGetHandleClearColor(const Handle* handle, float outRgba[4]) {
    if (!outRgba) return false; std::lock_guard<std::mutex> lock(g_mutex);
    if (!IsAliveLocked(handle)) return false; ComputeClearColor(handle->id, outRgba); return true;
}
}
