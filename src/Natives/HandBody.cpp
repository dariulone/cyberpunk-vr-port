// HandBody -- natives lifted out of src/Natives/Natives.cpp, which held every family at once.
//
// Hands, palms and the physics body: where a hand is, what it may touch, and the
// velocity API that lets a held object be thrown rather than teleported.
//
// The cut was placed by the seam map and then SNAPPED to the nearest point at brace depth zero.
// Boundaries taken from line numbers alone are how a split lands in the middle of a function; the
// check is cheap and it is the same lesson as every other generator in this restructure.
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/GameEngine.hpp>
#include <sstream>
#include <locale>
#include <clocale>
#include "Natives/DebugView.hpp"
#include "Natives/HeldBody.hpp"
#include "Anim/GraspPose.hpp"
#include "Natives/PhysHands.hpp"
#include "Core/VrCoreShared.hpp"
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
#include <RED4ext/Scripting/Natives/Generated/physics/TraceResult.hpp>
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
#include "Runtimes/OpenXRManager.hpp"
#include "Natives/NativeHelpers.hpp"
#include "Natives/NativeState.hpp"




void ArmVRAnimPosePlayer(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    int r = VRIK_DoArmPlayer();
    if (aOut) *aOut = r;
}

// mode 0=total calls, 1=player-match calls, 2=last matched bone buffer low-32.
void GetVRAnimPoseStats(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t mode = 0;
    RED4ext::GetParameter(aFrame, &mode);
    aFrame->code++;
    uint64_t v = (mode == 1) ? g_AnimPoseMatchCalls
               : (mode == 2) ? g_AnimPoseLastBoneBuf
               : g_AnimPoseTotalCalls;
    if (aOut) *aOut = static_cast<int32_t>(v & 0xFFFFFFFF);
}

// Dumps every metaRig bone name with its index so we can locate the hand bones.
void DumpPlayerBoneNames(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    if (aOut) *aOut = 0;

    std::ofstream out(VRDiagPath("player_bone_names.txt"), std::ios::trunc);
    auto* animObj = FindPlayerAnimatedObjectByComponentName("root");
    auto* metaRig = animObj ? animObj->metaRig : nullptr;
    if (!metaRig) { if (aOut) *aOut = -1; return; }

    const uint32_t n = metaRig->boneNames.Size();
    if (out.is_open()) {
        out << "boneCount=" << n << "\n";
        for (uint32_t i = 0; i < n; ++i) {
            const char* name = metaRig->boneNames[i].ToString();
            out << i << "\t" << (name ? name : "<null>") << "\n";
        }
    }
    if (aOut) *aOut = static_cast<int32_t>(n);
}

// SMOKE FINGER-HOLD controls (see g_VRSmokeFinger* + the block in Hooked_AnimPoseApply).
// SetVRSmokeFingers(active): 1 = replay the captured finger curl every player pose pass
// (fingers close around the cigarette; VRIK still drives the wrist to the controller),
// 0 = release. Returns the active state.
void SetVRSmokeFingers(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t active = 0;
    RED4ext::GetParameter(aFrame, &active);
    aFrame->code++;
    g_VRSmokeFingerActive = active ? 1 : 0;
    if (aOut) *aOut = g_VRSmokeFingerActive;
}

// VRSmokeCaptureFingers(): latch the current right-hand finger locals on the next player
// pose pass (call while the vanilla hold-cigarette workspot plays via AMM, fingers curled).
// Returns the resolved finger-bone count (>0 confirms the capture is armed; 0 = bones not
// resolved yet, enter VR / spawn the player first).
void VRSmokeCaptureFingers(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    g_VRSmokeFingerCapture = 1;
    if (aOut) *aOut = g_VRSmokeFingerCount;
}

// VRSmokeDumpFingers(): write the captured grip pose to CyberpunkVR_SmokeGrip.ini (next to
// the game exe). Name-keyed lines, so it reloads on the player skeleton regardless of index.
// The live cig nudge (SetVRSmokeCigOffset) is BAKED into the saved cig transform, so reloading
// (with the nudge reset) reproduces exactly what you tuned. Returns the number of lines written.
void VRSmokeDumpFingers(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    // Force '.' decimals for the whole dump regardless of the process/thread CRT locale, so the
    // file always round-trips (the loader parses '.' via classic() locale). Per-thread scope.
    _configthreadlocale(_ENABLE_PER_THREAD_LOCALE);
    const char* prevNum = std::setlocale(LC_NUMERIC, nullptr);
    std::string savedNum = prevNum ? prevNum : "C";
    std::setlocale(LC_NUMERIC, "C");
    int written = 0;
    char buf[256];
    // RIGHT hand (cigarette) -> CyberpunkVR_SmokeGrip_right.ini
    if (g_VRSmokeFingerHave || g_VRSmokeCigHave) {
        std::ofstream f(VRGripPath("CyberpunkVR_SmokeGrip_right.ini"), std::ios::trunc);
        if (f.is_open()) {
            f << "# CyberpunkVR RIGHT-hand cigarette grip v1 (auto-generated by VRSmokeDumpFingers)\n";
            f << "# F <bone> qx qy qz qw    |    C WeaponRight px py pz qx qy qz qw\n";
            if (g_VRSmokeFingerHave) for (int k = 0; k < g_VRSmokeFingerCount && k < 32; ++k) {
                if (g_VRSmokeFingerName[k][0] == '\0') continue;
                std::snprintf(buf, sizeof(buf), "F %s %.9g %.9g %.9g %.9g\n",
                    g_VRSmokeFingerName[k],
                    g_VRSmokeFingerRot[k][0], g_VRSmokeFingerRot[k][1],
                    g_VRSmokeFingerRot[k][2], g_VRSmokeFingerRot[k][3]);
                f << buf; ++written;
            }
            if (g_VRSmokeCigHave && g_VRSmokeCigIdx >= 0) {
                const float pos[3] = { g_VRSmokeCigPos[0]+g_VRSmokeCigOffP[0],
                                       g_VRSmokeCigPos[1]+g_VRSmokeCigOffP[1],
                                       g_VRSmokeCigPos[2]+g_VRSmokeCigOffP[2] };
                const float base[4] = { g_VRSmokeCigRot[0], g_VRSmokeCigRot[1], g_VRSmokeCigRot[2], g_VRSmokeCigRot[3] };
                const float off[4]  = { g_VRSmokeCigOffQ[0], g_VRSmokeCigOffQ[1], g_VRSmokeCigOffQ[2], g_VRSmokeCigOffQ[3] };
                float rot[4]; VRIK_QuatMul(base, off, rot); VRIK_QuatNorm(rot);
                std::snprintf(buf, sizeof(buf), "C WeaponRight %.9g %.9g %.9g %.9g %.9g %.9g %.9g\n",
                    pos[0], pos[1], pos[2], rot[0], rot[1], rot[2], rot[3]);
                f << buf; ++written;
            }
        }
    }
    // LEFT hand (lighter) -> CyberpunkVR_LighterGrip_Left.ini
    if (g_VRSmokeFingerHaveL || g_VRSmokeLighterHave) {
        std::ofstream f(VRGripPath("CyberpunkVR_LighterGrip_Left.ini"), std::ios::trunc);
        if (f.is_open()) {
            f << "# CyberpunkVR LEFT-hand lighter grip v1 (auto-generated by VRSmokeDumpFingers)\n";
            f << "# F <bone> qx qy qz qw | C WeaponLeft px py pz qx qy qz qw | K LeftThumbFlick qx qy qz qw\n";
            if (g_VRSmokeFingerHaveL) for (int k = 0; k < g_VRSmokeFingerCountL && k < 32; ++k) {
                if (g_VRSmokeFingerNameL[k][0] == '\0') continue;
                std::snprintf(buf, sizeof(buf), "F %s %.9g %.9g %.9g %.9g\n",
                    g_VRSmokeFingerNameL[k],
                    g_VRSmokeFingerRotL[k][0], g_VRSmokeFingerRotL[k][1],
                    g_VRSmokeFingerRotL[k][2], g_VRSmokeFingerRotL[k][3]);
                f << buf; ++written;
            }
            if (g_VRSmokeLighterHave && g_VRSmokeLighterIdx >= 0) {
                const float pos[3] = { g_VRSmokeLighterPos[0]+g_VRSmokeLighterOffP[0],
                                       g_VRSmokeLighterPos[1]+g_VRSmokeLighterOffP[1],
                                       g_VRSmokeLighterPos[2]+g_VRSmokeLighterOffP[2] };
                const float base[4] = { g_VRSmokeLighterRot[0], g_VRSmokeLighterRot[1], g_VRSmokeLighterRot[2], g_VRSmokeLighterRot[3] };
                const float off[4]  = { g_VRSmokeLighterOffQ[0], g_VRSmokeLighterOffQ[1], g_VRSmokeLighterOffQ[2], g_VRSmokeLighterOffQ[3] };
                float rot[4]; VRIK_QuatMul(base, off, rot); VRIK_QuatNorm(rot);
                std::snprintf(buf, sizeof(buf), "C WeaponLeft %.9g %.9g %.9g %.9g %.9g %.9g %.9g\n",
                    pos[0], pos[1], pos[2], rot[0], rot[1], rot[2], rot[3]);
                f << buf; ++written;
            }
            if (g_VRSmokeFingerHaveL) {
                std::snprintf(buf, sizeof(buf), "K LeftThumbFlick %.9g %.9g %.9g %.9g\n",
                    g_VRSmokeThumbFlickL[0], g_VRSmokeThumbFlickL[1], g_VRSmokeThumbFlickL[2], g_VRSmokeThumbFlickL[3]);
                f << buf; ++written;
            }
        }
    }
    // LEFT hand (CIGARETTE) -> CyberpunkVR_SmokeGrip_Left.ini (separate hold for the cig in the left hand)
    if (g_VRSmokeCigLHave) {
        std::ofstream f(VRGripPath("CyberpunkVR_SmokeGrip_Left.ini"), std::ios::trunc);
        if (f.is_open()) {
            f << "# CyberpunkVR LEFT-hand cigarette grip v1 (auto-generated by VRSmokeDumpFingers)\n";
            f << "# F <bone> qx qy qz qw | C WeaponLeft px py pz qx qy qz qw\n";
            for (int k = 0; k < g_VRSmokeFingerCountL && k < 32; ++k) {
                if (g_VRSmokeFingerNameL[k][0] == '\0') continue;
                std::snprintf(buf, sizeof(buf), "F %s %.9g %.9g %.9g %.9g\n",
                    g_VRSmokeFingerNameL[k],
                    g_VRSmokeFingerRotLC[k][0], g_VRSmokeFingerRotLC[k][1],
                    g_VRSmokeFingerRotLC[k][2], g_VRSmokeFingerRotLC[k][3]);
                f << buf; ++written;
            }
            if (g_VRSmokeLighterIdx >= 0) {
                std::snprintf(buf, sizeof(buf), "C WeaponLeft %.9g %.9g %.9g %.9g %.9g %.9g %.9g\n",
                    g_VRSmokeCigLPos[0], g_VRSmokeCigLPos[1], g_VRSmokeCigLPos[2],
                    g_VRSmokeCigLRot[0], g_VRSmokeCigLRot[1], g_VRSmokeCigLRot[2], g_VRSmokeCigLRot[3]);
                f << buf; ++written;
            }
        }
    }
    std::setlocale(LC_NUMERIC, savedNum.c_str());
    if (aOut) *aOut = written;
}

// SetVRSmokeCig(enable): 1 = also place the WeaponRight (cig) slot when the grip is applied,
// 0 = leave it alone (fingers-only, in case the captured slot transform looks wrong).
void SetVRSmokeCig(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t en = 1;
    RED4ext::GetParameter(aFrame, &en);
    aFrame->code++;
    g_VRSmokeCigEnable = en ? 1 : 0;
    if (aOut) *aOut = g_VRSmokeCigEnable;
}

// VRSmokeMouthDist(): model-space distance from the cig slot to the mouth, computed in the VRIK
// body solve (tracks HMD + controller). redscript uses this for the inhale/put-in-mouth gesture
// instead of the FPP camera, whose world position ignores HMD positional/lean tracking in VR.
void VRSmokeMouthDist(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, float* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    if (aOut) *aOut = g_VRSmokeMouthDist;
}

// VRSmokeMouthDistL(): same as VRSmokeMouthDist but for the LEFT hand (left controller <-> mouth).
// Lets redscript gate the LEFT grip on the LEFT hand being at the lips, so raising the LEFT hand to
// the mouth (with the right hand down) still toggles the cig.
void VRSmokeMouthDistL(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, float* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    if (aOut) *aOut = g_VRSmokeMouthDistL;
}

// SetVRSmokeCigOffset(x,y,z,pitch,yaw,roll): live nudge of the cig in the hand (bone-local
// metres + degrees) for tuning in VR. Bake it with VRSmokeDumpFingers when it looks right.
void SetVRSmokeCigOffset(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    float x=0.0f, y=0.0f, z=0.0f, pitch=0.0f, yaw=0.0f, roll=0.0f;
    RED4ext::GetParameter(aFrame, &x);
    RED4ext::GetParameter(aFrame, &y);
    RED4ext::GetParameter(aFrame, &z);
    RED4ext::GetParameter(aFrame, &pitch);
    RED4ext::GetParameter(aFrame, &yaw);
    RED4ext::GetParameter(aFrame, &roll);
    aFrame->code++;
    g_VRSmokeCigOffP[0]=x; g_VRSmokeCigOffP[1]=y; g_VRSmokeCigOffP[2]=z;
    const float d2r = 0.01745329252f * 0.5f;
    float cp = std::cos(pitch*d2r), sp = std::sin(pitch*d2r);
    float cy = std::cos(yaw*d2r),   sy = std::sin(yaw*d2r);
    float cr = std::cos(roll*d2r),  sr = std::sin(roll*d2r);
    g_VRSmokeCigOffQ[0] = sp*cy*cr + cp*sy*sr;
    g_VRSmokeCigOffQ[1] = cp*sy*cr - sp*cy*sr;
    g_VRSmokeCigOffQ[2] = cp*cy*sr + sp*sy*cr;
    g_VRSmokeCigOffQ[3] = cp*cy*cr - sp*sy*sr;
    if (aOut) *aOut = 1;
}

