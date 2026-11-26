#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
// AnimDumps -- writing down what the engine's animation objects actually contain.
//
// Every non-obvious fact the animation work in this project rests on came out of one of these dumps:
// which graph variables exist and what they are named, which meta-rig tracks a rig carries, what a
// listener list holds, which properties an anim class really has as against what the .reds dump claims.
//
// THE .reds DUMP IS INCOMPLETE, and that is the reason this file is not a debugging leftover. Functions
// that exist and are callable are missing from it -- Set/GetLinearVelocity on the physics body interface
// being the case that cost the most time -- so "does this API exist" has to be answered by looking at
// the live RTTI rather than by reading the dump.
//
// All of it writes to a file rather than the log, because these outputs are hundreds of lines and are
// read afterwards, side by side, not watched as they happen.

#include <RED4ext/RED4ext.hpp>
#include <RED4ext/GameEngine.hpp>
#include <sstream>
#include <locale>
#include <clocale>
#include "Utils/SharedSlots.hpp"   // CyberpunkVR_Hands_Shared slot map (single source of truth)
#include <RED4ext/Containers/StaticArray.hpp>
#include <RED4ext/Scripting/Natives/ScriptGameInstance.hpp>
#include <RED4ext/Scripting/Utils.hpp>
#include <RED4ext/Scripting/Functions.hpp>
#include <RED4ext/Scripting/CProperty.hpp>
#include <RED4ext/Scripting/Natives/Generated/WorldPosition.hpp>
#include <RED4ext/Scripting/Natives/Transform.hpp>
#include <RED4ext/Scripting/Natives/animRig.hpp>
#include <RED4ext/Scripting/Natives/Generated/Vector4.hpp>
#include <RED4ext/Scripting/Natives/Generated/Quaternion.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimGraph.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimFeature_IK.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimFeature_MeleeIKData.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimFeature_WeaponUser.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimVariableBool.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimVariableContainer.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimVariableFloat.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimVariableInt.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimVariableQuaternion.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimVariableTransform.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimVariableVector.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/AnimationControlBinding.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/AnimInputSetterAnimFeature.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/AnimInputSetterFloat.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/AnimInputSetterVector.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/IBinding.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/IKTargetAddEvent.hpp>
#include <RED4ext/Scripting/Natives/Generated/red/Event.hpp>
#include <RED4ext/Scripting/Natives/entEntity.hpp>
#include <RED4ext/Scripting/Natives/entAnimationControllerComponent.hpp>
#include <RED4ext/Scripting/Natives/entIPlacedComponent.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/AnimatedComponent.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/StaticOrientationProvider.hpp>
#include <RED4ext/Scripting/Natives/worldAnimationSystem.hpp>
#include <RED4ext/Scripting/Natives/worldAnimationSystemScriptInterface.hpp>
#include <RED4ext/Scripting/Natives/entSkinnedMeshComponent.hpp>
#include <RED4ext/Scripting/Natives/entAnimationControllerComponent.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/GarmentSkinnedMeshComponent.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/MeshComponent.hpp>
#include <windows.h>
#include <algorithm>
#include <cctype>
#include <cstring>
#include <atomic>
#include <cstdint>
#include <fstream>
#include <utility>
#include <iomanip>
#include <string>
#include "Anim/VrikHook.hpp"
#include "Anim/WeaponAim.hpp"
#include "Natives/NativeState.hpp"
#include "Natives/AnimInternal.hpp"
#include "Natives/NativeHelpers.hpp"
#include <MinHook.h>
#include "Natives/NativeFunctions.hpp"
#include "Natives/NativeHelpers.hpp"
#include "Natives/NativeState.hpp"

#if !CVR_RUST_VRIK
// The C++ build's dumps; the Rust build's are rust/crates/cvr-ffi/src/anim_dumps.rs.
static void DumpFunctionDetails(std::ofstream& aOut, RED4ext::CBaseFunction* aFunc)
{
    if (!aFunc)
        return;

    aOut << aFunc->fullName.ToString() << "\n";
    aOut << "  shortName=" << aFunc->shortName.ToString() << "\n";
    aOut << "  return=" << (aFunc->returnType ? GetTypeNameForDump(aFunc->returnType->type) : "Void") << "\n";
    aOut << "  params(" << aFunc->params.Size() << ")\n";

    for (uint32_t i = 0; i < aFunc->params.Size(); ++i)
    {
        auto* param = aFunc->params[i];
        aOut << "    [" << i << "] name=" << (param ? param->name.ToString() : "<null>")
             << " type=" << (param ? GetTypeNameForDump(param->type) : "<null>")
             << " out=" << ((param && param->flags.isOut) ? 1 : 0)
             << " optional=" << ((param && param->flags.isOptional) ? 1 : 0)
             << "\n";
    }

    aOut << "\n";
}

static void DumpClassProperties(std::ofstream& aOut, const char* aClassName)
{
    auto* rtti = RED4ext::CRTTISystem::Get();
    auto* cls = rtti ? rtti->GetClass(aClassName) : nullptr;

    aOut << "==================================================\n";
    aOut << "CLASS " << aClassName << "\n";
    aOut << "==================================================\n";
    if (!cls)
    {
        aOut << "<missing>\n\n";
        return;
    }

    RED4ext::DynArray<RED4ext::CProperty*> props;
    cls->GetProperties(props);

    aOut << "size=" << cls->size << " alignment=" << cls->alignment << " propCount=" << props.Size() << "\n";
    for (uint32_t i = 0; i < props.Size(); ++i)
    {
        auto* prop = props[i];
        aOut << "  [" << i << "] name=" << (prop ? prop->name.ToString() : "<null>")
             << " type=" << (prop ? GetTypeNameForDump(prop->type) : "<null>")
             << " offset=0x" << std::hex << (prop ? prop->valueOffset : 0) << std::dec
             << " inHolder=" << ((prop && prop->flags.inValueHolder) ? 1 : 0)
             << " handle=" << ((prop && prop->flags.isHandle) ? 1 : 0)
             << "\n";
    }

    aOut << "\n";
}

