#if !CVR_RUST_VRIK   // the Rust build registers this file's hook from rust/crates/cvr-render/src/hook_table.rs
// Route scene-driven vendor events before the engine creates a menu instance.
// CET's RequestVendorMenu override sees script calls only. Native quest requests
// reach this UI event queue directly, before any MenuScenario callback runs.
#include "Natives/VendorRoute.hpp"
#include "Natives/NativeFunctions.hpp"
#include "Core/GameBuild.hpp"
#include "Hooks/Hook.hpp"
#include "MinHook.h"
#include <RED4ext/Scripting/Utils.hpp>
#include <windows.h>
#include <atomic>
#include <cstring>
#include <mutex>

#if CVR_RUST_VRIK
// The queue hook, the held request, its watchdog and the two natives are rust/crates/cvr-rig/src/vrik/vendor_route.rs;
// the registration stays.
extern "C" bool cvr_hook_vendor_route_install();
CVR_HOOK("VendorRoute",::cvr::hooks::Stage::Boot,84,cvr_hook_vendor_route_install);
#else
extern void Log(const char*,...);
namespace cvr::vendor_route {
namespace {
using namespace RED4ext;
using QueueFn=uintptr_t(*)(void*,const Handle<IScriptable>&);
QueueFn originalQueue{};
std::atomic<ULONGLONG> heartbeat{};
std::atomic<bool> installed{},hasPending{},watchdogRegistered{};
std::mutex mutex;
WeakHandle<IScriptable> uiSystem;
struct Request {
    WeakHandle<IScriptable> owner;
    Handle<IScriptable> event,panel;
    ULONGLONG captured{};
    void* queue{};
};
Request pending;
void* InternalQueue(const Handle<IScriptable>& system) {
    return system?*reinterpret_cast<void* const*>(reinterpret_cast<const uint8_t*>(system.GetPtr())+0x48):nullptr;
}

Handle<IScriptable> Panel(const Handle<IScriptable>& event) {
    if(!event || event->GetType()->GetName()!=CName("inkMenuInstance_SwitchToScenario"))return {};
    // 2.5 constructor 27B2D1C: CName at +40, retained userdata handle at +48.
    const auto* bytes=reinterpret_cast<const uint8_t*>(event.GetPtr());
    if(*reinterpret_cast<const CName*>(bytes+0x40)!=CName("MenuScenario_Vendor"))return {};
    const auto& panel=*reinterpret_cast<const Handle<IScriptable>*>(bytes+0x48);
    if(!panel || panel->GetType()->GetName()!=CName("questVendorPanelData"))return {};
    return panel;
}
void Forward(Request&& request) {
    auto owner=request.owner.Lock();
    if(owner && InternalQueue(owner)==request.queue && request.event && originalQueue)originalQueue(request.queue,request.event);
}
uintptr_t Queue(void* queue,const Handle<IScriptable>& event) {
    const auto now=GetTickCount64(),last=heartbeat.load(std::memory_order_acquire);
    if(last && now-last<1000) {
        auto panel=Panel(event);
        if(panel) {
            std::lock_guard lock(mutex);
            auto owner=uiSystem.Lock();
            if(!pending.event && owner && InternalQueue(owner)==queue) {
                pending.owner=uiSystem;pending.queue=queue;
                pending.event=event;pending.panel=std::move(panel);pending.captured=now;
                hasPending.store(true,std::memory_order_release);
                Log("[vendor-route] native request retained before menu activation\n");
                return 0;
            }
        }
    }
    return originalQueue(queue,event);
}
bool Install() {
    if(!game_build::Supported() || game_build::Overridden() || !watchdogRegistered) {
        Log("[vendor-route] install gate supported=%d overridden=%d watchdog=%d\n",game_build::Supported(),game_build::Overridden(),watchdogRegistered.load());
        return false;
    }
    const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    auto* site=reinterpret_cast<uint8_t*>(base+0xB22F18);
    // Shared ink queue: GameSystemUI::QueueEvent (B22EF8) AND the quest UI
    // command (2C60920) call this. The quest command bypasses GameSystemUI.
    const uint8_t prefix[]={0x48,0x8b,0xc4,0x48,0x89,0x58,0x10,0x48,0x89,0x70,0x18,0x57,0x48,0x83,0xec,0x30};
    if(std::memcmp(site,prefix,sizeof(prefix)) ||
       *reinterpret_cast<const uintptr_t*>(base+0x30BD9C0)!=base+0xB22EF8) {
        Log("[vendor-route] UI queue byte guard mismatch\n");return false;
    }
    const auto created=MH_CreateHook(site,reinterpret_cast<void*>(&Queue),reinterpret_cast<void**>(&originalQueue));
    if(created!=MH_OK){Log("[vendor-route] create hook failed %d\n",int(created));return false;}
    const auto enabled=MH_EnableHook(site);
    if(enabled!=MH_OK){Log("[vendor-route] enable hook failed %d\n",int(enabled));MH_RemoveHook(site);return false;}
    installed=true;return true;
}
CVR_HOOK("VendorRoute",::cvr::hooks::Stage::Boot,84,Install);
}

void WatchdogRegistered(bool ready) {watchdogRegistered=ready;}
Handle<RED4ext::IScriptable> Poll(bool enabled,const RED4ext::Handle<RED4ext::IScriptable>& system) {
    Request fallback;
    Handle<RED4ext::IScriptable> result;
    {
        std::lock_guard lock(mutex);
        if(system && system->GetType()->GetName()==CName("gameuiGameSystemUI"))uiSystem=system;
        heartbeat.store(enabled&&installed&&uiSystem.Lock()?GetTickCount64():0,std::memory_order_release);
        if(enabled)result=pending.panel;
        else {fallback=std::move(pending);pending={};hasPending=false;}
    }
    Forward(std::move(fallback));
    return result;
}
bool Resolve(const RED4ext::Handle<RED4ext::IScriptable>& panel,bool accepted) {
    Request request;
    {
        std::lock_guard lock(mutex);
        if(!panel || panel!=pending.panel)return false;
        request=std::move(pending);pending={};hasPending=false;
    }
    if(!accepted)Forward(std::move(request));
    Log("[vendor-route] %s\n",accepted?"accepted by world menu":"returned to native menu");
    return true;
}
bool Update(RED4ext::CGameApplication*) {
    if(!hasPending.load(std::memory_order_acquire))return false;
    Request fallback;
    {
        std::lock_guard lock(mutex);
        if(pending.event && GetTickCount64()-pending.captured>=1500) {
            fallback=std::move(pending);pending={};hasPending=false;
        }
    }
    if(fallback.event) {
        Log("[vendor-route] timeout; restoring native request\n");
        Forward(std::move(fallback));
    }
    return false;
}
bool Reset(RED4ext::CGameApplication*) {
    heartbeat=0;
    std::lock_guard lock(mutex);pending={};uiSystem.Reset();hasPending=false;
    return true;
}
}

void VRMenuVendorPoll(RED4ext::IScriptable*,RED4ext::CStackFrame* stack,RED4ext::Handle<RED4ext::IScriptable>* out,int64_t) {
    bool enabled=false;RED4ext::Handle<RED4ext::IScriptable> system;
    RED4ext::GetParameter(stack,&enabled);RED4ext::GetParameter(stack,&system);++stack->code;
    auto panel=cvr::vendor_route::Poll(enabled,system);if(out)*out=std::move(panel);
}
void VRMenuVendorResolve(RED4ext::IScriptable*,RED4ext::CStackFrame* stack,bool* out,int64_t) {
    RED4ext::Handle<RED4ext::IScriptable> panel;bool accepted=false;
    RED4ext::GetParameter(stack,&panel);RED4ext::GetParameter(stack,&accepted);++stack->code;
    const bool ok=cvr::vendor_route::Resolve(panel,accepted);if(out)*out=ok;
}
#endif
#endif
