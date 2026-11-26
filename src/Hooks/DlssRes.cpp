#if !CVR_RUST_VRIK   // the Rust build registers this file's hook from rust/crates/cvr-render/src/hook_table.rs
// DlssRes -- one hook, one file. Installed from the registry it registers itself with at the
// bottom; see Hooks/Hook.hpp for why the order is declared here rather than in a boot function.
//
// The DLSS render/target sizes, as the engine decides them. Read-only; the override it used to
// apply is gone, so this reports what the engine chose and nothing else.

#include "Hooks/Hook.hpp"
#if CVR_RUST_VRIK
// rust/crates/cvr-rig/src/vrik/dlss_res.rs: the size log (OnDLSSResCallback, called from src/Stereo/Dlss.cpp).
extern "C" bool cvr_hook_dlss_res_install();
CVR_HOOK("DlssRes", ::cvr::hooks::Stage::Boot, 90, cvr_hook_dlss_res_install);
#else
#include "Core/VrCoreShared.hpp"
#include "Hooks/Hook.hpp"
#include "Utils/MemorySafe.hpp"

#include <windows.h>
#include <cstdint>
#include <cstddef>

namespace {
uint64_t g_dlssResHits = 0;
}  // namespace

extern "C" void __fastcall OnDLSSResCallback(void* dlssPtr) {
    g_dlssResHits++;

    const uintptr_t dlss = reinterpret_cast<uintptr_t>(dlssPtr);
    if (dlss < 0x10000) {
        return;
    }

    const uintptr_t prev = g_dlssResPtr;
    g_dlssResPtr = dlss;

    // Log a state the FIRST time it is seen, not whenever it differs from the previous call.
    //
    // The old test assumed a single DLSS state. With a second view there are two, and the
    // hook alternates between them, so "changed since last call" was true every single time
    // -- the log filled with the same two pointers in turn. Remembering which ones we have
    // already reported keeps the useful signal (a new state appeared) and drops the noise.
    bool firstSight = false;
    {
        static uintptr_t s_seen[8] = {};
        static unsigned s_seenN = 0;
        bool known = false;
        for (unsigned i = 0; i < s_seenN; ++i) {
            if (s_seen[i] == dlss) { known = true; break; }
        }
        if (!known && s_seenN < 8) {
            s_seen[s_seenN++] = dlss;
            firstSight = true;
        }
    }
    (void)prev;
    if (firstSight || ((g_dlssResHits % 600) == 1)) {
        // Log the render/target PAIR, not the three height slots. The old line printed +0x04,
        // +0x18 and +0x1C -- all the same field in triplicate -- which is why it read a tidy
        // "2560/2560/2560" while the actual render size was the lopsided 1485x2560 sitting one
        // field to the left, at +0x00. Now the shape is visible without a debugger.
        uint32_t renderW = 0, renderH = 0, targetW = 0, targetH = 0;
        ReadU32Safe(dlss + 0x00, &renderW);
        ReadU32Safe(dlss + 0x04, &renderH);
        ReadU32Safe(dlss + 0x20, &targetW);
        ReadU32Safe(dlss + 0x24, &targetH);
        const float sx = targetW ? static_cast<float>(renderW) / static_cast<float>(targetW) : 0.0f;
        const float sy = targetH ? static_cast<float>(renderH) / static_cast<float>(targetH) : 0.0f;
        // Read-only now. The override this used to report on is gone, so the line reports what
        // the ENGINE decided and nothing else -- which is all it was ever useful for.
        // On CHANGE only, same reason as the SettingsRes line: 120 identical lines a session.
        static uint32_t s_lastRW = 0, s_lastRH = 0, s_lastTW = 0, s_lastTH = 0;
        const bool changed = (renderW != s_lastRW) || (renderH != s_lastRH) ||
                             (targetW != s_lastTW) || (targetH != s_lastTH);
        s_lastRW = renderW; s_lastRH = renderH; s_lastTW = targetW; s_lastTH = targetH;
        if (changed)
        Log("DLSSRes hook: ptr=%p render=%ux%u target=%ux%u scale=%.3f/%.3f\n",
            reinterpret_cast<void*>(dlss),
            renderW, renderH, targetW, targetH, sx, sy);
    }
}

// 2.31 patched the tail of the render-res writer (mov [rbx+30],r14 / mov [rbx+4],eax / [rbx+18] / [rbx+1C]).
// 2.5's writer (sub_140425054) clamps every size to at least 16 and leaves through several stores, so there
// is no single tail left to patch. The stereo render-res detour (Stereo/Dlss.cpp, Detour_RenderRes) sees the
// same struct (view+0x34) and calls OnDLSSResCallback after the engine has written it.
bool InstallDLSSResHook() {
    Log("DLSSRes: reported from the render-res detour (no code patch on 2.5)\n");
    return true;
}

CVR_HOOK("DlssRes", ::cvr::hooks::Stage::Boot, 90, InstallDLSSResHook);
#endif
#endif
