#include "Render/RegisteredConstantUpload.hpp"
#include "Render/NativeStereoFamilies.hpp"
#include "Render/NativeStereoProbe.hpp"
#include <array>
#include <mutex>
#include <intrin.h>

namespace cvr::stereo::constants {
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's registered queue order is rust/crates/cvr-render/src/ecl_table.rs'
std::recursive_mutex& RegisteredQueueOrderMutex(){static std::recursive_mutex mutex;return mutex;}
#endif
#if !CVR_RUST_VRIK   // the Rust build's observer: rust/crates/cvr-render/src/queue_api_observer.rs
namespace {
using QueueFn=HRESULT(STDMETHODCALLTYPE*)(ID3D12CommandQueue*,ID3D12Fence*,UINT64);
using FenceFn=HRESULT(STDMETHODCALLTYPE*)(ID3D12Fence*,UINT64);
struct QueueEntry {void** table{};QueueFn signal{},wait{};};
struct FenceEntry {void** table{};FenceFn signal{};};
std::array<QueueEntry,16> queues;
std::array<FenceEntry,16> fences;
std::atomic<UINT> queueCount{},fenceCount{};
std::mutex installMutex;
thread_local UINT queueApiDepth{};
struct QueueApiScope {QueueApiScope(){++queueApiDepth;}~QueueApiScope(){--queueApiDepth;}};
template<class T,size_t N>
const T* Find(void* object,const std::array<T,N>& entries,const std::atomic<UINT>& count) {
    auto** table=*reinterpret_cast<void***>(object);
    for(UINT i=0,n=count.load(std::memory_order_acquire);i<n;++i)if(entries[i].table==table)return &entries[i];
    return nullptr;
}
HRESULT STDMETHODCALLTYPE CpuSignal(ID3D12Fence* fence,UINT64 value) {
    const auto caller=reinterpret_cast<uintptr_t>(_ReturnAddress());
    const auto* entry=Find(fence,fences,fenceCount);if(!entry)return E_UNEXPECTED;
    if(!RegisteredTracking())return entry->signal(fence,value);
    std::lock_guard lock(RegisteredQueueOrderMutex());RegisterFence(fence);
    const auto result=entry->signal(fence,value);
    if(SUCCEEDED(result))RegisteredCpuSignal(fence,value,caller,queueApiDepth);
    return result;
}
void ObserveFenceApi(ID3D12Fence* fence) {
    if(!fence || Find(fence,fences,fenceCount))return;
    std::lock_guard lock(installMutex);
    if(Find(fence,fences,fenceCount))return;
    const auto n=fenceCount.load();if(n>=fences.size()){RegisteredCpuSignal(fence);return;}
    auto** table=*reinterpret_cast<void***>(fence);DWORD old{};
    if(!VirtualProtect(table+10,sizeof(void*),PAGE_READWRITE,&old)){RegisteredCpuSignal(fence);return;}
    fences[n]={table,reinterpret_cast<FenceFn>(table[10])};fenceCount.store(n+1,std::memory_order_release);
    InterlockedExchangePointer(reinterpret_cast<void* volatile*>(table+10),reinterpret_cast<void*>(&CpuSignal));
    DWORD ignored{};VirtualProtect(table+10,sizeof(void*),old,&ignored);
}
HRESULT STDMETHODCALLTYPE Signal(ID3D12CommandQueue* queue,ID3D12Fence* fence,UINT64 value) {
    const auto* entry=Find(queue,queues,queueCount);if(!entry)return E_UNEXPECTED;
    if(!RegisteredTracking())return entry->signal(queue,fence,value);
    std::lock_guard lock(RegisteredQueueOrderMutex());RegisterFence(fence);ObserveFenceApi(fence);
    QueueApiScope scope;const auto result=entry->signal(queue,fence,value);
    if(SUCCEEDED(result))RegisteredSignal(queue,fence,value);
    return result;
}
HRESULT STDMETHODCALLTYPE Wait(ID3D12CommandQueue* queue,ID3D12Fence* fence,UINT64 value) {
    const auto* entry=Find(queue,queues,queueCount);if(!entry)return E_UNEXPECTED;
    if(!RegisteredTracking())return entry->wait(queue,fence,value);
    std::lock_guard lock(RegisteredQueueOrderMutex());RegisterFence(fence);ObserveFenceApi(fence);
    QueueApiScope scope;const auto result=entry->wait(queue,fence,value);
    if(SUCCEEDED(result))RegisteredWait(queue,fence,value);
    return result;
}
}
void ObserveQueueApi(ID3D12CommandQueue* queue) {
    if(!queue || !native_probe::ObserveBindings() || !families::Enabled() || Find(queue,queues,queueCount))return;
    std::lock_guard lock(installMutex);
    if(Find(queue,queues,queueCount))return;
    const auto n=queueCount.load();if(n>=queues.size())return;
    auto** table=*reinterpret_cast<void***>(queue);DWORD old{};
    if(!VirtualProtect(table+14,2*sizeof(void*),PAGE_READWRITE,&old))return;
    queues[n]={table,reinterpret_cast<QueueFn>(table[14]),reinterpret_cast<QueueFn>(table[15])};queueCount.store(n+1,std::memory_order_release);
    InterlockedExchangePointer(reinterpret_cast<void* volatile*>(table+14),reinterpret_cast<void*>(&Signal));
    InterlockedExchangePointer(reinterpret_cast<void* volatile*>(table+15),reinterpret_cast<void*>(&Wait));
    DWORD ignored{};VirtualProtect(table+14,2*sizeof(void*),old,&ignored);
}
#endif
}
