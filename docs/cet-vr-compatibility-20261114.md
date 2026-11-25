# CET overlay and CyberFinger: investigation, 2026-11-14

Follow-up development: [general mod UI bridge and CET renderer adapter](mod-ui-vr.md).
That work adds a separate VR canvas and laser input; it does not establish the
cause of the watchdog hangs documented below. Installation and headset validation
are separate from the source/build checks.

## Confirmed evidence

- User reported a freeze after toggling CET; they are unsure whether the full
  character editor was still open. Latest crash report for that session:
  `Cyberpunk2077-20261114-231555-44184-54316` under the authorized REDengine
  ReportQueue. Reason: `Watchdog timeout! (120 seconds)`, exception `0x80000003`.
  This is a stalled game, not evidence of a GPU device-removal error.
- Both standalone `cyber_engine_tweaks.asi` and `cyberpunkvrport.dll` were loaded.
  The installed VR package is `0.2.3-cyberfinger-wrist-r4`.
- Bounded dump stack scans are saved under `build/cet-vr-diagnostics/`.
  The main thread was waiting in engine job-dispatch code; its stack contains
  candidate addresses in both Lua hosts. The scans are **not unwound call
  stacks** and do not establish a culpable module or prove deadlock.
- Character Preset Manager's archived activity log records editor opening at
  23:13:24, leaving/uninitializing at 23:13:36, CET opening at 23:13:44, and CET
  closing at 23:13:45. The watchdog report was created at 23:15:55. Thus the last
  logged full-editor session had closed before those final CET toggles.
- CET's binding was F8 + F9 (`0x77`, `0x78`); the port polls F8 independently in
  `src/Core/LiveControlsPoll.cpp`, and the game's input XML assigns F9 to quick
  load. This is a confirmed binding conflict, not a confirmed cause of the hang.
  The port's separate Lua overlay is Scroll Lock. Character Preset Manager's
  editor action was minus (`0xBD`).

## Low stance after character-editor use

The user subsequently reported bent legs and a viewpoint too low to use the
apartment mirror. The game had exited before live stance diagnostics could be
read. A second report, `Cyberpunk2077-20261114-232537-31904-50336`, also records
a 120-second watchdog timeout. It does not establish a cause for the stance.
The current CPM activity log records creator uninitialization at 23:21:35 and
the final CET close at 23:22:56; the report was written at 23:25:38.

The installed configuration still enables physical crouch with a 0.450 m
threshold; `wrist-r4` preserved all six INIs. The right stick's full backward
travel requests manual crouch. F7 captures a new position reference and resets
the tracked posture origin. Check manual crouch first, then recenter in the
normal playing posture; temporarily disabling physical crouch separates the
tracked request from manual/game stance. No stance setting or save was changed
by this investigation. Read the desired/applied/actual crouch diagnostics during
a reproduction before attributing this to CET or changing the input code.

Follow-up, 2026-11-15: the user confirms slow movement and inability to exit
crouch. No Cyberpunk process was present during the check, so no live stance
values were obtained. The installed physical-crouch Lua script matches the
repository SHA256; the current INI enables physical crouch at 0.350 m (changed
since the earlier read, not by this investigation). Source/mapping inspection
also confirms that the mod emits R3 for right-stick-back crouch, while the game
assigns R3 to crouch only under Alternative, and to melee under Legacy/Agile.
The later live check below separates native stance from physical crouch without
changing either the game settings or the save.

Live reproduction, PID 51236: twelve samples showed physical depth 0.0 m,
desired 0, applied 0, actual crouch 1, locomotion 1/detailed 3. The user tapped
keyboard C and reported normal standing/movement; eight follow-up samples
confirmed actual crouch 0, locomotion 0/detailed 1, still with no physical-crouch
request or effect. Thus the reproduced stance was native game crouch, not a
tracked-height request. The reason for entering that stance is still unknown.
Read-only samples are saved in `build/cet-vr-diagnostics/crouch-51236-before.json`
and `crouch-51236-after-c.json`.

