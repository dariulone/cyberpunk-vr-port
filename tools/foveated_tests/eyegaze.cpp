#include "Runtimes/EyeGaze.hpp"
#include "Utils/XrMath.hpp"
#include <openxr/openxr.h>
#include <array>
#include <atomic>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <iostream>
#include <sstream>
#include <stdexcept>
#include <string>
#include <thread>
#include <utility>
#include <vector>

extern void Log(const char* /*fmt*/, ...) {}
extern "C" {
extern std::atomic<int32_t> CyberpunkVR_TestEyeGazeEnable;
extern std::atomic<float> CyberpunkVR_TestEyeGazeTanX;
extern std::atomic<float> CyberpunkVR_TestEyeGazeTanY;
}

namespace {
using namespace cvr::eyegaze;
void Check(bool ok, const char* msg) { if (!ok) throw std::runtime_error(msg); }

std::atomic<uint64_t> g_fakeNowUs{1'000'000ull};
uint64_t FakeClockUs() { return g_fakeNowUs.load(std::memory_order_relaxed); }

struct MockXrState {
    std::atomic<XrBool32> supportsEyeGaze{XR_FALSE}, poseActive{XR_FALSE};
    std::atomic<uint32_t> boundSourcesCount{0};
    std::atomic<XrResult> boundSourcesResult{XR_SUCCESS}, poseResult{XR_SUCCESS}, locateResult{XR_SUCCESS};
    std::atomic<bool> failLocateWithNext{false};
    std::atomic<XrSpaceLocationFlags> locateFlags{0};
    std::atomic<float> qx{0}, qy{0}, qz{0}, qw{1}, px{0}, py{0}, pz{0};
    std::atomic<XrTime> sampleTimeUs{0};
    std::atomic<int> actionSetsAlive{0}, actionsAlive{0}, spacesAlive{0};
    std::atomic<uint64_t> nextHandle{1}, nextPath{100};
    std::atomic<uint64_t> getSysPropsCalls{0}, enumBoundCalls{0}, locateCalls{0}, locateNextCalls{0}, locateNoNextCalls{0}, suggestCalls{0};
    std::vector<std::pair<XrActionSet, XrAction>> createdActions;
    std::vector<std::pair<XrPath, std::string>> pathTable;
    std::string lastProfile, lastBindingPath;

