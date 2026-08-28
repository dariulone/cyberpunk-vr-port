// LiveControlsPoll -- reading the forty-odd live controls, once per tick, off the game threads.
//
// The controls are volatile scalars that hooks read directly (see Core/LiveControls.hpp for why there
// are no accessors). This file is the only writer, and it runs on the worker thread, which is what makes
// direct reads safe enough: a hook sees either the old value or the new one, never a half-written
// struct, because nothing here writes anything wider than a scalar.
//
// READ THE FILE, NOT THE SOURCE DEFAULT. A value that looks wrong is checked against vrport.ini, not
// against the initialiser in the header -- xr_motion_predict_ms sat at 50.9 in the ini while the source
// said something else, and that was the VR judder for a week.
//
// MakeLiveControlsUiState / PersistLiveControlsUiState are the overlay's side: it edits a snapshot and
// hands it back, so a slider dragged in the headset survives a restart.

#include "Overlay/ImGuiOverlay.hpp"   // OverlayArmLoadGuard
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

// Defined in src\Stereo\ViewReuse.cpp. Declared here rather than in a header because that
// is how the stereo module already shares it (CommandListCensus.cpp does the same), and this
// file only needs to hand it the value read out of vrport.ini.
extern "C" __declspec(dllexport) extern int32_t CyberpunkVR_CascadeSaveMain;
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

// Poll the CET VRIK mod's recenter request (written with an incrementing counter on
// save load / OnGameAttached); recenter when the counter changes.
static void PollVrikRecenterRequest() {
    InitRuntimePaths();
    WIN32_FILE_ATTRIBUTE_DATA fd;
    if (!GetFileAttributesExA(g_vrikRecenterPath, GetFileExInfoStandard, &fd)) return;
    if (CompareFileTime(&fd.ftLastWriteTime, &g_lastVrikRecenterWrite) == 0) return;
    g_lastVrikRecenterWrite = fd.ftLastWriteTime;

    FILE* file = _fsopen(g_vrikRecenterPath, "r", _SH_DENYNO);
    if (!file) return;
    char line[64];
    int counter = -1;
    while (fgets(line, sizeof(line), file)) {
        int v = 0;
        if (sscanf_s(line, "recenter=%d", &v) == 1) { counter = v; break; }
    }
    fclose(file);
    if (counter == 0) return;
    // Baseline was captured at startup (InitRuntimePaths); any later change = a fresh
    // OnGameAttached this session.
    if (counter != g_lastVrikRecenterCounter) {
        g_lastVrikRecenterCounter = counter;
        OpenXRManager::Get().RequestRecenter();
        // THE SAME SIGNAL MARKS THE LOAD for the overlay's pacing guard: on the branch this pacing
        // came from, every recorded DXGI_ERROR_DEVICE_HUNG landed within a few frames of this point.
        OverlayArmLoadGuard("save load");
        Log("VRIK recenter request (save load) -> recentering. counter=%d\n", counter);
    }
}

// Tracking-smoothing accessors (atomics live in openxr_manager.cpp). The proxy
// owns their ini persistence: parse -> Set* on file change, Get* -> write on Save.
extern "C" float GetHmdTrackingSmooth(); extern "C" void SetHmdTrackingSmooth(float);
// How near the support point the off hand has to be before the two-handed hold is offered, in metres.
extern "C" __declspec(dllexport) extern float CyberpunkVR_TwoHandRadius;
// WHERE THE SCANNER'S HUD SITS -- four movable pieces of it, as (x, y, scale) each. Written by the
// in-game editor through VRScannerSlotSet, read back by the CyberpunkVRPort_ScannerHud redscript.
//
//   [0..2]    the scanner frame      scannerGameController
//   [3..5]    the details panel      scannerDetailsGameController
//   [6..8]    the quickhack panel    QuickhacksListGameController
//   [9..11]   the hint line          ScannerHintInkGameController
//   [12..14]  the cyberdeck memory   the panel's top_panel container
//   [15..17]  the script list        the panel's left_panel container
//   [18..20]  the script description the panel's right_panel container
//
// The last three are CHILDREN of the quickhack panel above them, which is the point: they shared
// one movable block and could not be separated. Their offsets compose with the parent's -- move
// the panel and they follow, then nudge one of them on top of that.
//
// They are CONTAINERS, named as the .inkwidget names them, and that is a correction: the first
// version moved the widgets the controller keeps private refs to -- cells_memory and the list --
// which are INSIDE those containers, so the background, the separator line and the headings stayed
// behind. top_panel holds the memory's title, line, fluff and cells; left_panel holds the list and
// its heading; right_panel holds the description block.
//
// It started as ONE triple for the whole scanner, which was wrong before it was written: these are
// four separate widgets sitting in four places, so a single offset can only be right for one of them.
//
// Offsets are in the HUD's own design pixels (1920x1080), positive x right and positive y down, which
// is the convention inkWidget.SetTranslation takes. All four ship at (0, 0, 1): a mod that moves
// somebody's HUD before they ask it to is a bug, however good the default.
//
// AND THIS FILE IS THE PERSISTENCE, not just the source. A redscript cannot write a file and the CET
// bridge is not a dependency this wants, so the editor writes into these globals and asks for one ini
// save when it closes. The poll below only re-reads the ini when its write time CHANGES, so a live
// drag is not clobbered by the next poll.
// NOT ZEROES ANY MORE. These are the numbers the layout was actually dragged to in the headset, read
// back out of vrport.ini, and they ship because that ini is per-install and never enters the
// repository: on a fresh install zeroes mean the vanilla layout and every tester hunting the same
// numbers again. An existing ini still wins -- the poll below reads the file over these -- so nobody's
// own layout is touched. Same trade the port already makes with HUDitor's persistency.json.
extern "C" __declspec(dllexport) float CyberpunkVR_ScannerSlots[21] = {
    -10.4f,    0.0f, 1.000f,   // the scanner frame
   -178.2f,  -57.4f, 0.400f,   // the details panel
    -75.4f,    1.8f, 0.800f,   // the quickhack panel
      0.4f,    1.0f, 1.000f,   // the hint line
     17.6f,  517.8f, 0.700f,   // top_panel: the cyberdeck memory, whole
    260.8f,   -8.0f, 0.600f,   // left_panel: the script list and its heading
      0.0f,    0.0f, 1.000f,   // right_panel: the description block, left where it was
};
// The hand filter is UEVR-form now and lives in FlushHandsToShared; its speed is this.
extern "C" __declspec(dllexport) extern float CyberpunkVR_HandLerpSpeed;
// 1 = hand offsets measured from the filtered head (the one they are re-anchored on).
extern "C" __declspec(dllexport) extern int CyberpunkVR_HandRelToFilteredHead;
// 1 = the published hand pose is located once per frame, for that frame's instant.
extern "C" __declspec(dllexport) extern int CyberpunkVR_HandLocatePerFrame;

extern "C" __declspec(dllexport) bool GetWeaponAimEnabled() {
    return OpenXRManager::Get().GetWeaponAimEnable();
}

