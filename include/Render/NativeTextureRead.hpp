#pragma once
#include <cstddef>
#include "Render/NativeBufferRead.hpp"
#include "Render/NativeCommandRecording.hpp"
#include "Render/RegisteredConstantUpload.hpp"
#include "Render/NativeSourceCbvDetails.hpp"
#include <d3d12.h>
#include <wrl/client.h>
#include <array>
#include <cstdint>
#include <cstring>
#include <cmath>
#include <memory>
#include <utility>
#include <vector>

namespace cvr::stereo::texture_read {
struct NativeTexture {uint64_t registry{},resource{};uint32_t handle{},reserved{};};
struct State {uint32_t value{},status{};};
namespace detail {
#if CVR_RUST_VRIK
// The pure rules below: rust/crates/cvr-render/src/texture_read.rs (tools/single_pass_tests/native_texture_state_test runs
// its cases against them in its Rust variant).
extern "C" {
bool cvr_tr_copy_read_mask(State,uint32_t*);
bool cvr_tr_has_read_state(State,uint32_t);
bool cvr_tr_source_read_state(State,UINT);
bool cvr_tr_writable_state(State,uint32_t);
bool cvr_tr_alpha_format(DXGI_FORMAT);
bool cvr_tr_typed_format(DXGI_FORMAT);
bool cvr_tr_immutable_shape(const D3D12_RESOURCE_DESC*);
bool cvr_tr_shape(const D3D12_RESOURCE_DESC*);
bool cvr_tr_view_mips(const D3D12_RESOURCE_DESC*,const D3D12_SHADER_RESOURCE_VIEW_DESC*,UINT*,UINT*);
bool cvr_tr_view_subresources(const D3D12_RESOURCE_DESC*,const D3D12_SHADER_RESOURCE_VIEW_DESC*,UINT*,size_t,size_t*);
bool cvr_tr_footprint_range(const D3D12_PLACED_SUBRESOURCE_FOOTPRINT*,UINT,uint64_t,uint64_t,uint64_t*);
uint64_t cvr_tr_fixed_owner(const std::array<uint8_t,0x58>*);
}
#endif
inline constexpr uint32_t ShaderReads=D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE|D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
inline constexpr uint32_t AllowedReads=ShaderReads|D3D12_RESOURCE_STATE_COPY_SOURCE;
#if CVR_RUST_VRIK
inline bool CopyReadMask(State before,uint32_t& required){return cvr_tr_copy_read_mask(before,&required);}
#else
inline bool CopyReadMask(State before,uint32_t& required){
    required=0;
    if(before.status!=1 && before.status!=2 && before.status!=5)return false;
    const auto previous=before.status==5?0u:before.value;
    if(previous&~AllowedReads)return false;
    required=previous|AllowedReads;return true;
}
#endif
#if CVR_RUST_VRIK
inline bool HasReadState(State state,uint32_t required){return cvr_tr_has_read_state(state,required);}
#else
inline bool HasReadState(State state,uint32_t required){
    return (state.status==1 || state.status==2) && !(state.value&~AllowedReads) && (state.value&required)==required;
}
#endif
#if CVR_RUST_VRIK
inline bool SourceReadState(State state,UINT subresources){return cvr_tr_source_read_state(state,subresources);}
#else
inline bool SourceReadState(State state,UINT subresources){
    return subresources && (state.status==1 || (state.status==2 && subresources==1)) &&
        (state.value&ShaderReads) && !(state.value&~AllowedReads);
}
#endif
#if CVR_RUST_VRIK
inline bool WritableState(State state,uint32_t expected){return cvr_tr_writable_state(state,expected);}
#else
inline bool WritableState(State state,uint32_t expected){
    return (expected==D3D12_RESOURCE_STATE_RENDER_TARGET || expected==D3D12_RESOURCE_STATE_DEPTH_WRITE) &&
        state.status==1 && state.value==expected;
}
#endif
struct Allocation {uint64_t heap{},offset{},bytes{};uint32_t kind{};};
#if CVR_RUST_VRIK
static_assert(sizeof(Allocation)==32,"mirrored by rust/crates/cvr-render/src/texture_read.rs");
extern "C" {
bool cvr_tr_known_allocation(const Allocation*);
bool cvr_tr_could_alias(const Allocation*,const Allocation*);
bool cvr_tr_alias_affects(uint64_t,const Allocation*,uint64_t,const Allocation*,uint64_t,const Allocation*);
}
#endif
#if CVR_RUST_VRIK
inline bool KnownAllocation(const Allocation& allocation){return cvr_tr_known_allocation(&allocation);}
#else
inline bool KnownAllocation(const Allocation& allocation){
    return allocation.kind==1 || (allocation.kind==2 && allocation.heap && allocation.bytes &&
        allocation.bytes!=UINT64_MAX && allocation.offset<=UINT64_MAX-allocation.bytes);
}
#endif
#if CVR_RUST_VRIK
inline bool CouldAlias(const Allocation& source,const Allocation& other){return cvr_tr_could_alias(&source,&other);}
#else
inline bool CouldAlias(const Allocation& source,const Allocation& other){
    if(source.kind==1 || other.kind==1)return false;
    if(source.heap && other.heap && source.heap!=other.heap)return false;
    if(!KnownAllocation(source) || !KnownAllocation(other))return true;
    return source.offset<other.offset+other.bytes && other.offset<source.offset+source.bytes;
}
#endif
#if CVR_RUST_VRIK
inline bool AliasAffects(uint64_t resource,const Allocation& source,uint64_t before,const Allocation& from,
                         uint64_t after,const Allocation& to){return cvr_tr_alias_affects(resource,&source,before,&from,after,&to);}
#else
inline bool AliasAffects(uint64_t resource,const Allocation& source,uint64_t before,const Allocation& from,
                         uint64_t after,const Allocation& to){
    if(resource==before || resource==after)return true;
    if(!before && !after)return source.kind!=1;
    return (before && CouldAlias(source,from)) || (after && CouldAlias(source,to));
}
#endif
struct CreationProof:buffer_read::detail::History {
    bool acquired{};
    CreationProof(bool enabled=false){valid=enabled;}
    bool Acquire(){if(!Matches(epoch))return false;acquired=true;return true;}
    void Written(uint64_t recording){
        if(acquired){Invalidate();return;}
        buffer_read::detail::History::Written(recording);
    }
};
#if CVR_RUST_VRIK
inline bool AlphaFormat(DXGI_FORMAT format){return cvr_tr_alpha_format(format);}
#else
inline bool AlphaFormat(DXGI_FORMAT format){
    return format==DXGI_FORMAT_BC1_UNORM_SRGB || format==DXGI_FORMAT_BC7_UNORM_SRGB;
}
#endif
#if CVR_RUST_VRIK
inline bool TypedFormat(DXGI_FORMAT format){return cvr_tr_typed_format(format);}
#else
inline bool TypedFormat(DXGI_FORMAT format){
    return format==DXGI_FORMAT_R8_UNORM || format==DXGI_FORMAT_R16_UNORM ||
        format==DXGI_FORMAT_R16G16_UNORM || format==DXGI_FORMAT_R16G16B16A16_UNORM ||
        format==DXGI_FORMAT_R11G11B10_FLOAT || format==DXGI_FORMAT_R32_UINT;
}
#endif
// Lifetime observation for shader-readable assets, not a copy-format contract.
#if CVR_RUST_VRIK
inline bool ImmutableShape(const D3D12_RESOURCE_DESC& desc){return cvr_tr_immutable_shape(&desc);}
#else
inline bool ImmutableShape(const D3D12_RESOURCE_DESC& desc){
    return desc.Dimension>=D3D12_RESOURCE_DIMENSION_TEXTURE1D && desc.Dimension<=D3D12_RESOURCE_DIMENSION_TEXTURE3D &&
        desc.Width && desc.Height && desc.DepthOrArraySize && desc.MipLevels && desc.Format!=DXGI_FORMAT_UNKNOWN &&
        desc.SampleDesc.Count==1 && !desc.SampleDesc.Quality && desc.Flags==D3D12_RESOURCE_FLAG_NONE &&
        desc.Layout==D3D12_TEXTURE_LAYOUT_UNKNOWN;
}
#endif
#if CVR_RUST_VRIK
inline bool Shape(const D3D12_RESOURCE_DESC& desc){return cvr_tr_shape(&desc);}
#else
inline bool Shape(const D3D12_RESOURCE_DESC& desc){
    if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_TEXTURE2D || !desc.Width || desc.Width>4096 ||
        !desc.Height || desc.Height>4096 || !desc.DepthOrArraySize || desc.DepthOrArraySize>2048 || desc.SampleDesc.Count!=1 ||
        desc.SampleDesc.Quality || desc.Flags!=D3D12_RESOURCE_FLAG_NONE || desc.Layout!=D3D12_TEXTURE_LAYOUT_UNKNOWN ||
        !desc.MipLevels)return false;
    if(!TypedFormat(desc.Format) && !AlphaFormat(desc.Format))return false;
    if(AlphaFormat(desc.Format) && (desc.Width%4 || desc.Height%4))return false;
    auto size=desc.Width>desc.Height?desc.Width:desc.Height;UINT levels=1;
    while(size>1){size>>=1;++levels;}
    return desc.MipLevels<=levels && UINT(desc.MipLevels)*desc.DepthOrArraySize<=D3D12_REQ_SUBRESOURCES;
}
#endif
#if CVR_RUST_VRIK
inline bool ViewMips(const D3D12_RESOURCE_DESC& desc,const D3D12_SHADER_RESOURCE_VIEW_DESC& view,UINT& first,UINT& count){
    return cvr_tr_view_mips(&desc,&view,&first,&count);
}
#else
inline bool ViewMips(const D3D12_RESOURCE_DESC& desc,const D3D12_SHADER_RESOURCE_VIEW_DESC& view,UINT& first,UINT& count){
    first=count=0;
    if(!Shape(desc) || desc.DepthOrArraySize!=1 || view.ViewDimension!=D3D12_SRV_DIMENSION_TEXTURE2D || view.Format!=desc.Format ||
        view.Shader4ComponentMapping!=D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING || view.Texture2D.PlaneSlice ||
        view.Texture2D.MostDetailedMip>=desc.MipLevels)return false;
    const auto remaining=UINT(desc.MipLevels)-view.Texture2D.MostDetailedMip;
    const auto visible=view.Texture2D.MipLevels==UINT_MAX?remaining:view.Texture2D.MipLevels;
    if(!visible || visible>remaining)return false;
    first=view.Texture2D.MostDetailedMip;count=visible;return true;
}
#endif
#if CVR_RUST_VRIK
inline bool ViewSubresources(const D3D12_RESOURCE_DESC& desc,const D3D12_SHADER_RESOURCE_VIEW_DESC& view,
                            std::vector<UINT>& output){
    output.assign(D3D12_REQ_SUBRESOURCES,0);size_t count{};
    const bool ok=cvr_tr_view_subresources(&desc,&view,output.data(),output.size(),&count);
    output.resize(count);return ok;
}
#else
inline bool ViewSubresources(const D3D12_RESOURCE_DESC& desc,const D3D12_SHADER_RESOURCE_VIEW_DESC& view,
                            std::vector<UINT>& output){
    output.clear();
    if(!Shape(desc) || view.Format!=desc.Format ||
        view.Shader4ComponentMapping!=D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING)return false;
    UINT first{},count{},slice{},slices{};float clamp{};
    if(view.ViewDimension==D3D12_SRV_DIMENSION_TEXTURE2D){
        if(!ViewMips(desc,view,first,count))return false;
        slices=1;clamp=view.Texture2D.ResourceMinLODClamp;
    }else if(view.ViewDimension==D3D12_SRV_DIMENSION_TEXTURE2DARRAY){
        const auto& array=view.Texture2DArray;
        if(array.PlaneSlice || array.MostDetailedMip>=desc.MipLevels || !array.ArraySize ||
            array.FirstArraySlice>=desc.DepthOrArraySize || array.ArraySize>UINT(desc.DepthOrArraySize)-array.FirstArraySlice)return false;
        first=array.MostDetailedMip;const UINT remaining=UINT(desc.MipLevels)-first;
        count=array.MipLevels==UINT_MAX?remaining:array.MipLevels;
        if(!count || count>remaining)return false;
        slice=array.FirstArraySlice;slices=array.ArraySize;clamp=array.ResourceMinLODClamp;
    }else return false;
    if(!std::isfinite(clamp) || clamp<0 || clamp>desc.MipLevels-1)return false;
    std::vector<UINT> indices;indices.reserve(size_t(count)*slices);
    // Keep native array indices, including a nonzero FirstArraySlice. Plane0 only.
    for(UINT layer=0;layer<slices;++layer)for(UINT mip=0;mip<count;++mip)
        indices.push_back(first+mip+(slice+layer)*UINT(desc.MipLevels));
    output=std::move(indices);return true;
}
#endif
#if CVR_RUST_VRIK
inline bool FootprintRange(const D3D12_PLACED_SUBRESOURCE_FOOTPRINT& layout,UINT rows,uint64_t rowBytes,
                           uint64_t destinationBytes,uint64_t& bytes){
    return cvr_tr_footprint_range(&layout,rows,rowBytes,destinationBytes,&bytes);
}
#else
inline bool FootprintRange(const D3D12_PLACED_SUBRESOURCE_FOOTPRINT& layout,UINT rows,uint64_t rowBytes,
                           uint64_t destinationBytes,uint64_t& bytes){
    bytes=0;
    if(layout.Offset%D3D12_TEXTURE_DATA_PLACEMENT_ALIGNMENT || !rows || !rowBytes ||
        layout.Footprint.Depth!=1 || !layout.Footprint.RowPitch ||
        layout.Footprint.RowPitch%D3D12_TEXTURE_DATA_PITCH_ALIGNMENT || rowBytes>layout.Footprint.RowPitch ||
        layout.Offset>destinationBytes)return false;
    const uint64_t beforeLast=uint64_t(layout.Footprint.RowPitch)*(rows-1);
    if(beforeLast>UINT64_MAX-rowBytes || beforeLast+rowBytes>destinationBytes-layout.Offset)return false;
    bytes=beforeLast+rowBytes;return true;
}
#endif
struct MipFootprint {
    UINT mip{};D3D12_PLACED_SUBRESOURCE_FOOTPRINT layout{};UINT rows{};
    uint64_t rowBytes{},bytes{};
};
#if CVR_RUST_VRIK
static_assert(sizeof(MipFootprint)==64,"mirrored by rust/crates/cvr-render/src/texture_read.rs");
extern "C" {
bool cvr_tr_subresource_layout(ID3D12Device*,const D3D12_RESOURCE_DESC*,UINT,uint64_t,MipFootprint*);
bool cvr_tr_view_layouts(ID3D12Device*,const D3D12_RESOURCE_DESC*,const D3D12_SHADER_RESOURCE_VIEW_DESC*,uint64_t,MipFootprint*,size_t,size_t*);
}
#endif
#if CVR_RUST_VRIK
inline bool SubresourceLayout(ID3D12Device* device,const D3D12_RESOURCE_DESC& desc,UINT subresource,
                              uint64_t offset,MipFootprint& output){
    return cvr_tr_subresource_layout(device,&desc,subresource,offset,&output);
}
#else
inline bool SubresourceLayout(ID3D12Device* device,const D3D12_RESOURCE_DESC& desc,UINT subresource,
                              uint64_t offset,MipFootprint& output){
    output={};if(!device || !Shape(desc) || subresource>=UINT(desc.MipLevels)*desc.DepthOrArraySize ||
        offset%D3D12_TEXTURE_DATA_PLACEMENT_ALIGNMENT)return false;
    MipFootprint next;next.mip=subresource;uint64_t total{};
    device->GetCopyableFootprints(&desc,subresource,1,offset,&next.layout,&next.rows,&next.rowBytes,&total);
    if(!total || total==UINT64_MAX || next.layout.Offset!=offset || next.layout.Footprint.Format!=desc.Format ||
        !FootprintRange(next.layout,next.rows,next.rowBytes,UINT64_MAX,next.bytes))return false;
    output=next;return true;
}
#endif
// Layout metadata alone grants no source identity, content, state or copy lease.
#if CVR_RUST_VRIK
inline bool ViewLayouts(ID3D12Device* device,const D3D12_RESOURCE_DESC& desc,const D3D12_SHADER_RESOURCE_VIEW_DESC& view,
                        uint64_t offset,std::vector<MipFootprint>& output){
    output.assign(D3D12_REQ_SUBRESOURCES,MipFootprint{});size_t count{};
    const bool ok=cvr_tr_view_layouts(device,&desc,&view,offset,output.data(),output.size(),&count);
    output.resize(count);return ok;
}
#else
inline bool ViewLayouts(ID3D12Device* device,const D3D12_RESOURCE_DESC& desc,const D3D12_SHADER_RESOURCE_VIEW_DESC& view,
                        uint64_t offset,std::vector<MipFootprint>& output){
    output.clear();std::vector<UINT> indices;
    if(!device || offset%D3D12_TEXTURE_DATA_PLACEMENT_ALIGNMENT || !ViewSubresources(desc,view,indices))return false;
    std::vector<MipFootprint> result;result.reserve(indices.size());
    for(size_t index=0;index<indices.size();++index){MipFootprint layout;
        if(!SubresourceLayout(device,desc,indices[index],offset,layout))return false;
        result.push_back(layout);const auto end=layout.layout.Offset+layout.bytes;
        if(index+1<indices.size()){
            if(end>UINT64_MAX-(D3D12_TEXTURE_DATA_PLACEMENT_ALIGNMENT-1))return false;
            offset=(end+D3D12_TEXTURE_DATA_PLACEMENT_ALIGNMENT-1)&~uint64_t(D3D12_TEXTURE_DATA_PLACEMENT_ALIGNMENT-1);
        }
    }
    output=std::move(result);return true;
}
#endif
#if CVR_RUST_VRIK
inline uint64_t FixedOwner(const std::array<uint8_t,0x58>& record){return cvr_tr_fixed_owner(&record);}
#else
inline uint64_t FixedOwner(const std::array<uint8_t,0x58>& record){
    uint64_t owner{};uint32_t binding{};uint16_t flags{};
    std::memcpy(&owner,record.data()+0x10,8);std::memcpy(&binding,record.data()+0x48,4);std::memcpy(&flags,record.data()+0x56,2);
    return record[8]==1 && binding==ShaderReads && (flags==0x41 || flags==0xc1)?owner:0;
}
#endif
struct FixedHandle {
    NativeTexture native;uint64_t record{},lock{},owner{};int32_t references{};
};
struct FixedPinSnapshot {
    FixedHandle handle;source_cbv_details::Allocation allocation;source_cbv_details::Allocator allocator;
};
using FixedRead=bool(*)(void*,uint64_t,void*,size_t);
struct FixedPinOperations {
    FixedRead read{};
    bool(*tryLock)(void*,uint64_t)noexcept{};
    bool(*retainHandle)(void*,uint64_t,int32_t)noexcept{};
    bool(*retainOwner)(void*,uint64_t,bool)noexcept{}; // false allocator, true allocation; retains only, never cleans up.
    void(*releaseHandle)(void*,uint32_t)noexcept{};
    void(*unlock)(void*,uint64_t)noexcept{};
};
#if CVR_RUST_VRIK
// The bootstrap: rust/crates/cvr-render/src/texture_pins.rs (C-unwind there: a fixture's reader may throw through it).
static_assert(sizeof(FixedHandle)==56 && sizeof(FixedPinSnapshot)==312 && offsetof(FixedPinSnapshot,allocation)==56 &&
    offsetof(FixedPinSnapshot,allocator)==248 && sizeof(FixedPinOperations)==48,"mirrored by rust/crates/cvr-render/src/texture_pins.rs");
extern "C" {
bool cvr_tr_read_fixed_handle(FixedRead,void*,uint64_t,const NativeTexture*,FixedHandle*);
bool cvr_tr_read_fixed_pin(FixedRead,void*,uint64_t,const NativeTexture*,FixedPinSnapshot*);
bool cvr_tr_same_fixed_pins(const FixedPinSnapshot*,const FixedPinSnapshot*);
bool cvr_tr_bootstrap_fixed_pins(const FixedPinOperations*,void*,uint64_t,const NativeTexture*,FixedPinSnapshot*,uint32_t*);
}
inline bool ReadFixedHandle(FixedRead read,void* user,uint64_t base,const NativeTexture& wanted,FixedHandle& output){
    return cvr_tr_read_fixed_handle(read,user,base,&wanted,&output);
}
inline bool ReadFixedPin(FixedRead read,void* user,uint64_t base,const NativeTexture& wanted,FixedPinSnapshot& output){
    return cvr_tr_read_fixed_pin(read,user,base,&wanted,&output);
}
inline bool SameFixedPins(const FixedPinSnapshot& a,const FixedPinSnapshot& b){return cvr_tr_same_fixed_pins(&a,&b);}
inline bool BootstrapFixedPins(const FixedPinOperations& ops,void* user,uint64_t base,const NativeTexture& wanted,
                               FixedPinSnapshot& output,uint32_t& rejected){
    return cvr_tr_bootstrap_fixed_pins(&ops,user,base,&wanted,&output,&rejected);
}
#else
template<class T> inline bool FixedField(FixedRead read,void* user,uint64_t address,uint64_t offset,T& output){
    return address && offset<=UINT64_MAX-address && source_cbv_details::detail::Copy(read,user,address+offset,&output,sizeof(output));
}
// Requires the caller's successful exclusive try-lock, not an observed lock byte.
// Published free slots are cleared before that same lock is released by every
// guarded allocator/free/failure/shutdown path. Refcount zero is never revived.
inline bool ReadFixedHandle(FixedRead read,void* user,uint64_t base,const NativeTexture& wanted,FixedHandle& output){
    output={};uint64_t registry{},lock{},resource{};uint8_t stopped{},locked{};int32_t references{};
    if(!base || !wanted.registry || !wanted.resource || !wanted.handle || wanted.handle>32768 ||
        !FixedField(read,user,base,0x3BCDCA8,registry) || registry!=wanted.registry ||
        registry>UINT64_MAX-0xCEBF30-0x1650 || !FixedField(read,user,registry,1,stopped) || stopped ||
        !FixedField(read,user,registry,0x2F1C0,lock) || lock!=registry+0x2F180 ||
        !FixedField(read,user,lock,0,locked) || locked!=0xFF)return false;
    const auto record=registry+0x2F1D8+uint64_t(wanted.handle-1)*0xB0;
    std::array<uint8_t,0x58> bytes{};
    if(!FixedField(read,user,record-8,0,references) || references<=0 || references>=INT32_MAX ||
        !source_cbv_details::detail::Copy(read,user,record,bytes.data(),bytes.size()))return false;
    std::memcpy(&resource,bytes.data(),8);const auto owner=FixedOwner(bytes);
    if(resource!=wanted.resource || !owner)return false;
    const auto table=registry+0xCEBF30;uint64_t tableLock{};uint32_t freeCount{},free{};
    if(!FixedField(read,user,table,0,tableLock) || tableLock!=lock ||
        !FixedField(read,user,table,8,freeCount) || freeCount>32)return false;
    for(uint32_t i=0;i<32;++i){int32_t refs{};uint32_t handle{};
        if(!FixedField(read,user,table,0x10+uint64_t(i)*0xB0,refs) || refs< -1 || refs==INT32_MAX ||
            !FixedField(read,user,table,0x40+uint64_t(i)*0xB0,handle) || handle>32768 || handle==wanted.handle)return false;
        free+=refs== -1;
    }
    if(free!=freeCount)return false;
    output={wanted,record,lock,owner,references};return true;
}
inline bool ReadFixedPin(FixedRead read,void* user,uint64_t base,const NativeTexture& wanted,FixedPinSnapshot& output){
    output={};FixedPinSnapshot next;if(!ReadFixedHandle(read,user,base,wanted,next.handle))return false;
    next.allocation=source_cbv_details::detail::ReadAllocation(read,user,base,next.handle.owner);
    next.allocator=source_cbv_details::detail::ReadAllocator(read,user,base,wanted.registry,next.allocation);
    using O=source_cbv_details::Observation;
    if(next.allocation.observation!=O::Known || next.allocation.resource!=wanted.resource || next.allocation.kind>1 ||
        next.allocation.references>=INT32_MAX-1 || next.allocator.observation!=O::Known ||
        next.allocator.references>=INT32_MAX-1 || !next.allocator.pimplMatches)return false;
    output=next;return true;
}
inline bool SameFixedPins(const FixedPinSnapshot& a,const FixedPinSnapshot& b){
    const auto& x=a.allocation;const auto& y=b.allocation;const auto& u=a.allocator;const auto& v=b.allocator;
    return a.handle.native.registry==b.handle.native.registry && a.handle.native.resource==b.handle.native.resource &&
        a.handle.native.handle==b.handle.native.handle && a.handle.record==b.handle.record && a.handle.lock==b.handle.lock &&
        a.handle.owner==b.handle.owner && x.address==y.address && x.vtable==y.vtable && x.pimpl==y.pimpl &&
        x.resource==y.resource && x.bytes==y.bytes && x.heap==y.heap && x.block==y.block && x.handle==y.handle && x.kind==y.kind &&
        u.address==v.address && u.vtable==v.vtable && u.pimpl==v.pimpl;
}
// Shared by runtime and pure race fixtures. Native owners remain in the caller's
// packet even on failure, so their COM releases happen AFTER this guard unlocks.
inline bool BootstrapFixedPins(const FixedPinOperations& ops,void* user,uint64_t base,const NativeTexture& wanted,
                               FixedPinSnapshot& output,uint32_t& rejected){
    output={};rejected=1;
    if(!ops.read || !ops.tryLock || !ops.retainHandle || !ops.retainOwner || !ops.releaseHandle || !ops.unlock ||
        !wanted.registry || wanted.registry>UINT64_MAX-0x2F180)return false;
    const auto lock=wanted.registry+0x2F180;
    rejected=2;if(!ops.tryLock(user,lock))return false;
    struct Guard {
        const FixedPinOperations& ops;void* user;uint64_t lock;uint32_t handle{};
        ~Guard(){if(handle)ops.releaseHandle(user,handle);ops.unlock(user,lock);}
    } guard{ops,user,lock};
    FixedHandle first;rejected=3;if(!ReadFixedHandle(ops.read,user,base,wanted,first) || first.references>=INT32_MAX-1)return false;
    rejected=4;if(!ops.retainHandle(user,first.record-8,first.references))return false;
    guard.handle=wanted.handle;
    FixedPinSnapshot snapshot;rejected=5;if(!ReadFixedPin(ops.read,user,base,wanted,snapshot))return false;
    rejected=6;if(!ops.retainOwner(user,snapshot.allocator.address,false))return false;
    rejected=7;if(!ops.retainOwner(user,snapshot.allocation.address,true))return false;
    FixedPinSnapshot after;rejected=8;
    if(!ReadFixedPin(ops.read,user,base,wanted,after) || !SameFixedPins(snapshot,after))return false;
    output=snapshot;rejected=0;return true;
}
#endif
}
using ReadMemory=bool(*)(void*,uint64_t,void*,size_t);
#if CVR_RUST_VRIK
static_assert(sizeof(NativeTexture)==24 && sizeof(State)==8,"mirrored by rust/crates/cvr-render/src/texture_read.rs");
extern "C" State cvr_tr_read_state(const NativeTexture*,uint64_t,uint64_t,ReadMemory,void*);
inline State ReadState(const NativeTexture& source,uint64_t context,uint64_t list,ReadMemory read,void* user=nullptr){
    return cvr_tr_read_state(&source,context,list,read,user);
}
#else
inline State ReadState(const NativeTexture& source,uint64_t context,uint64_t list,ReadMemory read,void* user=nullptr){
    auto memory=[&](uint64_t address,void* output,size_t bytes){return address && address<=UINT64_MAX-bytes && read && read(user,address,output,bytes);};
    if(!source.registry || !source.resource || !source.handle || source.handle>32768 || !context || !list || context>UINT64_MAX-0x630)return {0,3};
    const auto offset=0x2f1d8+uint64_t(source.handle-1)*0xb0;
    if(source.registry>UINT64_MAX-offset-0x58)return {0,3};
    std::array<uint8_t,0x58> record{};uint64_t resource{},actualList{};uint32_t pending{};
    if(!memory(source.registry+offset,record.data(),record.size()))return {0,3};
    std::memcpy(&resource,record.data(),8);
    if(resource!=source.resource || !memory(context+0x30,&actualList,8) || actualList!=list)return {0,3};
    if(record[0x56]&0x40)return {0,8};
    if(!memory(context+0x534,&pending,4))return {0,3};
    if(pending)return {0,4};
    struct Map {uint64_t buckets;uint32_t count,bucketCount;uint64_t data;uint32_t capacity,stride;};
    static_assert(sizeof(Map)==32);Map map{};
    if(!memory(context+0x538,&map,sizeof(map)))return {0,6};
    if(map.count>map.capacity || map.count>map.bucketCount || map.capacity>1048576 || map.bucketCount>1048576)return {0,6};
    const bool emptyStorage=!map.buckets && !map.bucketCount && !map.data && !map.capacity;
    if(emptyStorage){
        if(map.stride && (map.stride<20 || map.stride>64 || map.stride%4))return {0,6};
        return {0,5};
    }
    if(!map.bucketCount || !map.capacity || map.stride<20 || map.stride>64 || map.stride%4 || !map.buckets || !map.data)return {0,6};
    if(!map.count)return {0,5};
    const auto bucket=uint64_t(source.handle%map.bucketCount)*4;
    if(map.buckets>UINT64_MAX-bucket-4)return {0,6};
    uint32_t at{};if(!memory(map.buckets+bucket,&at,4))return {0,6};
    for(UINT visited=0;visited<map.count && visited<64 && at!=UINT_MAX;++visited){
        if(at>=map.capacity || map.data>UINT64_MAX-uint64_t(at)*map.stride-20)return {0,6};
        std::array<uint32_t,5> entry{};if(!memory(map.data+uint64_t(at)*map.stride,entry.data(),20))return {0,6};
        if(entry[1]==source.handle && entry[2]==source.handle){
            if(entry[4]!=0x80000000)return {entry[4],1};
            const UINT first=entry[3]&65535,count=entry[3]>>16;
            if(first==65535 || count!=1)return {0,7};
            uint64_t data{};uint32_t capacity{},size{},state{};
            if(!memory(context+0x598,&data,8) || !data || !memory(context+0x5a0,&capacity,4) ||
                !memory(context+0x5a4,&size,4) || first>=size || size>capacity || size>65535 ||
                data>UINT64_MAX-uint64_t(first)*4-4 || !memory(data+uint64_t(first)*4,&state,4) || state==0x80000000)return {0,7};
            return {state,2};
        }
        at=entry[0];
    }
    // Only a terminated bucket chain proves absence; a bounded walk can also
    // end because the table is cyclic, corrupt, or too long to verify.
    return {0,at==UINT_MAX?5u:6u};
}
#endif
struct Lease {Microsoft::WRL::ComPtr<ID3D12Resource> resource;NativeTexture native;uint64_t receipt{},owner{};};
struct ImmutableLease {Microsoft::WRL::ComPtr<IUnknown> owner;Lease source;};
class FixedAssetPins {
    struct Data;std::shared_ptr<const Data> data_;
    friend bool PinFixedAsset(ID3D12GraphicsCommandList*,uint64_t,FixedAssetPins&);
    friend bool GetFixedPinnedResource(const FixedAssetPins&,Microsoft::WRL::ComPtr<ID3D12Resource>&);
    friend bool AcquirePinnedImmutable(ID3D12GraphicsCommandList*,const FixedAssetPins&,ImmutableLease&);
public:explicit operator bool()const{return bool(data_);}
};
// A numeric weak candidate is a hint only. Native byte-lock/positive-ref CAS
// establishes ownership without calling COM on the weak resource. This grants
// no descriptor identity, fixed contents, residency or shader-read permission.
bool PinFixedAsset(ID3D12GraphicsCommandList*,uint64_t candidateResource,FixedAssetPins&);
// Caller checks the original descriptor cookie/version using this owned object.
bool GetFixedPinnedResource(const FixedAssetPins&,Microsoft::WRL::ComPtr<ID3D12Resource>&);
// Original source callback only. Uses EXACT original pins, never a newly resolved
// owner. Independently requires the existing fixed initial-upload/state proof.
bool AcquirePinnedImmutable(ID3D12GraphicsCommandList*,const FixedAssetPins&,ImmutableLease&);
// registry,handle,resource,allocation,allocator,bootstrap rejection,expected,actual.
// Failure121 instead reports mismatching code RVA in slot0 and fingerprints in6/7.
// Bootstrap rejection3 reports coherent POD-only identity in3/4/6/7:
// actual resource, tag|(flags<<32), owner, refs|(binding<<32), or zeros if unread.
std::array<uint64_t,8> FixedPinLastDetails();
// Original draw callback with an already-held shader resource. Pins the typed
// allocation/allocator and checks the fixed initial-upload content receipt.
// Does not register a future native consumer or grant destination execution.
bool AcquireImmutable(ID3D12GraphicsCommandList*,ID3D12Resource*,ImmutableLease&);
bool ValidateImmutable(const ImmutableLease&,ID3D12CommandQueue* submissionQueue=nullptr);
struct ImmutablePageable {
    ImmutableLease source;
    Microsoft::WRL::ComPtr<ID3D12Pageable> pageable;
};
// Authenticates the held native allocation and returns its physical-memory owner:
// committed resource or exact placed heap. Retains all source allocation pins.
// This identifies a residency target only; it grants no ordering or read epoch.
bool GetImmutablePageable(ID3D12Device*,const ImmutableLease&,ImmutablePageable&);
struct ImmutableUse {ImmutableLease source;native_recording::Lease recording;};
// Original native recording only. Adds the retained fixed asset to the actual
// native residency/use set; does not infer an epoch from an earlier source fence.
bool RegisterImmutableUse(ID3D12GraphicsCommandList*,const ImmutableLease&,ImmutableUse&);
bool ValidateImmutableUse(const ImmutableUse&,ID3D12CommandQueue*);
// Allocation metadata includes private destinations so disjointness is known;
// native content receipts exclude them. Other observers include internal
// writes/barriers to owned native sources, independently of capture windows.
void ResourceCreated(ID3D12Resource*,D3D12_RESOURCE_STATES initialState,bool placed,ID3D12Heap* heap=nullptr,
                     uint64_t offset=0,uint64_t allocationBytes=0);
