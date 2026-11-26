#pragma once
#include "Render/NativeGraphJobs.hpp"
#include <limits>

namespace cvr::stereo::graph_jobs {
inline constexpr uint32_t CompilerDispatchReturn=0x40A76C,CompilerCallReturn=0xBB68B5;
struct CompilerBinding {
    uint64_t consumerId{},epoch{},generation{},renderer{},wrapper{},pool{},frameOwner{},executionGraph{},producerContext{};
    bool operator==(const CompilerBinding&)const=default;
};
enum class CompilerIdentityFailure:uint32_t {
    None,Arguments,Caller,AlreadyAttempted,MissingApi,Read,ProducerIdentity,Renderer,Wrapper,Pool,
    FrameOwner,Group,References,Pending,Changed,NotCaptured,AlreadyMatched,CompilerJob,
    CompilerParent,NoCallbackToken,NotMatched,AlreadyReturned
};
#if CVR_RUST_VRIK
// The lease: rust/crates/cvr-render/src/native/compiler_identity.rs through graph_jobs_abi.rs
// (tools/single_pass_tests/compiler_identity_test runs its cases against it in its Rust variant).
struct RustCompilerLease;
struct RustCompilerState {
    CompilerBinding binding;uint64_t group,producerContext,rendererArgument,compilerJob,compilerParentHandle;
    uint32_t failure;bool captured,matched,dispatchReturned,compilerReturned,rootReturned,failed;
};
static_assert(sizeof(CompilerBinding)==72 && sizeof(RustCompilerState)==128,"mirrored by graph_jobs_abi.rs");
extern "C" {
RustCompilerLease* cvr_cl_new();
void cvr_cl_free(RustCompilerLease*);
bool cvr_cl_capture(RustCompilerLease*,const CompilerBinding*,uint64_t,uint64_t,uint64_t,uint64_t,ReadMemory,const JobLeaseApi*);
bool cvr_cl_matches_parent(const RustCompilerLease*,uint64_t,ReadMemory);
bool cvr_cl_match(RustCompilerLease*,uint64_t,uint8_t,uint64_t,uint64_t,ReadMemory);
bool cvr_cl_step(RustCompilerLease*,uint32_t);
bool cvr_cl_matches_binding(const RustCompilerLease*,const CompilerBinding*);
void cvr_cl_state(const RustCompilerLease*,RustCompilerState*);
}
#endif

// Exact ABC574/AF59C0/142634/142F88/1EDA84 body and dispatch coverage are supplied
// by the caller. The graph label comes from the verified owned constructor/D0
// receipt: ABC574 and AF59C0 carry renderer, not graph. This pins only D1; it
// neither owns definitions nor proves resource freshness or GPU completion.
// Caller serializes methods; native retain/release/complete must not throw.
#if CVR_RUST_VRIK
class CompilerIdentityLease {
public:
    CompilerIdentityLease():rust_(cvr_cl_new()){}
    ~CompilerIdentityLease(){cvr_cl_free(rust_);}
    CompilerIdentityLease(const CompilerIdentityLease&)=delete;
    CompilerIdentityLease& operator=(const CompilerIdentityLease&)=delete;
    CompilerIdentityLease(CompilerIdentityLease&&)=delete;
    CompilerIdentityLease& operator=(CompilerIdentityLease&&)=delete;

    // A throwing reader unwinds through the rules (their pin releases the group) and is refused with Read here.
    bool CaptureBeforeDispatch(const CompilerBinding& binding,uint64_t producerJob,uint64_t rendererArgumentAddress,
                               uint64_t caller,uint64_t imageBase,ReadMemory read,JobLeaseApi api)try{
        return cvr_cl_capture(rust_,&binding,producerJob,rendererArgumentAddress,caller,imageBase,read,&api);
    }catch(...){return cvr_cl_step(rust_,4);}

    // Non-mutating domain filter: unrelated frame compilations may use the same
    // pool. A failed filter is not a completion or an authority to discard pins.
    bool MatchesCompilerParent(uint64_t compilerJob,ReadMemory read)const try{
        return cvr_cl_matches_parent(rust_,compilerJob,read);
    }catch(...){return false;}

    // The AF59C0 local JobQueue's +20 points to the actual worker handle. It is
    // not that worker itself: never apply worker+68/+150 or read job+38 as a
    // renderer. Matching may happen after root return; no producer stack reads.
    bool MatchBeforeCompiler(uint64_t pool,uint8_t mode,uint64_t compilerJob,uint64_t caller,ReadMemory read)try{
        return cvr_cl_match(rust_,pool,mode,compilerJob,caller,read);
    }catch(...){return cvr_cl_step(rust_,4);}

