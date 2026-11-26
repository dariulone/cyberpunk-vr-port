#pragma once
#include "Render/NativeGraphSnapshot.hpp"
#include "Render/NativeJobLease.hpp"
#include <memory>
#include <set>

namespace cvr::stereo::graph_jobs {
using ReadMemory=graph_snapshot::Read;
inline constexpr uint32_t MaxNodes=255,QueueReturn=0x3E90BA;
struct NativeArray {uint64_t data{};uint32_t capacity{},count{};bool operator==(const NativeArray&)const=default;};
struct NodeGroups {uint64_t begin{},end{};uint32_t incoming{},outgoing{};bool operator==(const NodeGroups&)const=default;};
static_assert(sizeof(NativeArray)==16 && sizeof(NodeGroups)==24);
struct Mapping {uint32_t node{},ordinal{};NodeGroups groups;bool operator==(const Mapping&)const=default;};
enum class Failure:uint32_t {None,Arguments,Caller,Source,Owner,Header,Count,Groups,Changed,Allocation};
#if CVR_RUST_VRIK
// The rules: rust/crates/cvr-render/src/native/graph_jobs.rs through graph_jobs_abi.rs
// (tools/single_pass_tests/graph_jobs_test runs its cases against them in its Rust variant).
struct RustPlan;struct RustLeaseSet;
struct RustSourceView {const graph_snapshot::Node* nodes;size_t nodeCount;const graph_snapshot::Edge* edges;size_t edgeCount;uint32_t failure;};
struct RustPlanView {
    uint64_t ownerAddress,owner,graph,groupsAddress,base;NativeArray array;const Mapping* mapping;size_t mappingCount;
    RustSourceView source;bool ready;
};
struct RustPlanOut {uint32_t failure;std::array<uint64_t,8> diagnostic;uint64_t owner;NativeArray array;const Mapping* mapping;size_t mappingCount;};
static_assert(sizeof(Mapping)==32 && sizeof(RustPlanView)==120 && sizeof(RustPlanOut)==112,"mirrored by graph_jobs_abi.rs");
extern "C" {
RustPlan* cvr_gj_plan_capture(uint64_t,uint64_t,uint64_t,uint64_t,uint64_t,const RustSourceView*,ReadMemory,RustPlanOut*);
void cvr_gj_plan_free(RustPlan*);
bool cvr_gj_plan_matches(const RustPlanView*,ReadMemory);
RustLeaseSet* cvr_gj_leases_new(const JobLeaseApi*);
void cvr_gj_leases_free(RustLeaseSet*);
bool cvr_gj_leases_capture(RustLeaseSet*,const RustPlanView*,ReadMemory);
bool cvr_gj_leases_complete(const RustLeaseSet*,uint32_t,uint32_t,bool);
bool cvr_gj_leases_state(const RustLeaseSet*,size_t*);
}
#endif

// Only after the original 0x32D60C returns, while its caller still owns the
// 24-byte group array. This receipt identifies native groups, not a lifetime
// lease by itself. Worker/context refcount +8 is NOT the job-group refcount.
class CapturePlan {
public:
#if CVR_RUST_VRIK
    bool Capture(uint64_t renderOwner,uint64_t graph,uint64_t groups,uint64_t caller,uint64_t imageBase,
                 const graph_snapshot::Snapshot& source,ReadMemory read)try{
        [[maybe_unused]] const auto previousSource=source_;
        *this=CapturePlan{};
        const RustSourceView view{source.nodes.data(),source.nodes.size(),source.edges.data(),source.edges.size(),source.failure};
        RustPlanOut out{};const auto plan=cvr_gj_plan_capture(renderOwner,graph,groups,caller,imageBase,&view,read,&out);
        failure_=Failure(out.failure);diagnostic_=out.diagnostic;
        if(!plan)return false;
        struct Release{RustPlan* plan;~Release(){cvr_gj_plan_free(plan);}}release{plan};
        mapping_.assign(out.mapping,out.mapping+out.mappingCount);
        ownerAddress_=renderOwner;owner_=out.owner;graph_=graph;groupsAddress_=groups;array_=out.array;base_=imageBase;
        source_=std::make_shared<const graph_snapshot::Snapshot>(source);ready_=true;return true;
    }catch(...){*this=CapturePlan{};failure_=Failure::Allocation;return false;}
    bool Matches(ReadMemory read)const{
        if(!ready_ || !source_)return false;
        const auto view=RustView();return cvr_gj_plan_matches(&view,read);
    }
    RustPlanView RustView()const{
        const RustSourceView source=source_?RustSourceView{source_->nodes.data(),source_->nodes.size(),source_->edges.data(),
            source_->edges.size(),source_->failure}:RustSourceView{};
        return {ownerAddress_,owner_,graph_,groupsAddress_,base_,array_,mapping_.data(),mapping_.size(),source,ready_ && source_};
    }
#else
    bool Capture(uint64_t renderOwner,uint64_t graph,uint64_t groups,uint64_t caller,uint64_t imageBase,
                 const graph_snapshot::Snapshot& source,ReadMemory read)try{
        [[maybe_unused]] const auto previousSource=source_;
        *this=CapturePlan{};failure_=Failure::Arguments;
        if(!read || !Pointer(renderOwner) || !Pointer(graph) || !Pointer(groups) || renderOwner>UINT64_MAX-8 ||
            graph>UINT64_MAX-0x70 || groups>UINT64_MAX-sizeof(NativeArray) || !imageBase || imageBase>UINT64_MAX-QueueReturn)return false;
        failure_=Failure::Caller;if(caller!=imageBase+QueueReturn)return false;
        failure_=Failure::Source;
        diagnostic_[0]=1;diagnostic_[2]=source.nodes.size();
        if(source.failure || source.nodes.empty() || source.nodes.size()>MaxNodes || source.edges.size()>65536)return false;
        std::set<uint32_t> ids;
        for(size_t i=0;i<source.nodes.size();++i){const auto& node=source.nodes[i];
            if(node.order!=i || graph_snapshot::Field<uint32_t>(node.bytes,0)!=i || !ids.insert(node.handle&0xDFFFFFFFu).second){
                diagnostic_[1]=i;diagnostic_[3]=graph_snapshot::Field<uint32_t>(node.bytes,0);return false;}}
        Snapshot first,second;
        if(!ReadOnce(read,renderOwner,graph,groups,source,first) || !ReadOnce(read,renderOwner,graph,groups,source,second))return false;
        failure_=Failure::Changed;if(first!=second)return false;
        ownerAddress_=renderOwner;owner_=first.owner;graph_=graph;groupsAddress_=groups;array_=first.array;base_=imageBase;
        mapping_.reserve(source.nodes.size());
        for(size_t i=0;i<source.nodes.size();++i)mapping_.push_back({source.nodes[i].handle&0xDFFFFFFFu,uint32_t(i),first.groups[i]});
        source_=std::make_shared<const graph_snapshot::Snapshot>(source);ready_=true;failure_=Failure::None;return true;
    }catch(...){*this=CapturePlan{};failure_=Failure::Allocation;return false;}
    bool Matches(ReadMemory read)const{
        if(!ready_ || !source_)return false;CapturePlan now;
        return now.Capture(ownerAddress_,graph_,groupsAddress_,base_+QueueReturn,base_,*source_,read) &&
            now.owner_==owner_ && now.array_==array_ && now.mapping_==mapping_;
    }
#endif
    bool Ready()const{return ready_;}
    Failure Rejection()const{return failure_;}
    uint64_t OwnerAddress()const{return ownerAddress_;}
    uint64_t FrameOwner()const{return owner_;}
    uint64_t GraphAddress()const{return graph_;}
    uint64_t ImageBase()const{return base_;}
    const std::array<uint64_t,8>& Diagnostic()const{return diagnostic_;}
    uint64_t GroupsAddress()const{return groupsAddress_;}
    const NativeArray& Array()const{return array_;}
    const std::vector<Mapping>& Nodes()const{return mapping_;}
    std::shared_ptr<const graph_snapshot::Snapshot> Source()const{return source_;}
private:
#if !CVR_RUST_VRIK
    struct Snapshot {uint64_t owner{};NativeArray array;std::vector<NodeGroups> groups;bool operator==(const Snapshot&)const=default;};
    static bool Pointer(uint64_t value){return value && !(value&7);}
    bool ReadOnce(ReadMemory read,uint64_t renderOwner,uint64_t graph,uint64_t groups,
                  const graph_snapshot::Snapshot& source,Snapshot& out){
        out={};failure_=Failure::Owner;
        if(!read(renderOwner,&out.owner,8) || !Pointer(out.owner))return false;
        failure_=Failure::Header;
        if(!read(groups,&out.array,sizeof(out.array)))return false;
        const auto& h=out.array;failure_=Failure::Count;
        if(!Pointer(h.data) || h.count!=source.nodes.size() || h.capacity<h.count || h.capacity>8192 ||
            h.data>UINT64_MAX-uint64_t(h.capacity)*sizeof(NodeGroups))return false;
        uint32_t count{};if(!read(graph+0x10,&count,4) || count!=h.count)return false;
        failure_=Failure::Source;graph_snapshot::Snapshot current;
        const bool captured=current.Capture(graph,read);diagnostic_[0]=2;diagnostic_[3]=current.nodes.size();diagnostic_[4]=current.failure;
        if(!captured || current.nodes!=source.nodes || current.edges!=source.edges){
            if(captured){
                for(size_t i=0;i<std::min(current.nodes.size(),source.nodes.size());++i)if(current.nodes[i]!=source.nodes[i]){
                    if(!diagnostic_[5]){diagnostic_[1]=i;const auto* a=reinterpret_cast<const uint8_t*>(&current.nodes[i]);
                        const auto* b=reinterpret_cast<const uint8_t*>(&source.nodes[i]);for(size_t at=0;at<sizeof(graph_snapshot::Node);++at)if(a[at]!=b[at]){diagnostic_[7]=at;break;}}
                    ++diagnostic_[5];}
                for(size_t i=0;i<std::min(current.edges.size(),source.edges.size());++i)diagnostic_[6]+=current.edges[i]!=source.edges[i];
            }return false;}
        out.groups.resize(h.count);failure_=Failure::Groups;
        if(!read(h.data,out.groups.data(),size_t(h.count)*sizeof(NodeGroups)))return false;
        for(const auto& row:out.groups){
            if(!Pointer(row.begin) || !Pointer(row.end) || row.begin>UINT64_MAX-0x20 || row.end>UINT64_MAX-0x20 ||
                row.incoming>65536 || row.outgoing>65536)return false;
            for(const auto pointer:{row.begin,row.end}){int32_t refs{};
                // Pending may legitimately change while jobs execute. Read only
                // the positive group reference as a liveness precondition.
                if(!read(pointer+0x1C,&refs,4) || refs<=0)return false;
            }
        }
        uint64_t ownerAfter{};NativeArray headerAfter;
        failure_=Failure::Changed;
        return read(renderOwner,&ownerAfter,8) && ownerAfter==out.owner && read(groups,&headerAfter,sizeof(headerAfter)) && headerAfter==out.array;
    }
#endif
    std::shared_ptr<const graph_snapshot::Snapshot> source_;
    NativeArray array_;std::vector<Mapping> mapping_;
    uint64_t ownerAddress_{},owner_{},graph_{},groupsAddress_{},base_{};
    bool ready_{};Failure failure_=Failure::None;
    std::array<uint64_t,8> diagnostic_{};
};

#if CVR_RUST_VRIK
class LeaseSet {
public:
    explicit LeaseSet(JobLeaseApi api):rust_(cvr_gj_leases_new(&api)){}
    ~LeaseSet(){cvr_gj_leases_free(rust_);}
    LeaseSet(const LeaseSet&)=delete;
    LeaseSet& operator=(const LeaseSet&)=delete;
    // The native group array still owns its groups during both validation
    // passes. Native retain/release callbacks must not throw.
    bool Capture(const CapturePlan& plan,ReadMemory read)try{
        const auto view=plan.RustView();return cvr_gj_leases_capture(rust_,&view,read);
    }catch(...){return false;}
    bool Complete()const{return cvr_gj_leases_complete(rust_,0,0,true);}
    // Bind selection to the held ordinal/handle mapping, never to a later raw
    // graph or group-array read. This is completion, not execution permission.
    bool Complete(uint32_t ordinal,uint32_t handle)const{return cvr_gj_leases_complete(rust_,ordinal,handle,false);}
    bool Sealed()const{size_t nodes{};return cvr_gj_leases_state(rust_,&nodes);}
    size_t Nodes()const{size_t nodes{};cvr_gj_leases_state(rust_,&nodes);return nodes;}
private:
    RustLeaseSet* rust_;
};
#else
class LeaseSet {
public:
    explicit LeaseSet(JobLeaseApi api):api_(api){}
    LeaseSet(const LeaseSet&)=delete;
    LeaseSet& operator=(const LeaseSet&)=delete;
    // The native group array still owns its groups during both validation
    // passes. Native retain/release callbacks must not throw.
    bool Capture(const CapturePlan& plan,ReadMemory read)try{
        if(sealed_ || !api_.retain || !api_.release || !api_.complete || !plan.Ready() || !plan.Matches(read))return false;
        std::vector<std::unique_ptr<NativeJobLease>> next;next.reserve(plan.Nodes().size());
        for(size_t i=0;i<plan.Nodes().size();++i)next.push_back(std::make_unique<NativeJobLease>(api_));
        for(size_t i=0;i<plan.Nodes().size();++i){const auto& group=plan.Nodes()[i].groups;
            if(!next[i]->Capture(reinterpret_cast<void*>(uintptr_t(group.begin)),reinterpret_cast<void*>(uintptr_t(group.end))))return false;}
        if(!plan.Matches(read))return false;
        auto mapping=plan.Nodes();
        mapping_=std::move(mapping);leases_=std::move(next);sealed_=true;return true;
    }catch(...){return false;}
    bool Complete()const{
        if(!sealed_ || leases_.empty())return false;
        for(const auto& lease:leases_)if(!lease->Complete())return false;return true;
    }
    // Bind selection to the held ordinal/handle mapping, never to a later raw
    // graph or group-array read. This is completion, not execution permission.
    bool Complete(uint32_t ordinal,uint32_t handle)const{
        return sealed_ && ordinal<mapping_.size() && ordinal<leases_.size() &&
            mapping_[ordinal].ordinal==ordinal && mapping_[ordinal].node==handle && leases_[ordinal]->Complete();
    }
    bool Sealed()const{return sealed_;}
    size_t Nodes()const{return leases_.size();}
private:
    JobLeaseApi api_;std::vector<std::unique_ptr<NativeJobLease>> leases_;bool sealed_{};
    std::vector<Mapping> mapping_;
};
#endif
}
