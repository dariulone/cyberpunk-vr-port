#pragma once
#include "Runtimes/CyberFingerUi.hpp"
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
    LeftBlackHold=256, RightBlackHold=512, LeftStick=1024, RightStick=2048,
    LeftStickClick=4096, RightStickClick=8192
};
inline std::atomic<uint32_t> hintSources{};
enum FootHint : unsigned { Walking=1, MoveRight=2, DashEnabled=4, DualGuns=8, ClickSprint=16, WornGearShortcut=32 };
inline std::atomic<unsigned> footHints{};

inline uint32_t PhysicalHintSource(std::string_view hand, std::string_view component, bool clickAction = false) {
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
    if (named("joystick") || named("thumbstick")) {
        if (input.ends_with("click") || input.ends_with("press")) return left ? LeftStickClick : RightStickClick;
        if (clickAction && (input == "joystick" || input == "thumbstick")) return left ? LeftStickClick : RightStickClick;
        return left ? LeftStick : RightStick;
    }
    return 0;
}

struct HintLayout {
    uint32_t sources{};
    bool leftWeapon{}, leftMouse{}, seated{}, exitGrip{}, driving{}, vehicleFire{};
    bool dialogue{}, interaction{}, defaultExitTime{}, armed{}, firearm{}, melee{};
    bool menu{}, computer{}, scanner{}, scannerPaging{}, leftScanStick{true};
    bool fists{}, katana{}, leftSword{}, secondGrip{}, dualKatana{}, mirroredButtons{};
    MenuContext context{};
    unsigned foot{};
};

