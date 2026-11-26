#include "Render/StereoDescriptorAudit.hpp"
#include "Render/NativeStereoFamilies.hpp"
#include "Render/NativeTextureRead.hpp"
#include "Render/RenderProbeScope.hpp"
#include <dxgi1_6.h>
#include <cstdio>
#include <source_location>
#include <stdexcept>
#include <string>
using Microsoft::WRL::ComPtr;
using namespace cvr::stereo;
namespace cvr::stereo::native_probe {
bool ObserveBindings(){return true;}
bool CurrentState(ID3D12GraphicsCommandList*,DrawRecord& output){output={};return false;}
}
namespace cvr::stereo::families {
std::shared_ptr<const Family> Bound(ID3D12GraphicsCommandList*){return {};}
std::shared_ptr<const Family> Lookup(ID3D12PipelineState*){return {};}
}
namespace cvr::stereo::camera_upload {Context CurrentContext(uintptr_t){return {};}}
namespace cvr::stereo::native_recording {bool ValidateRecording(const Lease&,ID3D12GraphicsCommandList*,camera_upload::Context){return false;}}
#if !CVR_RUST_VRIK
// The Rust build defines it (the header forwards to Rust).
namespace cvr::stereo::scene_state {bool CaptureSourceBindings(ID3D12GraphicsCommandList*,Bindings& output){output={};return false;}}
#endif
namespace cvr::stereo::texture_read {
bool PinFixedAsset(ID3D12GraphicsCommandList*,uint64_t,FixedAssetPins& output){output={};return false;}
bool GetFixedPinnedResource(const FixedAssetPins&,ComPtr<ID3D12Resource>& output){output.Reset();return false;}
bool AcquirePinnedImmutable(ID3D12GraphicsCommandList*,const FixedAssetPins&,ImmutableLease& output){output={};return false;}
bool ValidateImmutable(const ImmutableLease&,ID3D12CommandQueue*){return false;}
uint32_t LastFailure(){return 120;}
std::array<uint64_t,8> FixedPinLastDetails(){return {};}
}
extern "C" std::atomic<uint32_t> CyberpunkVR_NativeStereoProbeState{1};
static void Require(bool value,const std::source_location at=std::source_location::current()){
    if(!value)throw std::runtime_error("table metadata assertion failed at line "+std::to_string(at.line()));
}
static void Check(HRESULT value,const std::source_location at=std::source_location::current()){
    if(FAILED(value))throw std::runtime_error("table metadata D3D12 error "+std::to_string(uint32_t(value))+" at line "+std::to_string(at.line()));
}
static ComPtr<ID3D12RootSignature> Root(ID3D12Device* device,std::span<const D3D12_DESCRIPTOR_RANGE> ranges,bool constants=false){
    std::vector<D3D12_ROOT_PARAMETER> parameters(ranges.size()+(constants?1:0));
    for(size_t i=0;i<ranges.size();++i){parameters[i].ParameterType=D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;
        parameters[i].DescriptorTable={1,&ranges[i]};parameters[i].ShaderVisibility=D3D12_SHADER_VISIBILITY_ALL;}
    if(constants){parameters.back().ParameterType=D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS;parameters.back().Constants={13,9,1};}
    D3D12_ROOT_SIGNATURE_DESC desc{UINT(parameters.size()),parameters.data(),0,nullptr,D3D12_ROOT_SIGNATURE_FLAG_NONE};
    ComPtr<ID3DBlob> bytes,error;Check(D3D12SerializeRootSignature(&desc,D3D_ROOT_SIGNATURE_VERSION_1,&bytes,&error));
    ComPtr<ID3D12RootSignature> root;Check(device->CreateRootSignature(0,bytes->GetBufferPointer(),bytes->GetBufferSize(),IID_PPV_ARGS(&root)));
    descriptors::RootCreated(root.Get(),bytes->GetBufferPointer(),bytes->GetBufferSize());return root;
}
static ComPtr<ID3D12DescriptorHeap> Heap(ID3D12Device* device,UINT count,D3D12_DESCRIPTOR_HEAP_TYPE type=D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV){
    D3D12_DESCRIPTOR_HEAP_DESC desc{type,count,D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,0};ComPtr<ID3D12DescriptorHeap> heap;
    Check(device->CreateDescriptorHeap(&desc,IID_PPV_ARGS(&heap)));descriptors::HeapCreated(device,heap.Get());return heap;
}
static ComPtr<ID3D12Resource> Buffer(ID3D12Device* device){
    D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;D3D12_RESOURCE_DESC desc{};
    desc.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;desc.Width=65536;desc.Height=1;desc.DepthOrArraySize=1;desc.MipLevels=1;
    desc.SampleDesc.Count=1;desc.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;desc.Flags=D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;
    ComPtr<ID3D12Resource> result;Check(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_COMMON,nullptr,IID_PPV_ARGS(&result)));
    return result;
}
static ULONG Refs(IUnknown* object){object->AddRef();return object->Release();}
int main()try{
    using namespace descriptors;
    ComPtr<IDXGIFactory4> factory;Check(CreateDXGIFactory1(IID_PPV_ARGS(&factory)));ComPtr<IDXGIAdapter> adapter;
    Check(factory->EnumWarpAdapter(IID_PPV_ARGS(&adapter)));ComPtr<ID3D12Device> device;
    Check(D3D12CreateDevice(adapter.Get(),D3D_FEATURE_LEVEL_11_0,IID_PPV_ARGS(&device)));Initialize(device.Get());
    BorrowedRangeObserverReady(true);Require(BeginBindings());
    auto resource=Buffer(device.Get()),counter=Buffer(device.Get());uint64_t cookie=99;
    Require(!GetHeldResourceCookie(resource,cookie) && !cookie);
    const auto createdCookie=SourceResourceCookieAtCreation(resource.Get());Require(createdCookie);
    Require(GetHeldResourceCookie(resource,cookie) && cookie==createdCookie);
    Require(!GetHeldResourceCookie({},cookie) && !cookie);
    auto heap=Heap(device.Get(),16);const auto stride=device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
    const auto cpu=heap->GetCPUDescriptorHandleForHeapStart();const auto gpu=heap->GetGPUDescriptorHandleForHeapStart();
    auto at=[&](UINT slot){return D3D12_CPU_DESCRIPTOR_HANDLE{cpu.ptr+uint64_t(slot)*stride};};
    D3D12_CONSTANT_BUFFER_VIEW_DESC cbv{resource->GetGPUVirtualAddress(),256};
    D3D12_SHADER_RESOURCE_VIEW_DESC srv{};srv.Format=DXGI_FORMAT_UNKNOWN;srv.ViewDimension=D3D12_SRV_DIMENSION_BUFFER;
    srv.Shader4ComponentMapping=D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;srv.Buffer={2,4,16,D3D12_BUFFER_SRV_FLAG_NONE};
    D3D12_SHADER_RESOURCE_VIEW_DESC nullSrv{};nullSrv.Format=DXGI_FORMAT_R8G8B8A8_UNORM;nullSrv.ViewDimension=D3D12_SRV_DIMENSION_TEXTURE2D;
    nullSrv.Shader4ComponentMapping=D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;nullSrv.Texture2D.MipLevels=1;
    D3D12_UNORDERED_ACCESS_VIEW_DESC uav{};uav.Format=DXGI_FORMAT_UNKNOWN;uav.ViewDimension=D3D12_UAV_DIMENSION_BUFFER;
    uav.Buffer={3,8,16,4096,D3D12_BUFFER_UAV_FLAG_NONE};
    auto nullUav=uav;nullUav.Buffer.CounterOffsetInBytes=0;
    {
        MutationScope mutation;
        device->CreateConstantBufferView(&cbv,at(0));Cbv(&cbv,at(0));
        device->CreateShaderResourceView(resource.Get(),&srv,at(1));Srv(resource.Get(),&srv,at(1));
        device->CreateShaderResourceView(nullptr,&nullSrv,at(2));Srv(nullptr,&nullSrv,at(2));
        device->CreateUnorderedAccessView(resource.Get(),counter.Get(),&uav,at(3));Uav(resource.Get(),counter.Get(),&uav,at(3));
        device->CreateUnorderedAccessView(nullptr,nullptr,&nullUav,at(4));Uav(nullptr,nullptr,&nullUav,at(4));
    }
    const std::array ranges{
        D3D12_DESCRIPTOR_RANGE{D3D12_DESCRIPTOR_RANGE_TYPE_CBV,1,0,0,0},
        D3D12_DESCRIPTOR_RANGE{D3D12_DESCRIPTOR_RANGE_TYPE_SRV,2,0,0,1},
        D3D12_DESCRIPTOR_RANGE{D3D12_DESCRIPTOR_RANGE_TYPE_UAV,2,0,0,3}};
    auto root=Root(device.Get(),ranges);const std::array<ComPtr<ID3D12DescriptorHeap>,1> heaps{heap};
    const std::array bound{BoundTableMetadata{0,gpu},BoundTableMetadata{1,gpu},BoundTableMetadata{2,gpu}};
    const auto resourceRefs=Refs(resource.Get()),counterRefs=Refs(counter.Get());
    TableMetadataSnapshot snapshot;TableMetadataSummary summary;
    Require(CaptureTableMetadata(root,heaps,bound,snapshot) && snapshot && DescribeTableMetadata(snapshot,summary));
    Require(summary.complete && !summary.unknownFlags && summary.ranges.size()==3 && summary.values.size()==5 && !summary.unknownSlots);
    Require(summary.values[0][0]==1 && summary.values[0][4]==cbv.BufferLocation && summary.values[0][5]==256);
    Require(summary.values[1][0]==2 && summary.values[1][2]==createdCookie && summary.values[1][7]==2);
    Require(summary.values[2][0]==2 && !summary.values[2][1] && !summary.values[2][2]);
    uint64_t counterCookie{};Require(GetHeldResourceCookie(counter,counterCookie));
    Require(summary.values[3][0]==3 && summary.values[3][2]==createdCookie && summary.values[3][3]==counterCookie && summary.values[3][10]==4096);
    Require(summary.values[4][0]==3 && !summary.values[4][1] && !summary.values[4][2] && !summary.values[4][3]);
    Require(Refs(resource.Get())==resourceRefs && Refs(counter.Get())==counterRefs);
    Require(summary.ranges[1].tableOffset==1 && summary.ranges[1].firstValue==1 && summary.ranges[1].identity.count==2);
    {MutationScope mutation;Require(!ValidateTableMetadataNoOverwrite(snapshot));}
    Require(ValidateTableMetadataNoOverwrite(snapshot));
    {
        MutationScope mutation;device->CreateShaderResourceView(nullptr,&nullSrv,at(15));Srv(nullptr,&nullSrv,at(15));
    }
    Require(ValidateTableMetadataNoOverwrite(snapshot));
    // Resource destruction does not invalidate descriptor-byte metadata or grant
    // permission to revive its old resource cookie. The snapshot owns no assets.
    resource.Reset();counter.Reset();Require(ValidateTableMetadataNoOverwrite(snapshot) && DescribeTableMetadata(snapshot,summary));
    {
        MutationScope mutation;device->CreateShaderResourceView(nullptr,&nullSrv,at(1));Srv(nullptr,&nullSrv,at(1));
    }
    Require(!ValidateTableMetadataNoOverwrite(snapshot) && !DescribeTableMetadata(snapshot,summary) && summary.values.empty());
    Require(CaptureTableMetadata(root,heaps,bound,snapshot));
    {MutationScope mutation;probe::Scope internal;Srv(nullptr,&nullSrv,at(2));}
    Require(!ValidateTableMetadataNoOverwrite(snapshot));
    Require(CaptureTableMetadata(root,heaps,bound,snapshot) && DescribeTableMetadata(snapshot,summary));
    Require(!summary.complete && summary.unknownSlots==1 && (summary.unknownFlags&TableMetadataUnknownSlots));

    const std::array unbounded{D3D12_DESCRIPTOR_RANGE{D3D12_DESCRIPTOR_RANGE_TYPE_SRV,UINT_MAX,0,1,2}};
    auto dynamicRoot=Root(device.Get(),unbounded);const std::array dynamicBound{BoundTableMetadata{0,{gpu.ptr+stride}}};
    Require(CaptureTableMetadata(dynamicRoot,heaps,dynamicBound,snapshot) && DescribeTableMetadata(snapshot,summary));
    Require(summary.ranges.size()==1 && summary.ranges[0].physicalRemainder && summary.ranges[0].identity.count==13 && summary.values.size()==13);
    Require(summary.ranges[0].identity.gpu==gpu.ptr+3*stride && (summary.unknownFlags&TableMetadataWrongDescriptorKind));
    auto mixed=Root(device.Get(),std::span(ranges).first(1),true);
    Require(CaptureTableMetadata(mixed,heaps,std::span(bound).first(1),snapshot) && DescribeTableMetadata(snapshot,summary));
    Require(!summary.complete && (summary.unknownFlags&TableMetadataOtherRootParameters));
    Require(CaptureTableMetadata(root,heaps,std::span(bound).first(1),snapshot) && DescribeTableMetadata(snapshot,summary));
    Require(summary.unboundTables==2 && (summary.unknownFlags&TableMetadataUnboundTables));

    auto rejects=[&](const auto& selectedRoot,const auto& selectedHeaps,const auto& selectedTables){
        Require(!CaptureTableMetadata(selectedRoot,selectedHeaps,selectedTables,snapshot) && !snapshot && TableMetadataLastFailure()!=TableMetadataFailure::None);};
    std::array duplicate{bound[0],bound[0]};rejects(root,heaps,duplicate);
    std::array invalid{BoundTableMetadata{64,gpu}};rejects(root,heaps,invalid);
    invalid={BoundTableMetadata{3,gpu}};rejects(root,heaps,invalid);
    invalid={BoundTableMetadata{0,{0}}};rejects(root,heaps,invalid);
    invalid={BoundTableMetadata{0,{gpu.ptr+1}}};rejects(root,heaps,invalid);
    invalid={BoundTableMetadata{2,{gpu.ptr+14*stride}}};rejects(root,heaps,invalid);
    const std::array<ComPtr<ID3D12DescriptorHeap>,2> duplicateHeaps{heap,heap};rejects(root,duplicateHeaps,bound);
    const std::array<ComPtr<ID3D12DescriptorHeap>,0> noHeaps{};rejects(root,noHeaps,bound);
    const std::array<BoundTableMetadata,0> noTables{};
    Require(CaptureTableMetadata(root,noHeaps,noTables,snapshot) && DescribeTableMetadata(snapshot,summary));
    Require(summary.unboundTables==3 && summary.values.empty() && !summary.complete);

    auto samplerHeap=Heap(device.Get(),4,D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER);
    const std::array<ComPtr<ID3D12DescriptorHeap>,2> bothHeaps{heap,samplerHeap};
    D3D12_SAMPLER_DESC sampler{};sampler.Filter=D3D12_FILTER_MIN_MAG_MIP_LINEAR;
    sampler.AddressU=sampler.AddressV=sampler.AddressW=D3D12_TEXTURE_ADDRESS_MODE_CLAMP;
    sampler.MaxAnisotropy=1;sampler.ComparisonFunc=D3D12_COMPARISON_FUNC_ALWAYS;sampler.MaxLOD=D3D12_FLOAT32_MAX;
    {MutationScope mutation;device->CreateSampler(&sampler,samplerHeap->GetCPUDescriptorHandleForHeapStart());Sampler(&sampler,samplerHeap->GetCPUDescriptorHandleForHeapStart());}
    const std::array samplerRanges{D3D12_DESCRIPTOR_RANGE{D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER,1,0,0,0}};
    auto samplerRoot=Root(device.Get(),samplerRanges);const std::array samplerBound{BoundTableMetadata{0,samplerHeap->GetGPUDescriptorHandleForHeapStart()}};
    Require(CaptureTableMetadata(samplerRoot,bothHeaps,samplerBound,snapshot) && DescribeTableMetadata(snapshot,summary));
    Require(summary.complete && summary.values.size()==1 && summary.values[0][0]==4);
    rejects(samplerRoot,heaps,samplerBound);

    auto large=Heap(device.Get(),TableMetadataSlotLimit+1);const std::array<ComPtr<ID3D12DescriptorHeap>,1> largeHeaps{large};
    const std::array whole{D3D12_DESCRIPTOR_RANGE{D3D12_DESCRIPTOR_RANGE_TYPE_SRV,UINT_MAX,0,1,0}};
    auto wholeRoot=Root(device.Get(),whole);std::array largeBound{BoundTableMetadata{0,large->GetGPUDescriptorHandleForHeapStart()}};
    rejects(wholeRoot,largeHeaps,largeBound);Require(TableMetadataLastFailure()==TableMetadataFailure::Capacity);
    largeBound[0].table.ptr+=stride;
    Require(CaptureTableMetadata(wholeRoot,largeHeaps,largeBound,snapshot) && DescribeTableMetadata(snapshot,summary));
    Require(summary.values.size()==TableMetadataSlotLimit && summary.unknownSlots==TableMetadataSlotLimit);
    EndBindings();Require(!ValidateTableMetadataNoOverwrite(snapshot));snapshot={};
    std::puts("table metadata capture, weak resource independence, bounds and no-overwrite validation passed");return 0;
}catch(const std::exception& error){std::fprintf(stderr,"%s\n",error.what());return 1;}
