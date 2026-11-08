#include "Hooks/ScannerGesture.hpp"
#include "Hooks/Handedness.hpp"
#include "Camera/GazeCenter.hpp"
#include "Camera/TakeoverTargeting.hpp"
#include <array>
#include <cstring>
#include <stdexcept>
#include <limits>
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
    cvr::input::ScannerSqueeze squeeze;
    Check(!squeeze.Update(1,true));   // held from the start: a grip must open first
    Check(!squeeze.Update(0,true));Check(squeeze.Update(1,true));          // a clean press at the ear: scan
    Check(squeeze.Update(1,false)); // leaving keeps the squeeze; the zone's own exit timer decides
    squeeze.Update(0,false);Check(!squeeze.Update(1,false));Check(!squeeze.Update(1,true)); // carried in: no scan
    squeeze.Update(0,false);
    Check(!squeeze.Update(.8f,false));Check(!squeeze.Update(.6f,true));Check(!squeeze.Update(.8f,true));
    Check(!squeeze.Update(.4f,true));Check(!squeeze.Update(.9f,true)); // a held grip dipping at the ear is no new press
    Check(!squeeze.Update(.3f,true));Check(squeeze.Update(.75f,true));  // a real release, then a press: scan
    squeeze.Reset();Check(!squeeze.Update(1,true));                     // held across a reset: no scan
    Check(!squeeze.Update(0,true));Check(squeeze.Update(1,true));
    Check(!squeeze.Update(std::numeric_limits<float>::quiet_NaN(),true)); // a bad sample reads as open
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
               leftGrip,rightGrip;bool leftHandValid,rightHandValid;};
    const Pad pad{0x1040,.1f,.9f,.2f,.3f,-.4f,-.5f,.6f,0,true,false};
    const Pad m=cvr::input::Mirrored(pad);
    Check(m.buttons==0x4080 && m.leftTrigger==.9f && m.rightTrigger==.1f && m.leftThumbX==-.4f && m.leftThumbY==-.5f);
    Check(m.rightThumbX==.2f && m.rightThumbY==.3f && m.leftGrip==0 && m.rightGrip==.6f);
    Check(!m.leftHandValid && m.rightHandValid);
}
