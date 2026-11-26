#pragma once
#include "Render/NativeGraphSnapshot.hpp"
#include <algorithm>
#include <map>
#include <memory>
#include <set>
#include <string_view>
#if CVR_RUST_VRIK
#include <array>
#include <cstddef>
#endif

namespace cvr::stereo::paired_frontier {
using graph_snapshot::Field;
using Link=std::pair<uint32_t,uint32_t>;
using Edges=std::set<Link>;
enum class Result:uint32_t {Rejected,CandidateNeedsCpuAndResourceCompile};
enum class Boundary:uint32_t {Solid=0,Discard=1};
enum class Failure:uint32_t {None,Source,Nodes,Edges,CpuCycle,OrderCycle,Roles,Prefix,OrderBoundary,CandidateCycle,Invariant,Allocation};
struct Roles {
    uint32_t collector{},depth{},solidReady{},discard{},afterDiscard{},postGBuffer{},lighting{},final2D{};
    std::vector<uint32_t> solid,prefix;
};
struct Invariants {
    bool originalCpuAcyclic{},originalOrderAcyclic{},candidateOrderAcyclic{};
    bool cpuUnchanged{},unrelatedOrderEdgesPreserved{},bothProducersBeforeMainDiscard{};
    bool mainGeometryBeforeLighting{},peerGeometryBeforeLighting{};
    // These require native scheduling/resource/parent receipts, not this planner.
    bool cpuProducerClosureProved{},resourceOrderCompiled{},parentPassMappingProved{};
};
constexpr uint64_t Name(std::string_view value){
    uint64_t hash=14695981039346656037ull;for(const auto c:value)hash=(hash^uint8_t(c))*1099511628211ull;return hash;
}
namespace detail {
#if CVR_RUST_VRIK
// The helpers: rust/crates/cvr-render/src/paired_frontier.rs (tools/single_pass_tests/paired_pass_frontier_test runs its
// cases against them in its Rust variant). Edge sets are lent flattened, in their order.
using RustLink=std::array<uint32_t,2>;
extern "C" {
size_t cvr_pf_path(const RustLink*,size_t,uint32_t,uint32_t,uint32_t*);
bool cvr_pf_acyclic(const uint32_t*,size_t,const RustLink*,size_t);
bool cvr_pf_only(uint32_t,const RustLink*,size_t,uint32_t,uint32_t*);
uint64_t cvr_pf_fingerprint(const graph_snapshot::Node*,size_t,const graph_snapshot::Edge*,size_t,uint32_t,uint64_t);
}
inline std::vector<RustLink> Flat(const Edges& edges){
    std::vector<RustLink> flat;flat.reserve(edges.size());for(const auto& [from,to]:edges)flat.push_back({from,to});return flat;
}
inline std::vector<uint32_t> Path(const Edges& edges,uint32_t from,uint32_t to){
    const auto flat=Flat(edges);std::vector<uint32_t> result(flat.size()+1);
    result.resize(cvr_pf_path(flat.data(),flat.size(),from,to,result.data()));return result;
}
inline bool Acyclic(const std::set<uint32_t>& nodes,const Edges& edges){
    const std::vector<uint32_t> ids(nodes.begin(),nodes.end());const auto flat=Flat(edges);
    return cvr_pf_acyclic(ids.data(),ids.size(),flat.data(),flat.size());
}
inline bool OnlySuccessor(const Edges& edges,uint32_t source,uint32_t& target){
    const auto flat=Flat(edges);return cvr_pf_only(0,flat.data(),flat.size(),source,&target);
}
inline bool OnlyPredecessor(const Edges& edges,uint32_t target,uint32_t& source){
    const auto flat=Flat(edges);return cvr_pf_only(1,flat.data(),flat.size(),target,&source);
}
inline uint64_t Fingerprint(const graph_snapshot::Snapshot& source,uint64_t base){
    return cvr_pf_fingerprint(source.nodes.data(),source.nodes.size(),source.edges.data(),source.edges.size(),source.failure,base);
}
// Plan::BuildCandidate's members, lent by Rust until cvr_pf_free.
struct RustRoles {uint32_t ids[8];const uint32_t* solid;size_t solidCount;const uint32_t* prefix;size_t prefixCount;};
struct RustLinks {const RustLink* data;size_t count;};
struct RustIds {const uint32_t* data;size_t count;};
struct RustPlan {
    uint32_t result,failure,boundary;bool hasSource;uint64_t base,fingerprint;
    RustRoles roles[2];RustLinks links[9];RustIds ids[3];Invariants invariants;
};
struct RustBuilt;
static_assert(sizeof(RustLink)==8 && sizeof(Invariants)==11 && sizeof(RustRoles)==64 && sizeof(RustPlan)==368 &&
    offsetof(RustPlan,invariants)==352,"mirrored by paired_frontier.rs");
extern "C" {
RustBuilt* cvr_pf_build(const graph_snapshot::Node*,size_t,const graph_snapshot::Edge*,size_t,uint32_t,uint64_t,uint32_t,RustPlan*);
void cvr_pf_free(RustBuilt*);
}
inline Edges Unflat(const RustLinks& links){
    Edges edges;for(size_t i=0;i<links.count;++i)edges.emplace_hint(edges.end(),links.data[i][0],links.data[i][1]);return edges;
}
inline std::vector<uint32_t> Ids(const uint32_t* data,size_t count){return std::vector<uint32_t>(data,data+count);}
#else
inline std::vector<uint32_t> Path(const Edges& edges,uint32_t from,uint32_t to){
    std::map<uint32_t,uint32_t> previous{{from,from}};std::vector<uint32_t> pending{from};
    for(size_t i=0;i<pending.size();++i){const auto current=pending[i];
        if(current==to){std::vector<uint32_t> result{to};while(result.back()!=from)result.push_back(previous.at(result.back()));
            std::reverse(result.begin(),result.end());return result;}
        for(auto edge=edges.lower_bound({current,0});edge!=edges.end() && edge->first==current;++edge)
            if(previous.emplace(edge->second,current).second)pending.push_back(edge->second);
    }
    return {};
}
inline bool Acyclic(const std::set<uint32_t>& nodes,const Edges& edges){
    std::map<uint32_t,size_t> incoming;for(const auto id:nodes)incoming[id]=0;
    for(const auto& [from,to]:edges){if(!incoming.contains(from) || !incoming.contains(to))return false;++incoming[to];}
    std::vector<uint32_t> pending;for(const auto& [id,count]:incoming)if(!count)pending.push_back(id);
    for(size_t i=0;i<pending.size();++i)
        for(auto edge=edges.lower_bound({pending[i],0});edge!=edges.end() && edge->first==pending[i];++edge)
            if(!--incoming[edge->second])pending.push_back(edge->second);
    return pending.size()==nodes.size();
}
inline bool OnlySuccessor(const Edges& edges,uint32_t source,uint32_t& target){
    auto edge=edges.lower_bound({source,0});if(edge==edges.end() || edge->first!=source)return false;
    target=edge->second;++edge;return edge==edges.end() || edge->first!=source;
}
inline bool OnlyPredecessor(const Edges& edges,uint32_t target,uint32_t& source){
    size_t count{};for(const auto& [a,b]:edges)if(b==target){source=a;++count;}return count==1;
}
inline uint64_t Fingerprint(const graph_snapshot::Snapshot& source,uint64_t base){
    uint64_t hash=14695981039346656037ull;
    const auto bytes=[&](const auto& values){for(const auto value:values)hash=(hash^uint8_t(value))*1099511628211ull;};
    const auto number=[&](uint64_t value){for(uint32_t i=0;i<8;++i){hash=(hash^uint8_t(value))*1099511628211ull;value>>=8;}};
    number(base);number(source.failure);number(source.nodes.size());number(source.edges.size());
    for(const auto& node:source.nodes){number(node.handle);number(node.order);bytes(node.bytes);number(node.vtable);number(node.work);bytes(node.definition);}
    for(const auto& edge:source.edges){number(edge.handle);number(edge.order);bytes(edge.bytes);}
    return hash;
}
#endif
}

// This is an immutable source-bound topology receipt, NOT NativePreparationSchedule
// JobGraph, a resource dependency graph, or an executable native graph shadow.
class Plan {
public:
#if CVR_RUST_VRIK
    // The rules: paired_frontier.rs; a rejection keeps the members computed so far, as here.
    Result BuildCandidate(const graph_snapshot::Snapshot& source,uint64_t imageBase,Boundary boundary=Boundary::Solid)try{
        [[maybe_unused]] const auto previousSource=source_;
        *this=Plan{};
        detail::RustPlan lent{};
        const auto built=detail::cvr_pf_build(source.nodes.data(),source.nodes.size(),source.edges.data(),source.edges.size(),
            source.failure,imageBase,uint32_t(boundary),&lent);
        struct Release{detail::RustBuilt* built;~Release(){detail::cvr_pf_free(built);}}release{built};
        for(uint32_t eye=0;eye<2;++eye){const auto& from=lent.roles[eye];auto& roles=roles_[eye];
            roles.collector=from.ids[0];roles.depth=from.ids[1];roles.solidReady=from.ids[2];roles.discard=from.ids[3];
            roles.afterDiscard=from.ids[4];roles.postGBuffer=from.ids[5];roles.lighting=from.ids[6];roles.final2D=from.ids[7];
            roles.solid=detail::Ids(from.solid,from.solidCount);roles.prefix=detail::Ids(from.prefix,from.prefixCount);
        }
        for(uint32_t phase=0;phase<2;++phase){
            original_[phase]=detail::Unflat(lent.links[phase]);proposed_[phase]=detail::Unflat(lent.links[2+phase]);
            removed_[phase]=detail::Unflat(lent.links[4+phase]);added_[phase]=detail::Unflat(lent.links[6+phase]);
        }
        cpuIncoming_=detail::Unflat(lent.links[8]);invariants_=lent.invariants;
        cpuBlocker_=detail::Ids(lent.ids[0].data,lent.ids[0].count);orderBlocker_=detail::Ids(lent.ids[1].data,lent.ids[1].count);
        peerCpuClosure_=detail::Ids(lent.ids[2].data,lent.ids[2].count);boundary_=Boundary(lent.boundary);
        if(lent.hasSource){source_=std::make_shared<const graph_snapshot::Snapshot>(source);base_=lent.base;fingerprint_=lent.fingerprint;}
        failure_=Failure(lent.failure);result_=Result(lent.result);return result_;
    }catch(...){*this=Plan{};failure_=Failure::Allocation;return result_;}
#else
    Result BuildCandidate(const graph_snapshot::Snapshot& source,uint64_t imageBase,Boundary boundary=Boundary::Solid)try{
        [[maybe_unused]] const auto previousSource=source_;
        *this=Plan{};failure_=Failure::Source;
        if(source.failure || source.nodes.empty() || source.nodes.size()>255 || source.edges.size()>65536 ||
            !imageBase || imageBase>UINT64_MAX-0xD5BAC4 || (boundary!=Boundary::Solid && boundary!=Boundary::Discard))return result_;
        boundary_=boundary;
        std::map<uint32_t,const graph_snapshot::Node*> nodes;std::set<uint32_t> ids,edgeIds;
        failure_=Failure::Nodes;
        for(size_t i=0;i<source.nodes.size();++i){const auto& node=source.nodes[i];const auto id=node.handle&0xDFFFFFFFu;
            const auto view=Field<uint32_t>(node.bytes,24),kind=Field<uint32_t>(node.bytes,8);
            if(id>=65536 || node.order!=i || Field<uint32_t>(node.bytes,0)!=node.order || view>1 ||
                (kind!=0 && kind!=1 && kind!=2 && kind!=4) || !ids.insert(id).second ||
                (node.work && (!node.vtable || !Field<uint64_t>(node.bytes,16) || Field<uint64_t>(node.definition,0)!=node.vtable)))return result_;
            nodes.emplace(id,&node);
        }
        failure_=Failure::Edges;
        for(size_t i=0;i<source.edges.size();++i){const auto& edge=source.edges[i];const auto id=edge.handle&0xBFFFFFFFu;
            const auto phase=Field<uint32_t>(edge.bytes,4),from=Field<uint32_t>(edge.bytes,8)&0xDFFFFFFFu,to=Field<uint32_t>(edge.bytes,12)&0xDFFFFFFFu;
            if(id>=262144 || edge.order!=i || Field<uint32_t>(edge.bytes,0)!=edge.order || phase>1 || !edgeIds.insert(id).second ||
                !ids.contains(from) || !ids.contains(to) || from==to || !original_[phase].emplace(from,to).second)return result_;
        }
        failure_=Failure::CpuCycle;invariants_.originalCpuAcyclic=detail::Acyclic(ids,original_[0]);if(!invariants_.originalCpuAcyclic)return result_;
        failure_=Failure::OrderCycle;invariants_.originalOrderAcyclic=detail::Acyclic(ids,original_[1]);if(!invariants_.originalOrderAcyclic)return result_;
        failure_=Failure::Roles;
        std::array<std::array<uint32_t,6>,2> counts{};
        for(const auto& [id,node]:nodes){const auto view=Field<uint32_t>(node->bytes,24);auto& roles=roles_[view];
            const auto work=node->work>=imageBase?node->work-imageBase:0;
            if(work==0xC0F204){roles.collector=id;++counts[view][0];}
            if(work!=0x332D5C)continue;const auto name=Field<uint64_t>(node->definition,24);
            if(name==Name("DepthPrepass")){roles.depth=id;++counts[view][1];}
            else if(name==Name("GBuffer_Solid"))roles.solid.push_back(id);
            else if(name==Name("GBuffer_Discard")){roles.discard=id;++counts[view][2];}
            else if(name==Name("PostGBufferOverrides")){roles.postGBuffer=id;++counts[view][3];}
            else if(name==Name("Lighting")){roles.lighting=id;++counts[view][4];}
            else if(name==Name("RenderFinal2D")){roles.final2D=id;++counts[view][5];}
        }
        for(uint32_t eye=0;eye<2;++eye){if(counts[eye]!=std::array<uint32_t,6>{1,1,1,1,1,1} || roles_[eye].solid.size()!=4)return result_;
            uint32_t ready{};if(!detail::OnlyPredecessor(original_[1],roles_[eye].discard,ready))return result_;
            const auto* node=nodes.at(ready);
            if(node->work!=imageBase+0x655FAC || Field<uint32_t>(node->bytes,24)!=eye || Field<uint32_t>(node->definition,24)!=2)return result_;
            roles_[eye].solidReady=ready;
        }
        failure_=Failure::Prefix;
        // Exact verified native producer shape, identified by roles, not node IDs.
        constexpr std::array<uint32_t,21> expected{1,17,18,19,32,33,34,35,40,41,42,43,44,45,46,47,48,48,48,48,49};
        for(uint32_t eye=0;eye<2;++eye){auto& roles=roles_[eye];uint32_t current=roles.collector;
            for(size_t step=0;step<expected.size();++step){
                const auto* node=nodes.at(current);if(Field<uint32_t>(node->bytes,24)!=eye || PrefixRole(*node,imageBase)!=expected[step])return result_;
                roles.prefix.push_back(current);
                if(step+1<expected.size()){uint32_t next{};if(!detail::OnlySuccessor(original_[1],current,next))return result_;
                    uint32_t previous{};if(!detail::OnlyPredecessor(original_[1],next,previous) || previous!=current)return result_;current=next;}
            }
            if(current!=roles.solidReady || std::find(roles.prefix.begin(),roles.prefix.end(),roles.depth)==roles.prefix.end())return result_;
            std::set<uint32_t> expectedSolid(roles.solid.begin(),roles.solid.end());
            if(expectedSolid!=std::set<uint32_t>(roles.prefix.end()-5,roles.prefix.end()-1))return result_;
        }
        failure_=Failure::OrderBoundary;
        for(uint32_t eye=0;eye<2;++eye){auto& roles=roles_[eye];
            const bool successor=detail::OnlySuccessor(original_[1],roles.discard,roles.afterDiscard);
            if(boundary_!=Boundary::Discard)continue;
            uint32_t previous{};
            if(!successor || !detail::OnlyPredecessor(original_[1],roles.afterDiscard,previous) || previous!=roles.discard)return result_;
            // The whole discard pass precedes the unchanged per-eye post/lighting suffix.
            uint32_t current=roles.discard;
            for(const auto target:{roles.postGBuffer,roles.lighting,roles.final2D}){
                while(current!=target){uint32_t next{};
                    if(!detail::OnlySuccessor(original_[1],current,next) || Field<uint32_t>(nodes.at(next)->bytes,24)!=eye ||
                        !detail::OnlyPredecessor(original_[1],next,previous) || previous!=current)return result_;
                    current=next;
                }
            }
        }
        const auto& main=roles_[0];const auto& peer=roles_[1];
        uint32_t previous{};
        if(!detail::OnlyPredecessor(original_[1],peer.collector,previous) || previous!=main.final2D ||
            !original_[1].contains({main.solidReady,main.discard}) || !original_[1].contains({peer.solidReady,peer.discard}) ||
            detail::Path(original_[1],main.discard,main.postGBuffer).empty() || detail::Path(original_[1],main.postGBuffer,main.lighting).empty() ||
            detail::Path(original_[1],main.lighting,main.final2D).empty() ||
            detail::Path(original_[1],peer.discard,peer.postGBuffer).empty() || detail::Path(original_[1],peer.postGBuffer,peer.lighting).empty())return result_;
        const auto tail=EarlyTail(),continuation=boundary_==Boundary::Discard?peer.afterDiscard:peer.discard;
        removed_[1]={{main.solidReady,main.discard},{main.final2D,peer.collector},{tail,continuation}};
        added_[1]={{main.solidReady,peer.collector},{tail,main.discard},{main.final2D,continuation}};
        proposed_=original_;
        for(const auto edge:removed_[1])if(proposed_[1].erase(edge)!=1)return result_;
        for(const auto edge:added_[1])if(!proposed_[1].insert(edge).second)return result_;
        failure_=Failure::CandidateCycle;invariants_.candidateOrderAcyclic=detail::Acyclic(ids,proposed_[1]);if(!invariants_.candidateOrderAcyclic)return result_;
        invariants_.cpuUnchanged=proposed_[0]==original_[0];
        auto unrelated=proposed_[1];for(const auto edge:added_[1])unrelated.erase(edge);for(const auto edge:removed_[1])unrelated.insert(edge);
        invariants_.unrelatedOrderEdgesPreserved=unrelated==original_[1];
        invariants_.bothProducersBeforeMainDiscard=!detail::Path(proposed_[1],main.solidReady,main.discard).empty() && !detail::Path(proposed_[1],tail,main.discard).empty();
        invariants_.mainGeometryBeforeLighting=!detail::Path(proposed_[1],main.discard,main.postGBuffer).empty() && !detail::Path(proposed_[1],main.postGBuffer,main.lighting).empty();
        invariants_.peerGeometryBeforeLighting=!detail::Path(proposed_[1],peer.discard,peer.postGBuffer).empty() && !detail::Path(proposed_[1],peer.postGBuffer,peer.lighting).empty();
        failure_=Failure::Invariant;
        if(!invariants_.cpuUnchanged || !invariants_.unrelatedOrderEdgesPreserved || !invariants_.bothProducersBeforeMainDiscard ||
            !invariants_.mainGeometryBeforeLighting || !invariants_.peerGeometryBeforeLighting)return result_;
        cpuBlocker_=detail::Path(original_[0],main.discard,tail);
        orderBlocker_=detail::Path(original_[1],main.discard,tail);
        // Include synthetic joins and per-eye prerequisites in the reported CPU
        // closure, but do not assume their ownership constraints can be moved.
        std::set<uint32_t> closure{tail};bool changed=true;
        while(changed){changed=false;for(const auto& [a,b]:original_[0])if(closure.contains(b) && Field<uint32_t>(nodes.at(a)->bytes,24)==1)
            changed|=closure.insert(a).second;}
        peerCpuClosure_.assign(closure.begin(),closure.end());
        for(const auto& edge:original_[0])if(!closure.contains(edge.first) && closure.contains(edge.second))cpuIncoming_.insert(edge);
        source_=std::make_shared<const graph_snapshot::Snapshot>(source);base_=imageBase;fingerprint_=detail::Fingerprint(source,imageBase);
        failure_=Failure::None;result_=Result::CandidateNeedsCpuAndResourceCompile;return result_;
    }catch(...){*this=Plan{};failure_=Failure::Allocation;return result_;}
#endif

