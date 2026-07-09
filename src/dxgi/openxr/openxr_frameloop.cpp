// openxr_frameloop.cpp - the inline/AER XR frame loop (PumpInlineFrame).
// Split verbatim from openxr_manager.cpp; this is an OpenXRManager method. Shared
// module state/helpers come from openxr_internal.h (inline, single instance).
#include "openxr_manager.h"
#include "openxr_internal.h"
#include "openxr_math.h"
#include "shared_slots.h"
#include "runtime_fov_correction.h"
#include <cstdarg>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <cmath>
#include <vector>
#include <utility>
#include <chrono>
#include <thread>
#include <memory>
#include <algorithm>
#include <dxgi1_4.h>

void OpenXRManager::PumpInlineFrame() {
    // When the dedicated submit thread owns the XR frame loop (AER always, or mono
    // on SteamVR), the Present thread must NOT drive xrWaitFrame/submit here: those
    // blocking fence/swapchain + compositor waits would freeze the game on the
    // Present thread (severely so under SteamVR's frame pacing). Just make sure the
    // submit thread is awake and return immediately.
    if (UseThreadedSubmit()) {
        NotifyAerThread();
        return;
    }
    // Inline mono (VDXR / non-SteamVR): single-owner handshake with a bounded (8 ms)
    // wait so a mode switch can never freeze the Present thread. If the submit thread
    // has not parked yet, skip this present (screen holds one frame) instead of
    // blocking.
    if (!AcquireFrameLoop(FrameLoopOwner::Inline, 8)) {
        return;
    }
    FrameThreadMain();
    ReleaseFrameLoop(FrameLoopOwner::Inline);
}

