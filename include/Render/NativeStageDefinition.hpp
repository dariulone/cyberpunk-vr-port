#pragma once
#include "Render/NativeGraphSnapshot.hpp"
#include "Render/NativeDefinitionTree.hpp"
#include <memory>

namespace cvr::stereo::stage_definition {
using ReadMemory=definition_tree::ReadMemory;
inline constexpr uint32_t WorkRva=0x33DDA4,VtableRva=0x30947B8;
inline constexpr uint32_t HeaderBytes=64,StageBytes=48,MaxStages=definition_tree::MaxArrayCapacity;
using Header=std::array<uint8_t,HeaderBytes>;
using Stage=std::array<uint8_t,StageBytes>;
enum class Failure:uint32_t {None,Arguments,Tree,Graph,Reachability,Identity,Array,Bounds,Read,Changed,Allocation};
#if CVR_RUST_VRIK
// The rules: rust/crates/cvr-render/src/native/stage_definition.rs through definition_tree_abi.rs
// (tools/single_pass_tests/native_stage_definition_test runs its cases against them in its Rust variant).
struct RustStages;
struct RustStageOut {Header header;const Stage* stages;size_t count;uint32_t failure;};
static_assert(sizeof(graph_snapshot::Node)==104 && sizeof(RustStageOut)==88,"mirrored by definition_tree_abi.rs");
extern "C" {
RustStages* cvr_sd_capture(const graph_snapshot::Node*,const definition_tree::RustView*,bool,uint64_t,bool,uint32_t,
    ReadMemory,RustStageOut*);
void cvr_sd_free(RustStages*);
uint32_t cvr_sd_query(const graph_snapshot::Node*,const definition_tree::RustView*,bool,uint64_t,uint32_t,bool);
}
#endif

// Captures native definition bytes only. The caller must hold the original
// definition owner while reading; a copied tree or this snapshot grants no
// lifetime, callback, clone, scheduling, resource-state or execution permission.
class Snapshot {
public:
#if CVR_RUST_VRIK
    static bool Reachable(const graph_snapshot::Node& graph,const definition_tree::Snapshot& tree,uint32_t target)try{
        const auto view=tree.RustRecords();return cvr_sd_query(&graph,&view,tree.Ready(),tree.ImageBase(),target,false)!=0;
    }catch(...){return false;}
    bool Capture(const graph_snapshot::Node& graph,const definition_tree::Snapshot& tree,ReadMemory read){
        return Capture(graph,tree,RootIndex(graph,tree),read);
    }
    bool Capture(const graph_snapshot::Node& graph,const definition_tree::Snapshot& tree,uint32_t target,ReadMemory read)try{
        *this={};const auto view=tree.RustRecords();RustStageOut out{};
        const auto stages=cvr_sd_capture(&graph,&view,tree.Ready(),tree.ImageBase(),tree.LayoutGuarded(),target,read,&out);
        failure_=Failure(out.failure);if(!stages)return false;
        struct Release{RustStages* stages;~Release(){cvr_sd_free(stages);}}release{stages};
        graph_=graph;target_=target;header_=out.header;stages_.assign(out.stages,out.stages+out.count);
        tree_=std::make_shared<const definition_tree::Snapshot>(tree);ready_=true;failure_=Failure::None;return true;
    }catch(...){*this={};failure_=Failure::Allocation;return false;}
#else
    static bool Reachable(const graph_snapshot::Node& graph,const definition_tree::Snapshot& tree,uint32_t target)try{
        if(!GraphShape(graph,tree) || target>=tree.Nodes().size())return false;
        const auto root=RootIndex(graph,tree);if(root==definition_tree::NoIndex)return false;
        std::vector<uint32_t> pending{root};std::vector<bool> seen(tree.Nodes().size());seen[root]=true;
        for(size_t at=0;at<pending.size();++at){
            const auto index=pending[at];if(index==target)return true;
            const auto& node=tree.Nodes()[index];
            if(node.firstHandle==definition_tree::NoIndex){if(node.handleCount)return false;continue;}
            if(node.firstHandle>tree.Handles().size() || node.handleCount>tree.Handles().size()-node.firstHandle)return false;
            for(uint32_t i=0;i<node.handleCount;++i){const auto& handle=tree.Handles()[node.firstHandle+i];
                if(handle.parent!=index)return false;
                if(handle.child==definition_tree::NoIndex){if(handle.object)return false;continue;}
                if(handle.child>=tree.Nodes().size() || handle.object!=tree.Nodes()[handle.child].object)return false;
                if(!seen[handle.child]){seen[handle.child]=true;pending.push_back(handle.child);}
            }
        }
        return false;
    }catch(...){return false;}
    bool Capture(const graph_snapshot::Node& graph,const definition_tree::Snapshot& tree,ReadMemory read){
        return Capture(graph,tree,RootIndex(graph,tree),read);
    }
    bool Capture(const graph_snapshot::Node& graph,const definition_tree::Snapshot& tree,uint32_t target,ReadMemory read)try{
        *this={};failure_=Failure::Arguments;
        if(!read || !GraphShape(graph,tree) || target>=tree.Nodes().size())return false;
        failure_=Failure::Reachability;if(!Reachable(graph,tree,target))return false;
        failure_=Failure::Tree;if(!tree.MatchesBefore(read))return false;
        failure_=Failure::Graph;if(!ReadGraph(graph,read))return false;
        const auto& node=tree.Nodes()[target];const auto base=tree.ImageBase();
        failure_=Failure::Identity;
        if(node.kind!=definition_tree::Kind::RenderElements || node.vtable!=base+VtableRva || node.work!=base+WorkRva ||
            node.destructor!=base+definition_tree::RenderElementsDestructorRva || !Address(node.object,HeaderBytes) ||
            node.array>=tree.Arrays().size())return false;
        const auto& array=tree.Arrays()[node.array];failure_=Failure::Array;
        if(array.node!=target || array.headerAddress!=node.object+0x30 || array.stride!=StageBytes ||
            array.count>array.capacity || array.capacity>MaxStages || !array.data || (array.data&7))return false;
        failure_=Failure::Bounds;
        if(array.count && !Address(array.data,uint64_t(array.count)*StageBytes))return false;
        Header first{},second{};std::vector<Stage> before(array.count),after(array.count);
        failure_=Failure::Read;
        if(!read(node.object,first.data(),first.size()) || !HeaderMatches(first,node,array))return false;
        if(array.count && !read(array.data,before.data(),size_t(array.count)*StageBytes))return false;
        if(!read(node.object,second.data(),second.size()) || !HeaderMatches(second,node,array))return false;
        if(array.count && !read(array.data,after.data(),size_t(array.count)*StageBytes))return false;
        failure_=Failure::Changed;
        if(first!=second || before!=after || !tree.MatchesBefore(read) || !ReadGraph(graph,read))return false;
        Header finalHeader{};
        if(!read(node.object,finalHeader.data(),finalHeader.size()) || finalHeader!=second)return false;
        graph_=graph;target_=target;header_=second;stages_=std::move(after);
        tree_=std::make_shared<const definition_tree::Snapshot>(tree);ready_=true;failure_=Failure::None;return true;
    }catch(...){*this={};failure_=Failure::Allocation;return false;}
#endif
    bool MatchesBefore(ReadMemory read)const{
        if(!ready_ || !tree_)return false;Snapshot current;
        return current.Capture(graph_,*tree_,target_,read) && current.header_==header_ && current.stages_==stages_;
    }
    bool Ready()const{return ready_;}
    Failure Rejection()const{return failure_;}
    const Header& RawHeader()const{return header_;}
    const std::vector<Stage>& Stages()const{return stages_;}
    uint32_t TreeIndex()const{return target_;}
    uint64_t Object()const{return ready_?tree_->Nodes()[target_].object:0;}
    bool ExecutionPermission()const{return false;}
private:
#if CVR_RUST_VRIK
    static uint32_t RootIndex(const graph_snapshot::Node& graph,const definition_tree::Snapshot& tree){
        const auto view=tree.RustRecords();return cvr_sd_query(&graph,&view,tree.Ready(),tree.ImageBase(),0,true);
    }
#else
    static bool Address(uint64_t address,uint64_t bytes){return address && !(address&7) && address<=UINT64_MAX-bytes;}
    static bool GraphShape(const graph_snapshot::Node& graph,const definition_tree::Snapshot& tree){
        const auto base=tree.ImageBase();const auto id=graph.handle&0xDFFFFFFFu;
        return tree.Ready() && base && base<=UINT64_MAX-0x3BCDCA8 && id<65536 &&
            (graph.handle==id || graph.handle==(id|0x20000000u)) && graph.order==graph_snapshot::Field<uint32_t>(graph.bytes,0) &&
            graph_snapshot::Field<uint32_t>(graph.bytes,24)<2 && graph.vtable && graph.work &&
            graph_snapshot::Field<uint64_t>(graph.definition,0)==graph.vtable &&
            Address(graph_snapshot::Field<uint64_t>(graph.bytes,16),32);
    }
    static uint32_t RootIndex(const graph_snapshot::Node& graph,const definition_tree::Snapshot& tree){
        if(!tree.Ready())return definition_tree::NoIndex;
        const auto object=graph_snapshot::Field<uint64_t>(graph.bytes,16);
        for(const auto& root:tree.Roots())if(root.object==object && root.node<tree.Nodes().size()){
            const auto& node=tree.Nodes()[root.node];
            if(node.object==object && node.work==graph.work && node.vtable==graph.vtable)return root.node;
        }
        return definition_tree::NoIndex;
    }
    static bool ReadGraph(const graph_snapshot::Node& graph,ReadMemory read){
        std::array<uint8_t,32> definition{};uint64_t work{};
        return Address(graph.vtable,16) && read(graph_snapshot::Field<uint64_t>(graph.bytes,16),definition.data(),definition.size()) &&
            definition==graph.definition && read(graph.vtable+8,&work,8) && work==graph.work;
    }
    static bool HeaderMatches(const Header& header,const definition_tree::Node& node,const definition_tree::Array& array){
        return graph_snapshot::Field<uint64_t>(header,0)==node.vtable && graph_snapshot::Field<uint64_t>(header,0x30)==array.data &&
            graph_snapshot::Field<uint32_t>(header,0x38)==array.capacity && graph_snapshot::Field<uint32_t>(header,0x3C)==array.count;
    }
#endif
    bool ready_{};Failure failure_=Failure::None;uint32_t target_=definition_tree::NoIndex;
    graph_snapshot::Node graph_;Header header_{};std::vector<Stage> stages_;
    std::shared_ptr<const definition_tree::Snapshot> tree_;
};
// Definition header: +0x18 bool gate(context), +0x20 before(context,span),
// +0x28 after(context), +0x30 NativeVector16<48-byte stage>. Before receives the
// FULL stage list deduplicated by {DWORD+0x10,BYTE+0x14}, not a sliced prefix.
// Stage bytes: callback+0, flags+8, mode+0xC, stage+0x10, technique+0x14,
// filters+0x18/+0x1C, state+0x20, prebuilt-list+0x24, partition+0x28/+0x2C.
// Unknown/padding bytes and prebuilt partition fields are retained verbatim.
}
