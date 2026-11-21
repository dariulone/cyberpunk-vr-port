#include "Runtimes/ProximityGrip.hpp"
#include "Runtimes/CyberFingerInput.hpp"
#include "Runtimes/FingerGrip.hpp"
#include "Fbt/Game/Trackers.hpp"
#include "Fbt/Game/ExtendedBody.hpp"
#include "Fbt/Game/StickGait.hpp"
#include <array>
#include <fstream>
#include <iostream>
#include <limits>
#include <stdexcept>
#include <string>
#include <vector>

using namespace wrpuppets::fbt;
void Check(bool ok, const char* reason) { if (!ok) throw std::runtime_error(reason); }
bool Near(Vec3 a, Vec3 b) { return Length(a - b) < 1e-4f; }
bool SameRotation(Quat a, Quat b) {
    return Near(Rotate(a, {1,0,0}), Rotate(b, {1,0,0})) && Near(Rotate(a, {0,1,0}), Rotate(b, {0,1,0}));
}

void Grabs() {
    cvr::cyberfinger::ProximityGrip grip;
    Check(!grip.Update(false, true, 0), "invalid startup grabs");
    Check(!grip.Update(true, true, 1), "closed hand on startup grabs");
    Check(!grip.Update(true, false, 10), "opening grabs");
    Check(grip.Update(true, true, 20), "deliberate close did not grab");
    Check(grip.Update(true, false, 30), "one open frame drops");
    Check(grip.Update(true, true, 80), "brief finger glitch drops");
    Check(grip.Update(false, false, 120), "brief tracking loss drops");
    Check(grip.Update(true, true, 160), "brief reacquisition drops");
    Check(!grip.Update(false, true, 410), "long loss keeps stale grip");
    Check(!grip.Update(true, true, 420), "reacquisition with fist grabs");
    Check(!grip.Update(true, false, 430), "rearm grabs");
    Check(grip.Update(true, true, 440), "close after rearm fails");
    Check(grip.Update(true, false, 450), "release grace absent");
    Check(!grip.Update(true, false, 550), "deliberate opening does not release");
    Check(grip.Update(true, true, 560), "second grab fails");
    Check(!grip.Update(true, true, 1000), "focus gap regrabs without opening");
    grip.Update(true,false,1010);
    Check(!grip.Update(true,true,1020,false),"using a control started a fist grab");
    Check(!grip.Update(true,true,1030),"releasing a control with fingers closed started a grab");
    grip.Update(true,false,1040);
    Check(grip.Update(true,true,1050),"fresh nearby grab after control use failed");
    Check(grip.Update(true,true,1060,false),"firing trigger dropped held object");
    const float relaxed[4]{.45f,.55f,.60f,.62f}, fist[4]{.9f,.95f,.91f,.94f};
    Check(!cvr::fingergrip::Update(false, relaxed) && cvr::fingergrip::Update(false, fist), "fist thresholds regress");
}

void Buttons() {
    cvr::cyberfinger::Actions actions;
    int reads = 0;
    Check(actions.Buttons([&](XrAction) { ++reads; return true; }) == 0 && reads == 0, "disabled profile reads handles");
    std::vector<std::string> names;
    actions.Create([&](XrAction& a, XrActionType type, const char* name, const char*, bool perHand) {
        Check(type == XR_ACTION_TYPE_BOOLEAN_INPUT && !perHand, "utility action scope/type");
        names.emplace_back(name);
        a = reinterpret_cast<XrAction>(static_cast<uintptr_t>(names.size()));
    });
    Check(names == std::vector<std::string>{"cf_inventory", "cf_weapon", "cf_scanner", "cf_heal", "cf_settings"}, "binding names changed");
    const uint16_t bits[]{0x20, 0x8000, 0x100, 0x1,0x0400};
    for (unsigned mask = 0; mask < 32; ++mask) {
        uint16_t expected = 0;
        for (int i = 0; i < 5; ++i) if (mask & (1u << i)) expected |= bits[i];
        const auto actual = actions.Buttons([&](XrAction a) { return (mask & (1u << (reinterpret_cast<uintptr_t>(a) - 1))) != 0; });
        Check(actual == expected, "button alias or missing action");
    }
    actions = {};
    Check(!actions.Buttons([](XrAction) { return true; }), "session teardown keeps handles");
    Check(cvr::trackers::kRoleCount == 8 && cvr::trackers::kRightFoot == 2, "tracker role ABI changed");
}

