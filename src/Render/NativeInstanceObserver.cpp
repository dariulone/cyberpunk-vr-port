#include "Render/NativeInstanceObserver.hpp"
#include "Render/NativeInstanceWitness.hpp"
#include "Render/NativeSourceCbvDetails.hpp"
#include "Render/RegisteredConstantUpload.hpp"
#include "Render/RenderProbeScope.hpp"
#include "Render/CommandResources.hpp"
#include "Render/InstanceGpuProbe.hpp"
#include <windows.h>
#include <tlhelp32.h>
#include <intrin.h>
#include <algorithm>
#include <cstring>
#include <mutex>
#include <vector>
#if CVR_RUST_VRIK
// The native instance observer is Rust (rust/crates/cvr-render/src/instance_observer_native.rs: the rings'
// pinned backings, the writer and publication calls, the reservation witness, the ledger, the source
// queries, the submission check and the GPU copies); these forwards keep the header's API.
namespace cvr::stereo::instance_observer::rust_abi {
// The C++-typed checks the Rust observer calls back.
struct Host {
    bool(*layout)(ID3D12PipelineState*,uint32_t);
    bool(*shader)(ID3D12PipelineState*,const void*);
};
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
// A SourceCandidate as the Rust checks read it.
struct CandidateView {
    instance_ring::CompletedWriter producer;instance_ring::ReadRequest source;const uint8_t* bytes;size_t size;IUnknown* owner;bool cpu;
};
// QueryAtSource's result: the payload and the receipt, one reference each.
struct SourceOut {
    const void* payload;instance_ring::CompletedWriter producer;instance_ring::ReadRequest source;instance_ring::Publication publication;
    size_t offset,bytes;IUnknown* owner;
};
// instance_snapshot_view::Input as RecordGpuCapture checks it.
struct InputView {
    uint64_t root,list,generation,occurrence;UINT eye;bool bytes;ID3D12PipelineState* pipeline;const void* shader;
    D3D12_VERTEX_BUFFER_VIEW original;D3D12_DRAW_INDEXED_ARGUMENTS draw;
};
static_assert(sizeof(CandidateView)==296 && sizeof(SourceOut)==360 && sizeof(InputView)==96 && sizeof(native_probe::DrawRecord)==1096 &&
    sizeof(geometry::BufferLease)==80 && sizeof(native_recording::Lease)==40,"mirrored by instance_observer_native.rs");
#endif
}
extern "C" {
void cvr_nio2_attach(const cvr::stereo::instance_observer::rust_abi::Host*);
bool cvr_nio2_call_live(const void*);
void cvr_nio2_call_drop(const void*);
void cvr_nio2_returned(const void*,uint32_t);
void cvr_nio2_published(const void*);
}
namespace cvr::stereo::instance_observer {
namespace {
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's host is rust/crates/cvr-render/src/instance_observer_native.rs' own
const rust_abi::Host host{
    [](ID3D12PipelineState* pipeline,uint32_t stride)->bool{try{const auto shader=shader_snapshot::Capture(pipeline);
        return shader && detail::InstanceLayout(shader->input,stride);}catch(...){return false;}},
    [](ID3D12PipelineState* pipeline,const void* shader)->bool{try{return shader_snapshot::Capture(pipeline).get()==shader;}catch(...){return false;}},
};
[[maybe_unused]] const bool attached=(cvr_nio2_attach(&host),true);
#endif
// The plugin's registration of the detours (the host is Rust's): rust/crates/cvr-render/src/engine_detours.rs
// cvr_static_registrations.
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
rust_abi::CandidateView View(const SourceCandidate& candidate)noexcept{
    const auto bytes=candidate.cpu.Bytes();
    return {candidate.cpu.Producer(),candidate.cpu.Source(),bytes.data(),bytes.size(),candidate.owner.Get(),bool(candidate.cpu)};
}
#endif
}
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
namespace detail {
// The Rust call (one Arc reference) the Call holds.
struct CallData {
    const void* rust{};
    explicit CallData(const void* value):rust(value){}
    ~CallData(){cvr_nio2_call_drop(rust);}
};
}
Call::~Call()=default;Call::Call(Call&&)noexcept=default;Call& Call::operator=(Call&&)noexcept=default;
Call::operator bool()const noexcept{return data_ && cvr_nio2_call_live(data_->rust);}
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
void Returned(Call& token,uint32_t first)noexcept{if(token.data_)cvr_nio2_returned(token.data_->rust,first);}
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
void Returned(Call& token)noexcept{if(token.data_)cvr_nio2_published(token.data_->rust);}
#endif
}
#else

