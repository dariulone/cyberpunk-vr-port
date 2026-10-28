#include "Utils/DebugGate.hpp"
// Native ink paint/primitive ownership, verified in world-markers-20260923.md.
#include "Stereo/WorldMarkers.hpp"
#include "Stereo/StereoInternal.hpp"
#include "Render/WorldMarkerProjection.hpp"
#include "Render/WorldMarkerScale.hpp"
#include "Render/WorldMarkerOwnership.hpp"
#include "Render/WorldMarkerShader.hpp"
#include "Camera/CameraLink.hpp"
#include "Hooks/Hook.hpp"
#include "Hooks/Trampoline.hpp"
#include "Runtimes/AsymmetricProjection.hpp"
#include "Stereo/EngineStereo.hpp"
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Scripting/Functions.hpp>
#include <RED4ext/Scripting/Utils.hpp>
#include <RED4ext/Scripting/Natives/inkWidget.hpp>
#include <RED4ext/Scripting/Natives/inkIWidgetLogicController.hpp>
#include <RED4ext/Scripting/Natives/Generated/ink/WidgetLogicController.hpp>
#include <RED4ext/Scripting/Natives/inkLayer.hpp>
#include <RED4ext/Scripting/Natives/inkLayerProxy.hpp>
#include <MinHook.h>
#include <array>
#include <atomic>
#include <cstring>
#include <mutex>
#include <shared_mutex>
#include <unordered_map>
#include <vector>

