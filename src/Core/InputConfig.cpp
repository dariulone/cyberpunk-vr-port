#include "Core/InputConfig.hpp"
#include "Core/Paths.hpp"
#include "Stereo/EngineRvas.hpp"

#include <windows.h>

#include <pugixml.hpp>

#include <algorithm>
#include <cstring>
#include <filesystem>
#include <string>
#include <vector>

extern void Log(const char* fmt, ...);

namespace cvr::input_config {
namespace {

namespace fs = std::filesystem;

// Which file each kind of <bindings> child belongs in, and the attribute that names it (Input Loader's rules: a
// blend has none and is always added).
struct Kind { const char* node; bool mappings; const char* key; };
constexpr Kind kKinds[] = {
    {"blend", false, nullptr},       {"context", false, "name"},      {"hold", false, "action"},
    {"multitap", false, "action"},   {"repeat", false, "action"},     {"toggle", false, "action"},
    {"acceptedEvents", false, "action"},
    {"mapping", true, "name"},       {"buttonGroup", true, "id"},     {"pairedAxes", true, "name"},
    {"preset", true, "name"},
};

const Kind* KindOf(const char* node) {
    for (const Kind& kind : kKinds)
        if (std::strcmp(kind.node, node) == 0) return &kind;
    return nullptr;
}

// <game>\ from <game>\bin\x64\Cyberpunk2077.exe.
fs::path GameRoot() {
    std::vector<wchar_t> exe(32768);
    const DWORD length = GetModuleFileNameW(nullptr, exe.data(), static_cast<DWORD>(exe.size()));
    if (!length || length >= exe.size()) return {};
    return fs::path(std::wstring(exe.data(), length)).parent_path().parent_path().parent_path();
}

std::vector<fs::path> PortFiles() {
    std::vector<fs::path> files;
    std::error_code error;
    for (const auto& entry : fs::directory_iterator(fs::path(paths::Dir(paths::Folder::Config)) / L"input", error))
        if (entry.is_regular_file() && entry.path().extension() == L".xml") files.push_back(entry.path());
    std::sort(files.begin(), files.end());
    return files;
}

// Input Loader merges r6\input\: the port's files are put there, as they are, and the variables are left to it.
void HandToInputLoader(const fs::path& root, const std::vector<fs::path>& files) {
    std::error_code error;
    const fs::path target = root / L"r6" / L"input";
    fs::create_directories(target, error);
    for (const fs::path& file : files) {
        const fs::path to = target / file.filename();
        if (fs::exists(to, error) && fs::file_size(to, error) == fs::file_size(file, error)) {
            std::string a, b;
            auto read = [](const fs::path& p, std::string& out) {
                FILE* f = _wfopen(p.c_str(), L"rb");
                if (!f) return;
                char buffer[4096];
                for (size_t n; (n = std::fread(buffer, 1, sizeof(buffer), f)) > 0;) out.append(buffer, n);
                std::fclose(f);
            };
            read(file, a);
            read(to, b);
            if (a == b) continue;
        }
        fs::copy_file(file, to, fs::copy_options::overwrite_existing, error);
        Log("[input] %ls -> %ls for Input Loader%s\n", file.filename().c_str(), to.c_str(), error ? " FAILED" : "");
    }
}

// One port file into the two documents. Returns the nodes merged.
int Merge(const fs::path& file, pugi::xml_node contexts, pugi::xml_node mappings) {
    pugi::xml_document document;
    const pugi::xml_parse_result parsed = document.load_file(file.c_str());
    const pugi::xml_node bindings = document.child("bindings");
    if (!parsed || !bindings) {
        Log("[input] %ls: not a <bindings> document (%s)\n", file.filename().c_str(), parsed.description());
        return 0;
    }
    int merged = 0;
    for (pugi::xml_node node : bindings.children()) {
        if (node.type() != pugi::node_element) continue;
        const Kind* kind = KindOf(node.name());
        if (!kind) {
            Log("[input] %ls: <%s> is no input binding, skipped\n", file.filename().c_str(), node.name());
            continue;
        }
        pugi::xml_node target = kind->mappings ? mappings : contexts;
        const pugi::xml_attribute id = kind->key ? node.attribute(kind->key) : pugi::xml_attribute();
        pugi::xml_node existing = id ? target.find_child_by_attribute(node.name(), kind->key, id.value()) : pugi::xml_node();
        if (existing && node.attribute("append").as_bool()) {
            for (pugi::xml_node child : node.children()) existing.append_copy(child);
        } else {
            if (existing) target.remove_child(existing);
            target.append_copy(node);
        }
        ++merged;
    }
    return merged;
}

// A string config variable, checked to be the one named before its own setter is called.
bool SetVariable(uintptr_t rva, const char* name, const std::string& value) {
    const auto base = reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    auto* variable = reinterpret_cast<uint8_t*>(base + rva);
    const auto vtable = *reinterpret_cast<uintptr_t* const*>(variable);
    const char* variableName = *reinterpret_cast<const char* const*>(variable + 8);
    const char* group = *reinterpret_cast<const char* const*>(variable + 16);
    if (reinterpret_cast<uintptr_t>(vtable) != base + detail::CONFIG_STRING_VAR_VTABLE_RVA || !variableName || !group ||
        std::strcmp(variableName, name) != 0 || std::strcmp(group, "Player/Input") != 0) {
        Log("[input] config variable %s not where this build keeps it: left alone\n", name);
        return false;
    }
    struct StringView { const char* text; uint32_t length; } view{value.c_str(), static_cast<uint32_t>(value.size())};
    using SetFromString = bool(__fastcall*)(void*, const StringView*);
    const bool ok = reinterpret_cast<SetFromString>(vtable[3])(variable, &view);
    Log("[input] Player/Input %s = %s%s\n", name, value.c_str(), ok ? "" : " (REFUSED)");
    return ok;
}

}  // namespace

void Apply() {
    LARGE_INTEGER started{}, frequency{};
    QueryPerformanceCounter(&started);
    QueryPerformanceFrequency(&frequency);
    const fs::path root = GameRoot();
    const std::vector<fs::path> files = PortFiles();
    if (root.empty() || files.empty()) return;
    std::error_code error;
    if (fs::exists(fs::path(paths::Dir(paths::Folder::Root)).parent_path().parent_path() / L"input_loader" / L"input_loader.dll",
                   error)) {
        Log("[input] Input Loader is installed: it merges the port's bindings (r6\\input)\n");
        HandToInputLoader(root, files);
        return;
    }
    // Input Loader writes its ini at run time, so removing the mod leaves it behind -- and the engine reads it after
    // this, which would point both variables back at Input Loader's last, stale merge. It is set aside, not deleted.
    const fs::path orphan = root / L"engine" / L"config" / L"platform" / L"pc" / L"input_loader.ini";
    if (fs::exists(orphan, error)) {
        fs::path aside = orphan;
        aside += L".orphaned";
        fs::rename(orphan, aside, error);
        Log("[input] Input Loader is gone but its ini was left: moved to %ls%s\n", aside.filename().c_str(),
            error ? " FAILED" : "");
    }
    pugi::xml_document contexts, mappings;
    const fs::path vanilla = root / L"r6" / L"config";
    if (!contexts.load_file((vanilla / L"inputContexts.xml").c_str()) ||
        !mappings.load_file((vanilla / L"inputUserMappings.xml").c_str()) || !contexts.child("bindings") ||
        !mappings.child("bindings")) {
        Log("[input] the game's r6\\config input files could not be read: the port's bindings are not added\n");
        return;
    }
    int merged = 0;
    for (const fs::path& file : files) merged += Merge(file, contexts.child("bindings"), mappings.child("bindings"));
    // Written beside the port's own files; the variables take the path relative to r6\, like their defaults.
    const fs::path out = fs::path(paths::Dir(paths::Folder::Config)) / L"input" / L"merged";
    fs::create_directories(out, error);
    const fs::path contextsFile = out / L"inputContexts.xml", mappingsFile = out / L"inputUserMappings.xml";
    if (!contexts.save_file(contextsFile.c_str(), "\t", pugi::format_default, pugi::encoding_utf8) ||
        !mappings.save_file(mappingsFile.c_str(), "\t", pugi::format_default, pugi::encoding_utf8)) {
        Log("[input] could not write the merged input files to %ls\n", out.c_str());
        return;
    }
    const fs::path r6 = root / L"r6";
    const std::string contextsPath = fs::relative(contextsFile, r6, error).string();
    const std::string mappingsPath = fs::relative(mappingsFile, r6, error).string();
    if (error || contextsPath.empty() || mappingsPath.empty()) return;
    LARGE_INTEGER done{};
    QueryPerformanceCounter(&done);
    Log("[input] %d bindings from %zu file(s) merged into the game's input configuration (%.1f ms)\n", merged,
        files.size(), double(done.QuadPart - started.QuadPart) * 1000.0 / double(frequency.QuadPart));
    SetVariable(detail::INPUT_CONTEXT_FILE_VAR_RVA, "InputContextFile", contextsPath);
    SetVariable(detail::INPUT_MAPPING_FILE_VAR_RVA, "InputMappingFile", mappingsPath);
}

}  // namespace cvr::input_config
