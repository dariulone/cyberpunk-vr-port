// The exports src/Render/NativeGeometrySnapshots.cpp defines in the reference build and the Rust build defines in Rust (its rust-build
// modules): their reference definitions, for the single-pass tests built whole in the Rust build's form, whose
// Rust library leaves those modules out.
#include "Render/NativeGeometrySnapshots.hpp"
#include <atomic>
#include <cstdint>
extern "C" {
__declspec(dllexport) uint64_t CyberpunkVR_GeometryFreezeBytes{},CyberpunkVR_GeometryFreezeRecords{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_GeometryFreezeCount{},CyberpunkVR_GeometryFreezeEnable{},CyberpunkVR_GeometryFreezeFailure{},CyberpunkVR_GeometryFreezePending{},CyberpunkVR_GeometryFreezeSeq{};
}
