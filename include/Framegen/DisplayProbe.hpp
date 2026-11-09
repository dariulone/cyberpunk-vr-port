#pragma once
#include "Framegen/CaptureTiming.hpp"
#include "Framegen/DisplayDeadline.hpp"
namespace cvr::framegen {
// Single XR owner. Four slots, no waits/queue dependencies. Owner must drain
// the bound queue before destruction, or retain this pool with that queue.
class DisplayProbePool {
    struct Slot {
        Microsoft::WRL::ComPtr<ID3D12CommandAllocator> allocator;
        Microsoft::WRL::ComPtr<ID3D12GraphicsCommandList> list;
        std::shared_ptr<CaptureTiming> timing;
        DisplayDeadline deadline;
        bool occupied{};
    };
    std::array<Slot,4> slots;
    CaptureTimingPool timings;
    Microsoft::WRL::ComPtr<ID3D12Device> device;
    Microsoft::WRL::ComPtr<ID3D12Fence> fence;
    uint64_t sequence{};
public:
    DisplayDeadline* Begin(ID3D12CommandQueue* queue){
        if(!queue || queue->GetDesc().Type!=D3D12_COMMAND_LIST_TYPE_DIRECT)return nullptr;
        if(!device){
            if(FAILED(queue->GetDevice(IID_PPV_ARGS(&device))))return nullptr;
        }
        if(!fence && FAILED(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&fence))))return nullptr;
        for(auto& s:slots){
            if(s.occupied)continue;
            if(!s.allocator && FAILED(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,IID_PPV_ARGS(&s.allocator))))return nullptr;
            if(!s.list){
                if(FAILED(device->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_DIRECT,s.allocator.Get(),nullptr,IID_PPV_ARGS(&s.list))))return nullptr;
                if(FAILED(s.list->Close()))return nullptr;
            }
            if(FAILED(s.allocator->Reset()) || FAILED(s.list->Reset(s.allocator.Get(),nullptr)))return nullptr;
            s.timing=timings.Next(device.Get());if(!s.timing){s.list->Close();return nullptr;}
            s.deadline={};s.occupied=true;
            s.timing->Start(s.list.Get());s.timing->End(s.list.Get());
            if(FAILED(s.list->Close()))return nullptr; // quarantine failed slot
            s.timing->Enqueue(queue);
            ID3D12CommandList* lists[]{s.list.Get()};queue->ExecuteCommandLists(1,lists);
            const auto value=++sequence;
            if(FAILED(queue->Signal(fence.Get(),value)))return nullptr; // retain all in-flight objects
            s.timing->Submitted(fence.Get(),value);
            return &s.deadline;
        }
        return nullptr; // bounded skip instead of stealing a pending allocator/query
    }
    template<class Collect> void Poll(Collect collect){
        for(auto& s:slots){
            if(!s.occupied || !s.deadline.sealed || !s.timing || !s.timing->Complete())continue;
            collect(s.deadline,s.timing->Read());
            s.timing.reset();s.occupied=false;
        }
    }
};
}
