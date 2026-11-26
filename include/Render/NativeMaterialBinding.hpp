#pragma once
#include "Render/NativeCameraBindingState.hpp"
#include "Render/NativeIaBinding.hpp"
#include "Render/NativeBufferRead.hpp"
#include "Render/StereoDescriptorAudit.hpp"
#include <array>
#include <cstddef>
#include <cstdint>
#include <cstring>

namespace cvr::stereo::material_binding {
inline constexpr uint32_t ShaderRegister=4,StageIndex=1,LogicalBytes=292;
enum class Status:uint32_t {
    Ignored,Registered,Transient,UnsupportedExecutable,MissingContext,
    InvalidBinding,DescriptorMismatch,UnsupportedResource,MissingObservation,
    StaleObservation,Capacity,Failure
};
struct Record {
    uint64_t gpu{},resource{},descriptor{};
    uint32_t bytes{},kind{};
    bool operator==(const Record&)const=default;
};
struct CpuSlot {
    uint64_t layout{},table{},address{},descriptor{};
    uint32_t parameter{},index{},tableOffset{};
    bool operator==(const CpuSlot&)const=default;
};
struct Snapshot {
    camera_upload::Context context;
    uint64_t registry{},recordAddress{};
    uint32_t handle{};
    Record record;
    CpuSlot slot;
    bool operator==(const Snapshot&)const=default;
};
namespace detail {
using MemoryReader=geometry::MemoryReader;
#if CVR_RUST_VRIK
// The rules below: rust/crates/cvr-render/src/material.rs (tools/single_pass_tests/material_binding_test runs its cases
// against them in its Rust variant).
static_assert(sizeof(Record)==32 && sizeof(CpuSlot)==48 && sizeof(Snapshot)==128 && offsetof(Snapshot,record)==48 &&
    offsetof(Snapshot,slot)==80,"mirrored by material.rs");
extern "C" {
bool cvr_mat_read_snapshot(MemoryReader,void*,uint64_t,const camera_upload::Context*,uint32_t,Snapshot*,bool);
bool cvr_mat_fits_cbv(const Snapshot*,uint64_t,uint32_t);
bool cvr_mat_matches_draw(const Snapshot*,const Snapshot*,uint32_t,uint32_t,uint64_t,uint32_t,uint64_t,uint32_t);
void cvr_mat_native(const Snapshot*,geometry::NativeBuffer*);
}
#endif
#if CVR_RUST_VRIK
inline bool ReadSnapshotOnce(MemoryReader reader,void* user,uint64_t game,
                             camera_upload::Context context,uint32_t handle,Snapshot& out){
    return cvr_mat_read_snapshot(reader,user,game,&context,handle,&out,true);
}
#else
inline bool ReadSnapshotOnce(MemoryReader reader,void* user,uint64_t game,
                             camera_upload::Context context,uint32_t handle,Snapshot& out){
    out={};
    auto read=[&](uint64_t p,void* data,size_t bytes){return reader && p && p<=UINT64_MAX-bytes && reader(user,p,data,bytes);};
    if(!game || game>UINT64_MAX-0x3BCDCB0 || !context || context.context>UINT64_MAX-0x68 ||
        context.state>UINT64_MAX-0x70 || !handle || handle>0x100000)return false;
    Snapshot next;next.context=context;next.handle=handle;
    uint64_t list{},state{};
    if(!read(context.context+0x30,&list,8) || !read(context.context+0x60,&state,8) ||
        list!=context.list || state!=context.state || !read(game+0x3BCDCA8,&next.registry,8) || !next.registry)return false;
    const auto offset=0x5C0AF8+uint64_t(handle-1)*0xB0;
    if(next.registry>UINT64_MAX-offset-0x50)return false;
    next.recordAddress=next.registry+offset;std::array<uint8_t,0x50> bytes{};
    if(!read(next.recordAddress,bytes.data(),bytes.size()) || (bytes[6]&15)!=6)return false;
    std::memcpy(&next.record.bytes,bytes.data(),4);next.record.kind=bytes[6]>>4;
    if(next.record.bytes<LogicalBytes)return false;
    if(next.record.kind!=4){
        std::memcpy(&next.record.gpu,bytes.data()+8,8);
        std::memcpy(&next.record.resource,bytes.data()+16,8);
        std::memcpy(&next.record.descriptor,bytes.data()+0x48,8);
        if(!next.record.gpu || (next.record.gpu&255) || !next.record.resource || !next.record.descriptor ||
            next.record.gpu>UINT64_MAX-next.record.bytes)return false;
    }
    auto& slot=next.slot;
    if(!read(context.state+0x68,&slot.layout,8) || !read(context.state+0x28,&slot.table,8) ||
        !slot.layout || !slot.table || slot.layout>UINT64_MAX-0x478)return false;
    uint8_t parameter{};
    if(!read(slot.layout+0x477,&parameter,1) || parameter>=64)return false;
    slot.parameter=parameter;const auto entry=slot.layout+0x38+uint64_t(parameter)*16;
    uint32_t type{},at{};uint16_t first{},count{};
    if(!read(entry,&type,4) || type!=2 || !read(entry+4,&at,4) || !read(entry+8,&first,2) ||
        !read(entry+10,&count,2) || first>ShaderRegister || ShaderRegister-first>=count)return false;
    slot.tableOffset=ShaderRegister-first;
    if(at>=8192 || slot.tableOffset>=8192-at)return false;
    slot.index=at+slot.tableOffset;
    if(slot.table>UINT64_MAX-uint64_t(slot.index)*8-8)return false;
    slot.address=slot.table+uint64_t(slot.index)*8;
    if(!read(slot.address,&slot.descriptor,8) || !slot.descriptor ||
        (next.record.kind!=4 && slot.descriptor!=next.record.descriptor))return false;
    out=next;return true;
}
#endif
// The fingerprinted bind has just selected this CPU slot. A transient handle
// proves its current descriptor only; its registry resource fields are ignored.
#if CVR_RUST_VRIK
inline bool ReadSnapshot(MemoryReader reader,void* user,uint64_t game,
                         camera_upload::Context context,uint32_t handle,Snapshot& out){
    return cvr_mat_read_snapshot(reader,user,game,&context,handle,&out,false);
}
#else
inline bool ReadSnapshot(MemoryReader reader,void* user,uint64_t game,
                         camera_upload::Context context,uint32_t handle,Snapshot& out){
    out={};Snapshot a,b;
    if(!ReadSnapshotOnce(reader,user,game,context,handle,a) ||
        !ReadSnapshotOnce(reader,user,game,context,handle,b) || a!=b)return false;
    out=a;return true;
}
#endif
#if CVR_RUST_VRIK
inline bool FitsCbv(const Snapshot& source,uint64_t address,uint32_t bytes){return cvr_mat_fits_cbv(&source,address,bytes);}
#else
inline bool FitsCbv(const Snapshot& source,uint64_t address,uint32_t bytes){
    if(!source.handle || source.record.bytes<LogicalBytes || !address || (address&255) ||
        bytes<LogicalBytes || bytes>65536 || (bytes&255) || address>UINT64_MAX-LogicalBytes)return false;
    return source.record.kind==4 || (source.record.gpu==address && source.record.resource &&
        source.record.descriptor==source.slot.descriptor);
}
#endif
#if CVR_RUST_VRIK
inline bool MatchesDraw(const Snapshot& saved,const Snapshot& current,uint32_t parameter,uint32_t offset,
                        uint64_t savedAddress,uint32_t savedBytes,uint64_t actualAddress,uint32_t actualBytes){
    return cvr_mat_matches_draw(&saved,&current,parameter,offset,savedAddress,savedBytes,actualAddress,actualBytes);
}
#else
inline bool MatchesDraw(const Snapshot& saved,const Snapshot& current,uint32_t parameter,uint32_t offset,
                        uint64_t savedAddress,uint32_t savedBytes,uint64_t actualAddress,uint32_t actualBytes){
    return saved==current && current.slot.parameter==parameter && current.slot.tableOffset==offset &&
        savedAddress==actualAddress && savedBytes==actualBytes && FitsCbv(current,actualAddress,actualBytes);
}
#endif
#if CVR_RUST_VRIK
inline geometry::NativeBuffer Native(const Snapshot& source){geometry::NativeBuffer out;cvr_mat_native(&source,&out);return out;}
#else
inline geometry::NativeBuffer Native(const Snapshot& source){
    if(source.record.kind==4)return {};
    return {source.record.resource,source.record.gpu,source.record.bytes,source.registry,
        source.context.context,source.handle,source.record.kind};
}
#endif
}
struct Lease {
    Snapshot native;
    descriptors::CbvBinding binding;
    descriptors::DescriptorStamp stamp;
    geometry::BufferLease source;
    uint64_t sourceOffset{},serial{};
    buffer_read::Proof readProof;
};
// Call once BEFORE installing the 0x336B54 detour. The cached full-body proof
// remains usable after the hook patches its entry; a late first check fails.
bool Compatible(uintptr_t gameBase);
// Call after original 0x336B54 returns, with its stage INDEX (PS=1), while the
// engine owns the native bind. A 0x337968 stage mask must be decoded by its caller.
Status ObserveBind(uintptr_t gameBase,uint32_t binding,uint32_t handle,uint32_t stageIndex);
// Only at an actual native draw. Registered returns the retained exact source;
// Transient returns descriptor identity with no source ownership or byte proof.
Status ResolveAtDraw(ID3D12GraphicsCommandList*,uint64_t root,uint32_t parameter,uint32_t tableOffset,Lease&);
// Root integration must call Reset on recording reset/destruction and Clear at
// observation shutdown. At most 128 current recording slots are retained.
void Reset(ID3D12GraphicsCommandList*);
void Clear();
}
