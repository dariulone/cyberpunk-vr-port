// RemoteCamera -- the one thing the plugin cannot find out for itself: WHICH camera the player took over.
//
// WHY THIS EXISTS AT ALL. The camera writer recognises cameras by their component name, and a surveillance
// camera's is `cameraComponent`. That is not an identity: measured with a diagnostic build, the engine
// patches every such camera in the area -- 20559 identity changes cycling between four objects in one
// session -- so the view attached itself to whichever came last, with no takeover in progress at all.
//
// The script side knows exactly which object is controlled (TakeOverControlSystem.GetControlledObject),
// and it cannot be asked from the plugin's own periodic poll: that runs on the worker thread, where
// calling into the script VM is not safe in this process. So the answer is published INTO the plugin from
// a CET tick instead, four times a second, and the camera writer requires it:
//
//     VRRemoteCamera(active, x, y, z)
//
// active 0 clears the gate; anything else arms it with that world position as the target. A
// `cameraComponent` is then only followed while it sits within a metre and a half of it, which is what
// makes exactly one camera win. With nothing published, nothing is followed -- the safe default.
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Scripting/Functions.hpp>

#include <cstdint>

#include "Camera/CameraState.hpp"    // g_remoteCamOn, g_remoteCamPosFP
#include "Core/VrCoreShared.hpp"      // Log

extern "C" __declspec(dllexport) extern int CyberpunkVR_BdFovWrite;
extern "C" __declspec(dllexport) extern int CyberpunkVR_BdHeadWrite;
// DEFINED HERE, not merely declared. It used to live beside the second eye's HUD capture, which is
// render code and has gone back to 0.1.5 with the rest of it. This is the only consumer left, and
// the toggle belongs to the braindance camera rather than to the stereo path.
extern "C" __declspec(dllexport) int CyberpunkVR_HudInBraindance = 0;
#include "Natives/NativeFunctions.hpp"
#include "Utils/LogThrottle.hpp"   // g_verboseLog: the launcher's DEBUG box

// WHERE THE LIVE PLAYER'S CAMERA IS. A braindance swaps the player for `braindance_replacer.ent`, a
// PlayerPuppet carrying its own gameFPPCameraComponent named `camera`, so two live objects answer to the
// name the camera writer picks MAIN by and the latch flaps between them -- one frame our composed pose,
// the next the engine's own. Script always knows which player is live; the plugin cannot. Consulted only
// while a braindance is running.
void VRPlayerCamera(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    int32_t active = 0;
    float x = 0.0f, y = 0.0f, z = 0.0f;
    RED4ext::GetParameter(aFrame, &active);
    RED4ext::GetParameter(aFrame, &x);
    RED4ext::GetParameter(aFrame, &y);
    RED4ext::GetParameter(aFrame, &z);
    aFrame->code++;

    if (active == 0)
    {
        g_playerCamOn.store(0, std::memory_order_relaxed);
        if (aOut) *aOut = 0;
        return;
    }
    g_playerCamPosFP[0].store(static_cast<int32_t>(x * 131072.0f), std::memory_order_relaxed);
    g_playerCamPosFP[1].store(static_cast<int32_t>(y * 131072.0f), std::memory_order_relaxed);
    g_playerCamPosFP[2].store(static_cast<int32_t>(z * 131072.0f), std::memory_order_relaxed);
    g_playerCamOn.store(1, std::memory_order_release);
    if (aOut) *aOut = 1;
}

// THE SCENE'S OWN FOV, handed over by the script that already reads it.
//
// The plugin cannot ask for it: a braindance renders through a camera no component points at, and the
// only value it could learn was one caught on a frame where the pose test happened to match -- 54
// times out of 1801, which is why the fov arrived late and started over in every new scene. The script
// has had the number all along, from Game.GetCameraSystem():GetActiveCameraFOV.
void VRBdSceneFov(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);
    float fov = 0.0f;
    RED4ext::GetParameter(aFrame, &fov);
    aFrame->code++;
    const bool ok = (fov > 1.0f && fov < 179.0f);
    if (ok) g_bdSceneFov.store(fov, std::memory_order_relaxed);
    if (aOut) *aOut = ok ? 1 : 0;
}

