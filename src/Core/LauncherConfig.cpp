// LauncherConfig -- where this build's files are, and what the launcher decided.
//
// Paths first, because everything else needs them: the module directory, the game's bind, the CET
// mod folders. They are resolved from the LOADED MODULE rather than assumed, because a red4ext plugin
// does not run with the game's working directory and every relative path here would otherwise resolve
// somewhere else on a tester's machine.
//
// EnsureLiveControlFileExists writes the ini if it is missing rather than failing: the overlay and the
// launcher both read it, and a missing file used to mean every live control silently kept its compiled
// default. A file that exists with defaults in it is inspectable; an absent one is not.

#include <windows.h>
#include <psapi.h>
#include <xinput.h>
#include "Utils/SharedSlots.hpp"   // CyberpunkVR_Hands_Shared slot map (single source of truth)
#include <cstdint>
#include <cstddef>
#include <cstdio>
#include <cstdarg>
#include <cstring>
#include <cmath>
#include <share.h>
#include "Utils/AobScanner.hpp"
#include "Overlay/LiveControlsUi.hpp"
#include "Overlay/LauncherDialog.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include "Runtimes/RuntimeFovCorrection.hpp"
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Scripting/Natives/ScriptGameInstance.hpp>
#include <iostream>
#include <MinHook.h>
#include "Hooks/SwapChain.hpp"
#include "Utils/LogThrottle.hpp"
#include "Hooks/Trampoline.hpp"
#include "Utils/MemorySafe.hpp"
#include "Core/Telemetry.hpp"
#include "Core/LiveControls.hpp"
#include "Core/VrCoreShared.hpp"
#include "Core/CoreInternal.hpp"
#include "Camera/CameraLink.hpp"
#include "Hooks/Hook.hpp"

// Non-static: the RED4ext plugin entry calls this in place of DllMain.
void InitRuntimePaths() {
    if (g_gameDir[0] != '\0') return;

    GetModuleFileNameA(nullptr, g_gameDir, MAX_PATH);
    char* lastSlash = strrchr(g_gameDir, '\\');
    if (lastSlash) {
        *lastSlash = '\0';
    }

    strcpy_s(g_liveControlPath, g_gameDir);
    strcat_s(g_liveControlPath, "\\vrport.ini");

    strcpy_s(g_launcherConfigPath, g_gameDir);
    strcat_s(g_launcherConfigPath, "\\vrport-launcher.ini");

    strcpy_s(g_vrikSettingsPath, g_gameDir);
    strcat_s(g_vrikSettingsPath, "\\plugins\\cyber_engine_tweaks\\mods\\CyberpunkVRPort_VRIK\\vrik_settings.ini");
    strcpy_s(g_vrikRecenterPath, g_gameDir);
    strcat_s(g_vrikRecenterPath, "\\plugins\\cyber_engine_tweaks\\mods\\CyberpunkVRPort_VRIK\\vrik_recenter.ini");

    // Default: mouse pitch suppressed (VR uses the HMD for pitch).
    g_liveControls.xrDisableMouseY = 1;

    // Default: immersive holsters ON (current behaviour -- equip by visual holster).
    g_liveControls.xrImmersiveHolsters = 1;

    // The in-vehicle head offset starts at zero: it is a correction on top of the standing
    // calibration, and a default that moved the car view would be a surprise, not a feature.
    g_liveControls.xrVehHeadOffsetX = 0.0f;
    g_liveControls.xrVehHeadOffsetY = 0.0f;
    g_liveControls.xrVehHeadOffsetZ = 0.0f;

    // DRIVING. Wheel grab ON. 0.28 m is a hand-sized reach around the animated wheel pose -- wide
    // enough that you do not have to hunt for a wheel you cannot see, tight enough that a hand
    // resting on your knee never arms it.
    g_liveControls.xrWheelGrab = 1;
    g_liveControls.xrWheelRadius = 0.28f;
    // 90 deg = hands vertical is full lock, the 1:1 reading of a real wheel.
    g_liveControls.xrWheelSteerMaxDeg = 90.0f;
    // 1.5 deg of deadzone: enough to swallow tremor, small enough that the wheel does not feel dead
    // off centre. Every degree here is a degree the car ignores.
    g_liveControls.xrWheelSteerDeadDeg = 1.5f;
    // Horn ON: a hand on the middle of the wheel honks, as it would in a real car. 12 cm is the hub
    // pad, comfortably inside the ~17 cm the animation holds the rim at.
    // OFF: the horn gesture has no button left. Vehicle_Horn is X, and X is the exit now -- see
    // the horn block in XInput.cpp. On, the gesture is counted and nothing is pressed.
    g_liveControls.xrWheelHorn = 0;
    g_liveControls.xrWheelHornRadius = 0.12f;
    // Driving with a gun: trigger = fire, throttle latched. 0.5 per second on the trim means the
    // stick takes two seconds held to walk the throttle from idle to floored -- fast enough to chase
    // a target, slow enough that a thumb resting on the stick does not change your speed.
    g_liveControls.xrVehicleGunTrigger = 1;
    g_liveControls.xrVehicleThrottleTrim = 0.5f;

    // Default: suspend VRIK during true cinematics (Tier4_FPPCinematic and up). The avatar is
    // driven by the engine authored scene animation there, and VRIK fighting it looks wrong.
    g_liveControls.xrCutsceneSuspendTier = 3;

    // Default ON: VR controller -> XInput gamepad pipeline. Both the entry-point
    // detour (xrXInputInstall) and the gameplay action set (xrInputActions) are
    // required for the game to see the controller; without them CP2077 detects no
    // pad and shows keyboard glyphs. Applied here so an ini missing these keys
    // still enables the controller. Override to 0 in vrport.ini on a runtime where
    // the binding/entry-point patch keeps the game from reaching its main menu.
    g_liveControls.xrXInputInstall = 1;
    g_liveControls.xrInputActions = 1;

    // Off until the user opts in: Quest 3 canted optics are the reason this exists,
    // and a default-on correction on a symmetric headset is a surprise, not a feature.
    g_liveControls.xrLensBoxCenter = 0;
    g_liveControls.xrViewBoxPitchDeg = 0.0f;
    g_liveControls.xrViewBoxYawDeg = 0.0f;

    // Capture the recenter-request baseline NOW (before CET could write), so the
    // first OnGameAttached this session is seen as a change and triggers a recenter,
    // while a stale counter left over from a previous session does not.
    WIN32_FILE_ATTRIBUTE_DATA rfd;
    if (GetFileAttributesExA(g_vrikRecenterPath, GetFileExInfoStandard, &rfd)) {
        g_lastVrikRecenterWrite = rfd.ftLastWriteTime;
        FILE* rf = _fsopen(g_vrikRecenterPath, "r", _SH_DENYNO);
        if (rf) {
            char line[64]; int v = 0;
            while (fgets(line, sizeof(line), rf)) {
                if (sscanf_s(line, "recenter=%d", &v) == 1) { g_lastVrikRecenterCounter = v; break; }
            }
            fclose(rf);
        }
    }
}



