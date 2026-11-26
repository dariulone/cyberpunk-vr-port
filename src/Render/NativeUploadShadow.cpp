#include "Render/NativeUploadShadow.hpp"
#if !CVR_RUST_VRIK
// The C++ build's watched buffer; the Rust build's is rust/crates/cvr-render/src/upload_shadow.rs (the header calls it).
#include <windows.h>
#include <d3d12.h>
#include <atomic>
#include <mutex>
#include <new>
namespace cvr::stereo::packets::upload_shadow {
namespace {
std::mutex mutex;
std::atomic<uint64_t> target{};
uint64_t resourceId{},size{};bool rejected{};UploadShadow shadow;
}
void Watch(uint64_t gpu,uint64_t resource,uint64_t bytes){
    std::lock_guard lock(mutex);target.store(0,std::memory_order_release);resourceId=0;size=0;rejected=false;
    if(gpu && resource && bytes && bytes<=UploadShadow::Limit){resourceId=resource;size=bytes;shadow.Reset(size);target.store(gpu,std::memory_order_release);}
    else shadow.Reset();
}
void Stop(){std::lock_guard lock(mutex);target.store(0,std::memory_order_release);shadow.Reset();}
bool Active(){return target.load(std::memory_order_relaxed)!=0;}
void Invalidate(ID3D12Resource* destination){
    if(!Active())return;std::lock_guard lock(mutex);
    if(reinterpret_cast<uintptr_t>(destination)==resourceId)shadow.Reset(size);
}
bool Read(uint64_t gpu,uint64_t resource,uint64_t offset,void* output,size_t bytes){
    std::lock_guard lock(mutex);
    return gpu && gpu==target.load(std::memory_order_relaxed) && resource==resourceId && !rejected && offset<=SIZE_MAX && shadow.Read(size_t(offset),output,bytes);
}
void Observe(ID3D12Resource* destination,uint64_t offset,ID3D12Resource* source,uint64_t sourceOffset,uint64_t bytes){
    if(!Active() || !destination || !source || !bytes)return;
    std::lock_guard lock(mutex);
    if(reinterpret_cast<uintptr_t>(destination)!=resourceId || rejected)return;
    try {
        const auto desc=destination->GetDesc();
        if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER || desc.Width!=size ||
            destination->GetGPUVirtualAddress()!=target.load(std::memory_order_relaxed) ||
            (desc.Flags&D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS)){
            rejected=true;shadow.Reset();return;
        }
        if(offset>size || bytes>size-offset){shadow.Reset(size);return;}
        const auto input=source->GetDesc();D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS flags{};
        if(input.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER || sourceOffset>input.Width || bytes>input.Width-sourceOffset ||
            FAILED(source->GetHeapProperties(&heap,&flags)) || heap.Type!=D3D12_HEAP_TYPE_UPLOAD){shadow.Invalidate(size_t(offset),size_t(bytes));return;}
        std::vector<uint8_t> owned(static_cast<size_t>(bytes));void* data{};
        const D3D12_RANGE range{SIZE_T(sourceOffset),SIZE_T(sourceOffset+bytes)};
        const auto mapped=source->Map(0,&range,&data);
        if(FAILED(mapped)){shadow.Invalidate(size_t(offset),size_t(bytes));return;}
        if(!data){const D3D12_RANGE none{};source->Unmap(0,&none);shadow.Invalidate(size_t(offset),size_t(bytes));return;}
        SIZE_T read{};const auto address=reinterpret_cast<uintptr_t>(data);
        const bool copied=address<=UINT64_MAX-sourceOffset && ReadProcessMemory(GetCurrentProcess(),
            reinterpret_cast<const void*>(address+sourceOffset),owned.data(),owned.size(),&read) && read==owned.size();
        const D3D12_RANGE none{};source->Unmap(0,&none);
        if(copied)shadow.Write(size_t(offset),owned);else shadow.Invalidate(size_t(offset),size_t(bytes));
    }catch(const std::bad_alloc&){rejected=true;shadow.Reset();}
}
}
#endif
