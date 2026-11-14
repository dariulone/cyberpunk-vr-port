#include "Framegen/Presenter.hpp"
#include "Framegen/StatsOverlay.hpp"
#include "Framegen/GpuTimer.hpp"
#include "Framegen/InputCopy.hpp"
#include "Render/StereoDepth.hpp"
#include "Runtimes/DepthSwapchainImage.hpp"
#include "Utils/DebugGate.hpp"
extern "C" { int CyberpunkVR_RuntimeDiagnostics=0; }
extern "C" { extern char CyberpunkVR_FramegenReport[2048]; extern std::atomic<uint32_t> CyberpunkVR_FramegenReportSeq; }
#include <dxgi1_6.h>
#include <d3d12sdklayers.h>
#include <DirectXPackedVector.h>
#include <algorithm>
#include <array>
#include <filesystem>
#include <functional>
#include <cmath>
#include <cstdio>
#include <cstdarg>
#include <cstring>
#include <stdexcept>
#include <string>
#include <thread>
#include <vector>
#include <dbghelp.h>

using Microsoft::WRL::ComPtr;
using namespace cvr::framegen;
// The AFW launch mode is the plugin's (Render/Afw.cpp, with its plugin and OSR); these tests never start in it.
namespace cvr::afw {bool Launch() {return false;}}
void Log(const char* format,...) {va_list args;va_start(args,format);vprintf(format,args);va_end(args);fflush(stdout);}
LONG WINAPI Crash(EXCEPTION_POINTERS* exception) {
    static volatile LONG dumping=0;if(InterlockedExchange(&dumping,1))return EXCEPTION_EXECUTE_HANDLER;
    HMODULE module{};wchar_t path[MAX_PATH]{};
    GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS|GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
        reinterpret_cast<LPCWSTR>(exception->ExceptionRecord->ExceptionAddress),&module);
    GetModuleFileNameW(module,path,MAX_PATH);
    std::fwprintf(stderr,L"Exception %08X at %s+%llX\n",exception->ExceptionRecord->ExceptionCode,path,
        uint64_t(reinterpret_cast<uintptr_t>(exception->ExceptionRecord->ExceptionAddress)-reinterpret_cast<uintptr_t>(module)));
    const auto process=GetCurrentProcess();SymInitialize(process,nullptr,TRUE);
    CONTEXT context=*exception->ContextRecord;STACKFRAME64 stack{};
    stack.AddrPC={context.Rip,0,AddrModeFlat};stack.AddrStack={context.Rsp,0,AddrModeFlat};stack.AddrFrame={context.Rbp,0,AddrModeFlat};
    for(int i=0;i<24 && StackWalk64(IMAGE_FILE_MACHINE_AMD64,process,GetCurrentThread(),&stack,&context,nullptr,SymFunctionTableAccess64,SymGetModuleBase64,nullptr);++i) {
        const auto base=SymGetModuleBase64(process,stack.AddrPC.Offset);GetModuleFileNameW(reinterpret_cast<HMODULE>(base),path,MAX_PATH);
        std::fwprintf(stderr,L"  %s+%llX\n",path,stack.AddrPC.Offset-base);
    }
    auto file=CreateFileW(L"framegen-test-crash.dmp",GENERIC_WRITE,0,nullptr,CREATE_ALWAYS,FILE_ATTRIBUTE_NORMAL,nullptr);
    if(file!=INVALID_HANDLE_VALUE) {
        MINIDUMP_EXCEPTION_INFORMATION info{GetCurrentThreadId(),exception,FALSE};
        MiniDumpWriteDump(GetCurrentProcess(),GetCurrentProcessId(),file,MiniDumpNormal,&info,nullptr,nullptr);CloseHandle(file);
    }
    return EXCEPTION_EXECUTE_HANDLER;
}
namespace cvr::framegen {uint64_t InputVram(){return 0;}void ReleaseInputs(){}}
void Check(bool value,const char* text) {if(!value)throw std::runtime_error(text);}
void Hr(HRESULT result) {if(FAILED(result)) {char text[64];std::snprintf(text,sizeof(text),"D3D12 failure %08X",unsigned(result));throw std::runtime_error(text);}}
struct Gpu {
    ComPtr<ID3D12Device> device;ComPtr<ID3D12CommandQueue> queue;ComPtr<ID3D12CommandAllocator> allocator;
    ComPtr<ID3D12GraphicsCommandList> list;ComPtr<ID3D12Fence> fence;uint64_t value{};
    Gpu(bool hardware=false) {
        ComPtr<ID3D12Debug> debug;if(SUCCEEDED(D3D12GetDebugInterface(IID_PPV_ARGS(&debug))))debug->EnableDebugLayer();
        ComPtr<IDXGIFactory6> factory;Hr(CreateDXGIFactory2(0,IID_PPV_ARGS(&factory)));
        ComPtr<IDXGIAdapter1> adapter;
        if(hardware) {
            for(unsigned i=0;factory->EnumAdapterByGpuPreference(i,DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE,IID_PPV_ARGS(&adapter))==S_OK;++i) {
                DXGI_ADAPTER_DESC1 desc{};adapter->GetDesc1(&desc);if(desc.VendorId==0x10de)break;adapter.Reset();
            }
            Check(adapter.Get()!=nullptr,"NVIDIA hardware missing");
        } else Hr(factory->EnumWarpAdapter(IID_PPV_ARGS(&adapter)));
        Hr(D3D12CreateDevice(adapter.Get(),D3D_FEATURE_LEVEL_12_0,IID_PPV_ARGS(&device)));
        D3D12_COMMAND_QUEUE_DESC desc{};Hr(device->CreateCommandQueue(&desc,IID_PPV_ARGS(&queue)));
        Hr(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,IID_PPV_ARGS(&allocator)));
        Hr(device->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_DIRECT,allocator.Get(),nullptr,IID_PPV_ARGS(&list)));list->Close();
        Hr(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&fence)));
    }
    void Wait() {
        Hr(queue->Signal(fence.Get(),++value));auto event=CreateEventW(nullptr,FALSE,FALSE,nullptr);Check(event!=nullptr,"event missing");
        Hr(fence->SetEventOnCompletion(value,event));const auto result=WaitForSingleObject(event,20000);CloseHandle(event);Check(result==WAIT_OBJECT_0,"GPU test timeout");
        ComPtr<ID3D12InfoQueue> info;if(SUCCEEDED(device.As(&info))) {
            const auto count=info->GetNumStoredMessages();
            for(uint64_t i=0;i<count;++i) {
                SIZE_T length=0;info->GetMessage(i,nullptr,&length);std::vector<uint8_t> data(length);auto* message=reinterpret_cast<D3D12_MESSAGE*>(data.data());
                info->GetMessage(i,message,&length);
                if(message->Severity==D3D12_MESSAGE_SEVERITY_ERROR || message->Severity==D3D12_MESSAGE_SEVERITY_CORRUPTION) {
                    std::fprintf(stderr,"GPU validation: %s\n",message->pDescription);throw std::runtime_error("GPU validation error");
                }
            }
            info->ClearStoredMessages();
        }
    }
    void Begin() {Hr(allocator->Reset());Hr(list->Reset(allocator.Get(),nullptr));}
    void Execute() {Hr(list->Close());ID3D12CommandList* lists[]={list.Get()};queue->ExecuteCommandLists(1,lists);Wait();}
    ComPtr<ID3D12Resource> Texture(unsigned w,unsigned h,DXGI_FORMAT format,D3D12_RESOURCE_STATES state,D3D12_RESOURCE_FLAGS flags=D3D12_RESOURCE_FLAG_NONE) {
        D3D12_RESOURCE_DESC d{};d.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D;d.Width=w;d.Height=h;d.DepthOrArraySize=1;d.MipLevels=1;
        d.Format=format;d.SampleDesc.Count=1;d.Flags=flags;D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;
        ComPtr<ID3D12Resource> r;Hr(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&d,state,nullptr,IID_PPV_ARGS(&r)));return r;
    }
    ComPtr<ID3D12Resource> Buffer(uint64_t bytes,D3D12_HEAP_TYPE type,D3D12_RESOURCE_STATES state) {
        D3D12_RESOURCE_DESC d{};d.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;d.Width=bytes;d.Height=1;d.DepthOrArraySize=1;
        d.MipLevels=1;d.SampleDesc.Count=1;d.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;D3D12_HEAP_PROPERTIES heap{};heap.Type=type;
        ComPtr<ID3D12Resource> r;Hr(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&d,state,nullptr,IID_PPV_ARGS(&r)));return r;
    }
    void Upload(ID3D12Resource* target,const void* data,unsigned pixelSize,D3D12_RESOURCE_STATES after) {
        const auto desc=target->GetDesc();D3D12_PLACED_SUBRESOURCE_FOOTPRINT footprint{};UINT rows{};UINT64 rowBytes{},bytes{};
        device->GetCopyableFootprints(&desc,0,1,0,&footprint,&rows,&rowBytes,&bytes);
        auto upload=Buffer(bytes,D3D12_HEAP_TYPE_UPLOAD,D3D12_RESOURCE_STATE_GENERIC_READ);void* dst{};D3D12_RANGE none{};Hr(upload->Map(0,&none,&dst));
        for(unsigned row=0;row<rows;++row)std::memcpy(static_cast<uint8_t*>(dst)+row*footprint.Footprint.RowPitch,static_cast<const uint8_t*>(data)+row*desc.Width*pixelSize,size_t(desc.Width)*pixelSize);
        upload->Unmap(0,nullptr);Begin();D3D12_TEXTURE_COPY_LOCATION src{},out{};src.pResource=upload.Get();src.Type=D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;src.PlacedFootprint=footprint;
        out.pResource=target;out.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;list->CopyTextureRegion(&out,0,0,0,&src,nullptr);
        D3D12_RESOURCE_BARRIER b{};b.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;b.Transition={target,0,D3D12_RESOURCE_STATE_COPY_DEST,after};list->ResourceBarrier(1,&b);Execute();
    }
    std::vector<uint8_t> Read(ID3D12Resource* source,unsigned pixelSize=4) {
        const auto d=source->GetDesc();D3D12_PLACED_SUBRESOURCE_FOOTPRINT f{};UINT rows{};UINT64 rowBytes{},bytes{};
        device->GetCopyableFootprints(&d,0,1,0,&f,&rows,&rowBytes,&bytes);auto readback=Buffer(bytes,D3D12_HEAP_TYPE_READBACK,D3D12_RESOURCE_STATE_COPY_DEST);
        Begin();D3D12_TEXTURE_COPY_LOCATION src{},dst{};src.pResource=source;src.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
        dst.pResource=readback.Get();dst.Type=D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;dst.PlacedFootprint=f;list->CopyTextureRegion(&dst,0,0,0,&src,nullptr);Execute();
        void* data{};Hr(readback->Map(0,nullptr,&data));std::vector<uint8_t> result(size_t(d.Width)*d.Height*pixelSize);
        for(unsigned y=0;y<rows;++y)std::memcpy(result.data()+y*d.Width*pixelSize,static_cast<uint8_t*>(data)+y*f.Footprint.RowPitch,size_t(d.Width)*pixelSize);
        D3D12_RANGE none{};readback->Unmap(0,&none);return result;
    }
};
namespace mock {
ID3D12Device* device{};ComPtr<ID3D12Resource> texture;bool acquired{},timeout{};unsigned acquisitions{},releases{};
}
extern "C" XrResult XRAPI_CALL xrEnumerateSwapchainFormats(XrSession,uint32_t capacity,uint32_t* count,int64_t* formats) {*count=1;if(capacity)formats[0]=DXGI_FORMAT_R8G8B8A8_UNORM_SRGB;return XR_SUCCESS;}
extern "C" XrResult XRAPI_CALL xrCreateSwapchain(XrSession,const XrSwapchainCreateInfo* info,XrSwapchain* out) {
    Check((info->width==720 && info->height==452) || (info->width==420 && info->height==404) ||
          (info->width==360 && info->height==222),"unexpected overlay allocation");
    D3D12_RESOURCE_DESC d{};d.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D;d.Width=info->width;d.Height=info->height;d.DepthOrArraySize=1;
    d.MipLevels=1;d.SampleDesc.Count=1;d.Format=DXGI_FORMAT_R8G8B8A8_TYPELESS;d.Flags=D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
    D3D12_HEAP_PROPERTIES p{};p.Type=D3D12_HEAP_TYPE_DEFAULT;
    Hr(mock::device->CreateCommittedResource(&p,D3D12_HEAP_FLAG_NONE,&d,D3D12_RESOURCE_STATE_RENDER_TARGET,nullptr,IID_PPV_ARGS(&mock::texture)));
    *out=reinterpret_cast<XrSwapchain>(1);return XR_SUCCESS;
}
extern "C" XrResult XRAPI_CALL xrDestroySwapchain(XrSwapchain) {mock::texture.Reset();return XR_SUCCESS;}
extern "C" XrResult XRAPI_CALL xrEnumerateSwapchainImages(XrSwapchain,uint32_t capacity,uint32_t* count,XrSwapchainImageBaseHeader* images) {*count=1;if(capacity)reinterpret_cast<XrSwapchainImageD3D12KHR*>(images)[0].texture=mock::texture.Get();return XR_SUCCESS;}
extern "C" XrResult XRAPI_CALL xrAcquireSwapchainImage(XrSwapchain,const XrSwapchainImageAcquireInfo*,uint32_t* index) {Check(!mock::acquired,"double acquire");mock::acquired=true;++mock::acquisitions;*index=0;return XR_SUCCESS;}
extern "C" XrResult XRAPI_CALL xrWaitSwapchainImage(XrSwapchain,const XrSwapchainImageWaitInfo* info) {Check(info->timeout==0,"stats overlay blocks on the runtime");return mock::timeout?XR_TIMEOUT_EXPIRED:XR_SUCCESS;}
extern "C" XrResult XRAPI_CALL xrReleaseSwapchainImage(XrSwapchain,const XrSwapchainImageReleaseInfo*) {Check(mock::acquired,"release without acquire");mock::acquired=false;++mock::releases;return XR_SUCCESS;}