static RED4ext::CProperty* FindFirstPropertyByType(RED4ext::CClass* aClass, const char* aTypeSubstring, uint32_t aMinOffset = 0)
{
    if (!aClass || !aTypeSubstring)
        return nullptr;

    RED4ext::DynArray<RED4ext::CProperty*> props;
    aClass->GetProperties(props);
    for (uint32_t i = 0; i < props.Size(); ++i)
    {
        auto* prop = props[i];
        if (!prop || prop->valueOffset < aMinOffset)
            continue;

        if (ContainsInsensitive(GetTypeNameForDump(prop->type), aTypeSubstring))
            return prop;
    }

    return nullptr;
}

RED4ext::Handle<RED4ext::ent::IPositionProvider> CreateStaticPositionProvider(const RED4ext::Vector4& aPosition)
{
    auto* rtti = RED4ext::CRTTISystem::Get();
    auto* cls = rtti ? rtti->GetClass("entStaticPositionProvider") : nullptr;
    if (!cls)
        return {};

    auto* instance = cls->CreateInstance(true);
    if (!instance)
        return {};

    cls->InitializeProperties(instance);

    if (auto* vectorProp = FindFirstPropertyByType(cls, "Vector4", 0x50))
    {
        vectorProp->SetValue<RED4ext::Vector4>(instance, aPosition);
        return RED4ext::Handle<RED4ext::ent::IPositionProvider>(reinterpret_cast<RED4ext::ent::IPositionProvider*>(instance));
    }

    if (auto* worldPosProp = FindFirstPropertyByType(cls, "WorldPosition", 0x50))
    {
        const RED4ext::WorldPosition worldPos(aPosition);
        worldPosProp->SetValue<RED4ext::WorldPosition>(instance, worldPos);
        return RED4ext::Handle<RED4ext::ent::IPositionProvider>(reinterpret_cast<RED4ext::ent::IPositionProvider*>(instance));
    }

    return {};
}

RED4ext::Handle<RED4ext::ent::IOrientationProvider> CreateStaticOrientationProvider()
{
    auto* rtti = RED4ext::CRTTISystem::Get();
    auto* cls = rtti ? rtti->GetClass("entStaticOrientationProvider") : nullptr;
    if (!cls)
        return {};

    auto* instance = cls->CreateInstance(true);
    if (!instance)
        return {};

    cls->InitializeProperties(instance);

    auto* provider = reinterpret_cast<RED4ext::ent::StaticOrientationProvider*>(instance);
    provider->staticOrientation.i = 0.0f;
    provider->staticOrientation.j = 0.0f;
    provider->staticOrientation.k = 0.0f;
    provider->staticOrientation.r = 1.0f;
    return RED4ext::Handle<RED4ext::ent::IOrientationProvider>(provider);
}

// Static orientation provider with a given quaternion (= the VR controller aim). Used to swap the
// projectile launch's logicalOrientationProvider so the bullet flies down the controller/barrel.
RED4ext::Handle<RED4ext::ent::IOrientationProvider> CreateStaticOrientationProviderQ(const RED4ext::Quaternion& aQuat)
{
    auto* rtti = RED4ext::CRTTISystem::Get();
    auto* cls = rtti ? rtti->GetClass("entStaticOrientationProvider") : nullptr;
    if (!cls) return {};
    auto* instance = cls->CreateInstance(true);
    if (!instance) return {};
    cls->InitializeProperties(instance);
    auto* provider = reinterpret_cast<RED4ext::ent::StaticOrientationProvider*>(instance);
    provider->staticOrientation = aQuat;
    return RED4ext::Handle<RED4ext::ent::IOrientationProvider>(provider);
}

void AppendPlayerControllerIKState(std::ostream& aOut)
{
    auto* playerEntity = FindPlayerEntity();
    if (!playerEntity)
    {
        aOut << "playerEntity=<null>\n";
        return;
    }

    for (auto& componentHandle : playerEntity->components)
    {
        auto* component = componentHandle.instance;
        if (!component)
            continue;

        auto* type = component->GetType();
        if (!type || type->name != "entAnimationControllerComponent")
            continue;

        auto* controller = reinterpret_cast<RED4ext::ent::AnimationControllerComponent*>(component);
        aOut << "controller name=" << component->name.ToString()
             << " ptr=0x" << std::hex << reinterpret_cast<uintptr_t>(controller) << std::dec
             << " targetData.size=" << controller->ikTargetController.targetData.Size()
             << " ikParams=0x" << std::hex << reinterpret_cast<uintptr_t>(controller->ikTargetController.ikParams.instance) << std::dec
             << "\n";

        for (uint32_t i = 0; i < controller->ikTargetController.targetData.Size(); ++i)
        {
            const auto& target = controller->ikTargetController.targetData[i];
            aOut << "  [" << i << "] id=" << target.targetReference.id
                 << " part=" << target.targetReference.part.ToString()
                 << " posProvider=0x" << std::hex << reinterpret_cast<uintptr_t>(target.positionProvider.instance)
                 << " orientProvider=0x" << reinterpret_cast<uintptr_t>(target.orientationProvider.instance)
                 << std::dec << "\n";
        }
    }
}

void DumpBindingInfo(std::ofstream& aOut, RED4ext::ent::AnimationControlBinding* aBinding, const char* aPrefix)
{
    if (!aBinding)
    {
        aOut << aPrefix << "binding=<null>\n";
        return;
    }

    auto* serializable = reinterpret_cast<RED4ext::ISerializable*>(aBinding);
    auto* type = serializable->GetType();
    auto* binding = reinterpret_cast<RED4ext::ent::IBinding*>(aBinding);

    aOut << aPrefix
         << "binding ptr=0x" << std::hex << reinterpret_cast<uintptr_t>(aBinding) << std::dec
         << " type=" << (type ? type->name.ToString() : "<null>")
         << " enabled=" << (binding->enabled ? 1 : 0)
         << " bindName=" << binding->bindName.ToString()
         << "\n";
}

