#include "Runtimes/Haptics.hpp"
#include "Runtimes/Psvr2HapticChannel.hpp"
#include "Runtimes/HapticRouting.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include <windows.h>
#include <atomic>
#include <cmath>
#include <cstdlib>
#include <iostream>
#include <vector>
extern "C" {
int CyberpunkVR_TwoHandActive = 0;
float CyberpunkVR_WeaponKickDeg = 1;
extern int CyberpunkVR_Haptics;
extern float CyberpunkVR_HapticScale;
}
std::atomic<bool> g_isDriving{false};
namespace cvr::anim {
std::atomic<float> g_wheelBlendRight{0};
std::atomic<float> g_wheelBlendLeft{0};
}
struct Output { int hand; float amp; float seconds; };
std::vector<Output> output;
int OpenXRManager::ApplyHaptic(int hand, float amp, float sec) { output.push_back({hand,amp,sec}); return 0; }
int checks = 0;
void check(bool condition, const char* why) { ++checks; if (!condition) { std::cerr << why << '\n'; std::exit(1); } }
int main() {
    using namespace cvr::haptics;
    HapticRoute().BeginSession(false, false);
    SetLegacyGain(1.25f); SetVehicleGain(1);
    Pulse(1,.45f,45); Pulse(1,1,90); Tick();
    check(output.size()==1 && output[0].hand==1 && output[0].amp==1 && output[0].seconds>.08f,"Quest swing merged and dispatched");
    g_isDriving=true; SetVehicleHaptics(.1f,.1f); output.clear(); Tick();
    check(output.size()==1 && output[0].hand==0,"vehicle does not overwrite active right swing");
    Clear(); output.clear(); Shot(2,true); Tick();
    check(output.size()==2,"two-hand actual shot uses both hands");
    check(std::abs(output[0].amp/output[1].amp-.55f)<.001f,"old weaker support-hand ratio restored");
    Clear(); output.clear(); Play(kMeleeParry,kRight); Pulse(1,.5f,45); Tick();
    check(output.size()==1 && output[0].amp==1 && output[0].seconds<.021f,"upstream parry outranks swing");
    Clear(); output.clear(); SetVehicleHaptics(.3f,.4f); Tick();
    check(output.size()==2 && output[0].seconds==.045f,"vehicle supplies both hands");
    Clear(); output.clear(); SetVehicleHaptics(.2f,.4f); SetVehicleAudioChannels(1); Tick();
    check(output.size()==2 && output[0].amp==output[1].amp,"strongest audio channel reaches both hands");
    output.clear(); SetVehicleAudioChannels(2); Tick();
    check(output.size()==2 && std::abs(output[0].amp-.25f)<.001f && output[0].amp==output[1].amp,"left audio channel reaches both hands");
    output.clear(); SetVehicleAudioChannels(3); Tick();
    check(output.size()==2 && std::abs(output[0].amp-.5f)<.001f && output[0].amp==output[1].amp,"right audio channel reaches both hands");
    output.clear(); SetVehicleAudioChannels(0); SetVehicleAudioRequireGrip(true);
    cvr::anim::g_wheelBlendLeft=1; cvr::anim::g_wheelBlendRight=0; Tick();
    check(output.size()==1 && output[0].hand==0,"grip gate follows left wheel hand only");
    output.clear(); cvr::anim::g_wheelBlendLeft=0; cvr::anim::g_wheelBlendRight=1; Tick();
    check(output.size()==1 && output[0].hand==1,"grip gate follows right wheel hand only");
    output.clear(); cvr::anim::g_wheelBlendRight=0; Tick();
    check(output.empty(),"grip gate suppresses hands off");
    SetVehicleAudioRequireGrip(false); Tick(); check(output.size()==2,"disabling grip gate restores hands-off rumble");
    output.clear(); g_isDriving=false; Tick(); check(output.empty(),"vehicle stops on exit even with fresh audio");
    g_isDriving=true; Sleep(110); Tick(); check(output.empty(),"stale vehicle expires");
    Pulse(1,1,90); Sleep(110); Tick(); check(output.empty(),"stale swing expires");
    Clear(); Pulse(1,.8f,90); CyberpunkVR_Haptics=0; Tick();
    check(output.empty(),"master mute clears queued swing");
    CyberpunkVR_Haptics=1; Tick(); check(output.empty(),"unmute never replays old swing");
    SetVehicleHaptics(.3f,.4f); SetVehicleGain(0); Tick();
    check(output.empty(),"vehicle gain zero mutes existing audio");
    Clear(); HapticRoute().Request(2); HapticRoute().BeginSession(true,true);
    SetVehicleGain(1); SetVehicleHaptics(.5f,.5f); Shot(2,true); Tick();
    check(output.empty(),"Toolkit owns vehicle and shot effects exclusively");
    HapticRoute().BeginSession(true,false); Clear(); Pulse(0,.6f,85); Tick();
    check(output.size()==1 && output[0].hand==0,"PSVR2 without bridge receives generic swing");
    Clear(); output.clear(); Play(kBeltTouch,kLeft); Clear(); Tick();
    check(output.empty(),"focus/session clear drops all envelope events");
    Clear(); output.clear(); Shot(1,false,0,4.0f,true); Tick();
    check(output.size()==1 && output[0].hand==0,"dual native shot is only in firing left hand");
    Clear(); output.clear(); Shot(1,true,1,4.0f,true); Tick();
    check(output.size()==1 && output[0].hand==1,"dual suppresses support-hand vibration even if stale two-hand flag is set");
    Clear(); output.clear(); Play(kUiClick,kLeft); Tick();
    check(output.size()==1 && output[0].hand==0,"0.1.9 VR-menu interaction haptic retained");
    Clear(); output.clear(); HapticRoute().BeginSession(true,true);
    auto& channel=cvr::psvr2::Haptics();channel.events.clear();
    Shot(1,false,0,1.0f,true);Shot(1,false,1,4.0f,true);Tick();
    check(output.empty() && channel.events.size()==2,"dual Toolkit shots never call native output");
    check(channel.events[0].hand==0 && channel.events[0].amplitude==.85f && channel.events[0].duration==75,"left Toolkit dual impact restored");
    check(channel.events[1].hand==1 && channel.events[1].amplitude==1 && channel.events[1].duration==125,"heavy right Toolkit dual impact restored");
    Shot(2,true,1,1.0f,false);Tick();
    check(output.empty() && channel.events.size()==2,"single Toolkit gun remains owned by EDS");
    std::cout << checks << " production haptic playback checks passed\n";
}
