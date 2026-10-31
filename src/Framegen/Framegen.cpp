#include "Framegen/Framegen.hpp"
#include "Framegen/OfxrBridge.hpp"
#include "Framegen/FramePolicy.hpp"
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
}

namespace cvr::framegen {
namespace {
std::mutex mutex;
std::mutex settingsWriteMutex;
Settings settings;
std::atomic<bool> enabled{false},metrics{false};
std::atomic<bool> depthCapture{false};
std::atomic<bool> autoPace{true},pacingReady{false};
std::atomic<int64_t> displayPeriod{0};
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
        case Backend::Nvidia:return "OFXR V557 (NVIDIA)";
        case Backend::DlssG:return "NVIDIA DLSS-G";
        default:return "FidelityFX";
    }
}
Backend ActiveBackend() {const int v=activeBackend.load(std::memory_order_relaxed);return v<0 ? GetSettings().backend : Backend(v);}
void PublishActiveBackend(Backend value) {activeBackend.store(int(value),std::memory_order_relaxed);}
bool Enabled() {return enabled.load(std::memory_order_relaxed) && cvr::ofxr::NativeAllowed();}
bool DepthCaptureEnabled() {return depthCapture.load(std::memory_order_relaxed);}
bool CaptureInputsEnabled() {return Enabled() || DepthCaptureEnabled();}
void SetDepthCaptureEnabled(bool value) {depthCapture.store(value,std::memory_order_relaxed);}
bool MetricsEnabled() {return metrics.load(std::memory_order_relaxed);}
void SetSettings(Settings value) {
    std::lock_guard settingsLock(settingsWriteMutex);
    value.backend=Backend(std::clamp(int(value.backend),0,2));
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
    enabled.store(value.enabled && value.backend!=Backend::Nvidia,std::memory_order_relaxed);
    metrics.store(value.overlay,std::memory_order_relaxed);
    autoPace.store(value.autoPace,std::memory_order_relaxed);
    if(!value.enabled || value.backend==Backend::Nvidia || !value.autoPace){pacingIntervalMs=0;resetPacing=true;}
    if(!value.enabled) {pacingReady.store(false,std::memory_order_relaxed);stats.generationMs=0;}
    if(cvr::RuntimeDiagnosticsEnabled() && !value.overlay && disabledReportChanged) {
        CyberpunkVR_FramegenReportSeq.fetch_add(1,std::memory_order_acq_rel);
        std::snprintf(CyberpunkVR_FramegenReport,sizeof(CyberpunkVR_FramegenReport),
            "{\"enabled\":%d,\"backend\":%d,\"metrics\":0,\"status\":\"FPS telemetry disabled\"}",value.enabled,int(value.backend));
        CyberpunkVR_FramegenReportSeq.fetch_add(1,std::memory_order_release);
    }
    }
    cvr::ofxr::Configure(value);
    if(value.overlay)WakeHardwareTelemetry();else if(metricsChanged)StopHardwareTelemetry();
}
RuntimeStatus GetRuntimeStatus() {
    if(GetSettings().backend==Backend::Nvidia)return cvr::ofxr::Status();
    if(!cvr::ofxr::NativeAllowed()) {RuntimeStatus r{};std::snprintf(r.text,sizeof(r.text),"Restart the OpenXR runtime to switch from V557 to this generator.");return r;}
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
        "\"cpu_ms\":%.3f,\"gpu_ms\":%.3f,\"generation_ms\":%.3f,\"pace_ms\":%.3f,\"real\":%llu,\"generated\":%llu,\"real_submitted\":%llu,\"repeated\":%llu,\"skipped\":%llu,\"vram_bytes\":%llu,"
        "\"inputs\":[%llu,%llu],\"average_fps\":%.3f,\"minimum_fps\":%.3f,\"maximum_fps\":%.3f,\"low1\":%.3f,\"low01\":%.3f,\"cpu_usage\":%.2f,\"gpu_usage\":%.2f,\"gpu_temp\":%.1f,"
        "\"gpu_memory_used\":%llu,\"gpu_memory_total\":%llu,\"ram_used\":%llu,\"ram_total\":%llu,\"status\":\"%s\"}",settings.enabled,result.active,int(settings.backend),result.realFps,result.generatedFps,
        result.outputFps,result.repeatFps,result.cpuMs,result.gpuMs,result.generationMs,result.budgetMs,result.realFrames,result.generatedFrames,result.realSubmissions,result.repeatedFrames,result.skipped,
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
    if(lastRealTime>0 && now>lastRealTime) {stats.cpuMs=now-lastRealTime;cpu.Add(now,stats.cpuMs);cpuHistory.Add(now,stats.cpuMs);}
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
}
void OnGeneration(double ms) {if(!MetricsEnabled())return;std::lock_guard lock(mutex);if(settings.overlay)stats.generationMs=ms;}
void OnInput(unsigned eye,uint32_t w,uint32_t h) {
    if(eye>1 || !MetricsEnabled())return;std::lock_guard lock(mutex);if(!settings.overlay)return;++stats.inputFrames[eye];
    stats.renderWidth[eye]=w;stats.renderHeight[eye]=h;
    stats.inputsReady=stats.inputFrames[0] && stats.inputFrames[1];
}
void OnSkip() {if(!MetricsEnabled())return;std::lock_guard lock(mutex);if(settings.overlay)++stats.skipped;}
void SetVram(uint64_t bytes) {if(!MetricsEnabled())return;std::lock_guard lock(mutex);if(settings.overlay)stats.vramBytes=bytes;}
void DisplayPeriod(int64_t ns) {if(ns>=4000000 && ns<=40000000)displayPeriod.store(ns,std::memory_order_relaxed);}
void PacingReady(bool ready) {
    pacingReady.store(ready,std::memory_order_relaxed);
    if(!ready){pacingIntervalMs.store(0,std::memory_order_relaxed);resetPacing.store(true,std::memory_order_relaxed);}
}
void OnPresentationCycle(bool missedAdjacentFrame) {
    static AdaptivePacing feedback;
    if(!Enabled() || !autoPace.load(std::memory_order_relaxed)) {feedback.Reset();pacingIntervalMs=0;return;}
    if(resetPacing.exchange(false,std::memory_order_relaxed))feedback.Reset();
    pacingIntervalMs.store(feedback.Observe(NowMs(),double(displayPeriod.load(std::memory_order_relaxed))*2e-6,missedAdjacentFrame),std::memory_order_relaxed);
}
void PaceRenderedFrame() {
    struct Timer {HANDLE handle{};double last{};~Timer(){if(handle)CloseHandle(handle);}};
    // The game's Present jobs can run on different workers. A TLS deadline
    // limits each worker independently, allowing the combined stream to run
    // faster than half refresh. Serialize one schedule for that stream.
    static Timer timer;
    static std::mutex paceMutex;
    std::lock_guard paceLock(paceMutex);
    const auto period=displayPeriod.load(std::memory_order_relaxed);
    if(!Enabled() || !autoPace.load(std::memory_order_relaxed) || !pacingReady.load(std::memory_order_relaxed) || !period) {timer.last=0;return;}
    const double now=NowMs(),interval=std::max(double(period)*2e-6,pacingIntervalMs.load(std::memory_order_relaxed));
    const double remaining=timer.last+interval-now;
    if(remaining>.2 && remaining<=80) {
        if(!timer.handle)timer.handle=CreateWaitableTimerExW(nullptr,nullptr,CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,TIMER_ALL_ACCESS);
        if(timer.handle) {
            LARGE_INTEGER due{};due.QuadPart=-static_cast<LONGLONG>(remaining*10000.0);
            if(SetWaitableTimerEx(timer.handle,&due,0,nullptr,nullptr,nullptr,0))WaitForSingleObject(timer.handle,100);
        }
    }
    const auto done=NowMs();timer.last=timer.last && done<timer.last+interval*1.5 ? timer.last+interval : done;
}
bool ParseSetting(const char* line,Settings& value) {
    int n{};float f{};
    if(sscanf_s(line,"xr_framegen = %d",&n)==1)value.enabled=n!=0;
    else if(sscanf_s(line,"xr_framegen_logging = %d",&n)==1)value.ofxrLogging=n!=0;
    else if(sscanf_s(line,"xr_framegen_overlay = %d",&n)==1)value.overlay=n!=0;
    else if(sscanf_s(line,"xr_framegen_pace = %d",&n)==1)value.autoPace=n!=0;
    else if(sscanf_s(line,"xr_framegen_backend = %d",&n)==1)value.backend=Backend(std::clamp(n,0,2));
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
    std::fprintf(static_cast<FILE*>(file),"xr_framegen_logging=%d\n",s.ofxrLogging);
    std::fprintf(static_cast<FILE*>(file),"xr_framegen=%d\nxr_framegen_overlay=%d\nxr_framegen_pace=%d\nxr_framegen_backend=%d\nxr_framegen_quality=%d\nxr_framegen_flow_scale=%d\nxr_framegen_overlay_corner=%d\n",
        s.enabled,s.overlay,s.autoPace,int(s.backend),s.quality,s.flowScale,s.overlayCorner);
    std::fprintf(static_cast<FILE*>(file),"xr_framegen_overlay_layout=%d\nxr_framegen_overlay_follow=%d\nxr_framegen_history=%d\nxr_framegen_overlay_cone=%.2f\nxr_framegen_overlay_scale=%.3f\nxr_framegen_overlay_distance=%.3f\nxr_framegen_overlay_x=%.2f\nxr_framegen_overlay_y=%.2f\n",
        s.overlayLayout,s.overlayFollow,s.historySeconds,s.overlayCone,s.overlayScale,s.overlayDistance,s.overlayX,s.overlayY);
}
}
