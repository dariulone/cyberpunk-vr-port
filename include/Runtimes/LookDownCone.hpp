#pragma once
#include <algorithm>
#include <cmath>
#include "Anim/BodyBend.hpp"

namespace cvr::roomscale {
#if CVR_RUST_BODY
// rust/crates/cvr-body/src/look_down.rs
inline float HeadDownDegrees(float x,float y,float z,float w) { return cvr_head_down_degrees(x,y,z,w); }
inline float LookDownFreeLookCone(float downDegrees,float normalDegrees,float loweredDegrees,float bodyBendRadians=0) {
    return cvr_look_down_free_look_cone(downDegrees,normalDegrees,loweredDegrees,bodyBendRadians);
}
inline float BodyFreeLookCone(bool swimming,float swimmingDegrees,float downDegrees,
                             float normalDegrees,float loweredDegrees,float bodyBendRadians=0) {
    return cvr_body_free_look_cone(swimming,swimmingDegrees,downDegrees,normalDegrees,loweredDegrees,bodyBendRadians);
}
#else
// Pitch from the head's forward vector, independent of yaw and local roll.
// OpenXR forward is -Z; positive result means looking toward the floor.
inline float HeadDownDegrees(float x,float y,float z,float w) {
    const float norm=x*x+y*y+z*z+w*w;
    if(!std::isfinite(norm) || norm<.5f || norm>1.5f)return 0;
    const float down=std::clamp(2*(y*z-w*x)/norm,-1.0f,1.0f);
    return std::asin(down)*57.295779513f;
}
inline float LookDownFreeLookCone(float downDegrees,float normalDegrees,float loweredDegrees,float bodyBendRadians=0) {
    normalDegrees=std::clamp(std::isfinite(normalDegrees) ? normalDegrees:5.0f,0.0f,60.0f);
    loweredDegrees=std::clamp(std::isfinite(loweredDegrees) ? loweredDegrees:30.0f,0.0f,90.0f);
    if(!std::isfinite(downDegrees))downDegrees=0;
    // Leave small natural head tilts alone; reach the full wider cone by 30deg
    // down, before the usual chest-belt reach. Never narrow the player's cone.
    const float t=std::clamp((downDegrees-10.0f)/20.0f,0.0f,1.0f);
    const float blend=t*t*(3-2*t);
    const float cone=normalDegrees+(std::max(normalDegrees,loweredDegrees)-normalDegrees)*blend;
    return cone+(std::max(cone,60.0f)-cone)*cvr::body::BendWeight(bodyBendRadians);
}
inline float BodyFreeLookCone(bool swimming,float swimmingDegrees,float downDegrees,
                             float normalDegrees,float loweredDegrees,float bodyBendRadians=0) {
    if(swimming)return std::clamp(std::isfinite(swimmingDegrees) ? swimmingDegrees:5.0f,0.0f,90.0f);
    return LookDownFreeLookCone(downDegrees,normalDegrees,loweredDegrees,bodyBendRadians);
}
#endif
}
