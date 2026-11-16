#pragma once
#include <algorithm>
#include <cmath>
#include <cstdint>
#include "Anim/LadderGripProfile.hpp"

namespace cvr::ladder {
inline bool GripEnabled(bool handPhysics, bool ladderGrip) { return handPhysics && ladderGrip; }
struct Vec {
    float x{},y{},z{};
    Vec operator+(Vec b) const { return {x+b.x,y+b.y,z+b.z}; }
    Vec operator-(Vec b) const { return {x-b.x,y-b.y,z-b.z}; }
    Vec operator*(float k) const { return {x*k,y*k,z*k}; }
};
inline float Dot(Vec a,Vec b) { return a.x*b.x+a.y*b.y+a.z*b.z; }
inline float Length(Vec a) { return std::sqrt(Dot(a,a)); }
inline bool Finite(Vec a) { return std::isfinite(a.x)&&std::isfinite(a.y)&&std::isfinite(a.z); }
inline Vec Cross(Vec a,Vec b) { return {a.y*b.z-a.z*b.y,a.z*b.x-a.x*b.z,a.x*b.y-a.y*b.x}; }
inline bool Normalize(Vec& a) { const float n=Length(a);if(!Finite(a)||n<.001f)return false;a=a*(1/n);return true; }
struct Rotation {
    float x{},y{},z{},w=1;
    Rotation Inverse() const { return {-x,-y,-z,w}; }
    Rotation operator*(Rotation b) const {
        return {w*b.x+x*b.w+y*b.z-z*b.y,w*b.y-x*b.z+y*b.w+z*b.x,
            w*b.z+x*b.y-y*b.x+z*b.w,w*b.w-x*b.x-y*b.y-z*b.z};
    }
    Vec Rotate(Vec v) const { const auto q=(*this)*Rotation{v.x,v.y,v.z,0}*Inverse();return {q.x,q.y,q.z}; }
    bool Normalize() {
        const float n=std::sqrt(x*x+y*y+z*z+w*w);
        if(!std::isfinite(n)||n<.001f)return false;
        x/=n;y/=n;z/=n;w/=n;return true;
    }
};
inline Rotation FromAxes(Vec x,Vec y,Vec z) {
    Rotation q;const float trace=x.x+y.y+z.z;
    if(trace>0) { const float s=2*std::sqrt(trace+1);q={(y.z-z.y)/s,(z.x-x.z)/s,(x.y-y.x)/s,s*.25f}; }
    else if(x.x>y.y && x.x>z.z) { const float s=2*std::sqrt(1+x.x-y.y-z.z);q={s*.25f,(y.x+x.y)/s,(z.x+x.z)/s,(y.z-z.y)/s}; }
    else if(y.y>z.z) { const float s=2*std::sqrt(1+y.y-x.x-z.z);q={(y.x+x.y)/s,s*.25f,(z.y+y.z)/s,(z.x-x.z)/s}; }
    else { const float s=2*std::sqrt(1+z.z-x.x-y.y);q={(z.x+x.z)/s,(z.y+y.z)/s,s*.25f,(x.y-y.x)/s}; }
    q.Normalize();return q;
}
inline Vec PalmOffset(int side) { const auto* p=profile::hands[side].contact;return {p[0],p[1],p[2]}; }
inline bool OnLadder(int detailed) { return detailed>=10 && detailed<=12; }
inline bool Fresh(uintptr_t owner,uintptr_t current,uint64_t stamp,uint64_t now,int detailed) {
    return owner && owner==current && stamp && now>=stamp && now-stamp<=250000 && OnLadder(detailed);
}
struct Geometry {
    struct Segment { Vec a,b; };
    Vec position{},normal{},up{},right{};
    float height{},topStep{},halfWidth=.30f,rungStep=.40f,firstRung=.40f;
    float measuredTop{};   // the top of the ladder's highest module, measured from its meshes (0 = not known)
    Segment topRails[10]{};
    int topRailCount{};
    uint64_t identity{};
    bool valid{};
    bool Prepare() {
        valid=false;
        if(!Finite(position)||!Normalize(up)||!Normalize(normal)||!std::isfinite(height)||height<.5f||height>100)return false;
        normal=normal-up*Dot(normal,up);if(!Normalize(normal))return false;
        right=Cross(up,normal);if(!Normalize(right))return false;
        valid=halfWidth>.1f && halfWidth<1 && rungStep>.1f && rungStep<1 && std::isfinite(firstRung) &&
            std::isfinite(topStep) && std::abs(topStep)<=1 && identity!=0;return valid;
    }
    // The state-machine exit plane is one step below the physical top -- on the ladders it was measured on. The
    // description does not always say where the ladder ends: a 10 m ladder of two 4 m modules and a 2 m one under a
    // finisher rail says 9.1 and -0.3, and its two last rungs (9.6, 10.0) and the rail could not be gripped
    // (2026-11-05: "on a long ladder, at the end, it does not take the last rung"). Its modules say it: their rungs stand
    // every 0.4 m from the foot up to each module's top (generic_ladder_a/b/c, every height), so the top of the highest
    // module (SetMeasuredTop) is the last rung's height when it is known.
    float PhysicalTop() const { return measuredTop>0 ? measuredTop:height-std::min(topStep,0.0f); }
    bool SetMeasuredTop(float top) {
        if(!valid || !std::isfinite(top) || top<height-1 || top>height+3 || top>100)return false;
        measuredTop=top;return true;
    }
    bool SetTopRails(Vec origin,Vec x,Vec y,Vec z) {
        if(!valid || !Finite(origin) || !Normalize(x) || !Normalize(y) || !Normalize(z) ||
            Dot(z,up)<.98f || std::abs(Dot(x,right))<.98f || std::abs(Dot(y,normal))<.98f)return false;
        const Vec delta=origin-position;
        if(std::abs(Dot(delta,up)-PhysicalTop())>.25f ||
            std::abs(Dot(delta,right))>.15f || std::abs(Dot(delta,normal))>.4f)return false;
        // generic_ladder_f_finisher_rail: two U-shaped rails. Centre lines
        // measured from its mesh; transform comes from this ladder component.
        topRailCount=0;
        auto point=[&](Vec p) { return origin+x*p.x+y*p.y+z*p.z; };
        auto segment=[&](Vec a,Vec b) { topRails[topRailCount++]={point(a),point(b)}; };
        for(float side:{-1.0f,1.0f}) {
            const float outer=side*.4023f,inner=side*.287f;
            segment({inner,.1235f,.013f},{outer,.1235f,.08f});
            segment({outer,.1235f,.08f},{outer,.1235f,1.035f});
            segment({outer,.1235f,1.035f},{outer,-.1579f,1.035f});
            segment({outer,-.1579f,1.035f},{outer,-.1579f,.08f});
            segment({outer,-.1579f,.08f},{inner,-.1579f,.013f});
        }
        return true;
    }
};
struct Contact { Vec point{};float distance{};int kind{};Vec axis{}; }; // 1 rail, 2 rung, 3 top rail
inline Contact Closest(const Geometry& g,Vec world) {
    if(!g.valid||!Finite(world))return {};
    const Vec d=world-g.position;
    const float x=Dot(d,g.right),z=Dot(d,g.up);
    const float rail=x<0 ? -g.halfWidth:g.halfWidth;
    const Vec railPoint=g.position+g.right*rail+g.up*std::clamp(z,0.0f,g.PhysicalTop())+g.normal*.025f;
    const float rung=g.firstRung+std::round((z-g.firstRung)/g.rungStep)*g.rungStep;
    Contact result{railPoint,Length(world-railPoint),1,g.up};
    if(rung>=0 && rung<=g.PhysicalTop()+.001f) {
        const Vec point=g.position+g.right*std::clamp(x,-g.halfWidth,g.halfWidth)+g.up*rung+g.normal*.025f;
        const float distance=Length(world-point);
        if(distance<result.distance)result={point,distance,2,g.right};
    }
    for(int i=0;i<g.topRailCount;++i) {
        const auto& line=g.topRails[i];const Vec span=line.b-line.a;
        const Vec point=line.a+span*std::clamp(Dot(world-line.a,span)/std::max(Dot(span,span),1e-8f),0.0f,1.0f)+g.normal*.025f;
        const float distance=Length(world-point);
        if(distance<result.distance) { Vec axis=span;Normalize(axis);if(Dot(axis,g.up)<0)axis=axis*-1;result={point,distance,3,axis}; }
    }
    return result;
}
inline Rotation GripRotation(const Geometry& g,const Contact& contact,int side) {
    Vec up=contact.axis;
    // Opposite thumbs point toward the centre on a horizontal rung.
    if(contact.kind==2 && side==1)up=up*-1;
    Vec forward=g.normal*-1;
    forward=forward-up*Dot(forward,up);
    if(!Normalize(forward)) { forward=g.up*-1;forward=forward-up*Dot(forward,up);Normalize(forward); }
    Vec right=Cross(forward,up);Normalize(right);forward=Cross(up,right);Normalize(forward);
    const auto* q=profile::hands[side].rotation;
    return FromAxes(right,forward,up)*Rotation{q[0],q[1],q[2],q[3]};
}
struct Frame {
    Vec tracking[2]{},world[2]{},trackingUp{0,0,1};
    float grip[2]{},height{};
    float finishDistance=1.0f;
    bool autoFinish=true;
    bool handValid[2]{},valid{};
    uint64_t sequence{},origin{},stamp{};
};
class Climber {
public:
    void Reset() { *this={}; }
    bool Held(int side) const { return side>=0 && side<2 && m_held[side]; }
    Vec Anchor(int side) const { return m_anchor[side]; }
    int Kind(int side) const { return m_kind[side]; }
    float Debt() const { return m_debt; }
    bool Finishing() const { return m_finishing; }
    bool Constrain(int side,Vec bodyPosition,Rotation bodyRotation,Vec& target,Rotation& handRotation) {
        if(!Held(side)||!Finite(bodyPosition)||!bodyRotation.Normalize()||!handRotation.Normalize())return false;
        const auto inverse=bodyRotation.Inverse();
        // The rail touches the palm, not the wrist/forearm joint targeted by IK.
        target=inverse.Rotate(m_anchor[side]-m_rotation[side].Rotate(PalmOffset(side))-bodyPosition);
        handRotation=inverse*m_rotation[side];return true;
    }
    float Update(const Geometry& g,const Frame& f,uint64_t now,bool allowed) {
        const bool valid=allowed && g.valid && f.valid && f.sequence && f.origin && f.stamp && now>=f.stamp &&
            now-f.stamp<=250000 && std::isfinite(f.height) && Finite(f.trackingUp);
        if(!valid) { Release(f);return 0; }
        if(m_origin!=f.origin || m_geometry!=g.identity || (m_stamp && (f.stamp<m_stamp || f.stamp-m_stamp>150000))) {
            Release(f);m_origin=f.origin;m_geometry=g.identity;
        }
        const bool fresh=f.sequence!=m_sequence;
        const float dt=m_stamp && f.stamp>m_stamp ? float(f.stamp-m_stamp)*1e-6f:.02f;
        const int previousHands=int(m_held[0])+int(m_held[1]);
        float pull=0;int movingHands=0;
        for(int side=0;side<2;++side) {
            const bool wasDown=m_down[side],wasHeld=m_held[side];
            if(f.grip[side]<=.35f)m_down[side]=false;
            else if(f.grip[side]>=.65f)m_down[side]=true;
            if(!m_down[side]) { m_held[side]=false;m_kind[side]=0; }
            if(!f.handValid[side] || !Finite(f.tracking[side]) || !Finite(f.world[side])) {
                m_held[side]=false;continue;
            }
            if(!wasDown && m_down[side]) {
                const auto contact=Closest(g,f.world[side]);
                if(contact.kind && contact.distance<=.11f) {
                    m_held[side]=true;m_anchor[side]=contact.point;m_kind[side]=contact.kind;
                    m_rotation[side]=GripRotation(g,contact,side);
                }
            }
            if(m_held[side] && Length(f.world[side]-m_anchor[side])>.65f) { m_held[side]=false;m_kind[side]=0; }
            if(fresh && wasHeld && m_held[side]) {
                const Vec delta=f.tracking[side]-m_previous[side];
                if(Length(delta)>std::max(.18f,4*dt)) { m_held[side]=false;m_kind[side]=0; }
                else { pull-=Dot(delta,f.trackingUp);++movingHands; }
            }
            if(fresh)m_previous[side]=f.tracking[side];
        }
        const int hands=int(m_held[0])+int(m_held[1]);
        const bool wasFinishing=m_finishing;
        const float signedPull=movingHands ? pull/float(movingHands):0;
        const float remaining=g.height-f.height;
        const float finishDistance=std::clamp(f.finishDistance,.2f,1.2f);
        // THE FLOOR AT THE TOP CAN LIE ABOVE THE EXIT PLANE: by -topStep (0.4 m on the
        // generic ladders with a finisher). The climb used to stop 0.35 m past the plane,
        // short of that floor, and V dropped back down the ladder on exactly those ladders
        // (2026-11-04: "on some ladders the character falls at the end").
        const float floorAbove=std::max(0.0f,-std::min(g.topStep,0.0f));
        if(!f.autoFinish || remaining>finishDistance || remaining<-(floorAbove+.35f) ||
            (!f.handValid[0] && !f.handValid[1]) || signedPull<-.008f) {
            m_finishing=false;m_upIntent=0;
        } else if(!m_finishing) {
            m_upIntent=std::clamp(m_upIntent+signedPull,0.0f,.08f);
            if(hands && m_upIntent>=.05f) {
                m_finishing=true;m_finishStart=m_progressStamp=now;m_bestHeight=f.height;
            }
        }
        if(m_finishing) {
            if(f.height>m_bestHeight+.01f) { m_bestHeight=f.height;m_progressStamp=now; }
            // Over the top the native exit carries V forward before it lifts it again, so a
            // pause in height is no stall until it has lasted a while.
            if(now-m_finishStart>4000000 || now-m_progressStamp>1200000) {
                m_finishing=false;m_upIntent=0;
            }
        }
        if(wasFinishing || m_finishing) { m_target=f.height;m_debt=0; }
        if(!hands) { m_target=f.height;m_debt=0; }
        else {
            if(!previousHands)m_target=f.height;
            if(movingHands)m_target+=pull/float(movingHands);
            m_target=std::clamp(m_target,f.height-.30f,f.height+.30f);
            m_debt=m_target-f.height;
        }
        m_sequence=f.sequence;if(fresh)m_stamp=f.stamp;
        // Finish only after an intentional upward pull near the top. The
        // native ladder controller performs the exit; manual input cancels it.
        if(m_finishing)return .65f;
        if(!hands || std::abs(m_debt)<.002f)return 0;
        return std::clamp(m_debt/std::max(.12f,3.0f*std::min(dt,.15f)),-.75f,.75f);
    }
private:
    void Release(const Frame& f) {
        Reset();m_target=f.height;
        for(int i=0;i<2;++i) { m_down[i]=f.grip[i]>.35f;m_previous[i]=f.tracking[i]; }
    }
    bool m_down[2]{},m_held[2]{};
    Rotation m_rotation[2]{};
    int m_kind[2]{};
    Vec m_previous[2]{},m_anchor[2]{};
    float m_target{},m_debt{};
    bool m_finishing{};
    float m_upIntent{},m_bestHeight{};
    uint64_t m_finishStart{},m_progressStamp{};
    uint64_t m_sequence{},m_stamp{},m_origin{},m_geometry{};
};
}
