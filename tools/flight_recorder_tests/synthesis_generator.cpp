#include "Framegen/SourceTimeline.hpp"
// Reuses tools/framegen_tests/main.cpp's stereo midpoint fixture with the
// production Generator and real NVIDIA/FFX libraries; no game/runtime required.
#include "Render/StereoDepth.hpp"
#include "Framegen/Generator.hpp"
#include "Framegen/CaptureTiming.hpp"
#include "Framegen/InputCopy.hpp"
#include "Framegen/FlightRecorder.hpp"
#include "Framegen/RefreshCadence.hpp"
#include <dxgi1_6.h>
#include <d3d12sdklayers.h>
#include <DirectXPackedVector.h>
#include <array>
#include <cmath>
#include <cstdio>
#include <cstdarg>
#include <cstring>
#include <stdexcept>
#include <string>
#include <vector>
#include <thread>
#include <chrono>
using Microsoft::WRL::ComPtr;
using namespace cvr::framegen;
void Log(const char* format,...){va_list a;va_start(a,format);vprintf(format,a);va_end(a);fflush(stdout);}
void Check(bool v,const char* msg){if(!v)throw std::runtime_error(msg);}
void Hr(HRESULT h){if(FAILED(h))throw std::runtime_error("GPU HRESULT "+std::to_string(unsigned(h)));}
static bool targeted=true;
static unsigned privateDispatches{},sharedDispatches{},syncFailures{};
static unsigned queueSamples{},pendingSamples{},capturePendingSamples{};
static double maxQueueMs{},maxTotalMs{},maxCaptureQueueMs{},maxBeforeReadyMs{},maxAfterReadyMs{};
static ComPtr<ID3D12Fence> testSynthesisGate;
static unsigned capturePairs{},captureInvalid{};
namespace cvr::framegen::refresh {bool Targeted(){return targeted;}}
namespace cvr::framegen::flight {
void Count(Event e){if(e==Event::SynthesisPrivate)++privateDispatches;if(e==Event::SynthesisShared)++sharedDispatches;if(e==Event::SynthesisSyncFailed)++syncFailures;if(e==Event::GenerationCapturePending)++capturePendingSamples;if(e==Event::CaptureProbePaired)++capturePairs;if(e==Event::CaptureProbeInvalid)++captureInvalid;}
void Time(Timing t,double ms){
    if(t==Timing::GenerationQueue){++queueSamples;maxQueueMs=std::max(maxQueueMs,ms);}
    if(t==Timing::GenerationTotal)maxTotalMs=std::max(maxTotalMs,ms);
    if(t==Timing::CaptureQueue)maxCaptureQueueMs=std::max(maxCaptureQueueMs,ms);
    if(t==Timing::GenerationBeforeCaptureReady)maxBeforeReadyMs=std::max(maxBeforeReadyMs,ms);
    if(t==Timing::GenerationAfterCaptureReady)maxAfterReadyMs=std::max(maxAfterReadyMs,ms);
    if(t==Timing::GenerationQueueInputsPending)++pendingSamples;
}
}
namespace cvr::framegen {
Settings settings;Statistics stats;Backend backend;
void SetSettings(Settings s){settings=s;stats={};}
bool MetricsEnabled(){return settings.overlay;}
Statistics GetStatistics(){return stats;}
void Status(const char* t,bool){Log("status: %s\n",t);}
void OnSkip(){}
void OnGeneration(double ms,double span){if(settings.overlay){stats.generationMs=ms;stats.generationSpanMs=span;}}
void PublishActiveBackend(Backend b){backend=b;}
Backend ActiveBackend(){return backend;}
void SetVram(uint64_t){}
uint64_t InputVram(){return 0;}
void CaptureFrame(ID3D12Device*,ID3D12CommandQueue* queue,ID3D12Resource* const[2],const std::shared_ptr<const Inputs> (&)[2],const EyePair (&)[2]){if(testSynthesisGate)Hr(queue->Wait(testSynthesisGate.Get(),1));}
}
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
void GpuGeneration(bool nvidia,bool engineMotion=true,bool rgbaMotion=false,bool halfMotion=false,Backend backend=Backend::FidelityFX,int flowScale=50,bool probeRun=false,bool postReadyDelay=false) {
    Gpu gpu(true);Generator generator;CaptureTimingPool capturePool;Settings settings;settings.backend=nvidia?Backend::Nvidia:Backend::FidelityFX;settings.flowScale=flowScale;
    if(backend==Backend::DlssG)settings.backend=backend;
    ComPtr<ID3D12CommandQueue> display;
    D3D12_COMMAND_QUEUE_DESC dq{};Hr(gpu.device->CreateCommandQueue(&dq,IID_PPV_ARGS(&display)));
    const bool dlss=backend==Backend::DlssG;
    // NGX's frame generation feature has a minimum size; the moving box keeps the same pixels.
    const unsigned width=dlss?512:128,height=dlss?384:96;
    std::vector<uint8_t> previousPixels[2];
    EyePair cameras[2];
    for(auto& eye:cameras)for(auto* camera:{&eye.previous,&eye.current}) {camera->fov[0]=-.8f;camera->fov[1]=.8f;camera->fov[2]=.7f;camera->fov[3]=-.7f;}
    for(unsigned frame=0;frame<8;++frame) {
        flight::enabled.store(probeRun && frame>=1);
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
            in->readyFence=gpu.fence;in->fenceValue=gpu.value;
            colors[eye]=color[eye].Get();inputs[eye]=in;
        }
        auto colorLease=std::make_shared<std::array<ComPtr<ID3D12Resource>,2>>(std::array<ComPtr<ID3D12Resource>,2>{color[0],color[1]});
        std::weak_ptr<void> weakColors=colorLease;
        // Delay a real capture receipt after all uploads and generator creation.
        // Production calibration must report the pre-execution wait, rather
        // than confusing it with the short generation shader time.
        std::jthread delayedCapture;
        std::shared_ptr<CaptureTiming> captureTiming;
        if(probeRun && frame==1){
            ComPtr<ID3D12Fence> gate;Hr(gpu.device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&gate)));
            if(postReadyDelay)testSynthesisGate=gate;
            else {Hr(gpu.queue->Wait(gate.Get(),1));Hr(gpu.queue->Signal(gpu.fence.Get(),++gpu.value));}
            delayedCapture=std::jthread([gate]{std::this_thread::sleep_for(std::chrono::milliseconds(60));gate->Signal(1);});
            captureTiming=capturePool.Next(gpu.device.Get());Check(bool(captureTiming),"capture probe allocation");
            gpu.Begin();captureTiming->Start(gpu.list.Get());captureTiming->End(gpu.list.Get());
            Hr(gpu.list->Close());ID3D12CommandList* lists[]{gpu.list.Get()};
            captureTiming->Enqueue(gpu.queue.Get());gpu.queue->ExecuteCommandLists(1,lists);
            Hr(gpu.queue->Signal(gpu.fence.Get(),++gpu.value));captureTiming->Submitted(gpu.fence.Get(),gpu.value);
            if(postReadyDelay)gpu.Wait(); // make input ready before testing synthesis backlog
            else Check(!captureTiming->Read(),"pending capture probe readback mapped too early");
        }
        // Simulate a slow previous display consumer with an actual queued receipt.
        // Release the gate on scope exit even when an assertion fails.
        struct GateRelease {ComPtr<ID3D12Fence> gate;~GateRelease(){if(gate)gate->Signal(1);}} release;
        ComPtr<ID3D12Fence> heldConsumer;
        const bool holdConsumer=targeted && nvidia && !probeRun && frame==2;
        if(holdConsumer){
            Hr(gpu.device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&release.gate)));
            Hr(gpu.device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&heldConsumer)));
            Hr(display->Wait(release.gate.Get(),1));Hr(display->Signal(heldConsumer.Get(),1));
            generator.ConsumerFence(heldConsumer.Get(),1);
        }
        const bool aheadTest=targeted && nvidia && !probeRun && frame==3;
        if(aheadTest){Hr(gpu.device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&release.gate)));testSynthesisGate=release.gate;}
        bool interpolated=false;
        Check(generator.Generate(gpu.device.Get(),gpu.queue.Get(),colors,inputs,cameras,settings,22.22,frame==0,frame+1,&interpolated,gpu.fence.Get(),gpu.value,colorLease,targeted?display.Get():nullptr,captureTiming,aheadTest),"generation failed");
        testSynthesisGate.Reset();
        if(aheadTest){
            ComPtr<ID3D12Fence> currentReal;Hr(gpu.device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&currentReal)));
            Hr(display->Signal(currentReal.Get(),1));
            const auto deadline=GetTickCount64()+1500;
            while(currentReal->GetCompletedValue()<1 && GetTickCount64()<deadline)Sleep(1);
            Check(currentReal->GetCompletedValue()==1 && !generator.OutputReady(),"deferred generation blocked current real display");
            colorLease.reset();generator.Poll();Check(!weakColors.expired(),"prepared inputs retired before GPU completion");
            Hr(release.gate->Signal(1));Check(generator.JoinOutput(display.Get()),"prepared output join failed");
            std::puts("PASS: prepared synthesis stays off current real display queue; delayed output joined only for consumption");
        }
        if(holdConsumer){
            const auto deadline=GetTickCount64()+1500;
            while(!generator.TestPackedReady() && GetTickCount64()<deadline)Sleep(1);
            Check(generator.TestPackedReady(),"packing was unnecessarily blocked by previous display consumer");
            Check(heldConsumer->GetCompletedValue()==0 && !generator.TestOutputReady(),"output escaped its previous consumer dependency");
            // Read on the independent producer queue while interpolation is blocked.
            for(unsigned eye=0;eye<2;++eye)
                Check(gpu.Read(generator.Output(eye))==previousPixels[eye],"held previous-eye output overwritten before consumer released");
            colorLease.reset();generator.Poll();Check(!weakColors.expired(),"packing-only completion retired capture lease too early");
            Check(!generator.Reset(),"packing-only completion allowed shared output/history destruction");
            Hr(release.gate->Signal(1));
            std::puts("PASS: production packing overlaps blocked consumer; both old eyes and leases retained");
        }
        auto producer=gpu.queue;
        if(targeted)gpu.queue=display;
        Check(ActiveBackend()==(settings.flowScale?settings.backend:Backend::FidelityFX),"the requested generator fell back to another one");
        Check(interpolated==(frame!=0 && !reconfigured),"priming or a changed motion resolution reported as a generated frame");gpu.Wait();
        if(!settings.overlay)Check(!MetricsEnabled() && GetStatistics().generatedFrames==0 && GetStatistics().generationMs==0,
            "generation continued collecting metrics while its overlay was off");
        if(frame>=(halfMotion?7u:6u))Check(GetStatistics().generationMs>0,"generation GPU timing did not resume after the readback ring filled");
        double centroids[2]{};
        for(unsigned eye=0;eye<2;++eye) {
            auto pixels=gpu.Read(generator.Output(eye));previousPixels[eye]=pixels;double xsum=0,weight=0;
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
        colorLease.reset();generator.Poll();
        Check(weakColors.expired(),"completed color lease did not retire");
        Check(leased[0].expired() && leased[1].expired(),"completed input leases require another generation to retire");
        gpu.queue=producer;
    }
    std::puts("Retirement begin");
    ComPtr<ID3D12Fence> held;Hr(gpu.device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&held)));
    generator.ConsumerFence(held.Get(),1);Check(!generator.Reset(),"destroyed an output while a consumer still owns it");
    if(targeted){
        std::thread release([&]{std::this_thread::sleep_for(std::chrono::milliseconds(20));Hr(held->Signal(1));});
        generator.Shutdown();release.join();Check(generator.Idle(),"private queue shutdown did not drain delayed consumer");
    } else Hr(held->Signal(1));
    Check(generator.Reset(),"idle resources did not retire");
    flight::enabled.store(false);
    std::puts("Retirement done");
}

