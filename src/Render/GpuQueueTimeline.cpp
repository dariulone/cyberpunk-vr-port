#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
// GpuQueueTimeline -- diagnostic: when each queue of the game actually runs its batches.
//
// nvidia-smi says how much of the time the GPU is busy, not where it waits. Asked for through
// CyberpunkVR_GpuTimelineRequest (a number of submissions), the next ExecuteCommandLists calls on the
// direct and compute queues get two pre-recorded command lists added to the same call, each writing a
// GPU timestamp: one before the game's lists, one after. With the CPU time of each call, the queues'
// clock calibration and the Present times, a tool can tell for every gap on a queue whether the GPU had
// nothing to run (the batch was submitted after the queue went idle) or was held by a fence.
//
// Nothing is installed in a normal run: the lists are created on the Present after the first request
// (a one-off stall), and the hook does nothing while no capture is on.
// Read by tools/hitch_tests/gpu_queue_timeline.py.

#include "Render/GpuQueueTimeline.hpp"
#include "Camera/ImagePoseIdentity.hpp"
#include <wrl/client.h>
#include <atomic>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <vector>
#include <windows.h>

#if !CVR_RUST_VRIK
extern void Log(const char* fmt, ...);
#endif
#if !CVR_RUST_VRIK
extern std::atomic<void*> g_presentQueue;   // Hooks/SwapChain.cpp
#endif

#if !CVR_RUST_VRIK
extern "C" {
#endif
// Write a number of submissions to capture; 0 when taken.
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/used_moved_exports_rest.rs.
#else
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_GpuTimelineRequest{0};
// 0 idle, 1 capturing, 2 waiting for the GPU, 3 done (records valid), 4 failed.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_GpuTimelineState{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_GpuTimelineCount{0};
// Per queue type (0 direct, 1 compute): [t*3] timestamp frequency, [t*3+1] GPU timestamp and [t*3+2] QPC
// of one calibration; [6] QPC frequency.
__declspec(dllexport) uint64_t CyberpunkVR_GpuTimelineCalib[8]{};
// Per record, 6 words: kind (0 direct, 1 compute, 2 Present) | lists << 8, queue pointer, QPC when the
// call came in, QPC when the original returned, GPU timestamp before the lists, GPU timestamp after them.
// Kinds 3 (queue Signal) and 4 (queue Wait): queue pointer, QPC, fence pointer, fence value, 0.
__declspec(dllexport) uint64_t CyberpunkVR_GpuTimelineRecords[16384 * 6]{};
#endif
#if !CVR_RUST_VRIK
}
#endif

