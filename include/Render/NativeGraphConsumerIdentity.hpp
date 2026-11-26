#pragma once
#include "Render/NativeGraphJobReceipt.hpp"
#include <limits>

namespace cvr::stereo::graph_jobs {
inline constexpr uint32_t ConstructReturn=0x40A4AB;
enum class IdentityFailure:uint32_t {
    None,Arguments,Caller,AlreadyCaptured,MissingApi,Read,References,Changed,
    NotCaptured,ConstructAlreadyChecked,ConstructIdentity,SourceNotConsumed
};
#if CVR_RUST_VRIK
// The ticket: rust/crates/cvr-render/src/native/consumer_identity.rs through graph_jobs_abi.rs
// (tools/single_pass_tests/graph_consumer_identity_test runs its cases against it in its Rust variant).
struct RustRootTicket;
struct RustTicketState {
    uint64_t id,epoch,generation,imageBase,frameOwner,sourceGraph,graph,group;
    uint32_t failure;bool captured,constructVerified,redirected;
};
static_assert(sizeof(RustTicketState)==72,"mirrored by graph_jobs_abi.rs");
extern "C" {
RustRootTicket* cvr_ci_new();
void cvr_ci_free(RustRootTicket*);
bool cvr_ci_capture(RustRootTicket*,uint64_t,uint64_t,uint64_t,uint64_t,uint64_t,uint64_t,uint64_t,uint64_t,ReadMemory,
    const JobLeaseApi*,const FrameOwnerApi*,uint64_t);
bool cvr_ci_verify(RustRootTicket*,uint64_t,ReadMemory);
bool cvr_ci_match(const RustRootTicket*,uint64_t,uint64_t,uint64_t,ReadMemory,bool);
void cvr_ci_fail_read(RustRootTicket*);
void cvr_ci_state(const RustRootTicket*,RustTicketState*);
}
#endif

// Native callbacks are nonthrowing; the caller serializes this object. This pins
// identity only: neither a zero pending count nor cancellation proves completion.
#if CVR_RUST_VRIK
class NativeRootTicket {
public:
    NativeRootTicket():rust_(cvr_ci_new()){}
    ~NativeRootTicket(){cvr_ci_free(rust_);}
    NativeRootTicket(const NativeRootTicket&)=delete;
    NativeRootTicket& operator=(const NativeRootTicket&)=delete;
    NativeRootTicket(NativeRootTicket&&)=delete;
    NativeRootTicket& operator=(NativeRootTicket&&)=delete;

    // An execution override records the caller's future publication contract.
    // This ticket neither publishes the slot nor proves storage/definition leases.
    // A throwing reader unwinds through the rules (their pins release) and is refused with Read here.
    bool CaptureBeforeConstruct(uint64_t ownerAddress,uint64_t graphPointerAddress,uint64_t completionHandleAddress,
                                uint64_t caller,uint64_t imageBase,uint64_t epoch,uint64_t generation,uint64_t monotonicId,
                                ReadMemory read,JobLeaseApi jobs,FrameOwnerApi owner,uint64_t expectedExecutionGraph=0)try{
        return cvr_ci_capture(rust_,ownerAddress,graphPointerAddress,completionHandleAddress,caller,imageBase,epoch,generation,
            monotonicId,read,&jobs,&owner,expectedExecutionGraph);
    }catch(...){cvr_ci_fail_read(rust_);return false;}

    // Immediate post-B56968 only: the original root's source handle is still live.
    // No owner/graph stack addresses are retained or read by either later method.
    bool VerifyConstruct(uint64_t destination,ReadMemory read)try{
        return cvr_ci_verify(rust_,destination,read);
    }catch(...){cvr_ci_fail_read(rust_);return false;}

