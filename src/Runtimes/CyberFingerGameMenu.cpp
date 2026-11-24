#include "Runtimes/CyberFingerGameMenu.hpp"
#include "Runtimes/CyberFingerMenuPolicy.hpp"
#include "Runtimes/CyberFingerInput.hpp"
#include "Runtimes/CyberFingerUi.hpp"
#include "Render/DesktopMirror.hpp"
#include <windows.h>
#include <mutex>
#include <algorithm>

extern HWND g_gameHwnd;
namespace cvr::cfmenu {
namespace {
std::mutex mutex;
cvr::vrui::Tracking tracking;
Snapshot snapshot;
cvr::cyberfinger::MenuNavigation navigation;
cvr::cyberfinger::MenuFocus focus;
cvr::cyberfinger::NativeMenuButtons nativeButtons;
bool active{},armed[2]{},pressed[2]{},edges[2]{};
int hand{};uint16_t pulse{},heldButtons{};uint64_t pulseUntil{};
bool directional{};
bool desktop{},mouseKnown{},keyboardHints{};int mouseX{},mouseY{};
cvr::cyberfinger::MenuContext context{};
bool grabArmed[2]{},verticalArmed{},anchored{};XrPosef anchor{};uint64_t anchorOrigin{};
bool Foreground(){return g_gameHwnd && GetForegroundWindow()==g_gameHwnd;}
void Clear(){tracking={};snapshot={};navigation={};focus={};nativeButtons={};active=false;hand=0;pulse=heldButtons=0;pulseUntil=0;
    for(int i=0;i<2;++i)armed[i]=pressed[i]=edges[i]=grabArmed[i]=false;
    verticalArmed=false;context={};desktop=mouseKnown=keyboardHints=false;}
}
void Reset(){std::lock_guard lock(mutex);Clear();anchored=false;}
void SuspendInput(){std::lock_guard lock(mutex);const auto pose=tracking;Clear();tracking=pose;}
void DesktopMouse(uint32_t message,int x,int y,uintptr_t extraInfo) {
    std::lock_guard lock(mutex);
    if(!active)return;
    if (message == WM_KEYDOWN || message == WM_SYSKEYDOWN) {
        keyboardHints = true;
        return;
    }
    const bool move=message==WM_MOUSEMOVE;
    const bool click=message==WM_LBUTTONDOWN || message==WM_RBUTTONDOWN ||
        message==WM_LBUTTONDBLCLK || message==WM_RBUTTONDBLCLK;
    const bool wheel=message==WM_MOUSEWHEEL || message==WM_MOUSEHWHEEL;
    if(!move && !click && !wheel)return;
    const bool moved=move && mouseKnown && (std::abs(x-mouseX)>1 || std::abs(y-mouseY)>1);
    if(move){mouseX=x;mouseY=y;mouseKnown=true;}
    if(extraInfo!=PointerTag && (moved || click || wheel) && !snapshot.down && !snapshot.secondary)desktop=true;
}
void Hands(const cvr::vrui::Tracking& value,bool enabled) {
    std::lock_guard lock(mutex);
    // Input focus and menu context can change while the same panel remains
    // open. Keep its anchor until Reset (menu closed) or an origin change.
    if(!enabled || !value.cyberfinger || !value.valid){Clear();tracking=value;return;}
    const auto now=GetTickCount64();
    const auto wanted=cvr::cyberfinger::Context();
    if(!active || now-snapshot.tick>250 || context!=wanted){Clear();active=true;hand=value.preferLeft?0:1;context=wanted;}
    const bool map=context==cvr::cyberfinger::MenuContext::Map;
    const bool journal=context==cvr::cyberfinger::MenuContext::Journal;
    if(map || journal)hand=value.preferLeft?0:1;
    tracking=value;snapshot.active=true;snapshot.tick=now;snapshot.head=value.head.position;
    const bool list=context==cvr::cyberfinger::MenuContext::General;
    const auto& right=value.hands[1];
    const auto& stick=map?value.hands[hand]:right;
    const bool validStick=stick.valid && std::isfinite(stick.stickX) && std::isfinite(stick.stickY);
    if (!validStick) verticalArmed=false;
    else if (std::abs(stick.stickY)<.3f && (!list || std::abs(stick.stickX)<.3f)) verticalArmed=true;
    const bool vertical=validStick && verticalArmed && std::abs(stick.stickY)>.4f;
    const bool alongList=list && validStick && std::abs(stick.stickY)>std::abs(stick.stickX);
    snapshot.zoom=map && vertical ? stick.stickY : 0.f;
    snapshot.scroll=alongList && vertical ? stick.stickY : 0.f;
    const float tabX=(list && (!validStick || !verticalArmed || alongList)) ? 0.f : right.stickX;
    const auto steps=navigation.Step(true,map?0:value.hands[0].stickX,map?0:value.hands[0].stickY,tabX,
        false,false,now);
    const auto actions=nativeButtons.Step(value.menuLeft,value.menuRight,
        value.hands[0].grip>.5f,value.hands[1].grip>.5f);
    for(int i=0;i<2;++i)if(value.hands[i].valid && value.hands[i].grip<.3f)grabArmed[i]=true;
    const bool ownGrab=grabArmed[hand] && value.hands[hand].valid && value.hands[hand].grip>.5f;
    const bool otherGrab=grabArmed[1-hand] && value.hands[1-hand].valid && value.hands[1-hand].grip>.5f;
    heldButtons=map?(otherGrab?0x8000:0):((ownGrab || otherGrab)?0x4000:0);
    if(context==cvr::cyberfinger::MenuContext::PictureFrame && heldButtons)heldButtons=0x8000;
    snapshot.secondary=map && ownGrab;
    directional=steps.direction!=0;
    const uint16_t buttons=cvr::cyberfinger::DirectionButton(steps.direction) |
        (steps.tab<0?0x0100:steps.tab>0?0x0200:0) | (actions&0x2000);
    if(buttons){pulse=buttons;pulseUntil=now+70;}
    snapshot.buttons=(now<pulseUntil?pulse:0)|heldButtons;
    for(int i=0;i<2;++i) {
        if(!value.valid || !value.hands[i].valid){armed[i]=pressed[i]=edges[i]=grabArmed[i]=false;continue;}
        const auto trigger=value.hands[i].trigger;
        if(trigger<.3f)armed[i]=true;
        const bool down=trigger>(pressed[i]?.35f:.65f);
        edges[i]=armed[i] && down && !pressed[i];pressed[i]=down;
    }
    if(!value.valid){snapshot.ray=snapshot.hit=snapshot.down=snapshot.secondary=false;heldButtons=snapshot.buttons=0;}
}
XrPosef PanelPose(const XrPosef& pose,uint64_t origin,const cvr::vrui::Settings& settings) {
    std::lock_guard lock(mutex);
    // Native mod menus use the same ink canvas as stock menus. Anchor every
    // active CF modal canvas, not only the apartment picture frame, so head
    // motion cannot move the laser's target. Other profiles keep their pose.
    if(!tracking.cyberfinger){anchored=false;return pose;}
    if((!anchored || anchorOrigin!=origin) && tracking.valid){
        anchor=cvr::vrui::OpeningPose(tracking.head,settings);anchored=true;anchorOrigin=origin;
    }
    if(!anchored)return pose;
    return anchor;
}
XrExtent2Df PanelSize(XrExtent2Df legacy,const cvr::vrui::Settings& settings) {
    std::lock_guard lock(mutex);
    return tracking.cyberfinger?cvr::vrui::PanelBounds(legacy.height>0?legacy.width/legacy.height:0,settings):legacy;
}
void Panel(const XrPosef& pose,float width,float height) {
    std::lock_guard lock(mutex);
    if(!active)return;
    const auto now=GetTickCount64();
    cvr::vrui::Hit hits[2];
    for(int i=0;i<2;++i)if(tracking.valid && tracking.hands[i].valid)
        hits[i]=cvr::vrui::Intersect(tracking.hands[i].aim,pose,width,height);
    const bool special=context==cvr::cyberfinger::MenuContext::Map || context==cvr::cyberfinger::MenuContext::Journal;
    if(!special && !snapshot.down && !snapshot.focus) {
        const int other=1-hand;
        if(hits[other].valid && edges[other])hand=other;
    }
    const auto& hit=hits[hand];
    if(directional || edges[hand] || edges[1-hand])desktop=false;
    const float u=hit.valid?hit.x/cvr::vrui::CanvasWidth:snapshot.u;
    const float v=hit.valid?hit.y/cvr::vrui::CanvasHeight:snapshot.v;
    snapshot.focus=focus.Step(directional,hit.valid,u,v);
    snapshot.hit=hit.valid && !snapshot.focus;
    // Without a ray target, A still activates the game's highlighted item.
    // With a ray target, the same trigger clicks it: never send A and a mouse
    // click together, which could select twice or confirm the next dialog.
    if(snapshot.focus || !snapshot.hit) {
        snapshot.down=false;
        if(edges[hand] || (!special && edges[1-hand])) {
            pulse=context==cvr::cyberfinger::MenuContext::PictureFrame?0x4000:0x1000;pulseUntil=now+70;
        }
        snapshot.buttons=(now<pulseUntil?pulse:0)|heldButtons;
    } else snapshot.down=snapshot.hit && armed[hand] && pressed[hand] && (snapshot.down || edges[hand]);
    if(special && edges[1-hand]) {
        pulse=context==cvr::cyberfinger::MenuContext::Map?0x0040:0x0080;pulseUntil=now+70;
    }
    snapshot.buttons=(now<pulseUntil?pulse:0)|heldButtons;
    snapshot.secondary=snapshot.secondary && snapshot.hit;
    if (directional || edges[0] || edges[1] || snapshot.buttons || snapshot.secondary || snapshot.zoom != 0 || snapshot.scroll != 0) keyboardHints=false;
    if(desktop){snapshot.hit=snapshot.down=snapshot.secondary=false;}
    // The beam and cursor share the exact displayed quad intersection. Keep a
    // short aiming ray off the canvas too, but yield visuals with input when
    // desktop mouse or joystick focus owns the menu. Tracking loss never
    // transfers the beam to the other hand implicitly.
    const auto& pointer=tracking.hands[hand];
    snapshot.ray=tracking.valid && pointer.valid && !desktop && !snapshot.focus;
    snapshot.u=u;snapshot.v=v;snapshot.start=pointer.aim.position;
    snapshot.end=hit.valid?hit.point:cvr::vrui::Add(snapshot.start,RotateVector(pointer.aim.orientation,{0,0,-1.4f}));
    // An edge is consumed by this frame's panel, not replayed by a repeated image.
    directional=false;edges[0]=edges[1]=false;
}
Snapshot Current() {
    std::lock_guard lock(mutex);
    if(!cvr::cyberfinger::Enabled() || GetTickCount64()-snapshot.tick>250)return {};
    auto value = snapshot;
    value.desktop = desktop || keyboardHints;
    return value;
}
bool Cursor(float& u,float& v) {
    {std::lock_guard lock(mutex);if(desktop)return false;}
    const auto state=Current();
    if(!state.active || !state.hit || !Foreground())return false;
    u=state.u;v=state.v;return true;
}
void DispatchPointer() {
    // XInput is polled on the game thread. Serialize other callers too, so
    // each button edge is inserted once even if a plugin polls concurrently.
    static std::mutex delivery;
    std::lock_guard lock(delivery);
    static bool mouseDown=false,secondaryDown=false;
    static float previousU=-1,previousV=-1;
    const auto state=Current();
    const bool owns=state.active && state.hit && Foreground();
    bool down=owns && state.down;
    bool positioned=owns && state.u==previousU && state.v==previousV;
    if(owns && (state.u!=previousU || state.v!=previousV || down!=mouseDown || state.secondary!=secondaryDown)) {
        RECT client{};POINT origin{};
        if(GetDesktopPhysicalClientRect(g_gameHwnd,client,origin) && client.right>2 && client.bottom>2) {
            // Keep the real cursor inside this window. Its virtual position is
            // supplied by the game's cursor/message hooks, independent of crop.
            const LONG x=origin.x+std::clamp(LONG(state.u*client.right),1L,client.right-2);
            const LONG y=origin.y+std::clamp(LONG(state.v*client.bottom),1L,client.bottom-2);
            static const auto metrics=reinterpret_cast<int(WINAPI*)(int)>(GetProcAddress(GetModuleHandleW(L"user32.dll"),"GetSystemMetrics"));
            const int left=metrics(SM_XVIRTUALSCREEN),top=metrics(SM_YVIRTUALSCREEN);
            const int w=metrics(SM_CXVIRTUALSCREEN),h=metrics(SM_CYVIRTUALSCREEN);
            if(w>1 && h>1 && Foreground()) {
                INPUT event{};event.type=INPUT_MOUSE;
                event.mi.dwExtraInfo=PointerTag;
                event.mi.dx=LONG(double(x-left)*65535/(w-1));event.mi.dy=LONG(double(y-top)*65535/(h-1));
                event.mi.dwFlags=MOUSEEVENTF_ABSOLUTE|MOUSEEVENTF_VIRTUALDESK|MOUSEEVENTF_MOVE;
                if(SendInput(1,&event,sizeof(event))==1){previousU=state.u;previousV=state.v;positioned=true;}
            }
        }
    }
    // Never click an old desktop location if window mapping or movement failed.
    down=down && positioned && Foreground();
    if(down!=mouseDown) {
        INPUT event{};event.type=INPUT_MOUSE;event.mi.dwFlags=down?MOUSEEVENTF_LEFTDOWN:MOUSEEVENTF_LEFTUP;
        event.mi.dwExtraInfo=PointerTag;
        if(SendInput(1,&event,sizeof(event))==1)mouseDown=down;
    }
    const bool secondary=owns && positioned && state.secondary && Foreground();
    if(secondary!=secondaryDown) {
        INPUT event{};event.type=INPUT_MOUSE;event.mi.dwFlags=secondary?MOUSEEVENTF_RIGHTDOWN:MOUSEEVENTF_RIGHTUP;
        event.mi.dwExtraInfo=PointerTag;
        if(SendInput(1,&event,sizeof(event))==1)secondaryDown=secondary;
    }
    if(!owns)previousU=previousV=-1;
}
}
