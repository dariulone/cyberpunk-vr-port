#include "Render/NativeAllocatorBinding.hpp"
#include <cstdio>
#include <map>
#include <stdexcept>

namespace tree=cvr::stereo::definition_tree;
namespace binding=cvr::stereo::allocator_binding;
namespace {
constexpr uint64_t Base=0x100000000ull,Common=Base+binding::CommonRva,ArrayAllocator=Base+binding::VectorAllocatorRva;
constexpr uint64_t SmallBegin=0x100000,SmallEnd=0x110000,MiddleBegin=0x200000,MiddleEnd=0x220000;
constexpr uint64_t Driver0=0x100100,Driver1=0x100200,Elements=0x100300,Description=0x200100,Name=0x200300,Plain=0x100900;
constexpr uint64_t Control0=0x100500,Control1=0x200500,NullControl=0x100600;
constexpr uint64_t Handles0=0x101000,Handles1=0x104000,TopData=0x102000,ElementsData=0x103000;
constexpr uint64_t DescriptionData=0x201000,StringData=0x202001,Foreign=0x300000;
constexpr uint64_t DriverVtable=Base+0x3094850,ElementsVtable=Base+0x2B39F70,DescriptionVtable=Base+0x2B39DF0;
constexpr uint64_t CStringVtable=Base+0x2B3A038,PlainVtable=Base+0x2B39E80;
constexpr binding::LayoutFacts Facts{true,true,false};
std::map<uint64_t,uint8_t> memory;
struct Tracker {binding::Lifecycle value{17,0,0,false,true};uint32_t calls{},failAt{},throwAt{},changeAt{};};
Tracker tracker;
uint64_t reads{},failAt{},throwAt{},changeAt{},changeAddress{},changeValue{};
bool resetDuringRead{};
void Require(bool value,const char* message){if(!value)throw std::runtime_error(message);}
template<class T>void Put(uint64_t address,const T& value){
    const auto* bytes=reinterpret_cast<const uint8_t*>(&value);
    for(size_t i=0;i<sizeof(value);++i)memory[address+i]=bytes[i];
}
bool Read(uint64_t address,void* destination,size_t bytes){
    ++reads;if(reads==throwAt)throw std::runtime_error("Injected allocator read failure");
    if(reads==failAt)return false;
    if(reads==changeAt)Put(changeAddress,changeValue);
    if(resetDuringRead && reads==9)++tracker.value.resetEpoch;
    auto* out=static_cast<uint8_t*>(destination);
    for(size_t i=0;i<bytes;++i){const auto found=memory.find(address+i);if(found==memory.end())return false;out[i]=found->second;}
    return true;
}
bool Observe(void* context,binding::Lifecycle& value){
    auto& current=*static_cast<Tracker*>(context);++current.calls;
    if(current.calls==current.throwAt)throw std::runtime_error("Injected lifecycle sample failure");
    if(current.calls==current.failAt)return false;
    if(current.calls==current.changeAt)++current.value.incarnation;
    value=current.value;return true;
}
void Fresh(){
    memory.clear();tracker=Tracker{};reads=failAt=throwAt=changeAt=changeAddress=changeValue=0;resetDuringRead=false;
    Put(Base+tree::ObjectAllocatorRva,Common|uint64_t(5));Put(Base+tree::ControlAllocatorRva,Common|uint64_t(3));
    Put(Base+tree::StringAllocatorRva,Common|uint64_t(7));Put(Common+binding::PoolOffset,Base+binding::LivePoolVtableRva);
    Put(Common+binding::SmallBeginOffset,SmallBegin);Put(Common+binding::SmallEndOffset,SmallEnd);
    Put(Common+binding::MiddleBeginOffset,MiddleBegin);Put(Common+binding::MiddleEndOffset,MiddleEnd);
    Put(ArrayAllocator+0x20,Base+binding::VectorFreeRva);
}
bool Capture(binding::Receipt& receipt,binding::LayoutFacts facts=Facts){
    return receipt.Capture(Base,facts,tracker.value,Read,Observe,&tracker);
}
void Captured(binding::Receipt& receipt){Require(Capture(receipt),"Known allocator binding capture failed");}
void Methods(uint64_t vtable,uint64_t destructor,uint64_t work){Put(vtable,destructor);Put(vtable+8,work);}
void References(uint64_t control,int32_t strong=1){Put(control,strong);Put(control+4,int32_t(1));}
void Vector(uint64_t object,uint64_t offset,uint64_t data,uint32_t capacity,uint32_t count,uint32_t stride){
    Put(object+offset,tree::NativeVector16{capacity?data:ArrayAllocator,capacity,count});
    if(capacity)Put(data+uint64_t(capacity)*stride,ArrayAllocator);
}
void CString(uint64_t object=Name,uint64_t data=StringData){
    Put(object,CStringVtable);std::array<uint8_t,32> storage{};
    const uint32_t capacity=31,flags=0x40000005;
    std::memcpy(storage.data(),&data,8);std::memcpy(storage.data()+0x10,&capacity,4);std::memcpy(storage.data()+0x14,&flags,4);
    Put(object+0x20,storage);
}
void Dag(uint64_t plain=Plain){
    Fresh();Methods(DriverVtable,Base+tree::SceneDriverDestructorRva,Base+0x332D5C);
    Methods(ElementsVtable,Base+tree::RenderElementsDestructorRva,Base+0x23A938);
    Methods(DescriptionVtable,Base+tree::DescriptionOwnerDestructorRva,Base+0xC26DB8);
    Methods(CStringVtable,Base+tree::CStringDestructorRva,Base+0x58DA9C);Methods(PlainVtable,Base+0x983FD8,Base+0x7883B0);
    Put(Driver0,DriverVtable);Put(Driver1,DriverVtable);Put(Elements,ElementsVtable);Put(Description,DescriptionVtable);Put(plain,PlainVtable);
    Vector(Driver0,0x28,Handles0,4,3,16);Vector(Driver1,0x28,Handles1,1,1,16);
    Vector(Elements,0x30,ElementsData,2,1,48);Vector(Description,0x20,DescriptionData,2,1,48);CString();
    Put(Handles0,tree::NativeHandle{Elements,Control0});Put(Handles0+16,tree::NativeHandle{Name,Control1});
    Put(Handles0+32,tree::NativeHandle{0,NullControl});Put(Handles1,tree::NativeHandle{Elements,Control0});
    References(Control0,2);References(Control1);References(NullControl);Put(TopData+64,ArrayAllocator);
}
tree::Snapshot Tree(uint64_t plain=Plain){
    const std::array<uint64_t,4> roots{Driver0,Driver1,Description,plain};tree::Snapshot snapshot;
    Require(snapshot.Capture(roots,Base,Read,true),"Sparse allocator fixture tree capture failed");return snapshot;
}
constexpr tree::NativeVector16 Header{TopData,8,4};
binding::Receipt Bound(const tree::Snapshot& tree){
    binding::Receipt receipt;Captured(receipt);Require(receipt.BindSaved(Header,tree),"Known saved allocation inventory was rejected");return receipt;
}
void PositiveInventoryAndClassification(){
    Dag();const auto tree=Tree();const auto before=memory;auto receipt=Bound(tree);
    Require(memory==before && receipt.Captured() && receipt.SavedBound() && !receipt.AllExactExtents(),
        "Range receipt wrote native state or invented full object/control extents");
    Require(receipt.Allocations().size()==15 && receipt.SavedHeader()==Header,"Saved pointer inventory lost an allocation");
    size_t objects{},controls{},arrays{},strings{},top{};
    for(const auto& allocation:receipt.Allocations()){
        Require(allocation.region!=binding::Region::Unsupported && receipt.ClassifySpan(allocation.address,allocation.bytes)==allocation.region,
            "Saved pointer was not assigned its complete known span");
        if(allocation.kind==binding::Kind::Object){++objects;Require(allocation.extent==binding::ExtentKind::ParsedMinimum,"Node prefix called full heap size");}
        if(allocation.kind==binding::Kind::Control){++controls;Require(allocation.bytes==8 && allocation.extent==binding::ExtentKind::ParsedMinimum,"Control prefix called full heap size");}
        if(allocation.kind==binding::Kind::Array){++arrays;Require(allocation.extent==binding::ExtentKind::ExactAllocation,"Array capacity/trailer extent lost");}
        if(allocation.kind==binding::Kind::CString){++strings;Require(allocation.address==StringData && allocation.bytes==32,"CString used inline storage or guessed alignment");}
        if(allocation.kind==binding::Kind::TopVector){++top;Require(allocation.bytes==72,"Top allocation used count instead of capacity/trailer");}
    }
    Require(objects==6 && controls==3 && arrays==4 && strings==1 && top==1,"Shared/null controls or allocation families were lost");
    binding::Receipt current;Captured(current);
    Require(receipt.MatchesFresh(current) && receipt.MatchesBefore(Read,Observe,&tracker),"Fresh unchanged structural binding did not match");
    Require(receipt.ClassifyPointer(SmallBegin)==binding::Region::Small && receipt.ClassifyPointer(SmallEnd)==binding::Region::Unsupported &&
        receipt.ClassifyPointer(MiddleBegin)==binding::Region::Middle && receipt.ClassifyPointer(MiddleEnd)==binding::Region::Unsupported,
        "Native half-open endpoint classification changed");
    Require(receipt.ClassifySpan(SmallEnd-8,8)==binding::Region::Small && receipt.ClassifySpan(SmallEnd-8,9)==binding::Region::Unsupported,
        "Known extent crossed its arena endpoint");
    Require(receipt.ClassifySpan(SmallBegin+4095,2)==binding::Region::Small,"Invented a 4K page-boundary exclusion");
    Require(receipt.ClassifySpan(StringData,32)==binding::Region::Middle,"Byte-aligned CString free pointer was rounded/rejected");
    Require(receipt.ClassifySpan(UINT64_MAX-7,8)==binding::Region::Unsupported && receipt.ClassifySpan(SmallBegin,0)==binding::Region::Unsupported,
        "Overflow/zero extent was supported");
    for(const auto unsupported:{uint64_t(0),SmallBegin-1,SmallEnd,MiddleBegin-1,MiddleEnd,Foreign,UINT64_MAX})
        Require(receipt.ClassifyPointer(unsupported)==binding::Region::Unsupported,"Fallback/free pointer outside both reservations was supported");
    Require(!tree.NativeOwnershipProved() && !tree.DestructorSafetyProved(),"Range capture promoted observation to ownership");
}
void InvalidDomainsAndLifecycle(){
    for(unsigned invalid=0;invalid<14;++invalid){Fresh();binding::Receipt receipt;auto facts=Facts;
        switch(invalid){
        case 0:facts.globalsAndComponentGuarded=false;break;case 1:facts.allocationRoutesGuarded=false;break;
        case 2:tracker.value.incarnation=0;break;case 3:tracker.value.coverageComplete=false;break;
        case 4:tracker.value.closing=true;break;case 5:tracker.value.inFlight=1;break;
        case 6:Put(Common+binding::PoolOffset,uint64_t(0));break;
        case 7:Put(Common+binding::PoolOffset,Base+0x2FF9C10);break;
        case 8:Put(Common+binding::SmallBeginOffset,uint64_t(0));break;
        case 9:Put(Common+binding::SmallEndOffset,SmallBegin);break;
        case 10:Put(Common+binding::MiddleEndOffset,MiddleBegin-1);break;
        case 11:Put(Common+binding::MiddleBeginOffset,SmallEnd-8);break;
        case 12:Put(Common+binding::MiddleEndOffset,uint64_t(0));break;
        case 13:Put(Common+binding::SmallBeginOffset,UINT64_MAX);break;
        }
        Require(!Capture(receipt,facts) && !receipt.Captured() && !receipt.SavedBound() && receipt.Allocations().empty(),
            "Partial/unconstructed/reset/destructed allocator domain was admitted");
    }
    for(const auto global:{tree::ObjectAllocatorRva,tree::ControlAllocatorRva,tree::StringAllocatorRva}){Fresh();
        Put(Base+global,Common+8);binding::Receipt receipt;Require(!Capture(receipt),"Foreign category allocator global accepted");
    }
    Fresh();Put(Common+binding::SmallBeginOffset,SmallBegin+3);Put(Common+binding::SmallEndOffset,SmallEnd+5);
    Put(Common+binding::MiddleBeginOffset,MiddleBegin+1);Put(Common+binding::MiddleEndOffset,MiddleEnd+9);
    binding::Receipt unaligned;Require(Capture(unaligned),"Invented 64K/aligned reservation-bound requirements");
    Fresh();Put(Common+binding::SmallEndOffset,MiddleBegin);binding::Receipt adjacent;Captured(adjacent);
    Require(adjacent.ClassifyPointer(MiddleBegin)==binding::Region::Middle,"Adjacent half-open reservations were treated as overlapping");
    Dag();auto saved=Bound(Tree());++tracker.value.incarnation;binding::Receipt reinitialized;Captured(reinitialized);
    Require(!saved.MatchesFresh(reinitialized) && !saved.MatchesBefore(Read,Observe,&tracker),"Allocator reinitialization laundered old saved allocations");
    Dag();saved=Bound(Tree());++tracker.value.resetEpoch;binding::Receipt reset;Captured(reset);
    Require(!saved.MatchesFresh(reset),"Reset epoch change was hidden by equal globals/vptr/bounds");
    tracker.value.closing=true;Require(!saved.MatchesBefore(Read,Observe,&tracker),"Matching readable memory inferred allocator liveness after close");
}
void BindingDriftAndReadRollback(){
    const std::array<uint64_t,8> fields{Base+tree::ObjectAllocatorRva,Base+tree::ControlAllocatorRva,Base+tree::StringAllocatorRva,
        Common+binding::PoolOffset,Common+binding::SmallBeginOffset,Common+binding::SmallEndOffset,
        Common+binding::MiddleBeginOffset,Common+binding::MiddleEndOffset};
    for(const auto address:fields){Dag();auto saved=Bound(Tree());uint64_t value{};Require(Read(address,&value,8),"Missing binding drift field");
        Put(address,address==Common+binding::PoolOffset?Base+0x2FF9C10:address<Common?value^1:value+8);
        Require(!saved.MatchesBefore(Read,Observe,&tracker),"Changed raw global/vptr/bound matched old saved binding");
    }
    Fresh();binding::Receipt baseline;Captured(baseline);const auto captureReads=reads;
    for(uint64_t at=1;at<=captureReads;++at)for(const bool throws:{false,true}){Fresh();if(throws)throwAt=at;else failAt=at;
        binding::Receipt receipt;Require(!Capture(receipt) && !receipt.Captured() && receipt.Allocations().empty(),"Partial/throwing read retained a binding");
    }
    for(const auto address:fields){Fresh();uint64_t value{};Require(Read(address,&value,8),"Missing torn binding field");reads=0;
        changeAt=9;changeAddress=address;changeValue=address==Common+binding::PoolOffset?Base+0x2FF9C10:address<Common?value^1:value+8;
        binding::Receipt receipt;Require(!Capture(receipt) && !receipt.Captured(),"Torn global/vptr/arena bounds accepted");
    }
    for(uint32_t at=1;at<=2;++at)for(const bool throws:{false,true}){Fresh();if(throws)tracker.throwAt=at;else tracker.failAt=at;
        binding::Receipt receipt;Require(!Capture(receipt) && !receipt.Captured(),"Failed lifecycle observation admitted binding");
    }
    Fresh();tracker.changeAt=2;binding::Receipt changed;Require(!Capture(changed),"Incarnation changed during capture but matched");
    Fresh();resetDuringRead=true;binding::Receipt resetting;Require(!Capture(resetting),"Reset changed during physical reads but matched");
    Dag();auto recaptured=Bound(Tree());reads=0;failAt=1;
    Require(!Capture(recaptured) && !recaptured.Captured() && !recaptured.SavedBound() && recaptured.Allocations().empty(),
        "Failed recapture leaked old saved receipt metadata");
}
void UnsupportedPointersAndSpans(){
    Dag();const auto tree=Tree();binding::Receipt receipt;Captured(receipt);
    auto badHeader=Header;badHeader.data=Foreign;
    Require(!receipt.BindSaved(badHeader,tree) && !receipt.SavedBound() && receipt.Allocations().empty(),"Fallback top-vector allocation admitted");
    Dag(Foreign);Captured(receipt);Require(!receipt.BindSaved(Header,Tree(Foreign)),"Fallback node allocation admitted");
    Dag();Put(Handles0+32,tree::NativeHandle{0,Foreign});References(Foreign);Captured(receipt);
    Require(!receipt.BindSaved(Header,Tree()),"Fallback controlled-null allocation omitted/admitted");
    Dag();Vector(Description,0x20,Foreign,2,1,48);Captured(receipt);
    Require(!receipt.BindSaved(Header,Tree()),"Fallback nested array allocation admitted");
    Dag();CString(Name,Foreign+1);Captured(receipt);
    Require(!receipt.BindSaved(Header,Tree()),"Fallback byte-aligned CString allocation admitted");
    Dag();CString(Name,MiddleEnd-31);Captured(receipt);
    Require(!receipt.BindSaved(Header,Tree()) && receipt.Rejection()==binding::Failure::ExtentCrossesRegion,"CString extent crossed region endpoint");
    Dag();CString(Name,MiddleEnd-32);Captured(receipt);Require(receipt.BindSaved(Header,Tree()),"CString exact endpoint fit rejected");
    Dag();Captured(receipt);badHeader=Header;badHeader.data=Driver0;
    Require(!receipt.BindSaved(badHeader,Tree()) && receipt.Rejection()==binding::Failure::Overlap,"Top allocation overlapping parsed object admitted");
    Dag();Captured(receipt);badHeader=Header;badHeader.count-=1;
    Require(!receipt.BindSaved(badHeader,Tree()),"Header/root slot count mismatch admitted");
    Dag();Captured(receipt);badHeader=Header;badHeader.capacity=UINT32_MAX;
    Require(!receipt.BindSaved(badHeader,Tree()) && !receipt.SavedBound() && receipt.Allocations().empty(),"Huge known top span bypassed bounds");
    Dag();Captured(receipt);badHeader=Header;badHeader.data=UINT64_MAX-7;
    Require(!receipt.BindSaved(badHeader,Tree()),"Overflowing top allocation admitted");
    Dag();Captured(receipt);tree::Snapshot unready;Require(!receipt.BindSaved(Header,unready),"Unready tree admitted");
    Dag();Put(Base+tree::StringAllocatorRva,Common|uint64_t(1));const auto differentTree=Tree();Fresh();Captured(receipt);
    Require(!receipt.BindSaved(Header,differentTree),"CString global identity mismatch ignored during saved binding");
}
void ExactExtentFactsAndSentinels(){
    Dag();const auto tree=Tree();auto minimum=Bound(tree);std::vector<binding::ExtentFact> extents;
    for(const auto& allocation:minimum.Allocations())if(allocation.kind==binding::Kind::Object || allocation.kind==binding::Kind::Control)
        extents.push_back({allocation.kind,allocation.address,allocation.kind==binding::Kind::Object?uint64_t(128):uint64_t(16)});
    binding::Receipt exact;Captured(exact);
    Require(!exact.BindSaved(Header,tree,extents),"Unaudited supplied full allocation sizes were accepted");
    Captured(exact);Require(Capture(exact,{true,true,true}) && exact.BindSaved(Header,tree,extents) && exact.AllExactExtents(),
        "Explicit audited full allocation extents were not retained");
    for(const auto kind:{binding::Kind::Object,binding::Kind::Control}){
        auto bad=extents;const auto found=std::find_if(bad.begin(),bad.end(),[&](const binding::ExtentFact& fact){return fact.kind==kind;});
        Require(found!=bad.end(),"Missing extent fact family");found->bytes=4;
        Require(!exact.BindSaved(Header,tree,bad),"Supplied size smaller than parsed prefix accepted");
    }
    auto duplicate=extents;duplicate.push_back(extents.front());Require(!exact.BindSaved(Header,tree,duplicate),"Duplicate exact size fact accepted");
    auto stale=extents;stale.push_back({binding::Kind::Object,Foreign,128});Require(!exact.BindSaved(Header,tree,stale),"Unconsumed foreign exact size fact accepted");
    auto overflow=extents;overflow.front().bytes=UINT64_MAX;Require(!exact.BindSaved(Header,tree,overflow),"Overflowing exact extent accepted");
    auto overlap=extents;
    const auto driver=std::find_if(overlap.begin(),overlap.end(),[](const binding::ExtentFact& fact){return fact.address==Driver0;});
    driver->bytes=0x120;Require(!exact.BindSaved(Header,tree,overlap),"Known exact extent overlapping another object accepted");
    Dag(SmallEnd-8);Captured(minimum);Require(minimum.BindSaved(Header,Tree(SmallEnd-8)),"Plain minimum parsed prefix endpoint rejected");
    Require(Capture(exact,{true,true,true}),"Endpoint exact fact capture failed");
    const std::array<binding::ExtentFact,1> crosses{{{binding::Kind::Object,SmallEnd-8,16}}};
    Require(!exact.BindSaved(Header,Tree(SmallEnd-8),crosses),"Actual pointer classification was mistaken for larger extent validity");
    Fresh();tree::Snapshot empty;const std::array<uint64_t,0> roots{};
    Require(empty.Capture(roots,Base,Read,true),"Empty sentinel tree capture failed");Captured(minimum);
    Require(minimum.BindSaved({ArrayAllocator,0,0},empty) && minimum.Allocations().empty() && !minimum.AllExactExtents(),
        "Empty top allocator sentinel was fabricated as a heap allocation");
}
}

int main()try{
    PositiveInventoryAndClassification();InvalidDomainsAndLifecycle();BindingDriftAndReadRollback();
    UnsupportedPointersAndSpans();ExactExtentFactsAndSentinels();
    std::puts("PASS native allocator binding: exact globals/vptr/small-middle bounds and caller incarnation, all saved free pointers, known capacity/string spans and minimum object/control prefixes; no guessed pages, liveness or native-free authority");return 0;
}catch(const std::exception& error){std::fprintf(stderr,"%s\n",error.what());return 1;}