extern void Log(const char*,...);
extern "C" float CyberpunkVRPort_HalfIpd();
extern "C" float GetGameRenderFovDeg();
extern "C" float CyberpunkVR_MainAdsZoomFactor;
extern "C" int CyberpunkVR_MainIsRightEye;
extern "C" int CyberpunkVR_StereoSubmit;
extern "C" void CvrWorldMarkerTextPublishDetour();
extern "C" { uintptr_t CvrWorldMarkerTextPublishTarget{}; }   // the native publication the thunk jumps on to
extern "C" int GetMenuMode();
extern "C" int GetMenuRectMode();
extern "C" float CyberpunkVR_HudRootWidth, CyberpunkVR_HudRootHeight;   // Natives/HudPanel.cpp
extern int g_launcherWidth;
extern int g_launcherHeight;
extern "C" {
__declspec(dllexport) std::atomic<int32_t> CyberpunkVR_WorldMarkerStereo{1};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_WorldMarkerRoots{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_WorldMarkerQuads{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_WorldMarkerTexts{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_WorldMarkerCameraUploads[2]{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_WorldMarkerTextUploads[2]{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_WorldMarkerMappinStages[7]{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_WorldMarkerProjectionStages[5]{};
// 1 = under the asymmetric projection, move each view's flat HUD by that view's own projection shift
// (see InkShift). 0 = the HUD composed at the same pixels in both views, as before.
__declspec(dllexport) std::atomic<int32_t> CyberpunkVR_AsymHudShift{1};
__declspec(dllexport) std::atomic<int32_t> CyberpunkVR_WorldMarkerProbe{0};
// Unclamped markers whose point is off the canvas or outside either eye's window: 0 = not drawn, 1 = drawn at the
// point (then seen by one eye only).
__declspec(dllexport) std::atomic<int32_t> CyberpunkVR_WorldMarkerOffCanvas{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_WorldMarkerProbeSequence[2]{};
__declspec(dllexport) uint64_t CyberpunkVR_WorldMarkerProbeWords[2][4]{};
__declspec(dllexport) float CyberpunkVR_WorldMarkerProbeFloats[2][64]{};
__declspec(dllexport) std::atomic<int32_t> CyberpunkVR_WorldMarkerReproject{1};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_WorldMarkerWorldVertices{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_WorldMarkerWorldTexts{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_WorldMarkerWorldCameras[2]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_WorldMarkerTraceSequence{};
__declspec(dllexport) uint64_t CyberpunkVR_WorldMarkerTraceWords[64][8]{};
__declspec(dllexport) float CyberpunkVR_WorldMarkerTraceFloats[64][48]{};
__declspec(dllexport) uint64_t CyberpunkVR_WorldMarkerTextLast[8]{};
// Live A/B, all on by default: markers hang level from their point (no head roll); exact, unrounded margins;
// no pixel snapping of lifted geometry; an effect takes its marker's projection despite a few ownerless records.
__declspec(dllexport) std::atomic<int32_t> CyberpunkVR_WorldMarkerLevel{1};
__declspec(dllexport) std::atomic<int32_t> CyberpunkVR_WorldMarkerExactMargin{1};
__declspec(dllexport) std::atomic<int32_t> CyberpunkVR_WorldMarkerUnsnap{1};
__declspec(dllexport) std::atomic<int32_t> CyberpunkVR_WorldMarkerCompositeLoose{1};
// Effects composed: [0] one marker's only, [1] one marker's with ownerless records, taken, [2] with ownerless
// records, seen, [3] two markers', [4] no marker's, [5] records the port never saw allocated. The last mixed one:
// root, owned records, ownerless records, tick.
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_WorldMarkerCompositeStats[8]{};
__declspec(dllexport) uint64_t CyberpunkVR_WorldMarkerCompositeMixed[4]{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_WorldMarkerExactMargins{0};
// A marker's vertices left on the canvas: [0] already tagged, [1] a z of their own, [2] inside an offscreen effect,
// [3] no source camera, [4] the lift failed; with the last marker root of each.
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_WorldMarkerUnlifted[5]{};
__declspec(dllexport) uint64_t CyberpunkVR_WorldMarkerUnliftedRoot[5]{};
// Top-level paints of a marker root's descendant (Paint): count; last root, widget, levels up, tick.
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_WorldMarkerDetachedPaints{0};
__declspec(dllexport) uint64_t CyberpunkVR_WorldMarkerDetachedPaint[4]{};
// The layers of the marker root named here (with WorldMarkerProbe), a ring of 64: words record, kind | 256 lifted |
// 512 offscreen | vertices << 16, tick, first vertex colour (half4); floats canvas bounds (text: the matrix's
// translation, then the record's exact one), first and last vertex UV (raw 16-bit).
__declspec(dllexport) uint64_t CyberpunkVR_WorldMarkerLayerRoot=0;
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_WorldMarkerLayerCount{0};
__declspec(dllexport) uint64_t CyberpunkVR_WorldMarkerLayerWords[64][4]{};
__declspec(dllexport) float CyberpunkVR_WorldMarkerLayerFloats[64][8]{};
}

namespace cvr::markers {
namespace {
using namespace RED4ext;
const uintptr_t base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
std::atomic<bool> hooksReady{false};
bool Enabled() {
    return hooksReady.load(std::memory_order_acquire) && VertexShadersReady() &&
        CyberpunkVR_WorldMarkerStereo.load(std::memory_order_relaxed) && CyberpunkVR_StereoSubmit;
}
template<class T> T& At(void* p,size_t offset) { return *reinterpret_cast<T*>(static_cast<uint8_t*>(p)+offset); }
struct RootDepth { WeakHandle<ink::Widget> root; float inverseDepth{}; uint64_t stamp{}; MarkerScale scale; bool inheritDepth{}; SourceProjection source{}; MarkerAnchor anchor{}; Vector2 savedCachePivot{}; bool ownsCachePivot{}; bool hidden{}; };
std::shared_mutex rootsMutex;
std::unordered_map<ink::Widget*,RootDepth> roots;
struct ProjectionOwner { WeakHandle<IScriptable> projection; WeakHandle<ink::Widget> widget; uint64_t stamp{}; };
std::mutex projectionMutex;
std::unordered_map<void*,ProjectionOwner> projectionOwners;
thread_local float paintDepth=0;
thread_local uintptr_t paintOwner=0;
thread_local unsigned offscreenPass=0;
thread_local float textDepth=0;
thread_local void* mappinViewContext=nullptr;
thread_local SourceProjection paintSource{},textSource{};
thread_local MarkerAnchor paintAnchor{},textAnchor{};
thread_local bool unsnapVertices=false;
thread_local unsigned paintNesting=0;
thread_local float textExact[2]{};thread_local bool textExactValid=false;
thread_local uintptr_t textLayerRoot=0;
struct InkCamera { float width{},height{},parallax{},squeeze{1}; uint64_t stamp{}; bool worldReady{}; };
thread_local InkCamera inkCameras[2];
thread_local int inkSlot=-1;
bool ReprojectionEnabled() {
    return Enabled() && cvr::engine_stereo::Enabled() && CyberpunkVR_WorldMarkerReproject.load(std::memory_order_relaxed);
}
bool ReadUiProjection(void* view,float width,float height,SourceProjection& source) {
    // 2.5 ink projection view -> gameCameraSystem. Its WorldToScreen implementation
    // projects through the native camera at system+0x60 (sub_14054AD6C).
    __try {
        if(!view || At<uintptr_t>(view,0)!=base+0x3620AA8)return false;
        auto* system=At<uint8_t*>(view,8);
        if(!system || At<uintptr_t>(system,0)!=base+0x30BB4A8)return false;
        if(!ReadProjection(system+0x60,width,height,source))return false;
        // A one-eye layer's view places through one eye's window (Stereo/EngineStereo.hpp UiViewNarrowing).
        const float narrow=cvr::engine_stereo::UiViewNarrowing(view);
        source.p00*=narrow;source.p20*=narrow;
        return true;
    } __except(EXCEPTION_EXECUTE_HANDLER) {return false;}
}
std::mutex sourceMutex;
void* lastSourceView{};
float lastSourceSize[2]{};
uint64_t lastSourceStamp{};
SourceProjection CaptureSource(void* view,const float* size) {
    SourceProjection source;
    if(!ReprojectionEnabled() || !size || !ReadUiProjection(view,size[0],size[1],source))return {};
    std::lock_guard lock(sourceMutex);
    lastSourceView=view;lastSourceSize[0]=size[0];lastSourceSize[1]=size[1];lastSourceStamp=GetTickCount64();
    return source;
}
SourceProjection CurrentUiSource() {
    void* view{};float size[2]{};
    {
        std::lock_guard lock(sourceMutex);
        if(GetTickCount64()-lastSourceStamp>100 || GetMenuMode())return {};
        view=lastSourceView;std::memcpy(size,lastSourceSize,sizeof(size));
    }
    SourceProjection source;
    return ReprojectionEnabled() && ReadUiProjection(view,size[0],size[1],source)?source:SourceProjection{};
}
// The eyes' last cameras as the recording workers read them, for the UI update's "does each eye see this point"
// (CollectMappin). A frame or two behind the update's own camera, which is well inside an eye window's margin.
std::mutex eyeSourceMutex;
SourceProjection eyeSources[2];
uint64_t eyeSourceStamps[2]{};
void KeepEyeSource(int eye,const SourceProjection& source) {
    std::lock_guard lock(eyeSourceMutex);
    eyeSources[eye]=source;eyeSourceStamps[eye]=GetTickCount64();
}
// True unless an eye with a fresh camera has the point outside its window (no fresh camera: no verdict).
bool BothEyesSee(const float point[3]) {
    SourceProjection eyes[2];uint64_t stamps[2];
    {std::lock_guard lock(eyeSourceMutex);std::memcpy(eyes,eyeSources,sizeof(eyes));std::memcpy(stamps,eyeSourceStamps,sizeof(stamps));}
    const auto now=GetTickCount64();
    for(int eye=0;eye<2;++eye) {
        if(!eyes[eye].Valid() || now-stamps[eye]>250)continue;
        double v[3]{};
        for(int i=0;i<3;++i) {
            const double d=double(point[i])-eyes[eye].origin[i];
            v[0]+=d*eyes[eye].right[i];v[1]+=d*eyes[eye].up[i];v[2]+=d*eyes[eye].forward[i];
        }
        if(!(v[2]>.05))return false;
        const double x=v[0]/v[2]*eyes[eye].p00+eyes[eye].p20,y=v[1]/v[2]*eyes[eye].p11+eyes[eye].p21;
        if(!(std::abs(x)<=1) || !(std::abs(y)<=1))return false;
    }
    return true;
}
bool ReadEyeCamera(int eye,float width,float height,WorldMarkerCamera& packet) {
    // The worker's own view, not the global MAIN pointer which can already refer
    // to a newer prepared frame while this command list is being recorded.
    const uintptr_t view=detail::t_current_node_view;
    if(eye<0 || eye>1 || !view)return false;
    SourceProjection target;
    __try {
        auto* descriptor=reinterpret_cast<uint8_t*>(view+0xE0);
        if(!(descriptor[0x399]&1))return false;
        auto* camera=*reinterpret_cast<uint8_t**>(descriptor+(eye?0x3D8:0x3D0));
        if(!camera || camera[0x398]!=eye+1)return false;
        if(!ReadProjection(camera,width,height,target,true))return false;
        packet=MarkerCamera(target);
    } __except(EXCEPTION_EXECUTE_HANDLER) {return false;}
    KeepEyeSource(eye,target);
    return true;
}
bool CanLift(const SourceProjection& source) {
    // Geometry generation precedes recording and can run on a different worker.
    // A worker's last camera upload is not this geometry's render destination:
    // using that TLS state randomly alternated between world and flat vertices.
    // Source ownership and the explicit offscreen-composition scope decide the
    // encoding; the recording worker supplies the matching eye packet later.
    return ReprojectionEnabled() && !offscreenPass && source.Valid();
}
void CollectMappin(void* controller,const void* position);

std::mutex traceMutex;
size_t TraceSlot(uintptr_t root) {
    size_t oldest=0;
    for(size_t i=0;i<64;++i) {
        const auto* row=CyberpunkVR_WorldMarkerTraceWords[i];
        if(row[0]==root || !row[0])return i;
        if(row[4]<CyberpunkVR_WorldMarkerTraceWords[oldest][4])oldest=i;
    }
    return oldest;
}
void TraceMappin(void* controller,ink::Widget* root,const float* point,const float* screen,const SourceProjection& source,bool valid,bool clamped) {
    if(!CyberpunkVR_WorldMarkerProbe.load(std::memory_order_relaxed))return;
    std::lock_guard lock(traceMutex);
    CyberpunkVR_WorldMarkerTraceSequence.fetch_add(1,std::memory_order_acq_rel);
    const auto i=TraceSlot(reinterpret_cast<uintptr_t>(root));
    auto* w=CyberpunkVR_WorldMarkerTraceWords[i];auto* f=CyberpunkVR_WorldMarkerTraceFloats[i];
    if(w[0]!=reinterpret_cast<uintptr_t>(root)){std::memset(w,0,64);std::memset(f,0,192);}
    w[0]=reinterpret_cast<uintptr_t>(root);w[1]=reinterpret_cast<uintptr_t>(controller);
    w[2]=At<uintptr_t>(controller,0x40);w[3]=At<uintptr_t>(controller,0x170);w[4]=GetTickCount64();
    w[7]=(w[7]&~15ull)|(valid?1:0)|(clamped?2:0)|(At<uint8_t>(controller,0x190)?4:0)|(At<uint8_t>(controller,0xE3)?8:0);
    std::memcpy(f,point,12);f[3]=SourceDepth(source,point);if(screen)std::memcpy(f+4,screen,8);
    f[8]=source.width;f[9]=source.height;f[10]=root->layout.margin.left;f[11]=root->layout.margin.top;
    f[12]=root->size.X;f[13]=root->size.Y;f[14]=root->renderTransform.scale.X;f[15]=root->renderTransform.scale.Y;
    std::memcpy(f+16,source.origin,12);std::memcpy(f+20,source.forward,12);
    if(source.Valid() && f[3]>0) {
        float x=0,y=0;for(int a=0;a<3;++a){x+=(point[a]-source.origin[a])*source.right[a];y+=(point[a]-source.origin[a])*source.up[a];}
        f[6]=(x/f[3]*source.p00+source.p20+1)*source.width*.5f;
        f[7]=(1-y/f[3]*source.p11-source.p21)*source.height*.5f;
    }
    CyberpunkVR_WorldMarkerTraceSequence.fetch_add(1,std::memory_order_release);
}

void ProbeView(int kind,void* root,void* view,const float* world,const float* size,const float* screen,
    const float* collectedWorld=nullptr) {
    if(!CyberpunkVR_WorldMarkerProbe.load(std::memory_order_relaxed) || !view || !world)return;
    float data[64]{};
    std::memcpy(data,world,12);
    const auto* vt=*static_cast<uintptr_t**>(view);
    using PositionFn=void (*)(void*,float*);
    reinterpret_cast<PositionFn>(vt[48/8])(view,data+4);
    using VectorFn=const float* (*)(void*,float*);
    for(int i=0;i<3;++i) {
        float temporary[4]{};
        const float* result=reinterpret_cast<VectorFn>(vt[(64+i*8)/8])(view,temporary);
        if(!result)return;
        std::memcpy(data+8+i*4,result,12);
    }
    for(int i=0;i<3;++i)data[3]+=(world[i]-data[4+i])*data[8+i];
    using ProjectFn=const float* (*)(void*,float*,const float*);
    for(int sample=0;sample<4;++sample) {
        float point[4]{},result[4]{};
        for(int i=0;i<3;++i)point[i]=sample==0?world[i]:data[4+i]+data[8+i]+(sample==2?data[12+i]:sample==3?data[16+i]:0);
        const float* projected=reinterpret_cast<ProjectFn>(vt[104/8])(view,result,point);
        if(!projected)return;
        std::memcpy(data+20+sample*4,projected,8);
    }
    if(size)std::memcpy(data+36,size,8);
    if(screen)std::memcpy(data+38,screen,8);
    camera::LocatedCameraFrame located{};
    if(camera::LocatedCameraFrameRead(&located)) {
        std::memcpy(data+40,located.worldPos,12);
        std::memcpy(data+44,located.worldQuat,16);
        data[43]=float(located.sequence);
    }
    if(collectedWorld)std::memcpy(data+48,collectedWorld,12);
    static std::mutex mutex;
    std::lock_guard lock(mutex);
    CyberpunkVR_WorldMarkerProbeSequence[kind].fetch_add(1,std::memory_order_acq_rel);
    auto* words=CyberpunkVR_WorldMarkerProbeWords[kind];
    words[0]=GetTickCount64();words[1]=reinterpret_cast<uintptr_t>(root);
    words[2]=reinterpret_cast<uintptr_t>(view);words[3]=reinterpret_cast<uintptr_t>(vt);
    std::memcpy(CyberpunkVR_WorldMarkerProbeFloats[kind],data,sizeof(data));
    CyberpunkVR_WorldMarkerProbeSequence[kind].fetch_add(1,std::memory_order_release);
}

using MappinUpdateFn=void (*)(void*,void*);
MappinUpdateFn originalMappinUpdate{};
void MappinUpdate(void* controller,void* context) {
    const auto previous=mappinViewContext;mappinViewContext=context;
    originalMappinUpdate(controller,context);
    // Native clamp/visibility state is finalized AFTER SetScreenPosition.
    if(ReprojectionEnabled())CollectMappin(controller,&At<float>(controller,120));
    mappinViewContext=previous;
}

bool HudRoot(const Handle<ink::Widget>& root) {
    auto current=root;
    for(int i=0;current && i<24;++i) {
        const auto proxy=current->layerProxy;
        const auto layer=proxy ? proxy->layer.Lock() : Handle<ink::Layer>{};
        if(layer) return layer->GetType()->GetName()==CName("inkHUDLayer");
        current=current->parentWidget.Lock();
    }
    return false;
}
bool WorldMappinController(IScriptable* controller) {
    // The world container's game controller is NOT Widget::logicController:
    // the live Root/HUDMiddleWidget/window ancestor chain has no logic controller.
    // This call site is already MappinBaseController; classify the actual caller.
    auto type=controller->GetType();
    for(int i=0;type && i<64;++i,type=type->parent) {
        const auto name=type->GetName();
        if(name==CName("gameuiBaseMinimapMappinController") ||
           name==CName("gameuiBaseWorldMapMappinController")) return false;
        if(name==CName("gameuiMappinBaseController")) return true;
    }
    return false;
}
void SetWidgetPivot(const Handle<ink::Widget>& root,Vector2 pivot) {
    if(root->renderTransformPivot.X==pivot.X && root->renderTransformPivot.Y==pivot.Y)return;
    // Two script overloads share the short name: (pivot: Vector2) native, (x, y) scripted.
    auto setter=root->GetType()->GetFunction("SetRenderTransformPivot");
    StackArgs_t args;
    if(setter && setter->params.Size()==1 && setter->params[0]->type->GetName()==CName("Vector2"))
        args.emplace_back(nullptr,&pivot);
    else if(setter && setter->params.Size()==2) {
        args.emplace_back(nullptr,&pivot.X);args.emplace_back(nullptr,&pivot.Y);
    } else setter=nullptr;
    if(setter)ExecuteFunction(root.GetPtr(),setter,nullptr,args);
}
bool SetWidgetScale(const Handle<ink::Widget>& root,Vector2 scale) {
    auto setter=root->GetType()->GetFunction("SetScale");
    if(!setter || setter->params.Size()!=1 || setter->params[0]->type->GetName()!=CName("Vector2")) {
        static std::atomic<bool> warned{false};
        if(!warned.exchange(true))Log("[world-markers] SetScale(Vector2) unavailable; original marker size retained\n");
        return false;
    }
    // The engine setter preserves the pivot and invalidates paint.
    StackArgs_t args;args.emplace_back(nullptr,&scale);
    return ExecuteFunction(root.GetPtr(),setter,nullptr,args);
}

void Remember(const Handle<ink::Widget>& root,const float* world,bool valid=true,bool scaleMarker=false,bool inheritDepth=false,
    bool pinToAnchor=false,const SourceProjection* source=nullptr,const Vector2* cachePivot=nullptr,bool cached=false,
    bool hidden=false) {
    if(!root || !HudRoot(root)) return;
    camera::LocatedCameraFrame frame{};
    const float depth=!valid || !world?0:source && source->Valid()?SourceDepth(*source,world):
        camera::LocatedCameraFrameRead(&frame)?ForwardDepth(world,frame.worldPos,frame.worldQuat):0;
    const uint64_t now=GetTickCount64();
    Vector2 target{},anchor=root->layout.anchorPoint,native=root->renderTransform.scale;bool resize=false;
    {
        std::unique_lock lock(rootsMutex);
        auto& entry=roots[root.GetPtr()];
        if(entry.root.Lock().GetPtr()!=root.GetPtr())entry={WeakHandle<ink::Widget>(root)};
        entry.inverseDepth=depth>0 ? 1/depth : 0;entry.stamp=now;
        entry.source=depth>0 && source?*source:SourceProjection{};
        entry.anchor={};
        if(depth>0 && source && world)AnchorOf(*source,world,entry.anchor);
        entry.inheritDepth=inheritDepth;entry.hidden=hidden;
        if(cached) {
            if(cachePivot) {
                if(!entry.ownsCachePivot)entry.savedCachePivot=root->renderTransformPivot;
                entry.ownsCachePivot=true;anchor=*cachePivot;pinToAnchor=true;
            } else if(entry.ownsCachePivot) {
                anchor=entry.savedCachePivot;pinToAnchor=true;entry.ownsCachePivot=false;
            }
        } else if(pinToAnchor && !entry.ownsCachePivot) {
            // A live root's own pivot, for the hand-back when its controller caches it (ReleaseCachedRoot).
            entry.savedCachePivot=root->renderTransformPivot;entry.ownsCachePivot=true;
        }
        // Under the engine's stereo the HUD's design scale is 1.92x the one-eye root's (HudDesignRatio).
        if(scaleMarker)resize=entry.scale.Target(root->renderTransform.scale.X,root->renderTransform.scale.Y,target.X,target.Y,
            .7f*cvr::engine_stereo::HudDesignRatio(CyberpunkVR_HudRootWidth,CyberpunkVR_HudRootHeight));
        if(roots.size()>512) {
            for(auto it=roots.begin();it!=roots.end();) {
                // Retain scale ownership while a hidden marker is still alive;
                // expiring it by time alone would shrink it again on reappearance.
                if(now-it->second.stamp>2000 && !it->second.root.Lock())it=roots.erase(it);else ++it;
            }
        }
        CVR_DIAGNOSTIC(CyberpunkVR_WorldMarkerRoots.store(roots.size(),std::memory_order_relaxed));
    }
    // SHRINK ABOUT THE POINT ON THE TARGET. A mappin controller's own root is placed by its anchor point
    // and its content hangs from that point (quest/gameplay mappins: a 75x75 root, anchor 0,0, the
    // content canvas at 0,0), while the authored pivot is the box's centre. Scaled about the centre, the
    // icon moved (1-0.7)*37.5 units right and down, off the NPC's nameplate chevron, which is not
    // scaled. Cached presentation textures use their native cache origin instead:
    // its anchor is (-controller.cacheOffset), not necessarily the texture centre.
    if(pinToAnchor)SetWidgetPivot(root,anchor);
    // Only native world-mappin roots are resized, never map icons or chatters.
    if(resize && SetWidgetScale(root,target)) {
        std::unique_lock lock(rootsMutex);
        auto it=roots.find(root.GetPtr());
        if(it!=roots.end() && it->second.root.Lock().GetPtr()==root.GetPtr())it->second.scale.Commit(target.X,target.Y,native.X,native.Y);
    }
}

// THE LIVE ROOT OF A CACHED MAPPIN IS THE GAME'S AGAIN. A cached mappin shows a picture of its live root: the
// presentation manager moves that root into a slot of its offscreen cache canvas (sub_1420B1940, from
// sub_14047FE70 / sub_142214170, which then set +0x190) and draws the slot into the picture. There it kept the
// port's marker size and depth: drawn at 0.364 and the picture shrunk once more (an NPC's stealth chevron 13
// instead of 36 canvas px, hanging off its anchor), and for the first second its vertices lifted into the world,
// which the cache canvas's camera cannot project. Measured on the 2.5 stealth mappins: the same slot rectangle
// (830..866, 839..857) in two sessions, at scale 0.3276. The icon changed size and jumped each time an NPC crossed
// between the live and the cached distance.
void ReleaseCachedRoot(const Handle<ink::Widget>& root) {
    if(!root)return;
    {
        std::shared_lock lock(rootsMutex);
        if(!roots.contains(root.GetPtr()))return;
    }
    Vector2 scale{},pivot{};bool restoreScale=false,restorePivot=false;
    {
        std::unique_lock lock(rootsMutex);
        const auto it=roots.find(root.GetPtr());
        if(it==roots.end())return;
        if(it->second.root.Lock().GetPtr()==root.GetPtr()) {
            restoreScale=it->second.scale.Restore(root->renderTransform.scale.X,root->renderTransform.scale.Y,scale.X,scale.Y);
            restorePivot=it->second.ownsCachePivot;pivot=it->second.savedCachePivot;
        }
        roots.erase(it);
    }
    if(restorePivot)SetWidgetPivot(root,pivot);
    if(restoreScale)SetWidgetScale(root,scale);
}

// A MARKER OFF THE SCREEN IS NOT HELD AT ITS EDGE. On a monitor a mappin that should clamp (sub_14054A0B4, the
// controllers' slot +352: tracked, an off-screen arrow, a quest) sits at the screen's edge while its point is off
// it. In the headset that edge is one eye's canvas about the head's forward, which the lens windows put 12% inside
// the outer eye and past the inner one: the marker at the edge of one eye only (2026-10-24, turned away from
// the car). Unclamped, the update puts the icon on its point's projection wherever that is (sub_14054A890 without
// its clamp), and the marker is seen where the point is, as an NPC's is. 1 = the game's clamp.
__declspec(dllexport) std::atomic<int32_t> CyberpunkVR_WorldMarkerEdgeClamp{0};
bool EdgeClampOff(void* controller) {
    return controller && !CyberpunkVR_WorldMarkerEdgeClamp.load(std::memory_order_relaxed) && Enabled() &&
        cvr::engine_stereo::UiOneEye() && WorldMappinController(static_cast<IScriptable*>(controller));
}
using ShouldClampFn=bool (*)(void*);
ShouldClampFn originalShouldClamp{};
bool ShouldClamp(void* controller) {
    return EdgeClampOff(controller) ? false : originalShouldClamp(controller);
}

using MappinPositionFn=uintptr_t (*)(void*,void*);
MappinPositionFn originalMappinPosition{};
void CollectMappin(void* controller,const void* position) {
    if(!Enabled()) return;
    CVR_DIAGNOSTIC(CyberpunkVR_WorldMarkerMappinStages[1].fetch_add(1,std::memory_order_relaxed));
    // This setter belongs to MappinBaseController. GetMappin's native thunk
    // copies exactly its weak handle at A0; the root is the SDK's weak handle40.
    // Follow the native setter's selected target, including the separate
    // presentation widget used by cached/attached mappins.
    const bool presentation=At<uint8_t>(controller,0x190)!=0;
    auto root=At<WeakHandle<ink::Widget>>(controller,presentation?0x170:0x40).Lock();
    if(!root) return;
    CVR_DIAGNOSTIC(CyberpunkVR_WorldMarkerMappinStages[2].fetch_add(1,std::memory_order_relaxed));
    if(!WorldMappinController(static_cast<IScriptable*>(controller))) return;
    if(presentation)ReleaseCachedRoot(At<WeakHandle<ink::Widget>>(controller,0x40).Lock());
    CVR_DIAGNOSTIC(CyberpunkVR_WorldMarkerMappinStages[3].fetch_add(1,std::memory_order_relaxed));
    if(!HudRoot(root)) return;
    CVR_DIAGNOSTIC(CyberpunkVR_WorldMarkerMappinStages[4].fetch_add(1,std::memory_order_relaxed));
    // Visibility can be assigned later in this same native update. Paint itself
    // gates hidden widgets; collecting the anchor must not depend on that order.
    const auto vt=*static_cast<uintptr_t**>(controller);
    const bool clamped=reinterpret_cast<bool (*)(void*)>(vt[0x158/8])(controller);
    auto mappin=At<WeakHandle<IScriptable>>(controller,0xA0).Lock();
    Vector4 world{};
    const auto getter=mappin ? mappin->GetType()->GetFunction("GetWorldPosition") : nullptr;
    // THE POINT IS KEPT EVEN WHEN THIS CONTROLLER DOES NOT PROJECT (+0xE3, SetProjectToScreenSpace off).
    // Such a root can still be drawn where the world point is: two quest mappins on one looted body,
    // one of them with the flag off, both visible at the same spot -- that one, given no depth, sat at
    // infinity and split from the other by the full parallax in the second view. A clamped edge icon
    // is not at its world point and keeps no depth.
    const bool valid=!clamped && getter && ExecuteFunction(mappin.GetPtr(),getter,&world);
    float p[]={world.X,world.Y,world.Z};
    SourceProjection source;
    if(valid && ReprojectionEnabled()) {
        // Outside the update the presentation manager re-applies the last update's screen point when it swaps a
        // cached picture in (sub_140480C58): the last update's camera placed it. Without one that frame was drawn
        // flat, off the target.
        source=mappinViewContext ? CaptureSource(At<void*>(mappinViewContext,0),&At<float>(mappinViewContext,80))
            : CurrentUiSource();
        float actual[4]{};
        using PointFn=const float* (*)(void*,float*);
        const auto* point=reinterpret_cast<PointFn>(vt[312/8])(controller,actual);
        if(point && source.Valid())std::memcpy(p,point,sizeof(p));
    }
    // A POINT OFF THE UI'S SCREEN IS NOT WHERE ITS ICON IS -- while the game clamps. Its icon is at the screen's edge
    // or off it, whatever the clamp state says, and the point's depth along the view is small when it is off to the
    // side (0.3 m measured): an edge icon lifted to that depth hung in front of the face, at the edge in one eye and
    // halfway in in the other.
    // UNCLAMPED (EdgeClampOff) THE ICON IS ON ITS POINT, ON THE SCREEN OR OFF IT, and is lifted to it: each eye shows
    // it where it sees the point, the outer eye past the canvas's edge too. Kept flat off the canvas it took the
    // engine's own stereo term (WorldMarkers.hlsl StereoSprite; 2.2 NDC on the teleport marker, markeroutline.rdc
    // event 94960) and stood in one eye only (2026-10-24: the car's and the teleport's markers, right eye only).
    // A point behind the head has no place on the screen -- the update's projection mirrors it in front -- and the
    // marker is not drawn (Paint).
    // ONLY WHERE THE PLAYER LOOKS (CyberpunkVR_WorldMarkerOffCanvas 0): a point that one eye cannot see is not drawn
    // either -- off the canvas, or on its edge outside the inner eye's window. Drawn at its point it stood at the edge
    // of one eye only whenever the player turned away (2026-10-27, V's apartment: a quest marker 5.6 m off to the
    // left, at canvas x 175 of 2648, in the left eye only).
    const bool unclamped=EdgeClampOff(controller);
    bool atPoint=valid,behind=false;
    if(valid && source.Valid()) {
        MarkerAnchor at;
        const bool ahead=AnchorOf(source,p,at);
        const bool onCanvas=at.canvas[0]>=0 && at.canvas[0]<=source.width && at.canvas[1]>=0 && at.canvas[1]<=source.height;
        atPoint=ahead && (unclamped || onCanvas);
        const bool inSight=onCanvas && BothEyesSee(p);
        behind=unclamped && (!ahead || (!inSight && !CyberpunkVR_WorldMarkerOffCanvas.load(std::memory_order_relaxed)));
    }
    if(valid && mappinViewContext && CyberpunkVR_WorldMarkerProbe.load(std::memory_order_relaxed)) {
        float actual[4]{};
        using PointFn=const float* (*)(void*,float*);
        const auto* point=reinterpret_cast<PointFn>(vt[312/8])(controller,actual);
        if(point)ProbeView(0,root.GetPtr(),At<void*>(mappinViewContext,0),point,
            &At<float>(mappinViewContext,80),static_cast<const float*>(position),p);
    }
    // Stealth icons can be reparented into the NPC nameplate. In that mode
    // they inherit its projection instead of masking the parent depth with 0;
    // with no projected parent to inherit from they take their own point (Paint).
    Vector2 cachePivot{};
    const bool pinCache=presentation && ReprojectionEnabled() && CachedMarkerPivot(root->size.X,root->size.Y,
        At<float>(controller,0x168),At<float>(controller,0x16C),cachePivot.X,cachePivot.Y);
    Remember(root,p,atPoint,true,!At<uint8_t>(controller,0xE3),!presentation,&source,pinCache?&cachePivot:nullptr,presentation,behind);
    TraceMappin(controller,root.GetPtr(),p,static_cast<const float*>(position),source,atPoint,clamped);
    CVR_DIAGNOSTIC(CyberpunkVR_WorldMarkerMappinStages[5].fetch_add(1,std::memory_order_relaxed));
    if(valid) CVR_DIAGNOSTIC(CyberpunkVR_WorldMarkerMappinStages[6].fetch_add(1,std::memory_order_relaxed));
}
// THE MARKER'S MARGIN UNROUNDED. The screen-position setter (sub_14054A0F8) rounds the margin to whole design
// units -- 1.326 canvas pixels on the 2.5 HUD -- for the live root (screen, plus a size term only under the larger
// marker option) and the cached picture (screen + cache offset + sub_142232480's adjustment). Lifted into the world
// from that, every marker stepped against the world by up to 0.66 px while the head moved. The exact margin goes
// back through the same setter (sub_14054B3A4), and only where it rounds to what the game wrote.
void ExactMargin(void* controller,const float* screen) {
    if(!screen || !At<uint8_t>(controller,0xE3) || !CyberpunkVR_WorldMarkerExactMargin.load(std::memory_order_relaxed) ||
       !WorldMappinController(static_cast<IScriptable*>(controller)))return;
    const bool presentation=At<uint8_t>(controller,0x190)!=0;
    const auto widget=At<WeakHandle<ink::Widget>>(controller,presentation?0x170:0x40).Lock();
    if(!widget)return;
    float exact[2]={screen[0],screen[1]};
    if(presentation) {
        float adjust[2]{};
        using AdjustFn=float* (*)(void*,float*,const float*);
        reinterpret_cast<AdjustFn>(base+0x2232480)(controller,adjust,&widget->size.X);
        exact[0]+=At<float>(controller,0x168)+adjust[0];exact[1]+=At<float>(controller,0x16C)+adjust[1];
    }
    const auto& margin=widget->layout.margin;
    if(!std::isfinite(exact[0]) || !std::isfinite(exact[1]) || margin.right!=0 || margin.bottom!=0 ||
       std::abs(margin.left-exact[0])>.5001f || std::abs(margin.top-exact[1])>.5001f)return;
    if(margin.left==exact[0] && margin.top==exact[1])return;
    const float value[4]={exact[0],exact[1],0,0};
    reinterpret_cast<void (*)(ink::Widget*,const float*)>(base+0x54B3A4)(widget.GetPtr(),value);
    CVR_DIAGNOSTIC(CyberpunkVR_WorldMarkerExactMargins.fetch_add(1,std::memory_order_relaxed));
}
uintptr_t MappinPosition(void* controller,void* position) {
    CVR_DIAGNOSTIC(CyberpunkVR_WorldMarkerMappinStages[0].fetch_add(1,std::memory_order_relaxed));
    const auto result=originalMappinPosition(controller,position);
    if(ReprojectionEnabled())ExactMargin(controller,static_cast<const float*>(position));
    if(!ReprojectionEnabled() || !mappinViewContext)CollectMappin(controller,position);
    return result;
}

using ProjectionFn=void (*)(void*,void*,void*,float);
ProjectionFn originalProjection{};
void Projection(void* projection,void* view,void* size,float worldScale) {
    CVR_DIAGNOSTIC(CyberpunkVR_WorldMarkerProjectionStages[0].fetch_add(1,std::memory_order_relaxed));
    originalProjection(projection,view,size,worldScale);
    if(!Enabled()) return;
    CVR_DIAGNOSTIC(CyberpunkVR_WorldMarkerProjectionStages[1].fetch_add(1,std::memory_order_relaxed));
    Handle<ink::Widget> root;
    {
        std::lock_guard lock(projectionMutex);
        const auto it=projectionOwners.find(projection);
        if(it!=projectionOwners.end() && it->second.projection.Lock().GetPtr()==projection && GetTickCount64()-it->second.stamp<1000)
            root=it->second.widget.Lock();
    }
    if(!root) {
        auto owner=At<WeakHandle<IScriptable>>(projection,0xD0).Lock();
        const auto logicClass=CRTTISystem::Get()->GetClass("inkIWidgetLogicController");
        if(!owner || !logicClass || !owner->GetType()->IsA(logicClass)) return;
        root=At<WeakHandle<ink::Widget>>(owner.GetPtr(),0x40).Lock();
    }
    CVR_DIAGNOSTIC(CyberpunkVR_WorldMarkerProjectionStages[2].fetch_add(1,std::memory_order_relaxed));
    if(!root || !HudRoot(root)) return;
    CVR_DIAGNOSTIC(CyberpunkVR_WorldMarkerProjectionStages[3].fetch_add(1,std::memory_order_relaxed));
    float world[3]{};
    // The projection update's own world point: entity/slot lookup with the static +B0 fallback, the
    // +C0 offset (2/worldScale when +E0 adjusts), the distance-scaled +E4 offset. 2.31 had this inline
    // and the port repeated it; 2.5 moved it into this function (projection, out, view, worldScale),
    // which also refreshes +104 with the distance the update just stored.
    using WorldFn=float* (*)(void*,float*,void*,float);
    reinterpret_cast<WorldFn>(base+0x2142700)(projection,world,view,worldScale);
    ProbeView(1,root.GetPtr(),view,world,static_cast<const float*>(size),&At<float>(projection,272));
    const auto source=CaptureSource(view,static_cast<const float*>(size));
    Remember(root,world,true,false,false,false,&source);
    CVR_DIAGNOSTIC(CyberpunkVR_WorldMarkerProjectionStages[4].fetch_add(1,std::memory_order_relaxed));
}

using PaintFn=void (*)(ink::Widget*,void*);
PaintFn originalPaint{};
void Paint(ink::Widget* widget,void* context) {
    const float previous=paintDepth;
    const auto previousOwner=paintOwner;
    const auto previousSource=paintSource;
    const auto previousAnchor=paintAnchor;
    bool hidden=false;
    if(Enabled()) {
        std::shared_lock lock(rootsMutex);
        const auto it=roots.find(widget);
        if(it!=roots.end()) {
            const auto live=it->second.root.Lock();
            // A marker whose point is behind the head (CollectMappin) stays as its update last left it.
            hidden=live.GetPtr()==widget && it->second.hidden;
            const float own=live.GetPtr()==widget && GetTickCount64()-it->second.stamp<1000
                ? it->second.inverseDepth : 0;
            // An inheriting root keeps the depth of the projected root it is painted inside (a
            // nameplate); with none around it, it is painted at its own point instead of at infinity.
            if(!it->second.inheritDepth || paintDepth<=0) {
                paintDepth=own;
                paintOwner=paintDepth>0?reinterpret_cast<uintptr_t>(widget):0;
                paintSource=paintDepth>0?it->second.source:SourceProjection{};
                paintAnchor=paintDepth>0?it->second.anchor:MarkerAnchor{};
            }
        }
        // Diagnostic: a marker's widget painted by a pass of its own, outside its root's Paint (an effect's
        // content?), gets no depth. Counted with the root and widget.
        if(!paintNesting && paintDepth<=0 && it==roots.end() && ::cvr::RuntimeDiagnosticsEnabled() && widget) {
            auto parent=widget->parentWidget.Lock();
            for(int i=0;parent && i<24;++i,parent=parent->parentWidget.Lock()) {
                const auto found=roots.find(parent.GetPtr());
                if(found==roots.end())continue;
                CyberpunkVR_WorldMarkerDetachedPaints.fetch_add(1,std::memory_order_relaxed);
                CyberpunkVR_WorldMarkerDetachedPaint[0]=reinterpret_cast<uintptr_t>(parent.GetPtr());
                CyberpunkVR_WorldMarkerDetachedPaint[1]=reinterpret_cast<uintptr_t>(widget);
                CyberpunkVR_WorldMarkerDetachedPaint[2]=uint64_t(i+1);
                CyberpunkVR_WorldMarkerDetachedPaint[3]=GetTickCount64();
                break;
            }
        }
    } else {paintDepth=0;paintOwner=0;paintSource={};paintAnchor={};}
    if(!hidden) {
        ++paintNesting;
        originalPaint(widget,context);
        --paintNesting;
    }
    paintDepth=previous;
    paintOwner=previousOwner;
    paintSource=previousSource;
    paintAnchor=previousAnchor;
}

// A pool can reallocate while collecting a frame. Keep metadata by pool/index
// and rebuild the address index on growth; never infer ownership from rectangles.
struct PrimitivePool { uintptr_t data{}; std::vector<MarkerOwner> depths; uint64_t stamp{}; };
struct PrimitiveDepthEntry { void* owner{}; MarkerOwner marker; };
std::mutex primitiveMutex;
std::unordered_map<void*,PrimitivePool> pools;
std::unordered_map<void*,PrimitiveDepthEntry> primitives;
void ForgetPoolRecords(void* owner,const PrimitivePool& pool) {
    for(size_t i=0;i<pool.depths.size();++i) {
        const auto it=primitives.find(reinterpret_cast<void*>(pool.data+i*0x130));
        if(it!=primitives.end() && it->second.owner==owner) primitives.erase(it);
    }
}
using AllocateFn=void* (*)(void*);
AllocateFn originalAllocate{};
void* Allocate(void* batch) {
    void* result=originalAllocate(batch);
    if(!result || !hooksReady.load(std::memory_order_acquire)) return result;
    void* owner=At<void*>(batch,0x28);
    const auto data=At<uintptr_t>(owner,0);
    const auto address=reinterpret_cast<uintptr_t>(result);
    if(address<data || (address-data)%0x130 || (address-data)/0x130>131072) return result;
    const size_t index=(address-data)/0x130;
    std::lock_guard lock(primitiveMutex);
    auto& pool=pools[owner];
    if(index==0 || pool.data!=data) {
        ForgetPoolRecords(owner,pool);
        if(index==0) pool.depths.clear();
        else for(size_t i=0;i<pool.depths.size();++i)
            if(pool.depths[i].depth>0) primitives[reinterpret_cast<void*>(data+i*0x130)]={owner,pool.depths[i]};
        pool.data=data;
    }
    pool.stamp=GetTickCount64();
    if(pool.depths.size()<=index) pool.depths.resize(index+1);
    pool.depths[index]={paintOwner,paintDepth,paintSource,paintAnchor};
    if(paintDepth>0) primitives[result]={owner,pool.depths[index]}; else primitives.erase(result);
    static uint64_t nextPrune=0;
    if(pool.stamp>=nextPrune) {
        const uint64_t now=pool.stamp;nextPrune=now+10000;
        for(auto it=pools.begin();it!=pools.end();) {
            if(now-it->second.stamp>60000) {
                ForgetPoolRecords(it->first,it->second);it=pools.erase(it);
            } else ++it;
        }
    }
    return result;
}
MarkerOwner PrimitiveMarker(void* record) {
    if(!Enabled()) return {};
    std::lock_guard lock(primitiveMutex);
    const auto it=primitives.find(record);
    if(it==primitives.end()) return {};
    const auto pool=pools.find(it->second.owner);
    if(pool!=pools.end()) pool->second.stamp=GetTickCount64();
    return it->second.marker;
}

// AN EFFECT IS ITS MARKER'S, EVEN WITH A FEW RECORDS OF NO MARKER IN IT. A flattened effect (a marker's glow or
// shadow, composed offscreen and drawn back as one quad) took its marker's projection only when every record in it
// had one; a single record without (CyberpunkVR_WorldMarkerCompositeStats[2]) left the quad flat, at infinity in
// each eye's window: a blurred double of the marker beside it, left in the left eye, right in the right one.
// Records of one marker and no other then decide, unless the ownerless ones outnumber them (an effect over the
// ordinary HUD with a marker inside). Two markers in one effect still keep it flat.
MarkerOwner CompositeOwner(void* source) {
    MarkerOwner common;
    if(!source || !Enabled())return common;
    const auto count=At<uint32_t>(source,0xC);
    const auto* indices=At<uint32_t*>(source,0);
    const auto batchPool=At<void*>(source,0x10);
    if(!indices || !batchPool || count>4096)return common;
    const auto batches=At<uintptr_t>(batchPool,0);
    const auto batchCount=At<uint32_t>(batchPool,0x10);
    if(!batches || batchCount>131072)return common;
    uint32_t owned=0,ownerless=0;
    {
        std::lock_guard lock(primitiveMutex);
        const auto unknown=[] {
            CVR_DIAGNOSTIC(CyberpunkVR_WorldMarkerCompositeStats[5].fetch_add(1,std::memory_order_relaxed));
            return MarkerOwner{};
        };
        for(uint32_t i=0;i<count;++i) {
            if(indices[i]>=batchCount)return unknown();
            auto* batch=reinterpret_cast<void*>(batches+size_t(indices[i])*0xC0); // 2.31: 0xB0
            const auto records=At<uint32_t*>(batch,0x18);
            const auto size=At<uint32_t>(batch,0x30);
            if(!size)continue;
            if(!records || size>131072)return unknown();
            const auto owner=At<void*>(batch,0x28);
            const auto pool=pools.find(owner);
            if(pool==pools.end() || pool->second.data!=At<uintptr_t>(owner,0))return unknown();
            for(uint32_t j=0;j<size;++j) {
                if(records[j]>=pool->second.depths.size())return unknown();
                const auto& record=pool->second.depths[records[j]];
                if(!record.root || !(record.depth>0)) {++ownerless;continue;}
                if(!MergeEffectOwner(common,record)) {
                    CVR_DIAGNOSTIC(CyberpunkVR_WorldMarkerCompositeStats[3].fetch_add(1,std::memory_order_relaxed));
                    return {};
                }
                ++owned;
            }
        }
    }
    if(!owned) {
        CVR_DIAGNOSTIC(CyberpunkVR_WorldMarkerCompositeStats[4].fetch_add(1,std::memory_order_relaxed));
        return {};
    }
    if(!ownerless) {
        CVR_DIAGNOSTIC(CyberpunkVR_WorldMarkerCompositeStats[0].fetch_add(1,std::memory_order_relaxed));
        return common;
    }
    CVR_DIAGNOSTIC(CyberpunkVR_WorldMarkerCompositeStats[2].fetch_add(1,std::memory_order_relaxed));
    CyberpunkVR_WorldMarkerCompositeMixed[0]=common.root;CyberpunkVR_WorldMarkerCompositeMixed[1]=owned;
    CyberpunkVR_WorldMarkerCompositeMixed[2]=ownerless;CyberpunkVR_WorldMarkerCompositeMixed[3]=GetTickCount64();
    if(!CyberpunkVR_WorldMarkerCompositeLoose.load(std::memory_order_relaxed) || ownerless>owned)return {};
    CVR_DIAGNOSTIC(CyberpunkVR_WorldMarkerCompositeStats[1].fetch_add(1,std::memory_order_relaxed));
    return common;
}
using CompositeFn=uintptr_t (*)(void*,void*,void*,void*,void*,void*,uint8_t);
CompositeFn originalComposite{};
uintptr_t Composite(void* context,void* destination,void* source,void* writer,void* texture,void* bounds,uint8_t mode) {
    const auto owner=CompositeOwner(source);
    const auto savedOwner=paintOwner;const float savedDepth=paintDepth;
    const auto savedSource=paintSource;const auto savedAnchor=paintAnchor;
    paintOwner=owner.root;paintDepth=owner.depth;
    paintSource=owner.source;paintAnchor=owner.anchor;
    ++offscreenPass;
    const auto result=originalComposite(context,destination,source,writer,texture,bounds,mode);
    --offscreenPass;paintOwner=savedOwner;paintDepth=savedDepth;paintSource=savedSource;paintAnchor=savedAnchor;
    return result;
}
struct VertexStream { void* geometry; uint8_t* vertices; uint32_t count; };
static_assert(offsetof(VertexStream,count)==0x10 && sizeof(VertexStream)==0x18);
using EmitFn=void (*)(void*,void*,VertexStream*);
EmitFn originalQuad{},originalTriangles{};
// The layers of one marker (CyberpunkVR_WorldMarkerLayerRoot), as emitted: kind 1 quad, 2 triangles, 3 text.
void NoteLayer(uintptr_t root,void* record,uint32_t kind,const uint8_t* vertices,uint32_t count,bool lift,const float* bounds) {
    const auto n=CyberpunkVR_WorldMarkerLayerCount.fetch_add(1,std::memory_order_relaxed)%64;
    auto* w=CyberpunkVR_WorldMarkerLayerWords[n];auto* f=CyberpunkVR_WorldMarkerLayerFloats[n];
    w[0]=reinterpret_cast<uintptr_t>(record);w[1]=kind|(lift?256:0)|(offscreenPass?512:0)|(uint64_t(count)<<16);
    w[2]=GetTickCount64();w[3]=0;
    if(vertices && count) {std::memcpy(&w[3],vertices+12,8);}
    if(bounds)std::memcpy(f,bounds,16);
    else if(vertices && count) {
        const auto* v=reinterpret_cast<const float*>(vertices);
        f[0]=f[2]=v[0];f[1]=f[3]=v[1];
        for(uint32_t i=1;i<count;++i) {
            v=reinterpret_cast<const float*>(vertices+size_t(i)*24);
            f[0]=std::min(f[0],v[0]);f[1]=std::min(f[1],v[1]);f[2]=std::max(f[2],v[0]);f[3]=std::max(f[3],v[1]);
        }
    }
    for(int i=0;i<4;++i)f[4+i]=0;
    if(vertices && count) {
        uint32_t uv=0;std::memcpy(&uv,vertices+20,4);f[4]=float(uv&0xFFFF);f[5]=float(uv>>16);
        std::memcpy(&uv,vertices+size_t(count-1)*24+20,4);f[6]=float(uv&0xFFFF);f[7]=float(uv>>16);
    }
    (void)root;
}
// Level about the marker's point when it has one (CanvasToWorldLevel), the source camera's own axes otherwise.
bool LiftMarker(const MarkerOwner& marker,float x,float y,float world[3]) {
    if(CyberpunkVR_WorldMarkerLevel.load(std::memory_order_relaxed) && marker.anchor.valid)
        return CanvasToWorldLevel(marker.source,marker.anchor,marker.depth,x,y,world);
    return CanvasToWorld(marker.source,marker.depth,x,y,world);
}
void Emit(EmitFn original,void* context,void* record,VertexStream* stream) {
    const auto before=stream->count;
    const auto marker=PrimitiveMarker(record);
    const float encoded=EncodeDepth(marker.depth);
    // Pixel-snapped geometry, once lifted into the world, steps against it while the head moves (QuadSnap).
    const bool previousUnsnap=unsnapVertices;
    unsnapVertices=encoded && CanLift(marker.source) && CyberpunkVR_WorldMarkerUnsnap.load(std::memory_order_relaxed);
    original(context,record,stream);
    unsnapVertices=previousUnsnap;
    if(!encoded || !stream->vertices || stream->count<before || stream->count-before>65536) return;
    if(CyberpunkVR_WorldMarkerProbe.load(std::memory_order_relaxed) && !offscreenPass && before<stream->count) {
        std::lock_guard lock(traceMutex);
        CyberpunkVR_WorldMarkerTraceSequence.fetch_add(1,std::memory_order_acq_rel);
        const auto slot=TraceSlot(marker.root);auto* w=CyberpunkVR_WorldMarkerTraceWords[slot];auto* f=CyberpunkVR_WorldMarkerTraceFloats[slot];
        if(w[0]==marker.root) {
            w[5]=GetTickCount64();w[6]=reinterpret_cast<uintptr_t>(record);w[7]=(w[7]&15)|(CanLift(marker.source)?16:0);
            const float* first=reinterpret_cast<float*>(stream->vertices+size_t(before)*24);
            f[24]=f[26]=first[0];f[25]=f[27]=first[1];
            for(uint32_t n=before+1;n<stream->count;++n){const auto* v=reinterpret_cast<float*>(stream->vertices+size_t(n)*24);f[24]=std::min(f[24],v[0]);f[25]=std::min(f[25],v[1]);f[26]=std::max(f[26],v[0]);f[27]=std::max(f[27],v[1]);}
            std::memcpy(f+28,marker.source.origin,12);std::memcpy(f+32,marker.source.forward,12);
            LiftMarker(marker,(f[24]+f[26])*.5f,(f[25]+f[27])*.5f,f+36);
            f[39]=marker.depth;
            LiftMarker(marker,(f[24]+f[26])*.5f,f[25],f+40);
        }
        if(marker.root && marker.root==CyberpunkVR_WorldMarkerLayerRoot)
            NoteLayer(marker.root,record,original==originalTriangles?2:1,stream->vertices+size_t(before)*24,stream->count-before,
                      CanLift(marker.source),nullptr);
        CyberpunkVR_WorldMarkerTraceSequence.fetch_add(1,std::memory_order_release);
    }
    for(uint32_t i=before;i<stream->count;++i) {
        auto* position=reinterpret_cast<float*>(stream->vertices+size_t(i)*24);
        auto& alpha=*reinterpret_cast<uint16_t*>(stream->vertices+size_t(i)*24+18);
        auto& z=position[2];
        int reason=alpha&0x8000 ? 0 : z!=0 && !(DecodeDepth(z)>0) ? 1 : offscreenPass ? 2 : !CanLift(marker.source) ? 3 : 4;
        if(reason==4) {
            float world[3];
            if(LiftMarker(marker,position[0],position[1],world)) {
                std::memcpy(position,world,sizeof(world));alpha|=0x8000;
                CVR_DIAGNOSTIC(CyberpunkVR_WorldMarkerWorldVertices.fetch_add(1,std::memory_order_relaxed));
                continue;
            }
        }
        CVR_DIAGNOSTIC((CyberpunkVR_WorldMarkerUnlifted[reason].fetch_add(1,std::memory_order_relaxed),
                        CyberpunkVR_WorldMarkerUnliftedRoot[reason]=marker.root));
        if(z==0) z=encoded;
    }
    CVR_DIAGNOSTIC(CyberpunkVR_WorldMarkerQuads.fetch_add(1,std::memory_order_relaxed));
}
void Quad(void* context,void* record,VertexStream* stream) { Emit(originalQuad,context,record,stream); }
void Triangles(void* context,void* record,VertexStream* stream) { Emit(originalTriangles,context,record,stream); }
// 2.5: (record, writer, batch+50); 2.31 took (record, writer) and read the batch itself.
using TextFn=void (*)(void*,void*,void*);
TextFn originalText{};
void Text(void* record,void* context,void* batchData) {
    const float previous=textDepth;
    const auto previousSource=textSource;
    const auto previousAnchor=textAnchor;
    const auto marker=PrimitiveMarker(record);textDepth=marker.depth;textSource=marker.source;textAnchor=marker.anchor;
    // The record's own translation, before the builder rounds it to the pixel (TextTranslationExact).
    const float previousExact[2]={textExact[0],textExact[1]};const bool previousExactValid=textExactValid;
    textExactValid=record && marker.depth>0;
    const auto previousLayerRoot=textLayerRoot;textLayerRoot=marker.root;
    if(textExactValid) {textExact[0]=At<float>(record,0x10);textExact[1]=At<float>(record,0x14);}
    originalText(record,context,batchData);
    textExact[0]=previousExact[0];textExact[1]=previousExact[1];textExactValid=previousExactValid;
    textLayerRoot=previousLayerRoot;
    textDepth=previous;
    textSource=previousSource;
    textAnchor=previousAnchor;
}

// The quad emitter's vertex transform (sub_14028A6D0, from sub_14028A064 only) rounds each vertex to the canvas
// pixel when the batch snaps. Not for a marker that is lifted into the world: there the pixel is no grid.
using SnapFn=void (*)(void*,float*,float*,float*,char);
SnapFn originalSnap{};
void QuadSnap(void* context,float* begin,float* end,float* matrix,char snap) {
    originalSnap(context,begin,end,matrix,unsnapVertices ? 0 : snap);
}

// THE FLAT HUD UNDER THE ASYMMETRIC PROJECTION. Each eye renders its own off-centre window
// (Runtimes/AsymmetricProjection.hpp), so straight ahead is not the middle of either picture: it sits at
// the view's NDC shift. The engine knows nothing of that on the HUD side -- markers are projected with
// the game camera's centred frustum and the HUD canvas is composed over each view pixel for pixel.
// Measured on the Quest 3 profile (build/optics-20261011/asym_hud.py): a mappin landed on the same
// pixel in both eyes, exactly where the centred frustum puts it, while its target sat 116 px to either
// side. Adding the view's own shift to the canvas camera's translation fixes both kinds of element at
// once: a projected marker lands on its target, and a screen-fixed one (crosshair, prompts) sits
// straight ahead in each eye instead of at the middle of an off-centre picture.
//
// AND THE CANVAS IS SCALED TO THE WINDOW'S ASPECT. The window's aspect is the headset's (Quest 3:
// 0.9407), the canvas's is the game resolution's (2560x2848: 0.8989), and the game camera that
// places markers works at the latter -- measured: markers 4.5% too far from the centre, exactly the
// ratio. Squeezing the canvas horizontally by canvas aspect / window aspect puts them back, and gives
// screen-fixed elements square pixels again (the world is drawn into the same non-square target).
//
// Only the canvas camera, recognised by the eye's aspect: the separately drawn HUD slots (health,
// minimap, ... the free-look panel's sources) have their own cameras of their own sizes and must stay
// where they are in their textures. Never in menus.
//
// UNDER THE ENGINE'S STEREO (Stereo/EngineStereo.hpp) the UI pass runs once per eye, each with its own camera
// block (InkCameraEye: 1 left, 2 right), into that eye's half of the HUD target. The canvas is the window, two
// eyes wide (5296 x 2864 on the Quest 3 profile), and markers are placed in it by the same centred frustum, now
// at the canvas's aspect: the eyes' vertical span, 1.85 times as wide. Unmoved, each eye showed that whole canvas
// squeezed into its own half -- a mappin straight ahead stood 15 deg to the left of its target in one eye, 15 deg
// to the right in the other, and 12 deg low. The eye's own window in the canvas's terms is the move above: a
// squeeze of 2 (a canvas pixel is an eye pixel again), the centre to the eye's shift.
bool EngineEyeWindow(int eye,cvr::projection::ViewWindow& window) {
    cvr::projection::ViewWindow centre{};
    if(!cvr::projection::ViewWindowOf(0,&centre))return false;
    // The eyes share the centre's vertical span; with the eye windows off they render the centre's window.
    cvr::engine_stereo::EyeProjection own{};
    if(cvr::engine_stereo::EyeWindow(eye,&own))window={own.shiftX,own.shiftY,own.aspect*centre.halfY,centre.halfY};
    else window=centre;
    return true;
}
// eye: the engine's stereo eye (0 left, 1 right) the canvas is drawn into, -1 outside that mode.
// With the UI laid out for one eye (src/Stereo/EngineStereo.cpp) the canvas is one eye's under the engine's stereo
// too: drawn into the eye's half it is an eye pixel per canvas pixel already, and only the centre moves.
bool InkShift(int side,int eye,float width,float height,cvr::projection::ViewWindow& window,float& squeeze) {
    // Flat panels must not inherit the gameplay HUD's eye offset or squeeze.
    if(!CyberpunkVR_AsymHudShift.load(std::memory_order_relaxed) || GetMenuMode() || GetMenuRectMode() ||
       cvr::engine_stereo::OnePicture() || g_launcherHeight<=0 || !(height>0))return false;
    const float across=eye>=0 && !cvr::engine_stereo::UiOneEye() ? 2.0f : 1.0f;   // the eyes the canvas spans
    const float aspect=across*float(g_launcherWidth)/float(g_launcherHeight);
    if(std::fabs(width/height-aspect)>.01f*aspect)return false;
    if(eye>=0 ? !EngineEyeWindow(eye,window) : !cvr::projection::ViewWindowOf(side,&window))return false;
    squeeze=(width/height)/(window.halfX/window.halfY);
    return std::isfinite(squeeze) && squeeze>.5f*across && squeeze<2*across;
}

// The canvas camera last uploaded on this thread, per view (or per eye of the engine's stereo), for the text
// matrices drawn under it: its size, and the parallax and squeeze written into it.
// The second eye's marker parallax, in canvas NDC per inverse metre (the camera's x scale multiplies
// it into view NDC, which is why the asymmetric case divides by the squeeze).
float EyeScale(float width,float height) {
    const float zoom=CyberpunkVR_MainAdsZoomFactor;
    const float z=std::isfinite(zoom) && zoom>.05f ? zoom : 1;
    cvr::projection::ViewWindow window{};float squeeze=1;
    if(InkShift(1,-1,width,height,window,squeeze)) {
        // The second view's own window: ipd / distance radians, over its horizontal half span.
        const float ipd=2*CyberpunkVRPort_HalfIpd();
        if(!std::isfinite(ipd) || ipd<=0 || ipd>.5f)return 0;
        const float scale=ipd*z/window.halfX/squeeze;
        return CyberpunkVR_MainIsRightEye!=0 ? scale : -scale;
    }
    return StereoScale(2*CyberpunkVRPort_HalfIpd(),GetGameRenderFovDeg(),width/height,z,CyberpunkVR_MainIsRightEye!=0);
}
// The engine's stereo: markers are placed from the head centre and each eye sits half the IPD off it, so a
// marker at distance d moves half the IPD / d radians, right in the left eye -- in the eye's view NDC, the unit
// the camera's x row ends in (the replacement shaders add it after the squeeze).
float EngineEyeParallax(int eye,const cvr::projection::ViewWindow& window) {
    const float zoom=CyberpunkVR_MainAdsZoomFactor;
    const float z=std::isfinite(zoom) && zoom>.05f ? zoom : 1;
    const float half=CyberpunkVRPort_HalfIpd();
    if(!std::isfinite(half) || half<=0 || half>.25f)return 0;
    const float scale=half*z/window.halfX;
    return eye==0 ? scale : -scale;
}

bool InstallTextCall() {
    // The text record's geometry publication: movups [rbx+10],xmm7 (its colour), then the call that
    // publishes rbx (2.5 Text, sub_1422F63D0). 2.31 published through `lea rcx,[rbx+1F8]` + call, and
    // the port replaced that LEA; 2.5 passes the geometry itself (mov rcx,rbx), so there is no 7-byte
    // instruction left to take. The call's own target is redirected instead: the thunk tags the
    // geometry and tail-jumps to the publication, which sees the native return address it always had.
    auto* site=reinterpret_cast<uint8_t*>(base+0x22F66C3);
    const uint8_t expected[]={0x0f,0x11,0x7b,0x10,0xe8,0x58,0x74,0x98,0xfe};
    if(std::memcmp(site,expected,sizeof(expected))) return false;
    auto* call=site+4;
    int32_t original{};
    std::memcpy(&original,call+1,4);
    CvrWorldMarkerTextPublishTarget=reinterpret_cast<uintptr_t>(call+5)+original;
    auto* relay=static_cast<uint8_t*>(AllocateTrampoline(call,14));
    if(!relay) return false;
    const uint8_t jump[]={0xff,0x25,0,0,0,0};
    std::memcpy(relay,jump,sizeof(jump));
    const auto target=reinterpret_cast<uintptr_t>(&CvrWorldMarkerTextPublishDetour);
    std::memcpy(relay+6,&target,sizeof(target));
    const auto distance=reinterpret_cast<intptr_t>(relay)-reinterpret_cast<intptr_t>(call+5);
    if(distance<INT32_MIN || distance>INT32_MAX) return false;
    DWORD previous{};
    if(!VirtualProtect(call,5,PAGE_EXECUTE_READWRITE,&previous)) return false;
    const auto relative=static_cast<int32_t>(distance);
    std::memcpy(call+1,&relative,4);
    DWORD unused{};VirtualProtect(call,5,previous,&unused);
    FlushInstructionCache(GetCurrentProcess(),relay,14);
    FlushInstructionCache(GetCurrentProcess(),call,5);
    return true;
}

bool Install() {
    struct Hook { uintptr_t rva; const char* bytes; size_t size; void* detour; void** original; };
    const Hook hooks[]={
        {0x548FC4,"\x48\x8b\xc4\x48\x89\x58\x08\x55\x56\x57\x41\x54\x41\x55\x41\x56",16,reinterpret_cast<void*>(&MappinUpdate),reinterpret_cast<void**>(&originalMappinUpdate)},
        {0x54A0F8,"\x48\x89\x5c\x24\x10\x48\x89\x7c\x24\x20\x55\x48\x8b\xec",14,reinterpret_cast<void*>(&MappinPosition),reinterpret_cast<void**>(&originalMappinPosition)},
        {0x2A4B18,"\x48\x8b\xc4\x48\x89\x58\x10\x48\x89\x70\x18\x57\x48\x83\xec",15,reinterpret_cast<void*>(&Projection),reinterpret_cast<void**>(&originalProjection)},
        {0x27F26C,"\x48\x8b\xc4\x48\x89\x58\x08\x48\x89\x70\x10\x48\x89\x78\x18",15,reinterpret_cast<void*>(&Paint),reinterpret_cast<void**>(&originalPaint)},
        {0x28894C,"\x48\x8b\xc4\x48\x89\x58\x10\x48\x89\x68\x18\x48\x89\x70\x20",15,reinterpret_cast<void*>(&Allocate),reinterpret_cast<void**>(&originalAllocate)},
        {0x28A064,"\x48\x8b\xc4\x48\x89\x58\x08\x4c\x89\x40\x18\x48\x89\x50\x10",15,reinterpret_cast<void*>(&Quad),reinterpret_cast<void**>(&originalQuad)},
        {0x28A774,"\x48\x8b\xc4\x48\x89\x58\x18\x48\x89\x48\x08\x55\x56\x57",14,reinterpret_cast<void*>(&Triangles),reinterpret_cast<void**>(&originalTriangles)},
        {0x22F63D0,"\x48\x8b\xc4\x53\x55\x56\x48\x81\xec\xd0\x00\x00\x00",13,reinterpret_cast<void*>(&Text),reinterpret_cast<void**>(&originalText)},
        {0x1F4490,"\x48\x8b\xc4\x48\x89\x58\x08\x48\x89\x70\x10\x48\x89\x78\x20",15,reinterpret_cast<void*>(&Composite),reinterpret_cast<void**>(&originalComposite)},
        {0x28A6D0,"\x48\x89\x5c\x24\x08\x48\x89\x74\x24\x10\x57\x48\x83\xec\x30",15,reinterpret_cast<void*>(&QuadSnap),reinterpret_cast<void**>(&originalSnap)},
        {0x54A0B4,"\x48\x89\x5c\x24\x08\x57\x48\x83\xec\x20\x48\x8b\xf9",13,reinterpret_cast<void*>(&ShouldClamp),reinterpret_cast<void**>(&originalShouldClamp)},
    };
    for(const auto& h:hooks) if(std::memcmp(reinterpret_cast<void*>(base+h.rva),h.bytes,h.size)) {
        Log("[world-markers] signature mismatch at %llX; native placement retained\n",static_cast<unsigned long long>(h.rva));return false;
    }
    size_t made=0;
    for(const auto& h:hooks) {
        if(MH_CreateHook(reinterpret_cast<void*>(base+h.rva),h.detour,h.original)!=MH_OK) break;
        ++made;
    }
    bool ok=made==std::size(hooks);
    if(ok) for(const auto& h:hooks) if(MH_EnableHook(reinterpret_cast<void*>(base+h.rva))!=MH_OK) { ok=false;break; }
    if(ok) ok=InstallTextCall();
    if(!ok) {
        for(size_t i=0;i<made;++i) { MH_DisableHook(reinterpret_cast<void*>(base+hooks[i].rva));MH_RemoveHook(reinterpret_cast<void*>(base+hooks[i].rva)); }
        return false;
    }
    hooksReady.store(true,std::memory_order_release);
    Log("[world-markers] native projection ownership hooks installed\n");return true;
}
CVR_HOOK("WorldMarkers",cvr::hooks::Stage::Boot,83,Install);
}

void RegisterWidget(const RED4ext::Handle<RED4ext::ink::Widget>& widget,const RED4ext::Vector4& world) {
    const float point[3]={world.X,world.Y,world.Z};
    const auto source=CurrentUiSource();
    Remember(widget,point,world.W>.5f,false,false,false,&source);
}

void RegisterProjection(const Handle<ink::Widget>& widget,const Handle<IScriptable>& projection) {
    if(!widget || !HudRoot(widget))return;
    std::lock_guard lock(projectionMutex);
    for(auto it=projectionOwners.begin();it!=projectionOwners.end();) {
        const auto live=it->second.widget.Lock();
        if(!live || (live.GetPtr()==widget.GetPtr() && it->first!=projection.GetPtr()))it=projectionOwners.erase(it);else ++it;
    }
    if(projection)projectionOwners[projection.GetPtr()]={WeakHandle<IScriptable>(projection),WeakHandle<ink::Widget>(widget),GetTickCount64()};
    else Remember(widget,nullptr,false);
}

// Called before the native geometry publication, while its writer still owns it.
extern "C" void CvrWorldMarkerTextTag(void* geometry) {
    if(geometry && CyberpunkVR_WorldMarkerProbe.load(std::memory_order_relaxed)) {
        CyberpunkVR_WorldMarkerTextLast[0]=reinterpret_cast<uintptr_t>(geometry);
        CyberpunkVR_WorldMarkerTextLast[1]=GetTickCount64();
        CyberpunkVR_WorldMarkerTextLast[2]=At<uintptr_t>(geometry,0x240);
        CyberpunkVR_WorldMarkerTextLast[3]=detail::t_current_node_view;
        CyberpunkVR_WorldMarkerTextLast[4]=inkSlot+1;
    }
    if(geometry && CanLift(textSource)) {
        // Text colour is consumed by the native font renderer before the VS.
        // Keep its opacity intact; the affine matrix already identifies text.
        const bool level=CyberpunkVR_WorldMarkerLevel.load(std::memory_order_relaxed);
        auto* matrix=&At<float>(geometry,0x150);
        if(CyberpunkVR_WorldMarkerLayerRoot && CyberpunkVR_WorldMarkerProbe.load(std::memory_order_relaxed) && textAnchor.valid &&
           textLayerRoot==CyberpunkVR_WorldMarkerLayerRoot) {
            const float b[4]={matrix[12],matrix[13],textExactValid?textExact[0]:0,textExactValid?textExact[1]:0};
            NoteLayer(textLayerRoot,geometry,3,nullptr,0,true,b);
        }
        if(textExactValid && CyberpunkVR_WorldMarkerUnsnap.load(std::memory_order_relaxed))
            TextTranslationExact(matrix,textExact[0],textExact[1]);
        if(TextToWorld(matrix,textSource,textDepth,level ? &textAnchor : nullptr)) {
            CVR_DIAGNOSTIC(CyberpunkVR_WorldMarkerWorldTexts.fetch_add(1,std::memory_order_relaxed));
            return;
        }
    }
    const float encoded=EncodeDepth(textDepth);
    if(encoded && geometry && At<float>(geometry,0x188)==0 && At<float>(geometry,0x18C)==1) {
        At<float>(geometry,0x188)=encoded;
        CVR_DIAGNOSTIC(CyberpunkVR_WorldMarkerTexts.fetch_add(1,std::memory_order_relaxed));
    }
}
static_assert(kInkCameraBytes==kRewriteCameraBytes && kRewriteTextBytes==112 && kWorldCameraBytes==kRewriteScratchBytes);
// The width an eye's canvas camera's layout fills: one eye's of a camera two eyes wide while the UI is laid out for
// one eye (Stereo/EngineStereo.hpp UiOneEye) -- the camera keeps the target's size, the layer has the eye's.
float ContentWidth(float width,float height) {
    if(!cvr::engine_stereo::UiOneEye() || g_launcherHeight<=0 || !(height>0))return width;
    const float aspect=2.0f*float(g_launcherWidth)/float(g_launcherHeight);
    return std::fabs(width/height-aspect)<=.01f*aspect ? .5f*width : width;
}
bool RewriteConstants(uint32_t size,const void* source,void* scratch,uint32_t* shipped) {
    if(shipped)*shipped=size;
    if(!source || !scratch) return false;
    const int side=detail::t_view_side;
    const bool engineStereo=cvr::engine_stereo::Enabled();
    if(IsInkCamera(source,size)) {
        const auto* p=static_cast<const float*>(source);
        // The engine's stereo draws the UI once per eye; its centre (eye 0) draws none of the headset's.
        const int eye=engineStereo ? InkCameraEye(source)-1 : -1;
        const int slot=engineStereo ? eye : side;
        if(slot<0 || slot>1) return false;
        inkSlot=slot;
        inkCameras[slot]={p[188],p[189],0,1,GetTickCount64()};
        CVR_DIAGNOSTIC(CyberpunkVR_WorldMarkerCameraUploads[slot].fetch_add(1,std::memory_order_relaxed));
        if(offscreenPass) return false;
        WorldMarkerCamera worldCamera{};
        const bool worldReady=engineStereo && Enabled() && ReadEyeCamera(eye,p[188],p[189],worldCamera);
        inkCameras[slot].worldReady=worldReady;
        cvr::projection::ViewWindow window{};float squeeze=1;
        const float content=engineStereo ? ContentWidth(p[188],p[189]) : p[188];
        const bool shift=InkShift(side,eye,content,p[189],window,squeeze);
        // Both eyes of the engine's stereo are off the centre the markers were placed from; of the port's own
        // two views only the second is.
        const bool parallax=Enabled() && (engineStereo ? shift : side==1);
        if(!parallax && !shift && !worldReady) return false;
        std::memcpy(scratch,source,size);
        if(engineStereo) {
            // Keep the native prefix intact; its CBV already rounds 864 to 1024.
            // Explicitly clear the tail on a missed camera instead of exposing
            // stale padding to a previously cached world-tagged UI batch.
            std::memcpy(static_cast<uint8_t*>(scratch)+kInkCameraBytes,&worldCamera,sizeof(worldCamera));
            if(shipped)*shipped=kWorldCameraBytes;
            if(worldReady)CVR_DIAGNOSTIC(CyberpunkVR_WorldMarkerWorldCameras[slot].fetch_add(1,std::memory_order_relaxed));
        }
        auto* out=static_cast<float*>(scratch);
        // Clip x = 2x/w - 1 + z * out[2], clip y = 1 - 2y/h (+ out[7]); w = 1 for the flat camera.
        // The squeeze scales x about the canvas centre, then the centre moves to the view's shift --
        // in every copy of the canvas matrix (ShiftInkCamera). The parallax stays in the first: only
        // the replacement sprite/text shaders read it, and only they decode the depth tag.
        if(parallax) out[2]=engineStereo ? EngineEyeParallax(eye,window) : EyeScale(p[188],p[189])*squeeze;
        if(shift) ShiftInkCamera(out,p,size,squeeze,window.shiftX,window.shiftY,content);
        inkCameras[slot].parallax=parallax ? out[2] : 0;inkCameras[slot].squeeze=squeeze;
        if(engineStereo) {
            // Each eye's move, once per change: what the engine wrote, what the eye gets.
            static float logged[2][4]{};
            float* l=logged[slot];
            if(l[0]!=out[0] || l[1]!=out[3] || l[2]!=out[7] || l[3]!=out[2]) {
                l[0]=out[0];l[1]=out[3];l[2]=out[7];l[3]=out[2];
                Log("[world-markers] eye %d canvas %.0fx%.0f: x %.6f %.4f y %.4f -> x %.6f %.4f y %.4f parallax %.5f "
                    "(squeeze %.4f window %.4f %.4f %.4f %.4f)\n",eye,p[188],p[189],p[0],p[3],p[7],out[0],out[3],out[7],out[2],
                    squeeze,window.shiftX,window.shiftY,window.halfX,window.halfY);
            }
        }
        return true;
    }
    // Direct uploads can decode here. Batched native composition bypasses these
    // uploaders; the replacement ink shaders also decode the same matrix tag.
    if(size!=112) return false;
    // Engine stereo's VS decodes both direct and batched text, preserving the
    // marker flag that suppresses the engine's separate widget stereo offset.
    if(engineStereo)return false;
    const auto* p=static_cast<const float*>(source);
    if(!TextMatrixDepth(p,size)) return false;
    std::memcpy(scratch,source,size);
    auto* out=static_cast<float*>(scratch);
    float shift=0;
    const int slot=engineStereo ? inkSlot : side;
    if(slot>=0 && slot<2) {
        CVR_DIAGNOSTIC(CyberpunkVR_WorldMarkerTextUploads[slot].fetch_add(1,std::memory_order_relaxed));
        const auto& camera=inkCameras[slot];
        // The camera's parallax is in view NDC after its squeeze; the text matrix moves in canvas pixels before it.
        if(Enabled() && !offscreenPass && camera.width>0 && camera.squeeze>0 && GetTickCount64()-camera.stamp<1000)
            shift=camera.parallax/camera.squeeze*camera.width*.5f;
    }
    ReprojectTextMatrix(out,shift);
    return true;
}
}

// Script-owned world indicators use the same paint/vertex path as mappins.
void VRWorldMarkerAnchor(RED4ext::IScriptable*,RED4ext::CStackFrame* stack,void*,int64_t) {
    RED4ext::Handle<RED4ext::ink::Widget> widget;RED4ext::Vector4 world{};
    RED4ext::GetParameter(stack,&widget);RED4ext::GetParameter(stack,&world);stack->code++;
    cvr::markers::RegisterWidget(widget,world);
}

void VRWorldMarkerProjection(RED4ext::IScriptable*,RED4ext::CStackFrame* stack,void*,int64_t) {
    RED4ext::Handle<RED4ext::ink::Widget> widget;RED4ext::Handle<RED4ext::IScriptable> projection;
    RED4ext::GetParameter(stack,&widget);RED4ext::GetParameter(stack,&projection);stack->code++;
    cvr::markers::RegisterProjection(widget,projection);
}