void PollLiveControls() {
    InitRuntimePaths();
    PollVrikRecenterRequest();

    WIN32_FILE_ATTRIBUTE_DATA fileData;
    if (!GetFileAttributesExA(g_liveControlPath, GetFileExInfoStandard, &fileData)) {
        return;
    }

    if (CompareFileTime(&fileData.ftLastWriteTime, &g_lastLiveControlWrite) == 0) {
        return;
    }

    g_lastLiveControlWrite = fileData.ftLastWriteTime;

    float xrHeadOffsetX = 0.0f;
    float xrHeadOffsetY = 0.0f;
    float xrHeadOffsetZ = 0.0f;
    int xrRecenter = 0;
    int xrMonoSubmit = 1;
    // Seeded from the live value, not from a constant: an ini without the key must leave the
    // current mode alone rather than reset it.
    int xrThreadedSubmit = CyberpunkVR_ThreadedMonoSubmit;
    int xrCascadeSaveMain = CyberpunkVR_CascadeSaveMain;
    int xrWindowWidth = 0;
    int xrWindowHeight = 0;
    float xrForceFov = 0.0f;
    int xrMenuRect = 0;
    float xrMenuFov = 65.0f;
    float xrMenuFollowDeg = 60.0f;
    float xrPitchSign = 1.0f;
    float xrPitchScale = 1.35f;
    int xrSyncSequential = 1;
    int xr3DofMovement = 0;
    int xrFirstLaunch = 1;
    float xrMotionPredictMs = 0.0f;
    float xrStereoScale = 1.0f;
    float xrWorldScale = 1.0f;
    float xrIpdScale = 1.0f;
    float xrSharpness = 0.0f;
    float xrSharpmix = 1.0f;
    int xrReuseLastFrame = 0;
    int xrPairLock = 0;
    int xrRenderPoseSubmit = 1;
    int xrPoseLag = 1;
    int xrRuntime = 0;
    // Default ON: cross-queue Signal hook now serializes our depth read
    // against the game's render writers. Compositor depth-aware reprojection
    // fixes far-object shift on head turn. Users can still override via ini.
    int xrDepthSubmit = 1;
    int xrMovementControl = g_liveControls.xrMovementControl;
    int xrDisableMouseY = g_liveControls.xrDisableMouseY;
    int xrXInputHook = g_liveControls.xrXInputHook != 0 ? g_liveControls.xrXInputHook : 1;
    int xrSnapTurn = g_liveControls.xrSnapTurn;
    float xrHmdSmooth = GetHmdTrackingSmooth();
    float xrHandLerp = CyberpunkVR_HandLerpSpeed;
    float xrTwoHandRadius = CyberpunkVR_TwoHandRadius;
    float xrScannerSlots[21];
    for (int i = 0; i < 21; ++i) xrScannerSlots[i] = CyberpunkVR_ScannerSlots[i];
    int   xrHandRelFiltered = CyberpunkVR_HandRelToFilteredHead;
    int   xrHandPerFrame    = CyberpunkVR_HandLocatePerFrame;
    static const char kLegacyReuseLastFrameKey[] = {
        'x','r','_','o','u','t','p','u','t','_','r','e','a','l','v','r',0
    };
    auto tryParseIntKey = [](const char* text, const char* key, int* outValue) {
        if (!text || !key || !outValue) return false;
        const size_t keyLen = strlen(key);
        if (_strnicmp(text, key, keyLen) != 0) return false;
        const char* cursor = text + keyLen;
        while (*cursor == ' ' || *cursor == '\t') ++cursor;
        if (*cursor != '=') return false;
        ++cursor;
        while (*cursor == ' ' || *cursor == '\t') ++cursor;
        *outValue = atoi(cursor);
        return true;
    };
    float xrSnapTurnAngleDeg = g_liveControls.xrSnapTurnAngleDeg > 0.0f ? g_liveControls.xrSnapTurnAngleDeg : 30.0f;
    int xrMovementSource = g_liveControls.xrMovementSource;
    int xrLaserDotMode = g_liveControls.xrLaserDotMode;
    int xrXInputInstall = g_liveControls.xrXInputInstall;
    int xrInputActions = g_liveControls.xrInputActions;
    int xrMonoXQueueWait = g_liveControls.xrMonoXQueueWait;
    int xrSnapTurnPulseMs = g_liveControls.xrSnapTurnPulseMs > 0 ? g_liveControls.xrSnapTurnPulseMs : 30;
    int xrMonoDepthCapture = g_liveControls.xrMonoDepthCapture;
    int xrSnapTurnYawIndex = g_liveControls.xrSnapTurnYawIndex >= 0 && g_liveControls.xrSnapTurnYawIndex <= 3 ? g_liveControls.xrSnapTurnYawIndex : 1;
    int xrImmersiveHolsters = g_liveControls.xrImmersiveHolsters;
    int xrPhysicalBodyRotation = g_liveControls.xrPhysicalBodyRotation;
    int xrCutsceneSuspendTier = g_liveControls.xrCutsceneSuspendTier;
    float xrVehHeadOffsetX = g_liveControls.xrVehHeadOffsetX;
    float xrVehHeadOffsetY = g_liveControls.xrVehHeadOffsetY;
    float xrVehHeadOffsetZ = g_liveControls.xrVehHeadOffsetZ;
    int xrWheelGrab = g_liveControls.xrWheelGrab;
    float xrWheelRadius = g_liveControls.xrWheelRadius > 0.0f ? g_liveControls.xrWheelRadius : 0.28f;
    float xrWheelSteerMaxDeg = g_liveControls.xrWheelSteerMaxDeg > 0.0f ? g_liveControls.xrWheelSteerMaxDeg : 90.0f;
    float xrWheelSteerDeadDeg = g_liveControls.xrWheelSteerDeadDeg >= 0.0f ? g_liveControls.xrWheelSteerDeadDeg : 1.5f;
    int xrWheelHorn = g_liveControls.xrWheelHorn;
    float xrWheelHornRadius = g_liveControls.xrWheelHornRadius > 0.0f ? g_liveControls.xrWheelHornRadius : 0.12f;
    int xrVehicleGunTrigger = g_liveControls.xrVehicleGunTrigger;
    float xrVehicleThrottleTrim = g_liveControls.xrVehicleThrottleTrim > 0.0f ? g_liveControls.xrVehicleThrottleTrim : 0.5f;

    FILE* file = _fsopen(g_liveControlPath, "r", _SH_DENYNO);
    if (!file) return;

    char line[128];
    while (fgets(line, sizeof(line), file)) {
        float value = 0.0f;

        if (sscanf_s(line, "xr_head_offset_x=%f", &value) == 1 ||
            sscanf_s(line, "xr_head_offset_x = %f", &value) == 1) {
            xrHeadOffsetX = value;
            continue;
        }
        if (sscanf_s(line, "xr_head_offset_y=%f", &value) == 1 ||
            sscanf_s(line, "xr_head_offset_y = %f", &value) == 1) {
            xrHeadOffsetY = value;
            continue;
        }
        if (sscanf_s(line, "xr_head_offset_z=%f", &value) == 1 ||
            sscanf_s(line, "xr_head_offset_z = %f", &value) == 1) {
            xrHeadOffsetZ = value;
            continue;
        }
        int intValue = 0;
        if (sscanf_s(line, "xr_recenter=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_recenter = %d", &intValue) == 1) {
            xrRecenter = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_mono_submit=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_mono_submit = %d", &intValue) == 1) {
            xrMonoSubmit = intValue;
            continue;
        }
        // -1 auto (thread on SteamVR, inline on Virtual Desktop), 0 inline, 1 thread.
        if (sscanf_s(line, "xr_threaded_submit=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_threaded_submit = %d", &intValue) == 1) {
            xrThreadedSubmit = (intValue < 0) ? -1 : (intValue > 0 ? 1 : 0);
            continue;
        }
        // 1 keeps the cascade SaveMain fix, 0 lets MAIN clear the shared shadow atlas again.
        if (sscanf_s(line, "xr_cascade_save_main=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_cascade_save_main = %d", &intValue) == 1) {
            xrCascadeSaveMain = intValue != 0 ? 1 : 0;
            continue;
        }

        if (sscanf_s(line, "xr_window_width=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_window_width = %d", &intValue) == 1) {
            xrWindowWidth = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_window_height=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_window_height = %d", &intValue) == 1) {
            xrWindowHeight = intValue;
            continue;
        }

        if (sscanf_s(line, "xr_force_fov=%f", &value) == 1 ||
            sscanf_s(line, "xr_force_fov = %f", &value) == 1) {
            xrForceFov = value;
            continue;
        }
        if (sscanf_s(line, "xr_menu_rect=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_menu_rect = %d", &intValue) == 1) {
            xrMenuRect = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_menu_fov=%f", &value) == 1 ||
            sscanf_s(line, "xr_menu_fov = %f", &value) == 1) {
            xrMenuFov = value;
            continue;
        }
        if (sscanf_s(line, "xr_menu_follow_deg=%f", &value) == 1 ||
            sscanf_s(line, "xr_menu_follow_deg = %f", &value) == 1) {
            xrMenuFollowDeg = value;
            continue;
        }
        if (sscanf_s(line, "xr_pitch_sign=%f", &value) == 1 ||
            sscanf_s(line, "xr_pitch_sign = %f", &value) == 1) {
            xrPitchSign = value < 0.0f ? -1.0f : 1.0f;
            continue;
        }
        if (sscanf_s(line, "xr_pitch_scale=%f", &value) == 1 ||
            sscanf_s(line, "xr_pitch_scale = %f", &value) == 1) {
            xrPitchScale = value > 0.01f ? value : 1.0f;
            continue;
        }
        if (sscanf_s(line, "xr_sync_sequential=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_sync_sequential = %d", &intValue) == 1) {
            xrSyncSequential = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_3dof_movement=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_3dof_movement = %d", &intValue) == 1) {
            xr3DofMovement = intValue;
            continue;
        }
        if (sscanf_s(line, "first_launch=%d", &intValue) == 1 ||
            sscanf_s(line, "first_launch = %d", &intValue) == 1) {
            xrFirstLaunch = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_motion_predict_ms=%f", &value) == 1 ||
            sscanf_s(line, "xr_motion_predict_ms = %f", &value) == 1) {
            xrMotionPredictMs = value;
            continue;
        }
        if (sscanf_s(line, "xr_stereo_scale=%f", &value) == 1 ||
            sscanf_s(line, "xr_stereo_scale = %f", &value) == 1) {
            xrStereoScale = value;
            continue;
        }
        if (sscanf_s(line, "xr_world_scale=%f", &value) == 1 ||
            sscanf_s(line, "xr_world_scale = %f", &value) == 1) {
            xrWorldScale = value;
            continue;
        }
        if (sscanf_s(line, "xr_ipd_scale=%f", &value) == 1 ||
            sscanf_s(line, "xr_ipd_scale = %f", &value) == 1) {
            xrIpdScale = value;
            continue;
        }
        if (sscanf_s(line, "xr_sharpness=%f", &value) == 1 ||
            sscanf_s(line, "xr_sharpness = %f", &value) == 1) {
            xrSharpness = value;
            continue;
        }
        if (sscanf_s(line, "xr_sharpmix=%f", &value) == 1 ||
            sscanf_s(line, "xr_sharpmix = %f", &value) == 1) {
            xrSharpmix = value;
            continue;
        }
        if (sscanf_s(line, "xr_hmd_smooth=%f", &value) == 1 ||
            sscanf_s(line, "xr_hmd_smooth = %f", &value) == 1) {
            xrHmdSmooth = value;
            continue;
        }
        if (sscanf_s(line, "xr_hand_per_frame=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_hand_per_frame = %d", &intValue) == 1) {
            xrHandPerFrame = intValue != 0 ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_hand_rel_filtered=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_hand_rel_filtered = %d", &intValue) == 1) {
            xrHandRelFiltered = intValue != 0 ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_hand_lerp_speed=%f", &value) == 1 ||
            sscanf_s(line, "xr_hand_lerp_speed = %f", &value) == 1) {
            xrHandLerp = value;
            continue;
        }
        // Three numbers on one line per piece, because twelve keys for one feature is a wall of ini
        // nobody can read. The editor writes them back in exactly this shape.
        {
            float sx = 0.0f, sy = 0.0f, ss = 0.0f;
            int slot = -1;
            if      (sscanf_s(line, "xr_scanner_frame=%f,%f,%f",   &sx, &sy, &ss) == 3) slot = 0;
            else if (sscanf_s(line, "xr_scanner_details=%f,%f,%f", &sx, &sy, &ss) == 3) slot = 1;
            else if (sscanf_s(line, "xr_scanner_hacks=%f,%f,%f",   &sx, &sy, &ss) == 3) slot = 2;
            else if (sscanf_s(line, "xr_scanner_hint=%f,%f,%f",    &sx, &sy, &ss) == 3) slot = 3;
            else if (sscanf_s(line, "xr_scanner_memory=%f,%f,%f",  &sx, &sy, &ss) == 3) slot = 4;
            else if (sscanf_s(line, "xr_scanner_scripts=%f,%f,%f", &sx, &sy, &ss) == 3) slot = 5;
            else if (sscanf_s(line, "xr_scanner_desc=%f,%f,%f",    &sx, &sy, &ss) == 3) slot = 6;
            if (slot >= 0) {
                xrScannerSlots[slot * 3 + 0] = sx;
                xrScannerSlots[slot * 3 + 1] = sy;
                xrScannerSlots[slot * 3 + 2] = ss;
                continue;
            }
        }
        if (sscanf_s(line, "xr_two_hand_radius=%f", &value) == 1 ||
            sscanf_s(line, "xr_two_hand_radius = %f", &value) == 1) {
            xrTwoHandRadius = value;
            continue;
        }
        if (sscanf_s(line, "xr_render_pose_submit=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_render_pose_submit = %d", &intValue) == 1) {
            xrRenderPoseSubmit = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_reuse_last_frame=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_reuse_last_frame = %d", &intValue) == 1 ||
            tryParseIntKey(line, kLegacyReuseLastFrameKey, &intValue)) {
            xrReuseLastFrame = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_pair_lock=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_pair_lock = %d", &intValue) == 1) {
            xrPairLock = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_pose_lag=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_pose_lag = %d", &intValue) == 1) {
            xrPoseLag = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_runtime=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_runtime = %d", &intValue) == 1) {
            xrRuntime = ClampRuntimeMode(intValue);
            continue;
        }
        if (sscanf_s(line, "xr_depth_submit=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_depth_submit = %d", &intValue) == 1) {
            xrDepthSubmit = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_movement_control=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_movement_control = %d", &intValue) == 1) {
            xrMovementControl = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_disable_mouse_y=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_disable_mouse_y = %d", &intValue) == 1) {
            xrDisableMouseY = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_xinput_hook=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_xinput_hook = %d", &intValue) == 1) {
            xrXInputHook = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_snap_turn=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_snap_turn = %d", &intValue) == 1) {
            xrSnapTurn = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_snap_turn_angle_deg=%f", &value) == 1 ||
            sscanf_s(line, "xr_snap_turn_angle_deg = %f", &value) == 1) {
            xrSnapTurnAngleDeg = value;
            continue;
        }
        if (sscanf_s(line, "xr_movement_source=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_movement_source = %d", &intValue) == 1) {
            xrMovementSource = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_laser_dot_mode=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_laser_dot_mode = %d", &intValue) == 1) {
            xrLaserDotMode = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_cutscene_suspend_tier=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_cutscene_suspend_tier = %d", &intValue) == 1) {
            xrCutsceneSuspendTier = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_physical_body_rotation=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_physical_body_rotation = %d", &intValue) == 1) {
            xrPhysicalBodyRotation = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_xinput_install=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_xinput_install = %d", &intValue) == 1) {
            xrXInputInstall = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_input_actions=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_input_actions = %d", &intValue) == 1) {
            xrInputActions = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_mono_xqueue_wait=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_mono_xqueue_wait = %d", &intValue) == 1) {
            xrMonoXQueueWait = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_snap_turn_pulse_ms=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_snap_turn_pulse_ms = %d", &intValue) == 1) {
            xrSnapTurnPulseMs = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_mono_depth_capture=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_mono_depth_capture = %d", &intValue) == 1) {
            xrMonoDepthCapture = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_snap_turn_yaw_index=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_snap_turn_yaw_index = %d", &intValue) == 1) {
            xrSnapTurnYawIndex = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_immersive_holsters=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_immersive_holsters = %d", &intValue) == 1) {
            xrImmersiveHolsters = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_veh_head_offset_x=%f", &value) == 1 ||
            sscanf_s(line, "xr_veh_head_offset_x = %f", &value) == 1) {
            xrVehHeadOffsetX = value;
            continue;
        }
        if (sscanf_s(line, "xr_veh_head_offset_y=%f", &value) == 1 ||
            sscanf_s(line, "xr_veh_head_offset_y = %f", &value) == 1) {
            xrVehHeadOffsetY = value;
            continue;
        }
        if (sscanf_s(line, "xr_veh_head_offset_z=%f", &value) == 1 ||
            sscanf_s(line, "xr_veh_head_offset_z = %f", &value) == 1) {
            xrVehHeadOffsetZ = value;
            continue;
        }
        if (sscanf_s(line, "xr_wheel_grab=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_wheel_grab = %d", &intValue) == 1) {
            xrWheelGrab = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_wheel_radius=%f", &value) == 1 ||
            sscanf_s(line, "xr_wheel_radius = %f", &value) == 1) {
            xrWheelRadius = value;
            continue;
        }
        if (sscanf_s(line, "xr_wheel_steer_max_deg=%f", &value) == 1 ||
            sscanf_s(line, "xr_wheel_steer_max_deg = %f", &value) == 1) {
            xrWheelSteerMaxDeg = value;
            continue;
        }
        if (sscanf_s(line, "xr_wheel_steer_dead_deg=%f", &value) == 1 ||
            sscanf_s(line, "xr_wheel_steer_dead_deg = %f", &value) == 1) {
            xrWheelSteerDeadDeg = value;
            continue;
        }
        if (sscanf_s(line, "xr_wheel_horn=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_wheel_horn = %d", &intValue) == 1) {
            xrWheelHorn = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_wheel_horn_radius=%f", &value) == 1 ||
            sscanf_s(line, "xr_wheel_horn_radius = %f", &value) == 1) {
            xrWheelHornRadius = value;
            continue;
        }
        if (sscanf_s(line, "xr_vehicle_gun_trigger=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_vehicle_gun_trigger = %d", &intValue) == 1) {
            xrVehicleGunTrigger = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_vehicle_throttle_trim=%f", &value) == 1 ||
            sscanf_s(line, "xr_vehicle_throttle_trim = %f", &value) == 1) {
            xrVehicleThrottleTrim = value;
            continue;
        }

    }
    fclose(file);

    const int prevXrRecenter = g_liveControls.xrRecenter;
    const int prevXrMonoSubmit = g_liveControls.xrMonoSubmit;
    const bool changed = g_liveControls.xrHeadOffsetX != xrHeadOffsetX ||
        g_liveControls.xrHeadOffsetY != xrHeadOffsetY ||
        g_liveControls.xrHeadOffsetZ != xrHeadOffsetZ ||
        g_liveControls.xrRecenter != xrRecenter ||
        g_liveControls.xrMonoSubmit != xrMonoSubmit ||
        g_liveControls.xrForceFov != xrForceFov ||
        g_liveControls.xrMenuRect != xrMenuRect ||
        g_liveControls.xrMenuFov != xrMenuFov ||
        g_liveControls.xrMenuFollowDeg != xrMenuFollowDeg ||
        g_liveControls.xr3DofMovement != xr3DofMovement ||
        g_liveControls.xrFirstLaunch != xrFirstLaunch ||
        g_liveControls.xrMotionPredictMs != xrMotionPredictMs ||
        g_liveControls.xrStereoScale != xrStereoScale ||
        g_liveControls.xrWorldScale != xrWorldScale ||
        g_liveControls.xrIpdScale != xrIpdScale ||
        g_liveControls.xrSharpness != xrSharpness ||
        g_liveControls.xrSharpmix != xrSharpmix ||
        g_liveControls.xrReuseLastFrame != xrReuseLastFrame ||
        g_liveControls.xrPairLock != xrPairLock ||
        g_liveControls.xrRenderPoseSubmit != xrRenderPoseSubmit ||
        g_liveControls.xrRuntime != xrRuntime ||
        g_liveControls.xrDepthSubmit != xrDepthSubmit;

    g_liveControls.xrHeadOffsetX = xrHeadOffsetX;
    g_liveControls.xrHeadOffsetY = xrHeadOffsetY;
    g_liveControls.xrHeadOffsetZ = xrHeadOffsetZ;
    g_liveControls.xrRecenter = xrRecenter;
    g_liveControls.xrMonoSubmit = xrMonoSubmit;
    g_liveControls.xrForceFov = xrForceFov;
    g_liveControls.xrMenuRect = xrMenuRect;
    g_liveControls.xrMenuFov = xrMenuFov;
    g_liveControls.xrMenuFollowDeg = xrMenuFollowDeg;
    g_liveControls.xr3DofMovement = xr3DofMovement;
    g_liveControls.xrFirstLaunch = xrFirstLaunch != 0 ? 1 : 0;
    g_liveControls.xrMotionPredictMs = xrMotionPredictMs >= 0.0f ? xrMotionPredictMs : 0.0f;
    g_liveControls.xrStereoScale = xrStereoScale < 0.0f ? 0.0f : (xrStereoScale > 10.0f ? 10.0f : xrStereoScale);
    g_liveControls.xrWorldScale = xrWorldScale < 0.05f ? 0.05f : (xrWorldScale > 20.0f ? 20.0f : xrWorldScale);
    g_liveControls.xrIpdScale = xrIpdScale < 0.0f ? 0.0f : (xrIpdScale > 5.0f ? 5.0f : xrIpdScale);
    g_liveControls.xrSharpness = xrSharpness < 0.0f ? 0.0f : (xrSharpness > 1.0f ? 1.0f : xrSharpness);
    g_liveControls.xrSharpmix = xrSharpmix < 0.0f ? 0.0f : (xrSharpmix > 1.0f ? 1.0f : xrSharpmix);
    g_liveControls.xrReuseLastFrame = xrReuseLastFrame != 0 ? 1 : 0;
    g_liveControls.xrPairLock = xrPairLock != 0 ? 1 : 0;
    g_liveControls.xrRenderPoseSubmit = xrRenderPoseSubmit != 0 ? 1 : 0;
    g_liveControls.xrPoseLag = xrPoseLag;
    g_liveControls.xrRuntime = ClampRuntimeMode(xrRuntime);
    g_liveControls.xrDepthSubmit = xrDepthSubmit != 0 ? 1 : 0;
    // xrMovementSource is the authoritative locomotion mode (0..3); legacy
    // xrMovementControl mirrors it for old configs (0 = Game, anything else
    // means VR-driven so map to legacy 1).
    if (xrMovementSource < 0 || xrMovementSource > 3) xrMovementSource = xrMovementControl != 0 ? 1 : 0;
    g_liveControls.xrMovementSource = xrMovementSource;
    g_liveControls.xrLaserDotMode = (xrLaserDotMode < 0) ? 0 : (xrLaserDotMode > 2 ? 2 : xrLaserDotMode);
    g_liveControls.xrMovementControl = xrMovementSource != 0 ? 1 : 0;
    g_liveControls.xrPhysicalBodyRotation = xrPhysicalBodyRotation != 0 ? 1 : 0;
    g_liveControls.xrCutsceneSuspendTier =
        (xrCutsceneSuspendTier < -1) ? -1 : (xrCutsceneSuspendTier > 4 ? 4 : xrCutsceneSuspendTier);
    g_liveControls.xrDisableMouseY = xrDisableMouseY != 0 ? 1 : 0;
    g_liveControls.xrXInputHook = xrXInputHook != 0 ? 1 : 0;
    g_liveControls.xrSnapTurn = xrSnapTurn != 0 ? 1 : 0;
    g_liveControls.xrSnapTurnAngleDeg = xrSnapTurnAngleDeg > 0.0f ? xrSnapTurnAngleDeg : 30.0f;
    g_liveControls.xrXInputInstall = xrXInputInstall != 0 ? 1 : 0;
    g_liveControls.xrInputActions = xrInputActions != 0 ? 1 : 0;
    g_liveControls.xrMonoXQueueWait = xrMonoXQueueWait != 0 ? 1 : 0;
    g_liveControls.xrSnapTurnPulseMs = xrSnapTurnPulseMs > 0 ? xrSnapTurnPulseMs : 30;
    g_liveControls.xrMonoDepthCapture = xrMonoDepthCapture != 0 ? 1 : 0;
    g_liveControls.xrSnapTurnYawIndex = (xrSnapTurnYawIndex >= 0 && xrSnapTurnYawIndex <= 3) ? xrSnapTurnYawIndex : 1;
    g_liveControls.xrImmersiveHolsters = xrImmersiveHolsters != 0 ? 1 : 0;
    OpenXRManager::Get().SetImmersiveHolsters(g_liveControls.xrImmersiveHolsters);
    // Clamped wider than the slider (+/-0.50) but still bounded: the value can arrive from a
    // hand-edited ini, and a metre of head offset is not a setting, it is a typo.
    auto clampVehOff = [](float v) { return (v < -1.0f) ? -1.0f : (v > 1.0f ? 1.0f : v); };
    g_liveControls.xrVehHeadOffsetX = clampVehOff(xrVehHeadOffsetX);
    g_liveControls.xrVehHeadOffsetY = clampVehOff(xrVehHeadOffsetY);
    g_liveControls.xrVehHeadOffsetZ = clampVehOff(xrVehHeadOffsetZ);
    g_liveControls.xrWheelGrab = xrWheelGrab != 0 ? 1 : 0;
    // Clamped, not trusted: these come from a text file. Below ~8 cm the grab is unreachable for a
    // hand you cannot see; above 60 cm every grip in a car is a grab.
    g_liveControls.xrWheelRadius = (xrWheelRadius < 0.08f) ? 0.08f
                                 : (xrWheelRadius > 0.60f ? 0.60f : xrWheelRadius);
    g_liveControls.xrWheelSteerMaxDeg = (xrWheelSteerMaxDeg < 30.0f) ? 30.0f
                                      : (xrWheelSteerMaxDeg > 120.0f ? 120.0f : xrWheelSteerMaxDeg);
    // The deadzone is capped well under the smallest full-lock angle (30 deg): one that met or passed
    // it would leave no range at all between "dead" and "full lock".
    g_liveControls.xrWheelSteerDeadDeg = (xrWheelSteerDeadDeg < 0.0f) ? 0.0f
                                       : (xrWheelSteerDeadDeg > 20.0f ? 20.0f : xrWheelSteerDeadDeg);
    g_liveControls.xrWheelHorn = xrWheelHorn != 0 ? 1 : 0;
    // Below 4 cm the hub is unhittable with a hand you cannot see; above 30 cm it swallows the rim,
    // and every reach for the wheel would honk.
    g_liveControls.xrWheelHornRadius = (xrWheelHornRadius < 0.04f) ? 0.04f
                                     : (xrWheelHornRadius > 0.30f ? 0.30f : xrWheelHornRadius);
    g_liveControls.xrVehicleGunTrigger = xrVehicleGunTrigger != 0 ? 1 : 0;
    g_liveControls.xrVehicleThrottleTrim = (xrVehicleThrottleTrim < 0.05f) ? 0.05f
                                         : (xrVehicleThrottleTrim > 3.0f ? 3.0f : xrVehicleThrottleTrim);
    SetHmdTrackingSmooth(xrHmdSmooth);
    CyberpunkVR_HandLerpSpeed = (xrHandLerp < 0.0f) ? 0.0f : ((xrHandLerp > 30.0f) ? 30.0f : xrHandLerp);
    // Clamped rather than trusted: at 0 the hold can never be offered and at a third of a metre it is
    // offered for a hand nowhere near the weapon, and both read as "the feature is broken".
    CyberpunkVR_TwoHandRadius = (xrTwoHandRadius < 0.02f) ? 0.02f
                              : ((xrTwoHandRadius > 0.30f) ? 0.30f : xrTwoHandRadius);
    // Clamped to one screen either way, and to a scale that leaves something on screen. A piece dragged
    // ten thousand pixels off or scaled to nothing looks exactly like a broken mod, and an ini edited by
    // hand is the likeliest way to get there. The scale range matches what the editor's wheel allows,
    // so a saved layout and a live one cannot disagree.
    for (int i = 0; i < 7; ++i) {
        float sx = xrScannerSlots[i * 3 + 0];
        float sy = xrScannerSlots[i * 3 + 1];
        float ss = xrScannerSlots[i * 3 + 2];
        CyberpunkVR_ScannerSlots[i * 3 + 0] = (sx < -1920.0f) ? -1920.0f : ((sx > 1920.0f) ? 1920.0f : sx);
        CyberpunkVR_ScannerSlots[i * 3 + 1] = (sy < -1080.0f) ? -1080.0f : ((sy > 1080.0f) ? 1080.0f : sy);
        CyberpunkVR_ScannerSlots[i * 3 + 2] = (ss < 0.10f) ? 0.10f : ((ss > 5.0f) ? 5.0f : ss);
    }
    CyberpunkVR_HandRelToFilteredHead = xrHandRelFiltered;
    CyberpunkVR_HandLocatePerFrame = xrHandPerFrame;
    // NEGATIVE IS ALLOWED, down to one period BEHIND the frame's target, and that is not a mistake.
    // We locate the hands at a FUTURE instant, so the runtime extrapolates -- and extrapolation noise
    // grows with the distance predicted. The spec guarantees at least 50 ms of retained history, so
    // aiming closer to now, or slightly behind it, is an accurate measurement rather than a guess. It
    // trades latency for steadiness, which is the trade this shake is about.
    WriteVrikSettingsFile(); // keep the CET-facing bridge file in sync with vrport.ini
    if (prevXrRecenter == 0 && xrRecenter != 0) {
        OpenXRManager::Get().RequestRecenter();
        Log("OpenXR recenter requested.\n");
    }

    if (prevXrMonoSubmit != xrMonoSubmit) {
        OpenXRManager::Get().SetMonoSubmitEnabled(xrMonoSubmit != 0);
        Log("OpenXR mono submit %s.\n", xrMonoSubmit != 0 ? "enabled" : "disabled");
    }

    // A plain global rather than a live-controls field: producer and consumer are both inside
    // this plugin and there is no CET boundary to cross, so OpenXRManager reads it directly.
    if (CyberpunkVR_ThreadedMonoSubmit != xrThreadedSubmit) {
        CyberpunkVR_ThreadedMonoSubmit = xrThreadedSubmit;
        const char* how = (xrThreadedSubmit == 0) ? "INLINE pump (forced)"
                        : (xrThreadedSubmit > 0)  ? "submit THREAD (forced)"
                                                  : "auto";
        Log("OpenXR submit owner: xr_threaded_submit=%d -> %s; resolved now = %s\n",
            xrThreadedSubmit, how,
            OpenXRManager::Get().UseThreadedSubmit() ? "submit thread" : "inline pump");
    }

    if (CyberpunkVR_CascadeSaveMain != xrCascadeSaveMain) {
        CyberpunkVR_CascadeSaveMain = xrCascadeSaveMain;
        Log("Cascade shadows: xr_cascade_save_main=%d -> MAIN %s clear the shared atlas.\n",
            xrCascadeSaveMain,
            xrCascadeSaveMain ? "does NOT" : "does");
    }


    if (changed && g_verboseLog) {
        Log("Live controls updated: xr_head_offset=(%.4f,%.4f,%.4f) xr_recenter=%d xr_mono_submit=%d xr_force_fov=%.3f xr_menu_rect=%d xr_menu_fov=%.3f xr_3dof_movement=%d xr_motion_predict_ms=%.2f xr_stereo_scale=%.3f xr_render_pose_submit=%d xr_runtime=%d\n",
            g_liveControls.xrHeadOffsetX, g_liveControls.xrHeadOffsetY, g_liveControls.xrHeadOffsetZ, g_liveControls.xrRecenter, g_liveControls.xrMonoSubmit, g_liveControls.xrForceFov, g_liveControls.xrMenuRect, g_liveControls.xrMenuFov, g_liveControls.xr3DofMovement, g_liveControls.xrMotionPredictMs, g_liveControls.xrStereoScale, g_liveControls.xrRenderPoseSubmit, g_liveControls.xrRuntime);
        if (g_liveControls.xrRuntime != 0) {
            Log("Live controls: xr_runtime=%d will apply on next startup before OpenXR init.\n", g_liveControls.xrRuntime);
        }
    }
}

