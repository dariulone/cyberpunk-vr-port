#pragma once
#include "Anim/WheelSteering.hpp"
#include "Anim/WheelPrediction.hpp"
#include "Utils/XrMath.hpp"

namespace cvr::anim {
struct WheelTrackingFrame {
    XrPosef head{{0,0,0,1},{}};
    XrVector3f hands[2]{}; // head-relative, right then left
    uint64_t sequence{},origin{},stampUs{};
    uint8_t validHands{};
    bool headValid{};
};
// THE WHEEL THE HANDS HOLD, through gaps. `held` is the hands that OWN the wheel (their grip closed on it); a grab ends
// only when that ownership ends (held 0), the caller resets on a new grab, and a recenter starts a new tracking space.
// Tracking that goes stale, a hand that drops out of tracking, a publication that pauses (a game freeze stops all of
// them at once) only SUSPEND it: the turn so far and the hands' last vector are kept, the last output is held a moment,
// and when tracking comes back the turn the hands made meanwhile is added. Reset there instead, the wheel came back
// centred under hands still turned -- "after a freeze the wheel's position is off" (players, 2026-11-05).
class TrackedWheelSteering {
public:
    static constexpr uint64_t kStaleUs=250000;   // tracking older than this is not used
    static constexpr uint64_t kHoldUs=2000000;   // the last output is held this long through a suspension, then 0
    void Reset() { *this={}; }
    float Angle() const { return m_wheel.Angle(); }
    // No usable sample now, the grab still owned: keep everything, hold the last output a moment.
    float Suspend(uint64_t now) {
        m_prediction.Reset();
        return m_fresh && now>=m_fresh && now-m_fresh<=kHoldUs ? m_last:0;
    }
    float Update(uint8_t held,const WheelTrackingFrame& frame,uint64_t now,
                 float radius,float limit,float dead,float predictionMs=0) {
        held&=3;
        if(!held) {Reset();return 0;}
        const uint8_t tracked=held&frame.validHands;
        const auto& q=frame.head.orientation;
        const float norm=q.x*q.x+q.y*q.y+q.z*q.z+q.w*q.w;
        if(!tracked || !frame.headValid || !frame.origin || !frame.sequence || !frame.stampUs ||
           now<frame.stampUs || now-frame.stampUs>kStaleUs || !Finite(frame.head.position) ||
           !std::isfinite(norm) || norm<.5f || norm>1.5f)return Suspend(now);
        if(m_origin && m_origin!=frame.origin)Reset();
        else if(m_sequence!=frame.sequence && m_stamp && frame.stampUs<=m_stamp)return Suspend(now);
        m_origin=frame.origin;
        const float horizon=WheelPrediction::ClampMs(predictionMs);
        if(m_held!=tracked || horizon!=m_horizon){m_prediction.Reset();m_horizon=horizon;}
        if(m_sequence==frame.sequence && m_held==tracked)return Keep(now,Output(now,limit,dead));
        const float scale=1/std::sqrt(norm);
        const XrQuaternionf rotation{q.x*scale,q.y*scale,q.z*scale,q.w*scale};
        WheelPoint points[2]{};
        for(int i=0;i<2;++i)if(tracked&(1<<i)) {
            const auto local=frame.hands[i];
            if(!Finite(local) || local.x*local.x+local.y*local.y+local.z*local.z>4)return Suspend(now);
            const auto world=RotateVector(rotation,local);
            // Physical recenter-space right/up, independent of the character,
            // game camera, car animation and head-relative controller offsets.
            points[i]={frame.head.position.x+world.x,frame.head.position.y+world.y};
        }
        m_sequence=frame.sequence;m_stamp=frame.stampUs;m_held=tracked;
        const float direct=m_wheel.Update(tracked,points[0],points[1],radius,limit,dead);
        if(!m_horizon)return Keep(now,direct);
        m_prediction.Push(Angle(),frame.stampUs);
        return Keep(now,Output(now,limit,dead));
    }
private:
    static bool Finite(XrVector3f p) {return std::isfinite(p.x)&&std::isfinite(p.y)&&std::isfinite(p.z);}
    float Keep(uint64_t now,float out) {m_fresh=now;m_last=out;return out;}
    WheelSteering m_wheel;
    WheelPrediction m_prediction;
    float m_horizon{},m_last{};
    float Output(uint64_t now,float limit,float dead) const {
        return WheelSteering::Output(m_prediction.Angle(Angle(),now,m_horizon,limit,dead),limit,dead);
    }
    uint64_t m_sequence{},m_origin{},m_stamp{},m_fresh{};
    uint8_t m_held{};
};
}
