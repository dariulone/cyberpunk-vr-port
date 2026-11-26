#include "Anim/TwoHandGrip.hpp"
#include "Anim/CharacterRig.hpp"
#include "Anim/VrikHook.hpp"
#include <array>
#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <stdexcept>
#include <thread>
#include <vector>

using Vec = std::array<float, 3>;
using Quat = std::array<float, 4>;
constexpr Quat identity{0, 0, 0, 1};
struct Pose { Vec pos{}; Quat rot = identity; };

// Boundaries of the pose solve: a fixed XR frame, an equipped item, the rig's
// hand reflection, and an already sampled recoil spring. Grip selection,
// steering, release, quaternion math and both pose outputs are production code.
extern bool g_hasWeaponEquipped, testWeaponInLeftHand;
volatile int g_VRBodyCarryingState = -1;
static bool dual{};
static uint64_t weaponRevision = 1;
static float shotRise{}, shotBack{};
extern "C" {
char CyberpunkVR_WeaponName[64] = "grad";
extern int CyberpunkVR_WeaponClass;
int CyberpunkVR_VrikBodyCarryHandOff = 0;
extern int CyberpunkVR_TwoHandActive, CyberpunkVR_TwoHandAim, CyberpunkVR_TwoHandSuppress;
extern float CyberpunkVR_TwoHandWeight, CyberpunkVR_DebugTwoHandAimDeg, CyberpunkVR_DebugTwoHandDist;
extern float CyberpunkVR_TwoHandBreakRadius, CyberpunkVR_TwoHandBreakMs;
void RecoilSample(int side, int primary, float* back, float* rise) {
    if (side != 1 || !primary) std::abort();
    *back = shotBack;
    *rise = shotRise;
}
void RecoilTravelMix(float* back, float* down) { *back = 1; *down = 0; }
}
uint64_t PrimaryWeaponRevision() { return weaponRevision; }
namespace cvr::anim {
bool TwoGunsInHand() { return dual; }
bool MirrorFingerToRight(int, const float*, int*, float*) { return false; }
void MirrorHeldHand(float* offset, float*) {
    for (int i = 0; i < 3; ++i) offset[i] = -offset[i];
}
}

static void check(bool ok, const char* message) {
    if (!ok) throw std::runtime_error(message);
}
static float distance(const Vec& a, const Vec& b) {
    return std::hypot(a[0]-b[0], a[1]-b[1], a[2]-b[2]);
}
static void checkRotation(const Quat& a, const Quat& b) {
    float dot = 0;
    for (int i = 0; i < 4; ++i) dot += a[i] * b[i];
    check(std::abs(std::abs(dot)-1) < 1e-5f, "wrist orientation failed to follow the shot");
}

