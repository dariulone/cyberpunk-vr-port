#include "Overlay/ModUiInput.hpp"
#include "Overlay/ModUiImGui.hpp"
#include "CyberpunkVrAdapter.h"
#include "Core/LiveControls.hpp"
LiveControls g_liveControls{};
#include <iostream>
#include <stdexcept>
#include <string>
#include <vector>
using cvr::modui::Input;
using namespace cvr::vrui;
void Check(bool value,const char* message){if(!value)throw std::runtime_error(message);}
uint32_t cetFlags{};
bool cetFocused{};
const CvrModUiApi cetApi{sizeof(CvrModUiApi),CVR_MOD_UI_VERSION,
    [](const char*)->CvrModUiHandle{return 1;},nullptr,
    [](CvrModUiHandle,CvrModUiViewport* v){*v={sizeof(*v),1440,1080,0};return 1;},
    [](CvrModUiHandle,uint32_t flags){cetFlags=flags;},
    [](CvrModUiHandle,CvrModUiInput* out,uint32_t)->uint32_t{out[0]={-1,-1,0,0,cetFocused?CVR_MOD_UI_ACTIVE:0u,0};return 1;}};
struct Fixture {
    Input input;Tracking t;uint64_t tick=1000;
    Fixture(){t.valid=t.cyberfinger=t.preferLeft=true;t.origin=1;t.time=1000000000;t.head.position.y=1.7f;
        for(auto& h:t.hands){h.valid=true;h.aim.position={0,1.7f,-.2f};}step();drain();}
    void step(bool visible=true,bool interactive=true){t.time+=11111111;tick+=11;input.Step(t,visible,interactive,tick);}
    std::vector<CvrModUiInput> drain(){CvrModUiInput e[64];const auto n=input.Poll(e,64);return {e,e+n};}
    CvrModUiInput last(){auto v=drain();Check(!v.empty(),"no pointer sample");return v.back();}
};
int main(int argc,char** argv)try{
    Check(argc==2,"test name missing");std::string test=argv[1];Fixture f;
    if(test=="pointer"){
        f.t.hands[0].trigger=1;f.step();Check(f.last().buttons==CVR_MOD_UI_PRIMARY,"preferred left trigger did not click");
        f.t.hands[0].trigger=0;f.step();Check(!f.last().buttons,"trigger release stuck");
        f.t.hands[1].trigger=1;f.step();Check(f.last().buttons==CVR_MOD_UI_PRIMARY,"right trigger did not transfer pointer");
        f.t.hands[1].valid=false;f.step();Check(!f.last().buttons,"tracking loss retained click");
        f.t.hands[1].valid=true;f.step();Check(!f.last().buttons,"tracking regain clicked while held");
        f.t.hands[1].trigger=0;f.step();f.t.hands[1].trigger=1;f.step();Check(f.last().buttons!=0,"release did not rearm");
        f.t.hands[1].trigger=0;f.step();f.step(true,false);f.step();
        f.t.hands[0].valid=false;f.step();Check(!f.input.View().ray,"CF tracking dropout switched to right hand");
        f.t.hands[0].valid=true;f.step();f.t.hands[0].trigger=1;f.step();
        Check(f.last().buttons==CVR_MOD_UI_PRIMARY,"reopening did not restore configured left hand");
    }else if(test=="anchor"){
        const auto pose=f.input.View().pose;f.t.head.position.x=1;f.t.head.orientation={0,.5f,0,.8660254f};f.step();
        const auto after=f.input.View().pose;Check(after.position.x==pose.position.x && after.orientation.y==pose.orientation.y,"panel followed head");
        Check(f.input.View().ray,"laser vanished outside buttons");
        f.t.valid=false;f.step();Check(!f.input.View().valid,"tracking loss kept a live pointer");
        f.t.head.position.x=5;f.t.valid=true;f.step();
        Check(f.input.View().pose.position.x==pose.position.x,"tracking recovery reattached panel to head");
        cvr::vrui::Settings settings;settings.scale=.75f;settings.distance=2;
        f.input.Step(f.t,true,true,f.tick,settings);
        Check(std::abs(f.input.View().width-1.1625f)<.001f,"mod panel ignored shared size");
        f.input.Step(f.t,false,false,f.tick,settings);f.t.head.orientation={0,0,0,1};
        f.input.Step(f.t,true,true,f.tick,settings);
        Check(std::abs(f.input.View().pose.position.z+2)<.001f,"reopening ignored shared distance");
    }else if(test=="grab"){
        f.t.hands[0].grip=1;f.step();Check(f.last().buttons==CVR_MOD_UI_SECONDARY,"Grip did not right-click");
        f.t.hands[0].grip=0;f.step();f.drain();
        f.t.hands[0].aim.position.y=1.7f+f.input.View().height*.49f;f.step();f.t.hands[0].grip=1;f.step();
        Check(f.input.View().dragging && !f.last().buttons,"title drag also right-clicked");
        const auto x=f.input.View().pose.position.x;f.t.hands[0].aim.position.x+=.2f;f.step();Check(f.input.View().pose.position.x>x+.19f,"Grip did not move panel");
    }else if(test=="scroll"){
        f.t.hands[0].stickY=.8f;f.step();Check(f.last().wheel>0,"pointer-hand scroll missing");
        f.t.hands[0].stickY=0;f.t.hands[1].stickY=.8f;f.step();Check(f.last().wheel==0,"opposite-hand Y scrolled");
    }else if(test=="tabs"){
        f.t.hands[1].valid=false;f.t.hands[1].stickX=1;f.step();Check(f.last().tab==1,"opposite physical stick lost with optical pose");
        f.step();Check(f.last().tab==0,"held stick repeated tabs");
        f.t.hands[1].stickX=0;f.step();f.t.hands[1].stickX=-1;f.step();Check(f.last().tab==-1,"tab rearm failed");
    }else if(test=="release"){
        f.step(false);f.t.hands[0].trigger=1;f.step();Check(!f.last().buttons,"opening with held trigger clicked");
        f.t.hands[0].trigger=0;f.step();f.t.hands[0].trigger=1;f.step();f.drain();
        f.step(true,false);Check(!f.last().buttons,"focus loss did not release");
        f.step();Check(!f.last().buttons,"focus regain clicked held trigger");
        f.t.menuLeft=true;f.step();f.t.menuLeft=false;f.step();Check(f.last().flags&CVR_MOD_UI_BACK,"Menu release did not Back");
        f.t.menuLeft=f.t.menuRight=true;f.step();f.t.menuLeft=f.t.menuRight=false;f.step();Check(!(f.last().flags&CVR_MOD_UI_BACK),"HUD chord leaked Back");
    }else if(test=="capture"){
        SetExternalCapture(true);Check(CapturesInput(),"external UI did not claim gameplay");SetExternalCapture(false);Check(!CapturesInput(),"external capture stuck");
    }else if(test=="imgui"){
        ImGui::CreateContext();auto& io=ImGui::GetIO();io.IniFilename=nullptr;io.DisplaySize={1440,1080};io.DeltaTime=1.f/90;
        unsigned char* pixels;int w,h;io.Fonts->GetTexDataAsRGBA32(&pixels,&w,&h);
        cvr::modui::client::ImGuiInput feed;
        auto frame=[&](int tab){CvrModUiInput e{200,200,0,0,CVR_MOD_UI_ACTIVE,tab};feed.Feed(&e,1);ImGui::NewFrame();
            ImGui::SetNextWindowPos({50,50});ImGui::SetNextWindowSize({600,500});ImGui::Begin("Test");
            if(ImGui::BeginTabBar("tabs")){for(const char* name:{"First","Second"})if(ImGui::BeginTabItem(name)){ImGui::TextUnformatted(name);ImGui::EndTabItem();}ImGui::EndTabBar();}
            ImGui::End();feed.Finish("Test canvas");ImGui::Render();};
        frame(0);auto* bar=GImGui->TabBars.TryGetMapData(0);Check(bar && bar->Tabs.Size==2,"test tabs missing");const auto first=bar->SelectedTabId;
        frame(1);frame(0);Check(bar->SelectedTabId!=first,"standard ImGui tabs did not change");
        auto* window=ImGui::FindWindowByName("Test");
        ImGui::SetWindowPos(window,{4000,2000});ImGui::SetWindowSize(window,{2400,1400});
        feed.Viewport({sizeof(CvrModUiViewport),1440,1080,0},true);
        Check(window->Pos.x>=8 && window->Pos.y>=40 && window->Pos.x+window->SizeFull.x<=1440 &&
            window->Pos.y+window->SizeFull.y<=1080,"SBS window remained off the VR canvas");
        CvrModUiInput scroll{200,200,.2f,0,CVR_MOD_UI_ACTIVE,0};feed.Feed(&scroll,1);ImGui::NewFrame();
        Check(ImGui::GetIO().MouseWheel>0,"scroll lost at ImGui NewFrame");ImGui::EndFrame();
        CvrModUiInput back{-1,-1,0,0,CVR_MOD_UI_ACTIVE|CVR_MOD_UI_BACK,0};feed.Feed(&back,1);Check(feed.CloseRequested(),"idle Menu did not close");
        ImGui::DestroyContext();
    }else if(test=="mouse"){
        ImGui::CreateContext();auto& io=ImGui::GetIO();io.IniFilename=nullptr;io.DisplaySize={1440,1080};io.DeltaTime=1.f/90;
        io.ConfigInputTrickleEventQueue=false;unsigned char* p;int w,h;io.Fonts->GetTexDataAsRGBA32(&p,&w,&h);
        cvr::modui::client::ImGuiInput feed;feed.Viewport({sizeof(CvrModUiViewport),1440,1080,0},true);
        CvrModUiInput e{100,100,0,0,CVR_MOD_UI_ACTIVE,0};
        auto frame=[&]{feed.Feed(&e,1);ImGui::NewFrame();ImGui::EndFrame();};
        io.AddMousePosEvent(400,400);frame();Check(io.MousePos.x==100,"passive desktop position stole laser");
        io.AddMousePosEvent(450,420);frame();Check(io.MousePos.x==450,"desktop movement was erased");
        io.AddMouseButtonEvent(0,true);frame();Check(io.MouseDown[0],"desktop click was erased");
        io.AddMouseButtonEvent(0,false);frame();Check(!io.MouseDown[0],"desktop release stuck");
        frame();Check(io.MousePos.x==450,"idle laser overwrote mouse position");
        e.buttons=CVR_MOD_UI_PRIMARY;frame();Check(io.MousePos.x==100 && io.MouseDown[0],"trigger did not reclaim laser");
        e.buttons=0;frame();e.flags=0;io.AddMousePosEvent(600,600);io.AddMouseButtonEvent(0,true);frame();
        e.flags=0;io.AddMouseButtonEvent(0,false);frame();
        io.AddMouseButtonEvent(0,true);frame();Check(io.MouseDown[0],"passive mod HUD erased desktop events");
        ImGui::DestroyContext();
    }else if(test=="cet_lifetime"){
        ImGui::CreateContext();auto& io=ImGui::GetIO();io.IniFilename=nullptr;io.DeltaTime=1.f/90;
        unsigned char* p;int w,h;io.Fonts->GetTexDataAsRGBA32(&p,&w,&h);
        CyberpunkVrAdapter adapter(&cetApi);
        auto frame=[&](bool open){adapter.Prepare();ImGui::NewFrame();
            ImGui::Begin("Always-running mod HUD");ImGui::TextUnformatted("This window accepts mouse input");ImGui::End();
            adapter.Finish(open);ImGui::Render();};
        frame(false);frame(false);Check(cetFlags==0,"closed CET retained a mod HUD surface");
        Check(adapter.PassThrough(WM_MOUSEMOVE,0),"closed CET swallowed desktop mouse");
        frame(true);Check(cetFlags==(CVR_MOD_UI_VISIBLE|CVR_MOD_UI_INTERACTIVE),"open CET failed to capture");
        cetFocused=true;frame(true);Check(!adapter.PassThrough(WM_MOUSEMOVE,0),"focused CET yielded its own mouse");
        Check(adapter.PassThrough(WM_KEYUP,VK_F10),"CET swallowed the F10 shortcut");
        cetFocused=false;frame(true);Check(adapter.PassThrough(WM_MOUSEMOVE,0),"CET swallowed F10's desktop mouse");
        frame(false);Check(cetFlags==0,"closing CET retained its strip or input capture");
        adapter.Hide();Check(!cetFlags,"hidden adapter retained visibility");ImGui::DestroyContext();
    }else if(test=="abi"){
        static_assert(sizeof(CvrModUiInput)==24);static_assert(std::is_standard_layout_v<CvrModUiApi>);
        Check(CVR_MOD_UI_VERSION==1,"ABI changed without test update");
    }else throw std::runtime_error("unknown test");
    std::cout<<test<<" passed\n";return 0;
}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}
