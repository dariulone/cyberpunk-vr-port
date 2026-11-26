// The frame generation's statistics panel (cvr_framegen_overlay_draw: src/Framegen/StatsOverlayDraw.cpp, or with
// CVR_RUST_VRIK rust/crates/cvr-gpu/src/framegen_overlay.rs) drawn for pseudo-random statistics in the three layouts:
// rates, frame times and their graphs, percentiles, unknown values ("--"), the hardware's load, memory, cores and names,
// the generator's three backends and its absence. Checked: the panel's corners, border and body, and text drawn.
// "trace" prints a hash of every panel's pixels, so the C++ build and the Rust build can be compared pixel for pixel
// (cmake/ParityTrace.cmake).
#include "Framegen/Framegen.hpp"
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <limits>
#include <stdexcept>
#include <vector>

extern "C" void cvr_framegen_overlay_draw(uint32_t* pixels, int width, int height, const cvr::framegen::Settings* settings,
                                          const cvr::framegen::Statistics* stats, uint32_t hour, uint32_t minute, int backend);

namespace {
uint64_t g_state = 0x9E3779B97F4A7C15ull;
uint64_t Next() {
    g_state ^= g_state << 13;
    g_state ^= g_state >> 7;
    g_state ^= g_state << 17;
    return g_state;
}
double Uniform(double lo, double hi) { return lo + (hi - lo) * double(Next() >> 11) * (1.0 / 9007199254740992.0); }
// A value that is sometimes unknown: negative, not a number, or zero.
double Sometimes(double lo, double hi) {
    switch (Next() % 8) {
        case 0: return -1;
        case 1: return std::numeric_limits<double>::quiet_NaN();
        case 2: return 0;
        default: return Uniform(lo, hi);
    }
}
void Name(char* out, size_t size, const char* base) {
    if (Next() % 4 == 0) { out[0] = 0; return; }
    std::snprintf(out, size, "%s %u-%u", base, unsigned(Next() % 9000), unsigned(Next() % 90));
}

void Fill(cvr::framegen::Settings& s, cvr::framegen::Statistics& st, int seed) {
    s.overlayLayout = seed % 3;
    s.enabled = seed % 5 != 0;
    s.historySeconds = int(Uniform(5, 120));
    st.realFps = Uniform(20, 140); st.generatedFps = Uniform(0, 140); st.outputFps = Uniform(20, 280);
    st.cpuMs = Uniform(2, 40); st.gpuMs = Uniform(2, 40); st.generationMs = Uniform(0, 6);
    st.averageFps = Uniform(20, 140); st.minimumFps = Uniform(5, 60); st.maximumFps = Uniform(60, 300);
    st.low1Fps = Sometimes(5, 60); st.low01Fps = Sometimes(1, 40);
    st.cpuAverageMs = Uniform(2, 30); st.gpuAverageMs = Uniform(2, 30);
    st.cpuP99Ms = Sometimes(5, 60); st.cpuP999Ms = Sometimes(5, 90); st.gpuP99Ms = Sometimes(5, 60); st.gpuP999Ms = Sometimes(5, 90);
    st.elapsedSeconds = Uniform(0, 7200); st.budgetMs = Uniform(5, 30); st.repeatPercent = Uniform(0, 100);
    for (auto& v : st.cpuGraph) v = float(Uniform(0, 70));
    for (auto& v : st.gpuGraph) v = float(Uniform(0, 70));
    st.skipped = Next() % 100000; st.vramBytes = Next() % (3ull << 30);
    auto& h = st.hardware;
    h.cpuUsage = Sometimes(0, 100); h.gpuUsage = Sometimes(0, 100); h.gpuTemperature = Sometimes(30, 95);
    h.coreMaximum = Sometimes(0, 100);
    h.ramTotal = Next() % 2 ? (Next() % (64ull << 30)) : 0; h.ramUsed = h.ramTotal ? Next() % h.ramTotal : 0;
    h.gpuMemoryValid = Next() % 2; h.gpuMemoryTotal = Next() % 3 ? (Next() % (24ull << 30)) : 0; h.gpuMemoryUsed = Next() % (24ull << 30);
    h.gpuProcessMemoryValid = Next() % 2; h.gpuProcessUsed = Next() % (12ull << 30); h.appWorkingSet = Next() % (32ull << 30);
    h.coreCount = unsigned(Next() % 33);
    for (unsigned i = 0; i < h.coreCount && i < h.cores.size(); ++i) h.cores[i] = float(Uniform(-10, 120));
    Name(h.cpuName, sizeof(h.cpuName), "AMD Ryzen 9 7950X3D 16-Core Processor with a long name");
    Name(h.gpuName, sizeof(h.gpuName), "NVIDIA GeForce RTX 4090");
}

uint64_t Hash(const std::vector<uint32_t>& pixels) {
    uint64_t hash = 1469598103934665603ull;
    for (uint32_t p : pixels)
        for (int b = 0; b < 4; ++b) hash = (hash ^ ((p >> (8 * b)) & 0xFF)) * 1099511628211ull;
    return hash;
}
void Check(bool v, const char* text) { if (!v) throw std::runtime_error(text); }
}  // namespace

int main(int argc, char** argv) {
    const bool trace = argc > 1 && std::strcmp(argv[1], "trace") == 0;
    try {
        for (int seed = 0; seed < 90; ++seed) {
            cvr::framegen::Settings settings;
            cvr::framegen::Statistics stats;
            Fill(settings, stats, seed);
            const int width = settings.overlayLayout == 0 ? 720 : settings.overlayLayout == 1 ? 420 : 360;
            const int height = settings.overlayLayout == 0 ? 452 : settings.overlayLayout == 1 ? 404 : 222;
            std::vector<uint32_t> pixels(size_t(width) * height, 0x12345678u);
            cvr_framegen_overlay_draw(pixels.data(), width, height, &settings, &stats, unsigned(seed % 24), unsigned((seed * 7) % 60),
                                      seed % 3);
            Check(pixels[0] == 0, "corner");
            Check(pixels[size_t(100) * width + 1] == 0xFFA4A4A4u, "border");
            bool text = false;
            for (uint32_t p : pixels) text |= p == 0xFFF2F2F2u;
            Check(text, "text");
            if (trace) std::printf("%d %016llx\n", seed, static_cast<unsigned long long>(Hash(pixels)));
        }
    } catch (const std::exception& e) {
        std::printf("FAILED: %s\n", e.what());
        return 1;
    }
    if (!trace) std::printf("framegen overlay: 90 panels drawn\n");
    return 0;
}
