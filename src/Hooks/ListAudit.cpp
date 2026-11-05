// ListAudit -- see include/Hooks/ListAudit.hpp. Diagnostic only: nothing here runs unless CVR_GPU_DIAG asks
// for "lists", and then every call goes through one mutex.
#include "Hooks/ListAudit.hpp"

#include <MinHook.h>
#include <windows.h>
#include <wrl.h>
#include <atomic>
#include <cstdio>
#include <cstring>
#include <mutex>
#include <unordered_map>

extern void Log(const char* fmt, ...);

namespace cvr::list_audit {
namespace {
struct Stack {
    void* frames[20]{};
    USHORT count{};
    DWORD thread{};
    void Capture() {
        count = RtlCaptureStackBackTrace(2, 20, frames, nullptr);
        thread = GetCurrentThreadId();
    }
};

struct AllocatorRecord {
    uint64_t epoch{};
    Stack reset;
};

struct ListRecord {
    ID3D12CommandAllocator* allocator{};
    uint64_t epoch{};
    bool open{};
    Stack recorded;
};

std::mutex mutex;
std::unordered_map<ID3D12CommandAllocator*, AllocatorRecord> allocators;
std::unordered_map<ID3D12GraphicsCommandList*, ListRecord> lists;
std::atomic<int> reports{0};

using PFN_AllocatorReset = HRESULT(STDMETHODCALLTYPE*)(ID3D12CommandAllocator*);
PFN_AllocatorReset allocatorResetOriginal = nullptr;

void Format(const Stack& stack, char* out, size_t size) {
    size_t used = 0;
    out[0] = 0;
    for (USHORT i = 0; i < stack.count && used + 72 < size; ++i) {
        HMODULE module = nullptr;
        char name[MAX_PATH]{};
        if (GetModuleHandleExA(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                               static_cast<LPCSTR>(stack.frames[i]), &module) && module &&
            GetModuleFileNameA(module, name, MAX_PATH)) {
            const char* base = strrchr(name, '\\');
            used += snprintf(out + used, size - used, " %s+0x%llX", base ? base + 1 : name,
                             static_cast<unsigned long long>(reinterpret_cast<uintptr_t>(stack.frames[i]) -
                                                             reinterpret_cast<uintptr_t>(module)));
        } else {
            used += snprintf(out + used, size - used, " %p", stack.frames[i]);
        }
    }
}

void Report(const char* what, ID3D12CommandQueue* queue, ID3D12GraphicsCommandList* list, const ListRecord& record,
            const AllocatorRecord* allocator, const Stack& submit) {
    if (reports.fetch_add(1, std::memory_order_relaxed) >= 8) return;
    char text[2048];
    Log("[list audit] %s: queue=%p list=%p allocator=%p list epoch=%llu allocator epoch=%llu\n", what, queue, list,
        record.allocator, static_cast<unsigned long long>(record.epoch),
        static_cast<unsigned long long>(allocator ? allocator->epoch : 0));
    Format(submit, text, sizeof(text));
    Log("[list audit]   submitted by thread %lu:%s\n", submit.thread, text);
    Format(record.recorded, text, sizeof(text));
    Log("[list audit]   recorded (Reset) by thread %lu:%s\n", record.recorded.thread, text);
    if (allocator) {
        Format(allocator->reset, text, sizeof(text));
        Log("[list audit]   allocator last reset by thread %lu:%s\n", allocator->reset.thread, text);
    }
}

HRESULT STDMETHODCALLTYPE AllocatorResetHook(ID3D12CommandAllocator* self) {
    const HRESULT result = allocatorResetOriginal(self);
    if (SUCCEEDED(result)) {
        std::lock_guard<std::mutex> lock(mutex);
        auto& record = allocators[self];
        ++record.epoch;
        record.reset.Capture();
    }
    return result;
}
}  // namespace

bool Enabled() {
    static const bool enabled = [] {
        char value[64]{};
        const DWORD n = GetEnvironmentVariableA("CVR_GPU_DIAG", value, sizeof(value));
        return n && n < sizeof(value) && strstr(value, "lists") != nullptr;
    }();
    return enabled;
}

void InstallAllocatorHook(ID3D12Device* device) {
    if (!Enabled() || !device || allocatorResetOriginal) return;
    Microsoft::WRL::ComPtr<ID3D12CommandAllocator> probe;
    if (FAILED(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&probe)))) {
        Log("[list audit] no allocator to hook\n");
        return;
    }
    void* target = (*reinterpret_cast<void***>(probe.Get()))[8];   // ID3D12CommandAllocator::Reset
    const MH_STATUS init = MH_Initialize();
    if ((init != MH_OK && init != MH_ERROR_ALREADY_INITIALIZED) ||
        MH_CreateHook(target, reinterpret_cast<void*>(&AllocatorResetHook),
                      reinterpret_cast<void**>(&allocatorResetOriginal)) != MH_OK ||
        MH_EnableHook(target) != MH_OK) {
        Log("[list audit] allocator Reset hook failed at %p\n", target);
        allocatorResetOriginal = nullptr;
        return;
    }
    Log("[list audit] on: allocator Reset hooked at %p\n", target);
}

void ListReset(ID3D12GraphicsCommandList* list, ID3D12CommandAllocator* allocator) {
    if (!Enabled() || !list) return;
    std::lock_guard<std::mutex> lock(mutex);
    auto& record = lists[list];
    record.allocator = allocator;
    record.epoch = allocator ? allocators[allocator].epoch : 0;
    record.open = true;
    record.recorded.Capture();
}

void ListClose(ID3D12GraphicsCommandList* list) {
    if (!Enabled() || !list) return;
    std::lock_guard<std::mutex> lock(mutex);
    auto it = lists.find(list);
    if (it != lists.end()) it->second.open = false;
}

void Execute(ID3D12CommandQueue* queue, UINT count, ID3D12CommandList* const* submitted) {
    if (!Enabled() || !submitted || !allocatorResetOriginal) return;
    Stack submit;
    bool captured = false;
    std::lock_guard<std::mutex> lock(mutex);
    for (UINT i = 0; i < count; ++i) {
        auto* list = static_cast<ID3D12GraphicsCommandList*>(submitted[i]);
        auto it = lists.find(list);
        if (it == lists.end()) continue;
        const ListRecord& record = it->second;
        const AllocatorRecord* allocator = nullptr;
        if (record.allocator) {
            auto found = allocators.find(record.allocator);
            if (found != allocators.end()) allocator = &found->second;
        }
        const bool reset = allocator && allocator->epoch != record.epoch;
        if (!record.open && !reset) continue;
        if (!captured) { submit.Capture(); captured = true; }
        Report(record.open ? "list submitted while open" : "allocator reset after the list was recorded", queue,
               list, record, allocator, submit);
    }
}
}  // namespace cvr::list_audit
