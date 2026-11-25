#include "Runtimes/EyeGaze.hpp"
#include "Utils/XrMath.hpp"
#include <windows.h>
#include <atomic>
#include <cstring>

extern void Log(const char* fmt, ...);

// Exported test override for simulator / automated live probes (tools/foveated_tests/live_probe.py).
// 0 = real OpenXR gaze; 1 = inject valid gaze ray (TanX, TanY, -1); 2 = simulate blink/dropout.
extern "C" {
__declspec(dllexport) std::atomic<int32_t> CyberpunkVR_TestEyeGazeEnable{0};
__declspec(dllexport) std::atomic<float> CyberpunkVR_TestEyeGazeTanX{0.0f};
__declspec(dllexport) std::atomic<float> CyberpunkVR_TestEyeGazeTanY{0.0f};
}

namespace cvr::eyegaze {
namespace {

constexpr const char* kProfilePath = "/interaction_profiles/ext/eye_gaze_interaction";
constexpr const char* kGazePosePath = "/user/eyes_ext/input/gaze_ext/pose";
constexpr float kDefaultConvergenceMeters = 2.0f;

bool g_extensionEnabled = false;
bool g_instanceReady = false;
bool g_systemSupported = false;
int32_t g_prevTestMode = 0;
std::atomic<bool> g_recheckBound{true};
bool g_boundChecked = false;
bool g_boundAny = false;
XrResult g_lastBoundRes = XR_SUCCESS;
uint64_t g_lastBoundCheckUs = 0;
bool g_locateErrorLogged = false;
bool g_sampleTimeUnsupported = false;
XrTime g_lastSampleTime = 0;
FrustumTangents g_lastWindows[2]{};
bool g_haveLastWindows = false;

XrInstance g_instance = XR_NULL_HANDLE;
XrActionSet g_actionSet = XR_NULL_HANDLE;
XrAction g_gazeAction = XR_NULL_HANDLE;
XrSpace g_gazeSpace = XR_NULL_HANDLE;
GazeFilter g_filter;

// Seqlock-protected published snapshot (single writer on XR loop, lock-free readers on render/UI threads).
std::atomic<uint32_t> g_seq{0};
std::atomic<uint8_t> g_state{static_cast<uint8_t>(TrackingState::Unavailable)};
std::atomic<bool> g_pubExtEnabled{false};
std::atomic<bool> g_pubSysSupported{false};
std::atomic<float> g_offsetX[2]{};
std::atomic<float> g_offsetY[2]{};
std::atomic<float> g_blend{0.0f};
std::atomic<uint64_t> g_validSamples{0};
std::atomic<uint64_t (*)()> g_testClockUs{nullptr};

uint64_t NowUs() {
    if (auto* fn = g_testClockUs.load(std::memory_order_acquire)) return fn();
    static const int64_t freq = [] {
        LARGE_INTEGER f{};
        QueryPerformanceFrequency(&f);
        return f.QuadPart > 0 ? f.QuadPart : 1;
    }();
    LARGE_INTEGER c{};
    QueryPerformanceCounter(&c);
    const int64_t whole = c.QuadPart / freq;
    const int64_t rem = c.QuadPart % freq;
    return static_cast<uint64_t>(whole) * 1'000'000ull +
           static_cast<uint64_t>((rem * 1'000'000ll) / freq);
}

void Publish(TrackingState state, const float ox[2], const float oy[2], float blend) {
    g_seq.fetch_add(1, std::memory_order_relaxed);
    std::atomic_thread_fence(std::memory_order_release);
    g_state.store(static_cast<uint8_t>(state), std::memory_order_relaxed);
    g_pubExtEnabled.store(g_extensionEnabled && g_instanceReady, std::memory_order_relaxed);
    g_pubSysSupported.store(g_systemSupported, std::memory_order_relaxed);
    g_offsetX[0].store(ox[0], std::memory_order_relaxed);
    g_offsetX[1].store(ox[1], std::memory_order_relaxed);
    g_offsetY[0].store(oy[0], std::memory_order_relaxed);
    g_offsetY[1].store(oy[1], std::memory_order_relaxed);
    g_blend.store(blend, std::memory_order_relaxed);
    g_seq.fetch_add(1, std::memory_order_release);
}

void ResetPublished(TrackingState state) {
    const float zero[2]{0.0f, 0.0f};
    Publish(state, zero, zero, 0.0f);
}

}  // namespace

void ChooseExtensions(const std::vector<XrExtensionProperties>& available, std::vector<const char*>& enabled) {
    g_extensionEnabled = false;
    g_instanceReady = false;
    g_systemSupported = false;
    for (const auto& p : available) {
        if (std::strcmp(p.extensionName, XR_EXT_EYE_GAZE_INTERACTION_EXTENSION_NAME) == 0) {
            g_extensionEnabled = true;
            enabled.push_back(XR_EXT_EYE_GAZE_INTERACTION_EXTENSION_NAME);
            break;
        }
    }
    ResetPublished(TrackingState::Unavailable);
    Log("OpenXRManager: eye-gaze (%s) supported=%d\n",
        XR_EXT_EYE_GAZE_INTERACTION_EXTENSION_NAME, g_extensionEnabled ? 1 : 0);
}

void InspectSystem(XrInstance instance, XrSystemId systemId) {
    g_instance = instance;
    g_instanceReady = false;
    g_systemSupported = false;
    if (!g_extensionEnabled || instance == XR_NULL_HANDLE || systemId == XR_NULL_SYSTEM_ID) {
        ResetPublished(TrackingState::Unavailable);
        return;
    }

    XrSystemEyeGazeInteractionPropertiesEXT gazeProps{XR_TYPE_SYSTEM_EYE_GAZE_INTERACTION_PROPERTIES_EXT};
    XrSystemProperties sysProps{XR_TYPE_SYSTEM_PROPERTIES};
    sysProps.next = &gazeProps;
    const XrResult res = xrGetSystemProperties(instance, systemId, &sysProps);
    if (XR_SUCCEEDED(res)) {
        g_systemSupported = (gazeProps.supportsEyeGazeInteraction == XR_TRUE);
    }
    g_instanceReady = true;
    ResetPublished(TrackingState::Inactive);
    Log("[eye-gaze] system supportsEyeGazeInteraction=%d (res=%d)\n",
        g_systemSupported ? 1 : 0, static_cast<int>(res));
}

void CreateActions(XrInstance instance) {
    if (!g_extensionEnabled || instance == XR_NULL_HANDLE) return;
    g_instance = instance;

    XrActionSetCreateInfo setInfo{XR_TYPE_ACTION_SET_CREATE_INFO};
    strcpy_s(setInfo.actionSetName, "eye_gaze");
    strcpy_s(setInfo.localizedActionSetName, "Eye Gaze");
    setInfo.priority = 0;
    XrResult res = xrCreateActionSet(instance, &setInfo, &g_actionSet);
    if (XR_FAILED(res) || g_actionSet == XR_NULL_HANDLE) {
        Log("[eye-gaze] failed to create action set (res=%d)\n", static_cast<int>(res));
        g_actionSet = XR_NULL_HANDLE;
        return;
    }

    XrActionCreateInfo actionInfo{XR_TYPE_ACTION_CREATE_INFO};
    actionInfo.actionType = XR_ACTION_TYPE_POSE_INPUT;
    strcpy_s(actionInfo.actionName, "gaze_pose");
    strcpy_s(actionInfo.localizedActionName, "Eye Gaze Pose");
    res = xrCreateAction(g_actionSet, &actionInfo, &g_gazeAction);
    if (XR_FAILED(res) || g_gazeAction == XR_NULL_HANDLE) {
        Log("[eye-gaze] failed to create gaze_pose action (res=%d)\n", static_cast<int>(res));
        xrDestroyActionSet(g_actionSet);
        g_actionSet = XR_NULL_HANDLE;
        g_gazeAction = XR_NULL_HANDLE;
        return;
    }

    XrPath profile = XR_NULL_PATH;
    XrPath bindingPath = XR_NULL_PATH;
    if (XR_SUCCEEDED(xrStringToPath(instance, kProfilePath, &profile)) &&
        XR_SUCCEEDED(xrStringToPath(instance, kGazePosePath, &bindingPath))) {
        XrActionSuggestedBinding binding{g_gazeAction, bindingPath};
        XrInteractionProfileSuggestedBinding sb{XR_TYPE_INTERACTION_PROFILE_SUGGESTED_BINDING};
        sb.interactionProfile = profile;
        sb.suggestedBindings = &binding;
        sb.countSuggestedBindings = 1;
        const XrResult suggestRes = xrSuggestInteractionProfileBindings(instance, &sb);
        Log("[eye-gaze] suggest bindings %s -> %d\n", kProfilePath, static_cast<int>(suggestRes));
    }
}

XrActionSet ActionSet() {
    return g_actionSet;
}

void OnSessionCreated(XrSession session) {
    g_boundChecked = false;
    g_boundAny = false;
    g_lastBoundRes = XR_SUCCESS;
    g_lastBoundCheckUs = 0;
    g_recheckBound.store(true, std::memory_order_relaxed);
    g_locateErrorLogged = false;
    g_sampleTimeUnsupported = false;
    g_lastSampleTime = 0;
    g_prevTestMode = 0;
    g_haveLastWindows = false;
    g_filter.Reset();
    if (session == XR_NULL_HANDLE || g_gazeAction == XR_NULL_HANDLE) return;

    XrActionSpaceCreateInfo spaceInfo{XR_TYPE_ACTION_SPACE_CREATE_INFO};
    spaceInfo.action = g_gazeAction;
    spaceInfo.subactionPath = XR_NULL_PATH;
    spaceInfo.poseInActionSpace.orientation.w = 1.0f;
    const XrResult res = xrCreateActionSpace(session, &spaceInfo, &g_gazeSpace);
    if (XR_FAILED(res)) {
        Log("[eye-gaze] xrCreateActionSpace failed (res=%d)\n", static_cast<int>(res));
        g_gazeSpace = XR_NULL_HANDLE;
    }
}

void OnSessionStopped() {
    g_lastSampleTime = 0;
    g_prevTestMode = 0;
    g_filter.Reset();
    const TrackingState st = (!g_extensionEnabled || !g_instanceReady)
        ? TrackingState::Unavailable
        : ((g_boundChecked && !g_boundAny) ? TrackingState::Unbound : TrackingState::Inactive);
    ResetPublished(st);
}

void OnSessionDestroying() {
    if (g_gazeSpace != XR_NULL_HANDLE) {
        xrDestroySpace(g_gazeSpace);
        g_gazeSpace = XR_NULL_HANDLE;
    }
    g_boundChecked = false;
    g_boundAny = false;
    g_lastBoundRes = XR_SUCCESS;
    g_lastBoundCheckUs = 0;
    g_recheckBound.store(true, std::memory_order_relaxed);
    g_locateErrorLogged = false;
    g_sampleTimeUnsupported = false;
    g_lastSampleTime = 0;
    g_prevTestMode = 0;
    g_haveLastWindows = false;
    g_filter.Reset();
    ResetPublished((g_extensionEnabled && g_instanceReady) ? TrackingState::Inactive : TrackingState::Unavailable);
}

void OnInteractionProfileChanged() {
    g_recheckBound.store(true, std::memory_order_relaxed);
}

void Shutdown() {
    OnSessionDestroying();
    if (g_actionSet != XR_NULL_HANDLE) {
        xrDestroyActionSet(g_actionSet);
        g_actionSet = XR_NULL_HANDLE;
    }
    g_gazeAction = XR_NULL_HANDLE;
    g_instance = XR_NULL_HANDLE;
    g_extensionEnabled = false;
    g_instanceReady = false;
    g_systemSupported = false;
    g_prevTestMode = 0;
    ResetPublished(TrackingState::Unavailable);
}

void Update(XrSession session, XrSessionState sessionState, XrSpace viewSpace, XrTime locateTime,
            bool canLocate, const FrustumTangents eyeWindows[2], bool windowsValid, float ipdMeters) {
    const int32_t testMode = CyberpunkVR_TestEyeGazeEnable.load(std::memory_order_relaxed);
    const uint64_t nowUs = NowUs();
    if ((g_prevTestMode != 0) != (testMode != 0)) {
        g_filter.Reset();
        g_lastSampleTime = 0;
    }
    g_prevTestMode = testMode;

    bool sampleValid = false;
    bool duplicateSample = false;
    Vec3 sampleOrigin{0.0f, 0.0f, 0.0f};
    Vec3 sampleDir{0.0f, 0.0f, -1.0f};

    if (testMode != 0) {
        if (testMode == 1) {
            sampleValid = true;
            sampleDir = {
                CyberpunkVR_TestEyeGazeTanX.load(std::memory_order_relaxed),
                CyberpunkVR_TestEyeGazeTanY.load(std::memory_order_relaxed),
                -1.0f,
            };
        }
    } else {
        if (!g_extensionEnabled || g_gazeSpace == XR_NULL_HANDLE || viewSpace == XR_NULL_HANDLE) {
            ResetPublished((!g_extensionEnabled || !g_instanceReady) ? TrackingState::Unavailable : TrackingState::Inactive);
            return;
        }
        if (sessionState == XR_SESSION_STATE_FOCUSED && canLocate && session != XR_NULL_HANDLE && g_gazeAction != XR_NULL_HANDLE) {
            const bool recheck = g_recheckBound.exchange(false, std::memory_order_relaxed);
            const bool periodic = !g_boundAny &&
                (g_lastBoundCheckUs == 0 || (nowUs >= g_lastBoundCheckUs && (nowUs - g_lastBoundCheckUs) >= 2'000'000ull));
            if (recheck || periodic) {
                g_lastBoundCheckUs = nowUs;
                XrBoundSourcesForActionEnumerateInfo boundInfo{XR_TYPE_BOUND_SOURCES_FOR_ACTION_ENUMERATE_INFO};
                boundInfo.action = g_gazeAction;
                uint32_t count = 0;
                const XrResult boundRes = xrEnumerateBoundSourcesForAction(session, &boundInfo, 0, &count, nullptr);
                const bool prevBoundAny = g_boundAny;
                g_boundAny = XR_SUCCEEDED(boundRes) && count > 0;
                if (!g_boundChecked || g_boundAny != prevBoundAny || boundRes != g_lastBoundRes) {
                    Log("[eye-gaze] bound sources=%u (res=%d, systemSupported=%d)\n",
                        count, static_cast<int>(boundRes), g_systemSupported ? 1 : 0);
                }
                g_lastBoundRes = boundRes;
                g_boundChecked = true;
            }

            XrActionStateGetInfo getInfo{XR_TYPE_ACTION_STATE_GET_INFO};
            getInfo.action = g_gazeAction;
            XrActionStatePose poseState{XR_TYPE_ACTION_STATE_POSE};
            const XrResult poseRes = xrGetActionStatePose(session, &getInfo, &poseState);
            const bool poseActive = XR_SUCCEEDED(poseRes) && poseState.isActive == XR_TRUE;
            if (poseActive) {
                g_boundAny = true;
            }
            if (poseActive || g_boundAny || g_systemSupported) {
                XrEyeGazeSampleTimeEXT sampleTime{XR_TYPE_EYE_GAZE_SAMPLE_TIME_EXT};
                XrSpaceLocation loc{XR_TYPE_SPACE_LOCATION};
                loc.next = g_sampleTimeUnsupported ? nullptr : &sampleTime;
                XrResult locRes = xrLocateSpace(g_gazeSpace, viewSpace, locateTime, &loc);
                if (XR_FAILED(locRes) && !g_sampleTimeUnsupported) {
                    XrSpaceLocation retryLoc{XR_TYPE_SPACE_LOCATION};
                    const XrResult retryRes = xrLocateSpace(g_gazeSpace, viewSpace, locateTime, &retryLoc);
                    if (XR_SUCCEEDED(retryRes)) {
                        g_sampleTimeUnsupported = true;
                        loc = retryLoc;
                        locRes = retryRes;
                        Log("[eye-gaze] XrEyeGazeSampleTimeEXT unsupported; falling back to unchained xrLocateSpace\n");
                    }
                }
                if (XR_SUCCEEDED(locRes)) {
                    constexpr XrSpaceLocationFlags kNeedOri =
                        XR_SPACE_LOCATION_ORIENTATION_VALID_BIT | XR_SPACE_LOCATION_ORIENTATION_TRACKED_BIT;
                    if ((loc.locationFlags & kNeedOri) == kNeedOri) {
                        const XrVector3f fwd = RotateVector(loc.pose.orientation, XrVector3f{0.0f, 0.0f, -1.0f});
                        sampleDir = {fwd.x, fwd.y, fwd.z};
                        if ((loc.locationFlags & XR_SPACE_LOCATION_POSITION_VALID_BIT) != 0) {
                            sampleOrigin = {loc.pose.position.x, loc.pose.position.y, loc.pose.position.z};
                        }
                        sampleValid = true;
                        g_boundAny = true;
                        if (!g_sampleTimeUnsupported && sampleTime.time > 0) {
                            duplicateSample = (g_lastSampleTime > 0 && sampleTime.time == g_lastSampleTime);
                            g_lastSampleTime = sampleTime.time;
                        }
                    }
                } else if (!g_locateErrorLogged) {
                    g_locateErrorLogged = true;
                    Log("[eye-gaze] xrLocateSpace failed (res=%d)\n", static_cast<int>(locRes));
                }
            }
        }
    }

    if (windowsValid && eyeWindows != nullptr) {
        g_lastWindows[0] = eyeWindows[0];
        g_lastWindows[1] = eyeWindows[1];
        g_haveLastWindows = true;
    }

    g_filter.UpdateUs(sampleValid, sampleOrigin, sampleDir, nowUs, duplicateSample);
    if (sampleValid && !duplicateSample) {
        g_validSamples.fetch_add(1, std::memory_order_relaxed);
    }

    if (!g_filter.HasDirection()) {
        const TrackingState st = ((!g_extensionEnabled || !g_instanceReady) && testMode == 0)
            ? TrackingState::Unavailable
            : ((g_boundChecked && !g_boundAny && testMode == 0) ? TrackingState::Unbound : g_filter.State());
        ResetPublished(st);
        return;
    }

    const FrustumTangents* activeWindows = (windowsValid && eyeWindows != nullptr)
        ? eyeWindows
        : (g_haveLastWindows ? g_lastWindows : nullptr);
    if (!activeWindows) {
        ResetPublished(TrackingState::Inactive);
        return;
    }

    float ox[2]{0.0f, 0.0f};
    float oy[2]{0.0f, 0.0f};
    bool ok = true;
    for (unsigned e = 0; e < 2; ++e) {
        ok &= ProjectGazeToEyePercent(g_filter.Origin(), g_filter.Direction(), e, ipdMeters,
                                      kDefaultConvergenceMeters, activeWindows[e], ox[e], oy[e]);
    }
    if (!ok) {
        ResetPublished(TrackingState::Inactive);
        return;
    }
    Publish(g_filter.State(), ox, oy, g_filter.ActiveBlend());
}

Snapshot GetSnapshot() {
    static thread_local Snapshot s_lastGood{};
    for (int attempt = 0; attempt < 64; ++attempt) {
        const uint32_t s0 = g_seq.load(std::memory_order_acquire);
        if (s0 & 1u) {
            YieldProcessor();
            continue;
        }
        Snapshot s;
        s.state = static_cast<TrackingState>(g_state.load(std::memory_order_relaxed));
        s.extensionEnabled = g_pubExtEnabled.load(std::memory_order_relaxed);
        s.systemSupported = g_pubSysSupported.load(std::memory_order_relaxed);
        s.offsetX[0] = g_offsetX[0].load(std::memory_order_relaxed);
        s.offsetX[1] = g_offsetX[1].load(std::memory_order_relaxed);
        s.offsetY[0] = g_offsetY[0].load(std::memory_order_relaxed);
        s.offsetY[1] = g_offsetY[1].load(std::memory_order_relaxed);
        s.blend = g_blend.load(std::memory_order_relaxed);
        s.validSamples = g_validSamples.load(std::memory_order_relaxed);
        std::atomic_thread_fence(std::memory_order_acquire);
        if (s0 == g_seq.load(std::memory_order_relaxed)) {
            s_lastGood = s;
            return s;
        }
        YieldProcessor();
    }
    return s_lastGood;
}

bool ResolveEyeOffsets(unsigned physicalEye, float fallbackX, float fallbackY,
                       float maxAbsX, float maxAbsY, float& outX, float& outY) {
    outX = ResolveClampedOffset(fallbackX, fallbackX, maxAbsX, 0.0f);
    outY = ResolveClampedOffset(fallbackY, fallbackY, maxAbsY, 0.0f);
    if (physicalEye > 1) return false;
    const Snapshot s = GetSnapshot();
    if (!(s.blend > 0.0f)) return false;
    outX = ResolveClampedOffset(s.offsetX[physicalEye], fallbackX, maxAbsX, s.blend);
    outY = ResolveClampedOffset(s.offsetY[physicalEye], fallbackY, maxAbsY, s.blend);
    return true;
}

bool ResolveEyeOffsets(unsigned physicalEye, float fallbackX, float fallbackY,
                       float& outX, float& outY) {
    return ResolveEyeOffsets(physicalEye, fallbackX, fallbackY, 50.0f, 50.0f, outX, outY);
}

void SetClockForTesting(uint64_t (*clockUs)()) {
    g_testClockUs.store(clockUs, std::memory_order_release);
}

}  // namespace cvr::eyegaze
