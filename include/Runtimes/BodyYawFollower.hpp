#pragma once
#include "Runtimes/BodyFollowZone.hpp"
#include <cstdint>

namespace cvr::roomscale {
// The offset belongs to an active body follower, never to a suspended scene.
// A menu only stops Step; it does not change ownership of the on-foot heading.
class BodyYawFollower {
public:
    void SetEnabled(bool enabled) {
        m_enabled=enabled;
        if(!enabled)Reset();
    }
    bool Enabled() const { return m_enabled; }
    float Offset() const { return m_offset; }
    void Reset() { m_offset=0;m_stamp=0;m_zone.Reset(); }
    // Feedback comes from the native adapter's committed quaternion. A missed
    // request must not become a fictitious applied turn in the next solve.
    void Observe(float offset, uint64_t origin) {
        if (!std::isfinite(offset)) return;
        if (origin != m_origin) { m_origin=origin; Reset(); }
        m_offset=std::remainder(offset,6.28318530718f);
    }
    float Step(float headYaw,float cone,uint64_t stamp,uint64_t origin) {
        if(!m_enabled)return 0;
        if(origin!=m_origin) { m_origin=origin;Reset();return 0; }
        if(!std::isfinite(headYaw) || !std::isfinite(cone))return 0;
        if(m_stamp && stamp<=m_stamp)return 0;
        const float dt=m_stamp ? float(stamp-m_stamp)*.000001f : .011111111f;
        if(m_stamp && stamp-m_stamp>250000)m_zone.Reset();
        m_stamp=stamp;
        const float residual=std::remainder(headYaw-m_offset,6.28318530718f);
        const float step=m_zone.Step(residual,std::max(cone,0.0f),dt);
        m_offset=std::remainder(m_offset+step,6.28318530718f);
        return step;
    }
private:
    BodyFollowZone m_zone;
    uint64_t m_stamp{},m_origin{};
    float m_offset{};
    bool m_enabled{};
};
}
