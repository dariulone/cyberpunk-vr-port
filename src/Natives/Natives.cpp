#include <RED4ext/RED4ext.hpp>
#include "Natives/NativeFunctions.hpp"  // the natives defined here, with C linkage in the Rust build
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
// MinHook came in as a side effect of the weapon-aim header carrying its own
// implementation. This file installs detours of its own, so it asks for it directly.
#include <MinHook.h>


// Defined later in this file; forward-declared so the per-frame calibration
// bridge in UpdateVRIKAnimInputs can read them / trigger a diag dump.
extern volatile float g_VRCamI, g_VRCamJ, g_VRCamK, g_VRCamR;
// WriteVRDiagCore is declared in Natives/NativeHelpers.hpp; the definition moved with the
// calibration family.

#if CVR_RUST_VRIK
// The Rust build's hand natives, the overlay's calibration, the FPP component dump, the chunk-debug setters, their
// helpers and the GetPlayer call are rust/crates/cvr-ffi/src/hand_natives.rs and anim_lookup.rs; nothing in this build
// calls the helpers' C++ names any more.

// The fields hand_natives.rs's FPP component dump reads.
RED4EXT_ASSERT_OFFSET(RED4ext::ent::Entity, templatePath, 0x60);
RED4EXT_ASSERT_OFFSET(RED4ext::ent::IComponent, isEnabled, 0x8B);
RED4EXT_ASSERT_OFFSET(RED4ext::ent::SkinnedMeshComponent, chunkMask, 0x248);
RED4EXT_ASSERT_OFFSET(RED4ext::ent::MeshComponent, visualScale, 0x178);
RED4EXT_ASSERT_OFFSET(RED4ext::ent::MeshComponent, chunkMask, 0x198);
#else
// Reads IK calibration the in-headset overlay published into shared memory
// (slots [33..48]) and applies it to the live globals. When [33] (valid) is 0
// the plugin keeps its baked defaults so the CET sliders still work standalone.
// Also polls the one-shot diag-request counter ([48]) and dumps a diag file
// when it changes, so the overlay's "Log VR Diag" button works in-headset.
void PollVRCalibFromShared() {
    if (!g_pSharedHands) return;
    // Shoulder anatomical offsets from auto-calibration (slots 70..75, validity in 76).
    // These live outside the regular [34..47] calibration block.
    if (g_pSharedHands[76] != 0.0f) {
        g_VRShoulderRX = g_pSharedHands[70];
        g_VRShoulderRY = g_pSharedHands[71];
        g_VRShoulderRZ = g_pSharedHands[72];
        g_VRShoulderLX = g_pSharedHands[73];
        g_VRShoulderLY = g_pSharedHands[74];
        g_VRShoulderLZ = g_pSharedHands[75];
    }
    // T-pose measured anatomy from auto-calibration: [77]/[78] = real arm length R/L (m),
    // [79] = HMD eye height (m), [80] = valid. Drives the gizmo-path arm-bone scaling
    // (a straight real arm -> straight avatar arm) instead of the old position-scale hack.
    if (g_pSharedHands[80] != 0.0f) {
        g_VRUserArmLenR   = g_pSharedHands[77];
        g_VRUserArmLenL   = g_pSharedHands[78];
        g_VRUserEyeHeight = g_pSharedHands[79];
    }
    if (g_pSharedHands[33] != 0.0f) {
        const float* c = &g_pSharedHands[34]; // scaleR,scaleL,heightR,heightL,swingR,swingL,poleR,poleL, wRpyr(3), wLpyr(3)
        g_VRScaleR = c[0]; g_VRScaleL = c[1];
        g_VROffRZ  = c[2]; g_VROffLZ  = c[3];
        g_VRElbowSwingR = c[4]; g_VRElbowSwingL = c[5];
        g_VRElbowPoleR  = c[6]; g_VRElbowPoleL  = c[7];
        // Wrist corrections: euler(pitch,yaw,roll) deg -> quat (same XYZ convention as SetVRHandOffset).
        const float d2r = 0.01745329252f * 0.5f;
        for (int side = 0; side < 2; ++side) {
            float p = c[8 + side*3], y = c[9 + side*3], r = c[10 + side*3];
            float cp = std::cos(p*d2r), sp = std::sin(p*d2r);
            float cy = std::cos(y*d2r), sy = std::sin(y*d2r);
            float cr = std::cos(r*d2r), sr = std::sin(r*d2r);
            float qi = sp*cy*cr + cp*sy*sr;
            float qj = cp*sy*cr - sp*cy*sr;
            float qk = cp*cy*sr + sp*sy*cr;
            float qr = cp*cy*cr - sp*sy*sr;
            if (side == 0) { g_VRWristR_I = qi; g_VRWristR_J = qj; g_VRWristR_K = qk; g_VRWristR_R = qr; }
            else           { g_VRWristL_I = qi; g_VRWristL_J = qj; g_VRWristL_K = qk; g_VRWristL_R = qr; }
        }
    }
    // One-shot diag request (monotonic counter from the overlay).
    static int s_lastDiagReq = 0;
    int req = static_cast<int>(g_pSharedHands[48]);
    if (req != s_lastDiagReq) {
        s_lastDiagReq = req;
        // Camera position isn't published; the decisive diag lines don't need it.
        WriteVRDiagCore(0, 0, 0, g_VRCamI, g_VRCamJ, g_VRCamK, g_VRCamR);
    }
}

