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
#include <cmath>
#include "Runtimes/HudFollow.hpp"
#include "Render/HudPanel.hpp"

namespace cvr::hud {
// What a runtime accepts in a quad layer: a finite position, an orientation of unit length within 1% (the
// OpenXR rule; Monado and WiVRn check it) and a finite, positive size. One layer outside it fails the whole
// xrEndFrame (XR_ERROR_POSE_INVALID, issue #138): the world frame with it.
inline bool Submittable(const XrCompositionLayerQuad& quad) {
    const auto& p=quad.pose.position;const auto& q=quad.pose.orientation;
    const float length=std::sqrt(q.x*q.x+q.y*q.y+q.z*q.z+q.w*q.w);
    return std::isfinite(p.x) && std::isfinite(p.y) && std::isfinite(p.z) &&
        std::isfinite(length) && std::fabs(length-1.0f)<=0.01f &&
        std::isfinite(quad.size.width) && std::isfinite(quad.size.height) &&
        quad.size.width>0.0f && quad.size.height>0.0f;
}
struct PoseDebug {
    uint64_t frames=0;
    int64_t displayTime=0;
    float headYaw=0,panelYaw=0,dt=0,eyeError=0;
    int32_t mode=0,tracked=0,layers=0;
    XrVector3f head{},left{},right{};
    uint64_t sourceSerial=0,sourceTick=0;
};
static_assert(sizeof(PoseDebug)==96 && offsetof(PoseDebug,sourceSerial)==80);
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
}
extern "C" __declspec(dllexport) extern std::atomic<uint64_t> CyberpunkVR_HudSubmittedFrames;
