#include <stdafx.h>

#include <luajit.h>

#include <algorithm>
#include <map>
#include <mutex>
#include <string>
#include <unordered_map>
#include <vector>

// The plugin's log, one line (src/Core/Log.cpp).
extern "C" void CvrLogText(const char* text);

// Where the host's Lua time goes, for moving the heaviest of it native: LuaJIT's own sampling profiler (1 ms) on the
// host's state, started when the environment has CVR_LUA_PROFILE. Each sample is the line running (marked when no
// mod's onUpdate runs it: an Observe callback, a draw) and the VM's state (compiled code, interpreter, C, GC, JIT
// compiler). Summed up with the onUpdate summary, every 30 s.
namespace {

std::mutex g_lock;
std::unordered_map<std::string, int> g_lines;
struct Calls
{
    int count = 0;
    double ms = 0;
};
std::unordered_map<RED4ext::CBaseFunction*, Calls> g_overrides;
// The game functions Lua calls (RTTIHelper::ExecuteFunction), per mod running at the time: which calls a mod's frame is
// made of, the list to move native or to call less often.
std::map<std::pair<const char*, RED4ext::CBaseFunction*>, Calls> g_calls;
int g_states[256]{};
int g_total = 0;
const char* g_mod = "-";
bool g_running = false;

// A call that carries more than one tick comes after the VM sat idle (LuaJIT hands those ticks to the first line it
// runs next): dropped, or the first Lua of every frame and every override call would look like most of the time.
void OnSample(void*, lua_State* L, int samples, int vmstate)
{
    if (samples > 1)
        return;
    size_t length = 0;
    const char* where = luaJIT_profile_dumpstack(L, "pl", 1, &length);
    std::string key(where, length);
    // The mod's folder and file, not the whole path.
    if (const auto at = key.rfind("\\lua\\"); at != std::string::npos)
        key.erase(0, at + 5);
    if (g_mod[0] == '-' && !g_mod[1])
        key += " (outside onUpdate)";
    else if (key.find('\\') == std::string::npos)
        key = std::string(g_mod) + "\\" + key; // a mod's own init.lua comes without its folder
    std::lock_guard guard(g_lock);
    ++g_lines[key];
    ++g_states[vmstate & 0xFF];
    ++g_total;
}

} // namespace

extern "C" bool CvrProfileWanted()
{
    static const bool wanted = GetEnvironmentVariableW(L"CVR_LUA_PROFILE", nullptr, 0) > 0;
    return wanted;
}

extern "C" void CvrProfileStart(lua_State* L)
{
    if (g_running || !CvrProfileWanted())
        return;
    luaJIT_profile_start(L, "li1", OnSample, nullptr);
    g_running = true;
    CvrLogText("[lua] profiler on (CVR_LUA_PROFILE): 1 ms samples, summed every 30 s\n");
}

// An overridden function's call (FunctionOverride), its Lua chain included.
extern "C" void CvrProfileOverride(RED4ext::CBaseFunction* apFunction, double aMs)
{
    std::lock_guard guard(g_lock);
    auto& calls = g_overrides[apFunction];
    ++calls.count;
    calls.ms += aMs;
}

// A game function called from Lua (RTTIHelper::ExecuteFunction), with the mod whose onUpdate runs ("-" outside one).
extern "C" void CvrProfileCall(RED4ext::CBaseFunction* apFunction, double aMs)
{
    std::lock_guard guard(g_lock);
    auto& calls = g_calls[{g_mod, apFunction}];
    ++calls.count;
    calls.ms += aMs;
}

// Set by the store around each mod's onUpdate (main thread; the samples come from the thread running Lua).
extern "C" void CvrProfileMod(const char* acMod)
{
    g_mod = acMod ? acMod : "-";
}

