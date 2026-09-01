// PoseRig -- natives lifted out of src/Natives/Natives.cpp, which held every family at once.
//
// The pose and rig natives: which bone a script may write, in what space, and the
// recorded-pose replay the reload module drives the hands with.
//
// The cut was placed by the seam map and then SNAPPED to the nearest point at brace depth zero.
// Boundaries taken from line numbers alone are how a split lands in the middle of a function; the
// check is cheap and it is the same lesson as every other generator in this restructure.
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/GameEngine.hpp>
#include <sstream>
#include <locale>
#include <clocale>
#include "Utils/SharedSlots.hpp"   // CyberpunkVR_Hands_Shared slot map (single source of truth)
#include "Core/CoreInternal.hpp"   // PersistLiveControlsUiState: the scanner editor saves its layout
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




void SetVRBoneDebugIndex(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, void* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(aOut); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t index = -1;
    RED4ext::GetParameter(aFrame, &index);
    aFrame->code++;
    g_CalibrationBoneIndex = index;
}

// ---- Pose-apply hook control ----

// Installs the MinHook on the pose-apply function (module+0x17DDB4).
void InstallVRAnimPoseHook(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    // Only the pose-apply hook is installed; the ComponentFunc21 hook is dead code
    // that crushed FPS (see UpdateVRIKAnimInputs note).
    bool ok = InstallAnimPoseHook();
    if (aOut) *aOut = ok ? 1 : 0;
}

// ---- Weapon-aim native hook (M1 instrumentation) script API ----

// Installs the MinHooks on the projectile (+0x28D4B8) and TargetHelper (+0x46F774)
// shot functions. M1 = read-only instrumentation; returns 1 on success.
void InstallWeaponAimHook(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    bool ok = InstallWeaponAimHooks();
    if (aOut) *aOut = ok ? 1 : 0;
}

// Writes the current hook stats + last sampled vectors to weapon_aim_native.txt.
// Defined below, with the provider table it reads -- the constants and arrays live down there.
// DumpProviderSlots moved with the orientation-provider instrument to
// src/Natives/OrientationProvider.cpp; declared in Natives/NativeHelpers.hpp.

void DumpWeaponAimHookStats(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    std::ofstream out(VRDiagPath("weapon_aim_native.txt"), std::ios::out | std::ios::trunc);
    if (!out.is_open()) { if (aOut) *aOut = -1; return; }
    out << "installed=" << g_waInstalled << " enable=" << g_waEnable << " mode=" << g_waMode << "\n";
    out << "XFORM-GETTER calls=" << g_xfCalls << " mutated=" << g_xfMutated
        << " mode=" << g_xfMode << " testYaw=" << g_xfTestYaw << " shotInProg=" << g_shotInProgress << "  <== THE camera->shot lever\n";
    out << "  xf out-orient = " << g_xfLastOut[0] << " " << g_xfLastOut[1] << " " << g_xfLastOut[2] << " " << g_xfLastOut[3] << "\n";
    DumpProviderSlots(out);
    out << "SHOTSNAP calls=" << g_ssCalls << " snapped=" << g_ssSnapped
        << " camPtr=0x" << std::hex << g_ssCamPtr << std::dec
        << " enable=" << g_ssEnable << " mode=" << g_ssMode << " testYaw=" << g_ssTestYaw << "\n";
    out << "  bracket quat = " << g_ssCamQuat[0] << " " << g_ssCamQuat[1] << " " << g_ssCamQuat[2] << " " << g_ssCamQuat[3] << "\n";
    out << "  cam+0xD0(local) = " << g_ssDiagD0[0] << " " << g_ssDiagD0[1] << " " << g_ssDiagD0[2] << " " << g_ssDiagD0[3] << "\n";
    out << "  cam+0xF0(world) = " << g_ssDiagF0[0] << " " << g_ssDiagF0[1] << " " << g_ssDiagF0[2] << " " << g_ssDiagF0[3]
        << "  <== bracket target (default)\n";
    out << "  cam+0x110       = " << g_ssDiag110[0] << " " << g_ssDiag110[1] << " " << g_ssDiag110[2] << " " << g_ssDiag110[3] << "\n";
    out << "HEADING calls=" << g_waHeadCalls << " camObj=0x" << std::hex << g_waHeadObj << std::dec
        << " force=" << g_waHeadForce << "\n";
    out << "  set yaw/pitch=" << g_waHeadYaw << "/" << g_waHeadPitch
        << "  orig[+4E4]/[+4E8]=" << g_waHeadOrig4E4 << "/" << g_waHeadOrig4E8
        << "  [+4B8]=" << g_waHeadVal4B8 << " flag[+474]=" << g_waHeadFlag474 << "\n";
    out << "xhUpd calls=" << g_waXhCalls << " mutated=" << g_waXhMutated << "  <== crosshair-aim (UI)\n";
    out << "  cache+0x350 pos = " << g_waXhPos[0] << " " << g_waXhPos[1] << " " << g_waXhPos[2] << " " << g_waXhPos[3] << "\n";
    out << "  cache+0x370 dir = " << g_waXhDir[0] << " " << g_waXhDir[1] << " " << g_waXhDir[2] << " " << g_waXhDir[3]
        << "  (|xyz|=" << std::sqrt(g_waXhDir[0]*g_waXhDir[0]+g_waXhDir[1]*g_waXhDir[1]+g_waXhDir[2]*g_waXhDir[2]) << ")\n";
    out << "exeBase=0x" << std::hex << g_waExeBase << std::dec << "\n";
    out << "projCalls=" << g_waProjCalls << " projMutated=" << g_waProjMutated << "  <== projectile bullet lever\n";
    out << "  projCtrl=" << g_waProjCtrl << " always=" << g_waProjAlways << " fireInShot=" << g_fireInShot
        << " rejectReason=" << g_waProjRejectReason << " lastRetRva=0x" << std::hex << g_waProjLastRetRva << std::dec
        << " gateRva=0x" << std::hex << g_waProjGateRva << std::dec
        << " ctrlLen2=" << g_projDump[44] << " spd=" << g_projDump[45] << " targetLen2=" << g_projDump[46] << "\n";
    out << "  projRetCounts: 36F9FF(queue)=" << g_waProjRet36F9FF << " 36FD7C(update)=" << g_waProjRet36FD7C
        << " 4E5109(active)=" << g_waProjRet4E5109 << " 4E615F(alt)=" << g_waProjRet4E615F << "\n";
    out << "targetCalls=" << g_waTargetCalls << " fromShot=" << g_waTargetFromShot
        << " redirects=" << g_waRedirects << " lastRetRva=0x" << std::hex << g_waLastRetRva << std::dec
        << " testYaw=" << g_xfTestYaw << " plane=" << g_xfTestPlane << "\n";
    out << "classifyCalls=" << g_waClassifyCalls << " fromShot=" << g_waClassifyFromShot << "\n";
    out << "normPatched=" << g_waNormPatched << " normShot(@0x46F0E5)=" << g_waNormShot
        << " normMutated=" << g_waNormMutated << "  <== bullet-dir lever\n";
    out << "fireNormPatched=" << g_waFireNormPatched << " fireNormShot(@0x84C968)=" << g_waFireNormShot
        << " fireNormMutated=" << g_waFireNormMutated << "  <== weapon fire Normalize(target-muzzle)\n";
    out << "physPatched=" << g_waPhysPatched << " physCalls(@0x46F1EA)=" << g_waPhysCalls
        << " physMutated=" << g_waPhysMutated << "\n";
    out << "shotPipeline: CandA(0x291D9C8)=" << g_waCandA << " CandB(0x291DD54)=" << g_waCandB
        << " SVP(0x292263C)=" << g_waSVP << " SFVW(0x29216D0)=" << g_waSFVW << "\n";
    out << "physArgSnapshot snapped=" << g_waDbgSnapped << " (look for the unit dir vector ~= camera forward)\n";
    out << "-- arg3 (basis?) floats @+0x00..0x120 --\n";
    for (int i = 0; i < 72; i += 4)
        out << "   +0x" << std::hex << (i*4) << std::dec << ": "
            << g_waDbgArg3[i] << " " << g_waDbgArg3[i+1] << " " << g_waDbgArg3[i+2] << " " << g_waDbgArg3[i+3] << "\n";
    out << "-- rayList floats @+0x00..0xA0 --\n";
    for (int i = 0; i < 40; i += 4)
        out << "   +0x" << std::hex << (i*4) << std::dec << ": "
            << g_waDbgRay[i] << " " << g_waDbgRay[i+1] << " " << g_waDbgRay[i+2] << " " << g_waDbgRay[i+3] << "\n";
    out << "-- rayList[0] entry floats @+0x00..0x70 --\n";
    for (int i = 0; i < 28; i += 4)
        out << "   +0x" << std::hex << (i*4) << std::dec << ": "
            << g_waDbgRayEntry[i] << " " << g_waDbgRayEntry[i+1] << " " << g_waDbgRayEntry[i+2] << " " << g_waDbgRayEntry[i+3] << "\n";
    out << "publishedFwd  = " << g_waFwd[0] << " " << g_waFwd[1] << " " << g_waFwd[2] << " (seq " << g_waFwdSeq << ")\n";
    out << "publishedPos  = " << g_waPos[0] << " " << g_waPos[1] << " " << g_waPos[2] << "\n";
    out << "shotOrigin    = " << g_waTargetOrigin[0] << " " << g_waTargetOrigin[1] << " " << g_waTargetOrigin[2] << "\n";
    out << "origAimDelta  = " << g_waTargetDir[0] << " " << g_waTargetDir[1] << " " << g_waTargetDir[2] << "\n";
    out.close();
    if (aOut) *aOut = 1;
}

