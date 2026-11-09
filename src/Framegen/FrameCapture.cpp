#include "Framegen/FrameCapture.hpp"
#include "Core/Paths.hpp"
#include <atomic>
#include <algorithm>
#include <cstdio>
#include <ctime>
#include <filesystem>
#include <mutex>
#include <string>
#include <thread>
#include <vector>

extern void Log(const char*,...);
extern bool OverlayIsVisible();
extern "C" __declspec(dllexport) std::atomic<int32_t> CyberpunkVR_FramegenCapture{0};
namespace cvr::framegen {
namespace {
using Microsoft::WRL::ComPtr;
struct Image {
    ComPtr<ID3D12Resource> readback;
    D3D12_PLACED_SUBRESOURCE_FOOTPRINT footprint{};
    UINT rows{};UINT64 rowBytes{};
    uint32_t width{},height{};DXGI_FORMAT format{};
};
struct Frame {
    Image color[2],depth[2],motion[2];
    CameraData camera[2]{};
    EyeCamera eye[2]{};
    uint32_t frameId{};
    // The head-pose sample each eye was drawn from (Inputs::poseId), and when the frame was captured (QPC): two frames
    // with one pose sample are the port reusing a pose; two samples with one orientation are the runtime's.
    uint64_t poseId[2]{};
    int64_t capturedQpc{};
};
struct State {
    std::mutex mutex;
    ID3D12Device* device{};
    ComPtr<ID3D12CommandAllocator> allocator;
    ComPtr<ID3D12GraphicsCommandList> list;
    ComPtr<ID3D12Fence> fence;
    uint64_t value{};
    std::vector<Frame> frames;
    bool writing{};
    uint64_t due{},expires{};
    Settings settings{};
} state;

void Barrier(ID3D12GraphicsCommandList* list,ID3D12Resource* resource,D3D12_RESOURCE_STATES a,D3D12_RESOURCE_STATES b) {
    D3D12_RESOURCE_BARRIER v{};v.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    v.Transition={resource,D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,a,b};list->ResourceBarrier(1,&v);
}
bool Copy(ID3D12Device* device,ID3D12GraphicsCommandList* list,ID3D12Resource* source,Image& image) {
    const auto desc=source->GetDesc();UINT64 bytes{};
    device->GetCopyableFootprints(&desc,0,1,0,&image.footprint,&image.rows,&image.rowBytes,&bytes);
    D3D12_RESOURCE_DESC buffer{};buffer.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;buffer.Width=bytes;buffer.Height=1;
    buffer.DepthOrArraySize=1;buffer.MipLevels=1;buffer.SampleDesc.Count=1;buffer.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_READBACK;
    if(FAILED(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&buffer,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&image.readback))))return false;
    D3D12_TEXTURE_COPY_LOCATION from{},to{};from.pResource=source;from.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    to.pResource=image.readback.Get();to.Type=D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;to.PlacedFootprint=image.footprint;
    list->CopyTextureRegion(&to,0,0,0,&from,nullptr);
    image.width=uint32_t(desc.Width);image.height=desc.Height;image.format=desc.Format;return true;
}
bool Write(const Image& image,const std::filesystem::path& path) {
    void* data{};if(FAILED(image.readback->Map(0,nullptr,&data)))return false;
    FILE* file=_wfopen(path.c_str(),L"wb");bool ok=file!=nullptr;
    for(UINT row=0;ok && row<image.rows;++row)
        ok=std::fwrite(static_cast<const uint8_t*>(data)+image.footprint.Offset+size_t(row)*image.footprint.Footprint.RowPitch,1,size_t(image.rowBytes),file)==size_t(image.rowBytes);
    if(file)std::fclose(file);
    D3D12_RANGE none{};image.readback->Unmap(0,&none);return ok;
}
void Floats(FILE* f,const char* name,const float* v,unsigned n,bool comma=true) {
    std::fprintf(f,"\"%s\":[",name);for(unsigned i=0;i<n;++i)std::fprintf(f,"%s%.9g",i?",":"",v[i]);std::fprintf(f,"]%s",comma?",":"");
}
void WriteAll(std::vector<Frame> frames,ComPtr<ID3D12Fence> fence,uint64_t value,Settings settings) {
    HANDLE event=CreateEventW(nullptr,FALSE,FALSE,nullptr);
    if(fence->GetCompletedValue()<value && event) {fence->SetEventOnCompletion(value,event);WaitForSingleObject(event,10000);}
    if(event)CloseHandle(event);
    bool ok=fence->GetCompletedValue()>=value;
    char stamp[32];const std::time_t now=std::time(nullptr);std::tm local{};localtime_s(&local,&now);std::strftime(stamp,sizeof(stamp),"%Y%m%d-%H%M%S",&local);
    const auto directory=std::filesystem::path(cvr::paths::Dir(cvr::paths::Folder::Log))/L"framegen-capture"/(std::string(stamp)+"-"+std::to_string(GetTickCount64()));
    std::error_code error;std::filesystem::create_directories(directory,error);ok=ok && !error;
    FILE* json=ok?_wfopen((directory/L"capture.json").c_str(),L"w"):nullptr;ok=json!=nullptr;
    if(ok)std::fprintf(json,"{\"schema\":2,\"backend\":%d,\"quality\":%d,\"flowScale\":%d,\"qpcFrequency\":%lld,\"frames\":[\n",
        int(settings.backend),settings.quality,settings.flowScale,[](){LARGE_INTEGER f{};QueryPerformanceFrequency(&f);return f.QuadPart;}());
    for(size_t k=0;ok && k<frames.size();++k) {
        const auto& frame=frames[k];
        std::fprintf(json,"%s{\"frame\":%u,\"eyes\":[",k?",\n":"",frame.frameId);
        for(unsigned e=0;e<2 && ok;++e) {
            char name[64];
            std::snprintf(name,sizeof(name),"f%zu_e%u_color.raw",k,e);ok=Write(frame.color[e],directory/name);
            std::snprintf(name,sizeof(name),"f%zu_e%u_depth.raw",k,e);ok=ok && Write(frame.depth[e],directory/name);
            std::snprintf(name,sizeof(name),"f%zu_e%u_motion.raw",k,e);ok=ok && (!frame.motion[e].readback || Write(frame.motion[e],directory/name));
            const auto& c=frame.camera[e];const auto& v=frame.eye[e];
            std::fprintf(json,"%s{\"color\":[%u,%u,%d],\"depth\":[%u,%u,%d],\"motion\":[%u,%u,%d],",e?",":"",
                frame.color[e].width,frame.color[e].height,int(frame.color[e].format),frame.depth[e].width,frame.depth[e].height,int(frame.depth[e].format),
                frame.motion[e].width,frame.motion[e].height,int(frame.motion[e].format));
            Floats(json,"jitter",c.jitter,2);Floats(json,"motionScale",c.motionScale,2);Floats(json,"position",c.position,3);
            Floats(json,"up",c.up,3);Floats(json,"right",c.right,3);Floats(json,"forward",c.forward,3);
            const float planes[4]={c.nearPlane,c.farPlane,c.verticalFov,c.aspect};Floats(json,"planes",planes,4);
            std::fprintf(json,"\"inverted\":%d,\"infinite\":%d,\"jittered\":%d,\"pose\":%llu,\"qpc\":%lld,",c.inverted,c.infinite,c.jittered,
                static_cast<unsigned long long>(frame.poseId[e]),static_cast<long long>(frame.capturedQpc));
            Floats(json,"orientation",v.orientation,4);Floats(json,"fov",v.fov,4,false);std::fprintf(json,"}");
        }
        std::fprintf(json,"]}");
    }
    if(json) {std::fprintf(json,"\n]}\n");std::fclose(json);}
    Log("[framegen] capture of %zu frames %s: %ls\n",frames.size(),ok?"written":"FAILED",directory.c_str());
    {std::lock_guard lock(state.mutex);
        // Timeout must not release allocations the GPU may still be writing.
        if(fence->GetCompletedValue()<value)state.frames=std::move(frames);
        state.writing=false;CyberpunkVR_FramegenCapture.store(ok?2:-1);
    }
}
}


