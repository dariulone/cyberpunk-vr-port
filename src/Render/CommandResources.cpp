#include "Render/CommandResources.hpp"
#include <wrl/client.h>
#include <algorithm>
#include <atomic>
#include <deque>
#include <mutex>
#include <new>
#include <unordered_map>

#if !CVR_RUST_VRIK
// The C++ build's tracker; the Rust build's is rust/crates/cvr-gpu/src/command_resources.rs (the header calls it).
namespace cvr::gpu {
using Microsoft::WRL::ComPtr;
struct ResourceBatch { std::vector<ComPtr<IUnknown>> resources; };
namespace {
// The list owns a Recording, but GPU submissions own only its immutable batch.
// Destroying/recycling the list therefore expires its weak registry entry even
// when an older submission still holds the resources alive.
struct Recording { std::shared_ptr<const ResourceBatch> batch; };
constexpr GUID recordingKey{0x29c6b412,0xb5a9,0x4b64,{0x9f,0x1c,0x40,0x3a,0x97,0xae,0x07,0x32}};
class RecordingOwner final : public IUnknown {
    std::atomic<ULONG> refs{1};
public:
    std::shared_ptr<Recording> recording;
    explicit RecordingOwner(std::shared_ptr<Recording> value) : recording(std::move(value)) {}
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id, void** out) override {
        if (!out) return E_POINTER;
        *out = nullptr;
        if (id != __uuidof(IUnknown)) return E_NOINTERFACE;
        *out = static_cast<IUnknown*>(this); AddRef(); return S_OK;
    }
    ULONG STDMETHODCALLTYPE AddRef() override { return ++refs; }
    ULONG STDMETHODCALLTYPE Release() override { const auto n = --refs; if (!n) delete this; return n; }
};
struct Flight { uint64_t value{}; ResourceSubmission submission; };
struct QueueState {
    ComPtr<ID3D12CommandQueue> queue;
    ComPtr<ID3D12Fence> fence;
    uint64_t value{};
    std::deque<Flight> flights;
};
std::mutex mutex;
std::unordered_map<ID3D12CommandList*, std::weak_ptr<Recording>> recordings;
std::unordered_map<ID3D12CommandQueue*, QueueState> queues;
std::vector<ResourceSubmission> failedSubmissions;
std::atomic<bool> haveRecordings{}, haveFlights{}, failed{};
void PruneRecordings() {
    for (auto it = recordings.begin(); it != recordings.end();)
        if (it->second.expired()) it = recordings.erase(it); else ++it;
    haveRecordings.store(!recordings.empty(), std::memory_order_release);
}
void CollectLocked() {
    bool pending = false;
    for (auto& [_, state] : queues) {
        const auto done = state.fence ? state.fence->GetCompletedValue() : 0;
        while (!state.flights.empty() && state.flights.front().value <= done) state.flights.pop_front();
        pending |= !state.flights.empty();
    }
    haveFlights.store(pending, std::memory_order_release);
}
}

bool KeepCommandResources(ID3D12GraphicsCommandList* list, std::initializer_list<IUnknown*> objects) {
    if (!list || failed.load(std::memory_order_relaxed)) return false;
    try {
        std::lock_guard lock(mutex);
        PruneRecordings();
        auto batch = std::make_shared<ResourceBatch>();
        const auto previous = recordings.find(list);
        if (previous != recordings.end()) if (auto current = previous->second.lock()) batch->resources = current->batch->resources;
        for (auto* object : objects) {
            if (!object) return false;
            if (std::none_of(batch->resources.begin(), batch->resources.end(), [object](const auto& p) { return p.Get() == object; }))
                batch->resources.emplace_back(object);
        }
        if (batch->resources.empty()) return false;
        auto recording = std::make_shared<Recording>(); recording->batch = std::move(batch);
        ComPtr<RecordingOwner> owner; owner.Attach(new RecordingOwner(recording));
        const auto [entry, inserted] = recordings.try_emplace(list);
        const auto previousRecording = entry->second;
        entry->second = recording;
        const HRESULT hr = list->SetPrivateDataInterface(recordingKey, owner.Get());
        if (FAILED(hr)) {
            if (inserted) recordings.erase(entry); else entry->second = previousRecording;
            return false;
        }
        haveRecordings.store(true, std::memory_order_release);
        return true;
    } catch (const std::bad_alloc&) { return false; }
}

ResourceSubmission PrepareCommandResources(UINT count, ID3D12CommandList* const* lists) {
    ResourceSubmission result;
    if (!lists || !haveRecordings.load(std::memory_order_acquire)) return result;
    std::lock_guard lock(mutex);
    for (UINT i = 0; i < count; ++i) {
        const auto it = recordings.find(lists[i]);
        if (it == recordings.end()) continue;
        if (auto recording = it->second.lock()) result.batches.push_back(recording->batch);
    }
    return result;
}

bool HasCommandResource(ID3D12GraphicsCommandList* list,IUnknown* object) {
    if(!list || !object || !haveRecordings.load(std::memory_order_acquire))return false;
    std::lock_guard lock(mutex);const auto found=recordings.find(list);if(found==recordings.end())return false;
    const auto recording=found->second.lock();
    return recording && std::any_of(recording->batch->resources.begin(),recording->batch->resources.end(),
        [object](const auto& value){return value.Get()==object;});
}

void SubmitCommandResources(ID3D12CommandQueue* queue, ResourceSubmission submission) {
    if (submission.batches.empty()) return;
    std::lock_guard lock(mutex);
    CollectLocked();
    auto& state = queues[queue];
    if (!state.fence && queue) {
        ComPtr<ID3D12Device> device;
        if (SUCCEEDED(queue->GetDevice(IID_PPV_ARGS(&device))) &&
            SUCCEEDED(device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&state.fence)))) {
            state.queue = queue;
            state.fence->SetName(L"CVR_command_resource_lifetime");
        }
    }
    if (!state.fence || FAILED(queue->Signal(state.fence.Get(), state.value + 1))) {
        // The commands were already submitted. Keep their objects alive if a
        // removed device/failed fence prevents proving completion; stop new work.
        failedSubmissions.push_back(std::move(submission));
        failed.store(true, std::memory_order_relaxed);
        return;
    }
    state.flights.push_back({++state.value, std::move(submission)});
    haveFlights.store(true, std::memory_order_release);
}

void ResetCommandResources(ID3D12GraphicsCommandList* list) {
    if (!list || !haveRecordings.load(std::memory_order_acquire)) return;
    bool tracked = false;
    {
        std::lock_guard lock(mutex);
        tracked = recordings.erase(list) != 0;
        haveRecordings.store(!recordings.empty(), std::memory_order_release);
    }
    if (tracked) list->SetPrivateDataInterface(recordingKey, nullptr);
}

void CollectCommandResources() {
    if (!haveFlights.load(std::memory_order_acquire) && !haveRecordings.load(std::memory_order_acquire)) return;
    std::lock_guard lock(mutex);
    CollectLocked();
    PruneRecordings();
}
}
#endif
