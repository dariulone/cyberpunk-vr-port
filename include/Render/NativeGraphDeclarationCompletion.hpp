#pragma once
#include "Render/NativeGraphJobs.hpp"
#include <limits>

namespace cvr::stereo::graph_jobs {
inline constexpr uint32_t DeclarationConstructReturn=0x40A4AB,DeclarationReturn=0x40A756;
enum class DeclarationCompletionFailure:uint32_t {
    None,Arguments,Caller,AlreadyAttempted,MissingApi,Read,FrameOwner,Group,References,Pending,Changed,
    NotCaptured,ProducerClosed,AlreadyMatched,DeclarationIdentity,NotMatched,AlreadyReturned
};
#if CVR_RUST_VRIK
// The lease: rust/crates/cvr-render/src/native/declaration_completion.rs through graph_jobs_abi.rs
// (tools/single_pass_tests/graph_declaration_completion_test runs its cases against it in its Rust variant).
struct RustDeclarationLease;
struct RustDeclarationState {
    uint64_t contextAddress,group,frameOwner,executionGraph,imageBase,epoch,generation,id;
    uint32_t failure;bool captured,matched,declareReturned,rootReturned,failed;
};
static_assert(sizeof(RustDeclarationState)==80,"mirrored by graph_jobs_abi.rs");
extern "C" {
RustDeclarationLease* cvr_dc_new();
void cvr_dc_free(RustDeclarationLease*);
bool cvr_dc_capture(RustDeclarationLease*,uint64_t,uint64_t,uint64_t,uint64_t,uint64_t,uint64_t,uint64_t,ReadMemory,const JobLeaseApi*);
bool cvr_dc_match(RustDeclarationLease*,uint64_t,uint64_t,uint64_t,ReadMemory);
bool cvr_dc_step(RustDeclarationLease*,uint32_t);
void cvr_dc_state(const RustDeclarationLease*,RustDeclarationState*);
}
#endif

// Caller serializes access and proves native body/dispatch coverage separately.
// This pins only the D0 group; it neither owns definitions nor waits or authorizes
// storage destruction. Native retain/release/complete callbacks must not throw.
#if CVR_RUST_VRIK
class DeclarationCompletionLease {
public:
    DeclarationCompletionLease():rust_(cvr_dc_new()){}
    ~DeclarationCompletionLease(){cvr_dc_free(rust_);}
    DeclarationCompletionLease(const DeclarationCompletionLease&)=delete;
    DeclarationCompletionLease& operator=(const DeclarationCompletionLease&)=delete;
    DeclarationCompletionLease(DeclarationCompletionLease&&)=delete;
    DeclarationCompletionLease& operator=(DeclarationCompletionLease&&)=delete;

    // A throwing reader unwinds through the rules (their pin releases the group) and is refused with Read here.
    bool CaptureBeforeConstruct(uint64_t ownerAddress,uint64_t executionGraph,uint64_t caller,uint64_t imageBase,
                                uint64_t epoch,uint64_t generation,uint64_t consumerId,ReadMemory read,JobLeaseApi api)try{
        return cvr_dc_capture(rust_,ownerAddress,executionGraph,caller,imageBase,epoch,generation,consumerId,read,&api);
    }catch(...){return cvr_dc_step(rust_,3);}

    // Immediate before originalA86438 only, while the original root still owns
    // its context. Native141B10 may rotate context+18 immediately after dispatch.
    bool MatchBeforeDeclare(uint64_t job,uint64_t graph,uint64_t caller,ReadMemory read)try{
        return cvr_dc_match(rust_,job,graph,caller,read);
    }catch(...){return cvr_dc_step(rust_,3);}

