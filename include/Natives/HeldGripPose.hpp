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
// The visual wrist may already be rotated by the body's weight. Keep its
// reachable grip point, but retain the controller's independent angular goal;
// feeding the corrected wrist back would cancel the torque that should turn
// the held part toward the controller.
inline bool DriveTarget(const float* drawnP,const float* drawnQ,const float* controlQ,
                        const float* bodyP,const float* bodyQ,const float* grip,float* outP,float* outQ) {
    float draw[4],control[4],relation[4];
    std::copy_n(drawnQ,4,draw);std::copy_n(controlQ,4,control);std::copy_n(bodyQ,4,relation);
    if(!Normalize(draw)||!Normalize(control)||!Normalize(relation))return false;
    float inHand[3],point[3],rotatedGrip[3];Rotate(relation,grip,inHand);
    for(int k=0;k<3;++k)inHand[k]+=bodyP[k];
    Rotate(draw,inHand,point);Multiply(control,relation,outQ);
    if(!Normalize(outQ))return false;
    Rotate(outQ,grip,rotatedGrip);
    for(int k=0;k<3;++k){outP[k]=drawnP[k]+point[k]-rotatedGrip[k];if(!std::isfinite(outP[k]))return false;}
    return true;
}
// Two separated grips determine the body's swing as well as its translation.
// Keep the average wrist twist, then align the line through the body's grips
// with the line through the hands. Averaging wrist rotations alone left a body
// horizontal when one hand was raised, despite both hands holding it.
inline bool TwoHandTarget(const float* aP,const float* aQ,const float* aGrip,
                          const float* bP,const float* bQ,const float* bGrip,
                          float* outP,float* outQ,float* centre) {
    float qa[4],qb[4];std::copy_n(aQ,4,qa);std::copy_n(bQ,4,qb);
    if(!Normalize(qa)||!Normalize(qb))return false;
    float dot=0;for(int k=0;k<4;++k)dot+=qa[k]*qb[k];
    const float sign=dot<0 ? -1.f:1.f;
    for(int k=0;k<4;++k)outQ[k]=qa[k]+sign*qb[k];
    if(!Normalize(outQ))return false;
    float ga[3],gb[3],local[3],world[3],mid[3],from[3];
    Rotate(qa,aGrip,ga);Rotate(qb,bGrip,gb);
    float local2=0,world2=0;
    for(int k=0;k<3;++k) {
        ga[k]+=aP[k];gb[k]+=bP[k];
        centre[k]=.5f*(aGrip[k]+bGrip[k]);
        mid[k]=.5f*(ga[k]+gb[k]);
        local[k]=bGrip[k]-aGrip[k];world[k]=gb[k]-ga[k];
        if(!std::isfinite(centre[k])||!std::isfinite(mid[k]))return false;
        local2+=local[k]*local[k];world2+=world[k]*world[k];
    }
    if(!std::isfinite(local2)||!std::isfinite(world2))return false;
    // Coincident/near-coincident grips cannot reliably define an axis.
    if(local2>=.0001f && world2>=.0001f) {
        Rotate(outQ,local,from);
        const float il=1/std::sqrt(local2),iw=1/std::sqrt(world2);
        dot=0;for(int k=0;k<3;++k){from[k]*=il;world[k]*=iw;dot+=from[k]*world[k];}
        float swing[4]={from[1]*world[2]-from[2]*world[1],from[2]*world[0]-from[0]*world[2],
                        from[0]*world[1]-from[1]*world[0],1+std::clamp(dot,-1.f,1.f)};
        if(dot < -.999999f) {
            // A half turn has no unique shortest axis. Choose a stable axis
            // perpendicular to the grip line, using its least-aligned basis.
            int axis=0;for(int k=1;k<3;++k)if(std::abs(from[k])<std::abs(from[axis]))axis=k;
            const float basis[3]={axis==0?1.f:0.f,axis==1?1.f:0.f,axis==2?1.f:0.f};
            swing[0]=from[1]*basis[2]-from[2]*basis[1];
            swing[1]=from[2]*basis[0]-from[0]*basis[2];
            swing[2]=from[0]*basis[1]-from[1]*basis[0];swing[3]=0;
        }
        if(!Normalize(swing))return false;
        Multiply(swing,outQ,outQ);
        if(!Normalize(outQ))return false;
    }
    float offset[3];Rotate(outQ,centre,offset);
    for(int k=0;k<3;++k)outP[k]=mid[k]-offset[k];
    return true;
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
