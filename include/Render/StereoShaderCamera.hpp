#pragma once
#include "Render/StereoCameraInput.hpp"
#include <xmmintrin.h>

#if defined(_MSC_VER)
#pragma float_control(precise, on, push)
#pragma fp_contract(off)
#endif
namespace cvr::stereo {
using InvertShaderMatrix=void*(*)(const void*,void*);
using ShaderCamera=std::array<uint8_t,848>;
struct ShaderCameraHistory {std::array<uint8_t,0x184> bytes{};};
namespace camera_math {
struct alignas(16) Matrix { float v[4][4]; };
inline Matrix Load(const uint8_t* data,size_t offset) {
    Matrix result;std::memcpy(&result,data+offset,sizeof(result));return result;
}
inline Matrix Transpose(const Matrix& a) {
    Matrix result{};
    for(unsigned r=0;r<4;++r)for(unsigned c=0;c<4;++c)result.v[r][c]=a.v[c][r];
    return result;
}
inline Matrix Multiply(const Matrix& a,const Matrix& b) {
    Matrix result{};
    // Match the native helper's float rounding, including the first add order.
    for(unsigned row=0;row<4;++row) {
        auto value=_mm_add_ps(_mm_mul_ps(_mm_set1_ps(a.v[row][1]),_mm_load_ps(b.v[1])),
                              _mm_mul_ps(_mm_set1_ps(a.v[row][0]),_mm_load_ps(b.v[0])));
        value=_mm_add_ps(value,_mm_mul_ps(_mm_set1_ps(a.v[row][2]),_mm_load_ps(b.v[2])));
        value=_mm_add_ps(value,_mm_mul_ps(_mm_set1_ps(a.v[row][3]),_mm_load_ps(b.v[3])));
        _mm_store_ps(result.v[row],value);
    }
    return result;
}
inline void Relative(Matrix& value) {
    value.v[3][0]=value.v[3][1]=value.v[3][2]=0;value.v[3][3]=1;
}
inline bool Finite(const Matrix& value) {
    for(const auto& row:value.v)for(float f:row)if(!std::isfinite(f))return false;
    return true;
}
}

#if CVR_RUST_VRIK
// The rules: rust/crates/cvr-render/src/shader_camera.rs (tools/single_pass_tests/shader_camera_test runs its cases against
// them in its Rust variant), with the native matrix helper's float rounding. camera_math stays for the fixtures.
extern "C" {
bool cvr_scam_build(const ShaderCamera*,const uint8_t*,const uint8_t*,size_t,float,InvertShaderMatrix,ShaderCamera*);
bool cvr_scam_decode(const uint8_t*,size_t,const ShaderCameraHistory*,uint8_t*,float*);
}
// Same-projection, perspective, camera-relative scene constants. Shared view
// metadata must already be eligible; previous data and temporal weight belong
// to the PEER eye. Never substitute the first eye's temporal history.
// The matrix callback is pure numeric work on owned buffers, not a GPU upload.
inline bool BuildPeerShaderCamera(const ShaderCamera& shared,const CameraInput& current,
    const CameraInput& previous,float temporalWeight,InvertShaderMatrix invert,ShaderCamera& output) {
    return cvr_scam_build(&shared,current.bytes.data(),previous.bytes.data(),current.bytes.size(),temporalWeight,invert,&output);
}
// The paired graph has already prepared both current cameras. Its history
// object is separate from camera +0x70: native 0x297904 reads pose at +8/+0x20,
// view/projection at +0x30/+0xB0 and the temporal weight at +0x180.
inline bool DecodeShaderCameraHistory(const CameraInput& current,const ShaderCameraHistory& history,
    CameraInput& previous,float& temporalWeight){
    CameraInput result=current;
    if(!cvr_scam_decode(current.bytes.data(),current.bytes.size(),&history,result.bytes.data(),&temporalWeight))return false;
    previous=result;return true;
}
#else
// Same-projection, perspective, camera-relative scene constants. Shared view
// metadata must already be eligible; previous data and temporal weight belong
// to the PEER eye. Never substitute the first eye's temporal history.
// The matrix callback is pure numeric work on owned buffers, not a GPU upload.
inline bool BuildPeerShaderCamera(const ShaderCamera& shared,const CameraInput& current,
    const CameraInput& previous,float temporalWeight,InvertShaderMatrix invert,ShaderCamera& output) {
    using namespace camera_math;
    const auto width=current.Read<uint32_t>(0x378),height=current.Read<uint32_t>(0x37C);
    if(!invert || !width || !height || width>16384 || height>16384 ||
       !std::isfinite(temporalWeight) || temporalWeight<0 ||
       !(current.Read<float>(0x20)>0) || !(previous.Read<float>(0x20)>0) ||
       !(current.Read<float>(0x40)>0) || !(current.Read<float>(0x44)>current.Read<float>(0x40)) ||
       !(current.Read<uint8_t>(0x384)&4) || !(previous.Read<uint8_t>(0x384)&4) ||
       previous.Read<uint32_t>(0x378)!=width || previous.Read<uint32_t>(0x37C)!=height)return false;
    auto view=Load(current.bytes.data(),0x50),projection=Load(current.bytes.data(),0x2F0);
    auto previousView=Load(previous.bytes.data(),0x50),previousProjection=Load(previous.bytes.data(),0x2F0);
    const auto vp=Transpose(Load(current.bytes.data(),0x2B0));
    if(!Finite(view) || !Finite(projection) || !Finite(previousView) || !Finite(previousProjection) || !Finite(vp))return false;
    Matrix inverse{};invert(&vp,&inverse);if(!Finite(inverse))return false;
    ShaderCamera result=shared;
    auto put=[&](size_t offset,const void* source,size_t count){std::memcpy(result.data()+offset,source,count);};
    auto putMatrix=[&](size_t offset,const Matrix& matrix){put(offset,&matrix,sizeof(matrix));};
    putMatrix(0,vp);putMatrix(64,inverse);putMatrix(128,Transpose(projection));
    // The native history's projection excludes the previous temporal jitter.
    previousProjection.v[2][0]=previousProjection.v[2][1]=0;
    putMatrix(192,Transpose(Multiply(previousView,previousProjection)));
    Relative(previousView);
    Matrix translation{};for(unsigned i=0;i<4;++i)translation.v[i][i]=1;
    float world[4]{0,0,0,1},oldWorld[4]{0,0,0,1};int32_t fixed[4]{};
    for(unsigned i=0;i<3;++i) {
        const auto now=current.Read<int32_t>(i*4),old=previous.Read<int32_t>(i*4);
        const auto delta=int64_t(now)-old;
        if(delta<std::numeric_limits<int32_t>::min() || delta>std::numeric_limits<int32_t>::max())return false;
        translation.v[3][i]=static_cast<float>(delta)/131072.0f;
        world[i]=static_cast<float>(now)/131072.0f;oldWorld[i]=static_cast<float>(old)/131072.0f;fixed[i]=now;
    }
    putMatrix(256,Transpose(Multiply(Multiply(translation,previousView),previousProjection)));
    putMatrix(320,Transpose(view));Relative(view);putMatrix(384,Transpose(view));
    putMatrix(448,Transpose(Multiply(view,projection)));
    putMatrix(512,Transpose(Load(current.bytes.data(),0x110)));
    put(576,world,12);put(588,&temporalWeight,4);put(592,world,16);put(608,fixed,16);put(624,world,16);
    put(640,current.bytes.data()+0x250,48);put(688,oldWorld,16);put(704,previous.bytes.data()+0x250,48);
    put(800,current.bytes.data()+0x290,16);
    const float jitter[2]{2.0f*current.Read<float>(0x370)/static_cast<float>(width),
                          2.0f*current.Read<float>(0x374)/static_cast<float>(height)};
    if(!std::isfinite(jitter[0]) || !std::isfinite(jitter[1]))return false;
    put(816,jitter,8);put(824,current.bytes.data()+0x380,4);
    output=result;return true;
}

// The paired graph has already prepared both current cameras. Its history
// object is separate from camera +0x70: native 0x297904 reads pose at +8/+0x20,
// view/projection at +0x30/+0xB0 and the temporal weight at +0x180.
inline bool DecodeShaderCameraHistory(const CameraInput& current,const ShaderCameraHistory& history,
    CameraInput& previous,float& temporalWeight){
    if(!history.bytes[0])return false;
    float weight{};std::memcpy(&weight,history.bytes.data()+0x180,4);
    const auto view=camera_math::Load(history.bytes.data(),0x30),projection=camera_math::Load(history.bytes.data(),0xB0);
    if(!std::isfinite(weight) || weight<0 || !camera_math::Finite(view) || !camera_math::Finite(projection) ||
        projection.v[2][0]!=0 || projection.v[2][1]!=0)return false;
    CameraInput result=current;
    std::memcpy(result.bytes.data(),history.bytes.data()+8,12);
    std::memcpy(result.bytes.data()+16,history.bytes.data()+0x20,16);
    std::memcpy(result.bytes.data()+0x50,&view,64);
    std::memcpy(result.bytes.data()+0x2F0,&projection,64);
    constexpr unsigned axes[]{0,2,1};
    for(unsigned axis=0;axis<3;++axis){
        const float direction[]{view.v[0][axes[axis]],view.v[1][axes[axis]],view.v[2][axes[axis]],0};
        std::memcpy(result.bytes.data()+0x250+axis*16,direction,16);
    }
    previous=result;temporalWeight=weight;return true;
}
#endif
}
#if defined(_MSC_VER)
#pragma float_control(pop)
#endif
