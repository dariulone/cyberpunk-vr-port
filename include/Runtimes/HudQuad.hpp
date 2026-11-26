#pragma once
#include <windows.h>
#include <d3d12.h>
#include <openxr/openxr.h>
#include <openxr/openxr_platform.h>
#include <wrl.h>
#include <vector>
#include <memory>
#include <array>
#include <atomic>
#include "Runtimes/HudFollow.hpp"
#include "Render/HudPanel.hpp"

namespace cvr::hud {
struct PoseDebug {
    uint64_t frames=0;
    int64_t displayTime=0;
    float headYaw=0,panelYaw=0,dt=0,eyeError=0;
    int32_t mode=0,tracked=0,layers=0;
    XrVector3f head{},left{},right{};
    uint64_t sourceSerial=0,sourceTick=0;
};
static_assert(sizeof(PoseDebug)==96 && offsetof(PoseDebug,sourceSerial)==80);
#if CVR_RUST_VRIK
}
// The quad is rust/crates/cvr-xr/src/hud_quad.rs; this class holds one.
extern "C" {
void* cvr_xr_hud_quad_new(unsigned channel);
void cvr_xr_hud_quad_free(void* quad);
const XrCompositionLayerBaseHeader* cvr_xr_hud_quad_prepare(void* quad, uint64_t session, uint64_t space, void* device, void* queue,
    bool enabled, bool tracked, const XrPosef* head, uint64_t origin, int64_t displayTime, const XrVector3f* eyeOffsets, float bodyYaw);
const XrCompositionLayerBaseHeader* cvr_xr_hud_quad_second_eye(const void* quad);
void cvr_xr_hud_quad_shutdown(void* quad);
}
namespace cvr::hud {
class Quad {
public:
    explicit Quad(Channel value=Channel::Main):rust(cvr_xr_hud_quad_new(static_cast<unsigned>(value))){}
    ~Quad() { cvr_xr_hud_quad_free(rust); }
    Quad(const Quad&) = delete;
    Quad& operator=(const Quad&) = delete;
    const XrCompositionLayerBaseHeader* Prepare(XrSession session, XrSpace space,
        ID3D12Device* device, ID3D12CommandQueue* queue, bool enabled,
        bool tracked, const XrPosef& head, uint64_t origin,
        XrTime displayTime=0, const XrVector3f* eyeOffsets=nullptr, float bodyYaw=0) {
        return cvr_xr_hud_quad_prepare(rust, reinterpret_cast<uint64_t>(session), reinterpret_cast<uint64_t>(space), device, queue,
                                       enabled, tracked, &head, origin, displayTime, eyeOffsets, bodyYaw);
    }
    const XrCompositionLayerBaseHeader* SecondEye() const { return cvr_xr_hud_quad_second_eye(rust); }
    void Shutdown() { cvr_xr_hud_quad_shutdown(rust); }
    // The Rust quad, for rust/crates/cvr-xr/src/hud_quad.rs cvr_xr_hud_layers.
    void* Raw() const { return rust; }
private:
    void* rust;
};
#else
class Quad {
public:
    explicit Quad(Channel value=Channel::Main):channel(value){}
    const XrCompositionLayerBaseHeader* Prepare(XrSession session, XrSpace space,
        ID3D12Device* device, ID3D12CommandQueue* queue, bool enabled,
        bool tracked, const XrPosef& head, uint64_t origin,
        XrTime displayTime=0, const XrVector3f* eyeOffsets=nullptr, float bodyYaw=0);
    const XrCompositionLayerBaseHeader* SecondEye() const {
        return secondEye ? reinterpret_cast<const XrCompositionLayerBaseHeader*>(&rightLayer) : nullptr;
    }
    void Shutdown();
private:
    Channel channel;
    bool Ensure(XrSession session, ID3D12Device* device, uint32_t width, uint32_t height);
    void RetireCompleted(uint64_t completed);
    bool UpdateImage(const std::shared_ptr<Frame>& frame, ID3D12CommandQueue* queue, bool pipelined);
    XrSwapchain swapchain = XR_NULL_HANDLE;
    std::vector<XrSwapchainImageD3D12KHR> images;
    struct CopySlot {
        Microsoft::WRL::ComPtr<ID3D12CommandAllocator> allocator;
        Microsoft::WRL::ComPtr<ID3D12GraphicsCommandList> commands;
        std::shared_ptr<Frame> frame;
        uint64_t fenceValue=0;
    };
    std::array<CopySlot,3> copies;
    size_t copyCursor=0;
    Microsoft::WRL::ComPtr<ID3D12Fence> fence;
    uint64_t fenceValue=0, originSerial=0, lastStamp=0;
    uint32_t width=0,height=0;
    uint32_t acquiredIndex=0;
    bool acquired=false;
    bool submissionFailed=false,publishedMasked=false,publishedLootVisible=false;
    uint64_t publishedTick=0;
    HANDLE event=nullptr;
    Follow follow;
    DisplayClock clock;
    int followMode=-1;
    bool secondEye=false;
    XrPosef anchor{{0,0,0,1},{0,0,0}};
    XrVector3f lastEyeOffsets[2]{};
    bool eyeOffsetsValid=false;
    XrCompositionLayerQuad layer{XR_TYPE_COMPOSITION_LAYER_QUAD};
    XrCompositionLayerQuad rightLayer{XR_TYPE_COMPOSITION_LAYER_QUAD};
};
#endif
}
extern "C" __declspec(dllexport) extern std::atomic<uint64_t> CyberpunkVR_HudSubmittedFrames;