void DumpAnimTrackParameters(std::ofstream& aOut, RED4ext::ent::AnimatedComponent* aAnimated, const char* aPrefix)
{
    if (!aAnimated)
        return;

    for (uint32_t i = 0; i < aAnimated->animParameters.Size(); ++i)
    {
        const auto& param = aAnimated->animParameters[i];
        aOut << aPrefix
             << "param[" << i << "] track=" << param.animTrackName.ToString()
             << " name=" << param.parameterName.ToString()
             << " default=" << param.defaultValue
             << "\n";
    }
}

// template LoadResourceRef moved to Natives/AnimInternal.hpp: a template has to be
// VISIBLE where it is instantiated, so a declaration alone cannot work.

void DumpNameListFiltered(std::ofstream& aOut, const char* aLabel, const RED4ext::DynArray<RED4ext::CName>& aNames)
{
    aOut << aLabel << " count=" << aNames.Size() << "\n";
    for (uint32_t i = 0; i < aNames.Size(); ++i)
    {
        const char* name = aNames[i].ToString();
        if (!name)
            continue;

        if (ContainsInsensitive(name, "arm") ||
            ContainsInsensitive(name, "hand") ||
            ContainsInsensitive(name, "ik") ||
            ContainsInsensitive(name, "weapon") ||
            ContainsInsensitive(name, "zoom") ||
            ContainsInsensitive(name, "render") ||
            ContainsInsensitive(name, "visibility"))
        {
            aOut << "  - " << name << "\n";
        }
    }
}

RED4ext::ent::AnimatedComponent* FindPlayerAnimatedComponentByName(const char* aName)
{
    if (!aName)
        return nullptr;

    auto* playerEntity = FindPlayerEntity();
    if (!playerEntity)
        return nullptr;

    for (auto& componentHandle : playerEntity->components)
    {
        auto* component = componentHandle.instance;
        if (!component)
            continue;

        auto* type = component->GetType();
        if (!type || type->name != "entAnimatedComponent")
            continue;

        const char* name = component->name.ToString();
        if (name && std::strcmp(name, aName) == 0)
            return reinterpret_cast<RED4ext::ent::AnimatedComponent*>(component);
    }

    return nullptr;
}

void DumpAnimControllerListeners(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;

    std::ofstream out(VRDiagPath("anim_controller_listeners.txt"), std::ios::trunc);
    if (!out.is_open())
    {
        if (aOut) *aOut = -1;
        return;
    }

    auto* rtti = RED4ext::CRTTISystem::Get();
    auto* controllerCls = rtti ? rtti->GetClass("entAnimationControllerComponent") : nullptr;
    if (!controllerCls)
    {
        if (aOut) *aOut = -2;
        return;
    }

    const char* eventNames[] = {
        "redEvent",
        "entAnimInputSetter",
        "entAnimInputSetterVector",
        "entAnimInputSetterAnimFeature",
        "entIKTargetAddEvent"
    };

    out << "CLASS entAnimationControllerComponent\n";
    out << "listenerCount=" << controllerCls->listeners.Size() << "\n\n";

    out << "Known event type ids:\n";
    for (const char* eventName : eventNames)
    {
        auto* eventCls = rtti->GetClass(eventName);
        out << "  " << eventName << " -> eventTypeId="
            << (eventCls ? eventCls->eventTypeId : -1)
            << "\n";
    }

    out << "\nListeners:\n";
    for (uint32_t i = 0; i < controllerCls->listeners.Size(); ++i)
    {
        const auto& listener = controllerCls->listeners[i];
        out << "  [" << i << "] callback=" << listener.callbackName.ToString()
            << " eventTypeId=" << listener.eventTypeId
            << " scripted=" << (listener.isScripted ? 1 : 0)
            << "\n";
    }

    out.close();
    if (aOut) *aOut = static_cast<int32_t>(controllerCls->listeners.Size());
}

void DumpAnimControllerFunctionDetails(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;

    std::ofstream out(VRDiagPath("anim_controller_function_details.txt"), std::ios::trunc);
    if (!out.is_open())
    {
        if (aOut) *aOut = -1;
        return;
    }

    auto* rtti = RED4ext::CRTTISystem::Get();
    auto* controllerCls = rtti ? rtti->GetClass("entAnimationControllerComponent") : nullptr;
    auto* entityCls = rtti ? rtti->GetClass("entEntity") : nullptr;

    const char* controllerFuncs[] = {
        "ApplyFeature",
        "PushEvent",
        "SetInputVector",
        "SetInputFloat",
        "SetInputBool",
        "SetInputQuaternion",
        "OnSetInputVectorEvent"
    };

    const char* entityFuncs[] = {
        "QueueEvent",
        "QueueEventForNodeID",
        "QueueEventForEntityID"
    };

    out << "CLASS entAnimationControllerComponent\n\n";
    if (controllerCls)
    {
        for (const char* name : controllerFuncs)
            DumpFunctionDetails(out, controllerCls->GetFunction(name));
    }

    out << "CLASS entEntity\n\n";
    if (entityCls)
    {
        for (const char* name : entityFuncs)
            DumpFunctionDetails(out, entityCls->GetFunction(name));
    }

    out.close();
    if (aOut) *aOut = 1;
}