    void Reset() {
        supportsEyeGaze = XR_FALSE; poseActive = XR_FALSE; boundSourcesCount = 0;
        boundSourcesResult = poseResult = locateResult = XR_SUCCESS;
        failLocateWithNext = false; locateFlags = 0; sampleTimeUs = 0; SetPose({0, 0, 0, 1});
        actionSetsAlive = actionsAlive = spacesAlive = 0;
        getSysPropsCalls = enumBoundCalls = locateCalls = locateNextCalls = locateNoNextCalls = suggestCalls = 0;
        createdActions.clear(); pathTable.clear(); lastProfile.clear(); lastBindingPath.clear();
    }
    void SetPose(XrQuaternionf q, XrVector3f p = {}) { qx = q.x; qy = q.y; qz = q.z; qw = q.w; px = p.x; py = p.y; pz = p.z; }
    XrPosef GetPose() const { return {{qx, qy, qz, qw}, {px, py, pz}}; }
} g_xr;

const auto kInst = reinterpret_cast<XrInstance>(uintptr_t(0x1001));
const auto kSys = XrSystemId(0x2001);
const auto kSess1 = reinterpret_cast<XrSession>(uintptr_t(0x3001));
const auto kSess2 = reinterpret_cast<XrSession>(uintptr_t(0x3002));
const auto kView = reinterpret_cast<XrSpace>(uintptr_t(0x4001));
constexpr XrSpaceLocationFlags kTracked = XR_SPACE_LOCATION_ORIENTATION_VALID_BIT |
    XR_SPACE_LOCATION_ORIENTATION_TRACKED_BIT | XR_SPACE_LOCATION_POSITION_VALID_BIT | XR_SPACE_LOCATION_POSITION_TRACKED_BIT;

const std::array<FrustumTangents, 2> kQ3 = [] {
    constexpr float kDeg = 3.1415926535f / 180.0f;
    return std::array<FrustumTangents, 2>{{
        {std::tan(-54.0f * kDeg), std::tan(40.0f * kDeg), std::tan(-49.0f * kDeg), std::tan(50.0f * kDeg)},
        {std::tan(-40.0f * kDeg), std::tan(54.0f * kDeg), std::tan(-49.0f * kDeg), std::tan(50.0f * kDeg)},
    }};
}();

XrQuaternionf MakeGazeQuat(float tanX, float tanY) {
    Vec3 t{tanX, tanY, -1.0f}; Check(Normalize(t), "MakeGazeQuat normalize");
    XrQuaternionf q{t.y, -t.x, 0.0f, 1.0f - t.z};
    const float n = std::sqrt(q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w); Check(n > 1e-6f, "MakeGazeQuat norm");
    q.x /= n; q.y /= n; q.z /= n; q.w /= n;
    const XrVector3f r = RotateVector(q, {0, 0, -1});
    Check(std::fabs(r.x - t.x) < 1e-5f && std::fabs(r.y - t.y) < 1e-5f && std::fabs(r.z - t.z) < 1e-5f, "MakeGazeQuat rotate");
    return q;
}

std::vector<XrExtensionProperties> MakeExts(bool withGaze) {
    std::vector<XrExtensionProperties> v;
    XrExtensionProperties p{XR_TYPE_EXTENSION_PROPERTIES}; strcpy_s(p.extensionName, "XR_KHR_D3D12_enable"); p.extensionVersion = 9; v.push_back(p);
    if (withGaze) { XrExtensionProperties g{XR_TYPE_EXTENSION_PROPERTIES}; strcpy_s(g.extensionName, XR_EXT_EYE_GAZE_INTERACTION_EXTENSION_NAME); g.extensionVersion = 1; v.push_back(g); }
    return v;
}

void BeginSession(uint64_t nowUs, XrBool32 sysSupported, XrSession sess = kSess1) {
    g_xr.Reset(); CyberpunkVR_TestEyeGazeEnable = 0; g_fakeNowUs = nowUs; SetClockForTesting(&FakeClockUs);
    std::vector<const char*> enabled; ChooseExtensions(MakeExts(true), enabled);
    Check(enabled.size() == 1 && std::strcmp(enabled[0], XR_EXT_EYE_GAZE_INTERACTION_EXTENSION_NAME) == 0, "ext enabled");
    g_xr.supportsEyeGaze = sysSupported; InspectSystem(kInst, kSys); CreateActions(kInst); OnSessionCreated(sess);
}

void Step(uint64_t nowUs, XrSession sess = kSess1, const FrustumTangents* win = kQ3.data()) {
    g_fakeNowUs = nowUs;
    Update(sess, XR_SESSION_STATE_FOCUSED, kView, static_cast<XrTime>(nowUs), true, win, win != nullptr, 0.064f);
}
}  // namespace

