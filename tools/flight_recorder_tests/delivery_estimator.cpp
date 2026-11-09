#define CPVR_DELIVERY_TEST
#include "../../src/Framegen/SteamVrReader.cpp"
#include <cmath>
#include <cstdlib>
namespace xrfg {void RecordDeliveryEvent(BridgeFlightOperation,int64_t,uint64_t,uint64_t,uint64_t){}}
static void check(bool v){if(!v)std::abort();}
static double run(unsigned gap,unsigned presents,unsigned mispresent,unsigned predicted,bool burst=false){
 xrfg::SteamVrDelivery::Impl x;
 for(unsigned i=0;i<61;++i){vr::Compositor_FrameTiming t{};t.m_nFrameIndex=i*gap+1;t.m_flSystemTimeInSeconds=10+double(i*gap)/90;t.m_nNumFramePresents=presents;t.m_nNumMisPresented=(burst&&i%4==0)?2:mispresent;t.m_nReprojectionFlags=predicted<<4;x.observe(t);}
 x.close_window(1000000000);return x.delivered;
}
int main(){
 check(std::abs(run(1,1,0,0)-90)<.01);
 check(std::abs(run(2,1,0,0)-45)<.01); // index advances at90 but only45 actual records
 check(std::abs(run(2,2,1,0)-45)<.01); // do not double-charge repeat and late successor
 check(std::abs(run(1,1,2,2)-90)<.01); // prediction is not loss
 check(std::abs(run(1,1,1,0)-90)<.01); // stable timing offset is not loss
 check(run(1,1,0,0,true)<75); // irregular excess does reduce estimate
 xrfg::SteamVrDelivery d(false);d.mark_established();check(!d.delivered_fps(1000000000));
}
