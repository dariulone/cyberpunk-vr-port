// The exports src/Render/StereoDescriptorAudit.cpp defines in the reference build and the Rust build defines in Rust (its rust-build
// modules): their reference definitions, for the single-pass tests built whole in the Rust build's form, whose
// Rust library leaves those modules out.
#include "Render/StereoDescriptorAudit.hpp"
#include <atomic>
#include <cstdint>
extern "C" {
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_DescriptorBindingCount{},CyberpunkVR_DescriptorCaptureFailure{},CyberpunkVR_DescriptorCaptureSeq{},CyberpunkVR_DescriptorDenseActive{1},CyberpunkVR_DescriptorDenseSlots{1},CyberpunkVR_DescriptorDrawCount{},CyberpunkVR_DescriptorIdCount{},CyberpunkVR_DescriptorLiveMetadata{1},CyberpunkVR_DescriptorLiveMetadataActive{},CyberpunkVR_DescriptorPendingLists{},CyberpunkVR_DescriptorValueCount{};
__declspec(dllexport) uint64_t CyberpunkVR_DescriptorBindings{},CyberpunkVR_DescriptorDraws{},CyberpunkVR_DescriptorHeapSlots{},CyberpunkVR_DescriptorIds{},CyberpunkVR_DescriptorStoreStats[6]{},CyberpunkVR_DescriptorValues{};
__declspec(dllexport) uint32_t CyberpunkVR_DescriptorRootCount{};
}
