#pragma once
#include "Utils/XrMath.hpp"
#include "Runtimes/BodyYawPrediction.hpp"
#include <algorithm>
#include <cstdint>

namespace cvr::body {
// All three devices belong to one locate time/origin. Hands are raw HMD-local
// positions, before weapon offsets or hand interpolation. This is an estimate
// of torso yaw, not a measurement from a waist tracker.
struct TrackingFrame {
    XrQuaternionf head{0,0,0,1};
    XrVector3f headPosition{}; // recentered tracking-base coordinates
    XrVector3f hands[2]{};
    XrQuaternionf rotations[2]{{0,0,0,1},{0,0,0,1}};
    bool valid=false,tracked[2]{};
    uint64_t sequence{},stampUs{},origin{};
};
struct YawEstimate {float yaw{},confidence{};bool valid=false;};
inline bool TrackingFrameFresh(const TrackingFrame& frame,uint64_t currentOrigin,uint64_t nowUs){
    return frame.valid && frame.origin==currentOrigin && currentOrigin!=0 && frame.sequence!=0 &&
        frame.stampUs!=0 && nowUs>=frame.stampUs && nowUs-frame.stampUs<=150000;
}

class TrackedBodyYaw {
    struct Observation {XrVector3f hand[2],worldHand[2];XrQuaternionf rotation[2],headRotation;};
    Observation anchor{},filtered{},motionWitness{};
    uint64_t origin{},sequence{},stamp{},acquireStart{},rebaseUntil{};
    float anchorYaw{},target{},output{},evidence{};
    // The head's turn ahead of the torso's when the reference was last rebased mid-turn (radians, see Update).
    float headLead{};
    // The reference held when a short suspension began (a hand jump, a lost sample): its head and lead are carried into
    // the reference taken when the hands come back (Update, acquisition), as a reshape carries them.
    uint64_t keptStamp{};
    bool keptAnchor=false;
    bool haveAnchor=false,haveFilter=false,absoluteInitial=true,haveMotionWitness=false;
    bool rigidHeld=false;   // a whole-player turn seen for one sample, waiting for the next to confirm it
public:
    // The last update's gates, for the debug export (BodyYawFollow.cpp CyberpunkVR_DebugTrackedTrace).
    // bits: 0 coherent, 1 useWrists, 2 rotationsConfirm, 3 wristConsensus, 4 sameHands, 5 headSupports,
    // 6 restSupports, 7 positionMotion, 8 radial, 9 rawGeometrySupports, 10 atReference, 11 orientationsAgree,
    // 12 oneOrientationAgrees, 13 useLine, 14 wrist consensus before the witnesses, 15 witness (hand), 16 witness
    // (world), 17 motion witness, 18 head support (rotations), 19 closes on head (rotations), 20 rebased, 21 reshaped;
    // 24 early return: acquiring, 25 axis too short, 26 rigid jump, 27 suspended, 28 freeze carried
    struct Trace {float delta{},wristMean{},headDelta{},headLead{},accepted{},evidence{},residual{},target{};uint32_t bits{};};
    Trace trace{};
private:
    BodyYawPrediction prediction;
    static constexpr float pi=3.14159265359f;
    static constexpr float kHeadLeadSlack=.05f;  // rad: the torso may end this far past the head
    static float Wrap(float a){return std::remainder(a,2*pi);}
    static bool Newer(uint64_t a,uint64_t b){
        // Native hand publications use a wrapping32-bit sequence.
        if(a<=UINT32_MAX && b<=UINT32_MAX)return static_cast<int32_t>(static_cast<uint32_t>(a)-static_cast<uint32_t>(b))>0;
        return a>b;
    }
    static XrVector3f Sub(XrVector3f a,XrVector3f b){return {a.x-b.x,a.y-b.y,a.z-b.z};}
    static XrVector3f Mid(XrVector3f a,XrVector3f b){return {(a.x+b.x)*.5f,(a.y+b.y)*.5f,(a.z+b.z)*.5f};}
    static float Radius(XrVector3f a){return std::hypot(a.x,a.z);}
    static float Length(XrVector3f a){return std::sqrt(a.x*a.x+a.y*a.y+a.z*a.z);}
    static bool Finite(XrVector3f a){return std::isfinite(a.x)&&std::isfinite(a.y)&&std::isfinite(a.z);}
    static bool Normalize(XrQuaternionf& q){
        const float n=q.x*q.x+q.y*q.y+q.z*q.z+q.w*q.w;
        if(!std::isfinite(n) || n<.5f || n>1.5f)return false;
        const float inv=1/std::sqrt(n);q.x*=inv;q.y*=inv;q.z*=inv;q.w*=inv;return true;
    }
    static float RotationYaw(XrQuaternionf from,XrQuaternionf to){
        const auto q=MultiplyQuat(to,ConjugateQuat(from));return Wrap(2*std::atan2(q.y,q.w));
    }
    static float Angle(XrVector3f from,XrVector3f to){
        return std::atan2(from.z*to.x-from.x*to.z,from.x*to.x+from.z*to.z);
    }
    static XrVector3f Rotate(XrVector3f p,float a){
        const float c=std::cos(a),s=std::sin(a);return {c*p.x+s*p.z,p.y,-s*p.x+c*p.z};
    }
    static bool Decode(const TrackingFrame& frame,Observation& out){
        if(!frame.valid || !frame.tracked[0] || !frame.tracked[1])return false;
        if(!Finite(frame.headPosition) || Length(frame.headPosition)>1000)return false;
        auto q=frame.head;
        if(!Normalize(q))return false;
        out.headRotation=q;
        for(int h=0;h<2;++h){
            if(!Finite(frame.hands[h]) || Length(frame.hands[h])<.10f || Length(frame.hands[h])>1.6f)return false;
            // Approximate eye-to-neck lever. The hand line itself is independent
            // of this estimate; it prevents small head pivots from looking like
            // common hand motion when the controllers are held together.
            out.hand[h]=RotateVector(q,Sub(frame.hands[h],{0,-.10f,.08f}));
            const auto relative=RotateVector(q,frame.hands[h]);
            out.worldHand[h]={frame.headPosition.x+relative.x,frame.headPosition.y+relative.y,frame.headPosition.z+relative.z};
            auto handRotation=frame.rotations[h];if(!Normalize(handRotation))return false;
            out.rotation[h]=MultiplyQuat(q,handRotation);
        }
        return true;
    }
    // continuous: the same turn goes on from a new reference (a rebase, a whole-player jump, a freeze carried) -- the
    // head's yaw against the torso is carried to it (a carried freeze kept the old lead while the head had turned alone
    // during the freeze: 41 degrees wrong, and the next turn toward the head was refused half way)
    void Anchor(const Observation& value,bool continuous=false){
        const float lead=continuous && haveAnchor ?
            std::clamp(Wrap(headLead+RotationYaw(anchor.headRotation,value.headRotation)-Wrap(target-anchorYaw)),-1.6f,1.6f):0.0f;
        if(continuous)headLead=lead;
        anchor=value;anchorYaw=target;haveAnchor=true;
        motionWitness=value;haveMotionWitness=true;
        rebaseUntil=continuous ? stamp+50000:0;
        if(!continuous){evidence=0;headLead=0;prediction.Suspend();}
    }
    // The torso's turn between two observations far apart in time: both wrists turned by one yaw, and both hands where
    // the first observation's hands are, turned about the neck by it. False when the hands moved on their own.
    static bool TurnAcrossGap(const Observation& before,const Observation& after,float& turn){
        const float w0=RotationYaw(before.rotation[0],after.rotation[0]),w1=RotationYaw(before.rotation[1],after.rotation[1]);
        const float mean=w0+Wrap(w1-w0)*.5f;
        if(std::abs(Wrap(w0-w1))>.10f+.15f*std::abs(mean))return false;
        for(int h=0;h<2;++h)
            if(Length(Sub(after.hand[h],Rotate(before.hand[h],mean)))>.12f)return false;
        turn=mean;
        return true;
    }
    // A new reference for a new hand shape (a gesture, the hands lowered or raised): the torso has not turned, so the
    // head's yaw against it is carried over, as at a mid-turn rebase. HANDS CHANGED, THEN A TURN (2026-11-07: the hybrid
    // lost the torso when the hands were lowered and the player turned again): the lead was zeroed here, and the head had
    // been 61 degrees off the torso when the hands changed -- the next turn of the torso toward the head was taken for
    // a turn against it and refused half way (simulator, random turns: 42 of 106 degrees, the body left at the neck limit).
    void Reshape(const Observation& value){
        const float lead=std::clamp(Wrap(headLead+RotationYaw(anchor.headRotation,value.headRotation)-Wrap(target-anchorYaw)),
            -1.6f,1.6f);
        Anchor(value);headLead=lead;
    }
    static bool Rest(const Observation& p){
        return p.hand[0].y<-.35f && p.hand[1].y<-.35f &&
            std::abs(p.hand[0].y-p.hand[1].y)<.25f &&
            Radius(Sub(p.hand[1],p.hand[0]))>.28f &&
            Radius(p.hand[0])<.65f && Radius(p.hand[1])<.65f;
    }
public:
    void Reset(float seed=0,bool useAbsoluteInitial=true){*this={};absoluteInitial=useAbsoluteInitial;target=output=anchorYaw=std::isfinite(seed)?Wrap(seed):0;prediction.Reset(output);}
    void Suspend(){trace.bits|=1u<<27;if(haveAnchor){keptAnchor=true;keptStamp=stamp;}haveAnchor=haveFilter=false;rigidHeld=false;stamp=sequence=0;evidence=0;target=output;prediction.Reset(output);}
    bool Ready() const{return haveAnchor;}
    YawEstimate Predict(uint64_t nowUs) const {
        if(!haveAnchor || !stamp || nowUs<stamp || nowUs-stamp>150000)return {output,0,false};
        return {prediction.Sample(float(nowUs-stamp)*1e-6f),std::min(1.f,evidence/.045f),true};
    }
    YawEstimate Update(const TrackingFrame& frame){
        if(origin && frame.origin<origin)return {output,0,false};
        if(origin && frame.origin!=origin){Reset(0,absoluteInitial);}
        origin=frame.origin;
        Observation current{};
        if(!frame.origin || !frame.stampUs || !frame.sequence || !Decode(frame,current)){
            Suspend();return {output,0,false};
        }
        const Observation unfiltered=current;
        if(stamp && (frame.stampUs<=stamp || !Newer(frame.sequence,sequence)))return {output,0,true};
        const float dt=stamp?float(frame.stampUs-stamp)*1e-6f:0;
        if(dt>.20f){
            // A FREEZE (2026-11-07). No sample for longer than a frame ever
            // takes: the game hitched. Suspended, the reference was taken afresh where the player stood after it and any
            // turn made during the hitch was lost -- the body stayed behind the hands until the neck limit dragged it.
            // Both wrists turned by one yaw and both hands moved round the neck with them is the torso turned: carried.
            float turn=0;
            if(haveAnchor && haveFilter && TurnAcrossGap(filtered,current,turn)){
                trace.bits=1u<<28;
                target=output=Wrap(target+turn);
                prediction.Reset(output);
                stamp=frame.stampUs;sequence=frame.sequence;
                rigidHeld=false;filtered=current;Anchor(current,true);
                return {output,1,true};
            }
            Suspend();
        }
        stamp=frame.stampUs;sequence=frame.sequence;
        if(!haveFilter){filtered=current;haveFilter=true;acquireStart=stamp;}
        else {
            const float jumpLimit=25*std::max(dt,0.0f)+.08f;
            const float headJump=RotationYaw(filtered.headRotation,current.headRotation);
            const float wristJump[2]{RotationYaw(filtered.rotation[0],current.rotation[0]),
                                     RotationYaw(filtered.rotation[1],current.rotation[1])};
            if(std::abs(headJump)>jumpLimit && std::abs(wristJump[0])>jumpLimit && std::abs(wristJump[1])>jumpLimit){
                // THE WHOLE PLAYER TURNED AT ONCE (2026-11-07). Head and both wrists turning by the same yaw faster than
                // a person turns is the player turned as one -- the simulator's mouse (its controllers ride on the head),
                // or the tracking space turning under the player -- and the torso turned with them: carried. Suspended
                // instead, the turn was lost: the reference came back where the body had been left (dragged at the neck
                // limit), and looking down the body stayed 40-65 degrees off the head. Devices jumping apart are a tracking fault, suspended as before; and one sample
                // turned and the next back is an outlier: the first is held (the estimate unchanged, the reference and
                // the filter kept from before it) and the turn carried only when the next sample is still turned.
                const float tolerance=.10f+.15f*std::abs(headJump);
                const bool rigid=haveAnchor && std::abs(Wrap(wristJump[0]-headJump))<tolerance &&
                    std::abs(Wrap(wristJump[1]-headJump))<tolerance;
                trace.bits=1u<<26;
                if(!rigid){Suspend();return {output,0,false};}
                if(!rigidHeld){rigidHeld=true;return {output,0,true};}
                rigidHeld=false;
                target=output=Wrap(target+headJump);
                prediction.Reset(output);
                filtered=current;Anchor(current,true);
                return {output,1,true};
            }
            rigidHeld=false;
            for(int h=0;h<2;++h){
                if(Length(Sub(current.hand[h],filtered.hand[h]))>.5f){Suspend();return {output,0,false};}
                const float a=-std::expm1(-std::min(dt,.1f)/.025f);
                filtered.hand[h].x+=(current.hand[h].x-filtered.hand[h].x)*a;
                filtered.hand[h].y+=(current.hand[h].y-filtered.hand[h].y)*a;
                filtered.hand[h].z+=(current.hand[h].z-filtered.hand[h].z)*a;
                filtered.worldHand[h].x+=(current.worldHand[h].x-filtered.worldHand[h].x)*a;
                filtered.worldHand[h].y+=(current.worldHand[h].y-filtered.worldHand[h].y)*a;
                filtered.worldHand[h].z+=(current.worldHand[h].z-filtered.worldHand[h].z)*a;
            }
            const float rotAlpha=haveAnchor?1:-std::expm1(-std::min(dt,.1f)/.025f);
            filtered.headRotation=NlerpQuat(filtered.headRotation,current.headRotation,rotAlpha);
            filtered.rotation[0]=NlerpQuat(filtered.rotation[0],current.rotation[0],rotAlpha);
            filtered.rotation[1]=NlerpQuat(filtered.rotation[1],current.rotation[1],rotAlpha);
        }
        current=filtered;
        if(!haveAnchor){
            // A single noisy first sample otherwise becomes a permanent yaw
            // bias. Briefly acquire a filtered reference on startup/recovery.
            trace.bits=1u<<24;
            if(stamp-acquireStart<120000)return {output,0,true};
            // A symmetric, low-hand stance provides an absolute initial heading.
            // Otherwise retain the current body heading until coherent motion.
            const auto centre=Mid(current.hand[0],current.hand[1]);
            // Back after a short suspension (the hands jumped to a new hold, a sample went missing): the torso is taken
            // not to have turned, the estimate runs on from where it was, and the head's yaw against it is carried --
            // a lead zeroed here, with the head turned when the hands changed, had the next turn toward the head
            // refused as a turn against it (the body left at the neck limit; HybridHandsChangeThenTurn).
            const bool carry=keptAnchor && keptStamp && stamp>keptStamp && stamp-keptStamp<1500000;
            keptAnchor=false;
            if(carry){Reshape(current);return {output,0,true};}
            if(absoluteInitial && Rest(current) && Radius(centre)<.14f){
                const auto line=Sub(current.hand[1],current.hand[0]);
                const float yaw=std::atan2(-line.z,line.x);
                // Do not flip a low hand line just because the player looks past
                // ninety degrees. Crossed/ambiguous startup hands keep the seed.
                if(std::abs(Wrap(yaw-output))<=pi*.5f+.00001f)target=yaw;
            }
            Anchor(current);return {output,0,true};
        }
        const auto before=Sub(anchor.hand[1],anchor.hand[0]),after=Sub(current.hand[1],current.hand[0]);
        const bool useLine=Radius(before)>.14f && Radius(after)>.14f;
        const auto oldAxis=useLine?before:Mid(anchor.hand[0],anchor.hand[1]);
        const auto newAxis=useLine?after:Mid(current.hand[0],current.hand[1]);
        if(Radius(oldAxis)<.12f || Radius(newAxis)<.12f){trace.bits=1u<<25;Reshape(current);return {output,0,true};}
        const float delta=Angle(oldAxis,newAxis);
        float handDelta[2]{},residual=0;bool radial=true;
        for(int h=0;h<2;++h){
            radial&=Radius(anchor.hand[h])>.10f && Radius(current.hand[h])>.10f;
            handDelta[h]=Angle(anchor.hand[h],current.hand[h]);
            residual=std::max(residual,Length(Sub(current.hand[h],Rotate(anchor.hand[h],delta))));
        }
        const float magnitude=std::max(std::abs(handDelta[0]),std::abs(handDelta[1]));
        const float agreement=magnitude>.001f?std::min(std::abs(handDelta[0]),std::abs(handDelta[1]))/magnitude:1;
        const float headDelta=RotationYaw(anchor.headRotation,current.headRotation);
        // THE TORSO CATCHING UP WITH THE HEAD (2026-11-07). A person turns the head first; the torso arrives after the
        // head has stopped. Within one reference that is head support (the head turned more than the torso). After a
        // mid-turn rebase (below, every 60 degrees) the head's turn so far was dropped with the old reference: the rest
        // of the torso's turn came with a still head and was refused as a hand gesture -- the body stopped 25 degrees
        // short of a 90-degree turn and stayed there while the head was down (simulator, rifle hold, head 20-30 % ahead).
        // The head's yaw against the torso at the rebase is kept (headLead): a torso moving from where it was accepted
        // toward the head, and never past it, is supported -- in a continuing turn and in a turn back alike.
        const float accepted=Wrap(target-anchorYaw);
        auto closesOnHead=[&](float turn){
            const float toHead=Wrap(headLead+headDelta-accepted),moved=Wrap(turn-accepted);
            return moved*toHead>0 && std::abs(moved)<=std::abs(toHead)+kHeadLeadSlack;
        };
        const bool sameHands=handDelta[0]*handDelta[1]>=0 && agreement>.55f &&
            std::abs(Wrap(handDelta[0]-delta))<.008f+.30f*std::abs(delta) &&
            std::abs(Wrap(handDelta[1]-delta))<.008f+.30f*std::abs(delta);
        const bool headSupports=(headDelta*delta>=0 && std::abs(headDelta)>=.40f*std::abs(delta)) || closesOnHead(delta);
        bool orientationsAgree=true,oneOrientationAgrees=false;
        float wristDelta[2]{};
        for(int h=0;h<2;++h){
            const float angle=RotationYaw(anchor.rotation[h],current.rotation[h]);
            wristDelta[h]=angle;
            const bool agrees=angle*delta>=0 && std::abs(angle)>=.40f*std::abs(delta) &&
                std::abs(Wrap(angle-delta))<.025f+.35f*std::abs(delta);
            orientationsAgree&=agrees;oneOrientationAgrees|=agrees;
        }
        // Low arms swinging in opposite directions can mimic a rotating hand
        // line. Without head motion, require both wrist orientations to confirm
        // the turn as well; their pitch swing alone contributes no yaw.
        const bool restSupports=useLine && Rest(anchor) && Rest(current) && orientationsAgree;
        const float wristMean=wristDelta[0]+Wrap(wristDelta[1]-wristDelta[0])*.5f;
        const float wristMax=std::max(std::abs(wristDelta[0]),std::abs(wristDelta[1]));
        bool wristConsensus=wristDelta[0]*wristDelta[1]>=0 &&
            (wristMax<.001f || std::min(std::abs(wristDelta[0]),std::abs(wristDelta[1]))>.55f*wristMax) &&
            std::abs(Wrap(wristDelta[0]-wristDelta[1]))<.015f+.25f*std::abs(wristMean) &&
            ((headDelta*wristMean>=0 && std::abs(headDelta)>=.40f*std::abs(wristMean)) || closesOnHead(wristMean));
        const bool rotationsConfirm=wristConsensus && wristMax>.003f;
        const bool consensusBefore=wristConsensus;
        const bool rotHeadSupport=headDelta*wristMean>=0 && std::abs(headDelta)>=.40f*std::abs(wristMean);
        const bool rotClosesOnHead=closesOnHead(wristMean);
        bool witnessHand=true,witnessWorld=true,witnessMotion=true;
        const auto rawAxis=useLine?Sub(unfiltered.hand[1],unfiltered.hand[0]):Mid(unfiltered.hand[0],unfiltered.hand[1]);
        const bool rawGeometrySupports=std::abs(Wrap(Angle(oldAxis,rawAxis)-wristMean))<.030f;
        for(int h=0;h<2;++h){
            const float expected=Length(Sub(Rotate(anchor.hand[h],wristMean),anchor.hand[h]));
            // Immediately after a coherent reference rebase, the positional
            // low-pass starts from that raw instant. Use raw position witnesses
            // for its short warm-up; ordinary gestures retain filtered witnesses.
            // The wrist rotations refer to the current raw instant. A live
            // movement witness below permits the matching raw positions during
            // a turn, including crossing the old anchor on a reversal. Idle
            // gestures keep the low-pass witness instead.
            const auto& witness=(stamp<rebaseUntil || std::abs(prediction.Rate())>.174532925f ||
                (rawGeometrySupports && evidence>=.045f)) ? unfiltered:current;
            witnessHand&=Length(Sub(witness.hand[h],anchor.hand[h]))>=std::min(expected*.15f,.006f);
            wristConsensus&=Length(Sub(witness.hand[h],anchor.hand[h]))>=std::min(expected*.15f,.006f);
            // A pair of wrists turning under an independently moving head is
            // not a torso turn. With 3 mm device noise, a 15% / 6 mm witness
            // could occasionally accept that gesture. Require a meaningful
            // fraction of the predicted orbit in world space; the threshold
            // still tends to zero with the turn, so tiny body turns have no cone.
            witnessWorld&=Length(Sub(witness.worldHand[h],anchor.worldHand[h]))>=std::min(expected*.40f,.012f);
            wristConsensus&=Length(Sub(witness.worldHand[h],anchor.worldHand[h]))>=std::min(expected*.40f,.012f);
            if(haveMotionWitness && !rawGeometrySupports) {
                // Old displacement relative to the long-lived reference cannot
                // authorize a NEW wrist/head gesture after the body has stopped.
                const float freshYaw=RotationYaw(motionWitness.rotation[h],unfiltered.rotation[h]);
                const auto orbit=Sub(Rotate(motionWitness.hand[h],freshYaw),motionWitness.hand[h]);
                const auto travel=Sub(unfiltered.worldHand[h],motionWitness.worldHand[h]);
                const float freshOrbit=Length(orbit);
                // Require travel ALONG the predicted orbit, not just arbitrary
                // tracker noise or a radial reach of the same magnitude.
                const float along=travel.x*orbit.x+travel.y*orbit.y+travel.z*orbit.z;
                const bool activeGesture=Length(travel)>.012f;
                witnessMotion&=along>=freshOrbit*std::min(freshOrbit*.40f,.012f) || activeGesture;
                wristConsensus&=along>=freshOrbit*std::min(freshOrbit*.40f,.012f) || activeGesture;
            }
        }
        bool positionMotion=true;
        for(int h=0;h<2;++h){
            const float expected=Length(Sub(Rotate(anchor.hand[h],delta),anchor.hand[h]));
            positionMotion&=Length(Sub(current.worldHand[h],anchor.worldHand[h]))>=std::min(expected*.15f,.006f);
        }
        // Walking/reloading can change the hand triangle during a real turn.
        // Agreement of all three device rotations is an independent witness;
        // stationary hands veto wrist-only gestures even if the head follows.
        // A return to the saved pose is evidence too. Requiring fresh positional
        // movement at zero delta otherwise strands the last filtered step after
        // an out-and-back turn. Follow the measured tiny yaw (not a zero snap),
        // and require both hand positions to agree with that reference shape.
        bool atReference=wristMax<.003f;
        for(int h=0;h<2;++h)
            atReference&=Length(Sub(unfiltered.hand[h],Rotate(anchor.hand[h],wristMean)))<.012f;
        const bool useWrists=(rotationsConfirm && wristConsensus) || atReference;
        const bool coherent=useWrists || (!rotationsConfirm && radial && sameHands && residual<.035f &&
            positionMotion && oneOrientationAgrees && (useLine || orientationsAgree) && (headSupports || restSupports));
        const float turn=useWrists?wristMean:delta;
        trace={delta,wristMean,headDelta,headLead,accepted,evidence,residual,target,
            uint32_t(coherent)|uint32_t(useWrists)<<1|uint32_t(rotationsConfirm)<<2|uint32_t(wristConsensus)<<3|
            uint32_t(sameHands)<<4|uint32_t(headSupports)<<5|uint32_t(restSupports)<<6|uint32_t(positionMotion)<<7|
            uint32_t(radial)<<8|uint32_t(rawGeometrySupports)<<9|uint32_t(atReference)<<10|uint32_t(orientationsAgree)<<11|
            uint32_t(oneOrientationAgrees)<<12|uint32_t(useLine)<<13|uint32_t(consensusBefore)<<14|uint32_t(witnessHand)<<15|
            uint32_t(witnessWorld)<<16|uint32_t(witnessMotion)<<17|uint32_t(rotHeadSupport)<<18|uint32_t(rotClosesOnHead)<<19};
        if(coherent){
            evidence+=std::max(0.0f,dt);
            if(evidence>=.045f) {
                target=Wrap(anchorYaw+turn);
                motionWitness=unfiltered;haveMotionWitness=true;
            }
        }else {
            // Near the saved angle, millimetre position noise can temporarily
            // fail the displacement witness even though raw geometry and all
            // rotations still agree. Hold that sample without restarting the
            // 45ms acquisition on a confirmed continuing/reversing turn.
            // A gesture that disagrees with raw geometry loses evidence now.
            if(!(rotationsConfirm && rawGeometrySupports && evidence>=.045f))evidence=0;
            // Hand gestures change the reference shape, not the torso heading.
            // A head-only glance keeps the reference so a later torso turn can
            // follow the already-turned head without waiting for another glance.
            float rawResidual=0;
            for(int h=0;h<2;++h)
                rawResidual=std::max(rawResidual,Length(Sub(unfiltered.hand[h],Rotate(anchor.hand[h],delta))));
            const bool shapeChanged=residual>.06f && rawResidual>.06f;
            const bool spanChanged=useLine && std::abs(Radius(after)-Radius(before))>.10f &&
                std::abs(Radius(Sub(unfiltered.hand[1],unfiltered.hand[0]))-Radius(before))>.10f;
            // Filter lag on a reversal is not a new arm gesture. Both the raw
            // and filtered geometry must have left the saved shape to replace it.
            if(!rotationsConfirm && (shapeChanged || spanChanged)){Reshape(current);trace.bits|=1u<<21;}
        }
        output=prediction.Update(target,dt,coherent && evidence>=.045f);
        // Refresh only after an accepted substantial turn. Refreshing an idle
        // reference periodically integrates independent tracker noise as drift.
        if(coherent && evidence>=.045f && std::abs(turn)>1.05f){
            // Positions have a 25 ms filter but accepted wrist rotations are
            // current. Saving that mixed-time geometry produces a permanent
            // bias when the turn reverses. Reanchor all fields at one raw time.
            const float rawTurn=useWrists?turn:Angle(oldAxis,rawAxis);
            // the head's yaw against the torso, carried to the new reference
            const float lead=std::clamp(Wrap(headLead+RotationYaw(anchor.headRotation,unfiltered.headRotation)-rawTurn),-1.6f,1.6f);
            target=Wrap(anchorYaw+rawTurn);
            // Rebase both observations at the same raw instant. Keeping the old
            // positional filter here briefly puts the next sample BEHIND its
            // new anchor and vetoes the already-confirmed continuing rotation.
            filtered=unfiltered;Anchor(unfiltered,true);headLead=lead;trace.bits|=1u<<20;
        }
        return {output,coherent?std::min(1.0f,evidence/.045f):0,true};
    }
};
}
