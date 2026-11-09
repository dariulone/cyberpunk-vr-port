#include "Framegen/SourceTimeline.hpp"
#include "Framegen/DisplayDeadline.hpp"
#include "Framegen/FlightRecorder.hpp"
#include "Framegen/Framegen.hpp"
#include "Core/Paths.hpp"
#include <nlohmann/json.hpp>
#include <windows.h>
#include <chrono>
#include <thread>
#include <fstream>
#include <filesystem>
#include <iostream>
#include <vector>
#include <stdexcept>
using namespace cvr::framegen;
namespace fs=std::filesystem;
static fs::path root;
static bool badPath=false;
namespace cvr::paths {std::wstring File(Folder,const wchar_t* name){return ((badPath?root/L"missing":root)/name).wstring();}}
namespace cvr::framegen {
Settings GetSettings(){Settings s;s.enabled=true;s.overlay=false;return s;}
Backend ActiveBackend(){return Backend::Nvidia;}
bool FpsCap(){return true;}
RuntimeStatus GetRuntimeStatus(){RuntimeStatus r;strcpy_s(r.text,"fixture");return r;}
double NowMs(){return double(GetTickCount64());}
}
static void check(bool value,const char* label){if(!value)throw std::runtime_error(label);}
static void waitState(int expected){for(int i=0;i<200&&flight::State()!=expected;++i)std::this_thread::sleep_for(std::chrono::milliseconds(10));check(flight::State()==expected,"worker state");}
int main(){try{
 root=fs::current_path()/("fixture-"+std::to_string(GetCurrentProcessId()));fs::create_directories(root);
 flight::Configure(true);check(flight::State()==1,"no runtime means waiting");check(fs::is_empty(root),"no startup file before runtime");
 flight::Runtime("fixture VDXR",false,"Quest fixture");waitState(2);
 std::vector<std::thread> producers;
 for(int t=0;t<4;++t)producers.emplace_back([]{for(int i=0;i<10000;++i)flight::Count(flight::Event::Capture);});
 for(auto& t:producers)t.join();
 for(int i=0;i<6;++i)flight::Count(flight::Event::AcceptedReal);
 for(int i=0;i<4;++i)flight::Count(flight::Event::AcceptedSynthetic);
 flight::Count(flight::Event::FallbackRepeat);flight::Count(flight::Event::Marker);
 flight::Period(1000./90);flight::Pace(30,true);flight::Input(0,1000,1000);flight::Output(3000,3000);
 flight::Time(flight::Timing::GenerationGpu,4);flight::Time(flight::Timing::GenerationGpu,8);
 flight::Time(flight::Timing::GenerationGpu,-1);flight::Error(-123);
 DisplayDeadline probe;probe.probeMs=100;probe.remainingMs=8;probe.marginMs=3;probe.periodMs=11.111111;
 probe.handoffMs=101;probe.returnedMs=102;probe.kind=1;probe.accepted=probe.sealed=true;
 for(int i=0;i<100;++i)flight::Display(probe,103,2);
 SourceRecord source;source.frame=16;source.mask=63;source.pose[0]=100;source.pose[1]=101;source.rr=true;source.end[1][2]=96;source.captureStart=100;
 for(int i=0;i<100;++i)flight::Source(source);
 flight::Configure(false);check(!flight::Enabled(),"disabled after stop");
 uint64_t sourceN=0,sourceDropped=0;
 uint64_t captured=0,real=0,fg=0,repeat=0,gpuN=0,probeN=0,probeDropped=0;double gpuSum=0;bool stop=false,header=false;
 for(auto& e:fs::directory_iterator(root)) {std::ifstream file(e.path());std::string line;
 while(std::getline(file,line)){auto j=nlohmann::json::parse(line);if(j["type"]=="header"){header=true;continue;}
 stop=stop||j["type"]=="stop";captured+=j["events"]["capture"].get<uint64_t>();real+=j["events"]["accepted_real"].get<uint64_t>();fg+=j["events"]["accepted_synthetic"].get<uint64_t>();repeat+=j["events"]["fallback_repeat"].get<uint64_t>();
 check(j["settings"]["steamvr_package_requested"]==true && j["steamvr_package_active"]==false,"requested preference differs from VDXR active route");
 check(j["compositor"]["estimate_fps"].is_null(),"VDXR has no fabricated FPS");check(j["last_xr_error"]==-123,"error persisted");
 sourceN+=j["source_probes"].size();sourceDropped+=j["events"]["source_record_dropped"].get<uint64_t>();
 for(const auto& s:j["source_probes"]){check(s["frame"]==16 && s["mask"]==63 && s["rr"]==true && s["eyes"][1]["pose"]==101 && s["eyes"][1]["stages"][2]["end_qpc_ms"]==96 && s["capture_start_qpc_ms"]==100,"source frame and raw timestamps preserved");}
 probeN+=j["display_probes"].size();probeDropped+=j["events"]["display_record_dropped"].get<uint64_t>();
 for(const auto& p:j["display_probes"]){check(p["handoff_qpc_ms"]==101 && p["gpu_end_qpc_ms"]==103 && p["kind"]==1,"paired GPU/CPU timestamps preserved");}
 auto t=j["timings"]["generation_gpu_ms"];auto n=t["n"].get<uint64_t>();gpuN+=n;if(n)gpuSum+=t["mean"].get<double>()*n;
 }}
 check(sourceN>0 && sourceN+sourceDropped==100,"bounded source records and final flush");
 check(probeN>0 && probeN+probeDropped==100,"bounded nonblocking probe records/drop accounting and final flush");
 check(header&&stop,"header and final flush");check(captured==40000&&real==6&&fg==4&&repeat==1,"concurrent counters distinct");check(gpuN==2&&gpuSum==12,"timing samples and invalid rejection");
 flight::Count(flight::Event::Capture);
 for(int i=0;i<3;++i){flight::Configure(true);waitState(2);std::this_thread::sleep_for(std::chrono::milliseconds(15));flight::Configure(false);}
 flight::Stop();flight::Configure(true);check(flight::State()==1,"runtime recreation waits");
 badPath=true;flight::Runtime("fixture",false,"fixture");waitState(-1);check(!flight::Enabled(),"IO failure disables callbacks");flight::Configure(false);flight::Stop();flight::ReleaseConnection();
 std::cout<<"PASS: lifecycle, overlay-off counters, concurrency, null delivery, final flush, restart, IO failure\n";
 return 0;
 }catch(const std::exception& e){std::cerr<<e.what()<<'\n';flight::Stop();return 1;}}
