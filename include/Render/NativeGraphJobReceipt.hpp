#pragma once
#include "Render/NativeGraphJobs.hpp"

namespace cvr::stereo::graph_jobs {
inline constexpr size_t MaxPendingReceipts=4;
struct FrameOwnerApi {
    void(*retain)(void*);
    void(*release)(void*);
};
enum class ReceiptFailure:uint32_t {None,Arguments,AlreadyCaptured,MissingApi,PlanChanged,JobCapture,Allocation,OwnerReference};
#if CVR_RUST_VRIK
// The receipt: rust/crates/cvr-render/src/native/job_receipt.rs through graph_jobs_abi.rs
// (tools/single_pass_tests/graph_job_receipt_test runs its cases against it in its Rust variant).
struct RustReceipt;
struct RustReceiptState {uint32_t failure;bool captured,selected;size_t nodeCount;uint64_t epoch,generation,imageBase,graph,frameOwner;};
static_assert(sizeof(RustReceiptState)==56,"mirrored by graph_jobs_abi.rs");
extern "C" {
RustReceipt* cvr_jr_new(const JobLeaseApi*,const FrameOwnerApi*);
void cvr_jr_free(RustReceipt*);
bool cvr_jr_capture(RustReceipt*,const RustPlanView*,uint64_t,uint64_t,uint64_t,ReadMemory);
bool cvr_jr_match(RustReceipt*,uint64_t,uint64_t,uint64_t,uint64_t,uint64_t,const RustSourceView*,bool);
bool cvr_jr_complete(const RustReceipt*);
void cvr_jr_state(const RustReceipt*,RustReceiptState*);
}
#endif

// One receipt in the caller's bounded pending collection. Capture must run
// while the native queue callback still owns its payload and node-group array.
// Frame-owner references (+8) and group references (+1C) use separate APIs.
// All native reference callbacks must be nonthrowing. Caller serializes access.
#if CVR_RUST_VRIK
class DeferredReceipt {
public:
    DeferredReceipt(JobLeaseApi jobs,FrameOwnerApi owner):rust_(cvr_jr_new(&jobs,&owner)){}
    ~DeferredReceipt(){cvr_jr_free(rust_);}
    DeferredReceipt(const DeferredReceipt&)=delete;
    DeferredReceipt& operator=(const DeferredReceipt&)=delete;
    bool Capture(const CapturePlan& plan,uint64_t epoch,uint64_t generation,uint64_t imageBase,ReadMemory read){
        const auto view=plan.RustView();
        if(!cvr_jr_capture(rust_,&view,epoch,generation,imageBase,read))return false;
        // The plan's immutable deep copy, the one the receipt was captured with.
        source_=plan.Source();return true;
    }
    bool Match(uint64_t epoch,uint64_t generation,uint64_t imageBase,uint64_t graphAddress,uint64_t declarationFrameOwner,
               const graph_snapshot::Snapshot& source)const{
        const RustSourceView view{source.nodes.data(),source.nodes.size(),source.edges.data(),source.edges.size(),source.failure};
        return cvr_jr_match(rust_,epoch,generation,imageBase,graphAddress,declarationFrameOwner,&view,false);
    }
    bool Select(uint64_t epoch,uint64_t generation,uint64_t imageBase,uint64_t graphAddress,uint64_t declarationFrameOwner,
                const graph_snapshot::Snapshot& source){
        const RustSourceView view{source.nodes.data(),source.nodes.size(),source.edges.data(),source.edges.size(),source.failure};
        return cvr_jr_match(rust_,epoch,generation,imageBase,graphAddress,declarationFrameOwner,&view,true);
    }
    // Completion is independent of selection. A completed pending receipt must
    // remain available for the later declaration association, until root retires it.
    bool Complete()const{return cvr_jr_complete(rust_);}
    bool Captured()const{return State().captured;}
    bool Selected()const{return State().selected;}
    ReceiptFailure Rejection()const{return ReceiptFailure(State().failure);}
    size_t NodeCount()const{return State().nodeCount;}
    uint64_t Epoch()const{return State().epoch;}
    uint64_t Generation()const{return State().generation;}
    uint64_t ImageBase()const{return State().imageBase;}
    uint64_t GraphAddress()const{return State().graph;}
    uint64_t FrameOwner()const{return State().frameOwner;}
    std::shared_ptr<const graph_snapshot::Snapshot> Source()const{return source_;}
private:
    RustReceiptState State()const{RustReceiptState state{};cvr_jr_state(rust_,&state);return state;}
    RustReceipt* rust_;
    std::shared_ptr<const graph_snapshot::Snapshot> source_;
};
#else
class DeferredReceipt {
public:
    DeferredReceipt(JobLeaseApi jobs,FrameOwnerApi owner):jobsApi_(jobs),ownerApi_(owner){}
    ~DeferredReceipt(){jobs_.reset();if(frameOwner_)ownerApi_.release(reinterpret_cast<void*>(uintptr_t(frameOwner_)));}
    DeferredReceipt(const DeferredReceipt&)=delete;
    DeferredReceipt& operator=(const DeferredReceipt&)=delete;
    bool Capture(const CapturePlan& plan,uint64_t epoch,uint64_t generation,uint64_t imageBase,ReadMemory read)try{
        if(captured_){failure_=ReceiptFailure::AlreadyCaptured;return false;}
        failure_=ReceiptFailure::Arguments;
        if(!epoch || !generation || !imageBase || imageBase!=plan.ImageBase() || imageBase>UINT64_MAX-QueueReturn || !read || !plan.Ready() ||
            !plan.FrameOwner() || !plan.GraphAddress() || !plan.Source() || plan.Nodes().empty() || plan.Nodes().size()>MaxNodes)return false;
        failure_=ReceiptFailure::MissingApi;
        if(!ownerApi_.retain || !ownerApi_.release || !jobsApi_.retain || !jobsApi_.release || !jobsApi_.complete)return false;
        failure_=ReceiptFailure::PlanChanged;if(!plan.Matches(read))return false;
        failure_=ReceiptFailure::OwnerReference;int32_t ownerReferences{};
        if(plan.FrameOwner()>UINT64_MAX-12 || !read(plan.FrameOwner()+8,&ownerReferences,sizeof(ownerReferences)) || ownerReferences<=0)return false;
        struct OwnerLease {
            FrameOwnerApi api;void* value{};
            ~OwnerLease(){if(value)api.release(value);}
        } owner{ownerApi_};
        owner.value=reinterpret_cast<void*>(uintptr_t(plan.FrameOwner()));ownerApi_.retain(owner.value);
        auto jobs=std::make_unique<LeaseSet>(jobsApi_);failure_=ReceiptFailure::JobCapture;
        if(!jobs->Capture(plan,read))return false;
        // Source is an immutable deep copy. No later operation reads the raw
        // queue-payload owner address or temporary group-array header.
        source_=plan.Source();epoch_=epoch;generation_=generation;imageBase_=imageBase;
        graph_=plan.GraphAddress();frameOwner_=plan.FrameOwner();jobs_=std::move(jobs);owner.value=nullptr;
        captured_=true;failure_=ReceiptFailure::None;return true;
    }catch(...){failure_=ReceiptFailure::Allocation;return false;}
    bool Match(uint64_t epoch,uint64_t generation,uint64_t imageBase,uint64_t graphAddress,uint64_t declarationFrameOwner,
               const graph_snapshot::Snapshot& source)const{
        return captured_ && source_ && epoch==epoch_ && generation==generation_ && imageBase==imageBase_ && graphAddress==graph_ &&
            declarationFrameOwner==frameOwner_ && !source.failure && source.nodes==source_->nodes && source.edges==source_->edges;
    }
    bool Select(uint64_t epoch,uint64_t generation,uint64_t imageBase,uint64_t graphAddress,uint64_t declarationFrameOwner,
                const graph_snapshot::Snapshot& source){
        if(selected_ || !Match(epoch,generation,imageBase,graphAddress,declarationFrameOwner,source))return false;
        selected_=true;return true;
    }
    // Completion is independent of selection. A completed pending receipt must
    // remain available for the later declaration association, until root retires it.
    bool Complete()const{return captured_ && jobs_ && jobs_->Complete();}
    bool Captured()const{return captured_;}
    bool Selected()const{return selected_;}
    ReceiptFailure Rejection()const{return failure_;}
    size_t NodeCount()const{return jobs_?jobs_->Nodes():0;}
    uint64_t Epoch()const{return epoch_;}
    uint64_t Generation()const{return generation_;}
    uint64_t ImageBase()const{return imageBase_;}
    uint64_t GraphAddress()const{return graph_;}
    uint64_t FrameOwner()const{return frameOwner_;}
    std::shared_ptr<const graph_snapshot::Snapshot> Source()const{return source_;}
private:
    JobLeaseApi jobsApi_;FrameOwnerApi ownerApi_;
    std::unique_ptr<LeaseSet> jobs_;
    std::shared_ptr<const graph_snapshot::Snapshot> source_;
    uint64_t epoch_{},generation_{},imageBase_{},graph_{},frameOwner_{};
    bool captured_{},selected_{};ReceiptFailure failure_=ReceiptFailure::None;
};
#endif
}
