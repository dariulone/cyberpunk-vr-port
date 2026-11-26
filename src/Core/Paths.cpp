// The plugin's folders, owned by Rust since stage 2 (rust/crates/cvr-host/src/paths.rs: the folder this module was
// loaded from, the subfolders made on first use, the legacy-layout migration and the other-copy checks, which only
// the Rust entry points call). These are the C++ half's views of the same folders.
#include "Core/Paths.hpp"

#include "Rust/CvrRust.h"

#include <array>
#include <mutex>

namespace cvr::paths {
namespace {

constexpr size_t kFolders = 9;

const std::array<std::wstring, kFolders>& Wide() {
    static const std::array<std::wstring, kFolders> dirs = [] {
        std::array<std::wstring, kFolders> out;
        for (size_t i = 0; i < kFolders; ++i) out[i] = reinterpret_cast<const wchar_t*>(cvr_paths_dir_w(uint32_t(i)));
        return out;
    }();
    return dirs;
}

}  // namespace

const std::wstring& Dir(Folder folder) { return Wide()[static_cast<size_t>(folder)]; }
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
std::wstring File(Folder folder, const wchar_t* name) { return Dir(folder) + name; }
#endif
std::string DirA(Folder folder) { return cvr_paths_dir_a(static_cast<uint32_t>(folder)); }
std::string FileA(Folder folder, const char* name) { return DirA(folder) + name; }

}  // namespace cvr::paths