// Live stat getter for the overlay (no file). which: 0=targetCalls, 1=redirects,
// 2=projCalls, 3=installed, 4=lastFwdSeq.
void GetWeaponAimStat(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t which = 0;
    RED4ext::GetParameter(aFrame, &which);
    aFrame->code++;
    int32_t v = 0;
    switch (which) {
        case 0: v = static_cast<int32_t>(g_waTargetCalls); break;
        case 1: v = static_cast<int32_t>(g_waRedirects); break;
        case 2: v = static_cast<int32_t>(g_waProjCalls); break;
        case 3: v = g_waInstalled; break;
        case 4: v = static_cast<int32_t>(g_waFwdSeq); break;
        case 5: v = static_cast<int32_t>(g_waClassifyFromShot); break;
        case 6: v = static_cast<int32_t>(g_waClassifyCalls); break;
        case 7: v = static_cast<int32_t>(g_waTargetFromShot); break;
        case 8: v = static_cast<int32_t>(g_waPhysCalls); break;
        case 9: v = static_cast<int32_t>(g_waPhysMutated); break;
        case 10: v = g_waPhysPatched; break;
        case 11: v = static_cast<int32_t>(g_waCandA); break;
        case 12: v = static_cast<int32_t>(g_waCandB); break;
        case 13: v = static_cast<int32_t>(g_waSVP); break;
        case 14: v = static_cast<int32_t>(g_waSFVW); break;
        case 15: v = static_cast<int32_t>(g_waNormShot); break;
        case 16: v = static_cast<int32_t>(g_waNormMutated); break;
        case 17: v = g_waNormPatched; break;
        case 18: v = static_cast<int32_t>(g_waProjMutated); break;
        case 19: v = static_cast<int32_t>(g_waXhCalls); break;
        case 20: v = static_cast<int32_t>(g_waXhMutated); break;
        case 21: v = static_cast<int32_t>(g_waHeadCalls); break;
        case 22: v = (g_waHeadObj != 0) ? 1 : 0; break;
        case 23: v = g_waHeadForce; break;
        case 24: v = static_cast<int32_t>(g_ssCalls); break;
        case 25: v = static_cast<int32_t>(g_ssSnapped); break;
        case 26: v = (g_ssCamPtr != 0) ? 1 : 0; break;
        case 27: v = static_cast<int32_t>(g_xfCalls); break;
        case 28: v = static_cast<int32_t>(g_xfMutated); break;
        case 29: v = static_cast<int32_t>(g_goCalls); break;
        case 30: v = static_cast<int32_t>(g_goMutated); break;
        case 31: v = static_cast<int32_t>(g_waTgtOvr); break;
        case 32: v = g_waFireNormPatched; break;
        case 33: v = static_cast<int32_t>(g_waFireNormShot); break;
        case 34: v = static_cast<int32_t>(g_waFireNormMutated); break;
        case 35: v = static_cast<int32_t>(g_waProjRejectReason); break;
        case 36: v = static_cast<int32_t>(g_waProjLastRetRva); break;
        case 37: v = g_waProjCtrl; break;
        case 38: v = g_waProjAlways; break;
        case 39: v = static_cast<int32_t>(g_waProjRet36F9FF); break;
        case 40: v = static_cast<int32_t>(g_waProjRet36FD7C); break;
        case 41: v = static_cast<int32_t>(g_waProjRet4E5109); break;
        case 42: v = static_cast<int32_t>(g_waProjRet4E615F); break;
        case 43: v = static_cast<int32_t>(g_waProjGateRva); break;
        default: v = -1; break;
    }
    if (aOut) *aOut = v;
}

// Writes the live FPP-camera address + key field addresses to cam_addr.txt, formatted for
// an external memory-inspection tool ("find what writes/accesses to this address"). No guessing -- it finds the
// exact instruction that writes the aim/orientation, on the live game, no restarts.
void DumpVRCamAddr(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    std::ofstream out(VRDiagPath("cam_addr.txt"), std::ios::out | std::ios::trunc);
    if (!out.is_open()) { if (aOut) *aOut = -1; return; }
    const uintptr_t cam = g_ssCamPtr;
    out << "=== Camera watch targets (live FPP camera) ===\n";
    out << "camPtr            = " << std::hex << cam << "\n";
    out << "cam+0xF0 (WORLD orientation quat, what render+shot use):  " << (cam + 0xF0) << "\n";
    out << "cam+0xD0 (local orientation, usually identity):           " << (cam + 0xD0) << "\n";
    out << "cam+0x110 (world position):                               " << (cam + 0x110) << std::dec << "\n";
    out << "\nIn your memory-inspection tool: attach to Cyberpunk2077.exe -> Memory View -> Ctrl+G -> paste\n";
    out << "the cam+0xF0 address -> right-click the byte -> 'Find out what ACCESSES this address'\n";
    out << "-> shoot at the wall + turn head -> the instruction list = the aim readers/writers.\n";
    out << "Send me that instruction list (the 'Cyberpunk2077.exe+XXXXXX' addresses).\n";
    out.close();
    if (aOut) *aOut = 1;
}

// HW-breakpoint trace control: start watching the shot camera field (camPtr+offset) for
// reads, fire a shot, stop, dump the accessor RVAs. offsetSel: 0=+0x110(origin) 1=+0xF0(orient).
void StartVRCamTrace(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t offsetSel = 0, gated = 0, writeOnly = 0;
    RED4ext::GetParameter(aFrame, &offsetSel);
    RED4ext::GetParameter(aFrame, &gated);
    RED4ext::GetParameter(aFrame, &writeOnly);
    aFrame->code++;
    uintptr_t watch = 0;
    if (offsetSel == 2) {
        // LOCATED camera (dxgi HMD-injection / render cam) quat, from shared mem [51]/[52]+16.
        if (g_pSharedHands) {
            uint32_t lo=0, hi=0;
            std::memcpy(&lo, &g_pSharedHands[51], 4);
            std::memcpy(&hi, &g_pSharedHands[52], 4);
            uintptr_t cam = (static_cast<uintptr_t>(hi) << 32) | static_cast<uintptr_t>(lo);
            if (cam) watch = cam + 16;
        }
    } else {
        const uintptr_t off = (offsetSel == 1) ? 0xF0 : 0x110;
        if (g_ssCamPtr) watch = g_ssCamPtr + off;
    }
    if (watch) { Wa_StartTrace(watch, gated, writeOnly); if (aOut) *aOut = 1; }
    else if (aOut) *aOut = 0;
}
void StopVRCamTrace(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    Wa_StopTrace();
    if (aOut) *aOut = g_traceCount;
}
void DumpVRCamTrace(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    std::ofstream out(VRDiagPath("cam_trace.txt"), std::ios::out | std::ios::trunc);
    if (!out.is_open()) { if (aOut) *aOut = -1; return; }
    out << "watched=0x" << std::hex << g_traceAddr << std::dec
        << " hits=" << g_traceHits << " uniqueRVAs=" << g_traceCount << " active=" << g_traceActive << "\n";
    out << "accessor RVAs + hit COUNT (low=per-shot reader, high=per-frame render):\n";
    for (int i = 0; i < g_traceCount && i < 128; ++i)
        out << "  0x" << std::hex << (0x140000000ull + g_traceRvas[i]) << std::dec
            << "  count=" << g_traceRvaCounts[i] << "\n";
    out.close();
    if (aOut) *aOut = g_traceCount;
}

// CET publishes the FPP camera object pointer each frame so the ShotSnap hook can
// bracket cam+0xD0. Mirrors SetVRRightHandEntity (handle -> instance pointer).
// cam+0xD0 additive head-inject control.
volatile int g_headLocalEnable = 0;
volatile int g_headLocalConv = 0;

// ADDITIVE HEAD INJECT: write the head-relative quaternion (hmdRel,
// shared slots 16..19) into the FPP cam LOCAL orientation @ cam+0xD0. The game composes
// world = heading (X) local(cam+0xD0), so the VIEW gets the head while the HEADING (=stick
// aim) is untouched -> the bullet follows the stick, not the head. dxgi must be in SKIP-HMD
// ALWAYS mode (HMD now flows via cam+0xD0). conv selects the VR->game-local axis mapping.
// Separate __try function (the script-callback can't use __try -- it has C++ unwinding objects).
static void WriteHeadLocal() {
    if (!(g_headLocalEnable && g_ssCamPtr && g_pSharedHands)) return;
    RefreshHandsSnapshot();
    const float hi = SharedPose(16), hj = SharedPose(17), hk = SharedPose(18), hr = SharedPose(19);
    const float l = hi*hi + hj*hj + hk*hk + hr*hr;
    if (l <= 0.25f) return;
    float qi, qj, qk, qr;
    switch (g_headLocalConv) {
        case 1: qi =  hi; qj = -hk; qk =  hj; qr = hr; break; // VRIK map (i,-k,j,r)
        case 2: qi = -hi; qj = -hj; qk = -hk; qr = hr; break; // inverse
        case 3: qi =  hi; qj =  hk; qk = -hj; qr = hr; break;
        case 4: qi = -hi; qj =  hk; qk =  hj; qr = hr; break;
        case 5: qi =  hk; qj =  hj; qk = -hi; qr = hr; break;
        default: qi = hi; qj = hj; qk = hk; qr = hr; break; // identity
    }
    __try {
        float* q = reinterpret_cast<float*>(g_ssCamPtr + 0xD0);
        q[0] = qi; q[1] = qj; q[2] = qk; q[3] = qr;
    } __except (EXCEPTION_EXECUTE_HANDLER) {}
}

void SetVRShotCamera(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, void* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(aOut); RED4EXT_UNUSED_PARAMETER(a4);
    RED4ext::Handle<RED4ext::IScriptable> cam;
    RED4ext::GetParameter(aFrame, &cam);
    aFrame->code++;
    g_ssCamPtr = reinterpret_cast<uintptr_t>(cam.instance); // FPP camera component instance
    WriteHeadLocal();
}

void SetVRHeadLocal(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t enable = 0, conv = 0;
    RED4ext::GetParameter(aFrame, &enable);
    RED4ext::GetParameter(aFrame, &conv);
    aFrame->code++;
    g_headLocalEnable = enable; g_headLocalConv = conv;
    if (aOut) *aOut = conv;
}

// SKIP-HMD test: tells dxgi (via shared mem [58]) to skip the HMD camera overwrite.
// mode 0=off, 1=ALWAYS (no head -- view follows game aim), 2=shot-frame only (decouple test).
void SetVRSkipHmdTest(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t mode = 0;
    RED4ext::GetParameter(aFrame, &mode);
    aFrame->code++;
    EnsureSharedMemory();
    if (g_pSharedHands) reinterpret_cast<volatile uint32_t*>(g_pSharedHands)[58] = static_cast<uint32_t>(mode);
    if (aOut) *aOut = mode;
}

// MENU OPEN bridge: redscript sets this when a full-screen menu (e.g. the world
// map) opens/closes. dxgi reads shared[81] in OnLocateCameraCallback and
// ApplySettings/DLSSResolutionOverride: while the flag is set, dxgi stops
// driving the game camera with the HMD orientation (menu/map does NOT swim with
// head rotation) and suspends the square-
// resolution force (fixes map pin drift on pan/zoom).
// SLOT CHOICE — slot [81] is dedicated and unused elsewhere. Do NOT use [63]
// (overwritten every frame by the hand delta-quaternion in OnLocateCameraCallback)
// or [70..75] (shoulder-calibration slots read by PollVRCalibFromShared as
// g_VRShoulderRX/RY/RZ — writing the map flag there zeroes the VRIK shoulder
// pose and causes body jitter whenever the map opens/closes).
static constexpr int kWorldMapMenuOpenSharedSlot = 81;
void SetVRMenuOpen(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t open = 0;
    RED4ext::GetParameter(aFrame, &open);
    aFrame->code++;
    EnsureSharedMemory();
    if (g_pSharedHands) {
        reinterpret_cast<volatile uint32_t*>(g_pSharedHands)[kWorldMapMenuOpenSharedSlot] = static_cast<uint32_t>(open ? 1 : 0);
    }
    if (aOut) *aOut = open;
}

// THE SCANNER'S HUD LAYOUT, both ways. The redscript editor reads a piece's place to apply it and
// writes it back as the player drags; the plugin owns the number because a script has nowhere to put
// one. No shared slot is involved: nothing here crosses into Lua, and a native reads the global
// directly, so publishing into the map first would only add a second place for it to go stale.
extern "C" __declspec(dllexport) extern float CyberpunkVR_ScannerSlots[12];

