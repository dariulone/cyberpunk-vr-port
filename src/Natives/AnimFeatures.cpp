#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
// AnimFeatures -- building an anim feature, and getting the engine to accept it.
//
// An anim feature is how a value reaches the animation graph from outside: you construct a Handle of the
// right RTTI class, fill it, and hand it to the AnimationControllerComponent. Every part of that is a
// place to be wrong, and the three Test*Route natives exist because being wrong is silent -- the feature
// is accepted and nothing moves.
//
// THE Handle REFCOUNT IS THE TRAP. A Handle constructed and dropped without being installed takes the
// feature with it, and a Handle installed twice leaves a count that never reaches zero. ApplyFeatureHandle
// is the single place that install happens, so there is one place to read when the arms stop following.
//
// The IK feature is the one that matters in play: it turns the engine own Ik2 on, and WORLD-space
// providers aim it. The route tests here are what established that -- not the RTTI dump, which lists the
// class and says nothing about whether the engine will act on it.

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
// The C++ build's; the Rust build's features and routes are rust/crates/cvr-ffi/src/anim_inputs.rs.
static RED4ext::Handle<RED4ext::anim::AnimFeature> CreateWeaponUserFeature(const RED4ext::Vector4& aLeft,
                                                                           const RED4ext::Vector4& aRight)
{
    auto* rtti = RED4ext::CRTTISystem::Get();
    auto* cls = rtti ? rtti->GetClass("animAnimFeature_WeaponUser") : nullptr;
    if (!cls)
        return {};

    auto* instance = cls->CreateInstance(true);
    if (!instance)
        return {};

    cls->InitializeProperties(instance);

    auto* feature = reinterpret_cast<RED4ext::anim::AnimFeature_WeaponUser*>(instance);
    feature->ikLeftHandLocalPosition = aLeft;
    feature->ikRightHandLocalPosition = aRight;

    return RED4ext::Handle<RED4ext::anim::AnimFeature>(reinterpret_cast<RED4ext::anim::AnimFeature*>(feature));
}

static RED4ext::Handle<RED4ext::anim::AnimFeature> CreateIKFeature(const RED4ext::Vector4& aPoint,
                                                                   const RED4ext::Vector4& aNormal,
                                                                   float aWeight)
{
    auto* rtti = RED4ext::CRTTISystem::Get();
    auto* cls = rtti ? rtti->GetClass("animAnimFeature_IK") : nullptr;
    if (!cls)
        return {};

    auto* instance = cls->CreateInstance(true);
    if (!instance)
        return {};

    cls->InitializeProperties(instance);

    auto* feature = reinterpret_cast<RED4ext::anim::AnimFeature_IK*>(instance);
    feature->point = aPoint;
    feature->normal = aNormal;
    feature->weight = aWeight;
    return RED4ext::Handle<RED4ext::anim::AnimFeature>(reinterpret_cast<RED4ext::anim::AnimFeature*>(feature));
}

static RED4ext::Handle<RED4ext::anim::AnimFeature> CreateMeleeIKDataFeature(const RED4ext::Vector4& aHead,
                                                                             const RED4ext::Vector4& aChest,
                                                                             const RED4ext::Vector4& aOffset)
{
    auto* rtti = RED4ext::CRTTISystem::Get();
    auto* cls = rtti ? rtti->GetClass("animAnimFeature_MeleeIKData") : nullptr;
    if (!cls)
        return {};

    auto* instance = cls->CreateInstance(true);
    if (!instance)
        return {};

    cls->InitializeProperties(instance);

    auto* feature = reinterpret_cast<RED4ext::anim::AnimFeature_MeleeIKData*>(instance);
    feature->isValid = true;
    feature->headPosition = aHead;
    feature->chestPosition = aChest;
    feature->ikOffset = aOffset;
    return RED4ext::Handle<RED4ext::anim::AnimFeature>(reinterpret_cast<RED4ext::anim::AnimFeature*>(feature));
}

