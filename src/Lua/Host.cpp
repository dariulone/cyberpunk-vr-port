#include <stdafx.h>

#include "Lua/Host.hpp"
#include "Migration.hpp"

#include "CET.h"
#include "Utils.h"

#include "Core/Paths.hpp"

#include <string>

// The plugin's log, one line (src/Core/Log.cpp): CET's logging namespace takes the name Log here.
extern "C" void CvrLogText(const char* text);

namespace {

template <typename... Args> void PluginLog(fmt::format_string<Args...> aFormat, Args&&... aArgs)
{
    CvrLogText(fmt::format(aFormat, std::forward<Args>(aArgs)...).c_str());
}

}  // namespace

namespace {

// What CET's program owns, in the order it makes them (CET.cpp): the VM hooks the engine as it is made.
struct HostState
{
    Paths paths;
    Options options;
    VKBindings bindings{paths, options};
    D3D12 d3d12;
    Overlay overlay;
    LuaVM vm{paths, options, bindings, d3d12};
};

HostState* g_host = nullptr;

// An installed CET's overlay key (MigrateFromCet); 0 without one.
uint64_t g_cetOverlayKey = 0;

}  // namespace

CET& CET::Get()
{
    static CET s_facade;
    return s_facade;
}

const Paths& CET::GetPaths() const noexcept { return g_host->paths; }
const Options& CET::GetOptions() const noexcept { return g_host->options; }
D3D12& CET::GetD3D12() noexcept { return g_host->d3d12; }
VKBindings& CET::GetBindings() noexcept { return g_host->bindings; }
Overlay& CET::GetOverlay() noexcept { return g_host->overlay; }
LuaVM& CET::GetVM() noexcept { return g_host->vm; }
bool CET::IsRunning() noexcept { return g_host != nullptr; }

Paths::Paths()
{
    std::vector<wchar_t> exe(32768);
    const DWORD length = GetModuleFileNameW(nullptr, exe.data(), static_cast<DWORD>(exe.size()));
    m_exe = std::wstring(exe.data(), length);
    m_gameRoot = m_exe.parent_path();
    m_cetRoot = std::filesystem::path(cvr::paths::Dir(cvr::paths::Folder::Root)).parent_path();
    const auto config = std::filesystem::path(cvr::paths::Dir(cvr::paths::Folder::Config)).parent_path();
    m_config = config / L"lua.json";
    m_persistentState = config / L"lua_persistent.json";
    m_vkBindings = config / L"lua_bindings.json";
    m_tweakdb = config / L"tweakdb";
    m_modsRoot = std::filesystem::path(cvr::paths::Dir(cvr::paths::Folder::Lua)).parent_path();
    m_r6CacheModdedRoot = m_gameRoot / L".." / L".." / L"r6" / L"cache" / L"modded";
    m_fonts = m_cetRoot / L"fonts";
}

const std::filesystem::path& Paths::Executable() const { return m_exe; }
const std::filesystem::path& Paths::GameRoot() const { return m_gameRoot; }
const std::filesystem::path& Paths::CETRoot() const { return m_cetRoot; }
const std::filesystem::path& Paths::Config() const { return m_config; }
const std::filesystem::path& Paths::PersistentState() const { return m_persistentState; }
const std::filesystem::path& Paths::VKBindings() const { return m_vkBindings; }
const std::filesystem::path& Paths::ModsRoot() const { return m_modsRoot; }
const std::filesystem::path& Paths::R6CacheModdedRoot() const { return m_r6CacheModdedRoot; }
const std::filesystem::path& Paths::Fonts() const { return m_fonts; }
const std::filesystem::path& Paths::TweakDB() const { return m_tweakdb; }

