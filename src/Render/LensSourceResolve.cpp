#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
// Lens source resolve -- see PhysicalOpticsShaders.hpp (source::) and
// lens_source_resolve_cs.hlsl.
//
// ajax.rdc, in GPU order: the list holding the glass pass STARTS with the
// scene copy (barrier, CopyTextureRegion t32 <- scene colour), then draws the
// glass. The scene colour already rests in COPY_SOURCE|PIXEL|NON_PIXEL (2240)
// and the engine velocity in PIXEL|NON_PIXEL (192) when that list starts, so a
// list of the port's own, executed right before it, reads both as they are and
// never touches the engine's recording state. The engine list only gets one
// extra copy (the resolved image over t32) right after its own; the glass keeps
// its own shader, since that image is jittered like the frame.
#include "Render/PhysicalOpticsShaders.hpp"
#if CVR_RUST_VRIK
// The resolve is rust/crates/cvr-gpu/src/optics_source.rs; its exports (the moved exports' modules) are attached by
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
#include <mutex>
#include <vector>

extern void Log(const char*,...);
extern "C" __declspec(dllexport) std::atomic<int32_t> CyberpunkVR_OpticsSourceResolve{1};
extern "C" __declspec(dllexport) std::atomic<float> CyberpunkVR_OpticsResolveAlpha{0.2f};
extern "C" __declspec(dllexport) std::atomic<float> CyberpunkVR_OpticsResolveGamma{1.0f};
extern "C" __declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_OpticsResolveCount{};
// Scene copies registered per view side, and the last scene colour / eye-pose key seen per side.
extern "C" __declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_OpticsResolveSides[2]{};
// GPU time of one eye's resolve list (dispatch + barriers), microseconds, smoothed.
extern "C" __declspec(dllexport) std::atomic<float> CyberpunkVR_OpticsResolveMicroseconds{};
// How the view's jitter (px, y up) maps to the content shift (px, y down):
// 0 (x,-y), 1 (-x,y), 2 (x,y), 3 (-x,-y).
extern "C" __declspec(dllexport) std::atomic<int32_t> CyberpunkVR_OpticsResolveJitterMode{0};
// Clean lens (LensMask.cpp): the upscaler rejects lens history, the glass always gets the
// stabilised image, sharpened by CAS of this strength (0 = none).
extern "C" std::atomic<int32_t> CyberpunkVR_OpticsLensClean;
extern "C" __declspec(dllexport) std::atomic<float> CyberpunkVR_OpticsLensSharpen{0.35f};