    bool MarkOriginalDispatchReturned()noexcept{return cvr_cl_step(rust_,0);}
    bool MarkOriginalCompilerReturned()noexcept{return cvr_cl_step(rust_,1);}
    // Normal original219730 return, not cancellation, timeout, or AA scheduler
    // return. Clear ephemeral addresses even when a prior proof failed.
    bool MarkOriginalRootReturned()noexcept{return cvr_cl_step(rust_,2);}
    bool Complete()const noexcept{return cvr_cl_step(rust_,3);}
    bool Captured()const noexcept{return State().captured;}
    bool Matched()const noexcept{return State().matched;}
    bool OriginalDispatchReturned()const noexcept{return State().dispatchReturned;}
    bool OriginalCompilerReturned()const noexcept{return State().compilerReturned;}
    bool OriginalRootReturned()const noexcept{return State().rootReturned;}
    bool Failed()const noexcept{return State().failed;}
    uint64_t Group()const noexcept{return State().group;}
    const CompilerBinding& Binding()const noexcept{binding_=State().binding;return binding_;}
    bool MatchesBinding(const CompilerBinding& expected)const noexcept{return cvr_cl_matches_binding(rust_,&expected);}
    uint64_t ProducerContextAddress()const noexcept{return State().producerContext;}
    uint64_t RendererArgumentAddress()const noexcept{return State().rendererArgument;}
    uint64_t CompilerJobAddress()const noexcept{return State().compilerJob;}
    // Frozen identity label only; no late dereference is permitted.
    uint64_t CompilerParentHandle()const noexcept{return State().compilerParentHandle;}
    CompilerIdentityFailure Rejection()const noexcept{return CompilerIdentityFailure(State().failure);}
private:
    RustCompilerState State()const noexcept{RustCompilerState state{};cvr_cl_state(rust_,&state);return state;}
    RustCompilerLease* rust_;
    // Binding()'s returned copy (the caller serializes this object).
    mutable CompilerBinding binding_;
};
#else
class CompilerIdentityLease {
public:
    CompilerIdentityLease()=default;
    ~CompilerIdentityLease(){if(groupPin_)api_.release(&groupPin_);}
    CompilerIdentityLease(const CompilerIdentityLease&)=delete;
    CompilerIdentityLease& operator=(const CompilerIdentityLease&)=delete;
    CompilerIdentityLease(CompilerIdentityLease&&)=delete;
    CompilerIdentityLease& operator=(CompilerIdentityLease&&)=delete;

    bool CaptureBeforeDispatch(const CompilerBinding& binding,uint64_t producerJob,uint64_t rendererArgumentAddress,
                               uint64_t caller,uint64_t imageBase,ReadMemory read,JobLeaseApi api)try{
        if(failed_)return false;
        if(attempted_)return Reject(CompilerIdentityFailure::AlreadyAttempted);
        attempted_=true;
        if(!read || !imageBase || imageBase>UINT64_MAX-CompilerCallReturn || !binding.consumerId || !binding.epoch ||
            !binding.generation || !Pointer(producerJob,0x40) || !Pointer(rendererArgumentAddress,8) ||
            !Pointer(binding.renderer,0xA0) || !Pointer(binding.wrapper,8) || !Pointer(binding.pool,3) ||
            !Pointer(binding.frameOwner,12) || !Pointer(binding.executionGraph,0x70))
            return Reject(CompilerIdentityFailure::Arguments);
        if(producerJob!=binding.producerContext)return Reject(CompilerIdentityFailure::ProducerIdentity);
        if(caller!=imageBase+CompilerDispatchReturn)return Reject(CompilerIdentityFailure::Caller);
        if(!api.retain || !api.release || !api.complete)return Reject(CompilerIdentityFailure::MissingApi);
        DispatchIdentity first,second;
        if(!ReadDispatch(binding,producerJob,rendererArgumentAddress,read,true,first) ||
            !ReadDispatch(binding,producerJob,rendererArgumentAddress,read,true,second))return Reject(failure_);
        if(first!=second)return Reject(CompilerIdentityFailure::Changed);
        struct Pin {
            JobLeaseApi api;void* value{};
            ~Pin(){if(value)api.release(&value);}
        } pin{api,reinterpret_cast<void*>(uintptr_t(first.group))};
        api.retain(pin.value);
        DispatchIdentity third;
        if(!ReadDispatch(binding,producerJob,rendererArgumentAddress,read,false,second) ||
            !ReadDispatch(binding,producerJob,rendererArgumentAddress,read,false,third))return Reject(failure_);
        if(first!=second || first!=third)return Reject(CompilerIdentityFailure::Changed);
        binding_=binding;imageBase_=imageBase;producerContext_=producerJob;rendererArgumentAddress_=rendererArgumentAddress;
        api_=api;groupPin_=pin.value;pin.value=nullptr;captured_=true;failure_=CompilerIdentityFailure::None;return true;
    }catch(...){return Reject(CompilerIdentityFailure::Read);}

