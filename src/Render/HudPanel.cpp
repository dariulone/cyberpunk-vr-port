#include "Render/HudPanel.hpp"
#include "Render/OpticsProbe.hpp"
#include "Render/ColorBlit.hpp"
#include "Render/GpuStageProfile.hpp"
#include "Core/LiveControls.hpp"
#include <cstring>
#include <algorithm>
#include <mutex>
#include <array>
#include <atomic>

extern void Log(const char*, ...);
extern "C" ID3D12Resource* CyberpunkVR_GetFrameConstantBuffer();
extern "C" ID3D12Resource* CyberpunkVR_GetHudConstantBuffer();
extern "C" float CyberpunkVR_HudEngineCurvature[2];
extern "C" ID3D12Resource* CyberpunkVR_GetHudExposureBuffer();
namespace cvr::hud {
using Microsoft::WRL::ComPtr;
namespace {
struct Slot {
    std::shared_ptr<Frame> frame;
    std::shared_ptr<Snapshot> sources;
    ComPtr<ID3D12CommandAllocator> allocator;
    ComPtr<ID3D12GraphicsCommandList> commands;
    ComPtr<ID3D12DescriptorHeap> rtv;
    ColorBlit blit; // One ring per fenced slot: sprites, mips, blur and native HUD style.
    ComPtr<ID3D12Resource> raw, blur, blurTemp, defaults;
    ComPtr<ID3D12Resource> frameConstants, hudConstants, exposure;
    float* defaultData=nullptr;
    uint64_t fenceValue = 0;
};
struct Pipeline {
    std::mutex snapshotMutex,renderMutex;
    std::shared_ptr<Snapshot> snapshot;
    std::shared_ptr<Frame> latest;
    std::atomic<uint64_t> consumerStamp{0};
    std::array<Slot,3> slots;
    ComPtr<ID3D12Fence> fence;
    ComPtr<ID3D12Device> deviceOwner;
    uint64_t submitted=0;
};
std::array<Pipeline,ChannelCount> pipelines;

D3D12_RESOURCE_BARRIER Transition(ID3D12Resource* resource, D3D12_RESOURCE_STATES before,
                                 D3D12_RESOURCE_STATES after, UINT mip=D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES) {
    D3D12_RESOURCE_BARRIER b{};
    b.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    b.Transition = {resource,mip,before,after};
    return b;
}
bool Initialize(Slot& slot, ID3D12Device* device, uint32_t width, uint32_t height) {
    if (slot.frame) {
        auto desc = slot.frame->canvas->GetDesc();
        if (desc.Width == width && desc.Height == height) return true;
    }
    slot.frame.reset();slot.raw.Reset();slot.blur.Reset();slot.blurTemp.Reset();
    if(!slot.defaults) {
        D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_UPLOAD;
        D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;desc.Width=1024;desc.Height=1;
        desc.DepthOrArraySize=1;desc.MipLevels=1;desc.SampleDesc.Count=1;desc.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
        if(FAILED(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_GENERIC_READ,
            nullptr,IID_PPV_ARGS(&slot.defaults)))) return false;
        void* mapped=nullptr;if(FAILED(slot.defaults->Map(0,nullptr,&mapped)))return false;
        slot.defaultData=static_cast<float*>(mapped);std::memset(mapped,0,1024);slot.defaultData[6]=1.0f;
    }
    if (!slot.allocator && FAILED(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,
                                                                IID_PPV_ARGS(&slot.allocator)))) return false;
    if (!slot.commands) {
        if (FAILED(device->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_DIRECT,slot.allocator.Get(),
                                            nullptr,IID_PPV_ARGS(&slot.commands)))) return false;
        slot.commands->Close();
    }
    if (!slot.rtv) {
        D3D12_DESCRIPTOR_HEAP_DESC desc{}; desc.Type=D3D12_DESCRIPTOR_HEAP_TYPE_RTV; desc.NumDescriptors=1;
        if (FAILED(device->CreateDescriptorHeap(&desc,IID_PPV_ARGS(&slot.rtv)))) return false;
    }
    D3D12_RESOURCE_DESC desc{};
    desc.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D; desc.Width=width; desc.Height=height;
    desc.DepthOrArraySize=1; desc.MipLevels=1; desc.Format=DXGI_FORMAT_R8G8B8A8_TYPELESS;
    desc.SampleDesc.Count=1; desc.Flags=D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
    D3D12_HEAP_PROPERTIES heap{}; heap.Type=D3D12_HEAP_TYPE_DEFAULT;
    D3D12_CLEAR_VALUE clear{}; clear.Format=DXGI_FORMAT_R8G8B8A8_UNORM_SRGB;
    auto frame=std::make_shared<Frame>();
    if (FAILED(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,
        D3D12_RESOURCE_STATE_COPY_SOURCE,&clear,IID_PPV_ARGS(&frame->canvas)))) return false;
    frame->canvas->SetName(L"CyberpunkVR separate HUD panel");
    desc.MipLevels=5;
    if(FAILED(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
        &clear,IID_PPV_ARGS(&slot.raw)))) return false;
    desc.Width=std::max<UINT64>(1,width/2);desc.Height=std::max<UINT>(1,height/2);desc.MipLevels=4;
    if(FAILED(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
        &clear,IID_PPV_ARGS(&slot.blur)))) return false;
    desc.MipLevels=1;
    if(FAILED(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
        &clear,IID_PPV_ARGS(&slot.blurTemp)))) return false;
    D3D12_RENDER_TARGET_VIEW_DESC view{}; view.Format=clear.Format; view.ViewDimension=D3D12_RTV_DIMENSION_TEXTURE2D;
    device->CreateRenderTargetView(slot.raw.Get(),&view,slot.rtv->GetCPUDescriptorHandleForHeapStart());
    if (!slot.blit.EnsureInitialized(device,clear.Format,width,height)) return false;
    slot.frame=std::move(frame);
    return true;
}
}