static RED4ext::Handle<RED4ext::ent::AnimInputSetterVector> CreateVectorInputEvent(const RED4ext::CName& aKey,
                                                                                     const RED4ext::Vector4& aValue)
{
    auto* rtti = RED4ext::CRTTISystem::Get();
    auto* cls = rtti ? rtti->GetClass("entAnimInputSetterVector") : nullptr;
    if (!cls)
        return {};

    auto* instance = cls->CreateInstance(true);
    if (!instance)
        return {};

    cls->InitializeProperties(instance);

    auto* evt = reinterpret_cast<RED4ext::ent::AnimInputSetterVector*>(instance);
    evt->key = aKey;
    evt->value = aValue;
    return RED4ext::Handle<RED4ext::ent::AnimInputSetterVector>(evt);
}

static RED4ext::Handle<RED4ext::ent::AnimInputSetterFloat> CreateFloatInputEvent(const RED4ext::CName& aKey,
                                                                                  float aValue)
{
    auto* rtti = RED4ext::CRTTISystem::Get();
    auto* cls = rtti ? rtti->GetClass("entAnimInputSetterFloat") : nullptr;
    if (!cls)
        return {};

    auto* instance = cls->CreateInstance(true);
    if (!instance)
        return {};

    cls->InitializeProperties(instance);

    auto* evt = reinterpret_cast<RED4ext::ent::AnimInputSetterFloat*>(instance);
    evt->key = aKey;
    evt->value = aValue;
    return RED4ext::Handle<RED4ext::ent::AnimInputSetterFloat>(evt);
}

static RED4ext::Handle<RED4ext::ent::AnimInputSetterAnimFeature> CreateFeatureInputEvent(const RED4ext::CName& aKey,
                                                                                           const RED4ext::Handle<RED4ext::anim::AnimFeature>& aFeature)
{
    if (!aFeature)
        return {};

    auto* rtti = RED4ext::CRTTISystem::Get();
    auto* cls = rtti ? rtti->GetClass("entAnimInputSetterAnimFeature") : nullptr;
    if (!cls)
        return {};

    auto* instance = cls->CreateInstance(true);
    if (!instance)
        return {};

    cls->InitializeProperties(instance);

    auto* evt = reinterpret_cast<RED4ext::ent::AnimInputSetterAnimFeature*>(instance);
    evt->key = aKey;
    evt->delay = 0.0f;
    evt->value = aFeature;
    return RED4ext::Handle<RED4ext::ent::AnimInputSetterAnimFeature>(evt);
}

static RED4ext::Handle<RED4ext::ent::AnimInputSetterAnimFeature> CreateFeatureInputEvent(const RED4ext::CName& aKey,
                                                                                           const RED4ext::Vector4& aLeft,
                                                                                           const RED4ext::Vector4& aRight)
{
    auto feature = CreateWeaponUserFeature(aLeft, aRight);
    return CreateFeatureInputEvent(aKey, feature);
}

bool QueuePlayerEvent(const RED4ext::Handle<RED4ext::red::Event>& aEvent)
{
    auto* playerEntity = FindPlayerEntity();
    if (!playerEntity)
        return false;

    auto* cls = playerEntity->GetType();
    auto* func = cls ? cls->GetFunction("QueueEvent") : nullptr;
    if (!func)
        return false;

    RED4ext::StackArgs_t args;
    args.emplace_back(nullptr, const_cast<RED4ext::Handle<RED4ext::red::Event>*>(&aEvent));
    return RED4ext::ExecuteFunction(playerEntity, func, nullptr, args);
}

int32_t SetFloatInputDirect(const RED4ext::CName& aKey, float aValue)
{
    auto* controller = FindPlayerAnimationController();
    if (!controller)
        return -40;

    auto* cls = controller->GetType();
    auto* func = cls ? cls->GetFunction("SetInputFloat") : nullptr;
    if (!func)
        return -41;

    RED4ext::StackArgs_t args;
    args.emplace_back(nullptr, const_cast<RED4ext::CName*>(&aKey));
    args.emplace_back(nullptr, &aValue);
    return RED4ext::ExecuteFunction(controller, func, nullptr, args) ? 10 : -42;
}

