#pragma once
#include "Render/NativeGeometryLeases.hpp"
#include "Render/NativeIndirectCopy.hpp"
#include <atomic>
#include <type_traits>
#include <algorithm>

namespace cvr::stereo::ia_snapshot {
enum class Role:uint64_t {Index,Vertex};
enum class Failure:uint64_t {
    None,Disabled,Empty,Arguments,Scope,Code,Parent,CurrentIa,Recording,NativeRecord,
    CreationDeferred,HeapDeferred,ProtectedDeferred,PoolDeferred,PlacedDeferred,
    UploadDeferred,ClassDeferred,AliasDeferred,EarlyFixedDeferred,BackingDeferred,
    AllocationDeferred,AllocatorDeferred,State,Capacity,Retention,Changed,
    Registration,RecoveryVerified,RecoveryFatal,AlreadyRecorded,Submission,Exception,ResidencyDeferred
};
// Current numeric observations only. These never create a copy/allocation receipt.
struct Observation {
    uint64_t role{},slot{},reason{},resource{},gpu{},width{},viewGpu{},viewBytes{},strideOrFormat{},comOffset{};
    uint64_t nativeResource{},nativeGpu{},nativeBytes{},registry{},context{},handle{},nativeKind{},recordAddress{},lowType{};
    uint64_t heap{},heapFlags{},resourceFlags{},creationKind{},creationHeap{},creationOffset{},creationBytes{},initialKnown{},initialState{};
    uint64_t unprotected{},allocation{},allocationKind{},allocationResource{},allocationHeap{},allocationBytes{},allocationPimpl{},allocator{},allocatorPimpl{};
    uint64_t stateValue{},stateStatus{},stateKind{},list{},generation{},occurrence{},flags{},reservedBytes{},privateOffset{},reserved{}; // Pool packed identity or verified placed heap offset.
    uint64_t aliasPayload{},aliasControl{},poolOwner{},poolSlot{},poolUnit{},allocationObservation{},allocatorObservation{},poolObservation{},protectedResult{};
    uint64_t dimension{},alignment{},height{},depth{},mips{},heapNodeMasks{},sessionPresent{},shapeKnown{};
};
static_assert(sizeof(Observation)==64*sizeof(uint64_t) && std::is_trivially_copyable_v<Observation>);
struct Observations {
    static constexpr UINT MaxViews=17;
    std::array<Observation,MaxViews> rows{};
    UINT count{},admitted{};Failure first=Failure::None;
};
// Trusted caller certifies the unchanged ORIGINAL native read epoch and queue
// edges for these full bound views, immediately before this original draw.
// No COM reference, exported flag or state label supplies a future source epoch.
struct ReadEpochCertificate {
    uint64_t list{},generation{},occurrence{};bool certified{};
    // Slot7 can be captured by the separate instance GPU path. Its omission
    // never means a complete IA set; the caller must join that owned version.
    uint32_t separateVertices{};
};
struct PreparedCopies {
    Microsoft::WRL::ComPtr<IUnknown> owner;
    uint64_t list{},generation{},occurrence{},copiedBytes{};UINT count{};
    // copiedBytes is logical source coverage, including reused private ranges.
    uint64_t transferredBytes{},reusedBytes{};
};
struct PrivateIa {
    Role role{};UINT slot=UINT_MAX;
    Microsoft::WRL::ComPtr<ID3D12Resource> resource;
    Microsoft::WRL::ComPtr<IUnknown> owner;
    uint64_t sourceOffset{},privateOffset{},copiedBytes{},reservedBytes{};
    uint64_t originOccurrence{};bool reused{};
    D3D12_INDEX_BUFFER_VIEW index{};D3D12_VERTEX_BUFFER_VIEW vertex{};
};
namespace detail {
inline constexpr uint64_t MaxArenaBytes=64*1024*1024,Alignment=16;
#if CVR_RUST_VRIK
// The rules below: rust/crates/cvr-render/src/ia_snapshot.rs (tools/single_pass_tests/native_ia_snapshot_test runs its
// cases against them in its Rust variant).
struct GeometryPool;struct PlacedAllocation;struct CopyRange;struct RecoveryContext;
using AllocationOffsetReader=bool(*)(void*,uint64_t,uint64_t&);
extern "C" {
bool cvr_ia_selection(uint32_t,uint32_t,uint32_t*);
bool cvr_ia_read_placed_offset(geometry::MemoryReader,AllocationOffsetReader,void*,uint64_t,uint64_t,const PlacedAllocation*,PlacedAllocation*);
bool cvr_ia_logical_range(uint64_t,uint64_t,uint64_t,uint64_t,uint64_t,uint64_t);
bool cvr_ia_read_geometry_pool(geometry::MemoryReader,void*,uint64_t,const geometry::NativeBuffer*,const std::array<uint8_t,0xB0>*,
    uint64_t,uint64_t,uint64_t,uint64_t,GeometryPool*,uint8_t);
bool cvr_ia_requires_uniform_state_proof(uint32_t,uint32_t);
uint32_t cvr_ia_read_bit(uint64_t);
bool cvr_ia_read_mask(const geometry::ReadState*,uint32_t,uint32_t,bool);
bool cvr_ia_recovery_mask(uint32_t,const geometry::ReadState*,uint32_t,uint32_t,uint32_t*);
bool cvr_ia_barrier_capacity(uint64_t,uint32_t,uint32_t);
bool cvr_ia_reserve(uint64_t,uint64_t,uint64_t,uint64_t*,uint64_t*);
bool cvr_ia_plan_copies(const CopyRange*,size_t,uint64_t,uint64_t,CopyRange*,size_t*,uint64_t*,uint64_t*,uint64_t*);
bool cvr_ia_index_view(uint64_t,uint64_t,uint64_t,const D3D12_INDEX_BUFFER_VIEW*,D3D12_INDEX_BUFFER_VIEW*);
bool cvr_ia_vertex_view(uint64_t,uint64_t,uint64_t,const D3D12_VERTEX_BUFFER_VIEW*,D3D12_VERTEX_BUFFER_VIEW*);
uint64_t cvr_ia_admit(const Observation*);
bool cvr_ia_recovery_identity(const geometry::NativeBuffer*,const std::array<uint8_t,0xB0>*,uint64_t,const RecoveryContext*,
    const geometry::NativeBuffer*,const std::array<uint8_t,0xB0>*,uint64_t,const RecoveryContext*);
}
#endif
#if CVR_RUST_VRIK
inline bool Selection(uint32_t original,uint32_t separate,uint32_t& copied)noexcept{return cvr_ia_selection(original,separate,&copied);}
#else
inline bool Selection(uint32_t original,uint32_t separate,uint32_t& copied)noexcept {
    copied=0;if(original&~65535u || (separate!=0 && separate!=128) || (separate&original)!=separate)return false;
    copied=original&~separate;return true;
}
#endif
enum Flag:uint64_t {RecordKnown=1,CreationKnown=2,AllocationKnown=4,AllocatorKnown=8,SameDevice=16,
    ScopeCurrent=32,BindingsCurrent=64,ParentCurrent=128,ContextKnown=256,CodeKnown=512,FullBacking=1024,Pinned=2048,GeometryPoolKnown=4096,PlacedKnown=8192};
inline constexpr uint64_t RequiredFlags=RecordKnown|CreationKnown|AllocationKnown|AllocatorKnown|SameDevice|
    ScopeCurrent|BindingsCurrent|ParentCurrent|ContextKnown|CodeKnown|FullBacking;
struct GeometryPool {
    uint64_t header{},data{},entry{},resource{},owner{},occupiedWord{};
    uint32_t packed{},sizeClass{},group{},slot{},unit{},capacity{},count{},slots{};
    std::array<uint32_t,5> units{};
    bool operator==(const GeometryPool&)const=default;
};
template<class T,size_t N>inline T PoolField(const std::array<uint8_t,N>& raw,size_t offset)noexcept {
    T value{};std::memcpy(&value,raw.data()+offset,sizeof(value));return value;
}
struct PlacedAllocation {
    uint64_t allocation{},resource{},pimpl{},block{},handle{},heap{},bytes{},metadata{},metadataVtable{},offsetMethod{},offset{};
    bool operator==(const PlacedAllocation&)const=default;
};
using AllocationOffsetReader=bool(*)(void*,uint64_t,uint64_t&);
// GetOffset uses metadata's virtual lookup; its handle is not a byte offset.
// The caller pins the typed allocation/allocator before invoking this reader.
#if CVR_RUST_VRIK
inline bool ReadPlacedOffset(geometry::MemoryReader reader,AllocationOffsetReader getter,void* user,uint64_t base,uint64_t imageBytes,
    const PlacedAllocation& expected,PlacedAllocation& output){
    static_assert(sizeof(PlacedAllocation)==88,"mirrored by texture_immutable.rs");
    return cvr_ia_read_placed_offset(reader,getter,user,base,imageBytes,&expected,&output);
}
#else
inline bool ReadPlacedOffset(geometry::MemoryReader reader,AllocationOffsetReader getter,void* user,uint64_t base,uint64_t imageBytes,
    const PlacedAllocation& expected,PlacedAllocation& output){
    output={};if(!reader || !getter || !base || imageBytes<=0x306EC50 || imageBytes>UINT64_MAX-base ||
        !expected.allocation || expected.allocation>UINT64_MAX-0x68 || !expected.resource || !expected.pimpl ||
        !expected.block || expected.block>UINT64_MAX-0x48 || !expected.handle || !expected.heap || !expected.bytes)return false;
    const auto read=[&](uint64_t at,void* value,size_t bytes){return at && at<=UINT64_MAX-bytes && reader(user,at,value,bytes);};
    const auto valid=[&](const std::array<uint8_t,0x68>& raw){return PoolField<uint64_t>(raw,0)==base+0x30466A8 &&
        PoolField<uint32_t>(raw,8) && PoolField<uint32_t>(raw,8)<INT32_MAX && (PoolField<uint32_t>(raw,0x60)&3)==1 &&
        PoolField<uint64_t>(raw,0x10)==expected.pimpl && PoolField<uint64_t>(raw,0x18)==expected.bytes &&
        PoolField<uint64_t>(raw,0x28)==expected.resource && PoolField<uint64_t>(raw,0x40)==expected.handle && PoolField<uint64_t>(raw,0x48)==expected.block;};
    const auto once=[&](PlacedAllocation& current){current=expected;std::array<uint8_t,0x68> raw{};uint64_t heap{},metadata{};
        if(!read(expected.allocation,raw.data(),raw.size()) || !valid(raw) || !read(expected.block+0x38,&heap,8) || heap!=expected.heap ||
            !read(expected.block+0x40,&metadata,8) || !metadata || metadata>UINT64_MAX-8 ||
            !read(metadata,&current.metadataVtable,8) || current.metadataVtable<base || current.metadataVtable-base>imageBytes-0x38 ||
            !read(current.metadataVtable+0x30,&current.offsetMethod,8) || current.offsetMethod<base || current.offsetMethod-base>=imageBytes)return false;
        current.metadata=metadata;if(!getter(user,expected.allocation,current.offset))return false;
        uint64_t nowHeap{},nowMetadata{},nowVtable{},nowMethod{};
        return read(expected.allocation,raw.data(),raw.size()) && valid(raw) &&
            read(expected.block+0x38,&nowHeap,8) && nowHeap==heap && read(expected.block+0x40,&nowMetadata,8) && nowMetadata==metadata &&
            read(metadata,&nowVtable,8) && nowVtable==current.metadataVtable && read(current.metadataVtable+0x30,&nowMethod,8) && nowMethod==current.offsetMethod;
    };
    PlacedAllocation first,again;if(!once(first) || !once(again) || first!=again)return false;output=first;return true;
}
#endif
#if CVR_RUST_VRIK
inline bool LogicalRange(uint64_t gpu,uint64_t bytes,uint64_t nativeGpu,uint64_t nativeBytes,uint64_t viewGpu,uint64_t viewBytes)noexcept{
    return cvr_ia_logical_range(gpu,bytes,nativeGpu,nativeBytes,viewGpu,viewBytes);
}
#else
inline bool LogicalRange(uint64_t gpu,uint64_t bytes,uint64_t nativeGpu,uint64_t nativeBytes,uint64_t viewGpu,uint64_t viewBytes)noexcept {
    return gpu && bytes && gpu<=UINT64_MAX-bytes && nativeGpu>=gpu && nativeBytes && nativeGpu-gpu<=bytes &&
        nativeBytes<=bytes-(nativeGpu-gpu) && viewGpu>=nativeGpu && viewBytes && viewGpu-nativeGpu<=nativeBytes &&
        viewBytes<=nativeBytes-(viewGpu-nativeGpu);
}
#endif
// AE7CD0's geometry pools use five size classes. Occupancy and the typed
// allocation owner are separate from the arithmetic logical-view bounds.
#if CVR_RUST_VRIK
inline bool ReadGeometryPool(geometry::MemoryReader reader,void* user,uint64_t base,const geometry::NativeBuffer& native,
    const std::array<uint8_t,0xB0>& saved,uint64_t gpu,uint64_t bytes,uint64_t viewGpu,uint64_t viewBytes,GeometryPool& output,uint8_t lowType=5){
    static_assert(sizeof(GeometryPool)==104,"mirrored by ia_snapshot.rs");
    return cvr_ia_read_geometry_pool(reader,user,base,&native,&saved,gpu,bytes,viewGpu,viewBytes,&output,lowType);
}
#else
inline bool ReadGeometryPool(geometry::MemoryReader reader,void* user,uint64_t base,const geometry::NativeBuffer& native,
    const std::array<uint8_t,0xB0>& saved,uint64_t gpu,uint64_t bytes,uint64_t viewGpu,uint64_t viewBytes,GeometryPool& output,uint8_t lowType=5){
    output={};
    const auto read=[&](uint64_t address,void* value,size_t size){return reader && address && address<=UINT64_MAX-size && reader(user,address,value,size);};
    if(!base || base>UINT64_MAX-0x3BCDCB0 || !native.registry || native.registry>UINT64_MAX-0x1AE2C6A || !native.context ||
        !native.handle || native.handle>0x100000 || native.kind!=1 || (lowType!=5 && lowType!=13) || saved[6]!=(0x10|lowType) || !native.resource ||
        (native.bytes>32768 && (native.bytes<=65536 || native.bytes>98304)) ||
        !LogicalRange(gpu,bytes,native.gpu,native.bytes,viewGpu,viewBytes) ||
        PoolField<uint64_t>(saved,0x18) || PoolField<uint64_t>(saved,0x20) || PoolField<uint64_t>(saved,0x28))return false;
    const uint64_t recordOffset=0x5C0AF8+uint64_t(native.handle-1)*0xB0;
    if(native.registry>UINT64_MAX-recordOffset-saved.size())return false;
    const auto recordAddress=native.registry+recordOffset;
    const auto once=[&](GeometryPool& pool){
        std::array<uint8_t,0xB0> raw{};uint64_t registry{};uint8_t enabled{},disabled{};
        if(!read(base+0x3BCDCA8,&registry,8) || registry!=native.registry ||
            !read(native.registry+0x14105C0,&enabled,1) || !enabled ||
            !read(native.registry+0x1AE2C69,&disabled,1) || (disabled && lowType==5) ||
            !read(recordAddress,raw.data(),raw.size()) || raw!=saved ||
            PoolField<uint32_t>(raw,0)!=native.bytes || PoolField<uint64_t>(raw,8)!=native.gpu ||
            PoolField<uint64_t>(raw,16)!=native.resource || !read(base+0x36F44C8,pool.units.data(),sizeof(pool.units)))return false;
        pool.packed=PoolField<uint32_t>(raw,0x40);pool.sizeClass=pool.packed&7;
        pool.group=(pool.packed&65535)>>3;pool.slot=pool.packed>>16;
        if(pool.sizeClass>=pool.units.size())return false;
        for(size_t i=0;i<pool.units.size();++i)if(!pool.units[i] || pool.units[i]>98304 || (i && pool.units[i]<=pool.units[i-1]))return false;
        pool.unit=pool.units[pool.sizeClass];
        if(native.bytes>pool.unit || (pool.sizeClass && native.bytes<=pool.units[pool.sizeClass-1]) || bytes%pool.unit ||
            bytes/pool.unit>1024 || !bytes || pool.slot>=bytes/pool.unit || native.gpu!=gpu+uint64_t(pool.slot)*pool.unit)return false;
        pool.slots=uint32_t(bytes/pool.unit);pool.header=native.registry+0x1410548+uint64_t(pool.sizeClass)*24;
        std::array<uint8_t,16> header{};if(!read(pool.header,header.data(),header.size()))return false;
        pool.data=PoolField<uint64_t>(header,0);pool.capacity=PoolField<uint32_t>(header,8);pool.count=PoolField<uint32_t>(header,12);
        if(!pool.data || !pool.count || pool.count>pool.capacity || pool.capacity>8192 || pool.group>=pool.count ||
            pool.data>UINT64_MAX-uint64_t(pool.group)*144-144)return false;
        pool.entry=pool.data+uint64_t(pool.group)*144;std::array<uint8_t,144> entry{};
        if(!read(pool.entry,entry.data(),entry.size()))return false;
        pool.resource=PoolField<uint64_t>(entry,0);pool.owner=PoolField<uint64_t>(entry,8);
        pool.occupiedWord=PoolField<uint64_t>(entry,16+uint64_t(pool.slot/64)*8);
        return pool.resource==native.resource && pool.owner && (pool.occupiedWord&(uint64_t(1)<<(pool.slot%64)));
    };
    GeometryPool first,again;if(!once(first) || !once(again) || first!=again)return false;output=first;return true;
}
#endif
#if CVR_RUST_VRIK
inline bool RequiresUniformStateProof(uint32_t first,uint32_t second)noexcept{return cvr_ia_requires_uniform_state_proof(first,second);}
#else
inline bool RequiresUniformStateProof(uint32_t first,uint32_t second)noexcept {
    return first==0 || first==3 || second==0 || second==3;
}
#endif
#if CVR_RUST_VRIK
inline uint32_t ReadBit(Role role)noexcept{return cvr_ia_read_bit(uint64_t(role));}
#else
inline uint32_t ReadBit(Role role)noexcept{return role==Role::Index?D3D12_RESOURCE_STATE_INDEX_BUFFER:D3D12_RESOURCE_STATE_VERTEX_AND_CONSTANT_BUFFER;}
#endif
#if CVR_RUST_VRIK
inline bool ReadMask(geometry::ReadState state,uint32_t kind,uint32_t reads,bool ready=false)noexcept{return cvr_ia_read_mask(&state,kind,reads,ready);}
#else
inline bool ReadMask(geometry::ReadState state,uint32_t kind,uint32_t reads,bool ready=false)noexcept {
    return (kind==0 || kind==3) && state.kind==kind && state.status==2 && reads &&
        (state.value&reads)==reads && !(state.value&~UINT(D3D12_RESOURCE_STATE_GENERIC_READ)) &&
        (!ready || (state.value&D3D12_RESOURCE_STATE_COPY_SOURCE));
}
#endif
#if CVR_RUST_VRIK
inline bool RecoveryMask(uint32_t previous,geometry::ReadState current,uint32_t kind,uint32_t reads,uint32_t& required)noexcept{
    return cvr_ia_recovery_mask(previous,&current,kind,reads,&required);
}
#else
inline bool RecoveryMask(uint32_t previous,geometry::ReadState current,uint32_t kind,uint32_t reads,uint32_t& required)noexcept {
    required=0;if(!ReadMask({previous,2,kind},kind,reads) || current.status!=2 || current.kind!=kind ||
        (current.value&~UINT(D3D12_RESOURCE_STATE_GENERIC_READ)))return false;
    required=previous|current.value;return true;
}
#endif
#if CVR_RUST_VRIK
inline bool BarrierCapacity(uint64_t data,uint32_t capacity,uint32_t pending)noexcept{return cvr_ia_barrier_capacity(data,capacity,pending);}
#else
inline bool BarrierCapacity(uint64_t data,uint32_t capacity,uint32_t pending)noexcept {
    return data && capacity && capacity<=1048576 && !pending && data<=UINT64_MAX-uint64_t(capacity)*32;
}
#endif
#if CVR_RUST_VRIK
inline bool Reserve(uint64_t cursor,uint64_t capacity,uint64_t bytes,uint64_t& offset,uint64_t& end)noexcept{
    return cvr_ia_reserve(cursor,capacity,bytes,&offset,&end);
}
#else
inline bool Reserve(uint64_t cursor,uint64_t capacity,uint64_t bytes,uint64_t& offset,uint64_t& end)noexcept {
    offset=end=0;if(!capacity || capacity>MaxArenaBytes || capacity%Alignment || cursor%Alignment || cursor>capacity ||
        !bytes || bytes>UINT32_MAX || bytes>UINT64_MAX-(Alignment-1))return false;
    const auto padded=(bytes+Alignment-1)&~(Alignment-1);if(padded>capacity-cursor)return false;
    offset=cursor;end=cursor+padded;return true;
}
#endif
struct CopyRange {uint64_t resource{},sourceOffset{},bytes{},destination{};};
struct CopyPlan {std::vector<CopyRange> ranges;std::vector<uint64_t> destinations;uint64_t next{},copied{};};
// Union only overlapping/touching views of one resource at one original call.
// Never bridges an unread gap or shares a version across source occurrences.
#if CVR_RUST_VRIK
inline bool PlanCopies(std::span<const CopyRange> input,uint64_t cursor,uint64_t capacity,CopyPlan& output){
    static_assert(sizeof(CopyRange)==32,"mirrored by ia_snapshot.rs");
    output={};CopyPlan plan;plan.ranges.resize(input.size());plan.destinations.resize(input.size());size_t count{};
    if(!cvr_ia_plan_copies(input.data(),input.size(),cursor,capacity,plan.ranges.data(),&count,plan.destinations.data(),
        &plan.next,&plan.copied))return false;
    plan.ranges.resize(count);output=std::move(plan);return true;
}
#else
inline bool PlanCopies(std::span<const CopyRange> input,uint64_t cursor,uint64_t capacity,CopyPlan& output){
    output={};if(input.empty() || input.size()>Observations::MaxViews)return false;
    auto sorted=std::vector<CopyRange>(input.begin(),input.end());
    for(const auto& range:sorted)if(!range.resource || !range.bytes || range.sourceOffset>UINT64_MAX-range.bytes)return false;
    std::sort(sorted.begin(),sorted.end(),[](const auto& a,const auto& b){return a.resource<b.resource || (a.resource==b.resource && a.sourceOffset<b.sourceOffset);});
    CopyPlan plan;
    for(const auto& range:sorted){
        if(!plan.ranges.empty()){auto& last=plan.ranges.back();
            if(last.resource==range.resource && range.sourceOffset<=last.sourceOffset+last.bytes){
                last.bytes=(std::max)(last.sourceOffset+last.bytes,range.sourceOffset+range.bytes)-last.sourceOffset;continue;}}
        plan.ranges.push_back(range);
    }
    for(auto& range:plan.ranges){const auto prefix=range.sourceOffset%Alignment;uint64_t start{},end{};
        if(range.bytes>UINT64_MAX-prefix || !Reserve(cursor,capacity,range.bytes+prefix,start,end))return false;
        range.destination=start+prefix;cursor=end;plan.copied+=range.bytes;
    }
    for(const auto& range:input){const auto group=std::find_if(plan.ranges.begin(),plan.ranges.end(),[&](const auto& copy){
            return copy.resource==range.resource && range.sourceOffset>=copy.sourceOffset &&
                range.sourceOffset-copy.sourceOffset<=copy.bytes && range.bytes<=copy.bytes-(range.sourceOffset-copy.sourceOffset);});
        if(group==plan.ranges.end())return false;plan.destinations.push_back(group->destination+range.sourceOffset-group->sourceOffset);
    }
    plan.next=cursor;output=std::move(plan);return true;
}
#endif
#if CVR_RUST_VRIK
inline bool IndexView(uint64_t gpu,uint64_t offset,uint64_t capacity,const D3D12_INDEX_BUFFER_VIEW& source,D3D12_INDEX_BUFFER_VIEW& out)noexcept{
    return cvr_ia_index_view(gpu,offset,capacity,&source,&out);
}
#else
inline bool IndexView(uint64_t gpu,uint64_t offset,uint64_t capacity,const D3D12_INDEX_BUFFER_VIEW& source,D3D12_INDEX_BUFFER_VIEW& out)noexcept {
    out={};const UINT unit=source.Format==DXGI_FORMAT_R16_UINT?2:source.Format==DXGI_FORMAT_R32_UINT?4:0;
    if(!unit || !gpu || gpu>UINT64_MAX-capacity || !source.SizeInBytes || offset>capacity || source.SizeInBytes>capacity-offset ||
        gpu>UINT64_MAX-offset || (gpu+offset)%unit || source.SizeInBytes%unit)return false;
    out=source;out.BufferLocation=gpu+offset;return true;
}
#endif
#if CVR_RUST_VRIK
inline bool VertexView(uint64_t gpu,uint64_t offset,uint64_t capacity,const D3D12_VERTEX_BUFFER_VIEW& source,D3D12_VERTEX_BUFFER_VIEW& out)noexcept{
    return cvr_ia_vertex_view(gpu,offset,capacity,&source,&out);
}
#else
inline bool VertexView(uint64_t gpu,uint64_t offset,uint64_t capacity,const D3D12_VERTEX_BUFFER_VIEW& source,D3D12_VERTEX_BUFFER_VIEW& out)noexcept {
    out={};if(!gpu || gpu>UINT64_MAX-capacity || !source.SizeInBytes || !source.StrideInBytes || offset>capacity || source.SizeInBytes>capacity-offset || gpu>UINT64_MAX-offset)return false;
    out=source;out.BufferLocation=gpu+offset;return true;
}
#endif
#if CVR_RUST_VRIK
inline Failure Admit(const Observation& a)noexcept{return Failure(cvr_ia_admit(&a));}
#else
inline Failure Admit(const Observation& a)noexcept {
    if(!(a.flags&ScopeCurrent))return Failure::Scope;
    if(!(a.flags&CodeKnown))return Failure::Code;
    if(!(a.flags&ParentCurrent))return Failure::Parent;
    if(!(a.flags&BindingsCurrent))return Failure::CurrentIa;
    if(!(a.flags&ContextKnown) || !a.list || !a.generation || !a.occurrence)return Failure::Recording;
    if(!(a.flags&RecordKnown) || !a.resource || a.nativeResource!=a.resource || !a.registry || !a.context || !a.handle || a.handle>0x100000)return Failure::NativeRecord;
    if(a.heap==D3D12_HEAP_TYPE_UPLOAD || a.heap==5)return Failure::UploadDeferred;
    if(a.heap!=D3D12_HEAP_TYPE_DEFAULT)return Failure::HeapDeferred;
    const bool pooled=(a.flags&GeometryPoolKnown)!=0;
    const bool placed=(a.flags&PlacedKnown)!=0;
    if(a.poolOwner && !pooled)return Failure::PoolDeferred;
    if(pooled && (a.nativeKind!=1 || a.lowType!=5 || !a.poolOwner || a.poolOwner!=a.allocation || a.poolObservation!=1 ||
        a.reserved>UINT32_MAX || (a.reserved&7)>=5 || (a.reserved>>16)!=a.poolSlot ||
        !a.poolUnit || a.poolUnit>98304 || a.width%a.poolUnit || a.width/a.poolUnit>1024 || a.poolSlot>=a.width/a.poolUnit ||
        a.nativeGpu<a.gpu || a.nativeGpu-a.gpu!=a.poolSlot*a.poolUnit || a.nativeBytes>a.poolUnit))return Failure::PoolDeferred;
    if(a.aliasPayload || a.aliasControl)return Failure::AliasDeferred;
    if(!(a.flags&CreationKnown))return Failure::CreationDeferred;
    if(a.creationKind==2){
        if(!placed || pooled || !a.creationHeap || !a.creationBytes || a.creationOffset>UINT64_MAX-a.creationBytes ||
            a.creationOffset%D3D12_DEFAULT_RESOURCE_PLACEMENT_ALIGNMENT || a.reserved!=a.creationOffset ||
            a.allocationKind!=1 || a.allocationHeap!=a.creationHeap || a.allocationBytes!=a.creationBytes)return Failure::PlacedDeferred;
    }else if(placed || a.creationKind!=1 || a.creationHeap || a.creationOffset || a.creationBytes)return Failure::CreationDeferred;
    if(!a.unprotected || a.sessionPresent || (a.heapFlags&~uint64_t(D3D12_HEAP_FLAG_DENY_RT_DS_TEXTURES|D3D12_HEAP_FLAG_DENY_NON_RT_DS_TEXTURES)))return Failure::ProtectedDeferred;
    if(!(a.flags&SameDevice) || !a.shapeKnown || a.dimension!=D3D12_RESOURCE_DIMENSION_BUFFER || !a.width || !a.gpu || a.gpu>UINT64_MAX-a.width ||
        a.height!=1 || a.depth!=1 || a.mips!=1 || (a.alignment && a.alignment!=D3D12_DEFAULT_RESOURCE_PLACEMENT_ALIGNMENT) ||
        (a.heapNodeMasks>>32)>1 || uint32_t(a.heapNodeMasks)>1 ||
        (!pooled && (!(a.flags&FullBacking) || a.nativeGpu!=a.gpu || a.nativeBytes!=a.width)) ||
        (pooled && !LogicalRange(a.gpu,a.width,a.nativeGpu,a.nativeBytes,a.viewGpu,a.viewBytes)))
        return Failure::BackingDeferred;
    if(!a.viewGpu || !a.viewBytes || a.viewGpu<a.gpu || a.comOffset!=a.viewGpu-a.gpu || a.comOffset>a.width || a.viewBytes>a.width-a.comOffset)return Failure::Arguments;
    if(a.role>uint64_t(Role::Vertex) || (a.role==uint64_t(Role::Index)?a.slot!=UINT_MAX:a.slot>=16))return Failure::Arguments;
    if(a.role==uint64_t(Role::Index)){
        const UINT unit=a.strideOrFormat==DXGI_FORMAT_R16_UINT?2:a.strideOrFormat==DXGI_FORMAT_R32_UINT?4:0;
        if(!unit || a.viewGpu%unit || a.viewBytes%unit)return Failure::Arguments;
    }else if(!a.strideOrFormat || a.strideOrFormat>UINT32_MAX)return Failure::Arguments;
    if(!(a.flags&AllocationKnown) || !a.allocation || a.allocationKind!=(placed?1u:0u) || a.allocationResource!=a.resource ||
        (!placed && a.allocationHeap) || a.allocationBytes<a.width || !a.allocationPimpl)
        return Failure::AllocationDeferred;
    if(!(a.flags&AllocatorKnown) || !a.allocator || a.allocatorPimpl!=a.allocationPimpl)return Failure::AllocatorDeferred;
    if(a.nativeKind==1){
        if(a.stateStatus==9)return Failure::EarlyFixedDeferred;
        if(a.resourceFlags || !a.initialKnown || (a.initialState!=D3D12_RESOURCE_STATE_COMMON && a.initialState!=D3D12_RESOURCE_STATE_GENERIC_READ) ||
            a.stateKind!=1 || a.stateStatus!=8 || a.stateValue!=D3D12_RESOURCE_STATE_GENERIC_READ)return Failure::State;
    }else if(a.nativeKind==0 || a.nativeKind==3){
        if(a.resourceFlags&~uint64_t(D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS) ||
            !ReadMask({UINT(a.stateValue),UINT(a.stateStatus),UINT(a.stateKind)},UINT(a.nativeKind),ReadBit(Role(a.role))))return Failure::State;
    }else return Failure::ClassDeferred;
    return Failure::None;
}
#endif
struct RecoveryContext {uint64_t address{},list{},state{},submitted{};UINT kind{},pending{};bool known{};};
#if CVR_RUST_VRIK
inline bool RecoveryIdentity(const geometry::NativeBuffer& saved,const std::array<uint8_t,0xB0>& raw,
    uint64_t address,const RecoveryContext& context,const geometry::NativeBuffer& current,
    const std::array<uint8_t,0xB0>& currentRaw,uint64_t currentAddress,const RecoveryContext& now)noexcept{
    static_assert(sizeof(RecoveryContext)==48,"mirrored by ia_snapshot.rs");
    return cvr_ia_recovery_identity(&saved,&raw,address,&context,&current,&currentRaw,currentAddress,&now);
}
#else
inline bool RecoveryIdentity(const geometry::NativeBuffer& saved,const std::array<uint8_t,0xB0>& raw,
    uint64_t address,const RecoveryContext& context,const geometry::NativeBuffer& current,
    const std::array<uint8_t,0xB0>& currentRaw,uint64_t currentAddress,const RecoveryContext& now)noexcept {
    return context.known && now.known && context.address && context.list && context.state && !context.pending && !context.submitted &&
        (context.kind==1 || context.kind==2) && now.address==context.address && now.list==context.list && now.state==context.state &&
        now.kind==context.kind && !now.submitted && saved.context==context.address && current.context==saved.context &&
        saved.resource && current.resource==saved.resource && saved.gpu && current.gpu==saved.gpu && saved.bytes && current.bytes==saved.bytes &&
        saved.registry && current.registry==saved.registry && saved.handle && current.handle==saved.handle && saved.kind==current.kind &&
        (saved.kind==0 || saved.kind==3) && address && currentAddress==address && raw==currentRaw;
}
#endif
struct NativePins {Microsoft::WRL::ComPtr<IUnknown> allocator,allocation;}; // Allocation releases first.
enum class Stage:UINT {Prepared,Recording,Recorded,Failed};
class Once {
public:
    bool Begin()noexcept{auto expected=Stage::Prepared;return stage_.compare_exchange_strong(expected,Stage::Recording);}
    void Recorded()noexcept{stage_.store(Stage::Recorded,std::memory_order_release);}
    void Failed()noexcept{stage_.store(Stage::Failed,std::memory_order_release);}
    Stage Current()const noexcept{return stage_.load(std::memory_order_acquire);}
private:std::atomic<Stage> stage_{Stage::Prepared};
};
}
class ActualCallScope {
public:
    ActualCallScope(ID3D12GraphicsCommandList*,uint64_t generation,uint64_t occurrence)noexcept;
    ~ActualCallScope();ActualCallScope(const ActualCallScope&)=delete;ActualCallScope& operator=(const ActualCallScope&)=delete;
    bool Active()const noexcept;
private:indirect_copy::detail::ScopeStamp stamp_;
};
// Caller holds the typed native allocation and allocator throughout this read.
// Checks the guarded native getter and its metadata route, not a GPU read epoch.
bool ReadPinnedPlacedOffset(const detail::PlacedAllocation&,detail::PlacedAllocation&);
// Fresh native binding metadata only. Neither pins the pool nor grants a read.
bool ReadFixedPool(const geometry::NativeBuffer&,const std::array<uint8_t,0xB0>&,uint64_t gpu,uint64_t bytes,
    uint64_t viewGpu,uint64_t viewBytes,uint8_t lowType,detail::GeometryPool&);
class SnapshotArena {
public:
    SnapshotArena();~SnapshotArena();SnapshotArena(const SnapshotArena&)=delete;SnapshotArena& operator=(const SnapshotArena&)=delete;
    HRESULT Create(ID3D12Device*,ID3D12GraphicsCommandList*,uint64_t generation,uint64_t capacityBytes);
    bool Finalize(ID3D12GraphicsCommandList*);void Abort()noexcept;
    bool Finalized()const;uint64_t Bytes()const;
private:
    struct Storage;Microsoft::WRL::ComPtr<Storage> storage_;
    friend bool PrepareAtActualCall(ID3D12GraphicsCommandList*,const geometry::DrawResources&,bool,
        const native_recording::Lease&,const preparation::GraphParentRecord&,const ReadEpochCertificate&,
        SnapshotArena&,PreparedCopies&,Observations&);
};
// Full current IB (if indexed) and every consumed VB slot only. No index decode,
// inferred tight vertex span, GPU-address lookup, source submission or draw rewrite.
bool ObserveAtActualCall(ID3D12GraphicsCommandList*,const geometry::DrawResources&,bool indexed,
    const native_recording::Lease&,const preparation::GraphParentRecord&,const ReadEpochCertificate&,Observations&);
bool PrepareAtActualCall(ID3D12GraphicsCommandList*,const geometry::DrawResources&,bool indexed,
    const native_recording::Lease&,const preparation::GraphParentRecord&,const ReadEpochCertificate&,
    SnapshotArena&,PreparedCopies&,Observations&);
bool ValidateAtActualCall(ID3D12GraphicsCommandList*,const PreparedCopies&);
bool RecordAtActualCall(ID3D12GraphicsCommandList*,const PreparedCopies&,std::vector<PrivateIa>&);
bool ValidateSourceSubmission(const PreparedCopies&,ID3D12CommandQueue*);
// Held original binding metadata of a finalized private copy, not permission
// to access the original native backing at a later recording.
bool CopiedSourceShape(const PreparedCopies&,geometry::DrawResources&);
Failure LastFailure();
}
extern "C" {__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_NativeIaSnapshotsEnable;}
