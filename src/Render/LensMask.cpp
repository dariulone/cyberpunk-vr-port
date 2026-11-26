#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
// Upscaler history mask of the scope image -- see PhysicalOpticsShaders.hpp (mask::)
// and lens_mask_ps.hlsl.
//
// One R8 mask per eye at the velocity target's size, kept in RENDER_TARGET for
// its whole life. Right after each glass draw (and, on the camera path, each
// camera-surface draw) the port redraws the same geometry into it with 1. When
// that eye's velocity is tagged for DLSS, the mask goes with it as
// kBufferTypeBiasCurrentColorHint, lifecycle eOnlyValidNow: Streamline copies
// it on the tagging list right away, so the port clears it on that same list
// after the tag and the next frame starts empty. A mask nothing drew into since
// the last tag is not tagged.
#include "Render/PhysicalOpticsShaders.hpp"
#if CVR_RUST_VRIK
// The Rust build: the mask is rust/crates/cvr-gpu/src/optics.rs. The exports stay here (attached at load), and so does
// SetTag's front, which hands Rust the list's unhooked ClearRenderTargetView.
#include "Stereo/StereoInternal.hpp"
#include <atomic>
#if CVR_RUST_VRIK
// The switch and counters are Rust's (used_moved_exports_rest.rs), attached by export_attach.rs as the plugin loads.
#else
extern "C" __declspec(dllexport) std::atomic<int32_t> CyberpunkVR_OpticsLensClean{1};
extern "C" __declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_OpticsLensMaskDraws[2]{};
extern "C" __declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_OpticsLensMaskTags[2]{};
extern "C" void cvr_optics_mask_attach(std::atomic<int32_t>*,std::atomic<uint64_t>*,std::atomic<uint64_t>*);
namespace {
const bool attached=(cvr_optics_mask_attach(&CyberpunkVR_OpticsLensClean,CyberpunkVR_OpticsLensMaskDraws,CyberpunkVR_OpticsLensMaskTags),true);
}
#endif
#else
#include "Render/RenderProbeScope.hpp"
#include "Stereo/StereoInternal.hpp"
#include <sl_core_types.h>
#include <windows.h>
#include <wrl/client.h>
#include <array>
#include <atomic>
#include <mutex>
#include <vector>

extern void Log(const char*,...);
// 1: the upscaler takes the current frame inside the scope image (and the lens
// source resolve hands the glass its stabilised image always). 0: off.
extern "C" __declspec(dllexport) std::atomic<int32_t> CyberpunkVR_OpticsLensClean{1};
// Per eye: mask redraws recorded, masks tagged for DLSS.
extern "C" __declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_OpticsLensMaskDraws[2]{};
extern "C" __declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_OpticsLensMaskTags[2]{};

namespace cvr::optics::mask {
namespace {
using Microsoft::WRL::ComPtr;
struct Side {
    ComPtr<ID3D12Resource> mask;
    ComPtr<ID3D12DescriptorHeap> heap;
    D3D12_CPU_DESCRIPTOR_HANDLE rtv{};
    UINT64 width{};
    UINT height{};
    std::atomic<bool> dirty{};
    std::atomic<uint64_t> tagged{};   // GetTickCount64 of the last tag
};
struct Retired {ComPtr<ID3D12Resource> mask;ComPtr<ID3D12DescriptorHeap> heap;uint64_t tick{};};
std::mutex mutex;
std::array<Side,2> sides;
std::vector<Retired> retired;
std::atomic<bool> refused{};   // Streamline rejected the mask tag once: never sent again

bool ReadVelocityTag(const sl::ResourceTag* tag,ID3D12Resource** resource,sl::Extent* extent) {
    __try {
        if(tag->structType!=sl::ResourceTag::s_structType || tag->structVersion!=1 || tag->type!=sl::kBufferTypeMotionVectors ||
           !tag->resource || tag->resource->structType!=sl::Resource::s_structType || tag->resource->structVersion!=1 ||
           tag->resource->type!=sl::ResourceType::eTex2d || !tag->resource->native)return false;
        *resource=static_cast<ID3D12Resource*>(tag->resource->native);*extent=tag->extent;return true;
    } __except(EXCEPTION_EXECUTE_HANDLER) {return false;}
}
// Creates (or resizes) the eye's mask; true when it is new and still holds garbage.
bool Ensure(Side& side,ID3D12Resource* velocity,bool& created) {
    created=false;
    const auto desc=velocity->GetDesc();
    if(side.mask && side.width==desc.Width && side.height==desc.Height)return true;
    ComPtr<ID3D12Device> device;
    if(FAILED(velocity->GetDevice(IID_PPV_ARGS(&device))))return false;
    const auto now=GetTickCount64();
    std::erase_if(retired,[&](const Retired& item){return now-item.tick>3000;});
    if(side.mask)retired.push_back({std::move(side.mask),std::move(side.heap),now});
    side.mask.Reset();side.heap.Reset();side.width=0;side.height=0;
    D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;
    D3D12_RESOURCE_DESC m{};m.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D;m.Width=desc.Width;m.Height=desc.Height;
    m.DepthOrArraySize=1;m.MipLevels=1;m.SampleDesc.Count=1;m.Format=DXGI_FORMAT_R8_UNORM;m.Flags=D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
    D3D12_CLEAR_VALUE clear{};clear.Format=DXGI_FORMAT_R8_UNORM;
    D3D12_DESCRIPTOR_HEAP_DESC rtvHeap{D3D12_DESCRIPTOR_HEAP_TYPE_RTV,1,D3D12_DESCRIPTOR_HEAP_FLAG_NONE,0};
    stereo::probe::Scope internal;
    if(FAILED(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&m,D3D12_RESOURCE_STATE_RENDER_TARGET,&clear,IID_PPV_ARGS(&side.mask))) ||
       FAILED(device->CreateDescriptorHeap(&rtvHeap,IID_PPV_ARGS(&side.heap)))) {side.mask.Reset();side.heap.Reset();return false;}
    side.mask->SetName(L"CVR scope history mask");
    side.rtv=side.heap->GetCPUDescriptorHandleForHeapStart();
    D3D12_RENDER_TARGET_VIEW_DESC view{};view.Format=DXGI_FORMAT_R8_UNORM;view.ViewDimension=D3D12_RTV_DIMENSION_TEXTURE2D;
    device->CreateRenderTargetView(side.mask.Get(),&view,side.rtv);
    side.width=desc.Width;side.height=desc.Height;side.dirty.store(false,std::memory_order_relaxed);
    created=true;
    Log("[optics] Scope history mask %llux%u\n",static_cast<unsigned long long>(desc.Width),desc.Height);
    return true;
}
void Clear(ID3D12GraphicsCommandList* list,const Side& side) {
    // The driver's own entry: the port's and the stage replay's observers never see it.
    const auto* entry=cvr::detail::command_list_hook_entry(list);
    static constexpr FLOAT zero[4]{};
    stereo::probe::Scope internal;
    if(entry && entry->clear_rtv_original)entry->clear_rtv_original(list,side.rtv,zero,0,nullptr);
    else list->ClearRenderTargetView(side.rtv,zero,0,nullptr);
}
}

