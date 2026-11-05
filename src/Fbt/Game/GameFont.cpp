// The game's own font for its text language: the language from the game's settings file; Rajdhani Medium, the VR
// menu's text face, for every Latin-script language, the game's Cyrillic Rajdhani for Russian and Ukrainian, the game's
// own font for the others (as the VR menu takes them, CyberpunkVRPort_Menu modules/ink.lua K.FONTS). All TrueType, which
// the overlay's rasterizer reads (Blender, the menu's face for the other Latin languages, is CFF). Loaded through the
// game's resource loader; the font file is the rendFont's fontBuffer. The letters to keep: those of the view's texts in that language's
// translation table (modules/lang/settings/<language>.lua: the "Step ..." and "Not taken: ..." entries).
#include "Fbt/Game/GameFont.hpp"
#include "Core/Paths.hpp"
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/ResourceLoader.hpp>
#include <RED4ext/ResourceReference.hpp>
#include <RED4ext/Scripting/Natives/Generated/rend/Font.hpp>
#include <windows.h>
#include <cstring>
#include <fstream>
#include <sstream>

extern void Log(const char*, ...);

namespace cvr::fbt::game {
namespace {
std::string ReadFile(const std::wstring& path) {
    std::ifstream f(path, std::ios::binary);
    std::ostringstream s;
    if (f) s << f.rdbuf();
    return s.str();
}

// "/language" -> "OnScreen" -> its "value" in %LOCALAPPDATA%\CD Projekt Red\Cyberpunk 2077\UserSettings.json.
std::string TextLanguage() {
    wchar_t local[MAX_PATH]{};
    if (!GetEnvironmentVariableW(L"LOCALAPPDATA", local, MAX_PATH)) return "en-us";
    const std::string json = ReadFile(std::wstring(local) + L"\\CD Projekt Red\\Cyberpunk 2077\\UserSettings.json");
    size_t at = json.find("\"/language\"");
    if (at != std::string::npos) at = json.find("\"OnScreen\"", at);
    if (at != std::string::npos) at = json.find("\"value\"", at);
    if (at != std::string::npos) at = json.find('"', json.find(':', at));
    if (at == std::string::npos) return "en-us";
    const size_t end = json.find('"', at + 1);
    const std::string code = end == std::string::npos ? "" : json.substr(at + 1, end - at - 1);
    return code.size() == 5 && code[2] == '-' ? code : "en-us";
}

// Every letter of the view's texts in each language is in its font (checked against the game's files, 2026-11-03).
struct Choice { const char* language; const char* path; };
constexpr const char* kRajdhani = "base\\gameplay\\gui\\fonts\\raj\\raj-medium.fnt";
const Choice kChoices[] = {
    {"ru-ru", "base\\gameplay\\gui\\fonts\\foreign\\russian\\rajdhani_medium_neu.fnt"},
    {"ua-ua", "base\\gameplay\\gui\\fonts\\foreign\\russian\\rajdhani_medium_neu.fnt"},
    {"jp-jp", "base\\gameplay\\gui\\fonts\\foreign\\japanese\\mgenplus\\mgenplus-1cp-medium.fnt"},
    {"kr-kr", "base\\gameplay\\gui\\fonts\\foreign\\korean\\kbiz_go\\kbizgom.fnt"},
    {"zh-cn", "base\\gameplay\\gui\\fonts\\foreign\\chinese\\jing_xi_heig\\arudjingxiheig30_db.fnt"},
    {"zh-tw", "base\\gameplay\\gui\\fonts\\foreign\\chinese_traditional\\jing_xi_heig_b5\\arudjingxiheib5_db.fnt"},
    {"th-th", "base\\gameplay\\gui\\fonts\\foreign\\thai\\th_sarabun_new\\thsarabunnew_bold.fnt"},
    {"ar-ar", "base\\gameplay\\gui\\fonts\\foreign\\arabic\\ara_es_nawar\\araesnawar-regular.fnt"},
};

// The view's texts in that language: the values of the "Step ..." and "Not taken: ..." entries.
std::string ViewTexts(const std::string& language) {
    std::string out;
    if (language == "en-us") return out;
    const std::wstring path = cvr::paths::File(cvr::paths::Folder::Lua,
        (L"CyberpunkVRPort_Menu\\modules\\lang\\settings\\" + std::wstring(language.begin(), language.end()) + L".lua").c_str());
    std::istringstream lines(ReadFile(path));
    for (std::string line; std::getline(lines, line);) {
        if (line.rfind("  [\"Step ", 0) != 0 && line.rfind("  [\"Not taken: ", 0) != 0) continue;
        const size_t eq = line.find("\"] = \"");
        const size_t end = line.rfind("\",");
        if (eq != std::string::npos && end != std::string::npos && end > eq + 6) out += line.substr(eq + 6, end - eq - 6);
    }
    return out;
}

template <typename Ready> bool WaitFor(Ready ready, unsigned timeoutMs) {
    const ULONGLONG until = GetTickCount64() + timeoutMs;
    while (!ready()) {
        if (GetTickCount64() > until) return false;
        Sleep(5);
    }
    return true;
}
}  // namespace

bool ReadGameTextFont(GameTextFont* out, unsigned timeoutMs, std::string* why) {
    auto fail = [&](const char* reason) { if (why) *why = reason; return false; };
    out->language = TextLanguage();
    out->path = kRajdhani;
    for (const Choice& c : kChoices) if (out->language == c.language) out->path = c.path;
    out->texts = ViewTexts(out->language);

    auto* loader = RED4ext::ResourceLoader::Get();
    if (!loader) return fail("the game's resource loader is not up");
    auto token = loader->LoadAsync<RED4ext::rend::Font>(RED4ext::ResourcePath(out->path.c_str()));
    if (!token) return fail("the font could not be requested");
    if (!WaitFor([&] { return token->IsLoaded() || token->IsFailed(); }, timeoutMs)) return fail("the font did not load in time");
    if (token->IsFailed()) return fail("the font is not in the game's archives");
    RED4ext::rend::Font* font = token->resource.GetPtr();
    if (!font || !font->fontBuffer.buffer.data || font->fontBuffer.buffer.size < 12) return fail("the font has no data");
    const auto* data = static_cast<const uint8_t*>(font->fontBuffer.buffer.data);
    const uint32_t size = font->fontBuffer.buffer.size;
    // TrueType outlines only (what the overlay's rasterizer reads): 0x00010000 or 'true'; not 'OTTO' (CFF) or 'ttcf'.
    const bool trueType = (data[0] == 0 && data[1] == 1 && data[2] == 0 && data[3] == 0) || std::memcmp(data, "true", 4) == 0;
    if (!trueType) {
        Log("[fbt] the game's font %s starts %02X %02X %02X %02X: not a TrueType file\n", out->path.c_str(), data[0],
            data[1], data[2], data[3]);
        return fail("not a TrueType file");
    }
    out->file.assign(data, data + size);
    return true;
}

}  // namespace cvr::fbt::game
