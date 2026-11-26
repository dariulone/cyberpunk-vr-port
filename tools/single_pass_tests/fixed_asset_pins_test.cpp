#include "Render/NativeTextureRead.hpp"
#include <cstdio>
#include <map>
#include <stdexcept>
#include <vector>

using namespace cvr::stereo;
namespace d=texture_read::detail;
static void Require(bool value){if(!value)throw std::runtime_error("fixed asset pin assertion failed");}
struct Fixture {
    static constexpr uint64_t Base=0x140000000,Registry=0x30000000,Lock=Registry+0x2F180,Record=Registry+0x2F1D8;
    static constexpr uint64_t Resource=0x90000,Allocation=0xA0000,Allocator=0xB0000,Pimpl=0xC0000;
    static constexpr uint64_t Table=Registry+0xCEBF30;
    std::map<uint64_t,uint8_t> memory;
    std::vector<unsigned> events;
    unsigned retains{},releases{},unlocks{},ownedAllocator{},ownedAllocation{},outsideReleases{};
    unsigned failRetain{},mutateAfterRetain{};bool raceToZero{},raceIncrement{},orderError{},resetPending{},resetBlocked{};
    bool exceptionRead{},resourceHeld{},resourceAlive=true;
    template<class T> void Put(uint64_t address,const T& value){
        const auto* bytes=reinterpret_cast<const uint8_t*>(&value);
        for(size_t i=0;i<sizeof(value);++i)memory[address+i]=bytes[i];
    }
    template<class T> T Get(uint64_t address)const{
        T result{};auto* bytes=reinterpret_cast<uint8_t*>(&result);
        for(size_t i=0;i<sizeof(result);++i){const auto found=memory.find(address+i);if(found==memory.end())throw std::runtime_error("missing fixture byte");bytes[i]=found->second;}
        return result;
    }
    Fixture(){
        Put(Base+0x3BCDCA8,Registry);Put(Registry+1,uint8_t(0));Put(Registry+0x2F1C0,Lock);Put(Lock,uint8_t(0));
        Put(Record-8,int32_t(3));Put(Record,std::array<uint8_t,0x58>{});Put(Record,Resource);
        Put(Record+8,uint8_t(1));Put(Record+0x10,Allocation);Put(Record+0x48,d::ShaderReads);Put(Record+0x56,uint16_t(0x41));
        Put(Table,Lock);Put(Table+8,uint32_t(32));
        for(uint32_t i=0;i<32;++i){Put(Table+0x10+uint64_t(i)*0xB0,int32_t(-1));Put(Table+0x40+uint64_t(i)*0xB0,uint32_t(0));}
        Put(Allocation,std::array<uint8_t,0x68>{});Put(Allocation,Base+source_cbv_details::detail::AllocationVtable);
        Put(Allocation+8,uint32_t(1));Put(Allocation+0x10,Pimpl);Put(Allocation+0x18,uint64_t(0x10000));
        Put(Allocation+0x28,Resource);Put(Allocation+0x60,uint32_t(0));
        Put(Allocator,std::array<uint8_t,0x18>{});Put(Allocator,Base+source_cbv_details::detail::AllocatorVtable);
        Put(Allocator+8,uint32_t(1));Put(Allocator+0x10,Pimpl);Put(Registry+0x1410540,Allocator);
        constexpr std::array<uint64_t,5> allocationMethods{0x2FAC534,0x2FAAF80,0x348220,0x3483A4,0x348244};
        constexpr std::array<uint64_t,5> allocatorMethods{0x2FAC534,0x2FAAF80,0x348220,0x18F3FB4,0x18F3F48};
        for(size_t i=0;i<5;++i){Put(Base+source_cbv_details::detail::AllocationVtable+i*8,Base+allocationMethods[i]);
            Put(Base+source_cbv_details::detail::AllocatorVtable+i*8,Base+allocatorMethods[i]);}
    }
    static bool Read(void* user,uint64_t address,void* output,size_t size){
        auto& f=*static_cast<Fixture*>(user);
        if(f.exceptionRead && f.retains)throw std::runtime_error("injected read failure");
        auto* bytes=static_cast<uint8_t*>(output);
        for(size_t i=0;i<size;++i){const auto found=f.memory.find(address+i);if(found==f.memory.end())return false;bytes[i]=found->second;}
        return true;
    }
    static bool TryLock(void* user,uint64_t address)noexcept{
        auto& f=*static_cast<Fixture*>(user);f.events.push_back(1);
        if(address!=Lock || f.Get<uint8_t>(Lock))return false;
        f.Put(Lock,uint8_t(0xFF));return true;
    }
    static bool RetainHandle(void* user,uint64_t address,int32_t expected)noexcept{
        auto& f=*static_cast<Fixture*>(user);f.events.push_back(2);
        if(f.Get<uint8_t>(Lock)!=0xFF || address!=Record-8)f.orderError=true;
        if(f.raceToZero)f.Put(Record-8,int32_t(0));
        if(f.raceIncrement)f.Put(Record-8,expected+1);
        if(expected<=0 || expected>=INT32_MAX-1 || f.Get<int32_t>(address)!=expected)return false;
        f.Put(address,expected+1);++f.retains;return true;
    }
    static bool RetainOwner(void* user,uint64_t address,bool allocation)noexcept{
        auto& f=*static_cast<Fixture*>(user);const unsigned which=allocation?2:1;f.events.push_back(allocation?4:3);
        if(f.Get<uint8_t>(Lock)!=0xFF || f.Get<int32_t>(Record-8)<=0 || !f.retains ||
            address!=(allocation?Allocation:Allocator))f.orderError=true;
        if(allocation)++f.ownedAllocation;else ++f.ownedAllocator;
        f.Put(address+8,f.Get<uint32_t>(address+8)+1);
        if(allocation && f.mutateAfterRetain==1)f.Put(Record+0x10,Allocation+8);
        if(allocation && f.mutateAfterRetain==2)f.Put(Allocation+0x28,Resource+8);
        if(allocation && f.resetPending)f.resetBlocked=f.Get<uint8_t>(Lock)==0xFF;
        return f.failRetain!=which;
    }
    static void ReleaseHandle(void* user,uint32_t handle)noexcept{
        auto& f=*static_cast<Fixture*>(user);f.events.push_back(5);
        if(f.Get<uint8_t>(Lock)!=0xFF || handle!=1 || f.Get<int32_t>(Record-8)<=0)f.orderError=true;
        f.Put(Record-8,f.Get<int32_t>(Record-8)-1);++f.releases;
    }
    static void Unlock(void* user,uint64_t address)noexcept{
        auto& f=*static_cast<Fixture*>(user);f.events.push_back(6);
        if(address!=Lock || f.Get<uint8_t>(Lock)!=0xFF || f.releases!=f.retains)f.orderError=true;
        f.Put(Lock,uint8_t(0));++f.unlocks;
        if(f.resetPending){f.Put(Record-8,int32_t(-1));f.Put(Record,uint64_t(0));f.Put(Record+8,uint8_t(0));f.Put(Record+0x10,uint64_t(0));
            f.Put(Allocation+8,f.Get<uint32_t>(Allocation+8)-1);f.resourceAlive=f.Get<uint32_t>(Allocation+8)!=0 || f.resourceHeld;}
    }
    bool Run(d::FixedPinSnapshot& snapshot,uint32_t& rejection){
        const d::FixedPinOperations ops{Read,TryLock,RetainHandle,RetainOwner,ReleaseHandle,Unlock};
        return d::BootstrapFixedPins(ops,this,Base,{Registry,Resource,1},snapshot,rejection);
    }
    void Cleanup(){
        Require(!Get<uint8_t>(Lock));events.push_back(7);
        if(ownedAllocation){Put(Allocation+8,Get<uint32_t>(Allocation+8)-ownedAllocation);outsideReleases+=ownedAllocation;ownedAllocation=0;}
        if(ownedAllocator){Put(Allocator+8,Get<uint32_t>(Allocator+8)-ownedAllocator);outsideReleases+=ownedAllocator;ownedAllocator=0;}
        if(!Get<uint32_t>(Allocation+8) && !resourceHeld)resourceAlive=false;
    }
    void RetainResourceOutside(){
        Require(!Get<uint8_t>(Lock) && resourceAlive && ownedAllocation && Get<uint32_t>(Allocation+8)>0);
        events.push_back(8);resourceHeld=true;
    }
};
static void Success(){
    Fixture f;d::FixedPinSnapshot value;uint32_t rejected{};
    Require(f.Run(value,rejected) && !rejected && !f.orderError);
    Require(f.events==std::vector<unsigned>{1,2,3,4,5,6});
    Require(f.retains==1 && f.releases==1 && f.unlocks==1 && f.Get<int32_t>(Fixture::Record-8)==3);
    Require(value.handle.native.resource==Fixture::Resource && value.allocation.address==Fixture::Allocation &&
        value.allocator.address==Fixture::Allocator && f.ownedAllocation==1 && f.ownedAllocator==1 && !f.outsideReleases);
    f.RetainResourceOutside();f.Cleanup();Require(f.outsideReleases==2 && f.resourceAlive);
    auto changed=value;changed.allocation.references+=4;changed.allocator.references+=7;changed.handle.references+=2;
    Require(d::SameFixedPins(value,changed));
    changed=value;changed.handle.owner+=8;Require(!d::SameFixedPins(value,changed));
    changed=value;changed.allocation.address+=8;Require(!d::SameFixedPins(value,changed));
    changed=value;changed.allocation.resource+=8;Require(!d::SameFixedPins(value,changed));
    changed=value;changed.allocation.heap+=8;Require(!d::SameFixedPins(value,changed));
    changed=value;changed.allocator.address+=8;Require(!d::SameFixedPins(value,changed));
    changed=value;changed.allocator.pimpl+=8;Require(!d::SameFixedPins(value,changed));
    Fixture placed;constexpr uint64_t Block=0xD0000,Heap=0xE0000;
    placed.Put(Fixture::Allocation+0x60,uint32_t(1));placed.Put(Fixture::Allocation+0x40,uint64_t(73));
    placed.Put(Fixture::Allocation+0x48,Block);placed.Put(Block+0x38,Heap);
    Require(placed.Run(value,rejected) && value.allocation.kind==1 && value.allocation.heap==Heap &&
        value.allocation.block==Block && value.allocation.handle==73);placed.Cleanup();
}
template<class Mutate> static void Reject(Mutate mutate,uint32_t expected,bool nativePin=false,unsigned owners=0){
    Fixture f;mutate(f);d::FixedPinSnapshot value;uint32_t rejected{};
    Require(!f.Run(value,rejected) && rejected==expected && !value.handle.native.resource && !f.orderError);
    Require(f.retains==unsigned(nativePin) && f.releases==unsigned(nativePin) &&
        f.ownedAllocation+f.ownedAllocator==owners);
    if(expected!=2){Require(f.unlocks==1 && !f.Get<uint8_t>(Fixture::Lock));f.Cleanup();Require(f.outsideReleases==owners);}
}
static void Races(){
    Reject([](auto& f){f.Put(Fixture::Lock,uint8_t(0xFF));},2);
    for(const auto refs:{int32_t(-1),int32_t(0),INT32_MAX-1,INT32_MAX})Reject([&](auto& f){f.Put(Fixture::Record-8,refs);},3);
    Reject([](auto& f){f.raceToZero=true;},4);
    Reject([](auto& f){f.raceIncrement=true;},4);
    Reject([](auto& f){f.Put(Fixture::Record,uint64_t(0));},3);
    Reject([](auto& f){f.Put(Fixture::Record+8,uint8_t(2));},3);
    Reject([](auto& f){f.Put(Fixture::Record+0x10,uint64_t(0));},3);
    Reject([](auto& f){f.Put(Fixture::Record+0x56,uint16_t(0x0500));},3);
    Reject([](auto& f){f.Put(Fixture::Table+0x40,uint32_t(1));},3);
    Reject([](auto& f){f.Put(Fixture::Table,Fixture::Lock+1);},3);
    Reject([](auto& f){f.Put(Fixture::Allocation,Fixture::Base+source_cbv_details::detail::AllocationVtable+8);},5,true);
    Reject([](auto& f){f.Put(Fixture::Allocator+0x10,Fixture::Pimpl+8);},5,true);
    Reject([](auto& f){f.Put(Fixture::Allocation+0x28,Fixture::Resource+8);},5,true);
    Reject([](auto& f){f.Put(Fixture::Allocation+8,uint32_t(0));},5,true);
    Reject([](auto& f){f.Put(Fixture::Allocation+0x60,uint32_t(2));f.Put(Fixture::Allocation+0x58,uint64_t(0xE0000));},5,true);
    Reject([](auto& f){f.Put(Fixture::Allocator+8,uint32_t(INT32_MAX-1));},5,true);
    Reject([](auto& f){f.failRetain=1;},6,true,1);
    Reject([](auto& f){f.failRetain=2;},7,true,2);
    Reject([](auto& f){f.mutateAfterRetain=1;},8,true,2);
    Reject([](auto& f){f.mutateAfterRetain=2;},8,true,2);
    Fixture f;f.resetPending=true;d::FixedPinSnapshot value;uint32_t rejected{};
    Require(f.Run(value,rejected) && f.resetBlocked && !f.orderError && f.Get<int32_t>(Fixture::Record-8)==-1);
    // A forced reset after unlock no longer has our native handle reference to
    // corrupt, while the exact original allocation remains independently held.
    Require(f.releases==f.retains && f.ownedAllocation==1 && value.allocation.resource==Fixture::Resource);
    Require(!f.Get<uint64_t>(Fixture::Record) && f.Get<uint32_t>(Fixture::Allocation+8)==1 && f.resourceAlive);
    f.RetainResourceOutside();f.Cleanup();Require(f.resourceAlive && !f.Get<uint32_t>(Fixture::Allocation+8));
    Fixture throwing;throwing.exceptionRead=true;bool caught{};
    try{throwing.Run(value,rejected);}catch(const std::runtime_error&){caught=true;}
    Require(caught && throwing.retains==1 && throwing.releases==1 && throwing.unlocks==1 && !throwing.orderError);
    throwing.Cleanup();
}
static void FreshPublication(){
    Fixture f;f.Put(Fixture::Record-8,int32_t(1));f.Put(Fixture::Record,std::array<uint8_t,0x58>{});
    f.Put(Fixture::Record+0x56,uint16_t(0x0500));d::FixedPinSnapshot value;uint32_t rejected{};
    Require(!f.Run(value,rejected) && rejected==3 && !f.retains);
    f.Put(Fixture::Record,Fixture::Resource);f.Put(Fixture::Record+8,uint8_t(1));
    Require(!f.Run(value,rejected) && rejected==3 && !f.retains);
    f.Put(Fixture::Record+0x10,Fixture::Allocation);f.Put(Fixture::Record+0x48,d::ShaderReads);
    Require(!f.Run(value,rejected) && rejected==3 && !f.retains);
    f.Put(Fixture::Record+0x56,uint16_t(0xC1));
    Require(f.Run(value,rejected) && !rejected && f.Get<int32_t>(Fixture::Record-8)==1 && !f.orderError);f.Cleanup();
    texture_read::FixedAssetPins absent;Require(!absent);
}
int main()try{
    Success();Races();FreshPublication();
    std::puts("PASS fixed native positive-ref bootstrap, fresh-slot identity and release-before-unlock ordering");return 0;
}catch(const std::exception& error){std::fprintf(stderr,"%s\n",error.what());return 1;}
