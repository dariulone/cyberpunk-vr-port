#include "Runtimes/HudFollow.hpp"
#include "Camera/GazeCenter.hpp"
#include "Utils/DebugGate.hpp"
extern "C" { int CyberpunkVR_RuntimeDiagnostics=0; }
#include "Render/ColorBlit.hpp"
#include "Render/HudPanel.hpp"
#include "Runtimes/HudQuad.hpp"
#include "Render/GpuStageProfile.hpp"
extern "C" { extern cvr::hud::PoseDebug CyberpunkVR_HudPoseDebug; extern std::atomic<uint32_t> CyberpunkVR_HudPoseDebugSeq; }
#include "Core/LiveControls.hpp"
#include "Runtimes/HudLayout.hpp"
#include "Runtimes/HudCrop.hpp"
#include <dxgi1_6.h>
#include <iostream>
#include <stdexcept>
#include <limits>
#include <string>
#include <cstdarg>
#include <array>

using Microsoft::WRL::ComPtr;
void Log(const char* format, ...) { va_list args; va_start(args,format); vprintf(format,args); va_end(args); }
// Synthetic source textures have no engine handle. Real native release is tested in-game.
cvr::hud::TextureLease::~TextureLease()=default;
void Check(bool ok,const char* message) { if (!ok) throw std::runtime_error(message); }
void Hr(HRESULT result) { Check(SUCCEEDED(result),"D3D12 call failed"); }
namespace cvr::gpu::profile {
BatchPtr Begin(ID3D12GraphicsCommandList*) {return {};}
Scope::Scope(const BatchPtr&,Stage) {}
Scope::~Scope() {}
void Submitted(BatchPtr,ID3D12CommandQueue*,ID3D12Fence*,UINT64) {}
}
// The optics probe rides on the HUD capture in the plugin; nothing to probe here.
namespace cvr::optics::probe { void Capture(ID3D12Device*,ID3D12CommandQueue*) {} }
// The engine's own HUD bend, kept by Stereo/Hud.cpp (not linked here) for the panel composite.
extern "C" float CyberpunkVR_HudEngineCurvature[2]={0.0f,0.0f};
constexpr float rad=0.01745329252f;

LiveControls g_liveControls{};
ComPtr<ID3D12Resource> testHudConstants,testFrameConstants;
extern "C" ID3D12Resource* CyberpunkVR_GetFrameConstantBuffer() {return testFrameConstants.Get();}
extern "C" ID3D12Resource* CyberpunkVR_GetHudConstantBuffer() {return testHudConstants.Get();}
extern "C" ID3D12Resource* CyberpunkVR_GetHudExposureBuffer() {return nullptr;}
namespace xrtest {
ID3D12Device* device=nullptr;
ComPtr<ID3D12Resource> image;
bool acquired=false,waited=false,timeout=false;
int acquireCalls=0,releaseCalls=0;
}
extern "C" {
XRAPI_ATTR XrResult XRAPI_CALL xrEnumerateSwapchainFormats(XrSession,uint32_t cap,uint32_t* count,int64_t* values) {
    *count=1;if(cap)values[0]=DXGI_FORMAT_R8G8B8A8_UNORM_SRGB;return XR_SUCCESS;
}
XRAPI_ATTR XrResult XRAPI_CALL xrCreateSwapchain(XrSession,const XrSwapchainCreateInfo* info,XrSwapchain* out) {
    D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;
    D3D12_RESOURCE_DESC d{};d.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D;d.Width=info->width;d.Height=info->height;
    d.DepthOrArraySize=1;d.MipLevels=1;d.SampleDesc.Count=1;d.Format=DXGI_FORMAT_R8G8B8A8_TYPELESS;d.Flags=D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
    if(FAILED(xrtest::device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&d,D3D12_RESOURCE_STATE_RENDER_TARGET,nullptr,IID_PPV_ARGS(&xrtest::image))))return XR_ERROR_RUNTIME_FAILURE;
    *out=reinterpret_cast<XrSwapchain>(1);return XR_SUCCESS;
}
XRAPI_ATTR XrResult XRAPI_CALL xrEnumerateSwapchainImages(XrSwapchain,uint32_t cap,uint32_t* count,XrSwapchainImageBaseHeader* images) {
    *count=1;if(cap)reinterpret_cast<XrSwapchainImageD3D12KHR*>(images)->texture=xrtest::image.Get();return XR_SUCCESS;
}
XRAPI_ATTR XrResult XRAPI_CALL xrAcquireSwapchainImage(XrSwapchain,const XrSwapchainImageAcquireInfo*,uint32_t* index) {
    if(xrtest::acquired)return XR_ERROR_CALL_ORDER_INVALID;
    xrtest::acquired=true;xrtest::waited=false;++xrtest::acquireCalls;*index=0;return XR_SUCCESS;
}
XRAPI_ATTR XrResult XRAPI_CALL xrWaitSwapchainImage(XrSwapchain,const XrSwapchainImageWaitInfo*) {
    if(!xrtest::acquired)return XR_ERROR_CALL_ORDER_INVALID;
    if(xrtest::timeout){xrtest::timeout=false;return XR_TIMEOUT_EXPIRED;}
    xrtest::waited=true;return XR_SUCCESS;
}
XRAPI_ATTR XrResult XRAPI_CALL xrReleaseSwapchainImage(XrSwapchain,const XrSwapchainImageReleaseInfo*) {
    if(!xrtest::acquired || !xrtest::waited)return XR_ERROR_CALL_ORDER_INVALID;
    xrtest::acquired=false;++xrtest::releaseCalls;return XR_SUCCESS;
}
XRAPI_ATTR XrResult XRAPI_CALL xrDestroySwapchain(XrSwapchain) {xrtest::image.Reset();xrtest::acquired=false;return XR_SUCCESS;}
}


