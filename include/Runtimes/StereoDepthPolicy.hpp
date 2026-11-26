#pragma once
#include "Framegen/Presenter.hpp"
#include <algorithm>
#include <cmath>

namespace cvr::depth {
enum class Reject : uint32_t { None, Synthetic, Missing, Identity, Projection, Camera, Queue, Resource, Busy, Runtime };
struct View {
    uint32_t width{},height{};
    float nearZ{},farZ{},jitter[2]{};
};
struct Plan { View eyes[2]; };
inline bool SamePose(const XrPosef& a,const XrPosef& b) {
    const auto& x=a.orientation;const auto& y=b.orientation;
    const float dot=x.x*y.x+x.y*y.y+x.z*y.z+x.w*y.w;
    const auto& p=a.position;const auto& q=b.position;
    return std::isfinite(dot) && std::abs(std::abs(dot)-1)<1e-5f &&
        std::abs(p.x-q.x)<1e-5f && std::abs(p.y-q.y)<1e-5f && std::abs(p.z-q.z)<1e-5f;
}
inline bool SameProjection(const framegen::CameraData& c,const XrFovf& f) {
    const float l=std::tan(f.angleLeft),r=std::tan(f.angleRight),u=std::tan(f.angleUp),d=std::tan(f.angleDown);
    const float halfY=std::tan(c.verticalFov*.5f),halfX=halfY*c.aspect;
    const auto close=[](float x,float y){return std::isfinite(x) && std::isfinite(y) &&
        std::abs(x-y)<.002f*std::max(1.f,std::abs(y));};
    // These are the same off-centre projection coefficients the engine receives.
    return r>l && u>d && close(halfX,(r-l)*.5f) && close(halfY,(u-d)*.5f) &&
        close(c.shift[0],-(r+l)/(r-l)) && close(c.shift[1],-(u+d)/(u-d));
}
inline Reject Describe(const framegen::Selection& selected,
    const XrCompositionLayerProjectionView* views,uint32_t count,float gameUnitsPerMeter,Plan& out) {
    out={};
    // SDK interpolation returns color only. Its B inputs and internal occlusion depth
    // are not depth at the synthetic image's interpolated camera/instant.
    if(selected.synthetic)return Reject::Synthetic;
    const auto& f=selected.frame;
    if(count!=2 || !views || !f.identity.stereo || !f.identity.serial || !f.inputs[0] || !f.inputs[1])return Reject::Missing;
    if(!std::isfinite(gameUnitsPerMeter) || gameUnitsPerMeter<=0)return Reject::Camera;
    if(f.inputs[0]->view==f.inputs[1]->view)return Reject::Identity;
    for(unsigned eye=0;eye<2;++eye) {
        const auto& input=*f.inputs[eye];const auto& c=input.camera;
        if(!input.submitted || !input.hasDepth || !input.depth || !input.width || !input.height)return Reject::Missing;
        if(!f.identity.origin || !f.identity.pose[eye] || input.poseId!=f.identity.pose[eye] ||
            input.origin!=f.identity.origin || input.frameId!=f.identity.nativeFrame || input.view<1 || input.view>2 ||
            !SamePose(f.pose[eye],views[eye].pose))return Reject::Identity;
        if(!SameProjection(c,views[eye].fov))return Reject::Projection;
        if(!std::isfinite(c.nearPlane) || c.nearPlane<=0 || !(c.farPlane>c.nearPlane) ||
            (!std::isfinite(c.farPlane) && !c.infinite) ||
            !std::isfinite(c.jitter[0]) || !std::isfinite(c.jitter[1]) ||
            std::abs(c.jitter[0])>2 || std::abs(c.jitter[1])>2)return Reject::Camera;
        auto& v=out.eyes[eye];v.width=input.width;v.height=input.height;
        // The game multiplies head translation by worldScale; XR uses metres.
        const float n=c.nearPlane/gameUnitsPerMeter,farClip=c.farPlane/gameUnitsPerMeter;
        v.nearZ=c.inverted?farClip:n;v.farZ=c.inverted?n:farClip;
        v.jitter[0]=c.jitter[0];v.jitter[1]=c.jitter[1];
    }
    return Reject::None;
}
}
