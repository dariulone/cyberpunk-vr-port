#include "Render/GpuFinerVrs.hpp"
#if !CVR_RUST_VRIK
// The C++ build's reduction; the Rust build's is rust/crates/cvr-gpu/src/finer_vrs.rs (the header calls it).
#include "Render/CommandResources.hpp"
#include "Render/RenderProbeScope.hpp"
#include <d3dcompiler.h>
#include <atomic>
#include <algorithm>
#include <cstring>
#include <set>

namespace cvr::stereo {
using Microsoft::WRL::ComPtr;
namespace {
constexpr UINT MaxSources=512;
constexpr char Shader[]=R"(
Texture2D<uint> inputs[512] : register(t0);
RWTexture2D<uint> outputRate : register(u0);
RWByteAddressBuffer status : register(u1);
cbuffer Parameters : register(b0) { uint width,height,count,padding; };
[numthreads(1,1,1)] void Initialize() { status.Store2(0,uint2(0,0)); }
[numthreads(8,8,1)] void Reduce(uint3 id : SV_DispatchThreadID) {
    if(id.x>=width || id.y>=height)return;
    uint2 finest=uint2(2,2);
    for(uint i=0;i<count;++i) {
        uint rate=inputs[i].Load(int3(id.xy,0));
        bool valid=rate==0 || rate==1 || rate==4 || rate==5 || rate==6 || rate==9 || rate==10;
        if(!valid) { uint ignored;status.InterlockedOr(0,1,ignored);rate=0; }
        finest=min(finest,uint2(rate>>2,rate&3));
    }
    outputRate[id.xy]=(finest.x<<2)|finest.y;
}
)";
bool SameDevice(ID3D12Device* device,ID3D12DeviceChild* child){ComPtr<ID3D12Device> actual;
    return child && SUCCEEDED(child->GetDevice(IID_PPV_ARGS(&actual))) && actual.Get()==device;}
D3D12_RESOURCE_BARRIER Transition(ID3D12Resource* resource,D3D12_RESOURCE_STATES before,D3D12_RESOURCE_STATES after){
    D3D12_RESOURCE_BARRIER result{};result.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    result.Transition={resource,0,before,after};return result;
}
}
struct GpuFinerVrs::Storage final:IUnknown {
    std::atomic<ULONG> refs{1};ComPtr<ID3D12Device> device;ComPtr<ID3D12RootSignature> root;
    ComPtr<ID3D12PipelineState> initialize,reduce;ComPtr<ID3D12DescriptorHeap> heap;
    ComPtr<ID3D12Resource> image,status,readback;std::vector<ComPtr<ID3D12Resource>> sources;
    D3D12_PLACED_SUBRESOURCE_FOOTPRINT footprint{};UINT width{},height{},stride{};UINT64 statusOffset{},bytes{};bool recorded{};
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** out)override{if(!out)return E_POINTER;*out=nullptr;
        if(id!=__uuidof(IUnknown))return E_NOINTERFACE;*out=this;AddRef();return S_OK;}
    ULONG STDMETHODCALLTYPE AddRef()override{return ++refs;}
    ULONG STDMETHODCALLTYPE Release()override{const auto count=--refs;if(!count)delete this;return count;}
};
GpuFinerVrs::GpuFinerVrs()=default;GpuFinerVrs::~GpuFinerVrs()=default;
HRESULT GpuFinerVrs::Create(ID3D12Device* device,std::span<ID3D12Resource* const> images)try{
    if(storage_ || !device || device->GetNodeCount()!=1 || images.empty() || images.size()>MaxSources)return E_INVALIDARG;
    ComPtr<Storage> next;next.Attach(new Storage);next->device=device;std::set<ID3D12Resource*> seen;
    for(auto* image:images){if(!SameDevice(device,image))return E_INVALIDARG;const auto desc=image->GetDesc();
        if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_TEXTURE2D || desc.Format!=DXGI_FORMAT_R8_UINT || desc.MipLevels!=1 ||
            desc.DepthOrArraySize!=1 || desc.SampleDesc.Count!=1 || desc.SampleDesc.Quality || !desc.Width || !desc.Height ||
            desc.Width>512 || desc.Height>512 || (desc.Flags&D3D12_RESOURCE_FLAG_DENY_SHADER_RESOURCE))return E_INVALIDARG;
        if(next->width && (desc.Width!=next->width || desc.Height!=next->height))return E_INVALIDARG;
        next->width=UINT(desc.Width);next->height=desc.Height;if(seen.insert(image).second)next->sources.emplace_back(image);
    }
    D3D12_FEATURE_DATA_FORMAT_SUPPORT support{DXGI_FORMAT_R8_UINT};
    if(FAILED(device->CheckFeatureSupport(D3D12_FEATURE_FORMAT_SUPPORT,&support,sizeof(support))) ||
        !(support.Support1&D3D12_FORMAT_SUPPORT1_SHADER_LOAD) || !(support.Support2&D3D12_FORMAT_SUPPORT2_UAV_TYPED_STORE))return E_NOTIMPL;
    probe::Scope internal;
    D3D12_DESCRIPTOR_RANGE ranges[]{{D3D12_DESCRIPTOR_RANGE_TYPE_SRV,MaxSources,0,0,0},{D3D12_DESCRIPTOR_RANGE_TYPE_UAV,1,0,0,0}};
    D3D12_ROOT_PARAMETER parameters[4]{};
    for(UINT i=0;i<2;++i){parameters[i].ParameterType=D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;parameters[i].DescriptorTable={1,ranges+i};}
    parameters[2].ParameterType=D3D12_ROOT_PARAMETER_TYPE_UAV;parameters[2].Descriptor.ShaderRegister=1;
    parameters[3].ParameterType=D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS;parameters[3].Constants.Num32BitValues=4;
    D3D12_ROOT_SIGNATURE_DESC root{4,parameters,0,nullptr,D3D12_ROOT_SIGNATURE_FLAG_NONE};ComPtr<ID3DBlob> serialized,error;
    auto hr=D3D12SerializeRootSignature(&root,D3D_ROOT_SIGNATURE_VERSION_1,&serialized,&error);if(FAILED(hr))return hr;
    hr=device->CreateRootSignature(0,serialized->GetBufferPointer(),serialized->GetBufferSize(),IID_PPV_ARGS(&next->root));if(FAILED(hr))return hr;
    for(UINT i=0;i<2;++i){ComPtr<ID3DBlob> shader;
        hr=D3DCompile(Shader,sizeof(Shader)-1,nullptr,nullptr,nullptr,i?"Reduce":"Initialize","cs_5_1",D3DCOMPILE_OPTIMIZATION_LEVEL3,0,&shader,&error);
        if(FAILED(hr))return hr;D3D12_COMPUTE_PIPELINE_STATE_DESC pipeline{};pipeline.pRootSignature=next->root.Get();
        pipeline.CS={shader->GetBufferPointer(),shader->GetBufferSize()};
        hr=device->CreateComputePipelineState(&pipeline,IID_PPV_ARGS(i?&next->reduce:&next->initialize));if(FAILED(hr))return hr;
    }
    D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D;desc.Format=DXGI_FORMAT_R8_UINT;
    desc.Width=next->width;desc.Height=next->height;desc.DepthOrArraySize=desc.MipLevels=desc.SampleDesc.Count=1;desc.Flags=D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;
    D3D12_HEAP_PROPERTIES memory{};memory.Type=D3D12_HEAP_TYPE_DEFAULT;
    hr=device->CreateCommittedResource(&memory,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_UNORDERED_ACCESS,nullptr,IID_PPV_ARGS(&next->image));if(FAILED(hr))return hr;
    UINT64 total{};device->GetCopyableFootprints(&desc,0,1,0,&next->footprint,nullptr,nullptr,&total);
    if(total==UINT64_MAX || total>1024*1024)return E_INVALIDARG;next->statusOffset=(total+7)&~UINT64(7);next->bytes=next->statusOffset+8;
    desc={};desc.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;desc.Width=8;
    desc.Height=desc.DepthOrArraySize=desc.MipLevels=desc.SampleDesc.Count=1;desc.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;desc.Flags=D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;
    hr=device->CreateCommittedResource(&memory,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_UNORDERED_ACCESS,nullptr,IID_PPV_ARGS(&next->status));if(FAILED(hr))return hr;
    desc.Width=next->bytes;desc.Flags=D3D12_RESOURCE_FLAG_NONE;memory.Type=D3D12_HEAP_TYPE_READBACK;
    hr=device->CreateCommittedResource(&memory,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&next->readback));if(FAILED(hr))return hr;
    const D3D12_DESCRIPTOR_HEAP_DESC heap{D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,MaxSources+1,D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,0};
    hr=device->CreateDescriptorHeap(&heap,IID_PPV_ARGS(&next->heap));if(FAILED(hr))return hr;next->stride=device->GetDescriptorHandleIncrementSize(heap.Type);
    const auto start=next->heap->GetCPUDescriptorHandleForHeapStart();D3D12_SHADER_RESOURCE_VIEW_DESC srv{};
    srv.Format=DXGI_FORMAT_R8_UINT;srv.ViewDimension=D3D12_SRV_DIMENSION_TEXTURE2D;srv.Shader4ComponentMapping=D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;srv.Texture2D.MipLevels=1;
    for(UINT i=0;i<MaxSources;++i)device->CreateShaderResourceView(i<next->sources.size()?next->sources[i].Get():nullptr,&srv,{start.ptr+SIZE_T(i)*next->stride});
    D3D12_UNORDERED_ACCESS_VIEW_DESC uav{};uav.Format=DXGI_FORMAT_R8_UINT;uav.ViewDimension=D3D12_UAV_DIMENSION_TEXTURE2D;
    device->CreateUnorderedAccessView(next->image.Get(),nullptr,&uav,{start.ptr+SIZE_T(MaxSources)*next->stride});storage_=std::move(next);return S_OK;
}catch(...){return E_OUTOFMEMORY;}
bool GpuFinerVrs::Record(ID3D12GraphicsCommandList* list){
    if(!storage_ || storage_->recorded || !list || list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT || !SameDevice(storage_->device.Get(),list) ||
        !gpu::KeepCommandResources(list,{storage_.Get()}))return false;
    auto& s=*storage_.Get();std::vector<D3D12_RESOURCE_BARRIER> barriers;barriers.reserve(s.sources.size()+2);
    for(const auto& input:s.sources)barriers.push_back(Transition(input.Get(),D3D12_RESOURCE_STATE_SHADING_RATE_SOURCE,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE));
    probe::Scope internal;s.recorded=true;list->SetPredication(nullptr,0,D3D12_PREDICATION_OP_EQUAL_ZERO);
    list->ResourceBarrier(UINT(barriers.size()),barriers.data());ID3D12DescriptorHeap* heap=s.heap.Get();list->SetDescriptorHeaps(1,&heap);
    list->SetComputeRootSignature(s.root.Get());const auto start=s.heap->GetGPUDescriptorHandleForHeapStart();
    list->SetComputeRootDescriptorTable(0,start);list->SetComputeRootDescriptorTable(1,{start.ptr+UINT64(MaxSources)*s.stride});
    list->SetComputeRootUnorderedAccessView(2,s.status->GetGPUVirtualAddress());const UINT values[]{s.width,s.height,UINT(s.sources.size()),0};
    list->SetComputeRoot32BitConstants(3,4,values,0);list->SetPipelineState(s.initialize.Get());list->Dispatch(1,1,1);
    D3D12_RESOURCE_BARRIER status{};status.Type=D3D12_RESOURCE_BARRIER_TYPE_UAV;status.UAV.pResource=s.status.Get();list->ResourceBarrier(1,&status);
    list->SetPipelineState(s.reduce.Get());list->Dispatch((s.width+7)/8,(s.height+7)/8,1);
    for(auto& barrier:barriers)std::swap(barrier.Transition.StateBefore,barrier.Transition.StateAfter);
    barriers.push_back(Transition(s.image.Get(),D3D12_RESOURCE_STATE_UNORDERED_ACCESS,D3D12_RESOURCE_STATE_COPY_SOURCE));
    barriers.push_back(Transition(s.status.Get(),D3D12_RESOURCE_STATE_UNORDERED_ACCESS,D3D12_RESOURCE_STATE_COPY_SOURCE));list->ResourceBarrier(UINT(barriers.size()),barriers.data());
    D3D12_TEXTURE_COPY_LOCATION from{},to{};from.pResource=s.image.Get();from.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    to.pResource=s.readback.Get();to.Type=D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;to.PlacedFootprint=s.footprint;list->CopyTextureRegion(&to,0,0,0,&from,nullptr);
    list->CopyBufferRegion(s.readback.Get(),s.statusOffset,s.status.Get(),0,8);
    const auto ready=Transition(s.image.Get(),D3D12_RESOURCE_STATE_COPY_SOURCE,D3D12_RESOURCE_STATE_SHADING_RATE_SOURCE);list->ResourceBarrier(1,&ready);return true;
}
ID3D12Resource* GpuFinerVrs::Image()const{return storage_?storage_->image.Get():nullptr;}
UINT GpuFinerVrs::SourceCount()const{return storage_?UINT(storage_->sources.size()):0;}
UINT GpuFinerVrs::DispatchCount()const{return storage_ && storage_->recorded?2:0;}
UINT64 GpuFinerVrs::ReadbackBytes()const{return storage_?storage_->bytes:0;}
HRESULT GpuFinerVrs::Read(std::vector<uint8_t>& pixels,UINT& invalidRates)const{
    invalidRates=0;if(!storage_ || !storage_->recorded || !pixels.empty())return E_PENDING;const auto& s=*storage_.Get();
    std::vector<uint8_t> output(size_t(s.width)*s.height);void* mapped{};const D3D12_RANGE range{0,SIZE_T(s.bytes)};
    const auto hr=s.readback->Map(0,&range,&mapped);if(FAILED(hr) || !mapped)return FAILED(hr)?hr:E_FAIL;
    for(UINT y=0;y<s.height;++y)std::memcpy(output.data()+size_t(y)*s.width,static_cast<const uint8_t*>(mapped)+s.footprint.Offset+size_t(y)*s.footprint.Footprint.RowPitch,s.width);
    UINT64 errors{};std::memcpy(&errors,static_cast<const uint8_t*>(mapped)+s.statusOffset,8);const D3D12_RANGE none{};s.readback->Unmap(0,&none);
    if(errors>1)return E_FAIL;invalidRates=UINT(errors);pixels=std::move(output);return S_OK;
}
}
#endif
