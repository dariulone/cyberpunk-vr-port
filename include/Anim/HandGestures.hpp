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
void ApplyCalibration(int side,const char* suffix,const CalibrationWeights& weights,float* rotation);
void PublishInput(float leftGrip,float rightGrip,float leftTrigger,float rightTrigger,bool rightThumbsUp,bool enabled);
void PublishEquipment(int occupiedMask);
// A hand the port has put something in, beside the game's weapon slots: the gun carried in the left hand, a reload's
// magazine, shell or grip, a cigarette. Side 0 left, 1 right (the controllers as they are).
bool PortHolds(int side);
void Apply(uint8_t* boneBuffer);
bool OwnsHand(int side);
bool ThumbReference(int side,float* rotation);
}
