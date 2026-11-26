#pragma once

// RECOIL THAT REACHES THE HANDS: a damped spring per hand given a velocity impulse per round, sampled into the arm
// solve's IK target. See src/Anim/Recoil.cpp (or its Rust port, rust/crates/cvr-rig/src/vrik/recoil.rs, under
// CVR_RUST_VRIK). The controls and read-outs are defined in src/Anim/CharacterRigState.cpp, with the reasoning
// behind each number, and exported from the DLL for the ini and the overlay.

#include <cstdint>

extern "C" {
// One round out of the weapon in hand / out of the left gun while two are held (src/Hooks/WeaponAim.cpp).
void RecoilOnShot();
void RecoilOnShotLeft();
// Advance both springs to now: once per fresh solve, before either arm is built (src/Hooks/AnimPose.cpp).
void RecoilTick();
// How the throw is split between backwards and down, unit length.
void RecoilTravelMix(float* outBack, float* outDown);
// One hand's displacement in its own frame: metres of throw, radians of rise about its right axis.
void RecoilSample(int side, int weaponHand, float* outBackM, float* outRiseRad);
// When the last round left, on the recoil's own clock (milliseconds).
double RecoilLastShotMs();

__declspec(dllexport) extern int   CyberpunkVR_HandRecoil, CyberpunkVR_WeaponClass, CyberpunkVR_WeaponClassLeft;
__declspec(dllexport) extern float CyberpunkVR_HandRecoilBackCm, CyberpunkVR_HandRecoilBackMaxCm;
__declspec(dllexport) extern float CyberpunkVR_HandRecoilRiseDeg, CyberpunkVR_HandRecoilReturnMs;
__declspec(dllexport) extern float CyberpunkVR_WeaponKickDeg, CyberpunkVR_WeaponKickScale, CyberpunkVR_WeaponKickDegLeft;
__declspec(dllexport) extern float CyberpunkVR_RecoilDownPistol, CyberpunkVR_RecoilDownRifle, CyberpunkVR_RecoilDownShotgun;
__declspec(dllexport) extern float CyberpunkVR_PistolBackMul, CyberpunkVR_RifleRiseMul, CyberpunkVR_RifleBackMul;
__declspec(dllexport) extern float CyberpunkVR_ShotgunRiseMul, CyberpunkVR_ShotgunBackMul;
__declspec(dllexport) extern float CyberpunkVR_SniperRiseMul, CyberpunkVR_SniperBackMul, CyberpunkVR_RecoilScale;
__declspec(dllexport) extern float CyberpunkVR_TwoHandBackMul, CyberpunkVR_HandRecoilRefKick, CyberpunkVR_HandRecoilMaxDeg;
__declspec(dllexport) extern float CyberpunkVR_HandRecoilReturnPow, CyberpunkVR_HandRecoilReturnMinMs;
__declspec(dllexport) extern float CyberpunkVR_HandRecoilReturnMaxMs;
__declspec(dllexport) extern float CyberpunkVR_RecoilClimbFrac, CyberpunkVR_RecoilClimbMaxDeg, CyberpunkVR_RecoilClimbMs;
__declspec(dllexport) extern float CyberpunkVR_DebugRecoilClimbDeg, CyberpunkVR_DebugRecoilCm;
__declspec(dllexport) extern float CyberpunkVR_DebugRecoilAppliedDeg, CyberpunkVR_DebugRecoilPeakDeg;
__declspec(dllexport) extern float CyberpunkVR_DebugRecoilPeakDegLeft;
__declspec(dllexport) extern uint64_t CyberpunkVR_DebugRecoilShots, CyberpunkVR_DebugRecoilTicks;
}
