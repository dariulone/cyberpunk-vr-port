// openxr_calibration.cpp - recenter, auto-calibration, and calibration-file I/O.
// Split verbatim from openxr_manager.cpp (OpenXRManager methods). Shared module
// state/helpers via openxr_internal.h (inline).
#include "Runtimes/WristBase.hpp"
#include "Core/Paths.hpp"
#include "Runtimes/BodyProportions.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include "Runtimes/OpenXRInternal.hpp"
#include "Utils/XrMath.hpp"
#include "Utils/SharedSlots.hpp"
#include "Camera/NeckCameraMount.hpp"
#include "Overlay/LiveControlsUi.hpp"
#include <cstdarg>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <cmath>
#include <vector>
#include <utility>
#include <chrono>
#include <algorithm>

void OpenXRManager::RotateBaseYaw(float radians) {
    // Rotate the recenter base about vertical (XR +Y) so the HMD's base-relative yaw
    // shifts by -radians. Called by the dxgi body-realign logic together with an equal
    // heading injection; the pair leaves the rendered view and the HMD-local hand
    // poses stationary while the body turns underneath.
    if (radians == 0.0f) return;
    std::lock_guard<std::mutex> lock(m_renderPoseMutex);
    if (!m_basePoseSet) return;
    const float h = radians * 0.5f;
    const XrQuaternionf ry{0.0f, sinf(h), 0.0f, cosf(h)};
    XrQuaternionf q = MultiplyQuat(m_basePose.orientation, ry);
    const float n = sqrtf(q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w);
    if (n > 1e-8f) {
        const float inv = 1.0f / n;
        q.x *= inv; q.y *= inv; q.z *= inv; q.w *= inv;
    }
    m_basePose.orientation = q;
}

void OpenXRManager::RequestRecenter() {
    m_recenterRequested.store(true, std::memory_order_relaxed);
}

void OpenXRManager::BeginExternalPoseReset() {
    std::lock_guard lock(m_externalResetMutex);
    m_externalPoseReset.Begin();
}

void OpenXRManager::EndExternalPoseReset() {
    std::lock_guard lock(m_externalResetMutex);
    m_externalPoseReset.End();
    Log("OpenXRManager: runtime Reset View completed; rebase before publishing another pose.\n");
}

// ==== AUTO-CALIBRATION ====
//
// Procedure (matches standard VR title flow):
//   1. The player clicks Start; HUD shows "Stretch arms out to the SIDES and stand straight. 3..2..1".
//   2. While sampling we read live HMD + both controller HMD-local gizmo positions, keep max armSpan.
//   3. After secs seconds we derive anatomical numbers and publish them via SetVRHandCalib +
//      SetShoulderAnatomical, then save to file.
//
// Body proportions used:
//   * armSpan ~= height (Da Vinci) -> we use armSpan as the calibration baseline.
//   * shoulder half-width  ~= 0.135 * armSpan  (from human anthropometry tables)
//   * HMD -> shoulder backward depth ~= 0.04 * armSpan (eyes are ahead of neck)
//   * arm length is measured directly from calibrated shoulder to controller/gizmo wrist
void OpenXRManager::StartAutoCalibration(float secs) {
    m_calibSeconds.store(secs, std::memory_order_relaxed);
    m_calibProgress.store(0.0f, std::memory_order_relaxed);
    m_calibArmSpanMax = 0.0f;
    m_calibHmdHeightSum = 0.0f;
    m_calibSampleCount = 0;
    m_calibCtrlPosSumR[0]=m_calibCtrlPosSumR[1]=m_calibCtrlPosSumR[2]=0.0f;
    m_calibCtrlPosSumL[0]=m_calibCtrlPosSumL[1]=m_calibCtrlPosSumL[2]=0.0f;
    m_calibEyeFloorSum = 0.0f;
    m_calibEyeFloorCount = 0;
    m_calibStart = 0.0;
    EnsureCalibDefaults();
    m_calibState.store(1, std::memory_order_relaxed);
    Log("Auto-calibration: started (%.1fs T-pose sample). Stretch arms STRAIGHT OUT to the sides.\n", secs);
}

