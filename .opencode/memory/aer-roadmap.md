---
name: aer-roadmap
description: Roadmap and decisions for AER 2.0 (Luke-Ross-style) in CyberpunkVRPort
metadata: 
  node_type: memory
  type: project
  originSessionId: 0353ad85-8636-45bd-8b7d-635741e848f9
---

Target: AER 2.0 like Luke Ross RealVR.dll ("AER V2 1/2 rate"). Three pillars:
1. **AFR (one eye per game frame)** — ✅ already working in the project.
2. **Reprojection / interpolation** — partial. Rotational reprojection already works
   (frame thread is HMD-paced and resubmits the held `m_capturedEyeFrames` pair each
   display frame with the current `predictedDisplayTime`, so the runtime time-warps it).
   Missing: depth submission (`XR_KHR_composition_layer_depth`) for positional
   reprojection / SteamVR motion-smoothing. Pose extrapolation was missing → now added.
3. **Disable heavy post-effects** (SSR, Bloom, Motion Blur, Chromatic Aberration, DoF,
   Film Grain, Lens Flare, Vignette) — ❌ not started; they break AER (per-eye
   screen-space mismatch → inter-eye flicker).

User decisions (2026-05-30):
- Post-effects / engine settings: **hybrid + RED4ext** — keep the dxgi proxy for
  AER/render, add a RED4ext companion plugin for engine CVars (SSR, Bloom). Some effects
  (motion blur, CA, film grain, DoF, lens flare) are reachable via the already-hooked
  `CP2077SettingsRes`; SSR/Bloom need CVar-level access.
- Start order: **1/2-rate pacing (pillar 2)** first.

Progress:
- ✅ Stage 1 (1/2-rate pacing): head velocity captured via `XrSpaceVelocity` chained to
  `xrLocateSpace`; forward pose prediction added in `GetHeadPose` (the single consumer,
  dxgi_proxy.cpp:~1326); new setting `xr_motion_predict_ms` (default 0 = off). Prediction
  applies only to the **live** pose, NOT the sequential-sync latched pose — so to use it,
  set `xr_sync_sequential=0` (sync-freeze and prediction are opposed latency strategies).
- ✅ `xr_stereo_scale` debug/comfort multiplier (default 1.0) — folded into
  `GetDesiredHalfIpd()`; F10 slider "Stereo separation x". Crank to 3-5x to make AER
  eye-alternation obvious for verification.
- ⚠️ **Stereo injection moved/duplicated into `OnLocateCameraCallback`** (the LocateCamera
  hook, ~dxgi_proxy.cpp:1327). KEY FINDING: the original `OnPatchCameraCallback` writes the
  per-eye IPD shift to a struct that does NOT reach the render (logged spans of 13.5 m and
  FLT_MAX positions → mostly `applied=0`, and no visible effect even at 5x). LocateCamera
  writes the camera transform that DOES drive the view (head orientation/6DoF work there),
  so the per-eye ±IPD/2 lateral shift along the head-right vector is injected there,
  ungated by 3DoF/6DoF. Symptom that triggered this: "no eye alternation on the flat
  monitor" + stereo_scale had no effect. NEEDS HMD VALIDATION (user's PICO 4).
- ✅ Stereo confirmed working on HW after the LocateCamera fix (monitor wobble + depth).
  But rendering at the raw runtime IPD hyper-separated (near objects doubled); comfortable
  at ~0.25-0.3x. Root: REDengine first-person world-scale, NOT a per-person thing.
  FIX: `GetDesiredHalfIpd()` = runtimeHalfIpd (auto per-headset, from OpenXR view
  separation) × `kVrStereoCalibration` (0.30f, universal game constant) × xr_stereo_scale
  (personal tweak, default 1.0). So IPD is automatic for any person/headset; 1.0 is now the
  calibrated natural value. kVrStereoCalibration is applied to the IPD path only (left
  GetWorldScale=1.0 so 6DoF translation is untouched). If still off on HW, nudge the 0.30
  constant. Reset user's persisted xr_stereo_scale 5.0→1.0.
- ✅ Post-effects: user confirmed disabling DLSS + motion blur + CA/film grain/lens flare
  in the game's own graphics menu removes the AER temporal ghosting ("transparent residual"
  double-image). The programmatic Pillar-3 (force these off via settings hook/RED4ext) is
  still TODO but the manual path works.
- ⚠️ Reprojection (Pillar 2): head-turn smoothing is a RUNTIME feature — Virtual Desktop
  Synchronous Spacewarp (SSW) / SteamVR Motion Smoothing. Our frame thread submits every
  vsync with the held pair, so "Auto" SSW may not engage → recommend SSW "Always On".
- ⚠️ STEREO WAS INVERTED (pseudoscopic): near-object parallax landed on the wrong eyes.
  Fixed by flipping the per-eye sign in the LocateCamera IPD block (eye0 now +right, eye1
  -right). If it ever inverts again, that single sign is the lever.
- ⚠️ Motion prediction caused asymmetric LEFT-eye tearing when enabled (predict=30): it
  moves the RENDER pose (GetHeadPose) but NOT the submitted reprojection pose, so the
  runtime warps "off". DISABLED for now (predict=0). To re-enable properly, the same
  forward prediction must also be applied to the submitted per-eye projection pose
  (m_capturedEyeFrames[].pose / m_syncedEyePoses), not just the render pose.
