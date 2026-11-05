// Dred -- what killed the device, when D3D12 will not say.
//
// A device-removed HRESULT on its own carries no information: the call that returned it is usually not
// the call that broke anything. DRED (Device Removed Extended Data) is the engine-side answer -- it
// records auto-breadcrumbs of the last GPU work submitted and the page fault that ended it.
//
// IT MUST BE ENABLED BEFORE ANY DEVICE EXISTS, which is why EnableDredOnce is called from the plugin's
// earliest entry point and not from anywhere that looks like it belongs to the swapchain. Enable it
// after the device is created and it records nothing, silently.
//
// DumpDredOnce fires at most once per session: the first device removal is the one with a cause, and a
// loop of them is the same cause reported repeatedly.

#include <MinHook.h>
#include "Hooks/SwapChainInternal.hpp"
#include "Hooks/ListAudit.hpp"
#include <thread>
#include "Hooks/SwapChain.hpp"
#include "Overlay/ImGuiOverlay.hpp"
#include "Hooks/Ngx.hpp"
#include <cstdint>
#include <cstdio>
#include <unordered_map>
#include <unordered_set>
#include <mutex>
#include <atomic>
#include <wrl.h>
#include <d3d12sdklayers.h>
#include <cstring>

extern void Log(const char* fmt, ...);
extern volatile int g_verboseLog; // gate per-frame spam (ClipCursor / depth-diag)
extern "C" void PrepareStartupLiveControls();
extern "C" int CyberpunkVR_IsMainViewRecording();
extern "C" int CyberpunkVR_ViewKeyHookActive();
extern "C" void CyberpunkVR_ProfPublish();
extern "C" UINT GetForcedSwapchainWidth();
extern "C" UINT GetForcedSwapchainHeight();
extern "C" UINT GetForcedDisplayModeWidth();
extern "C" UINT GetForcedDisplayModeHeight();
extern "C" UINT GetForcedWindowWidth();
extern "C" UINT GetForcedWindowHeight();
extern "C" int GetMenuMode();

void RememberDredDevice(ID3D12Device* device) {
    if (!device) {
        return;
    }
    std::lock_guard<std::mutex> lock(g_dredMutex);
    if (g_dredDevice.Get() != device) {
        g_dredDevice = device;
        g_dredDumped = false;
        Log("[DRED] Tracking D3D12 device %p\n", device);
    }
}

void RememberDredDeviceFromSwapChain(IDXGISwapChain* swapChain) {
    if (!swapChain) {
        return;
    }

    Microsoft::WRL::ComPtr<ID3D12Device> device;
    HRESULT hr = swapChain->GetDevice(IID_PPV_ARGS(&device));
    if (FAILED(hr)) {
        Microsoft::WRL::ComPtr<ID3D12Resource> backBuffer;
        hr = swapChain->GetBuffer(0, IID_PPV_ARGS(&backBuffer));
        if (SUCCEEDED(hr) && backBuffer) {
            hr = backBuffer->GetDevice(IID_PPV_ARGS(&device));
        }
    }

    if (SUCCEEDED(hr) && device) {
        RememberDredDevice(device.Get());
    }
}

