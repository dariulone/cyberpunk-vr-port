#pragma once
#include "Render/NativeStageDefinition.hpp"
#include <type_traits>

namespace cvr::stereo::scene_sections {
using ReadMemory=definition_tree::ReadMemory;
using Header=std::array<uint8_t,56>;
inline constexpr uint32_t VtableRva=0x3094850,WorkRva=0x332D5C,ChildCount=11,PrefixChildren=6,TailChildren=5;
inline constexpr uint32_t NoIndex=UINT32_MAX;
constexpr uint64_t Name(const char* value){
    uint64_t hash=14695981039346656037ull;for(;*value;++value)hash=(hash^uint8_t(*value))*1099511628211ull;return hash;
}
struct Child {
    uint64_t object{},controlBlock{},vtable{},work{},destructor{};
    uint32_t slot{},capturedBytes{};
    std::array<uint8_t,32> rawPrefix{};
    bool operator==(const Child&)const=default;
};
static_assert(sizeof(Child)==80 && std::is_trivially_copyable_v<Child>);
enum class Failure:uint32_t {None,Arguments,Graph,Tree,Root,Array,Bounds,Handle,Child,Pattern,Stages,Changed,Read,Allocation};
namespace detail {
struct Profile {uint32_t vtable,work,destructor,bytes,parameter;};
// Constructors 9852F4/9844DC/1505BC0/984540/985354 explicitly allocate
// 32/32/24/32/32 bytes. No guessed fourth QWORD is read from the 24-byte child.
inline constexpr Profile Setup{0x36D1CD8,0x33BD84,0x8E7E38,32,7};
inline constexpr Profile Bind{0x36D1858,0xA00830,0x8E7E38,32,0x200010};
inline constexpr Profile Render{0x30947B8,0x33DDA4,0x8EA4E0,32,UINT32_MAX};
inline constexpr Profile Special{0x36D1708,0x7F6A60,0x8EA620,24,UINT32_MAX};
inline constexpr Profile Unbind{0x36D1888,0x41ADE8,0x8E7E38,32,0x200010};
inline constexpr Profile End{0x36D1C78,0x33D510,0x8E7E38,32,7};
inline constexpr std::array<Profile,ChildCount> Pattern{Setup,Bind,Render,Special,Unbind,End,Setup,Bind,Render,Unbind,End};
inline bool Address(uint64_t at,uint64_t bytes){return at && !(at&7) && at<=UINT64_MAX-bytes;}
template<class T,size_t N>T Field(const std::array<uint8_t,N>& bytes,size_t offset){return graph_snapshot::Field<T>(bytes,offset);}
#if CVR_RUST_VRIK
// The rules: rust/crates/cvr-render/src/scene_sections.rs (tools/single_pass_tests/native_scene_sections_test runs its
// cases against them in its Rust variant). Capture keeps its order of reads and only orchestrates them; the profiles
// above stay for the fixtures.
struct RustObservation {Header header;definition_tree::NativeHandle handles[ChildCount];Child children[ChildCount];};
struct RustCapture {RustObservation observation;uint32_t root,failure;};
static_assert(sizeof(definition_tree::NativeHandle)==16 && sizeof(RustObservation)==1112 && sizeof(RustCapture)==1120,
    "mirrored by scene_sections.rs");
extern "C" {
bool cvr_ss_capture(const graph_snapshot::Node*,const definition_tree::RustView*,bool,uint64_t,bool,ReadMemory,RustCapture*);
bool cvr_ss_recheck(const graph_snapshot::Node*,const definition_tree::RustView*,bool,uint64_t,bool,uint32_t,ReadMemory,
    const RustObservation*,uint32_t*);
bool cvr_ss_render(const stage_definition::Header*,const stage_definition::Stage*,size_t,bool,uint64_t,bool,const std::array<uint8_t,32>*);
}
// detail::Stages alone: the header against its own prefix.
inline bool Stages(const stage_definition::Snapshot& snapshot,uint64_t base,bool late){
    std::array<uint8_t,32> prefix{};std::copy_n(snapshot.RawHeader().begin(),prefix.size(),prefix.begin());
    return cvr_ss_render(&snapshot.RawHeader(),snapshot.Stages().data(),snapshot.Stages().size(),snapshot.Ready(),base,late,&prefix);
}
#else
inline bool Stages(const stage_definition::Snapshot& snapshot,uint64_t base,bool late){
    if(!snapshot.Ready() || snapshot.Stages().size()!=(late?4u:2u))return false;
    const auto& header=snapshot.RawHeader();
    // 2.5: the stage callbacks are sub_1422749E0 / sub_142274AE0 (2.31 0x1134C40 / 0x1D41BE0); the stage object
    // they are allocated with grew from 0x40 to 0x48 bytes, so the header checks below are 2.31's until re-audited.
    if(Field<uint64_t>(header,0x18)!=base+(late?0x22749E0:0x2274AE0) ||
        Field<uint64_t>(header,0x20) || Field<uint64_t>(header,0x28))return false;
    for(size_t i=0;i<snapshot.Stages().size();++i){const auto& entry=snapshot.Stages()[i];
        const uint32_t stage=late?(i<2?9u:8u):7u;
        if(Field<uint64_t>(entry,0) || Field<uint32_t>(entry,8)!=(i%2?17u:33u) || entry[0xC]!=(i%2?1:0) ||
            Field<uint32_t>(entry,0x10)!=stage || entry[0x14]!=(i%2?0:2) ||
            Field<uint32_t>(entry,0x18) || Field<uint32_t>(entry,0x1C) || entry[0x20] ||
            Field<uint32_t>(entry,0x24)!=UINT32_MAX || Field<uint32_t>(entry,0x28) || Field<uint32_t>(entry,0x2C))return false;
    }
    return true;
}
#endif
}
// Source-bound section PLAN, not a native clone or execution receipt. Exact
// ordered handles preserve multiplicity, including shared setup objects. The
// caller must hold original definition owners throughout Capture/MatchesBefore.
class Snapshot {
public:
#if CVR_RUST_VRIK
    bool Capture(const graph_snapshot::Node& graph,const definition_tree::Snapshot& tree,ReadMemory read)try{
        *this={};
        const auto view=tree.RustRecords();const auto base=tree.ImageBase();detail::RustCapture seen{};
        if(!detail::cvr_ss_capture(&graph,&view,tree.Ready(),base,tree.LayoutGuarded(),read,&seen)){failure_=Failure(seen.failure);return false;}
        const auto& node=tree.Nodes()[seen.root];
        std::array<stage_definition::Snapshot,2> render;
        for(uint32_t i=0;i<2;++i){const auto slot=i?8u:2u;const auto& handle=tree.Handles()[node.firstHandle+slot];
            failure_=Failure::Stages;
            if(!render[i].Capture(graph,tree,handle.child,read) ||
                !detail::cvr_ss_render(&render[i].RawHeader(),render[i].Stages().data(),render[i].Stages().size(),render[i].Ready(),base,i!=0,
                    &seen.observation.children[slot].rawPrefix))return false;
        }
        uint32_t failure{};
        const auto same=detail::cvr_ss_recheck(&graph,&view,tree.Ready(),base,tree.LayoutGuarded(),seen.root,read,&seen.observation,&failure);
        failure_=Failure(failure);
        if(!same || !render[0].MatchesBefore(read) || !render[1].MatchesBefore(read))return false;
        graph_=graph;observed_.header=seen.observation.header;
        observed_.handles.assign(std::begin(seen.observation.handles),std::end(seen.observation.handles));
        observed_.children.assign(std::begin(seen.observation.children),std::end(seen.observation.children));render_=std::move(render);
        tree_=std::make_shared<const definition_tree::Snapshot>(tree);ready_=true;failure_=Failure::None;return true;
    }catch(...){*this={};failure_=Failure::Allocation;return false;}
#else
    bool Capture(const graph_snapshot::Node& graph,const definition_tree::Snapshot& tree,ReadMemory read)try{
        *this={};failure_=Failure::Arguments;
        if(!read || !tree.Ready() || !tree.ImageBase() || tree.ImageBase()>UINT64_MAX-0x3BCDCA8)return false;
        const auto base=tree.ImageBase();const auto object=detail::Field<uint64_t>(graph.bytes,16);const auto id=graph.handle&0xDFFFFFFFu;
        failure_=Failure::Graph;
        if(id>=65536 || (graph.handle!=id && graph.handle!=(id|0x20000000u)) ||
            graph.order!=detail::Field<uint32_t>(graph.bytes,0) || detail::Field<uint32_t>(graph.bytes,24)>1 ||
            graph.vtable!=base+VtableRva || graph.work!=base+WorkRva ||
            detail::Field<uint64_t>(graph.definition,0)!=graph.vtable ||
            detail::Field<uint64_t>(graph.definition,24)!=Name("GBuffer_Discard") || !detail::Address(object,56))return false;
        failure_=Failure::Tree;if(!tree.MatchesBefore(read))return false;
        uint32_t root=NoIndex;
        for(const auto& value:tree.Roots())if(value.object==object && value.node<tree.Nodes().size()){
            if(root!=NoIndex)return false;root=value.node;
        }
        failure_=Failure::Root;
        if(root==NoIndex || tree.Nodes()[root].object!=object || tree.Nodes()[root].kind!=definition_tree::Kind::SceneDriver ||
            tree.Nodes()[root].vtable!=graph.vtable || tree.Nodes()[root].work!=graph.work ||
            tree.Nodes()[root].destructor!=base+definition_tree::SceneDriverDestructorRva)return false;
        const auto& node=tree.Nodes()[root];failure_=Failure::Array;
        if(node.array>=tree.Arrays().size())return false;const auto& array=tree.Arrays()[node.array];
        if(array.node!=root || array.headerAddress!=object+0x28 || array.stride!=16 || !array.data ||
            array.count>array.capacity || array.capacity>definition_tree::MaxArrayCapacity ||
            node.handleCount!=array.count || node.firstHandle==NoIndex || node.firstHandle>tree.Handles().size() ||
            node.handleCount>tree.Handles().size()-node.firstHandle)return false;
        failure_=Failure::Pattern;if(array.count!=ChildCount)return false;
        failure_=Failure::Bounds;if(!detail::Address(array.data,uint64_t(array.count)*16))return false;
        Observation first,second;
        if(!ReadOnce(graph,tree,root,read,first) || !ReadOnce(graph,tree,root,read,second))return false;
        failure_=Failure::Changed;if(first!=second)return false;
        std::array<stage_definition::Snapshot,2> render;
        for(uint32_t i=0;i<2;++i){const auto slot=i?8u:2u;const auto& handle=tree.Handles()[node.firstHandle+slot];
            failure_=Failure::Stages;
            if(!render[i].Capture(graph,tree,handle.child,read) || !detail::Stages(render[i],base,i!=0) ||
                !std::equal(second.children[slot].rawPrefix.begin(),second.children[slot].rawPrefix.end(),render[i].RawHeader().begin()))return false;
        }
        failure_=Failure::Changed;Observation final;
        if(!tree.MatchesBefore(read) || !ReadOnce(graph,tree,root,read,final) || final!=second ||
            !render[0].MatchesBefore(read) || !render[1].MatchesBefore(read))return false;
        graph_=graph;observed_=std::move(second);render_=std::move(render);
        tree_=std::make_shared<const definition_tree::Snapshot>(tree);ready_=true;failure_=Failure::None;return true;
    }catch(...){*this={};failure_=Failure::Allocation;return false;}
#endif
    bool MatchesBefore(ReadMemory read)const{
        if(!ready_ || !tree_)return false;Snapshot current;
        return current.Capture(graph_,*tree_,read) && current.observed_==observed_ &&
            current.render_[0].RawHeader()==render_[0].RawHeader() && current.render_[0].Stages()==render_[0].Stages() &&
            current.render_[1].RawHeader()==render_[1].RawHeader() && current.render_[1].Stages()==render_[1].Stages();
    }
    bool Ready()const{return ready_;}
    Failure Rejection()const{return failure_;}
    const graph_snapshot::Node& RootIdentity()const{return graph_;}
    const Header& RawHeader()const{return observed_.header;}
    const std::vector<definition_tree::NativeHandle>& Handles()const{return observed_.handles;}
    const std::vector<Child>& Children()const{return observed_.children;}
    const std::array<stage_definition::Snapshot,2>& RenderElements()const{return render_;}
    uint32_t SplitIndex()const{return ready_?PrefixChildren:NoIndex;}
    uint32_t PrefixCount()const{return ready_?PrefixChildren:0;}
    uint32_t TailCount()const{return ready_?TailChildren:0;}
    bool ExecutionPermission()const{return false;}
private:
    struct Observation {
        Header header{};std::vector<definition_tree::NativeHandle> handles;std::vector<Child> children;
        bool operator==(const Observation&)const=default;
    };
#if !CVR_RUST_VRIK
    bool ReadOnce(const graph_snapshot::Node& graph,const definition_tree::Snapshot& tree,uint32_t root,ReadMemory read,Observation& out){
        out={};const auto base=tree.ImageBase();const auto& node=tree.Nodes()[root];const auto& array=tree.Arrays()[node.array];
        failure_=Failure::Read;
        if(!read(node.object,out.header.data(),out.header.size()))return false;
        failure_=Failure::Root;
        if(!std::equal(graph.definition.begin(),graph.definition.end(),out.header.begin()) ||
            detail::Field<uint64_t>(out.header,0)!=graph.vtable || detail::Field<uint64_t>(out.header,24)!=Name("GBuffer_Discard") ||
            detail::Field<uint32_t>(out.header,32)!=1 || detail::Field<uint64_t>(out.header,40)!=array.data ||
            detail::Field<uint32_t>(out.header,48)!=array.capacity || detail::Field<uint32_t>(out.header,52)!=array.count)return false;
        out.handles.resize(ChildCount);out.children.reserve(ChildCount);failure_=Failure::Read;
        if(!read(array.data,out.handles.data(),ChildCount*sizeof(definition_tree::NativeHandle)))return false;
        for(uint32_t slot=0;slot<ChildCount;++slot){const auto& handle=out.handles[slot];const auto& captured=tree.Handles()[node.firstHandle+slot];
            failure_=Failure::Handle;
            if(!handle.object || !handle.controlBlock || handle.object!=captured.object || handle.controlBlock!=captured.controlBlock ||
                captured.slot!=slot || captured.parent!=root || captured.address!=array.data+uint64_t(slot)*16 ||
                captured.child>=tree.Nodes().size())return false;
            const auto& child=tree.Nodes()[captured.child];const auto& expected=detail::Pattern[slot];
            failure_=Failure::Child;
            if(child.object!=handle.object || child.controlBlock!=handle.controlBlock || child.vtable!=base+expected.vtable ||
                child.work!=base+expected.work || child.destructor!=base+expected.destructor ||
                !detail::Address(child.object,expected.bytes))return false;
            Child row{child.object,child.controlBlock,child.vtable,child.work,child.destructor,slot,expected.bytes,{}};
            uint64_t vtable{},destructor{},work{};failure_=Failure::Read;
            if(!read(row.object,row.rawPrefix.data(),row.capturedBytes) || !read(row.object,&vtable,8) ||
                !detail::Address(vtable,16) || !read(vtable,&destructor,8) || !read(vtable+8,&work,8))return false;
            failure_=Failure::Changed;
            if(vtable!=row.vtable || destructor!=row.destructor || work!=row.work || detail::Field<uint64_t>(row.rawPrefix,0)!=vtable)return false;
            failure_=Failure::Pattern;
            if(expected.parameter!=UINT32_MAX && detail::Field<uint32_t>(row.rawPrefix,24)!=expected.parameter)return false;
            out.children.push_back(row);
        }
        return true;
    }
#endif
    graph_snapshot::Node graph_;Observation observed_;
    std::array<stage_definition::Snapshot,2> render_;
    std::shared_ptr<const definition_tree::Snapshot> tree_;
    bool ready_{};Failure failure_=Failure::None;
};
}