bool ScheduleFrameCapture(unsigned delaySeconds) {
    std::lock_guard lock(state.mutex);
    if(state.writing || CyberpunkVR_FramegenCapture.load()==1 ||
       (state.fence && state.fence->GetCompletedValue()<state.value))return false;
    state.frames.clear();state.settings=GetSettings();
    state.due=GetTickCount64()+uint64_t(std::min(delaySeconds,60u))*1000;
    state.expires=state.due+60000;
    CyberpunkVR_FramegenCapture.store(1);
    Log("[framegen] image capture armed; delay=%us, close F10 and continue moving\n",delaySeconds);
    return true;
}
void CancelFrameCapture() {
    std::lock_guard lock(state.mutex);
    if(!state.writing){CyberpunkVR_FramegenCapture.store(0);state.due=state.expires=0;}
}
void FrameCaptureStatus(char* text,size_t capacity,bool* busy) {
    std::lock_guard lock(state.mutex);
    auto status=CyberpunkVR_FramegenCapture.load();const auto now=GetTickCount64();
    if(status==1 && !state.writing && state.expires && now>=state.expires){
        CyberpunkVR_FramegenCapture.store(-1);status=-1;
        Log("[framegen] image capture timed out waiting for consecutive gameplay inputs\n");
    }
    if(busy)*busy=status==1 || state.writing;
    if(!text || !capacity)return;
    if(state.writing)std::snprintf(text,capacity,"Saving images...");
    else if(status==1 && state.due>now)std::snprintf(text,capacity,"Starts in %llu s; close F10 and ride",(state.due-now+999)/1000);
    else if(status==1)std::snprintf(text,capacity,"Waiting for gameplay: %zu/3 consecutive frames",state.frames.size());
    else if(status==2)std::snprintf(text,capacity,"Saved in cyberpunkvrport/log/framegen-capture");
    else if(status<0)std::snprintf(text,capacity,"Capture failed or timed out; see cyberpunkvrport.log");
    else std::snprintf(text,capacity,"Idle (no image recording)");
}