// INITIALIZE wrist defaults if they've never been set. Without this, finalisation reads zero
// values from m_calib[] (atomic float default = 0), publishes identity wrist quats, and the
// hand orientation breaks (palm faces wrong way). These match the plugin's baked-in defaults
// (g_VRWristR_* and g_VRWristL_*) so the player sees the same wrist behaviour as before auto-cal.
void OpenXRManager::EnsureCalibDefaults() {
    if (m_calib[0].load(std::memory_order_relaxed) == 0.0f
        && m_calib[1].load(std::memory_order_relaxed) == 0.0f
        && m_calib[9].load(std::memory_order_relaxed) == 0.0f) {
        m_calib[0].store(1.05f, std::memory_order_relaxed);   // scaleR
        m_calib[1].store(1.06f, std::memory_order_relaxed);   // scaleL
        m_calib[2].store(0.0f,  std::memory_order_relaxed);   // heightR
        m_calib[3].store(0.0f,  std::memory_order_relaxed);   // heightL
        m_calib[4].store(1.0f,  std::memory_order_relaxed);   // swingR
        m_calib[5].store(1.0f,  std::memory_order_relaxed);   // swingL
        m_calib[6].store(0.0f,  std::memory_order_relaxed);   // poleR
        m_calib[7].store(0.0f,  std::memory_order_relaxed);   // poleL
        // The wrists start on the baked base (Runtimes/WristBase.hpp): wRp wRy wRr wLp wLy wLr.
        for (int i = 0; i < 6; ++i) m_calib[8 + i].store(cvr::calib::kWristBase[i], std::memory_order_relaxed);
        Log("Auto-calibration: initialised wrist/elbow defaults (the baked wrist base).\n");
    }
}

// The eyes over the floor now: the view located in the runtime's STAGE space, whose floor is y = 0. The T-pose's
// own LOCAL space has its origin wherever the session started, so its heights are not over anything.
bool OpenXRManager::LocateEyesOverFloor(float* height) {
    if (!height || m_stageUnavailable || m_session == XR_NULL_HANDLE || m_viewSpace == XR_NULL_HANDLE) return false;
    if (m_stageSpace == XR_NULL_HANDLE) {
        XrReferenceSpaceCreateInfo info{XR_TYPE_REFERENCE_SPACE_CREATE_INFO};
        info.referenceSpaceType = XR_REFERENCE_SPACE_TYPE_STAGE;
        info.poseInReferenceSpace.orientation.w = 1.0f;
        if (XR_FAILED(xrCreateReferenceSpace(m_session, &info, &m_stageSpace))) {
            m_stageSpace = XR_NULL_HANDLE;
            m_stageUnavailable = true;
            Log("Auto-calibration: the runtime has no STAGE space -- the eyes' height over the floor is not measured.\n");
            return false;
        }
    }
    const XrTime time = GetFrameAimTimeNow();
    if (time <= 0) return false;
    XrSpaceLocation location{XR_TYPE_SPACE_LOCATION};
    if (XR_FAILED(xrLocateSpace(m_viewSpace, m_stageSpace, time, &location)) ||
        !(location.locationFlags & XR_SPACE_LOCATION_POSITION_VALID_BIT)) return false;
    *height = location.pose.position.y;
    return std::isfinite(*height) && *height > 0.5f && *height < 2.5f;
}

