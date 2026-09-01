// CameraFov -- one hook, one file. It registers itself at the bottom; Hooks/Hook.hpp says why the
// stage and order live here rather than in a boot function.
//
// The camera field of view. The engine field is VERTICAL and the horizontal derives from
// the render aspect -- writing the wrong one of the two distorts the projection rather
// than widening it.

#include "Core/VrCoreShared.hpp"
#include "Core/LiveControls.hpp"
#include "Core/Telemetry.hpp"
#include "Hooks/Hook.hpp"
#include "Hooks/Trampoline.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include "Utils/AobScanner.hpp"
#include "Utils/MemorySafe.hpp"
#include "Runtimes/RuntimeFovCorrection.hpp"

#include <windows.h>
#include <cstdint>
#include <cstddef>

// EXTERNAL LINKAGE ON PURPOSE: the hub prints this in its own diagnostic line, so a copy per
// translation unit would leave that line reporting a number nothing ever wrote.
volatile float g_dbgLastOriginalFov = 0.0f;

namespace {
volatile float g_lodFovOverride = 120.0f;
uint64_t g_normalFovHookHits = 0;

}  // namespace


// g_lodFovOverride is WRITTEN HERE AND READ BY NOBODY. It is computed as the applied FOV
// plus 30 degrees and printed in the diagnostic line below, and that is its entire life:
// no LOD site consumes it. Kept because the log line is load-bearing for diagnosis, and
// named here so the next reader does not spend an afternoon assuming the engine reads it.

extern "C" void __fastcall OnNormalFovHookCallback(void* cameraState, float originalFov) {
    g_normalFovHookHits++;
    g_dbgLastOriginalFov = originalFov;
    g_dbgFovCamState = cameraState;

    // Aim at the HORIZONTAL we submit -- the de-canted symmetric span -- and then solve for the
    // vertical that makes the engine derive exactly that horizontal from the render aspect. The
    // old code wrote the horizontal straight into a field the engine reads as vertical, which on a
    // symmetric headset is harmless (the two are equal) and on a canted one is four degrees wrong.
    constexpr float kD2R = 3.1415926535f / 180.0f;
    const float forced = GetForcedFov();

    // Moved above the decision: sizing the frustum to COVER the panel needs the aspect, because the
    // engine derives the vertical from it (PR #24).
    const float aspect = (g_launcherWidth > 1 && g_launcherHeight > 1)
        ? (static_cast<float>(g_launcherWidth) / static_cast<float>(g_launcherHeight))
        : 1.0f;

    float targetHDeg = 0.0f;
    if (forced > 1.0f && forced < 170.0f) {
        targetHDeg = forced;                      // xr_force_fov has always meant the horizontal
    } else {
        XrFovf lf{}, rf{};
        if (OpenXRManager::Get().GetCurrentEyeFov(0, &lf) &&
            OpenXRManager::Get().GetCurrentEyeFov(1, &rf)) {
            // Size to COVER the panel, not to match its span. The submit layer recentres the frustum
            // on the eye axis but never rotates the pose to match it, so on a canted headset a
            // span-sized frustum leaves the outer edge and the bottom black. See
            // GetPanelCoveringHorizontalFovDeg for the geometry. The submit follows this value
            // through GetGameRenderFovDeg, so the two ends of the contract cannot drift apart.
            targetHDeg = GetPanelCoveringHorizontalFovDeg(lf, rf, aspect);
        }
        if (!(targetHDeg > 1.0f && targetHDeg < 170.0f)) {
            targetHDeg = OpenXRManager::Get().GetRuntimeHorizontalFovDeg();
        }
    }

    float verticalDeg = originalFov;
    float horizontalDeg = 0.0f;
    if (targetHDeg > 1.0f && targetHDeg < 170.0f && aspect > 0.05f && aspect < 20.0f) {
        verticalDeg   = 2.0f * atanf(tanf(targetHDeg * 0.5f * kD2R) / aspect) / kD2R;
        horizontalDeg = targetHDeg;
    } else {
        // Before XR is up there is nothing to derive from. Leave the engine's own value in place
        // and report the horizontal it implies, so no consumer reads a number that was never real.
        horizontalDeg = 2.0f * atanf(tanf(originalFov * 0.5f * kD2R) * aspect) / kD2R;
    }
    if (!(verticalDeg > 1.0f && verticalDeg < 179.0f)) verticalDeg = originalFov;

    g_normalFovOverrideValue = verticalDeg;      // what the engine's field receives
    g_engineHorizontalFovDeg = horizontalDeg;    // what it therefore renders, for submit + overlay

    static uint64_t s_fovLog = 0;
    if (s_fovLog < 20) {
        Log("NormalFOV: original=%.3f xr_force_fov=%.3f targetH=%.3f aspect=%.5f (%dx%d)"
            " -> wroteV=%.3f derivedH=%.3f\n",
            originalFov, forced, targetHDeg, aspect, g_launcherWidth, g_launcherHeight,
            verticalDeg, horizontalDeg);
        s_fovLog++;
    }

    // LOD cone: a bit wider than the actual FOV so edge/floor geometry doesn't pop;
    // based on whatever FOV we ultimately use (native or user-forced).
    g_lodFovOverride = g_normalFovOverrideValue + 30.0f;

    // The camera FOV hook writes the FOV ONLY to camera
    // +0x410 -- which our trampoline already forces via the patched xmm3 -- and never
    // touches +0x414. We used to also write +0x414; that slot is NOT the horizontal
    // FOV, and overwriting it distorted the projection ("world too big", visible in
    // BOTH Mono and AER => a monocular/FOV artifact, not IPD). Leave
    // +0x414 alone.
    //
    // if (cameraState && desiredFov > 1.0f) {
    //     const uintptr_t stateAddr = reinterpret_cast<uintptr_t>(cameraState);
    //     WriteFloatSafe(stateAddr + 0x414, g_normalFovOverrideValue);
    // }

    if (g_verboseLog && (g_normalFovHookHits % 600) == 1) {
        float currentHFov = 0.0f;
        float currentVFov = 0.0f;
        const uintptr_t stateAddr = reinterpret_cast<uintptr_t>(cameraState);
        ReadFloatSafe(stateAddr + 0x410, &currentHFov);
        ReadFloatSafe(stateAddr + 0x414, &currentVFov);
        Log("NormalFOV hook: state=%p original=%.6f desired=%.6f storedH=%.6f storedV=%.6f lodFov=%.6f runtimeHFov=%.6f runtimeIPD=%.6f\n",
            cameraState,
            originalFov,
            g_normalFovOverrideValue,
            currentHFov,
            currentVFov,
            g_lodFovOverride,
            OpenXRManager::Get().GetRuntimeHorizontalFovDeg(),
            OpenXRManager::Get().GetRuntimeIpd());
    }
}

