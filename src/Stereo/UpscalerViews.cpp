#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
// UpscalerViews -- one temporal-upscaler history per eye for the upscalers that keep a single
// engine-wide context.
//
// DLSS got its second history from a second Streamline viewport (Dlss.cpp). FSR 2 needs nothing: it is
// the engine's own compute passes (CRenderNode_ApplyFSR2) and its history resources are named per view.
// The other three keep ONE context in a renderer-wide manager, and both eyes dispatch into it:
//
//   FSR 3.0   ffx_fsr3_x64.dll          FfxFsr3Context at manager+44, manager = renderer+18032
//   FSR 3.1+  amd_fidelityfx_dx12.dll   ffxContext of the manager at renderer+18024 (FSR 4 goes here too)
//   XeSS      libxess.dll               xess_context_handle_t at manager+32
//
// With one context each eye's frame is accumulated onto the other eye's history, which is an IPD-wide
// ghost on everything near. So a second context is made next to the engine's -- inside the engine's own
// create call, from the description it passes, so nothing has to be copied or kept alive -- and the
// second eye's dispatches are pointed at it. Destroy and every setting call follow the engine's
// context. All of these run under the engine's manager lock, which is what serialises them here.
//
// The second eye is t_view_side == 1, published by the node dispatcher for the node that dispatches.
// Framegen reads its depth and motion inputs here as well, before the library changes their state.

#include "Stereo/UpscalerViews.hpp"
#include "Stereo/StereoInternal.hpp"
#include "Stereo/EngineRvas.hpp"
#include "Stereo/DetourRegistry.hpp"
#include "Framegen/NativeInputs.hpp"
#include "Utils/TestMarker.hpp"
#include "MinHook.h"
#include <windows.h>
#include <d3d12.h>
#include <atomic>
#include <cstdint>
#include <cstring>
#include <memory>
#include <mutex>
#include <new>

#if !CVR_RUST_VRIK
extern void Log(const char*, ...);
extern "C" ID3D12Device* CyberpunkVR_GetGameDevice();
#endif

// 1 = the second eye uses its own context (default); 0 = both eyes share the engine's, as before.
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs.
#else
extern "C" __declspec(dllexport) int32_t CyberpunkVR_UpscalerPerEye = 1;
// Per library (0 FSR 3.0, 1 FFX API, 2 XeSS): second contexts created, first-eye dispatches,
// second-eye dispatches given their own context, failures (create or install).
extern "C" __declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_UpscalerPerEyeStats[3][4]{};
#endif

