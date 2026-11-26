#pragma once
#include "Anim/VehiclePosePolicy.hpp"
#include <cstdint>
#if CVR_RUST_ROOM
#include "Rust/CvrRust.h"
#endif

namespace cvr::input {
struct VehicleButtons {
    uint16_t owned{};
    bool exitOnX{},confirmOnA{};
};
#if CVR_RUST_ROOM
// rust/crates/cvr-body/src/policy.rs; VehicleExitHold keeps its fields (Rust has its layout).
static_assert(sizeof(VehicleButtons) == 4);
inline VehicleButtons VehicleButtonPolicy(bool mounted,int state) {
    VehicleButtons out;
    cvr_vehicle_button_policy(mounted,state,&out);
    return out;
}
class VehicleExitHold {
public:
    bool Update(bool allowed,bool xHeld,uint64_t now,uint64_t holdMs) {
        return cvr_vehicle_exit_hold_update(this,allowed,xHeld,now,holdMs);
    }
private:
    uint64_t m_since{};
    bool m_down{};
};
static_assert(sizeof(VehicleExitHold) == 16);
#else
inline VehicleButtons VehicleButtonPolicy(bool mounted,int state) {
    // Window combat keeps vanilla X reload. B remains protected from an
    // accidental exit; the ordinary seat's X hold and A mirror are suspended.
    if(cvr::anim::IsPassengerWindowCombat(state))return {0x2000,false,false};
    return mounted ? VehicleButtons{0x6000,true,true} : VehicleButtons{};
}
class VehicleExitHold {
public:
    bool Update(bool allowed,bool xHeld,uint64_t now,uint64_t holdMs) {
        if(!allowed || !xHeld) { m_down=false;return false; }
        if(!m_down) { m_down=true;m_since=now; }
        return now>=m_since && now-m_since>=holdMs;
    }
private:
    uint64_t m_since{};
    bool m_down{};
};
#endif
}