extern "C" {
XRAPI_ATTR XrResult XRAPI_CALL xrGetSystemProperties(XrInstance, XrSystemId, XrSystemProperties* p) {
    g_xr.getSysPropsCalls++;
    if (!p || p->type != XR_TYPE_SYSTEM_PROPERTIES) return XR_ERROR_VALIDATION_FAILURE;
    for (auto* e = static_cast<XrBaseOutStructure*>(p->next); e; e = e->next)
        if (e->type == XR_TYPE_SYSTEM_EYE_GAZE_INTERACTION_PROPERTIES_EXT)
            reinterpret_cast<XrSystemEyeGazeInteractionPropertiesEXT*>(e)->supportsEyeGazeInteraction = g_xr.supportsEyeGaze;
    return XR_SUCCESS;
}
XRAPI_ATTR XrResult XRAPI_CALL xrCreateActionSet(XrInstance, const XrActionSetCreateInfo* ci, XrActionSet* out) {
    if (!ci || ci->type != XR_TYPE_ACTION_SET_CREATE_INFO || !out) return XR_ERROR_VALIDATION_FAILURE;
    *out = reinterpret_cast<XrActionSet>(uintptr_t(g_xr.nextHandle++)); g_xr.actionSetsAlive++; return XR_SUCCESS;
}
XRAPI_ATTR XrResult XRAPI_CALL xrCreateAction(XrActionSet set, const XrActionCreateInfo* ci, XrAction* out) {
    if (set == XR_NULL_HANDLE || !ci || ci->type != XR_TYPE_ACTION_CREATE_INFO || !out) return XR_ERROR_VALIDATION_FAILURE;
    *out = reinterpret_cast<XrAction>(uintptr_t(g_xr.nextHandle++)); g_xr.actionsAlive++; g_xr.createdActions.emplace_back(set, *out); return XR_SUCCESS;
}
XRAPI_ATTR XrResult XRAPI_CALL xrDestroyActionSet(XrActionSet set) {
    if (set == XR_NULL_HANDLE) return XR_ERROR_HANDLE_INVALID;
    g_xr.actionSetsAlive--;
    for (auto it = g_xr.createdActions.begin(); it != g_xr.createdActions.end();)
        if (it->first == set) { g_xr.actionsAlive--; it = g_xr.createdActions.erase(it); } else ++it;
    return XR_SUCCESS;
}
XRAPI_ATTR XrResult XRAPI_CALL xrStringToPath(XrInstance, const char* s, XrPath* out) {
    if (!s || !out) return XR_ERROR_VALIDATION_FAILURE;
    for (const auto& [p, str] : g_xr.pathTable) if (str == s) { *out = p; return XR_SUCCESS; }
    *out = XrPath(g_xr.nextPath++); g_xr.pathTable.emplace_back(*out, s); return XR_SUCCESS;
}
XRAPI_ATTR XrResult XRAPI_CALL xrSuggestInteractionProfileBindings(XrInstance, const XrInteractionProfileSuggestedBinding* sb) {
    if (!sb || sb->type != XR_TYPE_INTERACTION_PROFILE_SUGGESTED_BINDING) return XR_ERROR_VALIDATION_FAILURE;
    g_xr.suggestCalls++;
    for (const auto& [p, str] : g_xr.pathTable) {
        if (p == sb->interactionProfile) g_xr.lastProfile = str;
        if (sb->countSuggestedBindings > 0 && p == sb->suggestedBindings[0].binding) g_xr.lastBindingPath = str;
    }
    return XR_SUCCESS;
}
XRAPI_ATTR XrResult XRAPI_CALL xrCreateActionSpace(XrSession s, const XrActionSpaceCreateInfo* ci, XrSpace* out) {
    if (s == XR_NULL_HANDLE || !ci || ci->type != XR_TYPE_ACTION_SPACE_CREATE_INFO || !out) return XR_ERROR_VALIDATION_FAILURE;
    *out = reinterpret_cast<XrSpace>(uintptr_t(g_xr.nextHandle++)); g_xr.spacesAlive++; return XR_SUCCESS;
}
XRAPI_ATTR XrResult XRAPI_CALL xrDestroySpace(XrSpace s) {
    if (s == XR_NULL_HANDLE) return XR_ERROR_HANDLE_INVALID;
    g_xr.spacesAlive--; return XR_SUCCESS;
}
XRAPI_ATTR XrResult XRAPI_CALL xrEnumerateBoundSourcesForAction(XrSession, const XrBoundSourcesForActionEnumerateInfo* ei, uint32_t, uint32_t* count, XrPath*) {
    g_xr.enumBoundCalls++;
    if (!ei || ei->type != XR_TYPE_BOUND_SOURCES_FOR_ACTION_ENUMERATE_INFO || !count) return XR_ERROR_VALIDATION_FAILURE;
    *count = g_xr.boundSourcesCount; return g_xr.boundSourcesResult;
}
XRAPI_ATTR XrResult XRAPI_CALL xrGetActionStatePose(XrSession, const XrActionStateGetInfo* gi, XrActionStatePose* st) {
    if (!gi || gi->type != XR_TYPE_ACTION_STATE_GET_INFO || !st || st->type != XR_TYPE_ACTION_STATE_POSE) return XR_ERROR_VALIDATION_FAILURE;
    st->isActive = g_xr.poseActive; return g_xr.poseResult;
}
XRAPI_ATTR XrResult XRAPI_CALL xrLocateSpace(XrSpace, XrSpace, XrTime, XrSpaceLocation* loc) {
    g_xr.locateCalls++;
    if (!loc || loc->type != XR_TYPE_SPACE_LOCATION) return XR_ERROR_VALIDATION_FAILURE;
    if (loc->next) { g_xr.locateNextCalls++; if (g_xr.failLocateWithNext) return XR_ERROR_VALIDATION_FAILURE; }
    else g_xr.locateNoNextCalls++;
    const XrResult res = g_xr.locateResult;
    if (XR_FAILED(res)) return res;
    loc->locationFlags = g_xr.locateFlags; loc->pose = g_xr.GetPose();
    for (auto* e = static_cast<XrBaseOutStructure*>(loc->next); e; e = e->next)
        if (e->type == XR_TYPE_EYE_GAZE_SAMPLE_TIME_EXT) reinterpret_cast<XrEyeGazeSampleTimeEXT*>(e)->time = g_xr.sampleTimeUs;
    return res;
}
}  // extern "C"

