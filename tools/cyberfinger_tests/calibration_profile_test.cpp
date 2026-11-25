#include "Fbt/Game/CalibrationProfile.hpp"
#include <windows.h>
#include <fstream>
#include <iostream>
#include <iterator>
#include <stdexcept>

using namespace cvr::fbt::game;
using namespace wrpuppets::fbt;
namespace fs = std::filesystem;

namespace {
void Check(bool ok, const char* message) {
    if (!ok) throw std::runtime_error(message);
}
bool Equal(Vec3 a, Vec3 b) { return a.x == b.x && a.y == b.y && a.z == b.z; }
bool Equal(Quat a, Quat b) { return a.x == b.x && a.y == b.y && a.z == b.z && a.w == b.w; }

void RoundTrip(const fs::path& path, const CalibrationProfile& profile) {
    Check(SaveCalibrationProfile(path, profile), "save failed");
    CalibrationProfile next{};
    Check(LoadCalibrationProfile(path, next) == ProfileRead::Loaded, "reload failed");
    const auto& a = profile.body;
    const auto& b = next.body;
    Check(b.done && a.scale == b.scale && a.playerEyeHeight == b.playerEyeHeight && a.avatarEyeHeight == b.avatarEyeHeight &&
          Equal(a.origin, b.origin) && Equal(a.yawInverse, b.yawInverse) && Equal(a.headRotInverse, b.headRotInverse) &&
          a.armLength[0] == b.armLength[0] && a.armLength[1] == b.armLength[1], "body or arm measurements changed");
    for (int i = 0; i < kCalCount; ++i)
        Check(a.valid[i] == b.valid[i] && (!a.valid[i] ||
              (Equal(a.offset[i], b.offset[i]) && Equal(a.rotInverse[i], b.rotInverse[i]))), "tracker changed");
    const auto& chest = profile.chest;
    Check(chest.valid == next.chest.valid && (!chest.valid || (Equal(chest.offset, next.chest.offset) &&
          Equal(chest.rotationInverse, next.chest.rotationInverse) && Equal(chest.referenceRotation, next.chest.referenceRotation))),
          "chest changed");
}
const std::string legacy =
    "; v2 calibration with optional devices absent and one unmeasured arm\n"
    "version=2\nscale=1\nplayer_eye_height=1.7\nv_eye_height=1.7\norigin=0.2,-0.1,0\n"
    "yaw_inverse=0,0,0,1\nhead_rot_inverse=0,0,0,1\narm_length=0,0.54\n"
    "left_foot_offset=0.1,0,0\nleft_foot_rot_inverse=0,0,0,1\n"
    "right_foot_offset=-0.1,0,0\nright_foot_rot_inverse=0,0,0,1\n";

void Test(const fs::path& dir, const fs::path& existing) {
    const auto file = dir / "profile.ini";
    CalibrationProfile p{};
    Check(LoadCalibrationProfile(file, p) == ProfileRead::Missing, "missing file activated");
    std::ofstream(file) << legacy;
    Check(LoadCalibrationProfile(file, p) == ProfileRead::Loaded, "v2 did not load");
    Check(!p.chest.valid && p.body.done && !p.body.valid[kCalWaist] && p.body.valid[kCalLeftFoot] &&
          p.body.valid[kCalRightFoot] && !p.body.valid[kCalLeftHand] && p.body.armLength[1] == 0.54f, "v2 migration");
    RoundTrip(file, p);
    p.chest.valid = true;
    p.chest.offset = {-0.14f, 0.09f, 0.22f};
    p.chest.rotationInverse = {0, 0.6f, 0, 0.8f};
    p.chest.referenceRotation = {0.6f, 0, 0.8f, 0};
    RoundTrip(file, p);
    auto bad = p;
    bad.chest.rotationInverse = {};
    bad.chest.rotationInverse.w = 0;
    Check(!SaveCalibrationProfile(file, bad), "invalid overwrite succeeded");
    std::ifstream saved(file);
    const std::string full{std::istreambuf_iterator<char>(saved), {}};
    saved.close();
    Check(LoadCalibrationProfile(file, bad) == ProfileRead::Loaded &&
          Equal(bad.chest.rotationInverse, p.chest.rotationInverse), "failed write damaged previous file");
    for (const auto& invalid : {full.substr(0, full.find("complete=")), legacy + "waist_offset=1,2,3\n",
            legacy.substr(0, legacy.find("right_foot_rot_inverse")), legacy + "scale=2\n",
            std::string(legacy).replace(legacy.find("scale=1"), 7, "scale=nan"),
            std::string(legacy).replace(legacy.find("version=2"), 9, "version=9"),
            std::string(legacy).replace(legacy.find("arm_length=0,0.54"), 17, "arm_length=0,1junk")}) {
        std::ofstream(file) << invalid;
        Check(LoadCalibrationProfile(file, p) == ProfileRead::Invalid && p.chest.valid && p.body.done,
              "invalid read accepted or changed active profile");
    }
    if (!existing.empty()) {
        Check(LoadCalibrationProfile(existing, p) == ProfileRead::Loaded, "existing on-disk profile failed to load");
        RoundTrip(file, p);  // write only to the test directory
        std::cout << "Existing calibration restored: height " << p.body.playerEyeHeight << ", arms "
                  << p.body.armLength[0] << '/' << p.body.armLength[1] << ", chest " << p.chest.valid << '\n';
    }
}
}  // namespace

int main(int argc, char** argv) {
    const auto dir = fs::current_path() / ("fbt-profile-test-" + std::to_string(GetCurrentProcessId()));
    if (!fs::create_directory(dir)) return 1;
    try {
        Test(dir, argc > 1 ? fs::path(argv[1]) : fs::path{});
    } catch (const std::exception& e) {
        std::cerr << e.what() << '\n';
        return 1;
    }
    fs::remove(dir / "profile.ini");
    fs::remove(dir);
    std::cout << "FBT profile tests passed\n";
}