void DumpInterestingAnimClassProperties(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;

    std::ofstream out(VRDiagPath("anim_interesting_class_properties.txt"), std::ios::trunc);
    if (!out.is_open())
    {
        if (aOut) *aOut = -1;
        return;
    }

    const char* classNames[] = {
        "entIBinding",
        "entAnimationControlBinding",
        "entStaticPositionProvider",
        "entStaticOrientationProvider",
        "entIKTargetAddEvent",
        "entIKTargetRemoveEvent",
        "entAnimInputSetterVector",
        "entAnimInputSetterAnimFeature",
        "entAnimatedComponent",
        "entAnimationControllerComponent"
    };

    for (const char* className : classNames)
        DumpClassProperties(out, className);

    out.close();
    if (aOut) *aOut = 1;
}

void DumpAnimationSystemCandidates(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;

    std::ofstream out(VRDiagPath("animation_system_candidates.txt"), std::ios::trunc);
    if (!out.is_open())
    {
        if (aOut) *aOut = -1;
        return;
    }

    auto* playerEntity = FindPlayerEntity();
    if (!playerEntity)
    {
        if (aOut) *aOut = -2;
        return;
    }

    DumpEntityAnimationInfo(out, playerEntity, "player_entity");

    out.close();
    if (aOut) *aOut = 1;
}

void DumpPlayerAnimatedObjectRuntime(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;

    std::ofstream out(VRDiagPath("player_animated_object_runtime.txt"), std::ios::trunc);
    if (!out.is_open())
    {
        if (aOut) *aOut = -79;
        return;
    }

    auto* playerEntity = FindPlayerEntity();
    auto* engine = RED4ext::CGameEngine::Get();
    auto* framework = engine ? engine->framework : nullptr;
    auto* animationSystem = playerEntity
        ? FindWorldAnimationSystemFromScene(playerEntity->runtimeScene, framework ? framework->unk18 : 0, &out)
        : nullptr;
    if (!playerEntity || !animationSystem)
    {
        if (aOut) *aOut = -80;
        return;
    }

    int32_t matches = 0;
    for (uint32_t bucketIndex = 0; bucketIndex < RED4ext::world::AnimationSystem::BucketCount; ++bucketIndex)
    {
        auto& bucket = animationSystem->entitityBuckets[bucketIndex];
        const uint32_t entryCount = bucket.entities.Size();
        for (uint32_t entryIndex = 0; entryIndex < entryCount; ++entryIndex)
        {
            auto& entityHandle = bucket.entities[entryIndex];
            auto* entity = reinterpret_cast<RED4ext::ent::Entity*>(entityHandle.instance);
            if (entity != playerEntity)
                continue;

            auto* animatedComponent = bucket.animatedComponents[entryIndex];
            auto* animatedObject = bucket.animatedObjects[entryIndex];

            out << "bucket=" << bucketIndex << " entry=" << entryIndex
                << " animatedObject=0x" << std::hex << reinterpret_cast<uintptr_t>(animatedObject)
                << " animatedComponent=0x" << reinterpret_cast<uintptr_t>(animatedComponent)
                << std::dec;
            if (animatedComponent)
                out << " componentName=" << animatedComponent->name.ToString();
            out << "\n";

            if (animatedObject)
            {
                out << "  metaRigID=" << animatedObject->metaRigID
                    << " metaRig=0x" << std::hex << reinterpret_cast<uintptr_t>(animatedObject->metaRig)
                    << " metaRigInfo=0x" << reinterpret_cast<uintptr_t>(animatedObject->metaRigInfo)
                    << std::dec
                    << " distance=" << animatedObject->distanceFromCamera
                    << " cameraLevel=" << animatedObject->cameraDistanceLevel
                    << " lastLevel=" << animatedObject->lastDistanceLevel
                    << "\n";
                DumpMetaRigTracks(out, animatedObject->metaRig, "  ");
            }

            if (entryIndex < bucket.componentBindings.Size())
            {
                const auto& bindingSet = bucket.componentBindings[entryIndex];
                out << "  componentBindings=" << bindingSet.bindings.Size() << "\n";
                for (uint32_t i = 0; i < bindingSet.bindings.Size(); ++i)
                {
                    const auto& binding = bindingSet.bindings[i];
                    out << "    [" << i << "] placed=0x" << std::hex << reinterpret_cast<uintptr_t>(binding.placedComponent.instance)
                        << " animComp=0x" << reinterpret_cast<uintptr_t>(binding.animComponent)
                        << " attachment=0x" << reinterpret_cast<uintptr_t>(binding.transformAttachment.instance)
                        << std::dec;
                    if (binding.animComponent)
                        out << " animCompName=" << binding.animComponent->name.ToString();
                    out << "\n";
                }
            }

            out << "\n";
            ++matches;
        }
    }

    out.close();
    if (aOut) *aOut = matches;
}

void DumpRootMetaRigTracks(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;

    std::ofstream out(VRDiagPath("root_metarig_tracks.txt"), std::ios::trunc);
    if (!out.is_open())
    {
        if (aOut) *aOut = -82;
        return;
    }

    auto* animatedObject = FindPlayerAnimatedObjectByComponentName("root");
    if (!animatedObject || !animatedObject->metaRig)
    {
        if (aOut) *aOut = -83;
        return;
    }

    DumpMetaRigTracks(out, animatedObject->metaRig, "");
    out.close();
    if (aOut) *aOut = 1;
}

