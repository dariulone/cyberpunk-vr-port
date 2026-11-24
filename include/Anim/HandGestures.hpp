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
// Finger-tracking gloves (xr_finger_bones): every digit follows its own measured bend, thumb..little, from a
// straight finger to the native fist. Glove curl reads about 0.1 with the hand open and 1 in a fist, so
// kTrackedOpen..kTrackedClosed is stretched to 0..1.
constexpr float kTrackedOpen=.15f,kTrackedClosed=.90f;
constexpr Targets FromTracking(const float curl[5]) {
    Targets result;
    for(int digit=0;digit<5;++digit) {
        const float x=(curl[digit]-kTrackedOpen)/(kTrackedClosed-kTrackedOpen);
        result.closed[digit]=x<0?0.f:(x>1?1.f:x);
        result.straight[digit]=1;
    }
    return result;
}
namespace detail {
constexpr float kOpenHand[5]={.10f,.12f,.10f,.14f,.18f},kFist[5]={.95f,1,.96f,1,1},kPoint[5]={.90f,.15f,1,1,1};
static_assert(FromTracking(kOpenHand).closed[2]==0 && FromTracking(kOpenHand).straight[2]==1,"an open hand is straight");
static_assert(FromTracking(kFist).closed[0]==1 && FromTracking(kFist).closed[4]==1,"a fist closes every digit");
static_assert(FromTracking(kPoint).closed[1]==0 && FromTracking(kPoint).closed[2]==1,"each digit is its own");
static_assert(FromTracking(kPoint).closed[0]>.98f && FromTracking(kPoint).closed[0]<=1,"a bent thumb closes");
}
using CalibrationWeights=std::array<float,4>;
inline CalibrationWeights CalibrationTargets(float grip,float trigger,bool thumbsUp,bool wire) {
    grip=std::clamp(grip,0.f,1.f);trigger=std::clamp(trigger,0.f,1.f);
    if(wire)return {};
    if(thumbsUp && grip>.5f)return {0,0,grip,0};
    return {grip*(1-trigger),grip*trigger,0,(1-grip)*trigger};
}
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
void ApplyCalibration(int side,const char* suffix,const CalibrationWeights& weights,float* rotation);
void PublishInput(float leftGrip,float rightGrip,float leftTrigger,float rightTrigger,bool rightThumbsUp,bool enabled);
void PublishEquipment(int occupiedMask);
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
// Finger-tracking gloves (xr_finger_bones): every digit of both hands from the gloves, written after every other
// finger layer (weapons, reload, smoking, wheel, ladder, carry), so nothing else poses the fingers. Nothing without data.
void ApplyGloves(uint8_t* boneBuffer);
bool OwnsHand(int side);
bool ThumbReference(int side,float* rotation);
// FINGER SPREAD (xr_finger_spread): the bone that spreads a digit (thumb = its root, the others their knuckle) and the
// axis it turns around, in that bone's parent space, from the rig's reference pose: the palm's normal, signed so a
// positive angle moves the digit away from the middle finger (the thumb: away from the index). Digit 1, 3 or 4, or 0.
bool SpreadAxis(int side,int digit,int* bone,float axis[3]);
}
