#include "Runtimes/CyberFingerInput.hpp"
#include "Core/Paths.hpp"
#include <atomic>
#include <cstdio>

extern void Log(const char*, ...);
namespace cvr::cyberfinger {
namespace { std::atomic<bool> g_enabled{false}; }
void Configure() {
    FILE* file = nullptr;
    int enabled = 0;
    const auto path = cvr::paths::FileA(cvr::paths::Folder::Config, "cyberfinger.ini");
    if (fopen_s(&file, path.c_str(), "r") == 0 && file) {
        char line[256];
        while (fgets(line, sizeof(line), file)) {
            int value = 0;
            if (sscanf_s(line, " enabled = %d", &value) == 1) enabled = value;
        }
        fclose(file);
    }
    g_enabled.store(enabled == 1, std::memory_order_relaxed);
    Log("[cyberfinger] profile %s (config/cyberfinger.ini; restart to change)\n", enabled == 1 ? "enabled" : "disabled");
}
bool Enabled() { return g_enabled.load(std::memory_order_relaxed); }
}
