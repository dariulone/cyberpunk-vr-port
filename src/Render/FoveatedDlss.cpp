#include "Render/FoveatedDlss.hpp"
#include "Render/FoveatedGpu.hpp"
#include "Render/FoveatedNgx.hpp"
#include "Render/SharpenPolicy.hpp"
#include "Render/CommandResources.hpp"
#include "Render/RenderProbeScope.hpp"
#include "Camera/PoseIdentity.hpp"
#include "Hooks/FarInlineHook.hpp"
#include "Stereo/EngineStereo.hpp"
#include "Stereo/EngineStereoDlss.hpp"
#include <MinHook.h>
#include <json/json.h>
#include <algorithm>
#include <array>
#include <atomic>
#include <cmath>
#include <mutex>
#include <unordered_map>

extern void Log(const char*,...);
extern "C" int GetMenuMode();
extern "C" {
__declspec(dllexport) char CyberpunkVR_FoveatedReport[32768]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_FoveatedReportSeq{};
}
namespace cvr::foveated {
using Microsoft::WRL::ComPtr;
namespace {
std::mutex mutex,statsMutex,timingMutex;
Status status;
std::atomic<bool> installed{};
thread_local bool internal{};
struct Recursion { bool previous=internal; Recursion(){internal=true;} ~Recursion(){internal=previous;} };
struct Route { ngx::Evaluate evaluate{};ngx::Create create{};ngx::Release release{};void* target{}; };
std::array<Route,2> routes;
constexpr const char* presetKeys[]{"DLSS.Hint.Render.Preset.DLAA","DLSS.Hint.Render.Preset.Quality",
    "DLSS.Hint.Render.Preset.Balanced","DLSS.Hint.Render.Preset.Performance",
    "DLSS.Hint.Render.Preset.UltraPerformance","DLSS.Hint.Render.Preset.UltraQuality"};
ngx::CreationRegistry gameFeatures;
ngx::Create originalCreate{};
std::atomic<uint64_t> calls{},active{},failed{},nextInstall{},lastCall{};
std::mutex retiredMutex;
std::vector<std::pair<ngx::Release,ngx::Handle*>> retiredFeatures;

class Owned : public IUnknown {
    std::atomic<ULONG> refs{1};
public:
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** out) override {
        if(!out)return E_POINTER;*out=nullptr;if(id!=__uuidof(IUnknown))return E_NOINTERFACE;
        *out=this;AddRef();return S_OK;
    }
    ULONG STDMETHODCALLTYPE AddRef() override{return ++refs;}
    ULONG STDMETHODCALLTYPE Release() override{const auto n=--refs;if(!n)delete this;return n;}
    virtual ~Owned()=default;
};
struct Timed : Owned {
    ComPtr<ID3D12Device> device;
    ComPtr<ID3D12QueryHeap> queries;
    ComPtr<ID3D12Resource> readback;
    std::atomic<uint64_t> frequency{};
    unsigned count{};bool native{};
    ~Timed() override {
        const auto f=frequency.load();if(!f || !count || FAILED(device->GetDeviceRemovedReason()))return;
        void* data{};D3D12_RANGE range{0,count*sizeof(uint64_t)};
        if(FAILED(readback->Map(0,&range,&data)) || !data)return;
        const auto* ticks=static_cast<const uint64_t*>(data);bool valid=true;
        for(unsigned i=1;i<count;++i)valid &= ticks[i]>=ticks[i-1] && ticks[i]-ticks[i-1]<f;
        if(valid) {
            auto ms=[&](unsigned a,unsigned b){return float(double(ticks[b]-ticks[a])*1000.0/f);};
            std::lock_guard lock(statsMutex);
            if(native)status.nativeMs=ms(0,1);
            else {status.peripheralMs=ms(0,1);status.foveatedMs=ms(1,2);status.totalMs=ms(0,3);}
        }
        D3D12_RANGE none{};readback->Unmap(0,&none);
    }
};
std::unordered_map<ID3D12CommandList*,std::vector<ComPtr<Timed>>> timings;
ComPtr<Timed> Timer(ID3D12GraphicsCommandList* list,bool native) {
    // Sample one in thirty calls; timing must not become a per-frame allocator load.
    if(calls.load()%30 || (native && !active.load()))return {};
    ComPtr<Timed> t;t.Attach(new Timed);t->native=native;
    if(FAILED(list->GetDevice(IID_PPV_ARGS(&t->device))))return {};
    D3D12_QUERY_HEAP_DESC query{};query.Type=D3D12_QUERY_HEAP_TYPE_TIMESTAMP;query.Count=4;
    D3D12_RESOURCE_DESC d{};d.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;d.Width=32;
    d.Height=1;d.DepthOrArraySize=1;d.MipLevels=1;d.SampleDesc.Count=1;d.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_READBACK;
    if(FAILED(t->device->CreateQueryHeap(&query,IID_PPV_ARGS(&t->queries))) ||
        FAILED(t->device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&d,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&t->readback))) ||
        !cvr::gpu::KeepCommandResources(list,{t.Get()}))return {};
    list->EndQuery(t->queries.Get(),D3D12_QUERY_TYPE_TIMESTAMP,0);return t;
}
void Stamp(ID3D12GraphicsCommandList* list,Timed* t,unsigned index) {if(t)list->EndQuery(t->queries.Get(),D3D12_QUERY_TYPE_TIMESTAMP,index);}
void FinishTimer(ID3D12GraphicsCommandList* list,const ComPtr<Timed>& t,unsigned count) {
    if(!t)return;t->count=count;
    list->ResolveQueryData(t->queries.Get(),D3D12_QUERY_TYPE_TIMESTAMP,0,count,t->readback.Get(),0);
    std::lock_guard lock(timingMutex);timings[list].push_back(t);
}
struct State : Owned {
    FoveatedGpu gpu;
    ComPtr<ID3D12Device> device;
    ngx::Handle* handles[2]{};
    ngx::Release release{};
    const ngx::Handle* original{};
    unsigned renderW{},renderH{},outputW{},outputH{},flags{},quality{};
    std::array<unsigned,6> presets{};
    Settings settings;
    uint64_t evaluations{},resets{};
    bool first=true;
    ~State() override {
        // Destruction can occur under the command-resource registry's lock.
        // Defer NGX callbacks to Present to avoid re-entering D3D from that lock.
        std::lock_guard lock(retiredMutex);
        for(auto* handle:handles)if(handle && release)retiredFeatures.emplace_back(release,handle);
    }
};
std::array<ComPtr<State>,2> views;
std::array<bool,2> nativeReset{};
std::array<uint64_t,2> viewCalls{};

