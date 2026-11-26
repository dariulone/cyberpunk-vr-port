#pragma once
#include <d3d12.h>
#include <array>
#include <cstdint>
#include <span>
#include <vector>

namespace cvr::stereo {
// Matches patch_resource_pairs.py: space-0 register N -> 2*N + SV_ViewID.
struct PairedBinding {
    D3D12_DESCRIPTOR_RANGE_TYPE type{};
    D3D12_SHADER_VISIBILITY visibility{};
    UINT sourceRegister{},privateSpace{};
    bool operator==(const PairedBinding&) const = default;
};
struct PairedBindingLocation {
    PairedBinding binding{};
    UINT sourceParameter{},sourceOffset{},destinationParameter{},destinationOffset{};
    D3D12_ROOT_PARAMETER_TYPE sourceType{};
    bool operator==(const PairedBindingLocation&) const = default;
};
struct PairedRootPlan {
    std::vector<uint8_t> serialized;
    std::vector<PairedBindingLocation> bindings;
    std::array<UINT,2> parameters{UINT_MAX,UINT_MAX},descriptors{}; // VS, PS
    UINT originalParameters{},rootDwords{};
    bool operator==(const PairedRootPlan&) const = default;
};
static_assert(sizeof(PairedBinding)==16 && sizeof(PairedBindingLocation)==36,"mirrored by rust/crates/cvr-render/src/paired_root.rs");
#if CVR_RUST_VRIK
}
// The builder: rust/crates/cvr-render/src/paired_root.rs; the plan is copied out of its handle.
extern "C" {
HRESULT cvr_paired_root_build(const void* bytes,size_t length,const cvr::stereo::PairedBinding* bindings,size_t count,void** plan);
void cvr_paired_root_parts(const void* plan,const uint8_t** serialized,size_t* serializedLength,
                           const cvr::stereo::PairedBindingLocation** bindings,size_t* bindingCount,UINT* scalars);
void cvr_paired_root_free(void* plan);
}
namespace cvr::stereo {
// Preserves every original parameter index, range, flag and static sampler.
// Adds at most one mixed CBV/SRV table per stage. Failure leaves output unchanged.
inline HRESULT BuildPairedRoot(const void* bytes,size_t length,std::span<const PairedBinding> bindings,PairedRootPlan& output) {
    void* plan=nullptr;
    const HRESULT hr=cvr_paired_root_build(bytes,length,bindings.data(),bindings.size(),&plan);
    if(FAILED(hr))return hr;
    const uint8_t* serialized{};size_t serializedLength{};const PairedBindingLocation* located{};size_t count{};UINT k[6]{};
    cvr_paired_root_parts(plan,&serialized,&serializedLength,&located,&count,k);
    PairedRootPlan result;
    result.serialized.assign(serialized,serialized+serializedLength);result.bindings.assign(located,located+count);
    result.parameters={k[0],k[1]};result.descriptors={k[2],k[3]};result.originalParameters=k[4];result.rootDwords=k[5];
    cvr_paired_root_free(plan);
    output=std::move(result);return S_OK;
}
}
#else
// Preserves every original parameter index, range, flag and static sampler.
// Adds at most one mixed CBV/SRV table per stage. No resource is retained or
// copied here; source locations describe where a future owned bank gets data.
// Failure leaves output unchanged. Serialized input may be root version 1.0/1.1.
HRESULT BuildPairedRoot(const void*,size_t,std::span<const PairedBinding>,PairedRootPlan&);
}
#endif
