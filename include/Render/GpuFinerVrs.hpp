#pragma once
#include <d3d12.h>
#include <wrl/client.h>
#include <array>
#include <cstdint>
#include <span>
#include <vector>

#if CVR_RUST_VRIK
#include "Render/RenderProbeScope.hpp"
// The reduction: rust/crates/cvr-gpu/src/finer_vrs.rs.
extern "C" {
void* cvr_finer_vrs_new();
void cvr_finer_vrs_free(void*);
HRESULT cvr_finer_vrs_create(const void*,ID3D12Device*,ID3D12Resource* const* images,size_t count);
bool cvr_finer_vrs_record(const void*,ID3D12GraphicsCommandList*);
void cvr_finer_vrs_info(const void*,ID3D12Resource** image,uint64_t* counts);
HRESULT cvr_finer_vrs_read(const void*,uint8_t* pixels,size_t length,UINT* invalidRates);
}
namespace cvr::stereo {
class GpuFinerVrs {
public:
    GpuFinerVrs():rust_(cvr_finer_vrs_new()){}
    ~GpuFinerVrs(){cvr_finer_vrs_free(rust_);}
    GpuFinerVrs(const GpuFinerVrs&)=delete;
    GpuFinerVrs& operator=(const GpuFinerVrs&)=delete;
    HRESULT Create(ID3D12Device* device,std::span<ID3D12Resource* const> images){return cvr_finer_vrs_create(rust_,device,images.data(),images.size());}
    bool Record(ID3D12GraphicsCommandList* list){return cvr_finer_vrs_record(rust_,list);}
    ID3D12Resource* Image()const{ID3D12Resource* image{};cvr_finer_vrs_info(rust_,&image,nullptr);return image;}
    UINT SourceCount()const{return UINT(Counts()[0]);}
    UINT DispatchCount()const{return UINT(Counts()[1]);}
    UINT64 ReadbackBytes()const{return Counts()[2];}
    HRESULT Read(std::vector<uint8_t>& pixels,UINT& invalidRates)const{
        invalidRates=0;if(!pixels.empty())return E_PENDING;
        std::vector<uint8_t> output(static_cast<size_t>(Counts()[3]));
        const HRESULT hr=cvr_finer_vrs_read(rust_,output.data(),output.size(),&invalidRates);
        if(SUCCEEDED(hr))pixels=std::move(output);
        return hr;
    }
private:
    std::array<uint64_t,4> Counts()const{std::array<uint64_t,4> counts{};cvr_finer_vrs_info(rust_,nullptr,counts.data());return counts;}
    void* rust_;
};
}
#else
namespace cvr::stereo {
// Inputs are owned original-position snapshots in SHADING_RATE_SOURCE state.
// The caller proves their GPU producer order; no source pixels are read on CPU
// during creation or recording. Source states are restored after reduction.
class GpuFinerVrs {
public:
    GpuFinerVrs();~GpuFinerVrs();
    HRESULT Create(ID3D12Device*,std::span<ID3D12Resource* const>);
    // Single-use DIRECT recording. Changes heaps and compute bindings.
    bool Record(ID3D12GraphicsCommandList*);
    ID3D12Resource* Image()const;
    UINT SourceCount()const;
    UINT DispatchCount()const;
    UINT64 ReadbackBytes()const;
    // Diagnostic only, after the consumer's actual submission fence completes.
    HRESULT Read(std::vector<uint8_t>& pixels,UINT& invalidRates)const;
private:
    struct Storage;Microsoft::WRL::ComPtr<Storage> storage_;
};
}
#endif
