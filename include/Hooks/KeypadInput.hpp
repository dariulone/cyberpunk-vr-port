#pragma once
#include "Hooks/AnalogStick.hpp"
#include <cstdint>
#if CVR_RUST_INPUT
#include <cstddef>
#include <cstring>
#endif

namespace cvr::input {
constexpr uint64_t KeypadInputTimeoutMs = 350;
struct KeypadPoint { float x{}, y{}; };

// The C++ reference for rust/crates/cvr-input's keypad module (see AnalogStick.hpp).
namespace cpp {
inline bool KeypadInputFresh(uint64_t stamp, uint64_t now) {
    return stamp != 0 && now >= stamp && now - stamp <= KeypadInputTimeoutMs;
}
inline float KeypadCursorAxis(float value, float deadzone) {
    // UI_MoveCursor* reads the raw left axes, not the paired locomotion axes.
    return .5f * AnalogAxis(value, deadzone, 1.f);
}
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

#if CVR_RUST_INPUT
inline bool KeypadInputFresh(uint64_t stamp, uint64_t now) { return cvr_keypad_input_fresh(stamp, now); }
inline float KeypadCursorAxis(float value, float deadzone) { return cvr_keypad_cursor_axis(value, deadzone); }
struct KeypadCursor {
    KeypadPoint position{}, previousHead{};
    bool initialized{}, hadHead{};

    bool Update(bool headHit, KeypadPoint head, KeypadPoint stick, KeypadPoint size, float dt) {
        CvrKeypadCursor state;
        std::memcpy(&state, this, sizeof state);
        const bool claimed = cvr_keypad_cursor_update(&state, headHit, {head.x, head.y}, {stick.x, stick.y},
                                                      {size.x, size.y}, dt);
        std::memcpy(this, &state, sizeof state);
        return claimed;
    }
};
static_assert(sizeof(KeypadCursor) == sizeof(CvrKeypadCursor) &&
              offsetof(KeypadCursor, previousHead) == offsetof(CvrKeypadCursor, previousHead) &&
              offsetof(KeypadCursor, initialized) == offsetof(CvrKeypadCursor, initialized) &&
              offsetof(KeypadCursor, hadHead) == offsetof(CvrKeypadCursor, hadHead));
#else
using cpp::KeypadInputFresh;
using cpp::KeypadCursorAxis;
using cpp::KeypadCursor;
#endif

bool KeypadInputActive();
void PublishKeypadStick(float x, float y, float deadzone);
}
