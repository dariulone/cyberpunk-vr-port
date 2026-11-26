// The test's adapter over the Rust settings (formerly src/Core/GameSettings.cpp's Rust-build half; the plugin calls
// rust/crates/cvr-settings directly): game_settings_tests_rust runs main.cpp's program through this, against
// cvr_rust.lib, and its trace is compared with the C++ reference's byte for byte.
#include "Core/GameSettings.hpp"

// The merge, the install, one option set and read: rust/crates/cvr-settings/src/game_settings.rs (the file read and
// written as jsoncpp did, rust/crates/cvr-settings/src/json.rs). This half turns the C ABI into the header's types.
#include <cstdint>
#include <cstring>

extern "C" {
struct CvrBuf { uint8_t* ptr; size_t len; };
struct CvrMergeOut { bool ok; uint32_t changed; CvrBuf json, error, skipped; };
struct CvrInstallOut { bool ok; uint32_t changed; CvrBuf backup, error, skipped; };
struct CvrSetOut { bool ok, changed; CvrBuf previous, error; };
void cvr_settings_buf_free(CvrBuf buf);
void cvr_game_settings_merge(const char* player, size_t playerLen, const char* preset, size_t presetLen, CvrMergeOut* out);
void cvr_game_settings_install(const wchar_t* preset, size_t presetLen, const wchar_t* player, size_t playerLen,
                               CvrInstallOut* out);
void cvr_game_settings_set(const wchar_t* player, size_t playerLen, const char* group, size_t groupLen, const char* name,
                           size_t nameLen, const char* value, size_t valueLen, int index, CvrSetOut* out);
void cvr_game_settings_player_file(CvrBuf* out);
void cvr_game_settings_get(const wchar_t* player, size_t playerLen, const char* group, size_t groupLen, const char* name,
                           size_t nameLen, CvrBuf* out);
}

namespace cvr::settings {
namespace {
// Each buffer taken into a C++ value and handed back to Rust.
std::string Take(CvrBuf buf) {
    std::string out(reinterpret_cast<const char*>(buf.ptr), buf.len);
    cvr_settings_buf_free(buf);
    return out;
}
std::wstring TakeWide(CvrBuf buf) {
    std::wstring out(buf.len / sizeof(wchar_t), L'\0');
    if (!out.empty()) std::memcpy(out.data(), buf.ptr, out.size() * sizeof(wchar_t));
    cvr_settings_buf_free(buf);
    return out;
}
std::vector<std::string> TakeList(CvrBuf buf) {
    std::vector<std::string> out;
    for (size_t at = 0; at + 4 <= buf.len;) {
        uint32_t length = 0;
        std::memcpy(&length, buf.ptr + at, 4);
        at += 4;
        out.emplace_back(reinterpret_cast<const char*>(buf.ptr + at), length);
        at += length;
    }
    cvr_settings_buf_free(buf);
    return out;
}
}  // namespace

MergeResult MergeGameSettings(std::string_view player, std::string_view preset) {
    CvrMergeOut out{};
    cvr_game_settings_merge(player.data(), player.size(), preset.data(), preset.size(), &out);
    MergeResult result;
    result.ok = out.ok;
    result.changed = out.changed;
    result.json = Take(out.json);
    result.error = Take(out.error);
    result.skipped = TakeList(out.skipped);
    return result;
}
InstallResult InstallGameSettings(const std::filesystem::path& preset, const std::filesystem::path& player) {
    CvrInstallOut out{};
    const auto& from = preset.native();
    const auto& to = player.native();
    cvr_game_settings_install(from.c_str(), from.size(), to.c_str(), to.size(), &out);
    InstallResult result;
    result.ok = out.ok;
    result.changed = out.changed;
    result.backup = TakeWide(out.backup);
    result.error = Take(out.error);
    result.skipped = TakeList(out.skipped);
    return result;
}
SetResult SetGameSetting(const std::filesystem::path& player, std::string_view group, std::string_view name,
                         std::string_view value, int index) {
    CvrSetOut out{};
    const auto& path = player.native();
    cvr_game_settings_set(path.c_str(), path.size(), group.data(), group.size(), name.data(), name.size(), value.data(),
                          value.size(), index, &out);
    SetResult result;
    result.ok = out.ok;
    result.changed = out.changed;
    result.previous = Take(out.previous);
    result.error = Take(out.error);
    return result;
}
std::filesystem::path PlayerSettingsFile() {
    CvrBuf out{};
    cvr_game_settings_player_file(&out);
    return std::filesystem::path(TakeWide(out));
}
std::string GetGameSetting(const std::filesystem::path& player, std::string_view group, std::string_view name) {
    CvrBuf out{};
    const auto& path = player.native();
    cvr_game_settings_get(path.c_str(), path.size(), group.data(), group.size(), name.data(), name.size(), &out);
    return Take(out);
}
}  // namespace cvr::settings
