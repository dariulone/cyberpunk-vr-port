#pragma once
#include "ModUiImGui.hpp"
#include "ModUiProducer.hpp"
#include <atomic>

// Included in CET's D3D12 class. This uses CET's own ImGui/backend; the VR DLL
// only receives a texture/fence and emits plain input events through the C ABI.
class CyberpunkVrAdapter {
    const CvrModUiApi* api{};
    std::atomic<CvrModUiHandle> source{};
    std::atomic<bool> vr{},interactive{},visible{},inputFocused{};
    cvr::modui::client::ImGuiInput input;
    cvr::modui::client::Producer producer;
    bool scaled{};
    float desktopFontScale{1};
    ImGuiStyle desktopStyle{};
public:
    explicit CyberpunkVrAdapter(const CvrModUiApi* bridge=nullptr):api(bridge){}
    // CET's Win32 hook can precede F10's hook. Yield mouse/keys when the host
    // gives another overlay focus, and always let F10/Insert reach that host.
    bool PassThrough(UINT message,WPARAM key)const{
        const bool shortcut=(message==WM_KEYDOWN || message==WM_KEYUP || message==WM_SYSKEYDOWN || message==WM_SYSKEYUP) &&
            (key==VK_F10 || key==VK_INSERT);
        return vr && (!interactive || !inputFocused || shortcut);
    }
    SIZE Resolution(SIZE desktop)const{return vr?SIZE{1440,1080}:desktop;}
    void Scale(SIZE desktop,const ImGuiStyle& reference) {
        auto& io=ImGui::GetIO();
        if(vr && !scaled){
            desktopStyle=ImGui::GetStyle();desktopFontScale=io.FontGlobalScale;
            const float rasterScale=std::max(.1f,std::min(float(desktop.cx)/1920.f,float(desktop.cy)/1080.f));
            ImGui::GetStyle()=reference;io.FontGlobalScale=desktopFontScale/rasterScale;scaled=true;
        }else if(!vr && scaled){ImGui::GetStyle()=desktopStyle;io.FontGlobalScale=desktopFontScale;scaled=false;}
    }
    bool Prepare() {
        if(!api)api=cvr::modui::client::Discover();
        if(api && !source)source=api->Register("Cyber Engine Tweaks");
        CvrModUiViewport viewport{sizeof(CvrModUiViewport)};
        vr=api && source && api->Viewport(source,&viewport);
        input.Viewport(viewport,vr);
        if(vr){CvrModUiInput events[64];const auto n=api->Poll(source,events,64);
            if(n)inputFocused=(events[n-1].flags&CVR_MOD_UI_ACTIVE)!=0;
            input.Feed(events,n);
        }else inputFocused=false;
        return vr && input.CloseRequested();
    }
    void Finish(bool overlayOpen) {
        if(!api || !source)return;
        // Many mods keep an ordinary ImGui window alive in onDraw even with
        // CET closed. PgDn closes the entire CET surface, including its strip
        // and pointer; background windows must not leave an invisible modal UI.
        visible=interactive=overlayOpen;
        api->State(source,vr && visible?CVR_MOD_UI_VISIBLE|(interactive?CVR_MOD_UI_INTERACTIVE:0):0);
        if(vr && interactive)input.Finish("Cyber Engine Tweaks  |  Grip top strip to move");
    }
    template<class RenderFn>
    bool Render(ImDrawData* draw,ID3D12Device* device,ID3D12CommandQueue* queue,
                DXGI_FORMAT format,ID3D12DescriptorHeap* heap,RenderFn render) {
        if(!draw)return false;
        if(!vr)return producer.Busy();
        // Closed CET stays absent in VR even if a mod still draws in onDraw.
        if(!visible)return true;
        if(draw->DisplaySize.x!=1440 || draw->DisplaySize.y!=1080)return true;
        return producer.Render(api,source,device,queue,format,heap,render);
    }
    bool Drain(){return producer.Drain();}
    void Hide(){if(api && source)api->State(source,0);vr=interactive=visible=inputFocused=false;scaled=false;input={};}
};