void DumpRootAnimatedObjectFloatArrayCandidates(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;

    std::ofstream out(VRDiagPath("root_animated_object_float_candidates.txt"), std::ios::trunc);
    if (!out.is_open())
    {
        if (aOut) *aOut = -87;
        return;
    }

    auto* animatedObject = FindPlayerAnimatedObjectByComponentName("root");
    auto* metaRig = animatedObject ? animatedObject->metaRig : nullptr;
    if (!animatedObject || !metaRig)
    {
        if (aOut) *aOut = -88;
        return;
    }

    const uint32_t expectedTrackCount = (metaRig->trackNames.Size() < metaRig->referenceTracks.Size())
        ? metaRig->trackNames.Size()
        : metaRig->referenceTracks.Size();

    out << "animatedObject=0x" << std::hex << reinterpret_cast<uintptr_t>(animatedObject)
        << " metaRig=0x" << reinterpret_cast<uintptr_t>(metaRig)
        << std::dec << " expectedTrackCount=" << expectedTrackCount << "\n\n";

    int32_t candidates = 0;
    uint8_t* base = reinterpret_cast<uint8_t*>(animatedObject);

    auto dumpCandidate = [&](const char* label, size_t ownerOff, uint8_t* ownerBase, size_t off)
    {
        uint64_t entries = SafeReadQword(ownerBase, off + 0x0);
        uint32_t capacity = SafeReadU32(ownerBase, off + 0x8);
        uint32_t size = SafeReadU32(ownerBase, off + 0xC);
        if (std::strcmp(ClassifyQword(entries), "HEAP") != 0)
            return;
        if (size == 0 || size > 4096 || capacity < size || capacity > 8192)
            return;
        if (size != expectedTrackCount && size != expectedTrackCount * 2 && size != expectedTrackCount * 4)
            return;

        out << label << " ownerOff=0x" << std::hex << ownerOff << " arrOff=0x" << off
            << " entries=0x" << entries << std::dec
            << " size=" << size << " capacity=" << capacity << "\n";

        uint8_t* arrBase = reinterpret_cast<uint8_t*>(entries);
        const uint32_t preview = size < 12 ? size : 12;
        for (uint32_t i = 0; i < preview; ++i)
        {
            out << "  [" << i << "]=" << SafeReadFloat(arrBase, i * sizeof(float)) << "\n";
        }
        out << "\n";
        ++candidates;
    };

    for (size_t off = 0; off + 0x10 <= 0x180; off += 4)
        dumpCandidate("direct", off, base, off);

    for (size_t ptrOff = 0; ptrOff + 8 <= 0x180; ptrOff += 8)
    {
        uint64_t p = SafeReadQword(base, ptrOff);
        if (std::strcmp(ClassifyQword(p), "HEAP") != 0)
            continue;

        uint8_t* nested = reinterpret_cast<uint8_t*>(p);
        for (size_t off = 0; off + 0x10 <= 0x200; off += 4)
            dumpCandidate("nested", ptrOff, nested, off);
    }

    out.close();
    if (aOut) *aOut = candidates;
}

// ================================================================================================
// THE GRAPH-SIDE DUMPS, moved here from AnimGraph.cpp where they were interleaved with the code that
// WRITES those variables.
//
// Reading and writing are different jobs with different risks: a dump walks structures it did not
// build and must survive anything, while a write has to be correct about one field. Keeping them in one
// file made every change to either read like a change to both.
// ================================================================================================

void DumpAnimGraphVariables(std::ofstream& aOut, RED4ext::anim::AnimGraph* aGraph, const char* aPrefix)
{
    auto* vars = aGraph ? aGraph->variables.instance : nullptr;
    if (!vars)
    {
        aOut << aPrefix << "graphVariables=<null>\n";
        return;
    }

    aOut << aPrefix << "graphVariables bool=" << vars->boolVariables.Size()
         << " int=" << vars->intVariables.Size()
         << " float=" << vars->floatVariables.Size()
         << " vector=" << vars->vectorVariables.Size()
         << " quat=" << vars->quaternionVariables.Size()
         << " transform=" << vars->transformVariables.Size()
         << "\n";

    for (uint32_t i = 0; i < vars->boolVariables.Size(); ++i)
    {
        auto* v = vars->boolVariables[i].instance;
        const char* name = v ? v->name.ToString() : nullptr;
        if (!v || !IsInterestingAnimName(name))
            continue;
        aOut << aPrefix << "bool[" << i << "] name=" << name
             << " value=" << (v->value ? 1 : 0)
             << " default=" << (v->default_ ? 1 : 0)
             << "\n";
    }

    for (uint32_t i = 0; i < vars->intVariables.Size(); ++i)
    {
        auto* v = vars->intVariables[i].instance;
        const char* name = v ? v->name.ToString() : nullptr;
        if (!v || !IsInterestingAnimName(name))
            continue;
        aOut << aPrefix << "int[" << i << "] name=" << name
             << " value=" << v->value
             << " default=" << v->default_
             << " min=" << v->min
             << " max=" << v->max
             << "\n";
    }

    for (uint32_t i = 0; i < vars->floatVariables.Size(); ++i)
    {
        auto* v = vars->floatVariables[i].instance;
        const char* name = v ? v->name.ToString() : nullptr;
        if (!v || !IsInterestingAnimName(name))
            continue;
        aOut << aPrefix << "float[" << i << "] name=" << name
             << " value=" << v->value
             << " default=" << v->default_
             << " min=" << v->min
             << " max=" << v->max
             << "\n";
    }

    for (uint32_t i = 0; i < vars->vectorVariables.Size(); ++i)
    {
        auto* v = vars->vectorVariables[i].instance;
        const char* name = v ? v->name.ToString() : nullptr;
        if (!v || !IsInterestingAnimName(name))
            continue;
        aOut << aPrefix << "vector[" << i << "] name=" << name
             << " value=(" << v->x << ", " << v->y << ", " << v->z << ", " << v->w << ")"
             << " default=(" << v->default_.X << ", " << v->default_.Y << ", " << v->default_.Z << ", " << v->default_.W << ")"
             << "\n";
    }

    for (uint32_t i = 0; i < vars->quaternionVariables.Size(); ++i)
    {
        auto* v = vars->quaternionVariables[i].instance;
        const char* name = v ? v->name.ToString() : nullptr;
        if (!v || !IsInterestingAnimName(name))
            continue;
        aOut << aPrefix << "quat[" << i << "] name=" << name
             << " value=(roll=" << v->roll << ", pitch=" << v->pitch << ", yaw=" << v->yaw << ")"
             << "\n";
    }

    for (uint32_t i = 0; i < vars->transformVariables.Size(); ++i)
    {
        auto* v = vars->transformVariables[i].instance;
        const char* name = v ? v->name.ToString() : nullptr;
        if (!v || !IsInterestingAnimName(name))
            continue;
        aOut << aPrefix << "transform[" << i << "] name=" << name << "\n";
    }
}

