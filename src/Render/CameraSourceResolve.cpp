#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
// Camera-scope source resolve -- see PhysicalOpticsShaders.hpp (camera::) and
// camera_source_resolve_cs.hlsl.
//
// The scope's RTT view ends with its CopyToTexture node (0x3FE078), which
// writes the camera image into the DynamicTexture the scope UI displays. That
// node records on its own thread; the target it binds and the transitions it
// (and the rest of its list) records give the image and the state it is left
// in when the list ends. Right after that list is submitted the port runs a
// list of its own on the same queue: the image is read where it rests,
// accumulated with the reprojected history, and the result copied back over it
// -- before anything that samples it can run. The engine's lists are never
// touched.
#include "Render/PhysicalOpticsShaders.hpp"
#if CVR_RUST_VRIK
// The resolve is rust/crates/cvr-gpu/src/optics_camera.rs; its exports (the moved exports' modules) are attached by
// rust/crates/cvr-render/src/export_attach.rs as the plugin loads. Nothing of this file remains in the Rust build.
#else
#include "Render/RenderProbeScope.hpp"
#include "Render/ViewCameraLayout.hpp"
#include <windows.h>
#include <wrl/client.h>
#include <algorithm>
#include <array>
#include <atomic>
#include <cmath>
#include <cstring>
#include <memory>
#include <mutex>
#include <vector>

extern void Log(const char*,...);
extern "C" __declspec(dllexport) std::atomic<int32_t> CyberpunkVR_OpticsCameraResolve{1};
extern "C" __declspec(dllexport) std::atomic<float> CyberpunkVR_OpticsCameraAlpha{0.12f};
extern "C" __declspec(dllexport) std::atomic<float> CyberpunkVR_OpticsCameraMotionAlpha{0.35f};
extern "C" __declspec(dllexport) std::atomic<float> CyberpunkVR_OpticsCameraGamma{1.25f};
extern "C" __declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_OpticsCameraResolveCount{};
// Copy nodes that found no target, and resolves refused for an unobserved state.
extern "C" __declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_OpticsCameraResolveMisses[2]{};
extern "C" __declspec(dllexport) std::atomic<float> CyberpunkVR_OpticsCameraResolveMicroseconds{};

