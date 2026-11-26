#pragma once

// ================================================================================================
// What the swapchain family hands itself, after src/Hooks/SwapChain.cpp was split by subject.
//
// Same rule as the other internal headers: a name belongs here when a file OTHER than the one defining
// it uses that name. The boot order is the thing to keep in view -- DRED must be enabled before any
// device exists, the OS hooks before the window is shown, and the depth capture after the device.
// ================================================================================================

#include <windows.h>
#include <d3d12.h>
#include <dxgi1_6.h>
#include <wrl/client.h>
#include <unordered_map>

#include <mutex>
#include <new>
#include <cstdint>
#include "Hooks/OriginalMethodTable.hpp"
using EnumOutputsFn = HRESULT(STDMETHODCALLTYPE*)(IDXGIAdapter*, UINT, IDXGIOutput**);
using ExecuteCommandListsFn = void(STDMETHODCALLTYPE*)(ID3D12CommandQueue*, UINT, ID3D12CommandList* const*);
using GetDisplayModeList1Fn = HRESULT(STDMETHODCALLTYPE*)(IDXGIOutput1*, DXGI_FORMAT, UINT, UINT*, DXGI_MODE_DESC1*);
using GetDisplayModeListFn = HRESULT(STDMETHODCALLTYPE*)(IDXGIOutput*, DXGI_FORMAT, UINT, UINT*, DXGI_MODE_DESC*);
bool IsDeviceRemovedHr(HRESULT hr);
bool PatchVtableMethod(void** vtable, size_t slot, void* hook);
const char* DebugName(const char* ansiName, const wchar_t* wideName, char* buffer, size_t bufferSize);
// The largest render-aspect size inside the bounds (Hooks/SwapChain.cpp); the window hooks use it.
void FitRenderSizeInsideBounds(UINT renderWidth, UINT renderHeight, UINT maxWidth, UINT maxHeight,
                               UINT& outWidth, UINT& outHeight);
extern HWND g_gameHwnd;
extern Microsoft::WRL::ComPtr<ID3D12Device> g_dredDevice;
extern bool g_cursorClipped;
extern bool g_dredDumped;
extern std::atomic<void*> g_eclSeenQueues[16];
extern std::mutex g_dredMutex;
#if !CVR_RUST_VRIK
extern std::mutex g_vtableMutex;
extern std::unordered_map<uintptr_t, void*> g_originalVtableMethods;
// Lock-free mirror of g_originalVtableMethods, filled by PatchVtableMethod (Hooks/OriginalMethodTable.hpp).
extern cvr::hooks::OriginalMethodTable<> g_originalMethodTable;
#endif
// 1 (default) = lock-free lookup; 0 = the previous locked lookup, kept only for a same-launch A/B.
extern "C" __declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_FastOriginalLookup;
void DumpDredOnce(IDXGISwapChain* swapChain, HRESULT presentHr);
void EnableDredOnce();
void InstallAdapterHook(IDXGIAdapter* adapter);
void InstallCommandQueueDiagHook(ID3D12CommandQueue* queue);
void InstallDepthCaptureHooks(ID3D12Device* device);
bool DescriptorMutationHooksReady(ID3D12Device* device);
void InstallOSHooks();
void InstallOutputHook(IDXGIOutput* output);
void RememberDredDevice(ID3D12Device* device);
void RememberDredDeviceFromSwapChain(IDXGISwapChain* swapChain);
void UpdateCursorCapture(HWND hwnd);

// Brace-initialised atomics. Worth a line of its own: the pass that generated the rest of this header
// matches a name followed by `(`, `[`, `=` or `;` and these are followed by `{`, so it reported them as
// unresolvable while the build failed on them. Copied from the definitions in DepthCapture.cpp.
#include <atomic>
extern std::atomic<uint64_t> g_eclTotalCalls;
extern std::atomic<uint64_t> g_eclTotalLists;
extern std::atomic<uint64_t> g_eclPresentCalls;
extern std::atomic<uint64_t> g_eclPresentLists;
extern std::atomic<uint32_t> g_eclDistinctQueues;
extern std::atomic<uint32_t> g_presentHeight;

