#pragma once
#include "ModUiApi.h"
#include <d3d12.h>
#include <wrl/client.h>
#include <utility>

namespace cvr::modui::client {
// GPU resources stay in the owning renderer. RenderFn records that renderer's
// own draw data and descriptor heap; no draw-list ABI is assumed by the host.
class Producer {
    Microsoft::WRL::ComPtr<ID3D12Device> device;
    Microsoft::WRL::ComPtr<ID3D12CommandAllocator> allocator;
    Microsoft::WRL::ComPtr<ID3D12GraphicsCommandList> list;
    Microsoft::WRL::ComPtr<ID3D12Fence> fence;
    uint64_t submitted{};
public:
    bool Busy()const{return fence && fence->GetCompletedValue()!=UINT64_MAX && fence->GetCompletedValue()<submitted;}
    bool Reset(){if(Busy())return false;list.Reset();allocator.Reset();fence.Reset();device.Reset();submitted=0;return true;}
    bool Drain(){
        if(Busy()){
            const auto event=CreateEventW(nullptr,FALSE,FALSE,nullptr);if(!event)return false;
            const auto hr=fence->SetEventOnCompletion(submitted,event);
            const auto result=SUCCEEDED(hr)?WaitForSingleObject(event,2000):WAIT_FAILED;CloseHandle(event);
            if(result!=WAIT_OBJECT_0)return false;
        }
        return Reset();
    }
    template<class RenderFn>
    bool Render(const CvrModUiApi* api,CvrModUiHandle source,ID3D12Device* d,ID3D12CommandQueue* queue,
                DXGI_FORMAT format,ID3D12DescriptorHeap* heap,RenderFn render){
        // false means desktop fallback; true also covers safely skipping a busy
        // VR frame. Never draw a VR-sized frame onto the game's SBS backbuffer.
        CvrModUiViewport viewport{sizeof(CvrModUiViewport)};
        if(!api || !source || !api->Viewport(source,&viewport))return Busy();
        if(!d || !queue || !heap || Busy())return true;
        if(device.Get()!=d){if(!Reset())return true;device=d;}
        if(!fence){
            if(FAILED(d->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&fence))) ||
               FAILED(d->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,IID_PPV_ARGS(&allocator))) ||
               FAILED(d->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_DIRECT,allocator.Get(),nullptr,IID_PPV_ARGS(&list)))){
                Reset();return true;
            }
            list->Close();
        }
        CvrModUiTarget target{sizeof(CvrModUiTarget)};
        if(!api->Begin(source,d,uint32_t(format),&target))return true;
        if(FAILED(allocator->Reset()) || FAILED(list->Reset(allocator.Get(),nullptr))){api->Cancel(source,target.lease);return true;}
        D3D12_RESOURCE_BARRIER barrier{};barrier.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        barrier.Transition={static_cast<ID3D12Resource*>(target.texture),D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
            D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,D3D12_RESOURCE_STATE_RENDER_TARGET};
        list->ResourceBarrier(1,&barrier);
        D3D12_CPU_DESCRIPTOR_HANDLE rtv{target.rtv};const float clear[4]{};
        list->ClearRenderTargetView(rtv,clear,0,nullptr);list->OMSetRenderTargets(1,&rtv,FALSE,nullptr);list->SetDescriptorHeaps(1,&heap);
        render(list.Get());
        std::swap(barrier.Transition.StateBefore,barrier.Transition.StateAfter);list->ResourceBarrier(1,&barrier);
        if(FAILED(list->Close())){api->Cancel(source,target.lease);return true;}
        ID3D12CommandList* commands[]{list.Get()};queue->ExecuteCommandLists(1,commands);
        // If signaling fails, do not Cancel a lease already referenced by GPU
        // work. The host quarantines it; it cannot re-enter the pool.
        if(SUCCEEDED(queue->Signal(fence.Get(),++submitted)))api->Submit(source,target.lease,fence.Get(),submitted);
        return true;
    }
};
}
