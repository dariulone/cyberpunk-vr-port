#pragma once
#include <cmath>
#include "Utils/XrMath.hpp"
#if CVR_RUST_ROOM
#include "Rust/CvrRust.h"
#endif

namespace cvr::camera {
#if CVR_RUST_ROOM
// rust/crates/cvr-camera/src/frames.rs
inline bool ScriptedCameraTier(int tier) { return tier>=2 && tier<=5; }
inline bool UseNativeMainHeading(bool mounted,int tier,bool device,bool externalScene) {
    return cvr_use_native_main_heading(mounted,tier,device,externalScene);
}
inline bool CompositionBaseYaw(const float* composed,const float* headXr,float* yaw) {
    return cvr_composition_base_yaw(composed,headXr,yaw);
}
#else
inline bool ScriptedCameraTier(int tier) { return tier>=2 && tier<=5; }
inline bool UseNativeMainHeading(bool mounted,int tier,bool device,bool externalScene) {
    return (mounted || ScriptedCameraTier(tier)) && !device && !externalScene;
}
// Read the base from one completed composition and ITS head sample. No second
// read of a newer body yaw or of the scene's camera animation enters this pair.
inline bool CompositionBaseYaw(const float* composed,const float* headXr,float* yaw) {
    if(!composed || !headXr || !yaw)return false;
    float a=0,b=0;for(int i=0;i<4;++i) { a+=composed[i]*composed[i];b+=headXr[i]*headXr[i]; }
    if(!std::isfinite(a+b) || a<.5f || a>1.5f || b<.5f || b>1.5f)return false;
    const auto base=MultiplyQuat({composed[0],composed[1],composed[2],composed[3]},
        ConjugateQuat({headXr[0],-headXr[2],headXr[1],headXr[3]}));
    const auto q=NlerpQuat(base,base,0);
    const float x=2*(q.x*q.y-q.z*q.w),y=1-2*(q.x*q.x+q.z*q.z);
    if(x*x+y*y<1e-6f)return false;
    *yaw=std::atan2(-x,y);return true;
}
#endif
}
