#pragma once
#include <openxr/openxr.h>
#include "Hooks/Handedness.hpp"
#include "Hooks/WeaponTrigger.hpp"
#include <cstdint>
#include <cmath>

namespace cvr::cyberfinger {
constexpr uint16_t SettingsButton=0x0400; // private action; never forwarded to XInput
// Taps follow the gun. Holds stay on their physical wrist, with priority over a same-hand tap.
constexpr uint16_t WristButtons(bool leftHanded,bool leftTap,bool rightTap,bool leftHold,bool rightHold) {
    return (leftHold ? 0x0010 : leftTap ? (leftHanded ? 0x2000 : 0x4000) : 0)
         | (rightHold ? 0x0001 : rightTap ? (leftHanded ? 0x4000 : 0x2000) : 0);
}
// Separate opt-in file so saving the ordinary VR menu cannot discard this profile.
void Configure();
bool Configured();
// Named CyberFinger actions must be bound to an active controller. Config alone
// must not change another controller's menu or gesture behavior.
void NoteBindingActivity(bool active);
bool Enabled();
// Releasing a menu, D-pad chord or dialogue with a deflected stick must not move the car.
struct DriverControls {
    bool pedalReady{}, previousArmed{}, previousLeft{};
    cvr::input::WeaponTrigger weapon, support, camera;

