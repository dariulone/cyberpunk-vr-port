# Save-load transition investigation, 2026-11-16

## November 18 compositor panel

The user confirmed that loading a saved game looks correct on the Quest Pro.
On returning to the main menu, the artwork started correctly, stretched, then
gave way to a normal lobby. The first exit in the recording
`build/loading-diagnostic/exit-1795059878618.jsonl` shows the loading panel
at 218.81 s. The main-menu controller opened at 219.52 s. By 219.67 s the
headset received a scene projection while the loading layer was still ticking.
The image capture `47060-83359453` contains three window/panel pairs before
the switch. The stretched interval was reported in the headset.

The return correction keeps the existing panel and its last mono artwork
during that overlap. The game thread checks the lobby curtain, the new lobby's
composition and the visible root widgets of the loading controllers. Two clear
updates release the panel. The existing 20-second lobby recovery interval
bounds the wait if the readiness signals fail. A headset retest of the return
transition is pending.

The combined return-to-lobby and mirror-wrist build was installed with SHA256
`97d593a7493d1dffd5d7ce1b88e6fe05e8a359014f255d4320cba30baf59a482`.
The deployment receipt is in `build/roomscale-plugin-deploy-20261118-230643/`.

The one-shot capture request was still present when work resumed. The earlier
recording established that the loading layer stayed active. The previous build
drew the panel into two game eye images and submitted them as scene projections.
The November 18 build sends one flat artwork texture on an opaque OpenXR quad,
using the same layer type as Esc. SteamVR positions it for each eye. The panel
anchors on the first valid head pose and can reanchor after a tracking reset.
During frames where the engine reports drawing the world, the capture holds
the last mono artwork. A load that has yet to supply mono artwork starts dark.
The lobby curtain and gameplay blink no longer affect the save-load artwork.

The full Release build and all 26 overlay/loading checks passed. The GPU test
compared the pixels of the captured source and cropped panel. The placement
test covered head motion, initially unavailable tracking and a space change.
During a save load the diagnostic log should say
`kind=2 ... paired=0 layer=quad`, then `kind=0 ... layer=projection` as
gameplay returns. For this build, the diagnostic's `eye.ppm` is the flat
texture used on the quad. SteamVR applies its final placement after that point.

Installed DLL SHA256 `E87A2AF53518E98DC1E866422684F211DBC32579610265A92AAD81F69F309055`.
The deployment receipt is `build/loading-diagnostic/panel-deployment.json`,
the matching symbols are in `build/loading-diagnostic/panel-symbols/`, and
the prior DLL is backed up in `build/roomscale-plugin-deploy-20261118-221301/`.

## November 16 baseline

The merged 0.2.45 candidate still flickers during saved-game transitions on
Quest Pro / Steam Link / AMD RX 7900 XT. The user describes loading picture and
game/lobby alternating; on the desktop a non-SBS image precedes SBS rendering,
but the transition does not appear correctly in the headset. The HUD toggle,
melee toggle and Esc-menu laser were confirmed working by the user.

## Recorded evidence

Read-only process sampling used the exact installed DLL/PDB, SHA256
`7D78E81EA2856E16C29ABE0797F492CE9ABF9FAB76E5CEA9F71EE7B8328E1A55`.
Artifact: `build/loading-transition-20261116.jsonl`, 4,896 samples at roughly
20 ms intervals. This is state telemetry, not an image/video recording.

- The save load ran approximately 00:31:45–00:31:54 local time. All 431 sampled
  loading records had Loading presentation on both capture and submission.
- The loading screen remained up, held and ready, with one unchanged anchor.
  Its initial fade completed normally; opacity then remained 1.
- The OpenXR session stayed running and focused. Tracking origin changed once,
  from 3 to 4. No repeated presentation-mode transitions were logged.
- The always-enabled empty-end counter did not increase. The longest observed
  gap between newly submitted source serials was about 187 ms.
- RuntimeDiagnostics was off. Gated counters remaining zero are **not** evidence
  that every OpenXR API call succeeded. Sampling can straddle publication;
  momentary `lastSubmission.valid=false` values are not failures by themselves.
- SteamVR recorded one driver-sync timeout at 00:31:59, after this load ended;
  it does not explain repeated flicker throughout the recorded loading interval.

These observations narrow the investigation to image content / GPU delivery;
they do not establish a root cause or confirm a fix. In particular, the loading
panel refreshes from the desktop only when `EngineStereo::WorldDrawn()` is false.
The relationship between that heuristic and the actual backbuffer pixels still
needs to be checked.

## One-shot image capture

Create an empty `log/loading-capture.request` in the VR plugin's directory.
The plugin consumes that exact request and arms the next post-boot loading
transition; ordinary gameplay and the introductory notice are not recorded.
An unused request expires after ten minutes of running frames.

During that load, `Render/LoadingCapture` copies the desktop backbuffer and the
main VR-eye snapshot from the same capture command list. It saves paired PPM
thumbnails (at most 480 pixels wide) and `frames.jsonl` under
`log/loading-capture/<pid>-<tick>/`. Metadata includes the source serial,
loading flag, fresh-picture decision, world-drawn flag and physical eye index.
It samples at most five pairs per second, up to 60 pairs, and stops at the end
of the first load. The diagnostic samples precede the desktop-mirror crop.

Two fence-owned readback slots bound memory use. Busy slots skip samples;
there is no GPU wait, debugger attachment, input injection or game-state edit.
Only completed readbacks are mapped; shutdown retains pending GPU write targets
until their fence finishes. The next capture remains disabled until explicitly
requested again. Image readback/disk work adds diagnostic overhead, so this is
not a performance benchmark.

The GPU regression uses deliberately different window/eye colors and checks
opt-in behavior, waiting for a load, deferred readback, shutdown ownership,
metadata and pixel contents. All 26 overlay/loading tests passed, including
D3D12 validation on the WARP test device. The full Release plugin build passed.

## Installed diagnostic build

- Integration worktree / branch only; original `main` remains unchanged.
- DLL SHA256: `91D18B8EBA9150013D83E0DB46D08754A33E8EEE5887FB5C55FBB0D187E16B40`.
- DLL/PDB: `build/loading-diagnostic/package/`.
- Previous installed DLL/PDB: `build/loading-diagnostic/installed-symbols/`.
- Verified backup: `build/roomscale-plugin-deploy-20261116-004900/`.
- Deployment and independent verification: `build/loading-diagnostic/`.
- Only the plugin DLL was replaced; an empty capture-request file was added.
  Runtime/launcher/calibration settings, CET bindings and CET ASI hashes match.
- One capture is armed. Next: launch from GOG, load a save, inspect the paired
  thumbnails against the metadata before deciding on a rendering correction.

The pixel diagnostic is **not a flicker fix**. The headset transition still
requires investigation and subsequent user confirmation.
