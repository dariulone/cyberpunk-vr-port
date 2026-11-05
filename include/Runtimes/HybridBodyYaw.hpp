#pragma once
#include "Runtimes/BodyFollowZone.hpp"
#include "Runtimes/TrackedBodyYaw.hpp"

namespace cvr::body {
enum class RotationMode { Off, HeadCone, Tracked, Hybrid };
// Retain the older INI switches. An explicit tracked-only request takes priority
// over the new factory default, so existing presets can still select that mode.
inline RotationMode RotationModeFromFlags(int physical,int tracked,int hybrid) {
    return tracked?RotationMode::Tracked:hybrid?RotationMode::Hybrid:
        physical?RotationMode::HeadCone:RotationMode::Off;
}
inline bool NeedsBodyTracking(RotationMode mode) {
    return mode==RotationMode::Tracked || mode==RotationMode::Hybrid;
}
struct HybridInput {
    float headYaw{},headDownDegrees{},bodyBendRadians{},bodyOffset{},coneDegrees=10;
    YawEstimate estimate{};
    bool yawDefined=true,estimateReady=false;
    uint64_t stampUs{},origin{};
};
struct HybridCommand { float targetYaw{},coneDegrees{};bool tracked=false; };

// Owns the head-cone phase and its transitions. The native follower applies the
// resulting target as body motion; the independent camera reference is untouched.
class HybridBodyYaw {
    cvr::roomscale::BodyFollowZone headZone;
    uint64_t origin{},stamp{};
    float alignment{},headBias{};
    bool tracked=false,haveSource=false,aligned=false,headReady=false;
    static float Wrap(float a){return std::remainder(a,6.28318530718f);}
public:
    static constexpr float DownEnter=10,DownExit=8,BendEnter=5,BendExit=3;
    void Reset(){*this={};}
    void Suspend(){stamp=0;aligned=headReady=false;headZone.Reset();}
    bool Tracked() const{return tracked;}
    HybridCommand Step(const HybridInput& in){
        float body=std::isfinite(in.bodyOffset)?in.bodyOffset:0;
        const float cone=std::clamp(std::isfinite(in.coneDegrees)?in.coneDegrees:10.0f,0.0f,60.0f);
        if(!in.origin || !in.stampUs || !std::isfinite(in.headDownDegrees) || !std::isfinite(in.bodyBendRadians))
            return {body,tracked?0:cone,tracked};
        if(origin && in.origin<origin)return {body,tracked?0:cone,tracked};
        if(origin && in.origin!=origin){Reset();body=0;}
        origin=in.origin;
        if(stamp && in.stampUs<=stamp)return {body,tracked?0:cone,tracked};
        const float dt=stamp?float(in.stampUs-stamp)*1e-6f:0;
        if(dt>.25f)Suspend();
        stamp=in.stampUs;

        const float bend=in.bodyBendRadians*57.295779513f;
        bool next=tracked;
        if(in.headDownDegrees>=DownEnter || bend>=BendEnter)next=true;
        else if(in.headDownDegrees<=DownExit && bend<=BendExit)next=false;
        if(!haveSource || next!=tracked){
            tracked=next;haveSource=true;aligned=headReady=false;headZone.Reset();
        }
        if(tracked){
            if(!in.estimateReady || !in.estimate.valid || !std::isfinite(in.estimate.yaw)){
                aligned=false;return {body,0,true};
            }
            // Align to the avatar at handover/recovery, not to an arbitrary
            // absolute hand-line heading. The tracker stays warm while upright.
            if(!aligned){alignment=Wrap(body-in.estimate.yaw);aligned=true;}
            return {Wrap(in.estimate.yaw+alignment),0,true};
        }

        if(!in.yawDefined || !std::isfinite(in.headYaw)){
            headReady=false;headZone.Reset();return {body,cone,false};
        }
        if(!headReady){headBias=Wrap(body-in.headYaw);headReady=true;return {body,cone,false};}
        // Return to the head cone gradually when standing up with the head
        // turned. No angle is injected just because the source changed.
        headBias*=std::exp(-std::min(dt,.1f)/.12f);
        if(std::abs(headBias)<.00001f)headBias=0;
        const float residual=Wrap(in.headYaw+headBias-body);
        float step=headZone.Step(residual,cone*.01745329252f,dt);
        const float limit=12.566371f*std::max(0.0f,dt); // body-only, 720 deg/s maximum
        step=std::clamp(step,-limit,limit);
        return {Wrap(body+step),cone,false};
    }
};
}
