#pragma once
#include <cmath>
#include <cstdint>
#include <cstring>

namespace cvr::markers {
struct SourceProjection {
    float origin[3]{},right[3]{},up[3]{},forward[3]{};
    float p00{},p11{},p20{},p21{},width{},height{};
    bool Valid() const {return width>0 && height>0 && p00>0 && p11>0;}
};
inline constexpr float kWorldCameraMagic=538012.0f;
inline constexpr float kWorldTextMagic=-16384.0f;
inline constexpr uint32_t kWorldCameraBytes=944;
struct WorldMarkerCamera {
    float origin[4]{},right[4]{},up[4]{},forward[4]{},projection[4]{};
};
static_assert(sizeof(SourceProjection)==72 && sizeof(WorldMarkerCamera)==80);

#if CVR_RUST_VRIK
// The arithmetic: rust/crates/cvr-render/src/world_marker_math.rs through world_marker_abi.rs (tools/world_marker_tests
// runs its cases against it in its Rust variant).
struct MarkerAnchor;
extern "C" {
bool cvr_wm_read_projection(const uint8_t*,float,float,SourceProjection*,bool);
float cvr_wm_source_depth(const SourceProjection*,const float*);
bool cvr_wm_canvas_to_world(bool,const SourceProjection*,const MarkerAnchor*,float,float,float,float*);
bool cvr_wm_anchor_of(const SourceProjection*,const float*,MarkerAnchor*);
void cvr_wm_level_axes(const SourceProjection*,float*,float*);
bool cvr_wm_text_translation_exact(float*,float,float);
bool cvr_wm_text_to_world(float*,const SourceProjection*,float,const MarkerAnchor*);
void cvr_wm_marker_camera(const SourceProjection*,WorldMarkerCamera*);
}
inline bool ReadProjection(const uint8_t* camera,float width,float height,SourceProjection& out,bool unjitter=false){
    return cvr_wm_read_projection(camera,width,height,&out,unjitter);
}
inline float SourceDepth(const SourceProjection& source,const float point[3]){return cvr_wm_source_depth(&source,point);}
inline bool CanvasToWorld(const SourceProjection& source,float inverseDepth,float x,float y,float world[3]){
    return cvr_wm_canvas_to_world(false,&source,nullptr,inverseDepth,x,y,world);
}
#else
inline bool ReadProjection(const uint8_t* camera,float width,float height,SourceProjection& out,bool unjitter=false) {
    if(!camera || !std::isfinite(width) || !std::isfinite(height) || width<1 || height<1 || width>16384 || height>16384)return false;
    float q[4],p[16];int32_t position[3];
    std::memcpy(q,camera+0x10,sizeof(q));std::memcpy(p,camera+0x190,sizeof(p));std::memcpy(position,camera,sizeof(position));
    double norm=0;
    for(float v:q){if(!std::isfinite(v))return false;norm+=double(v)*v;}
    if(norm<.99 || norm>1.01 || !std::isfinite(p[0]) || !std::isfinite(p[5]) ||
       p[0]<=0 || p[5]<=0 || std::abs(p[11]-1)>1e-5f || p[15]!=0)return false;
    const float x=q[0],y=q[1],z=q[2],w=q[3];
    SourceProjection value;
    for(int i=0;i<3;++i)value.origin[i]=float(double(position[i])/131072.0);
    const float r[]={1-2*(y*y+z*z),2*(x*y+z*w),2*(x*z-y*w)};
    const float f[]={2*(x*y-z*w),1-2*(x*x+z*z),2*(y*z+x*w)};
    const float u[]={2*(x*z+y*w),2*(y*z-x*w),1-2*(x*x+y*y)};
    std::memcpy(value.right,r,sizeof(r));std::memcpy(value.forward,f,sizeof(f));std::memcpy(value.up,u,sizeof(u));
    value.p00=p[0];value.p11=p[5];value.p20=p[8];value.p21=p[9];value.width=width;value.height=height;
    if(unjitter) {
        float jitter[2];uint32_t dimensions[2];
        std::memcpy(jitter,camera+0x370,sizeof(jitter));std::memcpy(dimensions,camera+0x378,sizeof(dimensions));
        if(dimensions[0]>1 && dimensions[1]>1) {
            value.p20-=2*jitter[0]/dimensions[0];value.p21-=2*jitter[1]/dimensions[1];
        }
    }
    if(!std::isfinite(value.p20) || !std::isfinite(value.p21))return false;
    out=value;return true;
}
inline float SourceDepth(const SourceProjection& source,const float point[3]) {
    double depth=0;
    for(int i=0;i<3;++i)depth+=(double(point[i])-source.origin[i])*source.forward[i];
    return std::isfinite(depth) && depth>=.05 ? float(depth) : 0;
}
inline bool CanvasToWorld(const SourceProjection& source,float inverseDepth,float x,float y,float world[3]) {
    if(!source.Valid() || !std::isfinite(inverseDepth) || inverseDepth<=0 || inverseDepth>20 || !std::isfinite(x) || !std::isfinite(y))return false;
    const double vx=((2.0*x/source.width-1)-source.p20)/source.p00;
    const double vy=((1-2.0*y/source.height)-source.p21)/source.p11;
    for(int i=0;i<3;++i)world[i]=float(source.origin[i]+(source.forward[i]+vx*source.right[i]+vy*source.up[i])/inverseDepth);
    return std::isfinite(world[0]) && std::isfinite(world[1]) && std::isfinite(world[2]);
}
#endif
// A marker's world point with the canvas pixel the source camera puts it on: the point its geometry hangs from.
struct MarkerAnchor { float world[3]{},canvas[2]{}; bool valid{}; };
#if CVR_RUST_VRIK
static_assert(sizeof(MarkerAnchor)==24,"mirrored by world_marker_math.rs");
inline bool AnchorOf(const SourceProjection& source,const float point[3],MarkerAnchor& anchor){return cvr_wm_anchor_of(&source,point,&anchor);}
// The source's picture plane without its roll: right level with the world (game Z up), up across it and the
// forward. Straight up or down there is no level right, and the source's own axes stay.
inline void LevelAxes(const SourceProjection& source,float right[3],float up[3]){cvr_wm_level_axes(&source,right,up);}
// THE MARKER HANGS LEVEL FROM ITS POINT (CanvasToWorldLevel): the point stays exact; the pixels around it go out
// along the level axes, at its depth.
inline bool CanvasToWorldLevel(const SourceProjection& source,const MarkerAnchor& anchor,float inverseDepth,float x,float y,float world[3]){
    return cvr_wm_canvas_to_world(true,&source,&anchor,inverseDepth,x,y,world);
}
inline bool TextTranslationExact(float* matrix,float x,float y){return cvr_wm_text_translation_exact(matrix,x,y);}
inline bool TextToWorld(float* matrix,const SourceProjection& source,float inverseDepth,const MarkerAnchor* anchor=nullptr){
    return cvr_wm_text_to_world(matrix,&source,inverseDepth,anchor);
}
inline WorldMarkerCamera MarkerCamera(const SourceProjection& eye){WorldMarkerCamera result;cvr_wm_marker_camera(&eye,&result);return result;}
#else
inline bool AnchorOf(const SourceProjection& source,const float point[3],MarkerAnchor& anchor) {
    anchor={};
    if(!source.Valid())return false;
    double v[3]{};
    for(int i=0;i<3;++i) {
        const double d=double(point[i])-source.origin[i];
        v[0]+=d*source.right[i];v[1]+=d*source.up[i];v[2]+=d*source.forward[i];
    }
    if(!std::isfinite(v[2]) || v[2]<.05)return false;
    anchor.canvas[0]=float((v[0]/v[2]*source.p00+source.p20+1)*source.width*.5);
    anchor.canvas[1]=float((1-v[1]/v[2]*source.p11-source.p21)*source.height*.5);
    std::memcpy(anchor.world,point,sizeof(anchor.world));
    anchor.valid=std::isfinite(anchor.canvas[0]) && std::isfinite(anchor.canvas[1]);
    return anchor.valid;
}
// The source's picture plane without its roll: right level with the world (game Z up), up across it and the
// forward. Straight up or down there is no level right, and the source's own axes stay.
inline void LevelAxes(const SourceProjection& source,float right[3],float up[3]) {
    const float* f=source.forward;
    const double length=std::sqrt(double(f[0])*f[0]+double(f[1])*f[1]);
    if(!(length>1e-3)) {std::memcpy(right,source.right,12);std::memcpy(up,source.up,12);return;}
    right[0]=float(f[1]/length);right[1]=float(-f[0]/length);right[2]=0;
    up[0]=right[1]*f[2]-right[2]*f[1];up[1]=right[2]*f[0]-right[0]*f[2];up[2]=right[0]*f[1]-right[1]*f[0];
}
// THE MARKER HANGS LEVEL FROM ITS POINT. The canvas is laid out in the source camera's picture, roll included:
// lifted pixel by pixel, a marker turned with the head's roll and its offset from the point (an NPC icon above the
// head) swung round it. The point stays exact; the pixels around it go out along the level axes, at its depth.
inline bool CanvasToWorldLevel(const SourceProjection& source,const MarkerAnchor& anchor,float inverseDepth,float x,float y,float world[3]) {
    if(!anchor.valid || !source.Valid() || !std::isfinite(inverseDepth) || inverseDepth<=0 || inverseDepth>20 ||
       !std::isfinite(x) || !std::isfinite(y))return false;
    float right[3],up[3];LevelAxes(source,right,up);
    const double vx=2.0*(x-anchor.canvas[0])/(source.width*source.p00);
    const double vy=-2.0*(y-anchor.canvas[1])/(source.height*source.p11);
    for(int i=0;i<3;++i)world[i]=float(anchor.world[i]+(vx*right[i]+vy*up[i])/inverseDepth);
    return std::isfinite(world[0]) && std::isfinite(world[1]) && std::isfinite(world[2]);
}
// The text builder (sub_1422F63D0) rounds the matrix's translation to the canvas pixel while the HUD snaps
// (dword_143944288); lifted into the world, that pixel is a step against it when the head moves. The record's own
// translation goes back where the builder's rounds from it.
inline bool TextTranslationExact(float* matrix,float x,float y) {
    if(!matrix || !std::isfinite(x) || !std::isfinite(y) || std::fabs(matrix[12]-x)>.5001f || std::fabs(matrix[13]-y)>.5001f)
        return false;
    matrix[12]=x;matrix[13]=y;return true;
}
// Native text geometry stores a column-major affine matrix; the draw publishes
// its first three rows. The unused local-Z coefficient marks the lifted matrix.
// With a valid anchor the glyphs hang level from it (CanvasToWorldLevel).
inline bool TextToWorld(float* matrix,const SourceProjection& source,float inverseDepth,const MarkerAnchor* anchor=nullptr) {
    if(!matrix || matrix[2]!=0 || matrix[6]!=0 || matrix[10]!=1 || matrix[14]!=0 ||
       matrix[3]!=0 || matrix[7]!=0 || matrix[11]!=0 || matrix[15]!=1)return false;
    const bool level=anchor && anchor->valid;
    float origin[3];
    if(level ? !CanvasToWorldLevel(source,*anchor,inverseDepth,matrix[12],matrix[13],origin)
             : !CanvasToWorld(source,inverseDepth,matrix[12],matrix[13],origin))return false;
    float right[3],up[3];
    if(level)LevelAxes(source,right,up);
    else {std::memcpy(right,source.right,12);std::memcpy(up,source.up,12);}
    const double x=2.0/(source.width*source.p00*inverseDepth),y=-2.0/(source.height*source.p11*inverseDepth);
    float result[16]{};result[15]=1;
    for(int i=0;i<3;++i) {
        result[i]=float(x*right[i]*matrix[0]+y*up[i]*matrix[1]);
        result[4+i]=float(x*right[i]*matrix[4]+y*up[i]*matrix[5]);
        result[12+i]=origin[i];
    }
    result[10]=kWorldTextMagic;
    std::memcpy(matrix,result,sizeof(result));return true;
}
inline WorldMarkerCamera MarkerCamera(const SourceProjection& eye) {
    WorldMarkerCamera result;
    std::memcpy(result.origin,eye.origin,12);result.origin[3]=kWorldCameraMagic;
    std::memcpy(result.right,eye.right,12);std::memcpy(result.up,eye.up,12);std::memcpy(result.forward,eye.forward,12);
    result.projection[0]=eye.p00;result.projection[1]=eye.p11;result.projection[2]=eye.p20;result.projection[3]=eye.p21;
    return result;
}
#endif
}
