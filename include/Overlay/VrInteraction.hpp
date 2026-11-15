#pragma once
#include "Runtimes/HudFollow.hpp"
#include "Utils/XrMath.hpp"
#include <algorithm>
#include <cmath>

namespace cvr::vrui {
constexpr unsigned CanvasWidth=1440, CanvasHeight=1080;
struct DesktopRect {float x{},y{},scale=1;};
inline DesktopRect DesktopPlacement(float w,float h){
    const float s=std::max(.01f,std::min(w/CanvasWidth,h/CanvasHeight)*.92f);
    return {(w-CanvasWidth*s)*.5f,(h-CanvasHeight*s)*.5f,s};
}
struct Settings {
    float distance=1.4f,minDistance=.6f,maxDistance=3.0f,width=1.55f;
    float cone=60,scale=1,fontScale=1,offsetX=0,offsetY=0,depthSpeed=.75f;
    int hand=1; // 0 left, 1 right, 2 automatic
    bool follow=true;
};
inline Settings Sanitize(Settings s) {
    auto clamp=[](float v,float lo,float hi,float def){return std::isfinite(v)?std::clamp(v,lo,hi):def;};
    s.minDistance=clamp(s.minDistance,.35f,2,.6f);s.maxDistance=clamp(s.maxDistance,s.minDistance+.1f,5,3);
    s.distance=clamp(s.distance,s.minDistance,s.maxDistance,1.4f);
    s.width=clamp(s.width,.6f,2.4f,1.55f);s.scale=clamp(s.scale,.6f,1.5f,1);
    s.fontScale=clamp(s.fontScale,.8f,1.4f,1);s.cone=clamp(s.cone,5,90,60);
    s.offsetX=clamp(s.offsetX,-1.5f,1.5f,0);s.offsetY=clamp(s.offsetY,-1,1,0);
    s.depthSpeed=clamp(s.depthSpeed,.1f,2,.75f);s.hand=std::clamp(s.hand,0,2);return s;
}
struct Hand {XrPosef aim{{0,0,0,1},{}};bool valid=false,stickClick=false;float trigger=0,grip=0,stickY=0,stickX=0;};
struct Tracking {XrPosef head{{0,0,0,1},{}};Hand hands[2];bool valid=false;uint64_t origin=0;int64_t time=0;bool cyberfinger=false,preferLeft=false,menuLeft=false,menuRight=false,menuButton=false;};
inline XrVector3f Add(XrVector3f a,XrVector3f b){return {a.x+b.x,a.y+b.y,a.z+b.z};}
inline XrVector3f Sub(XrVector3f a,XrVector3f b){return {a.x-b.x,a.y-b.y,a.z-b.z};}
inline XrVector3f Mul(XrVector3f a,float s){return {a.x*s,a.y*s,a.z*s};}
inline float Dot(XrVector3f a,XrVector3f b){return a.x*b.x+a.y*b.y+a.z*b.z;}
struct Hit {bool valid=false;float x{},y{},distance{};XrVector3f point{};};
inline Hit Intersect(const XrPosef& ray,const XrPosef& panel,float width,float height) {
    const auto inv=ConjugateQuat(panel.orientation);
    const auto origin=RotateVector(inv,Sub(ray.position,panel.position));
    const auto direction=RotateVector(inv,RotateVector(ray.orientation,{0,0,-1}));
    if(!std::isfinite(direction.z) || direction.z>=-.0001f || origin.z<=0)return {};
    const float t=-origin.z/direction.z;
    if(!std::isfinite(t) || t<=0 || t>10 || width<=0 || height<=0)return {};
    const auto p=Add(origin,Mul(direction,t));
    if(std::abs(p.x)>width*.5f || std::abs(p.y)>height*.5f)return {};
    return {true,(p.x/width+.5f)*CanvasWidth,(.5f-p.y/height)*CanvasHeight,t,Add(ray.position,Mul(RotateVector(ray.orientation,{0,0,-1}),t))};
}
// Both sticks clicked and held: fires once per hold, after `seconds`.
struct HoldChord {
    static constexpr double seconds=.5;
    double held=0;bool latched=false;
    bool Update(bool both,float dt) {
        if(!both){held=0;latched=false;return false;}
        if(latched)return false;
        if(std::isfinite(dt) && dt>0 && dt<=.25f)held+=dt;else held=0;
        if(held>=seconds){latched=true;return true;}return false;
    }
};
// Pure pose policy used by the XR worker and the interaction tests.
//
// THE GRIP CARRIES THE PANEL ON THE CONTROLLER. It used to follow only the hand's TRANSLATION, one to
// one: at two metres a 25 cm reach moves the panel a few degrees and turning the wrist does nothing, so
// a grab barely nudged it along its radius. Now the panel's centre is held in the controller's own frame
// at the moment of the grab and carried rigidly -- turning the wrist swings it around on the full
// lever, moving the hand moves it, the stick still pushes it away and pulls it in. Whatever the drag
// produces is stored as before (distance and offsets in the follow heading's frame, clamped), and the
// panel stays upright, turned to face the head.
struct Placement {
    cvr::hud::Follow follow;XrPosef pose{{0,0,0,1},{}};
    uint64_t origin=0;bool valid=false;int dragging=-1;
    XrQuaternionf frame{0,0,0,1};   // the follow heading the distance and offsets are measured in
    XrVector3f grabLocal{};         // the panel's centre in the dragging controller's own frame
    void Reset(){follow.Reset();valid=false;dragging=-1;}
    void Face(const XrVector3f& head) {
        const float dx=pose.position.x-head.x,dz=pose.position.z-head.z;
        if(dx*dx+dz*dz<1e-4f){pose.orientation=frame;return;}
        const float yaw=std::atan2(-dx,-dz);
        pose.orientation={0,std::sin(yaw*.5f),0,std::cos(yaw*.5f)};
    }
    void Hold(const Hand& hand){grabLocal=RotateVector(ConjugateQuat(hand.aim.orientation),Sub(pose.position,hand.aim.position));}
    void Update(const Tracking& t,Settings& s,float dt) {
        if(!t.valid){dragging=-1;follow.ResetDelay();return;}
        if(origin!=t.origin){Reset();origin=t.origin;}
        const auto q=t.head.orientation;
        const float head=cvr::hud::HeadYaw(q.x,q.y,q.z,q.w,follow.yaw);
        if(dragging>=0 && (!t.hands[dragging].valid || t.hands[dragging].grip<.5f))dragging=-1;
        if(dragging<0) {
            if(!s.follow){follow.yaw=head;follow.valid=true;follow.following=false;follow.ResetDelay();}
            const float yaw=s.follow?follow.Update(head,s.cone*.01745329252f,dt,true):head;
            frame={0,std::sin(yaw*.5f),0,std::cos(yaw*.5f)};
        } else {
            const auto& hand=t.hands[dragging];
            const auto target=Add(hand.aim.position,RotateVector(hand.aim.orientation,grabLocal));
            const auto local=RotateVector(ConjugateQuat(frame),Sub(target,t.head.position));
            // CF placement follows the hand in all three dimensions. It does
            // not require a grip+stick combination on that same controller.
            const float axis=!t.cyberfinger && std::abs(hand.stickY)>.2f?hand.stickY:0;
            s.distance=std::clamp(-local.z+axis*s.depthSpeed*std::clamp(dt,0.0f,.05f),s.minDistance,s.maxDistance);
            s.offsetX=std::clamp(local.x,-1.5f,1.5f);s.offsetY=std::clamp(local.y,-1.0f,1.0f);
            follow.ResetDelay();
        }
        pose.position=Add(t.head.position,RotateVector(frame,{s.offsetX,s.offsetY,-s.distance}));
        Face(t.head.position);
        // Re-held on the clamped result, so a bound or the stick is not undone by the next frame.
        if(dragging>=0)Hold(t.hands[dragging]);
        valid=true;
    }
    void Grab(int hand,const Tracking& t){dragging=hand;Hold(t.hands[hand]);follow.ResetDelay();}
};
}
