#include "Framegen/Framegen.hpp"
#include "Framegen/RefreshCadence.hpp"
#include "Framegen/FlightRecorder.hpp"
#include "xrfg/steamvr_delivery.hpp"
#include <windows.h>
#include <cstdio>
#include <cstdlib>
#include <string>
#include <chrono>
#include <iostream>
using namespace cvr::framegen;
extern "C" {int CyberpunkVR_RuntimeDiagnostics=0;}
namespace cvr::framegen {
void WakeHardwareTelemetry(){} void StopHardwareTelemetry(){}
HardwareStatistics GetHardwareStatistics(){return {};}
namespace flight {
void Count(Event){} void Configure(bool){} void Time(Timing,double){}
void Input(unsigned,uint32_t,uint32_t){} void Period(double){} void Pace(double,bool){}
}
}
namespace cvr::afw {bool Launch(){return false;}}
namespace xrfg {
struct SteamVrDelivery::Impl {};
SteamVrDelivery::SteamVrDelivery(bool) noexcept{}
SteamVrDelivery::~SteamVrDelivery(){}
void SteamVrDelivery::mark_established() noexcept{}
std::optional<std::chrono::nanoseconds> SteamVrDelivery::display_period() noexcept {return std::chrono::nanoseconds(11111111);}
}
void check(bool v,const char* why){if(!v){std::cerr<<why<<'\n';refresh::Stop();std::exit(1);}}
Settings roundtrip(Settings before) {
 FILE* file{};check(tmpfile_s(&file)==0 && file,"settings temp file");
 WriteSettings(file,before);rewind(file);Settings after;char line[512]{};
 while(fgets(line,sizeof(line),file))check(ParseSetting(line,after),"written key parses");fclose(file);return after;
}
int main(){
 Settings requested;check(requested.steamVrOptimizations,"existing installs keep package default on");
 for(bool package:{false,true})for(bool adaptive:{false,true}) {
  requested.steamVrOptimizations=package;requested.autoPace=adaptive;
  const auto loaded=roundtrip(requested);
  check(loaded.steamVrOptimizations==package && loaded.autoPace==adaptive,"independent settings roundtrip");
 }
 for(bool package:{false,true})for(int retired:{0,1}) {
  Settings legacy;legacy.steamVrOptimizations=package;
  const std::string line="xr_framegen_steamvr_presentation_isolation="+std::to_string(retired);
  check(ParseSetting(line.c_str(),legacy),"retired split key consumed");
  check(legacy.steamVrOptimizations==package,"retired key must never override the unified package");
  const auto loaded=roundtrip(legacy);check(loaded.steamVrOptimizations==package,"unified preference persists after retired key");
 }
 check(!ParseSetting("xr_framegen_steamvr_optimizations=oops",requested),"invalid value ignored");
 check(ParseSetting("xr_framegen_steamvr_optimizations = 0",requested),"disable parses");
 SetSettings(requested);refresh::Start(true,GetSettings().steamVrOptimizations);
 check(!refresh::Targeted() && refresh::Resolve(22222222)==22222222,"SteamVR opt-out keeps runtime cadence");
 requested.steamVrOptimizations=true;SetSettings(requested);
 check(!refresh::Targeted(),"live enable must not change session routing");
 refresh::Stop();refresh::Start(true,GetSettings().steamVrOptimizations);
 check(refresh::Targeted(),"recreated SteamVR uses saved enable");
 requested.steamVrOptimizations=false;SetSettings(requested);
 check(refresh::Targeted(),"live disable must not change active queue/timing route");
 requested.autoPace=false;SetSettings(requested);check(refresh::Targeted(),"adaptive remains independent");
 refresh::Stop();refresh::Start(true,GetSettings().steamVrOptimizations);
 check(!refresh::Targeted(),"recreated SteamVR honors saved disable");
 for(bool package:{false,true}) {
  requested.steamVrOptimizations=package;SetSettings(requested);refresh::Start(false,package);
  check(!refresh::Targeted() && refresh::Resolve(22222222)==22222222,"VDXR unaffected in both settings");
 }
 for(auto b:{Backend::FidelityFX,Backend::Nvidia,Backend::DlssG,Backend::NvidiaV557}){
  requested.backend=b;SetSettings(requested);check(roundtrip(GetSettings()).backend==b,"backend enum roundtrip");
 }
 requested.backend=Backend::NvidiaV557;requested.flowScale=0;SetSettings(requested);
 check(GetSettings().flowScale==50,"V557 requires optical flow");
 refresh::Stop();std::cout<<"PASS: actual settings roundtrip, existing-install default, startup latch, independent adaptive control and VDXR bypass\n";
}