struct Gpu {
    ComPtr<ID3D12Device> device; ComPtr<ID3D12CommandQueue> queue;
    ComPtr<ID3D12CommandAllocator> allocator; ComPtr<ID3D12GraphicsCommandList> cmd;
    ComPtr<ID3D12Fence> fence; uint64_t value=0; HANDLE event=CreateEventW(nullptr,FALSE,FALSE,nullptr);
    Gpu() {
        ComPtr<IDXGIFactory4> factory; Hr(CreateDXGIFactory1(IID_PPV_ARGS(&factory)));
        ComPtr<IDXGIAdapter> warp; Hr(factory->EnumWarpAdapter(IID_PPV_ARGS(&warp)));
        Hr(D3D12CreateDevice(warp.Get(),D3D_FEATURE_LEVEL_11_0,IID_PPV_ARGS(&device)));
        D3D12_COMMAND_QUEUE_DESC q{}; Hr(device->CreateCommandQueue(&q,IID_PPV_ARGS(&queue)));
        Hr(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,IID_PPV_ARGS(&allocator)));
        Hr(device->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_DIRECT,allocator.Get(),nullptr,IID_PPV_ARGS(&cmd)));
        Hr(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&fence)));
    }
    ~Gpu() { CloseHandle(event); }
    void Wait() { Hr(queue->Signal(fence.Get(),++value)); Hr(fence->SetEventOnCompletion(value,event)); Check(WaitForSingleObject(event,5000)==WAIT_OBJECT_0,"GPU timeout"); }
    void Submit() { Hr(cmd->Close()); ID3D12CommandList* lists[]={cmd.Get()};queue->ExecuteCommandLists(1,lists);Wait(); }
    void Reset() { Hr(allocator->Reset()); Hr(cmd->Reset(allocator.Get(),nullptr)); }
    ComPtr<ID3D12Resource> Buffer(size_t bytes,D3D12_HEAP_TYPE type,D3D12_RESOURCE_STATES state) {
        D3D12_HEAP_PROPERTIES heap{};heap.Type=type;
        D3D12_RESOURCE_DESC d{};d.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;d.Width=bytes;d.Height=1;
        d.DepthOrArraySize=1;d.MipLevels=1;d.SampleDesc.Count=1;d.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
        ComPtr<ID3D12Resource> r;Hr(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&d,state,nullptr,IID_PPV_ARGS(&r)));return r;
    }
    ComPtr<ID3D12Resource> Texture(unsigned w,unsigned h,D3D12_RESOURCE_STATES state,bool rt=false) {
        D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;
        D3D12_RESOURCE_DESC d{};d.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D;d.Width=w;d.Height=h;
        d.DepthOrArraySize=1;d.MipLevels=1;d.SampleDesc.Count=1;d.Format=DXGI_FORMAT_R8G8B8A8_TYPELESS;
        if(rt)d.Flags=D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
        ComPtr<ID3D12Resource> r;Hr(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&d,state,nullptr,IID_PPV_ARGS(&r)));return r;
    }
    void Barrier(ID3D12Resource* r,D3D12_RESOURCE_STATES before,D3D12_RESOURCE_STATES after) {
        D3D12_RESOURCE_BARRIER b{};b.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        b.Transition={r,D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,before,after};cmd->ResourceBarrier(1,&b);
    }
};

