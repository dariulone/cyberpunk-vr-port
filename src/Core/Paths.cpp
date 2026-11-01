#include "Core/Paths.hpp"

#include <windows.h>

#include <array>
#include <mutex>
#include <vector>

extern void Log(const char* fmt, ...);

namespace cvr::paths {
namespace {

constexpr const wchar_t* kNames[] = {L"",         L"shaders\\", L"grips\\",   L"archive\\",
                                      L"config\\", L"log\\",     L"scripts\\", L"redscript\\", L"lua\\"};
// The folder an earlier release installed the plugin in.
constexpr const wchar_t* kLegacyPlugin = L"CyberpunkVR_Stereo";
// The plugin's file, and its name up to 0.1.9: a copy under either name is the port.
constexpr const wchar_t* kModuleNames[] = {L"cyberpunkvrport.dll", L"CyberpunkVR_Stereo.dll"};

std::wstring FolderOf(HMODULE module) {
    std::vector<wchar_t> path(32768);
    const DWORD length = GetModuleFileNameW(module, path.data(), static_cast<DWORD>(path.size()));
    if (!length || length >= path.size()) return {};
    std::wstring folder(path.data(), length);
    const size_t slash = folder.find_last_of(L'\\');
    return slash == std::wstring::npos ? std::wstring() : folder.substr(0, slash + 1);
}

std::wstring ModuleFolder() {
    HMODULE self{};
    if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                            reinterpret_cast<LPCWSTR>(&ModuleFolder), &self))
        return {};
    return FolderOf(self);
}

// No logging in here: the log's own path comes from it.
struct Folders {
    std::array<std::wstring, std::size(kNames)> dir;
    Folders() {
        const std::wstring root = ModuleFolder();
        for (size_t i = 0; i < dir.size(); ++i) {
            dir[i] = root + kNames[i];
            if (i && !root.empty()) CreateDirectoryW(dir[i].c_str(), nullptr);
        }
    }
};

const Folders& Get() {
    static const Folders folders;
    return folders;
}

std::string Narrow(const std::wstring& wide) {
    if (wide.empty()) return {};
    const int length = WideCharToMultiByte(CP_ACP, 0, wide.c_str(), static_cast<int>(wide.size()), nullptr, 0, nullptr, nullptr);
    std::string out(static_cast<size_t>(length), '\0');
    WideCharToMultiByte(CP_ACP, 0, wide.c_str(), static_cast<int>(wide.size()), out.data(), length, nullptr, nullptr);
    return out;
}

bool Exists(const std::wstring& path) { return GetFileAttributesW(path.c_str()) != INVALID_FILE_ATTRIBUTES; }

// red4ext\plugins\ -- the folder this plugin's folder is in.
std::wstring PluginsFolder() {
    std::wstring root = Dir(Folder::Root);
    if (root.size() < 2) return {};
    root.pop_back();
    const size_t slash = root.find_last_of(L'\\');
    return slash == std::wstring::npos ? std::wstring() : root.substr(0, slash + 1);
}

bool SamePath(const std::wstring& a, const std::wstring& b) {
    return CompareStringOrdinal(a.c_str(), static_cast<int>(a.size()), b.c_str(), static_cast<int>(b.size()), TRUE) == CSTR_EQUAL;
}

// Both files read whole: what migrates is a settings file or a pose, a few kilobytes.
bool SameContent(const std::wstring& a, const std::wstring& b) {
    auto read = [](const std::wstring& path, std::vector<char>& out) {
        const HANDLE file = CreateFileW(path.c_str(), GENERIC_READ, FILE_SHARE_READ, nullptr, OPEN_EXISTING, 0, nullptr);
        if (file == INVALID_HANDLE_VALUE) return false;
        LARGE_INTEGER size{};
        DWORD got = 0;
        const bool ok = GetFileSizeEx(file, &size) && size.QuadPart <= (1 << 20) &&
                        (out.resize(static_cast<size_t>(size.QuadPart)), true) &&
                        ReadFile(file, out.data(), static_cast<DWORD>(out.size()), &got, nullptr) && got == out.size();
        CloseHandle(file);
        return ok;
    };
    std::vector<char> x, y;
    return read(a, x) && read(b, y) && x == y;
}

