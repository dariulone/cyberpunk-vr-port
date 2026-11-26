#pragma once
#include <cstdint>

namespace cvr::capture {
// A capture is published when queued, before its GPU fence completes. Preserve
// an unpresented predecessor until it completes or a newer completed capture
// can replace it. Otherwise a GPU queue more than two frames deep makes BOTH
// selectable captures perpetually unfinished, even while the GPU advances.
inline bool ProtectPrevious(uint64_t previous, uint64_t presented,uint64_t origin=1,uint64_t currentOrigin=1) {
    return previous && previous > presented && origin && origin==currentOrigin;
}
inline bool ReplacePrevious(uint64_t previous, uint64_t presented,
                            uint64_t candidateFence, uint64_t completedFence) {
    return !ProtectPrevious(previous, presented) || candidateFence <= completedFence;
}
inline bool ReusableEye(uint64_t source,uint64_t requested,uint32_t maxReuse,uint64_t origin,uint64_t currentOrigin) {
    return source && source<=requested && origin && origin==currentOrigin &&
        (source==requested || (maxReuse && requested-source<=maxReuse));
}
}