namespace cvr::optics::source {
namespace {
using Microsoft::WRL::ComPtr;
constexpr DXGI_FORMAT SceneFormat=DXGI_FORMAT_R11G11B10_FLOAT,HistoryFormat=DXGI_FORMAT_R16G16B16A16_FLOAT;
constexpr UINT Ring=3,Descriptors=5;
struct Registration {
    ID3D12CommandList* list{};
    int side{};
    ComPtr<ID3D12Resource> scene,velocity;
    float jitter[2]{};
};
struct Entry {
    ComPtr<ID3D12CommandAllocator> allocator;
    ComPtr<ID3D12GraphicsCommandList> list;
    ComPtr<ID3D12DescriptorHeap> heap;
    uint64_t fence{};
    bool timed{};
};
struct Side {
    ComPtr<ID3D12Resource> history[2],output;
    UINT width{},height{},previous{};
    bool primed{};
    uint64_t tick{};
    std::array<Entry,Ring> ring;
};
struct Retired {ComPtr<ID3D12Resource> resource;uint64_t fence{};};
struct Constants {UINT width,height;float inverseWidth,inverseHeight,alpha,gamma;UINT reset,clean;float jitterX,jitterY,sharpen;};
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
    std::array<Side,2> sides;
    std::vector<Registration> pending;
    std::vector<Retired> retired;
} state;
std::atomic<unsigned> pendingCount{};
// Each eye's view jitter as the node dispatches see it, packed x|y.
std::array<std::atomic<uint64_t>,2> viewJitter{};
// The view a node renders sits at work context +0x18 (NativeInputs ViewOf);
// its scene camera descriptor's +0x370/+0x374 is its jitter in pixels, y up
// (Inputs.cpp ViewCamera, the values handed to FSR 3 as (x, -y)).
bool ReadJitter(const void* workContext,float* jitter) {
    __try {
        const auto* view=*reinterpret_cast<const uint8_t* const*>(static_cast<const uint8_t*>(workContext)+0x18);
        if(!view)return false;
        std::memcpy(jitter,view+cvr::view_layout::kCamera+0x370,8);return true;
    } __except(EXCEPTION_EXECUTE_HANDLER) {return false;}
}

bool Fail(const char* why) {
    state.failed=true;Log("[optics] Lens source resolve off: %s\n",why);return false;
}
bool Initialize(ID3D12Device* device) {
    if(state.failed)return false;
    if(state.device)return state.device==device;
    D3D12_SHADER_BYTECODE cs{};
    if(!LensSourceResolveShader(cs))return Fail("shader missing");
    D3D12_FEATURE_DATA_FORMAT_SUPPORT support{SceneFormat};
    if(FAILED(device->CheckFeatureSupport(D3D12_FEATURE_FORMAT_SUPPORT,&support,sizeof(support))) ||
       !(support.Support2&D3D12_FORMAT_SUPPORT2_UAV_TYPED_STORE))return Fail("no R11G11B10 UAV store");
    stereo::probe::Scope internal;
    if(FAILED(device->CreateRootSignature(0,cs.pShaderBytecode,cs.BytecodeLength,IID_PPV_ARGS(&state.root))))return Fail("root signature");
    D3D12_COMPUTE_PIPELINE_STATE_DESC desc{};desc.pRootSignature=state.root.Get();desc.CS=cs;
    if(FAILED(device->CreateComputePipelineState(&desc,IID_PPV_ARGS(&state.pipeline))))return Fail("pipeline");
    if(FAILED(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&state.fence))))return Fail("fence");
    state.stride=device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
    // Two timestamps per ring entry, read back once the entry comes round again.
    D3D12_QUERY_HEAP_DESC queries{D3D12_QUERY_HEAP_TYPE_TIMESTAMP,2*Ring*2,0};
    D3D12_HEAP_PROPERTIES readback{};readback.Type=D3D12_HEAP_TYPE_READBACK;
    D3D12_RESOURCE_DESC buffer{};buffer.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;buffer.Width=2*Ring*2*sizeof(uint64_t);
    buffer.Height=1;buffer.DepthOrArraySize=1;buffer.MipLevels=1;buffer.SampleDesc.Count=1;buffer.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    void* mapped{};
    if(SUCCEEDED(device->CreateQueryHeap(&queries,IID_PPV_ARGS(&state.queries))) &&
       SUCCEEDED(device->CreateCommittedResource(&readback,D3D12_HEAP_FLAG_NONE,&buffer,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&state.timestamps))) &&
       SUCCEEDED(state.timestamps->Map(0,nullptr,&mapped)))state.stamps=static_cast<const uint64_t*>(mapped);
    else {state.queries.Reset();state.timestamps.Reset();}
    state.device=device;return true;
}
bool EnsureSide(Side& side,UINT width,UINT height) {
    if(side.output && side.width==width && side.height==height)return true;
    for(auto& history:side.history)if(history)state.retired.push_back({std::move(history),state.value});
    if(side.output)state.retired.push_back({std::move(side.output),state.value});
    side.width=side.height=0;side.primed=false;side.previous=0;
    D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;
    D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D;desc.Width=width;desc.Height=height;
    desc.DepthOrArraySize=1;desc.MipLevels=1;desc.SampleDesc.Count=1;desc.Flags=D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;
    desc.Format=HistoryFormat;
    for(auto& history:side.history)
        if(FAILED(state.device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
            nullptr,IID_PPV_ARGS(&history))))return false;
    desc.Format=SceneFormat;
    if(FAILED(state.device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_COPY_SOURCE,
        nullptr,IID_PPV_ARGS(&side.output))))return false;
    side.history[0]->SetName(L"CVR lens source history 0");side.history[1]->SetName(L"CVR lens source history 1");
    side.output->SetName(L"CVR lens source resolved");
    side.width=width;side.height=height;
    Log("[optics] Lens source resolve %ux%u\n",width,height);
    return true;
}
void Transition(ID3D12GraphicsCommandList* list,ID3D12Resource* a,D3D12_RESOURCE_STATES aBefore,D3D12_RESOURCE_STATES aAfter,
                ID3D12Resource* b,D3D12_RESOURCE_STATES bBefore,D3D12_RESOURCE_STATES bAfter) {
    D3D12_RESOURCE_BARRIER barriers[2]{};
    for(auto& barrier:barriers)barrier.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barriers[0].Transition={a,D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,aBefore,aAfter};
    barriers[1].Transition={b,D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,bBefore,bAfter};
    list->ResourceBarrier(2,barriers);
}
void Resolve(ID3D12CommandQueue* queue,const Registration& r,ExecuteFn execute) {
    if(queue->GetDesc().Type!=D3D12_COMMAND_LIST_TYPE_DIRECT)return;
    ComPtr<ID3D12Device> device;
    if(FAILED(r.scene->GetDevice(IID_PPV_ARGS(&device))))return;
    std::lock_guard lock(state.mutex);
    if(!Initialize(device.Get()))return;
    const auto completed=state.fence->GetCompletedValue();
    std::erase_if(state.retired,[&](const Retired& item){return item.fence<=completed;});
    stereo::probe::Scope internal;
    auto& side=state.sides[r.side];
    const auto scene=r.scene->GetDesc();
    if(!EnsureSide(side,UINT(scene.Width),scene.Height))return;
    Entry* entry=nullptr;
    for(auto& item:side.ring)if(item.fence<=completed){entry=&item;break;}
    const UINT query=entry?UINT(2*(size_t(r.side)*Ring+size_t(entry-side.ring.data()))):0;
    if(entry && entry->timed && state.stamps) {
        uint64_t frequency{};
        if(SUCCEEDED(queue->GetTimestampFrequency(&frequency)) && frequency && state.stamps[query+1]>state.stamps[query]) {
            const float us=float(double(state.stamps[query+1]-state.stamps[query])*1e6/double(frequency));
            const float previous=CyberpunkVR_OpticsResolveMicroseconds.load(std::memory_order_relaxed);
            CyberpunkVR_OpticsResolveMicroseconds.store(previous>0?previous+0.05f*(us-previous):us,std::memory_order_relaxed);
        }
        entry->timed=false;
    }
    if(!entry){
        static std::atomic<bool> logged{};
        if(!logged.exchange(true))Log("[optics] Lens source resolve: no free command list, frame skipped\n");
        return;
    }
    if(!entry->allocator) {
        D3D12_DESCRIPTOR_HEAP_DESC heap{D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,Descriptors,D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,0};
        if(FAILED(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,IID_PPV_ARGS(&entry->allocator))) ||
           FAILED(device->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_DIRECT,entry->allocator.Get(),nullptr,IID_PPV_ARGS(&entry->list))) ||
           FAILED(entry->list->Close()) || FAILED(device->CreateDescriptorHeap(&heap,IID_PPV_ARGS(&entry->heap)))) {*entry={};return;}
    }
    if(FAILED(entry->allocator->Reset()) || FAILED(entry->list->Reset(entry->allocator.Get(),state.pipeline.Get())))return;
    const UINT next=side.previous^1u;
    const auto cpu=entry->heap->GetCPUDescriptorHandleForHeapStart();
    const auto at=[&](UINT i){return D3D12_CPU_DESCRIPTOR_HANDLE{cpu.ptr+SIZE_T(i)*state.stride};};
    D3D12_SHADER_RESOURCE_VIEW_DESC view{};view.ViewDimension=D3D12_SRV_DIMENSION_TEXTURE2D;
    view.Shader4ComponentMapping=D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;view.Texture2D.MipLevels=1;
    view.Format=SceneFormat;device->CreateShaderResourceView(r.scene.Get(),&view,at(0));
    view.Format=DXGI_FORMAT_R16G16_FLOAT;device->CreateShaderResourceView(r.velocity.Get(),&view,at(1));
    view.Format=HistoryFormat;device->CreateShaderResourceView(side.history[side.previous].Get(),&view,at(2));
    D3D12_UNORDERED_ACCESS_VIEW_DESC uav{};uav.ViewDimension=D3D12_UAV_DIMENSION_TEXTURE2D;
    uav.Format=HistoryFormat;device->CreateUnorderedAccessView(side.history[next].Get(),nullptr,&uav,at(3));
    uav.Format=SceneFormat;device->CreateUnorderedAccessView(side.output.Get(),nullptr,&uav,at(4));
    const auto now=GetTickCount64();
    const Constants constants{side.width,side.height,1.0f/float(side.width),1.0f/float(side.height),
        std::clamp(CyberpunkVR_OpticsResolveAlpha.load(std::memory_order_relaxed),0.02f,1.0f),
        std::clamp(CyberpunkVR_OpticsResolveGamma.load(std::memory_order_relaxed),0.25f,4.0f),
        UINT(!side.primed || now-side.tick>250),UINT(mask::Active(r.side)),r.jitter[0],r.jitter[1],
        std::clamp(CyberpunkVR_OpticsLensSharpen.load(std::memory_order_relaxed),0.0f,1.0f)};
    auto* list=entry->list.Get();
    if(state.queries)list->EndQuery(state.queries.Get(),D3D12_QUERY_TYPE_TIMESTAMP,query);
    Transition(list,side.history[next].Get(),D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
               side.output.Get(),D3D12_RESOURCE_STATE_COPY_SOURCE,D3D12_RESOURCE_STATE_UNORDERED_ACCESS);
    ID3D12DescriptorHeap* heaps[]{entry->heap.Get()};list->SetDescriptorHeaps(1,heaps);
    list->SetComputeRootSignature(state.root.Get());
    list->SetComputeRoot32BitConstants(0,sizeof(constants)/4,&constants,0);
    list->SetComputeRootDescriptorTable(1,entry->heap->GetGPUDescriptorHandleForHeapStart());
    list->Dispatch((side.width+7)/8,(side.height+7)/8,1);
    Transition(list,side.history[next].Get(),D3D12_RESOURCE_STATE_UNORDERED_ACCESS,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
               side.output.Get(),D3D12_RESOURCE_STATE_UNORDERED_ACCESS,D3D12_RESOURCE_STATE_COPY_SOURCE);
    if(state.queries) {
        list->EndQuery(state.queries.Get(),D3D12_QUERY_TYPE_TIMESTAMP,query+1);
        list->ResolveQueryData(state.queries.Get(),D3D12_QUERY_TYPE_TIMESTAMP,query,2,state.timestamps.Get(),uint64_t(query)*sizeof(uint64_t));
        entry->timed=true;
    }
    if(FAILED(list->Close()))return;
    ID3D12CommandList* lists[]{list};execute(queue,1,lists);
    if(FAILED(queue->Signal(state.fence.Get(),++state.value)))return;
    entry->fence=state.value;
    side.previous=next;side.primed=true;side.tick=now;
    CyberpunkVR_OpticsResolveCount.fetch_add(1,std::memory_order_relaxed);
}
}

