#pragma once
#include "Render/RegisteredConstantUpload.hpp"
#include <array>
#include <span>

namespace cvr::stereo {
// Physical copy inputs only. The caller separately authenticates native
// allocation lifetime, access ordering, residency and the final states.
// Retain this object until its submitted queue fence completes. No CPU waits,
// queue submission, mapping, native registration or export permission here.
class NativeOutputCopy {
public:
    struct Target {
        constants::ResourceCreationInfo creation;
        UINT planeCount{};
        std::array<D3D12_RESOURCE_STATES,2> finalStates{};
    };
    using Arrays=std::array<Microsoft::WRL::ComPtr<ID3D12Resource>,4>;
    using Targets=std::array<std::array<Target,4>,2>;
    struct Plane {
        UINT side{},slot{},plane{};
        D3D12_PLACED_SUBRESOURCE_FOOTPRINT footprint{};
        UINT rows{},stride{},component{},bytes{};
    };
#if CVR_RUST_VRIK
    // The copy: rust/crates/cvr-gpu/src/output_copy.rs (targets handed over borrowed; the planes copied back).
    struct RustTarget {
        uint32_t kind; ID3D12Resource* resource; ID3D12Heap* heap; uint64_t heapOffset,allocationBytes;
        UINT planeCount; D3D12_RESOURCE_STATES finalStates[2];
    };
    NativeOutputCopy();
    ~NativeOutputCopy();
    NativeOutputCopy(const NativeOutputCopy&)=delete;
    NativeOutputCopy& operator=(const NativeOutputCopy&)=delete;
    HRESULT Create(ID3D12Device*,const Arrays&,const Targets&,ID3D12Resource* predicate,UINT64 offset);
    bool Record(ID3D12GraphicsCommandList*);
    ID3D12Resource* Readback()const;
    std::span<const Plane> Planes()const{return Readback()?std::span<const Plane>(planes_):std::span<const Plane>{};}
    UINT64 Bytes()const;
    bool Recorded()const;
private:
    void* rust_;
    std::array<Plane,10> planes_{};
#else
    NativeOutputCopy()=default;
    NativeOutputCopy(const NativeOutputCopy&)=delete;
    NativeOutputCopy& operator=(const NativeOutputCopy&)=delete;
    // Compact input slots are 0/1/2/4; outer target index is native side.
    // Arrays must already be COPY_SOURCE; the predicate must be readable for
    // predication. Neither state can be queried from the D3D12 objects.
    HRESULT Create(ID3D12Device*,const Arrays&,const Targets&,ID3D12Resource* predicate,UINT64 offset);
    // One recording only, on a caller-owned DIRECT list. Leaves predication
    // disabled. Predicate zero skips only image writes, never barriers/readback.
    bool Record(ID3D12GraphicsCommandList*);
    ID3D12Resource* Readback()const{return readback_.Get();}
    std::span<const Plane> Planes()const{return readback_?std::span<const Plane>(planes_):std::span<const Plane>{};}
    UINT64 Bytes()const{return bytes_;}
    bool Recorded()const{return recorded_;}
private:
    Microsoft::WRL::ComPtr<ID3D12Device> device_;
    Arrays arrays_;
    Targets targets_;
    Microsoft::WRL::ComPtr<ID3D12Resource> predicate_,readback_;
    std::array<Plane,10> planes_{};
    UINT64 predicateOffset_{},bytes_{};
    bool recorded_{};
#endif
};
static_assert(sizeof(NativeOutputCopy::Plane)==64,"mirrored by rust/crates/cvr-gpu/src/output_copy.rs");
#if CVR_RUST_VRIK
}
extern "C" {
void* cvr_output_copy_new();
void cvr_output_copy_free(void*);
HRESULT cvr_output_copy_create(const void*,ID3D12Device*,ID3D12Resource* const (*arrays)[4],
                               const cvr::stereo::NativeOutputCopy::RustTarget (*targets)[8],ID3D12Resource* predicate,UINT64 offset);
bool cvr_output_copy_record(const void*,ID3D12GraphicsCommandList*);
void cvr_output_copy_info(const void*,ID3D12Resource** readback,UINT64* bytes,bool* recorded,
                          std::array<cvr::stereo::NativeOutputCopy::Plane,10>* planes);
}
namespace cvr::stereo {
inline NativeOutputCopy::NativeOutputCopy():rust_(cvr_output_copy_new()){}
inline NativeOutputCopy::~NativeOutputCopy(){cvr_output_copy_free(rust_);}
inline HRESULT NativeOutputCopy::Create(ID3D12Device* device,const Arrays& arrays,const Targets& targets,ID3D12Resource* predicate,UINT64 offset){
    ID3D12Resource* const raw[4]{arrays[0].Get(),arrays[1].Get(),arrays[2].Get(),arrays[3].Get()};
    RustTarget rows[8]{};
    for(UINT side=0;side<2;++side)for(UINT slot=0;slot<4;++slot){const auto& t=targets[side][slot];
        rows[side*4+slot]={t.creation.kind,t.creation.resource.Get(),t.creation.heap.Get(),t.creation.heapOffset,t.creation.allocationBytes,
                           t.planeCount,{t.finalStates[0],t.finalStates[1]}};}
    const HRESULT hr=cvr_output_copy_create(rust_,device,&raw,&rows,predicate,offset);
    if(SUCCEEDED(hr))cvr_output_copy_info(rust_,nullptr,nullptr,nullptr,&planes_);
    return hr;
}
inline bool NativeOutputCopy::Record(ID3D12GraphicsCommandList* list){return cvr_output_copy_record(rust_,list);}
inline ID3D12Resource* NativeOutputCopy::Readback()const{ID3D12Resource* r{};cvr_output_copy_info(rust_,&r,nullptr,nullptr,nullptr);return r;}
inline UINT64 NativeOutputCopy::Bytes()const{UINT64 b{};cvr_output_copy_info(rust_,nullptr,&b,nullptr,nullptr);return b;}
inline bool NativeOutputCopy::Recorded()const{bool r{};cvr_output_copy_info(rust_,nullptr,nullptr,&r,nullptr);return r;}
#endif
}
