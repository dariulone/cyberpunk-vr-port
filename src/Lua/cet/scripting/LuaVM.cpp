#include <stdafx.h>

#include "LuaVM.h"

#include <CET.h>

// The plugin's log, one line (src/Core/Log.cpp).
extern "C" void CvrLogText(const char* text);
// The host's profiler (src/Lua/Profiler.cpp), on CVR_LUA_PROFILE.
extern "C" bool CvrProfileWanted();
extern "C" void CvrProfileStart(lua_State* L);
extern "C" void CvrProfileReport(size_t aTicks);

namespace
{
// Every way in from the game goes through here: an exception stops the host, the game runs on without it.
template <typename F> void Guarded(bool& aFailed, const char* acWhat, F&& aCall)
{
    if (aFailed)
        return;
    std::string what;
    try
    {
        aCall();
        return;
    }
    catch (const std::exception& e)
    {
        what = e.what();
    }
    catch (...)
    {
        what = "unknown exception";
    }
    aFailed = true;
    Log::Error("{} failed, the Lua host stops: {}", acWhat, what);
    CvrLogText(fmt::format("[lua] {} failed, the Lua host stops: {}\n", acWhat, what).c_str());
}

// The host's cost on the game's main thread: each onUpdate tick (the mods' updates, the binds, the GC steps the
// mods take) timed, summed up in the plugin's log every 30 s. Main thread only.
struct UpdateCost
{
    std::vector<float> ms;
    std::chrono::steady_clock::time_point since{std::chrono::steady_clock::now()};

    void Add(float aMs, const LuaVM& acVm)
    {
        ms.push_back(aMs);
        const auto now = std::chrono::steady_clock::now();
        const float seconds = std::chrono::duration<float>(now - since).count();
        if (seconds < 30.f)
            return;
        std::sort(ms.begin(), ms.end());
        double sum = 0;
        for (const float v : ms)
            sum += v;
        std::string mods;
        const auto cost = acVm.TakeUpdateCost();
        for (size_t i = 0; i < cost.size() && i < 6; ++i)
            mods += fmt::format("{}{} {:.3f}", i ? ", " : "", cost[i].first, cost[i].second / ms.size());
        CvrLogText(fmt::format("[lua] onUpdate over {:.0f} s: {} ticks, mean {:.3f} ms, p50 {:.3f}, p99 {:.3f}, max {:.3f}; per tick: {}\n",
                               seconds, ms.size(), sum / ms.size(), ms[ms.size() / 2], ms[ms.size() * 99 / 100], ms.back(), mods)
                       .c_str());
        CvrProfileReport(ms.size());
        ms.clear();
        since = now;
    }
};
UpdateCost s_updateCost;
} // namespace

const VKBind* LuaVM::GetBind(const VKModBind& acModBind) const
{
    return m_scripting.GetBind(acModBind);
}

const TiltedPhoques::Vector<VKBind>* LuaVM::GetBinds(const std::string& acModName) const
{
    return m_scripting.GetBinds(acModName);
}

const TiltedPhoques::Map<std::string, std::reference_wrapper<const TiltedPhoques::Vector<VKBind>>>& LuaVM::GetAllBinds() const
{
    return m_scripting.GetAllBinds();
}

bool LuaVM::ExecuteLua(const std::string& acCommand) const
{
    if (!m_initialized)
    {
        spdlog::get("scripting")->info("Command not executed! LuaVM is not yet initialized!");
        return false;
    }

    return m_scripting.ExecuteLua(acCommand);
}

// The host's collector pacing (src/Lua/HostGc.cpp).
extern "C" void CvrLuaGcRegister(sol::state* apState);
extern "C" void CvrLuaGcSafePoint();

void LuaVM::Update(float aDeltaTime)
{
    if (!m_initialized)
        return;

    CvrLuaGcSafePoint();

    const auto start = std::chrono::steady_clock::now();
    const struct Timed
    {
        std::chrono::steady_clock::time_point start;
        const LuaVM& vm;
        ~Timed() { s_updateCost.Add(std::chrono::duration<float, std::milli>(std::chrono::steady_clock::now() - start).count(), vm); }
    } timed{start, *this};

    Guarded(m_failed, "onUpdate", [&]
    {
        CET::Get().GetBindings().Update();

        if (m_reload)
        {
            m_scripting.ReloadAllMods();

            m_reload = false;
        }

        m_scripting.TriggerOnUpdate(aDeltaTime);
    });
}

void LuaVM::Draw() const
{
    if (!m_initialized || m_drawBlocked)
        return;

    Guarded(m_failed, "onDraw", [&] { m_scripting.TriggerOnDraw(); });
}

void LuaVM::ReloadAllMods()
{
    if (m_initialized)
        m_reload = true;
}

void LuaVM::OnOverlayOpen() const
{
    if (m_initialized)
        Guarded(m_failed, "onOverlayOpen", [&] { m_scripting.TriggerOnOverlayOpen(); });
}

