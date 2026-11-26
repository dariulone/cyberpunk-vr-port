#pragma once
#include "Stereo/RenderParity.hpp"
#include <algorithm>
#include <cstring>

namespace cvr::stereo {
using FogCamera = std::array<float,212>;
using FogProjection = std::array<float,16>;
using FogGrid = std::array<float,16>;
#if CVR_RUST_VRIK
// The rules: rust/crates/cvr-render/src/fog_history.rs through render_parity_abi.rs (tools/render_parity_tests' fog cases
// run against them in their Rust variant).
extern "C" {
bool cvr_fog_read_grid(const void*,FogGrid*);
bool cvr_fog_same(uint32_t,const std::array<float,16>*,const std::array<float,16>*);
bool cvr_fog_projection(uint32_t,const FogCamera*,FogProjection*);
bool cvr_fog_apply_history(const FogCamera*,const FogProjection*,FogCamera*);
bool cvr_fog_can_borrow(const FogSample*,const RenderOwner*,uintptr_t,uint32_t,uint64_t,uint64_t,const FogAllocation*,
    const FogAllocation*,const FogAllocation*,const FogGrid*,const FogGrid*);
}
inline bool ReadFogGrid(const void* constants,FogGrid& out){return cvr_fog_read_grid(constants,&out);}
inline bool SameFogGrid(const FogGrid& a,const FogGrid& b){return cvr_fog_same(0,&a,&b);}
// camera[51].xy: the NDC jitter alone (the native VS removes it this way, see lens_velocity_ps.hlsl).
// Anything larger than this is not a sub-pixel jitter.
constexpr unsigned kFogCameraJitter=204;
constexpr float kFogMaxJitter=0.05f;
// The projection alone with the jitter taken out: the frustum the view's froxel grid spans.
inline bool FogFrustum(const FogCamera& camera,FogProjection& output){return cvr_fog_projection(0,&camera,&output);}
// The projection with the jitter subtracted and the off-centre shift kept, times the view.
inline bool FogCurrentProjection(const FogCamera& camera,FogProjection& output){return cvr_fog_projection(1,&camera,&output);}
// MAIN's fog history stands in for the second view's only when both froxel grids span the same frustum.
inline bool SameFogFrustum(const FogProjection& a,const FogProjection& b){return cvr_fog_same(1,&a,&b);}
inline bool ApplyFogHistoryProjection(const FogCamera& native,const FogProjection& source,FogCamera& output){
    return cvr_fog_apply_history(&native,&source,&output);
}
inline bool CanBorrowFog(const FogSample& sample,const RenderOwner& owner,uintptr_t registry,
    uint32_t frame,uint64_t pose,uint64_t time,const FogAllocation& live,
    const FogAllocation& scatteringOutput,const FogAllocation& integratedOutput,
    const FogGrid& sourceGrid,const FogGrid& currentGrid) {
    return cvr_fog_can_borrow(&sample,&owner,registry,frame,pose,time,&live,&scatteringOutput,&integratedOutput,&sourceGrid,&currentGrid);
}
#else
inline bool ReadFogGrid(const void* constants,FogGrid& out) {
    if(!constants)return false;
    const auto* bytes=static_cast<const unsigned char*>(constants);
    FogGrid value{};
    std::memcpy(value.data(),bytes+25*16,32);
    std::memcpy(value.data()+8,bytes+43*16,32);
    for(float f:value)if(!std::isfinite(f))return false;
    for(unsigned i=9;i<12;++i)if(value[i]<1 || value[i]>2048)return false;
    for(unsigned i=12;i<16;++i)if(value[i]<=0)return false;
    out=value;return true;
}
inline bool SameFogGrid(const FogGrid& a,const FogGrid& b) {
    for(unsigned i=0;i<a.size();++i)
        if(!std::isfinite(a[i]) || !std::isfinite(b[i]) ||
           std::abs(a[i]-b[i])>1e-5f*std::max(1.0f,std::max(std::abs(a[i]),std::abs(b[i]))))return false;
    return true;
}
// camera[51].xy: the NDC jitter alone (the native VS removes it this way, see lens_velocity_ps.hlsl).
// Anything larger than this is not a sub-pixel jitter.
constexpr unsigned kFogCameraJitter=204;
constexpr float kFogMaxJitter=0.05f;
// The projection alone with the jitter taken out: the frustum the view's froxel grid spans.
inline bool FogFrustum(const FogCamera& camera,FogProjection& output);
inline bool FogCurrentProjection(const FogCamera& camera,FogProjection& output) {
    // CameraShaderConsts stores transposed matrices. Fog froxels exclude the
    // temporal projection jitter, as does the native previous-VP matrix -- but
    // NOT the projection's own off-centre shift (Stereo/AsymProjection.cpp),
    // which P20/P21 (p[2], p[6]) carry next to the jitter. Subtract the jitter
    // and keep the shift: zeroing P20/P21 dropped the shift as well, and the
    // second eye then read MAIN's fog history one shift away -- the eyes
    // disagreed about the haze and glare around the sun (2026-10-15).
    const float* projection=camera.data()+32;
    const float* view=camera.data()+80;
    const float* jitter=camera.data()+kFogCameraJitter;
    for(unsigned i=0;i<16;++i)
        if(!std::isfinite(projection[i]) || !std::isfinite(view[i]))return false;
    if(projection[0]<=0 || projection[5]<=0 || std::abs(projection[14]-1)>1e-4f ||
       std::abs(projection[15])>1e-4f || std::abs(view[15]-1)>1e-4f)return false;
    if(!std::isfinite(jitter[0]) || !std::isfinite(jitter[1]) ||
       std::abs(jitter[0])>kFogMaxJitter || std::abs(jitter[1])>kFogMaxJitter)return false;
    FogProjection p{};
    if(!FogFrustum(camera,p))return false;
    FogProjection result{};
    for(unsigned r=0;r<4;++r)for(unsigned c=0;c<4;++c) {
        float sum{};
        for(unsigned k=0;k<4;++k)sum+=p[r*4+k]*view[k*4+c];
        if(!std::isfinite(sum))return false;
        result[r*4+c]=sum;
    }
    output=result;return true;
}
inline bool FogFrustum(const FogCamera& camera,FogProjection& output) {
    const float* projection=camera.data()+32;
    const float* jitter=camera.data()+kFogCameraJitter;
    for(unsigned i=0;i<16;++i)if(!std::isfinite(projection[i]))return false;
    if(!std::isfinite(jitter[0]) || !std::isfinite(jitter[1]) ||
       std::abs(jitter[0])>kFogMaxJitter || std::abs(jitter[1])>kFogMaxJitter)return false;
    auto p=FogProjection{};std::copy_n(projection,16,p.begin());
    p[2]-=jitter[0];p[6]-=jitter[1];
    output=p;return true;
}
// MAIN's fog history stands in for the second view's only when both froxel grids span the same
// frustum. SameFogGrid compares the depth slicing alone; with each eye its own window (asymmetric
// projection, inner guard 0) the grids cover different frusta, and the borrowed history -- resampled
// across them -- made the second eye's haze differ from MAIN's instead of agreeing with it (measured:
// eyes 2.08 apart with the borrow, 1.05 without, at the reported sun/haze spot).
inline bool SameFogFrustum(const FogProjection& a,const FogProjection& b) {
    for(unsigned i=0;i<a.size();++i)
        if(!std::isfinite(a[i]) || !std::isfinite(b[i]) ||
           std::abs(a[i]-b[i])>1e-4f*std::max(1.0f,std::max(std::abs(a[i]),std::abs(b[i]))))return false;
    return true;
}
inline bool ApplyFogHistoryProjection(const FogCamera& native,const FogProjection& source,FogCamera& output) {
    FogProjection current{};
    if(!FogCurrentProjection(native,current))return false;
    for(float value:source)if(!std::isfinite(value))return false;
    output=native;
    std::copy(source.begin(),source.end(),output.begin()+48);
    return true;
}
inline bool CanBorrowFog(const FogSample& sample,const RenderOwner& owner,uintptr_t registry,
    uint32_t frame,uint64_t pose,uint64_t time,const FogAllocation& live,
    const FogAllocation& scatteringOutput,const FogAllocation& integratedOutput,
    const FogGrid& sourceGrid,const FogGrid& currentGrid) {
    return sample.Eligible(owner,registry,frame,pose,time) && live==sample.allocation &&
        scatteringOutput && integratedOutput && live.resource!=scatteringOutput.resource &&
        live.resource!=integratedOutput.resource && SameFogGrid(sourceGrid,currentGrid);
}
#endif
void ObserveFogConstants(uint32_t size,const void* source);
}