#if CVR_RUST_VRIK
// The Rust build: the timeline is rust/crates/cvr-gpu/src/queue_timeline.rs; the exports stay above, attached at
// load with the image submission mutex and the swap chain's queue.
// The timeline's table is built in rust/crates/cvr-gpu/src/queue_timeline.rs.
#else
namespace cvr::gpu::queue_timeline {
namespace {
using Microsoft::WRL::ComPtr;
constexpr uint32_t kRecords = 16384;
constexpr uint32_t kPairs = 4096;   // timed submissions per queue type
constexpr uint32_t kQueues = 8;     // distinct queues per type

struct TypeState {
    ComPtr<ID3D12QueryHeap> heap;
    ComPtr<ID3D12Resource> readback;
    ComPtr<ID3D12CommandAllocator> allocator;
    std::vector<ComPtr<ID3D12GraphicsCommandList>> lists;   // 2 per pair: before, after
    ComPtr<ID3D12GraphicsCommandList> resolve;
    ID3D12CommandQueue* queues[kQueues]{};                    // queues that ran timed submissions
    void* callers[kQueues][4]{};                              // first code outside this DLL that submitted
    ComPtr<ID3D12Fence> queueFences[kQueues];
    uint64_t queueFenceValue[kQueues]{};
    ComPtr<ID3D12Fence> done;
    uint64_t doneValue = 0;
    uint32_t used = 0;
    uint32_t next = 0;
    bool calibrated = false;
};
TypeState g_types[2];
// Records, slots and the capture count move under cvr::camera::ImageSubmissionMutex(): the inner
// submission hook takes it too, and the XR submit holds it around its own ExecuteCommandLists, so a
// mutex of our own taken first would deadlock against it.
std::atomic<bool> g_ready{false};
std::atomic<uint32_t> g_capture{0};   // submissions still to time
std::vector<uint32_t> g_slot(kRecords, 0);   // record -> pair index

uint64_t Qpc() { LARGE_INTEGER t; QueryPerformanceCounter(&t); return uint64_t(t.QuadPart); }

// Queue Signal / Wait, recorded while a capture runs: which fence holds a held batch, and when its
// signal was issued. Patched into the queue vtables on the first request; outside a capture the hooks
// only forward.
using SignalFn = HRESULT(STDMETHODCALLTYPE*)(ID3D12CommandQueue*, ID3D12Fence*, UINT64);
struct SyncHook { void** table; SignalFn signal; SignalFn wait; };
SyncHook g_syncHooks[4]{};
std::atomic<uint32_t> g_syncHookCount{0};

const SyncHook* SyncHookOf(ID3D12CommandQueue* queue) {
    void** table = *reinterpret_cast<void***>(queue);
    for (uint32_t i = 0, n = g_syncHookCount.load(std::memory_order_acquire); i < n; ++i)
        if (g_syncHooks[i].table == table) return &g_syncHooks[i];
    return nullptr;
}

void RecordSync(uint64_t kind, ID3D12CommandQueue* queue, ID3D12Fence* fence, UINT64 value) {
    if (!g_capture.load(std::memory_order_relaxed)) return;
    const uint32_t record = CyberpunkVR_GpuTimelineCount.fetch_add(1);
    if (record >= kRecords) { CyberpunkVR_GpuTimelineCount.store(kRecords); return; }
    uint64_t* r = &CyberpunkVR_GpuTimelineRecords[record * 6];
    r[0] = kind; r[1] = reinterpret_cast<uint64_t>(queue); r[2] = Qpc();
    r[3] = reinterpret_cast<uint64_t>(fence); r[4] = value; r[5] = 0;
}

HRESULT STDMETHODCALLTYPE TimelineSignal(ID3D12CommandQueue* queue, ID3D12Fence* fence, UINT64 value) {
    const auto* hook = SyncHookOf(queue);
    RecordSync(3, queue, fence, value);
    return hook ? hook->signal(queue, fence, value) : E_FAIL;
}

HRESULT STDMETHODCALLTYPE TimelineWait(ID3D12CommandQueue* queue, ID3D12Fence* fence, UINT64 value) {
    const auto* hook = SyncHookOf(queue);
    RecordSync(4, queue, fence, value);
    return hook ? hook->wait(queue, fence, value) : E_FAIL;
}

void HookSync(ID3D12CommandQueue* queue) {
    if (!queue || SyncHookOf(queue)) return;
    const uint32_t n = g_syncHookCount.load();
    if (n >= 4) return;
    void** table = *reinterpret_cast<void***>(queue);
    DWORD old = 0;
    if (!VirtualProtect(&table[14], 2 * sizeof(void*), PAGE_EXECUTE_READWRITE, &old)) return;
    g_syncHooks[n] = SyncHook{table, reinterpret_cast<SignalFn>(table[14]), reinterpret_cast<SignalFn>(table[15])};
    g_syncHookCount.store(n + 1, std::memory_order_release);
    table[14] = reinterpret_cast<void*>(&TimelineSignal);
    table[15] = reinterpret_cast<void*>(&TimelineWait);
    VirtualProtect(&table[14], 2 * sizeof(void*), old, &old);
}

HMODULE ModuleOf(const void* address) {
    HMODULE module = nullptr;
    GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                       static_cast<LPCWSTR>(address), &module);
    return module;
}

// Who submits on each queue: the first return address outside this DLL and the D3D12 runtime.
void NoteCaller(TypeState& s, uint32_t q) {
    static const HMODULE self = ModuleOf(reinterpret_cast<const void*>(&ModuleOf));
    static const HMODULE d3d12 = GetModuleHandleW(L"d3d12.dll");
    void* frames[16]{};
    const USHORT n = RtlCaptureStackBackTrace(1, 16, frames, nullptr);
    void* caller = nullptr;
    for (USHORT i = 0; i < n && !caller; ++i) {
        const HMODULE m = ModuleOf(frames[i]);
        if (m && m != self && m != d3d12) caller = frames[i];
    }
    if (!caller) caller = n ? frames[n - 1] : nullptr;
    for (auto& slot : s.callers[q]) {
        if (slot == caller) return;
        if (!slot) { slot = caller; return; }
    }
}

void LogCallers() {
    for (int t = 0; t < 2; ++t) {
        auto& s = g_types[t];
        for (uint32_t q = 0; q < s.used; ++q) {
            const auto desc = s.queues[q]->GetDesc();
            char line[512];
            int len = snprintf(line, sizeof(line), "[gpu-timeline] %s queue %p priority %d flags %u:", t ? "compute" : "direct",
                               static_cast<void*>(s.queues[q]), desc.Priority, unsigned(desc.Flags));
            for (void* caller : s.callers[q]) {
                if (!caller || len >= int(sizeof(line)) - 80) continue;
                const HMODULE m = ModuleOf(caller);
                char name[MAX_PATH] = "?";
                if (m) GetModuleFileNameA(m, name, MAX_PATH);
                const char* base = strrchr(name, '\\');
                len += snprintf(line + len, sizeof(line) - len, " %s+%#llx", base ? base + 1 : name,
                                static_cast<unsigned long long>(reinterpret_cast<uintptr_t>(caller) - reinterpret_cast<uintptr_t>(m)));
            }
            Log("%s\n", line);
        }
    }
}

int TypeIndex(ID3D12CommandQueue* queue) {
    const auto type = queue->GetDesc().Type;
    if (type == D3D12_COMMAND_LIST_TYPE_DIRECT) return 0;
    if (type == D3D12_COMMAND_LIST_TYPE_COMPUTE) return 1;
    return -1;
}

bool Prepare(ID3D12Device* device) {
    for (int t = 0; t < 2; ++t) {
        auto& s = g_types[t];
        const auto type = t == 0 ? D3D12_COMMAND_LIST_TYPE_DIRECT : D3D12_COMMAND_LIST_TYPE_COMPUTE;
        D3D12_QUERY_HEAP_DESC hd{}; hd.Type = D3D12_QUERY_HEAP_TYPE_TIMESTAMP; hd.Count = kPairs * 2;
        D3D12_RESOURCE_DESC rd{}; rd.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER; rd.Width = uint64_t(kPairs) * 2 * 8;
        rd.Height = rd.DepthOrArraySize = rd.MipLevels = 1; rd.SampleDesc.Count = 1; rd.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
        D3D12_HEAP_PROPERTIES hp{}; hp.Type = D3D12_HEAP_TYPE_READBACK;
        if (FAILED(device->CreateQueryHeap(&hd, IID_PPV_ARGS(&s.heap))) ||
            FAILED(device->CreateCommittedResource(&hp, D3D12_HEAP_FLAG_NONE, &rd, D3D12_RESOURCE_STATE_COPY_DEST, nullptr, IID_PPV_ARGS(&s.readback))) ||
            FAILED(device->CreateCommandAllocator(type, IID_PPV_ARGS(&s.allocator))) ||
            FAILED(device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&s.done))))
            return false;
        for (auto& fence : s.queueFences)
            if (FAILED(device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fence)))) return false;
        s.lists.resize(kPairs * 2);
        for (uint32_t i = 0; i < kPairs * 2; ++i) {
            if (FAILED(device->CreateCommandList(0, type, s.allocator.Get(), nullptr, IID_PPV_ARGS(&s.lists[i])))) return false;
            s.lists[i]->EndQuery(s.heap.Get(), D3D12_QUERY_TYPE_TIMESTAMP, i);
            if (FAILED(s.lists[i]->Close())) return false;
        }
        if (FAILED(device->CreateCommandList(0, type, s.allocator.Get(), nullptr, IID_PPV_ARGS(&s.resolve)))) return false;
        s.resolve->ResolveQueryData(s.heap.Get(), D3D12_QUERY_TYPE_TIMESTAMP, 0, kPairs * 2, s.readback.Get(), 0);
        if (FAILED(s.resolve->Close())) return false;
    }
    return true;
}

