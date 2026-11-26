#pragma once
#include <atomic>
#include <bit>
#include <cmath>
#include <cstdint>

namespace cvr::gaze {
// One binocular gaze ray through the vertical centre of the rendered views.
// Horizontal eye asymmetry is retained: their separate centres are not a common ray.
struct Center {float tangentY{},ndcY{};};
#if CVR_RUST_VRIK
// The centre and the head-look switch, owned by rust/crates/cvr-xr/src/gaze.rs (the frame loop publishes there).
struct XrGazeShared {std::atomic<uint64_t> centerBits;std::atomic<bool> headLook;};
static_assert(sizeof(XrGazeShared) == 16, "mirrored by GazeShared in rust/crates/cvr-xr/src/gaze.rs");
extern "C" XrGazeShared* cvr_xr_gaze_shared();
inline std::atomic<uint64_t>& centerBits = cvr_xr_gaze_shared()->centerBits;
inline std::atomic<bool>& headLook = cvr_xr_gaze_shared()->headLook;
#else
inline std::atomic<uint64_t> centerBits{};
inline std::atomic<bool> headLook{true};
#endif
inline Center FromWindows(float up0,float down0,float up1,float down1) {
    const float middle=(up0+down0+up1+down1)*.25f;
    const float half=(up0-down0+up1-down1)*.25f;
    if(!std::isfinite(middle) || !std::isfinite(half) || half<.05f || std::abs(middle)>1.f)return {};
    return {middle,middle/half};
}
inline void Publish(Center value){centerBits.store(std::bit_cast<uint64_t>(value),std::memory_order_release);}
inline Center Read(){return headLook.load(std::memory_order_relaxed) ? std::bit_cast<Center>(centerBits.load(std::memory_order_acquire)) : Center{};}
inline float CanvasOffset(Center center,float height,float designScale) {
    return height>0 && designScale>0 ? -center.ndcY*height*.5f/designScale : 0.f;
}
}
