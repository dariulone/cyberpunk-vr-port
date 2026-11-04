# Full-body tracking in the game

[![Full-body tracking in Cyberpunk 2077 VR: the calibration and walking around](https://img.youtube.com/vi/tEELPU7dMhs/maxresdefault.jpg)](https://www.youtube.com/watch?v=tEELPU7dMhs)

Three Vive Trackers (waist, left foot, right foot) drive V's legs, hips and spine, and V's arms reach the controllers.
Without trackers, or before a calibration, nothing changes: the port runs as it does without this folder.

The full-body IK is the [wrpuppets-fbt-ik](https://github.com/Syncher-source/wrpuppets-fbt-ik) library (MIT,
engine-independent, submodule `externals/wrpuppets-fbt-ik`). `src/Fbt/Game` (and `include/Fbt/Game`) is the game
side. The [cyberpunk-fbt-lab](https://github.com/Syncher-source/cyberpunk-fbt-lab) runs the same IK on V's skeleton
outside the game, records and replays sessions: FBT bug reports go there, with a recording.

## Setup

1. In SteamVR, *Manage Vive Trackers*: give each tracker its role, **Waist**, **Left Foot** and **Right Foot**. The
   trackers are found by these roles (OpenXR `XR_HTCX_vive_tracker_interaction`), not by their order.
2. In the settings, Full-body tracking: *Enable Vive trackers* (it also turns on the legs, spine and arms switches),
   then restart the game.
3. Load a save, then *Calibrate full-body tracking* in the settings (in game only, not in the lobby), at every session: belts and
   straps move.
   1. look ahead at the horizon line, both triggers: eye height and heading;
   2. arms along the body, look down, both triggers: scale and the trackers' offsets to V's hips and feet;
   3. the T-pose on the cross drawn in front of you (centred on your feet): head on its top, arms along its arms,
      until the 4 s countdown ends: the arms' length.

Switches in `fbt_settings.ini` (and the settings): V's legs follow the trackers; V's spine and head follow the headset;
V's arms on the full-body IK.

## How it sits in the port

The view always follows the headset 1:1, and V's body is placed under it from the trackers: the goals are measured
from the head the view uses (the filtered head the hands are located with), in this frame's view, and the camera
stands at V's eyes as calibrated. The solve runs inside VRIK's own pass, so VRIK's suspends (cutscenes, vehicles,
ladders, swimming, takedowns, finishers, body carry) cover it too.

| File | What |
|---|---|
| `Trackers.cpp` | the Vive trackers through OpenXR (waist, feet) and the controllers' grip poses, located with the hands, head-relative and in STAGE |
| `Body.cpp` | V's hips, legs and spine from the trackers (the IK on the game's bone buffer), placed under this frame's view; the arms; the heels and toes held under the solved feet; `BodyActive()`; `fbt_settings.ini` |
| `Calibration.cpp` | the three-step calibration on the library's `Calibrator`; `fbt_calibration.ini` |
| `Helpers.cpp` | the calibration's graphics in the view (horizon line, cross, lights, lines, arrow, the step's text) |
| `GameFont.cpp` | the step texts in the game's own font, read from its archives for the game's language |
| `Menu.cpp` | the settings section (the trackers' switch, the calibration button, the body switches) |
| `Natives.cpp` | the script natives for the CET menu (`VRFbtCalibrating`, `VRFbtCalibrationStep`, `VRFbtSetCalibrationText`) |

## Where the port calls in

One line each, except where noted.

| Port file | Call |
|---|---|
| `Runtimes/OpenXRManager.cpp` | `trackers::ChooseExtensions`, `OnInstanceCreated`, `CreateActions`, `OnSessionCreated`, `OnSessionDestroying` |
| `Runtimes/OpenXRFrameLoop.cpp` | `trackers::Locate` (with the hands), `fbt::game::NoteTrigger` |
| `Hooks/AnimPose.cpp` | `fbt::game::Calibrating()` -> `VRIK_HoldReferencePose` (V in its reference pose while calibrating); `fbt::game::SolveLowerBody` on foot (not on a ladder or swimming), with this frame's view; `fbt::game::SolveArm` in place of each `VRIK_SolveArm` (3 places), on the port's final hand target; while `BodyActive()`, `g_VRCamModelPos` is the view's own model point (the camera stands at V's eyes: the menu laser, the fingertip and the hand stops convert through it) |
| `Hooks/PatchCamera.cpp` | `fbt::game::CameraEyeHeight` -> the view's height at V's eyes |
| `Hooks/RoomscaleMove.cpp` | `CyberpunkVR_HeadLeadsCapsule` only while not `BodyActive()` (the trackers place the body); room-scale movement paused while `Calibrating()` |
| `Hooks/BodyYawFollow.cpp` | paused while `Calibrating()` |
| `Anim/CharacterRig.cpp` | `CyberpunkVR_ClipBodyMoves` only while not `BodyActive()` (the trackers move the body through slides, falls and jumps). Also, for now: V's reference pose and rig file for the calibration (`VRIK_ReadFbtReference`, `VRIK_HoldReferencePose`, `VRIK_FbtRigPath`, the rig dump) and `VRIK_MarkLegOwned`, so the model cache replays our legs |
| `Natives/VrMenu.cpp` | the buttons kept from the game while `Calibrating()` (each step is a pull of both triggers) |
| `Overlay/OverlayPanels.cpp` | `fbt::game::DrawSettingsSection(state)` |
| `Overlay/ImGuiOverlay.cpp` | `DrawFbtCalibrationHelpers()`, `LoadCyberpunkFont` |
| `Natives/Registration.cpp` | `fbt::game::RegisterNatives(rtti)` |
| `Core/LiveControlsPoll.cpp`, `Overlay/LiveControlsUi.hpp` | the `xr_vive_trackers` key and its switch in the panel's state |
| CET `CyberpunkVRPort_Menu` | `menu.lua`: the menu closes while calibrating and hands the step texts over translated; `lang/settings`: the texts in all 18 languages |