void MoveOne(const std::wstring& from, const std::wstring& to) {
    if (!Exists(from)) return;
    if (Exists(to)) {
        // The same bytes are already in place (the release ships them there): the old copy is only clutter.
        if (SameContent(from, to) && DeleteFileW(from.c_str()))
            Log("[paths] %ls removed: the same file is in place at %ls\n", from.c_str(), to.c_str());
        else
            Log("[paths] %ls kept where it is: %ls is already in place\n", from.c_str(), to.c_str());
        return;
    }
    if (MoveFileExW(from.c_str(), to.c_str(), MOVEFILE_COPY_ALLOWED | MOVEFILE_WRITE_THROUGH))
        Log("[paths] moved %ls -> %ls\n", from.c_str(), to.c_str());
    else
        Log("[paths] could not move %ls -> %ls (error %lu)\n", from.c_str(), to.c_str(), GetLastError());
}

}  // namespace

const std::wstring& Dir(Folder folder) { return Get().dir[static_cast<size_t>(folder)]; }
std::wstring File(Folder folder, const wchar_t* name) { return Dir(folder) + name; }
std::string DirA(Folder folder) { return Narrow(Dir(folder)); }
std::string FileA(Folder folder, const char* name) { return DirA(folder) + name; }

void MigrateLegacyLayout() {
    static std::once_flag once;
    std::call_once(once, [] {
        const std::wstring exe = FolderOf(nullptr);
        if (exe.empty() || Dir(Folder::Root).empty()) return;
        for (const wchar_t* name : {L"vrport.ini", L"vrport-launcher.ini"})
            MoveOne(exe + name, File(Folder::Config, name));
        WIN32_FIND_DATAW found{};
        const HANDLE find = FindFirstFileW((exe + L"CyberpunkVR_*Grip_*.ini").c_str(), &found);
        if (find != INVALID_HANDLE_VALUE) {
            do {
                if (!(found.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY))
                    MoveOne(exe + found.cFileName, File(Folder::Grips, found.cFileName));
            } while (FindNextFileW(find, &found));
            FindClose(find);
        }
        const std::wstring plugins = PluginsFolder();
        const std::wstring legacy = plugins.empty() ? std::wstring() : plugins + kLegacyPlugin + L"\\";
        if (!legacy.empty() && !SamePath(legacy, Dir(Folder::Root)))
            MoveOne(legacy + L"vrik_calibration.ini", File(Folder::Config, L"vrik_calibration.ini"));
    });
}

std::wstring OtherCopy() {
    const std::wstring plugins = PluginsFolder();
    if (plugins.empty()) return {};
    WIN32_FIND_DATAW found{};
    const HANDLE find = FindFirstFileW((plugins + L"*").c_str(), &found);
    if (find == INVALID_HANDLE_VALUE) return {};
    std::wstring other;
    do {
        if (!(found.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) || found.cFileName[0] == L'.') continue;
        const std::wstring folder = plugins + found.cFileName + L"\\";
        if (SamePath(folder, Dir(Folder::Root))) continue;
        for (const wchar_t* name : kModuleNames)
            if (other.empty() && Exists(folder + name)) other = folder + name;
    } while (other.empty() && FindNextFileW(find, &found));
    FindClose(find);
    return other;
}

std::wstring RetireOldName() {
    const std::wstring root = Dir(Folder::Root);
    if (root.empty()) return {};
    const std::wstring old = root + kModuleNames[1];
    if (!Exists(old)) return {};
    // RED4ext usually comes to this file first (NTFS lists cyberpunkvrport.dll before CyberpunkVR_Stereo.dll).
    // Renamed before it loads, the older copy never does and this one carries on; already loaded, it runs this
    // launch -- a loaded module's file can be renamed, not deleted -- and the next start loads this one alone.
    const bool loaded = GetModuleHandleW(old.c_str()) != nullptr;
    const std::wstring retired = old + L".old";
    DeleteFileW(retired.c_str());
    if (!MoveFileExW(old.c_str(), retired.c_str(), MOVEFILE_REPLACE_EXISTING)) {
        Log("[paths] %ls is an older copy of this plugin and could not be renamed (%lu)\n", old.c_str(), GetLastError());
        return old;
    }
    Log("[paths] %ls (an older copy of this plugin) renamed to .old%ls\n", old.c_str(),
        loaded ? L"; it is already loaded, so this copy stays off for this launch" : L"");
    return loaded ? old : std::wstring{};
}

}  // namespace cvr::paths
