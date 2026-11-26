#pragma once
#include "Natives/HandReferenceFrame.hpp"
#include <algorithm>

namespace cvr::blade {
using handspace::Vector;
using handspace::Rotation;

// +Y is the asset's cutting edge. Remove its component along the authored
// grip-to-tip line in item space before either the hand or the target turns.
inline bool LocalCutEdge(Vector shaft,Vector& edge) {
    const float n=handspace::LengthSquared(shaft);
    if(!(n>1.e-6f && std::isfinite(n))) return false;
    edge=Vector{0,1,0}-shaft*(shaft.y/n);
    const float e=handspace::LengthSquared(edge);
    if(!(e>.01f)) return false;
    edge=edge*(1.f/std::sqrt(e));return true;
}

// The actuator has to start at the solver contact, including when a puppet
// reaction left the raw controller a long way from its held virtual wrist.
// Both local D6 frames coincide at zero commanded travel.
inline Vector EdgeDriveWrist(Vector bodyPoint,Rotation bodyFrame,Vector handAnchor,Rotation handFrame,
                             float advance,Rotation& wristRotation) {
    wristRotation=Multiply(bodyFrame,Conjugate(handFrame));
    return bodyPoint+Rotate(bodyFrame,{advance,0,0})-Rotate(wristRotation,handAnchor);
}

// Both samples are in the coupled actor's frame. Common body motion drops
// out. Only movement along the D6's currently free translation is carried
// into the animated slot. Locked-axis solver error must not move the pin.
struct RailTravel {
    Vector last{}, total{};
    void Sample(Vector current, Vector axis, bool sliding) {
        const Vector step=current-last;
        if(sliding) total=total+axis*(step.x*axis.x+step.y*axis.y+step.z*axis.z);
        last=current;
    }
};

struct BladeAttachment {
    Vector wristLocal{},initialTravel{},priorSlip{},slotSlip{};
    Rotation bodyToSlot{};

    void BindWrist(Vector station,Vector wrist,Rotation rotation) {
        wristLocal=Rotate(Conjugate(rotation),station-wrist);
    }
    void BindRail(Vector travel,Rotation body,Rotation slot) {
        initialTravel=travel;priorSlip=slotSlip;
        bodyToSlot=Multiply(Conjugate(slot),body);
    }
    Vector Goal(Vector animatedPoint,Rotation slot,Vector travel) {
        slotSlip=priorSlip+Rotate(bodyToSlot,travel-initialTravel);
        return animatedPoint+Rotate(slot,slotSlip);
    }
    Vector Wrist(Vector goal,Rotation rotation) const { return goal-Rotate(rotation,wristLocal); }
};

// Permit displacement along the tracked wrist's travel. Bound only the
// remaining displacement of the virtual wrist since this contact was seated.
struct BladeWristLeash {
    Vector seated{},controller{};
    bool Limit(Vector tracked, Vector& wrist) const {
        constexpr float slack=.12f, release=.48f;
        const auto travel=wrist-seated;
        const auto input=tracked-controller;
        if(!Finite(input) || !Finite(travel)) return false;
        const float inputSquared=LengthSquared(input);
        const float along=input.x*travel.x+input.y*travel.y+input.z*travel.z;
        const float fraction=inputSquared>1.e-8f ? std::clamp(along/inputSquared,0.f,1.f) : 0.f;
        const auto supported=seated+input*fraction;
        const auto offset=wrist-supported;
        const float gap=std::sqrt(LengthSquared(offset));
        constexpr float freePlay=.012f;
        if(gap>freePlay) {
            const float give=std::min(slack,freePlay+.25f*(gap-freePlay));
            wrist=supported+offset*(give/gap);
        }
        return gap<=release;
    }
};

inline Vector ReleaseWrist(Vector physics,Vector controller,Vector lastOffset,float t) {
    t=std::clamp(t,0.f,1.f);
    const float fade=t*t*(3.f-2.f*t);
    const auto held=controller+lastOffset;
    return held+(physics-held)*fade;
}
}
