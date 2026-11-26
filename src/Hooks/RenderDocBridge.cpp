#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
// RenderDocBridge -- let this plugin's detours coexist with a RenderDoc capture layer.
//
// THE FAILURE THIS FIXES, from RenderDoc's own log rather than from a guess. Launching the game under
// RenderDoc printed this once per call, endlessly:
//
//     nvapi_hooks.cpp(335) - Error - Didn't pass RenderDoc-wrapped device to nvapi function
//
// and Cyberpunk then refused to start, reporting that ray tracing failed to load -- with ray tracing
// switched off in its settings. The ray-tracing message is a symptom: what actually failed was NVAPI
// initialisation, because the D3D12 device it was handed was not RenderDoc's wrapper.
//
// THE CHAIN. HookFactoryExportsIn() resolves CreateDXGIFactory* with the plain loader and MinHooks the
// address it gets. RenderDoc has already patched those entry points, but a MinHook trampoline built from
// the RAW address bypasses that patch -- so our detour returns an UNWRAPPED factory. Every adapter
// enumerated from it is unwrapped, the device created from that adapter is unwrapped, and RenderDoc is
// left complaining about the device it never got to wrap. One plain GetProcAddress at the top of the
// chain is enough to make the whole capture useless.
//
// THE FIX is not to hook less but to AIM DIFFERENTLY: resolve our detour targets through RenderDoc when
// RenderDoc is present, so our trampolines call its serializer and it sees the creation calls. This needs
// an export RenderDoc does not ship -- RENDERDOC_CPVR_GetHookedProcAddress, a thin wrapper over its
// internal Hooked_GetProcAddress, added in this project's own RenderDoc build (renderdoc-src,
// renderdoc/os/win32/win32_hook.cpp). Without that export the resolver falls back to the raw address and
// says so in the log, which is the difference between a capture that is empty and one that is silently
// wrong.
//
// When renderdoc.dll is not resident -- the ordinary case -- both functions are a plain GetProcAddress
// and cost one GetModuleHandleA.

#include "Core/VrCoreShared.hpp"

#include <windows.h>

#if CVR_RUST_VRIK
// The bridge is rust/crates/cvr-gpu/src/renderdoc_bridge.rs.
#else
extern void Log(const char* fmt, ...);

namespace {

// Logged once per function name, because a resolution that silently differs from what the author
// expected is exactly the class of bug this file exists to fix.
bool ShouldLog(const char* functionName) {
    static const char* seen[16] = {};
    static int count = 0;
    for (int i = 0; i < count; ++i)
        if (seen[i] == functionName || (seen[i] && strcmp(seen[i], functionName) == 0)) return false;
    if (count < 16) seen[count++] = functionName;
    return true;
}

}  // namespace