static RED4ext::world::AnimationSystem* ScanForAnimationSystemInBlock(uint8_t* aBase, size_t aSize, std::ofstream* aOut = nullptr);
static const char* ClassifyQword(uint64_t v);
static uint64_t SafeReadQword(uint8_t* base, size_t off);
static uint32_t SafeReadU32(uint8_t* base, size_t off);
static float SafeReadFloat(uint8_t* base, size_t off);
static RED4ext::CClass* SafeGetObjectType(void* aPtr);

// Cached pointer to the live worldAnimationSystem. The discovery scan
// (SafeGetObjectType over hundreds of arbitrary heap pointers) is the main
// source of intermittent crashes, so we run it once and reuse the result.
RED4ext::world::AnimationSystem* g_cachedAnimationSystem = nullptr;

bool ContainsInsensitive(const char* haystack, const char* needle) {
    if (!haystack || !needle) return false;
    std::string h(haystack);
    std::string n(needle);
    std::transform(h.begin(), h.end(), h.begin(), [](unsigned char c) { return static_cast<char>(std::tolower(c)); });
    std::transform(n.begin(), n.end(), n.begin(), [](unsigned char c) { return static_cast<char>(std::tolower(c)); });
    return h.find(n) != std::string::npos;
}

bool EqualsInsensitive(const char* a, const char* b) {
    if (!a || !b) return false;
    for (; *a && *b; ++a, ++b) {
        if (std::tolower(static_cast<unsigned char>(*a)) != std::tolower(static_cast<unsigned char>(*b)))
            return false;
    }
    return *a == *b;
}

bool ClassIsA(RED4ext::CClass* type, RED4ext::CName className) {
    while (type) {
        if (type->name == className) return true;
        type = type->parent;
    }
    return false;
}

