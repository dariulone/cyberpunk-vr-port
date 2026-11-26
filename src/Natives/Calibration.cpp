#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
// Calibration -- natives lifted out of src/Natives/Natives.cpp, which held every family at once.
//
// Calibration and the diagnostic writer: the measured anatomy the solve is scaled
// to, and the dump that records what it measured.
//
// The cut was placed by the seam map and then SNAPPED to the nearest point at brace depth zero.
// Boundaries taken from line numbers alone are how a split lands in the middle of a function; the
// check is cheap and it is the same lesson as every other generator in this restructure.
#if !CVR_RUST_VRIK
// The C++ build's calibration natives; the Rust build's are rust/crates/cvr-rig/src/vrik/calibration.rs.
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/GameEngine.hpp>
#include <sstream>
#include <locale>
#include <clocale>
#include "Utils/SharedSlots.hpp"   // CyberpunkVR_Hands_Shared slot map (single source of truth)
#include <RED4ext/Containers/StaticArray.hpp>
#include <RED4ext/Scripting/Natives/ScriptGameInstance.hpp>
#include <RED4ext/Scripting/Utils.hpp>
#include <RED4ext/Scripting/Functions.hpp>
#include <RED4ext/Scripting/CProperty.hpp>
#include <RED4ext/Scripting/Natives/Generated/WorldPosition.hpp>
#include <RED4ext/Scripting/Natives/Transform.hpp>
#include <RED4ext/Scripting/Natives/animRig.hpp>
#include <RED4ext/Scripting/Natives/Generated/Vector4.hpp>
#include <RED4ext/Scripting/Natives/Generated/Quaternion.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimGraph.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimFeature_IK.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimFeature_MeleeIKData.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimFeature_WeaponUser.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimVariableBool.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimVariableContainer.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimVariableFloat.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimVariableInt.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimVariableQuaternion.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimVariableTransform.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimVariableVector.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/AnimationControlBinding.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/AnimInputSetterAnimFeature.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/AnimInputSetterFloat.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/AnimInputSetterVector.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/IBinding.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/IKTargetAddEvent.hpp>
#include <RED4ext/Scripting/Natives/Generated/red/Event.hpp>
#include <RED4ext/Scripting/Natives/entEntity.hpp>
#include <RED4ext/Scripting/Natives/entAnimationControllerComponent.hpp>
#include <RED4ext/Scripting/Natives/entIPlacedComponent.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/AnimatedComponent.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/StaticOrientationProvider.hpp>
#include <RED4ext/Scripting/Natives/worldAnimationSystem.hpp>
#include <RED4ext/Scripting/Natives/worldAnimationSystemScriptInterface.hpp>
#include <RED4ext/Scripting/Natives/entSkinnedMeshComponent.hpp>
#include <RED4ext/Scripting/Natives/entAnimationControllerComponent.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/GarmentSkinnedMeshComponent.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/MeshComponent.hpp>
#include <windows.h>
#include <algorithm>
#include <cctype>
#include <cstring>
#include <atomic>
#include <cstdint>
#include <fstream>
#include <utility>
#include <iomanip>
#include <string>
#include "Anim/VrikHook.hpp"
#include "Anim/WeaponAim.hpp"
#include "Natives/NativeState.hpp"
#include "Natives/NativeHelpers.hpp"
#include <MinHook.h>
#include "Natives/NativeFunctions.hpp"
#include "Natives/NativeHelpers.hpp"
#include "Natives/NativeState.hpp"




void SetVRBindMode(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    int32_t mode = 0;
    RED4ext::GetParameter(aFrame, &mode);
    aFrame->code++;
    g_VRBind = mode;
    // Re-arm the pose reference captures (hips lock + girdle translation pin): the player
    // re-toggles VRIK while standing unarmed to recalibrate the anatomical references.
    if (mode > 0) g_VRPoseCapGen = g_VRPoseCapGen + 1;
    if (aOut) *aOut = 1;
}

// Per-hand reach scale + position offset. hand: 0 = right, 1 = left, else = both.
// Also keeps the legacy global scale/offset (modes 1..3) in sync when hand == both.
void SetVRBindParams(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    float scale = 1.0f, x = 0.0f, y = 0.0f, z = 0.0f;
    int32_t axis = 1, hand = 2;
    RED4ext::GetParameter(aFrame, &scale);
    RED4ext::GetParameter(aFrame, &x);
    RED4ext::GetParameter(aFrame, &y);
    RED4ext::GetParameter(aFrame, &z);
    RED4ext::GetParameter(aFrame, &axis);
    RED4ext::GetParameter(aFrame, &hand);
    aFrame->code++;

    g_VRBindAxis = axis;
    if (hand != 1) { g_VRScaleR = scale; g_VROffRX = x; g_VROffRY = y; g_VROffRZ = z; }
    if (hand != 0) { g_VRScaleL = scale; g_VROffLX = x; g_VROffLY = y; g_VROffLZ = z; }
    if (hand == 2) { // also drive legacy globals used by modes 1..3
        g_VRBindScale = scale; g_VRBindOffX = x; g_VRBindOffY = y; g_VRBindOffZ = z;
    }

    if (aOut) *aOut = 1;
}

