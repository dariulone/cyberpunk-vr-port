#pragma once
#include <algorithm>
#include <cmath>
#include <cstdint>

#if CVR_RUST_VRIK
// The yaw-follow in Rust (rust/crates/cvr-xr/src/hud.rs): the structs keep their layout and call it.
extern "C" {
float cvr_hud_head_yaw(float x, float y, float z, float w, float fallback);
float cvr_hud_clock_step(void* clock, int64_t time);
float cvr_hud_follow_update(void* follow, float head, float cone, float dt, bool delayedCatchup);
}
namespace cvr::hud {
inline float HeadYaw(float x,float y,float z,float w,float fallback) { return cvr_hud_head_yaw(x,y,z,w,fallback); }
struct DisplayClock {
    int64_t last=0;
    float elapsed=0;
    void Reset() { last=0; elapsed=0; }
    float Step(int64_t time) { return cvr_hud_clock_step(this, time); }
};
struct Follow {
    float yaw = 0;
    bool valid = false;
    bool following = false;
    bool delayPending = false;
    float stillYaw = 0;
    double stillSeconds = 0;
    void ResetDelay() { delayPending=false; stillSeconds=0; }
    void Reset() { valid = following = false; ResetDelay(); }
    float Update(float head, float cone, float dt, bool delayedCatchup=false) {
        return cvr_hud_follow_update(this, head, cone, dt, delayedCatchup);
    }
};
static_assert(sizeof(DisplayClock) == 16 && sizeof(Follow) == 24, "mirrored by rust/crates/cvr-xr/src/hud.rs");
}
#else
namespace cvr::hud {
inline float HeadYaw(float x,float y,float z,float w,float fallback) {
    const float n=x*x+y*y+z*z+w*w;
    if(!std::isfinite(n) || n<0.5f || n>1.5f) return fallback;
    const float inv=1/std::sqrt(n);x*=inv;y*=inv;z*=inv;w*=inv;
    const float fx=2*(w*y+x*z),fz=1-2*(x*x+y*y);
    // Looking almost straight up/down has no stable horizontal heading.
    if(fx*fx+fz*fz<0.01f) return fallback;
    return std::atan2(fx,fz);
}
// Use the runtime's nanosecond display timeline, not GetTickCount's 15.6 ms ticks.
struct DisplayClock {
    int64_t last=0;
    float elapsed=0;
    void Reset() { last=0; elapsed=0; }
    float Step(int64_t time) {
        elapsed=0;
        if(time<=0) return 0;
        elapsed=last && time>last ? float(double(time-last)*1e-9) : 0;
        last=time;return std::clamp(elapsed,0.0f,0.05f);
    }
};
// Yaw only, like the menu panel. A completed catch-up starts a fresh free zone.
struct Follow {
    float yaw = 0;
    bool valid = false;
    bool following = false;
    bool delayPending = false;
    float stillYaw = 0;
    double stillSeconds = 0;
    static float Wrap(float a) { return std::remainder(a, 6.28318530718f); }
    void ResetDelay() { delayPending=false; stillSeconds=0; }
    void Reset() { valid = following = false; ResetDelay(); }
    float Update(float head, float cone, float dt, bool delayedCatchup=false) {
        if (!std::isfinite(head)) { ResetDelay(); return yaw; }
        if (!valid) { yaw = head; valid = true; following = false; ResetDelay(); return yaw; }
        float delta = Wrap(head - yaw);
        if (std::abs(delta) > cone) following = true;
        // Menus/HUD can settle back after a turn inside the free-look cone.
        // Measure rest against a fixed heading, allowing 0.5 deg of HMD jitter;
        // sustained turning moves that heading and restarts the three-second wait.
        constexpr float minDelayAngle=0.1745329252f, restTolerance=0.00872664626f;
        const bool delayRange=delayedCatchup && std::abs(delta)>minDelayAngle && std::abs(delta)<cone;
        if (following || !delayRange || !std::isfinite(dt) || dt<=0 || dt>0.25f) {
            ResetDelay();
        } else if (!delayPending || std::abs(Wrap(head-stillYaw))>restTolerance) {
            delayPending=true; stillYaw=head; stillSeconds=0;
        } else {
            stillSeconds+=dt;
            if (stillSeconds>=3.0) { following=true; ResetDelay(); }
        }
        if (following) {
            const float step = 3.0f * (std::isfinite(dt) ? std::clamp(dt, 0.0f, 0.05f) : 0.0f);
            yaw = Wrap(yaw + std::clamp(delta, -step, step));
            if (std::abs(Wrap(head - yaw)) <= 0.034906585f) following = false;
        }
        return yaw;
    }
};
}
#endif
