#pragma once
#include "Render/NativeJitter.hpp"
#include "Render/ViewCameraLayout.hpp"
#include <array>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <limits>

namespace cvr::stereo {
// The camera descriptor fields the port copies and rebuilds (904 bytes, Render/ViewCameraLayout.hpp). Game
// 2.5's matrix builder also reads fields from +0x398 on (an extra projection mode, on when bit 1 of +0x399 is
// set): the zeroed tail keeps a rebuilt copy on the usual projection and its reads inside this object.
struct alignas(16) CameraInput {
    std::array<uint8_t,904> bytes{};
    std::array<uint8_t,cvr::view_layout::kCameraStride-904> tail{};
    template<class T> T Read(size_t offset) const {T value;std::memcpy(&value,bytes.data()+offset,sizeof(value));return value;}
    template<class T> void Write(size_t offset,T value) {std::memcpy(bytes.data()+offset,&value,sizeof(value));}
};
using RebuildCameraInput=void(*)(void*);
// The peer's own projection where it differs from the source's: under the asymmetric projection each
// eye renders its own window (aspect, and an NDC shift of the centre at +0x30/+0x34), so a camera
// predicted for the other eye from this one's input must take that eye's, not this one's.
struct PeerProjection { float aspect, shiftX, shiftY; };
#if CVR_RUST_VRIK
// The rules: rust/crates/cvr-render/src/camera_input.rs (tools/single_pass_tests/peer_camera_test runs its cases
// against them in its Rust variant).
extern "C" bool cvr_cam_prepare(bool,const uint8_t*,const uint8_t*,const float(*)[3],float,const JitterSample*,uint32_t,
    uint32_t,RebuildCameraInput,uint8_t*,const PeerProjection*);
#endif
#if CVR_RUST_VRIK
inline bool PrepareCameraAtPose(const uint8_t* source,const uint8_t* pose,
    const JitterSample& jitter,uint32_t width,uint32_t height,RebuildCameraInput rebuild,CameraInput& result,
    const PeerProjection* projection=nullptr) {
    static_assert(sizeof(JitterSample)==20 && sizeof(PeerProjection)==12,"mirrored by camera_input.rs");
    return cvr_cam_prepare(true,source,pose,nullptr,0.0f,&jitter,width,height,rebuild,result.bytes.data(),projection);
}
// Construct a same-projection peer camera in private storage. The caller must
// supply the peer's native temporal sample and validate projection eligibility.
// No native camera, native history or native jitter counter is modified.
inline bool PreparePeerCamera(const uint8_t* source,const float right[3],float signedHalfIpd,
    const JitterSample& jitter,uint32_t width,uint32_t height,RebuildCameraInput rebuild,CameraInput& result,
    const PeerProjection* projection=nullptr) {
    return cvr_cam_prepare(false,source,nullptr,reinterpret_cast<const float(*)[3]>(right),signedHalfIpd,&jitter,width,height,
        rebuild,result.bytes.data(),projection);
}
#else
inline void ApplyPeerProjection(CameraInput& result,const PeerProjection* projection) {
    if(!projection || !std::isfinite(projection->aspect) || !(projection->aspect>0) ||
       !std::isfinite(projection->shiftX) || !std::isfinite(projection->shiftY))return;
    result.Write<float>(0x28,projection->aspect);
    result.Write<float>(0x30,projection->shiftX);result.Write<float>(0x34,projection->shiftY);
}
// The render view publishes this pose before preparing its camera matrices.
// Prefer the peer's own published pose to inferring its orientation from the
// other eye; camera/body updates may introduce small differences between them.
inline bool PrepareCameraAtPose(const uint8_t* source,const uint8_t* pose,
    const JitterSample& jitter,uint32_t width,uint32_t height,RebuildCameraInput rebuild,CameraInput& result,
    const PeerProjection* projection=nullptr) {
    if(!source || !pose || !rebuild || !width || !height || width>16384 || height>16384 ||
       !std::isfinite(jitter.pixelX) || !std::isfinite(jitter.pixelY))return false;
    float quaternion[4]{};std::memcpy(quaternion,pose+16,16);float norm{};
    for(float component:quaternion){if(!std::isfinite(component))return false;norm+=component*component;}
    if(norm<.99f || norm>1.01f)return false;
    std::memcpy(result.bytes.data(),source,result.bytes.size());
    std::memcpy(result.bytes.data(),pose,12);std::memcpy(result.bytes.data()+16,pose+16,16);
    for(unsigned i=0;i<3;++i)result.Write<float>(0x280+i*4,static_cast<float>(result.Read<int32_t>(i*4))/131072.0f);
    result.Write<float>(0x370,jitter.pixelX);result.Write<float>(0x374,jitter.pixelY);
    result.Write<uint32_t>(0x378,width);result.Write<uint32_t>(0x37C,height);result.Write<uint32_t>(0x380,jitter.phase);
    ApplyPeerProjection(result,projection);
    rebuild(result.bytes.data());return true;
}
// Construct a same-projection peer camera in private storage. The caller must
// supply the peer's native temporal sample and validate projection eligibility.
// No native camera, native history or native jitter counter is modified.
inline bool PreparePeerCamera(const uint8_t* source,const float right[3],float signedHalfIpd,
    const JitterSample& jitter,uint32_t width,uint32_t height,RebuildCameraInput rebuild,CameraInput& result,
    const PeerProjection* projection=nullptr) {
    if(!source || !right || !rebuild || !width || !height || width>16384 || height>16384 ||
       !std::isfinite(signedHalfIpd) || std::abs(signedHalfIpd)>.15f)return false;
    float norm{};for(unsigned i=0;i<3;++i){if(!std::isfinite(right[i]))return false;norm+=right[i]*right[i];}
    if(norm<.99f || norm>1.01f || !std::isfinite(jitter.pixelX) || !std::isfinite(jitter.pixelY))return false;
    std::memcpy(result.bytes.data(),source,result.bytes.size());
    for(unsigned i=0;i<3;++i){
        // Match PatchCamera's two symmetric truncations in fixed-point units.
        volatile float half=right[i]*signedHalfIpd;
        volatile float fixed=half*131072.0f;
        const auto delta=2*static_cast<int64_t>(static_cast<int32_t>(fixed));
        const auto position=int64_t(result.Read<int32_t>(i*4))+delta;
        if(position<std::numeric_limits<int32_t>::min() || position>std::numeric_limits<int32_t>::max())return false;
        result.Write<int32_t>(i*4,static_cast<int32_t>(position));
        result.Write<float>(0x280+i*4,static_cast<float>(position)/131072.0f);
    }
    result.Write<float>(0x370,jitter.pixelX);result.Write<float>(0x374,jitter.pixelY);
    result.Write<uint32_t>(0x378,width);result.Write<uint32_t>(0x37C,height);result.Write<uint32_t>(0x380,jitter.phase);
    ApplyPeerProjection(result,projection);
    rebuild(result.bytes.data());
    return true;
}
#endif
}