int32_t QueueFloatInputEvent(const RED4ext::CName& aKey, float aValue)
{
    auto evt = CreateFloatInputEvent(aKey, aValue);
    if (!evt)
        return -43;

    return QueuePlayerEvent(static_cast<RED4ext::Handle<RED4ext::red::Event>>(evt)) ? 11 : -44;
}

void AppendAnimFloatTestLog(const char* aRouteName, const RED4ext::CName& aKey, float aValue, int32_t aResult)
{
    std::ofstream out(VRDiagPath("anim_float_input_test_log.txt"), std::ios::app);
    if (!out.is_open())
        return;

    out << "route=" << (aRouteName ? aRouteName : "<null>")
        << " key=" << aKey.ToString()
        << " value=" << aValue
        << " result=" << aResult
        << "\n";
}

bool ResolveAnimFloatPreset(int32_t aMode, RED4ext::CName& aOut)
{
    switch (aMode)
    {
    case 1:
        aOut = RED4ext::CName("visibilityLeftArm");
        return true;
    case 2:
        aOut = RED4ext::CName("visibilityRightArm");
        return true;
    case 3:
        aOut = RED4ext::CName("renderPlaneLeftArm");
        return true;
    case 4:
        aOut = RED4ext::CName("renderPlane");
        return true;
    case 5:
        aOut = RED4ext::CName("renderPlaneInspect");
        return true;
    default:
        return false;
    }
}

bool ResolveRootGraphFloatPreset(int32_t aMode, RED4ext::CName& aOut)
{
    switch (aMode)
    {
    case 1:
        aOut = RED4ext::CName("disable_maya_engine_right_hand");
        return true;
    case 2:
        aOut = RED4ext::CName("disable_maya_engine_left_hand");
        return true;
    case 3:
        aOut = RED4ext::CName("pla_right_hand_attach");
        return true;
    case 4:
        aOut = RED4ext::CName("pla_left_hand_attach");
        return true;
    case 5:
        aOut = RED4ext::CName("procedural_ironsight_camera");
        return true;
    case 6:
        aOut = RED4ext::CName("camera_pitch");
        return true;
    case 7:
        aOut = RED4ext::CName("camera_yaw");
        return true;
    default:
        return false;
    }
}

bool ResolveRootGraphVectorPreset(int32_t aMode, RED4ext::CName& aOut)
{
    switch (aMode)
    {
    case 1:
        aOut = RED4ext::CName("weapon_offset_shoulder");
        return true;
    case 2:
        aOut = RED4ext::CName("weapon_offset_aiming");
        return true;
    case 3:
        aOut = RED4ext::CName("weapon_rotation_shoulder");
        return true;
    case 4:
        aOut = RED4ext::CName("weapon_rotation_aiming");
        return true;
    case 5:
        aOut = RED4ext::CName("debug_stand_camera_position");
        return true;
    case 6:
        aOut = RED4ext::CName("debug_crouch_camera_position");
        return true;
    default:
        return false;
    }
}

