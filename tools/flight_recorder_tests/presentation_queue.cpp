#define main synthesis_queue_fixture_main
#include "synthesis_queue.cpp"
#undef main
#include "Runtimes/PresentationQueue.hpp"
#include "Runtimes/DepthSwapchainImage.hpp"
#include "Framegen/FlightRecorder.hpp"
#include <cstdarg>
#include "Render/HudPanel.hpp"
void Log(const char*,...){}
static unsigned appWaits{},runtimeWaits{},syncFailures{},releasedImages{};
static XrResult waitResult=XR_SUCCESS;
static unsigned acquiredImages{};
extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrAcquireSwapchainImage(XrSwapchain,const XrSwapchainImageAcquireInfo*,uint32_t* index){*index=2;++acquiredImages;return XR_SUCCESS;}
namespace cvr::framegen::flight {
void Time(Timing,double){}
void Display(const DisplayDeadline&,double,double){}
void Count(Event e){if(e==Event::PresentationApplicationWait)++appWaits;
 if(e==Event::PresentationRuntimeWait)++runtimeWaits;if(e==Event::PresentationSyncFailed)++syncFailures;}
}
extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrWaitSwapchainImage(XrSwapchain,const XrSwapchainImageWaitInfo*){return waitResult;}
extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrReleaseSwapchainImage(XrSwapchain,const XrSwapchainImageReleaseInfo*){++releasedImages;return XR_SUCCESS;}
void runPresentation(ID3D12Device* d,bool isolated){
    D3D12_COMMAND_QUEUE_DESC desc{};ComPtr<ID3D12CommandQueue> app;
    hr(d->CreateCommandQueue(&desc,IID_PPV_ARGS(&app)),"application queue");
    cvr::xrqueue::GpuQueue route;check(route.Initialize(d,app.Get(),isolated),"binding initialize");
    check(route.Private()==isolated,"private/shared binding");
    SynthesisQueue synth;check(synth.Initialize(d,app.Get(),isolated),"synthesis initialize");
    ComPtr<ID3D12Fence> input,generated,displayed,gate,appTail;
    for(auto* f:{input.GetAddressOf(),generated.GetAddressOf(),displayed.GetAddressOf(),gate.GetAddressOf(),appTail.GetAddressOf()})hr(d->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(f)),"fence");
    const unsigned bytes=256;
    auto upload=buffer(d,D3D12_HEAP_TYPE_UPLOAD,bytes,D3D12_RESOURCE_STATE_GENERIC_READ);
    auto capture=buffer(d,D3D12_HEAP_TYPE_DEFAULT,bytes,D3D12_RESOURCE_STATE_COPY_DEST);
    auto output=buffer(d,D3D12_HEAP_TYPE_DEFAULT,bytes,D3D12_RESOURCE_STATE_COPY_DEST);
    auto rb=buffer(d,D3D12_HEAP_TYPE_READBACK,bytes,D3D12_RESOURCE_STATE_COPY_DEST);
    void* ptr{};hr(upload->Map(0,nullptr,&ptr),"upload map");for(unsigned j=0;j<bytes/4;++j)static_cast<uint32_t*>(ptr)[j]=0xD00D0000+j;upload->Unmap(0,nullptr);
    std::vector<Commands> cmds;cmds.reserve(32*3);
    for(unsigned frame=1;frame<=24;++frame){
        cmds.emplace_back(d);auto& a=cmds.back();
        if(frame>1)barrier(a.l.Get(),capture.Get(),D3D12_RESOURCE_STATE_COPY_SOURCE,D3D12_RESOURCE_STATE_COPY_DEST);
        a.l->CopyBufferRegion(capture.Get(),0,upload.Get(),0,bytes);
        barrier(a.l.Get(),capture.Get(),D3D12_RESOURCE_STATE_COPY_DEST,D3D12_RESOURCE_STATE_COPY_SOURCE);a.submit(app.Get());
        hr(app->Signal(input.Get(),frame),"captured input receipt");
        if(isolated){hr(app->Wait(gate.Get(),frame),"block unrelated next game rendering");hr(app->Signal(appTail.Get(),frame),"game tail");}
        check(synth.Begin(input.Get(),frame,displayed.Get(),frame-1),"synthesis waits capture / previous display");
        cmds.emplace_back(d);auto& b=cmds.back();
        if(frame>1)barrier(b.l.Get(),output.Get(),D3D12_RESOURCE_STATE_COPY_SOURCE,D3D12_RESOURCE_STATE_COPY_DEST);
        b.l->CopyBufferRegion(output.Get(),0,capture.Get(),0,bytes);
        barrier(b.l.Get(),output.Get(),D3D12_RESOURCE_STATE_COPY_DEST,D3D12_RESOURCE_STATE_COPY_SOURCE);b.submit(synth.Queue());
        hr(synth.Queue()->Signal(generated.Get(),frame),"generation receipt");
        check(synth.JoinOutput(generated.Get(),frame,route.Queue()),"join output to presentation, not game");
        cmds.emplace_back(d);auto& c=cmds.back();c.l->CopyBufferRegion(rb.Get(),0,output.Get(),0,bytes);c.submit(route.Queue());
        hr(route.Queue()->Signal(displayed.Get(),frame),"runtime display mark");awaitFence(displayed.Get(),frame);
        if(isolated){check(appTail->GetCompletedValue()<frame,"presentation must finish independently of blocked next game frame");hr(gate->Signal(frame),"unblock next game work");awaitFence(appTail.Get(),frame);}
        D3D12_RANGE read{0,bytes};hr(rb->Map(0,&read,&ptr),"read result");
        for(unsigned j=0;j<bytes/4;++j)check(static_cast<uint32_t*>(ptr)[j]==0xD00D0000+j,"image contents preserved");D3D12_RANGE none{};rb->Unmap(0,&none);
        // A game-written HUD image: wait availability, write, join before release.
        check(route.ToApplication(),"carry runtime image availability");
        check(route.ToRuntime(),"carry exact HUD image release");
        check(route.WaitCapture(input.Get(),frame),"real/depth image receipt");
        check(route.WaitCapture(nullptr,0),"boot capture fallback receipt");
        const auto mark=route.Mark();check(route.Drain(),"drain HUD and real copy graph");
        check(isolated?route.Ready(mark):mark==0,"whole-presentation receipt readiness");
    }
}

