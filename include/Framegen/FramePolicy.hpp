#pragma once
#include <cmath>
#include <cstdint>
#include <algorithm>

namespace cvr::framegen {
// OpenXR predictedDisplayPeriod describes the next application frame, not necessarily the physical refresh.
// SteamVR sometimes reports one doubled period while still presenting at 90 Hz. Do not double the producer's
// framegen interval again for that one-frame scheduling decision. A sustained rate change is adopted after 1 s;
// a faster cadence is accepted immediately so throttled startup can recover without an artificial slow frame.
class PresentationPeriod {
    int64_t stable{},candidate{};
    double candidateSince{},last{};
public:
    void Reset() {*this={};}
    int64_t Observe(double now,int64_t reported) {
        if(!std::isfinite(now) || reported<4000000 || reported>40000000)return stable;
        if(!stable || now<last || now-last>1000) {stable=reported;candidate=0;last=now;return stable;}
        last=now;
        if(std::abs(reported-stable)<=stable/1000) {candidate=0;return stable;}
        if(reported<stable) {stable=reported;candidate=0;return stable;}
        if(!candidate || std::abs(reported-candidate)>candidate/1000) {
            candidate=reported;candidateSince=now;
        } else if(now-candidateSince>=1000) {stable=candidate;candidate=0;}
        return stable;
    }
};
// THE REAL-FRAME CADENCE OF FRAME GENERATION: two display periods a real frame (45 of 90 Hz), or three (30) while
// the game cannot make two -- a whole-period lock, as SteamVR motion smoothing and Meta's Application SpaceWarp hold
// an application at an exact fraction of the refresh. An interval between the two is the worst of both: on the
// display grid it mixes two- and three-period frames. The earlier back-off stretched the interval 4% a second on
// missing native neighbours and recovered 0.5% of a period a second: measured at H10, a teleport left it at 25.2 ms
// and 24-30 of a second's 33 real frames took 33 ms -- 30-39 fps on screen while the game itself needed 8-14 ms.
// So the decision is the game's own frame time against the two-period budget, per one-second window; a teleport,
// a load or a skipped capture misses neighbours for reasons a slower cadence does not cure.
class CadenceLock {
    double windowStart{},period{};
    unsigned frames{},over{},tight{};
    bool three{};
public:
    void Reset() {*this={};}
    bool Three() const {return three;}
    // now and work in ms; work is the game's own time for this frame, from its release to the end of its Present.
    double Observe(double now,double displayPeriod,double work) {
        if(!std::isfinite(now) || !std::isfinite(displayPeriod) || displayPeriod<=0) {Reset();return 0;}
        if(displayPeriod!=period || now<windowStart || now-windowStart>2000) {
            *this={};period=displayPeriod;windowStart=now;
        }
        // A load or a hitch (100 ms and more) is not the game's steady cost.
        if(std::isfinite(work) && work>0 && work<100) {
            const double budget=period*2;
            ++frames;
            if(work>budget-1.0)++over;   // this frame could not have made a two-period slot
            if(work>budget-3.0)++tight;  // too close to it to return safely
        }
        if(now-windowStart>=1000) {
            if(frames>=10) {
                if(!three && over*4>=frames)three=true;
                else if(three && tight*20<=frames)three=false;
            }
            windowStart=now;frames=over=tight=0;
        }
        return period*(three?3:2);
    }
};
// THE REAL FRAMES OF FRAME GENERATION, LOCKED TO THE DISPLAY CYCLE (xr_framegen_pace). The XR thread takes the newest
// real frame at each xrWaitFrame return; with a midpoint between two real frames, a real frame is due every other
// cycle. A free-running release (last release + interval) kept whatever phase it started with and drifted: a Present
// landing near the instant the XR thread takes its frame missed it on a fixed rhythm (one display frame in four
// reprojected, re-rolled by every restart). Like the plain refresh cap, the producer is held so its NEXT Present
// lands mid-cycle, the farthest point from that instant, and real frames stay whole display periods apart: two, or
// three while CadenceLock says the game cannot make two. A game slower than that is never held.
class PhaseLockedPacing {
    double target{},released{},work{};
public:
    void Reset() {*this={};}
    double Target() const {return target;}
    double Work() const {return work;}
    // The game's own time for the frame ending now, unsmoothed; NAN before the first release.
    double Elapsed(double now) const {return released>0 && now>=released ? now-released : NAN;}
    // now: the end of this Present; cycle: the latest xrWaitFrame return; all in ms. Returns how long to hold the
    // producer before releasing it. Call Released with the actual release time afterwards.
    double Hold(double now,double cycle,double period,double interval) {
        if(!std::isfinite(now) || !std::isfinite(period) || period<=0 || !std::isfinite(interval)) {Reset();return 0;}
        interval=std::max(interval,period*2);
        // The game's own time from its release to the end of its Present, smoothed over about eight frames.
        if(released>0) {const double w=now-released;if(w>0 && w<100)work=work>0 ? work+(w-work)*.125 : w;}
        const double earliest=now+work;
        // A schedule that fell more than two real frames behind (a load, a menu) starts again from now.
        double desired=target>0 && earliest-target<interval*2 ? std::max(target+interval,earliest) : earliest;
        double next=desired;
        if(std::isfinite(cycle) && now>=cycle && now-cycle<=500) {
            // The first mid-cycle instant at or after the wanted one, a quarter period of slack for a cycle measured
            // again since the previous frame.
            const double start=cycle+period*.5;
            next=start+period*std::ceil((desired-start)/period-.25);
        }
        target=next;
        const double hold=next-work-now;
        return hold>.2 ? std::min(hold,std::min(interval+period,80.0)) : 0;
    }
    void Released(double now) {released=now;}
};
// WHETHER xrWaitFrame's RETURNS ARE A DISPLAY CLOCK. The phase lock above needs them a whole number of predicted
// periods apart. The SteamVR null driver returns 136 times a second against a 90 Hz period: a grid rebuilt from each
// return moved by a fraction of a period every cycle, and the real frames spread from 22 to 30 ms (45 -> 40 fps,
// 552 of 1207 over 25 ms). A runtime whose loop does not keep its period falls back to the plain interval.
class CycleClock {
    double last{},error{};
    unsigned count{};
public:
    void Reset() {*this={};}
    void Tick(double now,double period) {
        if(!std::isfinite(now) || !std::isfinite(period) || period<=0) {Reset();return;}
        if(last>0 && now>last && now-last<500) {
            const double dt=now-last;
            // A cycle the loop skipped still lands on the grid: measured against the nearest whole number of periods.
            const double cycles=std::max(1.0,std::round(dt/period));
            const double e=std::min(1.0,std::abs(dt-cycles*period)/period);
            error=count ? error+(e-error)*.05 : e;++count;
        } else if(last>0) {error=0;count=0;}
        last=now;
    }
    bool Steady() const {return count>=30 && error<.12;}
    double Error() const {return error;}
};
struct Identity {
    uint64_t serial{}, origin{};
    uint32_t nativeFrame{};
    uint64_t pose[2]{};
    double capturedMs{};
    bool stereo{}, inputs{}, reset{};
};
inline bool CanInterpolate(const Identity& a,const Identity& b) {
    const double dt=b.capturedMs-a.capturedMs;
    return a.serial && b.serial>a.serial && a.origin==b.origin && a.origin &&
        uint32_t(b.nativeFrame-a.nativeFrame)==1 &&
        a.stereo && b.stereo && a.inputs && b.inputs && !b.reset &&
        a.pose[0] && a.pose[1] && b.pose[0] && b.pose[1] &&
        std::isfinite(dt) && dt>0 && dt<=100;
}
// A generated midpoint is always followed by its real endpoint. Never replace
// the pending endpoint with a newer capture, or motion reverses/skips time.
class Cadence {
public:
    enum class Next { Repeat, Real, Midpoint, PendingReal };
    Next Select(uint64_t serial,bool pairReady,bool enabled) const {
        if(pending_)return Next::PendingReal;
        if(!serial || serial==displayed_)return Next::Repeat;
        return enabled && pairReady ? Next::Midpoint : Next::Real;
    }
    void Accepted(Next kind,uint64_t serial) {
        if(kind==Next::Midpoint)pending_=serial;
        else if(kind==Next::Real || kind==Next::PendingReal) {displayed_=serial;pending_=0;}
    }
    void Reset() {pending_=displayed_=0;}
    uint64_t Pending() const {return pending_;}
private:
    uint64_t pending_{},displayed_{};
};
}
