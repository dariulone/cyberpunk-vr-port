// The exports src/Render/NativeGeometryLeases.cpp defines in the reference build and the Rust build defines in Rust (its rust-build
// modules): their reference definitions, for the single-pass tests built whole in the Rust build's form, whose
// Rust library leaves those modules out.
#include "Render/NativeGeometryLeases.hpp"
#include <atomic>
#include <cstdint>
extern "C" {
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_GeometryLeaseCompatible{},CyberpunkVR_GeometryLeaseDrawCount{},CyberpunkVR_GeometryLeaseFailure{},CyberpunkVR_GeometryLeaseResourceCount{},CyberpunkVR_GeometryLeaseSeq{};
__declspec(dllexport) uint64_t CyberpunkVR_GeometryLeaseDraws{},CyberpunkVR_GeometryLeaseReadStates{},CyberpunkVR_GeometryLeaseResources{};
}
