#pragma once
#include <d3d12.h>
#include <cstdint>

namespace cvr::loading::diagnostic {
// One-shot, opt-in pixel evidence. Create log/loading-capture.request to arm
// the next post-boot load. At most 60 pairs of window/VR-eye thumbnails are
// saved at 5 Hz. No GPU waits or worker threads; busy readbacks skip a sample.
struct Frame {
    uint64_t serial{};
    bool loading{}, freshPicture{}, worldDrawn{};
    unsigned eye{};
};
// Capture thread only. Window rests in PRESENT, eye in COPY_SOURCE. The caller
// signals fence/value after executing this list. Resources stay owned until
// that fence completes. Normal rendering never depends on capture success.
void Record(ID3D12Device*, ID3D12GraphicsCommandList*, ID3D12Resource* window,
            ID3D12Resource* eye, ID3D12Fence*, uint64_t value, const Frame&);
// Release completed resources; keep any in-flight readbacks alive without waiting.
void Shutdown();
}
