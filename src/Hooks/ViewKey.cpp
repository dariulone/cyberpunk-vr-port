#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
#include "Utils/DebugGate.hpp"
// ViewKey -- which of the engine's views is recording on this thread.
//
// One fact, one hook: the frame-graph node dispatcher is entered with a work context whose view ctx
// carries the view's identity hash, and knowing it is what lets everything downstream say "this is
// MAIN" or "this is the second eye".
//
// STAGE PostStereo, AND THAT IS THE WHOLE POINT OF HAVING STAGES. The stereo module hooks this same
// dispatcher (mirror epilogue, profiler, view identity) and MinHook permits exactly ONE hook per
// target -- whoever installs second gets MH_ERROR_ALREADY_CREATED and silently does nothing. The
// old boot expressed that by calling InitStereoOnce() on the line above this install and trusting
// nobody would ever reorder the two. It is now a declared stage, so the constraint survives being
// read by someone who does not already know it. When the stereo module owns the site, this hook is
// SKIPPED rather than installed -- and the log says so, where the installer's bare `return true`
// used to report a success that never happened.

#include "Core/VrCoreShared.hpp"
#include "Hooks/Hook.hpp"
#include "Utils/AobScanner.hpp"

#include <windows.h>
#include <MinHook.h>
#include <cstdint>

#if !CVR_RUST_VRIK
extern "C" __declspec(dllexport) int CyberpunkVR_IsMainViewActive();
extern "C" __declspec(dllexport) extern int CyberpunkVR_StereoModuleLoaded;
extern void InitGameModuleInfo();
extern uintptr_t g_gameModuleBase;
#endif

#if CVR_RUST_VRIK
// The hook and the thread's view are rust/crates/cvr-rig/src/vrik/view_key.rs; the exports and the registration stay.
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/used_moved_exports_rest.rs.
#else
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugViewKeyMainNodes = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugViewKeyOtherNodes = 0;
#endif
#if !CVR_RUST_VRIK   // registered from rust/crates/cvr-render/src/hook_table.rs
CVR_HOOK_IF("ViewKey", ::cvr::hooks::Stage::PostStereo, 10, cvr_hook_view_key_install, cvr_hook_view_key_wanted);
#endif
#else
namespace {
// The site is the stereo module's when it is loaded; asking here keeps the answer out of the
// installer, so the registry can report "skipped" instead of a false "ok".
bool ViewKeyWanted() { return CyberpunkVR_StereoModuleLoaded == 0; }
}  // namespace

constexpr uintptr_t NODE_DISPATCH_RVA = 0x332F90;
using NodeDispatchFnP = uint8_t (__fastcall*)(uintptr_t* node, uint8_t* work_context, void* args);
static NodeDispatchFnP g_origNodeDispatch = nullptr;
thread_local uint64_t t_dxgiViewKey = 0;
thread_local bool     t_dxgiViewKeyKnown = false;
static std::atomic<bool> s_viewKeyObserved{false};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugViewKeyMainNodes = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugViewKeyOtherNodes = 0;

// The stereo module hooks this same dispatcher (mirror epilogue, profiler, view identity) and
// MinHook permits exactly one hook per target -- whichever installs second gets
// MH_ERROR_ALREADY_CREATED and silently does nothing. sync_stereo installs first (it boots from
// the DXGI factory export, this pass runs 8 s later on the worker thread) and already exports
// the identity, so the answer is taken from there rather than hooked a second time.
extern "C" __declspec(dllexport) int CyberpunkVR_IsMainViewActive();
extern "C" __declspec(dllexport) int CyberpunkVR_GetActiveViewKey(unsigned long long* out);
// Defined further down with the boot code; declared here because the two accessors below
// have to know whether the stereo module is the one answering.
extern "C" __declspec(dllexport) extern int CyberpunkVR_StereoModuleLoaded;

// 1 while a node of the MAIN view is recording on this thread.
extern "C" __declspec(dllexport) int CyberpunkVR_IsMainViewRecording() {
    if (CyberpunkVR_StereoModuleLoaded) return CyberpunkVR_IsMainViewActive();
    return (t_dxgiViewKeyKnown && t_dxgiViewKey == 0) ? 1 : 0;
}
// 0 until the dispatcher hook is in and has actually seen view-carrying nodes, so callers
// can fall back instead of silently treating "no information" as "not MAIN".
extern "C" __declspec(dllexport) int CyberpunkVR_ViewKeyHookActive() {
    if (CyberpunkVR_StereoModuleLoaded) return 1;
    return (g_origNodeDispatch != nullptr &&
            s_viewKeyObserved.load(std::memory_order_relaxed))
           ? 1 : 0;
}

static uint8_t __fastcall Detour_ViewKeyDispatch(uintptr_t* node, uint8_t* work_context,
                                                 void* args) {
    uint64_t key = 0;
    bool known = false;
    if (work_context) {
        __try {
            const uintptr_t ctx = *reinterpret_cast<uintptr_t*>(work_context + 0x18);
            if (ctx) {
                key = *reinterpret_cast<uint64_t*>(ctx + 0x28);
                known = true;
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) { known = false; }
    }
    // Saved and restored: the dispatcher re-enters itself (the scene driver runs one nested
    // dispatch per pass), so a nested node must not leave the parent's view mis-tagged.
    const bool prevKnown = t_dxgiViewKeyKnown;
    const uint64_t prevKey = t_dxgiViewKey;
    t_dxgiViewKeyKnown = known;
    t_dxgiViewKey = key;
    if (known) {
        s_viewKeyObserved.store(true,std::memory_order_relaxed);
        if (key == 0) CVR_DIAGNOSTIC(++CyberpunkVR_DebugViewKeyMainNodes);
        else          CVR_DIAGNOSTIC(++CyberpunkVR_DebugViewKeyOtherNodes);
    }
    const uint8_t r = g_origNodeDispatch(node, work_context, args);
    t_dxgiViewKeyKnown = prevKnown;
    t_dxgiViewKey = prevKey;
    return r;
}

bool InstallViewKeyHook() {
    // Already covered: sync_stereo hooked this dispatcher first and exports the identity,
    // which CyberpunkVR_IsMainViewRecording now defers to. Hooking again would only earn
    // MH_ERROR_ALREADY_CREATED.
    if (CyberpunkVR_StereoModuleLoaded) return true;
    InitGameModuleInfo();
    if (!g_gameModuleBase) return false;
    static bool s_mhReady = false;
    if (!s_mhReady) {
        const MH_STATUS st = MH_Initialize();
        if (st != MH_OK && st != MH_ERROR_ALREADY_INITIALIZED) return false;
        s_mhReady = true;
    }
    void* target = reinterpret_cast<void*>(g_gameModuleBase + NODE_DISPATCH_RVA);
    if (MH_CreateHook(target, reinterpret_cast<void*>(&Detour_ViewKeyDispatch),
                      reinterpret_cast<void**>(&g_origNodeDispatch)) != MH_OK) {
        return false;
    }
    return MH_EnableHook(target) == MH_OK;
}

CVR_HOOK_IF("ViewKey", ::cvr::hooks::Stage::PostStereo, 10, InstallViewKeyHook, ViewKeyWanted);
#endif
#endif