LiveControlsUiState MakeLiveControlsUiState() {
    LiveControlsUiState state{};
    state.xrHeadOffsetX = g_liveControls.xrHeadOffsetX;
    state.xrHeadOffsetY = g_liveControls.xrHeadOffsetY;
    state.xrHeadOffsetZ = g_liveControls.xrHeadOffsetZ;
    state.xrRecenter = g_liveControls.xrRecenter;
    state.xrMonoSubmit = g_liveControls.xrMonoSubmit;
    state.xrForceFov = g_liveControls.xrForceFov;
    state.xrMenuRect = g_liveControls.xrMenuRect;
    state.xrMenuFov = g_liveControls.xrMenuFov;
    state.xrMenuFollowDeg = g_liveControls.xrMenuFollowDeg;
    state.xr3DofMovement = g_liveControls.xr3DofMovement;
    state.xrFirstLaunch = g_liveControls.xrFirstLaunch;
    state.xrMotionPredictMs = g_liveControls.xrMotionPredictMs;
    state.xrStereoScale = g_liveControls.xrStereoScale;
    state.xrWorldScale = g_liveControls.xrWorldScale;
    state.xrIpdScale = g_liveControls.xrIpdScale;
    state.xrSharpness = g_liveControls.xrSharpness;
    state.xrSharpmix = g_liveControls.xrSharpmix;
    state.xrReuseLastFrame = g_liveControls.xrReuseLastFrame;
    state.xrPairLock = g_liveControls.xrPairLock;
    state.xrRenderPoseSubmit = g_liveControls.xrRenderPoseSubmit;
    state.xrPoseLag = g_liveControls.xrPoseLag;
    state.xrRuntime = g_liveControls.xrRuntime;
    state.xrMovementControl = g_liveControls.xrMovementControl;
    state.xrDisableMouseY = g_liveControls.xrDisableMouseY;
    state.xrXInputHook = g_liveControls.xrXInputHook;
    state.xrSnapTurn = g_liveControls.xrSnapTurn;
    state.xrSnapTurnAngleDeg = g_liveControls.xrSnapTurnAngleDeg;
    state.xrMovementSource = g_liveControls.xrMovementSource;
    state.xrLaserDotMode = g_liveControls.xrLaserDotMode;
    state.xrPhysicalBodyRotation = g_liveControls.xrPhysicalBodyRotation;
    state.xrCutsceneSuspendTier = g_liveControls.xrCutsceneSuspendTier;
    state.xrXInputInstall = g_liveControls.xrXInputInstall;
    state.xrInputActions = g_liveControls.xrInputActions;
    state.xrMonoXQueueWait = g_liveControls.xrMonoXQueueWait;
    state.xrMonoDepthCapture = g_liveControls.xrMonoDepthCapture;
    state.xrSnapTurnPulseMs = g_liveControls.xrSnapTurnPulseMs;
    state.xrImmersiveHolsters = g_liveControls.xrImmersiveHolsters;
    state.xrVehHeadOffsetX = g_liveControls.xrVehHeadOffsetX;
    state.xrVehHeadOffsetY = g_liveControls.xrVehHeadOffsetY;
    state.xrVehHeadOffsetZ = g_liveControls.xrVehHeadOffsetZ;
    state.xrWheelGrab = g_liveControls.xrWheelGrab;
    state.xrWheelRadius = g_liveControls.xrWheelRadius;
    state.xrWheelSteerMaxDeg = g_liveControls.xrWheelSteerMaxDeg;
    state.xrWheelSteerDeadDeg = g_liveControls.xrWheelSteerDeadDeg;
    state.xrWheelHorn = g_liveControls.xrWheelHorn;
    state.xrWheelHornRadius = g_liveControls.xrWheelHornRadius;
    state.xrVehicleGunTrigger = g_liveControls.xrVehicleGunTrigger;
    state.xrVehicleThrottleTrim = g_liveControls.xrVehicleThrottleTrim;
    return state;
}