// Per-hand elbow pole spin in degrees (rotates the IK elbow direction around the
// shoulder->hand axis). hand: 0 = right, 1 = left, else = both.
void SetVRElbowPole(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    float angle = 0.0f;
    int32_t hand = 2;
    RED4ext::GetParameter(aFrame, &angle);
    RED4ext::GetParameter(aFrame, &hand);
    aFrame->code++;
    if (hand != 1) g_VRElbowPoleR = angle;
    if (hand != 0) g_VRElbowPoleL = angle;
    if (aOut) *aOut = 1;
}

// Per-hand elbow-swing gain. Scales the arm-swing position heuristic that swings the elbow
// as the hand sweeps through its arc. 1.0 = the faithful heuristic, 0 = elbow locked straight
// down, negative = swing the other way. hand: 0 = right, 1 = left, else = both.
void SetVRElbowSwing(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    float gain = 1.0f;
    int32_t hand = 2;
    RED4ext::GetParameter(aFrame, &gain);
    RED4ext::GetParameter(aFrame, &hand);
    aFrame->code++;
    if (hand != 1) g_VRElbowSwingR = gain;
    if (hand != 0) g_VRElbowSwingL = gain;
    if (aOut) *aOut = 1;
}

// Constant wrist-orientation correction (degrees, hand-local pitch/yaw/roll) per hand.
// hand: 0 = right, 1 = left, anything else = both. Lets the player dial in the palm/finger
// alignment live from the CET console without a rebuild. Applied as handRot = mapQuat *
// wristCorr in VRIK_BuildHandTarget. Calibrated defaults: R(0,-90,-21), L(-180,-90,-15) (Runtimes/WristBase.hpp).
void SetVRHandOffset(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    float pitch = 0.0f, yaw = 0.0f, roll = 0.0f;
    int32_t hand = 2; // default: both
    RED4ext::GetParameter(aFrame, &pitch);
    RED4ext::GetParameter(aFrame, &yaw);
    RED4ext::GetParameter(aFrame, &roll);
    RED4ext::GetParameter(aFrame, &hand);
    aFrame->code++;

    const float d2r = 0.01745329252f * 0.5f;
    float cp = std::cos(pitch*d2r), sp = std::sin(pitch*d2r);
    float cy = std::cos(yaw*d2r),   sy = std::sin(yaw*d2r);
    float cr = std::cos(roll*d2r),  sr = std::sin(roll*d2r);
    // XYZ (pitch about X, yaw about Y, roll about Z) intrinsic compose.
    float qi = sp*cy*cr + cp*sy*sr;
    float qj = cp*sy*cr - sp*cy*sr;
    float qk = cp*cy*sr + sp*sy*cr;
    float qr = cp*cy*cr - sp*sy*sr;

    if (hand != 1) { g_VRWristR_I = qi; g_VRWristR_J = qj; g_VRWristR_K = qk; g_VRWristR_R = qr; }
    if (hand != 0) { g_VRWristL_I = qi; g_VRWristL_J = qj; g_VRWristL_K = qk; g_VRWristL_R = qr; }

    if (aOut) *aOut = 1;
}

void SetVRBindBones(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t leftIdx = 23, rightIdx = 24;
    RED4ext::GetParameter(aFrame, &leftIdx);
    RED4ext::GetParameter(aFrame, &rightIdx);
    aFrame->code++;
    
    g_VRLeftBoneIdx = leftIdx;
    g_VRRightBoneIdx = rightIdx;
    
    if (aOut) *aOut = 1;
}

// Override the resolved head bone index (calibration). -1 disables head-relative.
void SetVRHeadBone(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t idx = -1;
    RED4ext::GetParameter(aFrame, &idx);
    aFrame->code++;
    g_VRHeadBoneIdx = idx;
    if (aOut) *aOut = g_VRHeadBoneIdx;
}

// Toggle head-relative hand composition (1 = on, 0 = write head-local offset directly).
void SetVRUseHeadRelative(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t on = 1;
    RED4ext::GetParameter(aFrame, &on);
    aFrame->code++;
    g_VRUseHeadRelative = on ? 1 : 0;
    if (aOut) *aOut = g_VRUseHeadRelative;
}

void SetVRDiagCapture(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t on = 0;
    RED4ext::GetParameter(aFrame, &on);
    aFrame->code++;
    g_VRDiagCapture = on ? 1 : 0;
    if (aOut) *aOut = g_VRDiagCapture;
}

