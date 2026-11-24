#include "Framegen/DlssFrameGen.hpp"
#include "Core/Paths.hpp"
#include "Utils/DebugGate.hpp"
#include <atomic>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <mutex>
#include <string>
#ifdef CVR_NATIVE_DLSSG
#include <nvsdk_ngx_helpers_dlssg_d3d.h>
#include "Framegen/AnrFrameGen.hpp"
#endif

extern void Log(const char*,...);
namespace cvr::framegen::dlssg {
namespace {
void Multiply(const float* a,const float* b,float* out) {
    float r[16];
    for(int i=0;i<4;++i)for(int j=0;j<4;++j) {float s=0;for(int k=0;k<4;++k)s+=a[i*4+k]*b[k*4+j];r[i*4+j]=s;}
    std::memcpy(out,r,sizeof(r));
}
bool Invert(const float* m,float* out) {
    float inv[16];
    inv[0]=m[5]*m[10]*m[15]-m[5]*m[11]*m[14]-m[9]*m[6]*m[15]+m[9]*m[7]*m[14]+m[13]*m[6]*m[11]-m[13]*m[7]*m[10];
    inv[4]=-m[4]*m[10]*m[15]+m[4]*m[11]*m[14]+m[8]*m[6]*m[15]-m[8]*m[7]*m[14]-m[12]*m[6]*m[11]+m[12]*m[7]*m[10];
    inv[8]=m[4]*m[9]*m[15]-m[4]*m[11]*m[13]-m[8]*m[5]*m[15]+m[8]*m[7]*m[13]+m[12]*m[5]*m[11]-m[12]*m[7]*m[9];
    inv[12]=-m[4]*m[9]*m[14]+m[4]*m[10]*m[13]+m[8]*m[5]*m[14]-m[8]*m[6]*m[13]-m[12]*m[5]*m[10]+m[12]*m[6]*m[9];
    inv[1]=-m[1]*m[10]*m[15]+m[1]*m[11]*m[14]+m[9]*m[2]*m[15]-m[9]*m[3]*m[14]-m[13]*m[2]*m[11]+m[13]*m[3]*m[10];
    inv[5]=m[0]*m[10]*m[15]-m[0]*m[11]*m[14]-m[8]*m[2]*m[15]+m[8]*m[3]*m[14]+m[12]*m[2]*m[11]-m[12]*m[3]*m[10];
    inv[9]=-m[0]*m[9]*m[15]+m[0]*m[11]*m[13]+m[8]*m[1]*m[15]-m[8]*m[3]*m[13]-m[12]*m[1]*m[11]+m[12]*m[3]*m[9];
    inv[13]=m[0]*m[9]*m[14]-m[0]*m[10]*m[13]-m[8]*m[1]*m[14]+m[8]*m[2]*m[13]+m[12]*m[1]*m[10]-m[12]*m[2]*m[9];
    inv[2]=m[1]*m[6]*m[15]-m[1]*m[7]*m[14]-m[5]*m[2]*m[15]+m[5]*m[3]*m[14]+m[13]*m[2]*m[7]-m[13]*m[3]*m[6];
    inv[6]=-m[0]*m[6]*m[15]+m[0]*m[7]*m[14]+m[4]*m[2]*m[15]-m[4]*m[3]*m[14]-m[12]*m[2]*m[7]+m[12]*m[3]*m[6];
    inv[10]=m[0]*m[5]*m[15]-m[0]*m[7]*m[13]-m[4]*m[1]*m[15]+m[4]*m[3]*m[13]+m[12]*m[1]*m[7]-m[12]*m[3]*m[5];
    inv[14]=-m[0]*m[5]*m[14]+m[0]*m[6]*m[13]+m[4]*m[1]*m[14]-m[4]*m[2]*m[13]-m[12]*m[1]*m[6]+m[12]*m[2]*m[5];
    inv[3]=-m[1]*m[6]*m[11]+m[1]*m[7]*m[10]+m[5]*m[2]*m[11]-m[5]*m[3]*m[10]-m[9]*m[2]*m[7]+m[9]*m[3]*m[6];
    inv[7]=m[0]*m[6]*m[11]-m[0]*m[7]*m[10]-m[4]*m[2]*m[11]+m[4]*m[3]*m[10]+m[8]*m[2]*m[7]-m[8]*m[3]*m[6];
    inv[11]=-m[0]*m[5]*m[11]+m[0]*m[7]*m[9]+m[4]*m[1]*m[11]-m[4]*m[3]*m[9]-m[8]*m[1]*m[7]+m[8]*m[3]*m[5];
    inv[15]=m[0]*m[5]*m[10]-m[0]*m[6]*m[9]-m[4]*m[1]*m[10]+m[4]*m[2]*m[9]+m[8]*m[1]*m[6]-m[8]*m[2]*m[5];
    const float det=m[0]*inv[0]+m[1]*inv[4]+m[2]*inv[8]+m[3]*inv[12];
    if(!std::isfinite(det) || std::abs(det)<1e-20f)return false;
    for(int i=0;i<16;++i)out[i]=inv[i]/det;
    return true;
}
void Identity(float* m) {std::memset(m,0,sizeof(float)*16);m[0]=m[5]=m[10]=m[15]=1;}
// World -> view with the camera's own axes as view x/y/z, relative to an origin near the camera
// (world positions in a city are large; this keeps the translation precise).
void View(const CameraData& c,const float* origin,float* m) {
    const float p[3]={c.position[0]-origin[0],c.position[1]-origin[1],c.position[2]-origin[2]};
    const float* r=c.right;const float* u=c.up;const float* f=c.forward;
    const float v[16]={r[0],u[0],f[0],0, r[1],u[1],f[1],0, r[2],u[2],f[2],0,
        -(p[0]*r[0]+p[1]*r[1]+p[2]*r[2]),-(p[0]*u[0]+p[1]*u[1]+p[2]*u[2]),-(p[0]*f[0]+p[1]*f[1]+p[2]*f[2]),1};
    std::memcpy(m,v,sizeof(v));
}
// View -> clip, D3D clip space (x right, y up, depth 0..1), reversed when the engine's depth is.
void Projection(const CameraData& c,float* m) {
    const float ys=1.0f/std::tan(c.verticalFov*.5f),xs=ys/c.aspect,n=c.nearPlane,f=c.farPlane;
    float a{},b{};
    if(c.inverted) {
        if(c.infinite || !std::isfinite(f)) {a=0;b=n;}
        else {a=-n/(f-n);b=n*f/(f-n);}
    } else {
        if(c.infinite || !std::isfinite(f)) {a=1;b=-n;}
        else {a=f/(f-n);b=-n*f/(f-n);}
    }
    // Row vectors: clip.x = xs*x + shift.x*z, so the frustum's axis lands at NDC shift (the engine's own
    // convention for an off-centre window); zero for the centred one.
    const float p[16]={xs,0,0,0, 0,ys,0,0, c.shift[0],c.shift[1],a,1, 0,0,b,0};
    std::memcpy(m,p,sizeof(p));
}
std::mutex mutex;
ID3D12Device* initialized{};
bool available{};
std::string reason="NVIDIA DLSS SDK was not built into this plugin";
std::wstring Directory(HMODULE module) {
    wchar_t path[MAX_PATH]{};GetModuleFileNameW(module,path,MAX_PATH);
    std::wstring result(path);const auto slash=result.find_last_of(L"\\/");
    return slash==std::wstring::npos ? L"." : result.substr(0,slash);
}
#ifdef CVR_NATIVE_DLSSG
void NVSDK_CONV NgxLog(const char* message,NVSDK_NGX_Logging_Level,NVSDK_NGX_Feature) {
    if(message)Log("[ngx] %s",message);
}
#endif
}

bool BuildMatrices(const CameraData& current,const CameraData* previous,Matrices* out) {
    if(!out || !(current.aspect>0) || !(current.verticalFov>0) || !(current.nearPlane>0))return false;
    Projection(current,out->viewToClip);
    if(!Invert(out->viewToClip,out->clipToView))return false;
    if(!previous) {Identity(out->clipToPrevClip);Identity(out->prevClipToClip);return true;}
    float viewNow[16],viewThen[16],projectionThen[16],viewToWorld[16],chain[16];
    View(current,current.position,viewNow);View(*previous,current.position,viewThen);Projection(*previous,projectionThen);
    if(!Invert(viewNow,viewToWorld))return false;
    // Row vectors: clip -> view -> world -> previous view -> previous clip.
    Multiply(out->clipToView,viewToWorld,chain);Multiply(chain,viewThen,chain);Multiply(chain,projectionThen,out->clipToPrevClip);
    return Invert(out->clipToPrevClip,out->prevClipToClip);
}

const char* Reason(Provider provider) {
#ifdef CVR_NATIVE_DLSSG
    if(provider==Provider::Anr)return anr::Reason();
#endif
    std::lock_guard lock(mutex);static thread_local std::string copy;copy=reason;return copy.c_str();
}

#ifdef CVR_NATIVE_DLSSG
struct Feature {
    Provider provider{};
    NVSDK_NGX_Handle* handle{};
    NVSDK_NGX_Parameter* parameters{};
    uint64_t bytes{};
};
void FeatureDeleter::operator()(Feature* feature) const {
    if(!feature)return;
    if(feature->provider==Provider::Anr) {
        if(feature->handle)anr::Release(feature->handle);
        if(feature->parameters)anr::DestroyParameters(feature->parameters);
    } else {
        if(feature->handle)NVSDK_NGX_D3D12_ReleaseFeature(feature->handle);
        if(feature->parameters)NVSDK_NGX_D3D12_DestroyParameters(feature->parameters);
    }
    delete feature;
}
uint64_t Bytes(const Feature& feature) {return feature.bytes;}

bool Available(ID3D12Device* device,Provider provider) {
    if(provider==Provider::Anr)return anr::Available(device);
    std::lock_guard lock(mutex);
    if(!device)return false;
    if(device==initialized)return available;
    initialized=device;available=false;
    // NGX is one instance per adapter for the whole process, shared with the game's own DLSS.
    // It is never shut down here: NVSDK_NGX_D3D12_Shutdown1 would take the game's features down too.
    HMODULE self{};
    GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS|GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
        reinterpret_cast<LPCWSTR>(&Available),&self);
    // nvngx_dlssg.dll ships with the game next to its executable; the plugin folder is searched too.
    const std::wstring game=Directory(nullptr),plugin=Directory(self);
    const wchar_t* paths[]={game.c_str(),plugin.c_str()};
    NVSDK_NGX_FeatureCommonInfo info{};info.PathListInfo.Path=paths;info.PathListInfo.Length=2;
    info.LoggingInfo.LoggingCallback=&NgxLog;
    info.LoggingInfo.MinimumLoggingLevel=cvr::RuntimeDiagnosticsEnabled()?NVSDK_NGX_LOGGING_LEVEL_ON:NVSDK_NGX_LOGGING_LEVEL_OFF;
    // NGX writes its logs to the application data path: the plugin's log folder.
    const auto result=NVSDK_NGX_D3D12_Init_with_ProjectID("5f0c9a3e-7d24-4b8e-9c61-2a7e8d4f1b30",NVSDK_NGX_ENGINE_TYPE_CUSTOM,
        "CyberpunkVR",cvr::paths::Dir(cvr::paths::Folder::Log).c_str(),device,&info);
    if(NVSDK_NGX_FAILED(result)) {
        char text[96];std::snprintf(text,sizeof(text),"NGX initialization failed (0x%08X)",unsigned(result));reason=text;
        Log("[framegen] DLSS-G: %s\n",text);return false;
    }
    NVSDK_NGX_Parameter* caps{};
    if(NVSDK_NGX_FAILED(NVSDK_NGX_D3D12_GetCapabilityParameters(&caps)) || !caps) {reason="NGX capability query failed";return false;}
    int supported=0,needsDriver=0,initResult=0;unsigned frames=0,major=0,minor=0;
    NVSDK_NGX_Parameter_GetI(caps,NVSDK_NGX_Parameter_FrameGeneration_Available,&supported);
    NVSDK_NGX_Parameter_GetI(caps,NVSDK_NGX_Parameter_FrameGeneration_NeedsUpdatedDriver,&needsDriver);
    NVSDK_NGX_Parameter_GetI(caps,NVSDK_NGX_Parameter_FrameGeneration_FeatureInitResult,&initResult);
    NVSDK_NGX_Parameter_GetUI(caps,NVSDK_NGX_DLSSG_Parameter_MultiFrameCountMax,&frames);
    NVSDK_NGX_Parameter_GetUI(caps,NVSDK_NGX_Parameter_FrameGeneration_MinDriverVersionMajor,&major);
    NVSDK_NGX_Parameter_GetUI(caps,NVSDK_NGX_Parameter_FrameGeneration_MinDriverVersionMinor,&minor);
    NVSDK_NGX_D3D12_DestroyParameters(caps);
    Log("[framegen] DLSS-G: available=%d frames=%u init=0x%08X needsDriver=%d minDriver=%u.%u\n",supported,frames,unsigned(initResult),needsDriver,major,minor);
    if(!supported) {
        reason=needsDriver?"DLSS Frame Generation needs a newer NVIDIA driver":"DLSS Frame Generation is not supported on this GPU (RTX 40/50 required)";
        return false;
    }
    available=true;reason.clear();return true;
}