// The update summary's tick count, for the overrides' calls and time per tick.
extern "C" void CvrProfileReport(size_t aTicks)
{
    if (!g_running)
        return;
    std::vector<std::pair<std::string, int>> lines;
    std::vector<std::pair<RED4ext::CBaseFunction*, Calls>> overrides;
    std::vector<std::pair<std::pair<const char*, RED4ext::CBaseFunction*>, Calls>> gameCalls;
    int states[256];
    int total;
    {
        std::lock_guard guard(g_lock);
        overrides.assign(g_overrides.begin(), g_overrides.end());
        g_overrides.clear();
        gameCalls.assign(g_calls.begin(), g_calls.end());
        g_calls.clear();
        lines.assign(g_lines.begin(), g_lines.end());
        std::copy(std::begin(g_states), std::end(g_states), std::begin(states));
        total = g_total;
        g_lines.clear();
        std::fill(std::begin(g_states), std::end(g_states), 0);
        g_total = 0;
    }
    if (!total)
        return;
    std::sort(lines.begin(), lines.end(), [](const auto& a, const auto& b) { return a.second > b.second; });
    std::string text = fmt::format("[lua] profile: {} samples; compiled {:.0f}%, interpreted {:.0f}%, C {:.0f}%, GC {:.0f}%, JIT {:.0f}%;", total,
                                   100.0 * states['N'] / total, 100.0 * states['I'] / total, 100.0 * states['C'] / total,
                                   100.0 * states['G'] / total, 100.0 * states['J'] / total);
    for (size_t i = 0; i < lines.size() && i < 48; ++i)
        text += fmt::format("{} {} {:.1f}%", i ? "," : "", lines[i].first, 100.0 * lines[i].second / total);
    text += '\n';
    CvrLogText(text.c_str());

    // The same samples per file: which module of a mod its time is in.
    std::unordered_map<std::string, int> files;
    for (const auto& [line, count] : lines)
    {
        auto file = line;
        if (const auto colon = file.rfind(':'); colon != std::string::npos)
        {
            const auto rest = file.find(' ', colon);
            file.erase(colon, rest == std::string::npos ? std::string::npos : rest - colon);
        }
        files[file] += count;
    }
    std::vector<std::pair<std::string, int>> byFile(files.begin(), files.end());
    std::sort(byFile.begin(), byFile.end(), [](const auto& a, const auto& b) { return a.second > b.second; });
    std::string fileText = "[lua] profile by file:";
    for (size_t i = 0; i < byFile.size() && i < 24; ++i)
        fileText += fmt::format("{} {} {:.1f}%", i ? "," : "", byFile[i].first, 100.0 * byFile[i].second / total);
    fileText += '\n';
    CvrLogText(fileText.c_str());

    if (!gameCalls.empty() && aTicks)
    {
        std::sort(gameCalls.begin(), gameCalls.end(), [](const auto& a, const auto& b) { return a.second.ms > b.second.ms; });
        double sum = 0;
        for (const auto& c : gameCalls)
            sum += c.second.ms;
        std::string line = fmt::format("[lua] game calls per tick ({:.3f} ms in all):", sum / aTicks);
        for (size_t i = 0; i < gameCalls.size() && i < 24; ++i)
        {
            auto* pFunction = gameCalls[i].first.second;
            auto* pClass = pFunction->GetParent();
            line += fmt::format("{} {} {}{}{} {:.1f} x {:.3f} ms", i ? "," : "", gameCalls[i].first.first,
                                pClass ? pClass->GetName().ToString() : "", pClass ? "::" : "", pFunction->shortName.ToString(),
                                double(gameCalls[i].second.count) / aTicks, gameCalls[i].second.ms / aTicks);
        }
        line += '\n';
        CvrLogText(line.c_str());
    }

    if (overrides.empty() || !aTicks)
        return;
    std::sort(overrides.begin(), overrides.end(), [](const auto& a, const auto& b) { return a.second.ms > b.second.ms; });
    std::string calls = "[lua] overridden functions per tick:";
    for (size_t i = 0; i < overrides.size() && i < 10; ++i)
    {
        auto* pFunction = overrides[i].first;
        auto* pClass = pFunction->GetParent();
        calls += fmt::format("{} {}{}{} {:.1f} calls {:.3f} ms", i ? "," : "", pClass ? pClass->GetName().ToString() : "",
                             pClass ? "::" : "", pFunction->shortName.ToString(), double(overrides[i].second.count) / aTicks,
                             overrides[i].second.ms / aTicks);
    }
    calls += '\n';
    CvrLogText(calls.c_str());
}