    // Non-mutating domain filter: unrelated frame compilations may use the same
    // pool. A failed filter is not a completion or an authority to discard pins.
    bool MatchesCompilerParent(uint64_t compilerJob,ReadMemory read)const try{
        if(!captured_ || failed_ || !read)return false;
        Parent first,second;CompilerIdentityFailure reason{};
        return ReadParent(compilerJob,read,first,reason) && ReadParent(compilerJob,read,second,reason) &&
            first==second && first.group==Group();
    }catch(...){return false;}

    // The AF59C0 local JobQueue's +20 points to the actual worker handle. It is
    // not that worker itself: never apply worker+68/+150 or read job+38 as a
    // renderer. Matching may happen after root return; no producer stack reads.
    bool MatchBeforeCompiler(uint64_t pool,uint8_t mode,uint64_t compilerJob,uint64_t caller,ReadMemory read)try{
        if(failed_)return false;
        if(!captured_)return Reject(CompilerIdentityFailure::NotCaptured);
        if(matched_)return Reject(CompilerIdentityFailure::AlreadyMatched);
        if(!read || !Pointer(pool,3))return Reject(CompilerIdentityFailure::Arguments);
        if(caller!=imageBase_+CompilerCallReturn || mode!=2)return Reject(CompilerIdentityFailure::Caller);
        if(pool!=binding_.pool)return Reject(CompilerIdentityFailure::Pool);
        CompileIdentity first,second;
        if(!ReadCompile(compilerJob,pool,read,first) || !ReadCompile(compilerJob,pool,read,second))return Reject(failure_);
        if(first!=second)return Reject(CompilerIdentityFailure::Changed);
        compilerJob_=compilerJob;compilerParentHandle_=first.parent.handle;matched_=true;
        failure_=CompilerIdentityFailure::None;return true;
    }catch(...){return Reject(CompilerIdentityFailure::Read);}

