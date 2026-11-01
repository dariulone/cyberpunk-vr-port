#include "Core/ScriptCompiler.hpp"

#include "Core/Paths.hpp"
#include "Stereo/EngineRvas.hpp"

#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Relocation.hpp>

#include <Windows.h>

#include <algorithm>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <string>
#include <vector>

extern void Log(const char* fmt, ...);

namespace cvr::scripts {
namespace {

namespace fs = std::filesystem;

constexpr uint32_t kExecuteProcessHash = 2203918127;  // the engine starting a process: the script compiler among them

fs::path GameRoot() {
    std::vector<wchar_t> exe(32768);
    const DWORD length = GetModuleFileNameW(nullptr, exe.data(), static_cast<DWORD>(exe.size()));
    if (!length || length >= exe.size()) return {};
    return fs::path(std::wstring(exe.data(), length)).parent_path().parent_path().parent_path();
}

// redscript's compiler where its installer puts it (scc_lib.dll; an old release has only scc.exe).
bool RedscriptInstalled(const fs::path& root) {
    std::error_code error;
    const fs::path tools = root / L"engine" / L"tools";
    return fs::exists(tools / L"scc_lib.dll", error) || fs::exists(tools / L"scc.exe", error);
}

bool SameBytes(const fs::path& a, const fs::path& b) {
    std::error_code error;
    if (!fs::exists(b, error) || fs::file_size(a, error) != fs::file_size(b, error)) return false;
    std::ifstream x(a, std::ios::binary), y(b, std::ios::binary);
    return std::equal(std::istreambuf_iterator<char>(x), std::istreambuf_iterator<char>(),
                      std::istreambuf_iterator<char>(y), std::istreambuf_iterator<char>());
}

// r6\scripts\CyberpunkVRPort_*: the same bytes as the plugin's copy are deleted, anything else is renamed .legacy,
// which the compiler does not read; the folders left empty go.
void RetireLegacyScripts(const fs::path& root) {
    const fs::path scripts = root / L"r6" / L"scripts";
    const fs::path own = paths::Dir(paths::Folder::Scripts);
    std::error_code error;
    if (!fs::is_directory(scripts, error)) return;
    std::vector<fs::path> folders;
    for (const auto& entry : fs::directory_iterator(scripts, error))
        if (entry.is_directory(error) && entry.path().filename().wstring().rfind(L"CyberpunkVRPort_", 0) == 0)
            folders.push_back(entry.path());
    int deleted = 0, renamed = 0;
    for (const fs::path& folder : folders) {
        std::vector<fs::path> files, dirs;
        for (const auto& entry : fs::recursive_directory_iterator(folder, error)) {
            if (entry.is_directory(error)) dirs.push_back(entry.path());
            else if (entry.path().extension() == L".reds") files.push_back(entry.path());
        }
        for (const fs::path& file : files) {
            if (SameBytes(file, own / fs::relative(file, scripts, error))) {
                if (fs::remove(file, error)) ++deleted;
            } else {
                fs::path aside = file;
                aside += L".legacy";
                fs::rename(file, aside, error);
                if (!error) ++renamed;
            }
        }
        // Deepest first: the mod's own folder last.
        dirs.insert(dirs.begin(), folder);
        for (auto it = dirs.rbegin(); it != dirs.rend(); ++it)
            if (fs::is_empty(*it, error)) fs::remove(*it, error);
    }
    if (deleted || renamed)
        Log("[scripts] an earlier release's scripts in r6\\scripts taken out: %d deleted (same as the plugin's), %d "
            "renamed .legacy\n", deleted, renamed);
}

// Scripts/EnableCompilation: a bool config variable (vtable +0, name +8, group +16, value +48) whose vtable slot 3
// sets it from a string view {const char*, u32 length}. Set here, before the engine reads its config: an ini that
// names it still decides.
bool EnableCompilation() {
    const auto base = reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    auto* variable = reinterpret_cast<uint8_t*>(base + detail::SCRIPTS_ENABLE_COMPILATION_VAR_RVA);
    const auto vtable = *reinterpret_cast<uintptr_t* const*>(variable);
    const char* name = *reinterpret_cast<const char* const*>(variable + 8);
    const char* group = *reinterpret_cast<const char* const*>(variable + 16);
    if (reinterpret_cast<uintptr_t>(vtable) != base + detail::CONFIG_BOOL_VAR_VTABLE_RVA || !name || !group ||
        std::strcmp(name, "EnableCompilation") != 0 || std::strcmp(group, "Scripts") != 0) {
        Log("[scripts] Scripts/EnableCompilation is not where this build keeps it: the scripts are not compiled\n");
        return false;
    }
    struct StringView { const char* text; uint32_t length; } view{"true", 4};
    using SetFromString = bool(__fastcall*)(void*, const StringView*);
    return reinterpret_cast<SetFromString>(vtable[3])(variable, &view);
}

// The engine starts scc.exe with the compilation's arguments; RED4ext's hook on the same function runs the compiler
// in process from the scc_lib.dll beside the path it is given. The plugin's path goes in place of the engine's.
struct ProcessArguments;  // the engine's fixed wide string, passed through
using ExecuteProcessFn = bool (*)(void*, RED4ext::CString&, ProcessArguments&, RED4ext::CString&, char);
ExecuteProcessFn g_executeProcess = nullptr;
std::string g_compiler;  // redscript\scc.exe in the ANSI code page, as the engine's own path is

bool Detour_ExecuteProcess(void* self, RED4ext::CString& command, ProcessArguments& arguments,
                           RED4ext::CString& directory, char flags) {
    if (!std::strstr(command.c_str(), "scc.exe")) return g_executeProcess(self, command, arguments, directory, flags);
    RED4ext::CString compiler(g_compiler.c_str());
    LARGE_INTEGER started{}, done{}, frequency{};
    QueryPerformanceCounter(&started);
    const bool ok = g_executeProcess(self, compiler, arguments, directory, flags);
    QueryPerformanceCounter(&done);
    QueryPerformanceFrequency(&frequency);
    Log("[scripts] compiled with the plugin's compiler (redscript %s): %s, %.0f ms\n", CVR_REDSCRIPT_VERSION,
        ok ? "done" : "FAILED", 1000.0 * static_cast<double>(done.QuadPart - started.QuadPart) / frequency.QuadPart);
    return ok;
}

}  // namespace

void Install(RED4ext::v1::PluginHandle handle, const RED4ext::v1::Sdk* sdk) {
    const fs::path root = GameRoot();
    if (root.empty()) return;
    RetireLegacyScripts(root);
    const std::wstring own = paths::Dir(paths::Folder::Scripts);
    const bool added = sdk->scripts && sdk->scripts->Add(handle, own.c_str());
    Log("[scripts] %ls %s\n", own.c_str(), added ? "joins the script compilation" : "NOT added to the script compilation");
    if (RedscriptInstalled(root)) {
        Log("[scripts] redscript is installed: its compiler compiles the port's scripts\n");
        return;
    }
    std::error_code error;
    if (!fs::exists(paths::File(paths::Folder::Redscript, L"scc_lib.dll"), error)) {
        Log("[scripts] neither redscript nor the plugin's compiler (redscript\\scc_lib.dll): the scripts are not "
            "compiled\n");
        return;
    }
    if (!EnableCompilation()) return;
    g_compiler = paths::FileA(paths::Folder::Redscript, "scc.exe");
    void* target = reinterpret_cast<void*>(RED4ext::UniversalRelocBase::Resolve(kExecuteProcessHash));
    if (!target || !sdk->hooking->Attach(handle, target, reinterpret_cast<void*>(&Detour_ExecuteProcess),
                                         reinterpret_cast<void**>(&g_executeProcess))) {
        Log("[scripts] the engine's start of the compiler is NOT hooked: the scripts are not compiled\n");
        return;
    }
    Log("[scripts] redscript is not installed: the plugin's compiler compiles the scripts\n");
}

}  // namespace cvr::scripts
