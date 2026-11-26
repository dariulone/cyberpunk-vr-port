#pragma once
#if CVR_RUST_VRIK
#include <cstdint>
// THE STEREO DEPTH LAYER IS RUST'S (rust/crates/cvr-render/src/depth_layer.rs): the policy, the eyes' depth swapchains,
// the record and its renderer, the report. This face is OpenXRManager's member -- the renderer's finished slots let go,
// its idleness, the renderer freed with the manager -- with the layer's state the manager still asks for.
extern "C" {
bool cvr_xr_depth_supported();
void cvr_xr_depth_set_supported(bool value);
uint64_t cvr_xr_depth_swapchain(uint32_t eye);
void cvr_xr_depth_forget_format();
void cvr_xr_depth_destroy_swapchains();
void cvr_xr_depth_poll();
bool cvr_xr_depth_idle();
void cvr_xr_depth_shutdown();
}
namespace cvr::depth {
class Renderer {
public:
    Renderer() = default;
    Renderer(const Renderer&) = delete;
    Renderer& operator=(const Renderer&) = delete;
    ~Renderer() { cvr_xr_depth_shutdown(); }
    bool Idle() const { return cvr_xr_depth_idle(); }
    void Poll() { cvr_xr_depth_poll(); }
};
}
#else
#include "Runtimes/StereoDepthPolicy.hpp"
#include <memory>

namespace cvr::depth {
// Owned input snapshots are held until the SUBMIT fence, not just their producer
// fence. Each in-flight list has independent descriptors and output textures.
class Renderer {
public:
    Renderer();
    ~Renderer();
    bool Record(ID3D12Device*,ID3D12GraphicsCommandList*,ID3D12CommandQueue*,
        const std::shared_ptr<const framegen::Inputs> (&inputs)[2],
        ID3D12Resource* const (&destinations)[2],const Plan&,unsigned slot,
        ID3D12Fence* submitFence,uint64_t submitValue);
    bool Idle() const;
    void Poll();
private:
    struct Impl;
    std::unique_ptr<Impl> impl;
};
}
#endif
