#include <windows.h>
#include <openxr/openxr.h>
#include <openxr/openxr_loader_negotiation.h>
#include <cstring>
namespace {
PFN_xrGetInstanceProcAddr downstream{};
unsigned observed{};
XrResult XRAPI_CALL Properties(XrInstance instance,XrInstanceProperties* properties) {
    ++observed;
    PFN_xrVoidFunction fn{};
    auto result=downstream(instance,"xrGetInstanceProperties",&fn);
    return XR_FAILED(result)?result:reinterpret_cast<PFN_xrGetInstanceProperties>(fn)(instance,properties);
}
XrResult XRAPI_CALL Gipa(XrInstance instance,const char* name,PFN_xrVoidFunction* out) {
    if(!strcmp(name,"xrGetInstanceProperties")) {*out=reinterpret_cast<PFN_xrVoidFunction>(Properties);return XR_SUCCESS;}
    if(!downstream) {*out=nullptr;return XR_ERROR_FUNCTION_UNSUPPORTED;}
    return downstream(instance,name,out);
}
XrResult XRAPI_CALL Create(const XrInstanceCreateInfo* info,const XrApiLayerCreateInfo* chain,XrInstance* instance) {
    observed=0;
    downstream=chain->nextInfo->nextGetInstanceProcAddr;
    auto create=chain->nextInfo->nextCreateApiLayerInstance;
    auto next=*chain;next.nextInfo=chain->nextInfo->next;
    return create(info,&next,instance);
}
}
extern "C" __declspec(dllexport) unsigned ObserverCalls() {return observed;}
extern "C" __declspec(dllexport) XrResult XRAPI_CALL xrNegotiateLoaderApiLayerInterface(
    const XrNegotiateLoaderInfo*,const char*,XrNegotiateApiLayerRequest* out) {
    out->layerInterfaceVersion=1;out->layerApiVersion=XR_MAKE_VERSION(1,0,0);
    out->getInstanceProcAddr=Gipa;out->createApiLayerInstance=Create;
    return XR_SUCCESS;
}
