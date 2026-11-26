// Hidden-area mask over the parts of each eye's render that the lens never shows
// (docs/hidden-area-mask-20261009.md).
//
// ClearDepthStencilView restricted to rectangles is the whole mechanism; it changes no pipeline,
// root signature, target or barrier of the engine's command list, so nothing has to be restored.
// While geometry is drawn the hidden cells hold the near plane (reverse-Z, GREATER_EQUAL), so no
// draw lands there. Whenever the scene depth leaves DEPTH_WRITE they get the far plane instead: the
// passes that read it then see sky, which they skip on their own. That reset is where the saving
// comes from; the near plane alone saves nothing, because the dominant G-buffer materials discard
// and export SV_Depth and so still run their pixel shaders.
#include "Render/HiddenArea.hpp"
#include "Render/HiddenAreaShaders.hpp"
#include "Render/RenderProbeScope.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include "Stereo/EngineStereo.hpp"
#include <algorithm>
#include <cmath>
#include <memory>
#include <mutex>
#include <unordered_map>

#if !CVR_RUST_VRIK
extern "C" int GetMenuMode();
extern "C" float GetGameRenderFovDeg();
extern "C" float GetGameRenderVerticalFovDeg();
extern "C" int CyberpunkVR_MainIsRightEye;
extern int g_launcherWidth;
extern int g_launcherHeight;
#endif

