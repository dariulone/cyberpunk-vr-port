#pragma once

// ================================================================================================
// VrikHook -- the pose-apply detour, and the few things outside it may call.
//
// This used to be a 4,400-line header carrying its whole implementation, included by exactly one
// file. Of the 43 functions it defined, a handful are reached from outside; the rest were reachable
// only because a header cannot keep a secret. The implementation is src/Anim/VrikHook.cpp; the
// shared ABI is Anim/VrikState.hpp.
// ================================================================================================

#include "Anim/VrikState.hpp"

// The public surface, copied from the definitions rather than typed. Guessing one of these was how
// `const float* SharedPose()` came to be declared for a `float SharedPose(int)` -- which compiles on
// both sides and fails only at link time, naming a function that exists.
bool InstallAnimPoseHook();
extern "C" float SharedPose(int i);
extern "C" void RefreshHandsSnapshot();
extern "C" bool VRIK_IsReadable(const void* p, size_t n);
// C linkage: the solve's helpers (src/Anim/CharacterRig.cpp, or rust/crates/cvr-rig/src/vrik/rig.rs).
extern "C" {
void VRIK_QuatConj(const float* q, float* o);
void VRIK_QuatMul(const float* a, const float* b, float* o);
void VRIK_QuatNorm(float* q);
void VRIK_QuatRotateVec(const float* q, const float* v, float* o);
}