extern "C" {
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_NativeInstanceObserverEnable{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_NativeInstanceObserverInstalled{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_NativeInstanceObserverResetRequest{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_NativeInstanceObserverFirstLoss{};
__declspec(dllexport) std::array<std::atomic<uint64_t>,24> CyberpunkVR_NativeInstanceObserverDiagnostics{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_NativeInstanceGpuProbePhase{};
}
namespace cvr::stereo::instance_observer {
namespace {
using Microsoft::WRL::ComPtr;
using Observation=source_cbv_details::Observation;
constexpr GUID receiptType{0xa29299ae,0xc2e4,0x4e54,{0x87,0xa5,0x3c,0xa0,0x72,0xf6,0x04,0xb1}};
thread_local Status last{};thread_local QueryObservation query;
bool Read(void*,uint64_t address,void* output,size_t bytes)noexcept {SIZE_T copied{};return address && address<=UINT64_MAX-bytes &&
    ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<const void*>(address),output,bytes,&copied) && copied==bytes;}
uint64_t Base()noexcept{return reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));}
bool Enabled()noexcept{return CyberpunkVR_NativeInstanceObserverEnable.load(std::memory_order_acquire)!=0 &&
    CyberpunkVR_NativeInstanceObserverInstalled.load(std::memory_order_acquire)==7 && !probe::internalCommands;}
bool Retain(uint64_t address,IUnknown*& output)noexcept {
    output=nullptr;__try{if(!address)return false;auto* object=reinterpret_cast<IUnknown*>(address);object->AddRef();output=object;return true;}
    __except(EXCEPTION_EXECUTE_HANDLER){return false;}
}
struct RingFields {uint32_t handle{},capacity{},stride{};uint64_t mapped{},head{},tail{};bool operator==(const RingFields&)const=default;};
bool Fields(uint64_t ring,RingFields& output)noexcept {
    output={};const auto once=[&](RingFields& value){return ring && !(ring&7) && ring<=UINT64_MAX-0x60 &&
        Read(nullptr,ring,&value.handle,4) && Read(nullptr,ring+8,&value.mapped,8) && Read(nullptr,ring+0x10,&value.capacity,4) &&
        Read(nullptr,ring+0x14,&value.stride,4) && Read(nullptr,ring+0x38,&value.tail,8) && Read(nullptr,ring+0x40,&value.head,8);};
    RingFields again;if(!once(output) || !once(again) || output.handle!=again.handle || output.mapped!=again.mapped ||
        output.capacity!=again.capacity || output.stride!=again.stride)return false;output=again;return true;
}
bool Class(const RingFields& value)noexcept{return (value.capacity==0x20000 && value.stride==48) || (value.capacity==0x8000 && value.stride==64);}
camera_upload::Context Current()noexcept {
    uint64_t tls{},context{},state{},list{};uint8_t initialized{};
    if(!Read(nullptr,__readgsqword(0x58),&tls,8) || !tls || tls>UINT64_MAX-0x1A0 || !Read(nullptr,tls+0x14,&initialized,1) ||
        !initialized || !Read(nullptr,tls+0x198,&context,8) || !context || context>UINT64_MAX-0x68 ||
        !Read(nullptr,context+0x60,&state,8) || !Read(nullptr,context+0x30,&list,8))return {};
    return {context,state,list};
}
bool Stamp(uint64_t registry,uint64_t context,uint64_t& output)noexcept {
    output=0;uint32_t slot{};uint64_t version{},again{};
    if(!registry || !context || context>UINT64_MAX-0x54 || !Read(nullptr,context+0x50,&slot,4))return false;
    const auto offset=0x1408960+uint64_t(slot&0x3FF)*24;
    if(registry>UINT64_MAX-offset-8)return false;const auto address=registry+offset;
    if(!Read(nullptr,address,&version,8) ||
        !Read(nullptr,address,&again,8) || again!=version || version>=UINT64_MAX/1024)return false;
    output=((version+1)<<10)|(slot&0x3FF);return output!=0;
}
bool Scope(uint64_t occurrence,instance_ring::Scope& output,native_recording::Lease& recording) {
    output={};recording={};const auto context=Current();uint64_t registry{},stamp{};
    if(!context || !Read(nullptr,Base()+0x3BCDCA8,&registry,8) || !registry ||
        !native_recording::Capture(reinterpret_cast<ID3D12GraphicsCommandList*>(context.list),context,recording) || recording.registry!=registry ||
        !Stamp(registry,context.context,stamp))return false;
    const auto native=source_cbv_details::detail::ReadContext(Read,nullptr,context.context,context.list);
    if(native.observation!=Observation::Known || !native_recording::ValidateRecording(recording,
        reinterpret_cast<ID3D12GraphicsCommandList*>(context.list),context))return false;
    output={context.list,recording.generation,occurrence,GetCurrentThreadId(),context.context,registry,stamp};return true;
}
bool SameAllocation(const source_cbv_details::Allocation& a,const source_cbv_details::Allocation& b)noexcept {
    return a.observation==Observation::Known && b.observation==Observation::Known && a.address==b.address && a.vtable==b.vtable &&
        a.pimpl==b.pimpl && a.bytes==b.bytes && a.resource==b.resource && a.heap==b.heap && a.block==b.block && a.handle==b.handle && a.kind==b.kind;
}
struct Backing {
    geometry::BufferLease source;constants::ResourceCreationInfo creation;
    ComPtr<IUnknown> allocator,allocation;
    source_cbv_details::Allocation nativeAllocation;source_cbv_details::Allocator nativeAllocator;
    std::array<uint8_t,0xB0> raw{};uint64_t recordAddress{},cookie{};
    instance_ring::RingObservation observed;bool quarantined{};
};
bool Record(uint64_t registry,uint32_t handle,std::array<uint8_t,0xB0>& raw,uint64_t& address)noexcept {
    address=0;if(!registry || !handle || handle>0x100000)return false;const auto offset=0x5C0AF8+uint64_t(handle-1)*0xB0;
    if(registry>UINT64_MAX-offset-raw.size())return false;address=registry+offset;
    return source_cbv_details::detail::Bytes(Read,nullptr,address,raw)==Observation::Known;
}
bool Allocation(const Backing& value)noexcept {
    const auto allocation=source_cbv_details::detail::ReadAllocation(Read,nullptr,Base(),reinterpret_cast<uintptr_t>(value.allocation.Get()));
    const auto allocator=source_cbv_details::detail::ReadAllocator(Read,nullptr,Base(),value.observed.registry,allocation);
    return value.allocation && value.allocator && SameAllocation(value.nativeAllocation,allocation) && allocator.observation==Observation::Known &&
        allocator.address==reinterpret_cast<uintptr_t>(value.allocator.Get()) && allocator.vtable==value.nativeAllocator.vtable &&
        allocator.pimpl==value.nativeAllocator.pimpl && allocator.pimplMatches;
}
bool Physical(const Backing& value,instance_ring::RingObservation& output) {
    output={};RingFields fields;std::array<uint8_t,0xB0> raw{};uint64_t address{};constants::ResourceCreationInfo creation;
    if(!Fields(value.observed.ring,fields) || !Class(fields) || fields.handle!=value.observed.handle || fields.mapped!=value.observed.mapped ||
        fields.capacity!=value.observed.capacity || fields.stride!=value.observed.stride || !Record(value.observed.registry,fields.handle,raw,address) ||
        address!=value.recordAddress || raw!=value.raw || !Allocation(value) ||
        !constants::QueryResourceCreation(value.source.resource,creation) || creation.kind!=value.creation.kind || creation.resource!=value.creation.resource ||
        creation.heap!=value.creation.heap || creation.heapOffset!=value.creation.heapOffset || creation.allocationBytes!=value.creation.allocationBytes)return false;
    output=value.observed;output.head=fields.head;output.tail=fields.tail;instance_ring::Range range;
    if(output.head<output.tail || output.head-output.tail>output.capacity)return false;
    const auto desc=value.source.resource->GetDesc();D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS flags{};
    return desc.Dimension==D3D12_RESOURCE_DIMENSION_BUFFER && desc.Width==output.width && !desc.Flags &&
        value.source.resource->GetGPUVirtualAddress()==output.gpu && SUCCEEDED(value.source.resource->GetHeapProperties(&heap,&flags)) && heap.Type==D3D12_HEAP_TYPE_UPLOAD;
}
std::shared_ptr<Backing> Capture(uint64_t ring,const instance_ring::Scope& scope) {
    RingFields fields;if(!Fields(ring,fields) || !Class(fields))return {};auto value=std::make_shared<Backing>();
    if(!Record(scope.registry,fields.handle,value->raw,value->recordAddress) || value->raw[6]!=0x51 ||
        source_cbv_details::detail::Field<uint64_t>(value->raw,0x20) || source_cbv_details::detail::Field<uint64_t>(value->raw,0x28) ||
        source_cbv_details::detail::Field<uint64_t>(value->raw,0x40)!=fields.mapped)return {};
    const auto resource=source_cbv_details::detail::Field<uint64_t>(value->raw,0x10);IUnknown* retained{};
    if(!Retain(resource,retained))return {};ComPtr<IUnknown> owner;owner.Attach(retained);
    if(FAILED(owner.As(&value->source.resource)))return {};const auto desc=value->source.resource->GetDesc();D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS flags{};
    const auto gpu=value->source.resource->GetGPUVirtualAddress();const auto width=uint64_t(fields.capacity)*fields.stride;
    if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER || desc.Width!=width || desc.Flags || !gpu || gpu>UINT64_MAX-width ||
        fields.mapped>UINT64_MAX-width || source_cbv_details::detail::Field<uint64_t>(value->raw,8)!=gpu ||
        source_cbv_details::detail::Field<uint32_t>(value->raw,0)!=width ||
        FAILED(value->source.resource->GetHeapProperties(&heap,&flags)) || heap.Type!=D3D12_HEAP_TYPE_UPLOAD ||
        !constants::QueryResourceCreation(value->source.resource,value->creation) || value->creation.kind!=2 || !value->creation.heap ||
        !value->creation.initialStateKnown || value->creation.initialState!=D3D12_RESOURCE_STATE_GENERIC_READ)return {};
    const auto allocation=source_cbv_details::detail::Field<uint64_t>(value->raw,0x18);
    value->nativeAllocation=source_cbv_details::detail::ReadAllocation(Read,nullptr,Base(),allocation);
    value->nativeAllocator=source_cbv_details::detail::ReadAllocator(Read,nullptr,Base(),scope.registry,value->nativeAllocation);
    if(value->nativeAllocation.observation!=Observation::Known || value->nativeAllocation.kind!=1 || value->nativeAllocation.resource!=resource ||
        value->nativeAllocation.bytes!=value->creation.allocationBytes || value->nativeAllocation.bytes<width ||
        value->nativeAllocation.heap!=reinterpret_cast<uintptr_t>(value->creation.heap.Get()) ||
        value->nativeAllocator.observation!=Observation::Known || !value->nativeAllocator.pimplMatches)return {};
    retained=nullptr;if(!Retain(value->nativeAllocator.address,retained))return {};value->allocator.Attach(retained);
    retained=nullptr;if(!Retain(allocation,retained))return {};value->allocation.Attach(retained);
    value->cookie=descriptors::SourceResourceCookieAtCreation(value->source.resource.Get());if(!value->cookie)return {};
    value->source.gpu=gpu;value->source.bytes=width;value->source.heapType=D3D12_HEAP_TYPE_UPLOAD;
    value->source.native={resource,gpu,width,scope.registry,scope.context,fields.handle,5};
    value->observed={ring,1,resource,gpu,fields.mapped,scope.registry,allocation,value->nativeAllocator.address,
        reinterpret_cast<uintptr_t>(value->creation.heap.Get()),width,fields.capacity,fields.head,fields.tail,fields.handle,fields.stride,5};
    instance_ring::RingObservation current;if(!Physical(*value,current))return {};value->observed=current;return value;
}
bool SameBacking(const Backing& a,const Backing& b)noexcept {
    auto left=a.observed,right=b.observed;left.generation=right.generation=left.head=right.head=left.tail=right.tail=0;
    return left==right && a.cookie==b.cookie && a.raw==b.raw;
}
struct WriterRecord {std::shared_ptr<Backing> backing;instance_ring::CompletedWriter completed;detail::ReservationRange reservation;
    std::weak_ptr<const instance_ring::WriterTicket> presence;};
struct State {
    std::recursive_mutex gate;std::atomic<bool> lost{},observed{};
    instance_ring::Ledger ledger;uint64_t epoch=1,nonce{},generation{},reservation{};size_t active{};
    std::vector<std::shared_ptr<Backing>> rings;
    std::vector<detail::CachedCursor> cursors;std::vector<WriterRecord> writers;std::vector<uint64_t> unknownRings;
};
State state;std::mutex codeMutex;bool codeAttempted{},codeValid{};uint64_t codeBase{};
void Lose(Status status)noexcept {last=status;bool expected=false;
    if(state.lost.compare_exchange_strong(expected,true,std::memory_order_acq_rel))
        CyberpunkVR_NativeInstanceObserverFirstLoss.store(uint32_t(status)+1,std::memory_order_release);}
bool Healthy()noexcept{return !state.lost.load(std::memory_order_acquire);}
void ApplyLoss()noexcept {if(!Healthy()){state.ledger.Unknown(0);state.cursors.clear();state.writers.clear();}}
bool ResetAtIdle()noexcept {
    if(!CyberpunkVR_NativeInstanceObserverResetRequest.load(std::memory_order_acquire))return true;
    if(state.active){Lose(Status::Pending);return false;}
    if(state.epoch==UINT64_MAX){Lose(Status::Capacity);return false;}
    state.ledger.Reset();state.rings.clear();state.cursors.clear();state.writers.clear();state.unknownRings.clear();++state.epoch;
    state.observed.store(false,std::memory_order_release);state.lost.store(false,std::memory_order_release);
    CyberpunkVR_NativeInstanceObserverFirstLoss.store(0,std::memory_order_release);
    for(auto& count:CyberpunkVR_NativeInstanceObserverDiagnostics)count.store(0,std::memory_order_relaxed);
    CyberpunkVR_NativeInstanceObserverResetRequest.store(0,std::memory_order_release);return true;
}
void LocalUnknown(uint64_t ring,Status status) {
    if(size_t(status)<19)CyberpunkVR_NativeInstanceObserverDiagnostics[size_t(status)].fetch_add(1,std::memory_order_relaxed);
    last=status;state.ledger.Unknown(ring);
    const auto at=std::find_if(state.rings.begin(),state.rings.end(),[&](const auto& value){return value->observed.ring==ring;});
    if(at!=state.rings.end())(*at)->quarantined=true;
    else if(std::find(state.unknownRings.begin(),state.unknownRings.end(),ring)==state.unknownRings.end()){
        if(state.unknownRings.size()>=64){Lose(Status::Capacity);return;}state.unknownRings.push_back(ring);}
    std::erase_if(state.cursors,[&](const auto& value){return value.ring==ring;});
    std::erase_if(state.writers,[&](const auto& value){return value.backing->observed.ring==ring;});
}
void Unobserved(uint64_t ring,Status status)noexcept try {
    last=status;if(!Enabled())return;std::lock_guard lock(state.gate);
    RingFields fields;if(Fields(ring,fields) && Class(fields))LocalUnknown(ring,status);
}catch(...){Lose(Status::Exception);}
bool Okay(instance_ring::Status value)noexcept{return value==instance_ring::Status::Observed || value==instance_ring::Status::Recreated || value==instance_ring::Status::Overlap;}
std::shared_ptr<Backing> Track(std::shared_ptr<Backing> value) {
    if(!value)return {};
    const auto at=std::find_if(state.rings.begin(),state.rings.end(),[&](const auto& old){return old->observed.ring==value->observed.ring;});
    if(at!=state.rings.end() && SameBacking(**at,*value)){value->observed.generation=(*at)->observed.generation;return *at;}
    if(state.generation==UINT64_MAX || (at==state.rings.end() && state.rings.size()>=64)){Lose(Status::Capacity);return {};}
    value->observed.generation=++state.generation;
    value->quarantined=std::find(state.unknownRings.begin(),state.unknownRings.end(),value->observed.ring)!=state.unknownRings.end();
    if(at==state.rings.end())state.rings.push_back(value);else{const auto ring=value->observed.ring;
        std::erase_if(state.cursors,[&](const auto& old){return old.ring==ring;});
        std::erase_if(state.writers,[&](const auto& old){return old.backing->observed.ring==ring;});*at=value;}
    if(!Okay(state.ledger.Observe(value->observed))){LocalUnknown(value->observed.ring,Status::Changed);return {};}
    if(value->quarantined)state.ledger.Unknown(value->observed.ring);return value;
}
void Put(QueryField field,uint64_t value)noexcept {query.values[size_t(field)]=value;}
bool Fail(Status status)noexcept {last=status;Put(QueryField::Status,uint64_t(status));return false;}
bool NativeCaller(uintptr_t caller,std::span<const uint32_t> returns)noexcept {
    const auto base=Base();return caller>=base && std::find(returns.begin(),returns.end(),caller-base)!=returns.end();
}
}
namespace detail {
enum class Kind:uint8_t {Writer,Reserve,Publish};
struct CallData {
    Kind kind{};bool completed{},counted{},untrackedCached{};uint64_t nonce{},thread{};std::shared_ptr<Backing> backing;
    instance_ring::Scope scope;native_recording::Lease recording;RingFields before;
    uint64_t cursorAddress{};Cursor cursor;uint32_t instances{};instance_ring::WriterTicket ticket;
    std::optional<CachedCursor> previous;std::optional<ReservationRange> actual;
    CallData* parent{};uint64_t publicationStamp{};
    explicit CallData(Kind value):kind(value){}
    ~CallData();
};
thread_local CallData* writer{};
CallData::~CallData(){std::lock_guard lock(state.gate);if(!completed && counted){if(Healthy())try{LocalUnknown(backing?backing->observed.ring:0,Status::Returned);}catch(...){Lose(Status::Exception);}ApplyLoss();}
    if(writer==this)writer=nullptr;if(counted && state.active)--state.active;}
}
namespace {
std::shared_ptr<detail::CallData> Start(detail::Kind kind,uint64_t ring) {
    if(!Enabled()){if(state.observed.load(std::memory_order_acquire))Lose(Status::Disabled);return {};}
    if(!Compatible()){Lose(Status::Code);return {};}
    RingFields classification;
    if(Fields(ring,classification) && !Class(classification))return {};
    auto call=std::make_shared<detail::CallData>(kind);
    std::lock_guard lock(state.gate);
    if(!ResetAtIdle()){call->completed=true;return {};}
    if(!Healthy()){call->completed=true;ApplyLoss();return {};}
    RingFields fields;if(!Fields(ring,fields)){call->completed=true;LocalUnknown(ring,Status::Changed);return {};}
    // The guarded renderer also has a stride160 ring. It is a different native
    // resource class; only a changed already-held target ring is a proof loss.
    if(!Class(fields)){call->completed=true;last=Status::Backing;
        if(std::any_of(state.rings.begin(),state.rings.end(),[&](const auto& value){return value->observed.ring==ring;}))Lose(Status::Changed);
        return {};}
    if(state.nonce==UINT64_MAX){call->completed=true;Lose(Status::Capacity);return {};}
    call->nonce=++state.nonce;call->thread=GetCurrentThreadId();++state.active;call->counted=true;
    if(!Scope(call->nonce,call->scope,call->recording)){call->completed=true;LocalUnknown(ring,Status::Scope);return {};}
    call->backing=Track(Capture(ring,call->scope));if(!call->backing){call->completed=true;LocalUnknown(ring,Status::Backing);return {};}
    if(!Fields(ring,call->before)){call->completed=true;LocalUnknown(ring,Status::Changed);return {};}
    state.observed.store(true,std::memory_order_release);last=Status::Observed;return call;
}
void Finish(detail::CallData& call)noexcept {call.completed=true;if(detail::writer==&call)detail::writer=nullptr;}
struct Receipt final:IUnknown {
    std::atomic<ULONG> refs{1};uint64_t epoch{};std::shared_ptr<Backing> backing;
    instance_ring::ReadRequest request;instance_ring::CpuCandidate cpu;native_recording::Lease recording;
    native_probe::DrawRecord draw;preparation::GraphParentRecord parent;bool probeRecorded{};
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** out)override{if(!out)return E_POINTER;*out=nullptr;
        if(id!=__uuidof(IUnknown) && id!=receiptType)return E_NOINTERFACE;*out=this;AddRef();return S_OK;}
    ULONG STDMETHODCALLTYPE AddRef()override{return ++refs;}
    ULONG STDMETHODCALLTYPE Release()override{const auto count=--refs;if(!count)delete this;return count;}
};
ComPtr<Receipt> Owner(IUnknown* value){ComPtr<Receipt> result;if(value)value->QueryInterface(receiptType,reinterpret_cast<void**>(result.GetAddressOf()));return result;}
}
void PrepareCodeGuards(uintptr_t base)noexcept {
    std::lock_guard lock(codeMutex);if(codeAttempted)return;codeAttempted=true;codeBase=base;codeValid=base && base==Base();
    constexpr std::array<instance_ring::Code,5> callers{{{0x333C80,0xD6,0x625CD726062737B4ull},
        {0x335900,0xED,0xF528EEF77337A62Dull},{0x939970,0x10E,0x41C77E4F7FA53C26ull},
        {0x2E9528C,0xCD,0x73EB63DD38089156ull},{0x2457F1C,0x68,0xFAC4991B2FFFFD49ull}}};
    std::array<uint8_t,1024> bytes{};
    const auto guard=[&](const instance_ring::Code& code){if(!codeValid || code.bytes>bytes.size() || base>UINT64_MAX-code.rva-code.bytes ||
        !Read(nullptr,base+code.rva,bytes.data(),code.bytes) || native_recording::detail::Fingerprint({bytes.data(),code.bytes})!=code.fnv)codeValid=false;};
    for(const auto& code:instance_ring::NativeCode)guard(code);for(const auto& code:callers)guard(code);
}
bool Compatible()noexcept {std::lock_guard lock(codeMutex);return codeValid && codeBase==Base();}
bool PrepareWitness(uintptr_t base)noexcept{return instance_witness::Prepare(base);}
bool InstallWitness(uintptr_t base,bool quiescent)noexcept{return instance_witness::Install(base,quiescent);}
bool WitnessInstalled()noexcept{return instance_witness::Installed();}
void ObserveReservationWitness(const upload_reservation::ThunkFrame* frame)noexcept try {
    if(!frame || !Enabled() || !Healthy() || frame->rax!=frame->r9 || !(frame->flags&0x40))return;
    RingFields fields;if(!Fields(frame->r10,fields) || !Class(fields))return;
    constexpr std::array<uint32_t,3> callers{0x3371EF,0x337211,0x9399BB};
    if(!NativeCaller(frame->nativeCaller,callers)){Unobserved(frame->r10,Status::Caller);return;}
    detail::ReservationRange range;
    if(frame->rbx>UINT32_MAX || frame->r8>UINT32_MAX || frame->capacity!=fields.capacity ||
        !detail::ReservationResult(frame->r9,fields.capacity,uint32_t(frame->rbx),uint32_t(frame->r8),frame->rcx,range) || range.absoluteBegin!=frame->r11){
        Unobserved(frame->r10,Status::Reservation);return;}
    auto* parent=detail::writer;
    std::shared_ptr<detail::CallData> standalone;
    if(!parent || !parent->backing || parent->backing->observed.ring!=frame->r10){standalone=Start(detail::Kind::Reserve,frame->r10);if(!standalone)return;}
    std::lock_guard lock(state.gate);if(!Healthy())return;
    auto backing=standalone?standalone->backing:parent->backing;
    instance_ring::RingObservation current;if(!Physical(*backing,current) || range.absoluteEnd>current.head){
        LocalUnknown(frame->r10,Status::Changed);if(standalone)Finish(*standalone);return;}
    if(state.reservation==UINT64_MAX){Lose(Status::Capacity);if(standalone)Finish(*standalone);return;}
    range.sequence=++state.reservation;
    if(backing->quarantined){if(state.ledger.RestartObservation(current)!=instance_ring::Status::Observed){LocalUnknown(frame->r10,Status::Changed);if(standalone)Finish(*standalone);return;}
        if(parent && !standalone)parent->ticket={};
        backing->quarantined=false;std::erase(state.unknownRings,current.ring);}
    const bool joined=parent && !standalone && parent->backing==backing && parent->thread==GetCurrentThreadId() && !parent->completed;
    if(joined && !parent->ticket.id && state.ledger.Begin(current,parent->scope,parent->ticket)!=instance_ring::Status::Observed){LocalUnknown(frame->r10,Status::Changed);return;}
    if(!Okay(state.ledger.Reuse(current,range.absoluteBegin,range.instances,joined?&parent->ticket:nullptr))){LocalUnknown(frame->r10,Status::Changed);if(standalone)Finish(*standalone);return;}
    if(joined){if(parent->actual){LocalUnknown(frame->r10,Status::Reservation);return;}parent->actual=range;
        const auto qualified=state.ledger.QualifyWrite(current,parent->ticket,range.absoluteBegin,range.first,parent->instances);
        if(qualified!=instance_ring::Status::Observed && qualified!=instance_ring::Status::Overlap)LocalUnknown(frame->r10,Status::Changed);}
    if(standalone)Finish(*standalone);last=Status::Observed;
}catch(...){Lose(Status::Exception);}
void SetInstalled(uint32_t mask)noexcept {
    const auto previous=CyberpunkVR_NativeInstanceObserverInstalled.exchange(mask&7,std::memory_order_acq_rel);
    if((mask&~7u) || (previous!=mask && state.observed.load(std::memory_order_acquire)))Lose(Status::Changed);
}
extern "C" void CvrNativeInstanceReservationObserve(const cvr::stereo::upload_reservation::ThunkFrame* frame)noexcept {
    cvr::stereo::instance_observer::ObserveReservationWitness(frame);
}
bool Dormant()noexcept {
    return CyberpunkVR_NativeInstanceObserverEnable.load(std::memory_order_acquire)==0 && !state.observed.load(std::memory_order_acquire);
}
Call::~Call()=default;Call::Call(Call&&)noexcept=default;Call& Call::operator=(Call&&)noexcept=default;
Call::operator bool()const noexcept{return data_ && !data_->completed && Healthy();}
Call Enter(void* ring,const void*,uint32_t instances,uint32_t* cursor,uintptr_t caller)noexcept try {
    if(Enabled())CyberpunkVR_NativeInstanceObserverDiagnostics[19].fetch_add(1,std::memory_order_relaxed);
    Call result;constexpr std::array<uint32_t,5> returns{0x333CDF,0x335991,0x9399A4,0x2E952C5,0x2457F46};
    if(Enabled() && !NativeCaller(caller,returns)){Unobserved(reinterpret_cast<uintptr_t>(ring),Status::Caller);return result;}
    auto call=Start(detail::Kind::Writer,reinterpret_cast<uintptr_t>(ring));if(!call)return result;
    std::lock_guard lock(state.gate);
    if(detail::writer || !instances || !cursor || instances>65536/call->before.stride ||
        !Read(nullptr,reinterpret_cast<uintptr_t>(cursor),&call->cursor,sizeof(call->cursor))){call->completed=true;LocalUnknown(call->backing->observed.ring,Status::Range);return result;}
    call->cursorAddress=reinterpret_cast<uintptr_t>(cursor);call->instances=instances;
    instance_ring::RingObservation current;if(!Physical(*call->backing,current)){
        call->completed=true;LocalUnknown(call->backing->observed.ring,Status::Changed);return result;}
    std::erase_if(state.cursors,[&](const auto& value){return value.ring==current.ring && value.ringGeneration==current.generation &&
        (value.reservation.absoluteEnd<=current.tail || value.next.first==value.next.end);});
    const auto cached=std::find_if(state.cursors.begin(),state.cursors.end(),[&](const auto& value){return value.address==call->cursorAddress &&
        value.ring==current.ring && value.ringGeneration==current.generation && value.thread==call->thread;});
    if(cached!=state.cursors.end())call->previous=*cached;
    const bool cachedPath=call->cursor.first<call->cursor.end && instances<=uint64_t(call->cursor.end)-call->cursor.first;
    detail::WriterRange planned;
    if(cachedPath){const detail::Cursor after{call->cursor.first+instances,call->cursor.end};
        if(!detail::ResolveWriter(call->cursorAddress,current.ring,current.generation,call->thread,uint32_t(current.capacity),
            call->cursor,after,call->cursor.first,instances,{},call->previous,planned)){
            if(!call->backing->quarantined && (state.ledger.Begin(current,call->scope,call->ticket)!=instance_ring::Status::Observed ||
                !Okay(state.ledger.InvalidatePhysical(current,call->cursor.first,instances)))){
                call->completed=true;LocalUnknown(current.ring,Status::Range);return result;}
            std::erase_if(state.cursors,[&](const auto& value){return value.address==call->cursorAddress;});
            call->untrackedCached=true;detail::writer=call.get();result.data_=std::move(call);return result;
        }
    }
    if(!call->backing->quarantined && state.ledger.Begin(current,call->scope,call->ticket)!=instance_ring::Status::Observed){
        call->completed=true;LocalUnknown(current.ring,Status::Changed);return result;}
    if(call->ticket.id && cachedPath && state.ledger.QualifyWrite(current,call->ticket,planned.absoluteBegin,call->cursor.first,instances)!=instance_ring::Status::Observed){
        call->completed=true;LocalUnknown(current.ring,Status::Cache);return result;}
    detail::writer=call.get();result.data_=std::move(call);return result;
}catch(...){Lose(Status::Exception);return {};}
Call Reservation(void* ring,uint32_t instances,uintptr_t caller)noexcept try {
    // Kept for source compatibility only. The entry hook cannot observe a
    // concurrent allocator's absolute CAS result; Root installs the witness.
    (void)ring;(void)instances;(void)caller;return {};
}catch(...){Lose(Status::Exception);return {};}
Call Publication(void* ring,uintptr_t caller)noexcept try {
    Call result;constexpr std::array<uint32_t,3> returns{0x4F34AF,0x4F34BB,0x4F34C7};
    if(Enabled() && !NativeCaller(caller,returns)){Unobserved(reinterpret_cast<uintptr_t>(ring),Status::Caller);return result;}
    auto call=Start(detail::Kind::Publish,reinterpret_cast<uintptr_t>(ring));if(!call)return result;
    call->publicationStamp=call->scope.stamp;result.data_=std::move(call);return result;
}catch(...){Lose(Status::Exception);return {};}
void Returned(Call& token,uint32_t first)noexcept try {
    if(!token.data_)return;auto& call=*token.data_;if(call.completed)return;
    std::unique_lock lock(state.gate);
    if(!Healthy() || !Enabled() || call.thread!=GetCurrentThreadId()){Finish(call);Lose(Status::Changed);ApplyLoss();return;}
    instance_ring::RingObservation current;if(!Physical(*call.backing,current)){Finish(call);LocalUnknown(call.backing->observed.ring,Status::Changed);return;}
    if(call.kind!=detail::Kind::Writer){Finish(call);Lose(Status::Returned);ApplyLoss();return;}
    if(call.untrackedCached){detail::Cursor after;
        if(call.actual || first!=call.cursor.first || !Read(nullptr,call.cursorAddress,&after,sizeof(after)) ||
            after.first!=first+call.instances || after.end!=call.cursor.end)LocalUnknown(current.ring,Status::Cache);
        else if(!call.ticket.id || !Okay(state.ledger.FinishUnobserved(current,call.ticket,first,call.instances)))LocalUnknown(current.ring,Status::Cache);
        CyberpunkVR_NativeInstanceObserverDiagnostics[21].fetch_add(1,std::memory_order_relaxed);Finish(call);return;}
    detail::Cursor after;detail::WriterRange range;
    if(!Read(nullptr,call.cursorAddress,&after,sizeof(after)) || !detail::ResolveWriter(call.cursorAddress,current.ring,current.generation,call.thread,
        uint32_t(current.capacity),call.cursor,after,first,call.instances,call.actual,call.previous,range)){
        Finish(call);LocalUnknown(current.ring,Status::Cache);return;}
    instance_ring::Range checked;if(!instance_ring::InstanceRange(current,range.absoluteBegin,first,call.instances,checked)){
        Finish(call);LocalUnknown(current.ring,Status::Range);return;}
    const auto discard=[&](Status reason){state.ledger.DiscardCompleted(call.ticket);Finish(call);last=reason;};
    const auto qualified=state.ledger.QualifyWrite(current,call.ticket,range.absoluteBegin,first,call.instances);
    if(qualified!=instance_ring::Status::Observed){discard(Status::Pending);return;}
    if(range.cache){const auto existing=std::find_if(state.cursors.begin(),state.cursors.end(),[&](const auto& value){return value.address==call.cursorAddress;});
        if(existing!=state.cursors.end())*existing=*range.cache;else{if(state.cursors.size()>=MaxCursors){Finish(call);Lose(Status::Capacity);ApplyLoss();return;}state.cursors.push_back(*range.cache);}}
    if(state.ledger.ValidateWrite(call.ticket)!=instance_ring::Status::Observed){discard(Status::Pending);return;}
    lock.unlock();
    std::vector<uint8_t> bytes(size_t(checked.bytes)),again(bytes.size());
    const bool sampled=Read(nullptr,current.mapped+checked.byteOffset,bytes.data(),bytes.size()) && Read(nullptr,current.mapped+checked.byteOffset,again.data(),again.size()) &&
        bytes==again;
    lock.lock();
    if(!sampled || !Healthy()){discard(Status::Sample);if(!Healthy())ApplyLoss();return;}
    if(!Physical(*call.backing,current)){Finish(call);LocalUnknown(call.backing->observed.ring,Status::Changed);return;}
    if(state.ledger.ValidateWrite(call.ticket)!=instance_ring::Status::Observed){discard(Status::Pending);return;}
    const instance_ring::CompletedWriter completed{call.ticket,range.absoluteBegin,first,call.instances};
    if(state.ledger.Complete(current,completed,bytes)!=instance_ring::Status::Completed){discard(Status::Pending);return;}
    std::erase_if(state.writers,[](const auto& value){return value.presence.expired();});
    const auto presence=state.ledger.CompletedPresence(call.ticket);
    if(presence.expired() || state.writers.size()>=MaxWriters){Finish(call);LocalUnknown(current.ring,Status::Capacity);return;}
    state.writers.push_back({call.backing,completed,range.reservation,presence});
    CyberpunkVR_NativeInstanceObserverDiagnostics[20].fetch_add(1,std::memory_order_relaxed);Finish(call);last=Status::Observed;
}catch(...){Lose(Status::Exception);std::lock_guard lock(state.gate);if(token.data_)Finish(*token.data_);ApplyLoss();}
void Returned(Call& token)noexcept try {
    if(!token.data_)return;auto& call=*token.data_;if(call.completed)return;
    std::lock_guard lock(state.gate);
    if(call.kind!=detail::Kind::Publish || !Healthy() || !Enabled() || call.thread!=GetCurrentThreadId()){Finish(call);Lose(Status::Returned);ApplyLoss();return;}
    instance_ring::RingObservation current;uint64_t data{};uint32_t capacity{},count{},index{};std::array<uint64_t,2> entry{},again{};
    if(!Physical(*call.backing,current)){Finish(call);LocalUnknown(call.backing->observed.ring,Status::Changed);return;}
    if(current.head!=call.before.head ||
        !Read(nullptr,current.ring+0x1C,&data,8) || !Read(nullptr,current.ring+0x24,&capacity,4) ||
        !Read(nullptr,current.ring+0x28,&count,4) || !Read(nullptr,current.ring+0x30,&index,4) || !data || !capacity || capacity>4096 ||
        !count || count>capacity || index>=capacity || data>UINT64_MAX-uint64_t(index)*16-16 ||
        !Read(nullptr,data+uint64_t(index)*16,entry.data(),16) || !Read(nullptr,data+uint64_t(index)*16,again.data(),16) || entry!=again ||
        entry[0]!=call.publicationStamp || entry[1]!=call.before.head || !Healthy() ||
        state.ledger.Publish(current,{call.scope,entry[1]})!=instance_ring::Status::Published){
        Finish(call);last=Status::Changed;CyberpunkVR_NativeInstanceObserverDiagnostics[22].fetch_add(1,std::memory_order_relaxed);
        if(!Healthy())ApplyLoss();return;}
    Finish(call);last=Status::Observed;
}catch(...){Lose(Status::Exception);std::lock_guard lock(state.gate);if(token.data_)Finish(*token.data_);ApplyLoss();}
bool QueryAtSource(ID3D12GraphicsCommandList* list,const geometry::DrawResources& source,bool indexed,const native_recording::Lease& recording,
    const preparation::GraphParentRecord& parent,uint64_t occurrence,SourceCandidate& output)noexcept try {
    output={};query={};Put(QueryField::SourceList,reinterpret_cast<uintptr_t>(list));
    Put(QueryField::SourceGeneration,recording.generation);Put(QueryField::SourceOccurrence,occurrence);Put(QueryField::SourceEye,source.draw.side);
    if(!Enabled())return Fail(Status::Disabled);if(!Compatible())return Fail(Status::Code);
    std::lock_guard lock(state.gate);
    if(!ResetAtIdle())return Fail(Status::Pending);
    Put(QueryField::ObserverEpoch,state.epoch);if(!Healthy()){ApplyLoss();return Fail(Status::Changed);}const auto& draw=source.draw;
    if(!list || !occurrence || draw.list!=reinterpret_cast<uintptr_t>(list) || !(draw.flags&128) || (draw.flags&64) ||
        !(draw.inputSlotMask&128) || !draw.instances)return Fail(Status::Range);
    if(!preparation::RevalidateGraphParent(parent))return Fail(Status::Parent);Put(QueryField::Flags,CurrentParent);
    const auto context=Current();if(!context || context.list!=draw.list || recording.list!=draw.list || recording.context!=context.context ||
        !native_recording::ValidateRecording(recording,list,context))return Fail(Status::Scope);
    Put(QueryField::Flags,query.values[size_t(QueryField::Flags)]|CurrentRecording);
    geometry::DrawResources actual;if(!geometry::CopyCurrentBindings(list,draw,indexed,actual))return Fail(Status::Scope);
    const auto& held=source.vertices[7];const auto& currentLease=actual.vertices[7];const auto& view=draw.vertices[7];
    if(!held.resource || currentLease.resource!=held.resource || held.gpu!=currentLease.gpu || held.bytes!=currentLease.bytes ||
        std::memcmp(&view,&actual.draw.vertices[7],sizeof(view)) || held.native.handle!=currentLease.native.handle ||
        held.native.registry!=recording.registry || held.native.kind!=5 || held.heapType!=D3D12_HEAP_TYPE_UPLOAD)return Fail(Status::Backing);
    native_probe::DrawRecord actualDraw;scene_state::DrawState currentState;
    if(!native_probe::CurrentState(list,actualDraw) || actualDraw.pipeline!=draw.pipeline || actualDraw.root!=draw.root ||
        !scene_state::CaptureDrawState(list,reinterpret_cast<ID3D12PipelineState*>(actualDraw.pipeline),currentState) || currentState.original!=source.state.original ||
        reinterpret_cast<uintptr_t>(currentState.original.Get())!=draw.pipeline)return Fail(Status::Layout);
    const auto shader=shader_snapshot::Capture(currentState.original.Get());if(!shader || !detail::InstanceLayout(shader->input,view.StrideInBytes))return Fail(Status::Layout);
    Put(QueryField::Flags,query.values[size_t(QueryField::Flags)]|CurrentIa);const auto relative=uint64_t(draw.firstInstance)*view.StrideInBytes;
    const auto bytes=uint64_t(draw.instances)*view.StrideInBytes;
    if(view.BufferLocation<held.gpu || view.BufferLocation-held.gpu>held.bytes || view.SizeInBytes>held.bytes-(view.BufferLocation-held.gpu) ||
        relative>view.SizeInBytes || bytes>view.SizeInBytes-relative)return Fail(Status::Range);
    const auto offset=view.BufferLocation-held.gpu+relative;
    Put(QueryField::Resource,reinterpret_cast<uintptr_t>(held.resource.Get()));Put(QueryField::Handle,held.native.handle);Put(QueryField::Kind,held.native.kind);
    Put(QueryField::Stride,view.StrideInBytes);Put(QueryField::Width,held.bytes);Put(QueryField::ViewOffset,view.BufferLocation-held.gpu);
    Put(QueryField::FirstInstance,draw.firstInstance);Put(QueryField::Instances,draw.instances);
    uint64_t stamp{};if(!Stamp(recording.registry,recording.context,stamp))return Fail(Status::Scope);
    const instance_ring::Scope scope{draw.list,recording.generation,occurrence,GetCurrentThreadId(),recording.context,recording.registry,stamp};
    for(auto it=state.writers.rbegin();it!=state.writers.rend();++it){const auto& writer=*it;const auto& backing=*writer.backing;
        if(backing.source.resource!=held.resource || backing.observed.handle!=held.native.handle || backing.observed.registry!=held.native.registry ||
            backing.observed.stride!=view.StrideInBytes || backing.observed.width!=held.bytes)continue;
        const auto writerOffset=uint64_t(writer.completed.firstInstance)*view.StrideInBytes;
        const auto writerBytes=uint64_t(writer.completed.instances)*view.StrideInBytes;
        if(offset<writerOffset || offset-writerOffset>writerBytes || bytes>writerBytes-(offset-writerOffset))continue;
        instance_ring::RingObservation observed;if(!Physical(backing,observed))return Fail(Status::Changed);
        const auto absolute=writer.completed.absoluteBegin+(offset-writerOffset)/view.StrideInBytes;
        if(offset%view.StrideInBytes)return Fail(Status::Range);
        const instance_ring::ReadRequest request{writer.completed.writer,scope,absolute,uint32_t(offset/view.StrideInBytes),draw.instances};
        instance_ring::CpuCandidate cpu;const auto status=state.ledger.Query(observed,request,cpu);
        if(status!=instance_ring::Status::Completed && status!=instance_ring::Status::Published)continue;
        if(!Healthy() || cpu.Bytes().size()!=bytes)return Fail(Status::Changed);
        ComPtr<Receipt> owner;owner.Attach(new Receipt);owner->epoch=state.epoch;owner->backing=writer.backing;owner->cpu=cpu;owner->request=request;owner->recording=recording;
        owner->draw=draw;owner->parent=parent;
        Put(QueryField::Flags,query.values[size_t(QueryField::Flags)]|OwnedCpu|BackingPinned|(status==instance_ring::Status::Published?PublicationObserved:0));
        Put(QueryField::Ring,observed.ring);Put(QueryField::RingGeneration,observed.generation);Put(QueryField::AbsoluteBegin,absolute);
        Put(QueryField::ReservationSequence,writer.reservation.sequence);Put(QueryField::ReservationBegin,writer.reservation.absoluteBegin);
        Put(QueryField::ReservationEnd,writer.reservation.absoluteEnd);Put(QueryField::ReservationHeadBefore,writer.reservation.headBefore);
        Put(QueryField::RetiredTail,observed.tail);Put(QueryField::PublishedStamp,cpu.Published().scope.stamp);Put(QueryField::PublishedHead,cpu.Published().head);
        Put(QueryField::CpuBytes,bytes);const auto& producer=writer.completed.writer.scope;
        Put(QueryField::ProducerList,producer.list);Put(QueryField::ProducerGeneration,producer.generation);Put(QueryField::ProducerOccurrence,producer.occurrence);
        Put(QueryField::ProducerThread,producer.thread);Put(QueryField::ProducerStamp,producer.stamp);Put(QueryField::LiveCpuBytes,state.ledger.LiveCpuBytes());
        last=Status::Observed;Put(QueryField::Status,uint64_t(last));output={cpu,owner,query};return true;
    }return Fail(Status::NoProducer);
}catch(...){output={};Lose(Status::Exception);return Fail(Status::Exception);}
bool ValidateSource(const SourceCandidate& candidate,ID3D12CommandQueue* queue)noexcept try {
    if(!Enabled() || !Compatible() || !Healthy() || !candidate.cpu || !queue)return false;
    std::lock_guard lock(state.gate);
    const auto owner=Owner(candidate.owner.Get());if(!owner || owner->epoch!=state.epoch || !owner->backing ||
        candidate.cpu.Producer().writer!=owner->cpu.Producer().writer || candidate.cpu.Source()!=owner->request ||
        candidate.cpu.Bytes().size()!=owner->cpu.Bytes().size() ||
        !native_recording::ValidateSubmission(owner->recording,queue))return false;
    instance_ring::RingObservation observed;if(!Physical(*owner->backing,observed))return false;instance_ring::CpuCandidate current;
    const auto status=state.ledger.Query(observed,owner->request,current);
    return Healthy() && (status==instance_ring::Status::Completed || status==instance_ring::Status::Published) && current.Bytes().size()==owner->cpu.Bytes().size() &&
        std::equal(current.Bytes().begin(),current.Bytes().end(),owner->cpu.Bytes().begin());
}catch(...){Lose(Status::Exception);return false;}
namespace {
bool RecordGpuCopy(ID3D12GraphicsCommandList* list,const SourceCandidate& candidate,ID3D12Resource* readback,uint64_t offset,
    ID3D12Resource* predicate,uint64_t predicateOffset,ID3D12Resource* privateCopy,uint64_t privateOffset,const instance_snapshot_view::Input* input)noexcept try {
    CyberpunkVR_NativeInstanceGpuProbePhase=1;
    if(!Enabled() || !Compatible() || !Healthy() || !list || !readback || !predicate || predicateOffset%8)return false;
    std::unique_lock lock(state.gate);const auto owner=Owner(candidate.owner.Get());
    CyberpunkVR_NativeInstanceGpuProbePhase=2;
    if(!owner || owner->probeRecorded || owner->epoch!=state.epoch || !owner->backing || !candidate.cpu ||
        candidate.cpu.Producer().writer!=owner->cpu.Producer().writer || candidate.cpu.Source()!=owner->request ||
        list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT || owner->draw.flags&64 ||
        !preparation::RevalidateGraphParent(owner->parent))return false;
    if(bool(privateCopy)!=bool(input))return false;
    if(input){const auto& draw=owner->draw;const auto& args=input->draw;
        if(input->bytes || input->scope.root!=draw.root || input->scope.list!=owner->recording.list || input->scope.generation!=owner->recording.generation ||
            input->scope.occurrence!=owner->request.source.occurrence || input->scope.eye!=draw.side ||
            reinterpret_cast<uintptr_t>(input->pipeline.Get())!=draw.pipeline || shader_snapshot::Capture(input->pipeline.Get())!=input->shader ||
            std::memcmp(&input->original,&draw.vertices[7],sizeof(input->original)) || args.IndexCountPerInstance!=draw.indices || args.InstanceCount!=draw.instances ||
            args.StartIndexLocation!=draw.firstIndex || args.BaseVertexLocation!=draw.baseVertex || args.StartInstanceLocation!=draw.firstInstance)return false;
    }
    const auto context=Current();if(!context || context.list!=owner->recording.list ||
        !native_recording::ValidateRecording(owner->recording,list,context))return false;
    native_probe::DrawRecord actual;geometry::DrawResources bindings;
    CyberpunkVR_NativeInstanceGpuProbePhase=3;
    if(!native_probe::CurrentState(list,actual) || actual.pipeline!=owner->draw.pipeline || actual.root!=owner->draw.root || (actual.flags&64) ||
        !geometry::CopyCurrentBindings(list,owner->draw,true,bindings))return false;
    const auto& held=owner->backing->source;const auto& view=owner->draw.vertices[7];
    if(bindings.vertices[7].resource!=held.resource || bindings.vertices[7].native.handle!=held.native.handle ||
        std::memcmp(&bindings.draw.vertices[7],&view,sizeof(view)))return false;
    instance_ring::RingObservation observed;if(!Physical(*owner->backing,observed))return false;
    CyberpunkVR_NativeInstanceGpuProbePhase=4;
    instance_ring::CpuCandidate current;const auto status=state.ledger.Query(observed,owner->request,current);
    if((status!=instance_ring::Status::Completed && status!=instance_ring::Status::Published) ||
        current.Bytes().size()!=candidate.cpu.Bytes().size() || !std::equal(current.Bytes().begin(),current.Bytes().end(),candidate.cpu.Bytes().begin()))return false;
    const auto bytes=current.Bytes().size();const auto sourceOffset=uint64_t(owner->request.firstInstance)*observed.stride;
    const auto destination=readback->GetDesc(),permit=predicate->GetDesc();D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS flags{};
    if(!bytes || sourceOffset>held.bytes || bytes>held.bytes-sourceOffset || destination.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER ||
        offset>destination.Width || bytes>destination.Width-offset || destination.Flags || permit.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER ||
        predicateOffset>permit.Width || 8>permit.Width-predicateOffset || permit.Flags ||
        FAILED(readback->GetHeapProperties(&heap,&flags)) || heap.Type!=D3D12_HEAP_TYPE_READBACK ||
        FAILED(predicate->GetHeapProperties(&heap,&flags)) || heap.Type!=D3D12_HEAP_TYPE_UPLOAD)return false;
    if(privateCopy){const auto target=privateCopy->GetDesc();
        if(target.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER || target.Flags || privateOffset>target.Width || bytes>target.Width-privateOffset ||
            FAILED(privateCopy->GetHeapProperties(&heap,&flags)) || heap.Type!=D3D12_HEAP_TYPE_DEFAULT)return false;}
    ComPtr<ID3D12Device> device,other;if(FAILED(list->GetDevice(IID_PPV_ARGS(&device))))return false;
    CyberpunkVR_NativeInstanceGpuProbePhase=5;
    for(auto* resource:{held.resource.Get(),readback,predicate})if(FAILED(resource->GetDevice(IID_PPV_ARGS(&other))) || device!=other)return false;
    if(privateCopy && (FAILED(privateCopy->GetDevice(IID_PPV_ARGS(&other))) || device!=other))return false;
    owner->probeRecorded=true;lock.unlock();
    CyberpunkVR_NativeInstanceGpuProbePhase=6;
    if(!gpu::KeepCommandResources(list,{owner.Get(),held.resource.Get(),readback,predicate}))return false;
    if(privateCopy && !gpu::KeepCommandResources(list,{privateCopy}))return false;
    CyberpunkVR_NativeInstanceGpuProbePhase=7;
    {probe::Scope internal;
        if(privateCopy)instance_gpu_probe::Record(list,held.resource.Get(),sourceOffset,privateCopy,privateOffset,bytes,predicate,predicateOffset);
        instance_gpu_probe::Record(list,held.resource.Get(),sourceOffset,readback,offset,bytes,predicate,predicateOffset);}
    CyberpunkVR_NativeInstanceGpuProbePhase=8;
    return true;
}catch(...){return false;}
}
bool RecordGpuProbe(ID3D12GraphicsCommandList* list,const SourceCandidate& candidate,ID3D12Resource* readback,uint64_t offset,
    ID3D12Resource* predicate,uint64_t predicateOffset)noexcept {
    return RecordGpuCopy(list,candidate,readback,offset,predicate,predicateOffset,nullptr,0,nullptr);
}
bool RecordGpuCapture(ID3D12GraphicsCommandList* list,const SourceCandidate& candidate,const instance_snapshot_view::Input& input,
    ID3D12Resource* destination,uint64_t destinationOffset,ID3D12Resource* readback,uint64_t readbackOffset,
    ID3D12Resource* predicate,uint64_t predicateOffset)noexcept {
    if(!destination)return false;
    return RecordGpuCopy(list,candidate,readback,readbackOffset,predicate,predicateOffset,destination,destinationOffset,&input);
}
QueryObservation LastQuery()noexcept{return query;}
Status LastStatus()noexcept{return last;}
void Clear()noexcept {
    std::unique_lock lock(state.gate,std::try_to_lock);if(!lock.owns_lock()){Lose(Status::Contention);return;}
    if(CyberpunkVR_NativeInstanceObserverEnable.load(std::memory_order_acquire) || state.active || state.epoch==UINT64_MAX){Lose(Status::Pending);return;}
    state.ledger.Reset();state.rings.clear();state.cursors.clear();state.writers.clear();state.unknownRings.clear();++state.epoch;
    state.observed.store(false,std::memory_order_release);state.lost.store(false,std::memory_order_release);query={};last=Status::Disabled;
}
}
#endif
