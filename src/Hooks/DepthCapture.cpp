// DepthCapture -- finding the game's depth buffer, and the queue synchronisation that makes it usable.
//
// The depth buffer is not handed to anyone: it is bound. So the way to find it is to watch what the
// engine binds -- CreateDepthStencilView records which resource a descriptor names, OMSetRenderTargets
// records which descriptor is bound when, and ResourceBarrier records the state it rests in. Three
// hooks, one answer.
//
// PATCHING A VTABLE SLOT IS DONE ONCE PER (VTABLE, SLOT), and MakeVtableSlotKey is why: command lists
// are pooled and recycled, so the same vtable arrives thousands of times and patching it repeatedly
// would chain our hook to itself.
//
// The queue-signal wait exists because our own work reads what the game's queue produced. Waiting on
// ALL of the game's signals rather than the newest is deliberate -- the engine signals several fences
// per frame and which one covers the depth write is not fixed.

#include <MinHook.h>
#include "Hooks/SwapChainInternal.hpp"
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

uintptr_t MakeVtableSlotKey(void** vtable, size_t slot) {
    return reinterpret_cast<uintptr_t>(vtable) ^ (static_cast<uintptr_t>(slot) * 0x9E3779B97F4A7C15ull);
}

// template GetOriginalMethod moved to Hooks/SwapChainInternal.hpp: a template must be VISIBLE
// where it is instantiated, and DisplayModes.cpp instantiates it with two different types.

bool PatchVtableMethod(void** vtable, size_t slot, void* hook) {
    if (!vtable || !hook) return false;

    void* methodSlot = vtable[slot];
    if (!methodSlot) return false;

    const uintptr_t key = MakeVtableSlotKey(vtable, slot);
    std::lock_guard<std::mutex> lock(g_vtableMutex);
    if (methodSlot == hook) {
        return g_originalVtableMethods.find(key) != g_originalVtableMethods.end();
    }

    DWORD oldProtect = 0;
    if (!VirtualProtect(&vtable[slot], sizeof(void*), PAGE_EXECUTE_READWRITE, &oldProtect)) {
        return false;
    }

    if (g_originalVtableMethods.find(key) == g_originalVtableMethods.end()) {
        g_originalVtableMethods[key] = methodSlot;
    }
    vtable[slot] = hook;

    DWORD restoreProtect = 0;
    VirtualProtect(&vtable[slot], sizeof(void*), oldProtect, &restoreProtect);
    FlushInstructionCache(GetCurrentProcess(), &vtable[slot], sizeof(void*));
    return true;
}

static void RecordEclQueue(void* queue) {
    for (uint32_t i = 0; i < 16; ++i) {
        void* seen = g_eclSeenQueues[i].load(std::memory_order_relaxed);
        if (seen == queue) return;
        if (seen == nullptr) {
            void* expected = nullptr;
            if (g_eclSeenQueues[i].compare_exchange_strong(expected, queue, std::memory_order_relaxed)) {
                g_eclDistinctQueues.fetch_add(1, std::memory_order_relaxed);
                return;
            }
            if (g_eclSeenQueues[i].load(std::memory_order_relaxed) == queue) return;
        }
    }
}

// [DEPTH-DIAG] Scene-depth identification spike (groundwork for alternate-eye depth
// submission). Tearing is the flat color-only projection layer reprojected without
// depth; the fix is submitting the game's depth as XR_KHR_composition_layer_depth.
// First step: reliably PIN the game's main scene depth-stencil resource. This is
// pure observation (descriptor map + vtable read) — NO GPU copy/readback/barrier,
// so it cannot trigger the device-removed crashes seen with prior GPU passes.
using CreateDepthStencilViewFn = void(STDMETHODCALLTYPE*)(ID3D12Device*, ID3D12Resource*, const D3D12_DEPTH_STENCIL_VIEW_DESC*, D3D12_CPU_DESCRIPTOR_HANDLE);
using OMSetRenderTargetsFn = void(STDMETHODCALLTYPE*)(ID3D12GraphicsCommandList*, UINT, const D3D12_CPU_DESCRIPTOR_HANDLE*, BOOL, const D3D12_CPU_DESCRIPTOR_HANDLE*);
using ResourceBarrierFn = void(STDMETHODCALLTYPE*)(ID3D12GraphicsCommandList*, UINT, const D3D12_RESOURCE_BARRIER*);

