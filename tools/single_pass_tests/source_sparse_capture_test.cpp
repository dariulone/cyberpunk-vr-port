#include "Render/StereoDescriptorAudit.hpp"
#include "Render/NativeStereoFamilies.hpp"
#include "Render/NativeTextureRead.hpp"
#include <d3d12sdklayers.h>
#include <dxgi1_6.h>
#include <algorithm>
#include <array>
#include <atomic>
#include <cstdio>
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

static void Require(bool value,const char* message,const std::source_location at=std::source_location::current()){
    if(!value)throw std::runtime_error(std::string(message)+" at line "+std::to_string(at.line()));
}
static void Check(HRESULT value,const std::source_location at=std::source_location::current()){
    if(FAILED(value))throw std::runtime_error("D3D12 HRESULT "+std::to_string(uint32_t(value))+" at line "+std::to_string(at.line()));
}
class Sentinel final:public IUnknown {
    std::atomic<ULONG> refs_{1};std::atomic<UINT>& destroyed_;
public:
    explicit Sentinel(std::atomic<UINT>& destroyed):destroyed_(destroyed){}
    ~Sentinel(){++destroyed_;}
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** output)override{
        if(!output)return E_POINTER;*output=nullptr;if(id!=__uuidof(IUnknown))return E_NOINTERFACE;*output=this;AddRef();return S_OK;
    }
    ULONG STDMETHODCALLTYPE AddRef()override{return ++refs_;}
    ULONG STDMETHODCALLTYPE Release()override{const auto left=--refs_;if(!left)delete this;return left;}
};
static ComPtr<ID3D12Resource> Buffer(ID3D12Device* device,UINT64 bytes,D3D12_HEAP_TYPE type,D3D12_RESOURCE_FLAGS flags=D3D12_RESOURCE_FLAG_NONE){
    D3D12_HEAP_PROPERTIES heap{};heap.Type=type;D3D12_RESOURCE_DESC desc{};
    desc.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;desc.Width=bytes;desc.Height=desc.DepthOrArraySize=desc.MipLevels=desc.SampleDesc.Count=1;
    desc.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;desc.Flags=flags;ComPtr<ID3D12Resource> result;
    Check(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,type==D3D12_HEAP_TYPE_UPLOAD?D3D12_RESOURCE_STATE_GENERIC_READ:D3D12_RESOURCE_STATE_COMMON,nullptr,IID_PPV_ARGS(&result)));return result;
}
struct Fixture {
    static constexpr UINT Extent=32768,Base=32;
    ID3D12Device* device{};UINT stride{},samplerStride{};
    ComPtr<ID3D12RootSignature> root;ComPtr<ID3D12DescriptorHeap> visible,samplers;
    ComPtr<ID3D12CommandAllocator> allocator;ComPtr<ID3D12GraphicsCommandList> list;
    ComPtr<ID3D12Resource> constants,read,other,unordered,counter;
    D3D12_SHADER_RESOURCE_VIEW_DESC srv{};D3D12_UNORDERED_ACCESS_VIEW_DESC uav{};
    D3D12_CONSTANT_BUFFER_VIEW_DESC cbv{};D3D12_SAMPLER_DESC sampler{};
    audit::CbvOrigin origin;scene_state::Bindings bindings;
    std::array<audit::SourceSlotUse,4> dynamicUses{{{32763,1,D3D12_DESCRIPTOR_RANGE_TYPE_SRV},{29614,1,D3D12_DESCRIPTOR_RANGE_TYPE_SRV},
        {30989,1,D3D12_DESCRIPTOR_RANGE_TYPE_SRV},{29614,1,D3D12_DESCRIPTOR_RANGE_TYPE_SRV}}};
    std::array<audit::SourceSlotUse,3> mixedUses{{{4,1,D3D12_DESCRIPTOR_RANGE_TYPE_UAV},{0,1,D3D12_DESCRIPTOR_RANGE_TYPE_CBV},{2,1,D3D12_DESCRIPTOR_RANGE_TYPE_SRV}}};
    std::array<audit::SourceSlotUse,2> samplerUses{{{3,1,D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER},{1,1,D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER}}};
    std::array<audit::SourceTableRequest,3> requests;
    D3D12_CPU_DESCRIPTOR_HANDLE Cpu(UINT offset)const{return {visible->GetCPUDescriptorHandleForHeapStart().ptr+SIZE_T(offset)*stride};}
    D3D12_GPU_DESCRIPTOR_HANDLE Gpu(UINT offset)const{return {visible->GetGPUDescriptorHandleForHeapStart().ptr+UINT64(offset)*stride};}
    D3D12_CPU_DESCRIPTOR_HANDLE SamplerCpu(UINT offset)const{return {samplers->GetCPUDescriptorHandleForHeapStart().ptr+SIZE_T(offset)*samplerStride};}
    void Srv(UINT offset,ID3D12Resource* resource){device->CreateShaderResourceView(resource,&srv,Cpu(offset));audit::Srv(resource,&srv,Cpu(offset));}
    void RestoreCbv(){device->CreateConstantBufferView(&cbv,Cpu(0));audit::Cbv(&cbv,Cpu(0),&origin);}
    void Bind(){ID3D12DescriptorHeap* heaps[]{visible.Get(),samplers.Get()};list->SetDescriptorHeaps(2,heaps);audit::HeapsBound(device,2,heaps);
        list->SetGraphicsRootSignature(root.Get());audit::Root(list.Get(),root.Get());
        list->SetGraphicsRootDescriptorTable(0,Gpu(Base));audit::Table(list.Get(),0,Gpu(Base));
        list->SetGraphicsRootDescriptorTable(1,Gpu(0));audit::Table(list.Get(),1,Gpu(0));
        const auto samplerGpu=samplers->GetGPUDescriptorHandleForHeapStart();list->SetGraphicsRootDescriptorTable(2,samplerGpu);audit::Table(list.Get(),2,samplerGpu);
        bindings={};bindings.root=root.Get();bindings.tableMask=7;bindings.tables[0]=Gpu(Base);bindings.tables[1]=Gpu(0);bindings.tables[2]=samplerGpu;
        bindings.heaps={visible.Get(),samplers.Get()};bindings.heapCount=2;bindings.heapsSeen=true;bindings.resetSeen=true;
    }
    explicit Fixture(ID3D12Device* input,std::atomic<UINT>& originDestroyed):device(input){
        stride=device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);samplerStride=device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER);
        const D3D12_DESCRIPTOR_HEAP_DESC resourceDesc{D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,Extent+Base,D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,0};
        Check(device->CreateDescriptorHeap(&resourceDesc,IID_PPV_ARGS(&visible)));audit::HeapCreated(device,visible.Get());
        const D3D12_DESCRIPTOR_HEAP_DESC samplerDesc{D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER,4,D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,0};
        Check(device->CreateDescriptorHeap(&samplerDesc,IID_PPV_ARGS(&samplers)));audit::HeapCreated(device,samplers.Get());
        const D3D12_DESCRIPTOR_RANGE dynamic{D3D12_DESCRIPTOR_RANGE_TYPE_SRV,UINT_MAX,0,1,0};
        const D3D12_DESCRIPTOR_RANGE mixed[]{{D3D12_DESCRIPTOR_RANGE_TYPE_CBV,1,4,0,0},{D3D12_DESCRIPTOR_RANGE_TYPE_SRV,1,0,0,2},{D3D12_DESCRIPTOR_RANGE_TYPE_UAV,1,0,0,4}};
        const D3D12_DESCRIPTOR_RANGE samplerRange{D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER,4,0,0,0};std::array<D3D12_ROOT_PARAMETER,3> params{};
        for(auto& parameter:params){parameter.ParameterType=D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;parameter.ShaderVisibility=D3D12_SHADER_VISIBILITY_ALL;}
        params[0].DescriptorTable={1,&dynamic};params[1].DescriptorTable={3,mixed};params[2].DescriptorTable={1,&samplerRange};
        const D3D12_ROOT_SIGNATURE_DESC rootDesc{UINT(params.size()),params.data(),0,nullptr,D3D12_ROOT_SIGNATURE_FLAG_NONE};ComPtr<ID3DBlob> bytes,error;
        Check(D3D12SerializeRootSignature(&rootDesc,D3D_ROOT_SIGNATURE_VERSION_1,&bytes,&error));
        Check(device->CreateRootSignature(0,bytes->GetBufferPointer(),bytes->GetBufferSize(),IID_PPV_ARGS(&root)));audit::RootCreated(root.Get(),bytes->GetBufferPointer(),bytes->GetBufferSize());
        Check(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,IID_PPV_ARGS(&allocator)));
        Check(device->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_DIRECT,allocator.Get(),nullptr,IID_PPV_ARGS(&list)));
        constants=Buffer(device,512,D3D12_HEAP_TYPE_UPLOAD);read=Buffer(device,512,D3D12_HEAP_TYPE_DEFAULT);other=Buffer(device,512,D3D12_HEAP_TYPE_DEFAULT);
        unordered=Buffer(device,512,D3D12_HEAP_TYPE_DEFAULT,D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS);counter=Buffer(device,4096,D3D12_HEAP_TYPE_DEFAULT,D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS);
        srv.Format=DXGI_FORMAT_R32_UINT;srv.ViewDimension=D3D12_SRV_DIMENSION_BUFFER;srv.Shader4ComponentMapping=D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;srv.Buffer.NumElements=16;
        Srv(Base+29614,read.Get());Srv(Base+30989,nullptr);Srv(Base+32763,other.Get());Srv(2,read.Get());
        uav.Format=DXGI_FORMAT_UNKNOWN;uav.ViewDimension=D3D12_UAV_DIMENSION_BUFFER;uav.Buffer.NumElements=16;uav.Buffer.StructureByteStride=4;
        device->CreateUnorderedAccessView(unordered.Get(),counter.Get(),&uav,Cpu(4));audit::Uav(unordered.Get(),counter.Get(),&uav,Cpu(4));
        cbv={constants->GetGPUVirtualAddress(),256};origin.cookie=audit::SourceResourceCookieAtCreation(constants.Get());origin.id=0xB400;
        origin.owner.Attach(new Sentinel(originDestroyed));Require(origin.cookie!=0,"Missing current CBV resource cookie");RestoreCbv();
        sampler.Filter=D3D12_FILTER_MIN_MAG_MIP_POINT;sampler.AddressU=sampler.AddressV=sampler.AddressW=D3D12_TEXTURE_ADDRESS_MODE_CLAMP;
        sampler.MaxAnisotropy=1;sampler.ComparisonFunc=D3D12_COMPARISON_FUNC_NEVER;sampler.MaxLOD=D3D12_FLOAT32_MAX;
        for(UINT offset:{1u,3u}){device->CreateSampler(&sampler,SamplerCpu(offset));audit::Sampler(&sampler,SamplerCpu(offset));}
        requests={audit::SourceTableRequest{0,Extent,dynamicUses},audit::SourceTableRequest{1,5,mixedUses},audit::SourceTableRequest{2,4,samplerUses}};
        Require(audit::BeginBindings(),"Live metadata binding mode failed");Bind();
    }
};
struct Provider {
    Fixture& fixture;ComPtr<IUnknown> owner;ComPtr<ID3D12Resource> wrong;
    UINT mode{},calls{};bool wrongBacking{},timedOut{},reentered{};HANDLE done{};std::thread worker;
    explicit Provider(Fixture& input):fixture(input){}
    ~Provider(){Finish();}
    void Finish(){if(worker.joinable())worker.join();if(done){CloseHandle(done);done=nullptr;}}
    static bool Resolve(void* user,ID3D12GraphicsCommandList* list,UINT parameter,UINT offset,const audit::DescriptorStamp& stamp,
        const ComPtr<IUnknown>& origin,audit::SourceCbvBacking& output){
        auto& self=*static_cast<Provider*>(user);++self.calls;
        Require(parameter==1 && offset==0 && stamp.value[0]==1 && origin==self.fixture.origin.owner,"CBV provider lost its original offset/origin");
        if(self.mode){self.done=CreateEventW(nullptr,FALSE,FALSE,nullptr);Require(self.done!=nullptr,"Provider reentry event failed");
            self.worker=std::thread([&self,list]{audit::DescriptorStamp current;
                self.reentered=audit::CaptureDescriptor(list,reinterpret_cast<uintptr_t>(self.fixture.root.Get()),1,0,current);
                if(self.mode==2)self.fixture.Srv(Fixture::Base+29614,nullptr);
                if(self.mode==3)self.fixture.Srv(Fixture::Base+100,self.fixture.other.Get());
                if(self.mode==4)audit::Table(list,0,self.fixture.Gpu(Fixture::Base+1));SetEvent(self.done);
            });self.timedOut=WaitForSingleObject(self.done,5000)!=WAIT_OBJECT_0;if(self.timedOut)return false;
        }
        output.resource=self.wrongBacking?self.wrong:self.fixture.constants;output.owner=self.owner;return true;
    }
};