// VRViewWorldPos()/VRViewWorldRot(): the rendered view pose in world space, unmodified.
// A script composes a tracked point as  world = VRViewWorldPos + rotate(VRViewWorldRot, local),
// where `local` is the controller offset from GetRightVRHandPos with OpenXR axes mapped to the
// game's (x, -z, y) and multiplied by world scale. Used by the basketball grip: the ball has to
// follow the real hand, and the FPP camera position ignores HMD positional tracking and lean.
void VRViewWorldPos(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    if (aOut) {
        aOut->X = g_VRViewWorldPos[0];
        aOut->Y = g_VRViewWorldPos[1];
        aOut->Z = g_VRViewWorldPos[2];
        aOut->W = static_cast<float>(g_VRViewWorldValid);
    }
}

// VRPalmWorldPos(right): world position of the palm centre, taken from the SOLVED avatar bone
// (RightHandMiddle1 / LeftHandMiddle1) rather than the controller. What the player sees holding
// the ball is the avatar hand, and arm IK puts that hand somewhere slightly different from the
// raw controller pose -- a ball placed at the controller visibly floats beside the palm.
//
// The bone is in model space and the view pose is in world space, so the two are bridged through
// the camera, which is known in both:
//     world = viewWorld + (viewRot * conj(camModelRot)) * (palmModel - camModelPos)
// W = 1 when the skeleton has been solved at least once, 0 otherwise.
void VRPalmWorldPos(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t right = 1;
    RED4ext::GetParameter(aFrame, &right);
    aFrame->code++;
    if (!aOut) return;
    aOut->X = 0.0f; aOut->Y = 0.0f; aOut->Z = 0.0f; aOut->W = 0.0f;
    if (!g_VRPalmModelValid || !g_VRViewWorldValid) return;
    const int idx = right ? g_VRPalmRIdx : g_VRPalmLIdx;
    if (idx < 0) return;

    const float camRot[4] = { g_VRCamModelRot[0], g_VRCamModelRot[1], g_VRCamModelRot[2], g_VRCamModelRot[3] };
    const float viewRot[4] = { g_VRViewWorldRot[0], g_VRViewWorldRot[1], g_VRViewWorldRot[2], g_VRViewWorldRot[3] };
    float camInv[4]; VRIK_QuatConj(camRot, camInv);
    float m2w[4]; VRIK_QuatMul(viewRot, camInv, m2w); VRIK_QuatNorm(m2w);

    const volatile float* pm = right ? g_VRPalmModelR : g_VRPalmModelL;
    const float rel[3] = { pm[0] - g_VRCamModelPos[0],
                           pm[1] - g_VRCamModelPos[1],
                           pm[2] - g_VRCamModelPos[2] };
    float w[3]; VRIK_QuatRotateVec(m2w, rel, w);
    aOut->X = g_VRViewWorldPos[0] + w[0];
    aOut->Y = g_VRViewWorldPos[1] + w[1];
    aOut->Z = g_VRViewWorldPos[2] + w[2];
    aOut->W = 1.0f;
}

// Publishes a physics-resolved hand position for the arm solver to stop at. Called from the driving loop
// once per hand per frame with the body's WORLD position; the conversion to model space happens here so it
// uses the very same frame pair as VRPalmWorldPos above and cannot drift from it.
//
// active = false clears the hold, so the arm goes straight back to following the controller.
void VRHandStop(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t right = 1;
    bool active = false;
    RED4ext::Vector4 world{};
    RED4ext::GetParameter(aFrame, &right);
    RED4ext::GetParameter(aFrame, &active);
    RED4ext::GetParameter(aFrame, &world);
    aFrame->code++;
    const int side = right ? 1 : 0;
    if (aOut) *aOut = false;
    if (!active) { g_VRHandStopValid[side] = 0; return; }
    if (!g_VRPalmModelValid || !g_VRViewWorldValid) { g_VRHandStopValid[side] = 0; return; }

    // model -> world is m2w = viewWorldRot * conj(camModelRot), anchored at viewWorldPos / camModelPos,
    // so world -> model is its exact inverse.
    const float camRot[4]  = { g_VRCamModelRot[0], g_VRCamModelRot[1], g_VRCamModelRot[2], g_VRCamModelRot[3] };
    const float viewRot[4] = { g_VRViewWorldRot[0], g_VRViewWorldRot[1], g_VRViewWorldRot[2], g_VRViewWorldRot[3] };
    float camInv[4]; VRIK_QuatConj(camRot, camInv);
    float m2w[4]; VRIK_QuatMul(viewRot, camInv, m2w); VRIK_QuatNorm(m2w);
    float w2m[4]; VRIK_QuatConj(m2w, w2m);

    const float rel[3] = { world.X - g_VRViewWorldPos[0],
                           world.Y - g_VRViewWorldPos[1],
                           world.Z - g_VRViewWorldPos[2] };
    float m[3]; VRIK_QuatRotateVec(w2m, rel, m);
    g_VRHandStopModel[side][0] = g_VRCamModelPos[0] + m[0];
    g_VRHandStopModel[side][1] = g_VRCamModelPos[1] + m[1];
    g_VRHandStopModel[side][2] = g_VRCamModelPos[2] + m[2];
    g_VRHandStopValid[side] = 1;
    if (aOut) *aOut = true;
}

// World -> model, for anything that is not a player bone: the weapon, a prop, a hit point from a query.
//
// The hand-collision work happens entirely in model space, where the bones are, and the weapon's transform only
// exists in world space. These two convert with the SAME frame pair as VRPalmWorldPos and VRHandStop, so a gun
// capsule and a body capsule end up in one consistent space rather than a few centimetres apart -- which is the
// size of the whole effect.
static bool VRWorldModelFrame(float* outW2M, float* outAnchorW, float* outAnchorM) {
    if (!g_VRPalmModelValid || !g_VRViewWorldValid) return false;
    const float camRot[4]  = { g_VRCamModelRot[0], g_VRCamModelRot[1], g_VRCamModelRot[2], g_VRCamModelRot[3] };
    const float viewRot[4] = { g_VRViewWorldRot[0], g_VRViewWorldRot[1], g_VRViewWorldRot[2], g_VRViewWorldRot[3] };
    float camInv[4]; VRIK_QuatConj(camRot, camInv);
    float m2w[4]; VRIK_QuatMul(viewRot, camInv, m2w); VRIK_QuatNorm(m2w);
    VRIK_QuatConj(m2w, outW2M);
    for (int i = 0; i < 3; ++i) {
        outAnchorW[i] = g_VRViewWorldPos[i];
        outAnchorM[i] = g_VRCamModelPos[i];
    }
    return true;
}

void VRWorldToModel(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    RED4ext::Vector4 world{};
    RED4ext::GetParameter(aFrame, &world);
    aFrame->code++;
    if (!aOut) return;
    aOut->X = 0.0f; aOut->Y = 0.0f; aOut->Z = 0.0f; aOut->W = 0.0f;
    float w2m[4], aw[3], am[3];
    if (!VRWorldModelFrame(w2m, aw, am)) return;
    const float rel[3] = { world.X - aw[0], world.Y - aw[1], world.Z - aw[2] };
    float m[3]; VRIK_QuatRotateVec(w2m, rel, m);
    aOut->X = am[0] + m[0]; aOut->Y = am[1] + m[1]; aOut->Z = am[2] + m[2]; aOut->W = 1.0f;
}

// A DIRECTION, so no anchor is applied -- rotation only.
void VRWorldDirToModel(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    RED4ext::Vector4 dir{};
    RED4ext::GetParameter(aFrame, &dir);
    aFrame->code++;
    if (!aOut) return;
    aOut->X = 0.0f; aOut->Y = 0.0f; aOut->Z = 0.0f; aOut->W = 0.0f;
    float w2m[4], aw[3], am[3];
    if (!VRWorldModelFrame(w2m, aw, am)) return;
    const float v[3] = { dir.X, dir.Y, dir.Z };
    float m[3]; VRIK_QuatRotateVec(w2m, v, m);
    aOut->X = m[0]; aOut->Y = m[1]; aOut->Z = m[2]; aOut->W = 1.0f;
}

// The same pair as below, but in MODEL space -- the frame the bones are already in.
//
// Everything that decides where a hand may go compares it against the player's own geometry, and every bone
// readout (VRBoneModelPos) is model space. Converting each capsule into world, or the target out of it, would
// mix two frames that are anchored differently -- the model/world pair here rides the render view, while a
// bone-to-world conversion rides the entity transform -- and a few centimetres of mismatch is exactly the size
// of the effect being built. So the collision work stays in model space and never converts at all.
void VRHandRawModel(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t right = 1;
    RED4ext::GetParameter(aFrame, &right);
    aFrame->code++;
    if (!aOut) return;
    aOut->X = 0.0f; aOut->Y = 0.0f; aOut->Z = 0.0f; aOut->W = 0.0f;
    const int side = right ? 1 : 0;
    if (!g_VRHandRawValid[side]) return;
    aOut->X = g_VRHandRawModel[side][0];
    aOut->Y = g_VRHandRawModel[side][1];
    aOut->Z = g_VRHandRawModel[side][2];
    aOut->W = 1.0f;
}

// The TRACKED hand rotation in model space, i.e. the orientation before any hold. The collision work orients
// the hand's own capsule with this rather than with the solved bone, which would depend on the previous frame's
// clamp and shake.
void VRHandRawRot(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Quaternion* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t right = 1;
    RED4ext::GetParameter(aFrame, &right);
    aFrame->code++;
    if (!aOut) return;
    const int side = right ? 1 : 0;
    aOut->i = g_VRHandRawRot[side][0];
    aOut->j = g_VRHandRawRot[side][1];
    aOut->k = g_VRHandRawRot[side][2];
    aOut->r = g_VRHandRawRot[side][3];
}

// THE HAND'S OWN MOTION in MODEL space: the controller's velocity (spin false, m/s, at the hand's scale) or its
// angular velocity (spin true, rad/s) as the runtime measures it in the room, turned into the model's axes the way
// the hands' positions are (XR (x, y, z) -> game (x, -z, y), then the view's model rotation). W = 1 when valid.
// A difference of model positions is no measure of a flick: every step of the body's yaw (10-13 deg at a time)
// moves a hand half a metre out by centimetres in one frame -- 2.5 m/s was read off a 0.1 m/s hand on a slow head
// turn, and the revolver's crane, shut by a 3 m/s flick, shut by itself.
void VRHandMotion(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t right = 1;
    bool spin = false;
    RED4ext::GetParameter(aFrame, &right);
    RED4ext::GetParameter(aFrame, &spin);
    aFrame->code++;
    if (!aOut) return;
    aOut->X = 0.0f; aOut->Y = 0.0f; aOut->Z = 0.0f; aOut->W = 0.0f;
    float linear[3], angular[3];
    if (!g_VRPalmModelValid || !OpenXRManager::Get().GetHandMotion(right ? 1 : 0, linear, angular)) return;
    const float* v = spin ? angular : linear;
    const float scale = spin ? 1.0f : (right ? g_VRScaleR : g_VRScaleL);
    const float game[3] = { v[0] * scale, -v[2] * scale, v[1] * scale };
    const float view[4] = { g_VRCamModelRot[0], g_VRCamModelRot[1], g_VRCamModelRot[2], g_VRCamModelRot[3] };
    float model[3];
    VRIK_QuatRotateVec(view, game, model);
    aOut->X = model[0];
    aOut->Y = model[1];
    aOut->Z = model[2];
    aOut->W = 1.0f;
}

// Hold the hand at a MODEL-space position. No conversion, so nothing can drift.
void VRHandStopModel(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t right = 1;
    bool active = false;
    RED4ext::Vector4 model{};
    RED4ext::GetParameter(aFrame, &right);
    RED4ext::GetParameter(aFrame, &active);
    RED4ext::GetParameter(aFrame, &model);
    aFrame->code++;
    const int side = right ? 1 : 0;
    if (!active) { g_VRHandStopValid[side] = 0; if (aOut) *aOut = true; return; }
    g_VRHandStopModel[side][0] = model.X;
    g_VRHandStopModel[side][1] = model.Y;
    g_VRHandStopModel[side][2] = model.Z;
    g_VRHandStopValid[side] = 1;
    if (aOut) *aOut = true;
}

// WRIST ROTATION LOCK from script: while on, the arm solve gets this MODEL-space quat instead of the
// controller's twist (see g_VRHandRotLock in the hook). The reload grip sends the gun's orientation composed
// with the recorded grip rotation every grabbed frame, so the glued hand also POINTS like the game's animation.
void VRHandStopRot(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t side = 0, active = 0;
    float qi = 0.0f, qj = 0.0f, qk = 0.0f, qr = 1.0f;
    RED4ext::GetParameter(aFrame, &side);
    RED4ext::GetParameter(aFrame, &active);
    RED4ext::GetParameter(aFrame, &qi);
    RED4ext::GetParameter(aFrame, &qj);
    RED4ext::GetParameter(aFrame, &qk);
    RED4ext::GetParameter(aFrame, &qr);
    aFrame->code++;
    if (aOut) *aOut = false;
    if (side < 0 || side > 1) return;
    if (!active) { g_VRHandRotLockOn[side] = 0; if (aOut) *aOut = true; return; }
    const float len = std::sqrt(qi * qi + qj * qj + qk * qk + qr * qr);
    if (len < 1e-4f) return;
    g_VRHandRotLock[side][0] = qi / len;
    g_VRHandRotLock[side][1] = qj / len;
    g_VRHandRotLock[side][2] = qk / len;
    g_VRHandRotLock[side][3] = qr / len;
    g_VRHandRotLockOn[side] = 1;
    if (aOut) *aOut = true;
}

