#pragma once
#include <algorithm>
#include <cmath>
#include <cstdint>
#include <iomanip>
#include <limits>
#include <locale>
#include <optional>
#include <sstream>
#include <string>
#include <string_view>

namespace profiler {
inline double cpuPercent(uint64_t current, uint64_t previous, double seconds) {
    if (current < previous || seconds <= 0) return 0;
    return static_cast<double>(current - previous) / (seconds * 100000.0);
}
inline std::string number(double value, int precision = 3) {
    std::ostringstream s;
    s.imbue(std::locale::classic());
    s << std::fixed << std::setprecision(precision) << value;
    return s.str();
}
inline std::string optionalNumber(const std::optional<double>& value) {
    return value ? number(*value) : "";
}
inline std::string csv(std::string_view value) {
    std::string out = "\"";
    for (char c : value) {
        if (c == '"') out += '"';
        // Keep one physical line per record, including thread descriptions.
        out += (c == '\r' || c == '\n' || c == '\t') ? ' ' : c;
    }
    return out + '"';
}
struct GpuInstance {
    std::optional<uint32_t> pid;
    std::string adapter;
    std::string engine;
};
inline GpuInstance parseGpuInstance(std::string_view name) {
    GpuInstance result;
    if (name.starts_with("pid_")) {
        auto end = name.find('_', 4);
        if (end == std::string_view::npos || end == 4) return result;
        uint64_t pid = 0;
        for (size_t i = 4; i < end; ++i) {
            if (name[i] < '0' || name[i] > '9') return result;
            pid = pid * 10 + static_cast<unsigned>(name[i] - '0');
            if (pid > std::numeric_limits<uint32_t>::max()) return result;
        }
        result.pid = static_cast<uint32_t>(pid);
    }
    const auto luid = name.find("luid_");
    if (luid == std::string_view::npos) return result;
    const auto engine = name.find("_eng_", luid);
    result.adapter = name.substr(luid, engine == std::string_view::npos ? name.size() - luid : engine - luid);
    if (engine != std::string_view::npos) result.engine = name.substr(engine + 1);
    return result;
}
// Missing/invalid samples remain missing. In particular, absence of a WDDM
// instance is not evidence of zero VRAM or GPU usage.
struct Aggregate {
    std::optional<double> value;
    bool invalid = false;
    void add(std::optional<double> sample, bool maximum = false) {
        if (!sample || !std::isfinite(*sample) || *sample < 0) { invalid = true; return; }
        if (!value) value = *sample;
        else if (maximum) value = std::max(*value, *sample);
        else *value += *sample;
    }
    std::optional<double> complete() const { return invalid ? std::nullopt : value; }
    std::string status(bool available) const {
        if (!available) return "unavailable";
        if (invalid) return "invalid_or_partial";
        return value ? "ok" : "no_instance";
    }
};
}
