// THE GAME'S OWN PAD RUMBLE, ON THE VR CONTROLLERS.
//
// Cyberpunk plays its rumble as Wwise motion events (a crash, a hard landing, a vehicle gun...): the AkMotion
// XInput sink renders them into the two pad motors through XInputSetState, which it looks up itself with
// GetProcAddress (docs/haptics-20261017.md) -- so the exe's import table, where Hooks/XInput.cpp merges the VR
// controllers, never sees it. The export itself is hooked here, in every XInput variant, and the motors are
// handed to Runtimes/Haptics.cpp: while driving they are the car's vibration instead of the audio-derived
// envelope (xr_vehicle_haptic_source=0), which buzzed with every radio song.
//
// The sink also asks the real exports whether pad 0 is there. Without a physical pad the answer is no and it
// would never send a thing, though the game itself -- through the import-table hook -- is told a pad is
// connected. So pad 0 is reported present here too (a zeroed state, a gamepad with two motors), and a
// vibration sent to a missing pad is accepted. Nothing else changes for a real pad: its state passes through
// and it still feels its own rumble.

#include "Core/LiveControls.hpp"
#include "Core/VrCoreShared.hpp"
#include "Hooks/Hook.hpp"
#include "Runtimes/Haptics.hpp"

#include <MinHook.h>
#include <windows.h>
#include <Xinput.h>

#include <atomic>
#include <cstdint>
#include <cstring>

extern "C" __declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_GameRumbleCalls{0};
extern "C" __declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_GameRumbleNonZero{0};

namespace {
using SetState_t = DWORD(WINAPI*)(DWORD, XINPUT_VIBRATION*);
using GetState_t = DWORD(WINAPI*)(DWORD, XINPUT_STATE*);
using GetCaps_t = DWORD(WINAPI*)(DWORD, DWORD, XINPUT_CAPABILITIES*);

constexpr int kVariants = 3;
const char* const kDlls[kVariants] = {"XInput1_4.dll", "XInput1_3.dll", "XInput9_1_0.dll"};
// One trampoline per DLL: the variants are separate code (9_1_0 is not a forwarder on every Windows).
SetState_t g_setState[kVariants]{};
GetState_t g_getState[kVariants]{};
GetCaps_t g_getCaps[kVariants]{};
void* g_hooked[kVariants * 3]{};
int g_hookedCount = 0;

bool PadZeroIsOurs() { return g_liveControls.xrXInputHook != 0; }

template <int I>
DWORD WINAPI SetStateHook(DWORD user, XINPUT_VIBRATION* vibration) {
    DWORD result = g_setState[I] ? g_setState[I](user, vibration) : ERROR_DEVICE_NOT_CONNECTED;
    if (user != 0 || !vibration) return result;
    const float low = vibration->wLeftMotorSpeed / 65535.0f;
    const float high = vibration->wRightMotorSpeed / 65535.0f;
    cvr::haptics::SetGameRumble(low, high);
    const uint64_t calls = ++CyberpunkVR_GameRumbleCalls;
    if (low > 0.0f || high > 0.0f) {
        const uint64_t felt = ++CyberpunkVR_GameRumbleNonZero;
        if (felt == 1 || felt == 100 || felt == 1000)
            Log("[game-rumble] %s: rumble #%llu low=%.2f high=%.2f (calls %llu).\n", kDlls[I],
                static_cast<unsigned long long>(felt), low, high, static_cast<unsigned long long>(calls));
    } else if (calls == 1) {
        Log("[game-rumble] %s: first motor update (zero).\n", kDlls[I]);
    }
    return result == ERROR_SUCCESS || !PadZeroIsOurs() ? result : ERROR_SUCCESS;
}

template <int I>
DWORD WINAPI GetStateHook(DWORD user, XINPUT_STATE* state) {
    const DWORD result = g_getState[I] ? g_getState[I](user, state) : ERROR_DEVICE_NOT_CONNECTED;
    if (result == ERROR_SUCCESS || user != 0 || !state || !PadZeroIsOurs()) return result;
    std::memset(state, 0, sizeof(*state));
    return ERROR_SUCCESS;
}

template <int I>
DWORD WINAPI GetCapsHook(DWORD user, DWORD flags, XINPUT_CAPABILITIES* caps) {
    const DWORD result = g_getCaps[I] ? g_getCaps[I](user, flags, caps) : ERROR_DEVICE_NOT_CONNECTED;
    if (result == ERROR_SUCCESS || user != 0 || !caps || !PadZeroIsOurs()) return result;
    std::memset(caps, 0, sizeof(*caps));
    caps->Type = XINPUT_DEVTYPE_GAMEPAD;
    caps->SubType = XINPUT_DEVSUBTYPE_GAMEPAD;
    caps->Vibration.wLeftMotorSpeed = caps->Vibration.wRightMotorSpeed = 0xFFFF;
    return ERROR_SUCCESS;
}

template <typename T>
bool Hook(void* target, void* detour, T* original) {
    if (!target) return false;
    for (int i = 0; i < g_hookedCount; ++i)
        if (g_hooked[i] == target) return false;   // a forwarder: the same code is hooked already
    if (MH_CreateHook(target, detour, reinterpret_cast<void**>(original)) != MH_OK) return false;
    if (MH_EnableHook(target) != MH_OK) { MH_RemoveHook(target); *original = nullptr; return false; }
    g_hooked[g_hookedCount++] = target;
    return true;
}

template <int I>
int HookVariant() {
    // Loaded now and kept: the sink's own LoadLibrary later gets this same, hooked module.
    const HMODULE dll = LoadLibraryA(kDlls[I]);
    if (!dll) return 0;
    int hooked = 0;
    hooked += Hook(reinterpret_cast<void*>(GetProcAddress(dll, "XInputSetState")),
                   reinterpret_cast<void*>(&SetStateHook<I>), &g_setState[I]);
    hooked += Hook(reinterpret_cast<void*>(GetProcAddress(dll, "XInputGetState")),
                   reinterpret_cast<void*>(&GetStateHook<I>), &g_getState[I]);
    hooked += Hook(reinterpret_cast<void*>(GetProcAddress(dll, "XInputGetCapabilities")),
                   reinterpret_cast<void*>(&GetCapsHook<I>), &g_getCaps[I]);
    return hooked;
}

bool InstallGameRumble() {
    const auto init = MH_Initialize();
    if (init != MH_OK && init != MH_ERROR_ALREADY_INITIALIZED) return false;
    const int hooked = HookVariant<0>() + HookVariant<1>() + HookVariant<2>();
    Log("[game-rumble] XInput exports hooked: %d (vibration, presence and capabilities per DLL).\n", hooked);
    return g_setState[0] || g_setState[1] || g_setState[2];
}

bool GameRumbleWanted() { return g_liveControls.xrXInputInstall != 0; }
}  // namespace

// After the import-table hook (XInput, 100): that one records the export's address as the real function, and
// calling it through this detour only adds the presence answer it already gives.
CVR_HOOK_IF("GameRumble", ::cvr::hooks::Stage::Boot, 101, InstallGameRumble, GameRumbleWanted);
