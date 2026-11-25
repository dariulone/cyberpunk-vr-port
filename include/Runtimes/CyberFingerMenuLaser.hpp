#pragma once
#include <windows.h>
#include <d3d12.h>
#include <wrl.h>
#include <openxr/openxr.h>
#include <openxr/openxr_platform.h>
#include <vector>

namespace cvr::cfmenu {
// A small solid-color swapchain for the original game's menu pointer. It does
// not depend on F10 having been opened or on the desktop mirror's crop.
class Laser {
public:
    const XrCompositionLayerBaseHeader* Prepare(XrSession,XrSpace,ID3D12Device*,ID3D12CommandQueue*);
    void Shutdown();
private:
    XrSwapchain swapchain{};
    DXGI_FORMAT format=DXGI_FORMAT_UNKNOWN;
    Microsoft::WRL::ComPtr<ID3D12CommandAllocator> allocator;
    Microsoft::WRL::ComPtr<ID3D12GraphicsCommandList> list;
    Microsoft::WRL::ComPtr<ID3D12DescriptorHeap> rtv;
    Microsoft::WRL::ComPtr<ID3D12Fence> fence;
    std::vector<XrSwapchainImageD3D12KHR> images;
    bool acquired{},published{};uint32_t index{};uint64_t fenceValue{};
    XrCompositionLayerQuad layer{XR_TYPE_COMPOSITION_LAYER_QUAD};
};
}