#if CVR_RUST_VRIK
// THE PER-EYE CONTEXTS ARE RUST'S (rust/crates/cvr-render/src/upscaler_views.rs): the switch and counters above, the
// providers detour's registration and two services stay here.
// The node dispatcher's view side on this thread, and framegen's capture of the upscaler inputs.
// The Rust build registers these detours from Rust (rust/crates/cvr-render/src/engine_detours.rs).
#else
namespace cvr::stereo::upscaler_views {
namespace {
enum Library { kFsr3, kFfxApi, kXess };
enum Stat { kCreated, kFirstEye, kSecondEye, kFailed };
void Count(Library library, Stat stat) { CyberpunkVR_UpscalerPerEyeStats[library][stat].fetch_add(1, std::memory_order_relaxed); }

bool SecondEye() { return CyberpunkVR_UpscalerPerEye != 0 && cvr::detail::t_view_side == 1; }

template <class Fn>
bool HookExport(HMODULE module, const char* name, Fn detour, Fn* original) {
    void* target = module ? reinterpret_cast<void*>(GetProcAddress(module, name)) : nullptr;
    if (!target) return false;
    if (MH_CreateHook(target, reinterpret_cast<void*>(detour), reinterpret_cast<void**>(original)) != MH_OK) return false;
    if (MH_EnableHook(target) != MH_OK) { MH_RemoveHook(target); *original = nullptr; return false; }
    return true;
}

// ---- FSR 3.0 ------------------------------------------------------------------------------------
// FidelityFX SDK 1.0 as the game ships it. The context description is 0x208 bytes: flags and three
// sizes, then the shared-resource, upscaling and frame-interpolation backend interfaces of 0x98 bytes
// each (sub_141D4EE90 fills them at +0x20, +0xB8 and +0x150), the message callback and the format.
// The engine builds all three interfaces on ONE scratch buffer sized for four backend contexts. The
// second context gets its own scratch and interfaces instead of the engine's spare slots, so it cannot
// run the engine out of them.
namespace fsr3 {
using Create = int32_t (*)(void* context, void* description);
using Destroy = int32_t (*)(void* context);
using Dispatch = int32_t (*)(void* context, const void* description);
using ScratchSize = size_t (*)(size_t maxContexts);
using Interface = int32_t (*)(void* backend, void* device, void* scratch, size_t size, size_t maxContexts);
constexpr size_t kDescriptionBytes = 0x208, kInterfaceBytes = 0x98, kMaxContexts = 4;
constexpr size_t kInterfaces[3] = {0x20, 0xB8, 0x150};
// The engine reserves 1,639,012 bytes for its FfxFsr3Context (manager+44 to manager+1639056).
constexpr size_t kContextBytes = 0x200000;
Create originalCreate{};
Destroy originalDestroy{};
Dispatch originalDispatch{};
ScratchSize scratchSize{};
Interface makeInterface{};
HMODULE backendModule{};
std::mutex mutex;
void* engine{};
std::unique_ptr<uint32_t[]> context;
std::unique_ptr<uint8_t[]> scratch;
std::atomic<void*> second{};

bool InBackend(const void* p) {
    if (!backendModule) return false;
    const auto base = reinterpret_cast<uintptr_t>(backendModule);
    const auto* dos = reinterpret_cast<const IMAGE_DOS_HEADER*>(base);
    const auto* nt = reinterpret_cast<const IMAGE_NT_HEADERS*>(base + dos->e_lfanew);
    const auto at = reinterpret_cast<uintptr_t>(p);
    return at >= base && at < base + nt->OptionalHeader.SizeOfImage;
}

void Release() {
    if (void* own = second.exchange(nullptr)) originalDestroy(own);
    context.reset();
    scratch.reset();
    engine = nullptr;
}

int32_t HookCreate(void* engineContext, void* description) {
    const int32_t result = originalCreate(engineContext, description);
    if (result != 0 || !description || !scratchSize || !makeInterface) return result;
    std::lock_guard lock(mutex);
    Release();
    // Only take a layout that is the one this was written against: every interface must start with a
    // function of the game's ffx_backend_dx12_x64.dll.
    for (size_t at : kInterfaces) {
        void* first{};
        std::memcpy(&first, static_cast<uint8_t*>(description) + at, sizeof(first));
        if (!InBackend(first)) {
            Log("[upscaler] FSR 3.0: unexpected context description, the second eye keeps the shared history\n");
            Count(kFsr3, kFailed);
            return result;
        }
    }
    ID3D12Device* device = CyberpunkVR_GetGameDevice();
    const size_t bytes = scratchSize(kMaxContexts);
    uint8_t copy[kDescriptionBytes];
    std::memcpy(copy, description, sizeof(copy));
    scratch.reset(new (std::nothrow) uint8_t[bytes]());
    context.reset(new (std::nothrow) uint32_t[kContextBytes / 4]());
    bool ok = device && scratch && context;
    for (size_t at : kInterfaces)
        ok = ok && makeInterface(copy + at, device, scratch.get(), bytes, kMaxContexts) == 0;
    if (ok && originalCreate(context.get(), copy) == 0) {
        engine = engineContext;
        second.store(context.get(), std::memory_order_release);
        Count(kFsr3, kCreated);
        Log("[upscaler] FSR 3.0: second-eye context created (scratch %zu bytes)\n", bytes);
    } else {
        context.reset();
        scratch.reset();
        Count(kFsr3, kFailed);
        Log("[upscaler] FSR 3.0: second-eye context failed, the second eye keeps the shared history\n");
    }
    return result;
}

int32_t HookDestroy(void* engineContext) {
    {
        std::lock_guard lock(mutex);
        if (engineContext == engine) Release();
    }
    return originalDestroy(engineContext);
}

int32_t HookDispatch(void* engineContext, const void* description) {
    ID3D12GraphicsCommandList* list{};
    if (description) std::memcpy(&list, description, sizeof(list));   // FfxCommandList is the first member
    if (list) cvr::framegen::CaptureUpscalerInputs(list);
    if (engineContext == engine && SecondEye()) {
        if (void* own = second.load(std::memory_order_acquire)) {
            Count(kFsr3, kSecondEye);
            return originalDispatch(own, description);
        }
    }
    if (engineContext == engine) Count(kFsr3, kFirstEye);
    return originalDispatch(engineContext, description);
}

void Install() {
    static bool done = false;
    if (done) return;
    const HMODULE module = GetModuleHandleW(L"ffx_fsr3_x64.dll");
    backendModule = GetModuleHandleW(L"ffx_backend_dx12_x64.dll");
    if (!module || !backendModule) return;
    done = true;
    scratchSize = reinterpret_cast<ScratchSize>(GetProcAddress(backendModule, "ffxGetScratchMemorySizeDX12"));
    makeInterface = reinterpret_cast<Interface>(GetProcAddress(backendModule, "ffxGetInterfaceDX12"));
    const bool ok = scratchSize && makeInterface &&
        HookExport(module, "ffxFsr3ContextCreate", &HookCreate, &originalCreate) &&
        HookExport(module, "ffxFsr3ContextDestroy", &HookDestroy, &originalDestroy) &&
        HookExport(module, "ffxFsr3ContextDispatchUpscale", &HookDispatch, &originalDispatch);
    if (!ok) Count(kFsr3, kFailed);
    Log("[upscaler] FSR 3.0 per-eye hooks %s\n", ok ? "installed" : "FAILED");
}
}  // namespace fsr3

// ---- FSR 3.1 / FSR 4 (FidelityFX API) -----------------------------------------------------------
// Descriptions are chains of {uint64 type, next}. Only the upscaler is doubled; frame generation and
// the swap chain stay single. ffxDispatch's upscale description has the command list right after the
// header.
namespace ffxapi {
struct Header { uint64_t type; Header* next; };
constexpr uint64_t kCreateUpscale = 0x00010000, kDispatchUpscale = 0x00010001, kReactiveMask = 0x00010006;
using Create = uint32_t (*)(void** context, Header* description, const void* allocation);
using Destroy = uint32_t (*)(void** context, const void* allocation);
using Configure = uint32_t (*)(void** context, const Header* description);
using Dispatch = uint32_t (*)(void** context, const Header* description);
Create originalCreate{};
Destroy originalDestroy{};
Configure originalConfigure{};
Dispatch originalDispatch{};
std::mutex mutex;
void* engine{};               // the engine's upscale context handle
void* second{};               // ours
std::atomic<void**> secondSlot{};

bool Upscale(const Header* description) { return description && description->type == kCreateUpscale; }

uint32_t HookCreate(void** context, Header* description, const void* allocation) {
    const uint32_t result = originalCreate(context, description, allocation);
    if (result != 0 || !context || !*context || !Upscale(description)) return result;
    std::lock_guard lock(mutex);
    if (second) {
        secondSlot.store(nullptr);
        originalDestroy(&second, allocation);
        second = nullptr;
    }
    void* own{};
    if (originalCreate(&own, description, allocation) == 0 && own) {
        engine = *context;
        second = own;
        secondSlot.store(&second, std::memory_order_release);
        Count(kFfxApi, kCreated);
        Log("[upscaler] FidelityFX API: second-eye upscale context created\n");
    } else {
        engine = *context;
        Count(kFfxApi, kFailed);
        Log("[upscaler] FidelityFX API: second-eye upscale context failed, the second eye keeps the shared history\n");
    }
    return result;
}

uint32_t HookDestroy(void** context, const void* allocation) {
    {
        std::lock_guard lock(mutex);
        if (context && *context && *context == engine) {
            secondSlot.store(nullptr);
            if (second) originalDestroy(&second, allocation);
            second = engine = nullptr;
        }
    }
    return originalDestroy(context, allocation);
}

uint32_t HookConfigure(void** context, const Header* description) {
    const uint32_t result = originalConfigure(context, description);
    if (context && *context && *context == engine)
        if (void** own = secondSlot.load(std::memory_order_acquire)) originalConfigure(own, description);
    return result;
}

uint32_t HookDispatch(void** context, const Header* description) {
    const bool upscale = description && (description->type == kDispatchUpscale || description->type == kReactiveMask);
    if (!upscale || !context || !*context || *context != engine) return originalDispatch(context, description);
    if (description->type == kDispatchUpscale) {
        ID3D12GraphicsCommandList* list{};
        std::memcpy(&list, reinterpret_cast<const uint8_t*>(description) + sizeof(Header), sizeof(list));
        if (list) cvr::framegen::CaptureUpscalerInputs(list);
    }
    if (SecondEye()) {
        if (void** own = secondSlot.load(std::memory_order_acquire)) {
            if (description->type == kDispatchUpscale) Count(kFfxApi, kSecondEye);
            return originalDispatch(own, description);
        }
    }
    if (description->type == kDispatchUpscale) Count(kFfxApi, kFirstEye);
    return originalDispatch(context, description);
}

std::atomic<bool> installed{false};
void Install(HMODULE module) {
    if (!module || installed.exchange(true)) return;
    const bool ok = HookExport(module, "ffxCreateContext", &HookCreate, &originalCreate) &&
        HookExport(module, "ffxDestroyContext", &HookDestroy, &originalDestroy) &&
        HookExport(module, "ffxConfigure", &HookConfigure, &originalConfigure) &&
        HookExport(module, "ffxDispatch", &HookDispatch, &originalDispatch);
    if (!ok) Count(kFfxApi, kFailed);
    Log("[upscaler] FidelityFX API per-eye hooks %s\n", ok ? "installed" : "FAILED");
}
}  // namespace ffxapi

// ---- XeSS ---------------------------------------------------------------------------------------
// The engine creates the context, sets four context parameters, then on every mode change sets the
// velocity scale and calls xessD3D12Init with library-owned heaps (no heap of the engine is named, so
// two contexts cannot alias each other's memory). Each of those is repeated on the second context.
namespace xess {
using Create = int32_t (*)(ID3D12Device* device, void** context);
using Destroy = int32_t (*)(void* context);
using Init = int32_t (*)(void* context, const void* parameters);
using Execute = int32_t (*)(void* context, ID3D12GraphicsCommandList* list, const void* parameters);
using VelocityScale = int32_t (*)(void* context, float x, float y);
using ParameterF = int32_t (*)(void* context, uint32_t parameter, float value);
Create originalCreate{};
Destroy originalDestroy{};
Init originalInit{};
Execute originalExecute{};
VelocityScale originalVelocityScale{};
ParameterF originalParameterF{};
std::mutex mutex;
void* engine{};
std::atomic<void*> second{};

int32_t HookCreate(ID3D12Device* device, void** context) {
    const int32_t result = originalCreate(device, context);
    if (result != 0 || !context || !*context) return result;
    std::lock_guard lock(mutex);
    if (void* old = second.exchange(nullptr)) originalDestroy(old);
    void* own{};
    engine = *context;
    if (originalCreate(device, &own) == 0 && own) {
        second.store(own, std::memory_order_release);
        Count(kXess, kCreated);
        Log("[upscaler] XeSS: second-eye context created\n");
    } else {
        Count(kXess, kFailed);
        Log("[upscaler] XeSS: second-eye context failed, the second eye keeps the shared history\n");
    }
    return result;
}

int32_t HookDestroy(void* context) {
    {
        std::lock_guard lock(mutex);
        if (context && context == engine) {
            if (void* own = second.exchange(nullptr)) originalDestroy(own);
            engine = nullptr;
        }
    }
    return originalDestroy(context);
}

void* Mirror(void* context) { return context && context == engine ? second.load(std::memory_order_acquire) : nullptr; }

int32_t HookInit(void* context, const void* parameters) {
    const int32_t result = originalInit(context, parameters);
    if (void* own = Mirror(context)) {
        if (originalInit(own, parameters) != 0) {
            Count(kXess, kFailed);
            Log("[upscaler] XeSS: second-eye init failed, the second eye keeps the shared history\n");
            std::lock_guard lock(mutex);
            if (void* failed = second.exchange(nullptr)) originalDestroy(failed);
        }
    }
    return result;
}

int32_t HookVelocityScale(void* context, float x, float y) {
    const int32_t result = originalVelocityScale(context, x, y);
    if (void* own = Mirror(context)) originalVelocityScale(own, x, y);
    return result;
}

int32_t HookParameterF(void* context, uint32_t parameter, float value) {
    const int32_t result = originalParameterF(context, parameter, value);
    if (void* own = Mirror(context)) originalParameterF(own, parameter, value);
    return result;
}

int32_t HookExecute(void* context, ID3D12GraphicsCommandList* list, const void* parameters) {
    if (list) cvr::framegen::CaptureUpscalerInputs(list);
    if (context && context == engine && SecondEye()) {
        if (void* own = second.load(std::memory_order_acquire)) {
            Count(kXess, kSecondEye);
            return originalExecute(own, list, parameters);
        }
    }
    if (context && context == engine) Count(kXess, kFirstEye);
    return originalExecute(context, list, parameters);
}

void Install() {
    static bool done = false;
    if (done) return;
    const HMODULE module = GetModuleHandleW(L"libxess.dll");
    if (!module) return;
    done = true;
    const bool ok = HookExport(module, "xessD3D12CreateContext", &HookCreate, &originalCreate) &&
        HookExport(module, "xessDestroyContext", &HookDestroy, &originalDestroy) &&
        HookExport(module, "xessD3D12Init", &HookInit, &originalInit) &&
        HookExport(module, "xessD3D12Execute", &HookExecute, &originalExecute) &&
        HookExport(module, "xessSetVelocityScale", &HookVelocityScale, &originalVelocityScale) &&
        HookExport(module, "xessSetContextParameterF", &HookParameterF, &originalParameterF);
    if (!ok) Count(kXess, kFailed);
    Log("[upscaler] XeSS per-eye hooks %s\n", ok ? "installed" : "FAILED");
}
}  // namespace xess
}  // namespace

void Install() {
    fsr3::Install();
    xess::Install();
    // 2.5 links the FidelityFX loader statically (it hands upscale and frame-generation contexts on to
    // amd_fidelityfx_upscaler_dx12 / _framegeneration_dx12), so it is loaded before any plugin: its exports are
    // the ones the game calls. 2.31 loaded amd_fidelityfx_dx12 itself (see the loader detour below).
    HMODULE ffx = GetModuleHandleW(L"amd_fidelityfx_loader_dx12.dll");
    if (!ffx) ffx = GetModuleHandleW(L"amd_fidelityfx_dx12.dll");
    ffxapi::Install(ffx);
}
}  // namespace cvr::stereo::upscaler_views

