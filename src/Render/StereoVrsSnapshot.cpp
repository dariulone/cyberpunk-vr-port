#include "Render/StereoVrsSnapshot.hpp"
#if !CVR_RUST_VRIK
// The C++ build's snapshot; the Rust build's is rust/crates/cvr-gpu/src/vrs_snapshot.rs (the header calls it).
#include "Render/CommandResources.hpp"
#include <cstring>
namespace cvr::stereo {
HRESULT StereoVrsSnapshot::Initialize(ID3D12Device* device,ID3D12Resource* image) {
    if(!device || !image || frozen_)return E_INVALIDARG;desc_=image->GetDesc();
    if(desc_.Dimension!=D3D12_RESOURCE_DIMENSION_TEXTURE2D || desc_.Format!=DXGI_FORMAT_R8_UINT ||
       desc_.MipLevels!=1 || desc_.DepthOrArraySize!=1 || desc_.SampleDesc.Count!=1 || !desc_.Width || !desc_.Height || desc_.Width>512 || desc_.Height>512)return E_NOTIMPL;
    auto d=desc_;d.Alignment=0;d.Flags=D3D12_RESOURCE_FLAG_NONE;D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;
    auto hr=device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&d,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&frozen_));if(FAILED(hr))return hr;
    UINT64 size{};device->GetCopyableFootprints(&desc_,0,1,0,&footprints_[0],nullptr,nullptr,&size);
    footprints_[1]=footprints_[0];footprints_[1].Offset=(size+511)&~UINT64(511);bytes_=footprints_[1].Offset+size;
    d={};d.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;d.Width=bytes_;d.Height=d.DepthOrArraySize=d.MipLevels=d.SampleDesc.Count=1;d.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    heap.Type=D3D12_HEAP_TYPE_READBACK;return device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&d,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&readback_));
}
bool StereoVrsSnapshot::Retain(ID3D12GraphicsCommandList* list)const{return cvr::gpu::KeepCommandResources(list,{frozen_.Get(),readback_.Get()});}
HRESULT StereoVrsSnapshot::Capture(ID3D12GraphicsCommandList* list,ID3D12Resource* image,UINT eye) {
    if(!list || !image || !frozen_ || eye>1 || (capturedMask_&(1u<<eye)))return E_INVALIDARG;const auto d=image->GetDesc();
    if(d.Dimension!=desc_.Dimension || d.Format!=desc_.Format || d.Width!=desc_.Width || d.Height!=desc_.Height || d.MipLevels!=1 || d.DepthOrArraySize!=1 || d.SampleDesc.Count!=1)return E_INVALIDARG;
    Microsoft::WRL::ComPtr<ID3D12GraphicsCommandList5> list5;auto hr=list->QueryInterface(IID_PPV_ARGS(&list5));if(FAILED(hr))return hr;
    if(!Retain(list) || !cvr::gpu::KeepCommandResources(list,{image}))return E_OUTOFMEMORY;
    list5->RSSetShadingRateImage(nullptr);
    D3D12_RESOURCE_BARRIER b{};b.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    b.Transition={image,0,D3D12_RESOURCE_STATE_SHADING_RATE_SOURCE,D3D12_RESOURCE_STATE_COPY_SOURCE};list->ResourceBarrier(1,&b);
    D3D12_TEXTURE_COPY_LOCATION from{},to{};from.pResource=image;from.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    to.pResource=readback_.Get();to.Type=D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;to.PlacedFootprint=footprints_[eye];list->CopyTextureRegion(&to,0,0,0,&from,nullptr);
    if(!eye){to={};to.pResource=frozen_.Get();to.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;list->CopyTextureRegion(&to,0,0,0,&from,nullptr);}
    b.Transition.StateBefore=D3D12_RESOURCE_STATE_COPY_SOURCE;b.Transition.StateAfter=D3D12_RESOURCE_STATE_SHADING_RATE_SOURCE;list->ResourceBarrier(1,&b);
    if(!eye){b.Transition={frozen_.Get(),0,D3D12_RESOURCE_STATE_COPY_DEST,D3D12_RESOURCE_STATE_SHADING_RATE_SOURCE};list->ResourceBarrier(1,&b);}
    list5->RSSetShadingRateImage(image);capturedMask_|=1u<<eye;return S_OK;
}
HRESULT StereoVrsSnapshot::ReadEye(UINT eye,std::vector<uint8_t>& output)const {
    if(eye>1 || !(capturedMask_&(1u<<eye)) || !readback_ || !output.empty())return E_INVALIDARG;
    std::vector<uint8_t> pixels(size_t(desc_.Width)*desc_.Height);void* mapped{};const D3D12_RANGE range{0,SIZE_T(bytes_)};
    auto result=readback_->Map(0,&range,&mapped);if(FAILED(result))return result;if(!mapped)return E_FAIL;
    for(UINT y=0;y<desc_.Height;++y)std::memcpy(pixels.data()+size_t(y)*size_t(desc_.Width),
        static_cast<const uint8_t*>(mapped)+footprints_[eye].Offset+size_t(y)*footprints_[eye].Footprint.RowPitch,size_t(desc_.Width));
    const D3D12_RANGE none{};readback_->Unmap(0,&none);output=std::move(pixels);return S_OK;
}
HRESULT StereoVrsSnapshot::Read(uint64_t& differences,std::array<std::array<uint64_t,16>,2>& histogram)const {
    if(capturedMask_!=3 || !readback_)return E_PENDING;
    void* data{};D3D12_RANGE range{0,SIZE_T(bytes_)};auto hr=readback_->Map(0,&range,&data);if(FAILED(hr))return hr;if(!data)return E_FAIL;
    differences=0;histogram={};const auto* bytes=static_cast<const uint8_t*>(data);
    for(UINT y=0;y<desc_.Height;++y)for(UINT x=0;x<desc_.Width;++x){const auto a=bytes[footprints_[0].Offset+y*footprints_[0].Footprint.RowPitch+x];
        const auto b=bytes[footprints_[1].Offset+y*footprints_[1].Footprint.RowPitch+x];differences+=a!=b;if(a<16)++histogram[0][a];if(b<16)++histogram[1][b];}
    const D3D12_RANGE none{};readback_->Unmap(0,&none);return S_OK;
}
}
#endif
