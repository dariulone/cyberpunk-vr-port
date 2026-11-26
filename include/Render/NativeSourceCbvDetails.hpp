#pragma once
#include "Render/NativeSourceCbvBinding.hpp"
#include <climits>
#include <cstddef>

namespace cvr::stereo::source_cbv_details {
enum class Observation:uint32_t {Unknown,Known,Absent,NotApplicable,Unreadable,Changed,Mismatch,UnknownAbi,Unsupported};
enum class Status:uint32_t {Captured,Arguments,SourceBefore,Resource,Context,Record,SourceAfter,Changed,Exception};
enum class AllocationPath:uint32_t {Unknown,Direct,Alias,Pool,Arena};
struct Resource {
    Observation observation{};
    D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS heapFlags{};D3D12_RESOURCE_FLAGS flags{};
    uint64_t width{},gpu{},comOffset{};
};
struct Context {
    Observation observation{};uint64_t address{},list{},state{},submitted{};uint32_t kind{},pending{};
    bool operator==(const Context&)const=default;
};
struct Record {
    Observation observation{};uint64_t address{};std::array<uint8_t,0xB0> bytes{};
};
struct Alias {
    Observation observation{};uint64_t payload{},control{},allocation{},alignment{},bytes{};
    uint32_t flags{};int32_t references{};std::array<uint8_t,0x38> raw{};
};
struct Pool {
    Observation observation{};uint64_t entry{},resource{},owner{},occupiedWord{};
    uint32_t unit{},packed{},group{},slot{};bool occupied{};
};
struct Allocation {
    Observation observation{};uint64_t address{},vtable{},pimpl{},bytes{},resource{},heap{},block{},handle{};
    uint32_t references{},kind{};bool associationKnown{},resourceMatches{},extentCovers{},aliasShapeKnown{},aliasShapeMatches{};
    std::array<uint8_t,0x68> raw{};
};
struct Allocator {
    Observation observation{};uint64_t address{},vtable{},pimpl{};uint32_t references{};
    bool associationKnown{},pimplMatches{};std::array<uint8_t,0x18> raw{};
};
struct Detail {
    Status status=Status::Arguments;Resource resource;Context context;Record record;
    geometry::ReadState state;bool stateQueried{},stateValueKnown{};
    AllocationPath allocationPath{};Alias alias;Pool pool;Allocation allocation;Allocator allocator;
};
namespace detail {
// 2.5: the pooled allocation record (0x68 bytes, sub_140349CFC -> sub_140349D94) took vtable 0x30466A8
// (2.31: 0x2B28438); its first five slots are the ones Methods() checks.
inline constexpr uint64_t AllocationVtable=0x30466A8,AllocatorVtable=0x306ED78;
template<class T,size_t N>T Field(const std::array<uint8_t,N>& bytes,size_t at){T value{};std::memcpy(&value,bytes.data()+at,sizeof(value));return value;}
inline bool Copy(geometry::MemoryReader read,void* user,uint64_t at,void* output,size_t bytes){
    return read && at && at<=UINT64_MAX-bytes && read(user,at,output,bytes);
}
template<size_t N>Observation Bytes(geometry::MemoryReader read,void* user,uint64_t at,std::array<uint8_t,N>& output){
    std::array<uint8_t,N> again{};if(!Copy(read,user,at,output.data(),N) || !Copy(read,user,at,again.data(),N))return Observation::Unreadable;
    return output==again?Observation::Known:Observation::Changed;
}
template<size_t N>Observation RefCountedBytes(geometry::MemoryReader read,void* user,uint64_t at,std::array<uint8_t,N>& output){
    static_assert(N>=12);
    std::array<uint8_t,N> again{};if(!Copy(read,user,at,output.data(),N) || !Copy(read,user,at,again.data(),N))return Observation::Unreadable;
    const auto before=Field<uint32_t>(output,8),after=Field<uint32_t>(again,8);
    if(!before || before>=INT32_MAX || !after || after>=INT32_MAX)return Observation::Unsupported;
    // The guarded native AddRef/Release mutate only the DWORD at+8. Keep an
    // actual sample, while requiring every identity/allocation byte to agree.
    for(size_t i=0;i<N;++i)if((i<8 || i>=12) && output[i]!=again[i])return Observation::Changed;
    output=again;return Observation::Known;
}
#if CVR_RUST_VRIK
// The readers below: rust/crates/cvr-render/src/source_cbv_details.rs (tools/single_pass_tests/source_cbv_details_test
// runs its cases against them in its Rust variant). Bytes, RefCountedBytes and Field stay templates here.
static_assert(sizeof(Context)==48 && sizeof(Record)==192 && sizeof(Alias)==112 && sizeof(Pool)==64 && sizeof(Allocation)==192 &&
    sizeof(Allocator)==64 && sizeof(geometry::ReadState)==12 && sizeof(geometry::NativeBuffer)==48 &&
    offsetof(Allocation,raw)==85 && offsetof(Allocator,raw)==38 && offsetof(Alias,raw)==56 && offsetof(Pool,occupied)==56,
    "mirrored by rust/crates/cvr-render/src/source_cbv_details.rs");
extern "C" {
bool cvr_scd_state_value_known(const geometry::ReadState*);
bool cvr_scd_matches_record(const std::array<uint8_t,0xB0>*,const geometry::NativeBuffer*);
void cvr_scd_read_record(geometry::MemoryReader,void*,const geometry::NativeBuffer*,Record*);
void cvr_scd_read_context(geometry::MemoryReader,void*,uint64_t,uint64_t,Context*);
void cvr_scd_read_allocation(geometry::MemoryReader,void*,uint64_t,uint64_t,Allocation*);
void cvr_scd_read_allocator(geometry::MemoryReader,void*,uint64_t,uint64_t,const Allocation*,Allocator*);
void cvr_scd_read_alias(geometry::MemoryReader,void*,uint64_t,uint64_t,Alias*);
void cvr_scd_read_pool(geometry::MemoryReader,void*,uint64_t,const geometry::NativeBuffer*,const Record*,uint64_t,uint64_t,Pool*);
}
inline bool StateValueKnown(geometry::ReadState state)noexcept{return cvr_scd_state_value_known(&state);}
inline bool MatchesRecord(const std::array<uint8_t,0xB0>& bytes,const geometry::NativeBuffer& native)noexcept{return cvr_scd_matches_record(&bytes,&native);}
inline Record ReadRecord(geometry::MemoryReader read,void* user,const geometry::NativeBuffer& native){
    Record out;cvr_scd_read_record(read,user,&native,&out);return out;
}
inline Context ReadContext(geometry::MemoryReader read,void* user,uint64_t address,uint64_t list){
    Context out;cvr_scd_read_context(read,user,address,list,&out);return out;
}
inline Allocation ReadAllocation(geometry::MemoryReader read,void* user,uint64_t base,uint64_t address){
    Allocation out;cvr_scd_read_allocation(read,user,base,address,&out);return out;
}
inline Allocator ReadAllocator(geometry::MemoryReader read,void* user,uint64_t base,uint64_t registry,const Allocation& allocation){
    Allocator out;cvr_scd_read_allocator(read,user,base,registry,&allocation,&out);return out;
}
inline Alias ReadAlias(geometry::MemoryReader read,void* user,uint64_t payload,uint64_t control){
    Alias out;cvr_scd_read_alias(read,user,payload,control,&out);return out;
}
inline Pool ReadPool(geometry::MemoryReader read,void* user,uint64_t base,const geometry::NativeBuffer& native,const Record& record,uint64_t gpu,uint64_t width){
    Pool out;cvr_scd_read_pool(read,user,base,&native,&record,gpu,width,&out);return out;
}
#else
inline bool StateValueKnown(geometry::ReadState state)noexcept{return state.status==1 || state.status==2 || state.status==8 || state.status==9;}
inline bool MatchesRecord(const std::array<uint8_t,0xB0>& bytes,const geometry::NativeBuffer& native)noexcept {
    if((bytes[6]&15)!=6 || bytes[6]>>4!=native.kind || Field<uint32_t>(bytes,0)!=native.bytes)return false;
    return native.kind==4 || (Field<uint64_t>(bytes,8)==native.gpu && Field<uint64_t>(bytes,0x10)==native.resource);
}
inline Record ReadRecord(geometry::MemoryReader read,void* user,const geometry::NativeBuffer& native){
    Record out;if(!native.handle){out.observation=Observation::NotApplicable;return out;}
    const uint64_t offset=0x5C0AF8+uint64_t(native.handle-1)*0xB0;
    if(native.handle>0x100000 || !native.registry || native.registry>UINT64_MAX-offset-0xB0){out.observation=Observation::Mismatch;return out;}
    out.address=native.registry+offset;out.observation=Bytes(read,user,out.address,out.bytes);
    if(out.observation==Observation::Known && !MatchesRecord(out.bytes,native))out.observation=Observation::Mismatch;return out;
}
inline Context ReadContext(geometry::MemoryReader read,void* user,uint64_t address,uint64_t list){
    Context out;out.address=address;
    const auto once=[&](Context& value){value.address=address;return address && address<=UINT64_MAX-0x640 &&
        Copy(read,user,address+0x30,&value.list,8) && Copy(read,user,address+0x60,&value.state,8) &&
        Copy(read,user,address+0x68,&value.kind,4) && Copy(read,user,address+0x638,&value.submitted,8) && Copy(read,user,address+0x534,&value.pending,4);};
    Context again;if(!once(out) || !once(again))out.observation=Observation::Unreadable;
    else if(out!=again)out.observation=Observation::Changed;
    else out.observation=out.list==list && out.state && (out.kind==1 || out.kind==2) && !out.submitted?Observation::Known:Observation::Mismatch;
    return out;
}
inline bool Methods(geometry::MemoryReader read,void* user,uint64_t base,bool allocator){
    const auto offset=allocator?AllocatorVtable:AllocationVtable;if(!base || base>UINT64_MAX-offset-40)return false;
    std::array<uint8_t,40> bytes{};if(Bytes(read,user,base+offset,bytes)!=Observation::Known)return false;
    const std::array<uint64_t,5> expected=allocator?std::array<uint64_t,5>{0x2FAC534,0x2FAAF80,0x348220,0x18F3FB4,0x18F3F48}:
        std::array<uint64_t,5>{0x2FAC534,0x2FAAF80,0x348220,0x3483A4,0x348244};
    for(size_t i=0;i<expected.size();++i)if(base>UINT64_MAX-expected[i] || Field<uint64_t>(bytes,i*8)!=base+expected[i])return false;return true;
}
inline Allocation ReadAllocation(geometry::MemoryReader read,void* user,uint64_t base,uint64_t address){
    Allocation out;out.address=address;if(!address){out.observation=Observation::Absent;return out;}
    out.observation=RefCountedBytes(read,user,address,out.raw);if(out.observation!=Observation::Known)return out;
    out.vtable=Field<uint64_t>(out.raw,0);
    if(!base || base>UINT64_MAX-AllocationVtable || out.vtable!=base+AllocationVtable || !Methods(read,user,base,false)){out.observation=Observation::UnknownAbi;return out;}
    out.references=Field<uint32_t>(out.raw,8);out.pimpl=Field<uint64_t>(out.raw,0x10);out.bytes=Field<uint64_t>(out.raw,0x18);
    out.resource=Field<uint64_t>(out.raw,0x28);out.kind=Field<uint32_t>(out.raw,0x60)&3;
    if(!out.references || out.references>=INT32_MAX || !out.pimpl || !out.bytes || out.kind==3){out.observation=Observation::Unsupported;return out;}
    if(out.kind==1){out.handle=Field<uint64_t>(out.raw,0x40);out.block=Field<uint64_t>(out.raw,0x48);uint64_t again{};
        if(!out.block || out.block>UINT64_MAX-0x40 || !Copy(read,user,out.block+0x38,&out.heap,8) ||
            !Copy(read,user,out.block+0x38,&again,8) || !out.heap)out.observation=Observation::Unreadable;
        else if(out.heap!=again)out.observation=Observation::Changed;
    }else if(out.kind==2){out.heap=Field<uint64_t>(out.raw,0x58);if(!out.heap)out.observation=Observation::Unsupported;}
    return out;
}
inline Allocator ReadAllocator(geometry::MemoryReader read,void* user,uint64_t base,uint64_t registry,const Allocation& allocation){
    Allocator out;uint64_t again{};if(!registry || registry>UINT64_MAX-0x1410548 || !Copy(read,user,registry+0x1410540,&out.address,8) ||
        !Copy(read,user,registry+0x1410540,&again,8)){out.observation=Observation::Unreadable;return out;}
    if(out.address!=again){out.observation=Observation::Changed;return out;}
    if(!out.address){out.observation=Observation::Absent;return out;}
    out.observation=RefCountedBytes(read,user,out.address,out.raw);if(out.observation!=Observation::Known)return out;
    out.vtable=Field<uint64_t>(out.raw,0);
    if(!base || base>UINT64_MAX-AllocatorVtable || out.vtable!=base+AllocatorVtable || !Methods(read,user,base,true)){out.observation=Observation::UnknownAbi;return out;}
    out.references=Field<uint32_t>(out.raw,8);out.pimpl=Field<uint64_t>(out.raw,0x10);
    if(!out.references || out.references>=INT32_MAX || !out.pimpl){out.observation=Observation::Unsupported;return out;}
    out.associationKnown=allocation.observation==Observation::Known;out.pimplMatches=out.associationKnown && out.pimpl==allocation.pimpl;return out;
}
inline Alias ReadAlias(geometry::MemoryReader read,void* user,uint64_t payload,uint64_t control){
    Alias out;out.payload=payload;out.control=control;if(!payload && !control){out.observation=Observation::Absent;return out;}
    if(!payload || !control){out.observation=Observation::Mismatch;return out;}
    out.observation=Bytes(read,user,payload,out.raw);int32_t again{};if(out.observation!=Observation::Known)return out;
    if(!Copy(read,user,control,&out.references,4) || !Copy(read,user,control,&again,4)){out.observation=Observation::Unreadable;return out;}
    if(out.references!=again){out.observation=Observation::Changed;return out;}
    out.allocation=Field<uint64_t>(out.raw,8);out.flags=Field<uint32_t>(out.raw,0x10);out.alignment=Field<uint64_t>(out.raw,0x18);out.bytes=Field<uint64_t>(out.raw,0x20);
    if(out.references<=0 || out.references==INT32_MAX || !out.allocation || out.flags!=0xC0 || out.alignment!=0x10000 || !out.bytes)out.observation=Observation::Unsupported;return out;
}
inline Pool ReadPool(geometry::MemoryReader read,void* user,uint64_t base,const geometry::NativeBuffer& native,const Record& record,uint64_t gpu,uint64_t width){
    Pool out;if(native.kind!=1){out.observation=Observation::NotApplicable;return out;}
    if(record.observation!=Observation::Known || record.bytes[6]!=0x16 || !MatchesRecord(record.bytes,native) ||
        !base || base>UINT64_MAX-0x36F44CC || !native.bytes || native.bytes>512 ||
        !gpu || width<512 || width>512*1024 || width%512 || gpu>UINT64_MAX-width || native.registry>UINT64_MAX-0x14105C1){out.observation=Observation::Unsupported;return out;}
    uint8_t enabled{};if(!Copy(read,user,base+0x36F44C8,&out.unit,4) || !Copy(read,user,native.registry+0x14105C0,&enabled,1)){out.observation=Observation::Unreadable;return out;}
    if(!enabled){out.observation=Observation::Absent;return out;}
    out.packed=Field<uint32_t>(record.bytes,0x40);out.group=(out.packed&65535)>>3;out.slot=out.packed>>16;
    if(out.unit!=512 || (out.packed&7) || out.slot>=width/512 || native.gpu!=gpu+uint64_t(out.slot)*512){out.observation=Observation::Mismatch;return out;}
    std::array<uint8_t,16> group{};out.observation=Bytes(read,user,native.registry+0x1410548,group);if(out.observation!=Observation::Known)return out;
    const auto data=Field<uint64_t>(group,0);const auto capacity=Field<uint32_t>(group,8),count=Field<uint32_t>(group,12);
    if(!data || !count || count>capacity || capacity>8192 || out.group>=count || data>UINT64_MAX-uint64_t(out.group)*144-144){out.observation=Observation::Mismatch;return out;}
    out.entry=data+uint64_t(out.group)*144;std::array<uint8_t,144> entry{};out.observation=Bytes(read,user,out.entry,entry);if(out.observation!=Observation::Known)return out;
    out.resource=Field<uint64_t>(entry,0);out.owner=Field<uint64_t>(entry,8);out.occupiedWord=Field<uint64_t>(entry,16+uint64_t(out.slot/64)*8);out.occupied=(out.occupiedWord&(uint64_t(1)<<(out.slot%64)))!=0;
    if(out.resource!=native.resource || !out.owner || !out.occupied)out.observation=Observation::Mismatch;return out;
}
#endif
inline bool SameSource(const source_cbv::SourceCbvBacking& a,const source_cbv::SourceCbvBacking& b){
    return a.resource==b.resource && a.originOwner==b.originOwner && a.stamp.gpu==b.stamp.gpu && a.stamp.heap==b.stamp.heap && a.stamp.value==b.stamp.value &&
        a.recording.token==b.recording.token && a.recording.generation==b.recording.generation && a.recording.context==b.recording.context &&
        a.recording.list==b.recording.list && a.recording.registry==b.recording.registry && source_cbv::detail::SameBuffer(a.native,b.native) &&
        a.cookie==b.cookie && a.origin==b.origin && a.gpu==b.gpu && a.backingBytes==b.backingBytes && a.sourceOffset==b.sourceOffset &&
        a.serial==b.serial && a.cpuDescriptor==b.cpuDescriptor && a.descriptorBytes==b.descriptorBytes && a.logicalBytes==b.logicalBytes &&
        a.stage==b.stage && a.shaderRegister==b.shaderRegister && a.creatorRva==b.creatorRva &&
        a.reservedBytes==b.reservedBytes && a.completedProducerRva==b.completedProducerRva &&
        a.creationRecording.token==b.creationRecording.token && a.creationRecording.list==b.creationRecording.list &&
        a.creationRecording.generation==b.creationRecording.generation && a.creationRecording.context==b.creationRecording.context &&
        a.creationRecording.registry==b.creationRecording.registry && a.writerNonce==b.writerNonce && a.writerBytes==b.writerBytes &&
        a.writerDestination==b.writerDestination && a.writerRecording.token==b.writerRecording.token &&
        a.writerRecording.list==b.writerRecording.list && a.writerRecording.generation==b.writerRecording.generation &&
        a.writerRecording.context==b.writerRecording.context && a.writerRecording.registry==b.writerRecording.registry;
}
}
// Current source metadata only. No allocation pin, native invocation, CPU Map,
// resource transition, copy/content/state admission or deferred-read permission.
bool CaptureAtSource(ID3D12GraphicsCommandList*,uint64_t root,UINT parameter,UINT offset,
    const source_cbv::SourceCbvBacking&,Detail&);
}
