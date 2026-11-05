#include "Core/Paths.hpp"
#include "Fbt/Game/Trackers.hpp"
#include "Fbt/Game/Calibration.hpp"
#include "Anim/CharacterRig.hpp"
#include <string>
#include <Windows.h>
#include <atomic>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <mutex>

extern void Log(const char*, ...);

namespace cvr::trackers {
namespace {
constexpr const char* kRolePath[kRoleCount] = {
    "/user/vive_tracker_htcx/role/waist",
    "/user/vive_tracker_htcx/role/left_foot",
    "/user/vive_tracker_htcx/role/right_foot",
};
constexpr const char* kBindingPath[kRoleCount] = {
    "/user/vive_tracker_htcx/role/waist/input/grip/pose",
    "/user/vive_tracker_htcx/role/left_foot/input/grip/pose",
    "/user/vive_tracker_htcx/role/right_foot/input/grip/pose",
};
constexpr const char* kRoleName[kRoleCount] = {"waist", "left foot", "right foot"};
constexpr const char* kProfile = "/interaction_profiles/htc/vive_tracker_htcx";

bool g_enabled = false, g_available = false;
XrInstance g_instance = XR_NULL_HANDLE;
XrSession g_session = XR_NULL_HANDLE;
PFN_xrEnumerateViveTrackerPathsHTCX pEnumerate = nullptr;
XrPath g_rolePaths[kRoleCount]{};
XrAction g_poseAction = XR_NULL_HANDLE;
XrSpace g_spaces[kRoleCount]{};
XrSpace g_stage = XR_NULL_HANDLE;
XrPosef g_headStage{{0, 0, 0, 1}, {0, 0, 0}};
bool g_headStageValid = false;
std::atomic<unsigned long long> g_locateCount{0};

std::mutex g_poseMutex;
Pose g_poses[kRoleCount];

// Log state, touched only by Locate (the XR thread).
ULONGLONG g_lastEnumerate = 0, g_lastReport = 0;
int g_validSamples[kRoleCount]{}, g_samples[kRoleCount]{}, g_dropouts[kRoleCount]{};
bool g_wasValid[kRoleCount]{};
uint32_t g_lastCount = UINT32_MAX;

// One number from vrport.ini ("key=value" or "key = value"); fallback when missing.
float ReadIni(const char* key, float fallback) {
    const std::string path = cvr::paths::FileA(cvr::paths::Folder::Config, "vrport.ini");
    FILE* f = nullptr;
    if (fopen_s(&f, path.c_str(), "r") != 0 || !f) return fallback;
    char line[256], a[96], b[96];
    sprintf_s(a, "%s=%%f", key); sprintf_s(b, "%s = %%f", key);
    float value = fallback;
    while (fgets(line, sizeof(line), f)) {
        float v = 0;
        if (sscanf_s(line, a, &v) == 1 || sscanf_s(line, b, &v) == 1) { value = v; break; }
    }
    fclose(f);
    return value;
}
int ReadSetting() { return ReadIni("xr_vive_trackers", 0.0f) == 1.0f ? 1 : 0; }

XrQuaternionf Conj(const XrQuaternionf& q) { return {-q.x, -q.y, -q.z, q.w}; }
XrQuaternionf Mul(const XrQuaternionf& a, const XrQuaternionf& b) {
    return {a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y,
            a.w * b.y - a.x * b.z + a.y * b.w + a.z * b.x,
            a.w * b.z + a.x * b.y - a.y * b.x + a.z * b.w,
            a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z};
}
XrVector3f Rotate(const XrQuaternionf& q, const XrVector3f& v) {
    const XrQuaternionf r = Mul(Mul(q, {v.x, v.y, v.z, 0}), Conj(q));
    return {r.x, r.y, r.z};
}


// The controllers' grip spaces (from OpenXRManager) and their poses, as the trackers'.
XrSpace g_handSpaces[2]{};
Pose g_hands[2];


// Lists every tracker the runtime knows and its role; logs only when the count changes (trackers switched on or off,
// roles reassigned in SteamVR).
void Enumerate() {
    if (!pEnumerate) return;
    uint32_t count = 0;
    if (XR_FAILED(pEnumerate(g_instance, 0, &count, nullptr))) return;
    if (count == g_lastCount) return;
    g_lastCount = count;
    std::vector<XrViveTrackerPathsHTCX> paths(count, {XR_TYPE_VIVE_TRACKER_PATHS_HTCX});
    if (count && XR_FAILED(pEnumerate(g_instance, count, &count, paths.data()))) return;
    Log("[trackers] %u tracker(s) known to the runtime\n", count);
    for (const auto& p : paths) {
        char persistent[XR_MAX_PATH_LENGTH]{}, role[XR_MAX_PATH_LENGTH]{};
        uint32_t n = 0;
        if (p.persistentPath == XR_NULL_PATH || XR_FAILED(xrPathToString(g_instance, p.persistentPath, sizeof(persistent), &n, persistent)))
            strcpy_s(persistent, "?");
        if (p.rolePath == XR_NULL_PATH) strcpy_s(role, "(no role)");
        else if (XR_FAILED(xrPathToString(g_instance, p.rolePath, sizeof(role), &n, role))) strcpy_s(role, "?");
        Log("[trackers]   %s role=%s\n", persistent, role);
    }
}
}

void ChooseExtensions(const std::vector<XrExtensionProperties>& available, std::vector<const char*>& enabled) {
    g_enabled = ReadSetting() == 1;
    if (!g_enabled) return;
    for (const auto& p : available)
        if (std::strcmp(p.extensionName, XR_HTCX_VIVE_TRACKER_INTERACTION_EXTENSION_NAME) == 0) g_available = true;
    if (g_available) enabled.push_back(XR_HTCX_VIVE_TRACKER_INTERACTION_EXTENSION_NAME);
    Log("[trackers] xr_vive_trackers=1: %s %s\n", XR_HTCX_VIVE_TRACKER_INTERACTION_EXTENSION_NAME,
        g_available ? "available" : "NOT available (no trackers)");
}

void OnInstanceCreated(XrInstance instance) {
    if (!g_enabled || !g_available) return;
    g_instance = instance;
    xrGetInstanceProcAddr(instance, "xrEnumerateViveTrackerPathsHTCX", reinterpret_cast<PFN_xrVoidFunction*>(&pEnumerate));
    if (!pEnumerate) { Log("[trackers] xrEnumerateViveTrackerPathsHTCX missing\n"); g_available = false; return; }
    Enumerate();
}

void CreateActions(XrInstance instance, XrActionSet actionSet) {
    if (!g_enabled || !g_available || actionSet == XR_NULL_HANDLE) return;
    for (int r = 0; r < kRoleCount; ++r)
        if (XR_FAILED(xrStringToPath(instance, kRolePath[r], &g_rolePaths[r]))) { Log("[trackers] bad role path %s\n", kRolePath[r]); return; }
    XrActionCreateInfo info{XR_TYPE_ACTION_CREATE_INFO};
    info.actionType = XR_ACTION_TYPE_POSE_INPUT;
    strcpy_s(info.actionName, "vive_tracker_pose");
    strcpy_s(info.localizedActionName, "Vive Tracker Pose");
    info.countSubactionPaths = kRoleCount;
    info.subactionPaths = g_rolePaths;
    if (const XrResult r = xrCreateAction(actionSet, &info, &g_poseAction); XR_FAILED(r)) {
        Log("[trackers] pose action not created (%d)\n", static_cast<int>(r));
        g_poseAction = XR_NULL_HANDLE;
        return;
    }
    XrPath profile = XR_NULL_PATH;
    if (XR_FAILED(xrStringToPath(instance, kProfile, &profile))) return;
    XrActionSuggestedBinding bindings[kRoleCount]{};
    uint32_t count = 0;
    for (int r = 0; r < kRoleCount; ++r) {
        XrPath p = XR_NULL_PATH;
        if (XR_SUCCEEDED(xrStringToPath(instance, kBindingPath[r], &p))) bindings[count++] = {g_poseAction, p};
    }
    XrInteractionProfileSuggestedBinding sb{XR_TYPE_INTERACTION_PROFILE_SUGGESTED_BINDING};
    sb.interactionProfile = profile;
    sb.suggestedBindings = bindings;
    sb.countSuggestedBindings = count;
    Log("[trackers] suggest bindings %s -> %d (count=%u)\n", kProfile, static_cast<int>(xrSuggestInteractionProfileBindings(instance, &sb)), count);
}

void OnSessionCreated(XrSession session, const XrSpace handSpaces[2]) {
    if (g_poseAction == XR_NULL_HANDLE) return;
    g_session = session;
    g_handSpaces[0] = handSpaces ? handSpaces[0] : XR_NULL_HANDLE;   // owned by OpenXRManager
    g_handSpaces[1] = handSpaces ? handSpaces[1] : XR_NULL_HANDLE;
    XrReferenceSpaceCreateInfo stageInfo{XR_TYPE_REFERENCE_SPACE_CREATE_INFO};
    stageInfo.referenceSpaceType = XR_REFERENCE_SPACE_TYPE_STAGE;
    stageInfo.poseInReferenceSpace.orientation.w = 1.0f;
    if (const XrResult res = xrCreateReferenceSpace(session, &stageInfo, &g_stage); XR_FAILED(res)) {
        Log("[trackers] STAGE space not created (%d)\n", static_cast<int>(res));
        g_stage = XR_NULL_HANDLE;
    } else {
        XrExtent2Df bounds{};
        const XrResult b = xrGetReferenceSpaceBoundsRect(session, XR_REFERENCE_SPACE_TYPE_STAGE, &bounds);
        Log("[trackers] STAGE space created; play area %.2f x %.2f m (%d)\n", bounds.width, bounds.height, static_cast<int>(b));
    }
    for (int r = 0; r < kRoleCount; ++r) {
        XrActionSpaceCreateInfo ci{XR_TYPE_ACTION_SPACE_CREATE_INFO};
        ci.action = g_poseAction;
        ci.subactionPath = g_rolePaths[r];
        ci.poseInActionSpace.orientation.w = 1.0f;
        if (const XrResult res = xrCreateActionSpace(session, &ci, &g_spaces[r]); XR_FAILED(res)) {
            Log("[trackers] %s space not created (%d)\n", kRoleName[r], static_cast<int>(res));
            g_spaces[r] = XR_NULL_HANDLE;
        }
    }
}

void OnSessionDestroying() {
    for (auto& s : g_spaces) if (s != XR_NULL_HANDLE) { xrDestroySpace(s); s = XR_NULL_HANDLE; }
    if (g_stage != XR_NULL_HANDLE) { xrDestroySpace(g_stage); g_stage = XR_NULL_HANDLE; }
    g_session = XR_NULL_HANDLE;
    g_handSpaces[0] = g_handSpaces[1] = XR_NULL_HANDLE;
    cvr::fbt::game::CancelCalibration();   // a session ending mid-calibration
    std::lock_guard lock(g_poseMutex);
    for (auto& p : g_poses) p = {};
    for (auto& h : g_hands) h = {};
}

void Locate(XrSpace base, XrSpace view, XrTime time, const XrPosef& head) {
    if (g_session == XR_NULL_HANDLE) return;
    const ULONGLONG now = GetTickCount64();
    if (now - g_lastEnumerate >= 2000) { g_lastEnumerate = now; Enumerate(); }

    const XrQuaternionf headInv = Conj(head.orientation);
    Pose poses[kRoleCount];
    bool active[kRoleCount]{};
    XrSpaceLocationFlags flags[kRoleCount]{};
    for (int r = 0; r < kRoleCount; ++r) {
        if (g_spaces[r] == XR_NULL_HANDLE) continue;
        XrActionStateGetInfo getInfo{XR_TYPE_ACTION_STATE_GET_INFO};
        getInfo.action = g_poseAction;
        getInfo.subactionPath = g_rolePaths[r];
        XrActionStatePose state{XR_TYPE_ACTION_STATE_POSE};
        active[r] = XR_SUCCEEDED(xrGetActionStatePose(g_session, &getInfo, &state)) && state.isActive;
        if (active[r]) {
            XrSpaceLocation loc{XR_TYPE_SPACE_LOCATION};
            constexpr XrSpaceLocationFlags kNeed = XR_SPACE_LOCATION_POSITION_VALID_BIT | XR_SPACE_LOCATION_ORIENTATION_VALID_BIT;
            if (XR_SUCCEEDED(xrLocateSpace(g_spaces[r], base, time, &loc))) {
                flags[r] = loc.locationFlags;
                if ((loc.locationFlags & kNeed) == kNeed) {
                    poses[r].valid = true;
                    poses[r].pos = Rotate(headInv, {loc.pose.position.x - head.position.x, loc.pose.position.y - head.position.y,
                                                    loc.pose.position.z - head.position.z});
                    poses[r].ori = Mul(headInv, loc.pose.orientation);
                }
            }
            XrSpaceLocation stageLoc{XR_TYPE_SPACE_LOCATION};
            if (g_stage != XR_NULL_HANDLE && XR_SUCCEEDED(xrLocateSpace(g_spaces[r], g_stage, time, &stageLoc)) &&
                (stageLoc.locationFlags & kNeed) == kNeed) {
                poses[r].stageValid = true;
                poses[r].stage = stageLoc.pose;
            }
        }
        ++g_samples[r];
        if (poses[r].valid) ++g_validSamples[r];
        else if (g_wasValid[r]) ++g_dropouts[r];
        g_wasValid[r] = poses[r].valid;
    }
    // The controllers (grip poses), the same way: head-relative for the helpers, STAGE for the calibration.
    Pose hands[2];
    for (int h = 0; h < 2; ++h) {
        if (g_handSpaces[h] == XR_NULL_HANDLE) continue;
        constexpr XrSpaceLocationFlags kNeed = XR_SPACE_LOCATION_POSITION_VALID_BIT | XR_SPACE_LOCATION_ORIENTATION_VALID_BIT;
        XrSpaceLocation loc{XR_TYPE_SPACE_LOCATION};
        if (XR_SUCCEEDED(xrLocateSpace(g_handSpaces[h], base, time, &loc)) && (loc.locationFlags & kNeed) == kNeed) {
            hands[h].valid = true;
            hands[h].pos = Rotate(headInv, {loc.pose.position.x - head.position.x, loc.pose.position.y - head.position.y,
                                            loc.pose.position.z - head.position.z});
            hands[h].ori = Mul(headInv, loc.pose.orientation);
        }
        XrSpaceLocation stageLoc{XR_TYPE_SPACE_LOCATION};
        if (g_stage != XR_NULL_HANDLE && XR_SUCCEEDED(xrLocateSpace(g_handSpaces[h], g_stage, time, &stageLoc)) &&
            (stageLoc.locationFlags & kNeed) == kNeed) {
            hands[h].stageValid = true;
            hands[h].stage = stageLoc.pose;
        }
    }
    XrSpaceLocation headLoc{XR_TYPE_SPACE_LOCATION};
    constexpr XrSpaceLocationFlags kNeedHead = XR_SPACE_LOCATION_POSITION_VALID_BIT | XR_SPACE_LOCATION_ORIENTATION_VALID_BIT;
    const bool headOk = g_stage != XR_NULL_HANDLE && view != XR_NULL_HANDLE && XR_SUCCEEDED(xrLocateSpace(view, g_stage, time, &headLoc)) &&
                        (headLoc.locationFlags & kNeedHead) == kNeedHead;
    // THE HEAD THE BODY IS MEASURED FROM MUST BE THE HEAD IT IS PLACED ON -- the port's rule for the hands
    // (Runtimes/OpenXRFrameLoop.cpp, headForHands). The body is placed on the camera, which follows the FILTERED head;
    // measured from the raw located head, the waist's offset carried the difference of the two, and V's hips swung by it
    // whenever the head moved (in game: a wobble of the waist and the chest like a bubble, the waist itself still).
    // So the head published in STAGE is the head given here (`head`, the filtered one in `base`), taken into STAGE by
    // the base -> STAGE relation of this same instant (the raw headset located in both).
    XrPosef headStage = headLoc.pose;
    if (headOk) {
        XrSpaceLocation inBase{XR_TYPE_SPACE_LOCATION};
        if (XR_SUCCEEDED(xrLocateSpace(view, base, time, &inBase)) && (inBase.locationFlags & kNeedHead) == kNeedHead) {
            // baseToStage = rawStage * inverse(rawBase); headStage = baseToStage * head
            const XrQuaternionf q = Mul(headLoc.pose.orientation, Conj(inBase.pose.orientation));
            const XrVector3f rb = Rotate(q, inBase.pose.position);
            const XrVector3f t{headLoc.pose.position.x - rb.x, headLoc.pose.position.y - rb.y, headLoc.pose.position.z - rb.z};
            const XrVector3f hp = Rotate(q, head.position);
            headStage.position = {t.x + hp.x, t.y + hp.y, t.z + hp.z};
            headStage.orientation = Mul(q, head.orientation);
        }
    }
    {
        std::lock_guard lock(g_poseMutex);
        for (int r = 0; r < kRoleCount; ++r) g_poses[r] = poses[r];
        g_headStageValid = headOk;
        if (headOk) g_headStage = headStage;
        g_locateCount.fetch_add(1, std::memory_order_relaxed);
        g_hands[0] = hands[0];
        g_hands[1] = hands[1];
    }
    if (cvr::fbt::game::CalibrationStep() != 0) cvr::fbt::game::CalibrationTick(view, g_stage, time, poses, hands);

    if (now - g_lastReport < 1000) return;
    const float seconds = g_lastReport ? float(now - g_lastReport) / 1000.0f : 1.0f;
    g_lastReport = now;
    if (g_stage != XR_NULL_HANDLE) {
        // The head in STAGE (y = eye height above SteamVR's floor) and LOCAL in STAGE: the change of base between the
        // port's space and the floor centre. Its tilt should be 0 and its values constant until the runtime moves a space.
        XrSpaceLocation headStage{XR_TYPE_SPACE_LOCATION}, localStage{XR_TYPE_SPACE_LOCATION};
        const bool headOk = view != XR_NULL_HANDLE && XR_SUCCEEDED(xrLocateSpace(view, g_stage, time, &headStage)) &&
                            (headStage.locationFlags & XR_SPACE_LOCATION_POSITION_VALID_BIT);
        const bool localOk = XR_SUCCEEDED(xrLocateSpace(base, g_stage, time, &localStage)) &&
                             (localStage.locationFlags & XR_SPACE_LOCATION_ORIENTATION_VALID_BIT);
        const XrQuaternionf& q = localStage.pose.orientation;
        const XrVector3f up = Rotate(q, {0, 1, 0}), fwd = Rotate(q, {0, 0, -1});
        const float tiltDeg = std::acos(up.y > 1.0f ? 1.0f : (up.y < -1.0f ? -1.0f : up.y)) * 57.29578f;
        const float yawDeg = std::atan2(-fwd.x, -fwd.z) * 57.29578f;
        Log("[trackers] head stage=(%+.3f %+.3f %+.3f)%s | LOCAL in STAGE pos=(%+.3f %+.3f %+.3f) yaw=%+.1f tilt=%.2f deg%s\n",
            headStage.pose.position.x, headStage.pose.position.y, headStage.pose.position.z, headOk ? "" : " (invalid)",
            localStage.pose.position.x, localStage.pose.position.y, localStage.pose.position.z, yawDeg, tiltDeg,
            localOk ? "" : " (invalid)");
    }
    for (int r = 0; r < kRoleCount; ++r) {
        const Pose& p = poses[r];
        Log("[trackers] %-10s active=%d flags=0x%02X valid=%.0f/s of %.0f/s drops=%d rel=(%+.3f %+.3f %+.3f) "
            "ori=(%+.3f %+.3f %+.3f %+.3f) stage=(%+.3f %+.3f %+.3f)\n",
            kRoleName[r], active[r] ? 1 : 0, static_cast<unsigned>(flags[r]), g_validSamples[r] / seconds, g_samples[r] / seconds,
            g_dropouts[r], p.pos.x, p.pos.y, p.pos.z, p.ori.x, p.ori.y, p.ori.z, p.ori.w, p.stage.position.x, p.stage.position.y,
            p.stage.position.z);
        g_samples[r] = g_validSamples[r] = g_dropouts[r] = 0;
    }
}

bool Running() { return g_session != XR_NULL_HANDLE; }

bool ReadHand(int hand, Pose* out) {
    if (!out || hand < 0 || hand > 1) return false;
    std::lock_guard lock(g_poseMutex);
    *out = g_hands[hand];
    return out->valid;
}


unsigned long long LocateCount() { return g_locateCount.load(std::memory_order_relaxed); }

bool ReadHeadStage(XrPosef* out) {
    if (!out) return false;
    std::lock_guard lock(g_poseMutex);
    *out = g_headStage;
    return g_headStageValid;
}

bool Read(Role role, Pose* out) {
    if (!out || role < 0 || role >= kRoleCount) return false;
    std::lock_guard lock(g_poseMutex);
    *out = g_poses[role];
    return out->valid;
}

}
