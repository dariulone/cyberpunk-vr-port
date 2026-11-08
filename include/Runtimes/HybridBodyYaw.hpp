#pragma once
#include "Runtimes/BodyFollowZone.hpp"
#include "Runtimes/TrackedBodyYaw.hpp"

namespace cvr::body {
enum class RotationMode { Off, HeadCone, Tracked, Hybrid, HeadLed };
// Retain the older INI switches. An explicit tracked-only request takes priority
// over the new factory default, so existing presets can still select that mode.
// HeadLed (Runtimes/HeadLedBodyTurn.hpp, the factory default since 2026-11-07) comes before the hybrid: an ini written
// before it has hybrid on and no head-led key, which reads as on.
inline RotationMode RotationModeFromFlags(int physical,int tracked,int hybrid,int headLed=0) {
    return tracked?RotationMode::Tracked:headLed?RotationMode::HeadLed:hybrid?RotationMode::Hybrid:
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
    float loweredConeDegrees=30; // the head cone while the tracked source takes over (xr_body_free_look_down_deg)
};
struct HybridCommand { float targetYaw{},coneDegrees{};bool tracked=false;float trackedWeight{},handsYaw{},neckPull{},alignment{};bool estimate=false; };

// Owns the head-cone phase and its transitions. The native follower applies the
// resulting target as body motion; the independent camera reference is untouched.
//
// THE TWO SOURCES FADE INTO EACH OTHER (2026-11-07). Upright the body follows the head (a cone); looking down
// or bent it follows the hands (TrackedBodyYaw), so a look at the belt does not turn it. The source used to switch in
// one frame, and the hand source was aligned to wherever the body happened to be at that frame. A glance to the side
// that lowered the head on the way (a look at the hip) had the cone drag the body after the head before the head was
// low enough, and the drag was then kept as the hands' heading: the body stood 23 degrees off the torso for as long as
// the head stayed down (simulator). Now:
//   - the cue still has its hysteresis (DownEnter/DownExit, BendEnter/BendExit), but the source's share fades in over
//     FadeIn and out over FadeOut -- both sources run all the time and the body takes the blend of their steps;
//   - the hand source is aligned to the body only while the head cone is settled (and only as far as the body is the
//     head's: the share not yet the hands'): a body being dragged by a glance is not a torso heading;
//   - the head cone widens toward the lowered cone with the hands' share, as HeadCone mode does looking down;
//   - and the head never stays more than NeckLimit off the hands' heading: no neck turns that far, so a turn the hand
//     source has not taken yet (lost controllers, a gesture it refused, its own lag in a fast turn) pulls the body
//     along. The pull is a rubber band, not a new alignment: it lets go as the head comes back or the hands catch up.
//     (It was written into the alignment at first, and when the hands caught up with a fast turn they had lagged the
//     pull counted twice: turned round and back looking down, the body stood 40 degrees right of the head,
//     2026-11-07.)
//   - THE HEAD ALONE IS NO TORSO (2026-11-07).
//     The hands' estimate is relative, and the alignment that maps it onto the body was refitted whenever the head cone
//     had settled -- but upright the cone follows the head, so a look round with the torso still (the hands where they
//     were) was written into the hands' heading: looking down afterwards the body stood where the head had looked, not
//     at the hands (simulator with the hands on the body, random turns: hands at 53 degrees, body and head at -109).
//     The alignment now only trims a small bias while the head looks over the hands (within AgreeWindow), slowly; and
//     takes the body whole only when the head has stayed farther off the hands' heading than any neck turns
//     (NeckRefit, RefitAfter): then the hands' estimate missed a turn, and the head is the better torso.
//   - and a freeze (no update for 0.25 s) keeps the alignment: the hands' estimate carries the turn made during it
//     (TrackedBodyYaw), and a refit to the body left where it was before the freeze would have cut that turn out.
//   - so does a short loss of the estimate (LostRefitAfter): the hands lowered or raised to a new hold jump far enough
//     for the estimator to take a new reference (a few hundred ms without an estimate), and its yaw runs on through
//     that unchanged. The alignment was dropped there and refitted to the body when the estimate came back -- upright,
//     the body is the head's, and a head turned alone when the hands changed became the torso (simulator: rifle
//     lowered with the head 60 degrees round, then a 70-degree turn looking down; the body ended 46 degrees off).
class HybridBodyYaw {
    cvr::roomscale::BodyFollowZone headZone;
    uint64_t origin{},stamp{};
    float alignment{},weight{},neckPull{},handsHold{},beyondFor{},lostFor{};
    bool tracked=false,haveSource=false,aligned=false,haveHold=false;
    static float Wrap(float a){return std::remainder(a,6.28318530718f);}
public:
    static constexpr float DownEnter=10,DownExit=8,BendEnter=5,BendExit=3;
    static constexpr float FadeIn=.08f,FadeOut=.30f;          // s: the hands' share coming in / going out
    static constexpr float NeckLimit=1.1344640f;              // 65 degrees: the head off the body, at most
    static constexpr float NeckGain=10.0f;                    // 1/s: how fast a body past it is pulled back in
    static constexpr float NeckRelease=3.0f;                  // 1/s: how fast the pull lets go when no longer needed
    static constexpr float AgreeWindow=.26179939f;            // 15 degrees: the head looking over the hands
    static constexpr float AgreeTau=1.0f;                     // s: the bias trim's time constant there
    static constexpr float NeckRefit=1.4835299f;              // 85 degrees: past any neck turn
    static constexpr float RefitAfter=.4f;                    // s: held that far off before the hands' heading is replaced
    static constexpr float LostRefitAfter=1.5f;               // s: the estimate gone longer than this is aligned afresh
    void Reset(){*this={};}
    // keepAlignment: a freeze of our own updates (the hands' estimate runs on); otherwise the source is gone (a scene,
    // a menu) and the hands are aligned afresh when it comes back
    void Suspend(bool keepAlignment=false){stamp=0;if(!keepAlignment)aligned=false;beyondFor=0;headZone.Reset();}
    bool Tracked() const{return tracked;}
    float TrackedWeight() const{return weight;}
    HybridCommand Step(const HybridInput& in){
        float body=std::isfinite(in.bodyOffset)?in.bodyOffset:0;
        const float cone=std::clamp(std::isfinite(in.coneDegrees)?in.coneDegrees:10.0f,0.0f,60.0f);
        const float lowered=std::clamp(std::isfinite(in.loweredConeDegrees)?in.loweredConeDegrees:30.0f,cone,90.0f);
        float handsTarget=body;bool haveEstimate=false;
        auto command=[&](float target){
            const float c=cone+(lowered-cone)*weight;
            return HybridCommand{target,tracked?0:c,tracked,weight,handsTarget,neckPull,alignment,haveEstimate};
        };
        if(!in.origin || !in.stampUs || !std::isfinite(in.headDownDegrees) || !std::isfinite(in.bodyBendRadians))
            return command(body);
        if(origin && in.origin<origin)return command(body);
        if(origin && in.origin!=origin){Reset();body=0;}
        origin=in.origin;
        if(stamp && in.stampUs<=stamp)return command(body);
        const float dt=stamp?float(in.stampUs-stamp)*1e-6f:0;
        if(dt>.25f)Suspend(true);
        stamp=in.stampUs;

        const float bend=in.bodyBendRadians*57.295779513f;
        bool next=tracked;
        if(in.headDownDegrees>=DownEnter || bend>=BendEnter)next=true;
        else if(in.headDownDegrees<=DownExit && bend<=BendExit)next=false;
        if(!haveSource){tracked=next;haveSource=true;weight=next?1.0f:0.0f;headZone.Reset();}
        tracked=next;
        const float want=tracked?1.0f:0.0f;
        if(dt>0){
            weight+=(want-weight)*-std::expm1(-std::min(dt,.1f)/(want>weight?FadeIn:FadeOut));
            if(std::abs(want-weight)<.01f)weight=want;
        }

        // the hands' heading, aligned to the body while the body is the head cone's: that cone settled, the head inside
        // its normal width (not the widened one: the cone opening as the head goes down stops a drag, it does not
        // make the dragged body the torso's) -- and then only as the head stands to the hands (THE HEAD ALONE above)
        const bool estimate=in.estimateReady && in.estimate.valid && std::isfinite(in.estimate.yaw);
        haveEstimate=estimate;
        const bool head=in.yawDefined && std::isfinite(in.headYaw);
        if(estimate){
            const float fit=Wrap(body-in.estimate.yaw);
            const bool underHead=head && std::abs(Wrap(in.headYaw-body))<=cone*.01745329252f+1e-4f;
            // (the body includes the neck limit's pull: what the alignment takes of the body, the pull gives up)
            lostFor=0;
            if(!aligned){alignment=fit;aligned=true;neckPull=0;beyondFor=0;}
            else if(underHead && weight<1 && !headZone.Following()) {
                const float off=std::abs(Wrap(in.headYaw-in.estimate.yaw-alignment));
                float share=0;
                if(off<=AgreeWindow){beyondFor=0;share=dt>0 ? -std::expm1(-std::min(dt,.1f)/AgreeTau):0.0f;}
                else if(off>NeckRefit){beyondFor+=std::max(dt,0.0f);share=beyondFor>=RefitAfter ? 1.0f:0.0f;}
                else beyondFor=0;
                share*=1-weight;
                alignment=Wrap(alignment+Wrap(fit-alignment)*share);
                neckPull*=1-share;
            }else beyondFor=0;
        }else{
            beyondFor=0;
            lostFor+=std::max(dt,0.0f);
            if(lostFor>LostRefitAfter)aligned=false;
        }
        // the hands' heading; without an estimate, the last one held (the body stays where the hands left it)
        if(estimate){handsTarget=Wrap(in.estimate.yaw+alignment);handsHold=handsTarget;haveHold=true;}
        else{if(!haveHold){handsHold=body;haveHold=true;}handsTarget=handsHold;}
        if(head && dt>0){
            const float gap=Wrap(in.headYaw-handsTarget);
            const float needed=std::copysign(std::max(0.0f,std::abs(gap)-NeckLimit),gap);
            const bool grow=needed*neckPull<0 || std::abs(needed)>std::abs(neckPull);
            const float change=(needed-neckPull)*-std::expm1(-std::min(dt,.1f)*(grow?NeckGain:NeckRelease));
            // no faster than the body may turn: a pull ahead of the body only drags it on after the head is back
            const float most=12.566371f*dt;
            neckPull+=std::clamp(change,-most,most);
            if(std::abs(neckPull)<1e-5f && needed==0)neckPull=0;
        }else if(!head)neckPull=0;
        const float handStep=Wrap(handsTarget+neckPull-body);

        float coneStep=0;
        if(head && weight<1){
            coneStep=headZone.Step(Wrap(in.headYaw-body),(cone+(lowered-cone)*weight)*.01745329252f,dt);
        }else headZone.Reset();
        float step=(1-weight)*coneStep+weight*handStep;
        const float limit=12.566371f*std::max(0.0f,dt); // body-only, 720 deg/s maximum
        step=std::clamp(step,-limit,limit);
        return command(Wrap(body+step));
    }
};
}
