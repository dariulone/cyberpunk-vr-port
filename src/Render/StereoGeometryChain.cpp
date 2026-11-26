#include "Render/StereoGeometryChain.hpp"
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's chain is geometry_chain.rs' (no StereoGeometryChain is made)
// THE CHAIN IS RUST'S (rust/crates/cvr-render/src/geometry_chain.rs): its storage is a Rust COM object (IUnknown) the
// recordings keep alive; this face hands it the captured targets as their raw record.
#include <algorithm>
#include <iterator>
using cvr::stereo::scene_state::detail::GeometryTargetsRaw;
extern "C" {
HRESULT cvr_geometry_chain_create(ID3D12Device*,UINT,UINT,void**);
HRESULT cvr_geometry_chain_prepare_eye(void*,ID3D12GraphicsCommandList*,UINT,const GeometryTargetsRaw*);
bool cvr_geometry_chain_clear_private_eye(void*,ID3D12GraphicsCommandList*,UINT,UINT,float,UINT8);
bool cvr_geometry_chain_bind(void*,ID3D12GraphicsCommandList*,UINT,UINT);
bool cvr_geometry_chain_seed_private_depth(void*,ID3D12GraphicsCommandList*,UINT,ID3D12Resource*);
bool cvr_geometry_chain_seed_private_eye(void*,ID3D12GraphicsCommandList*,UINT,const GeometryTargetsRaw*);
bool cvr_geometry_chain_restore_native(void*,ID3D12GraphicsCommandList*,UINT,UINT);
bool cvr_geometry_chain_export_eye(void*,ID3D12GraphicsCommandList*,UINT);
ID3D12Resource* cvr_geometry_chain_resource(void*,UINT);
void cvr_geometry_chain_counts(void*,uint64_t (*)[7]);
}
namespace cvr::stereo {
struct StereoGeometryChain::Storage:IUnknown {};
namespace {
GeometryTargetsRaw Raw(const scene_state::GeometryTargets& targets){
    GeometryTargetsRaw raw{};
    for(UINT i=0;i<5;++i){raw.resources[i]=targets.resources[i].Get();raw.views[i]=targets.views[i];raw.handles[i]=targets.handles[i];}
    raw.colors=targets.colors;return raw;
}
}
StereoGeometryChain::StereoGeometryChain()=default;
StereoGeometryChain::~StereoGeometryChain()=default;
HRESULT StereoGeometryChain::Create(ID3D12Device* device,UINT width,UINT height){
    if(storage_)return E_INVALIDARG;
    void* storage{};const auto hr=cvr_geometry_chain_create(device,width,height,&storage);
    if(SUCCEEDED(hr))storage_.Attach(static_cast<Storage*>(storage));
    return hr;
}
HRESULT StereoGeometryChain::PrepareEye(ID3D12GraphicsCommandList* list,UINT eye,const scene_state::GeometryTargets& targets){
    const auto raw=Raw(targets);return cvr_geometry_chain_prepare_eye(storage_.Get(),list,eye,&raw);
}
bool StereoGeometryChain::ClearPrivateEye(ID3D12GraphicsCommandList* list,UINT eye,UINT colors,float depth,UINT8 stencil){
    return cvr_geometry_chain_clear_private_eye(storage_.Get(),list,eye,colors,depth,stencil);
}
bool StereoGeometryChain::Bind(ID3D12GraphicsCommandList* list,UINT eyeMask,UINT colors){
    return cvr_geometry_chain_bind(storage_.Get(),list,eyeMask,colors);
}
bool StereoGeometryChain::SeedPrivateDepth(ID3D12GraphicsCommandList* list,UINT eye,ID3D12Resource* source){
    return cvr_geometry_chain_seed_private_depth(storage_.Get(),list,eye,source);
}
bool StereoGeometryChain::SeedPrivateEye(ID3D12GraphicsCommandList* list,UINT eye,const scene_state::GeometryTargets& baseline){
    const auto raw=Raw(baseline);return cvr_geometry_chain_seed_private_eye(storage_.Get(),list,eye,&raw);
}
bool StereoGeometryChain::RestoreNative(ID3D12GraphicsCommandList* list,UINT eye,UINT colors){
    return cvr_geometry_chain_restore_native(storage_.Get(),list,eye,colors);
}
bool StereoGeometryChain::ExportEye(ID3D12GraphicsCommandList* list,UINT eye){
    return cvr_geometry_chain_export_eye(storage_.Get(),list,eye);
}
ID3D12Resource* StereoGeometryChain::Resource(UINT slot)const{return cvr_geometry_chain_resource(storage_.Get(),slot);}
std::array<uint64_t,7> StereoGeometryChain::Counts()const{
    uint64_t counts[7]{};cvr_geometry_chain_counts(storage_.Get(),&counts);
    std::array<uint64_t,7> out{};std::copy(std::begin(counts),std::end(counts),out.begin());return out;
}
}
#endif
#else
#include "Render/StereoTargetArray.hpp"
#include "Render/CommandResources.hpp"
#include "Render/RenderProbeScope.hpp"
#include <new>
#include <cmath>

