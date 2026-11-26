// The guarded copies the Rust half reads and writes engine memory through: Rust cannot catch an access violation,
// so each float goes through MemorySafe.hpp's __try, element by element and stopping at the first that faults, as
// ReadFloatArraySafe / WriteFloatArraySafe (src/Core/Log.cpp) do. Both builds; tools/vrik_tests links it as is.
#include "Utils/MemorySafe.hpp"
#include <cstddef>

#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's are rust/crates/cvr-rig/src/vrik/env.rs's (the vrik tests' stubs call these)
extern "C" {
bool cvr_cpp_read_floats_safe(const float* src, float* out, size_t count) {
    if (!src || !out) return false;
    for (size_t i = 0; i < count; ++i)
        if (!ReadFloatSafe(reinterpret_cast<uintptr_t>(src + i), &out[i])) return false;
    return true;
}
bool cvr_cpp_write_floats_safe(float* dst, const float* values, size_t count) {
    if (!dst || !values) return false;
    for (size_t i = 0; i < count; ++i)
        if (!WriteFloatSafe(reinterpret_cast<uintptr_t>(dst + i), values[i])) return false;
    return true;
}
}
#endif