bool InstallNormalFovHook() {
    const char* pattern = "\xF3\x0F\x11\x99\x10\x04\x00\x00\x48\x8B\x91\x60\x03\x00\x00";
    const char* mask = "xxxxxxxxxxxxxxx";
    uint8_t* found = static_cast<uint8_t*>(FindPattern("Cyberpunk2077.exe", pattern, mask));
    if (!found) return false;

    constexpr int replaceLen = 15;
    void* tramp = AllocateTrampoline(found, 512);
    if (!tramp) return false;

    uint8_t* code = static_cast<uint8_t*>(tramp);
    int pos = 0;

    code[pos++] = 0x9C;
    code[pos++] = 0x50;
    code[pos++] = 0x51;
    code[pos++] = 0x52;
    code[pos++] = 0x41; code[pos++] = 0x50;
    code[pos++] = 0x41; code[pos++] = 0x51;
    code[pos++] = 0x41; code[pos++] = 0x52;
    code[pos++] = 0x41; code[pos++] = 0x53;
    code[pos++] = 0x55;

    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x40;
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x04; code[pos++] = 0x24;
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10;
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20;
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30;

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xE5;
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xE4; code[pos++] = 0xF0;
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x20;

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xC9; // mov rcx, rcx
    code[pos++] = 0x0F; code[pos++] = 0x28; code[pos++] = 0xCB; // movaps xmm1, xmm3
    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(OnNormalFovHookCallback));
    code[pos++] = 0xFF; code[pos++] = 0xD0;

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xEC;

    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x04; code[pos++] = 0x24;
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10;
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20;
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30;
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xC4; code[pos++] = 0x40;

    code[pos++] = 0x5D;
    code[pos++] = 0x41; code[pos++] = 0x5B;
    code[pos++] = 0x41; code[pos++] = 0x5A;
    code[pos++] = 0x41; code[pos++] = 0x59;
    code[pos++] = 0x41; code[pos++] = 0x58;
    code[pos++] = 0x5A;
    code[pos++] = 0x59;
    code[pos++] = 0x58;
    code[pos++] = 0x9D;

    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(&g_normalFovOverrideValue));
    code[pos++] = 0xF3; code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x18; // movss xmm3,[rax]
    code[pos++] = 0xF3; code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x99; code[pos++] = 0x10; code[pos++] = 0x04; code[pos++] = 0x00; code[pos++] = 0x00;
    code[pos++] = 0x48; code[pos++] = 0x8B; code[pos++] = 0x91; code[pos++] = 0x60; code[pos++] = 0x03; code[pos++] = 0x00; code[pos++] = 0x00;

    code[pos++] = 0xE9;
    *reinterpret_cast<int32_t*>(code + pos) = static_cast<int32_t>((found + replaceLen) - (code + pos + 4));
    pos += 4;

    DWORD oldProtect;
    VirtualProtect(found, replaceLen, PAGE_EXECUTE_READWRITE, &oldProtect);
    found[0] = 0xE9;
    *reinterpret_cast<int32_t*>(found + 1) = static_cast<int32_t>(code - (found + 5));
    for (int i = 5; i < replaceLen; ++i) found[i] = 0x90;
    VirtualProtect(found, replaceLen, oldProtect, &oldProtect);
    FlushInstructionCache(GetCurrentProcess(), found, replaceLen);
    return true;
}
CVR_HOOK("CameraFov", ::cvr::hooks::Stage::Boot, 35, InstallNormalFovHook);