struct DepthDsvInfo {
    ID3D12Resource* resource;
    UINT format;
    UINT width;
    UINT height;
};
std::unordered_map<SIZE_T, DepthDsvInfo> g_dsvMap;
std::mutex g_dsvMutex;
std::atomic<uint32_t> g_dsvCount{0};

std::atomic<ID3D12Resource*> g_sceneDepthRes{nullptr};
// Size of the image actually being presented -- the scene depth has to match it, see the
// selection in the OM hook below.
std::atomic<uint32_t> g_presentWidth{0};
std::atomic<uint32_t> g_presentHeight{0};
// Was the currently pinned scene depth bound again this frame? While it is, the pick is
// left alone; the choice only reopens after it has been absent for a few frames.
std::atomic<bool>     g_sceneDepthSeen{false};
std::atomic<uint32_t> g_sceneDepthMissFrames{0};
// Guards the AddRef/Release lifecycle of the cached scene-depth resource. The
// cached slot owns exactly ONE reference so the pointer stays valid even after
// the game frees its own depth (e.g. across a save-load resource recreation).
// Without this, a later gameDepth->GetDesc() / copy dereferences a freed vtable
// and crashes with a DEP execution violation.
std::mutex g_sceneDepthRefMutex;
std::atomic<UINT> g_sceneDepthW{0};
std::atomic<UINT> g_sceneDepthH{0};
std::atomic<UINT> g_sceneDepthFmt{0};
std::atomic<uint64_t> g_sceneDepthArea{0};
std::atomic<UINT> g_sceneDepthState{0}; // D3D12_RESOURCE_STATE_COMMON until an explicit transition is observed
std::atomic<uint64_t> g_omSetRtCalls{0};
// The command list that last bound the scene-depth DSV, and the queue that executed
// it = the game's depth-writer queue. Recording our depth resolve on THAT queue makes
// it FIFO-ordered after the game's depth write (no cross-queue Wait -> no CP2077 hang).
std::atomic<ID3D12CommandList*> g_sceneDepthBinderList{nullptr};
std::atomic<ID3D12CommandQueue*> g_sceneDepthWriterQueue{nullptr};
std::mutex g_cmdVtMutex;
std::unordered_set<void**> g_patchedCmdVtables;
std::atomic<uint32_t> g_distinctCmdVtables{0};

void STDMETHODCALLTYPE HookedCreateDepthStencilView(ID3D12Device* device, ID3D12Resource* resource, const D3D12_DEPTH_STENCIL_VIEW_DESC* desc, D3D12_CPU_DESCRIPTOR_HANDLE dest) {
    void** vtable = *reinterpret_cast<void***>(device);
    CreateDepthStencilViewFn originalFn = GetOriginalMethod<CreateDepthStencilViewFn>(vtable, 21);
    if (originalFn) {
        originalFn(device, resource, desc, dest);
    }
    if (resource && dest.ptr) {
        D3D12_RESOURCE_DESC rd = resource->GetDesc();
        DepthDsvInfo info{};
        info.resource = resource;
        info.format = desc ? static_cast<UINT>(desc->Format) : static_cast<UINT>(rd.Format);
        info.width = static_cast<UINT>(rd.Width);
        info.height = rd.Height;
        std::lock_guard<std::mutex> lock(g_dsvMutex);
        if (g_dsvMap.find(dest.ptr) == g_dsvMap.end()) {
            g_dsvCount.fetch_add(1, std::memory_order_relaxed);
        }
        g_dsvMap[dest.ptr] = info;
    }
}

