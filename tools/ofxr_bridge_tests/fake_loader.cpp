// Real loader + real V557, with only the headset runtime replaced by a fixture.
#define main unused_ofxr_fixture_main
#include "tests/layer_call_chain.cpp"
#undef main
XrResult XRAPI_CALL RuntimeCreate(const XrInstanceCreateInfo* info,XrInstance* instance) {
    return fake_create_api_layer_instance(info,nullptr,instance);
}
XrResult XRAPI_CALL RuntimeGipa(XrInstance instance,const char* name,PFN_xrVoidFunction* out) {
    if(!strcmp(name,"xrCreateInstance")) {*out=reinterpret_cast<PFN_xrVoidFunction>(RuntimeCreate);return XR_SUCCESS;}
    return fake_get_instance_proc_addr(instance,name,out);
}
extern "C" __declspec(dllexport) XrResult XRAPI_CALL xrNegotiateLoaderRuntimeInterface(
    const XrNegotiateLoaderInfo*,XrNegotiateRuntimeRequest* request) {
    request->runtimeInterfaceVersion=1;
    request->runtimeApiVersion=XR_MAKE_VERSION(1,0,0);
    request->getInstanceProcAddr=RuntimeGipa;
    return XR_SUCCESS;
}
