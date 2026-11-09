#include "Framegen/SteamVrPacer.hpp"
#include "Framegen/SubmitSchedule.hpp"
#include "Framegen/IdlePrepare.hpp"
#include "Framegen/DisplayDeadline.hpp"
#include "Framegen/RefreshCadence.hpp"
#include "Framegen/FlightRecorder.hpp"
#include "xrfg/steamvr_delivery.hpp"
#include "xrfg/submit_margin.hpp"
#include <windows.h>
#include <chrono>
#include <cmath>

namespace cvr::framegen {
namespace {
using Clock=std::chrono::steady_clock;
int64_t Now(){return std::chrono::duration_cast<std::chrono::nanoseconds>(Clock::now().time_since_epoch()).count();}
double Ms(int64_t ns){return double(ns)*1e-6;}
}
struct SteamVrPacer::Impl {
    SubmitSchedule schedule;
    xrfg::SteamVrDelivery reader{true};
    HANDLE timer{};
    int64_t period{},runningStart{},lastSubmit{},entered{},cycleStarted{};
    unsigned missingReadings{};
    Kind kind{Kind::Repeat};
    ~Impl(){if(timer)CloseHandle(timer);}
    void Clear(){schedule.Reset();period=runningStart=lastSubmit=entered=cycleStarted=0;missingReadings=0;}
};
SteamVrPacer::SteamVrPacer()=default;
SteamVrPacer::~SteamVrPacer()=default;
void SteamVrPacer::Reset(){impl.reset();}
void SteamVrPacer::Begin(bool enabled) {
    // Read() is populated by the established-session worker. This gate avoids
    // opening an OpenVR connection on the XR thread or on VDXR/probe sessions.
    const auto physical=refresh::Read();
    if(!enabled || !refresh::Targeted() || !physical.periodNs){if(impl)impl->Clear();return;}
    if(!impl)impl=std::make_unique<Impl>();
    auto& p=*impl;
    p.reader.mark_established();
    if(p.period!=physical.periodNs){p.Clear();p.period=physical.periodNs;}
    if(!p.timer){
        p.timer=CreateWaitableTimerExW(nullptr,nullptr,CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,TIMER_ALL_ACCESS);
        if(!p.timer)p.timer=CreateWaitableTimerExW(nullptr,nullptr,0,TIMER_ALL_ACCESS);
    }
    if(!p.timer){p.Clear();flight::Count(flight::Event::PacerTimerFailed);flight::Count(flight::Event::PacerUnavailable);return;}
    p.cycleStarted=Now();
}
void SteamVrPacer::BeforeSubmit(Kind kind,uint64_t receipt,bool (*ready)(uint64_t),DisplayDeadline* probe,void* wake,bool (*prepare)(void*),void* context) {
    if(!impl || !impl->period)return;
    auto& p=*impl;
    // Native interpolation is queued by Select(). Unlike the complete V557
    // layer, it is not already prepared before this XR loop selects an image.
    // Spend the pacing hold AFTER queuing generation/copies, giving the GPU
    // this interval to finish and placing both frame kinds on the same grid.
    const auto holdStart=Now();
    flight::Time(flight::Timing::PrepareBeforeHold,Ms(holdStart-p.cycleStarted));
    const auto hold=p.schedule.Begin(holdStart,p.period,true);
    if(hold>0){
        LARGE_INTEGER due{};due.QuadPart=-std::max<int64_t>(1,hold/100);
        if(!SetWaitableTimerEx(p.timer,&due,0,nullptr,nullptr,nullptr,0)){
            p.Clear();flight::Count(flight::Event::PacerTimerFailed);flight::Count(flight::Event::PacerUnavailable);return;
        }
        bool waited=false;
        if(kind!=Kind::Synthetic && prepare){
            const auto idle=WaitWithPreparation(p.timer,HANDLE(wake),holdStart+hold,prepare,context,[]{return Now();});
            waited=idle.success;
            for(unsigned i=0;i<idle.attempts;++i)flight::Count(flight::Event::PrepareAheadAttempt);
            if(idle.attempts)flight::Time(flight::Timing::PrepareAheadCpu,idle.cpuMs);
            if(idle.overrunMs>0){flight::Count(flight::Event::PrepareAheadOverrun);flight::Time(flight::Timing::PrepareAheadOverrun,idle.overrunMs);}
        }else waited=WaitForSingleObject(p.timer,DWORD(hold/1000000+3))==WAIT_OBJECT_0;
        if(!waited){
            CancelWaitableTimer(p.timer);p.Clear();flight::Count(flight::Event::PacerTimerFailed);flight::Count(flight::Event::PacerUnavailable);return;
        }
    }
    flight::Count(flight::Event::PacedCycle);
    flight::Time(flight::Timing::SubmitHold,Ms(Now()-holdStart));
    const auto start=Now();
    // A real compositor record updates the margin; smoothing and 1ms clearance
    // follow V557. Invalid records retain the conservative default 3ms margin.
    if(auto frame=p.reader.last_presentation()){
        const auto ms=frame->compositor_render_start_ms;
        if(std::isfinite(ms) && ms>0 && ms*1e6<p.period){
            const auto ns=int64_t(ms*1e6);
            p.runningStart=p.runningStart?p.runningStart+(ns-p.runningStart)/16:ns;
        }
    }
    const auto margin=xrfg::submit_margin_for_running_start(
        std::chrono::nanoseconds(p.period),std::chrono::nanoseconds(p.runningStart)).count();
    const double probeStart=probe?DeadlineQpcMs():0;
    const auto remaining=p.reader.frame_time_remaining();
    if(probe){
        const double probeEnd=DeadlineQpcMs();
        probe->probeMs=(probeStart+probeEnd)*.5;probe->probeWidthMs=probeEnd-probeStart;
        probe->periodMs=Ms(p.period);probe->remainingMs=remaining?Ms(remaining->count()):0;
        probe->marginMs=Ms(margin);probe->kind=unsigned(kind);
    }
    flight::Time(flight::Timing::SubmitProbe,Ms(Now()-start));
    const auto gpu=receipt && ready ? (ready(receipt)?SubmitSchedule::Gpu::Ready:SubmitSchedule::Gpu::Pending):SubmitSchedule::Gpu::Unknown;
    if(gpu==SubmitSchedule::Gpu::Pending)flight::Count(flight::Event::PhaseGpuPending);
    const auto step=p.schedule.Observe(remaining?remaining->count():0,margin,kind==Kind::Real,gpu);
    if(kind==Kind::Real && gpu==SubmitSchedule::Gpu::Ready && p.schedule.Recovering())
        flight::Count(flight::Event::PhaseGpuRecovery);
    if(remaining && remaining->count()>0 && remaining->count()<=p.period*4){
        p.missingReadings=0;
        flight::Time(kind==Kind::Real?flight::Timing::RealRemaining:
            kind==Kind::Synthetic?flight::Timing::SyntheticRemaining:flight::Timing::RepeatRemaining,Ms(remaining->count()));
        flight::Time(flight::Timing::SubmitMargin,Ms(margin));
        if(step){
            flight::Count(p.schedule.JumpPending()?flight::Event::PhaseJump:
                step>0?flight::Event::PhaseLater:flight::Event::PhaseEarlier);
            flight::Time(flight::Timing::PhaseStep,Ms(std::abs(step)));
        }
    } else {
        // V557 skips feedback when a reading is unavailable. An isolated
        // rollover must not destroy a healthy grid and immediately reacquire.
        // Bound the grace interval; sustained loss falls back to runtime pacing.
        flight::Count(!remaining?flight::Event::PacerNoReading:
            remaining->count()<=0?flight::Event::PacerNonpositive:flight::Event::PacerOutOfRange);
        if(++p.missingReadings>=3){p.Clear();flight::Count(flight::Event::PacerUnavailable);return;}
    }
    p.kind=kind;p.entered=Now();
    if(p.lastSubmit)flight::Time(kind==Kind::Real?flight::Timing::RealHandoffGap:
        kind==Kind::Synthetic?flight::Timing::SyntheticHandoffGap:flight::Timing::RepeatHandoffGap,Ms(p.entered-p.lastSubmit));
    p.lastSubmit=p.entered;
}
void SteamVrPacer::AfterSubmit(bool accepted) {
    if(!impl || !impl->schedule.Active())return;
    auto& p=*impl;
    if(p.entered)flight::Time(p.kind==Kind::Real?flight::Timing::RealEnd:
        p.kind==Kind::Synthetic?flight::Timing::SyntheticEnd:flight::Timing::RepeatEnd,Ms(Now()-p.entered));
    p.schedule.End(accepted,Now());p.entered=0;
    if(!accepted)p.Clear();
}
}