// Where tracking alone would put the hand, in WORLD space -- the target the driving loop must steer the
// body towards. W = 0 when the frame pair is not ready yet.
void VRHandRawWorld(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t right = 1;
    RED4ext::GetParameter(aFrame, &right);
    aFrame->code++;
    if (!aOut) return;
    aOut->X = 0.0f; aOut->Y = 0.0f; aOut->Z = 0.0f; aOut->W = 0.0f;
    const int side = right ? 1 : 0;
    if (!g_VRHandRawValid[side] || !g_VRPalmModelValid || !g_VRViewWorldValid) return;

    const float camRot[4]  = { g_VRCamModelRot[0], g_VRCamModelRot[1], g_VRCamModelRot[2], g_VRCamModelRot[3] };
    const float viewRot[4] = { g_VRViewWorldRot[0], g_VRViewWorldRot[1], g_VRViewWorldRot[2], g_VRViewWorldRot[3] };
    float camInv[4]; VRIK_QuatConj(camRot, camInv);
    float m2w[4]; VRIK_QuatMul(viewRot, camInv, m2w); VRIK_QuatNorm(m2w);

    const float rel[3] = { g_VRHandRawModel[side][0] - g_VRCamModelPos[0],
                           g_VRHandRawModel[side][1] - g_VRCamModelPos[1],
                           g_VRHandRawModel[side][2] - g_VRCamModelPos[2] };
    float w[3]; VRIK_QuatRotateVec(m2w, rel, w);
    aOut->X = g_VRViewWorldPos[0] + w[0];
    aOut->Y = g_VRViewWorldPos[1] + w[1];
    aOut->Z = g_VRViewWorldPos[2] + w[2];
    aOut->W = 1.0f;
}

// The deadband from script, so it can be tuned in the headset instead of by rebuilding.
void VRHandStopDeadband(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    float v = 0.02f;
    RED4ext::GetParameter(aFrame, &v);
    aFrame->code++;
    if (v < 0.0f) v = 0.0f;
    if (v > 0.30f) v = 0.30f;
    g_VRHandStopDeadband = v;
    if (aOut) *aOut = true;
}

// Palm offset from the camera, expressed in the CAMERA's own frame (model space).
// MEASURED 2026-08-02: VRViewWorldPos is not the eye point -- it sits ~1.03 m below and 0.49 m
// aside from the real FPP camera -- so composing world positions against it put the palm under
// the floor. This native avoids the question entirely: it returns a camera-relative offset, and
// the script composes it with the FPP camera transform it can read directly.
void VRPalmCamLocal(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t right = 1;
    RED4ext::GetParameter(aFrame, &right);
    aFrame->code++;
    if (!aOut) return;
    aOut->X = 0.0f; aOut->Y = 0.0f; aOut->Z = 0.0f; aOut->W = 0.0f;
    if (!g_VRPalmModelValid) return;
    const int idx = right ? g_VRPalmRIdx : g_VRPalmLIdx;
    if (idx < 0) return;

    const volatile float* pm = right ? g_VRPalmModelR : g_VRPalmModelL;
    const float rel[3] = { pm[0] - g_VRCamModelPos[0],
                           pm[1] - g_VRCamModelPos[1],
                           pm[2] - g_VRCamModelPos[2] };
    const float camRot[4] = { g_VRCamModelRot[0], g_VRCamModelRot[1], g_VRCamModelRot[2], g_VRCamModelRot[3] };
    float camInv[4]; VRIK_QuatConj(camRot, camInv);
    float local[3]; VRIK_QuatRotateVec(camInv, rel, local);
    aOut->X = local[0];
    aOut->Y = local[1];
    aOut->Z = local[2];
    aOut->W = 1.0f;
}

// ---- the rig measurement channel ---------------------------------------------------------------
//
// Any bone of the SOLVED rig, by index, in model space -- the same space as VRPalmModelPos, so world =
// playerPos + playerRot * this. Needed because the collision capsules are authored from guessed anatomy
// (a vanilla ragdoll's radii) while the thing they must match is the VR arm the player swings: the arms
// came out the wrong size, with the hand collider stopping at the base of the palm and no fingers.
// Guessing again is not a plan; these read the joints themselves, fingertips included.
//
// Only valid while SetVRDiagCapture(1) is on, because that is the only time the post-solve, whole-skeleton
// FK is computed -- without it the arms in the snapshot would be the engine's animated idle pose.
// Index table: DumpPlayerBoneNames() writes player_bone_names.txt.
void VRBoneSnapCount(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    if (aOut) *aOut = g_VRFKSnapCount;
}

void VRBoneModelPos(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t idx = -1;
    RED4ext::GetParameter(aFrame, &idx);
    aFrame->code++;
    if (!aOut) return;
    aOut->X = 0.0f; aOut->Y = 0.0f; aOut->Z = 0.0f; aOut->W = 0.0f;
    if (idx < 0 || idx >= g_VRFKSnapCount) return;
    aOut->X = g_VRFKSnapPos[idx][0];
    aOut->Y = g_VRFKSnapPos[idx][1];
    aOut->Z = g_VRFKSnapPos[idx][2];
    aOut->W = 1.0f;
}

void VRBoneModelRot(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Quaternion* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t idx = -1;
    RED4ext::GetParameter(aFrame, &idx);
    aFrame->code++;
    if (!aOut) return;
    aOut->i = 0.0f; aOut->j = 0.0f; aOut->k = 0.0f; aOut->r = 1.0f;
    if (idx < 0 || idx >= g_VRFKSnapCount) return;
    aOut->i = g_VRFKSnapRot[idx][0];
    aOut->j = g_VRFKSnapRot[idx][1];
    aOut->k = g_VRFKSnapRot[idx][2];
    aOut->r = g_VRFKSnapRot[idx][3];
}

// Raw model-space palm position, for diagnosing which composition is correct.
void VRPalmModelPos(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t right = 1;
    RED4ext::GetParameter(aFrame, &right);
    aFrame->code++;
    if (!aOut) return;
    const volatile float* pm = right ? g_VRPalmModelR : g_VRPalmModelL;
    aOut->X = pm[0]; aOut->Y = pm[1]; aOut->Z = pm[2];
    aOut->W = g_VRPalmModelValid ? 1.0f : 0.0f;
}

// ---- the physics body's velocity API ----------------------------------------------------------
//
// The RTTI class is `entPhysicalBodyInterface`; `PhysicalBodyInterface` is only the redscript
// alias, which is why the first lookup here came back empty. It really does carry
// SetLinearVelocity / GetLinearVelocity / SetAngularVelocity / SetMass -- none of which appear in
// engine_re/redscript_ref, the dump this project had been treating as the whole API.
//
// MEASURED via CET DumpType: those entries have EMPTY parameter and return descriptors, while
// their neighbours do not (`SetTransform(pos: Transform)`, `AddLinearImpulse(impulse: Vector4,
// originInCOM: Bool, [opt] offset: Vector4)`). So the binding was never fully described rather
// than being absent, and that is exactly what CET trips over: "requires 0 parameter(s)", result
// nil. The native implementations are still registered and can be invoked from here with a
// hand-built CStack, which does not consult the descriptor.
//
// GetMass is the control for that claim: the mesh authors 0.624 kg, so if the probe prints 0.624
// the invoke path is sound and the velocity calls on the same path can be trusted.
static RED4ext::CBaseFunction* VRBodyFunc(const char* aShortName) {
    auto* rtti = RED4ext::CRTTISystem::Get();
    auto* cls = rtti->GetClass("entPhysicalBodyInterface");
    if (!cls) cls = rtti->GetClass("PhysicalBodyInterface");
    if (!cls) return nullptr;
    const RED4ext::CName want(aShortName);
    for (uint32_t i = 0; i < cls->funcs.size(); ++i) {
        auto* f = cls->funcs[i];
        if (f && f->shortName == want) return f;
    }
    return nullptr;
}

