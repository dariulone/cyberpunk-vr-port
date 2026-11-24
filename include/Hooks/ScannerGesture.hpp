#pragma once
#include <atomic>
#include <cmath>
#include <cstdint>

namespace cvr::input {
void PublishScannerVision(int state);
int ScannerVision(); // -1 means no recent game-state sample

// World slow motion belongs to the world, not to tracked hands. Scanner and global time scale are independent
// reasons: leaving one must not resume physics while the other still holds it. Each transition invalidates old work.
class ScannerHandPhysics {
    std::atomic<uint64_t> state{0}; // low bits: scanner, time scale; upper bits: transition generation
    void Reason(uint64_t mask, bool active) {
        auto before=state.load(std::memory_order_acquire);
        for (;;) {
            const auto flags=active ? ((before&3)|mask) : ((before&3)&~mask);
            if(flags==(before&3))return;
            const auto after=((before&~uint64_t(3))+4)|flags;
            if(state.compare_exchange_weak(before,after,std::memory_order_acq_rel))return;
        }
    }
public:
    void Publish(int vision) { if(vision>=0)Reason(1,vision==1); }
    void TimeScale(float scale) { if(std::isfinite(scale) && scale>=0)Reason(2,scale<.999f); }
    uint64_t Epoch() const {
        const auto value=state.load(std::memory_order_acquire);
        return ((value>>2)<<1)|((value&3)!=0 ? 1 : 0);
    }
    bool ScannerActive() const { return (state.load(std::memory_order_acquire)&1)!=0; }
    bool Active() const { return (Epoch() & 1) != 0; }
    bool Accepts(uint64_t completed) const { return !(completed & 1) && Epoch() == completed; }
};
inline ScannerHandPhysics scannerHandPhysics;

class ScannerRequest {
    uint64_t started{};
    bool pending{},accepted{};
public:
    void Update(bool& requested,int vision,uint64_t now) {
        if(!requested){pending=accepted=false;started=0;return;}
        if(!pending){pending=true;started=now;accepted=false;}
        if(vision==1)accepted=true;
        // An input request is not proof that the game opened the scanner.
        // Release rejected/stale requests and respect a game-driven exit.
        if((accepted && vision==0) || (vision!=1 && now>=started && now-started>=750)) {
            requested=false;pending=accepted=false;started=0;
        }
    }
};
// The runtime reports the controller's grip centre, below/outside the ear,
// rather than the point where the fingers touch the headset.
struct ScannerEarZone {
    float x=-0.18f, y=-0.08f, z=0.08f;
    float radius=0.22f, margin=0.04f;
    bool Contains(float px,float py,float pz,bool armed) const {
        if(!std::isfinite(px) || !std::isfinite(py) || !std::isfinite(pz) || px>-.04f)return false;
        const float r0=radius>.02f && radius<=.60f ? radius : .22f;
        const float extra=margin>=0 && margin<=.30f ? margin : .04f;
        const float r=r0+(armed?extra:0);
        const float dx=px-x,dy=py-y,dz=pz-z;
        return dx*dx+dy*dy+dz*dz<=r*r;
    }
};

// Sample the physical LEFT stick before its D-pad/deferred-sprint conversion. Only a new
// press inside the settled LEFT ear zone belongs to the scanner. Ownership lasts through
// release, even after leaving the zone or opening a menu, so it cannot become a late sprint.
class ScannerClick {
    bool held{},primed{},claimed{},armed{};
    double inMs{},outMs{};
    uint64_t presses{};
public:
    bool Update(bool enabled,bool down,bool valid,float x,float y,float z,double dtMs,
                const ScannerEarZone& zone={},int enterMs=80,int exitMs=250) {
        const bool was=held;
        held=down;
        const bool releasing=claimed && !down;
        if(!down)claimed=false;
        if(!enabled || dtMs>250) {
            // A tracking/input pause cannot turn an unobserved press into a fresh one.
            armed=primed=false;inMs=outMs=0;
        } else {
            if(!std::isfinite(dtMs) || dtMs<0)dtMs=0;
            if(dtMs>100)dtMs=100;
            const bool inside=valid && zone.Contains(x,y,z,armed);
            if(inside){inMs+=dtMs;outMs=0;}else{outMs+=dtMs;inMs=0;}
            if(!armed && inMs>=(enterMs>0?enterMs:80))armed=true;
            if(armed && outMs>=(exitMs>0?exitMs:250))armed=false;
            if(!down)primed=true;
            if(down && !was && primed && armed && inside) {claimed=true;++presses;}
        }
        return claimed || releasing;
    }
    uint64_t Presses() const {return presses;}
    bool Held() const {return held && claimed && armed;}
};

// The raw click's other owner. A scanner-owned hold/release never generates a deferred L3
// pulse or increments the sprint counter. Ordinary L3 and D-pad chords keep their old policy.
class DeferredLeftStickClick {
    bool held{},used{};
public:
    bool Update(bool down,bool otherOwner) {
        if(otherOwner)used=true;
        if(down){held=true;return false;}
        const bool emit=held && !used;
        held=used=false;
        return emit;
    }
};
}
