#pragma once
#include "Overlay/VrOverlay.hpp"
#include "Render/ColorBlit.hpp"
#include <openxr/openxr_platform.h>
#include <memory>
#include <vector>
#if CVR_RUST_VRIK
// THE CANVASES AND THE PANEL ARE RUST'S (rust/crates/cvr-xr/src/vr_panel.rs): a canvas here is a lease on one of
// Rust's, its texture and view held for the lease; the class holds the Rust panel.
extern "C" {
void* cvr_vrui_panel_new();
void cvr_vrui_panel_free(void* panel);
const XrCompositionLayerBaseHeader* cvr_vrui_panel_prepare(void* panel, uint64_t session, uint64_t space, void* device,
                                                           void* queue);
const XrCompositionLayerBaseHeader* cvr_vrui_panel_ray(const void* panel);
void cvr_vrui_panel_shutdown(void* panel);
}
namespace cvr::vrui {
struct Canvas {
    Microsoft::WRL::ComPtr<ID3D12Resource> texture;
    Microsoft::WRL::ComPtr<ID3D12DescriptorHeap> rtv;
    const void* lease = nullptr;
};
std::shared_ptr<Canvas> AcquireCanvas(ID3D12Device*,DXGI_FORMAT);
void PublishCanvas(const std::shared_ptr<Canvas>&,ID3D12Fence*,uint64_t);
void ClearCanvas();
class Panel {
public:
    Panel() : rust(cvr_vrui_panel_new()) {}
    ~Panel() { cvr_vrui_panel_free(rust); }
    Panel(const Panel&) = delete;
    Panel& operator=(const Panel&) = delete;
    const XrCompositionLayerBaseHeader* Prepare(XrSession session, XrSpace space, ID3D12Device* device,
                                                ID3D12CommandQueue* queue) {
        return cvr_vrui_panel_prepare(rust, reinterpret_cast<uint64_t>(session), reinterpret_cast<uint64_t>(space),
                                      device, queue);
    }
    const XrCompositionLayerBaseHeader* Ray() const { return cvr_vrui_panel_ray(rust); }
    void Shutdown() { cvr_vrui_panel_shutdown(rust); }
    // The Rust panel, for rust/crates/cvr-render/src/xr_cycle_end.rs.
    void* Raw() const { return rust; }
private:
    void* rust;
};
}
#else
namespace cvr::vrui {
struct Canvas {
    Microsoft::WRL::ComPtr<ID3D12Resource> texture;
    Microsoft::WRL::ComPtr<ID3D12DescriptorHeap> rtv;
    Microsoft::WRL::ComPtr<ID3D12Fence> fence;
    uint64_t value=0,serial=0;
    DXGI_FORMAT format=DXGI_FORMAT_UNKNOWN;
};
std::shared_ptr<Canvas> AcquireCanvas(ID3D12Device*,DXGI_FORMAT);
void PublishCanvas(const std::shared_ptr<Canvas>&,ID3D12Fence*,uint64_t);
void ClearCanvas();
class Panel {
public:
    const XrCompositionLayerBaseHeader* Prepare(XrSession,XrSpace,ID3D12Device*,ID3D12CommandQueue*);
    const XrCompositionLayerBaseHeader* Ray() const;
    void Shutdown();
private:
    XrSwapchain swapchain{};
    std::vector<XrSwapchainImageD3D12KHR> images;
    Microsoft::WRL::ComPtr<ID3D12CommandAllocator> allocator;
    Microsoft::WRL::ComPtr<ID3D12GraphicsCommandList> list;
    Microsoft::WRL::ComPtr<ID3D12Fence> fence;
    std::shared_ptr<Canvas> flight;
    ColorBlit blit;
    uint64_t fenceValue=0,lastSerial=0;
    uint32_t index=0;bool acquired=false,published=false,rayVisible=false;
    XrCompositionLayerQuad layer{XR_TYPE_COMPOSITION_LAYER_QUAD},ray{XR_TYPE_COMPOSITION_LAYER_QUAD};
};
}
#endif
