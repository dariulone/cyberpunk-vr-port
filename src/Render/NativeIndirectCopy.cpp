#include "Render/NativeIndirectCopy.hpp"
#include "Render/CommandResources.hpp"
#include "Render/NativeIndirectPolicy.hpp"
#include "Render/RenderProbeScope.hpp"
#include <windows.h>
#include <atomic>
#include <new>

#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
// The native indirect copy at the actual call: rust/crates/cvr-render/src/indirect_copy_native.rs (the engine reads, the
// receipt kept with the list, the native registration, the proof's checks and the per-thread scope ledger). The graph
// parent's revalidation stays in NativePreparation.cpp and comes along with each call.
namespace cvr::stereo::indirect_copy {
static_assert(sizeof(IndirectCopyProof)==40 && offsetof(IndirectCopyProof,state)==8 &&
    offsetof(IndirectCopyProof,recordingGeneration)==16 && offsetof(IndirectCopyProof,allocationOwner)==32 &&
    sizeof(geometry::IndirectBinding)==392,"mirrored by indirect_copy_native.rs' Proof and ia_binding.rs' Binding");
using ParentCheck=bool(*)(const preparation::GraphParentRecord*);
extern "C" {
void cvr_ic_scope_enter(uint64_t,uint64_t,uint64_t,detail::ScopeStamp*);
void cvr_ic_scope_exit(const detail::ScopeStamp*);
bool cvr_ic_scope_active(const detail::ScopeStamp*);
uint32_t cvr_ic_last_failure();
bool cvr_ic_prepare(ID3D12GraphicsCommandList*,const geometry::IndirectBinding*,const preparation::GraphParentRecord*,uint64_t,
                    IndirectCopyProof*,ParentCheck);
bool cvr_ic_validate(ID3D12GraphicsCommandList*,const IndirectCopyProof*,UINT64,UINT64,ParentCheck);
}
namespace {
bool ParentCurrent(const preparation::GraphParentRecord* parent){return preparation::RevalidateGraphParent(*parent);}
}
ActualCallScope::ActualCallScope(ID3D12GraphicsCommandList* list,uint64_t generation,uint64_t position)noexcept{
    cvr_ic_scope_enter(reinterpret_cast<uintptr_t>(list),generation,position,&stamp_);
}
ActualCallScope::~ActualCallScope(){cvr_ic_scope_exit(&stamp_);}
bool ActualCallScope::Active()const noexcept{return cvr_ic_scope_active(&stamp_);}
uint32_t LastFailure(){return cvr_ic_last_failure();}
bool PrepareAtActualCall(ID3D12GraphicsCommandList* list,const geometry::IndirectBinding& binding,
    const preparation::GraphParentRecord& parent,uint64_t position,IndirectCopyProof& output){
    return cvr_ic_prepare(list,&binding,&parent,position,&output,&ParentCurrent);
}
bool Validate(ID3D12GraphicsCommandList* list,const IndirectCopyProof& proof,UINT64 offset,UINT64 bytes){
    return cvr_ic_validate(list,&proof,offset,bytes,&ParentCurrent);
}
}
#endif
#else
namespace cvr::stereo::indirect_copy {
using Microsoft::WRL::ComPtr;
namespace {
thread_local detail::ScopeLedger scopes;
thread_local Failure failure{};
constexpr GUID receiptType{0x45f6d18a,0x394f,0x45c1,{0xb2,0x23,0x2a,0x68,0x17,0x5c,0x04,0x7e}};
constexpr uint64_t AllocationVtable=0x30466A8,AllocatorVtable=0x306ED78;
bool Read(uint64_t address,void* output,size_t bytes){
    SIZE_T copied{};return address && address<=UINT64_MAX-bytes &&
        ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<const void*>(address),output,bytes,&copied) && copied==bytes;
}
template<class T,size_t N>T Field(const std::array<uint8_t,N>& data,size_t offset){
    T value{};std::memcpy(&value,data.data()+offset,sizeof(value));return value;
}
bool Hash(uint64_t base,uint32_t rva,uint32_t count,uint64_t expected){
    std::array<uint8_t,2048> bytes{};
    if(count>bytes.size() || base>UINT64_MAX-rva || !Read(base+rva,bytes.data(),count))return false;
    uint64_t value=14695981039346656037ull;for(UINT i=0;i<count;++i)value=(value^bytes[i])*1099511628211ull;
    return value==expected;
}
bool Device(ID3D12GraphicsCommandList* list,ID3D12DeviceChild* resource){
    ComPtr<ID3D12Device> a,b;return resource && SUCCEEDED(list->GetDevice(IID_PPV_ARGS(&a))) &&
        SUCCEEDED(resource->GetDevice(IID_PPV_ARGS(&b))) && a==b;
}
bool Context(const geometry::IndirectBinding& binding,uint32_t& kind,uint32_t& pending){
    const auto context=binding.context;uint64_t list{},state{},submitted{};
    return context && context.context<=UINT64_MAX-0x640 &&
        Read(context.context+0x30,&list,8) && list==context.list && Read(context.context+0x60,&state,8) && state==context.state &&
        Read(context.context+0x68,&kind,4) && (kind==1 || kind==2) && Read(context.context+0x638,&submitted,8) && !submitted &&
        Read(context.context+0x534,&pending,4) && !pending;
}
bool AllocationBinding(const geometry::IndirectBinding& source,detail::AllocationBinding& result){
    result={};const auto direct=Field<uint64_t>(source.rawRecord,0x18);
    const auto payload=Field<uint64_t>(source.rawRecord,0x20),control=Field<uint64_t>(source.rawRecord,0x28);
    if(direct){if(payload || control)return false;result.allocation=direct;return true;}
    if(!payload || !control || payload>UINT64_MAX-0x38)return false;
    std::array<uint8_t,0x38> bytes{};int32_t refs{};
    if(!Read(payload,bytes.data(),bytes.size()) || !Read(control,&refs,4) || refs<=0 || refs==INT32_MAX)return false;
    result={Field<uint64_t>(bytes,8),payload,control,Field<uint32_t>(bytes,0x10),
        Field<uint64_t>(bytes,0x18),Field<uint64_t>(bytes,0x20),true};
    return result.allocation && result.flags==0xC0 && result.alignment==0x10000 && result.bytes;
}
bool Allocation(uint64_t base,uint64_t address,detail::Allocation& output){
    std::array<uint8_t,0x68> bytes{};
    if(!address || base>UINT64_MAX-AllocationVtable || !Read(address,bytes.data(),bytes.size()))return false;
    const auto refs=Field<uint32_t>(bytes,8);
    detail::Allocation result{address,Field<uint64_t>(bytes,0),Field<uint64_t>(bytes,0x10),
        Field<uint64_t>(bytes,0x18),Field<uint64_t>(bytes,0x28)};
    result.kind=Field<uint32_t>(bytes,0x60)&3;
    if(result.vtable!=base+AllocationVtable || !refs || refs>=INT32_MAX || !result.pimpl || !result.bytes || result.kind==3)return false;
    if(result.kind==1){
        result.block=Field<uint64_t>(bytes,0x48);result.handle=Field<uint64_t>(bytes,0x40);
        if(!result.block || result.block>UINT64_MAX-0x40 || !Read(result.block+0x38,&result.heap,8) || !result.heap)return false;
    }else if(result.kind==2){result.heap=Field<uint64_t>(bytes,0x58);if(!result.heap)return false;}
    output=result;return true;
}
bool Allocator(uint64_t base,const geometry::IndirectBinding& source,const detail::Allocation& allocation,uint64_t& address){
    address=0;if(base>UINT64_MAX-AllocatorVtable || source.arguments.native.registry>UINT64_MAX-0x1410548 ||
        !Read(source.arguments.native.registry+0x1410540,&address,8) || !address)return false;
    std::array<uint8_t,0x18> bytes{};
    if(!Read(address,bytes.data(),bytes.size()) || Field<uint64_t>(bytes,0)!=base+AllocatorVtable ||
        !Field<uint32_t>(bytes,8) || Field<uint32_t>(bytes,8)>=INT32_MAX || Field<uint64_t>(bytes,0x10)!=allocation.pimpl)return false;
    return true;
}
bool Residency(const geometry::IndirectBinding& source){
    uint64_t resident{},tracker{};uint8_t rejected{};
    if(source.recordAddress>UINT64_MAX-0x70 || !Read(source.recordAddress+0x68,&resident,8))return false;
    if(!resident)return true;
    return resident==reinterpret_cast<uintptr_t>(source.arguments.resource.Get()) &&
        source.context.context<=UINT64_MAX-0x638 && Read(source.context.context+0x630,&tracker,8) && tracker &&
        tracker<=UINT64_MAX-38 && Read(tracker+37,&rejected,1) && !rejected;
}
bool TransitionCapacity(const geometry::IndirectBinding& source){
    if(source.state.value&D3D12_RESOURCE_STATE_COPY_SOURCE)return true;
    uint64_t data{};uint32_t capacity{},count{};
    return source.context.context<=UINT64_MAX-0x538 && Read(source.context.context+0x528,&data,8) && data &&
        Read(source.context.context+0x530,&capacity,4) && capacity && capacity<=1048576 &&
        data<=UINT64_MAX-uint64_t(capacity)*32 && Read(source.context.context+0x534,&count,4) && !count;
}
detail::Observation Observe(ID3D12GraphicsCommandList* list,const geometry::IndirectBinding& binding,
    const preparation::GraphParentRecord& parent,uint64_t position){
    detail::Observation value;value.native=binding.arguments.native;value.list=reinterpret_cast<uintptr_t>(list);
    value.resource=reinterpret_cast<uintptr_t>(binding.arguments.resource.Get());value.lowType=binding.rawRecord[6]&15;
    value.generation=binding.recording.generation;value.position=position;value.backingGpu=binding.arguments.gpu;
    value.backingBytes=binding.arguments.bytes;value.offset=binding.argumentOffset;value.bytes=binding.argumentBytes;
    value.recordAddress=binding.recordAddress;value.caller=binding.callerRva;value.maxCount=binding.maxCount;
    value.signatureKind=binding.signatureKind;value.stride=binding.signatureStride;value.heap=binding.arguments.heapType;
    value.flags=binding.arguments.flags;value.state=binding.state;value.fullBacking=binding.fullBacking;
    value.parentCurrent=preparation::RevalidateGraphParent(parent);
    const bool context=Context(binding,value.contextKind,value.pending);
    value.recordingCurrent=context && native_recording::ValidateRecording(binding.recording,list,binding.context);
    value.nativeOrigin=geometry::RevalidateIndirect(list,binding.recording,binding);
    value.sameDevice=Device(list,binding.arguments.resource.Get()) && Device(list,binding.signature.Get());
    if(!context)value.pending=UINT_MAX;return value;
}
struct Receipt final:IUnknown {
    std::atomic<ULONG> refs{1};
    // Native Allocation contains a raw allocator-Pimpl pointer. Release the
    // allocation before its allocator wrapper, including during shutdown.
    ComPtr<IUnknown> allocator,allocation;
    geometry::IndirectBinding binding;
    preparation::GraphParentRecord parent;
    detail::ScopeStamp scope;
    detail::AllocationBinding allocationBinding;
    detail::Allocation physical;
    uint64_t image{},allocatorAddress{};bool ready{};
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** out)override{
        if(!out)return E_POINTER;*out=nullptr;if(id!=__uuidof(IUnknown) && id!=receiptType)return E_NOINTERFACE;
        *out=this;AddRef();return S_OK;
    }
    ULONG STDMETHODCALLTYPE AddRef()override{return ++refs;}
    ULONG STDMETHODCALLTYPE Release()override{const auto count=--refs;if(!count)delete this;return count;}
};
ComPtr<Receipt> Owner(const IndirectCopyProof& proof){
    ComPtr<Receipt> result;
    if(proof.allocationOwner)proof.allocationOwner->QueryInterface(receiptType,reinterpret_cast<void**>(result.GetAddressOf()));
    return result;
}
bool Physical(ID3D12GraphicsCommandList* list,const Receipt& owner){
    detail::AllocationBinding binding;detail::Allocation physical;uint64_t allocator{};
    if(!owner.allocation || !owner.allocator || !AllocationBinding(owner.binding,binding) || binding!=owner.allocationBinding ||
        !Allocation(owner.image,binding.allocation,physical) || physical!=owner.physical ||
        !detail::AllocationForm(binding,physical,reinterpret_cast<uintptr_t>(owner.binding.arguments.resource.Get()),owner.binding.arguments.bytes) ||
        !Allocator(owner.image,owner.binding,physical,allocator) || allocator!=owner.allocatorAddress ||
        reinterpret_cast<uintptr_t>(owner.allocation.Get())!=physical.address || reinterpret_cast<uintptr_t>(owner.allocator.Get())!=allocator)return false;
    if(physical.heap){
        auto* heap=reinterpret_cast<ID3D12Heap*>(physical.heap);
        if(!Device(list,heap) || heap->GetDesc().Properties.Type!=D3D12_HEAP_TYPE_DEFAULT)return false;
    }
    return true;
}
}
ActualCallScope::ActualCallScope(ID3D12GraphicsCommandList* list,uint64_t generation,uint64_t position)noexcept:
    stamp_(scopes.Enter(reinterpret_cast<uintptr_t>(list),generation,position,GetCurrentThreadId())){}
