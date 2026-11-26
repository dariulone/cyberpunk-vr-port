#pragma once
#include <array>
#include <cstddef>
#include <cstdint>
#include <set>
#include <vector>

namespace cvr::stereo::definition_owner {
using ReadMemory=bool(*)(uint64_t,void*,size_t);
inline constexpr uint32_t MaxDefinitions=8192,MaxConsumers=8192;
struct NativeVector {
    uint64_t data{};
    uint32_t capacity{},count{};
    bool operator==(const NativeVector&)const=default;
};
static_assert(sizeof(NativeVector)==16 && offsetof(NativeVector,capacity)==8 && offsetof(NativeVector,count)==12);
enum class Failure:uint32_t {None,Arguments,Header,Bounds,Entries,Allocator,Changed,Allocation};
#if CVR_RUST_VRIK
// The rules: rust/crates/cvr-render/src/native/definition_owner.rs through definition_owner_abi.rs
// (tools/single_pass_tests/definition_owner_test and definition_source_test run their cases against them in their
// Rust variants).
struct RustMovePlan;struct RustRetireState;
struct RustPlanFields {uint64_t address,trailer;NativeVector saved,empty;const uint64_t* definitions;size_t count;uint32_t failure;bool ready;};
struct RustRetireView {uint64_t epoch;size_t consumers,pending;uint32_t failure;bool retiring,retired;};
static_assert(sizeof(RustPlanFields)==72 && sizeof(RustRetireView)==32,"mirrored by definition_owner_abi.rs");
extern "C" {
RustMovePlan* cvr_do_plan_capture(uint64_t,ReadMemory,RustPlanFields*);
void cvr_do_plan_free(RustMovePlan*);
bool cvr_do_plan_check(const RustPlanFields*,ReadMemory,bool);
RustRetireState* cvr_do_retire_new(uint64_t,bool);
void cvr_do_retire_free(RustRetireState*);
bool cvr_do_retire_step(RustRetireState*,uint32_t,uint64_t,uint64_t);
void cvr_do_retire_state(const RustRetireState*,RustRetireView*);
}
#endif

// A move proposal only. The caller proves the native graph/template type and
// serializes mutation before writing EmptyHeader to the actual native owner.
// SavedHeader contains native-allocated memory; the header itself is not owned
// by the native allocator and must never be passed as the allocation to free.
class MovePlan {
public:
#if CVR_RUST_VRIK
    bool Capture(uint64_t graph,ReadMemory read)try{
        *this=MovePlan{};RustPlanFields out{};
        const auto plan=cvr_do_plan_capture(graph,read,&out);
        // The owner address stays after a refusal past the arguments, as by the C++ rules.
        failure_=Failure(out.failure);address_=out.address;if(!plan)return false;
        struct Release{RustMovePlan* plan;~Release(){cvr_do_plan_free(plan);}}release{plan};
        saved_=out.saved;empty_=out.empty;trailer_=out.trailer;definitions_.assign(out.definitions,out.definitions+out.count);
        ready_=true;return true;
    }catch(...){*this=MovePlan{};failure_=Failure::Allocation;return false;}
    bool MatchesBefore(ReadMemory read)const{
        if(!ready_)return false;const auto fields=Fields();return cvr_do_plan_check(&fields,read,false);
    }
    // A plan Rust captured (graph_definition.rs for graph_definition::SourceReceipt), restored from its fields.
    static MovePlan Restored(const RustPlanFields& fields){
        MovePlan plan;plan.address_=fields.address;plan.trailer_=fields.trailer;plan.saved_=fields.saved;plan.empty_=fields.empty;
        plan.definitions_.assign(fields.definitions,fields.definitions+fields.count);plan.failure_=Failure(fields.failure);
        plan.ready_=fields.ready;return plan;
    }
    bool VerifyDetached(ReadMemory read)const{
        if(!ready_ || !read)return false;const auto fields=Fields();return cvr_do_plan_check(&fields,read,true);
    }
#else
    bool Capture(uint64_t graph,ReadMemory read)try{
        *this=MovePlan{};failure_=Failure::Arguments;
        if(!Pointer(graph) || graph>UINT64_MAX-0x88 || !read)return false;
        address_=graph+0x70;
        Snapshot first,second;
        if(!ReadOnce(read,address_,first) || !ReadOnce(read,address_,second))return false;
        failure_=Failure::Changed;if(first!=second)return false;
        saved_=first.header;empty_={first.allocator,0,0};trailer_=first.trailer;definitions_=std::move(first.entries);
        ready_=true;failure_=Failure::None;return true;
    }catch(...){*this=MovePlan{};failure_=Failure::Allocation;return false;}
    bool MatchesBefore(ReadMemory read)const{
        if(!ready_)return false;MovePlan current;
        return current.Capture(address_-0x70,read) && current.address_==address_ && current.saved_==saved_ &&
            current.empty_==empty_ && current.trailer_==trailer_ && current.definitions_==definitions_;
    }
    bool VerifyDetached(ReadMemory read)const{
        if(!ready_ || !read)return false;NativeVector a,b;
        return read(address_,&a,sizeof(a)) && read(address_,&b,sizeof(b)) && a==empty_ && b==empty_;
    }
#endif
    bool Ready()const{return ready_;}
    bool OwnsAllocation()const{return ready_ && saved_.capacity!=0;}
    Failure Rejection()const{return failure_;}
    uint64_t OwnerAddress()const{return address_;}
    uint64_t TrailerAddress()const{return trailer_;}
    const NativeVector& SavedHeader()const{return saved_;}
    const NativeVector& EmptyHeader()const{return empty_;}
    const std::vector<uint64_t>& Definitions()const{return definitions_;}
private:
#if CVR_RUST_VRIK
    RustPlanFields Fields()const{
        return {address_,trailer_,saved_,empty_,definitions_.data(),definitions_.size(),uint32_t(failure_),ready_};
    }
#else
    struct Snapshot {
        NativeVector header;uint64_t allocator{},trailer{};std::vector<uint64_t> entries;
        bool operator==(const Snapshot&)const=default;
    };
    static bool Pointer(uint64_t value){return value && !(value&7);}
    bool ReadOnce(ReadMemory read,uint64_t address,Snapshot& out){
        out={};failure_=Failure::Header;if(!read(address,&out.header,sizeof(out.header)))return false;
        const auto& h=out.header;failure_=Failure::Bounds;
        if(!Pointer(h.data) || h.capacity>MaxDefinitions || h.count>h.capacity)return false;
        if(!h.capacity){if(h.count || (h.data>=address-0x70 && h.data<address+16))return false;out.allocator=h.data;return true;}
        const uint64_t bytes=uint64_t(h.capacity)*sizeof(uint64_t),aligned=(bytes+7)&~uint64_t(7);
        if(aligned>UINT32_MAX || h.data>UINT64_MAX-aligned || h.data+aligned>UINT64_MAX-8)return false;
        if(h.data<address+16 && address-0x70<h.data+aligned+8)return false;
        out.trailer=h.data+aligned;out.entries.resize(h.count);
        failure_=Failure::Entries;
        if(h.count && !read(h.data,out.entries.data(),size_t(h.count)*sizeof(uint64_t)))return false;
        std::set<uint64_t> unique;
        // Synthetic graph nodes retain a null slot in the native owner vector.
        for(const auto entry:out.entries)if(entry && (!Pointer(entry) || !unique.insert(entry).second ||
            (entry>=h.data && entry<out.trailer+8) || (entry>=address-0x70 && entry<address+16)))return false;
        failure_=Failure::Allocator;
        if(!read(out.trailer,&out.allocator,8) || !Pointer(out.allocator) ||
            (out.allocator>=h.data && out.allocator<out.trailer+8))return false;
        return true;
    }
#endif
    NativeVector saved_,empty_;std::vector<uint64_t> definitions_;
    uint64_t address_{},trailer_{};bool ready_{};Failure failure_=Failure::None;
};

enum class RetireFailure:uint32_t {None,Epoch,UnaccountedPriorUse,Consumer,Capacity,UnclaimedMutation,Detach,SourceOpen,Incomplete,Repeated};
// The caller supplies actual native job-lease completions and holds its own
// short synchronization gate around every method. No method waits or frees.
// Source closure must exclude future users of this definition generation,
// including native cache hits; it is not equivalent to releasing a CVR clone.
#if CVR_RUST_VRIK
class RetireState {
public:
    explicit RetireState(uint64_t epoch,bool allPriorConsumersAccounted=false):
        rust_(cvr_do_retire_new(epoch,allPriorConsumersAccounted)){}
    ~RetireState(){cvr_do_retire_free(rust_);}
    RetireState(const RetireState&)=delete;
    RetireState& operator=(const RetireState&)=delete;
    bool AddConsumer(uint64_t epoch,uint64_t consumer){return cvr_do_retire_step(rust_,0,epoch,consumer);}
    bool CompleteConsumer(uint64_t epoch,uint64_t consumer){return cvr_do_retire_step(rust_,1,epoch,consumer);}
    bool VerifyDetach(uint64_t epoch,bool originalOwnerDetached){return cvr_do_retire_step(rust_,2,epoch,originalOwnerDetached);}
    bool CloseSourceUses(uint64_t epoch){return cvr_do_retire_step(rust_,3,epoch,0);}
    void UnclaimedNativeMutation(){cvr_do_retire_step(rust_,4,0,0);}
    bool ReadyToRetire()const{return cvr_do_retire_step(rust_,7,0,0);}
    bool BeginRetire(uint64_t epoch){return cvr_do_retire_step(rust_,5,epoch,0);}
    bool FinishRetire(uint64_t epoch){return cvr_do_retire_step(rust_,6,epoch,0);}
    // If the native destructor throws or completion is unknown, do not retry:
    // BeginRetire has consumed the single destruction attempt.
    bool Retiring()const{return View().retiring;}
    bool Retired()const{return View().retired;}
    uint64_t Epoch()const{return View().epoch;}
    size_t Consumers()const{return View().consumers;}
    size_t PendingConsumers()const{return View().pending;}
    RetireFailure FailureReason()const{return RetireFailure(View().failure);}
private:
    RustRetireView View()const{RustRetireView view{};cvr_do_retire_state(rust_,&view);return view;}
    RustRetireState* rust_;
};
#else
class RetireState {
public:
    explicit RetireState(uint64_t epoch,bool allPriorConsumersAccounted=false):epoch_(epoch){
        if(!epoch)Reject(RetireFailure::Epoch);
        else if(!allPriorConsumersAccounted)Reject(RetireFailure::UnaccountedPriorUse);
    }
    bool AddConsumer(uint64_t epoch,uint64_t consumer)try{
        if(!CheckEpoch(epoch))return false;
        if(!consumer || sourceClosed_ || retiring_ || retired_ || consumers_.contains(consumer))return Reject(RetireFailure::Consumer);
        if(consumers_.size()>=MaxConsumers)return Reject(RetireFailure::Capacity);
        consumers_.emplace(consumer);return true;
    }catch(...){return Reject(RetireFailure::Capacity);}
    bool CompleteConsumer(uint64_t epoch,uint64_t consumer){
        if(!CheckEpoch(epoch))return false;
        if(!consumer || retiring_ || retired_ || !consumers_.contains(consumer) || completed_.contains(consumer))return Reject(RetireFailure::Consumer);
        try{completed_.emplace(consumer);return true;}catch(...){return Reject(RetireFailure::Capacity);}
    }
    bool VerifyDetach(uint64_t epoch,bool originalOwnerDetached){
        if(!CheckEpoch(epoch))return false;
        if(detached_ || retiring_ || retired_ || !originalOwnerDetached)return Reject(RetireFailure::Detach);
        detached_=true;return true;
    }
    bool CloseSourceUses(uint64_t epoch){
        if(!CheckEpoch(epoch))return false;
        if(!detached_ || sourceClosed_ || retiring_ || retired_)return Reject(RetireFailure::SourceOpen);
        sourceClosed_=true;return true;
    }
    void UnclaimedNativeMutation(){Reject(RetireFailure::UnclaimedMutation);}
    bool ReadyToRetire()const{return failure_==RetireFailure::None && detached_ && sourceClosed_ && !retiring_ && !retired_ && consumers_.size()==completed_.size();}
    bool BeginRetire(uint64_t epoch){
        if(!CheckEpoch(epoch) || !ReadyToRetire())return false;
        retiring_=true;return true;
    }
    bool FinishRetire(uint64_t epoch){
        if(!CheckEpoch(epoch))return false;
        if(!retiring_ || retired_)return Reject(RetireFailure::Repeated);
        retired_=true;retiring_=false;return true;
    }
    // If the native destructor throws or completion is unknown, do not retry:
    // BeginRetire has consumed the single destruction attempt.
    bool Retiring()const{return retiring_;}
    bool Retired()const{return retired_;}
    uint64_t Epoch()const{return epoch_;}
    size_t Consumers()const{return consumers_.size();}
    size_t PendingConsumers()const{return consumers_.size()-completed_.size();}
    RetireFailure FailureReason()const{return failure_;}
private:
    bool Reject(RetireFailure failure){if(failure_==RetireFailure::None)failure_=failure;return false;}
    bool CheckEpoch(uint64_t epoch){if(failure_!=RetireFailure::None)return false;return epoch==epoch_ || Reject(RetireFailure::Epoch);}
    uint64_t epoch_{};std::set<uint64_t> consumers_,completed_;
    bool detached_{},sourceClosed_{},retiring_{},retired_{};
    RetireFailure failure_=RetireFailure::None;
};
#endif
}
