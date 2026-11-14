#include "Runtimes/HudQuad.hpp"
#include "Anim/WheelGrab.hpp"
#include "Render/GpuStageProfile.hpp"
#include "Core/LiveControls.hpp"
#include "Runtimes/HudClearance.hpp"
#include "Runtimes/InteractionDepth.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include <limits>
#include <dxgi1_6.h>
#include <d3d12sdklayers.h>
#include <chrono>
#include <cstdio>
#include <stdexcept>
#include <iostream>
#include <thread>

using Microsoft::WRL::ComPtr;
using namespace cvr::hud;
LiveControls g_liveControls{};
extern "C" int CyberpunkVR_RuntimeDiagnostics=0;
extern "C" std::atomic<uint32_t> CyberpunkVR_HudSubmitRing;
extern "C" cvr::hud::PoseDebug CyberpunkVR_HudPoseDebug;
void Log(const char*,...) {}
void Check(bool value,const char* message) {if(!value)throw std::runtime_error(message);}
void Hr(HRESULT value) {Check(SUCCEEDED(value),"D3D12 call failed");}
// Placement inputs are isolated: no world clearance query or tracked hands.
extern "C" int CyberpunkVR_InteractionAnchor=0,CyberpunkVR_HudClearance=0,CyberpunkVR_HudClearanceHands=0;
extern "C" float CyberpunkVR_HudClearanceGrow=0,CyberpunkVR_HudClearanceHandsScale=1;
namespace cvr::interaction {float TargetDepth(uint64_t) {return 0;}}
namespace cvr::hud::clearance {
float Published(uint64_t) {return kFree;}
float GrowFactor(float,float,float) {return 1;}
}
OpenXRManager& OpenXRManager::Get() {std::abort();}
bool OpenXRManager::GetHandPose(int,OpenXRHeadPose*) const {return false;}
namespace cvr::gpu::profile {
BatchPtr Begin(ID3D12GraphicsCommandList*) {return {};}
Scope::Scope(const BatchPtr&,Stage) {}
Scope::~Scope() {}
void Submitted(BatchPtr,ID3D12CommandQueue*,ID3D12Fence*,UINT64) {}
}
namespace cvr::anim {
bool testCarAnchor=false;
cvr::hud::VehicleMinimapAnchor testAnchor{};
bool WheelReadMinimapAnchor(uint64_t origin,cvr::hud::VehicleMinimapAnchor& out) {
    if(!testCarAnchor || !cvr::hud::FreshVehicleMinimap(testAnchor,GetTickCount64(),origin,42))return false;
    out=testAnchor;return true;
}
}
namespace mock {
ID3D12Device* device{};
std::array<ComPtr<ID3D12Resource>,3> textures;
std::shared_ptr<Frame> source;
bool ready{},acquired{},waited{},timeout{};
uint32_t nextImage{},image{},acquires{},releases{},destroys{};
XrDuration lastTimeout{};
}
namespace cvr::hud {
std::shared_ptr<Frame> Latest(Channel,bool) {return mock::source;}
void SetConsumerReady(bool ready,Channel) {mock::ready=ready;}
void DropConsumer(Channel) {mock::ready=false;}
}
extern "C" {
XrResult XRAPI_CALL xrEnumerateSwapchainFormats(XrSession,uint32_t capacity,uint32_t* count,int64_t* values) {
    *count=1;if(capacity)values[0]=DXGI_FORMAT_R8G8B8A8_UNORM_SRGB;return XR_SUCCESS;
}
XrResult XRAPI_CALL xrCreateSwapchain(XrSession,const XrSwapchainCreateInfo* ci,XrSwapchain* out) {
    D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;
    D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D;
    desc.Width=ci->width;desc.Height=ci->height;desc.Format=DXGI_FORMAT_R8G8B8A8_TYPELESS;
    desc.DepthOrArraySize=desc.MipLevels=1;desc.SampleDesc.Count=1;desc.Flags=D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
    for(auto& texture:mock::textures)Hr(mock::device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,
        D3D12_RESOURCE_STATE_RENDER_TARGET,nullptr,IID_PPV_ARGS(&texture)));
    *out=reinterpret_cast<XrSwapchain>(1);mock::nextImage=0;return XR_SUCCESS;
}
XrResult XRAPI_CALL xrEnumerateSwapchainImages(XrSwapchain,uint32_t capacity,uint32_t* count,XrSwapchainImageBaseHeader* out) {
    *count=3;if(capacity)for(unsigned i=0;i<3;++i)reinterpret_cast<XrSwapchainImageD3D12KHR*>(out)[i].texture=mock::textures[i].Get();return XR_SUCCESS;
}
XrResult XRAPI_CALL xrAcquireSwapchainImage(XrSwapchain,const XrSwapchainImageAcquireInfo*,uint32_t* image) {
    Check(!mock::acquired,"duplicate acquisition");mock::acquired=true;mock::waited=false;
    *image=mock::image=mock::nextImage++%3;++mock::acquires;return XR_SUCCESS;
}
XrResult XRAPI_CALL xrWaitSwapchainImage(XrSwapchain,const XrSwapchainImageWaitInfo* info) {
    Check(mock::acquired,"wait without acquire");mock::lastTimeout=info->timeout;
    if(mock::timeout)return XR_TIMEOUT_EXPIRED;mock::waited=true;return XR_SUCCESS;
}
XrResult XRAPI_CALL xrReleaseSwapchainImage(XrSwapchain,const XrSwapchainImageReleaseInfo*) {
    Check(mock::acquired && mock::waited,"unwaited image released");mock::acquired=false;++mock::releases;return XR_SUCCESS;
}
XrResult XRAPI_CALL xrDestroySwapchain(XrSwapchain) {
    Check(!mock::acquired,"acquired image destroyed");++mock::destroys;
    for(auto& texture:mock::textures)texture.Reset();return XR_SUCCESS;
}
}
void TestNumericGuard() {
    XrCompositionLayerQuad valid{XR_TYPE_COMPOSITION_LAYER_QUAD};
    valid.pose={{0,0,0,1},{0,1.7f,-1}};valid.size={1,1};
    Check(Submittable(valid),"valid quad refused");
    for(float w:{.99f,1.01f,.991f,1.009f,-1.f}) {
        auto quad=valid;quad.pose.orientation.w=w;
        Check(Submittable(quad),"orientation within 1% refused");
    }
    for(float w:{.989f,1.011f,0.f}) {
        auto quad=valid;quad.pose.orientation.w=w;
        Check(!Submittable(quad),"non-unit orientation accepted");
    }
    const float invalid[]={std::numeric_limits<float>::quiet_NaN(),
        std::numeric_limits<float>::infinity(),-std::numeric_limits<float>::infinity()};
    for(float value:invalid) {
        for(auto component:{&XrQuaternionf::x,&XrQuaternionf::y,&XrQuaternionf::z,&XrQuaternionf::w}) {
            auto quad=valid;quad.pose.orientation.*component=value;
            Check(!Submittable(quad),"nonfinite quaternion component accepted");
        }
        for(auto component:{&XrVector3f::x,&XrVector3f::y,&XrVector3f::z}) {
            auto quad=valid;quad.pose.position.*component=value;
            Check(!Submittable(quad),"nonfinite position component accepted");
        }
        for(auto component:{&XrExtent2Df::width,&XrExtent2Df::height}) {
            auto quad=valid;quad.size.*component=value;
            Check(!Submittable(quad),"nonfinite size accepted");
        }
    }
    for(float value:{0.f,-1.f})for(auto component:{&XrExtent2Df::width,&XrExtent2Df::height}) {
        auto quad=valid;quad.size.*component=value;
        Check(!Submittable(quad),"nonpositive size accepted");
    }
    auto huge=valid;huge.pose.orientation={1.6e27f,0,0,.999995f};
    Check(!Submittable(huge),"reported invalid quaternion accepted");
    huge.pose.orientation={std::numeric_limits<float>::max(),0,0,1};
    Check(!Submittable(huge),"overflow-sized quaternion accepted");
}
void TestOptionalLayers() {
    XrCompositionLayerProjection world{XR_TYPE_COMPOSITION_LAYER_PROJECTION};
    XrCompositionLayerQuad good{XR_TYPE_COMPOSITION_LAYER_QUAD}, bad{XR_TYPE_COMPOSITION_LAYER_QUAD};
    good.pose.orientation.w=1;good.size={1,1};bad=good;
    bad.pose.orientation.x=1.6e27f;
    const auto* wp=reinterpret_cast<const XrCompositionLayerBaseHeader*>(&world);
    const auto* gp=reinterpret_cast<const XrCompositionLayerBaseHeader*>(&good);
    const auto* bp=reinterpret_cast<const XrCompositionLayerBaseHeader*>(&bad);
    const XrCompositionLayerBaseHeader* layers[]={wp,bp,nullptr,gp};
    const auto count=1+FilterSubmittableQuads(layers+1,3);
    Check(count==2 && layers[0]==wp && layers[1]==gp,"bad overlay removed the world or changed layer order");
    // A cached pointer can become invalid before the next display cycle.
    good.size.width=std::numeric_limits<float>::quiet_NaN();
    Check(1+FilterSubmittableQuads(layers+1,count-1)==1 && layers[0]==wp,"cached overlay was not revalidated");
    Check(FilterSubmittableQuads(nullptr,0)==0,"empty optional list failed");
}
void TestVehicleAnchor() {
    const float wheel[3]={.1f,.6f,1.f},camera[3]={0,0,1.6f},rotation[4]={0,0,0,1};
    XrPosef head{{0,0,0,1},{0,1.6f,0}},base{{0,0,0,1},{}},pose{};
    Check(BuildVehicleMinimapPose(wheel,camera,rotation,head,base,1,false,pose),"valid wheel anchor refused");
    Check(std::abs(pose.position.x-.1f)<1e-5f && std::abs(pose.position.y-1.08f)<1e-5f &&
        std::abs(pose.position.z+1.52f)<1e-5f,"wheel offset/axis conversion wrong");
    // Leaned back to face the eye: its normal (+z) points at the camera, up and back.
    {
        const auto n=RotateVector(pose.orientation,{0,0,1});
        const XrVector3f toEye{head.position.x-pose.position.x,head.position.y-pose.position.y,head.position.z-pose.position.z};
        const float len=std::sqrt(toEye.x*toEye.x+toEye.y*toEye.y+toEye.z*toEye.z);
        Check(std::abs(n.y-toEye.y/len)<2e-3f && std::abs(n.z-toEye.z/len)<5e-2f,"projected map does not face the eye");
    }
    // The image stays below eye level and beyond arm's reach, like a real HUD's virtual image.
    Check(pose.position.y<head.position.y && -pose.position.z>1.f,"windshield image above the horizon or on the glass");
    const auto expected=pose;
    // Translating and yawing the head changes both sides of the paired camera,
    // not the panel's cabin-space position or orientation.
    const float half=std::sqrt(.5f),turnedCamera[4]={0,0,half,half},movedCamera[3]={.2f,0,1.6f};
    head.orientation={0,half,0,half};head.position.x=.2f;
    Check(BuildVehicleMinimapPose(wheel,movedCamera,turnedCamera,head,base,1,false,pose) &&
        std::abs(pose.position.x-expected.position.x)<1e-5f && std::abs(pose.position.z-expected.position.z)<1e-5f &&
        std::abs(pose.orientation.w-expected.orientation.w)<1e-5f && std::abs(pose.orientation.x-expected.orientation.x)<1e-5f,
        "head turn/lean dragged windshield map");
    base={{0,half,0,half},{2,0,3}};
    Check(BuildVehicleMinimapPose(wheel,movedCamera,turnedCamera,head,base,1,false,pose) &&
        std::abs(pose.position.x-.48f)<1e-5f && std::abs(pose.position.z-2.9f)<1e-5f,"recenter basis omitted");
    Check(!BuildVehicleMinimapPose(wheel,camera,rotation,head,base,0,false,pose),"zero world scale accepted");
    // A bike: over the middle of the bars and a hand above them, half a metre from the eye, facing it.
    {
        const float bars[3]={0,.45f,1.15f};XrPosef straight{{0,0,0,1},{0,1.6f,0}},bike{};
        Check(BuildVehicleMinimapPose(bars,camera,rotation,straight,{{0,0,0,1},{}},1,true,bike),"bike anchor refused");
        Check(std::abs(bike.position.y-(1.6f+1.15f+.12f-1.6f))<1e-5f && std::abs(bike.position.z+(.45f+.06f))<1e-5f,
            "bike map not over the bars");
        const auto n=RotateVector(bike.orientation,{0,0,1});
        Check(n.y>.4f && n.z>.6f,"bike map not leaned toward the rider");
    }
    VehicleMinimapAnchor anchor{expected,1000,7,42};
    Check(FreshVehicleMinimap(anchor,1100,7,42),"fresh anchor expired early");
    Check(!FreshVehicleMinimap(anchor,1101,7,42) && !FreshVehicleMinimap(anchor,999,7,42) &&
        !FreshVehicleMinimap(anchor,1001,8,42) && !FreshVehicleMinimap(anchor,1001,7,43) &&
        !FreshVehicleMinimap(anchor,1001,7,0),"stale/exit/vehicle-switch/recenter retained anchor");
}
int main() try {
    TestNumericGuard();
    TestVehicleAnchor();
    TestOptionalLayers();
    ComPtr<ID3D12Debug> debug;if(SUCCEEDED(D3D12GetDebugInterface(IID_PPV_ARGS(&debug))))debug->EnableDebugLayer();
    ComPtr<IDXGIFactory4> factory;Hr(CreateDXGIFactory1(IID_PPV_ARGS(&factory)));
    ComPtr<IDXGIAdapter> adapter;Hr(factory->EnumWarpAdapter(IID_PPV_ARGS(&adapter)));
    ComPtr<ID3D12Device> device;Hr(D3D12CreateDevice(adapter.Get(),D3D_FEATURE_LEVEL_11_0,IID_PPV_ARGS(&device)));mock::device=device.Get();
    ComPtr<ID3D12CommandQueue> queue;D3D12_COMMAND_QUEUE_DESC qd{};Hr(device->CreateCommandQueue(&qd,IID_PPV_ARGS(&queue)));
    ComPtr<ID3D12Fence> done,gate;Hr(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&done)));
    Hr(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&gate)));uint64_t doneValue{};
    auto drain=[&] {
        Hr(queue->Signal(done.Get(),++doneValue));HANDLE event=CreateEventW(nullptr,FALSE,FALSE,nullptr);
        Hr(done->SetEventOnCompletion(doneValue,event));auto status=WaitForSingleObject(event,5000);CloseHandle(event);Check(status==WAIT_OBJECT_0,"GPU timeout");
    };
    auto frame=[&](uint64_t serial,unsigned size=32) {
        auto f=std::make_shared<Frame>();f->serial=serial;f->stamp=GetTickCount64();f->masked=true;
        D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;
        D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D;desc.Width=desc.Height=size;
        desc.DepthOrArraySize=desc.MipLevels=1;desc.Format=DXGI_FORMAT_R8G8B8A8_UNORM;desc.SampleDesc.Count=1;
        Hr(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_COPY_SOURCE,nullptr,IID_PPV_ARGS(&f->canvas)));
        return f;
    };
    g_liveControls.xrHudStereoDepth=1;g_liveControls.xrHudFollowMode=0;
    XrPosef head{{0,0,0,1},{0,1.7f,0}};XrTime clock=1000000000;
    Quad quad(Channel::Interaction);
    auto prepare=[&](bool enabled=true,bool tracked=true,const XrVector3f* eyes=nullptr) {
        clock+=11111111;
        const auto begin=std::chrono::steady_clock::now();
        auto result=quad.Prepare(reinterpret_cast<XrSession>(1),reinterpret_cast<XrSpace>(1),device.Get(),queue.Get(),enabled,tracked,head,1,clock,eyes);
        if(CyberpunkVR_HudSubmitRing)Check(std::chrono::steady_clock::now()-begin<std::chrono::milliseconds(80),"pipelined prepare blocked");
        return reinterpret_cast<const XrCompositionLayerQuad*>(result);
    };
    // All three pending sources must survive until their own copy completes.
    Hr(queue->Wait(gate.Get(),1));
    std::array<std::weak_ptr<Frame>,3> old;
    for(unsigned i=0;i<3;++i){mock::source=frame(i+1);old[i]=mock::source;Check(prepare()!=nullptr,"free slot unavailable");}
    Check(mock::acquires==3 && mock::releases==3,"ring did not submit three copies");
    mock::source=frame(4);mock::source->masked=false;mock::source->lootVisible=true;
    // Last published image was masked, not loot. Busy fallback must use that
    // metadata, and continue head follow without a new acquisition.
    head.orientation={0,.3f,0,.9539392f};
    Check(prepare()!=nullptr && mock::ready,"busy fallback hid the prior image");
    Check(mock::acquires==3,"busy ring acquired another image");
    for(auto& f:old)Check(!f.expired(),"in-flight HUD source released early");
    Hr(gate->Signal(1));drain();
    Check(prepare()==nullptr && mock::ready,"first unmasked publication handshake changed");
    for(auto& f:old)Check(f.expired(),"completed sources were not retired");
    drain();mock::source=frame(5);Check(prepare()!=nullptr,"new masked publication missing");drain();
    const auto reused=mock::acquires;Check(prepare()!=nullptr && mock::acquires==reused,"same serial copied twice");

    // A nonblocking XR timeout keeps the acquisition and prior published image.
    mock::source=frame(6);mock::timeout=true;
    Check(prepare()!=nullptr && mock::acquired && mock::lastTimeout==0,"XR timeout fallback failed");
    auto pendingAcquires=mock::acquires,pendingReleases=mock::releases;
    Check(prepare()!=nullptr && mock::acquires==pendingAcquires && mock::releases==pendingReleases,"timeout duplicated acquisition/release");
    mock::timeout=false;Check(prepare()!=nullptr && !mock::acquired,"acquisition was not resumed");drain();
    Check(prepare(false)==nullptr && !mock::ready,"disable did not suppress the layer");
    mock::source=frame(7);Check(prepare()!=nullptr,"re-enable failed");drain();

    // Validate final poses in both stereo-depth modes, including cached tracking
    // loss, each eye's offsets, repeated images, and recovery on the next good pose.
    const auto validHead=head;
    for(int stereo:{1,0}) {
        g_liveControls.xrHudStereoDepth=stereo;
        XrVector3f eyes[2]={{-.032f,0,0},{.032f,0,0}};
        Check(prepare(true,true,eyes)!=nullptr,"valid per-eye setup failed");
        head.orientation={1.6e27f,0,0,.999995f};
        Check(prepare(true,true,eyes)==nullptr && !quad.SecondEye() && mock::ready,
            "invalid head orientation reached layer or unmasked native HUD");
        Check(prepare(true,false)==nullptr && !quad.SecondEye(),"cached invalid pose reached layer");
        head=validHead;
        Check(prepare(true,true,eyes)!=nullptr,"good pose did not recover after invalid pose");
        head.position.y=std::numeric_limits<float>::quiet_NaN();
        Check(prepare(true,true,eyes)==nullptr && !quad.SecondEye(),"NaN head position reached layer");
        head=validHead;
        Check(prepare(true,true,eyes)!=nullptr,"good position did not recover");
        if(!stereo)for(unsigned eye=0;eye<2;++eye) {
            eyes[eye].z=std::numeric_limits<float>::infinity();
            Check(prepare(true,true,eyes)==nullptr && !quad.SecondEye(),"invalid eye offset reached layer");
            Check(prepare(true,false)==nullptr && !quad.SecondEye(),"cached invalid eye offset reached layer");
            eyes[eye].z=0;
            Check(prepare(true,true,eyes)!=nullptr && quad.SecondEye(),"valid eye offset did not recover");
        }
        const auto fov=g_liveControls.xrInteractionFov;
        g_liveControls.xrInteractionFov=std::numeric_limits<float>::quiet_NaN();
        Check(prepare(true,true,eyes)==nullptr && !quad.SecondEye(),"NaN size reached layer");
        g_liveControls.xrInteractionFov=fov;
        Check(prepare(true,true,eyes)!=nullptr,"valid size did not recover");
    }
    g_liveControls.xrHudStereoDepth=1;drain();

    // Resize with a pending acquisition must wait/release it before destruction.
    mock::source=frame(8);mock::timeout=true;Check(prepare()!=nullptr && mock::acquired,"pending resize setup");
    mock::source=frame(9,64);const auto destroyedBefore=mock::destroys;
    Check(prepare()==nullptr && mock::destroys==destroyedBefore,"unready image destroyed on resize");
    mock::timeout=false;Check(prepare()!=nullptr && mock::destroys==destroyedBefore+1,"resize recovery failed");drain();
    mock::source=frame(10,64);CyberpunkVR_HudSubmitRing=0;Check(prepare()!=nullptr,"legacy A/B path failed");drain();
    CyberpunkVR_HudSubmitRing=1;mock::source.reset();Check(prepare()==nullptr && !mock::ready,"missing source retained HUD");
    quad.Shutdown();Check(!mock::acquired && mock::acquires==mock::releases,"unbalanced XR lifecycle");

    // Readiness must distinguish preparing a cached layer from publishing a
    // fresh source and from the frame loop successfully submitting that layer.
    Quad mainHud;
    CyberpunkVR_RuntimeDiagnostics=1;mock::source=frame(11,64);
    auto mainLayer=mainHud.Prepare(reinterpret_cast<XrSession>(1),reinterpret_cast<XrSpace>(1),
        device.Get(),queue.Get(),true,true,head,1,clock+11111111);
    Check(mainLayer && CyberpunkVR_HudPoseDebug.sourceSerial==mock::source->serial &&
        CyberpunkVR_HudPoseDebug.sourceTick==mock::source->stamp,"HUD source evidence missing");
    const auto observed=CyberpunkVR_HudPoseDebug.frames;
    drain();mainHud.Prepare(reinterpret_cast<XrSession>(1),reinterpret_cast<XrSpace>(1),
        device.Get(),queue.Get(),true,true,head,1,clock+22222222);
    Check(CyberpunkVR_HudPoseDebug.frames==observed+1 && CyberpunkVR_HudPoseDebug.sourceSerial==11,
        "Cached HUD incorrectly reported a new source");
    Check(CyberpunkVR_HudSubmittedFrames==0,"Preparation is not a successful xrEndFrame");
    CyberpunkVR_RuntimeDiagnostics=0;drain();mainHud.Shutdown();mock::source.reset();

    // The single minimap tile changes placement, not ownership or native widget visibility.
    {
        Quad minimap(Channel::VehicleMinimap);mock::source=frame(30,64);
        mock::source->hudRect={.75f,.1f,.2f,.3f};
        XrCompositionLayerQuad ordinary{XR_TYPE_COMPOSITION_LAYER_QUAD};
        ordinary.pose={{0,0,0,1},{0,1.7f,-2}};ordinary.size={2,2};ordinary.eyeVisibility=XR_EYE_VISIBILITY_BOTH;
        auto make=[&](bool enabled=true,const XrCompositionLayerBaseHeader* main=nullptr) {
            clock+=11111111;
            return reinterpret_cast<const XrCompositionLayerQuad*>(minimap.Prepare(reinterpret_cast<XrSession>(1),
                reinterpret_cast<XrSpace>(1),device.Get(),queue.Get(),enabled,true,head,1,clock,nullptr,0,main));
        };
        const auto* ordinaryHeader=reinterpret_cast<const XrCompositionLayerBaseHeader*>(&ordinary);
        auto result=make(true,ordinaryHeader);
        Check(result && std::abs(result->pose.position.x-.7f)<1e-5f &&
            std::abs(result->pose.position.y-2.2f)<1e-5f && std::abs(result->size.width-.4f)<1e-5f,
            "ordinary minimap layout did not follow Main exactly");drain();
        cvr::anim::testCarAnchor=true;
        cvr::anim::testAnchor={{{0,0,0,1},{.1f,1.4f,-.8f}},GetTickCount64(),1,42};
        result=make(true,ordinaryHeader);
        Check(!result && mock::ready,"opaque minimap flashed onto windshield before its projection style was ready");
        mock::source=frame(31,64);mock::source->hudRect={.75f,.1f,.2f,.3f};mock::source->windshieldMap=true;
        result=make(true,ordinaryHeader);
        Check(result && result->pose.position.x==.1f && result->pose.position.z==-.8f &&
            result->size.width==.20f && result->size.height==.12f &&
            result->eyeVisibility==XR_EYE_VISIBILITY_BOTH && !minimap.SecondEye(),"car minimap not cabin-locked stereo");
        drain();cvr::anim::testCarAnchor=false;
        result=make(true,ordinaryHeader);
        Check(!result && mock::ready,"projected minimap flashed back into the ordinary HUD on exit");
        mock::source=frame(32,64);mock::source->hudRect={.75f,.1f,.2f,.3f};
        result=make(true,ordinaryHeader);
        Check(result && std::abs(result->pose.position.x-.7f)<1e-5f,"exit/bike did not restore ordinary minimap");
        cvr::anim::testCarAnchor=true;cvr::anim::testAnchor.origin=2;
        result=make(true,ordinaryHeader);
        Check(result && std::abs(result->pose.position.x-.7f)<1e-5f,"recenter reused old car anchor");
        cvr::anim::testCarAnchor=false;
        Check(!make() && !mock::ready,"missing Main retained stale minimap or native mask");
        Check(!make(false,ordinaryHeader) && !mock::ready,"disabled HUD retained minimap mask");
        drain();minimap.Shutdown();mock::source.reset();
    }

    // All HUD channels use the same final guard, whether head locked or yaw-only.
    for(auto channel:{Channel::Main,Channel::Interaction,Channel::Basilisk,Channel::Surveillance,Channel::HitMarker}) {
        Quad panel(channel);mock::source=frame(12+Index(channel),64);
        const XrVector3f eyes[2]={{-.032f,0,0},{.032f,0,0}};
        auto make=[&] {
            clock+=11111111;
            return panel.Prepare(reinterpret_cast<XrSession>(1),reinterpret_cast<XrSpace>(1),
                device.Get(),queue.Get(),true,true,head,1,clock,eyes);
        };
        Check(make()!=nullptr,"channel valid setup failed");drain();
        head.position.x=std::numeric_limits<float>::quiet_NaN();
        Check(!make() && !panel.SecondEye(),"channel bypassed invalid final pose guard");
        head=validHead;Check(make()!=nullptr,"channel valid pose did not recover");
        drain();panel.Shutdown();mock::source.reset();
    }

    ComPtr<ID3D12InfoQueue> info;
    if(SUCCEEDED(device.As(&info)))for(UINT64 i=0;i<info->GetNumStoredMessages();++i) {
        SIZE_T bytes{};info->GetMessage(i,nullptr,&bytes);std::vector<uint8_t> storage(bytes);auto* message=reinterpret_cast<D3D12_MESSAGE*>(storage.data());
        info->GetMessage(i,message,&bytes);
        if(message->Severity<=D3D12_MESSAGE_SEVERITY_ERROR){std::cerr<<message->pDescription<<'\n';throw std::runtime_error("D3D12 validation failed");}
    }
    std::cout<<"PASS delayed GPU, three fenced slots, source lifetime, busy/stale metadata, XR timeout recovery, repeat, disable, resize and legacy switch\n";
    return 0;
} catch(const std::exception& e) {std::cerr<<e.what()<<'\n';return 1;}
