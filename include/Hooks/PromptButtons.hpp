#pragma once
// WHICH BUTTON A PROMPT SHOWS. The game draws the pad button bound to an action -- a part of its controller icon
// atlas (base\gameplay\gui\common\input\icons_durango.inkatlas: "x", "a_hold", "right_trigger", "left_axis_up"...).
// The port's XInput merge does not hand the game the controllers' buttons one to one: in a car physical A also
// presses X (a dialogue line's confirm, src/Hooks/XInput.cpp) and a held X presses B (the exit), so the game's prompt
// names a button the player does not press -- and left-handed (Hooks/Handedness.hpp) every pad button is pressed
// by the other hand's. PromptPart maps the part the game wants to show onto the part of the physical button that
// produces that pad button now; a part it does not know is shown as it is. It is applied to every key icon the game
// looks up (src/Natives/InputPrompts.cpp) and to the parts the UI scripts set themselves (VRPromptPart).
#include <string>
#include <string_view>

namespace RED4ext { struct CGameApplication; }

namespace cvr::input {

struct PromptLayout {
    bool vehicle{};      // mounted with the seat's mirrors on: pad X <- A, pad B <- X (held)
    bool leftHanded{};   // the controllers' roles traded: pad A <- physical X, pad LT <- physical RT...
    int bindings{2};     // how far (xr_left_handed_bindings, Hooks/Handedness.hpp): 0 triggers, 1 + buttons, 2 + sticks
};

// The layout the merge applies this frame (src/Natives/InputPrompts.cpp).
PromptLayout CurrentPromptLayout();

// "x_hold" -> "a_hold" in a car; then, left-handed, the physical button of that role ("a_hold" -> "x_hold");
// unknown parts unchanged.
std::string PromptPart(std::string_view part, const PromptLayout& layout);

// Every key icon on screen looked up again -- a widget keeps the part it was given until then. On the main thread;
// false while the game has no icon mapping yet.
bool RefreshPromptIcons();

// A RED4ext Running state's update (src/Main.cpp): the icons refreshed when the layout changes.
bool UpdatePromptIcons(RED4ext::CGameApplication*);

}  // namespace cvr::input