extern std::atomic<ID3D12CommandQueue*> g_sceneDepthWriterQueue;
extern std::atomic<ID3D12Resource*> g_sceneDepthRes;
extern std::mutex g_sceneDepthRefMutex;
extern std::atomic<UINT> g_sceneDepthFmt;
extern std::atomic<UINT> g_sceneDepthH;
extern std::atomic<UINT> g_sceneDepthState;
extern std::atomic<UINT> g_sceneDepthW;
extern std::atomic<bool>     g_sceneDepthSeen;
extern std::atomic<uint32_t> g_presentWidth;
extern std::atomic<uint32_t> g_sceneDepthMissFrames;
extern std::atomic<uint64_t> g_sceneDepthArea;
extern std::atomic<void*> g_presentQueue;
uintptr_t MakeVtableSlotKey(void** vtable, size_t slot);

// LAST IN THE FILE, deliberately. A template is compiled where it is instantiated, so every name it
// uses must already be declared above it -- putting it at the top produced four "undeclared
// identifier" errors inside the header itself.
// A TEMPLATE, so the definition and not a declaration -- DisplayModes.cpp instantiates it with two
// different function-pointer types. The map it reads is declared above (defined in SwapChain.cpp).
// Hot hooks call this per API call, from every render job thread: the published mirror answers
// without a lock; only a key the mirror could not hold falls back to the locked map.
#if CVR_RUST_VRIK
// The registry is rust/crates/cvr-engine/src/vtable.rs (PatchVtableMethod forwards there too).
extern "C" void* cvr_vtable_original(void** vtable, size_t slot);
template <typename T>
T GetOriginalMethod(void** vtable, size_t slot) {
    return reinterpret_cast<T>(cvr_vtable_original(vtable, slot));
}
#else
template <typename T>
T GetOriginalMethod(void** vtable, size_t slot) {
    const uintptr_t key = MakeVtableSlotKey(vtable, slot);
    if (CyberpunkVR_FastOriginalLookup.load(std::memory_order_relaxed))
        if (void* published = g_originalMethodTable.Find(key)) return reinterpret_cast<T>(published);
    std::lock_guard<std::mutex> lock(g_vtableMutex);
    auto it = g_originalVtableMethods.find(key);
    return it != g_originalVtableMethods.end() ? reinterpret_cast<T>(it->second) : nullptr;
}
#endif

// One call site's original, for the hooks the render jobs hit per draw (CopyDescriptorsSimple alone
// runs tens of thousands of times a frame, and the table probe was a fifth of that hook's own cost).
// The first vtable a site resolves is kept with its original: the registry is append-only and the
// first original wins, so that pair can never change. Any other vtable takes the registry as before,
// and so does CyberpunkVR_FastOriginalLookup=0, which keeps the locked A/B arm meaningful.
template <typename T>
struct OriginalSiteCache {
    struct Pair { void** table; T fn; };
    std::atomic<const Pair*> pair{nullptr};
};
template <typename T>
T GetOriginalMethodCached(OriginalSiteCache<T>& cache, void** vtable, size_t slot) {
    const auto* known = cache.pair.load(std::memory_order_acquire);
    if (known && known->table == vtable && CyberpunkVR_FastOriginalLookup.load(std::memory_order_relaxed)) return known->fn;
    const T fn = GetOriginalMethod<T>(vtable, slot);
    if (fn && !known) {
        // One small allocation per call site, kept for the life of the process.
        const auto* fresh = new (std::nothrow) typename OriginalSiteCache<T>::Pair{vtable, fn};
        const typename OriginalSiteCache<T>::Pair* expected = nullptr;
        if (fresh && !cache.pair.compare_exchange_strong(expected, fresh, std::memory_order_acq_rel)) delete fresh;
    }
    return fn;
}