// Under the submission mutex, capture over: the first queue of each type waits for the others and resolves.
void Finish(ExecuteFn original) {
    for (auto& s : g_types) {
        if (!s.used) continue;
        ID3D12CommandQueue* resolver = s.queues[0];
        for (uint32_t q = 1; q < s.used; ++q) {
            s.queues[q]->Signal(s.queueFences[q].Get(), ++s.queueFenceValue[q]);
            resolver->Wait(s.queueFences[q].Get(), s.queueFenceValue[q]);
        }
        ID3D12CommandList* list = s.resolve.Get();
        original(resolver, 1, &list);
        resolver->Signal(s.done.Get(), ++s.doneValue);
    }
    CyberpunkVR_GpuTimelineState.store(2);
}
}

bool Execute(ID3D12CommandQueue* queue, UINT count, ID3D12CommandList* const* lists, ExecuteFn original) {
    if (!g_capture.load(std::memory_order_relaxed) || !queue || !original || !lists || !count) return false;
    const int t = TypeIndex(queue);
    if (t < 0) return false;
    auto& s = g_types[t];
    std::lock_guard lock(cvr::camera::ImageSubmissionMutex());
    if (!g_capture.load(std::memory_order_relaxed)) return false;
    uint32_t q = 0;
    while (q < s.used && s.queues[q] != queue) ++q;
    if (s.next >= kPairs || CyberpunkVR_GpuTimelineCount.load() + 8 >= kRecords || (q == s.used && q == kQueues)) {
        g_capture.store(0);
        Finish(original);
        return false;
    }
    if (q == s.used) { s.queues[s.used++] = queue; HookSync(queue); }
    NoteCaller(s, q);
    if (!s.calibrated) {
        UINT64 frequency = 0, gpu = 0, cpu = 0;
        if (SUCCEEDED(queue->GetTimestampFrequency(&frequency)) && SUCCEEDED(queue->GetClockCalibration(&gpu, &cpu))) {
            CyberpunkVR_GpuTimelineCalib[t * 3 + 0] = frequency;
            CyberpunkVR_GpuTimelineCalib[t * 3 + 1] = gpu;
            CyberpunkVR_GpuTimelineCalib[t * 3 + 2] = cpu;
        }
        s.calibrated = true;
    }
    const uint32_t pair = s.next++;
    // One call as the game made it, with the two stamps around its lists.
    ID3D12CommandList* local[66];
    std::vector<ID3D12CommandList*> heap;
    ID3D12CommandList** all = local;
    if (count + 2 > 66) { heap.resize(count + 2); all = heap.data(); }
    all[0] = s.lists[pair * 2].Get();
    for (UINT i = 0; i < count; ++i) all[i + 1] = lists[i];
    all[count + 1] = s.lists[pair * 2 + 1].Get();
    const uint64_t call = Qpc();
    original(queue, count + 2, all);
    const uint64_t ret = Qpc();
    const uint32_t record = CyberpunkVR_GpuTimelineCount.fetch_add(1);
    if (record < kRecords) {
        uint64_t* r = &CyberpunkVR_GpuTimelineRecords[record * 6];
        r[0] = uint64_t(t) | (uint64_t(count) << 8);
        r[1] = reinterpret_cast<uint64_t>(queue);
        r[2] = call;
        r[3] = ret;
        r[4] = r[5] = 0;
        g_slot[record] = pair;
    }
    if (g_capture.fetch_sub(1) == 1) Finish(original);
    return true;
}