namespace cvr::optics::camera {
namespace {
using Microsoft::WRL::ComPtr;
constexpr UINT Ring=3,Descriptors=3,Unknown=UINT_MAX;
constexpr DXGI_FORMAT HistoryFormat=DXGI_FORMAT_R16G16B16A16_FLOAT;
struct Camera {
    float right[3]{},up[3]{},forward[3]{};
    float tanX{},tanY{};
    bool valid{};
};
struct Capture {
    ID3D12GraphicsCommandList* list{};
    ComPtr<ID3D12Resource> target;
    std::atomic<UINT> state{Unknown};   // the target's state as the list leaves it
    Camera camera;
};
struct Entry {
    ComPtr<ID3D12CommandAllocator> allocator;
    ComPtr<ID3D12GraphicsCommandList> list;
    ComPtr<ID3D12DescriptorHeap> heap;
    uint64_t fence{};
    bool timed{};
};
struct Retired {ComPtr<ID3D12Resource> resource;uint64_t fence{};};
struct Constants {
    UINT width,height;float inverseWidth,inverseHeight;
    float alpha,motionAlpha,gamma;UINT reset;
    float rows[3][4];
};
static_assert(sizeof(Constants)==20*4);
struct State {
    std::mutex mutex;
    ID3D12Device* device{};
    ComPtr<ID3D12RootSignature> root;
    ComPtr<ID3D12PipelineState> pipeline;
    ComPtr<ID3D12Fence> fence;
    ComPtr<ID3D12QueryHeap> queries;
    ComPtr<ID3D12Resource> timestamps;
    const uint64_t* stamps{};
    uint64_t value{};
    UINT stride{};
    bool failed{};
    ComPtr<ID3D12Resource> history[2];
    ID3D12Resource* source{};           // identity only: a new camera texture restarts the history
    UINT width{},height{},previous{};
    bool primed{};
    uint64_t tick{};
    Camera last;
    std::array<Entry,Ring> ring;
    std::vector<std::shared_ptr<Capture>> pending;
    std::vector<Retired> retired;
} state;
std::atomic<unsigned> pendingCount{};

// The copy node's recording thread.
struct NodeTransition {ID3D12Resource* resource;UINT after;};
thread_local bool t_inNode{};
thread_local Camera t_camera;
thread_local ID3D12GraphicsCommandList* t_list{};
thread_local ComPtr<ID3D12Resource> t_target;
thread_local std::array<NodeTransition,16> t_transitions{};
thread_local unsigned t_transitionCount{};
// The state the node's write implies: an RTV is drawn to in RENDER_TARGET, a
// copy destination is written in COPY_DEST. Not a guess -- D3D12 allows no
// other -- and a transition the node records after the write overrides it.
thread_local UINT t_written{};
// The camera image last captured. Every later transition of it, on any thread,
// is matched to the pending capture of its list, so the state the list leaves
// it in stays exact however the engine records the rest of that list.
std::atomic<ID3D12Resource*> watched{};
std::atomic<int> watchLog{};

float Length(const float* v) {return std::sqrt(v[0]*v[0]+v[1]*v[1]+v[2]*v[2]);}
float Dot(const float* a,const float* b) {return a[0]*b[0]+a[1]*b[1]+a[2]*b[2];}
// The view a node renders sits at work context +0x18; its camera basis and
// projection are the fields Framegen/Inputs.cpp ViewCamera hands to FSR 3 and
// DLSS-G: its scene camera descriptor (Render/ViewCameraLayout.hpp) -- right/forward/up
// +0x250/+0x260/+0x270, vertical fov +0x20 degrees, aspect +0x28.
bool ReadCamera(const void* workContext,Camera& camera) {
    camera={};
    __try {
        const auto* view=*reinterpret_cast<const uint8_t* const*>(static_cast<const uint8_t*>(workContext)+0x18);
        if(!view)return false;
        float degrees{},aspect{};
        const auto* d=view+cvr::view_layout::kCamera;
        std::memcpy(camera.right,d+0x250,12);std::memcpy(camera.forward,d+0x260,12);std::memcpy(camera.up,d+0x270,12);
        std::memcpy(&degrees,d+0x20,4);std::memcpy(&aspect,d+0x28,4);
        if(!std::isfinite(degrees) || !std::isfinite(aspect) || degrees<0.5f || degrees>170.0f || aspect<0.1f || aspect>10.0f)return false;
        for(const float* v:{camera.right,camera.up,camera.forward})
            if(!std::isfinite(v[0]) || !std::isfinite(v[1]) || !std::isfinite(v[2]) || std::abs(Length(v)-1.0f)>0.05f)return false;
        if(std::abs(Dot(camera.right,camera.forward))>0.05f || std::abs(Dot(camera.up,camera.forward))>0.05f)return false;
        camera.tanY=std::tan(degrees*0.5f*3.14159265f/180.0f);camera.tanX=camera.tanY*aspect;
        camera.valid=true;return true;
    } __except(EXCEPTION_EXECUTE_HANDLER) {camera={};return false;}
}
bool CameraImage(ID3D12Resource* resource) {
    if(!resource)return false;
    const auto d=resource->GetDesc();
    return d.Dimension==D3D12_RESOURCE_DIMENSION_TEXTURE2D && d.MipLevels==1 && d.DepthOrArraySize==1 && d.SampleDesc.Count==1 &&
        (d.Format==DXGI_FORMAT_R16G16B16A16_FLOAT || d.Format==DXGI_FORMAT_R16G16B16A16_TYPELESS) &&
        d.Width>=64 && d.Width<=4096 && d.Height>=64 && d.Height<=4096;
}

bool Fail(const char* why) {state.failed=true;Log("[optics] Camera source resolve off: %s\n",why);return false;}
bool Initialize(ID3D12Device* device) {
    if(state.failed)return false;
    if(state.device)return state.device==device;
    D3D12_SHADER_BYTECODE cs{};
    if(!CameraSourceResolveShader(cs))return Fail("shader missing");
    stereo::probe::Scope internal;
    if(FAILED(device->CreateRootSignature(0,cs.pShaderBytecode,cs.BytecodeLength,IID_PPV_ARGS(&state.root))))return Fail("root signature");
    D3D12_COMPUTE_PIPELINE_STATE_DESC desc{};desc.pRootSignature=state.root.Get();desc.CS=cs;
    if(FAILED(device->CreateComputePipelineState(&desc,IID_PPV_ARGS(&state.pipeline))))return Fail("pipeline");
    if(FAILED(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&state.fence))))return Fail("fence");
    state.stride=device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
    D3D12_QUERY_HEAP_DESC queries{D3D12_QUERY_HEAP_TYPE_TIMESTAMP,2*Ring,0};
    D3D12_HEAP_PROPERTIES readback{};readback.Type=D3D12_HEAP_TYPE_READBACK;
    D3D12_RESOURCE_DESC buffer{};buffer.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;buffer.Width=2*Ring*sizeof(uint64_t);
    buffer.Height=1;buffer.DepthOrArraySize=1;buffer.MipLevels=1;buffer.SampleDesc.Count=1;buffer.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    void* mapped{};
    if(SUCCEEDED(device->CreateQueryHeap(&queries,IID_PPV_ARGS(&state.queries))) &&
       SUCCEEDED(device->CreateCommittedResource(&readback,D3D12_HEAP_FLAG_NONE,&buffer,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&state.timestamps))) &&
       SUCCEEDED(state.timestamps->Map(0,nullptr,&mapped)))state.stamps=static_cast<const uint64_t*>(mapped);
    else {state.queries.Reset();state.timestamps.Reset();}
    state.device=device;return true;
}
bool EnsureHistory(UINT width,UINT height) {
    if(state.history[0] && state.width==width && state.height==height)return true;
    for(auto& history:state.history)if(history)state.retired.push_back({std::move(history),state.value});
    state.width=state.height=0;state.primed=false;state.previous=0;
    D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;
    D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D;desc.Width=width;desc.Height=height;
    desc.DepthOrArraySize=1;desc.MipLevels=1;desc.SampleDesc.Count=1;desc.Flags=D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;
    desc.Format=HistoryFormat;
    for(auto& history:state.history)
        if(FAILED(state.device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
            nullptr,IID_PPV_ARGS(&history))))return false;
    state.history[0]->SetName(L"CVR camera scope history 0");state.history[1]->SetName(L"CVR camera scope history 1");
    state.width=width;state.height=height;
    Log("[optics] Camera source resolve %ux%u\n",width,height);
    return true;
}
// Current NDC (x, y, 1) -> previous homogeneous NDC, rotation only: a ray
// through the current pixel in world space, expressed in the previous camera.
void Reprojection(const Camera& previous,const Camera& current,float rows[3][4]) {
    const float* p[3]={previous.right,previous.up,previous.forward};
    const float ps[3]={1.0f/previous.tanX,1.0f/previous.tanY,1.0f};
    const float* c[3]={current.right,current.up,current.forward};
    const float cs[3]={current.tanX,current.tanY,1.0f};
    for(int i=0;i<3;++i) {
        for(int j=0;j<3;++j)rows[i][j]=Dot(p[i],c[j])*ps[i]*cs[j];
        rows[i][3]=0.0f;
    }
}
void Transition(std::vector<D3D12_RESOURCE_BARRIER>& out,ID3D12Resource* resource,UINT before,UINT after) {
    if(before==after)return;
    D3D12_RESOURCE_BARRIER barrier{};barrier.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Transition={resource,D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,D3D12_RESOURCE_STATES(before),D3D12_RESOURCE_STATES(after)};
    out.push_back(barrier);
}
void Flush(ID3D12GraphicsCommandList* list,std::vector<D3D12_RESOURCE_BARRIER>& barriers) {
    if(!barriers.empty())list->ResourceBarrier(UINT(barriers.size()),barriers.data());
    barriers.clear();
}
void Resolve(ID3D12CommandQueue* queue,const Capture& capture,source::ExecuteFn execute) {
    if(queue->GetDesc().Type!=D3D12_COMMAND_LIST_TYPE_DIRECT)return;
    const UINT resting=capture.state.load(std::memory_order_acquire);
    // Never a guessed StateBefore for a texture the engine owns.
    if(resting==Unknown) {
        CyberpunkVR_OpticsCameraResolveMisses[1].fetch_add(1,std::memory_order_relaxed);return;
    }
    ComPtr<ID3D12Device> device;
    if(FAILED(capture.target->GetDevice(IID_PPV_ARGS(&device))))return;
    std::lock_guard lock(state.mutex);
    if(!Initialize(device.Get()))return;
    const auto completed=state.fence->GetCompletedValue();
    std::erase_if(state.retired,[&](const Retired& item){return item.fence<=completed;});
    stereo::probe::Scope internal;
    const auto target=capture.target->GetDesc();
    if(!EnsureHistory(UINT(target.Width),target.Height))return;
    Entry* entry=nullptr;
    for(auto& item:state.ring)if(item.fence<=completed){entry=&item;break;}
    if(!entry)return;
    const UINT query=UINT(2*(entry-state.ring.data()));
    if(entry->timed && state.stamps) {
        uint64_t frequency{};
        if(SUCCEEDED(queue->GetTimestampFrequency(&frequency)) && frequency && state.stamps[query+1]>state.stamps[query]) {
            const float us=float(double(state.stamps[query+1]-state.stamps[query])*1e6/double(frequency));
            const float previous=CyberpunkVR_OpticsCameraResolveMicroseconds.load(std::memory_order_relaxed);
            CyberpunkVR_OpticsCameraResolveMicroseconds.store(previous>0?previous+0.05f*(us-previous):us,std::memory_order_relaxed);
        }
        entry->timed=false;
    }
    if(!entry->allocator) {
        D3D12_DESCRIPTOR_HEAP_DESC heap{D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,Descriptors,D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,0};
        if(FAILED(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,IID_PPV_ARGS(&entry->allocator))) ||
           FAILED(device->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_DIRECT,entry->allocator.Get(),nullptr,IID_PPV_ARGS(&entry->list))) ||
           FAILED(entry->list->Close()) || FAILED(device->CreateDescriptorHeap(&heap,IID_PPV_ARGS(&entry->heap)))) {*entry={};return;}
    }
    if(FAILED(entry->allocator->Reset()) || FAILED(entry->list->Reset(entry->allocator.Get(),state.pipeline.Get())))return;
    const UINT next=state.previous^1u;
    const auto cpu=entry->heap->GetCPUDescriptorHandleForHeapStart();
    const auto at=[&](UINT i){return D3D12_CPU_DESCRIPTOR_HANDLE{cpu.ptr+SIZE_T(i)*state.stride};};
    D3D12_SHADER_RESOURCE_VIEW_DESC view{};view.ViewDimension=D3D12_SRV_DIMENSION_TEXTURE2D;
    view.Shader4ComponentMapping=D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;view.Texture2D.MipLevels=1;view.Format=HistoryFormat;
    device->CreateShaderResourceView(capture.target.Get(),&view,at(0));
    device->CreateShaderResourceView(state.history[state.previous].Get(),&view,at(1));
    D3D12_UNORDERED_ACCESS_VIEW_DESC uav{};uav.ViewDimension=D3D12_UAV_DIMENSION_TEXTURE2D;uav.Format=HistoryFormat;
    device->CreateUnorderedAccessView(state.history[next].Get(),nullptr,&uav,at(2));
    const auto now=GetTickCount64();
    const bool reset=!state.primed || now-state.tick>250 || state.source!=capture.target.Get() || !state.last.valid || !capture.camera.valid;
    Constants constants{state.width,state.height,1.0f/float(state.width),1.0f/float(state.height),
        std::clamp(CyberpunkVR_OpticsCameraAlpha.load(std::memory_order_relaxed),0.02f,1.0f),
        std::clamp(CyberpunkVR_OpticsCameraMotionAlpha.load(std::memory_order_relaxed),0.02f,1.0f),
        std::clamp(CyberpunkVR_OpticsCameraGamma.load(std::memory_order_relaxed),0.25f,4.0f),UINT(reset),{}};
    if(!reset)Reprojection(state.last,capture.camera,constants.rows);
    auto* list=entry->list.Get();
    if(state.queries)list->EndQuery(state.queries.Get(),D3D12_QUERY_TYPE_TIMESTAMP,query);
    // Read the image where it rests when it is already a shader resource.
    const UINT read=(resting&D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE)?resting:UINT(D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    std::vector<D3D12_RESOURCE_BARRIER> barriers;barriers.reserve(4);
    Transition(barriers,capture.target.Get(),resting,read);
    Transition(barriers,state.history[next].Get(),D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,D3D12_RESOURCE_STATE_UNORDERED_ACCESS);
    Flush(list,barriers);
    ID3D12DescriptorHeap* heaps[]{entry->heap.Get()};list->SetDescriptorHeaps(1,heaps);
    list->SetComputeRootSignature(state.root.Get());
    list->SetComputeRoot32BitConstants(0,sizeof(constants)/4,&constants,0);
    list->SetComputeRootDescriptorTable(1,entry->heap->GetGPUDescriptorHandleForHeapStart());
    list->Dispatch((state.width+7)/8,(state.height+7)/8,1);
    Transition(barriers,state.history[next].Get(),D3D12_RESOURCE_STATE_UNORDERED_ACCESS,D3D12_RESOURCE_STATE_COPY_SOURCE);
    Transition(barriers,capture.target.Get(),read,D3D12_RESOURCE_STATE_COPY_DEST);
    Flush(list,barriers);
    list->CopyResource(capture.target.Get(),state.history[next].Get());
    Transition(barriers,capture.target.Get(),D3D12_RESOURCE_STATE_COPY_DEST,resting);
    Transition(barriers,state.history[next].Get(),D3D12_RESOURCE_STATE_COPY_SOURCE,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    Flush(list,barriers);
    if(state.queries) {
        list->EndQuery(state.queries.Get(),D3D12_QUERY_TYPE_TIMESTAMP,query+1);
        list->ResolveQueryData(state.queries.Get(),D3D12_QUERY_TYPE_TIMESTAMP,query,2,state.timestamps.Get(),uint64_t(query)*sizeof(uint64_t));
        entry->timed=true;
    }
    if(FAILED(list->Close()))return;
    ID3D12CommandList* lists[]{list};execute(queue,1,lists);
    if(FAILED(queue->Signal(state.fence.Get(),++state.value)))return;
    entry->fence=state.value;
    state.previous=next;state.primed=true;state.tick=now;state.last=capture.camera;state.source=capture.target.Get();
    CyberpunkVR_OpticsCameraResolveCount.fetch_add(1,std::memory_order_relaxed);
}
}

