#include "Render/NativeTextureRead.hpp"
#include "Render/NativeStereoCameraUpload.hpp"
#include "Render/CommandResources.hpp"
#include "Render/RenderProbeScope.hpp"
#include "Render/NativeStereoProbe.hpp"
#include "Render/NativeSourceCbvDetails.hpp"
#include "Render/NativeIaSnapshot.hpp"
#include "Render/NativeStructuredBufferSnapshot.hpp"
#include "Render/NativeTargetLifetime.hpp"
#include <windows.h>
#include <algorithm>
#include <atomic>
#include <map>
#include <mutex>
#include <vector>

#if CVR_RUST_VRIK
// THE RECEIPTS' REGISTRY IS RUST'S (rust/crates/cvr-render/src/texture_receipts.rs): the observers, the receipts'
// acquisition and their registry check; the rest of this module asks it.
extern "C" {
void cvr_tr_resource_created(ID3D12Resource*,uint32_t,bool,ID3D12Heap*,uint64_t,uint64_t);
bool cvr_tr_alias_affects_resources(ID3D12Resource*,ID3D12Resource*,ID3D12Resource*);
bool cvr_tr_query_observed_allocation(ID3D12Resource*,uint32_t*,void**,uint64_t*,uint64_t*);
bool cvr_tr_native_queue(uint64_t,ID3D12CommandQueue*,bool);
// The Lease path (rust/crates/cvr-render/src/texture_lease.rs) and the module's failure code.
uint32_t cvr_tr_failure();
void cvr_tr_set_failure(uint32_t);
void cvr_tr_lease_attach(void (*)(uint64_t,void*));
bool cvr_tr_identity(const void*);
uint64_t cvr_tr_fixed_owner_of(const void*);
bool cvr_tr_validate(const void*,ID3D12CommandQueue*);
bool cvr_tr_copy_subresource(ID3D12GraphicsCommandList*,const void*,UINT,ID3D12Resource*,const D3D12_PLACED_SUBRESOURCE_FOOTPRINT*);
// The immutable leases (rust/crates/cvr-render/src/texture_immutable.rs); the owner functions serve the fixed pins.
void cvr_tr_immutable_attach(bool(*)(ID3D12Resource*,void*),bool(*)(const void*,void*));
bool cvr_tr_validate_immutable(const void*,ID3D12CommandQueue*);
bool cvr_tr_immutable_pageable(ID3D12Device*,const void*,void*);
bool cvr_tr_register_immutable_use(ID3D12GraphicsCommandList*,const void*,void*);
bool cvr_tr_validate_immutable_use(const void*,ID3D12CommandQueue*);
// The fixed pins (rust/crates/cvr-render/src/texture_pins.rs).
void cvr_tr_pins_attach(bool(*)(uint64_t,uint64_t,bool,uint32_t*));
void cvr_tr_fixed_pin_details(std::array<uint64_t,8>*);
bool cvr_tr_pin_fixed_asset(ID3D12GraphicsCommandList*,uint64_t,void*);
bool cvr_tr_fixed_pinned_resource(const void*,ID3D12Resource**);
bool cvr_tr_acquire_pinned_immutable(ID3D12GraphicsCommandList*,const void*,void*);
// The writable identities (rust/crates/cvr-render/src/texture_writable.rs).
bool cvr_tr_validate_writable_target(ID3D12GraphicsCommandList*,ID3D12Resource*,uint32_t);
bool cvr_tr_capture_writable_identity(ID3D12GraphicsCommandList*,ID3D12Resource*,uint32_t,void*);
bool cvr_tr_validate_writable_identity(const void*,const void*,ID3D12CommandQueue*);
}
#endif
namespace cvr::stereo::texture_read {
namespace {
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
// The failure code is Rust's (texture_lease.rs): writes and reads of `failure` go through it.
struct FailureCode {
    FailureCode& operator=(uint32_t value){cvr_tr_set_failure(value);return *this;}
    operator uint32_t()const{return cvr_tr_failure();}
};
FailureCode failure;
static_assert(sizeof(Lease)==48 && offsetof(Lease,native)==8 && offsetof(Lease,receipt)==32 && offsetof(Lease,owner)==40 &&
    sizeof(Microsoft::WRL::ComPtr<ID3D12Resource>)==8,"mirrored by rust/crates/cvr-render/src/texture_lease.rs");
void CurrentContextForRust(uint64_t game,void* out){*static_cast<camera_upload::Context*>(out)=camera_upload::CurrentContext(uintptr_t(game));}
[[maybe_unused]] const bool kLeaseAttached=(cvr_tr_lease_attach(&CurrentContextForRust),true);
// texture_immutable.rs asks the creation receipts and the engine's placed offsets of the C++ (their records as POD:
// the creation's ComPtrs land zeroed-then-assigned in Rust's record and become its references).
static_assert(sizeof(constants::ResourceCreationInfo)==48 && offsetof(constants::ResourceCreationInfo,resource)==16 &&
    offsetof(constants::ResourceCreationInfo,heap)==24 && offsetof(constants::ResourceCreationInfo,heapOffset)==32 &&
    sizeof(ia_snapshot::detail::PlacedAllocation)==88 && sizeof(ImmutableLease)==56 && offsetof(ImmutableLease,source)==8 &&
    sizeof(ImmutablePageable)==64 && offsetof(ImmutablePageable,pageable)==56 && sizeof(ImmutableUse)==96 &&
    offsetof(ImmutableUse,recording)==56,"mirrored by rust/crates/cvr-render/src/texture_immutable.rs");
bool QueryCreationForRust(ID3D12Resource* resource,void* out){
    return constants::QueryResourceCreation(Microsoft::WRL::ComPtr<ID3D12Resource>(resource),*static_cast<constants::ResourceCreationInfo*>(out));
}
bool PlacedOffsetForRust(const void* wanted,void* out){
    return ia_snapshot::ReadPinnedPlacedOffset(*static_cast<const ia_snapshot::detail::PlacedAllocation*>(wanted),
        *static_cast<ia_snapshot::detail::PlacedAllocation*>(out));
}
[[maybe_unused]] const bool kImmutableAttached=(cvr_tr_immutable_attach(&QueryCreationForRust,&PlacedOffsetForRust),true);
#endif
#else
thread_local uint32_t failure{};
#endif
#if !CVR_RUST_VRIK  // the Rust build's pins: rust/crates/cvr-render/src/texture_pins.rs
thread_local std::array<uint64_t,8> fixedPinDetails{};
#endif
#if !CVR_RUST_VRIK
constexpr GUID nativeHandleKey{0xe9165914,0x664a,0x4174,{0xb1,0x1b,0x20,0x3a,0xad,0xd5,0x34,0x87}};
constexpr GUID placementKey{0xe9165914,0x664a,0x4174,{0xb1,0x1b,0x20,0x3a,0xad,0xd5,0x34,0x88}};
constexpr GUID heapOwnerKey{0xe9165914,0x664a,0x4174,{0xb1,0x1b,0x20,0x3a,0xad,0xd5,0x34,0x89}};
constexpr GUID receiptKey{0xe9165914,0x664a,0x4174,{0xb1,0x1b,0x20,0x3a,0xad,0xd5,0x34,0x8a}};
constexpr GUID recordingCookieKey{0xe9165914,0x664a,0x4174,{0xb1,0x1b,0x20,0x3a,0xad,0xd5,0x34,0x8b}};
constexpr GUID recordingLifetimeKey{0xe9165914,0x664a,0x4174,{0xb1,0x1b,0x20,0x3a,0xad,0xd5,0x34,0x8c}};
#endif
struct Placement {detail::Allocation allocation;uint64_t receipt{};};
#if !CVR_RUST_VRIK  // the Rust build's registry: rust/crates/cvr-render/src/texture_receipts.rs
struct Receipt;
std::atomic<Receipt*> retired{};
struct Receipt final:IUnknown {
    std::atomic<ULONG> refs{1};uint64_t id{};ID3D12Resource* resource{};Receipt* next{};
    Microsoft::WRL::ComPtr<ID3D12Heap> heap;
    Microsoft::WRL::ComPtr<ID3D12CommandQueue> uploadQueue;uint64_t registry{};
    detail::Allocation allocation;detail::CreationProof proof;
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID iid,void** output)override{
        if(!output)return E_POINTER;*output=nullptr;if(iid!=__uuidof(IUnknown))return E_NOINTERFACE;
        *output=this;AddRef();return S_OK;
    }
    ULONG STDMETHODCALLTYPE AddRef()override{return ++refs;}
    ULONG STDMETHODCALLTYPE Release()override{
        const auto count=--refs;if(!count){
            // Resource destruction may hold a D3D12 object lock. Retire without
            // taking our metadata lock or releasing the retained heap here.
            next=retired.load(std::memory_order_relaxed);
            while(!retired.compare_exchange_weak(next,this,std::memory_order_release,std::memory_order_relaxed)){}
        }return count;
    }
};
struct Recording;
std::atomic<Recording*> retiredRecordings{};
struct Recording final:IUnknown {
    std::atomic<ULONG> refs{1};uint64_t cookie{},generation{};ID3D12CommandList* list{};Recording* next{};
    buffer_read::detail::RecordingState state;std::vector<uint64_t> writes;
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID iid,void** output)override{
        if(!output)return E_POINTER;*output=nullptr;if(iid!=__uuidof(IUnknown))return E_NOINTERFACE;
        *output=this;AddRef();return S_OK;
    }
    ULONG STDMETHODCALLTYPE AddRef()override{return ++refs;}
    ULONG STDMETHODCALLTYPE Release()override{
        const auto count=--refs;if(!count){next=retiredRecordings.load(std::memory_order_relaxed);
            while(!retiredRecordings.compare_exchange_weak(next,this,std::memory_order_release,std::memory_order_relaxed)){}
        }return count;
    }
};
std::mutex receiptMutex;
std::map<uint64_t,Receipt*> receipts;
std::map<uint64_t,Recording*> recordings;
uint64_t nextReceipt{},nextRecordingCookie{},nextRecording{};
bool receiptFailure{};
constexpr size_t ReceiptLimit=16384;
constexpr size_t RecordingLimit=2048;
constexpr size_t WriteLimit=4096;
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
bool Observe(){return native_probe::ObserveBindings();}
#endif
#if !CVR_RUST_VRIK  // the Rust build's placements: texture_receipts.rs
Placement PlacementOf(ID3D12Resource* resource){
    Placement result{};UINT bytes=sizeof(result);
    if(!resource || FAILED(resource->GetPrivateData(placementKey,&bytes,&result)) || bytes!=sizeof(result))return {};
    return result;
}
#endif
#if !CVR_RUST_VRIK  // the Rust build's registry: rust/crates/cvr-render/src/texture_receipts.rs
void DrainRetired(){
    auto* receipt=retired.exchange(nullptr,std::memory_order_acquire);
    while(receipt){auto* next=receipt->next;const auto found=receipts.find(receipt->id);
        if(found!=receipts.end() && found->second==receipt)receipts.erase(found);
        delete receipt;receipt=next;}
    auto* recording=retiredRecordings.exchange(nullptr,std::memory_order_acquire);
    while(recording){auto* next=recording->next;const auto found=recordings.find(recording->cookie);
        if(found!=recordings.end() && found->second==recording){
            for(auto& [id,source]:receipts)source->proof.Abandon(recording->generation);
            recordings.erase(found);
        }
        delete recording;recording=next;
    }
}
Recording* RecordingOf(ID3D12CommandList* list){
    uint64_t cookie{};UINT bytes=sizeof(cookie);
    if(!list || FAILED(list->GetPrivateData(recordingCookieKey,&bytes,&cookie)) || bytes!=sizeof(cookie))return nullptr;
    const auto found=recordings.find(cookie);
    return found!=recordings.end() && found->second->list==list?found->second:nullptr;
}
#endif
#if CVR_RUST_VRIK
#else
void ReceiptError(){std::lock_guard lock(receiptMutex);receiptFailure=true;}
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
bool Read(void*,uint64_t address,void* output,size_t bytes){
    SIZE_T copied{};return address && address<=UINT64_MAX-bytes &&
        ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<void*>(address),output,bytes,&copied) && copied==bytes;
}
#endif
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
bool Identity(const NativeTexture& source){return cvr_tr_identity(&source);}
#endif
#else
bool Identity(const NativeTexture& source){
    const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    uint64_t registry{},resource{};
    return source.registry && source.resource && source.handle && source.handle<=32768 &&
        Read(nullptr,base+0x3bcdca8,&registry,8) && registry==source.registry &&
        source.registry<=UINT64_MAX-0x2f1e0-uint64_t(source.handle-1)*0xb0 &&
        Read(nullptr,source.registry+0x2f1d8+uint64_t(source.handle-1)*0xb0,&resource,8) && resource==source.resource;
}
#endif
#if CVR_RUST_VRIK
#else
bool ResolveNative(ID3D12Resource* resource,NativeTexture& native){
    const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));uint64_t registry{};
    if(!resource || !Read(nullptr,base+0x3bcdca8,&registry,8) || !registry)return false;
    UINT bytes=sizeof(native);
    if(SUCCEEDED(resource->GetPrivateData(nativeHandleKey,&bytes,&native)) && bytes==sizeof(native) && native.registry==registry &&
        native.resource==reinterpret_cast<uintptr_t>(resource) && Identity(native))return true;
    constexpr size_t size=32768*0xb0;std::vector<uint8_t> records(size);
    failure=3;if(registry>UINT64_MAX-0x2f1d8-size || !Read(nullptr,registry+0x2f1d8,records.data(),records.size()))return false;
    native={registry,reinterpret_cast<uintptr_t>(resource),0};
    for(UINT i=0;i<32768;++i){uint64_t candidate{};std::memcpy(&candidate,records.data()+size_t(i)*0xb0,8);
        if(candidate==native.resource){if(native.handle){failure=4;return false;}native.handle=i+1;}}
    failure=5;if(!Identity(native))return false;
    return SUCCEEDED(resource->SetPrivateData(nativeHandleKey,sizeof(native),&native));
}
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
bool Shape(const D3D12_RESOURCE_DESC& desc){return detail::Shape(desc);}
#endif
#if !CVR_RUST_VRIK  // the Rust build's layouts: texture_receipts.rs
bool DefaultHeap(ID3D12Resource* resource){
    D3D12_HEAP_PROPERTIES properties{};D3D12_HEAP_FLAGS flags{};
    return SUCCEEDED(resource->GetHeapProperties(&properties,&flags)) && properties.Type==D3D12_HEAP_TYPE_DEFAULT;
}
bool MipLayout(ID3D12Device* device,const D3D12_RESOURCE_DESC& desc,UINT mip,uint64_t offset,MipFootprint& output){
    return detail::SubresourceLayout(device,desc,mip,offset,output);
}
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
bool SameLayout(const D3D12_PLACED_SUBRESOURCE_FOOTPRINT& a,const D3D12_PLACED_SUBRESOURCE_FOOTPRINT& b){
    return a.Offset==b.Offset && a.Footprint.Format==b.Footprint.Format && a.Footprint.Width==b.Footprint.Width &&
        a.Footprint.Height==b.Footprint.Height && a.Footprint.Depth==b.Footprint.Depth && a.Footprint.RowPitch==b.Footprint.RowPitch;
}
#endif
#if CVR_RUST_VRIK
#else
bool FixedCompatible(){
    static const bool compatible=[] {
        const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
        struct Code {uint32_t rva,bytes;uint64_t hash;};
        constexpr Code expected[]{
            {0x346f24,771,0x51ba1810b87b452eull},{0x5ea284,196,0x511481cf9505a352ull},
            {0x625a58,834,0x3e4654fab947d49full},{0x347f74,687,0x0eaa63353e6cd86cull},
            {0x3490f4,98,0xaf5dd974e8f5ed23ull},{0x346374,1076,0x36978fd0f3168dceull},
            {0x342f48,1188,0xadfee15f3177a181ull},{0x3e5d54,1789,0x63d0eefeac6254d8ull},
            {0x3e8a9c,55,0xc10a06ab43ea88f1ull},{0x3e7b90,180,0x22067cf1c84d3113ull},
            {0x5b3da8,641,0x5e94d16a5dbeca40ull},{0x5b4110,77,0x9e7d6243f6f033c4ull},
            {0x22ef830,1470,0x3541631d8d75a2e6ull},{0x3e1020,682,0x898f20179715110full},
            {0x3e47e8,371,0x936d5b6c41cb0a2eull},{0x3e12cc,148,0xa99eac2c327a1a01ull},
            {0x3e1abc,97,0xf0a1f30d3afc8031ull},{0x22F09C0,0x3F,0x0000000000000000ull},  // 2.5 rewrite of 2.31 sub_14021EB24 (8 call sites agree), not re-audited
           
            {0x34b588,346,0x6e1062b700b72556ull},{0x34c16c,438,0xf20d07f3c00d184full}
        };
        std::array<uint8_t,2048> bytes{};
        for(const auto code:expected){if(code.bytes>bytes.size() || !Read(nullptr,base+code.rva,bytes.data(),code.bytes))return false;
            uint64_t hash=14695981039346656037ull;for(UINT i=0;i<code.bytes;++i)hash=(hash^bytes[i])*1099511628211ull;
            if(hash!=code.hash)return false;}
        return true;
    }();return compatible;
}
#endif
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's texture_pins.rs reads the chains itself (target_lifetime.rs)
// texture_pins.rs's FixedPinCompatible asks the resize / shutdown hook chains of target_lifetime (C++ for now).
bool ChainForRust(uint64_t base,uint64_t cet,bool shutdown,uint32_t* rejected){
    target_lifetime::detail::ResizeChain chain;
    return shutdown?target_lifetime::detail::ReadShutdownChain(Read,nullptr,base,cet,chain,rejected):
        target_lifetime::detail::ReadResizeChain(Read,nullptr,base,cet,chain,rejected);
}
[[maybe_unused]] const bool kPinsAttached=(cvr_tr_pins_attach(&ChainForRust),true);
#endif
#else
bool FixedPinCompatible(){
    struct Result {bool accepted{};uint64_t rva{},expected{},actual{};};
    static const Result result=[] {
        const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
        constexpr native_recording::detail::Code expected[]{
            {0x5D99F4,100,0x416099FA1DEC4511ull},{0x347EEC,133,0xDFEDCE894319584Eull},
            {0x2FA5EE0,135,0x38FCAFE70967B57Aull},{0x34C3C0,806,0x9667FCCC34D1C033ull},
            {0x34C39C,36,0x41112018CADE440Dull},{0x34C33C,94,0x56F46708A50F91CDull},
            {0x22EE570,0x72,0x997908A535F51556ull},{0x34C320,27,0x23EB44C7C2F87CDEull},
            {0x8EC794,410,0x17CC8F1F467EDA14ull},{0x5A914C,182,0xA08B256E1A287BACull},
            {0x5D9A58,73,0x6A803BDA1A688053ull},{0x34C7A0,97,0xB4D21C2B77A7C288ull},
            {0x688788,1302,0x55625DB1357B5061ull},{0x22CA300,0x117,0xF6E2B8EDCE8E3545ull},
            {0x3E3248,145,0x61B5A10A99C316E9ull},{0x22CA420,0x118,0xDCAECFC3F6C5E70Dull},
            {0xBF4624,175,0xFAA8CC8CC5084187ull},{0x5B1938,195,0x773420FC3024FE47ull},
            {0x2FAAF80,13,0x4B71F66010D54C4Dull},{0x348220,33,0xEFE63006A4D426CBull},
            {0x348244,195,0x8DFB4CD8294CBA43ull},{0x3483A4,33,0x16D17AF39F50AAA4ull},
            {0x18F3FB4,105,0x8550A7D22883C599ull},{0x18F3F48,52,0x95650F6E1318A268ull},
            {0x2FAC534,57,0x2BA6EF10E4DE61C9ull},{0x349CFC,224,0x44B98676710DAFF2ull},
            {0x34A688,1018,0x48C1DFD837BDA037ull},{0x348548,532,0x8DA5EE9F82249354ull},
            {0x2458C6A,96,0xAD6AE86DE1275D37ull},{0x68AF98,278,0xE36CFE953900198Eull}
        };
        std::array<uint8_t,1536> bytes{};
        for(const auto code:expected){
            if(code.bytes>bytes.size() || !Read(nullptr,base+code.rva,bytes.data(),code.bytes))return Result{false,code.rva,code.hash,0};
            const auto actual=native_recording::detail::Fingerprint(std::span(bytes.data(),code.bytes));
            if(actual!=code.hash)return Result{false,code.rva,code.hash,actual};
        }
        return Result{true};
    }();
    if(!result.accepted){fixedPinDetails[0]=result.rva;fixedPinDetails[6]=result.expected;fixedPinDetails[7]=result.actual;return false;}
    const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    const auto cet=reinterpret_cast<uintptr_t>(GetModuleHandleW(L"cyber_engine_tweaks.asi"));
    target_lifetime::detail::ResizeChain chain;uint32_t rejected{};
    if(!target_lifetime::detail::ReadResizeChain(Read,nullptr,base,cet,chain,&rejected)){
        fixedPinDetails[0]=target_lifetime::detail::ResizeRva;fixedPinDetails[7]=rejected;return false;
    }
    if(!target_lifetime::detail::ReadShutdownChain(Read,nullptr,base,cet,chain,&rejected)){
        fixedPinDetails[0]=target_lifetime::detail::ShutdownRva;fixedPinDetails[7]=rejected;return false;
    }
    return true;
}
// Numeric hints only. Every cached handle is checked anew under the native lock;
// neither an old registry snapshot nor a recycled resource address grants a pin.
std::mutex fixedCandidateMutex;
uint64_t fixedCandidateRegistry{};
std::map<uint64_t,uint32_t> fixedCandidates;
bool FindFixedCandidate(uint64_t registry,uint64_t resource,NativeTexture& output){
    output={};constexpr size_t count=32768,stride=0xB0;
    if(!registry || !resource || registry>UINT64_MAX-0x2F1D8-count*stride)return false;
    std::lock_guard lock(fixedCandidateMutex);
    if(fixedCandidateRegistry==registry){const auto found=fixedCandidates.find(resource);
        if(found!=fixedCandidates.end() && found->second){uint64_t current{};
            if(Read(nullptr,registry+0x2F1D8+uint64_t(found->second-1)*stride,&current,8) && current==resource){
                output={registry,resource,found->second};return true;
            }
        }
    }
    std::vector<uint8_t> records(count*stride);
    if(!Read(nullptr,registry+0x2F1D8,records.data(),records.size()))return false;
    std::map<uint64_t,uint32_t> next;
    for(uint32_t i=0;i<count;++i){uint64_t candidate{};std::memcpy(&candidate,records.data()+size_t(i)*stride,8);
        if(candidate){const auto [entry,inserted]=next.emplace(candidate,i+1);if(!inserted)entry->second=0;}}
    fixedCandidates=std::move(next);fixedCandidateRegistry=registry;
    const auto found=fixedCandidates.find(resource);if(found==fixedCandidates.end() || !found->second)return false;
    output={registry,resource,found->second};return true;
}
struct FixedBootstrapPacket {
    uint64_t base{};Microsoft::WRL::ComPtr<IUnknown> allocator,allocation;
};
bool TryFixedLock(void*,uint64_t address)noexcept{
    return _InterlockedCompareExchange8(reinterpret_cast<volatile char*>(address),-1,0)==0;
}
bool RetainFixedHandle(void*,uint64_t address,int32_t expected)noexcept{
    return expected>0 && expected<INT32_MAX-1 &&
        _InterlockedCompareExchange(reinterpret_cast<volatile long*>(address),expected+1,expected)==expected;
}
bool RetainFixedOwner(void* opaque,uint64_t address,bool allocation)noexcept{
    auto& packet=*static_cast<FixedBootstrapPacket*>(opaque);auto& held=allocation?packet.allocation:packet.allocator;
    if(held || !address)return false;
    using AddRef=ULONG(*)(void*);
    const auto count=reinterpret_cast<AddRef>(packet.base+0x2FAAF80)(reinterpret_cast<void*>(address));
    // Even a refused post-increment count remains owned until AFTER unlock.
    held.Attach(reinterpret_cast<IUnknown*>(address));return count>1 && count<INT32_MAX;
}
void ReleaseFixedHandle(void* opaque,uint32_t handle)noexcept{
    using Release=uint32_t(*)(uint32_t*);const auto& packet=*static_cast<FixedBootstrapPacket*>(opaque);
    reinterpret_cast<Release>(packet.base+0x3E3248)(&handle);
}
void UnlockFixed(void*,uint64_t address)noexcept{
    _InterlockedExchange8(reinterpret_cast<volatile char*>(address),0);
}
#endif
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
uint64_t FixedOwner(const NativeTexture& source){return cvr_tr_fixed_owner_of(&source);}
#endif
#else
uint64_t FixedOwner(const NativeTexture& source){
    if(!Identity(source) || source.registry>UINT64_MAX-0x2f1d8-uint64_t(source.handle-1)*0xb0-0x58)return 0;
    std::array<uint8_t,0x58> record{};uint64_t resource{};
    if(!Read(nullptr,source.registry+0x2f1d8+uint64_t(source.handle-1)*0xb0,record.data(),record.size()))return 0;
    std::memcpy(&resource,record.data(),8);
    return resource==source.resource?detail::FixedOwner(record):0;
}
#endif
#if CVR_RUST_VRIK
#else
bool FixedContext(const camera_upload::Context& context){
    uint32_t kind{},pending{};uint64_t list{};
    return context && context.context<=UINT64_MAX-0x538 && Read(nullptr,context.context+0x68,&kind,4) && (kind==1 || kind==2) &&
        Read(nullptr,context.context+0x30,&list,8) && list==context.list && Read(nullptr,context.context+0x534,&pending,4) && !pending;
}
#endif
#if !CVR_RUST_VRIK  // the Rust build's registry: rust/crates/cvr-render/src/texture_receipts.rs
struct UploadOrigin {uint64_t registry{};Microsoft::WRL::ComPtr<ID3D12CommandQueue> queue;};
UploadOrigin InitialUploader(ID3D12GraphicsCommandList* list){
    if(!list || list->GetType()!=D3D12_COMMAND_LIST_TYPE_COPY || !FixedCompatible())return {};
    const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));uint64_t registry{},context{},nativeList{},queue{};uint32_t kind{};
    if(!Read(nullptr,base+0x3bcdca8,&registry,8) || !registry || registry>UINT64_MAX-0x1410638 ||
        !Read(nullptr,registry+0x1410628,&context,8) || !context || context>UINT64_MAX-0x6c ||
        !Read(nullptr,context+0x30,&nativeList,8) || nativeList!=reinterpret_cast<uintptr_t>(list) ||
        !Read(nullptr,context+0x68,&kind,4) || kind!=3 || !Read(nullptr,registry+0x1410630,&queue,8) || !queue)return {};
    UploadOrigin origin;origin.registry=registry;origin.queue=reinterpret_cast<ID3D12CommandQueue*>(queue);
    return origin.queue->GetDesc().Type==D3D12_COMMAND_LIST_TYPE_COPY?std::move(origin):UploadOrigin{};
}
#endif
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
bool NativeQueue(uint64_t registry,ID3D12CommandQueue* queue,bool upload){return cvr_tr_native_queue(registry,queue,upload);}
#endif
#else
bool NativeQueue(uint64_t registry,ID3D12CommandQueue* queue,bool upload){
    const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));uint64_t current{},expected{};
    const uint64_t offset=upload?0x1410630:0x14104d0;
    return registry && queue && registry<=UINT64_MAX-offset-8 && Read(nullptr,base+0x3bcdca8,&current,8) && current==registry &&
        Read(nullptr,registry+offset,&expected,8) && expected==reinterpret_cast<uintptr_t>(queue) &&
        queue->GetDesc().Type==(upload?D3D12_COMMAND_LIST_TYPE_COPY:D3D12_COMMAND_LIST_TYPE_DIRECT);
}
#endif
#if !CVR_RUST_VRIK  // the Rust build acquires in texture_receipts.rs
uint64_t AcquireReceipt(ID3D12Resource* resource,uint64_t registry){
    const auto placement=PlacementOf(resource);std::lock_guard lock(receiptMutex);DrainRetired();
    const auto found=receipts.find(placement.receipt);
    failure=60;if(!detail::KnownAllocation(placement.allocation) || !placement.receipt)return 0;
    failure=61;if(receiptFailure)return 0;
    failure=62;if(found==receipts.end())return 0;
    failure=63;if(found->second->resource!=resource)return 0;
    failure=64;if(found->second->registry!=registry)return 0;
    failure=65;if(!NativeQueue(registry,found->second->uploadQueue.Get(),true))return 0;
    failure=66;if(!found->second->proof.valid)return 0;
    failure=67;if(!found->second->proof.uploaded || found->second->proof.pending)return 0;
    failure=68;if(!found->second->proof.Acquire())return 0;
    return placement.receipt;
}
#endif
}
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
void ResourceCreated(ID3D12Resource* resource,D3D12_RESOURCE_STATES initialState,bool placed,ID3D12Heap* heap,
                     uint64_t offset,uint64_t allocationBytes){
    cvr_tr_resource_created(resource,uint32_t(initialState),placed,heap,offset,allocationBytes);
}
#endif
#else
void ResourceCreated(ID3D12Resource* resource,D3D12_RESOURCE_STATES initialState,bool placed,ID3D12Heap* heap,
                     uint64_t offset,uint64_t allocationBytes){
    if(!Observe() || !resource)return;
    try{
        std::lock_guard lock(receiptMutex);DrainRetired();if(PlacementOf(resource).allocation.kind)return;
        Placement placement{};placement.allocation.kind=placed?2:1;
        if(placed && heap && SUCCEEDED(resource->SetPrivateDataInterface(heapOwnerKey,heap)))
            placement.allocation={reinterpret_cast<uintptr_t>(heap),offset,allocationBytes,2};
        Microsoft::WRL::ComPtr<Receipt> receipt;
        if(!probe::internalCommands && !receiptFailure && initialState==D3D12_RESOURCE_STATE_COMMON && detail::ImmutableShape(resource->GetDesc()) && DefaultHeap(resource) &&
            detail::KnownAllocation(placement.allocation) && receipts.size()<ReceiptLimit && nextReceipt!=UINT64_MAX){
            receipt.Attach(new Receipt);receipt->id=++nextReceipt;receipt->resource=resource;receipt->heap=heap;
            receipt->allocation=placement.allocation;receipt->proof.valid=true;
            if(SUCCEEDED(resource->SetPrivateDataInterface(receiptKey,receipt.Get())))placement.receipt=receipt->id;
        }
        if(FAILED(resource->SetPrivateData(placementKey,sizeof(placement),&placement)))return;
        if(placement.receipt)receipts.emplace(placement.receipt,receipt.Get());
    }catch(...){ReceiptError();}
}
#endif
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
bool AliasAffects(ID3D12Resource* watched,ID3D12Resource* before,ID3D12Resource* after){
    return cvr_tr_alias_affects_resources(watched,before,after);
}
#endif
#else
bool AliasAffects(ID3D12Resource* watched,ID3D12Resource* before,ID3D12Resource* after)try{
    if(!watched || !Observe())return true;
    std::lock_guard lock(receiptMutex);if(receiptFailure)return true;
    const auto source=PlacementOf(watched),from=PlacementOf(before),to=PlacementOf(after);
    return detail::AliasAffects(reinterpret_cast<uintptr_t>(watched),source.allocation,
        reinterpret_cast<uintptr_t>(before),from.allocation,reinterpret_cast<uintptr_t>(after),to.allocation);
}catch(...){return true;}
#endif
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
bool QueryObservedAllocation(const Microsoft::WRL::ComPtr<ID3D12Resource>& heldResource,constants::ResourceCreationInfo& output)try{
    // The held resource may be the output's own (an aliased query): keep it before clearing the output.
    Microsoft::WRL::ComPtr<ID3D12Resource> resource=heldResource;output={};uint32_t kind{};void* heap{};uint64_t offset{},bytes{};
    if(!cvr_tr_query_observed_allocation(resource.Get(),&kind,&heap,&offset,&bytes))return false;
    constants::ResourceCreationInfo result;result.kind=kind;result.resource=resource;
    result.heap.Attach(static_cast<ID3D12Heap*>(heap));result.heapOffset=offset;result.allocationBytes=bytes;
    output=std::move(result);return true;
}catch(...){output={};return false;}
#endif
#else
bool QueryObservedAllocation(const Microsoft::WRL::ComPtr<ID3D12Resource>& heldResource,constants::ResourceCreationInfo& output)try{
    Microsoft::WRL::ComPtr<ID3D12Resource> resource=heldResource;output={};
    if(!resource || !Observe())return false;
    std::lock_guard lock(receiptMutex);if(receiptFailure)return false;
    const auto saved=PlacementOf(resource.Get());const auto& allocation=saved.allocation;
    if(!detail::KnownAllocation(allocation))return false;
    constants::ResourceCreationInfo result;result.kind=allocation.kind;result.resource=resource;
    if(allocation.kind==1){if(allocation.heap || allocation.offset || allocation.bytes)return false;}
    else{
        IUnknown* raw{};UINT bytes=sizeof(raw);
        if(FAILED(resource->GetPrivateData(heapOwnerKey,&bytes,&raw)) || bytes!=sizeof(raw) || !raw)return false;
        Microsoft::WRL::ComPtr<IUnknown> owner;owner.Attach(raw);
        if(FAILED(owner.As(&result.heap)) || reinterpret_cast<uintptr_t>(result.heap.Get())!=allocation.heap)return false;
        Microsoft::WRL::ComPtr<ID3D12Device> resourceDevice,heapDevice;
        if(FAILED(resource->GetDevice(IID_PPV_ARGS(&resourceDevice))) || !resourceDevice ||
            FAILED(result.heap->GetDevice(IID_PPV_ARGS(&heapDevice))) || resourceDevice!=heapDevice)return false;
        const auto heap=result.heap->GetDesc();const auto desc=resource->GetDesc();
        const auto required=resourceDevice->GetResourceAllocationInfo(0,1,&desc);
        if(allocation.offset>heap.SizeInBytes || allocation.bytes>heap.SizeInBytes-allocation.offset ||
            !required.SizeInBytes || required.SizeInBytes==UINT64_MAX || !required.Alignment ||
            allocation.offset%required.Alignment || allocation.bytes<required.SizeInBytes)return false;
        result.heapOffset=allocation.offset;result.allocationBytes=allocation.bytes;
    }
    const auto current=PlacementOf(resource.Get());
    if(current.receipt!=saved.receipt || current.allocation.heap!=allocation.heap || current.allocation.offset!=allocation.offset ||
        current.allocation.bytes!=allocation.bytes || current.allocation.kind!=allocation.kind)return false;
    output=std::move(result);return true;
}catch(...){output={};return false;}
#endif
#if CVR_RUST_VRIK
#else
void Barriers(ID3D12GraphicsCommandList*,UINT count,const D3D12_RESOURCE_BARRIER* barriers){
    if(!Observe() || !count)return;
    if(!barriers){ReceiptError();return;}
    try{
        std::lock_guard lock(receiptMutex);DrainRetired();if(receipts.empty())return;
        for(UINT i=0;i<count;++i){const auto& barrier=barriers[i];
            if(barrier.Type==D3D12_RESOURCE_BARRIER_TYPE_TRANSITION){
                if(!barrier.Transition.pResource){receiptFailure=true;return;}
                const auto placement=PlacementOf(barrier.Transition.pResource);const auto found=receipts.find(placement.receipt);
                if(found!=receipts.end())found->second->proof.Invalidate();
            }else if(barrier.Type==D3D12_RESOURCE_BARRIER_TYPE_ALIASING){
                const auto& alias=barrier.Aliasing;const auto before=PlacementOf(alias.pResourceBefore),after=PlacementOf(alias.pResourceAfter);
                for(auto& [id,receipt]:receipts){
                    if(detail::AliasAffects(reinterpret_cast<uintptr_t>(receipt->resource),receipt->allocation,
                        reinterpret_cast<uintptr_t>(alias.pResourceBefore),before.allocation,
                        reinterpret_cast<uintptr_t>(alias.pResourceAfter),after.allocation))receipt->proof.Invalidate();
                }
            }
        }
    }catch(...){ReceiptError();}
}
#endif
#if CVR_RUST_VRIK
#else
void Written(ID3D12GraphicsCommandList* list,ID3D12Resource* destination){
    if(!Observe())return;
    if(!destination){ReceiptError();return;}
    try{
        const auto placement=PlacementOf(destination);std::lock_guard lock(receiptMutex);DrainRetired();
        for(auto& [id,receipt]:receipts){
            if(receipt->resource!=destination && detail::CouldAlias(receipt->allocation,placement.allocation))receipt->proof.Invalidate();
        }
        const auto found=receipts.find(placement.receipt);
        if(found==receipts.end() || found->second->resource!=destination)return;
        auto& receipt=*found->second;if(!receipt.proof.valid)return;
        const auto origin=InitialUploader(list);auto* recording=RecordingOf(list);
        if(!origin.queue || !recording || !recording->state.valid || recording->state.pending || recording->state.submitted ||
            (receipt.registry && (receipt.registry!=origin.registry || receipt.uploadQueue!=origin.queue))){
            receipt.proof.Invalidate();return;
        }
        if(std::find(recording->writes.begin(),recording->writes.end(),receipt.id)==recording->writes.end()){
            if(recording->writes.size()>=WriteLimit){receiptFailure=true;return;}
            recording->writes.push_back(receipt.id);
        }
        receipt.registry=origin.registry;receipt.uploadQueue=origin.queue;receipt.proof.Written(recording->generation);
    }catch(...){ReceiptError();}
}
#endif
#if CVR_RUST_VRIK
#else
void Reset(ID3D12GraphicsCommandList* list){
    if(!Observe() || !list || list->GetType()!=D3D12_COMMAND_LIST_TYPE_COPY)return;
    try{
        std::lock_guard lock(receiptMutex);DrainRetired();auto* recording=RecordingOf(list);
        if(recording){
            for(auto& [id,receipt]:receipts)receipt->proof.Abandon(recording->generation);
            if(nextRecording==UINT64_MAX){recording->generation=0;receiptFailure=true;return;}
            recording->generation=++nextRecording;recording->state={};recording->writes.clear();return;
        }
        if(recordings.size()>=RecordingLimit || nextRecordingCookie==UINT64_MAX || nextRecording==UINT64_MAX){receiptFailure=true;return;}
        Microsoft::WRL::ComPtr<Recording> created;created.Attach(new Recording);
        created->cookie=++nextRecordingCookie;created->generation=++nextRecording;created->list=list;
        if(FAILED(list->SetPrivateDataInterface(recordingLifetimeKey,created.Get())) ||
            FAILED(list->SetPrivateData(recordingCookieKey,sizeof(created->cookie),&created->cookie))){receiptFailure=true;return;}
        recordings.emplace(created->cookie,created.Get());
    }catch(...){ReceiptError();}
}
#endif
#if CVR_RUST_VRIK
#else
void BeforeSubmit(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists){
    if(!Observe() || !count)return;
    if(!queue || !lists){ReceiptError();return;}
    try{
        std::lock_guard lock(receiptMutex);DrainRetired();
        for(UINT i=0;i<count;++i){auto* recording=RecordingOf(lists[i]);if(!recording || !recording->generation)continue;
            const bool fresh=recording->state.BeforeSubmit();
            for(const auto id:recording->writes){const auto found=receipts.find(id);if(found==receipts.end())continue;
                auto& receipt=*found->second;
                receipt.proof.BeforeSubmit(recording->generation,fresh && receipt.uploadQueue.Get()==queue && NativeQueue(receipt.registry,queue,true));
            }
        }
    }catch(...){ReceiptError();}
}
#endif
#if CVR_RUST_VRIK
#else
void Submitted(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists){
    if(!Observe() || !count)return;
    if(!queue || !lists){ReceiptError();return;}
    try{
        std::lock_guard lock(receiptMutex);DrainRetired();
        for(UINT i=0;i<count;++i){auto* recording=RecordingOf(lists[i]);if(!recording || !recording->generation)continue;
            const bool fresh=recording->state.Submitted();
            for(const auto id:recording->writes){const auto found=receipts.find(id);if(found==receipts.end())continue;
                auto& receipt=*found->second;
                receipt.proof.Submitted(recording->generation,fresh && receipt.uploadQueue.Get()==queue && NativeQueue(receipt.registry,queue,true));
            }
        }
    }catch(...){ReceiptError();}
}
#endif
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
bool Validate(const Lease& source,ID3D12CommandQueue* submissionQueue){return cvr_tr_validate(&source,submissionQueue);}
#endif
#else
bool Validate(const Lease& source,ID3D12CommandQueue* submissionQueue){
    if(!source.resource || source.native.resource!=reinterpret_cast<uintptr_t>(source.resource.Get()) || !Identity(source.native))return false;
    if(!source.receipt)return true;
    if(!FixedCompatible() || !source.owner || FixedOwner(source.native)!=source.owner ||
        (submissionQueue && !NativeQueue(source.native.registry,submissionQueue,false)))return false;
    const auto placement=PlacementOf(source.resource.Get());std::lock_guard lock(receiptMutex);DrainRetired();
    const auto found=receipts.find(source.receipt);
    return !receiptFailure && placement.receipt==source.receipt && found!=receipts.end() &&
        found->second->resource==source.resource.Get() && found->second->registry==source.native.registry &&
        found->second->proof.valid && found->second->proof.acquired && found->second->proof.uploaded && !found->second->proof.pending;
}
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
uint32_t LastFailure(){return failure;}
#endif
#if CVR_RUST_VRIK
#else
void ObserverUnavailable(){ReceiptError();}
#endif
#if CVR_RUST_VRIK
#else
bool Compatible(){
    static const bool result=[] {
        const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
        struct Code {uint32_t rva,bytes;uint64_t hash;};
        constexpr Code expected[]{{0x337230,0x33,0x96d1a06a9c2cba7full},{0x337264,0x11a,0xbbe79ba5988d6982ull},
            {0x337380,0x506,0x1ca82dd09817c1a3ull},{0x3e3bb4,0x36,0xf999b875f95ad550ull}};
        std::array<uint8_t,1536> bytes{};
        for(const auto code:expected){if(!Read(nullptr,base+code.rva,bytes.data(),code.bytes))return false;
            uint64_t hash=14695981039346656037ull;for(UINT i=0;i<code.bytes;++i)hash=(hash^bytes[i])*1099511628211ull;
            if(hash!=code.hash)return false;}
        return true;
    }();return result;
}
#endif
#if CVR_RUST_VRIK
#else
bool Acquire(ID3D12GraphicsCommandList* list,ID3D12Resource* resource,Lease& output)try{
    output={};failure=1;if(!list || !resource || !Compatible() || list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT || !Shape(resource->GetDesc()))return false;
    const auto desc=resource->GetDesc();
    failure=69;if(detail::AlphaFormat(desc.Format) && (!DefaultHeap(resource) || !detail::KnownAllocation(PlacementOf(resource).allocation)))return false;
    const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));const auto context=camera_upload::CurrentContext(base);
    failure=2;if(context.list!=reinterpret_cast<uintptr_t>(list))return false;
    NativeTexture native{};if(!ResolveNative(resource,native))return false;
    const auto state=ReadState(native,context.context,context.list,Read);
    if(state.status==8){
        failure=50;if(!FixedCompatible())return false;
        const auto owner=FixedOwner(native);failure=51;if(!owner)return false;
        failure=52;if(!DefaultHeap(resource))return false;
        failure=56;if(!FixedContext(context) || camera_upload::CurrentContext(base)!=context)return false;
        failure=53;const auto receipt=AcquireReceipt(resource,native.registry);if(!receipt)return false;
        output.resource=resource;output.native=native;output.receipt=receipt;output.owner=owner;
        failure=54;if(!Validate(output)){output={};return false;}
        failure=0;return true;
    }
    failure=10+state.status;
    if(detail::AlphaFormat(desc.Format)){
        if(state.status!=1 || !(state.value&detail::ShaderReads))return false;
    }else if(!detail::SourceReadState(state,UINT(desc.MipLevels)*desc.DepthOrArraySize))return false;
    output.resource=resource;output.native=native;failure=0;return true;
}catch(...){failure=6;return false;}
#endif
#if !CVR_RUST_VRIK
namespace {
#endif
#if !CVR_RUST_VRIK
constexpr GUID immutableOwnerType{0x22c496a1,0x7e0b,0x45f7,{0x94,0x9a,0x42,0x11,0xc7,0x09,0x80,0x31}};
struct ImmutableOwner final:IUnknown {
    std::atomic<ULONG> refs{1};Microsoft::WRL::ComPtr<IUnknown> allocatorPin,allocationPin;
    Lease source;constants::ResourceCreationInfo creation;
    source_cbv_details::Allocation allocation;source_cbv_details::Allocator allocator;
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** out)override{if(!out)return E_POINTER;*out=nullptr;
        if(id!=__uuidof(IUnknown) && id!=immutableOwnerType)return E_NOINTERFACE;*out=this;AddRef();return S_OK;}
    ULONG STDMETHODCALLTYPE AddRef()override{return ++refs;}
    ULONG STDMETHODCALLTYPE Release()override{const auto left=--refs;if(!left)delete this;return left;}
};
bool SameAllocation(const source_cbv_details::Allocation& a,const source_cbv_details::Allocation& b){
    using O=source_cbv_details::Observation;return a.observation==O::Known && b.observation==O::Known && a.address==b.address &&
        a.vtable==b.vtable && a.pimpl==b.pimpl && a.bytes==b.bytes && a.resource==b.resource && a.heap==b.heap &&
        a.block==b.block && a.handle==b.handle && a.kind==b.kind;
}
bool Physical(const ImmutableOwner& owner){
    using O=source_cbv_details::Observation;const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    const auto& saved=owner.source;constants::ResourceCreationInfo current;
    if(!owner.allocationPin || !owner.allocatorPin || !constants::QueryResourceCreation(saved.resource,current))return false;
    const auto& expected=owner.creation;
    if(current.resource!=saved.resource || current.resource!=expected.resource || current.kind!=expected.kind || current.heap!=expected.heap ||
        current.heapOffset!=expected.heapOffset || current.allocationBytes!=expected.allocationBytes ||
        !current.initialStateKnown || current.initialState!=D3D12_RESOURCE_STATE_COMMON)return false;
    const auto allocation=source_cbv_details::detail::ReadAllocation(Read,nullptr,base,reinterpret_cast<uintptr_t>(owner.allocationPin.Get()));
    const auto allocator=source_cbv_details::detail::ReadAllocator(Read,nullptr,base,saved.native.registry,allocation);
    if(!SameAllocation(owner.allocation,allocation) || allocation.address!=saved.owner || allocation.resource!=saved.native.resource ||
        allocator.observation!=O::Known || !allocator.pimplMatches || allocator.address!=reinterpret_cast<uintptr_t>(owner.allocatorPin.Get()) ||
        allocator.address!=owner.allocator.address || allocator.vtable!=owner.allocator.vtable || allocator.pimpl!=owner.allocator.pimpl)return false;
    if(current.kind==1)return allocation.kind==0 && !allocation.heap && !current.heap && !current.heapOffset && !current.allocationBytes;
    if(current.kind!=2 || allocation.kind!=1 || !current.heap || allocation.heap!=reinterpret_cast<uintptr_t>(current.heap.Get()) ||
        !current.allocationBytes || allocation.bytes!=current.allocationBytes)return false;
    const auto heap=current.heap->GetDesc();
    if(heap.Properties.Type!=D3D12_HEAP_TYPE_DEFAULT || current.heapOffset>heap.SizeInBytes || current.allocationBytes>heap.SizeInBytes-current.heapOffset)return false;
    ia_snapshot::detail::PlacedAllocation offset;
    const ia_snapshot::detail::PlacedAllocation wanted{allocation.address,allocation.resource,allocation.pimpl,allocation.block,allocation.handle,allocation.heap,allocation.bytes};
    return ia_snapshot::ReadPinnedPlacedOffset(wanted,offset) && offset.offset==current.heapOffset;
}
bool Unprotected(ID3D12Resource* resource){
    Microsoft::WRL::ComPtr<ID3D12Resource1> extended;Microsoft::WRL::ComPtr<IUnknown> session;
    const auto query=resource->QueryInterface(IID_PPV_ARGS(&extended));if(query==E_NOINTERFACE)return true;
    if(FAILED(query) || !extended)return false;
    const auto result=extended->GetProtectedResourceSession(IID_PPV_ARGS(&session));
    return structured_buffer_snapshot::detail::NoProtectedSession(result,bool(session));
}
#endif
#if !CVR_RUST_VRIK
}
#endif
struct FixedAssetPins::Data {
#if CVR_RUST_VRIK
    Microsoft::WRL::ComPtr<IUnknown> held;  // texture_immutable.rs's owner (texture_pins.rs writes the record)
#else
    Microsoft::WRL::ComPtr<ImmutableOwner> held;
#endif
    camera_upload::Context context;native_recording::Lease recording;
};
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
std::array<uint64_t,8> FixedPinLastDetails(){std::array<uint64_t,8> details{};cvr_tr_fixed_pin_details(&details);return details;}
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
bool PinFixedAsset(ID3D12GraphicsCommandList* list,uint64_t candidate,FixedAssetPins& output){
    static_assert(sizeof(FixedAssetPins::Data)==72 && offsetof(FixedAssetPins::Data,context)==8 && offsetof(FixedAssetPins::Data,recording)==32,
        "mirrored by PinsData in rust/crates/cvr-render/src/texture_pins.rs");
    output={};auto data=std::make_shared<FixedAssetPins::Data>();
    if(!cvr_tr_pin_fixed_asset(list,candidate,data.get()))return false;
    output.data_=std::move(data);return true;
}
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
bool GetFixedPinnedResource(const FixedAssetPins& pins,Microsoft::WRL::ComPtr<ID3D12Resource>& output){
    output.Reset();return cvr_tr_fixed_pinned_resource(pins.data_.get(),output.GetAddressOf());
}
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
bool AcquirePinnedImmutable(ID3D12GraphicsCommandList* list,const FixedAssetPins& pins,ImmutableLease& output){
    output={};return cvr_tr_acquire_pinned_immutable(list,pins.data_.get(),&output);
}
#endif
#else
std::array<uint64_t,8> FixedPinLastDetails(){return fixedPinDetails;}
bool PinFixedAsset(ID3D12GraphicsCommandList* list,uint64_t candidate,FixedAssetPins& output)try{
    output={};fixedPinDetails={};failure=120;
    if(!list || !candidate || !Observe() || probe::internalCommands || list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT)return false;
    failure=121;if(!Compatible() || !FixedCompatible() || !FixedPinCompatible())return false;
    const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));const auto context=camera_upload::CurrentContext(base);
    native_recording::Lease recording;failure=122;
    if(context.list!=reinterpret_cast<uintptr_t>(list) || !FixedContext(context) || !native_recording::Capture(list,context,recording))return false;
    NativeTexture native;failure=123;if(!FindFixedCandidate(recording.registry,candidate,native))return false;
    fixedPinDetails={native.registry,native.handle,native.resource,0,0,0,0,0};
    FixedBootstrapPacket packet;packet.base=base;detail::FixedPinSnapshot snapshot;uint32_t rejected{};
    const detail::FixedPinOperations ops{Read,TryFixedLock,RetainFixedHandle,RetainFixedOwner,ReleaseFixedHandle,UnlockFixed};
    failure=124;
    if(!detail::BootstrapFixedPins(ops,&packet,base,native,snapshot,rejected)){
        fixedPinDetails[5]=rejected;
        if(rejected==3){
            // POD-only failure evidence after unlock, never an ownership grant.
            const auto record=native.registry+0x2F1D8+uint64_t(native.handle-1)*0xB0;
            std::array<uint8_t,0x60> first{},second{};
            if(Read(nullptr,record-8,first.data(),first.size()) && Read(nullptr,record-8,second.data(),second.size()) && first==second){
                uint32_t refs{},binding{};uint16_t flags{};uint64_t resource{},owner{};
                std::memcpy(&refs,first.data(),4);std::memcpy(&resource,first.data()+8,8);std::memcpy(&owner,first.data()+0x18,8);
                std::memcpy(&binding,first.data()+0x50,4);std::memcpy(&flags,first.data()+0x5E,2);
                fixedPinDetails[3]=resource;fixedPinDetails[4]=uint64_t(first[0x10])|(uint64_t(flags)<<32);
                fixedPinDetails[6]=owner;fixedPinDetails[7]=uint64_t(refs)|(uint64_t(binding)<<32);
            }
        }
        return false;
    }
    fixedPinDetails[3]=snapshot.allocation.address;fixedPinDetails[4]=snapshot.allocator.address;
    failure=125;
    const auto current=source_cbv_details::detail::ReadAllocation(Read,nullptr,base,reinterpret_cast<uintptr_t>(packet.allocation.Get()));
    if(!SameAllocation(snapshot.allocation,current) || !packet.allocator || current.resource!=candidate)return false;
    // The pinned native allocation owns this resource (8780C8 releases +0x28
    // only at allocation destruction). No driver/interposer call ran under lock.
    Microsoft::WRL::ComPtr<ID3D12Resource> resource=reinterpret_cast<ID3D12Resource*>(current.resource);
    Microsoft::WRL::ComPtr<ImmutableOwner> owner;owner.Attach(new ImmutableOwner);
    owner->allocatorPin=std::move(packet.allocator);owner->allocationPin=std::move(packet.allocation);
    owner->source={resource,native,0,snapshot.allocation.address};owner->allocation=snapshot.allocation;owner->allocator=snapshot.allocator;
    failure=126;
    if(!constants::QueryResourceCreation(resource,owner->creation) || !Physical(*owner.Get()) || !Identity(native) ||
        FixedOwner(native)!=snapshot.allocation.address || camera_upload::CurrentContext(base)!=context || !FixedContext(context) ||
        !native_recording::ValidateRecording(recording,list,context))return false;
    auto data=std::make_shared<FixedAssetPins::Data>();data->held=std::move(owner);data->context=context;data->recording=std::move(recording);
    output.data_=std::move(data);failure=0;return true;
}catch(...){output={};failure=134;return false;}
bool GetFixedPinnedResource(const FixedAssetPins& pins,Microsoft::WRL::ComPtr<ID3D12Resource>& output){
    output.Reset();failure=127;if(!pins.data_ || !pins.data_->held || !pins.data_->held->source.resource)return false;
    output=pins.data_->held->source.resource;failure=0;return true;
}
bool AcquirePinnedImmutable(ID3D12GraphicsCommandList* list,const FixedAssetPins& pins,ImmutableLease& output)try{
    output={};fixedPinDetails={};failure=128;
    if(!list || !pins.data_ || !pins.data_->held || !Observe() || probe::internalCommands ||
        list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT || !Compatible() || !FixedCompatible())return false;
    const auto& captured=*pins.data_;const auto& held=*captured.held.Get();const auto& source=held.source;
    const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));const auto context=camera_upload::CurrentContext(base);
    failure=129;
    if(context!=captured.context || context.list!=reinterpret_cast<uintptr_t>(list) || !FixedContext(context) ||
        !native_recording::ValidateRecording(captured.recording,list,context) || !Identity(source.native) ||
        FixedOwner(source.native)!=source.owner || !Physical(held) ||
        ReadState(source.native,context.context,context.list,Read).status!=8)return false;
    failure=130;const auto& resource=source.resource;
    if(!detail::ImmutableShape(resource->GetDesc()) || !DefaultHeap(resource.Get()) || !Unprotected(resource.Get()))return false;
    Microsoft::WRL::ComPtr<ID3D12Device> device,actual;
    if(FAILED(resource->GetDevice(IID_PPV_ARGS(&device))) || FAILED(list->GetDevice(IID_PPV_ARGS(&actual))) ||
        !device || device!=actual || device->GetNodeCount()!=1)return false;
    const auto desc=resource->GetDesc();const auto required=device->GetResourceAllocationInfo(0,1,&desc);
    if(!required.SizeInBytes || required.SizeInBytes==UINT64_MAX || held.allocation.bytes<required.SizeInBytes)return false;
    const auto receipt=AcquireReceipt(resource.Get(),source.native.registry);if(!receipt)return false;
    // Copy only authenticated, already-owned pins. A changed registry owner is
    // a rejection above, never a replacement allocation to AddRef here.
    Microsoft::WRL::ComPtr<ImmutableOwner> owner;owner.Attach(new ImmutableOwner);
    owner->allocatorPin=held.allocatorPin;owner->allocationPin=held.allocationPin;owner->source=source;owner->source.receipt=receipt;
    owner->creation=held.creation;owner->allocation=held.allocation;owner->allocator=held.allocator;
    failure=131;
    if(!Physical(*owner.Get()) || !Validate(owner->source) || !FixedContext(context) ||
        camera_upload::CurrentContext(base)!=context || !native_recording::ValidateRecording(captured.recording,list,context))return false;
    output={owner,owner->source};failure=0;return true;
}catch(...){output={};failure=134;return false;}
#endif
#if CVR_RUST_VRIK
#else
bool AcquireImmutable(ID3D12GraphicsCommandList* list,ID3D12Resource* resource,ImmutableLease& output)try{
    output={};failure=85;
    if(!list || !resource || !Observe() || !Compatible() || !FixedCompatible() || !detail::ImmutableShape(resource->GetDesc()) ||
        list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT || !DefaultHeap(resource) || !Unprotected(resource))return false;
    const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));const auto context=camera_upload::CurrentContext(base);
    failure=86;if(context.list!=reinterpret_cast<uintptr_t>(list) || !FixedContext(context))return false;
    NativeTexture native;if(!ResolveNative(resource,native))return false;
    failure=87;const auto fixed=FixedOwner(native);if(!fixed || ReadState(native,context.context,context.list,Read).status!=8)return false;
    const auto receipt=AcquireReceipt(resource,native.registry);if(!receipt)return false;
    Microsoft::WRL::ComPtr<ImmutableOwner> owner;owner.Attach(new ImmutableOwner);owner->source={resource,native,receipt,fixed};
    owner->allocation=source_cbv_details::detail::ReadAllocation(Read,nullptr,base,fixed);
    owner->allocator=source_cbv_details::detail::ReadAllocator(Read,nullptr,base,native.registry,owner->allocation);
    using O=source_cbv_details::Observation;failure=88;
    if(owner->allocation.observation!=O::Known || owner->allocation.resource!=native.resource || owner->allocation.kind>1 ||
        owner->allocator.observation!=O::Known || !owner->allocator.pimplMatches)return false;
    failure=89;if(!constants::QueryResourceCreation(resource,owner->creation))return false;
    Microsoft::WRL::ComPtr<ID3D12Device> device,actual;
    if(FAILED(resource->GetDevice(IID_PPV_ARGS(&device))) || FAILED(list->GetDevice(IID_PPV_ARGS(&actual))) || device!=actual || device->GetNodeCount()!=1)return false;
    const auto desc=resource->GetDesc();const auto allocationInfo=device->GetResourceAllocationInfo(0,1,&desc);
    if(!allocationInfo.SizeInBytes || allocationInfo.SizeInBytes==UINT64_MAX || owner->allocation.bytes<allocationInfo.SizeInBytes)return false;
    owner->allocatorPin=reinterpret_cast<IUnknown*>(owner->allocator.address);owner->allocationPin=reinterpret_cast<IUnknown*>(owner->allocation.address);
    failure=90;if(!Physical(*owner.Get()) || !Validate(owner->source) || !FixedContext(context) ||
        camera_upload::CurrentContext(base)!=context)return false;
    output={owner,owner->source};failure=0;return true;
}catch(...){output={};failure=94;return false;}
#endif
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
bool ValidateImmutable(const ImmutableLease& value,ID3D12CommandQueue* queue){return cvr_tr_validate_immutable(&value,queue);}
#endif
#else
bool ValidateImmutable(const ImmutableLease& value,ID3D12CommandQueue* queue)try{
    Microsoft::WRL::ComPtr<ImmutableOwner> owner;failure=93;
    if(!value.owner || FAILED(value.owner->QueryInterface(immutableOwnerType,reinterpret_cast<void**>(owner.GetAddressOf()))) || !owner)return false;
    const auto& saved=owner->source;const auto& current=value.source;
    if(saved.resource!=current.resource || saved.native.registry!=current.native.registry || saved.native.resource!=current.native.resource ||
        saved.native.handle!=current.native.handle || saved.receipt!=current.receipt || saved.owner!=current.owner ||
        !Validate(saved,queue) || !Physical(*owner.Get()))return false;
    failure=0;return true;
}catch(...){failure=94;return false;}
#endif
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
bool GetImmutablePageable(ID3D12Device* device,const ImmutableLease& value,ImmutablePageable& output){
    ImmutablePageable next;const bool ok=cvr_tr_immutable_pageable(device,&value,&next);output=std::move(next);return ok;
}
#endif
#else
bool GetImmutablePageable(ID3D12Device* device,const ImmutableLease& value,ImmutablePageable& output)try{
    output={};failure=100;if(!device || device->GetNodeCount()!=1)return false;
    if(!ValidateImmutable(value))return false;
    Microsoft::WRL::ComPtr<ImmutableOwner> owner;failure=101;
    if(FAILED(value.owner->QueryInterface(immutableOwnerType,reinterpret_cast<void**>(owner.GetAddressOf()))) || !owner)return false;
    ImmutablePageable next;next.source=value;
    if(owner->creation.kind==1)next.pageable=owner->source.resource;
    else if(owner->creation.kind==2)next.pageable=owner->creation.heap;
    else return false;
    Microsoft::WRL::ComPtr<ID3D12Device> resourceDevice,pageableDevice;failure=102;
    if(!next.pageable || FAILED(owner->source.resource->GetDevice(IID_PPV_ARGS(&resourceDevice))) ||
        FAILED(next.pageable->GetDevice(IID_PPV_ARGS(&pageableDevice))) || resourceDevice.Get()!=device || pageableDevice.Get()!=device)return false;
    if(!ValidateImmutable(next.source))return false;
    output=std::move(next);failure=0;return true;
}catch(...){output={};failure=94;return false;}
#endif
#if !CVR_RUST_VRIK
bool RegisterImmutableUse(ID3D12GraphicsCommandList* list,const ImmutableLease& source,ImmutableUse& output)try{
    output={};failure=96;if(!list || probe::internalCommands || !ValidateImmutable(source))return false;
    const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));const auto context=camera_upload::CurrentContext(base);
    native_recording::Lease recording;
    failure=97;if(context.list!=reinterpret_cast<uintptr_t>(list) || !FixedContext(context) ||
        !native_recording::Capture(list,context,recording) || recording.registry!=source.source.native.registry)return false;
    using Register=void(*)(void*,uint32_t,uint32_t,uint32_t);
    {probe::Scope internal;reinterpret_cast<Register>(base+0x337264)(reinterpret_cast<void*>(context.context),source.source.native.handle,detail::ShaderReads,UINT_MAX);}
    uint64_t tracker{},resident{};uint8_t rejected{};const auto offset=0x2F1D8+uint64_t(source.source.native.handle-1)*0xB0;
    failure=98;if(recording.registry>UINT64_MAX-offset-0xB0 || context.context>UINT64_MAX-0x638)return false;
    const auto address=recording.registry+offset;
    failure=98;if(!Read(nullptr,address+0x68,&resident,8) || (resident &&
        (resident!=source.source.native.resource || !Read(nullptr,context.context+0x630,&tracker,8) || !tracker ||
         tracker>UINT64_MAX-38 || !Read(nullptr,tracker+37,&rejected,1) || rejected)))return false;
    if(camera_upload::CurrentContext(base)!=context || !FixedContext(context) || !ValidateImmutable(source) ||
        !native_recording::ValidateRecording(recording,list,context) ||
        !gpu::KeepCommandResources(list,{source.owner.Get(),recording.token.Get()}))return false;
    output={source,recording};failure=0;return true;
}catch(...){output={};failure=99;return false;}
#endif
#if !CVR_RUST_VRIK
bool ValidateImmutableUse(const ImmutableUse& use,ID3D12CommandQueue* queue){
    return queue && use.recording.token && native_recording::ValidateSubmission(use.recording,queue) && ValidateImmutable(use.source,queue);
}
#endif
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
bool ValidateWritableTarget(ID3D12GraphicsCommandList* list,ID3D12Resource* resource,D3D12_RESOURCE_STATES expected){
    return cvr_tr_validate_writable_target(list,resource,uint32_t(expected));
}
#endif
struct WritableTargetIdentity::Data {WritableTargetIdentity captured;};
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
bool CaptureWritableIdentity(ID3D12GraphicsCommandList* list,ID3D12Resource* resource,
                             D3D12_RESOURCE_STATES expected,WritableTargetIdentity& output){
    static_assert(sizeof(WritableTargetIdentity)==384 && offsetof(WritableTargetIdentity,resourceDesc)==136 &&
        offsetof(WritableTargetIdentity,nativeRecord)==268 && offsetof(WritableTargetIdentity,allocatorOwnerObservation)==360,
        "mirrored by rust/crates/cvr-render/src/texture_writable.rs");
    Microsoft::WRL::ComPtr<ID3D12Resource> held=resource;output={};WritableTargetIdentity next;
    if(!cvr_tr_capture_writable_identity(list,held.Get(),uint32_t(expected),&next))return false;
    auto authenticated=std::make_shared<WritableTargetIdentity::Data>();authenticated->captured=next;
    next.data_=std::move(authenticated);output=std::move(next);return true;
}
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
bool ValidateWritableIdentity(const WritableTargetIdentity& value,ID3D12CommandQueue* queue){
    return cvr_tr_validate_writable_identity(&value,value.data_?&value.data_->captured:nullptr,queue);
}
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
void AuthenticateWritableIdentity(WritableTargetIdentity& value){
    auto authenticated=std::make_shared<WritableTargetIdentity::Data>();authenticated->captured=value;value.data_=std::move(authenticated);
}
#endif
#else
bool ValidateWritableTarget(ID3D12GraphicsCommandList* list,ID3D12Resource* resource,D3D12_RESOURCE_STATES expected)try{
    failure=80;if(!list || !resource || !Compatible() || list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT ||
        !detail::WritableState({uint32_t(expected),1},uint32_t(expected)))return false;
    const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));const auto context=camera_upload::CurrentContext(base);
    failure=81;if(context.list!=reinterpret_cast<uintptr_t>(list) || !FixedContext(context))return false;
    NativeTexture native{};if(!ResolveNative(resource,native)){failure=82;return false;}
    const auto first=ReadState(native,context.context,context.list,Read);
    failure=83;if(!detail::WritableState(first,uint32_t(expected)))return false;
    const auto second=ReadState(native,context.context,context.list,Read);
    failure=84;if(!detail::WritableState(second,uint32_t(expected)) || !Identity(native) ||
        !FixedContext(context) || camera_upload::CurrentContext(base)!=context)return false;
    failure=0;return true;
}catch(...){failure=84;return false;}
struct WritableTargetIdentity::Data {WritableTargetIdentity captured;};
namespace {
bool ReadWritableRecord(const NativeTexture& native,std::array<uint8_t,0x58>& output){
    output={};if(!Identity(native))return false;
    const auto offset=0x2f1d8+uint64_t(native.handle-1)*0xb0;
    if(native.registry>UINT64_MAX-offset-output.size())return false;
    std::array<uint8_t,0x58> first{},second{};
    if(!Read(nullptr,native.registry+offset,first.data(),first.size()) ||
        !Read(nullptr,native.registry+offset,second.data(),second.size()))return false;
    const auto identity=detail::WritableRecord(second);
    // The rest of this record is an audit sample. Unknown or mutable fields
    // must not become an inferred allocation-owner or final-state contract.
    if(identity.resource!=native.resource || identity!=detail::WritableRecord(first) || !Identity(native))return false;
    output=second;return true;
}
bool WritablePhysicalMetadata(const WritableTargetIdentity& value){
    constants::ResourceCreationInfo current;
    if(!value.creation.resource || value.native.resource!=reinterpret_cast<uintptr_t>(value.creation.resource.Get()) ||
        !constants::QueryResourceCreation(value.creation.resource,current) || !detail::SameCreationMetadata(value.creation,current) ||
        !detail::SameResourceDescription(value.resourceDesc,current.resource->GetDesc()))return false;
    D3D12_HEAP_PROPERTIES properties{};D3D12_HEAP_FLAGS flags{};
    if(FAILED(current.resource->GetHeapProperties(&properties,&flags)) ||
        !detail::SameHeapProperties(value.resourceHeapProperties,properties) || flags!=value.resourceHeapFlags)return false;
    if(current.kind==2 && (!current.heap || !detail::SameHeapDescription(value.heapDesc,current.heap->GetDesc())))return false;
    std::array<uint8_t,0x58> record{};
    return ReadWritableRecord(value.native,record) && detail::WritableRecord(record)==detail::WritableRecord(value.nativeRecord);
}
}
bool CaptureWritableIdentity(ID3D12GraphicsCommandList* list,ID3D12Resource* resource,
                             D3D12_RESOURCE_STATES expected,WritableTargetIdentity& output)try{
    Microsoft::WRL::ComPtr<ID3D12Resource> held=resource;output={};
    failure=103;if(!list || !held || probe::internalCommands || !Observe())return false;
    WritableTargetIdentity next;next.creation.resource=held;next.expectedWritableState=expected;
    if(!ValidateWritableTarget(list,next.creation.resource.Get(),expected))return false;
    const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));next.originalContext=camera_upload::CurrentContext(base);
    failure=104;if(next.originalContext.list!=reinterpret_cast<uintptr_t>(list) ||
        !native_recording::Capture(list,next.originalContext,next.recording))return false;
    failure=105;if(!ResolveNative(resource,next.native) || next.native.registry!=next.recording.registry ||
        !ReadWritableRecord(next.native,next.nativeRecord)){failure=105;return false;}
    next.allocatorOwnerObservation=detail::WritableRecord(next.nativeRecord).owner;
    failure=106;if(!constants::QueryResourceCreation(next.creation.resource,next.creation))return false;
    next.resourceDesc=resource->GetDesc();if(next.creation.heap)next.heapDesc=next.creation.heap->GetDesc();
    Microsoft::WRL::ComPtr<ID3D12Device> resourceDevice,listDevice;
    failure=107;if(FAILED(resource->GetHeapProperties(&next.resourceHeapProperties,&next.resourceHeapFlags)) ||
        FAILED(resource->GetDevice(IID_PPV_ARGS(&resourceDevice))) || !resourceDevice ||
        FAILED(list->GetDevice(IID_PPV_ARGS(&listDevice))) || resourceDevice!=listDevice)return false;
    failure=108;if(!WritablePhysicalMetadata(next) || camera_upload::CurrentContext(base)!=next.originalContext ||
        !native_recording::ValidateRecording(next.recording,list,next.originalContext))return false;
    if(!ValidateWritableTarget(list,resource,expected))return false;
    failure=108;if(camera_upload::CurrentContext(base)!=next.originalContext || !WritablePhysicalMetadata(next) ||
        !native_recording::ValidateRecording(next.recording,list,next.originalContext))return false;
    auto authenticated=std::make_shared<WritableTargetIdentity::Data>();authenticated->captured=next;
    next.data_=std::move(authenticated);output=std::move(next);failure=0;return true;
}catch(...){output={};failure=111;return false;}
bool ValidateWritableIdentity(const WritableTargetIdentity& value,ID3D12CommandQueue* queue)try{
    failure=109;if(!value.data_ || !detail::SameWritableIdentityMetadata(value,value.data_->captured))return false;
    const auto& captured=value.data_->captured;
    failure=108;if(!WritablePhysicalMetadata(captured))return false;
    failure=110;if(queue && !native_recording::ValidateSubmission(captured.recording,queue))return false;
    failure=0;return true;
}catch(...){failure=111;return false;}
#endif
#if CVR_RUST_VRIK
#else
bool ViewFootprints(ID3D12Device* device,const Lease& source,const D3D12_SHADER_RESOURCE_VIEW_DESC& view,
                    uint64_t offset,std::vector<MipFootprint>& output)try{
    output.clear();failure=70;if(!device || !source.resource || offset%D3D12_TEXTURE_DATA_PLACEMENT_ALIGNMENT || !Validate(source))return false;
    Microsoft::WRL::ComPtr<ID3D12Device> actual;
    if(FAILED(source.resource->GetDevice(IID_PPV_ARGS(&actual))) || actual.Get()!=device)return false;
    std::vector<MipFootprint> result;failure=71;
    if(!detail::ViewLayouts(device,source.resource->GetDesc(),view,offset,result))return false;
    output=std::move(result);failure=0;return true;
}catch(...){output.clear();failure=70;return false;}
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
bool Copy(ID3D12GraphicsCommandList* list,const Lease& source,ID3D12Resource* buffer,const D3D12_PLACED_SUBRESOURCE_FOOTPRINT& layout){
    return CopySubresource(list,source,0,buffer,layout);
}
#endif
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
bool CopySubresource(ID3D12GraphicsCommandList* list,const Lease& source,UINT mip,ID3D12Resource* buffer,const D3D12_PLACED_SUBRESOURCE_FOOTPRINT& layout){
    return cvr_tr_copy_subresource(list,&source,mip,buffer,&layout);
}
#endif
#else
bool CopySubresource(ID3D12GraphicsCommandList* list,const Lease& source,UINT mip,ID3D12Resource* buffer,const D3D12_PLACED_SUBRESOURCE_FOOTPRINT& layout){
    failure=20;if(!list || !source.resource || !buffer || !Compatible() || list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT || !Identity(source.native) ||
        source.native.resource!=reinterpret_cast<uintptr_t>(source.resource.Get()))return false;
    const auto desc=source.resource->GetDesc(),destination=buffer->GetDesc();
    if(!Shape(desc) || mip>=UINT(desc.MipLevels)*desc.DepthOrArraySize || destination.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER)return false;
    Microsoft::WRL::ComPtr<ID3D12Device> a,b,c;
    if(FAILED(list->GetDevice(IID_PPV_ARGS(&a))) || FAILED(buffer->GetDevice(IID_PPV_ARGS(&b))) || FAILED(source.resource->GetDevice(IID_PPV_ARGS(&c))) || a!=b || a!=c)return false;
    MipFootprint expected;uint64_t bytes{};failure=71;
    if(!MipLayout(a.Get(),desc,mip,layout.Offset,expected) || !SameLayout(layout,expected.layout) ||
        !detail::FootprintRange(layout,expected.rows,expected.rowBytes,destination.Width,bytes))return false;
    failure=72;D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS flags{};
    if(FAILED(buffer->GetHeapProperties(&heap,&flags)) ||
        (heap.Type!=D3D12_HEAP_TYPE_DEFAULT && heap.Type!=D3D12_HEAP_TYPE_READBACK) ||
        (destination.Flags&~D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS))return false;
    const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));const auto context=camera_upload::CurrentContext(base);
    failure=21;if(context.list!=reinterpret_cast<uintptr_t>(list))return false;
    const bool implicit=source.receipt!=0;uint32_t required{};
    if(implicit){
        failure=42;if(!Validate(source) || !DefaultHeap(source.resource.Get()) || !FixedContext(context))return false;
        required=detail::AllowedReads;
    }else{
        const auto before=ReadState(source.native,context.context,context.list,Read);
        if((detail::AlphaFormat(desc.Format) || UINT(desc.MipLevels)*desc.DepthOrArraySize>1) && before.status!=1){failure=30+before.status;return false;}
        failure=30+before.status;if(!detail::CopyReadMask(before,required))return false;
    }
    if(!gpu::KeepCommandResources(list,{source.resource.Get(),buffer}))return false;
    using Register=void(*)(void*,uint32_t,uint32_t,uint32_t);using Flush=void(*)(void*);
    const auto registerRead=reinterpret_cast<Register>(base+0x337264);const auto flush=reinterpret_cast<Flush>(base+0x3e3bb4);
    probe::Scope internal;auto* nativeContext=reinterpret_cast<void*>(context.context);
    auto current=[&]{return camera_upload::CurrentContext(base)==context && Identity(source.native);};
    auto recover=[&]{
        failure=40;if(!current())return false;
        auto state=ReadState(source.native,context.context,context.list,Read);
        if(state.status==4){flush(nativeContext);state=ReadState(source.native,context.context,context.list,Read);}
        if((state.status!=1 && state.status!=2 && state.status!=5) || !current())return false;
        registerRead(nativeContext,source.native.handle,detail::ShaderReads,UINT_MAX);flush(nativeContext);
        state=ReadState(source.native,context.context,context.list,Read);
        if(current() && (!(detail::AlphaFormat(desc.Format) || UINT(desc.MipLevels)*desc.DepthOrArraySize>1) || state.status==1) &&
            detail::HasReadState(state,detail::ShaderReads))failure=41;
        return false;
    };
    failure=22;if(!current())return false;
    if(implicit){
        failure=42;if(!Validate(source) || !FixedContext(context))return false;
        // Flag 0x40 still registers native allocation use before bypassing state
        // tracking. COMMON/cumulative read promotion needs no source barrier.
        failure=40;registerRead(nativeContext,source.native.handle,required,UINT_MAX);flush(nativeContext);
        failure=40;if(!current() || !FixedContext(context) || !Validate(source))return false;
    }else{
        // 0x1F42D8 keeps an existing superset for a read request. Retain shader
        // permissions and leave the combined state in native tracking.
        failure=40;registerRead(nativeContext,source.native.handle,required,UINT_MAX);flush(nativeContext);
        const auto copiedState=ReadState(source.native,context.context,context.list,Read);
        if(!current() || ((detail::AlphaFormat(desc.Format) || UINT(desc.MipLevels)*desc.DepthOrArraySize>1) && copiedState.status!=1) ||
            !detail::HasReadState(copiedState,required))return recover();
    }
    D3D12_TEXTURE_COPY_LOCATION from{},to{};from.pResource=source.resource.Get();from.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;from.SubresourceIndex=mip;
    to.pResource=buffer;to.Type=D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;to.PlacedFootprint=layout;
    list->CopyTextureRegion(&to,0,0,0,&from,nullptr);
    failure=0;return true;
}
#endif
}
