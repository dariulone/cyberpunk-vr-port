#include "Framegen/AnrFrameGen.hpp"
#ifdef CVR_NATIVE_DLSSG
#include "Core/Paths.hpp"
#include "Utils/DebugGate.hpp"
#include <windows.h>
#include <cstdio>
#include <mutex>
#include <string>
#include <type_traits>
#include <unordered_map>

extern void Log(const char*,...);
namespace cvr::framegen::anr {
namespace {
// NGX's parameter table semantics as OptiScaler implements them (NVNGX_Parameter.h): a value read as another numeric
// type is converted, a pointer is read only as a pointer (or as an unsigned 64-bit integer), a missing key fails.
class Parameters final : public NVSDK_NGX_Parameter {
    enum class Kind:unsigned char {Ull,F,D,Ui,I,Ptr};
    struct Value {Kind kind;union {unsigned long long ull;float f;double d;unsigned int ui;int i;void* p;};};
    std::unordered_map<std::string,Value> values;
    Value& Put(const char* name,Kind kind) {auto& v=values[name ? name : ""];v.kind=kind;return v;}
    const Value* Find(const char* name) const {const auto it=values.find(name ? name : "");return it==values.end()?nullptr:&it->second;}
    template<class T> NVSDK_NGX_Result Number(const char* name,T* out) const {
        const auto* v=Find(name);
        if(!v || !out)return NVSDK_NGX_Result_Fail;
        switch(v->kind) {
            case Kind::Ull:*out=T(v->ull);break;case Kind::F:*out=T(v->f);break;case Kind::D:*out=T(v->d);break;
            case Kind::Ui:*out=T(v->ui);break;case Kind::I:*out=T(v->i);break;
            case Kind::Ptr:
                if constexpr(std::is_same_v<T,unsigned long long>) {*out=reinterpret_cast<unsigned long long>(v->p);break;}
                else return NVSDK_NGX_Result_Fail;
        }
        return NVSDK_NGX_Result_Success;
    }
    template<class T> NVSDK_NGX_Result Pointer(const char* name,T** out) const {
        const auto* v=Find(name);
        if(!v || !out || v->kind!=Kind::Ptr)return NVSDK_NGX_Result_Fail;
        *out=static_cast<T*>(v->p);return NVSDK_NGX_Result_Success;
    }
public:
    void Set(const char* n,unsigned long long v) override {Put(n,Kind::Ull).ull=v;}
    void Set(const char* n,float v) override {Put(n,Kind::F).f=v;}
    void Set(const char* n,double v) override {Put(n,Kind::D).d=v;}
    void Set(const char* n,unsigned int v) override {Put(n,Kind::Ui).ui=v;}
    void Set(const char* n,int v) override {Put(n,Kind::I).i=v;}
    void Set(const char* n,ID3D11Resource* v) override {Put(n,Kind::Ptr).p=v;}
    void Set(const char* n,ID3D12Resource* v) override {Put(n,Kind::Ptr).p=v;}
    void Set(const char* n,void* v) override {Put(n,Kind::Ptr).p=v;}
    NVSDK_NGX_Result Get(const char* n,unsigned long long* v) const override {return Number(n,v);}
    NVSDK_NGX_Result Get(const char* n,float* v) const override {return Number(n,v);}
    NVSDK_NGX_Result Get(const char* n,double* v) const override {return Number(n,v);}
    NVSDK_NGX_Result Get(const char* n,unsigned int* v) const override {return Number(n,v);}
    NVSDK_NGX_Result Get(const char* n,int* v) const override {return Number(n,v);}
    NVSDK_NGX_Result Get(const char* n,ID3D11Resource** v) const override {return Pointer(n,v);}
    NVSDK_NGX_Result Get(const char* n,ID3D12Resource** v) const override {return Pointer(n,v);}
    NVSDK_NGX_Result Get(const char* n,void** v) const override {return Pointer(n,v);}
    void Reset() override {values.clear();}
};
// The provider's NGX exports, the ones OptiScaler's Nvngx_DllProxy takes (public NGX signatures).
using InitExtFn=NVSDK_NGX_Result(NVSDK_CONV*)(unsigned long long,const wchar_t*,ID3D12Device*,NVSDK_NGX_Version,const NVSDK_NGX_FeatureCommonInfo*);
using CreateFn=NVSDK_NGX_Result(NVSDK_CONV*)(ID3D12GraphicsCommandList*,NVSDK_NGX_Feature,NVSDK_NGX_Parameter*,NVSDK_NGX_Handle**);
using EvaluateFn=NVSDK_NGX_Result(NVSDK_CONV*)(ID3D12GraphicsCommandList*,const NVSDK_NGX_Handle*,NVSDK_NGX_Parameter*,PFN_NVSDK_NGX_ProgressCallback);
using ReleaseFn=NVSDK_NGX_Result(NVSDK_CONV*)(NVSDK_NGX_Handle*);
using PopulateFn=NVSDK_NGX_Result(NVSDK_CONV*)(NVSDK_NGX_Parameter*);
std::mutex mutex;
HMODULE dll{};
InitExtFn initExt{};CreateFn create{};EvaluateFn evaluate{};ReleaseFn release{};PopulateFn populate{};
ID3D12Device* initialized{};
bool available{};
std::string reason="ANR FG was not looked for yet";
std::wstring PluginDirectory() {
    HMODULE self{};
    GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS|GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
        reinterpret_cast<LPCWSTR>(&PluginDirectory),&self);
    wchar_t path[MAX_PATH]{};GetModuleFileNameW(self,path,MAX_PATH);
    std::wstring result(path);const auto slash=result.find_last_of(L"\\/");
    return slash==std::wstring::npos ? L"." : result.substr(0,slash);
}
void NVSDK_CONV ProviderLog(const char* message,NVSDK_NGX_Logging_Level,NVSDK_NGX_Feature) {
    if(message)Log("[anr-fg] %s",message);
}
bool Load() {
    if(dll)return true;
    // As Nvngx_Custom loads it: the dll in the package's dlssg folder, the files it reads beside it.
    const std::wstring folder=PluginDirectory()+L"\\anr\\dlssg",path=folder+L"\\nvngx_dlssg.dll";
    if(GetFileAttributesW(path.c_str())==INVALID_FILE_ATTRIBUTES) {reason="ANR FG is not installed (anr\\dlssg\\nvngx_dlssg.dll beside the plugin)";return false;}
    dll=LoadLibraryExW(path.c_str(),nullptr,LOAD_WITH_ALTERED_SEARCH_PATH);
    if(!dll) {
        char text[96];std::snprintf(text,sizeof(text),"ANR FG could not be loaded (error %lu)",GetLastError());reason=text;return false;
    }
    initExt=reinterpret_cast<InitExtFn>(GetProcAddress(dll,"NVSDK_NGX_D3D12_Init_Ext"));
    create=reinterpret_cast<CreateFn>(GetProcAddress(dll,"NVSDK_NGX_D3D12_CreateFeature"));
    evaluate=reinterpret_cast<EvaluateFn>(GetProcAddress(dll,"NVSDK_NGX_D3D12_EvaluateFeature"));
    release=reinterpret_cast<ReleaseFn>(GetProcAddress(dll,"NVSDK_NGX_D3D12_ReleaseFeature"));
    populate=reinterpret_cast<PopulateFn>(GetProcAddress(dll,"NVSDK_NGX_D3D12_PopulateParameters_Impl"));
    if(!initExt || !create || !evaluate || !release) {
        reason="ANR FG's nvngx_dlssg.dll lacks the NGX D3D12 entry points";
        FreeLibrary(dll);dll=nullptr;return false;
    }
    return true;
}
}