void Message(const char* message) {std::lock_guard lock(statsMutex);strncpy_s(status.text,message,_TRUNCATE);}
bool TextureBounds(ID3D12Resource* resource,unsigned x,unsigned y,unsigned w,unsigned h) {
    if(!resource || !w || !h)return false;const auto d=resource->GetDesc();
    return d.Dimension==D3D12_RESOURCE_DIMENSION_TEXTURE2D && d.DepthOrArraySize==1 &&
        d.MipLevels==1 && d.SampleDesc.Count==1 && x<=d.Width && w<=d.Width-x && y<=d.Height && h<=d.Height-y;
}
bool ReadSource(const ngx::Parameters* p,Source& s,unsigned& flags,unsigned& quality) {
    s.color=ngx::Read<ID3D12Resource*>(p,"Color");s.depth=ngx::Read<ID3D12Resource*>(p,"Depth");
    s.motion=ngx::Read<ID3D12Resource*>(p,"MotionVectors");s.output=ngx::Read<ID3D12Resource*>(p,"Output");
    s.renderW=ngx::Integer(p,"DLSS.Render.Subrect.Dimensions.Width",ngx::Integer(p,"Width"));
    s.renderH=ngx::Integer(p,"DLSS.Render.Subrect.Dimensions.Height",ngx::Integer(p,"Height"));
    s.outputW=ngx::Integer(p,"OutWidth");s.outputH=ngx::Integer(p,"OutHeight");
    s.colorX=ngx::Integer(p,"DLSS.Input.Color.Subrect.Base.X");s.colorY=ngx::Integer(p,"DLSS.Input.Color.Subrect.Base.Y");
    s.depthX=ngx::Integer(p,"DLSS.Input.Depth.Subrect.Base.X");s.depthY=ngx::Integer(p,"DLSS.Input.Depth.Subrect.Base.Y");
    s.motionX=ngx::Integer(p,"DLSS.Input.MV.Subrect.Base.X");s.motionY=ngx::Integer(p,"DLSS.Input.MV.Subrect.Base.Y");
    flags=ngx::Integer(p,"DLSS.Feature.Create.Flags");quality=ngx::Integer(p,"PerfQualityValue");
    s.lowMotion=(flags&2)!=0;s.inverted=(flags&8)!=0;
    s.motionScaleX=ngx::Read<float>(p,"MV.Scale.X",1);s.motionScaleY=ngx::Read<float>(p,"MV.Scale.Y",1);
    return s.renderW>=64 && s.renderH>=64 && s.outputW<=16384 && s.outputH<=16384 &&
        s.outputW>=s.renderW && s.outputH>=s.renderH && std::isfinite(s.motionScaleX) && std::isfinite(s.motionScaleY) &&
        ngx::Integer(p,"DLSS.Output.Subrect.Base.X")==0 && ngx::Integer(p,"DLSS.Output.Subrect.Base.Y")==0 &&
        TextureBounds(s.color,s.colorX,s.colorY,s.renderW,s.renderH) && TextureBounds(s.depth,s.depthX,s.depthY,s.renderW,s.renderH) &&
        TextureBounds(s.motion,s.motionX,s.motionY,s.lowMotion?s.renderW:s.outputW,s.lowMotion?s.renderH:s.outputH) &&
        TextureBounds(s.output,0,0,s.outputW,s.outputH) && (s.output->GetDesc().Flags&D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS);
}
unsigned Dimension(float v) {return std::max(32u,static_cast<unsigned>(std::lround(v)));}
ComPtr<State> CreateState(ID3D12GraphicsCommandList* list,const Source& s,Settings settings,const ngx::Handle* original,
                         const ngx::FeatureConfig& config,const Route& route) {
    ComPtr<State> state;state.Attach(new State);
    if(FAILED(list->GetDevice(IID_PPV_ARGS(&state->device))))return {};
    const Region center=CenterRegion(settings,s.renderW,s.renderH,s.outputW,s.outputH);
    const float scales[]{1,1,.75f,.5f};
    Region peripheral{};peripheral.width=peripheral.inputW=Dimension(s.renderW*scales[settings.mode]);
    peripheral.height=peripheral.inputH=Dimension(s.renderH*scales[settings.mode]);
    if(!state->gpu.Initialize(state->device.Get(),center,peripheral))return {};
    state->settings=settings;state->original=original;state->flags=config.flags;state->quality=config.quality;
    state->presets=config.presets;
    state->renderW=s.renderW;state->renderH=s.renderH;state->outputW=s.outputW;state->outputH=s.outputH;state->release=route.release;
    Log("[foveated-native] center %ux%u -> %ux%u at %u,%u quality=%u peripheral=%ux%u\n",
        center.inputW,center.inputH,center.width,center.height,center.x,center.y,config.quality,peripheral.width,peripheral.height);
    return state;
}
ngx::Result EvaluateRegion(ID3D12GraphicsCommandList* list,ngx::Parameters* p,const Source& source,State& state,
                          const Route& route,bool central,bool reset) {
    auto& pass=central?state.gpu.center:state.gpu.periphery;const auto& r=pass.region;
    if(!state.gpu.Prepare(list,source,central))return 0xBAD00005;
    ngx::Override change(p);
    change.Set("Color",pass.color.Get());change.Set("Depth",pass.depth.Get());change.Set("MotionVectors",pass.motion.Get());change.Set("Output",pass.output.Get());
    change.Set("Width",r.inputW);change.Set("Height",r.inputH);change.Set("OutWidth",r.width);change.Set("OutHeight",r.height);
    change.Set("DLSS.Render.Subrect.Dimensions.Width",r.inputW);change.Set("DLSS.Render.Subrect.Dimensions.Height",r.inputH);
    for(const char* key:{"DLSS.Input.Color.Subrect.Base.X","DLSS.Input.Color.Subrect.Base.Y","DLSS.Input.Depth.Subrect.Base.X",
        "DLSS.Input.Depth.Subrect.Base.Y","DLSS.Input.MV.Subrect.Base.X","DLSS.Input.MV.Subrect.Base.Y",
        "DLSS.Output.Subrect.Base.X","DLSS.Output.Subrect.Base.Y","DLSS.Enable.Output.Subrects"})change.Set(key,0u);
    change.Set("PerfQualityValue",central?state.quality:5u);change.Set("DLSS.Feature.Create.Flags",state.flags|2u);
    for(unsigned i=0;i<6;++i)change.Set(presetKeys[i],central?state.presets[i]:5u);
    change.Set("MV.Scale.X",1.0f);change.Set("MV.Scale.Y",1.0f);
    if(!central) {
        change.Set("Jitter.Offset.X",ngx::Read<float>(p,"Jitter.Offset.X")*r.inputW/source.renderW);
        change.Set("Jitter.Offset.Y",ngx::Read<float>(p,"Jitter.Offset.Y")*r.inputH/source.renderH);
        change.Set("DLSS.Hint.Render.Preset.DLAA",5u);
    }
    change.Set("Reset",reset?1u:0u);
    auto*& handle=state.handles[central?0:1];
    if(!handle) {
        const auto result=route.create(list,1,p,&handle);
        Log("[foveated-native] create %s result=0x%08X handle=%p\n",central?"center":"periphery",result,handle);
        if(!ngx::Success(result) || !handle)return 0xBAD00005;
    }
    return route.evaluate(list,handle,p,nullptr);
}
ngx::Result Process(unsigned routeIndex,ID3D12GraphicsCommandList* list,const ngx::Handle* handle,const ngx::Parameters* params,void* callback) {
    auto& route=routes[routeIndex];if(!route.evaluate)return 0xBAD00007;
    if(internal || !list || !params || !handle)return route.evaluate(list,handle,params,callback);
    Recursion recursion;const auto pose=cvr::camera::CurrentNodePoseIdentity();
    if(!pose || pose.view<1 || pose.view>2 || list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT)
        return route.evaluate(list,handle,params,callback);
    cvr::sharpen::gameUpscaler.ObserveGameQuality(pose.view,ngx::Integer(params,"PerfQualityValue",UINT_MAX));
    // Engine stereo: both eyes are MAIN's view, each evaluated on its own viewport (Stereo/EngineStereoDlss.cpp) with
    // its half as the whole input and output -- told apart by the eye being evaluated, each with its own state.
    const int stereoEye=engine_stereo::Enabled() ? engine_stereo::dlss::EvaluatingEye() : -1;
    std::lock_guard lock(mutex);const unsigned eye=stereoEye>=0 ? unsigned(stereoEye) : pose.view-1;const auto settings=GetAppliedSettings();
    ++calls;lastCall=GetTickCount64();auto* p=const_cast<ngx::Parameters*>(params);
    const auto native=[&]() {
        ngx::Override change(p);if(nativeReset[eye]){change.Set("Reset",1u);nativeReset[eye]=false;}
        auto timer=Timer(list,true);const auto result=route.evaluate(list,handle,params,callback);
        Stamp(list,timer.Get(),1);FinishTimer(list,timer,2);return result;
    };
    // One evaluate over both eyes side by side (per-eye DLSS off) has no single centre region to keep sharp.
    if(!Enabled(settings) || GetMenuMode()!=0 || (engine_stereo::Enabled() && stereoEye<0)) {views[eye].Reset();return native();}
    Source source;unsigned flags{},quality{};
    if(!ReadSource(params,source,flags,quality)) {Message("Unsupported DLSS inputs; native rendering");return native();}
    ngx::FeatureConfig config{source.outputW,source.outputH,flags,quality};
    if(!gameFeatures.Find(handle,params,pose.view,config)) {
        views[eye].Reset();Message("Waiting for game DLSS configuration; native rendering");return native();
    }
    auto& state=views[eye];
    if(!state || state->original!=handle || state->renderW!=source.renderW || state->renderH!=source.renderH ||
        state->outputW!=source.outputW || state->outputH!=source.outputH || state->flags!=flags || state->quality!=quality || state->presets!=config.presets ||
        !state->settings.SameReconstruction(settings))
        state=CreateState(list,source,settings,handle,config,route);
    if(!state || !cvr::gpu::KeepCommandResources(list,{state.Get()})) {Message("Foveated resource initialization failed");return native();}
    cvr::stereo::probe::Scope privateCommands;
    const bool reset=state->first || ngx::Integer(params,"Reset")!=0;
    auto timer=Timer(list,false);
    auto result=EvaluateRegion(list,p,source,*state.Get(),route,false,reset);Stamp(list,timer.Get(),1);
    if(ngx::Success(result))result=EvaluateRegion(list,p,source,*state.Get(),route,true,reset);Stamp(list,timer.Get(),2);
    if(ngx::Success(result) && state->gpu.Composite(list,source,settings.showBoundary)) {
        Stamp(list,timer.Get(),3);FinishTimer(list,timer,4);
        state->first=false;++state->evaluations;state->resets+=reset;++viewCalls[eye];++active;nativeReset[eye]=true;
        Message("Active");return result;
    }
    ++failed;Log("[foveated-native] evaluation failed result=0x%08X; native fallback\n",result);
    state.Reset();nativeReset[eye]=true;Message("Foveated evaluation failed; native rendering");return native();
}
template<unsigned Index> ngx::Result Hook(ID3D12GraphicsCommandList* list,const ngx::Handle* handle,const ngx::Parameters* p,void* callback) {
    return Process(Index,list,handle,p,callback);
}
ngx::Result CreateHook(ID3D12GraphicsCommandList* list,unsigned feature,ngx::Parameters* p,ngx::Handle** handle) {
    if(internal || feature!=1 || !p)return originalCreate(list,feature,p,handle);
    Recursion recursion;
    ngx::FeatureConfig config{ngx::Integer(p,"OutWidth"),ngx::Integer(p,"OutHeight"),
        ngx::Integer(p,"DLSS.Feature.Create.Flags"),ngx::Integer(p,"PerfQualityValue")};
    for(unsigned i=0;i<6;++i)config.presets[i]=ngx::Integer(p,presetKeys[i]);
    const auto pose=cvr::camera::CurrentNodePoseIdentity();
    const auto result=originalCreate(list,feature,p,handle);
    if(ngx::Success(result) && handle && *handle) {
        std::lock_guard lock(mutex);gameFeatures.Remember(*handle,p,pose?pose.view:0,config);
        const auto& presets=config.presets;
        Log("[foveated-native] game feature=%p params=%p view=%u output=%ux%u quality=%u flags=0x%X presets=%u/%u/%u/%u/%u/%u\n",
            *handle,p,pose?pose.view:0,config.outputW,config.outputH,config.quality,config.flags,
            presets[0],presets[1],presets[2],presets[3],presets[4],presets[5]);
    }
    return result;
}
// A failed MH_CreateHook has already scanned the address space near its target for a trampoline block
// (VirtualQuery, about 10 ms in this process) -- on the game's Present thread. Retried every two seconds it
// cost one 45 fps frame slot each time. A failure is deterministic for a loaded module: try a few times, then
// wait for a different nvngx_dlss.dll before trying again.
constexpr unsigned kInstallAttempts=3;
HMODULE installModule{};
unsigned installAttempts{};
void Install() {
    auto module=GetModuleHandleW(L"nvngx_dlss.dll");if(!module)return;
    if(GetModuleHandleW(L"CheekyFoveatedDLSSRuntime.dll")){Message("Another foveated backend is loaded; restart required");return;}
    auto create=reinterpret_cast<ngx::Create>(GetProcAddress(module,"NVSDK_NGX_D3D12_CreateFeature"));
    auto release=reinterpret_cast<ngx::Release>(GetProcAddress(module,"NVSDK_NGX_D3D12_ReleaseFeature"));if(!create || !release)return;
    if(module!=installModule){installModule=module;installAttempts=0;}
    if(installAttempts>=kInstallAttempts)return;
    ++installAttempts;
    const auto failedAt=[](const char* what,MH_STATUS status) {
        Log("[foveated-native] %s failed: %s (attempt %u/%u)%s\n",what,MH_StatusToString(status),installAttempts,
            kInstallAttempts,installAttempts>=kInstallAttempts ? "; not retrying for this DLSS module" : "");
    };
    // nvngx_dlss.dll can be mapped among the game's reservations with no free page within MinHook's reach
    // (seen: base 0x1784DCA0000, nothing free within 2 GB) -- MH_ERROR_MEMORY_ALLOC, and the feature never ran.
    // Its NGX entry points open with plain register saves, so they take a far hook instead (Hooks/FarInlineHook).
    const auto hook=[&](void* target,void* detour,void** original,const char* what) {
        auto created=MH_CreateHook(target,detour,original);
        if(created==MH_OK)created=MH_EnableHook(target);
        if(created==MH_OK)return true;
        if(created==MH_ERROR_MEMORY_ALLOC) {
            const auto distant=cvr::hooks::FarInlineHook(target,detour,original);
            Log("[foveated-native] %s: no free memory near nvngx_dlss.dll for MinHook; far hook %s\n",what,
                cvr::hooks::ToString(distant));
            if(distant==cvr::hooks::FarHookResult::Ok)return true;
        }
        failedAt(what,created);return false;
    };
    if(!originalCreate && !hook(reinterpret_cast<void*>(create),reinterpret_cast<void*>(&CreateHook),
                                reinterpret_cast<void**>(&originalCreate),"CreateFeature hook"))return;
    create=originalCreate;
    const char* names[]{"NVSDK_NGX_D3D12_EvaluateFeature","NVSDK_NGX_D3D12_EvaluateFeature_C"};
    void* hooks[]{reinterpret_cast<void*>(&Hook<0>),reinterpret_cast<void*>(&Hook<1>)};
    for(unsigned i=0;i<2;++i) {
        auto* target=GetProcAddress(module,names[i]);if(!target || (i && target==routes[0].target))continue;
        if(routes[i].evaluate)continue;
        routes[i].create=create;routes[i].release=release;routes[i].target=target;
        if(hook(target,hooks[i],reinterpret_cast<void**>(&routes[i].evaluate),names[i])) {
            installed=true;Log("[foveated-native] hooked %s\n",names[i]);
        }
    }
}
void Publish() {
    const auto settings=GetAppliedSettings();Json::Value root;root["backend"]="native";root["enabled"]=Enabled(settings);
    root["pending_settings"]=!(settings==GetSettings());Status current;
    {std::lock_guard lock(statsMutex);status.loaded=installed;status.ready=installed;
        status.evaluations=calls;status.processed=active;status.views=unsigned(bool(views[0]))+unsigned(bool(views[1]));
        status.active=Enabled(settings) && GetTickCount64()-lastCall<2000 && !std::strcmp(status.text,"Active");
        if(!Enabled(settings))strcpy_s(status.text,"Off");
        else if(GetTickCount64()-lastCall>2000)strcpy_s(status.text,"Waiting for DLSS SR");current=status;}
    Json::Value api;api["state"]=current.text;api["evaluations"]=Json::UInt64(calls);api["active"]=Json::UInt64(active);
    api["failures"]=Json::UInt64(failed);api["native_ms"]=current.nativeMs;api["foveated_ms"]=current.foveatedMs;
    api["peripheral_ms"]=current.peripheralMs;api["after_pipeline_ms"]=Enabled(settings)?current.totalMs:current.nativeMs;
    root["apis"]=Json::arrayValue;root["apis"].append(Json::Value{});root["apis"].append(api);root["view_details"]=Json::arrayValue;
    for(unsigned i=0;i<2;++i)if(const auto& state=views[i]) {
        const auto& r=state->gpu.center.region;Json::Value v;v["id"]=i;v["eye"]=engine_stereo::Enabled() ? (i?"right":"left") : (i?"VRCAM":"MAIN");
        v["evaluations"]=Json::UInt64(viewCalls[i]);v["resets"]=Json::UInt64(state->resets);
        v["input_width"]=state->renderW;v["input_height"]=state->renderH;v["output_width"]=state->outputW;v["output_height"]=state->outputH;
        v["center_input_width"]=r.inputW;v["center_input_height"]=r.inputH;v["center_quality"]=state->quality;
        v["offset_x_percent"]=state->settings.offsetXPercent;v["offset_y_percent"]=state->settings.offsetYPercent;
        v["center_presets"]=Json::arrayValue;for(const auto preset:state->presets)v["center_presets"].append(preset);
        v["crop_width"]=r.width;v["crop_height"]=r.height;v["crop_x"]=r.x;v["crop_y"]=r.y;root["view_details"].append(v);
    }
    Json::StreamWriterBuilder writer;writer["indentation"]="";const auto text=Json::writeString(writer,root);
    CyberpunkVR_FoveatedReportSeq.fetch_add(1,std::memory_order_acq_rel);strncpy_s(CyberpunkVR_FoveatedReport,text.c_str(),_TRUNCATE);
    CyberpunkVR_FoveatedReportSeq.fetch_add(1,std::memory_order_release);
}
}
Status GetStatus(){std::lock_guard lock(statsMutex);return status;}
void Tick(ID3D12Device*,ID3D12CommandQueue*) {
    UpdateSettings();
    if(!installed && GetTickCount64()>=nextInstall){nextInstall=GetTickCount64()+2000;Install();}
    std::lock_guard lock(mutex);
    std::vector<std::pair<ngx::Release,ngx::Handle*>> retired;
    {std::lock_guard retiredLock(retiredMutex);retired.swap(retiredFeatures);}
    for(const auto& [release,handle]:retired) {
        HMODULE module{};
        if(GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS|GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            reinterpret_cast<LPCWSTR>(release),&module))release(handle);
    }
    static uint64_t nextReport{};if(GetTickCount64()>=nextReport){nextReport=GetTickCount64()+500;Publish();}
}
void BeforeSubmit(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists) {
    std::lock_guard lock(timingMutex);uint64_t frequency{};
    for(UINT i=0;i<count;++i)if(auto it=timings.find(lists[i]);it!=timings.end()) {
        if(!frequency)queue->GetTimestampFrequency(&frequency);
        for(auto& t:it->second)t->frequency.store(frequency);timings.erase(it);
    }
}
void ResetList(ID3D12GraphicsCommandList* list){std::lock_guard lock(timingMutex);timings.erase(list);}
void Shutdown(){std::lock_guard lock(mutex);for(auto& view:views)view.Reset();}
}