// DRED auto-breadcrumbs + page-fault reporting must be enabled BEFORE
// D3D12CreateDevice runs; otherwise GetAutoBreadcrumbsOutput/
// GetPageFaultAllocationOutput return empty data on device-removed and our
// DumpDredOnce() logs nothing useful. Call from each CreateDXGIFactory* entry
// — those run before any D3D12 device is created.
void EnableDredOnce() {
    static std::once_flag s_flag;
    std::call_once(s_flag, []() {
        HMODULE d3d12 = LoadLibraryA("d3d12.dll");
        if (!d3d12) {
            Log("[DRED] LoadLibrary(d3d12.dll) failed; DRED not enabled\n");
            return;
        }
        using PFN_D3D12GetDebugInterface = HRESULT(WINAPI*)(REFIID, void**);
        auto fn = reinterpret_cast<PFN_D3D12GetDebugInterface>(
            GetProcAddress(d3d12, "D3D12GetDebugInterface"));
        if (!fn) {
            Log("[DRED] D3D12GetDebugInterface export missing; DRED not enabled\n");
            return;
        }

        // Prefer Settings1 (Windows 10 2004+) — gives BreadcrumbContext for
        // richer DRED dumps; fall back to base Settings on older Windows.
        Microsoft::WRL::ComPtr<ID3D12DeviceRemovedExtendedDataSettings1> settings1;
        if (SUCCEEDED(fn(IID_PPV_ARGS(&settings1))) && settings1) {
            settings1->SetAutoBreadcrumbsEnablement(D3D12_DRED_ENABLEMENT_FORCED_ON);
            settings1->SetPageFaultEnablement(D3D12_DRED_ENABLEMENT_FORCED_ON);
            settings1->SetBreadcrumbContextEnablement(D3D12_DRED_ENABLEMENT_FORCED_ON);
            Log("[DRED] Enabled (settings1): AutoBreadcrumbs+PageFault+BreadcrumbContext\n");
            return;
        }
        Microsoft::WRL::ComPtr<ID3D12DeviceRemovedExtendedDataSettings> settings;
        if (SUCCEEDED(fn(IID_PPV_ARGS(&settings))) && settings) {
            settings->SetAutoBreadcrumbsEnablement(D3D12_DRED_ENABLEMENT_FORCED_ON);
            settings->SetPageFaultEnablement(D3D12_DRED_ENABLEMENT_FORCED_ON);
            Log("[DRED] Enabled (settings): AutoBreadcrumbs+PageFault\n");
            return;
        }
        Log("[DRED] D3D12GetDebugInterface QI for DRED settings failed; DRED not enabled\n");
    });
}

bool IsDeviceRemovedHr(HRESULT hr) {
    return hr == DXGI_ERROR_DEVICE_REMOVED ||
        hr == DXGI_ERROR_DEVICE_HUNG ||
        hr == DXGI_ERROR_DEVICE_RESET;
}

// D3D12_AUTO_BREADCRUMB_OP in its declaration order (d3d12.h).
static const char* BreadcrumbOpName(unsigned op) {
    static const char* const names[] = {
        "SetMarker", "BeginEvent", "EndEvent", "DrawInstanced", "DrawIndexedInstanced", "ExecuteIndirect",
        "Dispatch", "CopyBufferRegion", "CopyTextureRegion", "CopyResource", "CopyTiles", "ResolveSubresource",
        "ClearRenderTargetView", "ClearUnorderedAccessView", "ClearDepthStencilView", "ResourceBarrier",
        "ExecuteBundle", "Present", "ResolveQueryData", "BeginSubmission", "EndSubmission", "DecodeFrame",
        "ProcessFrames", "AtomicCopyBufferUint", "AtomicCopyBufferUint64", "ResolveSubresourceRegion",
        "WriteBufferImmediate", "DecodeFrame1", "SetProtectedResourceSession", "DecodeFrame2", "ProcessFrames1",
        "BuildRaytracingAccelerationStructure", "EmitRaytracingAccelerationStructurePostbuildInfo",
        "CopyRaytracingAccelerationStructure", "DispatchRays", "InitializeMetaCommand", "ExecuteMetaCommand",
        "EstimateMotion", "ResolveMotionVectorHeap", "SetPipelineState1", "InitializeExtensionCommand",
        "ExecuteExtensionCommand", "DispatchMesh", "EncodeFrame", "ResolveEncoderOutputMetadata", "Barrier",
        "BeginCommandList", "DispatchGraph", "SetProgram", "ProcessFrames2"};
    return op < sizeof(names) / sizeof(names[0]) ? names[op] : "-";
}