void Handedness() {
    // Match the public snapshot fields used by the production mapper without
    // pulling Direct3D into this executable. The real XInput consumer is compiled
    // by the adapters target as well.
    struct State {
        uint16_t buttons = 0, cyberFingerButtons = 0;
        float leftTrigger = .9f, rightTrigger = .2f;
        float leftGrip = .8f, rightGrip = .1f;
        float leftThumbX = .3f, leftThumbY = .7f, rightThumbX = -.4f, rightThumbY = -.6f;
        bool leftHandValid = true, rightHandValid = false;
    };
    State raw;
    raw.buttons = 0x1000 | 0x2000 | 0x40; // physical A, B, left stick click
    raw.cyberFingerButtons = 0x8000 | 0x20 | 0x100 | 0x1;
    const auto right = cvr::cyberfinger::ForGame(raw, false, 2);
    Check(right.rightTrigger == .2f && right.leftThumbY == .7f, "right-handed input changed");
    Check(right.buttons == (raw.buttons | raw.cyberFingerButtons), "right-handed utility merge failed");
    const auto left = cvr::cyberfinger::ForGame(raw, true, 0);
    Check(left.rightTrigger == .9f && left.leftTrigger == .2f, "left trigger does not fire");
    Check(left.rightGrip == .8f && left.leftGrip == .1f, "grab roles did not follow weapon hand");
    Check(left.rightHandValid && !left.leftHandValid, "tracking validity did not follow weapon hand");
    Check(left.leftThumbY == .7f && left.rightThumbX == -.4f, "left-handed preset swapped locomotion");
    Check(left.buttons == right.buttons, "left-handed preset changed physical buttons");
    const auto mirrored = cvr::cyberfinger::ForGame(raw, true, 2);
    Check(mirrored.leftThumbY == -.6f && mirrored.rightThumbY == .7f, "optional stick mirror failed");
    for (int mode = 0; mode <= 2; ++mode) {
        for (uint16_t utility : {uint16_t(0x8000), uint16_t(0x20), uint16_t(0x100), uint16_t(0x1)}) {
            State one; one.cyberFingerButtons = utility;
            Check(cvr::cyberfinger::ForGame(one, true, mode).buttons == utility,
                  "named utility action was mirrored (weapon wheel must not eject a magazine)");
        }
        State physical; physical.buttons = 0x8000;
        Check(cvr::cyberfinger::ForGame(physical, true, mode).buttons == (mode == 0 ? 0x8000 : 0x2000),
              "ordinary physical face button stopped obeying the selected mirror mode");
    }
    Check(raw.buttons == (0x1000 | 0x2000 | 0x40) && raw.leftTrigger == .9f, "mapper mutated physical snapshot");
    State captured{}; captured.leftTrigger = captured.rightTrigger = 0;
    Check(cvr::cyberfinger::ForGame(captured, true, 0).buttons == 0, "cleared overlay input emits utility actions");
    captured.cyberFingerButtons=cvr::cyberfinger::SettingsButton;
    Check(cvr::cyberfinger::ForGame(captured,true,0).buttons==0,"private settings action leaked to gamepad");
    // Same physical binding, both weapon hands and every optional mirror mode.
    // Test the downstream merge too: eject must never become weapon switch,
    // reload must never become jump, and pause must never become vehicle exit.
    for(bool weaponLeft:{false,true})for(int mode=0;mode<=2;++mode) {
        const uint16_t expected[]={uint16_t(weaponLeft?0x2000:0x4000),
            uint16_t(weaponLeft?0x4000:0x2000),uint16_t(weaponLeft?0x0010:0x0001),
            uint16_t(weaponLeft?0x0001:0x0010)};
        for(int control=0;control<4;++control) {
            State wrist;
            wrist.cyberFingerButtons=cvr::cyberfinger::WristButtons(weaponLeft,
                control==0,control==1,control==2,control==3);
            Check(cvr::cyberfinger::ForGame(wrist,weaponLeft,mode).buttons==expected[control],
                "black wrist role changed with mirror mode or weapon hand");
        }
        Check(cvr::cyberfinger::WristButtons(weaponLeft,true,true,true,true)==0x0011,
            "black holds also ejected or reloaded");
        Check(cvr::cyberfinger::WristButtons(weaponLeft,false,false,false,false)==0,
            "released black wrist generated input");
    }
}

