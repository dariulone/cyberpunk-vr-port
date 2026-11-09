#pragma once
#include <d3d12.h>
#include <wrl/client.h>
#include <cstdint>

namespace cvr::framegen {
// Queue choice follows OFXR V557 e48d6fcf6289's native D3D12 synthesis
// queue (openxr_layer.cpp). Native capture/consumer synchronization is adapted
// here. Runtime binding/copy isolation lives in Runtimes/PresentationQueue.hpp.
// Derived component: LGPL-3.0, see licenses/OFXR-LGPL-3.0.txt.
class SynthesisQueue {
    Microsoft::WRL::ComPtr<ID3D12CommandQueue> producer,execution;
    Microsoft::WRL::ComPtr<ID3D12Fence> fallbackFence;
    uint64_t fallbackValue{};
    HRESULT creationResult{S_OK};
public:
    bool Initialize(ID3D12Device*,ID3D12CommandQueue*,bool isolated);
    bool Private() const {return execution && execution.Get()!=producer.Get();}
    ID3D12CommandQueue* Queue() const {return execution.Get();}
    ID3D12CommandQueue* Producer() const {return producer.Get();}
    HRESULT CreationResult() const {return creationResult;}
    // These are GPU queue waits, never CPU waits. Every waited value must
    // already have been successfully signaled by its producing queue.
    bool Begin(ID3D12Fence* capture,uint64_t captureValue,
        ID3D12Fence* previousConsumer,uint64_t consumerValue);
    bool WaitInput(ID3D12Fence*,uint64_t);
    // Optional previous display receipt; enqueue before overwriting shared output.
    bool WaitConsumer(ID3D12Fence*,uint64_t);
    bool JoinOutput(ID3D12Fence*,uint64_t,ID3D12CommandQueue* consumer=nullptr);
};
}
