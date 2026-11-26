#include "Render/StereoDescriptorAudit.hpp"
#include "Render/NativeStereoFamilies.hpp"
#include "Render/NativeTextureRead.hpp"
#include "Render/StereoBindingBank.hpp"
#include "Render/CommandResources.hpp"
#include <d3d12sdklayers.h>
#include <d3dcompiler.h>
#include <dxgi1_6.h>
#include <algorithm>
#include <array>
#include <atomic>
#include <cstdio>
#include <cstring>
#include <iterator>
#include <source_location>
#include <stdexcept>
#include <string>
#include <thread>

using Microsoft::WRL::ComPtr;
using namespace cvr::stereo;
namespace audit=cvr::stereo::descriptors;
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

static void Require(bool value,const char* message,const std::source_location at=std::source_location::current()) {
    if(!value)throw std::runtime_error(std::string(message)+" at line "+std::to_string(at.line()));
}
static void Check(HRESULT value,const std::source_location at=std::source_location::current()) {
    if(FAILED(value))throw std::runtime_error("D3D12 HRESULT "+std::to_string(uint32_t(value))+" at line "+std::to_string(at.line()));
}
class Sentinel final:public IUnknown {
    std::atomic<ULONG> refs_{1};std::atomic<UINT>& destroyed_;
public:
    explicit Sentinel(std::atomic<UINT>& destroyed):destroyed_(destroyed){}
    ~Sentinel(){++destroyed_;}
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** output)override {
        if(!output)return E_POINTER;*output=nullptr;if(id!=__uuidof(IUnknown))return E_NOINTERFACE;*output=this;AddRef();return S_OK;
    }
    ULONG STDMETHODCALLTYPE AddRef()override{return ++refs_;}
    ULONG STDMETHODCALLTYPE Release()override{const auto left=--refs_;if(!left)delete this;return left;}
};
static void Transition(ID3D12GraphicsCommandList* list,ID3D12Resource* resource,D3D12_RESOURCE_STATES before,D3D12_RESOURCE_STATES after) {
    D3D12_RESOURCE_BARRIER barrier{};barrier.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Transition={resource,D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,before,after};list->ResourceBarrier(1,&barrier);
}
static ComPtr<ID3D12Resource> Buffer(ID3D12Device* device,UINT64 bytes,D3D12_HEAP_TYPE type,
                                   D3D12_RESOURCE_FLAGS flags,D3D12_RESOURCE_STATES state) {
    D3D12_HEAP_PROPERTIES heap{};heap.Type=type;
    D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;desc.Width=bytes;
    desc.Height=desc.DepthOrArraySize=desc.MipLevels=desc.SampleDesc.Count=1;desc.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;desc.Flags=flags;
    ComPtr<ID3D12Resource> result;Check(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,state,nullptr,IID_PPV_ARGS(&result)));return result;
}
static void WriteUInt(ID3D12Resource* resource,UINT value) {
    void* memory{};const D3D12_RANGE none{};Check(resource->Map(0,&none,&memory));std::memcpy(memory,&value,sizeof(value));
    const D3D12_RANGE written{0,sizeof(value)};resource->Unmap(0,&written);
}
static bool PrintValidationErrors(ID3D12Device* device){
    ComPtr<ID3D12InfoQueue> info;if(FAILED(device->QueryInterface(IID_PPV_ARGS(&info))))return false;bool failed{};
    for(UINT64 n=0;n<info->GetNumStoredMessages();++n){SIZE_T size{};info->GetMessage(n,nullptr,&size);std::vector<char> bytes(size);
        auto* message=reinterpret_cast<D3D12_MESSAGE*>(bytes.data());if(SUCCEEDED(info->GetMessage(n,message,&size)) && message->Severity<=D3D12_MESSAGE_SEVERITY_ERROR){
            std::fprintf(stderr,"%s\n",message->pDescription);failed=true;}}
    return failed;
}
static void Await(ID3D12CommandQueue* queue,ID3D12Fence* fence,UINT64 value) {
    Check(queue->Signal(fence,value));HANDLE event=CreateEventW(nullptr,FALSE,FALSE,nullptr);Require(event!=nullptr,"Fence event creation failed");
    const auto set=fence->SetEventOnCompletion(value,event);if(FAILED(set)){CloseHandle(event);Check(set);}
    const auto waited=WaitForSingleObject(event,10000);CloseHandle(event);Require(waited==WAIT_OBJECT_0,"WARP source-table GPU timeout");
    ComPtr<ID3D12Device> device;Check(queue->GetDevice(IID_PPV_ARGS(&device)));
    if(fence->GetCompletedValue()==UINT64_MAX || FAILED(device->GetDeviceRemovedReason())){
        PrintValidationErrors(device.Get());Check(device->GetDeviceRemovedReason());Require(false,"Removed device signaled a false completion");}
}
struct Provider {
    ComPtr<ID3D12Resource> resource,wrong;
    ComPtr<IUnknown> owner;
    bool wrongBacking{},mutate{},workerTimedOut{};
    UINT calls{};
    ID3D12Device* device{};
    D3D12_CPU_DESCRIPTOR_HANDLE mutationSlot{};
    D3D12_CONSTANT_BUFFER_VIEW_DESC mutationView{};
    HANDLE mutationDone{};
    std::thread worker;
    ~Provider(){if(worker.joinable())worker.join();if(mutationDone)CloseHandle(mutationDone);}
    static bool Resolve(void* user,ID3D12GraphicsCommandList*,UINT parameter,UINT offset,const audit::DescriptorStamp& stamp,
                        const ComPtr<IUnknown>& origin,audit::SourceCbvBacking& result) {
        auto& self=*static_cast<Provider*>(user);++self.calls;
        Require(parameter==33 && offset==0 && stamp.value[0]==1,"Unexpected CBV provider association");
        Require(!origin,"Ordinary fixture accidentally claimed native creation provenance");
        if(self.mutate){
            self.mutationDone=CreateEventW(nullptr,FALSE,FALSE,nullptr);Require(self.mutationDone!=nullptr,"Mutation event creation failed");
            self.worker=std::thread([&self]{audit::Cbv(&self.mutationView,self.mutationSlot);SetEvent(self.mutationDone);});
            self.workerTimedOut=WaitForSingleObject(self.mutationDone,5000)!=WAIT_OBJECT_0;
            if(self.workerTimedOut)return false;
        }
        result.resource=self.wrongBacking?self.wrong:self.resource;result.owner=self.owner;return true;
    }
    void FinishMutation(){if(worker.joinable())worker.join();if(mutationDone){CloseHandle(mutationDone);mutationDone=nullptr;}}
};

