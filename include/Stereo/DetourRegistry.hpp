#pragma once

#include <cstdint>

// ================================================================================================
// Engine detours, declared where they are written.
//
// WHAT THIS REPLACES. install_desc_ring_probe() is ~230 lines of the same five lines repeated
// forty-six times:
//
//     void* x = base + SOME_RVA;
//     if (MH_CreateHook(x, &Detour_Y, (void**)&g_orig_z) == MH_OK && MH_EnableHook(x) == MH_OK)
//         log("[tag] ... hooked @%p", x);
//     else log("[tag] failed to hook ... @%p", x);
//
// That function is also the reason src/Stereo/SyncStereo.cpp cannot be split: it names all
// forty-six detours and all forty-six trampolines, so it must SEE every one of them. Any attempt to
// move a detour into its own file has to move that mention too, and there is nowhere for it to go.
//
// A registry inverts it. A detour file says, once, at the bottom:
//
//     CVR_DETOUR("[build] full-build sub_141D43040", FULL_BUILD_RVA, Detour_FullBuild,
//                g_orig_full_build);
//
// and the install pass is one loop that neither knows nor needs to know how many there are. The
// mention lives with the code, which is the whole point.
//
// WHY THIS IS NOT Hooks/Hook.hpp. Those are pattern-scanned patch sites with hand-written
// trampolines and an arena; these are MinHook detours at a known RVA. The two have different
// failure modes and different things worth logging -- an RVA that no longer holds the expected
// function is a game-patch problem, a pattern that no longer matches is a different one -- and
// merging them would blur that. Same idea, deliberately separate registry.
//
// STATIC INITIALISATION is safe for the same reason as the hook registry: the list head is a
// function-local static, so the first detour to be constructed creates it and no translation-unit
// ordering matters.
// ================================================================================================

namespace cvr {
namespace detail {

// Whether a detour is wanted at all. Several of these are opt-in reuse optimisations and probes that
// the install pass wrapped in an `if` before calling; a detour nobody asked for is SKIPPED, not
// failed. Null means always wanted.
using DetourWantedFn = bool (*)();

struct EngineDetour {
    const char*     name;       // what the log calls it, including its [tag]
    uintptr_t       rva;        // offset into Cyberpunk2077.exe
    void*           detour;     // our function
    void**          original;   // where MinHook writes the trampoline
    DetourWantedFn  wanted;
    EngineDetour*   next;
    mutable bool    installed;  // set by the install pass (Rust's, through its address) on a const object

    EngineDetour(const char* n, uintptr_t r, void* d, void** o, DetourWantedFn w);
};

// Installs every registered detour, logging one line each. Returns the number that failed, so the
// caller can say so once and loudly. Requires MinHook to be initialised and g_exe_base resolved --
// it checks both rather than assuming, because getting here before the module base is known is how
// a detour silently lands at offset zero.
int InstallEngineDetours();

}  // namespace detail
}  // namespace cvr

#define CVR_DETOUR(displayName, rvaValue, detourFn, originalVar)                          \
    namespace {                                                                           \
    const ::cvr::detail::EngineDetour g_cvrDetourReg_##detourFn{                          \
        (displayName), (rvaValue), reinterpret_cast<void*>(&detourFn),                     \
        reinterpret_cast<void**>(&originalVar), nullptr};                                  \
    }

#define CVR_DETOUR_IF(displayName, rvaValue, detourFn, originalVar, wantedFn)             \
    namespace {                                                                           \
    const ::cvr::detail::EngineDetour g_cvrDetourReg_##detourFn{                          \
        (displayName), (rvaValue), reinterpret_cast<void*>(&detourFn),                     \
        reinterpret_cast<void**>(&originalVar), (wantedFn)};                               \
    }
