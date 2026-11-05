#pragma once
// THE LEFT-HANDED MODE (xr_left_handed, Core/LiveControls.hpp): the weapon hand is the left one, so the two
// controllers trade what they mean to the game -- as much of it as the player chose (xr_left_handed_bindings):
//
//   0  SAME AS RIGHT-HANDED: the buttons and the sticks keep their right-handed meaning; only the triggers and the grips
//      go with the weapon hand (the left trigger fires and the right one aims).
//   1  THE LEFT STICK WALKS, THE BUTTONS MIRRORED: the triggers and grips traded, and X jumps where A did, Y drops the
//      magazine where B did, A reloads and interacts where X did, B switches weapons where Y did; the sticks (and
//      their clicks) stay where they were.
//   2  EVERYTHING MIRRORED (the default, the first left-handed mode): as 1, and the right stick walks while the left
//      one turns, dashes and crouches; the right stick's click is the D-pad's where the left one's was.
//
// Every prompt names the button that does it now.
//
// The trade is made ONCE, where the controllers' state enters the XInput merge (src/Hooks/XInput.cpp), so all the
// merge does with it -- the pad it hands the game, the gestures read off the sticks, the port's own buttons and
// the shared slots the CET mods read as "the weapon hand" (right) and "the other hand" (left) -- follows. What a
// hand does as a hand stays that hand's, read from the controllers as they are: its fingers, its grip on a ladder
// rung, the arm cyberware it fires, and the menu tab its grip pages toward (a grip pages to its own side).
#include <cstdint>
#include <string>
#include <string_view>

namespace cvr::input {

// What each choice of xr_left_handed_bindings trades.
constexpr bool TradesButtons(int bindings) { return bindings >= 1; }
constexpr bool TradesSticks(int bindings) { return bindings >= 2; }

// The pad buttons with A and X, B and Y traded (`faces`) and the two stick clicks traded (`sticks`). The shoulders,
// Start, Back and the D-pad are no hand's (the shoulders are the grips' menu paging, emitted from the hands as they
// are) and stay.
constexpr uint16_t MirroredButtons(uint16_t buttons, bool faces = true, bool sticks = true) {
    constexpr uint16_t A = 0x1000, B = 0x2000, X = 0x4000, Y = 0x8000, LS = 0x0040, RS = 0x0080;
    uint16_t out = buttons;
    if (faces) {
        out = static_cast<uint16_t>((out & ~(A | B | X | Y)) | ((buttons & A) ? X : 0) | ((buttons & X) ? A : 0) |
                                    ((buttons & B) ? Y : 0) | ((buttons & Y) ? B : 0));
    }
    if (sticks) {
        out = static_cast<uint16_t>((out & ~(LS | RS)) | ((buttons & LS) ? RS : 0) | ((buttons & RS) ? LS : 0));
    }
    return out;
}

// A controller snapshot (VRControllerState, Runtimes/OpenXRManager.hpp) with its two hands' roles traded as far as
// `bindings` (xr_left_handed_bindings) says: the triggers and grips always, the face buttons from 1, the sticks at 2.
template <class State>
State Mirrored(State s, int bindings = 2) {
    State out = s;
    out.buttons = MirroredButtons(s.buttons, TradesButtons(bindings), TradesSticks(bindings));
    out.leftTrigger = s.rightTrigger;
    out.rightTrigger = s.leftTrigger;
    if (TradesSticks(bindings)) {
        out.leftThumbX = s.rightThumbX;
        out.leftThumbY = s.rightThumbY;
        out.rightThumbX = s.leftThumbX;
        out.rightThumbY = s.leftThumbY;
    }
    out.leftGrip = s.rightGrip;
    out.rightGrip = s.leftGrip;
    out.leftHandValid = s.rightHandValid;
    out.rightHandValid = s.leftHandValid;
    return out;
}

// The controller icon atlas part of the mirrored button (the game's names: engine\ink\input\
// input_key_icon_dictionary.json, base\gameplay\gui\common\input\icons_durango.inkatlas): "a_hold" -> "x_hold",
// "left_trigger" -> "right_trigger", "right_axis_up" -> "left_axis_up", "ls_filled" -> "rs_filled". The shoulders
// stay, as the buttons do; a part the atlas has no mirror of ("y_wide") is unchanged. `bindings` as in Mirrored: the
// triggers always, the face buttons from 1, the sticks (axes and clicks) at 2.
inline std::string MirroredPart(std::string_view part, int bindings = 2) {
    const auto swap = [&](std::string_view from, std::string_view to) -> std::string {
        return std::string(to) + std::string(part.substr(from.size()));
    };
    const auto starts = [&](std::string_view prefix) { return part.substr(0, prefix.size()) == prefix; };
    const bool trigger = part.find("trigger") != std::string_view::npos;
    if (starts("left_") && !starts("left_shoulder")) {
        return (trigger || TradesSticks(bindings)) ? swap("left_", "right_") : std::string(part);
    }
    if (starts("right_") && !starts("right_shoulder")) {
        return (trigger || TradesSticks(bindings)) ? swap("right_", "left_") : std::string(part);
    }
    if (starts("ls_")) return TradesSticks(bindings) ? swap("ls_", "rs_") : std::string(part);
    if (starts("rs_")) return TradesSticks(bindings) ? swap("rs_", "ls_") : std::string(part);
    if (!TradesButtons(bindings)) return std::string(part);
    const auto cut = part.find('_');
    const std::string_view base = part.substr(0, cut), rest = cut == std::string_view::npos ? "" : part.substr(cut);
    if (rest != "" && rest != "_hold" && rest != "_disabled") return std::string(part);
    const char* traded = base == "a" ? "x" : base == "x" ? "a" : base == "b" ? "y" : base == "y" ? "b" : nullptr;
    return traded ? std::string(traded) + std::string(rest) : std::string(part);
}

}  // namespace cvr::input