    bool MarkOriginalDeclareReturned()noexcept{return cvr_dc_step(rust_,0);}
    // Clear every context/stack address even when an earlier proof failed.
    // This marks normal original219730 return, not the outer AA scheduler return.
    bool MarkOriginalRootReturned()noexcept{return cvr_dc_step(rust_,1);}
    bool Complete()const noexcept{return cvr_dc_step(rust_,2);}
    bool Captured()const noexcept{return State().captured;}
    bool Matched()const noexcept{return State().matched;}
    bool OriginalDeclareReturned()const noexcept{return State().declareReturned;}
    bool OriginalRootReturned()const noexcept{return State().rootReturned;}
    bool Failed()const noexcept{return State().failed;}
    uint64_t ContextAddress()const noexcept{return State().contextAddress;}
    uint64_t Group()const noexcept{return State().group;}
    uint64_t FrameOwner()const noexcept{return State().frameOwner;}
    uint64_t ExecutionGraph()const noexcept{return State().executionGraph;}
    uint64_t ImageBase()const noexcept{return State().imageBase;}
    uint64_t Epoch()const noexcept{return State().epoch;}
    uint64_t Generation()const noexcept{return State().generation;}
    uint64_t Id()const noexcept{return State().id;}
    DeclarationCompletionFailure Rejection()const noexcept{return DeclarationCompletionFailure(State().failure);}
private:
    RustDeclarationState State()const noexcept{RustDeclarationState state{};cvr_dc_state(rust_,&state);return state;}
    RustDeclarationLease* rust_;
};
#else
class DeclarationCompletionLease {
public:
    DeclarationCompletionLease()=default;
    ~DeclarationCompletionLease(){if(groupPin_)api_.release(&groupPin_);}
    DeclarationCompletionLease(const DeclarationCompletionLease&)=delete;
    DeclarationCompletionLease& operator=(const DeclarationCompletionLease&)=delete;
    DeclarationCompletionLease(DeclarationCompletionLease&&)=delete;
    DeclarationCompletionLease& operator=(DeclarationCompletionLease&&)=delete;

    bool CaptureBeforeConstruct(uint64_t ownerAddress,uint64_t executionGraph,uint64_t caller,uint64_t imageBase,
                                uint64_t epoch,uint64_t generation,uint64_t consumerId,ReadMemory read,JobLeaseApi api)try{
        if(failed_)return false;
        if(attempted_)return Reject(DeclarationCompletionFailure::AlreadyAttempted);
        attempted_=true;
        if(!read || !imageBase || imageBase>UINT64_MAX-DeclarationReturn || !epoch || !generation || !consumerId ||
            ownerAddress<0x38 || !Pointer(ownerAddress-0x38,0x40) || !Pointer(executionGraph,0x70))
            return Reject(DeclarationCompletionFailure::Arguments);
        if(caller!=imageBase+DeclarationConstructReturn)return Reject(DeclarationCompletionFailure::Caller);
        if(!api.retain || !api.release || !api.complete)return Reject(DeclarationCompletionFailure::MissingApi);
        const auto context=ownerAddress-0x38;Identity first,second;
        if(!ReadIdentity(context,read,true,first) || !ReadIdentity(context,read,true,second))return Reject(failure_);
        if(first!=second)return Reject(DeclarationCompletionFailure::Changed);
        struct Pin {
            JobLeaseApi api;void* value{};
            ~Pin(){if(value)api.release(&value);}
        } pin{api,reinterpret_cast<void*>(uintptr_t(first.group))};
        api.retain(pin.value);
        Identity third;
        if(!ReadIdentity(context,read,false,second) || !ReadIdentity(context,read,false,third))return Reject(failure_);
        if(first!=second || first!=third)return Reject(DeclarationCompletionFailure::Changed);
        identity_=first;contextAddress_=context;executionGraph_=executionGraph;imageBase_=imageBase;
        epoch_=epoch;generation_=generation;consumerId_=consumerId;api_=api;groupPin_=pin.value;pin.value=nullptr;
        captured_=true;failure_=DeclarationCompletionFailure::None;return true;
    }catch(...){return Reject(DeclarationCompletionFailure::Read);}

    // Immediate before originalA86438 only, while the original root still owns
    // its context. Native141B10 may rotate context+18 immediately after dispatch.
    bool MatchBeforeDeclare(uint64_t job,uint64_t graph,uint64_t caller,ReadMemory read)try{
        if(failed_)return false;
        if(!captured_)return Reject(DeclarationCompletionFailure::NotCaptured);
        if(rootReturned_ || !contextAddress_)return Reject(DeclarationCompletionFailure::ProducerClosed);
        if(matched_)return Reject(DeclarationCompletionFailure::AlreadyMatched);
        if(!read || imageBase_>UINT64_MAX-DeclarationReturn)return Reject(DeclarationCompletionFailure::Arguments);
        if(caller!=imageBase_+DeclarationReturn)return Reject(DeclarationCompletionFailure::Caller);
        if(job!=contextAddress_ || graph!=executionGraph_)return Reject(DeclarationCompletionFailure::DeclarationIdentity);
        Identity first,second;
        if(!ReadIdentity(job,read,false,first) || !ReadIdentity(job,read,false,second))return Reject(failure_);
        if(first!=identity_ || second!=identity_)return Reject(DeclarationCompletionFailure::Changed);
        matched_=true;failure_=DeclarationCompletionFailure::None;return true;
    }catch(...){return Reject(DeclarationCompletionFailure::Read);}

