#include "Render/EarlyDepthPass.hpp"
#include "Render/EarlyDepthLimits.hpp"
#include "Render/CommandResources.hpp"
#include "Render/RenderProbeScope.hpp"
#include "Render/StereoTargetArray.hpp"
#include "Render/DepthCommitGate.hpp"
#include "Render/NativeMaterialRead.hpp"
#include <bit>
#include <cmath>
#include <algorithm>
#include <map>
#include <intrin.h>
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
// The pass is Rust's: rust/crates/cvr-render/src/early_depth_object.rs (its Storage an IUnknown there, which the command
// resources keep alive with the lists that recorded it; the state and recording in early_depth_pass.rs). What stays
// here are the C++ objects it reaches: the texture and material reads (their leases; the single-pass tests stub them),
// Main's family and bank, and the constant arenas' banks (StereoBindingBank over the paired-root plans).
namespace cvr::stereo {
struct EarlyDepthPass::Storage:IUnknown {};
}
namespace {
using namespace cvr::stereo;
static_assert(sizeof(texture_read::Lease)==48 && offsetof(texture_read::Lease,native)==8 && offsetof(texture_read::Lease,receipt)==32,
    "mirrored by texture_lease.rs's Lease");
static_assert(sizeof(buffer_read::Proof)==96 && sizeof(skinning::BoneRange)==16 && sizeof(InstanceRun)==8,
    "mirrored by buffer_read.rs's Proof and early_depth_pass.rs's ranges and runs");
struct CvrEdpAdmission {const uint8_t* masks;size_t maskCount,peerOnly;UINT bankDraw,bankDraws,bankSnapshots,alphaDepthContract,skinningReadContract,
    nativeCameraStages,renderTargets;int32_t depthFormat,depthFunc,depthWrite;UINT samplesCount,samplesQuality;uint64_t sourceRoot;
    bool nativeCamera,alphaReference,alphaOptimized,familyPipeline,ordinalIndependent,nativeCameraPipeline,pixelShader,fixedRaster,depthKnown,depthEnable;};
struct CvrEdoBank {void* user;bool(*capture)(void*,void*);bool(*bind)(void*,void*,UINT);};
struct CvrEdoReads {
    bool(*textureValid)(const texture_read::Lease*,ID3D12CommandQueue*);
    bool(*textureCopy)(ID3D12GraphicsCommandList*,const texture_read::Lease*,UINT,ID3D12Resource*,const D3D12_PLACED_SUBRESOURCE_FOOTPRINT*);
    uint32_t(*textureFailure)();
    void*(*materialClone)(const void*);
    void(*materialFree)(void*);
    ID3D12Resource*(*materialResource)(const void*);
    bool(*materialCopy)(ID3D12GraphicsCommandList*,const void*,ID3D12Resource*,uint64_t,buffer_read::Proof*);
    bool(*materialValid)(const void*,const buffer_read::Proof*,ID3D12CommandQueue*);
    uint32_t(*materialFailure)();
};
static_assert(sizeof(CvrEdpAdmission)==96 && sizeof(CvrEdoBank)==24 && sizeof(CvrEdoReads)==9*8,"mirrored by early_depth_object.rs");
}
extern "C" {
void cvr_edo_attach(const CvrEdoReads*);
HRESULT cvr_edo_create(ID3D12Device*,UINT,UINT,UINT,void**);
bool cvr_edo_capture_instances(void*,ID3D12GraphicsCommandList*,ID3D12Resource*,UINT64,UINT,int32_t,bool);
void cvr_edo_release_instance_read(void*,ID3D12GraphicsCommandList*);
void cvr_edo_counts(void*,uint32_t,uint64_t*);
void cvr_edo_native_counts(void*,uint32_t*);
bool cvr_edo_shader_source(void*,ID3D12Resource*);
bool cvr_edo_capture_skinning(void*,ID3D12GraphicsCommandList*,ID3D12Resource*,const skinning::BoneRange*,size_t,const texture_read::Lease*,UINT);
bool cvr_edo_capture_alpha(void*,ID3D12GraphicsCommandList*,const material_binding::Lease*,const texture_read::Lease*,const UINT*,size_t,UINT,UINT);
bool cvr_edo_start_native(void*,ID3D12GraphicsCommandList*,const native_probe::DrawRecord*,ID3D12Resource*,ID3D12Resource*,int32_t);
bool cvr_edo_observe_native(void*,ID3D12GraphicsCommandList*,const native_probe::DrawRecord*);
bool cvr_edo_original(void*,ID3D12GraphicsCommandList*,const native_probe::DrawRecord*);
bool cvr_edo_peer_remainder(void*,ID3D12GraphicsCommandList*,const native_probe::DrawRecord*,const InstanceRun*,size_t);
bool cvr_edo_finish_native(void*,ID3D12GraphicsCommandList*,UINT);
bool cvr_edo_commit_native(void*,bool);
void cvr_edo_abort_native(void*,ID3D12GraphicsCommandList*,UINT);
bool cvr_edo_main(void*,ID3D12GraphicsCommandList*,const native_probe::DrawRecord*,const CvrEdpAdmission*,const uint8_t*,size_t,const uint8_t*,size_t,
    UINT,bool,bool,ID3D12PipelineState*,ID3D12PipelineState*,ID3D12RootSignature*,const CvrEdoBank*);
bool cvr_edo_constants_open(void*);
ID3D12Device* cvr_edo_device(void*);
void* cvr_edo_arena(void*,ID3D12RootSignature*);
UINT cvr_edo_arena_count(void*);
void cvr_edo_adopt_arena(void*,ID3D12RootSignature*,void*);
bool cvr_edo_peer(void*,ID3D12GraphicsCommandList*,const native_probe::DrawRecord*);
bool cvr_edo_readback(void*,ID3D12GraphicsCommandList*);
HRESULT cvr_edo_read(void*,DepthComparisonResult*);
HRESULT cvr_edo_restart(void*);
void cvr_edo_before_submit(void*,ID3D12CommandQueue*,UINT,ID3D12CommandList* const*);
void cvr_edo_submitted(void*,ID3D12CommandQueue*,UINT,ID3D12CommandList* const*);
void cvr_edo_reset(void*,ID3D12CommandList*);
bool cvr_edo_failed(void*);
}
namespace {
const CvrEdoReads kReads{
    [](const texture_read::Lease* lease,ID3D12CommandQueue* queue){return texture_read::Validate(*lease,queue);},
    [](ID3D12GraphicsCommandList* list,const texture_read::Lease* lease,UINT mip,ID3D12Resource* buffer,const D3D12_PLACED_SUBRESOURCE_FOOTPRINT* layout){
        return mip==UINT_MAX?texture_read::Copy(list,*lease,buffer,*layout):texture_read::CopySubresource(list,*lease,mip,buffer,*layout);},
    []{return texture_read::LastFailure();},
    [](const void* lease)->void*{return new material_binding::Lease(*static_cast<const material_binding::Lease*>(lease));},
    [](void* lease){delete static_cast<material_binding::Lease*>(lease);},
    [](const void* lease){return static_cast<const material_binding::Lease*>(lease)->source.resource.Get();},
    [](ID3D12GraphicsCommandList* list,const void* lease,ID3D12Resource* destination,uint64_t offset,buffer_read::Proof* proof){
        return material_read::CopySnapshot(list,*static_cast<const material_binding::Lease*>(lease),destination,offset,*proof);},
    [](const void* lease,const buffer_read::Proof* proof,ID3D12CommandQueue* queue){
        return material_read::ValidateSnapshotSubmission(*static_cast<const material_binding::Lease*>(lease),*proof,queue);},
    []{return material_read::LastFailure();}};   // the arenas' banks are binding_bank.rs' storage (early_depth_object.rs frees them)
[[maybe_unused]] const bool kReadsAttached=(cvr_edo_attach(&kReads),true);
}
namespace cvr::stereo {
EarlyDepthPass::EarlyDepthPass()=default;
EarlyDepthPass::~EarlyDepthPass()=default;
HRESULT EarlyDepthPass::Create(ID3D12Device* device,UINT width,UINT height,UINT nativeMode){
    if(storage_)return E_INVALIDARG;
    void* created{};const auto hr=cvr_edo_create(device,width,height,nativeMode,&created);
    if(SUCCEEDED(hr))storage_.Attach(static_cast<Storage*>(static_cast<IUnknown*>(created)));
    return hr;
}
bool EarlyDepthPass::CaptureInstances(ID3D12GraphicsCommandList* list,ID3D12Resource* source,UINT64 offset,UINT bytes,D3D12_RESOURCE_STATES state,bool combinedRead){
    return storage_ && cvr_edo_capture_instances(storage_.Get(),list,source,offset,bytes,int32_t(state),combinedRead);
}
void EarlyDepthPass::ReleaseInstanceRead(ID3D12GraphicsCommandList* list){if(storage_)cvr_edo_release_instance_read(storage_.Get(),list);}
std::array<uint64_t,2> EarlyDepthPass::InstanceProofCounts()const{std::array<uint64_t,2> out{};if(storage_)cvr_edo_counts(storage_.Get(),0,out.data());return out;}
bool EarlyDepthPass::ShaderSource(ID3D12Resource* resource)const{return storage_ && cvr_edo_shader_source(storage_.Get(),resource);}
std::array<uint64_t,4> EarlyDepthPass::ShaderProofCounts()const{std::array<uint64_t,4> out{};if(storage_)cvr_edo_counts(storage_.Get(),2,out.data());return out;}
bool EarlyDepthPass::CaptureSkinning(ID3D12GraphicsCommandList* list,ID3D12Resource* bones,std::span<const skinning::BoneRange> ranges,
                                    const texture_read::Lease& texture,UINT sequence){
    return storage_ && cvr_edo_capture_skinning(storage_.Get(),list,bones,ranges.data(),ranges.size(),&texture,sequence);
}
bool EarlyDepthPass::CaptureAlpha(ID3D12GraphicsCommandList* list,const material_binding::Lease& material,
    const texture_read::Lease& texture,std::span<const UINT> mips,UINT expectedIndex,UINT sequence){
    return storage_ && cvr_edo_capture_alpha(storage_.Get(),list,&material,&texture,mips.data(),mips.size(),expectedIndex,sequence);
}
std::array<uint64_t,6> EarlyDepthPass::AlphaProofCounts()const{std::array<uint64_t,6> out{};if(storage_)cvr_edo_counts(storage_.Get(),1,out.data());return out;}
bool EarlyDepthPass::StartNative(ID3D12GraphicsCommandList* list,const native_probe::DrawRecord& d,ID3D12Resource* target,
                                ID3D12Resource* instanceSource,D3D12_RESOURCE_STATES instanceState){
    return storage_ && cvr_edo_start_native(storage_.Get(),list,&d,target,instanceSource,int32_t(instanceState));
}
bool EarlyDepthPass::ObserveNative(ID3D12GraphicsCommandList* list,const native_probe::DrawRecord& d){
    return storage_ && cvr_edo_observe_native(storage_.Get(),list,&d);
}
bool EarlyDepthPass::Original(ID3D12GraphicsCommandList* list,const native_probe::DrawRecord& d){
    return storage_ && cvr_edo_original(storage_.Get(),list,&d);
}
bool EarlyDepthPass::PeerRemainder(ID3D12GraphicsCommandList* list,const native_probe::DrawRecord& d,std::span<const InstanceRun> remaining){
    return storage_ && cvr_edo_peer_remainder(storage_.Get(),list,&d,remaining.data(),remaining.size());
}
bool EarlyDepthPass::FinishNative(ID3D12GraphicsCommandList* list,UINT side){return storage_ && cvr_edo_finish_native(storage_.Get(),list,side);}
std::array<uint32_t,8> EarlyDepthPass::NativeCounts()const{std::array<uint32_t,8> out{};if(storage_)cvr_edo_native_counts(storage_.Get(),out.data());return out;}
std::array<uint64_t,4> EarlyDepthPass::NativeDifferences()const{std::array<uint64_t,4> out{};if(storage_)cvr_edo_counts(storage_.Get(),4,out.data());return out;}
std::array<uint64_t,8> EarlyDepthPass::OptimizedCounts()const{std::array<uint64_t,8> out{};if(storage_)cvr_edo_counts(storage_.Get(),3,out.data());return out;}
std::array<uint64_t,6> EarlyDepthPass::GpuTimes()const{std::array<uint64_t,6> out{};if(storage_)cvr_edo_counts(storage_.Get(),5,out.data());return out;}
bool EarlyDepthPass::CommitNative(bool allow){return storage_ && cvr_edo_commit_native(storage_.Get(),allow);}
void EarlyDepthPass::AbortNative(ID3D12GraphicsCommandList* list,UINT side){if(storage_)cvr_edo_abort_native(storage_.Get(),list,side);}
bool EarlyDepthPass::Main(ID3D12GraphicsCommandList* list,const native_probe::DrawRecord& draw,const families::Family& family,StereoBindingBank& bank,
    std::span<const uint8_t> masks,std::span<const uint8_t> peerOnly,UINT bankDraw,bool nativeCamera,bool alphaReference,bool alphaOptimized){
    if(!storage_)return false;const auto& pass=family.pass;
    const CvrEdpAdmission admission{masks.data(),masks.size(),peerOnly.size(),bankDraw,bank.DrawCount(),bank.SnapshotCount(),family.alphaDepthContract,
        family.skinningReadContract,family.nativeCameraStages,pass.targets.NumRenderTargets,int32_t(pass.depth),int32_t(pass.depthState.DepthFunc),
        int32_t(pass.depthState.DepthWriteMask),pass.samples.Count,pass.samples.Quality,reinterpret_cast<uintptr_t>(family.sourceRoot.Get()),nativeCamera,
        alphaReference,alphaOptimized,family.pipeline!=nullptr,family.instanceOrdinalIndependent,family.nativeCameraPipeline!=nullptr,pass.pixelShader,
        pass.fixedRasterState,pass.depthStateKnown,pass.depthState.DepthEnable!=FALSE};
    const CvrEdoBank banks{&bank,
        [](void* b,void* l){auto& bank=*static_cast<StereoBindingBank*>(b);auto* recording=static_cast<ID3D12GraphicsCommandList*>(l);
            return bank.CapturedFor(recording) || (bank.CaptureEye(recording,0) && bank.CaptureEye(recording,1));},
        [](void* b,void* l,UINT index){return static_cast<StereoBindingBank*>(b)->Bind(static_cast<ID3D12GraphicsCommandList*>(l),index);}};
    return cvr_edo_main(storage_.Get(),list,&draw,&admission,masks.data(),masks.size(),peerOnly.data(),peerOnly.size(),bankDraw,nativeCamera,alphaOptimized,
        nativeCamera?family.nativeCameraPipeline.Get():family.pipeline.Get(),family.nativeCameraPipeline.Get(),family.sourceRoot.Get(),&banks);
}
bool EarlyDepthPass::MainConstants(ID3D12GraphicsCommandList* list,const native_probe::DrawRecord& draw,const families::Family& family,
    std::span<const BankPair> pairs,std::span<const uint8_t> masks,std::span<const uint8_t> extra){
    if(!storage_ || !cvr_edo_constants_open(storage_.Get()) || !family.pairedRoot)return false;
    const auto root=family.pairedRoot->root.Get();void* arena=cvr_edo_arena(storage_.Get(),root);
    if(!arena){
        if(cvr_edo_arena_count(storage_.Get())>=8)return false;
        StereoBindingBank made;
        if(FAILED(made.CreateArena(cvr_edo_device(storage_.Get()),root,family.pairedRoot->plan,early_depth::MaxDraws,UINT64(early_depth::MaxDraws)*4096)))
            return false;
        arena=made.DetachStorage();cvr_edo_adopt_arena(storage_.Get(),root,arena);   // the pass holds the bank's storage
    }
    StereoBindingBank bank;static_cast<IUnknown*>(arena)->AddRef();bank.AttachStorage(arena);   // a reference for this view
    UINT index{};if(FAILED(bank.AppendConstants(family.pairedRoot->plan,pairs,index)))return false;
    return Main(list,draw,family,bank,masks,extra,index);
}
UINT EarlyDepthPass::ArenaCount()const{return storage_?cvr_edo_arena_count(storage_.Get()):0;}
bool EarlyDepthPass::Peer(ID3D12GraphicsCommandList* list,const native_probe::DrawRecord& draw){return storage_ && cvr_edo_peer(storage_.Get(),list,&draw);}
bool EarlyDepthPass::Readback(ID3D12GraphicsCommandList* list){return storage_ && cvr_edo_readback(storage_.Get(),list);}
void EarlyDepthPass::BeforeSubmit(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists){
    if(storage_ && queue && lists)cvr_edo_before_submit(storage_.Get(),queue,count,lists);
}
void EarlyDepthPass::Submitted(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists){
    if(storage_ && queue && lists)cvr_edo_submitted(storage_.Get(),queue,count,lists);
}
bool EarlyDepthPass::Failed()const{return storage_ && cvr_edo_failed(storage_.Get());}
void EarlyDepthPass::Reset(ID3D12CommandList* list){if(storage_ && list)cvr_edo_reset(storage_.Get(),list);}
HRESULT EarlyDepthPass::Read(DepthComparisonResult& result){return storage_?cvr_edo_read(storage_.Get(),&result):E_UNEXPECTED;}
HRESULT EarlyDepthPass::Restart(){return storage_?cvr_edo_restart(storage_.Get()):E_UNEXPECTED;}
}
#endif
#else