    bool Matches(const graph_snapshot::Snapshot& current,uint64_t imageBase)const{
        return source_ && result_==Result::CandidateNeedsCpuAndResourceCompile && imageBase==base_ && !current.failure &&
            fingerprint_==detail::Fingerprint(current,imageBase) && source_->nodes==current.nodes && source_->edges==current.edges;
    }
    Result State()const{return result_;}
    Failure Rejection()const{return failure_;}
    bool ExecutionReady()const{return false;}
    Boundary SelectedBoundary()const{return boundary_;}
    uint32_t EarlyTail()const{return boundary_==Boundary::Discard?roles_[1].discard:roles_[1].solidReady;}
    uint64_t SourceFingerprint()const{return fingerprint_;}
    uint64_t ImageBase()const{return base_;}
    std::shared_ptr<const graph_snapshot::Snapshot> Source()const{return source_;}
    const std::array<Roles,2>& EyeRoles()const{return roles_;}
    const std::array<Edges,2>& Original()const{return original_;}
    const std::array<Edges,2>& Proposed()const{return proposed_;}
    const std::array<Edges,2>& Removed()const{return removed_;}
    const std::array<Edges,2>& Added()const{return added_;}
    const Invariants& Proofs()const{return invariants_;}
    const std::vector<uint32_t>& CpuBlockerPath()const{return cpuBlocker_;}
    const std::vector<uint32_t>& OrderBlockerPath()const{return orderBlocker_;}
    const std::vector<uint32_t>& PeerCpuClosure()const{return peerCpuClosure_;}
    const Edges& CpuIncoming()const{return cpuIncoming_;}
private:
#if !CVR_RUST_VRIK
    static uint32_t PrefixRole(const graph_snapshot::Node& node,uint64_t base){
        const auto work=node.work>=base?node.work-base:0;const auto word=Field<uint32_t>(node.definition,24);
        if(work==0xC0F204)return 1;
        if(work==0xBF436C)return word==17?17:word==2?18:word==4?19:0;
        if(work==0xD5BAC4 && word<4 && Field<uint32_t>(node.definition,28)==4)return 32+word;
        if(work==0x77596C)return 40;
        if(work==0x655FAC)return word==1?43:word==2?49:0;
        if(work==0xCB1E88)return 47;
        if(work!=0x332D5C)return 0;const auto name=Field<uint64_t>(node.definition,24);
        if(name==Name("RenderPrep"))return 41;
        if(name==Name("PrepareGloballyAccessibleSystems"))return 42;
        if(name==Name("AsyncWater"))return 44;
        if(name==Name("DepthPrepass"))return 45;
        if(name==Name("GBufferEarly_Velocity_Weapon"))return 46;
        if(name==Name("GBuffer_Solid"))return 48;
        return 0;
    }
#endif
    Result result_=Result::Rejected;Failure failure_=Failure::None;
    Boundary boundary_=Boundary::Solid;
    uint64_t base_{},fingerprint_{};
    std::shared_ptr<const graph_snapshot::Snapshot> source_;
    std::array<Roles,2> roles_;
    std::array<Edges,2> original_,proposed_,removed_,added_;
    Invariants invariants_;
    std::vector<uint32_t> cpuBlocker_,orderBlocker_,peerCpuClosure_;
    Edges cpuIncoming_;
};
}
