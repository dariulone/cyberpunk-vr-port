#pragma once
#include <d3d12.h>
#include <wrl/client.h>
#include <array>

#if CVR_RUST_VRIK
#include <utility>
// The resolve: rust/crates/cvr-gpu/src/group_resolve.rs (the sources handed over borrowed).
extern "C" {
bool cvr_group_resolve_supports(const D3D12_BLEND_DESC*,const D3D12_DEPTH_STENCIL_DESC*);
void* cvr_group_resolve_new();
void cvr_group_resolve_free(void*);
HRESULT cvr_group_resolve_initialize(void*,ID3D12Device*,ID3D12Resource* const (*sources)[4],bool depthOnly);
bool cvr_group_resolve_retain(const void*,ID3D12GraphicsCommandList*);
void cvr_group_resolve_record(const void*,ID3D12GraphicsCommandList*,UINT eye,UINT stencil,const D3D12_VIEWPORT*,const D3D12_RECT*,
    const D3D12_CPU_DESCRIPTOR_HANDLE (*targets)[3],D3D12_CPU_DESCRIPTOR_HANDLE depth);
}
namespace cvr::stereo {
// Opaque, reverse-Z group only: ALWAYS/REPLACE stencil, full MRT writes, no
// blending. The caller restores native graphics bindings after Record().
class StereoGroupResolve {
public:
    StereoGroupResolve():rust_(cvr_group_resolve_new()){}
    ~StereoGroupResolve(){cvr_group_resolve_free(rust_);}
    StereoGroupResolve(StereoGroupResolve&& other)noexcept:rust_(std::exchange(other.rust_,cvr_group_resolve_new())){}
    StereoGroupResolve& operator=(StereoGroupResolve&& other)noexcept{std::swap(rust_,other.rust_);return *this;}
    StereoGroupResolve(const StereoGroupResolve&)=delete;
    StereoGroupResolve& operator=(const StereoGroupResolve&)=delete;
    static bool Supports(const D3D12_BLEND_DESC& blend,const D3D12_DEPTH_STENCIL_DESC& depth){return cvr_group_resolve_supports(&blend,&depth);}
    HRESULT Initialize(ID3D12Device* device,const std::array<ID3D12Resource*,4>& sources,bool depthOnly=false){
        ID3D12Resource* const raw[4]{sources[0],sources[1],sources[2],sources[3]};
        return cvr_group_resolve_initialize(rust_,device,&raw,depthOnly);
    }
    bool Retain(ID3D12GraphicsCommandList* list) const{return cvr_group_resolve_retain(rust_,list);}
    void Record(ID3D12GraphicsCommandList* list,UINT eye,UINT stencilReference,
        const D3D12_VIEWPORT& viewport,const D3D12_RECT& scissor,
        const std::array<D3D12_CPU_DESCRIPTOR_HANDLE,3>& targets,D3D12_CPU_DESCRIPTOR_HANDLE depth) const{
        const D3D12_CPU_DESCRIPTOR_HANDLE raw[3]{targets[0],targets[1],targets[2]};
        cvr_group_resolve_record(rust_,list,eye,stencilReference,&viewport,&scissor,&raw,depth);
    }
private:
    void* rust_;
};
}
#else
namespace cvr::stereo {
// Opaque, reverse-Z group only: ALWAYS/REPLACE stencil, full MRT writes, no
// blending. The caller restores native graphics bindings after Record().
class StereoGroupResolve {
public:
    static bool Supports(const D3D12_BLEND_DESC&,const D3D12_DEPTH_STENCIL_DESC&);
    HRESULT Initialize(ID3D12Device*,const std::array<ID3D12Resource*,4>&,bool depthOnly=false);
    bool Retain(ID3D12GraphicsCommandList*) const;
    void Record(ID3D12GraphicsCommandList*,UINT eye,UINT stencilReference,
        const D3D12_VIEWPORT&,const D3D12_RECT&,
        const std::array<D3D12_CPU_DESCRIPTOR_HANDLE,3>&,D3D12_CPU_DESCRIPTOR_HANDLE) const;
private:
    Microsoft::WRL::ComPtr<ID3D12RootSignature> root_;
    Microsoft::WRL::ComPtr<ID3D12PipelineState> pipeline_;
    Microsoft::WRL::ComPtr<ID3D12DescriptorHeap> heap_;
    std::array<ID3D12Resource*,4> sources_{};
    bool depthOnly_{};
};
}
#endif
