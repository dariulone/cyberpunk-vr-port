#include "Overlay/VrOverlay.hpp"
#include "Runtimes/FingerGrip.hpp"
#include "Runtimes/CyberFingerMenuPolicy.hpp"
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
cvr::cyberfinger::HudChord cfHudChord;
cvr::cyberfinger::ButtonEdge cfWristMenu;
bool hudToggle{};
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
cvr::cyberfinger::MenuNavigation navigation;
cvr::cyberfinger::MenuFocus keyboardFocus;
cvr::cyberfinger::MenuBackButton menuBack;
int tabStep{};
bool cfWasVisible{};
bool cfTriggerArmed[2]{},cfTriggerDown[2]{};
bool secondaryDown{};
void ReleasePointer(){events.clear();events.push_back({-1,-1,0,false});buttonDown=secondaryDown=false;triggerArmed=false;}
void ToggleLocked(){wanted=!wanted;command=wanted?1:2;requestStamp=GetTickCount64();visible=false;capture=true;releasePending=true;placement.Reset();ReleasePointer();}
}
Settings GetSettings(){std::lock_guard lock(mutex);return settings;}
void SetSettings(Settings s){std::lock_guard lock(mutex);settings=Sanitize(s);}
bool Visible(){return visible.load(std::memory_order_relaxed);}
bool CapturesInput(){return capture.load(std::memory_order_relaxed);}
void Toggle(){std::lock_guard lock(mutex);ToggleLocked();}
bool ConsumeMenuSummon(){std::lock_guard lock(mutex);const bool s=menuSummon;menuSummon=false;return s;}
bool ConsumeHudToggle(){std::lock_guard lock(mutex);const bool value=hudToggle;hudToggle=false;return value;}
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
int ConsumeTabStep(){std::lock_guard lock(mutex);const int step=tabStep;tabStep=0;return step;}
void UpdateTracking(const Tracking& t){
    std::lock_guard lock(mutex);clock.Step(t.time);
    view.cyberfinger=t.cyberfinger;
    const bool both=t.valid && (t.cyberfinger ? (t.menuLeft && t.menuRight) :
        (t.hands[0].valid && t.hands[1].valid && t.hands[0].stickClick && t.hands[1].stickClick));
    // CF: right pink wrist opens the world menu; both Menu buttons toggle HUD.
    // Other controllers keep their existing stick-click summon chord.
    if(chord.Update(!t.cyberfinger && both,clock.elapsed)){menuSummon=true;releasePending=true;}
    // Physical Menu buttons do not require both optical hand poses to be visible.
    const bool cfValid=t.cyberfinger && t.valid;
    if(cfHudChord.Step(t.menuLeft,t.menuRight,cfValid,clock.elapsed)){hudToggle=true;releasePending=true;}
    if(!t.cyberfinger || !t.valid || !t.hands[1].valid)cfWristMenu.Reset();
    else if(cfWristMenu.Step(t.menuButton)){menuSummon=true;releasePending=true;}
    const auto now=GetTickCount64();
    // The idle Lua bridge only polls commands, so its last acknowledgement may
    // be arbitrarily old while closed. Give each new request its opening window;
    // require a live heartbeat only after that request has made the panel visible.
    if(wanted && ((visible && bridgeStamp && now-bridgeStamp>1500) || (!visible && now-requestStamp>3000))){wanted=false;visible=false;ReleasePointer();}
    // A grip that stays above half (a resting hand on Index controllers, or a grip held through the whole menu) kept this
    // pending forever, and with it the capture that zeroes all controller input for the game. Grips still have to let go
    // first, but only for 1.5 s; sticks and triggers as before. With the grip read from the fingers a hand closed on the
    // controller reads as gripping for as long as it holds it: the grips are not waited for at all.
    static uint64_t pendingSince=0;
    if(!releasePending)pendingSince=0;else if(!pendingSince)pendingSince=now;
    const bool gripsFree=cvr::fingergrip::GripFromFingers() || (t.hands[0].grip<.5f && t.hands[1].grip<.5f) ||
                         (pendingSince && now-pendingSince>1500);
    const bool summonReleased=t.cyberfinger?(!t.menuLeft && !t.menuRight && !t.menuButton):!both;
    if(releasePending && summonReleased && t.hands[0].trigger<.3f && t.hands[1].trigger<.3f && gripsFree)releasePending=false;
    capture=visible || wanted || both || cfHudChord.Claimed() || releasePending;
    view.hold=t.cyberfinger?cfHudChord.Held():float(chord.held);
    const auto nav=navigation.Step(t.cyberfinger && visible,t.hands[0].stickX,t.hands[0].stickY,t.hands[1].stickX,
        t.hands[0].grip>.5f,t.hands[1].grip>.5f,now);
    if(!visible){view.valid=false;cfWasVisible=false;tabStep=0;keyboardFocus.Reset();menuBack={};return;}
    if(!t.cyberfinger)menuBack={};
    const bool back=t.cyberfinger && menuBack.Step(t.menuLeft,t.menuRight);
    if(t.cyberfinger && !cfWasVisible){pointerHand=t.preferLeft?0:1;ReleasePointer();previousTrigger=false;
        for(int h=0;h<2;++h)cfTriggerArmed[h]=cfTriggerDown[h]=false;}
    cfWasVisible=t.cyberfinger;
    bool cfEdge[2]{};
    if(t.cyberfinger)for(int h=0;h<2;++h){
        const auto value=t.hands[h].trigger;
        if(value<.3f)cfTriggerArmed[h]=true;
        const bool pressed=value>(cfTriggerDown[h]?.35f:.65f);
        cfEdge[h]=cfTriggerArmed[h] && pressed && !cfTriggerDown[h];cfTriggerDown[h]=pressed;
    }
    if(t.cyberfinger)tabStep=std::clamp(tabStep+nav.tab,-1,1);
    const bool wasDragging=placement.dragging>=0;
    placement.Update(t,settings,clock.elapsed);
    if(wasDragging && placement.dragging<0)placementSave=true;
    view.pose=placement.pose;view.width=settings.width*settings.scale;view.height=view.width*CanvasHeight/CanvasWidth;
    view.headPosition=t.head.position;view.cyberfinger=t.cyberfinger;
    view.valid=placement.valid;view.hit={};view.ray=false;view.dragging=placement.dragging>=0;
    const bool automatic=t.cyberfinger || settings.hand==2;
    int hand=placement.dragging>=0?placement.dragging:(automatic?pointerHand:settings.hand);
    auto hit=[&](int h){return t.valid && t.hands[h].valid?Intersect(t.hands[h].aim,view.pose,view.width,view.height):Hit{};};
    view.hit=hit(hand);
    if(automatic && placement.dragging<0 && !buttonDown && !secondaryDown && !(t.cyberfinger && keyboardFocus.Active()) &&
        (t.cyberfinger?(!t.hands[hand].valid || cfEdge[1-hand] || (nav.grabMask&(1u<<(1-hand)))):!view.hit.valid) && hit(1-hand).valid){hand=1-hand;view.hit=hit(hand);}
    if(hand!=pointerHand){ReleasePointer();previousTrigger=false;pointerHand=hand;}
    const auto& input=t.hands[hand];
    const bool handle=view.hit.valid && (!t.cyberfinger || (view.hit.y<70 && view.hit.x<1160));
    const bool grabStart=input.grip>.7f && !gripHeld[hand] && handle && !buttonDown &&
        (!t.cyberfinger || (nav.grabMask&(1u<<hand)));
    if(back) {
        if(placement.dragging>=0){placement.dragging=-1;placementSave=true;}
        ReleasePointer();PointerEvent event;event.back=true;events.push_back(event);return;
    }
    if((!t.valid || !input.valid) && !t.cyberfinger){ReleasePointer();events.push_back({});return;}
    if(t.cyberfinger && (!t.valid || !input.valid)){buttonDown=false;cfTriggerArmed[hand]=false;}
    if(input.trigger<.3f)triggerArmed=true;
    if(t.cyberfinger)triggerArmed=cfTriggerArmed[hand];
    const bool pressed=input.trigger>(previousTrigger?.35f:.65f);
    const bool focus=t.cyberfinger && keyboardFocus.Step(nav.direction!=0,view.hit.valid,view.hit.x/CanvasWidth,view.hit.y/CanvasHeight);
    const bool activate=focus && (cfEdge[0] || cfEdge[1]);
    const bool down=!focus && triggerArmed && pressed && (!t.cyberfinger || (t.valid && input.valid && input.grip<.5f)) && (buttonDown ||
        ((t.cyberfinger?cfEdge[hand]:!previousTrigger) && view.hit.valid)) && placement.dragging<0;
    previousTrigger=pressed;
    float wheel=0;
    // CF: only the title strip moves the panel; elsewhere Grip is right-click.
    if(grabStart){placement.Grab(hand,t);view.dragging=true;}
    secondaryDown=t.cyberfinger && !focus && view.hit.valid && t.valid && input.valid &&
        !down && !handle && placement.dragging<0 && input.grip>.5f &&
        (secondaryDown || (nav.grabMask&(1u<<hand)));
    const float scroll=t.cyberfinger?t.hands[1].stickY:input.stickY;
    if(view.hit.valid && !down && input.grip<.5f && placement.dragging<0 && std::abs(scroll)>.2f)
        wheel=scroll*std::clamp(clock.elapsed,0.0f,.05f)*5;
    for(int h=0;h<2;++h)gripHeld[h]=t.hands[h].grip>.5f;
    buttonDown=down;
    if(events.size()<64)events.push_back({view.hit.valid?view.hit.x:-1,view.hit.valid?view.hit.y:-1,wheel,down,nav.direction,activate,focus,secondaryDown});
    else {events.clear();ReleasePointer();events.push_back({});}
    view.ray=view.hit.valid && !focus;view.rayStart=input.aim.position;view.rayEnd=view.hit.point;
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
