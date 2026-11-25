#pragma once
#include <windows.h>
#include <d3d12.h>
#include <openxr/openxr.h>
#include <openxr/openxr_platform.h>
#include <wrl/client.h>
#include <vector>
#include "Runtimes/HudFollow.hpp"

namespace cvr::framegen {
// A small persistent bitmap, refreshed four times a second. Submission only
// reuses its XR quad; no ImGui context, font atlas or per-frame texture raster.
class StatsOverlay {
public:
    const XrCompositionLayerBaseHeader* Prepare(XrSession,XrSpace,ID3D12Device*,ID3D12CommandQueue*,
        XrSpace local=XR_NULL_HANDLE,const XrPosef* head=nullptr,uint64_t origin=0,XrTime displayTime=0,
        const char* notice=nullptr);
    bool Owns(const XrCompositionLayerBaseHeader* candidate) const {
        return candidate == reinterpret_cast<const XrCompositionLayerBaseHeader*>(&layer);
    }
    bool Displays(const char* notice) const { return notice && notice == renderedNotice; }
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
    const char* renderedNotice{}; // Only static message literals from ModeNoticeText().
    uint32_t acquiredIndex{};bool acquired{},published{};
    uint32_t width{},height{},rowPitch{};
    std::vector<uint32_t> pixels;
    cvr::hud::Follow follow;
    cvr::hud::DisplayClock clock;
    XrPosef anchor{{0,0,0,1},{0,0,0}};
    uint64_t originSerial{};
    int followMode=-1;
};
}