void LogDredBreadcrumbs(ID3D12DeviceRemovedExtendedData* dred) {
    D3D12_DRED_AUTO_BREADCRUMBS_OUTPUT breadcrumbs{};
    const HRESULT hr = dred->GetAutoBreadcrumbsOutput(&breadcrumbs);
    if (FAILED(hr)) {
        Log("[DRED] GetAutoBreadcrumbsOutput failed: hr=0x%08X\n", static_cast<unsigned>(hr));
        return;
    }

    // A frame keeps a hundred command lists or more in the history, nearly all of them finished: the lists the GPU
    // had not finished are the ones that say where it stopped, so those are logged (and the finished ones counted).
    unsigned nodeIndex = 0, logged = 0, finished = 0;
    for (const D3D12_AUTO_BREADCRUMB_NODE* node = breadcrumbs.pHeadAutoBreadcrumbNode;
         node != nullptr && nodeIndex < 8192;
         node = node->pNext, ++nodeIndex) {
        const UINT last = node->pLastBreadcrumbValue ? *node->pLastBreadcrumbValue : UINT_MAX;
        if (last != UINT_MAX && last >= node->BreadcrumbCount) {
            ++finished;
            continue;
        }
        if (++logged > 64) continue;
        char listName[256]{};
        char queueName[256]{};
        const unsigned op = (node->pCommandHistory && last < node->BreadcrumbCount)
            ? static_cast<unsigned>(node->pCommandHistory[last])
            : UINT_MAX;
        const unsigned before = (node->pCommandHistory && last != UINT_MAX && last > 0 && last <= node->BreadcrumbCount)
            ? static_cast<unsigned>(node->pCommandHistory[last - 1])
            : UINT_MAX;
        Log("[DRED] Breadcrumb[%u] queue=%s list=%s completed=%u/%u op=%u(%s) after=%u(%s) queuePtr=%p listPtr=%p\n",
            nodeIndex,
            DebugName(node->pCommandQueueDebugNameA, node->pCommandQueueDebugNameW, queueName, sizeof(queueName)),
            DebugName(node->pCommandListDebugNameA, node->pCommandListDebugNameW, listName, sizeof(listName)),
            last,
            node->BreadcrumbCount,
            op, BreadcrumbOpName(op),
            before, BreadcrumbOpName(before),
            node->pCommandQueue,
            node->pCommandList);
    }
    Log("[DRED] Breadcrumbs: %u lists, %u finished, %u not finished (%u logged)\n",
        nodeIndex, finished, logged, logged < 64 ? logged : 64u);
}

void LogDredAllocations(const char* label, const D3D12_DRED_ALLOCATION_NODE* head) {
    unsigned index = 0;
    for (const D3D12_DRED_ALLOCATION_NODE* node = head;
         node != nullptr && index < 128;
         node = node->pNext, ++index) {
        char name[256]{};
        Log("[DRED] %s[%u] type=%u name=%s\n",
            label,
            index,
            static_cast<unsigned>(node->AllocationType),
            DebugName(node->ObjectNameA, node->ObjectNameW, name, sizeof(name)));
    }
}

void LogDredPageFault(ID3D12DeviceRemovedExtendedData* dred) {
    D3D12_DRED_PAGE_FAULT_OUTPUT pageFault{};
    const HRESULT hr = dred->GetPageFaultAllocationOutput(&pageFault);
    if (FAILED(hr)) {
        Log("[DRED] GetPageFaultAllocationOutput failed: hr=0x%08X\n", static_cast<unsigned>(hr));
        return;
    }

    Log("[DRED] PageFaultVA=0x%016llX\n", static_cast<unsigned long long>(pageFault.PageFaultVA));
    LogDredAllocations("ExistingAllocation", pageFault.pHeadExistingAllocationNode);
    LogDredAllocations("RecentFreedAllocation", pageFault.pHeadRecentFreedAllocationNode);
}

static void DumpDred(const Microsoft::WRL::ComPtr<ID3D12Device>& device, HRESULT presentHr);

