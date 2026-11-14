#include "Overlay/VrOverlay.hpp"
#include "Core/LiveControls.hpp"
LiveControls g_liveControls{};
namespace cvr::fingergrip { bool GripFromFingers(){return false;} }
#include <stdexcept>
#include <iostream>
#include <cstdio>
#include <limits>
#include <chrono>
#include <thread>
using namespace cvr::vrui;
void Check(bool v,const char* message){if(!v)throw std::runtime_error(message);}
void Near(float a,float b,const char* m){Check(std::abs(a-b)<.001f,m);}
Tracking Start(){Tracking t;t.valid=true;t.origin=1;t.time=1000000000;t.head.position.y=1.7f;
    for(auto& h:t.hands){h.valid=true;h.aim.position={0,1.7f,-.3f};}return t;}
int main(int argc,char** argv)try{
    Check(argc==2,"test name required");const std::string name=argv[1];auto t=Start();Settings s;
    if(name=="interface_master"){
        g_liveControls.xrVrInterface=0;
        t.hands[0].stickClick=t.hands[1].stickClick=true;
        for(int i=0;i<70;++i){t.time+=11111111;UpdateTracking(t);}
        Check(!CapturesInput() && !ConsumeMenuSummon(),"disabled SAO swallowed L3+R3");
        g_liveControls.xrVrInterface=1;
        for(int i=0;i<50;++i){t.time+=11111111;UpdateTracking(t);}
        Check(CapturesInput() && ConsumeMenuSummon(),"enabled SAO chord stopped working");
        // Disabling while still held must clear only the SAO release latch.
        g_liveControls.xrVrInterface=0;t.hands[0].trigger=1;
        t.time+=11111111;UpdateTracking(t);
        Check(!CapturesInput() && !ConsumeMenuSummon(),"disabled SAO retained a pending input capture");
        Toggle();Check(PollCommand()==1 && CapturesInput(),"SAO master disabled F10 opening");
        BridgeUpdate(true,true,"CONTINUE\nSETTINGS");t.time+=11111111;UpdateTracking(t);
        Check(Visible() && CapturesInput(),"SAO master released active settings panel input");
        Close();Check(PollCommand()==2,"settings close missing");
        t.time+=11111111;UpdateTracking(t);Check(CapturesInput(),"settings release latch was cleared by SAO master");
        t.hands[0].trigger=0;t.time+=11111111;UpdateTracking(t);
        Check(CapturesInput(),"settings released held stick clicks with SAO disabled");
        t.hands[0].stickClick=t.hands[1].stickClick=false;
        t.time+=11111111;UpdateTracking(t);Check(!CapturesInput(),"settings neutral release stuck");
    }else if(name=="chord"){
        HoldChord c;for(int i=0;i<49;++i)Check(!c.Update(true,.01f),"chord triggered before half a second");
        bool fired=false;for(int i=0;i<3;++i)fired|=c.Update(true,.01f);Check(fired,"half-second chord did not trigger");
        for(int i=0;i<300;++i)Check(!c.Update(true,.01f),"held chord retriggered");
        c.Update(false,.01f);for(int i=0;i<60;++i)c.Update(true,.01f);c.Update(false,.01f);
        for(int i=0;i<30;++i)Check(!c.Update(true,.01f),"partial holds accumulated across release");
    }else if(name=="chord_reopen" || name=="bridge_timeout"){
        Toggle();Check(PollCommand()==1,"opening command missing");BridgeUpdate(true,true,"CONTINUE\nSETTINGS");
        UpdateTracking(t);Check(Visible(),"initial opening failed");
        if(name=="bridge_timeout"){
            std::this_thread::sleep_for(std::chrono::milliseconds(1600));
            t.time+=11111111;UpdateTracking(t);Check(!Visible(),"unresponsive active bridge was not closed");
        }else{
            Close();Check(PollCommand()==2,"closing command missing");BridgeUpdate(false,true,"CONTINUE\nSETTINGS");
            std::this_thread::sleep_for(std::chrono::milliseconds(1600));
            // L3 + R3 belongs to the VR menu now: half a second asks for it once per hold, the panel stays shut
            t.hands[0].stickClick=t.hands[1].stickClick=true;
            for(int i=0;i<40;++i){t.time+=11111111;UpdateTracking(t);}
            Check(!ConsumeMenuSummon(),"stick hold asked for the VR menu before half a second");
            for(int i=0;i<10;++i){t.time+=11111111;UpdateTracking(t);}
            Check(ConsumeMenuSummon() && !ConsumeMenuSummon(),"half-second stick hold did not ask for the VR menu once");
            Check(PollCommand()==0 && !Visible(),"stick hold toggled the panel");
            for(int i=0;i<95;++i){t.time+=11111111;UpdateTracking(t);}
            Check(!ConsumeMenuSummon(),"continued hold asked for the VR menu again");
            t.hands[0].stickClick=t.hands[1].stickClick=false;t.time+=11111111;UpdateTracking(t);
            t.hands[0].stickClick=t.hands[1].stickClick=true;
            for(int i=0;i<50;++i){t.time+=11111111;UpdateTracking(t);}
            Check(ConsumeMenuSummon(),"second stick hold did not ask for the VR menu");
            // the keyboard's toggle still opens the panel
            Toggle();Check(PollCommand()==1,"panel toggle no longer opens the panel");
        }
    }else if(name=="desktop"){
        for(auto size:{XrVector2f{2048,2048},XrVector2f{1920,1080},XrVector2f{1280,720}}){
            const auto rect=DesktopPlacement(size.x,size.y);
            Check(rect.x>=0 && rect.y>=0 && rect.x+CanvasWidth*rect.scale<=size.x && rect.y+CanvasHeight*rect.scale<=size.y,"desktop mirror does not fit");
            Near(((720*rect.scale+rect.x)-rect.x)/rect.scale,720,"desktop pointer x mapping");
            Near(((540*rect.scale+rect.y)-rect.y)/rect.scale,540,"desktop pointer y mapping");
        }
    }else if(name=="ray"){
        XrPosef panel{{0,0,0,1},{0,1.7f,-1.4f}};
        const auto hit=Intersect(t.hands[1].aim,panel,1.6f,1.2f);Check(hit.valid,"forward ray missed");Near(hit.x,720,"center x");Near(hit.y,540,"center y");Near(hit.distance,1.1f,"ray distance");
        auto ray=t.hands[1].aim;ray.position.x=1;Check(!Intersect(ray,panel,1.6f,1.2f).valid,"outside panel accepted");
        ray=t.hands[1].aim;ray.orientation={0,1,0,0};Check(!Intersect(ray,panel,1.6f,1.2f).valid,"back-facing ray accepted");
        const XrQuaternionf q{0,std::sin(.4f),0,std::cos(.4f)};panel.orientation=q;panel.position=RotateVector(q,panel.position);
        ray=t.hands[1].aim;ray.orientation=q;ray.position=RotateVector(q,ray.position);
        Near(Intersect(ray,panel,1.6f,1.2f).x,720,"rotated panel ray");
    }else if(name=="drag" || name=="distance"){
        Placement p;p.Update(t,s,.01f);t.hands[1].grip=1;p.Grab(1,t);
        if(name=="drag"){
            t.hands[1].aim.position.x+=.25f;p.Update(t,s,.01f);Near(p.pose.position.x,.25f,"grip translation did not move panel");
            t.hands[1].grip=0;p.Update(t,s,.01f);Near(p.pose.position.x,.25f,"release lost placement");Check(p.dragging<0,"drag remained latched");
        }else{
            t.hands[1].stickY=1;for(int i=0;i<500;++i)p.Update(t,s,.02f);Near(s.distance,s.maxDistance,"forward stick did not stop at far bound");
            t.hands[1].stickY=-1;for(int i=0;i<500;++i)p.Update(t,s,.02f);Near(s.distance,s.minDistance,"backward stick did not stop at near bound");
        }
    }else if(name=="drag_rotate"){
        // The grip carries the panel on the controller: turning the wrist swings it on the full lever.
        Placement p;p.Update(t,s,.01f);t.hands[1].grip=1;p.Grab(1,t);
        const float a=.3f;t.hands[1].aim.orientation={0,std::sin(-a*.5f),0,std::cos(-a*.5f)};p.Update(t,s,.01f);
        Near(p.pose.position.x,1.1f*std::sin(a),"wrist turn did not swing the panel");
        Near(s.distance,.3f+1.1f*std::cos(a),"wrist turn changed the lever");
        Check(std::abs(p.pose.orientation.y)>.05f,"swung panel did not turn to face the head");
    }else if(name=="follow"){
        Placement p;p.Update(t,s,.01f);t.head.orientation={0,std::sin(.2f),0,std::cos(.2f)};
        for(int i=0;i<290;++i){p.Update(t,s,.01f);Near(p.follow.yaw,0,"cone moved before rest delay");}
        for(int i=0;i<70;++i)p.Update(t,s,.01f);Check(std::abs(p.follow.yaw-.4f)<.036f && !p.follow.following,"delayed follow did not settle");
    }else if(name=="tracking_loss"){
        Placement p;p.Update(t,s,.01f);t.hands[1].grip=1;p.Grab(1,t);const auto before=p.pose;t.valid=false;p.Update(t,s,.01f);
        Check(p.dragging<0 && p.pose.position.z==before.position.z,"tracking loss dragged panel");
        t.valid=true;t.origin=2;t.head.orientation={0,std::sin(.2f),0,std::cos(.2f)};p.Update(t,s,.01f);Near(p.follow.yaw,.4f,"origin reset retained anchor");
    }else if(name=="settings"){
        s.distance=99;s.minDistance=-1;s.maxDistance=0;s.cone=500;s.scale=std::numeric_limits<float>::quiet_NaN();s=Sanitize(s);
        Check(s.distance<=s.maxDistance && s.maxDistance>s.minDistance && s.cone==90 && s.scale==1,"settings clamping failed");
        FILE* f=std::tmpfile();Check(f!=nullptr,"temp file");WriteSettings(f,s);rewind(f);Settings restored;char line[128];while(std::fgets(line,sizeof(line),f))ParseSetting(line,restored);fclose(f);Near(restored.distance,s.distance,"setting round trip");
    }else if(name=="input" || name=="menu"){
        Toggle();Check(PollCommand()==1 && !Visible() && CapturesInput(),"overlay visible before pause acknowledgement");
        BridgeUpdate(true,true,"Resume\nSettings\nExit");Check(Visible(),"pause acknowledgement did not open overlay");
        UpdateTracking(t);ConsumePointerEvents();
        if(name=="input"){
            t.hands[1].trigger=1;t.time+=11111111;UpdateTracking(t);auto e=ConsumePointerEvents();Check(!e.empty() && e.back().down,"trigger click missing");
            t.hands[1].valid=false;t.time+=11111111;UpdateTracking(t);e=ConsumePointerEvents();Check(!e.empty() && !e.back().down,"tracking loss left click held");
            t.hands[1].valid=true;t.hands[1].trigger=0;t.hands[1].aim.position.x=5;t.time+=11111111;UpdateTracking(t);ConsumePointerEvents();
            t.hands[1].trigger=1;t.time+=11111111;UpdateTracking(t);ConsumePointerEvents();
            t.hands[1].aim.position.x=0;t.time+=11111111;UpdateTracking(t);e=ConsumePointerEvents();Check(!e.back().down,"entering panel with trigger held created a click");
            auto options=GetSettings();options.hand=2;SetSettings(options);t.hands[1].aim.position.x=5;t.hands[0].trigger=0;t.time+=11111111;UpdateTracking(t);ConsumePointerEvents();
            t.hands[0].trigger=1;t.time+=11111111;UpdateTracking(t);e=ConsumePointerEvents();Check(e.back().down,"automatic left pointer press missing");
            t.time+=11111111;UpdateTracking(t);e=ConsumePointerEvents();Check(e.back().down,"automatic pointer switched hands while pressed");
        }else{
            SelectMenuItem(1);Check(!Visible() && PollCommand()==101,"native menu index not dispatched");
            Check(CapturesInput(),"selection released held input to game");t.time+=11111111;UpdateTracking(t);Check(!CapturesInput(),"neutral input did not release capture");
        }
    }else Check(false,"unknown test");
    std::cout<<"PASS "<<name<<'\n';return 0;
}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}