ActualCallScope::~ActualCallScope(){if(stamp_.nonce)scopes.Exit(stamp_);}
bool ActualCallScope::Active()const noexcept{return scopes.Matches(stamp_,stamp_.list,stamp_.generation,stamp_.position,GetCurrentThreadId());}
uint32_t LastFailure(){return uint32_t(failure);}
bool Compatible(){
    static const bool compatible=[] {
        const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
        struct Code{uint32_t rva,bytes;uint64_t hash;};
        constexpr Code functions[]{
            {0x337DA0,384,0xADFE5FFA4DFDB190ull},{0x3E3BB4,54,0xF999B875F95AD550ull},
            {0x3377D0,406,0xCFA7FAC68D39C483ull},{0x3DF6C8,84,0xE6F5013C9EE381A8ull},
            {0x3DF71C,107,0x323015FE897D094Aull},{0x3445AC,1444,0x5C372BC97D5DEB01ull},
            {0x345BD0,103,0x175F3FAEDBA47E55ull},
            {0x689590,467,0x3B4E4CA60342C413ull},{0x6897C0,836,0x384B406967967322ull},
            {0x345734,46,0x9B07C7789C5010D6ull},{0x349CFC,224,0x44B98676710DAFF2ull},
            {0x2FAAF80,16,0x5025DF88EF167533ull},{0x348220,36,0x515FE90101A21305ull},
            {0x348244,195,0x8DFB4CD8294CBA43ull},{0x34AF20,36,0x5EC96C4652F87013ull},
            {0x349F5C,40,0x1B58D5C1FAEB7251ull},{0x18F3F48,52,0x95650F6E1318A268ull},
            {0x18F3FB4,105,0x8550A7D22883C599ull}
        };
        if(!base || !geometry::NativeIndirectCompatible())return false;
        for(const auto& code:functions)if(!Hash(base,code.rva,code.bytes,code.hash))return false;
        uint64_t allocationMethods[5]{},allocatorMethods[5]{};
        if(base>UINT64_MAX-AllocatorVtable || !Read(base+AllocationVtable,allocationMethods,sizeof(allocationMethods)) ||
            !Read(base+AllocatorVtable,allocatorMethods,sizeof(allocatorMethods)))return false;
        const uint64_t a[]{0x2FAC534,0x2FAAF80,0x348220,0x3483A4,0x348244};
        const uint64_t b[]{0x2FAC534,0x2FAAF80,0x348220,0x18F3FB4,0x18F3F48};
        for(UINT i=0;i<5;++i)if(allocationMethods[i]!=base+a[i] || allocatorMethods[i]!=base+b[i])return false;
        return true;
    }();return compatible;
}
bool PrepareAtActualCall(ID3D12GraphicsCommandList* list,const geometry::IndirectBinding& binding,
    const preparation::GraphParentRecord& parent,uint64_t position,IndirectCopyProof& output)try{
    failure=Failure::Arguments;
    if(probe::internalCommands || !list || output.resource || output.state || output.recordingGeneration || output.commandPosition || output.allocationOwner ||
        list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT || !binding.arguments.resource || !binding.signature)return false;
    const auto scope=scopes.Current();failure=Failure::Scope;
    if(!scopes.Matches(scope,reinterpret_cast<uintptr_t>(list),binding.recording.generation,position,GetCurrentThreadId()))return false;
    failure=Failure::Code;if(!Compatible())return false;
    failure=detail::Admit(Observe(list,binding,parent,position),parent);if(failure!=Failure::None)return false;
    failure=Failure::Allocation;detail::AllocationBinding allocationBinding;detail::Allocation physical;
    const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    if(!AllocationBinding(binding,allocationBinding) || !Allocation(base,allocationBinding.allocation,physical) ||
        !detail::AllocationForm(allocationBinding,physical,reinterpret_cast<uintptr_t>(binding.arguments.resource.Get()),binding.arguments.bytes))return false;
    failure=Failure::Allocator;uint64_t allocator{};if(!Allocator(base,binding,physical,allocator))return false;
    ComPtr<Receipt> owner;owner.Attach(new Receipt);owner->image=base;owner->binding=binding;owner->parent=parent;owner->scope=scope;
    owner->allocationBinding=allocationBinding;owner->physical=physical;owner->allocatorAddress=allocator;
    owner->allocator=reinterpret_cast<IUnknown*>(allocator);owner->allocation=reinterpret_cast<IUnknown*>(physical.address);
    failure=Failure::Allocation;if(!Physical(list,*owner.Get()))return false;
    failure=Failure::Residency;if(!Residency(binding))return false;
    failure=Failure::Capacity;if(!TransitionCapacity(binding))return false;
    IndirectCopyProof pending;pending.resource=binding.arguments.resource;pending.recordingGeneration=binding.recording.generation;
    pending.commandPosition=position;pending.allocationOwner=owner.Get();
    failure=Failure::Ownership;if(!gpu::KeepCommandResources(list,{owner.Get()}))return false;
    failure=detail::Admit(Observe(list,binding,parent,position),parent);
    if(failure!=Failure::None)return false;
    failure=Failure::Changed;
    if(!Physical(list,*owner.Get()) || !Residency(binding) ||
        !scopes.Matches(scope,reinterpret_cast<uintptr_t>(list),binding.recording.generation,position,GetCurrentThreadId()))return false;
    const auto required=binding.state.value|UINT(D3D12_RESOURCE_STATE_COPY_SOURCE);
    using Register=void(*)(void*,uint32_t,uint32_t);using Flush=void(*)(void*);
    {
        probe::Scope internal;
        reinterpret_cast<Register>(base+0x337DA0)(reinterpret_cast<void*>(binding.context.context),binding.arguments.native.handle,required);
        reinterpret_cast<Flush>(base+0x3E3BB4)(reinterpret_cast<void*>(binding.context.context));
    }
    failure=Failure::Registration;
    owner->binding.state=geometry::ReadStateAtDraw(list,owner->binding.arguments);
    if(owner->binding.state.value!=required || !detail::ReadMask(owner->binding.state,binding.arguments.native.kind,true) ||
        !geometry::RevalidateIndirect(list,owner->binding.recording,owner->binding) || !preparation::RevalidateGraphParent(parent) ||
        !Physical(list,*owner.Get()) || !Residency(owner->binding) ||
        !scopes.Matches(scope,reinterpret_cast<uintptr_t>(list),binding.recording.generation,position,GetCurrentThreadId()))return false;
    owner->ready=true;pending.state=D3D12_RESOURCE_STATES(required);
    if(!Validate(list,pending,binding.argumentOffset,20))return false;
    output=std::move(pending);failure=Failure::None;return true;
}catch(...){failure=Failure::Exception;return false;}
bool Validate(ID3D12GraphicsCommandList* list,const IndirectCopyProof& proof,UINT64 offset,UINT64 bytes)try{
    failure=Failure::Arguments;
    if(probe::internalCommands || !list || !proof.resource || !proof.state || !proof.allocationOwner || bytes!=20 || offset%4)return false;
    const auto owner=Owner(proof);if(!owner || !owner->ready)return false;
    failure=Failure::Scope;
    if(!scopes.Matches(owner->scope,reinterpret_cast<uintptr_t>(list),proof.recordingGeneration,proof.commandPosition,GetCurrentThreadId()))return false;
    failure=Failure::Changed;
    if(owner->image!=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr)) || proof.resource!=owner->binding.arguments.resource ||
        UINT(*proof.state)!=owner->binding.state.value || offset!=owner->binding.argumentOffset || proof.recordingGeneration!=owner->binding.recording.generation ||
        !detail::ReadMask(owner->binding.state,owner->binding.arguments.native.kind,true) || !Physical(list,*owner.Get()) || !Residency(owner->binding))return false;
    failure=detail::Admit(Observe(list,owner->binding,owner->parent,proof.commandPosition),owner->parent);
    if(failure!=Failure::None)return false;
    failure=Failure::None;return true;
}catch(...){failure=Failure::Exception;return false;}
bool ValidateSubmission(const IndirectCopyProof& proof,ID3D12CommandQueue* queue)try{
    failure=Failure::Arguments;if(!queue)return false;
    const auto owner=Owner(proof);
    failure=Failure::Ownership;if(!owner || !owner->ready || !owner->allocation || !owner->allocator)return false;
    failure=Failure::Changed;
    if(owner->image!=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr)) || proof.resource!=owner->binding.arguments.resource || !proof.state ||
        UINT(*proof.state)!=owner->binding.state.value || proof.recordingGeneration!=owner->binding.recording.generation ||
        proof.commandPosition!=owner->scope.position)return false;
    failure=Failure::Binding;if(!native_recording::ValidateSubmission(owner->binding.recording,queue))return false;
    failure=Failure::None;return true;
}catch(...){failure=Failure::Exception;return false;}
}
#endif
