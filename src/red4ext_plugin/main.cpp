#include <RED4ext/RED4ext.hpp>
#include <RED4ext/GameEngine.hpp>
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
#include <fstream>
#include <iomanip>
#include <string>

#include "vrik_hook.h"

static HANDLE g_hMapFile = NULL;
static float* g_pSharedHands = nullptr;
static bool g_chunkDebugEnabled = false;
static bool g_chunkDebugWasEnabled = false;
static int32_t g_chunkDebugComponentIndex = -1;
static int32_t g_chunkDebugHand = 1; // 0 = left, 1 = right
static int32_t g_chunkDebugBitSlots[4] = {0, -1, -1, -1};
static int32_t g_animInputTestMode = 0; // 0 off, 1 constant test pose, 2 mapped VR local positions
static int32_t g_animParamPersistentPreset = 0;
static float g_animParamPersistentValue = 0.0f;
static int32_t g_animParamPersistentLastResult = 0;
static int32_t g_rootGraphFloatPersistentPreset = 0;
static float g_rootGraphFloatPersistentValue = 0.0f;
static int32_t g_rootGraphFloatPersistentLastResult = 0;
static int32_t g_rootGraphVectorPersistentPreset = 0;
static RED4ext::Vector4 g_rootGraphVectorPersistentValue = {0, 0, 0, 0};
static int32_t g_rootGraphVectorPersistentLastResult = 0;
static int32_t g_rootMetaRigTrackPersistentPreset = 0;
static float g_rootMetaRigTrackPersistentValue = 0.0f;
static int32_t g_rootMetaRigTrackPersistentLastResult = 0;

static int32_t g_rootLiveTrackPersistentPreset = 0;
static float g_rootLiveTrackPersistentValue = 0.0f;
static int32_t g_rootLiveTrackPersistentArrayMode = 0;
static int32_t g_rootLiveTrackPersistentLastResult = 0;

RED4ext::Vector4 g_CameraWorldPos = {0,0,0,1};
int g_CalibrationBoneIndex = -1;
void* g_PlayerAnimComponent = nullptr;

// Bone-hook diagnostics (written from Hooked_ComponentFunc21).
volatile uint64_t g_hookTotalCalls = 0;   // every invocation of the hooked function
volatile uint64_t g_hookMatchCalls = 0;   // invocations where rcx == g_PlayerAnimComponent
volatile uint64_t g_hookBoneWrites = 0;   // invocations that reached the bone write

// Capture mode: record rcx of every call that has a skeleton (rcx+0x168->+0xE0 != null),
// so we can check whether the player's animated component flows through this function.
volatile int      g_hookCapture = 0;
volatile uint64_t g_hookSkeletalCalls = 0;
volatile uint32_t g_capturedRcx[32] = {0};   // distinct low-32 rcx of skeletal components
volatile uint64_t g_capturedFull[32] = {0};  // full 64-bit rcx, for arm-by-index / bone count
volatile int      g_capturedCount = 0;

// Pose-apply hook (sub_14017DDB4) state. See vrik_hook.h.
volatile uintptr_t g_PlayerTrackBufA = 0;
volatile uintptr_t g_PlayerTrackBufB = 0;
volatile int       g_AnimPoseDebug = 0;
volatile uint64_t  g_AnimPoseTotalCalls = 0;
volatile uint64_t  g_AnimPoseMatchCalls = 0;
volatile uintptr_t g_AnimPoseLastBoneBuf = 0;
volatile int       g_AnimPoseTestBone = -1;
volatile float     g_AnimPoseTestMag = 1.0f;

volatile int       g_VRBind = 0;
volatile float     g_VRBindScale = 1.0f;
volatile float     g_VRBindOffX = 0.0f;
volatile float     g_VRBindOffY = 0.0f;
volatile float     g_VRBindOffZ = 0.0f;
volatile int       g_VRBindAxis = 1;     // Y-up -> Z-up mapping by default
volatile int       g_VRRightBoneIdx = 24;
volatile int       g_VRLeftBoneIdx = 23;

static RED4ext::world::AnimationSystem* ScanForAnimationSystemInBlock(uint8_t* aBase, size_t aSize, std::ofstream* aOut = nullptr);
static const char* ClassifyQword(uint64_t v);
static uint64_t SafeReadQword(uint8_t* base, size_t off);
static uint32_t SafeReadU32(uint8_t* base, size_t off);
static float SafeReadFloat(uint8_t* base, size_t off);
static RED4ext::CClass* SafeGetObjectType(void* aPtr);

// Cached pointer to the live worldAnimationSystem. The discovery scan
// (SafeGetObjectType over hundreds of arbitrary heap pointers) is the main
// source of intermittent crashes, so we run it once and reuse the result.
static RED4ext::world::AnimationSystem* g_cachedAnimationSystem = nullptr;

static bool ContainsInsensitive(const char* haystack, const char* needle) {
    if (!haystack || !needle) return false;
    std::string h(haystack);
    std::string n(needle);
    std::transform(h.begin(), h.end(), h.begin(), [](unsigned char c) { return static_cast<char>(std::tolower(c)); });
    std::transform(n.begin(), n.end(), n.begin(), [](unsigned char c) { return static_cast<char>(std::tolower(c)); });
    return h.find(n) != std::string::npos;
}

static bool ClassIsA(RED4ext::CClass* type, RED4ext::CName className) {
    while (type) {
        if (type->name == className) return true;
        type = type->parent;
    }
    return false;
}

static bool IsLikelyFppArmComponent(const char* componentName) {
    if (!componentName) return false;
    return ContainsInsensitive(componentName, "_fpp_") ||
           ContainsInsensitive(componentName, "fpp_lights") ||
           ContainsInsensitive(componentName, "strongarms_holstered") ||
           ContainsInsensitive(componentName, "personal_link_default_holstered") ||
           ContainsInsensitive(componentName, "injection_mark") ||
           ContainsInsensitive(componentName, "_pma_base__") ||
           ContainsInsensitive(componentName, "_pwa_base__") ||
           ContainsInsensitive(componentName, "_pma_fpp__neck") ||
           ContainsInsensitive(componentName, "_pwa_fpp__neck") ||
           ContainsInsensitive(componentName, "holstered_arms_data");
}

static uint64_t BuildChunkDebugMask() {
    uint64_t mask = 0;
    for (int i = 0; i < 4; ++i) {
        const int32_t bit = g_chunkDebugBitSlots[i];
        if (bit >= 0 && bit < 64) {
            mask |= (1ull << bit);
        }
    }
    return mask;
}

void EnsureSharedMemory() {
    if (!g_pSharedHands) {
        g_hMapFile = CreateFileMappingA(INVALID_HANDLE_VALUE, NULL, PAGE_READWRITE, 0, 256, "CyberpunkVR_Hands_Shared");
        if (g_hMapFile) g_pSharedHands = (float*)MapViewOfFile(g_hMapFile, FILE_MAP_ALL_ACCESS, 0, 0, 256);
    }
}

void GetLeftVRHandValid(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t a4) {
    aFrame->code++; EnsureSharedMemory();
    if (aOut) *aOut = g_pSharedHands ? (g_pSharedHands[0] > 0.0f) : false;
}

void GetRightVRHandValid(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t a4) {
    aFrame->code++; EnsureSharedMemory();
    if (aOut) *aOut = g_pSharedHands ? (g_pSharedHands[8] > 0.0f) : false;
}

void GetLeftVRHandPos(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4) {
    aFrame->code++; EnsureSharedMemory();
    if (aOut) {
        if (g_pSharedHands) {
            aOut->X = g_pSharedHands[1]; aOut->Y = g_pSharedHands[2]; aOut->Z = g_pSharedHands[3];
        } else { aOut->X = aOut->Y = aOut->Z = 0.0f; }
        aOut->W = 1.0f;
    }
}

void GetRightVRHandPos(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4) {
    aFrame->code++; EnsureSharedMemory();
    if (aOut) {
        if (g_pSharedHands) {
            aOut->X = g_pSharedHands[9]; aOut->Y = g_pSharedHands[10]; aOut->Z = g_pSharedHands[11];
        } else { aOut->X = aOut->Y = aOut->Z = 0.0f; }
        aOut->W = 1.0f;
    }
}

void GetLeftVRHandRot(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Quaternion* aOut, int64_t a4) {
    aFrame->code++; EnsureSharedMemory();
    if (aOut) {
        if (g_pSharedHands) {
            aOut->i = g_pSharedHands[4]; aOut->j = g_pSharedHands[5]; aOut->k = g_pSharedHands[6]; aOut->r = g_pSharedHands[7];
        } else { aOut->i = aOut->j = aOut->k = 0.0f; aOut->r = 1.0f; }
    }
}

void GetRightVRHandRot(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Quaternion* aOut, int64_t a4) {
    aFrame->code++; EnsureSharedMemory();
    if (aOut) {
        if (g_pSharedHands) {
            aOut->i = g_pSharedHands[12]; aOut->j = g_pSharedHands[13]; aOut->k = g_pSharedHands[14]; aOut->r = g_pSharedHands[15];
        } else { aOut->i = aOut->j = aOut->k = 0.0f; aOut->r = 1.0f; }
    }
}

void IsVRHandLinked(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t a4) {
    aFrame->code++; EnsureSharedMemory();
    if (aOut) *aOut = (g_pSharedHands != nullptr);
}

static RED4ext::WeakHandle<RED4ext::IScriptable> g_rightHandEntity;

void SetVRRightHandEntity(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, void* aOut, int64_t a4) {
    RED4ext::Handle<RED4ext::IScriptable> ent;
    RED4ext::GetParameter(aFrame, &ent);
    aFrame->code++;
    g_rightHandEntity = ent;
}

void DumpVRFppComponents(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;

    RED4ext::ScriptGameInstance gameInstance;
    RED4ext::Handle<RED4ext::IScriptable> playerHandle;
    RED4ext::ExecuteGlobalFunction("GetPlayer;GameInstance", &playerHandle, gameInstance);
    if (!playerHandle) { if (aOut) *aOut = -1; return; }

    auto* playerEntity = reinterpret_cast<RED4ext::ent::Entity*>(playerHandle.instance);
    if (!playerEntity) { if (aOut) *aOut = -2; return; }

    std::ofstream out("C:\\Users\\dariulone\\Desktop\\CyberpunkVRPort\\fpp_components_dump.txt", std::ios::out | std::ios::trunc);
    if (!out.is_open()) { if (aOut) *aOut = -3; return; }

    out << "Player entity templatePathHash=0x" << std::hex << playerEntity->templatePath.hash << std::dec << "\n";
    out << "Components:\n";

    int32_t count = 0;
    for (auto& componentHandle : playerEntity->components) {
        auto* component = componentHandle.instance;
        if (!component) continue;

        RED4ext::CClass* type = component->GetType();
        const char* typeName = type ? type->name.ToString() : "<null>";
        const char* componentName = component->name.ToString();

        out << "[" << count << "] ptr=0x" << std::hex << reinterpret_cast<uintptr_t>(component) << std::dec
            << " name=" << (componentName ? componentName : "<null>")
            << " type=" << (typeName ? typeName : "<null>")
            << " enabled=" << (component->isEnabled ? 1 : 0);

        if (ClassIsA(type, "entSkinnedMeshComponent")) {
            auto* skinned = reinterpret_cast<RED4ext::ent::SkinnedMeshComponent*>(component);
            out << " meshAppearance=" << skinned->meshAppearance.ToString()
                << " meshPathHash=0x" << std::hex << skinned->mesh.path.hash << std::dec
                << " chunkMask=0x" << std::hex << skinned->chunkMask << std::dec;
        } else if (ClassIsA(type, "entMeshComponent")) {
            auto* mesh = reinterpret_cast<RED4ext::ent::MeshComponent*>(component);
            out << " meshAppearance=" << mesh->meshAppearance.ToString()
                << " meshPathHash=0x" << std::hex << mesh->mesh.path.hash << std::dec
                << " chunkMask=0x" << std::hex << mesh->chunkMask << std::dec
                << " visualScale=(" << mesh->visualScale.X << ", " << mesh->visualScale.Y << ", " << mesh->visualScale.Z << ")";
        }

        if (IsLikelyFppArmComponent(componentName)) {
            out << " [LIKELY_FPP_ARM]";
        }
        out << "\n";
        ++count;
    }

    out.close();
    if (aOut) *aOut = count;
}

void SetVRFppChunkDebugEnabled(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, void* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(aOut); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t enabled = 0;
    RED4ext::GetParameter(aFrame, &enabled);
    aFrame->code++;
    // VRFPP chunk debug disabled in this build.
    g_chunkDebugEnabled = false;
}

void SetVRFppChunkDebugComponentIndex(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, void* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(aOut); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t index = -1;
    RED4ext::GetParameter(aFrame, &index);
    aFrame->code++;
    g_chunkDebugComponentIndex = -1;
}

