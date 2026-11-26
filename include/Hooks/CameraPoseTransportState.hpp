#pragma once
// CameraPoseTransport's diagnostics (src/Hooks/CameraPoseTransport.cpp, or rust/crates/cvr-rig/src/vrik/
// pose_transport.rs): the records the overlay and the probes read. Defined in src/Camera/CameraLinkState.cpp for both
// builds; the Rust port mirrors these layouts.
#include "Camera/PoseAddressLedger.hpp"
#include <atomic>
#include <cstdint>

struct PoseBlendDebug {
    uint64_t calls{},labelled{},rejected{};
    uintptr_t director{};
    uint32_t count{},accepted{};
    float weights[8]{};
    uint64_t poseIds[8]{};
    uintptr_t components[8]{};
    cvr::camera::PoseFingerprint inputs[8]{},output{};
};
static_assert(sizeof(PoseBlendDebug)==456,"mirrored by PoseBlendDebug in rust/crates/cvr-rig/src/vrik/pose_transport.rs");
struct ExternalCameraDebug {
    uint64_t writes{},stampUs{},poseId{},origin{},component{};
    int32_t position[3]{};
    float rotation[4]{},head[7]{},fov{};
};
static_assert(sizeof(ExternalCameraDebug)==104);
extern "C" {
__declspec(dllexport) extern PoseBlendDebug CyberpunkVR_PoseBlendDebug;
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_PoseBlendDebugSeq;
__declspec(dllexport) extern std::atomic<uint64_t> CyberpunkVR_PoseMainReadMissing[2]; // blended / override setup
__declspec(dllexport) extern ExternalCameraDebug CyberpunkVR_ExternalCameraDebug[2];
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_ExternalCameraDebugSeq[2];
__declspec(dllexport) extern std::atomic<uint64_t> CyberpunkVR_ExternalSelectionStates[4]; // fresh / retained / inactive / unavailable
__declspec(dllexport) extern std::atomic<uint64_t> CyberpunkVR_ExternalNotify[3]; // MAIN pushes / expired or replaced / disabled or detached
}