struct Fixture {
    static constexpr UINT TableCount=10,VisibleBase=4;
    ID3D12Device* device{};
    ComPtr<ID3D12RootSignature> root;
    ComPtr<ID3DBlob> rootBytes;
    ComPtr<ID3D12DescriptorHeap> cpu,visible,samplerCpu,samplerVisible,rtvs;
    ComPtr<ID3D12Resource> constants,texture,extra,unordered,counter,upload,zero,target;
    ComPtr<ID3D12GraphicsCommandList> list;
    ComPtr<ID3D12CommandAllocator> allocator,next;
    ComPtr<ID3D12PipelineState> pipeline;
    UINT stride{},samplerStride{},rtvStride{};
    D3D12_SHADER_RESOURCE_VIEW_DESC srv{};
    D3D12_UNORDERED_ACCESS_VIEW_DESC uav{};
    D3D12_SAMPLER_DESC sampler{};
    D3D12_CONSTANT_BUFFER_VIEW_DESC cbv{};
    D3D12_RESOURCE_DESC targetDesc{};
    scene_state::Bindings bindings;
    std::array<audit::SourceSlotUse,4> resourceUses{{
        {0,1,D3D12_DESCRIPTOR_RANGE_TYPE_CBV},{2,2,D3D12_DESCRIPTOR_RANGE_TYPE_SRV},
        {5,2,D3D12_DESCRIPTOR_RANGE_TYPE_UAV},{8,1,D3D12_DESCRIPTOR_RANGE_TYPE_SRV}}};
    std::array<audit::SourceSlotUse,1> samplerUses{{{1,1,D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER}}};
    std::array<audit::SourceTableRequest,2> requests;
    D3D12_RESOURCE_STATES counterState=D3D12_RESOURCE_STATE_COPY_DEST;

    D3D12_CPU_DESCRIPTOR_HANDLE Cpu(UINT offset)const{return {cpu->GetCPUDescriptorHandleForHeapStart().ptr+SIZE_T(offset)*stride};}
    D3D12_CPU_DESCRIPTOR_HANDLE VisibleCpu(UINT offset)const{return {visible->GetCPUDescriptorHandleForHeapStart().ptr+SIZE_T(VisibleBase+offset)*stride};}
    D3D12_GPU_DESCRIPTOR_HANDLE VisibleGpu()const{return {visible->GetGPUDescriptorHandleForHeapStart().ptr+UINT64(VisibleBase)*stride};}
    D3D12_CPU_DESCRIPTOR_HANDLE SamplerCpu(UINT offset)const{return {samplerCpu->GetCPUDescriptorHandleForHeapStart().ptr+SIZE_T(offset)*samplerStride};}
    D3D12_CPU_DESCRIPTOR_HANDLE SamplerVisibleCpu(UINT offset)const{return {samplerVisible->GetCPUDescriptorHandleForHeapStart().ptr+SIZE_T(offset)*samplerStride};}
    D3D12_CPU_DESCRIPTOR_HANDLE Rtv(UINT offset)const{return {rtvs->GetCPUDescriptorHandleForHeapStart().ptr+SIZE_T(offset)*rtvStride};}
    std::span<const uint8_t> RootData()const{return {static_cast<const uint8_t*>(rootBytes->GetBufferPointer()),rootBytes->GetBufferSize()};}

    explicit Fixture(ID3D12Device* input):device(input) {
        stride=device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
        samplerStride=device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER);
        rtvStride=device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV);
        std::array<D3D12_DESCRIPTOR_RANGE,36> ranges{};std::array<D3D12_ROOT_PARAMETER,36> params{};
        for(UINT n=0;n<params.size();++n){ranges[n]={D3D12_DESCRIPTOR_RANGE_TYPE_CBV,1,100+n,99,0};
            params[n].ParameterType=D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;params[n].DescriptorTable={1,&ranges[n]};}
        ranges[0]={D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER,UINT_MAX,0,0,1};params[0].ShaderVisibility=D3D12_SHADER_VISIBILITY_PIXEL;
        const D3D12_DESCRIPTOR_RANGE mixed[]{
            {D3D12_DESCRIPTOR_RANGE_TYPE_CBV,1,0,0,0},{D3D12_DESCRIPTOR_RANGE_TYPE_SRV,1,0,0,2},
            {D3D12_DESCRIPTOR_RANGE_TYPE_SRV,1,1,0,D3D12_DESCRIPTOR_RANGE_OFFSET_APPEND},
            {D3D12_DESCRIPTOR_RANGE_TYPE_UAV,2,0,0,5},{D3D12_DESCRIPTOR_RANGE_TYPE_SRV,UINT_MAX,10,1,8}};
        params[33].DescriptorTable={UINT(std::size(mixed)),mixed};params[33].ShaderVisibility=D3D12_SHADER_VISIBILITY_PIXEL;
        const D3D12_ROOT_SIGNATURE_DESC layout{UINT(params.size()),params.data(),0,nullptr,D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT};
        ComPtr<ID3DBlob> error;Check(D3D12SerializeRootSignature(&layout,D3D_ROOT_SIGNATURE_VERSION_1,&rootBytes,&error));
        Check(device->CreateRootSignature(0,rootBytes->GetBufferPointer(),rootBytes->GetBufferSize(),IID_PPV_ARGS(&root)));
        audit::RootCreated(root.Get(),rootBytes->GetBufferPointer(),rootBytes->GetBufferSize());
        auto heap=[&](D3D12_DESCRIPTOR_HEAP_TYPE type,UINT count,D3D12_DESCRIPTOR_HEAP_FLAGS flags,ComPtr<ID3D12DescriptorHeap>& result){
            const D3D12_DESCRIPTOR_HEAP_DESC desc{type,count,flags,0};Check(device->CreateDescriptorHeap(&desc,IID_PPV_ARGS(&result)));
            if(type!=D3D12_DESCRIPTOR_HEAP_TYPE_RTV)audit::HeapCreated(device,result.Get());};
        heap(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,TableCount,D3D12_DESCRIPTOR_HEAP_FLAG_NONE,cpu);
        heap(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,64,D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,visible);
        heap(D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER,2048,D3D12_DESCRIPTOR_HEAP_FLAG_NONE,samplerCpu);
        heap(D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER,2048,D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,samplerVisible);
        heap(D3D12_DESCRIPTOR_HEAP_TYPE_RTV,3,D3D12_DESCRIPTOR_HEAP_FLAG_NONE,rtvs);
        constants=Buffer(device,512,D3D12_HEAP_TYPE_UPLOAD,D3D12_RESOURCE_FLAG_NONE,D3D12_RESOURCE_STATE_GENERIC_READ);WriteUInt(constants.Get(),6);
        upload=Buffer(device,4,D3D12_HEAP_TYPE_UPLOAD,D3D12_RESOURCE_FLAG_NONE,D3D12_RESOURCE_STATE_GENERIC_READ);WriteUInt(upload.Get(),2);
        zero=Buffer(device,4,D3D12_HEAP_TYPE_UPLOAD,D3D12_RESOURCE_FLAG_NONE,D3D12_RESOURCE_STATE_GENERIC_READ);WriteUInt(zero.Get(),0);
        unordered=Buffer(device,4,D3D12_HEAP_TYPE_DEFAULT,D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,D3D12_RESOURCE_STATE_COPY_DEST);
        counter=Buffer(device,4096,D3D12_HEAP_TYPE_DEFAULT,D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,D3D12_RESOURCE_STATE_COPY_DEST);
        D3D12_HEAP_PROPERTIES memory{};memory.Type=D3D12_HEAP_TYPE_DEFAULT;
        D3D12_RESOURCE_DESC image{};image.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D;image.Width=image.Height=1;
        image.DepthOrArraySize=image.MipLevels=image.SampleDesc.Count=1;image.Format=DXGI_FORMAT_R32G32B32A32_FLOAT;image.Flags=D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
        Check(device->CreateCommittedResource(&memory,D3D12_HEAP_FLAG_NONE,&image,D3D12_RESOURCE_STATE_RENDER_TARGET,nullptr,IID_PPV_ARGS(&texture)));
        Check(device->CreateCommittedResource(&memory,D3D12_HEAP_FLAG_NONE,&image,D3D12_RESOURCE_STATE_RENDER_TARGET,nullptr,IID_PPV_ARGS(&extra)));
        targetDesc=image;targetDesc.Width=3;targetDesc.Format=DXGI_FORMAT_R32G32B32A32_UINT;
        Check(device->CreateCommittedResource(&memory,D3D12_HEAP_FLAG_NONE,&targetDesc,D3D12_RESOURCE_STATE_RENDER_TARGET,nullptr,IID_PPV_ARGS(&target)));
        device->CreateRenderTargetView(texture.Get(),nullptr,Rtv(0));device->CreateRenderTargetView(extra.Get(),nullptr,Rtv(1));device->CreateRenderTargetView(target.Get(),nullptr,Rtv(2));
        srv.Format=image.Format;srv.ViewDimension=D3D12_SRV_DIMENSION_TEXTURE2D;srv.Texture2D.MipLevels=1;srv.Shader4ComponentMapping=D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;
        uav.ViewDimension=D3D12_UAV_DIMENSION_BUFFER;uav.Buffer.NumElements=1;uav.Buffer.StructureByteStride=4;
        cbv={constants->GetGPUVirtualAddress(),256};device->CreateConstantBufferView(&cbv,Cpu(0));audit::Cbv(&cbv,Cpu(0));
        for(UINT slot:{1u,3u,4u,7u,9u})device->CreateShaderResourceView(nullptr,&srv,Cpu(slot));
        device->CreateShaderResourceView(texture.Get(),nullptr,Cpu(2));audit::Srv(texture.Get(),nullptr,Cpu(2));
        audit::Srv(nullptr,&srv,Cpu(3));device->CreateUnorderedAccessView(unordered.Get(),counter.Get(),&uav,Cpu(5));audit::Uav(unordered.Get(),counter.Get(),&uav,Cpu(5));
        device->CreateUnorderedAccessView(nullptr,nullptr,&uav,Cpu(6));audit::Uav(nullptr,nullptr,&uav,Cpu(6));
        device->CreateShaderResourceView(extra.Get(),&srv,Cpu(8));audit::Srv(extra.Get(),&srv,Cpu(8));
        CopyResources();
        sampler.Filter=D3D12_FILTER_MIN_MAG_MIP_POINT;sampler.AddressU=sampler.AddressV=sampler.AddressW=D3D12_TEXTURE_ADDRESS_MODE_CLAMP;
        sampler.MaxAnisotropy=1;sampler.ComparisonFunc=D3D12_COMPARISON_FUNC_ALWAYS;sampler.MaxLOD=D3D12_FLOAT32_MAX;
        for(UINT slot=0;slot<3;++slot)device->CreateSampler(&sampler,SamplerCpu(slot));
        audit::Sampler(&sampler,SamplerCpu(1));CopySamplers();
        Check(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,IID_PPV_ARGS(&allocator)));
        Check(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,IID_PPV_ARGS(&next)));
        Check(device->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_DIRECT,allocator.Get(),nullptr,IID_PPV_ARGS(&list)));
        requests={audit::SourceTableRequest{33,TableCount,resourceUses},audit::SourceTableRequest{0,3,samplerUses}};
        Require(audit::BeginBindings(),"Audit binding session refused fixture");BindOriginal();
        const char shader[]=R"(