void SetVRFppChunkDebugHand(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, void* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(aOut); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t hand = 1;
    RED4ext::GetParameter(aFrame, &hand);
    aFrame->code++;
    g_chunkDebugHand = 1;
}

void SetVRFppChunkDebugBits(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, void* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(aOut); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t bit0 = -1, bit1 = -1, bit2 = -1, bit3 = -1;
    RED4ext::GetParameter(aFrame, &bit0);
    RED4ext::GetParameter(aFrame, &bit1);
    RED4ext::GetParameter(aFrame, &bit2);
    RED4ext::GetParameter(aFrame, &bit3);
    aFrame->code++;
    g_chunkDebugBitSlots[0] = -1;
    g_chunkDebugBitSlots[1] = -1;
    g_chunkDebugBitSlots[2] = -1;
    g_chunkDebugBitSlots[3] = -1;
}

static RED4ext::ent::AnimationControllerComponent* FindPlayerAnimationController()
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

static RED4ext::ent::Entity* FindPlayerEntity()
{
    RED4ext::ScriptGameInstance gameInstance;
    RED4ext::Handle<RED4ext::IScriptable> playerHandle;
    RED4ext::ExecuteGlobalFunction("GetPlayer;GameInstance", &playerHandle, gameInstance);
    if (!playerHandle)
        return nullptr;

    return reinterpret_cast<RED4ext::ent::Entity*>(playerHandle.instance);
}

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

static bool IsValidAnimationSystemPtr(void* aPtr)
{
    if (std::strcmp(ClassifyQword(reinterpret_cast<uint64_t>(aPtr)), "HEAP") != 0)
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
    if (!aRuntimeScene)
        return nullptr;

    uint64_t parent = SafeReadQword(reinterpret_cast<uint8_t*>(aRuntimeScene), 0x8);
    if (std::strcmp(ClassifyQword(parent), "HEAP") != 0)
        return nullptr;

    uint64_t cand = SafeReadQword(reinterpret_cast<uint8_t*>(parent), 0x38);
    if (std::strcmp(ClassifyQword(cand), "HEAP") != 0)
        return nullptr;

    if (IsValidAnimationSystemPtr(reinterpret_cast<void*>(cand)))
        return reinterpret_cast<RED4ext::world::AnimationSystem*>(cand);

    return nullptr;
}

static RED4ext::world::AnimationSystem* FindWorldAnimationSystemFromScene(RED4ext::world::RuntimeScene* aRuntimeScene,
                                                                          uintptr_t aFrameworkScene,
                                                                          std::ofstream* aOut = nullptr)
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

static RED4ext::anim::AnimatedObject* FindPlayerAnimatedObjectByComponentName(const char* aComponentName)
{
    auto* playerEntity = FindPlayerEntity();
    auto* engine = RED4ext::CGameEngine::Get();
    auto* framework = engine ? engine->framework : nullptr;
    auto* animationSystem = playerEntity
        ? FindWorldAnimationSystemFromScene(playerEntity->runtimeScene, framework ? framework->unk18 : 0, nullptr)
        : nullptr;
    if (!playerEntity || !animationSystem || !aComponentName)
        return nullptr;

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
                if (name && std::strcmp(name, aComponentName) == 0)
                    return bucket.animatedObjects[entryIndex];
            }
        }
        __except (EXCEPTION_EXECUTE_HANDLER)
        {
        }
    }

    return nullptr;
}

