#include "Anim/WheelGrab.hpp"
#include "Anim/LeftHanded.hpp"
#include "Anim/CharacterRig.hpp"
#include "Core/LiveControls.hpp"
#include "Utils/SharedSlots.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include "Anim/VrikHook.hpp"
#include "Anim/VrikState.hpp"
#include "Camera/CameraState.hpp"
#include "Camera/CameraLink.hpp"
#include "Camera/PlayerCameraState.hpp"
#include <windows.h>
#include <cmath>
#include <cstdlib>
#include <iostream>
LiveControls g_liveControls{};
std::atomic<bool> g_isDriving{true};
bool g_hasWeaponEquipped=false,g_isInVehicle=true;
volatile int g_menuModeValue=0;
float slots[256]{};float* g_pSharedHands=slots;
float g_fkPos[VRIK_MAX_BONES][3]{},g_fkRot[VRIK_MAX_BONES][4]{};
volatile int g_VRSmokeFingerActive=0,g_VRSmokeFingerActiveL=0,g_VRSmokeFingerCount=0,g_VRSmokeFingerCountL=0;
int g_VRSmokeFingerIdx[32]{},g_VRSmokeFingerIdxL[32]{};
// Boundaries unused by this ownership test must never silently emulate tracking.
OpenXRManager& OpenXRManager::Get(){std::abort();}
bool OpenXRManager::GetGestureHandFrame(OpenXRHeadPose*,OpenXRHeadPose*,uint64_t*,uint64_t*,bool)const{std::abort();}
void Log(const char*,...){}
void VRIK_QuatNorm(float* q){const float n=std::sqrt(q[0]*q[0]+q[1]*q[1]+q[2]*q[2]+q[3]*q[3]);for(int i=0;i<4;++i)q[i]/=n;}
namespace cvr::anim {bool WeaponInLeftHand(){return false;}}
namespace cvr::gestures {bool OwnsHand(int){return false;}}
float SharedPose(int index) {return slots[index];}
bool OverlayIsVisible(){return false;}
// The vehicle minimap anchor (PublishMinimapAnchor) is not under test: no camera pair, so it never publishes.
bool VRIK_ReadTransformSnapshot(VrikTransformSnapshot*){return false;}
namespace cvr::camera {bool CamWriteRecordFind(const float[4],OpenXRHeadPose*,uint32_t*,uint32_t*){return false;}}
float GetWorldScale(){return 1.f;}
uintptr_t CurrentMountedVehicleIdentity(){return 0;}
void VRIK_QuatConj(const float* q,float* o){o[0]=-q[0];o[1]=-q[1];o[2]=-q[2];o[3]=q[3];}
void VRIK_QuatMul(const float* a,const float* b,float* o){
    const float r[4]={a[3]*b[0]+a[0]*b[3]+a[1]*b[2]-a[2]*b[1],a[3]*b[1]-a[0]*b[2]+a[1]*b[3]+a[2]*b[0],
                      a[3]*b[2]+a[0]*b[1]-a[1]*b[0]+a[2]*b[3],a[3]*b[3]-a[0]*b[0]-a[1]*b[1]-a[2]*b[2]};
    for(int i=0;i<4;++i)o[i]=r[i];}
void VRIK_QuatRotateVec(const float* q,const float* v,float* o){
    const float p[4]={v[0],v[1],v[2],0};float c[4],t[4],r[4];VRIK_QuatConj(q,c);VRIK_QuatMul(q,p,t);VRIK_QuatMul(t,c,r);
    o[0]=r[0];o[1]=r[1];o[2]=r[2];}
