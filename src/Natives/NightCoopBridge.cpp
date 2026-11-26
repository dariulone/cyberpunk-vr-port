// NIGHTCOOP: the VR body as partners see it. See include/Natives/NightCoopBridge.hpp.
//
// WHAT GOES OUT, AND IN WHICH CONVENTION (read from NightCoop 0.2.1's SDK and its own pose simulator, which feeds the
// same poseSet): every transform in WORLD space (NCSDK_POSE_WORLD_SPACE); NightCoop turns it into our body's space with
// the player's own transform at the call, and the partner puts it back on our mirror there.
//   head   the head BONE's position, and the HMD's world rotation in the game's camera convention (identity looks
//          along +Y with +Z up). NightCoop measures which local axis of the head bone looks forward in the rig's
//          reference pose and turns the bone by this look; the position is what its neck reaches for.
//   hands  the LeftHand / RightHand bones as the solved rig has them: the bone's own world rotation, not a controller
//          frame. NightCoop's simulator builds its hands the same way (a bone rest rotation per side, swung to the
//          reach), and its arm IK puts the partner's hand bone exactly there -- so the partner's arm is ours.
//   fingers the curl of each digit as V's hand draws it, 0 straight .. 1 fist, left thumb..little then right; -1 where
//          the game's graph poses the hand (a weapon, a held magazine): the partner's game keeps its grip there.
//   aim    the muzzle and its forward line while a ranged weapon is out (the port's own muzzle publication), so the
//          partner's copy of our shots leaves our barrel, not our view.
//   crouch the physical squat, 0 standing .. 1 at NightCoop's full squat (its simulator drops the head 0.45 m there).
//
// WHERE IT COMES FROM: the solved-rig snapshot (g_VRFKSnap*, model space, published by the pose path while diag
// capture is on -- VRIK keeps it on whenever tracking drives the body) put in the world by the same model-to-world
// pair the menu's pointers use (the camera's model point and rotation against the view's world pose). A snapshot that
// stopped moving (VRIK suspended by a scene, a menu, loading) is old: the source is taken off and the partners'
// game shows our body by its own graph again.

#include "Natives/NightCoopBridge.hpp"

#include "Anim/CharacterRig.hpp"
#include "Anim/HandGestures.hpp"
#include "Anim/VrikHook.hpp"
#include "Anim/VrikState.hpp"
#include "Anim/WeaponAimState.hpp"
#include "Camera/CameraState.hpp"
#include "Core/VrCoreShared.hpp"
#include "Runtimes/FingerGrip.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include "Utils/SharedSlots.hpp"

#include "nightcoop_sdk.h"

#include <windows.h>

#include <algorithm>
#include <atomic>
#include <cmath>
#include <cstdio>
#include <cstring>

extern void Log(const char* fmt, ...);
extern "C" float CyberpunkVR_PhysicalCrouchDepth;   // Runtimes/PhysicalCrouch.cpp
extern "C" int CyberpunkVR_FingerBones;             // Anim/HandGestures.cpp
extern "C" float CyberpunkVR_GestureStraight[2][5];  // Anim/HandGestures.cpp

extern "C" {
// 1 = the VR body goes to NightCoop as a pose source (live). 0 = the partners see the game's own animation of V.
__declspec(dllexport) int CyberpunkVR_NightCoopPose = 1;
// 1 = in a vehicle the hands go too; 0 (default) = the head and the aim only: NightCoop's own seat pose keeps the
// partner's copy of our hands on its mirror of the wheel, which our wheel hands would not match.
__declspec(dllexport) int CyberpunkVR_NightCoopVehicleHands = 0;
// Read-only diagnostics: 0 NightCoop not found (yet), 1 found, its SDK table taken, 2 registered, -1 refused / its SDK
// switched off, -2 NightCoop unloaded. Bits of the last pose given (NCSDK_POSE_*), the answer of the last poseSet, how
// many poseSet calls were taken.
__declspec(dllexport) int CyberpunkVR_NightCoopLink = 0;
__declspec(dllexport) int CyberpunkVR_NightCoopPoseBits = 0;
__declspec(dllexport) int CyberpunkVR_NightCoopPoseResult = 0;
__declspec(dllexport) unsigned int CyberpunkVR_NightCoopPoseSets = 0;
// NightCoop's frame calls seen, and what stopped the last one short of poseSet: 0 none (sent), 1 switched off, 2 no VR
// session, 3 the VR arms not bound, 4 the solved rig not published lately, 5 no model-to-world frame, 6 no head or hand.
__declspec(dllexport) unsigned int CyberpunkVR_NightCoopFrames = 0;
__declspec(dllexport) int CyberpunkVR_NightCoopGate = -1;
}