- ✅ Inversion fix confirmed on HW. After fixing it, correct depth ≈ stereo_scale 5 under the
  old 0.30 calib → rebased `kVrStereoCalibration` to **1.5** so stereo_scale=1.0 = correct
  (~1.5x runtime half-IPD), still auto per-headset via runtimeIpd. (The earlier 0.3 was
  mis-measured against inverted stereo.)
- ✅ **Mono jitter/rubber-banding fixed for real** (2026-05-31). The actual fix was NOT
  depth, compositor mode, or guessed present-lag. The root cause was pose/frame mismatch on
  the async REDengine/D3D12 pipeline. The working solution binds the submitted pose to the
  exact render-camera `seq` seen by the engine: poses are stored keyed by `g_lastLocateSeq`,
  the final render path records the exact rendered `seq`, and Mono submit fetches that exact
  pose instead of guessing by present count. Result: stable head-tracked Mono with the user
  reporting the image as "super smooth" and only negligible residual artifacts.
- ✅ **AER frozen-HMD bug fixed** (2026-05-31). Cause: when switching from Mono to AER, the
  frame thread could remain parked in the Mono cadence inner-loop waiting for a mono serial
  that would never advance once AER was enabled. That stalled `xrWaitFrame/xrEndFrame`, so
  the HMD froze while the game kept promoting AER pairs. Fix: bail out of the Mono cadence
  wait loop as soon as `m_aerSubmitEnabled` flips on.
- ✅ **AER first-pair parity bug fixed** (2026-05-31). Cause: pair ids were derived from the
  global present counter parity while eye selection was reset independently on AER enable.
  Depending on when AER was toggled, eye0/eye1 of the first pair could land in different
  pair ids, so no complete pair ever formed and only empty frames were submitted. Fix:
  `m_aerPairCounter` is now advanced by the eye cadence itself (bumped on eye0 presents), so
  both eyes of a pair always share the same `pairId`.
- ✅ **AER eye-slot / rendered-eye mismatch fixed** (2026-05-31). This was the key fix for
  "no stereo depth, only alternation". Root cause: the eye used to tag/capture the frame in
  `OnPresent` came from the LIVE `m_renderEyeIndex`, while the camera hook had already used a
  potentially different eye on the frame that was actually rendered. Result: a frame rendered
  as eye1 could be stored in eye0's slot (or vice versa), so the HMD saw alternating views
  instead of stable stereo. Fix: just like Mono pose, the eye index is now stored by camera
  `seq` in `dxgi_proxy.cpp`, the final render path exposes the rendered `seq` + eye, and
  `OnPresent` uses the ACTUAL rendered eye for AER capture/tagging. This removed the visible
  alternation.
- ✅ **AER pseudoscopic depth fixed after the eye-tag repair** (2026-05-31). Once rendered-eye
  tagging was correct, the old per-eye sign in the LocateCamera IPD block became inverted.
  Working orthoscopic sign is now: eye0 -> `-right`, eye1 -> `+right`.
- ✅ **AER menu black-screen fixed** (2026-05-31). Cause: the AER menu path still depended on a
  valid stereo pair and could end up submitting an empty frame in menu contexts. Fix: when
  menu mode is active, AER deliberately falls back to the already-working mono snapshot/quad
  submit path for the menu only; world rendering remains AER.
- ✅ **`xr_aer_debug_eye` diagnostics fixed** (2026-05-31). Values 2 and 3 were silently
  clamped back to 1 in two separate state/plumbing paths, invalidating diagnostics. It now
  correctly preserves 0..3.
- ⚠️ `xr_aer_v2` / optical-flow frame generation is NOT the current baseline. The stable
  starting point is ordinary AER (AER 1.x) with the above fixes. AER V2 remains a separate
  path to validate later.
- ❗ OPEN: LEFT eye (eye 0) judders/tears on head turn, right eye fine; persists at
  predict=0, sync=1; user says SSW doesn't change it. Asymmetric. Leading hypothesis: AER's
  inherent inter-eye time offset (eye0 captured from ODD presents = 1 frame older than eye1
  from EVEN presents) is not being reprojected away — likely because the frame thread
  submits EVERY vsync with the held pair, so the runtime sees "full rate" and never engages
  motion-smoothing. Isolation tests queued: xr_aer_debug_eye=1 vs =2 (both eyes one source),
  and aer_submit=0 (mono). ini reloads live (~200ms poll), no restart needed.
  Likely fix: submit at content rate (not every vsync) so runtime reprojects, OR reduce the
  inter-eye age. PollLiveControls loops in a bg thread (dxgi_proxy.cpp:3308) Sleep(200).
- Current HW test config: aer_submit=1, sync_sequential=1, motion_predict_ms=0,
  stereo_scale=1.0, DLSS/MB/post off.
- ⏳ Next: confirm inversion fix + that predict=0 stops the tearing; then re-add prediction
  with submit-pose correction; remove dead PatchCamera shift; depth submission; RED4ext SSR/Bloom.

Current practical baseline after the 2026-05-31 session:
- Mono: working, smooth, seq-bound pose submit.
- AER world: working stereo depth, no eye-alternation bug, orthoscopic sign fixed.
- AER menu: works via mono fallback while menu is open.
- Things explicitly NOT to regress: the Mono seq-bound jitter fix; the AER seq-bound
  rendered-eye tagging; the orthoscopic LocateCamera IPD sign.

Note: I cannot test on VR hardware — ship tunables default-off and let the user validate.