cbuffer Constants:register(b0){uint value;};
Texture2D<float4> source:register(t0);Texture2D<float4> missing:register(t1);
Texture2D<float4> extraSource:register(t10,space1);SamplerState pointSampler:register(s0);
RWStructuredBuffer<uint> data:register(u0);RWStructuredBuffer<uint> missingData:register(u1);
float4 VS(uint id:SV_VertexID):SV_Position{return float4(id==2?3:-1,id==1?3:-1,0,1);}
uint4 PS():SV_Target{uint sampleValue=(uint)(source.SampleLevel(pointSampler,float2(2,2),0).r*4)+
 (uint)(extraSource.Load(int3(0,0,0)).r*8);
 uint nullValue=(uint)missing.Load(int3(0,0,0)).r+missingData[0];
 uint count=data.IncrementCounter();return uint4(value+sampleValue+data[0]+nullValue,value,sampleValue,count);})";
        ComPtr<ID3DBlob> vs,ps;Check(D3DCompile(shader,std::strlen(shader),nullptr,nullptr,nullptr,"VS","vs_5_1",D3DCOMPILE_OPTIMIZATION_LEVEL3,0,&vs,&error));
        Check(D3DCompile(shader,std::strlen(shader),nullptr,nullptr,nullptr,"PS","ps_5_1",D3DCOMPILE_OPTIMIZATION_LEVEL3,0,&ps,&error));
        D3D12_GRAPHICS_PIPELINE_STATE_DESC desc{};desc.pRootSignature=root.Get();desc.VS={vs->GetBufferPointer(),vs->GetBufferSize()};desc.PS={ps->GetBufferPointer(),ps->GetBufferSize()};
        desc.PrimitiveTopologyType=D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE;desc.NumRenderTargets=1;desc.RTVFormats[0]=targetDesc.Format;desc.SampleDesc.Count=1;desc.SampleMask=UINT_MAX;
        desc.RasterizerState.FillMode=D3D12_FILL_MODE_SOLID;desc.RasterizerState.CullMode=D3D12_CULL_MODE_NONE;desc.RasterizerState.DepthClipEnable=TRUE;
        auto& blend=desc.BlendState.RenderTarget[0];blend.SrcBlend=blend.SrcBlendAlpha=D3D12_BLEND_ONE;blend.DestBlend=blend.DestBlendAlpha=D3D12_BLEND_ZERO;
        blend.BlendOp=blend.BlendOpAlpha=D3D12_BLEND_OP_ADD;blend.LogicOp=D3D12_LOGIC_OP_NOOP;blend.RenderTargetWriteMask=15;
        Check(device->CreateGraphicsPipelineState(&desc,IID_PPV_ARGS(&pipeline)));
    }
    void CopyResources(){auto from=Cpu(0),to=VisibleCpu(0);UINT count=TableCount;
        device->CopyDescriptorsSimple(count,to,from,D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);audit::Copy(device,1,&to,&count,1,&from,&count,D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);}
    void CopySamplers(){auto from=SamplerCpu(0),to=SamplerVisibleCpu(0);UINT count=3;
        device->CopyDescriptorsSimple(count,to,from,D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER);audit::Copy(device,1,&to,&count,1,&from,&count,D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER);}
    void BindOriginal(){ID3D12DescriptorHeap* heaps[]{visible.Get(),samplerVisible.Get()};list->SetDescriptorHeaps(2,heaps);audit::HeapsBound(device,2,heaps);
        Require(cvr::gpu::KeepCommandResources(list.Get(),{visible.Get(),samplerVisible.Get()}),"Original recorded descriptor heaps lacked caller lifetime");
        list->SetGraphicsRootSignature(root.Get());audit::Root(list.Get(),root.Get());
        list->SetGraphicsRootDescriptorTable(33,VisibleGpu());audit::Table(list.Get(),33,VisibleGpu());
        const auto samplerGpu=samplerVisible->GetGPUDescriptorHandleForHeapStart();list->SetGraphicsRootDescriptorTable(0,samplerGpu);audit::Table(list.Get(),0,samplerGpu);
        bindings={};bindings.root=root.Get();bindings.tableMask=(uint64_t(1)<<33)|1;bindings.tables[33]=VisibleGpu();bindings.tables[0]=samplerGpu;
        bindings.heaps={visible.Get(),samplerVisible.Get()};bindings.heapCount=2;bindings.heapsSeen=true;bindings.resetSeen=true;}
    void ResetCounter(){if(counterState!=D3D12_RESOURCE_STATE_COPY_DEST)Transition(list.Get(),counter.Get(),counterState,D3D12_RESOURCE_STATE_COPY_DEST);
        list->CopyBufferRegion(counter.Get(),0,zero.Get(),0,4);Transition(list.Get(),counter.Get(),D3D12_RESOURCE_STATE_COPY_DEST,D3D12_RESOURCE_STATE_UNORDERED_ACCESS);
        counterState=D3D12_RESOURCE_STATE_UNORDERED_ACCESS;}
    void Draw(UINT pixel){const auto handle=Rtv(2);list->SetPipelineState(pipeline.Get());list->OMSetRenderTargets(1,&handle,FALSE,nullptr);
        list->IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);D3D12_VIEWPORT viewport{float(pixel),0,1,1,0,1};D3D12_RECT scissor{LONG(pixel),0,LONG(pixel+1),1};
        list->RSSetViewports(1,&viewport);list->RSSetScissorRects(1,&scissor);list->DrawInstanced(3,1,0,0);}
    void Overwrite(){for(UINT slot:{2u,3u,8u}){device->CreateShaderResourceView(nullptr,&srv,Cpu(slot));audit::Srv(nullptr,&srv,Cpu(slot));}
        device->CreateUnorderedAccessView(nullptr,nullptr,&uav,Cpu(5));audit::Uav(nullptr,nullptr,&uav,Cpu(5));
        sampler.AddressU=sampler.AddressV=sampler.AddressW=D3D12_TEXTURE_ADDRESS_MODE_BORDER;sampler.BorderColor[0]=.75f;
        device->CreateSampler(&sampler,SamplerCpu(1));audit::Sampler(&sampler,SamplerCpu(1));CopyResources();CopySamplers();}
};

