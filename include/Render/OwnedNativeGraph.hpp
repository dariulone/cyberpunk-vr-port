#pragma once
#include "Render/PairedPassFrontier.hpp"
#include <array>
#include <map>
#include <memory>
#include <vector>

namespace cvr::stereo {
// Owns the complete node/order/edge adjacency read by native985D0C/9861D0.
// This is NOT a native allocator pool or a definition owner. Only the audited
// synchronous sorter may mutate it. Async declaration/execution requires a
// separate lease on original unique definition owners and native job consumers.
#if CVR_RUST_VRIK
// The graph: rust/crates/cvr-render/src/owned_native_graph.rs (tools/single_pass_tests/owned_native_graph_test runs its
// cases against it in its Rust variant). Its buffers are Rust's; the header names them.
namespace owned_native_graph_detail {
struct RustGraph;struct RustHeld;
struct RustSnapshot {const graph_snapshot::Node* nodes;size_t nodeCount;const graph_snapshot::Edge* edges;size_t edgeCount;};
extern "C" {
RustGraph* cvr_ong_new();
void cvr_ong_free(RustGraph*);
bool cvr_ong_create(RustGraph*,const graph_snapshot::Node*,size_t,const graph_snapshot::Edge*,size_t,uint32_t,
    const std::array<uint32_t,2>*,size_t,const std::array<uint32_t,2>*,size_t);
const uint8_t* cvr_ong_query(RustGraph*,uint32_t);
RustHeld* cvr_ong_snapshot(const RustGraph*,RustSnapshot*);
void cvr_ong_snapshot_free(RustHeld*);
}
}
class OwnedNativeGraph {
public:
    using Edges=paired_frontier::Edges;
    OwnedNativeGraph():rust_(owned_native_graph_detail::cvr_ong_new()){}
    ~OwnedNativeGraph(){owned_native_graph_detail::cvr_ong_free(rust_);}
    OwnedNativeGraph(const OwnedNativeGraph&)=delete;
    OwnedNativeGraph& operator=(const OwnedNativeGraph&)=delete;
    bool Create(const graph_snapshot::Snapshot& source,const std::array<Edges,2>& edges)try{
        const auto cpu=paired_frontier::detail::Flat(edges[0]),order=paired_frontier::detail::Flat(edges[1]);
        return owned_native_graph_detail::cvr_ong_create(rust_,source.nodes.data(),source.nodes.size(),source.edges.data(),
            source.edges.size(),source.failure,cpu.data(),cpu.size(),order.data(),order.size());
    }catch(...){return false;}
    void* SortableData(){return const_cast<uint8_t*>(Query(0));}
    const std::array<uint8_t,0x70>& Header()const{return *reinterpret_cast<const std::array<uint8_t,0x70>*>(Query(1));}
    bool CompileReference(){return Query(2)!=nullptr;}
    bool ValidateCompiled()const{return Query(3)!=nullptr;}
    graph_snapshot::Snapshot Snapshot()const{
        graph_snapshot::Snapshot out;owned_native_graph_detail::RustSnapshot lent{};
        const auto held=owned_native_graph_detail::cvr_ong_snapshot(rust_,&lent);
        if(!held){out.failure=1;return out;}
        struct Release{owned_native_graph_detail::RustHeld* held;~Release(){owned_native_graph_detail::cvr_ong_snapshot_free(held);}}
            release{held};
        out.nodes.assign(lent.nodes,lent.nodes+lent.nodeCount);out.edges.assign(lent.edges,lent.edges+lent.edgeCount);return out;
    }
    bool SameSourceOrders()const{return Query(4)!=nullptr;}
private:
    const uint8_t* Query(uint32_t which)const{return owned_native_graph_detail::cvr_ong_query(rust_,which);}
    owned_native_graph_detail::RustGraph* rust_;
};
#else
class OwnedNativeGraph {
public:
    using Edges=paired_frontier::Edges;
    OwnedNativeGraph()=default;
    OwnedNativeGraph(const OwnedNativeGraph&)=delete;
    OwnedNativeGraph& operator=(const OwnedNativeGraph&)=delete;
    bool Create(const graph_snapshot::Snapshot& source,const std::array<Edges,2>& edges)try{
        if(source_ || source.failure || source.nodes.empty() || source.nodes.size()>255 || source.edges.size()>65536)return false;
        std::set<uint32_t> ids;uint32_t slots{};
        for(size_t i=0;i<source.nodes.size();++i){const auto& n=source.nodes[i];const auto id=n.handle&0xDFFFFFFFu;
            if(id>=65536 || n.order!=i || graph_snapshot::Field<uint32_t>(n.bytes,0)!=i || !ids.insert(id).second ||
               (n.handle!=id && n.handle!=(id|0x20000000u)))return false;slots=std::max(slots,id+1);}
        if(edges[0].size()+edges[1].size()>65536 || !paired_frontier::detail::Acyclic(ids,edges[0]) ||
           !paired_frontier::detail::Acyclic(ids,edges[1]))return false;
        source_=std::make_shared<const graph_snapshot::Snapshot>(source);edges_=edges;nodes_.resize(slots);
        for(const auto& n:source.nodes){const auto id=n.handle&0xDFFFFFFFu;nodes_[id]=n.bytes;order_.push_back(n.handle);
            for(const size_t at:{size_t(32),size_t(36),size_t(40),size_t(44)})Put(nodes_[id],at,Empty);}
        for(uint32_t phase=0;phase<2;++phase)for(const auto& [from,to]:edges[phase]){
            if(!ids.contains(from) || !ids.contains(to))return false;
            const auto index=uint32_t(edgeData_.size()),handle=index|0x40000000u;
            edgeData_.push_back({index,phase,from|0x20000000u,to|0x20000000u,
                graph_snapshot::Field<uint32_t>(nodes_[from],40+phase*4),graph_snapshot::Field<uint32_t>(nodes_[to],32+phase*4)});
            edgeOrder_.push_back(handle);Put(nodes_[from],40+phase*4,handle);Put(nodes_[to],32+phase*4,handle);
        }
        Put(header_,16,uint32_t(order_.size()));Put(header_,24,reinterpret_cast<uintptr_t>(nodes_.data()));
        Put(header_,40,reinterpret_cast<uintptr_t>(order_.data()));Put(header_,64,uint32_t(edgeData_.size()));
        Put(header_,72,reinterpret_cast<uintptr_t>(edgeData_.data()));Put(header_,88,reinterpret_cast<uintptr_t>(edgeOrder_.data()));
        constructed_=true;return true;
    }catch(...){return false;}
    void* SortableData(){return constructed_?header_.data():nullptr;}
    const std::array<uint8_t,0x70>& Header()const{return header_;}
    bool CompileReference(){
        if(!constructed_)return false;
        if(order_.size()!=source_->nodes.size())return false;
        for(size_t i=0;i<order_.size();++i)if(order_[i]!=source_->nodes[i].handle || (order_[i]&0xDFFFFFFFu)>=nodes_.size())return false;
        for(uint32_t phase=0;phase<2;++phase){
            const auto ranks=ReferenceRanks(phase);if(ranks.size()!=order_.size())return false;
            for(const auto& [id,rank]:ranks)nodes_[id][28+phase]=uint8_t(rank);
        }
        header_[0x68]=1;return ValidateCompiled();
    }
    bool ValidateCompiled()const{
        if(!constructed_ || !source_ || header_[0x68]!=1 || graph_snapshot::Field<uint32_t>(header_,16)!=order_.size() ||
           graph_snapshot::Field<uint64_t>(header_,24)!=reinterpret_cast<uintptr_t>(nodes_.data()) ||
           graph_snapshot::Field<uint64_t>(header_,40)!=reinterpret_cast<uintptr_t>(order_.data()) ||
           graph_snapshot::Field<uint32_t>(header_,64)!=edgeData_.size() ||
           graph_snapshot::Field<uint64_t>(header_,72)!=reinterpret_cast<uintptr_t>(edgeData_.data()) ||
           graph_snapshot::Field<uint64_t>(header_,88)!=reinterpret_cast<uintptr_t>(edgeOrder_.data()))return false;
        for(size_t i=0;i<source_->nodes.size();++i){const auto& original=source_->nodes[i];const auto id=original.handle&0xDFFFFFFFu;
            if(order_[i]!=original.handle || !std::equal(original.bytes.begin(),original.bytes.begin()+28,nodes_[id].begin()) ||
               nodes_[id][30]!=original.bytes[30] || nodes_[id][31]!=original.bytes[31])return false;}
        for(uint32_t phase=0;phase<2;++phase){const auto ranks=ReferenceRanks(phase);if(ranks.size()!=order_.size())return false;
            for(const auto& [id,rank]:ranks)if(nodes_[id][28+phase]!=rank)return false;}
        std::array<Edges,2> actual;std::array<std::set<uint32_t>,2> outgoingSeen,incomingSeen;
        for(const auto raw:order_){const auto id=raw&0xDFFFFFFFu;
            for(uint32_t phase=0;phase<2;++phase)for(uint32_t direction=0;direction<2;++direction){
                auto handle=graph_snapshot::Field<uint32_t>(nodes_[id],(direction?40:32)+phase*4);std::set<uint32_t> chain;
                while(handle!=Empty){const auto index=handle&0xBFFFFFFFu;
                    if(index>=edgeData_.size() || handle!=(index|0x40000000u) || !chain.insert(index).second)return false;
                    const auto& e=edgeData_[index];if(e[0]!=index || e[1]!=phase || (e[direction?2:3]&0xDFFFFFFFu)!=id)return false;
                    auto& seen=direction?outgoingSeen[phase]:incomingSeen[phase];if(!seen.insert(index).second)return false;
                    if(direction)actual[phase].emplace(e[2]&0xDFFFFFFFu,e[3]&0xDFFFFFFFu);handle=e[direction?4:5];
                }
            }
        }
        if(actual!=edges_)return false;
        for(uint32_t phase=0;phase<2;++phase)if(outgoingSeen[phase]!=incomingSeen[phase] || outgoingSeen[phase].size()!=edges_[phase].size())return false;
        for(size_t i=0;i<edgeOrder_.size();++i)if(edgeOrder_[i]!=(uint32_t(i)|0x40000000u))return false;
        return true;
    }
    graph_snapshot::Snapshot Snapshot()const{
        graph_snapshot::Snapshot out;if(!source_ || !ValidateCompiled()){out.failure=1;return out;}
        out.nodes=source_->nodes;for(auto& n:out.nodes)n.bytes=nodes_[n.handle&0xDFFFFFFFu];
        for(size_t i=0;i<edgeData_.size();++i){graph_snapshot::Edge e;e.handle=edgeOrder_[i];e.order=uint32_t(i);
            std::memcpy(e.bytes.data(),edgeData_[i].data(),24);out.edges.push_back(e);}return out;
    }
    bool SameSourceOrders()const{
        if(!source_ || !ValidateCompiled())return false;
        for(const auto& n:source_->nodes)for(uint32_t phase=0;phase<2;++phase)
            if(nodes_[n.handle&0xDFFFFFFFu][28+phase]!=n.bytes[28+phase])return false;
        return true;
    }
private:
    static constexpr uint32_t Empty=0x80000000u;
    template<class T,size_t N>static void Put(std::array<uint8_t,N>& out,size_t at,T value){std::memcpy(out.data()+at,&value,sizeof(value));}
    std::map<uint32_t,uint32_t> ReferenceRanks(uint32_t phase)const{
        std::map<uint32_t,uint32_t> degree,level,ranks;
        for(const auto raw:order_){const auto id=raw&0xDFFFFFFFu;degree[id]=level[id]=0;}
        for(const auto& [a,b]:edges_[phase]){if(!degree.contains(a) || !degree.contains(b))return {};++degree[b];}
        std::vector<uint32_t> ready;for(const auto& [id,d]:degree)if(!d)ready.push_back(id);
        for(size_t i=0;i<ready.size();++i)for(auto e=edges_[phase].lower_bound({ready[i],0});e!=edges_[phase].end() && e->first==ready[i];++e){
            level[e->second]=std::max(level[e->second],level[e->first]+1);if(!--degree[e->second])ready.push_back(e->second);}
        if(ready.size()!=order_.size())return {};
        std::vector<std::pair<uint32_t,uint32_t>> sorted;
        for(uint32_t ordinal=0;ordinal<order_.size();++ordinal)sorted.emplace_back(level.at(order_[ordinal]&0xDFFFFFFFu),ordinal);
        std::sort(sorted.begin(),sorted.end());for(uint32_t rank=0;rank<sorted.size();++rank)ranks[order_[sorted[rank].second]&0xDFFFFFFFu]=rank;
        return ranks;
    }
    alignas(8) std::array<uint8_t,0x70> header_{};
    bool constructed_{};
    std::shared_ptr<const graph_snapshot::Snapshot> source_;
    std::array<Edges,2> edges_;
    std::vector<std::array<uint8_t,48>> nodes_;
    std::vector<uint32_t> order_,edgeOrder_;
    std::vector<std::array<uint32_t,6>> edgeData_;
};
#endif
}
