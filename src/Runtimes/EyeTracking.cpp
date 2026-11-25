#include "Runtimes/EyeTracking.hpp"
#include "Runtimes/EyeGazeProjection.hpp"
#include "Runtimes/AsymmetricProjection.hpp"
#include "Render/FixedVrs.hpp"
#include <Windows.h>
#include <atomic>
#include <cstring>
#include <mutex>

extern void Log(const char*, ...);
namespace cvr::eye_tracking {
namespace {
bool extension{}, supported{};
XrAction action=XR_NULL_HANDLE;
XrSpace space=XR_NULL_HANDLE;
std::atomic<State> state{State::Unavailable};
std::mutex publication;
uint64_t epoch{};
void SetState(State value) {
    if(state.exchange(value)!=value)Log("[eye-tracking] state=%d (0 unavailable, 1 waiting, 2 tracking, 3 projection unavailable, 4 error)\n",int(value));
}
}
State GetState(){return state.load();}
void Clear(){std::lock_guard lock(publication);++epoch;vrs::UpdateGaze({},0);if(state.load()==State::Tracking)state.store(State::Waiting);}
void ChooseExtensions(const std::vector<XrExtensionProperties>& props,std::vector<const char*>& enabled) {
    extension=false;supported=false;state.store(State::Unavailable);Clear();
    for(const auto& p:props)if(!std::strcmp(p.extensionName,XR_EXT_EYE_GAZE_INTERACTION_EXTENSION_NAME)) {
        enabled.push_back(XR_EXT_EYE_GAZE_INTERACTION_EXTENSION_NAME);extension=true;break;
    }
    Log("[eye-tracking] XR_EXT_eye_gaze_interaction available=%d\n",int(extension));
}
void CreateAction(XrInstance instance,XrSystemId system,XrActionSet set) {
    if(!extension || !set)return;
    XrSystemEyeGazeInteractionPropertiesEXT gaze{XR_TYPE_SYSTEM_EYE_GAZE_INTERACTION_PROPERTIES_EXT};
    XrSystemProperties props{XR_TYPE_SYSTEM_PROPERTIES};props.next=&gaze;
    supported=XR_SUCCEEDED(xrGetSystemProperties(instance,system,&props)) && gaze.supportsEyeGazeInteraction;
    Log("[eye-tracking] system supports eye gaze=%d\n",int(supported));
    if(!supported)return;
    XrActionCreateInfo info{XR_TYPE_ACTION_CREATE_INFO};info.actionType=XR_ACTION_TYPE_POSE_INPUT;
    strcpy_s(info.actionName,"eye_gaze");strcpy_s(info.localizedActionName,"Eye gaze");
    XrPath profile{},path{};
    if(XR_FAILED(xrCreateAction(set,&info,&action)) ||
       XR_FAILED(xrStringToPath(instance,"/interaction_profiles/ext/eye_gaze_interaction",&profile)) ||
       XR_FAILED(xrStringToPath(instance,"/user/eyes_ext/input/gaze_ext/pose",&path))) {SetState(State::Error);return;}
    XrActionSuggestedBinding binding{action,path};
    XrInteractionProfileSuggestedBinding suggest{XR_TYPE_INTERACTION_PROFILE_SUGGESTED_BINDING};
    suggest.interactionProfile=profile;suggest.countSuggestedBindings=1;suggest.suggestedBindings=&binding;
    SetState(XR_SUCCEEDED(xrSuggestInteractionProfileBindings(instance,&suggest))?State::Waiting:State::Error);
}
void CreateSpace(XrSession session) {
    if(!action || state.load()==State::Error)return;
    XrActionSpaceCreateInfo info{XR_TYPE_ACTION_SPACE_CREATE_INFO};info.action=action;info.poseInActionSpace.orientation.w=1;
    if(XR_FAILED(xrCreateActionSpace(session,&info,&space)))SetState(State::Error);
}
void Sample(XrSession session,XrSpace head,XrTime time,bool focused,const XrView* views,unsigned count) {
    if(!space || !action)return;
    const auto settings=vrs::GetSettings();
    if(!settings.eyeTracked || !settings.mode){Clear();return;}
    if(!focused || !views || count!=2){Clear();return;}
    uint64_t started{};{std::lock_guard lock(publication);started=epoch;}
    XrActionStateGetInfo get{XR_TYPE_ACTION_STATE_GET_INFO};get.action=action;
    XrActionStatePose pose{XR_TYPE_ACTION_STATE_POSE};
    XrEyeGazeSampleTimeEXT sample{XR_TYPE_EYE_GAZE_SAMPLE_TIME_EXT};
    XrSpaceLocation location{XR_TYPE_SPACE_LOCATION};location.next=&sample;
    constexpr XrSpaceLocationFlags flags=XR_SPACE_LOCATION_ORIENTATION_VALID_BIT|XR_SPACE_LOCATION_ORIENTATION_TRACKED_BIT|
        XR_SPACE_LOCATION_POSITION_VALID_BIT|XR_SPACE_LOCATION_POSITION_TRACKED_BIT;
    if(XR_FAILED(xrGetActionStatePose(session,&get,&pose)) || !pose.isActive ||
       XR_FAILED(xrLocateSpace(space,head,time,&location)) || (location.locationFlags&flags)!=flags ||
       !FreshSample(time,sample.time)){Clear();return;}
    const XrFovf fovs[]{views[0].fov,views[1].fov};
    const XrQuaternionf orientations[]{views[0].pose.orientation,views[1].pose.orientation};
    projection::EyeWindow windows[2];
    if(!projection::Active() || !projection::Windows(fovs,orientations,windows)) {
        Clear();SetState(State::ProjectionUnavailable);return;
    }
    vrs::GazeCenters centers;
    for(int eye=0;eye<2;++eye)if(!Project(location.pose.orientation,windows[eye].l,windows[eye].r,windows[eye].d,windows[eye].u,
                                       centers[eye][0],centers[eye][1])) {Clear();return;}
    const auto now=GetTickCount64();
    const auto age=uint64_t(time>sample.time?(time-sample.time)/1000000:0);
    std::lock_guard lock(publication);
    if(started!=epoch)return; // focus loss/teardown won the race with xrLocateSpace
    vrs::UpdateGaze(centers,now>age?now-age:0);SetState(State::Tracking);
}
void Shutdown(){Clear();if(space)xrDestroySpace(space);space=XR_NULL_HANDLE;action=XR_NULL_HANDLE;supported=false;state.store(State::Unavailable);}
}
