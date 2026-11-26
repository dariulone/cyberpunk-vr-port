#pragma once
#include <cmath>
#if CVR_RUST_INPUT
#include <cstring>
#include "Rust/CvrRust.h"
#endif

namespace cvr::input {
// The C++ reference for rust/crates/cvr-input's trigger module (see AnalogStick.hpp).
namespace cpp {
// Hysteresis is shared by semi-auto, full-auto and charge weapons. A held
// value remains held; it is never converted into a repeating pulse train.
struct WeaponTrigger {
    static constexpr float Break=.85f,Reset=.20f;
    bool active{},down{},ready{};
    bool Update(bool enabled,float value) {
        if(!enabled || !std::isfinite(value)) {active=down=ready=false;return false;}
        if(!active) {active=true;down=false;ready=value<=Reset;}
        if(value<=Reset) {down=false;ready=true;}
        else if(ready && value>=Break) {down=true;ready=false;}
        return down;
    }
};
}

#if CVR_RUST_INPUT
struct WeaponTrigger {
    static constexpr float Break=.85f,Reset=.20f;
    bool active{},down{},ready{};
    bool Update(bool enabled,float value) {
        CvrWeaponTrigger state;
        std::memcpy(&state,this,sizeof state);
        const bool down_=cvr_weapon_trigger_update(&state,enabled,value);
        std::memcpy(this,&state,sizeof state);
        return down_;
    }
};
static_assert(sizeof(WeaponTrigger)==sizeof(CvrWeaponTrigger));
#else
using cpp::WeaponTrigger;
#endif
bool WeaponTriggerEnabled();
}
