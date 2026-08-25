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
#include "Natives/NativeFunctions.hpp"

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
        if (aOut) *aOut = 0;
        return;
    }

    // The same fixed point the camera components store their world position in, so the comparison in
    // DeviceCamPositionMatches is one subtraction with no unit conversion to get wrong.
    g_remoteCamPosFP[0].store(static_cast<int32_t>(x * 131072.0f), std::memory_order_relaxed);
    g_remoteCamPosFP[1].store(static_cast<int32_t>(y * 131072.0f), std::memory_order_relaxed);
    g_remoteCamPosFP[2].store(static_cast<int32_t>(z * 131072.0f), std::memory_order_relaxed);
    g_remoteCamOn.store(1, std::memory_order_release);
    if (aOut) *aOut = 1;
}
