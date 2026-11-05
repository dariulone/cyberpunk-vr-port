#pragma once

#include "Sandbox.h"

#include <mutex>
#include <unordered_map>

struct LuaSandbox
{
    LuaSandbox(Scripting* apScripting, const VKBindings& acVKBindings);
    ~LuaSandbox() = default;

    void Initialize();
    void PostInitializeScripting();
    void PostInitializeTweakDB();
    void PostInitializeMods();

    void ResetState();

    uint64_t CreateSandbox(
        const std::filesystem::path& acPath = "", const std::string& acName = "", bool aEnableExtraLibs = true, bool aEnableDB = true, bool aEnableIO = true,
        bool aEnableLogger = true);

    Sandbox& operator[](uint64_t aID);
    const Sandbox& operator[](uint64_t aID) const;

    [[nodiscard]] TiltedPhoques::Locked<sol::state, std::recursive_mutex> GetLockedState() const;

    void SetImGuiAvailable(bool aAvailable);
    bool GetImGuiAvailable() const;

    sol::table& GetGlobals();

    const bool GetIsLaunchedThroughMO2() const { return m_isLaunchedThroughMO2; }
    [[nodiscard]] std::filesystem::path 
    Relative(const std::filesystem::path& acFilePath, const std::filesystem::path& acRootPath) const;
    [[nodiscard]] std::filesystem::path
    GetLuaPath(const std::string& acFilePath, const std::filesystem::path& acRootPath, const bool acAllowNonExisting) const;
    [[nodiscard]] std::filesystem::path
    GetLuaPath(std::filesystem::path aFilePath, const std::filesystem::path& acRootPath, const bool acAllowNonExisting) const;

private:
    void InitializeExtraLibsForSandbox(Sandbox& aSandbox, const sol::state& acpState) const;
    void InitializeDBForSandbox(Sandbox& aSandbox, const sol::state& acpState);
    void InitializeIOForSandbox(Sandbox& aSandbox, const sol::state& acpState, const std::string& acName);
    void InitializeLoggerForSandbox(Sandbox& aSandbox, const sol::state& acpState, const std::string& acName) const;

    void CloseDBForSandbox(const Sandbox& aSandbox) const;

    Scripting* m_pScripting;
    const VKBindings& m_vkBindings;
    sol::table m_globals{};
    TiltedPhoques::Vector<Sandbox> m_sandboxes{};
    TiltedPhoques::Map<std::string, sol::object> m_modules{};
    // require's name as a mod passed it (its root, a tab, the name) -> its key in m_modules: a module already loaded
    // is returned without touching the disk.
    TiltedPhoques::Map<std::string, std::string> m_moduleKeys{};
    // GetLuaPath's answers for paths that may not exist yet (io.open, the targets of os.remove/os.rename): the root,
    // the current directory and the path -> the checked path. The check is the OS canonicalising the paths
    // (std::filesystem::relative, twice): 2 ms on average and 15-37 ms at times in the game folder, paid by a mod
    // polling a command file ten times a second. Its answer does not depend on the file existing, so it is worked out
    // once per path.
    mutable std::mutex m_pathLock{};
    mutable std::unordered_map<std::wstring, std::filesystem::path> m_pathCache{};

    bool m_imguiAvailable{false};
    bool m_isLaunchedThroughMO2{false};
};
