#include "Render/GpuGeometryCompare.hpp"
#if !CVR_RUST_VRIK
// The C++ build's comparison; the Rust build's is rust/crates/cvr-gpu/src/geometry_compare.rs (the header calls it).
#include "Render/CommandResources.hpp"
#include "Render/RenderProbeScope.hpp"
#include <d3dcompiler.h>
#include <atomic>
#include <algorithm>
#include <cstring>
#include <intrin.h>
#include <map>
#include <vector>

namespace cvr::stereo {
using Microsoft::WRL::ComPtr;
namespace {
constexpr char Shader[]=R"(
ByteAddressBuffer leftBytes : register(t0);
ByteAddressBuffer rightBytes : register(t1);
ByteAddressBuffer permission : register(t2);
ByteAddressBuffer ranges : register(t3);
RWByteAddressBuffer results : register(u0);
cbuffer Parameters : register(b0) { uint batchFirst, batchCount, batchWidth, pairCount; };
[numthreads(256,1,1)]
void Initialize(uint3 id : SV_DispatchThreadID) {
    if(id.x>=pairCount)return;
    uint2 allowed=permission.Load2(0);
    results.Store4(id.x*16,uint4(any(allowed!=0)?1:0,0,0,0));
}
uint Left(uint address,uint count) {
    uint shift=(address&3)*8;
    uint value=leftBytes.Load(address&~3u)>>shift;
    if((address&3)+count>4)value|=leftBytes.Load((address&~3u)+4)<<(32-shift);
    return value;
}
uint Right(uint address,uint count) {
    uint shift=(address&3)*8;
    uint value=rightBytes.Load(address&~3u)>>shift;
    if((address&3)+count>4)value|=rightBytes.Load((address&~3u)+4)<<(32-shift);
    return value;
}
[numthreads(256,1,1)]
void Compare(uint3 thread : SV_GroupThreadID,uint3 group : SV_GroupID) {
    uint index=group.x+batchWidth*group.y;
    if(index>=batchCount || !any(permission.Load2(0)!=0))return;
    uint4 range=ranges.Load4((batchFirst+index)*16);
    uint at=thread.x*4;
    if(at>=range.z)return;
    uint count=min(4u,range.z-at);
    uint mask=0xffffffffu>>(32-count*8);
    if(((Left(range.x+at,count)^Right(range.y+at,count))&mask)!=0) {
        uint ignored;
        results.InterlockedAnd(range.w*16,0,ignored);
        results.InterlockedOr(range.w*16+8,1,ignored);
    }
}
)";
bool SameDevice(ID3D12Device* device,ID3D12DeviceChild* value){
    ComPtr<ID3D12Device> actual;return value && SUCCEEDED(value->GetDevice(IID_PPV_ARGS(&actual))) && actual.Get()==device;
}
bool Range(ID3D12Device* device,ID3D12Resource* value,UINT64 offset,UINT64 bytes){
    if(!SameDevice(device,value) || !bytes || offset>UINT_MAX || bytes>UINT_MAX-offset)return false;
    const auto desc=value->GetDesc();
    return desc.Dimension==D3D12_RESOURCE_DIMENSION_BUFFER && !(desc.Flags&D3D12_RESOURCE_FLAG_DENY_SHADER_RESOURCE) &&
        value->GetGPUVirtualAddress() && ((offset+bytes+3)&~UINT64(3))<=desc.Width;
}
}
struct GpuGeometryCompare::Storage final: IUnknown {
    std::atomic<ULONG> refs{1};ComPtr<ID3D12Device> device;ComPtr<ID3D12RootSignature> root;
    ComPtr<ID3D12PipelineState> initialize,compare;ComPtr<ID3D12Resource> predicates,gate,metadata;
    struct Batch {std::array<ComPtr<ID3D12Resource>,2> resources;UINT first{},count{};};
    std::vector<Batch> batches;
    std::vector<GeometryCompareRange> ranges;UINT pairs{};UINT64 bytes{},gateOffset{};bool recorded{};
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** out)override{if(!out)return E_POINTER;*out=nullptr;if(id!=__uuidof(IUnknown))return E_NOINTERFACE;*out=this;AddRef();return S_OK;}
    ULONG STDMETHODCALLTYPE AddRef()override{return ++refs;}
    ULONG STDMETHODCALLTYPE Release()override{const auto n=--refs;if(!n)delete this;return n;}
};
GpuGeometryCompare::GpuGeometryCompare()=default;
GpuGeometryCompare::~GpuGeometryCompare()=default;
HRESULT GpuGeometryCompare::Create(ID3D12Device* device,UINT pairs,std::span<const GeometryCompareRange> ranges,ID3D12Resource* gate,UINT64 gateOffset)try{
    if(storage_ || !device || !pairs || pairs>512 || ranges.empty() || ranges.size()>8192 || gateOffset%8 || !Range(device,gate,gateOffset,8))return E_INVALIDARG;
    ComPtr<Storage> next;next.Attach(new Storage);next->device=device;next->pairs=pairs;next->gate=gate;next->gateOffset=gateOffset;
    std::vector<bool> covered(pairs);
    for(const auto& range:ranges){
        if(range.pair>=pairs || range.bytes>32*1024*1024 || next->bytes>512ull*1024*1024-range.bytes ||
            !Range(device,range.resources[0].Get(),range.offsets[0],range.bytes) || !Range(device,range.resources[1].Get(),range.offsets[1],range.bytes))return E_INVALIDARG;
        next->bytes+=range.bytes;covered[range.pair]=true;next->ranges.push_back(range);
    }
    for(const auto value:covered)if(!value)return E_INVALIDARG;
    std::map<std::pair<uintptr_t,uintptr_t>,std::vector<UINT>> groups;
    for(UINT i=0;i<next->ranges.size();++i){const auto& range=next->ranges[i];
        groups[{reinterpret_cast<uintptr_t>(range.resources[0].Get()),reinterpret_cast<uintptr_t>(range.resources[1].Get())}].push_back(i);}
    std::vector<std::array<UINT,4>> metadata;
    for(const auto& [key,indices]:groups){Storage::Batch batch;batch.resources=next->ranges[indices.front()].resources;batch.first=UINT(metadata.size());
        for(const auto index:indices){const auto& range=next->ranges[index];
            for(UINT64 offset=0;offset<range.bytes;offset+=1024)metadata.push_back({UINT(range.offsets[0]+offset),UINT(range.offsets[1]+offset),
                UINT((std::min)(UINT64(1024),range.bytes-offset)),range.pair});}
        batch.count=UINT(metadata.size())-batch.first;next->batches.push_back(std::move(batch));
    }
    probe::Scope internal;
    std::array<D3D12_ROOT_PARAMETER,6> parameters{};
    for(UINT i=0;i<4;++i){parameters[i].ParameterType=D3D12_ROOT_PARAMETER_TYPE_SRV;parameters[i].Descriptor.ShaderRegister=i;}
    parameters[4].ParameterType=D3D12_ROOT_PARAMETER_TYPE_UAV;
    parameters[5].ParameterType=D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS;parameters[5].Constants.Num32BitValues=4;
    const D3D12_ROOT_SIGNATURE_DESC desc{UINT(parameters.size()),parameters.data(),0,nullptr,D3D12_ROOT_SIGNATURE_FLAG_NONE};
    ComPtr<ID3DBlob> signature,error;auto hr=D3D12SerializeRootSignature(&desc,D3D_ROOT_SIGNATURE_VERSION_1,&signature,&error);if(FAILED(hr))return hr;
    hr=device->CreateRootSignature(0,signature->GetBufferPointer(),signature->GetBufferSize(),IID_PPV_ARGS(&next->root));if(FAILED(hr))return hr;
    for(UINT i=0;i<2;++i){ComPtr<ID3DBlob> shader;
        hr=D3DCompile(Shader,sizeof(Shader)-1,nullptr,nullptr,nullptr,i?"Compare":"Initialize","cs_5_0",D3DCOMPILE_OPTIMIZATION_LEVEL3,0,&shader,&error);if(FAILED(hr))return hr;
        D3D12_COMPUTE_PIPELINE_STATE_DESC pipeline{};pipeline.pRootSignature=next->root.Get();pipeline.CS={shader->GetBufferPointer(),shader->GetBufferSize()};
        hr=device->CreateComputePipelineState(&pipeline,IID_PPV_ARGS(i?&next->compare:&next->initialize));if(FAILED(hr))return hr;
    }
    D3D12_RESOURCE_DESC buffer{};buffer.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;buffer.Width=metadata.size()*16;
    buffer.Height=buffer.DepthOrArraySize=buffer.MipLevels=buffer.SampleDesc.Count=1;buffer.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_UPLOAD;
    hr=device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&buffer,D3D12_RESOURCE_STATE_GENERIC_READ,nullptr,IID_PPV_ARGS(&next->metadata));if(FAILED(hr))return hr;
    void* mapped{};const D3D12_RANGE none{};hr=next->metadata->Map(0,&none,&mapped);if(FAILED(hr) || !mapped)return FAILED(hr)?hr:E_FAIL;
    std::memcpy(mapped,metadata.data(),size_t(buffer.Width));_mm_sfence();const D3D12_RANGE written{0,SIZE_T(buffer.Width)};next->metadata->Unmap(0,&written);
    buffer.Width=UINT64(pairs)*16;buffer.Flags=D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;heap.Type=D3D12_HEAP_TYPE_DEFAULT;
    hr=device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&buffer,D3D12_RESOURCE_STATE_UNORDERED_ACCESS,nullptr,IID_PPV_ARGS(&next->predicates));if(FAILED(hr))return hr;
    storage_=std::move(next);return S_OK;
}catch(const std::bad_alloc&){return E_OUTOFMEMORY;}
bool GpuGeometryCompare::Record(ID3D12GraphicsCommandList* list){
    if(!storage_ || storage_->recorded || !list || list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT || !SameDevice(storage_->device.Get(),list) ||
        !gpu::KeepCommandResources(list,{storage_.Get()}))return false;
    auto& s=*storage_.Get();s.recorded=true;probe::Scope internal;
    list->SetPredication(nullptr,0,D3D12_PREDICATION_OP_EQUAL_ZERO);
    list->SetComputeRootSignature(s.root.Get());list->SetPipelineState(s.initialize.Get());
    list->SetComputeRootShaderResourceView(0,s.ranges[0].resources[0]->GetGPUVirtualAddress());
    list->SetComputeRootShaderResourceView(1,s.ranges[0].resources[1]->GetGPUVirtualAddress());
    list->SetComputeRootShaderResourceView(2,s.gate->GetGPUVirtualAddress()+s.gateOffset);
    list->SetComputeRootShaderResourceView(3,s.metadata->GetGPUVirtualAddress());
    list->SetComputeRootUnorderedAccessView(4,s.predicates->GetGPUVirtualAddress());
    const UINT initial[]{0,0,0,s.pairs};list->SetComputeRoot32BitConstants(5,4,initial,0);list->Dispatch((s.pairs+255)/256,1,1);
    D3D12_RESOURCE_BARRIER barrier{};barrier.Type=D3D12_RESOURCE_BARRIER_TYPE_UAV;barrier.UAV.pResource=s.predicates.Get();list->ResourceBarrier(1,&barrier);
    list->SetPipelineState(s.compare.Get());
    for(const auto& batch:s.batches){
        list->SetComputeRootShaderResourceView(0,batch.resources[0]->GetGPUVirtualAddress());
        list->SetComputeRootShaderResourceView(1,batch.resources[1]->GetGPUVirtualAddress());
        const auto width=(std::min)(batch.count,1024u);
        const UINT values[]{batch.first,batch.count,width,0};list->SetComputeRoot32BitConstants(5,4,values,0);
        // Atomic AND/OR from independent ranges commute; only initialization and
        // final predication depend on their order, not adjacent compare dispatches.
        list->Dispatch(width,(batch.count+width-1)/width,1);
    }
    barrier={};barrier.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Transition={s.predicates.Get(),D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
        D3D12_RESOURCE_STATES(D3D12_RESOURCE_STATE_PREDICATION|D3D12_RESOURCE_STATE_COPY_SOURCE|D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE)};
    list->ResourceBarrier(1,&barrier);
    return true;
}
ID3D12Resource* GpuGeometryCompare::Predicates()const{return storage_?storage_->predicates.Get():nullptr;}
UINT GpuGeometryCompare::PairCount()const{return storage_?storage_->pairs:0;}
UINT GpuGeometryCompare::RangeCount()const{return storage_?UINT(storage_->ranges.size()):0;}
UINT GpuGeometryCompare::DispatchCount()const{return storage_ && storage_->recorded?UINT(storage_->batches.size())+1:0;}
UINT64 GpuGeometryCompare::ComparedBytes()const{return storage_?storage_->bytes:0;}
}
#endif