// Diagnostic: logs the gizmo-computed world target (camPos + camQuat*mapLocalPos)
// next to the actual character arm-bone poses captured from the live pose buffer
// (g_VRDiagBones, snapshotted pre-write by the hook when SetVRDiagCapture(1)).
// The decisive lines compare (bufHand - bufHead) against (gizmoWorld - camPos):
// if they match, the bone buffer is model-space and head-relative IK is valid.
// Call from Lua each frame (or on a hotkey) passing the FPP camera world pose.
// Core diag writer, callable without a script frame (also used by the F10-overlay
// trigger path). camPos may be 0 -- the decisive comparison lines (gizmoWorld-cam.pos
// and bufHand-bufHead) are independent of the camera's absolute position.
void WriteVRDiagCore(float camX, float camY, float camZ,
                            float qi, float qj, float qk, float qr) {
    int32_t aOutLocal = 0; int32_t* aOut = &aOutLocal;
    EnsureSharedMemory();
    if (aOut) *aOut = 0;

    // Right-hand gizmo target, identical math to the CET gizmo (init.lua).
    float raw[3] = { 0, 0, 0 };
    if (g_pSharedHands) { raw[0] = g_pSharedHands[9]; raw[1] = g_pSharedHands[10]; raw[2] = g_pSharedHands[11]; }
    float local[3] = { raw[0], -raw[2], raw[1] };          // mapLocalPos: (x, -z, y)
    float camQuat[4] = { qi, qj, qk, qr };
    float worldOff[3];
    VRIK_QuatRotateVec(camQuat, local, worldOff);
    float gizmo[3] = { camX + worldOff[0], camY + worldOff[1], camZ + worldOff[2] };

    std::ofstream out(VRDiagPath("vrik_diag.txt"), std::ios::app);
    if (!out.is_open()) { if (aOut) *aOut = -1; return; }
    out << std::fixed << std::setprecision(4);
    out << "==== LogVRDiag ====\n";
    out << "cam.pos  = (" << camX << ", " << camY << ", " << camZ << ")\n";
    out << "cam.quat = (" << qi << ", " << qj << ", " << qk << ", " << qr << ")\n";
    out << "VR.rawR  = (" << raw[0] << ", " << raw[1] << ", " << raw[2] << ")\n";
    out << "gizmoWorld(R) = (" << gizmo[0] << ", " << gizmo[1] << ", " << gizmo[2] << ")\n";
    // HMD orientation rel to base (slots 16..19) + head-independent base-frame offset.
    if (g_pSharedHands) {
        const float* h = &g_pSharedHands[16];
        float baseOff[3] = {
            (h[3]*h[3]-h[0]*h[0]-h[1]*h[1]-h[2]*h[2])*raw[0] + 2.0f*(h[0]*h[1]-h[3]*h[2])*raw[1] + 2.0f*(h[0]*h[2]+h[3]*h[1])*raw[2],
            2.0f*(h[0]*h[1]+h[3]*h[2])*raw[0] + (h[3]*h[3]-h[0]*h[0]+h[1]*h[1]-h[2]*h[2])*raw[1] + 2.0f*(h[1]*h[2]-h[3]*h[0])*raw[2],
            2.0f*(h[0]*h[2]-h[3]*h[1])*raw[0] + 2.0f*(h[1]*h[2]+h[3]*h[0])*raw[1] + (h[3]*h[3]-h[0]*h[0]-h[1]*h[1]+h[2]*h[2])*raw[2],
        };
        out << "hmdRel   = (" << h[0] << ", " << h[1] << ", " << h[2] << ", " << h[3] << ")\n";
        out << "baseOff(hmdRel*raw) = (" << baseOff[0] << ", " << baseOff[1] << ", " << baseOff[2] << ")\n";
        out << "mapLocal(x,-z,y)    = (" << baseOff[0] << ", " << -baseOff[2] << ", " << baseOff[1] << ")\n";
    }
    out << "headIdx=" << g_VRHeadBoneIdx << " rightIdx=" << g_VRRightBoneIdx << " leftIdx=" << g_VRLeftBoneIdx
        << " diagCapture=" << g_VRDiagCapture << " lastBoneBuf=0x" << std::hex << g_AnimPoseLastBoneBuf << std::dec << "\n";

    struct NamedBone { int idx; const char* name; };
    const NamedBone bones[] = {
        {2, "Hips"}, {13, "Spine3"}, {16, "Neck"}, {22, "Head"},
        {15, "RightShoulder"}, {18, "RightArm"}, {21, "RightForeArm"}, {24, "RightHand"},
        {14, "LeftShoulder"},  {17, "LeftArm"},  {20, "LeftForeArm"},  {23, "LeftHand"},
    };
    for (const auto& b : bones) {
        if (b.idx < 0 || b.idx >= 32) continue;
        const float* d = &g_VRDiagBones[b.idx * 7];
        out << "bone[" << b.idx << "] " << b.name
            << " pos=(" << d[0] << ", " << d[1] << ", " << d[2] << ")"
            << " quat=(" << d[3] << ", " << d[4] << ", " << d[5] << ", " << d[6] << ")\n";
    }

    // Decisive comparison: model-space buffer => these two offsets match.
    const float* head = &g_VRDiagBones[22 * 7];
    const float* hand = &g_VRDiagBones[24 * 7];
    const float* sh   = &g_VRDiagBones[15 * 7];
    const float* fa   = &g_VRDiagBones[21 * 7];
    out << "bufHand24 - bufHead22 = (" << (hand[0]-head[0]) << ", " << (hand[1]-head[1]) << ", " << (hand[2]-head[2]) << ")\n";
    out << "gizmoWorld  - cam.pos = (" << (gizmo[0]-camX) << ", " << (gizmo[1]-camY) << ", " << (gizmo[2]-camZ) << ")\n";
    // Rest bone lengths (for the future IK): shoulder->forearm->hand.
    float l1 = std::sqrt((fa[0]-sh[0])*(fa[0]-sh[0]) + (fa[1]-sh[1])*(fa[1]-sh[1]) + (fa[2]-sh[2])*(fa[2]-sh[2]));
    float l2 = std::sqrt((hand[0]-fa[0])*(hand[0]-fa[0]) + (hand[1]-fa[1])*(hand[1]-fa[1]) + (hand[2]-fa[2])*(hand[2]-fa[2]));
    out << "restLen shoulder->forearm=" << l1 << " forearm->hand=" << l2 << "\n";

    // Full-IK (mode 4) intermediates from the last solve, in model space.
    out << "IK chain: rArm=" << g_VRRightUpperArmIdx << " rFore=" << g_VRRightForeArmIdx
        << " rHand=" << g_VRRightBoneIdx << " boneCount=" << g_VRBoneCount
        << " bind=" << g_VRBind << "\n";
    out << "IK target(model)   = (" << g_VRIKDbgTarget[0] << ", " << g_VRIKDbgTarget[1] << ", " << g_VRIKDbgTarget[2] << ")\n";
    out << "IK shoulder(model) = (" << g_VRIKDbgShoulder[0] << ", " << g_VRIKDbgShoulder[1] << ", " << g_VRIKDbgShoulder[2] << ")\n";
    out << "IK elbow(model)    = (" << g_VRIKDbgElbow[0] << ", " << g_VRIKDbgElbow[1] << ", " << g_VRIKDbgElbow[2] << ")\n";
    out << "IK hand body(lx,ly,lz,cross) = (" << g_VRIKDbgLocal[0] << ", " << g_VRIKDbgLocal[1] << ", " << g_VRIKDbgLocal[2] << ", " << g_VRIKDbgLocal[3] << ")\n";
    out << "IK lens upper=" << g_VRIKDbgLens[0] << " fore=" << g_VRIKDbgLens[1]
        << " scale=" << g_VRBindScale << " yaw=" << g_VRPlayerYaw << "\n";

    // ---- Phase-1 gate: gizmo-exact 1:1 validation -------------------------------
    // Reconstruct the camera (HMD) + the right gizmo hand in MODEL space from the world
    // transforms (world->model = Rz(-yaw)) and compare to what the IK actually used:
    //   * camModel  ~= head bone model pos  (small, ~constant eye offset) -> world->model OK.
    //   * gizmoModel ~= IK target(model)     (~0)                          -> 1:1 target OK.
    // Both require the 11-param SetVRPlayerYaw push (g_VRCamPosValid=1).
    {
        // Use the REAL camera/entity transforms the hook uses (the cam.pos params above are 0
        // when the diag is triggered from the overlay path). world->model = conj(entityQuat).
        float entQ[4] = { g_VREntityQI, g_VREntityQJ, g_VREntityQK, g_VREntityQR };
        float en = std::sqrt(entQ[0]*entQ[0]+entQ[1]*entQ[1]+entQ[2]*entQ[2]+entQ[3]*entQ[3]);
        if (en > 1e-4f) { entQ[0]/=en; entQ[1]/=en; entQ[2]/=en; entQ[3]/=en; } else { entQ[0]=0;entQ[1]=0;entQ[2]=0;entQ[3]=1; }
        float invEnt[4] = { -entQ[0], -entQ[1], -entQ[2], entQ[3] };
        auto toModel = [&](float x, float y, float z, float* o) {
            float v[3] = { x, y, z }; VRIK_QuatRotateVec(invEnt, v, o);
        };
        // Real-world right gizmo hand from the hook's camera pose (local = mapLocal(raw), above).
        float camQ2[4] = { g_VRCamI, g_VRCamJ, g_VRCamK, g_VRCamR };
        float woff[3]; VRIK_QuatRotateVec(camQ2, local, woff);
        float gizR[3] = { g_VRCamPosX + woff[0], g_VRCamPosY + woff[1], g_VRCamPosZ + woff[2] };
        float camModelP[3];  toModel(g_VRCamPosX - g_VREntityPosX, g_VRCamPosY - g_VREntityPosY, g_VRCamPosZ - g_VREntityPosZ, camModelP);
        float gizmoModel[3]; toModel(gizR[0]-g_VREntityPosX, gizR[1]-g_VREntityPosY, gizR[2]-g_VREntityPosZ, gizmoModel);
        out << "camPosValid=" << g_VRCamPosValid
            << " camPos=(" << g_VRCamPosX << ", " << g_VRCamPosY << ", " << g_VRCamPosZ << ")"
            << " entityPos=(" << g_VREntityPosX << ", " << g_VREntityPosY << ", " << g_VREntityPosZ << ")\n";
        out << "entityQuat = (" << g_VREntityQI << ", " << g_VREntityQJ << ", " << g_VREntityQK << ", " << g_VREntityQR << ")\n";
        out << "camModel(reconstructed) = (" << camModelP[0] << ", " << camModelP[1] << ", " << camModelP[2] << ")\n";
        if (g_VRHeadBoneIdx >= 0 && g_VRHeadBoneIdx < 256) {
            const float* hp = g_fkPos[g_VRHeadBoneIdx];
            out << "head bone(model FK)     = (" << hp[0] << ", " << hp[1] << ", " << hp[2] << ")"
                << "  delta camModel-head = (" << (camModelP[0]-hp[0]) << ", " << (camModelP[1]-hp[1]) << ", " << (camModelP[2]-hp[2]) << ")\n";
        }
        out << "gizmoModel(reconstructed) = (" << gizmoModel[0] << ", " << gizmoModel[1] << ", " << gizmoModel[2] << ")\n";
        out << "GATE delta gizmoModel - IKtarget = (" << (gizmoModel[0]-g_VRIKDbgTarget[0]) << ", "
            << (gizmoModel[1]-g_VRIKDbgTarget[1]) << ", " << (gizmoModel[2]-g_VRIKDbgTarget[2]) << ")  (want ~0)\n";
        out << "userArmLen R/L=" << g_VRUserArmLenR << "/" << g_VRUserArmLenL
            << " eyeHeight=" << g_VRUserEyeHeight << "\n";
        if (g_pSharedHands) {
            // Render-view pose channel + fixed-point scale auto-detect result.
            out << "viewPose raw[108..110]=(" << g_pSharedHands[108] << ", " << g_pSharedHands[109]
                << ", " << g_pSharedHands[110] << ", flag=" << g_pSharedHands[111] << ")"
                << " scaleUsed=" << g_vrikViewScaleUsed
                << (g_vrikViewScaleUsed == 0.0f ? " (REJECTED -> fallback)" : "") << "\n";
            out << "pairLocal[128..131]=(" << g_pSharedHands[128] << ", " << g_pSharedHands[129]
                << ", " << g_pSharedHands[130] << ", seq=" << g_pSharedHands[131]
                << ") entSeq[99]=" << g_pSharedHands[99]
                << " pairValid=" << g_VRCamPairValid << "\n";
        }
        // CAMERA-KICK TRACE dump: min/max/range per channel over the buffered pushes,
        // then a sparse tail. Identifies WHICH channel carries the sprint/shot kick:
        // raw local (position kick), quat i/j (rotational kick), eyeBake (view feedback
        // wag), and whether the filtered pair passes it.
        {
            const int n = (g_camTraceN < VR_CAMTRACE_CAP) ? g_camTraceN : VR_CAMTRACE_CAP;
            out << "camTrace freeze=" << g_camTraceFreeze
                << (g_camTraceFreeze == 0 ? " (FROZEN on sprint stop -- window preserved)" : " (live)")
                << "\n";
            if (n > 8) {
                static const char* kCol[22] = {
                    "rawX", "rawY", "rawZ", "quatI", "quatJ",
                    "fltX", "fltY", "fltZ", "eyeBX", "eyeBY", "eyeBZ", "camBY",
                    "locX", "locY", "locZ", "hipsYawDeg",
                    "tgtX", "tgtY", "handX", "handY", "shX", "shY" };
                out << "camTrace n=" << n << " (per-channel min..max [range])\n";
                for (int c = 0; c < 22; ++c) {
                    float mn = g_camTrace[0][c], mx = g_camTrace[0][c];
                    for (int i = 1; i < n; ++i) {
                        const float v = g_camTrace[i][c];
                        if (v < mn) mn = v;
                        if (v > mx) mx = v;
                    }
                    out << "  " << kCol[c] << " " << mn << " .. " << mx
                        << "  [" << (mx - mn) << "]\n";
                }
                // Sparse tail: last ~3s (180 pushes, every 4th -> 45 rows, oldest->newest).
                // Columns picked for the SPRINT-DIVE hunt: raw pair (engine cam local),
                // then the MODEL-SPACE actors -- right IK target (tgt), solved right hand
                // FK (hand), right shoulder joint (sh). A stationary IRL controller =
                // constant model position by design, so whichever column ramps during
                // the sprint engage/stop transient IS the diver (body lean -> sh; anchor
                // math -> tgt+hand together; solve-side -> hand alone).
                const int newest = (g_camTraceN - 1) % VR_CAMTRACE_CAP;
                out << "camTrace tail (oldest->newest):\n";
                for (int k = 176; k >= 0; k -= 4) {
                    if (k >= n) continue;
                    const int i = ((newest - k) % VR_CAMTRACE_CAP + VR_CAMTRACE_CAP) % VR_CAMTRACE_CAP;
                    out << "  raw=(" << g_camTrace[i][0] << ", " << g_camTrace[i][1] << ", " << g_camTrace[i][2]
                        << ") hipsYaw=" << g_camTrace[i][15]
                        << " tgt=(" << g_camTrace[i][16] << ", " << g_camTrace[i][17]
                        << ") hand=(" << g_camTrace[i][18] << ", " << g_camTrace[i][19]
                        << ") sh=(" << g_camTrace[i][20] << ", " << g_camTrace[i][21]
                        << ")\n";
                }
            }
            g_camTraceFreeze = -1;   // un-freeze: next sprint records a fresh window
        }
        for (int cs = 0; cs < 2; ++cs) {
            out << (cs == 0 ? "clavR" : "clavL")
                << " desired=(" << g_VRIKDbgClav[cs][0] << ", " << g_VRIKDbgClav[cs][1] << ", " << g_VRIKDbgClav[cs][2] << ")"
                << " joint=(" << g_VRIKDbgClav[cs][3] << ", " << g_VRIKDbgClav[cs][4] << ", " << g_VRIKDbgClav[cs][5] << ")"
                << " need=" << g_VRIKDbgClav[cs][6] << "deg applied=" << g_VRIKDbgClav[cs][7] << "deg\n";
        }
        if (g_pSharedHands) {
            out << "viewOffs eyeBake[116..119]=(" << g_pSharedHands[116] << ", " << g_pSharedHands[117]
                << ", " << g_pSharedHands[118] << ", valid=" << g_pSharedHands[119] << ")\n";
            out << "viewOffs dxgiTotal[120..123]=(" << g_pSharedHands[120] << ", " << g_pSharedHands[121]
                << ", " << g_pSharedHands[122] << ", valid=" << g_pSharedHands[123] << ")\n";
            out << "viewOffs camBake[91..93]=(" << g_pSharedHands[91] << ", " << g_pSharedHands[92]
                << ", " << g_pSharedHands[93] << ")\n";
            // Barrel laser-dot chain (reported: red dot gone, hand rays alive).
            // Gate: overlay draws only if [144] (weapon flag, dxgi) >= 0.9 AND the
            // muzzle forward [24..26] is published with [27]=1 (Weapon Lua ->
            // SetVRMuzzleQuat). One dump pinpoints which link is dead.
            out << "laserDot gate[144]=" << g_pSharedHands[144]
                << " muzzleFwd[24..26]=(" << g_pSharedHands[24] << ", " << g_pSharedHands[25]
                << ", " << g_pSharedHands[26] << ") valid[27]=" << g_pSharedHands[27]
                << " zoom[28]=" << g_pSharedHands[28] << "\n";
        }
        out << "solvesPerTick last=" << g_VRIKSolvesLastTick
            << " max=" << g_VRIKSolvesMaxTick
            << " bufA=0x" << std::hex << g_VRIKLastBufA
            << " bufB=0x" << g_VRIKLastBufB << std::dec
            << " totalCalls=" << g_AnimPoseTotalCalls
            << " matchCalls=" << g_AnimPoseMatchCalls
            << " replays=" << g_VRIKReplayTotal << "\n";
        out << "bodyUnderHMD=" << g_VRBodyUnderHMD << " chestDrop=" << g_VRChestDrop
            << " chestFwd=" << g_VRChestFwd << "\n";
        out << "chest target(model) = (" << g_VRIKDbgChestTgt[0] << ", " << g_VRIKDbgChestTgt[1] << ", " << g_VRIKDbgChestTgt[2] << ")\n";
        out << "chest actual(model) = (" << g_VRIKDbgChest[0] << ", " << g_VRIKDbgChest[1] << ", " << g_VRIKDbgChest[2] << ")\n";

        // Lower-body resolve + FK positions (model space, post-solve). If any reads UNRESOLVED the
        // bone name didn't match -> leg IK never runs -> squat can't bend the knees.
        out << "lowerbody idx: hips=" << g_VRHipsIdx
            << " rUpLeg=" << g_VRRightUpLegIdx << " rLeg=" << g_VRRightLegIdx << " rFoot=" << g_VRRightFootIdx
            << " lUpLeg=" << g_VRLeftUpLegIdx << " lLeg=" << g_VRLeftLegIdx << " lFoot=" << g_VRLeftFootIdx
            << " neck=" << g_VRNeckIdx << "\n";
        auto pfk = [&](int idx, const char* nm) {
            if (idx >= 0 && idx < 256)
                out << "  " << nm << "[" << idx << "] fk=(" << g_fkPos[idx][0] << ", " << g_fkPos[idx][1] << ", " << g_fkPos[idx][2] << ")\n";
            else
                out << "  " << nm << " = UNRESOLVED\n";
        };
        pfk(g_VRHipsIdx, "Hips"); pfk(g_VRRightUpLegIdx, "RUpLeg"); pfk(g_VRRightLegIdx, "RLeg"); pfk(g_VRRightFootIdx, "RFoot");
        pfk(g_VRLeftUpLegIdx, "LUpLeg"); pfk(g_VRLeftLegIdx, "LLeg"); pfk(g_VRLeftFootIdx, "LFoot");

        // Current avatar arm length in FK (post-scale) vs the target userArmLen -- if these don't
        // match, the bicep/forearm scaling isn't reaching the player's real arm length.
        auto fkArm = [&](int up, int fore, int hand) -> float {
            if (up<0||fore<0||hand<0||up>=256||fore>=256||hand>=256) return 0.0f;
            auto d=[&](int a,int b){ float dx=g_fkPos[a][0]-g_fkPos[b][0],dy=g_fkPos[a][1]-g_fkPos[b][1],dz=g_fkPos[a][2]-g_fkPos[b][2]; return std::sqrt(dx*dx+dy*dy+dz*dz); };
            return d(fore,up)+d(hand,fore);
        };
        auto fkLeg = [&](int up, int knee, int foot) -> float {
            if (up<0||knee<0||foot<0||up>=256||knee>=256||foot>=256) return 0.0f;
            auto d=[&](int a,int b){ float dx=g_fkPos[a][0]-g_fkPos[b][0],dy=g_fkPos[a][1]-g_fkPos[b][1],dz=g_fkPos[a][2]-g_fkPos[b][2]; return std::sqrt(dx*dx+dy*dy+dz*dz); };
            return d(knee,up)+d(foot,knee);
        };
        out << "avatar arm FK R/L = " << fkArm(g_VRRightUpperArmIdx,g_VRRightForeArmIdx,g_VRRightBoneIdx)
            << "/" << fkArm(g_VRLeftUpperArmIdx,g_VRLeftForeArmIdx,g_VRLeftBoneIdx)
            << "  target userArmLen=" << g_VRUserArmLenR << "/" << g_VRUserArmLenL << "\n";
        out << "avatar leg FK R/L = " << fkLeg(g_VRRightUpLegIdx,g_VRRightLegIdx,g_VRRightFootIdx)
            << "/" << fkLeg(g_VRLeftUpLegIdx,g_VRLeftLegIdx,g_VRLeftFootIdx) << "\n";
    }
    out << "LK target(model)   = (" << g_VRIKDbgTargetL[0] << ", " << g_VRIKDbgTargetL[1] << ", " << g_VRIKDbgTargetL[2] << ")\n";
    out << "LK shoulder(model) = (" << g_VRIKDbgShoulderL[0] << ", " << g_VRIKDbgShoulderL[1] << ", " << g_VRIKDbgShoulderL[2] << ")\n";
    out << "LK elbow(model)    = (" << g_VRIKDbgElbowL[0] << ", " << g_VRIKDbgElbowL[1] << ", " << g_VRIKDbgElbowL[2] << ")\n";
    out << "LK hand body(lx,ly,lz,cross) = (" << g_VRIKDbgLocalL[0] << ", " << g_VRIKDbgLocalL[1] << ", " << g_VRIKDbgLocalL[2] << ", " << g_VRIKDbgLocalL[3] << ")\n";
    out << "LK lens upper=" << g_VRIKDbgLensL[0] << " fore=" << g_VRIKDbgLensL[1] << "\n\n";

    if (aOut) *aOut = 1;
}

