#if !CVR_RUST_VRIK   // the Rust build registers this file's hook from rust/crates/cvr-render/src/hook_table.rs
#include "Utils/DebugGate.hpp"
// RebindCensus -- how many times per frame the engine re-establishes world transforms from their
// bindings, and nothing else.
//
// WHY THIS EXISTS. `sub_140329460` is the loop that walks a component's bindings, asks each provider
// for its slot transform ([vt+0xD8] -> sub_1401D92A0, which is simply parent+0xE0/+0xF0) and calls
// SetWorldTransform for it -- which lands in sub_1401D8558, the very function our camera write
// detours. Everything the camera inherits, heading included, arrives through this loop; the camera
// itself owns no yaw (its local quaternion is never written -- checked live with a write watchpoint
// that never fired).
//
// So the question this census answers is exactly one: does that loop run ONCE per rendered frame, or
// more than once? Twice would mean the skeleton is placed with two different world transforms inside
// one image, which is what a body double on a fast mouse turn looks like -- with the world itself
// unaffected, independent of VRIK, of VRCAM and of temporal upscaling. That is the reported symptom,
// and no counter we had could tell the two cases apart.
//
// The precedent for reading it this way is the HUD node: its census (150/s against 74 presents) is
// what let us RULE the HUD out, because the code already documented that DrawHUD enters twice by
// design. A rate is the cheapest instrument that can settle a "how many times" question, so measure
// before touching anything.
//
// Cost: one atomic increment on a call the engine already makes. No allocation, no logging on the
// hot path -- the rate is printed by the census in OpenXRPresent, once every couple of seconds.

#include "Core/VrCoreShared.hpp"
#include "Hooks/Hook.hpp"

#include <windows.h>
#include <MinHook.h>
#include <cstdint>

// Read by the rate census in src/Runtimes/OpenXRPresent.cpp, and live-readable so the number can be
// checked without waiting for a log line.
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/used_moved_exports_rest.rs.
extern "C" __declspec(dllexport) extern volatile uint64_t CyberpunkVR_DebugRebindLoopCalls;
#else
extern "C" __declspec(dllexport) volatile uint64_t CyberpunkVR_DebugRebindLoopCalls = 0;
#endif

#if CVR_RUST_VRIK
// rust/crates/cvr-rig/src/vrik/rebind_census.rs: the same detour and counter.
extern "C" bool cvr_hook_rebind_census_install();
CVR_HOOK("RebindCensus", ::cvr::hooks::Stage::Boot, 60, cvr_hook_rebind_census_install);
#else
namespace {

// sub_140329460: the binding -> SetWorldTransform loop. Hard RVA, like the pose-apply detour next
// door in AnimPose.cpp -- a pattern buys nothing for a function we identified by call site.
constexpr uint32_t kRebindLoopRva = 0x329460;

using RebindLoopFn = void* (*)(void*, void*, void*, void*);
RebindLoopFn g_origRebindLoop = nullptr;

void* Hooked_RebindLoop(void* a1, void* a2, void* a3, void* a4) {
    CVR_DIAGNOSTIC(InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugRebindLoopCalls)));
    return g_origRebindLoop(a1, a2, a3, a4);
}

bool InstallRebindCensusHook() {
    HMODULE mod = GetModuleHandleA("Cyberpunk2077.exe");
    if (!mod) return false;
    void* target = reinterpret_cast<void*>(reinterpret_cast<uintptr_t>(mod) + kRebindLoopRva);
    MH_Initialize();   // no-op when another hook already did it
    if (MH_CreateHook(target, &Hooked_RebindLoop,
                      reinterpret_cast<void**>(&g_origRebindLoop)) != MH_OK) {
        return false;
    }
    return MH_EnableHook(target) == MH_OK;
}

}  // namespace

CVR_HOOK("RebindCensus", ::cvr::hooks::Stage::Boot, 60, InstallRebindCensusHook);
#endif
#endif
