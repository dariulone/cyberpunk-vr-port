#pragma once
#include <cmath>
#include <cstdint>
#include <algorithm>

namespace cvr::framegen {
class AdaptivePacing {
    double interval{},base{},windowStart{},lastMiss{};
    unsigned misses{};
public:
    void Reset() {*this={};}
    double Observe(double now,double minimumInterval,bool missed) {
        if(!std::isfinite(now) || !std::isfinite(minimumInterval) || minimumInterval<=0)return interval;
        if(!interval || base!=minimumInterval || now<windowStart || now-windowStart>2000) {
            interval=base=minimumInterval;windowStart=lastMiss=now;misses=0;
        }
        if(missed){++misses;lastMiss=now;}
        if(now-windowStart>=1000) {
            // Back off on repeated missing neighbors, recover slowly after a
            // healthy interval. A single loading/camera spike must not cap FPS.
            if(misses>=2)interval=std::min(base*2,interval*1.04);
            else if(!misses && now-lastMiss>=5000)interval=std::max(base,interval-base*.005);
            windowStart=now;misses=0;
        }
        return interval;
    }
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
