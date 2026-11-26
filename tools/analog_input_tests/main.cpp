#include "Hooks/AnalogStick.hpp"
#include "Hooks/KeypadInput.hpp"
#include "Hooks/WeaponTrigger.hpp"
#include <bit>
#include <cstdint>
#include <cstring>
#include <iostream>
#include <limits>
#include <stdexcept>
#include <string>
#include <vector>
void Check(bool ok,const char* message){if(!ok)throw std::runtime_error(message);}
#if CVR_RUST_INPUT
// Both implementations on the same inputs, compared bit for bit: the C++ reference (cvr::input::cpp) and Rust
// (cvr_rust.lib) called directly. Dense sweeps, every boundary and its float neighbours, special values and random
// bit patterns, then random update sequences for the two stateful types.
namespace parity {
struct Rng { uint64_t s=0x9E3779B97F4A7C15ull; uint64_t Next(){s^=s<<13;s^=s>>7;s^=s<<17;return s;} };
bool Same(float a,float b){return std::bit_cast<uint32_t>(a)==std::bit_cast<uint32_t>(b);}
std::vector<float> Inputs() {
    std::vector<float> v;
    for(int i=-20000;i<=20000;++i)v.push_back(i*1e-4f);
    const float lim[]={0,.15f,.2f,.3f,.5f,.525f,.575f,.8f,.85f,.849f,.9f,.901f,1,1.01f,1.1f,2,-.1f,.31f,.79f};
    for(float b:lim)for(float s:{1.f,-1.f}){
        float x=s*b; v.push_back(x);
        float up=x,down=x;
        for(int k=0;k<4;++k){up=std::nextafter(up,INFINITY);down=std::nextafter(down,-INFINITY);v.push_back(up);v.push_back(down);}
    }
    using L=std::numeric_limits<float>;
    for(float x:{L::denorm_min(),L::min(),L::max(),L::infinity(),L::quiet_NaN(),L::signaling_NaN(),L::epsilon()}){v.push_back(x);v.push_back(-x);}
    Rng r;for(int i=0;i<200000;++i)v.push_back(std::bit_cast<float>(uint32_t(r.Next())));
    return v;
}
uint64_t Run() {
    using namespace cvr::input;
    uint64_t n=0;
    const auto in=Inputs();
    const float settings[]={0,.05f,.15f,.3f,.31f,-.1f,.8f,.9f,1,1.01f,.79f,INFINITY,-INFINITY,NAN};
    for(float x:in){
        Check(Same(cpp::StickDeadzone(x),cvr_stick_deadzone(x)),"StickDeadzone differs");
        Check(Same(cpp::StickFullInput(x),cvr_stick_full_input(x)),"StickFullInput differs");
        n+=2;
        for(float d:settings){
            Check(Same(cpp::KeypadCursorAxis(x,d),cvr_keypad_cursor_axis(x,d)),"KeypadCursorAxis differs");++n;
            for(float f:settings){Check(Same(cpp::AnalogAxis(x,d,f),cvr_analog_axis(x,d,f)),"AnalogAxis differs");++n;}
            for(bool a:{false,true})for(bool neg:{false,true}){
                Check(cpp::AtFullTravel(x,a,d,neg)==cvr_at_full_travel(x,a,d,neg),"AtFullTravel differs");++n;
            }
        }
    }
    for(int mode=-5;mode<=5;++mode)for(bool v:{false,true}){
        Check(cpp::UseAnalogMovement(mode,v)==cvr_use_analog_movement(mode,v),"UseAnalogMovement differs");++n;
    }
    const uint64_t stamps[]={0,1,349,350,351,999,1000,1350,1351,~0ull-351,~0ull-350,~0ull};
    for(uint64_t s:stamps)for(uint64_t t:stamps){
        for(int64_t dt:{-1,0,1,349,350,351})
            {Check(cpp::KeypadInputFresh(s,t+dt)==cvr_keypad_input_fresh(s,t+dt),"KeypadInputFresh differs");++n;}
    }
    Rng r;
    auto pick=[&](const std::vector<float>& pool){return pool[r.Next()%pool.size()];};
    const std::vector<float> coords={0,1,50,55,75,300,533,599,600,1065,1066,-5,2,1.5f,NAN,INFINITY,-INFINITY};
    const std::vector<float> sticks={0,.1f,-.1f,.5f,-.5f,.7f,-2,NAN,INFINITY};
    const std::vector<float> dts={0,1.f/30,1.f/90,1.f/144,.02f,.05f,.06f,10,-1,NAN,INFINITY};
    const std::vector<float> trig={0,.2f,.200001f,.5f,.84f,.849f,.85f,.9f,1,-1,2,NAN,INFINITY};
    for(int run=0;run<2000;++run){
        cpp::KeypadCursor a;CvrKeypadCursor b{};
        cpp::WeaponTrigger ta;CvrWeaponTrigger tb{};
        for(int step=0;step<200;++step){
            const bool hit=r.Next()&1;
            const KeypadPoint head{pick(coords),pick(coords)},stick{pick(sticks),pick(sticks)};
            const KeypadPoint size=(r.Next()%16)?KeypadPoint{600,1066}:KeypadPoint{pick(coords),pick(coords)};
            const float dt=pick(dts);
            const bool ra=a.Update(hit,head,stick,size,dt);
            const bool rb=cvr_keypad_cursor_update(&b,hit,{head.x,head.y},{stick.x,stick.y},{size.x,size.y},dt);
            // Field by field: the two bytes of padding after hadHead are not part of the state.
            Check(ra==rb && Same(a.position.x,b.position.x) && Same(a.position.y,b.position.y) &&
                  Same(a.previousHead.x,b.previousHead.x) && Same(a.previousHead.y,b.previousHead.y) &&
                  a.initialized==b.initialized && a.hadHead==b.hadHead,"KeypadCursor diverged");
            const bool en=(r.Next()%8)!=0;const float v=pick(trig);
            const bool da=ta.Update(en,v),db=cvr_weapon_trigger_update(&tb,en,v);
            Check(da==db && ta.active==tb.active && ta.down==tb.down && ta.ready==tb.ready,"WeaponTrigger diverged");
            n+=2;
        }
    }
    return n;
}
}
#endif
int main(int argc,char** argv)try {
    using namespace cvr::input;
    Check(argc==2,"case required");const std::string name=argv[1];
#if CVR_RUST_INPUT
    Check(cvr_rust_abi_version()==CVR_RUST_ABI_VERSION,"cvr_rust.lib ABI differs from include/Rust/CvrRust.h");
    if(name=="parity") {std::cout<<"PASS parity: "<<parity::Run()<<" identical results\n";return 0;}
#endif
    if(name=="weapon_trigger") {
        WeaponTrigger t;
        Check(!t.Update(true,0),"rest fired");
        for(float v:{.2f,.5f,.849f})Check(!t.Update(true,v),"early break");
        Check(t.Update(true,.85f),"break missed");
        for(int i=0;i<100;++i)Check(t.Update(true,1),"automatic hold interrupted");
        for(float v:{.84f,.5f,.201f,.9f})Check(t.Update(true,v),"hold reset early");
        Check(!t.Update(true,.20f),"reset missed");
        for(int i=0;i<100;++i){Check(t.Update(true,.85f),"rapid re-press missed");Check(!t.Update(true,.2f),"rapid reset missed");}
        Check(!t.Update(false,1),"inactive fired");
        Check(!t.Update(true,1) && !t.Update(true,.3f),"reentry with held trigger fired");
        Check(!t.Update(true,.2f) && t.Update(true,.85f),"reentry did not rearm");
        Check(!t.Update(true,std::numeric_limits<float>::quiet_NaN()),"invalid input fired");
        Check(!t.Update(true,1),"tracking recovery auto-fired");
    } else if(name=="keypad_cursor") {
        const KeypadPoint size{600,1066}, centre{300,533};
        for(int fps:{30,45,60,90,120,144}) {
            KeypadCursor cursor;
            Check(!cursor.Update(false,{}, {},size,.02f),"panel without a real hit was claimed");
            Check(cursor.Update(true,centre,{},size,0),"initial head hit failed");
            for(int i=0;i<fps;++i)cursor.Update(false,{}, {.1f,0},size,1.f/fps);
            Check(std::abs(cursor.position.x-390)<.02f,"stick speed depends on FPS or gaze");
            const auto held=cursor.position;
            cursor.Update(false,{}, {},size,.03f);
            Check(cursor.position.x==held.x,"cursor moves after releasing the stick");
            cursor.Update(true,{50,50},{},size,.02f);
            Check(cursor.position.x==held.x,"returning gaze teleports the held cursor");
            cursor.Update(true,{55,57},{},size,.02f);
            Check(std::abs(cursor.position.x-held.x-5)<.001f && std::abs(cursor.position.y-held.y-7)<.001f,"head movement stopped after stick use");
        }
        for(float axis:{-.5f,.5f}) {
            KeypadCursor cursor;cursor.Update(true,centre,{},size,0);
            for(int i=0;i<1000;++i)cursor.Update(false,{}, {axis,axis},size,.02f);
            Check(cursor.position.x>=1 && cursor.position.x<=599 && cursor.position.y>=1 && cursor.position.y<=1065,"cursor escaped panel");
            const auto edge=cursor.position;
            cursor.Update(false,{}, {-axis,-axis},size,.02f);
            Check(std::abs(cursor.position.x-edge.x)>8.9f,"edge overtravel delays reversal");
        }
        KeypadCursor cursor;cursor.Update(true,centre,{},size,0);
        cursor.Update(false,{}, {.5f,0},size,10);
        Check(std::abs(cursor.position.x-322.5f)<.001f,"hitch jumped across keypad");
        const auto before=cursor.position;
        const float nan=std::numeric_limits<float>::quiet_NaN();
        cursor.Update(true,{nan,nan},{nan,nan},size,nan);
        Check(cursor.position.x==before.x && cursor.position.y==before.y,"invalid pose/input moved cursor");
        Check(!cursor.Update(true,centre,{}, {0,1066},.01f) && !cursor.initialized,"invalid/resized panel retains cursor");
        cursor.Update(true,{75,80},{},size,0);
        Check(cursor.position.x==75 && cursor.position.y==80,"next device inherits old cursor");
    } else if(name=="keypad") {
        float previous=0;
        for(int i=0;i<=1000;++i) {
            const float raw=i/1000.f,value=KeypadCursorAxis(raw,.15f);
            Check(value>=previous && value<=.5f,"keypad cursor is not bounded and proportional");
            Check(KeypadCursorAxis(-raw,.15f)==-value,"keypad cursor direction is asymmetric");
            if(raw<=.15f)Check(value==0,"keypad centre drifts");
            previous=value;
        }
        Check(KeypadCursorAxis(.575f,.15f)>.249f && KeypadCursorAxis(.575f,.15f)<.251f,"partial keypad push became full movement");
        Check(KeypadCursorAxis(std::numeric_limits<float>::quiet_NaN(),.15f)==0,"invalid keypad input moves cursor");
        Check(!KeypadInputFresh(0,1000) && KeypadInputFresh(1000,1350) && !KeypadInputFresh(1000,1351),"keypad session does not expire");
        Check(!KeypadInputFresh(1000,999),"clock rollback keeps keypad input claimed");
    } else if(name=="curve") {
        for(float sign:{-1.f,1.f}) {
            Check(AnalogAxis(sign*.14f,.15f,.9f)==0,"centre drift escapes deadzone");
            Check(AnalogAxis(sign*.15f,.15f,.9f)==0,"deadzone boundary moves");
            Check(std::abs(AnalogAxis(sign*.525f,.15f,.9f)-sign*.5f)<1e-6f,"half usable travel must give half input");
            Check(AnalogAxis(sign*.9f,.15f,.9f)==sign,"outer threshold does not give full input");
            Check(AnalogAxis(sign*1.1f,.15f,.9f)==sign,"overshoot exceeds full input");
        }
    } else if(name=="ranges") {
        for(float dead:{0.f,.05f,.15f,.30f})for(float full:{.8f,.9f,1.f}) {
            float previous=-1;
            for(int i=-1000;i<=1000;++i) {
                const float raw=i*.001f,out=AnalogAxis(raw,dead,full);
                Check(std::isfinite(out) && std::abs(out)<=1,"analog output outside range");
                Check(out>=previous,"stick speed is not monotonic");previous=out;
                Check(std::abs(out+AnalogAxis(-raw,dead,full))<1e-6f,"asymmetric forward/back or strafe response");
            }
        }
    } else if(name=="gestures") {
        for(float dead:{0.f,.15f,.30f})for(float full:{.8f,.9f,1.f}) {
            Check(AnalogAxis(full,dead,full)==1,"full travel mapping lost");
            Check(!AtFullTravel(full-.001f,true,full),"partial travel activates sprint/dash");
            Check(AtFullTravel(full,true,full),"full raw threshold missed");
            Check(AtFullTravel(-full,true,full,true),"crouch threshold differs from forward");
            Check(!AtFullTravel(0,true,full),"consumed scanner axis activates sprint");
        }
        Check(!AtFullTravel(.9f,false,.8f) && AtFullTravel(.901f,false,1),"fixed-mode threshold changed");
    } else if(name=="crouch_pulse") {
        // A push held at the stop is one tap of R3; it must end while the stick is still held.
        StickPulse p;uint64_t t=1000;int presses=0;bool was=false;
        auto step=[&](bool stop,float travel,bool allowed=true) {
            const bool on=p.Update(stop,travel,t,100,allowed);presses+=on && !was;was=on;t+=11;return on;
        };
        Check(step(true,1),"push did not press at once");
        for(int i=0;i<200;++i)step(true,1);
        Check(presses==1 && !step(true,1),"held stick kept R3 down or pressed again");
        for(float travel:{.89f,.6f,.5f})step(false,travel);
        Check(!step(true,1) && presses==1,"bounce near the stop pressed again without returning");
        step(false,.2f);Check(step(true,1) && presses==2,"second push after return did not toggle back");
        step(false,0);
        Check(!step(true,1,false),"blocked context pressed");
        for(int i=0;i<20;++i)step(true,1,false);
        Check(!step(true,1) && presses==2,"stick held through a blocked context pressed when it ended");
        step(false,0,false);Check(step(true,1) && presses==3,"push after a blocked context was lost");
        p.Update(true,1,t,100,false);Check(!p.Update(true,1,t+1,100,false),"blocked context kept a pulse in flight");
        StickPulse q;Check(!q.Update(false,std::numeric_limits<float>::quiet_NaN(),0,100,true) && q.armed,"invalid travel left gesture disarmed");
    } else if(name=="invalid") {
        const float nan=std::numeric_limits<float>::quiet_NaN(),inf=std::numeric_limits<float>::infinity();
        for(float value:{nan,inf,-inf}) {
            Check(AnalogAxis(value,.15f,.9f)==0,"invalid axis is not neutral");
            Check(!AtFullTravel(value,true,.9f),"invalid axis activates a gesture");
            Check(StickDeadzone(value)==.15f && StickFullInput(value)==.9f,"invalid settings lack safe defaults");
        }
        Check(StickDeadzone(-.1f)==.15f && StickDeadzone(.31f)==.15f,"deadzone bounds not validated");
        Check(StickFullInput(.79f)==.9f && StickFullInput(1.01f)==.9f,"full threshold bounds not validated");
    } else if(name=="default") {
        Check(!UseAnalogMovement(0,false),"fixed default replaced with analog");
        Check(UseAnalogMovement(1,false),"analog switch has no effect on foot");
        Check(!UseAnalogMovement(1,true),"analog tuning changes driving");
        Check(!UseAnalogMovement(-1,false) && !UseAnalogMovement(2,false),"unknown movement mode accepted");
    } else throw std::runtime_error("unknown case");
    std::cout<<"PASS "<<name<<'\n';return 0;
}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}
