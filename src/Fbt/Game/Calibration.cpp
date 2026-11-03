// FULL-BODY TRACKING CALIBRATION in the game: the shared calibration (src/Fbt/FbtCalibration, the same as the FBT IK
// lab's) run on the devices Trackers.cpp locates, in three instants (see Calibration.hpp).
#include "Core/Paths.hpp"
#include "Fbt/Game/Calibration.hpp"
#include "Fbt/Game/Trackers.hpp"
#include "Anim/CharacterRig.hpp"
#include "Core/VrCoreShared.hpp"
#include "Hooks/RoomscaleMove.hpp"
#include "Lobby/MenuLobby.hpp"
#include <Windows.h>
#include <atomic>
#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <mutex>
#include <string>

extern void Log(const char*, ...);

using namespace wrpuppets::fbt;   // the IK library (externals/wrpuppets-fbt-ik)

namespace cvr::fbt::game {
using cvr::trackers::Pose;
using cvr::trackers::kRoleCount;
using cvr::trackers::kWaist;
using cvr::trackers::kLeftFoot;
using cvr::trackers::kRightFoot;
namespace {
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

// Each step: a pull of both triggers after they were released together (so the pull that pressed the menu button does
// not commit at once).
std::atomic<int> g_step{0};   // 0 off, 1 head (look ahead), 2 body (look down), 3 arms (the T-pose on the cross)
// The last pull not taken: which message (kRefuse*), until when -- shown in the view under the step's text.
std::atomic<int> g_refused{0};
std::atomic<ULONGLONG> g_refusedUntil{0};
void Refuse(int message) {
    g_refused.store(message, std::memory_order_relaxed);
    g_refusedUntil.store(GetTickCount64() + 3000, std::memory_order_relaxed);
}
void Accept() { g_refusedUntil.store(0, std::memory_order_relaxed); }
// A calibration belongs to the scene it started in: while it runs the game gets no triggers, grips or face buttons
// (Natives/VrMenu.cpp CapturesButtons) and the VR menu stays shut, so one left running took the buttons away through
// a save load and the main menu. Cancelled when the player changes (a load), when a game menu stays open for 2 s, and
// after 2 minutes without a step taken.
uintptr_t g_calibrationPlayer = 0;
ULONGLONG g_lastProgress = 0, g_menuSince = 0;
void Progress() { g_lastProgress = GetTickCount64(); }
std::atomic<float> g_trigger[2]{0.0f, 0.0f};
bool g_armed = false;
constexpr float kTriggerOn = 0.8f, kTriggerOff = 0.2f;
Calibration g_calibration{};
std::mutex g_calibrationMutex;
wrpuppets::fbt::Skeleton g_skeleton;
wrpuppets::fbt::Calibrator g_calibrator;
bool g_calibratorReady = false;
char g_skeletonPath[MAX_PATH]{};

float ArmElevationDeg(const float upper[3], const float fore[3]) {
    const float dx = fore[0] - upper[0], dy = fore[1] - upper[1], dz = fore[2] - upper[2];
    return std::atan2(dz, std::sqrt(dx * dx + dy * dy)) * 57.29578f;
}
void LogReference(const VrikFbtReference& v) {
    Log("[fbt-calib] V reference (%s rig, model space x right y fwd z up): eye=(%+.3f %+.3f %+.3f) hips=(%+.3f %+.3f %+.3f) "
        "feet=(%+.3f %+.3f %+.3f)/(%+.3f %+.3f %+.3f); arms below horizontal %.1f / %.1f deg (0 = T pose)\n", v.female ? "female" : "male",
        v.eye[0], v.eye[1], v.eye[2], v.hipsPos[0], v.hipsPos[1], v.hipsPos[2], v.footPos[0][0], v.footPos[0][1], v.footPos[0][2],
        v.footPos[1][0], v.footPos[1][1], v.footPos[1][2], -ArmElevationDeg(v.upperArmPos[0], v.foreArmPos[0]),
        -ArmElevationDeg(v.upperArmPos[1], v.foreArmPos[1]));
}

// Step 1's head, looking ahead at the horizon line: the capture's eye height, head reference and origin come from it,
// not from the head at step 2's pull -- that step looks down at the trackers, and a head pitched 60-70 deg drops the
// eyes some 20 cm and moves them forward (in-game capture: eye height 1.485 m against 1.69-1.71 in the lab).
XrPosef g_levelHead{{0, 0, 0, 1}, {0, 0, 0}};
constexpr float kLevelPullDeg = kCalibrationLevelDeg;   // step 1 counts only with the gaze this close to level
float HeadPitchDeg(const XrQuaternionf& q) {
    const XrVector3f f = Rotate(q, {0, 0, -1});   // the view's forward
    return std::asin(f.y > 1.0f ? 1.0f : (f.y < -1.0f ? -1.0f : f.y)) * 57.29578f;
}

// STEP 3, THE CROSS. Set when step 2 is taken: centred on the midpoint between the feet trackers and square to the
// line between them (standing straight is the head over that point, the arms along that line), V's proportions at the
// player's size: kCrossAhead in front, its top at the eyes, its arms at the shoulders, as long as the arms out. Without
// both feet, from step 1's head and gaze.
constexpr float kCrossAhead = 1.8f;
// The marks, metres: the head on the top (sideways, height), a hand on its arm (height, out along it as a share of the
// arm's half length, forward of the body); "near" (yellow) is twice the tolerance, half the reach.
constexpr float kHeadSide = 0.10f, kHeadHeight = 0.07f, kHandHeight = 0.08f, kHandOut = 0.75f, kHandDepth = 0.30f;
struct Cross {
    XrVector3f spot{}, fwd{}, right{}, base{};
    float tipY = 0, barY = 0, halfSpan = 0;
};
Cross g_cross;
std::mutex g_crossMutex;
void Save(const Calibration& c);
wrpuppets::fbt::RoomPose ToRoomImpl(bool valid, const XrPosef& p);
CrossView g_crossView;          // the last frame's, for the helpers
ULONGLONG g_holdStart = 0;      // when all three marks were reached (0 = not held)
float g_bestArm = 0;            // the widest span's arm length over this hold (V's scale; 0 = none yet)

float Dot3(const XrVector3f& a, const XrVector3f& b) { return a.x * b.x + a.y * b.y + a.z * b.z; }
XrVector3f Sub3(const XrVector3f& a, const XrVector3f& b) { return {a.x - b.x, a.y - b.y, a.z - b.z}; }

// The cross for step 3 from step 1's head and the calibration step 2 took; false without V's reference.
bool SetUpCross(const Calibration& c, const Pose& leftFoot, const Pose& rightFoot) {
    VrikFbtReference ref{};
    if (!VRIK_ReadFbtReference(&ref) || c.avatarEyeHeight <= 0.1f || c.playerEyeHeight <= 0.1f) return false;
    const float k = c.playerEyeHeight / c.avatarEyeHeight;   // V's metres -> the player's
    const XrVector3f f3 = Rotate(g_levelHead.orientation, {0, 0, -1});
    const float fl = std::sqrt(f3.x * f3.x + f3.z * f3.z);
    if (fl < 0.3f) return false;
    Cross x;
    x.fwd = {f3.x / fl, 0, f3.z / fl};
    x.right = {-x.fwd.z, 0, x.fwd.x};
    x.spot = {g_levelHead.position.x, 0, g_levelHead.position.z};
    const char* from = "the head (no feet trackers)";
    if (leftFoot.stageValid && rightFoot.stageValid) {
        const XrVector3f& a = leftFoot.stage.position;
        const XrVector3f& b = rightFoot.stage.position;
        float rx = b.x - a.x, rz = b.z - a.z;
        const float rl = std::sqrt(rx * rx + rz * rz);
        x.spot = {0.5f * (a.x + b.x), 0, 0.5f * (a.z + b.z)};
        from = "the feet (too close for their line: the gaze's heading)";
        if (rl > 0.10f) {   // the feet's line, turned to face the way the gaze looked (never flipped)
            rx /= rl; rz /= rl;
            if (rx * x.right.x + rz * x.right.z < 0) { rx = -rx; rz = -rz; }
            x.right = {rx, 0, rz};
            x.fwd = {rz, 0, -rx};   // right = (-fwd.z, 0, fwd.x)
            from = "the feet";
        }
        const float off = (g_levelHead.position.x - x.spot.x) * x.right.x + (g_levelHead.position.z - x.spot.z) * x.right.z;
        Log("[fbt-calib] step 3 cross from %s: step 1's head was %+.3f m sideways of the feet's midpoint\n", from, off);
    } else {
        Log("[fbt-calib] step 3 cross from %s\n", from);
    }
    x.base = {x.spot.x + x.fwd.x * kCrossAhead, 0, x.spot.z + x.fwd.z * kCrossAhead};
    x.tipY = c.playerEyeHeight;
    const float shoulderZ = 0.5f * (ref.upperArmPos[0][2] + ref.upperArmPos[1][2]);
    x.barY = c.playerEyeHeight - (c.avatarEyeHeight - shoulderZ) * k;
    const float halfShoulders = 0.5f * std::fabs(ref.upperArmPos[1][0] - ref.upperArmPos[0][0]);
    x.halfSpan = (halfShoulders + 0.5f * (g_calibrator.AvatarArmLength(0) + g_calibrator.AvatarArmLength(1))) * k;
    {
        std::lock_guard lock(g_crossMutex);
        g_cross = x;
        g_crossView = {};
    }
    g_holdStart = 0;
    g_bestArm = 0;
    Log("[fbt-calib] step 3 cross: %.2f m ahead, top %.3f m, arms at %.3f m, half span %.3f m (V's at the player's size)\n",
        kCrossAhead, x.tipY, x.barY, x.halfSpan);
    return true;
}

// 0 off its mark, 1 near, 2 on it, for a value against its tolerance.
int Mark(float error, float tolerance) { return error <= tolerance ? 2 : error <= 2 * tolerance ? 1 : 0; }

void CrossTick(XrSpace view, XrSpace stage, XrTime time, const Pose hands[2], const wrpuppets::fbt::RoomPose devices[]) {
    Cross x;
    {
        std::lock_guard lock(g_crossMutex);
        x = g_cross;
    }
    CrossView out;
    out.base = x.base; out.right = x.right; out.tipY = x.tipY; out.barY = x.barY; out.halfSpan = x.halfSpan;
    auto onCross = [&](const XrVector3f& p) {
        const float lateral = Dot3(Sub3(p, x.spot), x.right);
        return XrVector3f{x.base.x + x.right.x * lateral, p.y, x.base.z + x.right.z * lateral};
    };
    XrSpaceLocation headStage{XR_TYPE_SPACE_LOCATION};
    constexpr XrSpaceLocationFlags kNeed = XR_SPACE_LOCATION_POSITION_VALID_BIT | XR_SPACE_LOCATION_ORIENTATION_VALID_BIT;
    const bool headOk = stage != XR_NULL_HANDLE && view != XR_NULL_HANDLE &&
                        XR_SUCCEEDED(xrLocateSpace(view, stage, time, &headStage)) && (headStage.locationFlags & kNeed) == kNeed;
    if (headOk) {
        const XrVector3f& p = headStage.pose.position;
        out.dot[0] = onCross(p);
        const float side = std::fabs(Dot3(Sub3(p, x.spot), x.right)), up = std::fabs(p.y - x.tipY);
        const int level = Mark(std::fabs(HeadPitchDeg(headStage.pose.orientation)), kCalibrationLevelDeg);
        out.state[0] = std::min({Mark(side, kHeadSide), Mark(up, kHeadHeight), level});
    }
    for (int h = 0; h < 2; ++h) {
        if (!hands[h].stageValid) continue;
        const XrVector3f& p = hands[h].stage.position;
        out.dot[1 + h] = onCross(p);
        const XrVector3f d = Sub3(p, x.spot);
        const float along = Dot3(d, x.right) * (h == 0 ? -1.0f : 1.0f);   // out to its own side
        const float reach = x.halfSpan > 0.1f ? along / x.halfSpan : 0.0f;
        const int outMark = reach >= kHandOut ? 2 : reach >= 0.5f * kHandOut ? 1 : 0;
        out.state[1 + h] = std::min({Mark(std::fabs(p.y - x.barY), kHandHeight), Mark(std::fabs(Dot3(d, x.fwd)), kHandDepth), outMark});
    }
    const bool all = out.state[0] == 2 && out.state[1] == 2 && out.state[2] == 2;
    const ULONGLONG now = GetTickCount64();
    if (!all) {
        if (g_holdStart) Log("[fbt-calib] step 3: the pose was left (head %d, left hand %d, right hand %d): the countdown restarts\n",
                             out.state[0], out.state[1], out.state[2]);
        g_holdStart = 0;
        g_bestArm = 0;
    } else {
        if (!g_holdStart) { g_holdStart = now; Log("[fbt-calib] step 3: on the cross, countdown\n"); }
        Progress();
        // The widest span over the hold: the arms are straightest there (a bent elbow only shortens it).
        Calibration c;
        {
            std::lock_guard lock(g_calibrationMutex);
            c = g_calibration;
        }
        if (headOk && g_calibrator.MeasureArmsOut(c, ToRoomImpl(true, headStage.pose), devices) && c.armLength[0] > g_bestArm)
            g_bestArm = c.armLength[0];
        const float held = float(now - g_holdStart) / 1000.0f;
        out.countdown = std::max(0.0f, kCrossHoldSeconds - held);
        if (held >= kCrossHoldSeconds) {
            g_holdStart = 0;
            if (g_bestArm <= 0) {
                Log("[fbt-calib] step 3: the span between the hands was never a T-pose's (hands not side by side at the "
                    "shoulders, or the arms' length out of 0.6..1.5 x V's): hold it again\n");
                Refuse(kRefuseArm);
            } else {
                {
                    std::lock_guard lock(g_calibrationMutex);
                    g_calibration.armLength[0] = g_calibration.armLength[1] = g_bestArm;
                    c = g_calibration;
                }
                Accept();
                Log("[fbt-calib] step 3 captured: arms %.3f m each (V's scale, from the widest span) -> V's arms x %.3f / x %.3f. "
                    "Calibrated\n", g_bestArm, g_calibrator.ArmScale(c, 0), g_calibrator.ArmScale(c, 1));
                Save(c);
                g_step.store(0, std::memory_order_relaxed);
                Log("[fbt-calib] calibration mode off\n");
            }
            g_bestArm = 0;
        }
    }
    std::lock_guard lock(g_crossMutex);
    g_crossView = out;
}

// V's skeleton from the rig file the plugin writes (the FBT IK lab reads the same file).
bool PrepareCalibrator() {
    char path[MAX_PATH]{};
    VrikFbtReference ref{};
    if (!VRIK_ReadFbtReference(&ref) || !VRIK_FbtRigPath(ref.female, path, sizeof(path))) {
        static ULONGLONG s_lastLog = 0;   // asked every frame by the body solve: logged at most every 10 s
        if (GetTickCount64() - s_lastLog > 10000) { s_lastLog = GetTickCount64(); Log("[fbt-calib] V's rig is not loaded yet\n"); }
        return false;
    }
    if (g_calibratorReady && std::strcmp(path, g_skeletonPath) == 0) return true;
    std::string error;
    wrpuppets::fbt::Skeleton sk;
    if (!sk.Load(path, &error)) { Log("[fbt-calib] cannot read %s: %s\n", path, error.c_str()); return false; }
    g_skeleton = std::move(sk);
    if (!g_calibrator.Init(g_skeleton)) { Log("[fbt-calib] %s lacks bones the calibration needs\n", path); g_calibratorReady = false; return false; }
    strcpy_s(g_skeletonPath, path);
    g_calibratorReady = true;
    Log("[fbt-calib] V's skeleton: %s (%d bones)\n", path, g_skeleton.Count());
    return true;
}

// STAGE (x right, y up, -z forward) -> V's model axes (x right, y forward, z up), as the lab's VrSession does.
wrpuppets::fbt::RoomPose ToRoomImpl(bool valid, const XrPosef& p) {
    wrpuppets::fbt::RoomPose r;
    r.valid = valid;
    r.pos = {p.position.x, -p.position.z, p.position.y};
    r.rot = wrpuppets::fbt::Normalize(wrpuppets::fbt::Quat{p.orientation.x, -p.orientation.z, p.orientation.y, p.orientation.w});
    return r;
}

void Save(const Calibration& c) {
    const std::string path = cvr::paths::FileA(cvr::paths::Folder::Config, "fbt_calibration.ini");
    FILE* f = nullptr;
    if (fopen_s(&f, path.c_str(), "w") != 0 || !f) { Log("[fbt-calib] could not write %s\n", path.c_str()); return; }
    fprintf(f, "; Full-body tracking calibration (src/Fbt/FbtCalibration). Room = STAGE in V's model axes, metres.\n");
    fprintf(f, "version=2\nscale=%.7f\nplayer_eye_height=%.5f\nv_eye_height=%.5f\n", c.scale, c.playerEyeHeight, c.avatarEyeHeight);
    fprintf(f, "origin=%.6f,%.6f,%.6f\nyaw_inverse=%.7f,%.7f,%.7f,%.7f\nhead_rot_inverse=%.7f,%.7f,%.7f,%.7f\n", c.origin.x, c.origin.y,
            c.origin.z, c.yawInverse.x, c.yawInverse.y, c.yawInverse.z, c.yawInverse.w, c.headRotInverse.x, c.headRotInverse.y,
            c.headRotInverse.z, c.headRotInverse.w);
    fprintf(f, "arm_length=%.5f,%.5f\n", c.armLength[0], c.armLength[1]);
    const char* key[wrpuppets::fbt::kCalCount] = {"waist", "left_foot", "right_foot", "left_hand", "right_hand"};
    for (int i = 0; i < wrpuppets::fbt::kCalCount; ++i) {
        if (!c.valid[i]) continue;
        fprintf(f, "%s_offset=%.6f,%.6f,%.6f\n%s_rot_inverse=%.7f,%.7f,%.7f,%.7f\n", key[i], c.offset[i].x, c.offset[i].y, c.offset[i].z,
                key[i], c.rotInverse[i].x, c.rotInverse[i].y, c.rotInverse[i].z, c.rotInverse[i].w);
    }
    fclose(f);
    Log("[fbt-calib] saved %s\n", path);
}

void CalibrationStepTick(XrSpace view, XrSpace g_stage, XrTime time, const Pose poses[kRoleCount], const Pose hands[2]) {
    const int step = g_step.load(std::memory_order_relaxed);
    if (step == 3) {   // the T-pose: every frame, no pull
        wrpuppets::fbt::RoomPose devices[wrpuppets::fbt::kCalCount];
        devices[wrpuppets::fbt::kCalWaist] = ToRoomImpl(poses[kWaist].stageValid, poses[kWaist].stage);
        devices[wrpuppets::fbt::kCalLeftFoot] = ToRoomImpl(poses[kLeftFoot].stageValid, poses[kLeftFoot].stage);
        devices[wrpuppets::fbt::kCalRightFoot] = ToRoomImpl(poses[kRightFoot].stageValid, poses[kRightFoot].stage);
        devices[wrpuppets::fbt::kCalLeftHand] = ToRoomImpl(hands[0].stageValid, hands[0].stage);
        devices[wrpuppets::fbt::kCalRightHand] = ToRoomImpl(hands[1].stageValid, hands[1].stage);
        CrossTick(view, g_stage, time, hands, devices);
        return;
    }
    const float l = g_trigger[0].load(std::memory_order_relaxed), r = g_trigger[1].load(std::memory_order_relaxed);
    if (!g_armed) { g_armed = l < kTriggerOff && r < kTriggerOff; return; }
    if (l < kTriggerOn || r < kTriggerOn) return;
    g_armed = false;   // one pull, one action
    XrSpaceLocation headStage{XR_TYPE_SPACE_LOCATION};
    constexpr XrSpaceLocationFlags kNeed = XR_SPACE_LOCATION_POSITION_VALID_BIT | XR_SPACE_LOCATION_ORIENTATION_VALID_BIT;
    if (g_stage == XR_NULL_HANDLE || view == XR_NULL_HANDLE || XR_FAILED(xrLocateSpace(view, g_stage, time, &headStage)) ||
        (headStage.locationFlags & kNeed) != kNeed) {
        Log("[fbt-calib] head not tracked in STAGE: pull again\n");
        Refuse(kRefuseTracking);
        return;
    }
    wrpuppets::fbt::RoomPose head = ToRoomImpl(true, headStage.pose);
    wrpuppets::fbt::RoomPose devices[wrpuppets::fbt::kCalCount];
    devices[wrpuppets::fbt::kCalWaist] = ToRoomImpl(poses[kWaist].stageValid, poses[kWaist].stage);
    devices[wrpuppets::fbt::kCalLeftFoot] = ToRoomImpl(poses[kLeftFoot].stageValid, poses[kLeftFoot].stage);
    devices[wrpuppets::fbt::kCalRightFoot] = ToRoomImpl(poses[kRightFoot].stageValid, poses[kRightFoot].stage);
    devices[wrpuppets::fbt::kCalLeftHand] = ToRoomImpl(hands[0].stageValid, hands[0].stage);
    devices[wrpuppets::fbt::kCalRightHand] = ToRoomImpl(hands[1].stageValid, hands[1].stage);
    if (step == 1) {
        const float pitch = HeadPitchDeg(headStage.pose.orientation);
        if (std::fabs(pitch) > kLevelPullDeg) {
            Log("[fbt-calib] step 1: the gaze is %.0f deg off level -- look at the horizon line (it turns green), pull again\n", pitch);
            Refuse(kRefuseLevel);
            return;
        }
        g_levelHead = headStage.pose;
        Accept();
        Progress();
        Log("[fbt-calib] step 1 captured: eye height %.3f m, gaze %+.1f deg. Now arms along the body, look down, both triggers\n",
            headStage.pose.position.y, pitch);
        g_step.store(2, std::memory_order_relaxed);
        return;
    }
    if (step == 2) {
        if (!PrepareCalibrator()) return;
        head = ToRoomImpl(true, g_levelHead);
        // THE BODY FACES THE WAY THE FEET FACE, AND STANDS OVER THEM. The calibration takes its body frame from the
        // headset (origin under it, heading from it). Step 1's head is the level gaze, but the body may have turned or
        // shifted since, or leaned at it: a heading off the feet's kept V turned from the player and failed the T-pose
        // (its arms along the cross, which faces the feet, were "not out to the sides" of the head's frame -- in game,
        // after a turn to match V's feet), and a head off to one side kept V's hips and feet beside the view (the head
        // 14 cm left of the feet, every tracker offset -0.14 m sideways). With both feet tracked and apart, the capture
        // takes the headset as looking straight along the feet (its yaw turned onto theirs; pitch and roll kept) and
        // over their midpoint sideways; forwards and in height it stays as step 1 took it.
        if (devices[wrpuppets::fbt::kCalLeftFoot].valid && devices[wrpuppets::fbt::kCalRightFoot].valid) {
            using wrpuppets::fbt::Vec3;
            const Vec3& lf = devices[wrpuppets::fbt::kCalLeftFoot].pos;
            const Vec3& rf = devices[wrpuppets::fbt::kCalRightFoot].pos;
            float fx = rf.x - lf.x, fy = rf.y - lf.y;   // the feet's line, left to right (model axes: x right, y forward)
            const float fl = std::sqrt(fx * fx + fy * fy);
            const Vec3 h3 = wrpuppets::fbt::Rotate(head.rot, Vec3{0, 1, 0});
            const float hl = std::sqrt(h3.x * h3.x + h3.y * h3.y);
            if (fl > 0.10f && hl > 0.3f) {
                fx /= fl; fy /= fl;
                const float hx = h3.x / hl, hy = h3.y / hl;   // the gaze's heading, level
                float ffx = -fy, ffy = fx;                    // the feet's forward: their line turned +90 deg
                if (ffx * hx + ffy * hy < 0) { ffx = -ffx; ffy = -ffy; fx = -fx; fy = -fy; }   // the way the player faces
                const float turn = std::atan2(hx * ffy - hy * ffx, hx * ffx + hy * ffy);
                head.rot = wrpuppets::fbt::Normalize(wrpuppets::fbt::AxisAngle(Vec3{0, 0, 1}, turn) * head.rot);
                Log("[fbt-calib] step 2: the gaze was %+.1f deg off the feet's heading: the body faces the feet\n", -turn * 57.29578f);
            }
            const Vec3 r3 = wrpuppets::fbt::Rotate(head.rot, Vec3{1, 0, 0});
            const float rl = std::sqrt(r3.x * r3.x + r3.y * r3.y);
            if (rl > 0.3f) {
                const float rx = r3.x / rl, ry = r3.y / rl;   // the body's right, level
                const float side = (0.5f * (lf.x + rf.x) - head.pos.x) * rx + (0.5f * (lf.y + rf.y) - head.pos.y) * ry;
                head.pos.x += side * rx;
                head.pos.y += side * ry;
                Log("[fbt-calib] step 2: the head was %+.3f m sideways of the feet's midpoint (- left): taken over the feet\n", -side);
            }
        }
        Calibration c;
        const char* why = nullptr;
        if (!g_calibrator.Capture(head, devices, &c, &why)) {
            Log("[fbt-calib] step 2 not captured: %s; pull again\n", why ? why : "?");
            Refuse(kRefuseTracking);
            return;
        }
        Accept();
        Progress();
        c.armLength[0] = c.armLength[1] = 0;   // a hanging length is not a scale: step 2 measures the arms
        {
            std::lock_guard lock(g_calibrationMutex);
            g_calibration = c;
        }
        Log("[fbt-calib] step 2 captured: player eye height %.3f m, V's %.3f m -> scale %.4f; waist %s, feet %s/%s, hands %s/%s. "
            "Now the T-pose on the cross\n", c.playerEyeHeight, c.avatarEyeHeight, c.scale,
            c.valid[wrpuppets::fbt::kCalWaist] ? "ok" : "MISSING", c.valid[wrpuppets::fbt::kCalLeftFoot] ? "ok" : "MISSING",
            c.valid[wrpuppets::fbt::kCalRightFoot] ? "ok" : "MISSING", c.valid[wrpuppets::fbt::kCalLeftHand] ? "ok" : "missing",
            c.valid[wrpuppets::fbt::kCalRightHand] ? "ok" : "missing");
        if (!SetUpCross(c, poses[kLeftFoot], poses[kRightFoot])) {
            Log("[fbt-calib] V's reference is not available for the cross: arms not measured, V's kept. Calibrated\n");
            Save(c);
            g_step.store(0, std::memory_order_relaxed);
            return;
        }
        g_step.store(3, std::memory_order_relaxed);
        return;
    }
}
}  // namespace

void CalibrationTick(XrSpace view, XrSpace stage, XrTime time, const Pose poses[kRoleCount], const Pose hands[2]) {
    const ULONGLONG now = GetTickCount64();
    const uintptr_t player = cvr::roomscale::PlayerIdentity();
    g_menuSince = g_menuModeValue != 0 ? (g_menuSince ? g_menuSince : now) : 0;
    const char* why = player != g_calibrationPlayer ? "the player changed (a load or the main menu)"
                    : g_menuSince && now - g_menuSince > 2000 ? "a game menu is open"
                    : now - g_lastProgress > 120000 ? "2 minutes without a step" : nullptr;
    if (why) {
        Log("[fbt-calib] calibration cancelled: %s\n", why);
        CancelCalibration();
        return;
    }
    CalibrationStepTick(view, stage, time, poses, hands);
}

bool CalibrationAvailable() { return !cvr::lobby::Active() && cvr::roomscale::PlayerIdentity() != 0; }

void StartCalibration() {
    if (!CalibrationAvailable()) { Log("[fbt-calib] not started: the full-body calibration runs in game only (load a save)\n"); return; }
    if (!cvr::trackers::Running()) { Log("[fbt-calib] Vive trackers are off (xr_vive_trackers=0): nothing to calibrate\n"); return; }
    g_armed = false;
    Accept();
    g_calibrationPlayer = cvr::roomscale::PlayerIdentity();
    g_menuSince = 0;
    Progress();
    PrepareCalibrator();   // V's skeleton now, so a missing rig shows before the first pull
    g_step.store(1, std::memory_order_relaxed);
    Log("[fbt-calib] calibration step 1: look ahead at the horizon line; release then pull both triggers\n");
    VrikFbtReference v{};
    if (VRIK_ReadFbtReference(&v)) LogReference(v);
    else Log("[fbt-calib] V's reference pose is not available yet\n");
}
void CancelCalibration() {
    const int step = g_step.exchange(0, std::memory_order_relaxed);
    if (step == 1 || step == 2) Log("[fbt-calib] calibration cancelled\n");
    if (step == 3) {
        Calibration c;
        {
            std::lock_guard lock(g_calibrationMutex);
            c = g_calibration;
        }
        Save(c);
        Log("[fbt-calib] arms step cancelled: an arm not measured keeps V's length\n");
    }
}
bool ReadCross(CrossView* out) {
    if (g_step.load(std::memory_order_relaxed) != 3 || !out) return false;
    std::lock_guard lock(g_crossMutex);
    *out = g_crossView;
    return true;
}
bool Calibrating() { return g_step.load(std::memory_order_relaxed) != 0; }
int CalibrationStep() { return g_step.load(std::memory_order_relaxed); }

namespace {
std::mutex g_textMutex;
std::string g_stepText[kStepTextSlots] = {"", "Step 1: look ahead at the horizon line, then press both triggers",
                             "Step 2: arms down, look down, then press both triggers",
                             "Step 3: face the cross: head on its top, arms straight out along its arms, until the countdown ends",
                             "",
                             "Not taken: look straight at the horizon line until it turns green, then press again",
                             "Not taken: stretch both arms straight out along the cross, then hold still again",
                             "Not taken: stand straight with the headset, trackers and controllers tracked, then press again"};
}
void SetStepText(int step, const char* text) {
    if (step < 1 || step >= kStepTextSlots || !text || !*text) return;
    std::lock_guard lock(g_textMutex);
    g_stepText[step] = text;
}
std::string StepText(int step) {
    if (step < 1 || step >= kStepTextSlots) return {};
    std::lock_guard lock(g_textMutex);
    return g_stepText[step];
}
std::string RefusalText() {
    if (!Calibrating() || GetTickCount64() >= g_refusedUntil.load(std::memory_order_relaxed)) return {};
    return StepText(g_refused.load(std::memory_order_relaxed));
}

void NoteTrigger(int hand, float value) {
    if (hand == 0 || hand == 1) g_trigger[hand].store(value, std::memory_order_relaxed);
}
bool ReadCalibration(Calibration* out) {
    std::lock_guard lock(g_calibrationMutex);
    if (!out || !g_calibration.done) return false;
    *out = g_calibration;
    return true;
}

bool Rig(const wrpuppets::fbt::Skeleton** skeleton, const wrpuppets::fbt::Calibrator** calibrator) {
    static std::mutex m;
    std::lock_guard lock(m);
    if (!PrepareCalibrator()) return false;
    if (skeleton) *skeleton = &g_skeleton;
    if (calibrator) *calibrator = &g_calibrator;
    return true;
}

wrpuppets::fbt::RoomPose ToRoom(bool valid, const XrPosef& pose) { return ToRoomImpl(valid, pose); }

}  // namespace cvr::fbt::game
