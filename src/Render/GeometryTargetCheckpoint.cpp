#if !CVR_RUST_VRIK
// The C++ build's checkpoint; the Rust build's is rust/crates/cvr-gpu/src/geometry_checkpoint.rs (the header calls it).
#include "Render/GeometryTargetCheckpoint.hpp"
#include "Render/CommandResources.hpp"
#include "Render/RenderProbeScope.hpp"
#include <atomic>
#include <new>
#include <utility>

namespace cvr::stereo {
using Microsoft::WRL::ComPtr;
namespace {
constexpr std::array<DXGI_FORMAT,5> Formats{DXGI_FORMAT_R10G10B10A2_UNORM,DXGI_FORMAT_R10G10B10A2_UNORM,
    DXGI_FORMAT_R8G8B8A8_UNORM,DXGI_FORMAT_R16G16_FLOAT,DXGI_FORMAT_D32_FLOAT_S8X24_UINT};
std::atomic<uint64_t> nextIdentity{1};
uint64_t Identity(){
    auto id=nextIdentity.load(std::memory_order_relaxed);
    do{if(id==UINT64_MAX)return 0;}while(!nextIdentity.compare_exchange_weak(id,id+1,std::memory_order_relaxed));
    return id;
}
bool Selected(UINT slot,UINT colors){return slot==4 || slot<colors;}
bool SameDevice(ID3D12Device* device,ID3D12DeviceChild* child){
    ComPtr<ID3D12Device> actual;return child && SUCCEEDED(child->GetDevice(IID_PPV_ARGS(&actual))) && actual.Get()==device;
}
D3D12_RESOURCE_STATES Writable(UINT slot){return slot==4?D3D12_RESOURCE_STATE_DEPTH_WRITE:D3D12_RESOURCE_STATE_RENDER_TARGET;}
bool Validate(ID3D12Device* device,const scene_state::GeometryTargets& targets,std::array<D3D12_RESOURCE_DESC,5>& descs){
    if(!device || (targets.colors!=0 && targets.colors!=3 && targets.colors!=4))return false;
    UINT64 width{};UINT height{};
    for(UINT slot=0;slot<5;++slot)if(Selected(slot,targets.colors)){
        auto* source=targets.resources[slot].Get();const auto& view=targets.views[slot];
        if(!SameDevice(device,source) || !targets.handles[slot].ptr || !view.identity || view.resource!=reinterpret_cast<uintptr_t>(source) || view.viewFlags)return false;
        const auto desc=source->GetDesc();
        if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_TEXTURE2D || !desc.Width || !desc.Height || desc.Width>4096 || desc.Height>4096 ||
            desc.DepthOrArraySize!=1 || desc.MipLevels!=1 || desc.SampleDesc.Count!=1 || desc.SampleDesc.Quality || desc.Layout!=D3D12_TEXTURE_LAYOUT_UNKNOWN ||
            view.width!=desc.Width || view.height!=desc.Height || view.format!=UINT(desc.Format) || view.resourceFlags!=UINT(desc.Flags) ||
            view.viewFormat!=UINT(Formats[slot]) || (desc.Format!=Formats[slot] && !(slot==4 && desc.Format==DXGI_FORMAT_R32G8X24_TYPELESS)) ||
            !(desc.Flags&(slot==4?D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL:D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET)))return false;
        if(width && (width!=desc.Width || height!=desc.Height))return false;
        width=desc.Width;height=desc.Height;
        for(UINT previous=0;previous<slot;++previous)if(Selected(previous,targets.colors) && targets.resources[previous].Get()==source)return false;
        descs[slot]=desc;
    }
    return true;
}
}
struct GeometryTargetCheckpoint::Storage final:IUnknown {
    std::atomic<ULONG> refs{1};ComPtr<ID3D12Device> device;
    std::array<D3D12_RESOURCE_DESC,5> shape;
    scene_state::GeometryTargets targets,native;
    ComPtr<ID3D12DescriptorHeap> rtvs,dsv;
    bool captured{};
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** out)override{if(!out)return E_POINTER;*out=nullptr;if(id!=__uuidof(IUnknown))return E_NOINTERFACE;*out=this;AddRef();return S_OK;}
    ULONG STDMETHODCALLTYPE AddRef()override{return ++refs;}
    ULONG STDMETHODCALLTYPE Release()override{const auto n=--refs;if(!n)delete this;return n;}
};
GeometryTargetCheckpoint::GeometryTargetCheckpoint()=default;
GeometryTargetCheckpoint::~GeometryTargetCheckpoint()=default;
HRESULT GeometryTargetCheckpoint::Create(ID3D12Device* device,const scene_state::GeometryTargets& shape)try{
    if(storage_)return E_UNEXPECTED;
    std::array<D3D12_RESOURCE_DESC,5> descs{};if(!Validate(device,shape,descs))return E_INVALIDARG;
    ComPtr<Storage> s;s.Attach(new Storage);s->device=device;s->shape=descs;s->targets.colors=shape.colors;
    probe::Scope internal;HRESULT hr{};
    if(shape.colors){const D3D12_DESCRIPTOR_HEAP_DESC heap{D3D12_DESCRIPTOR_HEAP_TYPE_RTV,shape.colors,D3D12_DESCRIPTOR_HEAP_FLAG_NONE,0};
        hr=device->CreateDescriptorHeap(&heap,IID_PPV_ARGS(&s->rtvs));if(FAILED(hr))return hr;}
    const D3D12_DESCRIPTOR_HEAP_DESC depthHeap{D3D12_DESCRIPTOR_HEAP_TYPE_DSV,1,D3D12_DESCRIPTOR_HEAP_FLAG_NONE,0};
    hr=device->CreateDescriptorHeap(&depthHeap,IID_PPV_ARGS(&s->dsv));if(FAILED(hr))return hr;
    D3D12_HEAP_PROPERTIES memory{};memory.Type=D3D12_HEAP_TYPE_DEFAULT;
    const UINT stride=device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV);
    for(UINT slot=0;slot<5;++slot)if(Selected(slot,shape.colors)){
        auto desc=descs[slot];desc.Alignment=0;desc.Flags=slot==4?D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL:D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
        auto& resource=s->targets.resources[slot];
        hr=device->CreateCommittedResource(&memory,D3D12_HEAP_FLAG_NONE,&desc,Writable(slot),nullptr,IID_PPV_ARGS(&resource));if(FAILED(hr))return hr;
        auto& handle=s->targets.handles[slot];
        if(slot==4){handle=s->dsv->GetCPUDescriptorHandleForHeapStart();D3D12_DEPTH_STENCIL_VIEW_DESC view{};
            view.Format=Formats[slot];view.ViewDimension=D3D12_DSV_DIMENSION_TEXTURE2D;device->CreateDepthStencilView(resource.Get(),&view,handle);
        }else{handle={s->rtvs->GetCPUDescriptorHandleForHeapStart().ptr+SIZE_T(slot)*stride};D3D12_RENDER_TARGET_VIEW_DESC view{};
            view.Format=Formats[slot];view.ViewDimension=D3D12_RTV_DIMENSION_TEXTURE2D;device->CreateRenderTargetView(resource.Get(),&view,handle);}
        const auto identity=Identity();if(!identity)return E_UNEXPECTED;
        s->targets.views[slot]={identity,reinterpret_cast<uintptr_t>(resource.Get()),desc.Width,desc.Height,
            UINT(desc.Format),UINT(Formats[slot]),UINT(desc.Flags),0,0};
    }
    storage_=std::move(s);return S_OK;
}catch(const std::bad_alloc&){return E_OUTOFMEMORY;}
bool GeometryTargetCheckpoint::Capture(ID3D12GraphicsCommandList* list,const scene_state::GeometryTargets& native){
    if(!storage_ || !list || storage_->captured || list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT || !SameDevice(storage_->device.Get(),list))return false;
    auto& s=*storage_.Get();std::array<D3D12_RESOURCE_DESC,5> descs{};
    if(native.colors!=s.targets.colors || !Validate(s.device.Get(),native,descs))return false;
    for(UINT slot=0;slot<5;++slot)if(Selected(slot,native.colors)){
        const auto& expected=s.shape[slot];const auto& actual=descs[slot];
        if(actual.Width!=expected.Width || actual.Height!=expected.Height || actual.Format!=expected.Format || actual.Flags!=expected.Flags)return false;
        for(const auto& destination:s.targets.resources)if(destination.Get()==native.resources[slot].Get())return false;
    }
    if(!gpu::KeepCommandResources(list,{&s}))return false;
    s.native=native;
    std::array<D3D12_RESOURCE_BARRIER,10> barriers{};UINT count{};
    for(UINT slot=0;slot<5;++slot)if(Selected(slot,native.colors)){
        auto& source=barriers[count++];source.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        source.Transition={native.resources[slot].Get(),D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,Writable(slot),D3D12_RESOURCE_STATE_COPY_SOURCE};
        auto& destination=barriers[count++];destination.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        destination.Transition={s.targets.resources[slot].Get(),D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,Writable(slot),D3D12_RESOURCE_STATE_COPY_DEST};
    }
    probe::Scope internal;list->ResourceBarrier(count,barriers.data());
    for(UINT slot=0;slot<5;++slot)if(Selected(slot,native.colors))list->CopyResource(s.targets.resources[slot].Get(),native.resources[slot].Get());
    for(UINT i=0;i<count;++i)std::swap(barriers[i].Transition.StateBefore,barriers[i].Transition.StateAfter);
    list->ResourceBarrier(count,barriers.data());s.captured=true;return true;
}
bool GeometryTargetCheckpoint::Captured()const{return storage_ && storage_->captured;}
const scene_state::GeometryTargets& GeometryTargetCheckpoint::Targets()const{
    static const scene_state::GeometryTargets empty;return Captured()?storage_->targets:empty;
}
}
#endif