    bool MarkOriginalDispatchReturned()noexcept{
        rendererArgumentAddress_=0;
        if(failed_)return false;
        if(!captured_)return Reject(CompilerIdentityFailure::NotCaptured);
        if(dispatchReturned_)return Reject(CompilerIdentityFailure::AlreadyReturned);
        dispatchReturned_=true;return true;
    }
    bool MarkOriginalCompilerReturned()noexcept{
        compilerJob_=0;
        if(failed_)return false;
        if(!captured_)return Reject(CompilerIdentityFailure::NotCaptured);
        if(!matched_)return Reject(CompilerIdentityFailure::NotMatched);
        if(compilerReturned_)return Reject(CompilerIdentityFailure::AlreadyReturned);
        compilerReturned_=true;return true;
    }
    // Normal original219730 return, not cancellation, timeout, or AA scheduler
    // return. Clear ephemeral addresses even when a prior proof failed.
    bool MarkOriginalRootReturned()noexcept{
        producerContext_=rendererArgumentAddress_=0;
        if(!captured_)return Reject(CompilerIdentityFailure::NotCaptured);
        if(rootReturned_)return Reject(CompilerIdentityFailure::AlreadyReturned);
        rootReturned_=true;return !failed_;
    }
    bool Complete()const noexcept{
        return captured_ && !failed_ && matched_ && dispatchReturned_ && compilerReturned_ && rootReturned_ &&
            groupPin_ && api_.complete(groupPin_);
    }
    bool Captured()const noexcept{return captured_;}
    bool Matched()const noexcept{return matched_;}
    bool OriginalDispatchReturned()const noexcept{return dispatchReturned_;}
    bool OriginalCompilerReturned()const noexcept{return compilerReturned_;}
    bool OriginalRootReturned()const noexcept{return rootReturned_;}
    bool Failed()const noexcept{return failed_;}
    uint64_t Group()const noexcept{return reinterpret_cast<uintptr_t>(groupPin_);}
    const CompilerBinding& Binding()const noexcept{return binding_;}
    bool MatchesBinding(const CompilerBinding& expected)const noexcept{return captured_ && !failed_ && binding_==expected;}
    uint64_t ProducerContextAddress()const noexcept{return producerContext_;}
    uint64_t RendererArgumentAddress()const noexcept{return rendererArgumentAddress_;}
    uint64_t CompilerJobAddress()const noexcept{return compilerJob_;}
    // Frozen identity label only; no late dereference is permitted.
    uint64_t CompilerParentHandle()const noexcept{return compilerParentHandle_;}
    CompilerIdentityFailure Rejection()const noexcept{return failure_;}
private:
    struct DispatchIdentity {
        uint64_t group{},frame{},renderer{},wrapper{},pool{};
        bool operator==(const DispatchIdentity&)const=default;
    };
    struct Parent {uint64_t handle{},group{};bool operator==(const Parent&)const=default;};
    struct CompileIdentity {
        Parent parent;uint64_t wrapper{},pool{};
        bool operator==(const CompileIdentity&)const=default;
    };
    static bool Pointer(uint64_t address,uint64_t bytes)noexcept{
        return address && !(address&7) && address<=UINT64_MAX-bytes && address<=std::numeric_limits<uintptr_t>::max();
    }
    bool ReadDispatch(const CompilerBinding& binding,uint64_t job,uint64_t argument,ReadMemory read,
                      bool beforeRetain,DispatchIdentity& out){
        failure_=CompilerIdentityFailure::Read;
        if(!read(argument,&out.renderer,8) || !read(job+0x18,&out.group,8) || !read(job+0x38,&out.frame,8))return false;
        failure_=CompilerIdentityFailure::Renderer;if(out.renderer!=binding.renderer)return false;
        failure_=CompilerIdentityFailure::FrameOwner;if(out.frame!=binding.frameOwner)return false;
        failure_=CompilerIdentityFailure::Group;if(!Pointer(out.group,0x20) || out.group==out.frame)return false;
        failure_=CompilerIdentityFailure::Read;if(!read(out.renderer+0x98,&out.wrapper,8))return false;
        failure_=CompilerIdentityFailure::Wrapper;if(out.wrapper!=binding.wrapper)return false;
        failure_=CompilerIdentityFailure::Read;if(!read(out.wrapper,&out.pool,8))return false;
        failure_=CompilerIdentityFailure::Pool;if(out.pool!=binding.pool)return false;
        int32_t refs{},pending{};failure_=CompilerIdentityFailure::Read;
        if(!read(out.group+0x1c,&refs,4) || !read(out.group+0x18,&pending,4))return false;
        failure_=CompilerIdentityFailure::References;
        if(refs<=0 || (beforeRetain && refs==std::numeric_limits<int32_t>::max()))return false;
        failure_=CompilerIdentityFailure::Pending;if(pending<0)return false;
        return true;
    }
    static bool ReadParent(uint64_t job,ReadMemory read,Parent& out,CompilerIdentityFailure& reason){
        reason=CompilerIdentityFailure::CompilerJob;if(!Pointer(job,0x28))return false;
        reason=CompilerIdentityFailure::Read;if(!read(job+0x20,&out.handle,8))return false;
        reason=CompilerIdentityFailure::CompilerParent;if(!Pointer(out.handle,8))return false;
        reason=CompilerIdentityFailure::Read;if(!read(out.handle,&out.group,8))return false;
        reason=CompilerIdentityFailure::Group;if(!Pointer(out.group,0x20))return false;
        int32_t refs{},pending{};reason=CompilerIdentityFailure::Read;
        if(!read(out.group+0x1c,&refs,4) || !read(out.group+0x18,&pending,4))return false;
        reason=CompilerIdentityFailure::References;if(refs<=0)return false;
        reason=CompilerIdentityFailure::NoCallbackToken;if(pending<=0)return false;
        return true;
    }
    bool ReadCompile(uint64_t job,uint64_t pool,ReadMemory read,CompileIdentity& out){
        if(!ReadParent(job,read,out.parent,failure_))return false;
        failure_=CompilerIdentityFailure::Group;if(out.parent.group!=Group())return false;
        failure_=CompilerIdentityFailure::Read;if(!read(binding_.renderer+0x98,&out.wrapper,8))return false;
        failure_=CompilerIdentityFailure::Wrapper;if(out.wrapper!=binding_.wrapper)return false;
        failure_=CompilerIdentityFailure::Read;if(!read(out.wrapper,&out.pool,8))return false;
        failure_=CompilerIdentityFailure::Pool;if(out.pool!=pool || out.pool!=binding_.pool)return false;
        return true;
    }
    bool Reject(CompilerIdentityFailure reason)noexcept{
        if(!failed_)failure_=reason;failed_=true;return false;
    }
    CompilerBinding binding_;
    JobLeaseApi api_{};void* groupPin_{};
    uint64_t imageBase_{},producerContext_{},rendererArgumentAddress_{},compilerJob_{},compilerParentHandle_{};
    bool attempted_{},captured_{},matched_{},dispatchReturned_{},compilerReturned_{},rootReturned_{},failed_{};
    CompilerIdentityFailure failure_=CompilerIdentityFailure::None;
};
#endif
}