namespace cvr {

// The address our MinHook detours should target: RenderDoc's serializer when it has hooked this
// function, the loader's export otherwise.
void* RenderDocResolveHookTarget(HMODULE module, const char* functionName) {
    if (!module || !functionName) return nullptr;

    void* raw = reinterpret_cast<void*>(GetProcAddress(module, functionName));

    const HMODULE renderdoc = GetModuleHandleA("renderdoc.dll");
    if (!renderdoc) return raw;                     // no capture layer: nothing to coexist with

    // Streamline's device import is redirected directly to RenderDoc. The
    // loader export can remain unused even when GetHookedProcAddress returns
    // it. Hook the actual imported serializer before the first game device.
    if (strcmp(functionName,"D3D12CreateDevice")==0) {
        const auto* exe=reinterpret_cast<const uint8_t*>(GetModuleHandleW(nullptr));
        const auto* dos=reinterpret_cast<const IMAGE_DOS_HEADER*>(exe);
        const auto* nt=reinterpret_cast<const IMAGE_NT_HEADERS*>(exe+dos->e_lfanew);
        const auto& directory=nt->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT];
        const auto* imports=reinterpret_cast<const IMAGE_IMPORT_DESCRIPTOR*>(exe+directory.VirtualAddress);
        for(size_t i=0;directory.VirtualAddress && i<directory.Size/sizeof(*imports) && imports[i].Name;++i) {
            if(!imports[i].OriginalFirstThunk || !imports[i].FirstThunk)continue;
            const auto* names=reinterpret_cast<const IMAGE_THUNK_DATA*>(exe+imports[i].OriginalFirstThunk);
            const auto* slots=reinterpret_cast<const IMAGE_THUNK_DATA*>(exe+imports[i].FirstThunk);
            for(size_t j=0;names[j].u1.AddressOfData;++j) {
                if(IMAGE_SNAP_BY_ORDINAL(names[j].u1.Ordinal))continue;
                const auto* name=reinterpret_cast<const IMAGE_IMPORT_BY_NAME*>(exe+names[j].u1.AddressOfData);
                if(strcmp(reinterpret_cast<const char*>(name->Name),functionName))continue;
                auto* imported=reinterpret_cast<void*>(slots[j].u1.Function);HMODULE owner{};
                if(GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS|GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                    reinterpret_cast<LPCWSTR>(imported),&owner) && owner==renderdoc) {
                    if(ShouldLog(functionName))Log("[RenderDoc] '%s' routed through game import %p (loader %p)\n",functionName,imported,raw);
                    return imported;
                }
            }
        }
    }

    using ResolveFn = FARPROC(__cdecl*)(HMODULE, const char*);
    auto resolve = reinterpret_cast<ResolveFn>(
        GetProcAddress(renderdoc, "RENDERDOC_CPVR_GetHookedProcAddress"));
    if (!resolve) {
        // A stock RenderDoc build. Say so plainly: the capture will be launched, the game may even run,
        // and the capture will contain an unwrapped device -- which is worse than an outright failure
        // because it looks like data.
        if (ShouldLog(functionName))
            Log("[RenderDoc] renderdoc.dll is resident but RENDERDOC_CPVR_GetHookedProcAddress is "
                "missing -- '%s' stays raw, so captures will not see our wrapped objects. Use the "
                "renderdoc.dll built from renderdoc-src.\n", functionName);
        return raw;
    }

    void* hooked = reinterpret_cast<void*>(resolve(module, functionName));
    if (!hooked) return raw;                        // RenderDoc does not hook this one
    if (ShouldLog(functionName))
        Log("[RenderDoc] '%s' routed through the serializer %p (loader %p)\n",
            functionName, hooked, raw);
    return hooked;
}

// The loader's own export, for the rare detour that must reach the real entry point regardless of the
// capture layer. Separate from the above so a caller has to choose deliberately.
void* RenderDocResolveRawTarget(HMODULE module, const char* functionName) {
    if (!module || !functionName) return nullptr;
    return reinterpret_cast<void*>(GetProcAddress(module, functionName));
}

// ---- WHY THE GAME WOULD NOT START UNDER RENDERDOC AT ALL ----------------------------------------
//
// The address routing above turned out to be a no-op, and its own log said so: the serializer address it
// returned was IDENTICAL to the loader's, because RenderDoc does not patch the DXGI exports -- it hooks
// import tables instead. Printing both addresses is what made that visible rather than assumed.
//
// The real cause is a POLICY, not a wrapping bug. RenderDoc refuses NVAPI by default:
//
//     nvapi_hooks.cpp:619   if(RenderDoc::Inst().IsVendorExtensionEnabled(VendorExtensions::NvAPI))
//                             return real;
//                           else
//                             return NULL;      <-- every NVAPI entry point the game asks for
//
//     "NvAPI disabled: Returning NULL for nvapi_QueryInterface(NvAPI_EnumPhysicalGPUs)"
//
// Cyberpunk cannot enumerate its GPU through NVAPI, its NVIDIA-side initialisation fails, and what it
// puts on screen is that ray tracing failed to load -- with ray tracing switched off. The message names
// the symptom; NVAPI is the cause.
//
// There is an official switch for it, capture option 12, whose value is the PCI vendor id:
//
//     eRENDERDOC_Option_AllowUnsupportedVendorExtensions = 12,  value 0x10DE
//
// It is deliberately absent from renderdoccmd's flags -- RenderDoc calls this "explicitly unsupported"
// and warns that it can crash or replay incorrectly -- so the only way in is the in-process API, which is
// what this does. It must run before the game touches NVAPI, hence the call site in the plugin's early
// bootstrap next to the factory hooks.
//
// This is opt-in by the presence of renderdoc.dll: with no capture layer resident the function returns
// immediately and changes nothing about normal play.
namespace {

// Minimal ABI-compatible prefix of RENDERDOC_API_1_6_0 -- enough to reach SetCaptureOptionU32 without a
// link-time dependency on RenderDoc. The layout is the public header's; the unused entries keep the
// pointer offsets honest.
struct RenderDocApiPrefix {
    void* GetAPIVersion;
    int(__cdecl* SetCaptureOptionU32)(int, uint32_t);
};
using GetApiFn = int(__cdecl*)(int, void**);

}  // namespace

