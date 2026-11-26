#pragma once

#include <cmath>

namespace cvr::anim {
// Rendered hand displacement and its derivative. Advance the existing damped
// pose spring over the actual solve interval, including a fractional frame.
struct HandRecoilSpring {
    float x{},v{};
    void Advance(float omega,float dt) {
        if(dt<=0 || (x==0 && v==0))return;
        constexpr float damping=.55f;
        const float decay=damping*omega;
        const float frequency=omega*std::sqrt(1.f-damping*damping);
        const float envelope=std::exp(-decay*dt),c=std::cos(frequency*dt);
        const float s=std::sin(frequency*dt)/frequency;
        const float nextX=envelope*(x*c+(v+decay*x)*s);
        v=envelope*(v*c-(decay*v+omega*omega*x)*s);
        x=nextX;
        if(std::fabs(x)<1e-5f && std::fabs(v)<1e-4f){x=0;v=0;}
    }
};
}