// comp: 0 = x, 1 = y, 2 = scale. An out-of-range read answers 1.0 for the SCALE and 0 otherwise, so a
// script that asks the wrong question moves nothing rather than collapsing a widget to a point.
void VRScannerSlotGet(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, float* aOut, int64_t) {
    int32_t idx = 0, comp = 0;
    RED4ext::GetParameter(aFrame, &idx);
    RED4ext::GetParameter(aFrame, &comp);
    aFrame->code++;
    float v = (comp == 2) ? 1.0f : 0.0f;
    if (idx >= 0 && idx < 7 && comp >= 0 && comp < 3) v = CyberpunkVR_ScannerSlots[idx * 3 + comp];
    if (aOut) *aOut = v;
}

// Clamped HERE as well as in the ini read, and not out of tidiness: this is the path a live drag takes,
// so it is the one that decides whether a flick of the mouse can throw a panel off the screen.
void VRScannerSlotSet(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t) {
    int32_t idx = 0;
    float x = 0.0f, y = 0.0f, s = 1.0f;
    RED4ext::GetParameter(aFrame, &idx);
    RED4ext::GetParameter(aFrame, &x);
    RED4ext::GetParameter(aFrame, &y);
    RED4ext::GetParameter(aFrame, &s);
    aFrame->code++;
    if (idx >= 0 && idx < 7) {
        CyberpunkVR_ScannerSlots[idx * 3 + 0] = (x < -1920.0f) ? -1920.0f : ((x > 1920.0f) ? 1920.0f : x);
        CyberpunkVR_ScannerSlots[idx * 3 + 1] = (y < -1080.0f) ? -1080.0f : ((y > 1080.0f) ? 1080.0f : y);
        CyberpunkVR_ScannerSlots[idx * 3 + 2] = (s < 0.10f) ? 0.10f : ((s > 5.0f) ? 5.0f : s);
    }
    if (aOut) *aOut = idx;
}

// ONE WRITE, WHEN THE EDITOR CLOSES. Not per drag frame: the mouse delta arrives every frame it moves,
// and rewriting the whole ini at that rate would be hundreds of file writes for one gesture. The poll
// only re-reads the ini when its write time changes, so the drag itself needs no file at all.
void VRScannerSlotSave(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t) {
    aFrame->code++;
    PersistLiveControlsUiState(MakeLiveControlsUiState());
    if (aOut) *aOut = 1;
}

// A DEVICE SCREEN IS UP -- and it is a different fact from "a menu is open", which is why it gets
// its own slot rather than reusing the one above. The world-map flag stops the HMD driving the game
// camera; a computer screen must NOT do that, because the head still has to look around it. All this
// one changes is that the XInput merge stops consuming the right stick's Y, so the game's own
// UI_MoveY_Axis -- which is bound to IK_Pad_RightAxisY and nothing else -- can scroll the list.
void SetVRDeviceScreen(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t open = 0;
    RED4ext::GetParameter(aFrame, &open);
    aFrame->code++;
    EnsureSharedMemory();
    if (g_pSharedHands) {
        reinterpret_cast<volatile uint32_t*>(g_pSharedHands)[vrshared::kDeviceScreenOpen] =
            static_cast<uint32_t>(open ? 1 : 0);
    }
    if (aOut) *aOut = open;
}

// GetWorldOrientation (0x802390) override -- the Cheat-Engine-confirmed shot aim reader.
// mode: 0=off, 1=ALWAYS (test view+bullet), 2=gated-by-shot (decouple). plane for testYaw.
void SetVRGetOrient(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t mode = 0, plane = 0; float testYaw = 0.0f;
    RED4ext::GetParameter(aFrame, &mode);
    RED4ext::GetParameter(aFrame, &testYaw);
    RED4ext::GetParameter(aFrame, &plane);
    aFrame->code++;
    g_goMode = mode; g_goTestYaw = testYaw; g_goPlane = plane;
    if (aOut) *aOut = static_cast<int32_t>(g_goMutated);
}

// Camera-transform getter override control. mode: 0=off, 1=ALWAYS (test), 2=gated-by-shot.
void SetVRXformOverride(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t mode = 0; float testYaw = 0.0f; int32_t plane = 0;
    RED4ext::GetParameter(aFrame, &mode);
    RED4ext::GetParameter(aFrame, &testYaw);
    RED4ext::GetParameter(aFrame, &plane);
    aFrame->code++;
    g_xfMode = mode; g_xfTestYaw = testYaw; g_xfTestPlane = plane;
    if (aOut) *aOut = static_cast<int32_t>(g_waTargetFromShot);
}

// FIRE-SHOT lever. mode: 0=scan-only, 1=bend-test (rotate the field at the
// override offset by `angle` rad about `plane`), 2=controller override (write dxgi controller
// forward [shared 60..62]; neg flips sign). Returns the fire-call count.
void SetVRFireMode(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t mode = 0, plane = 0, neg = 0; float angle = 0.0f;
    RED4ext::GetParameter(aFrame, &mode);
    RED4ext::GetParameter(aFrame, &plane);
    RED4ext::GetParameter(aFrame, &angle);
    RED4ext::GetParameter(aFrame, &neg);
    aFrame->code++;
    g_fireMode = mode; g_firePlane = plane; g_fireTestAng = angle; g_fireNeg = neg;
    if (aOut) *aOut = static_cast<int32_t>(g_fireCalls);
}

// Configure the auto-scanner: src 0=r8(shot-ctx) 1=rdx(arg2) 2=*(rdx+0x10)(transform) 3=*(rdx),
// range = bytes to scan. Returns the scan source for confirmation.
void SetVRFireScan(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t src = 0, range = 0x2300;
    RED4ext::GetParameter(aFrame, &src);
    RED4ext::GetParameter(aFrame, &range);
    aFrame->code++;
    g_fireScanSrc = src; if (range > 0) g_fireScanRange = range;
    if (aOut) *aOut = src;
}

// Configure the override target (where mode 1/2 writes): src enum (same as scan), byte offset.
void SetVRFireOverrideTarget(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t src = 0, off = 0;
    RED4ext::GetParameter(aFrame, &src);
    RED4ext::GetParameter(aFrame, &off);
    aFrame->code++;
    g_fireOvrSrc = src; g_fireOvrOff = off;
    if (aOut) *aOut = off;
}

// Read back fire-hook state. idx: 0..3 override-target field (pre-write), 4..7 what we wrote,
// 8 calls, 9 mutated, 10 hitCount, 11 scanSrc.
void GetVRFireDump(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, float* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t idx = 0;
    RED4ext::GetParameter(aFrame, &idx);
    aFrame->code++;
    float v = 0.0f;
    if (idx >= 0 && idx <= 3) v = g_fireDir[idx];
    else if (idx >= 4 && idx <= 7) v = g_fireDirOut[idx - 4];
    else if (idx == 8) v = static_cast<float>(g_fireCalls);
    else if (idx == 9) v = static_cast<float>(g_fireMutated);
    else if (idx == 10) v = static_cast<float>(g_fireHitCount);
    else if (idx == 11) v = static_cast<float>(g_fireScanSrc);
    if (aOut) *aOut = v;
}

// Read an auto-scan hit. hit = 0..g_fireHitCount-1; field: 0=byteOffset, 1=x, 2=y, 3=z, 4=dotCtrl.
void GetVRFireHit(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, float* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t hit = 0, field = 0;
    RED4ext::GetParameter(aFrame, &hit);
    RED4ext::GetParameter(aFrame, &field);
    aFrame->code++;
    float v = 0.0f;
    if (hit >= 0 && hit < g_fireHitCount && hit < 24) {
        if (field == 0) v = static_cast<float>(g_fireHitOff[hit]);
        else if (field >= 1 && field <= 3) v = g_fireHitVec[hit*3 + (field-1)];
        else if (field == 4) v = g_fireHitDot[hit];
    }
    if (aOut) *aOut = v;
}

// TargetHelper clean controller-redirect: target = origin + controllerFwd*100. on, neg(flip).
// Returns the override count (so the UI confirms it applied during the shot).
void SetVRTargetCtrl(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t on = 0, neg = 0;
    RED4ext::GetParameter(aFrame, &on);
    RED4ext::GetParameter(aFrame, &neg);
    aFrame->code++;
    g_waTgtCtrl = on; g_waTgtNeg = neg;
    if (aOut) *aOut = static_cast<int32_t>(g_waTgtOvr);
}

// ★ TRANSFORM-orientation override: write the controller aim quat into the shooter transform
// (*(rdx+0x10)) that the bullet raycast uses. mode: 0 off, 1 +0xF0(world), 2 +0xD0(local), 3 both.
// off = the world-orient quat offset (default 0xF0) for scrubbing. Returns g_fireMutated.
void SetVRFireXform(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t mode = 0, off = 0xF0;
    RED4ext::GetParameter(aFrame, &mode);
    RED4ext::GetParameter(aFrame, &off);
    aFrame->code++;
    g_fireXform = mode; if (off >= 0 && off <= 0x400) g_fireXformOff = off;
    if (aOut) *aOut = static_cast<int32_t>(g_fireMutated);
}

// ★ CAM-SNAP: during the shot, force the FPP camera orientation to the controller so the projectile
// launch's orientation provider (which reads the camera) launches the bullet down the controller.
// mode on/off; off = cam quat offset (0xF0 world / 0xD0 local).
void SetVRFireCamSnap(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t on = 0, off = 0xF0;
    RED4ext::GetParameter(aFrame, &on);
    RED4ext::GetParameter(aFrame, &off);
    aFrame->code++;
    g_fireCamSnap = on; if (off >= 0 && off <= 0x400) g_fireCamSnapOff = off;
    if (aOut) *aOut = static_cast<int32_t>(g_fireMutated);
}

// ★ Projectile ShootEvent startVelocity -> controller forward (the player bullet is a projectile).
// on, neg(flip). Returns g_waProjMutated count.
void SetVRProjCtrl(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t on = 0, neg = 0, unguide = 1, always = 0;
    RED4ext::GetParameter(aFrame, &on);
    RED4ext::GetParameter(aFrame, &neg);
    RED4ext::GetParameter(aFrame, &unguide);
    RED4ext::GetParameter(aFrame, &always);
    aFrame->code++;
    g_waProjCtrl = on; g_waProjNeg = neg; g_waProjUnguide = unguide; g_waProjAlways = always;
    if (aOut) *aOut = static_cast<int32_t>(g_waProjMutated);
}

// Read projectile-event diagnostics. idx 0-2 startPoint, 3-5 startVelocity(pre), 6-8 targetPos(pre),
// 9 guided flag, 10-12 controller dir written.
void GetVRProjDump(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, float* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t idx = 0;
    RED4ext::GetParameter(aFrame, &idx);
    aFrame->code++;
    float v = (idx >= 0 && idx < 64) ? g_projDump[idx] : 0.0f;
    if (aOut) *aOut = v;
}

// Select which localToWorld row (0..3) is used as the world muzzle origin for targetPosition.
void SetVRProjOriginRow(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t row = 3;
    RED4ext::GetParameter(aFrame, &row);
    aFrame->code++;
    if (row >= 0 && row <= 3) g_waProjOriginRow = row;
    if (aOut) *aOut = g_waProjOriginRow;
}

// Restrict projectile copy mutation to one return RVA. 0 = all callers; default is 0x4E5109.
// This avoids mutating queue/template copies.
void SetVRProjGateRva(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t rva = 0;
    RED4ext::GetParameter(aFrame, &rva);
    aFrame->code++;
    g_waProjGateRva = static_cast<uint32_t>(rva);
    if (aOut) *aOut = static_cast<int32_t>(g_waProjGateRva);
}

