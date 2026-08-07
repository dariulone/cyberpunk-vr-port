// openxr_vive_trackers.cpp - body tracking via XR_HTCX_vive_tracker_interaction
// (feet + waist).
//
// SteamVR exposes every connected Vive Tracker with an assigned role through
// this extension (SteamVR -> Settings -> Controllers -> "Manage Vive Trackers";
// assign LEFT FOOT / RIGHT FOOT / WAIST there). Panda trackers show up in
// that same panel as Vive trackers and work identically. We:
//   1. enable the extension at instance creation (openxr_manager.cpp Init),
//   2. bind the body roles to one pose action (CreateViveTrackerActions),
//   3. create an action space per role (EnsureViveTrackerSpaces),
//   4. locate all roles next to the hand locate each frame and store them in
//      the SAME HMD-local convention as m_hands (PollViveTrackers),
//   5. publish them to shared slots [157..172] (feet) / [181..188] (waist)
//      inside the hands seqlock (FlushHandsToShared in openxr_manager.cpp)
//      for the VRIK plugin.
//
// The extension and all its types exist in the pinned OpenXR SDK (1.0.34);
// everything here no-ops cleanly on runtimes without it (m_viveTrackerExt stays
// false and the plugin keeps its animation-driven legs/hips).
#include "openxr_manager.h"
#include "openxr_internal.h"
#include <cstring>
#include <vector>

namespace {
    // Role-path and interaction-profile strings from the
    // XR_HTCX_vive_tracker_interaction spec. Index convention matches
    // OpenXRManager::kBodyTrackerCount: [0] = left foot, [1] = right foot,
    // [2] = waist.
    constexpr const char* kRoleStrings[3] = {
        "/user/vive_tracker_htcx/role/left_foot",
        "/user/vive_tracker_htcx/role/right_foot",
        "/user/vive_tracker_htcx/role/waist",
    };
    constexpr const char* kBindingStrings[3] = {
        "/user/vive_tracker_htcx/role/left_foot/input/grip/pose",
        "/user/vive_tracker_htcx/role/right_foot/input/grip/pose",
        "/user/vive_tracker_htcx/role/waist/input/grip/pose",
    };
    constexpr const char* kViveTrackerProfile =
        "/interaction_profiles/htc/vive_tracker_htcx";
    constexpr const char* kSideName[3] = { "L foot", "R foot", "waist" };
} // namespace

void OpenXRManager::InitViveTrackerSupport() {
    // Called from Init() right after xrCreateInstance. The extension was enabled
    // there iff the runtime advertised it; resolve the enumerate entry point and
    // log what is actually connected so support questions answer themselves.
    m_viveTrackerExt = false;
    m_pfnEnumViveTrackers = nullptr;

#ifdef XR_HTCX_vive_tracker_interaction
    PFN_xrVoidFunction pfn = nullptr;
    if (XR_SUCCEEDED(xrGetInstanceProcAddr(m_instance, "xrEnumerateViveTrackerPathsHTCX", &pfn)) && pfn) {
        m_pfnEnumViveTrackers = reinterpret_cast<PFN_xrEnumerateViveTrackerPathsHTCX>(pfn);
        m_viveTrackerExt = true;
    }
#endif
    if (!m_viveTrackerExt) {
        Log("OpenXRManager[FBT]: XR_HTCX_vive_tracker_interaction unavailable on this runtime -- body trackers disabled.\n");
        return;
    }

    // xrEnumerateViveTrackerPathsHTCX is the standard capacity idiom --
    // (instance, capacity, countOut, paths); there is NO systemId argument.
    uint32_t count = 0;
    XrResult res = m_pfnEnumViveTrackers(m_instance, 0, &count, nullptr);
    if (XR_FAILED(res) || count == 0) {
        Log("OpenXRManager[FBT]: extension present, no trackers connected (count=%u res=%d). "
            "Assign roles in SteamVR > Controllers > Manage Vive Trackers.\n", count, res);
        return;
    }
    std::vector<XrViveTrackerPathsHTCX> trackers(count, { XR_TYPE_VIVE_TRACKER_PATHS_HTCX });
    res = m_pfnEnumViveTrackers(m_instance, count, &count, trackers.data());
    if (XR_FAILED(res)) {
        Log("OpenXRManager[FBT]: xrEnumerateViveTrackerPathsHTCX failed res=%d\n", res);
        return;
    }
    int bodyRoles = 0;
    for (const auto& t : trackers) {
        char roleStr[128] = { 0 };
        uint32_t n = 0;
        if (XR_SUCCEEDED(xrPathToString(m_instance, t.rolePath, sizeof(roleStr), &n, roleStr))) {
            Log("OpenXRManager[FBT]: tracker role=%s\n", roleStr);
            for (int i = 0; i < kBodyTrackerCount; ++i)
                if (strcmp(roleStr, kRoleStrings[i]) == 0) ++bodyRoles;
        }
    }
    m_viveTrackerCount.store(bodyRoles > kBodyTrackerCount ? kBodyTrackerCount : bodyRoles, std::memory_order_relaxed);
    Log("OpenXRManager[FBT]: %u tracker(s) connected, %d body role(s) assigned (feet/waist).\n", count, bodyRoles);
}

