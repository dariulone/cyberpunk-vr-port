# Haptics: OpenXR and optional PSVR2 Toolkit

The port can send controller vibration through OpenXR on every supported headset,
including PSVR2. PSVR2 users can instead select the external Toolkit bridge for
advanced adaptive triggers and grip effects. The existing Triangle-touch D-pad
and Create-button mappings are preserved.

## Output selection

In **F10 → Controls**, select OpenXR, prefer Toolkit, or Auto. Auto is the default:
on PSVR2 it chooses Toolkit when a compatible bridge process owns a loopback UDP
listener in the same Windows session; otherwise it chooses OpenXR. Other headsets
always use OpenXR. Explicit Toolkit preference also falls back to OpenXR when
that listener is absent. Detection is a startup snapshot, not a driver-health test.

**Restart the game after changing the output selection.** Start the bridge before
the game for Toolkit output. Stop the external bridge/DSX when deliberately using
OpenXR: the port prevents its own two outputs from competing, but cannot stop
independent programs from writing to the controllers.

OpenXR vibration does not require Toolkit, Enhanced DualSense Support, or DSX.
A working runtime haptic binding is required. The log records bound output paths
and changes in OpenXR apply results; an accepted API call cannot prove that a
controller physically vibrated.

## Feedback and F10 controls

- Firing strength follows the weapon recoil kick. A genuinely supported,
  single weapon can vibrate the support hand more weakly. Dual-wield impacts
  use the firing-hand event without adding support-hand vibration.
- Separate hand queues prevent rapid left/right Toolkit impacts from overwriting
  each other in the bridge mailbox. Existing block/parry priorities are retained.
- Physical melee swings receive feedback through the included CET script/native
  pulse hook. Existing interaction event profiles remain available.
- Main OpenXR pulse gain and driving feedback strength are independently adjustable.
  Setting driving strength to zero disables the port's road feedback.
- Driving channels can use stereo, the stronger channel on both hands (default),
  left on both, or right on both. An optional per-hand grip gate restricts feedback
  to hands holding the vehicle controls. These road controls work with either output.
- Toolkit full-game audio enable/strength edits the existing PSVR2 Haptics companion
  settings. If the companion is absent or invalid, those controls explain why they
  are unavailable. They do not silently install external software.

Road feedback extracts 28–320 Hz energy from the Windows default audio output
and applies it only while driving. It is not an isolated engine-audio stream:
other applications and music can contribute. Stale capture samples expire.
The port's road feedback is separate from Toolkit full-game audio, so full-game
sound vibration can stay off while road feedback remains enabled.

The port handles dual-hand impact pulses; Toolkit single-gun recoil and adaptive
trigger profiles remain owned by Enhanced DualSense Support. This PR does not
change projectile aiming or invent a new source for the firing-hand event.

## Optional advanced PSVR2 setup

1. Download [PSVR2Toolkit Cyberpunk bridge v0.3.2](https://github.com/satyaloka93/PSVR2Toolkit/releases/tag/cyberpunk-dsx-bridge-v0.3.2)
   and extract outside the game folder. The ZIP separates `driver` and `bridge`.
   The `017` launcher/protocol name is intentional and remains compatible.
2. If Toolkit is not installed, follow the [base Toolkit installation guide](https://github.com/BnuuySolutions/PSVR2Toolkit/wiki/Installation).
   The stock Sony PSVR2 driver does not expose the Toolkit controller API needed
   for adaptive triggers and PCM grip vibration. The bridge ZIP contains a
   replacement driver, not all base Toolkit files or the original Sony backup.
3. **Keep an already compatible Toolkit driver.** The v0.3.2 bundled driver is
   identical to the v0.2.0/v0.3.1 bundle. It adds raw-trigger transport and clears
   stale grip PCM attenuation; this bridge uses standard CAPI trigger modes, so
   raw-trigger support alone does not require replacing a working installation.
   A different file hash alone does not prove incompatibility. If a tested driver
   baseline is needed, close SteamVR and use the optional
   `driver\INSTALL_RAW_TRIGGER_DRIVER.cmd`, following its backup/rollback instructions.
4. Install [Enhanced DualSense Support](https://www.nexusmods.com/cyberpunk2077/mods/4156)
   and [Native Settings UI](https://www.nexusmods.com/cyberpunk2077/mods/3518), including
   their prerequisites. Run once to create EDS settings, then close the game.
5. Run `bridge\setup_cyberpunk017.cmd`. It previews changes and asks for confirmation:
   it installs the audio-menu companion, sets `UDPautostart=false`, backs up and
   disables the incompatible EDS native launcher DLL, and keeps its CET mod/profiles.
6. Start SteamVR and the Sense controllers, run `bridge\run_cyberpunk017.cmd`, then
   launch Cyberpunk with Auto or Toolkit selected. **Steam DSX is not required.**
   Do not run DSX or the EDS UDPClient alongside this bridge.
7. Configure full-game audio in **Mods → DualSense settings → PSVR2 Haptics**, or
   through this port's F10 controls. Both edit the same companion preferences.
   The companion defaults new installations to audio off, with an initial opt-in
   strength of 35%; existing preferences are preserved.

This port PR ships no Toolkit driver, bridge executable, or EDS trigger profiles.
The later local short gear-shift polling correction is not part of the linked
v0.3.2 Toolkit release or this port change.

## Persistent settings

| Key | Meaning |
| --- | --- |
| `xr_psvr2_haptic_backend` | 0 OpenXR, 1 prefer Toolkit, 2 Auto; restart required |
| `xr_openxr_haptic_gain` | Main native pulse gain |
| `xr_openxr_vehicle_haptic_gain` | Road strength; 0 disables |
| `xr_vehicle_audio_channels` | 0 stereo, 1 stronger on both, 2 left on both, 3 right on both |
| `xr_vehicle_audio_require_grip` | Require the corresponding hand to hold a vehicle control |

These use the normal F10 save path, alongside existing `xr_haptics` and
`xr_haptic_scale`. Full-game Toolkit audio uses the separate companion
`bin/x64/plugins/cyber_engine_tweaks/mods/PSVR2 Haptics/settings.ini`.
