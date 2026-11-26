#if !CVR_RUST_VRIK
// The C++ build's arrays; the Rust build's are rust/crates/cvr-gpu/src/target_array.rs (the header calls them).
#include "Render/StereoTargetArray.hpp"
#include <array>
#include <utility>

namespace cvr::stereo {
namespace {
void LayerBarrier(D3D12_RESOURCE_BARRIER* barriers,UINT& count,ID3D12Resource* resource,UINT subresource,
    D3D12_RESOURCE_STATES before,D3D12_RESOURCE_STATES after){
    if(before==after)return;auto& barrier=barriers[count++];barrier.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Transition={resource,subresource,before,after};
}
}
HRESULT StereoTargetArray::ValidateLayer(ID3D12GraphicsCommandList* list,UINT eye,ID3D12Resource* native) const {
    if(!list || !resource_ || eye>1 || !planes_ || planes_>2 || native==resource_.Get() ||
        list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT)return E_INVALIDARG;
    Microsoft::WRL::ComPtr<ID3D12Device> device,listDevice,nativeDevice;
    auto hr=resource_->GetDevice(IID_PPV_ARGS(&device));if(FAILED(hr))return hr;
    hr=list->GetDevice(IID_PPV_ARGS(&listDevice));if(FAILED(hr))return hr;if(device!=listDevice)return E_INVALIDARG;
    const auto array=resource_->GetDesc();
    if(array.Dimension!=D3D12_RESOURCE_DIMENSION_TEXTURE2D || array.Width!=native_.Width || array.Height!=native_.Height ||
        array.DepthOrArraySize!=2 || array.MipLevels!=1 || array.SampleDesc.Count!=1 || array.SampleDesc.Quality ||
        array.Format!=native_.Format || array.Layout!=D3D12_TEXTURE_LAYOUT_UNKNOWN)return E_INVALIDARG;
    D3D12_FEATURE_DATA_FORMAT_INFO format{array.Format,0};
    hr=device->CheckFeatureSupport(D3D12_FEATURE_FORMAT_INFO,&format,sizeof(format));if(FAILED(hr))return hr;
    if(format.PlaneCount!=planes_)return E_INVALIDARG;
    if(!native)return S_OK;
    hr=native->GetDevice(IID_PPV_ARGS(&nativeDevice));if(FAILED(hr))return hr;if(device!=nativeDevice)return E_INVALIDARG;
    const auto desc=native->GetDesc();
    const bool depthFamily=native_.Format==DXGI_FORMAT_R32G8X24_TYPELESS && desc.Format==DXGI_FORMAT_D32_FLOAT_S8X24_UINT;
    if(desc.Dimension!=array.Dimension || desc.Width!=array.Width || desc.Height!=array.Height || desc.DepthOrArraySize!=1 ||
        desc.MipLevels!=1 || desc.SampleDesc.Count!=1 || desc.SampleDesc.Quality || desc.Layout!=array.Layout ||
        (desc.Format!=native_.Format && !depthFamily))return E_INVALIDARG;
    format={desc.Format,0};hr=device->CheckFeatureSupport(D3D12_FEATURE_FORMAT_INFO,&format,sizeof(format));if(FAILED(hr))return hr;
    return format.PlaneCount==planes_?S_OK:E_INVALIDARG;
}
HRESULT StereoTargetArray::TransitionEye(ID3D12GraphicsCommandList* list,UINT eye,D3D12_RESOURCE_STATES before,D3D12_RESOURCE_STATES after) const {
    const auto hr=ValidateLayer(list,eye,nullptr);if(FAILED(hr))return hr;
    std::array<D3D12_RESOURCE_BARRIER,2> barriers{};UINT count{};
    for(UINT plane=0;plane<planes_;++plane)LayerBarrier(barriers.data(),count,resource_.Get(),eye+plane*2,before,after);
    if(count)list->ResourceBarrier(count,barriers.data());return S_OK;
}
HRESULT StereoTargetArray::ImportEyeLayer(ID3D12GraphicsCommandList* list,UINT eye,D3D12_RESOURCE_STATES arrayState,
    ID3D12Resource* source,D3D12_RESOURCE_STATES sourceState) const {
    if(!source)return E_INVALIDARG;const auto hr=ValidateLayer(list,eye,source);if(FAILED(hr))return hr;
    std::array<D3D12_RESOURCE_BARRIER,3> barriers{};UINT count{};
    LayerBarrier(barriers.data(),count,source,D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,sourceState,D3D12_RESOURCE_STATE_COPY_SOURCE);
    for(UINT plane=0;plane<planes_;++plane)LayerBarrier(barriers.data(),count,resource_.Get(),eye+plane*2,arrayState,D3D12_RESOURCE_STATE_COPY_DEST);
    if(count)list->ResourceBarrier(count,barriers.data());
    for(UINT plane=0;plane<planes_;++plane){D3D12_TEXTURE_COPY_LOCATION from{},to{};
        from.pResource=source;from.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;from.SubresourceIndex=plane;
        to.pResource=resource_.Get();to.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;to.SubresourceIndex=eye+plane*2;
        list->CopyTextureRegion(&to,0,0,0,&from,nullptr);
    }
    for(UINT i=0;i<count;++i)std::swap(barriers[i].Transition.StateBefore,barriers[i].Transition.StateAfter);
    if(count)list->ResourceBarrier(count,barriers.data());return S_OK;
}
HRESULT StereoTargetArray::CopyEyeLayer(ID3D12GraphicsCommandList* list,UINT eye,D3D12_RESOURCE_STATES arrayState,
    ID3D12Resource* target,D3D12_RESOURCE_STATES before,D3D12_RESOURCE_STATES after) const {
    if(!planes_ || planes_>2)return E_INVALIDARG;
    const std::array<D3D12_RESOURCE_STATES,2> beforePlanes{before,before},afterPlanes{after,after};
    return CopyEyeLayer(list,eye,arrayState,target,std::span(beforePlanes).first(planes_),std::span(afterPlanes).first(planes_));
}
HRESULT StereoTargetArray::CopyEyeLayer(ID3D12GraphicsCommandList* list,UINT eye,D3D12_RESOURCE_STATES arrayState,
    ID3D12Resource* target,std::span<const D3D12_RESOURCE_STATES> before,std::span<const D3D12_RESOURCE_STATES> after) const {
    if(!target || before.size()!=planes_ || after.size()!=planes_)return E_INVALIDARG;
    const auto hr=ValidateLayer(list,eye,target);if(FAILED(hr))return hr;
    std::array<D3D12_RESOURCE_BARRIER,4> barriers{};UINT count{};
    for(UINT plane=0;plane<planes_;++plane)LayerBarrier(barriers.data(),count,resource_.Get(),eye+plane*2,arrayState,D3D12_RESOURCE_STATE_COPY_SOURCE);
    for(UINT plane=0;plane<planes_;++plane)LayerBarrier(barriers.data(),count,target,plane,before[plane],D3D12_RESOURCE_STATE_COPY_DEST);
    if(count)list->ResourceBarrier(count,barriers.data());
    for(UINT plane=0;plane<planes_;++plane){D3D12_TEXTURE_COPY_LOCATION from{},to{};
        from.pResource=resource_.Get();from.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;from.SubresourceIndex=eye+plane*2;
        to.pResource=target;to.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;to.SubresourceIndex=plane;
        list->CopyTextureRegion(&to,0,0,0,&from,nullptr);
    }
    count=0;
    for(UINT plane=0;plane<planes_;++plane)LayerBarrier(barriers.data(),count,resource_.Get(),eye+plane*2,D3D12_RESOURCE_STATE_COPY_SOURCE,arrayState);
    for(UINT plane=0;plane<planes_;++plane)LayerBarrier(barriers.data(),count,target,plane,D3D12_RESOURCE_STATE_COPY_DEST,after[plane]);
    if(count)list->ResourceBarrier(count,barriers.data());return S_OK;
}
HRESULT StereoTargetArray::ImportEye(ID3D12GraphicsCommandList* list,UINT eye,D3D12_RESOURCE_STATES arrayState,
    ID3D12Resource* source,D3D12_RESOURCE_STATES sourceState) const {
    return eye>1?E_INVALIDARG:ImportEyes(list,1u<<eye,arrayState,source,sourceState);
}
HRESULT StereoTargetArray::ImportEyes(ID3D12GraphicsCommandList* list,UINT eyeMask,D3D12_RESOURCE_STATES arrayState,
    ID3D12Resource* source,D3D12_RESOURCE_STATES sourceState) const {
    if(!list || !resource_ || !source || source==resource_.Get() || !eyeMask || eyeMask>3)return E_INVALIDARG;
    const auto d=source->GetDesc();const bool depthFamily=native_.Format==DXGI_FORMAT_R32G8X24_TYPELESS && d.Format==DXGI_FORMAT_D32_FLOAT_S8X24_UINT;
    if(d.Dimension!=native_.Dimension || d.Width!=native_.Width || d.Height!=native_.Height || d.DepthOrArraySize!=1 ||
       d.MipLevels!=1 || d.SampleDesc.Count!=1 || (d.Format!=native_.Format && !depthFamily))return E_INVALIDARG;
    D3D12_RESOURCE_BARRIER barriers[2]{};UINT count{};
    auto transition=[&](ID3D12Resource* r,D3D12_RESOURCE_STATES a,D3D12_RESOURCE_STATES b){if(a!=b){auto& t=barriers[count++];t.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;t.Transition={r,D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,a,b};}};
    transition(source,sourceState,D3D12_RESOURCE_STATE_COPY_SOURCE);transition(resource_.Get(),arrayState,D3D12_RESOURCE_STATE_COPY_DEST);
    if(count)list->ResourceBarrier(count,barriers);
    for(UINT eye=0;eye<2;++eye)if(eyeMask&(1u<<eye))for(UINT plane=0;plane<planes_;++plane){D3D12_TEXTURE_COPY_LOCATION from{},to{};
        from.pResource=source;from.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;from.SubresourceIndex=plane;
        to.pResource=resource_.Get();to.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;to.SubresourceIndex=eye+plane*2;list->CopyTextureRegion(&to,0,0,0,&from,nullptr);}
    count=0;transition(source,D3D12_RESOURCE_STATE_COPY_SOURCE,sourceState);transition(resource_.Get(),D3D12_RESOURCE_STATE_COPY_DEST,arrayState);
    if(count)list->ResourceBarrier(count,barriers);return S_OK;
}
HRESULT StereoTargetArray::Initialize(ID3D12Device* device,const D3D12_RESOURCE_DESC& desc,
    D3D12_RESOURCE_STATES state,const D3D12_CLEAR_VALUE* clear) {
    if(!device || resource_ || desc.Dimension!=D3D12_RESOURCE_DIMENSION_TEXTURE2D ||
       desc.DepthOrArraySize!=1 || desc.MipLevels!=1 || desc.SampleDesc.Count!=1 ||
       !desc.Width || !desc.Height || desc.Layout!=D3D12_TEXTURE_LAYOUT_UNKNOWN)return E_INVALIDARG;
    D3D12_FEATURE_DATA_FORMAT_INFO format{desc.Format,0};
    HRESULT hr=device->CheckFeatureSupport(D3D12_FEATURE_FORMAT_INFO,&format,sizeof(format));
    if(FAILED(hr))return hr;
    if(!format.PlaneCount || format.PlaneCount>2)return E_NOTIMPL;
    auto array=desc;array.DepthOrArraySize=2;array.Alignment=0;
    D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;
    Microsoft::WRL::ComPtr<ID3D12Resource> resource;
    hr=device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&array,state,clear,IID_PPV_ARGS(&resource));
    if(FAILED(hr))return hr;
    resource_=std::move(resource);native_=desc;planes_=format.PlaneCount;return S_OK;
}
HRESULT StereoTargetArray::CopyEye(ID3D12GraphicsCommandList* list,UINT eye,D3D12_RESOURCE_STATES sourceState,
    ID3D12Resource* target,D3D12_RESOURCE_STATES before,D3D12_RESOURCE_STATES after) const {
    if(!list || !resource_ || !target || target==resource_.Get() || eye>1)return E_INVALIDARG;
    const auto desc=target->GetDesc();
    const bool depthFamily=native_.Format==DXGI_FORMAT_R32G8X24_TYPELESS && desc.Format==DXGI_FORMAT_D32_FLOAT_S8X24_UINT;
    if(desc.Dimension!=native_.Dimension || desc.Width!=native_.Width || desc.Height!=native_.Height ||
       desc.DepthOrArraySize!=1 || desc.MipLevels!=1 || desc.SampleDesc.Count!=1 || (desc.Format!=native_.Format && !depthFamily))return E_INVALIDARG;
    std::array<D3D12_RESOURCE_BARRIER,2> barriers{};UINT count{};
    auto transition=[&](ID3D12Resource* r,D3D12_RESOURCE_STATES a,D3D12_RESOURCE_STATES b) {
        if(a==b)return;
        auto& barrier=barriers[count++];barrier.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        barrier.Transition={r,D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,a,b};
    };
    transition(resource_.Get(),sourceState,D3D12_RESOURCE_STATE_COPY_SOURCE);
    transition(target,before,D3D12_RESOURCE_STATE_COPY_DEST);
    if(count)list->ResourceBarrier(count,barriers.data());
    // Depth and stencil are separate planes. Both require an entire rectangle;
    // a depth-only copy would leave MAIN's stencil masks from another view.
    for(UINT plane=0;plane<planes_;++plane) {
        D3D12_TEXTURE_COPY_LOCATION source{},destination{};
        source.pResource=resource_.Get();source.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
        source.SubresourceIndex=eye+plane*2;
        destination.pResource=target;destination.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
        destination.SubresourceIndex=plane;
        list->CopyTextureRegion(&destination,0,0,0,&source,nullptr);
    }
    count=0;transition(resource_.Get(),D3D12_RESOURCE_STATE_COPY_SOURCE,sourceState);
    transition(target,D3D12_RESOURCE_STATE_COPY_DEST,after);
    if(count)list->ResourceBarrier(count,barriers.data());
    return S_OK;
}
}
#endif
