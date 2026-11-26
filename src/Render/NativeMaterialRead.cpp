#include "Render/NativeMaterialRead.hpp"
#include "Render/NativeStereoCameraUpload.hpp"
#include "Render/CommandResources.hpp"
#include "Render/RenderProbeScope.hpp"
#include <windows.h>
#if CVR_RUST_VRIK
// The game-code contract: rust/crates/cvr-render/src/binding_code.rs (0 pair preparation, 1 material read, 2 material
// binding; through ReadProcessMemory).
#endif

#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
// The reads are rust/crates/cvr-render/src/material_read.rs (the lease shared by layout).
namespace cvr::stereo::material_read {
static_assert(sizeof(material_binding::Lease)==528 && offsetof(material_binding::Lease,stamp)==160 &&
    offsetof(material_binding::Lease,source)==336 && offsetof(material_binding::Lease,readProof)==432,"mirrored by material_read.rs");
extern "C" {
uint32_t cvr_mread_last_failure();
bool cvr_mread_copy_snapshot(ID3D12GraphicsCommandList*,const material_binding::Lease*,ID3D12Resource*,uint64_t,buffer_read::Proof*);
bool cvr_mread_validate_snapshot_submission(const material_binding::Lease*,const buffer_read::Proof*,ID3D12CommandQueue*);
}
uint32_t LastFailure(){return cvr_mread_last_failure();}
bool CopySnapshot(ID3D12GraphicsCommandList* list,const material_binding::Lease& lease,ID3D12Resource* destination,
                  uint64_t offset,buffer_read::Proof& proof){return cvr_mread_copy_snapshot(list,&lease,destination,offset,&proof);}
bool ValidateSnapshotSubmission(const material_binding::Lease& lease,const buffer_read::Proof& proof,ID3D12CommandQueue* queue){
    return cvr_mread_validate_snapshot_submission(&lease,&proof,queue);
}
}
#endif
#else
namespace cvr::stereo::material_read {
namespace {
thread_local uint32_t failure{};
bool Read(void*,uint64_t address,void* output,size_t bytes){
    SIZE_T copied{};return address && address<=UINT64_MAX-bytes &&
        ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<const void*>(address),output,bytes,&copied) && copied==bytes;
}
bool Current(uintptr_t game,ID3D12GraphicsCommandList* list,const material_binding::Lease& lease){
    const auto context=camera_upload::CurrentContext(game);
    if(!context || context!=lease.native.context || context.list!=reinterpret_cast<uintptr_t>(list))return false;
    material_binding::Snapshot actual;
    return material_binding::detail::ReadSnapshot(Read,nullptr,game,context,lease.native.handle,actual) && actual==lease.native;
}
bool Descriptor(const material_binding::Lease& lease){
    const auto& binding=lease.binding;const auto& stamp=lease.stamp;descriptors::CbvBinding cpu;
    return stamp.gpu==binding.descriptor && stamp.heap==binding.heap && stamp.value[0]==1 &&
        stamp.value[4]==binding.address && stamp.value[5]==binding.bytes &&
        material_binding::detail::FitsCbv(lease.native,binding.address,binding.bytes) &&
        descriptors::ResolveCpuCbvAddress(lease.native.slot.descriptor,cpu) && cpu.address==binding.address &&
        cpu.bytes==binding.bytes && descriptors::MatchesDescriptor(stamp);
}
}
uint32_t LastFailure(){return failure;}
bool Compatible(){
    static const bool compatible=[] {
        const auto game=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
#if CVR_RUST_VRIK
        return cvr_binding_code(1,game);
#else
        struct Code{uint32_t rva,bytes;uint64_t hash;};
        constexpr Code expected[]{{0x337DA0,0x180,0xB36B2D868965D718ull},
            {0x3E3BB4,0x34,0x1A0F0B6F177A422Cull},{0x337230,0x33,0x0396F3E29AEB7926ull}};
        std::array<uint8_t,512> bytes{};
        for(const auto& code:expected){
            if(!game || game>UINT64_MAX-code.rva || code.bytes>bytes.size() || !Read(nullptr,game+code.rva,bytes.data(),code.bytes))return false;
            uint64_t hash=14695981039346656037ull;for(UINT i=0;i<code.bytes;++i)hash=(hash^bytes[i])*1099511628211ull;
            if(hash!=code.hash)return false;
        }return true;
#endif
    }();return compatible;
}
bool Validate(ID3D12GraphicsCommandList* list,const material_binding::Lease& lease)try{
    const auto game=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));const auto& source=lease.source;
    if(!list || !lease.serial || !source.resource || !material_binding::Compatible(game) ||
        !detail::NativeIdentity(lease.native,source.native) ||
        reinterpret_cast<uintptr_t>(source.resource.Get())!=lease.native.record.resource ||
        !detail::SourceRange(lease.binding.address,source.gpu,source.bytes,lease.sourceOffset) ||
        !Current(game,list,lease) || !Descriptor(lease))return false;
    const auto desc=source.resource->GetDesc();D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS flags{};
    if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER || desc.Width!=source.bytes || UINT(desc.Flags)!=source.flags ||
        (desc.Flags&D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS) || source.resource->GetGPUVirtualAddress()!=source.gpu ||
        FAILED(source.resource->GetHeapProperties(&heap,&flags)) || UINT(heap.Type)!=source.heapType)return false;
    return Current(game,list,lease) && Descriptor(lease);
}catch(...){return false;}
bool ValidateSubmission(const material_binding::Lease& lease,ID3D12CommandQueue* queue)try{
    const auto game=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));const auto& source=lease.source;
    const bool implicit=detail::NeedsBufferReceipt(source.heapType,lease.native.record.kind);
    if(!lease.serial || !source.resource || !material_binding::Compatible(game) ||
        !detail::NativeIdentity(lease.native,source.native) ||
        (!implicit && (!detail::SubmissionHeap(source.heapType,lease.native.record.kind) || lease.readProof.receipt)) ||
        (implicit && (!queue || !buffer_read::Validate(source,lease.readProof,queue))) ||
        reinterpret_cast<uintptr_t>(source.resource.Get())!=lease.native.record.resource ||
        !detail::SourceRange(lease.binding.address,source.gpu,source.bytes,lease.sourceOffset) ||
        !detail::SubmissionRecord(Read,nullptr,game,lease.native) || !Descriptor(lease))return false;
    const auto desc=source.resource->GetDesc();D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS flags{};
    if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER || desc.Width!=source.bytes || UINT(desc.Flags)!=source.flags ||
        (desc.Flags&D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS) || source.resource->GetGPUVirtualAddress()!=source.gpu ||
        FAILED(source.resource->GetHeapProperties(&heap,&flags)) || UINT(heap.Type)!=source.heapType)return false;
    return detail::SubmissionRecord(Read,nullptr,game,lease.native) && Descriptor(lease) &&
        (!implicit || buffer_read::Validate(source,lease.readProof,queue));
}catch(...){return false;}
namespace {
bool SnapshotIdentity(const material_binding::Lease& lease){
    const auto game=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));const auto& source=lease.source;
    if(!source.resource || source.heapType!=D3D12_HEAP_TYPE_DEFAULT || source.flags || lease.native.record.kind!=1 ||
        !detail::NativeIdentity(lease.native,source.native) || reinterpret_cast<uintptr_t>(source.resource.Get())!=lease.native.record.resource ||
        !detail::SourceRange(lease.binding.address,source.gpu,source.bytes,lease.sourceOffset) ||
        !detail::SubmissionRecord(Read,nullptr,game,lease.native))return false;
    const auto desc=source.resource->GetDesc();D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS flags{};
    return desc.Dimension==D3D12_RESOURCE_DIMENSION_BUFFER && desc.Width==source.bytes && !desc.Flags &&
        source.resource->GetGPUVirtualAddress()==source.gpu && SUCCEEDED(source.resource->GetHeapProperties(&heap,&flags)) &&
        heap.Type==D3D12_HEAP_TYPE_DEFAULT;
}
}
bool CopySnapshot(ID3D12GraphicsCommandList* list,const material_binding::Lease& lease,ID3D12Resource* destination,
                  uint64_t offset,buffer_read::Proof& proof)try{
    proof={};failure=1;if(!list || !destination || destination==lease.source.resource.Get() ||
        list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT)return false;
    failure=2;if(!Compatible())return false;
    failure=3;if(!SnapshotIdentity(lease))return false;
    const auto desc=destination->GetDesc();D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS flags{};
    failure=4;if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER || desc.Flags!=D3D12_RESOURCE_FLAG_NONE ||
        !detail::DestinationRange(desc.Width,offset) || FAILED(destination->GetHeapProperties(&heap,&flags)) || heap.Type!=D3D12_HEAP_TYPE_DEFAULT)return false;
    Microsoft::WRL::ComPtr<ID3D12Device> a,b,c;failure=5;
    if(FAILED(list->GetDevice(IID_PPV_ARGS(&a))) || FAILED(destination->GetDevice(IID_PPV_ARGS(&b))) ||
        FAILED(lease.source.resource->GetDevice(IID_PPV_ARGS(&c))) || a!=b || a!=c)return false;
    const auto context=camera_upload::CurrentContext(reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr)));
    failure=7;if(!context || context.list!=reinterpret_cast<uintptr_t>(list))return false;
    auto source=lease.source;source.native.context=context.context;
    failure=6;if(!gpu::KeepCommandResources(list,{source.resource.Get(),destination}))return false;
    if(!buffer_read::Capture(list,source,proof)){failure=100+buffer_read::LastFailure();return false;}
    failure=42;if(!SnapshotIdentity(lease) || !buffer_read::Validate(source,proof))return false;
    probe::Scope internal;list->CopyBufferRegion(destination,offset,source.resource.Get(),lease.sourceOffset,material_binding::LogicalBytes);
    failure=0;return true;
}catch(...){proof={};failure=8;return false;}
bool ValidateSnapshotSubmission(const material_binding::Lease& lease,const buffer_read::Proof& proof,ID3D12CommandQueue* queue)try{
    return queue && SnapshotIdentity(lease) && buffer_read::Validate(lease.source,proof,queue);
}catch(...){return false;}
bool Copy(ID3D12GraphicsCommandList* list,material_binding::Lease& source,ID3D12Resource* destination,uint64_t offset){
    bool mutated=false;
    try{
        source.readProof={};
        failure=1;
        if(!list || !destination || !source.source.resource || destination==source.source.resource.Get() ||
            list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT)return false;
        failure=2;if(!Compatible())return false;
        failure=3;if(!Validate(list,source))return false;
        const auto desc=destination->GetDesc();D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS flags{};
        failure=4;
        if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER || desc.Flags!=D3D12_RESOURCE_FLAG_NONE ||
            !detail::DestinationRange(desc.Width,offset) || FAILED(destination->GetHeapProperties(&heap,&flags)) ||
            heap.Type!=D3D12_HEAP_TYPE_READBACK)return false;
        Microsoft::WRL::ComPtr<ID3D12Device> a,b,c;
        failure=5;
        if(FAILED(list->GetDevice(IID_PPV_ARGS(&a))) || FAILED(destination->GetDevice(IID_PPV_ARGS(&b))) ||
            FAILED(source.source.resource->GetDevice(IID_PPV_ARGS(&c))) || a!=b || a!=c)return false;
        const auto kind=source.native.record.kind;const auto sourceHeap=source.source.heapType;
        if(detail::NeedsBufferReceipt(sourceHeap,kind)){
            failure=6;if(!gpu::KeepCommandResources(list,{source.source.resource.Get(),destination}))return false;
            if(!buffer_read::Capture(list,source.source,source.readProof)){failure=100+buffer_read::LastFailure();return false;}
            failure=42;if(!Validate(list,source) || !buffer_read::Validate(source.source,source.readProof))return false;
            list->CopyBufferRegion(destination,offset,source.source.resource.Get(),source.sourceOffset,material_binding::LogicalBytes);
            failure=0;return true;
        }
        const auto before=geometry::ReadStateAtDraw(list,source.source);
        uint32_t required{};
        failure=10+before.status;if(!detail::CopyReadMask(before,sourceHeap,kind,required))return false;
        failure=6;if(!gpu::KeepCommandResources(list,{source.source.resource.Get(),destination}))return false;
        const auto game=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
        const auto context=source.native.context;
        using Register=void(*)(void*,uint32_t,uint32_t);using Flush=void(*)(void*);
        const auto registerRead=reinterpret_cast<Register>(game+0x337DA0);
        const auto flush=reinterpret_cast<Flush>(game+0x3E3BB4);
        auto* nativeContext=reinterpret_cast<void*>(context.context);
        probe::Scope internal;
        auto recover=[&]{
            failure=40;if(!mutated || !Validate(list,source))return false;
            auto state=geometry::ReadStateAtDraw(list,source.source);
            if(state.status==4){
                flush(nativeContext);state=geometry::ReadStateAtDraw(list,source.source);
                if(!Validate(list,source))return false;
            }
            uint32_t recovery{};
            if(!detail::RecoveryReadMask(state,kind,before.value,recovery))return false;
            if((state.value&recovery)!=recovery){
                registerRead(nativeContext,source.native.handle,recovery);flush(nativeContext);
                state=geometry::ReadStateAtDraw(list,source.source);
            }
            if(Validate(list,source) && detail::HasReadState(state,sourceHeap,kind,recovery))failure=41;
            return false;
        };
        failure=7;if(!Validate(list,source))return false;
        const auto current=geometry::ReadStateAtDraw(list,source.source);
        uint32_t currentRequired{};
        if(!detail::CopyReadMask(current,sourceHeap,kind,currentRequired) || current.value!=before.value)return false;
        if(!detail::HasReadState(current,sourceHeap,kind,required)){
            mutated=true;failure=40;
            registerRead(nativeContext,source.native.handle,required);flush(nativeContext);
        }
        const auto ready=geometry::ReadStateAtDraw(list,source.source);
        if(!Validate(list,source) || !detail::HasReadState(ready,sourceHeap,kind,required)){
            if(mutated)return recover();failure=7;return false;
        }
        list->CopyBufferRegion(destination,offset,source.source.resource.Get(),source.sourceOffset,material_binding::LogicalBytes);
        failure=0;return true;
    }catch(...){failure=mutated?40u:8u;return false;}
}
}
#endif
