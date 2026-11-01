#include "Natives/NativeFunctions.hpp"
#include "Hooks/Reflex.hpp"
#include "Core/LiveControls.hpp"
#include "Core/VrCoreShared.hpp"
#include "Stereo/VrcamConfig.hpp"
#include <RED4ext/Scripting/Natives/entEntity.hpp>
#include <cstring>

// Diagnostic A/B only. Returning true retains the original Lua scan path.
extern "C" __declspec(dllexport) int CyberpunkVR_BodyCapsulePrefilter=1;

void VRShouldScanBodyCapsules(RED4ext::IScriptable*,RED4ext::CStackFrame* frame,bool* out,int64_t) {
    RED4ext::Handle<RED4ext::ent::Entity> entity;
    RED4ext::GetParameter(frame,&entity);++frame->code;
    if(!out)return;
    *out=true;
    if(!CyberpunkVR_BodyCapsulePrefilter)return;
    if(!entity){*out=false;return;}
    // Menus too: the main menu's puppet is attached and has no capsules, and sending it down the Lua path made that
    // path walk every component name each frame (the menu's periodic GC freeze, 2026-10-28).
    if(entity->status!=RED4ext::EntityStatus::Attached)return;
    // CET invokes this on the game thread. Codeware's GetComponents returns a
    // copy of this same Entity::components array. Read it in place: no nested
    // script invocation, array allocation, ownership transfer or retained cache.
    // Loading/unattached entities retain the original Lua path above.
    // The names only change with the set of components, so the name walk (a
    // name-pool lookup and a strstr for each of ~240 components, every frame
    // while no capsule exists) runs again only when the component pointers do.
    static const void* lastEntity=nullptr;
    static uint64_t lastSet=0;
    static bool lastFound=true;
    uint64_t set=0xcbf29ce484222325ull;
    for(const auto& component:entity->components)
        set=(set^reinterpret_cast<uintptr_t>(component.GetPtr()))*0x100000001b3ull;
    if(entity.GetPtr()==lastEntity && set==lastSet){*out=lastFound;return;}
    bool found=false;
    for(const auto& component:entity->components)if(component) {
        const char* name=component->name.ToString();
        if(name && std::strstr(name,"VRPortBody_")){found=true;break;}
    }
    lastEntity=entity.GetPtr();lastSet=set;lastFound=found;
    *out=found;
}

void VRReflexTiming(RED4ext::IScriptable*,RED4ext::CStackFrame* frame,RED4ext::CString* out,int64_t) {
    bool request=false;RED4ext::GetParameter(frame,&request);++frame->code;
    if(out)*out=RED4ext::CString(cvr::reflex::TimingReport(request).c_str());
}
void GetVRMouseYDisabled(RED4ext::IScriptable*,RED4ext::CStackFrame* frame,bool* out,int64_t) {
    ++frame->code;if(out)*out=g_liveControls.xrDisableMouseY!=0;
}

// The overlay's last VRCAM enable request: -1 none, 0 disable, 1 enable (see vrcam_enable_request).
void VRVrcamEnableRequest(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, int32_t* out, int64_t) {
    ++frame->code;
    if (out) *out = cvr::vrcam_enable_request();
}