// Immutable API placement observation, including our private destinations.
// Caller already holds the live resource. Historical initial state is unknown;
// this does not establish native allocation ownership or current GPU access.
bool QueryObservedAllocation(const Microsoft::WRL::ComPtr<ID3D12Resource>&,constants::ResourceCreationInfo&);
// Works for buffers and textures. The caller keeps all nonnull resources live;
// unavailable observation or unproven disjoint placement fails closed.
bool AliasAffects(ID3D12Resource* watched,ID3D12Resource* before,ID3D12Resource* after);
void Barriers(ID3D12GraphicsCommandList*,UINT,const D3D12_RESOURCE_BARRIER*);
// Pass nullptr as the list for DiscardResource. The initial-write exemption
// requires the exact native uploader recording and its observed queue submission.
void Written(ID3D12GraphicsCommandList*,ID3D12Resource* destination);
// Reset follows successful COPY-list creation/Reset. Submission hooks bracket
// the real ExecuteCommandLists call on every observed queue, including COPY.
void Reset(ID3D12GraphicsCommandList*);
void BeforeSubmit(ID3D12CommandQueue*,UINT,ID3D12CommandList* const*);
void Submitted(ID3D12CommandQueue*,UINT,ID3D12CommandList* const*);
bool Compatible();
void ObserverUnavailable();
bool Acquire(ID3D12GraphicsCommandList*,ID3D12Resource* actualBoundTexture,Lease&);
// Actual-draw-only proof for a caller-retained native target. Requires an exact
// uniform tracked RENDER_TARGET or DEPTH_WRITE state in this native context;
// pending, absent, fixed, and per-subresource states fail closed. Never registers
// resource use, flushes barriers, or infers state from another recording.
bool ValidateWritableTarget(ID3D12GraphicsCommandList*,ID3D12Resource*,D3D12_RESOURCE_STATES expected);
struct WritableTargetIdentity {
    constants::ResourceCreationInfo creation;
    NativeTexture native;
    native_recording::Lease recording;
    camera_upload::Context originalContext;
    D3D12_RESOURCE_DESC resourceDesc{};
    D3D12_HEAP_DESC heapDesc{}; // Present only for creation.kind==2 (placed).
    D3D12_HEAP_PROPERTIES resourceHeapProperties{};
    D3D12_HEAP_FLAGS resourceHeapFlags{};
    D3D12_RESOURCE_STATES expectedWritableState{};
    std::array<uint8_t,0x58> nativeRecord{}; // Original audit bytes, not a full stable ABI.
    uint64_t allocatorOwnerObservation{}; // Numeric native +0x10, never a COM owner.
private:
    struct Data;std::shared_ptr<const Data> data_;
    friend bool CaptureWritableIdentity(ID3D12GraphicsCommandList*,ID3D12Resource*,D3D12_RESOURCE_STATES,WritableTargetIdentity&);
    friend bool ValidateWritableIdentity(const WritableTargetIdentity&,ID3D12CommandQueue*);
    friend void AuthenticateWritableIdentity(WritableTargetIdentity&);
};
// The Rust build: an identity the target handoff's observer (rust/crates/cvr-render/src/target_handoff_observer.rs)
// captured and copied out gets its authenticated copy (itself, as captured).
void AuthenticateWritableIdentity(WritableTargetIdentity&);
namespace detail {
struct WritableRecordObservation {
    // Resource identity is known. kind/owner remain uninterpreted numbers;
    // equality cannot authenticate a native allocation lifetime.
    uint64_t resource{},owner{};uint8_t kind{};
    bool operator==(const WritableRecordObservation&)const=default;
};
inline WritableRecordObservation WritableRecord(const std::array<uint8_t,0x58>& bytes){
    WritableRecordObservation value;std::memcpy(&value.resource,bytes.data(),8);
    std::memcpy(&value.owner,bytes.data()+0x10,8);value.kind=bytes[8];return value;
}
inline bool SameResourceDescription(const D3D12_RESOURCE_DESC& a,const D3D12_RESOURCE_DESC& b){
    return a.Dimension==b.Dimension && a.Alignment==b.Alignment && a.Width==b.Width && a.Height==b.Height &&
        a.DepthOrArraySize==b.DepthOrArraySize && a.MipLevels==b.MipLevels && a.Format==b.Format &&
        a.SampleDesc.Count==b.SampleDesc.Count && a.SampleDesc.Quality==b.SampleDesc.Quality && a.Layout==b.Layout && a.Flags==b.Flags;
}
inline bool SameHeapProperties(const D3D12_HEAP_PROPERTIES& a,const D3D12_HEAP_PROPERTIES& b){
    return a.Type==b.Type && a.CPUPageProperty==b.CPUPageProperty && a.MemoryPoolPreference==b.MemoryPoolPreference &&
        a.CreationNodeMask==b.CreationNodeMask && a.VisibleNodeMask==b.VisibleNodeMask;
}
inline bool SameHeapDescription(const D3D12_HEAP_DESC& a,const D3D12_HEAP_DESC& b){
    return a.SizeInBytes==b.SizeInBytes && a.Alignment==b.Alignment && a.Flags==b.Flags && SameHeapProperties(a.Properties,b.Properties);
}
inline bool SameCreationMetadata(const constants::ResourceCreationInfo& a,const constants::ResourceCreationInfo& b){
    return a.resource==b.resource && a.kind==b.kind && a.heap==b.heap && a.heapOffset==b.heapOffset &&
        a.allocationBytes==b.allocationBytes && a.initialStateKnown==b.initialStateKnown && a.initialState==b.initialState;
}
// Metadata equality is not authentication or an allocation-lifetime proof.
inline bool SameWritableIdentityMetadata(const WritableTargetIdentity& a,const WritableTargetIdentity& b){
    return SameCreationMetadata(a.creation,b.creation) && a.native.registry==b.native.registry &&
        a.native.resource==b.native.resource && a.native.handle==b.native.handle && a.native.reserved==b.native.reserved &&
        a.recording.token==b.recording.token && a.recording.generation==b.recording.generation &&
        a.recording.context==b.recording.context && a.recording.list==b.recording.list && a.recording.registry==b.recording.registry &&
        a.originalContext==b.originalContext && SameResourceDescription(a.resourceDesc,b.resourceDesc) &&
        SameHeapDescription(a.heapDesc,b.heapDesc) && SameHeapProperties(a.resourceHeapProperties,b.resourceHeapProperties) &&
        a.resourceHeapFlags==b.resourceHeapFlags && a.expectedWritableState==b.expectedWritableState &&
        a.nativeRecord==b.nativeRecord && a.allocatorOwnerObservation==b.allocatorOwnerObservation;
}
}
// Original native draw only. Authenticates the writable state at capture and
// retains COM resource/heap creation metadata: kind 1 committed, kind 2 placed,
// with the exact heap/offset/bytes tuple. Native allocator ownership is unknown;
// +0x10 and the original record bytes are observations, never native owner pins.
// Grants only metadata consistency and COM retention, not residency, alias
// exclusion, final resource state, content preservation, or export permission.
bool CaptureWritableIdentity(ID3D12GraphicsCommandList*,ID3D12Resource*,D3D12_RESOURCE_STATES expected,WritableTargetIdentity&);
// Rechecks authenticated metadata and native record identity without ReadState
// or native context maps. Only a supplied queue also requires the original
// recording's pending native submission; it does not reassert writable state.
bool ValidateWritableIdentity(const WritableTargetIdentity&,ID3D12CommandQueue* submissionQueue=nullptr);
// Required again at submission, with its actual queue, for an implicit receipt.
// An invalid receipt must disable the caller's snapshot-copy predicate, not only
// the later depth replay. During recording the queue argument can be omitted.
bool Validate(const Lease&,ID3D12CommandQueue* submissionQueue=nullptr);
// Native registration retains SRV reads while adding COPY_SOURCE. The native
// tracker owns this combined post-state, including an unseen peer's first use.
// No native maps are edited and no StateBefore is guessed. Failure 41 rejects
// the copy after verified SRV recovery; failure 40 leaves readability unverified
// after native mutation, so callers must stop replay. Neither emits a copy.
// A verified immutable creation receipt instead uses COMMON/read promotion;
// native registration retains residency and no source transition is emitted.
// The caller must predicate that snapshot copy and finalize its predicate after
// submission-time Validate. Failure 42 rejects an invalid receipt before mutation.
bool Copy(ID3D12GraphicsCommandList*,const Lease&,ID3D12Resource* buffer,const D3D12_PLACED_SUBRESOURCE_FOOTPRINT&);
using MipFootprint=detail::MipFootprint;
// mip is the original subresource index: mipSlice + arraySlice*MipLevels.
// Every view-visible mip/layer, using device-provided copy layouts. bytes is the
// unpadded last-row extent, so the absolute buffer end is layout.Offset+bytes.
// Compare rowBytes over rows at RowPitch strides, never footprint padding.
bool ViewFootprints(ID3D12Device*,const Lease&,const D3D12_SHADER_RESOURCE_VIEW_DESC&,uint64_t baseOffset,std::vector<MipFootprint>&);
bool CopySubresource(ID3D12GraphicsCommandList*,const Lease&,UINT subresource,ID3D12Resource* buffer,const D3D12_PLACED_SUBRESOURCE_FOOTPRINT&);
// 60 missing allocation/receipt, 61 observer, 62 retired receipt, 63 resource,
// 64 registry, 65 uploader queue, 66 invalidated, 67 uploads incomplete, 68 acquire;
// 69 unproven BC placement/heap, 70 view/layout, 71 footprint, 72 destination.
// 80 target arguments, 81 target context, 82 target identity, 83 target state,
// 84 target state/context/identity changed during validation.
// 100 physical-pageable arguments, 101 owner kind/receipt, 102 device mismatch.
// 103 writable identity arguments, 104 recording, 105 native record,
// 106 creation metadata, 107 descriptions/device, 108 changed metadata,
// 109 authentication, 110 pending native submission, 111 exception.
// 120 fixed-pin arguments,121 native code,122 source context,123 candidate,
// 124 bootstrap (details[5]:1 arguments,2 lock busy,3 handle,4 CAS,
// 5 owner shape,6 allocator retain,7 allocation retain,8 changed identity),
// 125 pinned allocation changed,126 physical/context,127 pinned getter,
// 128 immutable arguments,129 source/pins,130 resource/device,131 final proof,
// 134 fixed-pin exception. Initial-upload failures retain their existing codes.
uint32_t LastFailure();
}
