#pragma once

// ================================================================================================
// Every script-callable native, declared once.
//
// The registration table in src/Natives/Registration.cpp names all of them by address, so it needs
// to see all of them -- and that is the only reason this header exists. It is generated from the
// definitions rather than typed: a native's signature is four parameters of which one varies, which
// is exactly the shape a human retypes wrongly and the linker then reports against a function that
// visibly exists.
//
// Adding a native means defining it in one of the src/Natives files, declaring it here, and naming
// it in the table. Three places, and the compiler catches two of them.
// ================================================================================================

#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Scripting/Natives/Generated/Vector4.hpp>
#include <RED4ext/Scripting/Natives/Generated/Quaternion.hpp>
#include <RED4ext/Scripting/Natives/Transform.hpp>

#include <cstdint>

void ArmVRAnimPosePlayer(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void DumpAnimControllerComponents(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void DumpAnimControllerFunctionDetails(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void DumpAnimControllerListeners(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void DumpAnimMemory(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void DumpAnimVTable(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, void* aOut, int64_t a4);
void DumpAnimationSystemCandidates(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void DumpAnimationSystemLookup(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void DumpInterestingAnimClassProperties(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void DumpPlayerAnimatedObjectRuntime(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void DumpPlayerBoneNames(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void DumpRootAnimatedObjectFloatArrayCandidates(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void DumpRootGraphVariables(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void DumpRootMetaRigTracks(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void DumpRuntimeClassFunctions(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void DumpVRCamAddr(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void DumpVRCamTrace(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void DumpVRFppComponents(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void DumpVRLiveProjectile(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t);
void DumpVRProjectileRtti(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void DumpWeaponAimHookStats(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void ForceHideVRFppArms(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void GetLeftVRHandPos(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4);
void GetLeftVRHandRot(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Quaternion* aOut, int64_t a4);
void GetLeftVRHandValid(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t a4);
void GetPlayerAnimParameterPersistentLastResult(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void GetRightVRHandPos(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4);
void GetRightVRHandRot(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Quaternion* aOut, int64_t a4);
void GetRightVRHandValid(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t a4);
void GetRootGraphPersistentLastResult(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void GetRootLiveTrackPersistentLastResult(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void GetRootMetaRigTrackPersistentLastResult(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void GetVRAnimPoseStats(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void GetVRFireDump(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, float* aOut, int64_t a4);
void GetVRFireHit(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, float* aOut, int64_t a4);
void GetVRMeleeTrigger(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t);
void GetVRPADump(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, float* aOut, int64_t);
void GetVRProjDump(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, float* aOut, int64_t a4);
void GetVRProjLiveDump(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, float* aOut, int64_t);
void GetVRProvDump(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, float* aOut, int64_t);
void GetVRSharedSlot(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, float* aOut, int64_t);
void GetVRSmokeCigVisualScaleY(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, float* aOut, int64_t a4);
void GetVRTrace(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, float* aOut, int64_t a4);
void GetVRTraceCaller(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, float* aOut, int64_t a4);
void GetVRWeaponAim(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void GetWeaponAimStat(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void InstallVRAnimPoseHook(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void InstallVRPrepareAttack(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t);
void InstallVRProvInstrument(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t);
void InstallWeaponAimHook(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void IsVRHandLinked(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t a4);
void LogVRDiag(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void ReadRootLiveTrackPreset(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void ResetVRProvCounts(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t);
void ResetVRTrace(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void RestoreVRFppArms(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void RunIKTargetAddTest(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetPlayerAnimParameter(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetPlayerAnimParameterPersistentPreset(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetPlayerAnimParameterPreset(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetRootGraphBoolVariableByName(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetRootGraphFloatPresetPersistent(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetRootGraphFloatVariableByName(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetRootGraphVectorPresetPersistent(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetRootGraphVectorVariableByName(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetRootLiveTrackPresetPersistent(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetRootMetaRigTrackPresetPersistent(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRAnimPoseBoneTest(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRAnimPoseDebug(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRBindBones(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRBindMode(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRBindParams(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRBoneDebugIndex(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, void* aOut, int64_t a4);
void SetVRCamAck(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t);
void SetVRCamBoneFreeze(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t);
void SetVRDiagCapture(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRElbowPole(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRElbowSwing(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRFireCamSnap(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRFireMode(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRFireOverrideTarget(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRFireScan(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRFireXform(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRFppChunkDebugBits(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, void* aOut, int64_t a4);
void SetVRFppChunkDebugComponentIndex(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, void* aOut, int64_t a4);
void SetVRFppChunkDebugEnabled(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, void* aOut, int64_t a4);
void SetVRFppChunkDebugHand(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, void* aOut, int64_t a4);
void SetVRGetOrient(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRHandOffset(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRHeadBone(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRHeadLocal(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRHeadingTest(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRIKAnimInputTestMode(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, void* aOut, int64_t a4);
void SetVRMeleeFire(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t);
void SetVRReloadOwnedHand(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t);
void SetVRSprintActive(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t);
void SetVRLocomotionState(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t);
void SetVRWeaponPoseState(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t);
void SetVRWeaponRaiseTransition(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t);
void SetVRMenuOpen(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
// The scanner's HUD layout, for the CyberpunkVRPort_ScannerHud redscript and its in-game editor.
// The forearm readout's watchdog: one 8-byte read per frame of the world widget's render-target
// handle, and a single component toggle on the frame it is swapped. See src/Natives/WristGuard.cpp for
// why that is the only repair and how the offset was located.
void VRWristGuard(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);

// Which surveillance camera the player took over, published from a CET tick because the plugin's own
// poll runs on the worker thread and must not call the script VM. See src/Natives/RemoteCamera.cpp.
void VRRemoteCamera(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
// Where the live player's camera component is, so MAIN can be told from a replacer's camera of the
// same name while a braindance is running.
void VRPlayerCamera(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
// Turn the braindance FOV write on or off live.
void VRBdSceneFov(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void VRBdFovWrite(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
// The HUD composite in the second eye during a braindance: the A/B for the white eye.
void VRHudSecondEye(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
// ...and the head rotation written into the braindance view.
void VRBdHeadWrite(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
// The scene's own camera pose, for a braindance: the second eye's lens, because neither side can name
// the object the scene renders through.
void VRSceneCamera(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
// The viewpoint offset of a taken-over camera, in the lens's own frame (right, forward, up), metres.
void VRDevCamOffset(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
// The taken-over entity's id, as a decimal string -- the exact identity of the camera to follow.
void VRTakeoverEntity(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
// 1 while a UI overlay that owns the B button is open (phone, radio port, vehicle list).
void VRUiPopup(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
// A braindance is running, and the FOV the game reports for its camera -- the only identity script can
// give for a camera that belongs to a scene.
void VRBraindance(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);

void VRScannerSlotGet(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, float* aOut, int64_t);
void VRScannerSlotSet(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t);
void VRScannerSlotSave(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t);
// 1 while a device screen (computer, terminal) is up; lets the right stick's Y reach the game's UI.
void SetVRDeviceScreen(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRMuzzlePos(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t);
// The aim line's hit point and surface normal, from the weapon module's own raycast.
void SetVRAimHit(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t);
// The equipped weapon's class, from its record: 0 unknown, 1 handgun/revolver, 2 rifle, 3 shotgun.
void SetVRWeaponClass(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t);
// The reach the carry's finger preview and its take gate share, metres (-1 = not carrying).
void GetVRCarryReach(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void* aOut, int64_t);
// ...and the same question already answered against the plugin's own radius: 1 near, 0 far, -1 unknown.
void GetVRCarryNear(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t);
// The resting RIGHT hand: capture it with empty hands, ask what came of it, switch it on and off.
void VRRestFingerCaptureRight(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t);
void VRRestFingerStatusRight(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t);
void VRRestFingerApplyRight(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t);
// Raised while the weapon is carried in the left hand: starts the wrist's blend out of the hold.
void SetVRCarryLeft(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t);
// Switches the two-hand weld off while the weapon is carried in the left hand.
void SetVRTwoHandSuppress(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t);
// One component of the left-hand carry offset, from the captured two-hand hold.
void GetVRCarryOffset(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void* aOut, int64_t);
void SetVRMuzzleQuat(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t);
void SetVRPairLead(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t);
void SetVRPairSlew(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t);
void SetVRPlayerYaw(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRPrepareAttackSwap(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t);
void SetVRProjCtrl(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRProjGateRva(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRProjOriginRow(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRProjSteer(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t);
void SetVRProvOverrideSlot(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t);
void SetVRProvQuatMode(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t);
void SetVRRightHandEntity(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, void* aOut, int64_t a4);
void SetVRShotCamera(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, void* aOut, int64_t a4);
void SetVRShotOrigin(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRShotSnap(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRSkipHmdTest(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRSmokeAnchorBone(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRSmokeCig(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRSmokeCigChunks(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRSmokeCigOffset(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRSmokeCigScaleY(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRSmokeCigVisualScale(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRSmokeFingers(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRSmokeFingersL(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRSmokeLeftCig(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRSmokeLighter(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRSmokeLighterOffset(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRSmokeMouthAnchor(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRSmokeMouthOffset(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRSmokeSmokeOffset(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRSmokeThumbFlickL(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRSmokeThumbPressL(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRTargetCtrl(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRTraceOverride(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRTriggerMode(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t);
void SetVRUseHeadRelative(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRWeaponAim(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRXformOverride(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void SetVRWeaponKick(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t);
void SetVRWeaponName(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t);
void SetVRZoomLevel(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t);
void StartVRCamTrace(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void StopVRCamTrace(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void TestAnimFloatInput(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void TestAnimFloatPreset(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void TestIKFeatureRoute(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void TestMeleeIKDataFeatureRoute(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void TestRootGraphFloatPreset(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void TestRootGraphVectorPreset(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void TestRootLiveTrackPreset(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void TestRootMetaRigTrackPreset(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void TestWeaponUserFeatureRoute(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void UpdateVRIKAnimInputs(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void VRBodyBonePos(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4);
void VRBodyGetAngVel(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4);
void VRBodyGetCOM(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4);
void VRBodyGetDims(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4);
void VRBodyGetPos(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4);
void VRBodyGetVel(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4);
void VRBodyIsKinematic(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void VRBodyProbe(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::CString* aOut, int64_t a4);
void VRBodySetAngVel(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t a4);
void VRBodySetKinematic(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t a4);
void VRBodySetPos(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t a4);
void VRBodySetSimMasks(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t a4);
void VRBodySetVel(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t a4);
void VRBoneModelPos(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4);
void VRBoneModelRot(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Quaternion* aOut, int64_t a4);
void VRBoneSnapCount(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void VRCamModelPos(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4);
// The player's own transform as the ENGINE has it this frame, so a script can measure how stale its own
// reading is. See the note at the definition in src/Natives/HandBody.cpp.
void VRPlayerEnginePos(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4);
void VRPlayerEngineRot(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Quaternion* aOut, int64_t a4);
void VRDumpBodyFuncs(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::CString* aOut, int64_t a4);
void VRHandRawModel(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4);
void VRHandRawRot(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Quaternion* aOut, int64_t a4);
void VRHandRawWorld(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4);
void VRHandStop(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t a4);
void VRHandStopDeadband(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t a4);
void VRHandStopModel(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t a4);
void VRHandStopRot(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t a4);
void VRPairUse(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void VRPairs(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void VRPalmCamLocal(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4);
void VRPalmModelPos(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4);
void VRPalmModelRot(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Quaternion* aOut, int64_t a4);
void VRPalmWorldPos(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4);
void VRPoseCensus(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void VRProbeBody(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::CString* aOut, int64_t a4);
void VRProjSteerTick(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t);
void VRRecordFK(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t);
void VRTwoHandCapture(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t);
void VRTwoHandStatus(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t);
void VRRestFingerCapture(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t);
void VRRestFingerStatus(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t);
void VRRestFingerApply(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t);
void VRReloadFingerApply(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t);
void VRReloadFingerBlend(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t);
void VRReloadFingerClear(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t);
void VRReloadFingerSet(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t);
void VRRigBone(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, float* aOut, int64_t);
void VRRigReset(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t);
void VRRigSignature(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t);
void VRRigStatus(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void VRRigTrack(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, float* aOut, int64_t);
void VRRigTrackWrite(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t);
void VRRigWrite(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void VRRigWriteAbs(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t);
void VRRigWriteClear(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t a4);
void VRRigWriteDiag(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void VRRigWriteOff(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t);
void VRRigWriteRot(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void VRRigWriteScale(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t);
void VRSmallRig(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void VRSmallRigUse(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void VRSmokeCaptureFingers(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void VRSmokeCaptureFingersL(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void VRSmokeDumpFingers(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void VRSmokeMouthDist(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, float* aOut, int64_t a4);
void VRSmokeMouthDistL(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, float* aOut, int64_t a4);
void VRSmokeMouthWorldPos(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4);
void VRSmokeMouthWorldRot(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Quaternion* aOut, int64_t a4);
void VRViewWorldPos(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4);
void VRViewWorldRot(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Quaternion* aOut, int64_t a4);
void VRWeaponPartLocal(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4);
void VRWeaponPartOffset(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t a4);
void VRWeaponPartSetIdx(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t a4);
void VRWeaponRigArm(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void VRWeaponRigNames(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::CString* aOut, int64_t a4);
void VRWeaponRigStatus(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void VRWeaponRigUse(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4);
void VRWorldDirToModel(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4);
void VRWorldToModel(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4);
