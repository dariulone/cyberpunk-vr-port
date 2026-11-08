#pragma once
#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>

namespace cvr::body {
// THE ONE NECK (2026-11-07). The eyes over the point the head turns about, in the head's axes (OpenXR: +Y up, -Z
// ahead): 10 cm up, 9 cm ahead. Roomscale walks this neck (Runtimes/RoomscaleMovement.hpp EyeFromNeck), the body turns
// about it (Camera/AnchorTranslation.hpp) and the bend tracker below follows it. The tracker had its own, 8 cm up and
// 15 cm ahead: a head turned on the spot then moved its neck along an arc the roomscale neck did not move, the
// difference was taken for a hinge and handed to roomscale -- after a few quick turns the capsule stood 13-18 mm off
// the head and kept wandering (simulator, after a few turns). One model, as BONELAB has (one skull-to-neck offset).
inline constexpr float EyeAboveNeck=.10f,EyeAheadOfNeck=.09f;
// How far a player's own eyes may sit off that model: neck travel a turn of the head explains within it is no hinge.
inline constexpr float TurnLeverSlack=.04f;
inline constexpr float BendReach=.63f;
inline constexpr float MaxBend=1.22173048f; // 70 degrees
// Double the activation distances, retaining each previous transition width.
inline constexpr float BendSpanStart=.24f,BendSpanFull=.33f;
inline constexpr float BendDropStart=.020f,BendDropFull=.048f;
inline constexpr float FloorReachDropStart=.16f,FloorReachDropFull=.29f;
// A walking head's bob: while the body walks after the head, a drop within this is a step, not a hinge.
inline constexpr float BendWalkBobDrop=.03f;
// The height's tracking noise: a drop this close to the accepted one is no descent yet (BendTracker::Update).
inline constexpr float BendDropNoise=.002f;
// A neck that stays within BendStillTravel (across and in height) for BendSteadyUpdates is standing still: its posture
// is accepted as it stands.
inline constexpr float BendStillTravel=.003f;
inline constexpr int BendSteadyUpdates=12;
// At the calibrated height (a drop within BendDropNoise) a hinge held from noise bleeds away by this share an update.
inline constexpr float BendNoiseBleed=.02f;
inline float SmoothRange(float value,float from,float to) {
    const float t=std::clamp((value-from)/(to-from),0.0f,1.0f);
    return t*t*(3-2*t);
}
inline float BendWeight(float angle) {
    return std::isfinite(angle) ? SmoothRange(angle,.08726646f,.43633231f):0;
}
inline float HingeDrop(float angle) {
    return std::isfinite(angle) ? BendReach*(1-std::cos(std::clamp(angle,0.0f,MaxBend))):0;
}
struct BendSample {
    float right{},forward{}; // fixed recenter axes, metres
    float angle{};          // physical hinge, radians
};

// Adapted from 8ca2fb40 BodyLocomotion::Posture and NeckMotionFilter.
// Downward travel along an arc grows the hinge; translation at held height
// remains walking. Vertical crouching alone does not become a forward bend.
// Run once under the frame-sample lock and carry the result with that pose.
class BendTracker {
public:
    float NeckDrop() const {return std::max(0.f,-m_neck[1]);}
    float MeasuredAngle() const {return m_measuredAngle;}
    // The height loss that is a squat (the physical crouch's threshold, PhysicalCrouch.hpp CrouchThreshold): the floor
    // reach's inferred hinge has handed over to the squat by then (Update).
    void SetSquatDepth(float depth) {if(std::isfinite(depth))m_squatDepth=std::clamp(depth,.15f,.55f);}
    // walking: the body is walking after the head (roomscale's follow in progress, cvr::roomscale::BodyFollowing).
    BendSample Update(const std::array<float,3>& head,const std::array<float,4>& q,uint64_t origin,bool swimming=false,
                      bool walking=false) {
        // No land posture may accumulate underwater and reappear on exit.
        if(swimming) { *this={};return {}; }
        for(float v:head)if(!std::isfinite(v))return m_sample;
        float norm=0;for(float v:q) { if(!std::isfinite(v))return m_sample;norm+=v*v; }
        if(norm<.5f || norm>1.5f)return m_sample;
        if(!m_initialized || m_origin!=origin) { *this={};m_origin=origin;m_initialized=true; }
        // Optical centre relative to neck, in OpenXR axes. Remove only motion
        // actually consistent with this lever, so orientation-only simulator
        // input cannot manufacture a translated neck.
        const float inv=1/std::sqrt(norm),x=q[0]*inv,y=q[1]*inv,z=q[2]*inv,w=q[3]*inv;
        const std::array<float,3> v{0,EyeAboveNeck,-EyeAheadOfNeck};
        const std::array<float,3> t{2*(y*v[2]-z*v[1]),2*(z*v[0]-x*v[2]),2*(x*v[1]-y*v[0])};
        const std::array<float,3> lever{w*t[0]+y*t[2]-z*t[1],w*t[1]+z*t[0]-x*t[2],w*t[2]+x*t[1]-y*t[0]};
        std::array<float,3> raw{},expected{},travel{};float projection=0,expectedSq=0;
        for(int i=0;i<3;++i) {
            raw[i]=head[i]-m_head[i];expected[i]=lever[i]-m_lever[i];
            projection+=raw[i]*expected[i];expectedSq+=expected[i]*expected[i];
        }
        const float leverWeight=expectedSq>1e-10f ? std::clamp((projection/expectedSq-.25f)/.5f,0.0f,1.0f):0;
        const auto previousNeck=m_neck;
        for(int i=0;i<3;++i) { travel[i]=raw[i]-expected[i]*leverWeight;m_neck[i]+=travel[i]; }
        m_head=head;m_lever=lever;
        // A TURN OF THE HEAD IS NO HINGE (2026-11-07). A player's own eyes do not sit exactly EyeAboveNeck/EyeAheadOfNeck
        // over their neck: 2 cm off, a quarter turn on the spot swings the neck the tracker sees round a 3 cm arc, and
        // that kept as a hinge walked the strict capsule 26-28 mm off after a few quick turns. The neck travel a lever
        // error of up to TurnLeverSlack explains for this update's yaw moves the accepted neck along with it: neither a
        // hinge nor a step (a hinge does not turn the head; one made while turning loses only that share).
        {
            auto turned=[&](float vx,float vy,float vz) {
                const float tx=2*(y*vz-z*vy),ty=2*(z*vx-x*vz),tz=2*(x*vy-y*vx);
                return std::array<float,3>{vx+w*tx+y*tz-z*ty,vy+w*ty+z*tx-x*tz,vz+w*tz+x*ty-y*tx};
            };
            const auto f=turned(0,0,-1),u=turned(0,1,0);
            float hx=f[0],hz=f[2];
            if(hx*hx+hz*hz<.09f) { hx=f[1]<0 ? u[0]:-u[0];hz=f[1]<0 ? u[2]:-u[2]; }
            const float yaw=std::atan2(-hx,-hz);
            if(m_haveYaw) {
                const float explained=TurnLeverSlack*std::abs(std::remainder(yaw-m_lastYaw,6.28318531f));
                const float along=std::hypot(travel[0],travel[2]);
                if(along>1e-9f && explained>0) {
                    const float share=std::min(1.0f,explained/along);
                    m_acceptedNeck[0]+=travel[0]*share;m_acceptedNeck[2]+=travel[2]*share;
                }
            }
            m_lastYaw=yaw;m_haveYaw=true;
        }
        // At level pitch/roll the optical lever has no vertical displacement.
        // Re-observe this exact height instead of retaining an integrated error
        // from noisy up/down head rotations in earlier gestures.
        if(std::abs(lever[1])<.0001f)m_neck[1]=head[1];
        const float drop=std::max(0.0f,-m_neck[1]);
        if(drop==m_lastDrop && std::hypot(m_neck[0]-previousNeck[0],m_neck[2]-previousNeck[2])>1e-6f) {
            // A completed exact endpoint owns its whole provisional hinge before
            // horizontal locomotion starts; no tail may be lost on that handoff.
            m_right=m_sample.right;m_forward=m_sample.forward;m_drop=drop;m_acceptedNeck=previousNeck;
        }
        m_lastDrop=drop;
        // standing at the calibrated height: a hinge still held is the noise's, and bleeds away (a smooth few hundred ms,
        // not the clamp below dropping it in one update)
        if(drop<BendDropNoise) { m_right*=1-BendNoiseBleed;m_forward*=1-BendNoiseBleed; }
        const float deltaDrop=drop-m_drop;
        const float dx=m_neck[0]-m_acceptedNeck[0],df=-(m_neck[2]-m_acceptedNeck[2]);
        const float length=std::hypot(dx,df);
        auto arc=[](float loss) {
            loss=std::clamp(loss,0.0f,BendReach);
            return std::min(BendReach*std::sin(MaxBend),std::sqrt(std::max(0.0f,loss*(2*BendReach-loss))));
        };
        auto loss=[](float span) { return BendReach-std::sqrt(std::max(0.0f,BendReach*BendReach-span*span)); };
        float right=m_right,forward=m_forward;
        float span=std::hypot(right,forward);
        // WALKING, A BOB IS NO HINGE (2026-11-06). A walking head drops a couple of centimetres and rises again with
        // every step, and near the top of the arc a 2 cm drop explains 16 cm of forward travel: the bob's descent was
        // taken out of the movement sample as a hinge and its rise gave it back, and the body's walk surged and sagged
        // with every step. A hinge grows while walking only past the bob.
        // A HEIGHT THAT ONLY TREMBLES HANDS NOTHING TO THE HINGE (2026-11-07). Near the top of the arc a fraction of a millimetre of drop explains centimetres of travel, so with the
        // drop a hair above the accepted one the whole travel since the last acceptance was the hinge's, and a hair below
        // it was walking again: standing still after fast turns (the neck estimate a few mm off and a centimetre low),
        // the hinge's XY flipped by that travel at the frame rate -- 5 mm in game, up to 31 mm in the test -- and
        // roomscale, which takes the hinge out of the head, walked the capsule forward and back with it. The first
        // BendDropNoise of descent now hands the travel over gradually; and a neck standing still is accepted as it
        // stands (below), so no travel is left over to hand back and forth.
        if(deltaDrop>0 && length>1e-7f && (!walking || drop>BendWalkBobDrop)) {
            const float growth=std::max(0.0f,arc(loss(span)+deltaDrop)-span);
            const float fraction=std::min(length,growth)/length*SmoothRange(deltaDrop,0.0f,BendDropNoise);
            right+=dx*fraction;forward+=df*fraction;
        } else if(deltaDrop<0 && span>1e-7f) {
            const float inward=std::max(0.0f,-(dx*right+df*forward)/span);
            const float shrink=std::max(0.0f,span-arc(std::max(0.0f,loss(span)+deltaDrop)));
            const float fraction=1-std::min(inward,shrink)/span;
            right*=fraction;forward*=fraction;
        }
        span=std::hypot(right,forward);
        // The arc's span for this drop. arc() is steep at a small drop: a hinge held at a drop of a millimetre was
        // clamped by centimetres on the frames the height's noise came out low and given back on the next. Within the
        // noise of the accepted drop the accepted one bounds it (the accepted hinge already fits it); a real rise, and
        // the calibrated height itself, still bound it at once.
        const float allowed=arc(drop>0 && std::abs(deltaDrop)<BendDropNoise ? std::max(drop,m_drop):drop);
        if(span>allowed && span>1e-7f) { right*=allowed/span;forward*=allowed/span;span=allowed; }
        // Evaluate the current raw endpoint every frame, but only commit a
        // segment after height changes beyond ordinary tracking noise. Otherwise
        // alternating +/-2mm height errors throw away half a real hinge's XY
        // travel. Provisional displacement remains immediate for native CCT.
        auto still=[&]() { m_steady=0;m_stillNeck=m_neck;m_stillDrop=drop; };
        if(std::abs(deltaDrop)>=.015f || drop==0) {
            m_right=right;m_forward=forward;m_drop=drop;m_acceptedNeck=m_neck;still();
        } else if(length>.18f) {
            // Translation at held height is walking (also while crouched or
            // bent), not unlimited pending hinge travel for a future descent.
            m_drop=drop;m_acceptedNeck=m_neck;still();
        } else if(std::hypot(m_neck[0]-m_stillNeck[0],m_neck[2]-m_stillNeck[2])<BendStillTravel &&
                  std::abs(drop-m_stillDrop)<BendStillTravel) {
            // the neck standing still: the posture as it stands is accepted (see the growth above)
            if(++m_steady>=BendSteadyUpdates) { m_right=right;m_forward=forward;m_drop=drop;m_acceptedNeck=m_neck;still(); }
        } else still();
        // Fade the VISIBLE pose, never the already classified displacement.
        // Returning the faded vector to CCT turns the missing hinge into a
        // walking request at the onset and again while standing up.
        const float weight=SmoothRange(span,BendSpanStart,BendSpanFull)*SmoothRange(drop,BendDropStart,BendDropFull);
        const float measured=std::asin(std::clamp(span/BendReach,0.0f,1.0f))*weight;
        m_measuredAngle=measured;
        // Reaching toward the floor can lower the head a long way while the
        // pelvis retreats and the neck barely advances. The backup posture
        // solver used a reach cue to infer this missing hinge. Here physical
        // neck descent plus downward gaze supplies that cue; gaze alone cannot
        // bend the body, and a vertical crouch looking ahead remains a crouch.
        const float down=std::clamp(2*(y*z-w*x),-1.0f,1.0f);
        // FROM THE LEAN INTO THE SQUAT (2026-11-07). Going
        // lower than a reach, the knees take over: the inferred hinge gives way as the drop nears the squat's depth and
        // is gone there. It used to stand at 55-70 degrees until the physical crouch came on and was then dropped in a
        // frame -- the torso stood up and the hips jumped from half a metre behind the head to under it. A function of
        // the drop alone, it is the same going down and coming up (the crouch's own hysteresis does not shape it).
        const float handover=std::clamp(m_squatDepth-.16f,FloorReachDropStart,m_squatDepth-.05f);
        const float reach=SmoothRange(down,.25f,.75f)*SmoothRange(drop,FloorReachDropStart,FloorReachDropFull)*
            (1-SmoothRange(drop,handover,m_squatDepth));
        const float inferred=std::acos(std::clamp(1-std::min(drop,HingeDrop(MaxBend))/BendReach,0.0f,1.0f));
        m_sample={right,forward,std::max(measured,inferred*reach)};
        return m_sample;
    }
private:
    std::array<float,3> m_head{},m_lever{},m_neck{};
    std::array<float,3> m_acceptedNeck{};
    float m_right{},m_forward{},m_drop{},m_lastDrop{},m_measuredAngle{};
    float m_squatDepth=.45f;
    float m_lastYaw{};
    bool m_haveYaw=false;
    std::array<float,3> m_stillNeck{};
    float m_stillDrop{};
    int m_steady{};
    uint64_t m_origin{};
    bool m_initialized{};
    BendSample m_sample{};
};
}
