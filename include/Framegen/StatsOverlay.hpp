#pragma once
#include <windows.h>
#include <d3d12.h>
#include <openxr/openxr.h>
#include <openxr/openxr_platform.h>
#include <wrl/client.h>
#include <vector>
#include "Runtimes/HudFollow.hpp"

namespace cvr::framegen {
#if CVR_RUST_VRIK
// The Rust build's panel is rust/crates/cvr-xr/src/framegen_overlay.rs (drawn by rust/crates/cvr-gpu/src/
// framegen_overlay.rs); this class keeps the name the frame loop holds.
class StatsOverlay {
public:
    StatsOverlay();
    ~StatsOverlay();
    StatsOverlay(const StatsOverlay&) = delete;
    StatsOverlay& operator=(const StatsOverlay&) = delete;
    const XrCompositionLayerBaseHeader* Prepare(XrSession,XrSpace,ID3D12Device*,ID3D12CommandQueue*,
        XrSpace local=XR_NULL_HANDLE,const XrPosef* head=nullptr,uint64_t origin=0,XrTime displayTime=0);
    void Shutdown(bool wait=false);
    // The Rust panel, for rust/crates/cvr-render/src/xr_cycle_end.rs.
    void* Raw() const { return rust_; }
private:
    void* rust_;
};
#else
// A small persistent bitmap, refreshed four times a second. Submission only
// reuses its XR quad; no ImGui context, font atlas or per-frame texture raster.
class StatsOverlay {
public:
    const XrCompositionLayerBaseHeader* Prepare(XrSession,XrSpace,ID3D12Device*,ID3D12CommandQueue*,
        XrSpace local=XR_NULL_HANDLE,const XrPosef* head=nullptr,uint64_t origin=0,XrTime displayTime=0);
    void Shutdown(bool wait=false);
private:
    XrSwapchain swapchain{};
    std::vector<XrSwapchainImageD3D12KHR> images;
    Microsoft::WRL::ComPtr<ID3D12CommandAllocator> allocator;
    Microsoft::WRL::ComPtr<ID3D12GraphicsCommandList> list;
    Microsoft::WRL::ComPtr<ID3D12Resource> upload;
    Microsoft::WRL::ComPtr<ID3D12Fence> fence;
    XrCompositionLayerQuad layer{XR_TYPE_COMPOSITION_LAYER_QUAD};
    uint64_t fenceValue{};double nextUpdate{};
    uint32_t acquiredIndex{};bool acquired{},published{};
    uint32_t width{},height{},rowPitch{};
    std::vector<uint32_t> pixels;
    cvr::hud::Follow follow;
    cvr::hud::DisplayClock clock;
    XrPosef anchor{{0,0,0,1},{0,0,0}};
    uint64_t originSerial{};
    int followMode=-1;
};
#endif
}
