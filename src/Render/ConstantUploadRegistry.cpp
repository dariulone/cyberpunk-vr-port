#include "Render/ConstantUploadRegistry.hpp"
#include "Render/NativeStereoFamilies.hpp"
#include "Render/NativeStereoProbe.hpp"
#include "Render/RenderProbeScope.hpp"
#include "Render/ObservationProfile.hpp"
#include "Render/UploadTouchReceipt.hpp"
#include <wrl/client.h>
#include <map>
#include <mutex>
#include <memory>
#include <cstring>
#include <intrin.h>
#if !CVR_RUST_VRIK
extern "C" {
#endif
#if CVR_RUST_VRIK
// The exported state is Rust's (rust/crates/cvr-render/src/constant_registry_exports.rs), declared in the headers.
#else
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ConstantUploadCount{},CyberpunkVR_ConstantUploadFailures{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_ConstantUploadBytes{},CyberpunkVR_ConstantUploadReads{},CyberpunkVR_ConstantUploadMisses{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ConstantUploadEvictions{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ConstantUploadTypes[6]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ConstantUploadNativeImports{},CyberpunkVR_ConstantUploadNativeRejects{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ConstantRegisteredTypes[6]{},CyberpunkVR_ConstantRegisteredRejects{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ConstantUploadApiHits{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ConstantUploadFastPath{1};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_ConstantUploadFastStats[6]{};
__declspec(dllexport) uint32_t CyberpunkVR_ConstantUploadFastStatsCount=6;
#endif
#if !CVR_RUST_VRIK
}
#endif
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
// THE REGISTRY IS RUST'S (rust/crates/cvr-render/src/upload_registry.rs, its plugin table there); a Rust test variant
// hands it its reference exports while it initialises, with the descriptor families' switch and the observation
// profile's counters.
namespace {
struct CvrUploadRegistryAttach {
    std::atomic<uint32_t>* count;std::atomic<uint32_t>* failures;
    std::atomic<uint64_t>* bytes;std::atomic<uint64_t>* reads;std::atomic<uint64_t>* misses;
    std::atomic<uint32_t>* evictions;std::atomic<uint32_t>* types;
    std::atomic<uint32_t>* nativeImports;std::atomic<uint32_t>* nativeRejects;std::atomic<uint32_t>* apiHits;
    std::atomic<uint32_t>* fastPath;std::atomic<uint64_t>* fastStats;
    std::atomic<uint32_t>* profileEnable;std::atomic<uint64_t>* profileStats;
    bool (*familiesEnabled)();
};
static_assert(sizeof(CvrUploadRegistryAttach)==120,"mirrored by rust/crates/cvr-render/src/upload_registry.rs");
bool FamiliesEnabled(){return cvr::stereo::families::Enabled();}
const CvrUploadRegistryAttach kAttach{&CyberpunkVR_ConstantUploadCount,&CyberpunkVR_ConstantUploadFailures,
    &CyberpunkVR_ConstantUploadBytes,&CyberpunkVR_ConstantUploadReads,&CyberpunkVR_ConstantUploadMisses,
    &CyberpunkVR_ConstantUploadEvictions,CyberpunkVR_ConstantUploadTypes,
    &CyberpunkVR_ConstantUploadNativeImports,&CyberpunkVR_ConstantUploadNativeRejects,&CyberpunkVR_ConstantUploadApiHits,
    &CyberpunkVR_ConstantUploadFastPath,CyberpunkVR_ConstantUploadFastStats,
    &CyberpunkVR_ObservationProfileEnable,CyberpunkVR_ObservationProfileStats,&FamiliesEnabled};
}
extern "C" void cvr_upload_registry_attach(const CvrUploadRegistryAttach*);
namespace {
[[maybe_unused]] const bool kUploadRegistryAttached=(cvr_upload_registry_attach(&kAttach),true);
}
extern "C" {
void cvr_upload_created(ID3D12Resource*);
void cvr_upload_address_observed(ID3D12Resource*,uint64_t);
void cvr_upload_observe_resource_api(ID3D12Resource*);
void cvr_upload_touch(const D3D12_CONSTANT_BUFFER_VIEW_DESC*);
ID3D12Resource* cvr_upload_registered_source(uint32_t,uint32_t);
bool cvr_upload_read(uint64_t,void*,size_t,bool fixedOnly);
}
namespace cvr::stereo::constants {
void UploadCreated(ID3D12Resource* resource){cvr_upload_created(resource);}
void UploadAddressObserved(ID3D12Resource* resource,uint64_t address){cvr_upload_address_observed(resource,address);}
void ObserveResourceApi(ID3D12Resource* resource){cvr_upload_observe_resource_api(resource);}
void TouchUpload(const D3D12_CONSTANT_BUFFER_VIEW_DESC* desc){cvr_upload_touch(desc);}
ID3D12Resource* RegisteredSource(uint32_t handle,uint32_t size){return cvr_upload_registered_source(handle,size);}
bool ReadUpload(uint64_t address,void* output,size_t bytes){return cvr_upload_read(address,output,bytes,false);}
bool ReadFixedUpload(uint64_t address,void* output,size_t bytes){return cvr_upload_read(address,output,bytes,true);}
}
#endif
#else
namespace cvr::stereo::constants {
namespace {
struct Upload {
    Microsoft::WRL::ComPtr<ID3D12Resource> resource;
    uint8_t* mapped{};uint64_t bytes{};std::atomic<uint64_t> used{};
    const bool fixedHeap;
    explicit Upload(bool fixed):fixedHeap(fixed){}
    ~Upload(){if(mapped){const D3D12_RANGE none{};resource->Unmap(0,&none);}}
};
std::mutex mutex;
std::map<uint64_t,std::shared_ptr<Upload>> uploads;
std::atomic<uint64_t> clock{};
std::atomic<uint64_t> registryGeneration{1};
thread_local UploadTouchReceipt touchReceipt;
struct LookupHint {
    uint64_t base{},generation{};std::weak_ptr<Upload> owner;
    void Clear(){base=generation=0;owner.reset();}
    void Remember(uint64_t address,const std::shared_ptr<Upload>& value){
        base=address;generation=registryGeneration.load(std::memory_order_relaxed);owner=value;
    }
    std::shared_ptr<Upload> Lock(uint64_t address,uint64_t bytes,ID3D12Resource* expected=nullptr){
        if(!generation || generation!=registryGeneration.load(std::memory_order_acquire) || address<base || !bytes)return {};
        auto live=owner.lock();
        if(!live || (expected && (address!=base || live->resource.Get()!=expected)) ||
            address-base>live->bytes || bytes>live->bytes-(address-base) ||
            generation!=registryGeneration.load(std::memory_order_acquire))return {};
        return live;
    }
};
// Weak hints do not pin evicted resources between calls. A successful lock
// retains the existing owner only while updating its advisory LRU timestamp.
thread_local LookupHint lookupHint;
void Touch(Upload& upload){
    const auto now=clock.fetch_add(1,std::memory_order_relaxed)+1;
    auto previous=upload.used.load(std::memory_order_relaxed);
    while(previous<now && !upload.used.compare_exchange_weak(previous,now,std::memory_order_relaxed)){}
}
constexpr uint64_t ByteLimit=512*1024*1024;
using GpuAddressFn=D3D12_GPU_VIRTUAL_ADDRESS(STDMETHODCALLTYPE*)(ID3D12Resource*);
struct AddressHook {void** table{};GpuAddressFn original{};};
std::atomic<CreationAddressObserver> creationAddressObserver{};
std::array<AddressHook,16> addressHooks;
std::atomic<UINT> addressHookCount{};
std::mutex addressHookMutex;
static D3D12_GPU_VIRTUAL_ADDRESS STDMETHODCALLTYPE GpuAddress(ID3D12Resource* resource) {
    const auto caller=reinterpret_cast<uintptr_t>(_ReturnAddress());
    auto** table=*reinterpret_cast<void***>(resource);const auto count=addressHookCount.load(std::memory_order_acquire);
    GpuAddressFn original{};for(UINT i=0;i<count;++i)if(addressHooks[i].table==table){original=addressHooks[i].original;break;}
    if(!original)return 0;
    const auto value=original(resource);const auto game=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    if(value && (caller==game+0x33468F || caller==game+0x333BE9)) {
        if(const auto observe=creationAddressObserver.load(std::memory_order_acquire))observe(resource,value,caller);
        // The native uploader retains this API argument while it gets the VA.
        // Calls made by UploadCreated originate in the plugin and cannot recur.
        UploadAddressObserved(resource,value);++CyberpunkVR_ConstantUploadApiHits;
    }
    return value;
}
bool ReadMemory(uint64_t address,void* output,size_t bytes) {
    SIZE_T copied{};return address && address<=UINT64_MAX-bytes &&
        ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<const void*>(address),output,bytes,&copied) && copied==bytes;
}
struct NativeSource {ID3D12Resource* resource{};uint64_t cpu{},gpu{},bytes{};};
NativeSource NativeCbvSource(const D3D12_CONSTANT_BUFFER_VIEW_DESC& desc) {
    NativeSource result;
    __try {
        const auto game=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));CONTEXT context{};RtlCaptureContext(&context);
        ULONG_PTR low{},high{};GetCurrentThreadStackLimits(&low,&high);
        for(UINT depth=0;depth<24 && context.Rsp>=low && context.Rsp<=high-8;++depth) {
            uintptr_t resource{};uint64_t cpu{};
            // Exact native CreateCBV return sites, verified from the current EXE.
            // The uploader owns the allocation throughout this synchronous call.
            if(context.Rip==game+0x333C1A && context.Rsp<=high-0x50) {
                if(!ReadMemory(context.Rsp+0x30,&resource,8) || !ReadMemory(context.Rsp+0x48,&cpu,8))return {};
            } else if(context.Rip==game+0x3346B1) {
                if(context.R14>UINT64_MAX-0x14F2240 || !ReadMemory(context.R14+0x14F2240,&resource,8))return {};
                cpu=context.R15;
            }
            if(resource && cpu) {
                auto* object=reinterpret_cast<ID3D12Resource*>(resource);const auto d=object->GetDesc();const auto gpu=object->GetGPUVirtualAddress();
                if(d.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER || desc.BufferLocation<gpu || desc.BufferLocation-gpu>d.Width ||
                   desc.SizeInBytes>d.Width-(desc.BufferLocation-gpu))return {};
                D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS flags{};
                if(FAILED(object->GetHeapProperties(&heap,&flags)) || (heap.Type!=D3D12_HEAP_TYPE_UPLOAD && heap.Type!=D3D12_HEAP_TYPE_GPU_UPLOAD &&
                   !(heap.Type==D3D12_HEAP_TYPE_CUSTOM && (heap.CPUPageProperty==D3D12_CPU_PAGE_PROPERTY_WRITE_COMBINE || heap.CPUPageProperty==D3D12_CPU_PAGE_PROPERTY_WRITE_BACK))))return {};
                // Check identity against the uploader's CPU allocation, not merely
                // against a recycled GPU virtual address in a native weak table.
                void* mapped{};D3D12_RANGE range{SIZE_T(desc.BufferLocation-gpu),SIZE_T(desc.BufferLocation-gpu+desc.SizeInBytes)};
                const auto hr=object->Map(0,&range,&mapped);if(FAILED(hr))return {};
                const bool same=mapped && reinterpret_cast<uintptr_t>(mapped)<=UINT64_MAX-(desc.BufferLocation-gpu) &&
                    reinterpret_cast<uintptr_t>(mapped)+(desc.BufferLocation-gpu)==cpu;
                const D3D12_RANGE none{};object->Unmap(0,&none);
                return same?NativeSource{object,cpu,gpu,d.Width}:NativeSource{};
            }
            DWORD64 image{};const auto function=RtlLookupFunctionEntry(context.Rip,&image,nullptr);
            if(function){PVOID handler{};DWORD64 frame{};RtlVirtualUnwind(UNW_FLAG_NHANDLER,image,context.Rip,function,&context,&handler,&frame,nullptr);}
            else {if(!ReadMemory(context.Rsp,&context.Rip,8))return {};context.Rsp+=8;}
        }
    }__except(EXCEPTION_EXECUTE_HANDLER){}
    return result;
}
}
void SetCreationAddressObserver(CreationAddressObserver observer){creationAddressObserver.store(observer,std::memory_order_release);}
void ObserveResourceApi(ID3D12Resource* resource) {
    if(probe::internalCommands || !native_probe::ObserveBindings() || !families::Enabled() || !resource || resource->GetDesc().Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER)return;
    std::lock_guard lock(addressHookMutex);auto** table=*reinterpret_cast<void***>(resource);
    const auto count=addressHookCount.load(std::memory_order_relaxed);
    for(UINT i=0;i<count;++i)if(addressHooks[i].table==table)return;
    if(count>=addressHooks.size()){++CyberpunkVR_ConstantUploadFailures;return;}
    DWORD old{};if(!VirtualProtect(table+11,sizeof(void*),PAGE_READWRITE,&old)){++CyberpunkVR_ConstantUploadFailures;return;}
    addressHooks[count]={table,reinterpret_cast<GpuAddressFn>(table[11])};addressHookCount.store(count+1,std::memory_order_release);
    InterlockedExchangePointer(reinterpret_cast<void* volatile*>(table+11),reinterpret_cast<void*>(&GpuAddress));
    DWORD ignored{};VirtualProtect(table+11,sizeof(void*),old,&ignored);
}
ID3D12Resource* RegisteredSource(uint32_t handle,uint32_t size) {
    __try {
        uint64_t registry{};std::array<uint8_t,32> record{};
        const auto game=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
        if(!handle || handle>0x100000 || !size || size>65536 || !ReadMemory(game+0x3BCDCA8,&registry,8) ||
           !registry || registry>UINT64_MAX-0x5C0B18-uint64_t(handle-1)*0xB0 ||
           !ReadMemory(registry+uint64_t(handle-1)*0xB0+0x5C0AF8,record.data(),record.size()))return nullptr;
        uint32_t bytes{};uint64_t address{};ID3D12Resource* resource{};
        std::memcpy(&bytes,record.data(),4);std::memcpy(&address,record.data()+8,8);std::memcpy(&resource,record.data()+16,8);
        // This is the record the native BufUpload(handle, size, data) is about
        // to access, not a historical GPU-address entry. Low type 6 is CBV.
        if((record[6]&15)!=6 || size>bytes || !resource || !address)return nullptr;
        // Opaque identity only. Resource methods/retention are deferred until
        // CopyBufferRegion supplies the actual live destination API argument.
        return resource;
    }__except(EXCEPTION_EXECUTE_HANDLER){return nullptr;}
}
void UploadCreated(ID3D12Resource* resource) {
    if(probe::internalCommands || !native_probe::ObserveBindings() || !families::Enabled() || !resource)return;
    try {
        const auto desc=resource->GetDesc();if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER)return;
        D3D12_HEAP_PROPERTIES properties{};D3D12_HEAP_FLAGS flags{};
        if(FAILED(resource->GetHeapProperties(&properties,&flags)))return;
        if(UINT(properties.Type)<6)++CyberpunkVR_ConstantUploadTypes[UINT(properties.Type)];
        const bool fixedHeap=properties.Type==D3D12_HEAP_TYPE_UPLOAD || properties.Type==D3D12_HEAP_TYPE_GPU_UPLOAD;
        const bool cpuAccessible=fixedHeap ||
            (properties.Type==D3D12_HEAP_TYPE_CUSTOM && (properties.CPUPageProperty==D3D12_CPU_PAGE_PROPERTY_WRITE_COMBINE || properties.CPUPageProperty==D3D12_CPU_PAGE_PROPERTY_WRITE_BACK));
        if(!cpuAccessible || (desc.Flags&D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS))return;
        const auto address=resource->GetGPUVirtualAddress();
        if(!address || !desc.Width || desc.Width>ByteLimit || address>UINT64_MAX-desc.Width)return;
        std::vector<std::shared_ptr<Upload>> retired;
        std::lock_guard lock(mutex);
        const auto next=uploads.lower_bound(address);
        if(next!=uploads.end() && next->first==address && next->second->resource.Get()==resource)return;
        if((next!=uploads.end() && next->first<address+desc.Width) ||
           (next!=uploads.begin() && std::prev(next)->first+std::prev(next)->second->bytes>address)) {++CyberpunkVR_ConstantUploadFailures;return;}
        while(!uploads.empty() && (uploads.size()>=256 || desc.Width>ByteLimit-CyberpunkVR_ConstantUploadBytes.load())) {
            auto oldest=uploads.begin();for(auto it=uploads.begin();it!=uploads.end();++it)
                if(it->second->used.load(std::memory_order_relaxed)<oldest->second->used.load(std::memory_order_relaxed))oldest=it;
            CyberpunkVR_ConstantUploadBytes-=oldest->second->bytes;retired.push_back(std::move(oldest->second));uploads.erase(oldest);
            registryGeneration.fetch_add(1,std::memory_order_release);++CyberpunkVR_ConstantUploadEvictions;
        }
        auto owned=std::make_shared<Upload>(fixedHeap);owned->resource=resource;owned->bytes=desc.Width;
        void* mapped{};const D3D12_RANGE range{0,SIZE_T(desc.Width)};
        if(FAILED(resource->Map(0,&range,&mapped))){++CyberpunkVR_ConstantUploadFailures;return;}
        if(!mapped){const D3D12_RANGE none{};resource->Unmap(0,&none);++CyberpunkVR_ConstantUploadFailures;return;}
        owned->mapped=static_cast<uint8_t*>(mapped);Touch(*owned);uploads.emplace(address,std::move(owned));
        registryGeneration.fetch_add(1,std::memory_order_release);
        CyberpunkVR_ConstantUploadBytes+=desc.Width;CyberpunkVR_ConstantUploadCount=UINT(uploads.size());
    }catch(...){++CyberpunkVR_ConstantUploadFailures;}
}
void UploadAddressObserved(ID3D12Resource* resource,uint64_t address) {
    touchReceipt.Clear();
    if(probe::internalCommands || !native_probe::ObserveBindings() || !families::Enabled() || !resource || !address)return;
    if(CyberpunkVR_ConstantUploadFastPath.load(std::memory_order_relaxed)!=1){lookupHint.Clear();UploadCreated(resource);return;}
    if(auto owned=lookupHint.Lock(address,1,resource)){
        Touch(*owned);touchReceipt.Set(address,owned->bytes,lookupHint.generation);
        ++CyberpunkVR_ConstantUploadFastStats[0];++CyberpunkVR_ConstantUploadFastStats[5];return;
    }
    // The API supplies a live resource. Matching its retained registry owner
    // avoids repeating immutable resource/heap queries for every allocation.
    auto known=[&]{
        std::lock_guard lock(mutex);const auto found=uploads.find(address);
        if(found==uploads.end() || found->second->resource.Get()!=resource)return false;
        Touch(*found->second);lookupHint.Remember(address,found->second);
        return touchReceipt.Set(address,found->second->bytes,registryGeneration.load(std::memory_order_relaxed));
    };
    try{
        if(known()){++CyberpunkVR_ConstantUploadFastStats[0];return;}
        ++CyberpunkVR_ConstantUploadFastStats[1];UploadCreated(resource);if(!known())lookupHint.Clear();
    }catch(...){touchReceipt.Clear();++CyberpunkVR_ConstantUploadFailures;}
}
void TouchUpload(const D3D12_CONSTANT_BUFFER_VIEW_DESC* desc) {
    if(probe::internalCommands || !native_probe::ObserveBindings() || !families::Enabled() || !desc)return;
    observation_profile::Scope profile(observation_profile::Phase::TouchUpload);
    const bool fast=CyberpunkVR_ConstantUploadFastPath.load(std::memory_order_relaxed)==1;
    const bool touched=touchReceipt.Take(desc->BufferLocation,desc->SizeInBytes,registryGeneration.load(std::memory_order_acquire));
    if(fast && touched){++CyberpunkVR_ConstantUploadFastStats[2];return;}
    if(fast){if(auto owned=lookupHint.Lock(desc->BufferLocation,desc->SizeInBytes)){
        Touch(*owned);++CyberpunkVR_ConstantUploadFastStats[4];return;}}
    else lookupHint.Clear();
    ++CyberpunkVR_ConstantUploadFastStats[3];
    {
        std::lock_guard lock(mutex);auto found=uploads.upper_bound(desc->BufferLocation);
        if(found!=uploads.begin()){--found;const auto offset=desc->BufferLocation-found->first;auto& owned=*found->second;
            if(offset<=owned.bytes && desc->SizeInBytes<=owned.bytes-offset){Touch(owned);
                if(fast)lookupHint.Remember(found->first,found->second);return;}}
    }
    const auto source=[&]{observation_profile::Scope unwind(observation_profile::Phase::UnwindUpload);return NativeCbvSource(*desc);}();
    if(source.resource){UploadCreated(source.resource);++CyberpunkVR_ConstantUploadNativeImports;}
    else ++CyberpunkVR_ConstantUploadNativeRejects;
}
namespace {
bool ReadOwnedUpload(uint64_t address,void* output,size_t bytes,bool fixedOnly) {
    if(!address || !output || !bytes || bytes>65536)return false;std::lock_guard lock(mutex);
    auto found=uploads.upper_bound(address);
    if(found==uploads.begin()){++CyberpunkVR_ConstantUploadMisses;return false;}--found;
    const auto offset=address-found->first;const auto& owned=*found->second;
    if(offset>owned.bytes || bytes>owned.bytes-offset || (fixedOnly && !owned.fixedHeap)){++CyberpunkVR_ConstantUploadMisses;return false;}
    SIZE_T copied{};
    if(!ReadProcessMemory(GetCurrentProcess(),owned.mapped+offset,output,bytes,&copied) || copied!=bytes){++CyberpunkVR_ConstantUploadMisses;return false;}
    Touch(*found->second);++CyberpunkVR_ConstantUploadReads;return true;
}
}
bool ReadUpload(uint64_t address,void* output,size_t bytes){return ReadOwnedUpload(address,output,bytes,false);}
bool ReadFixedUpload(uint64_t address,void* output,size_t bytes){return ReadOwnedUpload(address,output,bytes,true);}
}
#endif