void GpuTest(bool leases, const std::string& quadCase = "") {
    // These cases exercise angular/infinity placement, regardless of the
    // configured default for ordinary finite-depth HUD panels.
    if(quadCase=="gpu_quad_basilisk" || quadCase=="gpu_quad_recenter" ||
       quadCase=="gpu_quad_alignment" || quadCase=="gpu_quad_projection")g_liveControls.xrHudStereoDepth=0;
    Gpu gpu;
    const bool styleCase=quadCase=="gpu_style";
    testHudConstants=gpu.Buffer(512,D3D12_HEAP_TYPE_UPLOAD,D3D12_RESOURCE_STATE_GENERIC_READ);
    testFrameConstants=gpu.Buffer(512,D3D12_HEAP_TYPE_UPLOAD,D3D12_RESOURCE_STATE_GENERIC_READ);
    void* constants=nullptr;Hr(testHudConstants->Map(0,nullptr,&constants));std::memset(constants,0,512);
    auto values=static_cast<float*>(constants);values[26]=1;values[66]=128;values[67]=8;
    if(styleCase) {values[14]=0;values[15]=0.25f;values[16]=1;} // two-pixel native shadow below the tile
    testHudConstants->Unmap(0,nullptr);
    Hr(testFrameConstants->Map(0,nullptr,&constants));std::memset(constants,0,512);testFrameConstants->Unmap(0,nullptr);
    g_liveControls.xrHudBrightness=styleCase ? 1.0f : 0.5f;
    g_liveControls.xrHudShadow=styleCase ? 1.0f : 0.0f;g_liveControls.xrHudGlow=0;
    auto snapshot=std::make_shared<cvr::hud::Snapshot>();snapshot->width=128;snapshot->height=8;snapshot->masked=true;
    std::vector<ComPtr<ID3D12Resource>> uploads;
    for(unsigned i=0;i<64;++i) {
        auto texture=gpu.Texture(1,1,D3D12_RESOURCE_STATE_COPY_DEST);
        auto upload=gpu.Buffer(256,D3D12_HEAP_TYPE_UPLOAD,D3D12_RESOURCE_STATE_GENERIC_READ);
        void* mapped=nullptr;Hr(upload->Map(0,nullptr,&mapped));
        const unsigned char rgba[4]={static_cast<unsigned char>(32+i*3),0,0,255};memcpy(mapped,rgba,4);upload->Unmap(0,nullptr);
        D3D12_TEXTURE_COPY_LOCATION dst{};dst.pResource=texture.Get();dst.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
        D3D12_TEXTURE_COPY_LOCATION src{};src.pResource=upload.Get();src.Type=D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;
        src.PlacedFootprint.Footprint={DXGI_FORMAT_R8G8B8A8_UNORM,1,1,1,256};gpu.cmd->CopyTextureRegion(&dst,0,0,0,&src,nullptr);
        gpu.Barrier(texture.Get(),D3D12_RESOURCE_STATE_COPY_DEST,static_cast<D3D12_RESOURCE_STATES>(0xc0));
        cvr::hud::Sprite sprite;sprite.texture=std::make_shared<cvr::hud::TextureLease>();sprite.texture->resource=texture;
        sprite.x=float(i*2);sprite.y=2;sprite.width=2;sprite.height=4;
        sprite.tint[0]=sprite.tint[1]=sprite.tint[2]=0.25f;sprite.tint[3]=0.5f;
        snapshot->sprites.push_back(sprite);uploads.push_back(upload);
    }
    gpu.Submit();
    std::vector<std::shared_ptr<cvr::hud::Frame>> held;
    for(unsigned pass=0;pass<(leases?5u:1u);++pass) {
        auto next=std::make_shared<cvr::hud::Snapshot>(*snapshot);next->stamp=GetTickCount64();next->generation=pass+1;
        cvr::hud::Publish(next);cvr::hud::Capture(gpu.device.Get(),gpu.queue.Get());gpu.Wait();
        auto frame=cvr::hud::Latest();Check(frame!=nullptr,"panel capture missing");
        const bool lootCase=quadCase=="gpu_quad_loot";
        const bool interaction=quadCase=="gpu_quad_interaction" || lootCase;
        const bool basilisk=quadCase=="gpu_quad_basilisk";
        const bool sniper=quadCase=="gpu_quad_sniper";
        const bool gazeCase=quadCase=="gpu_quad_gaze";
        const bool surveillance=quadCase=="gpu_quad_surveillance" || sniper || gazeCase;
        if(gazeCase)cvr::gaze::Publish({-.231229f,-.1931f});
        const auto channel=surveillance?cvr::hud::Channel::Surveillance:
            (basilisk?cvr::hud::Channel::Basilisk:(interaction?cvr::hud::Channel::Interaction:cvr::hud::Channel::Main));
        if(sniper)Check(cvr::hud::EntryChannel(cvr::hud::EntryHash("briefing_sequence_player"),true)==channel,
                        "sniper briefing root did not select its head-locked channel");
        if(interaction || basilisk || surveillance){
            auto other=std::make_shared<cvr::hud::Snapshot>(*next);other->generation=99;
            other->lootVisible=lootCase;
            cvr::hud::Publish(other,channel);
            cvr::hud::Capture(gpu.device.Get(),gpu.queue.Get(),channel);gpu.Wait();
            auto otherFrame=cvr::hud::Latest(channel);
            Check(otherFrame && otherFrame->canvas!=frame->canvas && frame->generation!=99 && otherFrame->generation==99,"HUD channels alias textures or identity");
            cvr::hud::SetConsumerReady(true);cvr::hud::SetConsumerReady(false,cvr::hud::Channel::Interaction);
            Check(cvr::hud::ConsumerReady() && !cvr::hud::ConsumerReady(cvr::hud::Channel::Interaction),"consumer readiness leaked across channels");
        }
        if (!quadCase.empty() && !styleCase) {
            xrtest::device=gpu.device.Get(); cvr::hud::Quad quad(channel);
            XrPosef head{};head.orientation.w=1;head.position={1,2,3};
            XrVector3f eyes[2]={{-.032f,0,0},{.032f,0,0}};
            XrTime time=1000000000;float bodyYaw=0;
            auto prepare=[&](bool tracked=true,uint64_t origin=1) {time+=11111111;return quad.Prepare(reinterpret_cast<XrSession>(1),reinterpret_cast<XrSpace>(1),gpu.device.Get(),gpu.queue.Get(),true,tracked,head,origin,time,tracked?eyes:nullptr,bodyYaw);};
            if(basilisk || surveillance){
                g_liveControls.xrHudFollowMode=1;g_liveControls.xrHudFollowDeg=90;bodyYaw=1;
                for(float a:{.001f,.4f,-.6f,1.2f,0.f}) {
                    // Rotation around a tilted axis exercises yaw, pitch, roll
                    // and micro-movements below every other HUD cone.
                    head.orientation={sinf(a)*.36f,sinf(a)*.48f,sinf(a)*.8f,cosf(a)};
                    auto l=reinterpret_cast<const XrCompositionLayerQuad*>(prepare());
                    Check(l && quad.SecondEye(),"Basilisk HUD missing from an eye");
                    Check(std::abs(l->pose.orientation.x-head.orientation.x)<1e-6f &&
                          std::abs(l->pose.orientation.y-head.orientation.y)<1e-6f &&
                          std::abs(l->pose.orientation.z-head.orientation.z)<1e-6f &&
                          std::abs(l->pose.orientation.w-head.orientation.w)<1e-6f,"Basilisk HUD lagged, followed body, or retained a cone");
                    const float d=g_liveControls.xrHudDistance;
                    const auto& q=head.orientation;
                    const float gy=gazeCase?cvr::gaze::Read().tangentY*d:0;
                    const float expectedX=head.position.x-2*(q.x*q.z+q.w*q.y)*d+eyes[0].x+2*(q.x*q.y-q.w*q.z)*gy;
                    const float expectedY=head.position.y-2*(q.y*q.z-q.w*q.x)*d+eyes[0].y+(1-2*(q.x*q.x+q.z*q.z))*gy;
                    const float expectedZ=head.position.z-(1-2*(q.x*q.x+q.y*q.y))*d+eyes[0].z+2*(q.y*q.z+q.w*q.x)*gy;
                    Check(std::abs(l->pose.position.x-expectedX)<1e-5f && std::abs(l->pose.position.y-expectedY)<1e-5f &&
                          std::abs(l->pose.position.z-expectedZ)<1e-5f,"Basilisk HUD centre did not follow pitch/yaw immediately");
                    const auto* right=reinterpret_cast<const XrCompositionLayerQuad*>(quad.SecondEye());
                    Check(std::abs((right->pose.position.x-eyes[1].x)-(l->pose.position.x-eyes[0].x))<1.e-6f &&
                          std::abs(right->pose.position.y-l->pose.position.y)<1.e-6f,"gaze differs between eyes");
                }
            } else if(lootCase){
                g_liveControls.xrHudFollowMode=1;bodyYaw=1;g_liveControls.xrHudFollowDeg=5;
                g_liveControls.xrInteractionFollowDeg=90;
                Check(g_liveControls.xrLootFollowDeg==0,"loot default must be 0 degrees (head-locked)");
                g_liveControls.xrLootFollowDeg=10;
                Check(cvr::hud::Latest(cvr::hud::Channel::Interaction)->lootVisible && !frame->lootVisible,
                      "loot context missing from capture or leaked into main HUD");
                auto yaw=[&] {
                    auto l=reinterpret_cast<const XrCompositionLayerQuad*>(prepare());
                    Check(l!=nullptr,"loot layer missing");
                    return 2*std::atan2(l->pose.orientation.y,l->pose.orientation.w);
                };
                Check(yaw()==0,"initial loot heading changed");
                head.orientation={0,sinf(4.5f*rad),0,cosf(4.5f*rad)};
                for(int i=0;i<400;++i)Check(yaw()==0,"loot followed inside its 10-degree cone");
                head.orientation={0,sinf(6*rad),0,cosf(6*rad)};
                Check(yaw()>0,"loot inherited the 90-degree interaction cone");
                for(int i=0;i<40;++i)yaw();
                const float settled=yaw();
                Check(std::abs(settled-12*rad)<2.1f*rad,"loot catch-up did not settle");
                auto closed=std::make_shared<cvr::hud::Snapshot>(*next);closed->stamp=GetTickCount64();closed->lootVisible=false;
                cvr::hud::Publish(closed,cvr::hud::Channel::Interaction);
                cvr::hud::Capture(gpu.device.Get(),gpu.queue.Get(),cvr::hud::Channel::Interaction);gpu.Wait();
                Check(!cvr::hud::Latest(cvr::hud::Channel::Interaction)->lootVisible,"loot close did not reach the captured frame");
                const float headYaw=settled+80*rad;
                head.orientation={0,sinf(headYaw*.5f),0,cosf(headYaw*.5f)};
                for(int i=0;i<400;++i)Check(std::abs(yaw()-settled)<1e-6f,"dialog cone or yaw anchor changed after closing loot");
                auto reopened=std::make_shared<cvr::hud::Snapshot>(*closed);reopened->stamp=GetTickCount64();reopened->lootVisible=true;
                cvr::hud::Publish(reopened,cvr::hud::Channel::Interaction);
                cvr::hud::Capture(gpu.device.Get(),gpu.queue.Get(),cvr::hud::Channel::Interaction);gpu.Wait();
                const float nextYaw=yaw();
                Check(nextYaw>settled && nextYaw-settled<3*rad,"opening loot did not smoothly apply its own cone");
            } else if(interaction){
                g_liveControls.xrHudFollowMode=1;bodyYaw=1;g_liveControls.xrHudFollowDeg=5;
                Check(g_liveControls.xrInteractionFollowDeg==0 && g_liveControls.xrInteractionDistance==2 &&
                      g_liveControls.xrInteractionFov==75,"interaction defaults must be cone 0, 2 m, 75 degrees");
                // A zero cone is head-locked: the full HMD orientation, at once.
                head.orientation={0,sinf(20*rad),0,cosf(20*rad)};
                {auto l=reinterpret_cast<const XrCompositionLayerQuad*>(prepare());
                 Check(l && std::abs(l->pose.orientation.y-head.orientation.y)<1e-6f &&
                       std::abs(l->pose.orientation.w-head.orientation.w)<1e-6f,"zero interaction cone did not lock to the head");}
                head.orientation={0,0,0,1};
                g_liveControls.xrInteractionFollowDeg=90;g_liveControls.xrInteractionDistance=2;
                Check(prepare()!=nullptr,"interaction layer missing");
                head.orientation={0,sinf(40*rad),0,cosf(40*rad)};
                for(int i=0;i<400;++i){auto l=reinterpret_cast<const XrCompositionLayerQuad*>(prepare());
                    Check(l && l->pose.orientation.y==0,"interaction followed body, HUD cone or three-second rest timer");}
                head.orientation={0,sinf(50*rad),0,cosf(50*rad)};
                auto l=reinterpret_cast<const XrCompositionLayerQuad*>(prepare());
                Check(l && l->pose.orientation.y>0,"interaction failed to follow beyond 90 degrees");
                for(int i=0;i<100;++i)l=reinterpret_cast<const XrCompositionLayerQuad*>(prepare());
                Check(std::abs(2*std::atan2(l->pose.orientation.y,l->pose.orientation.w)-100*rad)<2.1f*rad,"interaction catch-up did not settle");
                g_liveControls.xrInteractionPanel=0;
                Check(!prepare() && !cvr::hud::ConsumerReady(cvr::hud::Channel::Interaction) && cvr::hud::ConsumerReady(),"disabling interaction changed main HUD readiness");
            } else if(quadCase=="gpu_quad_timeout") {
                xrtest::timeout=true;Check(!prepare(),"timeout was treated as successful wait");
                Check(xrtest::acquireCalls==1 && xrtest::releaseCalls==0,"timed out image was released");
                Check(prepare()!=nullptr,"timed out image could not be resumed");
                Check(xrtest::acquireCalls==1 && xrtest::releaseCalls==1,"acquired a second image before finishing the first");
            } else if(quadCase=="gpu_quad_serial") {
                Check(prepare()!=nullptr,"first panel missing");gpu.Wait();
                auto sameTime=std::make_shared<cvr::hud::Snapshot>(*snapshot);sameTime->stamp=frame->stamp;
                cvr::hud::Publish(sameTime);cvr::hud::Capture(gpu.device.Get(),gpu.queue.Get());gpu.Wait();
                Check(prepare()!=nullptr && xrtest::acquireCalls==2,"different captures with same timestamp were aliased");
            } else if(quadCase=="gpu_quad_alignment" || quadCase=="gpu_quad_projection") {
                for(int i=0;i<80;++i) {
                    float yaw=float(i)*.02f;head.orientation={0,sinf(yaw*.5f),0,cosf(yaw*.5f)};
                    head.position={sinf(yaw)*.4f,1.6f+cosf(yaw)*.1f,-cosf(yaw)*.2f};
                    eyes[0]={-.032f*cosf(yaw),0,.032f*sinf(yaw)};eyes[1]={-eyes[0].x,0,-eyes[0].z};
                    auto l=reinterpret_cast<const XrCompositionLayerQuad*>(prepare());
                    auto r=reinterpret_cast<const XrCompositionLayerQuad*>(quad.SecondEye());
                    Check(l && r && l->eyeVisibility==XR_EYE_VISIBILITY_LEFT && r->eyeVisibility==XR_EYE_VISIBILITY_RIGHT,"eye-specific HUD layers missing");
                    Check(std::abs((l->pose.position.x-eyes[0].x)-(r->pose.position.x-eyes[1].x))<1e-6f &&
                          std::abs((l->pose.position.z-eyes[0].z)-(r->pose.position.z-eyes[1].z))<1e-6f,"HUD angular positions differ between eyes");
                    if(quadCase=="gpu_quad_projection") {
                        // The simulator exposes asymmetric frusta. Compare rays after
                        // projection/unprojection, not the raw pixels in its two previews.
                        const XrFovf fovs[2]={{-54*rad,40*rad,43.98f*rad,-54.27f*rad},
                                              {-40*rad,54*rad,43.98f*rad,-54.27f*rad}};
                        const XrCompositionLayerQuad* layers[2]={l,r};
                        for(float offsetX:{-.3f,0.0f,.3f}) for(float offsetY:{-.1f,0.0f,.1f}) {
                            XrVector3f rays[2]{};float pixelX[2]{};
                            const float panelYaw=2*std::atan2(l->pose.orientation.y,l->pose.orientation.w);
                            for(int eye=0;eye<2;++eye) {
                                const auto& p=layers[eye]->pose.position;
                                XrVector3f v{p.x+cosf(panelYaw)*offsetX-head.position.x-eyes[eye].x,
                                             p.y+offsetY-head.position.y-eyes[eye].y,
                                             p.z-sinf(panelYaw)*offsetX-head.position.z-eyes[eye].z};
                                // Also exercise opposite view cant: the same world ray
                                // need not have the same angle in each eye's local frame.
                                const float viewYaw=yaw+(i%2 ? (eye==0 ? -.08f : .08f) : 0);
                                const float c=cosf(viewYaw),s=sinf(viewYaw);
                                const float x=c*v.x-s*v.z,z=s*v.x+c*v.z;
                                Check(z<0,"test HUD point behind view");
                                const auto& f=fovs[eye];
                                pixelX[eye]=(x/-z-tanf(f.angleLeft))/(tanf(f.angleRight)-tanf(f.angleLeft));
                                const float pixelY=(tanf(f.angleUp)-v.y/-z)/(tanf(f.angleUp)-tanf(f.angleDown));
                                const float tx=tanf(f.angleLeft)+pixelX[eye]*(tanf(f.angleRight)-tanf(f.angleLeft));
                                const float ty=tanf(f.angleUp)-pixelY*(tanf(f.angleUp)-tanf(f.angleDown));
                                XrVector3f ray{c*tx-s,ty,-s*tx-c};
                                const float norm=std::sqrt(ray.x*ray.x+ray.y*ray.y+ray.z*ray.z);
                                rays[eye]={ray.x/norm,ray.y/norm,ray.z/norm};
                            }
                            Check(std::abs(rays[0].x-rays[1].x)<2e-6f && std::abs(rays[0].y-rays[1].y)<2e-6f &&
                                  std::abs(rays[0].z-rays[1].z)<2e-6f,"projected HUD rays diverge between eyes");
                            if(i%2==0) Check(std::abs(pixelX[0]-pixelX[1]-.242512f)<1e-5f,
                                "asymmetric preview offset was incorrectly cancelled in angular HUD mode");
                        }
                    }
                }
            } else if(quadCase=="gpu_quad_body") {
                g_liveControls.xrHudFollowMode=1;bodyYaw=.4f;
                auto l=reinterpret_cast<const XrCompositionLayerQuad*>(prepare());
                Check(l && std::abs(l->pose.orientation.y-sinf(.2f))<1e-6f,"body heading not applied");
                head.orientation={0,sinf(.8f),0,cosf(.8f)};l=reinterpret_cast<const XrCompositionLayerQuad*>(prepare());
                Check(std::abs(l->pose.orientation.y-sinf(.2f))<1e-6f,"head turn overrode body mode");
                g_liveControls.xrHudFollowMode=0;
            } else if(quadCase=="gpu_quad_debug_gate") {
                Check(prepare()!=nullptr,"debug-off hid the HUD");
                Check(CyberpunkVR_HudPoseDebugSeq==0,"debug-off wrote the HUD snapshot");
                CyberpunkVR_RuntimeDiagnostics=1;
                Check(prepare()!=nullptr && CyberpunkVR_HudPoseDebugSeq==2 && CyberpunkVR_HudPoseDebug.frames==1,"debug-on snapshot missing");
                CyberpunkVR_RuntimeDiagnostics=0;
                head.orientation={0,sinf(45*rad),0,cosf(45*rad)};
                auto layer=reinterpret_cast<const XrCompositionLayerQuad*>(prepare());
                Check(layer && layer->pose.orientation.y>0,"debug-off stopped HUD follow");
                Check(CyberpunkVR_HudPoseDebugSeq==2 && CyberpunkVR_HudPoseDebug.frames==1,"HUD diagnostic snapshot kept updating");
            } else if(quadCase=="gpu_quad_delay") {
                g_liveControls.xrHudFollowDeg=60;
                Check(prepare()!=nullptr,"initial panel missing");
                head.orientation={0,sinf(10*rad),0,cosf(10*rad)};
                auto yaw=[&](bool tracked=true) {
                    auto layer=reinterpret_cast<const XrCompositionLayerQuad*>(prepare(tracked));
                    Check(layer!=nullptr,"delayed panel missing");
                    return 2*std::atan2(layer->pose.orientation.y,layer->pose.orientation.w);
                };
                yaw();
                for(int i=0;i<250;++i)Check(yaw()==0,"HUD moved before the delay");
                Check(yaw(false)==0,"tracking loss moved HUD");
                yaw();
                for(int i=0;i<250;++i)Check(yaw()==0,"tracking loss did not restart the wait");
                for(int i=0;i<40;++i)yaw();
                Check(std::abs(yaw()-20*rad)<2.1f*rad,"HUD did not catch up after rest");
            } else if(quadCase=="gpu_quad_tracking") {
                Check(!prepare(false),"untracked initial panel submitted");Check(!cvr::hud::ConsumerReady(),"native HUD masked before first valid pose");
                auto layer=reinterpret_cast<const XrCompositionLayerQuad*>(prepare());Check(layer!=nullptr,"valid panel missing");
                auto before=layer->pose;head.position={9,9,9};
                layer=reinterpret_cast<const XrCompositionLayerQuad*>(prepare(false));Check(layer && layer->pose.position.x==before.position.x,"tracking loss jumped panel");
            } else {
                Check(prepare()!=nullptr,"first panel missing");head.orientation={0,sinf(45*rad),0,cosf(45*rad)};
                auto layer=reinterpret_cast<const XrCompositionLayerQuad*>(prepare(true,2));
                Check(layer && std::abs(layer->pose.orientation.y-head.orientation.y)<0.001f,"recenter retained old HUD yaw");
                Check(layer->eyeVisibility==XR_EYE_VISIBILITY_LEFT && quad.SecondEye(),"HUD missing from one eye");
                Check(layer->layerFlags==XR_COMPOSITION_LAYER_BLEND_TEXTURE_SOURCE_ALPHA_BIT,"HUD alpha mode changed");
                Check(!quad.Prepare(reinterpret_cast<XrSession>(1),reinterpret_cast<XrSpace>(1),gpu.device.Get(),gpu.queue.Get(),false,true,head,2),"menu did not hide gameplay HUD");
                Check(!cvr::hud::ConsumerReady(),"menu did not restore native slots");
            }
            gpu.Wait();quad.Shutdown();xrtest::device=nullptr;
        }
        if(leases && pass<3) held.push_back(frame);
        if(leases && pass==3) {
            Check(frame==held.back(),"producer overwrote a held frame");
            held.erase(held.begin());
        }
        if(leases && pass==4) Check(frame!=held.back(),"released frame did not become reusable");
        gpu.Reset();
        auto readback=gpu.Buffer(512*8,D3D12_HEAP_TYPE_READBACK,D3D12_RESOURCE_STATE_COPY_DEST);
        D3D12_TEXTURE_COPY_LOCATION dst{};dst.pResource=readback.Get();dst.Type=D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;
        dst.PlacedFootprint.Footprint={DXGI_FORMAT_R8G8B8A8_UNORM,128,8,1,512};
        D3D12_TEXTURE_COPY_LOCATION src{};src.pResource=frame->canvas.Get();src.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
        gpu.cmd->CopyTextureRegion(&dst,0,0,0,&src,nullptr);gpu.Submit();
        void* data=nullptr;Hr(readback->Map(0,nullptr,&data));auto px=static_cast<unsigned char*>(data);
        for(unsigned i=0;i<64;++i) {
            float srgb=(32+i*3)/255.0f;
            float linear=srgb<=0.04045f ? srgb/12.92f : powf((srgb+0.055f)/1.055f,2.4f);
            linear*=styleCase ? 0.5f : 0.25f;float expected=linear<=0.0031308f ? 12.92f*linear : 1.055f*powf(linear,1/2.4f)-0.055f;
            for(unsigned dx=0;dx<2;++dx) {
                auto p=px+3*512+(i*2+dx)*4;
                Check(std::abs(int(p[0])-int(std::round(expected*255)))<=2,"sprite descriptor/tint/layout mismatch");
                if(!styleCase) Check(std::abs(int(p[3])-128)<=1,"premultiplied alpha mismatch");
            }
            Check(px[i*8+3]==0,"transparent canvas was not cleared");
            if(styleCase) {
                auto shadow=px+6*512+i*8;
                Check(shadow[3]>100 && shadow[0]==0,"native black shadow missing outside source tile");
            }
        }
        readback->Unmap(0,nullptr);
    }
    held.clear();
    for(size_t i=0;i<cvr::hud::ChannelCount;++i)cvr::hud::Shutdown(static_cast<cvr::hud::Channel>(i));
    testHudConstants.Reset();testFrameConstants.Reset();
}

