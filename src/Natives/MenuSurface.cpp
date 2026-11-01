// Dedicated native ink surfaces. The engine still renders the menu's world geometry with
// each eye's scene depth. Only its UI allocation changes: no shared-atlas slot or coverage LOD.
#include "Natives/MenuSurface.hpp"
#include "Natives/NativeFunctions.hpp"
#include "Core/GameBuild.hpp"
#include "Hooks/Hook.hpp"
#include "MinHook.h"
#include <RED4ext/Scripting/Utils.hpp>
#include <RED4ext/Scripting/Natives/Generated/WorldWidgetComponent.hpp>
#include <RED4ext/Scripting/Natives/Generated/DynamicTexture.hpp>
#include <RED4ext/Scripting/Natives/inkWidget.hpp>
#include <RED4ext/Scripting/Natives/inkIWidgetComponentWrapper.hpp>
#include <wrl/client.h>
#include <atomic>
#include <algorithm>
#include <array>
#include <cstring>
#include <mutex>
#include <unordered_map>
#include <vector>

extern void Log(const char*,...);
// Allocations, native updates, readiness polls, armed polls, recorded writes,
// submitted producer fences, completed surfaces, currently retained surfaces.
extern "C" __declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_MenuSurfaceStats[8]{};
namespace cvr::menu_surface {
namespace {
using namespace RED4ext;
using Microsoft::WRL::ComPtr;
const uintptr_t base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
template<class T> T& At(void* p,size_t offset){return *reinterpret_cast<T*>(static_cast<uint8_t*>(p)+offset);}
struct Surface {
    WeakHandle<WorldWidgetComponent> component;
    Handle<DynamicTexture> texture;
    ComPtr<ID3D12Resource> resource;
    ComPtr<ID3D12Fence> fence;
    uint64_t submitted{};
    bool armed{},ready{};
};
std::mutex mutex;
std::unordered_map<WorldWidgetComponent*,std::shared_ptr<Surface>> surfaces;
std::unordered_map<ID3D12CommandList*,std::vector<std::shared_ptr<Surface>>> recorded;
std::atomic<bool> installed{false},observing{false},tracked{false};
using AddFn=char(*)(void*,void*);
using UpdateFn=uintptr_t(*)(void*,void*);
AddFn originalAdd{};
UpdateFn originalUpdate{};

// The native world entry has a weak wrapper at +0x190. The wrapper has a weak
// component at +8 (2.31, constructors 962914 / 9B4D1C). Never keep a naked entity.
Handle<WorldWidgetComponent> Component(void* entry) {
    if(!entry || At<uintptr_t>(entry,0)!=base+0x3111DC8)return {};
    auto wrapper=At<WeakPtr<ink::IWidgetComponentWrapper>>(entry,0x190).Lock();
    if(!wrapper)return {};
    auto component=At<WeakHandle<WorldWidgetComponent>>(wrapper.GetPtr(),8).Lock();
    // The VR menu's screens, and the VR lobby's terminal (mods/cet/CyberpunkVRPort_Lobby modules/terminal.lua).
    if(!component || (component->name!=CName("vrp_menu_ui") && component->name!=CName("vrp_lobby_ui")) ||
       !component->GetType()->IsA(CRTTISystem::Get()->GetClass("WorldWidgetComponent")))return {};
    return component;
}
std::array<uint32_t,2> Size(const WorldWidgetComponent* c) {
    if(!c)return {};
    if(c->widgetResource.path==ResourcePath("vrport\\ui\\vr_menu.inkwidget"))return {3200,1600};
    if(c->widgetResource.path==ResourcePath("vrport\\ui\\vr_menu_beam.inkwidget"))return {16,2048};
    // The lobby terminal's screen: tools/vr_lobby_assets/make_lobby_assets.py CANVAS.
    if(c->widgetResource.path==ResourcePath("vrport\\ui\\vr_lobby.inkwidget"))return {2580,1980};
    // The new game's window in the lobby: lobby_layout.HOLO_WINDOW_CANVAS.
    if(c->widgetResource.path==ResourcePath("vrport\\ui\\vr_lobby_window.inkwidget"))return {3190,2900};
    return {};
}
void FitWindow(void* entry,const WorldWidgetComponent* component) {
    const auto size=Size(component);
    auto* window=At<ink::Widget*>(entry,128);
    if(!window || !size[0])return;
    // VirtualWindow::FitTexture (961948) assumes a square atlas slot: it uniformly
    // shrinks a portrait canvas and reports an aspect crop at +528 (2.31: +518) to the mesh material
    // (1CBC88). Our texture already has the canvas's exact extent. Both transforms are
    // identities here; preserving the atlas crop would cut the menu in half.
    const Vector2 scale{1,1};
    if(window->renderTransform.scale.X!=1 || window->renderTransform.scale.Y!=1) {
        using SetScale=void(*)(ink::Widget*,const Vector2*);
        reinterpret_cast<SetScale>(base+0x54A748)(window,&scale);
    }
    auto& crop=At<Vector2>(window,0x528);
    if(crop.X!=1 || crop.Y!=1) {
        crop=scale;
        At<uint8_t>(const_cast<WorldWidgetComponent*>(component),0x2A4)|=1;
    }
}
// THE DISPLAY'S RENDER PROXY CAN BE REPLACED UNDER THE COMPONENT. Switching the display mesh off and on (or the engine
// streaming it out and back) builds a new proxy, and a new one starts from the material's defaults: an opaque black
// plane. The component (update 3C5944, 2.5) sends its texture only while dirty (+0x2A4 bit 0), and in full only when it
// differs from the one it applied last (+0x250); a new proxy marks nothing dirty, the weak handle to the old one
// (+0x220, its count block +0x228) just expires. So keep the invariant ourselves: with no live proxy, nothing of ours
// is applied; while what is applied is not our texture (+0x230) and the component shows its texture (+0x8B), it is
// dirty. The next update then finds the proxy and binds our texture to it.
void Rebind(WorldWidgetComponent* component,DynamicTexture* texture) {
    auto* c=reinterpret_cast<uint8_t*>(component);
    if(!texture || At<DynamicTexture*>(c,0x230)!=texture)return;
    const auto* alive=At<volatile int32_t*>(c,0x228);
    if(!alive || !*alive)At<void*>(c,0x250)=nullptr;
    if(At<void*>(c,0x250)!=texture && At<uint8_t>(c,0x8B))At<uint8_t>(c,0x2A4)|=1;
}
void Prune() {
    for(auto it=surfaces.begin();it!=surfaces.end();) {
        const auto& s=it->second;
        const bool pending=std::any_of(recorded.begin(),recorded.end(),[&](const auto& list){
            return std::find(list.second.begin(),list.second.end(),s)!=list.second.end();
        });
        if(!pending && !s->component.Lock() && (!s->fence || !s->submitted ||
            s->fence->GetCompletedValue()>=s->submitted))it=surfaces.erase(it);
        else ++it;
    }
    CyberpunkVR_MenuSurfaceStats[7]=surfaces.size();
}
std::shared_ptr<Surface> Get(const Handle<WorldWidgetComponent>& component) {
    auto it=surfaces.find(component.GetPtr());
    if(it!=surfaces.end() && it->second->component.Lock()==component)return it->second;
    Prune();
    auto s=std::make_shared<Surface>();s->component=component;
    surfaces[component.GetPtr()]=s;tracked=true;CyberpunkVR_MenuSurfaceStats[7]=surfaces.size();return s;
}
// Marks a widget and its ancestors changed (flag 0x10 at +0x1FD, 2.5 sub 2805FC): ink rebuilds the window's draw
// data and paints it again.
void Invalidate(ink::Widget* widget) {
    using Fn=void(*)(ink::Widget*);
    if(widget)reinterpret_cast<Fn>(base+0x2805FC)(widget);
}
bool Create(Surface& s,const std::array<uint32_t,2>& size) {
    if(s.texture)return true;
    // Native DynamicTexture descriptor, verified against the HUD's own allocation at 28AA24.
    // Its constructor (821538) copies dimensions, format, samples and renderer flags.
    struct Descriptor {uint32_t width,height;uint8_t mips,samples,format,flags;uint32_t pad;void* backing;};
    static_assert(sizeof(Descriptor)==24);
    Descriptor d{size[0],size[1],0,1,6,0x14,0,nullptr};
    using CreateFn=Handle<DynamicTexture>*(*)(Handle<DynamicTexture>*,const Descriptor*);
    reinterpret_cast<CreateFn>(base+0x5EA8A4)(&s.texture,&d);
    if(!s.texture)return false;
    ++CyberpunkVR_MenuSurfaceStats[0];
    Log("[menu-surface] native target %ux%u component=%p texture=%p\n",size[0],size[1],
        s.component.Lock().GetPtr(),s.texture.GetPtr());
    return true;
}
char Add(void* manager,void* owner) {
    auto* entry=owner ? At<void*>(owner,0) : nullptr;
    auto component=Component(entry);const auto size=Size(component.GetPtr());
    if(!size[0])return originalAdd(manager,owner);
    std::shared_ptr<Surface> s;
    {
        std::lock_guard lock(mutex);s=Get(component);
        if(!Create(*s,size))return 0; // no low-resolution or uninitialized atlas fallback
    }
    // A native atlas entry already knows how to bind its target to its ink window and
    // mesh material. Give it a whole owned texture and full UVs instead of a shared slot.
    const float rect[4]={0,0,1,1};
    std::memcpy(static_cast<uint8_t*>(entry)+24,rect,sizeof(rect));
    std::memcpy(static_cast<uint8_t*>(entry)+40,rect,sizeof(rect));
    using Assign=uintptr_t(*)(void*,DynamicTexture*);
    reinterpret_cast<Assign>(At<uintptr_t*>(entry,0)[2])(entry,s->texture.GetPtr());
    FitWindow(entry,component.GetPtr());
    // Binding the texture must not wait for a disabled display to become visible.
    // This is the same wrapper API used by native entry::UpdateBinding (2A0F90).
    auto wrapper=At<WeakPtr<ink::IWidgetComponentWrapper>>(entry,0x190).Lock();
    if(wrapper) {
        auto* table=At<uintptr_t*>(wrapper.GetPtr(),0);
        using UVs=void(*)(void*,const float*);
        reinterpret_cast<UVs>(table[0x60/8])(wrapper.GetPtr(),rect);
        reinterpret_cast<Assign>(table[0x58/8])(wrapper.GetPtr(),s->texture.GetPtr());
    }
    return 1;
}
uintptr_t Update(void* entry,void* context) {
    auto component=Component(entry);
    const bool own=Size(component.GetPtr())[0]!=0;
    if(own) {
        // Streaming normally enables the wrapper only when its mesh becomes visible.
        // Keep the native window/binding update active while our display is preparing.
        auto wrapper=At<WeakPtr<ink::IWidgetComponentWrapper>>(entry,0x190).Lock();
        if(wrapper)At<uint8_t>(wrapper.GetPtr(),108)=1;
    }
    const auto result=originalUpdate(entry,context);
    if(own && At<void*>(entry,128)) {
        auto wrapper=At<WeakPtr<ink::IWidgetComponentWrapper>>(entry,0x190).Lock();
        if(wrapper)At<uint8_t>(wrapper.GetPtr(),108)=1;
        FitWindow(entry,component.GetPtr());
        Rebind(component.GetPtr(),SurfaceTexture(component.GetPtr()));
        ++CyberpunkVR_MenuSurfaceStats[1];
        // ink must paint while the display mesh is disabled during preparation. The
        // menu's tiny fixed set of targets remains live when looking away, too.
        At<float>(entry,56)=100.f;
        InterlockedExchange(reinterpret_cast<volatile LONG*>(static_cast<uint8_t*>(entry)+152),1);
        // A window none of whose widgets changed is painted from ink's cached draw data, and in that state its
        // atlas images (not its rects or text) drew at about half strength for a few frames every ~7 s: the lobby's
        // logo and pointer dot, measured on the mirror; the same window changed in every frame never did. These
        // screens are live and painted every frame, so their draw data is rebuilt every frame as well: the window
        // is marked changed, as any widget change marks it.
        Invalidate(At<ink::Widget*>(entry,128));
    }
    return result;
}
bool Install() {
    if(!cvr::game_build::Supported())return false;
    struct Hook {uintptr_t rva;const char* bytes;size_t size;void* detour;void** original;};
    const Hook hooks[]={
        {0x6FE754,"\x48\x89\x5c\x24\x10\x48\x89\x74\x24\x18\x48\x89\x7c\x24\x20\x55",16,reinterpret_cast<void*>(&Add),reinterpret_cast<void**>(&originalAdd)},
        {0x6067C0,"\x48\x8b\xc4\x48\x89\x58\x08\x48\x89\x70\x10\x48\x89\x78\x18\x4c",16,reinterpret_cast<void*>(&Update),reinterpret_cast<void**>(&originalUpdate)},
    };
    if(std::memcmp(reinterpret_cast<void*>(base+0x5EA8A4),"\x48\x89\x5c\x24\x08\x48\x89\x74\x24\x10\x57\x48\x83\xec\x30\x48",16))return false;
    if(std::memcmp(reinterpret_cast<void*>(base+0x54A748),"\x40\x53\x48\x83\xec\x20\xf3\x0f\x10\x0a\x48\x8b\xd9\x0f\x2e\x89",16))return false;
    if(std::memcmp(reinterpret_cast<void*>(base+0x2805FC),"\x48\x89\x5c\x24\x18\x55\x48\x8b\xec\x48\x83\xec\x60\x80\x89\xfd\x01\x00\x00\x10",20))return false;
    for(const auto& h:hooks)if(std::memcmp(reinterpret_cast<void*>(base+h.rva),h.bytes,h.size))return false;
    size_t created=0;
    for(const auto& h:hooks) {
        if(MH_CreateHook(reinterpret_cast<void*>(base+h.rva),h.detour,h.original)!=MH_OK)break;
        ++created;
    }
    if(created!=std::size(hooks)) {
        for(size_t i=0;i<created;++i)MH_RemoveHook(reinterpret_cast<void*>(base+hooks[i].rva));
        return false;
    }
    for(const auto& h:hooks)if(MH_EnableHook(reinterpret_cast<void*>(base+h.rva))!=MH_OK) {
        for(const auto& undo:hooks){MH_DisableHook(reinterpret_cast<void*>(base+undo.rva));MH_RemoveHook(reinterpret_cast<void*>(base+undo.rva));}
        return false;
    }
    installed=true;return true;
}
CVR_HOOK("MenuSurface",::cvr::hooks::Stage::Boot,83,Install);

bool FindResource(Surface& s) {
    if(s.resource)return true;
    // DynamicTexture's native backing holds a pool handle, not a D3D resource pointer.
    if(!s.texture)return false;
    const auto backing=At<uintptr_t>(s.texture.GetPtr(),0x70);
    if(!backing || *reinterpret_cast<uintptr_t*>(backing)!=base+0x30646A8)return false;
    const uint32_t id=*reinterpret_cast<uint32_t*>(backing+0x18);
    const auto pool=*reinterpret_cast<uintptr_t*>(base+0x3BCDCA8);
    if(!pool || !id || id>131072)return false;
    auto* resource=*reinterpret_cast<ID3D12Resource**>(pool+0x2F1D8+0xB0ull*(id-1));
    if(!resource)return false;
    auto d=resource->GetDesc();
    if(d.Dimension!=D3D12_RESOURCE_DIMENSION_TEXTURE2D || d.Width!=s.texture->width || d.Height!=s.texture->height)return false;
    s.resource=resource;return true;
}
}
bool Ready(const RED4ext::Handle<RED4ext::WorldWidgetComponent>& component,
           const RED4ext::Handle<RED4ext::ink::Widget>& root) {
    if(!installed || !component || !root ||
       !component->GetType()->IsA(CRTTISystem::Get()->GetClass("WorldWidgetComponent")) ||
       !Size(component.GetPtr())[0])return false;
    std::lock_guard lock(mutex);
    auto it=surfaces.find(component.GetPtr());
    if(it==surfaces.end())return false;
    auto& s=*it->second;
    if(s.component.Lock()!=component)return false;
    if(!FindResource(s))return false;
    ++CyberpunkVR_MenuSurfaceStats[2];
    const auto completed=s.fence?s.fence->GetCompletedValue():0;
    if(s.submitted && completed!=UINT64_MAX && completed>=s.submitted) {
        if(!s.ready)++CyberpunkVR_MenuSurfaceStats[6];
        s.ready=true;return true;
    }
    if(s.submitted)return false;
    s.armed=true;observing=true;
    ++CyberpunkVR_MenuSurfaceStats[3];
    // Called after Lua has constructed the canvas. Request a real ink repaint; readiness
    // below requires that target's write to finish on its actual GPU queue.
    Invalidate(root.GetPtr());
    return false;
}
void Barriers(ID3D12GraphicsCommandList* list,UINT count,const D3D12_RESOURCE_BARRIER* barriers) {
    if(!observing.load(std::memory_order_relaxed) || !barriers)return;
    constexpr auto writes=D3D12_RESOURCE_STATE_RENDER_TARGET|D3D12_RESOURCE_STATE_COPY_DEST|D3D12_RESOURCE_STATE_UNORDERED_ACCESS;
    for(UINT i=0;i<count;++i) {
        const auto& b=barriers[i];
        if(b.Type!=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION)continue;
        std::lock_guard lock(mutex);
        for(auto& [component,s]:surfaces)if(s->armed && !s->ready && !s->submitted && s->resource.Get()==b.Transition.pResource) {
            // Native ink targets enter RT from COMMON and decay after execution: there
            // need not be an outgoing RT->SRV barrier. Fence the entire producer list,
            // including its draws/clear, rather than waiting for that absent transition.
            if(b.Flags==D3D12_RESOURCE_BARRIER_FLAG_BEGIN_ONLY ||
               !((b.Transition.StateBefore|b.Transition.StateAfter)&writes))continue;
            ++CyberpunkVR_MenuSurfaceStats[4];
            auto& pending=recorded[list];
            if(std::find(pending.begin(),pending.end(),s)==pending.end())pending.push_back(s);
        }
    }
}
void ResetList(ID3D12GraphicsCommandList* list) {
    if(!tracked.load(std::memory_order_relaxed))return;
    std::lock_guard lock(mutex);recorded.erase(list);
}
void Written(ID3D12GraphicsCommandList* list,ID3D12Resource* resource) {
    if(!observing.load(std::memory_order_relaxed) || !resource)return;
    std::lock_guard lock(mutex);
    for(auto& [component,s]:surfaces)if(s->armed && !s->ready && !s->submitted && s->resource.Get()==resource) {
        ++CyberpunkVR_MenuSurfaceStats[4];
        auto& pending=recorded[list];
        if(std::find(pending.begin(),pending.end(),s)==pending.end())pending.push_back(s);
    }
}
bool Waiting(){return observing.load(std::memory_order_relaxed);}
RED4ext::DynamicTexture* SurfaceTexture(RED4ext::WorldWidgetComponent* component) {
    if(!installed || !component)return nullptr;
    std::lock_guard lock(mutex);
    const auto it=surfaces.find(component);
    return it!=surfaces.end() && it->second->component.Lock().GetPtr()==component ? it->second->texture.GetPtr() : nullptr;
}
void Submitted(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists) {
    if(!tracked.load(std::memory_order_relaxed) || !queue || !lists)return;
    std::lock_guard lock(mutex);
    for(UINT i=0;i<count;++i) {
        auto it=recorded.find(lists[i]);if(it==recorded.end())continue;
        for(auto& s:it->second)if(!s->submitted) {
            ComPtr<ID3D12Device> device;
            if(SUCCEEDED(queue->GetDevice(IID_PPV_ARGS(&device))) &&
               SUCCEEDED(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&s->fence))) &&
               SUCCEEDED(queue->Signal(s->fence.Get(),1))){s->submitted=1;++CyberpunkVR_MenuSurfaceStats[5];}
        }
        recorded.erase(it);
    }
    static uint64_t nextPrune{};
    const auto now=GetTickCount64();
    if(now>=nextPrune){Prune();nextPrune=now+250;}
    observing=std::any_of(surfaces.begin(),surfaces.end(),[](const auto& entry){
        const auto& s=*entry.second;return s.armed && !s.submitted && !s.ready;
    });
    tracked=!surfaces.empty() || !recorded.empty();
}
}

void VRMenuSurfaceReady(RED4ext::IScriptable*,RED4ext::CStackFrame* stack,bool* out,int64_t) {
    RED4ext::Handle<RED4ext::WorldWidgetComponent> component;
    RED4ext::Handle<RED4ext::ink::Widget> root;
    RED4ext::GetParameter(stack,&component);RED4ext::GetParameter(stack,&root);++stack->code;
    const bool ready=cvr::menu_surface::Ready(component,root);
    if(out)*out=ready;
}
