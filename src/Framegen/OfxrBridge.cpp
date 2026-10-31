#include <windows.h>
#include <d3d12.h>
#include <openxr/openxr.h>
#include <openxr/openxr_platform.h>
#include <openxr/openxr_loader_negotiation.h>
#include <psapi.h>
#include <filesystem>
#include <atomic>
#include <mutex>
#include <cstring>
#include <cstdio>
#include <algorithm>
#include <vector>
#include <string>
#include "Framegen/OfxrBridge.hpp"
#include "Framegen/Framegen.hpp"
#include "xrfg/provider_api.hpp"
extern void Log(const char*,...);

namespace cvr::ofxr {
namespace {
constexpr char layerName[]="XR_APILAYER_XRFrameBridge_diagnostic";
HMODULE loader{},backend{}; // Retained: the backend installs hooks with process lifetime.
PFN_xrGetInstanceProcAddr loaderGipa{},layerGipa{};
std::atomic<XrInstance> instance{XR_NULL_HANDLE};
std::atomic<unsigned> generation{1};
std::atomic<bool> attached{},external{};
std::mutex controlMutex;
using Request=int (*)(const OFXR_ControlSettingsV1*) noexcept;
using Snapshot=int (*)(OFXR_ControlSnapshotV1*) noexcept;
Request request{}; Snapshot snapshot{};
using SetLogging=int (*)(int) noexcept;
using GetLogging=int (*)() noexcept;
SetLogging setLogging{};GetLogging getLogging{},isRecording{};
std::atomic<bool> controlFailed{};
std::mutex createMutex;
std::filesystem::path Directory() {
    HMODULE self{};
    GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS|GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
        reinterpret_cast<LPCWSTR>(&Directory),&self);
    wchar_t path[32768]{};GetModuleFileNameW(self,path,32768);
    return std::filesystem::path(path).parent_path()/L"ofxr-v557";
}
bool Loader() {
    static const bool ready=[] {
        const auto path=Directory()/L"openxr_loader.dll";
        loader=LoadLibraryExW(path.c_str(),nullptr,LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR|LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
        if(loader)loaderGipa=reinterpret_cast<PFN_xrGetInstanceProcAddr>(GetProcAddress(loader,"xrGetInstanceProcAddr"));
        return loaderGipa!=nullptr;
    }();return ready;
}
bool HasExternal() {
    HMODULE modules[2048]{};DWORD bytes{};
    if(!EnumProcessModules(GetCurrentProcess(),modules,sizeof(modules),&bytes))return true;
    for(size_t i=0;i<std::min<size_t>(bytes/sizeof(HMODULE),std::size(modules));++i) {
        wchar_t name[MAX_PATH]{};GetModuleBaseNameW(GetCurrentProcess(),modules[i],name,MAX_PATH);
        if(_wcsnicmp(name,L"XR_APILAYER_XRFrameBridge",23)==0)return true;
    }return false;
}
bool ConfigureLocked(const framegen::Settings& settings) {
    if(!request)return false;
    OFXR_ControlSettingsV1 control{};
    control.enabled=settings.enabled && settings.backend==framegen::Backend::Nvidia && !external.load();
    control.backend=1;control.preset=2-settings.quality;
    control.scale=settings.flowScale==100?0:(settings.flowScale==75?1:2);
    control.backward=0;control.motion_vectors=1;
    bool ok=request(&control)!=0;
    if(setLogging && getLogging && getLogging()!=int(settings.ofxrLogging))ok=setLogging(settings.ofxrLogging?1:0)!=0 && ok;
    controlFailed.store(!ok);return ok;
}
// Discovery is scoped to instance creation. Preserve the user's existing paths;
// no registry edits and no persistent process environment changes.
class LayerPath {
    std::wstring previous;
    bool existed{},changed{};
public:
    LayerPath() {
        const DWORD size=GetEnvironmentVariableW(L"XR_API_LAYER_PATH",nullptr,0);
        existed=size!=0 || GetLastError()!=ERROR_ENVVAR_NOT_FOUND;
        if(size) {previous.resize(size);GetEnvironmentVariableW(L"XR_API_LAYER_PATH",previous.data(),size);previous.resize(size-1);}
        auto path=Directory().wstring();
        if(!previous.empty())path+=L";"+previous;
        changed=SetEnvironmentVariableW(L"XR_API_LAYER_PATH",path.c_str())!=0;
    }
    ~LayerPath() {if(changed)SetEnvironmentVariableW(L"XR_API_LAYER_PATH",existed?previous.c_str():nullptr);}
    bool Ready() const {return changed;}
};
bool ExternalAvailable() {
    if(HasExternal())return true;
    const auto enumerate=reinterpret_cast<PFN_xrEnumerateApiLayerProperties>(GetProcAddress(loader,"xrEnumerateApiLayerProperties"));
    uint32_t count{};
    if(!enumerate || XR_FAILED(enumerate(0,&count,nullptr)))return true;
    std::vector<XrApiLayerProperties> layers(count,{XR_TYPE_API_LAYER_PROPERTIES});
    if(count && XR_FAILED(enumerate(count,&count,layers.data())))return true;
    // Conservative exclusion: an installed OFXR manifest belongs to the external
    // installation. Never silently load that DLL in place of our private copy.
    for(const auto& layer:layers)
        if(std::strncmp(layer.layerName,"XR_APILAYER_XRFrameBridge",24)==0)return true;
    return false;
}
}
bool NativeAllowed() {return !attached.load();}
int RecordingStatus() {std::lock_guard lock(controlMutex);return attached.load() && isRecording?isRecording():-1;}
void Configure(const framegen::Settings& settings) {
    std::lock_guard lock(controlMutex);
    if(request && !ConfigureLocked(settings))Log("[ofxr-v557] failed to apply private backend settings\n");
}
framegen::RuntimeStatus Status() {
    framegen::RuntimeStatus out{};
    if(controlFailed.load())std::snprintf(out.text,sizeof(out.text),"V557 settings/logging write failed. Check backend folder permissions and cyberpunkvrport.log.");
    else if(external.load())std::snprintf(out.text,sizeof(out.text),"External OFXR is loaded. Disarm its tray and restart the game to use built-in V557.");
    else if(!attached.load())std::snprintf(out.text,sizeof(out.text),"V557 backend not attached; restart the OpenXR runtime after installation.");
    else {
        OFXR_ControlSnapshotV1 value{};
        std::lock_guard lock(controlMutex);
        if(snapshot && snapshot(&value)) {
            out.active=value.ready>0;
            std::snprintf(out.text,sizeof(out.text),"OFXR V557: %s (2X, separate panel synthesis off)",
                value.errors?"backend error":value.pending?"applying settings":value.ready?"enabled": "disabled");
        }
    }return out;
}
// A separate cache per wrapper/thread. Instance recreation invalidates cached dispatch;
// steady calls do not take a map lock or resolve symbols on the render thread.
template<class T> struct DispatchCache {unsigned epoch{};T function{};};
template<class T> T Resolve(const char* name,DispatchCache<T>& cache) {
    const auto epoch=generation.load(std::memory_order_acquire);
    if(cache.epoch!=epoch) {
        PFN_xrVoidFunction function{};
        if(Loader()) {
            auto gipa=attached.load()?layerGipa:loaderGipa;
            gipa(instance.load(),name,&function);
        }
        cache.function=reinterpret_cast<T>(function);cache.epoch=epoch;
    }return cache.function;
}
XrResult Create(const XrInstanceCreateInfo* info,XrInstance* out) {
    if(!info || !out)return XR_ERROR_VALIDATION_FAILURE;
    if(!Loader())return XR_ERROR_INITIALIZATION_FAILED;
    const auto create=reinterpret_cast<PFN_xrCreateInstance>(GetProcAddress(loader,"xrCreateInstance"));
    if(!create)return XR_ERROR_FUNCTION_UNSUPPORTED;
    std::lock_guard creationLock(createMutex);
    if(instance.load()!=XR_NULL_HANDLE)return XR_ERROR_LIMIT_REACHED;
    *out=XR_NULL_HANDLE;
    external.store(ExternalAvailable());
    if(external.load() || framegen::GetSettings().backend!=framegen::Backend::Nvidia) {
        const auto result=create(info,out);
        if(XR_SUCCEEDED(result)) {instance.store(*out);generation.fetch_add(1,std::memory_order_release);}
        Log("[ofxr-v557] %s; private backend bypassed\n",external.load()?"external OFXR available":"native generator selected");
        return result;
    }
    std::lock_guard lock(controlMutex);
    const auto path=Directory()/L"CyberpunkVR_OFXR.dll";
    if(!backend)backend=LoadLibraryExW(path.c_str(),nullptr,LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR|LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    request=backend?reinterpret_cast<Request>(GetProcAddress(backend,"OFXR_RequestControlV1")):nullptr;
    snapshot=backend?reinterpret_cast<Snapshot>(GetProcAddress(backend,"OFXR_GetControlSnapshotV1")):nullptr;
    setLogging=backend?reinterpret_cast<SetLogging>(GetProcAddress(backend,"OFXR_SetLoggingSettingV1")):nullptr;
    getLogging=backend?reinterpret_cast<GetLogging>(GetProcAddress(backend,"OFXR_GetLoggingSettingV1")):nullptr;
    isRecording=backend?reinterpret_cast<GetLogging>(GetProcAddress(backend,"OFXR_IsFlightRecordingV1")):nullptr;
    if(!backend || !ConfigureLocked(framegen::GetSettings()))return XR_ERROR_INITIALIZATION_FAILED;
    std::vector<const char*> names;
    for(uint32_t i=0;i<info->enabledApiLayerCount;++i)names.push_back(info->enabledApiLayerNames[i]);
    names.push_back(layerName);
    auto forwarded=*info;forwarded.enabledApiLayerCount=static_cast<uint32_t>(names.size());
    forwarded.enabledApiLayerNames=names.data();
    XrResult result;
    {
        LayerPath search;
        if(!search.Ready())return XR_ERROR_INITIALIZATION_FAILED;
        result=create(&forwarded,out);
    }
    if(XR_SUCCEEDED(result)) {
        // The loader owns the full chain. Implicit observer layers stay above
        // private OFXR; generated submissions go only to layers below OFXR.
        layerGipa=loaderGipa;instance.store(*out);attached.store(true);
        generation.fetch_add(1,std::memory_order_release);
        Log("[ofxr-v557] private patched V557 attached through loader-managed layer chain\n");
    } else {
        *out=XR_NULL_HANDLE;
        Log("[ofxr-v557] loader-managed attachment failed: %d (Win32 %lu)\n",int(result),GetLastError());
    }
    return result;
}
XrResult GetProc(XrInstance inst,const char* name,PFN_xrVoidFunction* out) {
    if(!name || !out)return XR_ERROR_VALIDATION_FAILURE;
    *out=nullptr;
    if(!Loader())return XR_ERROR_INITIALIZATION_FAILED;
    return (attached.load() && inst!=XR_NULL_HANDLE?layerGipa:loaderGipa)(inst,name,out);
}
XrResult Destroy(XrInstance inst) {
    PFN_xrVoidFunction fn{};auto result=GetProc(inst,"xrDestroyInstance",&fn);
    if(XR_SUCCEEDED(result)&&fn)result=reinterpret_cast<PFN_xrDestroyInstance>(fn)(inst);
    if(XR_SUCCEEDED(result)){attached.store(false);instance.store(XR_NULL_HANDLE);generation.fetch_add(1,std::memory_order_release);}
    return result;
}
}
extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrCreateInstance(const XrInstanceCreateInfo* info,XrInstance* out) {return cvr::ofxr::Create(info,out);}
extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrGetInstanceProcAddr(XrInstance inst,const char* name,PFN_xrVoidFunction* out) {return cvr::ofxr::GetProc(inst,name,out);}
extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrDestroyInstance(XrInstance inst) {return cvr::ofxr::Destroy(inst);}
#include "OfxrDispatch.inl"