struct ReplayGpu {
    std::function<ComPtr<ID3D12Resource>(unsigned,unsigned,DXGI_FORMAT,D3D12_RESOURCE_STATES)> texture;
    std::function<void(ID3D12Resource*,const void*,unsigned,D3D12_RESOURCE_STATES)> upload;
    std::function<std::vector<uint8_t>(ID3D12Resource*)> read;
    std::function<void()> wait;
    ID3D12Device* device{};ID3D12CommandQueue* queue{};
    std::function<void(ID3D12Fence*,uint64_t*)> fence;
};
int Replay(ReplayGpu&,const std::filesystem::path&,const std::filesystem::path&);
void GpuGeneration(bool nvidia,bool engineMotion=true,bool rgbaMotion=false,bool halfMotion=false,Backend backend=Backend::FidelityFX,int flowScale=50) {
    Gpu gpu(nvidia);Generator generator;Settings settings;settings.backend=nvidia?Backend::Nvidia:Backend::FidelityFX;settings.flowScale=flowScale;
    if(NgxBackend(backend))settings.backend=backend;
    // DLSS-G and ANR FG (a DLSS-G provider) take the same NGX path and the same tolerances.
    const bool dlss=NgxBackend(backend);
    // NGX's frame generation feature has a minimum size; the moving box keeps the same pixels.
    // ANR FG's programs cover 640x360 and up (its nr_fg log).
    const unsigned width=backend==Backend::Anr?768:(dlss?512:128),height=backend==Backend::Anr?432:(dlss?384:96);
    EyePair cameras[2];
    for(auto& eye:cameras)for(auto* camera:{&eye.previous,&eye.current}) {camera->fov[0]=-.8f;camera->fov[1]=.8f;camera->fov[2]=.7f;camera->fov[3]=-.7f;}
    for(unsigned frame=0;frame<8;++frame) {
        const bool reducedMotion=halfMotion && frame<4;
        const unsigned motionWidth=reducedMotion?width/2:width,motionHeight=reducedMotion?height/2:height;
        const bool reconfigured=halfMotion && frame==4;
        settings.enabled=true;settings.overlay=frame==0 || frame>=3;SetSettings(settings);
        ComPtr<ID3D12Resource> color[2];ID3D12Resource* colors[2]{};std::shared_ptr<const Inputs> inputs[2];
        for(unsigned eye=0;eye<2;++eye) {
            auto in=std::make_shared<Inputs>();in->width=in->motionWidth=width;in->height=in->motionHeight=height;
            in->camera.nearPlane=.1f;in->camera.farPlane=100;in->camera.verticalFov=1.3f;in->camera.aspect=float(width)/height;
            in->camera.inverted=true;in->camera.cameraMotion=true;in->camera.motionScale[0]=in->camera.motionScale[1]=1;
            in->camera.right[0]=in->camera.up[1]=in->camera.forward[2]=1;
            std::vector<uint32_t> pixels(width*height,0xFF302020);std::vector<float> depth(width*height,.002f),motion(width*height*2);
            const unsigned x0=eye==0 ? 32+frame*8 : 12+frame*8;
            for(unsigned y=32;y<64;++y)for(unsigned x=x0;x<x0+24;++x) {
                const auto i=y*width+x;pixels[i]=eye==0?0xFF50DCF0:0xFFF0DC50;
                if(engineMotion) {depth[i]=.5f;motion[i*2]=frame?-8.0f/width:0;}
            }
            color[eye]=gpu.Texture(width,height,DXGI_FORMAT_R8G8B8A8_UNORM,D3D12_RESOURCE_STATE_COPY_DEST);
            in->depth=gpu.Texture(width,height,DXGI_FORMAT_R32_FLOAT,D3D12_RESOURCE_STATE_COPY_DEST);
            auto nativeMotion=gpu.Texture(motionWidth,motionHeight,rgbaMotion?DXGI_FORMAT_R16G16B16A16_FLOAT:DXGI_FORMAT_R32G32_FLOAT,D3D12_RESOURCE_STATE_COPY_DEST);
            color[eye]->SetName(L"Test_Color");in->depth->SetName(L"Test_Depth");nativeMotion->SetName(L"Test_NativeMotion");
            gpu.Upload(color[eye].Get(),pixels.data(),4,D3D12_RESOURCE_STATE_COPY_SOURCE);
            gpu.Upload(in->depth.Get(),depth.data(),4,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
            if(rgbaMotion) {
                std::vector<uint16_t> packed(width*height*4);
                for(size_t i=0;i<width*height;++i) {
                    packed[i*4]=DirectX::PackedVector::XMConvertFloatToHalf(motion[i*2]);
                    packed[i*4+1]=DirectX::PackedVector::XMConvertFloatToHalf(motion[i*2+1]);
                    packed[i*4+2]=DirectX::PackedVector::XMConvertFloatToHalf(1024.f);
                    packed[i*4+3]=DirectX::PackedVector::XMConvertFloatToHalf(-512.f);
                }
                gpu.Upload(nativeMotion.Get(),packed.data(),8,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
            } else {
                std::vector<float> resized(motionWidth*motionHeight*2);
                for(unsigned y=0;y<motionHeight;++y)for(unsigned x=0;x<motionWidth;++x) {
                    const auto source=((y*height/motionHeight)*width+x*width/motionWidth)*2;
                    resized[(y*motionWidth+x)*2]=motion[source];resized[(y*motionWidth+x)*2+1]=motion[source+1];
                }
                gpu.Upload(nativeMotion.Get(),resized.data(),8,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
            }
            gpu.Begin();
            Check(CopyInput(gpu.list.Get(),{nativeMotion.Get(),D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,1,0,0,motionWidth,motionHeight},*in),
                  "native tagged motion input was rejected");
            gpu.Execute();
            colors[eye]=color[eye].Get();inputs[eye]=in;
        }
        bool interpolated=false;
        Check(generator.Generate(gpu.device.Get(),gpu.queue.Get(),colors,inputs,cameras,settings,22.22,frame==0,frame+1,&interpolated),"generation failed");
        Check(ActiveBackend()==(settings.flowScale?settings.backend:Backend::FidelityFX),"the requested generator fell back to another one");
        Check(interpolated==(frame!=0 && !reconfigured),"priming or a changed motion resolution reported as a generated frame");gpu.Wait();
        if(!settings.overlay)Check(!MetricsEnabled() && GetStatistics().generatedFrames==0 && GetStatistics().generationMs==0,
            "generation continued collecting metrics while its overlay was off");
        if(frame>=(halfMotion?7u:6u))Check(GetStatistics().generationMs>0,"generation GPU timing did not resume after the readback ring filled");
        double centroids[2]{};
        for(unsigned eye=0;eye<2;++eye) {
            auto pixels=gpu.Read(generator.Output(eye));double xsum=0,weight=0;
            for(unsigned y=24;y<72;++y)for(unsigned x=0;x<width;++x) {
                const auto* p=&pixels[(y*width+x)*4];const double value=std::max(0,int(p[eye?2:0])-100);
                xsum+=x*value;weight+=value;
            }
            Check(weight>1000,"generated frame lost the object");centroids[eye]=xsum/weight;
            const double expected=(eye?12:32)+frame*8+11.5-(interpolated?4:0);
            const unsigned expectedLeft=(eye?12:32)+frame*8-(interpolated?4:0);
            double error=0;
            for(unsigned y=24;y<72;++y)for(unsigned x=0;x<width;++x) {
                const uint32_t truth=y>=32 && y<64 && x>=expectedLeft && x<expectedLeft+24?(eye?0xFFF0DC50u:0xFF50DCF0u):0xFF302020u;
                for(unsigned channel=0;channel<3;++channel)error+=std::abs(int(pixels[(y*width+x)*4+channel])-int((truth>>(8*channel))&255));
            }
            error/=48*width*3;
            std::printf("frame %u eye %u centre %.3f expected %.3f mean RGB error %.4f/255\n",frame,eye,centroids[eye],expected,error);
            if(interpolated && backend!=Backend::FidelityFX)Check(error<(dlss?6.0:1.5),"generated frame differs from the true midpoint");
            Check(std::abs(centroids[eye]-expected)<(engineMotion && !dlss?.75:3.1),"interpolated motion does not lie between real endpoints");
        }
        Check(std::abs(centroids[0]-centroids[1]-20)<1.5,"eye histories leaked into each other");
        generator.ConsumerFence(gpu.fence.Get(),gpu.value);
        std::weak_ptr<const Inputs> leased[2]{inputs[0],inputs[1]};
        inputs[0].reset();inputs[1].reset();
        generator.Poll();
        Check(leased[0].expired() && leased[1].expired(),"completed input leases require another generation to retire");
    }
    std::puts("Retirement begin");
    ComPtr<ID3D12Fence> held;Hr(gpu.device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&held)));
    generator.ConsumerFence(held.Get(),1);Check(!generator.Reset(),"destroyed an output while a consumer still owns it");
    Hr(held->Signal(1));Check(generator.Reset(),"idle resources did not retire");
    std::puts("Retirement done");
}
void DepthCamera(CameraData& camera,const XrFovf& fov) {
    const float l=std::tan(fov.angleLeft),r=std::tan(fov.angleRight),u=std::tan(fov.angleUp),d=std::tan(fov.angleDown);
    camera.verticalFov=2*std::atan((u-d)*.5f);camera.aspect=(r-l)/(u-d);
    camera.shift[0]=-(r+l)/(r-l);camera.shift[1]=-(u+d)/(u-d);
}
void StereoDepth() {
    using cvr::depth::Reject;
    bool waiting=false;uint32_t imageIndex{};const auto swap=reinterpret_cast<XrSwapchain>(1);
    mock::acquired=false;mock::timeout=true;mock::acquisitions=mock::releases=0;
    Check(cvr::depth::WaitImage(swap,waiting,imageIndex)==XR_TIMEOUT_EXPIRED && waiting,"depth timeout lost acquisition");
    Check(cvr::depth::WaitImage(swap,waiting,imageIndex)==XR_TIMEOUT_EXPIRED && mock::acquisitions==1 && mock::releases==0,"timed-out image reacquired or released");
    mock::timeout=false;
    Check(cvr::depth::WaitImage(swap,waiting,imageIndex)==XR_SUCCESS && !waiting && mock::acquisitions==1,"pending wait did not finish");
    XrSwapchainImageReleaseInfo release{XR_TYPE_SWAPCHAIN_IMAGE_RELEASE_INFO};xrReleaseSwapchainImage(swap,&release);
    Gpu gpu;cvr::depth::Renderer renderer;
    constexpr uint32_t w=13,h=9;
    std::shared_ptr<Inputs> in[2];ComPtr<ID3D12Resource> dst[2];
    Selection selected;auto& frame=selected.frame;
    frame.identity={7,4,103,{11,22},100,true,true,false};
    XrCompositionLayerProjectionView views[2]{};
    for(unsigned e=0;e<2;++e) {
        frame.pose[e]={{0,0,0,1},{e?.032f:-.032f,1.7f,0}};
        frame.fov[e]=e?XrFovf{-.7f,.9f,.8f,-.95f}:XrFovf{-.9f,.7f,.8f,-.95f};
        views[e].pose=frame.pose[e];views[e].fov=frame.fov[e];
        in[e]=std::make_shared<Inputs>();auto& i=*in[e];
        i.view=e+1;i.frameId=103;i.poseId=frame.identity.pose[e];i.origin=4;
        i.hasDepth=i.submitted=true;i.width=w;i.height=h;i.queue=gpu.queue.Get();
        i.camera.nearPlane=.1f;i.camera.farPlane=100;i.camera.inverted=true;DepthCamera(i.camera,frame.fov[e]);
        i.camera.jitter[0]=e?-.25f:.25f;i.camera.jitter[1]=e?.25f:-.25f;
        i.depth=gpu.Texture(w,h,DXGI_FORMAT_R32_FLOAT,D3D12_RESOURCE_STATE_COPY_DEST);
        std::vector<float> pixels(w*h);
        for(uint32_t y=0;y<h;++y)for(uint32_t x=0;x<w;++x)pixels[y*w+x]=(e?.8f:.4f)+.001f*x+.002f*y;
        gpu.Upload(i.depth.Get(),pixels.data(),4,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
        i.readyFence=gpu.fence;i.fenceValue=gpu.value;
        dst[e]=gpu.Texture(w,h,DXGI_FORMAT_D32_FLOAT,D3D12_RESOURCE_STATE_DEPTH_WRITE,D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL);
        frame.inputs[e]=in[e];
    }
    cvr::depth::Plan plan;
    auto describe=[&]{return cvr::depth::Describe(selected,views,2,1,plan);};
    Check(describe()==Reject::None,"asymmetric per-eye depth rejected");
    Check(plan.eyes[0].nearZ==100 && plan.eyes[0].farZ==.1f,"reverse-Z planes");
    Check(cvr::depth::Describe(selected,views,2,2,plan)==Reject::None && plan.eyes[0].nearZ==50 && plan.eyes[0].farZ==.05f,"world scale not converted to XR metres");
    selected.synthetic=true;Check(describe()==Reject::Missing,"midpoint depth without B's motion vectors");selected.synthetic=false;
    ++in[0]->frameId;Check(describe()==Reject::Identity,"stale native frame accepted");--in[0]->frameId;
    ++in[0]->poseId;Check(describe()==Reject::Identity,"wrong camera pose accepted");--in[0]->poseId;
    ++in[0]->origin;Check(describe()==Reject::Identity,"recenter crossed depth identity");--in[0]->origin;
    std::swap(frame.inputs[0],frame.inputs[1]);Check(describe()==Reject::Identity,"swapped depth eyes accepted");std::swap(frame.inputs[0],frame.inputs[1]);
    const auto fov=views[0].fov;views[0].fov.angleLeft=-.8f;views[0].fov.angleRight=.8f;
    Check(describe()==Reject::Projection,"centred substitute for asymmetric depth accepted");views[0].fov=fov;
    views[0].pose.position.x+=.01f;Check(describe()==Reject::Identity,"relabelled color pose accepted");views[0].pose=frame.pose[0];
    in[0]->camera.inverted=false;Check(describe()==Reject::None && plan.eyes[0].nearZ==.1f && plan.eyes[0].farZ==100,"ordinary depth planes");in[0]->camera.inverted=true;
    Check(describe()==Reject::None,"restored depth rejected");
    ID3D12Resource* targets[]={dst[0].Get(),dst[1].Get()};
    ComPtr<ID3D12CommandQueue> other;D3D12_COMMAND_QUEUE_DESC q{};Hr(gpu.device->CreateCommandQueue(&q,IID_PPV_ARGS(&other)));
    gpu.Begin();
    // Another queue reads a capture only once the producer's queue has finished it (the runtime's own queue).
    in[0]->fenceValue=gpu.fence->GetCompletedValue()+1000;
    Check(!renderer.Record(gpu.device.Get(),gpu.list.Get(),other.Get(),frame.inputs,targets,plan,0,gpu.fence.Get(),gpu.value+1),"unfinished capture read from another queue");
    in[0]->fenceValue=gpu.value;
    Check(renderer.Record(gpu.device.Get(),gpu.list.Get(),gpu.queue.Get(),frame.inputs,targets,plan,0,gpu.fence.Get(),gpu.value+1),"depth resolve failed");
    Check(!renderer.Record(gpu.device.Get(),gpu.list.Get(),gpu.queue.Get(),frame.inputs,targets,plan,0,gpu.fence.Get(),gpu.value+1),"in-flight descriptors overwritten");
    std::weak_ptr<const Inputs> weak=frame.inputs[0];
    frame.inputs[0].reset();frame.inputs[1].reset();in[0].reset();in[1].reset();
    renderer.Poll();Check(!weak.expired(),"depth inputs retired before submit completion");
    gpu.Execute();renderer.Poll();Check(weak.expired(),"completed depth inputs retained");
    gpu.Begin();
    for(auto& d:dst){D3D12_RESOURCE_BARRIER b{};b.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;b.Transition={d.Get(),0,D3D12_RESOURCE_STATE_DEPTH_WRITE,D3D12_RESOURCE_STATE_COPY_SOURCE};gpu.list->ResourceBarrier(1,&b);}
    gpu.Execute();
    for(unsigned e=0;e<2;++e) {
        auto bytes=gpu.Read(dst[e].Get());
        for(uint32_t y=1;y<h-1;++y)for(uint32_t x=1;x<w-1;++x) {
            float actual;std::memcpy(&actual,bytes.data()+4*(y*w+x),4);
            const float expected=(e?.8f:.4f)+.001f*(x+plan.eyes[e].jitter[0])+.002f*(y+plan.eyes[e].jitter[1]);
            Check(std::abs(actual-expected)<2e-6f,"eye-specific depth or jitter direction corrupted");
        }
    }
    // A MIDPOINT: B's depth moved half of B's motion vectors. A constant vector of 8 pixels (toward A) puts what B
    // has at x at x+4 in the midpoint, so the midpoint's pixel x reads B's x-4.
    {
        gpu.Begin();
        for(auto& d:dst){D3D12_RESOURCE_BARRIER b{};b.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;b.Transition={d.Get(),0,D3D12_RESOURCE_STATE_COPY_SOURCE,D3D12_RESOURCE_STATE_DEPTH_WRITE};gpu.list->ResourceBarrier(1,&b);}
        gpu.Execute();
        std::shared_ptr<const Inputs> mid[2];cvr::depth::Plan warp{};
        for(unsigned e=0;e<2;++e) {
            auto i=std::make_shared<Inputs>();
            i->view=e+1;i->hasDepth=i->hasMotion=i->submitted=true;i->width=i->motionWidth=w;i->height=i->motionHeight=h;
            i->queue=gpu.queue.Get();i->camera.nearPlane=.1f;i->camera.farPlane=100;i->camera.inverted=true;
            i->depth=gpu.Texture(w,h,DXGI_FORMAT_R32_FLOAT,D3D12_RESOURCE_STATE_COPY_DEST);
            i->motion=gpu.Texture(w,h,DXGI_FORMAT_R32G32_FLOAT,D3D12_RESOURCE_STATE_COPY_DEST);
            std::vector<float> pixels(w*h),vectors(w*h*2);
            for(uint32_t y=0;y<h;++y)for(uint32_t x=0;x<w;++x){pixels[y*w+x]=(e?.8f:.4f)+.001f*x+.002f*y;vectors[2*(y*w+x)]=8.f/w;}
            gpu.Upload(i->depth.Get(),pixels.data(),4,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
            gpu.Upload(i->motion.Get(),vectors.data(),8,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
            i->readyFence=gpu.fence;i->fenceValue=gpu.value;
            auto& v=warp.eyes[e];v.width=w;v.height=h;v.nearZ=100;v.farZ=.1f;v.motion=.5f;v.motionScale[0]=v.motionScale[1]=1;
            mid[e]=i;
        }
        gpu.Begin();
        Check(renderer.Record(gpu.device.Get(),gpu.list.Get(),gpu.queue.Get(),mid,targets,warp,1,gpu.fence.Get(),gpu.value+1),"midpoint depth resolve failed");
        gpu.Execute();
        gpu.Begin();
        for(auto& d:dst){D3D12_RESOURCE_BARRIER b{};b.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;b.Transition={d.Get(),0,D3D12_RESOURCE_STATE_DEPTH_WRITE,D3D12_RESOURCE_STATE_COPY_SOURCE};gpu.list->ResourceBarrier(1,&b);}
        gpu.Execute();
        for(unsigned e=0;e<2;++e) {
            auto bytes=gpu.Read(dst[e].Get());
            for(uint32_t y=1;y<h-1;++y)for(uint32_t x=6;x<w-1;++x) {
                float actual;std::memcpy(&actual,bytes.data()+4*(y*w+x),4);
                const float expected=(e?.8f:.4f)+.001f*(float(x)-4)+.002f*y;
                Check(std::abs(actual-expected)<2e-6f,"midpoint depth not moved half of B's motion vectors");
            }
        }
    }
    std::puts("PASS: stereo depth identity/FOV/planes, per-eye GPU output, jitter, midpoint warp and submit-fence lifetime");
}
void PresenterTimeline() {
    Gpu gpu;Presenter presenter;Settings settings;settings.enabled=true;settings.backend=Backend::FidelityFX;
    settings.flowScale=0;settings.autoPace=false;SetSettings(settings);
    constexpr unsigned w=128,h=96;
    std::array<ComPtr<ID3D12Resource>,2> color;
    std::array<std::shared_ptr<Inputs>,2> inputs;
    for(unsigned eye=0;eye<2;++eye) {
        auto in=std::make_shared<Inputs>();in->width=in->motionWidth=w;in->height=in->motionHeight=h;
        in->camera.nearPlane=.1f;in->camera.farPlane=100;in->camera.verticalFov=1.3f;in->camera.aspect=float(w)/h;
        in->camera.inverted=true;in->camera.cameraMotion=true;in->camera.motionScale[0]=in->camera.motionScale[1]=1;
        in->camera.right[0]=in->camera.up[1]=in->camera.forward[2]=1;
        color[eye]=gpu.Texture(w,h,DXGI_FORMAT_R8G8B8A8_UNORM,D3D12_RESOURCE_STATE_COPY_DEST);
        in->depth=gpu.Texture(w,h,DXGI_FORMAT_R32_FLOAT,D3D12_RESOURCE_STATE_COPY_DEST);
        in->motion=gpu.Texture(w,h,DXGI_FORMAT_R32G32_FLOAT,D3D12_RESOURCE_STATE_COPY_DEST);
        std::vector<uint32_t> pixels(w*h,0xFF403020);std::vector<float> depth(w*h,.01f),motion(w*h*2);
        gpu.Upload(color[eye].Get(),pixels.data(),4,D3D12_RESOURCE_STATE_COPY_SOURCE);
        gpu.Upload(in->depth.Get(),depth.data(),4,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
        gpu.Upload(in->motion.Get(),motion.data(),8,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
        inputs[eye]=in;
    }
    auto frame=[&](uint64_t serial,double stamp,float yaw) {
        Frame f;f.identity={serial,1,uint32_t(serial),{serial*2,serial*2+1},stamp,true,true,false};
        f.lease=std::make_shared<ReadLease>();
        for(unsigned eye=0;eye<2;++eye) {
            f.color[eye]=color[eye].Get();f.inputs[eye]=inputs[eye];
            f.pose[eye]={{0,std::sin(yaw*.5f),0,std::cos(yaw*.5f)},{eye?.032f:-.032f,1.7f,0}};
            f.fov[eye]={-.9f,.7f,.8f,-.95f};
            auto labelled=std::make_shared<Inputs>(*inputs[eye]);
            labelled->frameId=uint32_t(serial);labelled->origin=1;labelled->poseId=f.identity.pose[eye];labelled->view=eye+1;
            labelled->hasDepth=labelled->hasMotion=labelled->submitted=true;
            labelled->queue=gpu.queue.Get();labelled->readyFence=gpu.fence;labelled->fenceValue=gpu.value;
            DepthCamera(labelled->camera,f.fov[eye]);f.inputs[eye]=std::move(labelled);
        }
        return f;
    };
    auto submit=[&](const Selection& s){gpu.Wait();presenter.Submitted(s,gpu.fence.Get(),gpu.value,true);};
    auto a=presenter.Select(gpu.device.Get(),gpu.queue.Get(),frame(1,100,0),true);
    Check(!a.synthetic && a.frame.identity.capturedMs==100,"first real timestamp");submit(a);
    auto b=presenter.Select(gpu.device.Get(),gpu.queue.Get(),frame(2,122,.1f),true);
    Check(b.synthetic && b.kind==Cadence::Next::Midpoint,"second pair did not generate");
    Check(b.frame.identity.capturedMs==111,"midpoint has endpoint age");
    Check(std::abs(b.frame.pose[0].orientation.y-std::sin(.025f))<1e-6f,"midpoint pose");submit(b);
    // C exists now, but the displayed image is still the pending real B with all B metadata.
    auto pending=presenter.Select(gpu.device.Get(),gpu.queue.Get(),frame(3,144,.2f),true);
    Check(!pending.synthetic && pending.kind==Cadence::Next::PendingReal,"pending endpoint lost");
    Check(pending.frame.identity.serial==2 && pending.frame.identity.capturedMs==122,"pending image acquired C timestamp");
    XrCompositionLayerProjectionView pendingViews[2]{};cvr::depth::Plan depthPlan;
    for(unsigned eye=0;eye<2;++eye){pendingViews[eye].pose=pending.frame.pose[eye];pendingViews[eye].fov=pending.frame.fov[eye];}
    Check(pending.frame.inputs[0]->frameId==2 && pending.frame.inputs[1]->frameId==2 &&
        cvr::depth::Describe(pending,pendingViews,2,1,depthPlan)==cvr::depth::Reject::None,"pending B lost its own depth to newer C");
    Check(cvr::depth::Describe(b,pendingViews,2,1,depthPlan)==cvr::depth::Reject::Identity,"midpoint depth labelled with B's pose");
    XrCompositionLayerProjectionView midViews[2]{};
    for(unsigned eye=0;eye<2;++eye){midViews[eye].pose=b.frame.pose[eye];midViews[eye].fov=b.frame.fov[eye];}
    Check(cvr::depth::Describe(b,midViews,2,1,depthPlan)==cvr::depth::Reject::None && depthPlan.eyes[0].motion==.5f &&
        depthPlan.eyes[1].motion==.5f,"midpoint has no depth of its own");
    Check(cvr::depth::Describe(pending,pendingViews,2,1,depthPlan)==cvr::depth::Reject::None && depthPlan.eyes[0].motion==0,
        "real B depth moved");
    Check(std::abs(pending.frame.pose[0].orientation.y-std::sin(.05f))<1e-6f,"pending image acquired C pose");
    gpu.Wait();presenter.Submitted(pending,gpu.fence.Get(),gpu.value,false,true);
    Check(presenter.Pending(),"temporary submission failure discarded an owed real endpoint");
    settings.enabled=false;SetSettings(settings);
    auto retry=presenter.Select(gpu.device.Get(),gpu.queue.Get(),frame(4,166,.3f),true);
    Check(retry.kind==Cadence::Next::PendingReal && retry.frame.identity.serial==2 &&
          retry.frame.identity.capturedMs==122,"failed endpoint was replaced after framegen OFF");
    submit(retry);Check(!presenter.Pending(),"accepted retry retained endpoint");
    settings.enabled=true;SetSettings(settings);
    auto next=presenter.Select(gpu.device.Get(),gpu.queue.Get(),frame(3,144,.2f),true);submit(next);
    Check(presenter.Pending(),"next midpoint not created");
    auto rejected=presenter.Select(gpu.device.Get(),gpu.queue.Get(),frame(4,166,.3f),true);
    gpu.Wait();presenter.Submitted(rejected,gpu.fence.Get(),gpu.value,false);
    Check(!presenter.Pending(),"non-SteamVR failure behavior changed");
    presenter.Reset();settings.enabled=false;SetSettings(settings);
}
void GpuInputCopy() {
    Gpu gpu;Inputs input;
    constexpr unsigned width=7,height=5,cropWidth=3,cropHeight=2;
    constexpr auto read=D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
    auto transition=[&](ID3D12Resource* resource,D3D12_RESOURCE_STATES before,D3D12_RESOURCE_STATES after){
        D3D12_RESOURCE_BARRIER b{};b.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        b.Transition={resource,D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,before,after};gpu.list->ResourceBarrier(1,&b);
    };
    for(auto format:{DXGI_FORMAT_R16G16_FLOAT,DXGI_FORMAT_R16G16B16A16_FLOAT,DXGI_FORMAT_R32G32_FLOAT,DXGI_FORMAT_R16G16_FLOAT}) {
        const unsigned stride=format==DXGI_FORMAT_R16G16_FLOAT?4:8;
        std::vector<uint8_t> pixels(width*height*stride);
        for(unsigned i=0;i<width*height;++i) {
            const float values[]{float(i)/32,-float(i)/64,1000.f,-500.f};
            if(format==DXGI_FORMAT_R32G32_FLOAT)std::memcpy(pixels.data()+i*stride,values,8);
            else for(unsigned c=0;c<stride/2;++c){
                const uint16_t half=DirectX::PackedVector::XMConvertFloatToHalf(values[c]);
                std::memcpy(pixels.data()+i*stride+c*2,&half,2);
            }
        }
        auto source=gpu.Texture(width,height,format,D3D12_RESOURCE_STATE_COPY_DEST);
        gpu.Upload(source.Get(),pixels.data(),stride,read);
        InputTag tag{source.Get(),read,1,2,1,cropWidth,cropHeight};
        gpu.Begin();Check(CopyInput(gpu.list.Get(),tag,input),"motion input copy failed");gpu.Execute();
        Check(input.hasMotion && input.motionWidth==cropWidth && input.motionHeight==cropHeight,"motion extent metadata changed");
        Check(input.motion->GetDesc().Format==format && input.bytes==cropWidth*cropHeight*stride,"motion format or VRAM accounting changed");
        gpu.Begin();transition(input.motion.Get(),read,D3D12_RESOURCE_STATE_COPY_SOURCE);gpu.Execute();
        const auto copied=gpu.Read(input.motion.Get(),stride);
        for(unsigned y=0;y<cropHeight;++y)
            Check(!std::memcmp(copied.data()+y*cropWidth*stride,pixels.data()+((y+1)*width+2)*stride,cropWidth*stride),"motion channels or crop changed");
        gpu.Begin();transition(input.motion.Get(),D3D12_RESOURCE_STATE_COPY_SOURCE,read);gpu.Execute();
        auto* previous=input.motion.Get();
        gpu.Begin();Check(CopyInput(gpu.list.Get(),tag,input),"motion copy could not reuse its resource");gpu.Execute();
        Check(input.motion.Get()==previous,"same-format input needlessly recreated its resource");
        tag.x=width;gpu.Begin();Check(!CopyInput(gpu.list.Get(),tag,input),"out-of-bounds input accepted");gpu.Execute();
        Check(input.motion.Get()==previous,"rejected input changed the existing resource");
    }
    auto invalid=gpu.Texture(width,height,DXGI_FORMAT_R8G8B8A8_UNORM,read);
    gpu.Begin();Check(!CopyInput(gpu.list.Get(),{invalid.Get(),read,1},input),"unsupported motion format accepted");gpu.Execute();
    auto depth=gpu.Texture(width,height,DXGI_FORMAT_R32_FLOAT,D3D12_RESOURCE_STATE_COPY_DEST);
    std::vector<float> values(width*height,.25f);gpu.Upload(depth.Get(),values.data(),4,read);
    gpu.Begin();Check(CopyInput(gpu.list.Get(),{depth.Get(),read,0},input),"existing depth path broke");gpu.Execute();
    Check(input.hasDepth && input.width==width && input.height==height && input.bytes==width*height*4+cropWidth*cropHeight*4,"depth/motion allocation accounting is wrong");
    gpu.Begin();transition(input.depth.Get(),read,D3D12_RESOURCE_STATE_COPY_SOURCE);gpu.Execute();
    const auto copied=gpu.Read(input.depth.Get());
    Check(copied.size()==values.size()*4 && !std::memcmp(copied.data(),values.data(),copied.size()),"depth values changed");
}
// Real-frame release against the display: the XR thread takes the newest real frame at each cycle start and shows
// midpoint, endpoint, then needs the next real frame (Cadence). A cycle with nothing new is a repeat (reprojected).
struct PacingRun {double repeats{},realStep{},spread{};};
PacingRun SimulatePacing(bool phaseLocked,double actualPeriod,double reportedPeriod,double startPhase,double work,
                         double workJitter,double interval,unsigned cycles=4000,bool gate=true) {
    uint64_t seed=0x9E3779B97F4A7C15ull^uint64_t(startPhase*1000+work*77+workJitter*13);
    auto random=[&] {seed=seed*6364136223846793005ull+1442695040888963407ull;return double(seed>>11)*(1.0/9007199254740992.0);};
    std::vector<double> select(cycles);
    for(unsigned n=0;n<cycles;++n)select[n]=startPhase+n*actualPeriod+(random()-.5)*.3;
    std::vector<double> published;
    PhaseLockedPacing phase;CycleClock clock;size_t ticked=0;double oldLast=0,release=0;
    while(release<select.back()) {
        const double now=release+work+(random()-.5)*2*workJitter;
        published.push_back(now);
        const auto at=std::upper_bound(select.begin(),select.end(),now);
        for(;ticked<size_t(at-select.begin());++ticked)clock.Tick(select[ticked],reportedPeriod); // the XR thread
        double cycle=at==select.begin() ? select.front()-actualPeriod : *(at-1);
        if(gate && !clock.Steady())cycle=std::nan("");
        double hold=0;
        if(phaseLocked)hold=phase.Hold(now,cycle,reportedPeriod,interval);
        else {const double remaining=oldLast+interval-now;hold=remaining>.2 && remaining<=80 ? remaining : 0;}
        release=now+hold+random()*.3; // timer overshoot
        if(phaseLocked)phase.Released(release);
        else oldLast=oldLast && release<oldLast+interval*1.5 ? oldLast+interval : release;
    }
    Cadence cadence;unsigned repeats=0,counted=0;size_t newest=0;double firstReal=0,lastReal=0;unsigned reals=0;
    for(unsigned n=0;n<cycles;++n) {
        while(newest<published.size() && published[newest]<=select[n])++newest;
        const auto kind=cadence.Select(newest,newest>1,true);cadence.Accepted(kind,newest);
        if(n<400)continue;
        ++counted;repeats+=kind==Cadence::Next::Repeat;
        if(kind==Cadence::Next::Midpoint) {if(!reals)firstReal=select[n];lastReal=select[n];++reals;}
    }
    // How evenly the real frames come: the spread of their Present intervals past the warm-up.
    double sum=0,squares=0;unsigned n=0;
    for(size_t i=published.size()/10+1;i<published.size();++i) {const double d=published[i]-published[i-1];sum+=d;squares+=d*d;++n;}
    const double mean=n ? sum/n : 0,spread=n ? std::sqrt(std::max(0.0,squares/n-mean*mean)) : 0;
    return {double(repeats)/counted,reals>1 ? (lastReal-firstReal)/(reals-1)/actualPeriod : 0,spread};
}
void PhasePacing() {
    const double period=1000.0/75;
    double oldWorst=0,newWorst=0;unsigned oldBad=0;
    // Every start phase of the display against the game, small frame-time jitter: the lottery of the report.
    for(unsigned i=0;i<24;++i) {
        const double startPhase=period*i/24;
        const auto old=SimulatePacing(false,period,period,startPhase,9,1.5,2*period);
        const auto locked=SimulatePacing(true,period,period,startPhase,9,1.5,2*period);
        oldWorst=std::max(oldWorst,old.repeats);newWorst=std::max(newWorst,locked.repeats);oldBad+=old.repeats>.05;
        Check(std::abs(locked.realStep-2)<.01,"locked real frames are not two display periods apart");
    }
    std::printf("free-running: worst %.1f%% repeats, %u/24 start phases over 5%%; phase-locked: worst %.2f%%\n",
                oldWorst*100,oldBad,newWorst*100);
    Check(oldBad>0,"test premise: the free-running release never lost its phase");
    Check(newWorst<.005,"phase-locked release still repeats display frames");
    // Adaptive pacing stretched the interval by fractions of a period after the misses: the free-running phase then
    // drifts through the boundary on its own. Locked, a 4 % stretch stays two periods.
    double oldStretch=0,lockedStretch=0;
    for(unsigned i=0;i<8;++i) {
        oldStretch=std::max(oldStretch,SimulatePacing(false,period,period,period*i/8,9,.5,2.08*period).repeats);
        lockedStretch=std::max(lockedStretch,SimulatePacing(true,period,period,period*i/8,9,.5,2.08*period).repeats);
    }
    std::printf("interval stretched 4%%: free-running worst %.1f%% repeats, phase-locked %.2f%%\n",oldStretch*100,lockedStretch*100);
    Check(oldStretch>.02 && lockedStretch<.005,"stretched interval: free-running premise or lock failed");
    // The runtime's period a little off the real one: the grid comes from the measured cycle start, not the period.
    const auto lockedDrift=SimulatePacing(true,period,period*1.0015,period*.5,9,.5,2*period,20000);
    Check(lockedDrift.repeats<.005,"phase lock did not follow the measured cycle");
    // Adaptive pacing a little above half refresh keeps two periods; past a quarter period it is three, still locked.
    Check(std::abs(SimulatePacing(true,period,period,3,9,1.5,2.1*period).realStep-2)<.01,"small stretch broke the pair cadence");
    const auto three=SimulatePacing(true,period,period,3,9,1.5,2.3*period);
    Check(std::abs(three.realStep-3)<.01 && std::abs(three.repeats-1.0/3)<.01,"larger stretch is not a steady three-period step");
    // A game slower than two periods (GPU-bound) is never held long and lands on the grid.
    const auto slow=SimulatePacing(true,period,period,3,2.4*period,1,2*period);
    Check(std::abs(slow.realStep-3)<.02,"slow game not stepped to whole display periods");
    // No XR cycle for over half a second: no grid to lock to, the plain interval.
    PhaseLockedPacing p;double t=1000;
    for(unsigned i=0;i<5;++i) {const double hold=p.Hold(t,0,period,2*period);p.Released(t+hold);t+=hold+5;}
    Check(std::abs(p.Target()-(1000+4*2*period))<.01,"stale cycle did not fall back to the interval");
    Check(p.Hold(std::nan(""),0,period,2*period)==0 && p.Target()==0,"invalid time kept a schedule");
    // xrWaitFrame returning faster than its predicted period (SteamVR null driver: 136/s against 90 Hz). Locked to
    // each return, the real frames spread; the clock check falls back to the even interval.
    const double ninety=1000.0/90,fast=1000.0/136.6;
    const auto raw=SimulatePacing(true,fast,ninety,1,9,1.5,2*ninety,6000,false);
    const auto gated=SimulatePacing(true,fast,ninety,1,9,1.5,2*ninety,6000,true);
    const auto plain=SimulatePacing(false,fast,ninety,1,9,1.5,2*ninety,6000);
    std::printf("fast XR clock: real-frame interval spread locked %.2f ms, checked %.2f ms, free-running %.2f ms\n",
                raw.spread,gated.spread,plain.spread);
    Check(raw.spread>1.5,"test premise: locking to a fast clock did not spread the frames");
    Check(gated.spread<plain.spread+.2,"clock check did not fall back to the even interval");
    // The clock itself: an even 90 Hz with jitter and skipped cycles keeps its grid; a fast or uneven loop does not.
    uint64_t seed=7;auto jitter=[&] {seed=seed*6364136223846793005ull+1442695040888963407ull;return (double(seed>>11)/9007199254740992.0-.5)*1.0;};
    CycleClock even;double t2=0;
    for(unsigned i=0;i<300;++i) {t2+=ninety*(i%17==0 ? 2 : 1)+jitter();even.Tick(t2,ninety);}
    Check(even.Steady(),"an even display clock with skipped cycles was not trusted");
    CycleClock quick;t2=0;for(unsigned i=0;i<300;++i) {t2+=fast+jitter();quick.Tick(t2,ninety);}
    Check(!quick.Steady(),"a loop faster than its period was trusted");
    CycleClock uneven;t2=0;for(unsigned i=0;i<300;++i) {t2+=ninety*(i%2 ? .6 : 1.4);uneven.Tick(t2,ninety);}
    Check(!uneven.Steady(),"an uneven loop was trusted");
    even.Tick(t2+1000,ninety);Check(!even.Steady(),"a stalled loop kept its trust");
}
int main(int argc,char** argv) try {
    setvbuf(stdout,nullptr,_IONBF,0);SetUnhandledExceptionFilter(Crash);
    Check(argc>=2,"case required");const std::string test=argv[1];
    if(test=="replay") {
        // framegen_tests replay <capture directory> <output directory>
        Check(argc==4,"replay <capture> <out>");Gpu gpu(true);ReplayGpu r;r.device=gpu.device.Get();r.queue=gpu.queue.Get();
        r.texture=[&](unsigned w,unsigned h,DXGI_FORMAT f,D3D12_RESOURCE_STATES s){return gpu.Texture(w,h,f,s);};
        r.upload=[&](ID3D12Resource* t,const void* d,unsigned p,D3D12_RESOURCE_STATES s){gpu.Upload(t,d,p,s);};
        r.read=[&](ID3D12Resource* t){return gpu.Read(t);};r.wait=[&]{gpu.Wait();};
        const int result=Replay(r,argv[2],argv[3]);StopHardwareTelemetry();return result;
    }
    if(test=="policy") {
        Identity a{1,1,10,{5,6},100,true,true,false},b{2,1,11,{5,6},122,true,true,false};
        Check(CanInterpolate(a,b),"same tracked pose across two real frames rejected");
        auto c=b;c.nativeFrame=12;Check(!CanInterpolate(a,c),"wrong motion-vector interval accepted");
        c=b;c.origin=2;Check(!CanInterpolate(a,c),"recenter crossed histories");c=b;c.inputs=false;Check(!CanInterpolate(a,c),"missing inputs accepted");
        c=b;c.reset=true;Check(!CanInterpolate(a,c),"camera cut accepted");c=b;c.capturedMs=250;Check(!CanInterpolate(a,c),"stale source accepted");
        c=b;c.stereo=false;Check(!CanInterpolate(a,c),"one-eye fallback interpolated");
    } else if(test=="stereo_depth") {
        StereoDepth();
    } else if(test=="presenter_timeline") {
        PresenterTimeline();
    } else if(test=="cadence") {
        Cadence c;Check(c.Select(1,false,true)==Cadence::Next::Real,"first frame not real");c.Accepted(Cadence::Next::Real,1);
        Check(c.Select(2,true,true)==Cadence::Next::Midpoint,"midpoint missing");c.Accepted(Cadence::Next::Midpoint,2);
        Check(c.Select(3,true,true)==Cadence::Next::PendingReal,"newer frame displaced endpoint");
        Check(c.Select(3,true,false)==Cadence::Next::PendingReal,"toggle dropped endpoint");c.Accepted(Cadence::Next::PendingReal,2);
        Check(c.Select(2,true,true)==Cadence::Next::Repeat,"repeat counted as new");c.Reset();Check(c.Pending()==0,"reset retained old endpoint");
    } else if(test=="settings") {
        Settings s;Check(s.backend==Backend::Nvidia && s.flowScale==50,"defaults are not NVIDIA OFA + FidelityFX at half-resolution flow");
        Check(ParseSetting("xr_framegen_flow_scale=0",s) && s.flowScale==0,"optical flow off parse");
        Check(ParseSetting("xr_framegen_backend=2",s) && s.backend==Backend::DlssG,"DLSS-G parse");
        Check(ParseSetting("xr_framegen_backend=3",s) && s.backend==Backend::Anr,"ANR FG parse");
        {Settings anrFg;anrFg.backend=Backend::Anr;SetSettings(anrFg);Check(GetSettings().backend==Backend::Anr,"ANR FG clamped away by SetSettings");SetSettings(Settings{});}
        Check(ParseSetting("xr_framegen=1",s) && s.enabled,"enable parse");
        Check(ParseSetting("xr_framegen_backend=0",s) && s.backend==Backend::FidelityFX,"backend parse");
        Check(ParseSetting("xr_framegen_flow_scale=75",s) && s.flowScale==75,"scale parse");
        Check(!Settings{}.cadenceLock,"cadence lock on by default: the refresh the headset was set to is the rate");
        Check(ParseSetting("xr_framegen_cadence_lock=1",s) && s.cadenceLock,"cadence lock parse");
        s.overlay=true;s.quality=1;s.overlayCorner=7;s.autoPace=false;s.overlayLayout=1;s.overlayFollow=1;
        s.historySeconds=60;s.overlayCone=35;s.overlayScale=1.2f;s.overlayDistance=2;s.overlayX=-3;s.overlayY=4;
        FILE* file=nullptr;Check(tmpfile_s(&file)==0,"temp file");WriteSettings(file,s);rewind(file);Settings restored;char line[256];
        while(fgets(line,sizeof(line),file))Check(ParseSetting(line,restored),"saved key did not parse");fclose(file);
        Check(restored.enabled && restored.overlay && !restored.autoPace && restored.backend==s.backend && restored.flowScale==75 && restored.quality==1 && restored.overlayCorner==7,"settings roundtrip");
        Check(restored.cadenceLock,"cadence lock roundtrip");
        Check(restored.overlayLayout==1 && restored.overlayFollow==1 && restored.historySeconds==60 && restored.overlayCone==35 &&
            std::abs(restored.overlayScale-1.2f)<.0001f && restored.overlayDistance==2 && restored.overlayX==-3 && restored.overlayY==4,"overlay settings lost");
    } else if(test=="cadence_lock") {
        constexpr double p=11.1111,two=p*2,three=p*3;
        CadenceLock lock;double t=0,interval=0;
        const auto run=[&](double seconds,const auto& work) {
            for(const double end=t+seconds*1000;t<end;t+=22.2222)interval=lock.Observe(t,p,work(unsigned(t/22.2222)));
        };
        // The measured H10 sprint: 8-14 ms of game time, with teleport-length hitches.
        run(5,[](unsigned i){return 8.0+(i%7);});
        Check(interval==two,"healthy game left two-period cadence");
        run(3,[](unsigned i){return i%40==0 ? 140.0 : i%45==0 ? 30.0 : 11.0;});
        Check(interval==two,"a hitch or an isolated slow frame dropped the cadence to 30 fps");
        // A game that cannot make 22.2 ms: a steady 30 instead of an alternating 22/33.
        run(2,[](unsigned i){return i%3==0 ? 21.8 : 18.0;});
        Check(interval==three,"sustained over-budget frames kept two periods");
        run(2,[](unsigned i){return i%8==0 ? 20.0 : 15.0;});
        Check(interval==three,"returned to two periods with frames still near the budget");
        run(2.5,[](unsigned){return 14.0;});
        Check(interval==two,"recovered headroom did not restore 45 fps within two windows");
        // Never an interval between whole periods: that mixes 22 and 33 ms frames on the display grid.
        for(double w=5;w<40;w+=.37) {
            const double v=lock.Observe(t+=22.2222,p,w);
            Check(v==two || v==three,"fractional interval");
        }
        Check(lock.Observe(t,13.8889,10)==13.8889*2,"refresh change retained the old cadence");
        lock.Reset();Check(lock.Observe(0,p,NAN)==two,"reset retained three periods");
        Check(lock.Observe(1,NAN,10)==0,"invalid period accepted");
    } else if(test=="presentation_period") {
        constexpr int64_t normal=11111111,doubled=22222222;
        PresentationPeriod period;
        Check(period.Observe(0,normal)==normal,"initial display period");
        for(unsigned i=1;i<600;++i) {
            const auto reported=i%88==0?doubled:normal;
            Check(period.Observe(i*11.111,reported)==normal,"one throttled XR frame doubled the producer interval");
        }
        for(unsigned i=600;i<695;++i)period.Observe(i*11.111,13888889);
        Check(period.Observe(695*11.111,13888889)==13888889,"sustained 72 Hz refresh was ignored");
        Check(period.Observe(696*11.111,8333333)==8333333,"faster refresh did not recover immediately");
        period.Reset();Check(period.Observe(0,doubled)==doubled,"slow startup period");
        Check(period.Observe(11,normal)==normal,"throttled startup could not recover");
        Check(period.Observe(NAN,normal)==normal && period.Observe(12,0)==normal,"invalid period damaged state");
        Check(period.Observe(2000,13888889)==13888889,"resumed session retained stale period");
    } else if(test=="phase_pacing") {
        PhasePacing();
    } else if(test=="pacing") {
        Settings s;s.enabled=true;s.autoPace=true;SetSettings(s);DisplayPeriod(5000000);PacingReady(true);
        const auto started=NowMs();
        // Present jobs can migrate between workers. Their common deadline
        // must survive a thread exiting, rather than restarting its timer.
        for(unsigned i=0;i<16;++i) {std::thread worker([] {PaceRenderedFrame();});worker.join();}
        const auto elapsed=NowMs()-started;
        std::printf("16 worker-migrating frames: %.2f ms (target150 ms)\n",elapsed);
        Check(elapsed>=130 && elapsed<2000,"frame pacing lost its deadline on a thread change");
        s.enabled=false;SetSettings(s);PaceRenderedFrame();
    } else if(test=="metrics") {
        Settings options;options.overlay=true;SetSettings(options);
        const double now=NowMs();for(unsigned i=0;i<45;++i) {OnRealFrame(i+1,now-1000+i*1000.0/45);OnSubmitted(i+1,false,false,now-1000+i*1000.0/45);OnSubmitted(i+1,true,false,now-1000+(i+.5)*1000.0/45);}
        for(unsigned i=0;i<100;++i)OnSubmitted(45,false,true,now-1000+i*10);
        const auto s=GetStatistics();Check(s.realFrames==45 && s.generatedFrames==45 && s.repeatedFrames==100,"counter semantics");
        Check(s.realFps>43 && s.realFps<46 && s.generatedFps>43 && s.generatedFps<46 && s.outputFps>88 && s.outputFps<91,"repeat inflated FPS");
        ResetStatistics();const auto settled=NowMs();
        OnSubmitted(1,false,true,settled-5000);OnSubmitted(1,false,true,settled-20);OnSubmitted(1,false,true,settled-10);
        Check(std::abs(GetStatistics().repeatFps-1)<.01,"sparse repeats used their burst interval instead of the observation window");
        ResetStatistics();const auto high=NowMs();
        for(unsigned i=0;i<2000;++i)OnRealFrame(i+1,high-1000+i*.5);
        const auto fast=GetStatistics();Check(fast.realFps>1980 && fast.realFps<2005,"bounded rate ring capped high FPS");
    } else if(test=="metrics_disabled") {
        Settings options;options.enabled=true;options.overlay=true;SetSettings(options);
        const auto now=NowMs();OnRealFrame(1,now-20);OnRealFrame(2,now);OnGpuFrame(5);OnGeneration(2,2);OnInput(0,128,96);
        Check(GetStatistics().realFrames==2,"enabled metrics not collected");
        options.overlay=false;SetSettings(options);
        Check(Enabled() && !MetricsEnabled(),"overlay switch changed framegen or kept collection alive");
        const auto sequence=GetHardwareStatistics().sequence;
        for(unsigned i=0;i<8;++i) {OnRealFrame(i+3,now+20+i);OnSubmitted(i+3,true,false,now+20+i);OnGpuFrame(6);OnGeneration(3,3);OnInput(1,128,96);OnSkip();SetVram(1234);}
        const auto stopped=GetStatistics();
        Check(stopped.realFrames==0 && stopped.generatedFrames==0 && stopped.skipped==0 && stopped.vramBytes==0 &&
            stopped.historySamples==0 && !stopped.gpuValid && stopped.generationMs==0 && stopped.inputFrames[0]==0 && stopped.inputFrames[1]==0,
            "disabled telemetry continued collecting");
        Sleep(50);Check(GetHardwareStatistics().sequence==sequence && GetHardwareStatistics().cpuUsage<0,"hardware polling continued while disabled");
        options.overlay=true;SetSettings(options);OnRealFrame(50,now+10000);OnRealFrame(51,now+10020);
        const auto resumed=GetStatistics();Check(resumed.realFrames==2 && resumed.cpuMs==20,"re-enable retained stale history or a disabled-time spike");
    } else if(test=="debug_gate") {
        std::atomic<unsigned> counter{};unsigned work=0;
        CVR_DIAGNOSTIC(counter.fetch_add(++work));
        Check(!work && counter==0,"disabled counter evaluated diagnostic work");
        Settings options;options.enabled=true;options.overlay=true;SetSettings(options);
        auto now=NowMs();OnRealFrame(1,now-20);OnRealFrame(2,now);OnInput(0,128,96);OnInput(1,128,96);
        auto stats=GetStatistics();
        Check(Enabled() && stats.realFrames==2 && stats.inputsReady,"debug-off disabled product metrics or framegen");
        Check(CyberpunkVR_FramegenReportSeq==0 && !CyberpunkVR_FramegenReport[0],"debug-off published diagnostic JSON");
        CyberpunkVR_RuntimeDiagnostics=1;
        if(true)CVR_DIAGNOSTIC(counter.fetch_add(++work));else Check(false,"diagnostic macro changed if/else control flow");
        Check(work==1 && counter==1,"enabled diagnostic counter did not run");
        ResetStatistics();OnRealFrame(3,now);GetStatistics();
        const auto seq=CyberpunkVR_FramegenReportSeq.load();
        Check(seq>0 && !(seq&1) && std::strstr(CyberpunkVR_FramegenReport,"\"metrics\":1"),"debug-on did not publish a complete report");
        CyberpunkVR_RuntimeDiagnostics=0;ResetStatistics();OnRealFrame(4,now);GetStatistics();
        Check(CyberpunkVR_FramegenReportSeq==seq,"disabling diagnostics did not stop publication");
        options.overlay=false;SetSettings(options);
        Check(Enabled() && !MetricsEnabled() && CyberpunkVR_FramegenReportSeq==seq,"overlay-off reactivated diagnostic publication");
    } else if(test=="history") {
        FrameHistory<2048> h;
        for(unsigned i=0;i<1000;++i)h.Add(i*10.0,i<990?10:20);
        const auto summary=h.Summarize(9990,30);
        Check(summary.samples==1000 && std::abs(summary.average-10.1)<1e-9,"history average");
        Check(1000/summary.peak==50 && 1000/summary.minimum==100,"minimum/maximum FPS reversed or incorrect");
        Check(summary.low1==50 && summary.low01==50 && summary.p999==20,"low FPS must average slowest samples");
        const auto graph=h.Graph(9990,30);Check(*std::max_element(graph.begin(),graph.end())==20,"graph hid a spike");
        Check(h.Summarize(50000,10).samples==0,"expired history retained");h.Clear();Check(h.Summarize(9990,30).samples==0,"reset retained history");
    } else if(test=="hardware") {
        Gpu gpu(true);Settings settings;settings.overlay=true;SetSettings(settings);ConfigureHardwareTelemetry(gpu.device.Get());
        HardwareStatistics h;
        for(unsigned i=0;i<30;++i) {Sleep(100);h=GetHardwareStatistics();if(h.cpuUsage>=0 && h.gpuUsage>=0 && h.coreMaximum>=0)break;}
        Check(h.ramTotal>0 && h.ramUsed<=h.ramTotal && h.appWorkingSet>0,"RAM metrics unavailable");
        Check(h.cpuUsage>=0 && h.cpuUsage<=100 && h.coreCount>0,"CPU load unavailable");
        Check(h.gpuUsage>=0 && h.gpuUsage<=100 && h.gpuMemoryValid && h.gpuMemoryTotal>=h.gpuMemoryUsed,"NVIDIA telemetry unavailable");
        std::printf("CPU %.1f%% cores %u GPU %.1f%% %.0fC VRAM %.2f/%.2fGiB RAM %.2f/%.2fGiB poll %.3fms\n",
            h.cpuUsage,h.coreCount,h.gpuUsage,h.gpuTemperature,h.gpuMemoryUsed/(1024.*1024*1024),h.gpuMemoryTotal/(1024.*1024*1024),
            h.ramUsed/(1024.*1024*1024),h.ramTotal/(1024.*1024*1024),h.sampleMs);
        const auto before=h.sequence;StopHardwareTelemetry();ConfigureHardwareTelemetry(gpu.device.Get());
        for(unsigned i=0;i<20 && GetHardwareStatistics().sequence<=before;++i)Sleep(100);
        Check(GetHardwareStatistics().sequence>before,"monitor did not restart on the same adapter");
    } else if(test=="gpu_timer") {
        Gpu gpu;Settings settings;settings.overlay=true;SetSettings(settings);
        for(unsigned i=0;i<20;++i) {BeforeGameCommands(gpu.queue.Get());gpu.Begin();gpu.Execute();FinishGpuFrame();gpu.Wait();}
        const auto stats=GetStatistics();Check(stats.gpuValid && stats.gpuMs>=0 && stats.gpuMs<500,"timestamp reports are invalid");
        settings.overlay=false;SetSettings(settings);FinishGpuFrame();gpu.Wait();FinishGpuFrame();
    } else if(test=="planar_depth") {
        Gpu gpu;
        auto source=gpu.Texture(64,64,DXGI_FORMAT_D32_FLOAT_S8X24_UINT,D3D12_RESOURCE_STATE_DEPTH_WRITE,D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL);
        D3D12_DESCRIPTOR_HEAP_DESC hd{};hd.Type=D3D12_DESCRIPTOR_HEAP_TYPE_DSV;hd.NumDescriptors=1;
        ComPtr<ID3D12DescriptorHeap> heap;Hr(gpu.device->CreateDescriptorHeap(&hd,IID_PPV_ARGS(&heap)));
        D3D12_DEPTH_STENCIL_VIEW_DESC vd{};vd.Format=DXGI_FORMAT_D32_FLOAT_S8X24_UINT;vd.ViewDimension=D3D12_DSV_DIMENSION_TEXTURE2D;
        const auto handle=heap->GetCPUDescriptorHandleForHeapStart();gpu.device->CreateDepthStencilView(source.Get(),&vd,handle);
        gpu.Begin();gpu.list->ClearDepthStencilView(handle,D3D12_CLEAR_FLAG_DEPTH,.25f,0,0,nullptr);
        D3D12_RESOURCE_BARRIER b{};b.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        b.Transition={source.Get(),0,D3D12_RESOURCE_STATE_DEPTH_WRITE,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE};gpu.list->ResourceBarrier(1,&b);
        Inputs copied;Check(CopyInput(gpu.list.Get(),{source.Get(),D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,0,0,0,64,64},copied),"planar input copy failed");
        b.Transition={source.Get(),1,D3D12_RESOURCE_STATE_DEPTH_WRITE,D3D12_RESOURCE_STATE_DEPTH_READ};gpu.list->ResourceBarrier(1,&b);
        b.Transition={copied.depth.Get(),0,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,D3D12_RESOURCE_STATE_COPY_SOURCE};gpu.list->ResourceBarrier(1,&b);gpu.Execute();
        const auto values=gpu.Read(copied.depth.Get());float value{};std::memcpy(&value,values.data(),sizeof(value));Check(value==.25f,"depth plane values changed");
    } else if(test=="input_copy")GpuInputCopy();
    else if(test=="gpu_fidelityfx_rgba16" || test=="gpu_nvidia_rgba16")GpuGeneration(test=="gpu_nvidia_rgba16",true,true);
    else if(test=="gpu_fidelityfx" || test=="gpu_nvidia")GpuGeneration(test=="gpu_nvidia");
    else if(test=="gpu_fidelityfx_half_motion" || test=="gpu_nvidia_half_motion")GpuGeneration(test=="gpu_nvidia_half_motion",true,false,true);
    else if(test=="gpu_fidelityfx_flow" || test=="gpu_nvidia_flow")GpuGeneration(test=="gpu_nvidia_flow",false);
    else if(test=="gpu_fidelityfx_noflow" || test=="gpu_nvidia_noflow")GpuGeneration(test=="gpu_nvidia_noflow",true,false,false,Backend::FidelityFX,0);
    else if(test=="gpu_dlssg")GpuGeneration(true,true,false,false,Backend::DlssG);
    else if(test=="gpu_anr")GpuGeneration(true,true,false,false,Backend::Anr);
    else if(test=="overlay" || test=="overlay_follow") {
        Gpu gpu;mock::device=gpu.device.Get();Settings settings;settings.overlay=true;SetSettings(settings);StatsOverlay overlay;
        auto prepare=[&] {return reinterpret_cast<const XrCompositionLayerQuad*>(overlay.Prepare(reinterpret_cast<XrSession>(1),reinterpret_cast<XrSpace>(1),gpu.device.Get(),gpu.queue.Get()));};
        mock::timeout=true;Check(prepare()==nullptr,"submitted an unwritten overlay");Check(mock::acquisitions==1 && mock::releases==0,"timeout ownership");
        settings.overlay=false;SetSettings(settings);Check(prepare()==nullptr && mock::texture && mock::releases==0,"disabled pending image was released before a successful wait");
        mock::timeout=false;Check(prepare()==nullptr && !mock::texture && !mock::acquired,"disabled overlay did not retire after its image became available");
        settings.overlay=true;SetSettings(settings);mock::timeout=true;Check(!prepare(),"new pending image submitted");
        mock::timeout=false;auto layer=prepare();Check(layer && layer->eyeVisibility==XR_EYE_VISIBILITY_BOTH,"overlay duplicated per-eye layers");gpu.Wait();
        const auto acquisitions=mock::acquisitions;for(int i=0;i<10;++i)Check(prepare()!=nullptr,"cached overlay disappeared");
        Check(mock::acquisitions==acquisitions,"overlay redraws at frame rate");
        if(test=="overlay_follow") {
            XrPosef head{{0,0,0,1},{0,1,0}};int64_t time=1000000000;
            auto tracked=[&] {time+=16666667;return reinterpret_cast<const XrCompositionLayerQuad*>(overlay.Prepare(reinterpret_cast<XrSession>(1),reinterpret_cast<XrSpace>(1),gpu.device.Get(),gpu.queue.Get(),reinterpret_cast<XrSpace>(2),&head,1,time));};
            auto l=tracked();Check(l && l->space==reinterpret_cast<XrSpace>(2),"free-look panel not world referenced");
            head.orientation={0,std::sin(.25f),0,std::cos(.25f)};l=tracked();Check(l->pose.orientation.y==0,"panel follows inside free cone");
            head.orientation={0,std::sin(.8f),0,std::cos(.8f)};for(int i=0;i<40;++i)l=tracked();Check(l->pose.orientation.y>.1f,"panel never catches up");
            settings.overlayFollow=0;settings.overlayCorner=5;SetSettings(settings);l=tracked();Check(l->space==reinterpret_cast<XrSpace>(1) && std::abs(l->pose.position.x)<.001f && l->pose.position.y<0,"bottom-center preset or head lock failed");
        }
        for(int layout=1;layout<=2;++layout) {settings.overlayLayout=layout;SetSettings(settings);Check(prepare()!=nullptr,"layout switch lost panel");gpu.Wait();}
        settings.overlay=false;SetSettings(settings);Check(!prepare(),"disabled overlay still submitted");
        Check(!mock::texture,"disabled overlay kept its swapchain");overlay.Shutdown();
    } else Check(false,"unknown test");
    StopHardwareTelemetry();std::printf("PASS %s\n",test.c_str());return 0;
} catch(const std::exception& error) {std::fprintf(stderr,"FAIL: %s\n",error.what());return 1;}
