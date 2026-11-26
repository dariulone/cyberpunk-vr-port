#pragma once
#include "Render/NativeDefinitionOwner.hpp"
#include "Render/PairedPassFrontier.hpp"
#include <map>

namespace cvr::stereo::graph_definition {
using ReadMemory=definition_owner::ReadMemory;
using NativeVector=definition_owner::NativeVector;
inline constexpr uint32_t CacheSlots=4,CacheSlotBytes=0x90,MaxOwners=128,MaxDefinitions=8192;
struct Owner {
    uint64_t perViewGraph{};
    definition_owner::MovePlan move;
};
struct Mapping {
    uint32_t nodeHandle{},ownerIndex{},definitionIndex{};
    uint64_t object{};
    bool operator==(const Mapping&)const=default;
};
enum class Failure:uint32_t {
    None,Arguments,Source,CacheSlot,OwnerVector,OwnerBounds,Owners,OwnerPlan,
    AllocationOverlap,DefinitionBudget,DuplicateDefinition,UnownedNode,Changed,Allocation
};
enum class CacheUsePolicy:uint32_t {StableCounter,AllowConcurrentHits};
// Membership does not prove immutable definition contents or execution safety.
enum class DefinitionContentPolicy:uint32_t {ImmutablePrefix,OwnerMembership};
// Compared fields, not proof of a write. Nested capture flags deliberately do
// not guess which private parser read failed or changed between its snapshots.
enum class ChangedDetail:uint32_t {
    CacheUse=1u<<0,CacheKey=1u<<1,OwnerVector=1u<<2,OwnerPointers=1u<<3,
    Allocator=1u<<4,OwnerHeader=1u<<5,Entries=1u<<6,NodeRaw=1u<<7,
    EdgeRaw=1u<<8,VtableWork=1u<<9,DefinitionPrefix=1u<<10,Mappings=1u<<11,
    DoubleSnapshot=1u<<12,Read=1u<<13,GraphCapture=1u<<14,OwnerCapture=1u<<15,
    CacheSlot=1u<<16,Fingerprint=1u<<17
};
#if CVR_RUST_VRIK
// The rules: rust/crates/cvr-render/src/graph_definition.rs (tools/single_pass_tests/definition_source_test runs its
// cases against them in its Rust variant). SourceReceipt owns the Rust receipt; its members stay here for the accessors.
namespace detail {
struct RustOwner {uint64_t perViewGraph;definition_owner::RustPlanFields plan;};
struct RustReceipt {
    uint32_t lastUse,definitionSlots;uint64_t key,allocator;NativeVector ownerVector;
    const RustOwner* owners;size_t ownerCount;const Mapping* mappings;size_t mappingCount;
    uint64_t cacheBase,graph,slot,base,fingerprint;uint32_t failure,changedDetails,policy,contentPolicy;bool ready;
};
struct RustHeld;
static_assert(sizeof(Mapping)==24 && sizeof(RustOwner)==80 && sizeof(RustReceipt)==136,"mirrored by graph_definition.rs");
extern "C" {
RustHeld* cvr_gd_capture(uint64_t,uint64_t,const graph_snapshot::Node*,size_t,const graph_snapshot::Edge*,size_t,uint32_t,uint64_t,
    ReadMemory,uint32_t,uint32_t,RustReceipt*);
bool cvr_gd_matches(const RustHeld*,ReadMemory,uint32_t*);
void cvr_gd_free(RustHeld*);
}
}
#endif

// A read-only mapping, not a lease, native-exclusive section, generation monitor or move.
// Equal bytes cannot exclude allocator ABA; runtime mutation hooks supply that proof.
class SourceReceipt {
public:
#if CVR_RUST_VRIK
    bool Capture(uint64_t cacheBase,uint64_t compositeGraph,const graph_snapshot::Snapshot& source,
                 uint64_t imageBase,ReadMemory read,CacheUsePolicy policy=CacheUsePolicy::StableCounter,
                 DefinitionContentPolicy contentPolicy=DefinitionContentPolicy::ImmutablePrefix)try{
        [[maybe_unused]] const auto priorSource=source_;
        *this=SourceReceipt{};detail::RustReceipt out{};
        const auto held=detail::cvr_gd_capture(cacheBase,compositeGraph,source.nodes.data(),source.nodes.size(),source.edges.data(),
            source.edges.size(),source.failure,imageBase,read,uint32_t(policy),uint32_t(contentPolicy),&out);
        if(held)rust_=std::shared_ptr<detail::RustHeld>(held,detail::cvr_gd_free);
        cacheBase_=out.cacheBase;graph_=out.graph;slot_=out.slot;base_=out.base;
        policy_=CacheUsePolicy(out.policy);contentPolicy_=DefinitionContentPolicy(out.contentPolicy);
        failure_=Failure(out.failure);changedDetails_=out.changedDetails;
        if(!held)return false;
        snapshot_.lastUse=out.lastUse;snapshot_.definitionSlots=out.definitionSlots;snapshot_.key=out.key;
        snapshot_.allocator=out.allocator;snapshot_.ownerVector=out.ownerVector;
        for(size_t i=0;i<out.ownerCount;++i)
            snapshot_.owners.push_back({out.owners[i].perViewGraph,definition_owner::MovePlan::Restored(out.owners[i].plan)});
        snapshot_.mappings.assign(out.mappings,out.mappings+out.mappingCount);
        source_=std::make_shared<const graph_snapshot::Snapshot>(source);fingerprint_=out.fingerprint;ready_=true;return true;
    }catch(...){*this=SourceReceipt{};failure_=Failure::Allocation;return false;}
    bool MatchesBefore(ReadMemory read,uint32_t* changedDetails=nullptr)const{
        if(changedDetails)*changedDetails=0;
        if(!ready_ || !source_ || !rust_)return false;
        uint32_t detail{};const auto same=detail::cvr_gd_matches(rust_.get(),read,&detail);
        if(changedDetails)*changedDetails=detail;return same;
    }
#else
    bool Capture(uint64_t cacheBase,uint64_t compositeGraph,const graph_snapshot::Snapshot& source,
                 uint64_t imageBase,ReadMemory read,CacheUsePolicy policy=CacheUsePolicy::StableCounter,
                 DefinitionContentPolicy contentPolicy=DefinitionContentPolicy::ImmutablePrefix)try{
        [[maybe_unused]] const auto priorSource=source_;
        *this=SourceReceipt{};failure_=Failure::Arguments;
        if(!read || !Pointer(cacheBase) || !Pointer(compositeGraph) || !imageBase ||
            (policy!=CacheUsePolicy::StableCounter && policy!=CacheUsePolicy::AllowConcurrentHits) ||
            (contentPolicy!=DefinitionContentPolicy::ImmutablePrefix && contentPolicy!=DefinitionContentPolicy::OwnerMembership) ||
            cacheBase>UINT64_MAX-uint64_t(CacheSlots)*CacheSlotBytes || compositeGraph>UINT64_MAX-0x80)return false;
        failure_=Failure::Source;
        if(source.failure || source.nodes.empty() || source.nodes.size()>8192 || source.edges.size()>65536)return false;
        cacheBase_=cacheBase;graph_=compositeGraph;base_=imageBase;policy_=policy;contentPolicy_=contentPolicy;
        failure_=Failure::CacheSlot;
        if(compositeGraph<cacheBase+0x10)return false;
        const auto displacement=compositeGraph-cacheBase-0x10;
        if(displacement%CacheSlotBytes || displacement/CacheSlotBytes>=CacheSlots)return false;
        slot_=cacheBase+displacement;
        Snapshot first,second;
        if(!ReadOnce(read,source,first) || !ReadOnce(read,source,second))return false;
        failure_=Failure::Changed;
        if(const auto detail=Difference(first,second,policy_))return Changed(detail|Bit(ChangedDetail::DoubleSnapshot));
        snapshot_=std::move(first);source_=std::make_shared<const graph_snapshot::Snapshot>(source);
        fingerprint_=paired_frontier::detail::Fingerprint(source,imageBase);ready_=true;failure_=Failure::None;return true;
    }catch(...){*this=SourceReceipt{};failure_=Failure::Allocation;return false;}
    bool MatchesBefore(ReadMemory read,uint32_t* changedDetails=nullptr)const{
        if(changedDetails)*changedDetails=0;
        if(!ready_ || !source_)return false;SourceReceipt current;
        if(!current.Capture(cacheBase_,graph_,*source_,base_,read,policy_,contentPolicy_)){
            if(changedDetails)*changedDetails=current.ChangedDetails();return false;}
        uint32_t detail=Difference(current.snapshot_,snapshot_,policy_);
        if(current.slot_!=slot_)detail|=Bit(ChangedDetail::CacheSlot);
        if(current.fingerprint_!=fingerprint_)detail|=Bit(ChangedDetail::Fingerprint);
        if(changedDetails)*changedDetails=detail;return !detail;
    }
#endif
    bool Ready()const{return ready_;}
    Failure Rejection()const{return failure_;}
    uint32_t ChangedDetails()const{return changedDetails_;}
    bool DefinitionOwnershipProved()const{return false;}
    bool NativeExclusiveProved()const{return false;}
    uint64_t SourceFingerprint()const{return fingerprint_;}
    uint64_t CacheBase()const{return cacheBase_;}
    uint64_t CacheSlot()const{return slot_;}
    uint64_t CacheKey()const{return snapshot_.key;}
    uint32_t LastUse()const{return snapshot_.lastUse;}
    uint64_t CompositeGraph()const{return graph_;}
    uint64_t ImageBase()const{return base_;}
    const NativeVector& OwnerVector()const{return snapshot_.ownerVector;}
    uint64_t OwnerVectorAllocator()const{return snapshot_.allocator;}
    uint32_t DefinitionSlots()const{return snapshot_.definitionSlots;}
    const std::vector<Owner>& Owners()const{return snapshot_.owners;}
    const std::vector<Mapping>& Mappings()const{return snapshot_.mappings;}
    std::shared_ptr<const graph_snapshot::Snapshot> Source()const{return source_;}
private:
    struct Snapshot {
        uint32_t lastUse{},definitionSlots{};
        uint64_t key{},allocator{};
        NativeVector ownerVector;
        std::vector<Owner> owners;
        std::vector<Mapping> mappings;
    };
#if !CVR_RUST_VRIK
    struct Range {uint64_t begin{},end{};};
    static bool Pointer(uint64_t value){return value && !(value&7);}
    static bool Overlap(Range a,Range b){return a.begin<b.end && b.begin<a.end;}
    static constexpr uint32_t Bit(ChangedDetail detail){return uint32_t(detail);}
    bool Changed(uint32_t detail){changedDetails_=detail;return false;}
    static uint32_t OwnerDifference(const definition_owner::MovePlan& a,const definition_owner::MovePlan& b){
        uint32_t detail{};
        if(a.SavedHeader()!=b.SavedHeader() || a.OwnerAddress()!=b.OwnerAddress())detail|=Bit(ChangedDetail::OwnerHeader);
        if(a.EmptyHeader()!=b.EmptyHeader() || a.TrailerAddress()!=b.TrailerAddress())detail|=Bit(ChangedDetail::Allocator);
        if(a.Definitions()!=b.Definitions())detail|=Bit(ChangedDetail::Entries);
        return detail;
    }
    static uint32_t OwnerFailure(definition_owner::Failure failure){
        uint32_t detail=Bit(ChangedDetail::OwnerCapture);
        using Reason=definition_owner::Failure;
        if(failure==Reason::Header || failure==Reason::Bounds)detail|=Bit(ChangedDetail::OwnerHeader);
        if(failure==Reason::Entries)detail|=Bit(ChangedDetail::Entries);
        if(failure==Reason::Allocator)detail|=Bit(ChangedDetail::Allocator);
        if(failure==Reason::Changed)detail|=Bit(ChangedDetail::DoubleSnapshot);
        return detail;
    }
    static uint32_t Difference(const Snapshot& a,const Snapshot& b,CacheUsePolicy policy){
        uint32_t detail{};
        if(policy==CacheUsePolicy::StableCounter && a.lastUse!=b.lastUse)detail|=Bit(ChangedDetail::CacheUse);
        if(a.key!=b.key)detail|=Bit(ChangedDetail::CacheKey);
        if(a.allocator!=b.allocator)detail|=Bit(ChangedDetail::Allocator);
        if(a.ownerVector!=b.ownerVector)detail|=Bit(ChangedDetail::OwnerVector);
        if(a.definitionSlots!=b.definitionSlots)detail|=Bit(ChangedDetail::Entries);
        if(a.mappings!=b.mappings)detail|=Bit(ChangedDetail::Mappings);
        if(a.owners.size()!=b.owners.size())detail|=Bit(ChangedDetail::OwnerPointers);
        for(size_t i=0;i<std::min(a.owners.size(),b.owners.size());++i){const auto& x=a.owners[i];const auto& y=b.owners[i];
            if(x.perViewGraph!=y.perViewGraph)detail|=Bit(ChangedDetail::OwnerPointers);
            detail|=OwnerDifference(x.move,y.move);}
        return detail;
    }
    static uint32_t GraphDifference(const graph_snapshot::Snapshot& a,const graph_snapshot::Snapshot& b,DefinitionContentPolicy policy){
        uint32_t detail{};
        if(a.nodes.size()!=b.nodes.size())detail|=Bit(ChangedDetail::NodeRaw);
        if(a.edges.size()!=b.edges.size())detail|=Bit(ChangedDetail::EdgeRaw);
        for(size_t i=0;i<std::min(a.nodes.size(),b.nodes.size());++i){const auto& x=a.nodes[i];const auto& y=b.nodes[i];
            if(x.handle!=y.handle || x.order!=y.order || x.bytes!=y.bytes)detail|=Bit(ChangedDetail::NodeRaw);
            if(x.vtable!=y.vtable || x.work!=y.work)detail|=Bit(ChangedDetail::VtableWork);
            if(policy==DefinitionContentPolicy::ImmutablePrefix && x.definition!=y.definition)detail|=Bit(ChangedDetail::DefinitionPrefix);}
        for(size_t i=0;i<std::min(a.edges.size(),b.edges.size());++i)
            if(a.edges[i]!=b.edges[i])detail|=Bit(ChangedDetail::EdgeRaw);
        return detail;
    }
    bool ReadOnce(ReadMemory read,const graph_snapshot::Snapshot& expected,Snapshot& out){
        out={};failure_=Failure::Source;graph_snapshot::Snapshot current;
        if(!current.Capture(graph_,read))return Changed(Bit(ChangedDetail::GraphCapture));
        if(const auto detail=GraphDifference(current,expected,contentPolicy_))return Changed(detail);
        failure_=Failure::OwnerVector;
        if(!read(slot_,&out.lastUse,4) || !read(slot_+8,&out.key,8) || !read(graph_+0x70,&out.ownerVector,sizeof(NativeVector)))return false;
        const auto& h=out.ownerVector;failure_=Failure::OwnerBounds;
        if(!Pointer(h.data) || !h.count || h.count>h.capacity || h.capacity>MaxOwners ||
            h.data>UINT64_MAX-uint64_t(h.capacity)*8-8)return false;
        const Range cache{cacheBase_,cacheBase_+uint64_t(CacheSlots)*CacheSlotBytes};
        const Range pointers{h.data,h.data+uint64_t(h.capacity)*8+8};
        if(Overlap(cache,pointers) || !read(pointers.end-8,&out.allocator,8) || !Pointer(out.allocator) ||
            (out.allocator>=pointers.begin && out.allocator<pointers.end))return false;
        std::vector<uint64_t> graphs(h.count);
        if(!read(h.data,graphs.data(),graphs.size()*8))return false;
        std::set<uint64_t> unique;std::vector<Range> graphRanges;
        failure_=Failure::Owners;
        for(const auto graph:graphs){
            if(!Pointer(graph) || graph>UINT64_MAX-0x88 || !unique.insert(graph).second)return false;
            const Range region{graph,graph+0x88};
            if(Overlap(region,cache) || Overlap(region,pointers) ||
                std::any_of(graphRanges.begin(),graphRanges.end(),[&](auto previous){return Overlap(region,previous);}))return false;
            graphRanges.push_back(region);
        }
        std::vector<Range> allocations;
        std::map<uint64_t,std::pair<uint32_t,uint32_t>> definitions;
        for(uint32_t i=0;i<graphs.size();++i){
            Owner owner;owner.perViewGraph=graphs[i];failure_=Failure::OwnerPlan;
            if(!owner.move.Capture(owner.perViewGraph,read))return Changed(OwnerFailure(owner.move.Rejection()));
            failure_=Failure::DefinitionBudget;
            if(owner.move.Definitions().size()>MaxDefinitions-out.definitionSlots)return false;
            out.definitionSlots+=uint32_t(owner.move.Definitions().size());
            if(owner.move.OwnsAllocation()){
                const Range region{owner.move.SavedHeader().data,owner.move.TrailerAddress()+8};failure_=Failure::AllocationOverlap;
                if(Overlap(region,cache) || Overlap(region,pointers) ||
                    std::any_of(graphRanges.begin(),graphRanges.end(),[&](auto object){return Overlap(region,object);}) ||
                    std::any_of(allocations.begin(),allocations.end(),[&](auto previous){return Overlap(region,previous);}))return false;
                allocations.push_back(region);
            }
            failure_=Failure::DuplicateDefinition;
            for(uint32_t j=0;j<owner.move.Definitions().size();++j){const auto definition=owner.move.Definitions()[j];
                if(definition && !definitions.emplace(definition,std::pair{i,j}).second)return false;}
            out.owners.push_back(std::move(owner));
        }
        failure_=Failure::AllocationOverlap;
        for(const auto& [object,owner]:definitions){
            const Range point{object,object+1};
            if(Overlap(point,cache) || Overlap(point,pointers) ||
                std::any_of(graphRanges.begin(),graphRanges.end(),[&](auto region){return Overlap(point,region);}) ||
                std::any_of(allocations.begin(),allocations.end(),[&](auto region){return Overlap(point,region);}))return false;
        }
        failure_=Failure::UnownedNode;
        for(const auto& node:expected.nodes){const auto object=graph_snapshot::Field<uint64_t>(node.bytes,16);
            if(!object){if(node.work || node.vtable || graph_snapshot::Field<uint32_t>(node.bytes,8)!=4)return false;continue;}
            const auto owner=definitions.find(object);if(owner==definitions.end())return false;
            out.mappings.push_back({node.handle,owner->second.first,owner->second.second,object});
        }
        failure_=Failure::Changed;uint32_t lastUse{};uint64_t key{};NativeVector ownerVector;
        if(!read(slot_,&lastUse,4))return Changed(Bit(ChangedDetail::Read)|Bit(ChangedDetail::CacheUse));
        if(policy_==CacheUsePolicy::StableCounter && lastUse!=out.lastUse)return Changed(Bit(ChangedDetail::CacheUse));
        if(!read(slot_+8,&key,8))return Changed(Bit(ChangedDetail::Read)|Bit(ChangedDetail::CacheKey));
        if(key!=out.key)return Changed(Bit(ChangedDetail::CacheKey));
        if(!read(graph_+0x70,&ownerVector,sizeof(ownerVector)))return Changed(Bit(ChangedDetail::Read)|Bit(ChangedDetail::OwnerVector));
        if(ownerVector!=h)return Changed(Bit(ChangedDetail::OwnerVector));
        std::vector<uint64_t> graphsAfter(h.count);
        if(!read(h.data,graphsAfter.data(),graphsAfter.size()*8))return Changed(Bit(ChangedDetail::Read)|Bit(ChangedDetail::OwnerPointers));
        if(graphsAfter!=graphs)return Changed(Bit(ChangedDetail::OwnerPointers));
        uint64_t allocator{};
        if(!read(pointers.end-8,&allocator,8))return Changed(Bit(ChangedDetail::Read)|Bit(ChangedDetail::Allocator));
        if(allocator!=out.allocator)return Changed(Bit(ChangedDetail::Allocator));
        for(const auto& owner:out.owners){definition_owner::MovePlan after;
            if(!after.Capture(owner.move.OwnerAddress()-0x70,read))return Changed(OwnerFailure(after.Rejection()));
            if(const auto detail=OwnerDifference(after,owner.move))return Changed(detail);}
        graph_snapshot::Snapshot after;
        if(!after.Capture(graph_,read))return Changed(Bit(ChangedDetail::GraphCapture));
        if(const auto detail=GraphDifference(after,expected,contentPolicy_))return Changed(detail);
        return true;
    }
#endif
    Snapshot snapshot_;
    std::shared_ptr<const graph_snapshot::Snapshot> source_;
    uint64_t cacheBase_{},graph_{},slot_{},base_{},fingerprint_{};
    bool ready_{};Failure failure_=Failure::None;
    uint32_t changedDetails_{};
    CacheUsePolicy policy_=CacheUsePolicy::StableCounter;
    DefinitionContentPolicy contentPolicy_=DefinitionContentPolicy::ImmutablePrefix;
#if CVR_RUST_VRIK
    std::shared_ptr<detail::RustHeld> rust_;
#endif
};
}