void Check(bool value,const char* message){if(!value){std::cerr<<message<<'\n';std::exit(1);}}
int main(){
    using namespace cvr::anim;
    g_liveControls.xrWheelGrab=1;g_liveControls.xrWheelRadius=.28f;
    slots[0]=slots[8]=1;g_fkPos[0][0]=.2f;g_fkPos[1][0]=-.2f;
    WheelReset();WheelCaptureAnim(0,0);WheelCaptureAnim(1,1);
    WheelStoreTarget(0,g_fkPos[0]);WheelStoreTarget(1,g_fkPos[1]);WheelUpdate(.02f);
    slots[49]=slots[vrshared::kLeftGripPressed]=1;
    WheelUpdate(.02f);WheelStoreTarget(0,g_fkPos[0]);WheelStoreTarget(1,g_fkPos[1]);WheelPublishGrab();
    Check(WheelControlState()==7,"both grips failed to engage");
    Sleep(270);Check(WheelControlState()==4,"test did not expire the old animation publication");
    WheelMaintainGrab();Check(WheelControlState()==7,"missing camera revoked an existing physical grip");
    slots[49]=0;WheelMaintainGrab();Check(WheelControlState()==6,"right release ignored during camera loss");
    slots[49]=1;WheelMaintainGrab();Check(WheelControlState()==6,"camera-loss path started a new grab without proximity");
    slots[0]=0;WheelMaintainGrab();Check(WheelControlState()==4,"tracking loss did not release the remaining grip");
    g_isDriving=false;Check(WheelControlState()==0,"wheel control escaped the driver seat");
    WheelReset();std::cout<<"PASS production wheel ownership across camera loss, release, regrab and tracking loss\n";
    // ANYWHERE ON THE RIM: a car's wheel, taken at the top by the right hand. Hub at the origin, radius .2.
    g_isDriving=true;slots[0]=slots[8]=1;
    WheelReset();WheelSetMountedCar(true);g_liveControls.xrWheelAnywhere=1;
    slots[49]=slots[vrshared::kLeftGripPressed]=0;
    g_fkRot[0][3]=g_fkRot[1][3]=1;
    WheelCaptureAnim(0,0);WheelCaptureAnim(1,1);
    WheelStoreTarget(0,g_fkPos[0]);WheelStoreTarget(1,g_fkPos[1]);WheelUpdate(.02f);
    const float lean=25*.01745329252f,R=.2f,top[3]={0,R*std::sin(lean),R*std::cos(lean)};
    WheelStoreTarget(0,top);WheelUpdate(.02f);
    slots[49]=1;
    for(int i=0;i<20;++i){WheelCaptureAnim(0,0);WheelCaptureAnim(1,1);WheelStoreTarget(0,top);WheelUpdate(.02f);}
    Check(!WheelHandsOff(0),"a hand on the rim stays solved");
    Check(g_wheelBlendRight.load()>.99f,"it takes hold there");
    float at[3]={top[0],top[1],top[2]},rot[4]={0,0,0,1};
    WheelBlendTarget(0,at,rot);
    for(int k=0;k<3;++k)Check(std::fabs(at[k]-top[k])<.001f,"held where it took hold");
    // the controller wanders: the hand stays where it holds the wheel
    const float pulled[3]={.3f,0,0};
    WheelStoreTarget(0,pulled);
    float next[3]={pulled[0],pulled[1],pulled[2]},nextRot[4]={0,0,0,1};
    WheelBlendTarget(0,next,nextRot);
    for(int k=0;k<3;++k)Check(std::fabs(next[k]-top[k])<.001f,"a hand on the wheel does not follow the controller");
    // the drawn wheel turns a quarter clockwise, the animated hands with it: the hand goes round with it, to 3 o'clock
    g_fkPos[0][0]=0;g_fkPos[0][1]=-R*std::sin(lean);g_fkPos[0][2]=-R*std::cos(lean);
    g_fkPos[1][0]=0;g_fkPos[1][1]=R*std::sin(lean);g_fkPos[1][2]=R*std::cos(lean);
    // in steps, as an animation does: all at once is taken for a change of grip
    for(int i=1;i<=9;++i) {
        const float a=-i*10*.01745329252f;
        g_fkPos[0][0]=R*std::cos(a);g_fkPos[0][1]=R*std::sin(a)*std::sin(lean);g_fkPos[0][2]=R*std::sin(a)*std::cos(lean);
        for(int k=0;k<3;++k)g_fkPos[1][k]=-g_fkPos[0][k];
        WheelCaptureAnim(0,0);WheelCaptureAnim(1,1);WheelStoreTarget(0,pulled);WheelUpdate(.02f);
    }
    float turned[3]={pulled[0],pulled[1],pulled[2]},turnedRot[4]={0,0,0,1};
    WheelBlendTarget(0,turned,turnedRot);
    Check(std::fabs(turned[0]-R)<.001f && std::fabs(turned[1])<.001f && std::fabs(turned[2])<.001f,"the hand left the wheel it holds");
    // a jump in the animation is a change of grip, not a turn
    g_fkPos[0][0]=-R;g_fkPos[0][1]=0;g_fkPos[0][2]=0;g_fkPos[1][0]=R;
    WheelCaptureAnim(0,0);WheelCaptureAnim(1,1);WheelStoreTarget(0,pulled);WheelUpdate(.02f);
    float jumped[3]={pulled[0],pulled[1],pulled[2]},jumpedRot[4]={0,0,0,1};
    WheelBlendTarget(0,jumped,jumpedRot);
    Check(std::fabs(jumped[0]-R)<.001f,"a hand popped with the animation's grip");
    g_fkPos[0][0]=R;g_fkPos[0][1]=g_fkPos[0][2]=0;g_fkPos[1][0]=-R;g_fkPos[1][1]=g_fkPos[1][2]=0;
    // past the rim's band it is not a grab
    slots[49]=0;WheelReset();WheelSetMountedCar(true);
    WheelCaptureAnim(0,0);WheelCaptureAnim(1,1);WheelStoreTarget(0,g_fkPos[0]);WheelStoreTarget(1,g_fkPos[1]);WheelUpdate(.02f);
    const float beside[3]={.6f,0,0};
    WheelStoreTarget(0,beside);WheelUpdate(.02f);slots[49]=1;WheelStoreTarget(0,beside);WheelUpdate(.02f);
    for(int i=0;i<10;++i){WheelStoreTarget(0,beside);WheelUpdate(.02f);}
    Check(g_wheelBlendRight.load()<.01f,"a squeeze away from the wheel is no grab");
}
