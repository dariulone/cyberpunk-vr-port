#pragma once
#include "Framegen/GpuCorrelation.hpp"
#include <d3d12.h>
#include <wrl/client.h>
#include <array>
#include <memory>
#include <optional>
#include <algorithm>

namespace cvr::framegen {
struct SourceFrame;
// Two timestamp queries in the EXISTING capture list. No extra GPU submission
// or CPU wait. Caller pins heap/readback to command-list lifetime before Start.
struct CaptureTiming {
    Microsoft::WRL::ComPtr<ID3D12QueryHeap> heap;
    Microsoft::WRL::ComPtr<ID3D12Resource> readback;
    Microsoft::WRL::ComPtr<ID3D12Fence> fence;
    uint64_t value{};
    bool recorded{};
    GpuCorrelation clock;
    std::shared_ptr<const SourceFrame> source;
    struct Result { double queueMs{},spanMs{},totalMs{},endQpcMs{}; };
    bool Complete() const {return fence && value && fence->GetCompletedValue()!=UINT64_MAX && fence->GetCompletedValue()>=value;}
    void Start(ID3D12GraphicsCommandList* list) {
        recorded=true;clock={};fence.Reset();value=0;source.reset();
        list->EndQuery(heap.Get(),D3D12_QUERY_TYPE_TIMESTAMP,0);
    }
    void End(ID3D12GraphicsCommandList* list) {
        list->EndQuery(heap.Get(),D3D12_QUERY_TYPE_TIMESTAMP,1);
        list->ResolveQueryData(heap.Get(),D3D12_QUERY_TYPE_TIMESTAMP,0,2,readback.Get(),0);
    }
    void Enqueue(ID3D12CommandQueue* queue) {
        LARGE_INTEGER hz{},now{};QueryPerformanceFrequency(&hz);
        if(FAILED(queue->GetTimestampFrequency(&clock.gpuHz)) || !clock.gpuHz || hz.QuadPart<=0 ||
           FAILED(queue->GetClockCalibration(&clock.gpu,&clock.cpu))) {clock={};return;}
        clock.cpuHz=uint64_t(hz.QuadPart);QueryPerformanceCounter(&now);clock.submitted=uint64_t(now.QuadPart);
    }
    void Submitted(ID3D12Fence* f,uint64_t v) {fence=f;value=v;}
    std::optional<Result> Read() const {
        if(!Complete())return {};
        void* mapped{};D3D12_RANGE range{0,16};
        if(FAILED(readback->Map(0,&range,&mapped)))return {};
        const auto* p=static_cast<const uint64_t*>(mapped);const uint64_t start=p[0],end=p[1];
        D3D12_RANGE none{};readback->Unmap(0,&none);
        const auto r=clock.Resolve(start,end);if(!r)return {};
        return Result{r->queueMs,r->totalMs-r->queueMs,r->totalMs,
            double(clock.submitted)*1000.0/double(clock.cpuHz)+r->totalMs};
    }
};

// A bounded reusable pool. A published sample cannot be overwritten while the
// XR frame or generator still references it. Failed/aborted submissions stay
// quarantined; they cannot turn into unsafe query reuse or unbounded allocation.
class CaptureTimingPool {
    std::array<std::shared_ptr<CaptureTiming>,4> slots;
    Microsoft::WRL::ComPtr<ID3D12Device> device;
public:
    std::shared_ptr<CaptureTiming> Next(ID3D12Device* d) {
        if(!d)return {};
        if(device.Get()!=d){slots={};device=d;}
        for(auto& s:slots) {
            if(s && (s.use_count()!=1 || (s->recorded && !s->Complete())))continue;
            if(!s) {
                auto sample=std::make_shared<CaptureTiming>();
                D3D12_QUERY_HEAP_DESC q{};q.Type=D3D12_QUERY_HEAP_TYPE_TIMESTAMP;q.Count=2;
                D3D12_RESOURCE_DESC r{};r.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;r.Width=16;
                r.Height=r.DepthOrArraySize=r.MipLevels=1;r.SampleDesc.Count=1;r.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
                D3D12_HEAP_PROPERTIES h{};h.Type=D3D12_HEAP_TYPE_READBACK;
                if(FAILED(d->CreateQueryHeap(&q,IID_PPV_ARGS(&sample->heap))) ||
                   FAILED(d->CreateCommittedResource(&h,D3D12_HEAP_FLAG_NONE,&r,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&sample->readback))))return {};
                s=std::move(sample);
            }
            return s;
        }
        return {};
    }
};

struct CaptureStartAttribution {double beforeReadyMs{},afterReadyMs{};};
inline std::optional<CaptureStartAttribution> AttributeCaptureStart(
    double captureEndQpcMs,double generationEnqueueQpcMs,double generationStartQpcMs) {
    if(!std::isfinite(captureEndQpcMs) || !std::isfinite(generationEnqueueQpcMs) ||
       !std::isfinite(generationStartQpcMs) || generationStartQpcMs<generationEnqueueQpcMs ||
       captureEndQpcMs>generationStartQpcMs+0.25)return {};
    // Clock calibration across queues has finite precision. Reject >0.25ms
    // negative gaps; clamp only this tiny overlap, never call it negative wait.
    const auto total=generationStartQpcMs-generationEnqueueQpcMs;
    const auto pending=std::clamp(captureEndQpcMs-generationEnqueueQpcMs,0.0,total);
    return CaptureStartAttribution{pending,total-pending};
}
}
