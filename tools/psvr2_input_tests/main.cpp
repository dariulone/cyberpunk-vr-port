#include "Hooks/Psvr2Input.hpp"
#include <cstdlib>
#include <iostream>
#include <limits>

static int checks = 0;
static void check(bool result, const char* text) {
    ++checks;
    if (!result) { std::cerr << "FAIL: " << text << '\n'; std::exit(1); }
}
int main() {
    using namespace cvr::input;
    check(IsPsvr2System("SteamVR/OpenXR : playstation_vr2"), "SteamVR Sense identity");
    check(IsPsvr2System("PlayStation VR2"), "case-insensitive identity");
    check(!IsPsvr2System("Oculus Quest2") && !IsPsvr2System(nullptr), "other HMDs unchanged");
    for (auto direction : {uint16_t(1),uint16_t(2),uint16_t(4),uint16_t(8)}) {
        float x = direction == 4 ? -.8f : direction == 8 ? .8f : 0;
        float y = direction == 1 ? .8f : direction == 2 ? -.8f : 0;
        uint16_t buttons = 0x8000; // Triangle click remains Y.
        check(TriangleDpad(true,x,y,buttons), "direction emitted");
        check(buttons == (0x8000 | direction) && x == 0 && y == 0, "Y preserved, look suppressed");
    }
    float x=.2f,y=.1f; uint16_t buttons=0;
    check(!TriangleDpad(true,x,y,buttons) && x==0 && y==0 && buttons==0, "whole-hold suppression below threshold");
    x=.8f; y=.9f;
    check(!TriangleDpad(false,x,y,buttons) && x==.8f && y==.9f, "unshifted/other HMD unaffected");
    check(TriangleDpad(true,x,y,buttons) && buttons==9, "diagonal D-pad");
    x=std::numeric_limits<float>::quiet_NaN(); y=0; buttons=0;
    check(!TriangleDpad(true,x,y,buttons) && x==0, "invalid axis neutralized");
    SenseSystemButton system;
    check(system.Update(true,1000,true)==0, "tap does not emit on press");
    check(system.Update(false,1499,true)==0x10, "499 ms release is Start");
    check(system.Update(false,1500,true)==0, "tap one shot");
    check(system.Update(true,2000,true)==0 && system.Update(true,2499,true)==0, "hold waits");
    check(system.Update(true,2500,true)==0x20, "500 ms hold is Back");
    check(system.Update(true,2700,true)==0 && system.Update(false,2800,true)==0, "long release never adds Start");
    check(system.Update(true,3000,true)==0 && system.Update(false,3500,true)==0x20, "slow poll across hold threshold");
    check(system.Update(true,4000,true)==0 && system.Update(false,4100,false)==0 && system.Update(false,4200,true)==0, "focus loss cancels tap");
    check(system.Update(true,5000,false)==0 && system.Update(false,5100,true)==0, "unavailable controller ignored");
    std::cout << checks << " PSVR2 policy checks passed\n";
}
