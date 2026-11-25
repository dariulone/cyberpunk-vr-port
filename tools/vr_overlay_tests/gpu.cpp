#include "Overlay/VrPanel.hpp"
#include "Runtimes/CyberFingerMenuLaser.hpp"
#include "Runtimes/CyberFingerGameMenu.hpp"
#include "Overlay/VrDraw.hpp"
#include "Overlay/OverlayInternal.hpp"
#include "Camera/ImagePoseIdentity.hpp"
#include <imgui_impl_dx12.h>
#include <dxgi1_6.h>
#include <d3d12sdklayers.h>
#include <cstdio>
#include <stdexcept>
#include <iostream>
#include <algorithm>
using Microsoft::WRL::ComPtr;
using namespace cvr::vrui;
void Log(const char*,...){}
#include "Overlay/VrWidgets.hpp"
namespace overlay {
bool DrawLiveControls(LiveControlsUiState&,int section,bool* editing){if(editing)*editing=false;if(section==3)widgets::DrawBindings();return false;}
bool DrawFpsOverlayControls(LiveControlsUiState&){return false;}
}
namespace cvr::camera {std::recursive_mutex& ImageSubmissionMutex(){static std::recursive_mutex m;return m;}}
void Check(bool v,const char* m){if(!v)throw std::runtime_error(m);}
void Hr(HRESULT r){Check(SUCCEEDED(r),"D3D12 call failed");}
void TestModUi(ID3D12Device*,ID3D12CommandQueue*);
namespace mock {
ID3D12Device* device;ComPtr<ID3D12Resource> texture;bool timeout=false,acquired=false;unsigned acquisitions=0,releases=0;
std::vector<int64_t> formats{DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,DXGI_FORMAT_R8G8B8A8_UNORM};
int64_t createdFormat{};
}
namespace cvr::cfmenu { Snapshot testPointer;Snapshot Current(){return testPointer;} }
extern "C" {
XrResult XRAPI_CALL xrEnumerateSwapchainFormats(XrSession,uint32_t cap,uint32_t* count,int64_t* out){
    *count=uint32_t(mock::formats.size());if(cap && cap<*count)return XR_ERROR_SIZE_INSUFFICIENT;
    if(cap)std::copy(mock::formats.begin(),mock::formats.end(),out);return XR_SUCCESS;
}
XrResult XRAPI_CALL xrCreateSwapchain(XrSession,const XrSwapchainCreateInfo* ci,XrSwapchain* out){
    if(std::find(mock::formats.begin(),mock::formats.end(),ci->format)==mock::formats.end())return XR_ERROR_SWAPCHAIN_FORMAT_UNSUPPORTED;
    mock::createdFormat=ci->format;
    D3D12_HEAP_PROPERTIES h{};h.Type=D3D12_HEAP_TYPE_DEFAULT;D3D12_RESOURCE_DESC d{};d.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D;d.Width=ci->width;d.Height=ci->height;d.Format=DXGI_FORMAT_R8G8B8A8_TYPELESS;
    d.DepthOrArraySize=1;d.MipLevels=1;d.SampleDesc.Count=1;d.Flags=D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
    Hr(mock::device->CreateCommittedResource(&h,D3D12_HEAP_FLAG_NONE,&d,D3D12_RESOURCE_STATE_RENDER_TARGET,nullptr,IID_PPV_ARGS(&mock::texture)));*out=reinterpret_cast<XrSwapchain>(1);return XR_SUCCESS;
}
XrResult XRAPI_CALL xrEnumerateSwapchainImages(XrSwapchain,uint32_t cap,uint32_t* count,XrSwapchainImageBaseHeader* out){*count=1;if(cap)reinterpret_cast<XrSwapchainImageD3D12KHR*>(out)[0].texture=mock::texture.Get();return XR_SUCCESS;}
XrResult XRAPI_CALL xrAcquireSwapchainImage(XrSwapchain,const XrSwapchainImageAcquireInfo*,uint32_t* out){Check(!mock::acquired,"duplicate XR acquisition");mock::acquired=true;++mock::acquisitions;*out=0;return XR_SUCCESS;}
XrResult XRAPI_CALL xrWaitSwapchainImage(XrSwapchain,const XrSwapchainImageWaitInfo*){return mock::timeout?XR_TIMEOUT_EXPIRED:XR_SUCCESS;}
XrResult XRAPI_CALL xrReleaseSwapchainImage(XrSwapchain,const XrSwapchainImageReleaseInfo*){Check(mock::acquired && !mock::timeout,"unwaited XR image released");mock::acquired=false;++mock::releases;return XR_SUCCESS;}
XrResult XRAPI_CALL xrDestroySwapchain(XrSwapchain){Check(!mock::acquired,"acquired XR image destroyed");mock::texture.Reset();return XR_SUCCESS;}
}
int main()try{
    ComPtr<ID3D12Debug> debug;if(SUCCEEDED(D3D12GetDebugInterface(IID_PPV_ARGS(&debug))))debug->EnableDebugLayer();
    ComPtr<IDXGIFactory4> factory;Hr(CreateDXGIFactory1(IID_PPV_ARGS(&factory)));ComPtr<IDXGIAdapter> warp;Hr(factory->EnumWarpAdapter(IID_PPV_ARGS(&warp)));
    ComPtr<ID3D12Device> device;Hr(D3D12CreateDevice(warp.Get(),D3D_FEATURE_LEVEL_11_0,IID_PPV_ARGS(&device)));mock::device=device.Get();
    ComPtr<ID3D12CommandQueue> queue;D3D12_COMMAND_QUEUE_DESC qd{};Hr(device->CreateCommandQueue(&qd,IID_PPV_ARGS(&queue)));
    ComPtr<ID3D12CommandAllocator> alloc;Hr(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,IID_PPV_ARGS(&alloc)));
    ComPtr<ID3D12GraphicsCommandList> list;Hr(device->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_DIRECT,alloc.Get(),nullptr,IID_PPV_ARGS(&list)));Hr(list->Close());
    ComPtr<ID3D12Fence> fence;Hr(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&fence)));uint64_t value=0;
    auto wait=[&]{Hr(queue->Signal(fence.Get(),++value));HANDLE e=CreateEventW(nullptr,FALSE,FALSE,nullptr);Hr(fence->SetEventOnCompletion(value,e));Check(WaitForSingleObject(e,5000)==WAIT_OBJECT_0,"GPU timeout");CloseHandle(e);};
    ComPtr<ID3D12DescriptorHeap> srv;D3D12_DESCRIPTOR_HEAP_DESC hd{};hd.Type=D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV;hd.NumDescriptors=1;hd.Flags=D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE;Hr(device->CreateDescriptorHeap(&hd,IID_PPV_ARGS(&srv)));
    ImGui::CreateContext();auto& io=ImGui::GetIO();io.IniFilename=nullptr;io.DisplaySize={CanvasWidth,CanvasHeight};io.DeltaTime=1.0f/90;
    io.Fonts->AddFontFromFileTTF("C:/Windows/Fonts/segoeui.ttf",28,nullptr,io.Fonts->GetGlyphRangesCyrillic());overlay::ApplyVrStyle();
    Check(ImGui_ImplDX12_Init(device.Get(),3,DXGI_FORMAT_R8G8B8A8_UNORM,srv.Get(),srv->GetCPUDescriptorHandleForHeapStart(),srv->GetGPUDescriptorHandleForHeapStart()),"ImGui init");
    Toggle();PollCommand();BridgeUpdate(true,true,"CONTINUE\nSAVE GAME\nLOAD GAME\nSETTINGS\nCREDITS\nMAIN MENU\nQUIT GAME");
    Tracking tracking;tracking.valid=true;tracking.origin=1;tracking.time=1000000000;tracking.head.position.y=1.7f;
    for(auto& hand:tracking.hands){hand.valid=true;hand.aim.position={0,1.7f,-.3f};}UpdateTracking(tracking);
    LiveControlsUiState state{};state.overlay=GetSettings();
    auto render=[&](const char* path,bool desktop=false){
        auto canvas=AcquireCanvas(device.Get(),DXGI_FORMAT_R8G8B8A8_UNORM);Check(bool(canvas),"canvas pool exhausted");
        ImGui_ImplDX12_NewFrame();ImGui::NewFrame();bool editing=false;overlay::DrawVrShell(state,editing);ImGui::Render();
        Hr(alloc->Reset());Hr(list->Reset(alloc.Get(),nullptr));D3D12_RESOURCE_BARRIER b{};b.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        b.Transition={canvas->texture.Get(),0,D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,D3D12_RESOURCE_STATE_RENDER_TARGET};list->ResourceBarrier(1,&b);
        auto rtv=canvas->rtv->GetCPUDescriptorHandleForHeapStart();float clear[4]{};list->ClearRenderTargetView(rtv,clear,0,nullptr);list->OMSetRenderTargets(1,&rtv,FALSE,nullptr);
        ID3D12DescriptorHeap* heaps[]={srv.Get()};list->SetDescriptorHeaps(1,heaps);
        auto* draw=ImGui::GetDrawData();const auto original=draw->CmdLists[0]->VtxBuffer[0].pos;
        if(desktop){DesktopDraw mirror(*draw,CanvasWidth,CanvasHeight);ImGui_ImplDX12_RenderDrawData(draw,list.Get());}
        else ImGui_ImplDX12_RenderDrawData(draw,list.Get());
        Check(std::abs(draw->CmdLists[0]->VtxBuffer[0].pos.x-original.x)<.001f && std::abs(draw->CmdLists[0]->VtxBuffer[0].pos.y-original.y)<.001f,"desktop transform corrupted XR draw data");
        std::swap(b.Transition.StateBefore,b.Transition.StateAfter);list->ResourceBarrier(1,&b);Hr(list->Close());ID3D12CommandList* commands[]={list.Get()};queue->ExecuteCommandLists(1,commands);wait();PublishCanvas(canvas,fence.Get(),value);
        if(path){
            constexpr unsigned pitch=(CanvasWidth*4+255)&~255u;
            ComPtr<ID3D12Resource> readback;D3D12_HEAP_PROPERTIES hp{};hp.Type=D3D12_HEAP_TYPE_READBACK;D3D12_RESOURCE_DESC d{};d.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;d.Width=uint64_t(pitch)*CanvasHeight;d.Height=1;d.DepthOrArraySize=1;d.MipLevels=1;d.SampleDesc.Count=1;d.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
            Hr(device->CreateCommittedResource(&hp,D3D12_HEAP_FLAG_NONE,&d,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&readback)));
            Hr(alloc->Reset());Hr(list->Reset(alloc.Get(),nullptr));b.Transition.StateBefore=D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;b.Transition.StateAfter=D3D12_RESOURCE_STATE_COPY_SOURCE;list->ResourceBarrier(1,&b);
            D3D12_TEXTURE_COPY_LOCATION src{},dst{};src.pResource=canvas->texture.Get();src.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;dst.pResource=readback.Get();dst.Type=D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;
            dst.PlacedFootprint.Footprint={DXGI_FORMAT_R8G8B8A8_UNORM,CanvasWidth,CanvasHeight,1,pitch};list->CopyTextureRegion(&dst,0,0,0,&src,nullptr);std::swap(b.Transition.StateBefore,b.Transition.StateAfter);list->ResourceBarrier(1,&b);Hr(list->Close());queue->ExecuteCommandLists(1,commands);wait();
            void* pixels=nullptr;Hr(readback->Map(0,nullptr,&pixels));FILE* f=nullptr;fopen_s(&f,path,"wb");Check(f!=nullptr,"image output");std::fprintf(f,"P6\n%u %u\n255\n",CanvasWidth,CanvasHeight);
            auto* p=static_cast<unsigned char*>(pixels);for(unsigned y=0;y<CanvasHeight;++y)for(unsigned x=0;x<CanvasWidth;++x)std::fwrite(p+size_t(y)*pitch+x*4,1,3,f);std::fclose(f);readback->Unmap(0,nullptr);
        }
    };
    render("vr-menu.ppm");Panel panel;auto prep=[&]{return panel.Prepare(reinterpret_cast<XrSession>(1),reinterpret_cast<XrSpace>(1),device.Get(),queue.Get());};
    mock::timeout=true;Check(!prep() && mock::acquisitions==1 && mock::releases==0,"XR timeout ownership");mock::timeout=false;
    auto* layer=reinterpret_cast<const XrCompositionLayerQuad*>(prep());Check(layer && layer->eyeVisibility==XR_EYE_VISIBILITY_BOTH && panel.Ray(),"shared panel/ray missing");wait();
    // Click the Overlay navigation button through ordinary ImGui mouse events.
    io.AddMousePosEvent(1310,135);io.AddMouseButtonEvent(0,true);render(nullptr);io.AddMouseButtonEvent(0,false);render(nullptr);render("vr-settings.ppm");
    render("vr-desktop.ppm",true);
    io.AddMousePosEvent(810,128);io.AddMouseButtonEvent(0,true);render(nullptr);
    io.AddMouseButtonEvent(0,false);render(nullptr);render("vr-bindings.ppm");
    prep();wait();Close(false);Check(!prep() && !mock::texture,"closed panel retained XR resources");
    ImGui_ImplDX12_Shutdown();ImGui::DestroyContext();wait();
    cvr::cfmenu::Laser laser;
    auto beam=[&]{return laser.Prepare(reinterpret_cast<XrSession>(1),reinterpret_cast<XrSpace>(1),device.Get(),queue.Get());};
    Check(!beam() && !mock::texture,"inactive CF allocated a laser");
    auto& pointer=cvr::cfmenu::testPointer;
    pointer.active=pointer.hit=pointer.ray=true;pointer.start={.2f,1,-.2f};pointer.end={0,1.4f,-2};pointer.head={0,1.7f,0};
    // A runtime can advertise sRGB without the linear UNORM variant. The
    // native beam must work under the same format requirements as F10/CET.
    mock::formats={DXGI_FORMAT_R8G8B8A8_UNORM_SRGB};
    const auto acquisitions=mock::acquisitions,releases=mock::releases;
    mock::timeout=true;Check(!beam() && mock::acquisitions==acquisitions+1 && mock::releases==releases,"laser lost XR image on wait timeout");
    mock::timeout=false;
    const auto* ray=reinterpret_cast<const XrCompositionLayerQuad*>(beam());wait();
    Check(ray && ray->eyeVisibility==XR_EYE_VISIBILITY_BOTH && ray->size.height>1,"CF menu laser geometry missing");
    Check(mock::createdFormat==DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,"sRGB-only runtime lost native laser");
    const auto tip=Add(ray->pose.position,RotateVector(ray->pose.orientation,{0,ray->size.height*.5f,0}));
    Check(Dot(Sub(tip,pointer.end),Sub(tip,pointer.end))<1e-6f,"laser endpoint differs from native cursor target");
    Check(mock::texture->GetDesc().Width==4 && mock::releases==releases+1,"laser static image not released");
    Check(beam() && mock::acquisitions==acquisitions+1,"static laser image unnecessarily reacquired");
    pointer.hit=false;Check(beam(),"off-canvas aiming ray disappeared");
    pointer.ray=false;Check(!beam(),"lost tracking or yielded pointer still submitted a ray");
    laser.Shutdown();Check(!mock::texture,"laser swapchain not released");
    pointer.ray=true;mock::formats={DXGI_FORMAT_R8G8B8A8_UNORM};
    Check(beam() && mock::createdFormat==DXGI_FORMAT_R8G8B8A8_UNORM,"linear-only runtime lost native laser fallback");
    wait();laser.Shutdown();mock::formats={DXGI_FORMAT_B8G8R8A8_UNORM_SRGB};
    Check(!beam() && !mock::texture,"unsupported format allocated a native laser");
    mock::formats={DXGI_FORMAT_R8G8B8A8_UNORM,DXGI_FORMAT_R8G8B8A8_UNORM_SRGB};
    Check(beam() && mock::createdFormat==DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,"native laser did not prefer panel-compatible sRGB");
    wait();laser.Shutdown();
    TestModUi(device.Get(),queue.Get());
    ComPtr<ID3D12InfoQueue> errors;if(SUCCEEDED(device.As(&errors)))for(UINT64 i=0;i<errors->GetNumStoredMessages();++i){SIZE_T size=0;errors->GetMessage(i,nullptr,&size);std::vector<unsigned char> storage(size);auto* m=reinterpret_cast<D3D12_MESSAGE*>(storage.data());errors->GetMessage(i,m,&size);if(m->Severity<=D3D12_MESSAGE_SEVERITY_ERROR){std::cerr<<m->pDescription<<'\n';Check(false,"D3D12 validation error");}}
    std::cout<<"PASS GPU panel, timeout, BOTH eyes, ray, cleanup and UI render\n";return 0;
}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}
