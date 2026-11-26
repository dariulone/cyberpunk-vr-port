#pragma once
#include "Natives/HandReferenceFrame.hpp"

namespace cvr::physhands {
// All positions are world coordinates. These POD snapshots cross the physics,
// player animation and NPC animation threads without retaining game objects.
struct GripControl {
    float position[3]{},rotation[4]{0,0,0,1},shoulder[3]{},reach{};
    uint64_t at{};
};
struct PersonContact {
    int owner{};uint64_t serial{},at{};
    float position[3]{},rotation[4]{0,0,0,1};
    float capsule[3]{},capsuleRotation[4]{0,0,0,1},radius{},half{};
    bool ready{};
};
struct PersonAttachment {
    int owner{};uint64_t serial{},at{};
    uint64_t revision{},controlAt{};
    float position[3]{},rotation[4]{0,0,0,1};
    float capsule[3]{},capsuleRotation[4]{0,0,0,1},radius{},half{};
    bool on{},release{};
};
bool ReadGripControl(int side,GripControl& out);
bool ReadPersonContact(int side,PersonContact& out);
bool ReadPersonAttachment(int side,PersonAttachment& out);
void PublishPersonAttachment(int side,const PersonAttachment& value);
inline bool LimitGripReach(const GripControl& control,float* point){
    float distance2=0;for(int k=0;k<3;++k){const float d=point[k]-control.shoulder[k];distance2+=d*d;}
    if(!std::isfinite(distance2)||!(control.reach>.01f)||!std::isfinite(control.reach))return false;
    const float distance=std::sqrt(distance2);
    if(distance>control.reach)for(int k=0;k<3;++k)point[k]=control.shoulder[k]+(point[k]-control.shoulder[k])*(control.reach/distance);
    return true;
}
}
