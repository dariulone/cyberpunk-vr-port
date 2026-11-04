#include "Core/Paths.hpp"
#include "Fbt/Game/Body.hpp"
#include "Fbt/Game/Calibration.hpp"
#include "Fbt/Game/Trackers.hpp"
#include "Fbt/FbtFilter.hpp"
#include "Fbt/FbtIk.hpp"
#include "Anim/CharacterRig.hpp"
#include "Anim/VrikState.hpp"
#include <Windows.h>
#include <algorithm>
#include <atomic>
#include <cmath>
#include <cstdio>
#include <cstring>

extern void Log(const char*, ...);

using namespace wrpuppets::fbt;   // the IK library (externals/wrpuppets-fbt-ik)

namespace cvr::fbt::game {
namespace {
std::atomic<int> g_legs{-1}, g_spine{0}, g_arms{0};   // g_legs -1: the file not read yet
std::atomic<float> g_eyeZ{0.0f};                     // V's eye height as calibrated, model space (the camera's target)
std::atomic<bool> g_eyeZOn{false};
std::atomic<unsigned long long> g_eyeZTime{0};       // when the solve last set it: older than 200 ms counts as off
float g_armScale[2] = {1.0f, 1.0f};                  // from the calibration, set by the body solve (same thread)
bool g_bodySolved = false;                            // the body solve ran in this pose pass (with the spine)

void SettingsPath(char* path) {
    strcpy_s(path, MAX_PATH, cvr::paths::FileA(cvr::paths::Folder::Config, "fbt_settings.ini").c_str());
}
void ReadSettings() {
    if (g_legs.load(std::memory_order_relaxed) >= 0) return;
    char path[MAX_PATH];
    SettingsPath(path);
    int legs = 0, spine = 0, arms = 0;
    FILE* f = nullptr;
    if (path[0] && fopen_s(&f, path, "r") == 0 && f) {
        char line[128];
        while (fgets(line, sizeof(line), f)) {
            sscanf_s(line, "legs=%d", &legs);
            sscanf_s(line, "spine=%d", &spine);
            sscanf_s(line, "arms=%d", &arms);
        }
        fclose(f);
    }
    g_spine.store(spine ? 1 : 0, std::memory_order_relaxed);
    g_arms.store(arms ? 1 : 0, std::memory_order_relaxed);
    g_legs.store(legs ? 1 : 0, std::memory_order_relaxed);
}
void WriteSettings() {
    char path[MAX_PATH];
    SettingsPath(path);
    FILE* f = nullptr;
    if (!path[0] || fopen_s(&f, path, "w") != 0 || !f) return;
    fprintf(f, "; Full-body tracking settings (src/Fbt/Game).\nlegs=%d\nspine=%d\narms=%d\n", g_legs.load() == 1 ? 1 : 0,
            g_spine.load() == 1 ? 1 : 0, g_arms.load() == 1 ? 1 : 0);
    fclose(f);
}

// The stack, its filters and the last state, owned by the pose pass (one thread).
struct State {
    const Skeleton* skeleton = nullptr;
    IkStack stack;
    bool stackReady = false;
    PoseFilter filters[kCalCount], head;
    LARGE_INTEGER lastTime{}, frequency{};
    ULONGLONG lastLog = 0;
    unsigned long long lastSample = 0;
    int solves = 0, newSamples = 0, sameSample = 0;   // since the last log line
} g;

float YawOf(Vec3 forward) { return std::atan2(forward.y, forward.x); }
// The heading of a view (x right, y forward, z up) that holds however far it pitches: forward and up both flattened,
// weighted by the pitch's cosine and sine -- for a pure nod exactly the facing direction, straight down included
// (the flattened forward alone shrinks to nothing and swings round looking down: chin on the chest, in game).
Vec3 Heading(Quat view) {
    const Vec3 f = Rotate(view, {0, 1, 0}), u = Rotate(view, {0, 0, 1});
    const float c = std::hypot(f.x, f.y), sn = f.z;
    return {f.x * c - u.x * sn, f.y * c - u.y * sn, 0};
}
}  // namespace

bool LegsEnabled() {
    ReadSettings();
    return g_legs.load(std::memory_order_relaxed) == 1;
}
void SetLegsEnabled(bool on) {
    ReadSettings();
    g_legs.store(on ? 1 : 0, std::memory_order_relaxed);
    WriteSettings();
    Log("[fbt] V's legs follow the trackers: %s\n", on ? "on" : "off");
}
bool SpineEnabled() {
    ReadSettings();
    return g_spine.load(std::memory_order_relaxed) == 1;
}
void SetSpineEnabled(bool on) {
    ReadSettings();
    g_spine.store(on ? 1 : 0, std::memory_order_relaxed);
    WriteSettings();
    Log("[fbt] V's spine and head follow the headset: %s\n", on ? "on" : "off");
}

bool ArmsEnabled() {
    ReadSettings();
    return g_arms.load(std::memory_order_relaxed) == 1;
}
void SetArmsEnabled(bool on) {
    ReadSettings();
    g_arms.store(on ? 1 : 0, std::memory_order_relaxed);
    WriteSettings();
    Log("[fbt] V's arms follow the controllers: %s\n", on ? "on" : "off");
}

bool BodyActive() {
    const ULONGLONG at = g_eyeZTime.load(std::memory_order_relaxed);
    return at != 0 && GetTickCount64() - at <= 200;
}

bool CameraEyeHeight(float* modelZ) {
    // Only while the body solve runs: off foot (vehicle, ladder, swimming), without a calibration or with the switches
    // off it stops, and the camera goes back to the game's.
    if (!g_eyeZOn.load(std::memory_order_relaxed) || GetTickCount64() - g_eyeZTime.load(std::memory_order_relaxed) > 200) return false;
    if (modelZ) *modelZ = g_eyeZ.load(std::memory_order_relaxed);
    return true;
}

void SolveLowerBody(uint8_t* boneBuf, const float camModelPos[3], const float camModelRot[4]) {
    g_bodySolved = false;
    if (!boneBuf || !LegsEnabled() || Calibrating()) { g_eyeZOn.store(false, std::memory_order_relaxed); return; }
    Calibration c;
    const Skeleton* sk = nullptr;
    const Calibrator* calibrator = nullptr;
    if (!ReadCalibration(&c) || !Rig(&sk, &calibrator)) return;
    if (sk != g.skeleton || !g.stackReady) {
        g.skeleton = sk;
        g.stackReady = g.stack.Init(*sk);
        if (!g.stackReady) Log("[fbt] the IK stack lacks bones on this rig\n");
    }
    if (!g.stackReady) return;

    // The devices (STAGE, the same instant as the head), filtered as in the lab; the head goal only held through
    // impossible steps.
    XrPosef headStage{};
    if (!cvr::trackers::ReadHeadStage(&headStage)) return;
    RoomPose devices[kCalCount];
    const cvr::trackers::Role roles[3] = {cvr::trackers::kWaist, cvr::trackers::kLeftFoot, cvr::trackers::kRightFoot};
    const int slots[3] = {kCalWaist, kCalLeftFoot, kCalRightFoot};
    for (int i = 0; i < 3; ++i) {
        cvr::trackers::Pose p{};
        cvr::trackers::Read(roles[i], &p);
        devices[slots[i]] = ToRoom(p.stageValid, p.stage);
    }
    // The time step from the high-resolution clock (GetTickCount64 ticks every 10-16 ms: at 90 fps many steps read 0,
    // which the filter takes as a restart and passes the raw pose through -- raw and filtered frames alternating).
    // A pass that sees the same device sample as the last one keeps the filter as it is.
    const ULONGLONG now = GetTickCount64();
    LARGE_INTEGER t;
    QueryPerformanceCounter(&t);
    if (!g.frequency.QuadPart) QueryPerformanceFrequency(&g.frequency);
    const unsigned long long sample = cvr::trackers::LocateCount();
    const bool fresh = sample != g.lastSample;
    ++g.solves;
    if (fresh) ++g.newSamples; else ++g.sameSample;
    static RoomPose s_filtered[kCalCount], s_head;
    if (fresh) {
        const float dt = g.lastTime.QuadPart ? std::fmin(0.1f, float(t.QuadPart - g.lastTime.QuadPart) / float(g.frequency.QuadPart)) : 0.0f;
        g.lastTime = t;
        g.lastSample = sample;
        FilterOptions fo;
        for (int i = 0; i < kCalCount; ++i) s_filtered[i] = g.filters[i].Filter(devices[i], dt, fo);
        s_head = g.head.Filter(ToRoom(true, headStage), dt, HeadGoalFilter());
    }
    for (int i = 0; i < kCalCount; ++i) devices[i] = s_filtered[i];
    const RoomPose head = s_head;
    if (!head.valid) return;

    // The game's pose (local transforms; the skeleton keeps the buffer's bone indices) -> the stack -> back.
    const int n = std::min(sk->Count(), std::min(VRIK_FKCount(), VRIK_MAX_BONES));
    Pose pose = sk->ReferencePose();
    for (int i = 0; i < n; ++i) {
        const float* t = reinterpret_cast<const float*>(boneBuf + i * 48 + VRIK_TRANS_OFF);
        const float* r = reinterpret_cast<const float*>(boneBuf + i * 48 + VRIK_ROT_OFF);
        pose.localPos[i] = {t[0], t[1], t[2]};
        pose.localRot[i] = {r[0], r[1], r[2], r[3]};
    }
    // The spine chain starts from V's reference pose, as in the lab: the solve bends it from where it starts, and the
    // game's animation moves it every frame (breathing, the first-person lean with the camera's pitch, strongest looking
    // down), which came through as the chest shaking with the head held still.
    if (SpineEnabled()) {
        const Pose ref = sk->ReferencePose();
        static const char* const kSpineChain[] = {"spine0", "spine1", "spine2", "spine3", "spine4", "spine5",
                                                  "spine6", "spine7", "neck",   "neck1",  "head"};
        for (const char* role : kSpineChain) {
            const int b = sk->Role(role);
            if (b < 0 || b >= n) continue;
            pose.localPos[b] = ref.localPos[b];
            pose.localRot[b] = ref.localRot[b];
        }
    }
    sk->Fk(pose);
    const int cameraBoneEarly = sk->Role("camera");
    const Vec3 portCameraBone = cameraBoneEarly >= 0 && cameraBoneEarly < n ? pose.modelPos[cameraBoneEarly] : Vec3{};
    // The goals in the calibration's model space, then into the game's: horizontally and in yaw relative to the
    // headset, placed from the camera; the height as calibrated, from the floor.
    Goal goals[kGoalCount];
    calibrator->Goals(c, head, devices, goals);
    const Vec3 eyeCal = calibrator->ToModel(c, head.pos);
    const Vec3 fwdCal = Heading(Normalize(c.yawInverse * head.rot));
    const Quat camRot{camModelRot[0], camModelRot[1], camModelRot[2], camModelRot[3]};
    const Vec3 fwdGame = Heading(camRot);
    if (std::hypot(fwdCal.x, fwdCal.y) < 0.1f || std::hypot(fwdGame.x, fwdGame.y) < 0.1f) return;   // a head rolled 90 deg
    const Quat turn = AxisAngle({0, 0, 1}, YawOf(fwdGame) - YawOf(fwdCal));
    const bool spine = SpineEnabled();
    for (int i = 0; i < kGoalCount; ++i) {
        if (!goals[i].active) continue;
        const Vec3 d = Rotate(turn, goals[i].pos - eyeCal);
        // The head relative to the camera in height too (V's eyes on the view); the body from the floor.
        const float z = i == kGoalHead ? camModelPos[2] + d.z : goals[i].pos.z;
        goals[i].pos = {camModelPos[0] + d.x, camModelPos[1] + d.y, z};
        goals[i].rot = Normalize(turn * goals[i].rot);
    }
    goals[kGoalLeftHand].active = goals[kGoalRightHand].active = false;   // the port's arms
    // The camera at V's eyes (with the spine): the camera patch sets the view's height to V's eye height as calibrated
    // (from the floor), and the head goal -- V's eyes on the headset -- takes that height too.
    const int headBone = sk->Role("head"), cameraBone = sk->Role("camera");
    const Vec3 portHead = headBone >= 0 && headBone < n ? pose.modelPos[headBone] : Vec3{};
    g_eyeZ.store(eyeCal.z, std::memory_order_relaxed);
    g_eyeZOn.store(spine, std::memory_order_relaxed);
    g_armScale[0] = calibrator->ArmScale(c, 0);
    g_armScale[1] = calibrator->ArmScale(c, 1);
    g_eyeZTime.store(GetTickCount64(), std::memory_order_relaxed);
    if (spine) goals[kGoalHead].pos.z += eyeCal.z - camModelPos[2];   // eyes at V's eye height, where the camera is drawn
    if (!spine) goals[kGoalHead].active = false;

    StackOptions o;
    o.spine = spine;
    o.arms = o.twist = o.clavicle = false;
    StackDebug dbg;
    g.stack.Solve(pose, goals, o, &dbg);
    // Written back: the hips and legs; with the spine also the spine chain to the head (Spine .. Spine7 as the rig has
    // them, Neck, Neck1, Head).
    const char* roleNames[] = {"hips", "left_upleg", "left_leg", "left_foot", "right_upleg", "right_leg", "right_foot",
                               "spine0", "spine1", "spine2", "spine3", "spine4", "spine5", "spine6", "spine7", "neck", "neck1", "head"};
    const int legRoles = 7;
    for (int k = 0; k < (spine ? static_cast<int>(sizeof(roleNames) / sizeof(roleNames[0])) : legRoles); ++k) {
        const char* role = roleNames[k];
        const int b = sk->Role(role);
        if (b < 0 || b >= n) continue;
        float* t = reinterpret_cast<float*>(boneBuf + b * 48 + VRIK_TRANS_OFF);
        float* r = reinterpret_cast<float*>(boneBuf + b * 48 + VRIK_ROT_OFF);
        t[0] = pose.localPos[b].x; t[1] = pose.localPos[b].y; t[2] = pose.localPos[b].z;
        r[0] = pose.localRot[b].x; r[1] = pose.localRot[b].y; r[2] = pose.localRot[b].z; r[3] = pose.localRot[b].w;
        if (k > 0 && k < legRoles) VRIK_MarkLegOwned(b);
    }
    // BELOW THE ANKLES: the heels and the toes as the reference pose has them under the solved feet. The solve ends at
    // the foot, and the game's walk cycle -- played while the player walks the room -- kept lifting the toes.
    {
        static const Skeleton* s_for = nullptr;
        static std::vector<int> s_belowFeet;
        if (s_for != sk) {
            s_for = sk;
            s_belowFeet.clear();
            const int feet[2] = {sk->Role("left_foot"), sk->Role("right_foot")};
            for (int i = 0; i < std::min(sk->Count(), n); ++i)
                for (int j = sk->BoneAt(i).parent; j >= 0; j = sk->BoneAt(j).parent)
                    if (j == feet[0] || j == feet[1]) { s_belowFeet.push_back(i); break; }
        }
        for (int b : s_belowFeet) {
            const Bone& bone = sk->BoneAt(b);
            float* t = reinterpret_cast<float*>(boneBuf + b * 48 + VRIK_TRANS_OFF);
            float* r = reinterpret_cast<float*>(boneBuf + b * 48 + VRIK_ROT_OFF);
            t[0] = bone.refLocalPos.x; t[1] = bone.refLocalPos.y; t[2] = bone.refLocalPos.z;
            r[0] = bone.refLocalRot.x; r[1] = bone.refLocalRot.y; r[2] = bone.refLocalRot.z; r[3] = bone.refLocalRot.w;
            VRIK_MarkLegOwned(b);
        }
    }
    VRIK_ComputeFK(boneBuf, VRIK_FKCount());   // the port's arm solve reads the moved shoulders
    g_bodySolved = spine;

    if (now - g.lastLog >= 2000) {
        const float seconds = g.lastLog ? (now - g.lastLog) / 1000.0f : 2.0f;
        g.lastLog = now;
        Log("[fbt] body rate: %.0f solves/s, %.0f with a new device sample, %.0f repeating one\n", g.solves / seconds,
            g.newSamples / seconds, g.sameSample / seconds);
        g.solves = g.newSamples = g.sameSample = 0;
        const Goal& h = goals[kGoalHips];
        const Goal& lf = goals[kGoalLeftFoot];
        if (spine) {
            const Vec3 cam{camModelPos[0], camModelPos[1], camModelPos[2]};
            const Vec3 portHead = goals[kGoalHead].pos;
            const Vec3 fwd = Normalize(fwdGame), right{fwd.y, -fwd.x, 0};
            auto rel = [&](Vec3 p, const char* name) {   // forward / right / up of the camera
                const Vec3 d = p - cam;
                Log("[fbt]   %-22s from the camera: %+5.1f cm forward %+5.1f cm right %+5.1f cm up\n", name, Dot(d, fwd) * 100,
                    Dot(d, right) * 100, d.z * 100);
            };
            Log("[fbt] spine: camera at V's eyes %.3f m (the pose pass saw it at %.3f m); head goal missed by %.1f mm, spine reach "
                "%.2f, %d passes\n", eyeCal.z, camModelPos[2], dbg.spineError * 1000, dbg.spineReach, dbg.spineIterations);
            rel(goals[kGoalHead].pos, "head goal (V's eyes on it)");
            rel(portHead, "the port's head");
            if (cameraBone >= 0 && cameraBone < n) rel(portCameraBone, "V's camera bone (port)");
            rel(goals[kGoalHips].pos, "hips goal");
        }
        Log("[fbt] body: hips goal (%+.3f %+.3f %+.3f) gave way %.1f cm; left foot goal (%+.3f %+.3f %+.3f); legs reach %.2f %.2f; "
            "camera (%+.3f %+.3f %+.3f) yaw %+.0f deg, headset yaw in the calibration %+.0f deg\n", h.pos.x, h.pos.y, h.pos.z,
            Length(dbg.hipsShift) * 100, lf.pos.x, lf.pos.y, lf.pos.z, dbg.legReach[0], dbg.legReach[1], camModelPos[0], camModelPos[1],
            camModelPos[2], YawOf(fwdGame) * 57.29578f, YawOf(fwdCal) * 57.29578f);
    }
}

bool SolveArm(uint8_t* boneBuf, int side, const float target[3], const float handRot[4]) {
    if (!boneBuf || side < 0 || side > 1 || !g_bodySolved || !ArmsEnabled() || !g.stackReady || !g.skeleton) return false;
    const Skeleton* sk = g.skeleton;
    const int n = std::min(sk->Count(), std::min(VRIK_FKCount(), VRIK_MAX_BONES));
    Pose pose = sk->ReferencePose();
    for (int i = 0; i < n; ++i) {
        const float* t = reinterpret_cast<const float*>(boneBuf + i * 48 + VRIK_TRANS_OFF);
        const float* r = reinterpret_cast<const float*>(boneBuf + i * 48 + VRIK_ROT_OFF);
        pose.localPos[i] = {t[0], t[1], t[2]};
        pose.localRot[i] = {r[0], r[1], r[2], r[3]};
    }
    sk->Fk(pose);
    Goal goals[kGoalCount];
    Goal& hand = goals[side == 0 ? kGoalLeftHand : kGoalRightHand];
    hand.active = true;
    hand.pos = {target[0], target[1], target[2]};
    hand.rot = Normalize(Quat{handRot[0], handRot[1], handRot[2], handRot[3]});
    StackOptions o;
    o.setHips = o.legs = o.spine = o.hipsGiveWay = false;   // the body is solved already: only this arm
    o.arms = true;
    o.armScale[0] = g_armScale[0];
    o.armScale[1] = g_armScale[1];
    StackDebug dbg;
    g.stack.Solve(pose, goals, o, &dbg);
    const char* roles[2][3] = {{"left_upperarm", "left_forearm", "left_hand"}, {"right_upperarm", "right_forearm", "right_hand"}};
    int bones[4];
    bones[1] = sk->Role(roles[side][0]);
    bones[0] = bones[1] >= 0 ? sk->BoneAt(bones[1]).parent : -1;   // the clavicle
    bones[2] = sk->Role(roles[side][1]);
    bones[3] = sk->Role(roles[side][2]);
    auto write = [&](int b) {
        if (b < 0 || b >= n) return;
        float* t = reinterpret_cast<float*>(boneBuf + b * 48 + VRIK_TRANS_OFF);
        float* r = reinterpret_cast<float*>(boneBuf + b * 48 + VRIK_ROT_OFF);
        t[0] = pose.localPos[b].x; t[1] = pose.localPos[b].y; t[2] = pose.localPos[b].z;
        r[0] = pose.localRot[b].x; r[1] = pose.localRot[b].y; r[2] = pose.localRot[b].z; r[3] = pose.localRot[b].w;
    };
    for (int b : bones) write(b);
    // Where the solve put the wrist, as VRIK_SolveArm publishes it: the palm the belt, the holders and the reach measure
    // from. Unwritten, it stayed where the port's own solve last left it, the moment the arms came here.
    if (bones[3] >= 0 && bones[3] < n) {
        const Vec3 wrist = pose.modelPos[bones[3]];
        float* pin = g_VRIKHandPin[side == 1 ? 0 : 1];
        pin[0] = wrist.x; pin[1] = wrist.y; pin[2] = wrist.z;
    }

    // THE TWIST HELPERS. The forearm's skin hangs on three helpers along it (l/r_Wrist_0..2_JNT, a third, two thirds and
    // the end of the forearm), which the game turns with the wrist: the hand's twist about the forearm's axis (against
    // V's rest pose) goes to each by its share of the forearm's length -- in game, with the hand's whole rotation on the
    // wrist and the helpers still, the hand turned and the forearm did not. The helpers carry only skin: the hand keeps
    // its pose. The upper arm's (l/r_SHL_0..1_JNT) and the forearm's sit along bones the arm scale shortens: their
    // offsets scale with it, so the skin stays on the shortened arm.
    const int fore = bones[2], handBone = bones[3];
    float twistDeg = 0;
    if (fore >= 0 && fore < n && handBone >= 0 && handBone < n) {
        const Bone& hb = sk->BoneAt(handBone);
        const Vec3 axis = Normalize(hb.refLocalPos);   // the forearm's axis in its own frame (+x left, -x right)
        const Quat local = Normalize(Conjugate(pose.modelRot[fore]) * pose.modelRot[handBone]);
        const Quat delta = Normalize(local * Conjugate(hb.refLocalRot));
        float twist = 2.0f * std::atan2(Dot({delta.x, delta.y, delta.z}, axis), delta.w);
        twist = std::remainder(twist, 6.2831853f);
        twistDeg = twist * 57.29578f;
        const float k = std::clamp(o.armScale[side] * o.armLengthTune, 0.5f, 2.0f);
        const float handLen = std::max(Length(hb.refLocalPos), 1e-3f);
        const char* pre = side == 0 ? "l_" : "r_";
        char name[32];
        for (int w = 0; w < 3; ++w) {
            sprintf_s(name, "%sWrist_%d_JNT", pre, w);
            const int b = sk->Find(name);
            if (b < 0 || b >= n || sk->BoneAt(b).parent != fore) continue;
            const Bone& wb = sk->BoneAt(b);
            const float share = std::clamp(Length(wb.refLocalPos) / handLen, 0.0f, 1.0f);
            pose.localPos[b] = wb.refLocalPos * k;
            pose.localRot[b] = Normalize(AxisAngle(axis, twist * share) * wb.refLocalRot);
            write(b);
        }
        for (int h = 0; h < 2; ++h) {
            sprintf_s(name, "%sSHL_%d_JNT", pre, h);
            const int b = sk->Find(name);
            if (b < 0 || b >= n || sk->BoneAt(b).parent != bones[1]) continue;
            pose.localPos[b] = sk->BoneAt(b).refLocalPos * k;
            write(b);
        }
    }
    VRIK_ComputeFK(boneBuf, VRIK_FKCount());   // what follows in the pass (palm, cigarette, holsters) reads the solved hand
    static ULONGLONG s_lastLog[2]{};
    const ULONGLONG now = GetTickCount64();
    if (now - s_lastLog[side] >= 2000) {
        s_lastLog[side] = now;
        Log("[fbt] %s arm: reach %.2f (x %.3f), clavicle %.1f deg, elbow dir (%+.2f %+.2f %+.2f), wrist twist %+.0f deg\n",
            side == 0 ? "left" : "right", dbg.armReach[side], o.armScale[side], dbg.clavicleDeg[side], dbg.elbowDir[side].x,
            dbg.elbowDir[side].y, dbg.elbowDir[side].z, twistDeg);
    }
    return true;
}

}  // namespace cvr::fbt::game
