#pragma once
#include <atomic>
#include <cstdint>

namespace cvr::scope_dlss {
inline std::atomic<bool> fullResolution{true};
inline constexpr uint64_t kRrFeature = 1ull << (71 - 64);
// CP2.5 RenderRes fallback at4255BD tests exactly these bits before calling
// the global upscaler ratio getter. Its dynamic-resolution branch tests a subset.
inline constexpr uint64_t kResolutionScaleBits =
    (1ull<<6)|(1ull<<7)|(1ull<<8)|(1ull<<9)|(1ull<<10)|
    (1ull<<12)|(1ull<<13)|(1ull<<14)|(1ull<<27);
inline bool FullResolutionEligible(bool enabled,bool scope,uint64_t flags,
                                   int64_t width,int64_t height) {
    return enabled && scope && (flags & kRrFeature) &&
        width>=16 && height>=16 && width<=4096 && height<=4096;
}
// Only around native resolution calculation, BEFORE graph construction. Never
// keep these flags masked into the graph: RR still needs its native denoiser
// inputs/passes. Native code writes every size/previous-size field consistently.
class NativeResolutionFlags {
    uint64_t& flags;
    uint64_t saved;
public:
    explicit NativeResolutionFlags(uint64_t& value) : flags(value),saved(value) {
        flags=saved & ~kResolutionScaleBits;
    }
    ~NativeResolutionFlags(){flags=saved;}
    NativeResolutionFlags(const NativeResolutionFlags&)=delete;
    NativeResolutionFlags& operator=(const NativeResolutionFlags&)=delete;
};
}
