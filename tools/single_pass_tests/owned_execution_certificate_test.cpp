#include "Render/OwnedGraphExecution.hpp"
#include <cstdio>
#include <cstring>
#include <map>
#include <set>
#include <stdexcept>
#include <string>
#include <tuple>

namespace tree=cvr::stereo::definition_tree;
namespace owned=cvr::stereo::owned_execution;
namespace {
constexpr uint64_t Base=0x100000000ull,CommonAllocator=Base+0x3BCDD80;
constexpr uint64_t ArrayAllocator=Base+0x30947E8,ArrayCallback=Base+0x120F6F0;
constexpr uint64_t Root0=0x10000,Root1=0x10100,Description=0x10200;
constexpr uint64_t Elements=0x20000,Name=0x20100,Control0=0x40000,Control1=0x40010;
constexpr uint64_t Handles0=0x30000,Handles1=0x30100,ElementsData=0x30200;
constexpr uint64_t DescriptionData=0x30300,StringData=0x30401;
std::map<uint64_t,uint8_t> memory;
uint32_t readCalls{},failAt{},triggerReads{};
uint64_t failAddress{},triggerAddress{},replacement{};

void Require(bool value,const std::string& message){if(!value)throw std::runtime_error(message);}
template<class T>void Put(uint64_t address,const T& value){
    const auto* bytes=reinterpret_cast<const uint8_t*>(&value);
    for(size_t i=0;i<sizeof(value);++i)memory[address+i]=bytes[i];
}
bool Read(uint64_t address,void* destination,size_t bytes){
    if(++readCalls==failAt || address==failAddress || address>UINT64_MAX-bytes)return false;
    auto* out=static_cast<uint8_t*>(destination);
    for(size_t i=0;i<bytes;++i){
        const auto found=memory.find(address+i);if(found==memory.end())return false;out[i]=found->second;
    }
    if(address==triggerAddress && ++triggerReads==1)Put(address,replacement);
    return true;
}
void Controls(){readCalls=failAt=triggerReads=0;failAddress=triggerAddress=replacement=0;}
void Fresh(){
    memory.clear();Controls();
    Put(Base+tree::ObjectAllocatorRva,CommonAllocator|uint64_t(5));
    Put(Base+tree::ControlAllocatorRva,CommonAllocator|uint64_t(3));
    Put(Base+tree::StringAllocatorRva,CommonAllocator|uint64_t(7));
    Put(ArrayAllocator+0x20,ArrayCallback);
}
const owned::CertifiedNode& Route(uint32_t destructor){
    const auto found=std::find_if(std::begin(owned::CertificateNodes),std::end(owned::CertificateNodes),
        [&](const owned::CertifiedNode& row){return row.destructor==destructor;});
    Require(found!=std::end(owned::CertificateNodes),"Missing certificate destructor route");return *found;
}
void Methods(uint64_t vtable,uint64_t destructor,uint64_t work){Put(vtable,destructor);Put(vtable+8,work);}
void Object(uint64_t object,const owned::CertifiedNode& row){
    Put(object,Base+row.vtable);Methods(Base+row.vtable,Base+row.destructor,Base+row.work);
}
void References(uint64_t control,int32_t strong=1,int32_t weak=1){Put(control,strong);Put(control+4,weak);}
void Vector(uint64_t object,uint64_t offset,uint64_t data,uint32_t capacity,uint32_t count,uint32_t stride,
            uint64_t allocator=ArrayAllocator){
    Put(object+offset,tree::NativeVector16{capacity?data:allocator,capacity,count});
    if(capacity)Put(data+uint64_t(capacity)*stride,allocator);
}
void CString(uint64_t object,uint32_t mode=1,uint32_t length=5,uint32_t capacity=31,
             uint64_t data=StringData,uint64_t allocator=0){
    Object(object,Route(tree::CStringDestructorRva));std::array<uint8_t,32> storage{};
    const uint32_t flags=(mode<<30)|length;
    std::memcpy(storage.data(),&data,8);std::memcpy(storage.data()+0x10,&capacity,4);
    std::memcpy(storage.data()+0x14,&flags,4);std::memcpy(storage.data()+0x18,&allocator,8);
    Put(object+0x20,storage);
}
void Leaf(uint64_t object,const owned::CertifiedNode& row){
    Object(object,row);
    switch(row.destructor){
    case tree::SceneDriverDestructorRva:Vector(object,0x28,0,0,0,16);break;
    case tree::RenderElementsDestructorRva:Vector(object,0x30,0,0,0,48);break;
    case tree::DescriptionOwnerDestructorRva:Vector(object,0x20,0,0,0,48);break;
    case tree::CStringDestructorRva:CString(object);break;
    default:break;
    }
}
tree::Snapshot Capture(std::span<const uint64_t> roots){
    tree::Snapshot snapshot;Require(snapshot.Capture(roots,Base,Read,true),"Sparse certificate tree capture failed");return snapshot;
}
tree::Snapshot Single(){const std::array<uint64_t,1> roots{Root0};return Capture(roots);}
void CheckCertified(const tree::Snapshot& snapshot){
    const auto before=memory;
    Require(owned::TreeCertified(snapshot),"Known certificate identity was rejected");
    Require(owned::AllocatorsMatch(snapshot,Read),"Known current allocator identities were rejected");
    Require(snapshot.MatchesBefore(Read),"Stable certificate tree did not structurally revalidate");
    Require(memory==before,"Certificate identity helpers changed native memory");
    Require(!snapshot.NativeOwnershipProved() && !snapshot.DestructorSafetyProved(),
        "Structural observation invented native ownership or destructor safety");
}
void CheckUncertified(const tree::Snapshot& snapshot){
    Require(!owned::TreeCertified(snapshot) && !owned::AllocatorsMatch(snapshot,Read),
        "Unknown destructor/type/allocator route was certified");
}
void CaptureRejected(std::span<const uint64_t> roots){
    tree::Snapshot snapshot;
    Require(!snapshot.Capture(roots,Base,Read,true) && !snapshot.Ready(),"Invalid child/control tree was captured");
    CheckUncertified(snapshot);
}
void Dag(){
    Fresh();Object(Root0,Route(tree::SceneDriverDestructorRva));Object(Root1,Route(tree::SceneDriverDestructorRva));
    Object(Elements,Route(tree::RenderElementsDestructorRva));Object(Description,Route(tree::DescriptionOwnerDestructorRva));
    CString(Name);
    Vector(Root0,0x28,Handles0,3,3,16);Vector(Root1,0x28,Handles1,1,1,16);
    Vector(Elements,0x30,ElementsData,2,1,48);Vector(Description,0x20,DescriptionData,2,1,48);
    Put(Handles0,tree::NativeHandle{Elements,Control0});Put(Handles0+16,tree::NativeHandle{Name,Control1});
    Put(Handles0+32,tree::NativeHandle{});Put(Handles1,tree::NativeHandle{Elements,Control0});
    References(Control0,2,1);References(Control1);
}
tree::Snapshot CaptureDag(){const std::array<uint64_t,3> roots{Root0,Root1,Description};return Capture(roots);}

void InventoryAndEveryCertifiedTriple(){
    constexpr std::array<uint32_t,10> expectedDestructors{
        0x8E7E38,0x8E913C,0x8EA4E0,0x8EA51C,0x8EA620,0x8EAB98,
        0x190F138,0x19159E0,0x2276DC0,0x2276DF0};
    Require(std::size(owned::CertificateNodes)==133,"Certificate changed its audited 133-triple inventory (2.5: FSR3 gone)");
    std::set<std::tuple<uint32_t,uint32_t,uint32_t>> triples;
    std::set<uint32_t> destructors;
    for(const auto& row:owned::CertificateNodes){
        Require(triples.emplace(row.vtable,row.work,row.destructor).second,"Duplicate certificate triple");
        destructors.insert(row.destructor);
        Fresh();Leaf(Root0,row);const auto snapshot=Single();
        Require(snapshot.Nodes().size()==1 && snapshot.Nodes()[0].vtable==Base+row.vtable &&
            snapshot.Nodes()[0].work==Base+row.work && snapshot.Nodes()[0].destructor==Base+row.destructor,
            "Certificate fixture did not capture its exact triple");
        CheckCertified(snapshot);
    }
    Require(destructors==std::set<uint32_t>(expectedDestructors.begin(),expectedDestructors.end()),
        "Certificate introduced an unaudited destructor or lost a certified route");
}
void CompositeRoutes(){
    Dag();const auto snapshot=CaptureDag();CheckCertified(snapshot);
    Require(snapshot.Nodes().size()==5 && snapshot.Handles().size()==4 && snapshot.Arrays().size()==4 &&
        snapshot.Strings().size()==1,"Composite DAG route inventory changed");
    Require(snapshot.Strings()[0].fallbackGlobalAddress==Base+tree::StringAllocatorRva &&
        snapshot.Strings()[0].fallbackGlobalAddress!=snapshot.Allocators()[0].address &&
        snapshot.Strings()[0].fallbackGlobalAddress!=snapshot.Allocators()[1].address &&
        snapshot.Strings()[0].fallbackFreeAdapter==Base+tree::StringFreeAdapterRva,
        "CString fallback was confused with the object/control allocator globals or callback ABI");
    for(const auto kind:{tree::Kind::SceneDriver,tree::Kind::RenderElements,tree::Kind::DescriptionOwner,tree::Kind::CString})
        Require(std::any_of(snapshot.Nodes().begin(),snapshot.Nodes().end(),
            [&](const tree::Node& node){return node.kind==kind;}),"Composite DAG missed a known structural route");
}
void UnknownTriples(){
    const auto& plain=Route(0x8E7E38);
    Fresh();Leaf(Root0,plain);Methods(Base+plain.vtable,Base+plain.destructor,Base+plain.work+1);CheckUncertified(Single());
    Fresh();Leaf(Root0,plain);Methods(Base+plain.vtable,Base+plain.destructor+1,Base+plain.work);CheckUncertified(Single());
    Fresh();Leaf(Root0,plain);const uint64_t unknownVtable=Base+0x3F00000;
    Put(Root0,unknownVtable);Methods(unknownVtable,Base+plain.destructor,Base+plain.work);CheckUncertified(Single());
    Fresh();Leaf(Root0,plain);Methods(Base+plain.vtable,Base+plain.destructor,Base+Route(0x8EA620).work);
    CheckUncertified(Single());
    Dag();const auto& elements=Route(tree::RenderElementsDestructorRva);
    Methods(Base+elements.vtable,Base+elements.destructor,Base+elements.work+1);CheckUncertified(CaptureDag());
    Dag();const auto& driver=Route(tree::SceneDriverDestructorRva);
    Put(Root0,unknownVtable);Methods(unknownVtable,Base+driver.destructor,Base+driver.work);CheckUncertified(CaptureDag());
}
void UnsupportedAllocatorAndStringRoutes(){
    const auto& plain=Route(0x8E7E38);
    for(const auto global:{tree::ObjectAllocatorRva,tree::ControlAllocatorRva}){
        Fresh();Leaf(Root0,plain);Put(Base+global,CommonAllocator+0x100);CheckUncertified(Single());
    }
    Fresh();Leaf(Root0,Route(tree::RenderElementsDestructorRva));Put(ArrayAllocator+0x20,ArrayCallback+1);
    CheckUncertified(Single());
    Fresh();Object(Root0,Route(tree::DescriptionOwnerDestructorRva));
    const auto other=ArrayAllocator+0x100;Put(other+0x20,ArrayCallback);
    Vector(Root0,0x20,DescriptionData,2,1,48,other);CheckUncertified(Single());
    Fresh();CString(Root0);Put(Base+tree::StringAllocatorRva,CommonAllocator+0x100);CheckUncertified(Single());
    Fresh();CString(Root0,1,5,31,StringData,ArrayAllocator);CheckUncertified(Single());
    Fresh();CString(Root0,0,5,0,0,0);CheckUncertified(Single());
    Fresh();CString(Root0,2,5,0,StringData,0);CheckUncertified(Single());
    Fresh();CString(Root0,1,0,0,StringData,0);CheckUncertified(Single());
}
void CurrentAllocatorIdentityChanges(){
    const std::array<uint64_t,4> addresses{
        Base+tree::ObjectAllocatorRva,Base+tree::ControlAllocatorRva,
        Base+tree::StringAllocatorRva,ArrayAllocator+0x20};
    for(const auto address:addresses){
        Dag();const auto snapshot=CaptureDag();uint64_t original{};Require(Read(address,&original,8),"Missing fixture identity");
        Put(address,original+8);
        Require(owned::TreeCertified(snapshot) && !owned::AllocatorsMatch(snapshot,Read),
            "Snapshot certification incorrectly replaced live allocator identity checks");
        Put(address,original);Require(owned::AllocatorsMatch(snapshot,Read),"Restored allocator identity did not match");
        Controls();failAddress=address;Require(!owned::AllocatorsMatch(snapshot,Read),"Unreadable allocator identity accepted");
    }
    for(const auto global:{tree::ObjectAllocatorRva,tree::ControlAllocatorRva,tree::StringAllocatorRva}){
        Dag();const auto snapshot=CaptureDag();Put(Base+global,CommonAllocator|uint64_t(1));
        Require(!owned::AllocatorsMatch(snapshot,Read),"Allocator low-flag identity replacement accepted");
    }
    for(const auto address:addresses){
        Dag();const auto snapshot=CaptureDag();uint64_t original{};Require(Read(address,&original,8),"Missing tear identity");
        Controls();triggerAddress=address;replacement=original+8;
        Require(!owned::AllocatorsMatch(snapshot,Read),"Torn double-read allocator identity accepted");
    }
    Dag();const auto snapshot=CaptureDag();Controls();Require(owned::AllocatorsMatch(snapshot,Read),"Stable identity fixture failed");
    const auto calls=readCalls;
    for(uint32_t call=1;call<=calls;++call){
        Controls();failAt=call;Require(!owned::AllocatorsMatch(snapshot,Read),"Failed allocator identity read accepted");
    }
    Controls();Require(!owned::AllocatorsMatch(snapshot,nullptr),"Missing allocator reader accepted");
}
void InvalidChildTrees(){
    tree::Snapshot unready;CheckUncertified(unready);
    Fresh();const std::array<uint64_t,2> nullRoots{0,0};CheckUncertified(Capture(nullRoots));
    const std::array<uint64_t,3> roots{Root0,Root1,Description};
    Dag();Put(Handles0,tree::NativeHandle{Elements,0});CaptureRejected(roots);
    Dag();Put(Handles0,tree::NativeHandle{Elements,Control0+1});CaptureRejected(roots);
    Dag();References(Control0,0,1);CaptureRejected(roots);
    Dag();References(Control0,2,0);CaptureRejected(roots);
    Dag();References(Control0,-1,1);CaptureRejected(roots);
    Dag();References(Control0,1,1);CaptureRejected(roots);
    Dag();Put(Handles1,tree::NativeHandle{Elements,Control1});CaptureRejected(roots);
    Dag();Put(Handles0,tree::NativeHandle{Root1,Control0});CaptureRejected(roots);
    Dag();memory.erase(Control0);CaptureRejected(roots);
    Dag();Put(Root0+0x28,tree::NativeVector16{Handles0,1,3});CaptureRejected(roots);
    Dag();Put(ElementsData+96,uint64_t(0));CaptureRejected(roots);
    Dag();tree::Snapshot unguarded;
    Require(!unguarded.Capture(roots,Base,Read,false),"Unguarded child layout was captured");CheckUncertified(unguarded);
    Dag();const auto stale=CaptureDag();References(Control0,0,1);
    Require(!stale.MatchesBefore(Read) && owned::TreeCertified(stale) && owned::AllocatorsMatch(stale,Read),
        "Identity-only helper was mistaken for current child/control or allocator-liveness proof");
}
}

int main()try{
    InventoryAndEveryCertifiedTriple();CompositeRoutes();UnknownTriples();UnsupportedAllocatorAndStringRoutes();
    CurrentAllocatorIdentityChanges();InvalidChildTrees();
    std::puts("PASS owned execution certificate: 133 unique triples, 10 destructor routes, shared child DAG and exact array/CString allocator identities; no allocator-liveness or native-free authority");
    return 0;
}catch(const std::exception& error){std::fprintf(stderr,"%s\n",error.what());return 1;}
