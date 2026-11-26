#include "Render/StereoDescriptorAudit.hpp"
#include "Render/NativeStereoFamilies.hpp"
#include "Render/NativeTextureRead.hpp"
#include "Render/RenderProbeScope.hpp"
#include <dxgi1_6.h>
#include <wrl/client.h>
#include <cstdio>
#include <bit>
#include <stdexcept>
#include <source_location>
#include <string>
#include <thread>
#include <vector>
using Microsoft::WRL::ComPtr;
using namespace cvr::stereo;
namespace cvr::stereo::native_probe {bool ObserveBindings(){return true;}}
namespace cvr::stereo::native_probe {bool CurrentState(ID3D12GraphicsCommandList*,DrawRecord& output){output={};return false;}}
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
static void Require(bool v,const std::source_location at=std::source_location::current()){
    if(!v)throw std::runtime_error("descriptor audit assertion failed at line "+std::to_string(at.line()));
}
static void Check(HRESULT r,const std::source_location at=std::source_location::current()){
    if(FAILED(r))throw std::runtime_error("descriptor audit D3D12 error "+std::to_string(uint32_t(r))+" at line "+std::to_string(at.line()));
}
static void BorrowedFixedViews(){
    descriptors::BorrowedFixedRange absent;descriptors::BorrowedSourceRanges witness;descriptors::BorrowedFixedRangeSummary summary;
    Require(!absent && !descriptors::DescribeBorrowedFixedRange(absent,summary) && !summary.allSlotsQualified);
    Require(!descriptors::ValidateBorrowedFixedRange(absent,witness,0));
    Require(!descriptors::CaptureBorrowedFixedRange(nullptr,{},witness,0,absent) && !absent);
    using C=descriptors::borrowed_fixed_detail::DescriptorClass;D3D12_SHADER_RESOURCE_VIEW_DESC classified{};
    descriptors::Value nullView{};nullView[0]=2;nullView[4]=DXGI_FORMAT_R8G8B8A8_UNORM;nullView[5]=D3D12_SRV_DIMENSION_TEXTURE2D;
    nullView[6]=D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;nullView[8]=1;
    Require(descriptors::borrowed_fixed_detail::Classify(nullView,classified)==C::TypedNull);
    auto nullArray=nullView;nullArray[5]=D3D12_SRV_DIMENSION_TEXTURE2DARRAY;nullArray[10]=1;
    Require(descriptors::borrowed_fixed_detail::Classify(nullArray,classified)==C::TypedNull);
    auto invalidNull=nullView;invalidNull[1]=1;
    Require(descriptors::borrowed_fixed_detail::Classify(invalidNull,classified)==C::UnsupportedDescriptor);
    invalidNull=nullView;invalidNull[2]=55;
    Require(descriptors::borrowed_fixed_detail::Classify(invalidNull,classified)==C::UnsupportedDescriptor);
    invalidNull=nullView;invalidNull[1]=2;
    Require(descriptors::borrowed_fixed_detail::Classify(invalidNull,classified)==C::UnsupportedView);
    invalidNull=nullView;invalidNull[1]=3;invalidNull[2]=55;
    Require(descriptors::borrowed_fixed_detail::Classify(invalidNull,classified)==C::UnsupportedView);
    invalidNull=nullView;invalidNull[5]=D3D12_SRV_DIMENSION_BUFFER;
    Require(descriptors::borrowed_fixed_detail::Classify(invalidNull,classified)==C::UnsupportedView);
    invalidNull={};invalidNull[0]=2;invalidNull[5]=D3D12_SRV_DIMENSION_RAYTRACING_ACCELERATION_STRUCTURE;
    invalidNull[6]=D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;invalidNull[7]=0x10000;
    Require(descriptors::borrowed_fixed_detail::Classify(invalidNull,classified)==C::UnsupportedView);
    invalidNull[7]=0;Require(descriptors::borrowed_fixed_detail::Classify(invalidNull,classified)==C::UnsupportedView);
    invalidNull=nullView;invalidNull[8]=0;
    Require(descriptors::borrowed_fixed_detail::Classify(invalidNull,classified)==C::UnsupportedDescriptor);
    invalidNull=nullView;invalidNull[9]=1;
    Require(descriptors::borrowed_fixed_detail::Classify(invalidNull,classified)==C::UnsupportedDescriptor);
    invalidNull=nullView;invalidNull[7]=D3D12_REQ_MIP_LEVELS;
    Require(descriptors::borrowed_fixed_detail::Classify(invalidNull,classified)==C::UnsupportedDescriptor);
    invalidNull=nullView;invalidNull[8]=D3D12_REQ_MIP_LEVELS+1;
    Require(descriptors::borrowed_fixed_detail::Classify(invalidNull,classified)==C::UnsupportedDescriptor);
    invalidNull=nullView;invalidNull[10]=0x7FC00000u;
    Require(descriptors::borrowed_fixed_detail::Classify(invalidNull,classified)==C::UnsupportedDescriptor);
    invalidNull=nullView;invalidNull[10]=std::bit_cast<uint32_t>(-1.0f);
    Require(descriptors::borrowed_fixed_detail::Classify(invalidNull,classified)==C::UnsupportedDescriptor);
    invalidNull=nullArray;invalidNull[8]=0;
    Require(descriptors::borrowed_fixed_detail::Classify(invalidNull,classified)==C::UnsupportedDescriptor);
    invalidNull=nullArray;invalidNull[10]=0;
    Require(descriptors::borrowed_fixed_detail::Classify(invalidNull,classified)==C::UnsupportedDescriptor);
    invalidNull=nullArray;invalidNull[10]=UINT_MAX;
    Require(descriptors::borrowed_fixed_detail::Classify(invalidNull,classified)==C::UnsupportedDescriptor);
    invalidNull=nullArray;invalidNull[9]=D3D12_REQ_TEXTURE2D_ARRAY_AXIS_DIMENSION;
    Require(descriptors::borrowed_fixed_detail::Classify(invalidNull,classified)==C::UnsupportedDescriptor);
    invalidNull=nullArray;invalidNull[11]=1;
    Require(descriptors::borrowed_fixed_detail::Classify(invalidNull,classified)==C::UnsupportedDescriptor);
    invalidNull=nullArray;invalidNull[12]=0x7F800000u;
    Require(descriptors::borrowed_fixed_detail::Classify(invalidNull,classified)==C::UnsupportedDescriptor);
    auto remainingNull=nullView;remainingNull[8]=UINT_MAX;
    Require(descriptors::borrowed_fixed_detail::Classify(remainingNull,classified)==C::TypedNull);
    invalidNull={};Require(descriptors::borrowed_fixed_detail::Classify(invalidNull,classified)==C::Unknown);
    invalidNull=nullView;invalidNull[1]=1;invalidNull[2]=55;
    Require(descriptors::borrowed_fixed_detail::Classify(invalidNull,classified)==C::Candidate);
    D3D12_RESOURCE_DESC resource{};resource.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D;resource.Width=64;resource.Height=32;
    resource.DepthOrArraySize=1;resource.MipLevels=4;resource.SampleDesc.Count=1;resource.Format=DXGI_FORMAT_R8G8B8A8_TYPELESS;
    descriptors::Value canonical{};canonical[0]=2;canonical[1]=1;canonical[2]=71;
    D3D12_SHADER_RESOURCE_VIEW_DESC view{};view.ViewDimension=D3D12_SRV_DIMENSION_TEXTURE2D;view.Format=DXGI_FORMAT_R8G8B8A8_UNORM_SRGB;
    view.Shader4ComponentMapping=D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;view.Texture2D.MipLevels=UINT_MAX;
    const auto matches=[&](const auto& image,const auto& srv){return descriptors::borrowed_fixed_detail::ViewMatches(canonical,image,srv);};
    Require(matches(resource,view));auto wrong=view;wrong.Format=DXGI_FORMAT_BC7_UNORM;Require(!matches(resource,wrong));
    wrong=view;wrong.Texture2D.MostDetailedMip=4;Require(!matches(resource,wrong));
    wrong=view;wrong.Texture2D.MipLevels=5;Require(!matches(resource,wrong));
    wrong=view;wrong.Texture2D.MipLevels=0;Require(!matches(resource,wrong));
    wrong=view;wrong.Texture2D.PlaneSlice=1;Require(!matches(resource,wrong));
    wrong=view;wrong.Texture2D.ResourceMinLODClamp=std::bit_cast<float>(0x7FC00000u);Require(!matches(resource,wrong));
    auto writable=resource;writable.Flags=D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;Require(!matches(writable,view));
    auto array=resource;array.DepthOrArraySize=6;Require(!matches(array,view));
    auto arrayView=view;arrayView.ViewDimension=D3D12_SRV_DIMENSION_TEXTURE2DARRAY;arrayView.Texture2DArray={1,2,2,3,0,0};
    Require(matches(array,arrayView));wrong=arrayView;wrong.Texture2DArray.ArraySize=5;Require(!matches(array,wrong));
    wrong=arrayView;wrong.Texture2DArray.FirstArraySlice=6;Require(!matches(array,wrong));
    wrong=arrayView;wrong.Texture2DArray.MipLevels=4;Require(!matches(array,wrong));
    canonical[1]=5;canonical[13]=uint64_t(array.Dimension)|(uint64_t(array.Format)<<32);canonical[14]=array.Width;
    canonical[15]=uint64_t(array.Height)|(uint64_t(array.DepthOrArraySize)<<32)|(uint64_t(array.MipLevels)<<48);
    canonical[16]=uint64_t(array.Flags)|(uint64_t(array.Layout)<<32);canonical[17]=array.SampleDesc.Count;
    Require(matches(array,arrayView));++canonical[14];Require(!matches(array,arrayView));--canonical[14];
    canonical[1]|=2;Require(!matches(array,arrayView));
}
static void TypedBindings(ID3D12Device* device){
    D3D12_DESCRIPTOR_RANGE ranges[]{
        {D3D12_DESCRIPTOR_RANGE_TYPE_SRV,UINT_MAX,8,1,9},
        {D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER,3,0,0,1},
        {D3D12_DESCRIPTOR_RANGE_TYPE_SRV,1,67,0,0}};
    D3D12_ROOT_PARAMETER parameters[3]{};
    for(UINT i=0;i<3;++i){parameters[i].ParameterType=D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;
        parameters[i].ShaderVisibility=i==2?D3D12_SHADER_VISIBILITY_ALL:D3D12_SHADER_VISIBILITY_PIXEL;
        parameters[i].DescriptorTable={1,&ranges[i]};}
    D3D12_ROOT_SIGNATURE_DESC desc{3,parameters,0,nullptr,D3D12_ROOT_SIGNATURE_FLAG_NONE};
    ComPtr<ID3DBlob> bytes,error;Check(D3D12SerializeRootSignature(&desc,D3D_ROOT_SIGNATURE_VERSION_1,&bytes,&error));
    ComPtr<ID3D12RootSignature> root;Check(device->CreateRootSignature(0,bytes->GetBufferPointer(),bytes->GetBufferSize(),IID_PPV_ARGS(&root)));
    descriptors::RootCreated(root.Get(),bytes->GetBufferPointer(),bytes->GetBufferSize());
    const auto address=reinterpret_cast<uintptr_t>(root.Get());UINT parameter{},offset{};
    auto find=[&](D3D12_DESCRIPTOR_RANGE_TYPE type,UINT space,UINT reg,D3D12_SHADER_VISIBILITY visibility){
        return descriptors::FindBinding(address,type,space,reg,visibility,parameter,offset);};
    Require(find(D3D12_DESCRIPTOR_RANGE_TYPE_SRV,1,8,D3D12_SHADER_VISIBILITY_PIXEL) && parameter==0 && offset==9);
    Require(find(D3D12_DESCRIPTOR_RANGE_TYPE_SRV,1,32767,D3D12_SHADER_VISIBILITY_PIXEL) && offset==32768);
    Require(!find(D3D12_DESCRIPTOR_RANGE_TYPE_SRV,1,UINT_MAX,D3D12_SHADER_VISIBILITY_PIXEL) && parameter==UINT_MAX && offset==UINT_MAX);
    Require(!find(D3D12_DESCRIPTOR_RANGE_TYPE_SRV,1,7,D3D12_SHADER_VISIBILITY_PIXEL));
    Require(!find(D3D12_DESCRIPTOR_RANGE_TYPE_SRV,1,8,D3D12_SHADER_VISIBILITY_VERTEX));
    Require(!find(D3D12_DESCRIPTOR_RANGE_TYPE_SRV,1,8,D3D12_SHADER_VISIBILITY_ALL));
    Require(find(D3D12_DESCRIPTOR_RANGE_TYPE_SRV,0,67,D3D12_SHADER_VISIBILITY_PIXEL) && parameter==2 && !offset);
    Require(find(D3D12_DESCRIPTOR_RANGE_TYPE_SRV,0,67,D3D12_SHADER_VISIBILITY_VERTEX));
    Require(!find(D3D12_DESCRIPTOR_RANGE_TYPE_SRV,0,68,D3D12_SHADER_VISIBILITY_PIXEL));
    Require(find(D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER,0,2,D3D12_SHADER_VISIBILITY_PIXEL) && parameter==1 && offset==3);
    Require(!find(D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER,0,3,D3D12_SHADER_VISIBILITY_PIXEL));
    Require(!find(D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER,1,0,D3D12_SHADER_VISIBILITY_PIXEL));
    Require(!descriptors::FindBinding(address+1,D3D12_DESCRIPTOR_RANGE_TYPE_SRV,0,67,D3D12_SHADER_VISIBILITY_PIXEL,parameter,offset));
    ranges[0].OffsetInDescriptorsFromTableStart=7;
    Check(D3D12SerializeRootSignature(&desc,D3D_ROOT_SIGNATURE_VERSION_1,&bytes,&error));
    descriptors::RootCreated(root.Get(),bytes->GetBufferPointer(),bytes->GetBufferSize());
    Require(find(D3D12_DESCRIPTOR_RANGE_TYPE_SRV,1,UINT_MAX,D3D12_SHADER_VISIBILITY_PIXEL) && offset==UINT_MAX-1);

    ComPtr<ID3D12DescriptorHeap> srvs,samplers;
    D3D12_DESCRIPTOR_HEAP_DESC hd{D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,16,D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,0};
    Check(device->CreateDescriptorHeap(&hd,IID_PPV_ARGS(&srvs)));descriptors::HeapCreated(device,srvs.Get());
    hd.Type=D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER;Check(device->CreateDescriptorHeap(&hd,IID_PPV_ARGS(&samplers)));descriptors::HeapCreated(device,samplers.Get());
    const auto samplerStride=device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER);
    const auto samplerCpu=samplers->GetCPUDescriptorHandleForHeapStart();const auto samplerGpu=samplers->GetGPUDescriptorHandleForHeapStart();
    D3D12_SAMPLER_DESC sampler{};sampler.Filter=D3D12_FILTER_MIN_MAG_MIP_LINEAR;
    sampler.AddressU=D3D12_TEXTURE_ADDRESS_MODE_BORDER;sampler.AddressV=D3D12_TEXTURE_ADDRESS_MODE_CLAMP;sampler.AddressW=D3D12_TEXTURE_ADDRESS_MODE_WRAP;
    sampler.MipLODBias=-.75f;sampler.MaxAnisotropy=1;sampler.ComparisonFunc=D3D12_COMPARISON_FUNC_ALWAYS;
    sampler.BorderColor[0]=.25f;sampler.BorderColor[1]=.5f;sampler.BorderColor[2]=.75f;sampler.BorderColor[3]=1;
    sampler.MinLOD=.125f;sampler.MaxLOD=4.5f;
    const D3D12_CPU_DESCRIPTOR_HANDLE at{samplerCpu.ptr+3*samplerStride};
    device->CreateSampler(&sampler,at);descriptors::Sampler(&sampler,at);
    D3D12_RESOURCE_DESC td{};td.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D;td.Width=16;td.Height=8;
    td.DepthOrArraySize=6;td.MipLevels=4;td.Format=DXGI_FORMAT_R16G16B16A16_UNORM;td.SampleDesc.Count=1;
    D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;ComPtr<ID3D12Resource> texture;
    Check(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&td,D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,nullptr,IID_PPV_ARGS(&texture)));
    Require(!descriptors::MatchesHeldResourceCookie(texture.Get(),1));
    const auto cookie=descriptors::SourceResourceCookieAtCreation(texture.Get());
    Require(cookie && descriptors::MatchesHeldResourceCookie(texture.Get(),cookie) &&
        !descriptors::MatchesHeldResourceCookie(texture.Get(),cookie+1) && !descriptors::MatchesHeldResourceCookie(nullptr,cookie));
    D3D12_SHADER_RESOURCE_VIEW_DESC view{};view.Format=td.Format;view.ViewDimension=D3D12_SRV_DIMENSION_TEXTURE2DARRAY;
    view.Shader4ComponentMapping=D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;view.Texture2DArray={1,2,2,3,0,.5f};
    device->CreateShaderResourceView(texture.Get(),&view,srvs->GetCPUDescriptorHandleForHeapStart());
    descriptors::Srv(texture.Get(),&view,srvs->GetCPUDescriptorHandleForHeapStart());
    auto* list=reinterpret_cast<ID3D12GraphicsCommandList*>(0x123500);
    Require(descriptors::BeginBindings());descriptors::Root(list,root.Get());
    descriptors::Table(list,1,samplerGpu);descriptors::Table(list,2,srvs->GetGPUDescriptorHandleForHeapStart());
    descriptors::SamplerBinding sampled;descriptors::SrvBinding array;
    Require(descriptors::ResolveSamplerAtDraw(list,address,1,3,sampled) && sampled.stamp.gpu==samplerGpu.ptr+3*samplerStride && descriptors::MatchesDescriptor(sampled.stamp));
    const auto& actual=sampled.view;
    Require(actual.Filter==sampler.Filter && actual.AddressU==sampler.AddressU && actual.AddressV==sampler.AddressV && actual.AddressW==sampler.AddressW &&
        actual.MipLODBias==sampler.MipLODBias && actual.MaxAnisotropy==sampler.MaxAnisotropy && actual.ComparisonFunc==sampler.ComparisonFunc &&
        actual.MinLOD==sampler.MinLOD && actual.MaxLOD==sampler.MaxLOD);
    for(UINT i=0;i<4;++i)Require(actual.BorderColor[i]==sampler.BorderColor[i]);
    const auto samplerStamp=sampled.stamp;
    Require(!descriptors::ResolveSamplerAtDraw(list,address,1,2,sampled) && !sampled.stamp.gpu);
    Require(!descriptors::ResolveSamplerAtDraw(list,address+1,1,3,sampled));
    Require(!descriptors::ResolveSamplerAtDraw(list,address,64,3,sampled));
    Require(!descriptors::ResolveSamplerAtDraw(list,address,2,0,sampled));
    Require(!descriptors::CaptureDescriptor(list,address,1,3,sampled.stamp));
    Require(descriptors::ResolveSrvAtDraw(list,address,2,0,array) && array.resource==texture && array.stamp.value==array.description &&
        array.stamp.gpu==srvs->GetGPUDescriptorHandleForHeapStart().ptr && descriptors::MatchesDescriptor(array.stamp));
    const auto& av=array.view.Texture2DArray;
    Require(array.view.ViewDimension==view.ViewDimension && array.view.Format==view.Format && array.view.Shader4ComponentMapping==view.Shader4ComponentMapping &&
        av.MostDetailedMip==1 && av.MipLevels==2 && av.FirstArraySlice==2 && av.ArraySize==3 && !av.PlaneSlice && av.ResourceMinLODClamp==.5f);
    auto arrayStamp=array.stamp;
    view.Texture2DArray.MipLevels=UINT_MAX;view.Texture2DArray.ResourceMinLODClamp=0;
    device->CreateShaderResourceView(texture.Get(),&view,srvs->GetCPUDescriptorHandleForHeapStart());descriptors::Srv(texture.Get(),&view,srvs->GetCPUDescriptorHandleForHeapStart());
    Require(!descriptors::MatchesDescriptor(arrayStamp) && descriptors::ResolveSrvAtDraw(list,address,2,0,array) && array.view.Texture2DArray.MipLevels==UINT_MAX);
    arrayStamp=array.stamp;texture.Reset();Require(array.resource && descriptors::MatchesDescriptor(arrayStamp));
    descriptors::Reset(list);Require(!descriptors::ResolveSamplerAtDraw(list,address,1,3,sampled) && !descriptors::ResolveSrvAtDraw(list,address,2,0,array));
    Require(descriptors::MatchesDescriptor(samplerStamp) && !descriptors::MatchesDescriptor(arrayStamp));
    descriptors::Root(list,root.Get());descriptors::Table(list,1,samplerGpu);
    {probe::Scope internal;Require(descriptors::MatchesDescriptor(samplerStamp));}
    sampler.MaxLOD=9;device->CreateSampler(&sampler,at);descriptors::Sampler(&sampler,at);Require(!descriptors::MatchesDescriptor(samplerStamp));
    Require(descriptors::ResolveSamplerAtDraw(list,address,1,3,sampled));const auto changedStamp=sampled.stamp;
    samplers.Reset();Require(!descriptors::MatchesDescriptor(changedStamp) && !descriptors::ResolveSamplerAtDraw(list,address,1,3,sampled));
    descriptors::EndBindings();
}
int main() try {
    BorrowedFixedViews();
    ComPtr<IDXGIFactory4> factory;Check(CreateDXGIFactory1(IID_PPV_ARGS(&factory)));
    ComPtr<IDXGIAdapter> adapter;Check(factory->EnumWarpAdapter(IID_PPV_ARGS(&adapter)));
    ComPtr<ID3D12Device> device;Check(D3D12CreateDevice(adapter.Get(),D3D_FEATURE_LEVEL_11_0,IID_PPV_ARGS(&device)));
    descriptors::Initialize(device.Get());
    ComPtr<ID3D12DescriptorHeap> cpu,gpu;
    D3D12_DESCRIPTOR_HEAP_DESC hd{D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,512,D3D12_DESCRIPTOR_HEAP_FLAG_NONE,0};
    Check(device->CreateDescriptorHeap(&hd,IID_PPV_ARGS(&cpu)));descriptors::HeapCreated(device.Get(),cpu.Get());
    hd.Flags=D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE;Check(device->CreateDescriptorHeap(&hd,IID_PPV_ARGS(&gpu)));descriptors::HeapCreated(device.Get(),gpu.Get());
    const auto stride=device->GetDescriptorHandleIncrementSize(hd.Type);
    auto source=cpu->GetCPUDescriptorHandleForHeapStart(),destination=gpu->GetCPUDescriptorHandleForHeapStart();
    D3D12_SHADER_RESOURCE_VIEW_DESC view{};view.Format=DXGI_FORMAT_R8G8B8A8_UNORM;view.ViewDimension=D3D12_SRV_DIMENSION_TEXTURE2D;
    view.Shader4ComponentMapping=D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;view.Texture2D.MipLevels=1;
    device->CreateShaderResourceView(nullptr,&view,source);descriptors::Srv(nullptr,&view,source);
    device->CopyDescriptorsSimple(1,destination,source,hd.Type);
    UINT one=1;descriptors::Copy(device.Get(),1,&destination,&one,1,&source,&one,hd.Type);
    D3D12_DESCRIPTOR_RANGE range{D3D12_DESCRIPTOR_RANGE_TYPE_SRV,4,0,0,0};
    D3D12_ROOT_PARAMETER parameter{};parameter.ParameterType=D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;parameter.DescriptorTable={1,&range};
    D3D12_ROOT_SIGNATURE_DESC rootDesc{1,&parameter,0,nullptr,D3D12_ROOT_SIGNATURE_FLAG_NONE};
    ComPtr<ID3DBlob> bytes,error;Check(D3D12SerializeRootSignature(&rootDesc,D3D_ROOT_SIGNATURE_VERSION_1,&bytes,&error));
    ComPtr<ID3D12RootSignature> root;Check(device->CreateRootSignature(0,bytes->GetBufferPointer(),bytes->GetBufferSize(),IID_PPV_ARGS(&root)));
    descriptors::RootCreated(root.Get(),bytes->GetBufferPointer(),bytes->GetBufferSize());
    native_probe::DrawRecord draw{};draw.list=0x123400;draw.root=reinterpret_cast<uintptr_t>(root.Get());draw.tableMask=1;draw.tables[0]=gpu->GetGPUDescriptorHandleForHeapStart().ptr;
    descriptors::BeginCapture();draw.sequence=1;descriptors::Snapshot(draw);
    auto snapshot=[&](unsigned at){Require(CyberpunkVR_DescriptorBindingCount>at);return reinterpret_cast<const descriptors::Binding*>(CyberpunkVR_DescriptorBindings)[at];};
    const auto first=snapshot(0);Require(first.status==1 && first.valueCount==4 && first.heap);
    const auto ids=reinterpret_cast<const uint32_t*>(CyberpunkVR_DescriptorIds);
    Require(ids[0]!=0 && ids[1]==0);
    const auto firstId=ids[0];
    const auto firstValue=reinterpret_cast<const descriptors::Value*>(CyberpunkVR_DescriptorValues)[firstId];
    Require(firstValue[0]==2 && firstValue[2]==0 && firstValue[4]==DXGI_FORMAT_R8G8B8A8_UNORM);
    Require(!firstValue[1] && std::all_of(firstValue.begin()+13,firstValue.end(),[](auto x){return x==0;}));
    auto next=destination;next.ptr+=8*stride;
    descriptors::Copy(device.Get(),1,&next,&one,1,&source,&one,hd.Type);
    draw.sequence=2;descriptors::Snapshot(draw);Require(snapshot(1).version==first.version);
    view.Format=DXGI_FORMAT_R16G16B16A16_FLOAT;descriptors::Srv(nullptr,&view,source);
    descriptors::Copy(device.Get(),1,&destination,&one,1,&source,&one,hd.Type);
    draw.sequence=3;descriptors::Snapshot(draw);Require(snapshot(2).version>first.version);
    Require(reinterpret_cast<const descriptors::Value*>(CyberpunkVR_DescriptorValues)[firstId]==firstValue);
    descriptors::EndCapture();draw.sequence=4;descriptors::Snapshot(draw);Require(CyberpunkVR_DescriptorDrawCount==3);
    ID3D12CommandList* list=reinterpret_cast<ID3D12CommandList*>(draw.list);descriptors::BeforeSubmit(1,&list);
    const auto* captured=reinterpret_cast<const descriptors::DrawBindings*>(CyberpunkVR_DescriptorDraws);
    Require(captured[0].complete==6 && captured[1].complete==6 && captured[2].complete==3 && !CyberpunkVR_DescriptorPendingLists);
    Require(!CyberpunkVR_DescriptorCaptureFailure && !(CyberpunkVR_DescriptorCaptureSeq.load()&1));
    // Large bindless ranges retain exact timeline identity without per-draw copies.
    range.NumDescriptors=256;Check(D3D12SerializeRootSignature(&rootDesc,D3D_ROOT_SIGNATURE_VERSION_1,&bytes,&error));
    descriptors::RootCreated(root.Get(),bytes->GetBufferPointer(),bytes->GetBufferSize());
    descriptors::BeginCapture();descriptors::Snapshot(draw);Require(snapshot(0).status==1 && !snapshot(0).valueCount);
    descriptors::Reset(reinterpret_cast<ID3D12GraphicsCommandList*>(draw.list));
    Require(reinterpret_cast<const descriptors::DrawBindings*>(CyberpunkVR_DescriptorDraws)[0].complete==8 && !CyberpunkVR_DescriptorPendingLists);
    descriptors::EndCapture();
    std::array<D3D12_DESCRIPTOR_RANGE,64> wideRanges{};std::array<D3D12_ROOT_PARAMETER,64> wideParameters{};
    for(UINT i=0;i<64;++i){wideRanges[i]={D3D12_DESCRIPTOR_RANGE_TYPE_SRV,1,i,0,0};
        wideParameters[i].ParameterType=D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;wideParameters[i].DescriptorTable={1,&wideRanges[i]};}
    D3D12_ROOT_SIGNATURE_DESC wideRoot{64,wideParameters.data(),0,nullptr,D3D12_ROOT_SIGNATURE_FLAG_NONE};
    Check(D3D12SerializeRootSignature(&wideRoot,D3D_ROOT_SIGNATURE_VERSION_1,&bytes,&error));
    descriptors::RootCreated(root.Get(),bytes->GetBufferPointer(),bytes->GetBufferSize());
    descriptors::BeginCapture();draw.tableMask=0;
    descriptors::Root(reinterpret_cast<ID3D12GraphicsCommandList*>(draw.list),root.Get());
    descriptors::Table(reinterpret_cast<ID3D12GraphicsCommandList*>(draw.list),63,gpu->GetGPUDescriptorHandleForHeapStart());
    descriptors::Snapshot(draw);descriptors::EndCapture();descriptors::BeforeSubmit(1,&list);
    Require(snapshot(0).parameter==63 && reinterpret_cast<const descriptors::DrawBindings*>(CyberpunkVR_DescriptorDraws)[0].complete==3);
    range.NumDescriptors=UINT32_MAX;Check(D3D12SerializeRootSignature(&rootDesc,D3D_ROOT_SIGNATURE_VERSION_1,&bytes,&error));
    descriptors::RootCreated(root.Get(),bytes->GetBufferPointer(),bytes->GetBufferSize());
    descriptors::BeginCapture();draw.tableMask=1;descriptors::Snapshot(draw);descriptors::EndCapture();descriptors::BeforeSubmit(1,&list);
    Require(snapshot(0).count==512 && snapshot(0).status==1 && reinterpret_cast<const descriptors::DrawBindings*>(CyberpunkVR_DescriptorDraws)[0].complete==3);
    descriptors::EndCapture();
    const auto retainedGpu=gpu->GetGPUDescriptorHandleForHeapStart().ptr;
    const D3D12_CONSTANT_BUFFER_VIEW_DESC cbv{0x10000,256};descriptors::Cbv(&cbv,source);
    descriptors::Copy(device.Get(),1,&destination,&one,1,&source,&one,hd.Type);
    descriptors::CbvBinding cbvValue;
    Require(descriptors::ResolveCpuCbvAddress(source.ptr,cbvValue) && cbvValue.address==0x10000 && cbvValue.bytes==256);
    const auto recorded=CyberpunkVR_DescriptorDrawCount.load();auto* bindingList=reinterpret_cast<ID3D12GraphicsCommandList*>(draw.list);
    Require(descriptors::BeginBindings() && descriptors::Healthy() && CyberpunkVR_DescriptorLiveMetadataActive==1);descriptors::Root(bindingList,root.Get());
    descriptors::Table(bindingList,0,{retainedGpu});Require(descriptors::ResolveCbv(bindingList,draw.root,0,0,cbvValue) && cbvValue.address==0x10000);
    descriptors::Snapshot(draw);Require(CyberpunkVR_DescriptorDrawCount==recorded && !CyberpunkVR_DescriptorPendingLists);
    Require(descriptors::BeginBindings() && !descriptors::ResolveCbv(bindingList,draw.root,0,0,cbvValue));descriptors::EndBindings();
    Require(CyberpunkVR_DescriptorLiveMetadataActive==0);
    for(uint32_t live:{0u,1u,1u,0u}){
        CyberpunkVR_DescriptorLiveMetadata=live;Require(descriptors::BeginBindings());
        Require(CyberpunkVR_DescriptorLiveMetadataActive==live);
        const D3D12_CONSTANT_BUFFER_VIEW_DESC newer{0x20000,512};
        descriptors::Cbv(&newer,source);descriptors::Copy(device.Get(),1,&destination,&one,1,&source,&one,hd.Type);
        descriptors::Cbv(&cbv,source);
        Require(descriptors::ResolveCbvAddress(retainedGpu,cbvValue) && cbvValue.address==0x20000 && cbvValue.bytes==512);
        // A full capture overrides live mode and protects pending snapshots
        // until their command list is submitted, even after EndCapture.
        descriptors::BeginCapture();Require(CyberpunkVR_DescriptorLiveMetadataActive==0);
        descriptors::Snapshot(draw);descriptors::EndCapture();Require(!descriptors::BeginBindings());
        descriptors::Copy(device.Get(),1,&destination,&one,1,&source,&one,hd.Type);
        descriptors::BeforeSubmit(1,&list);
        Require(reinterpret_cast<const descriptors::DrawBindings*>(CyberpunkVR_DescriptorDraws)[0].complete==6);
        Require(descriptors::BeginBindings());descriptors::EndBindings();
    }
    CyberpunkVR_DescriptorLiveMetadata=1;
    for(uint32_t dense:{0u,1u,0u,1u}){
        descriptors::BeginCapture();descriptors::Snapshot(draw);const auto before=snapshot(0);
        CyberpunkVR_DescriptorDenseSlots=dense;
        descriptors::Cbv(&cbv,{source.ptr+8*stride});
        Require(CyberpunkVR_DescriptorDenseActive==dense);
        Require(descriptors::ResolveCbvAddress(retainedGpu,cbvValue) && cbvValue.address==0x10000);
        descriptors::Snapshot(draw);Require(snapshot(1).version==before.version && snapshot(1).heap==before.heap);
        descriptors::EndCapture();descriptors::BeforeSubmit(1,&list);
        Require(reinterpret_cast<const descriptors::DrawBindings*>(CyberpunkVR_DescriptorDraws)[0].complete==3);
    }
    const D3D12_CONSTANT_BUFFER_VIEW_DESC stampA{0x31000,256},stampB{0x32000,512};
    const D3D12_CPU_DESCRIPTOR_HANDLE stampSource{source.ptr+24*stride},stampDestination{destination.ptr+24*stride};
    for(uint32_t dense:{0u,1u})for(uint32_t live:{0u,1u}){
        CyberpunkVR_DescriptorDenseSlots=dense;CyberpunkVR_DescriptorLiveMetadata=live;
        Require(descriptors::BeginBindings() && CyberpunkVR_DescriptorLiveMetadataActive==live);
        descriptors::Root(bindingList,root.Get());descriptors::Table(bindingList,0,{retainedGpu});
        descriptors::Cbv(nullptr,stampSource);descriptors::Cbv(nullptr,stampDestination);
        descriptors::Cbv(&stampA,stampDestination);descriptors::DescriptorStamp stamp;
        Require(descriptors::CaptureDescriptor(bindingList,draw.root,0,24,stamp) && stamp.gpu==retainedGpu+24*stride &&
            stamp.heap && stamp.value[0]==1 && stamp.value[4]==stampA.BufferLocation && descriptors::MatchesDescriptor(stamp));
        Require(!descriptors::MatchesDescriptor({}));auto wrongHeap=stamp;++wrongHeap.heap;
        Require(!descriptors::MatchesDescriptor(wrongHeap));
        descriptors::DescriptorStamp invalid=stamp;
        Require(!descriptors::CaptureDescriptor(bindingList,draw.root+1,0,24,invalid) && !invalid.gpu);
        Require(!descriptors::CaptureDescriptor(bindingList,draw.root,64,0,invalid));
        Require(!descriptors::CaptureDescriptor(bindingList,draw.root,1,0,invalid));
        Require(!descriptors::CaptureDescriptor(bindingList,draw.root,0,25,invalid));
        descriptors::Table(bindingList,0,{retainedGpu+16*stride});
        Require(descriptors::MatchesDescriptor(stamp));descriptors::ClearBindings(bindingList);
        Require(descriptors::MatchesDescriptor(stamp));descriptors::Table(bindingList,0,{retainedGpu});
        // Live metadata can overwrite a sole-owner value in place, and clearing
        // a slot can recycle its ID. Neither preserves an earlier receipt.
        descriptors::Cbv(&stampB,stampDestination);Require(!descriptors::MatchesDescriptor(stamp));
        descriptors::Cbv(&stampA,stampDestination);Require(descriptors::MatchesDescriptor(stamp));
        descriptors::Cbv(nullptr,stampDestination);Require(!descriptors::MatchesDescriptor(stamp));
        invalid=stamp;Require(!descriptors::CaptureDescriptor(bindingList,draw.root,0,24,invalid) && !invalid.gpu);
        descriptors::Cbv(&stampB,stampDestination);Require(!descriptors::MatchesDescriptor(stamp));
        descriptors::Cbv(&stampA,stampDestination);Require(descriptors::MatchesDescriptor(stamp));
        descriptors::Cbv(&stampB,stampSource);
        descriptors::Copy(device.Get(),1,&stampDestination,&one,1,&stampSource,&one,hd.Type);
        Require(!descriptors::MatchesDescriptor(stamp));
        descriptors::Cbv(&stampA,stampSource);
        descriptors::Copy(device.Get(),1,&stampDestination,&one,1,&stampSource,&one,hd.Type);
        Require(descriptors::MatchesDescriptor(stamp));descriptors::EndBindings();
        Require(descriptors::MatchesDescriptor(stamp));
        {
            cvr::stereo::probe::Scope internal;descriptors::CbvBinding saved;
            Require(descriptors::MatchesDescriptor(stamp));
            Require(descriptors::ResolveCpuCbvAddress(stampDestination.ptr,saved) && saved.address==stampA.BufferLocation);
            descriptors::Cbv(&stampB,stampDestination);
            Require(descriptors::MatchesDescriptor(stamp));
        }
    }
    CyberpunkVR_DescriptorLiveMetadata=1;
    descriptors::BeginCapture();Require(!descriptors::BeginBindings());descriptors::EndCapture();
    range.NumDescriptors=1;Check(D3D12SerializeRootSignature(&rootDesc,D3D_ROOT_SIGNATURE_VERSION_1,&bytes,&error));
    descriptors::RootCreated(root.Get(),bytes->GetBufferPointer(),bytes->GetBufferSize());
    auto metadata=[&](ID3D12Resource* resource,const D3D12_SHADER_RESOURCE_VIEW_DESC& srv){
        const D3D12_CPU_DESCRIPTOR_HANDLE src{source.ptr+16*stride},dst{destination.ptr+16*stride};
        device->CreateShaderResourceView(resource,&srv,src);descriptors::Srv(resource,&srv,src);
        device->CopyDescriptorsSimple(1,dst,src,hd.Type);descriptors::Copy(device.Get(),1,&dst,&one,1,&src,&one,hd.Type);
        Require(descriptors::BeginBindings());descriptors::Root(bindingList,root.Get());descriptors::Table(bindingList,0,{retainedGpu+16*stride});
        descriptors::SrvBinding lease;
        Require(descriptors::ResolveSrvAtDraw(bindingList,draw.root,0,0,lease) && lease.resource.Get()==resource && lease.view.Format==srv.Format &&
            lease.view.ViewDimension==srv.ViewDimension && lease.description[2]);
        Require(!descriptors::ResolveSrvAtDraw(bindingList,draw.root+1,0,0,lease));
        Require(!descriptors::ResolveSrvAtDraw(bindingList,draw.root,1,0,lease));descriptors::EndBindings();
        descriptors::BeginCapture();draw.tables[0]=retainedGpu+16*stride;descriptors::Snapshot(draw);
        Require(snapshot(0).status==2 && snapshot(0).valueCount==1);
        const auto id=reinterpret_cast<const uint32_t*>(CyberpunkVR_DescriptorIds)[0];
        const auto value=reinterpret_cast<const descriptors::Value*>(CyberpunkVR_DescriptorValues)[id];
        descriptors::EndCapture();descriptors::BeforeSubmit(1,&list);draw.tables[0]=retainedGpu;return value;
    };
    D3D12_RESOURCE_DESC resourceDesc{};resourceDesc.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;resourceDesc.Width=4096;
    resourceDesc.Height=1;resourceDesc.DepthOrArraySize=resourceDesc.MipLevels=1;resourceDesc.SampleDesc.Count=1;resourceDesc.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_UPLOAD;ComPtr<ID3D12Resource> resource;
    Check(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&resourceDesc,D3D12_RESOURCE_STATE_GENERIC_READ,nullptr,IID_PPV_ARGS(&resource)));
    D3D12_SHADER_RESOURCE_VIEW_DESC bufferView{};bufferView.Format=DXGI_FORMAT_R32_TYPELESS;bufferView.ViewDimension=D3D12_SRV_DIMENSION_BUFFER;
    bufferView.Shader4ComponentMapping=D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;bufferView.Buffer={4,64,0,D3D12_BUFFER_SRV_FLAG_RAW};
    const auto bufferInfo=metadata(resource.Get(),bufferView);
    Require(bufferInfo[1]==13 && bufferInfo[2] && bufferInfo[13]==D3D12_RESOURCE_DIMENSION_BUFFER && bufferInfo[14]==4096 &&
        bufferInfo[15]==(1ull|(1ull<<32)|(1ull<<48)) && bufferInfo[16]==(uint64_t(D3D12_TEXTURE_LAYOUT_ROW_MAJOR)<<32) &&
        bufferInfo[17]==1 && (bufferInfo[18]&65535)==D3D12_HEAP_TYPE_UPLOAD && bufferInfo[19]==resource->GetGPUVirtualAddress());
    resource.Reset();resourceDesc.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D;resourceDesc.Width=8;resourceDesc.Height=4;
    resourceDesc.MipLevels=3;resourceDesc.Format=DXGI_FORMAT_R8_UNORM;resourceDesc.Layout=D3D12_TEXTURE_LAYOUT_UNKNOWN;heap.Type=D3D12_HEAP_TYPE_DEFAULT;
    Check(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&resourceDesc,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,nullptr,IID_PPV_ARGS(&resource)));
    view.Format=DXGI_FORMAT_R8_UNORM;view.Texture2D.MostDetailedMip=1;view.Texture2D.MipLevels=UINT_MAX;
    const auto textureInfo=metadata(resource.Get(),view);
    Require(textureInfo[1]==13 && textureInfo[2]!=bufferInfo[2] && textureInfo[13]==(uint64_t(D3D12_RESOURCE_DIMENSION_TEXTURE2D)|(uint64_t(DXGI_FORMAT_R8_UNORM)<<32)) &&
        textureInfo[14]==8 && textureInfo[15]==(4ull|(1ull<<32)|(3ull<<48)) && textureInfo[16]==0 && textureInfo[17]==1 &&
        (textureInfo[18]&65535)==D3D12_HEAP_TYPE_DEFAULT && textureInfo[19]==0 && textureInfo[7]==1 && textureInfo[8]==UINT_MAX);
    Require(descriptors::BeginBindings());descriptors::Root(bindingList,root.Get());descriptors::Table(bindingList,0,{retainedGpu+16*stride});
    descriptors::SrvBinding retained;descriptors::DescriptorStamp resourceStamp;
    Require(descriptors::CaptureDescriptor(bindingList,draw.root,0,0,resourceStamp) && descriptors::MatchesDescriptor(resourceStamp));
    Require(descriptors::ResolveSrvAtDraw(bindingList,draw.root,0,0,retained));resource.Reset();
    Require(retained.resource && retained.resource->GetDesc().Width==8 && descriptors::MatchesDescriptor(resourceStamp));retained.resource.Reset();
    Require(!descriptors::MatchesDescriptor(resourceStamp));
    Require(!descriptors::CaptureDescriptor(bindingList,draw.root,0,0,resourceStamp) && !resourceStamp.gpu);
    descriptors::DescriptorStamp heapStamp;descriptors::Table(bindingList,0,{retainedGpu});
    Require(descriptors::CaptureDescriptor(bindingList,draw.root,0,0,heapStamp));
    descriptors::Table(bindingList,0,{retainedGpu+16*stride});
    Require(!descriptors::ResolveSrvAtDraw(bindingList,draw.root,0,0,retained));descriptors::EndBindings();
    cpu.Reset();
    Require(!descriptors::ResolveCpuCbvAddress(source.ptr,cbvValue));
    Require(descriptors::ResolveCbvAddress(retainedGpu,cbvValue) && cbvValue.address==0x10000);
    gpu.Reset();Require(!descriptors::ResolveCbvAddress(retainedGpu,cbvValue) && !descriptors::MatchesDescriptor(heapStamp));
    Check(device->CreateDescriptorHeap(&hd,IID_PPV_ARGS(&gpu)));descriptors::HeapCreated(device.Get(),gpu.Get());
    descriptors::Cbv(&cbv,gpu->GetCPUDescriptorHandleForHeapStart());
    Require(descriptors::BeginBindings());descriptors::Root(bindingList,root.Get());
    descriptors::Table(bindingList,0,gpu->GetGPUDescriptorHandleForHeapStart());descriptors::DescriptorStamp replacementStamp;
    Require(descriptors::CaptureDescriptor(bindingList,draw.root,0,0,replacementStamp) && descriptors::MatchesDescriptor(replacementStamp));
    Require(replacementStamp.heap!=heapStamp.heap && replacementStamp.value==heapStamp.value);
    // Exercise a replaced heap at an identical saved address even when WARP
    // chooses a different GPU handle for the actual replacement allocation.
    heapStamp.gpu=replacementStamp.gpu;Require(!descriptors::MatchesDescriptor(heapStamp));
    descriptors::EndBindings();gpu.Reset();Require(!descriptors::MatchesDescriptor(replacementStamp));
    TypedBindings(device.Get());
    std::atomic<bool> failed{};std::vector<std::thread> workers;
    for(unsigned thread=0;thread<8;++thread)workers.emplace_back([&]{try{
        for(unsigned i=0;i<64;++i){ComPtr<ID3D12DescriptorHeap> temporary;
            const D3D12_DESCRIPTOR_HEAP_DESC h{D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,16,D3D12_DESCRIPTOR_HEAP_FLAG_NONE,0};
            Check(device->CreateDescriptorHeap(&h,IID_PPV_ARGS(&temporary)));descriptors::HeapCreated(device.Get(),temporary.Get());
            descriptors::Cbv(&cbv,temporary->GetCPUDescriptorHandleForHeapStart());}
    }catch(...){failed=true;}});
    for(auto& thread:workers)thread.join();descriptors::BeginCapture();descriptors::EndCapture();
    Require(!failed && !CyberpunkVR_DescriptorCaptureFailure && !CyberpunkVR_DescriptorStoreStats[0] &&
        !CyberpunkVR_DescriptorStoreStats[1] && !CyberpunkVR_DescriptorStoreStats[2] && !CyberpunkVR_DescriptorStoreStats[4]);
    std::puts("PASS descriptor ownership, submission receipts across rewrites/ID reuse/retirement, copy/version validity, 64 root slots and unbounded tables");return 0;
}catch(const std::exception& e){std::fprintf(stderr,"%s\n",e.what());return 1;}
