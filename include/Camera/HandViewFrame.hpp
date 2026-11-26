#pragma once
#include "Camera/AnchorTranslation.hpp"
#include "Utils/XrMath.hpp"

namespace cvr::camera {
struct HandViewFrame { AnchorVector position;XrQuaternionf rotation; };
#if CVR_RUST_ROOM
static_assert(sizeof(HandViewFrame) == 28);
inline HandViewFrame BuildHandViewFrame(AnchorVector bodyBase,const AnchorRecipe& recipe,
        AnchorVector headResidualXr,XrQuaternionf headXr,float bodyYaw,float trackingYaw,
        XrQuaternionf pitch) {
    HandViewFrame out;   // rust/crates/cvr-camera/src/frames.rs
    cvr_hand_view_frame(ToC(bodyBase),&recipe,ToC(headResidualXr),&headXr,bodyYaw,trackingYaw,&pitch,&out);
    return out;
}
#else
inline HandViewFrame BuildHandViewFrame(AnchorVector bodyBase,const AnchorRecipe& recipe,
        AnchorVector headResidualXr,XrQuaternionf headXr,float bodyYaw,float trackingYaw,
        XrQuaternionf pitch) {
    const auto delta=ComposeViewTranslation(
        {headResidualXr.x*recipe.scale,-headResidualXr.z*recipe.scale,headResidualXr.y*recipe.scale},
        recipe,trackingYaw,bodyYaw);
    const auto modelDelta=RotateAnchorYaw(delta,-bodyYaw);
    const XrQuaternionf invBody{0,0,-std::sin(bodyYaw*.5f),std::cos(bodyYaw*.5f)};
    const XrQuaternionf heading{0,0,std::sin(trackingYaw*.5f),std::cos(trackingYaw*.5f)};
    const XrQuaternionf mappedHead{headXr.x,-headXr.z,headXr.y,headXr.w};
    auto rotation=MultiplyQuat(invBody,MultiplyQuat(MultiplyQuat(heading,pitch),mappedHead));
    rotation=NlerpQuat(rotation,rotation,0);
    return {{bodyBase.x+modelDelta.x,bodyBase.y+modelDelta.y,bodyBase.z+modelDelta.z},rotation};
}
#endif
}