int32_t ForceVRNeutralAnimGraphInputs()
{
    int32_t writes = 0;
    auto add = [&](int32_t r) {
        if (r > 0) writes += r;
    };

    // Weapon/camera stance inputs that visually move the FPP camera, shoulders and hands.
    // Do not touch ironsight/ADS-specific inputs; weapon aiming must stay gameplay-correct.
    add(SetRootGraphFloatVariable(RED4ext::CName("disable_maya_engine_right_hand"), 1.0f));
    add(SetRootGraphFloatVariable(RED4ext::CName("disable_maya_engine_left_hand"), 1.0f));
    add(SetRootGraphFloatVariable(RED4ext::CName("camera_pitch"), 0.0f));
    add(SetRootGraphFloatVariable(RED4ext::CName("camera_yaw"), 0.0f));

    RED4ext::Vector4 zero{0, 0, 0, 0};
    add(SetRootGraphVectorVariable(RED4ext::CName("weapon_offset_shoulder"), zero));
    add(SetRootGraphVectorVariable(RED4ext::CName("weapon_rotation_shoulder"), zero));
    add(SetRootGraphVectorVariable(RED4ext::CName("debug_stand_camera_position"), zero));
    add(SetRootGraphVectorVariable(RED4ext::CName("debug_crouch_camera_position"), zero));

    // Runtime track arrays are the values the evaluated graph reads. Keep camera vertical offset
    // neutral and leave feet IK enabled; this is applied every frame while VRIK is active.
    add(SetRootLiveTrackValue(RED4ext::CName("cameraUpOffset"), 0.0f, 0));
    add(SetRootLiveTrackValue(RED4ext::CName("allowFeetIk"), 1.0f, 0));
    add(SetRootLiveTrackValue(RED4ext::CName("enableLeftFootIk"), 1.0f, 0));
    add(SetRootLiveTrackValue(RED4ext::CName("enableRightFootIk"), 1.0f, 0));

    return writes;
}

void AppendRootGraphVariableLog(const char* aSource, const RED4ext::CName& aKey, const char* aValueText, int32_t aResult)
{
    std::ofstream out(VRDiagPath("root_graph_variable_test_log.txt"), std::ios::app);
    if (!out.is_open())
        return;

    out << "source=" << (aSource ? aSource : "<null>")
        << " key=" << aKey.ToString()
        << " value=" << (aValueText ? aValueText : "<null>")
        << " result=" << aResult
        << "\n";
}

void AppendDirectAnimParamLog(const char* aSource, const RED4ext::CName& aKey, float aValue, int32_t aResult)
{
    std::ofstream out(VRDiagPath("anim_parameter_direct_test_log.txt"), std::ios::app);
    if (!out.is_open())
        return;

    out << "source=" << (aSource ? aSource : "<null>")
        << " key=" << aKey.ToString()
        << " value=" << aValue
        << " result=" << aResult
        << "\n";
}

int32_t SetPlayerAnimatedParameterValue(const RED4ext::CName& aKey, float aValue)
{
    auto* playerEntity = FindPlayerEntity();
    if (!playerEntity)
        return -50;

    int32_t updated = 0;
    for (auto& componentHandle : playerEntity->components)
    {
        auto* component = componentHandle.instance;
        if (!component)
            continue;

        auto* type = component->GetType();
        if (!type || type->name != "entAnimatedComponent")
            continue;

        auto* animated = reinterpret_cast<RED4ext::ent::AnimatedComponent*>(component);
        for (uint32_t i = 0; i < animated->animParameters.Size(); ++i)
        {
            auto& param = animated->animParameters[i];
            if (param.parameterName == aKey || param.animTrackName == aKey)
            {
                param.defaultValue = aValue;
                ++updated;
            }
        }
    }

    return updated;
}

int32_t QueueVectorInputEvents(const RED4ext::Vector4& aLeft, const RED4ext::Vector4& aRight)
{
    RED4ext::CName leftKey("ikLeftHandLocalPosition");
    RED4ext::CName rightKey("ikRightHandLocalPosition");

    auto leftEvt = CreateVectorInputEvent(leftKey, aLeft);
    auto rightEvt = CreateVectorInputEvent(rightKey, aRight);
    if (!leftEvt || !rightEvt)
        return -20;

    int32_t queued = 0;
    if (QueuePlayerEvent(static_cast<RED4ext::Handle<RED4ext::red::Event>>(leftEvt)))
        ++queued;
    if (QueuePlayerEvent(static_cast<RED4ext::Handle<RED4ext::red::Event>>(rightEvt)))
        ++queued;

    return queued == 2 ? 6 : -21;
}