Skeleton Rig() {
    // Independent synthetic skeleton, with a bent arm/leg and a five-joint spine.
    // Unit rotations make its expected positions and lengths explicit.
    std::ofstream file("test-rig.txt");
    file << "bone 0 -1 root 0 0 0 0 0 0 1 0 0 0 0 0 0 1\n"
         << "bone 1 0 middle 1 1 0 0 0 0 1 1 1 0 0 0 0 1\n"
         << "bone 2 1 end 1 -1 0 0 0 0 1 2 0 0 0 0 0 1\n"
         << "bone 3 -1 spine0 0 0 0 0 0 0 1 0 0 0 0 0 0 1\n"
         << "bone 4 3 spine1 0 .1 .2 0 0 0 1 0 .1 .2 0 0 0 1\n"
         << "bone 5 4 chest 0 -.1 .2 0 0 0 1 0 0 .4 0 0 0 1\n"
         << "bone 6 5 neck 0 .1 .2 0 0 0 1 0 .1 .6 0 0 0 1\n"
         << "bone 7 6 head 0 -.1 .2 0 0 0 1 0 0 .8 0 0 0 1\n";
    file.close();
    Skeleton sk;
    Check(sk.Load("test-rig.txt"), "synthetic rig failed to load");
    return sk;
}

void Body() {
    using namespace cvr::fbt::extended;
    const Skeleton sk = Rig();
    for (int degrees = -180; degrees <= 180; degrees += 15) {
        auto pose = sk.ReferencePose();
        const Vec3 root = pose.modelPos[0], tip = pose.modelPos[2];
        const Quat orientation = AxisAngle({0,1,0}, .4f);
        SetModel(sk, pose, 2, tip, orientation); sk.Fk(pose);
        const Vec3 pole = Rotate(AxisAngle({1,0,0}, degrees * .01745329252f), {1,1,0});
        Check(ApplyPole(sk, pose, 0, 1, 2, pole), "valid pole rejected");
        Check(Near(root, pose.modelPos[0]) && Near(tip, pose.modelPos[2]), "pole moves limb endpoint");
        Check(SameRotation(orientation, pose.modelRot[2]), "pole rotates hand/foot");
        Check(Near(pole, pose.modelPos[1]), "joint does not follow pole");
        Check(std::abs(Length(pose.modelPos[1] - root) - std::sqrt(2.f)) < 1e-4f, "pole stretches limb");
    }
    auto pose = sk.ReferencePose();
    Check(!ApplyPole(sk, pose, 0, 1, 2, {1,0,0}), "singular pole accepted");
    Check(!ApplyPole(sk, pose, 0, 1, 2, {std::numeric_limits<float>::quiet_NaN(),0,0}), "NaN pole accepted");
    Check(!ApplyPole(sk, pose, 0, 3, 2, {1,1,1}), "unrelated bones accepted");
    Chain lower, upper;
    Check(MakeChain(sk, 3, 5, lower) && MakeChain(sk, 5, 7, upper), "valid spine chains rejected");
    Chain wrong;
    Check(!MakeChain(sk, 0, 7, wrong), "disconnected chain accepted");
    Goal chest{true, {.04f,0,.4f}, {}}, head{true, {0,0,.8f}, {}};
    Check(ApplyChest(sk, pose, lower, upper, chest, head), "reachable chest rejected");
    Check(Length(pose.modelPos[5] - chest.pos) < .025f && Length(pose.modelPos[7] - head.pos) < .005f, "chest/head targets missed");
    const auto before = pose;
    chest.pos = {10,10,10};
    Check(!ApplyChest(sk, pose, lower, upper, chest, head), "unreachable chest accepted");
    for (int i = 0; i < sk.Count(); ++i) Check(Near(before.modelPos[i], pose.modelPos[i]), "fallback changed pose");
    chest.active = false;
    Check(!ApplyChest(sk, pose, lower, upper, chest, head), "absent tracker overrides spine");
}