namespace cvr::nightcoop {
namespace {

#define CVR_NC_STR2(x) #x
#define CVR_NC_STR(x) CVR_NC_STR2(x)
constexpr const char* kModId = "cyberpunkvrport.vr";   // stable forever: partners' NightCoop keys the source by it
constexpr const char* kModName = "CyberpunkVRPort (VR body)";
constexpr const char* kModVersion = CVR_NC_STR(CVR_VERSION_MAJOR) "." CVR_NC_STR(CVR_VERSION_MINOR) "." CVR_NC_STR(CVR_VERSION_PATCH);
constexpr uint32_t kProtocol = 1;
// NightCoop's simulator drops the head by up to 0.45 m at a crouch of 1 (0.40 by its cap): the same scale here.
constexpr float kFullSquatMetres = 0.45f;
// A relaxed hand, neither pulled straight nor closed: what the gesture model's rest pose is worth as a curl.
constexpr float kRestCurl = 0.25f;
constexpr ULONGLONG kStaleMs = 250;
constexpr ULONGLONG kLookupEveryMs = 1000;
constexpr ULONGLONG kRefusedRetryMs = 30000;

const NcSdkApi* g_api = nullptr;
NcSdkMod g_mod = 0;
NcSdkHooks g_hooks{};
ULONGLONG g_nextLookupMs = 0;
bool g_gone = false;          // NightCoop said it is unloading: never look again this run
bool g_poseOn = false;        // a source of ours is set at NightCoop
uint32_t g_lastSnapSeq = 0;
ULONGLONG g_snapMovedMs = 0;
uint32_t g_lastMuzzleSeq = 0;
ULONGLONG g_muzzleMovedMs = 0;
int32_t g_lastLoggedResult = 1;
uint32_t g_lastLoggedBits = 0xFFFFFFFFu;

void ModLog(int32_t level, const char* text) {
    if (g_api && g_mod && NCSDK_HAS(g_api, log)) g_api->log(g_mod, level, text);
}

bool Finite(const float* v, int n) {
    for (int i = 0; i < n; ++i)
        if (!std::isfinite(v[i])) return false;
    return true;
}

bool Normalized(float* q) {
    const float n = std::sqrt(q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]);
    if (!std::isfinite(n) || n < 0.5f || n > 1.5f) return false;
    for (int k = 0; k < 4; ++k) q[k] /= n;
    return true;
}

// The model-to-world pair of the solved rig: what the VR menu's pointers and the hand collision use.
struct ModelFrame {
    float rot[4];        // world <- model
    float camModel[3];   // the camera's point in model space ...
    float camWorld[3];   // ... and in the world
};

bool ReadModelFrame(ModelFrame& f) {
    if (!g_VRPalmModelValid || !g_VRViewWorldValid) return false;
    const float camRot[4] = {g_VRCamModelRot[0], g_VRCamModelRot[1], g_VRCamModelRot[2], g_VRCamModelRot[3]};
    const float viewRot[4] = {g_VRViewWorldRot[0], g_VRViewWorldRot[1], g_VRViewWorldRot[2], g_VRViewWorldRot[3]};
    float camInv[4];
    VRIK_QuatConj(camRot, camInv);
    VRIK_QuatMul(viewRot, camInv, f.rot);
    if (!Normalized(f.rot)) return false;
    for (int k = 0; k < 3; ++k) {
        f.camModel[k] = g_VRCamModelPos[k];
        f.camWorld[k] = g_VRViewWorldPos[k];
    }
    return Finite(f.camModel, 3) && Finite(f.camWorld, 3);
}

// A bone of the snapshot in the world.
bool BoneWorld(const ModelFrame& f, int bone, NcSdkTransform& out) {
    if (bone < 0 || bone >= g_VRFKSnapCount || bone >= VRIK_MAX_BONES) return false;
    const float rel[3] = {g_VRFKSnapPos[bone][0] - f.camModel[0], g_VRFKSnapPos[bone][1] - f.camModel[1],
                          g_VRFKSnapPos[bone][2] - f.camModel[2]};
    float q[4] = {g_VRFKSnapRot[bone][0], g_VRFKSnapRot[bone][1], g_VRFKSnapRot[bone][2], g_VRFKSnapRot[bone][3]};
    if (!Finite(rel, 3) || !Normalized(q)) return false;
    float w[3], wq[4];
    VRIK_QuatRotateVec(f.rot, rel, w);
    VRIK_QuatMul(f.rot, q, wq);
    if (!Normalized(wq)) return false;
    out.pos = {f.camWorld[0] + w[0], f.camWorld[1] + w[1], f.camWorld[2] + w[2]};
    out.rot = {wq[0], wq[1], wq[2], wq[3]};
    return true;
}

// One hand's five curls: tracked fingers, else the gesture model where it owns the hand, else the game's graph (-1).
bool HandCurls(int side, float* out) {
    float tracked[5];
    if (CyberpunkVR_FingerBones != 0 && fingergrip::TrackedCurl(side, tracked)) {
        const auto t = gestures::FromTracking(tracked);
        for (int d = 0; d < 5; ++d) out[d] = std::clamp(t.closed[d], 0.f, 1.f);
        return true;
    }
    if (!gestures::OwnsHand(side)) {
        for (int d = 0; d < 5; ++d) out[d] = -1.f;
        return false;
    }
    for (int d = 0; d < 5; ++d) {
        const float closed = std::clamp(gestures::FingerClosed(side, d), 0.f, 1.f);
        const float straight = std::clamp(CyberpunkVR_GestureStraight[side][d], 0.f, 1.f);
        out[d] = closed + (1.f - closed) * (1.f - straight) * kRestCurl;
    }
    return true;
}

// The muzzle while a ranged weapon is out: its point and its forward line (shared[24..26]), fresh only.
bool MuzzleAim(ULONGLONG now, NcSdkVec3& origin, NcSdkVec3& dir) {
    const uint32_t seq = g_provMuzzlePosSeq;
    if (seq != g_lastMuzzleSeq) {
        g_lastMuzzleSeq = seq;
        g_muzzleMovedMs = now;
    }
    if (!g_pSharedHands || !seq || now - g_muzzleMovedMs > kStaleMs) return false;
    if (g_pSharedHands[vrshared::kWeaponFlag] <= 0.5f || g_pSharedHands[27] <= 0.5f) return false;
    float d[3] = {g_pSharedHands[24], g_pSharedHands[25], g_pSharedHands[26]};
    const float p[3] = {g_provMuzzlePos[0], g_provMuzzlePos[1], g_provMuzzlePos[2]};
    const float n = std::sqrt(d[0] * d[0] + d[1] * d[1] + d[2] * d[2]);
    if (!Finite(p, 3) || !std::isfinite(n) || n < 0.5f) return false;
    origin = {p[0], p[1], p[2]};
    dir = {d[0] / n, d[1] / n, d[2] / n};
    return true;
}

void ClearPose(int gate, const char* why) {
    CyberpunkVR_NightCoopGate = gate;
    if (!g_poseOn) return;
    g_poseOn = false;
    CyberpunkVR_NightCoopPoseBits = 0;
    g_lastLoggedBits = 0xFFFFFFFFu;
    if (g_api && g_mod && NCSDK_HAS(g_api, poseClear)) g_api->poseClear(g_mod);
    Log("[nightcoop] pose source off: %s\n", why);
}

// NightCoop's frame (game thread, after its own tick).
void OnFrame(void*, float) {
    if (!g_api || !g_mod) return;
    ++CyberpunkVR_NightCoopFrames;
    const ULONGLONG now = GetTickCount64();
    const uint32_t seq = g_VRFKSnapSeq.load(std::memory_order_acquire);
    if (seq != g_lastSnapSeq) {
        g_lastSnapSeq = seq;
        g_snapMovedMs = now;
    }
    if (!CyberpunkVR_NightCoopPose) return ClearPose(1, "switched off (CyberpunkVR_NightCoopPose 0)");
    if (!OpenXRManager::Get().IsSessionRunning()) return ClearPose(2, "no VR session");
    if (!CyberpunkVR_PlayerEntityValid || g_VRBind != 4) return ClearPose(3, "the VR arms are not bound");
    if (!seq || now - g_snapMovedMs > kStaleMs) return ClearPose(4, "the solved rig is not published (VRIK suspended)");
    ModelFrame frame;
    if (!ReadModelFrame(frame)) return ClearPose(5, "no model-to-world frame");

    NcSdkPose pose{};
    pose.size = sizeof(pose);
    uint32_t valid = 0;
    NcSdkTransform head{};
    float view[4] = {g_VRViewWorldRot[0], g_VRViewWorldRot[1], g_VRViewWorldRot[2], g_VRViewWorldRot[3]};
    if (BoneWorld(frame, g_VRHeadBoneIdx, head) && Normalized(view)) {
        head.rot = {view[0], view[1], view[2], view[3]};
        pose.head = head;
        valid |= NCSDK_POSE_HEAD;
    }
    const bool vehicle = g_isInVehicle;
    if (!vehicle || CyberpunkVR_NightCoopVehicleHands) {
        if (BoneWorld(frame, g_VRLeftBoneIdx, pose.leftHand)) valid |= NCSDK_POSE_LEFT_HAND;
        if (BoneWorld(frame, g_VRRightBoneIdx, pose.rightHand)) valid |= NCSDK_POSE_RIGHT_HAND;
        bool anyFinger = false;
        for (int side = 0; side < 2; ++side)
            anyFinger |= HandCurls(side, pose.fingers + side * 5);
        if (anyFinger) valid |= NCSDK_POSE_FINGERS;
    } else {
        for (float& c : pose.fingers) c = -1.f;
    }
    if (MuzzleAim(now, pose.aimOrigin, pose.aimDir)) valid |= NCSDK_POSE_AIM;
    if (!vehicle) {
        const float depth = CyberpunkVR_PhysicalCrouchDepth;
        pose.physicalCrouch = std::isfinite(depth) ? std::clamp(depth / kFullSquatMetres, 0.f, 1.f) : 0.f;
        valid |= NCSDK_POSE_CROUCH;
    }
    if (!(valid & (NCSDK_POSE_HEAD | NCSDK_POSE_LEFT_HAND | NCSDK_POSE_RIGHT_HAND)))
        return ClearPose(6, "no head or hand in the snapshot");
    pose.valid = valid | NCSDK_POSE_WORLD_SPACE;
    CyberpunkVR_NightCoopGate = 0;

    const int32_t result = g_api->poseSet(g_mod, &pose);
    CyberpunkVR_NightCoopPoseResult = result;
    CyberpunkVR_NightCoopPoseBits = static_cast<int>(valid);
    if (result == NCSDK_OK) {
        ++CyberpunkVR_NightCoopPoseSets;
        g_poseOn = true;
    }
    if (result != g_lastLoggedResult || (result == NCSDK_OK && valid != g_lastLoggedBits)) {
        Log("[nightcoop] poseSet bits 0x%x%s -> %d\n", valid, vehicle ? " (in a vehicle)" : "", result);
        g_lastLoggedResult = result;
        g_lastLoggedBits = valid;
    }
}

void OnEvent(void*, int32_t event, NcSdkPeer peer) {
    const char* what = nullptr;
    switch (event) {
    case NCSDK_EV_SESSION_START: what = "session started"; break;
    case NCSDK_EV_SESSION_END: what = "session ended"; break;
    case NCSDK_EV_PEER_JOIN: what = "a player joined"; break;
    case NCSDK_EV_PEER_LEAVE: what = "a player left"; break;
    case NCSDK_EV_PEER_WORLD_IN: what = "a player is in our world"; break;
    case NCSDK_EV_PEER_WORLD_OUT: what = "a player left our world"; break;
    default: return;
    }
    Log("[nightcoop] %s (peer %u); our pose source %s\n", what, peer, g_poseOn ? "on" : "off");
}

// Partners' sources: NightCoop lays them on their bodies itself. Taken here only so they do not queue for polling.
void OnPartnerPose(void*, NcSdkPeer, const NcSdkPose*) {}

void OnShutdown(void*) {
    Log("[nightcoop] NightCoop is unloading: the bridge stops\n");
    g_api = nullptr;
    g_mod = 0;
    g_poseOn = false;
    g_gone = true;
    CyberpunkVR_NightCoopLink = -2;
}

void TryConnect(ULONGLONG now) {
    g_nextLookupMs = now + kLookupEveryMs;
    const HMODULE module = GetModuleHandleW(L"NightCoop.dll");
    if (!module) return;   // not installed, or not loaded yet
    const auto get = reinterpret_cast<NcSdkGetFn>(GetProcAddress(module, NCSDK_EXPORT_NAME));
    const NcSdkApi* api = get ? get(NCSDK_VERSION) : nullptr;
    if (!api || !NCSDK_HAS(api, registerMod) || !NCSDK_HAS(api, poseSet) || !NCSDK_HAS(api, poseClear)) {
        if (CyberpunkVR_NightCoopLink != -1)
            Log("[nightcoop] NightCoop.dll is loaded but gives no SDK table of version %u with poseSet (its SDK "
                "switched off?): the VR body stays local\n", NCSDK_VERSION);
        CyberpunkVR_NightCoopLink = -1;
        g_nextLookupMs = now + kRefusedRetryMs;
        return;
    }
    CyberpunkVR_NightCoopLink = 1;
    g_hooks = {};
    g_hooks.size = sizeof(g_hooks);
    g_hooks.frame = &OnFrame;
    g_hooks.event = &OnEvent;
    g_hooks.partnerPose = &OnPartnerPose;
    g_hooks.shutdown = &OnShutdown;
    NcSdkModInfo info{};
    info.size = sizeof(info);
    // Partners need nothing of ours to see the pose (NightCoop sends and lays it), so not NEEDED_BY_ALL.
    info.flags = NCSDK_MOD_USES_POSE;
    info.id = kModId;
    info.name = kModName;
    info.version = kModVersion;
    info.protocol = kProtocol;
    info.hooks = &g_hooks;
    const NcSdkMod mod = api->registerMod(&info);
    if (!mod) {
        Log("[nightcoop] NightCoop refused the registration of %s (see NightCoop's log): retrying in %llu s\n", kModId,
            kRefusedRetryMs / 1000);
        CyberpunkVR_NightCoopLink = -1;
        g_nextLookupMs = now + kRefusedRetryMs;
        return;
    }
    g_api = api;
    g_mod = mod;
    CyberpunkVR_NightCoopLink = 2;
    Log("[nightcoop] registered as %s %s (SDK table %u bytes): the VR head, hands, fingers, aim and crouch go to the "
        "partners as NightCoop's pose source\n", kModId, kModVersion, api->size);
    ModLog(NCSDK_LOG_INFO, "VR body: head, hands (solved rig), fingers, muzzle aim and physical crouch as the pose source");
}

} // namespace

bool Update(RED4ext::CGameApplication*) {
    if (g_api || g_gone) return false;
    const ULONGLONG now = GetTickCount64();
    if (now >= g_nextLookupMs) TryConnect(now);
    return false;
}

void Shutdown() {
    if (!g_api || !g_mod) return;
    if (g_poseOn && NCSDK_HAS(g_api, poseClear)) g_api->poseClear(g_mod);
    if (NCSDK_HAS(g_api, unregisterMod)) g_api->unregisterMod(g_mod);
    g_poseOn = false;
    g_api = nullptr;
    g_mod = 0;
}

} // namespace cvr::nightcoop