Right-stick-back still fails according to the user. With their additional
permission, `AppData/Local/CD Projekt Red/Cyberpunk 2077/UserSettings.json` was
read: the controller scheme is already Alternative and keyboard C is the crouch
toggle. Do not prescribe a scheme change for this installation. Live settings
also confirm left-handed weapons, bindings mode 0, movement speed mode 0 and
full-input threshold 0.9. The OpenXR session is focused and the CyberFinger
profile and selected repository v2 binding are active. Two initial zero-axis
recordings missed the user's movements; a subsequent recording captured all
stick directions. The isolated, sustained full-back test measured right Y about
-0.8467 (minimum -0.8583). Fixed-speed movement first applied a 0.18 deadzone and
then required 0.90 travel, making the effective raw threshold about 0.918.
This prevents that physical stick from requesting crouch. The recording is
`build/cet-vr-diagnostics/crouch-51236-right-back-held.json`.

The `controls-r5` correction gives CyberFinger dash/crouch a separate raw-axis
threshold, default 0.80, leaving other controllers unchanged. Crouch still uses
the native R3 action and needs an in-game check after installation. Saved
Alternative settings are evidence of configuration, not proof of the runtime
input mapping. Inventory and pause now use explicit native menu commands, and
OS cyberware uses a game-thread command queue: the prior CF request could be
cleared by the legacy chord updater before the game consumed it. Map tab
navigation and the physical HUD chord are also corrected. All 8 CyberFinger and
21 overlay test suites pass; see the session todo for final installation status.

## Recommended bindings

Page Down: CET Overlay Key. Page Up: Character Preset Manager's character-editor
action (`preset_manager_open_editor_input`). No in-game assignment was found for
these keys in the inspected game mappings or port hotkey handlers; the startup
launcher uses them for slider adjustment, before CET is in play. Set through
CET's Bindings page and Save, then restart because the port caches CET's key at
startup. At the user's subsequent request, the local GOG `bindings.json` was
updated while Cyberpunk was closed and verified: `cet.overlay_key` is
`9570149208162304` (Page Down), and `preset_manager_open_editor_input` is
`9288674231451648` (Page Up). All other bindings were preserved. Original backup:
`build/cet-vr-diagnostics/bindings-pgdn-pgup-754dd6ceff054211a9fee496da274063/bindings.before.json`.

## Rendering paths

The inspected CET checkout is `C:/Users/bengu/src/CyberEngineTweaks`, revision
`v1.38.1-11-g20b42c6`; it was read only. Do not assume it matches the installed
ASI without verifying its build identity.

CET builds ImGui frames on the game thread (`D3D12::PrepareUpdate`), copies draw
lists, and renders them into the game's backbuffer in `D3D12::Update`. Its own
Win32 input handler owns the overlay input. Character Preset Manager registers
`onOverlayOpen`, `onOverlayClose`, and `onDraw`, and draws through that host.
It also has a native character-editor bridge; keep both parts together.

The VR port has another Lua host and another ImGui renderer. Its F10 panel has
an OpenXR quad and laser input, while its Lua-mod windows still draw through
the desktop path. `PassRawInput` explicitly hands CET's key through when those
windows are open; it uses a cached key and can close the local overlay on the
last configured key's release without validating the complete chord. This is
a coexistence edge case to test, not evidence that it caused this watchdog.

New Game Plus Native is installed as a RED4ext plugin with redscript menu
extensions, including `NGPlusButtonAdder.reds` and character-creation menus.
It is a separate native-menu validation target, not a CET ImGui window.

## Next controlled checks

1. Remove the shortcut collision and restart; reproduce CET-only open/close
   from normal gameplay without entering the character editor.
2. Separately test Character Preset Manager editor enter/leave, then CET toggle.
   Record the time and preserve the next report if either stalls. Existing
   evidence is insufficient to disable a mod or claim a crash fix.
3. Validate New Game Plus's native menu selection using the existing hand laser.
4. For standalone CET in VR, use an explicit texture/fence and input bridge
   between the two renderers, retaining CET's own ImGui context and mod runtime.
   A proper implementation must handle panel sizing, laser coordinates, input
   capture, close/reopen, device resize, and shutdown before installation.

This investigation does not implement experimental CET overlay integration or
establish a fix for the watchdog crash. The separate CyberFinger controls update
and its installation status are recorded in `cyberfinger-session-todo.md`.