namespace {
#if !CVR_RUST_VRIK   // unused by the Rust build
struct ClearRecord { uint32_t node; int32_t side; uint32_t width, height, format, flags; float depth; uint32_t count, masked; };
#endif
// `before`/`after` are resource states, or barrier layouts with the top bit set for enhanced barriers.
#if !CVR_RUST_VRIK   // unused by the Rust build
struct BarrierRecord { uint32_t node; int32_t side; uint32_t before, after, subresource, flags, count, resets; };
#endif
}
extern "C" {
// Effective settings, also writable by live tests (an ini edit overwrites them again).
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/used_moved_exports_rest.rs.
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_HiddenAreaEnabled;
#else
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_HiddenAreaEnabled{0};
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/typed_moved_exports.rs.
__declspec(dllexport) extern std::atomic<float> CyberpunkVR_HiddenAreaRadius;
#else
__declspec(dllexport) std::atomic<float> CyberpunkVR_HiddenAreaRadius{115.0f};
#endif
// Only clears inside this render-graph node (work RVA) are masked; 0 = any node. Default:
// SetRenderTargetsGBuffer. The hologram, planar-reflection and water depths are square as well.
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/used_moved_exports_rest.rs.
#else
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_HiddenAreaNode{0xA00830};
// 1 = record every distinct native depth clear into CyberpunkVR_HiddenAreaClearLog.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_HiddenAreaLog{0};
#endif
#if !CVR_RUST_VRIK   // defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs
__declspec(dllexport) ClearRecord CyberpunkVR_HiddenAreaClearLog[32]{};
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/used_moved_exports_rest.rs.
#else
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_HiddenAreaMasked[2]{};
// 1 = put the hidden cells back to the far plane whenever the scene depth leaves DEPTH_WRITE;
// 2 = also back to the near plane when a G-buffer node takes it into DEPTH_WRITE again.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_HiddenAreaSky{2};
// Only barriers inside this node (work RVA) go to the far plane; 0 = every transition out of DEPTH_WRITE.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_HiddenAreaSkyNode{0};
#endif
// With CyberpunkVR_HiddenAreaLog: every distinct barrier on a masked scene depth.
#if !CVR_RUST_VRIK   // defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs
__declspec(dllexport) BarrierRecord CyberpunkVR_HiddenAreaBarrierLog[32]{};
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/used_moved_exports_rest.rs.
#else
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_HiddenAreaSkyResets[2]{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_HiddenAreaNearResets[2]{};
#endif
}

#if CVR_RUST_VRIK
// The Rust build: the mask is rust/crates/cvr-render/src/hidden_area_runtime.rs. The exports stay above, and the
// reads of the rest of the port stay here: the eye's frustum, the menu mode, engine stereo and the target's aspect.
// The runtime table is built in Rust (rust/crates/cvr-render/src/hidden_area_table.rs).
extern "C" {
}
namespace cvr::hidden_area {
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's live controls set them themselves (live_controls_poll.rs)
void SetSettings(Settings s) {
    s=Normalize(s);
    CyberpunkVR_HiddenAreaRadius.store(s.radius,std::memory_order_relaxed);
    CyberpunkVR_HiddenAreaEnabled.store(uint32_t(s.enabled),std::memory_order_relaxed);
}
#endif
}
#else
namespace cvr::hidden_area {
namespace {
constexpr uint32_t Cell=8, Margin=1;
constexpr size_t MaxRectsPerClear=64;
struct Cache {
    uint32_t width{},height{};float radius{};Fov rendered{},fov{};
    std::shared_ptr<const std::vector<Rect>> rects;
};
// The shader ellipse of an off-centre window is a fit (ShaderEllipseScales); kept until its inputs change.
struct EllipseCache { float radius{};Fov rendered{},fov{};bool ok{};float scales[4]{}; };
EllipseCache ellipseCache[2];
std::mutex mutex,logMutex;
Cache cache[2];
std::atomic<uint32_t> lastRects[2]{},lastWidth[2]{},lastHeight[2]{};
std::atomic<float> lastHidden[2]{};

struct Target { UINT format{},width{},height{};ID3D12Resource* resource{}; };   // identity only, not owned
std::mutex dsvMutex;
std::unordered_map<SIZE_T,Target> dsvs;
bool LookupDsv(SIZE_T handle,Target& out) {
    std::lock_guard lock(dsvMutex);
    const auto it=dsvs.find(handle);
    if(it==dsvs.end())return false;
    out=it->second;return true;
}

bool SceneDepthFormat(UINT f) {
    return f==DXGI_FORMAT_D32_FLOAT_S8X24_UINT || f==DXGI_FORMAT_R32G8X24_TYPELESS || f==DXGI_FORMAT_D32_FLOAT ||
           f==DXGI_FORMAT_R32_TYPELESS || f==DXGI_FORMAT_R32_FLOAT;
}
bool SameFov(const Fov& a,const Fov& b){return a.left==b.left && a.right==b.right && a.up==b.up && a.down==b.down;}

void Record(uint32_t node,int side,UINT format,UINT w,UINT h,D3D12_CLEAR_FLAGS flags,float depth,bool masked) {
    std::lock_guard lock(logMutex);
    for(auto& r:CyberpunkVR_HiddenAreaClearLog) {
        if(r.count && r.node==node && r.side==side && r.width==w && r.height==h && r.format==format && r.flags==uint32_t(flags) && r.depth==depth) {
            ++r.count;r.masked+=masked;return;
        }
        if(!r.count){r={node,side,w,h,format,uint32_t(flags),depth,1,masked?1u:0u};return;}
    }
}

void RecordBarrier(uint32_t node,int side,uint32_t before,uint32_t after,uint32_t subresource,uint32_t flags,bool reset) {
    std::lock_guard lock(logMutex);
    for(auto& r:CyberpunkVR_HiddenAreaBarrierLog) {
        if(r.count && r.node==node && r.side==side && r.before==before && r.after==after && r.subresource==subresource && r.flags==flags) {
            ++r.count;r.resets+=reset;return;
        }
        if(!r.count){r={node,side,before,after,subresource,flags,1,reset?1u:0u};return;}
    }
}

// Scene depths the masked clear has seen. A barrier names its resource, not a descriptor, so the
// reset needs a DSV of its own for it.
// The render graph also transitions it between nodes, where no view is current: there the view is
// the one whose clear last masked the resource.
constexpr size_t SceneSlots=8;
std::atomic<ID3D12Resource*> scenes[SceneSlots]{};
std::atomic<int32_t> sceneSides[SceneSlots]{};
std::atomic<uint32_t> sceneNext{0};
void RememberScene(ID3D12Resource* resource,int side) {
    if(!resource)return;
    for(size_t i=0;i<SceneSlots;++i)if(scenes[i].load(std::memory_order_relaxed)==resource){sceneSides[i].store(side,std::memory_order_relaxed);return;}
    const auto slot=sceneNext.fetch_add(1,std::memory_order_relaxed)%SceneSlots;
    sceneSides[slot].store(side,std::memory_order_relaxed);
    scenes[slot].store(resource,std::memory_order_relaxed);
}
// -2 = not a masked scene depth; otherwise the view to use.
int SceneSide(const ID3D12Resource* resource,int side) {
    if(!resource)return -2;
    for(size_t i=0;i<SceneSlots;++i)
        if(scenes[i].load(std::memory_order_relaxed)==resource)return side==0 || side==1?side:sceneSides[i].load(std::memory_order_relaxed);
    return -2;
}
// G-buffer nodes that take the scene depth back into DEPTH_WRITE: SetRenderTargetsGBuffer and
// SetRenderTargetsGBufferWithVelocityBuffer. Forward and sky phases keep the far plane, so the sky
// still fills the hidden cells and no stale colour stays there for bloom to spread.
bool GBufferNode(uint32_t node){return node==0xA00830 || node==0xB2A87C;}
DXGI_FORMAT DsvFormat(DXGI_FORMAT f) {
    switch(f) {
    case DXGI_FORMAT_R32G8X24_TYPELESS: case DXGI_FORMAT_D32_FLOAT_S8X24_UINT: return DXGI_FORMAT_D32_FLOAT_S8X24_UINT;
    case DXGI_FORMAT_R32_TYPELESS: case DXGI_FORMAT_D32_FLOAT: return DXGI_FORMAT_D32_FLOAT;
    default: return DXGI_FORMAT_UNKNOWN;
    }
}
// One descriptor per recording thread, written right before the clear that reads it.
constexpr UINT OwnSlots=64;
std::mutex ownMutex;
ID3D12Device* ownDevice{};
ID3D12DescriptorHeap* ownHeap{};
UINT ownStride{};
std::atomic<uint32_t> ownNext{0};
thread_local int ownSlot=-1;
bool OwnDsv(ID3D12Resource* resource,DXGI_FORMAT format,D3D12_CPU_DESCRIPTOR_HANDLE& out) {
    if(ownSlot<0)ownSlot=int(ownNext.fetch_add(1,std::memory_order_relaxed));
    if(ownSlot>=int(OwnSlots))return false;
    std::lock_guard lock(ownMutex);
    if(!ownHeap) {
        ID3D12Device* device{};
        if(FAILED(resource->GetDevice(IID_PPV_ARGS(&device))))return false;
        const D3D12_DESCRIPTOR_HEAP_DESC desc{D3D12_DESCRIPTOR_HEAP_TYPE_DSV,OwnSlots};
        if(FAILED(device->CreateDescriptorHeap(&desc,IID_PPV_ARGS(&ownHeap)))){device->Release();return false;}
        ownDevice=device;ownStride=device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_DSV);
    }
    out={ownHeap->GetCPUDescriptorHandleForHeapStart().ptr+size_t(ownSlot)*ownStride};
    D3D12_DEPTH_STENCIL_VIEW_DESC view{};view.Format=format;view.ViewDimension=D3D12_DSV_DIMENSION_TEXTURE2D;
    ownDevice->CreateDepthStencilView(resource,&view,out);
    return true;
}

std::shared_ptr<const std::vector<Rect>> RectsFor(int side,UINT w,UINT h);

// The hidden cells of `resource` to `depth`. Only recorded while the resource is in DEPTH_WRITE:
// before the barrier that ends its depth writes, or after the one that starts them.
bool ClearHidden(ID3D12GraphicsCommandList* list,ID3D12Resource* resource,int side,ClearFn clear,float depth) {
    // Engine stereo puts both eyes in one target; the mask is laid out for one eye per target.
    if(!clear || side<0 || side>1 || !CyberpunkVR_HiddenAreaEnabled.load(std::memory_order_relaxed) || GetMenuMode() ||
       cvr::engine_stereo::Enabled())return false;
    const auto d=resource->GetDesc();
    const DXGI_FORMAT format=DsvFormat(d.Format);
    if(format==DXGI_FORMAT_UNKNOWN || d.Dimension!=D3D12_RESOURCE_DIMENSION_TEXTURE2D || d.SampleDesc.Count!=1)return false;
    const auto cover=RectsFor(side,UINT(d.Width),d.Height);
    if(!cover || cover->empty())return false;
    stereo::probe::Scope internal;
    D3D12_CPU_DESCRIPTOR_HANDLE dsv;
    if(!OwnDsv(resource,format,dsv))return false;
    const auto* all=reinterpret_cast<const D3D12_RECT*>(cover->data());
    for(size_t at=0;at<cover->size();at+=MaxRectsPerClear)
        clear(list,dsv,D3D12_CLEAR_FLAG_DEPTH,depth,0,UINT(std::min(MaxRectsPerClear,cover->size()-at)),all+at);
    (depth==0.0f?CyberpunkVR_HiddenAreaSkyResets:CyberpunkVR_HiddenAreaNearResets)[side].fetch_add(1,std::memory_order_relaxed);
    return true;
}
bool ToFar(uint32_t node) {
    const uint32_t only=CyberpunkVR_HiddenAreaSkyNode.load(std::memory_order_relaxed);
    return CyberpunkVR_HiddenAreaSky.load(std::memory_order_relaxed)>=1 && (!only || only==node);
}
bool ToNear(uint32_t node){return CyberpunkVR_HiddenAreaSky.load(std::memory_order_relaxed)>=2 && GBufferNode(node);}

// The eye's runtime FOV and the window the engine renders for it, in tangents: the symmetric frustum,
// or the eye's own off-centre window under the asymmetric projection (Runtimes/AsymmetricProjection.hpp).
bool Frustum(int side,Fov& fov,Fov& rendered) {
    // MAIN is side 0; which physical eye it is follows the stereo assignment.
    const int eye=(side==0)==(CyberpunkVR_MainIsRightEye!=0)?1:0;
    XrFovf xf{};
    if(!OpenXRManager::Get().GetCurrentEyeFov(eye,&xf))return false;
    fov={std::tan(xf.angleLeft),std::tan(xf.angleRight),std::tan(xf.angleUp),std::tan(xf.angleDown)};
    if(!(fov.left<0 && fov.right>0 && fov.up>0 && fov.down<0))return false;
    AsymEyeWindows asym{};
    if(cvr::projection::Active() && OpenXRManager::Get().GetAsymmetricEyeWindows(&asym)) {
        const auto& w=asym.eye[eye];
        rendered={w.l,w.r,w.u,w.d};
        // The inner guard renders the other eye's side on purpose: screen-space effects need it to agree
        // between the eyes. Keep the ellipse off it by the same amount, or the mask takes it away again.
        const float guard=std::fmin(1.0f,std::fmax(0.0f,std::isfinite(CyberpunkVR_AsymInnerGuard)?CyberpunkVR_AsymInnerGuard:0.0f));
        XrFovf other{};
        if(guard>0 && OpenXRManager::Get().GetCurrentEyeFov(1-eye,&other)) {
            const float ul=std::fmin(fov.left,std::tan(other.angleLeft)), ur=std::fmax(fov.right,std::tan(other.angleRight));
            fov.left+=guard*(ul-fov.left);fov.right+=guard*(ur-fov.right);
        }
        return true;
    }
    const float h0=GetGameRenderFovDeg(), v0=GetGameRenderVerticalFovDeg();
    if(!(h0>1 && h0<170 && v0>1 && v0<179))return false;
    const float tanX=std::tan(h0*.5f*3.14159265f/180), tanY=std::tan(v0*.5f*3.14159265f/180);
    rendered={-tanX,tanX,tanY,-tanY};
    return true;
}
// Patched shaders skip the pixels outside a slightly larger ellipse, so whatever they leave out
// lies inside the hidden area with this much of the radius to spare for filters reading neighbours.
constexpr float ShaderMargin=0.02f;
void PublishShaderEllipse(int side) {
    Fov fov,rendered;
    if(!Frustum(side,fov,rendered))return;
    const float r=CyberpunkVR_HiddenAreaRadius.load(std::memory_order_relaxed)/100.0f*(1.0f+ShaderMargin);
    float scales[4];
    if(rendered.left==-rendered.right && rendered.down==-rendered.up) {
        // NDC x and y (y down) to the quadrant-normalised tangents the visible ellipse is defined in.
        const float tanX=rendered.right, tanY=rendered.up;
        const float s[4]{tanX/-fov.left,tanX/fov.right,tanY/fov.up,tanY/-fov.down};
        for(int i=0;i<4;++i)scales[i]=(s[i]/r)*(s[i]/r);
    } else {
        // The patched shaders centre their ellipse on NDC 0, which is not straight ahead in an
        // off-centre window: they get the smallest centred ellipse that still holds the visible area.
        auto& c=ellipseCache[side];
        if(c.radius!=r || !SameFov(c.rendered,rendered) || !SameFov(c.fov,fov)) {
            c.radius=r;c.rendered=rendered;c.fov=fov;
            c.ok=ShaderEllipseScales(fov,r*100.0f,rendered,c.scales);
        }
        if(!c.ok)return;
        for(int i=0;i<4;++i)scales[i]=c.scales[i];
    }
    shaders::Publish(side,scales);
}

std::shared_ptr<const std::vector<Rect>> RectsFor(int side,UINT w,UINT h) {
    Fov fov,rendered;
    if(!Frustum(side,fov,rendered))return {};
    const float radius=CyberpunkVR_HiddenAreaRadius.load(std::memory_order_relaxed);
    std::lock_guard lock(mutex);
    auto& c=cache[side];
    if(!c.rects || c.width!=w || c.height!=h || c.radius!=radius || !SameFov(c.rendered,rendered) || !SameFov(c.fov,fov)) {
        c={w,h,radius,rendered,fov,std::make_shared<const std::vector<Rect>>(BuildRects(fov,radius,w,h,rendered,Cell,Margin))};
        lastHidden[side].store(float(100*Coverage(*c.rects,w,h)),std::memory_order_relaxed);
    }
    return c.rects;
}
}

void DsvCreated(ID3D12Resource* resource,const D3D12_DEPTH_STENCIL_VIEW_DESC* desc,D3D12_CPU_DESCRIPTOR_HANDLE handle) {
    if(stereo::probe::internalCommands || !handle.ptr)return;
    Target target;
    if(resource) {
        const auto d=resource->GetDesc();
        if(d.Dimension==D3D12_RESOURCE_DIMENSION_TEXTURE2D && d.SampleDesc.Count==1 &&
           (!desc || (desc->ViewDimension==D3D12_DSV_DIMENSION_TEXTURE2D && !desc->Texture2D.MipSlice)))
            target={UINT(desc?desc->Format:d.Format),UINT(d.Width),d.Height,resource};
    }
    std::lock_guard lock(dsvMutex);
    if(dsvs.size()>=65536 && !dsvs.contains(handle.ptr))dsvs.clear();
    if(target.width)dsvs[handle.ptr]=target;else dsvs.erase(handle.ptr);
}
void CopyDescriptors(ID3D12Device* device,UINT destinations,const D3D12_CPU_DESCRIPTOR_HANDLE* dest,const UINT* sizes,
                     UINT sources,const D3D12_CPU_DESCRIPTOR_HANDLE* src,const UINT* counts,D3D12_DESCRIPTOR_HEAP_TYPE type) {
    if(type!=D3D12_DESCRIPTOR_HEAP_TYPE_DSV || !device || !dest || !src || stereo::probe::internalCommands)return;
    const auto stride=device->GetDescriptorHandleIncrementSize(type);
    std::lock_guard lock(dsvMutex);
    std::vector<std::pair<bool,Target>> copy;
    for(UINT i=0;i<sources;++i)for(UINT n=0;n<(counts?counts[i]:1);++n) {
        if(copy.size()>=65536)return;
        const auto it=dsvs.find(src[i].ptr+size_t(n)*stride);
        copy.emplace_back(it!=dsvs.end(),it!=dsvs.end()?it->second:Target{});
    }
    size_t at=0;
    for(UINT i=0;i<destinations;++i)for(UINT n=0;n<(sizes?sizes[i]:1);++n,++at) {
        const auto handle=dest[i].ptr+size_t(n)*stride;
        if(at<copy.size() && copy[at].first)dsvs[handle]=copy[at].second;else dsvs.erase(handle);
    }
}

Settings GetSettings() {
    return Normalize({int(CyberpunkVR_HiddenAreaEnabled.load(std::memory_order_relaxed)),
                      CyberpunkVR_HiddenAreaRadius.load(std::memory_order_relaxed)});
}
void SetSettings(Settings s) {
    s=Normalize(s);
    CyberpunkVR_HiddenAreaRadius.store(s.radius,std::memory_order_relaxed);
    CyberpunkVR_HiddenAreaEnabled.store(uint32_t(s.enabled),std::memory_order_relaxed);
}
Status GetStatus() {
    Status s;
    for(int i=0;i<2;++i) {
        s.masked[i]=CyberpunkVR_HiddenAreaMasked[i].load(std::memory_order_relaxed);
        s.rects[i]=lastRects[i].load(std::memory_order_relaxed);
        s.hiddenPercent[i]=lastHidden[i].load(std::memory_order_relaxed);
        s.width[i]=lastWidth[i].load(std::memory_order_relaxed);s.height[i]=lastHeight[i].load(std::memory_order_relaxed);
    }
    return s;
}

void AfterDepthClear(ID3D12GraphicsCommandList* list,D3D12_CPU_DESCRIPTOR_HANDLE dsv,D3D12_CLEAR_FLAGS flags,FLOAT depth,
                     UINT rects,int side,uint32_t node,ClearFn clear) {
    if(stereo::probe::internalCommands || !list || !clear)return;
    const bool log=CyberpunkVR_HiddenAreaLog.load(std::memory_order_relaxed)!=0;
    const bool on=CyberpunkVR_HiddenAreaEnabled.load(std::memory_order_relaxed)!=0;
    if(!log && !on)return;
    Target target;
    if(!(flags&D3D12_CLEAR_FLAG_DEPTH))return;
    const bool known=LookupDsv(dsv.ptr,target);   // an unknown descriptor is logged as 0x0
    if(!known && !log)return;
    const UINT format=target.format, w=target.width, h=target.height;
    // The view's own scene depth: a full clear to the reverse-Z far plane, a 32-bit depth format and
    // the eye's aspect. Shadow maps are 16-bit or a different shape; menus show the eye image flat.
    const float aspect=g_launcherHeight>0?float(g_launcherWidth)/float(g_launcherHeight):1.0f;
    const uint32_t only=CyberpunkVR_HiddenAreaNode.load(std::memory_order_relaxed);
    bool masked=on && side>=0 && side<=1 && !rects && depth==0.0f && SceneDepthFormat(format) && w>=256 && h>=256 &&
        std::fabs(float(w)/float(h)-aspect)<=.02f*aspect && (!only || only==node) && !GetMenuMode();
    if(masked) {
        const auto cover=RectsFor(side,w,h);
        masked=cover && !cover->empty();
        if(masked) {
            static_assert(sizeof(Rect)==sizeof(D3D12_RECT));
            stereo::probe::Scope internal;
            // Batched: one clear with 177 or 205 rectangles terminated the game without a crash report,
            // while 127 ran for minutes (2026-10-09). 64 per call stays well under that.
            const auto* all=reinterpret_cast<const D3D12_RECT*>(cover->data());
            for(size_t at=0;at<cover->size();at+=MaxRectsPerClear)
                clear(list,dsv,D3D12_CLEAR_FLAG_DEPTH,1.0f,0,UINT(std::min(MaxRectsPerClear,cover->size()-at)),all+at);
            CyberpunkVR_HiddenAreaMasked[side].fetch_add(1,std::memory_order_relaxed);
            RememberScene(target.resource,side);
            PublishShaderEllipse(side);
            lastRects[side].store(uint32_t(cover->size()),std::memory_order_relaxed);
            lastWidth[side].store(w,std::memory_order_relaxed);lastHeight[side].store(h,std::memory_order_relaxed);
        }
    }
    if(log)Record(node,side,format,w,h,flags,depth,masked);
}

void BeforeBarriers(ID3D12GraphicsCommandList* list,UINT count,const D3D12_RESOURCE_BARRIER* barriers,int side,uint32_t node,ClearFn clear) {
    if(stereo::probe::internalCommands || !list || !barriers || !scenes[0].load(std::memory_order_relaxed))return;
    const bool log=CyberpunkVR_HiddenAreaLog.load(std::memory_order_relaxed)!=0;
    for(UINT i=0;i<count;++i) {
        const auto& b=barriers[i];
        if(b.Type!=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION)continue;
        const auto& t=b.Transition;
        const int view=SceneSide(t.pResource,side);
        if(view==-2)continue;
        // Not an END_ONLY half: between the halves of a split barrier the resource is unusable.
        const bool leaves=(t.StateBefore&D3D12_RESOURCE_STATE_DEPTH_WRITE) && !(t.StateAfter&D3D12_RESOURCE_STATE_DEPTH_WRITE) &&
            !(b.Flags&D3D12_RESOURCE_BARRIER_FLAG_END_ONLY) &&
            (t.Subresource==D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES || t.Subresource==0);
        const bool reset=leaves && ToFar(node) && ClearHidden(list,t.pResource,view,clear,0.0f);
        if(log)RecordBarrier(node,side,uint32_t(t.StateBefore),uint32_t(t.StateAfter),t.Subresource,uint32_t(b.Flags),reset);
    }
}
void BeforeEnhancedBarriers(ID3D12GraphicsCommandList* list,UINT count,const D3D12_BARRIER_GROUP* groups,int side,uint32_t node,ClearFn clear) {
    if(stereo::probe::internalCommands || !list || !groups || !scenes[0].load(std::memory_order_relaxed))return;
    const bool log=CyberpunkVR_HiddenAreaLog.load(std::memory_order_relaxed)!=0;
    for(UINT g=0;g<count;++g) {
        if(groups[g].Type!=D3D12_BARRIER_TYPE_TEXTURE || !groups[g].pTextureBarriers)continue;
        for(UINT i=0;i<groups[g].NumBarriers;++i) {
            const auto& b=groups[g].pTextureBarriers[i];
            const int view=SceneSide(b.pResource,side);
            if(view==-2)continue;
            const auto& s=b.Subresources;
            const bool depthPlane=s.IndexOrFirstMipLevel==0xffffffffu || (s.IndexOrFirstMipLevel==0 && (!s.NumMipLevels || !s.FirstPlane));
            // The barrier must also wait for depth writes, which then covers the clear recorded here.
            const bool leaves=b.LayoutBefore==D3D12_BARRIER_LAYOUT_DEPTH_STENCIL_WRITE &&
                b.LayoutAfter!=D3D12_BARRIER_LAYOUT_DEPTH_STENCIL_WRITE && depthPlane &&
                (b.SyncBefore&(D3D12_BARRIER_SYNC_ALL|D3D12_BARRIER_SYNC_DRAW|D3D12_BARRIER_SYNC_DEPTH_STENCIL));
            const bool reset=leaves && ToFar(node) && ClearHidden(list,b.pResource,view,clear,0.0f);
            if(log)RecordBarrier(node,side,0x80000000u|uint32_t(b.LayoutBefore),0x80000000u|uint32_t(b.LayoutAfter),
                                 s.IndexOrFirstMipLevel,uint32_t(b.Flags),reset);
        }
    }
}

void AfterBarriers(ID3D12GraphicsCommandList* list,UINT count,const D3D12_RESOURCE_BARRIER* barriers,int side,uint32_t node,ClearFn clear) {
    if(stereo::probe::internalCommands || !list || !barriers || !ToNear(node) || !scenes[0].load(std::memory_order_relaxed))return;
    for(UINT i=0;i<count;++i) {
        const auto& b=barriers[i];
        if(b.Type!=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION)continue;
        const auto& t=b.Transition;
        const int view=SceneSide(t.pResource,side);
        if(view!=-2 && !(t.StateBefore&D3D12_RESOURCE_STATE_DEPTH_WRITE) && (t.StateAfter&D3D12_RESOURCE_STATE_DEPTH_WRITE) &&
           !(b.Flags&D3D12_RESOURCE_BARRIER_FLAG_BEGIN_ONLY) &&
           (t.Subresource==D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES || t.Subresource==0))
            ClearHidden(list,t.pResource,view,clear,1.0f);
    }
}
void AfterEnhancedBarriers(ID3D12GraphicsCommandList* list,UINT count,const D3D12_BARRIER_GROUP* groups,int side,uint32_t node,ClearFn clear) {
    if(stereo::probe::internalCommands || !list || !groups || !ToNear(node) || !scenes[0].load(std::memory_order_relaxed))return;
    for(UINT g=0;g<count;++g) {
        if(groups[g].Type!=D3D12_BARRIER_TYPE_TEXTURE || !groups[g].pTextureBarriers)continue;
        for(UINT i=0;i<groups[g].NumBarriers;++i) {
            const auto& b=groups[g].pTextureBarriers[i];
            const int view=SceneSide(b.pResource,side);
            const auto& s=b.Subresources;
            if(view!=-2 && b.LayoutBefore!=D3D12_BARRIER_LAYOUT_DEPTH_STENCIL_WRITE && b.LayoutAfter==D3D12_BARRIER_LAYOUT_DEPTH_STENCIL_WRITE &&
               (s.IndexOrFirstMipLevel==0xffffffffu || (s.IndexOrFirstMipLevel==0 && (!s.NumMipLevels || !s.FirstPlane))))
                ClearHidden(list,b.pResource,view,clear,1.0f);
        }
    }
}
}
#endif
