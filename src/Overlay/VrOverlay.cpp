#include "Overlay/VrOverlay.hpp"
#include "Runtimes/FingerGrip.hpp"
#include <windows.h>
#include <atomic>
#include <mutex>
#include <cstdio>
#include <sstream>

namespace cvr::vrui {
namespace {
std::mutex mutex;
Settings settings;
Placement placement;
HoldChord chord;
cvr::hud::DisplayClock clock;
std::atomic<bool> visible{false},capture{false};
int command=0;bool wanted=false,buttonDown=false,triggerArmed=false,releasePending=false,gripHeld[2]{},menuSummon=false;
uint64_t bridgeStamp=0,requestStamp=0;
std::vector<std::string> items;
std::vector<PointerEvent> events;
View view;
bool placementSave=false;
int pointerHand=1;
bool previousTrigger=false;
void ReleasePointer(){events.clear();events.push_back({-1,-1,0,false});buttonDown=false;triggerArmed=false;}
void ToggleLocked(){wanted=!wanted;command=wanted?1:2;requestStamp=GetTickCount64();visible=false;capture=true;releasePending=true;placement.Reset();ReleasePointer();}
}
Settings GetSettings(){std::lock_guard lock(mutex);return settings;}
void SetSettings(Settings s){std::lock_guard lock(mutex);settings=Sanitize(s);}
bool Visible(){return visible.load(std::memory_order_relaxed);}
bool CapturesInput(){return capture.load(std::memory_order_relaxed);}
void Toggle(){std::lock_guard lock(mutex);ToggleLocked();}
bool ConsumeMenuSummon(){std::lock_guard lock(mutex);const bool s=menuSummon;menuSummon=false;return s;}
void Close(bool resume){std::lock_guard lock(mutex);wanted=false;visible=false;capture=true;releasePending=true;if(resume)command=2;ReleasePointer();}
void Recenter(){std::lock_guard lock(mutex);settings.offsetX=settings.offsetY=0;placement.Reset();}
bool ConsumePlacementSave(){std::lock_guard lock(mutex);const bool save=placementSave;placementSave=false;return save;}
void SelectMenuItem(unsigned i){std::lock_guard lock(mutex);if(i>=items.size() || !visible)return;command=100+int(i);wanted=false;visible=false;capture=true;releasePending=true;ReleasePointer();}
int PollCommand(){std::lock_guard lock(mutex);const int c=command;command=0;return c;}
void BridgeUpdate(bool ready,bool inGame,const char* labels){
    std::lock_guard lock(mutex);bridgeStamp=GetTickCount64();
    items.clear();std::istringstream in(labels?labels:"");std::string line;
    while(items.size()<16 && std::getline(in,line))if(!line.empty())items.push_back(line.substr(0,240));
    if(wanted && ready && inGame && !items.empty()) {visible=true;capture=true;}
    else if(visible && (!ready || !inGame)){wanted=false;visible=false;capture=false;ReleasePointer();}
}
std::vector<std::string> MenuItems(){std::lock_guard lock(mutex);return items;}
std::vector<PointerEvent> ConsumePointerEvents(){std::lock_guard lock(mutex);std::vector<PointerEvent> out;out.swap(events);return out;}
View GetView(){std::lock_guard lock(mutex);return view;}
void UpdateTracking(const Tracking& t){
    std::lock_guard lock(mutex);clock.Step(t.time);
    const bool both=t.valid && t.hands[0].valid && t.hands[1].valid && t.hands[0].stickClick && t.hands[1].stickClick;
    // the sticks are held back from the game until they are let go, as after a panel toggle
    if(chord.Update(both,clock.elapsed)){menuSummon=true;releasePending=true;}
    const auto now=GetTickCount64();
    // The idle Lua bridge only polls commands, so its last acknowledgement may
    // be arbitrarily old while closed. Give each new request its opening window;
    // require a live heartbeat only after that request has made the panel visible.
    if(wanted && ((visible && bridgeStamp && now-bridgeStamp>1500) || (!visible && now-requestStamp>3000))){wanted=false;visible=false;ReleasePointer();}
    // With the grip read from the fingers a hand closed on the controller reads as gripping for as long as it holds it:
    // waiting for that grip to open held every button and stick back from the game and the VR menu.
    const bool gripsOpen=cvr::fingergrip::GripFromFingers() || (t.hands[0].grip<.5f && t.hands[1].grip<.5f);
    if(releasePending && !both && t.hands[0].trigger<.3f && t.hands[1].trigger<.3f && gripsOpen)releasePending=false;
    capture=visible || wanted || both || releasePending;
    view.hold=float(chord.held);
    if(!visible){view.valid=false;return;}
    const bool wasDragging=placement.dragging>=0;
    placement.Update(t,settings,clock.elapsed);
    if(wasDragging && placement.dragging<0)placementSave=true;
    view.pose=placement.pose;view.width=settings.width*settings.scale;view.height=view.width*CanvasHeight/CanvasWidth;
    view.headPosition=t.head.position;
    view.valid=placement.valid;view.hit={};view.ray=false;view.dragging=placement.dragging>=0;
    int hand=placement.dragging>=0?placement.dragging:(settings.hand==2?pointerHand:settings.hand);
    auto hit=[&](int h){return t.valid && t.hands[h].valid?Intersect(t.hands[h].aim,view.pose,view.width,view.height):Hit{};};
    view.hit=hit(hand);
    if(settings.hand==2 && placement.dragging<0 && !buttonDown && !view.hit.valid && hit(1-hand).valid){hand=1-hand;view.hit=hit(hand);}
    if(hand!=pointerHand){ReleasePointer();previousTrigger=false;pointerHand=hand;}
    const auto& input=t.hands[hand];
    if(!t.valid || !input.valid){ReleasePointer();events.push_back({});return;}
    if(input.trigger<.3f)triggerArmed=true;
    const bool pressed=input.trigger>(previousTrigger?.35f:.65f);
    const bool down=triggerArmed && pressed && (buttonDown || (!previousTrigger && view.hit.valid)) && placement.dragging<0;
    previousTrigger=pressed;
    float wheel=0;
    if(input.grip>.7f && !gripHeld[hand] && view.hit.valid && !buttonDown){placement.Grab(hand,t);view.dragging=true;}
    if(view.hit.valid && !down && input.grip<.5f && placement.dragging<0 && std::abs(input.stickY)>.2f)
        wheel=input.stickY*std::clamp(clock.elapsed,0.0f,.05f)*5;
    for(int h=0;h<2;++h)gripHeld[h]=t.hands[h].grip>.5f;
    buttonDown=down;
    if(events.size()<64)events.push_back({view.hit.valid?view.hit.x:-1,view.hit.valid?view.hit.y:-1,wheel,down});
    else {events.clear();ReleasePointer();events.push_back({});}
    view.ray=view.hit.valid;view.rayStart=input.aim.position;view.rayEnd=view.hit.point;
}
bool ParseSetting(const char* line,Settings& s){float v{};int n{};
    if(sscanf_s(line,"xr_overlay_distance = %f",&v)==1)s.distance=v;
    else if(sscanf_s(line,"xr_overlay_min_distance = %f",&v)==1)s.minDistance=v;
    else if(sscanf_s(line,"xr_overlay_max_distance = %f",&v)==1)s.maxDistance=v;
    else if(sscanf_s(line,"xr_overlay_width = %f",&v)==1)s.width=v;
    else if(sscanf_s(line,"xr_overlay_scale = %f",&v)==1)s.scale=v;
    else if(sscanf_s(line,"xr_overlay_font_scale = %f",&v)==1)s.fontScale=v;
    else if(sscanf_s(line,"xr_overlay_cone = %f",&v)==1)s.cone=v;
    else if(sscanf_s(line,"xr_overlay_offset_x = %f",&v)==1)s.offsetX=v;
    else if(sscanf_s(line,"xr_overlay_offset_y = %f",&v)==1)s.offsetY=v;
    else if(sscanf_s(line,"xr_overlay_depth_speed = %f",&v)==1)s.depthSpeed=v;
    else if(sscanf_s(line,"xr_overlay_hand = %d",&n)==1)s.hand=n;
    else if(sscanf_s(line,"xr_overlay_follow = %d",&n)==1)s.follow=n!=0;
    else return false;return true;
}
void WriteSettings(void* file,const Settings& s){std::fprintf(static_cast<FILE*>(file),
    "xr_overlay_distance=%.3f\nxr_overlay_min_distance=%.3f\nxr_overlay_max_distance=%.3f\nxr_overlay_width=%.3f\nxr_overlay_scale=%.3f\nxr_overlay_font_scale=%.3f\nxr_overlay_cone=%.2f\nxr_overlay_offset_x=%.3f\nxr_overlay_offset_y=%.3f\nxr_overlay_depth_speed=%.3f\nxr_overlay_hand=%d\nxr_overlay_follow=%d\n",
    s.distance,s.minDistance,s.maxDistance,s.width,s.scale,s.fontScale,s.cone,s.offsetX,s.offsetY,s.depthSpeed,s.hand,s.follow);}
}
