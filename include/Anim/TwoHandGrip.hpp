#pragma once

// ================================================================================================
// TwoHandGrip: the support hand on the weapon. See src/Anim/TwoHandGrip.cpp for what is captured
// and why the offset hangs off the right WRIST rather than off the weapon.
//
// Call order inside one pose apply, and it is load-bearing:
//   TwoHandCapture(boneBuf)                     -- only when asked, and only with VRIK off
//   TwoHandRight(targetR, hm, wristR, leftCtrl) -- in the right arm's branch, BEFORE handRot is built
//   TwoHandLeft(target, handRot)                 -- in the left arm's branch, before the stop zone
//   TwoHandFingers(boneBuf)                      -- after the resting pose, before the reload layer
// ================================================================================================

#include "Anim/VrikState.hpp"

#include <cstdint>

namespace cvr {
namespace anim {

void TwoHandCapture(uint8_t* boneBuf);
void TwoHandRight(const float* targetR, float* hm, const float* wristR, const float* leftCtrlModel);
bool TwoHandLeft(float* target, float* handRot);
// Selects the hold for the weapon currently in hand -- needed by the carry offset getter, which runs
// outside the pose pass.
void SelectHoldForCarry();
void TwoHandFingers(uint8_t* boneBuf);
// Disk half, called from the frame loop: never from the pose path.
void TwoHandTick();

}  // namespace anim
}  // namespace cvr

// C linkage (src/Anim/TwoHandGrip.cpp, or rust/crates/cvr-rig/src/vrik/two_hand.rs under CVR_RUST_VRIK).
extern "C" {
// The weapon's carry into the left hand and its return: the welded wrist rotation faded out, the drawn left wrist,
// and the right wrist sprung back onto the weapon; the slot offset the carry hangs the weapon by.
void CarryLeftBlend(float* handRot);
void CarryRecordLeft(const float* pos, const float* rot);
void CarryReturnRight(float* target, float* hm, const float* wristR);
float VRTwoHandCarryOffset(int idx);
// The switches and the live state (src/Anim/CharacterRigState.cpp, exported for the ini, the overlay and the natives).
__declspec(dllexport) extern int   CyberpunkVR_TwoHandGrip, CyberpunkVR_TwoHandSuppress, CyberpunkVR_DebugTwoHandHoldIdx, CyberpunkVR_CarryLeft;
__declspec(dllexport) extern int   CyberpunkVR_CarryReturnHand, CyberpunkVR_TwoHandAim, CyberpunkVR_TwoHandActive, CyberpunkVR_DebugTwoHandHave;
__declspec(dllexport) extern int   CyberpunkVR_TwoHandCaptureReq, CyberpunkVR_DebugTwoHandRefused, CyberpunkVR_DebugTwoHandSaved, CyberpunkVR_DebugTwoHandLoaded;
__declspec(dllexport) extern float CyberpunkVR_CarryBlendMs, CyberpunkVR_CarryBlendZeta, CyberpunkVR_DebugCarryBlend;
__declspec(dllexport) extern float CyberpunkVR_TwoHandRadius, CyberpunkVR_TwoHandRecoil, CyberpunkVR_TwoHandFadeS;
__declspec(dllexport) extern float CyberpunkVR_TwoHandEngageMs, CyberpunkVR_TwoHandEngageZeta, CyberpunkVR_TwoHandWeight;
__declspec(dllexport) extern float CyberpunkVR_TwoHandLever, CyberpunkVR_TwoHandReleaseMs, CyberpunkVR_CarryReturnMs;
__declspec(dllexport) extern float CyberpunkVR_CarryReturnZeta, CyberpunkVR_DebugCarryReturn, CyberpunkVR_CarryGripRadius;
__declspec(dllexport) extern float CyberpunkVR_CarryGripMs, CyberpunkVR_CarryGripZeta, CyberpunkVR_CarryGripBlend;
__declspec(dllexport) extern float CyberpunkVR_DebugCarryReach, CyberpunkVR_TwoHandAimGain, CyberpunkVR_TwoHandLeverFull;
__declspec(dllexport) extern float CyberpunkVR_TwoHandAimTau, CyberpunkVR_TwoHandAimMaxDeg, CyberpunkVR_TwoHandAimDeadDeg;
__declspec(dllexport) extern float CyberpunkVR_TwoHandAimReleaseMs, CyberpunkVR_DebugTwoHandAimDeg, CyberpunkVR_TwoHandBreakRadius;
__declspec(dllexport) extern float CyberpunkVR_TwoHandBreakMs, CyberpunkVR_TwoHandBlend, CyberpunkVR_DebugTwoHandDist;
__declspec(dllexport) extern float CyberpunkVR_DebugTwoHandBaseMm;
// A pump's fore-end under the support hand (SetVRTwoHandSlide, a deadline on VRIK_ClockMs), and the left controller
// as the hold read it (GetVRTwoHandLeftCtrl). Defined in src/Anim/CharacterRigState.cpp.
extern float    g_VRTwoHandSlide[3];
extern uint64_t g_VRTwoHandSlideUntil;
extern float    g_VRTwoHandCtrlL[3];
extern int      g_VRTwoHandCtrlLValid;
}