void OpenXRManager::TickAutoCalibration() {
    if (m_calibState.load(std::memory_order_relaxed) != 1) return;

    // Sim time in seconds (use QueryPerformanceCounter for steady clock; the frame loop runs
    // continuously while VR is active so this is monotonic).
    LARGE_INTEGER now, freq;
    QueryPerformanceCounter(&now);
    QueryPerformanceFrequency(&freq);
    double t = static_cast<double>(now.QuadPart) / static_cast<double>(freq.QuadPart);
    if (m_calibStart == 0.0) m_calibStart = t;
    float elapsed = static_cast<float>(t - m_calibStart);
    float total = m_calibSeconds.load(std::memory_order_relaxed);
    float prog = (total > 0.0f) ? (elapsed / total) : 1.0f;
    if (prog > 1.0f) prog = 1.0f;
    m_calibProgress.store(prog, std::memory_order_relaxed);

    // Sample current frame (under hand-mutex for atomicity with the frame loop writer).
    {
        std::lock_guard<std::mutex> lock(m_handMutex);
        if (m_hands[0].valid && m_hands[1].valid) {
            float dx = m_hands[1].posX - m_hands[0].posX;
            float dy = m_hands[1].posY - m_hands[0].posY;
            float dz = m_hands[1].posZ - m_hands[0].posZ;
            float span = std::sqrt(dx*dx + dy*dy + dz*dz);
            if (span > m_calibArmSpanMax) m_calibArmSpanMax = span;
            // Accumulate HMD-local controller positions so we know where each hand actually sits
            // (so X-sign tells us which is left/right -> calibration auto-detects swapped sticks).
            m_calibCtrlPosSumR[0] += m_hands[1].posX;
            m_calibCtrlPosSumR[1] += m_hands[1].posY;
            m_calibCtrlPosSumR[2] += m_hands[1].posZ;
            m_calibCtrlPosSumL[0] += m_hands[0].posX;
            m_calibCtrlPosSumL[1] += m_hands[0].posY;
            m_calibCtrlPosSumL[2] += m_hands[0].posZ;
            m_calibHmdHeightSum += m_posY.load(std::memory_order_relaxed);
            m_calibSampleCount++;
        }
    }
    {
        float eyes = 0.0f;
        if (LocateEyesOverFloor(&eyes)) { m_calibEyeFloorSum += eyes; ++m_calibEyeFloorCount; }
    }

    if (elapsed >= total) {
        // Finalise.
        float armSpan = m_calibArmSpanMax;
        if (armSpan < 0.5f || armSpan > 2.5f || m_calibSampleCount == 0) {
            Log("Auto-calibration: armSpan %.3fm out of plausible range — aborting.\n", armSpan);
            m_calibState.store(3, std::memory_order_relaxed);
            return;
        }
        float invN = 1.0f / static_cast<float>(m_calibSampleCount);
        float avgR[3] = { m_calibCtrlPosSumR[0]*invN, m_calibCtrlPosSumR[1]*invN, m_calibCtrlPosSumR[2]*invN };
        float avgL[3] = { m_calibCtrlPosSumL[0]*invN, m_calibCtrlPosSumL[1]*invN, m_calibCtrlPosSumL[2]*invN };

        // SHOULDER ANATOMICAL OFFSETS. These are body-frame OpenXR axes (X right, Y up,
        // Z back), sampled from the same gizmo/controller coordinates used for hand drawing.
        const float kShoulderHalfWidth = cvr::body::kShoulderHalfWidth;     // X | wrist-span to shoulder half-width; keep torso narrow
        const float kShoulderBackFromHmd = cvr::body::kShoulderBehindEyes;  // Z | eyes sit slightly in front of shoulders
        float shoulderHalf = kShoulderHalfWidth * armSpan;
        if (shoulderHalf < 0.10f) shoulderHalf = 0.10f;
        if (shoulderHalf > 0.20f) shoulderHalf = 0.20f;
        float rightSign = (avgR[0] >= avgL[0]) ? 1.0f : -1.0f;
        float rx = shoulderHalf * rightSign;
        float lx = -shoulderHalf * rightSign;
        // Y: both shoulders should share one height. Use the higher T-pose hand as the shoulder
        // level so a tired/lowered arm does not pull that shoulder down and shorten reach.
        float shoulderY = (avgR[1] > avgL[1]) ? avgR[1] : avgL[1];
        if (shoulderY > -0.12f) shoulderY = -0.12f;
        if (shoulderY < -0.30f) shoulderY = -0.30f;
        float ry = shoulderY;
        float lyv = shoulderY;
        float rz  = kShoulderBackFromHmd;
        float lzv = kShoulderBackFromHmd;
        SetShoulderAnatomical(rx, ry, rz, lx, lyv, lzv);

        // Arm-scale: realArmLen / modelArmLen. Real arm length is measured from the calibrated
        // shoulder pivot to the visible controller/gizmo wrist in the T-pose.
        auto len3 = [](float ax, float ay, float az, float bx, float by, float bz) -> float {
            float dx = ax - bx, dy = ay - by, dz = az - bz;
            return std::sqrt(dx*dx + dy*dy + dz*dz);
        };
        // Arm length from the ARM SPAN (max controller-to-controller distance over the T-pose),
        // not the per-frame AVERAGE controller position. The average is blurred by non-T-pose
        // frames and systematically UNDER-reads (diag showed 0.37 m for a ~0.55 m arm); the span
        // max captures the best fully-extended frame. armLen = (span - shoulderWidth) / 2.
        float spanArm = (armSpan - 2.0f * shoulderHalf) * 0.5f;
        if (spanArm < 0.40f) spanArm = 0.40f; if (spanArm > 0.85f) spanArm = 0.85f;
        // SYMMETRIC: real arms are the same length. The old per-hand normalisation from the blurred
        // averages produced a fake asymmetry (e.g. 0.62 vs 0.51), which made one hand under-reach
        // (the short side ended up "in the belt textures"). Use the span length for both.
        (void)len3;
        float realArmLenR = spanArm;
        float realArmLenL = spanArm;
        float realArmLen = spanArm;
        // The controller/gizmo coordinates are already in the same meter-like space consumed by
        // the solver. Keep the global scale near the proven defaults and only use T-pose length
        // asymmetry for small per-hand correction; dividing by an approximate model arm length
        // over-shrank targets and caused relaxed arms to keep an elbow bend.
        // Position-scale is GONE. With the gizmo-exact 1:1 hand target the hand sits on the
        // real controller position; avatar proportions are matched by scaling the arm BONES to
        // the measured arm length (plugin VRIK_ArmScale), not by stretching the target. Publish
        // the measured anatomy (arm length per hand + eye height) into [77..80] instead.
        // The eyes over the FLOOR (STAGE space); the LOCAL heights summed above are over the session's start.
        float eyeHeight = m_calibEyeFloorCount > 0 ? m_calibEyeFloorSum / static_cast<float>(m_calibEyeFloorCount) : 0.0f;
        m_measuredEyeHeight.store(eyeHeight, std::memory_order_relaxed);
        m_userArmLenR.store(realArmLenR, std::memory_order_relaxed);
        m_userArmLenL.store(realArmLenL, std::memory_order_relaxed);
        m_userEyeHeight.store(eyeHeight, std::memory_order_relaxed);
        m_measureValid.store(1, std::memory_order_relaxed);
        // Legacy modes (1..3) + the head-relative fallback still read a reach scale; keep it
        // neutral (1.0) so they are unaffected by the new measured-length path.
        float scaleR = 1.0f;
        float scaleL = 1.0f;
        (void)realArmLen;

        // PRESERVE all tunable values: swing, pole, wrist orientation. Auto-cal only
        // overwrites the anatomy (scale + shoulder offsets) — elbow/wrist tweaks stay.
        float swingR = m_calib[4].load(std::memory_order_relaxed);
        float swingL = m_calib[5].load(std::memory_order_relaxed);
        float poleR  = m_calib[6].load(std::memory_order_relaxed);
        float poleL  = m_calib[7].load(std::memory_order_relaxed);
        float wRp = m_calib[8].load(std::memory_order_relaxed);
        float wRy = m_calib[9].load(std::memory_order_relaxed);
        float wRr = m_calib[10].load(std::memory_order_relaxed);
        float wLp = m_calib[11].load(std::memory_order_relaxed);
        float wLy = m_calib[12].load(std::memory_order_relaxed);
        float wLr = m_calib[13].load(std::memory_order_relaxed);
        // Re-apply current swing/pole/wrist with new scale.
        if (swingR == 0.0f && swingL == 0.0f) { swingR = 1.0f; swingL = 1.0f; }
        SetVRHandCalib(scaleR, scaleL, 0.0f, 0.0f,
                       swingR, swingL, poleR, poleL,
                       wRp, wRy, wRr, wLp, wLy, wLr);

        SaveCalibrationToFile();
        Log("Auto-calibration DONE.\n");
        Log("  armSpan = %.3fm  armLenR/L = %.3f/%.3fm  eyeHeight = %.3fm\n", armSpan, realArmLenR, realArmLenL, eyeHeight);
        Log("  ctrl R world-local avg = (%.3f, %.3f, %.3f)\n", avgR[0], avgR[1], avgR[2]);
        Log("  ctrl L world-local avg = (%.3f, %.3f, %.3f)\n", avgL[0], avgL[1], avgL[2]);
        Log("  shoulder R = (%.3f, %.3f, %.3f)\n", rx, ry, rz);
        Log("  shoulder L = (%.3f, %.3f, %.3f)\n", lx, lyv, lzv);

        // Auto-apply the camera->head bake (no separate button needed): the player stood straight
        // in the T-pose, so the published head-vs-camera offset is exactly what we want to bake.
        BakeCameraOffset();

        // Auto-recenter at the end of calibration so the player's body forward direction matches
        // the OpenXR forward they just used during the T-pose. Without this the just-measured
        // shoulder anatomy is in the calibration frame but the runtime's tracking frame
        // may have drifted slightly.
        RequestRecenter();

        // A height already given decides the arms and the shoulders (ApplyUserHeight); the T-pose keeps which
        // way is right, the eyes' height and the camera.
        if (const float height = m_userHeight.load(std::memory_order_relaxed); height > 0.0f) ApplyUserHeight(height);

        m_calibState.store(2, std::memory_order_relaxed);
        m_calibProgress.store(1.0f, std::memory_order_relaxed);
    }
}

