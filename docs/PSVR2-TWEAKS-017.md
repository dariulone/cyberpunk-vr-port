# PS VR2 controls, haptics, and display options

This change adds PS VR2 Sense shortcuts, restores an optional advanced-haptics
path through PSVR2Toolkit, and exposes FOV and overlay-pacing controls in F10.
Other headset input behavior is unchanged.

## Sense controls

| Input | Action |
|---|---|
| Touch Triangle and move right stick | D-pad; suppresses right-stick turning while touched |
| Click Triangle | Existing Y action |
| Tap left Create | Start/pause |
| Hold left Create for 500 ms | Back/menu once; release does not also trigger Start |
| Triangle touch + R3 | Fallback menu chord |
| L3 + R3 | Existing overlay shortcut; retains priority |
| L3 + right stick | Existing D-pad fallback |

Create is mapped to the OpenXR application `SystemButton` action. If SteamVR
does not deliver it, edit the CyberpunkVR controller binding and assign physical
left Create to **SystemButton (Sense Create)**. SteamVR has previously omitted
that suggestion for Sense controllers. Right Options remains available for the
SteamVR dashboard. Triangle touch is `/user/hand/left/input/y/touch` under the
SteamVR Sense-to-Oculus-Touch mapping.

Pending Create taps are cancelled when focus or input is lost, or when the VR
overlay captures input. The D-pad shift clears physical and Steam input look
contributions and pending snap rotation.

## Optional PS VR2 haptics

The port exposes `SetVRHapticPulse(hand, amplitude, durationMs)` for the CET
weapon script. Swing and hit-check pulses use a dedicated mapping named
`Local\CyberpunkVR_PSVR2_Haptics_017_v1`; upstream input slots 157–160 keep
their controller meanings. Without a matching bridge, the optional pulses have
no PS VR2 actuator consumer.

The matching external bridge and driver are provided by the
[PSVR2Toolkit v0.3.1 release](https://github.com/satyaloka93/PSVR2Toolkit/releases/tag/cyberpunk-dsx-bridge-v0.3.1).
Install its driver and use its Cyberpunk 0.1.7 launcher. Also install the
Enhanced DualSense Support CET mod and Native Settings UI. Set its
`UDPautostart=false`; do not run DSX, its bundled UDPClient, another bridge, or
another Sense actuator client at the same time. The port does not install or
redistribute the Toolkit, driver, or Enhanced DualSense Support mod.

## F10 options

F10 → STEREO → STEREO VIEW contains the explicit FOV override and presets. The
override defaults to 104.5° on PS VR2 and can be disabled to return to automatic
projection coverage. Other presets remain selectable; narrower coverage can
expose image edges.

F10 → STEREO → PERFORMANCE contains **Stable pacing with no overlay graphics**.
It is on by default. When checked, the port submits the empty overlay command
list and fence even when the weapon dot is absent. This retained the prior
queue-pacing behavior in the user's external-OFXR bike test. Turning it off
skips that work and may reintroduce bike or post-dismount cadence drops. The
option is session-only and is not a frame-generation switch.

## Validation

`tools/psvr2_input_tests` covers D-pad directions, touch suppression, Create
tap/hold timing, focus cancellation, headset identity, and preservation of
Triangle-click and non-PSVR2 behavior. The FOV override and pacing option also
need headset verification on the target runtime. Source builds and policy tests
cannot prove physical Sense input or haptic output.