    struct Output { float pedal{}; uint16_t buttons{}; bool fire{}; };
    Output Update(bool active, bool armed, bool leftHanded, bool gunEnabled, bool chord,
                  float stickY, float leftTrigger, float rightTrigger, bool viewGrip) {
        if (!active) { *this = {}; return {}; }
        if (armed != previousArmed || leftHanded != previousLeft) {
            weapon = {}; support = {};
            previousArmed = armed; previousLeft = leftHanded;
        }
        Output result;
        constexpr float deadzone = .12f;
        if (chord || !std::isfinite(stickY)) pedalReady = false;
        else if (std::fabs(stickY) <= deadzone) pedalReady = true;
        else if (pedalReady) {
            const float travel = (std::fabs(stickY) - deadzone) / (1.f - deadzone);
            result.pedal = std::copysign(travel < 1.f ? travel : 1.f, stickY);
        }
        const bool primary = weapon.Update(!armed || gunEnabled, leftHanded ? leftTrigger : rightTrigger);
        const bool secondary = support.Update(true, leftHanded ? rightTrigger : leftTrigger);
        result.fire = armed && primary;
        if (!armed && primary) result.buttons |= 0x8000; // Y draws
        if (secondary) result.buttons |= armed ? 0x0100 : 0x4000; // LB aims, X sounds the horn
        if (camera.Update(true, viewGrip ? 1.f : 0.f)) result.buttons |= 0x0008; // D-pad Right
        return result;
    }
};
// Preserve a separately connected gamepad's analog input.
template<class Pad> void MergeDriverPedals(Pad& output, const Pad& gamepad, uint8_t brake, uint8_t gas) {
    output.bLeftTrigger = brake > gamepad.bLeftTrigger ? brake : gamepad.bLeftTrigger;
    output.bRightTrigger = gas > gamepad.bRightTrigger ? gas : gamepad.bRightTrigger;
}
inline float SanitizeStickActionThreshold(float value) {
    return std::isfinite(value) && value>=.5f && value<=.95f?value:.8f;
}
inline bool StickActionFullTravel(float raw,bool negative,float threshold) {
    return std::isfinite(raw) && (negative?-raw:raw)>=SanitizeStickActionThreshold(threshold);
}
float StickActionThreshold();

// Require neutral on entry so pressing the modifier while accelerating cannot send D-pad Up.
struct DpadChord {
    bool ready{};
    enum class UpAction { Unselected, Gamepad, WornGear };
    UpAction upAction{};
    bool toggleWornGear{};
    uint16_t Direction(bool held, float x, float y, float threshold, bool normalOnFoot=false) {
        toggleWornGear = false;
        if (!held || !std::isfinite(x) || !std::isfinite(y)) { *this = {}; return 0; }
        const float ax=std::fabs(x), ay=std::fabs(y);
        if (ax<=.2f && ay<=.2f) { ready=true; upAction=UpAction::Unselected; }
        if (!ready || (ax<threshold && ay<threshold)) return 0;
        if (ax>ay) return x<0 ? 0x0004 : 0x0008;
        if (y<0) return 0x0002;
        if (upAction == UpAction::Unselected) {
            upAction = normalOnFoot ? UpAction::WornGear : UpAction::Gamepad;
            toggleWornGear = normalOnFoot;
        }
        return upAction == UpAction::Gamepad ? 0x0001 : 0;
    }
};

// A held axis must cross neutral before that physical stick takes a new role.
// Clicks and the physical D-pad chord keep their usual bindings.
struct CombatSticks {
    bool swapped{}, waitLeft{}, waitRight{};
    template<class State> void Apply(State& state, bool swap) {
        if (swapped != swap) {
            swapped = swap;
            waitLeft = waitRight = true;
        }
        auto release = [](float& x, float& y, bool& wait) {
            if (!wait) return;
            wait = std::fabs(x) > .18f || std::fabs(y) > .18f;
            if (wait) x = y = 0.f;
        };
        release(state.leftThumbX, state.leftThumbY, waitLeft);
        release(state.rightThumbX, state.rightThumbY, waitRight);
        if (swap) {
            const float x = state.leftThumbX, y = state.leftThumbY;
            state.leftThumbX = state.rightThumbX; state.leftThumbY = state.rightThumbY;
            state.rightThumbX = x; state.rightThumbY = y;
        }
    }
};

// The left stick doubles as a D-pad modifier. Decide on release, and stretch a
// tap across several XR frames so a slower game poll can see the jump.
inline bool UsesLeftStickJump(bool cyberFinger, bool inVehicle, int sprintMode) {
    return cyberFinger && !inVehicle && sprintMode != 1;
}

class LeftStickJump {
    bool ready = false, pending = false;
    uint64_t until = 0;
public:
    bool Update(bool allowed, bool down, uint64_t now) {
        if (!allowed) {
            ready = pending = false;
            until = 0;
            return false;
        }
        if (down) {
            if (ready) pending = true;
            ready = false;
            until = 0;
        } else {
            if (pending) until = now + 80;
            ready = true;
            pending = false;
        }
        return now < until;
    }
};

// SteamVR's binding editor can assign CyberFinger's extra buttons to these actions.
// They deliberately have no Index default: that profile has no equivalent buttons.
struct Actions {
    XrAction inventory{}, weapon{}, scanner{}, heal{},settings{};
    template<class Make> void Create(Make&& make) {
        make(inventory, XR_ACTION_TYPE_BOOLEAN_INPUT, "cf_inventory", "CyberFinger Inventory", false);
        make(weapon, XR_ACTION_TYPE_BOOLEAN_INPUT, "cf_weapon", "CyberFinger Weapon Wheel", false);
        make(scanner, XR_ACTION_TYPE_BOOLEAN_INPUT, "cf_scanner", "CyberFinger Scanner", false);
        // Keep the action IDs so saved v2 bindings remain compatible.
        make(heal, XR_ACTION_TYPE_BOOLEAN_INPUT, "cf_heal", "CyberFinger Right Black Hold", false);
        make(settings, XR_ACTION_TYPE_BOOLEAN_INPUT, "cf_settings", "CyberFinger VR Menu", false);
    }
    template<class Read> uint16_t Buttons(Read&& read) const {
        uint16_t bits = 0;
        if (inventory && read(inventory)) bits |= 0x0020;
        if (weapon && read(weapon)) bits |= 0x8000;
        if (scanner && read(scanner)) bits |= 0x0100;
        if (heal && read(heal)) bits |= 0x0001;
        if (settings && read(settings)) bits |= SettingsButton;
        return bits;
    }
};

// Named game actions retain their meaning when physical controls are mirrored.
// Merge them after handedness, using the same setting as the physical inputs.
// In particular, Weapon Wheel (Y) must never turn into magazine eject (B).
template<class State> State ForGame(State state, bool leftHanded, int bindings) {
    if (leftHanded) state = cvr::input::Mirrored(state, bindings);
    state.buttons |= state.cyberFingerButtons & ~SettingsButton;
    return state;
}
}
