#pragma once
#include <d3d12.h>
#include <cstdint>
#include <wrl/client.h>
#include <algorithm>
#include <utility>
#include <cmath>

namespace cvr::framegen {
struct ProducerReceipt {
    Microsoft::WRL::ComPtr<ID3D12Fence> fence;
    uint64_t value{};
};
// Present jobs may migrate between workers. Capture and the pacing call within
// ONE Present stay on its worker; never substitute the global newest receipt.
inline thread_local ProducerReceipt producerReceipt;
inline void BeginProducerFrame(){producerReceipt={};}
inline void PublishProducerCapture(ID3D12Fence* fence,uint64_t value){
    if(fence && value)producerReceipt={fence,value};
}
inline ProducerReceipt TakeProducerCapture(){return std::exchange(producerReceipt,{});}

enum class ProducerGateResult {Missing,Ready,Released,Timeout,WaitFailed,DeviceLost};
struct ProducerGateReading {ProducerGateResult result{};double elapsedMs{};unsigned waits{};};
inline double ProducerGateBudgetMs(int64_t physicalPeriodNs){
    if(physicalPeriodNs<4000000 || physicalPeriodNs>40000000)return 0;
    return std::clamp(double(physicalPeriodNs)*.5e-6,1.0,8.0);
}

inline bool WaitProducerSlice(HANDLE& timer,double ms) {
    if(!timer)timer=CreateWaitableTimerExW(nullptr,nullptr,CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,TIMER_ALL_ACCESS);
    if(!timer)return false;
    LARGE_INTEGER due{};due.QuadPart=-std::max<LONGLONG>(1,static_cast<LONGLONG>(ms*10000.0));
    if(!SetWaitableTimerEx(timer,&due,0,nullptr,nullptr,nullptr,0))return false;
    if(WaitForSingleObject(timer,2)!=WAIT_OBJECT_0){CancelWaitableTimer(timer);return false;}
    return true;
}

// Only called at the END of game Present pacing, outside XR and submission locks.
// Poll between short high-resolution timer waits, avoiding pending event callbacks
// or registrations after a timeout/device loss. No busy-spin or GPU queue wait.
template<class Now,class Wait>
ProducerGateReading GateProducer(const ProducerReceipt& receipt,double budgetMs,Now now,Wait wait){
    if(!receipt.fence || !receipt.value || !std::isfinite(budgetMs) || budgetMs<=0)
        return {ProducerGateResult::Missing};
    auto completed=receipt.fence->GetCompletedValue();
    if(completed==UINT64_MAX)return {ProducerGateResult::DeviceLost};
    if(completed>=receipt.value)return {ProducerGateResult::Ready};
    budgetMs=std::min(budgetMs,8.0);
    const double start=now();unsigned waits=0;
    for(;;){
        const double elapsed=std::max(0.0,now()-start);
        if(elapsed>=budgetMs || waits>=16)return {ProducerGateResult::Timeout,elapsed,waits};
        ++waits;
        if(!wait(std::min(.5,budgetMs-elapsed)))
            return {ProducerGateResult::WaitFailed,std::max(0.0,now()-start),waits};
        completed=receipt.fence->GetCompletedValue();
        if(completed==UINT64_MAX)return {ProducerGateResult::DeviceLost,std::max(0.0,now()-start),waits};
        if(completed>=receipt.value)return {ProducerGateResult::Released,std::max(0.0,now()-start),waits};
    }
}
}
