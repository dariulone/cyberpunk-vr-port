#if !CVR_RUST_VRIK
// The C++ build's atlas copy; the Rust build's is in rust/crates/cvr-gpu/src/mono_cascades.rs.
#include "Render/MonoCascadeImage.hpp"
#include "Render/CommandResources.hpp"
#include "Render/RenderProbeScope.hpp"
#include "Render/SceneGpuProfile.hpp"

namespace cvr::cascades {
namespace {
struct ProfileRange {
    ID3D12GraphicsCommandList* list;
    bool active;
    ProfileRange(ID3D12GraphicsCommandList* command,int side,uint32_t detail):list(command),active(gpu::scene_profile::active.load(std::memory_order_relaxed)) {
        if(active)gpu::scene_profile::Mark(list,0x4EDFE0,side,nullptr,detail);
    }
    ~ProfileRange(){if(active)gpu::scene_profile::EndRange(list);}
};
void Barrier(ID3D12GraphicsCommandList* list,ID3D12Resource* resource,UINT sub,D3D12_RESOURCE_STATES before,D3D12_RESOURCE_STATES after) {
    if(before==after)return;
    D3D12_RESOURCE_BARRIER b{};b.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;b.Transition={resource,sub,before,after};list->ResourceBarrier(1,&b);
}
}
bool AtlasImage::Initialize(ID3D12Device* device,ID3D12Resource* source,DXGI_FORMAT format) {
    if(!device || !source || image)return false;shape=source->GetDesc();
    if(shape.Dimension!=D3D12_RESOURCE_DIMENSION_TEXTURE2D || shape.MipLevels!=1 || shape.DepthOrArraySize>8 ||
       !shape.Width || !shape.Height || shape.SampleDesc.Count!=1 || (format!=DXGI_FORMAT_D16_UNORM && format!=DXGI_FORMAT_D32_FLOAT))return false;
    auto d=shape;d.Alignment=0;d.Flags=D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL;
    const auto size=device->GetResourceAllocationInfo(0,1,&d).SizeInBytes;if(!size || size>512ull*1024*1024)return false;
    stereo::probe::Scope internal;D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;
    if(FAILED(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&d,D3D12_RESOURCE_STATE_COPY_SOURCE,nullptr,IID_PPV_ARGS(&image))))return false;
    image->SetName(L"CVR_mono_sun_atlas_committed");
    D3D12_DESCRIPTOR_HEAP_DESC h{};h.Type=D3D12_DESCRIPTOR_HEAP_TYPE_DSV;h.NumDescriptors=1;
    if(FAILED(device->CreateDescriptorHeap(&h,IID_PPV_ARGS(&descriptors))))return false;
    D3D12_DEPTH_STENCIL_VIEW_DESC view{};view.Format=format;view.ViewDimension=D3D12_DSV_DIMENSION_TEXTURE2DARRAY;view.Texture2DArray.ArraySize=d.DepthOrArraySize;
    device->CreateDepthStencilView(image.Get(),&view,descriptors->GetCPUDescriptorHandleForHeapStart());return true;
}
bool AtlasImage::Compatible(ID3D12Resource* source)const {
    if(!image || !descriptors || !source)return false;const auto d=source->GetDesc();
    return d.Dimension==shape.Dimension && d.Width==shape.Width && d.Height==shape.Height && d.Format==shape.Format &&
        d.MipLevels==1 && d.DepthOrArraySize==shape.DepthOrArraySize && d.SampleDesc.Count==1;
}
bool AtlasImage::Clear(ID3D12GraphicsCommandList* list,float depth) {
    if(!image || !descriptors || !gpu::KeepCommandResources(list,{image.Get(),descriptors.Get()}))return false;
    stereo::probe::Scope internal;
    ProfileRange timing(list,1,201);
    Barrier(list,image.Get(),D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,D3D12_RESOURCE_STATE_COPY_SOURCE,D3D12_RESOURCE_STATE_DEPTH_WRITE);
    list->ClearDepthStencilView(descriptors->GetCPUDescriptorHandleForHeapStart(),D3D12_CLEAR_FLAG_DEPTH,depth,0,0,nullptr);
    Barrier(list,image.Get(),D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,D3D12_RESOURCE_STATE_DEPTH_WRITE,D3D12_RESOURCE_STATE_COPY_SOURCE);return true;
}
bool AtlasImage::Capture(ID3D12GraphicsCommandList* list,ID3D12Resource* source,UINT slice,D3D12_RESOURCE_STATES before) {
    if(!Compatible(source) || slice>=shape.DepthOrArraySize || !gpu::KeepCommandResources(list,{image.Get(),source}))return false;
    stereo::probe::Scope internal;
    ProfileRange timing(list,1,101+slice);
    Barrier(list,source,slice,before,D3D12_RESOURCE_STATE_COPY_SOURCE);
    Barrier(list,image.Get(),slice,D3D12_RESOURCE_STATE_COPY_SOURCE,D3D12_RESOURCE_STATE_COPY_DEST);
    D3D12_TEXTURE_COPY_LOCATION from{},to{};from.pResource=source;from.SubresourceIndex=slice;to.pResource=image.Get();to.SubresourceIndex=slice;
    list->CopyTextureRegion(&to,0,0,0,&from,nullptr);
    Barrier(list,image.Get(),slice,D3D12_RESOURCE_STATE_COPY_DEST,D3D12_RESOURCE_STATE_COPY_SOURCE);
    Barrier(list,source,slice,D3D12_RESOURCE_STATE_COPY_SOURCE,before);return true;
}
bool AtlasImage::Restore(ID3D12GraphicsCommandList* list,ID3D12Resource* destination,UINT first,UINT count) {
    if(!Compatible(destination) || !count || first+count>shape.DepthOrArraySize || !gpu::KeepCommandResources(list,{image.Get(),destination}))return false;
    stereo::probe::Scope internal;
    ProfileRange timing(list,0,200);
    for(UINT i=first;i<first+count;++i) {
        Barrier(list,destination,i,D3D12_RESOURCE_STATE_DEPTH_WRITE,D3D12_RESOURCE_STATE_COPY_DEST);
        D3D12_TEXTURE_COPY_LOCATION from{},to{};from.pResource=image.Get();from.SubresourceIndex=i;to.pResource=destination;to.SubresourceIndex=i;
        list->CopyTextureRegion(&to,0,0,0,&from,nullptr);
        Barrier(list,destination,i,D3D12_RESOURCE_STATE_COPY_DEST,D3D12_RESOURCE_STATE_DEPTH_WRITE);
    }return true;
}
}
#endif
