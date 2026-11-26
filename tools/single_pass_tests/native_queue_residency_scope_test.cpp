#include "Render/NativeQueueResidencyScope.hpp"
#include <cstdio>
#include <cstring>
#include <map>
#include <source_location>
#include <stdexcept>
#include <string>

using namespace cvr::stereo::queue_residency;
static void Require(bool value,std::source_location at=std::source_location::current()){
    if(!value)throw std::runtime_error("native queue residency scope assertion failed at line "+std::to_string(at.line()));
}
struct Memory {
    std::map<uint64_t,uint8_t> bytes;
    uint64_t changeAt{};uint32_t changeRead{},readsAt{};
    template<class T> void Put(uint64_t address,const T& value){
        const auto* data=reinterpret_cast<const uint8_t*>(&value);
        for(size_t i=0;i<sizeof(value);++i)bytes[address+i]=data[i];
    }
    static bool Read(void* user,uint64_t address,void* output,size_t size){
        auto& memory=*static_cast<Memory*>(user);auto* data=static_cast<uint8_t*>(output);
        for(size_t i=0;i<size;++i){const auto found=memory.bytes.find(address+i);if(found==memory.bytes.end())return false;data[i]=found->second;}
        if(address==memory.changeAt && ++memory.readsAt==memory.changeRead)++data[0];
        return true;
    }
};
struct Fixture {
    static constexpr uint64_t Base=0x140000000,Registry=0x30000000,Manager=Registry+detail::ManagerOffset,Thread=17;
    static constexpr uint64_t Queue=0x70000,Lists=0x20000,Sets=0x21000,Set=0x40000,Set2=0x40100,Entries=0x41000;
    static constexpr uint64_t List=0x50000,List2=0x50100,Context=0x60000,FenceObject=0x90000,Fence=0x91000,Sync=0x80000;
    static constexpr uint64_t Resource=0xA0000,Record=Registry+0x2F1D8,Managed=Record+0x60,Generation=23;
    Memory memory;detail::Frame frame{Base+cvr::stereo::native_recording::detail::ExecuteReturn,0x10000,0x10091,Manager,Queue,2,0};
    detail::SetHeader header{UINT32_MAX,0,Entries,4,0,4096,1,0,0,0,Sync};
    Fixture(){
        memory.Put(Base+detail::RegistryRva,Registry);memory.Put(Registry+0x14104D0,Queue);
        memory.Put(frame.rbp+0x5F,Lists);memory.Put(frame.rbp+0x67,Sets);memory.Put(frame.rbp+0x6F,uint32_t(2));
        memory.Put(frame.rbp-0x41,Manager+0x138);memory.Put(frame.rbp-0x39,FenceObject);
        memory.Put(Manager+0x138+offsetof(RTL_CRITICAL_SECTION,OwningThread),Thread);
        memory.Put(Manager+0x138+offsetof(RTL_CRITICAL_SECTION,RecursionCount),int32_t(1));
        memory.Put(Manager+0xA8,Generation);memory.Put(Manager+0x190,Sync);
        memory.Put(FenceObject,Fence);memory.Put(FenceObject+8,uint64_t(5));
        memory.Put(Lists,std::array<uint64_t,2>{List,List2});memory.Put(Sets,std::array<uint64_t,2>{Set,Set2});
        memory.Put(Set,header);auto empty=header;empty.entries=0;empty.count=0;empty.bytes=0;memory.Put(Set2,empty);
        memory.Put(Entries,Managed);memory.Put(Context+0x30,List);memory.Put(Context+0x630,Set);
        memory.Put(Context+0x638,uint64_t(99));memory.Put(Context+0x68,uint32_t(1));
        memory.Put(Record,Resource);memory.Put(Record+0x10,uint64_t(0xB0000));memory.Put(Record+0x56,uint16_t(0x41));
        memory.Put(Managed,uint32_t(0));memory.Put(Managed+8,Resource);memory.Put(Managed+0x18,Generation);
    }
    bool Capture(detail::Snapshot& output,uint32_t* rejection=nullptr){return detail::ReadScope(Memory::Read,&memory,Base,Thread,frame,Queue,2,Lists,output,rejection);}
    bool Contains(const detail::Snapshot& scope,uint32_t* rejection=nullptr,std::array<uint64_t,12>* observed=nullptr){
        return detail::Contains(Memory::Read,&memory,scope,Context,List,Registry,1,Resource,rejection,observed);}
    bool ContainsRecording(const detail::Snapshot& scope,uint32_t* rejection=nullptr,std::array<uint64_t,12>* observed=nullptr){
        return detail::ContainsRecording(Memory::Read,&memory,scope,Context,List,Registry,rejection,observed);}
    bool Pageable(const detail::Snapshot& scope,uint64_t expected,detail::PageableMembership& result,uint32_t* rejection=nullptr,
                  std::array<uint64_t,12>* observed=nullptr){
        return detail::ReadPageableMembership(Memory::Read,&memory,scope,Context,List,Registry,expected,result,rejection,observed);}
};
static void PageableMembershipTests(){
    static constexpr uint64_t Heap=0xF0000,HeapManaged=0xE0000;
    Fixture source;detail::Snapshot scope;Require(source.Capture(scope));
    detail::PageableMembership captured,again;uint32_t rejected{};std::array<uint64_t,12> observed;
    Require(source.Pageable(scope,Fixture::Resource,captured,&rejected,&observed) && !rejected);
    Require(captured.managed==Fixture::Managed && captured.pageable==Fixture::Resource && !captured.memberIndex &&
        captured.list==Fixture::List && captured.sourceSet==Fixture::Set && captured.generation==Fixture::Generation);
    Require(source.Pageable(scope,Fixture::Resource,again) && captured==again);
    // The synthetic submitted bit demonstrates the intentional API split:
    // binding needs a pending native token, post-prefix POD checks do not.
    cvr::stereo::native_recording::detail::SubmissionState token;
    Require(token.BeforeSubmit(1,true) && token.Validating());token.Submitted();Require(!token.Validating());
    Require(source.Pageable(scope,Fixture::Resource,again) && captured==again);
    Fixture placed;placed.memory.Put(Fixture::Managed+8,uint64_t(0));
    placed.memory.Put(Fixture::Entries,HeapManaged);placed.memory.Put(HeapManaged,uint32_t(0));
    placed.memory.Put(HeapManaged+8,Heap);placed.memory.Put(HeapManaged+0x18,Fixture::Generation);
    Require(placed.Capture(scope) && placed.Pageable(scope,Heap,captured,&rejected,&observed));
    Require(captured.managed==HeapManaged && captured.pageable==Heap && observed[6]==Heap && observed[7]==Heap);
    Require(!placed.Contains(scope,&rejected) && rejected==334); // Preserve old texture-only semantics.
    Require(!placed.Pageable(scope,Fixture::Resource,again,&rejected) && rejected==409);
    Require(!placed.Pageable(scope,0,again,&rejected) && rejected==401);
    auto reject=[](auto mutate,uint32_t code){Fixture f;detail::Snapshot saved;Require(f.Capture(saved));mutate(f,saved);
        detail::PageableMembership result;uint32_t failure{};
        Require(!f.Pageable(saved,Fixture::Resource,result,&failure) && failure==code && !result.pageable);};
    reject([](auto&,auto& s){s.registry++;},401);
    reject([](auto&,auto& s){s.sets.pop_back();},401);
    reject([](auto&,auto& s){s.lists[1]=Fixture::List;},402);
    reject([](auto&,auto& s){s.lists[0]=Fixture::List2;},402);
    reject([](auto&,auto& s){s.sets[0].address=0;},402);
    reject([](auto& f,auto&){f.memory.bytes.erase(Fixture::Context+0x30);},403);
    reject([](auto& f,auto&){f.memory.Put(Fixture::Context+0x30,Fixture::List2);},404);
    reject([](auto& f,auto&){f.memory.Put(Fixture::Context+0x630,Fixture::Set2);},404);
    reject([](auto& f,auto&){f.memory.Put(Fixture::Context+0x638,uint64_t(0));},404);
    reject([](auto& f,auto&){f.memory.Put(Fixture::Context+0x68,uint32_t(3));},404);
    reject([](auto& f,auto&){f.memory.bytes.erase(Fixture::Set);},405);
    reject([](auto& f,auto&){f.header.open=1;f.memory.Put(Fixture::Set,f.header);},406);
    reject([](auto& f,auto&){f.header.entries+=8;f.memory.Put(Fixture::Set,f.header);},406);
    reject([](auto& f,auto&){f.memory.Put(Fixture::Entries,uint64_t(0));},407);
    reject([](auto& f,auto&){f.memory.Put(Fixture::Entries,Fixture::Managed+8);},407);
    reject([](auto& f,auto&){f.memory.bytes.erase(Fixture::Managed+8);},408);
    reject([](auto& f,auto&){f.memory.Put(Fixture::Managed+8,uint64_t(0));},409);
    reject([](auto& f,auto&){f.memory.Put(Fixture::Managed+8,Heap);},409);
    reject([](auto& f,auto&){f.memory.bytes.erase(Fixture::Managed);},411);
    reject([](auto& f,auto&){f.memory.bytes.erase(Fixture::Managed+0x18);},411);
    reject([](auto& f,auto&){f.memory.Put(Fixture::Managed,uint32_t(1));},412);
    reject([](auto& f,auto&){f.memory.Put(Fixture::Managed+0x18,Fixture::Generation+1);},412);
    for(const auto address:{Fixture::Set,Fixture::Entries,Fixture::Managed+8,Fixture::Managed,Fixture::Managed+0x18,
                           Fixture::Context+0x30,Fixture::Context+0x630,Fixture::Context+0x638,Fixture::Context+0x68}){
        reject([&](auto& f,auto&){f.memory.changeAt=address;f.memory.changeRead=2;},413);
    }
    for(bool sameManaged:{false,true}){
        Fixture duplicate;duplicate.header.count=2;duplicate.memory.Put(Fixture::Set,duplicate.header);
        duplicate.memory.Put(Fixture::Entries+8,sameManaged?Fixture::Managed:HeapManaged);
        duplicate.memory.Put(HeapManaged,uint32_t(0));duplicate.memory.Put(HeapManaged+8,Fixture::Resource);
        duplicate.memory.Put(HeapManaged+0x18,Fixture::Generation);
        Require(duplicate.Capture(scope) && !duplicate.Pageable(scope,Fixture::Resource,again,&rejected) && rejected==410);
    }
    Fixture peerOnly;auto empty=peerOnly.header;empty.entries=0;empty.count=0;empty.bytes=0;
    peerOnly.memory.Put(Fixture::Set,empty);peerOnly.memory.Put(Fixture::Set2,peerOnly.header);
    Require(peerOnly.Capture(scope) && !peerOnly.Pageable(scope,Fixture::Resource,again,&rejected) && rejected==409);
    Fixture changed;Require(changed.Capture(scope) && changed.Pageable(scope,Fixture::Resource,captured));
    changed.memory.Put(Fixture::Context+0x638,uint64_t(100));
    Require(changed.Pageable(scope,Fixture::Resource,again) && captured!=again);
    PendingPageableUse noAuthority;Require(!noAuthority);
}
static void InvocationPageableSearchTests(){
    static constexpr uint64_t Heap=0xF0000,Managed=0xE0000,Entries=0xD0000;
    Fixture f;detail::Snapshot scope;Require(f.Capture(scope));detail::PageableSearch report;
    Require(detail::SearchInvocationPageable(Memory::Read,&f.memory,scope,Fixture::List,Fixture::Resource,report));
    Require(report[0]==1 && report[1]==1 && !report[2] && report[6]==2 && report[7]==1 &&
        report[8]==1 && !report[9] && report[10]==1 && !report[11] && report[12]==0 &&
        report[14]==Fixture::Managed && report[18]==1 && !report[19]);
    // Absent in every set is valid inventory evidence, never membership.
    Require(detail::SearchInvocationPageable(Memory::Read,&f.memory,scope,Fixture::List,Heap,report));
    Require(report[1] && !report[8] && !report[9] && report[12]==UINT64_MAX && report[13]==UINT64_MAX && !report[18] && !report[19]);
    auto sibling=f.header;sibling.entries=Entries;
    f.memory.Put(Fixture::Set2,sibling);f.memory.Put(Entries,Managed);f.memory.Put(Managed,uint32_t(0));
    f.memory.Put(Managed+8,Heap);f.memory.Put(Managed+0x18,Fixture::Generation);Require(f.Capture(scope));
    detail::PageableMembership rejected;uint32_t code{};
    Require(!f.Pageable(scope,Heap,rejected,&code) && code==409);
    Require(detail::SearchInvocationPageable(Memory::Read,&f.memory,scope,Fixture::List,Heap,report));
    Require(!report[8] && report[9]==1 && !report[10] && report[11]==1 && report[12]==1 &&
        !report[13] && report[14]==Managed && report[15]==Fixture::List2 && !report[16] && report[17]==Fixture::Generation &&
        report[18]==1 && !report[19]);
    // Nonresident/older members remain visible but cannot appear qualified.
    f.memory.Put(Managed,uint32_t(1));f.memory.Put(Managed+0x18,Fixture::Generation-1);
    Require(detail::SearchInvocationPageable(Memory::Read,&f.memory,scope,Fixture::List,Heap,report));
    Require(report[9]==1 && !report[11] && report[16]==1 && report[17]==Fixture::Generation-1 && !report[19]);
    // Duplicate occurrences across sets are counted; distinct managed owners
    // remain a separate diagnostic and confer no permission.
    f.memory.Put(Managed,uint32_t(0));f.memory.Put(Managed+0x18,Fixture::Generation);
    sibling.count=2;f.memory.Put(Fixture::Set2,sibling);f.memory.Put(Entries+8,Managed);Require(f.Capture(scope));
    Require(detail::SearchInvocationPageable(Memory::Read,&f.memory,scope,Fixture::List,Heap,report));
    Require(report[9]==2 && report[11]==2 && report[18]==1 && !report[19]);
    auto malformed=scope;malformed.sets.pop_back();
    Require(!detail::SearchInvocationPageable(Memory::Read,&f.memory,malformed,Fixture::List,Heap,report) && report[2]==501);
    Require(!detail::SearchInvocationPageable(Memory::Read,&f.memory,scope,Fixture::List+8,Heap,report) && report[2]==502);
    Require(!detail::SearchInvocationPageable(Memory::Read,&f.memory,scope,Fixture::List,0,report) && report[2]==501);
    f.memory.Put(Entries,Managed+8);
    Require(!detail::SearchInvocationPageable(Memory::Read,&f.memory,scope,Fixture::List,Heap,report) && report[2]==504 && !report[1]);
    f.memory.Put(Entries,Managed);f.memory.bytes.erase(Managed+8);
    Require(!detail::SearchInvocationPageable(Memory::Read,&f.memory,scope,Fixture::List,Heap,report) && report[2]==505 && !report[1]);
    f.memory.Put(Managed+8,Heap);f.memory.changeAt=Managed+8;f.memory.changeRead=2;f.memory.readsAt=0;
    Require(!detail::SearchInvocationPageable(Memory::Read,&f.memory,scope,Fixture::List,Heap,report) && report[2]==506 && !report[1]);
    f.memory.changeAt=0;f.memory.Put(Fixture::Set2,detail::SetHeader{});
    Require(!detail::SearchInvocationPageable(Memory::Read,&f.memory,scope,Fixture::List,Heap,report) && report[2]==503 && !report[1]);
    Fixture outside;outside.memory.Put(Entries,Managed);outside.memory.Put(Managed+8,Heap);
    Require(outside.Capture(scope) && detail::SearchInvocationPageable(Memory::Read,&outside.memory,scope,Fixture::List,Heap,report) &&
        !report[9] && !report[18] && !report[19]);
}
int main()try{
    PageableMembershipTests();
    InvocationPageableSearchTests();
    Fixture good;detail::Snapshot saved;
    uint32_t successCode=UINT32_MAX;Require(good.Capture(saved,&successCode) && !successCode && good.Contains(saved));
    std::array<uint64_t,12> membership;membership.fill(UINT64_MAX);successCode=UINT32_MAX;
    Require(good.Contains(saved,&successCode,&membership) && !successCode);
    Require(membership==std::array<uint64_t,12>{Fixture::List,Fixture::List,Fixture::Set,Fixture::Set,99,1,
        Fixture::Resource,Fixture::Resource,0,Fixture::Generation,Fixture::Generation,0});
    Require(saved.sets.size()==2 && saved.sets[0].entries==std::vector<uint64_t>{Fixture::Managed});
    successCode=UINT32_MAX;membership.fill(UINT64_MAX);
    Require(good.ContainsRecording(saved,&successCode,&membership) && !successCode);
    Require(membership==std::array<uint64_t,12>{Fixture::List,Fixture::List,0,0,0,1,0,0,0,0,Fixture::Generation,UINT64_MAX});
    auto rejectRecording=[](auto mutate,uint32_t expected){Fixture value;detail::Snapshot scope;Require(value.Capture(scope));mutate(value,scope);
        uint32_t rejection{};std::array<uint64_t,12> observed;
        Require(!value.ContainsRecording(scope,&rejection,&observed) && rejection==expected);
        Require(observed[0]==Fixture::List && observed[11]==UINT64_MAX);};
    rejectRecording([](auto&,auto& scope){++scope.registry;},351);
    rejectRecording([](auto&,auto& scope){scope.lists[0]=Fixture::List2+8;},352);
    rejectRecording([](auto&,auto& scope){scope.lists[1]=Fixture::List;},353);
    rejectRecording([](auto& f,auto&){f.memory.bytes.erase(Fixture::Context+0x30);},354);
    rejectRecording([](auto& f,auto&){f.memory.Put(Fixture::Context+0x30,Fixture::List2);},355);
    rejectRecording([](auto& f,auto&){f.memory.bytes.erase(Fixture::Context+0x68);},356);
    rejectRecording([](auto& f,auto&){f.memory.Put(Fixture::Context+0x68,uint32_t(0));},357);
    rejectRecording([](auto& f,auto&){f.memory.Put(Fixture::Context+0x68,uint32_t(3));},357);
    Require(!detail::ContainsRecording(Memory::Read,&good.memory,saved,0,Fixture::List,Fixture::Registry));
    Require(!detail::ContainsRecording(Memory::Read,&good.memory,saved,Fixture::Context,0,Fixture::Registry));
    Require(!detail::ContainsRecording(Memory::Read,&good.memory,saved,Fixture::Context,Fixture::List,0));
    Require(!detail::ContainsRecording(Memory::Read,&good.memory,saved,Fixture::Context,Fixture::List2+8,Fixture::Registry));
    Require(!detail::ContainsRecording(Memory::Read,&good.memory,saved,Fixture::Context+8,Fixture::List,Fixture::Registry));
    Fixture untracked;untracked.memory.Put(Fixture::Sets,std::array<uint64_t,2>{0,0});detail::Snapshot untrackedScope;
    Require(untracked.Capture(untrackedScope));
    // Recording membership is independent of target-managed sets and pageable
    // records. Lease generation/pending validation belongs to the runtime wrapper.
    untracked.memory.bytes.clear();untracked.memory.Put(Fixture::Context+0x30,Fixture::List);
    untracked.memory.Put(Fixture::Context+0x68,uint32_t(1));Require(untracked.ContainsRecording(untrackedScope));
    untracked.memory.Put(Fixture::Context+0x68,uint32_t(2));Require(untracked.ContainsRecording(untrackedScope));
    // There is deliberately no memory at a master-set pointer and no insertion
    // mask in the managed object. Both are invalid witnesses after set close.
    detail::Snapshot again;Require(good.Capture(again) && detail::Same(saved,again));
    auto rejectCapture=[](auto mutate,uint32_t expected=0){Fixture value;mutate(value);detail::Snapshot result;uint32_t rejection{};
        Require(!value.Capture(result,&rejection) && rejection && (!expected || rejection==expected));};
    rejectCapture([](auto& f){++f.frame.rip;},12);
    rejectCapture([](auto& f){f.frame.status=int32_t(0x80004005);},13);
    rejectCapture([](auto& f){++f.frame.rsp;},14);
    rejectCapture([](auto& f){++f.frame.rbp;});
    rejectCapture([](auto& f){++f.frame.manager;},21);
    rejectCapture([](auto& f){++f.frame.queue;},15);
    rejectCapture([](auto& f){++f.frame.count;});
    rejectCapture([](auto& f){f.memory.Put(Fixture::Base+detail::RegistryRva,uint64_t(0));},20);
    rejectCapture([](auto& f){f.memory.Put(Fixture::Registry+0x14104D0,Fixture::Queue+8);},22);
    rejectCapture([](auto& f){f.memory.Put(f.frame.rbp+0x5F,Fixture::Lists+8);},30);
    rejectCapture([](auto& f){f.memory.Put(f.frame.rbp+0x67,uint64_t(0));},31);
    rejectCapture([](auto& f){f.memory.Put(f.frame.rbp+0x6F,uint32_t(1));},32);
    rejectCapture([](auto& f){f.memory.Put(f.frame.rbp-0x41,Fixture::Manager+0x110);},40);
    rejectCapture([](auto& f){f.memory.Put(Fixture::Manager+0x138+offsetof(RTL_CRITICAL_SECTION,OwningThread),Fixture::Thread+1);},41);
    rejectCapture([](auto& f){f.memory.Put(Fixture::Manager+0x138+offsetof(RTL_CRITICAL_SECTION,RecursionCount),int32_t(0));},42);
    rejectCapture([](auto& f){f.memory.Put(f.frame.rbp-0x39,uint64_t(0));},50);
    rejectCapture([](auto& f){f.memory.Put(Fixture::FenceObject,uint64_t(0));},51);
    rejectCapture([](auto& f){f.memory.Put(Fixture::FenceObject+8,uint64_t(0));},52);
    rejectCapture([](auto& f){f.memory.Put(Fixture::FenceObject+8,UINT64_MAX);});
    rejectCapture([](auto& f){f.memory.Put(Fixture::Manager+0xA8,UINT64_MAX);},53);
    rejectCapture([](auto& f){f.memory.Put(Fixture::Lists+8,Fixture::List);},34);
    rejectCapture([](auto& f){f.header.open=1;f.memory.Put(Fixture::Set,f.header);},61);
    rejectCapture([](auto& f){f.header.outOfMemory=1;f.memory.Put(Fixture::Set,f.header);});
    rejectCapture([](auto& f){f.header.index=0;f.memory.Put(Fixture::Set,f.header);});
    rejectCapture([](auto& f){f.header.count=-1;f.memory.Put(Fixture::Set,f.header);});
    rejectCapture([](auto& f){f.header.count=5;f.memory.Put(Fixture::Set,f.header);});
    rejectCapture([](auto& f){f.header.count=int32_t(detail::MaxSetObjects+1);f.header.capacity=UINT32_MAX;f.memory.Put(Fixture::Set,f.header);});
    rejectCapture([](auto& f){f.header.entries=0;f.memory.Put(Fixture::Set,f.header);});
    rejectCapture([](auto& f){f.header.syncManager=Fixture::Sync+8;f.memory.Put(Fixture::Set,f.header);});
    rejectCapture([](auto& f){f.memory.bytes.erase(Fixture::Entries);},63);
    rejectCapture([](auto& f){f.memory.Put(Fixture::Entries,uint64_t(0));},64);
    rejectCapture([](auto& f){f.header.entries=UINT64_MAX-3;f.memory.Put(Fixture::Set,f.header);});
    rejectCapture([](auto& f){f.memory.Put(Fixture::Manager+0x190,uint64_t(0));},54);
    rejectCapture([](auto& f){f.memory.bytes.erase(Fixture::Set);},60);
    rejectCapture([](auto& f){f.memory.bytes.erase(Fixture::Lists);},33);
    detail::Snapshot invalid;uint32_t argumentsCode{};
    Require(!detail::ReadScope(Memory::Read,&good.memory,Fixture::Base,Fixture::Thread,good.frame,Fixture::Queue,0,Fixture::Lists,invalid,&argumentsCode));
    Require(argumentsCode==11);

    auto rejectMember=[](auto mutate,uint32_t expected=0){Fixture value;detail::Snapshot scope;Require(value.Capture(scope));mutate(value);
        uint32_t rejection{};std::array<uint64_t,12> observed;
        Require(!value.Contains(scope,&rejection,&observed) && rejection && (!expected || rejection==expected));
        Require(observed[0]==Fixture::List && observed[2]==Fixture::Set && observed[10]==Fixture::Generation);};
    rejectMember([](auto& f){f.memory.Put(Fixture::Context+0x30,Fixture::List2);},311);
    rejectMember([](auto& f){f.memory.Put(Fixture::Context+0x630,Fixture::Set2);},313);
    rejectMember([](auto& f){f.memory.Put(Fixture::Context+0x638,uint64_t(0));},315);
    rejectMember([](auto& f){f.memory.Put(Fixture::Context+0x68,uint32_t(3));},317);
    rejectMember([](auto& f){f.memory.Put(Fixture::Record,Fixture::Resource+8);},332);
    rejectMember([](auto& f){f.memory.Put(Fixture::Managed+8,uint64_t(0));},334);
    rejectMember([](auto& f){f.memory.Put(Fixture::Managed+8,uint64_t(0xB0000));},334);
    rejectMember([](auto& f){f.memory.Put(Fixture::Managed,uint32_t(1));},336);
    rejectMember([](auto& f){f.memory.Put(Fixture::Managed+0x18,Fixture::Generation-1);},338);
    rejectMember([](auto& f){f.memory.Put(Fixture::Entries,Fixture::Resource);},342);
    rejectMember([](auto& f){f.memory.Put(Fixture::Entries,uint64_t(0xB0000));},342);
    rejectMember([](auto& f){f.header.open=1;f.memory.Put(Fixture::Set,f.header);},321);
    rejectMember([](auto& f){f.memory.bytes.erase(Fixture::Context+0x30);},310);
    rejectMember([](auto& f){f.memory.bytes.erase(Fixture::Context+0x630);},312);
    rejectMember([](auto& f){f.memory.bytes.erase(Fixture::Context+0x638);},314);
    rejectMember([](auto& f){f.memory.bytes.erase(Fixture::Context+0x68);},316);
    rejectMember([](auto& f){f.memory.bytes.erase(Fixture::Set);},320);
    rejectMember([](auto& f){f.memory.bytes.erase(Fixture::Record);},331);
    rejectMember([](auto& f){f.memory.bytes.erase(Fixture::Managed+8);},333);
    rejectMember([](auto& f){f.memory.bytes.erase(Fixture::Managed);},335);
    rejectMember([](auto& f){f.memory.bytes.erase(Fixture::Managed+0x18);},337);
    rejectMember([](auto& f){f.memory.bytes.erase(Fixture::Entries);},341);

    Fixture diagnostic;detail::Snapshot diagnosticScope;Require(diagnostic.Capture(diagnosticScope));uint32_t memberCode{};
    diagnostic.memory.Put(Fixture::Managed+8,uint64_t(0));
    Require(!diagnostic.Contains(diagnosticScope,&memberCode,&membership) && memberCode==334);
    Require(membership[6]==Fixture::Resource && !membership[7] && membership[9]==Fixture::Generation && !membership[11]);
    diagnostic=Fixture{};Require(diagnostic.Capture(diagnosticScope));diagnostic.memory.Put(Fixture::Managed+0x18,Fixture::Generation-1);
    Require(!diagnostic.Contains(diagnosticScope,&memberCode,&membership) && memberCode==338);
    Require(membership[9]==Fixture::Generation-1 && membership[10]==Fixture::Generation && !membership[11]);
    diagnostic=Fixture{};Require(diagnostic.Capture(diagnosticScope));diagnostic.memory.Put(Fixture::Context+0x630,Fixture::Set2);
    Require(!diagnostic.Contains(diagnosticScope,&memberCode,&membership) && memberCode==313);
    Require(membership[2]==Fixture::Set && membership[3]==Fixture::Set2 && membership[4]==99 && membership[5]==1);
    // Membership in the peer set cannot satisfy this source's own set contract.
    diagnostic=Fixture{};diagnostic.memory.Put(Fixture::Set2,diagnostic.header);
    auto emptyHeader=diagnostic.header;emptyHeader.entries=0;emptyHeader.count=0;emptyHeader.bytes=0;diagnostic.memory.Put(Fixture::Set,emptyHeader);
    Require(diagnostic.Capture(diagnosticScope));
    Require(!diagnostic.Contains(diagnosticScope,&memberCode,&membership) && memberCode==340 && membership[11]==UINT64_MAX);
    Require(membership[6]==Fixture::Resource && membership[7]==Fixture::Resource && membership[9]==Fixture::Generation);
    Require(!detail::Contains(Memory::Read,&good.memory,saved,Fixture::Context,Fixture::List,Fixture::Registry,0,Fixture::Resource));
    Require(!detail::Contains(Memory::Read,&good.memory,saved,Fixture::Context,Fixture::List,Fixture::Registry,32769,Fixture::Resource));
    Require(!detail::Contains(Memory::Read,&good.memory,saved,Fixture::Context,Fixture::List2,Fixture::Registry,1,Fixture::Resource));
    good.memory.Put(Fixture::Context+0x68,uint32_t(2));Require(good.Contains(saved));
    good.memory.Put(Fixture::Record+0x56,uint16_t(0xC1));Require(good.Contains(saved));
    good.memory.Put(Fixture::Managed+8,uint64_t(0));Require(!good.Contains(saved));

    Fixture changed;Require(changed.Capture(again));changed.memory.Put(Fixture::Manager+0xA8,Fixture::Generation+1);
    detail::Snapshot next;Require(changed.Capture(next) && !detail::Same(again,next));
    changed=Fixture{};Require(changed.Capture(again));changed.memory.Put(Fixture::FenceObject+8,uint64_t(6));
    Require(changed.Capture(next) && !detail::Same(again,next));
    changed=Fixture{};Require(changed.Capture(again));changed.memory.Put(Fixture::Entries,Fixture::Managed+0xB0);
    Require(changed.Capture(next) && !detail::Same(again,next));
    for(const auto code:detail::Contract)Require(code.rva && code.bytes && code.bytes<=2048 && code.hash);
    std::puts("PASS native live-frame scope, exact recording membership, closed source sets, fixed managed texture membership and failed-scope rejection");return 0;
}catch(const std::exception& error){std::fprintf(stderr,"%s\n",error.what());return 1;}