// Calling a function whose descriptor says "no parameters" with a parameter is exactly the kind of
// thing that can fault inside the engine's marshaller. Isolated so the probe reports a failure
// instead of taking the game down. No C++ objects here -- SEH and unwinding do not mix.
//
// AND NEVER ON A BROKEN THING. Every call here is on an entPhysicalBodyInterface made by NewBody, whose
// +0x40 and +0x44 are the proxy and the body. A destructible's body whose actor the game has released
// still reaches that freed actor through this interface; a velocity written there put it back into the
// scene's queries, and the player's controller crashed on it (DebugView.cpp, BodyGone).
static bool VRBodyBroken(const RED4ext::IScriptable* aBody) {
    if (!aBody) return true;
    const auto* raw = reinterpret_cast<const uint8_t*>(aBody);
    return cvr::debugview::BodyGone(*reinterpret_cast<const uint32_t*>(raw + 0x40), *reinterpret_cast<const uint32_t*>(raw + 0x44));
}
static bool VRSafeExecute(RED4ext::CBaseFunction* aFunc, RED4ext::CStack* aStack) {
    if (VRBodyBroken(aStack->GetContext())) return false;
    __try {
        return aFunc->Execute(aStack);
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
}

// The buffer is oversized: if the native actually returns a Vector3 it writes 12 bytes, and if it
// returns something wider we still have room. Either way nothing lands past the end.
static bool VRBodyCallGet(RED4ext::IScriptable* aBody, const char* aName, const char* aRetType,
                          void* aOut, size_t aOutSize) {
    if (!aBody || !aOut) return false;
    auto* f = VRBodyFunc(aName);
    if (!f) return false;
    auto* rtti = RED4ext::CRTTISystem::Get();
    alignas(16) uint8_t buf[64] = {};
    RED4ext::CStackType result(rtti->GetType(aRetType), buf);
    RED4ext::CStack stack(aBody, nullptr, 0, &result);
    if (!VRSafeExecute(f, &stack)) return false;
    std::memcpy(aOut, buf, aOutSize > sizeof(buf) ? sizeof(buf) : aOutSize);
    return true;
}

static bool VRBodyCallSetVec(RED4ext::IScriptable* aBody, const char* aName,
                             const RED4ext::Vector4& aVal) {
    if (!aBody) return false;
    auto* f = VRBodyFunc(aName);
    if (!f) return false;
    auto* rtti = RED4ext::CRTTISystem::Get();
    RED4ext::Vector4 v = aVal;
    RED4ext::CStackType args[1];
    args[0].type = rtti->GetType("Vector4");
    args[0].value = &v;
    RED4ext::CStack stack(aBody, args, 1, nullptr);
    return VRSafeExecute(f, &stack);
}

// Reads the body's own linear velocity. W = 1 when the call went through, 0 when it did not, so a
// caller can never mistake "the API is unavailable" for "the ball is stationary".
void VRBodyGetVel(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    RED4ext::Handle<RED4ext::IScriptable> h;
    RED4ext::GetParameter(aFrame, &h);
    aFrame->code++;
    if (!aOut) return;
    aOut->X = 0.0f; aOut->Y = 0.0f; aOut->Z = 0.0f; aOut->W = 0.0f;
    RED4ext::Vector4 v{};
    if (VRBodyCallGet(h.instance, "GetLinearVelocity", "Vector4", &v, sizeof(v))) {
        aOut->X = v.X; aOut->Y = v.Y; aOut->Z = v.Z; aOut->W = 1.0f;
    }
}

void VRBodyGetAngVel(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    RED4ext::Handle<RED4ext::IScriptable> h;
    RED4ext::GetParameter(aFrame, &h);
    aFrame->code++;
    if (!aOut) return;
    aOut->X = 0.0f; aOut->Y = 0.0f; aOut->Z = 0.0f; aOut->W = 0.0f;
    RED4ext::Vector4 v{};
    if (VRBodyCallGet(h.instance, "GetAngularVelocity", "Vector4", &v, sizeof(v))) {
        aOut->X = v.X; aOut->Y = v.Y; aOut->Z = v.Z; aOut->W = 1.0f;
    }
}

// Shape of a physics body. Same empty-descriptor situation as the velocity calls, so the same
// hand-built CStack. Needed to find out what the player's hit-query collider actually is: the
// thing a held ball bumps into is that body, not the locomotion capsule.
void VRBodyGetDims(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    RED4ext::Handle<RED4ext::IScriptable> h;
    RED4ext::GetParameter(aFrame, &h);
    aFrame->code++;
    if (!aOut) return;
    aOut->X = 0.0f; aOut->Y = 0.0f; aOut->Z = 0.0f; aOut->W = 0.0f;
    RED4ext::Vector4 v{};
    if (VRBodyCallGet(h.instance, "GetDimensions", "Vector4", &v, sizeof(v))) {
        aOut->X = v.X; aOut->Y = v.Y; aOut->Z = v.Z; aOut->W = 1.0f;
    }
}

void VRBodyGetCOM(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    RED4ext::Handle<RED4ext::IScriptable> h;
    RED4ext::GetParameter(aFrame, &h);
    aFrame->code++;
    if (!aOut) return;
    aOut->X = 0.0f; aOut->Y = 0.0f; aOut->Z = 0.0f; aOut->W = 0.0f;
    RED4ext::Vector4 v{};
    if (VRBodyCallGet(h.instance, "GetBoundsCenter", "Vector4", &v, sizeof(v))) {
        aOut->X = v.X; aOut->Y = v.Y; aOut->Z = v.Z; aOut->W = 1.0f;
    }
}

void VRBodySetVel(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    RED4ext::Handle<RED4ext::IScriptable> h;
    RED4ext::Vector4 v{};
    RED4ext::GetParameter(aFrame, &h);
    RED4ext::GetParameter(aFrame, &v);
    aFrame->code++;
    const bool ok = VRBodyCallSetVec(h.instance, "SetLinearVelocity", v);
    if (aOut) *aOut = ok;
}

// Flips a body between solver-owned and driven-by-us, at runtime.
//
// This is the whole mechanism behind hands that stop at walls. `simulationType` in the asset decides who
// owns the transform -- Kinematic means we write the pose from the bone and NO contact can ever move it,
// which is why filter masks alone can never make a hand collide with anything. But entPhysicalBodyInterface
// exposes SetIsKinematic, so the asset can stay Kinematic (the known-good state) and only the hands get
// flipped live, driven by velocity the way the basketball is.
//
// Measured why the flip has to be live rather than authored: a collider component built as Dynamic in the
// .ent is NOT held by its bone binding -- both hand capsules free-fell to Z = -8100, eight kilometres under
// the world, while the Kinematic forearm stayed on its bone. A solver-owned body must be driven every frame.
void VRBodySetKinematic(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    RED4ext::Handle<RED4ext::IScriptable> h;
    bool kinematic = true;
    RED4ext::GetParameter(aFrame, &h);
    RED4ext::GetParameter(aFrame, &kinematic);
    aFrame->code++;
    if (aOut) *aOut = false;
    if (!h.instance) return;
    auto* f = VRBodyFunc("SetIsKinematic");
    if (!f) return;
    auto* rtti = RED4ext::CRTTISystem::Get();
    bool v = kinematic;
    RED4ext::CStackType args[1];
    args[0].type = rtti->GetType("Bool");
    args[0].value = &v;
    RED4ext::CStack stack(h.instance, args, 1, nullptr);
    const bool ok = VRSafeExecute(f, &stack);
    if (aOut) *aOut = ok;
}

// Queries off hides a body from the player's movement, and from rays; it still collides. The game's native queues this for the body's proxy, by its id.
void VRBodySetQueryable(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    RED4ext::Handle<RED4ext::IScriptable> h;
    bool queryable = true;
    RED4ext::GetParameter(aFrame, &h);
    RED4ext::GetParameter(aFrame, &queryable);
    aFrame->code++;
    if (aOut) *aOut = false;
    if (!h.instance || VRBodyBroken(h.instance)) return;
    // a body with a PhysX actor of its own: switched at the PhysX step on that actor while it lives (HeldBody.cpp)
    {
        const auto* raw = reinterpret_cast<const uint8_t*>(h.instance);
        if (cvr::heldbody::QueryWanted(*reinterpret_cast<const uint32_t*>(raw + 0x40), *reinterpret_cast<const uint32_t*>(raw + 0x44), queryable)) {
            if (aOut) *aOut = true;
            return;
        }
    }
    auto* f = VRBodyFunc("SetIsQueryable");
    if (!f) return;
    auto* rtti = RED4ext::CRTTISystem::Get();
    bool v = queryable;
    RED4ext::CStackType args[1];
    args[0].type = rtti->GetType("Bool");
    args[0].value = &v;
    RED4ext::CStack stack(h.instance, args, 1, nullptr);
    const bool ok = VRSafeExecute(f, &stack);
    if (aOut) *aOut = ok;
}

// THE BODY A RAY HIT.
//
// physicsTraceResult is 0x60 bytes and script sees three fields of it. The rest, read off the game's own conversion from a
// PhysX hit (sub_1403DDE38 in 2.5): +0x48 the proxy, +0x4C the body within it (the PxActor's userData, which is those two
// packed), +0x50 the shape, +0x54 the distance, +0x59 the proxy's kind (byte 0x34 of the proxy, physics::ProxyType).
// And entPhysicalBodyInterface is nothing but that pair: its natives read +0x40 as the proxy and +0x44 as the body and look
// the rest up, validating the id. So one instance, filled in from a hit, IS the body it hit, and every VRBody* native works
// on it. Measured in game: small props are kind 10 (aggregate), 1 (physical system) or 3 (destruction); kind 7 is the
// world's baked collision, mass 0, and answers nothing.
namespace {
constexpr uint8_t kProxyPhysicalSystem = 1, kProxyDestruction = 3, kProxyAggregate = 10;

bool TraceMoves(uint8_t kind) {
    // 12 is a ragdoll: a body lying dead can be taken by the limb.
    return kind == kProxyPhysicalSystem || kind == kProxyDestruction || kind == kProxyAggregate || kind == 12;
}

RED4ext::IScriptable* NewBody(uint32_t proxy, uint32_t body) {
    auto* cls = RED4ext::CRTTISystem::Get()->GetClass("entPhysicalBodyInterface");
    if (!cls) return nullptr;
    auto* instance = static_cast<RED4ext::IScriptable*>(cls->CreateInstance(true));
    if (!instance) return nullptr;
    auto* raw = reinterpret_cast<uint8_t*>(instance);
    *reinterpret_cast<uint32_t*>(raw + 0x40) = proxy;
    *reinterpret_cast<uint32_t*>(raw + 0x44) = body;
    return instance;
}

// Once per proxy in the log: the numbers the next test is read from.
void TraceNote(uint32_t proxy, uint32_t body, uint32_t shape, uint8_t kind, float mass) {
    static uint32_t s_seen[256] = {};
    static uint32_t s_next = 0;
    for (const auto seen : s_seen)
        if (seen == proxy + 1) return;
    s_seen[s_next++ % 256] = proxy + 1;
    Log("[HandColl] touched proxy=%u body=%u shape=%u kind=%u mass=%.2f\n", proxy, body, shape, kind, mass);
}

struct TraceIds { uint32_t proxy, body, shape; uint8_t kind; };

TraceIds ReadTrace(const RED4ext::physics::TraceResult& trace) {
    const auto* raw = reinterpret_cast<const uint8_t*>(&trace);
    return { *reinterpret_cast<const uint32_t*>(raw + 0x48), *reinterpret_cast<const uint32_t*>(raw + 0x4C),
             *reinterpret_cast<const uint32_t*>(raw + 0x50), raw[0x59] };
}
}  // namespace

// VRTraceInfo(trace) -> Vector4: x the kind, y the proxy, z the body, w the mass. All zero when the trace carries no proxy.
void VRTraceInfo(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    alignas(16) RED4ext::physics::TraceResult trace{};
    RED4ext::GetParameter(aFrame, &trace);
    aFrame->code++;
    if (!aOut) return;
    *aOut = RED4ext::Vector4{};
    const auto ids = ReadTrace(trace);
    if (ids.proxy == 0) return;
    float mass = 0.0f;
    if (TraceMoves(ids.kind)) {
        static RED4ext::IScriptable* s_scratch = nullptr;
        if (!s_scratch) s_scratch = NewBody(ids.proxy, ids.body);
        if (s_scratch) {
            auto* raw = reinterpret_cast<uint8_t*>(s_scratch);
            *reinterpret_cast<uint32_t*>(raw + 0x40) = ids.proxy;
            *reinterpret_cast<uint32_t*>(raw + 0x44) = ids.body;
            VRBodyCallGet(s_scratch, "GetMass", "Float", &mass, sizeof(mass));
        }
    }
    TraceNote(ids.proxy, ids.body, ids.shape, ids.kind, mass);
    aOut->X = static_cast<float>(ids.kind);
    aOut->Y = static_cast<float>(ids.proxy);
    aOut->Z = static_cast<float>(ids.body);
    aOut->W = mass;
}

// VRTraceBody(trace) -> the body, for the VRBody* natives. Null for a kind that does not move.
void VRTraceBody(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Handle<RED4ext::IScriptable>* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    alignas(16) RED4ext::physics::TraceResult trace{};
    RED4ext::GetParameter(aFrame, &trace);
    aFrame->code++;
    if (!aOut) return;
    const auto ids = ReadTrace(trace);
    if (ids.proxy == 0 || !TraceMoves(ids.kind)) return;
    if (auto* instance = NewBody(ids.proxy, ids.body)) *aOut = RED4ext::Handle<RED4ext::IScriptable>(instance);
}

// WHAT A BODY WEIGHS, kept: a look at what a hand points at spent up to 33 ms on the bodies it found, not on finding them, and a body's mass stays what it is.
namespace {
struct MassSeen { uint32_t proxy, body; float mass; uint64_t at; };
double massMs = 0.0;
float MassOf(RED4ext::IScriptable* scratch, uint32_t proxy, uint32_t body) {
    static thread_local MassSeen seen[256]{};
    MassSeen& s = seen[(proxy * 31u + body) % 256];
    const uint64_t now = GetTickCount64();
    if (s.at && s.proxy == proxy && s.body == body && now - s.at < 60000) return s.mass;
    LARGE_INTEGER a, b, f;
    QueryPerformanceCounter(&a);
    float mass = 0.0f;
    VRBodyCallGet(scratch, "GetMass", "Float", &mass, sizeof(mass));
    QueryPerformanceCounter(&b);
    QueryPerformanceFrequency(&f);
    massMs += double(b.QuadPart - a.QuadPart) * 1000.0 / double(f.QuadPart);
    s = MassSeen{proxy, body, mass, now};
    return mass;
}
}  // namespace

// VRNearBody(center, radius, most mass) -> Vector4 as VRTraceInfo: what a hand closing here can take, nearest first, from PhysX's own list. All zero if nothing is.
void VRNearBody(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    RED4ext::Vector4 center{};
    float radius = 0.1f, most = 30.0f;
    RED4ext::GetParameter(aFrame, &center);
    RED4ext::GetParameter(aFrame, &radius);
    RED4ext::GetParameter(aFrame, &most);
    aFrame->code++;
    if (!aOut) return;
    *aOut = RED4ext::Vector4{};
    cvr::debugview::NearBody found[16];
    const int count = cvr::debugview::FindNear(center.X, center.Y, center.Z, radius, found, 16);
    static RED4ext::IScriptable* s_scratch = nullptr;
    for (int i = 0; i < count; ++i) {
        const int kind = cvr::debugview::ProxyKind(found[i].proxy);
        if (!TraceMoves(uint8_t(kind))) continue;
        if (!s_scratch) s_scratch = NewBody(found[i].proxy, found[i].body);
        if (!s_scratch) return;
        auto* raw = reinterpret_cast<uint8_t*>(s_scratch);
        *reinterpret_cast<uint32_t*>(raw + 0x40) = found[i].proxy;
        *reinterpret_cast<uint32_t*>(raw + 0x44) = found[i].body;
        const float mass = MassOf(s_scratch, found[i].proxy, found[i].body);
        if (mass <= 0.01f || mass > most || (kind == 1 && mass == 1.0f)) continue;   // exactly 1 kg of kind 1: the hand's own capsule (tools/ent/add_hand_physics.py), taken 218 times in one session   // no mass: not the simulation's to move, a character's collider for one
        *aOut = RED4ext::Vector4{float(kind), float(found[i].proxy), float(found[i].body), mass};
        return;
    }
}

// VRPickCone(from, a, b, cone, range, most mass, keep proxy, keep body) -> Vector4 as VRTraceInfo: what a hand points at, along either direction. All zero if nothing.
void VRPickCone(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    RED4ext::Vector4 from{}, a{}, b{};
    float cone = 0.15f, range = 6.0f, most = 6.0f;
    int32_t keepProxy = 0, keepBody = 0;
    RED4ext::GetParameter(aFrame, &from);
    RED4ext::GetParameter(aFrame, &a);
    RED4ext::GetParameter(aFrame, &b);
    RED4ext::GetParameter(aFrame, &cone);
    RED4ext::GetParameter(aFrame, &range);
    RED4ext::GetParameter(aFrame, &most);
    RED4ext::GetParameter(aFrame, &keepProxy);
    RED4ext::GetParameter(aFrame, &keepBody);
    aFrame->code++;
    if (!aOut) return;
    *aOut = RED4ext::Vector4{};
    const float f[3] = {from.X, from.Y, from.Z}, da[3] = {a.X, a.Y, a.Z}, db[3] = {b.X, b.Y, b.Z};
    cvr::debugview::NearBody found[16];
    // A LONG LOOK, in the log: what of it took so long. Frames of 40 to 70 ms were measured in the pointing.
    struct Look {
        LARGE_INTEGER start{}, walked{};
        int count = 0;
        Look() { QueryPerformanceCounter(&start); massMs = 0.0; }
        ~Look() {
            LARGE_INTEGER end, frequency;
            QueryPerformanceCounter(&end);
            QueryPerformanceFrequency(&frequency);
            const double unit = 1000.0 / double(frequency.QuadPart);
            const double whole = double(end.QuadPart - start.QuadPart) * unit;
            static int said = 0;
            if (whole > 8.0 && said < 60) {
                ++said;
                Log("[pick] a look took %.1f ms: %.1f walking the actors, %.1f on %d of them, of that %.1f asking masses\n", whole, double(walked.QuadPart - start.QuadPart) * unit, double(end.QuadPart - walked.QuadPart) * unit, count, massMs);
            }
        }
    } look;
    const int count = cvr::debugview::PickCone(f, da, db, cone, range, uint32_t(keepProxy), uint32_t(keepBody), found, 16);
    QueryPerformanceCounter(&look.walked);
    look.count = count;
    static RED4ext::IScriptable* s_scratch = nullptr;
    for (int i = 0; i < count; ++i) {
        const int kind = cvr::debugview::ProxyKind(found[i].proxy);
        if (!TraceMoves(uint8_t(kind)) || kind == 12) continue;
        if (!s_scratch) s_scratch = NewBody(found[i].proxy, found[i].body);
        if (!s_scratch) return;
        auto* raw = reinterpret_cast<uint8_t*>(s_scratch);
        *reinterpret_cast<uint32_t*>(raw + 0x40) = found[i].proxy;
        *reinterpret_cast<uint32_t*>(raw + 0x44) = found[i].body;
        const float mass = MassOf(s_scratch, found[i].proxy, found[i].body);
        if (mass <= 0.01f || mass > most || (kind == 1 && mass == 1.0f)) continue;   // exactly 1 kg of kind 1: the hand's own capsule (tools/ent/add_hand_physics.py), taken 218 times in one session
        *aOut = RED4ext::Vector4{float(kind), float(found[i].proxy), float(found[i].body), mass};
        return;
    }
}

// A BODY LYING DEAD, IN ONE CALL. Done from the script it was a call per limb and per thing asked of it: some sixty a frame through CET, and 6 ms measured. Its limbs keep their weight: the limb in the hand goes where it
// goes and the rest hangs from it, each no further below the hand than it lay from that limb. Only what would hang lower than that is held up, gently, so a hand near the floor drags a body with all its weight and
// a hand raised lifts it from the top down. Limbs at the same spot are one (of 44 bodies, some 20 places).
namespace {
constexpr int kCorpseLimbs = 64, kCorpses = 16;
constexpr float kLimbLongest = 1.7f, kLimbLift = 14.0f, kLimbPull = 30.0f, kLimbCalm = 0.96f, kLimbRise = 1.6f;
// Towed along the floor after a limb in the hand (VRCorpseHold, other -2): past how far it lay from it plus kTowSlack, a
// limb is drawn toward it at kTowPull per metre past (1/s^2), at most kTowMost m/s^2 and kTowSpeed m/s that way.
constexpr float kTowSlack = 0.05f, kTowPull = 40.0f, kTowMost = 20.0f, kTowSpeed = 2.0f;
struct CorpseLimb { uint16_t body; float hang; };
struct Corpse { uint32_t proxy; int bodies; int count; CorpseLimb limbs[kCorpseLimbs]; float whole; int heldWhenMeasured; };
Corpse corpses[kCorpses];
int corpseNext = 0;
RED4ext::IScriptable* corpseScratch = nullptr;

RED4ext::IScriptable* CorpseBody(uint32_t proxy, uint32_t body) {
    if (!corpseScratch) corpseScratch = NewBody(proxy, body);
    if (!corpseScratch) return nullptr;
    auto* raw = reinterpret_cast<uint8_t*>(corpseScratch);
    *reinterpret_cast<uint32_t*>(raw + 0x40) = proxy;
    *reinterpret_cast<uint32_t*>(raw + 0x44) = body;
    return corpseScratch;
}
Corpse* CorpseFind(uint32_t proxy) {
    for (auto& c : corpses)
        if (c.proxy == proxy) return &c;
    return nullptr;
}
// `held` is what the distances are from.
Corpse* CorpseMeasure(uint32_t proxy, int held) {
    Corpse* c = CorpseFind(proxy);
    if (!c) {
        c = &corpses[corpseNext];
        corpseNext = (corpseNext + 1) % kCorpses;
    }
    c->proxy = proxy;
    c->count = 0;
    c->whole = 0.0f;
    c->heldWhenMeasured = held;
    const int bodies = cvr::debugview::ProxyBodies(proxy);
    c->bodies = bodies < kCorpseLimbs ? bodies : kCorpseLimbs;
    RED4ext::Transform from{};
    auto* b = CorpseBody(proxy, uint32_t(held));
    if (!b || !VRBodyCallGet(b, "GetTransform", "Transform", &from, sizeof(from))) return c;
    float seen[kCorpseLimbs + 1][3];
    int seenCount = 0;
    seen[seenCount][0] = from.position.X; seen[seenCount][1] = from.position.Y; seen[seenCount][2] = from.position.Z; ++seenCount;
    float mass = 0.0f;
    if (VRBodyCallGet(b, "GetMass", "Float", &mass, sizeof(mass))) c->whole += mass;
    for (int i = 0; i < c->bodies; ++i) {
        if (i == held) continue;
        b = CorpseBody(proxy, uint32_t(i));
        RED4ext::Transform t{};
        if (!b || !VRBodyCallGet(b, "GetTransform", "Transform", &t, sizeof(t))) continue;
        bool same = false;
        for (int s = 0; s < seenCount && !same; ++s) {
            const float dx = t.position.X - seen[s][0], dy = t.position.Y - seen[s][1], dz = t.position.Z - seen[s][2];
            same = dx * dx + dy * dy + dz * dz < 1e-6f;
        }
        if (same) continue;
        seen[seenCount][0] = t.position.X; seen[seenCount][1] = t.position.Y; seen[seenCount][2] = t.position.Z; ++seenCount;
        const float dx = t.position.X - from.position.X, dy = t.position.Y - from.position.Y, dz = t.position.Z - from.position.Z;
        const float d = std::sqrt(dx * dx + dy * dy + dz * dz);
        c->limbs[c->count++] = CorpseLimb{uint16_t(i), d < kLimbLongest ? d : kLimbLongest};
        mass = 0.0f;
        if (VRBodyCallGet(b, "GetMass", "Float", &mass, sizeof(mass))) c->whole += mass;
    }
    Log("[corpse] proxy %u: %d bodies, %d more places, %.0f kg\n", proxy, c->bodies, c->count, c->whole);
    return c;
}
}  // namespace

// VRBodySurface(proxy, body, point) -> the point of the body's own shapes nearest `point` (the game's world), w how far
// (0 inside); w -1 when the body has no actor or no shape that is measured.
void VRBodySurface(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t proxy = 0, body = 0;
    RED4ext::Vector4 point{};
    RED4ext::GetParameter(aFrame, &proxy);
    RED4ext::GetParameter(aFrame, &body);
    RED4ext::GetParameter(aFrame, &point);
    aFrame->code++;
    if (!aOut) return;
    *aOut = RED4ext::Vector4{0.f, 0.f, 0.f, -1.f};
    if (proxy <= 0) return;
    void* scene = nullptr;
    float origin[3] = {0.f, 0.f, 0.f};
    void* actor = cvr::debugview::RigidDynamicOf(uint32_t(proxy), uint32_t(body), &scene, origin);
    if (!actor || cvr::debugview::BodyGone(uint32_t(proxy), uint32_t(body))) return;
    const float p[3] = {point.X, point.Y, point.Z};
    float c[3], d = 0.f;
    if (cvr::physhands::BodySurface(actor, origin, p, c, &d)) *aOut = RED4ext::Vector4{c[0], c[1], c[2], d};
}

namespace {
float g_graspSeatTurn[2][4] = {{0.f, 0.f, 0.f, 1.f}, {0.f, 0.f, 0.f, 1.f}};
}  // namespace

// VRGraspCapsule(side, style, center, axis, halfLength, radius, seat) -> the part's seat in the hand's frame; w 1 seated this call,
// 0 held as it was, -1 refused (another grasp has the hand, or no palm model yet) or let go.
// The grasp the fingers take on a capsule given in the hand bone's frame (its axis a unit vector there), as GraspPerson
// gives a living person's part (src/Anim/GraspPose.cpp): the script's carry of a body lying dead asks for it each frame a
// hand holds a part of it, and moves the place it took to the seat. Style 0 lets go. Styles as GraspPerson: 1 round it
// like a can, 2 so with the fingers wide, 3 gently, 4 the palm on it.
void VRGraspCapsule(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t side = 0, style = 0;
    RED4ext::Vector4 center{}, axis{};
    float half = 0.f, radius = 0.f, palmGap = -1.f;
    RED4ext::GetParameter(aFrame, &side);
    RED4ext::GetParameter(aFrame, &style);
    RED4ext::GetParameter(aFrame, &center);
    RED4ext::GetParameter(aFrame, &axis);
    RED4ext::GetParameter(aFrame, &half);
    RED4ext::GetParameter(aFrame, &radius);
    RED4ext::GetParameter(aFrame, &palmGap);
    aFrame->code++;
    if (!aOut) return;
    *aOut = RED4ext::Vector4{0.f, 0.f, 0.f, -1.f};
    if (side < 0 || side > 1) return;
    if (style <= 0) {
        cvr::anim::GraspPerson(side, 0, nullptr, nullptr, 0.f, 0.f);
        return;
    }
    // the capsule's axis as its local z: the shortest turn from z to the axis given
    float a[3] = {axis.X, axis.Y, axis.Z};
    const float n = std::sqrt(a[0] * a[0] + a[1] * a[1] + a[2] * a[2]);
    if (n < 1e-4f) return;
    for (float& v : a) v /= n;
    float q[4];
    if (a[2] > 0.9999f) {
        q[0] = q[1] = q[2] = 0.f; q[3] = 1.f;
    } else if (a[2] < -0.9999f) {
        q[0] = 1.f; q[1] = q[2] = q[3] = 0.f;   // a half turn about x
    } else {
        q[0] = -a[1]; q[1] = a[0]; q[2] = 0.f; q[3] = 1.f + a[2];   // (z x a, 1 + z.a)
        const float m = std::sqrt(q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]);
        for (float& v : q) v /= m;
    }
    const float p[3] = {center.X, center.Y, center.Z};
    float seatP[3] = {0.f, 0.f, 0.f}, seatQ[4] = {0.f, 0.f, 0.f, 1.f};
    bool seated = false;
    if (!cvr::anim::GraspPerson(side, style, p, q, std::max(0.f, half), std::max(0.005f, radius), seatP, seatQ, &seated, palmGap)) return;
    *aOut = RED4ext::Vector4{seatP[0], seatP[1], seatP[2], seated ? 1.f : 0.f};
    // the turn the seat gives the part, in the hand's frame (VRGraspSeatTurn): seat * capsule^-1
    float* t = g_graspSeatTurn[side];
    t[0] = 0.f; t[1] = 0.f; t[2] = 0.f; t[3] = 1.f;
    if (seated) {
        const float qi[4] = {-q[0], -q[1], -q[2], q[3]};
        const float* a2 = seatQ;
        t[0] = a2[3] * qi[0] + a2[0] * qi[3] + a2[1] * qi[2] - a2[2] * qi[1];
        t[1] = a2[3] * qi[1] - a2[0] * qi[2] + a2[1] * qi[3] + a2[2] * qi[0];
        t[2] = a2[3] * qi[2] + a2[0] * qi[1] - a2[1] * qi[0] + a2[2] * qi[3];
        t[3] = a2[3] * qi[3] - a2[0] * qi[0] - a2[1] * qi[1] - a2[2] * qi[2];
        const float m = std::sqrt(t[0] * t[0] + t[1] * t[1] + t[2] * t[2] + t[3] * t[3]);
        if (m > 1e-6f && std::isfinite(m))
            for (int k = 0; k < 4; ++k) t[k] /= m;
        else
            t[0] = t[1] = t[2] = 0.f, t[3] = 1.f;
    }
}