// Script-callable thin wrapper: reads the FPP camera world pose from the frame
// and forwards to WriteVRDiagCore. Same entry as the CET "Log VR Diag" button.
void LogVRDiag(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    float camX = 0, camY = 0, camZ = 0, qi = 0, qj = 0, qk = 0, qr = 1;
    RED4ext::GetParameter(aFrame, &camX);
    RED4ext::GetParameter(aFrame, &camY);
    RED4ext::GetParameter(aFrame, &camZ);
    RED4ext::GetParameter(aFrame, &qi);
    RED4ext::GetParameter(aFrame, &qj);
    RED4ext::GetParameter(aFrame, &qk);
    RED4ext::GetParameter(aFrame, &qr);
    aFrame->code++;
    WriteVRDiagCore(camX, camY, camZ, qi, qj, qk, qr);
    if (aOut) *aOut = 1;
}

// Dump VTable specifically for entAnimatedComponent
void DumpAnimVTable(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, void* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(aOut); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    
    std::ofstream out(VRDiagPath("anim_vtable_dump.txt"), std::ios::trunc);
    
    RED4ext::ScriptGameInstance gameInstance;
    RED4ext::Handle<RED4ext::IScriptable> playerHandle;
    RED4ext::ExecuteGlobalFunction("GetPlayer;GameInstance", &playerHandle, gameInstance);
    if (!playerHandle) { out << "No player\n"; return; }

    auto* playerEntity = reinterpret_cast<RED4ext::ent::Entity*>(playerHandle.instance);
    if (!playerEntity) { out << "No player entity\n"; return; }

    for (auto& componentHandle : playerEntity->components) {
        auto* component = componentHandle.instance;
        if (!component) continue;
        
        RED4ext::CClass* type = component->GetType();
        if (type && type->name == "entAnimatedComponent") {
            out << "Found entAnimatedComponent at: " << std::hex << (uintptr_t)component << "\n";
            
            uintptr_t* vtable = *reinterpret_cast<uintptr_t**>(component);
            out << "VTable Address: " << std::hex << (uintptr_t)vtable << "\n";
            
            HMODULE hMod = GetModuleHandleA("Cyberpunk2077.exe");
            uintptr_t base = (uintptr_t)hMod;
            
            for(int i = 0; i < 60; ++i) { // dump 60 just in case
                uintptr_t func = vtable[i];
                out << "  [" << std::dec << i << std::hex << "] Func: " << func << " (Cyberpunk2077.exe+" << (func - base) << ")\n";
            }
            break; 
        }
    }
    out.close();
}