static std::unique_ptr<StereoBindingBank> MakeBank(Fixture& fixture,const std::vector<audit::OwnedSourceTable>& captured) {
    std::vector<SharedBankTable> tables;for(const auto& table:captured){SharedBankTable value;value.parameter=table.parameter;value.count=table.count;
        value.cpuHeap=table.cpuHeap;value.leases={table.owner};value.identity=table.identity;value.version=table.version;tables.push_back(std::move(value));}
    const SourceBankDraw draw{fixture.root,fixture.RootData(),tables};auto bank=std::make_unique<StereoBindingBank>();Check(bank->CreateSourcePass(fixture.device,{&draw,1}));
    Require(bank->DrawCount()==1 && bank->DescriptorCount()==Fixture::TableCount && !bank->ConstantBytes() && !bank->SnapshotCount(),"Source bank implied input content freezing");return bank;
}

static void CapturedSrvProvenance(Fixture& fixture,Provider& provider,audit::SourceTableCache& cache,
    const std::vector<audit::OwnedSourceTable>& captured){
    audit::SrvBinding original;Require(audit::ResolveSrvAtDraw(fixture.list.Get(),reinterpret_cast<uintptr_t>(fixture.root.Get()),33,8,original),
        "Current explicit SRV did not resolve at the actual fixture binding");
    Require(captured[0].provenance && audit::MatchesCapturedSourceSrv(captured[0],8,original),"Actual finite capture did not retain immutable SRV provenance");
    Require(!audit::MatchesCapturedSourceSrv(captured[0],2,original) && !audit::MatchesCapturedSourceSrv(captured[0],Fixture::TableCount,original) &&
        !audit::MatchesCapturedSourceSrv(captured[1],1,original),"Another captured table/slot admitted this SRV");
    for(UINT field=0;field<9;++field){auto changed=captured[0];
        switch(field){case 0:++changed.parameter;break;case 1:++changed.count;break;case 2:++changed.identity;break;case 3:++changed.version;break;
        case 4:changed.sourceGpu+=fixture.stride;break;case 5:++changed.sourceHeap;break;case 6:changed.cpuHeap=fixture.cpu;break;
        case 7:changed.provenance.Reset();break;case 8:changed.provenance=captured[1].provenance;break;}
        Require(!audit::MatchesCapturedSourceSrv(changed,8,original),"Mutated labels/missing/foreign receipt minted finite SRV provenance");}
    std::atomic<UINT> fakeDestroyed{};ComPtr<IUnknown> fake;fake.Attach(new Sentinel(fakeDestroyed));
    auto forged=captured[0];forged.owner=fake;Require(!audit::MatchesCapturedSourceSrv(forged,8,original),"Fake owner admitted a captured SRV");
    forged=captured[0];forged.provenance=fake;Require(!audit::MatchesCapturedSourceSrv(forged,8,original),"Fake receipt admitted a captured SRV");
    auto swapped=original;swapped.resource=fixture.texture;
    Require(!audit::MatchesCapturedSourceSrv(captured[0],8,swapped),"Another resource already held at another slot inherited this cookie");
    swapped=original;swapped.view.Shader4ComponentMapping=D3D12_ENCODE_SHADER_4_COMPONENT_MAPPING(1,0,2,3);
    Require(!audit::MatchesCapturedSourceSrv(captured[0],8,swapped),"Typed view mutation retained canonical SRV provenance");
    for(UINT field=0;field<3;++field){auto rewritten=original;
        if(field==0)rewritten.description[6]=D3D12_ENCODE_SHADER_4_COMPONENT_MAPPING(1,0,2,3);
        if(field==1)rewritten.stamp.value[6]=D3D12_ENCODE_SHADER_4_COMPONENT_MAPPING(1,0,2,3);
        if(field==2)rewritten.stamp.gpu+=fixture.stride;
        Require(!audit::MatchesCapturedSourceSrv(captured[0],8,rewritten),"Canonical/stamp rewrite matched the earlier captured CPU slot");}
    auto view=fixture.srv;view.Shader4ComponentMapping=D3D12_ENCODE_SHADER_4_COMPONENT_MAPPING(1,0,2,3);
    fixture.device->CreateShaderResourceView(fixture.extra.Get(),&view,fixture.VisibleCpu(8));audit::Srv(fixture.extra.Get(),&view,fixture.VisibleCpu(8));
    audit::SrvBinding later;Require(audit::ResolveSrvAtDraw(fixture.list.Get(),reinterpret_cast<uintptr_t>(fixture.root.Get()),33,8,later) &&
        audit::MatchesDescriptor(later.stamp) && !audit::MatchesDescriptor(original.stamp),"Later rewritten SRV did not establish the intended live mismatch");
    Require(!audit::MatchesCapturedSourceSrv(captured[0],8,later) && audit::MatchesCapturedSourceSrv(captured[0],8,original),
        "Later live equality replaced the earlier immutable CPU descriptor version");
    audit::SourceTableCache temporary;std::vector<audit::OwnedSourceTable> laterCapture;
    Require(audit::CaptureSourceTablesAtDraw(fixture.device,fixture.list.Get(),fixture.bindings,fixture.requests,Provider::Resolve,&provider,temporary,laterCapture) &&
        audit::MatchesCapturedSourceSrv(laterCapture[0],8,later),"Genuine rewritten capture did not match its own immutable version");
    auto retained=laterCapture[0];temporary.Clear();laterCapture.clear();
    Require(audit::MatchesCapturedSourceSrv(retained,8,later),"Held capture receipt depended on mutable cache/history lookup");
    fixture.device->CreateShaderResourceView(fixture.texture.Get(),&fixture.srv,fixture.VisibleCpu(8));audit::Srv(fixture.texture.Get(),&fixture.srv,fixture.VisibleCpu(8));
    audit::SrvBinding otherResource;Require(audit::ResolveSrvAtDraw(fixture.list.Get(),reinterpret_cast<uintptr_t>(fixture.root.Get()),33,8,otherResource) &&
        audit::MatchesDescriptor(otherResource.stamp) && !audit::MatchesCapturedSourceSrv(captured[0],8,otherResource),
        "Later physical resource rewrite passed an earlier finite capture receipt");
    fixture.device->CreateShaderResourceView(fixture.extra.Get(),&fixture.srv,fixture.VisibleCpu(8));audit::Srv(fixture.extra.Get(),&fixture.srv,fixture.VisibleCpu(8));
    constexpr UINT otherBase=16;auto from=fixture.Cpu(0);
    const D3D12_CPU_DESCRIPTOR_HANDLE to{fixture.visible->GetCPUDescriptorHandleForHeapStart().ptr+SIZE_T(otherBase)*fixture.stride};UINT count=Fixture::TableCount;
    fixture.device->CopyDescriptorsSimple(count,to,from,D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
    audit::Copy(fixture.device,1,&to,&count,1,&from,&count,D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
    auto domain=fixture.bindings;domain.tables[33].ptr=fixture.visible->GetGPUDescriptorHandleForHeapStart().ptr+UINT64(otherBase)*fixture.stride;
    fixture.list->SetGraphicsRootDescriptorTable(33,domain.tables[33]);audit::Table(fixture.list.Get(),33,domain.tables[33]);
    std::vector<audit::OwnedSourceTable> otherCapture;
    Require(audit::CaptureSourceTablesAtDraw(fixture.device,fixture.list.Get(),domain,fixture.requests,Provider::Resolve,&provider,cache,otherCapture),"Another actual table domain did not capture");
    Require(otherCapture[0].owner==captured[0].owner && otherCapture[0].cpuHeap==captured[0].cpuHeap && otherCapture[0].identity==captured[0].identity &&
        otherCapture[0].sourceGpu!=captured[0].sourceGpu && otherCapture[0].provenance!=captured[0].provenance,"Per-capture provenance broke physical storage interning");
    audit::SrvBinding otherBinding;Require(audit::ResolveSrvAtDraw(fixture.list.Get(),reinterpret_cast<uintptr_t>(fixture.root.Get()),33,8,otherBinding) &&
        audit::MatchesCapturedSourceSrv(otherCapture[0],8,otherBinding) && !audit::MatchesCapturedSourceSrv(otherCapture[0],8,original) &&
        !audit::MatchesCapturedSourceSrv(captured[0],8,otherBinding),"Same interned storage borrowed another original GPU table domain");
    forged=otherCapture[0];forged.sourceGpu=captured[0].sourceGpu;
    Require(!audit::MatchesCapturedSourceSrv(forged,8,original),"Changed domain labels revived an earlier capture");
    forged=otherCapture[0];forged.provenance=captured[0].provenance;
    Require(!audit::MatchesCapturedSourceSrv(forged,8,otherBinding),"Another actual capture receipt certified different domain labels");
    fixture.BindOriginal();Require(audit::MatchesCapturedSourceSrv(captured[0],8,original),"Restored live binding affected immutable provenance");
}

static void Negatives(Fixture& fixture,Provider& provider,audit::SourceTableCache& cache) {
    const UINT tables=cache.TableCount(),descriptors=cache.DescriptorCount();
    auto reject=[&](ID3D12Device* device,ID3D12GraphicsCommandList* list,const scene_state::Bindings& bindings,
                    std::span<const audit::SourceTableRequest> requests,audit::SourceCbvProvider callback,void* user,UINT reason){
        std::vector<audit::OwnedSourceTable> output;Require(!audit::CaptureSourceTablesAtDraw(device,list,bindings,requests,callback,user,cache,output),"Invalid source-table capture accepted");
        Require(output.empty() && cache.TableCount()==tables && cache.DescriptorCount()==descriptors,"Failed source capture changed cache/output");
        Require(audit::LastSourceTableFailure().reason==reason,"Unexpected source capture failure reason");};
    const auto valid=fixture.bindings;
    reject(nullptr,fixture.list.Get(),valid,fixture.requests,Provider::Resolve,&provider,1);
    reject(fixture.device,nullptr,valid,fixture.requests,Provider::Resolve,&provider,1);
    auto changed=valid;changed.root=nullptr;reject(fixture.device,fixture.list.Get(),changed,fixture.requests,Provider::Resolve,&provider,1);
    changed=valid;changed.heapsSeen=false;reject(fixture.device,fixture.list.Get(),changed,fixture.requests,Provider::Resolve,&provider,1);
    changed=valid;changed.heapCount=3;reject(fixture.device,fixture.list.Get(),changed,fixture.requests,Provider::Resolve,&provider,1);
    changed=valid;changed.tableMask&=~(uint64_t(1)<<33);reject(fixture.device,fixture.list.Get(),changed,fixture.requests,Provider::Resolve,&provider,4);
    changed=valid;changed.tables[33].ptr+=fixture.stride;reject(fixture.device,fixture.list.Get(),changed,fixture.requests,Provider::Resolve,&provider,2);
    changed=valid;changed.heaps[0]=fixture.cpu.Get();reject(fixture.device,fixture.list.Get(),changed,fixture.requests,Provider::Resolve,&provider,5);
    for(UINT variant=0;variant<5;++variant){auto requests=fixture.requests;
        if(variant==0)requests[0].parameter=64;if(variant==1)requests[0].count=0;if(variant==2)requests[0].count=8;
        if(variant==3)requests[0].count=UINT_MAX;if(variant==4)requests[1].parameter=33;
        reject(fixture.device,fixture.list.Get(),valid,requests,Provider::Resolve,&provider,variant==0?2:4);}
    {auto requests=fixture.requests;requests[0].uses={};reject(fixture.device,fixture.list.Get(),valid,requests,Provider::Resolve,&provider,4);}
    {auto requests=fixture.requests;requests[1].count=2049;reject(fixture.device,fixture.list.Get(),valid,requests,Provider::Resolve,&provider,4);}
    for(UINT variant=0;variant<5;++variant){auto uses=fixture.resourceUses;
        if(variant==0)uses[0].count=0;if(variant==1)uses[0].type=D3D12_DESCRIPTOR_RANGE_TYPE_SRV;
        if(variant==2)uses[0].offset=UINT_MAX;if(variant==3)uses[0].count=UINT_MAX;if(variant==4)uses[3].offset=9;
        auto requests=fixture.requests;requests[0].uses=uses;
        reject(fixture.device,fixture.list.Get(),valid,requests,Provider::Resolve,&provider,variant==4?5:4);}
    reject(fixture.device,fixture.list.Get(),valid,fixture.requests,nullptr,nullptr,8);
    provider.wrong=Buffer(fixture.device,256,D3D12_HEAP_TYPE_UPLOAD,D3D12_RESOURCE_FLAG_NONE,D3D12_RESOURCE_STATE_GENERIC_READ);
    provider.wrongBacking=true;reject(fixture.device,fixture.list.Get(),valid,fixture.requests,Provider::Resolve,&provider,8);provider.wrongBacking=false;
    provider.mutate=true;provider.mutationSlot=fixture.VisibleCpu(0);provider.mutationView=fixture.cbv;provider.mutationView.BufferLocation+=256;
    reject(fixture.device,fixture.list.Get(),valid,fixture.requests,Provider::Resolve,&provider,9);provider.FinishMutation();
    Require(!provider.workerTimedOut,"CBV provider ran while holding metadata lock");provider.mutate=false;audit::Cbv(&fixture.cbv,fixture.VisibleCpu(0));
    auto overflowBindings=valid;overflowBindings.tables[33].ptr=fixture.visible->GetGPUDescriptorHandleForHeapStart().ptr+UINT64(63)*fixture.stride;
    audit::Table(fixture.list.Get(),33,overflowBindings.tables[33]);reject(fixture.device,fixture.list.Get(),overflowBindings,fixture.requests,Provider::Resolve,&provider,5);fixture.BindOriginal();
    ComPtr<ID3D12CommandAllocator> allocator;ComPtr<ID3D12GraphicsCommandList> other;
    Check(fixture.device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,IID_PPV_ARGS(&allocator)));
    Check(fixture.device->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_DIRECT,allocator.Get(),nullptr,IID_PPV_ARGS(&other)));
    reject(fixture.device,other.Get(),valid,fixture.requests,Provider::Resolve,&provider,2);Check(other->Close());
    ComPtr<ID3D12CommandAllocator> computeAllocator;ComPtr<ID3D12GraphicsCommandList> compute;
    Check(fixture.device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_COMPUTE,IID_PPV_ARGS(&computeAllocator)));
    Check(fixture.device->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_COMPUTE,computeAllocator.Get(),nullptr,IID_PPV_ARGS(&compute)));
    reject(fixture.device,compute.Get(),valid,fixture.requests,Provider::Resolve,&provider,1);Check(compute->Close());
    ComPtr<ID3D12RootSignature> untracked;
    ComPtr<ID3D12RootSignatureDeserializer> reader;ComPtr<ID3DBlob> untrackedBytes,error;
    Check(D3D12CreateRootSignatureDeserializer(fixture.rootBytes->GetBufferPointer(),fixture.rootBytes->GetBufferSize(),IID_PPV_ARGS(&reader)));
    auto untrackedLayout=*reader->GetRootSignatureDesc();untrackedLayout.Flags|=D3D12_ROOT_SIGNATURE_FLAG_DENY_HULL_SHADER_ROOT_ACCESS;
    Check(D3D12SerializeRootSignature(&untrackedLayout,D3D_ROOT_SIGNATURE_VERSION_1,&untrackedBytes,&error));
    Check(fixture.device->CreateRootSignature(0,untrackedBytes->GetBufferPointer(),untrackedBytes->GetBufferSize(),IID_PPV_ARGS(&untracked)));
    changed=valid;changed.root=untracked.Get();audit::Root(fixture.list.Get(),untracked.Get());
    audit::Table(fixture.list.Get(),33,valid.tables[33]);audit::Table(fixture.list.Get(),0,valid.tables[0]);
    reject(fixture.device,fixture.list.Get(),changed,fixture.requests,Provider::Resolve,&provider,3);fixture.BindOriginal();
    {
        D3D12_ROOT_PARAMETER constant{};constant.ParameterType=D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS;constant.Constants={0,0,1};
        const D3D12_ROOT_SIGNATURE_DESC layout{1,&constant,0,nullptr,D3D12_ROOT_SIGNATURE_FLAG_NONE};ComPtr<ID3DBlob> bytes,error;
        Check(D3D12SerializeRootSignature(&layout,D3D_ROOT_SIGNATURE_VERSION_1,&bytes,&error));ComPtr<ID3D12RootSignature> unsupported;
        Check(fixture.device->CreateRootSignature(0,bytes->GetBufferPointer(),bytes->GetBufferSize(),IID_PPV_ARGS(&unsupported)));
        audit::RootCreated(unsupported.Get(),bytes->GetBufferPointer(),bytes->GetBufferSize());
        changed=valid;changed.root=unsupported.Get();audit::Root(fixture.list.Get(),unsupported.Get());audit::Table(fixture.list.Get(),0,valid.tables[0]);
        reject(fixture.device,fixture.list.Get(),changed,{fixture.requests.data()+1,1},nullptr,nullptr,3);fixture.BindOriginal();
    }
    // D3D12CreateDevice is a singleton per adapter. A second WARP call may be
    // the SAME device, so foreign-device negatives need a different adapter.
    // No command queue or GPU work is created on that foreign device.
    ComPtr<IDXGIFactory4> factory;Check(CreateDXGIFactory1(IID_PPV_ARGS(&factory)));ComPtr<ID3D12Device> second;
    for(UINT index=0;!second;++index){ComPtr<IDXGIAdapter1> adapter;if(factory->EnumAdapters1(index,&adapter)==DXGI_ERROR_NOT_FOUND)break;
        DXGI_ADAPTER_DESC1 desc{};Check(adapter->GetDesc1(&desc));if(desc.Flags&DXGI_ADAPTER_FLAG_SOFTWARE)continue;
        ComPtr<ID3D12Device> candidate;if(SUCCEEDED(D3D12CreateDevice(adapter.Get(),D3D_FEATURE_LEVEL_11_0,IID_PPV_ARGS(&candidate))) && candidate.Get()!=fixture.device)second=std::move(candidate);}
    if(second){
        reject(second.Get(),fixture.list.Get(),valid,fixture.requests,Provider::Resolve,&provider,1);
        provider.wrong=Buffer(second.Get(),256,D3D12_HEAP_TYPE_UPLOAD,D3D12_RESOURCE_FLAG_NONE,D3D12_RESOURCE_STATE_GENERIC_READ);
        provider.wrongBacking=true;reject(fixture.device,fixture.list.Get(),valid,fixture.requests,Provider::Resolve,&provider,8);provider.wrongBacking=false;provider.wrong.Reset();
        ComPtr<ID3D12RootSignature> root;ComPtr<ID3D12CommandAllocator> owner;ComPtr<ID3D12GraphicsCommandList> commands;
        Check(second->CreateRootSignature(0,fixture.rootBytes->GetBufferPointer(),fixture.rootBytes->GetBufferSize(),IID_PPV_ARGS(&root)));
        Check(second->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,IID_PPV_ARGS(&owner)));
        Check(second->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_DIRECT,owner.Get(),nullptr,IID_PPV_ARGS(&commands)));
        scene_state::Bindings bindings;bindings.root=root.Get();bindings.heapsSeen=true;
        reject(second.Get(),commands.Get(),bindings,{},nullptr,nullptr,10);Check(commands->Close());
    }else std::puts("SKIP foreign-device negatives: no second D3D12-capable adapter; WARP may reuse its singleton");
    {
        std::vector<audit::OwnedSourceTable> output;Require(audit::CaptureSourceTablesAtDraw(fixture.device,fixture.list.Get(),valid,fixture.requests,Provider::Resolve,&provider,cache,output),"Valid source capture failed after rejected requests");
        const auto owner=output[0].owner.Get();Require(!audit::CaptureSourceTablesAtDraw(fixture.device,fixture.list.Get(),valid,fixture.requests,Provider::Resolve,&provider,cache,output) &&
            output[0].owner.Get()==owner && cache.TableCount()==tables && cache.DescriptorCount()==descriptors,"Nonempty output overwritten on capture failure");
    }
    {
        audit::SourceTableCache budget;std::vector<audit::OwnedSourceTable> first,failed;
        const audit::SourceTableRequest request{0,1025,fixture.samplerUses};
        Require(audit::CaptureSourceTablesAtDraw(fixture.device,fixture.list.Get(),valid,{&request,1},nullptr,nullptr,budget,first),"Admitted finite sampler extent failed");
        auto sampler=fixture.sampler;sampler.MipLODBias=.5f;audit::Sampler(&sampler,fixture.SamplerVisibleCpu(1));
        Require(!audit::CaptureSourceTablesAtDraw(fixture.device,fixture.list.Get(),valid,{&request,1},nullptr,nullptr,budget,failed) &&
            audit::LastSourceTableFailure().reason==12 && failed.empty() && budget.TableCount()==1 && budget.DescriptorCount()==1025,"Sampler cache budget failure was not transactional");
        audit::Sampler(&fixture.sampler,fixture.SamplerVisibleCpu(1));budget.Clear();Require(!budget.TableCount() && !budget.DescriptorCount(),"Cache Clear retained accounting");
    }
}

