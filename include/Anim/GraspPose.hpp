#pragma once
#include <cstdint>

// The fingers closed on what a hand holds, from the thing's shape and size (src/Anim/GraspPose.cpp, or its Rust port
// rust/crates/cvr-rig/src/vrik/grasp.rs under CVR_RUST_VRIK).
namespace cvr::anim {
// Each player pose pass: measures both palms, and writes the grasp of a hand that holds something.
void GraspFingers(uint8_t* boneBuf);
// Whether a hand (0 left, 1 right) holds a thing its fingers are closed on: from VRGraspSet to VRGraspClear.
bool GraspHolding(int side);
}

extern "C" {
// The natives' C interface (src/Natives/GraspNatives.cpp reads the script's parameters). side: 0 left, 1 right;
// shape: 0 box, 1 sphere, 2 cylinder, 3 cone, half sizes in metres; poses in the hand's weapon slot.
// The seat of a thing taken at (at, orientation): false, nothing written, until the palm is measured.
bool cvr_grasp_seat(int side, int shape, const float* half, const float* at, const float* orientation,
                    float* outPos, float* outRot);
bool cvr_grasp_set(int side, int shape, const float* half, const float* position, const float* orientation);
bool cvr_grasp_pin(int side, bool pinned);
bool cvr_grasp_blend(int side, float blend);
bool cvr_grasp_clear(int side);

// The read-outs (src/Anim/CharacterRigState.cpp): solves, passes that wrote a grasp, palms measured, segments that
// touched in the last solve; per side the last solve's kind and angles, the palm in the slot's frame, the normal's
// disagreement with the handedness, and the thumb search's cost, overlap, gap and goal distance.
__declspec(dllexport) extern volatile uint64_t CyberpunkVR_DebugGrasp[4];
__declspec(dllexport) extern volatile float CyberpunkVR_DebugGraspAngles[2][17];
__declspec(dllexport) extern volatile float CyberpunkVR_DebugGraspPalm[2][12];
__declspec(dllexport) extern volatile int CyberpunkVR_DebugGraspNormalFlip[2];
__declspec(dllexport) extern volatile float CyberpunkVR_DebugGraspThumb[2][4];
}