void LuaVM::OnOverlayClose() const
{
    if (m_initialized)
        Guarded(m_failed, "onOverlayClose", [&] { m_scripting.TriggerOnOverlayClose(); });
}

void LuaVM::Initialize()
{
    if (!IsInitialized())
        Guarded(m_failed, "the Lua state's setup", [&] { m_scripting.Initialize(); });
}

bool LuaVM::IsInitialized() const
{
    return m_initialized;
}

void LuaVM::BlockDraw(bool aBlockDraw)
{
    m_drawBlocked = aBlockDraw;
}

void LuaVM::RemoveTDBIDDerivedFrom(uint64_t aDBID)
{
    std::lock_guard _{m_tdbidLock};

    const auto it = m_tdbidDerivedLookup.find(aDBID);
    if (it != m_tdbidDerivedLookup.end())
    {
        for (uint64_t flatID : it->second)
        {
            m_tdbidLookup.erase(flatID);
        }

        m_tdbidDerivedLookup.erase(it);
    }
}

bool LuaVM::GetTDBIDDerivedFrom(uint64_t aDBID, TiltedPhoques::Vector<uint64_t>& aDerivedList)
{
    std::lock_guard _{m_tdbidLock};

    const auto it = m_tdbidDerivedLookup.find(aDBID & 0xFFFFFFFFFF);
    if (it == m_tdbidDerivedLookup.end())
        return false;

    aDerivedList.reserve(it->second.size());
    std::ranges::copy(it->second, std::back_inserter(aDerivedList));
    return true;
}

uint64_t LuaVM::GetTDBIDBase(uint64_t aDBID)
{
    std::lock_guard _{m_tdbidLock};

    const auto it = m_tdbidLookup.find(aDBID & 0xFFFFFFFFFF);
    if (it == m_tdbidLookup.end())
        return 0;
    return it->second.base;
}

TDBIDLookupEntry LuaVM::GetTDBIDLookupEntry(uint64_t aDBID)
{
    std::lock_guard _{m_tdbidLock};

    const auto it = m_tdbidLookup.find(aDBID & 0xFFFFFFFFFF);
    if (it == m_tdbidLookup.end())
        return {0, "<unknown>"};

    return it->second;
}

std::string LuaVM::GetTDBDIDDebugString(TDBID aDBID) const
{
    RED4ext::TweakDBID internal(aDBID.value);
    return internal.HasTDBOffset() ? fmt::format("<TDBID:{:08X}:{:02X}:{:06X}>", internal.name.hash, internal.name.length, internal.ToTDBOffset())
                                   : fmt::format("<TDBID:{:08X}:{:02X}>", internal.name.hash, internal.name.length);
}

std::string LuaVM::GetTDBIDString(uint64_t aDBID, bool aOnlyRegistered)
{
    std::lock_guard _{m_tdbidLock};

    auto it = m_tdbidLookup.find(aDBID & 0xFFFFFFFFFF);
    if (it == m_tdbidLookup.end())
        return aOnlyRegistered ? "" : GetTDBDIDDebugString(TDBID{aDBID});

    std::string string = it->second.name;
    uint64_t base = it->second.base;
    while (base != 0)
    {
        it = m_tdbidLookup.find(it->second.base);
        if (it == m_tdbidLookup.end())
        {
            string.insert(0, GetTDBDIDDebugString(TDBID{base}));
            break;
        }
        string.insert(0, it->second.name);
        base = it->second.base;
    }

    return string;
}

void LuaVM::RegisterTDBIDString(uint64_t aValue, uint64_t aBase, const std::string& acString)
{
    if (aValue == 0)
        return;
    std::lock_guard _{m_tdbidLock};

    m_tdbidLookup[aValue] = {aBase, acString};
    if (aBase != 0)
        m_tdbidDerivedLookup[aBase].insert(aValue);
}

void LuaVM::PostInitializeScripting()
{
    Guarded(m_failed, "the RTTI bridge's setup", [&] { m_scripting.PostInitializeScripting(); });
}

void LuaVM::PostInitializeTweakDB()
{
    Guarded(m_failed, "onTweak", [&] { m_scripting.PostInitializeTweakDB(); });
}

void LuaVM::PostInitializeMods()
{
    assert(!m_initialized);

    Guarded(m_failed, "loading the mods", [&]
    {
        m_scripting.PostInitializeMods();

        if (CvrProfileWanted())
        {
            auto lockedState = m_scripting.GetLockedState();
            CvrProfileStart(lockedState.Get().lua_state());
        }

        {
            auto lockedState = m_scripting.GetLockedState();
            CvrLuaGcRegister(&lockedState.Get());
        }

        spdlog::get("scripting")->info("LuaVM: initialization finished!");

        m_initialized = true;
    });
}
