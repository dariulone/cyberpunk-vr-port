#pragma once
// THE LEFT-HANDED MODE (xr_left_handed, Core/LiveControls.hpp): the weapon hand is the left one, so the two
// controllers trade what they mean to the game. The left trigger fires and the right one aims, the right stick
// walks and the left one turns, dashes and crouches, X jumps where A did, Y drops the magazine where B did, A
// reloads and interacts where X did, B switches weapons where Y did, and the right stick's click is the D-pad's
// where the left one's was. Every prompt names the button that does it now.
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

// The pad buttons with A and X, B and Y and the two stick clicks traded. The shoulders, Start, Back and the D-pad
// are no hand's (the shoulders are the grips' menu paging, emitted from the hands as they are) and stay.
constexpr uint16_t MirroredButtons(uint16_t buttons) {
    constexpr uint16_t A = 0x1000, B = 0x2000, X = 0x4000, Y = 0x8000, LS = 0x0040, RS = 0x0080;
    const uint16_t kept = static_cast<uint16_t>(buttons & ~(A | B | X | Y | LS | RS));
    return static_cast<uint16_t>(kept | ((buttons & A) ? X : 0) | ((buttons & X) ? A : 0) | ((buttons & B) ? Y : 0) |
                                 ((buttons & Y) ? B : 0) | ((buttons & LS) ? RS : 0) | ((buttons & RS) ? LS : 0));
}

// A controller snapshot (VRControllerState, Runtimes/OpenXRManager.hpp) with its two hands' roles traded.
template <class State>
State Mirrored(State s) {
    State out = s;
    out.buttons = MirroredButtons(s.buttons);
    out.leftTrigger = s.rightTrigger;
    out.rightTrigger = s.leftTrigger;
    out.leftThumbX = s.rightThumbX;
    out.leftThumbY = s.rightThumbY;
    out.rightThumbX = s.leftThumbX;
    out.rightThumbY = s.leftThumbY;
    out.leftGrip = s.rightGrip;
    out.rightGrip = s.leftGrip;
    out.leftHandValid = s.rightHandValid;
    out.rightHandValid = s.leftHandValid;
    return out;
}

// The controller icon atlas part of the mirrored button (the game's names: engine\ink\input\
// input_key_icon_dictionary.json, base\gameplay\gui\common\input\icons_durango.inkatlas): "a_hold" -> "x_hold",
// "left_trigger" -> "right_trigger", "right_axis_up" -> "left_axis_up", "ls_filled" -> "rs_filled". The shoulders
// stay, as the buttons do; a part the atlas has no mirror of ("y_wide") is unchanged.
inline std::string MirroredPart(std::string_view part) {
    const auto swap = [&](std::string_view from, std::string_view to) -> std::string {
        return std::string(to) + std::string(part.substr(from.size()));
    };
    const auto starts = [&](std::string_view prefix) { return part.substr(0, prefix.size()) == prefix; };
    if (starts("left_") && !starts("left_shoulder")) return swap("left_", "right_");
    if (starts("right_") && !starts("right_shoulder")) return swap("right_", "left_");
    if (starts("ls_")) return swap("ls_", "rs_");
    if (starts("rs_")) return swap("rs_", "ls_");
    const auto cut = part.find('_');
    const std::string_view base = part.substr(0, cut), rest = cut == std::string_view::npos ? "" : part.substr(cut);
    if (rest != "" && rest != "_hold" && rest != "_disabled") return std::string(part);
    const char* traded = base == "a" ? "x" : base == "x" ? "a" : base == "b" ? "y" : base == "y" ? "b" : nullptr;
    return traded ? std::string(traded) + std::string(rest) : std::string(part);
}

}  // namespace cvr::input