    bool MatchQueued(uint64_t payload,uint64_t graph,uint64_t caller,ReadMemory read)const try{
        return cvr_ci_match(rust_,payload,graph,caller,read,false);
    }catch(...){return false;}
    bool MatchExecutionCallback(uint64_t payload,uint64_t graph,uint64_t caller,ReadMemory read)const try{
        return cvr_ci_match(rust_,payload,graph,caller,read,true);
    }catch(...){return false;}
    bool Captured()const{return State().captured;}
    bool ConstructVerified()const{return State().constructVerified;}
    IdentityFailure Rejection()const{return IdentityFailure(State().failure);}
    uint64_t Id()const{return State().id;}
    uint64_t Epoch()const{return State().epoch;}
    uint64_t Generation()const{return State().generation;}
    uint64_t ImageBase()const{return State().imageBase;}
    uint64_t FrameOwner()const{return State().frameOwner;}
    uint64_t SourceGraphAddress()const{return State().sourceGraph;}
    uint64_t GraphAddress()const{return State().graph;}
    bool Redirected()const{return State().redirected;}
    uint64_t Group()const{return State().group;}
private:
    RustTicketState State()const{RustTicketState state{};cvr_ci_state(rust_,&state);return state;}
    RustRootTicket* rust_;
};
#else
class NativeRootTicket {
public:
    NativeRootTicket()=default;
    ~NativeRootTicket(){Release();}
    NativeRootTicket(const NativeRootTicket&)=delete;
    NativeRootTicket& operator=(const NativeRootTicket&)=delete;
    NativeRootTicket(NativeRootTicket&&)=delete;
    NativeRootTicket& operator=(NativeRootTicket&&)=delete;

    // An execution override records the caller's future publication contract.
    // This ticket neither publishes the slot nor proves storage/definition leases.
    bool CaptureBeforeConstruct(uint64_t ownerAddress,uint64_t graphPointerAddress,uint64_t completionHandleAddress,
                                uint64_t caller,uint64_t imageBase,uint64_t epoch,uint64_t generation,uint64_t monotonicId,
                                ReadMemory read,JobLeaseApi jobs,FrameOwnerApi owner,uint64_t expectedExecutionGraph=0)try{
        if(captured_){failure_=IdentityFailure::AlreadyCaptured;return false;}
        failure_=IdentityFailure::Arguments;
        if(!read || !monotonicId || !imageBase || imageBase>UINT64_MAX-ConstructReturn ||
            !Pointer(ownerAddress,8) || !Pointer(graphPointerAddress,8) || !Pointer(completionHandleAddress,8))return false;
        failure_=IdentityFailure::Caller;if(caller!=imageBase+ConstructReturn)return false;
        failure_=IdentityFailure::MissingApi;
        // complete is deliberately unused: a prerequisite group is not the graph's terminal receipt.
        if(!jobs.retain || !jobs.release || !owner.retain || !owner.release)return false;
        Identity first,second;failure_=IdentityFailure::Read;
        if(!ReadSource(read,ownerAddress,graphPointerAddress,completionHandleAddress,first) ||
            !ReadSource(read,ownerAddress,graphPointerAddress,completionHandleAddress,second))return false;
        failure_=IdentityFailure::Changed;if(first!=second)return false;
        failure_=IdentityFailure::Arguments;
        if(expectedExecutionGraph && (!Pointer(expectedExecutionGraph,0x70) || expectedExecutionGraph==first.owner || expectedExecutionGraph==first.group))return false;
        failure_=IdentityFailure::References;
        if(!References(read,first.owner+8) || !References(read,first.group+0x1c))return false;
        struct Pins {
            JobLeaseApi jobs;FrameOwnerApi owner;void* frame{};void* group{};
            ~Pins(){if(group)jobs.release(&group);if(frame)owner.release(frame);}
        } pins{jobs,owner};
        pins.frame=reinterpret_cast<void*>(uintptr_t(first.owner));owner.retain(pins.frame);
        pins.group=reinterpret_cast<void*>(uintptr_t(first.group));jobs.retain(pins.group);
        failure_=IdentityFailure::Read;
        if(!ReadSource(read,ownerAddress,graphPointerAddress,completionHandleAddress,second))return false;
        Identity third;if(!ReadSource(read,ownerAddress,graphPointerAddress,completionHandleAddress,third))return false;
        failure_=IdentityFailure::Changed;if(first!=second || first!=third)return false;
        failure_=IdentityFailure::References;
        if(!References(read,first.owner+8,false) || !References(read,first.group+0x1c,false))return false;
        sourceGraph_=first.graph;identity_=first;
        if(expectedExecutionGraph)identity_.graph=expectedExecutionGraph;
        imageBase_=imageBase;epoch_=epoch;generation_=generation;id_=monotonicId;
        completionAddress_=completionHandleAddress;jobsApi_=jobs;ownerApi_=owner;
        framePin_=pins.frame;groupPin_=pins.group;pins.frame=pins.group=nullptr;
        captured_=true;failure_=IdentityFailure::None;return true;
    }catch(...){failure_=IdentityFailure::Read;return false;}

