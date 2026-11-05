#pragma once
#include <filesystem>
#include <string>
#include <string_view>
#include <vector>

namespace cvr::settings {
// `skipped`: the preset's settings the player's file did not take -- not in this build of the game, or a value
// it would not read (GameSettings.cpp, Accepts).
struct MergeResult {bool ok{};unsigned changed{};std::string json,error;std::vector<std::string> skipped;};
struct InstallResult {bool ok{};unsigned changed{};std::filesystem::path backup;std::string error;
    std::vector<std::string> skipped;};
MergeResult MergeGameSettings(std::string_view player,std::string_view preset);
InstallResult InstallGameSettings(const std::filesystem::path& preset,const std::filesystem::path& player);
// One option's saved value and its list index, set in the player's settings file. The file is written
// (atomically) only when it holds something else; an option the file does not have is an error.
struct SetResult {bool ok{};bool changed{};std::string previous,error;};
SetResult SetGameSetting(const std::filesystem::path& player,std::string_view group,std::string_view name,
                         std::string_view value,int index);
// The player's settings file, %LOCALAPPDATA%\CD Projekt Red\Cyberpunk 2077\UserSettings.json -- the one the game reads
// its settings from as it starts; empty when there is no such folder.
std::filesystem::path PlayerSettingsFile();
// One option's saved value in the player's settings file, as text; empty when the file or the option is not there.
std::string GetGameSetting(const std::filesystem::path& player,std::string_view group,std::string_view name);
}
