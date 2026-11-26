#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
#include "Hooks/Trampoline.hpp"
#include "Rust/CvrRust.h"

#include <windows.h>

// Lifted verbatim out of the core hub when the hooks were split into a file each. None of the
// arithmetic changed -- only that there is now exactly one copy of it, which is the whole point.
// See Hooks/Trampoline.hpp for why a second copy fails silently.

// The arena itself is Rust's (rust/crates/cvr-engine/src/trampoline.rs), so the C++ hooks and the Rust ones carve their
// stubs from the same 64 KB block -- two arenas would fragment the +/-2 GB window. See Hooks/Trampoline.hpp.
#if !CVR_RUST_VRIK
void* AllocateTrampoline(void* targetAddress, size_t size) {
    return cvr_engine_allocate_trampoline(targetAddress, size);
}
#endif

#if !CVR_RUST_VRIK
void WriteMovRaxImm64(uint8_t* code, int& pos, uintptr_t value) {
    code[pos++] = 0x48;
    code[pos++] = 0xB8;
    *reinterpret_cast<uint64_t*>(code + pos) = static_cast<uint64_t>(value);
    pos += 8;
}
#endif

#if !CVR_RUST_VRIK
void WriteMovR11Imm64(uint8_t* code, int& pos, uintptr_t value) {
    code[pos++] = 0x49;
    code[pos++] = 0xBB;
    *reinterpret_cast<uint64_t*>(code + pos) = static_cast<uint64_t>(value);
    pos += 8;
}
#endif
#endif