static void Negatives(Fixture& fixture,Provider& provider,audit::SourceTableCache& cache){
    const auto versions=cache.SlotTableCount(),descriptors=cache.SlotDescriptorCount();
    const auto reject=[&](ID3D12Device* device,ID3D12GraphicsCommandList* list,const scene_state::Bindings& bindings,
        std::span<const audit::SourceTableRequest> requests,audit::SourceCbvProvider callback,void* user,UINT reason){
        std::vector<audit::OwnedSourceSlots> output;
        Require(!audit::CaptureSourceSlotsAtDraw(device,list,bindings,requests,callback,user,cache,output),"Invalid selected-slot capture succeeded");
        Require(output.empty() && cache.SlotTableCount()==versions && cache.SlotDescriptorCount()==descriptors && !cache.TableCount() && !cache.DescriptorCount(),"Failed selected-slot capture changed cache/output");
        Require(audit::LastSourceTableFailure().reason==reason,"Unexpected selected-slot failure reason");
    };
    const auto valid=fixture.bindings;
    reject(nullptr,fixture.list.Get(),valid,fixture.requests,Provider::Resolve,&provider,1);
    reject(fixture.device,nullptr,valid,fixture.requests,Provider::Resolve,&provider,1);
    auto binding=valid;binding.heapsSeen=false;reject(fixture.device,fixture.list.Get(),binding,fixture.requests,Provider::Resolve,&provider,1);
    binding=valid;binding.tables[0].ptr+=fixture.stride;reject(fixture.device,fixture.list.Get(),binding,fixture.requests,Provider::Resolve,&provider,2);
    binding=valid;binding.tableMask&=~1ull;reject(fixture.device,fixture.list.Get(),binding,fixture.requests,Provider::Resolve,&provider,4);
    binding=valid;binding.heaps[0]=fixture.samplers.Get();reject(fixture.device,fixture.list.Get(),binding,fixture.requests,Provider::Resolve,&provider,5);
    for(UINT variant=0;variant<4;++variant){auto requests=fixture.requests;
        if(variant==0)requests[0].count=0;if(variant==1)requests[0].count=UINT_MAX;if(variant==2)requests[0].uses={};if(variant==3)requests[1].parameter=0;
        reject(fixture.device,fixture.list.Get(),valid,requests,Provider::Resolve,&provider,4);
    }
    for(const auto use:{audit::SourceSlotUse{UINT_MAX,1,D3D12_DESCRIPTOR_RANGE_TYPE_SRV},audit::SourceSlotUse{32768,1,D3D12_DESCRIPTOR_RANGE_TYPE_SRV},
        audit::SourceSlotUse{0,UINT_MAX,D3D12_DESCRIPTOR_RANGE_TYPE_SRV},audit::SourceSlotUse{0,audit::SourceSlotCaptureLimit+1,D3D12_DESCRIPTOR_RANGE_TYPE_SRV},
        audit::SourceSlotUse{29614,1,D3D12_DESCRIPTOR_RANGE_TYPE_UAV},audit::SourceSlotUse{29614,0,D3D12_DESCRIPTOR_RANGE_TYPE_SRV}}){
        auto requests=fixture.requests;requests[0].uses={&use,1};reject(fixture.device,fixture.list.Get(),valid,requests,Provider::Resolve,&provider,4);
    }
    {const audit::SourceSlotUse hole{1,1,D3D12_DESCRIPTOR_RANGE_TYPE_SRV};auto requests=fixture.requests;requests[1].uses={&hole,1};
        reject(fixture.device,fixture.list.Get(),valid,requests,Provider::Resolve,&provider,4);}
    {const audit::SourceSlotUse uninitialized{32000,1,D3D12_DESCRIPTOR_RANGE_TYPE_SRV};auto requests=fixture.requests;requests[0].uses={&uninitialized,1};
        reject(fixture.device,fixture.list.Get(),valid,requests,Provider::Resolve,&provider,5);}
    {const audit::SourceSlotUse fullBudget{0,audit::SourceSlotCaptureLimit,D3D12_DESCRIPTOR_RANGE_TYPE_SRV};
        const std::array<audit::SourceTableRequest,3> requests{{fixture.requests[1],fixture.requests[2],{0,Fixture::Extent,{&fullBudget,1}}}};
        reject(fixture.device,fixture.list.Get(),valid,requests,Provider::Resolve,&provider,4);}
    {auto requests=fixture.requests;requests[0].count=Fixture::Extent+1;reject(fixture.device,fixture.list.Get(),valid,requests,Provider::Resolve,&provider,5);}
    reject(fixture.device,fixture.list.Get(),valid,fixture.requests,nullptr,nullptr,8);
    provider.wrong=Buffer(fixture.device,256,D3D12_HEAP_TYPE_UPLOAD);provider.wrongBacking=true;
    reject(fixture.device,fixture.list.Get(),valid,fixture.requests,Provider::Resolve,&provider,8);provider.wrongBacking=false;provider.wrong.Reset();
    provider.mode=2;reject(fixture.device,fixture.list.Get(),valid,fixture.requests,Provider::Resolve,&provider,9);provider.Finish();
    Require(!provider.timedOut && provider.reentered,"Provider was called under the metadata lock");provider.mode=0;fixture.Srv(Fixture::Base+29614,fixture.read.Get());
    provider.mode=4;reject(fixture.device,fixture.list.Get(),valid,fixture.requests,Provider::Resolve,&provider,9);provider.Finish();provider.mode=0;fixture.Bind();
    {auto invalid=fixture.srv;invalid.Format=DXGI_FORMAT_UNKNOWN;audit::Srv(fixture.read.Get(),&invalid,fixture.Cpu(Fixture::Base+29614));
        reject(fixture.device,fixture.list.Get(),valid,fixture.requests,Provider::Resolve,&provider,6);fixture.Srv(Fixture::Base+29614,fixture.read.Get());}
    {auto dangling=Buffer(fixture.device,512,D3D12_HEAP_TYPE_DEFAULT);fixture.Srv(Fixture::Base+32001,dangling.Get());dangling.Reset();
        const audit::SourceSlotUse dead{32001,1,D3D12_DESCRIPTOR_RANGE_TYPE_SRV};const audit::SourceTableRequest request{0,Fixture::Extent,{&dead,1}};
        reject(fixture.device,fixture.list.Get(),valid,{&request,1},nullptr,nullptr,7);}
    {auto dangling=Buffer(fixture.device,4096,D3D12_HEAP_TYPE_DEFAULT,D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS);
        audit::Uav(fixture.unordered.Get(),dangling.Get(),&fixture.uav,fixture.Cpu(4));dangling.Reset();
        reject(fixture.device,fixture.list.Get(),valid,fixture.requests,Provider::Resolve,&provider,7);
        audit::Uav(fixture.unordered.Get(),fixture.counter.Get(),&fixture.uav,fixture.Cpu(4));}
    {ComPtr<ID3D12CommandAllocator> allocator;ComPtr<ID3D12GraphicsCommandList> compute;
        Check(fixture.device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_COMPUTE,IID_PPV_ARGS(&allocator)));
        Check(fixture.device->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_COMPUTE,allocator.Get(),nullptr,IID_PPV_ARGS(&compute)));
        reject(fixture.device,compute.Get(),valid,fixture.requests,Provider::Resolve,&provider,1);Check(compute->Close());}
    {const std::array<audit::SourceSlotUse,2> conflict{{{29614,1,D3D12_DESCRIPTOR_RANGE_TYPE_SRV},{29614,1,D3D12_DESCRIPTOR_RANGE_TYPE_UAV}}};
        auto requests=fixture.requests;requests[0].uses=conflict;reject(fixture.device,fixture.list.Get(),valid,requests,Provider::Resolve,&provider,4);}
}