// THE VIEWPOINT OFFSET OF A TAKEN-OVER CAMERA, driven by the panel's three sliders.
//
// A native rather than an ini key on purpose. The slider has to answer while the person is looking
// through the camera -- that is the whole point of a slider -- and the ini is polled AND rewritten by the
// plugin from its own live values, so a writer on the script side would be racing the poll. This lands
// immediately and the CET mod keeps the numbers in its own file.
void VRDevCamOffset(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);
    float x = 0.0f, y = 0.0f, z = 0.0f;
    RED4ext::GetParameter(aFrame, &x);
    RED4ext::GetParameter(aFrame, &y);
    RED4ext::GetParameter(aFrame, &z);
    aFrame->code++;
    // Clamped, because a slider dragged to the end of its rail must not put the view inside the terrain
    // or a kilometre away: three metres each way is more than the geometry of any mount needs.
    auto clamp3 = [](float v) { return v < -3.0f ? -3.0f : (v > 3.0f ? 3.0f : v); };
    CyberpunkVR_DevCamOffsetX = clamp3(x);
    CyberpunkVR_DevCamOffsetY = clamp3(y);
    CyberpunkVR_DevCamOffsetZ = clamp3(z);
    if (g_verboseLog) Log("VRDevCamOffset: lens offset right=%.3f forward=%.3f up=%.3f m\n",
        CyberpunkVR_DevCamOffsetX, CyberpunkVR_DevCamOffsetY, CyberpunkVR_DevCamOffsetZ);
    if (aOut) *aOut = 1;
}

// THE TAKEN-OVER ENTITY'S ID, as a string.
//
// A string and not a number on purpose: an EntityID hash is 64 bits and Lua carries numbers as doubles,
// which are exact only to 53 -- a silently truncated id would match nothing and look exactly like "the
// mechanism does not work". CET prints the hash as e.g. "9003388ULL", so every digit is taken and
// everything else ignored.
void VRTakeoverEntity(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);
    RED4ext::CString text;
    RED4ext::GetParameter(aFrame, &text);
    aFrame->code++;

    uint64_t id = 0;
    const char* p = text.c_str();
    if (p) {
        for (; *p; ++p) {
            if (*p < '0' || *p > '9') {
                if (id != 0) break;      // digits ended: "9003388ULL"
                continue;                // leading junk
            }
            id = id * 10u + static_cast<uint64_t>(*p - '0');
        }
    }
    const uint64_t prev = g_takeoverEntityId.exchange(id, std::memory_order_relaxed);
    if (prev != id)
        if (g_verboseLog) Log("VRTakeoverEntity: entity id %llu (was %llu)\n",
            static_cast<unsigned long long>(id), static_cast<unsigned long long>(prev));
    if (aOut) *aOut = id != 0 ? 1 : 0;
}

// A LIVE SWITCH FOR THE BRAINDANCE FOV WRITE, so the white second eye can be tested against it from the
// bridge in one pass instead of one build per answer.
void VRBdFovWrite(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);
    int32_t on = 0;
    RED4ext::GetParameter(aFrame, &on);
    aFrame->code++;
    CyberpunkVR_BdFovWrite = on ? 1 : 0;
    if (g_verboseLog) Log("VRBdFovWrite: braindance fov write %s\n", on ? "ON" : "OFF");
    if (aOut) *aOut = CyberpunkVR_BdFovWrite;
}

// The HUD composite in the second eye, during a braindance only. 1 = composite it as usual, 0 = leave
// the second eye alone -- the A/B that says whether the white eye comes from this path.
void VRHudSecondEye(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);
    int32_t on = 0;
    RED4ext::GetParameter(aFrame, &on);
    aFrame->code++;
    CyberpunkVR_HudInBraindance = on ? 1 : 0;
    if (g_verboseLog) Log("VRHudSecondEye: HUD into the second eye during a braindance %s\n", on ? "ON" : "OFF");
    if (aOut) *aOut = CyberpunkVR_HudInBraindance;
}

