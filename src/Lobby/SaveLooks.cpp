// VRLobbySaveLooks() -> String: the looks the player's playthroughs stand at, for the lobby's engrams
// (mods/cet/CyberpunkVRPort_Lobby/modules/engrams.lua) -- one per playthrough, from its newest save.
//
// A save folder's metadata.9.json names its playthrough (Data.metadata.playthroughID), life path and genders; its
// sav.dat holds the look in the node CharacetrCustomization_Appearances (Lobby/SaveFile.hpp reads the file without the
// game). That node, game 2.5 (save versions 263..277 read the same): u32 node index, 11 bytes, then groups -- head,
// arms, body, then data this does not need. A group: u32 count, then sections; a section: a string (its name: TPP, FPP,
// character_customization, ...), u32 count of (u64 hash, string appearance, string option, 8 bytes), u32 count of
// (string region, string morph, 8 bytes). The same option shows in several sections; its appearance is the same.
// Strings are the engine's (Lobby/SaveFile.hpp detail::Vlq).
//
// The text handed back, one record a line, fields split by tabs -- first every save, newest first (the lobby's list
// to load from), then the looks:
//   save  <save folder> <playthrough> <life path> <body gender> <level> <save time, unix seconds> <location LocKey>
//         <tracked quest LocKey> <play time, seconds> <made with mods: 1 / 0 (metadata isModded)>
//   look  <playthrough> <save folder> <life path> <body gender> <brain gender> <level> <save time, unix seconds>
//   app   <group 0 head / 1 arms / 2 body> <option> <appearance>
//   morph <group> <region> <morph>
//   wear  <equipment area> <item TweakDBID, 16 hex digits> <rng seed> <unique counter>
//   outfit <attachment slot TweakDBID, 16 hex> <item TweakDBID, 16 hex> <rng seed> <unique counter>
// Each look's app, morph, wear and outfit lines follow it; newest playthrough first.
//
// WHAT V WEARS is the equipment system's (Lobby/SavePackage.hpp: the node ScriptableSystemsContainer is a package of
// the scripted systems' data): the player's EquipmentSystemPlayerData, its equipment's equipAreas -- an area and
// its slots' items -- and clothingVisualsInfo, per area whether its item is hidden and the item shown in its place
// (the game's wardrobe). One wear line per clothing area that shows something. EquipmentEx (a mod's outfits) keeps
// its own: EquipmentEx.OutfitState, while m_active its m_parts (an item and the slot it goes in) are what shows.
#include "Lobby/SaveFile.hpp"
#include "Lobby/SavePackage.hpp"
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Scripting/Utils.hpp>
#include <json/json.h>
#include <windows.h>
#include <shlobj.h>
#include <algorithm>
#include <filesystem>
#include <map>
#include <mutex>
#include <set>
#include <sstream>
#pragma comment(lib, "shell32.lib")
#pragma comment(lib, "ole32.lib")

extern void Log(const char*, ...);