bool Available(ID3D12Device* device) {
    std::lock_guard lock(mutex);
    if(!device)return false;
    if(device==initialized)return available;
    initialized=device;available=false;
    if(!Load()) {Log("[framegen] ANR FG: %s\n",reason.c_str());return false;}
    // OptiScaler forwards the game's own D3D12_Init_Ext with its feature info: the feature dll's folder first.
    const std::wstring folder=PluginDirectory()+L"\\anr\\dlssg";
    const wchar_t* paths[]={folder.c_str()};
    NVSDK_NGX_FeatureCommonInfo info{};info.PathListInfo.Path=paths;info.PathListInfo.Length=1;
    info.LoggingInfo.LoggingCallback=&ProviderLog;
    info.LoggingInfo.MinimumLoggingLevel=cvr::RuntimeDiagnosticsEnabled()?NVSDK_NGX_LOGGING_LEVEL_ON:NVSDK_NGX_LOGGING_LEVEL_OFF;
    const auto result=initExt(0x43795650ull,cvr::paths::Dir(cvr::paths::Folder::Log).c_str(),device,NVSDK_NGX_Version_API,&info);
    if(NVSDK_NGX_FAILED(result)) {
        char text[96];std::snprintf(text,sizeof(text),"ANR FG initialization failed (0x%08X)",unsigned(result));reason=text;
        Log("[framegen] %s\n",text);return false;
    }
    int supported=1;unsigned frames=0;
    if(populate) {
        Parameters caps;
        if(NVSDK_NGX_SUCCEED(populate(&caps))) {
            if(NVSDK_NGX_FAILED(caps.Get(NVSDK_NGX_Parameter_FrameGeneration_Available,&supported)))supported=1;
            caps.Get(NVSDK_NGX_DLSSG_Parameter_MultiFrameCountMax,&frames);
        }
    }
    Log("[framegen] ANR FG: loaded, available=%d frames=%u\n",supported,frames);
    if(!supported) {reason="ANR FG reports frame generation unavailable on this GPU";return false;}
    available=true;reason.clear();return true;
}
const char* Reason() {std::lock_guard lock(mutex);static thread_local std::string copy;copy=reason;return copy.c_str();}
NVSDK_NGX_Parameter* AllocateParameters() {return new Parameters;}
void DestroyParameters(NVSDK_NGX_Parameter* p) {delete static_cast<Parameters*>(p);}
NVSDK_NGX_Result Release(NVSDK_NGX_Handle* handle) {return release && handle ? release(handle) : NVSDK_NGX_Result_Fail;}
// The feature calls the DLSS SDK's helpers make at the end of this file: the provider's exports.
NVSDK_NGX_Result CreateFeature(ID3D12GraphicsCommandList* list,NVSDK_NGX_Feature feature,NVSDK_NGX_Parameter* p,NVSDK_NGX_Handle** handle) {
    return create ? create(list,feature,p,handle) : NVSDK_NGX_Result_Fail;
}
NVSDK_NGX_Result EvaluateFeature(ID3D12GraphicsCommandList* list,const NVSDK_NGX_Handle* handle,NVSDK_NGX_Parameter* p,PFN_NVSDK_NGX_ProgressCallback callback) {
    return evaluate ? evaluate(list,handle,p,callback) : NVSDK_NGX_Result_Fail;
}
}