void PersistLiveControlsUiState(const LiveControlsUiState& state) {
    InitRuntimePaths();
    FILE* file = _fsopen(g_liveControlPath, "w", _SH_DENYNO);
    if (!file) return;

    fprintf(file, "xr_head_offset_x=%.4f\n", state.xrHeadOffsetX);
    fprintf(file, "xr_head_offset_y=%.4f\n", state.xrHeadOffsetY);
    fprintf(file, "xr_head_offset_z=%.4f\n", state.xrHeadOffsetZ);
    fprintf(file, "xr_recenter=0\n");
    fprintf(file, "xr_mono_submit=%d\n", state.xrMonoSubmit != 0 ? 1 : 0);
    // Written from the global because it is not a UI control. It MUST be here all the same:
    // this function rewrites the whole file, and a key left out is a key deleted -- the
    // mistake that once ate xr_hand_predict.
    fprintf(file, "xr_threaded_submit=%d\n", CyberpunkVR_ThreadedMonoSubmit);
    fprintf(file, "xr_cascade_save_main=%d\n", CyberpunkVR_CascadeSaveMain != 0 ? 1 : 0);
    fprintf(file, "xr_force_fov=%.3f\n", state.xrForceFov);
    fprintf(file, "xr_menu_rect=%d\n", state.xrMenuRect != 0 ? 1 : 0);
    fprintf(file, "xr_menu_fov=%.3f\n", state.xrMenuFov);
    fprintf(file, "xr_menu_follow_deg=%.3f\n", state.xrMenuFollowDeg >= 5.0f ? state.xrMenuFollowDeg : 60.0f);
    fprintf(file, "xr_3dof_movement=%d\n", state.xr3DofMovement != 0 ? 1 : 0);
    // Not a control, but it MUST be written back: this function rewrites the whole file, so
    // leaving the key out would drop it, and the next launch would read the default 1 and
    // re-install the shipped settings over whatever the player had just changed.
    fprintf(file, "first_launch=%d\n", state.xrFirstLaunch != 0 ? 1 : 0);
    fprintf(file, "xr_motion_predict_ms=%.2f\n", state.xrMotionPredictMs);
    fprintf(file, "xr_stereo_scale=%.3f\n", state.xrStereoScale);
    fprintf(file, "xr_world_scale=%.3f\n", state.xrWorldScale);
    fprintf(file, "xr_ipd_scale=%.3f\n", state.xrIpdScale);
    fprintf(file, "xr_sharpness=%.3f\n", state.xrSharpness);
    fprintf(file, "xr_sharpmix=%.3f\n", state.xrSharpmix);
    fprintf(file, "xr_reuse_last_frame=%d\n", state.xrReuseLastFrame != 0 ? 1 : 0);
    fprintf(file, "xr_pair_lock=%d\n", state.xrPairLock != 0 ? 1 : 0);
    fprintf(file, "xr_render_pose_submit=%d\n", state.xrRenderPoseSubmit != 0 ? 1 : 0);
    fprintf(file, "xr_pose_lag=%d\n", state.xrPoseLag);
    fprintf(file, "xr_runtime=%d\n", ClampRuntimeMode(state.xrRuntime));
    fprintf(file, "xr_hmd_smooth=%.3f\n", GetHmdTrackingSmooth());
    // The hand filter's speed, in UEVR's units (follow per second, multiplied by dt at the point of
    // use). Replaces xr_hand_smooth, which was a fraction per FRAME and therefore frame-rate dependent.
    fprintf(file, "xr_hand_lerp_speed=%.3f\n", CyberpunkVR_HandLerpSpeed);
    // How near the support point the off hand has to come before the two-handed hold is offered, in
    // metres. Clamped to [0.02, 0.30] on read.
    fprintf(file, "xr_two_hand_radius=%.3f\n", CyberpunkVR_TwoHandRadius);
    // The scanner's four movable pieces, x,y,scale each, in 1920x1080 design pixels. Normally written by
    // the in-game editor rather than by hand -- hold RIGHT SHIFT while scanning.
    {
        static const char* kScannerKeys[7] = { "xr_scanner_frame", "xr_scanner_details",
                                              "xr_scanner_hacks", "xr_scanner_hint",
                                              "xr_scanner_memory", "xr_scanner_scripts",
                                              "xr_scanner_desc" };
        for (int i = 0; i < 7; ++i) {
            fprintf(file, "%s=%.1f,%.1f,%.3f\n", kScannerKeys[i],
                    CyberpunkVR_ScannerSlots[i * 3 + 0],
                    CyberpunkVR_ScannerSlots[i * 3 + 1],
                    CyberpunkVR_ScannerSlots[i * 3 + 2]);
        }
    }
    fprintf(file, "xr_movement_control=%d\n", state.xrMovementControl != 0 ? 1 : 0);
    fprintf(file, "xr_disable_mouse_y=%d\n", state.xrDisableMouseY != 0 ? 1 : 0);
    fprintf(file, "xr_xinput_hook=%d\n", state.xrXInputHook != 0 ? 1 : 0);
    fprintf(file, "xr_snap_turn=%d\n", state.xrSnapTurn != 0 ? 1 : 0);
    fprintf(file, "xr_snap_turn_angle_deg=%.2f\n", state.xrSnapTurnAngleDeg > 0.0f ? state.xrSnapTurnAngleDeg : 30.0f);
    fprintf(file, "xr_movement_source=%d\n", state.xrMovementSource < 0 ? 0 : (state.xrMovementSource > 3 ? 3 : state.xrMovementSource));
    fprintf(file, "xr_laser_dot_mode=%d\n", state.xrLaserDotMode < 0 ? 0 : (state.xrLaserDotMode > 2 ? 2 : state.xrLaserDotMode));
    fprintf(file, "xr_physical_body_rotation=%d\n", state.xrPhysicalBodyRotation != 0 ? 1 : 0);
    fprintf(file, "xr_cutscene_suspend_tier=%d\n",
            state.xrCutsceneSuspendTier < -1 ? -1 : (state.xrCutsceneSuspendTier > 4 ? 4 : state.xrCutsceneSuspendTier));
    fprintf(file, "xr_xinput_install=%d\n", state.xrXInputInstall != 0 ? 1 : 0);
    fprintf(file, "xr_input_actions=%d\n", state.xrInputActions != 0 ? 1 : 0);
    fprintf(file, "xr_mono_xqueue_wait=%d\n", state.xrMonoXQueueWait != 0 ? 1 : 0);
    fprintf(file, "xr_mono_depth_capture=%d\n", state.xrMonoDepthCapture != 0 ? 1 : 0);
    fprintf(file, "xr_snap_turn_pulse_ms=%d\n", state.xrSnapTurnPulseMs > 0 ? state.xrSnapTurnPulseMs : 30);
    fprintf(file, "xr_immersive_holsters=%d\n", state.xrImmersiveHolsters != 0 ? 1 : 0);
    fprintf(file, "xr_veh_head_offset_x=%.4f\n", state.xrVehHeadOffsetX);
    fprintf(file, "xr_veh_head_offset_y=%.4f\n", state.xrVehHeadOffsetY);
    fprintf(file, "xr_veh_head_offset_z=%.4f\n", state.xrVehHeadOffsetZ);
    fprintf(file, "xr_wheel_grab=%d\n", state.xrWheelGrab != 0 ? 1 : 0);
    fprintf(file, "xr_wheel_radius=%.3f\n", state.xrWheelRadius > 0.0f ? state.xrWheelRadius : 0.28f);
    fprintf(file, "xr_wheel_steer_max_deg=%.1f\n", state.xrWheelSteerMaxDeg > 0.0f ? state.xrWheelSteerMaxDeg : 90.0f);
    fprintf(file, "xr_wheel_steer_dead_deg=%.1f\n", state.xrWheelSteerDeadDeg >= 0.0f ? state.xrWheelSteerDeadDeg : 1.5f);
    fprintf(file, "xr_wheel_horn=%d\n", state.xrWheelHorn != 0 ? 1 : 0);
    fprintf(file, "xr_wheel_horn_radius=%.3f\n", state.xrWheelHornRadius > 0.0f ? state.xrWheelHornRadius : 0.12f);
    fprintf(file, "xr_vehicle_gun_trigger=%d\n", state.xrVehicleGunTrigger != 0 ? 1 : 0);
    fprintf(file, "xr_vehicle_throttle_trim=%.2f\n", state.xrVehicleThrottleTrim > 0.0f ? state.xrVehicleThrottleTrim : 0.5f);
    fclose(file);

    WIN32_FILE_ATTRIBUTE_DATA fileData;
    if (GetFileAttributesExA(g_liveControlPath, GetFileExInfoStandard, &fileData)) {
        g_lastLiveControlWrite = fileData.ftLastWriteTime;
    }
}

