#pragma once
#include <array>
#include <climits>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <d3d12.h>
#include <wrl/client.h>
#include "Render/NativeCommandRecording.hpp"

namespace cvr::stereo::geometry {
struct NativeBuffer {uint64_t resource{},gpu{},bytes{},registry{},context{};uint32_t handle{},kind{};};
struct IaFrame {uint64_t rva{},rsp{},rbx{},rbp{},rdi{},rsi{},r12{},r14{},r15{};};
using MemoryReader=bool(*)(void*,uint64_t,void*,size_t);
#if CVR_RUST_VRIK
// The rules below: rust/crates/cvr-render/src/ia_binding.rs (tools/single_pass_tests/indirect_binding_test,
// geometry_lease_test and source_srv_binding_test run their cases against them in their Rust variants).
struct ReadState;struct IndirectFrame;struct IndirectCall;struct IndirectNativeRecord;
extern "C" {
bool cvr_iab_decode_ia(const IaFrame*,bool,uint32_t,uint32_t,MemoryReader,void*,std::array<NativeBuffer,16>*);
bool cvr_iab_same_record(const IndirectNativeRecord*,const IndirectNativeRecord*);
bool cvr_iab_matches_origin(const IndirectFrame*,uint32_t,const camera_upload::Context*,uint64_t);
bool cvr_iab_decode(uint64_t,const IndirectFrame*,const camera_upload::Context*,const IndirectCall*,MemoryReader,void*,
    IndirectNativeRecord*,bool);
bool cvr_iab_source_range(const NativeBuffer*,uint64_t,uint64_t,uint64_t,uint64_t,bool*);
void cvr_iab_read_native_state(const NativeBuffer*,uint64_t,MemoryReader,void*,ReadState*);
}
#endif

// Decode only the exact native IA call sites. Resource records are borrowed
// from the caller which just completed native resource-use registration.
#if CVR_RUST_VRIK
inline bool DecodeIa(const IaFrame& f,bool index,uint32_t start,uint32_t count,
                     MemoryReader read,void* context,std::array<NativeBuffer,16>& result) {
    static_assert(sizeof(IaFrame)==72 && sizeof(NativeBuffer)==48,"mirrored by ia_binding.rs");
    return cvr_iab_decode_ia(&f,index,start,count,read,context,&result);
}
#else
inline bool DecodeIa(const IaFrame& f,bool index,uint32_t start,uint32_t count,
                     MemoryReader read,void* context,std::array<NativeBuffer,16>& result) {
    result={};if(!read || !count || count>16 || start>16-count)return false;
    auto memory=[&](uint64_t address,void* output,size_t size){return address && address<=UINT64_MAX-size && read(context,address,output,size);};
    auto record=[&](uint64_t registry,uint64_t offset,NativeBuffer& output) {
        if(!registry || offset%0xB0 || offset>=0xB0ull*0x100000 || registry>UINT64_MAX-0x5C0B10-offset)return false;
        std::array<uint8_t,24> bytes{};if(!memory(registry+offset+0x5C0AF8,bytes.data(),bytes.size()))return false;
        uint32_t size{};std::memcpy(&size,bytes.data(),4);std::memcpy(&output.gpu,bytes.data()+8,8);std::memcpy(&output.resource,bytes.data()+16,8);output.bytes=size;
        output.registry=registry;output.handle=uint32_t(offset/0xB0+1);output.kind=bytes[6]>>4;
        output.context=(f.rva==0x3392C7 || f.rva==0x339382)?f.r15:f.rva==0x339E6F?f.rdi:f.rsi;
        return output.resource && output.gpu && size && output.gpu<=UINT64_MAX-size;
    };
    auto handle=[&](uint64_t registry,uint32_t value,NativeBuffer& output) {
        return value && value<=0x100000 && record(registry,uint64_t(value-1)*0xB0,output);
    };
    if(index) {
        if(start || count!=1)return false;
        if(f.rva==0x34291C)return record(f.r12,f.rbx,result[0]);
        if(f.rva==0x3392C7 && f.rsp<=UINT64_MAX-0x120) {
            uint64_t pointer{};uint32_t id{};
            return memory(f.rsp+0x118,&pointer,8) && memory(pointer,&id,4) && handle(f.rdi,id,result[0]);
        }
    } else {
        if(f.rva==0x339E6F && count==1 && start==uint32_t(f.rsi))return record(f.rbp,f.rbx,result[0]);
        if(start)return false;
        if(f.rva==0x342980 && count==f.r14) {
            if(!record(f.r12,f.rbx,result[0]))return false;
            for(uint32_t i=1;i<count;++i)result[i]=result[0];return true;
        }
        if(f.rva==0x339382 && f.rsp<=UINT64_MAX-0x100) {
            uint32_t nativeCount{};
            if(!memory(f.rsp+0xF8,&nativeCount,4) || nativeCount!=count)return false;
            // R12 holds the signed difference between the handle and offset
            // arrays; RSI has advanced to the end of the offset array.
            const uint64_t end=f.r12+f.rsi;
            if(end<uint64_t(count)*4)return false;
            for(uint32_t i=0;i<count;++i) {
                uint32_t id{};if(!memory(end-uint64_t(count-i)*4,&id,4) || !handle(f.rdi,id,result[i]))return false;
            }
            return true;
        }
    }
    return false;
}
#endif
struct BufferLease {
    Microsoft::WRL::ComPtr<ID3D12Resource> resource;
    uint64_t gpu{},bytes{};
    uint32_t heapType{},flags{};
    NativeBuffer native;
};
struct ReadState {uint32_t value{},status{},kind{};};
static_assert(sizeof(ReadState)==12);
struct IndirectFrame {uint64_t rva{},rbx{},rdi{};};
struct IndirectCall {
    uint64_t list{},signature{},arguments{},argumentOffset{},countBuffer{},countOffset{};
    uint32_t maxCount{};
};
struct IndirectNativeRecord {
    NativeBuffer arguments;
    camera_upload::Context context;
    std::array<uint8_t,0xB0> rawRecord{};
    uint64_t recordAddress{},signature{};
    uint32_t callerRva{},signatureKind{},signatureStride{};
};
namespace indirect_binding_detail {
struct Code {uint32_t rva,bytes;uint64_t hash;};
inline constexpr std::array<Code,13> Contract{{
    {0x341DE8,111,0x14D3144825DB188Cull},{0x342AA8,116,0x2E6C6DB732EB83BAull},
    {0xD5434C,122,0x5CD66A35C9D8CF94ull},{0x413A74,114,0x6FAE48C95AD79B1Eull},
    {0x3424D4,125,0xB3E4B97DB74F9914ull},{0x342B1C,41,0xB4B1D0817B58E382ull},
    {0x337230,51,0x0396F3E29AEB7926ull},{0x3E3A0C,423,0x0EC5161BBFFBF98Full},
    {0x3E3BB4,52,0x1A0F0B6F177A422Cull},{0x337DA0,384,0xB36B2D868965D718ull},
    {0x338390,223,0x262EAAB2CEC6F5E0ull},{0x15A3530,341,0xFB537E7D4DF082B8ull},
    {0x341E58,1657,0x29164AE9FAF2EB68ull}
}};
#if CVR_RUST_VRIK
inline bool SameRecord(const IndirectNativeRecord& a,const IndirectNativeRecord& b){
    static_assert(sizeof(IndirectFrame)==24 && sizeof(IndirectCall)==56 && sizeof(IndirectNativeRecord)==280 &&
        offsetof(IndirectNativeRecord,recordAddress)==248,"mirrored by ia_binding.rs");
    return cvr_iab_same_record(&a,&b);
}
#else
inline bool SameRecord(const IndirectNativeRecord& a,const IndirectNativeRecord& b){
    const auto& x=a.arguments;const auto& y=b.arguments;
    return x.resource==y.resource && x.gpu==y.gpu && x.bytes==y.bytes && x.registry==y.registry && x.context==y.context &&
        x.handle==y.handle && x.kind==y.kind && a.context==b.context && a.rawRecord==b.rawRecord &&
        a.recordAddress==b.recordAddress && a.signature==b.signature && a.callerRva==b.callerRva &&
        a.signatureKind==b.signatureKind && a.signatureStride==b.signatureStride;
}
#endif
#if CVR_RUST_VRIK
inline bool MatchesOrigin(IndirectFrame frame,uint32_t caller,camera_upload::Context current,uint64_t registry){
    return cvr_iab_matches_origin(&frame,caller,&current,registry);
}
#else
inline bool MatchesOrigin(IndirectFrame frame,uint32_t caller,camera_upload::Context current,uint64_t registry){
    if(frame.rva!=caller || !current || !registry)return false;
    if(frame.rva==0x342B11)return frame.rbx==current.context && frame.rdi==registry;
    if(frame.rva==0xD543BB)return frame.rdi==current.context;
    if(frame.rva==0x413AE0)return frame.rbx==current.context;
    return false;
}
#endif
#if CVR_RUST_VRIK
inline bool DecodeOnce(uint64_t game,IndirectFrame frame,camera_upload::Context current,const IndirectCall& call,
    MemoryReader reader,void* user,IndirectNativeRecord& out){
    return cvr_iab_decode(game,&frame,&current,&call,reader,user,&out,true);
}
#else
inline bool DecodeOnce(uint64_t game,IndirectFrame frame,camera_upload::Context current,const IndirectCall& call,
    MemoryReader reader,void* user,IndirectNativeRecord& out){
    auto read=[&](uint64_t address,void* data,size_t bytes){return reader && address && address<=UINT64_MAX-bytes && reader(user,address,data,bytes);};
    uint64_t context{},signatureOffset{};UINT kind{},stride{};
    if(frame.rva==0x342B11 || frame.rva==0x413AE0){context=frame.rbx;signatureOffset=0x1410678;kind=1;stride=20;}
    else if(frame.rva==0xD543BB){context=frame.rdi;signatureOffset=0x1410670;kind=2;stride=16;}
    else return false;
    if(!game || game>UINT64_MAX-0x3BCDCB0 || !current || current.context!=context || current.list!=call.list || !call.signature ||
        !call.arguments || call.maxCount!=1 || call.countBuffer || call.countOffset || call.argumentOffset%4 ||
        context>UINT64_MAX-0x640)return false;
    IndirectNativeRecord pending;pending.context=current;pending.callerRva=uint32_t(frame.rva);pending.signatureKind=kind;pending.signatureStride=stride;
    auto& native=pending.arguments;uint64_t actualList{},actualState{},submitted{},signature{};UINT contextKind{},barriers{},offset{};
    if(!read(game+0x3BCDCA8,&native.registry,8) || !native.registry ||
        (frame.rva==0x342B11 && frame.rdi!=native.registry) || !read(context+0x30,&actualList,8) || actualList!=call.list ||
        !read(context+0x60,&actualState,8) || actualState!=current.state || !read(context+0x68,&contextKind,4) ||
        (contextKind!=1 && contextKind!=2) || !read(context+0x638,&submitted,8) || submitted ||
        !read(context+0x534,&barriers,4) || barriers || !read(context+0x6C,&native.handle,4) ||
        !native.handle || native.handle>0x100000 || !read(context+0x70,&offset,4) || offset!=call.argumentOffset ||
        native.registry>UINT64_MAX-signatureOffset-8 || !read(native.registry+signatureOffset,&signature,8) || signature!=call.signature)return false;
    const auto relative=0x5C0AF8+uint64_t(native.handle-1)*0xB0;
    if(native.registry>UINT64_MAX-relative-pending.rawRecord.size())return false;
    pending.recordAddress=native.registry+relative;
    if(!read(pending.recordAddress,pending.rawRecord.data(),pending.rawRecord.size()) || (pending.rawRecord[6]&15)!=0xB)return false;
    uint32_t bytes{};std::memcpy(&bytes,pending.rawRecord.data(),4);native.bytes=bytes;
    std::memcpy(&native.gpu,pending.rawRecord.data()+8,8);std::memcpy(&native.resource,pending.rawRecord.data()+16,8);
    native.kind=pending.rawRecord[6]>>4;native.context=context;pending.signature=signature;
    if(native.resource!=call.arguments || !native.gpu || !native.bytes || native.gpu>UINT64_MAX-native.bytes)return false;
    out=pending;return true;
}
#endif
#if CVR_RUST_VRIK
inline bool Decode(uint64_t game,IndirectFrame frame,camera_upload::Context current,const IndirectCall& call,
    MemoryReader reader,void* user,IndirectNativeRecord& out){
    return cvr_iab_decode(game,&frame,&current,&call,reader,user,&out,false);
}
#else
inline bool Decode(uint64_t game,IndirectFrame frame,camera_upload::Context current,const IndirectCall& call,
    MemoryReader reader,void* user,IndirectNativeRecord& out){
    IndirectNativeRecord first,second;
    if(!DecodeOnce(game,frame,current,call,reader,user,first) || !DecodeOnce(game,frame,current,call,reader,user,second) || !SameRecord(first,second))return false;
    out=first;return true;
}
#endif
#if CVR_RUST_VRIK
inline bool SourceRange(const NativeBuffer& native,uint64_t backingGpu,uint64_t backingBytes,uint64_t offset,uint64_t bytes,bool& fullBacking){
    return cvr_iab_source_range(&native,backingGpu,backingBytes,offset,bytes,&fullBacking);
}
#else
inline bool SourceRange(const NativeBuffer& native,uint64_t backingGpu,uint64_t backingBytes,uint64_t offset,uint64_t bytes,bool& fullBacking){
    fullBacking=false;
    if(!backingGpu || !backingBytes || backingGpu>UINT64_MAX-backingBytes || !native.gpu || !native.bytes || native.gpu>UINT64_MAX-native.bytes ||
        offset>backingBytes || bytes>backingBytes-offset || native.gpu<backingGpu || native.gpu-backingGpu>backingBytes ||
        native.bytes>backingBytes-(native.gpu-backingGpu) || backingGpu+offset<native.gpu || backingGpu+offset-native.gpu>native.bytes ||
        bytes>native.bytes-(backingGpu+offset-native.gpu))return false;
    fullBacking=native.gpu==backingGpu && native.bytes==backingBytes;return true;
}
#endif
}
struct IndirectBinding {
    BufferLease arguments;
    Microsoft::WRL::ComPtr<ID3D12CommandSignature> signature;
    camera_upload::Context context;
    native_recording::Lease recording;
    ReadState state;
    // Exact native buffer record, including unclassified allocation fields.
    std::array<uint8_t,0xB0> rawRecord{};
    uint64_t recordAddress{},argumentOffset{},argumentBytes{};
    uint32_t callerRva{},maxCount{},signatureKind{},signatureStride{};
    // Logical native allocation equals the actual COM buffer's complete range.
    // This is range information, not uniform-state/copy/allocation permission.
    bool fullBacking{};
};
enum class IndirectBindingFailure:uint32_t {
    None,Arguments,Fingerprints,Device,Origin,CurrentContext,Recording,Decode,
    Signature,ResourceRange,State,Revalidation,Exception
};
IndirectBindingFailure LastIndirectBindingFailure();
bool NativeIndirectCompatible();
// Actual native ExecuteIndirect callback only: maxCount=1, no count buffer,
// exact pure DRAW(16)/DRAW_INDEXED(20) signature and unmodified API offset.
// Resource ownership/state evidence does not grant copy or alias permission.
bool BoundIndirect(ID3D12GraphicsCommandList*,ID3D12CommandSignature*,UINT,ID3D12Resource*,UINT64,
    ID3D12Resource*,UINT64,const native_recording::Lease&,IndirectBinding&);
// Source-callback-only, while the exact native recording/context remains live.
// No registration, native map mutation, GPU command or resource-address search.
bool RevalidateIndirect(ID3D12GraphicsCommandList*,const native_recording::Lease&,const IndirectBinding&);
// Status 1: fixed upload heap, 2: native command-context state, 3: stale identity,
// 4: native barriers still pending, 5: unknown entry, 6: heap, 7: unsupported EXE.
// Status 8: verified native fixed-read class, observed COMMON/GENERIC_READ creation.
// Status 9: early fixed-read allocation covered by the verified native constructors.
// Query only on the recording thread at the actual draw; it never mutates a map.
#if CVR_RUST_VRIK
inline ReadState ReadNativeState(const NativeBuffer& source,uint64_t list,MemoryReader reader,void* context) {
    ReadState result;cvr_iab_read_native_state(&source,list,reader,context,&result);return result;
}
#else
inline ReadState ReadNativeState(const NativeBuffer& source,uint64_t list,MemoryReader reader,void* context) {
    ReadState result{0,3,source.kind};
    auto read=[&](uint64_t p,void* out,size_t n){return p && p<=UINT64_MAX-n && reader && reader(context,p,out,n);};
    if(!source.registry || !source.context || !source.handle || source.handle>0x100000 || source.context>UINT64_MAX-0x590)return result;
    const auto offset=0x5C0AF8+uint64_t(source.handle-1)*0xB0;
    if(source.registry>UINT64_MAX-offset-24)return result;
    std::array<uint8_t,24> record{};uint64_t nativeList{};uint32_t pending{};
    if(!read(source.registry+offset,record.data(),record.size()) || !read(source.context+0x30,&nativeList,8) || nativeList!=list)return result;
    uint64_t resource{},gpu{};uint32_t bytes{};std::memcpy(&bytes,record.data(),4);std::memcpy(&gpu,record.data()+8,8);std::memcpy(&resource,record.data()+16,8);
    if(resource!=source.resource || gpu!=source.gpu || bytes!=source.bytes || uint32_t(record[6]>>4)!=source.kind)return result;
    if(!read(source.context+0x534,&pending,4))return result;
    if(pending){result.status=4;return result;}
    struct Map {uint64_t buckets;uint32_t count,bucketCount;uint64_t data;uint32_t capacity,stride;};Map map{};
    static_assert(sizeof(Map)==32);
    result.status=5;
    if(!read(source.context+0x568,&map,sizeof(map)) || !map.count || map.count>map.capacity || map.capacity>1048576 ||
        !map.bucketCount || map.bucketCount>1048576 || map.stride!=16 || !map.buckets || !map.data)return result;
    const auto bucketOffset=uint64_t(source.handle%map.bucketCount)*4;if(map.buckets>UINT64_MAX-bucketOffset-4)return result;
    uint32_t at{};if(!read(map.buckets+bucketOffset,&at,4))return result;
    for(UINT n=0;n<map.count && n<64 && at!=UINT_MAX;++n) {
        if(at>=map.capacity || map.data>UINT64_MAX-uint64_t(at)*16-16)return result;
        std::array<uint32_t,4> entry{};if(!read(map.data+uint64_t(at)*16,entry.data(),16))return result;
        if(entry[1]==source.handle && entry[2]==source.handle) {
            if(entry[3]==0x80000000)return result;
            result.value=entry[3];result.status=2;return result;
        }
        at=entry[0];
    }
    return result;
}
#endif
ReadState ReadStateAtDraw(ID3D12GraphicsCommandList*,const BufferLease&);
void BufferCreated(ID3D12Resource*,D3D12_RESOURCE_STATES);
// Only call with a fresh native binding record, or a resource owned by the caller.
// It is not a way to revive a historical weak GPU-address lookup.
BufferLease AcquireBuffer(const NativeBuffer&,uint64_t address,uint64_t bytes);
bool NativeIaCompatible();
BufferLease BoundIndex(const D3D12_INDEX_BUFFER_VIEW&);
std::array<BufferLease,16> BoundVertices(UINT start,UINT count,const D3D12_VERTEX_BUFFER_VIEW*);
}