void runOverlaySource(ID3D12Device* d) {
    D3D12_COMMAND_QUEUE_DESC desc{};ComPtr<ID3D12CommandQueue> app;
    hr(d->CreateCommandQueue(&desc,IID_PPV_ARGS(&app)),"overlay application queue");
    cvr::xrqueue::GpuQueue route;check(route.Initialize(d,app.Get(),true)&&route.Private(),"overlay runtime binding");
    ComPtr<ID3D12Fence> gate,ready,done,tail;
    for(auto* f:{gate.GetAddressOf(),ready.GetAddressOf(),done.GetAddressOf(),tail.GetAddressOf()})
        hr(d->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(f)),"overlay fence");
    check(!route.WaitPublished(nullptr,1)&&!route.WaitPublished(ready.Get(),0),"missing overlay receipt rejected without game-tail fallback");
    constexpr unsigned bytes=256;
    auto upload=buffer(d,D3D12_HEAP_TYPE_UPLOAD,bytes,D3D12_RESOURCE_STATE_GENERIC_READ);
    auto rb=buffer(d,D3D12_HEAP_TYPE_READBACK,bytes,D3D12_RESOURCE_STATE_COPY_DEST);
    auto frame=std::make_shared<cvr::hud::Frame>();
    frame->canvas=buffer(d,D3D12_HEAP_TYPE_DEFAULT,bytes,D3D12_RESOURCE_STATE_COPY_DEST);
    frame->readyFence=ready;frame->readyValue=1;
    std::weak_ptr<cvr::hud::Frame> weak=frame;
    void* ptr{};hr(upload->Map(0,nullptr,&ptr),"overlay upload");
    for(unsigned i=0;i<bytes/4;++i)static_cast<uint32_t*>(ptr)[i]=0xFACE0000+i;
    upload->Unmap(0,nullptr);
    hr(app->Wait(gate.Get(),1),"delay canvas producer");
    Commands producer(d);producer.l->CopyBufferRegion(frame->canvas.Get(),0,upload.Get(),0,bytes);
    barrier(producer.l.Get(),frame->canvas.Get(),D3D12_RESOURCE_STATE_COPY_DEST,D3D12_RESOURCE_STATE_COPY_SOURCE);
    producer.submit(app.Get());hr(app->Signal(ready.Get(),1),"exact published canvas receipt");
    hr(app->Wait(gate.Get(),2),"block subsequent RT/NR frame");hr(app->Signal(tail.Get(),1),"next frame tail");
    check(route.WaitPublished(frame->readyFence.Get(),frame->readyValue),"runtime joins exact HUD source");
    Commands display(d);display.l->CopyBufferRegion(rb.Get(),0,frame->canvas.Get(),0,bytes);display.submit(route.Queue());
    hr(route.Queue()->Signal(done.Get(),1),"overlay copy completion");
    // Model Quad::CopySlot ownership: producer drops the publication while the
    // display retains exactly this source, until its own fence completes.
    auto inFlight=frame;frame.reset();
    check(!weak.expired() && ready->GetCompletedValue()==0 && done->GetCompletedValue()==0,"pending canvas remains pinned, unread before producer");
    hr(gate->Signal(1),"allow only the source frame");awaitFence(done.Get(),1);
    check(tail->GetCompletedValue()==0,"HUD delivery independent of next RT/NR frame");
    D3D12_RANGE read{0,bytes};hr(rb->Map(0,&read,&ptr),"overlay readback");
    for(unsigned i=0;i<bytes/4;++i)check(static_cast<uint32_t*>(ptr)[i]==0xFACE0000+i,"complete HUD pixels, no premature source read");
    D3D12_RANGE none{};rb->Unmap(0,&none);
    inFlight.reset();check(weak.expired(),"HUD source released only after consumer completion");
    hr(gate->Signal(2),"release next game frame");awaitFence(tail.Get(),1);
    cvr::xrqueue::GpuQueue shared;check(shared.Initialize(d,app.Get(),false)&&shared.WaitPublished(nullptr,0),"shared overlay path retains original queue ordering");
}

