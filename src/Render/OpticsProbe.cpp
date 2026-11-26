#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
#include "Render/OpticsProbe.hpp"
#include <atomic>
#include <mutex>
#include <cstring>

#if !CVR_RUST_VRIK
extern "C" {
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/used_moved_exports_rest.rs.
#else
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_OpticsProbeState{0};
__declspec(dllexport) uint32_t CyberpunkVR_OpticsProbeWidth=0;
__declspec(dllexport) uint32_t CyberpunkVR_OpticsProbeHeight=0;
__declspec(dllexport) uintptr_t CyberpunkVR_OpticsProbePixels=0;
__declspec(dllexport) uint32_t CyberpunkVR_OpticsProbeFormat=0;
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_OpticsProbeStage{0};
#endif
#if !CVR_RUST_VRIK
}
#endif

#if CVR_RUST_VRIK
// THE PROBE IS RUST'S (rust/crates/cvr-gpu/src/optics_probe.rs): the pending read, its copy, fence and readback, the
// staged render target and the pixels. This file keeps the exports above (attached to Rust), the C++ names as
// forwarders, and hands a request's texture lease over (given back through Rust when the probe is done with it).
#else

namespace cvr::optics::probe {
namespace {
using Microsoft::WRL::ComPtr;
struct Pending {
    std::shared_ptr<hud::TextureLease> texture;
    ComPtr<ID3D12CommandAllocator> allocator;
    ComPtr<ID3D12GraphicsCommandList> commands;
    ComPtr<ID3D12Resource> readback;
    ComPtr<ID3D12Fence> fence;
    D3D12_PLACED_SUBRESOURCE_FOOTPRINT layout{};
    uint64_t bytes{};
    uint32_t width{},height{},pixelBytes{4};
    bool submitted{};
    bool fencePending{};
    DXGI_FORMAT format{};
    D3D12_RESOURCE_STATES sourceState{static_cast<D3D12_RESOURCE_STATES>(0xc0)};
};
std::mutex mutex;
std::unique_ptr<Pending> pending;
std::vector<uint8_t> pixels;
thread_local ID3D12Resource* stageResource{};
thread_local ID3D12GraphicsCommandList* stageList{};
thread_local D3D12_RESOURCE_STATES stageState{};
constexpr auto reads=static_cast<D3D12_RESOURCE_STATES>(
    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE|D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
bool NativeSourceStillReadable(const hud::TextureLease& texture) {
    const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    const auto pool=*reinterpret_cast<const uintptr_t*>(base+0x3bcdca8);
    if(!pool || !texture.handle || texture.handle>131072)return false;
    const auto entry=pool+0xb0ull*(texture.handle-1);
    return *reinterpret_cast<ID3D12Resource* const*>(entry+0x2f1d8)==texture.resource.Get() &&
        *reinterpret_cast<const uint32_t*>(entry+0x2f220)==uint32_t(reads);
}
bool Record(Pending& p,ID3D12Device* device,ID3D12GraphicsCommandList* inlineList=nullptr) {
    const auto desc=p.texture->resource->GetDesc();
    if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_TEXTURE2D || desc.DepthOrArraySize!=1 ||
       desc.MipLevels!=1 || desc.SampleDesc.Count!=1 || !desc.Width || desc.Width>4096 ||
       !desc.Height || desc.Height>4096 || (desc.Format!=DXGI_FORMAT_R8G8B8A8_UNORM &&
       desc.Format!=DXGI_FORMAT_R8G8B8A8_TYPELESS && desc.Format!=DXGI_FORMAT_R8G8B8A8_UNORM_SRGB &&
       desc.Format!=DXGI_FORMAT_R11G11B10_FLOAT && desc.Format!=DXGI_FORMAT_R16G16B16A16_FLOAT &&
       desc.Format!=DXGI_FORMAT_R16G16B16A16_TYPELESS))return false;
    p.format=desc.Format;
    if(desc.Format==DXGI_FORMAT_R16G16B16A16_FLOAT || desc.Format==DXGI_FORMAT_R16G16B16A16_TYPELESS)p.pixelBytes=8;
    p.width=static_cast<uint32_t>(desc.Width);p.height=desc.Height;
    device->GetCopyableFootprints(&desc,0,1,0,&p.layout,nullptr,nullptr,&p.bytes);
    D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_READBACK;
    D3D12_RESOURCE_DESC buffer{};buffer.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;
    buffer.Width=p.bytes;buffer.Height=1;buffer.DepthOrArraySize=1;buffer.MipLevels=1;
    buffer.SampleDesc.Count=1;buffer.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    if(FAILED(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&buffer,D3D12_RESOURCE_STATE_COPY_DEST,
       nullptr,IID_PPV_ARGS(&p.readback))) ||
       FAILED(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&p.fence))))return false;
    if(!inlineList && (FAILED(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,IID_PPV_ARGS(&p.allocator))) ||
       FAILED(device->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_DIRECT,p.allocator.Get(),nullptr,IID_PPV_ARGS(&p.commands)))))return false;
    auto* commands=inlineList?inlineList:p.commands.Get();
    D3D12_RESOURCE_BARRIER barrier{};barrier.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Transition={p.texture->resource.Get(),D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,p.sourceState,D3D12_RESOURCE_STATE_COPY_SOURCE};
    commands->ResourceBarrier(1,&barrier);
    D3D12_TEXTURE_COPY_LOCATION from{};from.pResource=p.texture->resource.Get();from.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    D3D12_TEXTURE_COPY_LOCATION to{};to.pResource=p.readback.Get();to.Type=D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;to.PlacedFootprint=p.layout;
    commands->CopyTextureRegion(&to,0,0,0,&from,nullptr);
    std::swap(barrier.Transition.StateBefore,barrier.Transition.StateAfter);commands->ResourceBarrier(1,&barrier);
    return inlineList || SUCCEEDED(p.commands->Close());
}
}
bool StageRequested(){return CyberpunkVR_OpticsProbeStage.load(std::memory_order_relaxed)!=0;}
bool StageView(uint64_t key,uint64_t eyeKey) {
    const auto stage=CyberpunkVR_OpticsProbeStage.load(std::memory_order_relaxed);
    return (stage==1 && key==0x1e3f0afde930b6b1ull) || (stage==2 && key==0) ||
        (stage==3 && eyeKey && key==eyeKey);
}
void StageTarget(ID3D12GraphicsCommandList* list,ID3D12Resource* resource) {
    if(!StageRequested() || !resource)return;
    stageResource=resource;stageList=list;stageState=D3D12_RESOURCE_STATE_RENDER_TARGET;
}
void StageBarriers(ID3D12GraphicsCommandList* list,UINT count,const D3D12_RESOURCE_BARRIER* barriers) {
    if(!stageResource || list!=stageList || !barriers)return;
    for(UINT i=0;i<count;++i)if(barriers[i].Type==D3D12_RESOURCE_BARRIER_TYPE_TRANSITION &&
        barriers[i].Transition.pResource==stageResource)stageState=barriers[i].Transition.StateAfter;
}
void StageEnd(ID3D12Device* device) {
    auto* source=stageResource;auto* list=stageList;const auto state=stageState;
    stageResource=nullptr;stageList=nullptr;
    if(!source || !list || !device || !StageRequested())return;
    std::lock_guard lock(mutex);if(pending)return;
    pending=std::make_unique<Pending>();auto& p=*pending;
    p.texture=std::make_shared<hud::TextureLease>();p.texture->resource=source;p.sourceState=state;
    if(!Record(p,device,list)){pending.reset();CyberpunkVR_OpticsProbeState.store(4);}
    else {p.submitted=true;p.fencePending=true;CyberpunkVR_OpticsProbeState.store(2);}
    CyberpunkVR_OpticsProbeStage.store(0);
}
bool Request(std::shared_ptr<hud::TextureLease> texture) {
    if(!texture || !texture->resource)return false;
    std::lock_guard lock(mutex);if(pending)return false;
    pending=std::make_unique<Pending>();pending->texture=std::move(texture);
    CyberpunkVR_OpticsProbeState.store(1,std::memory_order_release);return true;
}
void Capture(ID3D12Device* device,ID3D12CommandQueue* queue) {
    if(!CyberpunkVR_OpticsProbeState.load(std::memory_order_acquire) || !device || !queue)return;
    std::lock_guard lock(mutex);if(!pending)return;
    auto& p=*pending;
    if(p.fencePending) {
        if(queue->GetDesc().Type!=D3D12_COMMAND_LIST_TYPE_DIRECT)return;
        if(FAILED(queue->Signal(p.fence.Get(),1))){CyberpunkVR_OpticsProbeState.store(4);return;}
        p.fencePending=false;return;
    }
    if(!p.submitted) {
        if(queue->GetDesc().Type!=D3D12_COMMAND_LIST_TYPE_DIRECT || !NativeSourceStillReadable(*p.texture) || !Record(p,device)) {
            pending.reset();CyberpunkVR_OpticsProbeState.store(4,std::memory_order_release);return;
        }
        ID3D12CommandList* lists[]={p.commands.Get()};queue->ExecuteCommandLists(1,lists);
        p.submitted=true;
        if(FAILED(queue->Signal(p.fence.Get(),1))) {
            // A submitted list must retain its storage even if signalling fails.
            CyberpunkVR_OpticsProbeState.store(4,std::memory_order_release);return;
        }
        CyberpunkVR_OpticsProbeState.store(2,std::memory_order_release);return;
    }
    const auto done=p.fence->GetCompletedValue();
    if(done==UINT64_MAX){pending.reset();CyberpunkVR_OpticsProbeState.store(4,std::memory_order_release);return;}
    if(done<1)return;
    void* data{};D3D12_RANGE range{0,static_cast<SIZE_T>(p.bytes)};
    if(FAILED(p.readback->Map(0,&range,&data))){pending.reset();CyberpunkVR_OpticsProbeState.store(4,std::memory_order_release);return;}
    pixels.resize(size_t(p.width)*p.height*p.pixelBytes);
    for(uint32_t y=0;y<p.height;++y)std::memcpy(pixels.data()+size_t(y)*p.width*p.pixelBytes,
        static_cast<const uint8_t*>(data)+p.layout.Offset+size_t(y)*p.layout.Footprint.RowPitch,size_t(p.width)*p.pixelBytes);
    D3D12_RANGE noWrites{};p.readback->Unmap(0,&noWrites);
    CyberpunkVR_OpticsProbeWidth=p.width;CyberpunkVR_OpticsProbeHeight=p.height;
    CyberpunkVR_OpticsProbeFormat=static_cast<uint32_t>(p.format);
    CyberpunkVR_OpticsProbePixels=reinterpret_cast<uintptr_t>(pixels.data());
    pending.reset();CyberpunkVR_OpticsProbeState.store(3,std::memory_order_release);
}
}
#endif
#endif
