#include "Framegen/Framegen.hpp"
#include "Framegen/FramePolicy.hpp"
#include "Render/Afw.hpp"
#include "Utils/DebugGate.hpp"
#include <windows.h>
#include <algorithm>
#include <array>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <mutex>
extern "C" {
__declspec(dllexport) char CyberpunkVR_FramegenReport[2048]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_FramegenReportSeq{};
// The frame cap (CapToDisplay): [0] frames held, [1] frames let through (the game at or below the refresh).
__declspec(dllexport) uint64_t CyberpunkVR_DebugFpsCap[2]{};
// Every frame's times for benchmarks (tools/vr_bench), written while the FPS metrics run: entry i of a ring is
// [i % 8192]; [0] counts real frames (CPU: the interval between real frames at the Present), [1] GPU frames (the
// game queue's span from the frame's first submission to its Present, GpuTimer.cpp). A reader takes the two counts
// at the start and the end of its window and reads the entries in between: no polling, no frame missed.
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_FrameRingCount[2]{};
__declspec(dllexport) float CyberpunkVR_FrameRingCpu[8192]{};
__declspec(dllexport) float CyberpunkVR_FrameRingGpu[8192]{};
// Bounded diagnostic ring: timestamp, lock wait, work estimate, old/new target, XR cycle, period,
// requested interval/hold, actual wait, release, reserved. Written only while runtime diagnostics are enabled.
__declspec(dllexport) double CyberpunkVR_PaceTrace[8192][12]{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_PaceTraceCount{};
// Presentation cycles whose real frame skipped a native frame of the game (diagnostics; pacing ignores them).
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_FramegenPairGaps{};
// 1 while CadenceLock holds real frames three display periods apart.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_FramegenThreePeriods{};
}

namespace cvr::framegen {
namespace {
std::mutex mutex;
std::mutex settingsWriteMutex;
Settings settings;
std::atomic<bool> enabled{false},metrics{false};
std::atomic<bool> depthCapture{false};
std::atomic<bool> autoPace{true},cadenceLock{false},pacingReady{false};
std::atomic<int64_t> displayPeriod{0};
// When xrWaitFrame last returned (NowMs): the phase of the display cycle the frame cap aims at.
std::atomic<double> displayCycleMs{0};
// Whether those returns keep the predicted period (CycleClock), so framegen's pacing may lock to them.
std::atomic<bool> cycleSteady{false};
std::atomic<bool> fpsCap{true};
std::atomic<double> pacingIntervalMs{0};
std::atomic<bool> resetPacing{true};
Statistics stats;
struct Sample {double stamp{},duration{};};
template<size_t N> struct Window {
    std::array<Sample,N> values{};
    size_t head{},count{};double started{};
    void Add(double now,double value=0) {if(!count)started=now;values[head]={now,value};head=(head+1)%N;count=std::min(count+1,N);}
    double Rate(double now) const {
        if(count<2)return 0;
        size_t used=0;double first=now,last=0;
        for(size_t i=0;i<count;++i) {
            first=std::min(first,values[i].stamp);last=std::max(last,values[i].stamp);
            if(values[i].stamp<=now && now-values[i].stamp<=2000)++used;
        }
        const double interval=(last-first)/(count-1);
        double window=std::min(2000.0,now-started+interval);
        // Menu rendering can fill the ring in less than two seconds. Preserve
        // that shorter measured window instead of capping its FPS at N/2.
        if(count==N)window=std::min(window,now-first+interval);
        // Use the full observation window for sparse events as well: two
        // repeats50ms apart in two seconds mean1 FPS, not20 FPS.
        return window>0 ? used*1000.0/window : 0;
    }
    double Peak(double now) const {
        double result=0;for(size_t i=0;i<count;++i)if(now-values[i].stamp<=2000)result=std::max(result,values[i].duration);
        return result;
    }
};
Window<512> real,generated,output,repeats,cpu,gpu;
FrameHistory<> cpuHistory,gpuHistory;
Statistics cached;
double nextRefresh{},nextSummary{},started{};
HistorySummary cpuSummary,gpuSummary;
uint64_t lastReal{};double lastRealTime{};
void ClearMetricsLocked() {
    real={};generated={};output={};repeats={};cpu={};gpu={};cpuHistory.Clear();gpuHistory.Clear();
    cpuSummary={};gpuSummary={};nextRefresh=nextSummary=0;started=0;lastReal=0;lastRealTime=0;
    const bool active=stats.active;char status[sizeof(stats.status)]{};std::memcpy(status,stats.status,sizeof(status));
    stats={};stats.active=active;std::memcpy(stats.status,status,sizeof(status));cached={};
}
}
double NowMs() {
    static const double scale=[] {LARGE_INTEGER f{};QueryPerformanceFrequency(&f);return 1000.0/double(f.QuadPart);}();
    LARGE_INTEGER v{};QueryPerformanceCounter(&v);return double(v.QuadPart)*scale;
}
Settings GetSettings() {std::lock_guard lock(mutex);return settings;}
namespace {std::atomic<int> activeBackend{-1};}
const char* BackendName(Backend value) {
    switch(value) {
        case Backend::Nvidia:return "NVIDIA OFA + FidelityFX";
        case Backend::DlssG:return "NVIDIA DLSS-G";
        case Backend::Anr:return "ANR FG";
        default:return "FidelityFX";
    }
}
Backend ActiveBackend() {const int v=activeBackend.load(std::memory_order_relaxed);return v<0 ? GetSettings().backend : Backend(v);}
void PublishActiveBackend(Backend value) {activeBackend.store(int(value),std::memory_order_relaxed);}
// Off for an AFW launch (Render/Afw.hpp): its pairs are one drawn eye and one warped, and the other eye's frame
// warp already stands in for a frame the engine did not draw.
bool Enabled() {return enabled.load(std::memory_order_relaxed) && !cvr::afw::Launch();}
bool DepthCaptureEnabled() {return depthCapture.load(std::memory_order_relaxed);}
// AFW (Render/Afw.hpp) warps with the frame's depth and motion.
bool CaptureInputsEnabled() {return Enabled() || DepthCaptureEnabled() || cvr::afw::Launch();}
bool MotionWanted() {return Enabled() || cvr::afw::Launch();}
void SetDepthCaptureEnabled(bool value) {depthCapture.store(value,std::memory_order_relaxed);}
bool MetricsEnabled() {return metrics.load(std::memory_order_relaxed);}
void SetSettings(Settings value) {
    std::lock_guard settingsLock(settingsWriteMutex);
    value.backend=Backend(std::clamp(int(value.backend),0,3));
    value.quality=std::clamp(value.quality,0,2);
    // 0 = no optical flow: frame interpolation on the engine's vectors alone.
    value.flowScale=value.flowScale==100 || value.flowScale==75 || value.flowScale==0 ? value.flowScale : 50;
    value.overlayCorner=std::clamp(value.overlayCorner,0,8);
    value.overlayLayout=std::clamp(value.overlayLayout,0,2);value.overlayFollow=value.overlayFollow!=0;
    value.historySeconds=value.historySeconds==10 || value.historySeconds==60 || value.historySeconds==120 ? value.historySeconds : 30;
    const auto bounded=[](float v,float lo,float hi,float fallback) {return std::isfinite(v) ? std::clamp(v,lo,hi) : fallback;};
    value.overlayCone=bounded(value.overlayCone,5,90,60);value.overlayScale=bounded(value.overlayScale,.5f,2,1);
    value.overlayDistance=bounded(value.overlayDistance,.5f,5,1.25f);
    value.overlayX=bounded(value.overlayX,-60,60,0);value.overlayY=bounded(value.overlayY,-45,45,0);
    bool metricsChanged=false;
    {std::lock_guard lock(mutex);
    metricsChanged=value.overlay!=settings.overlay;
    const bool disabledReportChanged=metricsChanged || value.enabled!=settings.enabled || value.backend!=settings.backend || !CyberpunkVR_FramegenReport[0];
    if(metricsChanged)ClearMetricsLocked();
    if(value.historySeconds!=settings.historySeconds)nextRefresh=nextSummary=0;
    settings=value;
    enabled.store(value.enabled,std::memory_order_relaxed);
    metrics.store(value.overlay,std::memory_order_relaxed);
    autoPace.store(value.autoPace,std::memory_order_relaxed);
    cadenceLock.store(value.cadenceLock,std::memory_order_relaxed);
    if(!value.enabled || !value.autoPace){pacingIntervalMs=0;resetPacing=true;}
    if(!value.enabled) {pacingReady.store(false,std::memory_order_relaxed);stats.generationMs=0;}
    if(cvr::RuntimeDiagnosticsEnabled() && !value.overlay && disabledReportChanged) {
        CyberpunkVR_FramegenReportSeq.fetch_add(1,std::memory_order_acq_rel);
        std::snprintf(CyberpunkVR_FramegenReport,sizeof(CyberpunkVR_FramegenReport),
            "{\"enabled\":%d,\"backend\":%d,\"metrics\":0,\"status\":\"FPS telemetry disabled\"}",value.enabled,int(value.backend));
        CyberpunkVR_FramegenReportSeq.fetch_add(1,std::memory_order_release);
    }
    }
    if(value.overlay)WakeHardwareTelemetry();else if(metricsChanged)StopHardwareTelemetry();
}
RuntimeStatus GetRuntimeStatus() {
    std::lock_guard lock(mutex);RuntimeStatus result{};result.active=stats.active;
    std::memcpy(result.text,stats.status,sizeof(result.text));return result;
}
Statistics GetStatistics() {
    std::lock_guard lock(mutex);
    if(!settings.overlay)return stats;
    const auto now=NowMs();
    if(now<nextRefresh)return cached;
    nextRefresh=now+250;auto result=stats;
    result.realFps=real.Rate(now);result.generatedFps=generated.Rate(now);
    result.outputFps=output.Rate(now);result.repeatFps=repeats.Rate(now);
    result.cpuPeakMs=cpu.Peak(now);result.gpuPeakMs=gpu.Peak(now);
    if(now>=nextSummary) {cpuSummary=cpuHistory.Summarize(now,settings.historySeconds);gpuSummary=gpuHistory.Summarize(now,settings.historySeconds);nextSummary=now+1000;}
    result.historySamples=cpuSummary.samples;result.cpuAverageMs=cpuSummary.average;result.gpuAverageMs=gpuSummary.average;
    result.averageFps=cpuSummary.average>0 ? 1000/cpuSummary.average : 0;result.low1Fps=cpuSummary.low1;result.low01Fps=cpuSummary.low01;
    result.minimumFps=cpuSummary.peak>0 ? 1000/cpuSummary.peak : 0;
    result.maximumFps=cpuSummary.minimum>0 ? 1000/cpuSummary.minimum : 0;
    result.cpuP99Ms=cpuSummary.p99;result.cpuP999Ms=cpuSummary.p999;result.gpuP99Ms=gpuSummary.p99;result.gpuP999Ms=gpuSummary.p999;
    result.cpuGraph=cpuHistory.Graph(now,settings.historySeconds);result.gpuGraph=gpuHistory.Graph(now,settings.historySeconds);
    result.hardware=GetHardwareStatistics();result.elapsedSeconds=started ? (now-started)/1000 : 0;
    result.budgetMs=double(displayPeriod.load(std::memory_order_relaxed))*1e-6*(settings.enabled?2:1);
    if(settings.enabled && settings.autoPace)result.budgetMs=std::max(result.budgetMs,pacingIntervalMs.load(std::memory_order_relaxed));
    if(result.budgetMs<=0)result.budgetMs=1000.0/90;
    const auto submissions=result.realSubmissions+result.generatedFrames+result.repeatedFrames;
    result.repeatPercent=submissions ? 100.0*result.repeatedFrames/submissions : 0;
    if(cvr::RuntimeDiagnosticsEnabled()) {
    CyberpunkVR_FramegenReportSeq.fetch_add(1,std::memory_order_acq_rel);
    std::snprintf(CyberpunkVR_FramegenReport,sizeof(CyberpunkVR_FramegenReport),
        "{\"enabled\":%d,\"active\":%d,\"backend\":%d,\"metrics\":1,\"real_fps\":%.3f,\"fg_fps\":%.3f,\"output_fps\":%.3f,\"repeats_fps\":%.3f,"
        "\"cpu_ms\":%.3f,\"gpu_ms\":%.3f,\"generation_ms\":%.3f,\"generation_span_ms\":%.3f,\"pace_ms\":%.3f,\"real\":%llu,\"generated\":%llu,\"real_submitted\":%llu,\"repeated\":%llu,\"skipped\":%llu,\"vram_bytes\":%llu,"
        "\"inputs\":[%llu,%llu],\"average_fps\":%.3f,\"minimum_fps\":%.3f,\"maximum_fps\":%.3f,\"low1\":%.3f,\"low01\":%.3f,\"cpu_usage\":%.2f,\"gpu_usage\":%.2f,\"gpu_temp\":%.1f,"
        "\"gpu_memory_used\":%llu,\"gpu_memory_total\":%llu,\"ram_used\":%llu,\"ram_total\":%llu,\"status\":\"%s\"}",settings.enabled,result.active,int(settings.backend),result.realFps,result.generatedFps,
        result.outputFps,result.repeatFps,result.cpuMs,result.gpuMs,result.generationMs,result.generationSpanMs,result.budgetMs,result.realFrames,result.generatedFrames,result.realSubmissions,result.repeatedFrames,result.skipped,
        result.vramBytes,result.inputFrames[0],result.inputFrames[1],result.averageFps,result.minimumFps,result.maximumFps,result.low1Fps,result.low01Fps,
        result.hardware.cpuUsage,result.hardware.gpuUsage,result.hardware.gpuTemperature,result.hardware.gpuMemoryUsed,result.hardware.gpuMemoryTotal,
        result.hardware.ramUsed,result.hardware.ramTotal,result.status);
    CyberpunkVR_FramegenReportSeq.fetch_add(1,std::memory_order_release);
    }
    cached=result;return result;
}
void ResetStatistics() {
    std::lock_guard lock(mutex);ClearMetricsLocked();
}
void Status(const char* text,bool active) {
    std::lock_guard lock(mutex);stats.active=active;std::snprintf(stats.status,sizeof(stats.status),"%s",text);
}
void OnRealFrame(uint64_t serial,double now) {
    if(!MetricsEnabled())return;std::lock_guard lock(mutex);if(!settings.overlay || !serial || serial==lastReal)return;
    if(!started)started=now;
    if(lastRealTime>0 && now>lastRealTime) {
        stats.cpuMs=now-lastRealTime;cpu.Add(now,stats.cpuMs);cpuHistory.Add(now,stats.cpuMs);
        const auto n=CyberpunkVR_FrameRingCount[0].load(std::memory_order_relaxed);
        CyberpunkVR_FrameRingCpu[n%8192]=float(stats.cpuMs);CyberpunkVR_FrameRingCount[0].store(n+1,std::memory_order_release);
    }
    lastReal=serial;lastRealTime=now;real.Add(now);++stats.realFrames;
}
void OnSubmitted(uint64_t,bool synth,bool repeat,double now) {
    if(!MetricsEnabled())return;std::lock_guard lock(mutex);if(!settings.overlay)return;
    if(repeat) {repeats.Add(now);++stats.repeatedFrames;return;}
    output.Add(now);if(synth) {generated.Add(now);++stats.generatedFrames;}else ++stats.realSubmissions;
}
void OnGpuFrame(double ms) {
    if(!MetricsEnabled() || !std::isfinite(ms) || ms<0 || ms>10000)return;
    std::lock_guard lock(mutex);if(!settings.overlay)return;stats.gpuValid=true;stats.gpuMs=ms;const auto now=NowMs();gpu.Add(now,ms);gpuHistory.Add(now,ms);
    const auto n=CyberpunkVR_FrameRingCount[1].load(std::memory_order_relaxed);
    CyberpunkVR_FrameRingGpu[n%8192]=float(ms);CyberpunkVR_FrameRingCount[1].store(n+1,std::memory_order_release);
}
void OnGeneration(double ms,double spanMs) {
    if(!MetricsEnabled())return;std::lock_guard lock(mutex);if(settings.overlay) {stats.generationMs=ms;stats.generationSpanMs=spanMs;}
}
void OnInput(unsigned eye,uint32_t w,uint32_t h) {
    if(eye>1 || !MetricsEnabled())return;std::lock_guard lock(mutex);if(!settings.overlay)return;++stats.inputFrames[eye];
    stats.renderWidth[eye]=w;stats.renderHeight[eye]=h;
    stats.inputsReady=stats.inputFrames[0] && stats.inputFrames[1];
}
void OnSkip() {if(!MetricsEnabled())return;std::lock_guard lock(mutex);if(settings.overlay)++stats.skipped;}
void SetVram(uint64_t bytes) {if(!MetricsEnabled())return;std::lock_guard lock(mutex);if(settings.overlay)stats.vramBytes=bytes;}
void DisplayPeriod(int64_t ns,bool steamVr) {
    if(ns<4000000 || ns>40000000)return;
    // One XR-loop owner writes the clock and period filter, including during a submit-owner handover.
    static CycleClock clock;
    static PresentationPeriod period;
    const double now=NowMs();
    if(steamVr)ns=period.Observe(now,ns);else period.Reset();
    clock.Tick(now,double(ns)*1e-6);
    displayPeriod.store(ns,std::memory_order_relaxed);displayCycleMs.store(now,std::memory_order_relaxed);
    cycleSteady.store(clock.Steady(),std::memory_order_relaxed);
}
bool FpsCap() {return fpsCap.load(std::memory_order_relaxed);}
void SetFpsCap(bool on) {fpsCap.store(on,std::memory_order_relaxed);}
namespace {
// THE GAME RUNS AT THE HEADSET'S RATE AT MOST (xr_fps_cap). A frame past the refresh is never shown, it only loads
// the GPU, and a game a little faster than the headset beats against it: every few seconds two frames land in one
// display cycle and the cycle after repeats one. So the producer is held after its Present until the instant that
// puts its NEXT Present in the middle of a display cycle, the farthest point from where the XR thread takes the
// newest frame (xrWaitFrame's return, DisplayPeriod). A game that cannot reach the refresh is never held.
// Caller holds the pacing lock.
void CapToDisplay(int64_t period) {
    struct State {HANDLE timer{};double released{},work{};~State(){if(timer)CloseHandle(timer);}};
    static State s;
    const double now=NowMs(),cycle=displayCycleMs.load(std::memory_order_relaxed),per=double(period)*1e-6;
    // No XR cycle for a while (session idle or lost): nothing to pace against.
    if(!fpsCap.load(std::memory_order_relaxed) || !period || now-cycle>100) {s.released=s.work=0;return;}
    // The game's own time from release to the end of its Present, smoothed over about eight frames.
    if(s.released>0) {const double w=now-s.released;if(w>0 && w<100)s.work=s.work>0 ? s.work+(w-s.work)*.125 : w;}
    double wait=0;
    if(s.work>0 && s.work<per*.9) {
        // The first mid-cycle instant at least half a period away: one period on from a Present already there.
        const double next=cycle+per*(.5+std::ceil((now-cycle)/per));
        wait=std::min(next-s.work-now,per*2);
    }
    if(wait>.2) {
        if(!s.timer)s.timer=CreateWaitableTimerExW(nullptr,nullptr,CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,TIMER_ALL_ACCESS);
        if(s.timer) {
            LARGE_INTEGER due{};due.QuadPart=-static_cast<LONGLONG>(wait*10000.0);
            if(SetWaitableTimerEx(s.timer,&due,0,nullptr,nullptr,nullptr,0))WaitForSingleObject(s.timer,100);
        }
        ++CyberpunkVR_DebugFpsCap[0];
    } else ++CyberpunkVR_DebugFpsCap[1];
    s.released=NowMs();
}
}
void PacingReady(bool ready) {
    pacingReady.store(ready,std::memory_order_relaxed);
    if(!ready){pacingIntervalMs.store(0,std::memory_order_relaxed);resetPacing.store(true,std::memory_order_relaxed);}
}
void OnPresentationCycle(bool missedAdjacentFrame) {
    if(missedAdjacentFrame)CyberpunkVR_FramegenPairGaps.fetch_add(1,std::memory_order_relaxed);
}
void PaceRenderedFrame() {
    struct Timer {HANDLE handle{};PhaseLockedPacing phase;CadenceLock cadence;~Timer(){if(handle)CloseHandle(handle);}};
    // The game's Present jobs can run on different workers. A TLS deadline
    // limits each worker independently, allowing the combined stream to run
    // faster than half refresh. Serialize one schedule for that stream.
    static Timer timer;
    static std::mutex paceMutex;
    const double entered=RuntimeDiagnosticsEnabled()?NowMs():0;
    std::lock_guard paceLock(paceMutex);
    const auto period=displayPeriod.load(std::memory_order_relaxed);
    // Framegen's pacing (two display periods a real frame) when it runs; the plain refresh cap otherwise.
    if(!Enabled() || !autoPace.load(std::memory_order_relaxed) || !pacingReady.load(std::memory_order_relaxed) || !period) {
        timer.phase.Reset();timer.cadence.Reset();pacingIntervalMs.store(0,std::memory_order_relaxed);
        CyberpunkVR_FramegenThreePeriods.store(0,std::memory_order_relaxed);CapToDisplay(period);return;
    }
    if(resetPacing.exchange(false,std::memory_order_relaxed))timer.cadence.Reset();
    const double now=NowMs(),per=double(period)*1e-6;
    // ALWAYS TWO DISPLAY PERIODS unless the cadence lock is asked for (xr_framegen_cadence_lock). The lock moved to three
    // on one heavy second (38% of frames over the budget at an H10 sprint) and stayed there seven seconds while the
    // game's own time was 15.6 ms at the median and 19.9 ms at p90 of a 22.2 ms budget: 30 fps for a game that
    // makes 45. A frame over the budget now costs that frame, and the XR side keeps one finished frame in hand.
    const bool lockCadence=cadenceLock.load(std::memory_order_relaxed);
    if(!lockCadence)timer.cadence.Reset();
    const double interval=lockCadence ? timer.cadence.Observe(now,per,timer.phase.Elapsed(now)) : per*2;
    pacingIntervalMs.store(interval,std::memory_order_relaxed);
    CyberpunkVR_FramegenThreePeriods.store(timer.cadence.Three()?1u:0u,std::memory_order_relaxed);
    // No grid from a loop that does not keep its period (CycleClock): the plain interval then.
    const double cycle=cycleSteady.load(std::memory_order_relaxed) ? displayCycleMs.load(std::memory_order_relaxed) : NAN;
    const double previousTarget=timer.phase.Target();
    const double hold=timer.phase.Hold(now,cycle,per,interval);
    const double waitStart=entered?NowMs():0;
    if(hold>0) {
        if(!timer.handle)timer.handle=CreateWaitableTimerExW(nullptr,nullptr,CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,TIMER_ALL_ACCESS);
        if(timer.handle) {
            LARGE_INTEGER due{};due.QuadPart=-static_cast<LONGLONG>(hold*10000.0);
            if(SetWaitableTimerEx(timer.handle,&due,0,nullptr,nullptr,nullptr,0))WaitForSingleObject(timer.handle,100);
        }
    }
    const double released=NowMs();
    timer.phase.Released(released);
    if(entered) {
        const auto n=CyberpunkVR_PaceTraceCount.load(std::memory_order_relaxed);
        const double row[12]={now,now-entered,timer.phase.Work(),previousTarget,timer.phase.Target(),cycle,
            per,interval,hold,released-waitStart,released,0};
        std::memcpy(CyberpunkVR_PaceTrace[n%8192],row,sizeof(row));
        CyberpunkVR_PaceTraceCount.store(n+1,std::memory_order_release);
    }
}
bool ParseSetting(const char* line,Settings& value) {
    int n{};float f{};
    if(sscanf_s(line,"xr_framegen = %d",&n)==1)value.enabled=n!=0;
    else if(sscanf_s(line,"xr_framegen_overlay = %d",&n)==1)value.overlay=n!=0;
    else if(sscanf_s(line,"xr_framegen_pace = %d",&n)==1)value.autoPace=n!=0;
    else if(sscanf_s(line,"xr_framegen_cadence_lock = %d",&n)==1)value.cadenceLock=n!=0;
    else if(sscanf_s(line,"xr_framegen_backend = %d",&n)==1)value.backend=Backend(std::clamp(n,0,3));
    else if(sscanf_s(line,"xr_framegen_quality = %d",&n)==1)value.quality=n;
    else if(sscanf_s(line,"xr_framegen_flow_scale = %d",&n)==1)value.flowScale=n;
    else if(sscanf_s(line,"xr_framegen_overlay_corner = %d",&n)==1)value.overlayCorner=n;
    else if(sscanf_s(line,"xr_framegen_overlay_layout = %d",&n)==1)value.overlayLayout=n;
    else if(sscanf_s(line,"xr_framegen_overlay_follow = %d",&n)==1)value.overlayFollow=n;
    else if(sscanf_s(line,"xr_framegen_history = %d",&n)==1)value.historySeconds=n;
    else if(sscanf_s(line,"xr_framegen_overlay_cone = %f",&f)==1)value.overlayCone=f;
    else if(sscanf_s(line,"xr_framegen_overlay_scale = %f",&f)==1)value.overlayScale=f;
    else if(sscanf_s(line,"xr_framegen_overlay_distance = %f",&f)==1)value.overlayDistance=f;
    else if(sscanf_s(line,"xr_framegen_overlay_x = %f",&f)==1)value.overlayX=f;
    else if(sscanf_s(line,"xr_framegen_overlay_y = %f",&f)==1)value.overlayY=f;
    else return false;
    return true;
}
void WriteSettings(void* file,const Settings& s) {
    std::fprintf(static_cast<FILE*>(file),"xr_framegen=%d\nxr_framegen_overlay=%d\nxr_framegen_pace=%d\nxr_framegen_backend=%d\nxr_framegen_quality=%d\nxr_framegen_flow_scale=%d\nxr_framegen_overlay_corner=%d\n",
        s.enabled,s.overlay,s.autoPace,int(s.backend),s.quality,s.flowScale,s.overlayCorner);
    std::fprintf(static_cast<FILE*>(file),"xr_framegen_cadence_lock=%d\n",s.cadenceLock ? 1 : 0);
    std::fprintf(static_cast<FILE*>(file),"xr_framegen_overlay_layout=%d\nxr_framegen_overlay_follow=%d\nxr_framegen_history=%d\nxr_framegen_overlay_cone=%.2f\nxr_framegen_overlay_scale=%.3f\nxr_framegen_overlay_distance=%.3f\nxr_framegen_overlay_x=%.2f\nxr_framegen_overlay_y=%.2f\n",
        s.overlayLayout,s.overlayFollow,s.historySeconds,s.overlayCone,s.overlayScale,s.overlayDistance,s.overlayX,s.overlayY);
}
}