// The same switch for the head rotation written into the braindance view.
void VRBdHeadWrite(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);
    int32_t on = 0;
    RED4ext::GetParameter(aFrame, &on);
    aFrame->code++;
    CyberpunkVR_BdHeadWrite = on ? 1 : 0;
    if (g_verboseLog) Log("VRBdHeadWrite: braindance head write %s\n", on ? "ON" : "OFF");
    if (aOut) *aOut = CyberpunkVR_BdHeadWrite;
}

// THE SCENE'S CAMERA, HANDED OVER WHOLE. In a braindance the scene takes camera control
// (GetSceneSystemCameraControlEnabled) and renders through a camera neither side can name: the port's
// classifier never sees it, and script has no handle on it either. What script CAN do is read its pose
// -- GetSceneSystemCameraLastCameraPosition / ...Orientation -- so it publishes that, and the plugin
// feeds it into the very lens the surveillance-camera fix uses to move the second eye.
void VRSceneCamera(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    int32_t active = 0;
    float x = 0.0f, y = 0.0f, z = 0.0f, qi = 0.0f, qj = 0.0f, qk = 0.0f, qr = 1.0f;
    RED4ext::GetParameter(aFrame, &active);
    RED4ext::GetParameter(aFrame, &x);
    RED4ext::GetParameter(aFrame, &y);
    RED4ext::GetParameter(aFrame, &z);
    RED4ext::GetParameter(aFrame, &qi);
    RED4ext::GetParameter(aFrame, &qj);
    RED4ext::GetParameter(aFrame, &qk);
    RED4ext::GetParameter(aFrame, &qr);
    aFrame->code++;

    if (active == 0)
    {
        if (g_bdActive.exchange(0, std::memory_order_relaxed))
        {
            g_bdScenePoseValid.store(0, std::memory_order_relaxed);
            BraindanceCameraRelease();
            if (g_verboseLog) Log("VRSceneCamera: gate OFF\n");
        }
        if (aOut) *aOut = 0;
        return;
    }

    // THE REFERENCE POSE, AND NOTHING ELSE. An earlier build wrote this straight into the second eye's
    // lens, which was a guess with no camera behind it: it moved that eye to a place the renderer was not
    // rendering from. What the pose is FOR is finding the object -- see BraindanceCameraMatch -- and once
    // that object is latched the lens comes from the object itself, exactly as it does for a surveillance
    // camera. Two writers for one lens is how the second eye ended up fighting itself.
    const float ql = qi*qi + qj*qj + qk*qk + qr*qr;
    if (ql > 0.9f && ql < 1.1f)
    {
        g_bdSceneQuat[0] = qi; g_bdSceneQuat[1] = qj; g_bdSceneQuat[2] = qk; g_bdSceneQuat[3] = qr;
    }
    // The same fixed point the camera components keep their world position in, so the comparison in
    // BraindanceCameraMatch is one subtraction with no unit conversion to get wrong.
    g_bdScenePosFP[0].store(static_cast<int32_t>(x * 131072.0f), std::memory_order_relaxed);
    g_bdScenePosFP[1].store(static_cast<int32_t>(y * 131072.0f), std::memory_order_relaxed);
    g_bdScenePosFP[2].store(static_cast<int32_t>(z * 131072.0f), std::memory_order_relaxed);
    g_bdScenePoseValid.store(1, std::memory_order_release);
    // The braindance camera hunt in PatchCamera tests EVERY component the site passes, so the
    // pointer filter has to stand down for the whole scene. patch_fast_note also refuses to arm
    // while this flag is set; this call is what closes the window between the two.
    PatchFastDisarm();
    if (!g_bdActive.exchange(1, std::memory_order_release))
        if (g_verboseLog) Log("VRSceneCamera: gate ON, scene camera at (%.2f %.2f %.2f) q=(%.3f %.3f %.3f %.3f)\n",
            static_cast<double>(x), static_cast<double>(y), static_cast<double>(z),
            static_cast<double>(qi), static_cast<double>(qj),
            static_cast<double>(qk), static_cast<double>(qr));
    if (aOut) *aOut = 1;
}