void OpenXRManager::CreateViveTrackerActions() {
    // Called inside the action-set setup block (m_actionSet already exists).
    // One pose action with the body role paths as subactions, suggested
    // against the vive_tracker_htcx interaction profile. Suggesting bindings for
    // a role with no tracker connected is legal and harmless -- the action state
    // simply never goes active for that subaction.
    if (!m_viveTrackerExt || m_actionSet == XR_NULL_HANDLE) return;
#ifdef XR_HTCX_vive_tracker_interaction
    for (int i = 0; i < kBodyTrackerCount; ++i) {
        if (XR_FAILED(xrStringToPath(m_instance, kRoleStrings[i], &m_trackerRolePaths[i]))) {
            Log("OpenXRManager[FBT]: xrStringToPath failed for %s -- body trackers off.\n", kRoleStrings[i]);
            return;
        }
    }

    XrActionCreateInfo info{ XR_TYPE_ACTION_CREATE_INFO };
    info.actionType = XR_ACTION_TYPE_POSE_INPUT;
    strcpy_s(info.actionName, "vive_tracker_pose");
    strcpy_s(info.localizedActionName, "Vive Tracker Pose");
    info.countSubactionPaths = kBodyTrackerCount;
    info.subactionPaths = m_trackerRolePaths;
    XrResult res = xrCreateAction(m_actionSet, &info, &m_trackerPoseAction);
    if (XR_FAILED(res)) {
        Log("OpenXRManager[FBT]: xrCreateAction failed res=%d -- body trackers off.\n", res);
        m_trackerPoseAction = XR_NULL_HANDLE;
        return;
    }

    XrPath profile = XR_NULL_PATH;
    if (XR_SUCCEEDED(xrStringToPath(m_instance, kViveTrackerProfile, &profile))) {
        XrActionSuggestedBinding bindings[kBodyTrackerCount]{};
        for (int i = 0; i < kBodyTrackerCount; ++i) {
            XrPath p = XR_NULL_PATH;
            if (XR_SUCCEEDED(xrStringToPath(m_instance, kBindingStrings[i], &p))) {
                bindings[i] = { m_trackerPoseAction, p };
            }
        }
        XrInteractionProfileSuggestedBinding sb{ XR_TYPE_INTERACTION_PROFILE_SUGGESTED_BINDING };
        sb.interactionProfile = profile;
        sb.suggestedBindings = bindings;
        sb.countSuggestedBindings = kBodyTrackerCount;
        res = xrSuggestInteractionProfileBindings(m_instance, &sb);
        Log("OpenXRManager[FBT]: suggest bindings %s -> res=%d\n", kViveTrackerProfile, res);
    }
#endif
}

void OpenXRManager::EnsureViveTrackerSpaces() {
    // Action spaces per body role. Safe to call repeatedly and legal while the
    // session is running, so a tracker turned on mid-game gets its space on the
    // next poll without a restart.
    if (!m_viveTrackerExt || m_trackerPoseAction == XR_NULL_HANDLE ||
        m_session == XR_NULL_HANDLE) return;
    for (int i = 0; i < kBodyTrackerCount; ++i) {
        if (m_trackerSpaces[i] != XR_NULL_HANDLE) continue;
        XrActionSpaceCreateInfo spaceInfo{ XR_TYPE_ACTION_SPACE_CREATE_INFO };
        spaceInfo.action = m_trackerPoseAction;
        spaceInfo.subactionPath = m_trackerRolePaths[i];
        spaceInfo.poseInActionSpace.orientation.w = 1.0f;
        XrResult res = xrCreateActionSpace(m_session, &spaceInfo, &m_trackerSpaces[i]);
        if (XR_FAILED(res)) {
            m_trackerSpaces[i] = XR_NULL_HANDLE;
            Log("OpenXRManager[FBT]: xrCreateActionSpace %s failed res=%d\n", kSideName[i], res);
        } else {
            Log("OpenXRManager[FBT]: tracker space created (%s).\n", kSideName[i]);
        }
    }
}

