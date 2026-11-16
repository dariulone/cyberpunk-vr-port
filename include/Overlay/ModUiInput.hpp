#pragma once
#include "Overlay/ModUiApi.h"
#include "Overlay/VrOverlay.hpp"
#include "Runtimes/CyberFingerMenuPolicy.hpp"
#include <deque>

namespace cvr::modui {
// One focused surface, regardless of which scripting/native framework drew it.
// World anchored: turning the head never steers the mouse or carries the panel.
class Input {
    cvr::vrui::View view{};
    bool opened{},interacting{},armed[2]{},trigger[2]{},grip[2]{},blocked{},down{},secondary{};
    int hand{},drag=-1;uint64_t origin{};int64_t stamp{};
    XrVector3f grabLocal{};
    cvr::cyberfinger::NativeMenuButtons menus;
    cvr::cyberfinger::AxisStep tabs;
    std::deque<CvrModUiInput> queue;
    void Push(CvrModUiInput e) {
        // Motion can be coalesced, but press/release and Back/tab edges cannot.
        if(queue.size()>=2 && !e.flags && !queue.back().flags &&
           e.buttons==queue.back().buttons && e.buttons==queue[queue.size()-2].buttons && !e.tab && !queue.back().tab) {
            e.wheel=std::clamp(e.wheel+queue.back().wheel,-.25f,.25f);queue.back()=e;return;
        }
        if(queue.size()>=64){queue.clear();blocked=true;down=secondary=false;for(auto& a:armed)a=false;e={-1,-1,0,0,0,0};}
        queue.push_back(e);
    }
public:
    const cvr::vrui::View& View()const{return view;}
    void Reset(){*this={};queue.push_back({-1,-1,0,0,0,0});}
    uint32_t Poll(CvrModUiInput* out,uint32_t cap) {
        uint32_t n=0;while(n<cap && !queue.empty()){out[n++]=queue.front();queue.pop_front();}return n;
    }
    void Step(const cvr::vrui::Tracking& t,bool visible,bool interactive,uint64_t now,const cvr::vrui::Settings& settings = {}) {
        using namespace cvr::vrui;
        if(!visible){Reset();return;}
        if(!t.valid){
            for(int h=0;h<2;++h)armed[h]=trigger[h]=grip[h]=false;
            menus={};tabs={};drag=-1;down=secondary=false;stamp=0;view.valid=view.ray=false;
            queue.clear();Push({-1,-1,0,0,0,0});return;
        }
        const float dt=stamp && t.time>stamp?std::clamp(float(t.time-stamp)*1e-9f,0.f,.05f):0.f;stamp=t.time;
        if(!opened || origin!=t.origin) {
            Reset();opened=true;origin=t.origin;hand=t.preferLeft?0:1;stamp=t.time;
            view.pose=OpeningPose(t.head,settings);
        }
        const auto size=PanelBounds(float(CanvasWidth)/CanvasHeight,settings);
        view.valid=true;view.width=size.width;view.height=size.height;view.headPosition=t.head.position;
        view.cyberfinger=t.cyberfinger;view.ray=false;view.hit={};view.dragging=false;
        if(!interactive){interacting=false;for(auto& a:armed)a=false;menus={};tabs={};drag=-1;down=secondary=false;Push({-1,-1,0,0,0,0});return;}
        // Passive mod HUDs can outlive the interactive overlay. Opening it
        // restores the configured hand without moving its world anchor.
        if(!interacting){hand=t.preferLeft?0:1;interacting=true;}
        bool edges[2]{},gripEdges[2]{};
        for(int h=0;h<2;++h) {
            const auto& p=t.hands[h];
            if(!p.valid || !std::isfinite(p.trigger) || !std::isfinite(p.grip)){armed[h]=trigger[h]=grip[h]=false;continue;}
            if(p.trigger<.3f && p.grip<.3f)armed[h]=true;
            const bool pressed=p.trigger>(trigger[h]?.35f:.65f),g=p.grip>.5f;
            edges[h]=armed[h] && pressed && !trigger[h];gripEdges[h]=armed[h] && g && !grip[h];trigger[h]=pressed;grip[h]=g;
        }
        if(blocked){if(!trigger[0] && !trigger[1] && !grip[0] && !grip[1])blocked=false;else {Push({-1,-1,0,0,0,0});return;}}
        auto hit=[&](int h){return t.hands[h].valid?Intersect(t.hands[h].aim,view.pose,view.width,view.height):Hit{};};
        // CF optical dropouts must not silently change handedness. Other
        // profiles retain their pose-loss fallback. Never transfer a held click.
        if(drag<0 && !trigger[hand] && !grip[hand] && hit(1-hand).valid &&
           (edges[1-hand] || (!t.cyberfinger && !t.hands[hand].valid))){
            hand=1-hand;tabs={};down=secondary=false;Push({-1,-1,0,0,0,0});
        }
        if(drag>=0 && (!t.hands[drag].valid || !grip[drag]))drag=-1;
        if(drag>=0){view.pose.position=Add(t.hands[drag].aim.position,RotateVector(t.hands[drag].aim.orientation,grabLocal));}
        view.hit=hit(hand);
        // Reserved top strip moves the complete canvas; Grip elsewhere is a
        // secondary click. Neither action requires a same-hand stick chord.
        if(drag<0 && gripEdges[hand] && view.hit.valid && view.hit.y<32 && !trigger[hand]){
            drag=hand;grabLocal=RotateVector(ConjugateQuat(t.hands[hand].aim.orientation),Sub(view.pose.position,t.hands[hand].aim.position));
        }
        view.dragging=drag>=0;
        if(!view.hit.valid || drag>=0 || !trigger[hand])down=false;
        if(!view.hit.valid || drag>=0 || !grip[hand])secondary=false;
        if(view.hit.valid && drag<0){
            if(edges[hand])down=true;
            if(gripEdges[hand] && !trigger[hand] && view.hit.y>=32)secondary=true;
        }
        const auto actions=menus.Step(t.menuLeft,t.menuRight,false,false);
        const int step=tabs.Step(t.hands[1-hand].stickX,0,now,false);
        CvrModUiInput e{-1,-1,0,0,(actions&0x2000)?CVR_MOD_UI_BACK:0u,step==3?-1:step==4?1:0};
        if(view.hit.valid && drag<0){
            e.x=view.hit.x;e.y=view.hit.y;
            if(down)e.buttons|=CVR_MOD_UI_PRIMARY;
            if(secondary && !down)e.buttons|=CVR_MOD_UI_SECONDARY;
            const float axis=t.hands[hand].stickY;
            if(!e.buttons && std::isfinite(axis) && std::abs(axis)>.2f)e.wheel=axis*dt*5.f;
        }
        Push(e);
        // The laser remains visible over empty space, not only widgets.
        if(t.hands[hand].valid){
            view.ray=true;view.rayStart=t.hands[hand].aim.position;
            view.rayEnd=view.hit.valid?view.hit.point:Add(view.rayStart,RotateVector(t.hands[hand].aim.orientation,{0,0,-1.4f}));
        }
    }
};
}