void STDMETHODCALLTYPE HookedOMSetRenderTargets(ID3D12GraphicsCommandList* list, UINT numRTVs, const D3D12_CPU_DESCRIPTOR_HANDLE* rtvs, BOOL singleHandle, const D3D12_CPU_DESCRIPTOR_HANDLE* dsv) {
    void** vtable = *reinterpret_cast<void***>(list);
    OMSetRenderTargetsFn originalFn = GetOriginalMethod<OMSetRenderTargetsFn>(vtable, 46);
    if (originalFn) {
        originalFn(list, numRTVs, rtvs, singleHandle, dsv);
    }
    g_omSetRtCalls.fetch_add(1, std::memory_order_relaxed);
    if (dsv && dsv->ptr) {
        DepthDsvInfo info{};
        bool found = false;
        {
            std::lock_guard<std::mutex> lock(g_dsvMutex);
            auto it = g_dsvMap.find(dsv->ptr);
            if (it != g_dsvMap.end()) { info = it->second; found = true; }
        }
        if (found) {
            // If this bind targets the current scene depth, remember the command list:
            // when it is later executed, the executing queue is the depth-writer queue.
            if (info.resource && info.resource == g_sceneDepthRes.load(std::memory_order_relaxed)) {
                g_sceneDepthBinderList.store(static_cast<ID3D12CommandList*>(list), std::memory_order_relaxed);
                // Still in use -> keep it. MAIN owns TWO same-size depth-stencils (an
                // R32_TYPELESS and an R32G8X24_TYPELESS; the capture shows both), so "bigger
                // wins" cannot choose between them and whichever the async workers happen to
                // bind first takes the slot -- the pick then alternates every frame (log:
                // "gameFmt=39" / "gameFmt=19" in turn). The depth CAPTURE only runs after the
                // same resource has held the slot for 60 frames, so an alternating pick never
                // warms up and depth is never submitted at all. Stickiness ends that: the
                // choice is only reopened once the current one stops appearing.
                g_sceneDepthSeen.store(true, std::memory_order_relaxed);
            }
            // Scene depth = the largest depth-stencil ever bound to OM -- BUT ONLY among
            // formats that can actually serve as a depth layer.
            //
            // "Largest ever" alone picks the wrong resource in VR. The biggest depth-stencil
            // the game binds is the SHADOW ATLAS, and CP2077 authors it 16-bit: the pinned
            // format came out as 53 (R16_TYPELESS), which the submit path then rejects as
            // not depth-resolvable and disables the depth layer entirely (log: "depth layer
            // disabled (gameFmt=53 ...)"). So the mod has never submitted depth, and the
            // runtime has only ever had orientation timewarp to work with -- no positional
            // reprojection, which is exactly what a compositor needs when the app runs below
            // the display rate.
            //
            // The scene depth is 32bpp (R32 family) or 64bpp (R32G8X24 family, resolved via
            // the depth-plane shader). Filtering on that leaves the geometry pass' own
            // buffer as the largest candidate.
            const uint32_t f = static_cast<uint32_t>(info.format);
            const bool resolvableDepth =
                f == DXGI_FORMAT_R32_TYPELESS      || f == DXGI_FORMAT_D32_FLOAT ||
                f == DXGI_FORMAT_R32_FLOAT         || f == DXGI_FORMAT_R32G8X24_TYPELESS ||
                f == DXGI_FORMAT_D32_FLOAT_S8X24_UINT ||
                f == DXGI_FORMAT_R32_FLOAT_X8X24_TYPELESS ||
                f == DXGI_FORMAT_X32_TYPELESS_G8X24_UINT;
            // ...and prefer the one whose size EQUALS the presented image.
            //
            // An Nsight capture of a MAIN+VRCAM frame settles both halves of this:
            //   2048x2048 R16_TYPELESS   <- shadow atlas, the biggest DSV of all
            //   2444x2444 R32G8X24       <- VRCAM scene depth
            //   1920x1080 R32G8X24       <- MAIN scene depth
            //   1418x1418 / 1114x627     <- the same two at DLSS input resolution
            //   1024x1024 R16 x6         <- more shadow maps
            // "Largest ever" picked the 2048 shadow atlas (4.2 Mpx beats MAIN's 2.1), which
            // is where gameFmt=53 came from. Filtering formats alone is still not enough:
            // with VRCAM on, the largest RESOLVABLE depth is its 2444x2444, so we would hand
            // the compositor VRCAM's depth next to MAIN's colour -- worse than no depth.
            // Matching the presented size picks MAIN's own buffer in both cases.
            // ...and only the SWAPCHAIN-sized one, chosen fresh every frame.
            //
            // Two independent corrections, both forced by an Nsight capture of a
            // MAIN+VRCAM frame:
            //
            // 1. Format. The depth-stencils present are 2444x2444 R32/R32G8X24 (VRCAM),
            //    2048x2048 R16 (shadow atlas), 1920x1080 R32G8X24 (MAIN), the same two at
            //    DLSS input size, and six 1024x1024 R16 shadow maps. "Largest ever" picked
            //    the 2048 shadow atlas -- 4.2 Mpx against MAIN's 2.1 -- and it is 16-bit,
            //    which is where the rejected gameFmt=53 came from.
            //
            // 2. "Ever" itself. Latching the maximum for the whole session means one wrong
            //    early pick is permanent, and it cannot follow a resolution change. The
            //    candidate is reset each present (ResetSceneDepthPick) so the choice is
            //    made from the frame in front of us.
            //
            // The size test is the swapchain's, which is MAIN's by definition. Note the
            // limit: once the resolution override makes VRCAM the same size, size stops
            // separating them and the view key from the render graph is required. This is
            // deliberately the MAIN-correct rule for now.
            // WHOSE depth: decided by the view, not by the resolution.
            //
            // Size cannot separate MAIN from VRCAM -- the resolution override deliberately
            // makes them equal. The render graph can: the node dispatcher carries the view
            // context and MAIN's key is 0 (CyberpunkVR_IsMainViewRecording, hooked in
            // vr_core.cpp). Nodes record on the dispatching thread, so this is the same
            // thread that is binding the DSV right now.
            //
            // Size stays only as the fallback for binds that happen outside any node
            // dispatch, and the format filter stays regardless: the biggest depth-stencil in
            // the frame is the 2048x2048 16-bit shadow atlas (Nsight), which is what the old
            // "largest ever" rule latched onto and why the depth layer never engaged.
            const uint32_t pw = g_presentWidth.load(std::memory_order_relaxed);
            const uint32_t ph = g_presentHeight.load(std::memory_order_relaxed);
            const bool sizeKnown = pw != 0 && ph != 0;
            const bool exactSize = sizeKnown && info.width == pw && info.height == ph;
            const bool viewKnown = CyberpunkVR_ViewKeyHookActive() != 0;
            const bool isMainView = CyberpunkVR_IsMainViewRecording() != 0;
            uint64_t area = 0;
            if (resolvableDepth) {
                const bool accept = viewKnown ? isMainView : (!sizeKnown || exactSize);
                if (accept) {
                    area = static_cast<uint64_t>(info.width) * static_cast<uint64_t>(info.height);
                }
            }
            if (area > g_sceneDepthArea.load(std::memory_order_relaxed)) {
                // Take ownership of one reference on the new resource and drop the
                // reference we held on the previous one. Keeping a ref means the
                // pointer is always safe to dereference, even if the game has since
                // freed its own handle to that depth buffer.
                std::lock_guard<std::mutex> reflock(g_sceneDepthRefMutex);
                if (area > g_sceneDepthArea.load(std::memory_order_relaxed)) {
                    if (info.resource) {
                        info.resource->AddRef();
                    }
                    ID3D12Resource* old = g_sceneDepthRes.exchange(info.resource, std::memory_order_relaxed);
                    g_sceneDepthArea.store(area, std::memory_order_relaxed);
                    g_sceneDepthW.store(info.width, std::memory_order_relaxed);
                    g_sceneDepthH.store(info.height, std::memory_order_relaxed);
                    g_sceneDepthFmt.store(info.format, std::memory_order_relaxed);
                    if (old && old != info.resource) {
                        old->Release();
                    }
                }
            }
        }
    }
    // Status is logged from the ECL hook so it appears even if OM is never called.
}