int main() try {
    // 1. Extension absent / unsupported.
    {
        g_xr.Reset(); CyberpunkVR_TestEyeGazeEnable = 0; g_fakeNowUs = 1'000'000ull; SetClockForTesting(&FakeClockUs);
        std::vector<const char*> enabled; ChooseExtensions(MakeExts(false), enabled);
        Check(enabled.empty(), "absent extension must not be enabled");
        Snapshot s = GetSnapshot();
        Check(s.state == TrackingState::Unavailable && !s.extensionEnabled && !s.systemSupported && s.blend == 0 && s.validSamples == 0, "absent ext snapshot");
        InspectSystem(kInst, kSys); Check(g_xr.getSysPropsCalls == 0, "InspectSystem skips xrGetSystemProperties when disabled");
        CreateActions(kInst); Check(ActionSet() == XR_NULL_HANDLE && g_xr.actionSetsAlive == 0 && g_xr.actionsAlive == 0, "CreateActions no-op");
        OnSessionCreated(kSess1); Check(g_xr.spacesAlive == 0, "OnSessionCreated no-op");
        Step(1'000'000ull); s = GetSnapshot();
        Check(s.state == TrackingState::Unavailable && !s.extensionEnabled && s.blend == 0 && s.validSamples == 0, "Update no-op");
        float ox = 99, oy = 99;
        Check(!ResolveEyeOffsets(0, 12.0f, -8.0f, ox, oy) && std::fabs(ox - 12.0f) < 1e-5f && std::fabs(oy + 8.0f) < 1e-5f, "ResolveEyeOffsets fallback");
        Shutdown();
    }

    // 2. Full multi-session lifecycle & clean restart.
    {
        const uint64_t baseSamples = GetSnapshot().validSamples;
        BeginSession(1'000'000ull, XR_TRUE, kSess1);
        Check(g_xr.getSysPropsCalls == 1 && ActionSet() != XR_NULL_HANDLE, "InspectSystem + CreateActions");
        Check(g_xr.actionSetsAlive == 1 && g_xr.actionsAlive == 1 && g_xr.spacesAlive == 1, "initial alive handles");
        Check(g_xr.suggestCalls == 1 && g_xr.lastProfile == "/interaction_profiles/ext/eye_gaze_interaction" &&
              g_xr.lastBindingPath == "/user/eyes_ext/input/gaze_ext/pose", "suggested binding paths");
        Snapshot s = GetSnapshot(); Check(s.state == TrackingState::Inactive && s.extensionEnabled && s.systemSupported, "post-InspectSystem Inactive");
        g_xr.boundSourcesCount = 1; g_xr.poseActive = XR_TRUE; g_xr.locateFlags = kTracked;
        g_xr.SetPose(MakeGazeQuat(0.0f, 0.0f)); g_xr.sampleTimeUs = 1'000'000; g_xr.failLocateWithNext = true;
        Step(1'000'000ull, kSess1); s = GetSnapshot();
        Check(s.state == TrackingState::Active && s.blend == 1.0f && s.validSamples == baseSamples + 1, "session 1 Active");
        Check(s.offsetX[0] != s.offsetX[1] && s.offsetX[0] > 10.0f && s.offsetX[1] < -10.0f, "Quest 3 asymmetric vergence");
        OnSessionStopped(); s = GetSnapshot();
        Check(s.state == TrackingState::Inactive && s.blend == 0 && s.offsetX[0] == 0 && s.offsetX[1] == 0, "OnSessionStopped reset");
        OnSessionDestroying(); Check(g_xr.spacesAlive == 0 && g_xr.actionSetsAlive == 1 && g_xr.actionsAlive == 1, "OnSessionDestroying releases space only");

        g_xr.failLocateWithNext = false; g_xr.boundSourcesCount = 0; g_xr.poseActive = XR_FALSE; g_xr.locateFlags = 0;
        const uint64_t enum0 = g_xr.enumBoundCalls, next0 = g_xr.locateNextCalls;
        OnSessionCreated(kSess2); Check(g_xr.spacesAlive == 1, "session 2 creates space");
        Step(1'020'000ull, kSess2, nullptr);
        Check(g_xr.enumBoundCalls == enum0 + 1 && g_xr.locateNextCalls == next0 + 1, "session 2 resets recheck & sampleTime");
        Check(GetSnapshot().state == TrackingState::Unbound && GetSnapshot().blend == 0, "session 2 starts Unbound");
        g_xr.boundSourcesCount = 1; g_xr.poseActive = XR_TRUE; g_xr.locateFlags = kTracked; g_xr.sampleTimeUs = 1'030'000;
        OnInteractionProfileChanged(); Step(1'030'000ull, kSess2, nullptr);
        Check(GetSnapshot().state == TrackingState::Inactive && GetSnapshot().blend == 0, "g_haveLastWindows cleared across sessions");
        g_xr.sampleTimeUs = 1'040'000; Step(1'040'000ull, kSess2); s = GetSnapshot();
        Check(s.state == TrackingState::Active && s.blend == 1.0f && s.validSamples == baseSamples + 3, "session 2 Active");
        Shutdown();
        Check(g_xr.spacesAlive == 0 && g_xr.actionSetsAlive == 0 && g_xr.actionsAlive == 0 && ActionSet() == XR_NULL_HANDLE, "Shutdown zero leaks");
        s = GetSnapshot(); Check(s.state == TrackingState::Unavailable && !s.extensionEnabled && !s.systemSupported && s.blend == 0, "Shutdown Unavailable");
    }

    // 3. SteamVR Link / PSVR2 unbound-to-active quirk + 2s periodic / OnInteractionProfileChanged recheck.
    {
        BeginSession(5'000'000ull, XR_FALSE); Step(5'000'000ull);
        Check(GetSnapshot().state == TrackingState::Unbound && !GetSnapshot().systemSupported && g_xr.enumBoundCalls == 1 && g_xr.locateCalls == 0, "Unbound initial");
        Step(6'500'000ull); Check(g_xr.enumBoundCalls == 1, "no recheck at +1.5s");
        Step(6'999'999ull); Check(g_xr.enumBoundCalls == 1, "no recheck at +1.999999s");
        Step(7'000'000ull); Check(g_xr.enumBoundCalls == 2, "periodic recheck at >=2s");
        OnInteractionProfileChanged(); Step(7'100'000ull); Check(g_xr.enumBoundCalls == 3, "OnInteractionProfileChanged immediate recheck");
        g_xr.poseActive = XR_TRUE; g_xr.locateFlags = kTracked; g_xr.SetPose(MakeGazeQuat(0.1f, 0.0f)); g_xr.sampleTimeUs = 7'120'000;
        Step(7'120'000ull);
        Check(GetSnapshot().state == TrackingState::Active && GetSnapshot().blend == 1.0f && !GetSnapshot().systemSupported, "poseActive transitions to Active");
        Shutdown();
    }

    // 4. XrEyeGazeSampleTimeEXT validation-error retry fallback.
    {
        BeginSession(10'000'000ull, XR_TRUE);
        g_xr.boundSourcesCount = 1; g_xr.poseActive = XR_TRUE; g_xr.locateFlags = kTracked;
        g_xr.SetPose(MakeGazeQuat(-0.12f, 0.08f)); g_xr.failLocateWithNext = true;
        Step(10'000'000ull);
        Check(g_xr.locateNextCalls == 1 && g_xr.locateNoNextCalls == 1 && GetSnapshot().state == TrackingState::Active, "same-frame unchained retry");
        Step(10'011'000ull); Step(10'022'000ull);
        Check(g_xr.locateNextCalls == 1 && g_xr.locateNoNextCalls == 3 && GetSnapshot().state == TrackingState::Active, "unchained latched");
        Shutdown();
    }

    // 5. Duplicate sampleTime gating, 350ms blink hold, post-blink decay, and frozen-sample stall.
    {
        BeginSession(20'000'000ull, XR_TRUE);
        g_xr.boundSourcesCount = 1; g_xr.poseActive = XR_TRUE; g_xr.locateFlags = kTracked;
        const uint64_t base = GetSnapshot().validSamples;
        g_xr.SetPose(MakeGazeQuat(0.0f, 0.0f)); g_xr.sampleTimeUs = 5'000'000; Step(20'000'000ull);
        const Snapshot s1 = GetSnapshot(); Check(s1.state == TrackingState::Active && s1.validSamples == base + 1, "distinct sample");
        g_xr.SetPose(MakeGazeQuat(std::tan(0.35f * 3.1415926535f / 180.0f), 0.0f)); Step(20'008'333ull);
        const Snapshot s2 = GetSnapshot(); Check(s2.validSamples == base + 1 && s2.offsetX[0] == s1.offsetX[0] && s2.offsetY[0] == s1.offsetY[0], "duplicate sample ignored");
        g_xr.sampleTimeUs = 5'016'667; Step(20'016'667ull);
        Check(GetSnapshot().validSamples == base + 2 && GetSnapshot().offsetX[0] > s1.offsetX[0], "distinct sample steps filter");

        g_xr.SetPose(MakeGazeQuat(0.25f, -0.15f)); g_xr.sampleTimeUs = 5'100'000; Step(20'100'000ull);
        const Snapshot sBlink = GetSnapshot(); Check(sBlink.state == TrackingState::Active && sBlink.validSamples == base + 3, "pre-blink Active");
        g_xr.locateFlags = XR_SPACE_LOCATION_ORIENTATION_VALID_BIT;
        Step(20'300'000ull); Snapshot sHold = GetSnapshot();
        Check(sHold.state == TrackingState::HoldingBlink && sHold.blend == 1.0f && sHold.offsetX[0] == sBlink.offsetX[0], "+200ms blink hold");
        Step(20'450'000ull); Check(GetSnapshot().state == TrackingState::HoldingBlink && GetSnapshot().blend == 1.0f, "+350ms boundary blink hold");
        Step(20'550'000ull); const Snapshot sDecay = GetSnapshot();
        Check(sDecay.state == TrackingState::Inactive && sDecay.blend > 0.0f && sDecay.blend < 1.0f, "+450ms post-blink decay");
        float rx = 0, ry = 0;
        Check(ResolveEyeOffsets(0, 0.0f, -10.0f, 50.0f, 50.0f, rx, ry) &&
              std::fabs(rx - ResolveClampedOffset(sDecay.offsetX[0], 0.0f, 50.0f, sDecay.blend)) < 1e-5f &&
              std::fabs(ry - ResolveClampedOffset(sDecay.offsetY[0], -10.0f, 50.0f, sDecay.blend)) < 1e-5f, "smooth fallback interpolation");

        g_xr.locateFlags = kTracked; g_xr.SetPose(MakeGazeQuat(-0.2f, 0.1f)); g_xr.sampleTimeUs = 6'000'000;
        constexpr uint64_t kStall0 = 25'000'000ull; Step(kStall0);
        const uint64_t stallBase = GetSnapshot().validSamples;
        bool ok60 = false, okBlink = false, okDecay = false, okZero = false;
        for (uint64_t dt = 20'000ull; dt <= 2'000'000ull; dt += 20'000ull) {
            Step(kStall0 + dt); const Snapshot st = GetSnapshot();
            Check(st.validSamples == stallBase, "frozen sample never increments validSamples");
            if (dt == 40'000ull) ok60 = (st.state == TrackingState::Active && st.blend == 1.0f);
            else if (dt == 200'000ull) okBlink = (st.state == TrackingState::HoldingBlink && st.blend == 1.0f);
            else if (dt == 460'000ull) okDecay = (st.state == TrackingState::Inactive && st.blend > 0 && st.blend < 1);
            else if (dt == 2'000'000ull) okZero = (st.state == TrackingState::Inactive && st.blend == 0);
        }
        Check(ok60 && okBlink && okDecay && okZero, "frozen sample stall hold/blink/decay/zero");
        Shutdown();
    }

    // 6. Cached frustum fallback & CyberpunkVR_TestEyeGazeEnable (0 -> 1 -> 2 -> 0).
    {
        BeginSession(30'000'000ull, XR_TRUE);
        g_xr.boundSourcesCount = 1; g_xr.poseActive = XR_TRUE; g_xr.locateFlags = kTracked;
        g_xr.SetPose(MakeGazeQuat(0.25f, -0.15f)); g_xr.sampleTimeUs = 30'000'000; Step(30'000'000ull);
        const Snapshot sc = GetSnapshot();
        g_xr.sampleTimeUs = 30'011'000; Step(30'011'000ull, kSess1, nullptr);
        const Snapshot sf = GetSnapshot();
        Check(sf.state == TrackingState::Active && sf.blend == 1.0f &&
              std::fabs(sf.offsetX[0] - sc.offsetX[0]) < 1e-5f && std::fabs(sf.offsetX[1] - sc.offsetX[1]) < 1e-5f &&
              std::fabs(sf.offsetY[0] - sc.offsetY[0]) < 1e-5f && std::fabs(sf.offsetY[1] - sc.offsetY[1]) < 1e-5f, "cached frustum fallback");
        Shutdown();

        float ex[2]{}, ey[2]{};
        for (unsigned e = 0; e < 2; ++e) Check(ProjectGazeToEyePercent({0, 0, 0}, {0.25f, -0.15f, -1}, e, 0.064f, 2.0f, kQ3[e], ex[e], ey[e]), "test ray proj");
        CyberpunkVR_TestEyeGazeTanX = 0.25f; CyberpunkVR_TestEyeGazeTanY = -0.15f; CyberpunkVR_TestEyeGazeEnable = 1;
        Step(40'000'000ull, XR_NULL_HANDLE); Snapshot st = GetSnapshot();
        Check(st.state == TrackingState::Active && st.blend == 1.0f &&
              std::fabs(st.offsetX[0] - ex[0]) < 1e-4f && std::fabs(st.offsetX[1] - ex[1]) < 1e-4f &&
              std::fabs(st.offsetY[0] - ey[0]) < 1e-4f && std::fabs(st.offsetY[1] - ey[1]) < 1e-4f, "standalone TestEyeGazeEnable=1");
        CyberpunkVR_TestEyeGazeEnable = 2;
        Step(40'200'000ull, XR_NULL_HANDLE); Check(GetSnapshot().state == TrackingState::HoldingBlink && GetSnapshot().blend == 1.0f, "test mode 2 blink");
        Step(40'450'000ull, XR_NULL_HANDLE); Check(GetSnapshot().state == TrackingState::Inactive && GetSnapshot().blend > 0 && GetSnapshot().blend < 1, "test mode 2 decay");
        CyberpunkVR_TestEyeGazeEnable = 0;
        Step(40'470'000ull, XR_NULL_HANDLE); st = GetSnapshot();
        Check(st.state == TrackingState::Unavailable && st.blend == 0 && st.offsetX[0] == 0 && st.offsetX[1] == 0, "test mode 0 reset");

        BeginSession(41'000'000ull, XR_TRUE); g_xr.boundSourcesCount = 1; g_xr.poseActive = XR_TRUE; g_xr.locateFlags = 0;
        CyberpunkVR_TestEyeGazeEnable = 1; Step(41'000'000ull); Check(GetSnapshot().state == TrackingState::Active, "session test mode 1");
        CyberpunkVR_TestEyeGazeEnable = 0; Step(41'010'000ull);
        Check(GetSnapshot().state == TrackingState::Inactive && GetSnapshot().blend == 0, "session test mode 1->0 resets filter");
        Shutdown();
    }

    // 7. 5-thread start-latched seqlock coherence stress (virtual clock + live Shutdown/re-init + real QPC).
    {
        BeginSession(100'000'000ull, XR_TRUE, kSess1);
        g_xr.boundSourcesCount = 1; g_xr.poseActive = XR_TRUE;
        constexpr float kAx = -0.55f, kAy = 0.20f, kBx = 0.55f, kBy = -0.20f;
        const XrQuaternionf qA = MakeGazeQuat(kAx, kAy), qB = MakeGazeQuat(kBx, kBy);
        float ax[2]{}, ay[2]{}, bx[2]{}, by[2]{};
        for (unsigned e = 0; e < 2; ++e) {
            Check(ProjectGazeToEyePercent({0, 0, 0}, {kAx, kAy, -1}, e, 0.064f, 2.0f, kQ3[e], ax[e], ay[e]), "Ray A proj");
            Check(ProjectGazeToEyePercent({0, 0, 0}, {kBx, kBy, -1}, e, 0.064f, 2.0f, kQ3[e], bx[e], by[e]), "Ray B proj");
        }
        Check(ax[0] < -5 && ax[1] < -5 && bx[0] > 5 && bx[1] > 5, "opposite-sign stereo rays");
        g_xr.locateFlags = kTracked; g_xr.SetPose(qA); g_xr.sampleTimeUs = 100'000'000; Step(100'000'000ull, kSess1);

        std::atomic<int> ready{0}, errors{0};
        std::atomic<bool> start{false}, stopReaders{false}, sawA{false}, sawB{false}, sawZeroOrBlink{false}, captured{false};
        std::string firstErr;
        auto fail = [&](const std::string& m) { errors++; if (!captured.exchange(true)) firstErr = m; };
        auto nearPair = [](const Snapshot& s, const float x[2], const float y[2]) {
            return std::fabs(s.offsetX[0] - x[0]) < 1e-3f && std::fabs(s.offsetX[1] - x[1]) < 1e-3f &&
                   std::fabs(s.offsetY[0] - y[0]) < 1e-3f && std::fabs(s.offsetY[1] - y[1]) < 1e-3f;
        };

        std::vector<std::thread> readers;
        for (int t = 0; t < 4; ++t) {
            readers.emplace_back([&] {
                ready.fetch_add(1, std::memory_order_acq_rel);
                while (!start.load(std::memory_order_acquire)) std::this_thread::yield();
                for (int r = 0; !stopReaders.load(std::memory_order_acquire) || r < 50'000; ++r) {
                    const Snapshot s = GetSnapshot();
                    if (!std::isfinite(s.offsetX[0]) || !std::isfinite(s.offsetX[1]) || !std::isfinite(s.offsetY[0]) ||
                        !std::isfinite(s.offsetY[1]) || !std::isfinite(s.blend) || s.blend < 0 || s.blend > 1) { fail("invalid snapshot"); break; }
                    if (s.blend == 0.0f) {
                        if (s.offsetX[0] != 0 || s.offsetX[1] != 0 || s.offsetY[0] != 0 || s.offsetY[1] != 0) { fail("zero blend non-zero offsets"); break; }
                        sawZeroOrBlink.store(true, std::memory_order_relaxed);
                    } else {
                        if ((s.state == TrackingState::Active || s.state == TrackingState::HoldingBlink) && s.blend != 1.0f) { fail("Active/Blink blend != 1"); break; }
                        if (s.state == TrackingState::HoldingBlink) sawZeroOrBlink.store(true, std::memory_order_relaxed);
                        const bool mA = nearPair(s, ax, ay), mB = nearPair(s, bx, by);
                        if (!mA && !mB) { std::ostringstream os; os << "torn ox=(" << s.offsetX[0] << "," << s.offsetX[1] << ") blend=" << s.blend; fail(os.str()); break; }
                        if (mA) sawA.store(true, std::memory_order_relaxed);
                        if (mB) sawB.store(true, std::memory_order_relaxed);
                    }
                    float rx = 0, ry = 0;
                    const bool ok = ResolveEyeOffsets(unsigned(r & 1), 3.0f, -4.0f, 45.0f, 45.0f, rx, ry);
                    if (!std::isfinite(rx) || !std::isfinite(ry) || std::fabs(rx) > 45.0001f || std::fabs(ry) > 45.0001f ||
                        (!ok && (std::fabs(rx - 3.0f) > 1e-5f || std::fabs(ry + 4.0f) > 1e-5f))) { fail("ResolveEyeOffsets invariant"); break; }
                }
            });
        }

        std::thread writer([&] {
            ready.fetch_add(1, std::memory_order_acq_rel);
            while (!start.load(std::memory_order_acquire)) std::this_thread::yield();
            auto drive = [&](int phase, bool adv) {
                if (phase == 0 || phase == 2) { g_xr.locateFlags = kTracked; g_xr.SetPose(phase == 0 ? qA : qB); g_xr.sampleTimeUs += 10'000; if (adv) g_fakeNowUs += 10'000ull; }
                else { g_xr.locateFlags = 0; if (adv) g_fakeNowUs += 60'000ull; }
            };
            for (int i = 0; i < 45'000; ++i) {
                drive((i / 10) % 4, true);
                Update(kSess1, XR_SESSION_STATE_FOCUSED, kView, static_cast<XrTime>(g_fakeNowUs.load()), true, kQ3.data(), true, 0.064f);
            }
            Shutdown(); SetClockForTesting(nullptr);
            std::vector<const char*> reinit; ChooseExtensions(MakeExts(true), reinit);
            g_xr.supportsEyeGaze = XR_TRUE; InspectSystem(kInst, kSys); CreateActions(kInst); OnSessionCreated(kSess2);
            g_xr.boundSourcesCount = 1; g_xr.poseActive = XR_TRUE;
            for (int i = 0; i < 5'000; ++i) {
                drive((i / 10) % 4, false);
                Update(kSess2, XR_SESSION_STATE_FOCUSED, kView, static_cast<XrTime>(200'000'000ll + int64_t(i) * 10'000ll), true, kQ3.data(), true, 0.064f);
            }
            stopReaders.store(true, std::memory_order_release);
        });

        while (ready.load(std::memory_order_acquire) < 5) std::this_thread::yield();
        start.store(true, std::memory_order_release);
        writer.join(); for (auto& r : readers) r.join();
        SetClockForTesting(nullptr);
        Check(errors == 0, firstErr.empty() ? "seqlock stress failed" : firstErr.c_str());
        Check(sawA && sawB && sawZeroOrBlink, "readers must observe Ray A, Ray B, and blink/zero states");
        Shutdown();
    }

    SetClockForTesting(nullptr);
    std::cout << "PASS EyeGaze runtime state machine, OpenXR mocks, test override, and seqlock coherence stress\n";
    return 0;
} catch (const std::exception& e) {
    SetClockForTesting(nullptr);
    std::cerr << "FAIL: " << e.what() << '\n';
    return 1;
}
