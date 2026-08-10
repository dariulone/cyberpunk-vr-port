// openxr_frameloop.cpp - the XR frame loop (PumpInlineFrame / FrameThreadMain).
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
#include "../../common/log_throttle.h"

// Run the XR cycle on every display frame and let xrWaitFrame pace it, re-submitting the
// last snapshot with ITS OWN pose when the game has not produced a new one. 0 restores the
// old behaviour (block until a fresh game frame, skipping XR frames entirely).
//
// This matches how the established injected-VR mods structure it: fholger's crysis_vrmod
// runs AwaitFrame (xrWaitFrame -> xrBeginFrame -> xrLocateViews) -> game renders ->
// FinishFrame (xrEndFrame) once per frame and never skips the cycle, and RealVR's CP2077
// loop is the same shape. The invariant they both keep is that the pose in the projection
// layer is the one the image was rendered against -- not one located at submit time.
extern "C" __declspec(dllexport) int CyberpunkVR_XrPaceByRuntime = 1;

// Definition for the declaration in openxr_manager.h -- see UseThreadedSubmit there, which is
// where the -1 policy lives.
//
// -1 = decide by runtime, 0 = force inline, 1 = force threaded. Set from xr_threaded_submit.
//
// IT USED TO DEFAULT TO 0, AND THE MEASUREMENT BEHIND THAT WAS REAL BUT NARROW. It read 16311 XR
// cycles against ~18469 presents -- the thread running slightly SLOWER than the game -- and
// concluded the thread gains nothing and costs phase, with 53% of present intervals holding
// either two locates or none. All true, and all measured on a runtime that tolerates a late
// frame.
//
// What it missed is what the inline path costs on a runtime that does NOT. Inline, the whole XR
// cycle runs inside the game's Present: xrWaitFrame, an acquire/wait per eye swapchain, then
// xrEndFrame, and only then the real Present. Every one of those is paced by the compositor, so
// the engine cannot start its next frame until the compositor allows it. Where the cadence is
// enforced strictly, a frame that misses the deadline by any margin is served a whole display
// period late, and the engine locks to exactly half the display rate -- measured on a Pimax
// Crystal Super, and identical at native resolution, at 50% resolution, and with DLSS on
// Performance, because the wait is on a clock rather than on work. Turning this on took the same
// machine from a pinned 45 to an unlocked frame rate.
//
// So phase jitter is the price of the threaded path and half rate is the price of the inline one.
// The default now picks per runtime instead of paying the second price everywhere.
extern "C" __declspec(dllexport) int CyberpunkVR_ThreadedMonoSubmit = -1;
// Display cycles driven versus frames actually submitted; the difference is the freeze.
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugXrCycles = 0;
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugMonoSubmits = 0;

// ---- WHAT INSTANT THE POSE IS AIMED AT ------------------------------------------------------
//
// 0 = this cycle's predictedDisplayTime (what it always was).
// 1 = predictedDisplayTime + period * PosePredictScale -- a constant offset (the UEVR shape).
// 2 = the measured fit (the RealVR shape): predict the display time of the frame the game is
//     building right now from the rolling regression, and locate everything at that.
//
// 2 is the default because there is no constant to pick: the XR loop is paced by xrWaitFrame at
// the headset rate on its own thread while the game presents at its own, so the offset between
// "the cycle that located this pose" and "the cycle that shows the frame built from it" drifts
// continuously. Mode 2 falls back to mode 1's arithmetic until the fit has enough samples.
extern "C" __declspec(dllexport) int   CyberpunkVR_PosePredictMode  = 2;
extern "C" __declspec(dllexport) float CyberpunkVR_PosePredictScale = 1.0f;
// How much of the measured frame-ahead offset to actually lead by, 0..1.
//
// 0 = aim exactly at the runtime's own predictedDisplayTime and add nothing. Strict: you turn
// your head, the world turns, and no part of the image was ever drawn from a viewpoint the head
// did not occupy. What latency remains is the compositor's job, and reprojection does it well.
// 1 = lead by a full game frame, which is textbook for an app that renders inside its own XR
// frame, and is what made the camera over-rotate here: we render one frame late, so leading on
// top of that pushes xrLocateSpace into 40-50 ms of extrapolation and it overshoots on direction
// changes. Raise it only if the world feels laggy AND the overshoot does not come back.
extern "C" __declspec(dllexport) float CyberpunkVR_PoseAimScale = 0.0f;
// Diagnostics: the measured game-frame period in microseconds, and how often the fit was used
// versus fell back. A slope that does not settle near the real frame time means the fit is being
// fed garbage and mode 1 is the honest choice.
// ---- HOW MANY PRESENTS AHEAD THE FRAME BEING BUILT WILL BE SHOWN ----------------------------
//
// We assume the frame the game is building now appears at present (count + 1). REDengine has its
// own render thread, so the frame presented at N may have been simulated with the camera written
// two intervals back rather than one. If that is so, every image is systematically one present
// behind the pose it is labelled with: the compositor over-warps, the next frame pulls it back,
// and that is judder that survives a perfectly paired pose ring -- which is exactly what the
// counters now show (SlotHit 11591, SlotReused 0, SlotMiss 0, and the judder still there).
//
// This is one integer and it decides the answer, so it is a knob rather than a guess. It is
// applied in BOTH places that must agree: the serial the slot is published under, and the serial
// the camera's locate is aimed at. The submit side keeps looking up its own present serial.
extern "C" __declspec(dllexport) int CyberpunkVR_EnginePipelineDepth = 0;

