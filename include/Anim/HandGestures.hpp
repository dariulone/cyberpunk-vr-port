#pragma once
#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>

namespace cvr::gestures {
struct Targets {float closed[5]{},straight[5]{};};
inline Targets Evaluate(float grip,float trigger,bool thumbsUp,bool wire,float deployment) {
    grip=std::clamp(grip,0.f,1.f);trigger=std::clamp(trigger,0.f,1.f);
    Targets result;
    if(wire) {
        float x=grip>.5f?std::clamp((deployment-.35f)/.30f,0.f,1.f):0.f;
        x=x*x*(3-2*x);
        for(auto& value:result.closed)value=x;
    } else {
        result.closed[0]=grip*trigger;
        result.closed[1]=trigger;
        result.straight[1]=grip*(1-trigger);
        result.closed[2]=result.closed[3]=result.closed[4]=grip;
        if(thumbsUp && grip>.5f) {
            result.closed[0]=0;result.straight[0]=grip;
            result.closed[1]=grip;result.straight[1]=0;
        }
    }
    return result;
}
using CalibrationWeights=std::array<float,4>;
inline CalibrationWeights CalibrationTargets(float grip,float trigger,bool thumbsUp,bool wire) {
    grip=std::clamp(grip,0.f,1.f);trigger=std::clamp(trigger,0.f,1.f);
    if(wire)return {};
    if(thumbsUp && grip>.5f)return {0,0,grip,0};
    return {grip*(1-trigger),grip*trigger,0,(1-grip)*trigger};
}
// A measured finger (Runtimes/HandTracking.hpp): its bend in degrees, added up over its joints, as 0 open .. 1 closed.
// The ends are what Index controllers report for a flat hand and a fist; thumb to little finger.
inline float FingerClosed(int digit,float bend) {
    constexpr float open[5]={30,25,30,35,40},closed[5]={120,270,275,270,240};
    if(digit<0 || digit>4)return 0;
    const float x=std::clamp((bend-open[digit])/(closed[digit]-open[digit]),0.f,1.f);
    // The thumb comes in levels: about 30 off the controller, 80-90 resting on the stick, 110-120 pushing it. Squared,
    // resting stays nearer the open thumb, so the levels look different.
    return digit==0?x*x:x;
}
// Each finger on its own: straight as measured open, through to the fist. The outer three never go quite flat: their
// sensors call a finger open a centimetre off the handle, and a relaxed hand has them a little bent, more toward the
// little finger.
inline Targets FromFingers(const float* closed) {
    constexpr float relaxed[5]={0,0,.12f,.16f,.2f};
    Targets result;
    for(int digit=0;digit<5;++digit) {
        result.closed[digit]=relaxed[digit]+(1-relaxed[digit])*closed[digit];result.straight[digit]=1;
    }
    return result;
}
// One step of a measured finger. The grip's sensors read a finger as fully open about a centimetre off the handle, so
// the three outer fingers would spring open: they open at a set pace instead, and close as fast as the rest.
inline float FollowFinger(float current,float wanted,float blend,int digit,float dt) {
    constexpr float openSeconds=.18f;
    const float next=current+(wanted-current)*blend;
    if(digit<2 || wanted>=current)return next;
    return std::max(next,current-dt/openSeconds);
}
// The corrections go by the same hand shapes: the three outer fingers stand for the grip, the index for the trigger,
// and a thumb lifted off the controller is the thumbs-up.
// Where a finger's limit goes from what the rays last said, which no longer depends on the finger. Down quickly but not in one
// frame, up only once the surface has stayed gone for a moment, and not at all for a change too small to be anything but the ray.
inline float StepFingerLimit(float limit,float wanted,float dt,float& waited) {
    constexpr float dead=.02f,fall=15.f,rise=2.5f,wait=.12f;
    if(wanted<limit-dead) {waited=0;return std::max(wanted,limit-fall*dt);}
    if(wanted>limit+dead) {
        waited+=dt;
        return waited<wait ? limit:std::min(wanted,limit+rise*dt);
    }
    waited=0;
    return limit;
}
inline CalibrationWeights FingerCalibration(const float* closed) {
    auto weights=CalibrationTargets((closed[2]+closed[3]+closed[4])/3,closed[1],false,false);
    weights[2]=1-closed[0];
    return weights;
}
void ApplyCalibration(int side,const char* suffix,const CalibrationWeights& weights,float* rotation);
void PublishInput(float leftGrip,float rightGrip,float leftTrigger,float rightTrigger,bool rightThumbsUp,bool enabled);
void PublishEquipment(int occupiedMask);
// Side 0 left, 1 right. Five values 0..1, thumb first, or null when that hand has no measurement. While fresh they
// take over from grip and trigger on an empty hand.
void PublishFingers(int side,const float* closed);
// How far a finger may close, 0 open .. 1 fist, worked out from the hand alone. With `hold` it closes exactly that far: the
// fingers around a thing in the hand. Side 0 left, 1 right; digit 0 thumb .. 4 little finger. A hand no report has come for
// lately is free again.
void FingerContact(int side,int digit,float limit,bool hold);
// The finger as drawn, and as it would be with nothing in its way. They are what the script measures a finger's path by.
// How far a finger is pressed back past straight, radians at its first joint, by a surface it runs into straight on.
void FingerBend(int side,int digit,float angle);
float FingerClosed(int side,int digit);
float FingerWanted(int side,int digit);
// A hand the port has put something in, beside the game's weapon slots: the gun carried in the left hand, a reload's
// magazine, shell or grip, a cigarette. Side 0 left, 1 right (the controllers as they are).
bool PortHolds(int side);
void Apply(uint8_t* boneBuffer);
bool OwnsHand(int side);
bool ThumbReference(int side,float* rotation);
}
