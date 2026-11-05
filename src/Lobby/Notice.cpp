// The notice, in the game's language (Lobby/Notice.hpp).
#include "Lobby/Notice.hpp"
#include "Core/ArchiveExtensions.hpp"
#include "Core/GameSettings.hpp"
#include <RED4ext/RED4ext.hpp>
#include <algorithm>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <mutex>
#include <string>

extern void Log(const char*, ...);

namespace cvr::lobby::notice {
namespace {
// A language the game is translated into: its name in the settings, the suffix of its legal notice's bink (the boot
// splash's own table, inkSplashScreenLoadingScreenLogicController introMessageBink), the locale its letters are set
// in, whether it reads right to left, and the port's word in it.
struct Language {
    const char* name;
    const char* bink;
    const wchar_t* locale;
    bool rightToLeft;
    const wchar_t* lines[2];
};
// The first is the one the game falls back to.
constexpr Language kLanguages[] = {
    {"en-us", "en", L"en-us", false,
     {L"cyberpunkvrport is an experimental fan-made mod.",
      L"An unofficial project, not affiliated with CD PROJEKT RED."}},
    {"ru-ru", "ru", L"ru-ru", false,
     {L"cyberpunkvrport — экспериментальный фанатский мод.",
      L"Неофициальный проект, не связанный с CD PROJEKT RED."}},
    {"ua-ua", "ua", L"uk-ua", false,
     {L"cyberpunkvrport — експериментальний фанатський мод.",
      L"Неофіційний проєкт, не пов'язаний із CD PROJEKT RED."}},
    {"pl-pl", "pl", L"pl-pl", false,
     {L"cyberpunkvrport to eksperymentalny mod stworzony przez fanów.",
      L"Nieoficjalny projekt, niezwiązany z CD PROJEKT RED."}},
    {"de-de", "de", L"de-de", false,
     {L"cyberpunkvrport ist eine experimentelle Fan-Mod.",
      L"Ein inoffizielles Projekt ohne Verbindung zu CD PROJEKT RED."}},
    {"fr-fr", "fr", L"fr-fr", false,
     {L"cyberpunkvrport est un mod expérimental créé par des fans.",
      L"Un projet non officiel, sans lien avec CD PROJEKT RED."}},
    {"es-es", "es", L"es-es", false,
     {L"cyberpunkvrport es un mod experimental creado por fans.",
      L"Un proyecto no oficial, sin relación con CD PROJEKT RED."}},
    {"es-mx", "mx", L"es-mx", false,
     {L"cyberpunkvrport es un mod experimental creado por fans.",
      L"Un proyecto no oficial, sin relación con CD PROJEKT RED."}},
    {"it-it", "it", L"it-it", false,
     {L"cyberpunkvrport è una mod sperimentale creata dai fan.",
      L"Un progetto non ufficiale, non affiliato a CD PROJEKT RED."}},
    {"pt-br", "pr", L"pt-br", false,
     {L"cyberpunkvrport é um mod experimental feito por fãs.",
      L"Um projeto não oficial, sem vínculo com a CD PROJEKT RED."}},
    {"cz-cz", "cz", L"cs-cz", false,
     {L"cyberpunkvrport je experimentální fanouškovský mód.",
      L"Neoficiální projekt bez vazby na CD PROJEKT RED."}},
    {"hu-hu", "hu", L"hu-hu", false,
     {L"A cyberpunkvrport egy kísérleti rajongói mod.",
      L"Nem hivatalos projekt, nem kapcsolódik a CD PROJEKT RED-hez."}},
    {"tr-tr", "tr", L"tr-tr", false,
     {L"cyberpunkvrport, hayranlar tarafından yapılmış deneysel bir moddur.",
      L"CD PROJEKT RED ile bağlantısı olmayan resmî olmayan bir projedir."}},
    {"jp-jp", "jp", L"ja-jp", false,
     {L"cyberpunkvrport はファンによる実験的なMODです。",
      L"CD PROJEKT REDとは関係のない非公式プロジェクトです。"}},
    {"kr-kr", "kr", L"ko-kr", false,
     {L"cyberpunkvrport는 팬이 만든 실험적인 모드입니다.",
      L"CD PROJEKT RED와 관련 없는 비공식 프로젝트입니다."}},
    {"zh-cn", "zh_cn", L"zh-cn", false,
     {L"cyberpunkvrport 是一个由玩家制作的实验性模组。",
      L"这是一个与 CD PROJEKT RED 无关的非官方项目。"}},
    {"zh-tw", "tw", L"zh-tw", false,
     {L"cyberpunkvrport 是一個由玩家製作的實驗性模組。",
      L"這是一個與 CD PROJEKT RED 無關的非官方專案。"}},
    {"th-th", "th", L"th-th", false,
     {L"cyberpunkvrport เป็นม็อดทดลองที่สร้างโดยแฟน ๆ",
      L"เป็นโปรเจกต์ไม่เป็นทางการ ไม่เกี่ยวข้องกับ CD PROJEKT RED"}},
    {"ar-ar", "ar", L"ar-sa", true,
     {L"cyberpunkvrport تعديل تجريبي من صنع المعجبين.",
      L"مشروع غير رسمي لا يرتبط بشركة CD PROJEKT RED."}},
};

// What the player's settings file says, read once.
struct Player {
    const Language* language = &kLanguages[0];
    float effects = 1.0f;
};

float Slider(const std::filesystem::path& file, const char* name) {
    const std::string value = cvr::settings::GetGameSetting(file, "/audio/volume", name);
    if (value.empty()) return 1.0f;
    return std::clamp(float(std::atof(value.c_str())) / 100.0f, 0.0f, 1.0f);
}

const Player& Settings() {
    static const Player player = [] {
        Player p;
        const auto file = cvr::settings::PlayerSettingsFile();
        const std::string onScreen = file.empty() ? std::string() : cvr::settings::GetGameSetting(file, "/language", "OnScreen");
        for (const auto& language : kLanguages)
            if (onScreen == language.name) p.language = &language;
        if (!file.empty()) p.effects = Slider(file, "MasterVolume") * Slider(file, "SfxVolume");
        Log("[notice] the game's language %s (%s), effects at %.2f\n", p.language->name,
            onScreen.empty() ? "not in the settings: the game's own" : "the settings'", p.effects);
        return p;
    }();
    return player;
}

// The ink settings as they loaded and the logo loop they named, till the game has booted.
std::mutex g_mutex;
RED4ext::WeakHandle<RED4ext::ISerializable> g_inkSettings;
RED4ext::ResourcePath g_logo;
bool g_booted = false;

// The ink settings' boot video, their logo loop, as they load: the legal notice's bink in the player's language.
void SetBootVideo(RED4ext::ISerializable* settings) {
    static const RED4ext::CName kField("initLoadingScreenLogoLoopVideo");
    auto* field = settings->GetNativeType()->GetProperty(kField);
    if (!field || !field->type || field->type->GetType() != RED4ext::ERTTIType::ResourceAsyncReference) {
        Log("[notice] the ink settings have no %s: the boot plays what they name\n", kField.ToString());
        return;
    }
    char path[128];
    std::snprintf(path, sizeof(path), "base\\movies\\fullscreen\\common\\cyberpunk2077_game_intro_message_%s.bk2",
                  Settings().language->bink);
    auto& video = field->GetValuePtr<RED4ext::ResourceAsyncReference<>>(settings)->path;
    {
        std::lock_guard lock(g_mutex);
        if (g_booted) return;
        g_inkSettings = settings->ref;
        g_logo = video;
        video = RED4ext::ResourcePath(path);
    }
    Log("[notice] the boot's loading screen plays %s\n", path);
}
}  // namespace

const cvr::render::CaptionText& Word() {
    static const cvr::render::CaptionText word = [] {
        const Language& language = *Settings().language;
        cvr::render::CaptionText text;
        text.lines = {{language.lines[0], 250.0f}, {language.lines[1], 333.0f}};
        text.size = 51.0f;
        text.light = 0.35f;
        text.locale = language.locale;
        text.rightToLeft = language.rightToLeft;
        return text;
    }();
    return word;
}

float EffectsVolume() { return Settings().effects; }

void Install() { cvr::archive::AddFixup("inkGameSettingsResource", &SetBootVideo); }

void BootOver() {
    static const RED4ext::CName kField("initLoadingScreenLogoLoopVideo");
    std::lock_guard lock(g_mutex);
    if (g_booted) return;
    g_booted = true;
    auto settings = g_inkSettings.Lock();
    g_inkSettings = {};
    auto* field = settings ? settings->GetNativeType()->GetProperty(kField) : nullptr;
    if (!field) return;
    // An aligned 8-byte write: a loading screen starting meanwhile reads the one or the other.
    field->GetValuePtr<RED4ext::ResourceAsyncReference<>>(settings.GetPtr())->path = g_logo;
    Log("[notice] booted: the loading screens loop the game's logo again\n");
}
}  // namespace cvr::lobby::notice
