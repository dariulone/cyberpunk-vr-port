#pragma once

// ================================================================================================
// What the three animation-native files hand each other.
//
// AnimGraph.cpp writes graph variables, AnimLookup.cpp finds the objects to write them on, and
// AnimDumps.cpp writes down what those objects contain. All three need the lookups; the dumps need a
// few of each other's helpers.
//
// Declarators are copied from the definitions, never retyped -- see include/Anim/VrikHook.hpp for what
// retyping one costs.
// ================================================================================================

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
#include "Natives/NativeHelpers.hpp"
#include <MinHook.h>
#include "Natives/NativeFunctions.hpp"
#include "Natives/NativeHelpers.hpp"
#include "Natives/NativeState.hpp"

// A TEMPLATE, so the definition and not a declaration: it has to be visible where it is
// instantiated, and both .cpp files that used it instantiate it with different types.
template<typename T>
T* LoadResourceRef(RED4ext::Ref<T>& aRef)
{
    if (aRef.path.IsEmpty())
        return nullptr;

    if (!aRef.IsLoaded())
    {
        if (!aRef.Load())
            return nullptr;
    }

    auto& handle = aRef.Get();
    return handle.instance;
}
RED4ext::Handle<RED4ext::ent::IOrientationProvider> CreateStaticOrientationProvider();
RED4ext::Handle<RED4ext::ent::IPositionProvider> CreateStaticPositionProvider(const RED4ext::Vector4& aPosition);
RED4ext::ent::AnimatedComponent* FindPlayerAnimatedComponentByName(const char* aName);
RED4ext::ent::AnimationControllerComponent* FindPlayerAnimationController();
RED4ext::world::AnimationSystem* FindWorldAnimationSystemFromScene(RED4ext::world::RuntimeScene* aRuntimeScene, uintptr_t aFrameworkScene, std::ofstream* aOut = nullptr);
void AppendPlayerControllerIKState(std::ostream& aOut);
void DumpAnimTrackParameters(std::ofstream& aOut, RED4ext::ent::AnimatedComponent* aAnimated, const char* aPrefix);
void DumpBindingInfo(std::ofstream& aOut, RED4ext::ent::AnimationControlBinding* aBinding, const char* aPrefix);
void DumpEntityAnimationInfo(std::ofstream& aOut, RED4ext::ent::Entity* aEntity, const char* aReason);
void DumpMetaRigTracks(std::ofstream& aOut, RED4ext::anim::MetaRig* aMetaRig, const char* aPrefix);
void DumpNameListFiltered(std::ofstream& aOut, const char* aLabel, const RED4ext::DynArray<RED4ext::CName>& aNames);
bool FillAnimTestPose(int32_t aMode, RED4ext::Vector4& aLeft, RED4ext::Vector4& aRight);
bool IsInterestingAnimName(const char* aName);
bool QueuePlayerEvent(const RED4ext::Handle<RED4ext::red::Event>& aEvent);
bool ResolveAnimFloatPreset(int32_t aMode, RED4ext::CName& aOut);
bool ResolveRootGraphFloatPreset(int32_t aMode, RED4ext::CName& aOut);
bool ResolveRootGraphVectorPreset(int32_t aMode, RED4ext::CName& aOut);
bool ResolveRootMetaRigTrackPreset(int32_t aMode, RED4ext::CName& aOut);
int32_t ApplyWeaponUserFeature(RED4ext::ent::AnimationControllerComponent* aController, const RED4ext::CName& aFeatureName, const RED4ext::Vector4& aLeft, const RED4ext::Vector4& aRight);
int32_t ForceVRNeutralAnimGraphInputs();
int32_t QueueFeatureInputEvent(const RED4ext::CName& aFeatureName, const RED4ext::Handle<RED4ext::anim::AnimFeature>& aFeature);
int32_t QueueFloatInputEvent(const RED4ext::CName& aKey, float aValue);
int32_t QueueVectorInputEvents(const RED4ext::Vector4& aLeft, const RED4ext::Vector4& aRight);
int32_t ReadRootLiveTrackValue(const RED4ext::CName& aName, int32_t aArrayMode);
int32_t SetFloatInputDirect(const RED4ext::CName& aKey, float aValue);
int32_t SetPlayerAnimatedParameterValue(const RED4ext::CName& aKey, float aValue);
int32_t SetRootGraphBoolVariable(const RED4ext::CName& aName, bool aValue);
int32_t SetRootGraphFloatVariable(const RED4ext::CName& aName, float aValue);
int32_t SetRootGraphVectorVariable(const RED4ext::CName& aName, const RED4ext::Vector4& aValue);
int32_t SetRootLiveTrackValue(const RED4ext::CName& aName, float aValue, int32_t aArrayMode);
int32_t SetRootMetaRigTrackValue(const RED4ext::CName& aName, float aValue);
void AppendAnimFloatTestLog(const char* aRouteName, const RED4ext::CName& aKey, float aValue, int32_t aResult);
void AppendAnimTestLog(const char* aModeName, int32_t aResult, const RED4ext::Vector4& aLeft, const RED4ext::Vector4& aRight);
void AppendDirectAnimParamLog(const char* aSource, const RED4ext::CName& aKey, float aValue, int32_t aResult);
void AppendRootGraphVariableLog(const char* aSource, const RED4ext::CName& aKey, const char* aValueText, int32_t aResult);
void AppendRootMetaRigTrackLog(const char* aSource, const RED4ext::CName& aKey, float aValue, int32_t aResult);
void DumpAnimGraphVariables(std::ofstream& aOut, RED4ext::anim::AnimGraph* aGraph, const char* aPrefix);
// The three-argument OVERLOAD. The generator declared only the two-argument one, so the call sites
// in AnimGraph.cpp resolved against it and failed on arity.
int32_t QueueFeatureInputEvent(const RED4ext::CName& aFeatureName, const RED4ext::Vector4& aLeft, const RED4ext::Vector4& aRight);