void STDMETHODCALLTYPE HookedResourceBarrier(ID3D12GraphicsCommandList* list, UINT numBarriers, const D3D12_RESOURCE_BARRIER* barriers) {
    void** vtable = *reinterpret_cast<void***>(list);
    ResourceBarrierFn originalFn = GetOriginalMethod<ResourceBarrierFn>(vtable, 26);
    if (originalFn) {
        originalFn(list, numBarriers, barriers);
    }
    // Track the scene depth's current resource state so a later (Present-time) copy
    // uses the CORRECT StateBefore. A wrong transition is the classic device-removed
    // cause — we never guess; we observe the game's own explicit transitions.
    ID3D12Resource* depth = g_sceneDepthRes.load(std::memory_order_relaxed);
    if (depth && barriers) {
        for (UINT i = 0; i < numBarriers; ++i) {
            if (barriers[i].Type == D3D12_RESOURCE_BARRIER_TYPE_TRANSITION &&
                barriers[i].Transition.pResource == depth) {
                const UINT after = static_cast<UINT>(barriers[i].Transition.StateAfter);
                g_sceneDepthState.store(after, std::memory_order_relaxed);
                // THE moment to take the depth: the engine has just made it shader-readable
                // for its own post passes. Waiting until Present means finding it back in
                // DEPTH_WRITE on many frames (measured), which is why the depth layer used to
                // come and go. Only a copy is injected here -- copies leave pipeline state
                // alone, so the engine's recording on this list is unaffected.
                if ((after & D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE) != 0 &&
                    CyberpunkVR_IsMainViewRecording()) {
                    OpenXRManager::Get().CaptureSceneDepthInline(list, depth, after);
                }
            }
        }
    }
}

