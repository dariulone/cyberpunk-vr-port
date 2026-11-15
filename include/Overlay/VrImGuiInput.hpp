#pragma once
#include "Overlay/VrOverlay.hpp"
#include <imgui.h>
#include <imgui_internal.h>
#include <span>
#include <cfloat>
namespace cvr::vrui {
class ImGuiPointerFeed {
    bool down=false,secondary=false,closeRequested=false;
public:
    void Reset(){down=secondary=closeRequested=false;}
    bool ConsumeClose(){const bool value=closeRequested;closeRequested=false;return value;}
    void Release(ImGuiIO& io){
        if(down || secondary){io.AddMousePosEvent(-FLT_MAX,-FLT_MAX);io.AddMouseButtonEvent(0,false);io.AddMouseButtonEvent(1,false);}
        down=secondary=false;
    }
    void Submit(ImGuiIO& io,std::span<const PointerEvent> samples,float x,float y){
        float wheel=0;
        for(const auto& e:samples){
            if(e.back) {
                // Read before NewFrame consumes Escape. Cancel the current
                // popup/edit first; only an idle panel should close outright.
                if(GImGui && (GImGui->OpenPopupStack.Size || GImGui->ActiveId)) {
                    io.AddKeyEvent(ImGuiKey_Escape,true);io.AddKeyEvent(ImGuiKey_Escape,false);
                } else closeRequested=true;
            }
            if(e.direction) {
                const ImGuiKey key=e.direction==1?ImGuiKey_UpArrow:e.direction==2?ImGuiKey_DownArrow:
                    e.direction==3?ImGuiKey_LeftArrow:ImGuiKey_RightArrow;
                io.AddKeyEvent(key,true);io.AddKeyEvent(key,false);
            }
            if(e.activate){io.AddKeyEvent(ImGuiKey_Enter,true);io.AddKeyEvent(ImGuiKey_Enter,false);}
            // Keep click edges and their hit positions, but coalesce continuous
            // controller motion into the latest position for this UI frame.
            if(e.down!=down){
                io.AddMousePosEvent(e.x<0?-FLT_MAX:e.x,e.y<0?-FLT_MAX:e.y);
                io.AddMouseButtonEvent(0,e.down);down=e.down;
            }
            if(e.secondary!=secondary){
                io.AddMousePosEvent(e.x<0?-FLT_MAX:e.x,e.y<0?-FLT_MAX:e.y);
                io.AddMouseButtonEvent(1,e.secondary);secondary=e.secondary;
            }
            wheel+=e.wheel;
        }
        const bool focus=!samples.empty() && samples.back().focus;
        io.AddMousePosEvent(x<0 || focus?-FLT_MAX:x,y<0 || focus?-FLT_MAX:y);
        io.AddMouseButtonEvent(0,down);
        io.AddMouseButtonEvent(1,secondary);
        // ImGui1.90 trickles queued motion and wheel into separate frames. A
        // continuously moving VR ray plus queued wheel therefore grows an
        // unbounded backlog even with one wheel event per rendered frame.
        // The stick is a continuous axis: apply its accumulated frame delta to
        // MouseWheel directly. Keep normal event trickling for trigger clicks.
        if(samples.empty() || samples.back().wheel==0 || down || secondary || x<0 || y<0)wheel=0;
        io.MouseWheel+=std::clamp(wheel,-.25f,.25f); // never replay a stalled frame's entire scroll history
    }
};

// Used under the overlay's pointer mutex. Win32 messages only update this
// mailbox; the render thread chooses one source before feeding ImGui.
class ImGuiPointerRouter {
    ImGuiPointerFeed feed;
    PointerEvent mouse;
    std::vector<PointerEvent> mouseEdges;
    float wheel=0,wheelH=0;
    uint64_t mouseActivity=0;
    bool vrSeen=false,desktopClaimed=false,mouseRequest=false,desktopOwner=false;
    static bool Inside(float x,float y){return x>=0 && y>=0 && x<CanvasWidth && y<CanvasHeight;}
public:
    void Reset(ImGuiIO& io){feed.Release(io);*this=ImGuiPointerRouter{};}
    bool ConsumeClose(){return feed.ConsumeClose();}
    void MouseMove(float x,float y,uint64_t now){
        if(x!=mouse.x || y!=mouse.y)mouseActivity=now;
        mouse.x=x;mouse.y=y;
    }
    void MouseButton(float x,float y,bool down,uint64_t now,int button=0){
        MouseMove(x,y,now);
        bool& current=button==1?mouse.secondary:mouse.down;
        if(down==current)return;
        current=down;mouseActivity=now;
        if(down && Inside(x,y))mouseRequest=true;
        // Keep short desktop clicks just like short controller trigger presses.
        if(mouseEdges.size()<64)mouseEdges.push_back(mouse);
        else {mouseEdges.clear();mouse.down=mouse.secondary=false;mouseEdges.push_back({});mouseRequest=false;}
    }
    void MouseWheel(float x,float y,float vertical,float horizontal,uint64_t now){
        MouseMove(x,y,now);
        if(!Inside(x,y))return;
        wheel+=vertical;wheelH+=horizontal;mouseActivity=now;mouseRequest=true;
    }
    void MouseLeave(uint64_t now){MouseMove(-1,-1,now);}
    void MouseFocusLost(uint64_t now){
        MouseButton(-1,-1,false,now);MouseButton(-1,-1,false,now,1);mouseRequest=false;desktopClaimed=false;
    }
    void Submit(ImGuiIO& io,std::span<const PointerEvent> samples,const Hit& hit,bool dragging,uint64_t now){
        vrSeen|=hit.valid;
        bool vrAction=dragging;
        for(const auto& e:samples)vrAction|=e.down || e.secondary || e.back || e.wheel!=0 || e.direction!=0 || e.activate;
        if(vrAction)desktopClaimed=false;
        else if(mouseRequest)desktopClaimed=true;
        else if(desktopClaimed && !mouse.down && !mouse.secondary && (!Inside(mouse.x,mouse.y) || now-mouseActivity>=1800))desktopClaimed=false;
        // Once a VR ray has reached this panel, passive/repeated WM_MOUSEMOVE
        // and WM_MOUSELEAVE must not replace it, including during tracking loss.
        // A deliberate desktop click/wheel can still operate the mirror.
        const bool desktop=desktopClaimed || (!vrSeen && !vrAction);
        if(desktop!=desktopOwner){feed.Release(io);desktopOwner=desktop;}
        if(desktop){
            feed.Submit(io,mouseEdges,mouse.x,mouse.y);
            if(Inside(mouse.x,mouse.y) && !mouse.down){io.MouseWheel+=wheel;io.MouseWheelH+=wheelH;}
        }else feed.Submit(io,samples,hit.valid?hit.x:-1,hit.valid?hit.y:-1);
        mouseEdges.clear();wheel=wheelH=0;mouseRequest=false;
    }
};
}
