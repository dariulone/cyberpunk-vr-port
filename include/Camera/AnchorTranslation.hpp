#pragma once
#include <cmath>

namespace cvr::camera {
struct AnchorVector {
    float x{}, y{}, z{};
};

inline AnchorVector ActiveVehicleCameraOffset(AnchorVector configured,bool passengerCombat) {
    return passengerCombat ? AnchorVector{} : configured;
}

inline AnchorVector RotateAnchorYaw(AnchorVector v, float yaw) {
    const float c=std::cos(yaw), s=std::sin(yaw);
    return {c*v.x-s*v.y, s*v.x+c*v.y, v.z};
}

// headResidual and trackingOffset use the recenter/play-space axes; modelOffset
// is published by the skeleton in model axes. They are not the same after the
// native physical body follower contributes yaw to the entity.
inline AnchorVector ComposeAnchorTranslation(AnchorVector headResidual,
                                            AnchorVector trackingOffset,
                                            AnchorVector modelOffset,
                                            float trackingYaw, float modelYaw) {
    const auto room=RotateAnchorYaw({headResidual.x+trackingOffset.x,
                                    headResidual.y+trackingOffset.y,
                                    headResidual.z+trackingOffset.z},trackingYaw);
    const auto body=RotateAnchorYaw(modelOffset,modelYaw);
    return {room.x+body.x,room.y+body.y,room.z+body.z};
}

struct AnchorRecipe {
    AnchorVector trackingOffset{}, modelOffset{};
    float scale{1.0f}, trackingYaw{}, modelYaw{};
    float centreYaw{};
    // On foot the HMD owns the optical centre. Calibration establishes the
    // tracking origin; body yaw changes the avatar's placement about that point.
    bool bodyOrbit{};
};

inline AnchorVector ComposeViewTranslation(AnchorVector residual,const AnchorRecipe& recipe,
                                           float trackingYaw,float bodyYaw) {
    return ComposeAnchorTranslation(residual,recipe.trackingOffset,recipe.modelOffset,
                                    trackingYaw,recipe.bodyOrbit ? recipe.centreYaw:bodyYaw);
}

// THE BODY'S NECK GOES WHERE THE PLAYER'S NECK IS. Two turns move the body, and they turn about different points:
//  - a PHYSICAL turn turns the head about the neck: the eyes travel round it (Runtimes/RoomscaleMovement.hpp,
//    EyeFromNeck) and the view follows them through the tracked residual. The neck stays put, and the body turns
//    about it. Turned about the eye instead, the lever was counted twice -- once by the tracking, once by the orbit
//    -- and after a 180 deg turn on the spot the body stood a quarter of a metre off the head (258 mm, simulator,
//    2026-11-06: "my head must stay right over the body ... big problems with physical body rotation");
//  - a STICK turn turns the world about the eyes: the calibrated eye (modelOffset, composed at the fixed centreYaw)
//    stays put, the tracking yaw turns, and with it the place of the neck behind the eyes. Turned about the neck as
//    well, the body swung round the view by twice the lever on every stick turn ("now the stick
//    turn does not turn about the body's orbit").
// So the body's neck -- the calibrated eye less the lever kEyeAheadOfNeck along the model's forward -- is put at the
// player's neck: the calibrated eye (turned by centreYaw) less the lever along the TRACKING forward (the view yaw,
// which only stick turns move). The body turns about its own neck by whatever yaw it has: a physical turn is a turn
// about the neck, a stick turn one about the eyes, and both at once compose.
inline constexpr float kOrbitEyeAheadOfNeck=0.09f;   // tracking metres, = cvr::roomscale::kEyeAheadOfNeck
inline float BodyOrbitScale(const AnchorRecipe& recipe) {
    return std::isfinite(recipe.scale) && recipe.scale>0 ? recipe.scale:1.0f;
}
// The eye's lever over the neck in model axes (straight ahead), scaled.
inline AnchorVector BodyOrbitLever(const AnchorRecipe& recipe) {
    return {0,kOrbitEyeAheadOfNeck*BodyOrbitScale(recipe),0};
}
// THE PLAYER'S NECK, NOT AN ASSUMED ONE (2026-11-07). The lever
// straight ahead is the neck only for a head that was level and straight ahead where roomscale began, and that never
// moved without the capsule since. Otherwise the player's neck stood off the assumed one by a fixed vector in the room
// -- 10-14 mm in the simulator -- and every physical turn swung the body round the wrong point: off the head by twice
// that times sin(turn/2), 20 mm at 90 degrees, back to nothing facing ahead again. restNeck, the roomscale ledger's neck
// at rest (cvr::roomscale::CameraRestNeck: tracking axes and metres, from the calibrated eye), puts the body's neck at
// the player's. More than kOrbitRestNeckReach off the assumed neck is not a neck (an offset the capsule never took):
// clamped to it.
inline constexpr float kOrbitRestNeckReach=0.15f;   // tracking metres
inline AnchorVector BodyOrbitNeckLever(const AnchorRecipe& recipe,const AnchorVector* restNeck) {
    const auto lever=BodyOrbitLever(recipe);
    if(!restNeck || !std::isfinite(restNeck->x) || !std::isfinite(restNeck->y))return lever;
    float dx=restNeck->x,dy=restNeck->y+kOrbitEyeAheadOfNeck;
    const float off=std::sqrt(dx*dx+dy*dy);
    if(off>kOrbitRestNeckReach){dx*=kOrbitRestNeckReach/off;dy*=kOrbitRestNeckReach/off;}
    const float scale=BodyOrbitScale(recipe);
    return {-dx*scale,(kOrbitEyeAheadOfNeck-dy)*scale,0};
}
// The body's neck in the model, as calibrated: the point a physical turn keeps in place.
inline AnchorVector BodyOrbitPivot(const AnchorRecipe& recipe) {
    const auto lever=BodyOrbitLever(recipe);
    return {recipe.modelOffset.x-lever.x,recipe.modelOffset.y-lever.y,0};
}
// trackingYaw: the view's yaw, the native (stick) turns' sum -- BodyTrackingYaw, centreYaw before any.
// restNeck: the player's neck at rest (see BodyOrbitNeckLever); null: the lever straight ahead.
inline AnchorVector BodyOrbitModel(const AnchorRecipe& recipe,float bodyYaw,float trackingYaw,
                                   const AnchorVector* restNeck=nullptr) {
    if(!recipe.bodyOrbit)return {};
    if(!std::isfinite(trackingYaw))trackingYaw=recipe.centreYaw;
    const auto lever=BodyOrbitNeckLever(recipe,restNeck);
    const auto eye=RotateAnchorYaw(recipe.modelOffset,recipe.centreYaw-bodyYaw);
    const auto behind=RotateAnchorYaw(lever,trackingYaw-bodyYaw);
    const auto pivot=BodyOrbitPivot(recipe);
    return {eye.x-behind.x-pivot.x,eye.y-behind.y-pivot.y,0};
}

inline AnchorRecipe MakeAnchorRecipe(AnchorVector manualOffset, AnchorVector vehicleOffset,
                                     AnchorVector bakedOffset, bool inVehicle,
                                     float scale, float trackingYaw, float modelYaw) {
    AnchorRecipe result{};
    result.scale=scale;
    result.trackingYaw=trackingYaw;
    result.modelYaw=modelYaw;
    result.centreYaw=modelYaw;
    result.modelOffset=bakedOffset;
    if (inVehicle) {
        // Seat adjustments retain the vehicle view basis. On foot, the same
        // sliders calibrate camera-to-body alignment, just like the bakes.
        result.trackingOffset={manualOffset.x+vehicleOffset.x,
                               manualOffset.y+vehicleOffset.y,
                               manualOffset.z+vehicleOffset.z};
    } else {
        result.modelOffset={bakedOffset.x+manualOffset.x,
                            bakedOffset.y+manualOffset.y,
                            bakedOffset.z+manualOffset.z};
    }
    return result;
}

inline AnchorVector AbsoluteHorizontalCameraBake(AnchorVector activeBake,
                                                 AnchorVector bodyTarget,
                                                 AnchorVector renderedCamera) {
    // The observation already contains activeBake; replacing it with just the
    // measured difference would undo the previous calibration on the next click.
    return {activeBake.x+bodyTarget.x-renderedCamera.x,
            activeBake.y+bodyTarget.y-renderedCamera.y,0.0f};
}
}
