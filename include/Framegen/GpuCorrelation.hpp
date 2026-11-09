#pragma once
#include <cstdint>
#include <optional>
#include <cmath>

namespace cvr::framegen {
// A single queue's calibration, captured immediately before a sampled CPU
// ExecuteCommandLists call. D3D12 timestamps are bottom-of-pipe markers: start
// means the first timestamp reached the GPU, not the first shader instruction.
// Never compare GPU ticks from different queues or with steady_clock ticks.
struct GpuCorrelation {
    uint64_t gpu{},cpu{},submitted{},gpuHz{},cpuHz{};
    struct Result {double queueMs{},totalMs{};};
    static double Delta(uint64_t a,uint64_t b){return a>=b?double(a-b):-double(b-a);}
    std::optional<Result> Resolve(uint64_t start,uint64_t end) const {
        if(!submitted || !cpuHz || !gpuHz || end<start)return {};
        const double offset=Delta(submitted,cpu)*1000.0/double(cpuHz);
        Result r{Delta(start,gpu)*1000.0/double(gpuHz)-offset,
                 Delta(end,gpu)*1000.0/double(gpuHz)-offset};
        // Do not silently turn bad calibration/clock drift into a zero wait.
        if(!std::isfinite(r.queueMs)||!std::isfinite(r.totalMs)||r.queueMs<0 ||
           r.totalMs<r.queueMs || r.totalMs>1000)return {};
        return r;
    }
};
}