extern "C" void GetLiveControlsUiState(LiveControlsUiState* outState) {
    if (!outState) return;
    *outState = MakeLiveControlsUiState();
}

extern "C" void RequestLiveControlsRecenter() {
    g_liveControls.xrRecenter = 0;
    OpenXRManager::Get().RequestRecenter();
    Log("ImGui: OpenXR recenter requested.\n");
}

extern "C" void SetLiveControlsUiState(const LiveControlsUiState* state, int persistToFile) {
    if (!state) return;

    const int prevMono = g_liveControls.xrMonoSubmit;

    g_liveControls.xrHeadOffsetX = state->xrHeadOffsetX;
    g_liveControls.xrHeadOffsetY = state->xrHeadOffsetY;
    g_liveControls.xrHeadOffsetZ = state->xrHeadOffsetZ;
    g_liveControls.xrRecenter = 0;
    g_liveControls.xrMonoSubmit = state->xrMonoSubmit != 0 ? 1 : 0;
    g_liveControls.xrForceFov = state->xrForceFov > 0.0f ? state->xrForceFov : 0.0f;
    g_liveControls.xrMenuRect = state->xrMenuRect != 0 ? 1 : 0;
    g_liveControls.xrMenuFov = state->xrMenuFov > 1.0f ? state->xrMenuFov : 65.0f;
    g_liveControls.xrMenuFollowDeg = (state->xrMenuFollowDeg >= 5.0f && state->xrMenuFollowDeg <= 90.0f) ? state->xrMenuFollowDeg : 60.0f;
    g_liveControls.xr3DofMovement = state->xr3DofMovement != 0 ? 1 : 0;
    g_liveControls.xrFirstLaunch = state->xrFirstLaunch != 0 ? 1 : 0;
    g_liveControls.xrMotionPredictMs = state->xrMotionPredictMs >= 0.0f ? state->xrMotionPredictMs : 0.0f;
    g_liveControls.xrStereoScale = state->xrStereoScale < 0.0f ? 0.0f : (state->xrStereoScale > 10.0f ? 10.0f : state->xrStereoScale);
    g_liveControls.xrWorldScale = state->xrWorldScale < 0.05f ? 0.05f : (state->xrWorldScale > 20.0f ? 20.0f : state->xrWorldScale);
    g_liveControls.xrIpdScale = state->xrIpdScale < 0.0f ? 0.0f : (state->xrIpdScale > 5.0f ? 5.0f : state->xrIpdScale);
    g_liveControls.xrSharpness = state->xrSharpness < 0.0f ? 0.0f : (state->xrSharpness > 1.0f ? 1.0f : state->xrSharpness);
    g_liveControls.xrSharpmix = state->xrSharpmix < 0.0f ? 0.0f : (state->xrSharpmix > 1.0f ? 1.0f : state->xrSharpmix);
    g_liveControls.xrReuseLastFrame = state->xrReuseLastFrame != 0 ? 1 : 0;
    g_liveControls.xrPairLock = state->xrPairLock != 0 ? 1 : 0;
    g_liveControls.xrRenderPoseSubmit = state->xrRenderPoseSubmit != 0 ? 1 : 0;
    g_liveControls.xrPoseLag = state->xrPoseLag;
    g_liveControls.xrRuntime = ClampRuntimeMode(state->xrRuntime);
    {
        int src = state->xrMovementSource;
        if (src < 0 || src > 3) src = state->xrMovementControl != 0 ? 1 : 0;
        g_liveControls.xrMovementSource = src;
        g_liveControls.xrMovementControl = src != 0 ? 1 : 0;
    }
    g_liveControls.xrLaserDotMode =
        (state->xrLaserDotMode < 0) ? 0 : (state->xrLaserDotMode > 2 ? 2 : state->xrLaserDotMode);
    g_liveControls.xrPhysicalBodyRotation = state->xrPhysicalBodyRotation != 0 ? 1 : 0;
    g_liveControls.xrCutsceneSuspendTier =
        (state->xrCutsceneSuspendTier < -1) ? -1
                                           : (state->xrCutsceneSuspendTier > 4 ? 4 : state->xrCutsceneSuspendTier);
    g_liveControls.xrDisableMouseY = state->xrDisableMouseY != 0 ? 1 : 0;
    g_liveControls.xrXInputHook = state->xrXInputHook != 0 ? 1 : 0;
    g_liveControls.xrSnapTurn = state->xrSnapTurn != 0 ? 1 : 0;
    g_liveControls.xrSnapTurnAngleDeg = state->xrSnapTurnAngleDeg > 0.0f ? state->xrSnapTurnAngleDeg : 30.0f;
    g_liveControls.xrXInputInstall = state->xrXInputInstall != 0 ? 1 : 0;
    g_liveControls.xrInputActions = state->xrInputActions != 0 ? 1 : 0;
    g_liveControls.xrMonoXQueueWait = state->xrMonoXQueueWait != 0 ? 1 : 0;
    g_liveControls.xrMonoDepthCapture = state->xrMonoDepthCapture != 0 ? 1 : 0;
    g_liveControls.xrSnapTurnPulseMs = state->xrSnapTurnPulseMs > 0 ? state->xrSnapTurnPulseMs : 30;
    g_liveControls.xrImmersiveHolsters = state->xrImmersiveHolsters != 0 ? 1 : 0;
    OpenXRManager::Get().SetImmersiveHolsters(g_liveControls.xrImmersiveHolsters);
    {   // In-vehicle head offset: same bound as the ini path, for the same reason.
        auto cl = [](float v) { return (v < -1.0f) ? -1.0f : (v > 1.0f ? 1.0f : v); };
        g_liveControls.xrVehHeadOffsetX = cl(state->xrVehHeadOffsetX);
        g_liveControls.xrVehHeadOffsetY = cl(state->xrVehHeadOffsetY);
        g_liveControls.xrVehHeadOffsetZ = cl(state->xrVehHeadOffsetZ);
    }
    // DRIVING. Same clamps as the ini path -- the overlay sliders already bound these, but the two
    // entry points must not be able to disagree about what a valid value is.
    g_liveControls.xrWheelGrab = state->xrWheelGrab != 0 ? 1 : 0;
    g_liveControls.xrWheelHorn = state->xrWheelHorn != 0 ? 1 : 0;
    g_liveControls.xrVehicleGunTrigger = state->xrVehicleGunTrigger != 0 ? 1 : 0;
    {
        const float r = state->xrWheelRadius;
        g_liveControls.xrWheelRadius = (r < 0.08f) ? 0.08f : (r > 0.60f ? 0.60f : r);
        const float m = state->xrWheelSteerMaxDeg;
        g_liveControls.xrWheelSteerMaxDeg = (m < 30.0f) ? 30.0f : (m > 120.0f ? 120.0f : m);
        const float d = state->xrWheelSteerDeadDeg;
        g_liveControls.xrWheelSteerDeadDeg = (d < 0.0f) ? 0.0f : (d > 20.0f ? 20.0f : d);
        const float hr = state->xrWheelHornRadius;
        g_liveControls.xrWheelHornRadius = (hr < 0.04f) ? 0.04f : (hr > 0.30f ? 0.30f : hr);
        const float tt = state->xrVehicleThrottleTrim;
        g_liveControls.xrVehicleThrottleTrim = (tt < 0.05f) ? 0.05f : (tt > 3.0f ? 3.0f : tt);
    }
    WriteVrikSettingsFile(); // publish mouse-Y flag for the CET VRIK mod

    if (prevMono != g_liveControls.xrMonoSubmit) {
        OpenXRManager::Get().SetMonoSubmitEnabled(g_liveControls.xrMonoSubmit != 0);
        Log("ImGui: OpenXR mono submit %s.\n", g_liveControls.xrMonoSubmit != 0 ? "enabled" : "disabled");
    }
    if (state->xrRecenter != 0) {
        RequestLiveControlsRecenter();
    }

    if (persistToFile != 0) {
        PersistLiveControlsUiState(MakeLiveControlsUiState());
    }
}

void PollHotkeys() {
    static bool f7WasDown = false;
    static bool f8WasDown = false;

    const bool f7Down = (GetAsyncKeyState(VK_F7) & 0x8000) != 0;
    const bool f8Down = (GetAsyncKeyState(VK_F8) & 0x8000) != 0;

    if (f7Down && !f7WasDown) {
        OpenXRManager::Get().RequestRecenter();
        Log("Hotkey F7: OpenXR recenter requested.\n");
    }

    if (f8Down && !f8WasDown) {
        g_liveControls.xrMenuRect = g_liveControls.xrMenuRect != 0 ? 0 : 1;
        Log("Hotkey F8: xr_menu_rect=%d (%s).\n",
            g_liveControls.xrMenuRect,
            g_liveControls.xrMenuRect != 0 ? "small HMD rectangle" : "full HMD rectangle");
    }

    f7WasDown = f7Down;
    f8WasDown = f8Down;
}
