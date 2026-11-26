#include "Render/NativeSceneSections.hpp"
#include <cstdio>
#include <map>
#include <stdexcept>

namespace sections=cvr::stereo::scene_sections;
namespace stage=cvr::stereo::stage_definition;
namespace tree=cvr::stereo::definition_tree;
namespace graph=cvr::stereo::graph_snapshot;
namespace {
constexpr uint64_t Base=0x140000000,Root=0x10000,Handles=0x20000,Children=0x30000,Controls=0x40000;
constexpr uint64_t EarlyStages=0x50000,LateStages=0x51000;
constexpr uint64_t ObjectAllocator=0x60000,ControlAllocator=0x61000,ArrayAllocator=0x62000;
constexpr uint64_t RootVtable=Base+sections::VtableRva;
std::map<uint64_t,uint8_t> memory;
uint64_t mutateAt{},mutateTarget{};size_t mutateBytes{};uint32_t mutateRead{},seen{};
void Require(bool value){if(!value)throw std::runtime_error("native scene sections assertion failed");}
template<class T>void Put(uint64_t address,const T& value){const auto* data=reinterpret_cast<const uint8_t*>(&value);
    for(size_t i=0;i<sizeof(value);++i)memory[address+i]=data[i];}
template<class T,size_t N>void Field(std::array<uint8_t,N>& bytes,size_t offset,T value){std::memcpy(bytes.data()+offset,&value,sizeof(value));}
bool Read(uint64_t address,void* out,size_t bytes){
    if(!address || address>UINT64_MAX-bytes)return false;auto* data=static_cast<uint8_t*>(out);
    for(size_t i=0;i<bytes;++i){const auto found=memory.find(address+i);if(found==memory.end())return false;data[i]=found->second;}
    if(address==mutateAt && (!mutateBytes || bytes==mutateBytes) && ++seen==mutateRead)memory[mutateTarget]^=1;
    return true;
}
constexpr uint64_t Object(uint32_t slot){return Children+slot*0x100;}
constexpr uint64_t Control(uint32_t slot){return Controls+slot*0x100;}
void StageArray(uint64_t address,bool late){
    for(uint32_t i=0;i<(late?4u:2u);++i){stage::Stage value{};
        Field(value,8,i%2?17u:33u);value[0xC]=uint8_t(i%2?1:0);
        Field(value,0x10,late?(i<2?9u:8u):7u);value[0x14]=uint8_t(i%2?0:2);
        Field(value,0x24,UINT32_MAX);
        // Native constructors leave padding opaque. It is preserved, not zero-normalized.
        value[0xD]=0xE1;value[0x15]=uint8_t(0x61+i);value[0x21]=0xF3;
        Put(address+uint64_t(i)*48,value);
    }
    Put(address+(late?4u:2u)*48,ArrayAllocator);
}
void Fresh(bool shared=false){
    memory.clear();mutateAt=mutateTarget=mutateBytes=mutateRead=seen=0;
    Put(Base+tree::ObjectAllocatorRva,ObjectAllocator);Put(Base+tree::ControlAllocatorRva,ControlAllocator);
    Put(ArrayAllocator+0x20,Base+0x985ABC);
    Put(RootVtable,Base+tree::SceneDriverDestructorRva);Put(RootVtable+8,Base+sections::WorkRva);
    sections::Header root{};Field(root,0,RootVtable);Field(root,24,sections::Name("GBuffer_Discard"));
    Field(root,32,uint32_t(1));Field(root,40,Handles);Field(root,48,sections::ChildCount);Field(root,52,sections::ChildCount);
    root[36]=0xB7;Put(Root,root);Put(Handles+16*sections::ChildCount,ArrayAllocator);
    for(uint32_t slot=0;slot<sections::ChildCount;++slot){const auto& profile=sections::detail::Pattern[slot];
        const auto vt=Base+profile.vtable;
        Put(vt,Base+profile.destructor);Put(vt+8,Base+profile.work);
        if(slot==2 || slot==8){const bool late=slot==8;stage::Header header{};
            Field(header,0,vt);Field(header,0x18,Base+(late?0x22749E0:0x2274AE0));
            Field(header,0x30,late?LateStages:EarlyStages);Field(header,0x38,late?4u:2u);Field(header,0x3C,late?4u:2u);
            Put(Object(slot),header);StageArray(late?LateStages:EarlyStages,late);
        }else if(profile.bytes==24){std::array<uint8_t,24> child{};Field(child,0,vt);child[9]=0x93;Put(Object(slot),child);
        }else{std::array<uint8_t,32> child{};Field(child,0,vt);Field(child,24,profile.parameter);
            child[9]=0x93;child[16]=slot==1 || slot==7?1:0;child[28]=0xDF;Put(Object(slot),child);
        }
        Put(Handles+16*slot,tree::NativeHandle{Object(slot),Control(slot)});
        Put(Control(slot),int32_t(1));Put(Control(slot)+4,int32_t(1));
    }
    if(shared){
        const std::array<std::array<uint32_t,2>,4> pairs{{{0,6},{1,7},{4,9},{5,10}}};
        for(const auto& pair:pairs){Put(Handles+16*pair[1],tree::NativeHandle{Object(pair[0]),Control(pair[0])});Put(Control(pair[0]),int32_t(2));}
    }
}
tree::Snapshot Tree(){tree::Snapshot value;const uint64_t roots[]{Root};Require(value.Capture(roots,Base,Read,true));return value;}
graph::Node Graph(uint32_t view=0){graph::Node value;value.handle=0x2000001A+view;value.order=2;
    value.vtable=RootVtable;value.work=Base+sections::WorkRva;Field(value.bytes,0,uint32_t(2));Field(value.bytes,16,Root);
    Field(value.bytes,24,view);Require(Read(Root,value.definition.data(),value.definition.size()));return value;
}
void Positive(){
    Fresh();const auto tree=Tree();const auto graph=Graph();const auto before=memory;sections::Snapshot value;
    Require(value.Capture(graph,tree,Read) && value.Ready() && value.Rejection()==sections::Failure::None && value.MatchesBefore(Read));
    Require(!value.ExecutionPermission() && value.SplitIndex()==6 && value.PrefixCount()==6 && value.TailCount()==5);
    Require(value.Handles().size()==11 && value.Children().size()==11 && value.RootIdentity().handle==graph.handle && memory==before);
    Require(value.Children()[3].capturedBytes==24 && !memory.contains(Object(3)+24));
    for(size_t i=24;i<32;++i)Require(value.Children()[3].rawPrefix[i]==0);
    Require(value.RawHeader()[36]==0xB7 && value.Children()[0].rawPrefix[28]==0xDF);
    Require(value.RenderElements()[0].Stages().size()==2 && value.RenderElements()[1].Stages().size()==4);
    Require(graph::Field<uint32_t>(value.RenderElements()[1].Stages()[2],0x10)==8 &&
        graph::Field<uint32_t>(value.RenderElements()[1].Stages()[3],0x10)==8 && value.RenderElements()[1].Stages()[2][0x15]==0x63);
    for(uint32_t slot=0;slot<11;++slot){const auto& row=value.Children()[slot];
        Require(row.slot==slot && row.object==value.Handles()[slot].object && row.controlBlock==value.Handles()[slot].controlBlock);
    }
    // Both graph views can reference the same original SceneDriver and child graph.
    sections::Snapshot other;Require(other.Capture(Graph(1),tree,Read) && other.RawHeader()==value.RawHeader() &&
        other.Handles()==value.Handles() && other.Children()==value.Children());
    Fresh(true);const auto sharedTree=Tree();Require(value.Capture(Graph(),sharedTree,Read));
    Require(value.Handles()[0]==value.Handles()[6] && value.Handles()[1]==value.Handles()[7] && value.Handles()[4]==value.Handles()[9] &&
        value.Handles()[5]==value.Handles()[10] && value.Children().size()==11 && value.MatchesBefore(Read));
    Require(sharedTree.Nodes().size()==8);
}
void GraphAndBounds(){
    const auto reject=[](auto mutate){Fresh();auto tree=Tree();auto graph=Graph();mutate(graph);
        sections::Snapshot value;Require(!value.Capture(graph,tree,Read) && !value.Ready() && value.Children().empty() &&
            value.Handles().empty() && value.SplitIndex()==UINT32_MAX && !value.MatchesBefore(Read) && !value.ExecutionPermission());};
    reject([](auto& graph){++graph.work;});reject([](auto& graph){++graph.vtable;});
    reject([](auto& graph){++graph.order;});reject([](auto& graph){graph.handle=0x80000000;});
    reject([](auto& graph){Field(graph.bytes,24,uint32_t(2));});reject([](auto& graph){Field(graph.bytes,16,UINT64_MAX-7);});
    reject([](auto& graph){Field(graph.definition,24,sections::Name("GBuffer_Other"));});
    reject([](auto&){Put(Root+32,uint32_t(0));});reject([](auto&){Put(Root+48,tree::MaxArrayCapacity+1);});
    reject([](auto&){Put(Root+52,uint32_t(10));});reject([](auto&){Put(Root+52,uint32_t(12));});
    reject([](auto&){Put(Root+40,UINT64_MAX-7);});reject([](auto&){Put(Root+40,Handles+1);});
    reject([](auto&){memory.erase(Root+55);});reject([](auto&){memory.erase(Handles+11*16-1);});
    reject([](auto&){memory.erase(Object(3)+23);});reject([](auto&){Put(RootVtable+8,Base+sections::WorkRva+1);});
    reject([](auto&){Put(Handles+16,tree::NativeHandle{Object(0),Control(0)});});
    reject([](auto&){Put(Control(0),int32_t(0));});reject([](auto&){Put(Handles+11*16,ArrayAllocator+8);});
    Fresh();const auto tree=Tree();sections::Snapshot value;
    Require(!value.Capture(Graph(),tree,nullptr));Require(!value.Capture(Graph(),tree::Snapshot{},Read));
}
void FreshPatternMutations(){
    // Capture the changed native tree first: rejection must come from the section profile,
    // not merely a stale definition-tree snapshot.
    const auto reject=[](auto mutate){Fresh();mutate();const auto tree=Tree();sections::Snapshot value;
        Require(!value.Capture(Graph(),tree,Read) && !value.Ready() && !value.ExecutionPermission());};
    reject([]{Put(Object(0)+24,uint32_t(8));});reject([]{Put(Object(1)+24,uint32_t(0x200011));});
    reject([]{Put(Object(4)+24,uint32_t(0x100010));});reject([]{Put(Object(10)+24,uint32_t(6));});
    reject([]{Put(Root+52,uint32_t(10));});
    reject([]{Put(Handles,tree::NativeHandle{Object(1),Control(1)});Put(Handles+16,tree::NativeHandle{Object(0),Control(0)});});
    reject([]{Put(Handles+3*16,tree::NativeHandle{Object(0),Control(0)});Put(Control(0),int32_t(2));});
    reject([]{Put(Base+sections::detail::Setup.vtable+8,Base+0x1EDD65);});
    reject([]{Put(Base+sections::detail::Special.vtable,Base+0x986799);});
    reject([]{Put(Object(8)+0x18,Base+0x22749E8);});reject([]{Put(Object(2)+0x18,Base+0x2274A70);});
    reject([]{Put(Object(8)+0x20,Base+0x123450);});reject([]{Put(Object(8)+0x28,Base+0x123450);});
    reject([]{Put(LateStages,Base+0x123450);});reject([]{Put(LateStages+8,uint32_t(17));});
    reject([]{Put(LateStages+0xC,uint8_t(1));});reject([]{Put(LateStages+0x14,uint8_t(0));});
    reject([]{Put(LateStages+96+0x10,uint32_t(9));});reject([]{Put(LateStages+0x18,uint32_t(1));});
    reject([]{Put(LateStages+0x1C,uint32_t(1));});reject([]{Put(LateStages+0x20,uint8_t(1));});
    reject([]{Put(LateStages+0x24,uint32_t(3));});reject([]{Put(LateStages+0x28,uint32_t(1));});
    reject([]{Put(LateStages+0x2C,uint32_t(2));});reject([]{Put(EarlyStages+0x10,uint32_t(9));});
    reject([]{Put(Object(8)+0x3C,uint32_t(2));});
}
void Coherence(){
    const auto mutation=[](uint64_t readAt,uint64_t changeAt,size_t bytes=0){Fresh();const auto tree=Tree();const auto graph=Graph();
        mutateAt=readAt;mutateRead=1;mutateTarget=changeAt;mutateBytes=bytes;sections::Snapshot value;
        Require(!value.Capture(graph,tree,Read) && !value.Ready() && !value.MatchesBefore(Read));};
    mutation(Root,Root+36,56);mutation(Object(0),Object(0)+28,32);mutation(Handles,Handles+16);
    mutation(EarlyStages,EarlyStages+0x15);mutation(LateStages,LateStages+96+0x10);mutation(LateStages,Object(8)+0x28);
    const auto changed=[](uint64_t at){Fresh();const auto tree=Tree();sections::Snapshot value;Require(value.Capture(Graph(),tree,Read));
        memory[at]^=1;Require(!value.MatchesBefore(Read));};
    changed(Root+36);changed(Object(3)+9);changed(Object(8)+0x20);changed(LateStages+3*48+0x15);changed(Handles+16);
    Fresh();auto tree=Tree();sections::Snapshot value;Require(value.Capture(Graph(),tree,Read));
    Put(Control(0),int32_t(2));Require(value.MatchesBefore(Read));
    Put(Control(0),int32_t(0));Require(!value.MatchesBefore(Read));
}
}
int main()try{Positive();GraphAndBounds();FreshPatternMutations();Coherence();
    std::puts("PASS source-bound GBuffer scene sections preserve ordered handles and full stage8 tail");return 0;}
catch(const std::exception& error){std::fprintf(stderr,"%s\n",error.what());return 1;}