void OpenXRManager::ApplyUserHeight(float meters) {
    if (!std::isfinite(meters)) return;
    const float height = std::clamp(meters, cvr::body::kMinHeight, cvr::body::kMaxHeight);
    const cvr::body::Proportions body = cvr::body::FromHeight(height);
    EnsureCalibDefaults();
    // Which way is right: what the last T-pose found (it tells swapped controllers), else the runtime's +X.
    const float right = m_calibExt[0].load(std::memory_order_relaxed) < 0.0f ? -1.0f : 1.0f;
    const float half = std::clamp(body.shoulderHalfWidth, 0.10f, 0.20f);
    const float drop = std::clamp(body.shoulderBelowEyes, 0.12f, 0.30f);
    const float back = cvr::body::kShoulderBehindEyes;
    SetShoulderAnatomical(half * right, -drop, back, -half * right, -drop, back);
    const float arm = std::clamp(body.armLength, 0.45f, 0.85f);
    m_userArmLenR.store(arm, std::memory_order_relaxed);
    m_userArmLenL.store(arm, std::memory_order_relaxed);
    if (m_userEyeHeight.load(std::memory_order_relaxed) <= 0.0f)
        m_userEyeHeight.store(body.eyeHeight, std::memory_order_relaxed);
    m_measureValid.store(1, std::memory_order_relaxed);
    m_userHeight.store(height, std::memory_order_relaxed);
    m_firstSetupDone.store(1, std::memory_order_relaxed);
    SaveCalibrationToFile();
    Log("Body from the height %.2f m: arms %.3f m (shoulder to wrist), shoulders %.3f m apart and %.3f m under the eyes\n",
        height, arm, 2.0f * half, drop);
}

