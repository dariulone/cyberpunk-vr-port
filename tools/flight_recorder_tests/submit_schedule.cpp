#include "Framegen/SubmitSchedule.hpp"
#include "Framegen/FramePolicy.hpp"
#include "xrfg/submit_margin.hpp"
#include <cstdlib>
#include <iostream>
#include <cmath>
using namespace cvr::framegen;
using namespace std::chrono_literals;
void check(bool ok,const char* what){if(!ok){std::cerr<<what<<'\n';std::exit(1);}}
int main(){
    for(int64_t p:{11111111LL,13888889LL,8333333LL}){
        SubmitSchedule s;
        int64_t t=1000000000;
        check(s.Begin(t,p,true)==0,"first cycle unheld");
        for(int i=0;i<120;++i){
            check(s.Observe(3000000,3000000,i%2==0)==0,"converged handoff");
            s.End(true,t);
            check(s.Begin(t+p/3,p,true)==p-p/3,"equal grid for real/synthetic/repeat");
            t+=p;
        }
        const auto jump=s.Observe(3000000+p,3000000,true);
        check(jump==p && s.JumpPending(),"whole scanout lead must not be folded away");
        s.End(true,t);
        check(s.Begin(t,p,true)==2*p,"bounded one-off acquisition hold");
        t+=2*p;
        check(s.Observe(3000000+p,3000000,true)==p/4,"cooldown avoids consecutive whole-scanout jumps");
        s.End(true,t);
        check(s.Begin(t+10*p,p,true)==0,"stalled frames never replay expired slots");
        t+=10*p;
        s.Observe(3000000,3000000,true);s.End(true,t);
        check(s.Begin(t,p,true)==p,"resume single period after a stall");
        check(s.Observe(13000000,3000000,false)==0,"synthetic/repeat cannot steer real phase");
        check(s.Observe(2900000,3000000,true)==0,"deadband");
        check(s.Observe(1000000,3000000,true)==-500000,"bounded earlier acquisition");
        s.End(false,t);check(!s.Active(),"rejected submission resets schedule");
        s.Begin(t,p,true);check(s.Begin(t,8333334,true)==0,"refresh change resets phase");
        s.Begin(t,p,true);s.End(true,t+p*3);
        const auto postStall=s.Begin(t+p*3,p,true);
        check(postStall>=p/4 && postStall<=p,"overrun skips slots without back-to-back handoffs");
        s.Begin(t,p,false);check(!s.Active(),"disabled/other runtime is inert");
        s.Begin(t,p,true);
        for(int i=0;i<120;++i){
            check(s.Observe(3000000+p,3000000,i%2==0,SubmitSchedule::Gpu::Pending)==0,
                "sustained GPU delay must not move the handoff phase");
            check(!s.JumpPending(),"GPU delay must not add a whole-period hold");
            s.End(true,t);check(s.Begin(t+p/3,p,true)==p-p/3,"GPU-pending retains absolute cadence");t+=p;
        }
        check(s.Observe(3000000+p,3000000,true,SubmitSchedule::Gpu::Ready)==0 && s.Recovering(),"one ready real frame cannot steer stale feedback");
        check(s.Observe(3000000+p,3000000,false,SubmitSchedule::Gpu::Ready)==0 && s.Recovering(),"synthetic readiness cannot satisfy recovery");
        check(s.Observe(3000000+p,3000000,false,SubmitSchedule::Gpu::Pending)==0 && s.Recovering(),"pending synthetic does not erase first healthy real endpoint");
        check(s.Observe(0,3000000,true,SubmitSchedule::Gpu::Ready)==0 && s.Recovering(),"invalid feedback cannot satisfy recovery");
        check(s.Observe(3000000+p,3000000,true,SubmitSchedule::Gpu::Ready)==p && !s.Recovering(),"two healthy real endpoints restore acquisition");
        s.End(true,t);s.Begin(t+2*p,p,true);
        s.Observe(3000000+p,3000000,false,SubmitSchedule::Gpu::Pending);
        check(!s.Recovering(),"pending synthetic frame must not arm real endpoint recovery");
        s.End(false,t);check(!s.Recovering(),"failure clears GPU recovery");
        s.Begin(t,p,true);s.Observe(13000000,3000000,true,SubmitSchedule::Gpu::Pending);
        check(s.Observe(3000000+p,3000000,true)==p,"missing probe preserves legacy behavior");
    }
    { // Generated frames can stay pending while real endpoints are healthy.
        // Recovery is defined over real endpoints, not all intervening submits.
        SubmitSchedule s;const int64_t p=11111111,t=1000000000;
        s.Begin(t,p,true);
        s.Observe(p+3000000,3000000,true,SubmitSchedule::Gpu::Pending);
        check(s.Observe(p+3000000,3000000,true,SubmitSchedule::Gpu::Ready)==0,"first healthy real held");
        s.Observe(p+3000000,3000000,false,SubmitSchedule::Gpu::Pending);
        // A genuinely pending REAL endpoint must still restart the recovery.
        s.Observe(p+3000000,3000000,true,SubmitSchedule::Gpu::Pending);
        check(s.Observe(p+3000000,3000000,true,SubmitSchedule::Gpu::Ready)==0 && s.Recovering(),"real pending restarts recovery");
        s.Observe(p+3000000,3000000,false,SubmitSchedule::Gpu::Pending);
        check(s.Observe(p+3000000,3000000,true,SubmitSchedule::Gpu::Ready)==p && !s.Recovering(),"interleaved pending synthetic cannot deadlock real recovery");
        s.End(true,t);s.Begin(t+2*p,p,true);
        for(int i=0;i<60;++i){
            check(s.Observe(3000000,3000000,false,SubmitSchedule::Gpu::Pending)==0,"generated/repeated frames never steer");
            check(!s.Recovering(),"generated/repeated frame cannot rearm real guard");
            check(s.Observe(1000000,3000000,true,SubmitSchedule::Gpu::Ready)==-500000,"real phase feedback remains live under alternating readiness");
        }
    }
    { // Preparation cost must come OUT OF the hold, not be added after it.
        SubmitSchedule s;const int64_t p=11111111,t=1000000000;
        s.Begin(t,p,true);s.End(true,t+500000);
        int64_t now=t;
        for(int64_t work:{300000LL,2400000LL,600000LL,1800000LL}){
            const auto hold=s.Begin(now+work,p,true);
            check(now+work+hold==now+p,"variable real/FG preparation shares an even handoff grid");
            now+=p;s.End(true,now+500000);
        }
    }
    check(xrfg::submit_margin_for_running_start(11111111ns,8011111ns)==2100000ns,"PSVR2 narrow running start leaves 1ms clearance");
    check(xrfg::submit_margin_for_running_start(11111111ns,6500000ns)==3ms,"wide running start uses 3ms");
    check(xrfg::submit_margin_for_running_start(11111111ns,10500000ns)==1500us,"margin safety floor");
    check(xrfg::submit_margin_for_running_start(11111111ns,0ns)==3ms,"missing timing default");
    AdaptivePacing legacy,steam;
    constexpr double base=22.222222;
    double a=0,b=0;
    for(int t=0;t<=10000;t+=100){a=legacy.Observe(t,base,true);b=steam.Observe(t,base,true,true);}
    check(std::abs(a-b)<1e-9 && b>31,"backoff unchanged under persistent misses");
    for(int t=10100;t<=20000;t+=100){a=legacy.Observe(t,base,false);b=steam.Observe(t,base,false,true);}
    check(b==base && a>30,"SteamVR recovers within 10 healthy seconds, legacy unchanged");
    for(int t=20100;t<=200000;t+=100)b=steam.Observe(t,base,true,true);
    check(b<=base*2 && b>=base,"backoff bounds under sustained overload");
    check(steam.Observe(200100,16.666666,false,true)==16.666666,"refresh-change recovery reset");
    std::cout<<"PASS: 72/90/120Hz cadence, unwrapped acquisition, cooldown, stall/failure/reset, margins, recovery\n";
}
