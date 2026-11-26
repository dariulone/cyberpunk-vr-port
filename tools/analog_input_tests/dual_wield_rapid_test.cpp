#include "Hooks/DualWieldRapidFire.hpp"
#include <iostream>
#include <stdexcept>

static void Check(bool yes, const char* why) { if (!yes) throw std::runtime_error(why); }
int main() try {
    using cvr::input::DualWieldRapidFire;
    DualWieldRapidFire gesture;
    Check(!gesture.Update(true, true, false, 100), "one trigger fired hurricane");
    gesture.Update(true, false, false, 120);
    Check(!gesture.Update(true, false, true, 250), "one pair fired hurricane");
    gesture.Update(true, false, false, 270);
    Check(!gesture.Update(true, true, false, 400), "missing second right press accepted");
    gesture.Update(true, false, false, 420);
    Check(gesture.Update(true, false, true, 550), "rapid alternating pulls missed");
    Check(gesture.Update(true, false, true, 749), "request disappeared before state machine can consume it");
    Check(!gesture.Update(true, true, true, 750), "held request never expired");
    for (unsigned t=751; t<1800; ++t)
        Check(!gesture.Update(true, true, true, t), "held automatic triggers repeated hurricane");

    gesture = {};
    for (unsigned t=0; t<1000; t+=100) {
        Check(!gesture.Update(true, true, false, t), "one fast hand triggered hurricane");
        gesture.Update(true, false, false, t+30);
    }
    gesture = {};
    Check(!gesture.Update(true, true, true, 100), "simultaneous first shot triggered hurricane");
    gesture.Update(true, false, false, 150);
    Check(gesture.Update(true, true, true, 400), "rapid paired pulls missed");
    Check(!gesture.Update(false, true, true, 410), "menu did not cancel request");
    Check(!gesture.Update(true, true, true, 420), "menu exit replayed gesture");

    gesture = {};
    gesture.Update(true, true, true, 100);
    gesture.Update(true, false, false, 150);
    Check(!gesture.Update(true, true, true, 701), "slow ordinary fire triggered hurricane");
    gesture.Update(true, false, false, 720);
    Check(gesture.Update(true, true, true, 900), "fresh rapid pair after stale input missed");
    std::cout << "PASS dual-wield rapid cadence, holds, one hand, context cancellation, expiry\n";
    return 0;
} catch (const std::exception& e) { std::cerr << e.what() << '\n'; return 1; }