DWORD OpenXRManager::FrameThreadMain() {
    uint64_t monoWaitLogCounter = 0;
    uint64_t steamVrStartupWaitLogCounter = 0;
    uint64_t displayFrameIndex = 0;

    static bool s_frameStartupDone = false;
    if (!s_frameStartupDone) {
        s_frameStartupDone = true;
        Log("OpenXRManager: Inline frame pump started.\n");
        // Try to restore the user's saved VRIK calibration once on startup. If no file,
        // seed m_calib[] with plugin defaults so later calibration stays sensible.
        if (!LoadCalibrationFromFile()) {
            m_calib[0].store(1.05f, std::memory_order_relaxed);
            m_calib[1].store(1.06f, std::memory_order_relaxed);
            m_calib[4].store(1.0f,  std::memory_order_relaxed);
            m_calib[5].store(1.0f,  std::memory_order_relaxed);
            m_calib[9].store(-90.0f,  std::memory_order_relaxed); // wRy
            m_calib[11].store(-180.0f,std::memory_order_relaxed); // wLp
            m_calib[12].store(-90.0f, std::memory_order_relaxed); // wLy
        }
    }

    constexpr float kPi = 3.1415926535f;
    auto clamp01 = [](float v) {
        if (v < 0.0f) return 0.0f;
        if (v > 1.0f) return 1.0f;
        return v;
    };
    auto smoothStep01 = [&](float v) {
        const float x = clamp01(v);
        return x * x * (3.0f - 2.0f * x);
    };
    auto quatAngleRad = [](const XrQuaternionf& a, const XrQuaternionf& b) {
        float dot = a.x * b.x + a.y * b.y + a.z * b.z + a.w * b.w;
        if (dot < 0.0f) dot = -dot;
        if (dot > 1.0f) dot = 1.0f;
        return 2.0f * acosf(dot);
    };
    auto normalizeAngle = [&](float angle) {
        while (angle > kPi) angle -= 2.0f * kPi;
        while (angle < -kPi) angle += 2.0f * kPi;
        return angle;
    };
    auto adaptiveFollow = [&](float strength, float delta, float quiet, float release) {
        if (strength <= 0.001f || release <= quiet) {
            return 1.0f;
        }
        const float stillFollow = 1.0f / (1.0f + 20.0f * strength);
        const float motion = smoothStep01((delta - quiet) / (release - quiet));
        return stillFollow + (1.0f - stillFollow) * motion;
    };
    auto resetTrackingPose = [](auto& state, const XrPosef& pose) {
        state.initialized = true;
        state.position = pose.position;
        state.orientation = pose.orientation;
    };
    auto filterTrackingPose = [&](auto& state,
                                  const XrPosef& rawPose,
                                  float strength,
                                  float quietPosMeters,
                                  float releasePosMeters,
                                  float quietAngleRad,
                                  float releaseAngleRad) {
        if (!state.initialized || strength <= 0.001f) {
            resetTrackingPose(state, rawPose);
            return rawPose;
        }

        const float dx = rawPose.position.x - state.position.x;
        const float dy = rawPose.position.y - state.position.y;
        const float dz = rawPose.position.z - state.position.z;
        const float posDelta = sqrtf(dx * dx + dy * dy + dz * dz);
        const float angDelta = quatAngleRad(state.orientation, rawPose.orientation);
        const float posT = adaptiveFollow(strength, posDelta, quietPosMeters, releasePosMeters);
        const float angT = adaptiveFollow(strength, angDelta, quietAngleRad, releaseAngleRad);

        state.position.x += dx * posT;
        state.position.y += dy * posT;
        state.position.z += dz * posT;
        state.orientation = NlerpQuat(state.orientation, rawPose.orientation, angT);

        XrPosef filtered = rawPose;
        filtered.position = state.position;
        filtered.orientation = state.orientation;
        return filtered;
    };
    auto resetTrackingAngle = [](auto& state, float angleRad) {
        state.initialized = true;
        state.angleRad = angleRad;
    };
    auto filterTrackingAngle = [&](auto& state,
                                   float rawAngleRad,
                                   float strength,
                                   float quietAngleRad,
                                   float releaseAngleRad) {
        rawAngleRad = normalizeAngle(rawAngleRad);
        if (!state.initialized || strength <= 0.001f) {
            resetTrackingAngle(state, rawAngleRad);
            return rawAngleRad;
        }

        const float delta = normalizeAngle(rawAngleRad - state.angleRad);
        const float angleT = adaptiveFollow(strength, fabsf(delta), quietAngleRad, releaseAngleRad);
        state.angleRad = normalizeAngle(state.angleRad + delta * angleT);
        return state.angleRad;
    };

    if (m_stopFrameThread.load(std::memory_order_relaxed)) return 0;
    do {
        PollEvents();
        TickAutoCalibration();

        if (!m_sessionRunning.load(std::memory_order_relaxed)) {
            Sleep(10);
            continue;
        }

        if (GetXrRuntimeMode() == 1) {
            uint32_t startupWidth = 0;
            uint32_t startupHeight = 0;
            uint32_t startupFormat = 0;
            {
                std::lock_guard<std::mutex> lock(m_presentMutex);
                startupWidth = m_lastPresentedWidth;
                startupHeight = m_lastPresentedHeight;
                startupFormat = m_lastPresentedFormat;
            }
            if (startupWidth == 0 || startupHeight == 0 || startupFormat == 0) {
                if (g_verboseLog && ((++steamVrStartupWaitLogCounter % 300) == 1)) {
                    Log("OpenXRManager: SteamVR startup wait. Deferring frame loop until first present provides a backbuffer. width=%u height=%u format=%u\n",
                        startupWidth,
                        startupHeight,
                        startupFormat);
                }
                Sleep(1);
                continue;
            }
        }

        // Half-rate submit — two modes:
        //  * AER V2 ON: do NOT skip. The engine is HMD-paced
        //    to ~90 Hz via the m_frameSyncEvent wait in OnPresent (one eye capture
        //    per vsync, alternating), and on EVERY display interval the submit
        //    block below sends the mod's OWN NvOF-synthesized frame for the stale
        //    eye warped to the fresh predicted pose -- no runtime SSW needed.
        //  * AER V2 OFF (legacy): skip the in-between interval so the runtime's
        //    own SSW/ASW fills it when available. Only gates on an already-
        //    submitted complete pair, so
        //    startup / mono fallback run normally.
        if (IsAERSubmitEnabled() &&
            m_monoSubmitEnabled.load(std::memory_order_relaxed) &&
            GetAERHalfRate() != 0 &&
            GetMenuRectMode() == 0 && GetMenuMode() == 0) {
            uint64_t currentPair = 0;
            uint64_t inBetweenReady = 0;
            uint64_t inBetweenShown = 0;
            {
                std::lock_guard<std::mutex> lock(m_presentMutex);
                if (m_capturedEyeFrames[0].pairId != 0 &&
                    m_capturedEyeFrames[0].pairId == m_capturedEyeFrames[1].pairId) {
                    currentPair = m_capturedEyeFrames[0].pairId;
                }
                inBetweenReady = m_inBetweenReadyPairId;
                inBetweenShown = m_inBetweenShownForPairId;
            }
            if (GetAERV2Enabled() != 0) {
                // UNIFIED PRODUCER: do NOT gate on capture novelty. Every xrWaitFrame
                // tick (90 Hz) we re-warp the latest captures to a fresh target time
                // with a new continuous QPC blend, so each eye is updated every display
                // interval even between real captures (45 -> 90). NvOF flow is cached
                // per (prev,curr) pair (AerV2Pipeline::RunNvOF), so only the cheap warp
                // re-runs on synth-only ticks; xrWaitFrame itself paces the loop.
                (void)inBetweenReady; (void)inBetweenShown; (void)currentPair;
            } else {
                if (currentPair != 0 && currentPair == m_lastSubmittedPairId) {
                    if (m_frameSyncEvent) {
                        SetEvent(m_frameSyncEvent);
                    }
                    Sleep(1);
                    continue;
                }
            }
        }

        // Mono cadence gate: if no NEW successfully captured game frame exists,
        // do not keep submitting the same snapshot as a brand new XR frame. That
        // defeats runtime motion smoothing and manifests as strong ghosting/double
        // images on head turns. Instead, wait for a fresh present and let the
        // runtime see the app's true cadence.
        if (m_monoSubmitEnabled.load(std::memory_order_relaxed) &&
            !IsAERSubmitEnabled() &&
            m_monoPresentEvent) {
            uint64_t latestMonoSerial = 0;
            {
                std::lock_guard<std::mutex> lock(m_presentMutex);
                latestMonoSerial = m_monoCapturedFrame.serial;
            }
            // Never block startup: until the first successful Mono submit, the frame
            // thread must keep running so xrLocateViews populates m_views and the
            // Present hook can produce the very first mono snapshot.
            if (m_lastSubmittedSerial != 0 && latestMonoSerial == m_lastSubmittedSerial) {
                // The event can still be signaled from an ALREADY-consumed frame if the
                // thread did not actually wait on it during the fresh submit path. So do
                // not trust the event by itself: only proceed when the serial changed.
                while (!m_stopFrameThread.load(std::memory_order_relaxed)) {
                    // Bail out if AER got enabled while we were parked here. Once AER
                    // is on, OnPresent stops producing mono snapshots (mono capture is
                    // guarded by !aerEnabled), so m_monoCapturedFrame.serial freezes and
                    // this loop would wait forever -> frame thread stalls -> xrWaitFrame/
                    // xrEndFrame never run -> HMD freezes while AER pairs pile up
                    // unsubmitted. Break so the outer loop re-evaluates and takes the
                    // AER submit path.
                    if (IsAERSubmitEnabled()) {
                        break;
                    }
                    const DWORD waitRes = WaitForSingleObject(m_monoPresentEvent, 10);
                    {
                        std::lock_guard<std::mutex> lock(m_presentMutex);
                        latestMonoSerial = m_monoCapturedFrame.serial;
                    }
                    if (latestMonoSerial != 0 && latestMonoSerial != m_lastSubmittedSerial) {
                        break;
                    }
                    if (!m_sessionRunning.load(std::memory_order_relaxed)) {
                        break;
                    }
                    if (waitRes == WAIT_TIMEOUT) {
                        Sleep(1);
                    }
                }
                if (latestMonoSerial == 0 || latestMonoSerial == m_lastSubmittedSerial) {
                    if (m_frameSyncEvent) {
                        SetEvent(m_frameSyncEvent);
                    }
                    continue;
                }
            }
        }

        XrFrameWaitInfo waitInfo{XR_TYPE_FRAME_WAIT_INFO};
        XrFrameState frameState{XR_TYPE_FRAME_STATE};
        XrResult res = xrWaitFrame(m_session, &waitInfo, &frameState);
        if (XR_FAILED(res)) {
            if (m_frameSyncEvent) {
                SetEvent(m_frameSyncEvent);
            }
            Sleep(10);
            continue;
        }
        // Advance the local 90 Hz slot index (only used to
        // ping-pong the synth scratch slot + stride logs). The blendFactor itself
        // is computed from QPC capture timestamps, not this counter.
        ++displayFrameIndex;
        if (frameState.predictedDisplayPeriod > 0) {
            m_predictedDisplayPeriodNs.store(frameState.predictedDisplayPeriod, std::memory_order_relaxed);
        }

        XrFrameBeginInfo beginInfo{XR_TYPE_FRAME_BEGIN_INFO};
        xrBeginFrame(m_session, &beginInfo);

        uint32_t viewCountOutput = 0;
        const bool monoEnabled = m_monoSubmitEnabled.load(std::memory_order_relaxed);
        const bool menuRectActive = (GetMenuRectMode() != 0) || (GetMenuMode() != 0);
        // Menu closed -> drop the latched panel anchors so the NEXT menu re-anchors in
        // front of wherever the player is looking at open time.
        if (!menuRectActive) {
            m_menuAnchorValid = false;
            m_menuEyeAnchorValid = false;
            m_menuFollowing = false;
        }
        const bool aerEnabled = monoEnabled && IsAERSubmitEnabled();
        bool useAerSubmit = aerEnabled && !menuRectActive;
        const bool monoReady = monoEnabled && EnsureMonoSubmitResources() && !m_eyeSwapchains.empty();
        if (monoReady && !m_views.empty()) {
            XrViewLocateInfo viewLocateInfo{XR_TYPE_VIEW_LOCATE_INFO};
            viewLocateInfo.viewConfigurationType = XR_VIEW_CONFIGURATION_TYPE_PRIMARY_STEREO;
            viewLocateInfo.displayTime = frameState.predictedDisplayTime;
            viewLocateInfo.space = m_localSpace;

            XrViewState viewState{XR_TYPE_VIEW_STATE};
            std::lock_guard<std::mutex> viewLock(m_viewMutex);
            const XrResult locateRes = xrLocateViews(m_session, &viewLocateInfo, &viewState, static_cast<uint32_t>(m_views.size()), &viewCountOutput, m_views.data());
            if (XR_FAILED(locateRes)) {
                Log("OpenXRManager: xrLocateViews failed (res=%d)\n", locateRes);
                viewCountOutput = 0;
            } else if (viewCountOutput >= 2) {
                const float hfov0 = (m_views[0].fov.angleRight - m_views[0].fov.angleLeft) * (180.0f / 3.1415926535f);
                const float hfov1 = (m_views[1].fov.angleRight - m_views[1].fov.angleLeft) * (180.0f / 3.1415926535f);
                const float vfov0 = (m_views[0].fov.angleUp - m_views[0].fov.angleDown) * (180.0f / 3.1415926535f);
                const float vfov1 = (m_views[1].fov.angleUp - m_views[1].fov.angleDown) * (180.0f / 3.1415926535f);
                const float dx = m_views[1].pose.position.x - m_views[0].pose.position.x;
                const float dy = m_views[1].pose.position.y - m_views[0].pose.position.y;
                const float dz = m_views[1].pose.position.z - m_views[0].pose.position.z;
                const float ipd = sqrtf(dx * dx + dy * dy + dz * dz);

                m_runtimeHorizontalFovDeg.store((hfov0 + hfov1) * 0.5f, std::memory_order_relaxed);
                m_runtimeVerticalFovDeg.store((vfov0 + vfov1) * 0.5f, std::memory_order_relaxed);
                m_runtimeIpd.store(ipd, std::memory_order_relaxed);
                MaybeLogRuntimeFovDetails(
                    m_views[0].fov,
                    m_views[1].fov,
                    (hfov0 + hfov1) * 0.5f,
                    (vfov0 + vfov1) * 0.5f,
                    ipd);

                // Feed the per-eye pose predictor used by the AER V2 synth path.
                if (viewState.viewStateFlags & XR_VIEW_STATE_ORIENTATION_VALID_BIT) {
                    if (m_qpcFreq == 0) {
                        LARGE_INTEGER qf; QueryPerformanceFrequency(&qf);
                        m_qpcFreq = qf.QuadPart;
                    }
                    LARGE_INTEGER pq; QueryPerformanceCounter(&pq);
                    const double tSec = static_cast<double>(pq.QuadPart) / static_cast<double>(m_qpcFreq);
                    for (int eye = 0; eye < 2 && eye < static_cast<int>(m_views.size()); ++eye) {
                        m_posePredictor[eye].AddSample(tSec, m_views[eye].pose);
                    }
                }
            }
        }

        XrSpaceVelocity headVelocity{XR_TYPE_SPACE_VELOCITY};
        XrSpaceLocation location{XR_TYPE_SPACE_LOCATION};
        location.next = &headVelocity;
        res = xrLocateSpace(m_viewSpace, m_localSpace, frameState.predictedDisplayTime, &location);
        const bool headPoseLocated = XR_SUCCEEDED(res) &&
            (location.locationFlags & XR_SPACE_LOCATION_POSITION_VALID_BIT) &&
            (location.locationFlags & XR_SPACE_LOCATION_ORIENTATION_VALID_BIT);
        if (headPoseLocated) {
            XrPosef basePose{};
            bool baseReset = false;
            {
                std::lock_guard<std::mutex> renderLock(m_renderPoseMutex);
                if (!m_basePoseSet || m_recenterRequested.exchange(false, std::memory_order_relaxed)) {
                    // YAW-ONLY BASE (native-VR recenter semantics). The old code captured the
                    // FULL HMD orientation -- whatever pitch/roll the user's head held at that
                    // moment got baked into the base, and conj(base)*pose then TILTED THE WORLD
                    // HORIZON for the whole session (recenter while glancing down = permanently
                    // sloped world). OpenXR runtimes recenter LOCAL space around gravity only
                    // (yaw); do exactly that: keep the position, extract only the heading.
                    // XR axes: X right, Y up, -Z forward.
                    const XrQuaternionf q = location.pose.orientation;
                    XrVector3f fwd = RotateVector(q, XrVector3f{0.0f, 0.0f, -1.0f});
                    float hx = fwd.x, hz = fwd.z;
                    if (hx*hx + hz*hz < 1e-6f) {
                        // Looking straight up/down: the head's UP vector projects onto the
                        // horizontal heading instead (its horizontal part points where the
                        // face would point when leveled).
                        XrVector3f up = RotateVector(q, XrVector3f{0.0f, 1.0f, 0.0f});
                        hx = (fwd.y < 0.0f) ? up.x : -up.x;
                        hz = (fwd.y < 0.0f) ? up.z : -up.z;
                    }
                    const float yaw = atan2f(-hx, -hz);
                    m_basePose.position = location.pose.position;
                    m_basePose.orientation = XrQuaternionf{0.0f, sinf(yaw*0.5f), 0.0f, cosf(yaw*0.5f)};
                    m_basePoseSet = true;
                    baseReset = true;
                    Log("OpenXRManager: Base pose captured (yaw-only, %.1f deg).\n", yaw * 57.29578f);
                }
                basePose = m_basePose;
            }

            XrQuaternionf baseInv = ConjugateQuat(basePose.orientation);
            XrVector3f relPosWorld{};
            relPosWorld.x = location.pose.position.x - basePose.position.x;
            relPosWorld.y = location.pose.position.y - basePose.position.y;
            relPosWorld.z = location.pose.position.z - basePose.position.z;
            XrVector3f relPos = RotateVector(baseInv, relPosWorld);
            XrQuaternionf relOri = MultiplyQuat(baseInv, location.pose.orientation);
            XrPosef filteredHeadPose{};
            filteredHeadPose.position = relPos;
            filteredHeadPose.orientation = relOri;
            if (baseReset) {
                m_headFilterState.initialized = false;
                m_handAimYawFilter[0].initialized = false;
                m_handAimYawFilter[1].initialized = false;
                // Recenter (or first base capture) -> re-anchor the menu panel so it
                // snaps back dead-center in front of the new forward.
                m_menuAnchorValid = false;
                m_menuEyeAnchorValid = false;
                m_menuFollowing = false;
            }
            filteredHeadPose = filterTrackingPose(
                m_headFilterState,
                filteredHeadPose,
                GetHmdTrackingSmooth(),
                0.0012f,
                0.0080f,
                0.0035f,
                0.0350f);

            m_posX.store(filteredHeadPose.position.x, std::memory_order_relaxed);
            m_posY.store(filteredHeadPose.position.y, std::memory_order_relaxed);
            m_posZ.store(filteredHeadPose.position.z, std::memory_order_relaxed);
            m_oriX.store(filteredHeadPose.orientation.x, std::memory_order_relaxed);
            m_oriY.store(filteredHeadPose.orientation.y, std::memory_order_relaxed);
            m_oriZ.store(filteredHeadPose.orientation.z, std::memory_order_relaxed);
            m_oriW.store(filteredHeadPose.orientation.w, std::memory_order_relaxed);
            m_poseValid.store(true, std::memory_order_relaxed);

            // [HANDS] Sync actions and locate hands
            static int s_handLogCounter = 0;
            bool doHandLog = g_verboseLog && (s_handLogCounter++ % 120 == 0);

            if (m_actionSet != XR_NULL_HANDLE) {
                XrActiveActionSet activeActionSet{};
                activeActionSet.actionSet = m_actionSet;
                activeActionSet.subactionPath = XR_NULL_PATH;

                XrActionsSyncInfo syncInfo{XR_TYPE_ACTIONS_SYNC_INFO};
                syncInfo.countActiveActionSets = 1;
                syncInfo.activeActionSets = &activeActionSet;
                XrResult syncRes = xrSyncActions(m_session, &syncInfo);
                
                if (doHandLog) {
                    Log("OpenXRManager[Hands]: syncRes=%d sessionState=%d\n", syncRes, (int)m_sessionState);
                }

                // Build a fresh controller snapshot for the XInput merge. Only used
                // when the gameplay-input kill switch is on; otherwise we stay byte-
                // for-byte identical to the pre-Controls-tab behaviour.
                const bool gameplayInputActive = (GetInputActionsEnabled() != 0) && (m_thumbstickAction != XR_NULL_HANDLE);
                VRControllerState ctrl{};
                // D-PAD chord state (left hand is processed first, right second):
                // HOLD the LEFT stick click, pick the direction with the RIGHT stick.
                bool leftStickClicked  = false;
                bool dpadUsedThisFrame = false;

                std::lock_guard<std::mutex> handLock(m_handMutex);
                for (int i = 0; i < 2; i++) {
                    XrActionStateGetInfo getInfo{XR_TYPE_ACTION_STATE_GET_INFO};
                    getInfo.action = m_handPoseAction;
                    getInfo.subactionPath = m_handPaths[i];

                    XrActionStatePose poseState{XR_TYPE_ACTION_STATE_POSE};
                    XrResult poseRes = xrGetActionStatePose(m_session, &getInfo, &poseState);

                    if (doHandLog) {
                        Log("OpenXRManager[Hands]: eye=%d poseRes=%d isActive=%d\n", i, poseRes, poseState.isActive);
                    }

                    m_hands[i].valid = false;
                    bool poseValid = false;
                    XrQuaternionf handRelOri{0,0,0,1};
                    if (poseState.isActive) {
                        XrSpaceLocation handLoc{XR_TYPE_SPACE_LOCATION};
                        XrResult locRes = xrLocateSpace(m_handSpaces[i], m_localSpace, frameState.predictedDisplayTime, &handLoc);

                        if (doHandLog) {
                            Log("OpenXRManager[Hands]: eye=%d locRes=%d flags=0x%X\n", i, locRes, handLoc.locationFlags);
                        }

                        if (XR_SUCCEEDED(locRes)) {
                            if ((handLoc.locationFlags & XR_SPACE_LOCATION_POSITION_VALID_BIT) &&
                                (handLoc.locationFlags & XR_SPACE_LOCATION_ORIENTATION_VALID_BIT)) {

                                XrQuaternionf headInv = ConjugateQuat(location.pose.orientation);
                                XrVector3f hrelPosWorld{};
                                hrelPosWorld.x = handLoc.pose.position.x - location.pose.position.x;
                                hrelPosWorld.y = handLoc.pose.position.y - location.pose.position.y;
                                hrelPosWorld.z = handLoc.pose.position.z - location.pose.position.z;
                                XrVector3f hrelPos = RotateVector(headInv, hrelPosWorld);
                                XrQuaternionf hrelOri = MultiplyQuat(headInv, handLoc.pose.orientation);

                                XrPosef filteredHandPose{};
                                filteredHandPose.position = hrelPos;
                                filteredHandPose.orientation = hrelOri;
                                filteredHandPose = filterTrackingPose(
                                    m_handFilterState[i],
                                    filteredHandPose,
                                    GetHandTrackingSmooth(),
                                    0.0018f,
                                    0.0120f,
                                    0.0050f,
                                    0.0450f);

                                m_hands[i].posX = filteredHandPose.position.x;
                                m_hands[i].posY = filteredHandPose.position.y;
                                m_hands[i].posZ = filteredHandPose.position.z;
                                m_hands[i].oriX = filteredHandPose.orientation.x;
                                m_hands[i].oriY = filteredHandPose.orientation.y;
                                m_hands[i].oriZ = filteredHandPose.orientation.z;
                                m_hands[i].oriW = filteredHandPose.orientation.w;
                                m_hands[i].valid = true;
                                poseValid = true;

                                if (gameplayInputActive) {
                                    // Yaw of the controller relative to the recenter base
                                    // (= body forward). Used by hand-oriented locomotion.
                                    handRelOri = MultiplyQuat(baseInv, handLoc.pose.orientation);
                                }
                            }
                        }
                    }
                    if (!poseValid) {
                        m_handFilterState[i].initialized = false;
                    }

                    if (!gameplayInputActive) continue; // legacy pose-only path, no new bookkeeping

                    if (i == 0) ctrl.leftHandValid  = poseValid;
                    else        ctrl.rightHandValid = poseValid;

                    // Aim-pose yaw -- this is where the controller POINTS, not
                    // where the palm faces. grip-pose -Z is "away from palm",
                    // which is MIRRORED between left and right hands and gave
                    // inverted/diverging locomotion direction.
                    bool aimYawValid = false;
                    if (poseValid && m_handAimSpaces[i] != XR_NULL_HANDLE) {
                        XrSpaceLocation aimLoc{XR_TYPE_SPACE_LOCATION};
                        if (XR_SUCCEEDED(xrLocateSpace(m_handAimSpaces[i], m_localSpace, frameState.predictedDisplayTime, &aimLoc)) &&
                            (aimLoc.locationFlags & XR_SPACE_LOCATION_ORIENTATION_VALID_BIT)) {
                            const XrQuaternionf q = MultiplyQuat(baseInv, aimLoc.pose.orientation);
                            // Same yaw extraction as GetHmdYawRelToBody so both
                            // HMD-locomotion and Hand-locomotion use the SAME
                            // sign convention. atan2(fwd.x, -fwd.z) was sign-
                            // inverted relative to this and produced mirrored
                            // walking direction.
                            const float yaw = std::atan2(2.0f * (q.w * q.y + q.x * q.z),
                                                         1.0f - 2.0f * (q.y * q.y + q.z * q.z));
                            const float filteredYaw = filterTrackingAngle(
                                m_handAimYawFilter[i],
                                yaw,
                                GetHandTrackingSmooth(),
                                0.0040f,
                                0.0800f);
                            m_handYawRelToBody[i].store(filteredYaw, std::memory_order_relaxed);
                            m_handYawValid[i].store(true, std::memory_order_relaxed);
                            aimYawValid = true;
                        }
                    }
                    if (!aimYawValid) {
                        m_handAimYawFilter[i].initialized = false;
                        m_handYawValid[i].store(false, std::memory_order_relaxed);
                    }

                    // -- Gameplay inputs (trigger/grip/stick/buttons) --
                    if (m_thumbstickAction == XR_NULL_HANDLE) continue;
                    auto getFloat = [&](XrAction a) -> float {
                        XrActionStateGetInfo gi{XR_TYPE_ACTION_STATE_GET_INFO};
                        gi.action = a;
                        gi.subactionPath = m_handPaths[i];
                        XrActionStateFloat st{XR_TYPE_ACTION_STATE_FLOAT};
                        if (XR_SUCCEEDED(xrGetActionStateFloat(m_session, &gi, &st)) && st.isActive)
                            return st.currentState;
                        return 0.0f;
                    };
                    auto getBool = [&](XrAction a) -> bool {
                        XrActionStateGetInfo gi{XR_TYPE_ACTION_STATE_GET_INFO};
                        gi.action = a;
                        gi.subactionPath = m_handPaths[i];
                        XrActionStateBoolean st{XR_TYPE_ACTION_STATE_BOOLEAN};
                        if (XR_SUCCEEDED(xrGetActionStateBoolean(m_session, &gi, &st)) && st.isActive)
                            return st.currentState != XR_FALSE;
                        return false;
                    };
                    auto getVec2 = [&](XrAction a, float& outX, float& outY) {
                        XrActionStateGetInfo gi{XR_TYPE_ACTION_STATE_GET_INFO};
                        gi.action = a;
                        gi.subactionPath = m_handPaths[i];
                        XrActionStateVector2f st{XR_TYPE_ACTION_STATE_VECTOR2F};
                        if (XR_SUCCEEDED(xrGetActionStateVector2f(m_session, &gi, &st)) && st.isActive) {
                            outX = st.currentState.x;
                            outY = st.currentState.y;
                        } else {
                            outX = 0.0f;
                            outY = 0.0f;
                        }
                    };

                    const float trig = getFloat(m_triggerAction);
                    const float grip = getFloat(m_gripAction);
                    float sx = 0.0f, sy = 0.0f;
                    getVec2(m_thumbstickAction, sx, sy);
                    const bool sclick = getBool(m_thumbstickClickAction);
                    const bool prim   = getBool(m_primaryButtonAction);
                    const bool sec    = getBool(m_secondaryButtonAction);

                    // XInput-compatible button bits so the hook can OR them into
                    // XINPUT_GAMEPAD.wButtons directly (XINPUT_GAMEPAD_*).
                    constexpr uint16_t XB_A              = 0x1000;
                    constexpr uint16_t XB_B              = 0x2000;
                    constexpr uint16_t XB_X              = 0x4000;
                    constexpr uint16_t XB_Y              = 0x8000;
                    constexpr uint16_t XB_LEFT_SHOULDER  = 0x0100;
                    constexpr uint16_t XB_RIGHT_SHOULDER = 0x0200;
                    constexpr uint16_t XB_LEFT_THUMB     = 0x0040;
                    constexpr uint16_t XB_RIGHT_THUMB    = 0x0080;
                    constexpr uint16_t XB_DPAD_UP        = 0x0001;
                    constexpr uint16_t XB_DPAD_DOWN      = 0x0002;
                    constexpr uint16_t XB_DPAD_LEFT     = 0x0004;
                    constexpr uint16_t XB_DPAD_RIGHT     = 0x0008;
                    (void)XB_LEFT_THUMB;

                    if (i == 0) {
                        ctrl.leftTrigger = trig;
                        ctrl.leftGrip    = grip;
                        ctrl.leftThumbX  = sx;
                        ctrl.leftThumbY  = sy;
                        if (prim)   ctrl.buttons |= XB_X;
                        if (sec)    ctrl.buttons |= XB_Y;
                        if (grip >= 0.7f) ctrl.buttons |= XB_LEFT_SHOULDER;

                        // LEFT stick click = D-Pad modifier (direction picked with the
                        // RIGHT stick, see the right-hand branch). The vanilla L3
                        // (sprint) is emitted DEFERRED, after the loop: only when the
                        // click is released without a D-Pad direction having been used.
                        leftStickClicked = sclick;
                    } else {
                        ctrl.rightTrigger = trig;
                        ctrl.rightGrip    = grip;
                        ctrl.rightThumbX  = sx;
                        ctrl.rightThumbY  = sy;
                        if (sclick) ctrl.buttons |= XB_RIGHT_THUMB;
                        if (prim)   ctrl.buttons |= XB_A;
                        if (sec)    ctrl.buttons |= XB_B;

                        // D-PAD CHORD: while the LEFT stick click is held, the RIGHT
                        // stick picks the D-Pad direction. The right axes are zeroed for
                        // the whole hold so snap-turn/camera cannot fire during selection.
                        if (leftStickClicked) {
                            constexpr float threshold = 0.5f;
                            if (sy > threshold)  { ctrl.buttons |= XB_DPAD_UP;    dpadUsedThisFrame = true; }
                            if (sy < -threshold) { ctrl.buttons |= XB_DPAD_DOWN;  dpadUsedThisFrame = true; }
                            if (sx < -threshold) { ctrl.buttons |= XB_DPAD_LEFT;  dpadUsedThisFrame = true; }
                            if (sx > threshold)  { ctrl.buttons |= XB_DPAD_RIGHT; dpadUsedThisFrame = true; }
                            ctrl.rightThumbX = 0.0f;
                            ctrl.rightThumbY = 0.0f;
                        }


                        // Right grip is RESERVED for the hand-to-holster equip system: a CET mod reads
                        // the grip value (shared[31] or similar) and the controller pose, and equips the
                        // weapon whose visual holster the hand is touching. Do NOT merge into XInput as
                        // ThrowGrenade — that would fire a grenade every time the player reaches for a
                        // holstered weapon.
                    }
                }

                // DEFERRED L3 (sprint): the left stick click doubles as the D-Pad
                // modifier. Emit the vanilla stick-click press only when the click is
                // RELEASED without any D-Pad direction having been used during the hold
                // (one-frame press; the game latches button edges per poll).
                {
                    static bool s_l3Held = false;
                    static bool s_l3UsedForDpad = false;
                    if (leftStickClicked) {
                        if (dpadUsedThisFrame) s_l3UsedForDpad = true;
                        s_l3Held = true;
                    } else {
                        if (s_l3Held && !s_l3UsedForDpad)
                            ctrl.buttons |= 0x0040;   // XINPUT_GAMEPAD_LEFT_THUMB
                        s_l3Held = false;
                        s_l3UsedForDpad = false;
                    }
                }

                if (gameplayInputActive) {
                    // Menu button is single (no per-hand binding) on Touch/Index/Vive/WMR.
                    if (m_menuButtonAction != XR_NULL_HANDLE) {
                        XrActionStateGetInfo gi{XR_TYPE_ACTION_STATE_GET_INFO};
                        gi.action = m_menuButtonAction;
                        gi.subactionPath = XR_NULL_PATH;
                        XrActionStateBoolean st{XR_TYPE_ACTION_STATE_BOOLEAN};
                        if (XR_SUCCEEDED(xrGetActionStateBoolean(m_session, &gi, &st)) && st.isActive && st.currentState)
                            ctrl.buttons |= 0x0010; // XINPUT_GAMEPAD_START
                    }

                    // Publish the snapshot for the XInput hook.
                    std::lock_guard<std::mutex> inLock(m_inputMutex);
                    m_controllerState = ctrl;
                }
            }

            // Rotate the head velocity into the same base-recentered frame as the
            // pose so GetHeadPose() can forward-predict (AER pose extrapolation).
            const bool angVelValid = (headVelocity.velocityFlags & XR_SPACE_VELOCITY_ANGULAR_VALID_BIT) != 0;
            const bool linVelValid = (headVelocity.velocityFlags & XR_SPACE_VELOCITY_LINEAR_VALID_BIT) != 0;
            if (angVelValid && linVelValid) {
                const XrVector3f angRel = RotateVector(baseInv, headVelocity.angularVelocity);
                const XrVector3f linRel = RotateVector(baseInv, headVelocity.linearVelocity);
                m_angVelX.store(angRel.x, std::memory_order_relaxed);
                m_angVelY.store(angRel.y, std::memory_order_relaxed);
                m_angVelZ.store(angRel.z, std::memory_order_relaxed);
                m_linVelX.store(linRel.x, std::memory_order_relaxed);
                m_linVelY.store(linRel.y, std::memory_order_relaxed);
                m_linVelZ.store(linRel.z, std::memory_order_relaxed);
                m_velValid.store(true, std::memory_order_relaxed);
            } else {
                m_velValid.store(false, std::memory_order_relaxed);
            }
        } else {
            m_headFilterState.initialized = false;
            m_velValid.store(false, std::memory_order_relaxed);
        }

        if (monoReady && viewCountOutput == m_eyeSwapchains.size()) {
            if (useAerSubmit) {
                ID3D12Resource* eyeSources[2] = {};
                ID3D12Resource* generatedSources[2] = {};  // synth texture per synth eye (null = raw)
                uint64_t eyeSerials[2] = {};
                uint64_t eyePairIds[2] = {};
                XrPosef eyePoses[2]{};
                XrFovf eyeFovs[2]{};
                bool eyeHasView[2] = {};
                ID3D12Resource* depthSource = nullptr;
                ID3D12Resource* r32depth = nullptr;   // CUDA-importable scene depth for the warp
                bool eyeIsSynth[2] = {false, false};
                bool anyEyeSynth = false;
                const uint32_t synthSlot = static_cast<uint32_t>(displayFrameIndex & 1ull);
                bool useInBetweenPair = false;
                uint32_t activeInBetweenSlot = 0;

                // ===== Frame-thread synth =====
                // Per eye, compute the CONTINUOUS blendFactor v24 from HIGH-RES QPC
                // timestamps (microsecond precision, signed math = no underflow):
                //   blend = (targetQpc - tPrev) / (tCurr - tPrev)
                // where tPrev/tCurr are the QPC of this eye's previous/current real
                // captures and targetQpc is the time this frame will be displayed.
                // blend≈1 -> raw current; ≈0 -> raw previous; (0,1) -> NvOF temporal
                // warp at that fraction so the synth lands exactly on display time
                // (no hardcoded 0.5 -> no head-rotation jitter). The warp is queued
                // AFTER the lock (ProcessTemporalFrame is non-blocking now).
                bool doSynth[2] = {false, false};
                float synthBlend[2] = {1.0f, 1.0f};
                float poseTargetBlend[2] = {1.0f, 1.0f};
                ID3D12Resource* sPrevTex[2] = {}, *sCurrTex[2] = {};
                ID3D12Resource* sPrevFlow[2] = {}, *sCurrFlow[2] = {};
                uint64_t sFlowConvVal[2] = {0, 0};  // max convert-fence value to GPU-Wait before NvOF
                ID3D12Resource* sPrevDepth[2] = {}, *sCurrDepth[2] = {};
                XrPosef sPrevPose[2]{}, sCurrPose[2]{}, sPredPose[2]{};
                XrFovf sFov[2]{};
                uint32_t sW[2] = {}, sH[2] = {};
                const bool v2 = (GetAERV2Enabled() != 0);

                // ANCHOR-TO-CAPTURE model (kills async present<->frame drift).
                // The engine renders mono alternate-eye, so each display frame one
                // eye is FRESH and the other is STALE by ~one capture interval. We
                // set the target time to the FRESHEST capture (max captureQpc), then:
                //   fresh eye  (tCurr == target)        -> blend≈1 -> raw
                //   stale eye  (tCurr <  target)        -> blend>1 -> FORWARD-warp it
                //                                          to the fresh eye's time.
                // Both eyes therefore represent the SAME instant -> coherent stereo,
                // and the target is anchored to real captures (no now+period drift,
                // no missed window). xr_pose_lag adds optional forward lead (periods)
                // to push the pair slightly toward actual display time.
                LARGE_INTEGER qpcFreq{};
                QueryPerformanceFrequency(&qpcFreq);
                uint64_t periodNs = m_predictedDisplayPeriodNs.load(std::memory_order_relaxed);
                if (periodNs == 0) periodNs = 11111111ull;  // 90 Hz default
                const int64_t periodTicks = (static_cast<int64_t>(periodNs) * qpcFreq.QuadPart) / 1000000000ll;
                // Forward lead in periods. Default xr_pose_lag=1 -> lead=0 so the
                // FRESH eye stays raw (target == its capture -> blend=1) and only
                // the STALE eye warps. Raising xr_pose_lag pushes the pair forward
                // toward real display time (more extrapolation, less latency).
                int lead = GetPoseLag() - 1; if (lead < 0) lead = 0;

                uint64_t capQpc[2] = {0, 0}, prevQpc[2] = {0, 0};
                bool haveHist[2] = {false, false};
                int64_t dbgBlendMilli[2] = {1000, 1000};
                int64_t dbgSpanUs[2] = {0, 0};

                {
                    std::lock_guard<std::mutex> lock(m_presentMutex);
                    for (int eye = 0; eye < 2; ++eye) {
                        eyeSerials[eye] = m_capturedEyeFrames[eye].serial;
                        eyePairIds[eye] = m_capturedEyeFrames[eye].pairId;
                        eyeHasView[eye] = m_capturedEyeFrames[eye].hasView;
                        capQpc[eye]  = m_capturedEyeFrames[eye].captureQpc;
                        prevQpc[eye] = m_previousCapturedEyeFrames[eye].captureQpc;
                        // Default: raw current capture (also the synth fallback).
                        if (m_capturedEyeFrames[eye].texture) {
                            eyeSources[eye] = m_capturedEyeFrames[eye].texture;
                            eyeSources[eye]->AddRef();
                            eyePoses[eye] = m_capturedEyeFrames[eye].pose;
                            eyeFovs[eye] = m_capturedEyeFrames[eye].fov;
                        }
                        // Unified producer: this is now the single synth producer for
                        // AER V2. The old OnPresent /
                        // ProcessAERV2Job worker producers are disabled for V2 (so no
                        // race), and OnPresent only captures + converts the per-eye
                        // NvOF input texture. Every 90 Hz display tick we warp each
                        // eye's latest real capture to ONE common target time with a
                        // continuous QPC blend (computed below), so both eyes land on
                        // the same instant. (v2 used to `continue` here, which left
                        // zero active producers = raw alternate-eye submit.)
                        // Snapshot the warp inputs unconditionally (decide raw vs
                        // synth AFTER the lock once the target is known).
                        haveHist[eye] =
                            m_previousCapturedEyeFrames[eye].texture && m_capturedEyeFrames[eye].texture &&
                            m_previousCapturedEyeFrames[eye].opticalFlowTexture &&
                            m_capturedEyeFrames[eye].opticalFlowTexture &&
                            m_capturedEyeFrames[eye].hasView && m_previousCapturedEyeFrames[eye].hasView &&
                            m_aerV2SynthEye[eye][synthSlot] != nullptr;
                        if (haveHist[eye]) {
                            sPrevTex[eye]  = m_previousCapturedEyeFrames[eye].texture;
                            sCurrTex[eye]  = m_capturedEyeFrames[eye].texture;
                            sPrevFlow[eye] = m_previousCapturedEyeFrames[eye].opticalFlowTexture;
                            sCurrFlow[eye] = m_capturedEyeFrames[eye].opticalFlowTexture;
                            // GPU-Wait target: both flow inputs must have finished their
                            // fire-and-forget convert before NvOF reads them.
                            {
                                const uint64_t pv = m_previousCapturedEyeFrames[eye].opticalFlowConvertValue;
                                const uint64_t cv = m_capturedEyeFrames[eye].opticalFlowConvertValue;
                                sFlowConvVal[eye] = pv > cv ? pv : cv;
                            }
                            if (m_previousCapturedEyeFrames[eye].depthTexture &&
                                m_previousCapturedEyeFrames[eye].depthSerial == m_previousCapturedEyeFrames[eye].serial)
                                sPrevDepth[eye] = m_previousCapturedEyeFrames[eye].depthTexture;
                            if (m_capturedEyeFrames[eye].depthTexture &&
                                m_capturedEyeFrames[eye].depthSerial == m_capturedEyeFrames[eye].serial)
                                sCurrDepth[eye] = m_capturedEyeFrames[eye].depthTexture;
                            sPrevPose[eye] = m_previousCapturedEyeFrames[eye].pose;
                            sCurrPose[eye] = m_capturedEyeFrames[eye].pose;
                            sFov[eye] = m_capturedEyeFrames[eye].fov;
                            sW[eye] = m_capturedEyeFrames[eye].width;
                            sH[eye] = m_capturedEyeFrames[eye].height;
                            sPrevTex[eye]->AddRef();  sCurrTex[eye]->AddRef();
                            sPrevFlow[eye]->AddRef(); sCurrFlow[eye]->AddRef();
                            if (sPrevDepth[eye]) sPrevDepth[eye]->AddRef();
                            if (sCurrDepth[eye]) sCurrDepth[eye]->AddRef();
                        }
                    }
                    if (m_depthLayerSupported && m_depthSnapshot && m_depthSnapshotSerial != 0) {
                        depthSource = m_depthSnapshot;
                        depthSource->AddRef();
                    }
                    // [DEPTH-AERV2] freshest CUDA-importable R32 depth for the warp.
                    if (m_depthSnapshotR32 && m_depthSnapshotR32Serial != 0) {
                        r32depth = m_depthSnapshotR32;
                        r32depth->AddRef();
                    }
                }

                // MODE-6 PER-EYE ALTERNATE-EYE.
                //
                // Each 90 Hz vsync, the eye that received a FRESH capture this
                // interval is submitted RAW (its captured texture @ captured pose;
                // the runtime's ATW reprojects it to display time -- same as mono).
                // The OTHER eye is NvOF-warped from its previous two SAME-EYE
                // captures (m_capturedEyeFrames[eye]/m_previousCapturedEyeFrames[eye]
                // = the per-eye source ring) to the display pose. With the engine
                // HMD-paced + the camera hook alternating, exactly one eye is fresh
                // per vsync -> each eye is real at 45 Hz, both eyes get a fresh
                // image at every 90 Hz submit.
                //
                // Replaces the old global isRealTick/synthTick pair-gate which
                // submitted BOTH eyes raw on a real tick (one up to ~22ms stale ->
                // temporal eye mismatch when moving) and BOTH warped on a synth
                // tick. Mode-6 always has one fresh + one warped -> no pair mismatch.
                static thread_local uint64_t s_lastEyePair[2] = {0, 0};
                LARGE_INTEGER mode6NowQpc; QueryPerformanceCounter(&mode6NowQpc);
                const int64_t mode6TargetQpc = mode6NowQpc.QuadPart + periodTicks +
                                                static_cast<int64_t>(lead) * periodTicks;
                // Fresh-window = 1.5 HMD periods. With the engine alternating eyes,
                // the eye captured this vsync has age~0 (fresh -> raw), the other
                // eye was captured ~2 periods ago (age>1.5 -> stale -> NvOF-warp).
                const int64_t freshWindowTicks = periodTicks + (periodTicks >> 1);
                for (int eye = 0; eye < 2; ++eye) {
                    // TIMESTAMP-BASED fresh detection (no eye0-first bias). An eye is
                    // "fresh" (raw submit) if it was captured within the fresh window.
                    // The old pairId-based test (eyePairIds[eye] > s_lastEyePair) was
                    // structurally biased: presentPairId increments ONLY on eye0
                    // capture, so eye0's pairId always led eye1's by 1, and with
                    // decoupled frame/present threads eye0 was detected "fresh" (raw,
                    // runtime-ATW-smooth) more often than eye1 (NvOF-warped, lower
                    // quality) -> the consistent "right eye worse than left" symptom.
                    // Actual capture age removes that bias: each eye gets an equal
                    // shot at being the raw eye based purely on temporal recency.
                    const int64_t ageTicks = mode6NowQpc.QuadPart - static_cast<int64_t>(capQpc[eye]);
                    const bool fresh = eyePairIds[eye] != 0 && ageTicks < freshWindowTicks;
                    s_lastEyePair[eye] = eyePairIds[eye];
                    const int64_t span =
                        static_cast<int64_t>(capQpc[eye]) - static_cast<int64_t>(prevQpc[eye]);
                    if (span > 0) dbgSpanUs[eye] = span * 1000000ll / qpcFreq.QuadPart;

                    if (!haveHist[eye] || span <= 0) {
                        // Warmup / no history: stays raw (captured texture).
                        continue;
                    }
                    // Blend to the display-target instant for THIS eye. Used as:
                    //   stale eye  -> image blend AND pose target
                    //   fresh eye  -> pose target only (image stays current-heavy)
                    // so the real eye can get a cheap pose-warp to display time.
                    double blend = static_cast<double>(mode6TargetQpc - static_cast<int64_t>(prevQpc[eye])) /
                                   static_cast<double>(span);
                    float maxExtrap = GetAerMaxExtrap();
                    if (maxExtrap < 1.0f) maxExtrap = 1.0f;
                    if (blend < 0.0) blend = 0.0;
                    if (blend > static_cast<double>(maxExtrap)) blend = static_cast<double>(maxExtrap);
                    // Small OF-overshoot bias. NvOF slightly over-extrapolates linear
                    // motion; subtract a small constant to pull the midpoint back.
                    blend -= 0.02;
                    if (blend < 0.0) blend = 0.0;
                    poseTargetBlend[eye] = static_cast<float>(blend);

                    if (fresh) {
                        // Idea #1 — lightweight real-eye improvement. When depth is
                        // enabled (GetAerXQueueWait!=0) we also run the FRESH eye
                        // through the warp pipeline, but keep imageBlend=1.0 so the
                        // CURRENT frame dominates while the pose target advances to
                        // the display instant (poseTargetBlend). This approximates a
                        // cheap pose-warp of the current frame: better left/right
                        // consistency and less ATW work, without the heavy temporal
                        // mixing of a normal stale-eye synth. When depth is off, raw
                        // + runtime ATW remains cheaper and usually better.
                        if (GetAerXQueueWait() != 0) {
                            doSynth[eye] = true;
                            synthBlend[eye] = 1.0f;
                        }
                        dbgBlendMilli[eye] = 1000;
                        continue;
                    }

                    // STALE eye: full NvOF temporal synth to the display target.
                    doSynth[eye] = true;
                    synthBlend[eye] = static_cast<float>(blend);
                    dbgBlendMilli[eye] = static_cast<int64_t>(blend * 1000.0);
                }
                // Suppress the now-unused legacy gate counters (kept for log compat).
                static thread_local uint64_t s_lastRealPairTickGate = 0; (void)s_lastRealPairTickGate;
                static thread_local int s_synthTickCount = 0; (void)s_synthTickCount;
                // Release the snapshot refs for eyes we ended up NOT synthesizing.
                for (int eye = 0; eye < 2; ++eye) {
                    if (haveHist[eye] && !doSynth[eye]) {
                        if (sPrevTex[eye]) { sPrevTex[eye]->Release(); sPrevTex[eye] = nullptr; }
                        if (sCurrTex[eye]) { sCurrTex[eye]->Release(); sCurrTex[eye] = nullptr; }
                        if (sPrevFlow[eye]) { sPrevFlow[eye]->Release(); sPrevFlow[eye] = nullptr; }
                        if (sCurrFlow[eye]) { sCurrFlow[eye]->Release(); sCurrFlow[eye] = nullptr; }
                        if (sPrevDepth[eye]) { sPrevDepth[eye]->Release(); sPrevDepth[eye] = nullptr; }
                        if (sCurrDepth[eye]) { sCurrDepth[eye]->Release(); sCurrDepth[eye] = nullptr; }
                    }
                }

                // [DLSS-MV] Copy the engine motion vectors into a CUDA-importable
                // shared scratch once per frame. Executed on m_d3dQueue so it is
                // FIFO-ordered BEFORE the pipeline's own m_d3dQueue->Signal that
                // CUDA waits on — no CPU sync needed. Failure -> mvSource stays
                // null -> kernel falls back to NvOF-only (haveMv=false).
                ID3D12Resource* mvSource = nullptr;
                float mvScaleX = NgxGetMvScaleX();
                float mvScaleY = NgxGetMvScaleY();
                if (anyEyeSynth) {
                    // Lazily create the DIRECT allocator+list used for the MV copy
                    // (the V2 worker that used to make these is disabled now).
                    if (!m_mvWarpAlloc || !m_mvWarpList) {
                        if (SUCCEEDED(m_d3dDevice->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&m_mvWarpAlloc))) &&
                            SUCCEEDED(m_d3dDevice->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, m_mvWarpAlloc.Get(), nullptr, IID_PPV_ARGS(&m_mvWarpList)))) {
                            m_mvWarpList->Close();
                            m_mvWarpAlloc->SetName(L"AERV2_mvcopy_alloc");
                            m_mvWarpList->SetName(L"AERV2_mvcopy_list");
                        } else {
                            m_mvWarpAlloc.Reset();
                            m_mvWarpList.Reset();
                        }
                    }
                    ID3D12Resource* engineMv = NgxAcquireMotionVectors();  // AddRef'd
                    if (engineMv && m_mvWarpAlloc && m_mvWarpList) {
                        const D3D12_RESOURCE_DESC mvDesc = engineMv->GetDesc();
                        bool recreate = !m_aerV2MvScratch;
                        if (m_aerV2MvScratch) {
                            const auto cur = m_aerV2MvScratch->GetDesc();
                            if (cur.Width != mvDesc.Width || cur.Height != mvDesc.Height || cur.Format != mvDesc.Format) {
                                m_aerV2MvScratch.Reset();
                                recreate = true;
                            }
                        }
                        if (recreate) {
                            D3D12_HEAP_PROPERTIES hp{}; hp.Type = D3D12_HEAP_TYPE_DEFAULT;
                            if (FAILED(m_d3dDevice->CreateCommittedResource(&hp, D3D12_HEAP_FLAG_SHARED, &mvDesc,
                                    D3D12_RESOURCE_STATE_COMMON, nullptr, IID_PPV_ARGS(&m_aerV2MvScratch)))) {
                                m_aerV2MvScratch.Reset();
                            } else {
                                SetD3DName(m_aerV2MvScratch.Get(), L"AERV2_engine_mv_scratch_shared");
                            }
                        }
                        if (m_aerV2MvScratch && SUCCEEDED(m_mvWarpAlloc->Reset()) &&
                            SUCCEEDED(m_mvWarpList->Reset(m_mvWarpAlloc.Get(), nullptr))) {
                            D3D12_RESOURCE_BARRIER b[2] = {};
                            b[0].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            b[0].Transition.pResource = engineMv;
                            b[0].Transition.StateBefore = D3D12_RESOURCE_STATE_COMMON;
                            b[0].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
                            b[0].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            b[1].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            b[1].Transition.pResource = m_aerV2MvScratch.Get();
                            b[1].Transition.StateBefore = D3D12_RESOURCE_STATE_COMMON;
                            b[1].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_DEST;
                            b[1].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            m_mvWarpList->ResourceBarrier(2, b);
                            m_mvWarpList->CopyResource(m_aerV2MvScratch.Get(), engineMv);
                            std::swap(b[0].Transition.StateBefore, b[0].Transition.StateAfter);
                            std::swap(b[1].Transition.StateBefore, b[1].Transition.StateAfter);
                            m_mvWarpList->ResourceBarrier(2, b);
                            m_mvWarpList->Close();
                            ID3D12CommandList* lists[] = { m_mvWarpList.Get() };
                            m_d3dQueue->ExecuteCommandLists(1, lists);  // FIFO before pipeline signal
                            mvSource = m_aerV2MvScratch.Get();
                        }
                    }
                    if (engineMv) engineMv->Release();
                }

                // NOTE: a previous "matched stereo" override forced BOTH submitted
                // eyes (and the warp targets) onto the freshest eye's orientation.
                // With pose-pair-locking already rendering both eyes from eye0's
                // frozen head pose, that override fought the rendered content: the
                // stale eye's pixels carry eye0's orientation but were submitted at a
                // different (freshest) orientation, so the compositor reprojected the
                // delta -> severe one-eye "orbital" tearing on head turns. Reverted:
                // each eye keeps its OWN extrapolated pose, consistent with its
                // rendered+warped content.

                // POSE/IMAGE TIME CONSISTENCY (critical for no head-turn jitter).
                // The stale eye's submitted pose and its NvOF-warped IMAGE must
                // represent the SAME temporal instant, or the image "swims" vs the
                // pose on head turns -> whole-image jitter in both eyes. We derive
                // BOTH from the same source: the time-based blend over the eye's
                // prev/curr captures. The pose is ExtrapolatePose(prev,curr,blend)
                // and the warp kernel interpolates the image at that same blend, so
                // they agree by construction. The runtime's ATW then corrects the
                // small residual from this blend instant to actual photon display
                // (a uniform, smooth correction).
                //
                // Do NOT set the submitted pose to m_views[eye].pose (the runtime's
                // predictedDisplayTime pose) while the image blend targets a
                // different instant (now+period): that desync was the head-turn
                // jitter. m_views is display-anchored, the capture-based blend is
                // capture-anchored, and without an epoch-exact XrTime->QPC map
                // they cannot be safely aligned -- so keep pose+image both
                // capture-anchored and let ATW do the rest.

                // Run the per-eye warp OUTSIDE the lock (non-blocking GPU queue).
                for (int eye = 0; eye < 2; ++eye) {
                    if (!doSynth[eye]) continue;
                    if (!m_aerV2Pipeline) m_aerV2Pipeline = std::make_unique<aer_v2::AerV2Pipeline>();
                    m_aerV2Pipeline->SetMode(aer_v2::AerV2Pipeline::Mode::AERv2HighQ);
                    m_aerV2Pipeline->SetForceMatchedEyePoses(true);
                    m_aerV2Pipeline->SetWarpTuning(GetAerRefineStrength(), GetAerOcclusionSharp(), GetAerFoveation(), GetFlowSmooth());
                    // Pose target for this warp. Normally equals synthBlend
                    // (stale eye: pose and image at the same extrapolated instant),
                    // but for Idea #1 the fresh eye can keep imageBlend=1.0
                    // (current-heavy) while still advancing its pose target toward
                    // the display instant via poseTargetBlend.
                    sPredPose[eye] = ExtrapolatePose(sPrevPose[eye], sCurrPose[eye], poseTargetBlend[eye]);
                    bool ok = false;
                    if (m_d3dDevice && m_d3dQueue && sW[eye] && sH[eye] &&
                        m_aerV2Pipeline->EnsureInitialized(m_d3dDevice, m_d3dQueue, sW[eye], sH[eye])) {
                        // GPU-Wait for the fire-and-forget flow-input conversion to
                        // finish before NvOF reads it. Issued on m_d3dQueue, which the
                        // pipeline uses for its own CUDA fence handshake, so the warp's
                        // CUDA work transitively waits for the conversion. No CPU stall.
                        if (sFlowConvVal[eye] != 0 && m_opticalFlow) {
                            if (ID3D12Fence* cf = m_opticalFlow->GetConvertFence()) {
                                m_d3dQueue->Wait(cf, sFlowConvVal[eye]);
                            }
                        }
                        ok = m_aerV2Pipeline->ProcessTemporalFrame(
                            eye,
                            sPrevFlow[eye], sCurrFlow[eye], sPrevTex[eye], sCurrTex[eye],
                            mvSource, r32depth, r32depth, mvScaleX, mvScaleY,
                            sFov[eye], sPrevPose[eye], sCurrPose[eye], sPredPose[eye],
                            synthBlend[eye], m_aerV2SynthEye[eye][synthSlot].Get());
                    }
                    if (ok) {
                        if (eyeSources[eye]) eyeSources[eye]->Release();
                        eyeSources[eye] = m_aerV2SynthEye[eye][synthSlot].Get();
                        eyeSources[eye]->AddRef();
                        generatedSources[eye] = m_aerV2SynthEye[eye][synthSlot].Get();
                        generatedSources[eye]->AddRef();
                        eyePoses[eye] = sPredPose[eye];
                        eyeFovs[eye] = sFov[eye];
                        eyeIsSynth[eye] = true;
                        anyEyeSynth = true;
                    }
                    if (sPrevTex[eye]) sPrevTex[eye]->Release();
                    if (sCurrTex[eye]) sCurrTex[eye]->Release();
                    if (sPrevFlow[eye]) sPrevFlow[eye]->Release();
                    if (sCurrFlow[eye]) sCurrFlow[eye]->Release();
                    if (sPrevDepth[eye]) sPrevDepth[eye]->Release();
                    if (sCurrDepth[eye]) sCurrDepth[eye]->Release();
                }
                const bool depthAvail = (r32depth != nullptr);
                if (r32depth) { r32depth->Release(); r32depth = nullptr; }
                if (g_verboseLog && v2 && (displayFrameIndex % 300) == 1) {
                    Log("OpenXRManager: [AER V2] depthAware=%d dlssMv=%d foveation=%.2f (mvScale=%.3f,%.3f)\n",
                        depthAvail ? 1 : 0, mvSource ? 1 : 0, GetAerFoveation(), mvScaleX, mvScaleY);
                }
                if (g_verboseLog && v2 && (displayFrameIndex % 300) == 1) {
                    // Anchor-to-capture: target = freshest capture (+lead). One eye
                    // is fresh (blend≈1 -> raw), the other is stale (blend>1 ->
                    // FORWARD-warped to match). blendMilli is blend*1000; span = that
                    // eye's real capture interval (us). Healthy: one eye ~1000, the
                    // other >1100 and synth=1.
                    Log("OpenXRManager: [AER V2] idx=%llu lead=%d blendL=%.3f blendR=%.3f synthL=%d synthR=%d | L:span=%lldus blendx1k=%lld R:span=%lldus blendx1k=%lld slot=%u\n",
                        static_cast<unsigned long long>(displayFrameIndex), lead,
                        synthBlend[0], synthBlend[1], eyeIsSynth[0] ? 1 : 0, eyeIsSynth[1] ? 1 : 0,
                        static_cast<long long>(dbgSpanUs[0]), static_cast<long long>(dbgBlendMilli[0]),
                        static_cast<long long>(dbgSpanUs[1]), static_cast<long long>(dbgBlendMilli[1]), synthSlot);
                }

                // ===== 1:1 half-rate consumer: on the in-between display interval,
                // submit the BOTH-eye NvOF-produced pair instead of resubmitting a
                // raw/current pair with one stale eye. This is the key cadence step
                // that gives both eyes a fresh image every display interval.
                if (v2 && GetAERHalfRate() != 0) {
                    uint64_t inBetweenPairId = 0;
                    XrPosef inBetweenPoses[2]{};
                    XrFovf inBetweenFovs[2]{};
                    bool inBetweenViews[2] = {};
                    {
                        std::lock_guard<std::mutex> lock(m_presentMutex);
                        const uint64_t currentPair =
                            (eyePairIds[0] != 0 && eyePairIds[0] == eyePairIds[1]) ? eyePairIds[0] : 0;
                        if (m_inBetweenReadyPairId != 0 &&
                            currentPair == m_lastSubmittedPairId &&
                            m_inBetweenReadyPairId > m_inBetweenShownForPairId) {
                            useInBetweenPair = true;
                            inBetweenPairId = m_inBetweenReadyPairId;
                            activeInBetweenSlot = m_inBetweenSlot;
                            for (int eye = 0; eye < 2; ++eye) {
                                inBetweenPoses[eye] = m_inBetweenEyePoses[eye];
                                inBetweenFovs[eye] = m_inBetweenEyeFovs[eye];
                                inBetweenViews[eye] = m_inBetweenEyeViewsValid[eye];
                            }
                        }
                    }
                    if (useInBetweenPair) {
                        for (int eye = 0; eye < 2; ++eye) {
                            if (eyeSources[eye]) { eyeSources[eye]->Release(); eyeSources[eye] = nullptr; }
                            if (generatedSources[eye]) { generatedSources[eye]->Release(); generatedSources[eye] = nullptr; }
                            if (m_aerV2InBetween[eye][activeInBetweenSlot]) {
                                eyeSources[eye] = m_aerV2InBetween[eye][activeInBetweenSlot].Get();
                                eyeSources[eye]->AddRef();
                                generatedSources[eye] = m_aerV2InBetween[eye][activeInBetweenSlot].Get();
                                generatedSources[eye]->AddRef();
                                eyePoses[eye] = inBetweenPoses[eye];
                                eyeFovs[eye] = inBetweenFovs[eye];
                                eyeHasView[eye] = inBetweenViews[eye];
                                eyeIsSynth[eye] = true;
                                anyEyeSynth = true;
                                // Keep the pair ids coherent for submit telemetry.
                                eyePairIds[eye] = inBetweenPairId;
                            }
                        }
                        m_inBetweenShownForPairId = inBetweenPairId;
                    }
                }

                const uint64_t submitSerial = eyeSerials[0] > eyeSerials[1] ? eyeSerials[0] : eyeSerials[1];
                const bool completePair = GetAERPairGate() == 0 || (eyePairIds[0] != 0 && eyePairIds[0] == eyePairIds[1]);
                const bool submitReadyAERV2 =
                    eyeSources[0] && eyeSources[1] &&
                    eyeSerials[0] != 0 && eyeSerials[1] != 0 &&
                    eyeHasView[0] && eyeHasView[1];
                // Phase 2d: substitute the OLDER eye in the current pair with
                // the worker-produced MV-warped frame. The warp advances that
                // eye's image one game frame forward using engine motion
                // vectors, so the submitted pair shares a single timestamp.
                // We addref a private snapshot of m_mvWarpedEye here for the
                // submit copy below; the slot itself is reused by the next
                // worker pass (worker's m_mvWarpEvent wait guarantees the GPU
                // write is complete before this addref/copy starts).
                ID3D12Resource* mvWarpedReplacement[2] = {nullptr, nullptr};
                const bool aerV2On = GetAERV2Enabled() != 0;
                if (!aerV2On && completePair && eyePairIds[0] != 0) {
                    for (int eye = 0; eye < 2; ++eye) {
                        if (m_mvWarpedValidPairId[eye].load(std::memory_order_acquire) == eyePairIds[0] &&
                            m_mvWarpedEye[eye]) {
                            m_mvWarpedEye[eye]->AddRef();
                            mvWarpedReplacement[eye] = m_mvWarpedEye[eye].Get();
                        }
                    }
                }
                
                // Reuse-last-frame path: OpenXR cannot "skip submit" without showing
                // nothing, so stale ticks must re-submit the last clean eye image.

                m_cmdAllocatorIndex = (m_cmdAllocatorIndex + 1) % 3;
                ID3D12CommandAllocator* currentAllocator = m_cmdAllocators[m_cmdAllocatorIndex];
                if (m_fenceValue >= 3 && m_fence->GetCompletedValue() < m_fenceValue - 2) {
                    m_fence->SetEventOnCompletion(m_fenceValue - 2, m_fenceEvent);
                    WaitForSingleObject(m_fenceEvent, 1000); // was INFINITE: bound GPU-fence wait so a stall can't hang the frame loop (Reset then fails -> submit skipped)
                }

                ID3D12GraphicsCommandList* m_cmdList = m_cmdLists[m_cmdAllocatorIndex];

                // Reuse the last clean frame on stale ticks instead of re-warping stale
                // content again.
                const bool reuseLastFrame = ReuseLastFrameOutputEnabled() && useAerSubmit &&
                    submitSerial != 0 && submitSerial == m_lastSubmittedSerial &&
                    m_lastGoodValid && m_lastGoodEye[0] && m_lastGoodEye[1];

                if (eyeSources[0] && eyeSources[1] && eyeSerials[0] != 0 && eyeSerials[1] != 0 && (aerV2On ? submitReadyAERV2 : completePair) && eyeHasView[0] && eyeHasView[1] &&
                    SUCCEEDED(currentAllocator->Reset()) && SUCCEEDED(m_cmdList->Reset(currentAllocator, nullptr))) {
                    bool copyReady = true;
                    bool releaseOk = true;
                    bool useDepthLayer = (depthSource != nullptr) && m_depthLayerSupported && !reuseLastFrame;
                    // alternate-eye AER V2 needs depth that matches the synthetic
                    // midpoint image. Feeding the compositor raw-frame depth for a
                    // synthetic color frame creates severe mismatches on nearby
                    // geometry (experienced as black/incorrect synthetic flashes).
                    // Until we synthesize matching midpoint depth, disable the XR
                    // depth layer on synthetic submits and let the compositor use
                    // color-only async warp for those frames.
                    if (anyEyeSynth) {
                        useDepthLayer = false;
                    }

                    // Phase 3: depth-based stereo reprojection. Identify the
                    // FRESHER eye in the pair (higher serial = newer capture)
                    // and synthesize the other from it + scene depth. This
                    // collapses the inter-eye sim gap (the OLDER eye is now
                    // replaced with a same-timestamp view derived from the
                    // FRESHER one).
                    bool stereoSynthOk = false;
                    int stereoSynthForEye = -1;  // which eye gets the synthesized texture
                    // AER V2 does not use the older D3D12 stereoSynth
                    // reprojection pass. In AER V2 mode the only synthetic
                    // image source must be the CUDA/NvOF pipeline below.
                    if (!aerV2On && depthSource && m_stereoSynthEye && m_d3dDevice) {
                        if (!m_stereoReproject) m_stereoReproject = std::make_unique<StereoReproject>();
                        const D3D12_RESOURCE_DESC stereoDesc = m_stereoSynthEye->GetDesc();
                        if (m_stereoReproject->EnsureInitialized(m_d3dDevice,
                                stereoDesc.Format,
                                static_cast<uint32_t>(stereoDesc.Width),
                                stereoDesc.Height)) {
                            const int fresherEye = (eyeSerials[0] >= eyeSerials[1]) ? 0 : 1;
                            const int targetEye = fresherEye ^ 1;
                            ID3D12Resource* srcColor = eyeSources[fresherEye];
                            ID3D12Resource* srcDepth = depthSource;

                            // FOV in radians (use the fresher eye's fov_x).
                            float fovLeft = std::fabs(eyeFovs[fresherEye].angleLeft);
                            float fovRight = std::fabs(eyeFovs[fresherEye].angleRight);
                            float horizFovRad = fovLeft + fovRight;
                            if (horizFovRad < 0.1f || horizFovRad > 3.14f) horizFovRad = 1.658f; // ~95°
                            // IPD: use runtime view positions if available, else default 0.065.
                            float ipdMeters = 0.065f;
                            if (m_views.size() >= 2) {
                                float dx = m_views[1].pose.position.x - m_views[0].pose.position.x;
                                float dy = m_views[1].pose.position.y - m_views[0].pose.position.y;
                                float dz = m_views[1].pose.position.z - m_views[0].pose.position.z;
                                float distance = std::sqrt(dx*dx + dy*dy + dz*dz);
                                if (distance > 0.04f && distance < 0.10f) ipdMeters = distance;
                            }
                            // CP2077 near plane (per FinalCamera log f40=0.02 observed).
                            const float nearZ = 0.02f;
                            // Convention: when synthesizing right(=eye1) from left(=eye0),
                            // sample further LEFT in source for each output pixel →
                            // positive sign. When synthesizing left from right → negative.
                            const float signLR = (targetEye == 1) ? +1.0f : -1.0f;

                            // Barriers: src color from COPY_SOURCE → PSR; depth
                            // snapshot from COPY_SOURCE → PSR; synth scratch from
                            // COMMON → RENDER_TARGET.
                            D3D12_RESOURCE_BARRIER pre[3] = {};
                            UINT preBc = 0;
                            auto addBar = [&](D3D12_RESOURCE_BARRIER* arr, UINT& bc, ID3D12Resource* r,
                                              D3D12_RESOURCE_STATES before, D3D12_RESOURCE_STATES after) {
                                if (!r || before == after) return;
                                arr[bc].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                                arr[bc].Transition.pResource = r;
                                arr[bc].Transition.StateBefore = before;
                                arr[bc].Transition.StateAfter = after;
                                arr[bc].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                                ++bc;
                            };
                            addBar(pre, preBc, srcColor, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE);
                            addBar(pre, preBc, srcDepth, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE);
                            addBar(pre, preBc, m_stereoSynthEye.Get(), D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_RENDER_TARGET);
                            if (preBc > 0) m_cmdList->ResourceBarrier(preBc, pre);

                            stereoSynthOk = m_stereoReproject->RecordReproject(
                                m_cmdList, srcColor, srcDepth, m_stereoSynthEye.Get(),
                                ipdMeters, horizFovRad, nearZ, signLR);

                            D3D12_RESOURCE_BARRIER post[3] = {};
                            UINT postBc = 0;
                            addBar(post, postBc, srcColor, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COPY_SOURCE);
                            addBar(post, postBc, srcDepth, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COPY_SOURCE);
                            addBar(post, postBc, m_stereoSynthEye.Get(), D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_COPY_SOURCE);
                            if (postBc > 0) m_cmdList->ResourceBarrier(postBc, post);

                            if (stereoSynthOk) stereoSynthForEye = targetEye;
                            static uint64_t s_logCounter = 0;
                            if (g_verboseLog && (s_logCounter++ % 300) == 0) {
                                Log("OpenXRManager: [stereoSynth] pair=%llu fresherEye=%d targetEye=%d ipd=%.4f fovH=%.3f synth=%d\n",
                                    static_cast<unsigned long long>(eyePairIds[0]),
                                    fresherEye, targetEye, ipdMeters, horizFovRad,
                                    stereoSynthOk ? 1 : 0);
                            }
                        }
                    }
                    std::vector<bool> acquiredEyes(viewCountOutput, false);
                    std::vector<bool> acquiredDepthEyes(viewCountOutput, false);
                    std::vector<XrCompositionLayerProjectionView> projectionViews(viewCountOutput);
                    std::vector<XrCompositionLayerDepthInfoKHR> depthInfos;
                    bool submittedSynthetic = false;
                    for (uint32_t i = 0; i < viewCountOutput; ++i) {
                        projectionViews[i] = {XR_TYPE_COMPOSITION_LAYER_PROJECTION_VIEW};
                    }
                    if (useDepthLayer) {
                        depthInfos.resize(viewCountOutput);
                        for (uint32_t i = 0; i < viewCountOutput; ++i) {
                            depthInfos[i] = {XR_TYPE_COMPOSITION_LAYER_DEPTH_INFO_KHR};
                        }
                    }

                    for (uint32_t eye = 0; eye < viewCountOutput; ++eye) {
                        uint32_t sourceEye = eye;
                        const int debugEye = GetAERDebugEye();
                        if (debugEye == 1) {
                            sourceEye = 1;
                        } else if (debugEye == 2) {
                            sourceEye = 0;
                        } else if (debugEye == 3) {
                            sourceEye = eye ^ 1;
                        }

                        uint32_t imageIndex = 0;
                        XrSwapchainImageAcquireInfo acquireInfo{XR_TYPE_SWAPCHAIN_IMAGE_ACQUIRE_INFO};
                        const XrResult acquireRes = xrAcquireSwapchainImage(m_eyeSwapchains[eye].handle, &acquireInfo, &imageIndex);
                        if (XR_FAILED(acquireRes)) {
                            Log("OpenXRManager: xrAcquireSwapchainImage failed for eye %u (res=%d)\n", eye, acquireRes);
                            copyReady = false;
                            break;
                        }
                        acquiredEyes[eye] = true;

                        XrSwapchainImageWaitInfo waitSwapchainInfo{XR_TYPE_SWAPCHAIN_IMAGE_WAIT_INFO};
                        waitSwapchainInfo.timeout = 200000000; // 200 ms (was XR_INFINITE_DURATION): never hang the frame loop
                        const XrResult waitRes = xrWaitSwapchainImage(m_eyeSwapchains[eye].handle, &waitSwapchainInfo);
                        if (XR_FAILED(waitRes) || waitRes == XR_TIMEOUT_EXPIRED) {
                            Log("OpenXRManager: xrWaitSwapchainImage failed/timeout for eye %u (res=%d)\n", eye, waitRes);
                            copyReady = false;
                            break;
                        }

                        ID3D12Resource* texture = m_eyeSwapchains[eye].images[imageIndex].texture;
                        ID3D12Resource* copySource = nullptr;
                        const bool syntheticSource = !reuseLastFrame && aerV2On &&
                            sourceEye < 2 &&
                            eyeIsSynth[sourceEye] &&
                            generatedSources[sourceEye] != nullptr;
                        if (syntheticSource) {
                            submittedSynthetic = true;
                        }
                        if (sourceEye < 2) {
                            if (aerV2On) {
                                copySource = syntheticSource
                                    ? generatedSources[sourceEye]
                                    : eyeSources[sourceEye];
                            } else if (static_cast<int>(sourceEye) == stereoSynthForEye && m_stereoSynthEye) {
                                copySource = m_stereoSynthEye.Get();
                            } else {
                                copySource = eyeSources[sourceEye];
                            }
                        }
                        if (reuseLastFrame && sourceEye < 2) {
                            copySource = m_lastGoodEye[sourceEye].Get();
                        }
                        if (!texture || sourceEye >= 2 || !copySource) {
                            Log("OpenXRManager: AER source/target missing for eye %u sourceEye %u image %u\n", eye, sourceEye, imageIndex);
                            copyReady = false;
                            break;
                        }

                        if (syntheticSource) {
                            // Synth eye: copy via the per-eye submit scratch so the
                            // CUDA-written m_aerV2SynthEye is never read concurrently.
                            const uint32_t scratchSlot = useInBetweenPair ? activeInBetweenSlot : synthSlot;
                            ID3D12Resource* submitScratch = useInBetweenPair
                                ? m_aerV2InBetweenSubmit[sourceEye][scratchSlot].Get()
                                : m_aerV2SubmitEye[sourceEye][scratchSlot].Get();
                            bool* scratchReady = useInBetweenPair
                                ? &m_aerV2InBetweenSubmitReady[sourceEye][scratchSlot]
                                : &m_aerV2SubmitEyeReady[sourceEye][scratchSlot];
                            if (!submitScratch) {
                                Log("OpenXRManager: AER submit scratch missing for eye %u slot %u\n",
                                    sourceEye, scratchSlot);
                                copyReady = false;
                                break;
                            }
                            D3D12_RESOURCE_BARRIER preScratch{};
                            preScratch.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            preScratch.Transition.pResource = submitScratch;
                            preScratch.Transition.StateBefore = *scratchReady
                                ? D3D12_RESOURCE_STATE_COPY_SOURCE
                                : D3D12_RESOURCE_STATE_COMMON;
                            preScratch.Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_DEST;
                            preScratch.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            m_cmdList->ResourceBarrier(1, &preScratch);

                            m_cmdList->CopyResource(submitScratch, copySource);

                            D3D12_RESOURCE_BARRIER postScratch{};
                            postScratch.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            postScratch.Transition.pResource = submitScratch;
                            postScratch.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_DEST;
                            postScratch.Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
                            postScratch.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            m_cmdList->ResourceBarrier(1, &postScratch);
                            *scratchReady = true;
                            copySource = submitScratch;
                        }

                        // CAS sharpen: when enabled, draw the sharpened source straight
                        // into the swapchain image instead of
                        // a plain copy. Source state differs by path (synth scratch is
                        // COPY_SOURCE, raw capture is COMMON), so transition from that.
                        const float sharpStrength = GetVrSharpness();
                        bool doSharpen = false;
                        // DISABLED: in-submit CAS GPU-crashes (device removal) on
                        // this runtime (VirtualDesktop OpenXR) -- rendering an RTV into
                        // the runtime swapchain image and/or sampling the capture as SRV
                        // faults. Needs a dedicated SRV-capable scratch (copy source ->
                        // own RT, sharpen that -> swapchain) before re-enabling; the
                        // Sharpen slider is inert meanwhile.
                        if (false && sharpStrength > 0.0001f && m_d3dDevice && texture) {
                            if (!m_sharpenPass) m_sharpenPass = std::make_unique<SharpenPass>();
                            const D3D12_RESOURCE_DESC sd = texture->GetDesc();
                            m_sharpenReady = m_sharpenPass->EnsureInitialized(
                                m_d3dDevice, sd.Format,
                                static_cast<uint32_t>(sd.Width), sd.Height);
                            doSharpen = m_sharpenReady;
                        }
                        if (doSharpen) {
                            // Both paths leave copySource in COPY_SOURCE: the raw eye
                            // capture rests in COPY_SOURCE (CapturePresentedFrame), and
                            // the synth scratch was just transitioned to COPY_SOURCE.
                            // (Assuming COMMON for raw was the CAS crash / device-removal.)
                            const D3D12_RESOURCE_STATES srcPrev = D3D12_RESOURCE_STATE_COPY_SOURCE;
                            D3D12_RESOURCE_BARRIER pre[2] = {};
                            pre[0].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            pre[0].Transition.pResource = copySource;
                            pre[0].Transition.StateBefore = srcPrev;
                            pre[0].Transition.StateAfter = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
                            pre[0].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            pre[1].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            pre[1].Transition.pResource = texture;
                            pre[1].Transition.StateBefore = D3D12_RESOURCE_STATE_COMMON;
                            pre[1].Transition.StateAfter = D3D12_RESOURCE_STATE_RENDER_TARGET;
                            pre[1].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            m_cmdList->ResourceBarrier(2, pre);

                            m_sharpenPass->RecordSharpen(m_cmdList, copySource, texture,
                                                         sharpStrength, GetVrSharpmix());

                            D3D12_RESOURCE_BARRIER post[2] = {};
                            post[0].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            post[0].Transition.pResource = texture;
                            post[0].Transition.StateBefore = D3D12_RESOURCE_STATE_RENDER_TARGET;
                            post[0].Transition.StateAfter = D3D12_RESOURCE_STATE_COMMON;
                            post[0].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            post[1].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            post[1].Transition.pResource = copySource;
                            post[1].Transition.StateBefore = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
                            post[1].Transition.StateAfter = srcPrev;
                            post[1].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            m_cmdList->ResourceBarrier(2, post);
                        } else {
                            D3D12_RESOURCE_BARRIER toCopyDest{};
                            toCopyDest.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            toCopyDest.Transition.pResource = texture;
                            toCopyDest.Transition.StateBefore = D3D12_RESOURCE_STATE_COMMON;
                            toCopyDest.Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_DEST;
                            toCopyDest.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            m_cmdList->ResourceBarrier(1, &toCopyDest);

                            m_cmdList->CopyResource(texture, copySource);

                            D3D12_RESOURCE_BARRIER toCommon{};
                            toCommon.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            toCommon.Transition.pResource = texture;
                            toCommon.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_DEST;
                            toCommon.Transition.StateAfter = D3D12_RESOURCE_STATE_COMMON;
                            toCommon.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            m_cmdList->ResourceBarrier(1, &toCommon);
                        }

                        // AER V2 submits the synthetic eye with its own predicted
                        // pose/FOV, not with the raw source eye's pose. The older debug/raw
                        // source-eye remap logic is only valid for non-synthetic paths.
                        if (syntheticSource) {
                            projectionViews[eye].pose = eyePoses[sourceEye];
                            projectionViews[eye].fov = eyeFovs[sourceEye];
                        } else {
                            const uint32_t poseEye = (debugEye == 1 || debugEye == 2 || debugEye == 3) ? sourceEye : eye;
                            if (poseEye < 2) {
                                projectionViews[eye].pose = eyePoses[poseEye];
                                projectionViews[eye].fov = eyeFovs[poseEye];
                            } else {
                                projectionViews[eye].pose = eyePoses[eye];
                                projectionViews[eye].fov = eyeFovs[eye];
                            }
                        }
                        projectionViews[eye].subImage.swapchain = m_eyeSwapchains[eye].handle;
                        projectionViews[eye].subImage.imageRect.offset = {0, 0};
                        projectionViews[eye].subImage.imageRect.extent = {m_eyeSwapchains[eye].width, m_eyeSwapchains[eye].height};
                        projectionViews[eye].subImage.imageArrayIndex = 0;

                        // Re-present: use the stashed pose so the runtime reprojects the
                        // last-good image to the current head. Normal: capture this eye
                        // into the persistent last-good texture
                        // (CopyResource from copySource, which rests in COPY_SOURCE) and
                        // stash its pose/fov for future re-presents.
                        if (reuseLastFrame && sourceEye < 2) {
                            projectionViews[eye].pose = m_lastGoodPose[sourceEye];
                            projectionViews[eye].fov = m_lastGoodFov[sourceEye];
                        } else if (ReuseLastFrameOutputEnabled() && sourceEye < 2 && copySource && m_d3dDevice) {
                            if (!m_lastGoodEye[sourceEye]) {
                                const D3D12_RESOURCE_DESC td = texture->GetDesc();
                                D3D12_HEAP_PROPERTIES hp{}; hp.Type = D3D12_HEAP_TYPE_DEFAULT;
                                D3D12_RESOURCE_DESC rd{};
                                rd.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
                                rd.Width = td.Width; rd.Height = td.Height;
                                rd.DepthOrArraySize = 1; rd.MipLevels = 1;
                                rd.Format = td.Format; rd.SampleDesc.Count = 1;
                                rd.Layout = D3D12_TEXTURE_LAYOUT_UNKNOWN;
                                m_d3dDevice->CreateCommittedResource(&hp, D3D12_HEAP_FLAG_NONE, &rd,
                                    D3D12_RESOURCE_STATE_COMMON, nullptr, IID_PPV_ARGS(&m_lastGoodEye[sourceEye]));
                                m_lastGoodEyeInited[sourceEye] = false;
                            }
                            if (m_lastGoodEye[sourceEye]) {
                                D3D12_RESOURCE_BARRIER b{};
                                b.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                                b.Transition.pResource = m_lastGoodEye[sourceEye].Get();
                                b.Transition.StateBefore = m_lastGoodEyeInited[sourceEye]
                                    ? D3D12_RESOURCE_STATE_COPY_SOURCE : D3D12_RESOURCE_STATE_COMMON;
                                b.Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_DEST;
                                b.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                                m_cmdList->ResourceBarrier(1, &b);
                                m_cmdList->CopyResource(m_lastGoodEye[sourceEye].Get(), copySource);
                                b.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_DEST;
                                b.Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
                                m_cmdList->ResourceBarrier(1, &b);
                                m_lastGoodEyeInited[sourceEye] = true;
                                m_lastGoodPose[sourceEye] = projectionViews[eye].pose;
                                m_lastGoodFov[sourceEye] = projectionViews[eye].fov;
                            }
                        }
                    }

                    if (!reuseLastFrame && copyReady &&
                        m_lastGoodEyeInited[0] && m_lastGoodEyeInited[1]) {
                        m_lastGoodValid = true;
                    }

                    // [DEPTH] Acquire each eye's depth swapchain, copy the scene-depth
                    // snapshot into it, and chain XR_TYPE_COMPOSITION_LAYER_DEPTH_INFO_KHR
                    // onto the projection view. Same shared snapshot for both eyes (the
                    // game renders one depth buffer per present). Reversed-Z is encoded via
                    // nearZ > farZ, NOT by swapping min/max depth. Mirrors the mono path.
                    if (copyReady && useDepthLayer) {
                        for (uint32_t eye = 0; eye < viewCountOutput; ++eye) {
                            if (m_eyeSwapchains[eye].depthHandle == XR_NULL_HANDLE) {
                                Log("OpenXRManager: [DEPTH] AER depthHandle missing for eye %u\n", eye);
                                useDepthLayer = false;
                                break;
                            }
                            uint32_t depthImageIndex = 0;
                            XrSwapchainImageAcquireInfo acquireInfo{XR_TYPE_SWAPCHAIN_IMAGE_ACQUIRE_INFO};
                            const XrResult acquireRes = xrAcquireSwapchainImage(m_eyeSwapchains[eye].depthHandle, &acquireInfo, &depthImageIndex);
                            if (XR_FAILED(acquireRes)) {
                                Log("OpenXRManager: [DEPTH] AER xrAcquireSwapchainImage failed for eye %u (res=%d)\n", eye, acquireRes);
                                useDepthLayer = false;
                                break;
                            }
                            acquiredDepthEyes[eye] = true;
                            XrSwapchainImageWaitInfo waitInfo{XR_TYPE_SWAPCHAIN_IMAGE_WAIT_INFO};
                            waitInfo.timeout = 200000000; // 200 ms (was XR_INFINITE_DURATION): never hang the frame loop
                            const XrResult waitRes = xrWaitSwapchainImage(m_eyeSwapchains[eye].depthHandle, &waitInfo);
                            if (XR_FAILED(waitRes) || waitRes == XR_TIMEOUT_EXPIRED) {
                                Log("OpenXRManager: [DEPTH] AER xrWaitSwapchainImage failed/timeout for eye %u (res=%d)\n", eye, waitRes);
                                useDepthLayer = false;
                                break;
                            }
                            ID3D12Resource* depthTexture = m_eyeSwapchains[eye].depthImages[depthImageIndex].texture;
                            if (!depthTexture) {
                                Log("OpenXRManager: [DEPTH] AER depth swapchain texture missing for eye %u image %u\n", eye, depthImageIndex);
                                useDepthLayer = false;
                                break;
                            }
                            D3D12_RESOURCE_BARRIER toCopyDest{};
                            toCopyDest.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            toCopyDest.Transition.pResource = depthTexture;
                            toCopyDest.Transition.StateBefore = D3D12_RESOURCE_STATE_COMMON;
                            toCopyDest.Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_DEST;
                            toCopyDest.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            m_cmdList->ResourceBarrier(1, &toCopyDest);

                            const D3D12_RESOURCE_DESC depthSrcDesc = depthSource->GetDesc();
                            const D3D12_RESOURCE_DESC depthDstDesc = depthTexture->GetDesc();
                            if (depthSrcDesc.Format == depthDstDesc.Format) {
                                m_cmdList->CopyResource(depthTexture, depthSource);
                            } else {
                                D3D12_TEXTURE_COPY_LOCATION dstLoc{};
                                dstLoc.pResource = depthTexture;
                                dstLoc.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
                                dstLoc.SubresourceIndex = 0;
                                D3D12_TEXTURE_COPY_LOCATION srcLoc{};
                                srcLoc.pResource = depthSource;
                                srcLoc.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
                                srcLoc.SubresourceIndex = 0;
                                m_cmdList->CopyTextureRegion(&dstLoc, 0, 0, 0, &srcLoc, nullptr);
                            }

                            D3D12_RESOURCE_BARRIER toCommon{};
                            toCommon.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            toCommon.Transition.pResource = depthTexture;
                            toCommon.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_DEST;
                            toCommon.Transition.StateAfter = D3D12_RESOURCE_STATE_COMMON;
                            toCommon.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            m_cmdList->ResourceBarrier(1, &toCommon);

                            depthInfos[eye].subImage.swapchain = m_eyeSwapchains[eye].depthHandle;
                            depthInfos[eye].subImage.imageRect.offset = {0, 0};
                            depthInfos[eye].subImage.imageRect.extent = {m_eyeSwapchains[eye].width, m_eyeSwapchains[eye].height};
                            depthInfos[eye].subImage.imageArrayIndex = 0;
                            depthInfos[eye].minDepth = 0.0f;
                            depthInfos[eye].maxDepth = 1.0f;
                            depthInfos[eye].nearZ = 10000.0f;
                            depthInfos[eye].farZ = 0.01f;
                            projectionViews[eye].next = &depthInfos[eye];
                        }
                    } else {
                        useDepthLayer = false;
                    }

                    // Phase 3: return stereo-synth scratch to COMMON for the
                    // next pair's reproject pre-barrier.
                    if (stereoSynthOk && m_stereoSynthEye) {
                        D3D12_RESOURCE_BARRIER toCommon{};
                        toCommon.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                        toCommon.Transition.pResource = m_stereoSynthEye.Get();
                        toCommon.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
                        toCommon.Transition.StateAfter = D3D12_RESOURCE_STATE_COMMON;
                        toCommon.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                        m_cmdList->ResourceBarrier(1, &toCommon);
                    }

                    m_cmdList->Close();
                    ID3D12CommandList* cmdLists[] = {m_cmdList};
                    m_d3dQueue->ExecuteCommandLists(1, cmdLists);
                    
                    ++m_fenceValue;
                    m_d3dQueue->Signal(m_fence, m_fenceValue);

                    for (uint32_t eye = 0; eye < viewCountOutput; ++eye) {
                        if (!acquiredEyes[eye]) {
                            continue;
                        }
                        XrSwapchainImageReleaseInfo releaseInfo{XR_TYPE_SWAPCHAIN_IMAGE_RELEASE_INFO};
                        const XrResult releaseRes = xrReleaseSwapchainImage(m_eyeSwapchains[eye].handle, &releaseInfo);
                        if (XR_FAILED(releaseRes)) {
                            Log("OpenXRManager: xrReleaseSwapchainImage failed for eye %u (res=%d)\n", eye, releaseRes);
                            releaseOk = false;
                        }
                    }

                    for (uint32_t eye = 0; eye < viewCountOutput; ++eye) {
                        if (!acquiredDepthEyes[eye]) {
                            continue;
                        }
                        XrSwapchainImageReleaseInfo releaseInfo{XR_TYPE_SWAPCHAIN_IMAGE_RELEASE_INFO};
                        const XrResult releaseRes = xrReleaseSwapchainImage(m_eyeSwapchains[eye].depthHandle, &releaseInfo);
                        if (XR_FAILED(releaseRes)) {
                            Log("OpenXRManager: [DEPTH] AER xrReleaseSwapchainImage failed for eye %u (res=%d)\n", eye, releaseRes);
                            useDepthLayer = false;
                        }
                    }

                    if (!useDepthLayer) {
                        for (uint32_t eye = 0; eye < viewCountOutput; ++eye) {
                            projectionViews[eye].next = nullptr;
                        }
                    }

                    if (copyReady && releaseOk) {
                        XrCompositionLayerProjection layerProj{XR_TYPE_COMPOSITION_LAYER_PROJECTION};
                        XrCompositionLayerQuad layerQuad{XR_TYPE_COMPOSITION_LAYER_QUAD};
                        const XrCompositionLayerBaseHeader* layers[1] = {nullptr};

                        if (menuRectActive) {
                            layerQuad.space = m_localSpace;
                            layerQuad.eyeVisibility = XR_EYE_VISIBILITY_BOTH;
                            layerQuad.subImage = projectionViews[0].subImage;

                            // LAZY-FOLLOW panel: stays put within the dead-zone, eases
                            // to the head past the threshold (see ComputeMenuQuadPose).
                            layerQuad.pose = ComputeMenuQuadPose(headPoseLocated, location.pose);

                            float quadWidth = 2.0f * 1.5f * tanf(GetMenuFov() * 3.14159f / 180.0f * 0.5f);
                            layerQuad.size = {quadWidth, quadWidth};
                            layers[0] = reinterpret_cast<const XrCompositionLayerBaseHeader*>(&layerQuad);
                        } else {
                            layerProj.space = m_localSpace;
                            layerProj.viewCount = viewCountOutput;
                            layerProj.views = projectionViews.data();
                            layers[0] = reinterpret_cast<const XrCompositionLayerBaseHeader*>(&layerProj);
                        }

                        XrFrameEndInfo endInfo{XR_TYPE_FRAME_END_INFO};
                        endInfo.displayTime = frameState.predictedDisplayTime;
                        endInfo.environmentBlendMode = XR_ENVIRONMENT_BLEND_MODE_OPAQUE;
                        endInfo.layerCount = 1;
                        endInfo.layers = layers;
                        const XrResult endRes = xrEndFrame(m_session, &endInfo);
                        if (XR_SUCCEEDED(endRes)) {
                            // AER submit telemetry. Track ratio of raw vs synth
                            // submits per 300-frame window — that's the lever
                            // we're trying to bend (synth share = perceived per-
                            // eye rate boost above baseline 34 Hz).
                            static uint64_t s_aerSubmitsRaw = 0;
                            static uint64_t s_aerSubmitsSynth = 0;
                            static uint64_t s_aerFreshSubmits = 0;
                            static uint64_t s_aerResubmits = 0;
                            const bool wasFresh = submitSerial != m_lastSubmittedSerial;
                            if (submittedSynthetic) ++s_aerSubmitsSynth; else ++s_aerSubmitsRaw;
                            if (wasFresh) ++s_aerFreshSubmits; else ++s_aerResubmits;
                            const bool stride = (eyePairIds[0] == 1 || (eyePairIds[0] % 300) == 0);
                            if (stride || g_verboseLog != 0) {
                                Log("OpenXRManager: AER frame submitted. left=%llu right=%llu pair=(%llu,%llu) fresh=%d shouldRender=%d debugEye=%d synth=%d depth=%d (window: raw=%llu synth=%llu fresh=%llu resub=%llu)\n",
                                    static_cast<unsigned long long>(eyeSerials[0]),
                                    static_cast<unsigned long long>(eyeSerials[1]),
                                    static_cast<unsigned long long>(eyePairIds[0]),
                                    static_cast<unsigned long long>(eyePairIds[1]),
                                    wasFresh ? 1 : 0,
                                    frameState.shouldRender ? 1 : 0,
                                    GetAERDebugEye(),
                                    submittedSynthetic ? 1 : 0,
                                    useDepthLayer ? 1 : 0,
                                    static_cast<unsigned long long>(s_aerSubmitsRaw),
                                    static_cast<unsigned long long>(s_aerSubmitsSynth),
                                    static_cast<unsigned long long>(s_aerFreshSubmits),
                                    static_cast<unsigned long long>(s_aerResubmits));
                            }
                            if (stride) {
                                // Reset window counters on the 300-pair stride
                                // so we always see the LAST 300-pair behavior,
                                // not a cumulative average that hides recent
                                // regressions.
                                s_aerSubmitsRaw = 0;
                                s_aerSubmitsSynth = 0;
                                s_aerFreshSubmits = 0;
                                s_aerResubmits = 0;
                            }
                            m_lastSubmittedSerial = submitSerial;
                            m_lastSubmittedPairId = eyePairIds[0];
                            eyeSources[0]->Release();
                            eyeSources[1]->Release();
                            if (generatedSources[0]) generatedSources[0]->Release();
                            if (generatedSources[1]) generatedSources[1]->Release();
                            if (mvWarpedReplacement[0]) mvWarpedReplacement[0]->Release();
                            if (mvWarpedReplacement[1]) mvWarpedReplacement[1]->Release();
                            if (depthSource) depthSource->Release();
                            if (m_frameSyncEvent) {
                                SetEvent(m_frameSyncEvent);
                            }
                            continue;
                        }

                        Log("OpenXRManager: xrEndFrame AER submit failed (res=%d)\n", endRes);
                    }
                } else if (((++monoWaitLogCounter % 300) == 1)) {
                    Log("OpenXRManager: AER submit waiting. left=%llu right=%llu pair=(%llu,%llu) complete=%d leftView=%d rightView=%d views=%u shouldRender=%d\n",
                        static_cast<unsigned long long>(eyeSerials[0]),
                        static_cast<unsigned long long>(eyeSerials[1]),
                        static_cast<unsigned long long>(eyePairIds[0]),
                        static_cast<unsigned long long>(eyePairIds[1]),
                        completePair ? 1 : 0,
                        eyeHasView[0] ? 1 : 0,
                        eyeHasView[1] ? 1 : 0,
                        viewCountOutput,
                        frameState.shouldRender ? 1 : 0);
                }

                if (eyeSources[0]) {
                    eyeSources[0]->Release();
                }
                if (eyeSources[1]) {
                    eyeSources[1]->Release();
                }
                if (generatedSources[0]) {
                    generatedSources[0]->Release();
                }
                if (generatedSources[1]) {
                    generatedSources[1]->Release();
                }
                if (mvWarpedReplacement[0]) {
                    mvWarpedReplacement[0]->Release();
                }
                if (mvWarpedReplacement[1]) {
                    mvWarpedReplacement[1]->Release();
                }
                if (depthSource) {
                    depthSource->Release();
                }
            } else {
                ID3D12Resource* monoSource = nullptr;
                ID3D12Resource* monoDepthSource = nullptr;
                uint64_t presentSerial = 0;
                uint64_t monoDepthFence = 0;   // writer-queue fence guarding monoDepthSource
                XrPosef monoPoses[2]{};
                XrFovf monoFovs[2]{};
                bool monoHasView[2] = {};
                bool monoHasDepth = false;
                {
                    std::lock_guard<std::mutex> lock(m_presentMutex);
                    if (m_monoCapturedFrame.texture &&
                        m_monoCapturedFrame.serial != 0 &&
                        m_monoCapturedFrame.hasView[0] &&
                        m_monoCapturedFrame.hasView[1]) {
                        monoSource = m_monoCapturedFrame.texture;
                        monoSource->AddRef();
                        presentSerial = m_monoCapturedFrame.serial;
                        for (int eye = 0; eye < 2; ++eye) {
                            monoPoses[eye] = m_monoCapturedFrame.poses[eye];
                            monoFovs[eye] = m_monoCapturedFrame.fovs[eye];
                            monoHasView[eye] = m_monoCapturedFrame.hasView[eye];
                        }
                        if (m_depthLayerSupported &&
                            m_depthSnapshot &&
                            m_depthSnapshotSerial == m_monoCapturedFrame.serial) {
                            monoDepthSource = m_depthSnapshot;
                            monoDepthSource->AddRef();
                            monoDepthFence = m_depthSnapshotWriterFence;
                            monoHasDepth = true;
                        }
                    }
                }

                std::unique_lock<std::mutex> monoSubmitLock(m_captureMutex, std::defer_lock);
                if (monoSource) {
                    monoSubmitLock.lock();
                }

                m_cmdAllocatorIndex = (m_cmdAllocatorIndex + 1) % 3;
                ID3D12CommandAllocator* currentAllocator = m_cmdAllocators[m_cmdAllocatorIndex];
                if (m_fenceValue >= 3 && m_fence->GetCompletedValue() < m_fenceValue - 2) {
                    m_fence->SetEventOnCompletion(m_fenceValue - 2, m_fenceEvent);
                    WaitForSingleObject(m_fenceEvent, 1000); // was INFINITE: bound GPU-fence wait so a stall can't hang the frame loop (Reset then fails -> submit skipped)
                }

                ID3D12GraphicsCommandList* m_cmdList = m_cmdLists[m_cmdAllocatorIndex];

                if (monoSource && monoHasView[0] && monoHasView[1] &&
                    SUCCEEDED(currentAllocator->Reset()) && SUCCEEDED(m_cmdList->Reset(currentAllocator, nullptr))) {
                    bool copyReady = true;
                    bool useDepthLayer = monoHasDepth && m_depthLayerSupported;
                    std::vector<bool> acquiredEyes(viewCountOutput, false);
                    std::vector<bool> acquiredDepthEyes(viewCountOutput, false);
                    std::vector<XrCompositionLayerProjectionView> projectionViews(viewCountOutput);
                    std::vector<XrCompositionLayerDepthInfoKHR> depthInfos;
                    for (uint32_t i = 0; i < viewCountOutput; ++i) {
                        projectionViews[i] = {XR_TYPE_COMPOSITION_LAYER_PROJECTION_VIEW};
                    }
                    if (useDepthLayer) {
                        depthInfos.resize(viewCountOutput);
                        for (uint32_t i = 0; i < viewCountOutput; ++i) {
                            depthInfos[i] = {XR_TYPE_COMPOSITION_LAYER_DEPTH_INFO_KHR};
                        }
                    }

                    for (uint32_t eye = 0; eye < viewCountOutput; ++eye) {
                        uint32_t imageIndex = 0;
                        XrSwapchainImageAcquireInfo acquireInfo{XR_TYPE_SWAPCHAIN_IMAGE_ACQUIRE_INFO};
                        const XrResult acquireRes = xrAcquireSwapchainImage(m_eyeSwapchains[eye].handle, &acquireInfo, &imageIndex);
                        if (XR_FAILED(acquireRes)) {
                            Log("OpenXRManager: xrAcquireSwapchainImage failed for eye %u (res=%d)\n", eye, acquireRes);
                            copyReady = false;
                            break;
                        }
                        acquiredEyes[eye] = true;

                        XrSwapchainImageWaitInfo waitSwapchainInfo{XR_TYPE_SWAPCHAIN_IMAGE_WAIT_INFO};
                        waitSwapchainInfo.timeout = 200000000; // 200 ms (was XR_INFINITE_DURATION): never hang the frame loop
                        const XrResult waitRes = xrWaitSwapchainImage(m_eyeSwapchains[eye].handle, &waitSwapchainInfo);
                        if (XR_FAILED(waitRes) || waitRes == XR_TIMEOUT_EXPIRED) {
                            Log("OpenXRManager: xrWaitSwapchainImage failed/timeout for eye %u (res=%d)\n", eye, waitRes);
                            copyReady = false;
                            break;
                        }

                        ID3D12Resource* texture = m_eyeSwapchains[eye].images[imageIndex].texture;
                        if (!texture) {
                            Log("OpenXRManager: XR swapchain texture missing for eye %u image %u\n", eye, imageIndex);
                            copyReady = false;
                            break;
                        }

                        // CAS sharpen (same as the AER path): when xr_sharpness>0, draw
                        // the sharpened mono source straight into the swapchain image.
                        // monoSource is always COMMON here (no synth scratch in mono).
                        const float monoSharp = GetVrSharpness();
                        bool doMonoSharpen = false;
                        // DISABLED (see AER path): in-submit CAS GPU-crashes; needs an
                        // SRV scratch rework before re-enabling.
                        if (false && monoSharp > 0.0001f && m_d3dDevice && texture && monoSource) {
                            if (!m_sharpenPass) m_sharpenPass = std::make_unique<SharpenPass>();
                            const D3D12_RESOURCE_DESC sd = texture->GetDesc();
                            m_sharpenReady = m_sharpenPass->EnsureInitialized(
                                m_d3dDevice, sd.Format,
                                static_cast<uint32_t>(sd.Width), sd.Height);
                            doMonoSharpen = m_sharpenReady;
                        }
                        if (doMonoSharpen) {
                            // monoSource rests in COPY_SOURCE (CaptureMonoPresentedFrame).
                            D3D12_RESOURCE_BARRIER pre[2] = {};
                            pre[0].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            pre[0].Transition.pResource = monoSource;
                            pre[0].Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
                            pre[0].Transition.StateAfter = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
                            pre[0].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            pre[1].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            pre[1].Transition.pResource = texture;
                            pre[1].Transition.StateBefore = D3D12_RESOURCE_STATE_COMMON;
                            pre[1].Transition.StateAfter = D3D12_RESOURCE_STATE_RENDER_TARGET;
                            pre[1].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            m_cmdList->ResourceBarrier(2, pre);

                            m_sharpenPass->RecordSharpen(m_cmdList, monoSource, texture,
                                                         monoSharp, GetVrSharpmix());

                            D3D12_RESOURCE_BARRIER post[2] = {};
                            post[0].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            post[0].Transition.pResource = texture;
                            post[0].Transition.StateBefore = D3D12_RESOURCE_STATE_RENDER_TARGET;
                            post[0].Transition.StateAfter = D3D12_RESOURCE_STATE_COMMON;
                            post[0].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            post[1].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            post[1].Transition.pResource = monoSource;
                            post[1].Transition.StateBefore = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
                            post[1].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
                            post[1].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            m_cmdList->ResourceBarrier(2, post);
                        } else {
                            D3D12_RESOURCE_BARRIER toCopyDest{};
                            toCopyDest.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            toCopyDest.Transition.pResource = texture;
                            toCopyDest.Transition.StateBefore = D3D12_RESOURCE_STATE_COMMON;
                            toCopyDest.Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_DEST;
                            toCopyDest.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            m_cmdList->ResourceBarrier(1, &toCopyDest);

                            m_cmdList->CopyResource(texture, monoSource);

                            D3D12_RESOURCE_BARRIER toCommon{};
                            toCommon.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            toCommon.Transition.pResource = texture;
                            toCommon.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_DEST;
                            toCommon.Transition.StateAfter = D3D12_RESOURCE_STATE_COMMON;
                            toCommon.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            m_cmdList->ResourceBarrier(1, &toCommon);
                        }

                        projectionViews[eye].pose = monoPoses[eye];
                        projectionViews[eye].fov = monoFovs[eye];
                        if (menuRectActive) {
                            // LAZY-FOLLOW menus (projection path). Head-locking to the
                            // live eye pose every frame made the panel drag 1:1 with the
                            // head (motion sickness). Instead LATCH the per-eye poses when
                            // the menu opens and RE-LATCH (snap re-center) only once the
                            // head yaw drifts past the follow threshold, so the panel holds
                            // still within the dead-zone. (Snap here vs the smooth quad
                            // path -- these poses carry per-eye IPD, so we re-anchor them
                            // whole rather than ease a single yaw.)
                            {
                                std::lock_guard<std::mutex> vl(m_viewMutex);
                                if (m_views.size() >= 2) {
                                    const XrQuaternionf o = m_views[0].pose.orientation;
                                    const float fx = -2.0f * (o.x * o.z + o.y * o.w);
                                    const float fz = 2.0f * (o.x * o.x + o.y * o.y) - 1.0f;
                                    const float headYaw = atan2f(-fx, -fz);
                                    float startRad = GetMenuFollowDeg() * 0.01745329252f;
                                    if (startRad < 0.0872f) startRad = 0.0872f;
                                    if (startRad > 1.5708f) startRad = 1.5708f;
                                    float off = headYaw - m_menuEyeAnchorYaw;
                                    while (off >  3.14159265f) off -= 6.28318531f;
                                    while (off < -3.14159265f) off += 6.28318531f;
                                    if (!m_menuEyeAnchorValid || fabsf(off) > startRad) {
                                        m_menuEyePoses[0] = m_views[0].pose;
                                        m_menuEyePoses[1] = m_views[1].pose;
                                        m_menuEyeAnchorYaw = headYaw;
                                        m_menuEyeAnchorValid = true;
                                    }
                                }
                            }
                            if (m_menuEyeAnchorValid && eye < 2) {
                                projectionViews[eye].pose = m_menuEyePoses[eye];
                            }
                            const float menuFovDeg = GetMenuFov();
                            if (menuFovDeg > 1.0f && menuFovDeg < 170.0f) {
                                const float halfFov = (menuFovDeg * 3.1415926535f / 180.0f) * 0.5f;
                                projectionViews[eye].fov.angleLeft = -halfFov;
                                projectionViews[eye].fov.angleRight = halfFov;
                                projectionViews[eye].fov.angleDown = -halfFov;
                                projectionViews[eye].fov.angleUp = halfFov;
                            }
                        }
                        projectionViews[eye].subImage.swapchain = m_eyeSwapchains[eye].handle;
                        projectionViews[eye].subImage.imageRect.offset = {0, 0};
                        projectionViews[eye].subImage.imageRect.extent = {m_eyeSwapchains[eye].width, m_eyeSwapchains[eye].height};
                        projectionViews[eye].subImage.imageArrayIndex = 0;
                    }

                    if (copyReady && useDepthLayer) {
                        for (uint32_t eye = 0; eye < viewCountOutput; ++eye) {
                            if (m_eyeSwapchains[eye].depthHandle == XR_NULL_HANDLE) {
                                Log("OpenXRManager: [DEPTH] depthHandle missing for eye %u\n", eye);
                                useDepthLayer = false;
                                break;
                            }

                            uint32_t depthImageIndex = 0;
                            XrSwapchainImageAcquireInfo acquireInfo{XR_TYPE_SWAPCHAIN_IMAGE_ACQUIRE_INFO};
                            const XrResult acquireRes = xrAcquireSwapchainImage(m_eyeSwapchains[eye].depthHandle, &acquireInfo, &depthImageIndex);
                            if (XR_FAILED(acquireRes)) {
                                Log("OpenXRManager: [DEPTH] xrAcquireSwapchainImage failed for eye %u (res=%d)\n", eye, acquireRes);
                                useDepthLayer = false;
                                break;
                            }
                            acquiredDepthEyes[eye] = true;

                            XrSwapchainImageWaitInfo waitInfo{XR_TYPE_SWAPCHAIN_IMAGE_WAIT_INFO};
                            // NOT infinite: this mono depth-swapchain path was never
                            // exercised at HEAD and froze the present thread here (log
                            // ended right after the depth swapchain was created). A finite
                            // timeout degrades a stuck depth image to "no depth this frame"
                            // instead of a hard freeze. XR_TIMEOUT is a success code, so any
                            // non-XR_SUCCESS result means "not ready" -> skip depth (the
                            // acquired image is released by the cleanup loop below).
                            waitInfo.timeout = 100000000; // 100 ms
                            const XrResult waitRes = xrWaitSwapchainImage(m_eyeSwapchains[eye].depthHandle, &waitInfo);
                            if (waitRes != XR_SUCCESS) {
                                Log("OpenXRManager: [DEPTH] mono depth image not ready eye=%u res=%d -> skip depth this frame\n", eye, waitRes);
                                useDepthLayer = false;
                                break;
                            }

                            ID3D12Resource* depthTexture = m_eyeSwapchains[eye].depthImages[depthImageIndex].texture;
                            if (!depthTexture) {
                                Log("OpenXRManager: [DEPTH] depth swapchain texture missing for eye %u image %u\n", eye, depthImageIndex);
                                useDepthLayer = false;
                                break;
                            }

                            D3D12_RESOURCE_BARRIER toCopyDest{};
                            toCopyDest.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            toCopyDest.Transition.pResource = depthTexture;
                            toCopyDest.Transition.StateBefore = D3D12_RESOURCE_STATE_COMMON;
                            toCopyDest.Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_DEST;
                            toCopyDest.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            m_cmdList->ResourceBarrier(1, &toCopyDest);

                            const D3D12_RESOURCE_DESC depthSrcDesc = monoDepthSource->GetDesc();
                            const D3D12_RESOURCE_DESC depthDstDesc = depthTexture->GetDesc();
                            if (depthSrcDesc.Format == depthDstDesc.Format) {
                                m_cmdList->CopyResource(depthTexture, monoDepthSource);
                            } else {
                                D3D12_TEXTURE_COPY_LOCATION dstLoc{};
                                dstLoc.pResource = depthTexture;
                                dstLoc.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
                                dstLoc.SubresourceIndex = 0;
                                D3D12_TEXTURE_COPY_LOCATION srcLoc{};
                                srcLoc.pResource = monoDepthSource;
                                srcLoc.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
                                srcLoc.SubresourceIndex = 0;
                                m_cmdList->CopyTextureRegion(&dstLoc, 0, 0, 0, &srcLoc, nullptr);
                            }

                            D3D12_RESOURCE_BARRIER toCommon{};
                            toCommon.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            toCommon.Transition.pResource = depthTexture;
                            toCommon.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_DEST;
                            toCommon.Transition.StateAfter = D3D12_RESOURCE_STATE_COMMON;
                            toCommon.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            m_cmdList->ResourceBarrier(1, &toCommon);

                            depthInfos[eye].subImage.swapchain = m_eyeSwapchains[eye].depthHandle;
                            depthInfos[eye].subImage.imageRect.offset = {0, 0};
                            depthInfos[eye].subImage.imageRect.extent = {m_eyeSwapchains[eye].width, m_eyeSwapchains[eye].height};
                            depthInfos[eye].subImage.imageArrayIndex = 0;
                            // OpenXR requires minDepth < maxDepth in [0,1]. Reversed-Z is encoded
                            // by swapping nearZ/farZ (nearZ > farZ), NOT by swapping min/max depth.
                            depthInfos[eye].minDepth = 0.0f;
                            depthInfos[eye].maxDepth = 1.0f;
                            depthInfos[eye].nearZ = 10000.0f;
                            depthInfos[eye].farZ = 0.01f;
                            projectionViews[eye].next = &depthInfos[eye];
                        }
                    }

                    if (!useDepthLayer) {
                        for (uint32_t eye = 0; eye < viewCountOutput; ++eye) {
                            projectionViews[eye].next = nullptr;
                        }
                    }

                    if (!copyReady) {
                        for (uint32_t eye = 0; eye < viewCountOutput; ++eye) {
                            if (!acquiredEyes[eye]) {
                                continue;
                            }
                            XrSwapchainImageReleaseInfo releaseInfo{XR_TYPE_SWAPCHAIN_IMAGE_RELEASE_INFO};
                            const XrResult releaseRes = xrReleaseSwapchainImage(m_eyeSwapchains[eye].handle, &releaseInfo);
                            if (XR_FAILED(releaseRes)) {
                                Log("OpenXRManager: xrReleaseSwapchainImage cleanup failed for eye %u (res=%d)\n", eye, releaseRes);
                            }
                        }
                        for (uint32_t eye = 0; eye < viewCountOutput; ++eye) {
                            if (!acquiredDepthEyes[eye]) {
                                continue;
                            }
                            XrSwapchainImageReleaseInfo releaseInfo{XR_TYPE_SWAPCHAIN_IMAGE_RELEASE_INFO};
                            const XrResult releaseRes = xrReleaseSwapchainImage(m_eyeSwapchains[eye].depthHandle, &releaseInfo);
                            if (XR_FAILED(releaseRes)) {
                                Log("OpenXRManager: [DEPTH] xrReleaseSwapchainImage cleanup failed for eye %u (res=%d)\n", eye, releaseRes);
                            }
                        }

                        m_cmdList->Close();
                        // NOTE: intentionally NO m_d3dQueue->Wait on the depth-writer fence
                        // here. Blocking the present queue on that fence deadlocks during
                        // menu/scene loads: the game's depth-writer queue can be parked
                        // behind its own async-compute Wait, so the fence is not reached,
                        // the present queue stalls, and the m_fence CPU wait next present
                        // freezes the game. The depth resolve is a REPROJECTION HINT: a
                        // rare frame-stale/torn read is invisible, a freeze is not. So we
                        // let the copy race (benign) instead of gating on the writer queue.
                        (void)monoDepthFence;
                        ID3D12CommandList* cmdLists[] = {m_cmdList};
                        m_d3dQueue->ExecuteCommandLists(1, cmdLists);
                        
                        ++m_fenceValue;
                        m_d3dQueue->Signal(m_fence, m_fenceValue);
                    } else {
                        m_cmdList->Close();
                        ID3D12CommandList* cmdLists[] = {m_cmdList};
                        m_d3dQueue->ExecuteCommandLists(1, cmdLists);
                        
                        ++m_fenceValue;
                        m_d3dQueue->Signal(m_fence, m_fenceValue);

                        bool releaseOk = true;
                        for (uint32_t eye = 0; eye < viewCountOutput; ++eye) {
                            if (!acquiredEyes[eye]) {
                                continue;
                            }
                            XrSwapchainImageReleaseInfo releaseInfo{XR_TYPE_SWAPCHAIN_IMAGE_RELEASE_INFO};
                            const XrResult releaseRes = xrReleaseSwapchainImage(m_eyeSwapchains[eye].handle, &releaseInfo);
                            if (XR_FAILED(releaseRes)) {
                                Log("OpenXRManager: xrReleaseSwapchainImage failed for eye %u (res=%d)\n", eye, releaseRes);
                                releaseOk = false;
                            }
                        }
                        for (uint32_t eye = 0; eye < viewCountOutput; ++eye) {
                            if (!acquiredDepthEyes[eye]) {
                                continue;
                            }
                            XrSwapchainImageReleaseInfo releaseInfo{XR_TYPE_SWAPCHAIN_IMAGE_RELEASE_INFO};
                            const XrResult releaseRes = xrReleaseSwapchainImage(m_eyeSwapchains[eye].depthHandle, &releaseInfo);
                            if (XR_FAILED(releaseRes)) {
                                Log("OpenXRManager: [DEPTH] xrReleaseSwapchainImage failed for eye %u (res=%d)\n", eye, releaseRes);
                                useDepthLayer = false;
                            }
                        }

                        if (!useDepthLayer) {
                            for (uint32_t eye = 0; eye < viewCountOutput; ++eye) {
                                projectionViews[eye].next = nullptr;
                            }
                        }

                        if (releaseOk) {
                            XrCompositionLayerProjection layerProj{XR_TYPE_COMPOSITION_LAYER_PROJECTION};
                            XrCompositionLayerQuad layerQuad{XR_TYPE_COMPOSITION_LAYER_QUAD};
                            const XrCompositionLayerBaseHeader* layers[1] = {nullptr};

                            if (menuRectActive) {
                                layerQuad.space = m_localSpace;
                                layerQuad.eyeVisibility = XR_EYE_VISIBILITY_BOTH;
                                layerQuad.subImage = projectionViews[0].subImage;

                            // LAZY-FOLLOW panel: stays put within the dead-zone, eases
                            // to the head past the threshold (see ComputeMenuQuadPose).
                            layerQuad.pose = ComputeMenuQuadPose(headPoseLocated, location.pose);

                                float quadWidth = 2.0f * 1.5f * tanf(GetMenuFov() * 3.14159f / 180.0f * 0.5f);
                                layerQuad.size = {quadWidth, quadWidth};
                                layers[0] = reinterpret_cast<const XrCompositionLayerBaseHeader*>(&layerQuad);
                            } else {
                                layerProj.space = m_localSpace;
                                layerProj.viewCount = viewCountOutput;
                                layerProj.views = projectionViews.data();
                                layers[0] = reinterpret_cast<const XrCompositionLayerBaseHeader*>(&layerProj);
                            }

                            XrFrameEndInfo endInfo{XR_TYPE_FRAME_END_INFO};
                            endInfo.displayTime = frameState.predictedDisplayTime;
                            endInfo.environmentBlendMode = XR_ENVIRONMENT_BLEND_MODE_OPAQUE;
                            endInfo.layerCount = 1;
                            endInfo.layers = layers;
                            const XrResult endRes = xrEndFrame(m_session, &endInfo);
                            if (XR_SUCCEEDED(endRes)) {
                                // DIAG: the angular gap between the SUBMITTED render pose
                                // (the head pose the captured frame was rendered with) and
                                // the CURRENT head pose (location.pose, freshly located this
                                // frame-thread tick). On a head turn this gap = the render->
                                // display latency the compositor must reproject; a large gap
                                // (many deg) is exactly the "image stretches on turn" — it's
                                // capture-pipeline latency, not FOV. Logged unconditionally
                                // for the first calls so we can MEASURE it.
                                {
                                    static uint32_t s_pd = 0;
                                    if (s_pd < 80) {
                                        const XrQuaternionf& a = monoPoses[0].orientation;
                                        const XrQuaternionf& b = location.pose.orientation;
                                        float dot = a.x*b.x + a.y*b.y + a.z*b.z + a.w*b.w;
                                        dot = dot < 0.0f ? -dot : dot; if (dot > 1.0f) dot = 1.0f;
                                        const float gapDeg = 2.0f * acosf(dot) * 180.0f / 3.1415926535f;
                                        Log("POSEDIAG: render->live head angular gap = %.2f deg | submitSerial=%llu lastSub=%llu (turn your head to read latency)\n",
                                            gapDeg, (unsigned long long)presentSerial, (unsigned long long)m_lastSubmittedSerial);
                                        s_pd++;
                                    }
                                }
                                if ((presentSerial % 300) == 1) {
                                    Log("OpenXRManager: Mono frame submitted. serial=%llu fresh=%d views=%u shouldRender=%d depth=%d\n",
                                        static_cast<unsigned long long>(presentSerial),
                                        presentSerial != m_lastSubmittedSerial ? 1 : 0,
                                        viewCountOutput,
                                        frameState.shouldRender ? 1 : 0,
                                        useDepthLayer ? 1 : 0);
                                }
                                m_lastSubmittedSerial = presentSerial;
                                monoSource->Release();
                                if (monoDepthSource) monoDepthSource->Release();
                                if (m_frameSyncEvent) {
                                    SetEvent(m_frameSyncEvent);
                                }
                                continue;
                            }

                            Log("OpenXRManager: xrEndFrame mono submit failed (res=%d)\n", endRes);
                        }
                    }
                }

                if (monoSource) {
                    monoSource->Release();
                }
                if (monoDepthSource) {
                    monoDepthSource->Release();
                }
            }
        } else if (monoEnabled && ((++monoWaitLogCounter % 300) == 1)) {
            Log("OpenXRManager: %s submit waiting. ready=%d views=%zu shouldRender=%d\n",
                useAerSubmit ? "AER" : "Mono",
                monoReady ? 1 : 0,
                m_views.size(),
                frameState.shouldRender ? 1 : 0);
        }

        XrFrameEndInfo endInfo{XR_TYPE_FRAME_END_INFO};
        endInfo.displayTime = frameState.predictedDisplayTime;
        endInfo.environmentBlendMode = XR_ENVIRONMENT_BLEND_MODE_OPAQUE;
        endInfo.layerCount = 0;
        endInfo.layers = nullptr;
        xrEndFrame(m_session, &endInfo);
        
        if (m_frameSyncEvent) {
            SetEvent(m_frameSyncEvent);
        }
    } while (false);

    return 0;
}
