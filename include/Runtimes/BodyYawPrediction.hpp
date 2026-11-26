#pragma once
#include <algorithm>
#include <array>
#include <cmath>
#if CVR_RUST_BODY
#include "Rust/CvrRust.h"
#endif

namespace cvr::body {
// Prediction operates only on an already accepted three-device yaw. It never
// treats a wrist gesture, a tracking reset, or filter catch-up as angular speed.
class BodyYawPrediction {
    float previous{},smooth{},value{},rate{},previousRate{},interval{},sinceChange{};
    bool initialized{},moving{};
    struct Point{float time{},yaw{};};
    std::array<Point,6> history{};
    unsigned count{};
    static float Wrap(float a){return std::remainder(a,6.28318530718f);}
public:
#if CVR_RUST_BODY
    // rust/crates/cvr-body/src/prediction.rs; this class's fields are its layout.
    void Reset(float seed=0){cvr_body_yaw_prediction_reset(this,seed);}
    void Suspend(){cvr_body_yaw_prediction_suspend(this);}
    float Update(float measured,float dt,bool accepted){return cvr_body_yaw_prediction_update(this,measured,dt,accepted);}
    float Sample(float ageSeconds) const {return cvr_body_yaw_prediction_sample(this,ageSeconds);}
#else
    void Reset(float seed=0){*this={};previous=smooth=value=seed;}
    void Suspend(){initialized=false;rate=previousRate=interval=sinceChange=0;count=0;moving=false;smooth=value;}
    float Update(float measured,float dt,bool accepted){
        if(!std::isfinite(measured) || !std::isfinite(dt) || dt<=0)return value;
        if(!initialized){previous=measured;initialized=true;}
        const float change=Wrap(measured-previous);
        const bool changed=std::abs(change)>.00001f;
        sinceChange+=dt;
        const float instant=changed ? change/sinceChange:0;
        previous=measured;
        if(changed){interval=sinceChange;sinceChange=0;}
        const bool usable=accepted && dt<=.05f && std::abs(instant)<=12.566371f;
        // Keep the regression window local in both time and angle. Long sessions
        // and repeated 360-degree turns cannot lose precision in an accumulator.
        for(unsigned i=0;i<count;++i){history[i].time-=dt;history[i].yaw-=change;}
        if(!usable || sinceChange>.05f)count=0;
        if(usable) {
            if(count==history.size()){std::move(history.begin()+1,history.end(),history.begin());--count;}
            history[count++]={0,0};
            while(count>3 && history[0].time<-.1f){std::move(history.begin()+1,history.begin()+count,history.begin());--count;}
        }
        float fitted=0,error=0,span=count>1 ? history[count-1].time-history[0].time:0;
        bool stable=false;
        if(count>=3 && span>=.018f && span<=.15f) {
            float mt=0,my=0;
            for(unsigned i=0;i<count;++i){mt+=history[i].time-history[0].time;my+=history[i].yaw-history[0].yaw;}
            mt/=count;my/=count;float tt=0,ty=0;
            for(unsigned i=0;i<count;++i){const float t=history[i].time-history[0].time-mt,y=history[i].yaw-history[0].yaw-my;tt+=t*t;ty+=t*y;}
            if(tt>1e-8f)fitted=ty/tt;
            for(unsigned i=0;i<count;++i){const float r=history[i].yaw-history[0].yaw-my-fitted*(history[i].time-history[0].time-mt);error+=r*r;}
            error=std::sqrt(error/count);
            stable=std::abs(fitted)>.174532925f && std::abs(fitted)<=12.566371f &&
                error<=std::max(.006f,std::abs(fitted)*std::min(interval,.035f)*.65f) &&
                !(change*fitted<0 && std::abs(change)>.004363323f);
        }
        previousRate=stable ? fitted:0;
        const bool stopped=usable && moving && !changed;
        if(stable && changed)rate=fitted;
        else rate=0;
        const float tau=stable ? .010f:.035f;
        const float alpha=-std::expm1(-std::min(dt,.1f)/tau);
        smooth=Wrap(smooth+Wrap(measured-smooth)*alpha);
        float target=smooth;
        if(stable) {
            // Exact first-order filter lag for this discrete sample interval.
            // The compensation is bounded by what the devices already measured;
            // it cannot overshoot an observed stop or accelerate on stale data.
            const float lag=dt/std::expm1(dt/tau);
            const float residual=Wrap(measured-smooth);
            const float lead=previousRate*lag;
            if(lead*residual>0)target=Wrap(smooth+std::copysign(std::min(std::abs(lead),std::abs(residual)),lead));
        }
        if(stopped){target=measured;smooth=measured;}
        // Losing prediction confidence must not make a still-forward measured
        // target pull the body backward toward the lagging low-pass state.
        const float measuredError=Wrap(measured-value),proposed=Wrap(target-value);
        if(measuredError*proposed<0)target=value;
        const float limit=12.566371f*dt;
        value=Wrap(value+std::clamp(Wrap(target-value),-limit,limit));
        moving=stable;
        return value;
    }
    float Sample(float ageSeconds) const {
        if(!std::isfinite(ageSeconds) || ageSeconds<0 || rate==0)return value;
        // One observed input interval, at most 15 ms and three degrees. Holding
        // a stale frame cannot integrate motion indefinitely. No additional
        // render/prediction horizon is guessed on top of OpenXR prediction.
        const float horizon=std::min({ageSeconds,interval,.015f});
        return Wrap(value+std::clamp(rate*horizon,-.052359878f,.052359878f));
    }
#endif
    float Value() const{return value;}
    float Rate() const{return rate;}
};
#if CVR_RUST_BODY
static_assert(sizeof(BodyYawPrediction) == 84);
#endif
}
