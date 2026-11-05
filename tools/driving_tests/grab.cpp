#include "Anim/WheelGrab.hpp"
#include "Anim/CharacterRig.hpp"
#include "Core/LiveControls.hpp"
#include "Utils/SharedSlots.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include <windows.h>
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
void VRIK_QuatNorm(float*){std::abort();}
namespace cvr::anim { bool WeaponInLeftHand(){return false;} }
float SharedPose(int index) {return slots[index];}
bool OverlayIsVisible(){return false;}
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
}