namespace cvr::lobby::looks {
namespace {
namespace fs = std::filesystem;

struct Look {
    std::string playthrough, folder, lifePath, bodyGender, brainGender;  // folder in UTF-8
    std::wstring folderPath;
    std::string location, quest;  // LocKeys
    double level{}, playTime{};
    int64_t time{};
    bool modded{};
    std::vector<std::string> lines;  // its app / morph lines
};

fs::path SaveRoot() {
    PWSTR games = nullptr;
    fs::path root;
    if (SUCCEEDED(SHGetKnownFolderPath(FOLDERID_SavedGames, 0, nullptr, &games)) && games)
        root = fs::path(games) / L"CD Projekt Red" / L"Cyberpunk 2077";
    CoTaskMemFree(games);
    return root;
}

std::string Utf8(const std::wstring& w) {
    if (w.empty()) return {};
    const int n = WideCharToMultiByte(CP_UTF8, 0, w.data(), static_cast<int>(w.size()), nullptr, 0, nullptr, nullptr);
    std::string s(static_cast<size_t>(std::max(n, 0)), '\0');
    WideCharToMultiByte(CP_UTF8, 0, w.data(), static_cast<int>(w.size()), s.data(), n, nullptr, nullptr);
    return s;
}

std::string Clean(std::string s) {
    for (auto& c : s)
        if (c == '\t' || c == '\n' || c == '\r') c = ' ';
    return s;
}

// The look out of the node's bytes, into `look.lines`; false when the bytes do not read as the layout above.
bool ReadLook(const uint8_t* d, size_t n, Look& look) {
    std::vector<uint8_t> v(d, d + n);
    size_t p = 15;
    bool narrow = false;
    auto u32 = [&](uint32_t& out) {
        if (p + 4 > v.size()) return false;
        out = save_file::detail::U32(&v[p]);
        p += 4;
        return true;
    };
    auto str = [&](std::string& out) {
        const size_t len = save_file::detail::Vlq(v, p, narrow);
        out.clear();
        if (narrow) {
            if (p + len > v.size()) return false;
            out.assign(reinterpret_cast<const char*>(&v[p]), len);
            p += len;
        } else {
            if (p + 2 * len > v.size()) return false;
            for (size_t c = 0; c < len; ++c) out.push_back(static_cast<char>(v[p + 2 * c]));
            p += 2 * len;
        }
        return true;
    };
    std::set<std::string> seen;
    for (int group = 0; group < 3; ++group) {
        uint32_t sections = 0;
        if (!u32(sections) || sections > 256) return false;
        for (uint32_t s = 0; s < sections; ++s) {
            std::string name, a, b;
            uint32_t apps = 0, morphs = 0;
            if (!str(name) || !u32(apps) || apps > 4096) return false;
            for (uint32_t k = 0; k < apps; ++k) {
                if (p + 8 > v.size()) return false;
                p += 8;
                if (!str(a) || !str(b) || p + 8 > v.size()) return false;
                p += 8;
                if (seen.insert("a" + b).second)
                    look.lines.push_back("app\t" + std::to_string(group) + "\t" + Clean(b) + "\t" + Clean(a));
            }
            if (!u32(morphs) || morphs > 4096) return false;
            for (uint32_t k = 0; k < morphs; ++k) {
                if (!str(a) || !str(b) || p + 8 > v.size()) return false;
                p += 8;
                if (seen.insert("m" + a).second)
                    look.lines.push_back("morph\t" + std::to_string(group) + "\t" + Clean(a) + "\t" + Clean(b));
            }
        }
    }
    return !look.lines.empty();
}

// A gameItemID struct: its TweakDBID, rng seed and unique counter.
struct Item {
    uint64_t id{};
    uint32_t seed{};
    uint16_t counter{};
};

Item ItemAt(const save_file::package::Package& pk, size_t at) {
    Item item;
    const auto props = pk.Props(at);
    if (const auto* p = pk.Find(props, "id")) item.id = pk.U64(p->at);
    if (const auto* p = pk.Find(props, "rngSeed")) item.seed = pk.U32(p->at);
    if (const auto* p = pk.Find(props, "uniqueCounter")) item.counter = pk.U16(p->at);
    return item;
}

std::string Hex(uint64_t v) {
    char b[20];
    snprintf(b, sizeof b, "%016llx", static_cast<unsigned long long>(v));
    return b;
}

std::string ItemLine(const Item& item) {
    return Hex(item.id) + "\t" + std::to_string(item.seed) + "\t" + std::to_string(item.counter);
}

// What V wears (the layout above) into `look.lines`.
bool ReadWear(const uint8_t* d, size_t n, Look& look) {
    using save_file::package::Package;
    Package pk;
    pk.isEnum = [](const std::string& t) {
        auto* rtti = RED4ext::CRTTISystem::Get();
        auto* type = rtti ? rtti->GetType(RED4ext::CName(t.c_str())) : nullptr;
        return type && type->GetType() == RED4ext::ERTTIType::Enum;
    };
    if (!pk.Open(d, n)) return false;
    static const std::set<std::string> clothing = {"Head", "Face", "OuterChest", "InnerChest", "Legs", "Feet",
                                                   "Outfit", "UnderwearTop", "UnderwearBottom"};
    bool found = false;
    for (size_t k = 0; k < pk.Chunks() && !found; ++k) {
        if (pk.ChunkType(k) != "EquipmentSystemPlayerData") continue;
        found = true;
        const auto props = pk.Props(pk.ChunkAt(k));
        // Per area: hidden, or another item shown in its place.
        std::map<std::string, std::pair<bool, Item>> visuals;
        if (const auto* info = pk.Find(props, "clothingVisualsInfo")) {
            for (size_t at : pk.Elements(info->type, info->at)) {
                const auto v = pk.Props(at);
                const auto* area = pk.Find(v, "areaType");
                if (!area) continue;
                const auto* hidden = pk.Find(v, "isHidden");
                const auto* item = pk.Find(v, "visualItem");
                visuals[pk.Name(pk.U16(area->at))] = {hidden && pk.U8(hidden->at) != 0,
                                                      item ? ItemAt(pk, item->at) : Item{}};
            }
        }
        const auto* equipment = pk.Find(props, "equipment");
        const auto loadout = equipment ? pk.Props(equipment->at) : std::vector<save_file::package::Prop>{};
        const auto* areas = pk.Find(loadout, "equipAreas");
        if (!areas) continue;
        for (size_t at : pk.Elements(areas->type, areas->at)) {
            const auto a = pk.Props(at);
            const auto* type = pk.Find(a, "areaType");
            const auto* slots = pk.Find(a, "equipSlots");
            if (!type || !slots) continue;
            const std::string area = pk.Name(pk.U16(type->at));
            if (!clothing.count(area)) continue;
            Item item;
            const auto elements = pk.Elements(slots->type, slots->at);
            if (!elements.empty())
                if (const auto* id = pk.Find(pk.Props(elements[0]), "itemID")) item = ItemAt(pk, id->at);
            const auto v = visuals.find(area);
            if (v != visuals.end()) {
                if (v->second.first) continue;
                if (v->second.second.id) item = v->second.second;
            }
            if (item.id) look.lines.push_back("wear\t" + area + "\t" + ItemLine(item));
        }
    }
    for (size_t k = 0; k < pk.Chunks(); ++k) {
        if (pk.ChunkType(k) != "EquipmentEx.OutfitState") continue;
        const auto props = pk.Props(pk.ChunkAt(k));
        const auto* active = pk.Find(props, "active");
        const auto* parts = pk.Find(props, "parts");
        if (!active || pk.U8(active->at) == 0 || !parts) break;
        for (size_t at : pk.Elements(parts->type, parts->at)) {
            const uint32_t chunk = pk.U32(at);
            if (chunk >= pk.Chunks()) continue;
            const auto part = pk.Props(pk.ChunkAt(chunk));
            const auto* item = pk.Find(part, "itemID");
            const auto* slot = pk.Find(part, "slotID");
            if (item && slot) look.lines.push_back("outfit\t" + Hex(pk.U64(slot->at)) + "\t" + ItemLine(ItemAt(pk, item->at)));
        }
        break;
    }
    return found;
}

std::string Text(const fs::path& path) {
    std::ifstream f(path, std::ios::binary);
    return std::string((std::istreambuf_iterator<char>(f)), {});
}

int64_t UnixTime(const fs::file_time_type& t) {
    const auto sys = std::chrono::clock_cast<std::chrono::system_clock>(t);
    return std::chrono::duration_cast<std::chrono::seconds>(sys.time_since_epoch()).count();
}

// The newest save per playthrough, its look read; reading a sav.dat again only when that folder's file changed.
std::string Scan() {
    static std::mutex lock;
    static std::map<std::wstring, std::pair<int64_t, Look>> cache;  // sav.dat path -> (its time, its look)
    std::lock_guard<std::mutex> guard(lock);
    const fs::path root = SaveRoot();
    std::error_code ec;
    if (root.empty() || !fs::is_directory(root, ec)) return {};
    std::map<std::string, Look> newest;
    std::vector<Look> saves;
    for (const auto& entry : fs::directory_iterator(root, ec)) {
        if (!entry.is_directory(ec)) continue;
        const fs::path meta = entry.path() / L"metadata.9.json", sav = entry.path() / L"sav.dat";
        if (!fs::exists(meta, ec) || !fs::exists(sav, ec)) continue;
        Json::Value j;
        Json::CharReaderBuilder builder;
        std::string errors;
        std::istringstream in(Text(meta));
        if (!Json::parseFromStream(builder, in, &j, &errors)) continue;
        const Json::Value& m = j["Data"]["metadata"];
        if (!m.isObject() || !m["playthroughID"].isString()) continue;
        Look look;
        look.playthrough = m["playthroughID"].asString();
        look.folderPath = entry.path().filename().wstring();
        look.folder = Utf8(look.folderPath);
        look.lifePath = m["lifePath"].asString();
        look.bodyGender = m["bodyGender"].asString();
        look.brainGender = m["brainGender"].asString();
        look.level = m["level"].isNumeric() ? m["level"].asDouble() : 0.0;
        look.location = m["locationName"].asString();
        look.quest = m["trackedQuest"].asString();
        look.playTime = m["playTime"].isNumeric() ? m["playTime"].asDouble() : 0.0;
        look.modded = m["isModded"].isBool() && m["isModded"].asBool();
        look.time = UnixTime(fs::last_write_time(sav, ec));
        saves.push_back(look);
        auto at = newest.find(look.playthrough);
        if (at == newest.end() || at->second.time < look.time) newest[look.playthrough] = std::move(look);
    }
    std::vector<Look*> order;
    for (auto& [id, look] : newest) {
        const std::wstring sav = (root / look.folderPath / L"sav.dat").wstring();
        auto hit = cache.find(sav);
        if (hit != cache.end() && hit->second.first == look.time) {
            look.lines = hit->second.second.lines;
        } else {
            const auto save = save_file::Read(sav);
            const auto* node = save.Find("CharacetrCustomization_Appearances");
            if (node && node->offset + node->size <= save.data.size() &&
                ReadLook(save.data.data() + node->offset, node->size, look)) {
                const auto* systems = save.Find("ScriptableSystemsContainer");
                if (!systems || systems->offset + systems->size > save.data.size() ||
                    !ReadWear(save.data.data() + systems->offset, systems->size, look))
                    Log("[VRLobby] save looks: %s: what V wears does not read\n", look.folder.c_str());
                cache[sav] = {look.time, look};
            } else {
                Log("[VRLobby] save looks: %s does not read (%s)\n", look.folder.c_str(),
                    save.error.empty() ? "no look node" : save.error.c_str());
                continue;
            }
        }
        order.push_back(&look);
    }
    std::sort(order.begin(), order.end(), [](const Look* a, const Look* b) { return a->time > b->time; });
    std::string out;
    std::sort(saves.begin(), saves.end(), [](const Look& a, const Look& b) { return a.time > b.time; });
    for (const Look& l : saves) {
        char numbers[64];
        snprintf(numbers, sizeof numbers, "%.0f\t%lld", l.level, static_cast<long long>(l.time));
        out += "save\t" + Clean(l.folder) + "\t" + Clean(l.playthrough) + "\t" + Clean(l.lifePath) + "\t" +
               Clean(l.bodyGender) + "\t" + numbers + "\t" + Clean(l.location) + "\t" + Clean(l.quest) + "\t" +
               std::to_string(static_cast<long long>(l.playTime)) + "\t" + (l.modded ? "1" : "0") + "\n";
    }
    for (const Look* l : order) {
        char level[32];
        snprintf(level, sizeof level, "%.0f", l->level);
        out += "look\t" + Clean(l->playthrough) + "\t" + Clean(l->folder) + "\t" + Clean(l->lifePath) + "\t" +
               Clean(l->bodyGender) + "\t" + Clean(l->brainGender) + "\t" + level + "\t" + std::to_string(l->time) + "\n";
        for (const auto& line : l->lines) out += line + "\n";
    }
    return out;
}
}  // namespace
}  // namespace cvr::lobby::looks

void VRLobbySaveLooks(RED4ext::IScriptable*, RED4ext::CStackFrame* f, RED4ext::CString* out, int64_t) {
    ++f->code;
    std::string text;
    try {
        text = cvr::lobby::looks::Scan();
    } catch (...) {
        Log("[VRLobby] save looks: scan failed\n");
    }
    if (out) *out = RED4ext::CString(text.c_str());
}
