#include "Hooks/LadderInput.hpp"
#include "Hooks/SwimmingInput.hpp"
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Scripting/Utils.hpp>
#include <algorithm>
#include <array>
#include <cmath>
#include <cstring>

extern void Log(const char*,...);

// The state machines' input reads, gamestateMachineGameScriptInterface::GetActionValue and IsActionJustPressed,
// with the player's machines given the gestures: the ladder's climbing axis on a resting MoveY, the swimming strokes
// on MoveY/ToggleSprint/Jump/Dive. They were Lua overrides (the Ladder and Swimming mods): ~10 calls a tick, made on
// the world tick's workers, each through the Lua host's whole override path and its state lock (~0.4 ms a tick, the
// host's most sampled lines). Here the function objects are swapped with native ones the way the host's overrides swap
// them: the original moves into the object made here and is called through it, so a mod's own Override of the same
// function still chains over this one.
namespace cvr::swimming {
namespace {
RED4ext::CBaseFunction* g_actionValue{};   // the originals, after the swap
RED4ext::CBaseFunction* g_justPressed{};
uint32_t g_ownerOffset{};                  // StateGameScriptInterface.owner, a WeakHandle<GameObject>

uintptr_t Owner(RED4ext::IScriptable* context) {
    if(!context || !g_ownerOffset)return 0;
    return *reinterpret_cast<const uintptr_t*>(reinterpret_cast<const uint8_t*>(context)+g_ownerOffset);
}
// The original with this one argument, without the argument's bytecode running twice.
bool CallOriginal(RED4ext::CBaseFunction* function,RED4ext::IScriptable* context,RED4ext::CName& name,void* out) {
    RED4ext::CStackType arg(function->params[0]->type,&name);
    RED4ext::CStackType result(function->returnType->type,out);
    RED4ext::CStack stack(context,&arg,1,&result);
    return function->Execute(&stack);
}
void ActionValue(RED4ext::IScriptable* context,RED4ext::CStackFrame* frame,float* out,int64_t) {
    RED4ext::CName name;
    RED4ext::GetParameter(frame,&name);
    frame->code++;
    float value=0;
    CallOriginal(g_actionValue,context,name,&value);
    const auto owner=Owner(context);
    // On a ladder the climbing axis stands in for a stick at rest (the stick still wins once pushed).
    static const uint64_t moveY=RED4ext::CName("MoveY").hash;
    if(name.hash==moveY && std::abs(value)<=.15f && IsPlayer(owner)) {
        const float axis=cvr::ladder::NativeAction();
        if(axis!=0)value=axis;
    }
    const float added=ScriptAction(owner,name.hash,false);
    if(out)*out=added>0 ? std::max(value,added):value;
}
void JustPressed(RED4ext::IScriptable* context,RED4ext::CStackFrame* frame,bool* out,int64_t) {
    RED4ext::CName name;
    RED4ext::GetParameter(frame,&name);
    frame->code++;
    bool pressed=false;
    CallOriginal(g_justPressed,context,name,&pressed);
    if(out)*out=pressed || ScriptAction(Owner(context),name.hash,true)>0;
}

template<typename T>
RED4ext::CBaseFunction* Swap(RED4ext::CClass* type,const char* name,RED4ext::ScriptingFunction_t<T> handler) {
    auto* real=type->GetFunction(RED4ext::CName(name));
    if(!real || !real->flags.isNative || real->flags.isStatic || real->params.Size()!=1 || !real->returnType)return nullptr;
    const auto fullName=std::string(real->fullName.ToString());
    auto* ours=RED4ext::CClassFunction::Create(type,fullName.c_str(),name,handler,real->flags);
    if(!ours)return nullptr;
    ours->parent=real->parent;
    ours->fullName=real->fullName;ours->shortName=real->shortName;ours->returnType=real->returnType;
    ours->params.Clear();
    for(auto* p:real->params)ours->params.PushBack(p);
    ours->localVars.Clear();
    for(auto* p:real->localVars)ours->localVars.PushBack(p);
    ours->unk20=real->unk20;ours->unk48=real->unk48;ours->unkAC=real->unkAC;
    std::memcpy(&ours->bytecode,&real->bytecode,sizeof(real->bytecode));
    std::array<uint8_t,sizeof(RED4ext::CClassFunction)> swap{};
    std::memcpy(swap.data(),real,swap.size());
    std::memcpy(static_cast<void*>(real),ours,swap.size());
    std::memcpy(static_cast<void*>(ours),swap.data(),swap.size());
    return ours;
}
}

bool InstallScriptHooks() {
    static bool done=false;
    if(done)return g_actionValue && g_justPressed;
    done=true;
    auto* rtti=RED4ext::CRTTISystem::Get();
    auto* type=rtti ? rtti->GetClass("gamestateMachineGameScriptInterface"):nullptr;
    if(!type) { Log("[input] state machine action reads: class missing, the gestures reach native locomotion only\n");return false; }
    if(auto* owner=type->GetProperty("owner"))g_ownerOffset=owner->valueOffset;
    g_actionValue=Swap(type,"GetActionValue",&ActionValue);
    g_justPressed=Swap(type,"IsActionJustPressed",&JustPressed);
    Log("[input] state machine action reads native: GetActionValue %s, IsActionJustPressed %s, owner +0x%X\n",
        g_actionValue ? "ok":"FAILED",g_justPressed ? "ok":"FAILED",g_ownerOffset);
    return g_actionValue && g_justPressed;
}
bool ScriptHooksActive() { return g_actionValue && g_justPressed; }
}

extern "C" bool CvrInstallActionReads() { return cvr::swimming::InstallScriptHooks(); }
