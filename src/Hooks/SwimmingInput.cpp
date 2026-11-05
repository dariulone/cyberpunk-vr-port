#include "Hooks/SwimmingInput.hpp"
#include "Hooks/LadderInput.hpp"
#include "Hooks/RoomscaleMove.hpp"
#include "Runtimes/SwimmingGesture.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include "Core/LiveControls.hpp"
#include "Core/VrCoreShared.hpp"
#include "Camera/CameraState.hpp"
#include "Overlay/ImGuiOverlay.hpp"
#include "Hooks/Hook.hpp"
#include <MinHook.h>
#include <windows.h>
#include <cstring>
#include <mutex>

extern "C" {
void* CvrSwimmingActionOriginal{};
float CvrSwimmingActionDetour(void*,uint64_t);
}

namespace cvr::swimming {
namespace {
std::mutex s_mutex;
uintptr_t s_player{};
uint64_t s_stamp{},s_strokes{};
int s_state=-1,s_fast=-1;
Breaststroke s_gesture;
AscendStroke s_ascent;
SprintButton s_sprint;
Input s_input{};
bool s_button{},s_tracking{};
bool s_automatic{},s_paused{},s_waterContext{};
bool s_manual{},s_manualSprint{};
XrVector3f s_direction{};
uint64_t s_inputStamp{},s_ascents{};
uintptr_t s_inputObject{},s_inputOwner{};
uint64_t s_inputOwnerStamp{};
using ActionFn=float(__fastcall*)(void*,uint64_t);
float OriginalAction(void* object,uint64_t name) {
    return reinterpret_cast<ActionFn>(CvrSwimmingActionOriginal)(object,name);
}
constexpr uint64_t Hash(const char* text) {
    uint64_t hash=14695981039346656037ull;
    while(*text) { hash^=uint8_t(*text++);hash*=1099511628211ull; }
    return hash;
}
enum Action { Forward=0,Boost=1,Ascend=2,Dive=3 };
bool ContextAllowed() {
    const int tier=g_sceneTier.load(std::memory_order_relaxed);
    return g_menuModeValue==0 && !cvr::ladder::Active() &&
        !g_isInVehicle && !g_bdActive.load(std::memory_order_relaxed) && !DeviceCamActive() &&
        !g_uiPopupOpen.load(std::memory_order_relaxed) && !OverlayIsVisible() && tier>0 && tier<4;
}
bool Fresh(uint64_t now) { return StateFresh(s_player,cvr::roomscale::PlayerIdentity(),s_stamp,now,s_state); }
bool Present(uint64_t now) { return PresenceFresh(s_player,cvr::roomscale::PlayerIdentity(),s_stamp,now,s_state,s_waterContext); }
void Clear() {
    s_gesture.Reset();s_ascent.Reset();s_sprint.Reset();s_input={};
    s_button=false;s_tracking=false;s_automatic=false;s_manual=false;s_manualSprint=false;s_inputStamp=0;
}
}

bool Active() {
    std::lock_guard lock(s_mutex);
    return Present(XrDiagNowUs());
}
// This is the same context getter the original locomotion input sampler has
// just called. Bind only a context whose provider belongs to GetPlayer().
static uintptr_t PlayerInputObject(void* locomotion) {
    __try {
        if(!locomotion)return 0;
        const auto wrapper=static_cast<uint8_t*>(locomotion)+0x30;
        const auto table=*reinterpret_cast<uintptr_t*>(wrapper);
        if(!table)return 0;
        const auto getter=*reinterpret_cast<uintptr_t*>(table);
        if(!getter)return 0;
        const auto context=reinterpret_cast<uintptr_t(__fastcall*)(void*)>(getter)(wrapper);
        if(!context || !cvr::roomscale::IsPlayerProvider(*reinterpret_cast<void**>(context+8)))return 0;
        return *reinterpret_cast<uintptr_t*>(context+0x10);
    } __except(EXCEPTION_EXECUTE_HANDLER) { return 0; }
}
void BindNativeInput(void* locomotion) {
    if(!Active() && !cvr::ladder::Active())return;
    const auto input=PlayerInputObject(locomotion);
    if(!input)return;
    std::lock_guard lock(s_mutex);
    s_inputObject=input;s_inputOwner=cvr::roomscale::PlayerIdentity();s_inputOwnerStamp=XrDiagNowUs();
}
float NativeAction(int action) {
    if(!ContextAllowed())return 0;
    std::lock_guard lock(s_mutex);const auto now=XrDiagNowUs();
    if(s_paused || !Fresh(now) || !s_inputStamp || now<s_inputStamp || now-s_inputStamp>100000)return 0;
    if(s_manual && action!=Ascend)return 0;
    switch(action) {
    case Forward:return ForwardAction(s_input.forward,s_input.boost,s_fast,s_manualSprint);
    case Boost:return s_input.boost && s_input.forward>.8f ? 1.0f:0.0f;
    case Ascend:return s_input.ascend>.1f ? 1.0f:0.0f;
    case Dive:return s_input.dive ? 1.0f:0.0f;
    default:return 0;
    }
}
extern "C" float CvrSwimmingActionAfter(void* object,uint64_t name,float original) {
    if(name!=Hash("MoveY") && name!=Hash("ToggleSprint") && name!=Hash("Jump") && name!=Hash("Dive"))return original;
    {
        std::lock_guard lock(s_mutex);const auto now=XrDiagNowUs();
        if(reinterpret_cast<uintptr_t>(object)!=s_inputObject || s_inputOwner!=cvr::roomscale::PlayerIdentity() ||
           now<s_inputOwnerStamp || now-s_inputOwnerStamp>250000)return original;
    }
    const float x=OriginalAction(object,Hash("MoveX")),y=OriginalAction(object,Hash("MoveY"));
    if(cvr::ladder::Active())return name==Hash("MoveY") ? cvr::ladder::MixMove(original,x,y):original;
    if(y<-.2f) { ResetInput();return original; }
    const bool manual=std::abs(x)>.15f || std::abs(y)>.15f;
    const bool manualSprint=OriginalAction(object,Hash("ToggleSprint"))>0 || OriginalAction(object,Hash("Sprint"))>0;
    { std::lock_guard lock(s_mutex);s_manual=manual;s_manualSprint=manualSprint; }
    const int action=name==Hash("MoveY") ? Forward : name==Hash("ToggleSprint") ? Boost : name==Hash("Jump") ? Ascend:Dive;
    const float value=(manual && (action==Forward || action==Boost || action==Dive)) ? 0:NativeAction(action);
    if(action==Forward) { std::lock_guard lock(s_mutex);s_automatic=value>0; }
    return value>0 ? std::max(original,value):original;
}
bool ReadSteering(XrVector3f* direction,int* state) {
    const bool context=ContextAllowed();
    std::lock_guard lock(s_mutex);
    const auto now=XrDiagNowUs();
    if(!context || s_paused || !Fresh(now) || !s_automatic || !s_inputStamp || now<s_inputStamp ||
       now-s_inputStamp>100000 || !direction || !state)return false;
    *direction=s_direction;*state=s_state;return true;
}

void PublishState(uintptr_t player,int state,int fast,bool paused,bool waterContext) {
    std::lock_guard lock(s_mutex);
    if(player!=s_player || (!InWater(state) && !waterContext) || paused)Clear();
    s_paused=paused;s_waterContext=waterContext;
    s_player=player;s_state=state;s_fast=std::clamp(fast,-1,1);s_stamp=XrDiagNowUs();
}
void ResetInput() { std::lock_guard lock(s_mutex);Clear(); }

Input UpdateInput(const VRControllerState& controllers,bool manualBack) {
    const bool context=ContextAllowed();
    {
        std::lock_guard lock(s_mutex);
        if(!context || s_paused || !Present(XrDiagNowUs())) { Clear();return {}; }
    }
    OpenXRHeadPose head{},hands[2]{};uint64_t sequence{},stamp{};
    const bool tracked=controllers.leftHandValid && controllers.rightHandValid &&
        OpenXRManager::Get().GetGestureHandFrame(&head,hands,&sequence,&stamp) && head.valid && hands[0].valid && hands[1].valid;
    Sample sample{};
    if(tracked) {
        sample.head={head.posX,head.posY,head.posZ};
        sample.orientation={head.oriX,head.oriY,head.oriZ,head.oriW};
        for(int i=0;i<2;++i)sample.hands[i]=HandInTrackingSpace(sample.head,sample.orientation,
            {hands[i].posX,hands[i].posY,hands[i].posZ});
        sample.sequence=sequence;sample.stampUs=stamp;sample.origin=head.originSerial;sample.valid=true;
    }
    std::lock_guard lock(s_mutex);
    const auto now=XrDiagNowUs();
    if(!Present(now)) { Clear();return {}; }
    const bool enabled=!manualBack && tracked && g_liveControls.xrBreaststrokeSwim!=0 && !s_paused;
    const auto motion=s_gesture.Update(sample,now,enabled);
    const auto ascent=s_ascent.Update(sample,now,enabled);
    s_tracking=tracked;
    if(motion.stroke)++s_strokes;
    if(ascent.stroke) { ++s_ascents;s_gesture.Reset(); }
    s_direction=tracked ? RotateVector(sample.orientation,{0,0,-1}):XrVector3f{};
    const bool ascending=ascent.forward>0 || ascent.stroke;
    s_input={ascending ? 0.0f:motion.forward,true,ascent.forward,
        !ascending && s_state==1 && motion.forward>.15f && s_direction.y<-.25f,!ascending && motion.boost};
    s_inputStamp=now;
    return s_input;
}

bool SprintInput(bool wanted) {
    const bool context=ContextAllowed();
    std::lock_guard lock(s_mutex);
    const auto now=XrDiagNowUs();
    if(!context || s_paused || !Fresh(now) || !s_input.water) { s_sprint.Reset();s_button=false;return false; }
    s_button=s_sprint.Update(wanted,s_fast,now);return s_button;
}
float ScriptAction(uintptr_t owner,uint64_t name,bool justPressed) {
    constexpr uint64_t moveY=Hash("MoveY"),sprint=Hash("ToggleSprint"),jump=Hash("Jump"),dive=Hash("Dive");
    int action;
    if(name==dive)action=Dive;
    else if(justPressed)return 0;
    else if(name==moveY)action=Forward;
    else if(name==sprint)action=Boost;
    else if(name==jump)action=Ascend;
    else return 0;
    if(!IsPlayer(owner))return 0;
    return NativeAction(action);
}
bool IsPlayer(uintptr_t owner) {
    std::lock_guard lock(s_mutex);
    return owner && owner==s_player;
}
float DebugValue(int index) {
    if(index>=20 && index<=23)return NativeAction(index-20);
    std::lock_guard lock(s_mutex);
    switch(index) {
    case 0:return float(s_state);
    case 1:return float(s_fast);
    case 2:return float(s_strokes);
    case 3:return float(s_gesture.CurrentPhase());
    case 4:return s_input.forward;
    case 5:return s_button ? 1.0f:0.0f;
    case 6:return s_stamp ? float(XrDiagNowUs()-s_stamp)*.001f:-1.0f;
    case 7:return s_input.water ? 1.0f:0.0f;
    case 8:return s_tracking ? 1.0f:0.0f;
    case 9:return s_input.ascend;
    case 10:return float(s_ascents);
    case 11:return s_automatic ? 1.0f:0.0f;
    case 12:return s_inputObject && s_inputOwner==cvr::roomscale::PlayerIdentity() ? 1.0f:0.0f;
    case 13:return s_waterContext ? 1.0f:0.0f;
    case 14:return s_input.boost ? 1.0f:0.0f;
    case 15:return s_gesture.Speed();
    case 16:return ScriptHooksActive() ? 1.0f:0.0f;
    default:return -1;
    }
}
static bool InstallActions() {
    // CP2077 2.31: shared CName action-table reader. No table entries or input
    // events are overwritten; only this player's four swimming reads differ.
    const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    const auto site=reinterpret_cast<void*>(base+0x648ECC);
    const uint8_t expected[]={0x48,0x8B,0x41,0x08,0x8B,0x49,0x14,0x48,0x8D,0x0C,0x49,0x48,0xC1,0xE1,0x04};
    if(std::memcmp(site,expected,sizeof(expected)))return false;
    const auto init=MH_Initialize();if(init!=MH_OK && init!=MH_ERROR_ALREADY_INITIALIZED)return false;
    if(MH_CreateHook(site,reinterpret_cast<void*>(&CvrSwimmingActionDetour),&CvrSwimmingActionOriginal)!=MH_OK)return false;
    return MH_EnableHook(site)==MH_OK;
}
CVR_HOOK("SwimmingActions",::cvr::hooks::Stage::Boot,101,InstallActions);
}
