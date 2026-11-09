// AnimLookup -- finding the objects every other animation native operates on.
//
// Nothing here changes anything. It answers "where is the player's animation controller", "which
// AnimatedObject belongs to this entity", "what animated components does the player actually have" --
// and it exists as a file because those answers were HARD to get and every one of them is reached by a
// different route.
//
// The routes are not interchangeable and the comments in each say why: an AnimatedObject is not
// reachable from the entity by a property, the world AnimationSystem is not in the RTTI type registry
// under a findable name, and the animation controller component is identified by its class rather than
// by a slot. A caller that picks the wrong one gets a null and no explanation, which is why these are
// named after what they find rather than after how they find it.

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

RED4ext::ent::AnimationControllerComponent* FindPlayerAnimationController()
{
    RED4ext::ScriptGameInstance gameInstance;
    RED4ext::Handle<RED4ext::IScriptable> playerHandle;
    RED4ext::ExecuteGlobalFunction("GetPlayer;GameInstance", &playerHandle, gameInstance);
    if (!playerHandle)
        return nullptr;

    auto* playerEntity = reinterpret_cast<RED4ext::ent::Entity*>(playerHandle.instance);
    if (!playerEntity)
        return nullptr;

    for (auto& componentHandle : playerEntity->components)
    {
        auto* component = componentHandle.instance;
        if (!component)
            continue;
        RED4ext::CClass* type = component->GetType();
        if (type && type->name == "entAnimationControllerComponent")
            return reinterpret_cast<RED4ext::ent::AnimationControllerComponent*>(component);
    }
    return nullptr;
}

RED4ext::ent::Entity* FindPlayerEntity()
{
    RED4ext::ScriptGameInstance gameInstance;
    RED4ext::Handle<RED4ext::IScriptable> playerHandle;
    RED4ext::ExecuteGlobalFunction("GetPlayer;GameInstance", &playerHandle, gameInstance);
    if (!playerHandle)
        return nullptr;

    return reinterpret_cast<RED4ext::ent::Entity*>(playerHandle.instance);
}

// RESTORED. An earlier pass of mine removed the wrong template from this file: it took the FIRST
// `template<typename T>` it found, which is this one. LoadResourceRef is the one that went to
// Natives/AnimInternal.hpp.
template<typename T>
static T* GetGameSystem(const char* aClassName)
{
    auto* engine = RED4ext::CGameEngine::Get();
    auto* framework = engine ? engine->framework : nullptr;
    auto* gameInstance = framework ? framework->gameInstance : nullptr;
    auto* rtti = RED4ext::CRTTISystem::Get();
    auto* cls = rtti ? rtti->GetClass(aClassName) : nullptr;
    if (!gameInstance || !cls)
        return nullptr;

    return reinterpret_cast<T*>(gameInstance->GetSystem(cls));
}

static RED4ext::world::AnimationSystem* GetWorldAnimationSystem()
{
    auto* iface = GetGameSystem<RED4ext::world::AnimationSystemScriptInterface>("worldAnimationSystemScriptInterface");
    return iface ? iface->animationSystem : nullptr;
}

// These candidates come from a known engine chain or the cached system, not a memory scan.
// Windows can allocate them below 1 TiB too: validate memory rather than assuming an ASLR range.
static bool IsValidAnimationSystemPtr(void* aPtr)
{
    if (!VRIK_IsReadable(aPtr, sizeof(void*)))
        return false;
    auto* type = SafeGetObjectType(aPtr);
    return type && type->name == "worldAnimationSystem";
}

