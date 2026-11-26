#include "Render/NativeStageDefinition.hpp"
#include <cstdio>
#include <map>
#include <stdexcept>

namespace stage=cvr::stereo::stage_definition;
namespace tree=cvr::stereo::definition_tree;
namespace graph=cvr::stereo::graph_snapshot;
namespace {
constexpr uint64_t Base=0x140000000,Root=0x10000,Child=0x20000,Other=0x21000,Handles=0x30000,Stages=0x40000;
constexpr uint64_t Control=0x50000,ObjectAllocator=0x60000,ControlAllocator=0x61000,ArrayAllocator=0x62000;
constexpr uint64_t RootVtable=Base+0x3094850,ChildVtable=Base+stage::VtableRva;
std::map<uint64_t,uint8_t> memory;
uint64_t mutateAt{},mutateTarget{};uint32_t mutateRead{},seen{};
void Require(bool value){if(!value)throw std::runtime_error("native stage definition assertion failed");}
template<class T>void Put(uint64_t address,const T& value){const auto* data=reinterpret_cast<const uint8_t*>(&value);
    for(size_t i=0;i<sizeof(value);++i)memory[address+i]=data[i];}
template<class T,size_t N>void Field(std::array<uint8_t,N>& bytes,size_t offset,T value){std::memcpy(bytes.data()+offset,&value,sizeof(value));}
bool Read(uint64_t address,void* out,size_t bytes){
    if(!address || address>UINT64_MAX-bytes)return false;auto* data=static_cast<uint8_t*>(out);
    for(size_t i=0;i<bytes;++i){const auto found=memory.find(address+i);if(found==memory.end())return false;data[i]=found->second;}
    if(address==mutateAt && ++seen==mutateRead)memory[mutateTarget]^=1;
    return true;
}
void Fresh(){
    memory.clear();mutateAt=mutateTarget=mutateRead=seen=0;
    Put(Base+tree::ObjectAllocatorRva,ObjectAllocator);Put(Base+tree::ControlAllocatorRva,ControlAllocator);
    Put(ArrayAllocator+0x20,Base+0x985ABC);
    Put(RootVtable,Base+tree::SceneDriverDestructorRva);Put(RootVtable+8,Base+0x332D5C);
    Put(ChildVtable,Base+tree::RenderElementsDestructorRva);Put(ChildVtable+8,Base+stage::WorkRva);
    Put(Root,std::array<uint8_t,0x38>{});Put(Root,RootVtable);
    Put(Root+0x28,tree::NativeVector16{Handles,1,1});Put(Handles,tree::NativeHandle{Child,Control});
    Put(Handles+16,ArrayAllocator);Put(Control,int32_t(1));Put(Control+4,int32_t(1));
    stage::Header header{};Field(header,0,ChildVtable);Field(header,0x18,Base+0x1D41B70);
    Field(header,0x20,Base+0xABC000);Field(header,0x28,Base+0x7875B0);
    Field(header,0x30,Stages);Field(header,0x38,uint32_t(3));Field(header,0x3C,uint32_t(3));Put(Child,header);
    for(uint32_t i=0;i<3;++i){stage::Stage value{};
        for(size_t j=0;j<value.size();++j)value[j]=uint8_t(19*i+j);
        Field(value,0,Base+0x123450+i*8);Field(value,0x10,i==2?9u:7u);value[0x14]=uint8_t(i);
        Field(value,0x24,i==2?17u:UINT32_MAX);Field(value,0x28,i==2?2u:0u);Field(value,0x2C,i==2?5u:1u);
        Put(Stages+uint64_t(i)*48,value);
    }
    Put(Stages+3*48,ArrayAllocator);
}
tree::Snapshot Tree(){tree::Snapshot value;const uint64_t roots[]{Root};Require(value.Capture(roots,Base,Read,true));return value;}
graph::Node Graph(){graph::Node value;value.handle=0x20000007;value.order=2;value.vtable=RootVtable;value.work=Base+0x332D5C;
    Field(value.bytes,0,uint32_t(2));Field(value.bytes,16,Root);Field(value.bytes,24,uint32_t(1));Require(Read(Root,value.definition.data(),32));return value;}
void Positive(){
    Fresh();const auto tree=Tree();const auto graph=Graph();const auto before=memory;stage::Snapshot value;
    Require(stage::Snapshot::Reachable(graph,tree,1) && value.Capture(graph,tree,1,Read) && value.Ready() && !value.ExecutionPermission());
    Require(value.Object()==Child && value.TreeIndex()==1 && value.Stages().size()==3 && value.MatchesBefore(Read) && memory==before);
    Require(graph::Field<uint64_t>(value.RawHeader(),0x20)==Base+0xABC000 &&
        graph::Field<uint32_t>(value.Stages()[2],0x24)==17 && graph::Field<uint32_t>(value.Stages()[2],0x28)==2 &&
        graph::Field<uint32_t>(value.Stages()[2],0x2C)==5);
    Require(!value.Capture(graph,tree,Read) && !value.Ready() && value.Stages().empty());
    // A direct RenderElements graph node remains supported without a child index.
    const uint64_t roots[]{Child};tree::Snapshot directTree;Require(directTree.Capture(roots,Base,Read,true));
    auto direct=graph;direct.vtable=ChildVtable;direct.work=Base+stage::WorkRva;Field(direct.bytes,16,Child);
    Require(Read(Child,direct.definition.data(),32) && value.Capture(direct,directTree,Read));
    auto peer=direct;Field(peer.bytes,24,uint32_t(0));stage::Snapshot second;
    Require(second.Capture(peer,directTree,Read) && second.RawHeader()==value.RawHeader() && second.Stages()==value.Stages());
}
void Mutations(){
    const auto reject=[](auto mutate){Fresh();auto tree=Tree();auto graph=Graph();mutate(graph);
        stage::Snapshot value;Require(!value.Capture(graph,tree,1,Read) && !value.Ready() && value.Stages().empty() && !value.MatchesBefore(Read));};
    reject([](auto& graph){++graph.work;});reject([](auto& graph){++graph.vtable;});
    reject([](auto& graph){Field(graph.bytes,16,Other);});reject([](auto& graph){Field(graph.bytes,24,uint32_t(2));});
    reject([](auto&){Put(ChildVtable+8,Base+stage::WorkRva+1);});
    reject([](auto&){Put(Child+0x38,uint32_t(stage::MaxStages+1));});
    reject([](auto&){Put(Child+0x3C,uint32_t(4));});
    reject([](auto&){Put(Child+0x30,UINT64_MAX-7);});
    reject([](auto&){memory.erase(Stages+47);});
    reject([](auto&){Put(Stages+3*48,ArrayAllocator+8);});
    reject([](auto&){mutateAt=Stages;mutateRead=1;mutateTarget=Stages+48+0x24;});
    reject([](auto&){mutateAt=Stages;mutateRead=1;mutateTarget=Child+0x28;});
    Fresh();auto tree=Tree();auto graph=Graph();stage::Snapshot value;Require(value.Capture(graph,tree,1,Read));
    memory[Stages+48+33]^=1;Require(!value.MatchesBefore(Read));
    Fresh();tree=Tree();graph=Graph();Require(value.Capture(graph,tree,1,Read));memory[Child+0x20]^=1;Require(!value.MatchesBefore(Read));
    Fresh();tree=Tree();graph=Graph();Require(value.Capture(graph,tree,1,Read));Put(Handles,tree::NativeHandle{});Require(!value.MatchesBefore(Read));
}
void Bounds(){
    Fresh();const uint64_t roots[]{Child};Put(Child+0x30,ArrayAllocator);Put(Child+0x38,uint32_t(0));Put(Child+0x3C,uint32_t(0));
    tree::Snapshot tree;Require(tree.Capture(roots,Base,Read,true));auto graph=Graph();Field(graph.bytes,16,Child);
    graph.vtable=ChildVtable;graph.work=Base+stage::WorkRva;Require(Read(Child,graph.definition.data(),32));
    stage::Snapshot value;Require(value.Capture(graph,tree,Read) && value.Stages().empty() && value.MatchesBefore(Read));
    Require(!value.Capture(graph,tree,UINT32_MAX,Read));
    Require(!value.Capture(graph,tree,nullptr));
    Fresh();Put(Other,std::array<uint8_t,64>{});Put(Other,ChildVtable);Put(Other+0x30,ArrayAllocator);
    const uint64_t separateRoots[]{Root,Other};Require(tree.Capture(separateRoots,Base,Read,true));graph=Graph();
    uint32_t unrelated=UINT32_MAX;for(uint32_t i=0;i<tree.Nodes().size();++i)if(tree.Nodes()[i].object==Other)unrelated=i;
    Require(unrelated!=UINT32_MAX && !stage::Snapshot::Reachable(graph,tree,unrelated) && !value.Capture(graph,tree,unrelated,Read));
}
}
int main()try{Positive();Mutations();Bounds();std::puts("PASS bounded native RenderElements header/stage capture and exact rooted identity");return 0;}
catch(const std::exception& error){std::fprintf(stderr,"%s\n",error.what());return 1;}
