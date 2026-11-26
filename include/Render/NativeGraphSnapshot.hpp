#pragma once
#include <array>
#include <cstdint>
#include <cstring>
#include <unordered_set>
#include <vector>

namespace cvr::stereo::graph_snapshot {
struct Node {uint32_t handle{},order{};std::array<uint8_t,48> bytes{};uint64_t vtable{},work{};std::array<uint8_t,32> definition{};bool operator==(const Node&)const=default;};
struct Edge {uint32_t handle{},order{};std::array<uint8_t,24> bytes{};bool operator==(const Edge&)const=default;};
static_assert(sizeof(Node)==104 && sizeof(Edge)==32);
using Read=bool(*)(uint64_t,void*,size_t);
template<class T,size_t N>T Field(const std::array<uint8_t,N>& bytes,size_t offset){T value{};std::memcpy(&value,bytes.data()+offset,sizeof(value));return value;}
#if CVR_RUST_VRIK
// The capture: rust/crates/cvr-render/src/native/graph_snapshot.rs through graph_snapshot_abi.rs
// (tools/single_pass_tests/graph_snapshot_test runs its cases against it in its Rust variant).
struct RustGraph;
struct RustGraphView {const Node* nodes;size_t nodeCount;const Edge* edges;size_t edgeCount;};
extern "C" {
RustGraph* cvr_gs_capture(uint64_t,Read,RustGraphView*,uint32_t*);
void cvr_gs_free(RustGraph*);
}
#endif
struct Snapshot {
    std::vector<Node> nodes;
    std::vector<Edge> edges;
    uint32_t failure{};
#if CVR_RUST_VRIK
    bool Capture(uint64_t graph,Read read){
        nodes.clear();edges.clear();failure=1;
        RustGraphView view{};const auto captured=cvr_gs_capture(graph,read,&view,&failure);
        if(!captured)return false;
        struct Release{RustGraph* graph;~Release(){cvr_gs_free(graph);}}release{captured};
        // The records read so far stay, as the C++ rules leave them on a refusal.
        nodes.assign(view.nodes,view.nodes+view.nodeCount);edges.assign(view.edges,view.edges+view.edgeCount);
        return !failure;
    }
#else
    bool Capture(uint64_t graph,Read read){
        nodes.clear();edges.clear();failure=1;
        std::array<uint8_t,0x70> header{};
        if(!read || !graph || !read(graph,header.data(),header.size()))return false;
        const auto count=Field<uint32_t>(header,0x10),edgeCount=Field<uint32_t>(header,0x40);
        const auto nodeBase=Field<uint64_t>(header,0x18),nodeOrder=Field<uint64_t>(header,0x28);
        const auto edgeBase=Field<uint64_t>(header,0x48),edgeOrder=Field<uint64_t>(header,0x58);
        failure=2;if(!count || count>8192 || edgeCount>65536 || !nodeBase || !nodeOrder || (edgeCount && (!edgeBase || !edgeOrder)))return false;
        std::vector<uint32_t> nodeIds(count),edgeIds(edgeCount);
        if(!read(nodeOrder,nodeIds.data(),nodeIds.size()*4) || (edgeCount && !read(edgeOrder,edgeIds.data(),edgeIds.size()*4)))return false;
        std::unordered_set<uint32_t> seen;
        failure=3;
        for(uint32_t i=0;i<count;++i){const auto id=nodeIds[i]&0xDFFFFFFFu;
            if(id>=65536 || !seen.insert(id).second)return false;
            Node node;node.handle=nodeIds[i];node.order=i;
            if(nodeBase>UINT64_MAX-uint64_t(id)*48 || !read(nodeBase+uint64_t(id)*48,node.bytes.data(),node.bytes.size()))return false;
            const auto object=Field<uint64_t>(node.bytes,16);
            if(object && read(object,&node.vtable,8) && node.vtable<=UINT64_MAX-8)read(node.vtable+8,&node.work,8);
            if(object)read(object,node.definition.data(),node.definition.size());
            nodes.push_back(node);
        }
        seen.clear();failure=4;
        for(uint32_t i=0;i<edgeCount;++i){const auto id=edgeIds[i]&0xBFFFFFFFu;
            if(id>=262144 || !seen.insert(id).second)return false;
            Edge edge;edge.handle=edgeIds[i];edge.order=i;
            if(edgeBase>UINT64_MAX-uint64_t(id)*24 || !read(edgeBase+uint64_t(id)*24,edge.bytes.data(),edge.bytes.size()))return false;
            edges.push_back(edge);
        }
        failure=5;
        for(const auto& node:nodes){std::array<uint8_t,48> again{};
            if(!read(nodeBase+uint64_t(node.handle&0xDFFFFFFFu)*48,again.data(),again.size()) || again!=node.bytes)return false;}
        for(const auto& edge:edges){std::array<uint8_t,24> again{};
            if(!read(edgeBase+uint64_t(edge.handle&0xBFFFFFFFu)*24,again.data(),again.size()) || again!=edge.bytes)return false;}
        std::array<uint8_t,0x70> after{};
        std::vector<uint32_t> nodesAfter(count),edgesAfter(edgeCount);
        if(!read(graph,after.data(),after.size()) || after!=header || !read(nodeOrder,nodesAfter.data(),count*4ull) || nodesAfter!=nodeIds ||
            (edgeCount && (!read(edgeOrder,edgesAfter.data(),edgeCount*4ull) || edgesAfter!=edgeIds)))return false;
        failure=0;return true;
    }
#endif
};
}
