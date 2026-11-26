#pragma once
#include <d3d12.h>
#include <wrl/client.h>
#include <array>
#include <span>

namespace cvr::stereo {
struct GeometryCompareRange {
    UINT pair{};
    std::array<Microsoft::WRL::ComPtr<ID3D12Resource>,2> resources;
    std::array<UINT64,2> offsets{};
    UINT64 bytes{};
};
// GPU equality plus complementary draw predicates. The caller certifies immutable
// inputs, NON_PIXEL_SHADER_RESOURCE states and producer order through execution.
// Each pair must have at least one range. A zero external gate disables BOTH
// branches; otherwise exactly one branch executes. No CPU readback is required.
// Results also finish NON_PIXEL_SHADER_RESOURCE, so a checked predicate can be
// the permission input of another comparison in the same GPU recording.
// All ranges sharing the same resource pair use one metadata-driven dispatch.
#if CVR_RUST_VRIK
}
#include <cstdint>
#include <vector>
// The comparison: rust/crates/cvr-gpu/src/geometry_compare.rs (the ranges handed over borrowed).
struct CvrGeometryCompareRange { UINT pair; ID3D12Resource* resources[2]; UINT64 offsets[2]; UINT64 bytes; };
extern "C" {
void* cvr_geometry_compare_new();
void cvr_geometry_compare_free(void*);
HRESULT cvr_geometry_compare_create(const void*,ID3D12Device*,UINT pairs,const CvrGeometryCompareRange* ranges,size_t count,ID3D12Resource* gate,UINT64 gateOffset);
bool cvr_geometry_compare_record(const void*,ID3D12GraphicsCommandList*);
void cvr_geometry_compare_info(const void*,ID3D12Resource** predicates,uint64_t* counts);
}
namespace cvr::stereo {
class GpuGeometryCompare {
public:
    GpuGeometryCompare():rust_(cvr_geometry_compare_new()){}
    ~GpuGeometryCompare(){cvr_geometry_compare_free(rust_);}
    GpuGeometryCompare(const GpuGeometryCompare&)=delete;
    GpuGeometryCompare& operator=(const GpuGeometryCompare&)=delete;
    HRESULT Create(ID3D12Device* device,UINT pairs,std::span<const GeometryCompareRange> ranges,ID3D12Resource* gate,UINT64 gateOffset=0){
        std::vector<CvrGeometryCompareRange> rows;rows.reserve(ranges.size());
        for(const auto& r:ranges)rows.push_back({r.pair,{r.resources[0].Get(),r.resources[1].Get()},{r.offsets[0],r.offsets[1]},r.bytes});
        return cvr_geometry_compare_create(rust_,device,pairs,rows.data(),rows.size(),gate,gateOffset);
    }
    bool Record(ID3D12GraphicsCommandList* list){return cvr_geometry_compare_record(rust_,list);}
    ID3D12Resource* Predicates() const{ID3D12Resource* p{};cvr_geometry_compare_info(rust_,&p,nullptr);return p;}
    static constexpr UINT64 Offset(UINT pair,bool fallback=false){return UINT64(pair)*16+(fallback?8:0);}
    UINT PairCount() const{return UINT(Counts()[0]);}
    UINT RangeCount() const{return UINT(Counts()[1]);}
    UINT DispatchCount() const{return UINT(Counts()[2]);}
    UINT64 ComparedBytes() const{return Counts()[3];}
private:
    std::array<uint64_t,4> Counts()const{std::array<uint64_t,4> n{};cvr_geometry_compare_info(rust_,nullptr,n.data());return n;}
    void* rust_;
};
}
#else
class GpuGeometryCompare {
public:
    GpuGeometryCompare();
    ~GpuGeometryCompare();
    HRESULT Create(ID3D12Device*,UINT pairs,std::span<const GeometryCompareRange>,ID3D12Resource* gate,UINT64 gateOffset=0);
    // Single recording, direct list only. Changes compute root/PSO and clears
    // predication; caller must restore native bindings. Retains all input owners.
    bool Record(ID3D12GraphicsCommandList*);
    ID3D12Resource* Predicates() const;
    static constexpr UINT64 Offset(UINT pair,bool fallback=false){return UINT64(pair)*16+(fallback?8:0);}
    UINT PairCount() const;
    UINT RangeCount() const;
    UINT DispatchCount() const;
    UINT64 ComparedBytes() const;
private:
    struct Storage;
    Microsoft::WRL::ComPtr<Storage> storage_;
};
}
#endif
