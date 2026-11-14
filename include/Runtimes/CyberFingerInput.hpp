#pragma once
#include <openxr/openxr.h>
#include "Hooks/Handedness.hpp"
#include <cstdint>

namespace cvr::cyberfinger {
// Separate opt-in file so saving the ordinary VR menu cannot discard this profile.
void Configure();
bool Enabled();

// SteamVR's binding editor can assign CyberFinger's extra buttons to these actions.
// They deliberately have no Index default: that profile has no equivalent buttons.
struct Actions {
    XrAction inventory{}, weapon{}, scanner{}, heal{};
    template<class Make> void Create(Make&& make) {
        make(inventory, XR_ACTION_TYPE_BOOLEAN_INPUT, "cf_inventory", "CyberFinger Inventory", false);
        make(weapon, XR_ACTION_TYPE_BOOLEAN_INPUT, "cf_weapon", "CyberFinger Weapon Wheel", false);
        make(scanner, XR_ACTION_TYPE_BOOLEAN_INPUT, "cf_scanner", "CyberFinger Scanner", false);
        make(heal, XR_ACTION_TYPE_BOOLEAN_INPUT, "cf_heal", "CyberFinger Quick Heal", false);
    }
    template<class Read> uint16_t Buttons(Read&& read) const {
        uint16_t bits = 0;
        if (inventory && read(inventory)) bits |= 0x0020;
        if (weapon && read(weapon)) bits |= 0x8000;
        if (scanner && read(scanner)) bits |= 0x0100;
        if (heal && read(heal)) bits |= 0x0001;
        return bits;
    }
};

// Named game actions retain their meaning when physical controls are mirrored.
// Merge them after handedness, using the same setting as the physical inputs.
// In particular, Weapon Wheel (Y) must never turn into magazine eject (B).
template<class State> State ForGame(State state, bool leftHanded, int bindings) {
    if (leftHanded) state = cvr::input::Mirrored(state, bindings);
    state.buttons |= state.cyberFingerButtons;
    return state;
}
}
