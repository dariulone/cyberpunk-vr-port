#include "Hooks/WeaponWheelHold.hpp"
#include <cstdio>
#include <stdexcept>
static void Check(bool value, const char* what) {
    if (!value) { std::fprintf(stderr, "FAIL: %s\n", what); throw std::runtime_error(what); }
}
int main() {
    using cvr::input::WheelHold;
    constexpr uint64_t T = 1000;
    {   // a tap reaches the game as a tap, after the release
        WheelHold w;
        auto r = w.Step(true, true, T);
        Check(!r.gameY && r.pending && !r.held, "a press is withheld while undecided");
        r = w.Step(true, true, T + 120);
        Check(!r.gameY && r.pending, "still undecided before the hold time");
        r = w.Step(false, true, T + 150);
        Check(r.gameY && !r.pending && !r.held, "a release before the hold time is a tap");
        r = w.Step(false, true, T + 150 + WheelHold::kTapMs - 1);
        Check(r.gameY, "the tap lasts kTapMs");
        r = w.Step(false, true, T + 150 + WheelHold::kTapMs);
        Check(!r.gameY, "and then ends");
    }
    {   // a hold is the wheel's and the game sees none of it, release included
        WheelHold w;
        w.Step(true, true, T);
        auto r = w.Step(true, true, T + WheelHold::kHoldMs);
        Check(r.held && !r.pending && !r.gameY, "held kHoldMs it is the wheel's");
        r = w.Step(true, true, T + 2000);
        Check(r.held && !r.gameY, "for as long as it lasts");
        r = w.Step(false, true, T + 2010);
        Check(!r.held && !r.gameY, "its release is not a tap");
        r = w.Step(false, true, T + 2020);
        Check(!r.gameY, "nor later");
    }
    {   // unarmed, Y is the game's
        WheelHold w;
        auto r = w.Step(true, false, T);
        Check(r.gameY && !r.pending && !r.held, "unarmed press passes");
        r = w.Step(true, true, T + 500);
        Check(r.gameY && !r.held, "a press that began unarmed stays the game's when the arming comes");
        r = w.Step(false, true, T + 510);
        Check(!r.gameY, "its release is the game's own release, no tap");
    }
    {   // the arming goes while undecided: the press falls back to the game
        WheelHold w;
        w.Step(true, true, T);
        auto r = w.Step(true, false, T + 100);
        Check(r.gameY && !r.pending, "undecided press falls back to the game");
        r = w.Step(true, true, T + 400);
        Check(r.gameY && !r.held, "and stays the game's");
    }
    {   // the arming goes during the wheel's hold: still swallowed to its end
        WheelHold w;
        w.Step(true, true, T);
        w.Step(true, true, T + 300);
        auto r = w.Step(true, false, T + 400);
        Check(r.held && !r.gameY, "a wheel hold is not handed to the game half way");
        r = w.Step(false, false, T + 500);
        Check(!r.held && !r.gameY, "and ends without a tap");
    }
    {   // a new press during the tap pulse
        WheelHold w;
        w.Step(true, true, T);
        w.Step(false, true, T + 50);
        auto r = w.Step(true, true, T + 80);
        Check(r.gameY && r.pending, "the pulse finishes under a new press");
        r = w.Step(true, true, T + 50 + WheelHold::kTapMs + 1);
        Check(!r.gameY && r.pending, "the new press is withheld after the pulse");
        r = w.Step(true, true, T + 80 + WheelHold::kHoldMs);
        Check(r.held, "and can become a hold");
    }
    std::puts("weapon wheel hold: all checks passed");
    return 0;
}
