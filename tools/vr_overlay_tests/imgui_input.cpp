#include "Overlay/VrImGuiInput.hpp"
#include <imgui_internal.h>
#include <array>
#include <iostream>
#include <stdexcept>
using namespace cvr::vrui;
void Check(bool v,const char* message){if(!v)throw std::runtime_error(message);}
void Pane(){ImGui::SetNextWindowPos({0,0});ImGui::SetNextWindowSize({900,700});ImGui::Begin("Scroll target");
    for(int i=0;i<200;++i)ImGui::Text("Setting %d",i);ImGui::End();}
void PointerRouting(ImGuiIO& io){
    ImGuiPointerRouter input;
    uint64_t now=10000;int vrClicks=0,mouseClicks=0;
    struct State {ImVec2 pos;bool down,clicked,released;float wheel;};
    auto frame=[&](std::span<const PointerEvent> events,Hit hit,bool dragging=false){
        // Win32's NewFrame fallback may still report the OS cursor first.
        io.AddMousePosEvent(-100,-100);input.Submit(io,events,hit,dragging,now);
        ImGui::NewFrame();
        const State state{io.MousePos,io.MouseDown[0],io.MouseClicked[0],io.MouseReleased[0],io.MouseWheel};
        ImGui::SetNextWindowPos({0,0});ImGui::SetNextWindowSize({900,700});
        ImGui::Begin("Pointer target",nullptr,ImGuiWindowFlags_NoSavedSettings);
        ImGui::SetCursorScreenPos({280,250});if(ImGui::Button("VR target",{160,60}))++vrClicks;
        ImGui::SetCursorScreenPos({500,250});if(ImGui::Button("Desktop target",{160,60}))++mouseClicks;
        ImGui::End();ImGui::EndFrame();now+=33;return state;
    };
    auto at=[](const State& s,float x,float y){return s.pos.x==x && s.pos.y==y;};
    Hit hit{true,320,280};
    std::array<PointerEvent,1> neutral{{{320,280,0,false}}};
    input.MouseMove(540,280,now);
    Check(at(frame({},{}),540,280),"desktop hover unavailable before VR pointing");
    Check(at(frame(neutral,hit),320,280),"recent passive mouse motion overrode the VR cursor");
    for(int i=0;i<180;++i){
        if(i%3==0)input.MouseLeave(now);
        else input.MouseMove(i%3==1?540:-20,280,now);
        hit.x=300+float(i%25);neutral[0].x=hit.x;
        Check(at(frame(neutral,hit),hit.x,hit.y),"Win32 mouse motion/leave detached cursor from VR ray");
        Check(GImGui->InputEventsQueue.Size==0,"passive mouse messages built a pointer backlog");
    }
    std::array<PointerEvent,1> lost{{{-1,-1,0,false}}};
    input.MouseMove(540,280,now);
    Check(frame(lost,{}).pos.x==-FLT_MAX,"tracking loss revived a stale desktop cursor");
    hit.x=320;neutral[0].x=320;
    Check(at(frame(neutral,hit),320,280),"VR cursor did not recover with tracking");
    std::array<PointerEvent,2> tap{{{320,280,0,true},{320,280,0,false}}};
    Check(frame(tap,hit).clicked,"VR click lost after noisy desktop input");
    frame(neutral,hit);frame(neutral,hit);
    Check(vrClicks==1 && mouseClicks==0,"trigger did not activate the pointed VR button");
    input.MouseButton(540,280,true,now);input.MouseButton(540,280,false,now);
    Check(at(frame(neutral,hit),540,280),"explicit desktop click did not claim the mirror");
    frame(neutral,hit);frame(neutral,hit);
    Check(vrClicks==1 && mouseClicks==1,"short desktop click was dropped or clicked the VR target");
    // Repeated WM_MOUSEMOVE at the same position must not extend mouse ownership.
    now+=1800;input.MouseMove(540,280,now);
    Check(at(frame(neutral,hit),320,280),"idle desktop cursor kept ownership indefinitely");
    input.MouseButton(540,280,true,now);frame(neutral,hit);
    now+=3000;
    const auto held=frame(neutral,hit);
    Check(at(held,540,280) && held.down,"desktop drag lost ownership at the idle timeout");
    // VR reclaim cancels the old mouse press outside all widgets, then delivers
    // the trigger at its own hit. No cross-source drag/release may click Desktop.
    const std::array<PointerEvent,1> trigger{{{320,280,0,true}}};
    frame(trigger,hit);frame(trigger,hit);frame(neutral,hit);frame(neutral,hit);
    Check(vrClicks==2 && mouseClicks==1,"source handoff leaked a click or lost the trigger release");
    input.MouseButton(540,280,false,now);
    input.MouseWheel(540,280,-1,0,now);
    const auto wheel=frame(neutral,hit);
    Check(at(wheel,540,280) && wheel.wheel==-1,"desktop wheel lost its target or native detent");
    input.MouseLeave(now);
    Check(at(frame(neutral,hit),320,280),"desktop leave hid a valid VR cursor after mouse use");
    input.MouseWheel(540,280,-1,0,now);frame(neutral,hit);
    const std::array<PointerEvent,1> stick{{{320,280,-.1f,false}}};
    const auto scroll=frame(stick,hit);
    Check(at(scroll,320,280) && std::abs(scroll.wheel+.1f)<.001f,"stick did not reclaim VR scrolling");
    input.MouseWheel(540,280,-1,0,now);frame(neutral,hit);
    Check(at(frame(neutral,hit,true),320,280),"grip dragging did not reclaim the VR cursor");
    input.MouseFocusLost(now);
    Check(at(frame(neutral,hit),320,280),"desktop focus loss cleared VR pointer ownership");
    input.Reset(io);input.MouseMove(540,280,now);
    Check(at(frame({},{}),540,280),"reopened panel retained old pointer ownership");
    Check(GImGui->InputEventsQueue.Size==0,"source transitions left deferred pointer events");
    input.MouseButton(540,280,true,now,1);frame({},{});
    Check(io.MouseDown[1] && !io.MouseDown[0],"desktop right-click did not survive pointer routing");
    input.MouseFocusLost(now);frame({},{});frame({},{});
    Check(!io.MouseDown[1],"desktop right-click stuck after focus loss");
}
void KeyboardNavigation(ImGuiIO& io) {
    io.ConfigFlags|=ImGuiConfigFlags_NavEnableKeyboard;
    ImGuiPointerFeed feed;int activated=0;ImGuiID second{};
    auto frame=[&](PointerEvent e,bool initial=false){
        feed.Submit(io,std::span<const PointerEvent>(&e,1),300,300);
        ImGui::NewFrame();ImGui::SetNextWindowPos({0,0});ImGui::SetNextWindowSize({900,700});
        if(initial)ImGui::SetNextWindowFocus();
        ImGui::Begin("CF navigation",nullptr,ImGuiWindowFlags_NoSavedSettings);
        if(initial)ImGui::SetKeyboardFocusHere();
        ImGui::Button("First");second=ImGui::GetID("Second");
        if(ImGui::Button("Second"))++activated;
        ImGui::Button("Third");ImGui::End();ImGui::EndFrame();
    };
    PointerEvent neutral;neutral.focus=true;
    frame(neutral,true);frame(neutral);frame(neutral);
    PointerEvent down=neutral;down.direction=2;frame(down);
    for(int i=0;i<3;++i)frame(neutral);
    Check(GImGui->NavId==second,"left stick did not move actual ImGui highlight");
    Check(io.MousePos.x==-FLT_MAX,"ray position fought keyboard focus");
    PointerEvent trigger=neutral;trigger.activate=true;frame(trigger);
    for(int i=0;i<3;++i)frame(neutral);
    Check(activated==1,"trigger did not activate highlighted ImGui button once");
    Check(GImGui->InputEventsQueue.Size==0,"keyboard navigation accumulated queued input");
}
int main()try{
    ImGui::CreateContext();auto& io=ImGui::GetIO();io.IniFilename=nullptr;io.DisplaySize={1440,1080};io.DeltaTime=1.0f/30;
    unsigned char* pixels;int w,h;io.Fonts->GetTexDataAsRGBA32(&pixels,&w,&h);
    ImGuiPointerFeed feed;
    for(int i=0;i<2;++i){io.AddMousePosEvent(300,300);ImGui::NewFrame();Pane();ImGui::EndFrame();}
    float applied=0;int peak=0;
    // 90Hz controller input and 30Hz UI, with continuous hand jitter. Include
    // the desktop-position event sent by the Win32 backend before VR input.
    for(int frame=0;frame<90;++frame){
        std::array<PointerEvent,3> samples;
        for(int i=0;i<3;++i)samples[i]={300+float((frame*3+i)%9),300,-5.0f/90,false};
        io.AddMousePosEvent(0,0);feed.Submit(io,samples,samples.back().x,samples.back().y);
        ImGui::NewFrame();applied+=io.MouseWheel;Pane();ImGui::EndFrame();
        peak=std::max(peak,GImGui->InputEventsQueue.Size);
    }
    Check(peak<=4,"stick scrolling accumulated a deferred ImGui event backlog");
    Check(std::abs(applied+15)<.01f,"scroll speed depends on consumer cadence or drops XR intervals");
    const float scrolled=ImGui::FindWindowByName("Scroll target")->Scroll.y;
    Check(scrolled>900 && scrolled<1100,"analog wheel did not move the actual hovered ImGui pane");
    std::array<PointerEvent,3> released{{{310,300,-.05f,false},{311,300,-.05f,false},{312,300,0,false}}};
    feed.Submit(io,released,312,300);ImGui::NewFrame();Check(io.MouseWheel==0,"stick release retained queued scrolling");Pane();ImGui::EndFrame();
    Check(ImGui::FindWindowByName("Scroll target")->Scroll.y==scrolled,"pane kept moving after stick release");
    std::array<PointerEvent,2> tap{{{312,300,0,true},{312,300,0,false}}};
    feed.Submit(io,tap,312,300);ImGui::NewFrame();Check(io.MouseClicked[0] && io.MouseDown[0],"short trigger press was collapsed");Pane();ImGui::EndFrame();
    std::array<PointerEvent,1> neutral{{{313,300,0,false}}};feed.Submit(io,neutral,313,300);ImGui::NewFrame();
    Check(io.MouseReleased[0] && !io.MouseDown[0] && io.MouseWheel==0,"trigger release delayed behind scrolling");Pane();ImGui::EndFrame();
    for(int i=0;i<4;++i){feed.Submit(io,neutral,313,300);ImGui::NewFrame();Check(io.MouseWheel==0,"scroll tail after neutral input");Pane();ImGui::EndFrame();}
    Check(GImGui->InputEventsQueue.Size==0,"input queue did not drain");
    std::array<PointerEvent,64> delayed;for(auto& e:delayed)e={313,300,-.05f,false};
    feed.Submit(io,delayed,313,300);ImGui::NewFrame();Check(std::abs(io.MouseWheel)<=.2501f,"render stall replayed an entire scroll history");Pane();ImGui::EndFrame();
    feed.Submit(io,delayed,-1,-1);ImGui::NewFrame();Check(io.MouseWheel==0,"ray outside panel kept scrolling");Pane();ImGui::EndFrame();
    PointerRouting(io);
    KeyboardNavigation(io);
    // Secondary input stays separate from primary clicks. Menu first cancels
    // an actual ImGui popup, then requests closing an idle settings panel.
    feed.Reset();
    std::array<PointerEvent,1> secondary{{{320,300,0,false,0,false,false,true}}};
    feed.Submit(io,secondary,320,300);ImGui::NewFrame();
    Check(io.MouseDown[1] && !io.MouseDown[0],"Grip was not an independent right mouse press");
    Pane();ImGui::EndFrame();
    feed.Submit(io,neutral,320,300);ImGui::NewFrame();
    Check(io.MouseReleased[1] && !io.MouseDown[0],"Grip release leaked a primary click");
    Pane();ImGui::EndFrame();
    feed.Submit(io,neutral,320,300);ImGui::NewFrame();
    ImGui::Begin("Back test");ImGui::OpenPopup("Options");
    if(ImGui::BeginPopup("Options")){ImGui::Text("Context actions");ImGui::EndPopup();}
    ImGui::End();ImGui::EndFrame();
    Check(GImGui->OpenPopupStack.Size>0,"test popup failed to open");
    std::array<PointerEvent,1> back{{{-1,-1,0,false,0,false,false,false,true}}};
    feed.Submit(io,back,-1,-1);Check(!feed.ConsumeClose(),"Menu closed panel instead of dismissing popup");
    ImGui::NewFrame();Pane();ImGui::EndFrame();
    feed.Submit(io,neutral,320,300);ImGui::NewFrame();Pane();ImGui::EndFrame();
    Check(GImGui->OpenPopupStack.Size==0,"Menu did not dismiss ImGui popup");
    feed.Submit(io,back,-1,-1);Check(feed.ConsumeClose(),"Menu did not close an idle panel");
    Check(!feed.ConsumeClose(),"Menu close request repeated");
    ImGui::DestroyContext();std::cout<<"PASS scrolling, pointer ownership, secondary input and popup-aware Back\n";return 0;
}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}