int main()try {
    ComPtr<ID3D12Debug> debug;Check(D3D12GetDebugInterface(IID_PPV_ARGS(&debug)));debug->EnableDebugLayer();
    ComPtr<IDXGIFactory4> factory;Check(CreateDXGIFactory1(IID_PPV_ARGS(&factory)));ComPtr<IDXGIAdapter> adapter;Check(factory->EnumWarpAdapter(IID_PPV_ARGS(&adapter)));
    ComPtr<ID3D12Device> device;Check(D3D12CreateDevice(adapter.Get(),D3D_FEATURE_LEVEL_12_0,IID_PPV_ARGS(&device)));audit::Initialize(device.Get());
    std::atomic<UINT> destroyed{},resourcesDestroyed{};
    Fixture fixture(device.Get());Provider provider;provider.resource=fixture.constants;provider.owner.Attach(new Sentinel(destroyed));
    constexpr GUID lifetimeGuid{0x7f58f014,0xe1d2,0x4df0,{0x91,0xc7,0x16,0xba,0x66,0xa0,0x7f,0x22}};
    for(auto* resource:{fixture.constants.Get(),fixture.texture.Get(),fixture.extra.Get(),fixture.unordered.Get(),fixture.counter.Get()}){
        ComPtr<IUnknown> lifetime;lifetime.Attach(new Sentinel(resourcesDestroyed));Check(resource->SetPrivateDataInterface(lifetimeGuid,lifetime.Get()));}
    audit::SourceTableCache cache;std::vector<audit::OwnedSourceTable> captured,repeated;
    std::vector<audit::SourceResourceBinding> dependencies;
    Require(audit::CaptureSourceTablesAtDraw(device.Get(),fixture.list.Get(),fixture.bindings,fixture.requests,Provider::Resolve,&provider,cache,captured,&dependencies),"Initial mixed source capture failed");
    Require(dependencies.size()==5 && dependencies[0].parameter==33 && dependencies[0].offset==2 && dependencies[0].resource==fixture.texture &&
        dependencies[0].type==D3D12_DESCRIPTOR_RANGE_TYPE_SRV && dependencies[1].offset==3 && !dependencies[1].resource &&
        dependencies[2].offset==5 && dependencies[2].resource==fixture.unordered && dependencies[2].counter==fixture.counter &&
        dependencies[2].type==D3D12_DESCRIPTOR_RANGE_TYPE_UAV && dependencies[3].offset==6 && !dependencies[3].resource &&
        dependencies[4].offset==8 && dependencies[4].resource==fixture.extra,"Original SRV/UAV dependencies lost offsets, live resources or typed nulls");
    for(const auto& dependency:dependencies)Require(dependency.stamp.gpu==captured[0].sourceGpu+UINT64(dependency.offset)*fixture.stride &&
        dependency.stamp.heap==captured[0].sourceHeap,"Dependency did not retain its actual source stamp");
    {std::vector<audit::OwnedSourceTable> untouched;
        Require(!audit::CaptureSourceTablesAtDraw(device.Get(),fixture.list.Get(),fixture.bindings,fixture.requests,Provider::Resolve,&provider,cache,untouched,&dependencies) &&
            untouched.empty() && dependencies.size()==5 && dependencies[2].counter==fixture.counter,"Nonempty dependency output was overwritten");}
    dependencies.clear();
    Require(captured.size()==2 && cache.TableCount()==2 && cache.DescriptorCount()==13,"Initial cache counts incorrect");
    for(const auto& table:captured)Require(table.cpuHeap && table.owner && table.provenance && table.identity && table.version && table.count &&
        table.cpuHeap->GetDesc().Flags==D3D12_DESCRIPTOR_HEAP_FLAG_NONE,"Owned source table was not immutable CPU-only storage");
    Require(audit::CaptureSourceTablesAtDraw(device.Get(),fixture.list.Get(),fixture.bindings,fixture.requests,Provider::Resolve,&provider,cache,repeated),"Repeated source capture failed");
    for(UINT n=0;n<captured.size();++n)Require(captured[n].identity==repeated[n].identity && captured[n].version==repeated[n].version &&
        captured[n].cpuHeap==repeated[n].cpuHeap && captured[n].owner==repeated[n].owner && captured[n].provenance!=repeated[n].provenance,
        "Stable source descriptors did not reuse owned CPU storage with separate actual capture receipts");
    CapturedSrvProvenance(fixture,provider,cache,captured);
    Negatives(fixture,provider,cache);auto bank=MakeBank(fixture,captured);
    ComPtr<ID3D12CommandQueue> queue;const D3D12_COMMAND_QUEUE_DESC queueDesc{};Check(device->CreateCommandQueue(&queueDesc,IID_PPV_ARGS(&queue)));
    ComPtr<ID3D12Fence> complete,gate;Check(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&complete)));Check(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&gate)));
    const float firstColor[]{.25f,0,0,1},secondColor[]{.5f,0,0,1};fixture.list->ClearRenderTargetView(fixture.Rtv(0),firstColor,0,nullptr);
    fixture.list->ClearRenderTargetView(fixture.Rtv(1),secondColor,0,nullptr);
    Transition(fixture.list.Get(),fixture.texture.Get(),D3D12_RESOURCE_STATE_RENDER_TARGET,D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE);
    Transition(fixture.list.Get(),fixture.extra.Get(),D3D12_RESOURCE_STATE_RENDER_TARGET,D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE);
    fixture.list->CopyBufferRegion(fixture.unordered.Get(),0,fixture.upload.Get(),0,4);
    Transition(fixture.list.Get(),fixture.unordered.Get(),D3D12_RESOURCE_STATE_COPY_DEST,D3D12_RESOURCE_STATE_UNORDERED_ACCESS);
    // Descriptor ownership intentionally does not snapshot the CBV contents.
    WriteUInt(fixture.constants.Get(),11);fixture.BindOriginal();fixture.ResetCounter();fixture.Draw(0);
    Require(bank->BindSource(fixture.list.Get()),"Owned root33 mixed table could not bind");fixture.ResetCounter();fixture.Draw(1);Check(fixture.list->Close());
    Require(!PrintValidationErrors(device.Get()),"Validation errors before first source-table submission");
    ID3D12CommandList* lists[]{fixture.list.Get()};auto firstSubmission=cvr::gpu::PrepareCommandResources(1,lists);
    Require(!cvr::gpu::SubmissionEmpty(firstSubmission),"Owned source bank lacked command-resource lifetime");queue->ExecuteCommandLists(1,lists);cvr::gpu::SubmitCommandResources(queue.Get(),std::move(firstSubmission));Await(queue.Get(),complete.Get(),1);
    Check(fixture.list->Reset(fixture.next.Get(),nullptr));cvr::gpu::ResetCommandResources(fixture.list.Get());audit::Reset(fixture.list.Get());cvr::gpu::CollectCommandResources();fixture.BindOriginal();
    fixture.Overwrite();std::vector<audit::OwnedSourceTable> changed;
    Require(audit::CaptureSourceTablesAtDraw(device.Get(),fixture.list.Get(),fixture.bindings,fixture.requests,Provider::Resolve,&provider,cache,changed),"Changed source capture failed");
    for(UINT n=0;n<captured.size();++n)Require(changed[n].identity!=captured[n].identity || changed[n].version!=captured[n].version,"New descriptor contents reused the old immutable cache version");
    WriteUInt(fixture.constants.Get(),13);Require(bank->BindSource(fixture.list.Get()),"Overwritten native descriptors changed old source bank");fixture.ResetCounter();fixture.Draw(2);
    D3D12_PLACED_SUBRESOURCE_FOOTPRINT footprint{};UINT64 bytes{};device->GetCopyableFootprints(&fixture.targetDesc,0,1,0,&footprint,nullptr,nullptr,&bytes);
    auto readback=Buffer(device.Get(),bytes,D3D12_HEAP_TYPE_READBACK,D3D12_RESOURCE_FLAG_NONE,D3D12_RESOURCE_STATE_COPY_DEST);
    Transition(fixture.list.Get(),fixture.target.Get(),D3D12_RESOURCE_STATE_RENDER_TARGET,D3D12_RESOURCE_STATE_COPY_SOURCE);
    D3D12_TEXTURE_COPY_LOCATION from{},to{};from.pResource=fixture.target.Get();from.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    to.pResource=readback.Get();to.Type=D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;to.PlacedFootprint=footprint;fixture.list->CopyTextureRegion(&to,0,0,0,&from,nullptr);Check(fixture.list->Close());
    Require(!PrintValidationErrors(device.Get()),"Validation errors before final source-table submission");
    auto submitted=cvr::gpu::PrepareCommandResources(1,lists);Require(!cvr::gpu::SubmissionEmpty(submitted),"Final owned draw had no submission receipt");
    captured.clear();repeated.clear();changed.clear();cache.Clear();provider.resource.Reset();provider.owner.Reset();
    fixture.constants.Reset();fixture.texture.Reset();fixture.extra.Reset();fixture.unordered.Reset();fixture.counter.Reset();fixture.cpu.Reset();fixture.visible.Reset();fixture.samplerCpu.Reset();fixture.samplerVisible.Reset();bank.reset();
    Require(!destroyed && !resourcesDestroyed,"Original caller/cache release destroyed recorded source backing or counter");
    Check(queue->Wait(gate.Get(),1));queue->ExecuteCommandLists(1,lists);cvr::gpu::SubmitCommandResources(queue.Get(),std::move(submitted));
    Check(fixture.list->Reset(fixture.allocator.Get(),nullptr));cvr::gpu::ResetCommandResources(fixture.list.Get());audit::Reset(fixture.list.Get());Check(fixture.list->Close());
    Require(!destroyed && !resourcesDestroyed,"Reset destroyed a GPU-in-flight source table or counter");Check(gate->Signal(1));Await(queue.Get(),complete.Get(),2);cvr::gpu::CollectCommandResources();
    Require(destroyed==1 && resourcesDestroyed==5,"Completed source bank leaked CBV/counter/read-resource owners");
    void* data{};const D3D12_RANGE range{0,SIZE_T(bytes)};Check(readback->Map(0,&range,&data));const auto* pixels=static_cast<const UINT*>(data);
    const std::array<UINT,12> expected{18,11,5,0,18,11,5,0,20,13,5,0};const bool equal=std::equal(expected.begin(),expected.end(),pixels);
    const D3D12_RANGE none{};readback->Unmap(0,&none);Require(equal,"Native/owned UInt outputs differed, counter owner was lost, or CBV contents were frozen");
    audit::EndBindings();ComPtr<ID3D12InfoQueue> info;Check(device.As(&info));
    for(UINT64 n=0;n<info->GetNumStoredMessages();++n){SIZE_T size{};Check(info->GetMessage(n,nullptr,&size));std::vector<char> storage(size);
        auto* message=reinterpret_cast<D3D12_MESSAGE*>(storage.data());Check(info->GetMessage(n,message,&size));
        if(message->Severity<=D3D12_MESSAGE_SEVERITY_ERROR){std::fprintf(stderr,"%s\n",message->pDescription);throw std::runtime_error("D3D12 validation error");}}
    std::puts("PASS source-table owned CPU descriptors, mixed/null/implicit views, UAV counter, root33 admitted finite tail, sparse typed holes, mutation race, cache reuse/rollback/budget, original UInt draw equivalence and fenced lifetime; input contents are NOT frozen; native creation/physical provenance NOT certified");return 0;
}catch(const std::exception& error){std::fprintf(stderr,"%s\n",error.what());return 1;}
