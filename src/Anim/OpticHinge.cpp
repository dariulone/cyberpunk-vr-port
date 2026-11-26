#include "Anim/OpticHinge.hpp"
#include "Anim/HeadAimWeapon.hpp"
#include "Anim/RigPoseWrites.hpp"
#include "Anim/VrikState.hpp"
#include "Camera/CameraState.hpp"
#include "Core/VrCoreShared.hpp"
#include "Hooks/RoomscaleMove.hpp"
#include "Natives/NativeFunctions.hpp"
#include "Natives/NativeHelpers.hpp"
#include <RED4ext/GameEngine.hpp>
#include <RED4ext/Containers/StaticArray.hpp>
#include <RED4ext/Scripting/Utils.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/AnimatedComponent.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/AnimationControlBinding.hpp>
#include <RED4ext/Scripting/Natives/animRig.hpp>
#include <RED4ext/Scripting/Natives/entEntity.hpp>
#include <RED4ext/Scripting/Natives/worldAnimationSystem.hpp>
#include <atomic>
#include <cmath>
#include <Windows.h>

// Existing verified lookup, used only on the Lua/native request thread.
RED4ext::world::AnimationSystem* FindWorldAnimationSystemFromScene(RED4ext::world::RuntimeScene*,uintptr_t,std::ofstream*);
extern "C" __declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_OpticsHingeStats[5]{}; // requests, accepted, applied, invalidated, conflict
// Diagnostic stage counters: disabled, context, player, signature, query, active
// mismatch, Barrel, control, graph, binding, accepted, owner scope, descriptor,
// current binding, context meta, source rig, track, remap, conflict, applied, expired.
extern "C" __declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_OpticsHingeStages[22]{};
extern "C" __declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_OpticsHingeLastStage{};

