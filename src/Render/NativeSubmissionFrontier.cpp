#include "Render/NativeSubmissionFrontier.hpp"
#include "Render/NativeContextOrder.hpp"
#include "Render/NativeDefinitionTracking.hpp"
#include "Render/NativeGraphParent.hpp"
#include "Render/NativeStereoProbe.hpp"
#include "Render/RenderProbeScope.hpp"
#include "MinHook.h"
#include <windows.h>
#include <intrin.h>
#include <mutex>

extern "C" {
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/declared_moved_exports.rs.
#else
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_SubmissionFrontierEnable{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_SubmissionFrontierInstalled{},CyberpunkVR_SubmissionFrontierState{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_SubmissionFrontierFailure{},CyberpunkVR_SubmissionFrontierSeq{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_SubmissionFrontierBoundaryFailure{};
__declspec(dllexport) uint64_t CyberpunkVR_SubmissionFrontierStats[16]{},CyberpunkVR_SubmissionFrontierDetails[16]{};
__declspec(dllexport) uint64_t CyberpunkVR_SubmissionFrontierRanges{},CyberpunkVR_SubmissionFrontierSlots{},CyberpunkVR_SubmissionFrontierSubmits{};
__declspec(dllexport) uint32_t CyberpunkVR_SubmissionFrontierRangeCount{},CyberpunkVR_SubmissionFrontierSlotCount{},CyberpunkVR_SubmissionFrontierSubmitCount{};
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs.
#else
__declspec(dllexport) uint32_t CyberpunkVR_SubmissionFrontierRangeBytes=24*8,CyberpunkVR_SubmissionFrontierSlotBytes=20*8,CyberpunkVR_SubmissionFrontierSubmitBytes=16*8;
#endif
#if !CVR_RUST_VRIK
extern std::atomic<uint32_t> CyberpunkVR_OwnedExecutionSeq;
extern uint64_t CyberpunkVR_OwnedExecutionStats[24];
#endif
}
#if CVR_RUST_VRIK
// The owner: rust/crates/cvr-render/src/submission_frontier_owner.rs (its rules submission_frontier.rs); the hook's
// frame, the graph identity, ObserveSubmit and the runtime: submission_frontier_host.rs. FindBoundary's sources are
// C++ leases, so it stays here.
extern "C" bool cvr_sfo_find_boundary(uint64_t,uint32_t,uint64_t,const std::array<cvr::stereo::submission_frontier::detail::BoundarySource,2>*,
    const cvr::stereo::native_recording::Lease* const (*)[2],bool,cvr::stereo::submission_frontier::Boundary*);
namespace cvr::stereo::submission_frontier {
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
bool FindBoundary(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists,const std::array<Source,2>& sources,Boundary& output)noexcept{
    std::array<detail::BoundarySource,2> numeric{};
    for(UINT side=0;side<2;++side){const auto& lease=sources[side].recording;const auto& parent=sources[side].parent;
        numeric[side]={lease.list,lease.context,lease.generation,reinterpret_cast<uintptr_t>(lease.token.Get()),lease.registry,
            parent.fingerprint,parent.work,parent.name,parent.slot,parent.handle,parent.viewIndex,parent.status};
    }
    const native_recording::Lease* const leases[2]{&sources[0].recording,&sources[1].recording};
    return cvr_sfo_find_boundary(reinterpret_cast<uintptr_t>(queue),count,reinterpret_cast<uintptr_t>(lists),&numeric,&leases,
        probe::internalCommands,&output);
}
#endif
}
#else
namespace cvr::stereo::submission_frontier {
namespace {
using Native=uint64_t(__fastcall*)(void*,void*,uint32_t,void*,uint32_t,void*);
Native original{};
bool hookCreated{};
std::mutex mutex,installMutex;
detail::AdmissionState admissions;
std::atomic<bool> installed{};
uint64_t installedBase{},nextInvocation{},nextBatch{};
std::array<std::array<uint64_t,24>,detail::MaxRanges> ranges{};
std::array<std::array<uint64_t,20>,detail::MaxRows> slots{};
std::array<std::array<uint64_t,16>,detail::MaxRows> submits{};
std::array<native_recording::Lease,detail::MaxRows> leases{};
detail::Ledger ledger;
struct ObservedBatch {
    detail::BoundaryBatch identity;uint32_t first{};
    std::array<uint32_t,detail::MaxSubmitLists> entries{};
} latestBatch;
thread_local detail::BoundaryFailure boundaryFailure{};
bool BoundaryReject(detail::BoundaryFailure reason)noexcept{
    boundaryFailure=reason;CyberpunkVR_SubmissionFrontierBoundaryFailure=uint32_t(reason);return false;
}
struct Write {
    Write(){CyberpunkVR_SubmissionFrontierSeq.fetch_add(1,std::memory_order_acq_rel);}
    ~Write(){CyberpunkVR_SubmissionFrontierSeq.fetch_add(1,std::memory_order_release);}
};
uint64_t Base(){return reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));}
bool Memory(void*,uint64_t address,void* out,size_t bytes){
    SIZE_T copied{};return address && address<=UINT64_MAX-bytes &&
        ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<const void*>(address),out,bytes,&copied) && copied==bytes;
}
bool GraphRead(uint64_t address,void* out,size_t bytes){return detail::Read(Memory,nullptr,address,out,bytes);}
void PublishAddresses(){
    CyberpunkVR_SubmissionFrontierRanges=reinterpret_cast<uintptr_t>(ranges.data());
    CyberpunkVR_SubmissionFrontierSlots=reinterpret_cast<uintptr_t>(slots.data());
    CyberpunkVR_SubmissionFrontierSubmits=reinterpret_cast<uintptr_t>(submits.data());
}
void Reject(detail::Failure reason){
    CyberpunkVR_SubmissionFrontierFailure=uint32_t(reason);
    if(reason==detail::Failure::Capacity){CyberpunkVR_SubmissionFrontierState=4;++CyberpunkVR_SubmissionFrontierStats[14];}
    else CyberpunkVR_SubmissionFrontierState=3;
}
bool Code(std::array<uint64_t,16>& diagnostic){
    const auto base=Base();diagnostic={};diagnostic[0]=base;if(!base || !native_recording::Compatible())return false;
#if CVR_RUST_VRIK
    static_assert(sizeof(native_recording::detail::Code)==16,"mirrored by code_tables.rs's Row");
    return cvr_nsfr_code(base,detail::Contract.data(),detail::Contract.size(),diagnostic.data()+1);
#else
    std::array<uint8_t,512> bytes{};
    for(const auto code:detail::Contract){
        diagnostic[1]=code.rva;diagnostic[2]=code.hash;diagnostic[3]=0;
        if(base>UINT64_MAX-code.rva || code.bytes>bytes.size() || !GraphRead(base+code.rva,bytes.data(),code.bytes))return false;
        diagnostic[3]=native_recording::detail::Fingerprint({bytes.data(),code.bytes});if(diagnostic[3]!=code.hash)return false;
    }
    return true;
#endif
}
#if CVR_RUST_VRIK
bool SyncFrame(uint64_t base,uint64_t& contextAddress,uint64_t& definition){
    return cvr_nsfr_sync_frame(base,detail::CallerReturn,&contextAddress,&definition);
}
#else
bool SyncFrame(uint64_t base,uint64_t& contextAddress,uint64_t& definition){
    contextAddress=definition=0;const auto* tib=reinterpret_cast<const NT_TIB*>(NtCurrentTeb());
    const auto low=reinterpret_cast<uintptr_t>(tib->StackLimit),high=reinterpret_cast<uintptr_t>(tib->StackBase);
    CONTEXT context{};RtlCaptureContext(&context);
    __try{
        for(size_t i=0;i<native_recording::detail::MaxUnwoundFrames;++i){
            if(!context.Rip || context.Rsp<low || context.Rsp>=high)return false;
            DWORD64 image{};const auto entry=RtlLookupFunctionEntry(context.Rip,&image,nullptr);
            if(!entry)return false;
            if(image==base){
                if(context.Rip!=base+detail::CallerReturn || entry->BeginAddress!=0x655FAC || entry->EndAddress<=detail::CallerReturn)return false;
                contextAddress=context.Rsi;definition=context.Rdi;return contextAddress && definition;
            }
            const auto before=context.Rsp;PVOID handler{};DWORD64 establisher{};
            RtlVirtualUnwind(UNW_FLAG_NHANDLER,image,context.Rip,entry,&context,&handler,&establisher,nullptr);
            if(context.Rsp<=before)return false;
        }
    }__except(EXCEPTION_EXECUTE_HANDLER){return false;}
    return false;
}
#endif
struct GraphIdentity {
    uint32_t status=1;uint64_t consumer{},graphEpoch{},pair{},frameOwner{},fingerprint{},context{},definition{};
    graph_parent::Receipt sync;graph_snapshot::Snapshot graph;
};
GraphIdentity Graph(uint64_t base,uint32_t end,uint32_t mode){
    GraphIdentity result;std::array<uint64_t,24> before{},after{};
    const auto sequence=CyberpunkVR_OwnedExecutionSeq.load(std::memory_order_acquire);
#if CVR_RUST_VRIK
    if(!cvr_nsfr_owned(reinterpret_cast<uintptr_t>(CyberpunkVR_OwnedExecutionStats),sequence,before.data()))return result;
#else
    if((sequence&1) || !GraphRead(reinterpret_cast<uintptr_t>(CyberpunkVR_OwnedExecutionStats),before.data(),sizeof(before)) ||
        (before[0]!=2 && before[0]!=3) || !before[1] || !before[2] || !before[3] || !before[6])return result;
#endif
    result.consumer=before[1];result.graphEpoch=before[2];result.pair=before[3];result.frameOwner=before[6];result.status=2;
    if(!definition_tracking::CopyOwnedDeclarationSnapshot(result.consumer,result.graph))return result;
    result.status=3;if(!SyncFrame(base,result.context,result.definition))return result;
    result.status=4;graph_parent::Context context;uint64_t frameOwner{};uint8_t flags{};
    if(!graph_parent::ReadContext(GraphRead,result.context,context) || context.slot!=end ||
        !detail::Field(Memory,nullptr,result.context,0,frameOwner) || !detail::Field(Memory,nullptr,result.context,0x30,flags) || !(flags&2))return result;
    result.status=5;if(frameOwner!=result.frameOwner)return result;
    result.status=6;
    if(!graph_parent::Resolve(result.graph,context,result.sync) || result.sync.object!=result.definition ||
        result.sync.work!=base+0x655FAC || uint32_t(result.sync.name)!=mode)return result;
    result.status=7;const auto order=context_order::Analyze(result.graph,base,GraphRead);
    if(!order.PrefixReady())return result;
    result.fingerprint=order.fingerprint;result.status=8;
    if(!GraphRead(reinterpret_cast<uintptr_t>(CyberpunkVR_OwnedExecutionStats),after.data(),sizeof(after)) ||
        before!=after || CyberpunkVR_OwnedExecutionSeq.load(std::memory_order_acquire)!=sequence)return result;
    result.status=0;return result;
}
struct CapturedSlot {detail::Slot slot;native_recording::Lease lease;uint32_t failure{};};
uint32_t Begin(void* header,void* control,uint32_t end,void* fences,uint32_t mode,void* job,uint64_t caller,uint64_t capturedEpoch)noexcept try{
    uint64_t invocation{};
    {
        std::lock_guard lock(mutex);Write write;
        if(!admissions.Matches(capturedEpoch)){Reject(detail::Failure::Changed);return UINT32_MAX;}
        PublishAddresses();++CyberpunkVR_SubmissionFrontierStats[0];
        if(CyberpunkVR_SubmissionFrontierRangeCount>=detail::MaxRanges || nextInvocation==UINT64_MAX){Reject(detail::Failure::Capacity);return UINT32_MAX;}
        invocation=++nextInvocation;
    }
    detail::Range range;detail::Failure failure{};
    const auto base=Base();const bool captured=installed.load(std::memory_order_acquire) && base==installedBase &&
        detail::CaptureRange(Memory,nullptr,base,caller,reinterpret_cast<uintptr_t>(header),end,mode,range,failure);
    if(!captured && failure==detail::Failure::None)failure=detail::Failure::Code;
    GraphIdentity identity;if(captured)identity=Graph(base,end,mode);
    std::vector<CapturedSlot> values;values.reserve(range.slots.size());
    if(captured)for(const auto& slot:range.slots){CapturedSlot value;value.slot=slot;
        const auto& context=slot.context;
        const auto producer=detail::ProducerSlot(slot.index);
        const bool publisher=!identity.status && std::any_of(identity.graph.nodes.begin(),identity.graph.nodes.end(),[&](const auto& node){
            return node.bytes[29]==producer && node.work==base+0x332D5C;});
        if(detail::TokenCaptureAllowed(identity.status,publisher,slot)){
            // Capture can only return a genuine existing native reset token.
            // The qualified synchronizer chain protects the bank's unique
            // context owner until original 1FAFA4 transfers it. Unknown graphs
            // do not grant COM-call lifetime, even after coherent raw reads.
            if(!native_recording::Capture(reinterpret_cast<ID3D12GraphicsCommandList*>(context.list),
                {context.address,context.state,context.list},value.lease))value.failure=native_recording::LastFailure();
            detail::Context again;
            if(!detail::ReadContext(Memory,nullptr,range.registry,slot.id,again) || again!=context){value.lease={};value.failure=100+uint32_t(detail::Failure::Changed);}
        }else if(context.address)value.failure=identity.status?200+identity.status:!publisher?209:100+uint32_t(detail::Failure::Context);
        values.push_back(std::move(value));
    }
    std::lock_guard lock(mutex);Write write;
    if(!admissions.Matches(capturedEpoch)){Reject(detail::Failure::Changed);return UINT32_MAX;}
    if(CyberpunkVR_SubmissionFrontierRangeCount>=detail::MaxRanges ||
        values.size()>detail::MaxRows-CyberpunkVR_SubmissionFrontierSlotCount){Reject(detail::Failure::Capacity);return UINT32_MAX;}
    const auto index=CyberpunkVR_SubmissionFrontierRangeCount++;auto& row=ranges[index];
    row={capturedEpoch,invocation,GetCurrentThreadId(),caller,reinterpret_cast<uintptr_t>(header),range.table,range.begin,end,mode,
        reinterpret_cast<uintptr_t>(control),reinterpret_cast<uintptr_t>(fences),reinterpret_cast<uintptr_t>(job),
        CyberpunkVR_SubmissionFrontierSlotCount,values.size(),0,0,uint32_t(failure),identity.status,identity.consumer,identity.graphEpoch,
        identity.pair,identity.frameOwner,identity.status?UINT64_MAX:identity.sync.handle,identity.fingerprint};
    const uint64_t diagnostic[]{base,0,0,0,caller,reinterpret_cast<uintptr_t>(header),range.table,range.begin,end,mode,identity.status,
        identity.context,identity.definition,GetCurrentThreadId(),invocation,uint32_t(failure)};
    std::copy_n(diagnostic,16,CyberpunkVR_SubmissionFrontierDetails);
    if(!captured){++CyberpunkVR_SubmissionFrontierStats[2];Reject(failure);return index;}
    ++CyberpunkVR_SubmissionFrontierStats[1];++CyberpunkVR_SubmissionFrontierStats[identity.status?8:7];
    for(auto& value:values){const auto slotIndex=CyberpunkVR_SubmissionFrontierSlotCount++;const auto& slot=value.slot;const auto& context=slot.context;
        const auto producer=detail::ProducerSlot(slot.index);uint64_t node=UINT64_MAX,view=UINT64_MAX,work{},name{};
        if(!identity.status)for(const auto& source:identity.graph.nodes)if(source.bytes[29]==producer){
            node=source.handle;view=graph_snapshot::Field<uint32_t>(source.bytes,24);work=source.work>=base?source.work-base:0;
            name=graph_snapshot::Field<uint64_t>(source.definition,24);break;
        }
        auto& output=slots[slotIndex];output={capturedEpoch,invocation,index,slot.index,producer,slot.id,context.address,context.list,context.state,
            context.kind,context.submitted,value.lease.generation,reinterpret_cast<uintptr_t>(value.lease.token.Get()),bool(value.lease.token),
            slot.failure!=detail::Failure::None?100+uint32_t(slot.failure):value.failure,node,view,work,name,context.residency};
        ++CyberpunkVR_SubmissionFrontierStats[3];if(context.address)++CyberpunkVR_SubmissionFrontierStats[4];
        if(value.lease.token)++CyberpunkVR_SubmissionFrontierStats[5];else if(context.address)++CyberpunkVR_SubmissionFrontierStats[6];
        if(context.address && context.list){
            const auto ledgerIndex=ledger.Size();
            if(!ledger.Add({context.list,context.address,value.lease.generation,reinterpret_cast<uintptr_t>(value.lease.token.Get()),
                invocation,slotIndex,!identity.status && node!=UINT64_MAX && work==0x332D5C})){Reject(detail::Failure::Capacity);return index;}
            leases[ledgerIndex]=std::move(value.lease);
        }
    }
    if(CyberpunkVR_SubmissionFrontierState.load()<3)CyberpunkVR_SubmissionFrontierState=1;
    return index;
}catch(...){try{std::lock_guard lock(mutex);Write write;Reject(detail::Failure::Exception);}catch(...){}return UINT32_MAX;}
struct Call {
    detail::Admission admission;
    uint32_t row=UINT32_MAX;uint64_t result{};bool returned{};
    Call()noexcept=default;
    Call(const Call&)=delete;Call& operator=(const Call&)=delete;
    bool Admit()noexcept try{
        std::lock_guard lock(mutex);
        return admissions.Admit(CyberpunkVR_SubmissionFrontierEnable.load(std::memory_order_acquire)==1,admission);
    }catch(...){return false;}
    ~Call()noexcept{
        if(!admission.active)return;
        try{std::lock_guard lock(mutex);Write write;
            if(admissions.Matches(admission.epoch) && row<CyberpunkVR_SubmissionFrontierRangeCount && ranges[row][0]==admission.epoch){
                ranges[row][14]=result;ranges[row][15]=returned;
                if(!returned){ranges[row][16]=uint32_t(detail::Failure::Exception);Reject(detail::Failure::Exception);}}
            admissions.Release(admission);
        }catch(...){}
    }
};
uint64_t __fastcall Hook(void* header,void* control,uint32_t end,void* fences,uint32_t mode,void* job){
    if(CyberpunkVR_SubmissionFrontierEnable.load(std::memory_order_acquire)!=1 || probe::internalCommands)
        return original(header,control,end,fences,mode,job);
    Call call;
    if(call.Admit())call.row=Begin(header,control,end,fences,mode,job,reinterpret_cast<uintptr_t>(_ReturnAddress()),call.admission.epoch);
    call.result=original(header,control,end,fences,mode,job);call.returned=true;return call.result;
}
}
bool Install()noexcept try{
    std::lock_guard installLock(installMutex);if(installed.load(std::memory_order_acquire))return installedBase==Base();
    if(hookCreated)return false;
    std::array<uint64_t,16> diagnostic;
    if(!Code(diagnostic)){std::lock_guard lock(mutex);Write write;std::copy(diagnostic.begin(),diagnostic.end(),CyberpunkVR_SubmissionFrontierDetails);
        CyberpunkVR_SubmissionFrontierInstalled=2;Reject(detail::Failure::Code);return false;}
    const auto base=Base();auto* target=reinterpret_cast<void*>(base+detail::HookRva);
    if(MH_CreateHook(target,reinterpret_cast<void*>(&Hook),reinterpret_cast<void**>(&original))!=MH_OK){
        CyberpunkVR_SubmissionFrontierInstalled=3;CyberpunkVR_SubmissionFrontierFailure=uint32_t(detail::Failure::Install);return false;}
    hookCreated=true;installedBase=base;
    if(MH_EnableHook(target)!=MH_OK){
        // Preserve the trampoline even after an ambiguous enable failure. An
        // original call must remain forwardable until host MinHook teardown.
        CyberpunkVR_SubmissionFrontierInstalled=3;CyberpunkVR_SubmissionFrontierFailure=uint32_t(detail::Failure::Install);return false;}
    {std::lock_guard lock(mutex);Write write;PublishAddresses();}
    installed.store(true,std::memory_order_release);CyberpunkVR_SubmissionFrontierInstalled=1;return true;
}catch(...){CyberpunkVR_SubmissionFrontierInstalled=3;CyberpunkVR_SubmissionFrontierFailure=uint32_t(detail::Failure::Exception);return false;}
bool Reset()noexcept try{
    std::lock_guard lock(mutex);
    if(!admissions.Reset(CyberpunkVR_SubmissionFrontierEnable.load(std::memory_order_acquire)!=0))return false;
    Write write;for(size_t i=0;i<ledger.Size();++i)leases[i]={};ledger.Clear();nextInvocation=nextBatch=0;latestBatch={};
    CyberpunkVR_SubmissionFrontierRangeCount=CyberpunkVR_SubmissionFrontierSlotCount=CyberpunkVR_SubmissionFrontierSubmitCount=0;
    std::fill_n(CyberpunkVR_SubmissionFrontierStats,16,0ull);std::fill_n(CyberpunkVR_SubmissionFrontierDetails,16,0ull);
    CyberpunkVR_SubmissionFrontierState=0;CyberpunkVR_SubmissionFrontierFailure=0;CyberpunkVR_SubmissionFrontierBoundaryFailure=0;
    PublishAddresses();return true;
}catch(...){return false;}
void ObserveSubmit(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists)noexcept try{
    if(CyberpunkVR_SubmissionFrontierEnable.load(std::memory_order_acquire)!=1 || probe::internalCommands || !queue || !lists || !count)return;
    std::lock_guard lock(mutex);
    if(CyberpunkVR_SubmissionFrontierEnable.load(std::memory_order_acquire)!=1)return;
    latestBatch={};
    Write write;
    if(count>detail::MaxSubmitLists){Reject(detail::Failure::Bounds);return;}
    std::array<uint64_t,detail::MaxSubmitLists> addresses{};
    if(!detail::Read(Memory,nullptr,reinterpret_cast<uintptr_t>(lists),addresses.data(),count*sizeof(uint64_t))){Reject(detail::Failure::Read);return;}
    bool relevant{};for(UINT i=0;i<count;++i)relevant|=ledger.Mentions(addresses[i]);if(!relevant)return;
    if(count>detail::MaxRows-CyberpunkVR_SubmissionFrontierSubmitCount || nextBatch==UINT64_MAX){Reject(detail::Failure::Capacity);return;}
    const auto batch=++nextBatch;++CyberpunkVR_SubmissionFrontierStats[9];
    const auto first=CyberpunkVR_SubmissionFrontierSubmitCount;
    for(UINT i=0;i<count;++i){const auto match=ledger.Observe(addresses[i],[&](uint32_t index,uint32_t& failure){
            const bool valid=native_recording::ValidateSubmission(leases[index],queue);failure=native_recording::LastFailure();return valid;});
        latestBatch.entries[i]=match.entry;
        auto& row=submits[CyberpunkVR_SubmissionFrontierSubmitCount++];row={admissions.Epoch(),batch,GetCurrentThreadId(),reinterpret_cast<uintptr_t>(queue),count,i,
            addresses[i],uint32_t(match.kind),0,UINT64_MAX,0,match.failure,0,UINT64_MAX,UINT64_MAX,UINT64_MAX};
        if(match.entry!=UINT32_MAX){const auto& key=ledger.At(match.entry);const auto& source=slots[key.row];
            row[8]=key.invocation;row[9]=key.row;row[10]=key.generation;row[12]=key.context;row[13]=source[4];row[14]=source[15];row[15]=source[16];}
        if(match.kind==detail::MatchKind::Matched || match.kind==detail::MatchKind::MatchedUnknownGraph)++CyberpunkVR_SubmissionFrontierStats[10];
        else if(match.kind==detail::MatchKind::UnknownList)++CyberpunkVR_SubmissionFrontierStats[11];
        else if(match.kind==detail::MatchKind::Replay)++CyberpunkVR_SubmissionFrontierStats[13];
        else ++CyberpunkVR_SubmissionFrontierStats[12];
    }
    latestBatch.first=first;latestBatch.identity={admissions.Epoch(),batch,GetCurrentThreadId(),reinterpret_cast<uintptr_t>(queue),
        reinterpret_cast<uintptr_t>(lists),count};
    if(CyberpunkVR_SubmissionFrontierState.load()<3)CyberpunkVR_SubmissionFrontierState=2;
}catch(...){try{std::lock_guard lock(mutex);Write write;Reject(detail::Failure::Exception);}catch(...){}}
bool FindBoundary(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists,const std::array<Source,2>& sources,Boundary& output)noexcept try{
    output={};
    if(!queue || !lists || count<3 || count>detail::MaxSubmitLists)return BoundaryReject(detail::BoundaryFailure::Arguments);
    if(probe::internalCommands || CyberpunkVR_SubmissionFrontierEnable.load(std::memory_order_acquire)!=1)return BoundaryReject(detail::BoundaryFailure::Disabled);
    std::lock_guard lock(mutex);
    if(CyberpunkVR_SubmissionFrontierEnable.load(std::memory_order_acquire)!=1 || !installed.load(std::memory_order_acquire))return BoundaryReject(detail::BoundaryFailure::Disabled);
    const detail::BoundaryBatch current{admissions.Epoch(),nextBatch,GetCurrentThreadId(),reinterpret_cast<uintptr_t>(queue),
        reinterpret_cast<uintptr_t>(lists),count};
    if(latestBatch.identity!=current || latestBatch.first>CyberpunkVR_SubmissionFrontierSubmitCount ||
        count!=CyberpunkVR_SubmissionFrontierSubmitCount-latestBatch.first)return BoundaryReject(detail::BoundaryFailure::Batch);
    const auto base=Base();if(!base || base!=installedBase || base>UINT64_MAX-0x332D5C)return BoundaryReject(detail::BoundaryFailure::Metadata);
    std::array<uint64_t,detail::MaxSubmitLists> addresses{},again{};
    if(!detail::Read(Memory,nullptr,current.lists,addresses.data(),count*sizeof(uint64_t)))return BoundaryReject(detail::BoundaryFailure::Read);
    std::array<detail::BoundaryList,detail::MaxSubmitLists> rows{};
    for(UINT i=0;i<count;++i){const auto& submitted=submits[latestBatch.first+i];auto& row=rows[i];
        if(submitted[0]!=current.probeEpoch || submitted[1]!=current.batch || submitted[2]!=current.thread ||
            submitted[3]!=current.queue || submitted[4]!=count || submitted[5]!=i)return BoundaryReject(detail::BoundaryFailure::Batch);
        if(submitted[6]!=addresses[i])return BoundaryReject(detail::BoundaryFailure::Changed);
        row.list=submitted[6];row.match=detail::MatchKind(submitted[7]);
        if(row.match!=detail::MatchKind::Matched)continue;
        const auto entry=latestBatch.entries[i];if(entry>=ledger.Size())return BoundaryReject(detail::BoundaryFailure::Metadata);
        const auto& key=ledger.At(entry);const auto& lease=leases[entry];
        if(!key.graphKnown || key.row>=CyberpunkVR_SubmissionFrontierSlotCount || submitted[9]!=key.row ||
            submitted[8]!=key.invocation || submitted[10]!=key.generation || submitted[11] || submitted[12]!=key.context ||
            key.list!=row.list || lease.list!=key.list || lease.context!=key.context || lease.generation!=key.generation ||
            reinterpret_cast<uintptr_t>(lease.token.Get())!=key.token || !key.token || !lease.registry)return BoundaryReject(detail::BoundaryFailure::Metadata);
        const auto& slot=slots[key.row];
        if(slot[0]!=current.probeEpoch || slot[1]!=key.invocation || slot[2]>=CyberpunkVR_SubmissionFrontierRangeCount ||
            slot[3]!=slot[4]+1 || slot[4]>=detail::MaxSlots || !slot[5] || slot[6]!=key.context || slot[7]!=key.list ||
            !slot[8] || (slot[9]!=1 && slot[9]!=2) || slot[10] || slot[11]!=key.generation || slot[12]!=key.token ||
            slot[13]!=1 || slot[14] || slot[15]>UINT32_MAX || slot[16]>1 || slot[17]!=0x332D5C ||
            submitted[13]!=slot[4] || submitted[14]!=slot[15] || submitted[15]!=slot[16])return BoundaryReject(detail::BoundaryFailure::Metadata);
        const auto& range=ranges[size_t(slot[2])];
        if(range[0]!=current.probeEpoch || range[1]!=key.invocation || range[16] || range[17] ||
            slot[3]<range[6] || slot[3]>range[7])return BoundaryReject(detail::BoundaryFailure::Metadata);
        row.context=key.context;row.generation=key.generation;row.token=key.token;row.registry=lease.registry;
        row.work=base+slot[17];row.name=slot[18];row.slot=uint32_t(slot[4]);row.node=uint32_t(slot[15]);row.view=uint32_t(slot[16]);
        row.graph={range[1],range[18],range[19],range[20],range[21],range[23],uint32_t(slot[2])};row.qualified=true;
    }
    std::array<detail::BoundarySource,2> numericSources{};
    for(UINT side=0;side<2;++side){const auto& source=sources[side];const auto& lease=source.recording;const auto& parent=source.parent;
        if(parent.work!=base+0x332D5C)return BoundaryReject(detail::BoundaryFailure::Source);
        numericSources[side]={lease.list,lease.context,lease.generation,reinterpret_cast<uintptr_t>(lease.token.Get()),lease.registry,
            parent.fingerprint,parent.work,parent.name,parent.slot,parent.handle,parent.viewIndex,parent.status};
    }
    Boundary boundary;detail::BoundaryFailure reason{};
    if(!detail::SelectBoundary(latestBatch.identity,current,{rows.data(),count},{addresses.data(),count},numericSources,boundary,&reason))return BoundaryReject(reason);
    for(const auto& source:sources)if(!native_recording::ValidateSubmission(source.recording,queue))return BoundaryReject(detail::BoundaryFailure::NativeToken);
    for(UINT i=0;i<count;++i)if(i>=boundary.afterIndex || i==boundary.sourceIndices[0] || i==boundary.sourceIndices[1]){
        const auto entry=latestBatch.entries[i];
        if(entry>=ledger.Size() || !native_recording::ValidateSubmission(leases[entry],queue))return BoundaryReject(detail::BoundaryFailure::NativeToken);
    }
    if(!detail::Read(Memory,nullptr,current.lists,again.data(),count*sizeof(uint64_t)) ||
        !std::equal(addresses.begin(),addresses.begin()+count,again.begin()))return BoundaryReject(detail::BoundaryFailure::Changed);
    output=boundary;boundaryFailure=detail::BoundaryFailure::None;CyberpunkVR_SubmissionFrontierBoundaryFailure=0;return true;
}catch(...){output={};return BoundaryReject(detail::BoundaryFailure::Exception);}
uint32_t LastBoundaryFailure()noexcept{return uint32_t(boundaryFailure);}
}
#endif
