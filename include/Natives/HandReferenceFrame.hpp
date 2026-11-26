#pragma once
#include <cmath>
#include <cstdint>

namespace cvr::handspace {
struct Vector { float x{},y{},z{}; };
struct Rotation { float x{},y{},z{},w{1}; };
inline Vector operator+(Vector a,Vector b){return {a.x+b.x,a.y+b.y,a.z+b.z};}
inline Vector operator-(Vector a,Vector b){return {a.x-b.x,a.y-b.y,a.z-b.z};}
inline Vector operator*(Vector a,float s){return {a.x*s,a.y*s,a.z*s};}
inline float LengthSquared(Vector a){return a.x*a.x+a.y*a.y+a.z*a.z;}
inline bool Finite(Vector a){return std::isfinite(a.x)&&std::isfinite(a.y)&&std::isfinite(a.z);}
inline Rotation Conjugate(Rotation q){return {-q.x,-q.y,-q.z,q.w};}
inline Rotation Multiply(Rotation a,Rotation b){
    return {a.w*b.x+a.x*b.w+a.y*b.z-a.z*b.y,
            a.w*b.y-a.x*b.z+a.y*b.w+a.z*b.x,
            a.w*b.z+a.x*b.y-a.y*b.x+a.z*b.w,
            a.w*b.w-a.x*b.x-a.y*b.y-a.z*b.z};
}
inline bool Normalize(Rotation& q){
    const float n=q.x*q.x+q.y*q.y+q.z*q.z+q.w*q.w;
    if(!std::isfinite(n)||n<1e-8f)return false;
    const float s=1/std::sqrt(n);q.x*=s;q.y*=s;q.z*=s;q.w*=s;return true;
}
inline Vector Rotate(Rotation q,Vector p){
    const Vector t{2*(q.y*p.z-q.z*p.y),2*(q.z*p.x-q.x*p.z),2*(q.x*p.y-q.y*p.x)};
    return {p.x+q.w*t.x+q.y*t.z-q.z*t.y,
            p.y+q.w*t.y+q.z*t.x-q.x*t.z,
            p.z+q.w*t.z+q.x*t.y-q.y*t.x};
}
struct Reference {
    Vector position{};
    Rotation rotation{};
    uintptr_t owner{};
    uint64_t origin{};
    bool valid{};
};
inline Reference Make(Vector p,Rotation q,uintptr_t owner,uint64_t origin){
    const bool valid=owner&&Finite(p)&&Normalize(q);
    return {p,q,owner,origin,valid};
}
inline Vector Point(const Reference& frame,Vector local){return frame.position+Rotate(frame.rotation,local);}

// Remove the same sample's HMD orientation and positional contribution from
// the rendered view. Animated torso yaw is merely the model-space basis; it
// must never turn a physics hand when the tracked controller did not turn.
inline Reference FromView(const Reference& entity,Vector viewModel,Rotation viewModelRotation,
                          Vector headDeltaModel,Rotation mappedHead){
    if(!entity.valid||!Finite(headDeltaModel)||!Normalize(viewModelRotation)||!Normalize(mappedHead))return {};
    return Make(Point(entity,viewModel-headDeltaModel),
                Multiply(Multiply(entity.rotation,viewModelRotation),Conjugate(mappedHead)),
                entity.owner,entity.origin);
}
struct Motion {
    Vector from{},to{};
    Rotation turn{};
    bool valid{};
    Vector Point(Vector p) const{return valid?to+Rotate(turn,p-from):p;}
    Vector Direction(Vector v) const{return valid?Rotate(turn,v):v;}
    Rotation Orientation(Rotation q) const{return valid?Multiply(turn,q):q;}
};
inline Motion Between(const Reference& before,const Reference& after,float maxTravel=1.f){
    if(!before.valid||!after.valid||before.owner!=after.owner||before.origin!=after.origin||
       !Finite(before.position)||!Finite(after.position)||!std::isfinite(maxTravel)||maxTravel<=0||
       LengthSquared(after.position-before.position)>maxTravel*maxTravel)return {};
    auto turn=Multiply(after.rotation,Conjugate(before.rotation));
    if(!Normalize(turn))return {};
    return {before.position,after.position,turn,true};
}
}
