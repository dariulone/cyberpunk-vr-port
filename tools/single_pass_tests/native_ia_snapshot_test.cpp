#include "Render/NativeIaSnapshot.hpp"
namespace cvr::stereo::native_probe {bool ObserveBindings(){return true;}}
#include "Render/StereoGeometryCommands.hpp"
#include "Render/CommandResources.hpp"
#include <d3d12sdklayers.h>
#include <d3dcompiler.h>
#include <dxgi1_6.h>
#include <algorithm>
#include <cstdio>
#include <cstring>
#include <map>
#include <stdexcept>
#include <string>
#include <vector>

using Microsoft::WRL::ComPtr;
using namespace cvr::stereo;
namespace ia=cvr::stereo::ia_snapshot;
static void Require(bool value,const char* message){if(!value)throw std::runtime_error(message);}
static void Check(HRESULT value){if(FAILED(value))throw std::runtime_error("HRESULT "+std::to_string(UINT(value)));}
static ia::Observation Fixed(){ia::Observation a;a.role=uint64_t(ia::Role::Index);a.slot=UINT_MAX;
    a.resource=a.nativeResource=a.allocationResource=0x1000;a.gpu=a.nativeGpu=0x100000;a.width=a.nativeBytes=a.allocationBytes=4096;
    a.viewGpu=a.gpu+8;a.viewBytes=12;a.strideOrFormat=DXGI_FORMAT_R16_UINT;a.comOffset=8;a.registry=0x2000;a.context=0x3000;
    a.handle=2;a.nativeKind=1;a.heap=D3D12_HEAP_TYPE_DEFAULT;a.creationKind=1;a.initialKnown=1;a.unprotected=1;
    a.allocation=0x4000;a.allocator=0x5000;a.allocationPimpl=a.allocatorPimpl=0x6000;
    a.stateValue=D3D12_RESOURCE_STATE_GENERIC_READ;a.stateStatus=8;a.stateKind=1;a.list=0x7000;a.generation=8;a.occurrence=9;
    a.flags=ia::detail::RequiredFlags;a.dimension=D3D12_RESOURCE_DIMENSION_BUFFER;a.height=a.depth=a.mips=a.shapeKnown=1;
    a.heapNodeMasks=(uint64_t(1)<<32)|1;return a;
}
class Counted final:public IUnknown {
    ULONG references_=1;UINT label_;std::vector<UINT>& released_;
public:
    Counted(UINT label,std::vector<UINT>& released):label_(label),released_(released){}
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** out)override{if(!out)return E_POINTER;*out=nullptr;
        if(id!=__uuidof(IUnknown))return E_NOINTERFACE;*out=this;AddRef();return S_OK;}
    ULONG STDMETHODCALLTYPE AddRef()override{return ++references_;}
    ULONG STDMETHODCALLTYPE Release()override{const auto value=--references_;if(!value){released_.push_back(label_);delete this;}return value;}
};
struct PoolMemory {
    std::map<uint64_t,uint8_t> bytes;
    uint64_t mutateOnRead{},mutateByte{};
    void PutBytes(uint64_t address,const void* value,size_t size){const auto* data=static_cast<const uint8_t*>(value);
        for(size_t i=0;i<size;++i)bytes[address+i]=data[i];}
    template<class T>void Put(uint64_t address,T value){PutBytes(address,&value,sizeof(value));}
    static bool Read(void* user,uint64_t address,void* value,size_t size){auto& memory=*static_cast<PoolMemory*>(user);auto* output=static_cast<uint8_t*>(value);
        for(size_t i=0;i<size;++i){const auto found=memory.bytes.find(address+i);if(found==memory.bytes.end())return false;output[i]=found->second;}
        if(address==memory.mutateOnRead){memory.bytes[memory.mutateByte]^=1;memory.mutateOnRead=0;}return true;}
};
struct GeometryPoolFixture {
    static constexpr uint64_t Base=0x140000000,Registry=0x100000,Data=0x900000,Gpu=0x80000000,Resource=0x3000,Owner=0x4000;
    PoolMemory memory;geometry::NativeBuffer native{Resource,Gpu+270*32768,25504,Registry,0x6000,7,1};
    std::array<uint8_t,0xB0> raw{};
    uint64_t width=33554432,viewGpu=native.gpu+24520,viewBytes=984;
    uint32_t sizeClass=3,group=2,slot=270;
    uint64_t Record()const{return Registry+0x5C0AF8+uint64_t(native.handle-1)*0xB0;}
    uint64_t Header()const{return Registry+0x1410548+uint64_t(sizeClass)*24;}
    uint64_t Entry()const{return Data+uint64_t(group)*144;}
    uint64_t Occupancy()const{return Entry()+16+uint64_t(slot/64)*8;}
    template<class T>void Field(size_t at,T value){std::memcpy(raw.data()+at,&value,sizeof(value));memory.Put(Record()+at,value);}
    GeometryPoolFixture(uint32_t poolClass=3,uint32_t poolSlot=270,uint32_t logicalBytes=25504,uint64_t resourceBytes=33554432){
        const std::array<uint32_t,5> units{512,2048,8192,32768,98304};sizeClass=poolClass;slot=poolSlot;width=resourceBytes;
        native.bytes=logicalBytes;native.gpu=Gpu+uint64_t(slot)*units[sizeClass];viewBytes=std::min(uint64_t(984),native.bytes);viewGpu=native.gpu+native.bytes-viewBytes;
        memory.Put(Base+0x3BCDCA8,Registry);memory.Put<uint8_t>(Registry+0x14105C0,1);memory.Put<uint8_t>(Registry+0x1AE2C69,0);
        memory.PutBytes(Base+0x36F44C8,units.data(),sizeof(units));
        raw[6]=0x15;memory.PutBytes(Record(),raw.data(),raw.size());Field(0,uint32_t(native.bytes));Field(8,native.gpu);Field(16,native.resource);
        Field(0x40,(slot<<16)|(group<<3)|sizeClass);memory.Put(Header(),Data);memory.Put<uint32_t>(Header()+8,4);memory.Put<uint32_t>(Header()+12,3);
        std::array<uint8_t,144> entry{};memory.PutBytes(Entry(),entry.data(),entry.size());memory.Put(Entry(),Resource);memory.Put(Entry()+8,Owner);
        memory.Put(Occupancy(),uint64_t(1)<<(slot%64));
    }
    bool Read(ia::detail::GeometryPool& output){return ia::detail::ReadGeometryPool(PoolMemory::Read,&memory,Base,native,raw,Gpu,width,viewGpu,viewBytes,output);}
};
static void GeometryPools(){
    {GeometryPoolFixture raw;raw.Field<uint8_t>(6,0x1D);raw.memory.Put<uint8_t>(raw.Registry+0x1AE2C69,1);
        ia::detail::GeometryPool pool;
        Require(!raw.Read(pool),"Raw material pool must not satisfy the IA contract");
        Require(ia::detail::ReadGeometryPool(PoolMemory::Read,&raw.memory,raw.Base,raw.native,raw.raw,raw.Gpu,raw.width,raw.viewGpu,raw.viewBytes,pool,13) &&
            pool.owner==raw.Owner,"Native raw-buffer pool remains eligible when the geometry-only pool switch is disabled");
        raw.memory.Put<uint64_t>(raw.Occupancy(),0);
        Require(!ia::detail::ReadGeometryPool(PoolMemory::Read,&raw.memory,raw.Base,raw.native,raw.raw,raw.Gpu,raw.width,raw.viewGpu,raw.viewBytes,pool,13),
            "Unoccupied raw material slot acquired a pool owner");}
    GeometryPoolFixture fixture;ia::detail::GeometryPool saved;
    Require(fixture.Read(saved) && saved.header==fixture.Header() && saved.data==fixture.Data && saved.entry==fixture.Entry() &&
        saved.resource==fixture.Resource && saved.owner==fixture.Owner && saved.sizeClass==3 && saved.group==2 && saved.slot==270 &&
        saved.unit==32768 && saved.slots==1024 && saved.packed==((270u<<16)|(2u<<3)|3u),"Geometry pool class/header/owner/occupancy layout differs");
    for(const auto example:std::array<std::array<uint32_t,4>,5>{{{0,1023,16,524288},{1,146,1048,1048576},{2,13,2114,8388608},
        {3,1023,32768,33554432},{4,255,66040,25165824}}}){
        GeometryPoolFixture other(example[0],example[1],example[2],example[3]);ia::detail::GeometryPool pool;
        Require(other.Read(pool) && pool.sizeClass==example[0] && pool.slot==example[1] && pool.owner==other.Owner,
            "Native geometry size class or final occupancy word rejected");}
    const auto reject=[](auto edit){GeometryPoolFixture bad;edit(bad);ia::detail::GeometryPool out;
        Require(!bad.Read(out) && out==ia::detail::GeometryPool{},"Invalid geometry pool acquired a live ownership observation");};
    reject([](auto& f){f.memory.template Put<uint8_t>(f.Registry+0x14105C0,0);});
    reject([](auto& f){f.memory.template Put<uint8_t>(f.Registry+0x1AE2C69,1);});
    reject([](auto& f){f.memory.Put(f.Base+0x3BCDCA8,f.Registry+8);});
    reject([](auto& f){f.Field(6,uint8_t(0x16));});
    reject([](auto& f){f.native.kind=3;});
    reject([](auto& f){f.native.handle=0;});
    reject([](auto& f){f.Field(0x18,uint64_t(0x4000));});
    reject([](auto& f){f.Field(0x20,uint64_t(0x4000));});
    reject([](auto& f){f.Field(0x28,uint64_t(0x4000));});
    reject([](auto& f){f.Field(0x40,(270u<<16)|(2u<<3)|5u);});
    reject([](auto& f){f.Field(0x40,(270u<<16)|(2u<<3)|2u);});
    reject([](auto& f){f.Field(0x40,(270u<<16)|(2u<<3)|4u);});
    reject([](auto& f){f.memory.template Put<uint32_t>(f.Base+0x36F44C8+12,8192);});
    reject([](auto& f){f.memory.template Put<uint32_t>(f.Base+0x36F44C8+16,98305);});
    reject([](auto& f){f.memory.Put(f.Header(),uint64_t(0));});
    reject([](auto& f){f.memory.Put(f.Header(),UINT64_MAX-144);});
    reject([](auto& f){f.memory.template Put<uint32_t>(f.Header()+8,8193);});
    reject([](auto& f){f.memory.template Put<uint32_t>(f.Header()+8,2);});
    reject([](auto& f){f.memory.template Put<uint32_t>(f.Header()+12,0);});
    reject([](auto& f){f.memory.template Put<uint32_t>(f.Header()+12,2);});
    reject([](auto& f){f.memory.Put(f.Entry(),f.Resource+8);});
    reject([](auto& f){f.memory.Put(f.Entry()+8,uint64_t(0));});
    reject([](auto& f){f.memory.Put(f.Occupancy(),uint64_t(0));});
    reject([](auto& f){f.native.resource+=8;});
    reject([](auto& f){f.memory.bytes.erase(f.Occupancy());});
    reject([](auto& f){f.memory.mutateOnRead=f.Entry();f.memory.mutateByte=f.Entry()+8;});
    reject([](auto& f){f.memory.mutateOnRead=f.Header();f.memory.mutateByte=f.Header()+8;});
    reject([](auto& f){f.memory.mutateOnRead=f.Record();f.memory.mutateByte=f.Record()+0x40;});
    reject([](auto& f){f.viewGpu=f.native.gpu-1;});
    reject([](auto& f){f.viewGpu=f.native.gpu+f.native.bytes;f.viewBytes=1;});
    reject([](auto& f){f.viewGpu=f.native.gpu+f.native.bytes-1;f.viewBytes=2;});
    reject([](auto& f){f.viewBytes=0;});
    reject([](auto& f){f.width-=1;});
    reject([](auto& f){f.width=uint64_t(32768)*1025;});
    reject([](auto& f){f.native.bytes=32769;f.Field(0,uint32_t(f.native.bytes));});
    reject([](auto& f){f.native.gpu=UINT64_MAX-16;f.viewGpu=f.native.gpu;f.Field(8,f.native.gpu);});
    fixture.viewGpu=fixture.native.gpu+fixture.native.bytes-2;fixture.viewBytes=2;
    ia::detail::GeometryPool last;Require(fixture.Read(last) && last==saved,"Last logical bytes lost their geometry pool association");
    fixture.memory.Put(fixture.Entry()+8,fixture.Owner+8);Require(fixture.Read(last) && last!=saved,"Changed pool owner retained its earlier observation");
    auto pooled=Fixed();pooled.flags=(pooled.flags&~uint64_t(ia::detail::FullBacking))|ia::detail::GeometryPoolKnown;pooled.lowType=5;
    pooled.width=pooled.allocationBytes=33554432;pooled.nativeGpu=pooled.gpu+270*32768;pooled.nativeBytes=25504;
    pooled.viewGpu=pooled.nativeGpu+24520;pooled.viewBytes=984;pooled.comOffset=pooled.viewGpu-pooled.gpu;
    pooled.poolOwner=pooled.allocation;pooled.poolSlot=270;pooled.poolUnit=32768;pooled.poolObservation=1;pooled.reserved=(270u<<16)|(2u<<3)|3u;
    Require(ia::detail::Admit(pooled)==ia::Failure::None,"Observed committed geometry pool range rejected");
    const auto deferred=[&](auto edit,ia::Failure expected){auto bad=pooled;edit(bad);Require(ia::detail::Admit(bad)==expected,"Unproven geometry pool admission reason differs");};
    deferred([](auto& a){a.flags&=~uint64_t(ia::detail::GeometryPoolKnown);},ia::Failure::PoolDeferred);
    deferred([](auto& a){a.poolObservation=8;},ia::Failure::PoolDeferred);
    deferred([](auto& a){a.poolOwner++;},ia::Failure::PoolDeferred);
    deferred([](auto& a){a.poolSlot=1024;},ia::Failure::PoolDeferred);
    deferred([](auto& a){a.reserved++;a.reserved=(a.reserved&~uint64_t(7))|5;},ia::Failure::PoolDeferred);
    deferred([](auto& a){a.nativeBytes=a.poolUnit+1;},ia::Failure::PoolDeferred);
    deferred([](auto& a){a.viewBytes++;},ia::Failure::BackingDeferred);
    deferred([](auto& a){a.allocationResource++;},ia::Failure::AllocationDeferred);
    deferred([](auto& a){a.allocationBytes=a.width-1;},ia::Failure::AllocationDeferred);
    deferred([](auto& a){a.allocationKind=1;},ia::Failure::AllocationDeferred);
    deferred([](auto& a){a.allocatorPimpl++;},ia::Failure::AllocatorDeferred);
    deferred([](auto& a){a.creationKind=2;},ia::Failure::PlacedDeferred);
    deferred([](auto& a){a.heap=D3D12_HEAP_TYPE_UPLOAD;},ia::Failure::UploadDeferred);
}
struct PlacedFixture {
    static constexpr uint64_t Base=0x140000000,ImageBytes=0x4EFC000,Metadata=0xA000,Vtable=Base+0x2B40000,Method=Base+0x870000;
    PoolMemory memory;
    ia::detail::PlacedAllocation expected{0x5000,0x3000,0x7000,0x8000,0x12345678,0x9000,196608};
    uint64_t offset=0x1730000,mutateAfterGetter{};UINT calls{};bool rejectGetter{},changeOffset{};
    PlacedFixture(){std::array<uint8_t,0x68> raw{};memory.PutBytes(expected.allocation,raw.data(),raw.size());
        memory.Put(expected.allocation,Base+0x30466A8);memory.Put<uint32_t>(expected.allocation+8,2);
        memory.Put(expected.allocation+0x10,expected.pimpl);memory.Put(expected.allocation+0x18,expected.bytes);
        memory.Put(expected.allocation+0x28,expected.resource);memory.Put(expected.allocation+0x40,expected.handle);
        memory.Put(expected.allocation+0x48,expected.block);memory.Put<uint32_t>(expected.allocation+0x60,1);
        memory.Put(expected.block+0x38,expected.heap);memory.Put(expected.block+0x40,Metadata);memory.Put(Metadata,Vtable);memory.Put(Vtable+0x30,Method);}
    static bool Read(void* user,uint64_t address,void* value,size_t bytes){return PoolMemory::Read(&static_cast<PlacedFixture*>(user)->memory,address,value,bytes);}
    static bool Get(void* user,uint64_t allocation,uint64_t& value){auto& fixture=*static_cast<PlacedFixture*>(user);++fixture.calls;
        if(fixture.rejectGetter || allocation!=fixture.expected.allocation)return false;value=fixture.offset;
        if(fixture.changeOffset)++fixture.offset;
        if(fixture.mutateAfterGetter){fixture.memory.bytes[fixture.mutateAfterGetter]^=1;fixture.mutateAfterGetter=0;}return true;}
    bool Query(ia::detail::PlacedAllocation& output){return ia::detail::ReadPlacedOffset(Read,Get,this,Base,ImageBytes,expected,output);}
};
static void PlacedAllocations(){
    PlacedFixture fixture;ia::detail::PlacedAllocation placed;
    Require(fixture.Query(placed) && fixture.calls==2 && placed.offset==fixture.offset && placed.offset!=placed.handle &&
        placed.metadata==fixture.Metadata && placed.metadataVtable==fixture.Vtable && placed.offsetMethod==fixture.Method,
        "Placed opaque handle was not resolved through the allocation offset getter");
    const auto reject=[](auto edit){PlacedFixture bad;edit(bad);ia::detail::PlacedAllocation output;
        Require(!bad.Query(output) && output==ia::detail::PlacedAllocation{},"Changed or unowned placed allocation acquired an offset proof");};
    reject([](auto& f){f.memory.Put(f.expected.allocation,f.Base+0x30466B0);});
    reject([](auto& f){f.memory.template Put<uint32_t>(f.expected.allocation+8,0);});
    reject([](auto& f){f.memory.template Put<uint32_t>(f.expected.allocation+0x60,0);});
    reject([](auto& f){f.memory.Put(f.expected.allocation+0x10,f.expected.pimpl+8);});
    reject([](auto& f){f.memory.Put(f.expected.allocation+0x18,f.expected.bytes-1);});
    reject([](auto& f){f.memory.Put(f.expected.allocation+0x28,f.expected.resource+8);});
    reject([](auto& f){f.memory.Put(f.expected.allocation+0x40,f.expected.handle+8);});
    reject([](auto& f){f.memory.Put(f.expected.allocation+0x48,f.expected.block+8);});
    reject([](auto& f){f.memory.Put(f.expected.block+0x38,f.expected.heap+8);});
    reject([](auto& f){f.memory.Put(f.expected.block+0x40,uint64_t(0));});
    reject([](auto& f){f.memory.Put(f.Metadata,uint64_t(0x100));});
    reject([](auto& f){f.memory.Put(f.Metadata,f.Base+f.ImageBytes-0x30);});
    reject([](auto& f){f.memory.Put(f.Vtable+0x30,f.Base+f.ImageBytes);});
    reject([](auto& f){f.rejectGetter=true;});
    reject([](auto& f){f.changeOffset=true;});
    reject([](auto& f){f.mutateAfterGetter=f.expected.block+0x40;});
    reject([](auto& f){f.mutateAfterGetter=f.Vtable+0x30;});
    reject([](auto& f){f.mutateAfterGetter=f.expected.allocation+0x28;});
    fixture.offset=0;Require(fixture.Query(placed) && !placed.offset,"Valid heap origin became an absent offset proof");
    auto admitted=Fixed();admitted.flags|=ia::detail::PlacedKnown;admitted.creationKind=2;admitted.creationHeap=admitted.allocationHeap=fixture.expected.heap;
    admitted.creationOffset=admitted.reserved=0x1730000;admitted.creationBytes=admitted.allocationBytes=196608;admitted.allocationKind=1;
    Require(ia::detail::Admit(admitted)==ia::Failure::None,"Pinned placed DEFAULT IA range rejected");
    const auto deferred=[&](auto edit,ia::Failure reason){auto bad=admitted;edit(bad);Require(ia::detail::Admit(bad)==reason,"Placed IA lost its exact deferred category");};
    deferred([](auto& a){a.flags&=~uint64_t(ia::detail::PlacedKnown);},ia::Failure::PlacedDeferred);
    deferred([](auto& a){a.reserved++;},ia::Failure::PlacedDeferred);
    deferred([](auto& a){a.creationOffset++;a.reserved=a.creationOffset;},ia::Failure::PlacedDeferred);
    deferred([](auto& a){a.creationOffset=a.reserved=UINT64_MAX-a.creationBytes+1;},ia::Failure::PlacedDeferred);
    deferred([](auto& a){a.allocationHeap++;},ia::Failure::PlacedDeferred);
    deferred([](auto& a){a.allocationBytes++;},ia::Failure::PlacedDeferred);
    deferred([](auto& a){a.allocationKind=0;},ia::Failure::PlacedDeferred);
    deferred([](auto& a){a.aliasPayload=1;},ia::Failure::AliasDeferred);
    deferred([](auto& a){a.nativeBytes--;},ia::Failure::BackingDeferred);
    deferred([](auto& a){a.stateStatus=9;},ia::Failure::EarlyFixedDeferred);
    deferred([](auto& a){a.heap=D3D12_HEAP_TYPE_UPLOAD;},ia::Failure::UploadDeferred);
    admitted.creationOffset=admitted.reserved=0;Require(ia::detail::Admit(admitted)==ia::Failure::None,"Verified offset zero was confused with a missing placement");
}
static void Pure(){
    {
        const std::array<ia::detail::CopyRange,5> input{{{1,18,100},{1,34,100},{1,134,12},{1,160,16},{2,18,100}}};
        ia::detail::CopyPlan plan;
        Require(ia::detail::PlanCopies(input,0,1024,plan) && plan.ranges.size()==3 && plan.copied==244,"Overlapping/touching IA spans were not coalesced exactly");
        Require(plan.ranges[0].sourceOffset==18 && plan.ranges[0].bytes==128 && plan.destinations[1]-plan.destinations[0]==16 &&
            plan.destinations[2]-plan.destinations[0]==116,"Coalesced IA addressing changed source byte offsets");
        for(size_t index=0;index<input.size();++index)Require(plan.destinations[index]%16==input[index].sourceOffset%16,"Source IA alignment was not preserved");
        Require(!ia::detail::PlanCopies(input,0,128,plan) && plan.ranges.empty(),"Truncated coalesced arena admitted");
        auto broken=input;broken[0].sourceOffset=UINT64_MAX;
        Require(!ia::detail::PlanCopies(broken,0,1024,plan),"Overflowing source union admitted");
    }
    uint32_t selected{};
    Require(ia::detail::Selection(143,128,selected) && selected==15,"Separate instance slot changed geometry mask");
    Require(ia::detail::Selection(143,0,selected) && selected==143,"Full IA selection changed mask");
    Require(!ia::detail::Selection(15,128,selected) && !selected,"Absent instance slot acquired separate ownership");
    Require(!ia::detail::Selection(143,1,selected) && !selected,"Non-instance omission admitted");
    GeometryPools();
    PlacedAllocations();
    Require(sizeof(ia::Observation)==512,"Observation is not POD64Q");
    const auto fixed=Fixed();Require(ia::detail::Admit(fixed)==ia::Failure::None,"Exact committed fixed-read shape rejected");
    for(UINT kind:{0u,3u}){auto a=fixed;a.nativeKind=a.stateKind=kind;a.stateStatus=2;a.stateValue=D3D12_RESOURCE_STATE_INDEX_BUFFER;
        Require(ia::detail::Admit(a)==ia::Failure::None,"Exact committed tracked index shape rejected");
        a.role=uint64_t(ia::Role::Vertex);a.slot=7;a.strideOrFormat=48;a.stateValue=D3D12_RESOURCE_STATE_VERTEX_AND_CONSTANT_BUFFER;
        Require(ia::detail::Admit(a)==ia::Failure::None,"Exact committed tracked vertex shape rejected");
        a.resourceFlags=D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;Require(ia::detail::Admit(a)==ia::Failure::None,"Read-only tracked UAV-capable backing rejected");
        a.stateValue=D3D12_RESOURCE_STATE_UNORDERED_ACCESS;Require(ia::detail::Admit(a)==ia::Failure::State,"Mutable write state acquired copy permission");}
    const auto reject=[&](ia::Observation a,ia::Failure expected){Require(ia::detail::Admit(a)==expected,"Deferred/invalid category reason differs");};
    auto a=fixed;a.heap=D3D12_HEAP_TYPE_UPLOAD;reject(a,ia::Failure::UploadDeferred);
    a=fixed;a.heap=5;reject(a,ia::Failure::UploadDeferred);
    a=fixed;a.creationKind=2;a.creationHeap=0x8000;reject(a,ia::Failure::PlacedDeferred);
    a=fixed;a.flags&=~ia::detail::CreationKnown;reject(a,ia::Failure::CreationDeferred);
    a=fixed;a.poolOwner=0x8000;reject(a,ia::Failure::PoolDeferred);
    a=fixed;a.aliasPayload=0x8000;reject(a,ia::Failure::AliasDeferred);
    a=fixed;a.unprotected=0;reject(a,ia::Failure::ProtectedDeferred);
    a=fixed;a.sessionPresent=1;reject(a,ia::Failure::ProtectedDeferred);
    a=fixed;a.heapFlags=D3D12_HEAP_FLAG_SHARED;reject(a,ia::Failure::ProtectedDeferred);
    a=fixed;a.stateStatus=9;reject(a,ia::Failure::EarlyFixedDeferred);
    a=fixed;a.stateStatus=5;reject(a,ia::Failure::State);
    a=fixed;a.nativeBytes--;reject(a,ia::Failure::BackingDeferred);
    a=fixed;a.allocationKind=1;reject(a,ia::Failure::AllocationDeferred);
    a=fixed;a.allocationResource++;reject(a,ia::Failure::AllocationDeferred);
    a=fixed;a.allocatorPimpl++;reject(a,ia::Failure::AllocatorDeferred);
    a=fixed;a.nativeKind=2;reject(a,ia::Failure::ClassDeferred);
    a=fixed;a.viewBytes=13;reject(a,ia::Failure::Arguments);
    a=fixed;a.strideOrFormat=DXGI_FORMAT_UNKNOWN;reject(a,ia::Failure::Arguments);
    a=fixed;a.viewGpu++;a.comOffset++;reject(a,ia::Failure::Arguments);
    a=fixed;a.comOffset++;reject(a,ia::Failure::Arguments);
    a=fixed;a.shapeKnown=0;reject(a,ia::Failure::BackingDeferred);
    a=fixed;a.gpu=UINT64_MAX-16;reject(a,ia::Failure::BackingDeferred);
    for(const auto flag:{ia::detail::ScopeCurrent,ia::detail::CodeKnown,ia::detail::ParentCurrent,ia::detail::BindingsCurrent,
        ia::detail::ContextKnown,ia::detail::RecordKnown,ia::detail::AllocationKnown,ia::detail::AllocatorKnown,ia::detail::SameDevice,ia::detail::FullBacking}){
        a=fixed;a.flags&=~uint64_t(flag);Require(ia::detail::Admit(a)!=ia::Failure::None,"Missing observed guard admitted metadata");}
    uint64_t offset{},end{};Require(ia::detail::Reserve(0,64,12,offset,end) && offset==0 && end==16,"Full IB view reservation differs");
    Require(ia::detail::Reserve(end,64,33,offset,end) && offset==16 && end==64,"Full VB reserved extent excludes padding");
    Require(!ia::detail::Reserve(end,64,1,offset,end),"Budget admitted one byte past full reserved extent");
    for(const auto capacity:{0ull,63ull,ia::detail::MaxArenaBytes+16})Require(!ia::detail::Reserve(0,capacity,16,offset,end),"Invalid arena capacity accepted");
    Require(!ia::detail::Reserve(1,64,16,offset,end) && !ia::detail::Reserve(0,64,0,offset,end),"Unaligned/empty view reservation accepted");
    D3D12_INDEX_BUFFER_VIEW index{0x100008,12,DXGI_FORMAT_R16_UINT},privateIndex{};D3D12_VERTEX_BUFFER_VIEW vertex{0x200020,768,48},privateVertex{};
    Require(ia::detail::IndexView(0x300000,0,784,index,privateIndex) && privateIndex.BufferLocation==0x300000 && privateIndex.SizeInBytes==12,"Private IB domain changed");
    Require(ia::detail::VertexView(0x300000,16,784,vertex,privateVertex) && privateVertex.BufferLocation==0x300010 && privateVertex.StrideInBytes==48 &&
        privateVertex.SizeInBytes==768,"Private VB domain changed");
    Require(!ia::detail::IndexView(UINT64_MAX-4,0,16,index,privateIndex) && !ia::detail::VertexView(0x300000,32,784,vertex,privateVertex),"Private view escaped its allocation");
    uint32_t required{};Require(ia::detail::RecoveryMask(3,{D3D12_RESOURCE_STATE_COPY_SOURCE,2,3},3,3,required) &&
        required==(3|D3D12_RESOURCE_STATE_COPY_SOURCE),"Recovery lost original combined IA reads");
    for(const geometry::ReadState state:std::array<geometry::ReadState,4>{{{0,4,3},{0,5,3},{D3D12_RESOURCE_STATE_UNORDERED_ACCESS,2,3},{3,2,0}}})
        Require(!ia::detail::RecoveryMask(3,state,3,3,required),"Unknown/writing/foreign state recovered silently");
    Require(ia::detail::BarrierCapacity(0x1000,32,0),"Bounded native transition storage rejected");
    Require(!ia::detail::BarrierCapacity(0,32,0) && !ia::detail::BarrierCapacity(0x1000,0,0) &&
        !ia::detail::BarrierCapacity(0x1000,1048577,0) && !ia::detail::BarrierCapacity(0x1000,32,1) &&
        !ia::detail::BarrierCapacity(UINT64_MAX-16,32,0),"Unknown/pending/overflowing transition storage accepted");
    geometry::NativeBuffer native{1,0x100000,4096,2,3,4,3};std::array<uint8_t,0xB0> raw{};raw[6]=0x36;
    const ia::detail::RecoveryContext before{3,5,6,0,1,0,true};auto now=before;now.pending=1;
    Require(ia::detail::RecoveryIdentity(native,raw,7,before,native,raw,7,now),"Exact pending recovery identity rejected");
    now.state++;Require(!ia::detail::RecoveryIdentity(native,raw,7,before,native,raw,7,now),"Foreign pending context acquired flush permission");
    now=before;auto changed=raw;changed[0x18]=1;Require(!ia::detail::RecoveryIdentity(native,raw,7,before,native,changed,7,now),"Changed allocation record acquired recovery permission");
    auto changedNative=native;changedNative.handle++;Require(!ia::detail::RecoveryIdentity(native,raw,7,before,changedNative,raw,7,now),"Reused native handle acquired recovery permission");
    ia::detail::Once once;Require(once.Begin() && !once.Begin(),"Copy recording was not once-only");once.Recorded();Require(!once.Begin(),"Recorded copy was replayed");
    ia::detail::Once failed;Require(failed.Begin(),"Failed fixture did not begin");failed.Failed();Require(!failed.Begin(),"Failed transaction revived");
    indirect_copy::detail::ScopeLedger scopes;const auto outer=scopes.Enter(1,2,3,4);
    Require(scopes.Matches(outer,1,2,3,4),"Exact source occurrence missing");const auto inner=scopes.Enter(1,2,4,4);
    Require(scopes.Exit(inner) && !scopes.Matches(outer,1,2,3,4),"Nested occurrence revived its outer copy scope");Require(scopes.Exit(outer),"Scope exit broke its stack");
    native_recording::detail::SubmissionState submitted;Require(submitted.BeforeSubmit(1,true),"First actual submission rejected");submitted.Submitted();
    Require(!submitted.BeforeSubmit(1,true),"Submitted native recording was replayed");
    std::vector<UINT> releases;{ia::detail::NativePins pins;pins.allocator.Attach(new Counted(2,releases));pins.allocation.Attach(new Counted(1,releases));
        {auto held=pins;Require(releases.empty(),"Copy pin destroyed a live native allocation");}Require(releases.empty(),"Temporary receipt lost original pins");}
    Require(releases==std::vector<UINT>({1,2}),"Allocator wrapper released before its native allocation");
}
static ComPtr<ID3D12Resource> Buffer(ID3D12Device* device,UINT64 bytes,D3D12_HEAP_TYPE type,D3D12_RESOURCE_STATES state,const void* data=nullptr){
    D3D12_HEAP_PROPERTIES heap{};heap.Type=type;D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;desc.Width=bytes;
    desc.Height=desc.DepthOrArraySize=desc.MipLevels=desc.SampleDesc.Count=1;desc.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;ComPtr<ID3D12Resource> resource;
    Check(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,state,nullptr,IID_PPV_ARGS(&resource)));
    if(data){void* mapped{};const D3D12_RANGE none{};Check(resource->Map(0,&none,&mapped));Require(mapped!=nullptr,"UPLOAD map failed");
        std::memcpy(mapped,data,SIZE_T(bytes));const D3D12_RANGE written{0,SIZE_T(bytes)};resource->Unmap(0,&written);}return resource;
}
static void Transition(ID3D12GraphicsCommandList* list,ID3D12Resource* resource,D3D12_RESOURCE_STATES before,D3D12_RESOURCE_STATES after){
    D3D12_RESOURCE_BARRIER barrier{};barrier.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;barrier.Transition={resource,D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,before,after};list->ResourceBarrier(1,&barrier);
}
static geometry::BufferLease Lease(ID3D12Resource* resource){geometry::BufferLease lease;lease.resource=resource;lease.gpu=resource->GetGPUVirtualAddress();lease.bytes=resource->GetDesc().Width;
    D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS flags{};Check(resource->GetHeapProperties(&heap,&flags));lease.heapType=heap.Type;lease.flags=resource->GetDesc().Flags;return lease;}
