#include "Runtimes/HandTracking.hpp"
#include "Anim/HandGestures.hpp"
#include "Core/LiveControls.hpp"
#include <cmath>
#include <cstring>

extern void Log(const char* fmt, ...);

namespace cvr::handtracking {
namespace {
bool g_tracking{}, g_source{}, g_range{};
PFN_xrCreateHandTrackerEXT g_create{};
PFN_xrLocateHandJointsEXT g_locate{};
XrSession g_session{};
XrHandTrackerEXT g_trackers[2]{};
bool g_ready{};
int g_loggedSource[2]{-2, -2};

constexpr int kFirst[5] = {XR_HAND_JOINT_THUMB_METACARPAL_EXT, XR_HAND_JOINT_INDEX_METACARPAL_EXT,
                           XR_HAND_JOINT_MIDDLE_METACARPAL_EXT, XR_HAND_JOINT_RING_METACARPAL_EXT,
                           XR_HAND_JOINT_LITTLE_METACARPAL_EXT};
constexpr int kLast[5] = {XR_HAND_JOINT_THUMB_TIP_EXT, XR_HAND_JOINT_INDEX_TIP_EXT, XR_HAND_JOINT_MIDDLE_TIP_EXT,
                          XR_HAND_JOINT_RING_TIP_EXT, XR_HAND_JOINT_LITTLE_TIP_EXT};

// The finger's bend: the angles between its bones, added up (degrees). -1 when a joint has no valid position.
float Bend(const XrHandJointLocationEXT* joints, int first, int last) {
    float total = 0, previous[3]{};
    for (int j = first; j < last; ++j) {
        if (!(joints[j].locationFlags & XR_SPACE_LOCATION_POSITION_VALID_BIT) ||
            !(joints[j + 1].locationFlags & XR_SPACE_LOCATION_POSITION_VALID_BIT)) return -1;
        const auto& a = joints[j].pose.position;
        const auto& b = joints[j + 1].pose.position;
        float bone[3] = {b.x - a.x, b.y - a.y, b.z - a.z};
        const float length = std::sqrt(bone[0] * bone[0] + bone[1] * bone[1] + bone[2] * bone[2]);
        if (length < 1e-5f) return -1;
        for (float& v : bone) v /= length;
        if (j > first) {
            const float dot = previous[0] * bone[0] + previous[1] * bone[1] + previous[2] * bone[2];
            total += std::acos(dot < -1 ? -1 : (dot > 1 ? 1 : dot)) * 57.29578f;
        }
        std::memcpy(previous, bone, sizeof(bone));
    }
    return total;
}

bool Prepare(XrInstance instance, XrSystemId system, XrSession session) {
    if (g_session == session) return g_ready;
    g_session = session;
    g_ready = false;
    if (!g_tracking) {
        Log("[handtracking] the runtime does not offer XR_EXT_hand_tracking\n");
        return false;
    }
    XrSystemHandTrackingPropertiesEXT hands{XR_TYPE_SYSTEM_HAND_TRACKING_PROPERTIES_EXT};
    XrSystemProperties properties{XR_TYPE_SYSTEM_PROPERTIES};
    properties.next = &hands;
    const XrResult queried = xrGetSystemProperties(instance, system, &properties);
    Log("[handtracking] system properties res=%d supportsHandTracking=%d\n", queried, int(hands.supportsHandTracking));
    if (XR_FAILED(queried) || !hands.supportsHandTracking) return false;
    xrGetInstanceProcAddr(instance, "xrCreateHandTrackerEXT", reinterpret_cast<PFN_xrVoidFunction*>(&g_create));
    xrGetInstanceProcAddr(instance, "xrLocateHandJointsEXT", reinterpret_cast<PFN_xrVoidFunction*>(&g_locate));
    if (!g_create || !g_locate) {
        Log("[handtracking] the runtime has no hand tracking functions\n");
        return false;
    }
    // The controller is asked for by name: without it a runtime may only answer for bare hands.
    XrHandTrackingDataSourceEXT sources[2] = {XR_HAND_TRACKING_DATA_SOURCE_UNOBSTRUCTED_EXT,
                                              XR_HAND_TRACKING_DATA_SOURCE_CONTROLLER_EXT};
    XrHandTrackingDataSourceInfoEXT sourceInfo{XR_TYPE_HAND_TRACKING_DATA_SOURCE_INFO_EXT};
    sourceInfo.requestedDataSourceCount = 2;
    sourceInfo.requestedDataSources = sources;
    for (int h = 0; h < 2; ++h) {
        XrHandTrackerCreateInfoEXT info{XR_TYPE_HAND_TRACKER_CREATE_INFO_EXT};
        if (g_source) info.next = &sourceInfo;
        info.hand = h == 0 ? XR_HAND_LEFT_EXT : XR_HAND_RIGHT_EXT;
        info.handJointSet = XR_HAND_JOINT_SET_DEFAULT_EXT;
        const XrResult created = g_create(session, &info, &g_trackers[h]);
        Log("[handtracking] hand %d tracker res=%d\n", h, created);
        if (XR_FAILED(created)) return false;
    }
    g_ready = true;
    return true;
}
}

const char* Extension(const char* name) {
    if (std::strcmp(name, XR_EXT_HAND_TRACKING_EXTENSION_NAME) == 0) {
        g_tracking = true;
        return XR_EXT_HAND_TRACKING_EXTENSION_NAME;
    }
    if (std::strcmp(name, XR_EXT_HAND_TRACKING_DATA_SOURCE_EXTENSION_NAME) == 0) {
        g_source = true;
        return XR_EXT_HAND_TRACKING_DATA_SOURCE_EXTENSION_NAME;
    }
    if (std::strcmp(name, XR_EXT_HAND_JOINTS_MOTION_RANGE_EXTENSION_NAME) == 0) {
        g_range = true;
        return XR_EXT_HAND_JOINTS_MOTION_RANGE_EXTENSION_NAME;
    }
    return nullptr;
}

void Tick(XrInstance instance, XrSystemId system, XrSession session, XrSpace space, XrTime time) {
    if (g_liveControls.xrFingerTracking == 0 || !Prepare(instance, system, session)) return;
    for (int h = 0; h < 2; ++h) {
        // Unobstructed: the fingers as they are, not closed round the controller.
        XrHandJointsMotionRangeInfoEXT range{XR_TYPE_HAND_JOINTS_MOTION_RANGE_INFO_EXT};
        range.handJointsMotionRange = XR_HAND_JOINTS_MOTION_RANGE_UNOBSTRUCTED_EXT;
        XrHandJointsLocateInfoEXT locate{XR_TYPE_HAND_JOINTS_LOCATE_INFO_EXT};
        if (g_range) locate.next = &range;
        locate.baseSpace = space;
        locate.time = time;
        XrHandJointLocationEXT joints[XR_HAND_JOINT_COUNT_EXT]{};
        XrHandTrackingDataSourceStateEXT source{XR_TYPE_HAND_TRACKING_DATA_SOURCE_STATE_EXT};
        XrHandJointLocationsEXT locations{XR_TYPE_HAND_JOINT_LOCATIONS_EXT};
        if (g_source) locations.next = &source;
        locations.jointCount = XR_HAND_JOINT_COUNT_EXT;
        locations.jointLocations = joints;
        const bool active = XR_SUCCEEDED(g_locate(g_trackers[h], &locate, &locations)) && locations.isActive;
        const int reported = !active ? -1 : (g_source && source.isActive ? int(source.dataSource) : 0);
        if (reported != g_loggedSource[h]) {
            g_loggedSource[h] = reported;
            Log("[handtracking] hand %d source=%d (-1 none, 0 unknown, 1 bare hand, 2 controller)\n", h, reported);
        }
        float closed[5];
        bool valid = active;
        for (int d = 0; valid && d < 5; ++d) {
            const float bend = Bend(joints, kFirst[d], kLast[d]);
            valid = bend >= 0;
            closed[d] = cvr::gestures::FingerClosed(d, bend);
        }
        cvr::gestures::PublishFingers(h, valid ? closed : nullptr);
    }
}
}
