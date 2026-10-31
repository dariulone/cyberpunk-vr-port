# OpenXR and PSVR2 haptics and controls

## Standard OpenXR feedback

Quest and other headsets use standard OpenXR output. PSVR2 can also use OpenXR if its runtime has working left/right haptic-output bindings; successful API calls alone do not prove physical vibration. F10 includes left/right test pulses and logs bound output paths once.

Gun feedback uses the actual firing hand and weapon kick strength. A genuinely two-handed single weapon adds a weaker support-hand pulse; dual wielding never duplicates a shot into the other hand, including when a stale two-hand flag remains. Physical melee swings add feedback through the CET weapon script and `SetVRHapticPulse`; these are swing cues, not proof of collision. Existing block/parry/belt/grenade/menu effects remain, with block/parry priority over swing and audio rumble. Expired events, mute and lost focus clear queued feedback.

Driving audio uses WASAPI loopback of the default playback endpoint, a 28–320 Hz road/engine envelope and transient emphasis. It runs on a worker, not the render thread, and stops on shutdown. It can include music and other applications; it is not isolated engine telemetry. Stale audio expires and stops on leaving the vehicle.

F10 Controls exposes master vibration, overall strength, OpenXR gun/swing/vehicle gain, driving-audio enable/strength, stereo/strongest/left/right channel selection and optional per-hand wheel/handlebar gating. With gating off, driving rumble continues with hands off. Values persist in `vrport.ini`.

Wheel grip is a per-hand toggle: click near the animated wheel/handlebar to latch, click again anywhere to release. Tracking loss, blocked controls or equipping the right-hand weapon release the relevant grab. Latched grips are withheld from holster/reload interactions. This is also the state used by the optional rumble grip gate.

## Optional PSVR2 Toolkit route

F10 offers **OpenXR**, **Prefer Toolkit (OpenXR fallback)** and **Auto** (default). Detection happens at session startup and requires a same-Windows-session `psvr2_toolkit_dsx_bridge_017.exe` owning a loopback UDP listener. A driver installation or a shared mapping alone is not detection. This is a process/listener check, not a driver-readiness handshake.

Start the bridge before launching for Toolkit output. Stop it before selecting OpenXR. Restart the game when changing output ownership: the external bridge has independent adaptive-trigger/audio writers, so hot switching would not reliably stop competing output. Other headsets always remain OpenXR. No driver, bridge process or OpenXR layer is installed or modified by this code.

Use the compatible [PSVR2Toolkit v0.3.2 package](https://github.com/satyaloka93/PSVR2Toolkit/releases/tag/cyberpunk-dsx-bridge-v0.3.2) and its setup instructions. Standard OpenXR needs neither Toolkit nor Steam DSX. Advanced adaptive triggers/audio require the companion driver/bridge/mod setup; Steam DSX is not required and should not run alongside the bridge. The compatible driver preserves raw trigger travel while trigger effects are active; v0.3.2 bundles the same driver as v0.3.1/v0.2.0, so those users do not need to replace it again.

The native shared-memory channel supplies heartbeats and per-hand swing/interaction/dual-shot pulses. Enhanced DualSense Support remains responsible for single-gun adaptive profiles; dual-wield vibration is routed from actual shot events rather than trigger-profile timers. External bridge improvements remain in the linked Toolkit release, not duplicated here.

F10 Toolkit audio enable/strength reads and atomically updates `bin/x64/plugins/cyber_engine_tweaks/mods/PSVR2 Haptics/settings.ini` (Version=1). Missing/invalid companion settings disable those controls with an explanation. Toolkit channel/grip controls are not offered because that bridge does not expose them. The bridge applies audio changes live; no F10 restart is needed for audio strength alone.

## PSVR2 controls

Touch Triangle and move the right stick for D-pad directions; turning is suppressed during the whole touch. Triangle click remains Y. The `SystemButton (Sense Create)` OpenXR action uses Create for tap=Start/Pause, hold >=500 ms=Back/Select. Triangle-touch+R3 provides the same fallback, while bare R3 remains crouch and L3+R3 remains the VR menu. Configure SteamVR's application binding if Create is not delivered; this is not SteamVR's reserved dashboard action.

The runtime system name gates PSVR2 behavior; other headsets retain their menu action. Quest thumb-rest support is a separate PR. FOV overrides, pacing, loading panels, aiming dot, weapon-switch/revolver fixes and separate bike/car camera offsets are not part of this PR.

## Verification

The combined behavior was tested on PSVR2 and Quest 3 in the fork's RC2 release. `tools/psvr2_input_tests` covers input/system-button timing, routing/missing-bridge fallback, pulse precedence, stale events/mute, firing-hand isolation, and actual production playback with an output fixture. The listener test needs the real bridge stopped; it refuses to alter a user's running bridge. Standalone upstream build/test results are recorded in the PR.