// Pump the player/camera WORLD position (from CET player:GetWorldPosition()) -> the projectile
// targetPosition origin (same world frame as the event's targetPosition).
void SetVRShotOrigin(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    float x = 0, y = 0, z = 0;
    RED4ext::GetParameter(aFrame, &x);
    RED4ext::GetParameter(aFrame, &y);
    RED4ext::GetParameter(aFrame, &z);
    aFrame->code++;
    g_shotOrigin[0] = x; g_shotOrigin[1] = y; g_shotOrigin[2] = z;
    if (aOut) *aOut = 1;
}

// === TRACE-DISPATCHER funnel instrumentation ===
// Control override: on=1 rewrites the ray END point to the controller forward during the shot;
// asFloat = ray origin/end format (1 float Vec3, 0 fixed-point); neg flips; gateRet (hex RVA, 0
// = all shot traces) restricts override to one caller once identified.
void SetVRTraceOverride(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t on = 0, writeOff = 0x18, force = 0, neg = 0, gateRet = 0;
    RED4ext::GetParameter(aFrame, &on);
    RED4ext::GetParameter(aFrame, &writeOff);  // byte offset in ray struct to write the unit dir
    RED4ext::GetParameter(aFrame, &force);     // 1 = write even if current isn't a unit vector
    RED4ext::GetParameter(aFrame, &neg);
    RED4ext::GetParameter(aFrame, &gateRet);
    aFrame->code++;
    g_trOverride = on; g_trWriteOff = writeOff; g_trForce = force; g_trNeg = neg; g_trGateRet = static_cast<uint32_t>(gateRet);
    if (aOut) *aOut = static_cast<int32_t>(g_trOvrCount);
}

// Reset the captured return-RVA ring so a fresh shot's callers can be observed.
void ResetVRTrace(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    g_trRetCount = 0; g_trShotCalls = 0; g_trOvrCount = 0;
    for (int i = 0; i < 12; ++i) { g_trRetRing[i] = 0; }
    if (aOut) *aOut = 0;
}

// Read trace summary. idx: 0=retCount, 1=shotCalls, 2=ovrCount; 10..25 = retRing[idx-10] (RVA).
void GetVRTrace(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, float* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t idx = 0;
    RED4ext::GetParameter(aFrame, &idx);
    aFrame->code++;
    float v = 0.0f;
    if (idx == 0) v = static_cast<float>(g_trRetCount);
    else if (idx == 1) v = static_cast<float>(g_trShotCalls);
    else if (idx == 2) v = static_cast<float>(g_trOvrCount);
    else if (idx >= 10 && idx < 26) v = static_cast<float>(g_trRetRing[idx - 10]);
    if (aOut) *aOut = v;
}

// Read a captured caller's ray. caller = 0..g_trRetCount-1. field: 0=retRVA, 1=hits,
// 10..21 = ray dword[field-10] as INT value, 30..41 = ray dword[field-30] as FLOAT.
// The bullet caller's ray has a real origin (muzzle/camera world pos) at dwords [2..4] (+0x08).
void GetVRTraceCaller(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, float* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t caller = 0, field = 0;
    RED4ext::GetParameter(aFrame, &caller);
    RED4ext::GetParameter(aFrame, &field);
    aFrame->code++;
    float v = 0.0f;
    if (caller >= 0 && caller < g_trRetCount && caller < 16) {
        if (field == 0) v = static_cast<float>(g_trRetRing[caller]);
        else if (field == 1) v = static_cast<float>(g_trCallerHits[caller]);
        else if (field >= 10 && field < 22) v = static_cast<float>(static_cast<int32_t>(g_trCallerRay[caller*12 + (field-10)]));
        else if (field >= 30 && field < 42) { uint32_t r = g_trCallerRay[caller*12 + (field-30)]; float f; memcpy(&f, &r, 4); v = f; }
        else if (field >= 50 && field < 54) v = g_trCallerDir[caller*4 + (field-50)]; // arg3 direction xyz w
    }
    if (aOut) *aOut = v;
}

// ShotSnap control: enable + compose mode + static-yaw sanity test (radians).
void SetVRShotSnap(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t enable = 0, mode = 0; float testYaw = 0.0f;
    RED4ext::GetParameter(aFrame, &enable);
    RED4ext::GetParameter(aFrame, &mode);
    RED4ext::GetParameter(aFrame, &testYaw);
    aFrame->code++;
    g_ssEnable = enable; g_ssMode = mode; g_ssTestYaw = testYaw;
    if (aOut) *aOut = static_cast<int32_t>(g_ssCalls);
}

// Heading-decouple test control from CET: force flag + static yaw/pitch offset.
void SetVRHeadingTest(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t force = 0; float yaw = 0.0f, pitch = 0.0f;
    RED4ext::GetParameter(aFrame, &force);
    RED4ext::GetParameter(aFrame, &yaw);
    RED4ext::GetParameter(aFrame, &pitch);
    aFrame->code++;
    g_waHeadForce = force; g_waHeadYaw = yaw; g_waHeadPitch = pitch;
    if (aOut) *aOut = static_cast<int32_t>(g_waHeadCalls);
}

// CET pushes the live weapon aim each frame: forward (unit, world), muzzle world pos,
// enable + mode + gate distance. mode bit0 = override the shot origin with the muzzle pos.
void SetVRWeaponAim(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    float fx = 0, fy = 0, fz = 0, px = 0, py = 0, pz = 0, gate = 5.0f;
    int32_t enable = 0, mode = 0;
    RED4ext::GetParameter(aFrame, &fx); RED4ext::GetParameter(aFrame, &fy); RED4ext::GetParameter(aFrame, &fz);
    RED4ext::GetParameter(aFrame, &px); RED4ext::GetParameter(aFrame, &py); RED4ext::GetParameter(aFrame, &pz);
    RED4ext::GetParameter(aFrame, &enable); RED4ext::GetParameter(aFrame, &mode); RED4ext::GetParameter(aFrame, &gate);
    aFrame->code++;
    g_waFwd[0] = fx; g_waFwd[1] = fy; g_waFwd[2] = fz;
    g_waPos[0] = px; g_waPos[1] = py; g_waPos[2] = pz;
    g_waEnable = enable; g_waMode = mode; g_waGateDist = gate;
    ++g_waFwdSeq;
    if (aOut) *aOut = static_cast<int32_t>(g_waFwdSeq);
}

void GetVRWeaponAim(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(aFrame);
    RED4EXT_UNUSED_PARAMETER(a4);
    
    if (aOut) {
        *aOut = g_waEnable;
    }
}


// The memory-scanning arm-from-component natives lived here and are GONE. Treating every qword of a live
// component as a candidate struct crashed the game on weapon draw -- an access violation reading 0x200000002
// from inside a string scan, because the candidate rig's bone-name array reported a believable Size() while its
// storage pointer was garbage. The rig assets give the bone indices offline (see VRWeaponPartSetIdx) and the
// pose function's own a4 argument identifies the rig (see the census), so nothing needs to be searched for.

// The observed pairs. mode -1 clears, -3 counts, -5 saturated; mode >= 0 with field 0 bones, 1 tracks,
// 2 hits with a weapon in hand, 3 hits without, 4/5 the last track buffer low/high.
void VRPairs(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t mode = -3, field = 0;
    RED4ext::GetParameter(aFrame, &mode);
    RED4ext::GetParameter(aFrame, &field);
    aFrame->code++;
    if (!aOut) return;
    *aOut = -1;
    if (mode == -1) { g_PairN = 0; g_PairFull = 0; *aOut = 1; return; }
    if (mode == -3) { *aOut = g_PairN; return; }
    if (mode == -5) { *aOut = g_PairFull; return; }
    if (mode < 0 || mode >= g_PairN) return;
    switch (field) {
        case 0: *aOut = int32_t(g_PairBones[mode]); break;
        case 1: *aOut = int32_t(g_PairTracks[mode]); break;
        case 2: *aOut = int32_t(g_PairOut[mode]); break;
        case 3: *aOut = int32_t(g_PairIn[mode]); break;
        case 4: *aOut = int32_t(uint32_t(g_PairBuf[mode] & 0xFFFFFFFFull)); break;
        case 5: *aOut = int32_t(uint32_t(g_PairBuf[mode] >> 32)); break;
        default: break;
    }
}

// Arm pair entry `index` as the weapon rig `which` (0 magazine, 1 frame), by the track buffer recorded for it.
void VRPairUse(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t index = -1, which = 0;
    RED4ext::GetParameter(aFrame, &index);
    RED4ext::GetParameter(aFrame, &which);
    aFrame->code++;
    if (aOut) *aOut = 0;
    if (index < 0 || index >= g_PairN || which < 0 || which >= VRRIG_N) return;
    g_RigBuf[which] = g_PairBuf[index];
    g_RigBones[which] = g_PairBones[index];
    g_RigTracks[which] = g_PairTracks[index];
    if (aOut) *aOut = 1;
}

// What the pair nomination has found. field: 0/1 passes matched for magazine/frame, 2/3 bones, 4/5 tracks,
// 6/7 track buffer low 32, 8/9 high 32.
void VRRigStatus(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t field = 0;
    RED4ext::GetParameter(aFrame, &field);
    aFrame->code++;
    if (!aOut) return;
    *aOut = -1;
    const int w = field & 1;
    switch (field >> 1) {
        case 0: *aOut = int32_t(g_RigSeen[w]); break;
        case 1: *aOut = int32_t(g_RigBones[w]); break;
        case 2: *aOut = int32_t(g_RigTracks[w]); break;
        case 3: *aOut = int32_t(uint32_t(g_RigBuf[w] & 0xFFFFFFFFull)); break;
        case 4: *aOut = int32_t(uint32_t(g_RigBuf[w] >> 32)); break;
        default: break;
    }
}

// Queue an offset onto one bone of one rig: which 0 = magazine, 1 = frame. Bone indices come from the rig
// assets -- magazine 0 mag_plug, 1 magazine, 2 magazine_reload, 3 mag_std, 4 mag_stdr; frame 5 front_slider,
// 6 back_slider, 8 mag_slot, 12 hammer, 15 weapon_trigger. Passing all zeroes for a bone removes its entry.
void VRRigWrite(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t which = -1, bone = -1;
    float x = 0.0f, y = 0.0f, z = 0.0f;
    RED4ext::GetParameter(aFrame, &which);
    RED4ext::GetParameter(aFrame, &bone);
    RED4ext::GetParameter(aFrame, &x);
    RED4ext::GetParameter(aFrame, &y);
    RED4ext::GetParameter(aFrame, &z);
    aFrame->code++;
    if (aOut) *aOut = -1;
    if (which < 0 || which >= VRRIG_N || bone < 0) return;
    int slot = -1;
    const int n = g_RigWriteN;
    for (int k = 0; k < n; ++k) {
        if (g_RigWriteWhich[k] == which && g_RigWriteBone[k] == bone) { slot = k; break; }
    }
    if (slot < 0) {
        if (n >= VRRIG_WRITES) return;
        slot = n;
        g_RigWriteWhich[slot] = which;
        g_RigWriteBone[slot] = bone;
        g_RigWriteHaveBase[slot] = 0;
        g_RigWriteApplied[slot] = 0;
        g_RigWriteSlot[slot] = -1;
        g_RigWritePin[slot] = -1;
        g_RigWriteScale[slot] = 0.0f;
        g_RigWriteAbs[slot] = 0;
        g_RigWriteQuatOn[slot] = 0;
        g_RigWriteRotAngle[slot] = 0.0f;
        g_RigWriteHaveBaseRot[slot] = 0;
        g_RigWriteN = n + 1;
    }
    g_RigWriteEnabled[slot] = 1;      // writing to a slot always re-arms it
    g_RigWriteOff[slot][0] = x;
    g_RigWriteOff[slot][1] = y;
    g_RigWriteOff[slot][2] = z;
    if (aOut) *aOut = slot;
}