namespace cvr::lua {

bool SelfTest()
{
    try
    {
        sol::state lua;
        lua.open_libraries(sol::lib::base, sol::lib::string, sol::lib::table, sol::lib::math, sol::lib::jit);
        const std::string version = lua["jit"]["version"].get_or(std::string("?"));
        const int sum = lua.script("local s = 0 for i = 1, 100 do s = s + i end return s");
        PluginLog("[lua] {} runs (self-test {})\n", version, sum);
        return sum == 5050;
    }
    catch (const std::exception& e)
    {
        PluginLog("[lua] the runtime failed its self-test: {}\n", e.what());
        return false;
    }
}

void Install()
{
    if (g_host)
        return;
    // CET's own log (Options.cpp) is the default logger the scripting core writes its notes to.
    const auto logs = std::filesystem::path(cvr::paths::Dir(cvr::paths::Folder::Log)) / L"lua";
    std::error_code error;
    create_directories(logs, error);
    set_default_logger(CreateLogger(logs / L"host.log", "main", nullptr, "[%Y-%m-%d %H:%M:%S UTC%z] [%l] [%!] [%t] %v"));
    // The host's and the mods' logs reach the disk every second, a game that is ended included.
    spdlog::flush_every(std::chrono::seconds(1));
    MH_Initialize();
    g_cetOverlayKey = MigrateFromCet();
    g_host = new HostState;
    g_host->vm.Prepare();
    PluginLog("[lua] the host is up: mods from {}\n", UTF16ToUTF8(g_host->paths.ModsRoot().native()));
}

void OnWndProc(HWND hwnd, UINT msg, WPARAM wParam, LPARAM lParam)
{
    if (g_host)
        g_host->bindings.OnWndProc(hwnd, msg, wParam, lParam);
}

bool OverlayOpen()
{
    return g_host && g_host->overlay.IsEnabled();
}

bool PassRawInput(LPARAM lParam)
{
    RAWINPUT raw{};
    UINT size = sizeof(raw);
    if (GetRawInputData(reinterpret_cast<HRAWINPUT>(lParam), RID_INPUT, &raw, &size, sizeof(RAWINPUTHEADER)) == static_cast<UINT>(-1))
        return true; // bigger than a key or a mouse event: another device's
    if (raw.header.dwType == RIM_TYPEMOUSE)
    {
        constexpr USHORT kReleases = RI_MOUSE_LEFT_BUTTON_UP | RI_MOUSE_RIGHT_BUTTON_UP | RI_MOUSE_MIDDLE_BUTTON_UP |
                                     RI_MOUSE_BUTTON_4_UP | RI_MOUSE_BUTTON_5_UP;
        return (raw.data.mouse.usButtonFlags & kReleases) != 0;
    }
    if (raw.header.dwType != RIM_TYPEKEYBOARD)
        return true;

    const USHORT pressed = raw.data.keyboard.VKey;
    const bool released = (raw.data.keyboard.Flags & RI_KEY_BREAK) != 0;
    if (g_cetOverlayKey && GetModuleHandleW(L"cyber_engine_tweaks.asi"))
    {
        // VKBindings' encoding: up to four keys, the first in the top 16 bits. CET fires its bind as the keys go up.
        bool part = false;
        USHORT last = 0;
        for (int shift = 48; shift >= 0; shift -= 16)
        {
            const auto key = static_cast<USHORT>(g_cetOverlayKey >> shift);
            if (!key)
                break;
            part = part || key == pressed;
            last = key;
        }
        if (part)
        {
            if (released && pressed == last)
                ToggleOverlay();
            return true;
        }
    }
    return released;
}

void Draw(float width, float height)
{
    if (!g_host)
        return;
    g_host->d3d12.SetResolution({static_cast<LONG>(width), static_cast<LONG>(height)});
    // CET draws every frame and each mod's onDraw returns at once while the overlay is closed; here the render
    // thread takes the Lua state only while it is open (onUpdate holds it on the main thread meanwhile).
    if (g_host->overlay.IsEnabled())
        g_host->vm.Draw();
}

}  // namespace cvr::lua

void ToggleOverlay()
{
    CET::Get().GetOverlay().Toggle();
}

void Overlay::Toggle()
{
    // From the overlay key's bind, on the window's thread.
    SetOverlayEnabled(*this, !m_enabled);
    if (m_enabled)
        g_host->vm.OnOverlayOpen();
    else
        g_host->vm.OnOverlayClose();
    PluginLog("[lua] overlay {}\n", m_enabled ? "open" : "closed");
}