namespace cvr::detail {
// 2.31 took the FidelityFX API entry points in sub_141D4DEA0 (GetProcAddress after its own LoadLibrary of
// amd_fidelityfx_dx12) and hooked there. 2.5 links amd_fidelityfx_loader_dx12 statically and has no such
// function: Install() above hooks the loaded module's exports instead.

// sub_142287A20 asks the FidelityFX API for its upscaler providers (ffxQuery GET_VERSIONS) and keeps
// one version id per family in the manager: +0x90 "2.x", +0x98 "3.x", +0xA0 "4.x", -1 when absent.
// The game offers FSR 4 only when +0xA0 is set (sub_141D40FF0), which needs an RDNA 4 driver; the
// ResolutionScaling "FSR4" path then creates its context with that version as an override
// (sub_141D4C7E0) and runs it through CRenderNode_ApplyFSR3.
//
// TEST ONLY, with cvr_simulate_fsr4.txt next to the executable: the 3.x provider is put in the 4.x slot,
// so any GPU runs exactly that path -- menu entry, ApplyFSR3 node, ffxCreateContext with the override,
// ffxConfigure, ffxDispatch, the per-eye context and framegen's capture -- with FSR 3.1 shaders.
using FfxApiProvidersFn = __int64(__fastcall*)(int64_t*);
static FfxApiProvidersFn g_orig_ffx_api_providers = nullptr;
static __int64 __fastcall Detour_FfxApiProviders(int64_t* manager) {
    const __int64 result = g_orig_ffx_api_providers(manager);
    __try {
        Log("[upscaler] FidelityFX API upscalers: 2.x=%llX 3.x=%llX 4.x=%llX default=%llX\n",
            (unsigned long long)manager[18], (unsigned long long)manager[19], (unsigned long long)manager[20],
            (unsigned long long)manager[21]);
        if (cvr::TestMarker("cvr_simulate_fsr4.txt") && manager[20] == -1 && manager[19] != -1) {
            manager[20] = manager[19];
            Log("[upscaler] TEST: FSR 4 simulated with the FSR 3.1 provider (cvr_simulate_fsr4.txt)\n");
        }
    } __except (EXCEPTION_EXECUTE_HANDLER) {}
    return result;
}
CVR_DETOUR("[upscaler] FidelityFX API providers sub_142287A20", FFX_API_PROVIDERS_RVA, Detour_FfxApiProviders, g_orig_ffx_api_providers)
}  // namespace cvr::detail
#endif
#endif