int32_t QueueFeatureInputEvent(const RED4ext::CName& aFeatureName,
                                      const RED4ext::Handle<RED4ext::anim::AnimFeature>& aFeature)
{
    auto evt = CreateFeatureInputEvent(aFeatureName, aFeature);
    if (!evt)
        return -22;

    return QueuePlayerEvent(static_cast<RED4ext::Handle<RED4ext::red::Event>>(evt)) ? 7 : -23;
}

int32_t QueueFeatureInputEvent(const RED4ext::CName& aFeatureName,
                                      const RED4ext::Vector4& aLeft,
                                      const RED4ext::Vector4& aRight)
{
    auto feature = CreateWeaponUserFeature(aLeft, aRight);
    return QueueFeatureInputEvent(aFeatureName, feature);
}

void AppendAnimTestLog(const char* aModeName, int32_t aResult, const RED4ext::Vector4& aLeft, const RED4ext::Vector4& aRight)
{
    std::ofstream out(VRDiagPath("vrik_anim_input_test_log.txt"), std::ios::app);
    if (!out.is_open())
        return;

    out << "mode=" << (aModeName ? aModeName : "<null>")
        << " result=" << aResult
        << " left=(" << aLeft.X << ", " << aLeft.Y << ", " << aLeft.Z << ", " << aLeft.W << ")"
        << " right=(" << aRight.X << ", " << aRight.Y << ", " << aRight.Z << ", " << aRight.W << ")"
        << "\n";
}

bool FillAnimTestPose(int32_t aMode, RED4ext::Vector4& aLeft, RED4ext::Vector4& aRight)
{
    if (aMode == 2)
    {
        EnsureSharedMemory();
        if (!g_pSharedHands)
            return false;

        // Approximate XR-local -> game-local mapping based on previous working hand-space conversion.
        aLeft.X = g_pSharedHands[1];
        aLeft.Y = -g_pSharedHands[3];
        aLeft.Z = g_pSharedHands[2];
        aLeft.W = 1.0f;

        aRight.X = g_pSharedHands[9];
        aRight.Y = -g_pSharedHands[11];
        aRight.Z = g_pSharedHands[10];
        aRight.W = 1.0f;
        return true;
    }

    // Keep all non-VR test modes on an exaggerated pose so visual response is obvious.
    aLeft.X = -0.65f; aLeft.Y = 0.75f; aLeft.Z = 0.35f; aLeft.W = 1.0f;
    aRight.X = 0.65f; aRight.Y = 0.75f; aRight.Z = 0.35f; aRight.W = 1.0f;
    return true;
}

static int32_t ApplyFeatureHandle(RED4ext::ent::AnimationControllerComponent* aController,
                                  const RED4ext::CName& aFeatureName,
                                  const RED4ext::Handle<RED4ext::anim::AnimFeature>& aFeature)
{
    if (!aController)
        return -10;

    auto* cls = aController->GetType();
    auto* func = cls ? cls->GetFunction("ApplyFeature") : nullptr;
    if (!func)
        return -11;

    if (func->params.Size() < 2)
        return -12;

    if (!aFeature)
        return -13;

    RED4ext::StackArgs_t args;
    args.emplace_back(nullptr, const_cast<RED4ext::CName*>(&aFeatureName));
    args.emplace_back(nullptr, const_cast<RED4ext::Handle<RED4ext::anim::AnimFeature>*>(&aFeature));

    const bool ok = RED4ext::ExecuteFunction(aController, func, nullptr, args);
    return ok ? 2 : -14;
}

int32_t ApplyWeaponUserFeature(RED4ext::ent::AnimationControllerComponent* aController,
                                      const RED4ext::CName& aFeatureName,
                                      const RED4ext::Vector4& aLeft,
                                      const RED4ext::Vector4& aRight)
{
    auto feature = CreateWeaponUserFeature(aLeft, aRight);
    return ApplyFeatureHandle(aController, aFeatureName, feature);
}

