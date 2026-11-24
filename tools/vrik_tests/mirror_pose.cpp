#include "Anim/MirrorPose.hpp"
#include "Anim/MirrorArms.hpp"
#include <cstdint>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <stdexcept>
#include <string>
#include <vector>

using namespace cvr::mirror;
namespace fs = std::filesystem;

static void Check(bool ok, const char* what) {
    if (!ok) throw std::runtime_error(what);
}

static bool Near(float a, float b) { return std::abs(a - b) < 1e-4f; }

static bool SameRotation(XrQuaternionf a, XrQuaternionf b) {
    return Near(std::abs(a.x*b.x + a.y*b.y + a.z*b.z + a.w*b.w), 1.f);
}

static auto Bytes(Local& pose) { return reinterpret_cast<unsigned char*>(&pose); }
static auto Bytes(const Local& pose) { return reinterpret_cast<const unsigned char*>(&pose); }

static void TrackedIndex() {
    Local open{{.04f,0,0},0,{0,0,0,1},{1,1,1,1}};
    Local mirror{{.035f,0,0},7,{0,0,-.34202015f,.93969262f},{.95f,1.02f,1,1}};
    const auto original = mirror;
    Check(MatchingFingerFrames(open, mirror, false), "different rest curls should be compatible");
    Check(!MatchingFingerFrames(open, mirror, true), "different metacarpal frames matched");
    mirror.position.x *= -1;
    Check(!MatchingFingerFrames(open, mirror, false), "reversed longitudinal axis matched");
    mirror = original;
    auto finger = Bind(0, Motion::Rotation, ReadRest(Bytes(open), nullptr), ReadRest(Bytes(mirror), nullptr));
    for (float angle : {0.f, -.4f, -1.5f, 0.f}) {
        open.rotation = {0,0,std::sin(angle*.5f),std::cos(angle*.5f)};
        Apply(finger, Bytes(open), Bytes(mirror));
        Check(!SameRotation(open.rotation, mirror.rotation), "fixture lost the extra rest curl");
        CopyFingerRotation(Bytes(open), Bytes(mirror));
        Check(SameRotation(open.rotation, mirror.rotation), "tracked index acquired extra curl or twist");
        Check(std::memcmp(&original, &mirror, 16) == 0 && std::memcmp(original.scale, mirror.scale, 16) == 0,
              "tracked index changed shape");
    }
    open.rotation = {};
    CopyFingerRotation(Bytes(open), Bytes(mirror));
    Check(SameRotation(mirror.rotation, {0,0,0,1}), "invalid tracked pose changed finger");
}

static void DifferentProportions() {
    constexpr float h = .70710678f;
    Local sourceParent{{},0,{0,0,h,h},{1,1,1,1}};
    Local targetParent{{},0,{0,0,0,1},{1,1,1,1}};
    auto a = ReadRest(Bytes(sourceParent), nullptr);
    auto b = ReadRest(Bytes(targetParent), nullptr);
    Local source{{.35f,0,1},0,{0,0,0,1},{1,1,1,1}};
    Local target{{.21f,.03f,.9f},7,{h,0,0,h},{.95f,1.02f,1.15f,1}};
    auto from = ReadRest(Bytes(source), &a);
    auto to = ReadRest(Bytes(target), &b);
    auto shoulder = Bind(4, MotionFor("LeftShoulder"), from, to);
    auto hips = Bind(2, MotionFor("Hips"), from, to);
    const auto original = target;
    Apply(shoulder, Bytes(source), Bytes(target));
    Check(SameRotation(target.rotation, original.rotation), "rest should produce the reflection's rest rotation");
    source.position.x += .2f;
    source.rotation = {0,h,0,h};
    Apply(shoulder, Bytes(source), Bytes(target));
    Check(std::memcmp(&target, &original, 16) == 0, "shoulder width or height changed");
    Check(std::memcmp(target.scale, original.scale, sizeof(target.scale)) == 0, "shoulder scale changed");
    // Source parent turns its local +Y twist into a model-space -X twist,
    // cancelling the destination's +X rest rotation.
    Check(SameRotation(target.rotation, {0,0,0,1}), "shoulder moved in the wrong reference frame");
    Apply(hips, Bytes(source), Bytes(target));
    Check(Near(target.position.x,.21f) && Near(target.position.y,.23f) && Near(target.position.z,.9f),
          "hips did not displace relative to the reflection's rest position");
    const auto once = target;
    Apply(hips, Bytes(source), Bytes(target));
    Check(std::memcmp(&target, &once, sizeof(target)) == 0, "root motion accumulated across frames");
    source.rotation = {};
    Apply(hips, Bytes(source), Bytes(target));
    Check(std::memcmp(&target, &once, sizeof(target)) == 0, "invalid source changed the target");
    Check(!ReadRest(Bytes(source), nullptr).valid && Bind(2, Motion::Rotation, {}, to).source < 0,
          "invalid reference pose should disable transfer");
}