inline std::string ControlHint(std::string_view action, bool hold, const HintLayout& s) {
    auto button = [&](HintSource source, const char* label) -> std::string {
        return (s.sources & source) ? label : "";
    };
    auto either = [&](HintSource left, HintSource right, const char* both, const char* l, const char* r) {
        if ((s.sources & left) && (s.sources & right)) return std::string(both);
        return s.sources & left ? button(left, l) : button(right, r);
    };
    auto trigger = [&](bool left) { return left ? button(LeftTrigger,"Left Trigger") : button(RightTrigger,"Right Trigger"); };
    auto grip = [&](bool left) { return left ? button(LeftGrip,"Left Grip") : button(RightGrip,"Right Grip"); };
    auto back = [&] { return either(LeftMenu,RightMenu,"Either Menu","Left Menu","Right Menu"); };
    if (s.context == MenuContext::ShardReader && (action == "cancel" || action == "close_popup"))
        return grip(false);
    if (s.menu) {
        if (action == "back" || action == "cancel" || action == "CloseMenuSingleKey" ||
            action == "UI_Cancel" || action == "UI_Exit" || action == "UI_smart_frame_close") return back();
        if (action == "next_menu") return button(RightStick,"Right Stick Right");
        if (action == "prior_menu") return button(RightStick,"Right Stick Left");
        const bool map = s.context == MenuContext::Map, journal = s.context == MenuContext::Journal;
        if (map) {
            if (action == "world_map_menu_track_waypoint") return grip(s.leftWeapon);
            if (action == "world_map_menu_jump_to_player") return trigger(!s.leftWeapon);
            if (action == "world_map_menu_open_quest" || action == "world_map_menu_open_quest_static") return grip(!s.leftWeapon);
            if (action == "world_map_menu_zoom_in" || action == "world_map_menu_zoom_out") {
                const bool up = action == "world_map_menu_zoom_in";
                return s.leftWeapon ? button(LeftStick,up ? "Left Stick Up" : "Left Stick Down")
                                    : button(RightStick,up ? "Right Stick Up" : "Right Stick Down");
            }
        }
        if (journal && action == "sort_list") return trigger(!s.leftWeapon);
        if (!map) {
            if (action == "navigate_up" || action == "UI_MoveUp") return button(LeftStick,"Left Stick Up");
            if (action == "navigate_down" || action == "UI_MoveDown") return button(LeftStick,"Left Stick Down");
            if (action == "navigate_left" || action == "UI_MoveLeft") return button(LeftStick,"Left Stick Left");
            if (action == "navigate_right" || action == "UI_MoveRight") return button(LeftStick,"Left Stick Right");
        }
        if (s.context == MenuContext::General && (action == "right_stick_y_scroll" || action == "UI_ScrollVertically"))
            return button(RightStick,"Right Stick Up/Down");
        const bool frame = s.context == MenuContext::PictureFrame;
        if (action == "click" || action == "equip_item" || action == "proceed" || (frame && action == "UI_smart_frame_confirm"))
            return map || journal ? trigger(s.leftWeapon) : either(LeftTrigger,RightTrigger,"Either Trigger","Left Trigger","Right Trigger");
        if ((!map && !frame && (action == "secondaryAction" || action == "preview_item" || action == "UI_Apply")) || (frame && action == "UI_smart_frame_remove"))
            return either(LeftGrip,RightGrip,"Either Grip","Left Grip","Right Grip");
        return {};
    }
    if (s.computer) {
        if (action == "click" || action == "click_devices") return trigger(s.leftMouse);
        if (action == "StopDeviceControl") return back();
        if (action == "UI_ScrollVertically") return s.leftMouse ? button(LeftStick,"Left Stick Up/Down") : button(RightStick,"Right Stick Up/Down");
        return {};
    }
    if (s.context == MenuContext::SurveillanceCamera && action == "StopDeviceControl")
        return button(RightMenu, "Right Menu");
    if (s.scanner) {
        if (action == "VisionHold" || action == "VisionToggle") return button(LeftGrip,"Left Grip");
        if (action == "UI_ApplyAndClose") return trigger(s.leftMouse);
        if (s.scannerPaging && (action == "UI_MoveUp" || action == "UI_MoveDown")) {
            const bool up = action == "UI_MoveUp";
            return s.leftScanStick ? button(LeftStick,up ? "Left Stick Up" : "Left Stick Down")
                                   : button(RightStick,up ? "Right Stick Up" : "Right Stick Down");
        }
        return {};
    }
    if (action == "ChoiceApply" && s.dialogue) {
        auto confirm = either(LeftTrigger,RightTrigger,"Either Trigger","Left Trigger","Right Trigger");
        if (s.driving && (s.sources & RightStickClick))
            return confirm.empty() ? "Right Stick Click" : confirm + " or Right Stick Click";
        return confirm;
    }
    if ((action == "ChoiceScroll" || action == "ChoiceScrollAxis") && s.dialogue)
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
    if ((s.driving || (s.foot & Walking)) && (s.sources & LeftStickClick) && (s.sources & RightStick)) {
        if (action == "CycleLights" && s.driving) return "Hold Left Stick Click + Right Stick Left";
        if (action == "PocketRadio" && !s.seated) return "Hold Left Stick Click + Right Stick Left";
        if (action == "CallVehicle" && !s.seated) return "Hold Left Stick Click + Right Stick Right";
        if (action == "PhoneInteract") return "Hold Left Stick Click + Right Stick Down";
    }
    if (s.driving) {
        if (action == "Handbrake") return button(RightStickClick, "Right Stick Click");
        if (action == "Accelerate") return button(RightStick, "Right Stick Forward");
        if (action == "Decelerate") return button(RightStick, "Right Stick Back");
        if (action == "ToggleVehCamera") return button(RightGrip, "Right Grip");
        if ((s.armed && (action == "CameraAim" || action == "UI_FakeCameraAim")) || (!s.armed && action == "VehicleHorn"))
            return s.leftWeapon ? button(RightTrigger, "Right Trigger") : button(LeftTrigger, "Left Trigger");
    }
    if (s.exitGrip && (action == "Exit" || action == "QuickExit" || action == "ExitCombatMode"))
        return button(LeftGrip, s.defaultExitTime ? "Hold Left Grip 0.4 s" : "Hold Left Grip");
    if ((s.driving && action == "EnterCombatMode") || action == "SwitchItem")
        return button(LeftMenu, "Tap Left Menu");
    if (action == "WeaponWheel") return button(LeftMenu, "Hold Left Menu");
    if (action == "Reload" && (s.firearm || (s.seated && s.armed)) && (!s.seated || !s.exitGrip || s.driving))
        return s.leftWeapon ? button(RightBlack, "Tap Right Black Wrist") : button(LeftBlack, "Tap Left Black Wrist");
    if ((!s.seated || s.driving) && (action == "UseConsumable" || action == "UseConsumable_Tutorial"))
        return button(RightBlackHold, "Hold Right Black Wrist");
    if (!s.seated && s.context != MenuContext::SurveillanceCamera && action == "IconicCyberware")
        return button(RightMenu, "Hold Right Menu");
    if (s.vehicleFire && (action == "UI_FakeRangedAttack" || action == "RangedAttack"))
        return s.leftWeapon ? button(LeftTrigger, "Left Trigger") : button(RightTrigger, "Right Trigger");
    if (!s.seated && (s.foot & Walking)) {
        const bool right = (s.foot & MoveRight) != 0;
        if (action == "Jump" && !s.mirroredButtons) return button(RightStickClick,"Right Stick Click");
        if (action == "Sprint" || action == "ToggleSprint") {
            if (s.foot & ClickSprint) return button(LeftStickClick,"Left Stick Click");
            return right ? button(RightStick,"Hold Right Stick Forward") : button(LeftStick,"Hold Left Stick Forward");
        }
        if (action == "Crouch" || action == "ToggleCrouch") return right ? button(LeftStick,"Left Stick Back") : button(RightStick,"Right Stick Back");
        if (action == "Dodge" && (s.foot & DashEnabled)) return right ? button(LeftStick,"Left Stick Forward") : button(RightStick,"Right Stick Forward");
    }
    if (!s.seated && s.firearm && !s.interaction && (s.foot & DualGuns)) {
        if (action == "RangedAttack") return either(LeftTrigger,RightTrigger,"Either Trigger","Left Trigger","Right Trigger");
        if (action == "CameraAim") return {};
    }
    if (!s.seated && !s.interaction && s.fists && (action=="MeleeBlock" || action=="MeleeBlock_Tutorial"))
        return "Raise hands + hold both Triggers";
    if (!s.seated && !s.interaction && s.katana && action == "HolsterWeapon") {
        if (s.dualKatana) return "Grip on the sword to put away";
        return grip(s.leftSword);
    }
    if (!s.seated && s.melee && !s.interaction && (action == "MeleeAttack" || action == "MeleeAttack_Tutorial")) {
        if (s.dualKatana) return "Swing either katana";
        if (s.katana && s.secondGrip) return s.leftSword ? "Fast swing / Right Grip at hilt for strong" : "Fast swing / Left Grip at hilt for strong";
        return "Swing tracked hand";
    }
    if (!s.seated && s.firearm && action == "RangedAttack" && !s.interaction)
        return s.leftWeapon ? button(LeftTrigger, "Left Trigger") : button(RightTrigger, "Right Trigger");
    if (!s.seated && s.firearm && !s.interaction && action == "CameraAim")
        return s.leftWeapon ? button(RightTrigger, "Right Trigger") : button(LeftTrigger, "Left Trigger");
    return {};
}
}
