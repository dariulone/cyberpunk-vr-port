#if !CVR_RUST_VRIK   // the Rust build registers this file's hook from rust/crates/cvr-render/src/hook_table.rs
// CameraPitch -- one hook, one file. It registers itself at the bottom; Hooks/Hook.hpp says why the
// stage and order live here rather than in a boot function.
//
// The camera pitch the engine is about to apply. In VR the head owns pitch, so the mouse
// must not also write it.

#include "Camera/CameraState.hpp"   // g_gamePitchRadians, published for both compose sites
#include "Core/VrCoreShared.hpp"
#include "Core/LiveControls.hpp"
#include "Core/Telemetry.hpp"
#include "Hooks/Hook.hpp"
#include "Hooks/Trampoline.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include "Utils/AobScanner.hpp"
#include "Utils/MemorySafe.hpp"

#include <windows.h>
#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstddef>

#if CVR_RUST_VRIK
// rust/crates/cvr-rig/src/vrik/camera_pitch.rs: the same site, stub and pitch.
extern "C" bool cvr_hook_camera_pitch_install();
CVR_HOOK("CameraPitch", ::cvr::hooks::Stage::Boot, 30, cvr_hook_camera_pitch_install);
#else

// State private to this hook -- it moved out of the hub with the code that uses it.
namespace {
uint64_t g_pitchHookHits = 0;
volatile float g_pitchOverrideValue = 0.0f;
}  // namespace

extern "C" void __fastcall OnPitchHookCallback(void* pitchState, float originalPitch) {
    g_pitchHookHits++;

    // Mouse-Y pitch in CP2077 also drives a constrained camera pivot offset, which moves the head
    // toward/away from the body in VR. That is why the default is to hold the game pitch at zero and
    // let the headset supply vertical look.
    //
    // BUT ONLY WHEN THE OPTION SAYS SO (dabinn, TofuExpress 11974ee5). Forcing zero unconditionally
    // is what left "Disable Mouse Y" without an off state: turning it off changed the stick handling
    // and nothing else, because the pitch was still being zeroed here and the compose was still
    // yaw-only. With the option off the game's pitch is preserved and composed with the HMD
    // orientation at both write sites.
    const float desiredPitch = g_liveControls.xrDisableMouseY != 0 ? 0.0f : originalPitch;

    // The clamp is read every call, not only when logging, because it is now load-bearing: the
    // published pitch has to be the one the game will actually settle at, or the view would
    // disagree with the engine at the ends of the range. The unit is inferred from the clamp
    // itself -- a limit past 3.2 cannot be radians -- rather than assumed.
    float minPitch = 0.0f;
    float maxPitch = 0.0f;
    ReadFloatSafe(reinterpret_cast<uintptr_t>(pitchState) + 0x14, &minPitch);
    ReadFloatSafe(reinterpret_cast<uintptr_t>(pitchState) + 0x18, &maxPitch);
    const float clampedPitch = (std::max)(minPitch, (std::min)(maxPitch, desiredPitch));
    const bool pitchUsesDegrees = fabsf(minPitch) > 3.2f || fabsf(maxPitch) > 3.2f;
    g_gamePitchRadians = clampedPitch * (pitchUsesDegrees ? 0.01745329252f : 1.0f);

    g_pitchOverrideValue = desiredPitch;

    if (g_verboseLog && (g_pitchHookHits % 600) == 1) {
        Log("Pitch hook: state=%p original=%.6f desired=%.6f clamp=[%.6f, %.6f]\n",
            pitchState,
            originalPitch,
            desiredPitch,
            minPitch,
            maxPitch);
    }
}

bool InstallPitchHook() {
    const char* pattern = "\xF3\x0F\x10\x4F\x14\xF3\x0F\x5F\xC8\xF3\x0F\x5D\x4F\x18";
    const char* mask = "xxxxxxxxxxxxxx";
    uint8_t* found = static_cast<uint8_t*>(FindPattern("Cyberpunk2077.exe", pattern, mask));
    if (!found) return false;

    constexpr int replaceLen = 14;
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

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xF9; // mov rcx, rdi
    code[pos++] = 0x0F; code[pos++] = 0x28; code[pos++] = 0xC8; // movaps xmm1, xmm0
    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(OnPitchHookCallback));
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

    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(&g_pitchOverrideValue));
    code[pos++] = 0xF3; code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x00; // movss xmm0,[rax]

    code[pos++] = 0xF3; code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x4F; code[pos++] = 0x14;
    code[pos++] = 0xF3; code[pos++] = 0x0F; code[pos++] = 0x5F; code[pos++] = 0xC8;
    code[pos++] = 0xF3; code[pos++] = 0x0F; code[pos++] = 0x5D; code[pos++] = 0x4F; code[pos++] = 0x18;

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
CVR_HOOK("CameraPitch", ::cvr::hooks::Stage::Boot, 30, InstallPitchHook);
#endif
#endif
