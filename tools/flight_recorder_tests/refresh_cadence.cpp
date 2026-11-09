#include "Framegen/RefreshCadence.hpp"
#include "Framegen/FramePolicy.hpp"
#include <iostream>
#include <cstdlib>
using namespace cvr::framegen;
using namespace cvr::framegen::refresh;
void check(bool value,const char* reason){if(!value){std::cerr<<reason<<'\n';std::exit(1);}}
int main(){
 constexpr int64_t hz90=11111111,hz120=8333333;
 // A90Hz headset remains90Hz when SteamVR temporarily schedules the app at45/30Hz.
 for(int multiple:{1,2,3})check(Select(true,hz90*multiple,{hz90,1000},1001)==hz90,"SteamVR throttling mistaken for physical refresh");
 // Also covers Quest over Steam Link: no change when runtime and panel rates agree.
 for(int64_t rate:{hz90,hz120,int64_t(13888889)})check(Select(true,rate,{rate,1000},1100)==rate,"unthrottled SteamVR altered");
 // VDXR is explicitly excluded even if a stale SteamVR observation is supplied.
 check(Select(false,hz90*2,{hz90,1000},1100)==hz90*2,"non-SteamVR was changed");
 check(Select(true,hz90*2,{},1100)==hz90*2,"missing refresh fallback");
 check(Select(true,hz90*2,{hz90,1000},7001)==hz90*2,"stale refresh fallback");
 check(Select(true,hz90*2,{hz90,1100},1000)==hz90*2,"future refresh invalid");
 check(Select(true,hz90,{1,1000},1100)==hz90,"invalid refresh fallback");
 check(Select(true,hz90,{hz120,1000},1100)==hz120,"90 to120 change");
 check(Select(true,hz120,{hz90,1000},1100)==hz90,"120 to90 change must not pin minimum");
 AdaptivePacing adaptive;
 for(int i=0;i<1000;++i){auto corrected=Select(true,hz90*(1+i%3),{hz90,1000},1000+i);auto budget=adaptive.Observe(i,corrected*2e-6,false);check(budget>22.22&&budget<22.23,"throttled periods expanded adaptive base");}
 check(Select(true,hz90,{hz90,1000},7000)==hz90,"valid freshness boundary");
 std::cout<<"PASS:90Hz throttling, unthrottled SteamVR, VDXR exclusion, missing/stale/invalid readings, refresh changes, adaptive base\n";
}