void CaptureFrame(ID3D12Device* device,ID3D12CommandQueue* queue,ID3D12Resource* const colors[2],
    const std::shared_ptr<const Inputs> (&inputs)[2],const EyePair (&cameras)[2]) {
    if(CyberpunkVR_FramegenCapture.load(std::memory_order_relaxed)!=1)return;
    std::lock_guard lock(state.mutex);
    // Recheck under the lock: cancellation/rearming can race this call.
    if(CyberpunkVR_FramegenCapture.load()!=1)return;
    const auto now=GetTickCount64();
    if(state.expires && now>=state.expires && !state.writing){CyberpunkVR_FramegenCapture.store(-1);return;}
    if(state.due && (now<state.due || OverlayIsVisible()))return;
    if(state.writing || !device || !queue || !colors[0] || !colors[1] || !inputs[0] || !inputs[1])return;
    // A real frame shown again (no newer one yet) is no break in the sequence.
    if(!state.frames.empty() && inputs[0]->frameId==state.frames.back().frameId)return;
    // The frames kept so far are released only once their copies are done. A frame that does not follow the last
    // one restarts the sequence, and clearing it first freed readbacks the GPU was still writing (TAA 2026-10-24:
    // device removed, a page fault on the copy's write).
    if(state.device==device && state.value && state.fence->GetCompletedValue()<state.value)return;
    if(!state.frames.empty() && inputs[0]->frameId!=state.frames.back().frameId+1)state.frames.clear();
    if(state.device!=device) {
        state.device=device;state.frames.clear();state.list.Reset();state.allocator.Reset();state.fence.Reset();state.value=0;
        if(FAILED(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,IID_PPV_ARGS(&state.allocator))) ||
           FAILED(device->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_DIRECT,state.allocator.Get(),nullptr,IID_PPV_ARGS(&state.list))) ||
           FAILED(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&state.fence)))) {state.device=nullptr;CyberpunkVR_FramegenCapture.store(-1);return;}
        state.list->Close();
    }
    if(FAILED(state.allocator->Reset()) || FAILED(state.list->Reset(state.allocator.Get(),nullptr)))return;
    if(state.frames.empty())state.settings=GetSettings();
    Frame frame;frame.frameId=inputs[0]->frameId;bool ok=true;
    constexpr auto read=D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;constexpr auto copy=D3D12_RESOURCE_STATE_COPY_SOURCE;
    for(unsigned e=0;e<2 && ok;++e) {
        const auto& in=*inputs[e];
        frame.camera[e]=in.camera;frame.eye[e]=cameras[e].current;frame.poseId[e]=in.poseId;
        if(e==0) {LARGE_INTEGER now{};QueryPerformanceCounter(&now);frame.capturedQpc=now.QuadPart;}
        ok=Copy(device,state.list.Get(),colors[e],frame.color[e]);   // the captures rest in COPY_SOURCE
        // Motion is optional: without frame generation it is not captured (the stereo check needs depth only).
        const bool motion=in.hasMotion && in.motion;
        if(!in.depth){ok=false;break;}
        Barrier(state.list.Get(),in.depth.Get(),read,copy);if(motion)Barrier(state.list.Get(),in.motion.Get(),read,copy);
        ok=ok && Copy(device,state.list.Get(),in.depth.Get(),frame.depth[e]) && (!motion || Copy(device,state.list.Get(),in.motion.Get(),frame.motion[e]));
        Barrier(state.list.Get(),in.depth.Get(),copy,read);if(motion)Barrier(state.list.Get(),in.motion.Get(),copy,read);
    }
    if(FAILED(state.list->Close()) || !ok) {CyberpunkVR_FramegenCapture.store(-1);state.frames.clear();return;}
    ID3D12CommandList* lists[]={state.list.Get()};queue->ExecuteCommandLists(1,lists);queue->Signal(state.fence.Get(),++state.value);
    state.frames.push_back(std::move(frame));
    if(state.frames.size()<3)return;
    state.writing=true;
    std::thread(WriteAll,std::move(state.frames),state.fence,state.value,state.settings).detach();
    state.frames.clear();
}
}