void Publish(std::shared_ptr<Snapshot> value,Channel channel) {
    auto& [snapshotMutex,renderMutex,snapshot,latest,consumerStamp,slots,fence,deviceOwner,submitted]=pipelines[Index(channel)];
    std::lock_guard lock(snapshotMutex); snapshot=std::move(value);
}
bool ConsumerReady(Channel channel) {
    auto& consumerStamp=pipelines[Index(channel)].consumerStamp;
    const auto stamp=consumerStamp.load(std::memory_order_acquire);
    return stamp && GetTickCount64()-stamp < 300;
}
void SetConsumerReady(bool ready,Channel channel) { pipelines[Index(channel)].consumerStamp.store(ready ? GetTickCount64() : 0,std::memory_order_release); }

void Capture(ID3D12Device* device, ID3D12CommandQueue* queue,Channel channel) {
    if(channel==Channel::Main)cvr::optics::probe::Capture(device,queue);
    auto& [snapshotMutex,renderMutex,snapshot,latest,consumerStamp,slots,fence,deviceOwner,submitted]=pipelines[Index(channel)];
    if (!device || !queue) return;
    std::shared_ptr<Snapshot> source;
    { std::lock_guard lock(snapshotMutex); source=snapshot; }
    std::lock_guard lock(renderMutex);
    if (source && source->sprites.empty() && source->emptyContent && GetTickCount64()-source->stamp <= 300) {
        // Nothing visible in the channel: publish an empty frame instead of building a canvas of
        // zeros, so the consumer stays ready and the native slots stay masked.
        auto frame=std::make_shared<Frame>();
        frame->empty=true; frame->stamp=source->stamp; frame->generation=source->generation;
        frame->masked=source->masked; frame->lootVisible=source->lootVisible;
        latest=std::move(frame);
        return;
    }
    if (!source || source->sprites.empty() || GetTickCount64()-source->stamp > 300) { latest.reset(); return; }
    if (!fence) {
        if (FAILED(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&fence)))) return;
        deviceOwner=device;
    }
    if (deviceOwner.Get()!=device) { SetConsumerReady(false,channel); return; }
    const auto completed=fence->GetCompletedValue();
    for (auto& slot:slots) {
        // The XR reader holds Frame until its GPU copy completes, including on
        // a separate presentation queue. The producer fence protects the native
        // source leases and allocator; shared ownership prevents canvas reuse.
        if (slot.fenceValue > completed || (slot.frame && slot.frame.use_count()!=1)) continue;
        slot.sources.reset();slot.frameConstants.Reset();slot.hudConstants.Reset();slot.exposure.Reset();
        if (!Initialize(slot,device,source->width,source->height)) { SetConsumerReady(false,channel); return; }
        if (FAILED(slot.allocator->Reset()) || FAILED(slot.commands->Reset(slot.allocator.Get(),nullptr))) return;
        auto cmd=slot.commands.Get();
        auto gpuProfile = cvr::gpu::profile::Begin(cmd);
        bool ok=true;
        {
        cvr::gpu::profile::Scope spriteScope(gpuProfile, cvr::gpu::profile::Stage::HudSprites);
        auto toRT=Transition(slot.raw.Get(),D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,D3D12_RESOURCE_STATE_RENDER_TARGET,0);
        cmd->ResourceBarrier(1,&toRT);
        const float clear[4]{};
        cmd->ClearRenderTargetView(slot.rtv->GetCPUDescriptorHandleForHeapStart(),clear,0,nullptr);
        for (auto& sprite:source->sprites) {
            // Native separate-window textures rest in PIXEL|NON_PIXEL_SHADER_RESOURCE.
            // Sampling needs no transition and leaves the game's resource-state tracker intact.
            ok &= slot.blit.RecordOverlay(cmd,sprite.texture->resource.Get(),slot.raw.Get(),
                0,true,sprite.x,sprite.y,sprite.width,sprite.height,sprite.tint);
        }
        auto rawRead=Transition(slot.raw.Get(),D3D12_RESOURCE_STATE_RENDER_TARGET,D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,0);
        cmd->ResourceBarrier(1,&rawRead);
        }
        {
        cvr::gpu::profile::Scope blurScope(gpuProfile, cvr::gpu::profile::Stage::HudBlur);
        auto pass=[&](ID3D12Resource* src,UINT srcMip,ID3D12Resource* dst,UINT dstMip,int blurAxis) {
            const auto d=dst->GetDesc();
            auto rt=Transition(dst,D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,D3D12_RESOURCE_STATE_RENDER_TARGET,dstMip);
            cmd->ResourceBarrier(1,&rt);
            ok &= slot.blit.RecordOverlay(cmd,src,dst,4,true,0,0,float(std::max<UINT64>(1,d.Width>>dstMip)),
                float(std::max<UINT>(1,d.Height>>dstMip)),nullptr,srcMip,dstMip,blurAxis);
            std::swap(rt.Transition.StateBefore,rt.Transition.StateAfter);cmd->ResourceBarrier(1,&rt);
        };
        for(UINT mip=1;mip<5;++mip)pass(slot.raw.Get(),mip-1,slot.raw.Get(),mip,0);
        pass(slot.raw.Get(),1,slot.blurTemp.Get(),0,1);
        pass(slot.blurTemp.Get(),0,slot.blur.Get(),0,2);
        for(UINT mip=1;mip<4;++mip)pass(slot.blur.Get(),mip-1,slot.blur.Get(),mip,0);
        }
        {
        cvr::gpu::profile::Scope styleScope(gpuProfile, cvr::gpu::profile::Stage::HudStyle);
        auto canvasRT=Transition(slot.frame->canvas.Get(),D3D12_RESOURCE_STATE_COPY_SOURCE,D3D12_RESOURCE_STATE_RENDER_TARGET);
        cmd->ResourceBarrier(1,&canvasRT);
        slot.frameConstants=CyberpunkVR_GetFrameConstantBuffer();
        slot.hudConstants=CyberpunkVR_GetHudConstantBuffer();
        slot.exposure=CyberpunkVR_GetHudExposureBuffer();
        // Defaults remain immutable while this slot is in flight. A missing optional
        // native buffer cannot prevent the HUD from appearing after a cold start.
        slot.defaultData[0]=float(GetTickCount64()%100000)*0.001f;
        ColorBlit::HudParams style{};style.layerOnly=1;
        if(CyberpunkVR_HudEngineCurvature[1]!=0.0f) {
            style.curvature[0]=CyberpunkVR_HudEngineCurvature[0];style.curvature[1]=CyberpunkVR_HudEngineCurvature[1];
            style.panelEngineCurve=1.0f;
        }
        style.panelBrightness=g_liveControls.xrHudBrightness;
        style.panelShadow=g_liveControls.xrHudShadow;
        style.panelGlow=g_liveControls.xrHudGlow;
        ok &= slot.blit.RecordHudComposite(cmd,slot.raw.Get(),slot.raw.Get(),slot.blur.Get(),
            slot.exposure ? slot.exposure.Get() : slot.defaults.Get(),
            slot.frameConstants ? slot.frameConstants.Get() : slot.defaults.Get(),
            slot.hudConstants ? slot.hudConstants.Get() : slot.defaults.Get(),slot.frame->canvas.Get(),style);
        auto toCopy=Transition(slot.frame->canvas.Get(),D3D12_RESOURCE_STATE_RENDER_TARGET,D3D12_RESOURCE_STATE_COPY_SOURCE);
        cmd->ResourceBarrier(1,&toCopy);
        }
        if (FAILED(cmd->Close())) return;
        ID3D12CommandList* lists[]={cmd}; queue->ExecuteCommandLists(1,lists);
        slot.fenceValue=++submitted;
        const bool signaled=SUCCEEDED(queue->Signal(fence.Get(),submitted));
        if (signaled)
            cvr::gpu::profile::Submitted(gpuProfile, queue, fence.Get(), submitted);
        slot.sources=source;
        slot.frame->stamp=source->stamp; slot.frame->generation=source->generation;
        slot.frame->serial=submitted;
        slot.frame->readyFence=fence;
        slot.frame->readyValue=signaled ? submitted : 0;
        slot.frame->masked=source->masked;
        slot.frame->lootVisible=source->lootVisible;
        if (ok && signaled) latest=slot.frame; else SetConsumerReady(false,channel);
        return;
    }
}
std::shared_ptr<Frame> Latest(Channel channel) {
    auto& [snapshotMutex,renderMutex,snapshot,latest,consumerStamp,slots,fence,deviceOwner,submitted]=pipelines[Index(channel)];
    std::lock_guard lock(renderMutex);
    return latest && GetTickCount64()-latest->stamp < 300 ? latest : nullptr;
}
void Shutdown(Channel channel) {
    auto& [snapshotMutex,renderMutex,snapshot,latest,consumerStamp,slots,fence,deviceOwner,submitted]=pipelines[Index(channel)];
    SetConsumerReady(false,channel);
    std::lock_guard lock(renderMutex);
    // Shutdown follows the manager's GPU drain. If the device still has pending work, retain
    // these bounded resources rather than release native heaps underneath a command list.
    if (fence && fence->GetCompletedValue()<submitted) return;
    latest.reset();
    for (auto& slot:slots) {
        slot.sources.reset(); slot.frame.reset(); slot.commands.Reset(); slot.allocator.Reset();
        slot.rtv.Reset(); slot.blit.Shutdown();slot.raw.Reset();slot.blur.Reset();slot.blurTemp.Reset();
        slot.frameConstants.Reset();slot.hudConstants.Reset();slot.exposure.Reset();
        if(slot.defaults && slot.defaultData)slot.defaults->Unmap(0,nullptr);
        slot.defaultData=nullptr;slot.defaults.Reset();slot.fenceValue=0;
    }
    fence.Reset(); deviceOwner.Reset(); submitted=0;
    std::lock_guard sourceLock(snapshotMutex); snapshot.reset();
}
}
