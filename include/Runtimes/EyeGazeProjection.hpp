#pragma once
#include <openxr/openxr.h>
#include <algorithm>
#include <cmath>

namespace cvr::eye_tracking {
// Project the combined gaze's forward ray (-Z) into a rendered head-space window.
// This is angular foveation: XR_EXT supplies no binocular convergence depth.
inline bool Project(XrQuaternionf q, float left, float right, float down, float up, float& x, float& y) {
    const float norm=q.x*q.x+q.y*q.y+q.z*q.z+q.w*q.w;
    if(!std::isfinite(norm) || norm<.9f || norm>1.1f || !std::isfinite(left) || !std::isfinite(right) ||
       !std::isfinite(down) || !std::isfinite(up) || right-left<.05f || up-down<.05f)return false;
    const float n=std::sqrt(norm);q.x/=n;q.y/=n;q.z/=n;q.w/=n;
    const float dx=-2*(q.x*q.z+q.w*q.y),dy=2*(q.w*q.x-q.y*q.z),dz=2*(q.x*q.x+q.y*q.y)-1;
    if(dz>=-.05f)return false;
    x=std::clamp((dx/-dz-left)/(right-left),0.f,1.f);
    y=std::clamp((up-dy/-dz)/(up-down),0.f,1.f);
    return std::isfinite(x) && std::isfinite(y);
}
inline bool FreshSample(XrTime predicted, XrTime sample) {
    return predicted>0 && sample>0 && sample>=predicted-100000000 && sample<=predicted+50000000;
}
}