FeaturePtr Create(ID3D12GraphicsCommandList* list,uint32_t width,uint32_t height,uint32_t renderWidth,uint32_t renderHeight,
    DXGI_FORMAT backbuffer,DXGI_FORMAT motion,DXGI_FORMAT depth,Provider provider) {
    FeaturePtr feature(new Feature);feature->provider=provider;
    const bool anrFg=provider==Provider::Anr;
    if(anrFg)feature->parameters=anr::AllocateParameters();
    else if(NVSDK_NGX_FAILED(NVSDK_NGX_D3D12_AllocateParameters(&feature->parameters)))return {};
    NVSDK_NGX_DLSSG_Create_Params create{};create.Width=width;create.Height=height;create.RenderWidth=renderWidth;
    create.RenderHeight=renderHeight;create.NativeBackbufferFormat=backbuffer;create.DynamicResolutionScaling=false;
    const auto result=anrFg ? anr::Create(list,&feature->handle,feature->parameters,&create) :
        NGX_D3D12_CREATE_DLSSG(list,1,1,&feature->handle,feature->parameters,&create);
    if(NVSDK_NGX_FAILED(result) || !feature->handle) {
        Log("[framegen] %s: feature %ux%u (render %ux%u) creation failed 0x%08X\n",anrFg?"ANR FG":"DLSS-G",
            width,height,renderWidth,renderHeight,unsigned(result));
        feature->handle=nullptr;return {};
    }
    if(anrFg) {
        Log("[framegen] ANR FG: feature %ux%u render %ux%u created\n",width,height,renderWidth,renderHeight);
        return feature;
    }
    NVSDK_NGX_Parameter* caps{};
    if(NVSDK_NGX_SUCCEED(NVSDK_NGX_D3D12_GetCapabilityParameters(&caps)) && caps) {
        size_t bytes=0;
        if(NVSDK_NGX_SUCCEED(NGX_D3D12_ESTIMATE_VRAM_DLSSG(caps,renderWidth,renderHeight,width,height,backbuffer,motion,depth,0,0,&bytes)))
            feature->bytes=bytes;
        NVSDK_NGX_D3D12_DestroyParameters(caps);
    }
    Log("[framegen] DLSS-G: feature %ux%u render %ux%u created, estimated %.1f MiB\n",width,height,renderWidth,renderHeight,double(feature->bytes)/1048576.0);
    return feature;
}

