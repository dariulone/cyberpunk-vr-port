#include "Render/NativeStereoProbe.hpp"
#include "Render/StereoGpuProbe.hpp"
#include "Render/RenderProbeScope.hpp"
#include "Render/StereoDescriptorAudit.hpp"
#include "Render/NativeSubmissionFrontier.hpp"
#include "Render/NativeTargetAccess.hpp"
#include "Render/NativeQueueResidencyScope.hpp"
#include <thread>
#include <mutex>
#include <cstdio>
#include <source_location>
#include <stdexcept>
#include <string>
using namespace cvr::stereo::native_probe;
namespace cvr::framegen {bool Enabled(){return false;}}
namespace cvr::stereo::prepared_pair {bool TestReleasedResetRace();bool TestNativeBindingModes();}
namespace cvr::stereo::retained_geometry {bool TestFrontierFailureDisarm();bool TestFrontierObserverModes();bool TestInfixCaptureLifetime();}
namespace cvr::stereo::constants {
std::recursive_mutex& RegisteredQueueOrderMutex(){static std::recursive_mutex mutex;return mutex;}
}
namespace cvr::stereo::submission_frontier {
bool Install()noexcept{return false;}
bool Reset()noexcept{return false;}
void ObserveSubmit(ID3D12CommandQueue*,UINT,ID3D12CommandList* const*)noexcept{}
bool FindBoundary(ID3D12CommandQueue*,UINT,ID3D12CommandList* const*,const std::array<Source,2>&,Boundary&)noexcept{return false;}
uint32_t LastBoundaryFailure()noexcept{return 1;}
}
// CVR_PROBE_EXPORTS: the Rust variant's own cvr_rust build (cvr-ffi's probe-test) defines the exports the Rust build
// does; this program then defines only the rest.
#if CVR_PROBE_EXPORTS
extern "C" {__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_OwnedSceneSplitEnable{};}
#else
extern "C" {__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_SubmissionFrontierEnable{},CyberpunkVR_OwnedSceneSplitEnable{};}
#endif
namespace cvr::detail {
using PrivateExecuteFn=void(STDMETHODCALLTYPE*)(ID3D12CommandQueue*,UINT,ID3D12CommandList* const*);
PrivateExecuteFn command_queue_original_execute(ID3D12CommandQueue*){return nullptr;}
}
#if !CVR_PROBE_EXPORTS
extern "C" { __declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoGpuProbeRequest{0},CyberpunkVR_StereoGpuProbeNativeReference{0},CyberpunkVR_StereoGpuProbeState{0},CyberpunkVR_StereoGpuProbeSceneDepth{0},CyberpunkVR_StereoGpuProbeDepthPrepass{0}; }
extern "C" { __declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_SinglePassTraceState{}; }
#endif
namespace cvr::stereo::gpu_probe {
void Boundary(ID3D12GraphicsCommandList*,const native_probe::DrawRecord&,bool,ID3D12PipelineState*){throw std::runtime_error("Disabled scene boundary");}
bool RouteNative(ID3D12GraphicsCommandList*,const native_probe::DrawRecord&){return false;}
void Draw(ID3D12GraphicsCommandList*,ID3D12PipelineState*,const native_probe::DrawRecord&,std::span<const uint8_t>){throw std::runtime_error("GPU work must not run in a disabled CPU snapshot test");}
}
static void Require(bool v,std::source_location at=std::source_location::current()){
    if(!v)throw std::runtime_error("Native probe snapshot assertion failed at line "+std::to_string(at.line()));
}
int main() try {
    Require(cvr::stereo::retained_geometry::TestFrontierFailureDisarm());
    Require(cvr::stereo::retained_geometry::TestFrontierObserverModes());
    Require(cvr::stereo::retained_geometry::TestInfixCaptureLifetime());
    Require(cvr::stereo::prepared_pair::TestNativeBindingModes());
    Require(cvr::stereo::prepared_pair::TestReleasedResetRace());
    Require(SetEnvironmentVariableW(L"CVR_STEREO_OBSERVE_BINDINGS",L"1")!=0);
    auto* pso=reinterpret_cast<ID3D12PipelineState*>(0x2000);
    auto* listA=reinterpret_cast<ID3D12GraphicsCommandList*>(0x3000);
    auto* listB=reinterpret_cast<ID3D12GraphicsCommandList*>(0x4000);
    auto* root=reinterpret_cast<ID3D12RootSignature*>(0x5000);
    CyberpunkVR_NativeStereoPipelines[0].original=reinterpret_cast<uintptr_t>(pso);CyberpunkVR_NativeStereoPipelineCount=1;
    Draw(listA,pso,0x33DDA4,1,36,1,0,0,0);Require(CyberpunkVR_NativeStereoDrawCount==0);
    CyberpunkVR_NativeStereoProbeRequest=2;FrameBoundary();
    Reset(listA,pso);Reset(listB,pso);RootSignature(listA,root);RootSignature(listB,root);
    D3D12_VERTEX_BUFFER_VIEW a{0x110000,64,64},b{0x220000,128,64};
    D3D12_INDEX_BUFFER_VIEW index{0x330000,72,DXGI_FORMAT_R16_UINT};
    VertexBuffers(listA,7,1,&a);VertexBuffers(listB,7,1,&b);IndexBuffer(listA,&index);
    RootTable(listA,2,{0x440000});RootCbv(listB,3,0x550000);
    const D3D12_VIEWPORT vp{0,0,1485,1485,0,1};const D3D12_RECT rect{0,0,1485,1485};Viewports(listA,1,&vp);Scissors(listA,1,&rect);
    {cvr::stereo::probe::Scope internal;RootTable(listA,2,{0xBAD});}
    // Contiguous RTV binds expose only one caller-provided handle even for MRT.
    D3D12_CPU_DESCRIPTOR_HANDLE target{0x660000};Targets(listA,4,&target,TRUE,nullptr);
    std::thread t1([&]{Draw(listA,pso,0x33DDA4,1,36,1,0,0,3);});
    std::thread t2([&]{Draw(listB,pso,0x33DDA4,0,36,2,0,0,5);});t1.join();t2.join();
    Require(CyberpunkVR_NativeStereoDrawCount==2);
    for(unsigned n=0;n<2;++n) {
        const auto& r=CyberpunkVR_NativeStereoDraws[n];Require(r.sequence==n+1);
        if(r.list==reinterpret_cast<uintptr_t>(listA)){Require(r.vertices[7].BufferLocation==a.BufferLocation && r.index.BufferLocation==index.BufferLocation);
            Require(r.tables[2]==0x440000 && r.cbvMask==0 && r.rtCount==4 && r.targets[1]==0 && r.flags==63 && r.viewport.Width==1485);}
        else Require(r.vertices[7].BufferLocation==b.BufferLocation && r.cbvs[3]==0x550000 && r.tableMask==0);
    }
    FrameBoundary();Pipeline(listA,pso);Draw(listA,pso,0x33DDA4,1,36,1,0,0,0);
    const auto& cleared=CyberpunkVR_NativeStereoDraws[2];Require(cleared.vertexMask==0 && cleared.root==0 && cleared.tableMask==0);
    FrameBoundary();Require(CyberpunkVR_NativeStereoProbeState==2);const auto count=CyberpunkVR_NativeStereoDrawCount.load();
    Draw(listA,pso,0x33DDA4,1,36,1,0,0,0);Require(CyberpunkVR_NativeStereoDrawCount==count);
    CyberpunkVR_NativeStereoProbeRequest=1;FrameBoundary();auto* other=reinterpret_cast<ID3D12PipelineState*>(0x9000);
    Reset(listA,other);Draw(listA,other,0x33DDA4,1,36,1,0,0,0);Require(CyberpunkVR_NativeStereoDrawCount==0);
    CyberpunkVR_NativeStereoAllDraws=1;Draw(listA,other,0x33DDA4,1,36,1,0,0,0);
    Require(CyberpunkVR_NativeStereoDrawCount==1 && CyberpunkVR_NativeStereoDraws[0].thread!=0 && CyberpunkVR_NativeStereoDraws[0].pipeline==reinterpret_cast<uintptr_t>(other));
    Predication(listA,reinterpret_cast<ID3D12Resource*>(0xA000));
    {cvr::stereo::probe::Scope internal;Predication(listA,nullptr);}
    Draw(listA,other,0x33DDA4,1,36,1,0,0,0);Require((CyberpunkVR_NativeStereoDraws[1].flags&64)!=0);
    Predication(listA,nullptr);Draw(listA,other,0x33DDA4,1,36,1,0,0,0);Require((CyberpunkVR_NativeStereoDraws[2].flags&64)==0);
    D3D12_INPUT_ELEMENT_DESC inputs[2]{};inputs[0].InputSlot=0;inputs[1].InputSlot=7;
    D3D12_GRAPHICS_PIPELINE_STATE_DESC descriptor{};descriptor.InputLayout={inputs,2};
    GraphicsCreated(nullptr,descriptor,other);Draw(listA,other,0x33DDA4,1,36,1,0,0,0);
    Require((CyberpunkVR_NativeStereoDraws[3].flags&128) && CyberpunkVR_NativeStereoDraws[3].inputSlotMask==129);
    DrawRecord indirectState;Require(CurrentState(listA,indirectState) && (indirectState.flags&128) && indirectState.inputSlotMask==129);
    Pipeline(listA,pso);Require(CurrentState(listA,indirectState) && !(indirectState.flags&128) && !indirectState.inputSlotMask);
    Pipeline(listA,other);Require(CurrentState(listA,indirectState) && (indirectState.flags&128) && indirectState.inputSlotMask==129);
    inputs[1].InputSlot=17;GraphicsCreated(nullptr,descriptor,other);Draw(listA,other,0x33DDA4,1,36,1,0,0,0);
    Require(!(CyberpunkVR_NativeStereoDraws[4].flags&128));
    Require(CurrentState(listA,indirectState) && !(indirectState.flags&128) && !indirectState.inputSlotMask);
    inputs[1].InputSlot=7;
    {cvr::stereo::probe::Scope internal;GraphicsCreated(nullptr,descriptor,other);}
    Draw(listA,other,0x33DDA4,1,36,1,0,0,0);Require(!(CyberpunkVR_NativeStereoDraws[5].flags&128));
    ID3D12DescriptorHeap* heaps[]{reinterpret_cast<ID3D12DescriptorHeap*>(0xC000)};
    Heaps(listA,1,heaps);RootTable(listA,2,{0x123000});Heaps(listA,1,heaps);
    DrawRecord state{};Require(CurrentState(listA,state) && state.tableMask==4);
    heaps[0]=reinterpret_cast<ID3D12DescriptorHeap*>(0xD000);Heaps(listA,1,heaps);
    Require(CurrentState(listA,state) && state.tableMask==0 && state.tables[2]==0);
    CyberpunkVR_NativeStereoAllDraws=0;FrameBoundary();
    Require(!(CyberpunkVR_NativeStereoProbeSeq.load()&1));
    cvr::stereo::descriptors::Reset(listA);cvr::stereo::descriptors::Reset(listB);
    const auto savedCount=CyberpunkVR_NativeStereoDrawCount.load(),savedSeq=CyberpunkVR_NativeStereoProbeSeq.load();
    const auto descriptorCount=CyberpunkVR_DescriptorDrawCount.load();
    for(unsigned i=0;i<5;++i){
        FrameBoundary(true);Require(CyberpunkVR_NativeStereoLiveBindings==1 && !CyberpunkVR_NativeStereoLiveFailure && CyberpunkVR_NativeStereoProbeState==1);
        Reset(listA,other);RootSignature(listA,root);RootTable(listA,2,{0xAB0000+i*32});
        VertexBuffers(listA,7,1,&a);Draw(listA,other,0x33DDA4,0,36,1,0,0,0);
        Require(CurrentState(listA,state) && state.root==reinterpret_cast<uintptr_t>(root) && state.tables[2]==0xAB0000+i*32 && !state.instanceResource);
        Require(CyberpunkVR_NativeStereoDrawCount==savedCount && CyberpunkVR_NativeStereoProbeSeq==savedSeq && CyberpunkVR_DescriptorDrawCount==descriptorCount);
    }
    CyberpunkVR_NativeStereoProbeRequest=1;FrameBoundary(true);
    Require(CyberpunkVR_NativeStereoLiveFailure==1 && CyberpunkVR_NativeStereoLiveBindings==1 && CyberpunkVR_NativeStereoProbeRequest==1);
    Require(CyberpunkVR_NativeStereoDrawCount==savedCount && CyberpunkVR_NativeStereoProbeSeq==savedSeq);
    CyberpunkVR_NativeStereoProbeRequest=0;CyberpunkVR_NativeStereoLiveFailure=0;
    FrameBoundary(false);Require(!CyberpunkVR_NativeStereoLiveBindings && !CyberpunkVR_NativeStereoProbeState && !CurrentState(listA,state));
    {
        namespace access=cvr::stereo::target_access;
        cvr::stereo::queue_residency::Invocation invocation;invocation.lists={123};
        Require(!cvr::stereo::queue_residency::DescribeInvocation({},invocation) && invocation.lists.empty() && !invocation.queue);
        access::Clear();access::Arm();Require(access::Observing());access::Finish();Require(!access::Observing());
        const auto finishedSeq=CyberpunkVR_TargetAccessSeq.load();access::Finish();Require(CyberpunkVR_TargetAccessSeq.load()==finishedSeq);
        access::Clear();access::Arm();Require(access::Observing());access::ObserverUnavailable();
        Require(CyberpunkVR_TargetAccessStats[14]==UINT(access::Failure::Unknown));
        access::Finish();access::Clear();access::Arm();Require(!access::Observing());
        Require(CyberpunkVR_TargetAccessStats[14]==UINT(access::Failure::Unknown));
        Require(!access::Query(nullptr,{},{}));Require(!CyberpunkVR_TargetAccessStats[13] && !CyberpunkVR_TargetAccessStats[15]);
    }
    std::puts("PASS disabled recording, independent interleaved lists, concurrent publication, bounded frames, contiguous MRT and stale-state reset");return 0;
}catch(const std::exception& e){std::fprintf(stderr,"%s\n",e.what());return 1;}
