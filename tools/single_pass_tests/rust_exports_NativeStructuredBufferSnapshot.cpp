// The exports src/Render/NativeStructuredBufferSnapshot.cpp defines in the reference build and the Rust build defines in Rust (its rust-build
// modules): their reference definitions, for the single-pass tests built whole in the Rust build's form, whose
// Rust library leaves those modules out.
#include "Render/NativeStructuredBufferSnapshot.hpp"
#include <atomic>
#include <cstdint>
extern "C" {
__declspec(dllexport) uint64_t CyberpunkVR_StructuredBufferFinalizeDiagnostic[8]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StructuredBufferSnapshotsEnable{};
}
