// AnimGraph -- natives lifted out of src/Natives/Natives.cpp, which held every family at once.
//
// The anim-graph and RTTI natives: reading and writing the engine's animation
// variables, features and bindings from script, plus the type dumps that made any of it
// findable in the first place.
//
// The cut was placed by the seam map and then SNAPPED to the nearest point at brace depth zero.
// Boundaries taken from line numbers alone are how a split lands in the middle of a function; the
// check is cheap and it is the same lesson as every other generator in this restructure.
#include <RED4ext/RED4ext.hpp>
#include "Anim/WheelGrab.hpp"
#include "Anim/FaceTracking.hpp"
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




// Moved to src/Natives/AnimLookup.cpp: finding the objects everything else operates on.

const char* GetTypeNameForDump(RED4ext::rtti::IType* aType)
{
    if (!aType)
        return "<null>";

    const char* computed = aType->GetComputedName().ToString();
    if (computed && computed[0])
        return computed;

    const char* native = aType->GetName().ToString();
    return native ? native : "<unnamed>";
}

// Moved to src/Natives/AnimDumps.cpp: the RTTI dump helpers.

bool IsInterestingAnimName(const char* aName)
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

// Moved to src/Natives/AnimDumps.cpp: the graph-variable, meta-rig, resource and entity dumps.

// Moved to src/Natives/AnimFeatures.cpp: building anim features and the routes that hand them to the controller.