    bool MarkOriginalDeclareReturned()noexcept{
        if(failed_)return false;
        if(!captured_)return Reject(DeclarationCompletionFailure::NotCaptured);
        if(!matched_)return Reject(DeclarationCompletionFailure::NotMatched);
        if(declareReturned_)return Reject(DeclarationCompletionFailure::AlreadyReturned);
        declareReturned_=true;failure_=DeclarationCompletionFailure::None;return true;
    }
    // Clear every context/stack address even when an earlier proof failed.
    // This marks normal original219730 return, not the outer AA scheduler return.
    bool MarkOriginalRootReturned()noexcept{
        contextAddress_=0;
        if(!captured_)return Reject(DeclarationCompletionFailure::NotCaptured);
        if(rootReturned_)return Reject(DeclarationCompletionFailure::AlreadyReturned);
        rootReturned_=true;return !failed_;
    }
    bool Complete()const noexcept{
        return captured_ && !failed_ && matched_ && declareReturned_ && rootReturned_ && groupPin_ && api_.complete(groupPin_);
    }
    bool Captured()const noexcept{return captured_;}
    bool Matched()const noexcept{return matched_;}
    bool OriginalDeclareReturned()const noexcept{return declareReturned_;}
    bool OriginalRootReturned()const noexcept{return rootReturned_;}
    bool Failed()const noexcept{return failed_;}
    uint64_t ContextAddress()const noexcept{return contextAddress_;}
    uint64_t Group()const noexcept{return reinterpret_cast<uintptr_t>(groupPin_);}
    uint64_t FrameOwner()const noexcept{return identity_.frameOwner;}
    uint64_t ExecutionGraph()const noexcept{return executionGraph_;}
    uint64_t ImageBase()const noexcept{return imageBase_;}
    uint64_t Epoch()const noexcept{return epoch_;}
    uint64_t Generation()const noexcept{return generation_;}
    uint64_t Id()const noexcept{return consumerId_;}
    DeclarationCompletionFailure Rejection()const noexcept{return failure_;}
private:
    struct Identity {
        uint64_t frameOwner{},group{};
        bool operator==(const Identity&)const=default;
    };
    static bool Pointer(uint64_t address,uint64_t bytes)noexcept{
        return address && !(address&7) && address<=UINT64_MAX-bytes && address<=std::numeric_limits<uintptr_t>::max();
    }
    bool ReadIdentity(uint64_t context,ReadMemory read,bool beforeRetain,Identity& out){
        failure_=DeclarationCompletionFailure::Read;
        if(!read(context+0x38,&out.frameOwner,8) || !read(context+0x18,&out.group,8))return false;
        failure_=DeclarationCompletionFailure::FrameOwner;if(!Pointer(out.frameOwner,12))return false;
        failure_=DeclarationCompletionFailure::Group;if(!Pointer(out.group,0x20) || out.group==out.frameOwner)return false;
        int32_t refs{},pending{};failure_=DeclarationCompletionFailure::Read;
        if(!read(out.group+0x1c,&refs,4) || !read(out.group+0x18,&pending,4))return false;
        failure_=DeclarationCompletionFailure::References;
        if(refs<=0 || (beforeRetain && refs==std::numeric_limits<int32_t>::max()))return false;
        failure_=DeclarationCompletionFailure::Pending;if(pending<0)return false;
        return true;
    }
    bool Reject(DeclarationCompletionFailure reason)noexcept{
        if(!failed_)failure_=reason;
        failed_=true;return false;
    }
    Identity identity_;
    JobLeaseApi api_{};void* groupPin_{};
    uint64_t contextAddress_{},executionGraph_{},imageBase_{},epoch_{},generation_{},consumerId_{};
    bool attempted_{},captured_{},matched_{},declareReturned_{},rootReturned_{},failed_{};
    DeclarationCompletionFailure failure_=DeclarationCompletionFailure::None;
};
#endif
}
