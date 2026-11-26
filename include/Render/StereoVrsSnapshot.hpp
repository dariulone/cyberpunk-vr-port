#pragma once
#include <d3d12.h>
#include <wrl/client.h>
#include <array>
#include <cstdint>
#include <vector>
#include <span>
#include <algorithm>
#if CVR_RUST_VRIK
// The snapshot and the finer-rate merge: rust/crates/cvr-gpu/src/vrs_snapshot.rs.
extern "C" {
bool cvr_vrs_build_finer(const uint8_t* a,size_t aLength,const uint8_t* b,size_t bLength,uint8_t* out);
void* cvr_vrs_snapshot_new();
void cvr_vrs_snapshot_free(void*);
HRESULT cvr_vrs_snapshot_initialize(const void*,ID3D12Device*,ID3D12Resource* image);
HRESULT cvr_vrs_snapshot_capture(const void*,ID3D12GraphicsCommandList*,ID3D12Resource* image,UINT eye);
bool cvr_vrs_snapshot_retain(const void*,ID3D12GraphicsCommandList*);
HRESULT cvr_vrs_snapshot_read_eye(const void*,UINT eye,uint8_t* out,size_t length);
HRESULT cvr_vrs_snapshot_read(const void*,uint64_t* differences,std::array<std::array<uint64_t,16>,2>* histogram);
void cvr_vrs_snapshot_info(const void*,ID3D12Resource** frozen,UINT* width,UINT* height);
}
namespace cvr::stereo {
inline bool BuildFinerVrs(std::span<const uint8_t> a,std::span<const uint8_t> b,std::vector<uint8_t>& out){
    out.clear();if(a.empty() || a.size()!=b.size())return false;
    std::vector<uint8_t> values(a.size());
    if(!cvr_vrs_build_finer(a.data(),a.size(),b.data(),b.size(),values.data()))return false;
    out=std::move(values);return true;
}
// One diagnostic frame. Owner retains native source images until its fence.
class StereoVrsSnapshot {
public:
    StereoVrsSnapshot():rust_(cvr_vrs_snapshot_new()){}
    ~StereoVrsSnapshot(){cvr_vrs_snapshot_free(rust_);}
    StereoVrsSnapshot(const StereoVrsSnapshot&)=delete;
    StereoVrsSnapshot& operator=(const StereoVrsSnapshot&)=delete;
    HRESULT Initialize(ID3D12Device* device,ID3D12Resource* image){return cvr_vrs_snapshot_initialize(rust_,device,image);}
    HRESULT Capture(ID3D12GraphicsCommandList* list,ID3D12Resource* image,UINT eye){return cvr_vrs_snapshot_capture(rust_,list,image,eye);}
    bool Retain(ID3D12GraphicsCommandList* list)const{return cvr_vrs_snapshot_retain(rust_,list);}
    HRESULT Read(uint64_t& differences,std::array<std::array<uint64_t,16>,2>& histogram)const{return cvr_vrs_snapshot_read(rust_,&differences,&histogram);}
    // Caller has observed the source submission fence. Returns pixels only.
    HRESULT ReadEye(UINT eye,std::vector<uint8_t>& output)const{
        if(!output.empty())return E_INVALIDARG;
        std::vector<uint8_t> pixels(static_cast<size_t>(Width())*Height());
        const HRESULT hr=cvr_vrs_snapshot_read_eye(rust_,eye,pixels.data(),pixels.size());
        if(SUCCEEDED(hr))output=std::move(pixels);
        return hr;
    }
    ID3D12Resource* SourceImage()const{ID3D12Resource* frozen{};cvr_vrs_snapshot_info(rust_,&frozen,nullptr,nullptr);return frozen;}
    UINT Width()const{UINT w{};cvr_vrs_snapshot_info(rust_,nullptr,&w,nullptr);return w;}
    UINT Height()const{UINT h{};cvr_vrs_snapshot_info(rust_,nullptr,nullptr,&h);return h;}
private:
    void* rust_;
};
}
#else
namespace cvr::stereo {
inline bool BuildFinerVrs(std::span<const uint8_t> a,std::span<const uint8_t> b,std::vector<uint8_t>& out){
    out.clear();if(a.empty() || a.size()!=b.size())return false;std::vector<uint8_t> values;values.reserve(a.size());
    const auto valid=[](uint8_t value){return value==0 || value==1 || value==4 || value==5 || value==6 || value==9 || value==10;};
    for(size_t i=0;i<a.size();++i){if(!valid(a[i]) || !valid(b[i]))return false;
        values.push_back(uint8_t(((std::min)(a[i]>>2,b[i]>>2)<<2)|(std::min)(a[i]&3,b[i]&3)));}
    out=std::move(values);return true;
}
// One diagnostic frame. Owner retains native source images until its fence.
class StereoVrsSnapshot {
public:
    HRESULT Initialize(ID3D12Device*,ID3D12Resource* image);
    HRESULT Capture(ID3D12GraphicsCommandList*,ID3D12Resource* image,UINT eye);
    bool Retain(ID3D12GraphicsCommandList*) const;
    HRESULT Read(uint64_t& differences,std::array<std::array<uint64_t,16>,2>& histogram) const;
    // Caller has observed the source submission fence. Returns pixels only.
    HRESULT ReadEye(UINT eye,std::vector<uint8_t>&)const;
    ID3D12Resource* SourceImage() const {return frozen_.Get();}
    UINT Width()const{return UINT(desc_.Width);}UINT Height()const{return desc_.Height;}
private:
    Microsoft::WRL::ComPtr<ID3D12Resource> frozen_,readback_;
    D3D12_RESOURCE_DESC desc_{};
    std::array<D3D12_PLACED_SUBRESOURCE_FOOTPRINT,2> footprints_{};
    UINT64 bytes_{};
    UINT capturedMask_{};
};
}
#endif
