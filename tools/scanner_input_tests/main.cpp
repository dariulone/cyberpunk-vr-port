#include "Hooks/ScannerGesture.hpp"
#include "Hooks/Handedness.hpp"
#include "Camera/GazeCenter.hpp"
#include "Camera/TakeoverTargeting.hpp"
#include <array>
#include <cstring>
#include <stdexcept>
#include <limits>
#include "Utils/SharedSlots.hpp"

float shared[256]{};
float* g_pSharedHands=shared;
float g_VRHandRawModel[2][3]{},g_VRHandRawRot[2][4]{},g_VRHandRotLock[2][4]{},g_VRHandStopModel[2][3]{};
int g_VRHandRawValid[2]{},g_VRHandRotLockOn[2]{},g_VRHandStopValid[2]{};
float CyberpunkVR_DebugHandStop[2][8]{},g_VRHandStopDeadband=.02f;
namespace cvr::physhands {
int noted{},adjusted{};
float targetSeen[3]{},rotationSeen[4]{};
void NoteTarget(int,const float* p,const float* q) {
    ++noted;std::memcpy(targetSeen,p,sizeof(targetSeen));std::memcpy(rotationSeen,q,sizeof(rotationSeen));
}
bool Adjust(int,float* p,float*) {++adjusted;p[0]=-5;return true;}
}
#include "scanner_hand_stop.inc"