    // Immediate post-B56968 only: the original root's source handle is still live.
    // No owner/graph stack addresses are retained or read by either later method.
    bool VerifyConstruct(uint64_t destination,ReadMemory read)try{
        failure_=IdentityFailure::NotCaptured;if(!captured_)return false;
        failure_=IdentityFailure::ConstructAlreadyChecked;if(constructChecked_)return false;
        constructChecked_=true;Identity first,second;uint64_t consumed{},again{};
        failure_=IdentityFailure::Read;
        if(!read || !ReadPayload(read,destination,first) || !read(completionAddress_,&consumed,8) ||
            !ReadPayload(read,destination,second) || !read(completionAddress_,&again,8))return false;
        failure_=IdentityFailure::ConstructIdentity;if(first!=identity_ || second!=identity_)return false;
        failure_=IdentityFailure::SourceNotConsumed;if(consumed || again)return false;
        completionAddress_=0;constructed_=true;failure_=IdentityFailure::None;return true;
    }catch(...){failure_=IdentityFailure::Read;return false;}

    bool MatchQueued(uint64_t payload,uint64_t graph,uint64_t caller,ReadMemory read)const try{
        if(!captured_ || !constructed_ || !read || graph!=identity_.graph ||
            imageBase_>UINT64_MAX-QueueReturn || caller!=imageBase_+QueueReturn)return false;
        Identity first,second;
        return ReadPayload(read,payload,first) && ReadPayload(read,payload,second) && first==identity_ && second==identity_;
    }catch(...){return false;}
    bool MatchExecutionCallback(uint64_t payload,uint64_t graph,uint64_t caller,ReadMemory read)const try{
        if(!captured_ || !constructed_ || !read || graph!=identity_.graph || imageBase_>UINT64_MAX-0x26E110 ||
            (caller!=imageBase_+0x26E110 && caller!=imageBase_+0x2735E3))return false;
        Identity first,second;
        return ReadPayload(read,payload,first) && ReadPayload(read,payload,second) && first==identity_ && second==identity_;
    }catch(...){return false;}
    bool Captured()const{return captured_;}
    bool ConstructVerified()const{return constructed_;}
    IdentityFailure Rejection()const{return failure_;}
    uint64_t Id()const{return id_;}
    uint64_t Epoch()const{return epoch_;}
    uint64_t Generation()const{return generation_;}
    uint64_t ImageBase()const{return imageBase_;}
    uint64_t FrameOwner()const{return identity_.owner;}
    uint64_t SourceGraphAddress()const{return sourceGraph_;}
    uint64_t GraphAddress()const{return identity_.graph;}
    bool Redirected()const{return captured_ && sourceGraph_!=identity_.graph;}
    uint64_t Group()const{return identity_.group;}
private:
    struct Identity {
        uint64_t owner{},graph{},group{};
        bool operator==(const Identity&)const=default;
    };
    static bool Pointer(uint64_t value,uint64_t bytes){
        return value && !(value&7) && value<=UINT64_MAX-bytes && value<=std::numeric_limits<uintptr_t>::max();
    }
    static bool Valid(const Identity& value){
        return Pointer(value.owner,12) && Pointer(value.graph,0x70) && Pointer(value.group,0x20) && value.owner!=value.group;
    }
    static bool ReadSource(ReadMemory read,uint64_t owner,uint64_t graph,uint64_t group,Identity& value){
        return read(owner,&value.owner,8) && read(graph,&value.graph,8) && read(group,&value.group,8) && Valid(value);
    }
    static bool ReadPayload(ReadMemory read,uint64_t address,Identity& value){
        return Pointer(address,0x20) && read(address,&value.owner,8) && read(address+8,&value.graph,8) &&
            read(address+0x18,&value.group,8) && Valid(value);
    }
    static bool References(ReadMemory read,uint64_t address,bool beforeIncrement=true){
        int32_t count{};return read(address,&count,4) && count>0 &&
            (!beforeIncrement || count<std::numeric_limits<int32_t>::max());
    }
    void Release(){
        if(groupPin_)jobsApi_.release(&groupPin_);
        if(framePin_){auto* value=framePin_;framePin_=nullptr;ownerApi_.release(value);}
    }
    Identity identity_;
    JobLeaseApi jobsApi_{};FrameOwnerApi ownerApi_{};
    void* framePin_{};void* groupPin_{};
    uint64_t imageBase_{},epoch_{},generation_{},id_{},completionAddress_{},sourceGraph_{};
    bool captured_{},constructChecked_{},constructed_{};
    IdentityFailure failure_=IdentityFailure::None;
};
#endif
}