// Set a ROTATION offset on one bone of one rig: angle in degrees about a local axis (ax,ay,az). For spinning
// parts -- the rotator disc, the hammer. Same slot keying as VRRigWrite, and a slot may carry both a translation
// and a rotation. angle 0 disables the rotation write for that slot.
void VRRigWriteRot(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t which = -1, bone = -1;
    float angle = 0.0f, ax = 0.0f, ay = 0.0f, az = 0.0f;
    RED4ext::GetParameter(aFrame, &which);
    RED4ext::GetParameter(aFrame, &bone);
    RED4ext::GetParameter(aFrame, &angle);
    RED4ext::GetParameter(aFrame, &ax);
    RED4ext::GetParameter(aFrame, &ay);
    RED4ext::GetParameter(aFrame, &az);
    aFrame->code++;
    if (aOut) *aOut = -1;
    if (which < 0 || which >= VRRIG_N || bone < 0) return;
    int slot = -1;
    const int n = g_RigWriteN;
    for (int k = 0; k < n; ++k) {
        if (g_RigWriteWhich[k] == which && g_RigWriteBone[k] == bone) { slot = k; break; }
    }
    if (slot < 0) {
        if (n >= VRRIG_WRITES) return;
        slot = n;
        g_RigWriteWhich[slot] = which;
        g_RigWriteBone[slot] = bone;
        g_RigWriteHaveBase[slot] = 0;
        g_RigWriteApplied[slot] = 0;
        g_RigWriteSlot[slot] = -1;
        g_RigWritePin[slot] = -1;
        g_RigWriteScale[slot] = 0.0f;
        g_RigWriteAbs[slot] = 0;
        g_RigWriteQuatOn[slot] = 0;
        g_RigWriteHaveBaseRot[slot] = 0;
        g_RigWriteN = n + 1;
    }
    g_RigWriteEnabled[slot] = 1;      // writing to a slot always re-arms it
    const float len = std::sqrt(ax * ax + ay * ay + az * az);
    if (len < 1e-6f) { angle = 0.0f; ax = 0.0f; ay = 1.0f; az = 0.0f; }  // no axis -> no rotation
    else { ax /= len; ay /= len; az /= len; }
    g_RigWriteRotAxis[slot][0] = ax;
    g_RigWriteRotAxis[slot][1] = ay;
    g_RigWriteRotAxis[slot][2] = az;
    g_RigWriteRotAngle[slot] = angle;
    if (aOut) *aOut = slot;
}

// HAND ONE BONE BACK to the animation: the slot keeps its values but stops being applied, so the very next pose
// pass leaves the game's own pose standing. This is the counterpart of VRRigWrite -- without it a part could only
// ever be taken over, never released, and "stop driving it" had to be faked by writing a rest position, which is
// itself an override (and a hardcoded one). Returns 1 if a slot for this bone existed.
void VRRigWriteOff(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t) {
    int32_t which = -1, bone = -1;
    RED4ext::GetParameter(aFrame, &which);
    RED4ext::GetParameter(aFrame, &bone);
    aFrame->code++;
    if (aOut) *aOut = 0;
    if (which < 0 || which >= VRRIG_N || bone < 0) return;
    const int n = g_RigWriteN;
    for (int k = 0; k < n && k < VRRIG_WRITES; ++k) {
        if (g_RigWriteWhich[k] != which || g_RigWriteBone[k] != bone) continue;
        g_RigWriteEnabled[k] = 0;
        g_RigWriteAbs[k] = 0;
        g_RigWriteQuatOn[k] = 0;
        g_RigWriteRotAngle[k] = 0.0f;
        g_RigWriteScale[k] = 0.0f;
        g_RigWriteOff[k][0] = 0.0f; g_RigWriteOff[k][1] = 0.0f; g_RigWriteOff[k][2] = 0.0f;
        if (aOut) *aOut = 1;
        return;
    }
}

void VRRigWriteClear(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    for (int k = 0; k < VRRIG_WRITES; ++k) {
        g_RigWriteOff[k][0] = 0.0f; g_RigWriteOff[k][1] = 0.0f; g_RigWriteOff[k][2] = 0.0f;
        g_RigWriteHaveBase[k] = 0;
        g_RigWriteApplied[k] = 0;
        g_RigWriteSlot[k] = -1;
        g_RigWritePin[k] = -1;
        g_RigWriteRotAngle[k] = 0.0f;
        g_RigWriteHaveBaseRot[k] = 0;
        g_RigWriteScale[k] = 0.0f;
        g_RigWriteAbs[k] = 0;
        g_RigWriteQuatOn[k] = 0;
        g_RigWriteEnabled[k] = 0;
    }
    // ...and hand every float track back to the animation. This is the panic button script reaches for, and a
    // magazine left hidden by a stale showMagazine write would be invisible with nothing on screen to explain it.
    for (int w = 0; w < VRRIG_N; ++w) {
        for (int e = 0; e < VRRIG_TRACKS; ++e) g_RigTrackOn[w][e] = 0;
    }
    g_RigWriteN = 0;
    if (aOut) *aOut = true;
}

// ---- RELOAD FINGER POSE natives (free-hand grip while holding a weapon part) ----
// Clear the pending grip pose for a hand (0 left, 1 right); its fingers go back under tracking.
void VRReloadFingerClear(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t) {
    int32_t hand = -1;
    RED4ext::GetParameter(aFrame, &hand);
    aFrame->code++;
    if (aOut) *aOut = 0;
    if (hand < 0 || hand > 1) return;
    for (int k = 0; k < 32; ++k) g_VRReloadFingerSet[hand][k] = 0;
    if (aOut) *aOut = 1;
}

// Set one finger bone's target parent-local rotation. The bone is matched by NAME (case-insensitive) against the
// resolved finger list, so callers pass the animation bone name (LeftHandIndex1, ...). The quaternion must already
// be in RUNTIME space -- feed the GLB local quat through (x,-z,y,w) first. Returns the finger slot, or -1.
void VRReloadFingerSet(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t) {
    int32_t hand = -1;
    RED4ext::CString name;
    float qx = 0.0f, qy = 0.0f, qz = 0.0f, qw = 1.0f;
    RED4ext::GetParameter(aFrame, &hand);
    RED4ext::GetParameter(aFrame, &name);
    RED4ext::GetParameter(aFrame, &qx);
    RED4ext::GetParameter(aFrame, &qy);
    RED4ext::GetParameter(aFrame, &qz);
    RED4ext::GetParameter(aFrame, &qw);
    aFrame->code++;
    if (aOut) *aOut = -1;
    if (hand < 0 || hand > 1) return;
    const char* bn = name.c_str();
    if (!bn || !bn[0]) return;
    const int cnt = (hand == 0) ? g_VRSmokeFingerCountL : g_VRSmokeFingerCount;
    for (int k = 0; k < cnt && k < 32; ++k) {
        const char* fn = (hand == 0) ? g_VRSmokeFingerNameL[k] : g_VRSmokeFingerName[k];
        if (fn && fn[0] && EqualsInsensitive(fn, bn)) {
            g_VRReloadFingerRot[hand][k][0] = qx; g_VRReloadFingerRot[hand][k][1] = qy;
            g_VRReloadFingerRot[hand][k][2] = qz; g_VRReloadFingerRot[hand][k][3] = qw;
            g_VRReloadFingerSet[hand][k] = 1;
            if (aOut) *aOut = k;
            return;
        }
    }
}

// Turn the grip pose on (1) or off (0) for a hand.
extern "C" __declspec(dllexport) extern int CyberpunkVR_RestFingerCaptureReq;
extern "C" __declspec(dllexport) extern int CyberpunkVR_DebugRestFingerRefused;
extern "C" __declspec(dllexport) extern int CyberpunkVR_DebugRestFingerHave;

extern "C" __declspec(dllexport) extern int CyberpunkVR_TwoHandCaptureReq;
extern "C" __declspec(dllexport) extern int CyberpunkVR_DebugTwoHandRefused;
extern "C" __declspec(dllexport) extern int CyberpunkVR_DebugTwoHandHave;
// ---- THE TWO-HAND GRIP, from the CET console ----
//
// `VRTwoHandCapture()` takes one frame of the game's own two-handed hold: where the left wrist sits
// against the right one and how its fingers are curled. It needs VRIK OFF -- with VRIK on, the arms in the
// buffer are the controllers, and capturing them would record the player's own pose rather than the
// game's. `VRTwoHandStatus()` answers 1 captured, 0 nothing yet, or a negative bitfield:
// -1 no weapon, -2 VRIK is on, -4 the rig is not resolved.
void VRTwoHandCapture(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t) {
    aFrame->code++;
    CyberpunkVR_DebugTwoHandRefused = 0;
    CyberpunkVR_TwoHandCaptureReq = 1;
    if (aOut) *aOut = 1;
}

void VRTwoHandStatus(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t) {
    aFrame->code++;
    if (!aOut) return;
    if (CyberpunkVR_DebugTwoHandRefused) { *aOut = -CyberpunkVR_DebugTwoHandRefused; return; }
    *aOut = CyberpunkVR_DebugTwoHandHave ? 1 : 0;
}

// ---- THE RESTING LEFT HAND, from the CET console ----
//
// `VRRestFingerCapture()` with empty hands takes one frame of the game's own idle animation and keeps it
// as the pose the left hand wears while a weapon is out; it is saved beside the game and loaded at boot,
// so it is recorded once. The capture itself happens on the animation thread (that is the only place the
// bone buffer exists), so this raises a flag and returns immediately -- ask `VRRestFingerStatus()` for
// what came of it:
//     1  captured
//     0  not yet -- the request has not been seen, which on a paused or loading game is normal
//    <0  refused, and the bits say why: -1 weapon in hand, -2 a reload grip owns the hand,
//        -4 cigarette, -8 in a vehicle (they combine: -3 is a weapon AND a grip)
void VRRestFingerCapture(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t) {
    aFrame->code++;
    CyberpunkVR_DebugRestFingerRefused = 0;
    CyberpunkVR_RestFingerCaptureReq = 1;
    if (aOut) *aOut = 1;
}

