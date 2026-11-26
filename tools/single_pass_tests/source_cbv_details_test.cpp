#include "Render/NativeSourceCbvDetails.hpp"
#include <cstdio>
#include <map>
#include <stdexcept>

using namespace cvr::stereo;
namespace inspect=cvr::stereo::source_cbv_details;
static void Require(bool value,const char* message){if(!value)throw std::runtime_error(message);}
struct Memory {
    std::map<uint64_t,uint8_t> bytes;uint64_t changing{};uint32_t hits{};size_t changingOffset{};
    template<class T>void Put(uint64_t at,T value){const auto* p=reinterpret_cast<const uint8_t*>(&value);for(size_t i=0;i<sizeof(value);++i)bytes[at+i]=p[i];}
    void Zero(uint64_t at,size_t count){for(size_t i=0;i<count;++i)bytes[at+i]=0;}
    static bool Read(void* user,uint64_t at,void* output,size_t count){auto& m=*static_cast<Memory*>(user);auto* p=static_cast<uint8_t*>(output);
        for(size_t i=0;i<count;++i){const auto found=m.bytes.find(at+i);if(found==m.bytes.end())return false;p[i]=found->second;}
        if(m.changing==at && ++m.hits==2 && m.changingOffset<count)p[m.changingOffset]^=1;return true;}
};
static void RecordAndContext(){
    Memory m;geometry::NativeBuffer native{0x9000,0x100000,848,0x20000,0x4000,7,3};
    const auto address=native.registry+0x5C0AF8+uint64_t(native.handle-1)*0xB0;m.Zero(address,0xB0);
    m.Put<uint32_t>(address,848);m.Put<uint8_t>(address+6,0x36);m.Put(address+8,native.gpu);m.Put(address+0x10,native.resource);
    auto record=inspect::detail::ReadRecord(Memory::Read,&m,native);
    Require(record.observation==inspect::Observation::Known && record.address==address,"Exact current B0 record rejected");
    native.bytes++;Require(inspect::detail::ReadRecord(Memory::Read,&m,native).observation==inspect::Observation::Mismatch,"Changed logical size accepted");native.bytes--;
    m.changing=address;Require(inspect::detail::ReadRecord(Memory::Read,&m,native).observation==inspect::Observation::Changed,"Torn B0 observation accepted");m.changing=0;m.hits=0;
    native.kind=4;native.gpu=native.resource=0;m.Put<uint8_t>(address+6,0x46);
    Require(inspect::detail::ReadRecord(Memory::Read,&m,native).observation==inspect::Observation::Known,"Transient B0 fields incorrectly treated as backing identity");
    native.handle=0;record=inspect::detail::ReadRecord(Memory::Read,&m,native);
    Require(record.observation==inspect::Observation::NotApplicable && !record.address,"Direct b5 invented a native B0");
    m.Put<uint64_t>(0x4030,0x5000);m.Put<uint64_t>(0x4060,0x6000);m.Put<uint32_t>(0x4068,1);m.Put<uint64_t>(0x4638,0);m.Put<uint32_t>(0x4534,0);
    const auto context=inspect::detail::ReadContext(Memory::Read,&m,0x4000,0x5000);
    Require(context.observation==inspect::Observation::Known && !context.submitted && !context.pending,"Known zero context fields mistaken for unknown");
    Require(inspect::detail::ReadContext(Memory::Read,&m,0x4000,0x5001).observation==inspect::Observation::Mismatch,"Foreign native list accepted");
    Require(inspect::detail::ReadContext(Memory::Read,&m,UINT64_MAX,0x5000).observation==inspect::Observation::Unreadable,"Context overflow accepted");
    Require(inspect::detail::StateValueKnown({0,2,3}) && !inspect::detail::StateValueKnown({0,5,3}),"Numeric COMMON and unknown state were collapsed");
    source_cbv::SourceCbvBacking saved,fresh;saved.recording.generation=fresh.recording.generation=9;
    saved.stamp.value[2]=fresh.stamp.value[2]=13;
    Require(inspect::detail::SameSource(saved,fresh),"Equal source metadata rejected");
    fresh.recording.generation++;Require(!inspect::detail::SameSource(saved,fresh),"Foreign source recording generation accepted");
    fresh=saved;fresh.stamp.value[2]++;Require(!inspect::detail::SameSource(saved,fresh),"Changed source descriptor origin accepted");
    fresh=saved;fresh.native.handle++;Require(!inspect::detail::SameSource(saved,fresh),"Changed source native handle accepted");
}
static void AllocationHeaders(){
    Memory m;const uint64_t base=0x140000000,allocation=0x7000,allocator=0x8000,registry=0x20000;
    // The 2.5 methods of the allocation and allocator vtables (detail::Methods).
    const std::array<uint64_t,5> a{0x2FAC534,0x2FAAF80,0x348220,0x3483A4,0x348244},b{0x2FAC534,0x2FAAF80,0x348220,0x18F3FB4,0x18F3F48};
    for(size_t i=0;i<5;++i){m.Put(base+inspect::detail::AllocationVtable+i*8,base+a[i]);m.Put(base+inspect::detail::AllocatorVtable+i*8,base+b[i]);}
    m.Zero(allocation,0x68);m.Put(allocation,base+inspect::detail::AllocationVtable);m.Put<uint32_t>(allocation+8,2);
    m.Put<uint64_t>(allocation+0x10,0x1234);m.Put<uint64_t>(allocation+0x18,65536);m.Put<uint64_t>(allocation+0x28,0x9000);
    auto observed=inspect::detail::ReadAllocation(Memory::Read,&m,base,allocation);
    Require(observed.observation==inspect::Observation::Known && observed.resource==0x9000 && observed.bytes==65536,"Known committed allocation header rejected");
    m.Zero(allocator,0x18);m.Put(allocator,base+inspect::detail::AllocatorVtable);m.Put<uint32_t>(allocator+8,3);m.Put<uint64_t>(allocator+0x10,0x1234);m.Put(registry+0x1410540,allocator);
    auto wrapper=inspect::detail::ReadAllocator(Memory::Read,&m,base,registry,observed);
    Require(wrapper.observation==inspect::Observation::Known && wrapper.associationKnown && wrapper.pimplMatches,"Allocator Pimpl association rejected");
    m.changing=allocation;m.changingOffset=8;m.hits=0;
    auto concurrent=inspect::detail::ReadAllocation(Memory::Read,&m,base,allocation);
    Require(concurrent.observation==inspect::Observation::Known && concurrent.references==3 && concurrent.resource==observed.resource &&
        inspect::detail::Field<uint32_t>(concurrent.raw,8)==3,"Concurrent AddRef was confused with changed allocation identity");
    m.changing=allocator;m.hits=0;wrapper=inspect::detail::ReadAllocator(Memory::Read,&m,base,registry,observed);
    Require(wrapper.observation==inspect::Observation::Known && wrapper.references==2 && wrapper.pimplMatches,
        "Concurrent Release was confused with changed allocator identity");
    for(const auto offset:{size_t(0),size_t(12),size_t(16)}){
        m.changingOffset=offset;m.hits=0;
        Require(inspect::detail::ReadAllocator(Memory::Read,&m,base,registry,observed).observation==inspect::Observation::Changed,
            "Refcount sampling ignored a vtable, padding or Pimpl mutation");
    }
    m.changing=allocation;m.changingOffset=0x28;m.hits=0;
    Require(inspect::detail::ReadAllocation(Memory::Read,&m,base,allocation).observation==inspect::Observation::Changed,
        "Refcount sampling ignored a resource replacement");
    m.changingOffset=8;m.hits=0;m.Put<uint32_t>(allocation+8,1);
    Require(inspect::detail::ReadAllocation(Memory::Read,&m,base,allocation).observation==inspect::Observation::Unsupported,
        "Observed final Release was admitted");
    m.hits=0;m.Put<uint32_t>(allocation+8,INT32_MAX-1);
    Require(inspect::detail::ReadAllocation(Memory::Read,&m,base,allocation).observation==inspect::Observation::Unsupported,
        "Refcount overflow in the second sample was admitted");
    m.hits=0;m.Put<uint32_t>(allocation+8,INT32_MAX);
    Require(inspect::detail::ReadAllocation(Memory::Read,&m,base,allocation).observation==inspect::Observation::Unsupported,
        "Invalid first refcount was hidden by a valid second sample");
    m.changing=0;m.hits=0;m.changingOffset=0;m.Put<uint32_t>(allocation+8,2);
    m.Put<uint64_t>(allocator+0x10,0x1235);wrapper=inspect::detail::ReadAllocator(Memory::Read,&m,base,registry,observed);
    Require(wrapper.observation==inspect::Observation::Known && wrapper.associationKnown && !wrapper.pimplMatches,"Foreign allocator association reported as matching");
    m.Put<uint64_t>(allocation,0xBEEF);Require(inspect::detail::ReadAllocation(Memory::Read,&m,base,allocation).observation==inspect::Observation::UnknownAbi,"Unknown allocation ABI interpreted");
    m.Put(allocation,base+inspect::detail::AllocationVtable);m.Put<uint32_t>(allocation+8,0);Require(inspect::detail::ReadAllocation(Memory::Read,&m,base,allocation).observation==inspect::Observation::Unsupported,"Retired allocation header accepted");
    m.Put<uint32_t>(allocation+8,2);m.Put<uint32_t>(allocation+0x60,1);m.Put<uint64_t>(allocation+0x48,0xA000);m.Put<uint64_t>(0xA038,0xB000);
    observed=inspect::detail::ReadAllocation(Memory::Read,&m,base,allocation);Require(observed.observation==inspect::Observation::Known && observed.heap==0xB000,"Placed allocation heap decode failed");
    m.Put<uint64_t>(base+inspect::detail::AllocationVtable+8,base+1);Require(inspect::detail::ReadAllocation(Memory::Read,&m,base,allocation).observation==inspect::Observation::UnknownAbi,"Wrong known-vtable method accepted");
}
static void AliasAndPool(){
    Memory m;const uint64_t payload=0x7000,control=0x8000,base=0x140000000,registry=0x20000;
    m.Zero(payload,0x38);m.Put<uint64_t>(payload+8,0x9000);m.Put<uint32_t>(payload+0x10,0xC0);m.Put<uint64_t>(payload+0x18,0x10000);m.Put<uint64_t>(payload+0x20,0x20000);m.Put<int32_t>(control,2);
    auto alias=inspect::detail::ReadAlias(Memory::Read,&m,payload,control);
    Require(alias.observation==inspect::Observation::Known && alias.allocation==0x9000,"Exact alias header rejected");
    m.Put<uint32_t>(payload+0x10,0x80);Require(inspect::detail::ReadAlias(Memory::Read,&m,payload,control).observation==inspect::Observation::Unsupported,"Unknown alias flags accepted");
    Require(inspect::detail::ReadAlias(Memory::Read,&m,payload,0).observation==inspect::Observation::Mismatch,"Missing alias control accepted");
    geometry::NativeBuffer native{0x9000,0x100200,16,registry,0x4000,1,1};inspect::Record record;record.observation=inspect::Observation::Known;
    record.bytes[6]=0x16;const uint32_t packed=1u<<16;std::memcpy(record.bytes.data()+0x40,&packed,4);
    uint32_t logical=16;std::memcpy(record.bytes.data(),&logical,4);std::memcpy(record.bytes.data()+8,&native.gpu,8);std::memcpy(record.bytes.data()+0x10,&native.resource,8);
    m.Put<uint32_t>(base+0x36F44C8,512);m.Put<uint8_t>(registry+0x14105C0,1);m.Put<uint64_t>(registry+0x1410548,0xA000);m.Put<uint32_t>(registry+0x1410550,1);m.Put<uint32_t>(registry+0x1410554,1);
    m.Zero(0xA000,144);m.Put<uint64_t>(0xA000,native.resource);m.Put<uint64_t>(0xA008,0xB000);m.Put<uint64_t>(0xA010,2);
    auto pool=inspect::detail::ReadPool(Memory::Read,&m,base,native,record,0x100000,262144);
    Require(pool.observation==inspect::Observation::Known && pool.slot==1 && pool.owner==0xB000 && pool.occupied,"Small non-292 pool payload rejected");
    native.bytes=1;logical=1;std::memcpy(record.bytes.data(),&logical,4);Require(inspect::detail::ReadPool(Memory::Read,&m,base,native,record,0x100000,262144).observation==inspect::Observation::Known,"One-byte observational pool range rejected");
    native.bytes=513;Require(inspect::detail::ReadPool(Memory::Read,&m,base,native,record,0x100000,262144).observation==inspect::Observation::Unsupported,"Unknown pool unit extent accepted");
    native.bytes=16;logical=16;std::memcpy(record.bytes.data(),&logical,4);m.Put<uint64_t>(0xA010,0);Require(inspect::detail::ReadPool(Memory::Read,&m,base,native,record,0x100000,262144).observation==inspect::Observation::Mismatch,"Unoccupied pool slot accepted");
    native.kind=4;Require(inspect::detail::ReadPool(Memory::Read,&m,base,native,record,0x100000,262144).observation==inspect::Observation::NotApplicable,"Transient B0 interpreted as arena pool owner");
}
int main()try{RecordAndContext();AllocationHeaders();AliasAndPool();std::puts("PASS pure current CBV detail observations");return 0;}
catch(const std::exception& error){std::fprintf(stderr,"%s\n",error.what());return 1;}