bool Enabled() {return CyberpunkVR_OpticsLensClean.load(std::memory_order_relaxed)!=0 && !refused.load(std::memory_order_relaxed);}
bool Active(int side) {
    return Enabled() && side>=0 && side<=1 && GetTickCount64()-sides[size_t(side)].tagged.load(std::memory_order_relaxed)<500;
}
bool Target(int side,ComPtr<ID3D12Resource>& resource,D3D12_CPU_DESCRIPTOR_HANDLE& rtv) {
    if(!Enabled() || side<0 || side>1)return false;
    std::lock_guard lock(mutex);
    auto& s=sides[size_t(side)];
    if(!s.mask)return false;
    resource=s.mask;rtv=s.rtv;return true;
}
void Drawn(int side) {
    if(side<0 || side>1)return;
    sides[size_t(side)].dirty.store(true,std::memory_order_release);
    CyberpunkVR_OpticsLensMaskDraws[side].fetch_add(1,std::memory_order_relaxed);
}
uint32_t SetTag(const void* viewport,const void* tags,uint32_t count,void* commandList,SetTagFn original,int side) {
    if(!original)return 1;
    if(!Enabled() || side<0 || side>1 || !tags || !count || count>=32 || !commandList)return original(viewport,tags,count,commandList);
    const auto* in=static_cast<const sl::ResourceTag*>(tags);
    ID3D12Resource* velocity{};sl::Extent extent{};
    for(uint32_t i=0;i<count && !velocity;++i)if(!ReadVelocityTag(in+i,&velocity,&extent))velocity=nullptr;
    if(!velocity)return original(viewport,tags,count,commandList);
    auto* list=static_cast<ID3D12GraphicsCommandList*>(commandList);
    std::lock_guard lock(mutex);
    auto& s=sides[size_t(side)];
    bool created{};
    if(!Ensure(s,velocity,created))return original(viewport,tags,count,commandList);
    if(created || !s.dirty.exchange(false,std::memory_order_acq_rel)) {
        const auto result=original(viewport,tags,count,commandList);
        if(created)Clear(list,s);
        return result;
    }
    std::array<sl::ResourceTag,32> extended;
    for(uint32_t i=0;i<count;++i)extended[i]=in[i];
    sl::Resource resource(sl::ResourceType::eTex2d,s.mask.Get(),uint32_t(D3D12_RESOURCE_STATE_RENDER_TARGET));
    extended[count]=sl::ResourceTag(&resource,sl::kBufferTypeBiasCurrentColorHint,sl::ResourceLifecycle::eOnlyValidNow,&extent);
    const auto result=original(viewport,extended.data(),count+1,commandList);
    if(result!=0) {
        // An older Streamline that does not know the hint: tag as the game did, mask off for good.
        refused.store(true,std::memory_order_relaxed);
        Log("[optics] Streamline refused the scope history mask (result %u); mask off\n",result);
        return original(viewport,tags,count,commandList);
    }
    // Streamline copied the volatile tag on this list; the next frame starts empty.
    Clear(list,s);
    s.tagged.store(GetTickCount64(),std::memory_order_relaxed);
    CyberpunkVR_OpticsLensMaskTags[side].fetch_add(1,std::memory_order_relaxed);
    return result;
}
}
#endif
#endif
