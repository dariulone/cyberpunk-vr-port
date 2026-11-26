#pragma once

#include <algorithm>
#include <cmath>

namespace cvr::anim::jointlimits {
constexpr float Pi=3.14159265358979323846f, Deg=Pi/180.f;
inline float Dot(const float* a,const float* b){return a[0]*b[0]+a[1]*b[1]+a[2]*b[2];}
inline void Cross(const float* a,const float* b,float* out){
    const float v[3]{a[1]*b[2]-a[2]*b[1],a[2]*b[0]-a[0]*b[2],a[0]*b[1]-a[1]*b[0]};std::copy_n(v,3,out);
}
inline bool Unit(float* v){const float n=std::sqrt(Dot(v,v));if(!(n>1e-7f)||!std::isfinite(n))return false;for(int k=0;k<3;++k)v[k]/=n;return true;}
inline bool Normalize(float* q){
    const float n=std::sqrt(q[0]*q[0]+q[1]*q[1]+q[2]*q[2]+q[3]*q[3]);
    if(!(n>1e-7f)||!std::isfinite(n))return false;for(int k=0;k<4;++k)q[k]/=n;return true;
}
inline void Conjugate(const float* q,float* out){out[0]=-q[0];out[1]=-q[1];out[2]=-q[2];out[3]=q[3];}
inline void Multiply(const float* a,const float* b,float* out){
    const float q[4]{a[3]*b[0]+a[0]*b[3]+a[1]*b[2]-a[2]*b[1],a[3]*b[1]-a[0]*b[2]+a[1]*b[3]+a[2]*b[0],
        a[3]*b[2]+a[0]*b[1]-a[1]*b[0]+a[2]*b[3],a[3]*b[3]-a[0]*b[0]-a[1]*b[1]-a[2]*b[2]};std::copy_n(q,4,out);
}
inline void Rotate(const float* q,const float* v,float* out){
    float t[3],u[3];Cross(q,v,t);Cross(q,t,u);
    for(int k=0;k<3;++k)out[k]=v[k]+2*(q[3]*t[k]+u[k]);
}
inline void AxisAngle(const float* axis,float angle,float* out){const float s=std::sin(angle*.5f);for(int k=0;k<3;++k)out[k]=axis[k]*s;out[3]=std::cos(angle*.5f);}
inline bool Arc(const float* from,const float* to,float* out){
    float a[3],b[3];std::copy_n(from,3,a);std::copy_n(to,3,b);if(!Unit(a)||!Unit(b))return false;
    const float dot=std::clamp(Dot(a,b),-1.f,1.f);
    if(dot<-.99999f){
        int least=0;for(int k=1;k<3;++k)if(std::abs(a[k])<std::abs(a[least]))least=k;
        float basis[3]{};basis[least]=1;Cross(a,basis,out);if(!Unit(out))return false;out[3]=0;return true;
    }
    Cross(a,b,out);out[3]=1+dot;return Normalize(out);
}
inline void Slerp(const float* a,const float* b,float w,float* out){
    float dot=0;for(int k=0;k<4;++k)dot+=a[k]*b[k];const float sign=dot<0?-1.f:1.f;dot=std::clamp(std::abs(dot),0.f,1.f);
    w=std::clamp(w,0.f,1.f);float x=1-w,y=w;
    if(dot<.9995f){const float angle=std::acos(dot),s=std::sin(angle);x=std::sin((1-w)*angle)/s;y=std::sin(w*angle)/s;}
    for(int k=0;k<4;++k)out[k]=a[k]*x+b[k]*sign*y;Normalize(out);
}
inline float Twist(const float* q,const float* axis){
    const float sign=q[3]<0?-1.f:1.f;return 2*std::atan2(Dot(q,axis)*sign,std::abs(q[3]));
}
// Follow a requested rotation across the quaternion +/-180-degree seam. The
// limited joint must keep its current stop instead of jumping to the other one.
struct TwistHistory {
    float raw{},angle{};bool valid{};
    void Observe(float value){angle=valid?angle+std::remainder(value-raw,2*Pi):value;raw=value;valid=true;}
};
inline bool ReferenceTwist(const float* reference,const float* desired,const float* boneAxis,float& angle){
    float axis[3],inverse[4],relative[4];std::copy_n(boneAxis,3,axis);if(!Unit(axis))return false;
    Conjugate(reference,inverse);Multiply(inverse,desired,relative);if(!Normalize(relative))return false;
    angle=Twist(relative,axis);return std::isfinite(angle);
}
struct Cone {float y,z,twist;};
// Functional limits for an actively held biped. Ragdoll shape-joint frames are
// not animation bone frames: their serialized angles must not be copied here.
inline constexpr Cone Shoulder{110*Deg,130*Deg,100*Deg}, Hip{65*Deg,100*Deg,45*Deg};
inline constexpr Cone Wrist{60*Deg,30*Deg,80*Deg}, Ankle{25*Deg,45*Deg,20*Deg};
// Clamp the rotation relative to the rig's reference local frame. Twist is
// about the bone, swing uses the orthogonal rig axes; no world-axis dependence.
inline bool Clamp(const float* reference,const float* desired,const float* boneAxis,Cone limit,float* out,const float* twistReference=nullptr){
    float inv[4],q[4],axis[3];std::copy_n(boneAxis,3,axis);if(!Unit(axis))return false;
    Conjugate(reference,inv);Multiply(inv,desired,q);if(!Normalize(q))return false;
    if(q[3]<0)for(float& v:q)v=-v;
    float z[3]{0,0,1};const float parallel=Dot(z,axis);for(int k=0;k<3;++k)z[k]-=parallel*axis[k];if(!Unit(z))return false;
    float y[3];Cross(z,axis,y);
    float twist[4],swing[4];const float angle=Twist(q,axis);
    AxisAngle(axis,angle,twist);Conjugate(twist,inv);Multiply(q,inv,swing);Normalize(swing);
    if(swing[3]<0)for(float& v:swing)v=-v;
    const float length=std::sqrt(Dot(swing,swing));
    if(length>1e-7f){
        const float turn=2*std::atan2(length,std::max(0.f,swing[3]));
        const float sy=Dot(swing,y)/length*turn,sz=Dot(swing,z)/length*turn;
        const float over=std::hypot(sy/limit.y,sz/limit.z);
        float swingAxis[3];for(int k=0;k<3;++k)swingAxis[k]=swing[k]/length;
        AxisAngle(swingAxis,turn/std::max(1.f,over),swing);
    }
    const float continuous=twistReference?*twistReference+std::remainder(angle-*twistReference,2*Pi):angle;
    AxisAngle(axis,std::clamp(continuous,-limit.twist,limit.twist),twist);
    Multiply(swing,twist,q);Multiply(reference,q,out);return Normalize(out);
}
struct Hinge {
    float rest[4]{0,0,0,1},axis[3]{0,0,1},incoming[3]{1,0,0},outgoing[3]{1,0,0},restAngle{};
    float sign=-1,minimum=2*Deg,maximum=145*Deg,twist=80*Deg;
};
inline bool Configure(Hinge& h,const float* reference,const float* incoming,const float* outgoing,bool leg){
    std::copy_n(reference,4,h.rest);if(!Normalize(h.rest))return false;
    std::copy_n(outgoing,3,h.outgoing);if(!Unit(h.outgoing))return false;
    const float z[3]{0,0,1};Rotate(h.rest,z,h.axis);
    float a[3],b[3],cross[3];std::copy_n(incoming,3,a);if(!Unit(a))return false;
    std::copy_n(a,3,h.incoming);
    Rotate(h.rest,h.outgoing,b);Cross(a,b,cross);
    // Humanoid rig hinge axes are Z (also authored by its foot IK setups).
    // Unknown axes are rejected instead of bending a different skeleton wrong.
    // NPC metarigs commonly use the authored A-pose (about 40 degrees of
    // elbow flexion), while the player/reference rig may be almost straight.
    // Keep that signed reference angle; only a degenerate fold is ambiguous.
    if(std::abs(Dot(a,h.axis))>.05f||std::abs(Dot(b,h.axis))>.05f||Dot(a,b)<-.985f)return false;
    h.restAngle=std::atan2(Dot(cross,h.axis),Dot(a,b));h.sign=leg?1.f:-1.f;
    h.maximum=(leg?150:145)*Deg;h.twist=(leg?3:80)*Deg;return true;
}
inline void Pose(const Hinge& h,float flex,const float* animated,float* out,const float* twistReference=nullptr){
    float inv[4],relative[4],bend[4],twist[4],base[4];Conjugate(h.rest,inv);Multiply(inv,animated,relative);
    const float raw=Twist(relative,h.outgoing);
    const float continuous=twistReference?*twistReference+std::remainder(raw-*twistReference,2*Pi):raw;
    const float pronation=std::clamp(continuous,-h.twist,h.twist);
    AxisAngle(h.axis,h.sign*std::clamp(flex,h.minimum,h.maximum)-h.restAngle,bend);Multiply(bend,h.rest,base);
    AxisAngle(h.outgoing,pronation,twist);Multiply(base,twist,out);Normalize(out);
}
} // namespace cvr::anim::jointlimits