ID3D12Resource* SceneCopied(ID3D12GraphicsCommandList* list,int side,const D3D12_TEXTURE_COPY_LOCATION* dst,
    UINT x,UINT y,UINT z,const D3D12_TEXTURE_COPY_LOCATION* src,const D3D12_BOX* box) {
    if(!list || side<0 || side>1 || !dst || !src || box || x || y || z ||
       !CyberpunkVR_OpticsSourceResolve.load(std::memory_order_relaxed) || !GlassRecent() ||
       list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT)return nullptr;
    if(dst->Type!=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX || src->Type!=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX ||
       dst->SubresourceIndex || src->SubresourceIndex || !dst->pResource || !src->pResource)return nullptr;
    const auto d=dst->pResource->GetDesc(),s=src->pResource->GetDesc();
    constexpr auto writable=D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET|D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL|D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;
    if(d.Dimension!=D3D12_RESOURCE_DIMENSION_TEXTURE2D || d.Format!=SceneFormat || d.MipLevels!=1 || d.DepthOrArraySize!=1 ||
       d.SampleDesc.Count!=1 || (d.Flags&writable) || s.Dimension!=D3D12_RESOURCE_DIMENSION_TEXTURE2D || s.Format!=SceneFormat ||
       s.Width!=d.Width || s.Height!=d.Height || !(s.Flags&D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET))return nullptr;
    auto velocity=MotionVectors();
    if(!velocity)return nullptr;
    const auto v=velocity->GetDesc();
    if(v.Width!=d.Width || v.Height!=d.Height)return nullptr;
    std::lock_guard lock(state.mutex);
    if(state.failed)return nullptr;
    std::erase_if(state.pending,[&](const Registration& item){return item.list==list;});
    if(state.pending.size()>=16)return nullptr;
    Registration r{list,side,src->pResource,std::move(velocity)};
    CyberpunkVR_OpticsResolveSides[side].fetch_add(1,std::memory_order_relaxed);
    // Content shift in pixels, y down: raw pixel p shows the world at p - shift.
    const auto packed=viewJitter[side].load(std::memory_order_relaxed);
    float view[2]{};std::memcpy(view,&packed,8);
    const auto mode=CyberpunkVR_OpticsResolveJitterMode.load(std::memory_order_relaxed);
    r.jitter[0]=(mode==1 || mode==3)?-view[0]:view[0];r.jitter[1]=(mode==0 || mode==3)?-view[1]:view[1];
    static std::atomic<int> logged{};
    if(logged.fetch_add(1)<4)Log("[optics] Lens source resolve side %d jitter %.4f %.4f px\n",side,r.jitter[0],r.jitter[1]);
    auto& target=state.sides[side];
    ID3D12Resource* output=nullptr;
    if(target.primed && target.output && target.width==d.Width && target.height==d.Height)output=target.output.Get();
    state.pending.push_back(std::move(r));
    pendingCount.store(unsigned(state.pending.size()),std::memory_order_release);
    return output;
}
namespace {std::atomic<int> stabilize{80};}
void SetStabilize(int percent) {
    percent=std::clamp(percent,0,100);stabilize.store(percent,std::memory_order_relaxed);
    CyberpunkVR_OpticsSourceResolve.store(percent>0,std::memory_order_relaxed);
    if(percent>0)CyberpunkVR_OpticsResolveAlpha.store((std::max)(0.05f,1.0f-percent*0.01f),std::memory_order_relaxed);
}
int GetStabilize() {return stabilize.load(std::memory_order_relaxed);}
void ViewDispatched(int side,const void* workContext) {
    if(side<0 || side>1 || !workContext || !CyberpunkVR_OpticsSourceResolve.load(std::memory_order_relaxed) || !GlassRecent())return;
    float jitter[2]{};
    if(!ReadJitter(workContext,jitter) || !std::isfinite(jitter[0]) || !std::isfinite(jitter[1]) ||
       std::abs(jitter[0])>2 || std::abs(jitter[1])>2)return;
    uint64_t packed{};std::memcpy(&packed,jitter,8);viewJitter[side].store(packed,std::memory_order_relaxed);
}
void ListReset(ID3D12GraphicsCommandList* list) {
    // The resolve's own lists reset through the same hook, under its lock.
    if(stereo::probe::internalCommands || !pendingCount.load(std::memory_order_acquire))return;
    std::lock_guard lock(state.mutex);
    std::erase_if(state.pending,[&](const Registration& item){return item.list==list;});
    pendingCount.store(unsigned(state.pending.size()),std::memory_order_release);
}
void Execute(ID3D12CommandQueue* queue,UINT n,ID3D12CommandList* const* lists,ExecuteFn execute) {
    if(!pendingCount.load(std::memory_order_acquire) || !queue || !lists){execute(queue,n,lists);return;}
    UINT start=0;
    for(UINT i=0;i<n;++i) {
        Registration r;
        {
            std::lock_guard lock(state.mutex);
            const auto it=std::find_if(state.pending.begin(),state.pending.end(),[&](const Registration& item){return item.list==lists[i];});
            if(it==state.pending.end())continue;
            r=std::move(*it);state.pending.erase(it);
            pendingCount.store(unsigned(state.pending.size()),std::memory_order_release);
        }
        if(i>start)execute(queue,i-start,lists+start);
        start=i;
        Resolve(queue,r,execute);
    }
    if(n>start)execute(queue,n-start,lists+start);
}
}
#endif
#endif
