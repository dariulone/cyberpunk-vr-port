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
class TrackedWheelSteering {
public:
    void Reset() { *this={}; }
    float Angle() const { return m_wheel.Angle(); }
    float Update(uint8_t held,const WheelTrackingFrame& frame,uint64_t now,
                 float radius,float limit,float dead,float predictionMs=0,
                 float rimRight=std::numeric_limits<float>::quiet_NaN(),float rimLeft=std::numeric_limits<float>::quiet_NaN()) {
        held&=frame.validHands;
        const auto& q=frame.head.orientation;
        const float norm=q.x*q.x+q.y*q.y+q.z*q.z+q.w*q.w;
        if(!held || !frame.headValid || !frame.origin || !frame.sequence || !frame.stampUs ||
           now<frame.stampUs || now-frame.stampUs>250000 || !Finite(frame.head.position) ||
           !std::isfinite(norm) || norm<.5f || norm>1.5f) {Reset();return 0;}
        if(m_origin!=frame.origin || (m_sequence!=frame.sequence && m_stamp && frame.stampUs<=m_stamp))Reset();
        m_origin=frame.origin;
        const float horizon=WheelPrediction::ClampMs(predictionMs);
        if(m_held!=held || horizon!=m_horizon){m_prediction.Reset();m_horizon=horizon;}
        if(m_sequence==frame.sequence && m_held==held)return Output(now,limit,dead);
        const float scale=1/std::sqrt(norm);
        const XrQuaternionf rotation{q.x*scale,q.y*scale,q.z*scale,q.w*scale};
        WheelPoint points[2]{};
        for(int i=0;i<2;++i)if(held&(1<<i)) {
            const auto local=frame.hands[i];
            if(!Finite(local) || local.x*local.x+local.y*local.y+local.z*local.z>4) {Reset();return 0;}
            const auto world=RotateVector(rotation,local);
            // Physical recenter-space right/up, independent of the character,
            // game camera, car animation and head-relative controller offsets.
            points[i]={frame.head.position.x+world.x,frame.head.position.y+world.y};
        }
        m_sequence=frame.sequence;m_stamp=frame.stampUs;m_held=held;
        const float direct=m_wheel.Update(held,points[0],points[1],radius,limit,dead,rimRight,rimLeft);
        if(!m_horizon)return direct;
        m_prediction.Push(Angle(),frame.stampUs);
        return Output(now,limit,dead);
    }
private:
    static bool Finite(XrVector3f p) {return std::isfinite(p.x)&&std::isfinite(p.y)&&std::isfinite(p.z);}
    WheelSteering m_wheel;
    WheelPrediction m_prediction;
    float m_horizon{};
    float Output(uint64_t now,float limit,float dead) const {
        return WheelSteering::Output(m_prediction.Angle(Angle(),now,m_horizon,limit,dead),limit,dead);
    }
    uint64_t m_sequence{},m_origin{},m_stamp{};
    uint8_t m_held{};
};
}
