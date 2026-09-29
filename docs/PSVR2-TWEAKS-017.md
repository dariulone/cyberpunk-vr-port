# PSVR2 tweaks on upstream 0.1.7: first commit

Base: official `0.1.7`, `a0c23fe110530bb74a73b3adf1ffa7d83050c43d`.
Branch: `psvr2-tweaks-0.1.7`. This is not the older `psvr2-tweaks` implementation.

## Scope

This first change restores the measured Sense input behavior and provides a safe
launcher for the existing optional PSVR2Toolkit DSX-compatible bridge. It does
not add Quest/general OpenXR haptics, alter rendering, change shared-memory
slot meanings, or port unrelated old gameplay remaps. General OpenXR haptics
must be a separate commit with explicit actuator ownership.

## Controls

| Sense input | Result |
|---|---|
| Touch Triangle, move right stick | D-pad, threshold 0.5; no right-stick turning during the whole touch |
| Click Triangle | Y remains available |
| Left Create / application SystemButton, release before 500 ms | Start / pause |
| Hold Create at least 500 ms | Back / in-game menu once; release does not also emit Start |
| Triangle touch + R3 without L3 | Same menu tap/hold fallback; suppresses R3's gameplay action |
| L3+R3 | Upstream overlay shortcut, priority over the fallback |
| L3+right stick | Existing upstream D-pad fallback and threshold unchanged |

The new touch action and SystemButton identity are created only for PSVR2,
identified from the OpenXR system name. Other headset bindings/menu behavior are
unchanged. SteamVR presents Sense as Oculus Touch; Triangle touch is
`/user/hand/left/input/y/touch`.

If Create does not arrive, open SteamVR controller bindings for CyberpunkVR and
assign physical **left Create** to **SystemButton (Sense Create)**. The Oculus
left-menu suggestion remains present, but SteamVR's Sense auto-remapper has
previously dropped that suggestion. An old custom binding may need updating to
include the touch action. Right Options/the SteamVR dashboard remain reserved;
this application action does not open the SteamVR dashboard.

SystemButton timing runs at XInput polling, not as a transient XR-frame pulse.
Losing input/focus or overlay capture cancels pending taps. Shift also clears
physical/Steam XInput look contributions and pending snap rotation.

## Optional adaptive triggers / advanced haptics

Run `scripts\Start-PSVR2Bridge.bat` **after starting SteamVR and before the game**.
Defaults match the existing local installation; use `-GameRoot` and `-BridgeRoot`
for other locations. `-CheckOnly` validates without starting the bridge.

Requirements installed separately:

- Enhanced DualSense Support CET mod and Native Settings UI.
- Compatible PSVR2Toolkit bridge/CAPI driver setup.
- Enhanced DualSense Support `UDPautostart=false`.
- No DSX app, bundled UDPClient, second bridge or other Sense actuator owner.
- Disable the unnecessary Enhanced DualSense Support native launcher DLL if
  present. Never patch its advertised game version to bypass RED4ext validation.

The wrapper does not edit those third-party settings, install drivers, start/kill
processes automatically, or expose an OpenXR vibration-output action. It passes
`--no-vr-motion-haptics` to the existing bridge. Weapon/adaptive-trigger profile
translation and audio-derived grip haptics remain available.

### Why legacy motion haptics are disabled

The old bridge interpreted shared float slots 157-160 as sequence, hand,
amplitude and duration. Upstream 0.1.7 assigns those same slots to right B,
left Y, R3 and right-trigger analog. Enabling the old watcher would misinterpret
input as haptic events. This commit deliberately does **not** overwrite upstream
slots or claim explicit swing/impact pulses work. Restoring those pulses needs
a separate versioned IPC channel and matching bridge consumer/producer changes.
Use this launcher, not the old bridge shortcut that enables that watcher.

## Validation and headset checklist

Standalone policy tests: `tools/psvr2_input_tests`. Test tap/hold boundaries,
focus cancellation, headset identity, all D-pad directions, shift suppression,
Triangle-click preservation and unchanged non-PSVR2 policy. Build with CMake and
run CTest; the test executable prints its check count.

Before considering a bundle release, test on the headset:

1. Touch Triangle and select all directions; verify no turn/crouch/dash.
2. Release touch and verify turning resumes. Triangle click still switches weapons.
3. Tap and hold Create; verify one correct menu event, including release behavior.
4. Test Triangle+R3 fallback and L3+R3 overlay priority; L3 alone still works.
5. Open/close the VR overlay and lose/regain focus; no delayed menu event.
6. With only the safe bridge running, verify weapon trigger resistance/recoil
   and audio haptics. Explicit legacy VR melee pulses are intentionally absent.

Successful builds, action suggestions and preflight checks do not establish
physical Sense input or haptic correctness; retain the runtime/headset test gate.

Sources: the old fork's `.okf/fixes/psvr2-triangle-dpad.md` and
`.okf/operations/psvr2-adaptive-triggers.md`; upstream `Utils/SharedSlots.hpp`;
PSVR2Toolkit `vr_motion_haptics.cpp` and `run_bridge.ps1`.