// In the plugin's config folder (Core/Paths.hpp).
static void GetCalibFilePath(char* out, size_t outSize) {
    _snprintf_s(out, outSize, _TRUNCATE, "%s", cvr::paths::FileA(cvr::paths::Folder::Config, "vrik_calibration.ini").c_str());
}

void OpenXRManager::GetCameraOffset(float* out) const {
    cvr::camera::AnchorVector mount{};
    cvr::camera::ReadNeckCameraMount(&mount);
    out[0]=mount.x+m_camBakeOffset[0].load(std::memory_order_relaxed);
    out[1]=mount.y+m_camBakeOffset[1].load(std::memory_order_relaxed);
    out[2]=mount.z+m_camBakeOffset[2].load(std::memory_order_relaxed);
}

void OpenXRManager::BakeCameraOffset() {
    // Reset to the rig's neutral neck mount, never sample an animated stance.
    ClearCameraOffset();
    LiveControlsUiState controls{};GetLiveControlsUiState(&controls);
    controls.xrHeadOffsetX=controls.xrHeadOffsetY=controls.xrHeadOffsetZ=0;
    SetLiveControlsUiState(&controls,1);
    SaveCalibrationToFile();
    Log("Camera mount reset: neutral neck + 15cm forward, reference eyes + 10cm up.\n");
}

