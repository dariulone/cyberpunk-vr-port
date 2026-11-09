#include "Framegen/SynthesisQueue.hpp"
#include <utility>

namespace cvr::framegen {
bool SynthesisQueue::Initialize(ID3D12Device* device,ID3D12CommandQueue* queue,bool isolated) {
    if(!device || !queue || queue->GetDesc().Type!=D3D12_COMMAND_LIST_TYPE_DIRECT)return false;
    producer=queue;execution=queue;fallbackValue=0;fallbackFence.Reset();creationResult=S_OK;
    if(!isolated)return true;
    D3D12_COMMAND_QUEUE_DESC desc{};
    desc.Type=D3D12_COMMAND_LIST_TYPE_DIRECT;
    desc.Priority=D3D12_COMMAND_QUEUE_PRIORITY_HIGH;
    desc.NodeMask=queue->GetDesc().NodeMask;
    Microsoft::WRL::ComPtr<ID3D12CommandQueue> candidate;
    creationResult=device->CreateCommandQueue(&desc,IID_PPV_ARGS(&candidate));
    if(FAILED(creationResult))return true; // same safe shared-queue fallback as V557
    creationResult=device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&fallbackFence));
    if(FAILED(creationResult))return true;
    candidate->SetName(L"CyberpunkVR SteamVR synthesis HIGH");
    fallbackFence->SetName(L"CyberpunkVR synthesis fallback input fence");
    execution=std::move(candidate);
    return true;
}
bool SynthesisQueue::Begin(ID3D12Fence* capture,uint64_t captureValue,
    ID3D12Fence* previousConsumer,uint64_t consumerValue) {
    if(!Private())return true;
    if(!capture || !captureValue){
        // Compatibility fallback for captures that don't carry a receipt.
        // Signal BEFORE any output join to prevent a cyclic queue dependency.
        const auto value=fallbackValue+1;
        if(FAILED(producer->Signal(fallbackFence.Get(),value)))return false;
        fallbackValue=value;capture=fallbackFence.Get();captureValue=value;
    }
    if(FAILED(execution->Wait(capture,captureValue)))return false;
    return WaitConsumer(previousConsumer,consumerValue);
}
bool SynthesisQueue::WaitConsumer(ID3D12Fence* fence,uint64_t value) {
    // Preserve the receipt even when it is moved after independent packing/OFA.
    // Output/history cannot be overwritten until the previous display is done.
    return !Private() || !fence || !value || SUCCEEDED(execution->Wait(fence,value));
}
bool SynthesisQueue::WaitInput(ID3D12Fence* fence,uint64_t value) {
    if(!Private())return true;
    return fence && value && SUCCEEDED(execution->Wait(fence,value));
}
bool SynthesisQueue::JoinOutput(ID3D12Fence* fence,uint64_t value,ID3D12CommandQueue* consumer) {
    auto* target=consumer?consumer:producer.Get();
    if(target==execution.Get() || !value)return true;
    return target && fence && SUCCEEDED(target->Wait(fence,value));
}
}