bool Evaluate(Feature& feature,ID3D12GraphicsCommandList* list,const Evaluation& e) {
    if(!feature.handle || !e.camera)return false;
    NVSDK_NGX_D3D12_DLSSG_Eval_Params in{};
    in.pBackbuffer=e.backbuffer;in.pDepth=e.depth;in.pMVecs=e.motion;in.pOutputInterpFrame=e.output;
    NVSDK_NGX_DLSSG_Opt_Eval_Params o{};
    Matrices matrices{};
    if(!BuildMatrices(*e.camera,e.reset?nullptr:e.previous,&matrices))return false;
    std::memcpy(o.cameraViewToClip,matrices.viewToClip,64);std::memcpy(o.clipToCameraView,matrices.clipToView,64);
    std::memcpy(o.clipToPrevClip,matrices.clipToPrevClip,64);std::memcpy(o.prevClipToClip,matrices.prevClipToClip,64);
    Identity(&o.clipToLensClip[0][0]);
    const auto& c=*e.camera;
    o.jitterOffset[0]=c.jittered?c.jitter[0]:0;o.jitterOffset[1]=c.jittered?c.jitter[1]:0;
    // Raw engine vector * motionScale is a fraction of the image; NGX wants pixels of the motion grid.
    o.mvecScale[0]=c.motionScale[0]*float(e.motionWidth);o.mvecScale[1]=c.motionScale[1]*float(e.motionHeight);
    std::memcpy(o.cameraPos,c.position,12);std::memcpy(o.cameraUp,c.up,12);
    std::memcpy(o.cameraRight,c.right,12);std::memcpy(o.cameraFwd,c.forward,12);
    o.cameraNear=c.nearPlane;o.cameraFar=c.farPlane;o.cameraFOV=c.verticalFov;o.cameraAspectRatio=c.aspect;
    o.colorBuffersHDR=false;o.depthInverted=c.inverted;o.cameraMotionIncluded=true;o.reset=e.reset;
    o.motionVectorsInvalidValue=3.402823466e+38f;o.motionVectorsDilated=false;o.menuDetectionEnabled=false;
    o.multiFrameCount=1;o.multiFrameIndex=1;
    o.backbufferSubrectSize={e.width,e.height};o.outputInterpSubrectSize={e.width,e.height};
    o.mvecsSubrectSize={e.motionWidth,e.motionHeight};o.depthSubrectSize={e.depthWidth,e.depthHeight};
    if(feature.provider==Provider::Anr) {
        // What OptiScaler's provider proxy sets before every evaluation (Nvngx_DllProxy, its defaults).
        feature.parameters->Set("DLSSG.ShowDebug",0);
        feature.parameters->Set("DLSSG.DispatchFlags",0x10000000u); // IGNORE_UI_TEXTURE
    }
    const auto result=feature.provider==Provider::Anr ? anr::Evaluate(list,feature.handle,feature.parameters,&in,&o) :
        NGX_D3D12_EVALUATE_DLSSG(list,feature.handle,feature.parameters,&in,&o);
    if(NVSDK_NGX_FAILED(result)) {
        static std::atomic<uint32_t> logged{};
        if(logged.fetch_add(1)<4)Log("[framegen] %s: evaluate failed 0x%08X\n",feature.provider==Provider::Anr?"ANR FG":"DLSS-G",unsigned(result));
        return false;
    }
    return true;
}
#else
struct Feature {};
void FeatureDeleter::operator()(Feature* feature) const {delete feature;}
uint64_t Bytes(const Feature&) {return 0;}
bool Available(ID3D12Device*,Provider) {return false;}
FeaturePtr Create(ID3D12GraphicsCommandList*,uint32_t,uint32_t,uint32_t,uint32_t,DXGI_FORMAT,DXGI_FORMAT,DXGI_FORMAT,Provider) {return {};}
bool Evaluate(Feature&,ID3D12GraphicsCommandList*,const Evaluation&) {return false;}
#endif
}