void TryHookCmdListVtable(ID3D12CommandList* anyList) {
    if (!anyList) return;
    // Only DIRECT command lists issue OMSetRenderTargets. Patch EVERY distinct
    // graphics-list vtable we observe (do NOT latch on the first) — if the engine
    // uses more than one command-list vtable, a single-latch hook would miss the
    // one that actually records the scene pass.
    if (anyList->GetType() != D3D12_COMMAND_LIST_TYPE_DIRECT) return;
    void** vtable = *reinterpret_cast<void***>(anyList);
    {
        std::lock_guard<std::mutex> lock(g_cmdVtMutex);
        if (g_patchedCmdVtables.count(vtable)) return;
        g_patchedCmdVtables.insert(vtable);
    }
    const uint32_t n = g_distinctCmdVtables.fetch_add(1, std::memory_order_relaxed) + 1;
    if (PatchVtableMethod(vtable, 46, reinterpret_cast<void*>(&HookedOMSetRenderTargets))) {
        if (g_verboseLog) {
            Log("[DEPTH-DIAG] Hooked OMSetRenderTargets on direct-list vtable=%p (distinctVtables=%u)\n",
                reinterpret_cast<void*>(vtable), n);
        }
    }
    // Same vtable: track depth state transitions (slot 26 = ResourceBarrier).
    PatchVtableMethod(vtable, 26, reinterpret_cast<void*>(&HookedResourceBarrier));
}

void InstallDepthCaptureHooks(ID3D12Device* device) {
    if (!device) return;
    void** vtable = *reinterpret_cast<void***>(device);
    if (PatchVtableMethod(vtable, 21, reinterpret_cast<void*>(&HookedCreateDepthStencilView))) {
        if (g_verboseLog) Log("[DEPTH-DIAG] Hooked ID3D12Device::CreateDepthStencilView (slot 21)\n");
    }
}

void TryHookQueueSignalVtable(ID3D12CommandQueue* queue); // defined below

