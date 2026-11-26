#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
#include "Render/GpuStageProfile.hpp"
#include "Render/CommandResources.hpp"
#include <wrl/client.h>
#include <algorithm>
#include <array>
#include <atomic>
#include <cstdio>
#include <mutex>
#include <vector>
#if CVR_RUST_VRIK
// The exports' tables are attached by rust/crates/cvr-render/src/export_attach.rs as the plugin loads;
// nothing of this file remains in the Rust build.
#else

extern "C" {
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_GpuStageRequest{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_GpuStageReportSeq{0};
__declspec(dllexport) char CyberpunkVR_GpuStageReport[2048] = "{\"status\":\"idle\"}";
}

namespace cvr::gpu::profile {
using Microsoft::WRL::ComPtr;
namespace {
constexpr unsigned MaxPairs = 16;
std::atomic<unsigned> budget{0}, outstanding{0}, cancelled{0};
std::atomic<bool> active{false};
std::mutex mutex;
struct Statistic { unsigned count{}; double total{}, peak{}; };
std::array<Statistic, static_cast<unsigned>(Stage::Count)> statistics;
unsigned completed{}, invalid{};
constexpr const char* names[] = {"CaptureMain", "CaptureSecond", "CaptureDepth", "SubmitColor", "SubmitDepth",
    "SecondHud", "SecondBlit", "HudSprites", "HudBlur", "HudStyle", "HudSubmit"};
static_assert(std::size(names) == static_cast<unsigned>(Stage::Count));
}
struct Batch {
    ID3D12GraphicsCommandList* list{}; // used only during recording
    ComPtr<ID3D12QueryHeap> heap;
    ComPtr<ID3D12Resource> readback;
    ComPtr<ID3D12Fence> fence;
    uint64_t value{}, frequency{};
    struct Pair { Stage stage{}; bool ended{}; };
    std::array<Pair, MaxPairs> pairs{};
    unsigned used{};
    bool counted{}, submitted{};
    ~Batch() {
        if (counted) {
            if (!submitted) cancelled.fetch_add(1, std::memory_order_relaxed);
            outstanding.fetch_sub(1, std::memory_order_release);
        }
    }
};
namespace {
std::vector<BatchPtr> pending;
void Publish() {
    CyberpunkVR_GpuStageReportSeq.fetch_add(1, std::memory_order_acq_rel);
    auto* out = CyberpunkVR_GpuStageReport;
    size_t at = std::snprintf(out, sizeof(CyberpunkVR_GpuStageReport),
        "{\"pending\":%s,\"remaining\":%u,\"outstanding\":%u,\"batches\":%u,\"cancelled\":%u,\"invalid\":%u,\"stages\":[",
        budget.load() || outstanding.load() ? "true" : "false", budget.load(), outstanding.load(),
        completed, cancelled.load(), invalid);
    for (unsigned i = 0; i < statistics.size(); ++i) {
        const auto& s = statistics[i];
        at += std::snprintf(out + at, sizeof(CyberpunkVR_GpuStageReport) - at,
            "%s{\"name\":\"%s\",\"count\":%u,\"mean_us\":%.3f,\"max_us\":%.3f}",
            i ? "," : "", names[i], s.count, s.count ? s.total / s.count : 0, s.peak);
    }
    std::snprintf(out + at, sizeof(CyberpunkVR_GpuStageReport) - at, "]}");
    CyberpunkVR_GpuStageReportSeq.fetch_add(1, std::memory_order_release);
}
}

BatchPtr Begin(ID3D12GraphicsCommandList* list) {
    if (!budget.load(std::memory_order_relaxed) || !list) return {};
    auto batch = std::make_shared<Batch>();
    {
        std::lock_guard lock(mutex);
        if (!budget.load(std::memory_order_relaxed)) return {};
        budget.fetch_sub(1, std::memory_order_relaxed);
        outstanding.fetch_add(1, std::memory_order_relaxed);
        batch->counted = true;
    }
    // Each bounded sample owns its queries; neither a list reset nor another
    // recording can overwrite an in-flight timestamp/readback slot.
    ComPtr<ID3D12Device> device;
    if (list->GetType() != D3D12_COMMAND_LIST_TYPE_DIRECT || FAILED(list->GetDevice(IID_PPV_ARGS(&device)))) return {};
    D3D12_QUERY_HEAP_DESC query{};
    query.Type = D3D12_QUERY_HEAP_TYPE_TIMESTAMP;
    query.Count = MaxPairs * 2;
    D3D12_RESOURCE_DESC desc{};
    desc.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
    desc.Width = MaxPairs * 2 * sizeof(uint64_t);
    desc.Height = desc.DepthOrArraySize = desc.MipLevels = 1;
    desc.SampleDesc.Count = 1;
    desc.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    D3D12_HEAP_PROPERTIES properties{};
    properties.Type = D3D12_HEAP_TYPE_READBACK;
    if (FAILED(device->CreateQueryHeap(&query, IID_PPV_ARGS(&batch->heap))) ||
        FAILED(device->CreateCommittedResource(&properties, D3D12_HEAP_FLAG_NONE, &desc,
            D3D12_RESOURCE_STATE_COPY_DEST, nullptr, IID_PPV_ARGS(&batch->readback)))) return {};
    // This also covers aborted recordings and possible command-list replay.
    if (!KeepCommandResources(list, {batch->heap.Get(), batch->readback.Get()})) return {};
    batch->list = list;
    return batch;
}

Scope::Scope(const BatchPtr& value, Stage stage) : batch(value.get()) {
    if (!batch || batch->used == MaxPairs || static_cast<unsigned>(stage) >= statistics.size()) return;
    index = batch->used++;
    batch->pairs[index].stage = stage;
    batch->list->EndQuery(batch->heap.Get(), D3D12_QUERY_TYPE_TIMESTAMP, index * 2);
}
Scope::~Scope() {
    if (index == ~0u) return;
    batch->list->EndQuery(batch->heap.Get(), D3D12_QUERY_TYPE_TIMESTAMP, index * 2 + 1);
    batch->list->ResolveQueryData(batch->heap.Get(), D3D12_QUERY_TYPE_TIMESTAMP,
        index * 2, 2, batch->readback.Get(), index * 2 * sizeof(uint64_t));
    batch->pairs[index].ended = true;
}

void Submitted(BatchPtr batch, ID3D12CommandQueue* queue, ID3D12Fence* fence, UINT64 value) {
    if (!batch || batch->submitted || !batch->used || !queue || !fence || !value) return;
    if (FAILED(queue->GetTimestampFrequency(&batch->frequency)) || !batch->frequency) return;
    batch->fence = fence;
    batch->value = value;
    batch->submitted = true;
    std::lock_guard lock(mutex);
    pending.push_back(std::move(batch));
}

void Poll() {
    if (!active.load(std::memory_order_relaxed) && !CyberpunkVR_GpuStageRequest.load(std::memory_order_relaxed)) return;
    std::lock_guard lock(mutex);
    bool changed = false;
    for (auto it = pending.begin(); it != pending.end();) {
        const auto& b = **it;
        const auto done = b.fence->GetCompletedValue();
        if (done < b.value) { ++it; continue; }
        void* mapped{};
        D3D12_RANGE range{0, b.used * 2 * sizeof(uint64_t)};
        if (done != UINT64_MAX && SUCCEEDED(b.readback->Map(0, &range, &mapped))) {
            const auto* ticks = static_cast<const uint64_t*>(mapped);
            for (unsigned i = 0; i < b.used; ++i) {
                if (!b.pairs[i].ended || ticks[i * 2 + 1] < ticks[i * 2]) { ++invalid; continue; }
                const double us = double(ticks[i * 2 + 1] - ticks[i * 2]) * 1e6 / double(b.frequency);
                auto& s = statistics[static_cast<unsigned>(b.pairs[i].stage)];
                ++s.count; s.total += us; s.peak = std::max(s.peak, us);
            }
            D3D12_RANGE none{};
            b.readback->Unmap(0, &none);
            ++completed;
        } else ++invalid;
        it = pending.erase(it);
        changed = true;
    }
    const bool idle = budget.load() == 0 && outstanding.load(std::memory_order_acquire) == 0;
    if (idle) {
        const auto request = CyberpunkVR_GpuStageRequest.exchange(0, std::memory_order_acq_rel);
        if (request) {
            statistics = {}; completed = invalid = 0; cancelled.store(0);
            budget.store(std::min(request, 32u), std::memory_order_release);
            active.store(true, std::memory_order_relaxed);
            changed = true;
        } else if (active.exchange(false, std::memory_order_relaxed)) changed = true;
    }
    if (changed) Publish();
}
}
#endif
#endif
