#pragma once
#include "Render/NativeCommandRecording.hpp"
#include "Render/NativePreparation.hpp"
#include <algorithm>
#include <array>
#include <atomic>
#include <cstddef>
#include <cstdint>
#include <span>
#include <utility>
#include <vector>

namespace cvr::stereo::submission_frontier {
struct Source {native_recording::Lease recording;preparation::GraphParentRecord parent;};
struct Boundary {
    uint64_t probeEpoch{},batch{},thread{},queue{},lists{},consumer{},graphEpoch{},pairGeneration{},frameOwner{},fingerprint{},invocation{};
    uint32_t count{},afterIndex{};std::array<uint32_t,2> sourceIndices{};
};
namespace detail {
// 2.5: sub_1422701F0 replaces 2.31 sub_14058DC98 (rewritten, not re-audited: its Contract row never matches).
inline constexpr uint32_t HookRva=0x22701F0,CallerReturn=0x65607B;
inline constexpr uint32_t MaxRanges=64,MaxSlots=256,MaxContexts=128,MaxRows=4096,MaxSubmitLists=256;
inline constexpr uint64_t RendererRva=0x3BBCAE8,RegistryRva=0x3BCDCA8;
using ReadMemory=bool(*)(void*,uint64_t,void*,size_t);
enum class Failure:uint32_t {None,Arguments,Caller,Renderer,Header,Bounds,Read,Changed,Registry,ContextId,Context,Code,Install,Capacity,Exception};
enum class MatchKind:uint32_t {UnknownList,Matched,MatchedUnknownGraph,MissingToken,RejectedToken,Replay,Ambiguous};
enum class BoundaryFailure:uint32_t {None,Arguments,Disabled,Batch,Read,Metadata,Source,SourceMatch,Graph,Suffix,Replay,Duplicate,MissingSource,NoSuffix,NativeToken,Changed,Exception};
struct Context {
    uint64_t address{},list{},state{},residency{},submitted{};uint32_t kind{};
    bool operator==(const Context&)const=default;
};
struct Slot {uint32_t index{},id{};Context context;Failure failure{};};
#if CVR_RUST_VRIK
// The rules: rust/crates/cvr-render/src/submission_frontier.rs (tools/single_pass_tests/submission_frontier_test runs its
// cases against them in its Rust variant). AdmissionState's and Ledger's members stay here, mirrored there.
struct RawRange {uint64_t header{},table{},registry{};uint32_t begin{},end{},mode{};};
struct Key;struct Admission;class AdmissionState;struct Match;class Ledger;struct BoundaryBatch;struct BoundaryList;
extern "C" {
bool cvr_sf_token_capture_allowed(uint32_t,bool,const Slot*);
uint32_t cvr_sf_id(uint32_t,uint32_t);
bool cvr_sf_read(ReadMemory,void*,uint64_t,bool,uint64_t,void*,size_t);
bool cvr_sf_read_context(ReadMemory,void*,uint64_t,uint32_t,Context*);
bool cvr_sf_capture_range(ReadMemory,void*,uint64_t,uint64_t,uint64_t,uint32_t,uint32_t,RawRange*,Slot*,size_t*,Failure*);
uint64_t cvr_sf_admission(AdmissionState*,uint32_t,bool,Admission*,uint64_t);
bool cvr_sf_ledger(Ledger*,uint32_t,const Key*,uint64_t);
void cvr_sf_ledger_observe(Ledger*,uint64_t,bool(*)(void*,uint32_t,uint32_t*),void*,Match*);
bool cvr_sf_select_boundary(const BoundaryBatch*,const BoundaryBatch*,const BoundaryList*,size_t,const uint64_t*,size_t,
    const void*,Boundary*,BoundaryFailure*);
}
inline bool TokenCaptureAllowed(uint32_t graphStatus,bool publisher,const Slot& slot)noexcept{
    return cvr_sf_token_capture_allowed(graphStatus,publisher,&slot);
}
#else
inline bool TokenCaptureAllowed(uint32_t graphStatus,bool publisher,const Slot& slot)noexcept{
    const auto& context=slot.context;
    return !graphStatus && publisher && slot.failure==Failure::None && context.address && context.list && context.state &&
        !context.submitted && (context.kind==1 || context.kind==2);
}
#endif
struct Range {
    uint64_t header{},table{},registry{};uint32_t begin{},end{},mode{};std::vector<Slot> slots;
};
inline constexpr std::array<native_recording::detail::Code,3> Contract{{
    {HookRva,0x84,0x0000000000000000ull},{0x655FAC,242,0x2FCB059521D7B173ull},
    {0x3E7DA0,73,0x3C36DA23B8209163ull}
}};
#if CVR_RUST_VRIK
inline bool Read(ReadMemory read,void* user,uint64_t address,void* out,size_t bytes){return cvr_sf_read(read,user,address,false,0,out,bytes);}
template<class T> bool Field(ReadMemory read,void* user,uint64_t address,uint64_t offset,T& out){
    return cvr_sf_read(read,user,address,true,offset,&out,sizeof(out));
}
inline bool NativeId(uint32_t id){return cvr_sf_id(0,id)!=0;}
inline uint32_t ProducerSlot(uint32_t tableIndex){return cvr_sf_id(1,tableIndex);}
inline bool ReadContext(ReadMemory read,void* user,uint64_t registry,uint32_t id,Context& output){
    return cvr_sf_read_context(read,user,registry,id,&output);
}
// Snapshot before the original range consumer moves contexts and clears IDs.
// A zero/invalid ID is retained as an empty native slot, never dereferenced.
inline bool CaptureRange(ReadMemory read,void* user,uint64_t base,uint64_t caller,uint64_t header,
                         uint32_t end,uint32_t mode,Range& output,Failure& failure){
    output={};RawRange raw;std::vector<Slot> slots(MaxSlots);size_t count{};
    if(!cvr_sf_capture_range(read,user,base,caller,header,end,mode,&raw,slots.data(),&count,&failure))return false;
    slots.resize(count);
    output.header=raw.header;output.table=raw.table;output.registry=raw.registry;
    output.begin=raw.begin;output.end=raw.end;output.mode=raw.mode;output.slots=std::move(slots);return true;
}
#else
inline bool Read(ReadMemory read,void* user,uint64_t address,void* out,size_t bytes){
    return read && address && bytes && address<=UINT64_MAX-bytes && read(user,address,out,bytes);
}
template<class T> bool Field(ReadMemory read,void* user,uint64_t address,uint64_t offset,T& out){
    return address && address<=UINT64_MAX-offset && Read(read,user,address+offset,&out,sizeof(out));
}
inline bool NativeId(uint32_t id){return id && id<0x80000000u;}
inline uint32_t ProducerSlot(uint32_t tableIndex){return tableIndex?tableIndex-1:UINT32_MAX;}
inline bool ReadContext(ReadMemory read,void* user,uint64_t registry,uint32_t id,Context& output){
    output={};if(!registry || !id || id>MaxContexts)return false;
    const auto offset=0xD6EDA8+uint64_t(id-1)*16;
    const auto once=[&](Context& value){
        return Field(read,user,registry,offset,value.address) && value.address &&
            Field(read,user,value.address,0x30,value.list) && Field(read,user,value.address,0x60,value.state) &&
            Field(read,user,value.address,0x68,value.kind) && Field(read,user,value.address,0x630,value.residency) &&
            Field(read,user,value.address,0x638,value.submitted);
    };
    Context a,b;if(!once(a) || !once(b) || a!=b)return false;output=a;return true;
}
// Snapshot before the original range consumer moves contexts and clears IDs.
// A zero/invalid ID is retained as an empty native slot, never dereferenced.
inline bool CaptureRange(ReadMemory read,void* user,uint64_t base,uint64_t caller,uint64_t header,
                         uint32_t end,uint32_t mode,Range& output,Failure& failure){
    output={};failure=Failure::Arguments;
    if(!base || !header || mode>2 || end>=MaxSlots)return false;
    failure=Failure::Caller;if(!native_recording::detail::Caller(base,caller,CallerReturn))return false;
    uint64_t renderer{};failure=Failure::Renderer;
    if(!Field(read,user,base,RendererRva,renderer) || !renderer || renderer>UINT64_MAX-0x4C60 || header!=renderer+0x4C60)return false;
    Range next;next.header=header;next.end=end;next.mode=mode;failure=Failure::Header;
    if(!Field(read,user,header,0,next.table) || !next.table || !Field(read,user,header,0x10,next.begin))return false;
    failure=Failure::Bounds;if(next.begin>end+1 || next.begin>MaxSlots)return false;
    failure=Failure::Registry;if(!Field(read,user,base,RegistryRva,next.registry) || !next.registry)return false;
    const auto count=end+1-next.begin;std::array<uint32_t,MaxSlots> ids{},again{};
    failure=Failure::Read;
    if(count && (next.table>UINT64_MAX-uint64_t(next.begin)*4 ||
        !Read(read,user,next.table+uint64_t(next.begin)*4,ids.data(),size_t(count)*4)))return false;
    next.slots.reserve(count);
    for(uint32_t i=0;i<count;++i){Slot slot;slot.index=next.begin+i;slot.id=ids[i];
        if(NativeId(slot.id)){
            if(slot.id>MaxContexts)slot.failure=Failure::ContextId;
            else if(!ReadContext(read,user,next.registry,slot.id,slot.context))slot.failure=Failure::Context;
        }
        next.slots.push_back(slot);
    }
    uint64_t table{},registryAfter{},rendererAfter{};uint32_t begin{};failure=Failure::Changed;
    if(!Field(read,user,header,0,table) || table!=next.table || !Field(read,user,header,0x10,begin) || begin!=next.begin ||
        !Field(read,user,base,RegistryRva,registryAfter) || registryAfter!=next.registry ||
        !Field(read,user,base,RendererRva,rendererAfter) || rendererAfter!=renderer ||
        (count && (!Read(read,user,next.table+uint64_t(next.begin)*4,again.data(),size_t(count)*4) ||
            !std::equal(ids.begin(),ids.begin()+count,again.begin()))))return false;
    output=std::move(next);failure=Failure::None;return true;
}
#endif
struct Key {
    uint64_t list{},context{},generation{},token{},invocation{};uint32_t row{};bool graphKnown{};
};
struct Admission {uint64_t epoch{};bool active{};};
// The owner serializes every operation with the same mutex as its exported
// rows. A fast enabled read is never an admission or an epoch certificate.
#if CVR_RUST_VRIK
class AdmissionState {
public:
    uint64_t Epoch()const noexcept{return Const(0);}
    uint32_t InFlight()const noexcept{return uint32_t(Const(1));}
    bool Admit(bool enabled,Admission& ticket)noexcept{return cvr_sf_admission(this,2,enabled,&ticket,0)!=0;}
    bool Matches(uint64_t epoch)const noexcept{return cvr_sf_admission(const_cast<AdmissionState*>(this),3,false,nullptr,epoch)!=0;}
    bool Release(Admission& ticket)noexcept{return cvr_sf_admission(this,4,false,&ticket,0)!=0;}
    bool Reset(bool enabled)noexcept{return cvr_sf_admission(this,5,enabled,nullptr,0)!=0;}
private:
    uint64_t Const(uint32_t which)const noexcept{return cvr_sf_admission(const_cast<AdmissionState*>(this),which,false,nullptr,0);}
    uint64_t epoch_=1;uint32_t inFlight_{};
};
#else
class AdmissionState {
public:
    uint64_t Epoch()const noexcept{return epoch_;}
    uint32_t InFlight()const noexcept{return inFlight_;}
    bool Admit(bool enabled,Admission& ticket)noexcept{
        if(!enabled || ticket.active || inFlight_==UINT32_MAX)return false;
        ticket={epoch_,true};++inFlight_;return true;
    }
    bool Matches(uint64_t epoch)const noexcept{return inFlight_ && epoch && epoch==epoch_;}
    bool Release(Admission& ticket)noexcept{
        if(!ticket.active || !Matches(ticket.epoch))return false;
        --inFlight_;ticket.active=false;return true;
    }
    bool Reset(bool enabled)noexcept{
        if(enabled || inFlight_ || epoch_==UINT64_MAX)return false;
        ++epoch_;return true;
    }
private:
    uint64_t epoch_=1;uint32_t inFlight_{};
};
#endif
struct Match {MatchKind kind=MatchKind::UnknownList;uint32_t entry=UINT32_MAX,failure{};};
#if CVR_RUST_VRIK
class Ledger {
public:
    bool Add(const Key& key){return cvr_sf_ledger(this,0,&key,0);}
    void Clear(){cvr_sf_ledger(this,1,nullptr,0);}
    size_t Size()const{return count_;}
    const Key& At(size_t index)const{return entries_[index].key;}
    bool Mentions(uint64_t list)const{return cvr_sf_ledger(const_cast<Ledger*>(this),2,nullptr,list);}
    // validate inspects held tokens, never the now-transferred/recycled ID bank.
    template<class Validate>Match Observe(uint64_t list,Validate&& validate){
        using Callable=std::remove_reference_t<Validate>;
        const auto call=[](void* context,uint32_t index,uint32_t* reason)->bool{return (*static_cast<Callable*>(context))(index,*reason);};
        Match result;cvr_sf_ledger_observe(this,list,call,const_cast<void*>(static_cast<const void*>(&validate)),&result);
        return result;
    }
private:
    struct Entry {Key key;bool used{};};std::array<Entry,MaxRows> entries_{};size_t count_{};
};
static_assert(sizeof(Context)==48 && sizeof(Slot)==64 && sizeof(RawRange)==40 && sizeof(Key)==48 && sizeof(Admission)==16 &&
    sizeof(AdmissionState)==16 && sizeof(Match)==12 && sizeof(Ledger)==MaxRows*56+8,"mirrored by submission_frontier.rs");
#else
class Ledger {
public:
    bool Add(const Key& key){if(!key.list || !key.context || count_==entries_.size())return false;entries_[count_++]={key,false};return true;}
    void Clear(){count_=0;}
    size_t Size()const{return count_;}
    const Key& At(size_t index)const{return entries_[index].key;}
    bool Mentions(uint64_t list)const{for(size_t i=0;i<count_;++i)if(entries_[i].key.list==list)return true;return false;}
    // validate inspects held tokens, never the now-transferred/recycled ID bank.
    template<class Validate>Match Observe(uint64_t list,Validate&& validate){
        Match fresh,missing,rejected,replayed;uint32_t validCount{};
        for(uint32_t i=0;i<count_;++i){auto& entry=entries_[i];const auto& key=entry.key;if(key.list!=list)continue;
            if(!key.token || !key.generation){if(missing.entry==UINT32_MAX)missing={MatchKind::MissingToken,i,0};continue;}
            uint32_t reason{};const bool valid=validate(i,reason);
            if(valid && !entry.used){fresh={key.graphKnown?MatchKind::Matched:MatchKind::MatchedUnknownGraph,i,0};++validCount;}
            else if(entry.used && (valid || reason==9 || reason==12)){if(replayed.entry==UINT32_MAX)replayed={MatchKind::Replay,i,reason};}
            else if(rejected.entry==UINT32_MAX)rejected={MatchKind::RejectedToken,i,reason};
        }
        if(validCount>1)return {MatchKind::Ambiguous,UINT32_MAX,0};
        if(validCount==1){entries_[fresh.entry].used=true;return fresh;}
        if(replayed.entry!=UINT32_MAX)return replayed;
        if(rejected.entry!=UINT32_MAX)return rejected;
        if(missing.entry!=UINT32_MAX)return missing;
        return {};
    }
private:
    struct Entry {Key key;bool used{};};std::array<Entry,MaxRows> entries_{};size_t count_{};
};
#endif
struct BoundaryBatch {
    uint64_t probeEpoch{},batch{},thread{},queue{},lists{};uint32_t count{};
    bool operator==(const BoundaryBatch&)const=default;
};
struct BoundaryGraph {
    uint64_t invocation{},consumer{},epoch{},pairGeneration{},frameOwner{},fingerprint{};uint32_t range{};
    bool operator==(const BoundaryGraph&)const=default;
};
struct BoundaryList {
    uint64_t list{},context{},generation{},token{},registry{},work{},name{};
    BoundaryGraph graph;uint32_t slot{},node{},view{};MatchKind match=MatchKind::UnknownList;bool qualified{};
};
struct BoundarySource {
    uint64_t list{},context{},generation{},token{},registry{},fingerprint{},work{},name{};
    uint32_t slot{},node{},view{},status{};
};
// Numeric selection only. The caller must also validate held native tokens
// while the exact observed batch is still pending its original queue call.
#if CVR_RUST_VRIK
inline bool SelectBoundary(const BoundaryBatch& observed,const BoundaryBatch& current,std::span<const BoundaryList> rows,
                           std::span<const uint64_t> lists,const std::array<BoundarySource,2>& sources,Boundary& output,
                           BoundaryFailure* failure=nullptr)noexcept{
    return cvr_sf_select_boundary(&observed,&current,rows.data(),rows.size(),lists.data(),lists.size(),&sources,&output,failure);
}
static_assert(sizeof(BoundaryBatch)==48 && sizeof(BoundaryGraph)==56 && sizeof(BoundaryList)==136 && sizeof(BoundarySource)==80 &&
    sizeof(Boundary)==104,"mirrored by submission_frontier.rs");
#else
inline bool SelectBoundary(const BoundaryBatch& observed,const BoundaryBatch& current,std::span<const BoundaryList> rows,
                           std::span<const uint64_t> lists,const std::array<BoundarySource,2>& sources,Boundary& output,
                           BoundaryFailure* failure=nullptr)noexcept{
    output={};if(failure)*failure=BoundaryFailure::None;
    const auto reject=[&](BoundaryFailure reason){if(failure)*failure=reason;return false;};
    if(!observed.probeEpoch || !observed.batch || !observed.thread || !observed.queue || !observed.lists ||
        observed!=current)return reject(BoundaryFailure::Batch);
    if(observed.count<3 || observed.count>MaxSubmitLists || rows.size()!=observed.count || lists.size()!=rows.size())return reject(BoundaryFailure::Arguments);
    std::array<uint32_t,2> indices{UINT32_MAX,UINT32_MAX};
    for(uint32_t i=0;i<rows.size();++i){const auto& row=rows[i];
        if(!lists[i] || row.list!=lists[i])return reject(BoundaryFailure::Changed);
        if(row.match!=MatchKind::UnknownList && row.match!=MatchKind::Matched)return reject(BoundaryFailure::Replay);
        for(uint32_t j=0;j<i;++j)if(lists[j]==lists[i])return reject(BoundaryFailure::Duplicate);
        for(uint32_t side=0;side<2;++side)if(row.list==sources[side].list)indices[side]=i;
    }
    for(uint32_t side=0;side<2;++side){const auto& source=sources[side];
        if(indices[side]==UINT32_MAX)return reject(BoundaryFailure::MissingSource);
        if(source.status || source.view!=side || !source.list || !source.context || !source.generation || !source.token ||
            !source.registry || !source.fingerprint || !source.work || source.slot>=MaxSlots)return reject(BoundaryFailure::Source);
        const auto& row=rows[indices[side]];
        if(row.match!=MatchKind::Matched || !row.qualified || row.context!=source.context || row.generation!=source.generation ||
            row.token!=source.token || row.registry!=source.registry || row.graph.fingerprint!=source.fingerprint ||
            row.slot!=source.slot || row.node!=source.node || row.view!=source.view || row.work!=source.work || row.name!=source.name)return reject(BoundaryFailure::SourceMatch);
    }
    if(indices[0]==indices[1] || sources[0].slot==sources[1].slot || sources[0].token==sources[1].token ||
        sources[0].context==sources[1].context || sources[0].node==sources[1].node)return reject(BoundaryFailure::Duplicate);
    if(sources[0].registry!=sources[1].registry || sources[0].work!=sources[1].work)return reject(BoundaryFailure::Source);
    const auto& graph=rows[indices[0]].graph;
    if(!graph.invocation || !graph.consumer || !graph.epoch || !graph.pairGeneration || !graph.frameOwner || !graph.fingerprint ||
        rows[indices[1]].graph!=graph)return reject(BoundaryFailure::Graph);
    const auto after=std::max(indices[0],indices[1])+1,highestSlot=std::max(sources[0].slot,sources[1].slot);
    if(after>=rows.size())return reject(BoundaryFailure::NoSuffix);
    for(uint32_t i=after;i<rows.size();++i){const auto& row=rows[i];
        if(row.match!=MatchKind::Matched || !row.qualified || !row.context || !row.generation || !row.token ||
            row.registry!=sources[0].registry || row.graph!=graph || row.slot<=highestSlot || row.slot>=MaxSlots ||
            row.view>1 || row.work!=sources[0].work)return reject(BoundaryFailure::Suffix);
        for(uint32_t j=0;j<i;++j)if(rows[j].qualified && (row.token==rows[j].token || row.slot==rows[j].slot))return reject(BoundaryFailure::Duplicate);
    }
    output={observed.probeEpoch,observed.batch,observed.thread,observed.queue,observed.lists,graph.consumer,graph.epoch,
        graph.pairGeneration,graph.frameOwner,graph.fingerprint,graph.invocation,observed.count,after,indices};return true;
}
#endif
}
// Install only from the host's normal initialized MinHook setup. No automatic
// installation, native scheduling changes, execution permission or GPU work.
bool Install()noexcept;
bool Reset()noexcept;
// Call after native_recording::BeforeSubmit and before its Submitted callback.
// Unknown list rows are emitted for each otherwise-observed native batch.
void ObserveSubmit(ID3D12CommandQueue*,UINT,ID3D12CommandList* const*)noexcept;
// Inspect before executing any prefix list. This does not claim the batch or
// grant GPU resource/state authority; the caller owns exactly-once insertion.
bool FindBoundary(ID3D12CommandQueue*,UINT,ID3D12CommandList* const*,const std::array<Source,2>&,Boundary&)noexcept;
uint32_t LastBoundaryFailure()noexcept;
}
extern "C" {
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_SubmissionFrontierEnable;
// Installed:0 absent,1 installed,2 code rejection,3 MinHook failure.
// State:0 empty,1 ranges,2 submissions,3 rejection,4 capacity reached.
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_SubmissionFrontierInstalled,CyberpunkVR_SubmissionFrontierState;
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_SubmissionFrontierFailure,CyberpunkVR_SubmissionFrontierSeq;
// Boundary failure values are detail::BoundaryFailure; this never grants permission.
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_SubmissionFrontierBoundaryFailure;
// Calls,ranges,rejectedRanges,slots,contexts,tokens,tokenFailures,knownGraphs,
// unknownGraphs,batches,matches,unknownLists,rejectedTokens,replays,overflows,permission(always0).
__declspec(dllexport) extern uint64_t CyberpunkVR_SubmissionFrontierStats[16],CyberpunkVR_SubmissionFrontierDetails[16];
// Details16Q:last base,guard RVA,expected hash,actual hash,caller,range header,
// table,begin,end,mode,graphStatus,work context,definition,thread,invocation,failure.
// Range24Q:probeEpoch,invocation,thread,caller,header,table,begin,end,mode,
// control,fences,jobContext,firstSlot,count,returnRax,returned,failure,graphStatus,
// consumer,graphEpoch,pairGeneration,frameOwner,syncHandle,graphFingerprint.
// graphStatus:0 qualified,1 no owned epoch,2 unavailable snapshot,3 unwind,
// 4 native context,5 frame owner,6 synchronizer identity,7 topology,8 changed.
// Consumer/epoch fields are candidates only when graphStatus is nonzero.
// Slot20Q:probeEpoch,invocation,rangeRow,tableIndex,producerSlot,contextId,
// context,list,state,kind,submitted,generation,token,captured,captureFailure,
// nodeHandle,viewIndex,workRva,name,residencySet.
// captureFailure: native_recording failure,100+range Failure,200+graphStatus,
// or209 for a populated slot without an authenticated context-publisher role.
// Submit16Q:probeEpoch,batch,thread,queue,count,index,list,MatchKind,invocation,
// slotRow,generation,nativeFailure,context,producerSlot,nodeHandle,viewIndex.
__declspec(dllexport) extern uint64_t CyberpunkVR_SubmissionFrontierRanges,CyberpunkVR_SubmissionFrontierSlots,CyberpunkVR_SubmissionFrontierSubmits;
__declspec(dllexport) extern uint32_t CyberpunkVR_SubmissionFrontierRangeCount,CyberpunkVR_SubmissionFrontierSlotCount,CyberpunkVR_SubmissionFrontierSubmitCount;
__declspec(dllexport) extern uint32_t CyberpunkVR_SubmissionFrontierRangeBytes,CyberpunkVR_SubmissionFrontierSlotBytes,CyberpunkVR_SubmissionFrontierSubmitBytes;
}