void STDMETHODCALLTYPE HookedExecuteCommandLists(ID3D12CommandQueue* queue, UINT numLists, ID3D12CommandList* const* lists) {
    g_eclTotalCalls.fetch_add(1, std::memory_order_relaxed);
    g_eclTotalLists.fetch_add(numLists, std::memory_order_relaxed);
    RecordEclQueue(queue);
    // Hook Signal on each distinct queue vtable so we can later GPU-Wait on the
    // game's most recent fence values (cross-queue sync for depth capture).
    // ExecuteCommandLists is the perfect discovery point: every queue the game
    // uses will eventually run something through here.
    TryHookQueueSignalVtable(queue);
    if (lists) {
        ID3D12CommandList* depthBinder = g_sceneDepthBinderList.load(std::memory_order_relaxed);
        for (UINT i = 0; i < numLists; ++i) {
            TryHookCmdListVtable(lists[i]);
            // The queue that submits the scene-depth binder list = depth-writer queue.
            if (depthBinder && lists[i] == depthBinder) {
                g_sceneDepthWriterQueue.store(queue, std::memory_order_relaxed);
            }
        }
    }
    if (g_verboseLog && (g_eclTotalCalls.load(std::memory_order_relaxed) % 600) == 1) {
        Log("[DEPTH-DIAG] status: sceneDepth res=%p %ux%u fmt=%u | distinctDSV=%u omCalls=%llu cmdVtables=%u\n",
            g_sceneDepthRes.load(std::memory_order_relaxed),
            g_sceneDepthW.load(std::memory_order_relaxed),
            g_sceneDepthH.load(std::memory_order_relaxed),
            g_sceneDepthFmt.load(std::memory_order_relaxed),
            g_dsvCount.load(std::memory_order_relaxed),
            static_cast<unsigned long long>(g_omSetRtCalls.load(std::memory_order_relaxed)),
            g_distinctCmdVtables.load(std::memory_order_relaxed));
    }
    if (queue == g_presentQueue.load(std::memory_order_relaxed)) {
        g_eclPresentCalls.fetch_add(1, std::memory_order_relaxed);
        g_eclPresentLists.fetch_add(numLists, std::memory_order_relaxed);
    }
    void** vtable = *reinterpret_cast<void***>(queue);
    ExecuteCommandListsFn originalFn = GetOriginalMethod<ExecuteCommandListsFn>(vtable, 10);
    if (originalFn) {
        originalFn(queue, numLists, lists);
    }
}

// ID3D12CommandQueue vtable slot 10 == ExecuteCommandLists. All command queues
// of the same type share one vtable, so patching once observes every queue.
void InstallCommandQueueDiagHook(ID3D12CommandQueue* queue) {
    if (!queue) return;
    g_presentQueue.store(queue, std::memory_order_relaxed);
    void** vtable = *reinterpret_cast<void***>(queue);
    PatchVtableMethod(vtable, 10, reinterpret_cast<void*>(&HookedExecuteCommandLists));
}

// Cross-queue fence tracker: the game writes scene depth on its own render
// queue while we want to copy that resource on the swapchain (present) queue.
// On VDXR + R32_TYPELESS the format guard passes our path through, and without
// explicit cross-queue sync our copy can race the game's writer → GPU hang.
// Hook ID3D12CommandQueue::Signal (vtable slot 14) on every distinct queue
// vtable we see and record (queue → last (fence, value)). Before our depth
// copy, our queue Waits on each tracked queue's last value — GPU-side, no
// CPU stall. If the game has not yet signaled anything we just no-op.
using SignalFn = HRESULT(STDMETHODCALLTYPE*)(ID3D12CommandQueue*, ID3D12Fence*, UINT64);

struct QueueSignalState {
    ID3D12Fence* fence = nullptr;
    UINT64 value = 0;
    // WHEN it was signalled. Without this the table below is a list of every queue the game has
    // EVER signalled, and the depth copy's cross-queue wait blocks on all of them forever.
    uint64_t tickMs = 0;
};
// A queue that has not signalled for this long is not part of the current frame's work; waiting
// on its last value is waiting on something nothing is going to advance.
static const uint64_t kSignalFreshMs = 250;
// And one this old is gone for good -- drop it and let go of its fence.
static const uint64_t kSignalDeadMs  = 5000;
// Counted so the guard can never become a silent behaviour change.
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugQueueWaitsIssued  = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugQueueWaitsSkipped = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugQueueWaitsDropped = 0;
std::mutex g_queueSignalMutex;
std::unordered_map<ID3D12CommandQueue*, QueueSignalState> g_queueLastSignal;
// Dedicated mutex for the hooked-vtable set. We CANNOT reuse g_vtableMutex
// here because PatchVtableMethod itself locks g_vtableMutex, which would
// cause a recursive lock on a non-recursive std::mutex (= undefined
// behavior, MSVC throws 0xE06D7363 C++ exception). Lesson learned the hard
// way: per-set membership tracking gets its own lightweight mutex.
std::mutex g_signalHookSetMutex;
std::unordered_set<void**> g_signalHookedVtables;

