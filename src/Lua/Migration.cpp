#include <stdafx.h>

#include "Migration.hpp"

#include "Utils.h"

#include "Core/Paths.hpp"

#include <algorithm>
#include <fstream>
#include <iterator>

// The plugin's log, one line (src/Core/Log.cpp).
extern "C" void CvrLogText(const char* text);

namespace cvr::lua {
namespace {

namespace fs = std::filesystem;

template <typename... Args> void PluginLog(fmt::format_string<Args...> aFormat, Args&&... aArgs)
{
    CvrLogText(fmt::format(aFormat, std::forward<Args>(aArgs)...).c_str());
}

bool SameBytes(const fs::path& a, const fs::path& b)
{
    std::error_code error;
    if (fs::file_size(a, error) != fs::file_size(b, error) || error)
        return false;
    std::ifstream x(a, std::ios::binary), y(b, std::ios::binary);
    return std::equal(std::istreambuf_iterator<char>(x), std::istreambuf_iterator<char>(), std::istreambuf_iterator<char>(y),
                      std::istreambuf_iterator<char>());
}

// CET's own per-mod files: its log (<mod>.log, rotated <mod>.N.log; the plugin's host writes its own) and the mod's
// sqlite database (db.sqlite3; the host has no db).
bool IsCetOwn(const fs::path& aRelative)
{
    return aRelative.extension() == L".log" || aRelative.filename() == L"db.sqlite3";
}

bool MoveOver(const fs::path& aFrom, const fs::path& aTo)
{
    std::error_code error;
    fs::create_directories(aTo.parent_path(), error);
    fs::rename(aFrom, aTo, error);
    if (!error)
        return true;
    // Another volume: copied, then removed.
    error.clear();
    fs::copy_file(aFrom, aTo, fs::copy_options::overwrite_existing, error);
    return !error && fs::remove(aFrom, error);
}

void RetireMod(const fs::path& aCetMod, const fs::path& aOwn, const fs::path& aLegacy)
{
    std::error_code error;
    std::vector<fs::path> files, dirs;
    for (const auto& entry : fs::recursive_directory_iterator(aCetMod, error))
    {
        if (entry.is_directory(error))
            dirs.push_back(entry.path());
        else
            files.push_back(entry.path());
    }
    int deleted = 0, adopted = 0, kept = 0, stuck = 0;
    for (const fs::path& file : files)
    {
        const fs::path relative = fs::relative(file, aCetMod, error);
        const fs::path own = aOwn / relative;
        bool done;
        if (fs::exists(own, error) && SameBytes(file, own))
        {
            done = fs::remove(file, error);
            deleted += done;
        }
        else if (!fs::exists(own, error) && !IsCetOwn(relative))
        {
            done = MoveOver(file, own);
            adopted += done;
        }
        else if (relative.extension() != L".lua" && !IsCetOwn(relative))
        {
            // Data the mod rewrites (its settings) over the shipped default: the player's copy is the one used.
            done = MoveOver(own, aLegacy / relative) && MoveOver(file, own);
            adopted += done;
        }
        else
        {
            done = MoveOver(file, aLegacy / relative);
            kept += done;
        }
        stuck += !done;
    }
    // Deepest first: the mod's own folder last.
    dirs.insert(dirs.begin(), aCetMod);
    for (auto it = dirs.rbegin(); it != dirs.rend(); ++it)
        if (fs::is_empty(*it, error))
            fs::remove(*it, error);
    PluginLog("[lua] {} taken out of CET's mods: {} deleted (same as the plugin's), {} of the player's (written at run "
              "time) moved in, {} kept in legacy\\cet{}\n",
              UTF16ToUTF8(aCetMod.filename().native()), deleted, adopted, kept,
              stuck ? fmt::format(", {} NOT MOVED (CET runs it too)", stuck) : std::string());
}

// VKBindings' encoding: the first key in the top 16 bits.
constexpr uint64_t kHome = static_cast<uint64_t>(VK_HOME) << 48;
constexpr uint64_t kScrollLock = static_cast<uint64_t>(VK_SCROLL) << 48;
constexpr uint64_t kPause = static_cast<uint64_t>(VK_PAUSE) << 48;

uint64_t KeyBesideCet(const uint64_t aCetKey)
{
    return aCetKey == kScrollLock ? kPause : kScrollLock;
}

// The "overlay_key" of a bindings file's "cet" entry; nullptr when the JSON has none.
nlohmann::json* OverlayKeyEntry(nlohmann::json& aBindings)
{
    if (!aBindings.is_object())
        return nullptr;
    const auto cet = aBindings.find("cet");
    if (cet == aBindings.end() || !cet->is_object())
        return nullptr;
    const auto key = cet->find("overlay_key");
    return key != cet->end() && key->is_number_unsigned() ? &*key : nullptr;
}

nlohmann::json ReadJson(const fs::path& aPath)
{
    if (std::ifstream in{aPath})
        return nlohmann::json::parse(in, nullptr, false);
    return nlohmann::json::object();
}

uint64_t ReadCetOverlayKey(const fs::path& aCetBindings)
{
    auto cet = ReadJson(aCetBindings);
    const auto* key = OverlayKeyEntry(cet);
    return key ? key->get<uint64_t>() : 0;
}

void SeedBindings(const fs::path& aCetBindings, const fs::path& aBindings, const uint64_t aCetKey,
                  const std::vector<std::string>& acMods)
{
    std::error_code error;
    if (fs::exists(aBindings, error))
        return;
    nlohmann::json cet = ReadJson(aCetBindings);
    if (cet.is_discarded() || !cet.is_object())
        cet = nlohmann::json::object();

    const uint64_t overlayKey = aCetKey ? KeyBesideCet(aCetKey) : kHome;

    nlohmann::json own = nlohmann::json::object();
    own["cet"]["overlay_key"] = overlayKey;
    int binds = 0;
    for (const std::string& mod : acMods)
    {
        if (const auto it = cet.find(mod); it != cet.end() && it->is_object())
        {
            own[mod] = *it;
            binds += static_cast<int>(it->size());
        }
    }
    fs::create_directories(aBindings.parent_path(), error);
    std::ofstream(aBindings) << own.dump(4);
    PluginLog("[lua] key binds made: overlay key {:#x}, {} mod bind(s) from CET's bindings.json\n", overlayKey, binds);
}

// Bindings made by 0.2.0 hold CET's key, or Home when CET came later. Another key was chosen on purpose: it stays.
void MoveOverlayKeyOffCet(const fs::path& aBindings, const uint64_t aCetKey)
{
    if (!aCetKey)
        return;
    auto own = ReadJson(aBindings);
    auto* key = OverlayKeyEntry(own);
    if (!key)
        return;
    const uint64_t current = key->get<uint64_t>();
    if (current != aCetKey && current != kHome)
        return;
    const uint64_t moved = KeyBesideCet(aCetKey);
    *key = moved;
    std::ofstream(aBindings) << own.dump(4);
    PluginLog("[lua] overlay key {:#x} -> {:#x}: CET's own overlay keeps its key and its input\n", current, moved);
}

}  // namespace

uint64_t MigrateFromCet()
{
    std::vector<wchar_t> exe(32768);
    const DWORD length = GetModuleFileNameW(nullptr, exe.data(), static_cast<DWORD>(exe.size()));
    const fs::path cetRoot = fs::path(std::wstring(exe.data(), length)).parent_path() / L"plugins" / L"cyber_engine_tweaks";
    const fs::path root = fs::path(paths::Dir(paths::Folder::Root)).parent_path();
    const fs::path mods = fs::path(paths::Dir(paths::Folder::Lua)).parent_path();

    std::error_code error;
    std::vector<std::string> names;
    for (const auto& entry : fs::directory_iterator(mods, error))
    {
        if (!entry.is_directory(error))
            continue;
        names.push_back(UTF16ToUTF8(entry.path().filename().native()));
        const fs::path cetMod = cetRoot / L"mods" / entry.path().filename();
        if (fs::is_directory(cetMod, error))
            RetireMod(cetMod, entry.path(), root / L"legacy" / L"cet" / entry.path().filename());
    }
    const fs::path cetBindings = cetRoot / L"bindings.json";
    const fs::path bindings = fs::path(paths::Dir(paths::Folder::Config)).parent_path() / L"lua_bindings.json";
    const uint64_t cetKey = ReadCetOverlayKey(cetBindings);
    SeedBindings(cetBindings, bindings, cetKey, names);
    MoveOverlayKeyOffCet(bindings, cetKey);
    return cetKey;
}

}  // namespace cvr::lua