// 1 = re-enable the adaptive head-pose smoother (xr_hmd_smooth). Off by default; see the use
// site for why a motion-dependent filter on the VIEW pose is judder rather than smoothing.
// Live-switchable so the two can be compared inside one session.
extern "C" __declspec(dllexport) int CyberpunkVR_HeadFilter = 1;
// How the submitted centre pose was obtained. Written = the exact sample the camera was built
// from (correct). Slot = the frame loop's own locate (a different sample; only right when the
// written one is missing).
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugPoseFromWrite = 0;
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugPoseFromSlot  = 0;
// Read-back identification of the frame's pose (vr_core.cpp / openxr_present.cpp).
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugFinalMatch;
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugFinalNoMatch;
extern "C" __declspec(dllexport) unsigned int       CyberpunkVR_DebugFinalAge;
extern "C" __declspec(dllexport) unsigned int       CyberpunkVR_DebugFinalTies;
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugFinalExact;
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugFinalApprox;
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugFinalExactTies;
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugFinalTieHits;
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugPoseReadBack;
extern "C" __declspec(dllexport) unsigned int      CyberpunkVR_DebugFitSlopeUs = 0;
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugFitUsed   = 0;
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugFitMissed = 0;

// ---- real stereo: VRCAM as the right eye ---------------------------------------------
// DEFAULT ON -- it has now run a session without incident.
//
// Safe as a default because it is self-disabling: the accessor returns null unless the second
// view produced a frame within the last few hundred ms, so menus, loads and a disabled VRCAM
// component all fall back to the mono path byte-for-byte.
extern "C" __declspec(dllexport) int CyberpunkVR_StereoSubmit = 1;
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugStereoEyeSubmits = 0;
// The VRCAM view's final colour, published by stereo/sync_stereo.cpp. "Fresh" = null once
// that view stops updating; see the definition for why existence alone is the wrong test.
extern "C" ID3D12Resource* CyberpunkVR_GetVrcamEyeTextureFresh();
// Tells sync_stereo the submit wants the snapshot taken; see the assignment in the submit.
extern "C" int CyberpunkVR_StereoEyeCapture;
// 1 = MAIN goes to the RIGHT eye and VRCAM to the left. Swaps the IMAGES only; the submitted
// per-eye poses stay as the runtime gave them, which is what keeps the stereo honest. The
// camera separation sign and the barrel dot are flipped alongside it in their own files --
// swapping any one of the three on its own inverts the depth.
extern "C" __declspec(dllexport) int CyberpunkVR_MainIsRightEye = 1;
// Collapse the two submitted eye poses to one orientation and the midpoint position whenever both
// eyes are shown a single image. On a canted headset the runtime's per-eye orientations differ, and
// anchoring one image at both of them pushes the copies apart in the direction the eyes cannot
// follow. 1 = collapse (the R.E.A.L.-VR-for-Witcher-3 shape), 0 = submit the runtime's pair.
extern "C" __declspec(dllexport) int CyberpunkVR_MonoCyclopeanPose = 1;
// How far away flat content should sit when both eyes are shown the same image -- the intro, the
// menus, and any frame the second view could not fill. 0 leaves it at infinity, which is what
// submitting one image to two eye poses amounts to and what makes it double. Metres.
//
// OFF, and it should have been from the start. Twice now it has been the visible regression:
// first rotating one eye and not the other (the probe caught 1.130 deg on the right eye alone),
// and then, with that gate fixed to "both eyes or neither", shifting the whole world sideways --
// "one eye is to the right of the other".
//
// The second one is the fundamental objection, not a bug in the gate. "Both eyes share one image"
// is true of the intro logo, which is flat and wants a distance -- and equally true of GAMEPLAY
// whenever the second view has not produced a frame, where the image is a real 3D scene at every
// depth at once. Converging that by ipd/distance is a flat 2.26 deg horizontal offset between the
// eyes, applied to a world that already had its own disparity. There is no signal here that tells
// the two cases apart, and the mono fallback is common precisely while the second eye is
// misbehaving -- so the one moment this would fire in gameplay is the worst one for it.
//
// A doubled intro logo is a far smaller price. Left tunable for anyone who wants to try it on a
// build where the second eye is solid.
extern "C" __declspec(dllexport) float CyberpunkVR_FlatDistanceM = 0.0f;