int main()try{
    ComPtr<ID3D12Debug> debug;Check(D3D12GetDebugInterface(IID_PPV_ARGS(&debug)));debug->EnableDebugLayer();
    ComPtr<IDXGIFactory4> factory;Check(CreateDXGIFactory1(IID_PPV_ARGS(&factory)));ComPtr<IDXGIAdapter> adapter;Check(factory->EnumWarpAdapter(IID_PPV_ARGS(&adapter)));
    ComPtr<ID3D12Device> device;Check(D3D12CreateDevice(adapter.Get(),D3D_FEATURE_LEVEL_12_0,IID_PPV_ARGS(&device)));audit::Initialize(device.Get());
    std::atomic<UINT> originsDestroyed{},extrasDestroyed{},resourcesDestroyed{};Fixture fixture(device.Get(),originsDestroyed);Provider provider(fixture);
    provider.owner.Attach(new Sentinel(extrasDestroyed));constexpr GUID lifetimeGuid{0xd8ef1a60,0xe923,0x4aac,{0x9c,0xd4,0x18,0x92,0x55,0x60,0x21,0x00}};
    for(auto* resource:{fixture.constants.Get(),fixture.read.Get(),fixture.other.Get(),fixture.unordered.Get(),fixture.counter.Get()}){
        ComPtr<IUnknown> sentinel;sentinel.Attach(new Sentinel(resourcesDestroyed));Check(resource->SetPrivateDataInterface(lifetimeGuid,sentinel.Get()));}
    audit::SourceTableCache cache;std::vector<audit::OwnedSourceSlots> captured,repeated;
    std::vector<audit::SourceResourceBinding> dependencies;
    Require(audit::CaptureSourceSlotsAtDraw(device.Get(),fixture.list.Get(),fixture.bindings,fixture.requests,Provider::Resolve,&provider,cache,captured,&dependencies),"Initial sparse selected-slot capture failed");
    Require(dependencies.size()==5 && dependencies[0].parameter==0 && dependencies[0].offset==29614 &&
        dependencies[1].offset==30989 && !dependencies[1].resource && dependencies[2].offset==32763 &&
        dependencies[3].parameter==1 && dependencies[3].offset==2 && dependencies[4].offset==4 &&
        dependencies[4].resource==fixture.unordered && dependencies[4].counter==fixture.counter &&
        dependencies[4].type==D3D12_DESCRIPTOR_RANGE_TYPE_UAV,"Sparse dependencies lost original high offsets or UAV counter");
    for(const auto& dependency:dependencies)Require(dependency.stamp.gpu==captured[dependency.parameter].sourceGpu+UINT64(dependency.offset)*fixture.stride &&
        dependency.stamp.heap==captured[dependency.parameter].sourceHeap,"Sparse dependency was rebased to a compact index");
    {std::vector<audit::OwnedSourceSlots> untouched;
        Require(!audit::CaptureSourceSlotsAtDraw(device.Get(),fixture.list.Get(),fixture.bindings,fixture.requests,Provider::Resolve,&provider,cache,untouched,&dependencies) &&
            untouched.empty() && dependencies.size()==5,"Sparse dependency output was not transactional");}
    dependencies.clear();
    Require(captured.size()==3 && cache.SlotTableCount()==3 && cache.SlotDescriptorCount()==8 && !cache.TableCount() && !cache.DescriptorCount(),"Sparse storage allocated original high-index extents or changed dense accounting");
    const std::array<std::vector<UINT>,3> offsets{{{29614,30989,32763},{0,2,4},{1,3}}};
    for(UINT table=0;table<captured.size();++table){const auto& owned=captured[table];
        Require(owned.parameter==table && owned.count==fixture.requests[table].count && owned.owner && owned.identity && owned.version &&
            owned.cpuHeap->GetDesc().NumDescriptors==offsets[table].size() && owned.cpuHeap->GetDesc().Flags==D3D12_DESCRIPTOR_HEAP_FLAG_NONE,"Selected CPU heap was not compact immutable storage");
        for(UINT n=0;n<owned.slots.size();++n){const auto& slot=owned.slots[n];
            const auto stride=table==2?fixture.samplerStride:fixture.stride;
            Require(slot.offset==offsets[table][n] && slot.compactIndex==n && slot.stamp.gpu==owned.sourceGpu+UINT64(slot.offset)*stride &&
                slot.stamp.heap==owned.sourceHeap && audit::MatchesSourceSlot(slot),"Original offset/stamp was rebased into compact storage");
        }
    }
    Require(!captured[0].slots[1].stamp.value[2] && captured[1].slots[2].stamp.value[2] && captured[1].slots[2].stamp.value[3],"Null SRV or exact UAV counter association was lost");
    Require(audit::MatchesCpuSourceSrv(fixture.Cpu(Fixture::Base+29614).ptr,captured[0].slots[0].stamp) &&
        !audit::MatchesCpuSourceSrv(fixture.Cpu(Fixture::Base+32763).ptr,captured[0].slots[0].stamp) &&
        !audit::MatchesCpuSourceSrv(0,captured[0].slots[0].stamp),"Current CPU SRV equality accepted another source slot");
    Require(!audit::MatchesDescriptor(captured[0].slots[1].stamp) && !audit::MatchesDescriptor(captured[1].slots[2].stamp),"Narrow legacy descriptor matching changed its null-SRV/UAV semantics");
    {auto invalid=captured[0].slots[0];invalid.type=D3D12_DESCRIPTOR_RANGE_TYPE_CBV;
        Require(!audit::MatchesSourceSlot(invalid),"Selected-slot matcher ignored its exact descriptor type");
        invalid=captured[0].slots[0];++invalid.stamp.heap;Require(!audit::MatchesSourceSlot(invalid),"Selected-slot matcher accepted another heap identity");
        invalid=captured[0].slots[0];invalid.stamp.value[0]=0;Require(!audit::MatchesSourceSlot(invalid),"Selected-slot matcher accepted a noncanonical value");
        Require(!audit::MatchesSourceSlot(audit::OwnedSourceSlot{}),"Empty selected-slot receipt matched current metadata");}
    Require(audit::CaptureSourceSlotsAtDraw(device.Get(),fixture.list.Get(),fixture.bindings,fixture.requests,Provider::Resolve,&provider,cache,repeated),"Stable sparse repeat failed");
    for(UINT n=0;n<captured.size();++n)Require(captured[n].identity==repeated[n].identity && captured[n].cpuHeap==repeated[n].cpuHeap && captured[n].owner==repeated[n].owner,"Identical compact versions were not interned");
    Negatives(fixture,provider,cache);
    provider.mode=1;std::vector<audit::OwnedSourceSlots> reentered;
    Require(audit::CaptureSourceSlotsAtDraw(device.Get(),fixture.list.Get(),fixture.bindings,fixture.requests,Provider::Resolve,&provider,cache,reentered),"Provider readonly reentry failed");provider.Finish();
    Require(provider.reentered && !provider.timedOut,"Provider ran under the metadata lock");provider.mode=3;std::vector<audit::OwnedSourceSlots> holeChanged;
    Require(audit::CaptureSourceSlotsAtDraw(device.Get(),fixture.list.Get(),fixture.bindings,fixture.requests,Provider::Resolve,&provider,cache,holeChanged),"Unused source-hole overwrite changed selected-slot validity");provider.Finish();provider.mode=0;
    for(UINT n=0;n<captured.size();++n)Require(holeChanged[n].identity==captured[n].identity,"Unused source-hole overwrite allocated a compact version");
    {const auto before=cache.SlotDescriptorCount();const auto owner=captured[0].owner.Get();
        Require(!audit::CaptureSourceSlotsAtDraw(device.Get(),fixture.list.Get(),fixture.bindings,fixture.requests,Provider::Resolve,&provider,cache,captured) &&
            captured[0].owner.Get()==owner && cache.SlotDescriptorCount()==before,"Nonempty selected-slot output was overwritten");}
    std::vector<audit::OwnedSourceSlots> changed;const auto originalSlot=captured[0].slots[0];fixture.Srv(Fixture::Base+29614,nullptr);
    Require(!audit::MatchesSourceSlot(originalSlot) && audit::CaptureSourceSlotsAtDraw(device.Get(),fixture.list.Get(),fixture.bindings,fixture.requests,Provider::Resolve,&provider,cache,changed) &&
        changed[0].identity!=captured[0].identity && changed[1].identity==captured[1].identity && captured[0].slots[0].stamp.value==originalSlot.stamp.value,"Overwrite altered old immutable storage or reused a changed version");
    fixture.Srv(Fixture::Base+29614,fixture.read.Get());
    std::vector<audit::OwnedSourceSlots> single,relocated;fixture.Srv(Fixture::Base+32100,fixture.other.Get());
    const audit::SourceSlotUse singleUse{32763,1,D3D12_DESCRIPTOR_RANGE_TYPE_SRV},relocatedUse{32100,1,D3D12_DESCRIPTOR_RANGE_TYPE_SRV};
    const audit::SourceTableRequest singleRequest{0,Fixture::Extent,{&singleUse,1}},relocatedRequest{0,Fixture::Extent,{&relocatedUse,1}};
    Require(audit::CaptureSourceSlotsAtDraw(device.Get(),fixture.list.Get(),fixture.bindings,{&singleRequest,1},nullptr,nullptr,cache,single) &&
        audit::CaptureSourceSlotsAtDraw(device.Get(),fixture.list.Get(),fixture.bindings,{&relocatedRequest,1},nullptr,nullptr,cache,relocated) &&
        single[0].owner==relocated[0].owner && single[0].slots[0].offset==32763 && relocated[0].slots[0].offset==32100 && single[0].cpuHeap->GetDesc().NumDescriptors==1,
        "Compact descriptor interning merged or discarded distinct original offsets");
    // Retire native CBV origin metadata so only the immutable capture retains it.
    audit::Cbv(&fixture.cbv,fixture.Cpu(0));fixture.origin.owner.Reset();provider.owner.Reset();
    Check(fixture.list->Close());audit::ClearBindings(fixture.list.Get());fixture.list.Reset();fixture.allocator.Reset();
    fixture.constants.Reset();fixture.read.Reset();fixture.other.Reset();fixture.unordered.Reset();fixture.counter.Reset();fixture.visible.Reset();fixture.samplers.Reset();
    cache.Clear();Require(!cache.SlotTableCount() && !cache.SlotDescriptorCount() && !originsDestroyed && !extrasDestroyed && !resourcesDestroyed,"Cache/native release destroyed selected backing, counter or CBV owner");
    captured.clear();repeated.clear();reentered.clear();holeChanged.clear();changed.clear();single.clear();relocated.clear();
    Require(originsDestroyed==1 && extrasDestroyed==1 && resourcesDestroyed==5,"Selected-slot versions leaked or omitted exact strong ownership");
    audit::EndBindings();ComPtr<ID3D12InfoQueue> info;Check(device.As(&info));
    for(UINT64 n=0;n<info->GetNumStoredMessages();++n){SIZE_T size{};Check(info->GetMessage(n,nullptr,&size));std::vector<char> storage(size);
        auto* message=reinterpret_cast<D3D12_MESSAGE*>(storage.data());Check(info->GetMessage(n,message,&size));
        if(message->Severity<=D3D12_MESSAGE_SEVERITY_ERROR){std::fprintf(stderr,"%s\n",message->pDescription);throw std::runtime_error("D3D12 sparse descriptor validation error");}}
    std::puts("PASS sparse high-offset live descriptors, compact immutable versions, original offsets, holes, provider reentry/revalidation, exact resource/counter/CBV owners and transactional failure; no GPU execution or input-content proof");return 0;
}catch(const std::exception& error){std::fprintf(stderr,"%s\n",error.what());return 1;}
