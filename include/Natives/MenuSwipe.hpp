#pragma once
#include <cmath>
#include <deque>

// The VR menu's summon swipe (Natives/VrMenu.cpp, VRMenuGesture): the right hand, grip squeezed, swept straight
// down. Here without the game or the runtime, so tools/vr_menu_tests can feed it hand paths.
namespace cvr::vrmenu {

// The swipe: at least this far down, inside this window, at least this fast on average, and mostly
// vertical. Measured against a deliberate flick (~0.3 m in ~0.25 s) and against a hand dropped to the
// side after an action (slower, and it starts low).
constexpr float kSwipeDrop = 0.20f;
constexpr double kSwipeWindow = 0.40;
constexpr float kSwipeSpeed = 0.75f;
constexpr float kSwipeSlant = 0.65f;
constexpr double kSwipeCooldown = 0.9;
// The history kept, a little longer than the window.
constexpr double kSwipeHistory = 0.6;

struct SwipeSample {
    double t;       // seconds
    float p[3];     // tracking space, metres, Y up
    bool held;      // the grip squeezed, and on nothing during this press
};

class SwipeDetector {
public:
    // One sample of the hand. `gripDown`: the grip squeezed; `holding`: that grip is on something (the steering
    // wheel, a ladder, an item in the fingers). A grip that has held something stays out of the swipe until it is
    // released: climbing is the same motion as the swipe (squeeze, pull the hand down), and the climb's last pull
    // goes on after the ladder has let go of the player at the top or on a jump off -- as does a hand that lets go
    // of the wheel or an item while it moves. True once per swipe, with where it started and ended.
    bool Push(double t, const float* p, bool gripDown, bool holding, SwipeSample* start, SwipeSample* end) {
        if (!gripDown) owned_ = false;
        else if (holding) owned_ = true;
        history_.push_back({t, {p[0], p[1], p[2]}, gripDown && !owned_});
        while (!history_.empty() && t - history_.front().t > kSwipeHistory) history_.pop_front();
        if (t - lastFire_ < kSwipeCooldown || history_.size() < 3 || !history_.back().held) return false;

        // The earliest squeezed sample of the window from which the hand fell far and straight enough.
        const SwipeSample& last = history_.back();
        const SwipeSample* first = nullptr;
        for (auto it = history_.rbegin(); it != history_.rend(); ++it) {
            if (last.t - it->t > kSwipeWindow || !it->held) break;
            const float drop = it->p[1] - last.p[1];
            const float dx = it->p[0] - last.p[0], dz = it->p[2] - last.p[2];
            if (drop >= kSwipeDrop && std::sqrt(dx * dx + dz * dz) <= kSwipeSlant * drop &&
                drop / float(last.t - it->t) >= kSwipeSpeed)
                first = &*it;
        }
        if (!first) return false;
        lastFire_ = t;
        if (start) *start = *first;
        if (end) *end = last;
        history_.clear();
        return true;
    }

    // Tracking lost or the gesture turned off: the window starts again. What the grip held is kept -- it is still
    // the same press as far as anything here knows.
    void Clear() { history_.clear(); }

private:
    std::deque<SwipeSample> history_;
    double lastFire_ = -10.0;
    bool owned_ = false;    // this press of the grip has held something
};

}  // namespace cvr::vrmenu