void TestWeaponFollow(bool weaponLeft) {
    static float hands[166]{};
    g_pSharedHands = hands;
    testWeaponInLeftHand = weaponLeft;
    g_hasWeaponEquipped = true;
    CyberpunkVR_WeaponClass = 4;
    CyberpunkVR_TwoHandSuppress = 0x1ffc; // offer only the primary Grad attachment in this fixture
    cvr::anim::TwoHandTick();
    // Orient the shipped Grad wrist capture along the controller's forward axis.
    const float sign = weaponLeft ? 1.0f : -1.0f;
    const Quat wrist{0, 0, sign * .70710678f, .70710678f};
    Vec support{};
    float& grip = hands[weaponLeft ? 49 : 155];
    auto solve = [&] {
        Pose gun;
        cvr::anim::SolveWeaponHandPose(gun.pos.data(), gun.rot.data(), wrist.data(), support.data(), identity.data());
        return gun;
    };
    auto fullWeld = [&] {
        // Sample the endpoint of the normal blend without a wall-clock wait.
        const float weight = CyberpunkVR_TwoHandWeight;
        CyberpunkVR_TwoHandWeight = 1;
        Pose pose;
        check(cvr::anim::TwoHandLeft(pose.pos.data(), pose.rot.data()), "support pose missing");
        CyberpunkVR_TwoHandWeight = weight;
        return pose;
    };

    solve();
    support = fullWeld().pos;
    grip = 1;
    solve();
    const Pose rest = fullWeld();
    std::printf("Grad primary at rest=(%.6f, %.6f, %.6f) reach=%.6f active=%d\n",
                rest.pos[0], rest.pos[1], rest.pos[2], CyberpunkVR_DebugTwoHandDist, CyberpunkVR_TwoHandActive);
    check(CyberpunkVR_TwoHandActive && distance(rest.pos, Vec{}) > .20f, "Grad hold did not engage");

    // A dash or Kerenzikov can leave one pose pass without a fresh grip sample. The closed hold
    // stays on the weapon through that gap, while an explicit reload-owned wrist still wins.
    const int supportHand = weaponLeft ? 1 : 0;
    g_pSharedHands[vrshared::kReloadOwnedHand] = -1.0f;
    g_VRReloadFingerActive[supportHand] = 0;
    grip = 0;
    solve();
    check(CyberpunkVR_TwoHandActive, "one missing grip sample released the two-hand hold");
    std::this_thread::sleep_for(std::chrono::milliseconds(70));
    solve();
    check(CyberpunkVR_TwoHandActive, "short grip gap released the two-hand hold");
    std::this_thread::sleep_for(std::chrono::milliseconds(80));
    solve();
    check(!CyberpunkVR_TwoHandActive, "a sustained grip release stayed latched");
    grip = 1;
    solve();
    check(CyberpunkVR_TwoHandActive, "fresh grip did not re-engage after a real release");
    g_VRReloadFingerActive[supportHand] = 1;
    g_pSharedHands[vrshared::kReloadOwnedHand] = -1.0f;
    solve();
    check(CyberpunkVR_TwoHandActive, "reload preview stole an active two-hand hold");
    g_pSharedHands[vrshared::kReloadOwnedHand] = static_cast<float>(supportHand);
    solve();
    check(!CyberpunkVR_TwoHandActive, "an explicitly owned reload wrist did not release the hold");
    g_VRReloadFingerActive[supportHand] = 0;
    g_pSharedHands[vrshared::kReloadOwnedHand] = -1.0f;
    grip = 0;
    solve();
    grip = 1;
    solve();
    check(CyberpunkVR_TwoHandActive, "two-hand hold did not recover after reload ownership ended");
    // Tighten the release radius to expose a synthetic hand displacement on
    // a single solve. Normal controller movement must still release the hold.
    CyberpunkVR_TwoHandBreakRadius = .02f;
    CyberpunkVR_TwoHandBreakMs = 0;
    for (int aim : {1, 2}) {
        CyberpunkVR_TwoHandAim = aim;
        for (float rise : {.21f, .08f, -.015f, 0.f}) {
            shotRise = rise;
            shotBack = .2f * rise;
            const Pose gun = solve();
            const Pose drawn = fullWeld();
            const Quat kick{std::sin(rise / 2), 0, 0, std::cos(rise / 2)};
            Vec expected;
            VRIK_QuatRotateVec(kick.data(), rest.pos.data(), expected.data());
            expected[1] -= shotBack;
            Quat expectedRot;
            VRIK_QuatMul(kick.data(), rest.rot.data(), expectedRot.data());
            std::printf("aim=%d rise=%.3f reach=%.6f steer=%.5f supportTravel=%.6f active=%d\n",
                        aim, rise, CyberpunkVR_DebugTwoHandDist, CyberpunkVR_DebugTwoHandAimDeg,
                        distance(rest.pos, drawn.pos), CyberpunkVR_TwoHandActive);
            check(CyberpunkVR_TwoHandActive && CyberpunkVR_DebugTwoHandDist < 1e-5f &&
                  CyberpunkVR_DebugTwoHandAimDeg < 1e-4f, "shot fed back into grip input");
            check(distance(gun.pos, Vec{0, -shotBack, 0}) < 1e-5f, "weapon travel lost");
            check(distance(drawn.pos, expected) < 1e-5f, "support offset sheared or kicked twice");
            checkRotation(gun.rot, kick);
            checkRotation(drawn.rot, expectedRot);
        }
    }
    CyberpunkVR_TwoHandAim = 1;
    CyberpunkVR_TwoHandBreakRadius = .60f;
    support[2] += .08f;
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(1);
    do {
        std::this_thread::sleep_for(std::chrono::milliseconds(5));
        solve();
    } while (CyberpunkVR_DebugTwoHandAimDeg < .5f && std::chrono::steady_clock::now() < deadline);
    std::printf("support input +0.080 m: reach=%.6f steer=%.4f\n",
                CyberpunkVR_DebugTwoHandDist, CyberpunkVR_DebugTwoHandAimDeg);
    check(CyberpunkVR_TwoHandActive && std::abs(CyberpunkVR_DebugTwoHandDist - .08f) < 1e-5f &&
          CyberpunkVR_DebugTwoHandAimDeg >= .5f, "moving support failed to steer");
    CyberpunkVR_TwoHandBreakRadius = .02f;
    support[2] += .15f;
    solve();
    check(!CyberpunkVR_TwoHandActive, "moving the real controller failed to break the grip");
    support = rest.pos;
    solve();
    check(CyberpunkVR_TwoHandActive, "support failed to return within reach");
    ++weaponRevision; // different instance, same Grad family
    solve();
    check(!CyberpunkVR_TwoHandActive && CyberpunkVR_TwoHandWeight == 0, "old grip survived replacement");
    grip = 0;
    solve();
    grip = 1;
    solve();
    check(CyberpunkVR_TwoHandActive, "new instance refused fresh grip");
    dual = true;
    solve();
    Pose other;
    check(!CyberpunkVR_TwoHandActive && !cvr::anim::TwoHandLeft(other.pos.data(), other.rot.data()),
          "second independent gun inherited support");
    dual = false;
    solve();
    check(!CyberpunkVR_TwoHandActive, "dual exit reused a held squeeze");
    hands[weaponLeft ? 155 : 49] = 1;
    testWeaponInLeftHand = !weaponLeft;
    solve();
    support = fullWeld().pos;
    solve();
    check(!CyberpunkVR_TwoHandActive, "rehand inherited a squeeze from the opposite controller");
}

