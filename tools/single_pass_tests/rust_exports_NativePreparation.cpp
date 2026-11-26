// The exports src/Render/NativePreparation.cpp defines in the reference build and the Rust build defines in Rust (its rust-build
// modules): their reference definitions, for the single-pass tests built whole in the Rust build's form, whose
// Rust library leaves those modules out.
#include "Render/NativePreparation.hpp"
#include <atomic>
#include <cstdint>
extern "C" {
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PreparationAllStages{},CyberpunkVR_PreparationCameraTrace{},CyberpunkVR_PreparationCount{},CyberpunkVR_PreparationFailure{},CyberpunkVR_PreparationPacketCount{},CyberpunkVR_PreparationPairCompatible{},CyberpunkVR_PreparationPairCompleted{},CyberpunkVR_PreparationPairMergeLimit{1},CyberpunkVR_PreparationPairMerged{},CyberpunkVR_PreparationPairRequest{},CyberpunkVR_PreparationPairState{},CyberpunkVR_PreparationPairVerified{},CyberpunkVR_PreparationRequest{},CyberpunkVR_PreparationRootCompatible{},CyberpunkVR_PreparationRootSkipped{},CyberpunkVR_PreparationRunDepth[2]{},CyberpunkVR_PreparationRunEyeCopies{},CyberpunkVR_PreparationRunEyeSubmissions{},CyberpunkVR_PreparationRunFailure{},CyberpunkVR_PreparationRunFrames{},CyberpunkVR_PreparationRunRequest{},CyberpunkVR_PreparationRunState{},CyberpunkVR_PreparationRunStop{},CyberpunkVR_PreparationSeq{},CyberpunkVR_PreparationState{};
__declspec(dllexport) uint32_t CyberpunkVR_PreparationGraphParentCount{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparationGraphParents{},CyberpunkVR_PreparationPackets{},CyberpunkVR_PreparationPairData[8]{},CyberpunkVR_PreparationPairEligibility[8]{},CyberpunkVR_PreparationPairRoot[8]{},CyberpunkVR_PreparationRecords{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_PreparationPairGeneration{};
}
