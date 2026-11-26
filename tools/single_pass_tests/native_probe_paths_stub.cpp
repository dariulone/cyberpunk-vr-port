// native_probe_test runs without the plugin's Rust host: its folders are empty (no config or log file is read).
#include "Core/Paths.hpp"

namespace cvr::paths {
const std::wstring& Dir(Folder) {
    static const std::wstring none;
    return none;
}
}  // namespace cvr::paths
