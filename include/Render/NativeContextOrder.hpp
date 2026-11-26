#pragma once
#include "Render/PairedPassFrontier.hpp"
#include <span>

namespace cvr::stereo::context_order {
using graph_snapshot::Field;
using Read=graph_snapshot::Read;
enum class Failure:uint32_t {None,Source,Classification,Edges,Ranks,CpuCycle,UnpublishedPrefix,Allocation};
struct Classification {
    uint32_t handle{},kind{},methodRva{};
    bool operator==(const Classification&)const=default;
};
struct Witness {uint32_t producer{},consumer{},producerSlot{},consumerSlot{},kind{};};
struct Result {
    Failure failure=Failure::Source;
    uint64_t fingerprint{};
    uint32_t publishers{},synchronizers{},requiredPairs{},provedPairs{};
    std::vector<Classification> classes;
    std::vector<Witness> missing;
    // Only CPU readiness for native prefix consumption; not resource/alias safety.
    bool PrefixReady()const{return failure==Failure::None;}
};

#if CVR_RUST_VRIK
// The rules: rust/crates/cvr-render/src/context_order.rs (tools/single_pass_tests/context_order_test runs its cases
// against them in its Rust variant). Analyze's result is assembled here from what Rust lends back.
namespace detail {
struct RustAnalysis {
    uint32_t failure,publishers;uint64_t fingerprint;uint32_t synchronizers,requiredPairs,provedPairs;
    const Classification* classes;size_t classCount;const Witness* missing;size_t missingCount;
};
struct RustHeld;
static_assert(sizeof(Classification)==12 && sizeof(Witness)==20 && sizeof(RustAnalysis)==64,"mirrored by context_order.rs");
extern "C" {
bool cvr_co_classify(const graph_snapshot::Node*,uint64_t,Read,Classification*);
RustHeld* cvr_co_analyze(const graph_snapshot::Node*,size_t,const graph_snapshot::Edge*,size_t,uint32_t,uint64_t,Read,RustAnalysis*);
void cvr_co_free(RustHeld*);
}
}
inline bool Classify(const graph_snapshot::Node& node,uint64_t base,Read read,Classification& out){
    return detail::cvr_co_classify(&node,base,read,&out);
}
// 1EC404 publishes type-2 contexts only in execution mode. 58DC98 consumes and
// clears EVERY slot through its phase-1 rank, even an as-yet unpublished slot.
// A phase-1 DAG alone cannot prove these producers finished on the CPU.
inline Result Analyze(const graph_snapshot::Snapshot& graph,uint64_t base,Read read)try{
    detail::RustAnalysis lent{};
    const auto held=detail::cvr_co_analyze(graph.nodes.data(),graph.nodes.size(),graph.edges.data(),graph.edges.size(),graph.failure,
        base,read,&lent);
    struct Release{detail::RustHeld* held;~Release(){detail::cvr_co_free(held);}}release{held};
    Result result;result.failure=Failure(lent.failure);result.fingerprint=lent.fingerprint;result.publishers=lent.publishers;
    result.synchronizers=lent.synchronizers;result.requiredPairs=lent.requiredPairs;result.provedPairs=lent.provedPairs;
    result.classes.assign(lent.classes,lent.classes+lent.classCount);result.missing.assign(lent.missing,lent.missing+lent.missingCount);
    return result;
}catch(...){Result result;result.failure=Failure::Allocation;return result;}
#else
inline bool Classify(const graph_snapshot::Node& node,uint64_t base,Read read,Classification& out){
    out={node.handle&0xDFFFFFFFu,0,0};
    if(!node.work && !node.vtable)return
        !Field<uint64_t>(node.bytes,16) &&
        std::all_of(node.definition.begin(),node.definition.end(),[](uint8_t byte){return !byte;});
    if(!base || !read || node.vtable<base || node.vtable-base>UINT32_MAX ||
        node.vtable>UINT64_MAX-0x30 || Field<uint64_t>(node.definition,0)!=node.vtable)return false;
    uint64_t method{};
    if(!read(node.vtable+0x28,&method,8) || method<base || method-base>UINT32_MAX)return false;
    struct Profile {uint32_t vtable,work;};
    constexpr Profile zero[]{
        {0x30946F8,0xCB1E88},{0x3094758,0xBF436C},{0x36D10F0,0xBFDD24},
        {0x36D10C0,0xBF60F0},{0x36D05B0,0xC28A18},{0x36D05E0,0x1FF2B0},
        {0x36D0580,0x4F4708},{0x36D0970,0xC7A5C8},{0x36D0940,0x5AEEC8},
        {0x36D0790,0x425AA0},{0x36D1738,0xD5BAC4},{0x36D13A8,0xC0F204},
        {0x36D1D98,0x77596C}};
    const auto vtable=node.vtable-base;
    out.methodRva=uint32_t(method-base);
    std::array<uint8_t,6> bytes{};
    if(vtable==0x3094850 && node.work==base+0x332D5C && out.methodRva==0x1941AC0){
        out.kind=2;
        return read(method,bytes.data(),6) && bytes==std::array<uint8_t,6>{0xB8,2,0,0,0,0xC3};
    }
    if(vtable==0x36CED58 && node.work==base+0x655FAC && out.methodRva==0x1942080){
        out.kind=3;
        return Field<uint32_t>(node.definition,24)<=2 && read(method,bytes.data(),6) &&
            bytes==std::array<uint8_t,6>{0xB8,3,0,0,0,0xC3};
    }
    const bool known=std::any_of(std::begin(zero),std::end(zero),[&](const auto& profile){
        return vtable==profile.vtable && node.work==base+profile.work;});
    return known && out.methodRva==0xADF4E0 && read(method,bytes.data(),3) &&
        bytes[0]==0x33 && bytes[1]==0xC0 && bytes[2]==0xC3;
}

// 1EC404 publishes type-2 contexts only in execution mode. 58DC98 consumes and
// clears EVERY slot through its phase-1 rank, even an as-yet unpublished slot.
// A phase-1 DAG alone cannot prove these producers finished on the CPU.
inline Result Analyze(const graph_snapshot::Snapshot& graph,uint64_t base,Read read)try{
    Result result;
    if(graph.failure || graph.nodes.empty() || graph.nodes.size()>220 || graph.edges.size()>65536 ||
        !base || base>UINT64_MAX-0x36D1CD8 || !read)return result;
    std::map<uint32_t,const graph_snapshot::Node*> nodes;
    std::set<uint32_t> ids;
    std::array<std::set<uint32_t>,2> ranks;
    for(size_t i=0;i<graph.nodes.size();++i){const auto& node=graph.nodes[i];const auto id=node.handle&0xDFFFFFFFu;
        if(id>=65536 || node.order!=i || Field<uint32_t>(node.bytes,0)!=i ||
            !ids.insert(id).second || Field<uint32_t>(node.bytes,24)>1)return result;
        for(size_t phase=0;phase<2;++phase)if(node.bytes[28+phase]>=graph.nodes.size() ||
            !ranks[phase].insert(node.bytes[28+phase]).second){result.failure=Failure::Ranks;return result;}
        Classification classification;
        if(!Classify(node,base,read,classification)){result.failure=Failure::Classification;return result;}
        result.publishers+=classification.kind==2;result.synchronizers+=classification.kind==3;
        result.classes.push_back(classification);nodes.emplace(id,&node);
    }
    std::array<paired_frontier::Edges,2> edges;
    std::set<uint32_t> edgeIds;
    result.failure=Failure::Edges;
    for(size_t i=0;i<graph.edges.size();++i){const auto& row=graph.edges[i];
        const auto phase=Field<uint32_t>(row.bytes,4),from=Field<uint32_t>(row.bytes,8)&0xDFFFFFFFu,
            to=Field<uint32_t>(row.bytes,12)&0xDFFFFFFFu;
        if(row.order!=i || Field<uint32_t>(row.bytes,0)!=i || phase>1 ||
            !edgeIds.insert(row.handle&0xBFFFFFFFu).second || !nodes.contains(from) || !nodes.contains(to) ||
            from==to || !edges[phase].emplace(from,to).second)return result;
        if(nodes.at(from)->bytes[28+phase]>=nodes.at(to)->bytes[28+phase]){result.failure=Failure::Ranks;return result;}
    }
    if(!paired_frontier::detail::Acyclic(ids,edges[0])){result.failure=Failure::CpuCycle;return result;}
    result.fingerprint=paired_frontier::detail::Fingerprint(graph,base);
    for(const auto& producer:result.classes)if(producer.kind==2 || producer.kind==3){
        const auto slot=nodes.at(producer.handle)->bytes[29];
        std::set<uint32_t> reachable{producer.handle};
        std::vector<uint32_t> pending{producer.handle};
        for(size_t i=0;i<pending.size();++i)for(auto edge=edges[0].lower_bound({pending[i],0});
            edge!=edges[0].end() && edge->first==pending[i];++edge)
            if(reachable.insert(edge->second).second)pending.push_back(edge->second);
        for(const auto& consumer:result.classes)if(consumer.kind==3){
            const auto end=nodes.at(consumer.handle)->bytes[29];if(slot>=end)continue;
            ++result.requiredPairs;
            if(reachable.contains(consumer.handle))++result.provedPairs;
            else result.missing.push_back({producer.handle,consumer.handle,slot,end,producer.kind});
        }
    }
    result.failure=result.missing.empty()?Failure::None:Failure::UnpublishedPrefix;
    return result;
}catch(...){Result result;result.failure=Failure::Allocation;return result;}
#endif
}