bool OpenXRManager::SaveCalibrationToFile() {
    char path[MAX_PATH];
    GetCalibFilePath(path, MAX_PATH);
    FILE* f = nullptr;
    if (fopen_s(&f, path, "w") != 0 || !f) {
        Log("SaveCalibration: failed to open %s\n", path);
        return false;
    }
    fprintf(f, "# CyberpunkVRPort VRIK auto-calibration\n");
    // Version 4: the wrist angles are OFFSETS from the baked base (Runtimes/WristBase.hpp).
    fprintf(f, "version=4\n");
    fprintf(f, "scaleR=%.4f\nscaleL=%.4f\n",
            m_calib[0].load(std::memory_order_relaxed),
            m_calib[1].load(std::memory_order_relaxed));
    fprintf(f, "heightR=%.4f\nheightL=%.4f\n",
            m_calib[2].load(std::memory_order_relaxed),
            m_calib[3].load(std::memory_order_relaxed));
    fprintf(f, "swingR=%.4f\nswingL=%.4f\n",
            m_calib[4].load(std::memory_order_relaxed),
            m_calib[5].load(std::memory_order_relaxed));
    fprintf(f, "poleR=%.4f\npoleL=%.4f\n",
            m_calib[6].load(std::memory_order_relaxed),
            m_calib[7].load(std::memory_order_relaxed));
    {
        float w[6];
        for (int i = 0; i < 6; ++i) w[i] = m_calib[8 + i].load(std::memory_order_relaxed) - cvr::calib::kWristBase[i];
        fprintf(f, "wRp=%.4f\nwRy=%.4f\nwRr=%.4f\n", w[0], w[1], w[2]);
        fprintf(f, "wLp=%.4f\nwLy=%.4f\nwLr=%.4f\n", w[3], w[4], w[5]);
    }
    fprintf(f, "shoulderRX=%.4f\nshoulderRY=%.4f\nshoulderRZ=%.4f\n",
            m_calibExt[0].load(std::memory_order_relaxed),
            m_calibExt[1].load(std::memory_order_relaxed),
            m_calibExt[2].load(std::memory_order_relaxed));
    fprintf(f, "shoulderLX=%.4f\nshoulderLY=%.4f\nshoulderLZ=%.4f\n",
            m_calibExt[3].load(std::memory_order_relaxed),
            m_calibExt[4].load(std::memory_order_relaxed),
            m_calibExt[5].load(std::memory_order_relaxed));
    fprintf(f, "cameraMountVersion=1\n");
    fprintf(f, "camBakeX=%.4f\ncamBakeY=%.4f\ncamBakeZ=%.4f\n",
            m_camBakeOffset[0].load(std::memory_order_relaxed),
            m_camBakeOffset[1].load(std::memory_order_relaxed),
            m_camBakeOffset[2].load(std::memory_order_relaxed));
    // Arm length / eye height were NEVER persisted -> every fresh session ran with
    // userArmLen=0 (no bone scaling: side reaches bent, hands short) until the player
    // recalibrated manually. Persist them like the rest of the calibration.
    fprintf(f, "userArmLenR=%.4f\nuserArmLenL=%.4f\nuserEyeHeight=%.4f\n",
            m_userArmLenR.load(std::memory_order_relaxed),
            m_userArmLenL.load(std::memory_order_relaxed),
            m_userEyeHeight.load(std::memory_order_relaxed));
    // The player's height (0 = not given) and whether the first-launch setup has been through.
    fprintf(f, "userHeight=%.4f\nfirstSetup=%d\n", m_userHeight.load(std::memory_order_relaxed),
            m_firstSetupDone.load(std::memory_order_relaxed));
    fclose(f);
    Log("Calibration saved -> %s\n", path);
    return true;
}

