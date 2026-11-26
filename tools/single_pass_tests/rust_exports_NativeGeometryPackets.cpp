// The exports src/Render/NativeGeometryPackets.cpp defines in the reference build and the Rust build defines in Rust (its rust-build
// modules): their reference definitions, for the single-pass tests built whole in the Rust build's form, whose
// Rust library leaves those modules out.
#include "Render/NativeGeometryPackets.hpp"
#include <atomic>
#include <cstdint>
extern "C" {
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_GeometryOriginCompatible{},CyberpunkVR_GeometryOriginMode{1},CyberpunkVR_GeometryOriginVerified{},CyberpunkVR_GeometryPacketCount{0},CyberpunkVR_GeometryPacketDataCount{0},CyberpunkVR_GeometryPacketInputBlobBytes{0},CyberpunkVR_GeometryPacketInputCount{0},CyberpunkVR_GeometryPacketInputFailure{0},CyberpunkVR_GeometryPacketRequest{0},CyberpunkVR_GeometryPacketResolve{0},CyberpunkVR_GeometryPacketSeq{0},CyberpunkVR_GeometryPacketState{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_GeometryOriginStats[4]{};
__declspec(dllexport) uint64_t CyberpunkVR_GeometryPacketInputBlob{},CyberpunkVR_GeometryPacketInputs{};
}