void DumpMetaRigTracks(std::ofstream& aOut, RED4ext::anim::MetaRig* aMetaRig, const char* aPrefix)
{
    if (!aMetaRig)
    {
        aOut << aPrefix << "metaRig=<null>\n";
        return;
    }

    aOut << aPrefix << "metaRig hash=0x" << std::hex << aMetaRig->hash << std::dec
         << " boneCount=" << aMetaRig->boneNames.Size()
         << " trackCount=" << aMetaRig->trackNames.Size()
         << " refTrackCount=" << aMetaRig->referenceTracks.Size()
         << "\n";

    const uint32_t count = (aMetaRig->trackNames.Size() < aMetaRig->referenceTracks.Size())
        ? aMetaRig->trackNames.Size()
        : aMetaRig->referenceTracks.Size();
    for (uint32_t i = 0; i < count; ++i)
    {
        const char* name = aMetaRig->trackNames[i].ToString();
        if (!IsInterestingAnimName(name))
            continue;

        aOut << aPrefix << "track[" << i << "] name=" << (name ? name : "<null>")
             << " value=" << aMetaRig->referenceTracks[i]
             << "\n";
    }
}

#if !CVR_RUST_VRIK
// The C++ build's setters; the Rust build's are rust/crates/cvr-ffi/src/anim_inputs.rs.
bool ResolveRootMetaRigTrackPreset(int32_t aMode, RED4ext::CName& aOut)
{
    switch (aMode)
    {
    case 1:
        aOut = RED4ext::CName("leftArmBodyPartVis");
        return true;
    case 2:
        aOut = RED4ext::CName("rightArmBodyPartVis");
        return true;
    case 3:
        aOut = RED4ext::CName("leftHandWorldSpace");
        return true;
    case 4:
        aOut = RED4ext::CName("rightHandWorldSpace");
        return true;
    case 5:
        aOut = RED4ext::CName("allowFeetIk");
        return true;
    case 6:
        aOut = RED4ext::CName("enableLeftFootIk");
        return true;
    case 7:
        aOut = RED4ext::CName("enableRightFootIk");
        return true;
    case 8:
        aOut = RED4ext::CName("cameraUpOffset");
        return true;
    default:
        return false;
    }
}

void AppendRootMetaRigTrackLog(const char* aSource, const RED4ext::CName& aKey, float aValue, int32_t aResult)
{
    std::ofstream out(VRDiagPath("root_metarig_track_test_log.txt"), std::ios::app);
    if (!out.is_open())
        return;

    out << "source=" << (aSource ? aSource : "<null>")
        << " key=" << aKey.ToString()
        << " value=" << aValue
        << " result=" << aResult
        << "\n";
}

int32_t SetRootMetaRigTrackValue(const RED4ext::CName& aName, float aValue)
{
    auto* animatedObject = FindPlayerAnimatedObjectByComponentName("root");
    auto* metaRig = animatedObject ? animatedObject->metaRig : nullptr;
    if (!metaRig)
        return -81;

    const uint32_t count = (metaRig->trackNames.Size() < metaRig->referenceTracks.Size())
        ? metaRig->trackNames.Size()
        : metaRig->referenceTracks.Size();

    int32_t updated = 0;
    for (uint32_t i = 0; i < count; ++i)
    {
        if (metaRig->trackNames[i] == aName)
        {
            __try
            {
                metaRig->referenceTracks[i] = aValue;
                ++updated;
            }
            __except (EXCEPTION_EXECUTE_HANDLER)
            {
                return -86;
            }
        }
    }

    return updated;
}

// Writes into the LIVE runtime track storage of the root AnimatedObject, not the
// read-only metaRig->referenceTracks resource defaults (which crash on write).
// The live float arrays were discovered by scan:
//   animatedObject+0x8  -> owner+0x40  (DynArray<float>, mirrors track values)
//   animatedObject+0x18 -> owner+0x18  (DynArray<float>, mirrors track values)
// DynArray layout: entries@+0x0, capacity@+0x8, size@+0xC.
// aArrayMode: 0 = both arrays, 1 = 0x8/0x40 only, 2 = 0x18/0x18 only.
int32_t SetRootLiveTrackValue(const RED4ext::CName& aName, float aValue, int32_t aArrayMode)
{
    auto* animatedObject = FindPlayerAnimatedObjectByComponentName("root");
    if (!animatedObject)
        return -90;

    __try
    {
        auto* metaRig = animatedObject->metaRig;
        if (!metaRig || std::strcmp(ClassifyQword(reinterpret_cast<uint64_t>(metaRig)), "HEAP") != 0)
            return -90;

        // Resolve track name -> index from the live trackNames table.
        int32_t trackIndex = -1;
        const uint32_t nameCount = metaRig->trackNames.Size();
        if (nameCount == 0 || nameCount > 8192)
            return -91;
        for (uint32_t i = 0; i < nameCount; ++i)
        {
            if (metaRig->trackNames[i] == aName)
            {
                trackIndex = static_cast<int32_t>(i);
                break;
            }
        }
        if (trackIndex < 0)
            return -91;

        struct LiveArray { size_t ownerOff; size_t arrOff; };
        const LiveArray arrays[2] = { { 0x8, 0x40 }, { 0x18, 0x18 } };

        uint8_t* base = reinterpret_cast<uint8_t*>(animatedObject);
        int32_t writes = 0;

        for (int32_t a = 0; a < 2; ++a)
        {
            if (aArrayMode == 1 && a != 0) continue;
            if (aArrayMode == 2 && a != 1) continue;

            uint64_t owner = SafeReadQword(base, arrays[a].ownerOff);
            if (std::strcmp(ClassifyQword(owner), "HEAP") != 0)
                continue;

            uint8_t* ownerBase = reinterpret_cast<uint8_t*>(owner);
            uint64_t entries = SafeReadQword(ownerBase, arrays[a].arrOff + 0x0);
            uint32_t size = SafeReadU32(ownerBase, arrays[a].arrOff + 0xC);
            if (std::strcmp(ClassifyQword(entries), "HEAP") != 0)
                continue;
            if (static_cast<uint32_t>(trackIndex) >= size)
                continue;

            float* arr = reinterpret_cast<float*>(entries);
            arr[trackIndex] = aValue;
            ++writes;
        }

        return writes;
    }
    __except (EXCEPTION_EXECUTE_HANDLER)
    {
        return -92;
    }
}

