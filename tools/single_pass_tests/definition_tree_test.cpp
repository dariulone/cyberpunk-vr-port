#include "Render/NativeDefinitionTree.hpp"
#include <cstdio>
#include <cstring>
#include <map>
#include <stdexcept>
#include <string>

namespace tree=cvr::stereo::definition_tree;
namespace {
constexpr uint64_t Base=0x100000000,Root0=0x10000,Root1=0x10100,Child=0x20000;
constexpr uint64_t DriverVtable=Base+0x3094850,ElementsVtable=Base+0x2B39F70,UnknownVtable=Base+0x2B3A100;
constexpr uint64_t DescriptionVtable=Base+0x2B39DF0,CStringVtable=Base+0x2B3A038;
constexpr uint64_t ObjectAllocator=0x800000,ControlAllocator=0x810000,ArrayAllocator=0x820000;
constexpr uint64_t StringAllocator=0x830000;
constexpr uint64_t Handles0=0x30000,Handles1=0x31000,Descriptions=0x32000,Control0=0x40000;
std::map<uint64_t,uint8_t> memory;
uint32_t readCalls{},triggerCalls{},triggerAt{};
uint64_t triggerAddress{},mutationAddress{},mutationValue{};
size_t mutationBytes{};
uint64_t failAddress{};
uint32_t failAt{};
void Require(bool value,const char* message){if(!value)throw std::runtime_error(message);}
template<class T>void Put(uint64_t address,const T& value){
    const auto* bytes=reinterpret_cast<const uint8_t*>(&value);
    for(size_t i=0;i<sizeof(value);++i)memory[address+i]=bytes[i];
}
bool Read(uint64_t address,void* destination,size_t bytes){
    if(++readCalls==failAt || address==failAddress || address>UINT64_MAX-bytes)return false;
    auto* out=static_cast<uint8_t*>(destination);
    for(size_t i=0;i<bytes;++i){const auto found=memory.find(address+i);if(found==memory.end())return false;out[i]=found->second;}
    if(address==triggerAddress && ++triggerCalls==triggerAt){
        const auto* value=reinterpret_cast<const uint8_t*>(&mutationValue);
        for(size_t i=0;i<mutationBytes;++i)memory[mutationAddress+i]=value[i];
    }
    return true;
}
void Controls(){
    readCalls=triggerCalls=triggerAt=failAt=0;triggerAddress=mutationAddress=mutationValue=failAddress=0;mutationBytes=0;
}
void Methods(uint64_t vtable,uint64_t destructor,uint64_t work){Put(vtable,destructor);Put(vtable+8,work);}
void Object(uint64_t object,uint64_t vtable){Put(object,vtable);}
void References(uint64_t control,int32_t strong=1,int32_t weak=1){Put(control,strong);Put(control+4,weak);}
void Vector(uint64_t object,uint64_t offset,uint64_t data,uint32_t capacity,uint32_t count,uint32_t stride){
    Put(object+offset,tree::NativeVector16{capacity?data:ArrayAllocator,capacity,count});
    if(capacity)Put(data+uint64_t(capacity)*stride,ArrayAllocator);
}
void CStringObject(uint64_t object,uint32_t mode,uint32_t length,uint32_t capacity,uint64_t data,uint64_t allocator){
    Object(object,CStringVtable);std::array<uint8_t,32> storage{};
    const auto flags=(mode<<30)|length;
    std::memcpy(storage.data(),&data,sizeof(data));std::memcpy(storage.data()+0x10,&capacity,sizeof(capacity));
    std::memcpy(storage.data()+0x14,&flags,sizeof(flags));std::memcpy(storage.data()+0x18,&allocator,sizeof(allocator));
    Put(object+0x20,storage);
}
void Fresh(){
    memory.clear();Controls();
    Put(Base+tree::ObjectAllocatorRva,ObjectAllocator|uint64_t(5));
    Put(Base+tree::ControlAllocatorRva,ControlAllocator|uint64_t(3));
    Put(ArrayAllocator+0x20,Base+0x985ABC);
    Methods(DriverVtable,Base+tree::SceneDriverDestructorRva,Base+0x332D5C);
    Methods(ElementsVtable,Base+tree::RenderElementsDestructorRva,Base+0x23A938);
    Methods(UnknownVtable,Base+0xABCDEF,Base+0x123457);
    Methods(DescriptionVtable,Base+tree::DescriptionOwnerDestructorRva,Base+0xC26DB8);
    Methods(CStringVtable,Base+tree::CStringDestructorRva,Base+0x58DA9C);
}
void Dag(){
    Fresh();Object(Root0,DriverVtable);Object(Root1,DriverVtable);Object(Child,ElementsVtable);
    Vector(Root0,0x28,Handles0,2,2,16);Vector(Root1,0x28,Handles1,1,1,16);
    Vector(Child,0x30,Descriptions,2,1,0x30);
    Put(Handles0,tree::NativeHandle{Child,Control0});Put(Handles0+16,tree::NativeHandle{});
    Put(Handles1,tree::NativeHandle{Child,Control0});References(Control0,2,1);
}
tree::Snapshot Capture(std::span<const uint64_t> roots){
    tree::Snapshot snapshot;Require(snapshot.Capture(roots,Base,Read,true),"Stable tree capture failed");return snapshot;
}
void Rejected(std::span<const uint64_t> roots,tree::Failure reason){
    tree::Snapshot snapshot;
    const bool accepted=snapshot.Capture(roots,Base,Read,true);
    if(accepted || snapshot.Ready() || snapshot.Rejection()!=reason)
        throw std::runtime_error("Native tree rejection expected "+std::to_string(uint32_t(reason))+
            ", got "+std::to_string(uint32_t(snapshot.Rejection())));
    Require(snapshot.Nodes().empty() && snapshot.Handles().empty() && !snapshot.MatchesBefore(Read),
        "Failed tree leaked accepted metadata");
    Require(snapshot.Arrays().empty() && snapshot.Strings().empty(),"Failed tree leaked owned storage metadata");
}
void Arm(uint64_t target,uint64_t value,size_t bytes=8){
    // The third allocator callback field is the last native read of Dag's first pass.
    triggerAddress=ArrayAllocator+0x20;triggerAt=3;mutationAddress=target;mutationValue=value;mutationBytes=bytes;
}
void Positive(){
    Dag();const std::array<uint64_t,3> roots{Root0,0,Root1};const auto before=memory;
    const auto snapshot=Capture(roots);
    Require(memory==before && snapshot.Ready() && snapshot.Rejection()==tree::Failure::None,"Capture mutated native storage");
    Require(snapshot.Nodes().size()==3 && snapshot.Handles().size()==3 && snapshot.Arrays().size()==3,
        "Shared child was duplicated or null handle lost");
    Require(snapshot.Roots().size()==3 && snapshot.Roots()[1].object==0 && snapshot.Roots()[1].node==tree::NoIndex,
        "Synthetic root slot lost");
    Require(snapshot.Nodes()[2].object==Child && snapshot.Nodes()[2].kind==tree::Kind::RenderElements &&
        snapshot.Nodes()[2].destructor==Base+tree::RenderElementsDestructorRva && snapshot.Nodes()[2].work==Base+0x23A938 &&
        snapshot.Nodes()[2].controlBlock==Control0 && snapshot.Nodes()[2].maxDepth==1,"Child identity incomplete");
    Require(snapshot.Handles()[0].child==snapshot.Handles()[2].child && snapshot.Handles()[1].child==tree::NoIndex &&
        snapshot.Handles()[0].strong==2 && snapshot.Handles()[0].weak==1,"Shared handle/reference capture incomplete");
    Require(snapshot.Arrays()[0].headerAddress==Root0+0x28 && snapshot.Arrays()[0].data==Handles0 &&
        snapshot.Arrays()[0].trailerAddress==Handles0+32 && snapshot.Arrays()[0].allocator==ArrayAllocator &&
        snapshot.Arrays()[0].freeCallback==Base+0x985ABC && snapshot.Arrays()[2].stride==0x30,
        "Native array allocator metadata incomplete");
    Require(snapshot.Allocators()[0].address==Base+tree::ObjectAllocatorRva && snapshot.Allocators()[0].raw==(ObjectAllocator|5) &&
        snapshot.Allocators()[0].allocator==ObjectAllocator && snapshot.Allocators()[1].allocator==ControlAllocator,
        "Flagged native allocator identities incomplete");
    Require(snapshot.MatchesBefore(Read) && memory==before,"Revalidation mutated native storage");
    Require(!snapshot.NativeOwnershipProved() && !snapshot.DestructorSafetyProved(),"Structural capture claimed a safety proof");

    Dag();Put(Handles0+16,tree::NativeHandle{0,Control0+16});References(Control0+16,1,2);
    const auto controlledNull=Capture(roots);
    Require(controlledNull.Handles()[1].object==0 && controlledNull.Handles()[1].controlBlock==Control0+16 &&
        controlledNull.Handles()[1].child==tree::NoIndex && controlledNull.Handles()[1].strong==1,
        "Native controlled-null handle semantics lost");

    Fresh();Object(Root0,UnknownVtable);const std::array<uint64_t,1> unknownRoot{Root0};tree::Snapshot unknown;
    Require(unknown.Capture(unknownRoot,Base,Read,false) && unknown.Nodes()[0].kind==tree::Kind::Unknown &&
        unknown.Nodes()[0].destructor==Base+0xABCDEF && unknown.Nodes()[0].work==Base+0x123457 && unknown.Arrays().empty(),
        "Unknown type was inferred safe or private object bytes were guessed");
    Dag();Object(Child,UnknownVtable);const auto unknownChild=Capture(roots);
    Require(unknownChild.Nodes()[2].kind==tree::Kind::Unknown && unknownChild.Nodes()[2].controlBlock==Control0 &&
        unknownChild.Arrays().size()==2,"Unknown shared child identity lost");
    Fresh();Methods(UnknownVtable,Base+tree::SceneDriverDestructorRva,Base+0x123457);
    Object(Root0,UnknownVtable);Object(Child,ElementsVtable);References(Control0);
    Vector(Root0,0x28,Handles0,1,1,16);Put(Handles0,tree::NativeHandle{Child,Control0});
    Vector(Child,0x30,0,0,0,0x30);const auto byDestructor=Capture(unknownRoot);
    Require(byDestructor.Nodes()[0].kind==tree::Kind::SceneDriver && byDestructor.Nodes().size()==2 &&
        byDestructor.Nodes()[1].kind==tree::Kind::RenderElements,"Recursion inferred from vtable/view index instead of guarded destructor");
    Dag();Vector(Root0,0x28,0,0,0,16);Vector(Root1,0x28,0,0,0,16);
    const auto empty=Capture(roots);
    Require(empty.Handles().empty() && !empty.Arrays()[0].trailerAddress && empty.Arrays()[0].allocator==ArrayAllocator,
        "Empty native vector allocator sentinel rejected");
    Fresh();const std::array<uint64_t,2> nullRoots{0,0};Require(Capture(nullRoots).Nodes().empty(),"All-null root slots rejected");
}
void CountsAndChanges(){
    const std::array<uint64_t,2> roots{Root0,Root1};
    Dag();Arm(Control0,4,4);const auto advancing=Capture(roots);
    Require(advancing.Handles()[0].strong==4 && advancing.Handles()[2].strong==4,"Second-pass positive reference sample not exported");
    Controls();References(Control0,3,5);Require(advancing.MatchesBefore(Read),"Harmless later reference changes rejected");
    Dag();Arm(Control0+4,4,4);Require(Capture(roots).Handles()[0].weak==4,"Harmless weak count advance rejected");
    Dag();Arm(Control0,0,4);Rejected(roots,tree::Failure::References);
    Dag();Arm(Control0+4,0,4);Rejected(roots,tree::Failure::References);
    Dag();Arm(Control0,1,4);Rejected(roots,tree::Failure::References);
    Dag();References(Control0,-1,1);Rejected(roots,tree::Failure::References);
    Dag();References(Control0,2,-1);Rejected(roots,tree::Failure::References);

    struct Change {uint64_t address,value;size_t bytes;};
    const std::array<Change,7> changes{{
        {Root0+0x34,1,4},{Handles0+16,0,8},{Handles0+32,ArrayAllocator+0x100,8},
        {ArrayAllocator+0x20,Base+0x985AB8,8},{ElementsVtable+8,Base+0x23A939,8},
        {Base+tree::ObjectAllocatorRva,ObjectAllocator|1,8},{Base+tree::ControlAllocatorRva,ControlAllocator+8,8}
    }};
    for(const auto& change:changes){
        Dag();
        if(change.address==Handles0+16){Put(Handles0+16,tree::NativeHandle{0,Control0+16});References(Control0+16,1,1);}
        if(change.address==Handles0+32)Put(ArrayAllocator+0x120,Base+0x985ABC);
        // The null object's control changes between two valid snapshots.
        if(change.address==Handles0+16)Arm(Handles0+24,0);else Arm(change.address,change.value,change.bytes);
        Rejected(roots,tree::Failure::Changed);
    }
    Dag();Object(Child+0x100,UnknownVtable);References(Control0+16);
    Put(Handles0+16,tree::NativeHandle{Child+0x100,Control0+16});Arm(Handles0+16,0);
    Rejected(roots,tree::Failure::Changed);
    Dag();const auto saved=Capture(roots);Put(ElementsVtable+8,Base+0x23A939);
    Require(!saved.MatchesBefore(Read),"Method identity drift accepted by revalidation");
    Dag();const auto stable=Capture(roots);Controls();failAddress=Root0;
    Require(!stable.MatchesBefore(Read),"Failed revalidation read accepted");
    Dag();tree::Snapshot reset;Require(reset.Capture(roots,Base,Read,true),"Reset fixture failed");
    Put(Control0,int32_t(0));Require(!reset.Capture(roots,Base,Read,true) && !reset.Ready() && reset.Nodes().empty(),
        "Failed recapture retained a ready snapshot");
}
void InvalidStructure(){
    const std::array<uint64_t,2> roots{Root0,Root1};tree::Snapshot snapshot;
    Dag();Require(!snapshot.Capture(roots,Base,Read,false) && snapshot.Rejection()==tree::Failure::Layout,"Unguarded SceneDriver layout parsed");
    Require(!snapshot.Capture(roots,0,Read,true) && snapshot.Rejection()==tree::Failure::Arguments,"Null image accepted");
    Require(!snapshot.Capture(roots,UINT64_MAX-7,Read,true) && snapshot.Rejection()==tree::Failure::Arguments,"Image RVA overflow accepted");
    Require(!snapshot.Capture(roots,Base,nullptr,true) && snapshot.Rejection()==tree::Failure::Arguments,"Missing reader accepted");
    std::vector<uint64_t> tooManyRoots(tree::MaxRoots+1,0);
    Require(!snapshot.Capture(tooManyRoots,Base,Read,true) && snapshot.Rejection()==tree::Failure::Arguments,"Root metadata overflow accepted");
    Dag();const std::array<uint64_t,2> duplicate{Root0,Root0};Rejected(duplicate,tree::Failure::Root);
    Dag();Put(Root0+0x28,tree::NativeVector16{Handles0,1,2});Rejected(roots,tree::Failure::Bounds);
    Dag();Put(Root0+0x28,tree::NativeVector16{Handles0,tree::MaxArrayCapacity+1,0});Rejected(roots,tree::Failure::Bounds);
    Dag();Put(Root0+0x28,tree::NativeVector16{0,2,2});Rejected(roots,tree::Failure::Bounds);
    Dag();Put(Root0+0x28,tree::NativeVector16{Handles0+1,2,2});Rejected(roots,tree::Failure::Bounds);
    Dag();Put(Root0+0x28,tree::NativeVector16{ArrayAllocator,0,1});Rejected(roots,tree::Failure::Bounds);
    Dag();Put(Root0+0x28,tree::NativeVector16{UINT64_MAX-7,2,0});Rejected(roots,tree::Failure::Bounds);
    Dag();Put(Handles0,tree::NativeHandle{Child,0});Rejected(roots,tree::Failure::Pairing);
    Dag();Put(Handles0,tree::NativeHandle{Child,Control0+1});Rejected(roots,tree::Failure::Control);
    Dag();Put(Handles0,tree::NativeHandle{Child+1,Control0});Rejected(roots,tree::Failure::Node);
    Dag();Put(Handles1,tree::NativeHandle{Child,Control0+16});References(Control0+16);Rejected(roots,tree::Failure::Pairing);
    Dag();Put(Handles1,tree::NativeHandle{Child+0x100,Control0});Object(Child+0x100,UnknownVtable);Rejected(roots,tree::Failure::Pairing);
    Dag();Put(Handles1,tree::NativeHandle{0,Control0});Rejected(roots,tree::Failure::Pairing);
    Dag();Put(Handles0,tree::NativeHandle{Root1,Control0});Rejected(roots,tree::Failure::Pairing);
    Dag();Put(Handles0,tree::NativeHandle{Child,Child});Rejected(roots,tree::Failure::Overlap);
    Dag();Put(Root1+0x28,tree::NativeVector16{Handles0,2,1});Rejected(roots,tree::Failure::Overlap);
    Dag();Put(Root0+0x28,tree::NativeVector16{Root1,1,0});Rejected(roots,tree::Failure::Overlap);
    Dag();Put(Child+0x30,tree::NativeVector16{Handles0,1,0});Rejected(roots,tree::Failure::Overlap);
    Dag();Put(Handles0+32,Handles0);Rejected(roots,tree::Failure::Overlap);
    Dag();Put(Base+tree::ObjectAllocatorRva,Root0);Rejected(roots,tree::Failure::Overlap);
    Dag();Put(Handles0+32,uint64_t(0));Rejected(roots,tree::Failure::Allocator);
    Dag();Put(Handles0+32,ArrayAllocator+1);Rejected(roots,tree::Failure::Allocator);
    Dag();Put(ArrayAllocator+0x20,uint64_t(0));Rejected(roots,tree::Failure::Allocator);
    Dag();Put(Base+tree::ControlAllocatorRva,uint64_t(7));Rejected(roots,tree::Failure::Allocator);
    Dag();memory.erase(Root0+0x28);Rejected(roots,tree::Failure::Header);
    Dag();memory.erase(Handles0);Rejected(roots,tree::Failure::Handles);
    Dag();memory.erase(Control0);Rejected(roots,tree::Failure::Control);
    Dag();memory.erase(Handles0+32);Rejected(roots,tree::Failure::Allocator);
    Dag();memory.erase(ArrayAllocator+0x20);Rejected(roots,tree::Failure::Allocator);
    Dag();memory.erase(ElementsVtable);Rejected(roots,tree::Failure::Node);
    Dag();Capture(roots);const auto total=readCalls;
    for(uint32_t i=1;i<=total;++i){Dag();failAt=i;Require(!snapshot.Capture(roots,Base,Read,true) && !snapshot.Ready(),"Failed reader accepted a partial tree");}
}
void PathsAndBudgets(){
    const std::array<uint64_t,1> roots{Root0};
    const auto chain=[&](uint32_t edges){
        Fresh();
        for(uint32_t i=0;i<=edges;++i){
            const auto object=Root0+uint64_t(i)*0x100;
            Object(object,i==edges?UnknownVtable:DriverVtable);
            if(i==edges)continue;
            const auto data=0x100000+uint64_t(i)*0x100,control=0x200000+uint64_t(i)*16;
            Vector(object,0x28,data,1,1,16);Put(data,tree::NativeHandle{object+0x100,control});References(control);
        }
    };
    chain(tree::MaxDepth);Require(Capture(roots).Nodes().back().maxDepth==tree::MaxDepth,"Maximum bounded path rejected");
    chain(tree::MaxDepth+1);Rejected(roots,tree::Failure::Depth);
    chain(3);constexpr uint64_t shortRoot=0x50000;
    Object(shortRoot,DriverVtable);Vector(shortRoot,0x28,0x300000,1,1,16);
    Put(0x300000,tree::NativeHandle{Root0+0x300,0x200020});References(0x200020,2);
    const std::array<uint64_t,2> sharedPaths{Root0,shortRoot};const auto deepShared=Capture(sharedPaths);
    const auto shared=std::find_if(deepShared.Nodes().begin(),deepShared.Nodes().end(),
        [](const tree::Node& node){return node.object==Root0+0x300;});
    Require(shared!=deepShared.Nodes().end() && shared->maxDepth==3,"Shared DAG did not preserve its longest path");
    // A cycle wholly below a separately owned root uses consistent shared pairs.
    Fresh();Object(Root0,DriverVtable);Object(Child,DriverVtable);Object(Child+0x100,DriverVtable);
    Vector(Root0,0x28,Handles0,1,1,16);Vector(Child,0x28,Handles1,1,1,16);
    Vector(Child+0x100,0x28,Descriptions,1,1,16);
    Put(Handles0,tree::NativeHandle{Child,Control0});Put(Handles1,tree::NativeHandle{Child+0x100,Control0+16});
    Put(Descriptions,tree::NativeHandle{Child,Control0});References(Control0,2);References(Control0+16);
    Rejected(roots,tree::Failure::Cycle);

    Fresh();std::vector<uint64_t> manyRoots;
    for(uint32_t i=0;i<=tree::MaxNodes;++i){const auto object=0x100000+uint64_t(i)*16;Object(object,UnknownVtable);manyRoots.push_back(object);}
    Rejected(manyRoots,tree::Failure::NodeBudget);
    manyRoots.pop_back();Require(Capture(manyRoots).Nodes().size()==tree::MaxNodes,"Maximum node budget rejected");
    Fresh();Object(Root0,DriverVtable);Vector(Root0,0x28,0x100000,tree::MaxHandles,tree::MaxHandles,16);
    for(uint32_t i=0;i<tree::MaxHandles;++i)Put(0x100000+uint64_t(i)*16,tree::NativeHandle{});
    Require(Capture(roots).Handles().size()==tree::MaxHandles,"Maximum handle budget rejected");
    Object(Root1,DriverVtable);Vector(Root1,0x28,Handles1,1,1,16);Put(Handles1,tree::NativeHandle{});
    const std::array<uint64_t,2> twoRoots{Root0,Root1};Rejected(twoRoots,tree::Failure::HandleBudget);
    Fresh();Object(Root0,DriverVtable);Object(Root1,DriverVtable);
    Vector(Root0,0x28,0x100000,tree::MaxHandles,0,16);Vector(Root1,0x28,Handles1,1,0,16);
    Rejected(twoRoots,tree::Failure::StorageBudget);
    Fresh();std::vector<uint64_t> storageRoots;
    for(uint32_t i=0;i<43;++i){
        const auto object=0x100000+uint64_t(i)*0x100,data=0x1000000+uint64_t(i)*0x100000;
        Object(object,ElementsVtable);Vector(object,0x30,data,tree::MaxArrayCapacity,0,0x30);storageRoots.push_back(object);
    }
    Rejected(storageRoots,tree::Failure::StorageBudget);
}
void DescriptionAndStringStorage(){
    const std::array<uint64_t,1> roots{Root0};tree::Snapshot snapshot;
    Fresh();Object(Root0,DescriptionVtable);Vector(Root0,0x20,Descriptions,2,1,48);
    const auto description=Capture(roots);
    Require(description.Nodes()[0].kind==tree::Kind::DescriptionOwner && description.Arrays().size()==1 &&
        description.Arrays()[0].headerAddress==Root0+0x20 && description.Arrays()[0].stride==48 &&
        description.Arrays()[0].trailerAddress==Descriptions+96 && description.Arrays()[0].allocator==ArrayAllocator &&
        description.Arrays()[0].freeCallback==Base+0x985ABC,"DescriptionOwner vector storage incomplete");
    Require(!snapshot.Capture(roots,Base,Read,false) && snapshot.Rejection()==tree::Failure::Layout,
        "Unguarded DescriptionOwner layout parsed");
    Fresh();Object(Root0,DescriptionVtable);Vector(Root0,0x20,0,0,0,48);
    Require(Capture(roots).Arrays()[0].allocator==ArrayAllocator,"Empty DescriptionOwner sentinel lost");

    Fresh();CStringObject(Root0,0,19,UINT32_MAX,UINT64_MAX,UINT64_MAX);const auto before=memory;
    const auto inlineString=Capture(roots);const auto& small=inlineString.Strings()[0];
    Require(inlineString.Nodes()[0].kind==tree::Kind::CString && inlineString.Nodes()[0].array==tree::NoIndex &&
        inlineString.Arrays().empty() && small.node==0 && small.storageAddress==Root0+0x20 && small.data==Root0+0x20 &&
        !small.mode && small.length==19 && small.capacity==19 && !small.allocationBytes && !small.freeCallback &&
        !small.fallbackGlobalAddress && !small.fallbackFreeAdapter && memory==before,
        "Inline CString capacity bytes were interpreted as a heap header or native storage changed");
    Require(inlineString.MatchesBefore(Read) && memory==before,"CString revalidation mutated storage");
    Require(!snapshot.Capture(roots,Base,Read,false) && snapshot.Rejection()==tree::Failure::Layout,"Unguarded CString layout parsed");
    Fresh();CStringObject(Root0,2,123,0,0x12345,UINT64_MAX);const auto borrowed=Capture(roots);
    Require(borrowed.Strings()[0].mode==2 && borrowed.Strings()[0].data==0x12345 && borrowed.Strings()[0].length==123 &&
        !borrowed.Strings()[0].allocationBytes && !borrowed.Strings()[0].freeCallback && !borrowed.Strings()[0].fallbackGlobalAddress,
        "Borrowed CString imposed data alignment, capacity or owned allocator semantics");
    Fresh();CStringObject(Root0,1,0,0,0x12345,UINT64_MAX);const auto zeroOwned=Capture(roots);
    Require(zeroOwned.Strings()[0].mode==1 && !zeroOwned.Strings()[0].allocationBytes && !zeroOwned.Strings()[0].freeCallback &&
        !zeroOwned.Strings()[0].fallbackGlobalAddress,"Owned-cap0 CString dereferenced an inactive free route");

    Fresh();CStringObject(Root0,1,5,31,Descriptions+1,0);
    Put(Base+tree::StringAllocatorRva,StringAllocator|uint64_t(7));const auto fallback=Capture(roots);
    const auto& heap=fallback.Strings()[0];
    Require(heap.mode==1 && heap.length==5 && heap.capacity==31 && heap.data==Descriptions+1 && heap.allocationBytes==32 &&
        heap.allocatorSlotAddress==Root0+0x38 && !heap.allocatorValue && !heap.freeCallback &&
        heap.fallbackGlobalAddress==Base+tree::StringAllocatorRva && heap.fallbackRaw==(StringAllocator|7) &&
        heap.fallbackAllocator==StringAllocator && heap.fallbackFreeAdapter==Base+tree::StringFreeAdapterRva,
        "Fallback CString route confused direct adapter with custom callback ABI");
    Fresh();CStringObject(Root0,1,13,15,Descriptions+3,ArrayAllocator);const auto custom=Capture(roots);
    Require(custom.Strings()[0].allocatorValue==ArrayAllocator && custom.Strings()[0].allocatorSlotAddress==Root0+0x38 &&
        custom.Strings()[0].freeCallback==Base+0x985ABC && custom.Strings()[0].allocationBytes==16 &&
        !custom.Strings()[0].fallbackGlobalAddress && !custom.Strings()[0].fallbackFreeAdapter,
        "Custom CString allocator slot or callback incomplete");
    Put(ArrayAllocator+0x20,Base+0xABCDE1);const auto unknownCallback=Capture(roots);
    Require(unknownCallback.Strings()[0].freeCallback==Base+0xABCDE1 && !unknownCallback.DestructorSafetyProved(),
        "Unknown custom CString callback was omitted or certified safe");

    Fresh();CStringObject(Root0,1,5,31,Descriptions,0);CStringObject(Root1,1,6,31,Descriptions+64,0);
    Put(Base+tree::StringAllocatorRva,StringAllocator|uint64_t(3));const std::array<uint64_t,2> twoRoots{Root0,Root1};
    const auto repeatedFallback=Capture(twoRoots);
    Require(repeatedFallback.Strings().size()==2 && repeatedFallback.Strings()[0].fallbackRaw==repeatedFallback.Strings()[1].fallbackRaw,
        "Shared CString fallback allocator global rejected or sampled inconsistently");
    Fresh();Object(Root0,DriverVtable);Vector(Root0,0x28,Handles0,1,1,16);
    Put(Handles0,tree::NativeHandle{Child,Control0});References(Control0);
    CStringObject(Child,0,7,UINT32_MAX,UINT64_MAX,UINT64_MAX);const auto childName=Capture(roots);
    Require(childName.Nodes()[1].kind==tree::Kind::CString && childName.Strings()[0].node==1 && childName.Nodes()[1].controlBlock==Control0,
        "Shared CString child storage lost its node identity");
    Fresh();Object(Root0,DescriptionVtable);Vector(Root0,0x20,Descriptions,1,1,48);
    CStringObject(Root1,0,0,UINT32_MAX,0,UINT64_MAX);const auto mixed=Capture(twoRoots);
    Require(mixed.Arrays().size()==1 && mixed.Strings().size()==1 && mixed.Strings()[0].node==1,"Mixed storage families were conflated");
}
void InvalidStringStorageAndChanges(){
    const std::array<uint64_t,1> roots{Root0};
    Fresh();CStringObject(Root0,3,0,0,0,0);Rejected(roots,tree::Failure::Bounds);
    Fresh();CStringObject(Root0,0,20,0,0,0);Rejected(roots,tree::Failure::Bounds);
    Fresh();CStringObject(Root0,1,16,15,Descriptions,ArrayAllocator);Rejected(roots,tree::Failure::Bounds);
    Fresh();CStringObject(Root0,1,0,UINT32_MAX,Descriptions,ArrayAllocator);Rejected(roots,tree::Failure::Bounds);
    Fresh();CStringObject(Root0,1,0,uint32_t(tree::MaxOwnedBytes),Descriptions,ArrayAllocator);Rejected(roots,tree::Failure::StorageBudget);
    Fresh();CStringObject(Root0,1,1,15,0,ArrayAllocator);Rejected(roots,tree::Failure::Bounds);
    Fresh();CStringObject(Root0,1,1,15,UINT64_MAX-3,ArrayAllocator);Rejected(roots,tree::Failure::Bounds);
    Fresh();CStringObject(Root0,1,1,15,Descriptions,ArrayAllocator+1);Rejected(roots,tree::Failure::Allocator);
    Fresh();CStringObject(Root0,1,1,15,Descriptions,Descriptions);Rejected(roots,tree::Failure::Overlap);
    Fresh();CStringObject(Root0,1,1,15,Descriptions,ArrayAllocator);Put(ArrayAllocator+0x20,uint64_t(0));Rejected(roots,tree::Failure::Allocator);
    Fresh();CStringObject(Root0,1,1,15,Descriptions,0);Rejected(roots,tree::Failure::Allocator);
    Fresh();CStringObject(Root0,1,1,15,Descriptions,0);Put(Base+tree::StringAllocatorRva,uint64_t(7));Rejected(roots,tree::Failure::Allocator);
    Fresh();CStringObject(Root0,1,1,15,Descriptions,0);Put(Base+tree::StringAllocatorRva,Descriptions);Rejected(roots,tree::Failure::Overlap);
    Fresh();CStringObject(Root0,0,0,0,0,0);memory.erase(Root0+0x3F);Rejected(roots,tree::Failure::Header);
    const std::array<uint64_t,2> twoRoots{Root0,Root1};
    Fresh();CStringObject(Root0,1,1,15,Root1+1,ArrayAllocator);Object(Root1,UnknownVtable);Rejected(twoRoots,tree::Failure::Overlap);
    Fresh();CStringObject(Root0,1,1,15,Descriptions,ArrayAllocator);CStringObject(Root1,1,1,15,Descriptions,ArrayAllocator);
    Rejected(twoRoots,tree::Failure::Overlap);
    Fresh();CStringObject(Root0,1,0,uint32_t(tree::MaxOwnedBytes-1),0x10000000,ArrayAllocator);
    Require(Capture(roots).Strings()[0].allocationBytes==tree::MaxOwnedBytes,"Maximum owned CString byte budget rejected");
    CStringObject(Root1,1,0,1,Descriptions,ArrayAllocator);Rejected(twoRoots,tree::Failure::StorageBudget);
    Fresh();Object(Root0,DescriptionVtable);Vector(Root0,0x20,Descriptions,1,0,48);
    CStringObject(Root1,1,1,15,Descriptions+8,ArrayAllocator);Rejected(twoRoots,tree::Failure::Overlap);

    const auto arm=[&](uint64_t trigger,uint64_t target,uint64_t value,size_t bytes=8){
        triggerAddress=trigger;triggerAt=1;mutationAddress=target;mutationValue=value;mutationBytes=bytes;
    };
    Fresh();CStringObject(Root0,0,2,UINT32_MAX,0,UINT64_MAX);
    arm(Root0+0x20,Root0+0x34,3,4);Rejected(roots,tree::Failure::Changed);
    Fresh();CStringObject(Root0,1,1,15,Descriptions,ArrayAllocator);
    arm(Root0+0x20,Root0+0x20,Descriptions+32);Rejected(roots,tree::Failure::Changed);
    Fresh();CStringObject(Root0,1,1,15,Descriptions,ArrayAllocator);
    arm(ArrayAllocator+0x20,ArrayAllocator+0x20,Base+0xABCDE1);Rejected(roots,tree::Failure::Changed);
    Fresh();CStringObject(Root0,1,1,15,Descriptions,0);Put(Base+tree::StringAllocatorRva,StringAllocator|uint64_t(1));
    arm(Base+tree::StringAllocatorRva,Base+tree::StringAllocatorRva,StringAllocator+8);Rejected(roots,tree::Failure::Changed);
    Fresh();CStringObject(Root0,1,0,0,Descriptions,0);Put(Base+tree::StringAllocatorRva,StringAllocator);
    arm(Root0+0x20,Root0+0x30,1,4);Rejected(roots,tree::Failure::Changed);
    Fresh();CStringObject(Root0,1,1,15,Descriptions,ArrayAllocator);const auto saved=Capture(roots);
    Put(Root0+0x34,uint32_t(0x40000002));Require(!saved.MatchesBefore(Read),"CString structural drift accepted by revalidation");
    for(const bool fallback:{false,true}){
        Fresh();CStringObject(Root0,1,1,15,Descriptions,fallback?0:ArrayAllocator);
        Put(Base+tree::StringAllocatorRva,StringAllocator);Capture(roots);const auto total=readCalls;
        for(uint32_t i=1;i<=total;++i){
            Fresh();CStringObject(Root0,1,1,15,Descriptions,fallback?0:ArrayAllocator);Put(Base+tree::StringAllocatorRva,StringAllocator);
            failAt=i;tree::Snapshot snapshot;
            Require(!snapshot.Capture(roots,Base,Read,true) && !snapshot.Ready() && snapshot.Strings().empty(),
                "Failed CString read accepted partial storage metadata");
        }
    }
}
}
int main()try{
    Positive();CountsAndChanges();InvalidStructure();PathsAndBudgets();DescriptionAndStringStorage();InvalidStringStorageAndChanges();
    std::puts("PASS native definition tree: bounded shared DAG, description vectors and exact CString storage/free-route metadata; positive reference samples and physical double-read drift; no native mutation or safety proof");
    return 0;
}catch(const std::exception& error){std::fprintf(stderr,"%s\n",error.what());return 1;}
