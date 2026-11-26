#include "Render/NativeOutputCopy.hpp"
#if !CVR_RUST_VRIK
// The C++ build's copy; the Rust build's is rust/crates/cvr-gpu/src/output_copy.rs (the header calls it).
#include <algorithm>
#include <climits>
#include <new>
#include <utility>

namespace cvr::stereo {
namespace {
using Microsoft::WRL::ComPtr;
constexpr std::array<UINT,4> Slots{0,1,2,4};
constexpr std::array<DXGI_FORMAT,4> Formats{DXGI_FORMAT_R10G10B10A2_UNORM,DXGI_FORMAT_R10G10B10A2_UNORM,
    DXGI_FORMAT_R8G8B8A8_UNORM,DXGI_FORMAT_R32G8X24_TYPELESS};
constexpr UINT64 MaxReadbackBytes=512ull*1024*1024;
bool SameDevice(ID3D12Device* device,ID3D12DeviceChild* child){
    ComPtr<ID3D12Device> actual;return child && SUCCEEDED(child->GetDevice(IID_PPV_ARGS(&actual))) && actual.Get()==device;
}
bool SameProperties(const D3D12_HEAP_PROPERTIES& a,const D3D12_HEAP_PROPERTIES& b){
    return a.Type==b.Type && a.CPUPageProperty==b.CPUPageProperty && a.MemoryPoolPreference==b.MemoryPoolPreference &&
        a.CreationNodeMask==b.CreationNodeMask && a.VisibleNodeMask==b.VisibleNodeMask;
}
bool ImageShape(ID3D12Device* device,ID3D12Resource* resource,UINT compactSlot,UINT layers,D3D12_RESOURCE_DESC& output){
    if(!SameDevice(device,resource))return false;const auto desc=resource->GetDesc();const bool depth=compactSlot==3;
    if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_TEXTURE2D || !desc.Width || desc.Width>UINT_MAX || !desc.Height ||
        desc.DepthOrArraySize!=layers || desc.MipLevels!=1 || desc.SampleDesc.Count!=1 || desc.SampleDesc.Quality ||
        desc.Layout!=D3D12_TEXTURE_LAYOUT_UNKNOWN || (desc.Flags&D3D12_RESOURCE_FLAG_ALLOW_SIMULTANEOUS_ACCESS) ||
        !(desc.Flags&(depth?D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL:D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET)) ||
        (desc.Format!=Formats[compactSlot] && !(depth && desc.Format==DXGI_FORMAT_D32_FLOAT_S8X24_UINT)))return false;
    D3D12_FEATURE_DATA_FORMAT_INFO format{desc.Format,0};
    if(FAILED(device->CheckFeatureSupport(D3D12_FEATURE_FORMAT_INFO,&format,sizeof(format))) || format.PlaneCount!=(depth?2u:1u))return false;
    output=desc;return true;
}
bool State(D3D12_RESOURCE_STATES state,bool depth,D3D12_RESOURCE_FLAGS flags){
    constexpr UINT shader=D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE|D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
    const UINT reads=shader|D3D12_RESOURCE_STATE_COPY_SOURCE|(depth?D3D12_RESOURCE_STATE_DEPTH_READ:0);
    const UINT writes=D3D12_RESOURCE_STATE_COPY_DEST|(depth?D3D12_RESOURCE_STATE_DEPTH_WRITE:D3D12_RESOURCE_STATE_RENDER_TARGET)|
        (!depth && (flags&D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS)?D3D12_RESOURCE_STATE_UNORDERED_ACCESS:0);
    const UINT value=UINT(state),write=value&writes;
    if(value&~(reads|writes) || ((flags&D3D12_RESOURCE_FLAG_DENY_SHADER_RESOURCE) && (value&shader)))return false;
    return !write || (!(write&(write-1)) && value==write);
}
bool Physical(ID3D12Device* device,const constants::ResourceCreationInfo& creation,const D3D12_RESOURCE_DESC& desc){
    if(creation.kind==1)return !creation.heap && !creation.heapOffset && !creation.allocationBytes;
    if(creation.kind!=2 || !SameDevice(device,creation.heap.Get()) || !creation.allocationBytes || creation.allocationBytes==UINT64_MAX)return false;
    const auto heap=creation.heap->GetDesc();const auto required=device->GetResourceAllocationInfo(0,1,&desc);
    D3D12_HEAP_PROPERTIES properties{};D3D12_HEAP_FLAGS flags{};
    return SUCCEEDED(creation.resource->GetHeapProperties(&properties,&flags)) && SameProperties(properties,heap.Properties) && flags==heap.Flags &&
        required.SizeInBytes && required.SizeInBytes!=UINT64_MAX && required.Alignment && creation.heapOffset%required.Alignment==0 &&
        creation.allocationBytes>=required.SizeInBytes && creation.heapOffset<=heap.SizeInBytes &&
        creation.allocationBytes<=heap.SizeInBytes-creation.heapOffset;
}
bool Overlap(const constants::ResourceCreationInfo& a,const constants::ResourceCreationInfo& b){
    if(a.resource==b.resource)return true;
    return a.kind==2 && b.kind==2 && a.heap==b.heap &&
        a.heapOffset<b.heapOffset+b.allocationBytes && b.heapOffset<a.heapOffset+a.allocationBytes;
}
void Barrier(std::array<D3D12_RESOURCE_BARRIER,10>& barriers,UINT& count,ID3D12Resource* resource,UINT plane,
             D3D12_RESOURCE_STATES before,D3D12_RESOURCE_STATES after){
    if(before==after)return;auto& value=barriers[count++];value.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    value.Transition={resource,plane,before,after};
}
}
HRESULT NativeOutputCopy::Create(ID3D12Device* device,const Arrays& arrays,const Targets& targets,
                                 ID3D12Resource* predicate,UINT64 offset)try{
    if(readback_)return E_UNEXPECTED;
    if(!device || device->GetNodeCount()!=1 || !SameDevice(device,predicate) || offset%8)return E_INVALIDARG;
    const auto predicateDesc=predicate->GetDesc();
    if(predicateDesc.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER || predicateDesc.Width<8 || offset>predicateDesc.Width-8)return E_INVALIDARG;
    std::array<D3D12_RESOURCE_DESC,4> arrayDescs{};
    std::array<std::array<D3D12_RESOURCE_DESC,4>,2> targetDescs{};
    for(UINT slot=0;slot<4;++slot){
        if(!ImageShape(device,arrays[slot].Get(),slot,2,arrayDescs[slot]))return E_INVALIDARG;
        if(slot && (arrayDescs[slot].Width!=arrayDescs[0].Width || arrayDescs[slot].Height!=arrayDescs[0].Height))return E_INVALIDARG;
        for(UINT prior=0;prior<slot;++prior)if(arrays[prior]==arrays[slot])return E_INVALIDARG;
    }
    for(UINT side=0;side<2;++side)for(UINT slot=0;slot<4;++slot){const auto& target=targets[side][slot];auto& desc=targetDescs[side][slot];
        if(target.planeCount!=(slot==3?2u:1u) || !ImageShape(device,target.creation.resource.Get(),slot,1,desc) ||
            desc.Width!=arrayDescs[slot].Width || desc.Height!=arrayDescs[slot].Height || !Physical(device,target.creation,desc))return E_INVALIDARG;
        for(UINT plane=0;plane<target.planeCount;++plane)if(!State(target.finalStates[plane],slot==3,desc.Flags))return E_INVALIDARG;
        for(const auto& source:arrays)if(source==target.creation.resource)return E_INVALIDARG;
        for(UINT prior=0;prior<side*4+slot;++prior)if(Overlap(target.creation,targets[prior/4][prior%4].creation))return E_INVALIDARG;
    }
    std::array<Plane,10> planes{};UINT index{};UINT64 total{};
    for(UINT side=0;side<2;++side)for(UINT slot=0;slot<4;++slot)for(UINT plane=0;plane<targets[side][slot].planeCount;++plane){
        const auto& desc=targetDescs[side][slot];auto& result=planes[index++];result.side=side;result.slot=Slots[slot];result.plane=plane;
        UINT64 rowBytes{},required{};
        device->GetCopyableFootprints(&desc,plane,1,total,&result.footprint,&result.rows,&rowBytes,&required);
        if(required==UINT64_MAX || !result.rows || result.rows!=desc.Height || !rowBytes || rowBytes%desc.Width ||
            result.footprint.Offset<total || result.footprint.Offset%512 || result.footprint.Footprint.Depth!=1 ||
            result.footprint.Footprint.Width!=desc.Width || result.footprint.Footprint.Height!=desc.Height ||
            result.footprint.Footprint.RowPitch<rowBytes || rowBytes/desc.Width>UINT_MAX)return E_INVALIDARG;
        result.stride=UINT(rowBytes/desc.Width);result.bytes=slot==3 && plane==1?1u:4u;
        result.component=slot==3 && plane==1 && result.stride==8?4u:0u;
        if((slot!=3 && result.stride!=4) || (slot==3 && result.stride!=4 && result.stride!=8 && !(plane==1 && result.stride==1)) ||
            result.component+result.bytes>result.stride)return E_INVALIDARG;
        const UINT64 payload=UINT64(result.rows-1)*result.footprint.Footprint.RowPitch+rowBytes;
        if(result.footprint.Offset>MaxReadbackBytes || payload>MaxReadbackBytes-result.footprint.Offset)return E_OUTOFMEMORY;
        total=(result.footprint.Offset+payload+511)&~UINT64(511);
    }
    if(index!=planes.size() || !total || total>MaxReadbackBytes)return E_INVALIDARG;
    D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;desc.Width=total;
    desc.Height=desc.DepthOrArraySize=desc.MipLevels=desc.SampleDesc.Count=1;desc.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_READBACK;ComPtr<ID3D12Resource> readback;
    const auto hr=device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&readback));
    if(FAILED(hr))return hr;
    device_=device;arrays_=arrays;targets_=targets;predicate_=predicate;predicateOffset_=offset;
    planes_=planes;bytes_=total;readback_=std::move(readback);return S_OK;
}catch(const std::bad_alloc&){return E_OUTOFMEMORY;}
bool NativeOutputCopy::Record(ID3D12GraphicsCommandList* list){
    if(!readback_ || recorded_ || !list || list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT || !SameDevice(device_.Get(),list))return false;
    std::array<D3D12_RESOURCE_BARRIER,10> barriers{};UINT count{};
    for(const auto& plane:planes_){const UINT slot=plane.slot==4?3:plane.slot;const auto& target=targets_[plane.side][slot];
        Barrier(barriers,count,target.creation.resource.Get(),plane.plane,target.finalStates[plane.plane],D3D12_RESOURCE_STATE_COPY_DEST);
    }
    list->SetPredication(nullptr,0,D3D12_PREDICATION_OP_EQUAL_ZERO);
    if(count)list->ResourceBarrier(count,barriers.data());
    list->SetPredication(predicate_.Get(),predicateOffset_,D3D12_PREDICATION_OP_EQUAL_ZERO);
    for(const auto& plane:planes_){const UINT slot=plane.slot==4?3:plane.slot;
        D3D12_TEXTURE_COPY_LOCATION from{},to{};from.pResource=arrays_[slot].Get();from.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
        from.SubresourceIndex=1-plane.side+plane.plane*2;
        to.pResource=targets_[plane.side][slot].creation.resource.Get();to.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;to.SubresourceIndex=plane.plane;
        list->CopyTextureRegion(&to,0,0,0,&from,nullptr);
    }
    list->SetPredication(nullptr,0,D3D12_PREDICATION_OP_EQUAL_ZERO);count=0;
    for(const auto& plane:planes_){const UINT slot=plane.slot==4?3:plane.slot;
        Barrier(barriers,count,targets_[plane.side][slot].creation.resource.Get(),plane.plane,D3D12_RESOURCE_STATE_COPY_DEST,D3D12_RESOURCE_STATE_COPY_SOURCE);
    }
    list->ResourceBarrier(count,barriers.data());
    for(const auto& plane:planes_){const UINT slot=plane.slot==4?3:plane.slot;
        D3D12_TEXTURE_COPY_LOCATION from{},to{};from.pResource=targets_[plane.side][slot].creation.resource.Get();
        from.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;from.SubresourceIndex=plane.plane;
        to.pResource=readback_.Get();to.Type=D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;to.PlacedFootprint=plane.footprint;
        list->CopyTextureRegion(&to,0,0,0,&from,nullptr);
    }
    count=0;
    for(const auto& plane:planes_){const UINT slot=plane.slot==4?3:plane.slot;const auto& target=targets_[plane.side][slot];
        Barrier(barriers,count,target.creation.resource.Get(),plane.plane,D3D12_RESOURCE_STATE_COPY_SOURCE,target.finalStates[plane.plane]);
    }
    if(count)list->ResourceBarrier(count,barriers.data());recorded_=true;return true;
}
}
#endif