// Rest and peak poses in model space from the central Grad controls. The
// left-weapon stop is the post-stop IK goal in take-054712. The right-weapon
// control in take-054115 supplies its measured solved support wrist.
struct FollowSample { Pose weapon; Vec attachment, stop; };
static constexpr FollowSample leftTake[] = {
    {{{-.09540442f,.57363951f,1.55768585f},{-.45451948f,-.54167527f,.45451960f,.54167515f}},
     {-.05067754f,.80071640f,1.61125076f},{-.05029887f,.78505838f,1.60604668f}},
    {{{-.12623851f,.56144994f,1.57720375f},{-.03475213f,-.60185641f,.52298099f,.60253799f}},
     {-.08151160f,.78249633f,1.65184665f},{-.04895610f,.78154576f,1.60464096f}}
};
static constexpr FollowSample rightTake[] = {
    {{{.09450018f,.37136859f,1.55988503f},{.56472063f,-.42554748f,-.56472051f,.42554760f}},
     {.04819337f,.62478828f,1.64790165f},{.04819602f,.62472969f,1.64790154f}},
    {{{.09452298f,.33057117f,1.55988503f},{.58195710f,-.40165406f,-.58195698f,.40165418f}},
     {.04821618f,.57578486f,1.66869092f},{.04823637f,.57584065f,1.66869164f}}
};