template<class T>
static std::vector<T> ReadFixture(const fs::path& path) {
    std::ifstream file(path, std::ios::binary | std::ios::ate);
    const std::streamoff size = file.tellg();
    Check(size > 0 && size % sizeof(T) == 0, "missing or truncated fixture");
    std::vector<T> data(static_cast<size_t>(size) / sizeof(T));
    file.seekg(0);
    Check(bool(file.read(reinterpret_cast<char*>(data.data()), size)), "fixture read failed");
    return data;
}

static void RecordedRig(const fs::path& dir) {
    const auto reference = ReadFixture<Local>(dir / "reference_full.bin");
    auto animated = ReadFixture<Local>(dir / "player_local.bin");
    auto parents = ReadFixture<int16_t>(dir / "player_parents.bin");
    Check(animated.size() == reference.size() && parents.size() == reference.size(), "rig sizes differ");
    std::ifstream names(dir / "player_names.txt");
    std::vector<Rest> rest;
    rest.reserve(reference.size());
    int motionCount = 0, nativeCount = 0;
    for (size_t i = 0; i < reference.size(); ++i) {
        size_t index;
        std::string name;
        Check(bool(names >> index >> name) && index == i, "missing bone name");
        const int p = parents[i];
        Check(p >= -1 && p < static_cast<int>(i), "invalid fixture hierarchy");
        rest.push_back(ReadRest(Bytes(reference[i]), p < 0 ? nullptr : &rest[p]));
        const auto motion = MotionFor(name);
        Local dest = reference[i];
        dest.position.x += .12f;
        dest.scale[1] *= .9f;
        const auto before = dest;
        const auto binding = Bind(static_cast<int>(i), motion, rest[i], rest[i]);
        Apply(binding, Bytes(animated[i]), Bytes(dest));
        if (motion == Motion::Native) {
            Check(std::memcmp(&dest, &before, sizeof(dest)) == 0, "native joint was overwritten");
            ++nativeCount;
        } else {
            Check(binding.source >= 0 && SameRotation(dest.rotation, animated[i].rotation), "tracked rotation lost");
            Check(std::memcmp(dest.scale, before.scale, sizeof(dest.scale)) == 0, "tracked joint lost scale");
            if (motion == Motion::Rotation)
                Check(std::memcmp(&dest, &before, 16) == 0, "tracked joint lost its position");
            ++motionCount;
        }
    }
    Check(motionCount > 60 && nativeCount > 400, "unexpected tracked joint coverage");
    for (auto name : {"face_root_JNT","mid_J_jaw_JNT","LeftEye","l_biceps_A_mscl_JNT"})
        Check(MotionFor(name) == Motion::Native, "game must retain face and muscle controls");
    for (auto name : {"Head","Neck","LeftArm","RightHand","LeftHandIndex3","shadow_RightHand"})
        Check(MotionFor(name) == Motion::Rotation, "head or hand lost tracking");
    std::cout << motionCount << " tracked joints, " << nativeCount << " native joints\n";
}

