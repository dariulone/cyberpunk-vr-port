#pragma once
// Adapted from OFXR V557 e48d6fcf6289 runtime binding/reacquire/release joins.
// LGPL-3.0: licenses/OFXR-LGPL-3.0.txt.
#include <d3d12.h>
#include "Framegen/DisplayProbe.hpp"
#include <wrl/client.h>
#include <openxr/openxr.h>
#include <mutex>
#include <cstdint>

namespace cvr::xrqueue {
class GpuQueue {
    Microsoft::WRL::ComPtr<ID3D12CommandQueue> application,runtime;
    Microsoft::WRL::ComPtr<ID3D12Fence> release,reacquire;
    uint64_t released{},reacquired{};
    std::mutex mutex;
    HRESULT result{S_OK};
    framegen::DisplayProbePool probes;
public:
    bool Initialize(ID3D12Device* d,ID3D12CommandQueue* q,bool enabled) {
        if(!d || !q || q->GetDesc().Type!=D3D12_COMMAND_LIST_TYPE_DIRECT)return false;
        application=q;runtime=q;
        if(!enabled)return true;
        D3D12_COMMAND_QUEUE_DESC desc{};desc.Type=D3D12_COMMAND_LIST_TYPE_DIRECT;desc.NodeMask=q->GetDesc().NodeMask;
        Microsoft::WRL::ComPtr<ID3D12CommandQueue> candidate;
        if(FAILED(result=d->CreateCommandQueue(&desc,IID_PPV_ARGS(&candidate))) ||
           FAILED(result=d->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&release))) ||
           FAILED(result=d->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&reacquire))))return true;
        runtime=candidate;runtime->SetName(L"CyberpunkVR SteamVR presentation");
        return true;
    }
    bool Private() const {return runtime && runtime.Get()!=application.Get();}
    ID3D12CommandQueue* Queue() const {return runtime.Get();}
    ID3D12CommandQueue* Application() const {return application.Get();}
    HRESULT CreationResult() const {return result;}
    // Immediately after a successful runtime wait, carry image availability
    // back to the application's queue before it writes an overlay image.
    bool ToApplication() {
        if(!Private())return true;
        std::lock_guard lock(mutex);
        const auto v=reacquired+1;
        if(FAILED(runtime->Signal(reacquire.Get(),v)))return false;
        reacquired=v;return SUCCEEDED(application->Wait(reacquire.Get(),v));
    }
    // Before releasing an application-written image, carry its exact completed-
    // command receipt to the bound queue. Later game work is not included.
    bool ToRuntime() {
        if(!Private())return true;
        std::lock_guard lock(mutex);
        const auto v=released+1;
        if(FAILED(application->Signal(release.Get(),v)))return false;
        released=v;return SUCCEEDED(runtime->Wait(release.Get(),v));
    }
    bool WaitCapture(ID3D12Fence* fence,uint64_t value) {
        if(!Private())return true;
        // Missing receipt: explicitly join existing producer work. Never read
        // unpublished images without a dependency, including boot/menu captures.
        return fence && value ? SUCCEEDED(runtime->Wait(fence,value)) : ToRuntime();
    }
    // Published overlay canvases must carry their own successful receipt.
    // Never substitute a signal at the current game-queue tail here: it may
    // include the NEXT RT/NR frame, delaying this display's otherwise-ready eyes.
    bool WaitPublished(ID3D12Fence* fence,uint64_t value) {
        if(!Private())return true;
        return fence && value && SUCCEEDED(runtime->Wait(fence,value));
    }
    uint64_t Mark() {
        if(!Private())return 0;
        std::lock_guard lock(mutex);const auto v=reacquired+1;
        if(FAILED(runtime->Signal(reacquire.Get(),v)))return 0;
        reacquired=v;return v;
    }
    bool Ready(uint64_t v) const {return v && reacquire && reacquire->GetCompletedValue()>=v;}
    framegen::DisplayDeadline* Probe(){return Private()?probes.Begin(runtime.Get()):nullptr;}
    template<class Collect> void PollProbes(Collect collect){probes.Poll(collect);}
    bool Drain() {
        if(!Private())return true;
        const auto v=Mark();if(!v)return false;
        const auto deadline=GetTickCount64()+2000;
        while(reacquire->GetCompletedValue()<v && GetTickCount64()<deadline)Sleep(1);
        return reacquire->GetCompletedValue()>=v;
    }
};
// One port-owned OpenXR session. Initialize/destroy only while its owner thread
// is stopped. Third-party layers retain responsibility for their own images.
bool Initialize(ID3D12Device*,ID3D12CommandQueue*,bool steamvr);
ID3D12CommandQueue* Queue();
bool Private();
bool WaitCapture(ID3D12Fence*,uint64_t);
bool WaitPublished(ID3D12Fence*,uint64_t);
XrResult WaitApplicationImage(XrSwapchain,const XrSwapchainImageWaitInfo*);
XrResult ReleaseApplicationImage(XrSwapchain,const XrSwapchainImageReleaseInfo*);
uint64_t Mark();
bool Ready(uint64_t);
framegen::DisplayDeadline* ProbeBegin(bool enabled);
void Drain();
void Shutdown();
}