// VRGraspSeatTurn(side) -> Quaternion: the turn the last VRGraspCapsule that seated the part gave it, in the hand bone's
// frame (a limb held round lies across the palm before the knuckles): the caller turns the part by it. None (identity)
// when that call did not seat it.
void VRGraspSeatTurn(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Quaternion* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t side = 0;
    RED4ext::GetParameter(aFrame, &side);
    aFrame->code++;
    if (!aOut) return;
    aOut->i = 0.f; aOut->j = 0.f; aOut->k = 0.f; aOut->r = 1.f;
    if (side != 0 && side != 1) return;
    const float* t = g_graspSeatTurn[side];
    aOut->i = t[0]; aOut->j = t[1]; aOut->k = t[2]; aOut->r = t[3];
}

// VRHandHoldBody(side, proxy, body, pos, rot, grip) -> Bool: the body held by the hand at a point of it, as a pistol is
// (src/Natives/PhysHands.cpp HoldBody): pos xyz, rot its pose in the hand bone's frame, pos w the grip's torque (N m); grip
// xyz the point held in the body's frame. At each PhysX step the point goes where the hand bone is and the body turns
// toward the hand's turn as far as the grip holds it; the rest of its ragdoll hangs from it, and the drawn hand turns with
// what its weight turns past that. Asked every frame it is held (unasked 0.3 s, it is let go); a proxy of 0 or less lets
// go, the body going on as it moved. False when the body has no actor of the PhysX this plugin reads, or none in the
// hands' scene.
void VRHandHoldBody(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t side = 0, proxy = 0, body = 0;
    RED4ext::Vector4 pos{}, grip{};
    RED4ext::Quaternion rot{};
    RED4ext::GetParameter(aFrame, &side);
    RED4ext::GetParameter(aFrame, &proxy);
    RED4ext::GetParameter(aFrame, &body);
    RED4ext::GetParameter(aFrame, &pos);
    RED4ext::GetParameter(aFrame, &rot);
    RED4ext::GetParameter(aFrame, &grip);
    aFrame->code++;
    bool ok = false;
    // THE PART'S OWN BODY. The body the script took is the one nearest the palm when the hand closed, and that is not always
    // the bone's under it: a palm on a shin had a neighbour 25 cm off it, the shin's point pinned to that, the leg left on the
    // floor. The body whose own shapes are nearest the point held is held instead, the script's pose carried onto it.
    // Found once for each part taken: a look walks every scene (0.3 to 2 ms), and the script asks every frame.
    static void* known[2] = {nullptr, nullptr};
    static void* knownPart[2] = {nullptr, nullptr};
    static void* knownScene[2] = {nullptr, nullptr};
    static uint32_t knownProxy[2] = {0, 0}, knownBody[2] = {0, 0}, knownPartBody[2] = {0, 0};
    static float knownGrip[2][3] = {};
    if (side == 0 || side == 1) {
        if (proxy <= 0) {
            known[side] = knownPart[side] = nullptr;
            ok = cvr::physhands::HoldBody(side, nullptr, nullptr, 0, 0, nullptr, nullptr, nullptr);
        } else {
            if (!known[side] || knownProxy[side] != uint32_t(proxy) || knownBody[side] != uint32_t(body) || knownGrip[side][0] != grip.X ||
                knownGrip[side][1] != grip.Y || knownGrip[side][2] != grip.Z || cvr::debugview::BodyGone(uint32_t(proxy), uint32_t(body)) ||
                cvr::debugview::BodyGone(uint32_t(proxy), knownPartBody[side])) {
                void* scene = nullptr;
                float origin[3] = {0.f, 0.f, 0.f};
                known[side] = cvr::debugview::RigidDynamicOf(uint32_t(proxy), uint32_t(body), &scene, origin);
                knownPart[side] = known[side];
                knownScene[side] = scene;
                knownPartBody[side] = uint32_t(body);
                knownProxy[side] = uint32_t(proxy);
                knownBody[side] = uint32_t(body);
                knownGrip[side][0] = grip.X; knownGrip[side][1] = grip.Y; knownGrip[side][2] = grip.Z;
                if (known[side]) {
                    cvr::debugview::NoteActor(uint32_t(proxy), uint32_t(body), known[side]);
                    const float local[3] = {grip.X, grip.Y, grip.Z};
                    float world[3], partOrigin[3], d = 0.f;
                    uint32_t partBody = 0;
                    void* partActor = nullptr;
                    void* partScene = nullptr;
                    if (cvr::physhands::ActorPoint(known[side], origin, local, world) &&
                        cvr::debugview::NearestBodyOf(uint32_t(proxy), world, 0.25f, &partBody, &partActor, &partScene, partOrigin, &d) &&
                        partActor && partBody != uint32_t(body) && partScene == scene) {
                        Log("[corpse] proxy %u: the point held is %.3f m off body %u's shapes, body %u's own; that one held\n", uint32_t(proxy), d, partBody,
                            uint32_t(body));
                        knownPart[side] = partActor;
                        knownPartBody[side] = partBody;
                        cvr::debugview::NoteActor(uint32_t(proxy), partBody, partActor);
                    }
                }
            }
            if (known[side] && knownPart[side]) {
                const float p[4] = {pos.X, pos.Y, pos.Z, pos.W};
                const float q[4] = {rot.i, rot.j, rot.k, rot.r};
                const float g[4] = {grip.X, grip.Y, grip.Z, grip.W};
                ok = cvr::physhands::HoldBody(side, knownPart[side], knownScene[side], uint32_t(proxy), knownPartBody[side], p, q, g,
                                              known[side], uint32_t(body));
            }
        }
    }
    if (aOut) *aOut = ok;
}