// Fast, safe path: the worldAnimationSystem consistently sits at the fixed
// chain runtimeScene+0x8 -> +0x38 (confirmed across runs in the lookup dumps).
// This avoids the full memory scan that calls GetType() on hundreds of
// arbitrary pointers and intermittently crashes. Only one GetType() call here.
static RED4ext::world::AnimationSystem* TryKnownAnimationSystemChain(RED4ext::world::RuntimeScene* aRuntimeScene)
{
    if (!VRIK_IsReadable(aRuntimeScene, 0x10))
        return nullptr;

    uint64_t parent = SafeReadQword(reinterpret_cast<uint8_t*>(aRuntimeScene), 0x8);
    if (!VRIK_IsReadable(reinterpret_cast<void*>(parent), 0x40))
        return nullptr;

    uint64_t cand = SafeReadQword(reinterpret_cast<uint8_t*>(parent), 0x38);
    if (IsValidAnimationSystemPtr(reinterpret_cast<void*>(cand)))
        return reinterpret_cast<RED4ext::world::AnimationSystem*>(cand);

    return nullptr;
}

RED4ext::world::AnimationSystem* FindWorldAnimationSystemFromScene(RED4ext::world::RuntimeScene* aRuntimeScene,
                                                                          uintptr_t aFrameworkScene,
                                                                          std::ofstream* aOut)
{
    // Reuse a previously discovered system if it still looks valid. This skips
    // the expensive/fragile memory scan on every call.
    if (g_cachedAnimationSystem && IsValidAnimationSystemPtr(g_cachedAnimationSystem))
        return g_cachedAnimationSystem;
    g_cachedAnimationSystem = nullptr;

    if (auto* sys = GetWorldAnimationSystem())
    {
        g_cachedAnimationSystem = sys;
        return sys;
    }

    // Fast direct chain first (cheap + safe), scan only as last resort.
    if (auto* sys = TryKnownAnimationSystemChain(aRuntimeScene))
    {
        if (aOut)
            *aOut << "found via fixed chain runtimeScene+0x8->+0x38\n";
        g_cachedAnimationSystem = sys;
        return sys;
    }

    if (aOut)
        *aOut << "GetWorldAnimationSystem() returned null, scanning runtime scene memory...\n";

    if (auto* sys = ScanForAnimationSystemInBlock(reinterpret_cast<uint8_t*>(aRuntimeScene), 0x4B8, aOut))
    {
        g_cachedAnimationSystem = sys;
        return sys;
    }

    if (aFrameworkScene)
    {
        if (auto* sys = ScanForAnimationSystemInBlock(reinterpret_cast<uint8_t*>(aFrameworkScene), 0x800, aOut))
        {
            g_cachedAnimationSystem = sys;
            return sys;
        }
    }

    return nullptr;
}

// The same walk, for any entity that has an animated component -- a weapon in hand is in these buckets too.
RED4ext::anim::AnimatedObject* FindAnimatedObjectForEntity(RED4ext::ent::Entity* aEntity,
                                                                 const char* aComponentName,
                                                                 bool aAnyComponent)
{
    auto* engine = RED4ext::CGameEngine::Get();
    auto* framework = engine ? engine->framework : nullptr;
    auto* animationSystem = aEntity
        ? FindWorldAnimationSystemFromScene(aEntity->runtimeScene, framework ? framework->unk18 : 0, nullptr)
        : nullptr;
    if (!aEntity || !animationSystem) return nullptr;

    for (uint32_t bucketIndex = 0; bucketIndex < RED4ext::world::AnimationSystem::BucketCount; ++bucketIndex)
    {
        __try
        {
            auto& bucket = animationSystem->entitityBuckets[bucketIndex];
            const uint32_t entryCount = bucket.entities.Size();
            for (uint32_t entryIndex = 0; entryIndex < entryCount; ++entryIndex)
            {
                auto* entity = reinterpret_cast<RED4ext::ent::Entity*>(bucket.entities[entryIndex].instance);
                if (entity != aEntity) continue;
                if (aAnyComponent) return bucket.animatedObjects[entryIndex];
                auto* animatedComponent = bucket.animatedComponents[entryIndex];
                const char* name = animatedComponent ? animatedComponent->name.ToString() : nullptr;
                if (name && aComponentName && std::strcmp(name, aComponentName) == 0)
                    return bucket.animatedObjects[entryIndex];
            }
        }
        __except (EXCEPTION_EXECUTE_HANDLER) {}
    }
    return nullptr;
}

