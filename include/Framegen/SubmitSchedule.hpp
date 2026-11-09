#pragma once
#include <algorithm>
#include <cstdint>

namespace cvr::framegen {
// SteamVR handoff acquisition adapted from OFXR V557 (e48d6fcf6289),
// openxr_layer.cpp. See licenses/OFXR-LGPL-3.0.txt. This is a native presenter
// adaptation, not a replacement of the generator or the complete OFXR layer.
// All times below are monotonic nanoseconds; no XR display timestamps change.
class SubmitSchedule {
    int64_t period{},next{},correction{};
    unsigned cooldown{};
    unsigned readyRecovery{};
    bool jumpPending{};
public:
    enum class Gpu { Unknown, Ready, Pending };
    void Reset(){*this={};}
    bool Active() const{return period!=0;}
    int64_t Begin(int64_t now,int64_t scanout,bool enabled) {
        if(!enabled || scanout<4000000 || scanout>40000000){Reset();return 0;}
        if(period!=scanout){Reset();period=scanout;next=now;}
        // Do not replay expired slots after a load/stall. Start acquisition again.
        if(now-next>period){next=now;correction=0;cooldown=0;jumpPending=false;}
        const auto cap=period*(jumpPending?2:1);
        jumpPending=false;
        const auto hold=std::clamp(next-now,int64_t{0},cap);
        if(next>now+cap)next=now+cap;
        return hold;
    }
    // Only fresh real endpoints steer phase. Never fold a whole scanout of
    // error modulo period: that hides the measured 3ms/14ms displacement.
    int64_t Observe(int64_t remaining,int64_t margin,bool real,Gpu gpu=Gpu::Unknown) {
        correction=0;
        // Recovery is measured across real endpoints. A synthetic or repeated
        // frame can legitimately still be pending between two completed real
        // frames; it must neither satisfy nor restart their recovery streak.
        if(!real)return 0;
        // CPU handoff phase is not evidence of scanout phase while this frame's
        // GPU work is pending. Preserve the absolute cadence, but do not chase
        // the delay by adding another whole scanout. No GPU/CPU wait is added.
        if(gpu==Gpu::Pending){readyRecovery=2;jumpPending=false;return 0;}
        if(gpu==Gpu::Unknown)readyRecovery=0; // legacy/shared queue path
        if(!period || remaining<=0 || remaining>period*4)return 0;
        if(readyRecovery && --readyRecovery)return 0;
        const auto error=remaining-margin;
        const bool far=error>period/2;
        if(!far)cooldown=0;
        if(far && !cooldown){
            correction=std::min(error,period*2);cooldown=8;jumpPending=true;
        } else {
            if(error>250000 || error< -250000)
                correction=std::clamp(error/4,-period/2,period/2);
            if(cooldown)--cooldown;
        }
        return correction;
    }
    void End(bool accepted,int64_t returnedAt) {
        if(!accepted){Reset();return;}
        if(period){
            next+=period+correction;
            // V557: keep the absolute grid but skip expired/too-close slots.
            // Resetting every deadline to returnedAt+period adds xrEndFrame's
            // cost to every cycle. Leaving an expired deadline bursts a pair.
            const auto earliest=returnedAt+period/4;
            if(next<earliest)next+=((earliest-next+period-1)/period)*period;
        }
        correction=0;
    }
    bool JumpPending() const{return jumpPending;}
    bool Recovering() const{return readyRecovery!=0;}
};
}