void EnsureLiveControlFileExists() {
    InitRuntimePaths();

    DWORD attrs = GetFileAttributesA(g_liveControlPath);
    if (attrs != INVALID_FILE_ATTRIBUTES) return;

    FILE* file = _fsopen(g_liveControlPath, "w", _SH_DENYNO);
    if (!file) return;

    fprintf(file, "xr_head_offset_x=0.000\n");
    fprintf(file, "xr_head_offset_y=0.000\n");
    fprintf(file, "xr_head_offset_z=0.000\n");
    fprintf(file, "xr_recenter=0\n");
    fprintf(file, "xr_mono_submit=1\n");
    // Who owns the XR frame loop: -1 auto (submit thread on SteamVR, inline pump on Virtual
    // Desktop), 0 force inline, 1 force the thread.
    fprintf(file, "xr_threaded_submit=-1\n");
    // 1 keeps the cascade SaveMain fix (MAIN samples the depth the second view wrote); 0 lets
    // MAIN clear the shared shadow atlas again. Revert to 0 if sun shadows go wrong in one eye.
    fprintf(file, "xr_cascade_save_main=1\n");
    fprintf(file, "xr_force_fov=0\n");
    fprintf(file, "xr_menu_rect=0\n");
    fprintf(file, "xr_menu_fov=65.0\n");
    fprintf(file, "xr_menu_follow_deg=60.0\n");
    fprintf(file, "xr_3dof_movement=0\n");
    fprintf(file, "first_launch=1\n");
    fprintf(file, "xr_motion_predict_ms=0.0\n");
    fprintf(file, "xr_stereo_scale=1.0\n");
    fprintf(file, "xr_world_scale=1.0\n");
    fprintf(file, "xr_ipd_scale=1.0\n");
    fprintf(file, "xr_sharpness=0.0\n");
    fprintf(file, "xr_sharpmix=1.0\n");
    fprintf(file, "xr_reuse_last_frame=0\n");
    fprintf(file, "xr_hmd_smooth=0.35\n");
    // UEVR's own default, in their units: follow-per-second, multiplied by delta time where it is
    // used, so the time constant belongs to the setting and not to the frame rate. The key it replaces
    // was a fraction per FRAME, and 0.45 of that was about 190 ms at 52 fps -- three times heavier than
    // the reference and applied to the raw pose rather than to what the arms read.
    // 25, not 15: the per-class values ship at 25/15/15/10/40, and the base is what a class of 0
    // falls back to -- a base below the commonest class made the fallback the odd one out.
    fprintf(file, "xr_hand_lerp_speed=25.0\n");
    fprintf(file, "xr_pair_lock=0\n");
    fprintf(file, "xr_render_pose_submit=1\n");
    fprintf(file, "xr_pose_lag=1\n");
    fprintf(file, "xr_runtime=0\n");
    // Default ON: now safe via cross-queue Signal hook (CyberpunkVRPort_
    // WaitOnAllGameSignals) that GPU-Waits on every tracked game queue
    // before our depth copy. Lets the compositor do depth-aware reprojection
    // → fixes far-building shift on head turn (parallax-correct timewarp
    // instead of orientation-only). Users on broken runtimes can set 0.
    fprintf(file, "xr_depth_submit=1\n");
    fprintf(file, "xr_movement_control=0\n");
    fprintf(file, "xr_disable_mouse_y=1\n");
    fprintf(file, "xr_xinput_hook=1\n");
    fprintf(file, "xr_snap_turn=0\n");
    fprintf(file, "xr_snap_turn_angle_deg=30\n");
    fprintf(file, "xr_movement_source=0\n");
    // Default ON for the gameplay-input pipeline: both flags are required for the
    // VR controller to reach CP2077 as an XInput pad (otherwise the game detects no
    // controller and shows keyboard glyphs). Set either to 0 in vrport.ini if a
    // busted runtime binding or the 14-byte XInput entry-point patch keeps the game
    // from reaching its main menu.
    fprintf(file, "xr_xinput_install=1\n");
    fprintf(file, "xr_input_actions=1\n");
    fprintf(file, "xr_mono_xqueue_wait=0\n");
    fprintf(file, "xr_snap_turn_pulse_ms=30\n");
    fprintf(file, "xr_mono_depth_capture=1\n");
    fprintf(file, "xr_lens_box_center=0\n");
    fprintf(file, "xr_view_box_pitch_deg=0.0\n");
    fprintf(file, "xr_view_box_yaw_deg=0.0\n");
    fclose(file);
}

