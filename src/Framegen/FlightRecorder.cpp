#include "Framegen/SourceTimeline.hpp"
#include "Framegen/DisplayDeadline.hpp"
#include "Framegen/RefreshCadence.hpp"
#include "Framegen/FlightRecorder.hpp"
#include "Framegen/Framegen.hpp"
#include "Core/Paths.hpp"
#include "xrfg/steamvr_delivery.hpp"
#include "xrfg/bridge_flight_logger.hpp"
#include <nlohmann/json.hpp>
#include <windows.h>
#include <array>
#include <chrono>
#include <condition_variable>
#include <cstdio>
#include <memory>
#include <mutex>
#include <string>
#include <thread>

namespace cvr::framegen::flight {
namespace {
using Json=nlohmann::json;
using Clock=std::chrono::steady_clock;
std::array<std::atomic<uint64_t>,size_t(Event::Count)> events{};
std::array<Accumulator,size_t(Timing::Count)> times{};
std::atomic<uint32_t> inputW[2]{},inputH[2]{},outputW{},outputH{};
struct DisplayRecord {DisplayDeadline deadline;double gpuEndMs{},queueMs{};};
std::mutex displayMutex;
std::array<DisplayRecord,64> displayRecords;
size_t displayCount{};
std::mutex sourceMutex;std::array<SourceRecord,32> sourceRecords;size_t sourceCount{};
std::atomic<double> periodMs{},paceMs{};
std::atomic<bool> pacing{};
std::atomic<int64_t> lastError{},attachResult{};
std::atomic<uint64_t> attachRoute{},attachPid{},attachSystem{},acceptedTotal{};

class Recorder {
    std::mutex control,waitMutex,statusMutex;
    std::condition_variable wake;
    std::thread worker;
    std::atomic<bool> stopping{false};
    std::atomic<int> state{0};
    bool requested{},ready{},steam{};
    std::string runtime,headset,status="Off";
    void SetStatus(int s,const char* text){std::lock_guard lock(statusMutex);status=text;state=s;}
    void Halt(){enabled=false;stopping=true;wake.notify_all();if(worker.joinable())worker.join();}
    void Launch() {
        if(!requested){SetStatus(0,"Off");return;}
        if(!ready){SetStatus(1,"Waiting for OpenXR runtime");return;}
        stopping=false;
        for(auto& e:events)e=0;
        for(auto& t:times)t.Drain();
        {std::lock_guard lock(displayMutex);displayCount=0;}
        {std::lock_guard lock(sourceMutex);sourceCount=0;}
        acceptedTotal=0;lastError=0;attachResult=0;attachRoute=attachPid=attachSystem=0;
        SetStatus(1,"Opening flight log");
        try{worker=std::thread([this]{Run();});}
        catch(...){SetStatus(-1,"Cannot start recorder thread");}
    }
    void Run() noexcept {
      try {
        SYSTEMTIME utc{};GetSystemTime(&utc);wchar_t name[160]{};
        swprintf_s(name,L"framegen-flight-%04u%02u%02u-%02u%02u%02u-%03u-p%lu.jsonl",
            utc.wYear,utc.wMonth,utc.wDay,utc.wHour,utc.wMinute,utc.wSecond,utc.wMilliseconds,GetCurrentProcessId());
        FILE* raw{};const auto path=cvr::paths::File(cvr::paths::Folder::Log,name);
        if(_wfopen_s(&raw,path.c_str(),L"wb")||!raw){SetStatus(-1,"Cannot open flight log; check log folder permissions");return;}
        std::unique_ptr<FILE,decltype(&fclose)> file(raw,&fclose);
        size_t bytes=0;
        auto write=[&](const Json& value) {
            const auto line=value.dump()+"\n";
            if(bytes+line.size()>16*1024*1024){SetStatus(-2,"16 MiB file limit reached; toggle to start another log");return false;}
            if(fwrite(line.data(),1,line.size(),file.get())!=line.size()||fflush(file.get())){SetStatus(-1,"Flight log write failed");return false;}
            bytes+=line.size();return true;
        };
        if(!write({{"type","header"},{"schema",1},{"pid",GetCurrentProcessId()},
            {"runtime",runtime},{"steamvr",steam},{"clock","QPC milliseconds"},
            {"build","upstream022-steamvr-package-unified-v3"},{"headset",headset},
            {"semantics","accepted=successful xrEndFrame, not displayed; generated_queued=GPU work queued, not completed; compositor_estimate is not unique-content FPS; window boundaries approximate"}}))return;
        xrfg::SteamVrDelivery reader(steam);
        enabled=true;SetStatus(2,"Writing framegen-flight log (1-second summaries)");
        auto start=Clock::now(),last=start;
        auto settingsJson=[] {
            auto s=GetSettings();return Json{{"enabled",s.enabled},{"backend",int(s.backend)},
                {"quality",s.quality},{"flow_scale",s.flowScale},{"adaptive_pacing",s.autoPace},{"steamvr_package_requested",s.steamVrOptimizations},{"fps_cap",FpsCap()}};
        };
        Json priorSettings=settingsJson();
        double deliverySum=0;uint64_t deliveryN=0,records=0,shown=0,zero=0,dropped=0,mispresented=0,extra=0,holes=0;
        uint32_t flags=0;double appGpu=0,compositorGpu=0,clientMax=0;
        std::unique_lock lock(waitMutex);
        for(;;) {
            wake.wait_for(lock,std::chrono::milliseconds(100),[&]{return stopping.load();});
            const bool final=stopping.load();lock.unlock();
            const auto now=Clock::now();
            if(steam && now-start>=std::chrono::seconds(3)&&acceptedTotal.load()>=90) {
                reader.mark_established();
                auto estimate=reader.delivered_fps(std::chrono::duration_cast<std::chrono::nanoseconds>(now.time_since_epoch()).count());
                if(estimate){deliverySum+=*estimate;++deliveryN;}
                for(const auto& f:reader.settled_frames()) {
                    ++records;if(f.presents)++shown;else ++zero;
                    dropped+=f.dropped;mispresented+=f.mispresented;holes+=f.skipped;
                    if(f.presents>1)extra+=f.presents-1;
                    flags|=f.reprojection_flags_window;appGpu+=f.total_render_gpu_us/1000.;
                    compositorGpu+=f.compositor_render_gpu_us/1000.;
                    clientMax=std::max(clientMax,double(f.client_frame_interval_ms));
                }
            }
            const double seconds=std::chrono::duration<double>(now-last).count();
            if(seconds>=1||final) {
                auto currentSettings=settingsJson();
                const auto physical=refresh::Read();
                const auto rawNs=int64_t(periodMs.load()*1e6);
                const auto effectiveNs=refresh::Select(refresh::Targeted(),rawNs,physical,GetTickCount64());
                Json row={{"type",final?"stop":"sample"},{"clock_ms",NowMs()},{"window_s",seconds},
                    {"settings",currentSettings},{"settings_changed",currentSettings!=priorSettings},
                    {"effective_backend",int(ActiveBackend())},{"period_ms",periodMs.load()},
                    {"pace_ms",paceMs.load()},{"pacing_active",pacing.load()},
                    {"scanout_correction_steamvr",refresh::Targeted()},
                    {"steamvr_package_active",refresh::Targeted()},
                    {"physical_period_ms",physical.periodNs?Json(physical.periodNs*1e-6):Json(nullptr)},
                    {"effective_fg_period_ms",effectiveNs*1e-6},
                    {"period_source",refresh::Targeted()&&physical.periodNs?"steamvr_scanout":"runtime_fallback"},
                    {"input",{{inputW[0].load(),inputH[0].load()},{inputW[1].load(),inputH[1].load()}}},
                    {"output_eye",{outputW.load(),outputH.load()}},{"last_xr_error",lastError.load()}};
                for(size_t i=0;i<events.size();++i)row["events"][Names[i]]=events[i].exchange(0);
                for(size_t i=0;i<times.size();++i){auto v=times[i].Drain();row["timings"][TimingNames[i]]={{"n",v.count},{"mean",v.count?Json(v.sumUs/1000./v.count):Json(nullptr)},{"max",v.count?Json(v.maxUs/1000.):Json(nullptr)}};}
                auto rs=GetRuntimeStatus();row["backend_status"]=rs.text;
                row["compositor"]={{"available",deliveryN>0},{"estimate_fps",deliveryN?Json(deliverySum/deliveryN):Json(nullptr)},
                    {"records",records},{"presented_records",shown},{"zero_present_records",zero},
                    {"dropped_reports",dropped},{"mispresented_reports",mispresented},{"extra_presents",extra},
                    {"history_holes",holes},{"reprojection_flags",flags},{"max_client_interval_ms",clientMax},
                    {"app_gpu_ms",records?Json(appGpu/records):Json(nullptr)},
                    {"compositor_gpu_ms",records?Json(compositorGpu/records):Json(nullptr)},
                    {"attach_result",attachResult.load()},{"attach_route",attachRoute.load()},{"attach_pid",attachPid.load()},
                    {"reason",steam?"V557 estimate; missing data stays null; real/generated attribution unavailable":"Runtime has no SteamVR compositor telemetry"}};
                std::array<DisplayRecord,64> probeBatch;size_t probeCount{};
                {std::lock_guard lock(displayMutex);probeCount=displayCount;
                    std::copy_n(displayRecords.begin(),probeCount,probeBatch.begin());displayCount=0;}
                row["display_probes"]=Json::array();
                for(size_t i=0;i<probeCount;++i){const auto& s=probeBatch[i];const auto& d=s.deadline;
                    row["display_probes"].push_back({{"kind",d.kind},{"probe_qpc_ms",d.probeMs},
                        {"probe_width_ms",d.probeWidthMs},{"remaining_ms",d.remainingMs},
                        {"margin_ms",d.marginMs},{"period_ms",d.periodMs},{"handoff_qpc_ms",d.handoffMs},
                        {"returned_qpc_ms",d.returnedMs},{"gpu_end_qpc_ms",s.gpuEndMs},{"queue_ms",s.queueMs}});
                }
                std::array<SourceRecord,32> sources;size_t nSources{};
                {std::lock_guard lock(sourceMutex);nSources=sourceCount;std::copy_n(sourceRecords.begin(),nSources,sources.begin());sourceCount=0;}
                row["source_probes"]=Json::array();
                for(size_t i=0;i<nSources;++i){const auto& s=sources[i];
                    Json eyes=Json::array();for(unsigned eye=0;eye<2;++eye){
                        Json stages=Json::array();for(unsigned k=0;k<3;++k)stages.push_back({{"end_qpc_ms",s.end[eye][k]},
                            {"record_qpc_ms",s.recorded[eye][k]},{"submitted_observed_qpc_ms",s.submitted[eye][k]}});
                        eyes.push_back({{"pose",s.pose[eye]},{"stages",stages}});
                    }
                    row["source_probes"].push_back({{"frame",s.frame},{"origin",s.origin},{"mask",s.mask},{"rr",s.rr},
                        {"eyes",eyes},{"stage_order",{"depth_copy","motion_copy","upscale_return_copyback"}},
                        {"capture_enqueue_qpc_ms",s.captureEnqueue},{"capture_start_qpc_ms",s.captureStart},
                        {"capture_end_qpc_ms",s.captureEnd},{"generation_enqueue_qpc_ms",s.generationEnqueue},
                        {"generation_start_qpc_ms",s.generationStart}});
                }
                if(!write(row))break;
                priorSettings=currentSettings;last=now;deliverySum=0;deliveryN=records=shown=zero=dropped=mispresented=extra=holes=0;
                flags=0;appGpu=compositorGpu=clientMax=0;
            }
            if(final)break;
            lock.lock();
        }
        enabled=false;
      } catch(...){enabled=false;SetStatus(-1,"Recorder failed; rendering continues without recording");}
    }
public:
    ~Recorder(){Halt();}
    void Configure(bool value){std::lock_guard lock(control);if(value==requested)return;Halt();requested=value;Launch();}
    void Runtime(const char* name,bool value,const char* system){std::lock_guard lock(control);Halt();runtime=name;headset=system;steam=value;ready=true;Launch();}
    void Stop(){std::lock_guard lock(control);Halt();ready=false;SetStatus(requested?1:0,requested?"Waiting for OpenXR runtime":"Off");}
    int State(){return state.load();}
    void Status(char* out,size_t n){std::lock_guard lock(statusMutex);if(n)snprintf(out,n,"%s",status.c_str());}
};
Recorder& Get(){static Recorder recorder;return recorder;}
}
// Rendering never waits for the recorder or performs JSON/file IO. Drop rather
// than block if the bounded handoff is momentarily busy/full.
void Source(const SourceRecord& s){
    if(!Enabled())return;
    std::unique_lock lock(sourceMutex,std::try_to_lock);
    if(!lock.owns_lock() || sourceCount==sourceRecords.size()){Count(Event::SourceRecordDropped);return;}
    sourceRecords[sourceCount++]=s;
}
void Display(const DisplayDeadline& d,double gpuEndMs,double queueMs){
    if(!Enabled())return;
    std::unique_lock lock(displayMutex,std::try_to_lock);
    if(!lock.owns_lock() || displayCount==displayRecords.size()){Count(Event::DisplayRecordDropped);return;}
    displayRecords[displayCount++]={d,gpuEndMs,queueMs};
}
void Count(Event e){if(!Enabled())return;events[size_t(e)].fetch_add(1,std::memory_order_relaxed);
    if(e==Event::AcceptedReal||e==Event::AcceptedSynthetic||e==Event::AcceptedRepeat||e==Event::FallbackRepeat)acceptedTotal.fetch_add(1,std::memory_order_relaxed);}
void Time(Timing t,double ms){if(Enabled())times[size_t(t)].Add(ms);}
void Input(unsigned eye,uint32_t w,uint32_t h){if(!Enabled()||eye>1)return;inputW[eye]=w;inputH[eye]=h;Count(eye?Event::InputRight:Event::InputLeft);}
void Output(uint32_t w,uint32_t h){if(Enabled()){outputW=w;outputH=h;}}
void Period(double ms){periodMs=ms;Count(Event::Cycle);}
void Pace(double interval,bool active){paceMs=interval;pacing=active;if(active)Time(Timing::PacingBudget,interval);}
void Error(int64_t result){if(Enabled()&&result<0){lastError=result;Count(Event::XrError);}}
void Configure(bool requested){Get().Configure(requested);}
void Runtime(const char* name,bool steamvr,const char* headset){Get().Runtime(name,steamvr,headset);}
void Stop(){Get().Stop();}
void ReleaseConnection(){xrfg::SteamVrDelivery::release_process_connection();}
int State(){return Get().State();}
void Status(char* text,size_t capacity){Get().Status(text,capacity);}
void Attach(int64_t result,uint64_t route,uint64_t pid,uint64_t system){attachResult=result;attachRoute=route;attachPid=pid;attachSystem=system;}
}
namespace xrfg {
void RecordDeliveryEvent(BridgeFlightOperation op,int64_t result,uint64_t a,uint64_t b,uint64_t c){
    if(op==BridgeFlightOperation::steamvr_delivery_attach)cvr::framegen::flight::Attach(result,a,b,c);
}
}