// Insert this plugin's detour BEHIND RenderDoc's hook for one function, so the call order becomes
//
//     game -> RenderDoc's hook -> our detour -> the real export
//
// instead of the knot that otherwise forms. Measured: with our MinHook on the interposer's
// CreateDXGIFactory* in place the capture layer reported no graphics API at all, and with those hooks
// switched off it reported D3D12 at once -- because RenderDoc's "original" is the ADDRESS of the export
// whose bytes we just patched, so calling it re-enters us and the factory is never wrapped.
//
// `trampoline` must be MinHook's original-function pointer, which reaches the real export without going
// through our patch. Handing over the patched address instead rebuilds the loop.
//
// Returns false when there is no capture layer, when it is a stock RenderDoc without the export, or when
// RenderDoc does not hook that function -- all three are ordinary, and only the second is worth a word in
// the log since it silently costs the capture.
bool RenderDocChainBehindHook(const char* module, const char* functionName, void* trampoline) {
    if (!module || !functionName || !trampoline) return false;

    const HMODULE renderdoc = GetModuleHandleA("renderdoc.dll");
    if (!renderdoc) return false;

    using SetOrigFn = bool(__cdecl*)(const char*, const char*, void*);
    auto setOrig = reinterpret_cast<SetOrigFn>(
        GetProcAddress(renderdoc, "RENDERDOC_CPVR_SetHookOriginal"));
    if (!setOrig) {
        if (ShouldLog(functionName))
            Log("[RenderDoc] RENDERDOC_CPVR_SetHookOriginal is missing -- '%s!%s' cannot be chained, so "
                "the capture layer will not see the factory. Use the renderdoc.dll built from "
                "renderdoc-src.\n", module, functionName);
        return false;
    }

    const bool ok = setOrig(module, functionName, trampoline);
    Log("[RenderDoc] chained behind '%s!%s': %s (our trampoline %p)\n", module, functionName,
        ok ? "ok" : "not hooked by RenderDoc", trampoline);
    return ok;
}

bool RenderDocAllowNvApi() {
    const HMODULE renderdoc = GetModuleHandleA("renderdoc.dll");
    if (!renderdoc) return false;

    auto getApi = reinterpret_cast<GetApiFn>(GetProcAddress(renderdoc, "RENDERDOC_GetAPI"));
    if (!getApi) {
        Log("[RenderDoc] renderdoc.dll is resident but RENDERDOC_GetAPI is missing\n");
        return false;
    }

    void* api = nullptr;
    if (getApi(10600 /* 1.6.0 */, &api) != 1 || !api) {
        Log("[RenderDoc] RENDERDOC_GetAPI(1.6.0) failed -- NVAPI stays disabled and the game will "
            "report that ray tracing could not load\n");
        return false;
    }

    auto* table = static_cast<RenderDocApiPrefix*>(api);
    if (!table->SetCaptureOptionU32) return false;
    const int ok = table->SetCaptureOptionU32(12 /* AllowUnsupportedVendorExtensions */, 0x10DE);
    Log("[RenderDoc] NVAPI vendor extension %s (capture option 12 = 0x10DE)\n",
        ok ? "ENABLED" : "REJECTED");
    return ok != 0;
}

}  // namespace cvr
#endif
#endif
