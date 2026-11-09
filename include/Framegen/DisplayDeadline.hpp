#pragma once
#include <cmath>
#include <initializer_list>
#include <optional>
#include <windows.h>
namespace cvr::framegen {
inline double DeadlineQpcMs(){
    static const double scale=[] {LARGE_INTEGER f{};QueryPerformanceFrequency(&f);return 1000.0/double(f.QuadPart);}();
    LARGE_INTEGER t{};QueryPerformanceCounter(&t);return double(t.QuadPart)*scale;
}
// GetFrameTimeRemaining can roll into the next frame before zero. These are
// runtime-reported boundary/setpoint estimates, NOT photon or scanout receipts.
struct DisplayDeadline {
    double probeMs{},probeWidthMs{},remainingMs{},marginMs{},periodMs{};
    double handoffMs{},returnedMs{};
    unsigned kind{}; // SteamVrPacer::Kind: real, synthetic, repeat
    bool accepted{},sealed{};
};
struct DisplayAttribution {
    double boundaryMs{},targetMs{},gpuFromHandoffMs{},gpuFromBoundaryMs{},handoffFromTargetMs{},gpuFromTargetMs{};
    bool futureBoundary{};
};
inline std::optional<DisplayAttribution> AttributeDisplay(const DisplayDeadline& d,double gpuEndMs){
    for(double v:{d.probeMs,d.probeWidthMs,d.remainingMs,d.marginMs,d.periodMs,d.handoffMs,d.returnedMs,gpuEndMs})
        if(!std::isfinite(v))return {};
    if(!d.sealed || !d.accepted || d.kind>2 || d.probeMs<=0 || d.probeWidthMs<0 || d.probeWidthMs>.25 ||
       d.periodMs<4 || d.periodMs>40 || d.remainingMs<=0 || d.remainingMs>d.periodMs*4 ||
       d.marginMs<=0 || d.marginMs>=d.periodMs || d.handoffMs<d.probeMs || d.returnedMs<d.handoffMs ||
       gpuEndMs<=0 || std::abs(gpuEndMs-d.handoffMs)>1000)return {};
    const auto boundary=d.probeMs+d.remainingMs,target=boundary-d.marginMs;
    return DisplayAttribution{boundary,target,gpuEndMs-d.handoffMs,gpuEndMs-boundary,
        d.handoffMs-target,gpuEndMs-target,d.remainingMs>d.periodMs};
}
}
