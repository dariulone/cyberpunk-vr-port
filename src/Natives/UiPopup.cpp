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

#include <RED4ext/RED4ext.hpp>

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
