#pragma once
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstring>

namespace cvr::markers {
// Flat ink vertices normally have z=0. Carry a bounded inverse forward depth
// through the shared UI mesh; the replacement VS removes it before projection.
#if CVR_RUST_VRIK
// The arithmetic: rust/crates/cvr-render/src/world_marker_math.rs through world_marker_abi.rs (tools/world_marker_tests
// runs its cases against it in its Rust variant).
extern "C" {
float cvr_wm_depth(uint32_t,float);
float cvr_wm_text_matrix_depth(const float*,size_t);
void cvr_wm_reproject_text_matrix(float*,float);
float cvr_wm_forward_depth(const float*,const float*,const float*);
float cvr_wm_stereo_scale(float,float,float,float,bool);
bool cvr_wm_is_ink_matrix(const float*,float,float);
bool cvr_wm_is_ink_camera(const void*,size_t);
int cvr_wm_ink_camera_eye(const void*);
int cvr_wm_shift_ink_camera(float*,const float*,size_t,float,float,float,float);
}
// Flat ink vertices normally have z=0. Carry a bounded inverse forward depth
// through the shared UI mesh; the replacement VS removes it before projection.
inline float EncodeDepth(float inverseDepth){return cvr_wm_depth(0,inverseDepth);}
inline float DecodeDepth(float value){return cvr_wm_depth(1,value);}
inline float TextMatrixDepth(const float* p,size_t bytes){return cvr_wm_text_matrix_depth(p,bytes);}
inline void ReprojectTextMatrix(float* p,float pixelsPerInverseDepth){cvr_wm_reproject_text_matrix(p,pixelsPerInverseDepth);}
inline float ForwardDepth(const float point[3], const float eye[3], const float q[4]){return cvr_wm_forward_depth(point,eye,q);}
inline float StereoScale(float ipd, float verticalFovDeg, float aspect, float zoom, bool mainRight){
    return cvr_wm_stereo_scale(ipd,verticalFovDeg,aspect,zoom,mainRight);
}
// The flat ink camera's matrix for a w x h canvas, exactly as the engine writes it.
inline bool IsInkMatrix(const float* m, float w, float h){return cvr_wm_is_ink_matrix(m,w,h);}
#else
inline float EncodeDepth(float inverseDepth) {
    return std::isfinite(inverseDepth) && inverseDepth > 0 && inverseDepth <= 20
        ? -65536.0f - 4096.0f * inverseDepth : 0.0f;
}
inline float DecodeDepth(float value) {
    return value < -65536.0f && value >= -147456.0f
        ? (-value - 65536.0f) / 4096.0f : 0.0f;
}
inline float TextMatrixDepth(const float* p,size_t bytes) {
    if(!p || bytes!=112 || p[8]!=0 || p[9]!=0 || p[10]!=1 || p[15]!=1) return 0;
    return DecodeDepth(p[11]);
}
inline void ReprojectTextMatrix(float* p,float pixelsPerInverseDepth) {
    const float depth=TextMatrixDepth(p,112);
    if(!depth) return;
    p[11]=0;
    if(std::isfinite(pixelsPerInverseDepth)) p[3]+=depth*pixelsPerInverseDepth;
}
inline float ForwardDepth(const float point[3], const float eye[3], const float q[4]) {
    const double norm = double(q[0])*q[0]+double(q[1])*q[1]+double(q[2])*q[2]+double(q[3])*q[3];
    if (!std::isfinite(norm) || norm < .99 || norm > 1.01) return 0;
    // Game camera axes: X right, Y forward, Z up.
    const float forward[3] = {2*(q[0]*q[1]-q[2]*q[3]),
        1-2*(q[0]*q[0]+q[2]*q[2]), 2*(q[1]*q[2]+q[0]*q[3])};
    float depth=0;
    for (int i=0;i<3;++i) depth+=(point[i]-eye[i])*forward[i];
    return std::isfinite(depth) && depth >= .05f ? depth : 0;
}
inline float StereoScale(float ipd, float verticalFovDeg, float aspect, float zoom, bool mainRight) {
    if (!std::isfinite(ipd) || ipd <= 0 || ipd > .5f ||
        !std::isfinite(verticalFovDeg) || verticalFovDeg <= 1 || verticalFovDeg >= 170 ||
        !std::isfinite(aspect) || aspect <= 0 || !std::isfinite(zoom) || zoom <= 0) return 0;
    const float scale = ipd * zoom / (std::tan(verticalFovDeg * .00872664626f) * aspect);
    return mainRight ? scale : -scale;
}
// The flat ink camera's matrix for a w x h canvas, exactly as the engine writes it.
inline bool IsInkMatrix(const float* m, float w, float h) {
    const float expected[16]={2/w,0,0,-1, 0,-2/h,0,1, 0,0,1,0, 0,0,0,1};
    for(int i=0;i<16;++i)
        if(!std::isfinite(m[i]) || std::abs(m[i]-expected[i])>1e-7f) return false;
    return true;
}
#endif
// The camera block as 2.5 uploads it (sub_14228C9A0 -> sub_140334328, size 0x360): CameraShaderConsts grew from
// 848 to 852 bytes by the eye the pass draws, an int at byte 848 -- 0 a one-view frame, 1 left / 2 right in the
// engine's own stereo frame (render feature 98) -- and is uploaded rounded to 864.
inline constexpr uint32_t kInkCameraBytes=864;
inline constexpr size_t kInkCameraEye=848;
// Identify the game's flat ink camera, never a perspective/shadow/world camera.
#if CVR_RUST_VRIK
inline bool IsInkCamera(const void* data, size_t bytes){return cvr_wm_is_ink_camera(data,bytes);}
inline int InkCameraEye(const void* data){return cvr_wm_ink_camera_eye(data);}
#else
inline bool IsInkCamera(const void* data, size_t bytes) {
    if (!data || bytes != kInkCameraBytes) return false;
    const auto* p=static_cast<const float*>(data);
    const float w=p[188],h=p[189];
    if (!(w>=640 && w<=16384 && h>=360 && h<=16384)) return false;
    return IsInkMatrix(p,w,h);
}
inline int InkCameraEye(const void* data) {
    int eye=0;
    std::memcpy(&eye,static_cast<const uint8_t*>(data)+kInkCameraEye,sizeof(eye));
    return eye;
}
#endif
// Squeezes clip x about the canvas centre and moves the centre to (shiftX, shiftY), in the camera's
// first matrix AND in every other copy of the canvas matrix the block carries. The engine writes that
// matrix five times (floats 0, 32, 48, 64 and 112; measured on the 2.31 HUD canvas). Ink's own sprite
// and text shaders read the first; the video widget's Bink material reads one of the others, so with
// only the first moved a tutorial popup's video stayed at the centred-frustum pixels in both eyes
// while its panel moved: doubled, and over the right eye's prompt. `content` is the width the layout fills from
// the canvas's left edge when it is narrower than the camera's canvas (Stereo/EngineStereo.hpp UiOneEye: one
// eye's of a camera two eyes wide); the squeeze and the move then apply to that. Returns the copies moved.
#if CVR_RUST_VRIK
inline int ShiftInkCamera(float* out, const float* source, size_t bytes, float squeeze, float shiftX, float shiftY,
                          float content=0.0f) {
    return cvr_wm_shift_ink_camera(out,source,bytes,squeeze,shiftX,shiftY,content);
}
#else
inline int ShiftInkCamera(float* out, const float* source, size_t bytes, float squeeze, float shiftX, float shiftY,
                          float content=0.0f) {
    const float w=source[188],h=source[189];
    const float wide=content>0.0f && content<w ? w/content : 1.0f;
    int copies=0;
    for(size_t m=0;m+16<=bytes/sizeof(float);m+=16) {
        if(m && !IsInkMatrix(source+m,w,h)) continue;
        out[m]*=squeeze*wide;out[m+3]=out[m+3]*squeeze*wide+squeeze*(wide-1.0f)+shiftX;out[m+7]+=shiftY;
        if(m) ++copies;
    }
    return copies;
}
#endif
}