// VRCorpseHold(proxy, the limb in a hand, the limb in the other or -1, how high the first is, dt) -> the whole body's mass. Measured anew each time a limb is taken (held changed).
// The other as -2: the hand has an arm, a leg or the head, and the body hangs from it by its own weight -- nothing is held
// up. It is towed along the floor instead: a limb farther along the floor from the one in the hand than it lay from it
// (plus kTowSlack) is drawn toward it along the floor. Pulled at the foot alone, the ragdoll's chain of joints gave and the
// trunk stayed where it lay however hard the foot was drawn (800 N: the hips 1 cm in 60 cm).
void VRCorpseHold(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, float* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t proxy = 0, held = 0, other = -1;
    float z = 0.0f, dt = 0.016f;
    RED4ext::GetParameter(aFrame, &proxy);
    RED4ext::GetParameter(aFrame, &held);
    RED4ext::GetParameter(aFrame, &other);
    RED4ext::GetParameter(aFrame, &z);
    RED4ext::GetParameter(aFrame, &dt);
    aFrame->code++;
    if (aOut) *aOut = 0.0f;
    Corpse* c = CorpseFind(uint32_t(proxy));
    if (other <= -2) {
        if (!c || c->heldWhenMeasured != held) c = CorpseMeasure(uint32_t(proxy), held);
        if (aOut) *aOut = c->whole;
        RED4ext::Transform h{};
        auto* hb = CorpseBody(uint32_t(proxy), uint32_t(held));
        if (!hb || !VRBodyCallGet(hb, "GetTransform", "Transform", &h, sizeof(h))) return;
        const float hx = h.position.X, hy = h.position.Y;
        for (int i = 0; i < c->count; ++i) {
            const CorpseLimb& limb = c->limbs[i];
            if (limb.body == held) continue;
            auto* b = CorpseBody(uint32_t(proxy), limb.body);
            RED4ext::Transform t{};
            if (!b || !VRBodyCallGet(b, "GetTransform", "Transform", &t, sizeof(t))) continue;
            const float dx = hx - t.position.X, dy = hy - t.position.Y;
            const float along = std::sqrt(dx * dx + dy * dy);
            const float past = along - limb.hang - kTowSlack;
            if (past <= 0.0f || along < 1e-3f) continue;
            RED4ext::Vector4 v{};
            if (!VRBodyCallGet(b, "GetLinearVelocity", "Vector4", &v, sizeof(v))) continue;
            const float ux = dx / along, uy = dy / along;
            const float a = past * kTowPull < kTowMost ? past * kTowPull : kTowMost;
            const float toward = v.X * ux + v.Y * uy;
            if (toward >= kTowSpeed) continue;
            const float add = std::min(a * dt, kTowSpeed - toward);
            VRBodyCallSetVec(b, "SetLinearVelocity", RED4ext::Vector4{v.X + ux * add, v.Y + uy * add, v.Z, 0.0f});
        }
        return;
    }
    if (!c || c->heldWhenMeasured != held) c = CorpseMeasure(uint32_t(proxy), held);
    if (aOut) *aOut = c->whole;
    for (int i = 0; i < c->count; ++i) {
        const CorpseLimb& limb = c->limbs[i];
        if (limb.body == held || limb.body == other) continue;
        auto* b = CorpseBody(uint32_t(proxy), limb.body);
        RED4ext::Transform t{};
        if (!b || !VRBodyCallGet(b, "GetTransform", "Transform", &t, sizeof(t))) continue;
        const float low = z - limb.hang - t.position.Z;
        if (low <= 0.0f) continue;
        RED4ext::Vector4 v{};
        if (!VRBodyCallGet(b, "GetLinearVelocity", "Vector4", &v, sizeof(v))) continue;
        const float pull = low * kLimbPull;
        const float up = pull < kLimbLift ? pull : kLimbLift;
        const float vz = v.Z + 9.81f * dt + up * dt;
        VRBodyCallSetVec(b, "SetLinearVelocity", RED4ext::Vector4{v.X * kLimbCalm, v.Y * kLimbCalm, vz < kLimbRise ? vz : kLimbRise, 0.0f});
    }
}

// VRCorpseSpeed(proxy) -> the fastest of its limbs, m/s; below 0 when it is no ragdoll any more.
void VRCorpseSpeed(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, float* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t proxy = 0;
    RED4ext::GetParameter(aFrame, &proxy);
    aFrame->code++;
    if (aOut) *aOut = -1.0f;
    if (cvr::debugview::ProxyKind(uint32_t(proxy)) != 12) return;
    Corpse* c = CorpseFind(uint32_t(proxy));
    if (!c) c = CorpseMeasure(uint32_t(proxy), 0);
    float most = 0.0f;
    for (int i = 0; i < c->count; ++i) {
        auto* b = CorpseBody(uint32_t(proxy), c->limbs[i].body);
        RED4ext::Vector4 v{};
        if (!b || !VRBodyCallGet(b, "GetLinearVelocity", "Vector4", &v, sizeof(v))) continue;
        const float s = v.X * v.X + v.Y * v.Y + v.Z * v.Z;
        if (s > most) most = s;
    }
    if (aOut) *aOut = std::sqrt(most);
}

// VRCorpseFreeze(proxy, frozen): every body of it kinematic where it lies, or back in the simulation.
void VRCorpseFreeze(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t proxy = 0;
    bool frozen = true;
    RED4ext::GetParameter(aFrame, &proxy);
    RED4ext::GetParameter(aFrame, &frozen);
    aFrame->code++;
    if (aOut) *aOut = false;
    Corpse* c = CorpseFind(uint32_t(proxy));
    auto* f = VRBodyFunc("SetIsKinematic");
    if (!c || !f) return;
    auto* rtti = RED4ext::CRTTISystem::Get();
    for (int i = 0; i < c->bodies; ++i) {
        auto* b = CorpseBody(uint32_t(proxy), uint32_t(i));
        if (!b) return;
        bool v = frozen;
        RED4ext::CStackType args[1];
        args[0].type = rtti->GetType("Bool");
        args[0].value = &v;
        RED4ext::CStack stack(b, args, 1, nullptr);
        VRSafeExecute(f, &stack);
    }
    if (aOut) *aOut = true;
}

// VRBodyInfo(proxy, body) -> Vector4 as VRTraceInfo: its kind, ids and mass. All zero if it is no thing that moves.
void VRBodyInfo(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t proxy = 0, body = 0;
    RED4ext::GetParameter(aFrame, &proxy);
    RED4ext::GetParameter(aFrame, &body);
    aFrame->code++;
    if (!aOut) return;
    *aOut = RED4ext::Vector4{};
    const int kind = cvr::debugview::ProxyKind(uint32_t(proxy));
    if (!TraceMoves(uint8_t(kind))) return;
    static RED4ext::IScriptable* s_scratch = nullptr;
    if (!s_scratch) s_scratch = NewBody(uint32_t(proxy), uint32_t(body));
    if (!s_scratch) return;
    auto* raw = reinterpret_cast<uint8_t*>(s_scratch);
    *reinterpret_cast<uint32_t*>(raw + 0x40) = uint32_t(proxy);
    *reinterpret_cast<uint32_t*>(raw + 0x44) = uint32_t(body);
    const float mass = MassOf(s_scratch, uint32_t(proxy), uint32_t(body));
    *aOut = RED4ext::Vector4{float(kind), float(proxy), float(body), mass};
}

// VRBodyOf(proxy, body) -> the body, for the VRBody* natives.
void VRBodyOf(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Handle<RED4ext::IScriptable>* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t proxy = 0, body = 0;
    RED4ext::GetParameter(aFrame, &proxy);
    RED4ext::GetParameter(aFrame, &body);
    aFrame->code++;
    if (!aOut) return;
    if (auto* instance = NewBody(uint32_t(proxy), uint32_t(body))) *aOut = RED4ext::Handle<RED4ext::IScriptable>(instance);
}

// The body's orientation, beside VRBodyGetPos.
void VRBodyGetRot(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Quaternion* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    RED4ext::Handle<RED4ext::IScriptable> h;
    RED4ext::GetParameter(aFrame, &h);
    aFrame->code++;
    if (!aOut) return;
    aOut->i = 0.0f; aOut->j = 0.0f; aOut->k = 0.0f; aOut->r = 1.0f;
    RED4ext::Transform t{};
    if (VRBodyCallGet(h.instance, "GetTransform", "Transform", &t, sizeof(t))) *aOut = t.orientation;
}

// Is the body currently solver-owned? Read back rather than assumed, because SetIsKinematic can silently
// fail the same way every other call on this interface can.
void VRBodyIsKinematic(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    RED4ext::Handle<RED4ext::IScriptable> h;
    RED4ext::GetParameter(aFrame, &h);
    aFrame->code++;
    if (aOut) *aOut = -1;                       // -1 = the call did not go through
    bool k = false;
    if (VRBodyCallGet(h.instance, "IsKinematic", "Bool", &k, sizeof(k)) && aOut)
        *aOut = k ? 1 : 0;
}

// The body's own pose. GetBoundsCenter (VRBodyGetCOM) is an AABB centre and drifts with the shape's
// orientation; GetTransform is the actual pose, which is what the IK target has to follow.
void VRBodyGetPos(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    RED4ext::Handle<RED4ext::IScriptable> h;
    RED4ext::GetParameter(aFrame, &h);
    aFrame->code++;
    if (!aOut) return;
    aOut->X = 0.0f; aOut->Y = 0.0f; aOut->Z = 0.0f; aOut->W = 0.0f;
    // Transform is { position, orientation } (RED4ext/Scripting/Natives/Transform.hpp, and the proxy's property 3
    // that GetTransform reads, sub_1402E298C: identity orientation in its second half). Read the other way round
    // this returned the orientation's i, j, k as the position.
    RED4ext::Transform t{};
    if (VRBodyCallGet(h.instance, "GetTransform", "Transform", &t, sizeof(t))) {
        aOut->X = t.position.X; aOut->Y = t.position.Y; aOut->Z = t.position.Z; aOut->W = 1.0f;
    }
}

// Teleport. The safety net for a driven body: a solver-owned hand can end up behind geometry or, with
// nothing driving it, at the bottom of the world, and no bounded velocity brings it back from there.
void VRBodySetPos(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    RED4ext::Handle<RED4ext::IScriptable> h;
    RED4ext::Vector4 pos{};
    RED4ext::Quaternion rot{};
    RED4ext::GetParameter(aFrame, &h);
    RED4ext::GetParameter(aFrame, &pos);
    RED4ext::GetParameter(aFrame, &rot);
    aFrame->code++;
    if (aOut) *aOut = false;
    if (!h.instance) return;
    auto* f = VRBodyFunc("SetTransform");
    if (!f) return;
    auto* rtti = RED4ext::CRTTISystem::Get();
    RED4ext::Transform t(pos, rot);             // { position, orientation }, as GetTransform above
    RED4ext::CStackType args[1];
    args[0].type = rtti->GetType("Transform");
    args[0].value = &t;
    RED4ext::CStack stack(h.instance, args, 1, nullptr);
    const bool ok = VRSafeExecute(f, &stack);
    if (aOut) *aOut = ok;
}