void DumpRootGraphVariables(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;

    std::ofstream out(VRDiagPath("root_graph_variables.txt"), std::ios::trunc);
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

// Plain-C++ arm helper (resolves the player's live track buffers). Shared by the
// script-callable ArmVRAnimPosePlayer and the in-VR overlay auto-activation below.
// Returns bitmask: 1=bufA set, 2=bufB set, -1=player not ready.
// VRIK_DoArmPlayer is declared in Natives/NativeHelpers.hpp; its definition moved with the
// VRIK arming family. A `static` forward declaration here would promise a definition in THIS
// file and MSVC says so plainly: "declared but not defined".

void UpdateVRIKAnimInputs(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;

    // --- In-VR overlay activation -------------------------------------------
    cvr::face::Tick();
    // The in-headset menu (imgui_overlay) writes a tracking-request code into
    // shared-memory slot [32] (0 = off, 2 = position+rotation). CET calls us
    // every frame on the game thread, so installing the hooks / arming the
    // player here is exactly as safe as the manual "Start VR Tracking" button.
    // Edge-triggered on g_VRBind so the CET button still works independently.
    EnsureSharedMemory();

    if (g_pSharedHands) {
        static bool     s_vrHooksInstalled = false;
        static bool     s_vrArmed = false;
        static int      s_lastReq = 0;
        // Desync detection (replaces the old fixed-interval re-arm timer). VRIK_DoArmPlayer()
        // is NOT cheap: it walks the world-animation-system entity buckets to find the player
        // AND scans every metaRig bone name (up to ~700 on the player rig). Calling it on a
        // fixed timer (every 6 or even every 60 frames) burns time on the same thread that also
        // has to get through Hooked_AnimPoseApply for every skeleton in the scene, which is
        // exactly why the VRIK update rate felt low. Instead, only call it when VRIK actually
        // falls out of sync.
        //
        // Hooked_AnimPoseApply (vrik_hook.h) already tells us this for free: it bumps
        // g_AnimPoseMatchCalls once per frame ONLY when the player's pose-apply trackBuf still
        // matches the pointers we cached from the last VRIK_DoArmPlayer() resolve. If the
        // engine swaps that track buffer out from under us (weapon draw/holster, save load,
        // area transition, vehicle in/out, scripted scene, ragdoll...), the match stops and
        // g_AnimPoseMatchCalls stops incrementing even though we are still "armed" -- that IS
        // the VRIK desync. We detect that stall and re-resolve only then.
        static uint64_t s_lastMatchCalls = 0;
        static int      s_staleFrames = 0;
        // ~10 stalled frames (well under 200ms at 60+ fps) before we pay for a re-resolve --
        // fast enough that a weapon swap / area load recovers quickly, but ignores normal
        // single-frame hitches so a healthy VRIK never pays the re-arm cost at all.
        constexpr int kStaleFrameThreshold = 10;
        int req = static_cast<int>(g_pSharedHands[32]);
        if (req > 0) {
            if (!s_vrHooksInstalled) {
                // Only the pose-apply hook is needed (the old ComponentFunc21 hook was
                // removed: it trampolined a super-hot per-component Update and tanked FPS).
                InstallAnimPoseHook();
                s_vrHooksInstalled = true;
            }
            const uint64_t matchCalls = g_AnimPoseMatchCalls;
            // Equipment can replace/grow the rig while the old track still
            // matches. Rebind on identity/layout changes, without waiting for
            // ten missing pose callbacks. The heartbeat remains a fallback.
            bool needRearm = !s_vrArmed || VRIK_PlayerBindingChanged();
            if (s_vrArmed) {
                if (matchCalls != s_lastMatchCalls) {
                    s_staleFrames = 0;             // still getting matched poses -> healthy
                } else if (++s_staleFrames >= kStaleFrameThreshold) {
                    needRearm = true;               // stalled -> VRIK desynced, re-resolve
                    s_staleFrames = 0;
                }
            }
            if (needRearm) {
                s_vrArmed = VRIK_DoArmPlayer() > 0;
                s_staleFrames = 0;
            }
            s_lastMatchCalls = matchCalls;
            if (s_lastReq <= 0) g_VRBind = req;   // off -> on edge
            if (g_VRNeutralizeAnimGraph != 0) {
                ForceVRNeutralAnimGraphInputs();
            }
            // Keep the diag bone snapshot fresh while tracking, so the overlay's
            // "Log VR Diag" works without the CET window's capture toggle.
            g_VRDiagCapture = 1;
        } else {
            if (s_lastReq > 0) {
                g_VRBind = 0; g_VRDiagCapture = 0;                    // on -> off edge
                g_pSharedHands[119] = 0.0f;  // eye-view offset invalid while VRIK is off
                // The wheel grab lives inside the mode-4 solve; with tracking off nothing would ever
                // lower its armed mask again, and a grip would stay out of its gameplay meaning for
                // the rest of the session.
                cvr::anim::WheelReset();
            }
            s_vrArmed = false;                     // re-arm on next activation
            s_staleFrames = 0;
            s_lastMatchCalls = g_AnimPoseMatchCalls;
        }
        s_lastReq = req;

        // Pull IK calibration the overlay published + service one-shot diag requests.
        PollVRCalibFromShared();
    }
    // ------------------------------------------------------------------------

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

// Moved to src/Natives/AnimDumps.cpp: the dump natives script calls.

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

    std::ofstream log(VRDiagPath("ik_target_add_test_log.txt"), std::ios::app);
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



// WINE PUTS THE HEAP SOMEWHERE ELSE, and that one fact stopped VRIK from ever arming on Linux
// (issue #14, diagnosed by wundervrc; verified against this tree before applying).
//
// Wine exports wine_get_version from ntdll and real Windows does not. Cached, because this is asked
// from a filter that runs over candidate qwords.
static bool IsRunningUnderWine() {
    static const bool s_wine = []() -> bool {
        HMODULE ntdll = GetModuleHandleA("ntdll.dll");
        return ntdll && GetProcAddress(ntdll, "wine_get_version") != nullptr;
    }();
    return s_wine;
}

// WHAT KIND OF THING A QWORD LOOKS LIKE, by address range. Several callers use this as a FILTER before
// handing a pointer to SafeGetObjectType -- i.e. before making a virtual call on it -- which is why the
// ranges are narrow rather than generous.
//
// THE HEAP FLOOR IS PER PLATFORM. 0x10000000000 is 1 TiB, which is where Windows' high-entropy ASLR
// puts the heap; Wine's allocator answers two orders of magnitude lower. Measured under Proton by the
// reporter: 0x19ad1a538 (~7 GiB), 0x59157fa80 (~24 GiB), 0x15a77d9ce0 (~87 GiB) -- every one of them
// below the floor, so EVERY pointer classified as "" and the animation-system lookup had nothing to
// return. IsValidAnimationSystemPtr gates on "HEAP", TryKnownAnimationSystemChain goes through it, and
// FindWorldAnimationSystemFromScene uses both, so VRIK_DoArmPlayer aborted at its first check: hands
// that never move, with correct controller poses and poseApply=0.0/s in the [vrik] line.
//
// Widened ONLY under Wine, so Windows behaviour is bit-for-bit what it was. That is deliberate: on
// Windows the narrow floor is what keeps the memory-scan paths from calling SafeGetObjectType on
// arbitrary qwords, and this project has measured that __try/__except does NOT save you from a bad
// access in this game -- REDEngine's vectored handler takes the exception first. The original crash in
// issue #14 was exactly that scan; with the chain resolving on the first hop it is never reached.
//
// The CODE/VTBL bound was also two digits short -- 0x00008000000000 is 512 GiB, BELOW its own lower
// bound of 127 TiB, so the interval was empty and module pointers came back as "". No caller's
// behaviour changes (they all test for "HEAP"), but the diagnostic dumps now name them correctly.
const char* ClassifyQword(uint64_t v) {
    const uint64_t heapFloor = IsRunningUnderWine() ? 0x0000000100000000ull    // 4 GiB
                                                    : 0x0000010000000000ull;  // 1 TiB
    if (v >= heapFloor && v < 0x0000700000000000ull)             return "HEAP";
    if (v >= 0x00007F0000000000ull && v < 0x0000800000000000ull) return "CODE/VTBL";
    return "";
}

uint64_t SafeReadQword(uint8_t* base, size_t off) {
    __try { return *reinterpret_cast<uint64_t*>(base + off); }
    __except(EXCEPTION_EXECUTE_HANDLER) { return 0; }
}

float SafeReadFloat(uint8_t* base, size_t off) {
    __try { return *reinterpret_cast<float*>(base + off); }
    __except(EXCEPTION_EXECUTE_HANDLER) { return 0.0f; }
}

uint32_t SafeReadU32(uint8_t* base, size_t off) {
    __try { return *reinterpret_cast<uint32_t*>(base + off); }
    __except(EXCEPTION_EXECUTE_HANDLER) { return 0; }
}

RED4ext::CClass* SafeGetObjectType(void* aPtr)
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

RED4ext::world::AnimationSystem* ScanForAnimationSystemInBlock(uint8_t* aBase, size_t aSize, std::ofstream* aOut)
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

    std::ofstream out(VRDiagPath("anim_memory_dump.txt"), std::ios::trunc);
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
