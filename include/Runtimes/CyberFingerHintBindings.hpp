#pragma once
#include <atomic>
#include <cstdint>
#include <string>
#include <string_view>

namespace cvr::cyberfinger {
// Only advertise controls for which OpenXR reports the physical source used by
// the input handler. Per-action checks also work for partially remapped layouts.
enum HintSource : uint32_t {
    LeftTrigger=1, RightTrigger=2, LeftGrip=4, RightGrip=8,
    LeftMenu=16, RightMenu=32, LeftBlack=64, RightBlack=128,
    LeftBlackHold=256, RightBlackHold=512, LeftStick=1024, RightStick=2048
};
inline std::atomic<uint32_t> hintSources{};

inline uint32_t PhysicalHintSource(std::string_view hand, std::string_view component) {
    auto lower = [](std::string_view value) {
        std::string text(value);
        for (auto& c : text) if (c >= 'A' && c <= 'Z') c = static_cast<char>(c + ('a' - 'A'));
        return text;
    };
    const auto side = lower(hand), input = lower(component);
    const bool left = side == "left hand" || side == "left" || side == "left controller";
    if (!left && side != "right hand" && side != "right" && side != "right controller") return 0;
    // Driver-localized names, including their optional SteamVR slot suffixes.
    auto named = [&](std::string_view name) {
        if (!input.starts_with(name)) return false;
        auto suffix = std::string_view(input).substr(name.size());
        if (suffix.empty()) return true;
        const auto separator = suffix.find_first_not_of(" /-");
        if (separator == 0 || separator == suffix.npos) return false;
        suffix.remove_prefix(separator);
        return suffix == "click" || suffix == "press" || suffix == "pull" ||
               suffix == "value" || suffix == "position";
    };
    if (named("primary (trigger)") || named("trigger")) return left ? LeftTrigger : RightTrigger;
    if (named("grip (the grab button, as pressed)") || named("grip")) return left ? LeftGrip : RightGrip;
    if (named("b (menu: context dial)") || named("b") || named("b button") || named("button b"))
        return left ? LeftMenu : RightMenu;
    if (input.starts_with("a held (black wrist button,"))
        return left ? LeftBlackHold : RightBlackHold;
    if (named("a (start/select, black wrist button)") || named("a") || named("a button") || named("button a"))
        return left ? LeftBlack : RightBlack;
    if (named("joystick") || named("thumbstick"))
        return left ? LeftStick : RightStick;
    return 0;
}

struct HintLayout {
    uint32_t sources{};
    bool leftWeapon{}, leftMouse{}, seated{}, exitGrip{}, driving{}, vehicleFire{};
    bool dialogue{}, interaction{}, defaultExitTime{};
};

inline std::string ControlHint(std::string_view action, bool hold, const HintLayout& s) {
    auto button = [&](HintSource source, const char* label) -> std::string {
        return (s.sources & source) ? label : "";
    };
    if (action == "ChoiceApply" && s.dialogue)
        return button(RightTrigger, "Right Trigger");
    if (action == "ChoiceScroll" && s.dialogue)
        return button(RightStick, "Right Stick Up/Down");
    if (s.interaction && !s.seated && !s.dialogue) {
        if (action == "Choice1" || action == "Choice1_Hold" || action == "Choice1_Release") {
            hold |= action == "Choice1_Hold";
            return s.leftMouse ? button(LeftTrigger, hold ? "Hold Left Trigger" : "Left Trigger")
                               : button(RightTrigger, hold ? "Hold Right Trigger" : "Right Trigger");
        }
        if (action == "Choice2" || action == "Choice2_Hold" || action == "Choice2_Release") {
            hold |= action == "Choice2_Hold";
            return s.leftMouse ? button(LeftGrip, hold ? "Hold Left Grip" : "Left Grip")
                               : button(RightGrip, hold ? "Hold Right Grip" : "Right Grip");
        }
    }
    if (s.dialogue) return {};
    if (s.exitGrip && (action == "Exit" || action == "ExitCombatMode"))
        return button(LeftGrip, s.defaultExitTime ? "Hold Left Grip 0.4 s" : "Hold Left Grip");
    if ((s.driving && action == "EnterCombatMode") || action == "SwitchItem")
        return button(LeftMenu, "Tap Left Menu");
    if (action == "WeaponWheel") return button(LeftMenu, "Hold Left Menu");
    if (action == "Reload" && (!s.seated || !s.exitGrip))
        return s.leftWeapon ? button(RightBlack, "Tap Right Black Wrist") : button(LeftBlack, "Tap Left Black Wrist");
    if (!s.seated && action == "UseConsumable")
        return s.leftWeapon ? button(RightBlackHold, "Hold Right Black Wrist") : button(LeftBlackHold, "Hold Left Black Wrist");
    if (!s.seated && action == "IconicCyberware") return button(RightMenu, "Hold Right Menu");
    if ((!s.seated && action == "RangedAttack" && !s.interaction) ||
        (s.vehicleFire && (action == "UI_FakeRangedAttack" || action == "RangedAttack")) ||
        (s.driving && !s.vehicleFire && action == "Accelerate"))
        return s.leftWeapon ? button(LeftTrigger, "Left Trigger") : button(RightTrigger, "Right Trigger");
    if ((!s.seated && !s.interaction && action == "CameraAim") || (s.driving && action == "Decelerate"))
        return s.leftWeapon ? button(RightTrigger, "Right Trigger") : button(LeftTrigger, "Left Trigger");
    return {};
}
}