// Reads the current LIVE track value (the same arrays SetRootLiveTrackValue writes).
// Returns value*1000 rounded as int so it survives the Int32 return path, or a
// negative sentinel on failure. aArrayMode: 1 = 0x8/0x40, 2 = 0x18/0x18 (default 1).
int32_t ReadRootLiveTrackValue(const RED4ext::CName& aName, int32_t aArrayMode)
{
    auto* animatedObject = FindPlayerAnimatedObjectByComponentName("root");
    if (!animatedObject)
        return -90;

    __try
    {
        auto* metaRig = animatedObject->metaRig;
        if (!metaRig || std::strcmp(ClassifyQword(reinterpret_cast<uint64_t>(metaRig)), "HEAP") != 0)
            return -90;

        int32_t trackIndex = -1;
        const uint32_t nameCount = metaRig->trackNames.Size();
        if (nameCount == 0 || nameCount > 8192)
            return -91;
        for (uint32_t i = 0; i < nameCount; ++i)
        {
            if (metaRig->trackNames[i] == aName) { trackIndex = static_cast<int32_t>(i); break; }
        }
        if (trackIndex < 0)
            return -91;

        const size_t ownerOff = (aArrayMode == 2) ? 0x18 : 0x8;
        const size_t arrOff   = (aArrayMode == 2) ? 0x18 : 0x40;

        uint64_t owner = SafeReadQword(reinterpret_cast<uint8_t*>(animatedObject), ownerOff);
        if (std::strcmp(ClassifyQword(owner), "HEAP") != 0)
            return -92;
        uint8_t* ownerBase = reinterpret_cast<uint8_t*>(owner);
        uint64_t entries = SafeReadQword(ownerBase, arrOff + 0x0);
        uint32_t size = SafeReadU32(ownerBase, arrOff + 0xC);
        if (std::strcmp(ClassifyQword(entries), "HEAP") != 0 || static_cast<uint32_t>(trackIndex) >= size)
            return -92;

        float v = reinterpret_cast<float*>(entries)[trackIndex];
        return static_cast<int32_t>(v * 1000.0f);
    }
    __except (EXCEPTION_EXECUTE_HANDLER)
    {
        return -92;
    }
}

int32_t SetRootGraphFloatVariable(const RED4ext::CName& aName, float aValue)
{
    auto* animated = FindPlayerAnimatedComponentByName("root");
    auto* graph = animated ? LoadResourceRef(animated->graph) : nullptr;
    auto* vars = graph ? graph->variables.instance : nullptr;
    if (!vars)
        return -70;

    int32_t updated = 0;
    for (uint32_t i = 0; i < vars->floatVariables.Size(); ++i)
    {
        auto* v = vars->floatVariables[i].instance;
        if (v && v->name == aName)
        {
            v->value = aValue;
            ++updated;
        }
    }
    return updated;
}

int32_t SetRootGraphBoolVariable(const RED4ext::CName& aName, bool aValue)
{
    auto* animated = FindPlayerAnimatedComponentByName("root");
    auto* graph = animated ? LoadResourceRef(animated->graph) : nullptr;
    auto* vars = graph ? graph->variables.instance : nullptr;
    if (!vars)
        return -71;

    int32_t updated = 0;
    for (uint32_t i = 0; i < vars->boolVariables.Size(); ++i)
    {
        auto* v = vars->boolVariables[i].instance;
        if (v && v->name == aName)
        {
            v->value = aValue;
            ++updated;
        }
    }
    return updated;
}

int32_t SetRootGraphVectorVariable(const RED4ext::CName& aName, const RED4ext::Vector4& aValue)
{
    auto* animated = FindPlayerAnimatedComponentByName("root");
    auto* graph = animated ? LoadResourceRef(animated->graph) : nullptr;
    auto* vars = graph ? graph->variables.instance : nullptr;
    if (!vars)
        return -72;

    int32_t updated = 0;
    for (uint32_t i = 0; i < vars->vectorVariables.Size(); ++i)
    {
        auto* v = vars->vectorVariables[i].instance;
        if (v && v->name == aName)
        {
            v->x = aValue.X;
            v->y = aValue.Y;
            v->z = aValue.Z;
            v->w = aValue.W;
            ++updated;
        }
    }
    return updated;
}

#endif