void TestSupportWrist(bool weaponLeft, uint8_t* native, int bones) {
    const auto& samples = weaponLeft ? leftTake : rightTake;
    const int side = weaponLeft ? 1 : 0; // support side, physical numbering
    const int supportBone = weaponLeft ? g_VRRightBoneIdx : g_VRLeftBoneIdx;
    const int primaryBone = weaponLeft ? g_VRLeftBoneIdx : g_VRRightBoneIdx;
    const int fore = g_VRBoneParent[supportBone], upper = g_VRBoneParent[fore];
    testWeaponInLeftHand = weaponLeft;
    g_hasWeaponEquipped = true;
    CyberpunkVR_WeaponClass = 4;
    const int hold = weaponLeft ? 3 : 0; // Grad rear/primary in the two recordings
    CyberpunkVR_TwoHandSuppress = 0x1ffe & ~(2 << hold);
    g_pSharedHands[vrshared::kReloadOwnedHand] = -1;
    cvr::anim::TwoHandTick();
    VRIK_ComputeFK(native, bones);
    float right[3], up[3], forward[3];
    check(VRIK_BodyAxesFromRig(right,up,forward), "body axes unavailable");
    auto separation = [](const float* a, const float* b) {
        return std::hypot(a[0]-b[0],a[1]-b[1],a[2]-b[2]);
    };
    const float foreLength = separation(g_fkPos[supportBone],g_fkPos[fore]);
    const float upperLength = separation(g_fkPos[fore],g_fkPos[upper]);
    const Vec offset = weaponLeft ? Vec{.232928455f,-.0133197078f,-.0447269586f}
                                  : Vec{-.267863303f,.0147551894f,.0463067591f};
    const Vec shoulder{g_fkPos[upper][0],g_fkPos[upper][1],g_fkPos[upper][2]};
    // Isolate the measured stop-stage loss, then replay the measured weapon
    // wrist itself so its final rotation and reach clipping drive the grip.
    for (int take=0;take<3;++take) {
        const bool replayAttachment=take==0;
        const float reach=take==2 ? 1.25f : .24f;
        Vec shift;
        for (int k=0;k<3;++k) shift[k]=shoulder[k] + reach*forward[k] - samples[0].attachment[k];
        Vec previous{}, previousGoal{};
        for (int frame=0;frame<2;++frame) {
            std::vector<uint8_t> pose(native,native+48*bones);
            auto* buffer=pose.data();
            VRIK_ComputeFK(buffer,bones);
            Pose gun=samples[0].weapon;
            Vec ctrl=samples[0].attachment;
            for(int k=0;k<3;++k){gun.pos[k]+=shift[k];ctrl[k]+=shift[k];}
            g_pSharedHands[weaponLeft ? 49 : 155]=1;
            cvr::anim::SolveWeaponHandPose(gun.pos.data(),gun.rot.data(),identity.data(),ctrl.data(),identity.data());
            check(CyberpunkVR_TwoHandActive, "capture did not engage the selected Grad hold");
            auto solveArm=[&](bool left, const Pose& goal) {
                VRIK_SolveArm(buffer,left?g_VRLeftUpperArmIdx:g_VRRightUpperArmIdx,
                              left?g_VRLeftForeArmIdx:g_VRRightForeArmIdx,left?g_VRLeftBoneIdx:g_VRRightBoneIdx,
                              goal.pos.data(),goal.rot.data(),right,up,forward,0,1,left,false);
            };
            gun=samples[frame].weapon;
            for(int k=0;k<3;++k) gun.pos[k]+=shift[k];
            if (replayAttachment) {
                Vec arm;
                VRIK_QuatRotateVec(gun.rot.data(),offset.data(),arm.data());
                for(int k=0;k<3;++k) gun.pos[k]=samples[frame].attachment[k]+shift[k]-arm[k];
            }
            solveArm(weaponLeft,gun);
            check(cvr::anim::RebaseFirearmSupport(buffer,primaryBone), "attachment lost after primary solve");
            Vec goal;
            VRIK_QuatRotateVec(g_fkRot[primaryBone],offset.data(),goal.data());
            for(int k=0;k<3;++k) goal[k]+=g_fkPos[primaryBone][k];
            Pose support{ctrl,identity};
            g_VRHandStopValid[side]=1; g_VRHandStopDeadband=0;
            for(int k=0;k<3;++k)
                g_VRHandStopModel[side][k]=goal[k]+(samples[frame].stop[k]-samples[frame].attachment[k]);
            CyberpunkVR_TwoHandWeight=1;
            if (replayAttachment) {
                Pose old=support;
                check(cvr::anim::TwoHandLeft(old.pos.data(),old.rot.data()),"lost old-path control");
                VRIK_ApplyHandStop(side,old.pos.data(),old.rot.data());
                if(weaponLeft && frame)
                    check(distance(old.pos,goal)>.04f,"old stop ordering failed to reproduce the loss");
            }
            cvr::anim::ApplyFirearmSupportStop(side,support.pos.data(),support.rot.data());
            check(distance(support.pos,goal)<.00005f, "independent stop displaced the owned grip");
            for(int k=0;k<3;++k)
                check(std::abs(g_VRHandRawModel[side][k]-goal[k])<.00005f,"physics received the wrong support target");
            solveArm(!weaponLeft,support);
            VRIK_ComputeFK(buffer,bones);
            Vec drawn;
            std::copy_n(g_fkPos[supportBone],3,drawn.data());
            const float gap=distance(drawn,goal);
            const float travel=frame ? distance(drawn,previous) : 0.f;
            std::printf("take=%s reach=%.2f frame=%d goalTravelMm=%.3f wristTravelMm=%.3f gapMm=%.3f\n",
                        replayAttachment ? "recorded-attachment" : "recorded-weapon",reach,frame,
                        frame ? 1000*distance(goal,previousGoal) : 0.f,1000*travel,1000*gap);
            if(reach<1) {
                check(gap<.0001f && (!frame || travel>.01f), "reachable FK wrist missed its moving grip");
                if (replayAttachment && frame)
                    check(std::abs(travel-distance(samples[0].attachment,samples[1].attachment))<.0001f,
                          "final FK travel missed recorded attachment travel");
                Quat drawnRot;
                std::copy_n(g_fkRot[supportBone],4,drawnRot.data());
                checkRotation(drawnRot,support.rot);
            } else {
                check(gap>.01f, "unreachable wrist was forced onto the grip");
                check(separation(g_fkPos[fore],g_fkPos[supportBone])<=foreLength*1.15f+.0001f &&
                      separation(g_fkPos[fore],g_fkPos[upper])<=upperLength*1.15f+.0001f,"arm exceeded stretch limit");
            }
            previous=drawn;previousGoal=goal;
        }
    }
    g_pSharedHands[vrshared::kReloadOwnedHand]=static_cast<float>(side);
    check(!cvr::anim::RebaseFirearmSupport(native,primaryBone),"reload-owned support selected grip override");
}