// Model-space samples from a 662-bone player and a 555-bone reflection,
// reduced to their common arm ancestors and converted back to local poses.
static void RecordedWrists(const fs::path& file) {
    using namespace DirectX;
    std::ifstream input(file);
    constexpr int count = 14;
    std::array<Local, count> rigs[2]{};
    int16_t parents[count];
    const char* names[] = {"Root","Hips","Spine","Spine1","Spine2","Spine3",
        "LeftShoulder","LeftArm","LeftForeArm","LeftHand","RightShoulder","RightArm","RightForeArm","RightHand"};
    for (auto& rig : rigs) {
        for (int i = 0; i < count; ++i) {
            auto& v = rig[i];
            std::string name;
            input >> name >> parents[i] >> v.position.x >> v.position.y >> v.position.z >>
                v.rotation.x >> v.rotation.y >> v.rotation.z >> v.rotation.w >> v.scale[0] >> v.scale[1] >> v.scale[2];
            Check(bool(input) && name == names[i], "invalid paired arm capture");
        }
    }
    auto* player = Bytes(rigs[0][0]);
    auto* mirror = Bytes(rigs[1][0]);
    const auto untouched = rigs[1];
    for (const int wrist : {9, 13}) {
        const auto chain = ArmPath(parents, count, wrist, wrist - 1, wrist - 2);
        arm::Model tracked[4], before[4], after[4];
        Check(arm::Read(chain, player, tracked) && arm::Read(chain, mirror, before), "invalid captured chain");
        Check(arm::Length(before[0].position - tracked[0].position) > .2f, "capture no longer reproduces the offset");
        Check(PlaceWrist(chain, player, chain, mirror) && arm::Read(chain, mirror, after), "wrist solve failed");
        const float error = arm::Length(after[0].position - tracked[0].position);
        Check(error < .001f, "reflected wrist missed its tracked XYZ position");
        Check(std::abs(XMVectorGetX(XMQuaternionDot(before[0].rotation, after[0].rotation))) > .99999f,
              "wrist orientation changed with elbow correction");
        Check(arm::Length(after[2].position - before[2].position) < .00001f, "shoulder moved");
        Check(PlaceWrist(chain, player, chain, mirror) && arm::Read(chain, mirror, after) &&
              arm::Length(after[0].position - tracked[0].position) < .001f, "repeat solve drifted");
        Check(PlaceWrist(chain, player, chain, mirror, true) && arm::Read(chain, mirror, after) &&
              arm::Length(after[0].position - tracked[0].position) < .001f &&
              std::abs(XMVectorGetX(XMQuaternionDot(tracked[0].rotation, after[0].rotation))) > .99999f,
              "tracked palm did not match the player's wrist position and orientation");
        // Deliberately cross the body, then reach beyond arm length. The target
        // is a position, so crossing is allowed and unreachable targets clamp.
        for (float d : {.15f, 1.5f}) {
            const auto goal = before[2].position + XMVectorSet(wrist == 9 ? d : -d, .1f, 0, 0);
            XMFLOAT3 delta;
            XMStoreFloat3(&delta, goal - tracked[0].position);
            const auto root = rigs[0][0].position;
            rigs[0][0].position = {root.x+delta.x, root.y+delta.y, root.z+delta.z};
            Check(PlaceWrist(chain, player, chain, mirror) && arm::Read(chain, mirror, after), "reach solve failed");
            const float reach = arm::Length(before[1].position-before[2].position) +
                                arm::Length(before[0].position-before[1].position);
            const auto aim = goal-before[2].position;
            const auto expected = before[2].position + XMVector3Normalize(aim)*std::min(arm::Length(aim),reach);
            Check(arm::Length(expected-after[0].position) < .001f, "cross-body or extended reach failed");
            rigs[0][0].position = root;
        }
    }
    for (int i = 0; i < count; ++i) {
        Check(std::memcmp(&rigs[1][i], &untouched[i], 16) == 0 &&
              std::memcmp(rigs[1][i].scale, untouched[i].scale, sizeof(Local::scale)) == 0, "arm resized");
    }
}

int main(int argc, char** argv) {
    try {
        Check(argc == 2 || argc == 3, "expected rig fixture directory and optional wrist capture");
        DifferentProportions();
        TrackedIndex();
        RecordedRig(argv[1]);
        RecordedWrists(argc == 3 ? fs::path(argv[2]) : fs::path(argv[1]) / "mirror_arms.txt");
        return 0;
    } catch (const std::exception& e) {
        std::cerr << e.what() << '\n';
        return 1;
    }
}
