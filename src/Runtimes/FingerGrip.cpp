#include "Runtimes/FingerGrip.hpp"
#include "Runtimes/FingerFilter.hpp"
#include <Windows.h>
#include <atomic>
#include <cmath>
#include <cstdarg>
#include <cstdio>
#include <cstring>

extern void Log(const char*, ...);

namespace cvr::fingergrip {
namespace {
bool g_enabled = false, g_handTracking = false, g_motionRange = false;
PFN_xrCreateHandTrackerEXT  pCreate  = nullptr;
PFN_xrDestroyHandTrackerEXT pDestroy = nullptr;
PFN_xrLocateHandJointsEXT   pLocate  = nullptr;
XrHandTrackerEXT g_tracker[2] = {XR_NULL_HANDLE, XR_NULL_HANDLE};
bool g_gripping[2] = {false, false};
// Once a hand has sent finger data, a pause in it keeps the last state instead of falling back to the squeeze value.
bool g_had[2] = {false, false};
int Hold(int hand) { return g_had[hand] ? (g_gripping[hand] ? 1 : 0) : -1; }

int ReadSetting() {
    char path[MAX_PATH]{};
    if (!GetModuleFileNameA(nullptr, path, MAX_PATH)) return 0;
    if (char* slash = strrchr(path, '\\')) *slash = '\0';
    strcat_s(path, "\\vrport.ini");
    FILE* f = nullptr;
    if (fopen_s(&f, path, "r") != 0 || !f) return 0;
    char line[256]; int value = 0;
    while (fgets(line, sizeof(line), f)) {
        int v = 0;
        if (sscanf_s(line, "xr_grip_from_fingers=%d", &v) == 1 || sscanf_s(line, "xr_grip_from_fingers = %d", &v) == 1) { value = v; break; }
    }
    fclose(f);
    return value;
}

float Angle(const XrVector3f& a, const XrVector3f& b) {
    const float la = std::sqrt(a.x * a.x + a.y * a.y + a.z * a.z), lb = std::sqrt(b.x * b.x + b.y * b.y + b.z * b.z);
    if (la < 1e-5f || lb < 1e-5f) return 0.0f;
    float c = (a.x * b.x + a.y * b.y + a.z * b.z) / (la * lb);
    c = c > 1.0f ? 1.0f : (c < -1.0f ? -1.0f : c);
    return std::acos(c);
}
XrVector3f Sub(const XrVector3f& a, const XrVector3f& b) { return {a.x - b.x, a.y - b.y, a.z - b.z}; }

// A finger's bend from its five joints (metacarpal .. tip): the sum of the three flexion angles (knuckle, middle
// and end joint), over what a fully closed finger reaches. A fist sums to about 4 rad; a relaxed hand to 1-2 rad.
constexpr float kFullBendRad = 3.6f;
float Curl(const XrHandJointLocationEXT* j, int first) {
    const XrVector3f p0 = j[first].pose.position, p1 = j[first + 1].pose.position, p2 = j[first + 2].pose.position,
                     p3 = j[first + 3].pose.position, p4 = j[first + 4].pose.position;
    const XrVector3f b0 = Sub(p1, p0), b1 = Sub(p2, p1), b2 = Sub(p3, p2), b3 = Sub(p4, p3);
    const float bend = Angle(b0, b1) + Angle(b1, b2) + Angle(b2, b3);
    const float c = bend / kFullBendRad;
    return c > 1.0f ? 1.0f : c;
}
// The thumb has one joint less (metacarpal, proximal, distal, tip): two flexion angles, about 1.6 rad when bent.
constexpr float kThumbFullBendRad = 1.6f;
float ThumbCurl(const XrHandJointLocationEXT* j) {
    const XrVector3f p0 = j[XR_HAND_JOINT_THUMB_METACARPAL_EXT].pose.position, p1 = j[XR_HAND_JOINT_THUMB_PROXIMAL_EXT].pose.position,
                     p2 = j[XR_HAND_JOINT_THUMB_DISTAL_EXT].pose.position, p3 = j[XR_HAND_JOINT_THUMB_TIP_EXT].pose.position;
    const XrVector3f b0 = Sub(p1, p0), b1 = Sub(p2, p1), b2 = Sub(p3, p2);
    const float c = (Angle(b0, b1) + Angle(b1, b2)) / kThumbFullBendRad;
    return c > 1.0f ? 1.0f : c;
}

// Last measured bend and spread per hand, for the animation thread. Curl: five bytes (thumb..little); spread: four
// bytes (thumb-index .. ring-little); both with a 23-bit millisecond stamp at bit 40 and bit 63 set once stored.
std::atomic<uint64_t> g_curl[2]{};
std::atomic<uint64_t> g_splay[2]{};
constexpr uint64_t kStampMask = (1ull << 23) - 1;
uint64_t Byte(float v) { return uint64_t((v < 0 ? 0.0f : (v > 1 ? 1.0f : v)) * 255.0f + 0.5f); }
void StoreSplay(int hand, const float splay[4]) {
    const uint64_t now = GetTickCount64();
    g_splay[hand].store((1ull << 63) | ((now & kStampMask) << 40) | Byte(splay[0]) | (Byte(splay[1]) << 8) |
                        (Byte(splay[2]) << 16) | (Byte(splay[3]) << 24), std::memory_order_release);
}

// SPREAD FROM THE JOINTS. The angle between neighbouring digits' first bones (thumb: proximal -> distal; fingers:
// proximal -> intermediate), each projected into the palm plane (index knuckle, little knuckle, wrist), over the angle
// of a pair fully apart: 0 touching .. 1 apart.
constexpr float kSplayFullDeg[4] = {60.f, 25.f, 20.f, 25.f};
bool SplayFromJoints(const XrHandJointLocationEXT* j, float splay[4]) {
    auto P = [&](int k) { return j[k].pose.position; };
    const XrVector3f across = Sub(P(XR_HAND_JOINT_LITTLE_PROXIMAL_EXT), P(XR_HAND_JOINT_INDEX_PROXIMAL_EXT));
    const XrVector3f down = Sub(P(XR_HAND_JOINT_WRIST_EXT), P(XR_HAND_JOINT_INDEX_PROXIMAL_EXT));
    XrVector3f n{across.y * down.z - across.z * down.y, across.z * down.x - across.x * down.z, across.x * down.y - across.y * down.x};
    const float nl = std::sqrt(n.x * n.x + n.y * n.y + n.z * n.z);
    if (nl < 1e-6f) return false;
    n = {n.x / nl, n.y / nl, n.z / nl};
    auto InPlane = [&](int from, int to) {
        const XrVector3f d = Sub(P(to), P(from));
        const float k = d.x * n.x + d.y * n.y + d.z * n.z;
        return XrVector3f{d.x - n.x * k, d.y - n.y * k, d.z - n.z * k};
    };
    const XrVector3f dir[5] = {InPlane(XR_HAND_JOINT_THUMB_PROXIMAL_EXT, XR_HAND_JOINT_THUMB_DISTAL_EXT),
                               InPlane(XR_HAND_JOINT_INDEX_PROXIMAL_EXT, XR_HAND_JOINT_INDEX_INTERMEDIATE_EXT),
                               InPlane(XR_HAND_JOINT_MIDDLE_PROXIMAL_EXT, XR_HAND_JOINT_MIDDLE_INTERMEDIATE_EXT),
                               InPlane(XR_HAND_JOINT_RING_PROXIMAL_EXT, XR_HAND_JOINT_RING_INTERMEDIATE_EXT),
                               InPlane(XR_HAND_JOINT_LITTLE_PROXIMAL_EXT, XR_HAND_JOINT_LITTLE_INTERMEDIATE_EXT)};
    for (int k = 0; k < 4; ++k) {
        const float deg = Angle(dir[k], dir[k + 1]) * 57.2957795f;
        const float s = deg / kSplayFullDeg[k];
        splay[k] = s < 0 ? 0.0f : (s > 1 ? 1.0f : s);
    }
    return true;
}

// The spike and zero-dropout filters (Runtimes/FingerFilter.hpp) on the joint curls.
void FilterLog(const char* format, ...) {
    char text[400]; va_list args; va_start(args, format); vsnprintf(text, sizeof(text), format, args); va_end(args);
    Log("[finger-filter] %s\n", text);
}
FingerFilter g_filter(FilterLog);

// Grip state and the values V's fingers read.
int Use(int hand, float thumb, const float curl[4]) {
    if (!g_had[hand]) Log("[finger-grip] %s hand: finger data from hand tracking\n", hand == 0 ? "left" : "right");
    g_had[hand] = true;
    const bool was = g_gripping[hand];
    g_gripping[hand] = Update(was, curl);
    const ULONGLONG now = GetTickCount64();
    if (thumb >= 0.0f)
        g_curl[hand].store((1ull << 63) | ((now & kStampMask) << 40) | Byte(thumb) | (Byte(curl[0]) << 8) |
                           (Byte(curl[1]) << 16) | (Byte(curl[2]) << 24) | (Byte(curl[3]) << 32),
                           std::memory_order_release);
    return g_gripping[hand] ? 1 : 0;
}

int EvaluateOpenXr(int hand, XrSpace base, XrTime time) {
    if (g_tracker[hand] == XR_NULL_HANDLE || !pLocate || base == XR_NULL_HANDLE) return -1;
    XrHandJointLocationEXT joints[XR_HAND_JOINT_COUNT_EXT]{};
    XrHandJointLocationsEXT locations{XR_TYPE_HAND_JOINT_LOCATIONS_EXT};
    locations.jointCount = XR_HAND_JOINT_COUNT_EXT;
    locations.jointLocations = joints;
    XrHandJointsMotionRangeInfoEXT range{XR_TYPE_HAND_JOINTS_MOTION_RANGE_INFO_EXT};
    range.handJointsMotionRange = XR_HAND_JOINTS_MOTION_RANGE_UNOBSTRUCTED_EXT;
    XrHandJointsLocateInfoEXT info{XR_TYPE_HAND_JOINTS_LOCATE_INFO_EXT};
    info.next = g_motionRange ? &range : nullptr;
    info.baseSpace = base;
    info.time = time;
    if (XR_FAILED(pLocate(g_tracker[hand], &info, &locations)) || !locations.isActive) return Hold(hand);
    for (int j = XR_HAND_JOINT_THUMB_METACARPAL_EXT; j <= XR_HAND_JOINT_LITTLE_TIP_EXT; ++j)
        if (!(joints[j].locationFlags & XR_SPACE_LOCATION_POSITION_VALID_BIT)) return Hold(hand);
    const float raw[4] = {Curl(joints, XR_HAND_JOINT_INDEX_METACARPAL_EXT), Curl(joints, XR_HAND_JOINT_MIDDLE_METACARPAL_EXT),
                          Curl(joints, XR_HAND_JOINT_RING_METACARPAL_EXT), Curl(joints, XR_HAND_JOINT_LITTLE_METACARPAL_EXT)};
    const ULONGLONG now = GetTickCount64();
    float curl[4];
    for (int f = 0; f < 4; ++f) curl[f] = g_filter.Filter(g_filter.fingers_[hand][f + 1], raw[f], now, hand, f + 1);
    const float thumb = g_filter.Filter(g_filter.fingers_[hand][0], ThumbCurl(joints), now, hand, 0);
    float splay[4];
    if (SplayFromJoints(joints, splay)) StoreSplay(hand, splay);
    return Use(hand, thumb, curl);
}
}

bool TrackedCurl(int hand, float curl[5]) {
    if (hand < 0 || hand > 1) return false;
    const uint64_t v = g_curl[hand].load(std::memory_order_acquire);
    if (!(v >> 63)) return false;   // no data yet; afterwards the last values hold through pauses
    for (int d = 0; d < 5; ++d) curl[d] = float((v >> (d * 8)) & 255) / 255.0f;
    return true;
}

bool TrackedSplay(int hand, float splay[4]) {
    if (hand < 0 || hand > 1) return false;
    const uint64_t v = g_splay[hand].load(std::memory_order_acquire);
    if (!(v >> 63)) return false;
    const uint64_t age = (GetTickCount64() - ((v >> 40) & kStampMask)) & kStampMask;
    if (age > 250) return false;
    for (int d = 0; d < 4; ++d) splay[d] = float((v >> (d * 8)) & 255) / 255.0f;
    return true;
}

void ChooseExtensions(const std::vector<XrExtensionProperties>& available, std::vector<const char*>& enabled) {
    g_enabled = ReadSetting() == 1;
    if (!g_enabled) return;
    for (const auto& p : available) {
        if (std::strcmp(p.extensionName, XR_EXT_HAND_TRACKING_EXTENSION_NAME) == 0) g_handTracking = true;
        if (std::strcmp(p.extensionName, XR_EXT_HAND_JOINTS_MOTION_RANGE_EXTENSION_NAME) == 0) g_motionRange = true;
    }
    if (g_handTracking) enabled.push_back(XR_EXT_HAND_TRACKING_EXTENSION_NAME);
    if (g_handTracking && g_motionRange) enabled.push_back(XR_EXT_HAND_JOINTS_MOTION_RANGE_EXTENSION_NAME);
    Log("[finger-grip] xr_grip_from_fingers=1: hand tracking %s, unobstructed motion range %s\n",
        g_handTracking ? "available" : "NOT available (normal grip)", g_motionRange ? "available" : "not available");
}

void OnSessionCreated(XrInstance instance, XrSession session) {
    if (!g_enabled || !g_handTracking) return;
    xrGetInstanceProcAddr(instance, "xrCreateHandTrackerEXT", reinterpret_cast<PFN_xrVoidFunction*>(&pCreate));
    xrGetInstanceProcAddr(instance, "xrDestroyHandTrackerEXT", reinterpret_cast<PFN_xrVoidFunction*>(&pDestroy));
    xrGetInstanceProcAddr(instance, "xrLocateHandJointsEXT", reinterpret_cast<PFN_xrVoidFunction*>(&pLocate));
    if (!pCreate || !pDestroy || !pLocate) { Log("[finger-grip] hand-tracking functions missing\n"); return; }
    for (int h = 0; h < 2; ++h) {
        XrHandTrackerCreateInfoEXT ci{XR_TYPE_HAND_TRACKER_CREATE_INFO_EXT};
        ci.hand = h == 0 ? XR_HAND_LEFT_EXT : XR_HAND_RIGHT_EXT;
        ci.handJointSet = XR_HAND_JOINT_SET_DEFAULT_EXT;
        const XrResult r = pCreate(session, &ci, &g_tracker[h]);
        if (XR_FAILED(r)) { Log("[finger-grip] %s hand tracker not created (%d)\n", h == 0 ? "left" : "right", static_cast<int>(r)); g_tracker[h] = XR_NULL_HANDLE; }
    }
}

void OnSessionDestroying() {
    for (auto& t : g_tracker) if (t != XR_NULL_HANDLE && pDestroy) { pDestroy(t); t = XR_NULL_HANDLE; }
    g_gripping[0] = g_gripping[1] = false;
    g_had[0] = g_had[1] = false;
    g_curl[0].store(0); g_curl[1].store(0);
    g_splay[0].store(0); g_splay[1].store(0);
}

int Evaluate(int hand, XrSpace base, XrTime time) {
    if (hand < 0 || hand > 1) return -1;
    return EvaluateOpenXr(hand, base, time);
}
}
