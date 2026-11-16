#pragma once
#include "ModUiApi.h"
#include <imgui.h>
#include <imgui_internal.h>
#include <algorithm>
#include <cfloat>
#include <windows.h>

namespace cvr::modui::client {
// Compile this adapter with the PRODUCER's ImGui. It never touches another
// renderer's context, fonts, descriptors, draw data, or backend state.
class ImGuiInput {
    float x=-1,y=-1;uint32_t buttons{};int tab{};bool active{},close{},entered{};
    bool desktop{},mouseKnown{},savedCursor{};ImVec2 lastMouse{};
    bool DesktopIntent() {
        bool intent=false;
        for(const auto& e:GImGui->InputEventsQueue){
            if(e.Type==ImGuiInputEventType_MousePos && e.MousePos.MouseSource==ImGuiMouseSource_Mouse){
                const ImVec2 p{e.MousePos.PosX,e.MousePos.PosY};
                if(p.x>-FLT_MAX && p.y>-FLT_MAX){
                    intent|=mouseKnown && (std::abs(p.x-lastMouse.x)>1 || std::abs(p.y-lastMouse.y)>1);
                    lastMouse=p;mouseKnown=true;
                }
            }else if(e.Type==ImGuiInputEventType_MouseButton && e.MouseButton.MouseSource==ImGuiMouseSource_Mouse)
                intent|=e.MouseButton.Down;
            else if(e.Type==ImGuiInputEventType_MouseWheel && e.MouseWheel.MouseSource==ImGuiMouseSource_Mouse)
                intent|=e.MouseWheel.WheelX!=0 || e.MouseWheel.WheelY!=0;
        }
        return intent;
    }
    static void ClearMouse(bool vrToo) {
        auto& q=GImGui->InputEventsQueue;
        for(int i=q.Size-1;i>=0;--i){const auto& e=q[i];
            const bool mouse=e.Type==ImGuiInputEventType_MousePos || e.Type==ImGuiInputEventType_MouseWheel || e.Type==ImGuiInputEventType_MouseButton;
            const bool pen=e.Type==ImGuiInputEventType_MousePos?e.MousePos.MouseSource==ImGuiMouseSource_Pen:
                e.Type==ImGuiInputEventType_MouseWheel?e.MouseWheel.MouseSource==ImGuiMouseSource_Pen:
                e.Type==ImGuiInputEventType_MouseButton && e.MouseButton.MouseSource==ImGuiMouseSource_Pen;
            if(mouse && (vrToo || !pen))q.erase(q.Data+i);
        }
    }
public:
    bool CloseRequested(){const bool value=close;close=false;return value;}
    void Release() {
        auto& io=ImGui::GetIO();ClearMouse(true);io.AddMouseSourceEvent(ImGuiMouseSource_Pen);
        io.AddMousePosEvent(-FLT_MAX,-FLT_MAX);io.AddMouseButtonEvent(0,false);io.AddMouseButtonEvent(1,false);
        io.AddMouseSourceEvent(ImGuiMouseSource_Mouse);buttons=0;x=y=-1;active=false;tab=0;
    }
    void Feed(const CvrModUiInput* samples,uint32_t count) {
        auto& io=ImGui::GetIO();float wheel=0;
        const bool incoming=count?(samples[count-1].flags&CVR_MOD_UI_ACTIVE)!=0:active;
        const bool mouseIntent=DesktopIntent();
        if(!incoming){
            if(active && !desktop)Release();
            active=desktop=false;buttons=0;io.MouseDrawCursor=savedCursor;return;
        }
        bool vrIntent=false;
        for(uint32_t i=0;i<count;++i){const auto& e=samples[i];
            vrIntent|=e.buttons && e.buttons!=buttons || e.wheel!=0 || e.tab!=0 || (e.flags&CVR_MOD_UI_BACK);
        }
        const bool wasDesktop=desktop;
        if(mouseIntent && !buttons)desktop=true;
        if(vrIntent)desktop=false;
        io.MouseDrawCursor=desktop || savedCursor;
        if(desktop){active=true;return;}
        ClearMouse(false);
        io.AddMouseSourceEvent(ImGuiMouseSource_Pen);
        if(wasDesktop){io.AddMouseButtonEvent(0,false);io.AddMouseButtonEvent(1,false);}
        for(uint32_t i=0;i<count;++i){const auto& e=samples[i];
            if(!(e.flags&CVR_MOD_UI_ACTIVE)){Release();continue;}
            active=true;io.AddMouseSourceEvent(ImGuiMouseSource_Pen);
            x=e.x;y=e.y;
            if(e.buttons!=buttons){
                io.AddMousePosEvent(x<0?-FLT_MAX:x,y<0?-FLT_MAX:y);
                for(int b=0;b<2;++b)if((e.buttons^buttons)&(1u<<b))io.AddMouseButtonEvent(b,(e.buttons&(1u<<b))!=0);
                buttons=e.buttons;
            }
            if(e.flags&CVR_MOD_UI_BACK){
                if(GImGui->OpenPopupStack.Size || GImGui->ActiveId){io.AddKeyEvent(ImGuiKey_Escape,true);io.AddKeyEvent(ImGuiKey_Escape,false);}
                else close=true;
            }
            tab=std::clamp(tab+e.tab,-1,1);wheel+=e.wheel;
        }
        if(active){io.AddMousePosEvent(x<0?-FLT_MAX:x,y<0?-FLT_MAX:y);if(!buttons && x>=0)io.MouseWheel+=std::clamp(wheel,-.25f,.25f);}
        io.AddMouseSourceEvent(ImGuiMouseSource_Mouse);
    }
    void Viewport(const CvrModUiViewport& viewport,bool vr) {
        auto& io=ImGui::GetIO();
        if(vr){
            if(!entered){savedCursor=io.MouseDrawCursor;desktop=mouseKnown=false;}
            io.DisplaySize={float(viewport.width),float(viewport.height)};io.DisplayFramebufferScale={1,1};
            // Recover windows saved at SBS desktop coordinates, including those
            // first opened after entering VR. Keep the canvas header reachable.
            for(auto* w:GImGui->Windows)if(!w->IsFallbackWindow && !(w->Flags&ImGuiWindowFlags_ChildWindow)){
                const ImVec2 size{std::min(w->SizeFull.x,io.DisplaySize.x-16),std::min(w->SizeFull.y,io.DisplaySize.y-48)};
                ImGui::SetWindowSize(w,size,ImGuiCond_Always);
                ImGui::SetWindowPos(w,{std::clamp(w->Pos.x,8.f,std::max(8.f,io.DisplaySize.x-size.x-8)),
                    std::clamp(w->Pos.y,40.f,std::max(40.f,io.DisplaySize.y-size.y-8))},ImGuiCond_Always);
            }
        }else if(entered){if(!desktop)Release();desktop=mouseKnown=false;io.MouseDrawCursor=savedCursor;}
        entered=vr;
    }
    void Finish(const char* name) {
        auto& g=*GImGui;
        // ImGui's native tab API does not expose next/previous selection. Keep
        // the version-specific implementation inside this adapter, not the ABI.
        if(tab){
            ImGuiTabBar* chosen=nullptr;float best=FLT_MAX;
            for(int i=0;i<g.TabBars.GetMapSize();++i)if(auto* bar=g.TabBars.TryGetMapData(i)){
                if(bar->CurrFrameVisible!=g.FrameCount || bar->Tabs.Size<2)continue;
                if(g.HoveredWindow && !g.HoveredWindow->Rect().Contains(bar->BarRect.GetCenter()))continue;
                const float d=std::abs(bar->BarRect.GetCenter().y-y);if(d<best){best=d;chosen=bar;}
            }
            if(chosen){int current=0;for(int i=0;i<chosen->Tabs.Size;++i)if(chosen->Tabs[i].ID==chosen->SelectedTabId)current=i;
                for(int step=1;step<chosen->Tabs.Size;++step){const int i=(current+tab*step+chosen->Tabs.Size)%chosen->Tabs.Size;
                    const auto& next=chosen->Tabs[i];if(next.LastFrameVisible==g.FrameCount && !(next.Flags&ImGuiTabItemFlags_Button)){
                        chosen->NextSelectedTabId=next.ID;break;
                    }
                }
            }
        }
        tab=0;
        auto* draw=ImGui::GetForegroundDrawList();
        draw->AddRectFilled({0,0},{ImGui::GetIO().DisplaySize.x,30},IM_COL32(18,22,29,240));
        draw->AddText({12,6},IM_COL32(220,230,240,255),name);
        // The XR ray samples this opaque texel, independently of window layout.
        draw->AddRectFilled({0,0},{3,3},IM_COL32(230,245,255,255));
    }
};
inline const CvrModUiApi* Discover(){
    const auto module=GetModuleHandleW(L"cyberpunkvrport.dll");if(!module)return nullptr;
    const auto get=reinterpret_cast<CvrGetModUiApi>(GetProcAddress(module,"CyberpunkVR_GetModUiApi"));
    return get?get(CVR_MOD_UI_VERSION,sizeof(CvrModUiApi)):nullptr;
}
}
