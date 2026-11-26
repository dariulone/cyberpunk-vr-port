#include "Render/GpuMultilayerMembership.hpp"
#include "Render/CommandResources.hpp"
#include "Render/RenderProbeScope.hpp"
#include <d3dcompiler.h>
#include <algorithm>
#include <atomic>
#include <climits>
#include <cstring>
#include <intrin.h>
#include <new>
#include <utility>
#include <vector>

#if CVR_RUST_VRIK
extern "C" {
HRESULT cvr_mlm_create(ID3D12Device*,const void*,size_t,ID3D12Resource*,UINT64,void**);
bool cvr_mlm_record(void*,ID3D12GraphicsCommandList*,bool(*)(void*,void*));
void cvr_mlm_info(void*,void*);
}
#endif
namespace cvr::stereo {
using Microsoft::WRL::ComPtr;
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin makes no GpuMultilayerMembership (prepared_pair_native.rs drives the Rust storage)
// The check: rust/crates/cvr-render/src/multilayer_membership.rs (tools/single_pass_tests/multilayer_membership_test runs
// its cases against it in its Rust variant). The storage is the Rust object; the family check and the command
// resources stay here.
namespace {
struct RustSource {
    ID3D12Resource* b4;ID3D12Resource* t5;UINT64 b4Offset,t5Offset,t5Bytes;UINT firstElement,numElements;const UINT* membership;
    UINT64 sourceId;UINT eye;
};
struct RustInfo {ID3D12Resource* predicates;ID3D12Resource* results;UINT sources,dispatches;};
static_assert(sizeof(RustSource)==72 && sizeof(RustInfo)==24,"mirrored by multilayer_membership.rs");
bool Family(const source_selectors::FamilyKey& value){
    return value.rootSha256==multilayer_selectors::RootSha256 && value.vertexSha256==multilayer_selectors::VertexSha256 &&
        value.pixelSha256==multilayer_selectors::PixelSha256;
}
bool Keep(void* list,void* storage)noexcept{
    try{return gpu::KeepCommandResources(static_cast<ID3D12GraphicsCommandList*>(list),{static_cast<IUnknown*>(storage)});}catch(...){return false;}
}
RustInfo Info(IUnknown* storage){RustInfo info{};if(storage)cvr_mlm_info(storage,&info);return info;}
}
// The Rust object: its IUnknown vtable is Rust's.
struct GpuMultilayerMembership::Storage:IUnknown {};
GpuMultilayerMembership::GpuMultilayerMembership()=default;
GpuMultilayerMembership::~GpuMultilayerMembership()=default;
HRESULT GpuMultilayerMembership::Create(ID3D12Device* device,std::span<const MultilayerMembershipSource> sources,
                                      ID3D12Resource* gate,UINT64 gateOffset)try{
    if(storage_ || !device)return E_INVALIDARG;
    std::vector<RustSource> lent;lent.reserve(sources.size());
    for(const auto& source:sources){
        if(!Family(source.family))return E_INVALIDARG;
        lent.push_back({source.b4.Get(),source.t5.Get(),source.b4Offset,source.t5Offset,source.t5Bytes,source.firstElement,
            source.numElements,source.membership.data(),source.sourceId,source.eye});
    }
    void* created{};const auto hr=cvr_mlm_create(device,lent.data(),lent.size(),gate,gateOffset,&created);if(FAILED(hr))return hr;
    storage_.Attach(static_cast<Storage*>(static_cast<IUnknown*>(created)));return S_OK;
}catch(const std::bad_alloc&){return E_OUTOFMEMORY;}
bool GpuMultilayerMembership::Record(ID3D12GraphicsCommandList* list){return storage_ && list && cvr_mlm_record(storage_.Get(),list,&Keep);}
ID3D12Resource* GpuMultilayerMembership::Predicates()const{return Info(storage_.Get()).predicates;}
ID3D12Resource* GpuMultilayerMembership::Results()const{return Info(storage_.Get()).results;}
UINT GpuMultilayerMembership::SourceCount()const{return Info(storage_.Get()).sources;}
UINT GpuMultilayerMembership::DispatchCount()const{return Info(storage_.Get()).dispatches;}
UINT64 GpuMultilayerMembership::ReadbackBytes()const{return UINT64(SourceCount())*48;}
#endif
#else
namespace {
constexpr char Shader[]=R"(
ByteAddressBuffer b4 : register(t0);
ByteAddressBuffer t5 : register(t1);
ByteAddressBuffer membership : register(t2);
ByteAddressBuffer producer : register(t3);
ByteAddressBuffer metadata : register(t4);
RWByteAddressBuffer predicates : register(u0);
RWByteAddressBuffer results : register(u1);
cbuffer Parameters : register(b0) {
    uint firstMetadata;
};
groupshared uint enabled, baseRecord, mask, invalidBase;
groupshared uint failures[32], missing[32], recordCounts[32], selectorCounts[32];
[numthreads(32,1,1)]
void Check(uint lane : SV_GroupIndex,uint3 group : SV_GroupID) {
    uint metadataOffset=(firstMetadata+group.x)*32;
    uint4 addresses=metadata.Load4(metadataOffset);
    uint2 identities=metadata.Load2(metadataOffset+16);
    uint b4Offset=addresses.x,t5Offset=addresses.y,firstElement=addresses.z,numElements=addresses.w;
    uint membershipOffset=identities.x,sourceIndex=identities.y;
    failures[lane]=0;missing[lane]=0xffffffffu;
    recordCounts[lane]=0;selectorCounts[lane]=0;
    if(lane==0) {
        enabled=any(producer.Load2(0)!=0)?1:0;
        baseRecord=0;mask=0;invalidBase=0;
        [branch] if(enabled) {
            float raw=asfloat(b4.Load(b4Offset+128));
            mask=b4.Load(b4Offset+224);
            if(isfinite(raw) && raw>=0.0f && raw<4294967296.0f)baseRecord=(uint)raw;
            else invalidBase=1;
        }
    }
    GroupMemoryBarrierWithGroupSync();
    [branch] if(enabled && !invalidBase && (mask&(1u<<lane))) {
        if(baseRecord>0xffffffffu-lane)failures[lane]=2;
        else {
            uint record=baseRecord+lane;
            [branch] if(record>=numElements)failures[lane]=4;
            else {
                uint address=t5Offset+(firstElement+record)*128;
                uint a=t5.Load(address+100), b=t5.Load(address+104), c=t5.Load(address+108);
                uint indices[5]={a,c>>16,c&65535u,b&65535u,b>>16};
                recordCounts[lane]=1;selectorCounts[lane]=5;
                [unroll] for(uint i=0;i<5;++i) {
                    uint index=indices[i],reason=0;
                    [branch] if(index>=32768u)reason=8;
                    else if(!(membership.Load(membershipOffset+(index>>5)*4)&(1u<<(index&31))))reason=16;
                    failures[lane]|=reason;
                    if(reason && missing[lane]==0xffffffffu)missing[lane]=index;
                }
            }
        }
    }
    GroupMemoryBarrierWithGroupSync();
    if(lane==0) {
        uint error=invalidBase,records=0,selectors=0,firstBit=0xffffffffu,firstIndex=0xffffffffu;
        [unroll] for(uint i=0;i<32;++i) {
            error|=failures[i];records+=recordCounts[i];selectors+=selectorCounts[i];
            if(failures[i] && firstBit==0xffffffffu){firstBit=i;firstIndex=missing[i];}
        }
        predicates.Store4(sourceIndex*16,uint4(enabled && !error?1:0,0,enabled && error?1:0,0));
        results.Store4(sourceIndex*32,uint4(enabled,error,baseRecord,mask));
        results.Store4(sourceIndex*32+16,uint4(records,selectors,firstBit,firstIndex));
    }
}
)";
bool SameDevice(ID3D12Device* device,ID3D12DeviceChild* child){
    ComPtr<ID3D12Device> actual;return child && SUCCEEDED(child->GetDevice(IID_PPV_ARGS(&actual))) && actual.Get()==device;
}
bool Range(ID3D12Device* device,ID3D12Resource* resource,UINT64 offset,UINT64 bytes){
    if(!SameDevice(device,resource) || !bytes || offset%4 || offset>UINT_MAX || bytes>UINT_MAX-offset)return false;
    const auto desc=resource->GetDesc();const auto gpu=resource->GetGPUVirtualAddress();
    return desc.Dimension==D3D12_RESOURCE_DIMENSION_BUFFER && !(desc.Flags&D3D12_RESOURCE_FLAG_DENY_SHADER_RESOURCE) &&
        gpu && !(gpu&3) && gpu<=UINT64_MAX-desc.Width && offset<=desc.Width && bytes<=desc.Width-offset;
}
bool Family(const source_selectors::FamilyKey& value){
    return value.rootSha256==multilayer_selectors::RootSha256 && value.vertexSha256==multilayer_selectors::VertexSha256 &&
        value.pixelSha256==multilayer_selectors::PixelSha256;
}
}
struct GpuMultilayerMembership::Storage final:IUnknown {
    std::atomic<ULONG> refs{1};ComPtr<ID3D12Device> device;ComPtr<ID3D12RootSignature> root;ComPtr<ID3D12PipelineState> pipeline;
    ComPtr<ID3D12Resource> gate,membership,metadata,predicates,results;
    struct Group {UINT first{},count{},source{};};
    std::vector<Group> groups;
    std::vector<MultilayerMembershipSource> sources;UINT64 gateOffset{};bool recorded{};
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** out)override{
        if(!out)return E_POINTER;*out=nullptr;if(id!=__uuidof(IUnknown))return E_NOINTERFACE;*out=this;AddRef();return S_OK;
    }
    ULONG STDMETHODCALLTYPE AddRef()override{return ++refs;}
    ULONG STDMETHODCALLTYPE Release()override{const auto n=--refs;if(!n)delete this;return n;}
};
GpuMultilayerMembership::GpuMultilayerMembership()=default;
GpuMultilayerMembership::~GpuMultilayerMembership()=default;
HRESULT GpuMultilayerMembership::Create(ID3D12Device* device,std::span<const MultilayerMembershipSource> sources,
                                      ID3D12Resource* gate,UINT64 gateOffset)try{
    if(storage_ || !device || device->GetNodeCount()!=1 || sources.empty() || sources.size()>512 ||
        gateOffset%8 || !Range(device,gate,gateOffset,8))return E_INVALIDARG;
    for(const auto& source:sources){
        if(!Family(source.family) || source.eye>1 || !source.sourceId || !source.numElements ||
            source.t5Bytes%multilayer_selectors::Stride || source.t5Bytes>1024*1024 ||
            !Range(device,source.b4.Get(),source.b4Offset,multilayer_selectors::B4Bytes) ||
            !Range(device,source.t5.Get(),source.t5Offset,source.t5Bytes))return E_INVALIDARG;
        const UINT64 first=UINT64(source.firstElement)*multilayer_selectors::Stride;
        const UINT64 bytes=UINT64(source.numElements)*multilayer_selectors::Stride;
        if(first>source.t5Bytes || bytes>source.t5Bytes-first)return E_INVALIDARG;
    }
    ComPtr<Storage> next;next.Attach(new Storage);next->device=device;next->gate=gate;next->gateOffset=gateOffset;
    next->sources.assign(sources.begin(),sources.end());probe::Scope internal;
    std::vector<std::vector<UINT>> grouped;
    for(UINT i=0;i<sources.size();++i){auto found=std::find_if(grouped.begin(),grouped.end(),[&](const auto& group){
            const auto& first=sources[group.front()];return first.b4==sources[i].b4 && first.t5==sources[i].t5;});
        if(found==grouped.end()){grouped.push_back({i});}else found->push_back(i);
    }
    std::vector<std::array<UINT,8>> metadata;metadata.reserve(sources.size());next->groups.reserve(grouped.size());
    for(const auto& group:grouped){next->groups.push_back({UINT(metadata.size()),UINT(group.size()),group.front()});
        for(const auto index:group){const auto& source=sources[index];
            metadata.push_back({UINT(source.b4Offset),UINT(source.t5Offset),source.firstElement,source.numElements,index*4096,index,0,0});
        }
    }
    if(metadata.size()!=sources.size() || next->groups.empty() || next->groups.size()>512)return E_INVALIDARG;
    std::array<D3D12_ROOT_PARAMETER,8> parameters{};
    for(UINT i=0;i<5;++i){parameters[i].ParameterType=D3D12_ROOT_PARAMETER_TYPE_SRV;parameters[i].Descriptor.ShaderRegister=i;}
    for(UINT i=0;i<2;++i){parameters[5+i].ParameterType=D3D12_ROOT_PARAMETER_TYPE_UAV;parameters[5+i].Descriptor.ShaderRegister=i;}
    parameters[7].ParameterType=D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS;parameters[7].Constants.Num32BitValues=1;
    const D3D12_ROOT_SIGNATURE_DESC root{UINT(parameters.size()),parameters.data(),0,nullptr,D3D12_ROOT_SIGNATURE_FLAG_NONE};
    ComPtr<ID3DBlob> binary,error;auto hr=D3D12SerializeRootSignature(&root,D3D_ROOT_SIGNATURE_VERSION_1,&binary,&error);if(FAILED(hr))return hr;
    hr=device->CreateRootSignature(0,binary->GetBufferPointer(),binary->GetBufferSize(),IID_PPV_ARGS(&next->root));if(FAILED(hr))return hr;
    hr=D3DCompile(Shader,sizeof(Shader)-1,nullptr,nullptr,nullptr,"Check","cs_5_0",
        D3DCOMPILE_OPTIMIZATION_LEVEL3|D3DCOMPILE_IEEE_STRICTNESS,0,&binary,&error);if(FAILED(hr))return hr;
    D3D12_COMPUTE_PIPELINE_STATE_DESC pipeline{};pipeline.pRootSignature=next->root.Get();pipeline.CS={binary->GetBufferPointer(),binary->GetBufferSize()};
    hr=device->CreateComputePipelineState(&pipeline,IID_PPV_ARGS(&next->pipeline));if(FAILED(hr))return hr;
    D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;desc.Width=sources.size()*4096;
    desc.Height=desc.DepthOrArraySize=desc.MipLevels=desc.SampleDesc.Count=1;desc.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_UPLOAD;
    hr=device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_GENERIC_READ,nullptr,IID_PPV_ARGS(&next->membership));if(FAILED(hr))return hr;
    void* mapped{};const D3D12_RANGE noRead{};hr=next->membership->Map(0,&noRead,&mapped);if(FAILED(hr) || !mapped)return FAILED(hr)?hr:E_FAIL;
    for(size_t i=0;i<sources.size();++i)std::memcpy(static_cast<uint8_t*>(mapped)+i*4096,sources[i].membership.data(),4096);
    _mm_sfence();const D3D12_RANGE written{0,SIZE_T(desc.Width)};next->membership->Unmap(0,&written);
    desc.Width=metadata.size()*sizeof(metadata[0]);
    hr=device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_GENERIC_READ,nullptr,IID_PPV_ARGS(&next->metadata));if(FAILED(hr))return hr;
    mapped=nullptr;hr=next->metadata->Map(0,&noRead,&mapped);if(FAILED(hr) || !mapped)return FAILED(hr)?hr:E_FAIL;
    std::memcpy(mapped,metadata.data(),SIZE_T(desc.Width));_mm_sfence();
    const D3D12_RANGE metadataWritten{0,SIZE_T(desc.Width)};next->metadata->Unmap(0,&metadataWritten);
    heap.Type=D3D12_HEAP_TYPE_DEFAULT;desc.Width=sources.size()*16;desc.Flags=D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;
    hr=device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_UNORDERED_ACCESS,nullptr,IID_PPV_ARGS(&next->predicates));if(FAILED(hr))return hr;
    desc.Width=sources.size()*32;
    hr=device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_UNORDERED_ACCESS,nullptr,IID_PPV_ARGS(&next->results));if(FAILED(hr))return hr;
    storage_=std::move(next);return S_OK;
}catch(const std::bad_alloc&){return E_OUTOFMEMORY;}
bool GpuMultilayerMembership::Record(ID3D12GraphicsCommandList* list){
    if(!storage_ || storage_->recorded || !list || list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT ||
        !SameDevice(storage_->device.Get(),list) || !gpu::KeepCommandResources(list,{storage_.Get()}))return false;
    auto& value=*storage_.Get();value.recorded=true;probe::Scope internal;
    list->SetPredication(nullptr,0,D3D12_PREDICATION_OP_EQUAL_ZERO);
    list->SetComputeRootSignature(value.root.Get());list->SetPipelineState(value.pipeline.Get());
    list->SetComputeRootShaderResourceView(2,value.membership->GetGPUVirtualAddress());
    list->SetComputeRootShaderResourceView(3,value.gate->GetGPUVirtualAddress()+value.gateOffset);
    list->SetComputeRootShaderResourceView(4,value.metadata->GetGPUVirtualAddress());
    list->SetComputeRootUnorderedAccessView(5,value.predicates->GetGPUVirtualAddress());
    list->SetComputeRootUnorderedAccessView(6,value.results->GetGPUVirtualAddress());
    for(const auto& group:value.groups){const auto& source=value.sources[group.source];
        list->SetComputeRootShaderResourceView(0,source.b4->GetGPUVirtualAddress());
        list->SetComputeRootShaderResourceView(1,source.t5->GetGPUVirtualAddress());
        list->SetComputeRoot32BitConstant(7,group.first,0);list->Dispatch(group.count,1,1);
    }
    std::array<D3D12_RESOURCE_BARRIER,2> barriers{};
    for(UINT i=0;i<2;++i){auto& barrier=barriers[i];barrier.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        const auto after=D3D12_RESOURCE_STATES(D3D12_RESOURCE_STATE_COPY_SOURCE|D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE|
            (i==0?D3D12_RESOURCE_STATE_PREDICATION:0));
        barrier.Transition={i==0?value.predicates.Get():value.results.Get(),D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,D3D12_RESOURCE_STATE_UNORDERED_ACCESS,after};}
    list->ResourceBarrier(UINT(barriers.size()),barriers.data());return true;
}
ID3D12Resource* GpuMultilayerMembership::Predicates()const{return storage_?storage_->predicates.Get():nullptr;}
ID3D12Resource* GpuMultilayerMembership::Results()const{return storage_?storage_->results.Get():nullptr;}
UINT GpuMultilayerMembership::SourceCount()const{return storage_?UINT(storage_->sources.size()):0;}
UINT GpuMultilayerMembership::DispatchCount()const{return storage_ && storage_->recorded?UINT(storage_->groups.size()):0;}
UINT64 GpuMultilayerMembership::ReadbackBytes()const{return UINT64(SourceCount())*48;}
#endif
}
