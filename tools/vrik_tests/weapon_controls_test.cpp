#include "Anim/HandRecoilSpring.hpp"
#include "Hooks/GameplaySecondary.hpp"
#include "Hooks/Handedness.hpp"
#include "Hooks/WeaponTrigger.hpp"
#include <cmath>
#include <stdexcept>

void check(bool condition,const char* what) {
    if(!condition)throw std::runtime_error(what);
}

int main() {
    for(int bindings=0;bindings<3;++bindings) {
        cvr::input::GameplaySecondary secondary;
        const bool mirror=cvr::input::TradesButtons(bindings);
        const int source=mirror?0:1;
        auto press=[&](bool eligible,uint16_t buttons) {
            const auto logical=cvr::input::MirroredButtons(buttons,mirror,cvr::input::TradesSticks(bindings));
            return secondary.Update(eligible,(logical&0x2000)!=0,source);
        };
        const uint16_t b=mirror?0x8000:0x2000,other=mirror?0x2000:0x8000;
        check(!press(true,b),"equip while held");
        check(!press(true,other) && press(true,b),"mirrored magazine button");
        check(!press(false,b) && !press(true,b),"held overlay exit leaked to gameplay");
        check(!press(true,0) && press(true,b),"new press after close");
        check(!secondary.Update(true,true,1-source),"held button survived binding swap");
    }

    cvr::input::WeaponTrigger trigger;
    check(!trigger.Update(true,0) && trigger.Update(true,.9f),"first squeeze");
    check(trigger.Update(true,.4f) && !trigger.Update(true,.1f),"trigger hysteresis");
    check(!trigger.Update(true,.8f) && trigger.Update(true,.9f),"second squeeze");

    cvr::input::FingerPull finger;
    cvr::input::FingerButton button;
    auto poll = [&](float value, uint64_t weapon = 1) {
        return button.Poll(true, weapon, finger.Sample(true, value));
    };
    check(!poll(0), "initial release");
    for (int i = 0; i < 12; ++i) {
        check(poll(.85f) && poll(.3f), "break or hold lost");
        check(!poll(.2f), "reset lost");
    }
    check(poll(1), "press before asynchronous reset");
    finger.Sample(true, .2f);
    finger.Sample(true, .85f);
    check(!poll(.5f) && poll(.5f), "XR reset and re-press coalesced by game polls");
    finger.Sample(true, 0);
    finger.Sample(true, 1);
    check(!poll(1) && !poll(0) && !poll(0), "replayed a pull already released");
    check(poll(1) && !poll(1, 2) && !poll(1, 2), "equip inherited a held trigger");
    check(!poll(0, 2) && poll(1, 2), "new pull after equip");
    check(!button.Poll(false, 2, finger.Sample(false, 1)), "pause leaked to gameplay");
    check(!poll(1, 2) && !poll(1, 2), "resume while held");
    check(!poll(0, 2) && poll(1, 2), "new pull after resume");
    for (int i = 0; i < 100; ++i) check(poll(.3f, 2), "continuous hold interrupted");

    cvr::input::FingerAction action;
    check(!action.Sample(4,false,10) && !action.Sample(4,true,11), "blocked pull inherited on resume");
    check(!action.Sample(0,false,12) && action.Sample(5,true,13)==5, "first fresh action after resume");
    check(action.Sample(5,true,113)==5 && !action.Sample(5,true,114), "expired action became fresh");
    check(action.Sample(6,true,115)==6 && !action.Sample(6,false,116) && !action.Sample(6,true,117),
          "reload gate repeated an interrupted action");
    check(!action.Sample(0,true,118), "released action queued");

    constexpr float omega=32.f;
    auto sample=[&](int steps) {
        cvr::anim::HandRecoilSpring spring{};
        spring.v=omega;
        for(int i=0;i<steps;++i)spring.Advance(omega,.08f/steps);
        return spring;
    };
    const auto reference=sample(1);
    for(int steps:{4,8,12}) {
        const auto result=sample(steps);
        check(std::abs(reference.x-result.x)<.00001f && std::abs(reference.v-result.v)<.0001f,
              "same elapsed time, different pose feedback");
    }
    auto spring=reference;
    spring.Advance(omega,1.f);
    check(std::abs(spring.x)<.0001f && std::abs(spring.v)<.001f,"pose failed to settle");
}