int main(){
    ComPtr<ID3D12Debug> debug;if(SUCCEEDED(D3D12GetDebugInterface(IID_PPV_ARGS(&debug))))debug->EnableDebugLayer();
    ComPtr<IDXGIFactory4> factory;hr(CreateDXGIFactory1(IID_PPV_ARGS(&factory)),"DXGI");ComPtr<ID3D12Device> d;
    for(UINT i=0;!d;++i){ComPtr<IDXGIAdapter1> a;if(factory->EnumAdapters1(i,&a)==DXGI_ERROR_NOT_FOUND)break;DXGI_ADAPTER_DESC1 desc{};a->GetDesc1(&desc);if(!(desc.Flags&DXGI_ADAPTER_FLAG_SOFTWARE))D3D12CreateDevice(a.Get(),D3D_FEATURE_LEVEL_11_0,IID_PPV_ARGS(&d));}
    check(d!=nullptr,"hardware adapter");ComPtr<ID3D12InfoQueue> info;d.As(&info);if(info)info->ClearStoredMessages();
    runPresentation(d.Get(),true);runPresentation(d.Get(),false);runOverlaySource(d.Get());
    D3D12_COMMAND_QUEUE_DESC desc{};ComPtr<ID3D12CommandQueue> app;hr(d->CreateCommandQueue(&desc,IID_PPV_ARGS(&app)),"wrapper queue");
    check(cvr::xrqueue::Initialize(d.Get(),app.Get(),true)&&cvr::xrqueue::Private(),"production binding wrapper");
    XrSwapchainImageWaitInfo wi{XR_TYPE_SWAPCHAIN_IMAGE_WAIT_INFO};XrSwapchainImageReleaseInfo ri{XR_TYPE_SWAPCHAIN_IMAGE_RELEASE_INFO};
    bool pending=false;uint32_t image{};
    waitResult=XR_TIMEOUT_EXPIRED;check(cvr::depth::WaitImage(XR_NULL_HANDLE,pending,image,200000000)==XR_TIMEOUT_EXPIRED&&pending&&acquiredImages==1,"color timeout retains acquisition");
    waitResult=XR_SUCCESS;check(cvr::depth::WaitImage(XR_NULL_HANDLE,pending,image,200000000)==XR_SUCCESS&&!pending&&image==2&&acquiredImages==1,"retry waits existing color acquisition");
    waitResult=XR_TIMEOUT_EXPIRED;check(cvr::xrqueue::WaitApplicationImage(XR_NULL_HANDLE,&wi)==XR_TIMEOUT_EXPIRED&&appWaits==0,"timeout does not transfer ownership");
    waitResult=XR_ERROR_RUNTIME_FAILURE;check(cvr::xrqueue::WaitApplicationImage(XR_NULL_HANDLE,&wi)==XR_ERROR_RUNTIME_FAILURE&&appWaits==0,"failed wait does not transfer ownership");
    waitResult=XR_SUCCESS;check(cvr::xrqueue::WaitApplicationImage(XR_NULL_HANDLE,&wi)==XR_SUCCESS&&appWaits==1,"successful HUD reacquire");
    check(cvr::xrqueue::ReleaseApplicationImage(XR_NULL_HANDLE,&ri)==XR_SUCCESS&&runtimeWaits==1&&releasedImages==1,"HUD release joined");
    cvr::xrqueue::Shutdown();check(!cvr::xrqueue::Private()&&syncFailures==0,"shutdown/wrapper errors");
    check(cvr::xrqueue::Initialize(d.Get(),app.Get(),false)&&!cvr::xrqueue::Private()&&cvr::xrqueue::Queue()==app.Get(),"VDXR shared binding unchanged");cvr::xrqueue::Shutdown();
    if(info)for(UINT64 i=0;i<info->GetNumStoredMessages();++i){SIZE_T size{};info->GetMessage(i,nullptr,&size);std::vector<char> bytes(size);auto* m=reinterpret_cast<D3D12_MESSAGE*>(bytes.data());info->GetMessage(i,m,&size);if(m->Severity<=D3D12_MESSAGE_SEVERITY_ERROR){std::cerr<<m->pDescription<<'\n';check(false,"D3D12 validation");}}
    std::cout<<"PASS: three-queue generation/display completes while next game work is blocked, 24 cycles, exact overlay receipts and pinned sources independent of blocked game tail, HUD ownership joins, wait timeout/failure, shared path, shutdown; debug="<<(info?1:0)<<'\n';
}