HRESULT STDMETHODCALLTYPE HookedQueueSignal(ID3D12CommandQueue* queue, ID3D12Fence* fence, UINT64 value) {
    void** vtable = *reinterpret_cast<void***>(queue);
    SignalFn originalFn = GetOriginalMethod<SignalFn>(vtable, 14);
    const HRESULT hr = originalFn ? originalFn(queue, fence, value) : E_FAIL;
    if (SUCCEEDED(hr) && fence) {
        std::lock_guard<std::mutex> lock(g_queueSignalMutex);
        auto& entry = g_queueLastSignal[queue];
        // Track by queue ptr; same queue may signal multiple fences but we
        // want the LATEST per-queue gate, regardless of which fence. Multiple
        // game queues each carry their own latest signal independently.
        if (entry.fence != fence) {
            if (entry.fence) entry.fence->Release();
            fence->AddRef();
            entry.fence = fence;
        }
        entry.value = value;
        entry.tickMs = GetTickCount64();
    }
    return hr;
}

void TryHookQueueSignalVtable(ID3D12CommandQueue* queue) {
    if (!queue) return;
    void** vtable = *reinterpret_cast<void***>(queue);
    {
        std::lock_guard<std::mutex> lock(g_signalHookSetMutex);
        if (g_signalHookedVtables.count(vtable)) return;
        g_signalHookedVtables.insert(vtable);
    }
    // PatchVtableMethod locks g_vtableMutex internally — must NOT be
    // called while we hold any other lock that PatchVtableMethod might
    // recursively try to take.
    PatchVtableMethod(vtable, 14, reinterpret_cast<void*>(&HookedQueueSignal));
}

// Issued on the consumer's queue (m_d3dQueue) BEFORE a CopyResource that may
// race game-side depth writes. Tries to wait on every tracked queue's most
// recent signal; harmless if none tracked yet.
extern "C" void CyberpunkVRPort_WaitOnAllGameSignals(ID3D12CommandQueue* consumerQueue) {
    if (!consumerQueue) return;
    // ONLY QUEUES THAT ARE STILL RUNNING. This used to wait on the last signal of every queue the
    // game had ever signalled, and that table was never pruned -- so a queue the engine stops feeding
    // (which is exactly what happens when the pause menu opens) left a Wait on our consumer queue that
    // nothing was ever going to satisfy. A GPU wait that never resolves is a hang for the whole
    // device, and it reads in the crash report as DXGI_ERROR_DEVICE_HUNG (0x887a0006) with DRED's
    // PageFaultVA at 0 and every command list of the next frame marked "Not started": the GPU did not
    // fault, it never began.
    //
    // Fresh means signalled within kSignalFreshMs. Anything older is not part of the frame we are
    // about to copy against and cannot race it; anything past kSignalDeadMs is dropped outright, which
    // also stops the table -- and the fence references it holds -- from growing for the whole session.
    std::vector<QueueSignalState> snapshot;
    const uint64_t now = GetTickCount64();
    {
        std::lock_guard<std::mutex> lock(g_queueSignalMutex);
        snapshot.reserve(g_queueLastSignal.size());
        for (auto it = g_queueLastSignal.begin(); it != g_queueLastSignal.end(); ) {
            const uint64_t age = (now >= it->second.tickMs) ? now - it->second.tickMs : 0;
            if (it->second.fence && age > kSignalDeadMs) {
                it->second.fence->Release();
                ++CyberpunkVR_DebugQueueWaitsDropped;
                it = g_queueLastSignal.erase(it);
                continue;
            }
            if (it->first != consumerQueue && it->second.fence) {
                if (age <= kSignalFreshMs) {
                    it->second.fence->AddRef();
                    snapshot.push_back(it->second);
                } else {
                    ++CyberpunkVR_DebugQueueWaitsSkipped;
                }
            }
            ++it;
        }
    }
    for (auto& s : snapshot) {
        // Already complete on the GPU: the copy cannot race it, so do not put a wait in the way.
        if (s.fence->GetCompletedValue() < s.value) {
            consumerQueue->Wait(s.fence, s.value);
            ++CyberpunkVR_DebugQueueWaitsIssued;
        } else {
            ++CyberpunkVR_DebugQueueWaitsSkipped;
        }
        s.fence->Release();
    }
}
