#pragma once
#include <cmath>
#if CVR_RUST_VRIK
#include <cstdint>
#endif

namespace cvr::markers {
// Native cached mappins place their texture at screenAnchor + cacheBounds.min.
// Scaling about this local point leaves the screen/world anchor fixed, even if
// an animation changes the cached bounds. The texture centre need not be it.
#if CVR_RUST_VRIK
// The arithmetic: rust/crates/cvr-render/src/world_marker_math.rs through world_marker_abi.rs.
struct MarkerScale;
extern "C" {
bool cvr_wm_cached_marker_pivot(float,float,float,float,float*,float*);
bool cvr_wm_marker_scale(uint32_t,MarkerScale*,float,float,float,float,float*,float*);
}
inline bool CachedMarkerPivot(float width,float height,float offsetX,float offsetY,float& x,float& y){
    return cvr_wm_cached_marker_pivot(width,height,offsetX,offsetY,&x,&y);
}
struct MarkerScale {
    float appliedX=0,appliedY=0,nativeX=1,nativeY=1;
    bool applied=false;
    bool Target(float currentX,float currentY,float& x,float& y,float factor=.7f) const {
        return cvr_wm_marker_scale(0,const_cast<MarkerScale*>(this),currentX,currentY,factor,0,&x,&y);
    }
    // from: the game's scale the target was made from.
    void Commit(float x,float y,float fromX,float fromY){cvr_wm_marker_scale(1,this,x,y,fromX,fromY,nullptr,nullptr);}
    // The game's scale back, while the widget still carries ours (a newer game scale is the game's own).
    bool Restore(float currentX,float currentY,float& x,float& y) const {
        return cvr_wm_marker_scale(2,const_cast<MarkerScale*>(this),currentX,currentY,0,0,&x,&y);
    }
};
static_assert(sizeof(MarkerScale)==20,"mirrored by world_marker_math.rs");
#else
inline bool CachedMarkerPivot(float width,float height,float offsetX,float offsetY,float& x,float& y) {
    if(!std::isfinite(width) || !std::isfinite(height) || width<=0 || height<=0 ||
       !std::isfinite(offsetX) || !std::isfinite(offsetY))return false;
    x=-offsetX/width;y=-offsetY/height;
    return std::abs(x)<=16 && std::abs(y)<=16;
}
struct MarkerScale {
    float appliedX=0,appliedY=0,nativeX=1,nativeY=1;
    bool applied=false;
    bool Target(float currentX,float currentY,float& x,float& y,float factor=.7f) const {
        if(!std::isfinite(currentX) || !std::isfinite(currentY) || !std::isfinite(factor) || !(factor>0))return false;
        if(applied && currentX==appliedX && currentY==appliedY)return false;
        x=currentX*factor;y=currentY*factor;
        return true;
    }
    // from: the game's scale the target was made from.
    void Commit(float x,float y,float fromX,float fromY){appliedX=x;appliedY=y;nativeX=fromX;nativeY=fromY;applied=true;}
    // The game's scale back, while the widget still carries ours (a newer game scale is the game's own).
    bool Restore(float currentX,float currentY,float& x,float& y) const {
        if(!applied || currentX!=appliedX || currentY!=appliedY)return false;
        x=nativeX;y=nativeY;
        return true;
    }
};
#endif
}
