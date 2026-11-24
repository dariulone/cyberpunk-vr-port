#include "Fbt/Game/CalibrationProfile.hpp"
#include <windows.h>
#include <charconv>
#include <cmath>
#include <fstream>
#include <iomanip>
#include <limits>
#include <locale>
#include <map>
#include <string>
#include <string_view>

namespace cvr::fbt::game {
namespace {
using namespace wrpuppets::fbt;
constexpr const char* kDeviceKeys[kCalCount] = {"waist", "left_foot", "right_foot", "left_hand", "right_hand"};
using Fields = std::map<std::string, std::string>;

template<class... Values>
bool Numbers(const Fields& fields, const std::string& key, Values&... values) {
    const auto it = fields.find(key);
    if (it == fields.end()) return false;
    std::string_view rest = it->second;
    size_t count = 0;
    auto read = [&](float& value) {
        const auto end = rest.data() + rest.size();
        const auto parsed = std::from_chars(rest.data(), end, value);
        if (parsed.ec != std::errc{} || !std::isfinite(value)) return false;
        if (++count == sizeof...(values)) return parsed.ptr == end;
        if (parsed.ptr == end || *parsed.ptr != ',') return false;
        rest.remove_prefix(parsed.ptr - rest.data() + 1);
        return true;
    };
    return (read(values) && ...);
}

bool Rotation(const Fields& fields, const std::string& key, Quat& q) {
    if (!Numbers(fields, key, q.x, q.y, q.z, q.w)) return false;
    const float lengthSquared = q.x*q.x + q.y*q.y + q.z*q.z + q.w*q.w;
    return std::abs(lengthSquared - 1.0f) < 0.001f;
}

bool Offset(const Fields& fields, const std::string& key, Vec3& v) {
    return Numbers(fields, key, v.x, v.y, v.z);
}

void Write(std::ostream& out, const std::string& key, const Vec3& v) {
    out << key << '=' << v.x << ',' << v.y << ',' << v.z << '\n';
}
void Write(std::ostream& out, const std::string& key, const Quat& q) {
    out << key << '=' << q.x << ',' << q.y << ',' << q.z << ',' << q.w << '\n';
}
}  // namespace

ProfileRead LoadCalibrationProfile(const std::filesystem::path& path, CalibrationProfile& profile) {
    std::ifstream in(path);
    if (!in) {
        std::error_code ec;
        return !std::filesystem::exists(path, ec) && !ec ? ProfileRead::Missing : ProfileRead::Invalid;
    }
    Fields fields;
    for (std::string line; std::getline(in, line);) {
        if (line.empty() || line.front() == ';') continue;
        const auto pos = line.find('=');
        if (pos == std::string::npos || !fields.emplace(line.substr(0, pos), line.substr(pos + 1)).second)
            return ProfileRead::Invalid;
    }
    CalibrationProfile loaded{};
    auto& c = loaded.body;
    const bool legacy = fields["version"] == "2";
    if (in.bad() || (!legacy && (fields["version"] != "3" || fields["complete"] != "1")) ||
        !Numbers(fields, "scale", c.scale) || c.scale <= 0 ||
        !Numbers(fields, "player_eye_height", c.playerEyeHeight) || c.playerEyeHeight < 0.5f ||
        !Numbers(fields, "v_eye_height", c.avatarEyeHeight) || c.avatarEyeHeight <= 0 ||
        !Offset(fields, "origin", c.origin) || !Rotation(fields, "yaw_inverse", c.yawInverse) ||
        !Rotation(fields, "head_rot_inverse", c.headRotInverse) ||
        !Numbers(fields, "arm_length", c.armLength[0], c.armLength[1]) || c.armLength[0] < 0 || c.armLength[1] < 0)
        return ProfileRead::Invalid;
    for (int i = 0; i < kCalCount; ++i) {
        const std::string offset = std::string(kDeviceKeys[i]) + "_offset";
        const std::string rotation = std::string(kDeviceKeys[i]) + "_rot_inverse";
        if (!fields.count(offset) && !fields.count(rotation)) continue;
        if (!Offset(fields, offset, c.offset[i]) || !Rotation(fields, rotation, c.rotInverse[i])) return ProfileRead::Invalid;
        c.valid[i] = true;
    }
    if (!c.valid[kCalLeftFoot] || !c.valid[kCalRightFoot]) return ProfileRead::Invalid;
    auto& chest = loaded.chest;
    if (!legacy && (fields.count("chest_offset") || fields.count("chest_rot_inverse") || fields.count("chest_ref_rot"))) {
        if (!Offset(fields, "chest_offset", chest.offset) || !Rotation(fields, "chest_rot_inverse", chest.rotationInverse) ||
            !Rotation(fields, "chest_ref_rot", chest.referenceRotation)) return ProfileRead::Invalid;
        chest.valid = true;
    }
    c.done = true;
    profile = loaded;
    return ProfileRead::Loaded;
}

bool SaveCalibrationProfile(const std::filesystem::path& path, const CalibrationProfile& profile) {
    const auto& c = profile.body;
    if (!c.done) return false;
    auto temporary = path;
    temporary += L"." + std::to_wstring(GetCurrentProcessId()) + L".tmp";
    std::ofstream out(temporary, std::ios::trunc);
    if (!out) return false;
    out.imbue(std::locale::classic());
    out << std::setprecision(std::numeric_limits<float>::max_digits10);
    out << "; Full-body tracking calibration. Room = STAGE in V's model axes, metres.\nversion=3\n";
    out << "scale=" << c.scale << "\nplayer_eye_height=" << c.playerEyeHeight << "\nv_eye_height=" << c.avatarEyeHeight << '\n';
    Write(out, "origin", c.origin);
    Write(out, "yaw_inverse", c.yawInverse);
    Write(out, "head_rot_inverse", c.headRotInverse);
    out << "arm_length=" << c.armLength[0] << ',' << c.armLength[1] << '\n';
    for (int i = 0; i < kCalCount; ++i) {
        if (!c.valid[i]) continue;
        Write(out, std::string(kDeviceKeys[i]) + "_offset", c.offset[i]);
        Write(out, std::string(kDeviceKeys[i]) + "_rot_inverse", c.rotInverse[i]);
    }
    if (profile.chest.valid) {
        Write(out, "chest_offset", profile.chest.offset);
        Write(out, "chest_rot_inverse", profile.chest.rotationInverse);
        Write(out, "chest_ref_rot", profile.chest.referenceRotation);
    }
    out << "complete=1\n";
    out.close();
    CalibrationProfile checked{};
    if (out && LoadCalibrationProfile(temporary, checked) == ProfileRead::Loaded &&
        MoveFileExW(temporary.c_str(), path.c_str(), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)) return true;
    DeleteFileW(temporary.c_str());
    return false;
}

}  // namespace cvr::fbt::game