void DumpDredOnce(IDXGISwapChain* swapChain, HRESULT presentHr) {
    Microsoft::WRL::ComPtr<ID3D12Device> device;
    {
        std::lock_guard<std::mutex> lock(g_dredMutex);
        if (g_dredDumped) {
            return;
        }
        device = g_dredDevice;
        g_dredDumped = true;
    }

    if (!device && swapChain) {
        swapChain->GetDevice(IID_PPV_ARGS(&device));
    }
    if (!device) {
        Log("[DRED] Present failed with hr=0x%08X but no D3D12 device is tracked\n", static_cast<unsigned>(presentHr));
        return;
    }
    DumpDred(device, presentHr);
}

// ---------------------------------------------------------------- GPU diagnostics on request
//
// CVR_GPU_DIAG (environment of the game process): "dred" or "1" turns DRED on before the game's device is made,
// "debug" also the D3D12 debug layer, "lists" the command-list audit (Hooks/ListAudit.cpp). None is on otherwise
// -- auto-breadcrumbs cost CPU on every command and the debug layer far more. EnableDredOnce from the swapchain wrapper is too late for both: the device exists by
// then, and DRED enabled after D3D12CreateDevice records nothing.
//
// The engine sees a removed device on its own fences and asserts before any Present of ours fails, so the dump
// also hangs off a fence of ours: a removed device completes every fence to UINT64_MAX, which signals an event
// waiting for that value. DRED's page-fault data needs the engine's Aftermath off (-gpuFlag noAftermath).
static int GpuDiagLevel() {
    static const int level = [] {
        char value[32]{};
        const DWORD n = GetEnvironmentVariableA("CVR_GPU_DIAG", value, sizeof(value));
        if (!n || n >= sizeof(value) || value[0] == '0') return 0;
        return strstr(value, "debug") ? 2 : 1;      // "lists" (Hooks/ListAudit.cpp) also turns DRED on
    }();
    return level;
}

static void STDMETHODCALLTYPE OnD3D12Message(D3D12_MESSAGE_CATEGORY category, D3D12_MESSAGE_SEVERITY severity,
                                             D3D12_MESSAGE_ID id, LPCSTR description, void*) {
    if (severity > D3D12_MESSAGE_SEVERITY_ERROR) return;
    // Each message the first few times; a resource released while the GPU still uses it every time.
    static std::atomic<uint16_t> seen[4096]{};
    const uint16_t count = seen[static_cast<unsigned>(id) & 4095].fetch_add(1, std::memory_order_relaxed);
    if (count >= 4 && id != D3D12_MESSAGE_ID_OBJECT_DELETED_WHILE_STILL_IN_USE) return;
    Log("[D3D12] %s id=%u cat=%u thread=%lu: %s\n", severity == D3D12_MESSAGE_SEVERITY_CORRUPTION ? "CORRUPTION" : "ERROR",
        static_cast<unsigned>(id), static_cast<unsigned>(category), GetCurrentThreadId(), description ? description : "");
    // The callback runs on the thread that made the call, inside it: the first few times a message is seen, its
    // caller is the stack here (module + RVA, for IDA or the module's PDB).
    if (count >= 2) return;
    void* frames[32]{};
    const USHORT n = RtlCaptureStackBackTrace(1, 32, frames, nullptr);
    char line[1024];
    size_t used = 0;
    for (USHORT i = 0; i < n && used + 64 < sizeof(line); ++i) {
        HMODULE module = nullptr;
        char name[MAX_PATH]{};
        if (GetModuleHandleExA(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                               static_cast<LPCSTR>(frames[i]), &module) && module &&
            GetModuleFileNameA(module, name, MAX_PATH)) {
            const char* base = strrchr(name, '\\');
            used += snprintf(line + used, sizeof(line) - used, " %s+0x%llX", base ? base + 1 : name,
                             static_cast<unsigned long long>(reinterpret_cast<uintptr_t>(frames[i]) -
                                                             reinterpret_cast<uintptr_t>(module)));
        } else {
            used += snprintf(line + used, sizeof(line) - used, " %p", frames[i]);
        }
    }
    Log("[D3D12]   stack:%s\n", line);
}

