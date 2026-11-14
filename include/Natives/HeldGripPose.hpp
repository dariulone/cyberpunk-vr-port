#pragma once
#include <algorithm>
#include <cmath>

namespace cvr::held_grip {
// Numerical mass scaling makes the held ragdoll stable against its connected bodies; it must not weaken the grip.
// Bound acceleration using the part's original inertia, and damp the angular chase before the target is reached.
inline void DriveAngular(const float* error, const float* velocity, const float* originalInertia,
                         float torque, float step, float maxSpin, float* out) {
    const float inertia=std::max({originalInertia[0],originalInertia[1],originalInertia[2],1e-3f});
    const float response=1-std::exp(-48.f*step);
    float dw[3],n2=0;
    for(int k=0;k<3;++k) {dw[k]=(error[k]*12.f-velocity[k])*response;n2+=dw[k]*dw[k];}
    const float n=std::sqrt(n2),limit=std::max(0.f,torque)/inertia*step;
    const float scale=n>limit ? limit/n : 1.f;
    float speed2=0;
    for(int k=0;k<3;++k) {out[k]=velocity[k]+dw[k]*scale;speed2+=out[k]*out[k];}
    const float speed=std::sqrt(speed2);
    if(speed>maxSpin)for(int k=0;k<3;++k)out[k]*=maxSpin/speed;
}
inline float PinnedForearmLength(float wanted, float minimum, float maximum, bool heldBody) {
    return heldBody ? wanted : std::clamp(wanted, minimum, maximum);
}
inline void Multiply(const float* a, const float* b, float* out) {
    const float q[4] = {
        a[3]*b[0]+a[0]*b[3]+a[1]*b[2]-a[2]*b[1],
        a[3]*b[1]-a[0]*b[2]+a[1]*b[3]+a[2]*b[0],
        a[3]*b[2]+a[0]*b[1]-a[1]*b[0]+a[2]*b[3],
        a[3]*b[3]-a[0]*b[0]-a[1]*b[1]-a[2]*b[2]};
    std::copy_n(q,4,out);
}
inline bool Normalize(float* q) {
    float n=0; for(int i=0;i<4;++i)n+=q[i]*q[i];
    if(!std::isfinite(n) || n<1e-10f)return false;
    n=1/std::sqrt(n);for(int i=0;i<4;++i)q[i]*=n;
    return true;
}
inline void Rotate(const float* q,const float* v,float* out) {
    const float t[3]={2*(q[1]*v[2]-q[2]*v[1]),2*(q[2]*v[0]-q[0]*v[2]),2*(q[0]*v[1]-q[1]*v[0])};
    const float r[3]={v[0]+q[3]*t[0]+q[1]*t[2]-q[2]*t[1],
                      v[1]+q[3]*t[1]+q[2]*t[0]-q[0]*t[2],
                      v[2]+q[3]*t[2]+q[0]*t[1]-q[1]*t[0]};
    std::copy_n(r,3,out);
}
// The held part's measured rotation and its fixed hand-relative rotation determine the wrist.
// Use each hand's own relation, not a two-hand average's error or an angle-limited correction.
inline bool WristRotation(const float* body,const float* bodyInHand,float* wrist) {
    const float inv[4]={-bodyInHand[0],-bodyInHand[1],-bodyInHand[2],bodyInHand[3]};
    Multiply(body,inv,wrist);return Normalize(wrist);
}
// Rotate about the point held, preserving the latest controller-driven grip position.
inline bool SeatRotation(float* position,float* rotation,const float* measured,const float* seat,float blend) {
    float q[4];const float sign=rotation[0]*measured[0]+rotation[1]*measured[1]+rotation[2]*measured[2]+rotation[3]*measured[3]<0?-1.f:1.f;
    const float w=std::clamp(blend,0.f,1.f);
    for(int i=0;i<4;++i)q[i]=rotation[i]*(1-w)+measured[i]*sign*w;
    if(!Normalize(q))return false;
    float before[3],after[3];Rotate(rotation,seat,before);Rotate(q,seat,after);
    for(int i=0;i<3;++i)position[i]+=before[i]-after[i];
    std::copy_n(q,4,rotation);return true;
}
}
