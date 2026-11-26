#include "Render/NativeMaterialBinding.hpp"
#include "Render/NativeStereoCameraUpload.hpp"
#include "Render/RenderProbeScope.hpp"
#include <windows.h>
#include <mutex>
#include <unordered_map>
#if CVR_RUST_VRIK
// The game-code contract: rust/crates/cvr-render/src/binding_code.rs (0 pair preparation, 1 material read, 2 material
// binding; through ReadProcessMemory).
#endif

#if CVR_RUST_VRIK
// The observations are rust/crates/cvr-render/src/material_observe.rs (the lease shared by layout with
// material_read.rs).
namespace cvr::stereo::material_binding {
extern "C" {
}
}
#else
namespace cvr::stereo::material_binding {
namespace {
struct Observation {
    Snapshot native;
    descriptors::CbvBinding cpu;
    geometry::BufferLease source;
    uint64_t serial{};
};
std::mutex mutex;
std::unordered_map<ID3D12GraphicsCommandList*,Observation> observations;
constexpr size_t Capacity=128;
uint64_t nextSerial{};
bool Read(void*,uint64_t address,void* output,size_t bytes){
    SIZE_T copied{};return address && address<=UINT64_MAX-bytes &&
        ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<const void*>(address),output,bytes,&copied) && copied==bytes;
}
bool Active(){return !probe::internalCommands && native_probe::ObserveBindings();}
bool SameDevice(ID3D12GraphicsCommandList* list,ID3D12Resource* resource){
    Microsoft::WRL::ComPtr<ID3D12Device> a,b;
    return list && resource && SUCCEEDED(list->GetDevice(IID_PPV_ARGS(&a))) &&
        SUCCEEDED(resource->GetDevice(IID_PPV_ARGS(&b))) && a==b;
}
bool SameCbv(const descriptors::CbvBinding& a,const descriptors::CbvBinding& b){
    return a.address==b.address && a.bytes==b.bytes;
}
}
bool Compatible(uintptr_t game){
    if(!game || game!=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr)))return false;
    static const bool compatible=[&]{
#if CVR_RUST_VRIK
        return cvr_binding_code(2,game);
#else
        struct Code{uint32_t rva,bytes;uint64_t hash;};
        constexpr Code expected[]{{0x336B54,0x1E7,0x664356FF35FEE308ull},
            {0x336D3C,0xF1,0x6B6B9044CD5F18EAull},
            {0x337230,0x33,0x96D1A06A9C2CBA7Full}};
        std::array<uint8_t,512> bytes{};
        for(const auto& code:expected){
            if(game>UINT64_MAX-code.rva || code.bytes>bytes.size() || !Read(nullptr,game+code.rva,bytes.data(),code.bytes))return false;
            uint64_t hash=14695981039346656037ull;for(UINT i=0;i<code.bytes;++i)hash=(hash^bytes[i])*1099511628211ull;
            if(hash!=code.hash)return false;
        }return true;
#endif
    }();return compatible;
}
void Reset(ID3D12GraphicsCommandList* list){
    decltype(observations)::node_type retired;
    {std::lock_guard lock(mutex);retired=observations.extract(list);}
}
void Clear(){
    decltype(observations) retired;
    {std::lock_guard lock(mutex);retired.swap(observations);}
}
Status ObserveBind(uintptr_t game,uint32_t binding,uint32_t handle,uint32_t stage)try{
    if(binding!=ShaderRegister || stage!=StageIndex || !Active())return Status::Ignored;
    if(!Compatible(game))return Status::UnsupportedExecutable;
    const auto context=camera_upload::CurrentContext(game);
    if(!context){Clear();return Status::MissingContext;}
    auto* list=reinterpret_cast<ID3D12GraphicsCommandList*>(context.list);
    Reset(list);
    Observation next;
    if(!detail::ReadSnapshot(Read,nullptr,game,context,handle,next.native))return Status::InvalidBinding;
    if(!descriptors::ResolveCpuCbvAddress(next.native.slot.descriptor,next.cpu) ||
        !detail::FitsCbv(next.native,next.cpu.address,next.cpu.bytes))return Status::DescriptorMismatch;
    const bool transient=next.native.record.kind==4;
    if(!transient){
        // Only this synchronous post-bind path can retain the native raw pointer.
        // Draw resolution below uses this retained resource, never a later lookup.
        next.source=geometry::AcquireBuffer(detail::Native(next.native),next.cpu.address,LogicalBytes);
        if(!next.source.resource || !SameDevice(list,next.source.resource.Get()) ||
            (next.source.heapType!=D3D12_HEAP_TYPE_DEFAULT && next.source.heapType!=D3D12_HEAP_TYPE_UPLOAD &&
             next.source.heapType!=D3D12_HEAP_TYPE_GPU_UPLOAD) || (next.source.flags&D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS))
            return Status::UnsupportedResource;
    }
    Snapshot again;descriptors::CbvBinding current;
    if(camera_upload::CurrentContext(game)!=context || !detail::ReadSnapshot(Read,nullptr,game,context,handle,again) ||
        next.native!=again || !descriptors::ResolveCpuCbvAddress(again.slot.descriptor,current) || !SameCbv(next.cpu,current))
        return Status::StaleObservation;
    {std::lock_guard lock(mutex);
        if(observations.contains(list))return Status::StaleObservation;
        if(observations.size()>=Capacity || nextSerial==UINT64_MAX)return Status::Capacity;
        next.serial=++nextSerial;observations.emplace(list,std::move(next));
    }
    return transient?Status::Transient:Status::Registered;
}catch(...){Clear();return Status::Failure;}
Status ResolveAtDraw(ID3D12GraphicsCommandList* list,uint64_t root,uint32_t parameter,uint32_t offset,Lease& out)try{
    out={};if(!Active())return Status::Ignored;
    const auto game=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    if(!Compatible(game))return Status::UnsupportedExecutable;
    const auto context=camera_upload::CurrentContext(game);
    if(!list || !root || !context || context.list!=reinterpret_cast<uintptr_t>(list))return Status::MissingContext;
    Observation saved;
    {std::lock_guard lock(mutex);const auto found=observations.find(list);
        if(found==observations.end())return Status::MissingObservation;saved=found->second;
    }
    if(saved.native.context!=context || !saved.serial)return Status::StaleObservation;
    Snapshot current;
    if(!detail::ReadSnapshot(Read,nullptr,game,context,saved.native.handle,current))return Status::StaleObservation;
    descriptors::CbvBinding cpu,actual;descriptors::DescriptorStamp stamp;
    if(!descriptors::ResolveCpuCbvAddress(current.slot.descriptor,cpu) || !SameCbv(cpu,saved.cpu) ||
        !descriptors::ResolveCbv(list,root,parameter,offset,actual) ||
        !detail::MatchesDraw(saved.native,current,parameter,offset,saved.cpu.address,saved.cpu.bytes,actual.address,actual.bytes) ||
        !descriptors::CaptureDescriptor(list,root,parameter,offset,stamp) || stamp.gpu!=actual.descriptor ||
        stamp.heap!=actual.heap || stamp.value[0]!=1 ||
        stamp.value[4]!=actual.address || stamp.value[5]!=actual.bytes)return Status::DescriptorMismatch;
    const bool transient=current.record.kind==4;
    uint64_t sourceOffset{};
    if(!transient){
        const auto& source=saved.source;
        if(!source.resource || reinterpret_cast<uintptr_t>(source.resource.Get())!=current.record.resource ||
            actual.address<source.gpu || actual.address-source.gpu>source.bytes ||
            LogicalBytes>source.bytes-(actual.address-source.gpu))return Status::StaleObservation;
        sourceOffset=actual.address-source.gpu;
    }
    Snapshot again;descriptors::CbvBinding finalCpu,finalGpu;
    if(camera_upload::CurrentContext(game)!=context || !detail::ReadSnapshot(Read,nullptr,game,context,saved.native.handle,again) ||
        again!=current || !descriptors::ResolveCpuCbvAddress(again.slot.descriptor,finalCpu) || !SameCbv(finalCpu,saved.cpu) ||
        !descriptors::ResolveCbv(list,root,parameter,offset,finalGpu) || finalGpu.descriptor!=actual.descriptor ||
        finalGpu.heap!=actual.heap || !SameCbv(finalGpu,actual) || !descriptors::MatchesDescriptor(stamp))return Status::StaleObservation;
    {std::lock_guard lock(mutex);const auto found=observations.find(list);
        if(found==observations.end() || found->second.serial!=saved.serial)return Status::StaleObservation;
    }
    out.native=current;out.binding=actual;out.stamp=stamp;out.source=std::move(saved.source);
    out.sourceOffset=sourceOffset;out.serial=saved.serial;
    return transient?Status::Transient:Status::Registered;
}catch(...){out={};return Status::Failure;}
}
#endif