void CrossQueueDepth() {
    Gpu gpu(true);cvr::depth::Renderer renderer;
    ComPtr<ID3D12CommandQueue> producer=gpu.queue,display;
    D3D12_COMMAND_QUEUE_DESC q{};Hr(gpu.device->CreateCommandQueue(&q,IID_PPV_ARGS(&display)));
    std::shared_ptr<const Inputs> inputs[2];ComPtr<ID3D12Resource> dest[2];ID3D12Resource* targets[2]{};
    cvr::depth::Plan plan;
    for(unsigned eye=0;eye<2;++eye){
        auto in=std::make_shared<Inputs>();in->width=in->height=32;in->queue=producer.Get();in->submitted=true;
        in->camera.nearPlane=.1f;in->camera.farPlane=100;in->camera.inverted=true;
        in->depth=gpu.Texture(32,32,DXGI_FORMAT_R32_FLOAT,D3D12_RESOURCE_STATE_COPY_DEST);
        std::vector<float> values(32*32,.25f+eye*.25f);
        gpu.Upload(in->depth.Get(),values.data(),4,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
        in->readyFence=gpu.fence;in->fenceValue=gpu.value;inputs[eye]=in;
        D3D12_HEAP_PROPERTIES hp{};hp.Type=D3D12_HEAP_TYPE_DEFAULT;
        D3D12_RESOURCE_DESC d=in->depth->GetDesc();d.Format=DXGI_FORMAT_R32_TYPELESS;d.Flags=D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL;
        Hr(gpu.device->CreateCommittedResource(&hp,D3D12_HEAP_FLAG_NONE,&d,D3D12_RESOURCE_STATE_DEPTH_WRITE,nullptr,IID_PPV_ARGS(&dest[eye])));
        targets[eye]=dest[eye].Get();plan.eyes[eye].width=plan.eyes[eye].height=32;
    }
    gpu.queue=display;gpu.Begin();
    Check(!renderer.Record(gpu.device.Get(),gpu.list.Get(),display.Get(),inputs,targets,plan,0,gpu.fence.Get(),gpu.value+1),"default depth must reject foreign queue");
    Check(renderer.Record(gpu.device.Get(),gpu.list.Get(),display.Get(),inputs,targets,plan,0,gpu.fence.Get(),gpu.value+1,true),"private depth receipt path failed");
    for(auto& d:dest){D3D12_RESOURCE_BARRIER b{};b.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;b.Transition={d.Get(),D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,D3D12_RESOURCE_STATE_DEPTH_WRITE,D3D12_RESOURCE_STATE_COPY_SOURCE};gpu.list->ResourceBarrier(1,&b);}
    gpu.Execute();
    for(unsigned eye=0;eye<2;++eye){auto bytes=gpu.Read(dest[eye].Get());for(unsigned i=0;i<32*32;++i){float value{};std::memcpy(&value,bytes.data()+i*4,4);Check(std::abs(value-(.25f+eye*.25f))<.0001f,"cross-queue depth image corrupted");}}
    renderer.Poll();Check(renderer.Idle(),"depth input lease retirement");
    std::puts("PASS: real GPU cross-queue stereo depth values and input retirement");
}

int main(){try {
    // Attribution math: same-frame ready boundary, not unrelated queue ticks.
    auto a=AttributeCaptureStart(105,100,108);Check(a && a->beforeReadyMs==5 && a->afterReadyMs==3,"split pending and post-ready delay");
    a=AttributeCaptureStart(95,100,108);Check(a && a->beforeReadyMs==0 && a->afterReadyMs==8,"already-ready capture");
    Check(!AttributeCaptureStart(109,100,108),"inconsistent cross-queue calibration rejected");
    a=AttributeCaptureStart(108.1,100,108);Check(a && a->beforeReadyMs==8 && a->afterReadyMs==0,"small calibration overlap clamped");
    Check(!AttributeCaptureStart(100,108,107),"backwards generation interval rejected");
    {
        Gpu gpu(true);CaptureTimingPool pool;std::array<std::shared_ptr<CaptureTiming>,4> pinned;
        for(auto& v:pinned){v=pool.Next(gpu.device.Get());Check(bool(v),"bounded probe pool slot");}
        Check(!pool.Next(gpu.device.Get()),"pinned probes must not allocate unbounded slots");
        auto* reusable=pinned[0].get();pinned[0].reset();Check(pool.Next(gpu.device.Get()).get()==reusable,"unrecorded probe slot reuse");
    }
    CrossQueueDepth();
    targeted=true;GpuGeneration(true);GpuGeneration(false);
    Check(privateDispatches>0 && sharedDispatches==0 && syncFailures==0,"private routing not exercised");
    targeted=false;GpuGeneration(true);
    Check(sharedDispatches>0 && syncFailures==0,"non-SteamVR native path changed");
    targeted=true;GpuGeneration(true,true,false,false,Backend::FidelityFX,50,true);
    Check(queueSamples>0 && pendingSamples>0 && capturePendingSamples>0 && maxQueueMs>15 && maxTotalMs>=maxQueueMs,
        "calibrated production timestamps did not detect delayed capture receipt");
    printf("Queue probe: samples=%u pending=%u queue=%.3fms total=%.3fms\n",queueSamples,pendingSamples,maxQueueMs,maxTotalMs);
    GpuGeneration(true,true,false,false,Backend::FidelityFX,50,true,true);
    Check(capturePairs>=2 && captureInvalid==0 && maxCaptureQueueMs>15 && maxBeforeReadyMs>15 && maxAfterReadyMs>15,
        "paired GPU timestamps failed to attribute delayed game capture");
    printf("Capture attribution: pairs=%u invalid=%u captureQueue=%.3fms generationBeforeReady=%.3fms postReady=%.3fms\n",capturePairs,captureInvalid,maxCaptureQueueMs,maxBeforeReadyMs,maxAfterReadyMs);
    printf("PASS: production OFA/FFX, both-eye midpoint, input/color leases, consumer retirement; private=%u shared=%u\n",privateDispatches,sharedDispatches);
    return 0;
} catch(const std::exception& e){fprintf(stderr,"FAIL: %s\n",e.what());return 1;}}

namespace cvr::framegen::flight {void Source(const SourceRecord&) {}}
namespace cvr::gpu {bool KeepCommandResources(ID3D12GraphicsCommandList*,std::initializer_list<IUnknown*>){return true;}}
