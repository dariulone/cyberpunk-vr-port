#pragma once
#include "Anim/VehiclePosePolicy.hpp"
#include <cstdint>

namespace cvr::input {
struct VehicleButtons {
    uint16_t owned{};
    bool exitOnX{},confirmOnA{};
};
inline VehicleButtons VehicleButtonPolicy(bool mounted,int state) {
    // Window combat keeps vanilla X reload. B remains protected from an
    // accidental exit; the ordinary seat's X hold and A mirror are suspended.
    if(cvr::anim::IsPassengerWindowCombat(state))return {0x2000,false,false};
    return mounted ? VehicleButtons{0x6000,true,true} : VehicleButtons{};
}
class VehicleExitHold {
public:
    bool Update(bool allowed,bool held,uint64_t now,uint64_t holdMs) {
        if(!allowed || !held) { m_down=false;return false; }
        if(!m_down) { m_down=true;m_since=now; }
        return now>=m_since && now-m_since>=holdMs;
    }
    // The same Grip controls the scanner and menus. Arm only after a release
    // in the seat, including after a gap in controller polling.
    bool UpdateGrip(bool allowed,bool held,uint64_t now,uint64_t holdMs) {
        if (!allowed || (m_lastGripPoll && (now<m_lastGripPoll || now-m_lastGripPoll>250)))
            m_gripArmed=false;
        m_lastGripPoll=now;
        if (allowed && !held) m_gripArmed=true;
        return Update(allowed && m_gripArmed,held,now,holdMs);
    }
private:
    bool m_gripArmed{};
    uint64_t m_lastGripPoll{};
    uint64_t m_since{};
    bool m_down{};
};
}
