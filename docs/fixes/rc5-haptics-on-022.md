# RC5 haptics on upstream 0.2.2

Restore the RC5 event model (`de19d872`), retaining 0.2.2's physical
left-handed mapping and all 16 haptic event profiles. This is a haptics-only
change; frame generation and adaptive pacing are unchanged.

- F10 Controls selects OpenXR, prefer Toolkit with fallback, or Auto on PSVR2.
  Auto is the default. Detection checks a live same-session Toolkit executable
  owning a loopback UDP listener once at startup. Other headsets use OpenXR.
- Restart after changing output. Start the Toolkit bridge before launching the
  game for advanced triggers; stop it when deliberately using OpenXR. The port
  does not stop other processes or replace drivers. A saved Toolkit preference
  falls back to OpenXR when the bridge listener is absent.
- Restore kick-based shot strength, weaker two-hand support vibration, physical
  melee swings, and optional low-frequency driving audio with gain, channel and
  wheel-grip controls. Blocks/parries retain priority over these envelopes.
- Dual wield impacts belong only to the gun firing, independently of adaptive
  trigger profiles. Toolkit impacts queue fairly across both hands instead of
  overwriting the single-slot bridge mailbox. Single-gun Toolkit recoil remains
  owned by Enhanced DualSense Support; install the existing companion/bridge.
- Toolkit audio enable/strength is editable in F10 through the existing PSVR2
  Haptics companion settings. Its driver, DSX profiles and bridge are external:
  https://github.com/satyaloka93/PSVR2Toolkit/releases
- OpenXR output logs actual bound haptic paths and changes in apply result. A
  successful API call is not evidence that a controller physically vibrated.

INI keys: `xr_psvr2_haptic_backend` (0 OpenXR / 1 prefer Toolkit / 2 Auto),
`xr_openxr_haptic_gain`, `xr_openxr_vehicle_haptic_gain`,
`xr_vehicle_audio_channels` (0 stereo / 1 maximum / 2 left / 3 right),
`xr_vehicle_audio_require_grip`, plus existing `xr_haptics` and `xr_haptic_scale`.
The new keys save through the normal F10 settings path.

Validation: Release DLL built with MSVC. Windows input, routing, production
playback and fair-queue tests pass. The socket-only bridge lifecycle fixture
skips when a real bridge is already detected, rather than stopping it; that
condition occurred on this machine. Physical controller strength still needs
in-game validation. The later independent aiming commit determines which gun
fired on the additional projectile/orientation paths.

Suggested PR: **Restore configurable OpenXR/Toolkit haptics and per-hand impacts**.
Include this code, Lua swing change and tests together. It does not depend on
our framegen package or driving-grip toggle.

## Follow-up: driving feedback while Toolkit is selected

The first integration hid the channel/grip controls and never started the port's
vehicle capture in Toolkit mode. This matches the inspected RC5 snapshot, but
fails the user's required later driving behavior: with the full-game audio layer
off, only the bike's left Machine-profile grip rhythm remains. The unchanged EDS
bike script still swaps the trigger baselines and moves the shift pop to R2.
Do not confuse that asymmetric semantic effect with balanced road audio.

The existing 28–320 Hz vehicle capture now feeds either selected owner. Toolkit
receives it through the existing fair per-hand mailbox; OpenXR remains excluded
in Toolkit mode. The F10 driving enable, strength, channels and grip gate are
available with either route. New configurations default to stronger-channel-to-
both. Existing saved channel preferences remain intact. The local deployment
explicitly selects both hands. Toolkit road pulses last 90 ms to cover alternating
mailbox delivery at 30 FPS; OpenXR retains 45 ms. Road effects stop being queued
on exit/mute/stale input and the final short pulse expires naturally.

This is an extension of the port's existing road-envelope path to Toolkit, not a
claim that the exact same conditional routing existed in the RC5 commit. No
bridge executable, trigger profile or full-game audio preference is replaced.
Full-game audio and DSX semantic rhythms can still contribute separately.
Production playback tests cover both owners, panned audio, grip gating, mute,
vehicle exit and priority. Physical balance remains a headset validation.