// BRAINDANCE, and it is the same problem as a surveillance camera with one thing missing: script can
// see that a braindance is running but cannot hand over the camera object, because that camera belongs
// to the scene. What it CAN hand over is the FOV the game reports for the active camera -- measured 55.9
// inside one, against the 104 this port forces for the headset -- and the plugin latches the component
// whose own fov field equals it. See ClassifyPatchCameraOwner.
void VRBraindance(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    int32_t active = 0;
    float fov = 0.0f;
    RED4ext::GetParameter(aFrame, &active);
    RED4ext::GetParameter(aFrame, &fov);
    aFrame->code++;

    if (active == 0)
    {
        if (g_bdActive.exchange(0, std::memory_order_relaxed))
        {
            g_bdScenePoseValid.store(0, std::memory_order_relaxed);
            BraindanceCameraRelease();
            // Hand the camera back exactly as the takeover path does when it ends, and drop the lens so
            // nothing of this braindance can be reused by whatever comes next.
            DeviceCamRestoreFov();
            g_devCamPosValid.store(0, std::memory_order_relaxed);
            g_devCamAimValid.store(0, std::memory_order_relaxed);
            g_devCamBaseValid.store(0, std::memory_order_relaxed);
            g_devCamViewValid.store(0, std::memory_order_relaxed);
        }
        g_bdWantFovMilli.store(0, std::memory_order_relaxed);
        if (aOut) *aOut = 0;
        return;
    }

    if (fov > 5.0f && fov < 179.0f)
        g_bdWantFovMilli.store(static_cast<int>(fov * 1000.0f), std::memory_order_relaxed);
    PatchFastDisarm();                                   // see the note at the other arm site
    g_bdActive.store(1, std::memory_order_release);
    if (aOut) *aOut = 1;
}

void VRRemoteCamera(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    int32_t active = 0;
    float x = 0.0f, y = 0.0f, z = 0.0f;
    RED4ext::GetParameter(aFrame, &active);
    RED4ext::GetParameter(aFrame, &x);
    RED4ext::GetParameter(aFrame, &y);
    RED4ext::GetParameter(aFrame, &z);
    aFrame->code++;

    if (active == 0)
    {
        g_remoteCamOn.store(0, std::memory_order_relaxed);
        // The camera's fov was raised to the one the headset needs; give the object back as it was found.
        DeviceCamRestoreFov();
        // And drop the lens, so nothing can be reused for whatever is controlled next. This is the only
        // place they are cleared: a timer clearing them mid-takeover put the second eye back on the
        // player for a frame.
        g_devCamPosValid.store(0, std::memory_order_relaxed);
        g_devCamAimValid.store(0, std::memory_order_relaxed);
        g_devCamBaseValid.store(0, std::memory_order_relaxed);
        g_devCamViewValid.store(0, std::memory_order_relaxed);
        // ...AND THE OBJECTS THEMSELVES. The lens is read every frame while it is bound, so a pointer left
        // behind after the entity unloads is a read into freed memory -- which this process turns into a
        // crash, since __try does not catch it here. The sticky latch goes too: kept, it would reject every
        // candidate of the NEXT takeover, because a claim only happens while nothing is held.
        TakeoverLensRelease();
        if (aOut) *aOut = 0;
        return;
    }

    // The same fixed point the camera components store their world position in, so the comparison in
    // DeviceCamPositionMatches is one subtraction with no unit conversion to get wrong.
    g_remoteCamPosFP[0].store(static_cast<int32_t>(x * 131072.0f), std::memory_order_relaxed);
    g_remoteCamPosFP[1].store(static_cast<int32_t>(y * 131072.0f), std::memory_order_relaxed);
    g_remoteCamPosFP[2].store(static_cast<int32_t>(z * 131072.0f), std::memory_order_relaxed);
    g_remoteCamOn.store(1, std::memory_order_release);
    // THE POINTER FILTER STANDS DOWN UNTIL THE LENS IS FOUND. The trampoline admits only the owners in
    // g_patchFast, so with it armed no component of the taken-over entity ever reaches the classifier --
    // and one of them is the only way to learn the entity, hence the lens. patch_fast_note refuses to arm
    // in this state as well; this call is what closes the window between the two, exactly as the
    // braindance hunt does above.
    if (g_lensComp.load(std::memory_order_relaxed) == 0) PatchFastDisarm();
    if (aOut) *aOut = 1;
}
