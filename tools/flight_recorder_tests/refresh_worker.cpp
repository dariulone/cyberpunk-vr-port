#include "Framegen/RefreshCadence.hpp"
#include "xrfg/steamvr_delivery.hpp"
#include <atomic>
#include <chrono>
#include <thread>
#include <iostream>
#include <cstdlib>
using namespace cvr::framegen;
static std::atomic<int64_t> fakePeriod{11111111};
static std::atomic<int> reads{};
// Fake reader exercises worker/lifecycle without opening a real OpenVR context.
namespace xrfg {
struct SteamVrDelivery::Impl {};
SteamVrDelivery::SteamVrDelivery(bool) noexcept{}
SteamVrDelivery::~SteamVrDelivery(){}
void SteamVrDelivery::mark_established() noexcept{}
std::optional<std::chrono::nanoseconds> SteamVrDelivery::display_period() noexcept {++reads;auto p=fakePeriod.load();return p?std::optional(std::chrono::nanoseconds(p)):std::nullopt;}
}
void check(bool v,const char* reason){if(!v){std::cerr<<reason<<'\n';refresh::Stop();std::exit(1);}}
void waitFor(int64_t expected){for(int i=0;i<550&&refresh::Resolve(22222222)!=expected;++i)std::this_thread::sleep_for(std::chrono::milliseconds(10));check(refresh::Resolve(22222222)==expected,"worker refresh update");}
int main(){
 refresh::Start(false);for(int i=0;i<100;++i)refresh::Submitted();check(!refresh::Targeted()&&reads==0,"VDXR must not start refresh reader");
 refresh::Start(true,false);for(int i=0;i<100;++i)refresh::Submitted();check(!refresh::Targeted()&&reads==0,"SteamVR opt-out must not start refresh reader");
 refresh::Start(true,true);check(refresh::Read().periodNs==0,"startup cannot inherit old rate");
 for(int i=0;i<90;++i)refresh::Submitted();waitFor(11111111);
 check(reads>0,"worker reads refresh independent of flight recorder");
 fakePeriod=8333333;waitFor(8333333);
 fakePeriod=11111111;waitFor(11111111);
 fakePeriod=0;waitFor(22222222);
 refresh::Stop();check(!refresh::Targeted()&&refresh::Read().periodNs==0,"stop clears observation");
 refresh::Start(false);check(refresh::Resolve(22222222)==22222222,"recreated VDXR session unaffected");refresh::Stop();
 std::cout<<"PASS:worker lifecycle, successful-submission gate, recorder independence, refresh changes and failed query fallback\n";
}
