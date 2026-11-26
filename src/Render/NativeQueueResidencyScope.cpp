#include "Render/NativeQueueResidencyScope.hpp"
#include "Render/NativeTextureRead.hpp"
#include <windows.h>
#include <utility>

namespace cvr::stereo::queue_residency {
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
// The runtime: rust/crates/cvr-render/src/queue_residency.rs. A Scope holds the Rust proof (a COM object); the
// pending pageable use's Data stays here, filled by Rust.
struct CvrQrInvocation {uint64_t thread,queue,listsAddress;const uint64_t* lists;size_t count;};
extern "C" {
bool cvr_qr_capture(ID3D12CommandQueue*,uint32_t,uint64_t,IUnknown**);
bool cvr_qr_validate(IUnknown*);
bool cvr_qr_contains_recording_runtime(IUnknown*,const native_recording::Lease*);
bool cvr_qr_contains_runtime(IUnknown*,const native_recording::Lease*,uint64_t,uint32_t,uint64_t);
bool cvr_qr_describe_invocation(IUnknown*,CvrQrInvocation*);
bool cvr_qr_bind_pageable_use(IUnknown*,const native_recording::Lease*,const texture_read::ImmutableLease*,
    texture_read::ImmutablePageable*,detail::PageableMembership*,uint32_t*);
bool cvr_qr_validate_pageable_use(IUnknown*,const native_recording::Lease*,const texture_read::ImmutablePageable*,
    const detail::PageableMembership*);
uint32_t cvr_qr_last_failure();
void cvr_qr_last_details(std::array<uint64_t,12>*);
void cvr_qr_membership_details(std::array<uint64_t,12>*);
void cvr_qr_pageable_search_details(std::array<uint64_t,20>*);
}
static_assert(sizeof(native_recording::Lease)==40 && sizeof(texture_read::ImmutablePageable)==64 && sizeof(CvrQrInvocation)==40,
    "mirrored by rust/crates/cvr-render/src/queue_residency.rs");
namespace detail {
struct Proof {Microsoft::WRL::ComPtr<IUnknown> rust;};
}
struct PendingPageableUse::Data {
    Scope scope;native_recording::Lease source;
    texture_read::ImmutablePageable physical;detail::PageableMembership membership;uint32_t pageableKind{};
};
namespace {
IUnknown* Rust(const Scope& scope){return scope.proof?scope.proof->rust.Get():nullptr;}
}
IUnknown* RustHandle(const Scope& scope){return Rust(scope);}
uint32_t LastFailure(){return cvr_qr_last_failure();}
std::array<uint64_t,12> LastDetails(){std::array<uint64_t,12> value{};cvr_qr_last_details(&value);return value;}
std::array<uint64_t,12> MembershipDetails(){std::array<uint64_t,12> value{};cvr_qr_membership_details(&value);return value;}
std::array<uint64_t,20> PageableSearchDetails(){std::array<uint64_t,20> value{};cvr_qr_pageable_search_details(&value);return value;}
bool Capture(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists,Scope& output){
    output={};auto proof=std::make_shared<detail::Proof>();
    if(!cvr_qr_capture(queue,count,reinterpret_cast<uintptr_t>(lists),proof->rust.GetAddressOf()))return false;
    output.proof=std::move(proof);return true;
}
bool Validate(const Scope& scope){return cvr_qr_validate(Rust(scope));}
bool ContainsRecording(const Scope& scope,const native_recording::Lease& source){
    return cvr_qr_contains_recording_runtime(Rust(scope),&source);
}
bool DescribeInvocation(const Scope& scope,Invocation& output){
    output={};CvrQrInvocation value{};if(!cvr_qr_describe_invocation(Rust(scope),&value))return false;
    output={value.thread,value.queue,value.listsAddress,std::vector<uint64_t>(value.lists,value.lists+value.count)};return true;
}
bool Contains(const Scope& scope,const native_recording::Lease& source,uint64_t registry,uint32_t handle,uint64_t resource){
    return cvr_qr_contains_runtime(Rust(scope),&source,registry,handle,resource);
}
bool BindPageableUse(const Scope& scope,const native_recording::Lease& source,const texture_read::ImmutableLease& asset,
                     PendingPageableUse& output){
    output={};auto data=std::make_shared<PendingPageableUse::Data>();data->scope=scope;data->source=source;
    if(!cvr_qr_bind_pageable_use(Rust(scope),&source,&asset,&data->physical,&data->membership,&data->pageableKind))return false;
    output.data_=std::move(data);return true;
}
bool ValidatePageableUse(const PendingPageableUse& value){
    const auto* data=value.data_.get();
    return cvr_qr_validate_pageable_use(data?Rust(data->scope):nullptr,data?&data->source:nullptr,
        data?&data->physical:nullptr,data?&data->membership:nullptr);
}
#endif
#else
namespace detail {
struct Proof {
    Snapshot snapshot;
    Microsoft::WRL::ComPtr<ID3D12CommandQueue> queue;
    Microsoft::WRL::ComPtr<ID3D12Fence> fence;
    Microsoft::WRL::ComPtr<ID3D12Device> device;
};
}
struct PendingPageableUse::Data {
    Scope scope;native_recording::Lease source;
    texture_read::ImmutablePageable physical;detail::PageableMembership membership;uint32_t pageableKind{};
};
namespace {
thread_local uint32_t failure{};
thread_local std::array<uint64_t,12> details{};
thread_local std::array<uint64_t,12> membershipDetails{};
thread_local detail::PageableSearch pageableSearch{};
void Stage(uint32_t value){failure=value;details[7]=value;}
bool DetailFailure(uint32_t value){details[8]=value;return false;}
void Expected(uint64_t queue,uint32_t count,uint64_t lists){details[9]=queue;details[10]=count;details[11]=lists;}
void Observed(const CONTEXT& context){
    details[0]=context.Rip;details[1]=context.Rsp;details[2]=context.Rbp;details[3]=context.Rsi;
    details[4]=context.R12;details[5]=context.R15;details[6]=context.Rbx;
}
bool Memory(void*,uint64_t address,void* output,size_t bytes){
    SIZE_T copied{};
    return address && address<=UINT64_MAX-bytes &&
        ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<const void*>(address),output,bytes,&copied) && copied==bytes;
}
uint64_t Base(){return reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));}
uint64_t ImageBytes(uint64_t base){
    IMAGE_DOS_HEADER dos{};IMAGE_NT_HEADERS64 nt{};
    if(!detail::Read(Memory,nullptr,base,&dos,sizeof(dos)) || dos.e_magic!=IMAGE_DOS_SIGNATURE ||
        dos.e_lfanew<=0 || dos.e_lfanew>0x100000 ||
        !detail::Field(Memory,nullptr,base,uint64_t(dos.e_lfanew),nt) || nt.Signature!=IMAGE_NT_SIGNATURE ||
        nt.FileHeader.Machine!=IMAGE_FILE_MACHINE_AMD64 || nt.OptionalHeader.Magic!=IMAGE_NT_OPTIONAL_HDR64_MAGIC ||
        nt.OptionalHeader.SizeOfImage<=native_recording::detail::ExecuteReturn ||
        base>UINT64_MAX-nt.OptionalHeader.SizeOfImage)return 0;
    return nt.OptionalHeader.SizeOfImage;
}
bool Compatible(){
    static const bool compatible=[] {
        if(!native_recording::Compatible())return false;
        const auto base=Base();std::array<uint8_t,2048> bytes{};
        for(const auto code:detail::Contract){
            if(!base || base>UINT64_MAX-code.rva || code.bytes>bytes.size() ||
                !detail::Read(Memory,nullptr,base+code.rva,bytes.data(),code.bytes) ||
                native_recording::detail::Fingerprint({bytes.data(),code.bytes})!=code.hash)return false;
        }
        return true;
    }();return compatible;
}
bool Unwind(uint64_t base,detail::Frame& output){
    output={};const auto imageBytes=ImageBytes(base);if(!imageBytes)return DetailFailure(101);
    const auto* tib=reinterpret_cast<const NT_TIB*>(NtCurrentTeb());
    const auto low=reinterpret_cast<uintptr_t>(tib->StackLimit),high=reinterpret_cast<uintptr_t>(tib->StackBase);
    CONTEXT context{};RtlCaptureContext(&context);
    __try{
        for(size_t i=0;i<native_recording::detail::MaxUnwoundFrames;++i){
            Observed(context);
            if(!context.Rip || context.Rsp<low || context.Rsp>=high)return DetailFailure(102);
            if(context.Rip>=base && context.Rip-base<imageBytes){
                if(context.Rip!=base+native_recording::detail::ExecuteReturn)return DetailFailure(103);
                if(context.Rbp<low || context.Rbp>UINT64_MAX-0x73 || context.Rbp+0x73>high ||
                    context.R15>UINT32_MAX || context.Rbx>UINT32_MAX)return DetailFailure(104);
                DWORD64 nativeBase{};const auto nativeEntry=RtlLookupFunctionEntry(context.Rip,&nativeBase,nullptr);
                if(!nativeEntry || nativeBase!=base || nativeEntry->BeginAddress!=0x3E1020 ||
                    nativeEntry->EndAddress<=native_recording::detail::ExecuteReturn)return DetailFailure(105);
                output={context.Rip,context.Rsp,context.Rbp,context.Rsi,context.R12,
                    uint32_t(context.R15),int32_t(uint32_t(context.Rbx))};
                details[8]=0;return true;
            }
            DWORD64 imageBase{};const auto entry=RtlLookupFunctionEntry(context.Rip,&imageBase,nullptr);
            if(!entry)return DetailFailure(106);
            const auto previous=context.Rsp;PVOID handlerData{};DWORD64 establisher{};
            RtlVirtualUnwind(UNW_FLAG_NHANDLER,imageBase,context.Rip,entry,&context,&handlerData,&establisher,nullptr);
            if(context.Rsp<=previous)return DetailFailure(107);
        }
    }__except(EXCEPTION_EXECUTE_HANDLER){return DetailFailure(109);}
    return DetailFailure(108);
}
bool FenceValid(const detail::Proof& proof){
    if(!proof.queue || !proof.fence || !proof.device)return DetailFailure(201);
    if(FAILED(proof.device->GetDeviceRemovedReason()))return DetailFailure(202);
    if(proof.fence->GetCompletedValue()>=proof.snapshot.fenceValue)return DetailFailure(203);
    details[8]=0;return true;
}
bool Live(const detail::Proof& proof){
    const auto& saved=proof.snapshot;Expected(saved.frame.queue,saved.frame.count,saved.listsAddress);
    Stage(2);if(!Compatible())return false;
    Stage(5);
    if(saved.thread!=GetCurrentThreadId() || saved.base!=Base())return false;
    detail::Frame frame;Stage(3);if(!Unwind(saved.base,frame))return false;
    detail::Snapshot current;Stage(4);uint32_t rejection{};
    const bool read=detail::ReadScope(Memory,nullptr,saved.base,saved.thread,frame,saved.frame.queue,saved.frame.count,saved.listsAddress,current,&rejection);
    details[8]=rejection;if(!read)return false;
    Stage(5);if(!detail::Same(saved,current))return false;
    Stage(6);return FenceValid(proof);
}
void ObserveSiblingPageables(const detail::Proof& scope,uint64_t list,uint64_t pageable)noexcept{
    const auto priorFailure=failure;const auto priorDetails=details;const auto priorMembership=membershipDetails;
    try{
        detail::PageableSearch first,second;
        if(!Live(scope))pageableSearch={1,0,508,pageable,list};
        else if(!detail::SearchInvocationPageable(Memory,nullptr,scope.snapshot,list,pageable,first))pageableSearch=first;
        else if(!detail::SearchInvocationPageable(Memory,nullptr,scope.snapshot,list,pageable,second))pageableSearch=second;
        else if(first!=second){pageableSearch=second;pageableSearch[1]=0;pageableSearch[2]=507;}
        else{pageableSearch=second;if(!Live(scope)){pageableSearch[1]=0;pageableSearch[2]=508;}}
    }catch(...){pageableSearch={1,0,509,pageable,list};}
    failure=priorFailure;details=priorDetails;membershipDetails=priorMembership;
}
}
uint32_t LastFailure(){return failure;}
std::array<uint64_t,12> LastDetails(){return details;}
std::array<uint64_t,12> MembershipDetails(){return membershipDetails;}
std::array<uint64_t,20> PageableSearchDetails(){return pageableSearch;}
bool Capture(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists,Scope& output)try{
    output={};details={};membershipDetails={};membershipDetails[11]=UINT64_MAX;Stage(1);
    Expected(reinterpret_cast<uintptr_t>(queue),count,reinterpret_cast<uintptr_t>(lists));
    if(!queue || !count || count>detail::MaxLists || !lists || queue->GetDesc().Type!=D3D12_COMMAND_LIST_TYPE_DIRECT)return false;
    Stage(2);if(!Compatible())return false;
    const auto base=Base();detail::Frame frame;Stage(3);if(!Unwind(base,frame))return false;
    auto proof=std::make_shared<detail::Proof>();Stage(4);uint32_t rejection{};
    const bool read=detail::ReadScope(Memory,nullptr,base,GetCurrentThreadId(),frame,reinterpret_cast<uintptr_t>(queue),count,
        reinterpret_cast<uintptr_t>(lists),proof->snapshot,&rejection);
    details[8]=rejection;if(!read)return false;
    proof->queue=queue;proof->fence=reinterpret_cast<ID3D12Fence*>(proof->snapshot.fence);Stage(6);
    Microsoft::WRL::ComPtr<ID3D12Device> fenceDevice;
    if(FAILED(queue->GetDevice(IID_PPV_ARGS(&proof->device))) || FAILED(proof->fence->GetDevice(IID_PPV_ARGS(&fenceDevice))) ||
        !proof->device || proof->device!=fenceDevice || !FenceValid(*proof))return false;
    if(!Live(*proof))return false;
    output.proof=std::move(proof);Stage(0);return true;
}catch(...){output={};Stage(9);return false;}
bool Validate(const Scope& scope)try{
    details={};membershipDetails={};membershipDetails[11]=UINT64_MAX;Stage(1);if(!scope.proof)return false;
    if(!Live(*scope.proof))return false;
    Stage(0);return true;
}catch(...){Stage(9);return false;}
bool ContainsRecording(const Scope& scope,const native_recording::Lease& source)try{
    details={};membershipDetails={};membershipDetails[11]=UINT64_MAX;Stage(1);if(!scope.proof || !source.token)return false;
    if(!Live(*scope.proof))return false;
    Stage(7);if(!native_recording::ValidateSubmission(source,scope.proof->queue.Get()))return false;
    Stage(8);uint32_t rejection{};
    const bool accepted=detail::ContainsRecording(Memory,nullptr,scope.proof->snapshot,source.context,source.list,source.registry,&rejection,&membershipDetails);
    details[8]=rejection;if(!accepted)return false;
    Stage(0);return true;
}catch(...){Stage(9);return false;}
bool DescribeInvocation(const Scope& scope,Invocation& output)try{
    output={};Stage(1);if(!scope.proof || !Live(*scope.proof))return false;
    const auto& snapshot=scope.proof->snapshot;
    Invocation value{snapshot.thread,snapshot.frame.queue,snapshot.listsAddress,snapshot.lists};
    if(!Live(*scope.proof))return false;
    output=std::move(value);Stage(0);return true;
}catch(...){output={};Stage(9);return false;}
bool Contains(const Scope& scope,const native_recording::Lease& source,uint64_t registry,uint32_t handle,uint64_t resource)try{
    details={};membershipDetails={};membershipDetails[11]=UINT64_MAX;Stage(1);if(!scope.proof || !source.token)return false;
    if(!Live(*scope.proof))return false;
    Stage(7);
    if(source.registry!=registry || !native_recording::ValidateSubmission(source,scope.proof->queue.Get()))return false;
    Stage(8);uint32_t rejection{};
    const bool accepted=detail::Contains(Memory,nullptr,scope.proof->snapshot,source.context,source.list,registry,handle,resource,&rejection,&membershipDetails);
    details[8]=rejection;if(!accepted)return false;
    Stage(0);return true;
}catch(...){Stage(9);return false;}
bool BindPageableUse(const Scope& scope,const native_recording::Lease& source,const texture_read::ImmutableLease& asset,
                     PendingPageableUse& output)try{
    output={};details={};membershipDetails={};pageableSearch={};membershipDetails[11]=UINT64_MAX;Stage(1);
    if(!scope.proof || !source.token || !asset.owner || !asset.source.resource)return false;
    if(!Live(*scope.proof))return false;
    Stage(7);if(source.registry!=asset.source.native.registry ||
        !native_recording::ValidateSubmission(source,scope.proof->queue.Get()))return false;
    auto proof=std::make_shared<PendingPageableUse::Data>();proof->scope=scope;proof->source=source;
    Stage(10);
    if(!texture_read::GetImmutablePageable(scope.proof->device.Get(),asset,proof->physical) ||
        !texture_read::ValidateImmutable(asset,scope.proof->queue.Get()))return false;
    if(proof->physical.pageable.Get()==static_cast<ID3D12Pageable*>(asset.source.resource.Get()))proof->pageableKind=1;
    else{
        Microsoft::WRL::ComPtr<ID3D12Heap> heap;
        if(!proof->physical.pageable || FAILED(proof->physical.pageable.As(&heap)) || !heap)return false;
        proof->pageableKind=2;
    }
    Stage(11);uint32_t rejected{};
    if(!detail::ReadPageableMembership(Memory,nullptr,scope.proof->snapshot,source.context,source.list,source.registry,
        reinterpret_cast<uintptr_t>(proof->physical.pageable.Get()),proof->membership,&rejected,&membershipDetails)){
        details[8]=rejected;
        if(rejected==409)ObserveSiblingPageables(*scope.proof,source.list,reinterpret_cast<uintptr_t>(proof->physical.pageable.Get()));
        return false;
    }
    Stage(7);if(!native_recording::ValidateSubmission(source,scope.proof->queue.Get()))return false;
    if(!Live(*scope.proof))return false;
    Stage(10);if(!texture_read::ValidateImmutable(proof->physical.source,scope.proof->queue.Get()))return false;
    output.data_=std::move(proof);Stage(0);return true;
}catch(...){output={};Stage(9);return false;}
bool ValidatePageableUse(const PendingPageableUse& value)try{
    details={};membershipDetails={};pageableSearch={};membershipDetails[11]=UINT64_MAX;Stage(1);
    if(!value.data_ || !value.data_->scope.proof)return false;
    const auto& saved=*value.data_;const auto& scope=*saved.scope.proof;
    if(!Live(scope))return false;
    Stage(10);texture_read::ImmutablePageable physical;
    if(!texture_read::GetImmutablePageable(scope.device.Get(),saved.physical.source,physical) ||
        physical.pageable.Get()!=saved.physical.pageable.Get() ||
        !texture_read::ValidateImmutable(saved.physical.source,scope.queue.Get()))return false;
    Stage(11);detail::PageableMembership current;uint32_t rejected{};
    if(!detail::ReadPageableMembership(Memory,nullptr,scope.snapshot,saved.source.context,saved.source.list,saved.source.registry,
        reinterpret_cast<uintptr_t>(saved.physical.pageable.Get()),current,&rejected,&membershipDetails)){
        details[8]=rejected;
        if(rejected==409)ObserveSiblingPageables(scope,saved.source.list,reinterpret_cast<uintptr_t>(saved.physical.pageable.Get()));
        return false;
    }
    if(current!=saved.membership){details[8]=413;return false;}
    if(!Live(scope))return false;
    Stage(10);if(!texture_read::ValidateImmutable(saved.physical.source,scope.queue.Get()))return false;
    Stage(0);return true;
}catch(...){Stage(9);return false;}
#endif
#if !CVR_RUST_VRIK
bool DescribePageableUse(const PendingPageableUse& value,PageableUseDescription& output){
    output={};if(!value.data_)return false;
    const auto& saved=*value.data_;const auto& member=saved.membership;
    output={member.managed,member.pageable,member.memberIndex,member.list,member.sourceSet,member.generation,saved.pageableKind};return true;
}
#endif
}