// Lists every animated component an entity owns, into a buffer, and returns how many there were. A weapon in
// hand is attached to the player, so its rig may well be registered under the PLAYER's entity -- this is what
// tells us instead of leaving it to a guess.
int ListAnimatedComponents(RED4ext::ent::Entity* aEntity, char* aOut, size_t aCap,
                                  RED4ext::anim::AnimatedObject** aFirstNonRoot)
{
    if (aOut && aCap) aOut[0] = 0;
    if (aFirstNonRoot) *aFirstNonRoot = nullptr;
    auto* engine = RED4ext::CGameEngine::Get();
    auto* framework = engine ? engine->framework : nullptr;
    auto* animationSystem = aEntity
        ? FindWorldAnimationSystemFromScene(aEntity->runtimeScene, framework ? framework->unk18 : 0, nullptr)
        : nullptr;
    if (!aEntity || !animationSystem) return -1;

    int count = 0;
    size_t used = 0;
    for (uint32_t bucketIndex = 0; bucketIndex < RED4ext::world::AnimationSystem::BucketCount; ++bucketIndex)
    {
        __try
        {
            auto& bucket = animationSystem->entitityBuckets[bucketIndex];
            const uint32_t entryCount = bucket.entities.Size();
            for (uint32_t entryIndex = 0; entryIndex < entryCount; ++entryIndex)
            {
                auto* entity = reinterpret_cast<RED4ext::ent::Entity*>(bucket.entities[entryIndex].instance);
                if (entity != aEntity) continue;
                ++count;
                auto* comp = bucket.animatedComponents[entryIndex];
                const char* name = comp ? comp->name.ToString() : nullptr;
                if (!name) name = "?";
                if (aOut && used + 2 < aCap) {
                    const size_t n = std::strlen(name);
                    if (used) aOut[used++] = ' ';
                    const size_t room = (aCap - 1) - used;
                    const size_t take = (n < room) ? n : room;
                    std::memcpy(aOut + used, name, take);
                    used += take;
                    aOut[used] = 0;
                }
                if (aFirstNonRoot && !*aFirstNonRoot && std::strcmp(name, "root") != 0)
                    *aFirstNonRoot = bucket.animatedObjects[entryIndex];
            }
        }
        __except (EXCEPTION_EXECUTE_HANDLER) {}
    }
    return count;
}

RED4ext::anim::AnimatedObject* FindPlayerAnimatedObjectByComponentName(const char* aComponentName)
{
    auto* playerEntity = FindPlayerEntity();
    auto* engine = RED4ext::CGameEngine::Get();
    auto* framework = engine ? engine->framework : nullptr;
    auto* animationSystem = playerEntity
        ? FindWorldAnimationSystemFromScene(playerEntity->runtimeScene, framework ? framework->unk18 : 0, nullptr)
        : nullptr;
    if (!playerEntity || !animationSystem || !aComponentName)
        return nullptr;

    // Validate the live bucket entry rather than keeping a raw AnimatedObject
    // alive across equipment/load changes. The root binding can now be checked
    // every game tick without walking every animated entity in the scene.
    static uint32_t rootBucket=0,rootEntry=0;
    const bool root=std::strcmp(aComponentName,"root")==0;
    if(root) {
        __try {
            auto& bucket=animationSystem->entitityBuckets[rootBucket];
            if(rootEntry<bucket.entities.Size() && rootEntry<bucket.animatedComponents.Size() &&
               rootEntry<bucket.animatedObjects.Size() && bucket.entities[rootEntry].instance==playerEntity) {
                auto* component=bucket.animatedComponents[rootEntry];
                if(component && component->name==RED4ext::CName("root"))return bucket.animatedObjects[rootEntry];
            }
        } __except(EXCEPTION_EXECUTE_HANDLER) {}
    }

    for (uint32_t bucketIndex = 0; bucketIndex < RED4ext::world::AnimationSystem::BucketCount; ++bucketIndex)
    {
        __try
        {
            auto& bucket = animationSystem->entitityBuckets[bucketIndex];
            const uint32_t entryCount = bucket.entities.Size();
            for (uint32_t entryIndex = 0; entryIndex < entryCount; ++entryIndex)
            {
                auto* entity = reinterpret_cast<RED4ext::ent::Entity*>(bucket.entities[entryIndex].instance);
                if (entity != playerEntity)
                    continue;

                auto* animatedComponent = bucket.animatedComponents[entryIndex];
                const char* name = animatedComponent ? animatedComponent->name.ToString() : nullptr;
                if (name && std::strcmp(name, aComponentName) == 0) {
                    if(root){rootBucket=bucketIndex;rootEntry=entryIndex;}
                    return bucket.animatedObjects[entryIndex];
                }
            }
        }
        __except (EXCEPTION_EXECUTE_HANDLER)
        {
        }
    }

    return nullptr;
}

