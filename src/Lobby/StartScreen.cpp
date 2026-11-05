// NO START SCREEN: the game goes from its boot straight to the main menu, as -skipStartScreen has it, without the
// argument. The start screen ("press any key", the engagement screen) is a flat picture waiting for a key; in the
// headset the game starts in the lobby, with its own intro (mods/cet/CyberpunkVRPort_Lobby). Verified against game
// 2.5 with IDA headless (2026-10-28).
//
// The pre-game session state (the one sub_140D4BE3C names PreGameSession) decides on entering (sub_14018D6F4, its
// vtable slot 45) between the two: the skip flag (byte_143C2B4E0) is set when skipStartScreen is among the launch
// arguments or the state's own parameters, cleared when forceStartScreen is, and then either the state goes on to
// the main menu (sub_142EFFA24: state 7) or it listens for any key behind the engagement screen (sub_141847DF8:
// state 1). Coming back from a game the state is given skipStartScreen itself (sub_140D4BE3C), so the flag is how
// the game asks the question every time. It is set before the state looks at it; forceStartScreen still wins.
#include "Stereo/DetourRegistry.hpp"
#include "Stereo/StereoInternal.hpp"
#include <cstdint>

extern "C" {
// 0 = the start screen shows unless -skipStartScreen is given, as the game has it.
__declspec(dllexport) int32_t CyberpunkVR_SkipStartScreen = 1;
}

namespace {
constexpr uintptr_t PRE_GAME_ENTER_RVA = 0x18D6F4;
constexpr uintptr_t kSkipStartScreen = 0x3C2B4E0;

using PreGameEnterFn = int64_t(__fastcall*)(void*, void*, void*, void*);
PreGameEnterFn g_origPreGameEnter = nullptr;

int64_t __fastcall Detour_PreGameEnter(void* state, void* app, void* params, void* launch) {
    if (CyberpunkVR_SkipStartScreen && cvr::detail::g_exe_base)
        *reinterpret_cast<uint8_t*>(reinterpret_cast<uintptr_t>(cvr::detail::g_exe_base) + kSkipStartScreen) = 1;
    return g_origPreGameEnter(state, app, params, launch);
}
}  // namespace

CVR_DETOUR("[boot] pre-game session enter sub_14018D6F4", PRE_GAME_ENTER_RVA, Detour_PreGameEnter, g_origPreGameEnter)
