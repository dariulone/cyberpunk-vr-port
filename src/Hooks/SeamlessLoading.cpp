// SEAMLESS LOADING: a loaded save, a new game, goes on into the world the moment the world is ready -- the loading
// screen does not wait for a button. Verified against game 2.5 with IDA headless and live (2026-10-28).
//
// The initial loading's screen (inkInitialLoadingScreenLogicController, vtable 0x1430D9AE8) waits for the player
// once its world is ready: when the load is done (+0x2BD) and its animation is ready for it (+0x2BE) it shows its
// skip panel and listens for skip_loading (sub_140CCFE84); the button sets +0x2BF (sub_140CCF6D4). The supervisor
// asks the controller each frame whether it can finish (sub_140190004: +0x1A1 or +0x2BF; vtable slot 45, and the
// thunk sub_14018FFF8 of the controller's listener interface) and closes the screen once it can. +0x1A1 is how the
// game loads what nobody has to confirm (a fast travel: set from the loading's own state, sub_140D6DCAC).
//
// The world being ready reaches the controller three ways -- its "world loaded" (slot 44, sub_142E010EC), the same
// code inlined in its listener interface (sub_140CCF490, the one a save loaded from the main menu goes through: its
// controller stays in state 2) and its state setter's 3 (slot 37, sub_1407D4F74) -- and every one sets +0x2BD first.
// So the question the supervisor asks is where this goes in: once +0x2BD is set, +0x1A1 is set too, and the screen
// closes when its world is ready, the way a fast travel's does. (Measured: state 2, +0x2BD/+0x2BE set, the screen
// waiting; +0x1A1 written live closed it within 2 s.)
//
// What the screen shows meanwhile hangs in the headset's dark before the player (Render/LoadingScreen.hpp).
#include "Stereo/DetourRegistry.hpp"
#include <windows.h>
#include <atomic>
#include <cstdint>

extern "C" {
// 0 = the loading screen waits for its button, as the game has it.
__declspec(dllexport) int32_t CyberpunkVR_SeamlessLoading = 1;
// Loadings that went on without the button.
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_DebugSeamlessLoads{0};
}

namespace {
constexpr uintptr_t CAN_FINISH_RVA = 0x190004;
constexpr size_t kNoConfirm = 0x1A1;
constexpr size_t kWorldLoaded = 0x2BD;

using CanFinishFn = bool(__fastcall*)(void*);
CanFinishFn g_origCanFinish = nullptr;

void NoConfirmOnceLoaded(void* controller) {
    __try {
        uint8_t* c = static_cast<uint8_t*>(controller);
        if (!c[kWorldLoaded] || c[kNoConfirm]) return;
        c[kNoConfirm] = 1;
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return;
    }
    CyberpunkVR_DebugSeamlessLoads.fetch_add(1, std::memory_order_relaxed);
}

bool __fastcall Detour_CanFinish(void* controller) {
    if (CyberpunkVR_SeamlessLoading && controller) NoConfirmOnceLoaded(controller);
    return g_origCanFinish(controller);
}
}  // namespace

CVR_DETOUR("[loading] initial loading can finish sub_140190004", CAN_FINISH_RVA, Detour_CanFinish, g_origCanFinish)