void StickSteps() {
    cvr::fbt::game::StickGait gait;
    Vec3 player{}, feet[2]{{-.1f,0,0},{.1f,0,0}};
    constexpr float dt = 1.f / 90;
    auto advance = [&](float speed, bool stick, bool lifted = false) {
        player.x += speed * dt; // strafe in world space
        gait.Advance(dt, player, feet, stick, lifted);
    };
    for (int i = 0; i < 90; ++i) advance(3.f, false);
    for (int i = 0; i < 90; ++i) { advance(0.f, false); }
    for (int i = 0; i < 90; ++i) advance(0.f, true);
    Check(Near(gait.offset[0], {}) && Near(gait.offset[1], {}), "blocked stick or room-scale motion generated steps");
    bool leftSwing = false, rightSwing = false;
    for (int i = 0; i < 180; ++i) {
        advance(3.f, true);
        leftSwing |= gait.offset[0].z > .06f && gait.offset[1].z < .01f;
        rightSwing |= gait.offset[1].z > .06f && gait.offset[0].z < .01f;
        Check(std::fabs(gait.offset[0].x + gait.offset[1].x) < 1e-4f && gait.offset[0].y == 0.f, "stride ignores direction");
    }
    Check(leftSwing && rightSwing && std::fabs(gait.offset[0].x) > .02f, "feet did not alternate during a stick run");
    for (int i = 0; i < 60; ++i) advance(3.f, true, true);
    Check(Near(gait.offset[0], {}) && Near(gait.offset[1], {}), "held physical step lost priority");
    for (int i = 0; i < 90; ++i) { feet[0].y += dt * .5f; advance(-3.f, true); }
    Check(Near(gait.offset[0], {}) && Near(gait.offset[1], {}), "physical foot motion doubled with the stride");
    for (int i = 0; i < 120; ++i) advance(-3.f, true);
    player.x += 15.f;
    advance(3.f, true);
    Check(Near(gait.offset[0], {}) && Near(gait.offset[1], {}), "teleport generated a stride");
    for (int i = 0; i < 90; ++i) advance(4.f, true);
    for (int i = 0; i < 90; ++i) advance(0.f, false);
    Check(Near(gait.offset[0], {}) && Near(gait.offset[1], {}), "stick release did not settle onto trackers");
}

int main() try {
    StickSteps();
    using namespace cvr::cyberfinger;
    Check(StickActionFullTravel(-.8467f,true,.8f),"measured CF full-back travel cannot crouch");
    Check(!StickActionFullTravel(-.3693f,true,.8f),"ordinary diagonal turn activated crouch");
    Check(!StickActionFullTravel(-.79f,true,.8f) && StickActionFullTravel(-.8f,true,.8f),"CF threshold boundary wrong");
    Check(!StickActionFullTravel(.98f,true,.8f) && StickActionFullTravel(.98f,false,.8f),"CF dash/crouch directions inverted");
    Check(!StickActionFullTravel(-.85f,true,.9f),"custom CF threshold ignored");
    Check(!StickActionFullTravel(std::numeric_limits<float>::quiet_NaN(),true,.8f),"invalid axis activated crouch");
    Check(SanitizeStickActionThreshold(.1f)==.8f && SanitizeStickActionThreshold(1.f)==.8f,"unsafe CF threshold accepted");
    Grabs(); Buttons(); Handedness(); Body();
    std::cout << "PASS grab loss/release/rearm, optional actions, both weapon hands and mirror modes, 8 roles, 25 limb directions, chest/head targets and fallback\n";
    return 0;
} catch (const std::exception& e) { std::cerr << e.what() << '\n'; return 1; }