namespace cvr::anim {
namespace {
using namespace RED4ext;
using optic_hinge::Identity;
enum class Stage:uint32_t {None,Disabled,Context,Player,Signature,Query,ActiveMismatch,Barrel,Control,Graph,Binding,
    Accepted,OwnerScope,Descriptor,CurrentBinding,ContextMeta,SourceRig,Track,Remap,Conflict,Applied,Expired};
void Mark(Stage stage){const auto i=static_cast<uint32_t>(stage);++CyberpunkVR_OpticsHingeStages[i];CyberpunkVR_OpticsHingeLastStage.store(i,std::memory_order_relaxed);}
struct Binding {
    world::AnimationSystem* system{};world::RuntimeScene* scene{};
    uint32_t bucket{},entry{};Identity identity;
};
struct State {
    optic_hinge::Target target;Binding binding;
    WeakHandle<ent::Entity> player,weapon;
    WeakHandle<ent::IComponent> component,graph;
    WeakHandle<ent::AnimationControlBinding> control;
};
// Weak controls must never be released after engine teardown by CRT destruction.
// Disable/holster/stale/expired paths clear the bounded current registration.
State& Current(){static auto* state=new State;return *state;}
std::atomic<bool> active{};
std::atomic<uintptr_t> activeObject{},activeDescriptor{};
bool Context(){return g_VRBind>0&&g_menuModeValue==0&&!DeviceCamActive()&&!IsHeadAimWeaponActive();}
void ClearLocked(){auto& s=Current();active.store(false,std::memory_order_release);activeObject=0;activeDescriptor=0;s.target={};s.binding={};s.control.Reset();s.graph.Reset();s.component.Reset();s.weapon.Reset();s.player.Reset();}
void Clear(){RigPoseStateGuard lock;ClearLocked();}
bool ReadBinding(const Binding& binding,Identity& result) {
    __try {
        if(!binding.system||binding.bucket>=world::AnimationSystem::BucketCount)return false;
        auto& bucket=binding.system->entitityBuckets[binding.bucket];const auto i=binding.entry;
        if(bucket.entities.Size()>world::AnimatedEntitiesBucket::MaxEntries||bucket.animatedComponents.Size()>world::AnimatedEntitiesBucket::MaxEntries||
           bucket.animatedObjects.Size()>world::AnimatedEntitiesBucket::MaxEntries)return false;
        if(i>=bucket.entities.Size()||i>=bucket.animatedComponents.Size()||i>=bucket.animatedObjects.Size())return false;
        auto* weapon=bucket.entities[i].instance;auto* graph=bucket.animatedComponents[i];auto* object=bucket.animatedObjects[i];
        auto* component=reinterpret_cast<ent::AnimatedComponent*>(binding.identity.component);
        if(!weapon||!component||!graph||!object||uintptr_t(weapon)!=binding.identity.weapon||uintptr_t(graph)!=binding.identity.graphComponent||
           weapon->runtimeScene!=binding.scene||component->owner!=weapon||graph->owner!=weapon||component->name!=CName("Barrel")||!component->rig.IsLoaded())return false;
        auto* control=component->controlBinding.instance;
        if(binding.identity.controlBinding){
            if(uintptr_t(control)!=binding.identity.controlBinding||!control->enabled||control->bindName!=CName("Receiver")||graph->name!=control->bindName)return false;
        }else if(graph!=component||(control&&control->enabled))return false;
        auto* rig=component->rig.Get().instance;auto* meta=object->metaRig;
        if(!rig||!meta||component->rig.path!=ResourcePath("ep1\\weapons\\firearms\\rifle_sniper\\nokota_osprey\\rig\\w_rifle_sniper__nokota_osprey__base1.rig")||
           rig->boneNames.Size()!=13||!rig->parentIndices||rig->parentIndices[8]!=1||rig->parentIndices[1]!=0||
           rig->boneNames[8]!=CName("scope_hinge")||rig->boneNames[5]!=CName("cocking_mechanism")||rig->boneNames[12]!=CName("handle")||
           !meta->boneNames.Size()||meta->boneNames.Size()>VRIK_MAX_BONES||meta->parentIndeces.Size()!=meta->boneNames.Size()||!rig->referencePoseLS)return false;
        const auto& rest=rig->referencePoseLS[8].Rotation;
        if(!std::isfinite(rest.k)||std::abs(std::abs(rest.k)-1.f)>.0001f||
           !std::isfinite(rest.i+rest.j+rest.r)||std::abs(rest.i)+std::abs(rest.j)+std::abs(rest.r)>.0001f)return false;
        // Existing ObjectPoseScope / final LocalToModel path uses object+0x18.
        // Its descriptor[3] is this instance's track buffer. referenceTracks is
        // shared by equal rig assets and is deliberately never an owner key.
        auto* descriptor=*reinterpret_cast<void**>(reinterpret_cast<uint8_t*>(object)+0x18);
        const uintptr_t track=descriptor?reinterpret_cast<uintptr_t*>(descriptor)[3]:0;
        if(!descriptor||!track)return false;
        result={binding.identity.player,uintptr_t(weapon),uintptr_t(component),uintptr_t(object),uintptr_t(descriptor),
                uintptr_t(rig),uintptr_t(meta),track,object->metaRigID,uintptr_t(graph),binding.identity.controlBinding};
        return bucket.entities[i].instance==weapon&&bucket.animatedComponents[i]==graph&&bucket.animatedObjects[i]==object&&
            (!binding.identity.controlBinding||(component->controlBinding.instance==control&&control->enabled&&control->bindName==graph->name));
    } __except(EXCEPTION_EXECUTE_HANDLER){return false;}
}
bool Scan(world::AnimationSystem* system,world::RuntimeScene* scene,Identity identity,Binding& result){
    __try {
        for(uint32_t b=0;b<world::AnimationSystem::BucketCount;++b){auto& bucket=system->entitityBuckets[b];
            const auto count=bucket.entities.Size();if(count>world::AnimatedEntitiesBucket::MaxEntries)continue;
            for(uint32_t i=0;i<count;++i){
                if(uintptr_t(bucket.entities[i].instance)!=identity.weapon||i>=bucket.animatedComponents.Size()||uintptr_t(bucket.animatedComponents[i])!=identity.graphComponent)continue;
                Binding candidate{system,scene,b,i,identity};
                if(ReadBinding(candidate,candidate.identity)){result=candidate;return true;}
            }}
        return false;
    } __except(EXCEPTION_EXECUTE_HANDLER){return false;}
}
bool Resolve(const Handle<ent::Entity>& player,const Handle<ent::Entity>& weapon,
             const Handle<ent::IComponent>& component,const Handle<ent::IComponent>& graph,
             const Handle<ent::AnimationControlBinding>& control,Binding& result) {
    Binding old;{RigPoseStateGuard lock;old=Current().binding;}
    old.identity.player=uintptr_t(player.instance);
    if(old.identity.weapon==uintptr_t(weapon.instance)&&old.identity.component==uintptr_t(component.instance)&&
       old.identity.graphComponent==uintptr_t(graph.instance)&&old.identity.controlBinding==uintptr_t(control.instance)&&old.scene==weapon->runtimeScene&&
       ReadBinding(old,old.identity)){result=old;return true;}
    auto* engine=CGameEngine::Get();auto* framework=engine?engine->framework:nullptr;
    auto* system=FindWorldAnimationSystemFromScene(weapon->runtimeScene,framework?framework->unk18:0,nullptr);
    if(!system)return false;
    Identity identity;identity.player=uintptr_t(player.instance);identity.weapon=uintptr_t(weapon.instance);identity.component=uintptr_t(component.instance);
    identity.graphComponent=uintptr_t(graph.instance);identity.controlBinding=uintptr_t(control.instance);
    return Scan(system,weapon->runtimeScene,identity,result);
}
bool IsActiveWeapon(const Handle<ent::Entity>& player,const Handle<ent::Entity>& weapon){
    auto* rtti=CRTTISystem::Get();auto* cls=rtti->GetClass("ScriptedPuppet");auto* fn=cls?cls->GetFunction("GetActiveWeapon"):nullptr;
    // ScriptedPuppet.GetActiveWeapon(ref<GameObject>) -> wref<WeaponObject>.
    // A CClass* ExecuteFunction overload performs GetSystem(class), which is
    // wrong for this static actor method. Validate the reflected signature and
    // invoke the void* overload with the actual player as execution context.
    if(!fn||!fn->flags.isStatic||fn->params.Size()!=1||!fn->returnType||!fn->params[0]->type||!fn->returnType->type||
       fn->params[0]->type->GetType()!=RED4ext::rtti::ERTTIType::Handle||fn->returnType->type->GetType()!=RED4ext::rtti::ERTTIType::WeakHandle||
       static_cast<CRTTIHandleType*>(fn->params[0]->type)->GetInnerType()!=rtti->GetClass("gameObject")||
       static_cast<CRTTIWeakHandleType*>(fn->returnType->type)->GetInnerType()!=rtti->GetClass("gameweaponObject")){
        Mark(Stage::Signature);return false;}
    WeakHandle<ent::Entity> current;StackArgs_t args;args.emplace_back(fn->params[0]->type,const_cast<Handle<ent::Entity>*>(&player));
    if(!ExecuteFunction(static_cast<void*>(player.instance),fn,&current,args)){Mark(Stage::Query);return false;}
    const bool matches=!current.Expired()&&current.instance==weapon.instance;
    if(!matches)Mark(Stage::ActiveMismatch);return matches;
}
bool Request(const Handle<ent::Entity>& weapon,bool enabled){
    ++CyberpunkVR_OpticsHingeStats[0];
    if(!enabled){Mark(Stage::Disabled);Clear();return true;}
    if(!weapon||!Context()){Mark(Stage::Context);Clear();return false;}
    Handle<ent::Entity> player(FindPlayerEntity());
    if(!player||uintptr_t(player.instance)!=roomscale::PlayerIdentity()){Mark(Stage::Player);Clear();return false;}
    if(!IsActiveWeapon(player,weapon)){Clear();return false;}
    Handle<ent::IComponent> component;auto* type=CRTTISystem::Get()->GetClass("entAnimatedComponent");
    for(const auto& c:weapon->components)if(c&&c->name==CName("Barrel")&&c->owner==weapon.instance&&type&&c->GetType()->IsA(type)){component=c;break;}
    if(!component){Mark(Stage::Barrel);Clear();return false;}
    auto control=static_cast<ent::AnimatedComponent*>(component.instance)->controlBinding;
    Handle<ent::IComponent> graph;
    if(control&&control->enabled){
        if(control->bindName!=CName("Receiver")){Mark(Stage::Control);Clear();return false;}
        for(const auto& c:weapon->components)if(c&&c->name==control->bindName&&c->owner==weapon.instance&&c->GetType()->IsA(type)){graph=c;break;}
    }else{graph=component;control.Reset();}
    Binding binding;
    if(!graph){Mark(Stage::Graph);Clear();return false;}
    if(!Resolve(player,weapon,component,graph,control,binding)){Mark(Stage::Binding);Clear();return false;}
    RigPoseStateGuard lock;auto& s=Current();s.binding=binding;s.player=player;s.weapon=weapon;s.component=component;s.graph=graph;s.control=control;
    s.target={binding.identity,GetTickCount64(),true};activeObject= binding.identity.object;activeDescriptor=binding.identity.descriptor;
    active.store(true,std::memory_order_release);++CyberpunkVR_OpticsHingeStats[1];Mark(Stage::Accepted);return true;
}

// All native pointer reads/write are contained here, with no RAII temporaries.
// The caller holds strong player/weapon/components/control handles and the rig lock.
int TryApply(const Binding& binding,const optic_hinge::Target& target,void* object,void** a1,void** a2,unsigned lod,
             void* poseDescriptor,uint8_t* bones,uintptr_t track,uint64_t now){
    __try{
        Identity current;if(!ReadBinding(binding,current)||current!=target.identity){Mark(Stage::CurrentBinding);return -1;}
        if(uintptr_t(a2[5])!=current.metaRig){Mark(Stage::ContextMeta);return 0;} // old in-flight graph context
        if(uintptr_t(a1[8])!=current.rig){Mark(Stage::SourceRig);return 0;}
        if(uintptr_t(poseDescriptor)!=current.descriptor){Mark(Stage::Descriptor);return 0;}
        if(track!=current.track){Mark(Stage::Track);return 0;}
        optic_hinge::Pass pass{current,uintptr_t(object),uintptr_t(poseDescriptor),uintptr_t(a1[8]),track,roomscale::PlayerIdentity(),uintptr_t(a2[5]),now,true,true,false};
        if(!optic_hinge::Matches(target,pass)){Mark(Stage::Expired);return 0;}
        auto* six=reinterpret_cast<uint8_t**>(a2)[6];auto* map=six?*reinterpret_cast<uint32_t**>(six+0x40):nullptr;
        auto* counts=six?*reinterpret_cast<uint16_t**>(six+0x50):nullptr;if(!map||!counts){Mark(Stage::Remap);return 0;}
        const uint32_t count=counts[lod],slots=*reinterpret_cast<uint32_t*>(static_cast<uint8_t*>(poseDescriptor)+8);
        auto* meta=reinterpret_cast<RED4ext::anim::MetaRig*>(current.metaRig);
        static_assert(sizeof(RED4ext::CName)==sizeof(uint64_t));
        const std::span<const uint64_t> names(reinterpret_cast<const uint64_t*>(&meta->boneNames[0]),meta->boneNames.Size());
        const bool applied=count<=optic_hinge::BoneCount&&optic_hinge::Apply(target,pass,std::span<const uint32_t>(map,count*2),bones,slots,names);
        Mark(applied?Stage::Applied:Stage::Remap);return applied?1:0;
    }__except(EXCEPTION_EXECUTE_HANDLER){return -1;}
}
}

void ApplyOpticHinge(void* object,void* objectDescriptor,void** a1,void** a2,unsigned lod,
                     void* poseDescriptor,uint8_t* bones,uintptr_t track) {
    if(!active.load(std::memory_order_acquire)||uintptr_t(object)!=activeObject.load(std::memory_order_relaxed))return;
    Mark(Stage::OwnerScope);
    if(uintptr_t(objectDescriptor)!=activeDescriptor.load(std::memory_order_relaxed)){Mark(Stage::Descriptor);return;}
    State snapshot;{RigPoseStateGuard lock;if(!Current().target.enabled)return;snapshot=Current();}
    auto player=snapshot.player.Lock();auto weapon=snapshot.weapon.Lock();auto component=snapshot.component.Lock();auto graph=snapshot.graph.Lock();
    auto control=snapshot.control.Lock();
    {RigPoseStateGuard lock;auto& s=Current();if(!s.target.enabled||s.target.identity!=snapshot.target.identity)return;
        const auto now=GetTickCount64();const bool alive=player&&weapon&&component&&graph&&(!s.target.identity.controlBinding||control);
        if(!Context()||!alive||now<s.target.stamp||now-s.target.stamp>=200||roomscale::PlayerIdentity()!=s.target.identity.player){
            Mark(Stage::Expired);++CyberpunkVR_OpticsHingeStats[3];ClearLocked();return;}
        for(int i=0;i<g_RigWriteN&&i<VRRIG_WRITES;++i)if(g_RigWriteEnabled[i]&&g_RigWriteWhich[i]==1&&g_RigWriteBone[i]==int(optic_hinge::Bone)){
            Mark(Stage::Conflict);++CyberpunkVR_OpticsHingeStats[4];return;}
        const int result=TryApply(s.binding,s.target,object,a1,a2,lod,poseDescriptor,bones,track,now);
        if(result>0)++CyberpunkVR_OpticsHingeStats[2];
        else if(result<0){++CyberpunkVR_OpticsHingeStats[3];ClearLocked();}
    } // engine strong handles are released only after the rig lock is gone
}
}

void VRPhysicalOpticsHinge(RED4ext::IScriptable*,RED4ext::CStackFrame* frame,bool* out,int64_t){
    RED4ext::Handle<RED4ext::ent::Entity> weapon;bool enabled=false;
    RED4ext::GetParameter(frame,&weapon);RED4ext::GetParameter(frame,&enabled);++frame->code;
    const bool result=cvr::anim::Request(weapon,enabled);if(out)*out=result;
}