void LoadLauncherConfig() {
    InitRuntimePaths();
    g_launcherWidth = 2048;
    g_launcherHeight = 2048;
    g_launcherHmdType = 0;
    g_launcherDebug = 0;

    FILE* file = _fsopen(g_launcherConfigPath, "r", _SH_DENYNO);
    if (!file) return;

    char line[128];
    while (fgets(line, sizeof(line), file)) {
        int intValue = 0;
        if (sscanf_s(line, "width=%d", &intValue) == 1 ||
            sscanf_s(line, "width = %d", &intValue) == 1) {
            g_launcherWidth = intValue > 0 ? intValue : g_launcherWidth;
            continue;
        }
        if (sscanf_s(line, "height=%d", &intValue) == 1 ||
            sscanf_s(line, "height = %d", &intValue) == 1) {
            g_launcherHeight = intValue > 0 ? intValue : g_launcherHeight;
            continue;
        }
        // <-- NUOVO: parsing hmd_type
        if (sscanf_s(line, "hmd_type=%d", &intValue) == 1 ||
            sscanf_s(line, "hmd_type = %d", &intValue) == 1) {
            g_launcherHmdType = intValue;
            continue;
        }
        if (sscanf_s(line, "debug=%d", &intValue) == 1 ||
            sscanf_s(line, "debug = %d", &intValue) == 1) {
            g_launcherDebug = intValue != 0 ? 1 : 0;
            continue;
        }
    }
    fclose(file);
}

void SaveLauncherConfig(int width, int height) {
    InitRuntimePaths();
    g_launcherWidth = width > 0 ? width : g_launcherWidth;
    g_launcherHeight = height > 0 ? height : g_launcherHeight;

    FILE* file = _fsopen(g_launcherConfigPath, "w", _SH_DENYNO);
    if (!file) return;
    fprintf(file, "width=%d\n", g_launcherWidth);
    fprintf(file, "height=%d\n", g_launcherHeight);
    fprintf(file, "hmd_type=%d\n", g_launcherHmdType);
    fprintf(file, "debug=%d\n", g_launcherDebug);
    fclose(file);
}

