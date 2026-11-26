// The HUD's element layout (src/Runtimes/HudLayout.cpp, or with CVR_RUST_VRIK rust/crates/cvr-settings/src/hud_layout.rs):
// hud_element_<key> lines parsed -- every key, prefix collisions, blanks, bad separators, short and extra fields,
// out-of-range, infinite and not-a-number values, other keys and other cases -- the settings set and read back
// sanitised, the status set and read back, and the settings written to a file. "trace" prints every parse's result
// and the file's bytes, so the C++ build and the Rust build can be compared byte for byte (cmake/ParityTrace.cmake).
#include "Runtimes/HudLayout.hpp"
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <stdexcept>
#include <string>
#include <vector>

#if CVR_RUST_VRIK
// cvr_rust.lib's settings parser reaches the C++ subsystems through this adapter (src/Core/LiveControlsParse.cpp).
extern "C" bool cvr_cpp_settings_parse_subsystem(int, const char*, void*) { return false; }
#endif

namespace {
uint64_t g_state = 0x2545F4914F6CDD1Dull;
uint64_t Next() {
    g_state ^= g_state << 13;
    g_state ^= g_state >> 7;
    g_state ^= g_state << 17;
    return g_state;
}
std::string Number() {
    static const char* special[] = {"0", "-0", "1e9", "-1e9", "inf", "-inf", "nan", "1.5e", "0x10", "3.", ".5", "-.25", "+7"};
    char text[64];
    switch (Next() % 6) {
        case 0: return special[Next() % (sizeof(special) / sizeof(*special))];
        case 1: std::snprintf(text, sizeof(text), "%d", int(Next() % 401) - 200); return text;
        default: std::snprintf(text, sizeof(text), "%.*f", int(Next() % 5), (double(Next() % 200001) - 100000.0) / 997.0); return text;
    }
}
std::string Blank() { static const char* b[] = {"", " ", "  ", "\t", " \t "}; return b[Next() % 5]; }
std::string Line() {
    std::string key;
    const auto pick = Next() % 10;
    if (pick < 7) key = std::string("hud_element_") + cvr::hud::Elements[Next() % cvr::hud::ElementCount].key;
    else if (pick == 7) key = "HUD_ELEMENT_" + std::string(cvr::hud::Elements[Next() % cvr::hud::ElementCount].key);
    else if (pick == 8) key = "hud_element_" + std::string(cvr::hud::Elements[Next() % cvr::hud::ElementCount].key) + "x";
    else key = "hud_elementminimap";
    std::string line = key + Blank() + (Next() % 9 == 0 ? ":" : "=") + Blank();
    const unsigned fields = Next() % 8 == 0 ? unsigned(Next() % 7) : 5;
    for (unsigned i = 0; i < fields; ++i) {
        if (i) line += Blank() + (Next() % 11 == 0 ? ";" : ",") + Blank();
        line += i == 4 && Next() % 2 ? std::to_string(int(Next() % 5) - 2) : Number();
    }
    if (Next() % 7 == 0) line += " trailing";
    return line;
}
void Print(const cvr::hud::LayoutSettings& s, bool trace) {
    if (!trace) return;
    for (size_t i = 0; i < s.size(); ++i)
        std::printf(" %zu:%.9g/%.9g/%.9g/%.9g/%d", i, s[i].x, s[i].y, s[i].scale, s[i].opacity, s[i].visible);
    std::printf("\n");
}
void Check(bool v, const char* text) { if (!v) throw std::runtime_error(text); }
}  // namespace

int main(int argc, char** argv) {
    const bool trace = argc > 1 && std::strcmp(argv[1], "trace") == 0;
    try {
        cvr::hud::LayoutSettings settings{};
        unsigned taken = 0;
        for (int i = 0; i < 3000; ++i) {
            const auto line = Line();
            const bool ok = cvr::hud::ParseLayoutSetting(line.c_str(), settings);
            taken += ok;
            if (trace) std::printf("%d %d %s\n", i, ok ? 1 : 0, line.c_str());
            if (trace && i % 100 == 99) Print(settings, true);
        }
        Check(taken > 1000, "too few lines taken");
        // Every key, exactly.
        for (size_t i = 0; i < cvr::hud::ElementCount; ++i) {
            const std::string line = std::string("hud_element_") + cvr::hud::Elements[i].key + "=1.25,-2.5,1.5,0.5,1";
            Check(cvr::hud::ParseLayoutSetting(line.c_str(), settings), "a key not taken");
            Check(settings[i].scale == 1.5f && settings[i].x == 1.25f, "a key's values");
        }
        Print(settings, trace);
        // Set and read back: sanitised.
        for (auto& s : settings) { s.x = float(Next() % 1000) - 500; s.scale = float(Next() % 100) / 10; s.opacity = -1; s.visible = int(Next() % 3) - 1; }
        settings[3].y = std::numeric_limits<float>::quiet_NaN();
        cvr::hud::SetLayoutSettings(settings);
        const auto back = cvr::hud::GetLayoutSettings();
        Print(back, trace);
        Check(back[0].opacity == 0, "sanitised opacity");
        cvr::hud::LayoutStatus status{};
        for (size_t i = 0; i < status.size(); ++i) status[i] = {i % 2 == 0, float(i), float(i * 2), float(i * 3), float(i * 4)};
        cvr::hud::SetLayoutStatus(status);
        const auto statusBack = cvr::hud::GetLayoutStatus();
        for (size_t i = 0; i < status.size(); ++i)
            Check(statusBack[i].available == status[i].available && statusBack[i].height == status[i].height, "status");
        // Written as fprintf writes it (text mode: CRLF).
        char path[L_tmpnam_s];
        Check(tmpnam_s(path, sizeof(path)) == 0, "tmpnam");
        FILE* file = nullptr;
        Check(fopen_s(&file, path, "w") == 0 && file, "open");
        cvr::hud::SaveLayoutSettings(file, back);
        std::fclose(file);
        Check(fopen_s(&file, path, "rb") == 0 && file, "reopen");
        std::string bytes;
        char buffer[4096];
        size_t n;
        while ((n = std::fread(buffer, 1, sizeof(buffer), file)) > 0) bytes.append(buffer, n);
        std::fclose(file);
        std::remove(path);
        Check(bytes.find("hud_element_health=") == 0 && bytes.find("\r\n") != std::string::npos, "file");
        if (trace) std::printf("%s", bytes.c_str());
    } catch (const std::exception& e) {
        std::printf("FAILED: %s\n", e.what());
        return 1;
    }
    if (!trace) std::printf("hud layout: lines, settings, status and file checked\n");
    return 0;
}
