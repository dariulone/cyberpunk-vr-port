#include "Render/NativeSourceCbvDetails.hpp"
#include <windows.h>
#if CVR_RUST_VRIK
// CaptureAtSource: rust/crates/cvr-render/src/source_cbv_snapshot_receipt.rs (the two reads at the source and the
// metadata between them).
namespace cvr::stereo::source_cbv_details {
static_assert(sizeof(Resource)==56 && sizeof(Detail)==760 && offsetof(Detail,record)==112 && offsetof(Detail,state)==304 &&
    offsetof(Detail,alias)==328 && offsetof(Detail,allocator)==696 && sizeof(source_cbv::SourceCbvBacking)==472,
    "mirrored by source_cbv_snapshot_receipt.rs");
}  // namespace cvr::stereo::source_cbv_details
#else

namespace cvr::stereo::source_cbv_details {
namespace {
bool Read(void*,uint64_t address,void* output,size_t bytes){SIZE_T copied{};
    return address && address<=UINT64_MAX-bytes && ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<const void*>(address),output,bytes,&copied) && copied==bytes;
}
bool Current(ID3D12GraphicsCommandList* list,uint64_t root,UINT parameter,UINT offset,
    const source_cbv::SourceCbvBacking& saved,source_cbv::SourceCbvBacking& fresh){
    const auto status=source_cbv::ReadAtSource(list,root,parameter,offset,saved.stamp,saved.originOwner.Get(),fresh);
    return (status==source_cbv::Status::Registered || status==source_cbv::Status::Created) && detail::SameSource(saved,fresh);
}
void ObserveAllocation(uint64_t base,const source_cbv::SourceCbvBacking& source,Detail& out){
    if(!source.native.handle || source.native.kind==4){out.allocationPath=AllocationPath::Arena;
        out.alias.observation=out.pool.observation=out.allocation.observation=out.allocator.observation=Observation::NotApplicable;return;}
    if(out.record.observation!=Observation::Known)return;
    const auto& raw=out.record.bytes;const auto direct=detail::Field<uint64_t>(raw,0x18);
    const auto payload=detail::Field<uint64_t>(raw,0x20),control=detail::Field<uint64_t>(raw,0x28);
    out.pool=detail::ReadPool(Read,nullptr,base,source.native,out.record,source.gpu,source.backingBytes);
    uint64_t owner{};
    if(out.pool.observation==Observation::Known){out.allocationPath=AllocationPath::Pool;owner=out.pool.owner;}
    else if(direct && !payload && !control){out.allocationPath=AllocationPath::Direct;owner=direct;}
    else if(!direct){out.alias=detail::ReadAlias(Read,nullptr,payload,control);
        if(out.alias.observation==Observation::Known){out.allocationPath=AllocationPath::Alias;owner=out.alias.allocation;}}
    else out.allocation.observation=Observation::Mismatch;
    if(owner){out.allocation=detail::ReadAllocation(Read,nullptr,base,owner);
        if(out.allocation.observation==Observation::Known){auto& allocation=out.allocation;
            allocation.extentCovers=allocation.bytes>=source.backingBytes;
            if(out.allocationPath==AllocationPath::Alias){allocation.aliasShapeKnown=true;
                allocation.aliasShapeMatches=!allocation.resource && allocation.heap && (allocation.kind==1 || allocation.kind==2) && allocation.bytes==out.alias.bytes;
            }else{allocation.associationKnown=true;allocation.resourceMatches=allocation.resource==reinterpret_cast<uintptr_t>(source.resource.Get());}}}
    out.allocator=detail::ReadAllocator(Read,nullptr,base,source.native.registry,out.allocation);
}
}
bool CaptureAtSource(ID3D12GraphicsCommandList* list,uint64_t root,UINT parameter,UINT offset,
    const source_cbv::SourceCbvBacking& source,Detail& output)try{
    output={};if(!list || !root || !source.resource || !source.recording.token || !source.recording.context)return false;
    output.status=Status::SourceBefore;source_cbv::SourceCbvBacking before,after;
    if(!Current(list,root,parameter,offset,source,before))return false;
    auto& resource=output.resource;output.status=Status::Resource;
    const auto desc=source.resource->GetDesc();resource.width=desc.Width;resource.flags=desc.Flags;
    resource.gpu=source.resource->GetGPUVirtualAddress();resource.comOffset=source.sourceOffset;
    if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER || FAILED(source.resource->GetHeapProperties(&resource.heap,&resource.heapFlags))){resource.observation=Observation::Unreadable;return false;}
    if(resource.gpu!=source.gpu || resource.width!=source.backingBytes || !source_cbv::detail::Descriptor(resource.gpu,resource.width,
        source.stamp.value[4],source.descriptorBytes) || source.stamp.value[4]-resource.gpu!=resource.comOffset){resource.observation=Observation::Mismatch;return false;}
    resource.observation=Observation::Known;output.status=Status::Context;
    output.context=detail::ReadContext(Read,nullptr,source.recording.context,reinterpret_cast<uintptr_t>(list));
    if(output.context.observation!=Observation::Known)return false;
    output.status=Status::Record;output.record=detail::ReadRecord(Read,nullptr,source.native);
    if(source.native.handle && output.record.observation!=Observation::Known)return false;
    geometry::BufferLease lease;lease.resource=source.resource;lease.gpu=resource.gpu;lease.bytes=resource.width;
    lease.heapType=UINT(resource.heap.Type);lease.flags=UINT(resource.flags);lease.native=source.native;
    output.state=geometry::ReadStateAtDraw(list,lease);output.stateQueried=true;output.stateValueKnown=detail::StateValueKnown(output.state);
    ObserveAllocation(reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr)),source,output);
    output.status=Status::SourceAfter;if(!Current(list,root,parameter,offset,source,after))return false;
    output.status=Status::Changed;const auto context=detail::ReadContext(Read,nullptr,source.recording.context,reinterpret_cast<uintptr_t>(list));
    if(context!=output.context || !detail::SameSource(before,after))return false;
    if(source.native.handle){const auto again=detail::ReadRecord(Read,nullptr,source.native);
        if(again.observation!=Observation::Known || again.address!=output.record.address || again.bytes!=output.record.bytes)return false;}
    output.status=Status::Captured;return true;
}catch(...){output.status=Status::Exception;return false;}
}
#endif