int main(int argc,char** argv) try {
    Check(argc==2,"case required");std::string name=argv[1];cvr::hud::Follow f;
    if(name=="channels") {
        using namespace cvr::hud;
        Check(EntryChannel(EntryHash("interactions_root"))==Channel::Interaction,"interaction root stayed in main HUD");
        Check(EntryChannel(EntryHash("input_hint"))==Channel::Main,"button hints were mistaken for interaction choices");
        Check(EntryChannel(EntryHash("subtitles"))==Channel::Main,"subtitles did not use ordinary HUD");
        Check(NeedsSeparateWindow(EntryHash("Notifications_Journal")) &&
              EntryChannel(EntryHash("Notifications_Journal"))==Channel::Main,
              "new-area journal notification bypasses texture HUD");
        const int journal=ElementIndex(EntryHash("Notifications_Journal"));
        Check(journal>=0 && std::string(Elements[journal].key)=="journal_notifications",
              "journal notification layout setting missing");
        Check(NeedsSeparateWindow(EntryHash("hud tank")) && EntryChannel(EntryHash("hud tank"))==Channel::Basilisk,
              "Basilisk HUD bypasses texture HUD");
        const int tank=ElementIndex(EntryHash("hud tank"));
        Check(tank>journal && std::string(Elements[tank].key)=="basilisk","Basilisk layout missing or old indices changed");
        for (const char* entry : {"q304_dossier", "q304_imprint_active_indicator", "briefing_sequence_player"}) {
            Check(NeedsSeparateWindow(EntryHash(entry)) && EntryChannel(EntryHash(entry))==Channel::Main,
                  "identity UI bypasses the main texture HUD");
            Check(ElementIndex(EntryHash(entry))>tank,"identity layout missing or old indices changed");
        }
        Check(EntryChannel(EntryHash("briefing_sequence_player"),true)==Channel::Surveillance,
              "active sniper HUD retained the ordinary freelook cone");
        Check(EntryChannel(EntryHash("briefing_sequence_player"),false)==Channel::Main,
              "leaving the sniper changed the story briefing follow policy");
        Check(EntryChannel(EntryHash("q304_dossier"),true)==Channel::Main &&
              EntryChannel(EntryHash("subtitles"),true)==Channel::Main,
              "sniper context changed an unrelated HUD entry");
        Check(!NeedsSeparateWindow(EntryHash("sex_hud")),"unrelated custom animation was moved");
        Check(!NeedsSeparateWindow(EntryHash("briefing")),"fullscreen briefing menu confused with story overlay");
        for(const char* entry:{"camera_hud","scanner","scanner_details"}) {
            Check(NeedsSeparateWindow(EntryHash(entry)) && EntryChannel(EntryHash(entry))==Channel::Surveillance,
                  "camera/scanner content missing from the independent head-locked HUD");
            Check(ElementIndex(EntryHash(entry))>ElementIndex(EntryHash("briefing_sequence_player")),
                  "camera/scanner layout missing or changed previous indices");
        }
        Check(NeedsSeparateWindow(EntryHash("hit marker")) && EntryChannel(EntryHash("hit marker"))==Channel::HitMarker &&
              EntryChannel(EntryHash("hit marker"),true)==Channel::HitMarker,
              "hit/kill markers missing from their own head-locked window");
        Check(EntryChannel(EntryHash("crosshair"))==Channel::Main && !NeedsSeparateWindow(EntryHash("crosshair")),
              "the projected crosshair was moved with the hit markers");
        Check(NeedsSeparateWindow(EntryHash("subtitles")) && NeedsSeparateWindow(EntryHash("interactions_root")),"direct roots were not converted");
        Check(!NeedsSeparateWindow(EntryHash("crosshair")) && !NeedsSeparateWindow(EntryHash("mappins")),"world/projected widgets were converted");
        Check(!DelayedFollow(Channel::Interaction) && DelayedFollow(Channel::Main),"rest delay belongs only to main HUD");
    } else if(name=="display_clock") {
        cvr::hud::DisplayClock clock;clock.Step(1000000000);
        for(int i=1;i<=500;++i) Check(std::abs(clock.Step(1000000000ll+i*11111111ll)-.011111111f)<1e-8f,"display clock quantized or skipped a step");
        Check(clock.Step(1000000000)==0,"backward time advanced HUD");clock.Reset();Check(clock.Step(9000000000)==0,"recenter retained old clock");
        Check(clock.Step(9100000000)==.05f && std::abs(clock.elapsed-.1f)<1e-6f,"ease clamp changed elapsed rest time");
    } else if(name=="vertical_head") {
        const float half=std::sqrt(.5f);
        Check(cvr::hud::HeadYaw(half,0,0,half,.7f)==.7f,"vertical look changed HUD heading");
        Check(std::abs(cvr::hud::HeadYaw(0,sinf(.3f),0,cosf(.3f),0)-.6f)<1e-6f,"normal head heading lost");
    } else if(name=="layout_latch") {
        cvr::hud::LayoutLatch latch;auto a=latch.Update({2000,100,400,300},false);
        auto b=latch.Update({1700,100,400,300},true);Check(b.x==a.x,"partial parent layout moved minimap");
        auto c=latch.Update({2100,100,400,300},false);Check(c.x==2100,"settled layout was not accepted");
    } else if(name=="layout_transform") {
        cvr::hud::ElementSettings e;e.x=10;e.y=-5;e.scale=.5f;
        auto rect=cvr::hud::ApplyElement({100,200,400,300},e,2000,1000);
        Check(rect.x==400 && rect.y==225 && rect.width==200 && rect.height==150,"element centre/normalized offsets wrong");
    } else if(name=="layout_keep_inside") {
        // Defaults leave the authored rect untouched, even one the game parks partly off-canvas.
        cvr::hud::ElementSettings d;
        auto same=cvr::hud::ApplyElement({1700,800,400,300},d,2000,1000);
        Check(same.x==1700 && same.y==800 && same.width==400 && same.height==300,"default changed an authored rect");
        // A corner element scaled 2x slides back inside instead of being cut at the canvas edge.
        cvr::hud::ElementSettings e;e.scale=2;
        auto big=cvr::hud::ApplyElement({1600,700,400,300},e,2000,1000);
        Check(big.width==800 && big.height==600,"scale changed while sliding inside");
        Check(big.x==1200 && big.y==400,"scaled corner element not kept on the canvas");
        // An offset that would leave the canvas stops at its edge.
        cvr::hud::ElementSettings o;o.x=-100;o.y=100;
        auto moved=cvr::hud::ApplyElement({100,100,200,100},o,2000,1000);
        Check(moved.x==0 && moved.y==900,"offset not clamped to the canvas");
        // An element scaled larger than the canvas shrinks to fit, keeping its aspect.
        cvr::hud::ElementSettings huge;huge.scale=3;
        auto fit=cvr::hud::ApplyElement({0,0,1000,600},huge,2000,1000);
        Check(std::abs(fit.height-1000.f)<0.01f && std::abs(fit.width-1000.f/0.6f)<0.01f && fit.x>=0 && fit.x+fit.width<=2000.01f,"oversized element not fitted");
    } else if(name=="scanner_crop") {
        for(float rootWidth:{1280.f,2648.f,2856.f,3616.f}) {
            const float scale=rootWidth/3840.f;
            const float textureHeight=std::floor(4000.f*scale);
            // A controller resets the crop after a larger offscreen texture exists.
            const cvr::hud::Rect reset{0,-32,3840,2224};
            auto crop=cvr::hud::KeepCropCentered(reset,rootWidth,textureHeight,scale);
            Check(std::abs((1080-crop.y)*scale-textureHeight*.5f)<.002f,"scanner centre moved inside retained texture");
            Check(std::abs((1920-crop.x)*scale-rootWidth*.5f)<.002f,"scanner horizontal centre moved");
            for(int i=0;i<10000;++i) {
                auto next=cvr::hud::KeepCropCentered(reset,rootWidth,textureHeight,scale);
                Check(next.x==crop.x && next.y==crop.y && next.width==crop.width && next.height==crop.height,"reset crop accumulated drift");
                auto repeated=cvr::hud::KeepCropCentered(crop,rootWidth,textureHeight,scale);
                Check(repeated.width==crop.width && repeated.height==crop.height,"crop kept growing");
                Check(crop.height*scale<=textureHeight+.001f,"rounding requests a larger allocation");
            }
            const cvr::hud::Rect details{1920,1080,3840,2160};
            const auto fixed=cvr::hud::KeepCropCentered(details,rootWidth,textureHeight,scale);
            Check(std::abs((2160-fixed.y)*scale-textureHeight*.5f)<.002f,"details panel centre moved");
            const auto missing=cvr::hud::KeepCropCentered(reset,0,0,scale);
            Check(missing.y==reset.y && missing.height==reset.height,"missing texture changed crop");
            const auto invalid=cvr::hud::KeepCropCentered(reset,rootWidth,textureHeight,0);
            Check(invalid.y==reset.y && invalid.height==reset.height,"unknown HUD scale changed crop");
        }
    } else if(name=="layout_config") {
        cvr::hud::LayoutSettings layout{};
        Check(cvr::hud::ParseLayoutSetting("hud_element_minimap=12,-8,0.75,0.4,1",layout),"setting not parsed");
        int index=cvr::hud::ElementIndex(cvr::hud::NameHash("minimap"));
        Check(layout[index].x==12 && layout[index].scale==.75f,"wrong element edited");
        Check(!cvr::hud::ParseLayoutSetting("hud_element_minimap_other=1,2,3,4,1",layout),"prefix matched wrong key");
        FILE* file=std::tmpfile();Check(file!=nullptr,"temp file");cvr::hud::SaveLayoutSettings(file,layout);rewind(file);
        cvr::hud::LayoutSettings restored{};char line[160];while(std::fgets(line,sizeof(line),file))cvr::hud::ParseLayoutSetting(line,restored);fclose(file);
        Check(restored[index].x==12 && restored[index].opacity==.4f,"layout save/reload lost values");
    } else if(name=="gpu_style") { GpuTest(false,name); }
    else if(name.rfind("gpu_quad_",0)==0) { GpuTest(false,name); }
    else if(name=="gpu_sprites" || name=="gpu_leases") { GpuTest(name=="gpu_leases"); }
    else if(name=="delayed_catchup") {
        f.Update(0,60*rad,.05f,true);f.Update(20*rad,60*rad,.05f,true);
        for(int i=0;i<59;++i)Check(f.Update(20*rad,60*rad,.05f,true)==0,"panel moved before three seconds");
        float y=f.Update(20*rad,60*rad,.05f,true);
        Check(y>0 && y<=.15001f,"delayed catch-up missing or snapped");
        for(int i=0;i<20;++i)f.Update(20*rad,60*rad,.01f,true);
        Check(!f.following && std::abs(f.yaw-20*rad)<2.1f*rad,"delayed catch-up did not settle");
        y=f.yaw;for(int i=0;i<400;++i)Check(f.Update(y+5*rad,60*rad,.01f,true)==y,"settled panel lost its free zone");
    } else if(name=="delayed_jitter") {
        f.Update(0,60*rad,.01f,true);f.Update(20*rad,60*rad,.01f,true);
        for(int i=0;i<299;++i)Check(f.Update((20+.15f*std::sin(float(i)))*rad,60*rad,.01f,true)==0,"jitter started follow early");
        for(int i=0;i<10;++i)f.Update((20+.15f*std::sin(float(i)))*rad,60*rad,.01f,true);
        Check(f.yaw>0,"headset jitter prevented delayed follow");
    } else if(name=="delayed_turn") {
        f.Update(0,60*rad,.01f,true);f.Update(20*rad,60*rad,.01f,true);
        for(int i=0;i<250;++i)f.Update(20*rad,60*rad,.01f,true);
        for(int i=0;i<200;++i)Check(f.Update((20+i*.1f)*rad,60*rad,.02f,true)==0,"continuing head turn did not restart the wait");
        f.Update(45*rad,60*rad,.01f,true);
        for(int i=0;i<290;++i)Check(f.Update(45*rad,60*rad,.01f,true)==0,"turn retained stale rest time");
        for(int i=0;i<20;++i)f.Update(45*rad,60*rad,.01f,true);
        Check(f.yaw>0,"new rest heading never followed");
    } else if(name=="delayed_limits") {
        for(float head:{0.0f,5.0f,10.0f,30.0f}) {
            f.Reset();f.Update(0,30*rad,.01f,true);
            for(int i=0;i<400;++i)Check(f.Update(head*rad,30*rad,.01f,true)==0,"delayed follow escaped the strict angle range");
        }
        Check(f.Update(31*rad,30*rad,.01f,true)>0,"outside-cone follow was delayed");
        f.Reset();f.Update(0,10*rad,.01f,true);
        for(int i=0;i<400;++i)Check(f.Update(9*rad,10*rad,.01f,true)==0,"small cone enabled delayed follow");
        Check(f.Update(11*rad,10*rad,.01f,true)>0,"small cone lost immediate catch-up");
    } else if(name=="delayed_reset") {
        const float nan=std::numeric_limits<float>::quiet_NaN();
        for(int reset=0;reset<5;++reset) {
            f.Reset();f.Update(0,60*rad,.01f,true);f.Update(20*rad,60*rad,.01f,true);
            for(int i=0;i<250;++i)f.Update(20*rad,60*rad,.01f,true);
            if(reset==0) f.ResetDelay(); // tracking lost without losing the anchor
            else if(reset==1) f.Update(nan,60*rad,.01f,true);
            else if(reset==2) f.Update(20*rad,60*rad,nan,true);
            else if(reset==3) f.Update(20*rad,60*rad,10,true); // long pause
            else f.Update(5*rad,60*rad,.01f,true); // return to the central dead zone
            for(int i=0;i<290;++i)Check(f.Update(20*rad,60*rad,.01f,true)==0,"reset retained delayed follow time");
            for(int i=0;i<30;++i)f.Update(20*rad,60*rad,.01f,true);
            Check(f.yaw>0,"reset prevented follow from resuming");
        }
        f.Reset();Check(f.Update(-1,60*rad,.01f,true)==-1 && !f.delayPending,"recenter retained delayed state");
    } else if(name=="delayed_wrap") {
        f.Update(179*rad,60*rad,.05f,true);f.Update(-151*rad,60*rad,.05f,true);
        for(int i=0;i<59;++i)Check(f.Update(-151*rad,60*rad,.05f,true)==179*rad,"wrapped yaw moved too early");
        const float before=f.yaw,after=f.Update(-151*rad,60*rad,.05f,true);
        Check(cvr::hud::Follow::Wrap(after-before)>0 && std::abs(cvr::hud::Follow::Wrap(after-before))<=.15001f,"wrapped catch-up took the long route");
    } else if(name=="delayed_slow_frames") {
        f.Update(0,60*rad,.1f,true);f.Update(20*rad,60*rad,.1f,true);
        for(int i=0;i<29;++i)Check(f.Update(20*rad,60*rad,.1f,true)==0,"slow frames followed too early");
        const float y=f.Update(20*rad,60*rad,.1f,true);
        Check(y>0 && y<=.15001f,"movement clamp lengthened the three-second wait");
    }
    else if(name=="cone") { f.Update(0,30*rad,.01f);for(int i=0;i<1000;++i)Check(f.Update(29*rad,30*rad,.01f)==0,"free zone moved"); }
    else if(name=="catchup") { f.Update(0,30*rad,.01f);for(int i=0;i<100;++i)f.Update(60*rad,30*rad,.01f);Check(!f.following && std::abs(f.yaw-60*rad)<2.1f*rad,"did not settle");float y=f.yaw;f.Update(y+20*rad,30*rad,.01f);Check(f.yaw==y,"free zone not restored"); }
    else if(name=="wrap") { f.Update(179*rad,30*rad,.01f);Check(f.Update(-179*rad,30*rad,.01f)==179*rad,"yaw wrap crossed long path"); }
    else if(name=="reversal") { f.Update(0,5*rad,.01f);f.Update(80*rad,5*rad,.01f);float y=f.yaw;f.Update(-80*rad,5*rad,.01f);Check(f.yaw<y && std::abs(f.yaw-y)<=.031f,"reverse turn overshot"); }
    else if(name=="invalid") { f.Update(1,30*rad,.01f);Check(f.Update(std::numeric_limits<float>::quiet_NaN(),30*rad,.01f)==1,"tracking gap corrupted yaw");f.Reset();Check(f.Update(-2,30*rad,.01f)==-2,"recenter retained old anchor"); }
    else if(name=="delta_limit") { f.Update(0,5*rad,.01f);Check(std::abs(f.Update(2,5*rad,20))<=.15001f,"pause caused teleport");float y=f.yaw;Check(f.Update(2,5*rad,-1)==y,"negative delta moved panel"); }
    else Check(false,"unknown case");
    std::cout<<"PASS "<<name<<'\n';return 0;
} catch(const std::exception& error) { std::cerr<<error.what()<<'\n';return 1; }
