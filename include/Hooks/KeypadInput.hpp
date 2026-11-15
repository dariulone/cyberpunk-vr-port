#pragma once
#include "Hooks/AnalogStick.hpp"
#include <cstdint>

namespace cvr::input {
constexpr uint64_t KeypadInputTimeoutMs = 350;
inline bool KeypadInputFresh(uint64_t stamp, uint64_t now) {
    return stamp != 0 && now >= stamp && now - stamp <= KeypadInputTimeoutMs;
}
inline float KeypadCursorAxis(float value, float deadzone) {
    // UI_MoveCursor* reads the raw left axes, not the paired locomotion axes.
    return .5f * AnalogAxis(value, deadzone, 1.f);
}
bool KeypadInputActive();
bool ComputerInputActive();
bool ComputerPointerHit();
void ComputerBack();
void PublishKeypadStick(float x, float y, float deadzone);

struct KeypadPoint { float x{}, y{}; };
struct KeypadCursor {
    KeypadPoint position{}, previousHead{};
    bool initialized{}, hadHead{};

    // Head movement remains relative to the native panel hit. The stick works
    // in panel pixels, so a tiny/distant screen cannot amplify its speed.
    bool Update(bool headHit, KeypadPoint head, KeypadPoint stick,
                KeypadPoint size, float dt) {
        if (!std::isfinite(size.x) || !std::isfinite(size.y) || size.x < 2 || size.y < 2) {
            *this = {}; return false;
        }
        headHit = headHit && std::isfinite(head.x) && std::isfinite(head.y);
        if (!initialized) {
            if (!headHit) return false;
            position = head; initialized = true;
        } else if (headHit && hadHead) {
            position.x += head.x - previousHead.x;
            position.y += head.y - previousHead.y;
        }
        hadHead = headHit;
        if (headHit) previousHead = head;
        const float step = std::isfinite(dt) ? std::clamp(dt, 0.f, .05f) : 0.f;
        const float speed = 1.5f * std::min(size.x, size.y);
        if (std::isfinite(stick.x)) position.x += std::clamp(stick.x, -.5f, .5f) * speed * step;
        if (std::isfinite(stick.y)) position.y -= std::clamp(stick.y, -.5f, .5f) * speed * step;
        position.x = std::clamp(position.x, 1.f, size.x - 1.f);
        position.y = std::clamp(position.y, 1.f, size.y - 1.f);
        return true;
    }
};
}