struct Gpu {
    ComPtr<ID3D12Device> device;ComPtr<ID3D12InfoQueue> info;ComPtr<ID3D12CommandQueue> queue;ComPtr<ID3D12CommandAllocator> allocator;
    ComPtr<ID3D12GraphicsCommandList> list;ComPtr<ID3D12Fence> fence;HANDLE event{};UINT64 value{};
    Gpu(){ComPtr<ID3D12Debug> debug;Check(D3D12GetDebugInterface(IID_PPV_ARGS(&debug)));debug->EnableDebugLayer();ComPtr<IDXGIFactory6> factory;
        Check(CreateDXGIFactory2(DXGI_CREATE_FACTORY_DEBUG,IID_PPV_ARGS(&factory)));ComPtr<IDXGIAdapter1> adapter;
        Check(factory->EnumAdapterByGpuPreference(0,DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE,IID_PPV_ARGS(&adapter)));DXGI_ADAPTER_DESC1 description{};
        Check(adapter->GetDesc1(&description));Require(!(description.Flags&DXGI_ADAPTER_FLAG_SOFTWARE),"Hardware adapter required");
        Check(D3D12CreateDevice(adapter.Get(),D3D_FEATURE_LEVEL_12_0,IID_PPV_ARGS(&device)));Check(device.As(&info));
        D3D12_COMMAND_QUEUE_DESC q{};q.Type=D3D12_COMMAND_LIST_TYPE_DIRECT;Check(device->CreateCommandQueue(&q,IID_PPV_ARGS(&queue)));
        Check(device->CreateCommandAllocator(q.Type,IID_PPV_ARGS(&allocator)));Check(device->CreateCommandList(0,q.Type,allocator.Get(),nullptr,IID_PPV_ARGS(&list)));
        Check(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&fence)));event=CreateEventW(nullptr,FALSE,FALSE,nullptr);Require(event!=nullptr,"Fence event failed");}
    ~Gpu(){if(event)CloseHandle(event);}
    void Execute(){Check(list->Close());ID3D12CommandList* lists[]{list.Get()};auto resources=cvr::gpu::PrepareCommandResources(1,lists);
        queue->ExecuteCommandLists(1,lists);cvr::gpu::SubmitCommandResources(queue.Get(),std::move(resources));Check(queue->Signal(fence.Get(),++value));
        Check(fence->SetEventOnCompletion(value,event));Require(WaitForSingleObject(event,30000)==WAIT_OBJECT_0,"GPU fence timed out");
        Require(fence->GetCompletedValue()!=UINT64_MAX && fence->GetCompletedValue()>=value,"Actual GPU completion missing");cvr::gpu::CollectCommandResources();cvr::gpu::ResetCommandResources(list.Get());}
    void Begin(){Check(allocator->Reset());Check(list->Reset(allocator.Get(),nullptr));}
    void Clean(){for(UINT64 i=0;i<info->GetNumStoredMessages();++i){SIZE_T bytes{};Check(info->GetMessage(i,nullptr,&bytes));std::vector<uint8_t> storage(bytes);
        auto* message=reinterpret_cast<D3D12_MESSAGE*>(storage.data());Check(info->GetMessage(i,message,&bytes));if(message->Severity<=D3D12_MESSAGE_SEVERITY_ERROR){
            std::fprintf(stderr,"%s\n",message->pDescription);throw std::runtime_error("GPU IA coordinate validation failed");}}}
};
static ComPtr<ID3DBlob> Shader(const char* source,const char* entry,const char* profile){ComPtr<ID3DBlob> code,errors;
    const auto hr=D3DCompile(source,std::strlen(source),nullptr,nullptr,nullptr,entry,profile,D3DCOMPILE_ENABLE_STRICTNESS|D3DCOMPILE_WARNINGS_ARE_ERRORS,0,&code,&errors);
    if(FAILED(hr) && errors)std::fprintf(stderr,"%s\n",static_cast<const char*>(errors->GetBufferPointer()));Check(hr);return code;}