// THE DLSS SDK'S OWN DLSS-G HELPERS, with their parameter setters and feature calls bound to the table and the provider
// above: the same keys and values NVIDIA's DLSS-G gets from NGX_D3D12_CREATE_DLSSG / NGX_D3D12_EVALUATE_DLSSG.
#define NVSDK_NGX_Parameter_SetUI(p,n,v) (p)->Set((n),static_cast<unsigned int>(v))
#define NVSDK_NGX_Parameter_SetF(p,n,v) (p)->Set((n),static_cast<float>(v))
#define NVSDK_NGX_Parameter_SetD3d12Resource(p,n,v) (p)->Set((n),static_cast<ID3D12Resource*>(v))
#define NVSDK_NGX_Parameter_SetVoidPointer(p,n,v) (p)->Set((n),static_cast<void*>(v))
#define NVSDK_NGX_Parameter_GetVoidPointer(p,n,v) (p)->Get((n),(v))
#define NVSDK_NGX_D3D12_CreateFeature cvr::framegen::anr::CreateFeature
#define NVSDK_NGX_D3D12_EvaluateFeature_C cvr::framegen::anr::EvaluateFeature
#include <nvsdk_ngx_helpers_dlssg_d3d.h>

namespace cvr::framegen::anr {
NVSDK_NGX_Result Create(ID3D12GraphicsCommandList* list,NVSDK_NGX_Handle** handle,NVSDK_NGX_Parameter* p,NVSDK_NGX_DLSSG_Create_Params* params) {
    return NGX_D3D12_CREATE_DLSSG(list,1,1,handle,p,params);
}
NVSDK_NGX_Result Evaluate(ID3D12GraphicsCommandList* list,NVSDK_NGX_Handle* handle,NVSDK_NGX_Parameter* p,
    NVSDK_NGX_D3D12_DLSSG_Eval_Params* in,NVSDK_NGX_DLSSG_Opt_Eval_Params* o) {
    return NGX_D3D12_EVALUATE_DLSSG(list,handle,p,in,o);
}
}
#endif