void DumpAnimationSystemLookup(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;

    std::ofstream out(VRDiagPath("animation_system_lookup.txt"), std::ios::trunc);
    if (!out.is_open())
    {
        if (aOut) *aOut = -1;
        return;
    }

    auto* playerEntity = FindPlayerEntity();
    auto* engine = RED4ext::CGameEngine::Get();
    auto* framework = engine ? engine->framework : nullptr;
    auto* gameInstance = framework ? framework->gameInstance : nullptr;
    auto* rtti = RED4ext::CRTTISystem::Get();

    out << "playerEntity=0x" << std::hex << reinterpret_cast<uintptr_t>(playerEntity)
        << " runtimeScene=0x" << reinterpret_cast<uintptr_t>(playerEntity ? playerEntity->runtimeScene : nullptr)
        << "\n";
    out << "engine=0x" << reinterpret_cast<uintptr_t>(engine)
        << " framework=0x" << reinterpret_cast<uintptr_t>(framework)
        << " gameInstance=0x" << reinterpret_cast<uintptr_t>(gameInstance)
        << " worldSceneFromFramework=0x" << static_cast<uintptr_t>(framework ? framework->unk18 : 0)
        << std::dec << "\n\n";

    const char* names[] = {
        "worldAnimationSystem",
        "worldAnimationSystemScriptInterface",
        "AnimationSystem",
        "worldRuntimeScene"
    };

    for (const char* name : names)
    {
        auto* nativeCls = rtti ? rtti->GetClass(name) : nullptr;
        auto* scriptCls = rtti ? rtti->GetClassByScriptName(name) : nullptr;
        auto* nativeSys = (gameInstance && nativeCls) ? gameInstance->GetSystem(nativeCls) : nullptr;
        auto* scriptSys = (gameInstance && scriptCls) ? gameInstance->GetSystem(scriptCls) : nullptr;

        out << "name=" << name << "\n";
        out << "  nativeCls=0x" << std::hex << reinterpret_cast<uintptr_t>(nativeCls)
            << " scriptCls=0x" << reinterpret_cast<uintptr_t>(scriptCls) << std::dec << "\n";
        out << "  nativeClsName=" << (nativeCls ? nativeCls->name.ToString() : "<null>")
            << " scriptClsName=" << (scriptCls ? scriptCls->name.ToString() : "<null>") << "\n";
        out << "  nativeSystem=0x" << std::hex << reinterpret_cast<uintptr_t>(nativeSys)
            << " scriptSystem=0x" << reinterpret_cast<uintptr_t>(scriptSys) << std::dec << "\n\n";
    }

    auto* scanned = FindWorldAnimationSystemFromScene(playerEntity ? playerEntity->runtimeScene : nullptr,
                                                      framework ? framework->unk18 : 0,
                                                      &out);
    out << "scannedAnimationSystem=0x" << std::hex << reinterpret_cast<uintptr_t>(scanned) << std::dec << "\n";

    if (aOut) *aOut = 1;
}