namespace cvr::stereo {
using Microsoft::WRL::ComPtr;
namespace {
constexpr std::array<DXGI_FORMAT,5> Formats{DXGI_FORMAT_R10G10B10A2_UNORM,DXGI_FORMAT_R10G10B10A2_UNORM,
    DXGI_FORMAT_R8G8B8A8_UNORM,DXGI_FORMAT_R16G16_FLOAT,DXGI_FORMAT_R32G8X24_TYPELESS};
bool ColorCount(UINT colors){return colors==0 || colors==3 || colors==4;}
UINT Mask(UINT colors){return ((1u<<colors)-1)|16u;}
D3D12_RESOURCE_STATES Writable(UINT slot){return slot==4?D3D12_RESOURCE_STATE_DEPTH_WRITE:D3D12_RESOURCE_STATE_RENDER_TARGET;}
bool SameDevice(ID3D12Device* device,ID3D12DeviceChild* child){
    ComPtr<ID3D12Device> actual;return child && SUCCEEDED(child->GetDevice(IID_PPV_ARGS(&actual))) && actual.Get()==device;
}
}
struct StereoGeometryChain::Storage final:IUnknown {
    std::atomic<ULONG> refs{1};ComPtr<ID3D12Device> device;
    std::array<StereoTargetArray,5> arrays;
    std::array<scene_state::GeometryTargets,2> native;
    ComPtr<ID3D12DescriptorHeap> rtvs,dsvs;
    UINT width{},height{},rtvStride{},dsvStride{};
    std::array<UINT,2> imported{};std::array<bool,2> sealed{};
    std::array<bool,2> privateOnly{};
    std::array<uint64_t,7> counts{};
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** out)override{if(!out)return E_POINTER;*out=nullptr;if(id!=__uuidof(IUnknown))return E_NOINTERFACE;*out=this;AddRef();return S_OK;}
    ULONG STDMETHODCALLTYPE AddRef()override{return ++refs;}
    ULONG STDMETHODCALLTYPE Release()override{const auto n=--refs;if(!n)delete this;return n;}
    D3D12_CPU_DESCRIPTOR_HANDLE Rtv(UINT view,UINT slot)const{return {rtvs->GetCPUDescriptorHandleForHeapStart().ptr+SIZE_T(view*4+slot)*rtvStride};}
    D3D12_CPU_DESCRIPTOR_HANDLE Dsv(UINT view)const{return {dsvs->GetCPUDescriptorHandleForHeapStart().ptr+SIZE_T(view)*dsvStride};}
    bool List(ID3D12GraphicsCommandList* list)const{return list && list->GetType()==D3D12_COMMAND_LIST_TYPE_DIRECT && SameDevice(device.Get(),list);}
};
StereoGeometryChain::StereoGeometryChain()=default;
StereoGeometryChain::~StereoGeometryChain()=default;
HRESULT StereoGeometryChain::Create(ID3D12Device* device,UINT width,UINT height)try{
    if(storage_ || !device || !width || !height || width>4096 || height>4096)return E_INVALIDARG;
    ComPtr<Storage> s;s.Attach(new Storage);s->device=device;s->width=width;s->height=height;
    probe::Scope internal;
    D3D12_DESCRIPTOR_HEAP_DESC heap{D3D12_DESCRIPTOR_HEAP_TYPE_RTV,12,D3D12_DESCRIPTOR_HEAP_FLAG_NONE,0};
    auto hr=device->CreateDescriptorHeap(&heap,IID_PPV_ARGS(&s->rtvs));if(FAILED(hr))return hr;
    heap.Type=D3D12_DESCRIPTOR_HEAP_TYPE_DSV;heap.NumDescriptors=3;
    hr=device->CreateDescriptorHeap(&heap,IID_PPV_ARGS(&s->dsvs));if(FAILED(hr))return hr;
    s->rtvStride=device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV);
    s->dsvStride=device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_DSV);
    for(UINT slot=0;slot<5;++slot){
        D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D;desc.Width=width;desc.Height=height;
        desc.DepthOrArraySize=desc.MipLevels=1;desc.SampleDesc.Count=1;desc.Format=Formats[slot];
        desc.Flags=slot==4?D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL:D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
        hr=s->arrays[slot].Initialize(device,desc,Writable(slot));if(FAILED(hr))return hr;
        for(UINT view=0;view<3;++view){
            if(slot==4){D3D12_DEPTH_STENCIL_VIEW_DESC dsv{};dsv.Format=DXGI_FORMAT_D32_FLOAT_S8X24_UINT;
                dsv.ViewDimension=D3D12_DSV_DIMENSION_TEXTURE2DARRAY;dsv.Texture2DArray.FirstArraySlice=view==2?0:view;
                dsv.Texture2DArray.ArraySize=view==2?2:1;device->CreateDepthStencilView(s->arrays[slot].Resource(),&dsv,s->Dsv(view));
            }else {D3D12_RENDER_TARGET_VIEW_DESC rtv{};rtv.Format=Formats[slot];rtv.ViewDimension=D3D12_RTV_DIMENSION_TEXTURE2DARRAY;
                rtv.Texture2DArray.FirstArraySlice=view==2?0:view;rtv.Texture2DArray.ArraySize=view==2?2:1;
                device->CreateRenderTargetView(s->arrays[slot].Resource(),&rtv,s->Rtv(view,slot));}
        }
    }
    storage_=std::move(s);return S_OK;
}catch(const std::bad_alloc&){return E_OUTOFMEMORY;}
HRESULT StereoGeometryChain::PrepareEye(ID3D12GraphicsCommandList* list,UINT eye,const scene_state::GeometryTargets& targets){
    if(!storage_ || eye>1 || !ColorCount(targets.colors))return E_INVALIDARG;
    auto& s=*storage_.Get();if(s.sealed[eye] || s.privateOnly[eye] || !s.List(list))return E_INVALIDARG;
    const auto mask=Mask(targets.colors);
    for(UINT slot=0;slot<5;++slot)if(mask&(1u<<slot)){
        auto* source=targets.resources[slot].Get();const auto& view=targets.views[slot];
        if(!SameDevice(s.device.Get(),source) || !targets.handles[slot].ptr || !view.identity || view.resource!=reinterpret_cast<uintptr_t>(source) || view.viewFlags)return E_INVALIDARG;
        const auto desc=source->GetDesc();
        if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_TEXTURE2D || desc.Width!=s.width || desc.Height!=s.height ||
            desc.DepthOrArraySize!=1 || desc.MipLevels!=1 || desc.SampleDesc.Count!=1 || desc.SampleDesc.Quality || desc.Layout!=D3D12_TEXTURE_LAYOUT_UNKNOWN ||
            view.width!=desc.Width || view.height!=desc.Height || view.format!=UINT(desc.Format) || view.resourceFlags!=UINT(desc.Flags) ||
            view.viewFormat!=UINT(slot==4?DXGI_FORMAT_D32_FLOAT_S8X24_UINT:Formats[slot]) ||
            (desc.Format!=Formats[slot] && !(slot==4 && desc.Format==DXGI_FORMAT_D32_FLOAT_S8X24_UINT)) ||
            !(desc.Flags&(slot==4?D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL:D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET)))return E_INVALIDARG;
        if(s.imported[eye]&(1u<<slot)){
            if(s.native[eye].resources[slot].Get()!=source || s.native[eye].views[slot].identity!=view.identity)return E_INVALIDARG;
        }
        for(UINT previous=0;previous<slot;++previous)if((mask&(1u<<previous)) && targets.resources[previous].Get()==source)return E_INVALIDARG;
        for(const auto& other:s.native[1-eye].resources)if(other.Get()==source)return E_INVALIDARG;
    }
    // All caller inputs have been checked before recording or changing ownership.
    if(!gpu::KeepCommandResources(list,{&s}))return E_OUTOFMEMORY;
    probe::Scope internal;
    for(UINT slot=0;slot<5;++slot)if(mask&(1u<<slot)){
        auto& native=s.native[eye];native.resources[slot]=targets.resources[slot];native.views[slot]=targets.views[slot];native.handles[slot]=targets.handles[slot];
        if(s.imported[eye]&(1u<<slot))continue;
        const auto hr=s.arrays[slot].ImportEyeLayer(list,eye,Writable(slot),native.resources[slot].Get(),Writable(slot));
        if(FAILED(hr))return hr;
        s.imported[eye]|=1u<<slot;++s.counts[eye];
    }
    s.native[eye].colors=targets.colors;return S_OK;
}
bool StereoGeometryChain::ClearPrivateEye(ID3D12GraphicsCommandList* list,UINT eye,UINT colors,float depth,UINT8 stencil){
    if(!storage_ || eye>1 || !ColorCount(colors) || !std::isfinite(depth) || depth<0 || depth>1)return false;
    auto& s=*storage_.Get();if(!s.List(list) || s.imported[eye] || s.sealed[eye] || !gpu::KeepCommandResources(list,{&s}))return false;
    probe::Scope internal;const float zero[4]{};
    for(UINT slot=0;slot<colors;++slot)list->ClearRenderTargetView(s.Rtv(eye,slot),zero,0,nullptr);
    list->ClearDepthStencilView(s.Dsv(eye),D3D12_CLEAR_FLAG_DEPTH|D3D12_CLEAR_FLAG_STENCIL,depth,stencil,0,nullptr);
    s.privateOnly[eye]=true;s.imported[eye]=Mask(colors);return true;
}
bool StereoGeometryChain::Bind(ID3D12GraphicsCommandList* list,UINT eyeMask,UINT colors){
    if(!storage_ || !eyeMask || eyeMask>3 || !ColorCount(colors))return false;auto& s=*storage_.Get();
    if(!s.List(list))return false;
    for(UINT eye=0;eye<2;++eye)if((eyeMask&(1u<<eye)) && (s.sealed[eye] || (s.imported[eye]&Mask(colors))!=Mask(colors)))return false;
    if(!gpu::KeepCommandResources(list,{&s}))return false;
    const auto view=eyeMask==3?2u:eyeMask-1;const auto rtv=s.Rtv(view,0),dsv=s.Dsv(view);
    probe::Scope internal;list->OMSetRenderTargets(colors,colors?&rtv:nullptr,TRUE,&dsv);++s.counts[eyeMask==3?4:view+2];return true;
}
bool StereoGeometryChain::SeedPrivateDepth(ID3D12GraphicsCommandList* list,UINT eye,ID3D12Resource* source){
    if(!storage_ || eye>1 || !source)return false;auto& s=*storage_.Get();
    if(!s.privateOnly[eye] || s.sealed[eye] || !s.List(list) || !SameDevice(s.device.Get(),source))return false;
    const auto desc=source->GetDesc();
    if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_TEXTURE2D || desc.Width!=s.width || desc.Height!=s.height || desc.DepthOrArraySize!=1 ||
        desc.MipLevels!=1 || desc.SampleDesc.Count!=1 || (desc.Format!=Formats[4] && desc.Format!=DXGI_FORMAT_D32_FLOAT_S8X24_UINT) ||
        !(desc.Flags&D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL) || !gpu::KeepCommandResources(list,{&s,source}))return false;
    probe::Scope internal;
    return SUCCEEDED(s.arrays[4].ImportEyeLayer(list,eye,D3D12_RESOURCE_STATE_DEPTH_WRITE,source,D3D12_RESOURCE_STATE_DEPTH_WRITE));
}
bool StereoGeometryChain::SeedPrivateEye(ID3D12GraphicsCommandList* list,UINT eye,const scene_state::GeometryTargets& baseline){
    if(!storage_ || eye>1 || (baseline.colors!=3 && baseline.colors!=4))return false;
    auto& s=*storage_.Get();if(!s.List(list) || s.imported[eye] || s.sealed[eye] || s.privateOnly[eye])return false;
    const auto mask=Mask(baseline.colors);
    for(UINT slot=0;slot<5;++slot)if(mask&(1u<<slot)){
        auto* source=baseline.resources[slot].Get();const auto& view=baseline.views[slot];
        if(!SameDevice(s.device.Get(),source) || !baseline.handles[slot].ptr || !view.identity ||
            view.resource!=reinterpret_cast<uintptr_t>(source) || view.viewFlags)return false;
        const auto desc=source->GetDesc();
        if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_TEXTURE2D || desc.Width!=s.width || desc.Height!=s.height ||
            desc.DepthOrArraySize!=1 || desc.MipLevels!=1 || desc.SampleDesc.Count!=1 || desc.SampleDesc.Quality || desc.Layout!=D3D12_TEXTURE_LAYOUT_UNKNOWN ||
            view.width!=desc.Width || view.height!=desc.Height || view.format!=UINT(desc.Format) || view.resourceFlags!=UINT(desc.Flags) ||
            view.viewFormat!=UINT(slot==4?DXGI_FORMAT_D32_FLOAT_S8X24_UINT:Formats[slot]) ||
            (desc.Format!=Formats[slot] && !(slot==4 && desc.Format==DXGI_FORMAT_D32_FLOAT_S8X24_UINT)) ||
            !(desc.Flags&(slot==4?D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL:D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET)))return false;
        for(UINT previous=0;previous<slot;++previous)if((mask&(1u<<previous)) && baseline.resources[previous].Get()==source)return false;
    }
    if(!gpu::KeepCommandResources(list,{&s}))return false;
    s.privateOnly[eye]=true;s.native[eye]=baseline;probe::Scope internal;
    for(UINT slot=0;slot<5;++slot)if(mask&(1u<<slot)){
        if(FAILED(s.arrays[slot].ImportEyeLayer(list,eye,Writable(slot),baseline.resources[slot].Get(),Writable(slot))))return false;
        s.imported[eye]|=1u<<slot;++s.counts[eye];
    }
    return true;
}
bool StereoGeometryChain::RestoreNative(ID3D12GraphicsCommandList* list,UINT eye,UINT colors){
    if(!storage_ || eye>1 || !ColorCount(colors))return false;auto& s=*storage_.Get();if(s.privateOnly[eye])return false;
    if(!s.List(list) || (s.imported[eye]&Mask(colors))!=Mask(colors) || !gpu::KeepCommandResources(list,{&s}))return false;
    const auto& native=s.native[eye];probe::Scope internal;
    list->OMSetRenderTargets(colors,colors?native.handles.data():nullptr,FALSE,&native.handles[4]);return true;
}
bool StereoGeometryChain::ExportEye(ID3D12GraphicsCommandList* list,UINT eye){
    if(!storage_ || eye>1)return false;auto& s=*storage_.Get();
    if(s.privateOnly[eye] || s.sealed[eye] || !(s.imported[eye]&16) || !s.List(list) || !gpu::KeepCommandResources(list,{&s}))return false;
    probe::Scope internal;
    for(UINT slot=0;slot<5;++slot)if(s.imported[eye]&(1u<<slot)){
        if(FAILED(s.arrays[slot].CopyEyeLayer(list,eye,Writable(slot),s.native[eye].resources[slot].Get(),Writable(slot),Writable(slot))) ||
            FAILED(s.arrays[slot].TransitionEye(list,eye,Writable(slot),D3D12_RESOURCE_STATE_COPY_SOURCE)))return false;
        ++s.counts[5+eye];
    }
    s.sealed[eye]=true;return RestoreNative(list,eye,s.native[eye].colors);
}
ID3D12Resource* StereoGeometryChain::Resource(UINT slot)const{return storage_ && slot<5?storage_->arrays[slot].Resource():nullptr;}
std::array<uint64_t,7> StereoGeometryChain::Counts()const{return storage_?storage_->counts:std::array<uint64_t,7>{};}
}
#endif
