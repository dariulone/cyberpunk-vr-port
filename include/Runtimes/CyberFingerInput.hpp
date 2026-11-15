#pragma once
#include <openxr/openxr.h>
#include "Hooks/Handedness.hpp"
#include <cstdint>
#include <cmath>

namespace cvr::cyberfinger {
constexpr uint16_t SettingsButton=0x0400; // private action; never forwarded to XInput
// Physical black-wrist controls follow the weapon hand independently of the
// optional face-button/stick mirror. Merge the result as named game actions,
// after handedness: weapon hand ejects/pauses; other hand reloads/heals.
constexpr uint16_t WristButtons(bool leftHanded,bool leftTap,bool rightTap,bool leftHold,bool rightHold) {
    const bool weaponTap=leftHanded?leftTap:rightTap, weaponHold=leftHanded?leftHold:rightHold;
    const bool otherTap=leftHanded?rightTap:leftTap, otherHold=leftHanded?rightHold:leftHold;
    return (weaponHold?0x0010:weaponTap?0x2000:0) | (otherHold?0x0001:otherTap?0x4000:0);
}
// Separate opt-in file so saving the ordinary VR menu cannot discard this profile.
void Configure();
bool Configured();
// Named CyberFinger actions must be bound to an active controller. Config alone
// must not change another controller's menu or gesture behavior.
void NoteBindingActivity(bool active);
bool Enabled();
inline float SanitizeStickActionThreshold(float value) {
    return std::isfinite(value) && value>=.5f && value<=.95f?value:.8f;
}
inline bool StickActionFullTravel(float raw,bool negative,float threshold) {
    return std::isfinite(raw) && (negative?-raw:raw)>=SanitizeStickActionThreshold(threshold);
}
float StickActionThreshold();

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