void Check(bool value){if(!value)throw std::runtime_error("scanner input regression");}
int main(){
    constexpr float pi=3.14159265358979323846f;
    const float up=std::tan(44*pi/180),down=std::tan(-55*pi/180),half=(up-down)*.5f;
    const auto gaze=cvr::gaze::FromWindows(up,down,up,down);
    Check(std::abs((gaze.tangentY-(up+down)*.5f)/half)<1.e-6f);
    // Native-dot translation and the XR panel must land on the same pixel.
    const float nativeY=(1+gaze.ndcY)*3088*.5f;
    Check(std::abs(nativeY+cvr::gaze::CanvasOffset(gaze,3088,2856.f/3840)*(2856.f/3840)-1544)<.001f);
    std::array<uint8_t,0x31240> cache{};
    // The 2.5 source cache is 0x2F0 bytes; the rest stands for the next heap block, which must stay untouched.
    std::array<uint8_t,0x400> source{};
    source.fill(0xA5);
    const auto next=source;
    const uintptr_t sourceAddress=reinterpret_cast<uintptr_t>(source.data());
    std::memcpy(cache.data()+cvr::camera::TargetingSourceCacheOffset,&sourceAddress,sizeof(sourceAddress));
    uintptr_t player=0x123400;uint64_t entity=42;uint32_t active=1;
    std::memcpy(cache.data(),&player,8);std::memcpy(cache.data()+8,&entity,8);std::memcpy(cache.data()+24,&active,4);
    const float camera[]{1,2,3,1,0,0,0,1},origin[]{4,5,6,1},forward[]{0,1,0,1};
    using namespace cvr::camera;
    std::memcpy(cache.data()+TargetingCameraTransformOffset,camera,sizeof(camera));
    std::memcpy(cache.data()+TargetingDefaultPositionOffset,origin,sizeof(origin));
    std::memcpy(cache.data()+TargetingDefaultForwardOffset,forward,sizeof(forward));
    // Garbage where the forward should be (a moved layout): the cache is skipped, the state still re-aimed.
    Check(CenterHeadGaze(cache.data(),player,entity,gaze.tangentY));
    Check(source==next);
    std::memcpy(cache.data()+TargetingDefaultForwardOffset,forward,sizeof(forward));
    std::memcpy(source.data()+0x260,origin,sizeof(origin));
    std::memcpy(source.data()+SourceCacheForwardOffset,forward,sizeof(forward));
    auto before=source;
    Check(!CenterHeadGaze(cache.data(),player,entity+1,gaze.tangentY));
    Check(CenterHeadGaze(cache.data(),player,entity,gaze.tangentY));
    float ray[4];std::memcpy(ray,cache.data()+TargetingDefaultForwardOffset,sizeof(ray));
    Check(std::abs(ray[2]/ray[1]-gaze.tangentY)<1.e-6f);
    Check(!std::memcmp(cache.data()+TargetingCameraTransformOffset,camera,sizeof(camera)));
    Check(!std::memcmp(cache.data()+TargetingDefaultPositionOffset,origin,sizeof(origin)));
    Check(!std::memcmp(source.data()+0x260,origin,sizeof(origin)));
    Check(!std::memcmp(source.data()+SourceCacheForwardOffset,ray,sizeof(ray)));
    std::memcpy(before.data()+SourceCacheForwardOffset,ray,sizeof(ray));
    Check(source==before); // nothing else written, nothing past the 0x2F0-byte object
    // Already centred last frame (the engine had not refreshed it yet): still the camera's ray, written again.
    std::memcpy(cache.data()+TargetingDefaultForwardOffset,forward,sizeof(forward));
    Check(CenterHeadGaze(cache.data(),player,entity,gaze.tangentY));
    Check(!std::memcmp(source.data()+SourceCacheForwardOffset,ray,sizeof(ray)));
    Check(cvr::gaze::FromWindows(1,-1,1,-1).tangentY==0);
    cvr::gaze::Publish(gaze);cvr::gaze::headLook=false;Check(cvr::gaze::Read().tangentY==0);cvr::gaze::headLook=true;
    cvr::input::ScannerEarZone zone;
    Check(zone.Contains(-.25f,-.10f,.15f,false));
    Check(zone.Contains(-.20f,-.18f,.12f,false));
    Check(zone.Contains(-.20f,0,-.05f,false));
    Check(!zone.Contains(-.20f,-.30f,-.40f,false)); // ordinary hand rest / weapon position
    Check(!zone.Contains(.10f,-.03f,.06f,false)); // other side of the head
    // Run the exact raw-click owner and deferred-L3 policy used before XInput/handedness.
    cvr::input::ScannerClick click;
    cvr::input::DeferredLeftStickClick deferred;
    unsigned sprintClicks=0;
    auto tick=[&](bool down,bool atEar=true,bool enabled=true,bool valid=true,double dt=20,bool dpad=false) {
        const bool claim=click.Update(enabled,down,valid,atEar?-.18f:0.f,-.08f,.08f,dt);
        if(deferred.Update(down,claim || dpad))++sprintClicks;
        return claim;
    };
    for(int i=0;i<6;++i)Check(!tick(true)); // held from startup, even once zone settles
    Check(click.Presses()==0 && !click.Held());
    Check(!tick(false)); // release the unclaimed ordinary L3
    Check(sprintClicks==1);
    Check(tick(true) && click.Presses()==1 && click.Held()); // fresh at ear, immediate
    for(int i=0;i<20;++i)Check(tick(true));
    Check(click.Presses()==1); // holding never toggles twice
    Check(tick(false));Check(!click.Held());Check(sprintClicks==1); // consume release too
    Check(tick(true) && click.Presses()==2); // release/rearm permits closing
    for(int i=0;i<20;++i)Check(tick(true,false)); // carry a consumed hold out of the zone
    Check(!click.Held());Check(tick(false,false));Check(sprintClicks==1);
    Check(!tick(false,false));Check(!tick(true,false));
    for(int i=0;i<10;++i)Check(!tick(true,true)); // ordinary L3 carried into ear never scans
    Check(click.Presses()==2);Check(!tick(false));Check(sprintClicks==2);
    Check(tick(true) && click.Presses()==3);
    Check(tick(true,true,false));Check(tick(false,true,false));Check(sprintClicks==2);
    // Pressing during a menu/disabled state cannot turn into a scanner click after resuming.
    Check(!tick(true,true,false));
    for(int i=0;i<10;++i)Check(!tick(true));
    Check(click.Presses()==3);Check(!tick(false));
    const auto ordinary=sprintClicks;
    Check(tick(true));
    for(int i=0;i<20;++i)Check(tick(true,true,true,false)); // invalid tracking cannot repeat
    Check(click.Presses()==4);Check(tick(false));Check(sprintClicks==ordinary);
    Check(!tick(false));
    // No press during the exit grace: the current pose, not just the armed latch, must be inside.
    Check(!tick(true,false));Check(click.Presses()==4);Check(!tick(false,false));
    // Ordinary D-pad use outside the ear still suppresses its deferred sprint.
    const auto beforeDpad=sprintClicks;
    Check(!tick(true,false,true,true,20,true));Check(!tick(false,false));Check(sprintClicks==beforeDpad);
    // Settling after an early press must not manufacture a fresh edge.
    cvr::input::ScannerClick early;
    Check(!early.Update(true,false,true,-.18f,-.08f,.08f,20));
    Check(!early.Update(true,true,true,-.18f,-.08f,.08f,20));
    Check(!early.Update(true,true,true,-.18f,-.08f,.08f,100));Check(early.Presses()==0);
    early.Update(true,false,true,-.18f,-.08f,.08f,20);
    Check(early.Update(true,true,true,-.18f,-.08f,.08f,20));
    Check(early.Update(true,false,true,-.18f,-.08f,.08f,20));
    Check(!early.Update(true,true,true,-.18f,-.08f,.08f,1000)); // resume while held: not a fresh edge
    Check(!early.Update(true,true,true,-.18f,-.08f,.08f,100));Check(early.Presses()==1);
    early.Update(true,false,true,-.18f,-.08f,.08f,20);
    Check(early.Update(true,true,true,-.18f,-.08f,.08f,20));Check(early.Presses()==2);
    Check(!zone.Contains(std::numeric_limits<float>::quiet_NaN(),0,0,false));
    cvr::input::ScannerRequest request;bool held=true;
    request.Update(held,0,1000);Check(held);
    request.Update(held,0,1749);Check(held);
    request.Update(held,0,1750);Check(!held); // rejected opening releases LB
    held=true;request.Update(held,0,2000);request.Update(held,1,2040);
    request.Update(held,1,10000);Check(held); // accepted hold can last indefinitely
    request.Update(held,0,10020);Check(!held); // game-driven exit releases our latch
    held=true;request.Update(held,-1,11000);request.Update(held,-1,11750);Check(!held);
    held=true;request.Update(held,1,12000);held=false;request.Update(held,1,12020);
    held=true;request.Update(held,0,12040);Check(held); // a new squeeze gets its own acceptance window
    // Scanner world slowdown must not make hands consume a pre-scan physics pose. The latch has
    // no freshness timeout: the input heartbeat expiring does not mean the game left focus mode.
    cvr::input::ScannerHandPhysics physics;
    Check(!physics.Active() && physics.Accepts(0));
    physics.Publish(0);Check(physics.Epoch()==0);
    const auto beforeScan=physics.Epoch();
    physics.Publish(1);
    const auto scanning=physics.Epoch();
    Check(physics.Active() && !physics.Accepts(beforeScan) && !physics.Accepts(scanning));
    physics.Publish(1);Check(physics.Epoch()==scanning); // heartbeat, not another transition
    physics.Publish(-1);Check(physics.Active() && physics.Epoch()==scanning); // unknown/stale is not exit
    physics.Publish(0);
    const auto afterScan=physics.Epoch();
    Check(!physics.Active() && afterScan!=beforeScan);
    Check(!physics.Accepts(beforeScan) && !physics.Accepts(scanning) && physics.Accepts(afterScan));
    // A rapid open/close before the worker/fetch runs still invalidates its queued result.
    physics.Publish(1);physics.Publish(0);
    Check(!physics.Active() && !physics.Accepts(afterScan) && physics.Accepts(physics.Epoch()));
    const auto ready=physics.Epoch();physics.Publish(0);Check(physics.Epoch()==ready);
    // A rejected input request never publishes Vision=1 and must not suspend physics.
    held=true;request.Update(held,0,13000);request.Update(held,0,13750);
    Check(!held && physics.Accepts(ready));
    cvr::input::ScannerHandPhysics slow;
    for(float scale:{.5f,.1f,.01f,0.f}) {
        const auto old=slow.Epoch();slow.TimeScale(scale);Check(slow.Active() && !slow.Accepts(old));
        slow.Publish(1);slow.TimeScale(1);Check(slow.Active()); // scanner still owns the suspension
        slow.TimeScale(scale);slow.Publish(0);Check(slow.Active()); // global slowdown still owns it
        slow.TimeScale(std::numeric_limits<float>::quiet_NaN());Check(slow.Active());
        slow.TimeScale(1);Check(!slow.Active() && slow.Accepts(slow.Epoch()) && !slow.Accepts(old));
    }
    // Exercise the production hand-stop function against a frozen body and a stale script lock.
    shared[vrshared::kReloadOwnedHand]=-1;
    cvr::input::scannerHandPhysics.Publish(1);
    for(int side=0;side<2;++side) {
        const float raw[]{.2f+float(side),.5f,1.1f},rot[]{0,0,0,1};
        float p[3],q[4];std::memcpy(p,raw,sizeof(p));std::memcpy(q,rot,sizeof(q));
        g_VRHandStopValid[side]=g_VRHandRotLockOn[side]=1;
        g_VRHandStopModel[side][0]=-5;g_VRHandRotLock[side][0]=1;
        VRIK_ApplyHandStop(side,p,q);
        Check(!std::memcmp(p,raw,sizeof(p)) && !std::memcmp(q,rot,sizeof(q)));
        Check(g_VRHandRawValid[side] && !std::memcmp(g_VRHandRawModel[side],raw,sizeof(p)));
        Check(!std::memcmp(cvr::physhands::targetSeen,raw,sizeof(p)) &&
              !std::memcmp(cvr::physhands::rotationSeen,rot,sizeof(q)));
        Check(!g_VRHandStopValid[side] && !g_VRHandRotLockOn[side]);
        Check(CyberpunkVR_DebugHandStop[side][3]==0 && CyberpunkVR_DebugHandStop[side][7]==0);
    }
    Check(cvr::physhands::noted==2 && cvr::physhands::adjusted==0);
    cvr::input::scannerHandPhysics.Publish(0);
    float resumed[]{.1f,.2f,.3f},resumeRot[]{0,0,0,1};
    VRIK_ApplyHandStop(0,resumed,resumeRot);
    Check(cvr::physhands::noted==3 && cvr::physhands::adjusted==1 && resumed[0]==-5);
    cvr::input::scannerHandPhysics.TimeScale(.1f);
    float slowPos[]{.1f,.2f,.3f},slowRot[]{0,0,0,1};
    VRIK_ApplyHandStop(0,slowPos,slowRot);
    Check(cvr::physhands::adjusted==1 && slowPos[0]==.1f); // world physics never replaces tracking
    shared[vrshared::kReloadOwnedHand]=0;
    g_VRHandStopValid[0]=g_VRHandRotLockOn[0]=1;g_VRHandRotLock[0][0]=0;g_VRHandRotLock[0][3]=1;
    g_VRHandStopModel[0][0]=.3f;g_VRHandStopModel[0][1]=.2f;g_VRHandStopModel[0][2]=.3f;
    VRIK_ApplyHandStop(0,slowPos,slowRot);
    Check(slowPos[0]>.2f && cvr::physhands::adjusted==1); // explicit reload ownership remains functional
    shared[vrshared::kReloadOwnedHand]=-1;cvr::input::scannerHandPhysics.TimeScale(1);
    // Left-handed: the hands' buttons traded, the rest kept, and trading twice is no trade.
    using cvr::input::MirroredButtons;using cvr::input::MirroredPart;
    Check(MirroredButtons(0x1000)==0x4000 && MirroredButtons(0x4000)==0x1000);   // A <-> X
    Check(MirroredButtons(0x2000)==0x8000 && MirroredButtons(0x8000)==0x2000);   // B <-> Y
    Check(MirroredButtons(0x0040)==0x0080 && MirroredButtons(0x0080)==0x0040);   // stick clicks
    Check(MirroredButtons(0x0300)==0x0300 && MirroredButtons(0x003F)==0x003F);   // shoulders, D-pad, Start, Back
    for(uint32_t b=0;b<0x10000;++b)Check(MirroredButtons(MirroredButtons(uint16_t(b)))==b);
    Check(MirroredPart("a")=="x" && MirroredPart("x_hold")=="a_hold" && MirroredPart("b_disabled")=="y_disabled");
    Check(MirroredPart("left_trigger_hold")=="right_trigger_hold" && MirroredPart("right_axis_up")=="left_axis_up");
    Check(MirroredPart("right_thumb")=="left_thumb" && MirroredPart("ls_filled")=="rs_filled");
    Check(MirroredPart("left_shoulder")=="left_shoulder" && MirroredPart("right_shoulder_hold")=="right_shoulder_hold");
    Check(MirroredPart("y_wide")=="y_wide" && MirroredPart("dpad_up")=="dpad_up" && MirroredPart("start")=="start");
    Check(MirroredPart("arrow_up")=="arrow_up" && MirroredPart("")=="");
    struct Pad{uint16_t buttons;float leftTrigger,rightTrigger,leftThumbX,leftThumbY,rightThumbX,rightThumbY,
               leftGrip,rightGrip;bool leftHandValid,rightHandValid;
               uint64_t scannerPresses{};bool scannerHeld{};};
    const Pad pad{0x1040,.1f,.9f,.2f,.3f,-.4f,-.5f,.6f,0,true,false};
    const Pad m=cvr::input::Mirrored(pad);
    Check(m.buttons==0x4080 && m.leftTrigger==.9f && m.rightTrigger==.1f && m.leftThumbX==-.4f && m.leftThumbY==-.5f);
    Check(m.rightThumbX==.2f && m.rightThumbY==.3f && m.leftGrip==0 && m.rightGrip==.6f);
    Check(!m.leftHandValid && m.rightHandValid);
    for(int bindings=0;bindings<=2;++bindings) {
        Pad physical=pad;
        physical.buttons=0; // the scanner consumed raw physical L3 before the button snapshot
        physical.scannerPresses=9;physical.scannerHeld=true;
        const auto mirrored=cvr::input::Mirrored(physical,bindings);
        Check(mirrored.buttons==0 && mirrored.scannerPresses==9 && mirrored.scannerHeld);
        physical.buttons=0x40;physical.scannerHeld=false; // ordinary L3 outside the ear
        const auto normal=cvr::input::Mirrored(physical,bindings);
        Check(normal.buttons==(bindings>=2?0x80:0x40)); // normal handedness policy unchanged
    }
}