void DumpAnimControllerComponents(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;

    RED4ext::ScriptGameInstance gameInstance;
    RED4ext::Handle<RED4ext::IScriptable> playerHandle;
    RED4ext::ExecuteGlobalFunction("GetPlayer;GameInstance", &playerHandle, gameInstance);
    if (!playerHandle)
    {
        if (aOut) *aOut = -1;
        return;
    }

    auto* playerEntity = reinterpret_cast<RED4ext::ent::Entity*>(playerHandle.instance);
    if (!playerEntity)
    {
        if (aOut) *aOut = -2;
        return;
    }

    std::ofstream out(VRDiagPath("anim_controller_dump.txt"), std::ios::trunc);
    int dumped = 0;

    for (auto& componentHandle : playerEntity->components)
    {
        auto* component = componentHandle.instance;
        if (!component)
            continue;

        RED4ext::CClass* type = component->GetType();
        if (!type)
            continue;

        if (type->name == "entAnimationControllerComponent")
        {
            auto* controller = reinterpret_cast<RED4ext::ent::AnimationControllerComponent*>(component);
            out << "==================================================\n";
            out << "AnimationControllerComponent ptr=0x" << std::hex << reinterpret_cast<uintptr_t>(controller) << std::dec << "\n";
            out << "name=" << component->name.ToString() << " enabled=" << (component->isEnabled ? 1 : 0) << "\n";
            out << "lookAtController ptr(backref)=0x" << std::hex << reinterpret_cast<uintptr_t>(controller->lookAtController.animationControllerComponent) << std::dec << "\n";
            out << "ikTargetController ptr(backref)=0x" << std::hex << reinterpret_cast<uintptr_t>(controller->ikTargetController.animationControllerComponent) << std::dec << "\n";
            out << "ikTargetController.targetData.size=" << controller->ikTargetController.targetData.Size() << "\n";

            for (uint32_t i = 0; i < controller->ikTargetController.targetData.Size(); ++i)
            {
                const auto& target = controller->ikTargetController.targetData[i];
                out << "  [" << i << "] id=" << target.targetReference.id
                    << " part=" << target.targetReference.part.ToString()
                    << " posProvider=0x" << std::hex << reinterpret_cast<uintptr_t>(target.positionProvider.instance)
                    << " orientProvider=0x" << reinterpret_cast<uintptr_t>(target.orientationProvider.instance)
                    << std::dec << "\n";
            }

            out << "ikParams=0x" << std::hex << reinterpret_cast<uintptr_t>(controller->ikTargetController.ikParams.instance) << std::dec << "\n";
            ++dumped;
        }
    }

    out.close();
    if (aOut) *aOut = dumped;
}

void DumpRuntimeClassFunctions(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;

    auto* rtti = RED4ext::CRTTISystem::Get();
    std::ofstream out(VRDiagPath("runtime_class_functions.txt"), std::ios::trunc);
    int dumped = 0;

    const char* classesToDump[] = {
        "entAnimationControllerComponent",
        "entEntity",
        "entAnimatedComponent"
    };

    for (const char* className : classesToDump)
    {
        RED4ext::CClass* cls = rtti->GetClass(className);
        if (!cls)
            continue;

        out << "==================================================\n";
        out << "CLASS " << className << "\n";
        out << "==================================================\n";

        out << "Member functions:\n";
        for (auto* func : cls->funcs)
        {
            if (!func)
                continue;
            out << "  - " << func->fullName.ToString() << "\n";
        }

        out << "Static functions:\n";
        for (auto* func : cls->staticFuncs)
        {
            if (!func)
                continue;
            out << "  - " << func->fullName.ToString() << "\n";
        }

        out << "\n";
        ++dumped;
    }

    out.close();
    if (aOut) *aOut = dumped;
}
#endif
#endif