bool NodeActive() {return t_inNode;}
void CopyNodeBegin(const void* workContext) {
    t_inNode=false;t_list=nullptr;t_target.Reset();t_transitionCount=0;t_written=Unknown;
    if(!workContext || !CyberpunkVR_OpticsCameraResolve.load(std::memory_order_relaxed) || state.failed)return;
    ReadCamera(workContext,t_camera);
    t_inNode=true;
}
void CopyNodeEnd() {
    if(!t_inNode)return;
    t_inNode=false;
    auto target=std::move(t_target);auto* list=t_list;t_list=nullptr;
    if(!target || !list || list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT) {
        const auto misses=CyberpunkVR_OpticsCameraResolveMisses[0].fetch_add(1,std::memory_order_relaxed);
        if(misses<2)Log("[optics] Camera source resolve: copy node bound no camera image (%s)\n",target?"not a direct list":"no target");
        return;
    }
    UINT resting=t_written;
    for(unsigned i=0;i<t_transitionCount;++i)if(t_transitions[i].resource==target.Get())resting=t_transitions[i].after;
    auto capture=std::make_shared<Capture>();
    capture->list=list;capture->target=std::move(target);capture->state.store(resting,std::memory_order_relaxed);capture->camera=t_camera;
    {
        std::lock_guard lock(state.mutex);
        std::erase_if(state.pending,[&](const std::shared_ptr<Capture>& item){return item->list==list;});
        if(state.pending.size()>=8)return;
        state.pending.push_back(capture);
        pendingCount.store(unsigned(state.pending.size()),std::memory_order_release);
    }
    static std::atomic<int> logged{};
    if(logged.fetch_add(1)<3) {
        const auto d=capture->target->GetDesc();
        Log("[optics] Camera source resolve: image %llux%u format %d state 0x%X camera %s (tan %.4f x %.4f)\n",
            static_cast<unsigned long long>(d.Width),d.Height,int(d.Format),resting,capture->camera.valid?"valid":"invalid",
            capture->camera.tanX,capture->camera.tanY);
    }
    watched.store(capture->target.Get(),std::memory_order_release);
}
void TargetBound(ID3D12GraphicsCommandList* list,ID3D12Resource* resource) {
    if(!t_inNode || !CameraImage(resource))return;
    t_target=resource;t_list=list;t_written=D3D12_RESOURCE_STATE_RENDER_TARGET;
    for(unsigned i=0;i<t_transitionCount;++i)if(t_transitions[i].resource==resource)t_transitions[i].resource=nullptr;
}
void Copied(ID3D12GraphicsCommandList* list,ID3D12Resource* destination) {
    if(!t_inNode || !CameraImage(destination))return;
    t_target=destination;t_list=list;t_written=D3D12_RESOURCE_STATE_COPY_DEST;
    for(unsigned i=0;i<t_transitionCount;++i)if(t_transitions[i].resource==destination)t_transitions[i].resource=nullptr;
}
void Barriers(ID3D12GraphicsCommandList* list,UINT count,const D3D12_RESOURCE_BARRIER* barriers) {
    auto* const image=watched.load(std::memory_order_acquire);
    if(!barriers || (!t_inNode && !image))return;
    for(UINT i=0;i<count;++i) {
        const auto& b=barriers[i];
        if(b.Type!=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION || (b.Flags&D3D12_RESOURCE_BARRIER_FLAG_BEGIN_ONLY))continue;
        auto* resource=b.Transition.pResource;
        if(t_inNode) {
            bool found=false;
            for(unsigned k=0;k<t_transitionCount;++k)
                if(t_transitions[k].resource==resource){t_transitions[k].after=b.Transition.StateAfter;found=true;break;}
            if(!found && t_transitionCount<t_transitions.size())t_transitions[t_transitionCount++]={resource,UINT(b.Transition.StateAfter)};
        }
        if(resource!=image || !resource)continue;
        bool matched=false;
        if(pendingCount.load(std::memory_order_acquire)) {
            std::lock_guard lock(state.mutex);
            for(auto& item:state.pending)if(item->list==list && item->target.Get()==resource) {
                item->state.store(b.Transition.StateAfter,std::memory_order_release);matched=true;
            }
        }
        if(watchLog.fetch_add(1,std::memory_order_relaxed)<12)
            Log("[optics] Camera image transition 0x%X -> 0x%X list %p thread %lu in-node %d pending-list %d\n",
                UINT(b.Transition.StateBefore),UINT(b.Transition.StateAfter),static_cast<void*>(list),GetCurrentThreadId(),int(t_inNode),int(matched));
    }
}
void ListReset(ID3D12GraphicsCommandList* list) {
    if(stereo::probe::internalCommands || !pendingCount.load(std::memory_order_acquire))return;
    std::lock_guard lock(state.mutex);
    std::erase_if(state.pending,[&](const std::shared_ptr<Capture>& item){return item->list==list;});
    pendingCount.store(unsigned(state.pending.size()),std::memory_order_release);
}
void Execute(ID3D12CommandQueue* queue,UINT n,ID3D12CommandList* const* lists,source::ExecuteFn execute) {
    if(!pendingCount.load(std::memory_order_acquire) || !queue || !lists){source::Execute(queue,n,lists,execute);return;}
    UINT start=0;
    for(UINT i=0;i<n;++i) {
        std::shared_ptr<Capture> capture;
        {
            std::lock_guard lock(state.mutex);
            const auto it=std::find_if(state.pending.begin(),state.pending.end(),[&](const auto& item){return item->list==lists[i];});
            if(it==state.pending.end())continue;
            capture=std::move(*it);state.pending.erase(it);
            pendingCount.store(unsigned(state.pending.size()),std::memory_order_release);
        }
        source::Execute(queue,i+1-start,lists+start,execute);
        start=i+1;
        if(CyberpunkVR_OpticsCameraResolve.load(std::memory_order_relaxed))Resolve(queue,*capture,execute);
    }
    if(n>start)source::Execute(queue,n-start,lists+start,execute);
}
}
#endif
#endif