bool IsLikelyFppArmComponent(const char* componentName) {
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

// SEH-guarded readability check via VirtualQuery (avoids the AV that crashed the game when a garbage
// float like 0xF51C006E was treated as a pointer and dereferenced). A kernel transition: never on a
// per-ray or per-pass path. Moved here from the deleted LiveProjectile.cpp.
bool IsReadable(uintptr_t a, size_t n) {
    if (a < 0x10000) return false;
    MEMORY_BASIC_INFORMATION mbi;
    if (!VirtualQuery(reinterpret_cast<void*>(a), &mbi, sizeof(mbi))) return false;
    if (mbi.State != MEM_COMMIT) return false;
    DWORD prot = mbi.Protect;
    if (prot & (PAGE_NOACCESS | PAGE_GUARD)) return false;
    if (!(prot & (PAGE_READONLY|PAGE_READWRITE|PAGE_EXECUTE_READ|PAGE_EXECUTE_READWRITE|PAGE_WRITECOPY|PAGE_EXECUTE_WRITECOPY))) return false;
    uintptr_t end = reinterpret_cast<uintptr_t>(mbi.BaseAddress) + mbi.RegionSize;
    return (a + n) <= end;
}

void EnsureSharedMemory() {
    if (!g_pSharedHands) {
        // 1024 bytes = 256 floats. [0..127] is the legacy crowded region (hands seqlock
        // at [127], HMD pos [124..126], entity [96..99], view offsets...); [128..131]
        // carries the clean camera-local pair (see SetVRPlayerYaw).
        g_hMapFile = CreateFileMappingA(INVALID_HANDLE_VALUE, NULL, PAGE_READWRITE, 0, 1024, "CyberpunkVR_Hands_Shared");
        if (g_hMapFile) g_pSharedHands = (float*)MapViewOfFile(g_hMapFile, FILE_MAP_ALL_ACCESS, 0, 0, 1024);
    }
}

// Seqlock reader facility (g_handsStable / RefreshHandsSnapshot / SharedPose) is
// defined in vrik_hook.h (included above) so the native AnimPose hook there — the
// real per-frame body-IK consumer — can use the same latched snapshot.

void GetLeftVRHandValid(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t a4) {
    aFrame->code++; EnsureSharedMemory(); RefreshHandsSnapshot();
    if (aOut) *aOut = g_handsStableValid ? (SharedPose(0) > 0.0f) : false;
}

void GetRightVRHandValid(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t a4) {
    aFrame->code++; EnsureSharedMemory(); RefreshHandsSnapshot();
    if (aOut) *aOut = g_handsStableValid ? (SharedPose(8) > 0.0f) : false;
}

void GetLeftVRHandPos(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4) {
    aFrame->code++; EnsureSharedMemory(); RefreshHandsSnapshot();
    if (aOut) {
        if (g_handsStableValid) {
            aOut->X = SharedPose(1); aOut->Y = SharedPose(2); aOut->Z = SharedPose(3);
        } else { aOut->X = aOut->Y = aOut->Z = 0.0f; }
        aOut->W = 1.0f;
    }
}

void GetRightVRHandPos(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4) {
    aFrame->code++; EnsureSharedMemory(); RefreshHandsSnapshot();
    if (aOut) {
        if (g_handsStableValid) {
            aOut->X = SharedPose(9); aOut->Y = SharedPose(10); aOut->Z = SharedPose(11);
        } else { aOut->X = aOut->Y = aOut->Z = 0.0f; }
        aOut->W = 1.0f;
    }
}

void GetLeftVRHandRot(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Quaternion* aOut, int64_t a4) {
    aFrame->code++; EnsureSharedMemory(); RefreshHandsSnapshot();
    if (aOut) {
        if (g_handsStableValid) {
            aOut->i = SharedPose(4); aOut->j = SharedPose(5); aOut->k = SharedPose(6); aOut->r = SharedPose(7);
        } else { aOut->i = aOut->j = aOut->k = 0.0f; aOut->r = 1.0f; }
    }
}

void GetRightVRHandRot(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Quaternion* aOut, int64_t a4) {
    aFrame->code++; EnsureSharedMemory(); RefreshHandsSnapshot();
    if (aOut) {
        if (g_handsStableValid) {
            aOut->i = SharedPose(12); aOut->j = SharedPose(13); aOut->k = SharedPose(14); aOut->r = SharedPose(15);
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

    std::ofstream out(VRDiagPath("fpp_components_dump.txt"), std::ios::out | std::ios::trunc);
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
#endif
