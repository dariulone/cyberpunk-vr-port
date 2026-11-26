#pragma once
#include "Render/OwnedNativeGraph.hpp"

namespace cvr::stereo::scene_expansion {
struct Definition {
    uint64_t object{};
    std::array<uint8_t,32> prefix{};
    bool operator==(const Definition&)const=default;
};
struct Mapping {
    uint32_t sourceHandle{},executionHandle{},section{};
    bool operator==(const Mapping&)const=default;
};
enum class Failure:uint32_t {None,Source,Identity,Definitions,Capacity,Topology,Compile,Contraction,Allocation};
#if CVR_RUST_VRIK
// The rules: rust/crates/cvr-render/src/scene_expansion.rs (tools/single_pass_tests/scene_prefix_order_plan_test and
// context_order_test run their cases against them in their Rust variants). The plan's members stay here.
namespace detail {
struct RustGraph {const graph_snapshot::Node* nodes;size_t nodeCount;const graph_snapshot::Edge* edges;size_t edgeCount;uint32_t failure;};
struct RustExpansion {
    RustGraph expanded;const Mapping* mappings;size_t mappingCount;const std::array<uint32_t,2>* edges[2];size_t edgeCounts[2];
    uint32_t tail,failure;
};
struct RustHeld;
static_assert(sizeof(Definition)==40 && sizeof(Mapping)==12 && sizeof(RustGraph)==40 && sizeof(RustExpansion)==96,
    "mirrored by scene_expansion.rs");
extern "C" {
RustHeld* cvr_sx_build(const RustGraph*,uint64_t,uint32_t,const std::array<Definition,2>*,RustExpansion*);
void cvr_sx_free(RustHeld*);
bool cvr_sx_matches_declaration(const RustGraph*,const RustGraph*,const Mapping*,size_t,const std::array<Definition,2>*,uint32_t,uint64_t,
    const RustGraph*,const RustGraph*,uint64_t);
}
inline RustGraph Lend(const graph_snapshot::Snapshot& graph){
    return {graph.nodes.data(),graph.nodes.size(),graph.edges.data(),graph.edges.size(),graph.failure};
}
// A plan's frozen members and its tail, lent (BothEyesPlan's rules).
struct RustFrozen {
    RustGraph source,expanded;const Mapping* mappings;size_t mappingCount;const std::array<Definition,2>* definitions;
    uint32_t selected,tail;uint64_t base;bool ready;
};
struct RustMappings;
static_assert(sizeof(RustFrozen)==128,"mirrored by scene_expansion.rs");
extern "C" {
uint32_t cvr_sxb_select(const RustGraph*,uint64_t,const std::array<std::array<Definition,2>,2>*,std::array<graph_snapshot::Node,2>*);
uint32_t cvr_sxb_compose(const RustGraph*,const std::array<graph_snapshot::Node,2>*,const RustFrozen*,const RustFrozen*,
    const std::array<uint32_t,2>*,size_t,const std::array<uint32_t,2>*,size_t,RustMappings**,const Mapping**,size_t*);
void cvr_sxb_free(RustMappings*);
bool cvr_sxb_matches_declaration(const RustFrozen*,const RustFrozen*,uint64_t,const RustGraph*,const RustGraph*,uint64_t);
}
}
#endif

// Exact graph topology expansion only. The caller separately owns both section
// definitions and proves their child coverage, native consumers and lifetimes.
class Plan {
public:
#if CVR_RUST_VRIK
    bool Build(const graph_snapshot::Snapshot& source,uint64_t base,uint32_t selectedHandle,
               const std::array<Definition,2>& definitions)try{
        *this={};const auto lent=detail::Lend(source);detail::RustExpansion out{};
        const auto held=detail::cvr_sx_build(&lent,base,selectedHandle,&definitions,&out);
        failure_=Failure(out.failure);if(!held)return false;
        struct Release{detail::RustHeld* held;~Release(){detail::cvr_sx_free(held);}}release{held};
        graph_snapshot::Snapshot expanded;expanded.failure=out.expanded.failure;
        expanded.nodes.assign(out.expanded.nodes,out.expanded.nodes+out.expanded.nodeCount);
        expanded.edges.assign(out.expanded.edges,out.expanded.edges+out.expanded.edgeCount);
        std::array<paired_frontier::Edges,2> expandedEdges;
        for(uint32_t phase=0;phase<2;++phase)expandedEdges[phase]=paired_frontier::detail::Unflat({out.edges[phase],out.edgeCounts[phase]});
        source_=source;expanded_=std::move(expanded);mappings_.assign(out.mappings,out.mappings+out.mappingCount);
        edges_=std::move(expandedEdges);definitions_=definitions;selected_=selectedHandle;tail_=out.tail;
        base_=base;ready_=true;failure_=Failure::None;return true;
    }catch(...){*this={};failure_=Failure::Allocation;return false;}
#else
    bool Build(const graph_snapshot::Snapshot& source,uint64_t base,uint32_t selectedHandle,
               const std::array<Definition,2>& definitions)try{
        *this={};failure_=Failure::Source;
        if(!base || base>UINT64_MAX-0x3094850 || source.failure || source.nodes.empty() || source.nodes.size()>=220)return false;
        std::array<paired_frontier::Edges,2> originalEdges;
        if(!ReadEdges(source,originalEdges) || !Adjacency(source,originalEdges))return false;
        OwnedNativeGraph original;
        if(!original.Create(source,originalEdges) || !original.CompileReference() || !original.SameSourceOrders())return false;
        const auto found=std::find_if(source.nodes.begin(),source.nodes.end(),[&](const auto& node){return node.handle==selectedHandle;});
        failure_=Failure::Identity;
        if(found==source.nodes.end() || found->vtable!=base+0x3094850 || found->work!=base+0x332D5C ||
            graph_snapshot::Field<uint64_t>(found->definition,0)!=found->vtable ||
            graph_snapshot::Field<uint64_t>(found->definition,24)!=paired_frontier::Name("GBuffer_Discard") ||
            graph_snapshot::Field<uint32_t>(found->bytes,24)>1)return false;
        const auto sourceObject=graph_snapshot::Field<uint64_t>(found->bytes,16);
        if(!sourceObject || (sourceObject&7) || sourceObject>UINT64_MAX-56)return false;
        failure_=Failure::Definitions;
        for(const auto& definition:definitions){
            if(!definition.object || (definition.object&7) || definition.object>UINT64_MAX-56 ||
                definition.object==sourceObject || definition.prefix!=found->definition)return false;
            for(const auto& node:source.nodes){const auto object=graph_snapshot::Field<uint64_t>(node.bytes,16);
                const uint64_t bytes=node.vtable==base+0x3094850 && node.work==base+0x332D5C?56:32;
                if(object && Overlap(definition.object,56,object,bytes))return false;}
        }
        if(Overlap(definitions[0].object,56,definitions[1].object,56))return false;
        std::set<uint32_t> ids;
        for(const auto& node:source.nodes)ids.insert(node.handle&0xDFFFFFFFu);
        uint32_t next{};while(ids.contains(next))++next;
        failure_=Failure::Capacity;if(next>=65536)return false;
        const auto originalId=selectedHandle&0xDFFFFFFFu;
        const auto tailHandle=next|(selectedHandle&0x20000000u);
        graph_snapshot::Snapshot expanded;
        std::vector<Mapping> mappings;
        for(const auto& node:source.nodes){
            auto copy=node;
            if(node.handle==selectedHandle){
                Put(copy.bytes,16,definitions[0].object);copy.definition=definitions[0].prefix;
                Append(expanded,copy);mappings.push_back({node.handle,copy.handle,1});
            }else{Append(expanded,copy);mappings.push_back({node.handle,copy.handle,0});}
        }
        auto tail=*found;tail.handle=tailHandle;Put(tail.bytes,16,definitions[1].object);tail.definition=definitions[1].prefix;
        Append(expanded,tail);mappings.push_back({selectedHandle,tailHandle,2});
        std::array<paired_frontier::Edges,2> expandedEdges;
        for(uint32_t phase=0;phase<2;++phase){
            for(const auto& [from,to]:originalEdges[phase])expandedEdges[phase].emplace(from==originalId?next:from,to);
            expandedEdges[phase].emplace(originalId,next);
        }
        failure_=Failure::Compile;
        OwnedNativeGraph compiled;
        if(!compiled.Create(expanded,expandedEdges) || !compiled.CompileReference())return false;
        expanded=compiled.Snapshot();
        failure_=Failure::Contraction;
        auto contracted=expandedEdges;
        for(uint32_t phase=0;phase<2;++phase){
            contracted[phase].clear();
            for(const auto& [from,to]:expandedEdges[phase]){
                if(from==originalId && to==next)continue;
                contracted[phase].emplace(from==next?originalId:from,to==next?originalId:to);
            }
        }
        if(contracted!=originalEdges)return false;
        source_=source;expanded_=std::move(expanded);mappings_=std::move(mappings);
        edges_=std::move(expandedEdges);definitions_=definitions;selected_=selectedHandle;tail_=tailHandle;
        base_=base;ready_=true;failure_=Failure::None;return true;
    }catch(...){*this={};failure_=Failure::Allocation;return false;}
#endif
    bool MatchesSource(const graph_snapshot::Snapshot& source,uint64_t base)const{
        return ready_ && base==base_ && source.failure==source_.failure && source.nodes==source_.nodes && source.edges==source_.edges;
    }
    bool MatchesCompiled(const graph_snapshot::Snapshot& actual)const{
        return ready_ && !actual.failure && actual.nodes==expanded_.nodes && actual.edges==expanded_.edges;
    }
#if CVR_RUST_VRIK
    bool MatchesDeclaration(const graph_snapshot::Snapshot& source,const graph_snapshot::Snapshot& expanded,uint64_t base)const{
        if(!ready_)return false;
        const auto frozenSource=detail::Lend(source_),frozenExpanded=detail::Lend(expanded_),current=detail::Lend(source),
            compiled=detail::Lend(expanded);
        return detail::cvr_sx_matches_declaration(&frozenSource,&frozenExpanded,mappings_.data(),mappings_.size(),&definitions_,selected_,
            base_,&current,&compiled,base);
    }
#else
    bool MatchesDeclaration(const graph_snapshot::Snapshot& source,const graph_snapshot::Snapshot& expanded,uint64_t base)const{
        if(!ready_ || base!=base_ || source.failure || expanded.failure || source.nodes.size()!=source_.nodes.size() ||
            expanded.nodes.size()!=expanded_.nodes.size() || source.edges!=source_.edges || expanded.edges!=expanded_.edges ||
            mappings_.size()!=expanded.nodes.size())return false;
        const auto selected=std::find_if(source_.nodes.begin(),source_.nodes.end(),[&](const auto& node){return node.handle==selected_;});
        if(selected==source_.nodes.end())return false;
        const auto selectedObject=graph_snapshot::Field<uint64_t>(selected->bytes,16);
        for(size_t i=0;i<source.nodes.size();++i){const auto& current=source.nodes[i];const auto& frozen=source_.nodes[i];
            if(!SameNodeStructure(current,frozen))return false;
            const auto object=graph_snapshot::Field<uint64_t>(current.bytes,16);
            // Other-eye aliases of the selected object are the same native
            // definition and cannot carry an independent refreshed prefix.
            if((!object || object==selectedObject) && current.definition!=frozen.definition)return false;
        }
        for(size_t i=0;i<expanded.nodes.size();++i){const auto& current=expanded.nodes[i];const auto& frozen=expanded_.nodes[i];
            const auto& mapping=mappings_[i];
            if(!SameNodeStructure(current,frozen) || mapping.executionHandle!=current.handle)return false;
            const auto original=std::find_if(source.nodes.begin(),source.nodes.end(),[&](const auto& node){return node.handle==mapping.sourceHandle;});
            if(original==source.nodes.end())return false;
            if(!mapping.section){
                if(current.handle!=original->handle || graph_snapshot::Field<uint64_t>(current.bytes,16)!=graph_snapshot::Field<uint64_t>(original->bytes,16) ||
                    current.definition!=original->definition)return false;
            }else{
                if(mapping.section>definitions_.size() || mapping.sourceHandle!=selected_ ||
                    current.definition!=frozen.definition || current.definition!=definitions_[mapping.section-1].prefix ||
                    graph_snapshot::Field<uint64_t>(current.bytes,16)!=definitions_[mapping.section-1].object)return false;
            }
        }
        return true;
    }
#endif
    bool Ready()const{return ready_;}
    Failure Rejection()const{return failure_;}
    const graph_snapshot::Snapshot& Source()const{return source_;}
    const std::array<Definition,2>& Definitions()const{return definitions_;}
    const graph_snapshot::Snapshot& Expanded()const{return expanded_;}
    const std::array<paired_frontier::Edges,2>& Edges()const{return edges_;}
    const std::vector<Mapping>& Mappings()const{return mappings_;}
    uint32_t PrefixHandle()const{return selected_;}
    uint32_t TailHandle()const{return tail_;}
    bool ExecutionPermitted()const{return false;}
#if CVR_RUST_VRIK
    detail::RustFrozen Frozen()const{
        return {detail::Lend(source_),detail::Lend(expanded_),mappings_.data(),mappings_.size(),&definitions_,selected_,tail_,base_,ready_};
    }
#endif
private:
#if !CVR_RUST_VRIK
    static bool SameNodeStructure(const graph_snapshot::Node& current,const graph_snapshot::Node& frozen){
        return current.handle==frozen.handle && current.order==frozen.order && current.bytes==frozen.bytes &&
            current.vtable==frozen.vtable && current.work==frozen.work &&
            graph_snapshot::Field<uint64_t>(current.definition,0)==graph_snapshot::Field<uint64_t>(frozen.definition,0);
    }
    template<class T,size_t N>static void Put(std::array<uint8_t,N>& bytes,size_t offset,T value){std::memcpy(bytes.data()+offset,&value,sizeof(value));}
    static bool Overlap(uint64_t a,uint64_t aBytes,uint64_t b,uint64_t bBytes){
        return a>UINT64_MAX-aBytes || b>UINT64_MAX-bBytes || (a<b+bBytes && b<a+aBytes);
    }
    static void Append(graph_snapshot::Snapshot& destination,graph_snapshot::Node node){
        node.order=uint32_t(destination.nodes.size());Put(node.bytes,0,node.order);destination.nodes.push_back(node);
    }
    static bool ReadEdges(const graph_snapshot::Snapshot& source,std::array<paired_frontier::Edges,2>& edges){
        if(source.edges.size()>65534)return false;
        std::set<uint32_t> seen;
        for(size_t i=0;i<source.edges.size();++i){const auto& edge=source.edges[i];
            const auto id=edge.handle&0xBFFFFFFFu,phase=graph_snapshot::Field<uint32_t>(edge.bytes,4);
            if(edge.order!=i || id>=262144 || (edge.handle!=id && edge.handle!=(id|0x40000000u)) || !seen.insert(id).second ||
                graph_snapshot::Field<uint32_t>(edge.bytes,0)!=edge.order || phase>1)return false;
            const auto from=graph_snapshot::Field<uint32_t>(edge.bytes,8),to=graph_snapshot::Field<uint32_t>(edge.bytes,12);
            if(from!=((from&0xDFFFFFFFu)|0x20000000u) || to!=((to&0xDFFFFFFFu)|0x20000000u) ||
                !edges[phase].emplace(from&0xDFFFFFFFu,to&0xDFFFFFFFu).second)return false;
        }
        return true;
    }
    static bool Adjacency(const graph_snapshot::Snapshot& source,const std::array<paired_frontier::Edges,2>& edges){
        std::map<uint32_t,const graph_snapshot::Edge*> byId;
        for(const auto& edge:source.edges)byId.emplace(edge.handle&0xBFFFFFFFu,&edge);
        std::array<std::set<uint32_t>,4> visited;
        for(const auto& node:source.nodes)for(uint32_t phase=0;phase<2;++phase)for(uint32_t direction=0;direction<2;++direction){
            auto handle=graph_snapshot::Field<uint32_t>(node.bytes,(direction?40:32)+phase*4);
            while(handle!=0x80000000u){
                const auto id=handle&0xBFFFFFFFu;
                const auto found=byId.find(id);
                if(handle!=(id|0x40000000u) || found==byId.end() || !visited[2*phase+direction].insert(id).second)return false;
                const auto& edge=*found->second;
                if(graph_snapshot::Field<uint32_t>(edge.bytes,4)!=phase ||
                    (graph_snapshot::Field<uint32_t>(edge.bytes,direction?8:12)&0xDFFFFFFFu)!=(node.handle&0xDFFFFFFFu))return false;
                handle=graph_snapshot::Field<uint32_t>(edge.bytes,direction?16:20);
            }
        }
        for(uint32_t phase=0;phase<2;++phase)
            if(visited[2*phase]!=visited[2*phase+1] || visited[2*phase].size()!=edges[phase].size())return false;
        return true;
    }
#endif
    bool ready_{};Failure failure_=Failure::None;uint64_t base_{};
    uint32_t selected_{},tail_{};
    graph_snapshot::Snapshot source_,expanded_;
    std::array<Definition,2> definitions_{};
    std::array<paired_frontier::Edges,2> edges_;
    std::vector<Mapping> mappings_;
};
}