// The plugin's own grip-to-grip reach while the weapon is carried, metres; -1 when it is not. The take
// gate reads this so the button and the finger preview cannot disagree about "near the weapon".
//
// There is no capture command beside it any more: the right hand's grip pose is learned from the hand
// itself every pass it holds a weapon (see VrikCarryGripPoseRight), because asking the player to record
// a pose per weapon for fifty weapons is not a feature.
extern "C" __declspec(dllexport) extern float CyberpunkVR_DebugCarryReach;
extern "C" __declspec(dllexport) extern float CyberpunkVR_CarryGripRadius;
void GetVRCarryReach(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void* aOut, int64_t) {
    aFrame->code++;
    if (aOut) *static_cast<float*>(aOut) = CyberpunkVR_DebugCarryReach;
}

// THE SAME QUESTION, ANSWERED WHERE THE RADIUS LIVES. The take gate used to hold its own copy of the
// radius, which is one number in two places and a preview that promises a take the button will not
// perform as soon as the two drift apart. -1 means the plugin has nothing to say (no carry running),
// and the caller may fall back to its own measurement.
void GetVRCarryNear(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t) {
    aFrame->code++;
    if (!aOut) return;
    const float d = CyberpunkVR_DebugCarryReach;
    *aOut = (d < 0.0f) ? -1 : ((d <= CyberpunkVR_CarryGripRadius) ? 1 : 0);
}

// ---- THE RESTING RIGHT HAND, same three commands ----
//
// Needed only since the weapon can be carried in the left hand: that is the one case where this hand is
// empty while the game still poses it around a grip. Capture with EMPTY hands, as on the left.
//     1  captured        0  not yet
//    <0  refused: -1 weapon in hand, -4 the cigarette owns these fingers
extern "C" __declspec(dllexport) extern int CyberpunkVR_RestFingerCaptureReqR;
extern "C" __declspec(dllexport) extern int CyberpunkVR_DebugRestFingerHaveR;
extern "C" __declspec(dllexport) extern int CyberpunkVR_DebugRestFingerRefusedR;
extern "C" __declspec(dllexport) extern int CyberpunkVR_RestFingerApplyR;

void VRRestFingerCaptureRight(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t) {
    aFrame->code++;
    CyberpunkVR_DebugRestFingerRefusedR = 0;
    CyberpunkVR_RestFingerCaptureReqR = 1;
    if (aOut) *aOut = 1;
}

void VRRestFingerStatusRight(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t) {
    aFrame->code++;
    if (!aOut) return;
    if (CyberpunkVR_DebugRestFingerRefusedR) { *aOut = -CyberpunkVR_DebugRestFingerRefusedR; return; }
    *aOut = CyberpunkVR_DebugRestFingerHaveR ? 1 : 0;
}

void VRRestFingerApplyRight(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t) {
    int32_t on = 1;
    RED4ext::GetParameter(aFrame, &on);
    aFrame->code++;
    CyberpunkVR_RestFingerApplyR = on ? 1 : 0;
    if (aOut) *aOut = CyberpunkVR_RestFingerApplyR;
}

void VRRestFingerStatus(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t) {
    aFrame->code++;
    if (!aOut) return;
    if (CyberpunkVR_DebugRestFingerRefused) { *aOut = -CyberpunkVR_DebugRestFingerRefused; return; }
    *aOut = CyberpunkVR_DebugRestFingerHave ? 1 : 0;
}

// Off puts the game's own two-handed-grip fingers back, which is the only way to compare the two.
void VRRestFingerApply(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t) {
    int32_t on = 1;
    RED4ext::GetParameter(aFrame, &on);
    aFrame->code++;
    g_VRRestFingerApply = on ? 1 : 0;
    if (aOut) *aOut = g_VRRestFingerApply;
}

void VRReloadFingerApply(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t) {
    int32_t hand = -1, on = 0;
    RED4ext::GetParameter(aFrame, &hand);
    RED4ext::GetParameter(aFrame, &on);
    aFrame->code++;
    if (aOut) *aOut = 0;
    if (hand < 0 || hand > 1) return;
    g_VRReloadFingerActive[hand] = on ? 1 : 0;
    // blend is NOT touched here: the caller sets it (VRReloadFingerBlend) BEFORE activating, so the very first
    // animation pass already mixes at the intended alpha -- no one-frame flash of the full pose.
    if (aOut) *aOut = 1;
}

// The preview ramp: how much of the grip pose is mixed onto the live tracked fingers (0..1). The Lua FSM ramps
// this over ~150 ms on preview enter/exit so the fingers glide instead of teleporting.
void VRReloadFingerBlend(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t) {
    int32_t hand = -1;
    float alpha = 1.0f;
    RED4ext::GetParameter(aFrame, &hand);
    RED4ext::GetParameter(aFrame, &alpha);
    aFrame->code++;
    if (aOut) *aOut = 0;
    if (hand < 0 || hand > 1) return;
    if (alpha < 0.0f) alpha = 0.0f;
    if (alpha > 1.0f) alpha = 1.0f;
    g_VRReloadFingerBlend[hand] = alpha;
    if (aOut) *aOut = 1;
}

// Forget the identified weapon rigs (track buffers), so the next pass re-identifies by bone name. Called on a
// weapon draw/switch: a stale latch on the previous instance's buffer is one way the slide stops responding.
void VRRigReset(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t) {
    aFrame->code++;
    for (int w = 0; w < VRRIG_N; ++w) {
        g_RigBuf[w] = 0; g_RigBones[w] = 0;
        g_RigTrackHave[w] = 0;                                  // read-back belongs to the rig that is going away
        for (int e = 0; e < VRRIG_TRACKS; ++e) g_RigTrackOn[w][e] = 0;
    }
    for (int l = 0; l < 4; ++l) { g_RigPassSeen[l] = 0; g_RigPassRn[l] = 0; g_RigPassDst[l] = 0; }
    if (aOut) *aOut = true;
}

// Set a uniform SCALE on one bone of one rig (the no-tracks visibility switch: 0.001 hides the round meshes,
// 1.0 restores them; <= 0 stops writing scale). Same slot keying as VRRigWrite/VRRigWriteRot.
void VRRigWriteScale(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t) {
    int32_t which = -1, bone = -1;
    float scale = 0.0f;
    RED4ext::GetParameter(aFrame, &which);
    RED4ext::GetParameter(aFrame, &bone);
    RED4ext::GetParameter(aFrame, &scale);
    aFrame->code++;
    if (aOut) *aOut = -1;
    if (which < 0 || which >= VRRIG_N || bone < 0) return;
    int slot = -1;
    const int n = g_RigWriteN;
    for (int k = 0; k < n; ++k) {
        if (g_RigWriteWhich[k] == which && g_RigWriteBone[k] == bone) { slot = k; break; }
    }
    if (slot < 0) {
        if (n >= VRRIG_WRITES) return;
        slot = n;
        g_RigWriteWhich[slot] = which;
        g_RigWriteBone[slot] = bone;
        g_RigWriteHaveBase[slot] = 0;
        g_RigWriteApplied[slot] = 0;
        g_RigWriteSlot[slot] = -1;
        g_RigWritePin[slot] = -1;
        g_RigWriteRotAngle[slot] = 0.0f;
        g_RigWriteHaveBaseRot[slot] = 0;
        g_RigWriteScale[slot] = 0.0f;
        g_RigWriteN = n + 1;
    }
    g_RigWriteEnabled[slot] = 1;      // writing to a slot always re-arms it
    if (scale > 4.0f) scale = 4.0f;
    g_RigWriteScale[slot] = scale;
    if (aOut) *aOut = slot;
}

// REPLAY a recorded pose on one rig bone: the local translation and rotation are written VERBATIM (not composed
// onto the bone's rest), because a path lifted from a recording already holds the game's own local values. Pass
// abs = 0 to go back to the ordinary base + offset behaviour.
void VRRigWriteAbs(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t) {
    int32_t which = -1, bone = -1, abs = 1;
    float x = 0.0f, y = 0.0f, z = 0.0f, qi = 0.0f, qj = 0.0f, qk = 0.0f, qr = 0.0f;
    RED4ext::GetParameter(aFrame, &which);
    RED4ext::GetParameter(aFrame, &bone);
    RED4ext::GetParameter(aFrame, &x);
    RED4ext::GetParameter(aFrame, &y);
    RED4ext::GetParameter(aFrame, &z);
    RED4ext::GetParameter(aFrame, &qi);
    RED4ext::GetParameter(aFrame, &qj);
    RED4ext::GetParameter(aFrame, &qk);
    RED4ext::GetParameter(aFrame, &qr);
    RED4ext::GetParameter(aFrame, &abs);
    aFrame->code++;
    if (aOut) *aOut = -1;
    if (which < 0 || which >= VRRIG_N || bone < 0) return;
    int slot = -1;
    const int n = g_RigWriteN;
    for (int k = 0; k < n; ++k) {
        if (g_RigWriteWhich[k] == which && g_RigWriteBone[k] == bone) { slot = k; break; }
    }
    if (slot < 0) {
        if (n >= VRRIG_WRITES) return;
        slot = n;
        g_RigWriteWhich[slot] = which;
        g_RigWriteBone[slot] = bone;
        g_RigWriteHaveBase[slot] = 0;
        g_RigWriteApplied[slot] = 0;
        g_RigWriteSlot[slot] = -1;
        g_RigWritePin[slot] = -1;
        g_RigWriteRotAngle[slot] = 0.0f;
        g_RigWriteHaveBaseRot[slot] = 0;
        g_RigWriteScale[slot] = 0.0f;
        g_RigWriteN = n + 1;
    }
    g_RigWriteEnabled[slot] = 1;      // writing to a slot always re-arms it
    g_RigWriteOff[slot][0] = x;
    g_RigWriteOff[slot][1] = y;
    g_RigWriteOff[slot][2] = z;
    g_RigWriteAbs[slot] = abs ? 1 : 0;
    const float ql = std::sqrt(qi * qi + qj * qj + qk * qk + qr * qr);
    if (ql > 1e-4f) {
        g_RigWriteQuat[slot][0] = qi / ql;
        g_RigWriteQuat[slot][1] = qj / ql;
        g_RigWriteQuat[slot][2] = qk / ql;
        g_RigWriteQuat[slot][3] = qr / ql;
        g_RigWriteQuatOn[slot] = 1;
    } else {
        g_RigWriteQuatOn[slot] = 0;       // a zero quaternion means "leave the rotation alone"
    }
    if (aOut) *aOut = slot;
}

// Read one weapon-rig bone's own local transform, as the GAME left it this frame: field 0..2 = translation x/y/z,
// 3..6 = rotation quaternion x/y/z/w. Returns 0 for an unseen rig/bone. This is what lets the recorder capture a
// native reload's part motion -- nothing else can see it (the vrp_ slots do not ride the bones, and a skinned
// mesh component's transform is just the weapon entity's).
void VRRigBone(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, float* aOut, int64_t) {
    int32_t which = -1, bone = -1, field = 0;
    RED4ext::GetParameter(aFrame, &which);
    RED4ext::GetParameter(aFrame, &bone);
    RED4ext::GetParameter(aFrame, &field);
    aFrame->code++;
    if (aOut) *aOut = 0.0f;
    if (which < 0 || which >= VRRIG_N || bone < 0 || bone >= 20 || field < 0 || field > 6) return;
    if (!g_RigPoseHave[which]) return;
    if (aOut) *aOut = g_RigPose[which][bone][field];
}