void OpenXRManager::PollViveTrackers(XrTime locateTime, const XrPosef& headPoseLocal) {
    // CALLER HOLDS m_handMutex (this writes m_trackers, the m_hands sibling).
    if (m_viveTrackerExt && m_trackerPoseAction != XR_NULL_HANDLE) {

    // Re-enumerate occasionally: trackers power on/off mid-session, and SteamVR
    // lets the user assign roles at runtime. Cheap count-only probe; the full
    // vector fetch only runs when the count changed.
    const double now = FbtNowSeconds();
    if (now - m_lastViveTrackerEnum > 2.0) {
        m_lastViveTrackerEnum = now;
        uint32_t count = 0;
        if (m_pfnEnumViveTrackers && XR_SUCCEEDED(m_pfnEnumViveTrackers(m_instance, 0, &count, nullptr)) && count > 0) {
            std::vector<XrViveTrackerPathsHTCX> trackers(count, { XR_TYPE_VIVE_TRACKER_PATHS_HTCX });
            if (XR_SUCCEEDED(m_pfnEnumViveTrackers(m_instance, count, &count, trackers.data()))) {
                int bodyRoles = 0;
                for (const auto& t : trackers) {
                    char roleStr[128] = { 0 };
                    uint32_t n = 0;
                    if (XR_SUCCEEDED(xrPathToString(m_instance, t.rolePath, sizeof(roleStr), &n, roleStr)))
                        for (int i = 0; i < kBodyTrackerCount; ++i)
                            if (strcmp(roleStr, kRoleStrings[i]) == 0) ++bodyRoles;
                }
                const int clamped = bodyRoles > kBodyTrackerCount ? kBodyTrackerCount : bodyRoles;
                if (clamped != m_viveTrackerCount.load(std::memory_order_relaxed)) {
                    m_viveTrackerCount.store(clamped, std::memory_order_relaxed);
                    Log("OpenXRManager[FBT]: body-role tracker count changed -> %d\n", clamped);
                }
            }
        }
    }

    EnsureViveTrackerSpaces();

    const float smooth = GetHandTrackingSmooth();
    const XrQuaternionf headInv = ConjugateQuat(headPoseLocal.orientation);
    for (int i = 0; i < kBodyTrackerCount; ++i) {
        m_trackers[i].valid = false;
        m_trackerStageValid[i] = false;
        if (m_trackerSpaces[i] == XR_NULL_HANDLE) continue;

        XrActionStateGetInfo getInfo{ XR_TYPE_ACTION_STATE_GET_INFO };
        getInfo.action = m_trackerPoseAction;
        getInfo.subactionPath = m_trackerRolePaths[i];
        XrActionStatePose poseState{ XR_TYPE_ACTION_STATE_POSE };
        if (XR_FAILED(xrGetActionStatePose(m_session, &getInfo, &poseState)) || !poseState.isActive) {
            m_trackerFilterState[i].initialized = false;
            continue;
        }

        XrSpaceLocation loc{ XR_TYPE_SPACE_LOCATION };
        if (XR_FAILED(xrLocateSpace(m_trackerSpaces[i], m_localSpace, locateTime, &loc)) ||
            !(loc.locationFlags & XR_SPACE_LOCATION_POSITION_VALID_BIT) ||
            !(loc.locationFlags & XR_SPACE_LOCATION_ORIENTATION_VALID_BIT)) {
            m_trackerFilterState[i].initialized = false;
            continue;
        }

        // Stage-space copy for calibration (floor-relative ankle/hip height)...
        m_trackerStagePos[i] = loc.pose.position;
        m_trackerStageValid[i] = true;

        // ...and the HMD-local form, IDENTICAL to the hand pipeline: rel =
        // conj(head) * (tracker - head), then the same adaptive jitter filter.
        // Keeping the exact hand convention means the plugin can drop the pose
        // into the same gizmo-exact frame the arms already solve in.
        XrVector3f relWorld{
            loc.pose.position.x - headPoseLocal.position.x,
            loc.pose.position.y - headPoseLocal.position.y,
            loc.pose.position.z - headPoseLocal.position.z,
        };
        XrPosef relPose{};
        relPose.position = RotateVector(headInv, relWorld);
        relPose.orientation = MultiplyQuat(headInv, loc.pose.orientation);
        XrPosef filtered = FilterTrackerPose(m_trackerFilterState[i], relPose, smooth);

        m_trackers[i].posX = filtered.position.x;
        m_trackers[i].posY = filtered.position.y;
        m_trackers[i].posZ = filtered.position.z;
        m_trackers[i].oriX = filtered.orientation.x;
        m_trackers[i].oriY = filtered.orientation.y;
        m_trackers[i].oriZ = filtered.orientation.z;
        m_trackers[i].oriW = filtered.orientation.w;
        m_trackers[i].valid = true;
    }
    } // HTCX provider block

    // OpenVR fallback provider (Panda trackers and friends): fills every role
    // the HTCX path left invalid this frame. No-op when openvr_api.dll / the
    // OpenVR client is unavailable.
    PollOpenVRTrackers(headPoseLocal);
}

bool OpenXRManager::GetBodyTrackerPose(int idx, OpenXRHeadPose* out) const {
    if (!out || idx < 0 || idx >= kBodyTrackerCount) return false;
    std::lock_guard<std::mutex> lock(const_cast<std::mutex&>(m_handMutex));
    *out = m_trackers[idx];
    return out->valid;
}