void OpenXRManager::PumpInlineFrame() {
    // When the dedicated submit thread owns the XR frame loop, the Present thread must
    // NOT drive xrWaitFrame/submit here: those blocking fence/swapchain + compositor
    // waits would freeze the game on the Present thread (severely so under SteamVR's
    // frame pacing). Just make sure the submit thread is awake and return immediately.
    if (UseThreadedSubmit()) {
        NotifySubmitThread();
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


        // Mono cadence gate: if no NEW successfully captured game frame exists,
        // do not keep submitting the same snapshot as a brand new XR frame. That
        // defeats runtime motion smoothing and manifests as strong ghosting/double
        // images on head turns. Instead, wait for a fresh present and let the
        // runtime see the app's true cadence.
        // PACE BY THE RUNTIME, NOT BY THE GAME (default).
        //
        // The block below waits for a fresh game frame before running the XR cycle at all.
        // That skips xrWaitFrame/xrEndFrame for every display frame the game did not manage
        // to fill -- and xrWaitFrame IS the pacing primitive. The runtime is then left with
        // nothing to compose for those frames: it holds the previous one and catches up when
        // ours finally arrives, and the phase between our submits and the display drifts.
        // The beat that produces is worst when the game rate sits well under the display
        // rate, which is exactly the reported behaviour: smooth above ~80 fps, juddering at
        // 50-60 on a 90 Hz headset.
        //
        // Re-submitting the same image is the CORRECT thing for an app slower than the
        // display, and it is safe here because the pose travels with the snapshot
        // (m_monoCapturedFrame.poses) rather than being located afresh at submit time. The
        // runtime sees an unchanged pose on a repeat and reprojects it itself -- which is
        // what makes 45 fps look smooth on a 90 Hz display, and it leaves the choice of
        // ASW/reprojection where it belongs, with the runtime and the user.
        //
        // The old wait is kept behind the flag: its comment was written when the pose WAS
        // re-located per submit, and under that condition it was right.
        if (!CyberpunkVR_XrPaceByRuntime &&
            m_monoSubmitEnabled.load(std::memory_order_relaxed) &&
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
        // Count display cycles against actual submissions. A cycle that reaches xrWaitFrame
        // but never gets to xrEndFrame is a display frame the runtime had nothing new for --
        // it holds the previous composition, which is exactly what a freeze in the headset
        // looks like while the game itself keeps running. Naming that number turns "sometimes
        // it freezes" into something measurable.
        ++CyberpunkVR_DebugXrCycles;
        if ((CyberpunkVR_DebugXrCycles % 600) == 0) {
            Log("XR pacing: cycles=%llu submits=%llu missed=%llu (%.1f%%)\n",
                (unsigned long long)CyberpunkVR_DebugXrCycles,
                (unsigned long long)CyberpunkVR_DebugMonoSubmits,
                (unsigned long long)(CyberpunkVR_DebugXrCycles - CyberpunkVR_DebugMonoSubmits),
                CyberpunkVR_DebugXrCycles
                    ? 100.0 * double(CyberpunkVR_DebugXrCycles - CyberpunkVR_DebugMonoSubmits)
                        / double(CyberpunkVR_DebugXrCycles)
                    : 0.0);
        }
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

        // ONE instant for every locate in this cycle, aimed at the frame that will USE the pose.
        //
        // The spec's own guidance is blunt about this: "every stage in the engine pipeline should
        // use the exact same display time for one particular application-generated frame", and
        // small inconsistencies "accumulate and cause visible judder". Until now each locate
        // (views, view space, both hands) independently passed frameState.predictedDisplayTime --
        // the same value, but aimed at THIS cycle rather than at the cycle that will display the
        // frame the game is building. See CyberpunkVR_PosePredictMode.
        int pipeDepth = CyberpunkVR_EnginePipelineDepth;
        if (pipeDepth < 0) pipeDepth = 0;
        if (pipeDepth > 3) pipeDepth = 3;
        const uint64_t buildingSerial =
            m_presentCount.load(std::memory_order_relaxed) + 1 + static_cast<uint64_t>(pipeDepth);
        // ONE CONTINUOUS EXPRESSION, NEVER TWO FORMULAS.
        //
        // The aim used to switch between the regression's answer (85% of frames) and a fallback
        // `predictedDisplayTime + period*scale` (the other 15%). Those are two different instants
        // about a frame apart, so every sixth or seventh frame was located at a visibly different
        // target -- a jump, then back. With the slots now pairing perfectly (SlotHit 9535,
        // Reused 0, Miss 0) that switching was the whole remaining spread: the measured
        // render->live gap sat at avg ~1.2 deg with excursions to 6.
        //
        // So the fit contributes an OFFSET, smoothed, and the aim is always
        // `predictedDisplayTime + offset*scale`. When the fit is rejected the offset simply
        // persists; nothing jumps, because there is no other formula to jump to.
        //
        // Scale defaults to 0: aim exactly where the runtime says the frame will be shown, and
        // add no lead of our own. Leading further is what produced "the camera over-rotates" --
        // xrLocateSpace extrapolating 40-50 ms overshoots on a direction change and draws the
        // world from a viewpoint the head never occupied, which reprojection cannot undo. The
        // residual latency of NOT leading is exactly what the compositor's reprojection is for.
        static double s_aimOffsetNs = 0.0;
        XrTime locateTime = frameState.predictedDisplayTime;
        if (CyberpunkVR_PosePredictMode == 2) {
            XrTime fitted = 0;
            const XrTime per = static_cast<XrTime>(frameState.predictedDisplayPeriod);
            if (FitPredictDisplayTime(buildingSerial, &fitted) && per > 0 &&
                fitted > frameState.predictedDisplayTime - per &&
                fitted < frameState.predictedDisplayTime + per * 4) {
                const double raw = static_cast<double>(fitted - frameState.predictedDisplayTime);
                s_aimOffsetNs = s_aimOffsetNs * 0.90 + raw * 0.10;   // slow, so it cannot jitter
                ++CyberpunkVR_DebugFitUsed;
            } else {
                ++CyberpunkVR_DebugFitMissed;                        // keep the last offset
            }
            CyberpunkVR_DebugFitSlopeUs = static_cast<unsigned int>(FitSlopeNs() / 1000.0);
            locateTime = frameState.predictedDisplayTime +
                static_cast<XrTime>(s_aimOffsetNs * CyberpunkVR_PoseAimScale);
        } else if (CyberpunkVR_PosePredictMode == 1) {
            locateTime = frameState.predictedDisplayTime +
                static_cast<XrTime>(frameState.predictedDisplayPeriod *
                                    CyberpunkVR_PosePredictScale);
        }
        // Publish it so the camera write aims at the very same instant instead of deriving its
        // own -- see SetFrameAimTime().
        SetFrameAimTime(locateTime);

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
        const bool monoReady = monoEnabled && EnsureMonoSubmitResources() && !m_eyeSwapchains.empty();
        if (monoReady && !m_views.empty()) {
            XrViewLocateInfo viewLocateInfo{XR_TYPE_VIEW_LOCATE_INFO};
            viewLocateInfo.viewConfigurationType = XR_VIEW_CONFIGURATION_TYPE_PRIMARY_STEREO;
            viewLocateInfo.displayTime = locateTime;
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

            }
        }

        XrSpaceVelocity headVelocity{XR_TYPE_SPACE_VELOCITY};
        XrSpaceLocation location{XR_TYPE_SPACE_LOCATION};
        location.next = &headVelocity;
        res = xrLocateSpace(m_viewSpace, m_localSpace, locateTime, &location);
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
            // Mirror it for the submit path, which has to undo this transform to put a rendered
            // pose back into local space. See GetRecenterBase().
            PublishRecenterBase(basePose);

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
            // HEAD SMOOTHING IS OFF BY DEFAULT, AND THAT IS NOT A TASTE CALL.
            //
            // adaptiveFollow does not low-pass the head at a fixed cutoff; it changes the
            // follow factor with how fast the head is moving. At the shipped xr_hmd_smooth of
            // 0.35 that is 1/8 per cycle when nearly still (~130 ms of lag) rising to ~1.0 once
            // motion passes ~2 deg per frame. So the lag on the head breathes with your own
            // movement, and there is no pose you can submit that describes it: the compositor
            // reprojects against a pose whose relationship to the pixels keeps changing. That is
            // judder by construction, it only shows while moving, and no amount of prediction or
            // slot-pairing can remove it.
            //
            // A VR view must be raw. Jitter, if any, belongs to the tracker and the runtime, not
            // to a filter we add on the render path. Hands keep their own filter -- they are IK
            // targets, not the viewpoint.
            if (CyberpunkVR_HeadFilter) {
                filteredHeadPose = filterTrackingPose(
                    m_headFilterState,
                    filteredHeadPose,
                    GetHmdTrackingSmooth(),
                    0.0012f,
                    0.0080f,
                    0.0035f,
                    0.0350f);
            } else {
                m_headFilterState.initialized = false;
            }

            // Everything this frame will need, stored together and stamped with the present that
            // will show it. From here the submit reads pose, per-eye offset and per-eye FOV out
            // of one slot instead of assembling them from three different moments.
            if (m_views.size() >= 2) {
                PublishFrameSlot(buildingSerial, locateTime, m_views.data(),
                                 static_cast<uint32_t>(m_views.size()), location.pose);
            }

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
                // ONE INSTANT: the head position that goes with these controller poses, plus the
                // age stamp, both taken inside the hand lock. The head atomics are written a few
                // lines above in this same iteration but OUTSIDE the lock, so a flush landing
                // between the two would have paired this iteration's head with the previous
                // iteration's hands -- 11 ms of silent mismatch at 90 Hz, in the one place that
                // exists to remove mismatch.
                {
                    LARGE_INTEGER c{}, f{};
                    QueryPerformanceCounter(&c);
                    QueryPerformanceFrequency(&f);
                    const double ms = (f.QuadPart > 0)
                        ? (double)c.QuadPart * 1000.0 / (double)f.QuadPart : 0.0;
                    m_handSampleMs.store((float)fmod(ms, 100000.0), std::memory_order_relaxed);
                    const bool dof3 = (Get3DofMovement() != 0);
                    m_handSampleHeadPos[0] = dof3 ? 0.0f : m_posX.load(std::memory_order_relaxed);
                    m_handSampleHeadPos[1] = dof3 ? 0.0f : m_posY.load(std::memory_order_relaxed);
                    m_handSampleHeadPos[2] = dof3 ? 0.0f : m_posZ.load(std::memory_order_relaxed);
                    m_handSampleHeadValid = true;
                }
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
                        XrResult locRes = xrLocateSpace(m_handSpaces[i], m_localSpace, locateTime, &handLoc);


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
                        if (XR_SUCCEEDED(xrLocateSpace(m_handAimSpaces[i], m_localSpace, locateTime, &aimLoc)) &&
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
            // pose so GetHeadPose() can forward-predict.
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
            {
                ID3D12Resource* monoSource = nullptr;
                ID3D12Resource* monoDepthSource = nullptr;
                uint64_t presentSerial = 0;
                uint64_t monoDepthFence = 0;   // writer-queue fence guarding monoDepthSource
                XrPosef monoPoses[2]{};
                XrFovf monoFovs[2]{};
                bool monoHasView[2] = {};
                bool monoHasDepth = false;
                uint64_t monoCaptureFence = 0;
                {
                    std::lock_guard<std::mutex> lock(m_presentMutex);
                    if (m_monoCapturedFrame.texture &&
                        m_monoCapturedFrame.serial != 0 &&
                        m_monoCapturedFrame.hasView[0] &&
                        m_monoCapturedFrame.hasView[1]) {
                        monoSource = m_monoCapturedFrame.texture;
                        monoSource->AddRef();
                        presentSerial = m_monoCapturedFrame.serial;
                        monoCaptureFence = m_monoCapturedFrame.captureFence;
                        for (int eye = 0; eye < 2; ++eye) {
                            monoPoses[eye] = m_monoCapturedFrame.poses[eye];
                            monoFovs[eye] = m_monoCapturedFrame.fovs[eye];
                            monoHasView[eye] = m_monoCapturedFrame.hasView[eye];
                        }
                        // Accept a slightly older depth snapshot instead of dropping the
                        // layer for that frame.
                        //
                        // The snapshot is only taken while the game depth is shader-readable;
                        // measured with VRCAM on, at Present it is often still in DEPTH_WRITE
                        // (state 0x10 rather than 0xE0), so an exact serial match makes the
                        // depth layer appear and disappear between frames. A compositor that
                        // gets depth on one frame and none on the next keeps switching
                        // reprojection modes, which is worse than never sending it. Depth is a
                        // reprojection hint, and one a few frames old is still a good one --
                        // the camera has barely moved -- so the layer stays present and
                        // consistent.
                        const uint64_t depthAge =
                            m_monoCapturedFrame.serial >= m_depthSnapshotSerial
                                ? m_monoCapturedFrame.serial - m_depthSnapshotSerial
                                : 0;
                        if (m_depthLayerSupported &&
                            m_depthSnapshot &&
                            m_depthSnapshotSerial != 0 &&
                            depthAge <= 8) {
                            monoDepthSource = m_depthSnapshot;
                            monoDepthSource->AddRef();
                            monoDepthFence = m_depthSnapshotWriterFence;
                            monoHasDepth = true;
                        }
                    }
                }

                // DO NOT take m_captureMutex here.
                //
                // The game's Present takes that same mutex for its capture, so holding it
                // across a submit -- which now happens once per DISPLAY frame and waits on
                // the compositor inside xrAcquireSwapchainImage/xrWaitSwapchainImage -- makes
                // the game's Present wait on the compositor through us. That is uneven frame
                // delivery, i.e. judder, and it is self-inflicted: it appeared when the mono
                // submit moved onto this thread.
                //
                // RealVR avoids the whole class of problem with a 3-slot queue where producer
                // and consumer never touch the same buffer. The same effect, far less code:
                // announce that the snapshot is being read, and let the producer SKIP its
                // capture for that frame instead of blocking. A skipped capture costs
                // nothing now -- the previous frame is re-submitted with its own pose, which
                // is exactly what the runtime wants anyway.

                m_cmdAllocatorIndex = (m_cmdAllocatorIndex + 1) % 3;
                ID3D12CommandAllocator* currentAllocator = m_cmdAllocators[m_cmdAllocatorIndex];
                if (m_fenceValue >= 3 && m_fence->GetCompletedValue() < m_fenceValue - 2) {
                    m_fence->SetEventOnCompletion(m_fenceValue - 2, m_fenceEvent);
                    WaitForSingleObject(m_fenceEvent, 50);   // 50 ms, not 1000: a display frame is ~11 ms, so a
                    // full second of waiting here IS the freeze it was meant to prevent. On
                    // timeout Reset fails, this cycle skips, and the next one submits again.
                }

                ID3D12GraphicsCommandList* m_cmdList = m_cmdLists[m_cmdAllocatorIndex];

                // ---- the second eye -------------------------------------------------------
                // OUR OWN right-eye copy, produced at Present on the capture list (see
                // EnsureVrcamEyeTexture / the blit in CaptureMonoPresentedFrame). By the time
                // it gets here it is already the eye swapchain's format and size, so it is
                // copied exactly the way MAIN's snapshot is -- no shader work, no engine
                // resource touched from this thread. That separation is the fix for the GPU
                // hang the submit-side version caused.
                //
                // Requiring the same serial keeps the pair honest: a frame with no VRCAM blit
                // (component off, menus, loading, or a skipped capture) leaves this null and
                // both eyes get MAIN, as before.
                //
                // The capture request follows the switch: the engine-side snapshot is a
                // full-resolution copy in the engine's own list (~24 MB a frame at 2444x2444),
                // so it must stop when nothing consumes it. sync_stereo ORs this with its own
                // mirror-window request, so turning stereo off does not break the mirror.
                CyberpunkVR_StereoEyeCapture = CyberpunkVR_StereoSubmit ? 1 : 0;
                ID3D12Resource* vrcamEye = nullptr;
                if (CyberpunkVR_StereoSubmit && viewCountOutput >= 2) {
                    std::lock_guard<std::mutex> lock(m_presentMutex);
                    if (m_vrcamEyeTex && m_vrcamEyeSerial != 0 &&
                        m_vrcamEyeSerial == presentSerial) {
                        vrcamEye = m_vrcamEyeTex;
                        vrcamEye->AddRef();   // the capture may recreate it on a resize
                    }
                }

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

                        // CAS sharpen: when xr_sharpness>0, draw
                        // the sharpened mono source straight into the swapchain image.
                        // monoSource is always COMMON here (no synth scratch in mono).
                        const float monoSharp = GetVrSharpness();
                        bool doMonoSharpen = false;
                        // DISABLED: in-submit CAS GPU-crashes; needs an
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
                        } else if (eye == (CyberpunkVR_MainIsRightEye ? 0u : 1u) && vrcamEye) {
                            {
                                // Identical in shape to the MAIN branch below -- both operands
                                // are ours, both already in the swapchain's format and size, so
                                // this is a plain copy and nothing here reaches into an engine
                                // resource. m_vrcamEyeTex rests in COPY_SOURCE between frames,
                                // which is where the capture's own barriers leave it.
                                D3D12_RESOURCE_BARRIER toCopyDest{};
                                toCopyDest.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                                toCopyDest.Transition.pResource = texture;
                                toCopyDest.Transition.StateBefore = D3D12_RESOURCE_STATE_COMMON;
                                toCopyDest.Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_DEST;
                                toCopyDest.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                                m_cmdList->ResourceBarrier(1, &toCopyDest);

                                m_cmdList->CopyResource(texture, vrcamEye);

                                D3D12_RESOURCE_BARRIER toCommon{};
                                toCommon.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                                toCommon.Transition.pResource = texture;
                                toCommon.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_DEST;
                                toCommon.Transition.StateAfter = D3D12_RESOURCE_STATE_COMMON;
                                toCommon.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                                m_cmdList->ResourceBarrier(1, &toCommon);
                                ++CyberpunkVR_DebugStereoEyeSubmits;
                            }
                        }
                        if (!(doMonoSharpen ||
                              (eye == (CyberpunkVR_MainIsRightEye ? 0u : 1u) && vrcamEye))) {
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

                        // ONE ORIENTATION FOR BOTH EYES WHEN THEY SHARE ONE IMAGE.
                        //
                        // On a CANTED headset the runtime hands back two eye poses whose
                        // orientations differ -- the Pimax Dream Air reports 4 deg of frustum
                        // asymmetry, 2 per eye, and the panels are physically turned by the same
                        // amount. Anchor one image at each of those orientations and its centre
                        // lands 2 deg outward in the left eye and 2 deg outward in the right: the
                        // two copies end up 4 deg apart, and apart in the DIVERGENT direction. No
                        // eyes fuse that. "Both images too far to the sides to converge" is a
                        // description of exactly this, not of content at infinity.
                        //
                        // The Witcher 3 VR mod, which the same tester reports working on the same
                        // headset, collapses the pair: one orientation (eye 0's) and the midpoint
                        // position go to both views, and the per-eye difference is carried
                        // entirely by each eye's own FOV. Same shape here.
                        //
                        // Nothing is lost on a headset with parallel panels -- the two
                        // orientations are equal there, so this is the identity. Which is also why
                        // it went unnoticed: every setup it has run on here has flat lenses.
                        if (eye == 0 && monoHasView[0] && monoHasView[1]) {
                            // The number this rests on, said out loud once: the angle between the
                            // two eye orientations the runtime handed us. Zero on flat lenses,
                            // twice the frustum asymmetry on canted ones.
                            static bool s_cantLogged = false;
                            if (!s_cantLogged) {
                                s_cantLogged = true;
                                const XrQuaternionf& a = monoPoses[0].orientation;
                                const XrQuaternionf& b = monoPoses[1].orientation;
                                float dot = a.x*b.x + a.y*b.y + a.z*b.z + a.w*b.w;
                                if (dot < 0.0f) dot = -dot;
                                if (dot > 1.0f) dot = 1.0f;
                                Log("OpenXRManager[CANT]: eye-pose orientations differ by %.3f deg "
                                    "(cyclopean=%d flatDist=%.2f)\n",
                                    2.0f * acosf(dot) * 57.2957795f,
                                    CyberpunkVR_MonoCyclopeanPose ? 1 : 0,
                                    CyberpunkVR_FlatDistanceM);
                            }
                        }
                        if (CyberpunkVR_MonoCyclopeanPose && eye < 2 && monoHasView[0] && monoHasView[1]) {
                            projectionViews[eye].pose.orientation = monoPoses[0].orientation;
                            projectionViews[eye].pose.position = {
                                (monoPoses[0].position.x + monoPoses[1].position.x) * 0.5f,
                                (monoPoses[0].position.y + monoPoses[1].position.y) * 0.5f,
                                (monoPoses[0].position.z + monoPoses[1].position.z) * 0.5f};
                        }
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

                        // FLAT CONTENT SITS AT OPTICAL INFINITY UNLESS IT IS GIVEN A DISTANCE.
                        //
                        // Whenever both eyes are shown ONE image -- the intro, the menus, and any
                        // frame where the second view has nothing to give -- the two eyes receive
                        // identical content from viewpoints one IPD apart, and identical content
                        // from two viewpoints is content infinitely far away. Fusing that needs
                        // parallel gaze. A tester described it exactly: the startup logo doubles,
                        // and "both images too far to the sides to converge".
                        //
                        // Fixed by sampling a WINDOW of the image that differs per eye. Shrink the
                        // rect by the disparity and slide each eye's window in by half of it: the
                        // right eye then sees the content displaced left and the left eye right,
                        // which is where an object at that distance belongs. The submitted FOV is
                        // recomputed from the sub-rect's own tangent range, so render == submit
                        // still holds -- the cost is the disparity's width of horizontal field,
                        // about 1.6% at the default distance.
                        // Done by ROTATING the submitted frustum, not by cropping the rect. The
                        // first attempt slid the sampled window and then recomputed the FOV from
                        // that window's own tangent range -- which is the identity: every pixel
                        // still lands at the angle it was rendered at, so it introduced no
                        // disparity at all, only a narrower field. Bringing content in from
                        // infinity means turning each eye INWARD by half the vergence angle, and
                        // for a projection layer that is a rigid shift of angleLeft and
                        // angleRight together.
                        //
                        // It is a deliberate lie of ipd/(2*distance) -- 1.1 deg per eye at the
                        // default -- about the angle the frame was rendered at. For flat content
                        // there is no world geometry for it to be wrong about, and on a mono
                        // fallback frame the honest alternative is content at infinity, which is
                        // what cannot be fused in the first place.
                        // BOTH eyes or neither. The first version asked "does THIS eye share the
                        // image", which in gameplay is true of exactly one of them -- the VRCAM eye
                        // has its own picture and the other does not. So it rotated one frustum and
                        // left the other alone, and the tester's probe caught it exactly:
                        //
                        //     left  eye submitted  L -50.822  R +50.822   asym +0.0000
                        //     right eye submitted  L -49.692  R +51.952   asym +2.2600
                        //
                        // 1.130 deg on one side only = ipd/(2*1.8m) applied once. A one-sided yaw
                        // between the eyes is divergence, which is the one thing the eyes cannot
                        // follow -- "left and right view are too far apart", measured. Convergence
                        // is only meaningful when the two eyes really are looking at one image.
                        const bool bothEyesShareOneImage = !vrcamEye;
                        if (bothEyesShareOneImage && CyberpunkVR_FlatDistanceM > 0.05f && eye < 2) {
                            const float ipd = GetRuntimeIpd();
                            if (ipd > 0.03f && ipd < 0.10f) {
                                const float half = 0.5f * (ipd / CyberpunkVR_FlatDistanceM);
                                // The left eye sees a near object to the RIGHT of where an
                                // infinitely distant one sits, so its frustum turns right.
                                const bool isLeft = (eye == (CyberpunkVR_MainIsRightEye ? 1u : 0u));
                                const float d = isLeft ? half : -half;
                                projectionViews[eye].fov.angleLeft  += d;
                                projectionViews[eye].fov.angleRight += d;
                            }
                        }
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
                            //
                            // The planes are the GAME's, measured off the live view context
                            // (ctx+0xB0 near, ctx+0xB4 far): 0.02 m and 16000 m. They used to be
                            // 10000/0.01 -- placeholders of the right shape and the wrong size, so
                            // every depth the compositor unprojected landed at the wrong distance.
                            // A runtime that reprojects on this layer was being told the world ends
                            // at ten kilometres and starts at a centimetre.
                            depthInfos[eye].minDepth = 0.0f;
                            depthInfos[eye].maxDepth = 1.0f;
                            depthInfos[eye].nearZ = 16000.0f;
                            depthInfos[eye].farZ = 0.02f;
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
                        // The one edge we own both ends of: these copies read the images the
                        // capture wrote on the GAME's queue, so wait for exactly that frame's
                        // copies. Nothing here waits on the engine itself -- ordering against the
                        // engine is the capture's problem, and it gets it free by running there.
                        if (m_ownXrQueue && m_captureFence && monoCaptureFence != 0) {
                            m_d3dQueue->Wait(m_captureFence, monoCaptureFence);
                        }
                        m_d3dQueue->ExecuteCommandLists(1, cmdLists);

                        ++m_fenceValue;
                        m_d3dQueue->Signal(m_fence, m_fenceValue);
                    } else {
                        m_cmdList->Close();
                        ID3D12CommandList* cmdLists[] = {m_cmdList};
                        // The one edge we own both ends of: these copies read the images the
                        // capture wrote on the GAME's queue, so wait for exactly that frame's
                        // copies. Nothing here waits on the engine itself -- ordering against the
                        // engine is the capture's problem, and it gets it free by running there.
                        if (m_ownXrQueue && m_captureFence && monoCaptureFence != 0) {
                            m_d3dQueue->Wait(m_captureFence, monoCaptureFence);
                        }
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
                                // JUDDER, AS A NUMBER.
                                //
                                // The gap between the pose the frame was rendered with and the
                                // live head pose IS the render-to-photon latency the compositor
                                // has to reproject away. What matters is not its size but its
                                // STEADINESS: a constant gap reprojects out perfectly, a gap that
                                // swings frame to frame is judder and cannot be filtered. So
                                // report the spread over a window rather than single samples --
                                // spread near zero while turning means the pose stream is clean
                                // and the cause is elsewhere; a wide spread names it here.
                                {
                                    const XrQuaternionf& a = monoPoses[0].orientation;
                                    const XrQuaternionf& b = location.pose.orientation;
                                    float dot = a.x*b.x + a.y*b.y + a.z*b.z + a.w*b.w;
                                    dot = dot < 0.0f ? -dot : dot; if (dot > 1.0f) dot = 1.0f;
                                    const float gapDeg = 2.0f * acosf(dot) * 180.0f / 3.1415926535f;
                                    static float s_gapMin = 1e9f, s_gapMax = -1e9f, s_gapSum = 0.0f;
                                    static uint32_t s_gapN = 0;
                                    if (gapDeg < s_gapMin) s_gapMin = gapDeg;
                                    if (gapDeg > s_gapMax) s_gapMax = gapDeg;
                                    s_gapSum += gapDeg;
                                    // CADENCE, the other half of the answer.
                                    //
                                    // A pose stream can be perfect and the headset still judder,
                                    // because a frame rate that is not a whole divisor of the
                                    // display rate cannot be shown evenly: at 52 fps on 90 Hz each
                                    // frame lives for one refresh or two in an irregular pattern,
                                    // and no reprojection removes that -- only 45 (exactly half)
                                    // or 90 does. So report both periods and the ratio next to the
                                    // pose numbers; if spread is small and the ratio is not near a
                                    // whole number, the remaining judder is cadence, not pose.
                                    const double dispMs =
                                        frameState.predictedDisplayPeriod / 1.0e6;
                                    const double gameMs = FitSlopeNs() / 1.0e6;
                                    if (++s_gapN >= 120) {
                                        // Every 120 frames is once every 1.7 s at 72 Hz. Kept
                                        // at that under DEBUG; otherwise every 30 s, which is
                                        // still often enough to catch readback exact=0.
                                        LOG_THROTTLED(30000, "POSEDIAG: render->live gap over %u frames: avg %.2f deg, "
                                            "min %.2f, max %.2f, spread %.2f | display %.2f ms "
                                            "(%.1f Hz), game %.2f ms (%.1f fps), ratio %.2f | "
                                            "poseFromWrite=%llu fromSlot=%llu\n",
                                            s_gapN, s_gapSum / static_cast<float>(s_gapN),
                                            s_gapMin, s_gapMax, s_gapMax - s_gapMin,
                                            dispMs, dispMs > 0.0 ? 1000.0 / dispMs : 0.0,
                                            gameMs, gameMs > 0.0 ? 1000.0 / gameMs : 0.0,
                                            dispMs > 0.0 ? gameMs / dispMs : 0.0,
                                            (unsigned long long)CyberpunkVR_DebugPoseFromWrite,
                                            (unsigned long long)CyberpunkVR_DebugPoseFromSlot);
                                        // The engine's render-ahead depth, MEASURED. `age` is how
                                        // many camera writes back the quaternion the render side
                                        // was about to draw with turned out to be; `qdepth` is how
                                        // many read-back frames are waiting to be presented. Both
                                        // steady = the pairing is exact. `nomatch` climbing means
                                        // the render side is drawing with a camera we did not
                                        // write, which would be a different problem entirely.
                                        LOG_THROTTLED(30000, "POSEDIAG: readback exact=%llu approx=%llu "
                                            "exactTies=%llu nomatch=%llu age=%u qdepth=%u "
                                            "used=%llu\n",
                                            (unsigned long long)CyberpunkVR_DebugFinalExact,
                                            (unsigned long long)CyberpunkVR_DebugFinalApprox,
                                            (unsigned long long)CyberpunkVR_DebugFinalExactTies,
                                            (unsigned long long)CyberpunkVR_DebugFinalNoMatch,
                                            CyberpunkVR_DebugFinalAge,
                                            OpenXRManager::Get().RenderedFrameQueueDepth(),
                                            (unsigned long long)CyberpunkVR_DebugPoseReadBack);
                                        s_gapN = 0; s_gapSum = 0.0f;
                                        s_gapMin = 1e9f; s_gapMax = -1e9f;
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
                                ++CyberpunkVR_DebugMonoSubmits;
                                // Feed the regression, but ONLY on a frame the game actually
                                // produced. A resubmitted stale snapshot carries its old serial
                                // against this cycle's display time, and those points do not lie
                                // on the line -- they are the same x with a later y, which drags
                                // the slope toward zero. RealVR samples per produced frame for
                                // the same reason.
                                if (presentSerial != m_lastSubmittedSerial) {
                                    FitAddDisplayTime(presentSerial, frameState.predictedDisplayTime);
                                }
                                m_lastSubmittedSerial = presentSerial;
                                monoSource->Release();
                                if (vrcamEye) vrcamEye->Release();
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
                if (vrcamEye) {
                    vrcamEye->Release();
                }
                if (monoDepthSource) {
                    monoDepthSource->Release();
                }
            }
        } else if (monoEnabled && ((++monoWaitLogCounter % 300) == 1)) {
            Log("OpenXRManager: %s submit waiting. ready=%d views=%zu shouldRender=%d\n",
                "Mono",
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