// The CName hash: FNV1a64 over the name's bytes. Verified against all five bone hashes that used to be compiled in
// for the Silverhand (mag_std, mag_stdr, front_slider, back_slider, mag_slot) -- exact on every one, so a weapon can
// hand the plugin plain bone NAMES and identification stays a byte comparison.
static uint64_t VRNameHash(const char* s) {
    uint64_t h = 0xCBF29CE484222325ull;
    if (!s) return h;
    for (; *s; ++s) { h ^= static_cast<uint8_t>(*s); h *= 0x100000001B3ull; }
    return h;
}

// Fill one signature slot directly, for the defaults registered at startup.
static void VRRigSigAdd(int which, uint32_t bones,
                        int i0, const char* n0, int i1, const char* n1, int i2, const char* n2) {
    const int n = g_RigSigN;
    if (which < 0 || which >= VRRIG_N || bones == 0 || n >= VRRIG_SIG_MAX) return;
    const int idx[VRRIG_SIG_NAMES] = { i0, i1, i2, -1 };
    const char* nm[VRRIG_SIG_NAMES] = { n0, n1, n2, nullptr };
    for (int k = 0; k < VRRIG_SIG_NAMES; ++k) {
        if (idx[k] < 0 || !nm[k] || !nm[k][0]) { g_RigSigIdx[n][k] = -1; g_RigSigHash[n][k] = 0; continue; }
        g_RigSigIdx[n][k] = idx[k];
        g_RigSigHash[n][k] = VRNameHash(nm[k]);
    }
    g_RigSigWhich[n] = which;
    g_RigSigBones[n] = bones;
    g_RigSigN = n + 1;
}

// REGISTER A RIG SIGNATURE: which rig slot it claims (0 magazine, 1 frame), how many bones that rig has, and up to
// four bone NAMES with the index each must sit at. Re-registering the same (which, bones) pair overwrites it, so a
// config can be reloaded without filling the table. An empty name skips its slot.
void VRRigSignature(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t) {
    int32_t which = -1, bones = 0;
    int32_t i0 = -1, i1 = -1, i2 = -1, i3 = -1;
    RED4ext::CString n0, n1, n2, n3;
    RED4ext::GetParameter(aFrame, &which);
    RED4ext::GetParameter(aFrame, &bones);
    RED4ext::GetParameter(aFrame, &i0); RED4ext::GetParameter(aFrame, &n0);
    RED4ext::GetParameter(aFrame, &i1); RED4ext::GetParameter(aFrame, &n1);
    RED4ext::GetParameter(aFrame, &i2); RED4ext::GetParameter(aFrame, &n2);
    RED4ext::GetParameter(aFrame, &i3); RED4ext::GetParameter(aFrame, &n3);
    aFrame->code++;
    if (aOut) *aOut = -1;
    if (which < 0 || which >= VRRIG_N || bones <= 0) return;

    // Hash the names FIRST, because the slot lookup has to compare the whole signature.
    const int32_t idx[VRRIG_SIG_NAMES] = { i0, i1, i2, i3 };
    const RED4ext::CString* nm[VRRIG_SIG_NAMES] = { &n0, &n1, &n2, &n3 };
    int32_t widx[VRRIG_SIG_NAMES];
    uint64_t whash[VRRIG_SIG_NAMES];
    for (int k = 0; k < VRRIG_SIG_NAMES; ++k) {
        const char* s = nm[k]->c_str();
        if (idx[k] < 0 || !s || !s[0]) { widx[k] = -1; whash[k] = 0; continue; }
        widx[k] = idx[k];
        whash[k] = VRNameHash(s);
    }

    // A SLOT IS REUSED ONLY BY AN IDENTICAL SIGNATURE. It used to be keyed on (which, bone count) alone, so that
    // re-registering on a redraw cost nothing -- but two DIFFERENT rigs can share a bone count, and then the second
    // one silently overwrote the first. Real case: the Militech Lexington's frame rig has 11 bones, exactly like the
    // Constitutional Unity's family, and names nothing `slide`; and its magazine rig is the SHARED 5-bone
    // w_handgun__mag_std, whose bone order differs from the per-weapon ones. Registering the Lexington would have
    // un-registered four working pistols. The matcher in the pose hook already walks every signature and compares
    // names, so distinct entries are all it needed.
    int slot = -1;
    const int n = g_RigSigN;
    for (int k = 0; k < n && k < VRRIG_SIG_MAX; ++k) {
        if (g_RigSigWhich[k] != which || g_RigSigBones[k] != static_cast<uint32_t>(bones)) continue;
        bool same = true;
        for (int j = 0; j < VRRIG_SIG_NAMES; ++j) {
            if (g_RigSigIdx[k][j] != widx[j] || g_RigSigHash[k][j] != whash[j]) { same = false; break; }
        }
        if (same) { slot = k; break; }
    }
    if (slot < 0) {
        if (n >= VRRIG_SIG_MAX) return;      // -1 to script: the caller logs it (see reload/rigs.lua)
        slot = n;
    }
    for (int k = 0; k < VRRIG_SIG_NAMES; ++k) {
        g_RigSigIdx[slot][k] = widx[k];
        g_RigSigHash[slot][k] = whash[k];
    }
    g_RigSigWhich[slot] = which;
    g_RigSigBones[slot] = static_cast<uint32_t>(bones);
    if (slot >= g_RigSigN) g_RigSigN = slot + 1;
    if (aOut) *aOut = slot;
}

// Read one of a weapon rig's FLOAT TRACKS as the game leaves it this frame. idx < 0 returns the rig's track count
// (magazine rig 2, frame rig 0), so script can check it is looking at the right rig before trusting a value.
// For the magazine rig index 0 is `showMagazine` and 1 is `showMagazineReload`, and the rig's own referenceTracks
// say they rest at 1 and 0 -- which is the check that proves this buffer is really the track values.
void VRRigTrack(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, float* aOut, int64_t) {
    int32_t which = -1, idx = -1;
    RED4ext::GetParameter(aFrame, &which);
    RED4ext::GetParameter(aFrame, &idx);
    aFrame->code++;
    if (aOut) *aOut = -1.0f;
    if (which < 0 || which >= VRRIG_N) return;
    if (idx < 0) { if (aOut) *aOut = static_cast<float>(g_RigTracks[which]); return; }
    if (idx >= VRRIG_TRACKS || !g_RigTrackHave[which]) return;
    if (aOut) *aOut = g_RigTrackVal[which][idx];
}

// Drive one of a weapon rig's FLOAT TRACKS -- the game's own way to show or hide a part. `on = 0` hands the track
// back to the animation. This is how the seated magazine is hidden while one is in the player's hand: the mesh
// component's visibilityAnimationParam is `showMagazine`, so writing 0 there is exactly what the game's own reload
// animation does. It touches no bone, so the game's magazine motion keeps running underneath (the scale trick this
// replaces had to hold a bone every frame, which pinned that animation and stopped native reloads ejecting).
void VRRigTrackWrite(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t) {
    int32_t which = -1, idx = -1, on = 1;
    float value = 0.0f;
    RED4ext::GetParameter(aFrame, &which);
    RED4ext::GetParameter(aFrame, &idx);
    RED4ext::GetParameter(aFrame, &value);
    RED4ext::GetParameter(aFrame, &on);
    aFrame->code++;
    if (aOut) *aOut = 0;
    if (which < 0 || which >= VRRIG_N || idx < 0 || idx >= VRRIG_TRACKS) return;
    g_RigTrackSet[which][idx] = value;
    g_RigTrackOn[which][idx] = on ? 1 : 0;
    if (aOut) *aOut = 1;
}

// Reload recorder toggle: while on, the pose hook recomputes the whole-skeleton FK from the ANIMATED pose every
// player pass and publishes it to VRBoneModelPos/Rot -- the only way those read live bones with VRIK off.
void VRRecordFK(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t) {
    int32_t on = 0;
    RED4ext::GetParameter(aFrame, &on);
    aFrame->code++;
    g_VRRecordFK = on ? 1 : 0;
    if (aOut) *aOut = g_VRRecordFK;
}

// Per-slot state of the write queue, so a failed write says WHY instead of leaving it to guesswork. slot < 0
// returns the slot count; otherwise field: 0 which, 1 bone, 2 haveBase, 3 applied (times the SET write ran),
// 4/5/6 offset x/y/z in mm, 7/8/9 base x/y/z in mm.
void VRRigWriteDiag(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t slot = -1, field = 0;
    RED4ext::GetParameter(aFrame, &slot);
    RED4ext::GetParameter(aFrame, &field);
    aFrame->code++;
    if (!aOut) return;
    if (slot < 0) { *aOut = g_RigWriteN; return; }
    *aOut = -1;
    if (slot >= VRRIG_WRITES) return;
    switch (field) {
        case 0: *aOut = g_RigWriteWhich[slot]; break;
        case 1: *aOut = g_RigWriteBone[slot]; break;
        case 2: *aOut = g_RigWriteHaveBase[slot]; break;
        case 3: *aOut = int32_t(g_RigWriteApplied[slot]); break;
        case 4: *aOut = int32_t(g_RigWriteOff[slot][0] * 1000.0f); break;
        case 5: *aOut = int32_t(g_RigWriteOff[slot][1] * 1000.0f); break;
        case 6: *aOut = int32_t(g_RigWriteOff[slot][2] * 1000.0f); break;
        case 7: *aOut = int32_t(g_RigWriteBase[slot][0] * 1000.0f); break;
        case 8: *aOut = int32_t(g_RigWriteBase[slot][1] * 1000.0f); break;
        case 9: *aOut = int32_t(g_RigWriteBase[slot][2] * 1000.0f); break;
        case 10: *aOut = g_RigWriteSlot[slot]; break;   // pose-buffer slot the LAST pass resolved this bone to
        case 11: *aOut = g_RigWritePin[slot]; break;    // the pinned (a4==0 authoritative) slot; -1 = not yet
        default:
            // 20..23 = frame-rig pass count by a4; 24..27 = that pass's remap entry count; 28..31 = where that
            // pass's remap puts write slot 0's bone (-1 = dropped). `slot` is ignored for these.
            if (field >= 20 && field <= 23) *aOut = int32_t(g_RigPassSeen[field - 20]);
            else if (field >= 24 && field <= 27) *aOut = g_RigPassRn[field - 24];
            else if (field >= 28 && field <= 31) *aOut = g_RigPassDst[field - 28];
            else if (field >= 32 && field <= 35) *aOut = g_RigPassSlots[field - 32];
            else if (field >= 36 && field <= 39) *aOut = g_RigPassBufLo[field - 36];
            else if (field >= 40 && field <= 47) *aOut = g_RigPassMap0[field - 40];
            break;
    }
}

