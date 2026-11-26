#if !CVR_RUST_VRIK   // the Rust build registers this file's hook from rust/crates/cvr-render/src/hook_table.rs
// Native world-UI allocation, verified against CP2077 a7de82945c03e041...
// The shared world atlas has only 1/2/4 slots at 2048/1024/512. A physical
// scope must not compete with city screens for its camera's pixel resolution.
#include "Hooks/Hook.hpp"
#if CVR_RUST_VRIK
// rust/crates/cvr-render/src/optics_surface.rs: the same three detours, the surfaces and the exports.
extern "C" bool cvr_hook_physical_optics_surface_install();
CVR_HOOK("PhysicalOpticsSurface",::cvr::hooks::Stage::Boot,83,cvr_hook_physical_optics_surface_install);
#else
#include <Windows.h>
#include <MinHook.h>
#include <array>
#include <atomic>
#include <bit>
#include <cmath>
#include <cstring>
#include <memory>
#include <mutex>
#include <unordered_map>
#include <RED4ext/Hashing/FNV1a.hpp>
#include <RED4ext/Scripting/Natives/entIComponent.hpp>

extern void Log(const char*,...);
extern "C" {
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_OpticsOwnSurface{1};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_OpticsSurfaceCreated{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_OpticsSurfaceReleased{0};
// Publish calls that passed each check in OpticsSize: 0 any renderer with a texture, 1 renderer type,
// 2 provider type, 3 live component, 4 port panel name, 5 canvas size; 6 port panel textures set.
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_OpticsSurfaceStage[8]{};
// The last port panel's canvas size as read (rejected sizes show why a panel went to the shared atlas).
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_OpticsSurfaceCanvas[2]{};
}

namespace {
const uintptr_t base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
template<class T> T& At(uintptr_t p,size_t offset) {return *reinterpret_cast<T*>(p+offset);}
struct NativeHandle {uintptr_t object{},control{};};
template<uintptr_t Release> struct Lease {
    NativeHandle handle;
    Lease()=default;
    Lease(const Lease&)=delete;
    ~Lease(){if(handle.control)reinterpret_cast<void(*)(NativeHandle*)>(base+Release)(&handle);}
};
struct Surface {
    Lease<0x198368> texture;
    Lease<0x409774> owner;
    uint32_t width{},height{};
    ~Surface(){if(texture.handle.object)CyberpunkVR_OpticsSurfaceReleased.fetch_add(1,std::memory_order_relaxed);}
};
struct Binding {
    Lease<0x409774> owner; // weak component: never prolongs its game lifetime
    std::shared_ptr<Surface> surface;
};
// Native engine ownership must end with the renderer, before its pools are torn
// down. Do not run native release functions from CRT static destruction.
struct Surfaces {
    std::mutex mutex;
    std::unordered_map<uintptr_t,std::shared_ptr<Surface>> entries;
    std::unordered_map<uintptr_t,std::shared_ptr<Binding>> bindings;
    std::unordered_map<uintptr_t,std::weak_ptr<Surface>> groups;
};
Surfaces& Owned(){static auto* value=new Surfaces;return *value;}
using PublishFn=uintptr_t(*)(uintptr_t);
using DestroyFn=uintptr_t(*)(uintptr_t,uint8_t);
using SetTextureFn=uintptr_t(*)(uintptr_t,uintptr_t);
PublishFn originalPublish{};
DestroyFn originalDestroy{};
SetTextureFn originalSetTexture{};
constexpr std::array<float,4> fullUV{0,0,1,1};
// One native ink atlas per weapon. The image shader composites these regions
// in a defined order; transparent mesh submission order cannot hide the reticle.
constexpr std::array<std::array<float,4>,3> panelUV{{
    {0,0,.5f,.5f}, {.5f,0,1,.5f}, {.5f,.5f,1,1}
}};
static_assert(offsetof(RED4ext::ent::IComponent,owner)==0x50);
constexpr int OpticsPanelSlot(uint64_t name) {
    switch(name) {
    case RED4ext::FNV1a64("vr_optic_panel"):
    case RED4ext::FNV1a64("vr_builtin_guillotine_panel"):
    case RED4ext::FNV1a64("vr_builtin_hercules_panel"):
        return 0; // Full-resolution camera feed, including the two integral lenses.
    case RED4ext::FNV1a64("vr_optic_reticle_front_panel"): return 1;
    case RED4ext::FNV1a64("vr_optic_reticle_rear_panel"): return 2;
    default: return -1;
    }
}

uintptr_t SetTexture(uintptr_t component,uintptr_t texture) {
    if(OpticsPanelSlot(At<uint64_t>(component,0x40))<0)return originalSetTexture(component,texture);
    CyberpunkVR_OpticsSurfaceStage[6].fetch_add(1,std::memory_order_relaxed);
    std::shared_ptr<Binding> previous;
    {
        auto& owned=Owned();std::lock_guard lock(owned.mutex);
        // A detached component can expire before its queued UI removal calls
        // the setter. Its weak reference makes these leftovers safe to reap.
        for(auto it=owned.bindings.begin();it!=owned.bindings.end();) {
            const auto control=it->second->owner.handle.control;
            if(!control || InterlockedCompareExchange(reinterpret_cast<volatile LONG*>(control),0,0)==0)
                it=owned.bindings.erase(it);
            else ++it;
        }
        auto found=owned.bindings.find(component);
        if(found!=owned.bindings.end())previous=found->second;
        std::shared_ptr<Surface> next;
        if(previous && previous->surface->texture.handle.object==texture)next=previous->surface;
        if(!next)for(const auto& [renderer,surface]:owned.entries)
            if(surface && surface->texture.handle.object==texture){next=surface;break;}
        if(next) {
            if(!previous || previous->surface!=next) {
                auto binding=std::make_shared<Binding>();
                binding->owner.handle=At<NativeHandle>(component,8);
                if(binding->owner.handle.control)
                    InterlockedIncrement(reinterpret_cast<volatile LONG*>(binding->owner.handle.control+4));
                binding->surface=std::move(next);owned.bindings[component]=std::move(binding);
            }
        } else if(found!=owned.bindings.end())owned.bindings.erase(found);
    }
    // WorldLayer unregister destroys the renderer BEFORE clearing the component
    // texture. Keep the old surface alive until the native setter and material
    // invalidation have completed; renderer ownership alone is insufficient.
    return originalSetTexture(component,texture);
}

bool OpticsSize(uintptr_t renderer,uint32_t& width,uint32_t& height,uintptr_t& owner,uint32_t& slot) {
    auto& stage=CyberpunkVR_OpticsSurfaceStage;
    stage[0].fetch_add(1,std::memory_order_relaxed);
    if(At<uintptr_t>(renderer,0)!=base+0x3111dc8)return false;
    stage[1].fetch_add(1,std::memory_order_relaxed);
    Lease<0x1c075c> provider;
    reinterpret_cast<void(*)(NativeHandle*,const void*)>(base+0x27c10c)(
        &provider.handle,reinterpret_cast<const void*>(renderer+400));
    if(!provider.handle.object || At<uintptr_t>(provider.handle.object,0)!=base+0x30636e8)return false;
    stage[2].fetch_add(1,std::memory_order_relaxed);
    Lease<0x198368> component;
    reinterpret_cast<void(*)(const void*,NativeHandle*)>(base+0x189688)(
        reinterpret_cast<const void*>(provider.handle.object+8),&component.handle);
    if(!component.handle.object)return false;
    stage[3].fetch_add(1,std::memory_order_relaxed);
    const int panelSlot=OpticsPanelSlot(At<uint64_t>(component.handle.object,0x40));
    if(panelSlot<0)return false;
    stage[4].fetch_add(1,std::memory_order_relaxed);
    owner=At<uintptr_t>(component.handle.object,0x50);
    if(!owner)return false;
    slot=static_cast<uint32_t>(panelSlot);
    const float w=At<float>(component.handle.object,0x2b0),h=At<float>(component.handle.object,0x2b4);
    CyberpunkVR_OpticsSurfaceCanvas[0].store(std::bit_cast<uint32_t>(w),std::memory_order_relaxed);
    CyberpunkVR_OpticsSurfaceCanvas[1].store(std::bit_cast<uint32_t>(h),std::memory_order_relaxed);
    if(!std::isfinite(w) || !std::isfinite(h) || w<256 || h<256 || w>2048 || h>2048)return false;
    stage[5].fetch_add(1,std::memory_order_relaxed);
    width=static_cast<uint32_t>(std::ceil(w));height=static_cast<uint32_t>(std::ceil(h));
    if(slot!=0) {
        // Native reticle artwork uses a 2048-square logical canvas. A 1024
        // surface preserves thin lines without allocating two extra 16MB RTs.
        width=std::min(width,1024u);height=std::min(height,1024u);
    }
    return true;
}

std::shared_ptr<Surface> GetSurface(uintptr_t renderer,uintptr_t owner) {
    auto& owned=Owned();std::lock_guard lock(owned.mutex);
    auto& entry=owned.entries[renderer];
    if(entry)return entry;
    for(auto it=owned.groups.begin();it!=owned.groups.end();)
        if(it->second.expired())it=owned.groups.erase(it);else ++it;
    const auto identity=At<NativeHandle>(owner,8);
    if(auto group=owned.groups[owner].lock();group && group->owner.handle.control==identity.control) {
        entry=group;return entry;
    }
    auto next=std::make_shared<Surface>();
    next->owner.handle=identity;
    if(identity.control)InterlockedIncrement(reinterpret_cast<volatile LONG*>(identity.control+4));
    // Same descriptor and factory as WorldDTManager's standalone (full-size)
    // texture path, RVA82099C. The game creates its GPU backing and transitions.
    struct Desc {uint32_t width,height,flags,reserved;uintptr_t backing;};
    static_assert(sizeof(Desc)==24);
    constexpr uint32_t width=2048,height=2048;
    const Desc desc{width,height,0x13070100,0,0}; // RGBA_Float16 retains scene HDR through ink.
    reinterpret_cast<NativeHandle*(*)(NativeHandle*,const Desc*)>(base+0x5ea8a4)(
        &next->texture.handle,&desc);
    if(!next->texture.handle.object || !next->texture.handle.control)return {};
    next->width=width;next->height=height;
    entry=next;
    owned.groups[owner]=next;
    CyberpunkVR_OpticsSurfaceCreated.fetch_add(1,std::memory_order_relaxed);
    Log("[optics-surface] native UI texture %ux%u for renderer=%p\n",width,height,reinterpret_cast<void*>(renderer));
    return next;
}

void BindWindow(uintptr_t window,uintptr_t texture,const std::array<float,4>& uv) {
    if(!window)return;
    // 2.5: the virtual window's fields from +4D8 on moved by 0x10 (texture 4D8->4E8, rect 4F0->500)
    const bool textureChanged=At<uintptr_t>(window,0x4E8)!=texture;
    const bool rectChanged=std::memcmp(reinterpret_cast<const void*>(window+0x500),uv.data(),sizeof(uv))!=0;
    if(rectChanged)reinterpret_cast<void(*)(uintptr_t,const float*)>(base+0x30fc34)(window,uv.data());
    if(textureChanged || rectChanged)
        reinterpret_cast<void(*)(uintptr_t,uintptr_t,uint8_t)>(base+0x30fd04)(window,texture,1);
}

uintptr_t Publish(uintptr_t renderer) {
    uint32_t width{},height{},slot{};uintptr_t owner{};
    if(!At<uintptr_t>(renderer,16) || !OpticsSize(renderer,width,height,owner,slot))return originalPublish(renderer);
    const auto pooledTexture=At<uintptr_t>(renderer,16);
    const auto pooledUV=At<std::array<float,4>>(renderer,24);
    const auto surface=CyberpunkVR_OpticsOwnSurface.load(std::memory_order_relaxed)
        ? GetSurface(renderer,owner) : std::shared_ptr<Surface>{};
    const auto texture=surface?surface->texture.handle.object:pooledTexture;
    const auto& uv=surface?panelUV[slot]:pooledUV;
    BindWindow(At<uintptr_t>(renderer,128),texture,uv);
    // Publish through the original component/material binder. Only this call
    // sees the dedicated surface; the allocator retains its own slot bookkeeping.
    At<uintptr_t>(renderer,16)=texture;
    At<std::array<float,4>>(renderer,24)=uv;
    const auto result=originalPublish(renderer);
    At<uintptr_t>(renderer,16)=pooledTexture;
    At<std::array<float,4>>(renderer,24)=pooledUV;
    return result;
}

uintptr_t Destroy(uintptr_t renderer,uint8_t flags) {
    std::shared_ptr<Surface> surface;
    {
        auto& owned=Owned();std::lock_guard lock(owned.mutex);
        auto found=owned.entries.find(renderer);
        if(found!=owned.entries.end()){surface=std::move(found->second);owned.entries.erase(found);}
    }
    const auto result=originalDestroy(renderer,flags);
    return result;
}

bool Install() {
    struct Signature {uintptr_t rva;std::initializer_list<uint8_t> bytes;};
    const Signature checks[]={
        {0x606660,{0x48,0x89,0x5c,0x24,0x08,0x48,0x89,0x74,0x24,0x10,0x48,0x89,0x7c,0x24,0x18,0x55,0x48,0x8b,0xec}},
        {0xdd98a4,{0x48,0x89,0x5c,0x24,0x08,0x57,0x48,0x83,0xec,0x20,0x48,0x8b,0xf9,0x8b,0xda}},
        {0x6069f0,{0x40,0x53,0x48,0x83,0xec,0x40,0x48,0x8b,0x81,0x30,0x02,0x00,0x00}},
        {0x5ea8a4,{0x48,0x89,0x5c,0x24,0x08,0x48,0x89,0x74,0x24,0x10,0x57,0x48,0x83,0xec,0x30}},
        {0x30fc34,{0x48,0x83,0xec,0x28,0xf3,0x0f,0x10,0x42,0x0c,0x0f,0x2f,0x42,0x04}},
        {0x30fd04,{0x48,0x89,0x5c,0x24,0x08,0x57,0x48,0x83,0xec,0x20,0x48,0x89,0x91,0xe8,0x04,0x00,0x00}},
        {0x27c10c,{0x48,0x8b,0x42,0x08,0x4c,0x8b,0xc2,0x4c,0x8b,0xc9}},
        {0x189688,{0x48,0x83,0x22,0x00,0x4c,0x8b,0xc2,0x48,0x83,0x62,0x08,0x00}},
        {0x1c075c,{0x48,0x89,0x5c,0x24,0x08,0x57,0x48,0x83,0xec,0x30,0x48,0x8b,0xf9}},
        {0x198368,{0x40,0x53,0x48,0x83,0xec,0x20,0x48,0x8b,0xd9,0x48,0x8b,0x49,0x08}},
        {0x409774,{0x40,0x53,0x48,0x83,0xec,0x30,0x48,0x8b,0xd9,0x48,0x8b,0x49,0x08}},
    };
    for(const auto& check:checks)
        if(std::memcmp(reinterpret_cast<const void*>(base+check.rva),check.bytes.begin(),check.bytes.size()))return false;
    void* publish=reinterpret_cast<void*>(base+0x606660);
    void* destroy=reinterpret_cast<void*>(base+0xdd98a4);
    void* setTexture=reinterpret_cast<void*>(base+0x6069f0);
    if(MH_CreateHook(publish,reinterpret_cast<void*>(&Publish),reinterpret_cast<void**>(&originalPublish))!=MH_OK)return false;
    if(MH_CreateHook(destroy,reinterpret_cast<void*>(&Destroy),reinterpret_cast<void**>(&originalDestroy))!=MH_OK){MH_RemoveHook(publish);return false;}
    if(MH_CreateHook(setTexture,reinterpret_cast<void*>(&SetTexture),reinterpret_cast<void**>(&originalSetTexture))!=MH_OK){MH_RemoveHook(publish);MH_RemoveHook(destroy);return false;}
    if(MH_EnableHook(publish)!=MH_OK || MH_EnableHook(destroy)!=MH_OK || MH_EnableHook(setTexture)!=MH_OK){MH_DisableHook(publish);MH_DisableHook(destroy);MH_DisableHook(setTexture);return false;}
    return true;
}
CVR_HOOK("PhysicalOpticsSurface",::cvr::hooks::Stage::Boot,83,Install);
}
#endif
#endif
