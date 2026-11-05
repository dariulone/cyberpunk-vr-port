// WHICH UI OVERLAY OWNS THE B BUTTON RIGHT NOW, published from the script side.
//
// The phone, the radio port and the vehicle list are OVERLAYS, not menus: the game's menu-mode value
// stays 0 while they are up, so the port's gameplay rules still applied to the pad and B -- which is
// Exit_Button for all three -- was being held back for the physical reload's magazine drop. With a
// weapon in hand there was then no way to close any of them.
//
// Script publishes it because only script can see them: the phone is a blackboard bool, the other two
// are widgets in the notification container. The plugin cannot ask -- its own polling runs on a worker
// thread, where a call into the scripting system is not safe.
#include "Natives/NativeFunctions.hpp"
#include "Core/VrCoreShared.hpp"
#include "Runtimes/OpenXRManager.hpp"

#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Scripting/Utils.hpp>

#include <windows.h>
#include <atomic>

std::atomic<int> g_uiPopupOpen{0};
std::atomic<unsigned long long> g_uiPopupClosedMs{0};

void VRUiPopup(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    int32_t active = 0;
    RED4ext::GetParameter(aFrame, &active);
    aFrame->code++;

    const int now = (active != 0) ? 1 : 0;
    const int prev = g_uiPopupOpen.exchange(now, std::memory_order_relaxed);
    // The moment it CLOSES is the one the block window is measured from; opening needs no stamp.
    if (prev != 0 && now == 0)
        g_uiPopupClosedMs.store(GetTickCount64(), std::memory_order_relaxed);
    if (aOut) *aOut = now;
}

// THE POPUPS' VISUAL STATE IS ONE MONO PICTURE OVER BOTH EYES (2026-10-17).
//
// The phone, the dialer, the vehicle list, the radio, message popups, shards and the car colour pick
// all call UISystem.RequestNewVisualState(n"inkModalPopupState"). Measured with the vehicle list open
// (build/monowire-housing-20261016/probe_vstate.py): both eyes carry MAIN's image (left eye vs MAIN
// 0.95, eyes 1.0 alike), so far content doubles under the asymmetric projection. Restoring the state
// under the open list brought the second eye back (eyes -0.22, as without the list), requesting it
// again took it away, and the state alone without a popup turned both eyes black. The second view
// draws MAIN's composition (the lend in Stereo/ViewReuse.cpp), so whatever the state lays over the
// frame is MAIN's picture in both eyes. Neither the circular background blur nor the lend itself
// changed it.
//
// The state is also what raised the engine's menu flag for these popups on 2.31 (the vehicle list:
// 1 before, 0 with the state held back), which sent them down the VR menu presentation.
//
// So in VR the state is not entered: the popups keep their own widgets, game context and time
// dilation. Requests held back are counted and only that many restores of the same state are
// swallowed, so the engine's state stack stays balanced if VR starts or stops while a popup is up.
extern "C" __declspec(dllexport) int CyberpunkVR_PopupVisualStateFilter = 1;
// [0] requests held back, [1] restores swallowed with them, [2] calls passed to the game
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_PopupVisualStateStats[3] = {};
static std::atomic<int> g_heldPopupStates{0};

// Whether the wrapped UISystem call goes ahead: (state, request) -> pass.
void VRPopupVisualState(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);
    RED4ext::CName state;
    bool request = false;
    RED4ext::GetParameter(aFrame, &state);
    RED4ext::GetParameter(aFrame, &request);
    aFrame->code++;
    static const RED4ext::CName kModalPopupState("inkModalPopupState");
    bool pass = true;
    if (state == kModalPopupState) {
        if (request) {
            if (CyberpunkVR_PopupVisualStateFilter && OpenXRManager::Get().IsSessionRunning()) {
                g_heldPopupStates.fetch_add(1, std::memory_order_relaxed);
                ++CyberpunkVR_PopupVisualStateStats[0];
                pass = false;
            }
        } else {
            int held = g_heldPopupStates.load(std::memory_order_relaxed);
            while (held > 0 && !g_heldPopupStates.compare_exchange_weak(held, held - 1, std::memory_order_relaxed)) {}
            if (held > 0) { ++CyberpunkVR_PopupVisualStateStats[1]; pass = false; }
        }
    }
    if (pass) ++CyberpunkVR_PopupVisualStateStats[2];
    if (aOut) *aOut = pass;
}