void TestWeaponUserFeatureRoute(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    int32_t route = 0;
    RED4ext::GetParameter(aFrame, &route);
    aFrame->code++;

    RED4ext::Vector4 left{};
    RED4ext::Vector4 right{};
    FillAnimTestPose(1, left, right);

    const RED4ext::CName featureName("weapon_user");
    int32_t result = -60;
    const char* modeName = "weapon_user_unknown";

    if (route == 0)
    {
        auto* controller = FindPlayerAnimationController();
        result = ApplyWeaponUserFeature(controller, featureName, left, right);
        modeName = "weapon_user_apply";
    }
    else if (route == 1)
    {
        result = QueueFeatureInputEvent(featureName, left, right);
        modeName = "weapon_user_queue";
    }

    AppendAnimTestLog(modeName, result, left, right);
    if (aOut) *aOut = result;
}

void TestIKFeatureRoute(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    int32_t mode = 0;
    int32_t route = 0;
    RED4ext::GetParameter(aFrame, &mode);
    RED4ext::GetParameter(aFrame, &route);
    aFrame->code++;

    const char* featureNameStr = nullptr;
    const char* logName = nullptr;
    if (mode == 1)
    {
        featureNameStr = "playerIK";
        logName = route == 0 ? "playerIK_apply" : "playerIK_queue";
    }
    else if (mode == 2)
    {
        featureNameStr = "interactionIK";
        logName = route == 0 ? "interactionIK_apply" : "interactionIK_queue";
    }
    else
    {
        if (aOut) *aOut = -61;
        return;
    }

    auto* playerEntity = FindPlayerEntity();
    auto* transform = playerEntity ? playerEntity->transformComponent : nullptr;
    if (!playerEntity || !transform)
    {
        if (aOut) *aOut = -62;
        return;
    }

    RED4ext::Vector4 point = transform->worldTransform.Position.AsVector4();
    point.X += 0.45f;
    point.Y += 0.35f;
    point.Z += 1.15f;
    point.W = 1.0f;

    RED4ext::Vector4 normal{};
    normal.X = 0.0f;
    normal.Y = 0.0f;
    normal.Z = 1.0f;
    normal.W = 0.0f;

    auto feature = CreateIKFeature(point, normal, 1.0f);
    const RED4ext::CName featureName(featureNameStr);
    int32_t result = -63;
    if (route == 0)
    {
        auto* controller = FindPlayerAnimationController();
        result = ApplyFeatureHandle(controller, featureName, feature);
    }
    else if (route == 1)
    {
        result = QueueFeatureInputEvent(featureName, feature);
    }

    AppendAnimTestLog(logName, result, point, normal);
    if (aOut) *aOut = result;
}

void TestMeleeIKDataFeatureRoute(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    int32_t route = 0;
    RED4ext::GetParameter(aFrame, &route);
    aFrame->code++;

    auto* playerEntity = FindPlayerEntity();
    auto* transform = playerEntity ? playerEntity->transformComponent : nullptr;
    if (!playerEntity || !transform)
    {
        if (aOut) *aOut = -64;
        return;
    }

    RED4ext::Vector4 base = transform->worldTransform.Position.AsVector4();
    RED4ext::Vector4 head = base;
    RED4ext::Vector4 chest = base;
    RED4ext::Vector4 offset{};

    head.Z += 1.55f; head.W = 1.0f;
    chest.Z += 1.15f; chest.W = 1.0f;
    offset.X = 0.55f; offset.Y = 0.35f; offset.Z = 0.15f; offset.W = 0.0f;

    auto feature = CreateMeleeIKDataFeature(head, chest, offset);
    const RED4ext::CName featureName("MeleeIKData");

    int32_t result = -65;
    const char* logName = route == 0 ? "MeleeIKData_apply" : "MeleeIKData_queue";
    if (route == 0)
    {
        auto* controller = FindPlayerAnimationController();
        result = ApplyFeatureHandle(controller, featureName, feature);
    }
    else if (route == 1)
    {
        result = QueueFeatureInputEvent(featureName, feature);
    }

    AppendAnimTestLog(logName, result, head, offset);
    if (aOut) *aOut = result;
}
#endif
#endif
