#pragma once
#include "Overlay/VrOverlay.hpp"
#include "Render/ColorBlit.hpp"
#include <openxr/openxr_platform.h>
#include <memory>
#include <vector>
namespace cvr::vrui {
struct Canvas {
    Microsoft::WRL::ComPtr<ID3D12Resource> texture;
    Microsoft::WRL::ComPtr<ID3D12DescriptorHeap> rtv;
    Microsoft::WRL::ComPtr<ID3D12Fence> fence;
    uint64_t value=0,serial=0;
    DXGI_FORMAT format=DXGI_FORMAT_UNKNOWN;
};
std::shared_ptr<Canvas> MakeCanvas(ID3D12Device*,DXGI_FORMAT);
std::shared_ptr<Canvas> AcquireCanvas(ID3D12Device*,DXGI_FORMAT);
void PublishCanvas(const std::shared_ptr<Canvas>&,ID3D12Fence*,uint64_t);
void ClearCanvas();
class Panel {
public:
    const XrCompositionLayerBaseHeader* Prepare(XrSession,XrSpace,ID3D12Device*,ID3D12CommandQueue*);
    const XrCompositionLayerBaseHeader* Prepare(XrSession,XrSpace,ID3D12Device*,ID3D12CommandQueue*,const View&,const std::shared_ptr<Canvas>&);
    const XrCompositionLayerBaseHeader* Ray() const;
    void Shutdown(bool clearDefaultCanvas=true);
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