// Returns the live animatedComponent instance from the worldAnimationSystem bucket
// (the object actually driven each frame), which differs from the handle wrapper
// in playerEntity->components. This is the instance whose IComponent::Update fires.
static RED4ext::ent::AnimatedComponent* FindPlayerBucketAnimatedComponent(const char* aComponentName)
{
    auto* playerEntity = FindPlayerEntity();
    auto* engine = RED4ext::CGameEngine::Get();
    auto* framework = engine ? engine->framework : nullptr;
    auto* animationSystem = playerEntity
        ? FindWorldAnimationSystemFromScene(playerEntity->runtimeScene, framework ? framework->unk18 : 0, nullptr)
        : nullptr;
    if (!playerEntity || !animationSystem || !aComponentName)
        return nullptr;

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
                if (name && std::strcmp(name, aComponentName) == 0)
                    return animatedComponent;
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

    std::ofstream out("C:\\Users\\dariulone\\Desktop\\CyberpunkVRPort\\animation_system_lookup.txt", std::ios::trunc);
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

static const char* GetTypeNameForDump(RED4ext::rtti::IType* aType)
{
    if (!aType)
        return "<null>";

    const char* computed = aType->GetComputedName().ToString();
    if (computed && computed[0])
        return computed;

    const char* native = aType->GetName().ToString();
    return native ? native : "<unnamed>";
}

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

static RED4ext::Handle<RED4ext::ent::IPositionProvider> CreateStaticPositionProvider(const RED4ext::Vector4& aPosition)
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

static RED4ext::Handle<RED4ext::ent::IOrientationProvider> CreateStaticOrientationProvider()
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

static void AppendPlayerControllerIKState(std::ofstream& aOut)
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

static void DumpBindingInfo(std::ofstream& aOut, RED4ext::ent::AnimationControlBinding* aBinding, const char* aPrefix)
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

static void DumpAnimTrackParameters(std::ofstream& aOut, RED4ext::ent::AnimatedComponent* aAnimated, const char* aPrefix)
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

template<typename T>
static T* LoadResourceRef(RED4ext::Ref<T>& aRef)
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

static void DumpNameListFiltered(std::ofstream& aOut, const char* aLabel, const RED4ext::DynArray<RED4ext::CName>& aNames)
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

static RED4ext::ent::AnimatedComponent* FindPlayerAnimatedComponentByName(const char* aName)
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

static bool IsInterestingAnimName(const char* aName)
{
    if (!aName)
        return false;

    return ContainsInsensitive(aName, "arm") ||
           ContainsInsensitive(aName, "hand") ||
           ContainsInsensitive(aName, "ik") ||
           ContainsInsensitive(aName, "weapon") ||
           ContainsInsensitive(aName, "zoom") ||
           ContainsInsensitive(aName, "render") ||
           ContainsInsensitive(aName, "visibility") ||
           ContainsInsensitive(aName, "camera") ||
           ContainsInsensitive(aName, "left") ||
           ContainsInsensitive(aName, "right");
}

static void DumpAnimGraphVariables(std::ofstream& aOut, RED4ext::anim::AnimGraph* aGraph, const char* aPrefix)
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

static void DumpMetaRigTracks(std::ofstream& aOut, RED4ext::anim::MetaRig* aMetaRig, const char* aPrefix)
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

static bool ResolveRootMetaRigTrackPreset(int32_t aMode, RED4ext::CName& aOut)
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

static void AppendRootMetaRigTrackLog(const char* aSource, const RED4ext::CName& aKey, float aValue, int32_t aResult)
{
    std::ofstream out("C:\\Users\\dariulone\\Desktop\\CyberpunkVRPort\\root_metarig_track_test_log.txt", std::ios::app);
    if (!out.is_open())
        return;

    out << "source=" << (aSource ? aSource : "<null>")
        << " key=" << aKey.ToString()
        << " value=" << aValue
        << " result=" << aResult
        << "\n";
}

static int32_t SetRootMetaRigTrackValue(const RED4ext::CName& aName, float aValue)
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
static int32_t SetRootLiveTrackValue(const RED4ext::CName& aName, float aValue, int32_t aArrayMode)
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
static int32_t ReadRootLiveTrackValue(const RED4ext::CName& aName, int32_t aArrayMode)
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

static int32_t SetRootGraphFloatVariable(const RED4ext::CName& aName, float aValue)
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

static int32_t SetRootGraphBoolVariable(const RED4ext::CName& aName, bool aValue)
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

static int32_t SetRootGraphVectorVariable(const RED4ext::CName& aName, const RED4ext::Vector4& aValue)
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

static void DumpEntityAnimationInfo(std::ofstream& aOut, RED4ext::ent::Entity* aEntity, const char* aReason)
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

static bool QueuePlayerEvent(const RED4ext::Handle<RED4ext::red::Event>& aEvent)
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

static int32_t SetFloatInputDirect(const RED4ext::CName& aKey, float aValue)
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

static int32_t QueueFloatInputEvent(const RED4ext::CName& aKey, float aValue)
{
    auto evt = CreateFloatInputEvent(aKey, aValue);
    if (!evt)
        return -43;

    return QueuePlayerEvent(static_cast<RED4ext::Handle<RED4ext::red::Event>>(evt)) ? 11 : -44;
}

static void AppendAnimFloatTestLog(const char* aRouteName, const RED4ext::CName& aKey, float aValue, int32_t aResult)
{
    std::ofstream out("C:\\Users\\dariulone\\Desktop\\CyberpunkVRPort\\anim_float_input_test_log.txt", std::ios::app);
    if (!out.is_open())
        return;

    out << "route=" << (aRouteName ? aRouteName : "<null>")
        << " key=" << aKey.ToString()
        << " value=" << aValue
        << " result=" << aResult
        << "\n";
}

static bool ResolveAnimFloatPreset(int32_t aMode, RED4ext::CName& aOut)
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

static bool ResolveRootGraphFloatPreset(int32_t aMode, RED4ext::CName& aOut)
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

static bool ResolveRootGraphVectorPreset(int32_t aMode, RED4ext::CName& aOut)
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

static void AppendRootGraphVariableLog(const char* aSource, const RED4ext::CName& aKey, const char* aValueText, int32_t aResult)
{
    std::ofstream out("C:\\Users\\dariulone\\Desktop\\CyberpunkVRPort\\root_graph_variable_test_log.txt", std::ios::app);
    if (!out.is_open())
        return;

    out << "source=" << (aSource ? aSource : "<null>")
        << " key=" << aKey.ToString()
        << " value=" << (aValueText ? aValueText : "<null>")
        << " result=" << aResult
        << "\n";
}

static void AppendDirectAnimParamLog(const char* aSource, const RED4ext::CName& aKey, float aValue, int32_t aResult)
{
    std::ofstream out("C:\\Users\\dariulone\\Desktop\\CyberpunkVRPort\\anim_parameter_direct_test_log.txt", std::ios::app);
    if (!out.is_open())
        return;

    out << "source=" << (aSource ? aSource : "<null>")
        << " key=" << aKey.ToString()
        << " value=" << aValue
        << " result=" << aResult
        << "\n";
}

static int32_t SetPlayerAnimatedParameterValue(const RED4ext::CName& aKey, float aValue)
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

static int32_t QueueVectorInputEvents(const RED4ext::Vector4& aLeft, const RED4ext::Vector4& aRight)
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

static int32_t QueueFeatureInputEvent(const RED4ext::CName& aFeatureName,
                                      const RED4ext::Handle<RED4ext::anim::AnimFeature>& aFeature)
{
    auto evt = CreateFeatureInputEvent(aFeatureName, aFeature);
    if (!evt)
        return -22;

    return QueuePlayerEvent(static_cast<RED4ext::Handle<RED4ext::red::Event>>(evt)) ? 7 : -23;
}

static int32_t QueueFeatureInputEvent(const RED4ext::CName& aFeatureName,
                                      const RED4ext::Vector4& aLeft,
                                      const RED4ext::Vector4& aRight)
{
    auto feature = CreateWeaponUserFeature(aLeft, aRight);
    return QueueFeatureInputEvent(aFeatureName, feature);
}

static void AppendAnimTestLog(const char* aModeName, int32_t aResult, const RED4ext::Vector4& aLeft, const RED4ext::Vector4& aRight)
{
    std::ofstream out("C:\\Users\\dariulone\\Desktop\\CyberpunkVRPort\\vrik_anim_input_test_log.txt", std::ios::app);
    if (!out.is_open())
        return;

    out << "mode=" << (aModeName ? aModeName : "<null>")
        << " result=" << aResult
        << " left=(" << aLeft.X << ", " << aLeft.Y << ", " << aLeft.Z << ", " << aLeft.W << ")"
        << " right=(" << aRight.X << ", " << aRight.Y << ", " << aRight.Z << ", " << aRight.W << ")"
        << "\n";
}

static bool FillAnimTestPose(int32_t aMode, RED4ext::Vector4& aLeft, RED4ext::Vector4& aRight)
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

static int32_t ApplyWeaponUserFeature(RED4ext::ent::AnimationControllerComponent* aController,
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

void DumpRootGraphVariables(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;

    std::ofstream out("C:\\Users\\dariulone\\Desktop\\CyberpunkVRPort\\root_graph_variables.txt", std::ios::trunc);
    if (!out.is_open())
    {
        if (aOut) *aOut = -73;
        return;
    }

    auto* animated = FindPlayerAnimatedComponentByName("root");
    auto* graph = animated ? LoadResourceRef(animated->graph) : nullptr;
    if (!graph)
    {
        if (aOut) *aOut = -74;
        return;
    }

    DumpAnimGraphVariables(out, graph, "");
    out.close();
    if (aOut) *aOut = 1;
}

void SetRootGraphBoolVariableByName(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    RED4ext::CName name;
    bool value = false;
    RED4ext::GetParameter(aFrame, &name);
    RED4ext::GetParameter(aFrame, &value);
    aFrame->code++;

    if (aOut) *aOut = SetRootGraphBoolVariable(name, value);
}

void SetRootGraphFloatVariableByName(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    RED4ext::CName name;
    float value = 0.0f;
    RED4ext::GetParameter(aFrame, &name);
    RED4ext::GetParameter(aFrame, &value);
    aFrame->code++;

    if (aOut) *aOut = SetRootGraphFloatVariable(name, value);
}

void SetRootGraphVectorVariableByName(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    RED4ext::CName name;
    RED4ext::Vector4 value{};
    RED4ext::GetParameter(aFrame, &name);
    RED4ext::GetParameter(aFrame, &value);
    aFrame->code++;

    if (aOut) *aOut = SetRootGraphVectorVariable(name, value);
}

void TestRootGraphFloatPreset(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    int32_t mode = 0;
    float value = 0.0f;
    RED4ext::GetParameter(aFrame, &mode);
    RED4ext::GetParameter(aFrame, &value);
    aFrame->code++;

    RED4ext::CName name;
    if (!ResolveRootGraphFloatPreset(mode, name))
    {
        if (aOut) *aOut = -75;
        return;
    }

    const int32_t result = SetRootGraphFloatVariable(name, value);
    AppendRootGraphVariableLog("float_preset", name, std::to_string(value).c_str(), result);
    if (aOut) *aOut = result;
}

void TestRootGraphVectorPreset(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    int32_t mode = 0;
    RED4ext::Vector4 value{};
    RED4ext::GetParameter(aFrame, &mode);
    RED4ext::GetParameter(aFrame, &value);
    aFrame->code++;

    RED4ext::CName name;
    if (!ResolveRootGraphVectorPreset(mode, name))
    {
        if (aOut) *aOut = -76;
        return;
    }

    const int32_t result = SetRootGraphVectorVariable(name, value);
    std::string valueText = std::to_string(value.X) + "," + std::to_string(value.Y) + "," + std::to_string(value.Z) + "," + std::to_string(value.W);
    AppendRootGraphVariableLog("vector_preset", name, valueText.c_str(), result);
    if (aOut) *aOut = result;
}

void SetRootGraphFloatPresetPersistent(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    int32_t mode = 0;
    float value = 0.0f;
    RED4ext::GetParameter(aFrame, &mode);
    RED4ext::GetParameter(aFrame, &value);
    aFrame->code++;

    g_rootGraphFloatPersistentPreset = mode;
    g_rootGraphFloatPersistentValue = value;
    g_rootGraphFloatPersistentLastResult = 0;
    if (aOut) *aOut = 1;
}

void SetRootGraphVectorPresetPersistent(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    int32_t mode = 0;
    RED4ext::Vector4 value{};
    RED4ext::GetParameter(aFrame, &mode);
    RED4ext::GetParameter(aFrame, &value);
    aFrame->code++;

    g_rootGraphVectorPersistentPreset = mode;
    g_rootGraphVectorPersistentValue = value;
    g_rootGraphVectorPersistentLastResult = 0;
    if (aOut) *aOut = 1;
}

void GetRootGraphPersistentLastResult(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    if (aOut)
    {
        *aOut = g_rootGraphVectorPersistentPreset != 0 ? g_rootGraphVectorPersistentLastResult : g_rootGraphFloatPersistentLastResult;
    }
}

void SetVRIKAnimInputTestMode(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, void* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(aOut); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t mode = 0;
    RED4ext::GetParameter(aFrame, &mode);
    aFrame->code++;
    g_animInputTestMode = mode;
}

void UpdateVRIKAnimInputs(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;

    if (g_rootGraphFloatPersistentPreset != 0)
    {
        RED4ext::CName name;
        if (ResolveRootGraphFloatPreset(g_rootGraphFloatPersistentPreset, name))
        {
            g_rootGraphFloatPersistentLastResult = SetRootGraphFloatVariable(name, g_rootGraphFloatPersistentValue);
        }
        else
        {
            g_rootGraphFloatPersistentLastResult = -77;
        }
    }

    if (g_rootGraphVectorPersistentPreset != 0)
    {
        RED4ext::CName name;
        if (ResolveRootGraphVectorPreset(g_rootGraphVectorPersistentPreset, name))
        {
            g_rootGraphVectorPersistentLastResult = SetRootGraphVectorVariable(name, g_rootGraphVectorPersistentValue);
        }
        else
        {
            g_rootGraphVectorPersistentLastResult = -78;
        }
    }

    if (g_rootMetaRigTrackPersistentPreset != 0)
    {
        RED4ext::CName name;
        if (ResolveRootMetaRigTrackPreset(g_rootMetaRigTrackPersistentPreset, name))
        {
            g_rootMetaRigTrackPersistentLastResult = SetRootMetaRigTrackValue(name, g_rootMetaRigTrackPersistentValue);
        }
        else
        {
            g_rootMetaRigTrackPersistentLastResult = -85;
        }
    }

    if (g_rootLiveTrackPersistentPreset != 0)
    {
        RED4ext::CName name;
        if (ResolveRootMetaRigTrackPreset(g_rootLiveTrackPersistentPreset, name))
        {
            g_rootLiveTrackPersistentLastResult = SetRootLiveTrackValue(name, g_rootLiveTrackPersistentValue, g_rootLiveTrackPersistentArrayMode);
        }
        else
        {
            g_rootLiveTrackPersistentLastResult = -93;
        }
    }

    if (g_animParamPersistentPreset != 0)
    {
        RED4ext::CName inputName;
        if (ResolveAnimFloatPreset(g_animParamPersistentPreset, inputName))
        {
            g_animParamPersistentLastResult = SetPlayerAnimatedParameterValue(inputName, g_animParamPersistentValue);
        }
        else
        {
            g_animParamPersistentLastResult = -52;
        }
    }

    if (g_animInputTestMode == 0)
    {
        if (aOut)
        {
            if (g_rootGraphVectorPersistentPreset != 0)
                *aOut = g_rootGraphVectorPersistentLastResult;
            else if (g_rootGraphFloatPersistentPreset != 0)
                *aOut = g_rootGraphFloatPersistentLastResult;
            else if (g_rootMetaRigTrackPersistentPreset != 0)
                *aOut = g_rootMetaRigTrackPersistentLastResult;
            else if (g_rootLiveTrackPersistentPreset != 0)
                *aOut = g_rootLiveTrackPersistentLastResult;
            else if (g_animParamPersistentPreset != 0)
                *aOut = g_animParamPersistentLastResult;
            else
                *aOut = 0;
        }
        return;
    }

    RED4ext::Vector4 left{};
    RED4ext::Vector4 right{};
    if (!FillAnimTestPose(g_animInputTestMode, left, right))
    {
        if (aOut) *aOut = -3;
        return;
    }

    if (g_animInputTestMode >= 6)
    {
        int32_t result = -99;
        const char* modeName = "queue_unknown";

        if (g_animInputTestMode == 6)
        {
            modeName = "queue_vector";
            result = QueueVectorInputEvents(left, right);
        }
        else if (g_animInputTestMode == 7)
        {
            modeName = "queue_feature_WeaponUser";
            result = QueueFeatureInputEvent(RED4ext::CName("WeaponUser"), left, right);
        }
        else if (g_animInputTestMode == 8)
        {
            modeName = "queue_feature_AnimFeature_WeaponUser";
            result = QueueFeatureInputEvent(RED4ext::CName("AnimFeature_WeaponUser"), left, right);
        }
        else if (g_animInputTestMode == 9)
        {
            modeName = "queue_feature_animAnimFeature_WeaponUser";
            result = QueueFeatureInputEvent(RED4ext::CName("animAnimFeature_WeaponUser"), left, right);
        }

        AppendAnimTestLog(modeName, result, left, right);
        if (aOut) *aOut = result;
        return;
    }

    auto* controller = FindPlayerAnimationController();
    if (!controller)
    {
        if (aOut) *aOut = -1;
        return;
    }

    if (g_animInputTestMode >= 3)
    {
        RED4ext::CName featureName("WeaponUser");
        if (g_animInputTestMode == 4)
            featureName = RED4ext::CName("AnimFeature_WeaponUser");
        else if (g_animInputTestMode == 5)
            featureName = RED4ext::CName("animAnimFeature_WeaponUser");

        const int32_t result = ApplyWeaponUserFeature(controller, featureName, left, right);
        AppendAnimTestLog("apply_feature", result, left, right);
        if (aOut) *aOut = result;
        return;
    }

    auto* cls = controller->GetType();
    auto* func = cls ? cls->GetFunction("SetInputVector") : nullptr;
    if (!func)
    {
        if (aOut) *aOut = -2;
        return;
    }

    RED4ext::CName leftKey("ikLeftHandLocalPosition");
    RED4ext::CName rightKey("ikRightHandLocalPosition");

    RED4ext::StackArgs_t leftArgs;
    leftArgs.emplace_back(nullptr, &leftKey);
    leftArgs.emplace_back(nullptr, &left);
    RED4ext::ExecuteFunction(controller, func, nullptr, leftArgs);

    RED4ext::StackArgs_t rightArgs;
    rightArgs.emplace_back(nullptr, &rightKey);
    rightArgs.emplace_back(nullptr, &right);
    RED4ext::ExecuteFunction(controller, func, nullptr, rightArgs);

    AppendAnimTestLog("set_input_vector", 1, left, right);
    if (aOut) *aOut = 1;
}

void DumpAnimControllerListeners(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;

    std::ofstream out("C:\\Users\\dariulone\\Desktop\\CyberpunkVRPort\\anim_controller_listeners.txt", std::ios::trunc);
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

    std::ofstream out("C:\\Users\\dariulone\\Desktop\\CyberpunkVRPort\\anim_controller_function_details.txt", std::ios::trunc);
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

    std::ofstream out("C:\\Users\\dariulone\\Desktop\\CyberpunkVRPort\\anim_interesting_class_properties.txt", std::ios::trunc);
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

    std::ofstream out("C:\\Users\\dariulone\\Desktop\\CyberpunkVRPort\\animation_system_candidates.txt", std::ios::trunc);
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

    std::ofstream out("C:\\Users\\dariulone\\Desktop\\CyberpunkVRPort\\player_animated_object_runtime.txt", std::ios::trunc);
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

    std::ofstream out("C:\\Users\\dariulone\\Desktop\\CyberpunkVRPort\\root_metarig_tracks.txt", std::ios::trunc);
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

    std::ofstream out("C:\\Users\\dariulone\\Desktop\\CyberpunkVRPort\\root_animated_object_float_candidates.txt", std::ios::trunc);
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

void TestRootMetaRigTrackPreset(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    int32_t mode = 0;
    float value = 0.0f;
    RED4ext::GetParameter(aFrame, &mode);
    RED4ext::GetParameter(aFrame, &value);
    aFrame->code++;

    RED4ext::CName name;
    if (!ResolveRootMetaRigTrackPreset(mode, name))
    {
        if (aOut) *aOut = -84;
        return;
    }

    const int32_t result = SetRootMetaRigTrackValue(name, value);
    AppendRootMetaRigTrackLog("oneshot", name, value, result);
    if (aOut) *aOut = result;
}

void SetRootMetaRigTrackPresetPersistent(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    int32_t mode = 0;
    float value = 0.0f;
    RED4ext::GetParameter(aFrame, &mode);
    RED4ext::GetParameter(aFrame, &value);
    aFrame->code++;

    g_rootMetaRigTrackPersistentPreset = mode;
    g_rootMetaRigTrackPersistentValue = value;
    g_rootMetaRigTrackPersistentLastResult = 0;
    if (aOut) *aOut = 1;
}

void GetRootMetaRigTrackPersistentLastResult(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    if (aOut) *aOut = g_rootMetaRigTrackPersistentLastResult;
}

void TestRootLiveTrackPreset(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    int32_t mode = 0;
    float value = 0.0f;
    int32_t arrayMode = 0;
    RED4ext::GetParameter(aFrame, &mode);
    RED4ext::GetParameter(aFrame, &value);
    RED4ext::GetParameter(aFrame, &arrayMode);
    aFrame->code++;

    RED4ext::CName name;
    if (!ResolveRootMetaRigTrackPreset(mode, name))
    {
        if (aOut) *aOut = -84;
        return;
    }

    const int32_t result = SetRootLiveTrackValue(name, value, arrayMode);
    AppendRootMetaRigTrackLog("live_oneshot", name, value, result);
    if (aOut) *aOut = result;
}

void SetRootLiveTrackPresetPersistent(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    int32_t mode = 0;
    float value = 0.0f;
    int32_t arrayMode = 0;
    RED4ext::GetParameter(aFrame, &mode);
    RED4ext::GetParameter(aFrame, &value);
    RED4ext::GetParameter(aFrame, &arrayMode);
    aFrame->code++;

    g_rootLiveTrackPersistentPreset = mode;
    g_rootLiveTrackPersistentValue = value;
    g_rootLiveTrackPersistentArrayMode = arrayMode;
    g_rootLiveTrackPersistentLastResult = 0;
    if (aOut) *aOut = 1;
}

void GetRootLiveTrackPersistentLastResult(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    if (aOut) *aOut = g_rootLiveTrackPersistentLastResult;
}

void ReadRootLiveTrackPreset(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    int32_t mode = 0;
    int32_t arrayMode = 1;
    RED4ext::GetParameter(aFrame, &mode);
    RED4ext::GetParameter(aFrame, &arrayMode);
    aFrame->code++;

    RED4ext::CName name;
    if (!ResolveRootMetaRigTrackPreset(mode, name))
    {
        if (aOut) *aOut = -84;
        return;
    }

    if (aOut) *aOut = ReadRootLiveTrackValue(name, arrayMode);
}

void RunIKTargetAddTest(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    int32_t mode = 0;
    RED4ext::GetParameter(aFrame, &mode);
    aFrame->code++;

    const char* leftPart = nullptr;
    const char* rightPart = nullptr;
    const char* label = nullptr;

    switch (mode)
    {
    case 1:
        label = "LeftHand_RightHand";
        leftPart = "LeftHand";
        rightPart = "RightHand";
        break;
    case 2:
        label = "l_hand_r_hand";
        leftPart = "l_hand";
        rightPart = "r_hand";
        break;
    case 3:
        label = "IK_Pad_Shoulders";
        leftPart = "IK_Pad_LeftShoulder";
        rightPart = "IK_Pad_RightShoulder";
        break;
    case 4:
        label = "LeftShoulder_RightShoulder";
        leftPart = "LeftShoulder";
        rightPart = "RightShoulder";
        break;
    default:
        if (aOut) *aOut = -30;
        return;
    }

    auto* playerEntity = FindPlayerEntity();
    auto* transform = playerEntity ? playerEntity->transformComponent : nullptr;
    if (!playerEntity || !transform)
    {
        if (aOut) *aOut = -31;
        return;
    }

    const RED4ext::Vector4 playerPos = transform->worldTransform.Position.AsVector4();
    RED4ext::Vector4 leftPos = playerPos;
    RED4ext::Vector4 rightPos = playerPos;
    leftPos.X -= 0.35f; leftPos.Y += 0.30f; leftPos.Z += 1.15f; leftPos.W = 1.0f;
    rightPos.X += 0.35f; rightPos.Y += 0.30f; rightPos.Z += 1.15f; rightPos.W = 1.0f;

    auto leftProvider = CreateStaticPositionProvider(leftPos);
    auto rightProvider = CreateStaticPositionProvider(rightPos);
    auto orientationProvider = CreateStaticOrientationProvider();

    std::ofstream log("C:\\Users\\dariulone\\Desktop\\CyberpunkVRPort\\ik_target_add_test_log.txt", std::ios::app);
    log << "mode=" << mode << " label=" << label << "\n";
    log << "playerPos=(" << playerPos.X << ", " << playerPos.Y << ", " << playerPos.Z << ", " << playerPos.W << ")\n";
    log << "leftPos=(" << leftPos.X << ", " << leftPos.Y << ", " << leftPos.Z << ", " << leftPos.W << ") rightPos=("
        << rightPos.X << ", " << rightPos.Y << ", " << rightPos.Z << ", " << rightPos.W << ")\n";

    if (!leftProvider || !rightProvider)
    {
        log << "providerCreation=failed\n";
        if (aOut) *aOut = -32;
        return;
    }

    auto* rtti = RED4ext::CRTTISystem::Get();
    auto* cls = rtti ? rtti->GetClass("entIKTargetAddEvent") : nullptr;
    if (!cls)
    {
        if (aOut) *aOut = -33;
        return;
    }

    auto* leftInstance = cls->CreateInstance(true);
    auto* rightInstance = cls->CreateInstance(true);
    if (!leftInstance || !rightInstance)
    {
        if (aOut) *aOut = -34;
        return;
    }

    cls->InitializeProperties(leftInstance);
    cls->InitializeProperties(rightInstance);

    auto* leftEvt = reinterpret_cast<RED4ext::ent::IKTargetAddEvent*>(leftInstance);
    auto* rightEvt = reinterpret_cast<RED4ext::ent::IKTargetAddEvent*>(rightInstance);

    leftEvt->targetPositionProvider = leftProvider;
    leftEvt->bodyPart = RED4ext::CName(leftPart);
    leftEvt->orientationProvider = orientationProvider;
    leftEvt->request.weightPosition = 1.0f;
    leftEvt->request.weightOrientation = 0.0f;
    leftEvt->request.transitionIn = 0.0f;
    leftEvt->request.transitionOut = 0.0f;
    leftEvt->request.priority = 100;

    rightEvt->targetPositionProvider = rightProvider;
    rightEvt->bodyPart = RED4ext::CName(rightPart);
    rightEvt->orientationProvider = orientationProvider;
    rightEvt->request.weightPosition = 1.0f;
    rightEvt->request.weightOrientation = 0.0f;
    rightEvt->request.transitionIn = 0.0f;
    rightEvt->request.transitionOut = 0.0f;
    rightEvt->request.priority = 100;

    const bool leftQueued = QueuePlayerEvent(static_cast<RED4ext::Handle<RED4ext::red::Event>>(RED4ext::Handle<RED4ext::ent::IKTargetAddEvent>(leftEvt)));
    const bool rightQueued = QueuePlayerEvent(static_cast<RED4ext::Handle<RED4ext::red::Event>>(RED4ext::Handle<RED4ext::ent::IKTargetAddEvent>(rightEvt)));

    log << "leftQueued=" << (leftQueued ? 1 : 0) << " rightQueued=" << (rightQueued ? 1 : 0) << "\n";
    log << "leftOutRef id=" << leftEvt->outIKTargetRef.id << " part=" << leftEvt->outIKTargetRef.part.ToString() << "\n";
    log << "rightOutRef id=" << rightEvt->outIKTargetRef.id << " part=" << rightEvt->outIKTargetRef.part.ToString() << "\n";
    AppendPlayerControllerIKState(log);
    log << "--------------------------------------------------\n";

    if (aOut) *aOut = (leftQueued ? 1 : 0) + (rightQueued ? 1 : 0);
}

void TestAnimFloatInput(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    RED4ext::CName inputName;
    float value = 0.0f;
    int32_t route = 0;
    RED4ext::GetParameter(aFrame, &inputName);
    RED4ext::GetParameter(aFrame, &value);
    RED4ext::GetParameter(aFrame, &route);
    aFrame->code++;

    int32_t result = -45;
    const char* routeName = "unknown";
    if (route == 0)
    {
        routeName = "direct";
        result = SetFloatInputDirect(inputName, value);
    }
    else if (route == 1)
    {
        routeName = "queue";
        result = QueueFloatInputEvent(inputName, value);
    }

    AppendAnimFloatTestLog(routeName, inputName, value, result);
    if (aOut) *aOut = result;
}

void TestAnimFloatPreset(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    int32_t mode = 0;
    float value = 0.0f;
    int32_t route = 0;
    RED4ext::GetParameter(aFrame, &mode);
    RED4ext::GetParameter(aFrame, &value);
    RED4ext::GetParameter(aFrame, &route);
    aFrame->code++;

    RED4ext::CName inputName;
    if (!ResolveAnimFloatPreset(mode, inputName))
    {
        if (aOut) *aOut = -46;
        return;
    }

    int32_t result = -47;
    const char* routeName = "unknown_preset";
    if (route == 0)
    {
        routeName = "direct_preset";
        result = SetFloatInputDirect(inputName, value);
    }
    else if (route == 1)
    {
        routeName = "queue_preset";
        result = QueueFloatInputEvent(inputName, value);
    }

    AppendAnimFloatTestLog(routeName, inputName, value, result);
    if (aOut) *aOut = result;
}

void SetPlayerAnimParameter(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    RED4ext::CName inputName;
    float value = 0.0f;
    RED4ext::GetParameter(aFrame, &inputName);
    RED4ext::GetParameter(aFrame, &value);
    aFrame->code++;

    const int32_t result = SetPlayerAnimatedParameterValue(inputName, value);
    AppendDirectAnimParamLog("oneshot", inputName, value, result);
    if (aOut) *aOut = result;
}

void SetPlayerAnimParameterPreset(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    int32_t mode = 0;
    float value = 0.0f;
    RED4ext::GetParameter(aFrame, &mode);
    RED4ext::GetParameter(aFrame, &value);
    aFrame->code++;

    RED4ext::CName inputName;
    if (!ResolveAnimFloatPreset(mode, inputName))
    {
        if (aOut) *aOut = -51;
        return;
    }

    const int32_t result = SetPlayerAnimatedParameterValue(inputName, value);
    AppendDirectAnimParamLog("oneshot_preset", inputName, value, result);
    if (aOut) *aOut = result;
}

void SetPlayerAnimParameterPersistentPreset(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    int32_t mode = 0;
    float value = 0.0f;
    RED4ext::GetParameter(aFrame, &mode);
    RED4ext::GetParameter(aFrame, &value);
    aFrame->code++;

    g_animParamPersistentPreset = mode;
    g_animParamPersistentValue = value;
    g_animParamPersistentLastResult = 0;
    if (aOut) *aOut = 1;
}

void GetPlayerAnimParameterPersistentLastResult(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    if (aOut) *aOut = g_animParamPersistentLastResult;
}

void RestoreVRFppArms(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;

    RED4ext::ScriptGameInstance gameInstance;
    RED4ext::Handle<RED4ext::IScriptable> playerHandle;
    RED4ext::ExecuteGlobalFunction("GetPlayer;GameInstance", &playerHandle, gameInstance);
    if (!playerHandle) { if (aOut) *aOut = -1; return; }

    auto* playerEntity = reinterpret_cast<RED4ext::ent::Entity*>(playerHandle.instance);
    if (!playerEntity) { if (aOut) *aOut = -2; return; }

    const RED4ext::CName placedComponentName("entIPlacedComponent");
    const RED4ext::CName meshComponentName("entMeshComponent");
    const RED4ext::CName skinnedMeshComponentName("entSkinnedMeshComponent");
    const RED4ext::CName garmentSkinnedMeshComponentName("entGarmentSkinnedMeshComponent");

    int32_t restored = 0;
    for (auto& componentHandle : playerEntity->components) {
        auto* component = componentHandle.instance;
        if (!component) continue;
        const char* componentName = component->name.ToString();
        if (!IsLikelyFppArmComponent(componentName)) continue;

        RED4ext::CClass* type = component->GetType();
        if (!type || !ClassIsA(type, placedComponentName)) continue;
        auto* placed = reinterpret_cast<RED4ext::ent::IPlacedComponent*>(component);

        component->isEnabled = true;
        if (ClassIsA(type, skinnedMeshComponentName) || ClassIsA(type, garmentSkinnedMeshComponentName)) {
            auto* skinned = reinterpret_cast<RED4ext::ent::SkinnedMeshComponent*>(component);
            skinned->chunkMask = 0xFFFFFFFFFFFFFFFFull;
        }
        if (ClassIsA(type, meshComponentName)) {
            auto* mesh = reinterpret_cast<RED4ext::ent::MeshComponent*>(component);
            mesh->chunkMask = 0xFFFFFFFFFFFFFFFFull;
            mesh->visualScale.X = 1.0f;
            mesh->visualScale.Y = 1.0f;
            mesh->visualScale.Z = 1.0f;
        }
        placed->localTransform.Position.x.Bits = 0;
        placed->localTransform.Position.y.Bits = 0;
        placed->localTransform.Position.z.Bits = 0;
        ++restored;
    }

    // Reset debug state too, so no later call re-hides things unexpectedly.
    g_chunkDebugEnabled = false;
    g_chunkDebugWasEnabled = false;
    if (aOut) *aOut = restored;
}

void ForceHideVRFppArms(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    // VRFPP hide path disabled in this build.
    // For backward compatibility, calling this function now restores the arms.
    RestoreVRFppArms(aContext, aFrame, aOut, a4);
}



static const char* ClassifyQword(uint64_t v) {
    if (v >= 0x0000010000000000ull && v < 0x0000700000000000ull) return "HEAP";
    if (v >= 0x00007F0000000000ull && v < 0x00008000000000ull)   return "CODE/VTBL";
    return "";
}

static uint64_t SafeReadQword(uint8_t* base, size_t off) {
    __try { return *reinterpret_cast<uint64_t*>(base + off); }
    __except(EXCEPTION_EXECUTE_HANDLER) { return 0; }
}

static float SafeReadFloat(uint8_t* base, size_t off) {
    __try { return *reinterpret_cast<float*>(base + off); }
    __except(EXCEPTION_EXECUTE_HANDLER) { return 0.0f; }
}

static uint32_t SafeReadU32(uint8_t* base, size_t off) {
    __try { return *reinterpret_cast<uint32_t*>(base + off); }
    __except(EXCEPTION_EXECUTE_HANDLER) { return 0; }
}

static RED4ext::CClass* SafeGetObjectType(void* aPtr)
{
    __try
    {
        auto* obj = reinterpret_cast<RED4ext::ISerializable*>(aPtr);
        return obj ? obj->GetType() : nullptr;
    }
    __except (EXCEPTION_EXECUTE_HANDLER)
    {
        return nullptr;
    }
}

static RED4ext::world::AnimationSystem* ScanForAnimationSystemInBlock(uint8_t* aBase, size_t aSize, std::ofstream* aOut)
{
    if (!aBase)
        return nullptr;

    for (size_t off = 0; off + 8 <= aSize; off += 8)
    {
        uint64_t p = SafeReadQword(aBase, off);
        if (std::strcmp(ClassifyQword(p), "HEAP") != 0)
            continue;

        auto* type = SafeGetObjectType(reinterpret_cast<void*>(p));
        const char* typeName = type ? type->name.ToString() : nullptr;
        if (typeName && aOut && ContainsInsensitive(typeName, "animationsystem"))
        {
            *aOut << "direct off=0x" << std::hex << off << " ptr=0x" << p << std::dec << " type=" << typeName << "\n";
        }

        if (type && type->name == "worldAnimationSystem")
            return reinterpret_cast<RED4ext::world::AnimationSystem*>(p);

        if (type && type->name == "worldAnimationSystemScriptInterface")
        {
            auto* iface = reinterpret_cast<RED4ext::world::AnimationSystemScriptInterface*>(p);
            if (iface->animationSystem)
                return iface->animationSystem;
        }

        uint8_t* nested = reinterpret_cast<uint8_t*>(p);
        for (size_t inner = 0; inner + 8 <= 0x200; inner += 8)
        {
            uint64_t q = SafeReadQword(nested, inner);
            if (std::strcmp(ClassifyQword(q), "HEAP") != 0)
                continue;

            auto* innerType = SafeGetObjectType(reinterpret_cast<void*>(q));
            const char* innerTypeName = innerType ? innerType->name.ToString() : nullptr;
            if (innerTypeName && aOut && ContainsInsensitive(innerTypeName, "animationsystem"))
            {
                *aOut << "nested parentOff=0x" << std::hex << off << " innerOff=0x" << inner << " ptr=0x" << q << std::dec
                    << " type=" << innerTypeName << "\n";
            }

            if (innerType && innerType->name == "worldAnimationSystem")
                return reinterpret_cast<RED4ext::world::AnimationSystem*>(q);

            if (innerType && innerType->name == "worldAnimationSystemScriptInterface")
            {
                auto* iface = reinterpret_cast<RED4ext::world::AnimationSystemScriptInterface*>(q);
                if (iface->animationSystem)
                    return iface->animationSystem;
            }
        }
    }

    return nullptr;
}

void DumpAnimMemory(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;

    RED4ext::ScriptGameInstance gameInstance;
    RED4ext::Handle<RED4ext::IScriptable> playerHandle;
    RED4ext::ExecuteGlobalFunction("GetPlayer;GameInstance", &playerHandle, gameInstance);
    if (!playerHandle) { if (aOut) *aOut = -1; return; }
    auto* playerEntity = reinterpret_cast<RED4ext::ent::Entity*>(playerHandle.instance);
    if (!playerEntity) { if (aOut) *aOut = -2; return; }

    std::ofstream out("C:\\Users\\dariulone\\Desktop\\CyberpunkVRPort\\anim_memory_dump.txt", std::ios::trunc);
    int dumped = 0;

    for (auto& componentHandle : playerEntity->components) {
        auto* component = componentHandle.instance;
        if (!component) continue;
        RED4ext::CClass* type = component->GetType();
        if (!type) continue;
        
        if (type->name == "entAnimatedComponent" && std::string(component->name.ToString()) == "root") {
            const char* nm = component->name.ToString();
            out << "==================================================\n";
            out << "COMPONENT name=" << nm << " ptr=0x" << std::hex << (uintptr_t)component << " type=" << type->name.ToString() << "\n";
            out << "==================================================\n";
            
            uint8_t* base = reinterpret_cast<uint8_t*>(component);
            
            // Level 1: Find Heap Pointers
            for (size_t off1 = 0x130; off1 < 0x2B0; off1 += 8) {
                uint64_t ptr1 = SafeReadQword(base, off1);
                if (std::string(ClassifyQword(ptr1)) == "HEAP") {
                    
                    // Scan inside ptr1
                    uint8_t* b1 = reinterpret_cast<uint8_t*>(ptr1);
                    for (size_t off2 = 0; off2 < 0x400; off2 += 8) {
                        uint64_t ptr2 = SafeReadQword(b1, off2);
                        if (std::string(ClassifyQword(ptr2)) == "HEAP") {
                            
                            // Scan inside ptr2 for QsTransform array
                            uint8_t* b2 = reinterpret_cast<uint8_t*>(ptr2);
                            int score = 0;
                            for (size_t i = 0; i < 10; ++i) {
                                float w = SafeReadFloat(b2, (i * 32) + 12);
                                if (w >= 0.99f && w <= 1.01f) {
                                    score++;
                                }
                            }
                            
                            if (score >= 3) {
                                out << "!!! FOUND BONE ARRAY CANDIDATE !!!\n";
                                out << "Component Base + 0x" << std::hex << off1 << " -> + 0x" << off2 << " -> ARRAY!\n";
                                out << "Score: " << std::dec << score << "/10 bones matched W=1.0\n\n";
                            }
                        }
                    }
                    
                    // Also check if ptr1 ITSELF is the array
                    int score = 0;
                    for (size_t i = 0; i < 10; ++i) {
                        float w = SafeReadFloat(b1, (i * 32) + 12);
                        if (w >= 0.99f && w <= 1.01f) {
                            score++;
                        }
                    }
                    if (score >= 3) {
                        out << "!!! FOUND BONE ARRAY CANDIDATE (Direct) !!!\n";
                        out << "Component Base + 0x" << std::hex << off1 << " -> ARRAY!\n";
                        out << "Score: " << std::dec << score << "/10 bones matched W=1.0\n\n";
                    }
                }
            }
            ++dumped;
        }
    }

    out.close();
    if (aOut) *aOut = dumped;
}

void SetVRBoneDebugIndex(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, void* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(aOut); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t index = -1;
    RED4ext::GetParameter(aFrame, &index);
    aFrame->code++;
    g_CalibrationBoneIndex = index;
}

// Wires the post-eval bone-write hook to a specific player animated component.
// Until this is set, g_PlayerAnimComponent stays null and the hook's filter
// (rcx == g_PlayerAnimComponent) never matches, so the bone write never runs.
// mode: 0=disable, 1=root, 2=deformations, 3=shadow. Returns 1 if set, else 0.
void ArmVRBoneHook(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t mode = 0;
    RED4ext::GetParameter(aFrame, &mode);
    aFrame->code++;

    if (mode == 0) {
        g_PlayerAnimComponent = nullptr;
        if (aOut) *aOut = 1;
        return;
    }

    // modes 1-3: component handle from playerEntity->components
    // modes 4-6: LIVE bucket component from worldAnimationSystem (the updated instance)
    void* comp = nullptr;
    switch (mode) {
        case 1: comp = FindPlayerAnimatedComponentByName("root"); break;
        case 2: comp = FindPlayerAnimatedComponentByName("deformations"); break;
        case 3: comp = FindPlayerAnimatedComponentByName("shadow"); break;
        case 4: comp = FindPlayerBucketAnimatedComponent("root"); break;
        case 5: comp = FindPlayerBucketAnimatedComponent("deformations"); break;
        case 6: comp = FindPlayerBucketAnimatedComponent("shadow"); break;
        default: comp = nullptr; break;
    }
    g_PlayerAnimComponent = comp;
    if (aOut) *aOut = comp ? 1 : 0;
}

// Returns the currently armed bone-hook component pointer as low 32 bits (debug),
// or 0 if not armed.
void GetVRBoneHookArmed(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    if (aOut) *aOut = static_cast<int32_t>(reinterpret_cast<uintptr_t>(g_PlayerAnimComponent) & 0xFFFFFFFF);
}

// mode: 0=total hook calls, 1=filter matches (rcx==player comp), 2=bone-write reached,
//       3=skeletal calls captured.
void GetVRBoneHookStats(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t mode = 0;
    RED4ext::GetParameter(aFrame, &mode);
    aFrame->code++;
    uint64_t v = (mode == 1) ? g_hookMatchCalls : (mode == 2) ? g_hookBoneWrites
               : (mode == 3) ? g_hookSkeletalCalls : g_hookTotalCalls;
    if (v > 0x7FFFFFFF) v = 0x7FFFFFFF;
    if (aOut) *aOut = static_cast<int32_t>(v);
}

// Enables/disables capture mode and resets the captured list.
void SetVRBoneHookCapture(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t on = 0;
    RED4ext::GetParameter(aFrame, &on);
    aFrame->code++;
    if (on) { g_capturedCount = 0; g_hookSkeletalCalls = 0; }
    g_hookCapture = on ? 1 : 0;
    if (aOut) *aOut = 1;
}

void GetVRBoneHookCapturedCount(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    if (aOut) *aOut = g_capturedCount;
}

// Returns the captured skeletal rcx low-32 at index (compare against GetVRBoneHookArmed).
void GetVRBoneHookCapturedRcx(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t index = 0;
    RED4ext::GetParameter(aFrame, &index);
    aFrame->code++;
    if (index < 0 || index >= 32 || index >= g_capturedCount) { if (aOut) *aOut = 0; return; }
    if (aOut) *aOut = static_cast<int32_t>(g_capturedRcx[index]);
}

// Reads the bone count (DynArray size) of a captured skeletal component, so we can
// identify the player among the captured list by bone count (player root = 619).
// rcx -> modelInstance(+0x168) -> bone DynArray (+0xE0 ptr, +0xE8 size).
void GetVRBoneHookCapturedBoneCount(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t index = 0;
    RED4ext::GetParameter(aFrame, &index);
    aFrame->code++;
    if (aOut) *aOut = -1;
    if (index < 0 || index >= 32 || index >= g_capturedCount) return;
    void* rcx = reinterpret_cast<void*>(g_capturedFull[index]);
    if (!VRIK_IsReadable(rcx, 0x170)) return;
    void* mi = *reinterpret_cast<void**>(reinterpret_cast<uint8_t*>(rcx) + 0x168);
    if (!VRIK_IsReadable(mi, 0xF0)) return;
    uint32_t size = *reinterpret_cast<uint32_t*>(reinterpret_cast<uint8_t*>(mi) + 0xE8);
    if (aOut) *aOut = static_cast<int32_t>(size);
}

// Arms the bone-hook with a captured component pointer directly (bypasses bucket
// resolution). Use after identifying the player by bone count.
void ArmVRBoneHookByCaptureIndex(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t index = 0;
    RED4ext::GetParameter(aFrame, &index);
    aFrame->code++;
    if (index < 0 || index >= 32 || index >= g_capturedCount) { if (aOut) *aOut = 0; return; }
    g_PlayerAnimComponent = reinterpret_cast<void*>(g_capturedFull[index]);
    if (aOut) *aOut = 1;
}

// Prints the player body component's bone buffer base address so it can be fed to a
// Cheat Engine write-breakpoint ("find what writes to this address") -> reveals the
// real MetaRig->bone pose-apply function to hook for VR hands.
// component(+0x168)=modelInstance, (+0xE0)=bone DynArray entries ptr (the float buffer).
void GetPlayerBoneBufferAddress(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    if (aOut) *aOut = 0;

    std::ofstream out("C:\\Users\\dariulone\\Desktop\\CyberpunkVRPort\\player_bone_buffer.txt", std::ios::trunc);
    void* comp = FindPlayerBucketAnimatedComponent("root");
    if (!comp || !VRIK_IsReadable(comp, 0x170)) {
        if (out.is_open()) out << "component not resolved or unreadable comp=0x" << std::hex
                               << reinterpret_cast<uintptr_t>(comp) << "\n";
        if (aOut) *aOut = -1;
        return;
    }
    void* modelInstance = *reinterpret_cast<void**>(reinterpret_cast<uint8_t*>(comp) + 0x168);
    if (!VRIK_IsReadable(modelInstance, 0xF0)) {
        if (out.is_open()) out << "modelInstance unreadable mi=0x" << std::hex
                               << reinterpret_cast<uintptr_t>(modelInstance) << "\n";
        if (aOut) *aOut = -2;
        return;
    }
    if (out.is_open()) {
        out << std::hex
            << "component    = 0x" << reinterpret_cast<uintptr_t>(comp) << "\n"
            << "modelInstance= 0x" << reinterpret_cast<uintptr_t>(modelInstance) << std::dec << "\n\n";

        // Three candidate pose arrays found by the session-1 scan off modelInstance.
        const uint32_t candOffsets[] = { 0xE0, 0x208, 0x220 };
        for (uint32_t co : candOffsets) {
            void* arr = *reinterpret_cast<void**>(reinterpret_cast<uint8_t*>(modelInstance) + co);
            uint32_t szA = *reinterpret_cast<uint32_t*>(reinterpret_cast<uint8_t*>(modelInstance) + co + 0x8);
            uint32_t szB = *reinterpret_cast<uint32_t*>(reinterpret_cast<uint8_t*>(modelInstance) + co + 0xC);
            out << "candidate modelInstance+0x" << std::hex << co
                << " -> buffer=0x" << reinterpret_cast<uintptr_t>(arr) << std::dec
                << "  size@+8=" << szA << " size@+C=" << szB << "\n";
            if (VRIK_IsReadable(arr, 0x40)) {
                float* f = reinterpret_cast<float*>(arr);
                out << "   floats:";
                for (int i = 0; i < 8; ++i) out << " " << f[i];
                out << "\n";
            } else {
                out << "   (buffer not readable)\n";
            }
        }
    }
    void* boneBuffer = *reinterpret_cast<void**>(reinterpret_cast<uint8_t*>(modelInstance) + 0xE0);
    if (aOut) *aOut = static_cast<int32_t>(reinterpret_cast<uintptr_t>(boneBuffer) & 0xFFFFFFFF);
}

// ---- Pose-apply hook (sub_14017DDB4) control ----

// Installs the MinHook on the pose-apply function (module+0x17DDB4).
void InstallVRAnimPoseHook(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    bool ok1 = InstallVRIKMinHook();
    bool ok2 = InstallAnimPoseHook();
    if (aOut) *aOut = (ok1 && ok2) ? 1 : 0;
}

// Resolves the player's live track buffers (a2[7][3] candidates) so the hook can
// identify the player call. Returns bitmask: 1=bufA set, 2=bufB set.
void ArmVRAnimPosePlayer(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    if (aOut) *aOut = 0;

    auto* animObj = FindPlayerAnimatedObjectByComponentName("root");
    if (!animObj || !VRIK_IsReadable(animObj, 0x40)) { if (aOut) *aOut = -1; return; }
    uint8_t* base = reinterpret_cast<uint8_t*>(animObj);

    g_PlayerTrackBufA = 0;
    g_PlayerTrackBufB = 0;

    // A: *(*(animObj+0x8) + 0x40)
    void* ownerA = *reinterpret_cast<void**>(base + 0x8);
    if (VRIK_IsReadable(ownerA, 0x48))
        g_PlayerTrackBufA = *reinterpret_cast<uintptr_t*>(reinterpret_cast<uint8_t*>(ownerA) + 0x40);

    // B: *(*(animObj+0x18) + 0x18)
    void* ownerB = *reinterpret_cast<void**>(base + 0x18);
    if (VRIK_IsReadable(ownerB, 0x20))
        g_PlayerTrackBufB = *reinterpret_cast<uintptr_t*>(reinterpret_cast<uint8_t*>(ownerB) + 0x18);

    if (aOut) *aOut = (g_PlayerTrackBufA ? 1 : 0) | (g_PlayerTrackBufB ? 2 : 0);
}

void SetVRAnimPoseDebug(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t mode = 0;
    RED4ext::GetParameter(aFrame, &mode);
    aFrame->code++;
    g_AnimPoseDebug = mode;
    if (aOut) *aOut = 1;
}

// mode 0=total calls, 1=player-match calls, 2=last matched bone buffer low-32.
void GetVRAnimPoseStats(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t mode = 0;
    RED4ext::GetParameter(aFrame, &mode);
    aFrame->code++;
    uint64_t v = (mode == 1) ? g_AnimPoseMatchCalls
               : (mode == 2) ? g_AnimPoseLastBoneBuf
               : g_AnimPoseTotalCalls;
    if (aOut) *aOut = static_cast<int32_t>(v & 0xFFFFFFFF);
}

// Single-bone calibration: pushes only bone[index] translation by magnitude on every
// axis (debug mode 2). Sweep indices to find which one is a hand.
void SetVRAnimPoseBoneTest(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t index = -1;
    float   mag = 1.0f;
    RED4ext::GetParameter(aFrame, &index);
    RED4ext::GetParameter(aFrame, &mag);
    aFrame->code++;
    g_AnimPoseTestBone = index;
    g_AnimPoseTestMag = mag;
    g_AnimPoseDebug = (index >= 0) ? 2 : 0;
    if (aOut) *aOut = 1;
}

// Dumps every metaRig bone name with its index so we can locate the hand bones.
void DumpPlayerBoneNames(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    if (aOut) *aOut = 0;

    std::ofstream out("C:\\Users\\dariulone\\Desktop\\CyberpunkVRPort\\player_bone_names.txt", std::ios::trunc);
    auto* animObj = FindPlayerAnimatedObjectByComponentName("root");
    auto* metaRig = animObj ? animObj->metaRig : nullptr;
    if (!metaRig) { if (aOut) *aOut = -1; return; }

    const uint32_t n = metaRig->boneNames.Size();
    if (out.is_open()) {
        out << "boneCount=" << n << "\n";
        for (uint32_t i = 0; i < n; ++i) {
            const char* name = metaRig->boneNames[i].ToString();
            out << i << "\t" << (name ? name : "<null>") << "\n";
        }
    }
    if (aOut) *aOut = static_cast<int32_t>(n);
}

// VR Transform data from Lua (Camera and Player Model Space)




void SetVRBindMode(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t mode = 0;
    RED4ext::GetParameter(aFrame, &mode);
    aFrame->code++;
    g_VRBind = mode;
    if (aOut) *aOut = 1;
}

void SetVRBindParams(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    float scale = 1.0f, x = 0.0f, y = 0.0f, z = 0.0f;
    int32_t axis = 1;
    RED4ext::GetParameter(aFrame, &scale);
    RED4ext::GetParameter(aFrame, &x);
    RED4ext::GetParameter(aFrame, &y);
    RED4ext::GetParameter(aFrame, &z);
    RED4ext::GetParameter(aFrame, &axis);
    aFrame->code++;
    
    g_VRBindScale = scale;
    g_VRBindOffX = x;
    g_VRBindOffY = y;
    g_VRBindOffZ = z;
    g_VRBindAxis = axis;
    
    if (aOut) *aOut = 1;
}

void SetVRBindBones(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t leftIdx = 23, rightIdx = 24;
    RED4ext::GetParameter(aFrame, &leftIdx);
    RED4ext::GetParameter(aFrame, &rightIdx);
    aFrame->code++;
    
    g_VRLeftBoneIdx = leftIdx;
    g_VRRightBoneIdx = rightIdx;
    
    if (aOut) *aOut = 1;
}

// Dump VTable specifically for entAnimatedComponent
void DumpAnimVTable(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, void* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(aOut); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    
    std::ofstream out("C:\\Users\\dariulone\\Desktop\\CyberpunkVRPort\\anim_vtable_dump.txt", std::ios::trunc);
    
    RED4ext::ScriptGameInstance gameInstance;
    RED4ext::Handle<RED4ext::IScriptable> playerHandle;
    RED4ext::ExecuteGlobalFunction("GetPlayer;GameInstance", &playerHandle, gameInstance);
    if (!playerHandle) { out << "No player\n"; return; }

    auto* playerEntity = reinterpret_cast<RED4ext::ent::Entity*>(playerHandle.instance);
    if (!playerEntity) { out << "No player entity\n"; return; }

    for (auto& componentHandle : playerEntity->components) {
        auto* component = componentHandle.instance;
        if (!component) continue;
        
        RED4ext::CClass* type = component->GetType();
        if (type && type->name == "entAnimatedComponent") {
            out << "Found entAnimatedComponent at: " << std::hex << (uintptr_t)component << "\n";
            
            uintptr_t* vtable = *reinterpret_cast<uintptr_t**>(component);
            out << "VTable Address: " << std::hex << (uintptr_t)vtable << "\n";
            
            HMODULE hMod = GetModuleHandleA("Cyberpunk2077.exe");
            uintptr_t base = (uintptr_t)hMod;
            
            for(int i = 0; i < 60; ++i) { // dump 60 just in case
                uintptr_t func = vtable[i];
                out << "  [" << std::dec << i << std::hex << "] Func: " << func << " (Cyberpunk2077.exe+" << (func - base) << ")\n";
            }
            break; 
        }
    }
    out.close();
}

void DumpAnimControllerComponents(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;

    RED4ext::ScriptGameInstance gameInstance;
    RED4ext::Handle<RED4ext::IScriptable> playerHandle;
    RED4ext::ExecuteGlobalFunction("GetPlayer;GameInstance", &playerHandle, gameInstance);
    if (!playerHandle)
    {
        if (aOut) *aOut = -1;
        return;
    }

    auto* playerEntity = reinterpret_cast<RED4ext::ent::Entity*>(playerHandle.instance);
    if (!playerEntity)
    {
        if (aOut) *aOut = -2;
        return;
    }

    std::ofstream out("C:\\Users\\dariulone\\Desktop\\CyberpunkVRPort\\anim_controller_dump.txt", std::ios::trunc);
    int dumped = 0;

    for (auto& componentHandle : playerEntity->components)
    {
        auto* component = componentHandle.instance;
        if (!component)
            continue;

        RED4ext::CClass* type = component->GetType();
        if (!type)
            continue;

        if (type->name == "entAnimationControllerComponent")
        {
            auto* controller = reinterpret_cast<RED4ext::ent::AnimationControllerComponent*>(component);
            out << "==================================================\n";
            out << "AnimationControllerComponent ptr=0x" << std::hex << reinterpret_cast<uintptr_t>(controller) << std::dec << "\n";
            out << "name=" << component->name.ToString() << " enabled=" << (component->isEnabled ? 1 : 0) << "\n";
            out << "lookAtController ptr(backref)=0x" << std::hex << reinterpret_cast<uintptr_t>(controller->lookAtController.animationControllerComponent) << std::dec << "\n";
            out << "ikTargetController ptr(backref)=0x" << std::hex << reinterpret_cast<uintptr_t>(controller->ikTargetController.animationControllerComponent) << std::dec << "\n";
            out << "ikTargetController.targetData.size=" << controller->ikTargetController.targetData.Size() << "\n";

            for (uint32_t i = 0; i < controller->ikTargetController.targetData.Size(); ++i)
            {
                const auto& target = controller->ikTargetController.targetData[i];
                out << "  [" << i << "] id=" << target.targetReference.id
                    << " part=" << target.targetReference.part.ToString()
                    << " posProvider=0x" << std::hex << reinterpret_cast<uintptr_t>(target.positionProvider.instance)
                    << " orientProvider=0x" << reinterpret_cast<uintptr_t>(target.orientationProvider.instance)
                    << std::dec << "\n";
            }

            out << "ikParams=0x" << std::hex << reinterpret_cast<uintptr_t>(controller->ikTargetController.ikParams.instance) << std::dec << "\n";
            ++dumped;
        }
    }

    out.close();
    if (aOut) *aOut = dumped;
}

void DumpRuntimeClassFunctions(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;

    auto* rtti = RED4ext::CRTTISystem::Get();
    std::ofstream out("C:\\Users\\dariulone\\Desktop\\CyberpunkVRPort\\runtime_class_functions.txt", std::ios::trunc);
    int dumped = 0;

    const char* classesToDump[] = {
        "entAnimationControllerComponent",
        "entEntity",
        "entAnimatedComponent"
    };

    for (const char* className : classesToDump)
    {
        RED4ext::CClass* cls = rtti->GetClass(className);
        if (!cls)
            continue;

        out << "==================================================\n";
        out << "CLASS " << className << "\n";
        out << "==================================================\n";

        out << "Member functions:\n";
        for (auto* func : cls->funcs)
        {
            if (!func)
                continue;
            out << "  - " << func->fullName.ToString() << "\n";
        }

        out << "Static functions:\n";
        for (auto* func : cls->staticFuncs)
        {
            if (!func)
                continue;
            out << "  - " << func->fullName.ToString() << "\n";
        }

        out << "\n";
        ++dumped;
    }

    out.close();
    if (aOut) *aOut = dumped;
}

RED4EXT_C_EXPORT void RED4EXT_CALL PostRegisterTypes() {
    auto rtti = RED4ext::CRTTISystem::Get();
    RED4ext::CBaseFunction::Flags flags = {.isNative = true, .isStatic = true};

    auto f1 = RED4ext::CGlobalFunction::Create("GetLeftVRHandValid", "GetLeftVRHandValid", &GetLeftVRHandValid);
    f1->flags = flags; f1->SetReturnType("Bool"); rtti->RegisterFunction(f1);

    auto f2 = RED4ext::CGlobalFunction::Create("GetRightVRHandValid", "GetRightVRHandValid", &GetRightVRHandValid);
    f2->flags = flags; f2->SetReturnType("Bool"); rtti->RegisterFunction(f2);

    auto f3 = RED4ext::CGlobalFunction::Create("GetLeftVRHandPos", "GetLeftVRHandPos", &GetLeftVRHandPos);
    f3->flags = flags; f3->SetReturnType("Vector4"); rtti->RegisterFunction(f3);

    auto f4 = RED4ext::CGlobalFunction::Create("GetRightVRHandPos", "GetRightVRHandPos", &GetRightVRHandPos);
    f4->flags = flags; f4->SetReturnType("Vector4"); rtti->RegisterFunction(f4);

    auto f5 = RED4ext::CGlobalFunction::Create("GetLeftVRHandRot", "GetLeftVRHandRot", &GetLeftVRHandRot);
    f5->flags = flags; f5->SetReturnType("Quaternion"); rtti->RegisterFunction(f5);

    auto f6 = RED4ext::CGlobalFunction::Create("GetRightVRHandRot", "GetRightVRHandRot", &GetRightVRHandRot);
    f6->flags = flags; f6->SetReturnType("Quaternion"); rtti->RegisterFunction(f6);

    auto f7 = RED4ext::CGlobalFunction::Create("IsVRHandLinked", "IsVRHandLinked", &IsVRHandLinked);
    f7->flags = flags; f7->SetReturnType("Bool"); rtti->RegisterFunction(f7);

    auto f8 = RED4ext::CGlobalFunction::Create("ForceHideVRFppArms", "ForceHideVRFppArms", &ForceHideVRFppArms);
    f8->flags = flags; f8->SetReturnType("Int32"); rtti->RegisterFunction(f8);

    auto f8b = RED4ext::CGlobalFunction::Create("RestoreVRFppArms", "RestoreVRFppArms", &RestoreVRFppArms);
    f8b->flags = flags; f8b->SetReturnType("Int32"); rtti->RegisterFunction(f8b);

    auto f9 = RED4ext::CGlobalFunction::Create("SetVRRightHandEntity", "SetVRRightHandEntity", &SetVRRightHandEntity);
    f9->flags = flags; f9->AddParam("handle:IScriptable", "entity"); rtti->RegisterFunction(f9);

    auto f10 = RED4ext::CGlobalFunction::Create("DumpVRFppComponents", "DumpVRFppComponents", &DumpVRFppComponents);
    f10->flags = flags; f10->SetReturnType("Int32"); rtti->RegisterFunction(f10);

    auto f11 = RED4ext::CGlobalFunction::Create("SetVRFppChunkDebugEnabled", "SetVRFppChunkDebugEnabled", &SetVRFppChunkDebugEnabled);
    f11->flags = flags; f11->AddParam("Int32", "enabled"); rtti->RegisterFunction(f11);

    auto f12 = RED4ext::CGlobalFunction::Create("SetVRFppChunkDebugComponentIndex", "SetVRFppChunkDebugComponentIndex", &SetVRFppChunkDebugComponentIndex);
    f12->flags = flags; f12->AddParam("Int32", "index"); rtti->RegisterFunction(f12);

    auto f13 = RED4ext::CGlobalFunction::Create("SetVRFppChunkDebugHand", "SetVRFppChunkDebugHand", &SetVRFppChunkDebugHand);
    f13->flags = flags; f13->AddParam("Int32", "hand"); rtti->RegisterFunction(f13);

    auto f14 = RED4ext::CGlobalFunction::Create("SetVRFppChunkDebugBits", "SetVRFppChunkDebugBits", &SetVRFppChunkDebugBits);
    f14->flags = flags;
    f14->AddParam("Int32", "bit0");
    f14->AddParam("Int32", "bit1");
    f14->AddParam("Int32", "bit2");
    f14->AddParam("Int32", "bit3");
    rtti->RegisterFunction(f14);

    auto f15 = RED4ext::CGlobalFunction::Create("SetVRBoneDebugIndex", "SetVRBoneDebugIndex", &SetVRBoneDebugIndex);
    f15->flags = flags; f15->AddParam("Int32", "index"); rtti->RegisterFunction(f15);

    auto f15a = RED4ext::CGlobalFunction::Create("ArmVRBoneHook", "ArmVRBoneHook", &ArmVRBoneHook);
    f15a->flags = flags; f15a->SetReturnType("Int32"); f15a->AddParam("Int32", "mode"); rtti->RegisterFunction(f15a);

    auto f15b = RED4ext::CGlobalFunction::Create("GetVRBoneHookArmed", "GetVRBoneHookArmed", &GetVRBoneHookArmed);
    f15b->flags = flags; f15b->SetReturnType("Int32"); rtti->RegisterFunction(f15b);

    auto f15c = RED4ext::CGlobalFunction::Create("GetVRBoneHookStats", "GetVRBoneHookStats", &GetVRBoneHookStats);
    f15c->flags = flags; f15c->SetReturnType("Int32"); f15c->AddParam("Int32", "mode"); rtti->RegisterFunction(f15c);

    auto f15d = RED4ext::CGlobalFunction::Create("SetVRBoneHookCapture", "SetVRBoneHookCapture", &SetVRBoneHookCapture);
    f15d->flags = flags; f15d->SetReturnType("Int32"); f15d->AddParam("Int32", "on"); rtti->RegisterFunction(f15d);

    auto f15e = RED4ext::CGlobalFunction::Create("GetVRBoneHookCapturedCount", "GetVRBoneHookCapturedCount", &GetVRBoneHookCapturedCount);
    f15e->flags = flags; f15e->SetReturnType("Int32"); rtti->RegisterFunction(f15e);

    auto f15f = RED4ext::CGlobalFunction::Create("GetVRBoneHookCapturedRcx", "GetVRBoneHookCapturedRcx", &GetVRBoneHookCapturedRcx);
    f15f->flags = flags; f15f->SetReturnType("Int32"); f15f->AddParam("Int32", "index"); rtti->RegisterFunction(f15f);

    auto f15g = RED4ext::CGlobalFunction::Create("GetVRBoneHookCapturedBoneCount", "GetVRBoneHookCapturedBoneCount", &GetVRBoneHookCapturedBoneCount);
    f15g->flags = flags; f15g->SetReturnType("Int32"); f15g->AddParam("Int32", "index"); rtti->RegisterFunction(f15g);

    auto f15h = RED4ext::CGlobalFunction::Create("ArmVRBoneHookByCaptureIndex", "ArmVRBoneHookByCaptureIndex", &ArmVRBoneHookByCaptureIndex);
    f15h->flags = flags; f15h->SetReturnType("Int32"); f15h->AddParam("Int32", "index"); rtti->RegisterFunction(f15h);

    auto f15i = RED4ext::CGlobalFunction::Create("GetPlayerBoneBufferAddress", "GetPlayerBoneBufferAddress", &GetPlayerBoneBufferAddress);
    f15i->flags = flags; f15i->SetReturnType("Int32"); rtti->RegisterFunction(f15i);

    auto f15j = RED4ext::CGlobalFunction::Create("InstallVRAnimPoseHook", "InstallVRAnimPoseHook", &InstallVRAnimPoseHook);
    f15j->flags = flags; f15j->SetReturnType("Int32"); rtti->RegisterFunction(f15j);

    auto f15k = RED4ext::CGlobalFunction::Create("ArmVRAnimPosePlayer", "ArmVRAnimPosePlayer", &ArmVRAnimPosePlayer);
    f15k->flags = flags; f15k->SetReturnType("Int32"); rtti->RegisterFunction(f15k);

    auto f15l = RED4ext::CGlobalFunction::Create("SetVRAnimPoseDebug", "SetVRAnimPoseDebug", &SetVRAnimPoseDebug);
    f15l->flags = flags; f15l->SetReturnType("Int32"); f15l->AddParam("Int32", "mode"); rtti->RegisterFunction(f15l);

    auto f15m = RED4ext::CGlobalFunction::Create("GetVRAnimPoseStats", "GetVRAnimPoseStats", &GetVRAnimPoseStats);
    f15m->flags = flags; f15m->SetReturnType("Int32"); f15m->AddParam("Int32", "mode"); rtti->RegisterFunction(f15m);

    auto f15n = RED4ext::CGlobalFunction::Create("SetVRAnimPoseBoneTest", "SetVRAnimPoseBoneTest", &SetVRAnimPoseBoneTest);
    f15n->flags = flags; f15n->SetReturnType("Int32"); f15n->AddParam("Int32", "index"); f15n->AddParam("Float", "mag"); rtti->RegisterFunction(f15n);

    auto f15o = RED4ext::CGlobalFunction::Create("DumpPlayerBoneNames", "DumpPlayerBoneNames", &DumpPlayerBoneNames);
    f15o->flags = flags; f15o->SetReturnType("Int32"); rtti->RegisterFunction(f15o);

    auto f15p = RED4ext::CGlobalFunction::Create("SetVRBindMode", "SetVRBindMode", &SetVRBindMode);
    f15p->flags = flags; f15p->SetReturnType("Int32"); f15p->AddParam("Int32", "mode"); rtti->RegisterFunction(f15p);

    auto f15q = RED4ext::CGlobalFunction::Create("SetVRBindParams", "SetVRBindParams", &SetVRBindParams);
    f15q->flags = flags; f15q->SetReturnType("Int32"); 
    f15q->AddParam("Float", "scale"); f15q->AddParam("Float", "x"); f15q->AddParam("Float", "y"); f15q->AddParam("Float", "z"); f15q->AddParam("Int32", "axis");
    rtti->RegisterFunction(f15q);

    auto f15r = RED4ext::CGlobalFunction::Create("SetVRBindBones", "SetVRBindBones", &SetVRBindBones);
    f15r->flags = flags; f15r->SetReturnType("Int32"); 
    f15r->AddParam("Int32", "leftIdx"); f15r->AddParam("Int32", "rightIdx");
    rtti->RegisterFunction(f15r);



    auto f19 = RED4ext::CGlobalFunction::Create("DumpAnimVTable", "DumpAnimVTable", &DumpAnimVTable);
    f19->flags = flags; f19->SetReturnType("Int32"); rtti->RegisterFunction(f19);

    auto f20 = RED4ext::CGlobalFunction::Create("DumpAnimMemory", "DumpAnimMemory", &DumpAnimMemory);
    f20->flags = flags; f20->SetReturnType("Int32"); rtti->RegisterFunction(f20);

    auto f21 = RED4ext::CGlobalFunction::Create("DumpAnimControllerComponents", "DumpAnimControllerComponents", &DumpAnimControllerComponents);
    f21->flags = flags; f21->SetReturnType("Int32"); rtti->RegisterFunction(f21);

    auto f22 = RED4ext::CGlobalFunction::Create("DumpRuntimeClassFunctions", "DumpRuntimeClassFunctions", &DumpRuntimeClassFunctions);
    f22->flags = flags; f22->SetReturnType("Int32"); rtti->RegisterFunction(f22);

    auto f23 = RED4ext::CGlobalFunction::Create("SetVRIKAnimInputTestMode", "SetVRIKAnimInputTestMode", &SetVRIKAnimInputTestMode);
    f23->flags = flags; f23->AddParam("Int32", "mode"); rtti->RegisterFunction(f23);

    auto f24 = RED4ext::CGlobalFunction::Create("UpdateVRIKAnimInputs", "UpdateVRIKAnimInputs", &UpdateVRIKAnimInputs);
    f24->flags = flags; f24->SetReturnType("Int32"); rtti->RegisterFunction(f24);

    auto f25 = RED4ext::CGlobalFunction::Create("DumpAnimControllerFunctionDetails", "DumpAnimControllerFunctionDetails", &DumpAnimControllerFunctionDetails);
    f25->flags = flags; f25->SetReturnType("Int32"); rtti->RegisterFunction(f25);

    auto f26 = RED4ext::CGlobalFunction::Create("DumpAnimControllerListeners", "DumpAnimControllerListeners", &DumpAnimControllerListeners);
    f26->flags = flags; f26->SetReturnType("Int32"); rtti->RegisterFunction(f26);

    auto f27 = RED4ext::CGlobalFunction::Create("DumpInterestingAnimClassProperties", "DumpInterestingAnimClassProperties", &DumpInterestingAnimClassProperties);
    f27->flags = flags; f27->SetReturnType("Int32"); rtti->RegisterFunction(f27);

    auto f28 = RED4ext::CGlobalFunction::Create("DumpAnimationSystemCandidates", "DumpAnimationSystemCandidates", &DumpAnimationSystemCandidates);
    f28->flags = flags; f28->SetReturnType("Int32"); rtti->RegisterFunction(f28);

    auto f29 = RED4ext::CGlobalFunction::Create("RunIKTargetAddTest", "RunIKTargetAddTest", &RunIKTargetAddTest);
    f29->flags = flags; f29->SetReturnType("Int32"); f29->AddParam("Int32", "mode"); rtti->RegisterFunction(f29);

    auto f30 = RED4ext::CGlobalFunction::Create("TestAnimFloatInput", "TestAnimFloatInput", &TestAnimFloatInput);
    f30->flags = flags; f30->SetReturnType("Int32");
    f30->AddParam("CName", "inputName");
    f30->AddParam("Float", "value");
    f30->AddParam("Int32", "route");
    rtti->RegisterFunction(f30);

    auto f31 = RED4ext::CGlobalFunction::Create("TestAnimFloatPreset", "TestAnimFloatPreset", &TestAnimFloatPreset);
    f31->flags = flags; f31->SetReturnType("Int32");
    f31->AddParam("Int32", "mode");
    f31->AddParam("Float", "value");
    f31->AddParam("Int32", "route");
    rtti->RegisterFunction(f31);

    auto f32 = RED4ext::CGlobalFunction::Create("SetPlayerAnimParameter", "SetPlayerAnimParameter", &SetPlayerAnimParameter);
    f32->flags = flags; f32->SetReturnType("Int32");
    f32->AddParam("CName", "inputName");
    f32->AddParam("Float", "value");
    rtti->RegisterFunction(f32);

    auto f33 = RED4ext::CGlobalFunction::Create("SetPlayerAnimParameterPreset", "SetPlayerAnimParameterPreset", &SetPlayerAnimParameterPreset);
    f33->flags = flags; f33->SetReturnType("Int32");
    f33->AddParam("Int32", "mode");
    f33->AddParam("Float", "value");
    rtti->RegisterFunction(f33);

    auto f34 = RED4ext::CGlobalFunction::Create("SetPlayerAnimParameterPersistentPreset", "SetPlayerAnimParameterPersistentPreset", &SetPlayerAnimParameterPersistentPreset);
    f34->flags = flags; f34->SetReturnType("Int32");
    f34->AddParam("Int32", "mode");
    f34->AddParam("Float", "value");
    rtti->RegisterFunction(f34);

    auto f35 = RED4ext::CGlobalFunction::Create("GetPlayerAnimParameterPersistentLastResult", "GetPlayerAnimParameterPersistentLastResult", &GetPlayerAnimParameterPersistentLastResult);
    f35->flags = flags; f35->SetReturnType("Int32");
    rtti->RegisterFunction(f35);

    auto f36 = RED4ext::CGlobalFunction::Create("TestWeaponUserFeatureRoute", "TestWeaponUserFeatureRoute", &TestWeaponUserFeatureRoute);
    f36->flags = flags; f36->SetReturnType("Int32");
    f36->AddParam("Int32", "route");
    rtti->RegisterFunction(f36);

    auto f37 = RED4ext::CGlobalFunction::Create("TestIKFeatureRoute", "TestIKFeatureRoute", &TestIKFeatureRoute);
    f37->flags = flags; f37->SetReturnType("Int32");
    f37->AddParam("Int32", "mode");
    f37->AddParam("Int32", "route");
    rtti->RegisterFunction(f37);

    auto f38 = RED4ext::CGlobalFunction::Create("TestMeleeIKDataFeatureRoute", "TestMeleeIKDataFeatureRoute", &TestMeleeIKDataFeatureRoute);
    f38->flags = flags; f38->SetReturnType("Int32");
    f38->AddParam("Int32", "route");
    rtti->RegisterFunction(f38);

    auto f39 = RED4ext::CGlobalFunction::Create("DumpRootGraphVariables", "DumpRootGraphVariables", &DumpRootGraphVariables);
    f39->flags = flags; f39->SetReturnType("Int32");
    rtti->RegisterFunction(f39);

    auto f40 = RED4ext::CGlobalFunction::Create("SetRootGraphBoolVariable", "SetRootGraphBoolVariable", &SetRootGraphBoolVariableByName);
    f40->flags = flags; f40->SetReturnType("Int32");
    f40->AddParam("CName", "name");
    f40->AddParam("Bool", "value");
    rtti->RegisterFunction(f40);

    auto f41 = RED4ext::CGlobalFunction::Create("SetRootGraphFloatVariable", "SetRootGraphFloatVariable", &SetRootGraphFloatVariableByName);
    f41->flags = flags; f41->SetReturnType("Int32");
    f41->AddParam("CName", "name");
    f41->AddParam("Float", "value");
    rtti->RegisterFunction(f41);

    auto f42 = RED4ext::CGlobalFunction::Create("SetRootGraphVectorVariable", "SetRootGraphVectorVariable", &SetRootGraphVectorVariableByName);
    f42->flags = flags; f42->SetReturnType("Int32");
    f42->AddParam("CName", "name");
    f42->AddParam("Vector4", "value");
    rtti->RegisterFunction(f42);

    auto f43 = RED4ext::CGlobalFunction::Create("TestRootGraphFloatPreset", "TestRootGraphFloatPreset", &TestRootGraphFloatPreset);
    f43->flags = flags; f43->SetReturnType("Int32");
    f43->AddParam("Int32", "mode");
    f43->AddParam("Float", "value");
    rtti->RegisterFunction(f43);

    auto f44 = RED4ext::CGlobalFunction::Create("TestRootGraphVectorPreset", "TestRootGraphVectorPreset", &TestRootGraphVectorPreset);
    f44->flags = flags; f44->SetReturnType("Int32");
    f44->AddParam("Int32", "mode");
    f44->AddParam("Vector4", "value");
    rtti->RegisterFunction(f44);

    auto f45 = RED4ext::CGlobalFunction::Create("SetRootGraphFloatPresetPersistent", "SetRootGraphFloatPresetPersistent", &SetRootGraphFloatPresetPersistent);
    f45->flags = flags; f45->SetReturnType("Int32");
    f45->AddParam("Int32", "mode");
    f45->AddParam("Float", "value");
    rtti->RegisterFunction(f45);

    auto f46 = RED4ext::CGlobalFunction::Create("SetRootGraphVectorPresetPersistent", "SetRootGraphVectorPresetPersistent", &SetRootGraphVectorPresetPersistent);
    f46->flags = flags; f46->SetReturnType("Int32");
    f46->AddParam("Int32", "mode");
    f46->AddParam("Vector4", "value");
    rtti->RegisterFunction(f46);

    auto f47 = RED4ext::CGlobalFunction::Create("GetRootGraphPersistentLastResult", "GetRootGraphPersistentLastResult", &GetRootGraphPersistentLastResult);
    f47->flags = flags; f47->SetReturnType("Int32");
    rtti->RegisterFunction(f47);

    auto f48 = RED4ext::CGlobalFunction::Create("DumpPlayerAnimatedObjectRuntime", "DumpPlayerAnimatedObjectRuntime", &DumpPlayerAnimatedObjectRuntime);
    f48->flags = flags; f48->SetReturnType("Int32");
    rtti->RegisterFunction(f48);

    auto f49 = RED4ext::CGlobalFunction::Create("DumpAnimationSystemLookup", "DumpAnimationSystemLookup", &DumpAnimationSystemLookup);
    f49->flags = flags; f49->SetReturnType("Int32");
    rtti->RegisterFunction(f49);

    auto f50 = RED4ext::CGlobalFunction::Create("DumpRootMetaRigTracks", "DumpRootMetaRigTracks", &DumpRootMetaRigTracks);
    f50->flags = flags; f50->SetReturnType("Int32");
    rtti->RegisterFunction(f50);

    auto f51 = RED4ext::CGlobalFunction::Create("TestRootMetaRigTrackPreset", "TestRootMetaRigTrackPreset", &TestRootMetaRigTrackPreset);
    f51->flags = flags; f51->SetReturnType("Int32");
    f51->AddParam("Int32", "mode");
    f51->AddParam("Float", "value");
    rtti->RegisterFunction(f51);

    auto f52 = RED4ext::CGlobalFunction::Create("SetRootMetaRigTrackPresetPersistent", "SetRootMetaRigTrackPresetPersistent", &SetRootMetaRigTrackPresetPersistent);
    f52->flags = flags; f52->SetReturnType("Int32");
    f52->AddParam("Int32", "mode");
    f52->AddParam("Float", "value");
    rtti->RegisterFunction(f52);

    auto f53 = RED4ext::CGlobalFunction::Create("GetRootMetaRigTrackPersistentLastResult", "GetRootMetaRigTrackPersistentLastResult", &GetRootMetaRigTrackPersistentLastResult);
    f53->flags = flags; f53->SetReturnType("Int32");
    rtti->RegisterFunction(f53);

    auto f54 = RED4ext::CGlobalFunction::Create("DumpRootAnimatedObjectFloatArrayCandidates", "DumpRootAnimatedObjectFloatArrayCandidates", &DumpRootAnimatedObjectFloatArrayCandidates);
    f54->flags = flags; f54->SetReturnType("Int32");
    rtti->RegisterFunction(f54);

    auto f55 = RED4ext::CGlobalFunction::Create("TestRootLiveTrackPreset", "TestRootLiveTrackPreset", &TestRootLiveTrackPreset);
    f55->flags = flags; f55->SetReturnType("Int32");
    f55->AddParam("Int32", "mode");
    f55->AddParam("Float", "value");
    f55->AddParam("Int32", "arrayMode");
    rtti->RegisterFunction(f55);

    auto f56 = RED4ext::CGlobalFunction::Create("SetRootLiveTrackPresetPersistent", "SetRootLiveTrackPresetPersistent", &SetRootLiveTrackPresetPersistent);
    f56->flags = flags; f56->SetReturnType("Int32");
    f56->AddParam("Int32", "mode");
    f56->AddParam("Float", "value");
    f56->AddParam("Int32", "arrayMode");
    rtti->RegisterFunction(f56);

    auto f57 = RED4ext::CGlobalFunction::Create("GetRootLiveTrackPersistentLastResult", "GetRootLiveTrackPersistentLastResult", &GetRootLiveTrackPersistentLastResult);
    f57->flags = flags; f57->SetReturnType("Int32");
    rtti->RegisterFunction(f57);

    auto f58 = RED4ext::CGlobalFunction::Create("ReadRootLiveTrackPreset", "ReadRootLiveTrackPreset", &ReadRootLiveTrackPreset);
    f58->flags = flags; f58->SetReturnType("Int32");
    f58->AddParam("Int32", "mode");
    f58->AddParam("Int32", "arrayMode");
    rtti->RegisterFunction(f58);
}

RED4EXT_C_EXPORT void RED4EXT_CALL RegisterTypes() {}

RED4EXT_C_EXPORT bool RED4EXT_CALL Main(RED4ext::v1::PluginHandle aHandle, RED4ext::v1::EMainReason aReason, const RED4ext::v1::Sdk* aSdk) {
    RED4EXT_UNUSED_PARAMETER(aHandle); RED4EXT_UNUSED_PARAMETER(aSdk);
    if (aReason == RED4ext::v1::EMainReason::Load) {
        auto rtti = RED4ext::CRTTISystem::Get();
        rtti->AddRegisterCallback(RegisterTypes);
        rtti->AddPostRegisterCallback(PostRegisterTypes);
    }
    return true;
}

RED4EXT_C_EXPORT void RED4EXT_CALL Query(RED4ext::v1::PluginInfo* aInfo) {
    aInfo->name = L"CyberpunkVR_Hands";
    aInfo->author = L"VRPort";
    aInfo->version = RED4EXT_V1_SEMVER(1, 0, 0);
    aInfo->runtime = RED4EXT_V1_RUNTIME_VERSION_INDEPENDENT;
    aInfo->sdk = RED4EXT_V1_SDK_VERSION_1_0_0_COMPAT_0_5_0;
}

RED4EXT_C_EXPORT uint32_t RED4EXT_CALL Supports() {
    return RED4EXT_API_VERSION_1_COMPAT_0;
}
