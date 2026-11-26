// The exports src/Render/NativeDepthPreparation.cpp defines in the reference build and the Rust build defines in Rust (its rust-build
// modules): their reference definitions, for the single-pass tests built whole in the Rust build's form, whose
// Rust library leaves those modules out.
#include "Render/NativePreparation.hpp"
#include "Render/NativeDepthPreparation.hpp"
#include <atomic>
#include <cstdint>
extern "C" {
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_EagerDepthCompatible{},CyberpunkVR_EagerDepthMode{},CyberpunkVR_EagerDepthVerified{};
__declspec(dllexport) uint32_t CyberpunkVR_EagerDepthStats[cvr::stereo::eager_depth::CounterCount]{};
}
