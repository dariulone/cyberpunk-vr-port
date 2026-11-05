// The VR menu's summon swipe (include/Natives/MenuSwipe.hpp) fed with simulated right-hand paths at 90 Hz.
// Each case is a separate ctest; "old" runs the same path with the grip's hold judged per sample, as
// VrMenu.cpp did before a grip that held something stayed out of the swipe until released.
#include "Natives/MenuSwipe.hpp"
#include <cstdlib>
#include <iostream>
#include <string>

using cvr::vrmenu::SwipeDetector;
using cvr::vrmenu::SwipeSample;

namespace {

void Check(bool ok, const char* text) {
    if (!ok) {
        std::cerr << "FAIL: " << text << '\n';
        std::exit(1);
    }
}

constexpr double kDt = 1.0 / 90.0;

struct Hand {
    SwipeDetector swipe;
    bool old = false;   // per-sample hold, the previous behaviour
    double t = 10.0;
    float p[3] = {0.25f, 1.35f, -0.30f};
    int fires = 0;
    SwipeSample start{}, end{};

    void Sample(bool grip, bool holding) {
        const bool fired = old ? swipe.Push(t, p, grip && !holding, false, &start, &end)
                               : swipe.Push(t, p, grip, holding, &start, &end);
        if (fired) ++fires;
        t += kDt;
    }
    // Moves the hand by (dx, dy, dz) metres over `seconds`, one sample per frame.
    void Move(float dx, float dy, float dz, double seconds, bool grip, bool holding) {
        const int n = seconds <= 0 ? 1 : int(seconds / kDt + 0.5);
        for (int i = 0; i < n; ++i) {
            p[0] += dx / n;
            p[1] += dy / n;
            p[2] += dz / n;
            Sample(grip, holding);
        }
    }
    void Hold(double seconds, bool grip, bool holding) { Move(0, 0, 0, seconds, grip, holding); }
};

// A deliberate swipe: still for a moment, then 0.30 m down in 0.25 s.
void Swipe(Hand& h, bool grip = true, bool holding = false) {
    h.Hold(0.10, grip, holding);
    h.Move(0, -0.30f, 0, 0.25, grip, holding);
    h.Hold(0.10, grip, holding);
}

// One pull of a grip climb: squeeze, pull 0.35 m down in 0.30 s, let go, reach 0.35 m up in 0.40 s.
void ClimbPull(Hand& h) {
    h.Hold(0.05, true, true);
    h.Move(0, -0.35f, 0, 0.30, true, true);
    h.Hold(0.05, false, true);
    h.Move(0, 0.35f, 0, 0.40, false, true);
}

// The last pull at the top: 0.12 m with the ladder still holding the player, then the ladder lets go (top exit
// or auto finish) and the same pull goes on, 0.30 m in 0.25 s, the grip still squeezed.
void TopExitPull(Hand& h) {
    h.Hold(0.05, true, true);
    h.Move(0, -0.12f, 0, 0.10, true, true);
    h.Move(0, -0.30f, 0, 0.25, true, false);
    h.Hold(0.30, true, false);
}

void FreeSwipe() {
    Hand h;
    Swipe(h);
    Check(h.fires == 1, "a free hand's gripped swipe did not open the menu");
    Check(h.start.p[1] - h.end.p[1] >= cvr::vrmenu::kSwipeDrop, "the swipe's start and end are not the drop");
}

void NoGrip() {
    Hand h;
    Swipe(h, false);
    Check(h.fires == 0, "an open hand dropping opened the menu");
}

void Slow() {
    Hand h;
    h.Hold(0.10, true, false);
    h.Move(0, -0.30f, 0, 1.00, true, false);
    Check(h.fires == 0, "a slow lowering of a gripped hand opened the menu");
}

void Slanted() {
    Hand h;
    h.Hold(0.10, true, false);
    h.Move(0.25f, -0.25f, 0, 0.25, true, false);
    Check(h.fires == 0, "a diagonal sweep opened the menu");
}

void Cooldown() {
    Hand h;
    Swipe(h);
    h.Move(0, 0.30f, 0, 0.15, false, false);
    Swipe(h);
    Check(h.fires == 1, "a second swipe inside the cooldown fired");
    h.Hold(1.0, false, false);
    h.Move(0, 0.30f, 0, 0.15, false, false);
    Swipe(h);
    Check(h.fires == 2, "a swipe after the cooldown did not fire");
}

void LadderClimb() {
    for (int mode = 0; mode < 2; ++mode) {
        Hand h;
        h.old = mode == 1;
        for (int i = 0; i < 8; ++i) ClimbPull(h);
        Check(h.fires == 0, "grip climbing opened the menu");
    }
}

void LadderTopExit() {
    Hand old;
    old.old = true;
    for (int i = 0; i < 3; ++i) ClimbPull(old);
    TopExitPull(old);
    Check(old.fires == 1, "the simulated top exit is not the case the per-sample hold missed");

    Hand h;
    for (int i = 0; i < 3; ++i) ClimbPull(h);
    TopExitPull(h);
    Check(h.fires == 0, "the climb's last pull went on after the top exit and opened the menu");
}

void LadderJumpOff() {
    Hand h;
    for (int i = 0; i < 2; ++i) ClimbPull(h);
    // Squeezed on a rung, the ladder lets go mid-move (a jump off), the hand drops with the body.
    h.Hold(0.10, true, true);
    h.Move(0, -0.40f, 0.05f, 0.30, true, false);
    h.Hold(0.20, true, false);
    Check(h.fires == 0, "a jump off the ladder with the grip squeezed opened the menu");
}

void RegripAfterExit() {
    Hand h;
    for (int i = 0; i < 3; ++i) ClimbPull(h);
    TopExitPull(h);
    Check(h.fires == 0, "the top exit opened the menu");
    h.Hold(0.20, false, false);
    h.Move(0, 0.30f, 0, 0.20, false, false);
    h.Hold(1.0, false, false);
    Swipe(h);
    Check(h.fires == 1, "after letting go of the grip a new press could not open the menu");
}

void WheelLetGo() {
    Hand old;
    old.old = true;
    old.Hold(0.20, true, true);
    old.Move(0, -0.08f, 0, 0.10, true, true);
    old.Move(0, -0.30f, 0, 0.25, true, false);
    Check(old.fires == 1, "the simulated wheel let-go is not the case the per-sample hold missed");

    Hand h;
    h.Hold(0.20, true, true);
    h.Move(0, -0.08f, 0, 0.10, true, true);
    h.Move(0, -0.30f, 0, 0.25, true, false);
    h.Hold(0.20, true, false);
    Check(h.fires == 0, "a hand leaving the wheel mid-move with the grip squeezed opened the menu");
}

void TrackingLost() {
    Hand h;
    h.Hold(0.05, true, true);
    h.Move(0, -0.10f, 0, 0.10, true, true);
    h.swipe.Clear();   // tracking lost for a moment mid-press, the ladder has let go when it is back
    h.Move(0, -0.30f, 0, 0.25, true, false);
    h.Hold(0.20, true, false);
    Check(h.fires == 0, "a tracking gap made a held grip count for the swipe");
}

}  // namespace

int main(int argc, char** argv) {
    const std::string test = argc > 1 ? argv[1] : "";
    if (test == "free_swipe") FreeSwipe();
    else if (test == "no_grip") NoGrip();
    else if (test == "slow") Slow();
    else if (test == "slanted") Slanted();
    else if (test == "cooldown") Cooldown();
    else if (test == "ladder_climb") LadderClimb();
    else if (test == "ladder_top_exit") LadderTopExit();
    else if (test == "ladder_jump_off") LadderJumpOff();
    else if (test == "regrip_after_exit") RegripAfterExit();
    else if (test == "wheel_let_go") WheelLetGo();
    else if (test == "tracking_lost") TrackingLost();
    else return 2;
    std::cout << "PASS " << test << '\n';
    return 0;
}