extern "C" void CyberpunkVRPort_GpuDiagBeforeDevice() {
    const int level = GpuDiagLevel();
    if (!level) return;
    EnableDredOnce();
    if (level < 2) return;
    using PFN_D3D12GetDebugInterface = HRESULT(WINAPI*)(REFIID, void**);
    HMODULE d3d12 = GetModuleHandleA("d3d12.dll");
    auto fn = d3d12 ? reinterpret_cast<PFN_D3D12GetDebugInterface>(GetProcAddress(d3d12, "D3D12GetDebugInterface")) : nullptr;
    Microsoft::WRL::ComPtr<ID3D12Debug> debug;
    if (fn && SUCCEEDED(fn(IID_PPV_ARGS(&debug))) && debug) {
        debug->EnableDebugLayer();
        Log("[GPU diag] D3D12 debug layer enabled\n");
    } else {
        Log("[GPU diag] D3D12 debug layer unavailable\n");
    }
}

extern "C" void CyberpunkVRPort_GpuDiagDevice(ID3D12Device* raw) {
    const int level = GpuDiagLevel();
    if (!level || !raw) return;
    static std::atomic<bool> started{false};
    if (started.exchange(true)) return;
    Microsoft::WRL::ComPtr<ID3D12Device> device(raw);
    RememberDredDevice(raw);
    cvr::list_audit::InstallAllocatorHook(raw);
    if (level >= 2) {
        Microsoft::WRL::ComPtr<ID3D12InfoQueue1> queue;
        DWORD cookie = 0;
        if (SUCCEEDED(device.As(&queue)) && queue &&
            SUCCEEDED(queue->RegisterMessageCallback(&OnD3D12Message, D3D12_MESSAGE_CALLBACK_FLAG_NONE, nullptr, &cookie))) {
            Log("[GPU diag] debug messages logged (cookie %lu)\n", cookie);
        } else {
            Log("[GPU diag] ID3D12InfoQueue1 unavailable: debug messages not logged\n");
        }
    }
    Microsoft::WRL::ComPtr<ID3D12Fence> fence;
    HANDLE event = CreateEventW(nullptr, FALSE, FALSE, nullptr);
    if (!event || FAILED(device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fence))) ||
        FAILED(fence->SetEventOnCompletion(UINT64_MAX, event))) {
        Log("[GPU diag] removal watch not armed\n");
        if (event) CloseHandle(event);
        return;
    }
    std::thread([device, fence, event] {
        WaitForSingleObject(event, INFINITE);
        CloseHandle(event);
        {
            std::lock_guard<std::mutex> lock(g_dredMutex);
            if (g_dredDumped) return;
            g_dredDumped = true;
        }
        DumpDred(device, S_OK);
    }).detach();
    Log("[GPU diag] DRED level %d, removal watch armed on device %p\n", level, raw);
}

static void DumpDred(const Microsoft::WRL::ComPtr<ID3D12Device>& device, HRESULT presentHr) {
    const HRESULT removedReason = device->GetDeviceRemovedReason();
    Log("[DRED] Device removed detected. presentHr=0x%08X reason=0x%08X device=%p\n",
        static_cast<unsigned>(presentHr),
        static_cast<unsigned>(removedReason),
        device.Get());

    Microsoft::WRL::ComPtr<ID3D12DeviceRemovedExtendedData> dred;
    const HRESULT qiHr = device.As(&dred);
    if (FAILED(qiHr) || !dred) {
        Log("[DRED] ID3D12DeviceRemovedExtendedData unavailable: hr=0x%08X\n", static_cast<unsigned>(qiHr));
        return;
    }

    LogDredBreadcrumbs(dred.Get());
    LogDredPageFault(dred.Get());
}