static void GpuCoalesced(){
    Gpu gpu;std::array<uint8_t,512> bytes{};
    for(size_t index=0;index<bytes.size();++index)bytes[index]=uint8_t(index*13+7);
    auto original=Buffer(gpu.device.Get(),bytes.size(),D3D12_HEAP_TYPE_UPLOAD,D3D12_RESOURCE_STATE_GENERIC_READ,bytes.data());
    const uint64_t identity=reinterpret_cast<uintptr_t>(original.Get());
    const std::array<ia::detail::CopyRange,4> input{{{identity,18,100},{identity,34,100},{identity,134,12},{identity,176,32}}};
    ia::detail::CopyPlan plan;Require(ia::detail::PlanCopies(input,0,512,plan) && plan.ranges.size()==2 && plan.copied==160,"GPU copy union differs");
    auto copied=Buffer(gpu.device.Get(),plan.next,D3D12_HEAP_TYPE_DEFAULT,D3D12_RESOURCE_STATE_COPY_DEST);
    auto readback=Buffer(gpu.device.Get(),plan.next,D3D12_HEAP_TYPE_READBACK,D3D12_RESOURCE_STATE_COPY_DEST);
    for(const auto& range:plan.ranges)gpu.list->CopyBufferRegion(copied.Get(),range.destination,original.Get(),range.sourceOffset,range.bytes);
    Transition(gpu.list.Get(),copied.Get(),D3D12_RESOURCE_STATE_COPY_DEST,D3D12_RESOURCE_STATE_COPY_SOURCE);
    gpu.list->CopyBufferRegion(readback.Get(),0,copied.Get(),0,plan.next);gpu.Execute();
    void* mapped{};const D3D12_RANGE range{0,SIZE_T(plan.next)};Check(readback->Map(0,&range,&mapped));
    for(size_t index=0;index<input.size();++index)Require(!std::memcmp(static_cast<const uint8_t*>(mapped)+plan.destinations[index],
        bytes.data()+input[index].sourceOffset,size_t(input[index].bytes)),"Coalesced GPU view no longer contains its original bytes");
    const D3D12_RANGE none{};readback->Unmap(0,&none);gpu.Clean();
}
static void GpuCoordinates(){
    // Fixture-owned uploads/copies exercise private view coordinates only.
    // They do not fabricate a NativeIaSnapshot receipt or native admission.
    Gpu gpu;std::array<uint16_t,16> indices{};indices.fill(65535);indices[6]=4;indices[7]=5;indices[8]=6;
    std::array<uint8_t,800> instanceBytes{};for(UINT n=0;n<16;++n){const UINT marker=900+n;std::memcpy(instanceBytes.data()+32+n*48,&marker,4);}
    auto originalIndex=Buffer(gpu.device.Get(),sizeof(indices),D3D12_HEAP_TYPE_UPLOAD,D3D12_RESOURCE_STATE_GENERIC_READ,indices.data());
    auto originalVertex=Buffer(gpu.device.Get(),instanceBytes.size(),D3D12_HEAP_TYPE_UPLOAD,D3D12_RESOURCE_STATE_GENERIC_READ,instanceBytes.data());
    const float positions[]{-1,-1,-1,3,3,-1};
    auto originalPositions=Buffer(gpu.device.Get(),sizeof(positions),D3D12_HEAP_TYPE_UPLOAD,D3D12_RESOURCE_STATE_GENERIC_READ,positions);
    const D3D12_INDEX_BUFFER_VIEW index{originalIndex->GetGPUVirtualAddress()+8,12,DXGI_FORMAT_R16_UINT};
    const D3D12_VERTEX_BUFFER_VIEW vertex{originalVertex->GetGPUVirtualAddress()+32,768,48};uint64_t indexOffset{},cursor{},vertexOffset{},end{};
    Require(ia::detail::Reserve(0,784,index.SizeInBytes,indexOffset,cursor) && ia::detail::Reserve(cursor,784,vertex.SizeInBytes,vertexOffset,end) && end==784,"GPU full-view reservation differs");
    uint64_t positionOffset{},positionEnd{};
    const D3D12_VERTEX_BUFFER_VIEW positionView{originalPositions->GetGPUVirtualAddress(),sizeof(positions),8};
    Require(ia::detail::Reserve(end,1024,sizeof(positions),positionOffset,positionEnd),"Position view reservation failed");end=positionEnd;
    auto privateBuffer=Buffer(gpu.device.Get(),end,D3D12_HEAP_TYPE_DEFAULT,D3D12_RESOURCE_STATE_COPY_DEST);
    gpu.list->CopyBufferRegion(privateBuffer.Get(),indexOffset,originalIndex.Get(),8,index.SizeInBytes);
    gpu.list->CopyBufferRegion(privateBuffer.Get(),vertexOffset,originalVertex.Get(),32,vertex.SizeInBytes);
    gpu.list->CopyBufferRegion(privateBuffer.Get(),positionOffset,originalPositions.Get(),0,sizeof(positions));
    Transition(gpu.list.Get(),privateBuffer.Get(),D3D12_RESOURCE_STATE_COPY_DEST,D3D12_RESOURCE_STATE_GENERIC_READ);
    Require(cvr::gpu::KeepCommandResources(gpu.list.Get(),{originalIndex.Get(),originalVertex.Get(),originalPositions.Get(),privateBuffer.Get()}),"Owning fixture copy resources lost");gpu.Execute();gpu.Begin();
    D3D12_INDEX_BUFFER_VIEW privateIndex{};D3D12_VERTEX_BUFFER_VIEW privateVertex{};
    Require(ia::detail::IndexView(privateBuffer->GetGPUVirtualAddress(),indexOffset,end,index,privateIndex) &&
        ia::detail::VertexView(privateBuffer->GetGPUVirtualAddress(),vertexOffset,end,vertex,privateVertex),"GPU private view relocation failed");
    D3D12_VERTEX_BUFFER_VIEW privatePositions{};
    Require(ia::detail::VertexView(privateBuffer->GetGPUVirtualAddress(),positionOffset,end,positionView,privatePositions),"Private position view relocation failed");
    constexpr const char* hlsl=R"(
struct Output { float4 position:SV_Position; nointerpolation uint marker:TEXCOORD0; };
Output VS(float2 p:POSITION,uint marker:INSTANCE) { Output o;
  o.position=float4(p,0,1);o.marker=marker;return o; }
uint4 PS(Output input):SV_Target { return uint4(input.marker,43,47,53); }
)";
    auto vs=Shader(hlsl,"VS","vs_5_0"),ps=Shader(hlsl,"PS","ps_5_0");D3D12_ROOT_SIGNATURE_DESC rootDesc{};rootDesc.Flags=D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT;
    ComPtr<ID3DBlob> serialized,errors;Check(D3D12SerializeRootSignature(&rootDesc,D3D_ROOT_SIGNATURE_VERSION_1,&serialized,&errors));ComPtr<ID3D12RootSignature> root;
    Check(gpu.device->CreateRootSignature(0,serialized->GetBufferPointer(),serialized->GetBufferSize(),IID_PPV_ARGS(&root)));
    const D3D12_INPUT_ELEMENT_DESC elements[]{{"INSTANCE",0,DXGI_FORMAT_R32_UINT,7,0,D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA,1},
        {"POSITION",0,DXGI_FORMAT_R32G32_FLOAT,0,0,D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,0}};
    D3D12_GRAPHICS_PIPELINE_STATE_DESC pso{};pso.pRootSignature=root.Get();pso.VS={vs->GetBufferPointer(),vs->GetBufferSize()};pso.PS={ps->GetBufferPointer(),ps->GetBufferSize()};
    pso.InputLayout={elements,2};pso.PrimitiveTopologyType=D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE;pso.SampleMask=UINT_MAX;pso.SampleDesc.Count=1;
    pso.NumRenderTargets=1;pso.RTVFormats[0]=DXGI_FORMAT_R32G32B32A32_UINT;pso.RasterizerState.FillMode=D3D12_FILL_MODE_SOLID;pso.RasterizerState.CullMode=D3D12_CULL_MODE_NONE;pso.RasterizerState.DepthClipEnable=TRUE;
    auto& blend=pso.BlendState.RenderTarget[0];blend.RenderTargetWriteMask=15;blend.SrcBlend=blend.SrcBlendAlpha=D3D12_BLEND_ONE;blend.DestBlend=blend.DestBlendAlpha=D3D12_BLEND_ZERO;
    blend.BlendOp=blend.BlendOpAlpha=D3D12_BLEND_OP_ADD;blend.LogicOp=D3D12_LOGIC_OP_NOOP;pso.DepthStencilState.DepthFunc=D3D12_COMPARISON_FUNC_ALWAYS;
    pso.DepthStencilState.FrontFace=pso.DepthStencilState.BackFace={D3D12_STENCIL_OP_KEEP,D3D12_STENCIL_OP_KEEP,D3D12_STENCIL_OP_KEEP,D3D12_COMPARISON_FUNC_ALWAYS};
    ComPtr<ID3D12PipelineState> pipeline;Check(gpu.device->CreateGraphicsPipelineState(&pso,IID_PPV_ARGS(&pipeline)));
    std::array<geometry::DrawResources,2> draws;for(UINT n=0;n<2;++n){auto& source=draws[n];source.index=Lease(n?privateBuffer.Get():originalIndex.Get());
        source.vertices[7]=Lease(n?privateBuffer.Get():originalVertex.Get());source.state.original=pipeline;auto& draw=source.draw;
        source.vertices[0]=Lease(n?privateBuffer.Get():originalPositions.Get());
        draw.flags=176;draw.inputSlotMask=129;draw.indices=3;draw.instances=1;draw.firstIndex=2;draw.baseVertex=-4;draw.firstInstance=7;
        draw.vertices[0]=n?privatePositions:positionView;
        draw.index=n?privateIndex:index;draw.vertices[7]=n?privateVertex:vertex;draw.pipeline=reinterpret_cast<uintptr_t>(pipeline.Get());}
    StereoGeometryCommands commands;Check(commands.CreateSources(gpu.device.Get(),draws));Require(commands.SourceCount()==2 && !commands.InstanceBytes(),"Private IA changed source coordinates");
    ComPtr<ID3D12DescriptorHeap> rtvs;D3D12_DESCRIPTOR_HEAP_DESC heapDesc{D3D12_DESCRIPTOR_HEAP_TYPE_RTV,2,D3D12_DESCRIPTOR_HEAP_FLAG_NONE,0};Check(gpu.device->CreateDescriptorHeap(&heapDesc,IID_PPV_ARGS(&rtvs)));
    const auto step=gpu.device->GetDescriptorHandleIncrementSize(heapDesc.Type);std::array<ComPtr<ID3D12Resource>,2> colors,readbacks;
    std::array<D3D12_PLACED_SUBRESOURCE_FOOTPRINT,2> layouts;std::array<D3D12_CPU_DESCRIPTOR_HANDLE,2> handles;
    D3D12_RESOURCE_DESC colorDesc{};colorDesc.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D;colorDesc.Width=colorDesc.Height=4;
    colorDesc.DepthOrArraySize=colorDesc.MipLevels=colorDesc.SampleDesc.Count=1;colorDesc.Format=DXGI_FORMAT_R32G32B32A32_UINT;colorDesc.Flags=D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
    D3D12_HEAP_PROPERTIES colorHeap{};colorHeap.Type=D3D12_HEAP_TYPE_DEFAULT;
    for(UINT n=0;n<2;++n){Check(gpu.device->CreateCommittedResource(&colorHeap,D3D12_HEAP_FLAG_NONE,&colorDesc,D3D12_RESOURCE_STATE_RENDER_TARGET,nullptr,IID_PPV_ARGS(&colors[n])));
        handles[n]={rtvs->GetCPUDescriptorHandleForHeapStart().ptr+SIZE_T(n)*step};gpu.device->CreateRenderTargetView(colors[n].Get(),nullptr,handles[n]);UINT64 bytes{};
        gpu.device->GetCopyableFootprints(&colorDesc,0,1,0,&layouts[n],nullptr,nullptr,&bytes);readbacks[n]=Buffer(gpu.device.Get(),bytes,D3D12_HEAP_TYPE_READBACK,D3D12_RESOURCE_STATE_COPY_DEST);}
    for(UINT round=0;round<2;++round){if(round){void* mapped{};const D3D12_RANGE none{};Check(originalVertex->Map(0,&none,&mapped));const UINT changed=2007;
            std::memcpy(static_cast<uint8_t*>(mapped)+32+7*48,&changed,4);const D3D12_RANGE written{32+7*48,32+7*48+4};originalVertex->Unmap(0,&written);gpu.Begin();}
        gpu.list->SetGraphicsRootSignature(root.Get());gpu.list->SetPipelineState(pipeline.Get());gpu.list->IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        const D3D12_VIEWPORT viewport{0,0,4,4,0,1};const D3D12_RECT scissor{0,0,4,4};gpu.list->RSSetViewports(1,&viewport);gpu.list->RSSetScissorRects(1,&scissor);
        for(UINT n=0;n<2;++n){const FLOAT clear[4]{};gpu.list->ClearRenderTargetView(handles[n],clear,0,nullptr);gpu.list->OMSetRenderTargets(1,&handles[n],FALSE,nullptr);
            Require(commands.RecordSourceGeometry(gpu.list.Get(),n),"Actual source geometry recording failed");Transition(gpu.list.Get(),colors[n].Get(),D3D12_RESOURCE_STATE_RENDER_TARGET,D3D12_RESOURCE_STATE_COPY_SOURCE);
            D3D12_TEXTURE_COPY_LOCATION to{},from{};to.pResource=readbacks[n].Get();to.Type=D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;to.PlacedFootprint=layouts[n];
            from.pResource=colors[n].Get();from.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;gpu.list->CopyTextureRegion(&to,0,0,0,&from,nullptr);
            Transition(gpu.list.Get(),colors[n].Get(),D3D12_RESOURCE_STATE_COPY_SOURCE,D3D12_RESOURCE_STATE_RENDER_TARGET);}
        Require(cvr::gpu::KeepCommandResources(gpu.list.Get(),{pipeline.Get(),root.Get(),rtvs.Get(),colors[0].Get(),colors[1].Get(),readbacks[0].Get(),readbacks[1].Get()}),"Consumer resources lost");
        gpu.Execute();for(UINT n=0;n<2;++n){void* mapped{};const D3D12_RANGE range{0,SIZE_T(readbacks[n]->GetDesc().Width)};Check(readbacks[n]->Map(0,&range,&mapped));
            bool equal=true;for(UINT y=0;y<4;++y)for(UINT x=0;x<4;++x){std::array<UINT,4> actual{};std::memcpy(actual.data(),static_cast<uint8_t*>(mapped)+layouts[n].Offset+y*layouts[n].Footprint.RowPitch+x*16,16);
                const std::array<UINT,4> expected{round && !n?2007u:907u,43,47,53};
                if(actual!=expected && equal)std::fprintf(stderr,"IA round %u draw %u pixel %u,%u: %u %u %u %u expected %u %u %u %u\n",
                    round,n,x,y,actual[0],actual[1],actual[2],actual[3],expected[0],expected[1],expected[2],expected[3]);
                equal&=actual==expected;}const D3D12_RANGE none{};readbacks[n]->Unmap(0,&none);Require(equal,"Private/original IA coordinates or immutable GPU bytes differ");}
        Require(draws[0].draw.firstIndex==2 && draws[1].draw.firstIndex==2 && draws[0].draw.baseVertex==-4 && draws[1].draw.baseVertex==-4 &&
            draws[0].draw.firstInstance==7 && draws[1].draw.firstInstance==7,"Snapshot rebased original indexed draw arguments");gpu.Clean();}
}
int main(int argc,char** argv)try{Pure();
    Require(ia::detail::RequiresUniformStateProof(0,1) && ia::detail::RequiresUniformStateProof(1,0) &&
        ia::detail::RequiresUniformStateProof(3,1) && ia::detail::RequiresUniformStateProof(1,3) &&
        !ia::detail::RequiresUniformStateProof(1,1),"Shared-COM tracked/fixed admission depends on IA role order");
    if(argc>1){Require(argc==2 && std::strcmp(argv[1],"--gpu")==0,"Only --gpu is supported");GpuCoordinates();GpuCoalesced();}
    std::puts("native_ia_snapshot_test: PASS bounded admission/recovery/once/lifetime math; optional GPU fixture owns its copies and proves coordinates, not native-source admission");return 0;
}catch(const std::exception& error){std::fprintf(stderr,"native_ia_snapshot_test: %s\n",error.what());return 1;}