// Simulation filter masks. `SetSimulationFilter` has the same empty descriptor as the velocity
// calls, so it goes through the same hand-built CStack; its parameter is a physicsSimulationFilter,
// which the RTTI describes as two Uint64 masks.
//
// This is what lets a held ball stop being a physics object entirely: with both masks zero it
// collides with nothing, so it can neither shove the player nor build up penetration inside them.
// The ball's authored masks come from the mesh -- see VRBallConst.SimMask1/2 on the script side.
void VRBodySetSimMasks(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    RED4ext::Handle<RED4ext::IScriptable> h;
    uint64_t m1 = 0, m2 = 0;
    RED4ext::GetParameter(aFrame, &h);
    RED4ext::GetParameter(aFrame, &m1);
    RED4ext::GetParameter(aFrame, &m2);
    aFrame->code++;
    if (aOut) *aOut = false;
    if (!h.instance) return;
    auto* f = VRBodyFunc("SetSimulationFilter");
    if (!f) return;
    auto* rtti = RED4ext::CRTTISystem::Get();
    struct { uint64_t mask1; uint64_t mask2; } filter{ m1, m2 };
    RED4ext::CStackType args[1];
    args[0].type = rtti->GetType("physicsSimulationFilter");
    args[0].value = &filter;
    RED4ext::CStack stack(h.instance, args, 1, nullptr);
    const bool ok = VRSafeExecute(f, &stack);
    if (aOut) *aOut = ok;
}

void VRBodySetAngVel(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    RED4ext::Handle<RED4ext::IScriptable> h;
    RED4ext::Vector4 v{};
    RED4ext::GetParameter(aFrame, &h);
    RED4ext::GetParameter(aFrame, &v);
    aFrame->code++;
    const bool ok = VRBodyCallSetVec(h.instance, "SetAngularVelocity", v);
    if (aOut) *aOut = ok;
}

// One-call recon: everything needed to decide whether the velocity API is usable.
void VRBodyProbe(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::CString* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    RED4ext::Handle<RED4ext::IScriptable> h;
    RED4ext::GetParameter(aFrame, &h);
    aFrame->code++;
    if (!aOut) return;
    std::ostringstream os;
    auto* body = h.instance;
    if (!body) { *aOut = RED4ext::CString("null body handle"); return; }

    const char* names[] = { "GetMass", "GetLinearVelocity", "GetAngularVelocity",
                            "SetLinearVelocity", "SetAngularVelocity" };
    for (const char* n : names) {
        auto* f = VRBodyFunc(n);
        os << n << ": ";
        if (!f) { os << "NOT IN RTTI\n"; continue; }
        os << "native=" << (f->flags.isNative ? 1 : 0)
           << " static=" << (f->flags.isStatic ? 1 : 0)
           << " params=" << f->params.size()
           << " ret=" << (f->returnType && f->returnType->type
                          ? f->returnType->type->GetName().ToString() : "none")
           << " reg=" << f->GetRegIndex() << "\n";
    }

    float mass = -1.0f;
    const bool massOk = VRBodyCallGet(body, "GetMass", "Float", &mass, sizeof(mass));
    os << "GetMass -> ok=" << (massOk ? 1 : 0) << " value=" << mass
       << "  (mesh authors 0.624)\n";

    RED4ext::Vector4 lin{}, ang{};
    const bool linOk = VRBodyCallGet(body, "GetLinearVelocity", "Vector4", &lin, sizeof(lin));
    const bool angOk = VRBodyCallGet(body, "GetAngularVelocity", "Vector4", &ang, sizeof(ang));
    os << "GetLinearVelocity  -> ok=" << (linOk ? 1 : 0)
       << " (" << lin.X << ", " << lin.Y << ", " << lin.Z << ", " << lin.W << ")\n";
    os << "GetAngularVelocity -> ok=" << (angOk ? 1 : 0)
       << " (" << ang.X << ", " << ang.Y << ", " << ang.Z << ", " << ang.W << ")\n";
    *aOut = RED4ext::CString(os.str().c_str());
}

// Full RTTI listing of the class, kept for when a new engine build shuffles the bindings.
void VRDumpBodyFuncs(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::CString* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    if (!aOut) return;
    std::ostringstream os;
    auto* rtti = RED4ext::CRTTISystem::Get();
    auto* cls = rtti->GetClass("entPhysicalBodyInterface");
    if (!cls) cls = rtti->GetClass("PhysicalBodyInterface");
    if (!cls) { *aOut = RED4ext::CString("entPhysicalBodyInterface class not found"); return; }
    os << "funcs=" << cls->funcs.size() << " staticFuncs=" << cls->staticFuncs.size() << "\n";
    for (uint32_t i = 0; i < cls->funcs.size(); ++i) {
        auto* f = cls->funcs[i];
        if (!f) continue;
        os << f->shortName.ToString() << "(";
        for (uint32_t p = 0; p < f->params.size(); ++p) {
            auto* prm = f->params[p];
            if (!prm) continue;
            if (p) os << ", ";
            os << (prm->type ? prm->type->GetName().ToString() : "?")
               << (prm->flags.isOut ? "&" : "");
        }
        os << ") -> " << (f->returnType && f->returnType->type
                          ? f->returnType->type->GetName().ToString() : "void") << "\n";
    }
    *aOut = RED4ext::CString(os.str().c_str());
}

// LEVEL-1 RECON: dump a script-side PhysicalBodyInterface so the PhysX actor behind it can be
// found. The script API only offers SetTransform / AddLinearImpulse / SetIsKinematic -- no way to
// read or set a velocity, which is what every problem today came back to. If the wrapper holds a
// PxRigidDynamic pointer, the plugin can drive the body properly (and later joint it to the hand).
//
// Prints qword slots and marks the ones that look like pointers into a loaded module, with the
// first qword of their target (a vtable pointer, if it is an object).
void VRProbeBody(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::CString* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    RED4ext::Handle<RED4ext::IScriptable> h;
    RED4ext::GetParameter(aFrame, &h);
    aFrame->code++;
    if (!aOut) return;

    std::ostringstream os;
    auto* obj = h.instance;
    if (!obj) { *aOut = RED4ext::CString("null handle"); return; }

    MEMORY_BASIC_INFORMATION mbi{};
    auto readable = [&](const void* p) -> bool {
        if (!p) return false;
        if (!VirtualQuery(p, &mbi, sizeof(mbi))) return false;
        if (mbi.State != MEM_COMMIT) return false;
        const DWORD bad = PAGE_NOACCESS | PAGE_GUARD;
        return (mbi.Protect & bad) == 0;
    };

    HMODULE exe = GetModuleHandleW(nullptr);
    os << "obj=" << std::hex << reinterpret_cast<uintptr_t>(obj) << " exe=" << reinterpret_cast<uintptr_t>(exe) << "\n";
    auto* q = reinterpret_cast<uintptr_t*>(obj);
    for (int i = 0; i < 24; ++i) {
        if (!readable(&q[i])) break;
        const uintptr_t v = q[i];
        os << "[" << std::dec << i * 8 << "] " << std::hex << v;
        if (v > 0x10000 && readable(reinterpret_cast<void*>(v))) {
            const uintptr_t inner = *reinterpret_cast<uintptr_t*>(v);
            os << " -> " << inner;
            // A vtable pointer inside the exe means the target is an object we can call into.
            if (inner > reinterpret_cast<uintptr_t>(exe) &&
                inner < reinterpret_cast<uintptr_t>(exe) + 0x8000000) {
                os << " (vtbl rva " << (inner - reinterpret_cast<uintptr_t>(exe)) << ")";
            }
        }
        os << "\n";
    }
    *aOut = RED4ext::CString(os.str().c_str());
}

// Orientation of the solved hand, model space. Composed with the player's world orientation it
// gives the world hand rotation, which the ball needs both to spin with the wrist and to have its
// palm offset pointed the right way.
void VRPalmModelRot(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Quaternion* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t right = 1;
    RED4ext::GetParameter(aFrame, &right);
    aFrame->code++;
    if (!aOut) return;
    const volatile float* q = right ? g_VRPalmModelRotR : g_VRPalmModelRotL;
    aOut->i = q[0]; aOut->j = q[1]; aOut->k = q[2]; aOut->r = q[3];
}

// Body bone in model space. 0 hips, 1 spine, 2 chest, 3 neck, 4 head, 5-7 left leg, 8-10 right
// leg. W = 1 when the slot holds a real bone.
void VRBodyBonePos(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t slot = 0;
    RED4ext::GetParameter(aFrame, &slot);
    aFrame->code++;
    if (!aOut) return;
    aOut->X = 0.0f; aOut->Y = 0.0f; aOut->Z = 0.0f; aOut->W = 0.0f;
    if (slot < 0 || slot > 10 || !g_VRBodyBoneOk[slot]) return;
    aOut->X = g_VRBodyBone[slot][0];
    aOut->Y = g_VRBodyBone[slot][1];
    aOut->Z = g_VRBodyBone[slot][2];
    aOut->W = 1.0f;
}

void VRCamModelPos(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    if (!aOut) return;
    aOut->X = g_VRCamModelPos[0]; aOut->Y = g_VRCamModelPos[1]; aOut->Z = g_VRCamModelPos[2];
    aOut->W = g_VRPalmModelValid ? 1.0f : 0.0f;
}

// ---- THE PLAYER'S OWN TRANSFORM, AS THE ENGINE HAS IT THIS FRAME -------------------------------
//
// Published by the body-yaw store site (src/Hooks/BodyYawCensus.cpp -> BodyYawFollowTick), which
// reads the engine's own state object rather than asking the script VM. Declared here rather than
// pulled in through Camera/CameraState.hpp so this file keeps its short include list; the definition
// carries the export.
//
// WHY A SCRIPT WANTS IT. `Game.GetPlayer():GetWorldPosition()` and this are the SAME quantity read at
// two different points in the frame, so their difference has no systematic part whatever -- no anchor
// mismatch, no height offset, none of what makes the view-composed routes (VRViewWorldPos and
// everything built on it) unusable for absolute placement. What is left in that difference is time,
// and nothing else.
//
// That matters to anything a script has to place in the WORLD beside the drawn hand. Bones do not
// need it -- a rig write lands in the pose the same frame draws it -- but a spawned entity is put
// where the script's own reading says the hand is, and if that reading is a frame old the entity
// trails the hand by a frame of travel: invisible standing still, a hand's width at a run. With this
// the script can measure that gap instead of predicting it, which is the difference between a
// correction that is exact and one that is a guess about the size of a frame.
//
// W = 1 once the store site has identified the player, 0 before that -- and a caller must check it,
// because 0 means these are the initial zeros and not a position in the world.
extern "C" {
    extern float CyberpunkVR_PlayerEntityPos[3];
    extern float CyberpunkVR_PlayerEntityQuat[4];
    extern int   CyberpunkVR_PlayerEntityValid;
}

void VRPlayerEnginePos(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    if (!aOut) return;
    aOut->X = CyberpunkVR_PlayerEntityPos[0];
    aOut->Y = CyberpunkVR_PlayerEntityPos[1];
    aOut->Z = CyberpunkVR_PlayerEntityPos[2];
    aOut->W = CyberpunkVR_PlayerEntityValid ? 1.0f : 0.0f;
}

// The same instant's body orientation. Yaw only by construction -- it is built from the engine's own
// heading at the store site -- which is what the body has and all a placement needs.
void VRPlayerEngineRot(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Quaternion* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    if (!aOut) return;
    aOut->i = CyberpunkVR_PlayerEntityQuat[0];
    aOut->j = CyberpunkVR_PlayerEntityQuat[1];
    aOut->k = CyberpunkVR_PlayerEntityQuat[2];
    aOut->r = CyberpunkVR_PlayerEntityQuat[3];
}

void VRViewWorldRot(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Quaternion* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    if (aOut) {
        aOut->i = g_VRViewWorldRot[0];
        aOut->j = g_VRViewWorldRot[1];
        aOut->k = g_VRViewWorldRot[2];
        aOut->r = g_VRViewWorldRot[3];
    }
}

// VRSmokeMouthWorldPos(): world mouth point for the exhale smoke (W = 1 valid / 0 not). Tracks the
// real HMD via the same view pose the cig anchor uses.
void VRSmokeMouthWorldPos(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    if (aOut) {
        aOut->X = g_VRSmokeMouthWorldPos[0];
        aOut->Y = g_VRSmokeMouthWorldPos[1];
        aOut->Z = g_VRSmokeMouthWorldPos[2];
        aOut->W = static_cast<float>(g_VRSmokeMouthWorldValid);
    }
}

// VRSmokeMouthWorldRot(): world orientation for the exhale smoke (view pose * smoke offset rot).
void VRSmokeMouthWorldRot(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Quaternion* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    if (aOut) {
        aOut->i = g_VRSmokeMouthWorldRot[0];
        aOut->j = g_VRSmokeMouthWorldRot[1];
        aOut->k = g_VRSmokeMouthWorldRot[2];
        aOut->r = g_VRSmokeMouthWorldRot[3];
    }
}

