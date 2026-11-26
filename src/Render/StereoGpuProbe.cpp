#include "Render/StereoGpuProbe.hpp"
#include "Render/StereoInstanceData.hpp"
#include "Render/NativeStereoCameraUpload.hpp"
#include "Render/RenderProbeScope.hpp"
#include "Render/CommandResources.hpp"
#include "Render/StereoTargetArray.hpp"
#include "Render/StereoGroupResolve.hpp"
#include "Render/StereoSceneState.hpp"
#include "Render/StereoVrsSnapshot.hpp"
#include <windows.h>
#include <wrl/client.h>
#include <array>
#include <vector>
#include <memory>
#include <mutex>
#include <cstring>
#include <thread>
#include <system_error>
#include <algorithm>
#include <span>
#include <emmintrin.h>
#if CVR_RUST_VRIK
// The draw checks: rust/crates/cvr-render/src/gpu_probe_checks.rs (the draw record mirrored by geometry_commands.rs).
// The readback comparison: rust/crates/cvr-render/src/gpu_probe_compare.rs (the copy layouts mirrored there).
// The probe's start and settings: rust/crates/cvr-render/src/gpu_probe_settings.rs (the draw as DrawRecord; the settings
// mirrored).
// The probe's start resets: rust/crates/cvr-render/src/native_state_exports.rs; its buffers: readback_buffers.rs.
extern "C" void cvr_sgp_reset_start();
// The probe's commands and views: rust/crates/cvr-render/src/gpu_probe_commands.rs (the draw as DrawRecord).
// The preparation gate and the readback: rust/crates/cvr-render/src/gpu_probe_gate.rs (the exports as pointers).
// The scene pieces -- Initialize's heaps and query heaps, the fine-rate scope, the direct-resolve views:
// rust/crates/cvr-render/src/gpu_probe_scene.rs (scene_state's Rate mirrored).
#endif
using Microsoft::WRL::ComPtr;
extern "C" {
#if CVR_RUST_VRIK
// The exported state is Rust's (rust/crates/cvr-render/src/native_state_exports.rs, the differences
// gpu_probe_runtime.rs'), declared in the header.
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the Rust test variants' reference definition
__declspec(dllexport) cvr::stereo::gpu_probe::Difference CyberpunkVR_StereoGpuProbeDifferences[128]{};
#endif
#else
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoGpuProbeRequest{0},CyberpunkVR_StereoGpuProbeState{0}; // 0 idle,1 recorded,2 submitted,3 checked,4 error,5 empty,6 incomplete
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoGpuProbeNativeReference{0},CyberpunkVR_StereoGpuProbeReferenceSkipped{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoGpuProbeQueueWaits{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoGpuProbeGroup{0},CyberpunkVR_StereoGpuProbeSourceDraws{0},CyberpunkVR_StereoGpuProbeMainDraws{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoGpuProbeMainViewMask{UINT_MAX},CyberpunkVR_StereoGpuProbeMainReferenceMask{UINT_MAX};
__declspec(dllexport) uint64_t CyberpunkVR_StereoGpuProbeEyeDifferences[2]{},CyberpunkVR_StereoGpuProbeTargetDifferences[4]{};
__declspec(dllexport) uint32_t CyberpunkVR_StereoGpuProbeDifferenceCount{};
__declspec(dllexport) cvr::stereo::gpu_probe::Difference CyberpunkVR_StereoGpuProbeDifferences[128]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoGpuProbePrepareGateMs{0},CyberpunkVR_StereoGpuProbePrepareGateReason{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoGpuProbeLateVisibility{0},CyberpunkVR_StereoGpuProbeMatchedDraws{0},CyberpunkVR_StereoGpuProbeFallbackDraws{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_StereoGpuProbePrepareGateStart{0},CyberpunkVR_StereoGpuProbePrepareGateMain{0},CyberpunkVR_StereoGpuProbePrepareGateRelease{0};
__declspec(dllexport) int32_t CyberpunkVR_StereoGpuProbeResult{};
__declspec(dllexport) uint64_t CyberpunkVR_StereoGpuProbeDifferentBytes{},CyberpunkVR_StereoGpuProbeComparedBytes{},CyberpunkVR_StereoGpuProbeCoverage[2]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoGpuProbeSceneDepth{0},CyberpunkVR_StereoGpuProbeSceneReject{0};
__declspec(dllexport) uint32_t CyberpunkVR_StereoGpuProbeSceneSize[2]{},CyberpunkVR_StereoGpuProbeScenePhase[2]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoGpuProbeSceneRoute{0},CyberpunkVR_StereoGpuProbeSceneCommitted{0};
__declspec(dllexport) uint32_t CyberpunkVR_StereoGpuProbeSceneRouted[2]{};
__declspec(dllexport) uint64_t CyberpunkVR_StereoGpuProbeSceneSamples[2]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoGpuProbeTiming{0},CyberpunkVR_StereoGpuProbeTimingDrops{0};
__declspec(dllexport) uint64_t CyberpunkVR_StereoGpuProbeTicks[7][2]{},CyberpunkVR_StereoGpuProbeFrequency[2]{};
__declspec(dllexport) uint32_t CyberpunkVR_StereoGpuProbeTimedCount[7][2]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoGpuProbeMixedMaterials{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoGpuProbeFineRate{0};
__declspec(dllexport) uint32_t CyberpunkVR_StereoGpuProbeVrs[2][4]{},CyberpunkVR_StereoGpuProbeVrsChanges[2]{};
__declspec(dllexport) uint64_t CyberpunkVR_StereoGpuProbeVrsImage[2]{};
__declspec(dllexport) uint32_t CyberpunkVR_StereoGpuProbeVrsSize[2]{},CyberpunkVR_StereoGpuProbeVrsCaptured[2]{};
__declspec(dllexport) uint64_t CyberpunkVR_StereoGpuProbeVrsDifference{},CyberpunkVR_StereoGpuProbeVrsHistogram[2][16]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoGpuProbeDepthPrepass{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoGpuProbeStartIndices{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoGpuProbeStartRun{0};
#endif
}
#if CVR_RUST_VRIK
// The probe: rust/crates/cvr-render/src/gpu_probe_runtime.rs. The exports (the plugin's are Rust's, the scene probe
// test's its reference definitions) and the camera upload's current binding are lent to it (the list state is its own),
// the exports in its Exports order. The plugin's runtime is gpu_probe_runtime.rs' own; the Rust test variants lend theirs.
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES
struct CvrGpuProbeRuntime {
    const void* exports[52];
    void(*resetStart)();
    uint64_t(*currentBinding)();
};
static_assert(sizeof(CvrGpuProbeRuntime)==54*8 && sizeof(cvr::stereo::gpu_probe::GroupContext)==32,"mirrored by gpu_probe_runtime.rs");
extern "C" {
void cvr_sgpw_attach(const CvrGpuProbeRuntime*);
void cvr_sgpw_begin_group(int32_t,uint32_t,cvr::stereo::gpu_probe::GroupContext*);
void cvr_sgpw_end_group(const cvr::stereo::gpu_probe::GroupContext*);
void cvr_sgpw_draw(ID3D12GraphicsCommandList*,ID3D12PipelineState*,const cvr::stereo::native_probe::DrawRecord*,const uint8_t*,size_t);
void cvr_sgpw_boundary(ID3D12GraphicsCommandList*,const cvr::stereo::native_probe::DrawRecord*,bool,ID3D12PipelineState*);
bool cvr_sgpw_route(ID3D12GraphicsCommandList*,const cvr::stereo::native_probe::DrawRecord*);
void cvr_sgpw_submit(bool,ID3D12CommandQueue*,UINT,ID3D12CommandList* const*);
void cvr_sgpw_poll();
}
namespace cvr::stereo::gpu_probe {
namespace {
const CvrGpuProbeRuntime kRuntime{{
    &CyberpunkVR_StereoGpuProbeRequest,&CyberpunkVR_StereoGpuProbeState,&CyberpunkVR_StereoGpuProbeNativeReference,
    &CyberpunkVR_StereoGpuProbeReferenceSkipped,&CyberpunkVR_StereoGpuProbeQueueWaits,&CyberpunkVR_StereoGpuProbeGroup,
    &CyberpunkVR_StereoGpuProbeSourceDraws,&CyberpunkVR_StereoGpuProbeMainDraws,&CyberpunkVR_StereoGpuProbeMainViewMask,
    &CyberpunkVR_StereoGpuProbeMainReferenceMask,CyberpunkVR_StereoGpuProbeEyeDifferences,CyberpunkVR_StereoGpuProbeTargetDifferences,
    &CyberpunkVR_StereoGpuProbeDifferenceCount,CyberpunkVR_StereoGpuProbeDifferences,&CyberpunkVR_StereoGpuProbePrepareGateMs,
    &CyberpunkVR_StereoGpuProbePrepareGateReason,&CyberpunkVR_StereoGpuProbeLateVisibility,&CyberpunkVR_StereoGpuProbeMatchedDraws,
    &CyberpunkVR_StereoGpuProbeFallbackDraws,&CyberpunkVR_StereoGpuProbePrepareGateStart,&CyberpunkVR_StereoGpuProbePrepareGateMain,
    &CyberpunkVR_StereoGpuProbePrepareGateRelease,&CyberpunkVR_StereoGpuProbeResult,&CyberpunkVR_StereoGpuProbeDifferentBytes,
    &CyberpunkVR_StereoGpuProbeComparedBytes,CyberpunkVR_StereoGpuProbeCoverage,&CyberpunkVR_StereoGpuProbeSceneDepth,
    &CyberpunkVR_StereoGpuProbeSceneReject,CyberpunkVR_StereoGpuProbeSceneSize,CyberpunkVR_StereoGpuProbeScenePhase,
    &CyberpunkVR_StereoGpuProbeSceneRoute,&CyberpunkVR_StereoGpuProbeSceneCommitted,CyberpunkVR_StereoGpuProbeSceneRouted,
    CyberpunkVR_StereoGpuProbeSceneSamples,&CyberpunkVR_StereoGpuProbeTiming,&CyberpunkVR_StereoGpuProbeTimingDrops,
    CyberpunkVR_StereoGpuProbeTicks,CyberpunkVR_StereoGpuProbeFrequency,CyberpunkVR_StereoGpuProbeTimedCount,
    &CyberpunkVR_StereoGpuProbeMixedMaterials,&CyberpunkVR_StereoGpuProbeFineRate,CyberpunkVR_StereoGpuProbeVrs,
    CyberpunkVR_StereoGpuProbeVrsChanges,CyberpunkVR_StereoGpuProbeVrsImage,CyberpunkVR_StereoGpuProbeVrsSize,
    CyberpunkVR_StereoGpuProbeVrsCaptured,&CyberpunkVR_StereoGpuProbeVrsDifference,CyberpunkVR_StereoGpuProbeVrsHistogram,
    &CyberpunkVR_StereoGpuProbeDepthPrepass,&CyberpunkVR_StereoGpuProbeStartIndices,&CyberpunkVR_StereoGpuProbeStartRun,
    &CyberpunkVR_NativeStereoProbeState,
},&cvr_sgp_reset_start,[]{return camera_upload::CurrentBinding().gpuAddress;}};
const bool kAttached=(cvr_sgpw_attach(&kRuntime),true);
}
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
GroupContext BeginGroup(int side,uint32_t plane){GroupContext previous;cvr_sgpw_begin_group(side,plane,&previous);return previous;}
void EndGroup(GroupContext previous){cvr_sgpw_end_group(&previous);}
void Draw(ID3D12GraphicsCommandList* list,ID3D12PipelineState* variant,const native_probe::DrawRecord& draw,std::span<const uint8_t> instances){
    cvr_sgpw_draw(list,variant,&draw,instances.data(),instances.size());
}
void Boundary(ID3D12GraphicsCommandList* list,const native_probe::DrawRecord& draw,bool requirePipelineChange,ID3D12PipelineState* candidateVariant){
    cvr_sgpw_boundary(list,&draw,requirePipelineChange,candidateVariant);
}
bool RouteNative(ID3D12GraphicsCommandList* list,const native_probe::DrawRecord& draw){return cvr_sgpw_route(list,&draw);}
void BeforeSubmit(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists){cvr_sgpw_submit(false,queue,count,lists);}
void Submitted(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists){cvr_sgpw_submit(true,queue,count,lists);}
void Poll(){cvr_sgpw_poll();}
#endif
}
#endif
#else
namespace cvr::stereo::gpu_probe {
namespace {
constexpr UINT Size=256;
constexpr UINT MaxGroupDraws=128,CommitOffset=MaxGroupDraws*8,PredicateBytes=CommitOffset+256;
uint64_t Now(){LARGE_INTEGER q{};QueryPerformanceCounter(&q);return q.QuadPart;}
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
struct PreparationGate {   // the gate itself: rust/crates/cvr-render/src/gpu_probe_gate.rs
    void* rust{};
    ~PreparationGate(){cvr_sgpg_free(rust);}
    uint32_t Reason()const{return cvr_sgpg_reason(rust);}
    bool Publish(volatile uint64_t* address){return cvr_sgpg_publish(rust,address);}
    bool Release(uint32_t why,volatile uint64_t* commit=nullptr){return cvr_sgpg_release(rust,why,commit);}
};
#endif
#else
struct PreparationGate {
    ComPtr<ID3D12Fence> fence;
    std::atomic<bool> armed{false};
    std::atomic<uint32_t> reason{0};
    std::mutex publication;
    bool Publish(volatile uint64_t* address) {
        std::lock_guard lock(publication);if(reason.load()!=0)return false;*address=1;return true;
    }
    bool Release(uint32_t why,volatile uint64_t* commit=nullptr) {
        std::lock_guard lock(publication);
        uint32_t expected{};
        if(reason.compare_exchange_strong(expected,why)) {
            if(commit){*commit=1;CyberpunkVR_StereoGpuProbeSceneCommitted=1;}
            _mm_sfence(); // publish write-combined upload bytes before waking GPU
            CyberpunkVR_StereoGpuProbePrepareGateRelease=Now();CyberpunkVR_StereoGpuProbePrepareGateReason=why;
            fence->Signal(1);return true;
        }
        return false;
    }
};
#endif
struct Surface {
    StereoTargetArray target;
    UINT planes=1;
    std::array<D3D12_PLACED_SUBRESOURCE_FOOTPRINT,4> footprints{};
    std::array<UINT64,4> rowBytes{};
    std::array<UINT,4> rows{};
};
struct Work {
    std::array<std::array<Surface,4>,2> surfaces;
    bool sceneDepth{},depthOnly{},anchorMain{};UINT width=Size,height=Size,stencil{};
    bool sceneRoute{},directResolve{},mixedMaterials{};uint64_t routeSequence[2]{};
    UINT rateOverride{};bool rateSeen[2]{};
    StereoVrsSnapshot vrsSnapshot;ComPtr<ID3D12Resource> sourceVrs;
    ComPtr<ID3D12QueryHeap> sceneQueries;UINT64 queryOffset{};
    ComPtr<ID3D12QueryHeap> timestamps;UINT64 timestampOffset{},frequency[2]{};
    struct Timed {UINT stage,eye,index;};std::array<Timed,512> timed{};UINT timedCount{};
    std::array<Surface,4> resolved;
    StereoGroupResolve resolver;
    bool imported[2]{},resolvedEye[2]{};
    scene_state::Targets nativeTargets[2];
    ComPtr<ID3D12DescriptorHeap> rtv,dsv;
    ComPtr<ID3D12Resource> readback;
    ComPtr<ID3D12Fence> fence;
    ComPtr<ID3D12Fence> producerFence;
    ComPtr<ID3D12CommandQueue> producerQueue;
    ComPtr<ID3D12PipelineState> pipeline;
    ID3D12CommandList* list{};
    ID3D12CommandList* mainList{};
    native_probe::DrawRecord sourceDraw{};
    bool nativeReference{},producerSubmitted{},mainRecorded{};
    bool group{},sourceEnded{},mainEnded{},mainSubmitted{},rejected{};
    uint64_t sourceGroup{},mainGroup{};uint32_t plane=UINT_MAX;
    uint32_t mainViewMask=UINT_MAX,mainReferenceMask=UINT_MAX;
    std::shared_ptr<PreparationGate> preparationGate;
    bool lateVisibility{};
    ComPtr<ID3D12Resource> predicates;
    void* predicateData{};
    std::array<native_probe::DrawRecord,MaxGroupDraws> sourceItems{};
    std::array<std::vector<uint8_t>,MaxGroupDraws> sourceInstances;
    std::array<bool,MaxGroupDraws> matched{};
    UINT rtvStride{},dsvStride{};UINT64 bytes{};
    D3D12_CPU_DESCRIPTOR_HANDLE Rtv(UINT mode,UINT surface,UINT eye=2) const {auto h=rtv->GetCPUDescriptorHandleForHeapStart();h.ptr+=(mode?(eye==1?12:6)+surface:eye<2?eye*3+surface:9+surface)*rtvStride;return h;}
    D3D12_CPU_DESCRIPTOR_HANDLE Dsv(UINT mode,UINT eye=2) const {auto h=dsv->GetCPUDescriptorHandleForHeapStart();h.ptr+=(mode?(eye==1?4:2):eye<2?eye:3)*dsvStride;return h;}
    D3D12_CPU_DESCRIPTOR_HANDLE ResolvedRtv(UINT eye,UINT target)const{auto h=rtv->GetCPUDescriptorHandleForHeapStart();h.ptr+=(15+eye*3+target)*rtvStride;return h;}
    D3D12_CPU_DESCRIPTOR_HANDLE ResolvedDsv(UINT eye)const{auto h=dsv->GetCPUDescriptorHandleForHeapStart();h.ptr+=(5+eye)*dsvStride;return h;}
    ~Work(){if(predicates && predicateData){D3D12_RANGE written{0,PredicateBytes};predicates->Unmap(0,&written);}}
};
std::mutex mutex;std::unique_ptr<Work> work;
std::atomic<uint64_t> nextGroup{};
thread_local GroupContext currentGroup;
enum Stage:UINT {NativeReference,Common,Import,Resolve,CopyBack,Readback,NativeFallback};
struct TimingScope {
    ID3D12GraphicsCommandList* list{};Work* owner{};UINT index{};
    TimingScope(ID3D12GraphicsCommandList* command,Work& w,Stage stage,UINT eye){
        if(!w.timestamps)return;if(w.timedCount==w.timed.size()){++CyberpunkVR_StereoGpuProbeTimingDrops;return;}
        list=command;owner=&w;index=w.timedCount*2;w.timed[w.timedCount++]={stage,eye,index};
#if CVR_RUST_VRIK
        cvr_sgpr_timestamp(list,w.timestamps.Get(),index,nullptr,0);
    }
    ~TimingScope(){if(owner)cvr_sgpr_timestamp(list,owner->timestamps.Get(),index+1,owner->readback.Get(),owner->timestampOffset+index*8);}
#else
        list->EndQuery(w.timestamps.Get(),D3D12_QUERY_TYPE_TIMESTAMP,index);
    }
    ~TimingScope(){if(owner){list->EndQuery(owner->timestamps.Get(),D3D12_QUERY_TYPE_TIMESTAMP,index+1);
        list->ResolveQueryData(owner->timestamps.Get(),D3D12_QUERY_TYPE_TIMESTAMP,index,2,owner->readback.Get(),owner->timestampOffset+index*8);}}
#endif
};
void Fail(HRESULT result){CyberpunkVR_StereoGpuProbeResult=result;CyberpunkVR_StereoGpuProbeState.store(4,std::memory_order_release);}
HRESULT Initialize(ID3D12Device* device,Work& w) {
#if CVR_RUST_VRIK
    auto hr=cvr_sgpr_heaps(device,w.sceneDepth,reinterpret_cast<void**>(w.rtv.ReleaseAndGetAddressOf()),
        reinterpret_cast<void**>(w.dsv.ReleaseAndGetAddressOf()),&w.rtvStride,&w.dsvStride);if(FAILED(hr))return hr;
#else
    D3D12_DESCRIPTOR_HEAP_DESC heap{};heap.NumDescriptors=w.sceneDepth?21:15;heap.Type=D3D12_DESCRIPTOR_HEAP_TYPE_RTV;
    auto hr=device->CreateDescriptorHeap(&heap,IID_PPV_ARGS(&w.rtv));if(FAILED(hr))return hr;
    heap.NumDescriptors=w.sceneDepth?7:5;heap.Type=D3D12_DESCRIPTOR_HEAP_TYPE_DSV;hr=device->CreateDescriptorHeap(&heap,IID_PPV_ARGS(&w.dsv));if(FAILED(hr))return hr;
    w.rtvStride=device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV);w.dsvStride=device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_DSV);
#endif
    const DXGI_FORMAT formats[]{DXGI_FORMAT_R10G10B10A2_UNORM,DXGI_FORMAT_R10G10B10A2_UNORM,DXGI_FORMAT_R8G8B8A8_UNORM,DXGI_FORMAT_R32G8X24_TYPELESS};
    for(UINT mode=0;mode<2;++mode)for(UINT target=w.depthOnly?3:0;target<4;++target) {
        auto& surface=w.surfaces[mode][target];const bool depth=target==3;
        D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D;desc.Width=w.width;desc.Height=w.height;
        desc.DepthOrArraySize=1;desc.MipLevels=1;desc.SampleDesc.Count=1;desc.Format=formats[target];
        desc.Flags=depth?D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL:D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
        D3D12_CLEAR_VALUE clear{};clear.Format=depth?DXGI_FORMAT_D32_FLOAT_S8X24_UINT:desc.Format;
        hr=surface.target.Initialize(device,desc,depth?D3D12_RESOURCE_STATE_DEPTH_WRITE:D3D12_RESOURCE_STATE_RENDER_TARGET,&clear);if(FAILED(hr))return hr;
#if CVR_RUST_VRIK
        surface.planes=surface.target.PlaneCount();
        w.bytes=cvr_sgpr_footprints(device,surface.target.Resource(),surface.planes,w.bytes,surface.footprints.data(),surface.rows.data(),surface.rowBytes.data());
        const D3D12_CPU_DESCRIPTOR_HANDLE eyes[]{depth?w.Dsv(mode,0):w.Rtv(mode,target,0),depth?w.Dsv(mode,1):w.Rtv(mode,target,1)};
        cvr_sgpr_views(device,surface.target.Resource(),depth,desc.Format,(depth?w.Dsv(mode):w.Rtv(mode,target)).ptr,eyes,mode?1:0);
#else
        surface.planes=surface.target.PlaneCount();const auto arrayDesc=surface.target.Resource()->GetDesc();UINT64 bytes{};
        device->GetCopyableFootprints(&arrayDesc,0,surface.planes*2,0,surface.footprints.data(),surface.rows.data(),surface.rowBytes.data(),&bytes);
        w.bytes=(w.bytes+511)&~UINT64(511);for(UINT sub=0;sub<surface.planes*2;++sub)surface.footprints[sub].Offset+=w.bytes;w.bytes+=bytes;
        if(depth){D3D12_DEPTH_STENCIL_VIEW_DESC view{};view.Format=clear.Format;view.ViewDimension=D3D12_DSV_DIMENSION_TEXTURE2DARRAY;view.Texture2DArray.ArraySize=2;device->CreateDepthStencilView(surface.target.Resource(),&view,w.Dsv(mode));}
        else {D3D12_RENDER_TARGET_VIEW_DESC view{};view.Format=desc.Format;view.ViewDimension=D3D12_RTV_DIMENSION_TEXTURE2DARRAY;view.Texture2DArray.ArraySize=2;device->CreateRenderTargetView(surface.target.Resource(),&view,w.Rtv(mode,target));}
        for(UINT eye=mode?1:0;eye<2;++eye) {
            if(depth){D3D12_DEPTH_STENCIL_VIEW_DESC view{};view.Format=clear.Format;view.ViewDimension=D3D12_DSV_DIMENSION_TEXTURE2DARRAY;view.Texture2DArray.FirstArraySlice=eye;view.Texture2DArray.ArraySize=1;device->CreateDepthStencilView(surface.target.Resource(),&view,w.Dsv(mode,eye));}
            else {D3D12_RENDER_TARGET_VIEW_DESC view{};view.Format=desc.Format;view.ViewDimension=D3D12_RTV_DIMENSION_TEXTURE2DARRAY;view.Texture2DArray.FirstArraySlice=eye;view.Texture2DArray.ArraySize=1;device->CreateRenderTargetView(surface.target.Resource(),&view,w.Rtv(mode,target,eye));}
        }
#endif
    }
    if(w.sceneDepth) {
        if(w.sourceVrs){hr=w.vrsSnapshot.Initialize(device,w.sourceVrs.Get());if(FAILED(hr))return hr;
            CyberpunkVR_StereoGpuProbeVrsSize[0]=w.vrsSnapshot.Width();CyberpunkVR_StereoGpuProbeVrsSize[1]=w.vrsSnapshot.Height();}
        if(!w.directResolve)for(UINT target=w.depthOnly?3:0;target<4;++target) {
            const bool depth=target==3;auto desc=w.surfaces[0][target].target.Resource()->GetDesc();desc.DepthOrArraySize=1;
            hr=w.resolved[target].target.Initialize(device,desc,depth?D3D12_RESOURCE_STATE_DEPTH_WRITE:D3D12_RESOURCE_STATE_RENDER_TARGET);if(FAILED(hr))return hr;
#if CVR_RUST_VRIK
            const D3D12_CPU_DESCRIPTOR_HANDLE eyes[]{depth?w.ResolvedDsv(0):w.ResolvedRtv(0,target),depth?w.ResolvedDsv(1):w.ResolvedRtv(1,target)};
            cvr_sgpr_views(device,w.resolved[target].target.Resource(),depth,desc.Format,0,eyes,0);
#else
            for(UINT eye=0;eye<2;++eye) {
                if(depth){D3D12_DEPTH_STENCIL_VIEW_DESC view{};view.Format=DXGI_FORMAT_D32_FLOAT_S8X24_UINT;view.ViewDimension=D3D12_DSV_DIMENSION_TEXTURE2DARRAY;
                    view.Texture2DArray.FirstArraySlice=eye;view.Texture2DArray.ArraySize=1;device->CreateDepthStencilView(w.resolved[target].target.Resource(),&view,w.ResolvedDsv(eye));}
                else {D3D12_RENDER_TARGET_VIEW_DESC view{};view.Format=desc.Format;view.ViewDimension=D3D12_RTV_DIMENSION_TEXTURE2DARRAY;
                    view.Texture2DArray.FirstArraySlice=eye;view.Texture2DArray.ArraySize=1;device->CreateRenderTargetView(w.resolved[target].target.Resource(),&view,w.ResolvedRtv(eye,target));}
            }
#endif
        }
        hr=w.resolver.Initialize(device,{w.surfaces[1][0].target.Resource(),w.surfaces[1][1].target.Resource(),w.surfaces[1][2].target.Resource(),w.surfaces[1][3].target.Resource()},w.depthOnly);if(FAILED(hr))return hr;
#if CVR_RUST_VRIK
        hr=cvr_sgpr_queries(device,D3D12_QUERY_HEAP_TYPE_OCCLUSION,2,&w.bytes,&w.queryOffset,reinterpret_cast<void**>(w.sceneQueries.ReleaseAndGetAddressOf()));if(FAILED(hr))return hr;
#else
        D3D12_QUERY_HEAP_DESC query{D3D12_QUERY_HEAP_TYPE_OCCLUSION,2,0};hr=device->CreateQueryHeap(&query,IID_PPV_ARGS(&w.sceneQueries));if(FAILED(hr))return hr;
        w.queryOffset=(w.bytes+7)&~UINT64(7);w.bytes=w.queryOffset+16;
#endif
    }
    if(CyberpunkVR_StereoGpuProbeTiming.load(std::memory_order_relaxed)){
#if CVR_RUST_VRIK
        hr=cvr_sgpr_queries(device,D3D12_QUERY_HEAP_TYPE_TIMESTAMP,1024,&w.bytes,&w.timestampOffset,reinterpret_cast<void**>(w.timestamps.ReleaseAndGetAddressOf()));if(FAILED(hr))return hr;
#else
        D3D12_QUERY_HEAP_DESC query{D3D12_QUERY_HEAP_TYPE_TIMESTAMP,1024,0};hr=device->CreateQueryHeap(&query,IID_PPV_ARGS(&w.timestamps));if(FAILED(hr))return hr;
        w.timestampOffset=(w.bytes+7)&~UINT64(7);w.bytes=w.timestampOffset+1024*8;
#endif
    }
#if CVR_RUST_VRIK
    hr=cvr_rb_buffer(device,nullptr,w.bytes,D3D12_HEAP_TYPE_READBACK,D3D12_RESOURCE_STATE_COPY_DEST,reinterpret_cast<void**>(w.readback.ReleaseAndGetAddressOf()));
    if(FAILED(hr))return hr;
    if(w.lateVisibility){void* predicates{};hr=cvr_rb_mapped_upload(device,PredicateBytes,&predicates,&w.predicateData);if(FAILED(hr))return hr;
        w.predicates.Attach(static_cast<ID3D12Resource*>(predicates));}
#else
    D3D12_RESOURCE_DESC buffer{};buffer.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;buffer.Width=w.bytes;
    buffer.Height=1;buffer.DepthOrArraySize=1;buffer.MipLevels=1;buffer.SampleDesc.Count=1;buffer.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    D3D12_HEAP_PROPERTIES memory{};memory.Type=D3D12_HEAP_TYPE_READBACK;
    hr=device->CreateCommittedResource(&memory,D3D12_HEAP_FLAG_NONE,&buffer,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&w.readback));if(FAILED(hr))return hr;
    if(w.lateVisibility) {
        buffer.Width=PredicateBytes;memory.Type=D3D12_HEAP_TYPE_UPLOAD;
        hr=device->CreateCommittedResource(&memory,D3D12_HEAP_FLAG_NONE,&buffer,D3D12_RESOURCE_STATE_GENERIC_READ,nullptr,IID_PPV_ARGS(&w.predicates));if(FAILED(hr))return hr;
        const D3D12_RANGE none{};hr=w.predicates->Map(0,&none,&w.predicateData);if(FAILED(hr))return hr;if(!w.predicateData)return E_FAIL;std::memset(w.predicateData,0,PredicateBytes);
    }
#endif
    hr=device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&w.producerFence));if(FAILED(hr))return hr;
    return device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&w.fence));
}
bool SameGeometry(const native_probe::DrawRecord& a,const native_probe::DrawRecord& b,
                  std::span<const uint8_t> instancesA={},std::span<const uint8_t> instancesB={}) {
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
    static_assert(sizeof(native_probe::DrawRecord)==1096,"mirrored by geometry_commands.rs");
    if(!cvr_sgpp_same_fields(&a,&b))return false;
    if(!instancesA.empty() || !instancesB.empty()) {
        if(!instance_data::Equal(a,instancesA,b,instancesB))return false;
    }else if(!cvr_sgpp_inline_instances(&a,&b))return false;
    return cvr_sgpp_vertex_views(&a,&b);
#endif
#else
    if(a.frame!=b.frame || a.pipeline!=b.pipeline || a.root!=b.root || a.indices!=b.indices || a.instances!=b.instances ||
       a.firstIndex!=b.firstIndex || a.baseVertex!=b.baseVertex || a.vertexMask!=b.vertexMask ||
       a.rtCount!=b.rtCount || a.depth!=b.depth || a.targets!=b.targets ||
       std::memcmp(&a.index,&b.index,sizeof(a.index)))return false;
    if(!instancesA.empty() || !instancesB.empty()) {
        if(!instance_data::Equal(a,instancesA,b,instancesB))return false;
    }else if(a.instances!=1 || !a.instanceDataBytes || a.instanceDataBytes!=b.instanceDataBytes || a.instanceData!=b.instanceData)return false;
    for(unsigned i=0;i<16;++i)if(i!=7 && (a.vertexMask&(1u<<i)) && std::memcmp(&a.vertices[i],&b.vertices[i],sizeof(a.vertices[i])))return false;
    return true;
#endif
}
bool RetainSurfaces(ID3D12GraphicsCommandList* list,Work& w) {
    return cvr::gpu::KeepCommandResources(list,{w.rtv.Get(),w.dsv.Get(),w.readback.Get(),w.pipeline.Get(),
        w.surfaces[0][3].target.Resource(),w.surfaces[1][3].target.Resource()}) &&
        (w.depthOnly || cvr::gpu::KeepCommandResources(list,{w.surfaces[0][0].target.Resource(),w.surfaces[0][1].target.Resource(),w.surfaces[0][2].target.Resource(),
        w.surfaces[1][0].target.Resource(),w.surfaces[1][1].target.Resource(),w.surfaces[1][2].target.Resource()}));
}
bool SamePass(const native_probe::DrawRecord& a,const native_probe::DrawRecord& b,bool mixed=false) {
#if CVR_RUST_VRIK
    return cvr_sgpp_same_pass(&a,&b,mixed);
#else
    return a.frame==b.frame && (mixed || a.pipeline==b.pipeline) && a.root==b.root && a.rtCount==b.rtCount &&
        a.targets==b.targets && a.depth==b.depth && (!b.rtCount || b.rtContiguous==0) && (b.flags&127)==63 &&
        std::memcmp(&a.viewport,&b.viewport,sizeof(a.viewport))==0 && std::memcmp(&a.scissor,&b.scissor,sizeof(a.scissor))==0;
#endif
}
void CopyMode(ID3D12GraphicsCommandList* list,Work& w,UINT mode,bool force=false) {
    if(w.sceneDepth && !force)return;
    TimingScope timing(list,w,Readback,currentGroup.side==0 || (w.mainRecorded && list==w.mainList)?1:0);
#if CVR_RUST_VRIK
    for(UINT target=w.depthOnly?3:0;target<4;++target){auto& surface=w.surfaces[mode][target];
        auto* resource=w.sceneDepth && mode?w.resolved[target].target.Resource():surface.target.Resource();
        cvr_sgpr_copy(list,resource,target==3?D3D12_RESOURCE_STATE_DEPTH_WRITE:D3D12_RESOURCE_STATE_RENDER_TARGET,w.readback.Get(),surface.footprints.data(),0,1,surface.planes*2,w.group);}
#else
    for(UINT target=w.depthOnly?3:0;target<4;++target) {
        auto& surface=w.surfaces[mode][target];auto* resource=w.sceneDepth && mode?w.resolved[target].target.Resource():surface.target.Resource();D3D12_RESOURCE_BARRIER barrier{};barrier.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        barrier.Transition={resource,D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,target==3?D3D12_RESOURCE_STATE_DEPTH_WRITE:D3D12_RESOURCE_STATE_RENDER_TARGET,D3D12_RESOURCE_STATE_COPY_SOURCE};list->ResourceBarrier(1,&barrier);
        for(UINT sub=0;sub<surface.planes*2;++sub) {
            D3D12_TEXTURE_COPY_LOCATION src{},dst{};src.pResource=resource;src.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;src.SubresourceIndex=sub;
            dst.pResource=w.readback.Get();dst.Type=D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;dst.PlacedFootprint=surface.footprints[sub];list->CopyTextureRegion(&dst,0,0,0,&src,nullptr);
        }
        if(w.group){std::swap(barrier.Transition.StateBefore,barrier.Transition.StateAfter);list->ResourceBarrier(1,&barrier);}
    }
#endif
}
void Restore(ID3D12GraphicsCommandList* list,const native_probe::DrawRecord& draw) {
#if CVR_RUST_VRIK
    cvr_sgpr_restore(list,&draw);
#else
    std::array<D3D12_CPU_DESCRIPTOR_HANDLE,8> targets{};for(unsigned i=0;i<8;++i)targets[i].ptr=draw.targets[i];
    const D3D12_CPU_DESCRIPTOR_HANDLE depth{draw.depth};list->OMSetRenderTargets(draw.rtCount,targets.data(),draw.rtContiguous,draw.depth?&depth:nullptr);
    list->RSSetViewports(1,&draw.viewport);list->RSSetScissorRects(1,&draw.scissor);list->SetPipelineState(reinterpret_cast<ID3D12PipelineState*>(draw.pipeline));
#endif
}
bool SceneReject(Work& w,UINT reason){w.rejected=true;CyberpunkVR_StereoGpuProbeSceneReject=reason;return false;}
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
struct FineRateScope {   // rust/crates/cvr-render/src/gpu_probe_scene.rs
    RustFineRate state{};bool ok=true;
    FineRateScope(ID3D12GraphicsCommandList* command,Work& w,UINT eye){
        const RustFineExports exports{CyberpunkVR_StereoGpuProbeVrs,CyberpunkVR_StereoGpuProbeVrsChanges,CyberpunkVR_StereoGpuProbeVrsImage};
        if(const auto reject=cvr_sgpr_fine_rate(command,w.sceneDepth,eye,&w.rateSeen[eye],w.rateOverride,w.rateOverride==2?w.vrsSnapshot.SourceImage():nullptr,&exports,&state))
            ok=SceneReject(w,reject);
    }
    ~FineRateScope(){cvr_sgpr_fine_rate_end(&state);}
};
#endif
#else
struct FineRateScope {
    ComPtr<ID3D12GraphicsCommandList5> list;
    scene_state::Bindings::Rate saved;
    bool ok=true;
    bool imageChanged{};
    FineRateScope(ID3D12GraphicsCommandList* command,Work& w,UINT eye) {
        if(!w.sceneDepth)return;scene_state::Bindings b;
        if(!scene_state::Snapshot(command,b)){ok=SceneReject(w,12);return;}
        saved=b.rate;const uint32_t values[]{saved.known?1u:0u,UINT(saved.value),UINT(saved.combiners[0]),UINT(saved.combiners[1])};
        const auto image=reinterpret_cast<uintptr_t>(saved.image);
        if(w.rateSeen[eye] && (std::memcmp(values,CyberpunkVR_StereoGpuProbeVrs[eye],sizeof(values)) || CyberpunkVR_StereoGpuProbeVrsImage[eye]!=image))++CyberpunkVR_StereoGpuProbeVrsChanges[eye];
        w.rateSeen[eye]=true;std::memcpy(CyberpunkVR_StereoGpuProbeVrs[eye],values,sizeof(values));CyberpunkVR_StereoGpuProbeVrsImage[eye]=image;
        if(!w.rateOverride || (w.rateOverride==2 && eye==0))return;
        if(!saved.known){ok=SceneReject(w,12);return;}
        const auto hr=command->QueryInterface(IID_PPV_ARGS(&list));
        if(hr==E_NOINTERFACE)return;
        if(FAILED(hr)){ok=SceneReject(w,12);return;}
        const D3D12_SHADING_RATE_COMBINER fine[]{D3D12_SHADING_RATE_COMBINER_PASSTHROUGH,D3D12_SHADING_RATE_COMBINER_PASSTHROUGH};
        if(w.rateOverride==1)list->RSSetShadingRate(D3D12_SHADING_RATE_1X1,fine);
        else if(w.vrsSnapshot.SourceImage()){list->RSSetShadingRateImage(w.vrsSnapshot.SourceImage());imageChanged=true;}
        else ok=SceneReject(w,13);
    }
    ~FineRateScope(){if(list){if(imageChanged)list->RSSetShadingRateImage(saved.image);list->RSSetShadingRate(saved.value,saved.combiners.data());}}
};
#endif
bool SceneBindings(ID3D12GraphicsCommandList* list,Work& w) {
    if(!w.sceneDepth)return true;scene_state::Bindings bindings;
    return (scene_state::Snapshot(list,bindings) && bindings.stencil==w.stencil) || SceneReject(w,9);
}
bool ImportScene(ID3D12GraphicsCommandList* list,Work& w,const native_probe::DrawRecord& draw,UINT eye) {
    scene_state::Bindings bindings;
    if(!scene_state::Snapshot(list,bindings) || bindings.stencil!=w.stencil)return SceneReject(w,2);
    if(!w.nativeTargets[eye].resources[3] && !scene_state::CaptureTargets(draw,w.nativeTargets[eye]))return SceneReject(w,3);
    if(w.sourceVrs){if(!bindings.rate.known || !bindings.rate.image || FAILED(w.vrsSnapshot.Capture(list,bindings.rate.image,eye)))return SceneReject(w,13);CyberpunkVR_StereoGpuProbeVrsCaptured[eye]=1;}
    const auto& r=w.nativeTargets[eye].resources;
    if(!w.resolver.Retain(list) || !cvr::gpu::KeepCommandResources(list,{w.sceneQueries.Get(),w.predicates.Get(),r[3].Get()}))return SceneReject(w,4);
    if(!w.depthOnly && !cvr::gpu::KeepCommandResources(list,{r[0].Get(),r[1].Get(),r[2].Get()}))return SceneReject(w,4);
    if(w.directResolve) {
#if CVR_RUST_VRIK
        const std::array<ID3D12Resource*,4> targets{r[0].Get(),r[1].Get(),r[2].Get(),r[3].Get()};
        const D3D12_CPU_DESCRIPTOR_HANDLE rtvs[]{w.ResolvedRtv(eye,0),w.ResolvedRtv(eye,1),w.ResolvedRtv(eye,2)};
        if(!cvr_sgpr_direct_views(list,targets.data(),rtvs,w.ResolvedDsv(eye).ptr,w.depthOnly))return SceneReject(w,11);
#else
        ComPtr<ID3D12Device> device;if(FAILED(list->GetDevice(IID_PPV_ARGS(&device))))return SceneReject(w,11);
        if(!w.depthOnly)for(UINT t=0;t<3;++t){D3D12_RENDER_TARGET_VIEW_DESC v{};v.Format=r[t]->GetDesc().Format;v.ViewDimension=D3D12_RTV_DIMENSION_TEXTURE2D;
            device->CreateRenderTargetView(r[t].Get(),&v,w.ResolvedRtv(eye,t));}
        D3D12_DEPTH_STENCIL_VIEW_DESC v{};v.Format=DXGI_FORMAT_D32_FLOAT_S8X24_UINT;v.ViewDimension=D3D12_DSV_DIMENSION_TEXTURE2D;device->CreateDepthStencilView(r[3].Get(),&v,w.ResolvedDsv(eye));
#endif
    }else if(!cvr::gpu::KeepCommandResources(list,{w.resolved[3].target.Resource()}) ||
        (!w.depthOnly && !cvr::gpu::KeepCommandResources(list,{w.resolved[0].target.Resource(),w.resolved[1].target.Resource(),w.resolved[2].target.Resource()})))return SceneReject(w,4);
    TimingScope timing(list,w,Import,eye);
    for(UINT target=w.depthOnly?3:0;target<4;++target){const auto state=target==3?D3D12_RESOURCE_STATE_DEPTH_WRITE:D3D12_RESOURCE_STATE_RENDER_TARGET;
        if(FAILED(w.surfaces[0][target].target.ImportEye(list,eye,state,r[target].Get(),state)) ||
           (!w.directResolve && FAILED(w.resolved[target].target.ImportEye(list,eye,state,r[target].Get(),state))))return SceneReject(w,5);}
    w.imported[eye]=true;CyberpunkVR_StereoGpuProbeScenePhase[eye]=1;return true;
}
void CopyNativeEye(ID3D12GraphicsCommandList* list,Work& w,UINT eye) {
    TimingScope timing(list,w,Readback,eye);
#if CVR_RUST_VRIK
    for(UINT t=w.depthOnly?3:0;t<4;++t)cvr_sgpr_copy(list,w.nativeTargets[eye].resources[t].Get(),t==3?D3D12_RESOURCE_STATE_DEPTH_WRITE:D3D12_RESOURCE_STATE_RENDER_TARGET,
        w.readback.Get(),w.surfaces[1][t].footprints.data(),eye,2,w.surfaces[1][t].planes,true);
#else
    for(UINT t=w.depthOnly?3:0;t<4;++t){auto* r=w.nativeTargets[eye].resources[t].Get();const auto state=t==3?D3D12_RESOURCE_STATE_DEPTH_WRITE:D3D12_RESOURCE_STATE_RENDER_TARGET;
        D3D12_RESOURCE_BARRIER b{};b.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;b.Transition={r,D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,state,D3D12_RESOURCE_STATE_COPY_SOURCE};list->ResourceBarrier(1,&b);
        const auto& surface=w.surfaces[1][t];for(UINT plane=0;plane<surface.planes;++plane){D3D12_TEXTURE_COPY_LOCATION from{},to{};
            from.pResource=r;from.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;from.SubresourceIndex=plane;
            to.pResource=w.readback.Get();to.Type=D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;to.PlacedFootprint=surface.footprints[eye+plane*2];list->CopyTextureRegion(&to,0,0,0,&from,nullptr);}
        std::swap(b.Transition.StateBefore,b.Transition.StateAfter);list->ResourceBarrier(1,&b);
    }
#endif
}
void FinishScene(ID3D12GraphicsCommandList* list,Work& w,const native_probe::DrawRecord& draw,UINT eye) {
    if(!w.sceneDepth || w.resolvedEye[eye] || !w.imported[eye] || w.rejected)return;
    scene_state::Bindings bindings;
    if(!scene_state::Snapshot(list,bindings)){SceneReject(w,6);return;}
    probe::Scope scope;
    const std::array<D3D12_CPU_DESCRIPTOR_HANDLE,3> targets{w.ResolvedRtv(eye,0),w.ResolvedRtv(eye,1),w.ResolvedRtv(eye,2)};
    if(w.directResolve)list->SetPredication(w.predicates.Get(),CommitOffset,D3D12_PREDICATION_OP_EQUAL_ZERO);
    {TimingScope timing(list,w,Resolve,eye);
        list->BeginQuery(w.sceneQueries.Get(),D3D12_QUERY_TYPE_OCCLUSION,eye);
        w.resolver.Record(list,eye,w.stencil,w.sourceDraw.viewport,w.sourceDraw.scissor,targets,w.ResolvedDsv(eye));
        list->EndQuery(w.sceneQueries.Get(),D3D12_QUERY_TYPE_OCCLUSION,eye);}
    if(w.directResolve)list->SetPredication(nullptr,0,D3D12_PREDICATION_OP_EQUAL_ZERO);
    list->ResolveQueryData(w.sceneQueries.Get(),D3D12_QUERY_TYPE_OCCLUSION,eye,1,w.readback.Get(),w.queryOffset+eye*8);
    if(w.sceneRoute && !w.directResolve) {
        {TimingScope timing(list,w,CopyBack,eye);
        list->SetPredication(w.predicates.Get(),CommitOffset,D3D12_PREDICATION_OP_EQUAL_ZERO);
        for(UINT t=w.depthOnly?3:0;t<4;++t){const auto state=t==3?D3D12_RESOURCE_STATE_DEPTH_WRITE:D3D12_RESOURCE_STATE_RENDER_TARGET;
            if(FAILED(w.resolved[t].target.CopyEye(list,eye,state,w.nativeTargets[eye].resources[t].Get(),state,state)))SceneReject(w,10);}
        list->SetPredication(nullptr,0,D3D12_PREDICATION_OP_EQUAL_ZERO);
        }
    }
    if(w.sceneRoute)CopyNativeEye(list,w,eye);
    bindings.Restore(list);Restore(list,draw);
    w.resolvedEye[eye]=true;CyberpunkVR_StereoGpuProbeScenePhase[eye]=2;
    // Both eyes of the readback are written together only after MAIN's prefix.
    if(eye==1){CopyMode(list,w,0,true);if(!w.sceneRoute)CopyMode(list,w,1,true);}
}
void Reference(ID3D12GraphicsCommandList* list,Work& w,const native_probe::DrawRecord& draw,UINT eye,UINT mode=0) {
    FineRateScope rate(list,w,eye);if(!rate.ok)return;
    TimingScope timing(list,w,mode?NativeFallback:NativeReference,eye);
#if CVR_RUST_VRIK
    const D3D12_CPU_DESCRIPTOR_HANDLE targets[]{w.Rtv(mode,0,eye),w.Rtv(mode,1,eye),w.Rtv(mode,2,eye)};
    cvr_sgpr_reference(list,&draw,w.sceneDepth,w.width,w.height,targets,w.Dsv(mode,eye).ptr,w.depthOnly);
#else
    const D3D12_VIEWPORT viewport=w.sceneDepth?draw.viewport:D3D12_VIEWPORT{0,0,float(w.width),float(w.height),0,1};const D3D12_RECT scissor=w.sceneDepth?draw.scissor:D3D12_RECT{0,0,LONG(w.width),LONG(w.height)};
    D3D12_CPU_DESCRIPTOR_HANDLE targets[]{w.Rtv(mode,0,eye),w.Rtv(mode,1,eye),w.Rtv(mode,2,eye)};const auto depth=w.Dsv(mode,eye);
    list->SetPipelineState(reinterpret_cast<ID3D12PipelineState*>(draw.pipeline));list->RSSetViewports(1,&viewport);list->RSSetScissorRects(1,&scissor);
    list->OMSetRenderTargets(w.depthOnly?0:3,w.depthOnly?nullptr:targets,FALSE,&depth);list->DrawIndexedInstanced(draw.indices,draw.instances,draw.firstIndex,draw.baseVertex,draw.firstInstance);
#endif
}
void SharedDraw(ID3D12GraphicsCommandList* list,ID3D12GraphicsCommandList1* views,Work& w,const native_probe::DrawRecord& draw,UINT ordinal,std::span<const uint8_t> instances) {
    FineRateScope rate(list,w,0);if(!rate.ok)return;
    TimingScope timing(list,w,Common,0);
#if CVR_RUST_VRIK
    if(w.lateVisibility){w.sourceItems[ordinal]=draw;
        try{w.sourceInstances[ordinal].assign(instances.begin(),instances.end());}catch(const std::bad_alloc&){w.sourceInstances[ordinal].clear();}}
    // Late visibility: exactly one predicated branch executes -- shared for a pair, the mono pipeline on slice zero
    // otherwise; else one draw under the group's view mask.
    cvr_sgpr_shared(list,views,w.lateVisibility?w.predicates.Get():nullptr,uint64_t(ordinal)*8,
        w.lateVisibility || !w.group || (w.mainViewMask&(1u<<ordinal))?3u:1u,&draw);
#else
    if(w.lateVisibility) {
        w.sourceItems[ordinal]=draw;
        try{w.sourceInstances[ordinal].assign(instances.begin(),instances.end());}catch(const std::bad_alloc&){w.sourceInstances[ordinal].clear();}
        // D3D12's predicate operation specifies when to SKIP the draw.
        // Exactly one branch executes: shared for a pair, VRCAM-only otherwise.
        list->SetPredication(w.predicates.Get(),ordinal*8,D3D12_PREDICATION_OP_EQUAL_ZERO);
        views->SetViewInstanceMask(3);list->DrawIndexedInstanced(draw.indices,draw.instances,draw.firstIndex,draw.baseVertex,draw.firstInstance);
        list->SetPredication(w.predicates.Get(),ordinal*8,D3D12_PREDICATION_OP_NOT_EQUAL_ZERO);
        // No partner: use the original mono pipeline for array slice zero.
        // Avoid paying the view-dependent VS cost for an eye-exclusive draw.
        list->SetPipelineState(reinterpret_cast<ID3D12PipelineState*>(draw.pipeline));
        views->SetViewInstanceMask(1);list->DrawIndexedInstanced(draw.indices,draw.instances,draw.firstIndex,draw.baseVertex,draw.firstInstance);
        list->SetPredication(nullptr,0,D3D12_PREDICATION_OP_EQUAL_ZERO);
    }else {views->SetViewInstanceMask(!w.group || (w.mainViewMask&(1u<<ordinal))?3:1);list->DrawIndexedInstanced(draw.indices,draw.instances,draw.firstIndex,draw.baseVertex,draw.firstInstance);}
#endif
}
void FinishGroupIfSubmitted() {
    if(work && work->group && work->sourceEnded && work->mainEnded && work->mainSubmitted && !work->rejected)
        CyberpunkVR_StereoGpuProbeState.store(2,std::memory_order_release);
}
}
void Boundary(ID3D12GraphicsCommandList* list,const native_probe::DrawRecord& draw,bool requirePipelineChange,ID3D12PipelineState* candidateVariant) {
    if(requirePipelineChange && currentGroup.id && draw.side==1 && CyberpunkVR_StereoGpuProbeDepthPrepass.load(std::memory_order_relaxed)) {
        const bool candidate=candidateVariant && draw.rtCount==0;
        if(candidate && (!currentGroup.depthCandidate || currentGroup.lastPipeline!=draw.pipeline))++currentGroup.depthRun;
        currentGroup.depthCandidate=candidate;currentGroup.lastPipeline=draw.pipeline;
    }
    if(CyberpunkVR_StereoGpuProbeState.load(std::memory_order_acquire)!=1)return;
    std::lock_guard lock(mutex);if(!work || !work->sceneDepth || work->rejected)return;
    const UINT eye=draw.side==1?0:1;
    if(currentGroup.id!=(eye?work->mainGroup:work->sourceGroup) || list!=(eye?work->mainList:work->list))return;
    if(requirePipelineChange){
        scene_state::Bindings b;const bool sameStencil=scene_state::Snapshot(list,b) && b.stencil==work->stencil;
        if(sameStencil && SamePass(work->sourceDraw,draw,work->mixedMaterials) &&
           (draw.pipeline==work->sourceDraw.pipeline || (candidateVariant && scene_state::Compatible(reinterpret_cast<ID3D12PipelineState*>(draw.pipeline)))))return;
    }
    FinishScene(list,*work,draw,eye);
}
bool RouteNative(ID3D12GraphicsCommandList* list,const native_probe::DrawRecord& draw) {
    if(CyberpunkVR_StereoGpuProbeState.load(std::memory_order_acquire)!=1 || !CyberpunkVR_StereoGpuProbeSceneRoute.load(std::memory_order_relaxed))return false;
    std::lock_guard lock(mutex);if(!work || !work->sceneRoute || work->rejected || draw.side>1 || !draw.sequence)return false;
    const UINT eye=draw.side==1?0:1;
    if(work->routeSequence[eye]!=draw.sequence || (eye?work->mainGroup:work->sourceGroup)!=currentGroup.id)return false;
    if(!cvr::gpu::KeepCommandResources(list,{work->predicates.Get()})){SceneReject(*work,4);return false;}
    probe::Scope scope;
    // The native draw remains recorded as the fallback. It is suppressed only
    // when MAIN completes validation and publishes the frame-wide commit flag.
#if CVR_RUST_VRIK
    cvr_sgpr_routed(list,work->predicates.Get(),CommitOffset,&draw);
#else
    list->SetPredication(work->predicates.Get(),CommitOffset,D3D12_PREDICATION_OP_NOT_EQUAL_ZERO);
    list->DrawIndexedInstanced(draw.indices,draw.instances,draw.firstIndex,draw.baseVertex,draw.firstInstance);
    list->SetPredication(nullptr,0,D3D12_PREDICATION_OP_EQUAL_ZERO);
#endif
    ++CyberpunkVR_StereoGpuProbeSceneRouted[eye];return true;
}
GroupContext BeginGroup(int side,uint32_t plane) {
    const auto previous=currentGroup;currentGroup={nextGroup.fetch_add(1,std::memory_order_relaxed)+1,side,plane};
    if(side==0 && CyberpunkVR_StereoGpuProbeState.load(std::memory_order_acquire)==1) {
        std::lock_guard lock(mutex);
        if(work && work->group && work->plane==plane && work->sourceEnded && work->preparationGate) {
            uint64_t empty{};const auto now=Now();
            if(CyberpunkVR_StereoGpuProbePrepareGateMain.compare_exchange_strong(empty,now) && !work->lateVisibility)work->preparationGate->Release(1);
        }
    }
    return previous;
}
void EndGroup(GroupContext previous) {
    const auto ended=currentGroup;currentGroup=previous;
    if(CyberpunkVR_StereoGpuProbeState.load(std::memory_order_acquire)!=1)return;
    std::lock_guard lock(mutex);if(!work || !work->group)return;
    if(work->sceneDepth && (ended.id==work->sourceGroup || ended.id==work->mainGroup)) {
        const UINT eye=ended.id==work->sourceGroup?0:1;auto* list=static_cast<ID3D12GraphicsCommandList*>(eye?work->mainList:work->list);
        native_probe::DrawRecord saved{};
        if(!work->resolvedEye[eye] && (!list || !native_probe::CurrentState(list,saved)))SceneReject(*work,7);
        if(list)FinishScene(list,*work,saved,eye);
        if(!work->resolvedEye[eye])SceneReject(*work,7);
    }
    if(ended.id==work->sourceGroup)work->sourceEnded=true;
    if(ended.id==work->mainGroup) {
        work->mainEnded=true;
        if(work->lateVisibility && work->preparationGate) {
            auto* commit=work->sceneRoute && !work->rejected?static_cast<volatile uint64_t*>(work->predicateData)+MaxGroupDraws:nullptr;
            if(!work->preparationGate->Release(work->rejected?4:1,commit))work->rejected=true;
        }
    }
    FinishGroupIfSubmitted();
}
void Draw(ID3D12GraphicsCommandList* list,ID3D12PipelineState* variant,const native_probe::DrawRecord& draw,std::span<const uint8_t> instances) {
    if(draw.side==1 && CyberpunkVR_StereoGpuProbeGroup.load(std::memory_order_relaxed) && CyberpunkVR_StereoGpuProbeState.load(std::memory_order_acquire)==1) {
        std::lock_guard lock(mutex);
        if(!work || !work->group || currentGroup.id!=work->sourceGroup || work->sourceEnded || work->rejected)return;
        if(!work->mixedMaterials && draw.pipeline!=work->sourceDraw.pipeline)return;
        if(work->sceneDepth && work->resolvedEye[0])return;
        const auto ordinal=CyberpunkVR_StereoGpuProbeSourceDraws.load(std::memory_order_relaxed);
        // This bounded test aggregates one native command list per eye. Never
        // append after submission/reset, nor silently compare a partial group.
        if(ordinal>=(work->lateVisibility?MaxGroupDraws:32) || work->producerSubmitted || list!=work->list || !SamePass(work->sourceDraw,draw,work->mixedMaterials) ||
            !camera_upload::CurrentBinding().gpuAddress || !variant){work->rejected=true;return;}
        ComPtr<ID3D12GraphicsCommandList1> views;const auto hr=list->QueryInterface(IID_PPV_ARGS(&views));if(FAILED(hr)){Fail(hr);return;}
        if(!SceneBindings(list,*work))return;
        if(!cvr::gpu::KeepCommandResources(list,{variant})){Fail(E_OUTOFMEMORY);return;}
        probe::Scope scope;Reference(list,*work,draw,0);
        D3D12_CPU_DESCRIPTOR_HANDLE targets[]{work->Rtv(1,0),work->Rtv(1,1),work->Rtv(1,2)};const auto depth=work->Dsv(1);
        list->SetPipelineState(variant);list->OMSetRenderTargets(work->depthOnly?0:3,work->depthOnly?nullptr:targets,FALSE,&depth);
        SharedDraw(list,views.Get(),*work,draw,ordinal,instances);
        CopyMode(list,*work,1);views->SetViewInstanceMask(UINT_MAX);Restore(list,draw);work->routeSequence[0]=draw.sequence;++CyberpunkVR_StereoGpuProbeSourceDraws;return;
    }
    if(draw.side==0 && CyberpunkVR_StereoGpuProbeNativeReference.load(std::memory_order_relaxed) && CyberpunkVR_StereoGpuProbeState.load(std::memory_order_acquire)==1) {
        std::lock_guard lock(mutex);
        if(!work || !work->nativeReference || work->rejected || (draw.flags&127)!=63 || list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT)return;
        if(work->group) {
            if(!currentGroup.id || currentGroup.side!=0 || currentGroup.plane!=work->plane ||
                (work->mainGroup && currentGroup.id!=work->mainGroup) || (!work->mixedMaterials && draw.pipeline!=work->sourceDraw.pipeline) || draw.frame!=work->sourceDraw.frame)return;
            // Native sorting differs between eyes. Start a later MAIN prefix at
            // its first exact member of the captured source group, not merely
            // the first source object (which may sort last or be culled).
            if(work->anchorMain && !work->mainGroup) {
                bool anchor=false;const auto count=CyberpunkVR_StereoGpuProbeSourceDraws.load(std::memory_order_relaxed);
                for(UINT i=0;i<count && !anchor;++i)anchor=SameGeometry(work->sourceItems[i],draw,work->sourceInstances[i],instances);
                if(!anchor)return;
            }
            if(work->sceneDepth && work->resolvedEye[1])return;
            if(!SamePass(work->sourceDraw,draw,work->mixedMaterials)){work->rejected=true;return;}
            if(CyberpunkVR_StereoGpuProbeMainDraws.load(std::memory_order_relaxed)>=(work->lateVisibility?MaxGroupDraws:32) || !work->sourceEnded || !work->producerSubmitted || work->mainSubmitted ||
                (work->mainList && work->mainList!=list)){work->rejected=true;++CyberpunkVR_StereoGpuProbeReferenceSkipped;return;}
            work->mainGroup=currentGroup.id;
        }else if(work->mainRecorded || !SameGeometry(work->sourceDraw,draw))return;
        // Recording MAIN can precede GPU completion. Once the producer has been
        // submitted and its fence enqueued, BeforeSubmit establishes ordering:
        // the same queue already orders both lists; another queue waits on GPU.
        if(!work->producerSubmitted){++CyberpunkVR_StereoGpuProbeReferenceSkipped;return;}
        if(!RetainSurfaces(list,*work)){Fail(E_OUTOFMEMORY);return;}
        if(work->timestamps && !cvr::gpu::KeepCommandResources(list,{work->timestamps.Get()})){Fail(E_OUTOFMEMORY);return;}
        if(!SceneBindings(list,*work))return;
        probe::Scope scope;
        if(work->sceneDepth && !work->imported[1] && !ImportScene(list,*work,draw,1))return;
        const auto ordinal=CyberpunkVR_StereoGpuProbeMainDraws.load(std::memory_order_relaxed);
        bool matched=false;
        if(work->lateVisibility) {
#if CVR_RUST_VRIK
            if(!work->preparationGate || work->preparationGate->Reason()!=0){work->rejected=true;return;}
#else
            if(!work->preparationGate || work->preparationGate->reason.load()!=0){work->rejected=true;return;}
#endif
            const auto count=CyberpunkVR_StereoGpuProbeSourceDraws.load(std::memory_order_relaxed);
            for(UINT i=0;i<count;++i)if(!work->matched[i] && SameGeometry(work->sourceItems[i],draw,work->sourceInstances[i],instances)) {
                if(!work->preparationGate->Publish(static_cast<volatile uint64_t*>(work->predicateData)+i)){work->rejected=true;return;}
                work->matched[i]=true;matched=true;++CyberpunkVR_StereoGpuProbeMatchedDraws;break;
            }
        }
        if(!work->group || work->mainReferenceMask==UINT_MAX || (ordinal<32 && (work->mainReferenceMask&(1u<<ordinal))))Reference(list,*work,draw,1);
        if(work->lateVisibility) {
            if(!matched){Reference(list,*work,draw,1,1);++CyberpunkVR_StereoGpuProbeFallbackDraws;}
            CopyMode(list,*work,1);
        }
        CopyMode(list,*work,0);Restore(list,draw);work->mainList=list;work->mainRecorded=true;work->routeSequence[1]=draw.sequence;++CyberpunkVR_StereoGpuProbeMainDraws;return;
    }
    if(!CyberpunkVR_StereoGpuProbeRequest.load(std::memory_order_relaxed) || CyberpunkVR_StereoGpuProbeState.load(std::memory_order_acquire)!=0 ||
#if CVR_RUST_VRIK
       !cvr_sgps_start(&draw,CyberpunkVR_StereoGpuProbeGroup.load(std::memory_order_relaxed)!=0,CyberpunkVR_StereoGpuProbeDepthPrepass.load()!=0) ||
#else
       draw.side!=1 || ((!CyberpunkVR_StereoGpuProbeGroup.load(std::memory_order_relaxed)) && (draw.indices!=7212 || draw.instances!=1)) || draw.rtCount!=(CyberpunkVR_StereoGpuProbeDepthPrepass.load()?0u:3u) || (draw.rtCount && draw.rtContiguous) || (draw.flags&127)!=63 ||
#endif
       !variant || !camera_upload::CurrentBinding().gpuAddress || list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT)return;
    const auto startIndices=CyberpunkVR_StereoGpuProbeStartIndices.load(std::memory_order_relaxed);
    const auto startRun=CyberpunkVR_StereoGpuProbeStartRun.load(std::memory_order_relaxed);
    if(startIndices && draw.indices!=startIndices)return;
    if(startRun && currentGroup.depthRun!=startRun)return;
    std::lock_guard lock(mutex);if(CyberpunkVR_StereoGpuProbeState.load()!=0 || !CyberpunkVR_StereoGpuProbeRequest.exchange(0))return;
    const bool group=CyberpunkVR_StereoGpuProbeGroup.load()!=0;
    const bool late=CyberpunkVR_StereoGpuProbeLateVisibility.load()!=0;
    const bool scene=CyberpunkVR_StereoGpuProbeSceneDepth.load()!=0;
    const bool route=CyberpunkVR_StereoGpuProbeSceneRoute.load()!=0;
    const auto routeMode=CyberpunkVR_StereoGpuProbeSceneRoute.load();
    const bool mixed=CyberpunkVR_StereoGpuProbeMixedMaterials.load()!=0;
    const auto rateOverride=CyberpunkVR_StereoGpuProbeFineRate.load();
    const bool depthOnly=CyberpunkVR_StereoGpuProbeDepthPrepass.load()!=0;
#if CVR_RUST_VRIK
    const RustProbeSettings settings{currentGroup.id,currentGroup.side,currentGroup.plane,routeMode,rateOverride,startIndices,startRun,
        CyberpunkVR_StereoGpuProbePrepareGateMs.load(),CyberpunkVR_StereoGpuProbeMainViewMask.load(),CyberpunkVR_StereoGpuProbeMainReferenceMask.load(),
        group,late,scene,mixed,depthOnly,CyberpunkVR_StereoGpuProbeNativeReference.load()!=0};
    if(!cvr_sgps_settings(&settings)){Fail(E_INVALIDARG);return;} // the rate override is diagnosis only; visible native VRS stays unchanged
#else
    if(group && (!currentGroup.id || currentGroup.side!=1 || currentGroup.plane==UINT_MAX || !CyberpunkVR_StereoGpuProbeNativeReference.load())){Fail(E_INVALIDARG);return;}
    if(late && (!group || !CyberpunkVR_StereoGpuProbePrepareGateMs.load() || CyberpunkVR_StereoGpuProbeMainViewMask!=UINT_MAX || CyberpunkVR_StereoGpuProbeMainReferenceMask!=UINT_MAX)){Fail(E_INVALIDARG);return;}
    if(scene && !late){Fail(E_INVALIDARG);return;}
    if(route && !scene){Fail(E_INVALIDARG);return;}
    if(routeMode>2){Fail(E_INVALIDARG);return;}
    if(mixed && !scene){Fail(E_INVALIDARG);return;}
    if(rateOverride>2 || (rateOverride && (!scene || route))){Fail(E_INVALIDARG);return;} // diagnosis only; visible native VRS stays unchanged
    if(depthOnly && (!scene || mixed || rateOverride)){Fail(E_INVALIDARG);return;}
    if((startIndices || startRun) && !depthOnly){Fail(E_INVALIDARG);return;}
#endif
#if CVR_RUST_VRIK
    cvr_sgp_reset_start();
#else
    std::memset(CyberpunkVR_StereoGpuProbeVrs,0,sizeof(CyberpunkVR_StereoGpuProbeVrs));
    std::memset(CyberpunkVR_StereoGpuProbeVrsChanges,0,sizeof(CyberpunkVR_StereoGpuProbeVrsChanges));
    std::memset(CyberpunkVR_StereoGpuProbeVrsImage,0,sizeof(CyberpunkVR_StereoGpuProbeVrsImage));
    std::memset(CyberpunkVR_StereoGpuProbeVrsSize,0,sizeof(CyberpunkVR_StereoGpuProbeVrsSize));std::memset(CyberpunkVR_StereoGpuProbeVrsCaptured,0,sizeof(CyberpunkVR_StereoGpuProbeVrsCaptured));
    CyberpunkVR_StereoGpuProbeVrsDifference=0;std::memset(CyberpunkVR_StereoGpuProbeVrsHistogram,0,sizeof(CyberpunkVR_StereoGpuProbeVrsHistogram));
    CyberpunkVR_StereoGpuProbeSceneCommitted=0;for(auto& x:CyberpunkVR_StereoGpuProbeSceneRouted)x=0;for(auto& x:CyberpunkVR_StereoGpuProbeSceneSamples)x=0;
    CyberpunkVR_StereoGpuProbeTimingDrops=0;for(auto& stage:CyberpunkVR_StereoGpuProbeTicks)for(auto& x:stage)x=0;
    for(auto& stage:CyberpunkVR_StereoGpuProbeTimedCount)for(auto& x:stage)x=0;for(auto& x:CyberpunkVR_StereoGpuProbeFrequency)x=0;
    CyberpunkVR_StereoGpuProbeSceneReject=0;for(auto& p:CyberpunkVR_StereoGpuProbeScenePhase)p=0;for(auto& s:CyberpunkVR_StereoGpuProbeSceneSize)s=0;
    CyberpunkVR_StereoGpuProbeReferenceSkipped=0;CyberpunkVR_StereoGpuProbeQueueWaits=0;CyberpunkVR_StereoGpuProbeResult=0;
    CyberpunkVR_StereoGpuProbeDifferentBytes=CyberpunkVR_StereoGpuProbeComparedBytes=0;
    CyberpunkVR_StereoGpuProbeCoverage[0]=CyberpunkVR_StereoGpuProbeCoverage[1]=0;
    for(auto& n:CyberpunkVR_StereoGpuProbeEyeDifferences)n=0;for(auto& n:CyberpunkVR_StereoGpuProbeTargetDifferences)n=0;CyberpunkVR_StereoGpuProbeDifferenceCount=0;
    CyberpunkVR_StereoGpuProbeSourceDraws=1;CyberpunkVR_StereoGpuProbeMainDraws=0;
    CyberpunkVR_StereoGpuProbeMatchedDraws=0;CyberpunkVR_StereoGpuProbeFallbackDraws=0;
    CyberpunkVR_StereoGpuProbePrepareGateReason=0;CyberpunkVR_StereoGpuProbePrepareGateStart=0;
    CyberpunkVR_StereoGpuProbePrepareGateMain=0;CyberpunkVR_StereoGpuProbePrepareGateRelease=0;
#endif
    probe::Scope scope;
    ComPtr<ID3D12Device> device;auto hr=list->GetDevice(IID_PPV_ARGS(&device));if(FAILED(hr)){Fail(hr);return;}
    ComPtr<ID3D12GraphicsCommandList1> views;hr=list->QueryInterface(IID_PPV_ARGS(&views));if(FAILED(hr)){Fail(hr);return;}
    auto pending=std::make_unique<Work>();pending->pipeline=variant;pending->list=list;pending->sourceDraw=draw;pending->nativeReference=CyberpunkVR_StereoGpuProbeNativeReference.load()!=0;
    pending->group=group;pending->sourceGroup=currentGroup.id;pending->plane=currentGroup.plane;
    pending->lateVisibility=late;
    pending->sceneDepth=scene;
    pending->sceneRoute=route;pending->directResolve=routeMode==2;pending->routeSequence[0]=draw.sequence;
    pending->mixedMaterials=mixed;
    pending->rateOverride=rateOverride;
    pending->depthOnly=depthOnly;
    pending->anchorMain=startIndices!=0 || startRun>1;
    if(scene) {
        scene_state::Bindings bindings;
        if(!scene_state::Compatible(reinterpret_cast<ID3D12PipelineState*>(draw.pipeline))){CyberpunkVR_StereoGpuProbeSceneReject=1;Fail(E_NOTIMPL);return;}
        if(!scene_state::Snapshot(list,bindings)){CyberpunkVR_StereoGpuProbeSceneReject=2;Fail(E_NOTIMPL);return;}
        if(!depthOnly && bindings.rate.known && bindings.rate.image)pending->sourceVrs=bindings.rate.image;
        if(!scene_state::CaptureTargets(draw,pending->nativeTargets[0])){CyberpunkVR_StereoGpuProbeSceneReject=3;Fail(E_NOTIMPL);return;}
        pending->width=UINT(draw.viewport.Width);pending->height=UINT(draw.viewport.Height);pending->stencil=bindings.stencil;
        CyberpunkVR_StereoGpuProbeSceneSize[0]=pending->width;CyberpunkVR_StereoGpuProbeSceneSize[1]=pending->height;
    }
    pending->mainViewMask=CyberpunkVR_StereoGpuProbeMainViewMask.load();pending->mainReferenceMask=CyberpunkVR_StereoGpuProbeMainReferenceMask.load();
    hr=Initialize(device.Get(),*pending);if(FAILED(hr)){Fail(hr);return;}
    auto& w=*pending;
    if(!RetainSurfaces(list,w)){Fail(E_OUTOFMEMORY);return;}
    if(w.predicates && !cvr::gpu::KeepCommandResources(list,{w.predicates.Get()})){Fail(E_OUTOFMEMORY);return;}
    if(w.timestamps && !cvr::gpu::KeepCommandResources(list,{w.timestamps.Get()})){Fail(E_OUTOFMEMORY);return;}
    // The native list already owns all original material/mesh/root bindings.
    // Only PSO, targets, viewport, scissor and view mask change, and are restored.
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
    cvr_sgpr_begin(list,variant,&draw,scene,w.width,w.height);
    for(UINT mode=0;mode<2;++mode){
        const D3D12_CPU_DESCRIPTOR_HANDLE targets[]{w.Rtv(mode,0),w.Rtv(mode,1),w.Rtv(mode,2)};
        cvr_sgpr_clear(list,targets,w.Dsv(mode).ptr,w.depthOnly,scene && mode?255:0);
        if(!mode && w.nativeReference){
            if(scene && !ImportScene(list,w,draw,0)){Restore(list,draw);Fail(E_FAIL);return;}
            Reference(list,w,draw,0);list->SetPipelineState(variant);
        }else if(!mode)cvr_sgpr_eyes(list,views.Get(),&draw);
        else SharedDraw(list,views.Get(),w,draw,0,instances);
        if(mode || !w.nativeReference)CopyMode(list,w,mode);
    }
#endif
#else
    const D3D12_VIEWPORT viewport=scene?draw.viewport:D3D12_VIEWPORT{0,0,float(w.width),float(w.height),0,1};const D3D12_RECT scissor=scene?draw.scissor:D3D12_RECT{0,0,LONG(w.width),LONG(w.height)};
    list->SetPipelineState(variant);list->RSSetViewports(1,&viewport);list->RSSetScissorRects(1,&scissor);
    const float zero[4]{};
    for(UINT mode=0;mode<2;++mode) {
        D3D12_CPU_DESCRIPTOR_HANDLE targets[]{w.Rtv(mode,0),w.Rtv(mode,1),w.Rtv(mode,2)};const auto depth=w.Dsv(mode);
        if(!w.depthOnly)for(const auto h:targets)list->ClearRenderTargetView(h,zero,0,nullptr);
        list->ClearDepthStencilView(depth,D3D12_CLEAR_FLAGS(D3D12_CLEAR_FLAG_DEPTH|D3D12_CLEAR_FLAG_STENCIL),0,scene && mode?255:0,0,nullptr);
        list->OMSetRenderTargets(w.depthOnly?0:3,w.depthOnly?nullptr:targets,FALSE,&depth);
        if(!mode && w.nativeReference) {
            if(scene && !ImportScene(list,w,draw,0)){Restore(list,draw);Fail(E_FAIL);return;}
            Reference(list,w,draw,0);list->SetPipelineState(variant);
        } else if(!mode) {
            for(UINT eye=0;eye<2;++eye){views->SetViewInstanceMask(1u<<eye);list->DrawIndexedInstanced(draw.indices,draw.instances,draw.firstIndex,draw.baseVertex,draw.firstInstance);}
        }else SharedDraw(list,views.Get(),w,draw,0,instances);
        if(mode || !w.nativeReference)CopyMode(list,w,mode);
    }
#endif
    views->SetViewInstanceMask(UINT_MAX);Restore(list,draw);
    work=std::move(pending);CyberpunkVR_StereoGpuProbeState.store(1,std::memory_order_release);
}
void BeforeSubmit(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists) {
    if(CyberpunkVR_StereoGpuProbeState.load(std::memory_order_acquire)!=1 || !lists)return;
    std::lock_guard lock(mutex);
    // One bounded diagnostic stall answers whether MAIN's CPU preparation can
    // progress while VRCAM's GPU work is still pending. A separate CPU worker
    // always releases it within the requested (at most 50 ms) timer interval.
    const auto gateMs=std::min(CyberpunkVR_StereoGpuProbePrepareGateMs.load(std::memory_order_relaxed),50u);
    if(work && work->group && !work->rejected && !work->producerSubmitted && !work->mainRecorded && !work->preparationGate && gateMs) {
        for(UINT i=0;i<count;++i)if(lists[i]==work->list) {
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
            std::shared_ptr<PreparationGate> gate;
            try {gate=std::make_shared<PreparationGate>();}catch(const std::bad_alloc&){Fail(E_OUTOFMEMORY);return;}
            const RustGateExports exports{&CyberpunkVR_StereoGpuProbeSceneCommitted,&CyberpunkVR_StereoGpuProbePrepareGateRelease,
                &CyberpunkVR_StereoGpuProbePrepareGateReason,&CyberpunkVR_StereoGpuProbePrepareGateStart};
            const auto hr=cvr_sgpg_begin(queue,gateMs,&exports,&gate->rust);if(gate->rust)work->preparationGate=gate;
            if(FAILED(hr)){Fail(hr);return;}
#endif
#else
            ComPtr<ID3D12Device> device;auto hr=queue->GetDevice(IID_PPV_ARGS(&device));if(FAILED(hr)){Fail(hr);return;}
            std::shared_ptr<PreparationGate> gate;
            try {gate=std::make_shared<PreparationGate>();}catch(const std::bad_alloc&){Fail(E_OUTOFMEMORY);return;}
            hr=device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&gate->fence));if(FAILED(hr)){Fail(hr);return;}
            try {
                std::thread([gate,gateMs]{gate->armed.wait(false);Sleep(gateMs);gate->Release(2);}).detach();
            }catch(const std::system_error&){Fail(E_OUTOFMEMORY);return;}
            work->preparationGate=gate;CyberpunkVR_StereoGpuProbePrepareGateStart=Now();
            hr=queue->Wait(gate->fence.Get(),1);
            gate->armed.store(true,std::memory_order_release);gate->armed.notify_one();
            if(FAILED(hr)){gate->Release(3);Fail(hr);return;}
#endif
            break;
        }
    }
    if(!work || !work->nativeReference || !work->mainRecorded || !work->producerSubmitted)return;
    for(UINT i=0;i<count;++i)if(lists[i]==work->mainList) {
        if(queue!=work->producerQueue.Get()) {
            const auto hr=queue->Wait(work->producerFence.Get(),1);
            if(FAILED(hr)){Fail(hr);return;}
            ++CyberpunkVR_StereoGpuProbeQueueWaits;
        }
        return;
    }
}
void Submitted(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists) {
    if(CyberpunkVR_StereoGpuProbeState.load(std::memory_order_acquire)!=1 || !lists)return;
    std::lock_guard lock(mutex);if(!work || CyberpunkVR_StereoGpuProbeState.load()!=1)return;
    for(UINT i=0;i<count;++i)if(lists[i]==work->list && !work->mainRecorded) {
        if(work->timestamps && FAILED(queue->GetTimestampFrequency(&work->frequency[0]))){Fail(E_FAIL);return;}
        if(work->nativeReference) {
            if(!work->producerSubmitted){const auto hr=queue->Signal(work->producerFence.Get(),1);if(FAILED(hr)){Fail(hr);return;}work->producerQueue=queue;work->producerSubmitted=true;}
        }else {const auto hr=queue->Signal(work->fence.Get(),1);if(FAILED(hr)){Fail(hr);return;}
            CyberpunkVR_StereoGpuProbeState.store(2,std::memory_order_release);return;}
    }
    if(work->nativeReference && work->mainRecorded)for(UINT i=0;i<count;++i)if(lists[i]==work->mainList) {
        if(work->mainSubmitted)return;
        if(work->timestamps && FAILED(queue->GetTimestampFrequency(&work->frequency[1]))){Fail(E_FAIL);return;}
        const auto hr=queue->Signal(work->fence.Get(),1);if(FAILED(hr)){Fail(hr);return;}
        work->mainSubmitted=true;
        if(work->group)FinishGroupIfSubmitted();else CyberpunkVR_StereoGpuProbeState.store(2,std::memory_order_release);return;
    }
}
void Poll() {
    if(CyberpunkVR_StereoGpuProbeState.load(std::memory_order_acquire)==1 && CyberpunkVR_NativeStereoProbeState.load(std::memory_order_acquire)!=1) {
        std::lock_guard lock(mutex);
        if(work && work->nativeReference && work->producerSubmitted && work->producerFence->GetCompletedValue()==1 &&
            (!work->mainRecorded || (work->group && work->mainSubmitted && work->fence->GetCompletedValue()==1))) {
            work.reset();CyberpunkVR_StereoGpuProbeState.store(6,std::memory_order_release);
        }
        return;
    }
    if(CyberpunkVR_StereoGpuProbeState.load(std::memory_order_acquire)!=2)return;
    std::lock_guard lock(mutex);if(!work)return;auto& w=*work;
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
    RustProbeLayout layouts[2][4]{};
    for(UINT mode=0;mode<2;++mode)for(UINT target=0;target<4;++target){const auto& s=w.surfaces[mode][target];auto& l=layouts[mode][target];
        l.planes=s.planes;std::copy(s.footprints.begin(),s.footprints.end(),l.footprints);std::copy(s.rowBytes.begin(),s.rowBytes.end(),l.rowBytes);
        std::copy(s.rows.begin(),s.rows.end(),l.rows);}
    static_assert(sizeof(CyberpunkVR_StereoGpuProbeDifferences[0])==28 && sizeof(w.timed[0])==12,"mirrored by gpu_probe_compare.rs and gpu_probe_timings.rs");
    const RustProbeRead exports{CyberpunkVR_StereoGpuProbeEyeDifferences,CyberpunkVR_StereoGpuProbeTargetDifferences,CyberpunkVR_StereoGpuProbeDifferences,
        &CyberpunkVR_StereoGpuProbeDifferenceCount,CyberpunkVR_StereoGpuProbeSceneSamples,&CyberpunkVR_StereoGpuProbeTimingDrops,
        CyberpunkVR_StereoGpuProbeTicks,CyberpunkVR_StereoGpuProbeTimedCount};
    uint64_t totals[4]{};
    const auto hr=cvr_sgpr_read(w.fence.Get(),w.readback.Get(),w.bytes,layouts,w.width,w.depthOnly,w.sceneDepth?w.queryOffset:UINT64_MAX,
        w.timed.data(),w.timedCount,bool(w.timestamps),w.timestampOffset,&exports,totals);
    if(hr==S_FALSE)return;if(FAILED(hr)){Fail(hr);return;}
    const uint64_t different=totals[0],compared=totals[1],coverage[2]{totals[2],totals[3]};
    if(w.timestamps){CyberpunkVR_StereoGpuProbeFrequency[0]=w.frequency[0];CyberpunkVR_StereoGpuProbeFrequency[1]=w.frequency[1];}
#endif
#else
    const auto completed=w.fence->GetCompletedValue();if(completed==UINT64_MAX){Fail(DXGI_ERROR_DEVICE_REMOVED);return;}if(completed<1)return;
    void* data{};D3D12_RANGE range{0,SIZE_T(w.bytes)};auto hr=w.readback->Map(0,&range,&data);if(FAILED(hr)){Fail(hr);return;}
    const auto* bytes=static_cast<const uint8_t*>(data);uint64_t different{},compared{},coverage[2]{};
    for(UINT target=w.depthOnly?3:0;target<4;++target) {
        const auto& a=w.surfaces[0][target];const auto& b=w.surfaces[1][target];
        for(UINT sub=0;sub<a.planes*2;++sub)for(UINT row=0;row<a.rows[sub];++row) {
            const auto* left=bytes+a.footprints[sub].Offset+row*a.footprints[sub].Footprint.RowPitch;
            const auto* right=bytes+b.footprints[sub].Offset+row*b.footprints[sub].Footprint.RowPitch;
            for(UINT64 byte=0;byte<a.rowBytes[sub];++byte){
                if(left[byte]!=right[byte]){++different;++CyberpunkVR_StereoGpuProbeEyeDifferences[sub%2];++CyberpunkVR_StereoGpuProbeTargetDifferences[target];}++compared;
            }
            const auto stride=a.rowBytes[sub]/w.width;
            if(stride==1 || stride==4)for(UINT x=0;x<w.width && CyberpunkVR_StereoGpuProbeDifferenceCount<128;++x) {
                uint32_t expected{},actual{};std::memcpy(&expected,left+x*stride,size_t(stride));std::memcpy(&actual,right+x*stride,size_t(stride));
                if(expected!=actual)CyberpunkVR_StereoGpuProbeDifferences[CyberpunkVR_StereoGpuProbeDifferenceCount++]={target,sub%2,sub/2,x,row,expected,actual};
            }
            if(target==0 || (w.depthOnly && target==3 && sub<2))for(UINT x=0;x<w.width;++x){uint32_t pixel{};std::memcpy(&pixel,right+x*4,4);coverage[sub]+=pixel!=0;}
        }
    }
    if(w.sceneDepth)std::memcpy(CyberpunkVR_StereoGpuProbeSceneSamples,bytes+w.queryOffset,16);
    if(w.timestamps){
        for(const auto& t:std::span(w.timed.data(),w.timedCount)){uint64_t ticks[2]{};std::memcpy(ticks,bytes+w.timestampOffset+t.index*8,16);
            if(ticks[1]<ticks[0]){++CyberpunkVR_StereoGpuProbeTimingDrops;continue;}
            CyberpunkVR_StereoGpuProbeTicks[t.stage][t.eye]+=ticks[1]-ticks[0];++CyberpunkVR_StereoGpuProbeTimedCount[t.stage][t.eye];}
        CyberpunkVR_StereoGpuProbeFrequency[0]=w.frequency[0];CyberpunkVR_StereoGpuProbeFrequency[1]=w.frequency[1];
    }
    D3D12_RANGE none{};w.readback->Unmap(0,&none);
#endif
    if(w.sourceVrs && CyberpunkVR_StereoGpuProbeVrsCaptured[0] && CyberpunkVR_StereoGpuProbeVrsCaptured[1]){
        std::array<std::array<uint64_t,16>,2> histogram{};auto result=w.vrsSnapshot.Read(CyberpunkVR_StereoGpuProbeVrsDifference,histogram);
        if(FAILED(result)){Fail(result);return;}std::memcpy(CyberpunkVR_StereoGpuProbeVrsHistogram,histogram.data(),sizeof(CyberpunkVR_StereoGpuProbeVrsHistogram));}
    CyberpunkVR_StereoGpuProbeDifferentBytes=different;CyberpunkVR_StereoGpuProbeComparedBytes=compared;
    CyberpunkVR_StereoGpuProbeCoverage[0]=coverage[0];CyberpunkVR_StereoGpuProbeCoverage[1]=coverage[1];CyberpunkVR_StereoGpuProbeResult=S_OK;
    work.reset();CyberpunkVR_StereoGpuProbeState.store(coverage[0] && coverage[1]?3:5,std::memory_order_release);
}
}
#endif