namespace cvr::stereo {
using Microsoft::WRL::ComPtr;
static_assert(early_depth::MaxDraws<=StereoBindingBank::MaxArenaDraws);
struct EarlyDepthPass::Storage final:IUnknown {
    struct RecordingToken final:IUnknown {
        std::atomic<ULONG> refs{1};
        HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** out)override{if(!out)return E_POINTER;*out=nullptr;if(id!=__uuidof(IUnknown))return E_NOINTERFACE;*out=this;AddRef();return S_OK;}
        ULONG STDMETHODCALLTYPE AddRef()override{return ++refs;}
        ULONG STDMETHODCALLTYPE Release()override{const auto n=--refs;if(!n)delete this;return n;}
    };
    std::atomic<ULONG> refs{1};ComPtr<ID3D12Device> device;
    std::array<ComPtr<RecordingToken>,2> recordingTokens;
    std::array<bool,2> submissionPending{},recordingRetired{};
    std::array<StereoTargetArray,2> targets;
    std::array<ComPtr<ID3D12Resource>,2> native;
    std::array<uint64_t,2> nativeDsv{};
    std::array<uint32_t,8> nativeCounts{}; // enabled, MAIN draws, VR draws, MAIN fallback, VR runs, MAIN export, VR export, imports
    std::array<uint64_t,4> nativeDifferences{}; // depth per eye, stencil per eye
    DepthCommitGate gate;
    std::array<ComPtr<ID3D12Resource>,2> instanceSnapshots;
    ComPtr<ID3D12Resource> instanceSource;
    std::vector<std::pair<UINT64,UINT>> instanceRanges;
    UINT instanceBytes{};bool instancesReadable{};
    std::array<ComPtr<ID3D12Resource>,2> shaderSnapshots;
    std::array<ComPtr<ID3D12Resource>,2> snapshotPermits;
    std::array<void*,2> permitMappings{};
    struct ShaderSource {
        ComPtr<ID3D12Resource> buffer;UINT64 sourceOffset{};UINT offset{},bytes{};
        texture_read::Lease texture;D3D12_PLACED_SUBRESOURCE_FOOTPRINT layout{};
        UINT mip{},rowBytes{},rows{};
        material_binding::Lease material;std::array<buffer_read::Proof,2> materialProof;
        ComPtr<ID3D12Resource> expectedIndex;
    };
    std::vector<ShaderSource> shaderSources;
    UINT shaderBytes{};bool shadersReadable{},shaderReady=true;
    UINT alphaCaptures{},alphaPendingSequence{},skinPendingSequence{};
    std::array<uint64_t,6> alphaStats{};
    std::array<uint64_t,4> shaderStats{};
    D3D12_RESOURCE_STATES instanceBefore{};bool instanceAugmented{};
    std::map<ID3D12RootSignature*,std::unique_ptr<StereoBindingBank>> arenas;
    // mode, GPU decision, MAIN draw commands, unconditional peer commands,
    // optimized peer ranges, fallback peer batches, native reference draws, commit admitted
    std::array<uint64_t,8> optimized{};
    ComPtr<ID3D12QueryHeap> timestamps;
    std::array<ComPtr<ID3D12Resource>,2> timingReadback;
    std::array<uint64_t,6> gpuTimes{};
    std::array<bool,2> nativeFinished{},timingEnded{};
    ComPtr<ID3D12Resource> readback;ComPtr<ID3D12DescriptorHeap> dsvs;
    ComPtr<ID3D12Fence> mainFence,doneFence;ComPtr<ID3D12CommandQueue> mainQueue;
    ID3D12CommandList* mainList{};ID3D12CommandList* peerList{};
    uint64_t serial=1;
    struct Plane {D3D12_PLACED_SUBRESOURCE_FOOTPRINT layout{};UINT stride{},component{};};
    std::array<Plane,12> planes;
    UINT stride{};UINT64 bytes{};bool initialized{},copied{},mainSubmitted{},peerSubmitted{},failed{};
    DepthComparisonResult result;
    ~Storage(){for(UINT i=0;i<2;++i)if(permitMappings[i]){const D3D12_RANGE written{0,8};snapshotPermits[i]->Unmap(0,&written);}}
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** out)override{if(!out)return E_POINTER;*out=nullptr;if(id!=__uuidof(IUnknown))return E_NOINTERFACE;*out=this;AddRef();return S_OK;}
    ULONG STDMETHODCALLTYPE AddRef()override{return ++refs;}
    ULONG STDMETHODCALLTYPE Release()override{const auto n=--refs;if(!n)delete this;return n;}
    bool Track(ID3D12GraphicsCommandList* list,UINT side){
        if(recordingRetired[side])return false;
        if(recordingTokens[side])return gpu::HasCommandResource(list,recordingTokens[side].Get());
        ComPtr<RecordingToken> token;token.Attach(new(std::nothrow) RecordingToken);
        if(!token || !gpu::KeepCommandResources(list,{this,token.Get()}))return false;
        recordingTokens[side]=std::move(token);return true;
    }
    D3D12_CPU_DESCRIPTOR_HANDLE Dsv(UINT i)const{return {dsvs->GetCPUDescriptorHandleForHeapStart().ptr+SIZE_T(i)*stride};}
    void EndTiming(ID3D12GraphicsCommandList* list,UINT side){
        if(!timestamps || timingEnded[side])return;
        list->EndQuery(timestamps.Get(),D3D12_QUERY_TYPE_TIMESTAMP,side*2+1);
        list->ResolveQueryData(timestamps.Get(),D3D12_QUERY_TYPE_TIMESTAMP,side*2,2,timingReadback[side].Get(),0);
        timingEnded[side]=true;
    }
    bool EnsureShaderStorage(){
        D3D12_RESOURCE_DESC bd{};bd.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;bd.Width=early_depth::MaxInstanceBytes;
        bd.Height=1;bd.DepthOrArraySize=bd.MipLevels=1;bd.SampleDesc.Count=1;bd.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
        D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;
        if(!shaderSnapshots[0]){std::array<ComPtr<ID3D12Resource>,2> buffers;
            for(auto& buffer:buffers)if(FAILED(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&bd,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&buffer))))return false;
            shaderSnapshots=std::move(buffers);
        }
        heap.Type=D3D12_HEAP_TYPE_UPLOAD;bd.Width=8;
        for(UINT i=0;i<2;++i)if(!snapshotPermits[i]){ComPtr<ID3D12Resource> permit;
            if(FAILED(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&bd,D3D12_RESOURCE_STATE_GENERIC_READ,nullptr,IID_PPV_ARGS(&permit))))return false;
            const D3D12_RANGE none{};void* mapped{};if(FAILED(permit->Map(0,&none,&mapped)))return false;
            if(!mapped){permit->Unmap(0,&none);return false;}
            *static_cast<volatile uint64_t*>(mapped)=0;_mm_sfence();snapshotPermits[i]=std::move(permit);permitMappings[i]=mapped;
        }
        return true;
    }
};
EarlyDepthPass::EarlyDepthPass()=default;
EarlyDepthPass::~EarlyDepthPass()=default;
namespace {
void Draw(ID3D12GraphicsCommandList* list,const native_probe::DrawRecord& d){list->DrawIndexedInstanced(d.indices,d.instances,d.firstIndex,d.baseVertex,d.firstInstance);}
struct Restore {
    ID3D12GraphicsCommandList* list;const native_probe::DrawRecord& draw;scene_state::Bindings bindings;
    ComPtr<ID3D12GraphicsCommandList1> views;ComPtr<ID3D12GraphicsCommandList5> rates;
    bool valid{},restoreDepth=true,restoreBindings=true;
    Restore(ID3D12GraphicsCommandList* l,const native_probe::DrawRecord& d,bool depth=true,bool restoreState=true):list(l),draw(d),restoreDepth(depth),restoreBindings(restoreState){
        valid=scene_state::Snapshot(l,bindings) && bindings.rate.known && SUCCEEDED(l->QueryInterface(IID_PPV_ARGS(&views))) &&
            SUCCEEDED(l->QueryInterface(IID_PPV_ARGS(&rates)));
    }
    ~Restore(){if(!valid)return;
        if(restoreBindings)bindings.Restore(list);views->SetViewInstanceMask(UINT_MAX);
        rates->RSSetShadingRateImage(bindings.rate.image);rates->RSSetShadingRate(bindings.rate.value,bindings.rate.combiners.data());
        if(restoreDepth){const D3D12_CPU_DESCRIPTOR_HANDLE depth{draw.depth};list->OMSetRenderTargets(0,nullptr,FALSE,&depth);}
        list->RSSetViewports(1,&draw.viewport);list->RSSetScissorRects(1,&draw.scissor);
        list->SetPipelineState(reinterpret_cast<ID3D12PipelineState*>(draw.pipeline));
    }
};
struct SnapshotPredicate {
    ID3D12GraphicsCommandList* list;
    SnapshotPredicate(ID3D12GraphicsCommandList* command,ID3D12Resource* permit):list(command){list->SetPredication(permit,0,D3D12_PREDICATION_OP_EQUAL_ZERO);}
    ~SnapshotPredicate(){list->SetPredication(nullptr,0,D3D12_PREDICATION_OP_EQUAL_ZERO);}
};
bool CopyReadableBuffer(ID3D12GraphicsCommandList* list,ID3D12Resource* source,D3D12_RESOURCE_STATES state,
                        ID3D12Resource* destination,std::span<const std::pair<UINT64,UINT>> ranges,UINT offset=0){
    constexpr UINT readStates=D3D12_RESOURCE_STATE_VERTEX_AND_CONSTANT_BUFFER|D3D12_RESOURCE_STATE_INDEX_BUFFER|
        D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE|D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE|
        D3D12_RESOURCE_STATE_COPY_SOURCE|D3D12_RESOURCE_STATE_INDIRECT_ARGUMENT;
    if(!(state&D3D12_RESOURCE_STATE_VERTEX_AND_CONSTANT_BUFFER) || (state&~readStates))return false;
    D3D12_RESOURCE_BARRIER barrier{};barrier.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Transition={source,D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,state,D3D12_RESOURCE_STATE_COPY_SOURCE};
    const bool transition=!(state&D3D12_RESOURCE_STATE_COPY_SOURCE);
    if(transition)list->ResourceBarrier(1,&barrier);
    for(const auto [from,bytes]:ranges){list->CopyBufferRegion(destination,offset,source,from,bytes);offset+=bytes;}
    if(transition){std::swap(barrier.Transition.StateBefore,barrier.Transition.StateAfter);list->ResourceBarrier(1,&barrier);}
    return true;
}
}
HRESULT EarlyDepthPass::Create(ID3D12Device* device,UINT width,UINT height,UINT nativeMode)try{
    if(storage_ || !device || !width || !height || width>4096 || height>4096 || nativeMode>3)return E_INVALIDARG;
    const bool wholeTarget=nativeMode!=0;
    ComPtr<Storage> s;s.Attach(new Storage);s->device=device;s->result.width=width;s->result.height=height;s->nativeCounts[0]=nativeMode;
    s->optimized[0]=nativeMode==2;s->optimized[1]=UINT64_MAX;
    probe::Scope internal;HRESULT hr{};
    if(nativeMode){
        const D3D12_QUERY_HEAP_DESC query{D3D12_QUERY_HEAP_TYPE_TIMESTAMP,4,0};
        hr=device->CreateQueryHeap(&query,IID_PPV_ARGS(&s->timestamps));if(FAILED(hr))return hr;
        D3D12_RESOURCE_DESC buffer{};buffer.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;buffer.Width=16;buffer.Height=1;
        buffer.DepthOrArraySize=buffer.MipLevels=1;buffer.SampleDesc.Count=1;buffer.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
        D3D12_HEAP_PROPERTIES memory{};memory.Type=D3D12_HEAP_TYPE_READBACK;
        for(auto& output:s->timingReadback){hr=device->CreateCommittedResource(&memory,D3D12_HEAP_FLAG_NONE,&buffer,
            D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&output));if(FAILED(hr))return hr;}
    }
    if(nativeMode!=3){
    D3D12_DESCRIPTOR_HEAP_DESC hd{D3D12_DESCRIPTOR_HEAP_TYPE_DSV,5,D3D12_DESCRIPTOR_HEAP_FLAG_NONE,0};
    hr=device->CreateDescriptorHeap(&hd,IID_PPV_ARGS(&s->dsvs));if(FAILED(hr))return hr;s->stride=device->GetDescriptorHandleIncrementSize(hd.Type);
    D3D12_RESOURCE_DESC target{};target.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D;target.Width=width;target.Height=height;
    target.DepthOrArraySize=1;target.MipLevels=1;target.Format=DXGI_FORMAT_R32G8X24_TYPELESS;target.SampleDesc.Count=1;target.Flags=D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL;
    D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;
    D3D12_CLEAR_VALUE clear{};clear.Format=DXGI_FORMAT_D32_FLOAT_S8X24_UINT;
    for(auto& resource:s->targets){hr=resource.Initialize(device,target,D3D12_RESOURCE_STATE_DEPTH_WRITE,&clear);if(FAILED(hr))return hr;}
    target.DepthOrArraySize=2;
    for(UINT i=0;i<5;++i){D3D12_DEPTH_STENCIL_VIEW_DESC view{};view.Format=clear.Format;view.ViewDimension=D3D12_DSV_DIMENSION_TEXTURE2DARRAY;
        view.Texture2DArray.ArraySize=i?1:2;view.Texture2DArray.FirstArraySlice=i==2 || i==4?1:0;
        device->CreateDepthStencilView(s->targets[i==1 || i==2?1:0].Resource(),&view,s->Dsv(i));}
    if(nativeMode==2){hr=s->gate.Create(device,s->targets[1].Resource());if(FAILED(hr))return hr;}
    if(nativeMode!=2){
    for(UINT plane=0;plane<2;++plane)for(UINT eye=0;eye<2;++eye)for(UINT mode=0;mode<(wholeTarget?3u:2u);++mode){
        auto& p=s->planes[mode==2?8+plane*2+eye:plane*4+eye*2+mode];UINT rows{};UINT64 rowBytes{},bytes{};
        device->GetCopyableFootprints(&target,eye+plane*2,1,s->bytes,&p.layout,&rows,&rowBytes,&bytes);
        if(bytes==UINT64_MAX || rows!=height || rowBytes%width)return E_INVALIDARG;p.stride=UINT(rowBytes/width);
        if((!plane && p.stride!=4 && p.stride!=8) || (plane && p.stride!=1 && p.stride!=8))return E_NOTIMPL;
        p.component=plane && p.stride==8?4:0;s->bytes=(p.layout.Offset+UINT64(p.layout.Footprint.RowPitch)*rows+511)&~UINT64(511);
    }
    if(s->bytes>512*1024*1024)return E_OUTOFMEMORY;
    auto buffer=target;buffer.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;buffer.Width=s->bytes;buffer.Height=1;buffer.DepthOrArraySize=1;
    buffer.Format=DXGI_FORMAT_UNKNOWN;buffer.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;buffer.Flags=D3D12_RESOURCE_FLAG_NONE;heap.Type=D3D12_HEAP_TYPE_READBACK;
    hr=device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&buffer,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&s->readback));if(FAILED(hr))return hr;
    }
    }
    hr=device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&s->mainFence));if(FAILED(hr))return hr;
    hr=device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&s->doneFence));if(FAILED(hr))return hr;
    storage_=std::move(s);return S_OK;
}catch(const std::bad_alloc&){return E_OUTOFMEMORY;}
bool EarlyDepthPass::CaptureInstances(ID3D12GraphicsCommandList* list,ID3D12Resource* source,UINT64 offset,UINT bytes,D3D12_RESOURCE_STATES state,bool combinedRead)try{
    if(!storage_ || !list || !source || storage_->nativeCounts[0]!=2 || storage_->mainList!=list ||
        storage_->mainSubmitted || storage_->nativeFinished[0] || storage_->failed || !bytes || bytes%4 || offset%4)return false;
    auto& s=*storage_.Get();const auto desc=source->GetDesc();ComPtr<ID3D12Device> device;
    if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER || (desc.Flags&D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS) ||
        offset>desc.Width || bytes>desc.Width-offset || s.instanceRanges.size()>=early_depth::MaxDraws ||
        bytes>early_depth::MaxInstanceBytes-s.instanceBytes || (s.instanceSource && s.instanceSource.Get()!=source) ||
        FAILED(source->GetDevice(IID_PPV_ARGS(&device))) || device.Get()!=s.device.Get())return false;
    constexpr UINT readStates=D3D12_RESOURCE_STATE_VERTEX_AND_CONSTANT_BUFFER|D3D12_RESOURCE_STATE_INDEX_BUFFER|
        D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE|D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE|
        D3D12_RESOURCE_STATE_COPY_SOURCE|D3D12_RESOURCE_STATE_INDIRECT_ARGUMENT;
    if(!(state&D3D12_RESOURCE_STATE_VERTEX_AND_CONSTANT_BUFFER) || (state&~readStates) ||
        (s.instanceAugmented && state!=s.instanceBefore))return false;
    probe::Scope internal;
    if(!s.instanceSnapshots[0]){
        D3D12_RESOURCE_DESC buffer{};buffer.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;buffer.Width=early_depth::MaxInstanceBytes;
        buffer.Height=1;buffer.DepthOrArraySize=buffer.MipLevels=1;buffer.SampleDesc.Count=1;buffer.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
        D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;
        std::array<ComPtr<ID3D12Resource>,2> pending;
        for(auto& item:pending)if(FAILED(s.device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&buffer,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&item))))return false;
        s.instanceSnapshots=std::move(pending);
    }
    if(!gpu::KeepCommandResources(list,{&s,source}))return false;
    const std::pair<UINT64,UINT> range{offset,bytes};
    // Reserve before recording: a later allocation failure cannot leave an
    // untracked GPU copy whose range was omitted from the admission proof.
    s.instanceRanges.reserve(early_depth::MaxDraws);
    if(s.instancesReadable){
        D3D12_RESOURCE_BARRIER barriers[2]{};
        for(UINT i=0;i<2;++i){barriers[i].Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
            barriers[i].Transition={s.instanceSnapshots[i].Get(),D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,D3D12_RESOURCE_STATE_COPY_DEST};}
        list->ResourceBarrier(2,barriers);s.instancesReadable=false;
    }
    if(!combinedRead){
        ReleaseInstanceRead(list);
        if(!CopyReadableBuffer(list,source,state,s.instanceSnapshots[0].Get(),{&range,1},s.instanceBytes))return false;
    }else {
    if(!s.instanceAugmented && !(state&D3D12_RESOURCE_STATE_COPY_SOURCE)){
        D3D12_RESOURCE_BARRIER barrier{};barrier.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        barrier.Transition={source,D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,state,state|D3D12_RESOURCE_STATE_COPY_SOURCE};
        list->ResourceBarrier(1,&barrier);s.instanceBefore=state;s.instanceAugmented=true;
    }
    list->CopyBufferRegion(s.instanceSnapshots[0].Get(),s.instanceBytes,source,offset,bytes);
    }
    s.instanceSource=source;s.instanceRanges.push_back(range);s.instanceBytes+=bytes;return true;
}catch(const std::bad_alloc&){return false;}
void EarlyDepthPass::ReleaseInstanceRead(ID3D12GraphicsCommandList* list){
    if(!storage_ || !storage_->instanceAugmented || list!=storage_->mainList)return;
    auto& s=*storage_.Get();probe::Scope internal;
    D3D12_RESOURCE_BARRIER barrier{};barrier.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Transition={s.instanceSource.Get(),D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
        s.instanceBefore|D3D12_RESOURCE_STATE_COPY_SOURCE,s.instanceBefore};
    list->ResourceBarrier(1,&barrier);s.instanceAugmented=false;
}
ID3D12Resource* EarlyDepthPass::InstanceSource()const{return storage_?storage_->instanceSource.Get():nullptr;}
std::array<uint64_t,2> EarlyDepthPass::InstanceProofCounts()const{return storage_?std::array<uint64_t,2>{storage_->instanceRanges.size(),storage_->instanceBytes}:std::array<uint64_t,2>{};}
bool EarlyDepthPass::ShaderSource(ID3D12Resource* resource)const{
    return storage_ && std::any_of(storage_->shaderSources.begin(),storage_->shaderSources.end(),[&](const auto& source){
        return !resource || source.buffer.Get()==resource || source.texture.resource.Get()==resource || source.material.source.resource.Get()==resource;});
}
std::array<uint64_t,4> EarlyDepthPass::ShaderProofCounts()const{return storage_?storage_->shaderStats:std::array<uint64_t,4>{};}
void EarlyDepthPass::RejectShaderSnapshots(){if(storage_){storage_->shaderReady=false;CommitNative(false);}}
bool EarlyDepthPass::CaptureSkinning(ID3D12GraphicsCommandList* list,ID3D12Resource* bones,std::span<const skinning::BoneRange> ranges,
                                    const texture_read::Lease& texture,UINT sequence)try{
    if(storage_)storage_->skinPendingSequence=0;
    if(!storage_ || !list || !bones || !texture.resource || ranges.empty() || storage_->nativeCounts[0]!=2 ||
        storage_->mainList!=list || storage_->mainSubmitted || storage_->nativeFinished[0] || storage_->failed ||
        !storage_->shaderReady || storage_->alphaPendingSequence)return false;
    auto& s=*storage_.Get();
    if(ranges.size()>=DepthCommitGate::MaxBufferProofs ||
        s.shaderSources.size()+ranges.size()+1+s.alphaCaptures>DepthCommitGate::MaxBufferProofs-1)return false;
    const auto desc=bones->GetDesc();D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS flags{};ComPtr<ID3D12Device> device;
    if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER || (desc.Flags&D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS) ||
        FAILED(bones->GetDevice(IID_PPV_ARGS(&device))) || device!=s.device || FAILED(bones->GetHeapProperties(&heap,&flags)) ||
        (heap.Type!=D3D12_HEAP_TYPE_UPLOAD && heap.Type!=D3D12_HEAP_TYPE_GPU_UPLOAD))return false;
    std::vector<Storage::ShaderSource> pending;pending.reserve(ranges.size()+1);uint64_t bytes=s.shaderBytes,boneBytes{};
    for(const auto range:ranges){
        bytes=(bytes+511)&~511ull;
        if(!range.bytes || range.offset%4 || range.bytes%4 || range.offset>desc.Width || range.bytes>desc.Width-range.offset ||
            bytes>early_depth::MaxInstanceBytes || range.bytes>early_depth::MaxInstanceBytes-bytes)return false;
        Storage::ShaderSource source;source.buffer=bones;source.sourceOffset=range.offset;source.offset=UINT(bytes);source.bytes=UINT(range.bytes);
        pending.push_back(std::move(source));bytes+=range.bytes;boneBytes+=range.bytes;
    }
    const auto td=texture.resource->GetDesc();Storage::ShaderSource image;image.texture=texture;UINT rows{};UINT64 rowBytes{},total{};
    if(td.Dimension!=D3D12_RESOURCE_DIMENSION_TEXTURE2D || td.Format!=DXGI_FORMAT_R8_UNORM || td.MipLevels!=1 || td.DepthOrArraySize!=1 || td.SampleDesc.Count!=1)return false;
    s.device->GetCopyableFootprints(&td,0,1,(bytes+511)&~511ull,&image.layout,&rows,&rowBytes,&total);
    const auto span=uint64_t(image.layout.Footprint.RowPitch)*(rows?rows-1:0)+((rowBytes+3)&~3ull);
    if(total==UINT64_MAX || rows!=td.Height || rowBytes!=td.Width || !rows || !rowBytes || image.layout.Offset>early_depth::MaxInstanceBytes ||
        span>early_depth::MaxInstanceBytes-image.layout.Offset)return false;
    image.offset=UINT(image.layout.Offset);image.bytes=UINT(span);image.rowBytes=UINT(rowBytes);image.rows=rows;bytes=image.layout.Offset+span;pending.push_back(std::move(image));
    s.shaderSources.reserve(DepthCommitGate::MaxBufferProofs-1);probe::Scope internal;
    if(!s.EnsureShaderStorage())return false;
    if(!gpu::KeepCommandResources(list,{&s,bones,texture.resource.Get()}))return false;
    if(s.shadersReadable){D3D12_RESOURCE_BARRIER barriers[2]{};
        for(UINT i=0;i<2;++i){barriers[i].Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
            barriers[i].Transition={s.shaderSnapshots[i].Get(),D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,D3D12_RESOURCE_STATE_COPY_DEST};}
        list->ResourceBarrier(2,barriers);s.shadersReadable=false;
    }
    {SnapshotPredicate predicate(list,s.snapshotPermits[0].Get());for(const auto& source:pending){
        if(source.buffer)list->CopyBufferRegion(s.shaderSnapshots[0].Get(),source.offset,source.buffer.Get(),source.sourceOffset,source.bytes);
        else if(!texture_read::Copy(list,source.texture,s.shaderSnapshots[0].Get(),source.layout))return false;
    }}
    for(auto& source:pending)s.shaderSources.push_back(std::move(source));
    s.shaderBytes=UINT(bytes);s.skinPendingSequence=sequence;++s.shaderStats[0];s.shaderStats[2]+=boneBytes;s.shaderStats[3]+=rowBytes*rows;return true;
}catch(const std::bad_alloc&){if(storage_)storage_->skinPendingSequence=0;return false;}
bool EarlyDepthPass::CaptureAlpha(ID3D12GraphicsCommandList* list,const material_binding::Lease& material,
    const texture_read::Lease& texture,std::span<const UINT> mips,UINT expectedIndex,UINT sequence)try{
    if(!storage_)return false;auto& s=*storage_.Get();
    auto reject=[&](uint64_t reason){s.skinPendingSequence=0;s.alphaStats[5]=reason;return false;};
    if(!list || !material.source.resource || !texture.resource || mips.empty() || expectedIndex>=32768 || !sequence ||
        storage_->nativeCounts[0]!=2 || storage_->mainList!=list || storage_->mainSubmitted || storage_->nativeFinished[0] ||
        storage_->failed || !storage_->shaderReady || storage_->alphaPendingSequence ||
        (storage_->skinPendingSequence && storage_->skinPendingSequence!=sequence))return reject(1);
    if(mips.size()>15 || s.shaderSources.size()+mips.size()+2+s.alphaCaptures>DepthCommitGate::MaxBufferProofs-1)return reject(2);
    const auto desc=texture.resource->GetDesc();ComPtr<ID3D12Device> device;
    if(FAILED(texture.resource->GetDevice(IID_PPV_ARGS(&device))) || device!=s.device ||
        desc.Dimension!=D3D12_RESOURCE_DIMENSION_TEXTURE2D || desc.DepthOrArraySize!=1 || desc.SampleDesc.Count!=1 ||
        desc.SampleDesc.Quality || desc.Flags || (desc.Format!=DXGI_FORMAT_BC1_UNORM_SRGB && desc.Format!=DXGI_FORMAT_BC7_UNORM_SRGB))return reject(3);
    std::vector<Storage::ShaderSource> pending;pending.reserve(mips.size()+1);
    uint64_t bytes=(uint64_t(s.shaderBytes)+511)&~511ull,textureBytes{};
    if(bytes>early_depth::MaxInstanceBytes || material_binding::LogicalBytes>early_depth::MaxInstanceBytes-bytes)return reject(4);
    Storage::ShaderSource b4;b4.material=material;b4.offset=UINT(bytes);b4.bytes=material_binding::LogicalBytes;
    bytes+=b4.bytes;pending.push_back(std::move(b4));
    for(size_t i=0;i<mips.size();++i){
        if(mips[i]>=desc.MipLevels || (i && mips[i]!=mips[i-1]+1))return reject(4);
        Storage::ShaderSource image;image.texture=texture;image.mip=mips[i];UINT rows{};UINT64 rowBytes{},total{};
        s.device->GetCopyableFootprints(&desc,image.mip,1,(bytes+511)&~511ull,&image.layout,&rows,&rowBytes,&total);
        const auto span=uint64_t(image.layout.Footprint.RowPitch)*(rows?rows-1:0)+((rowBytes+3)&~3ull);
        if(total==UINT64_MAX || !rows || !rowBytes || rowBytes>UINT_MAX || rows>65535 ||
            image.layout.Offset>early_depth::MaxInstanceBytes || span>early_depth::MaxInstanceBytes-image.layout.Offset)return reject(4);
        image.offset=UINT(image.layout.Offset);image.bytes=UINT(span);image.rowBytes=UINT(rowBytes);image.rows=rows;
        bytes=image.layout.Offset+span;textureBytes+=rowBytes*rows;pending.push_back(std::move(image));
    }
    s.shaderSources.reserve(DepthCommitGate::MaxBufferProofs-1);probe::Scope internal;
    if(!s.EnsureShaderStorage())return reject(5);
    D3D12_RESOURCE_DESC expected{};expected.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;expected.Width=16;
    expected.Height=1;expected.DepthOrArraySize=expected.MipLevels=1;expected.SampleDesc.Count=1;expected.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_UPLOAD;
    auto& constant=pending.front().expectedIndex;
    if(FAILED(s.device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&expected,D3D12_RESOURCE_STATE_GENERIC_READ,nullptr,IID_PPV_ARGS(&constant))))return reject(6);
    const D3D12_RANGE none{};void* mapped{};if(FAILED(constant->Map(0,&none,&mapped)))return reject(6);
    if(!mapped){constant->Unmap(0,&none);return reject(6);}
    const std::array<UINT,4> value{expectedIndex,0,0,0};std::memcpy(mapped,value.data(),sizeof(value));
    const D3D12_RANGE written{0,sizeof(value)};constant->Unmap(0,&written);_mm_sfence();
    if(!gpu::KeepCommandResources(list,{&s,material.source.resource.Get(),texture.resource.Get(),constant.Get()}))return reject(7);
    if(s.shadersReadable){D3D12_RESOURCE_BARRIER barriers[2]{};
        for(UINT i=0;i<2;++i){barriers[i].Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
            barriers[i].Transition={s.shaderSnapshots[i].Get(),D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,D3D12_RESOURCE_STATE_COPY_DEST};}
        list->ResourceBarrier(2,barriers);s.shadersReadable=false;
    }
    {SnapshotPredicate predicate(list,s.snapshotPermits[0].Get());for(auto& source:pending){
        const bool copied=source.material.source.resource?
            material_read::CopySnapshot(list,source.material,s.shaderSnapshots[0].Get(),source.offset,source.materialProof[0]):
            texture_read::CopySubresource(list,source.texture,source.mip,s.shaderSnapshots[0].Get(),source.layout);
        if(!copied){const auto failure=source.material.source.resource?material_read::LastFailure():texture_read::LastFailure();
            s.failed|=failure==40;RejectShaderSnapshots();return reject((source.material.source.resource?1000u:2000u)+failure);}
    }}
    for(auto& source:pending)s.shaderSources.push_back(std::move(source));
    s.shaderBytes=UINT(bytes);++s.alphaCaptures;s.alphaPendingSequence=sequence;
    ++s.alphaStats[0];s.alphaStats[3]+=material_binding::LogicalBytes;s.alphaStats[4]+=textureBytes;
    ++s.shaderStats[0];s.shaderStats[2]+=material_binding::LogicalBytes;s.shaderStats[3]+=textureBytes;return true;
}catch(const std::bad_alloc&){if(storage_){storage_->skinPendingSequence=0;storage_->alphaStats[5]=8;}return false;}
std::array<uint64_t,6> EarlyDepthPass::AlphaProofCounts()const{return storage_?storage_->alphaStats:std::array<uint64_t,6>{};}
bool EarlyDepthPass::StartNative(ID3D12GraphicsCommandList* list,const native_probe::DrawRecord& d,ID3D12Resource* target,
                                ID3D12Resource* instanceSource,D3D12_RESOURCE_STATES instanceState){
    if(!storage_ || !list || !target || d.side>1 || d.rtCount || !d.depth || (d.flags&64) || !storage_->nativeCounts[0] || storage_->failed || storage_->copied)return false;
    auto& s=*storage_.Get();const auto side=d.side;auto*& active=side?s.peerList:s.mainList;
    if(s.native[side])return s.native[side].Get()==target && s.nativeDsv[side]==d.depth && active==list;
    if((!side && s.mainSubmitted) || (side && !s.mainSubmitted) || (active && active!=list))return false;
    const auto desc=target->GetDesc();ComPtr<ID3D12Device> actual,listDevice;
    if(list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT || FAILED(list->GetDevice(IID_PPV_ARGS(&listDevice))) || listDevice.Get()!=s.device.Get())return false;
    if(FAILED(target->GetDevice(IID_PPV_ARGS(&actual))) || actual.Get()!=s.device.Get() || desc.Width!=s.result.width || desc.Height!=s.result.height ||
        desc.Dimension!=D3D12_RESOURCE_DIMENSION_TEXTURE2D || desc.DepthOrArraySize!=1 || desc.MipLevels!=1 || desc.SampleDesc.Count!=1 || desc.SampleDesc.Quality ||
        (desc.Format!=DXGI_FORMAT_R32G8X24_TYPELESS && desc.Format!=DXGI_FORMAT_D32_FLOAT_S8X24_UINT) || !(desc.Flags&D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL))return false;
    probe::Scope internal;if(!s.Track(list,side) || !gpu::KeepCommandResources(list,{&s,target}))return false;
    list->EndQuery(s.timestamps.Get(),D3D12_QUERY_TYPE_TIMESTAMP,side*2);
    if(s.nativeCounts[0]==3){s.native[side]=target;s.nativeDsv[side]=d.depth;active=list;s.initialized=true;return true;}
    // Every paired layer is fully imported before its first draw. The reference
    // array is overwritten at FinishNative, or initialized here for the GPU gate.
    s.initialized=true;
    const auto eye=side?0u:1u;
    if(s.nativeCounts[0]==2 && FAILED(s.targets[1].ImportEye(list,eye,D3D12_RESOURCE_STATE_DEPTH_WRITE,target,D3D12_RESOURCE_STATE_DEPTH_WRITE)))return false;
    if(!side){
        if(FAILED(s.targets[0].ImportEyes(list,3,D3D12_RESOURCE_STATE_DEPTH_WRITE,target,D3D12_RESOURCE_STATE_DEPTH_WRITE)))return false;
    }else if(s.nativeCounts[0]==2){
        std::vector<DepthCommitGate::BufferProof> proofs;proofs.reserve(1+s.shaderSources.size()+s.alphaCaptures);
        if(s.instanceBytes){
            if(instanceSource!=s.instanceSource.Get() || !CopyReadableBuffer(list,instanceSource,instanceState,
                s.instanceSnapshots[1].Get(),s.instanceRanges))return false;
            D3D12_RESOURCE_BARRIER barriers[2]{};
            for(UINT i=0;i<2;++i){barriers[i].Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                barriers[i].Transition={s.instanceSnapshots[i].Get(),D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                    D3D12_RESOURCE_STATE_COPY_DEST,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE};}
            list->ResourceBarrier(2,barriers);s.instancesReadable=true;
            proofs.push_back({s.instanceSnapshots[0].Get(),s.instanceSnapshots[1].Get(),s.instanceBytes});
        }
        if(!s.shaderSources.empty()){SnapshotPredicate predicate(list,s.snapshotPermits[1].Get());for(auto& source:s.shaderSources){
            if(source.material.source.resource){
                if(!material_read::CopySnapshot(list,source.material,s.shaderSnapshots[1].Get(),source.offset,source.materialProof[1])){
                    s.alphaStats[5]=1000u+material_read::LastFailure();s.shaderReady=false;if(material_read::LastFailure()==40)return false;break;
                }
                ++s.alphaStats[1];
            }else if(source.buffer)list->CopyBufferRegion(s.shaderSnapshots[1].Get(),source.offset,source.buffer.Get(),source.sourceOffset,source.bytes);
            else if(!(source.texture.resource->GetDesc().Format==DXGI_FORMAT_R8_UNORM?
                texture_read::Copy(list,source.texture,s.shaderSnapshots[1].Get(),source.layout):
                texture_read::CopySubresource(list,source.texture,source.mip,s.shaderSnapshots[1].Get(),source.layout))){
                if(source.texture.resource->GetDesc().Format!=DXGI_FORMAT_R8_UNORM)s.alphaStats[5]=2000u+texture_read::LastFailure();
                s.shaderReady=false;if(texture_read::LastFailure()==40)return false;break;
            }
            if(source.texture.resource && source.texture.resource->GetDesc().Format!=DXGI_FORMAT_R8_UNORM)++s.alphaStats[2];
            ++s.shaderStats[1];
        }}
        if(!s.shaderSources.empty() && s.shaderReady){D3D12_RESOURCE_BARRIER barriers[2]{};
            for(UINT i=0;i<2;++i){barriers[i].Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                barriers[i].Transition={s.shaderSnapshots[i].Get(),D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,D3D12_RESOURCE_STATE_COPY_DEST,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE};}
            list->ResourceBarrier(2,barriers);s.shadersReadable=true;
            for(const auto& source:s.shaderSources){DepthCommitGate::BufferProof proof{s.shaderSnapshots[0].Get(),s.shaderSnapshots[1].Get(),source.bytes};
                proof.mainOffset=proof.peerOffset=source.offset;
                if(source.texture.resource){proof.rowBytes=source.rowBytes;proof.rowPitch=source.layout.Footprint.RowPitch;proof.rows=source.rows;}
                proofs.push_back(proof);
                if(source.expectedIndex){DepthCommitGate::BufferProof index{s.shaderSnapshots[0].Get(),source.expectedIndex.Get(),4};
                    index.mainOffset=source.offset;proofs.push_back(index);}}
        }
        if(!s.gate.Prepare(list,d,proofs) || !s.gate.Bind(list,true))return false;
        const auto copied=s.targets[0].CopyEye(list,0,D3D12_RESOURCE_STATE_DEPTH_WRITE,target,D3D12_RESOURCE_STATE_DEPTH_WRITE,D3D12_RESOURCE_STATE_DEPTH_WRITE);
        list->SetPredication(nullptr,0,D3D12_PREDICATION_OP_EQUAL_ZERO);if(FAILED(copied))return false;
    }
    s.native[side]=target;s.nativeDsv[side]=d.depth;active=list;++s.nativeCounts[7];return true;
}
bool EarlyDepthPass::ObserveNative(ID3D12GraphicsCommandList* list,const native_probe::DrawRecord& d){
    if(!storage_ || !list || d.side>1 || storage_->nativeCounts[0]!=3 || storage_->failed || storage_->copied ||
        storage_->nativeFinished[d.side] || !storage_->native[d.side] || d.depth!=storage_->nativeDsv[d.side] ||
        list!=(d.side?storage_->peerList:storage_->mainList))return false;
    ++storage_->nativeCounts[1+d.side];if(!d.side)++storage_->result.draws;return true;
}
bool EarlyDepthPass::Original(ID3D12GraphicsCommandList* list,const native_probe::DrawRecord& d){
    if(d.side==1){const InstanceRun all{0,d.instances};return PeerRemainder(list,d,{&all,1});}
    if(!storage_ || !list || !storage_->nativeCounts[0] || storage_->nativeCounts[0]>2 || storage_->nativeFinished[0] || !storage_->native[0] || storage_->mainList!=list || storage_->mainSubmitted ||
        d.side || d.rtCount || d.depth!=storage_->nativeDsv[0] || storage_->failed || !d.indices || !d.instances)return false;
    auto& s=*storage_.Get();probe::Scope internal;if(!gpu::KeepCommandResources(list,{&s}))return false;
    auto target=s.Dsv(4);list->OMSetRenderTargets(0,nullptr,FALSE,&target);Draw(list,d);
    if(s.nativeCounts[0]!=2){target={d.depth};list->OMSetRenderTargets(0,nullptr,FALSE,&target);Draw(list,d);++s.optimized[6];}
    if(s.nativeCounts[0]==2)++s.optimized[2];
    s.skinPendingSequence=0;++s.result.draws;++s.nativeCounts[1];++s.nativeCounts[3];return true;
}
bool EarlyDepthPass::PeerRemainder(ID3D12GraphicsCommandList* list,const native_probe::DrawRecord& d,std::span<const InstanceRun> remaining){
    if(!storage_ || !list || !storage_->nativeCounts[0] || storage_->nativeCounts[0]>2 || storage_->nativeFinished[1] || !storage_->native[1] || storage_->peerList!=list || !storage_->mainSubmitted ||
        d.side!=1 || d.rtCount || d.depth!=storage_->nativeDsv[1] || storage_->failed || storage_->copied || !d.indices || !d.instances ||
        d.firstInstance>UINT_MAX-d.instances)return false;
    UINT end{};for(const auto run:remaining){if(!run.count || run.first<end || run.first>d.instances || run.count>d.instances-run.first)return false;end=run.first+run.count;}
    auto& s=*storage_.Get();probe::Scope internal;if(!gpu::KeepCommandResources(list,{&s}))return false;
    if(s.nativeCounts[0]==2){
        const D3D12_CPU_DESCRIPTOR_HANDLE target{d.depth};list->OMSetRenderTargets(0,nullptr,FALSE,&target);
        if(remaining.size()==1 && !remaining[0].first && remaining[0].count==d.instances){Draw(list,d);++s.optimized[3];}
        else {
            if(!s.gate.Bind(list,true))return false;
            for(const auto run:remaining){auto part=d;part.firstInstance+=run.first;part.instances=run.count;Draw(list,part);}
            if(!s.gate.Bind(list,false)){list->SetPredication(nullptr,0,D3D12_PREDICATION_OP_EQUAL_ZERO);return false;}
            Draw(list,d);list->SetPredication(nullptr,0,D3D12_PREDICATION_OP_EQUAL_ZERO);
            s.optimized[4]+=remaining.size();++s.optimized[5];
        }
        ++s.nativeCounts[2];s.nativeCounts[4]+=UINT(remaining.size());return true;
    }
    auto target=s.Dsv(3);list->OMSetRenderTargets(0,nullptr,FALSE,&target);
    for(const auto run:remaining){auto part=d;part.firstInstance+=run.first;part.instances=run.count;Draw(list,part);}
    target={d.depth};list->OMSetRenderTargets(0,nullptr,FALSE,&target);Draw(list,d);
    ++s.optimized[6];
    target={d.depth};list->OMSetRenderTargets(0,nullptr,FALSE,&target);
    ++s.nativeCounts[2];s.nativeCounts[4]+=UINT(remaining.size());return true;
}
bool EarlyDepthPass::FinishNative(ID3D12GraphicsCommandList* list,UINT side){
    if(!storage_ || !list || side>1 || !storage_->nativeCounts[0] || !storage_->native[side] || storage_->copied ||
        list!=(side?storage_->peerList:storage_->mainList) || storage_->nativeFinished[side])return false;
    auto& s=*storage_.Get();probe::Scope internal;if(!gpu::KeepCommandResources(list,{&s}))return false;
    if(!side)ReleaseInstanceRead(list);
    if(s.nativeCounts[0]==3){s.nativeFinished[side]=true;if(!side)s.EndTiming(list,side);return true;}
    if(s.nativeCounts[0]==2){
        if(!side && FAILED(s.targets[0].CopyEye(list,1,D3D12_RESOURCE_STATE_DEPTH_WRITE,s.native[0].Get(),
            D3D12_RESOURCE_STATE_DEPTH_WRITE,D3D12_RESOURCE_STATE_DEPTH_WRITE)))return false;
        const D3D12_CPU_DESCRIPTOR_HANDLE target{s.nativeDsv[side]};list->OMSetRenderTargets(0,nullptr,FALSE,&target);
        s.nativeFinished[side]=true;if(!side)s.EndTiming(list,side);++s.nativeCounts[5+side];return true;
    }
    // Capture actual native 2D output, then verify a round-trip through the
    // array. The paired result remains independent, including its view type.
    if(FAILED(s.targets[1].ImportEye(list,side?0:1,D3D12_RESOURCE_STATE_DEPTH_WRITE,s.native[side].Get(),D3D12_RESOURCE_STATE_DEPTH_WRITE)))return false;
    const auto hr=s.targets[1].CopyEye(list,side?0:1,D3D12_RESOURCE_STATE_DEPTH_WRITE,s.native[side].Get(),
        D3D12_RESOURCE_STATE_DEPTH_WRITE,D3D12_RESOURCE_STATE_DEPTH_WRITE);
    if(FAILED(hr))return false;
    D3D12_RESOURCE_BARRIER barrier{};barrier.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Transition={s.native[side].Get(),D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,D3D12_RESOURCE_STATE_DEPTH_WRITE,D3D12_RESOURCE_STATE_COPY_SOURCE};
    list->ResourceBarrier(1,&barrier);
    for(UINT plane=0;plane<2;++plane){D3D12_TEXTURE_COPY_LOCATION from{},to{};
        from.pResource=s.native[side].Get();from.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;from.SubresourceIndex=plane;
        to.pResource=s.readback.Get();to.Type=D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;to.PlacedFootprint=s.planes[8+plane*2+(side?0:1)].layout;
        list->CopyTextureRegion(&to,0,0,0,&from,nullptr);
    }
    std::swap(barrier.Transition.StateBefore,barrier.Transition.StateAfter);list->ResourceBarrier(1,&barrier);
    s.nativeFinished[side]=true;if(!side)s.EndTiming(list,side);++s.nativeCounts[5+side];return true;
}
std::array<uint32_t,8> EarlyDepthPass::NativeCounts()const{return storage_?storage_->nativeCounts:std::array<uint32_t,8>{};}
std::array<uint64_t,4> EarlyDepthPass::NativeDifferences()const{return storage_?storage_->nativeDifferences:std::array<uint64_t,4>{};}
std::array<uint64_t,8> EarlyDepthPass::OptimizedCounts()const{return storage_?storage_->optimized:std::array<uint64_t,8>{};}
std::array<uint64_t,6> EarlyDepthPass::GpuTimes()const{return storage_?storage_->gpuTimes:std::array<uint64_t,6>{};}
bool EarlyDepthPass::CommitNative(bool allow){
    if(!storage_ || storage_->nativeCounts[0]!=2)return false;
    allow=allow && storage_->shaderReady;if(!storage_->gate.Decide(allow))return false;
    storage_->optimized[7]=allow;return true;
}
void EarlyDepthPass::AbortNative(ID3D12GraphicsCommandList* list,UINT side){
    if(!storage_)return;auto& s=*storage_.Get();
    if(s.nativeCounts[0]==2){s.gate.Decide(false);s.optimized[7]=0;}
    FinishNative(list,side);
}
bool EarlyDepthPass::Main(ID3D12GraphicsCommandList* list,const native_probe::DrawRecord& draw,const families::Family& family,StereoBindingBank& bank,
    std::span<const uint8_t> masks,std::span<const uint8_t> peerOnly,UINT bankDraw,bool nativeCamera,bool alphaReference,bool alphaOptimized){
    if(!storage_ || !list || storage_->nativeCounts[0]>2 || storage_->nativeFinished[0] || draw.side!=0 || draw.rtCount || !draw.depth || !family.pipeline || storage_->failed || storage_->mainSubmitted ||
        (storage_->mainList && storage_->mainList!=list) || (!masks.empty() && (masks.size()!=draw.instances ||
        !family.instanceOrdinalIndependent || draw.firstInstance>UINT_MAX-draw.instances ||
        std::any_of(masks.begin(),masks.end(),[](auto mask){return mask!=2 && mask!=3;}))))return false;
    if(storage_->nativeCounts[0] && (!storage_->native[0] || storage_->nativeDsv[0]!=draw.depth))return false;
    const auto stride=draw.vertices[7].StrideInBytes;
    if(!peerOnly.empty() && (!family.instanceOrdinalIndependent || !stride || stride>256 || peerOnly.size()%stride ||
        peerOnly.size()>16384 || !draw.vertices[7].BufferLocation || !draw.vertices[7].SizeInBytes))return false;
    const auto& pass=family.pass;
    const bool alphaMode=(alphaReference && !alphaOptimized && storage_->nativeCounts[0]==1) ||
        (alphaOptimized && !alphaReference && storage_->nativeCounts[0]==2 && storage_->shaderReady &&
         storage_->alphaPendingSequence && storage_->alphaPendingSequence==draw.sequence);
    const bool combinedAlpha=family.alphaDepthContract==2 && family.skinningReadContract==2;
    const bool alphaContract=family.alphaDepthContract==1 || combinedAlpha;
    const bool jointCapture=!combinedAlpha || !alphaOptimized || storage_->skinPendingSequence==draw.sequence;
    const bool alphaAllowed=alphaMode && nativeCamera && alphaContract && jointCapture && family.nativeCameraStages==1 && pass.pixelShader;
    if(((alphaReference || alphaOptimized) && !alphaAllowed) || (pass.pixelShader && !alphaAllowed) || pass.targets.NumRenderTargets || pass.depth!=DXGI_FORMAT_D32_FLOAT_S8X24_UINT || !pass.fixedRasterState ||
        !pass.depthStateKnown || !pass.depthState.DepthEnable || pass.depthState.DepthWriteMask!=D3D12_DEPTH_WRITE_MASK_ALL ||
        (pass.depthState.DepthFunc!=D3D12_COMPARISON_FUNC_GREATER && pass.depthState.DepthFunc!=D3D12_COMPARISON_FUNC_GREATER_EQUAL) ||
        pass.samples.Count!=1 || pass.samples.Quality || (!nativeCamera && (bankDraw>=bank.DrawCount() || bank.SnapshotCount())) ||
        (nativeCamera && (!family.nativeCameraPipeline || draw.root!=reinterpret_cast<uintptr_t>(family.sourceRoot.Get()))) || (draw.flags&64) ||
        !draw.indices || !draw.instances || draw.viewport.TopLeftX || draw.viewport.TopLeftY || draw.viewport.MinDepth!=0 || draw.viewport.MaxDepth!=1 ||
        draw.viewport.Width!=storage_->result.width || draw.viewport.Height!=storage_->result.height)return false;
    auto& s=*storage_.Get();probe::Scope internal;Restore restore(list,draw,s.nativeCounts[0]!=2,!nativeCamera);if(!restore.valid || !s.Track(list,0))return false;
    if(nativeCamera && !gpu::KeepCommandResources(list,{family.nativeCameraPipeline.Get(),family.sourceRoot.Get()}))return false;
    ComPtr<ID3D12Resource> extra;
    if(!peerOnly.empty()){
        D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_UPLOAD;D3D12_RESOURCE_DESC buffer{};
        buffer.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;buffer.Width=peerOnly.size();buffer.Height=1;
        buffer.DepthOrArraySize=buffer.MipLevels=1;buffer.SampleDesc.Count=1;
        buffer.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
        if(FAILED(s.device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&buffer,D3D12_RESOURCE_STATE_GENERIC_READ,nullptr,IID_PPV_ARGS(&extra))))return false;
        void* data{};const D3D12_RANGE none{};if(FAILED(extra->Map(0,&none,&data)))return false;
        if(!data){extra->Unmap(0,&none);return false;}
        std::memcpy(data,peerOnly.data(),peerOnly.size());const D3D12_RANGE written{0,peerOnly.size()};extra->Unmap(0,&written);
        if(!gpu::KeepCommandResources(list,{extra.Get()}))return false;
    }
    if(!nativeCamera && !bank.CapturedFor(list) && (!bank.CaptureEye(list,0) || !bank.CaptureEye(list,1)))return false;
    s.mainList=list;
    if(!s.initialized){for(UINT i=0;i<3;++i)list->ClearDepthStencilView(s.Dsv(i),D3D12_CLEAR_FLAG_DEPTH|D3D12_CLEAR_FLAG_STENCIL,0,0,0,nullptr);s.initialized=true;}
    const auto paired=s.Dsv(0),reference=s.nativeCounts[0]?D3D12_CPU_DESCRIPTOR_HANDLE{draw.depth}:s.Dsv(2);
    restore.rates->RSSetShadingRateImage(nullptr);
    list->SetPipelineState(nativeCamera?family.nativeCameraPipeline.Get():family.pipeline.Get());if(!nativeCamera && !bank.Bind(list,bankDraw))return false;
    list->OMSetRenderTargets(0,nullptr,FALSE,&paired);
    UINT runs{};
    if(masks.empty()){restore.views->SetViewInstanceMask(3);Draw(list,draw);++runs;}
    else for(UINT first=0;first<draw.instances;){
        UINT end=first+1;while(end<draw.instances && masks[end]==masks[first])++end;
        auto run=draw;run.firstInstance+=first;run.instances=end-first;
        restore.views->SetViewInstanceMask(masks[first]);Draw(list,run);first=end;++runs;
    }
    if(extra){
        const D3D12_VERTEX_BUFFER_VIEW view{extra->GetGPUVirtualAddress(),UINT(peerOnly.size()),stride};
        auto additional=draw;additional.instances=UINT(peerOnly.size()/stride);additional.firstInstance=0;
        list->IASetVertexBuffers(7,1,&view);restore.views->SetViewInstanceMask(1);Draw(list,additional);
        list->IASetVertexBuffers(7,1,&draw.vertices[7]);
        ++runs;
    }
    if(s.nativeCounts[0]!=2){
        if(!nativeCamera)restore.bindings.Restore(list);restore.views->SetViewInstanceMask(UINT_MAX);
        list->SetPipelineState(reinterpret_cast<ID3D12PipelineState*>(draw.pipeline));list->OMSetRenderTargets(0,nullptr,FALSE,&reference);Draw(list,draw);
        ++s.optimized[6];
    }else s.optimized[2]+=runs;
    if(alphaOptimized)s.alphaPendingSequence=0;
    s.skinPendingSequence=0;
    ++s.result.draws;if(s.nativeCounts[0])++s.nativeCounts[1];return true;
}
bool EarlyDepthPass::MainConstants(ID3D12GraphicsCommandList* list,const native_probe::DrawRecord& draw,const families::Family& family,
    std::span<const BankPair> pairs,std::span<const uint8_t> masks,std::span<const uint8_t> extra){
    if(!storage_ || storage_->nativeCounts[0]!=2 || storage_->mainSubmitted || storage_->nativeFinished[0] || !family.pairedRoot)return false;
    auto& s=*storage_.Get();const auto root=family.pairedRoot->root.Get();auto found=s.arenas.find(root);
    if(found==s.arenas.end()){
        if(s.arenas.size()>=8)return false;
        auto arena=std::make_unique<StereoBindingBank>();
        if(FAILED(arena->CreateArena(s.device.Get(),root,family.pairedRoot->plan,early_depth::MaxDraws,UINT64(early_depth::MaxDraws)*4096)))return false;
        found=s.arenas.emplace(root,std::move(arena)).first;
    }
    UINT index{};if(FAILED(found->second->AppendConstants(family.pairedRoot->plan,pairs,index)))return false;
    return Main(list,draw,family,*found->second,masks,extra,index);
}
UINT EarlyDepthPass::ArenaCount()const{return storage_?UINT(storage_->arenas.size()):0;}
bool EarlyDepthPass::Peer(ID3D12GraphicsCommandList* list,const native_probe::DrawRecord& draw){
    if(!storage_ || !list || storage_->nativeCounts[0]>2 || draw.side!=1 || draw.rtCount || !draw.depth || storage_->failed || !storage_->mainSubmitted || storage_->copied ||
        (storage_->peerList && storage_->peerList!=list))return false;
    auto& s=*storage_.Get();probe::Scope internal;Restore restore(list,draw);if(!restore.valid || !s.Track(list,1))return false;
    s.peerList=list;const auto target=s.Dsv(1);list->OMSetRenderTargets(0,nullptr,FALSE,&target);Draw(list,draw);return true;
}
bool EarlyDepthPass::Readback(ID3D12GraphicsCommandList* list){
    if(!storage_ || !list || storage_->failed || !storage_->initialized || !storage_->mainSubmitted ||
        (storage_->peerList && storage_->peerList!=list) || storage_->copied)return false;
    auto& s=*storage_.Get();probe::Scope internal;if(!s.Track(list,1))return false;
    s.peerList=list;
    if(s.nativeCounts[0]>=2){
        if(!s.nativeFinished[0] || !s.nativeFinished[1] || (s.nativeCounts[0]==2 && !s.gate.Readback(list)))return false;
        s.EndTiming(list,1);s.copied=true;return true;
    }
    for(const auto& target:s.targets){D3D12_RESOURCE_BARRIER b{};b.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        b.Transition={target.Resource(),D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,D3D12_RESOURCE_STATE_DEPTH_WRITE,D3D12_RESOURCE_STATE_COPY_SOURCE};list->ResourceBarrier(1,&b);}
    for(UINT plane=0;plane<2;++plane)for(UINT eye=0;eye<2;++eye)for(UINT mode=0;mode<2;++mode){
        D3D12_TEXTURE_COPY_LOCATION from{},to{};from.pResource=s.targets[mode?0:1].Resource();from.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;from.SubresourceIndex=eye+plane*2;
        to.pResource=s.readback.Get();to.Type=D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;to.PlacedFootprint=s.planes[plane*4+eye*2+mode].layout;
        list->CopyTextureRegion(&to,0,0,0,&from,nullptr);
    }
    s.EndTiming(list,1);s.copied=true;return true;
}
void EarlyDepthPass::BeforeSubmit(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists){
    if(!storage_ || !queue || !lists)return;auto& s=*storage_.Get();
    for(UINT side=0;side<2;++side){
        const bool submitted=side?s.peerSubmitted:s.mainSubmitted;
        auto* expected=side?s.peerList:s.mainList;
        UINT occurrences{};for(UINT i=0;i<count;++i)occurrences+=lists[i]==expected;
        if(!occurrences || s.recordingRetired[side] || !s.recordingTokens[side] ||
            !gpu::HasCommandResource(static_cast<ID3D12GraphicsCommandList*>(expected),s.recordingTokens[side].Get()))continue;
        // Never rewrite an UPLOAD permit which an earlier submission may still
        // read. Replay violates the caller's one-shot native recording contract.
        if(submitted || s.submissionPending[side]){s.failed=true;continue;}
        s.submissionPending[side]=true;if(occurrences!=1)s.failed=true;
        if(s.permitMappings[side]){
            if(s.alphaPendingSequence || s.skinPendingSequence){s.alphaStats[5]=9;RejectShaderSnapshots();}
            for(const auto& source:s.shaderSources){
                if(source.texture.resource && !texture_read::Validate(source.texture,queue)){
                    if(source.texture.resource->GetDesc().Format!=DXGI_FORMAT_R8_UNORM)s.alphaStats[5]=4000+side;
                    RejectShaderSnapshots();break;
                }
                if(source.material.source.resource && !material_read::ValidateSnapshotSubmission(source.material,source.materialProof[side],queue)){
                    s.alphaStats[5]=3000+side;RejectShaderSnapshots();break;
                }
            }
            *static_cast<volatile uint64_t*>(s.permitMappings[side])=s.shaderReady && !s.failed?1:0;_mm_sfence();
        }
    }
    if(s.mainSubmitted && !s.peerSubmitted && s.submissionPending[1]){
        if(s.nativeCounts[0]==2)s.gate.Freeze();
        if(s.nativeCounts[0]!=3 && queue!=s.mainQueue.Get() && FAILED(queue->Wait(s.mainFence.Get(),s.serial)))s.failed=true;
    }
}
void EarlyDepthPass::Submitted(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists){
    if(!storage_ || !queue || !lists)return;auto& s=*storage_.Get();
    for(UINT i=0;i<count;++i){
        if(!s.mainSubmitted && s.submissionPending[0] && lists[i]==s.mainList){s.mainQueue=queue;
            if(s.timestamps)s.failed|=FAILED(queue->GetTimestampFrequency(&s.gpuTimes[2])) || !s.gpuTimes[2];
            s.failed|=FAILED(queue->Signal(s.mainFence.Get(),s.serial));s.mainSubmitted=true;s.submissionPending[0]=false;}
        else if(!s.peerSubmitted && s.submissionPending[1] && lists[i]==s.peerList){
            if(s.timestamps)s.failed|=FAILED(queue->GetTimestampFrequency(&s.gpuTimes[5])) || !s.gpuTimes[5];
            s.failed|=FAILED(queue->Signal(s.doneFence.Get(),s.serial));s.peerSubmitted=true;s.submissionPending[1]=false;}
    }
}
bool EarlyDepthPass::MainSubmitted()const{return storage_ && storage_->mainSubmitted && !storage_->failed;}
bool EarlyDepthPass::Failed()const{return storage_ && storage_->failed;}
void EarlyDepthPass::Reset(ID3D12CommandList* list){
    if(!storage_ || !list)return;auto& s=*storage_.Get();
    for(UINT side=0;side<2;++side)if(list==(side?s.peerList:s.mainList) && !s.recordingRetired[side] && s.recordingTokens[side]){
        if(!(side?s.peerSubmitted:s.mainSubmitted))s.failed=true;
        s.recordingRetired[side]=true;
    }
}
HRESULT EarlyDepthPass::Read(DepthComparisonResult& result){
    if(!storage_)return E_UNEXPECTED;auto& s=*storage_.Get();if(s.failed)return E_FAIL;
    const auto done=s.doneFence->GetCompletedValue(),mainDone=s.mainFence->GetCompletedValue();
    if(done==UINT64_MAX || mainDone==UINT64_MAX)return DXGI_ERROR_DEVICE_REMOVED;
    if(!s.peerSubmitted || done<s.serial || mainDone<s.serial)return S_FALSE;
    if(s.timestamps && !s.result.complete){
        for(UINT side=0;side<2;++side){
            if(!s.timingEnded[side] || !s.gpuTimes[side*3+2])return E_FAIL;
            void* data{};const D3D12_RANGE range{0,16};const auto hr=s.timingReadback[side]->Map(0,&range,&data);if(FAILED(hr))return hr;
            if(data)std::memcpy(&s.gpuTimes[side*3],data,16);
            const D3D12_RANGE none{};s.timingReadback[side]->Unmap(0,&none);
            if(!data || s.gpuTimes[side*3+1]<s.gpuTimes[side*3])return E_FAIL;
        }
    }
    if(s.nativeCounts[0]==3){s.result.complete=1;result=s.result;return S_OK;}
    if(s.nativeCounts[0]==2){const auto hr=s.gate.Read(s.optimized[1]);if(FAILED(hr))return hr;s.result.complete=1;result=s.result;return S_OK;}
    if(s.result.complete){result=s.result;return S_OK;}
    void* data{};const D3D12_RANGE range{0,SIZE_T(s.bytes)};auto hr=s.readback->Map(0,&range,&data);if(FAILED(hr))return hr;
    if(!data){const D3D12_RANGE none{};s.readback->Unmap(0,&none);return E_FAIL;}
    const auto* bytes=static_cast<const uint8_t*>(data);
    for(UINT eye=0;eye<2;++eye)for(UINT plane=0;plane<2;++plane){
        const auto& a=s.planes[plane*4+eye*2];const auto& b=s.planes[plane*4+eye*2+1];
        for(UINT y=0;y<s.result.height;++y)for(UINT x=0;x<s.result.width;++x){
            const auto* left=bytes+a.layout.Offset+UINT64(y)*a.layout.Footprint.RowPitch+UINT64(x)*a.stride+a.component;
            const auto* right=bytes+b.layout.Offset+UINT64(y)*b.layout.Footprint.RowPitch+UINT64(x)*b.stride+b.component;
            if(s.nativeCounts[0]){
                const auto& n=s.planes[8+plane*2+eye];const auto* actual=bytes+n.layout.Offset+UINT64(y)*n.layout.Footprint.RowPitch+UINT64(x)*n.stride+n.component;
                s.nativeDifferences[plane*2+eye]+=std::memcmp(left,actual,plane?1:4)!=0;
            }
            if(plane){s.result.stencilDifferent[eye]+=*left!=*right;continue;}
            uint32_t av{},bv{};std::memcpy(&av,left,4);std::memcpy(&bv,right,4);s.result.written[eye]+=av!=0;s.result.depthDifferent[eye]+=av!=bv;
            const auto af=std::bit_cast<float>(av),bf=std::bit_cast<float>(bv);
            if(!std::isfinite(af) || !std::isfinite(bf))++s.result.invalidDepth[eye];
            else s.result.maxDepthError[eye]=std::max(s.result.maxDepthError[eye],std::abs(af-bf));
        }
    }
    const D3D12_RANGE none{};s.readback->Unmap(0,&none);s.result.complete=1;result=s.result;return S_OK;
}
HRESULT EarlyDepthPass::Restart(){
    if(!storage_ || storage_->nativeCounts[0]!=2 || storage_->failed || storage_->instanceAugmented || storage_->serial>=UINT64_MAX-1)return E_UNEXPECTED;
    auto& s=*storage_.Get();DepthComparisonResult done;
    const auto hr=Read(done);if(hr!=S_OK)return hr;
    if(!done.complete || !s.gate.Restart())return E_FAIL;
    for(auto& [root,arena]:s.arenas)if(!arena->RestartArena())return E_FAIL;
    s.instanceRanges.clear();s.instanceBytes=0;s.instanceSource.Reset();
    s.shaderSources.clear();s.shaderBytes=0;s.shaderReady=true;s.shaderStats={};
    s.alphaCaptures=s.alphaPendingSequence=s.skinPendingSequence=0;s.alphaStats={};
    for(auto* mapped:s.permitMappings)if(mapped)*static_cast<volatile uint64_t*>(mapped)=0;
    _mm_sfence();
    ++s.serial;
    s.native={};s.nativeDsv={};s.nativeCounts={2,0,0,0,0,0,0,0};s.nativeDifferences={};
    s.optimized={1,UINT64_MAX,0,0,0,0,0,0};s.gpuTimes={};s.nativeFinished={};s.timingEnded={};
    s.mainQueue.Reset();s.mainList=s.peerList=nullptr;
    s.recordingTokens={};s.submissionPending={};s.recordingRetired={};
    s.initialized=s.copied=s.mainSubmitted=s.peerSubmitted=false;
    const auto width=s.result.width,height=s.result.height;s.result={};s.result.width=width;s.result.height=height;
    return S_OK;
}
}
#endif
