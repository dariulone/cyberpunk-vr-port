#pragma once
#include <cstdint>

namespace cvr::input {
// THE PORT'S WEAPON WHEEL TAKES THE HOLD OF Y. The game reads Y as SwitchItem (a tap: the next weapon) and as
// WeaponWheel (held kHoldMs: its own wheel, which slows time and moves the input to a UI context). The port's
// wheel (mods/cet/CyberpunkVRPort_Menu/modules/wheel.lua) keeps the game running, so the game must never see the
// hold: not to open its own wheel, and not to read the release that ends it as SwitchItem -- with nothing in hand
// that release draws the last weapon (upperBodyTransitions). A press is therefore withheld until it is decided. Let
// go before kHoldMs and the game gets a tap of kTapMs; held kHoldMs it is the wheel's until it ends, and the game
// sees none of it.
//
// Not armed (the setting off, no wheel script running, a menu or the scanner up), Y is the game's as it was. A
// hold already given to the wheel stays swallowed to its end even so: passed on half way, it would open the
// game's wheel after all.
struct WheelHoldResult {
    bool gameY{};      // Y as the game sees it
    bool pending{};    // pressed, not decided yet
    bool held{};       // the wheel's hold
};

class WheelHold {
public:
    static constexpr uint64_t kHoldMs = 300;   // the game's own WeaponWheel hold timeout (inputContexts.xml)
    static constexpr uint64_t kTapMs = 90;     // long enough for two game frames at 30 fps, short of any hold

    WheelHoldResult Step(bool y, bool armed, uint64_t now) {
        if (!y) {
            if (m_down && m_owned && !m_held && armed) m_tapUntil = now + kTapMs;
            m_down = m_held = false;
        } else if (!m_down) {
            m_down = true;
            m_since = now;
            m_owned = armed;
        } else if (m_owned && !m_held && armed && now - m_since >= kHoldMs) {
            m_held = true;
        }
        // A press that started unarmed is the game's to its end; one armed and undecided falls back to the game
        // when the arming goes.
        if (m_down && !m_held && !armed) m_owned = false;
        WheelHoldResult r;
        r.held = m_held;
        r.pending = m_down && m_owned && !m_held;
        r.gameY = now < m_tapUntil || (m_down && !m_owned);
        return r;
    }

private:
    uint64_t m_since{}, m_tapUntil{};
    bool m_down{}, m_held{}, m_owned{};
};
}
