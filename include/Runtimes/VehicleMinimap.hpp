#pragma once
#include "Utils/XrMath.hpp"
#include <cstdint>
#include <algorithm>
#include <cmath>

namespace cvr::hud {
struct VehicleMinimapAnchor {
    XrPosef pose{{0,0,0,1},{}};
    uint64_t stamp=0,origin=0;
    uintptr_t vehicle=0;
    bool bike=false;
};
// WHERE THE PROJECTED MAP HANGS, from the middle of the hands on the wheel or the bars (game units ~ metres, model
// forward/up), and its size. A car: like an automotive HUD's virtual image beyond the glass over the hood, about 1.4 m
// out on the measured car, a little below eye level. A bike: over the middle of the bars, a hand above them, where a
// bike's dash display sits, seen from half a metre. Sizes halved 2026-11-14 at the user's word (the car's was 0.40 x
// 0.24 m, the bike's 0.17 x 0.10): the map with the quest text under it, which scales with it. The bike's lowered
// 4 cm the same day, also at the user's word (it was 0.12 m over the bars).
struct MinimapPlacement {float forward,up,width,height;};
inline MinimapPlacement MinimapPlacementFor(bool bike) {
    return bike ? MinimapPlacement{.06f,.08f,.085f,.05f} : MinimapPlacement{.92f,.08f,.20f,.12f};
}
inline bool FreshVehicleMinimap(const VehicleMinimapAnchor& anchor,uint64_t now,uint64_t origin,uintptr_t vehicle) {
    return vehicle && anchor.vehicle==vehicle && anchor.origin==origin && anchor.stamp &&
        now>=anchor.stamp && now-anchor.stamp<=100;
}
// Invert the existing hand/view mapping: model X right/Y forward/Z up to XR
// X right/Y up/Z back. The paired camera and XR sample cancel HMD motion;
// neither the current HMD pose nor a world-space vehicle position is a pivot.
inline bool BuildVehicleMinimapPose(const float wheel[3],const float camera[3],const float cameraRotation[4],
        const XrPosef& head,const XrPosef& recenter,float worldScale,bool bike,XrPosef& out) {
    if(!std::isfinite(worldScale) || worldScale<=0)return false;
    const XrQuaternionf mappedCamera{cameraRotation[0],cameraRotation[2],-cameraRotation[1],cameraRotation[3]};
    const auto modelToLocal=MultiplyQuat(recenter.orientation,MultiplyQuat(head.orientation,ConjugateQuat(mappedCamera)));
    // On the measured car the wheel hub is 0.48 m ahead of and 0.22 m below the eye, which puts the image 1.4 m
    // away, centred about 6 degrees below eye level.
    const auto place=MinimapPlacementFor(bike);
    const XrVector3f delta{(wheel[0]-camera[0])/worldScale,
        (wheel[2]+place.up-camera[2])/worldScale,-(wheel[1]+place.forward-camera[1])/worldScale};
    const auto offset=RotateVector(modelToLocal,delta);
    // Leaned back about the vehicle's right axis until it faces the eye: from half a metre over the bars a
    // vertical panel is read at a slant; the car's virtual image leans only a few degrees.
    const float pitch=std::atan2(-delta.y,std::max(-delta.z,1e-3f));
    const XrQuaternionf lean{std::sin(-pitch*.5f),0,0,std::cos(-pitch*.5f)};
    const auto localHead=RotateVector(recenter.orientation,head.position);
    out={MultiplyQuat(modelToLocal,lean),{recenter.position.x+localHead.x+offset.x,
        recenter.position.y+localHead.y+offset.y,recenter.position.z+localHead.z+offset.z}};
    const auto& q=out.orientation;const auto& p=out.position;
    const float norm=q.x*q.x+q.y*q.y+q.z*q.z+q.w*q.w;
    return std::isfinite(p.x+p.y+p.z+norm) && norm>.98f && norm<1.02f &&
        delta.x*delta.x+delta.y*delta.y+delta.z*delta.z<9.f;
}
}