// SetVRSmokeSmokeOffset(x,y,z,pitch,yaw,roll): live tune of the exhale smoke -- HMD-local position
// (x=right, y=forward, z=up, m) and orientation (deg), independent of the cig mouth anchor.
void SetVRSmokeSmokeOffset(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    float x=0.0f, y=0.0f, z=0.0f, pitch=0.0f, yaw=0.0f, roll=0.0f;
    RED4ext::GetParameter(aFrame, &x); RED4ext::GetParameter(aFrame, &y); RED4ext::GetParameter(aFrame, &z);
    RED4ext::GetParameter(aFrame, &pitch); RED4ext::GetParameter(aFrame, &yaw); RED4ext::GetParameter(aFrame, &roll);
    aFrame->code++;
    g_VRSmokeSmokePos[0]=x; g_VRSmokeSmokePos[1]=y; g_VRSmokeSmokePos[2]=z;
    const float d2r = 0.01745329252f * 0.5f;
    float cp = std::cos(pitch*d2r), sp = std::sin(pitch*d2r);
    float cy = std::cos(yaw*d2r),   sy = std::sin(yaw*d2r);
    float cr = std::cos(roll*d2r),  sr = std::sin(roll*d2r);
    g_VRSmokeSmokeRot[0] = sp*cy*cr + cp*sy*sr;
    g_VRSmokeSmokeRot[1] = cp*sy*cr - sp*cy*sr;
    g_VRSmokeSmokeRot[2] = cp*cy*sr + sp*sy*cr;
    g_VRSmokeSmokeRot[3] = cp*cy*cr - sp*sy*sr;
    if (aOut) *aOut = 1;
}

// SetVRSmokeCigScaleY(y): burn-down -- Y (long-axis) scale of the cig slot bone, 1.0 = full length.
void SetVRSmokeCigScaleY(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    float y = 1.0f;
    RED4ext::GetParameter(aFrame, &y);
    aFrame->code++;
    if (y < 0.02f) y = 0.02f;
    g_VRSmokeCigScaleY = y;
    if (aOut) *aOut = 1;
}

// SetVRSmokeCigChunks(cig, count): burn-down EXPERIMENT via chunk hiding. Shows only the low `count`
// mesh chunks of the cig entity (count<=0 or >=64 -> all chunks). Returns the number of mesh
// components touched. If the cig mesh's chunks are ordered along its length, lowering `count`
// shortens it; test live from the console to learn the ordering.
void SetVRSmokeCigChunks(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    RED4ext::Handle<RED4ext::IScriptable> ent;
    RED4ext::GetParameter(aFrame, &ent);
    int32_t count = 0;
    RED4ext::GetParameter(aFrame, &count);
    aFrame->code++;
    const uint64_t mask = (count <= 0 || count >= 64) ? 0xFFFFFFFFFFFFFFFFull : ((1ull << count) - 1ull);
    auto* entity = reinterpret_cast<RED4ext::ent::Entity*>(ent.instance);
    if (!entity) { if (aOut) *aOut = -1; return; }
    const RED4ext::CName skinnedName("entSkinnedMeshComponent");
    const RED4ext::CName garmentName("entGarmentSkinnedMeshComponent");
    const RED4ext::CName meshName("entMeshComponent");
    int32_t hit = 0;
    for (auto& componentHandle : entity->components) {
        auto* component = componentHandle.instance;
        if (!component) continue;
        RED4ext::CClass* type = component->GetType();
        if (!type) continue;
        if (ClassIsA(type, skinnedName) || ClassIsA(type, garmentName)) {
            reinterpret_cast<RED4ext::ent::SkinnedMeshComponent*>(component)->chunkMask = mask; ++hit;
        } else if (ClassIsA(type, meshName)) {
            reinterpret_cast<RED4ext::ent::MeshComponent*>(component)->chunkMask = mask; ++hit;
        }
    }
    if (aOut) *aOut = hit;
}

// SetVRSmokeCigVisualScale(cig, y): burn-down for a STATIC-mesh cig. Sets visualScale.Y (length axis)
// on every entMeshComponent of the cig entity; keeps X/Z. Returns the number of mesh components hit
// (0 = the cig is still a skinned mesh / override archive not loaded). If the cig shrinks along the
// wrong axis in-game, switch .Y to .X or .Z here.
void SetVRSmokeCigVisualScale(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    RED4ext::Handle<RED4ext::IScriptable> ent;
    RED4ext::GetParameter(aFrame, &ent);
    float y = 1.0f;
    RED4ext::GetParameter(aFrame, &y);
    aFrame->code++;
    if (y < 0.02f) y = 0.02f;
    auto* entity = reinterpret_cast<RED4ext::ent::Entity*>(ent.instance);
    if (!entity) { if (aOut) *aOut = -1; return; }
    const RED4ext::CName meshName("entMeshComponent");
    int32_t hit = 0;
    for (auto& componentHandle : entity->components) {
        auto* component = componentHandle.instance;
        if (!component) continue;
        RED4ext::CClass* type = component->GetType();
        if (!type) continue;
        if (ClassIsA(type, meshName)) {
            reinterpret_cast<RED4ext::ent::MeshComponent*>(component)->visualScale.Y = y;
            ++hit;
        }
    }
    if (aOut) *aOut = hit;
}

// GetVRSmokeCigVisualScaleY(cig): current visualScale.Y of the cig's first entMeshComponent (the .ent
// default before we shrink it), or -1 if none. reds reads this once to learn the authored full length.
void GetVRSmokeCigVisualScaleY(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, float* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    RED4ext::Handle<RED4ext::IScriptable> ent;
    RED4ext::GetParameter(aFrame, &ent);
    aFrame->code++;
    float result = -1.0f;
    auto* entity = reinterpret_cast<RED4ext::ent::Entity*>(ent.instance);
    if (entity) {
        const RED4ext::CName meshName("entMeshComponent");
        for (auto& componentHandle : entity->components) {
            auto* component = componentHandle.instance;
            if (!component) continue;
            RED4ext::CClass* type = component->GetType();
            if (!type) continue;
            if (ClassIsA(type, meshName)) {
                result = reinterpret_cast<RED4ext::ent::MeshComponent*>(component)->visualScale.Y;
                break;
            }
        }
    }
    if (aOut) *aOut = result;
}

// SetVRSmokeMouthAnchor(on): 1 = pin the cig to the mouth (hands-free), 0 = back to the hand grip.
void SetVRSmokeMouthAnchor(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t on = 0;
    RED4ext::GetParameter(aFrame, &on);
    aFrame->code++;
    g_VRSmokeMouthAnchor = on ? 1 : 0;
    if (!g_VRSmokeMouthAnchor) { g_VRSmokeAnchorValid = 0; g_VRSmokeAltAnchorValid = 0; } // revert next pass
    if (aOut) *aOut = g_VRSmokeMouthAnchor;
}

// SetVRSmokeAnchorBone(sel): choose which bone the mouth-pin drives, so a prop on a NON-weapon slot
// can ride the lips with BOTH hands + weapon slots free. 0=off (WeaponRight path), 1=Neck1, 2=Head,
// 3=Neck. The bone is pinned to the HMD mouth via its parent's model FK (see vrik_hook.h).
void SetVRSmokeAnchorBone(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t sel = 0;
    RED4ext::GetParameter(aFrame, &sel);
    aFrame->code++;
    g_VRSmokeAnchorBoneSel = sel;
    if (sel == 0) { g_VRSmokeAnchorBoneIdx = -1; g_VRSmokeAltAnchorValid = 0; }
    if (aOut) *aOut = sel;
}

// SetVRSmokeMouthOffset(x,y,z,pitch,yaw,roll): live tune of the mouth-anchored cig. x,y,z = model-space
// position offset from the head bone (metres, +Y forward, +Z up); pitch/yaw/roll = the cig's model-space
// orientation at the lips (degrees). Adjust from the CET console until the cig sits right in VR.
void SetVRSmokeMouthOffset(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    float x=0.0f, y=0.0f, z=0.0f, pitch=0.0f, yaw=0.0f, roll=0.0f;
    RED4ext::GetParameter(aFrame, &x);
    RED4ext::GetParameter(aFrame, &y);
    RED4ext::GetParameter(aFrame, &z);
    RED4ext::GetParameter(aFrame, &pitch);
    RED4ext::GetParameter(aFrame, &yaw);
    RED4ext::GetParameter(aFrame, &roll);
    aFrame->code++;
    g_VRSmokeMouthPos[0]=x; g_VRSmokeMouthPos[1]=y; g_VRSmokeMouthPos[2]=z;
    const float d2r = 0.01745329252f * 0.5f;
    float cp = std::cos(pitch*d2r), sp = std::sin(pitch*d2r);
    float cy = std::cos(yaw*d2r),   sy = std::sin(yaw*d2r);
    float cr = std::cos(roll*d2r),  sr = std::sin(roll*d2r);
    g_VRSmokeMouthRot[0] = sp*cy*cr + cp*sy*sr;
    g_VRSmokeMouthRot[1] = cp*sy*cr - sp*cy*sr;
    g_VRSmokeMouthRot[2] = cp*cy*sr + sp*sy*cr;
    g_VRSmokeMouthRot[3] = cp*cy*cr - sp*sy*sr;
    if (aOut) *aOut = 1;
}

// ---- LEFT-HAND mirror natives (lighter grip) ----
void SetVRSmokeFingersL(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t active = 0;
    RED4ext::GetParameter(aFrame, &active);
    aFrame->code++;
    g_VRSmokeFingerActiveL = active ? 1 : 0;
    if (aOut) *aOut = g_VRSmokeFingerActiveL;
}

// SetVRSmokeLeftCig(on): 1 = the LEFT hand holds the CIGARETTE (apply the cig-left pose from
// CyberpunkVR_SmokeGrip_Left.ini); 0 = the LEFT hand holds the lighter (apply the lighter pose).
// reds sets this when the cig enters / leaves the left hand. Also routes the left-hand capture:
// with on=1, VRSmokeCaptureFingersL + VRSmokeDumpFingers record the cig-left pose.
void SetVRSmokeLeftCig(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t on = 0;
    RED4ext::GetParameter(aFrame, &on);
    aFrame->code++;
    g_VRSmokeLeftUseCig = on ? 1 : 0;
    if (aOut) *aOut = g_VRSmokeLeftUseCig;
}

// SetVRSmokeRightLighter(on): 1 = the RIGHT hand holds the lighter (left-handed smoking): the right-hand apply
// gives it the lighter pose mirrored from the left hand's; 0 = the right hand holds the cigarette.
void SetVRSmokeRightLighter(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t on = 0;
    RED4ext::GetParameter(aFrame, &on);
    aFrame->code++;
    g_VRSmokeRightLighter = on ? 1 : 0;
    if (aOut) *aOut = g_VRSmokeRightLighter;
}

void VRSmokeCaptureFingersL(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    g_VRSmokeFingerCaptureL = 1;
    if (aOut) *aOut = g_VRSmokeFingerCountL;
}

void SetVRSmokeLighter(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t en = 1;
    RED4ext::GetParameter(aFrame, &en);
    aFrame->code++;
    g_VRSmokeLighterEnable = en ? 1 : 0;
    if (aOut) *aOut = g_VRSmokeLighterEnable;
}

void SetVRSmokeLighterOffset(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    float x=0.0f, y=0.0f, z=0.0f, pitch=0.0f, yaw=0.0f, roll=0.0f;
    RED4ext::GetParameter(aFrame, &x);
    RED4ext::GetParameter(aFrame, &y);
    RED4ext::GetParameter(aFrame, &z);
    RED4ext::GetParameter(aFrame, &pitch);
    RED4ext::GetParameter(aFrame, &yaw);
    RED4ext::GetParameter(aFrame, &roll);
    aFrame->code++;
    g_VRSmokeLighterOffP[0]=x; g_VRSmokeLighterOffP[1]=y; g_VRSmokeLighterOffP[2]=z;
    const float d2r = 0.01745329252f * 0.5f;
    float cp = std::cos(pitch*d2r), sp = std::sin(pitch*d2r);
    float cy = std::cos(yaw*d2r),   sy = std::sin(yaw*d2r);
    float cr = std::cos(roll*d2r),  sr = std::sin(roll*d2r);
    g_VRSmokeLighterOffQ[0] = sp*cy*cr + cp*sy*sr;
    g_VRSmokeLighterOffQ[1] = cp*sy*cr - sp*cy*sr;
    g_VRSmokeLighterOffQ[2] = cp*cy*sr + sp*sy*cr;
    g_VRSmokeLighterOffQ[3] = cp*cy*cr - sp*sy*sr;
    if (aOut) *aOut = 1;
}

// SetVRSmokeThumbFlickL(pitch,yaw,roll): the FULL-press rotation delta for the left thumb
// (the "press the lighter wheel" motion at trigger = 1). Tune in VR, then bake via dump.
void SetVRSmokeThumbFlickL(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    float pitch=0.0f, yaw=0.0f, roll=0.0f;
    RED4ext::GetParameter(aFrame, &pitch);
    RED4ext::GetParameter(aFrame, &yaw);
    RED4ext::GetParameter(aFrame, &roll);
    aFrame->code++;
    const float d2r = 0.01745329252f * 0.5f;
    float cp = std::cos(pitch*d2r), sp = std::sin(pitch*d2r);
    float cy = std::cos(yaw*d2r),   sy = std::sin(yaw*d2r);
    float cr = std::cos(roll*d2r),  sr = std::sin(roll*d2r);
    g_VRSmokeThumbFlickL[0] = sp*cy*cr + cp*sy*sr;
    g_VRSmokeThumbFlickL[1] = cp*sy*cr - sp*cy*sr;
    g_VRSmokeThumbFlickL[2] = cp*cy*sr + sp*sy*cr;
    g_VRSmokeThumbFlickL[3] = cp*cy*cr - sp*sy*sr;
    if (aOut) *aOut = 1;
}

// SetVRSmokeThumbPressL(amount): manual press override (0..1) for tuning without the trigger.
// The live left trigger (shared[67]) still drives it; the larger of the two wins. Set 0 to
// hand control back to the trigger.
void SetVRSmokeThumbPressL(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    float amt = 0.0f;
    RED4ext::GetParameter(aFrame, &amt);
    aFrame->code++;
    if (amt < 0.0f) amt = 0.0f; else if (amt > 1.0f) amt = 1.0f;
    g_VRSmokeThumbPressManualL = amt;
    if (aOut) *aOut = 1;
}
