#include "Hooks/CyberwareChord.hpp"
#include "Hooks/KeypadInput.hpp"
#include "Anim/CharacterRig.hpp"   // VRIK_ClockMs
#include "Camera/CameraState.hpp"
#include "Core/VrCoreShared.hpp"
#include "Core/LiveControls.hpp"
#include "Overlay/VrOverlay.hpp"
#include "Utils/DebugGate.hpp"
#include <atomic>
#include <mutex>
#include <windows.h>

// The export is defined in src/Hooks/InputServices.cpp (both builds); the C++ chord (rust/crates/cvr-rig/src/vrik/
// cyberware_chord.rs under CVR_RUST_VRIK) asks the foreground and sends the key through src/Hooks/HookServices.cpp.
extern "C" std::atomic<uint64_t> CyberpunkVR_CyberwareChordSent;
extern "C" bool cvr_cpp_foreground_in_process();
extern "C" uint32_t cvr_cpp_send_input(uint32_t count,const void* inputs,int size);
namespace {
cvr::input::CyberwareHold chord;
std::mutex mutex;
std::atomic<uint64_t> pendingUntil{};
// A chord that fired with the game window behind, until when the script side may still take it.
std::atomic<uint64_t> scriptUntil{};
bool Allowed() {
    return g_liveControls.xrXInputHook!=0 && g_menuModeValue==0 && !cvr::vrui::CapturesInput() && !cvr::input::KeypadInputActive() &&
           !DeviceCamActive() && !g_bdActive.load(std::memory_order_relaxed) &&
           g_sceneTier.load(std::memory_order_relaxed)<=1;
}
}
bool cvr::input::UpdateCyberwareChord(bool l3,float grip,bool r3,bool inputActive) {
    std::lock_guard lock(mutex);
    const bool allowed=inputActive && Allowed();
    const auto now=VRIK_ClockMs();
    const auto result=chord.Step(l3,grip,r3,allowed,now);
    if(!allowed || r3)pendingUntil.store(0,std::memory_order_release);
    else if(result.fire)pendingUntil.store(now+200,std::memory_order_release);
    return result.claimed;
}
void cvr::input::DispatchCyberwareChord() {
    const auto until=pendingUntil.exchange(0,std::memory_order_acq_rel);
    if(!until || VRIK_ClockMs()>=until || !Allowed())return;
    if(!cvr_cpp_foreground_in_process()) {
        // The headset has the focus and the window is behind (BackgroundPad keeps the sticks working then): a key
        // would land in the other app, never in the game. The weapon mod's frame takes the chord instead and runs
        // the press's own activation, PlayerPuppet.ActivateIconicCyberware (VRActivateIconicCyberware).
        scriptUntil.store(VRIK_ClockMs()+500,std::memory_order_release);
        return;
    }
    // Input Loader appends F18 to IconicCyberware_Button, with no UI override:
    // game context/cooldown still decides whether the ability may activate.
    // Unlike E this private key survives a player's keyboard rebinding; unlike
    // LB+RB it cannot leak the shoulders' scheme-specific actions.
    INPUT input[2]{};input[0].type=INPUT_KEYBOARD;input[0].ki.wVk=VK_F18;
    input[1]=input[0];input[1].ki.dwFlags=KEYEVENTF_KEYUP;
    const auto sent=cvr_cpp_send_input(2,input,sizeof(INPUT));
    if(sent==1)cvr_cpp_send_input(1,input+1,sizeof(INPUT)); // release after a partial OS insertion
    if(sent==2)CVR_DIAGNOSTIC(++CyberpunkVR_CyberwareChordSent);
}
bool cvr::input::TakeCyberwareChord() {
    const auto until=scriptUntil.exchange(0,std::memory_order_acq_rel);
    // Half a second: a chord the script side did not take by then (a load, a pause) is not run later.
    return until && VRIK_ClockMs()<until && Allowed();
}
