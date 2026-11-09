#include "Framegen/DisplayDeadline.hpp"
#include "Framegen/SteamVrPacer.hpp"
#include "Framegen/RefreshCadence.hpp"
#include "Framegen/FlightRecorder.hpp"
#include "xrfg/steamvr_delivery.hpp"
#include <array>
#include <cstdlib>
#include <iostream>
#include <thread>
#include <chrono>
using namespace cvr::framegen;
bool targeted=false,available=true;
int64_t physical=11111111,remaining=2100000;
int reads=0,attached=0;
bool gpuReady=false;unsigned gpuReads=0;
bool Ready(uint64_t receipt){if(receipt!=7)std::abort();++gpuReads;return gpuReady;}
std::array<unsigned,size_t(flight::Event::Count)> events{};
std::array<unsigned,size_t(flight::Timing::Count)> times{};
std::array<double,size_t(flight::Timing::Count)> maxTime{};
namespace cvr::framegen::refresh {
bool Targeted(){return targeted;}
Observation Read(){return {physical,100};}
}
namespace cvr::framegen::flight {
void Count(Event e){++events[size_t(e)];}
void Time(Timing t,double ms){if(ms>=0){++times[size_t(t)];maxTime[size_t(t)]=std::max(maxTime[size_t(t)],ms);}}
}
namespace xrfg {
struct SteamVrDelivery::Impl {};
SteamVrDelivery::SteamVrDelivery(bool) noexcept{}
SteamVrDelivery::~SteamVrDelivery(){}
void SteamVrDelivery::mark_established() noexcept{++attached;}
std::optional<SteamVrDelivery::FramePresentation> SteamVrDelivery::last_presentation() noexcept{
    ++reads;FramePresentation f{};f.compositor_render_start_ms=8.011111f;return f;
}
std::optional<std::chrono::nanoseconds> SteamVrDelivery::frame_time_remaining() noexcept{
    ++reads;return available?std::optional(std::chrono::nanoseconds(remaining)):std::nullopt;
}
}
void check(bool v,const char* why){if(!v){std::cerr<<why<<'\n';std::exit(1);}}
int main(){
    SteamVrPacer p;
    const auto cycle=[&](bool enabled,SteamVrPacer::Kind kind,bool accepted=true){p.Begin(enabled);p.BeforeSubmit(kind);p.AfterSubmit(accepted);};
    cycle(true,SteamVrPacer::Kind::Real);
    check(reads==0 && attached==0,"VDXR bypasses timer and OpenVR attachment");
    targeted=true;physical=0;cycle(true,SteamVrPacer::Kind::Real);
    check(reads==0,"no physical sample means no pacing/attachment");
    physical=11111111;cycle(false,SteamVrPacer::Kind::Real);
    check(reads==0,"disabled/menu path bypasses pacing");
    cycle(true,SteamVrPacer::Kind::Real);cycle(true,SteamVrPacer::Kind::Synthetic);cycle(true,SteamVrPacer::Kind::Repeat);
    check(events[size_t(flight::Event::PacedCycle)]==3,"all three kinds paced");
    check(times[size_t(flight::Timing::RealEnd)]==1 && times[size_t(flight::Timing::SyntheticEnd)]==1 && times[size_t(flight::Timing::RepeatEnd)]==1,"split real/synth/repeat diagnostics");
    const auto paced=events[size_t(flight::Event::PacedCycle)];
    p.Begin(true);
    check(events[size_t(flight::Event::PacedCycle)]==paced,"Begin arms without consuming preparation interval");
    std::this_thread::sleep_for(std::chrono::milliseconds(3)); // simulated native preparation
    p.BeforeSubmit(SteamVrPacer::Kind::Synthetic);p.AfterSubmit(true);
    check(maxTime[size_t(flight::Timing::PrepareBeforeHold)]>=2,"preparation happens before the pacing hold");
    available=false;cycle(true,SteamVrPacer::Kind::Real);
    check(events[size_t(flight::Event::PacerUnavailable)]==0 && events[size_t(flight::Event::PacerNoReading)]==1,"isolated read failure keeps grid");
    cycle(true,SteamVrPacer::Kind::Synthetic);cycle(true,SteamVrPacer::Kind::Real);
    check(events[size_t(flight::Event::PacerUnavailable)]==1,"three missing readings fall back");
    available=true;remaining=0;cycle(true,SteamVrPacer::Kind::Real);
    check(events[size_t(flight::Event::PacerNonpositive)]==1,"deadline rollover distinguished from missing reader");
    remaining=2100000;cycle(true,SteamVrPacer::Kind::Real);
    available=false;cycle(true,SteamVrPacer::Kind::Real);cycle(true,SteamVrPacer::Kind::Real);
    check(events[size_t(flight::Event::PacerUnavailable)]==1,"good reading clears failure streak");
    available=true;cycle(true,SteamVrPacer::Kind::Real,false);
    p.Reset();remaining=13211111;
    const auto jumps=events[size_t(flight::Event::PhaseJump)];
    const auto guarded=[&]{p.Begin(true);p.BeforeSubmit(SteamVrPacer::Kind::Real,7,&Ready);p.AfterSubmit(true);};
    for(int i=0;i<6;++i)guarded();
    check(gpuReads==6 && events[size_t(flight::Event::PhaseGpuPending)]==6,"actual pacer probes readiness even without recording");
    check(events[size_t(flight::Event::PhaseJump)]==jumps,"pending GPU cannot cause phase jumps");
    gpuReady=true;guarded();
    check(events[size_t(flight::Event::PhaseGpuRecovery)]==1 && events[size_t(flight::Event::PhaseJump)]==jumps,"recovery requires two ready real frames");
    gpuReady=false;
    p.Begin(true);p.BeforeSubmit(SteamVrPacer::Kind::Synthetic,7,&Ready);p.AfterSubmit(true);
    p.Begin(true);p.BeforeSubmit(SteamVrPacer::Kind::Repeat,7,&Ready);p.AfterSubmit(true);
    check(events[size_t(flight::Event::PhaseJump)]==jumps,"pending synthetic/repeat never steer phase");
    gpuReady=true;
    guarded();check(events[size_t(flight::Event::PhaseJump)]==jumps+1,"healthy real endpoints restore phase despite pending generated/repeated frames");
    remaining=2100000;DisplayDeadline probe;
    p.Begin(true);p.BeforeSubmit(SteamVrPacer::Kind::Synthetic,7,&Ready,&probe);
    probe.handoffMs=DeadlineQpcMs();probe.returnedMs=DeadlineQpcMs();probe.accepted=probe.sealed=true;
    check(probe.kind==1 && probe.remainingMs==2.1 && probe.periodMs>11 && probe.marginMs>0,
        "production pacer records matching runtime deadline without changing feedback");
    check(AttributeDisplay(probe,probe.handoffMs).has_value(),"pacer QPC timeline is classifiable");
    p.AfterSubmit(true);
    // Production pacer invokes preparation only during real/repeat idle time.
    unsigned prepares=0;
    const auto prep=+[](void* context){++*static_cast<unsigned*>(context);return true;};
    p.Reset();physical=20000000;remaining=2100000;
    cycle(true,SteamVrPacer::Kind::Real); // establish next grid slot
    p.Begin(true);p.BeforeSubmit(SteamVrPacer::Kind::Real,0,nullptr,nullptr,nullptr,prep,&prepares);p.AfterSubmit(true);
    check(prepares==1,"real pacing hold prepares following pair");
    p.Begin(true);p.BeforeSubmit(SteamVrPacer::Kind::Synthetic,0,nullptr,nullptr,nullptr,prep,&prepares);p.AfterSubmit(true);
    check(prepares==1,"synthetic output cannot be overwritten during its own presentation");
    p.Begin(true);p.BeforeSubmit(SteamVrPacer::Kind::Repeat,0,nullptr,nullptr,nullptr,prep,&prepares);p.AfterSubmit(true);
    check(prepares==2,"repeat pacing hold can prepare next pair");
    p.Reset();targeted=false;const auto before=reads;cycle(true,SteamVrPacer::Kind::Real);
    check(reads==before,"recreated non-SteamVR session stays untouched");
    std::cout<<"PASS: native pacer runtime gate, recorder-independent operation, timing failure and session lifecycle\n";
}