void Present() {
    if (g_capture.load(std::memory_order_relaxed)) {
        std::lock_guard lock(cvr::camera::ImageSubmissionMutex());
        if (g_capture.load(std::memory_order_relaxed)) {
            const uint32_t record = CyberpunkVR_GpuTimelineCount.fetch_add(1);
            if (record < kRecords) {
                uint64_t* r = &CyberpunkVR_GpuTimelineRecords[record * 6];
                r[0] = 2; r[1] = 0; r[2] = r[3] = Qpc(); r[4] = r[5] = 0;
                g_slot[record] = 0;
            }
        }
        return;
    }
    const uint32_t state = CyberpunkVR_GpuTimelineState.load();
    if (state == 1) return;   // the last submission is in its Finish
    if (state == 2) {
        for (auto& s : g_types)
            if (s.used && s.done->GetCompletedValue() < s.doneValue) return;
        // Fill the GPU timestamps from the readbacks.
        uint32_t count = CyberpunkVR_GpuTimelineCount.load();
        if (count > kRecords) { count = kRecords; CyberpunkVR_GpuTimelineCount.store(count); }
        const uint64_t* stamps[2] = {};
        for (int t = 0; t < 2; ++t) {
            void* data = nullptr; D3D12_RANGE range{0, size_t(kPairs) * 2 * 8};
            if (g_types[t].used && SUCCEEDED(g_types[t].readback->Map(0, &range, &data))) stamps[t] = static_cast<const uint64_t*>(data);
        }
        for (uint32_t i = 0; i < count; ++i) {
            uint64_t* r = &CyberpunkVR_GpuTimelineRecords[i * 6];
            const uint32_t kind = uint32_t(r[0] & 0xFF);
            if (kind > 1 || !stamps[kind]) continue;
            r[4] = stamps[kind][g_slot[i] * 2];
            r[5] = stamps[kind][g_slot[i] * 2 + 1];
        }
        for (int t = 0; t < 2; ++t) if (stamps[t]) { D3D12_RANGE none{}; g_types[t].readback->Unmap(0, &none); }
        CyberpunkVR_GpuTimelineState.store(3);
        Log("[gpu-timeline] %u records captured (direct queues %u, compute queues %u)\n", count, g_types[0].used, g_types[1].used);
        LogCallers();
        return;
    }
    const uint32_t want = CyberpunkVR_GpuTimelineRequest.exchange(0);
    if (!want) return;
    if (!g_ready.load()) {
        ComPtr<ID3D12Device> device;
        auto* queue = static_cast<ID3D12CommandQueue*>(g_presentQueue.load());
        if (!queue || FAILED(queue->GetDevice(IID_PPV_ARGS(&device))) || !Prepare(device.Get())) {
            CyberpunkVR_GpuTimelineState.store(4);
            Log("[gpu-timeline] could not create the timestamp lists\n");
            return;
        }
        g_ready.store(true);
    }
    HookSync(static_cast<ID3D12CommandQueue*>(g_presentQueue.load()));
    LARGE_INTEGER f; QueryPerformanceFrequency(&f); CyberpunkVR_GpuTimelineCalib[6] = uint64_t(f.QuadPart);
    std::lock_guard lock(cvr::camera::ImageSubmissionMutex());
    for (auto& s : g_types) {
        s.next = 0; s.used = 0; s.calibrated = false;
        for (auto& row : s.callers) for (auto& c : row) c = nullptr;
    }
    CyberpunkVR_GpuTimelineCount.store(0);
    CyberpunkVR_GpuTimelineState.store(1);
    g_capture.store(want);
}
}
#endif
#endif
