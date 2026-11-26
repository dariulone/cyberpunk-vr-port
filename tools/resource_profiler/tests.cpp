#include "metrics.hpp"
#include <iostream>
#include <stdexcept>

static void check(bool condition, const char* message) {
    if (!condition) throw std::runtime_error(message);
}
int main() {
    using namespace profiler;
    try {
        check(cpuPercent(10000000, 0, 1) == 100, "one full CPU core");
        check(cpuPercent(20000000, 0, 1) == 200, "process can use two cores");
        check(cpuPercent(10000000, 0, 2) == 50, "actual elapsed interval");
        check(cpuPercent(1, 2, 1) == 0 && cpuPercent(1, 0, 0) == 0, "counter reset / zero interval");
        auto a = parseGpuInstance("pid_123_luid_0x00000000_0x0000abcd_phys_1_eng_2_engtype_Copy");
        check(a.pid == 123u && a.adapter == "luid_0x00000000_0x0000abcd_phys_1" && a.engine == "eng_2_engtype_Copy", "GPU attribution");
        auto b = parseGpuInstance("luid_0x00000000_0x0000abcd_phys_0");
        check(!b.pid && !b.adapter.empty() && b.engine.empty(), "adapter memory has no PID");
        check(!parseGpuInstance("pid_4294967296_luid_0x0").pid, "PID overflow");
        check(!parseGpuInstance("pid_bad_luid_0x0").pid, "invalid PID");
        Aggregate total, busy, missing;
        total.add(4); total.add(8);
        busy.add(40, true); busy.add(80, true);
        check(total.complete() == 12 && busy.complete() == 80, "memory sum versus busiest engine");
        check(!missing.complete() && missing.status(true) == "no_instance", "missing is not zero");
        total.add(std::nullopt);
        check(!total.complete() && total.status(true) == "invalid_or_partial", "partial GPU data not silently undercounted");
        check(missing.status(false) == "unavailable", "unsupported GPU counters");
        check(csv("a,\"b\"\nc") == "\"a,\"\"b\"\" c\"", "CSV escaping");
        std::cout << "All profiler metric tests passed.\n";
        return 0;
    } catch (const std::exception& e) { std::cerr << e.what() << '\n'; return 1; }
}