// The small-rig table, and the exact identification it carries.
//   mode -1 clear, -3 count, -5 saturated
//   mode >= 0 with field: 0 bones remapped, 1 a4, 2 buf low 32, 3 buf high 32, 4 hits armed, 5 hits holstered
void VRSmallRig(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t mode = -3, field = 0;
    RED4ext::GetParameter(aFrame, &mode);
    RED4ext::GetParameter(aFrame, &field);
    aFrame->code++;
    if (!aOut) return;
    *aOut = -1;
    if (mode == -1) { g_SmallN = 0; g_SmallFull = 0; *aOut = 1; return; }
    if (mode == -3) { *aOut = g_SmallN; return; }
    if (mode == -5) { *aOut = g_SmallFull; return; }
    if (mode < 0 || mode >= g_SmallN) return;
    switch (field) {
        case 0: *aOut = int32_t(g_SmallBones[mode]); break;
        case 1: *aOut = int32_t(g_SmallA4[mode]); break;
        case 2: *aOut = int32_t(uint32_t(g_SmallBuf[mode] & 0xFFFFFFFFull)); break;
        case 3: *aOut = int32_t(uint32_t(g_SmallBuf[mode] >> 32)); break;
        case 4: *aOut = int32_t(g_SmallOut[mode]); break;
        case 5: *aOut = int32_t(g_SmallIn[mode]); break;
        default: break;
    }
}

// Arm small-rig entry `index` as the weapon's rig, by the track buffer recorded for it.
void VRSmallRigUse(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t index = -1;
    RED4ext::GetParameter(aFrame, &index);
    aFrame->code++;
    if (aOut) *aOut = 0;
    if (index < 0 || index >= g_SmallN) { g_WeaponRigActive = 0; return; }
    g_WeaponTrackBufA = g_SmallBuf[index];
    g_WeaponTrackBufB = 0;
    g_WeaponPartHave = 0;
    g_WeaponPartCount = 24;
    g_WeaponRigActive = 1;
    if (aOut) *aOut = 1;
}

// The census: switch it on, read it back, and arm one of its entries as the weapon's rig.
//   mode -1 : clear and enable   -2 : disable   -3 : entry count
//   mode -4 : field says whether a weapon is in hand right now   -5 : did the table saturate
//   mode >= 0 with field: 0 hits, 1 a4, 2 trackBuf low 32, 3 trackBuf high 32, 4 born with a weapon out, 5 hits while armed, 6 hits while holstered
void VRPoseCensus(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t mode = -3, field = 0;
    RED4ext::GetParameter(aFrame, &mode);
    RED4ext::GetParameter(aFrame, &field);
    aFrame->code++;
    if (!aOut) return;
    *aOut = -1;
    if (mode == -1) { g_PoseCensusN = 0; g_PoseCensusFull = 0; g_PoseCensusOn = 1; *aOut = 1; return; }
    if (mode == -4) { g_PoseCensusWeaponOut = field ? 1 : 0; *aOut = 1; return; }
    if (mode == -5) { *aOut = g_PoseCensusFull; return; }
    if (mode == -2) { g_PoseCensusOn = 0; *aOut = 0; return; }
    if (mode == -3) { *aOut = g_PoseCensusN; return; }
    if (mode < 0 || mode >= g_PoseCensusN) return;
    switch (field) {
        case 0: *aOut = int32_t(g_PoseCensusHits[mode]); break;
        case 1: *aOut = int32_t(g_PoseCensusA4[mode]); break;
        case 2: *aOut = int32_t(uint32_t(g_PoseCensusBuf[mode] & 0xFFFFFFFFull)); break;
        case 3: *aOut = int32_t(uint32_t(g_PoseCensusBuf[mode] >> 32)); break;
        case 4: *aOut = g_PoseCensusArmed[mode]; break;
        case 5: *aOut = int32_t(g_PoseCensusHitsOut[mode]); break;
        case 6: *aOut = int32_t(g_PoseCensusHitsIn[mode]); break;
        default: break;
    }
}

// Arm census entry `index` as the weapon's rig. Nothing is dereferenced: the track buffer is a value the hook
// was handed, and matching it again is all the hook needs to recognise this rig's pass.
void VRWeaponRigUse(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t index = -1;
    RED4ext::GetParameter(aFrame, &index);
    aFrame->code++;
    if (aOut) *aOut = 0;
    if (index < 0 || index >= g_PoseCensusN) { g_WeaponRigActive = 0; return; }
    g_WeaponTrackBufA = g_PoseCensusBuf[index];
    g_WeaponTrackBufB = 0;
    g_WeaponPartHave = 0;
    g_WeaponPartCount = 24;
    g_WeaponRigActive = 1;
    if (aOut) *aOut = 1;
}

// Bone index per part slot, taken from the rig ASSET rather than searched for at runtime. The frame rig's order
// is barrel_plug, barrel, muzzle_slot, fx_muzzle, pos_ironsight, front_slider, back_slider, safelock, mag_slot,
// bullet_pull, bullet, bullet_reload, hammer, rotator, ammo_mover, weapon_trigger; the magazine rig's is
// mag_plug, magazine, magazine_reload, mag_std, mag_stdr.
void VRWeaponPartSetIdx(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t slot = -1, bone = -1;
    RED4ext::GetParameter(aFrame, &slot);
    RED4ext::GetParameter(aFrame, &bone);
    aFrame->code++;
    if (aOut) *aOut = false;
    if (slot < 0 || slot >= 24) return;
    g_WeaponPartIdx[slot] = (bone >= 0 && bone < VRIK_MAX_BONES) ? bone : -1;
    if (aOut) *aOut = true;
}

// The write test: offset one part's local translation, in metres, and enable writing.
void VRWeaponPartOffset(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t slot = -1;
    float x = 0.0f, y = 0.0f, z = 0.0f;
    RED4ext::GetParameter(aFrame, &slot);
    RED4ext::GetParameter(aFrame, &x);
    RED4ext::GetParameter(aFrame, &y);
    RED4ext::GetParameter(aFrame, &z);
    aFrame->code++;
    if (aOut) *aOut = false;
    if (slot < 0 || slot >= 24) return;
    g_WeaponPartOff[slot][0] = x;
    g_WeaponPartOff[slot][1] = y;
    g_WeaponPartOff[slot][2] = z;
    g_WeaponPartWriteOn = 1;
    if (aOut) *aOut = true;
}

// Arms the WEAPON's rig: resolve its track buffers and its part bone indices by name. Takes the weapon entity
// from script (Game.GetPlayer():GetActiveWeapon()), because finding it there is one call.
//
// Returns the number of part names resolved on this rig, 0 if the rig was found but carries none of them, and
// -1 if the rig itself could not be found -- three different failures worth telling apart.
void VRWeaponRigArm(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    RED4ext::Handle<RED4ext::IScriptable> h;
    RED4ext::GetParameter(aFrame, &h);
    aFrame->code++;
    if (aOut) *aOut = -1;

    g_WeaponRigActive = 0;
    g_WeaponPartHave = 0;
    g_WeaponPartCount = 0;
    g_WeaponTrackBufA = 0;
    g_WeaponTrackBufB = 0;
    for (int k = 0; k < 24; ++k) g_WeaponPartIdx[k] = -1;
    if (!h.instance) return;

    for (int k = 0; k < 8; ++k) g_WeaponRigDiag[k] = 0;
    g_WeaponRigNames[0] = 0;

    auto* ent = reinterpret_cast<RED4ext::ent::Entity*>(h.instance);
    // First: the weapon's own entity. A weapon's animated component is not called "root", so take any.
    auto* animObj = FindAnimatedObjectForEntity(ent, nullptr, /*anyComponent*/ true);
    g_WeaponRigDiag[0] = animObj ? 1 : 0;

    // Then the PLAYER's entity, because an attached weapon's rig may be registered there instead. Report every
    // component name either way -- a name that can be read beats a guess.
    RED4ext::anim::AnimatedObject* fromPlayer = nullptr;
    auto* pl = FindPlayerEntity();
    const int nOnPlayer = ListAnimatedComponents(pl, g_WeaponRigNames, sizeof(g_WeaponRigNames), &fromPlayer);
    g_WeaponRigDiag[5] = nOnPlayer;
    g_WeaponRigDiag[6] = fromPlayer ? 1 : 0;
    if (!animObj) animObj = fromPlayer;
    if (!animObj || !VRIK_IsReadable(animObj, 0x40)) return;

    uint8_t* base = reinterpret_cast<uint8_t*>(animObj);
    void* ownerA = *reinterpret_cast<void**>(base + 0x8);
    if (VRIK_IsReadable(ownerA, 0x48))
        g_WeaponTrackBufA = *reinterpret_cast<uintptr_t*>(reinterpret_cast<uint8_t*>(ownerA) + 0x40);
    void* ownerB = *reinterpret_cast<void**>(base + 0x18);
    if (VRIK_IsReadable(ownerB, 0x20))
        g_WeaponTrackBufB = *reinterpret_cast<uintptr_t*>(reinterpret_cast<uint8_t*>(ownerB) + 0x18);
    g_WeaponRigDiag[1] = g_WeaponTrackBufA ? 1 : 0;
    g_WeaponRigDiag[2] = g_WeaponTrackBufB ? 1 : 0;
    if (!g_WeaponTrackBufA && !g_WeaponTrackBufB) return;

    int found = 0;
    auto* metaRig = animObj->metaRig;
    if (metaRig && std::strcmp(ClassifyQword(reinterpret_cast<uint64_t>(metaRig)), "HEAP") == 0) {
        const uint32_t nb = metaRig->boneNames.Size();
        g_WeaponRigDiag[3] = int(nb);
        for (int k = 0; k < kWeaponPartN; ++k) {
            for (uint32_t b = 0; b < nb; ++b) {
                const char* nm = metaRig->boneNames[b].ToString();
                if (nm && std::strcmp(nm, kWeaponPartNames[k]) == 0) {
                    g_WeaponPartIdx[k] = int(b);
                    ++found;
                    break;
                }
            }
        }
    }
    g_WeaponRigDiag[4] = found;
    g_WeaponPartCount = kWeaponPartN;
    g_WeaponRigActive = 1;
    if (aOut) *aOut = found;
}

// One part's captured PARENT-LOCAL translation. W = 1 when the weapon's pose pass has been seen at least once.
void VRWeaponPartLocal(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t slot = 0;
    RED4ext::GetParameter(aFrame, &slot);
    aFrame->code++;
    if (!aOut) return;
    aOut->X = 0.0f; aOut->Y = 0.0f; aOut->Z = 0.0f; aOut->W = 0.0f;
    if (slot < 0 || slot >= kWeaponPartN || !g_WeaponPartHave) return;
    if (g_WeaponPartIdx[slot] < 0) return;
    aOut->X = g_WeaponPartPos[slot][0];
    aOut->Y = g_WeaponPartPos[slot][1];
    aOut->Z = g_WeaponPartPos[slot][2];
    aOut->W = 1.0f;
}

// The animated component names found on the player entity, space separated. Which of these is the weapon's rig
// is the whole question, and a name is worth more than another round of guessing.
void VRWeaponRigNames(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame,
                      RED4ext::CString* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    if (aOut) *aOut = RED4ext::CString(g_WeaponRigNames);
}

// Diagnostics: how many of this rig's pose passes the hook has seen, and which bone each part resolved to.
void VRWeaponRigStatus(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t slot = -1;
    RED4ext::GetParameter(aFrame, &slot);
    aFrame->code++;
    if (!aOut) return;
    if (slot == -1) { *aOut = int32_t(g_WeaponMatchCalls & 0x7FFFFFFF); return; }
    if (slot <= -2 && slot >= -9) { *aOut = g_WeaponRigDiag[-slot - 2]; return; }
    if (slot < 0) { *aOut = -1; return; }
    *aOut = (slot < kWeaponPartN) ? g_WeaponPartIdx[slot] : -1;
}
