#include <openxr/openxr.h>
#include <cstring>
#include <iostream>
#include <vector>
// Capability query only: no session, action spaces, gaze collection or frame submission.
int main(int argc,char** argv) {
    uint32_t count{};XrResult r=xrEnumerateInstanceExtensionProperties(nullptr,0,&count,nullptr);
    if(XR_FAILED(r)){std::cout<<"enumerate="<<r<<'\n';return 1;}
    std::vector<XrExtensionProperties> props(count,{XR_TYPE_EXTENSION_PROPERTIES});
    r=xrEnumerateInstanceExtensionProperties(nullptr,count,&count,props.data());if(XR_FAILED(r))return 1;
    const char* extension=XR_EXT_EYE_GAZE_INTERACTION_EXTENSION_NAME;bool found=false;
    for(const auto& p:props)if(!std::strcmp(p.extensionName,extension))found=true;
    std::cout<<"XR_EXT_eye_gaze_interaction="<<found<<'\n';if(!found)return 0;
    // Instance creation invokes installed implicit API layers. Keep the default
    // query to enumeration; --system explicitly opts into those layers as well.
    if(argc!=2 || std::strcmp(argv[1],"--system"))return 0;
    XrInstanceCreateInfo info{XR_TYPE_INSTANCE_CREATE_INFO};info.applicationInfo.apiVersion=XR_CURRENT_API_VERSION;
    strcpy_s(info.applicationInfo.applicationName,"CyberpunkVR eye capability query");info.enabledExtensionCount=1;info.enabledExtensionNames=&extension;
    XrInstance instance{};r=xrCreateInstance(&info,&instance);if(XR_FAILED(r)){std::cout<<"create="<<r<<'\n';return 1;}
    XrInstanceProperties runtime{XR_TYPE_INSTANCE_PROPERTIES};xrGetInstanceProperties(instance,&runtime);std::cout<<"runtime="<<runtime.runtimeName<<'\n';
    XrSystemGetInfo get{XR_TYPE_SYSTEM_GET_INFO};get.formFactor=XR_FORM_FACTOR_HEAD_MOUNTED_DISPLAY;XrSystemId system{};
    r=xrGetSystem(instance,&get,&system);
    if(XR_SUCCEEDED(r)) {
        XrSystemEyeGazeInteractionPropertiesEXT gaze{XR_TYPE_SYSTEM_EYE_GAZE_INTERACTION_PROPERTIES_EXT};
        XrSystemProperties sys{XR_TYPE_SYSTEM_PROPERTIES};sys.next=&gaze;r=xrGetSystemProperties(instance,system,&sys);
        if(XR_SUCCEEDED(r))std::cout<<"system="<<sys.systemName<<" eye_gaze_supported="<<gaze.supportsEyeGazeInteraction<<'\n';
    }
    if(XR_FAILED(r))std::cout<<"system_query="<<r<<'\n';
    xrDestroyInstance(instance);return XR_FAILED(r)?1:0;
}
