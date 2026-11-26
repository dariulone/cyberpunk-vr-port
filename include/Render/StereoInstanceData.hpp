#pragma once
#include "Render/NativeStereoProbe.hpp"
#include <span>
#include <cstring>
#include <vector>

namespace cvr::stereo::instance_data {
constexpr size_t MaxBytes=16*1024;
// The selected shaders consume a complete 48-byte transform (64 with skin
// metadata). Never accept a prefix/hash as proof of equal instance batches.
#if CVR_RUST_VRIK
// The rules: rust/crates/cvr-render/src/instance_data.rs (tools/single_pass_tests/instance_data_test runs its cases
// against them in its Rust variant); the draw record's fields are lent.
struct RustRangeInput {
    uint64_t bufferLocation;uint32_t sizeInBytes,stride;uint64_t instanceResource,instanceGpuBase,instanceBytesTotal;
    uint32_t vertexMask,instances,firstInstance,heapType;
};
static_assert(sizeof(RustRangeInput)==56,"mirrored by instance_data.rs");
extern "C" {
bool cvr_id_range(const RustRangeInput*,bool,uint64_t*,size_t*);
bool cvr_id_equal(uint32_t,uint32_t,const uint8_t*,size_t,uint32_t,uint32_t,const uint8_t*,size_t);
}
inline bool Range(const native_probe::DrawRecord& d,uint64_t& offset,size_t& bytes,bool allowObservedDefault=false) {
    const auto& v=d.vertices[7];
    const RustRangeInput input{v.BufferLocation,v.SizeInBytes,v.StrideInBytes,d.instanceResource,d.instanceGpuBase,
        d.instanceBytesTotal,d.vertexMask,d.instances,d.firstInstance,d.instanceHeapType};
    return cvr_id_range(&input,allowObservedDefault,&offset,&bytes);
}
inline bool Equal(const native_probe::DrawRecord& a,std::span<const uint8_t> x,
                  const native_probe::DrawRecord& b,std::span<const uint8_t> y) {
    return cvr_id_equal(a.instances,a.vertices[7].StrideInBytes,x.data(),x.size(),b.instances,b.vertices[7].StrideInBytes,y.data(),y.size());
}
#else
inline bool Range(const native_probe::DrawRecord& d,uint64_t& offset,size_t& bytes,bool allowObservedDefault=false) {
    const auto& v=d.vertices[7];
    if(!(d.vertexMask&(1u<<7)) || !d.instances || (v.StrideInBytes!=48 && v.StrideInBytes!=64) ||
       (d.instanceHeapType!=D3D12_HEAP_TYPE_UPLOAD && !(allowObservedDefault && d.instanceHeapType==D3D12_HEAP_TYPE_DEFAULT)) ||
       !d.instanceResource || v.BufferLocation<d.instanceGpuBase)return false;
    const uint64_t begin=uint64_t(d.firstInstance)*v.StrideInBytes,total=uint64_t(d.instances)*v.StrideInBytes;
    if(total>MaxBytes || begin>v.SizeInBytes || total>v.SizeInBytes-begin)return false;
    const uint64_t base=v.BufferLocation-d.instanceGpuBase;
    if(base>d.instanceBytesTotal || begin>d.instanceBytesTotal-base || total>d.instanceBytesTotal-base-begin)return false;
    offset=base+begin;bytes=size_t(total);return true;
}
inline bool Equal(const native_probe::DrawRecord& a,std::span<const uint8_t> x,
                  const native_probe::DrawRecord& b,std::span<const uint8_t> y) {
    const auto stride=a.vertices[7].StrideInBytes;
    return a.instances && a.instances==b.instances && (stride==48 || stride==64) && stride==b.vertices[7].StrideInBytes &&
        x.size()==uint64_t(a.instances)*stride && x.size()<=MaxBytes && x.size()==y.size() &&
        std::memcmp(x.data(),y.data(),x.size())==0;
}
#endif
// DEFAULT heaps cannot be mapped. The supplied reader must only return bytes
// observed in CPU uploads for this exact resource identity and complete range.
template<class Read> bool CopyObservedDefault(native_probe::DrawRecord& row,std::vector<uint8_t>* batch,Read read) {
    row.instanceDataBytes=0;row.instanceData={};if(batch)batch->clear();
    uint64_t offset{};size_t bytes{};
    if(row.instanceHeapType!=D3D12_HEAP_TYPE_DEFAULT || (!batch && row.instances!=1) ||
       !Range(row,offset,bytes,true))return false;
    if(batch)batch->resize(bytes);
    auto* output=batch?batch->data():row.instanceData.data();
    if(!read(row.instanceGpuBase,row.instanceResource,offset,output,bytes)) {
        row.instanceData={};if(batch)batch->clear();return false;
    }
    if(row.instances==1){if(batch)std::memcpy(row.instanceData.data(),output,bytes);row.instanceDataBytes=UINT(bytes);}
    return true;
}
}