static void DumpAnimatedResourceDetails(std::ofstream& aOut, RED4ext::ent::AnimatedComponent* aAnimated, const char* aPrefix)
{
    if (!aAnimated)
        return;

    auto* graph = LoadResourceRef(aAnimated->graph);
    if (graph)
    {
        aOut << aPrefix << "graphLoaded=1 animFeatures=" << graph->animFeatures.Size()
             << " additionalAnimDatabases=" << graph->additionalAnimDatabases.Size()
             << " useAnimCommands=" << (graph->useAnimCommands ? 1 : 0)
             << " hasMixerSlot=" << (graph->hasMixerSlot ? 1 : 0)
             << "\n";
        DumpAnimGraphVariables(aOut, graph, aPrefix);

        for (uint32_t i = 0; i < graph->animFeatures.Size(); ++i)
        {
            const auto& feature = graph->animFeatures[i];
            const char* name = feature.name.ToString();
            const char* className = feature.className.ToString();
            aOut << aPrefix << "feature[" << i << "] name=" << (name ? name : "<null>")
                 << " class=" << (className ? className : "<null>")
                 << " forceAllocate=" << (feature.forceAllocate ? 1 : 0)
                 << "\n";
        }

        for (uint32_t i = 0; i < graph->additionalAnimDatabases.Size(); ++i)
        {
            const auto& db = graph->additionalAnimDatabases[i];
            aOut << aPrefix << "animDb[" << i << "] name=" << db.name.ToString()
                 << " dbHash=0x" << std::hex << db.animDatabase.path.hash
                 << " overrideHash=0x" << db.overrideAnimDatabase.path.hash
                 << std::dec << "\n";
        }
    }
    else
    {
        aOut << aPrefix << "graphLoaded=0\n";
    }

    auto* rig = LoadResourceRef(aAnimated->rig);
    if (rig)
    {
        aOut << aPrefix << "rigLoaded=1 boneCount=" << rig->boneNames.Size()
             << " trackCount=" << rig->trackNames.Size()
             << " ikSetups=" << rig->ikSetups.Size()
             << "\n";
        DumpNameListFiltered(aOut, "    rig.trackNames", rig->trackNames);
        DumpNameListFiltered(aOut, "    rig.boneNames", rig->boneNames);
    }
    else
    {
        aOut << aPrefix << "rigLoaded=0\n";
    }

    aOut << aPrefix << "setup.gameplay count=" << aAnimated->animations.gameplay.Size() << "\n";
    for (uint32_t i = 0; i < aAnimated->animations.gameplay.Size(); ++i)
    {
        const auto& entry = aAnimated->animations.gameplay[i];
        aOut << aPrefix << "gameplay[" << i << "] animSetHash=0x" << std::hex << entry.animSet.path.hash << std::dec
             << " priority=" << static_cast<int32_t>(entry.priority)
             << " varCount=" << entry.variableNames.Size() << "\n";
        DumpNameListFiltered(aOut, "      gameplay.variables", entry.variableNames);
    }

    aOut << aPrefix << "setup.cinematics count=" << aAnimated->animations.cinematics.Size() << "\n";
    for (uint32_t i = 0; i < aAnimated->animations.cinematics.Size(); ++i)
    {
        const auto& entry = aAnimated->animations.cinematics[i];
        aOut << aPrefix << "cinematics[" << i << "] animSetHash=0x" << std::hex << entry.animSet.path.hash << std::dec
             << " priority=" << static_cast<int32_t>(entry.priority)
             << " varCount=" << entry.variableNames.Size() << "\n";
        DumpNameListFiltered(aOut, "      cinematic.variables", entry.variableNames);
    }
}

void DumpEntityAnimationInfo(std::ofstream& aOut, RED4ext::ent::Entity* aEntity, const char* aReason)
{
    if (!aEntity)
        return;

    aOut << "ENTITY reason=" << (aReason ? aReason : "<null>")
         << " ptr=0x" << std::hex << reinterpret_cast<uintptr_t>(aEntity) << std::dec
         << " appearance=" << aEntity->appearanceName.ToString()
         << " templateHash=0x" << std::hex << aEntity->templatePath.hash << std::dec
         << " status=" << static_cast<int32_t>(aEntity->status)
         << "\n";

    for (auto& componentHandle : aEntity->components)
    {
        auto* component = componentHandle.instance;
        if (!component)
            continue;

        auto* type = component->GetType();
        if (!type)
            continue;

        const bool isController = (type->name == "entAnimationControllerComponent");
        const bool isAnimated = (type->name == "entAnimatedComponent");
        if (!isController && !isAnimated)
            continue;

        aOut << "  component name=" << component->name.ToString()
             << " type=" << type->name.ToString()
             << " ptr=0x" << std::hex << reinterpret_cast<uintptr_t>(component) << std::dec
             << " enabled=" << (component->isEnabled ? 1 : 0)
             << (IsLikelyFppArmComponent(component->name.ToString()) ? " [LIKELY_FPP_ARM]" : "")
             << "\n";

        if (isController)
        {
            auto* controller = reinterpret_cast<RED4ext::ent::AnimationControllerComponent*>(component);
            aOut << "    controlBinding=0x" << std::hex << reinterpret_cast<uintptr_t>(controller->controlBinding.instance)
                 << " ikTargets=" << std::dec << controller->ikTargetController.targetData.Size()
                 << " ikParams=0x" << std::hex << reinterpret_cast<uintptr_t>(controller->ikTargetController.ikParams.instance)
                 << std::dec << "\n";
            DumpBindingInfo(aOut, controller->controlBinding.instance, "    ");
        }
        else
        {
            auto* animated = reinterpret_cast<RED4ext::ent::AnimatedComponent*>(component);
            aOut << "    rigHash=0x" << std::hex << animated->rig.path.hash
                 << " graphHash=0x" << animated->graph.path.hash
                 << " controlBinding=0x" << reinterpret_cast<uintptr_t>(animated->controlBinding.instance)
                 << std::dec << " animParams=" << animated->animParameters.Size()
                 << "\n";
            DumpBindingInfo(aOut, animated->controlBinding.instance, "    ");
            DumpAnimTrackParameters(aOut, animated, "    ");
            DumpAnimatedResourceDetails(aOut, animated, "    ");
        }
    }

    aOut << "\n";
}
#endif
#endif
