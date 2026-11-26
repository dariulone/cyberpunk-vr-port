#pragma once
// In-place constant patches of signed DXIL containers (docs/hidden-area-mask-20261009.md).
//
// A float constant in [1, 2) has bit 29 as its highest set bit, so in the LLVM bitcode of the DXIL
// part it is always six VBR6 chunks (36 bits) long. Writing another value from that range at the
// same bit offset keeps every other bit of the container where it was; only the container hash has
// to be recomputed, which is the DXBC MD5 variant D3D12 checks.
#include <array>
#include <cstddef>
#include <cstdint>
#include <vector>

namespace cvr::dxil_patch {
using Hash=std::array<uint8_t,16>;
#if CVR_RUST_VRIK
// The patches: rust/crates/cvr-render/src/dxil_patch.rs.
extern "C" {
void cvr_dxil_container_hash(const uint8_t* container,size_t size,uint8_t* out);
bool cvr_dxil_sign(uint8_t* container,size_t size);
bool cvr_dxil_bitcode_range(const uint8_t* container,size_t size,size_t* start,size_t* length);
bool cvr_dxil_placeholder_value(float value);
size_t cvr_dxil_find_float(const uint8_t* container,size_t size,float placeholder,size_t* out,size_t capacity);
bool cvr_dxil_write_float(uint8_t* container,size_t size,size_t bit,float value);
}
inline Hash ContainerHash(const uint8_t* container,size_t size){Hash h{};cvr_dxil_container_hash(container,size,h.data());return h;}
inline bool Sign(std::vector<uint8_t>& container){return cvr_dxil_sign(container.data(),container.size());}
inline bool BitcodeRange(const uint8_t* container,size_t size,size_t& start,size_t& length){
    return cvr_dxil_bitcode_range(container,size,&start,&length);
}
inline bool PlaceholderValue(float value){return cvr_dxil_placeholder_value(value);}
inline std::vector<size_t> FindFloat(const uint8_t* container,size_t size,float placeholder) {
    std::vector<size_t> found(8);
    const size_t count=cvr_dxil_find_float(container,size,placeholder,found.data(),found.size());
    if(count>found.size()){found.resize(count);cvr_dxil_find_float(container,size,placeholder,found.data(),found.size());}
    found.resize(count);return found;
}
inline bool WriteFloat(std::vector<uint8_t>& container,size_t bit,float value){
    return cvr_dxil_write_float(container.data(),container.size(),bit,value);
}
#else
// The hash stored at offset 4 of a container of `size` bytes.
Hash ContainerHash(const uint8_t* container,size_t size);
// Store ContainerHash at offset 4.
bool Sign(std::vector<uint8_t>& container);
// Byte range of the LLVM bitcode inside the container's DXIL part.
bool BitcodeRange(const uint8_t* container,size_t size,size_t& start,size_t& length);
bool PlaceholderValue(float);   // in [1, 2)
// Container bit offsets of every occurrence of the placeholder's VBR6 pattern in the bitcode.
std::vector<size_t> FindFloat(const uint8_t* container,size_t size,float placeholder);
// Write `value` (in [1, 2)) at a bit offset found for a placeholder. The hash is not updated.
bool WriteFloat(std::vector<uint8_t>& container,size_t bit,float value);
#endif
}
