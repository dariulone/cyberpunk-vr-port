#include "Runtimes/EyeTracking.hpp"
#include "Render/FixedVrs.hpp"
#include <cstring>
#include <cmath>
#include <iostream>
#include <stdexcept>

// Exercise the real adapter with a deterministic OpenXR runtime, including privacy loss.
int g_launcherWidth=2000,g_launcherHeight=2000;
extern "C" {
int32_t CyberpunkVR_AsymProjection=1,CyberpunkVR_AsymSquarePixels=1;
int32_t CyberpunkVR_EngineStereoEyeWindows=1,CyberpunkVR_MonoView=0;
float CyberpunkVR_AsymInnerGuard=0;
}
namespace cvr::engine_stereo {bool Enabled(){return true;}}
void Log(const char*,...){}
static cvr::vrs::Settings settings{1,40,40,0,0,true};
static cvr::vrs::GazeCenters published{};
static uint64_t stamp{};
namespace cvr::vrs {
Settings GetSettings(){return settings;}
void UpdateGaze(GazeCenters value,uint64_t tick){published=value;stamp=tick;}
}
static bool support=true,active=true,tracked=true,failBind=false;
static bool loseFocusDuringLocate=false;
static XrTime sampleTime=1000000000;
static XrQuaternionf direction{0,0,0,1};
static unsigned creates{},destroys{},locates{};
static void Check(bool value){if(!value)throw std::runtime_error("Eye tracking adapter assertion failed");}
extern "C" {
XRAPI_ATTR XrResult XRAPI_CALL xrGetSystemProperties(XrInstance,XrSystemId,XrSystemProperties* p){
    static_cast<XrSystemEyeGazeInteractionPropertiesEXT*>(p->next)->supportsEyeGazeInteraction=support;return XR_SUCCESS;
}
XRAPI_ATTR XrResult XRAPI_CALL xrCreateAction(XrActionSet,const XrActionCreateInfo* i,XrAction* a){
    Check(i->actionType==XR_ACTION_TYPE_POSE_INPUT && i->countSubactionPaths==0);++creates;*a=reinterpret_cast<XrAction>(1);return XR_SUCCESS;
}
XRAPI_ATTR XrResult XRAPI_CALL xrStringToPath(XrInstance,const char* text,XrPath* p){
    Check(!std::strcmp(text,"/interaction_profiles/ext/eye_gaze_interaction") || !std::strcmp(text,"/user/eyes_ext/input/gaze_ext/pose"));*p=1;return XR_SUCCESS;
}
XRAPI_ATTR XrResult XRAPI_CALL xrSuggestInteractionProfileBindings(XrInstance,const XrInteractionProfileSuggestedBinding* b){
    Check(b->countSuggestedBindings==1);return failBind?XR_ERROR_PATH_UNSUPPORTED:XR_SUCCESS;
}
XRAPI_ATTR XrResult XRAPI_CALL xrCreateActionSpace(XrSession,const XrActionSpaceCreateInfo* i,XrSpace* s){
    Check(i->poseInActionSpace.orientation.w==1 && !i->subactionPath);*s=reinterpret_cast<XrSpace>(2);return XR_SUCCESS;
}
XRAPI_ATTR XrResult XRAPI_CALL xrDestroySpace(XrSpace){++destroys;return XR_SUCCESS;}
XRAPI_ATTR XrResult XRAPI_CALL xrGetActionStatePose(XrSession,const XrActionStateGetInfo*,XrActionStatePose* p){p->isActive=active;return XR_SUCCESS;}
XRAPI_ATTR XrResult XRAPI_CALL xrLocateSpace(XrSpace,XrSpace base,XrTime time,XrSpaceLocation* l){
    Check(base==reinterpret_cast<XrSpace>(3) && time==1000000000);++locates;
    if(loseFocusDuringLocate)cvr::eye_tracking::Clear();
    l->locationFlags=XR_SPACE_LOCATION_ORIENTATION_VALID_BIT|XR_SPACE_LOCATION_POSITION_VALID_BIT;
    if(tracked)l->locationFlags|=XR_SPACE_LOCATION_ORIENTATION_TRACKED_BIT|XR_SPACE_LOCATION_POSITION_TRACKED_BIT;
    l->pose.orientation=direction;static_cast<XrEyeGazeSampleTimeEXT*>(l->next)->time=sampleTime;return XR_SUCCESS;
}
}
int main()try {
    using namespace cvr::eye_tracking;
    const auto instance=reinterpret_cast<XrInstance>(1);const auto set=reinterpret_cast<XrActionSet>(1);
    const auto session=reinterpret_cast<XrSession>(1);
    std::vector<const char*> enabled;ChooseExtensions({},enabled);CreateAction(instance,1,set);
    Check(enabled.empty() && creates==0 && GetState()==State::Unavailable);
    XrExtensionProperties ext{XR_TYPE_EXTENSION_PROPERTIES};strcpy_s(ext.extensionName,XR_EXT_EYE_GAZE_INTERACTION_EXTENSION_NAME);
    ChooseExtensions({ext},enabled);Check(enabled.size()==1);support=false;CreateAction(instance,1,set);Check(creates==0);
    support=true;CreateAction(instance,1,set);CreateSpace(session);Check(creates==1 && GetState()==State::Waiting);
    XrView views[2]{{XR_TYPE_VIEW},{XR_TYPE_VIEW}};
    for(auto& v:views){v.pose.orientation.w=1;v.fov={-.8f,.8f,.8f,-.8f};}
    auto sample=[&](bool focused=true){Sample(session,reinterpret_cast<XrSpace>(3),1000000000,focused,views,2);};
    sample();Check(stamp && GetState()==State::Tracking && std::abs(published[0][0]-.5f)<1e-5f);
    direction={0,-std::sin(.2f),0,std::cos(.2f)};sample();Check(stamp && published[0][0]>.5f);
    sample(false);Check(!stamp);sample();Check(stamp);
    loseFocusDuringLocate=true;sample();Check(!stamp);loseFocusDuringLocate=false;sample();Check(stamp);
    active=false;sample();Check(!stamp);active=true;
    tracked=false;sample();Check(!stamp);tracked=true;
    sampleTime=800000000;sample();Check(!stamp);sampleTime=1000000000;
    CyberpunkVR_AsymProjection=0;sample();Check(!stamp && GetState()==State::ProjectionUnavailable);CyberpunkVR_AsymProjection=1;
    sample();Check(stamp);settings.eyeTracked=false;const auto old=locates;sample();Check(!stamp && locates==old);settings.eyeTracked=true;
    Shutdown();Check(destroys==1 && GetState()==State::Unavailable && !stamp);
    failBind=true;CreateAction(instance,1,set);CreateSpace(session);sample();Check(!stamp && GetState()==State::Error);Shutdown();Check(destroys==1);
    std::cout<<"PASS OpenXR eye gaze capability, bindings, projection, focus/privacy loss, stale samples, off and teardown\n";
}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}