bool OpenXRManager::LoadCalibrationFromFile() {
    char path[MAX_PATH];
    GetCalibFilePath(path, MAX_PATH);
    FILE* f = nullptr;
    if (fopen_s(&f, path, "r") != 0 || !f) return false;
    float v[14] = {
        m_calib[0].load(std::memory_order_relaxed),
        m_calib[1].load(std::memory_order_relaxed),
        m_calib[2].load(std::memory_order_relaxed),
        m_calib[3].load(std::memory_order_relaxed),
        m_calib[4].load(std::memory_order_relaxed),
        m_calib[5].load(std::memory_order_relaxed),
        m_calib[6].load(std::memory_order_relaxed),
        m_calib[7].load(std::memory_order_relaxed),
        m_calib[8].load(std::memory_order_relaxed),
        m_calib[9].load(std::memory_order_relaxed),
        m_calib[10].load(std::memory_order_relaxed),
        m_calib[11].load(std::memory_order_relaxed),
        m_calib[12].load(std::memory_order_relaxed),
        m_calib[13].load(std::memory_order_relaxed),
    };
    float e[6] = {
        m_calibExt[0].load(std::memory_order_relaxed),
        m_calibExt[1].load(std::memory_order_relaxed),
        m_calibExt[2].load(std::memory_order_relaxed),
        m_calibExt[3].load(std::memory_order_relaxed),
        m_calibExt[4].load(std::memory_order_relaxed),
        m_calibExt[5].load(std::memory_order_relaxed),
    };
    float cb[3] = {
        m_camBakeOffset[0].load(std::memory_order_relaxed),
        m_camBakeOffset[1].load(std::memory_order_relaxed),
        m_camBakeOffset[2].load(std::memory_order_relaxed),
    };
    int version = 0, cameraMountVersion = 0;
    float wrist[6] = {};
    bool wristSeen[6] = {};
    char line[128];
    while (fgets(line, sizeof(line), f)) {
        char key[32]; float val;
        if (sscanf_s(line, "%31[^=]=%f", key, (unsigned)_countof(key), &val) != 2) continue;
        if (strcmp(key, "version") == 0) version = static_cast<int>(val);
        if (strcmp(key, "cameraMountVersion") == 0) cameraMountVersion=static_cast<int>(val);
        #define M(name, idx) if (strcmp(key, name) == 0) v[idx] = val;
        #define E(name, idx) if (strcmp(key, name) == 0) e[idx] = val;
        #define C(name, idx) if (strcmp(key, name) == 0) cb[idx] = val;
        M("scaleR",0) M("scaleL",1) M("heightR",2) M("heightL",3)
        M("swingR",4) M("swingL",5) M("poleR",6) M("poleL",7)
        #define W(name, idx) if (strcmp(key, name) == 0) { wrist[idx] = val; wristSeen[idx] = true; }
        W("wRp",0) W("wRy",1) W("wRr",2) W("wLp",3) W("wLy",4) W("wLr",5)
        #undef W
        E("shoulderRX",0) E("shoulderRY",1) E("shoulderRZ",2)
        E("shoulderLX",3) E("shoulderLY",4) E("shoulderLZ",5)
        C("camBakeX",0) C("camBakeY",1) C("camBakeZ",2)
        if (strcmp(key, "userArmLenR") == 0 && val >= 0.45f && val <= 0.95f)
            m_userArmLenR.store(val, std::memory_order_relaxed);
        if (strcmp(key, "userArmLenL") == 0 && val >= 0.45f && val <= 0.95f)
            m_userArmLenL.store(val, std::memory_order_relaxed);
        if (strcmp(key, "userEyeHeight") == 0 && val > 0.0f && val <= 2.2f)
            m_userEyeHeight.store(val, std::memory_order_relaxed);
        if (strcmp(key, "userHeight") == 0 && val >= cvr::body::kMinHeight && val <= cvr::body::kMaxHeight)
            m_userHeight.store(val, std::memory_order_relaxed);
        if (strcmp(key, "firstSetup") == 0)
            m_firstSetupDone.store(val != 0.0f ? 1 : 0, std::memory_order_relaxed);
        #undef M
        #undef E
        #undef C
    }
    fclose(f);
    // Old values are feet-based absolute bakes, not trims of the neck mount.
    if(cameraMountVersion<1)cb[0]=cb[1]=cb[2]=0;
    if (version < 2) {
        v[2] = 0.0f;
        v[3] = 0.0f;
    }
    // Version 4 stores the wrists as offsets from the baked base; older files hold the absolute angles.
    for (int i = 0; i < 6; ++i)
        if (wristSeen[i]) v[8 + i] = version >= 4 ? cvr::calib::kWristBase[i] + wrist[i] : wrist[i];
    if (version < 3) {
        if (v[0] < 0.90f || v[0] > 1.25f) v[0] = 1.05f;
        if (v[1] < 0.90f || v[1] > 1.25f) v[1] = 1.06f;
        float shoulderY = (e[1] > e[4]) ? e[1] : e[4];
        if (shoulderY > -0.12f) shoulderY = -0.12f;
        if (shoulderY < -0.30f) shoulderY = -0.30f;
        e[1] = shoulderY;
        e[4] = shoulderY;
    }
    SetVRHandCalib(v[0],v[1],v[2],v[3],v[4],v[5],v[6],v[7],v[8],v[9],v[10],v[11],v[12],v[13]);
    SetShoulderAnatomical(e[0],e[1],e[2],e[3],e[4],e[5]);
    SetCameraOffset(cb[0], cb[1], cb[2]);
    Log("Calibration loaded <- %s\n", path);
    return true;
}
