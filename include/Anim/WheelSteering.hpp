#pragma once
#include "Anim/WheelRange.hpp"
#include <algorithm>
#include <cmath>
#include <cstdint>
#include <limits>

namespace cvr::anim {
struct WheelPoint { float x{},y{}; };

// Calibrate at each grab/hand change, then integrate signed angle changes.
// One hand rotates around a virtual hub relative to its actual grab position;
// controller/animation offsets cannot put it below an unrelated neutral point.
// Two hands use their connecting line, so common translation does not steer.
class WheelSteering {
public:
    void Reset() { *this={}; }
    float Angle() const { return m_angle; }
    // rimRight/rimLeft: where on the rim a hand has hold, in radians from 3 o'clock anticlockwise, or a NaN for the
    // old assumption, a hand where the driving animation has it: 3 and 9 o'clock, turned with the wheel. With the hand
    // at the top, a hub assumed beside it read a sideways pull as hardly any turn.
    float Update(uint8_t held,WheelPoint right,WheelPoint left,float radius,
                 float maxDegrees,float deadDegrees,float rimRight=std::numeric_limits<float>::quiet_NaN(),
                 float rimLeft=std::numeric_limits<float>::quiet_NaN()) {
        held&=3;
        if(!held || ((held&1) && !Finite(right)) || ((held&2) && !Finite(left))) {
            Reset();return 0;
        }
        radius=std::isfinite(radius) ? std::clamp(radius,.08f,.45f):.19f;
        const float limit=WheelFullLockDegrees(maxDegrees);
        const bool rebase=held!=m_held || !m_valid;
        if(rebase) {
            m_held=held;
            if(held!=3) {
                const auto hand=held==1 ? right:left;
                const float rim=held==1 ? rimRight:rimLeft;
                const float angle=std::isfinite(rim) ? rim:(held==1 ? 0.f:kPi)-m_angle/kDegrees;
                m_pivot={hand.x-radius*std::cos(angle),hand.y-radius*std::sin(angle)};
            }
        }
        const WheelPoint v=held==3 ? Sub(right,left):Sub(held==1 ? right:left,m_pivot);
        const float length=Length(v);
        // Never let a collapsed span or a hand at the hub select an arbitrary
        // heading. Resume from a new reference rather than jumping across it.
        const float minimum=held==3 ? .08f:.04f;
        if(length<minimum || !std::isfinite(length)) {
            m_valid=false;return Output(m_angle,limit,deadDegrees);
        }
        if(!rebase && Length(Sub(v,m_previous))<=.5f) {
            const float cross=m_previous.x*v.y-m_previous.y*v.x;
            const float dot=m_previous.x*v.x+m_previous.y*v.y;
            const float delta=-std::atan2(cross,dot)*kDegrees;
            // Preserve the physical grab reference past full lock. Discarding
            // excess travel shifts neutral and counter-steers on the way back.
            // Only the output saturates; returning the hand returns to zero.
            m_angle+=delta;
        }
        m_previous=v;m_valid=true;
        return Output(m_angle,limit,deadDegrees);
    }
    static float Output(float angle,float maxDegrees,float deadDegrees) {
        const float limit=WheelFullLockDegrees(maxDegrees);
        float dead=std::isfinite(deadDegrees) && deadDegrees>=0 && deadDegrees<=20 ? deadDegrees:1.5f;
        dead=std::min(dead,limit-5);
        const float n=std::clamp((std::abs(angle)-dead)/(limit-dead),0.f,1.f);
        if(n<=0)return 0;
        return std::copysign(n,angle);
    }
private:
    static constexpr float kDegrees=57.295779513f;
    static constexpr float kPi=3.14159265f;
    static bool Finite(WheelPoint p) { return std::isfinite(p.x)&&std::isfinite(p.y); }
    static WheelPoint Sub(WheelPoint a,WheelPoint b) { return {a.x-b.x,a.y-b.y}; }
    static float Length(WheelPoint p) { return std::hypot(p.x,p.y); }
    uint8_t m_held{};
    bool m_valid{};
    float m_angle{};
    WheelPoint m_pivot{},m_previous{};
};

// Encode the desired post-deadzone axis without changing the player's settings.
// Vehicle TurnX uses the game's inner/outer stick range, not the VR stick range.
inline float WheelGamepadAxis(float value,float inner,float outer) {
    if(!std::isfinite(value))return 0;
    if(!std::isfinite(inner)||!std::isfinite(outer)||inner<0||inner>=.5f||outer>1||outer<=inner+.01f) {
        inner=.35f;outer=.9f;
    }
    const float magnitude=std::clamp(std::abs(value),0.f,1.f);
    return magnitude>0 ? std::copysign(inner+(outer-inner)*magnitude,value):0;
}
}
