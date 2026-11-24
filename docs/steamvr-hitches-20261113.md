# SteamVR frame pacing and interaction follow-up — 2026-11-13

## Scope and current status

Investigate sprint microstutters from ManualSave-497 at H10, compare single-pass ON/OFF, and verify both framegen states on SteamVR. The requested inline experiment was withdrawn after a HUD regression; the current requested direction is **threaded SteamVR submission**, leaving Virtual Desktop / Simulator behavior unchanged.

Additional requested changes: windshield-style minimap, no right-grip weapon transfer during two-hand use, slide grip under pistol cant, more support points on non-pistol weapons including melee.

No manual game save, no push. The user's independent `reference_view.lua` change is excluded. After the explicit no-focus instruction, tests use native background pad events only, not foreground activation or keyboard injection.

## Confirmed microstutter cause

`XrFrameState.predictedDisplayPeriod` is the display period for the **next application-generated frame**. OpenXR explicitly permits it to differ from hardware refresh.

On this physical SteamVR headset, the usual value is about 11.111 ms (90 Hz), but SteamVR occasionally returns 22.222 ms for one cycle. The port treated that as a physical refresh-rate change and doubled it again for framegen. The producer target jumped by about 50 ms and requested a 36–39 ms sleep.

A 70-second trace captured 19 such sleeps among 3,102 samples. All coincided with that doubled period. The mutex wait stayed below 0.086 ms; actual timer wait followed the requested wait. This was not a slow timer, physics-hand work, or an unexplained rendering stall.

### Fix and verification

`PresentationPeriod` filters the SteamVR period only for producer pacing. Single-cycle slow reports do not change the established cadence; sustained slower changes are adopted after one second, faster changes immediately, and session gaps reset it. The raw OpenXR period remains unchanged for view prediction and display-time calculations. VDXR/Simulator do not use this filter.

Physical-headset verification, same H10 save, 70 seconds with framegen ON. **Qualification:** subsequent compositor screenshots show the SteamVR dashboard open, and the launch log stops at XR `VISIBLE` (4), not `FOCUSED` (5). These results validate the observed pacing defect under that condition; the dashboard-closed run is in the next subsection:

| Measurement | Before | After |
|---|---:|---:|
| Requested sleeps >25 ms | 19 / 3,102 | 0 / 3,144 |
| Maximum requested sleep | 39.21 ms | 18.68 ms |
| Maximum actual wait | 39.36 ms | 18.84 ms |
| Real FPS after fix | — | 44.94 |
| Frame interval p99 after fix | — | 25.29 ms |
| Maximum frame interval after fix | — | 32.12 ms |

The earlier 60-second physical-headset run had three 45–52 ms frames, with 35–38 ms spent inside pacing. This identifies and fixes that particular mechanism; it does not promise absence of every possible streaming or OS stall.

### Normal gameplay (dashboard closed, session FOCUSED)

After the user dismissed the dashboard, the log reached `Session state -> 5`. Same-launch ABBA, background sprint, 3 runs per block (`headset_focused_fg_abba.json`):

| Framegen | Phase | Real FPS | p99 | Max | >33 ms |
|---|---|---:|---:|---:|---:|
| ON | stand | 44.59 | 30.49 ms | 33.42 ms | 1 / 714 |
| ON | sprint | 44.37 | 31.33 ms | 37.26 ms | 2 / 666 |
| OFF | stand | 56.76 | 25.26 ms | 31.36 ms | 0 / 908 |
| OFF | sprint | 53.37 | 30.73 ms | 35.91 ms | 3 / 801 |

Maximum pacing wait with framegen ON: 18.85 ms (median 8.40, p99 11.46); before the fix it reached 39 ms. Every remaining >30 ms frame had ≤7 ms pacing, so those are game-frame work during sprint streaming, not producer pacing. Framegen restored to ON.

Evidence: `build/sprint-hitch-20261113/headset_pace_trace.json`, `headset_pace_fixed.json`, `headset_fixed_frametimes.json`, `headset_threaded_spikes.json`, `headset_threaded_stacks.json`.

### Follow-up: latency and the 45 fps ceiling

Measured with `CyberpunkVR_DebugSubmitAge*` (capture → `xrEndFrame`) plus Valve compositor timing (frame readiness and the prediction count in `m_nReprojectionFlags`, bits 4–7). Same launch, ABBA, FOCUSED.

**Frames below 45 with framegen ON.** About 1% of Presents spent 10–17 ms inside `OnPresent`. New per-phase counters (`CyberpunkVR_OnPresentPhaseUs`, diagnostics only) placed it in `foveated::Tick`, exactly every 2.0 s. The foveated-DLSS hook installer retried every two seconds on the Present thread; MinHook failed with `MH_ERROR_MEMORY_ALLOC` after scanning the address space near `nvngx_dlss.dll` (VirtualQuery, ~11 ms here). Each retry cost one 45 fps slot. Now at most three attempts per loaded DLSS module, with the failure logged. Result: 0 slow Presents in 1,799 frames (was 20); 44.79–44.94 real fps (was 44.4–44.6), no >33 ms frames. The foveated-DLSS hook itself still cannot be installed in this process (feature is off); that is pre-existing.

**Latency with framegen ON.** The first version of this change generated only from GPU-finished captures on the runtime queue. A/B (`CyberpunkVR_SteamVrFinishedCapture`):

| Framegen ON | Capture → submit | SteamVR prediction | Compositor gaps / 25 s | Real fps |
|---|---:|---:|---:|---:|
| Finished capture only | 35–39 ms | 3.25–3.27 frames | 18–20 | 44.9 |
| Newest capture (old path, now default) | 22–24 ms | 3.42–3.53 frames | 25–28 | 44.9 |

Finished-only added ~16 ms before submit (about half the cycles fell back to the previous frame) for ~2 ms less display pipeline: net ~12 ms worse. Reverted to the newest capture with generation on the game queue, as before this change.

**Latency with framegen OFF.** Threaded with finished captures: 23 ms capture → submit, prediction ~3.0–3.2 frames. Newest capture: 10 ms, but SteamVR raises prediction to ~4.0 frames (+~10 ms) and the compositor misses 26–40 vsyncs per 25 s instead of 5–8. Net ~3 ms for visibly more judder, so finished captures stay the default without generation. The old inline path showed 17 ms capture → submit, yet the compositor registered only ~17 application frames per second (504 in 29 s) — the reported FPS drop with framegen off.

Evidence: `latency_probe.json`, `fg_queue_ab.json`, `fg_on_confirm.json`, `fg_off_ab.json`.

**Drops to 30–39 fps on longer runs.** 12-second sprint legs at H10 (forward 40–80 m and back) still fell to a 1-second minimum of 30–41 fps, with the game needing only 8–14 ms a frame. The long frames were the pacer's own holds (19–23 ms). The adaptive back-off stretched the real-frame interval 4% a second whenever the presenter saw native frames skipped (teleports, loads, skipped captures) and recovered 0.5% of a period a second after 5 s: a pace trace showed 25.24 ms, at which phase rounding put 24–30 of a second's ~33 real frames on a third display period. An interval between whole periods mixes 22 and 33 ms frames on the display grid.

Reference designs lock an application to an exact fraction of the refresh (SteamVR motion smoothing, Meta Application SpaceWarp: half rate); AMD FSR 3 asks for a real frame rate slightly below half the output rate and paces from a moving average of frame times. `CadenceLock` replaces the back-off: exactly two display periods, or exactly three while ≥25% of a one-second window's frames exceed the two-period budget minus 1 ms of the game's own time (release → end of Present); back to two when ≤5% come within 3 ms of it. Hitches of 100 ms and more are ignored; missing neighbours are now a diagnostic counter only (`CyberpunkVR_FramegenPairGaps`; `CyberpunkVR_FramegenThreePeriods` shows the lock).

Same route, two laps, teleport at each lap start (`long_run_cadence.json`): 44.84–44.94 fps per leg, 1-second minimum 44 on every leg (was 30–41), no frame over 33.4 ms, never three periods; 3–19 pair gaps per leg no longer change pacing. After a teleport: 44.8 fps, minimum 44.

### Follow-up: world judder while running (framegen ON)

Metric: the step of content time (the frame's simulation release) against the step of display time (`predictedDisplayTime`) for every XR cycle, from two diagnostic rings (`CyberpunkVR_XrCycleTrace`, `CyberpunkVR_XrStageTrace`; a resent cycle is a row of kind −2). A 30 s run at H10 = three 5 s legs forward and back with a teleport before every forward leg (longer legs took V off the ledge).

**Which frame to take.** ABBA across three headset sessions (Quest 3, Steam Link): newest capture 71–181 content jumps over 5 ms per 30 s with SteamVR waiting for the game's GPU work (25–100 skipped display cycles); finished captures only 15–61, 15–59 skipped. The cost is latency: 33 ms capture → submit at the median against 21 ms (SteamVR null driver, same build). `CyberpunkVR_SteamVrFinishedCapture` is now 3 (finished captures with and without generation); this supersedes the revert above.

**What is left: about 1–2 resent frames a second.** A capture watcher (diagnostics only, `CyberpunkVR_CaptureDoneTrace`) waits on the capture fence and stamps when each frame is done on the GPU. Of 41–52 resent cycles per run, every one was a frame the game had presented 1.5–17 ms before the XR thread took the cycle and the GPU finished 0.7–10 ms after it; none was presented late, none was finished in time and not taken. Present → GPU done is 12–25 ms (p10–p90 12.5–20 ms) with the GPU 85–94% busy, so the spread is wider than one display period.

Tried and not kept:

- XR thread at `THREAD_PRIORITY_HIGHEST`: no change (resends 45–60 vs 45–48). An 8–14 ms "gap after xrEndFrame" seen earlier was a resent cycle missing from the trace, not the thread losing the CPU.
- Present earlier than mid-cycle, fixed (4/8 ms) or from the p95 of Present → done: 13–53 resends against 24–53 at 0; inside the run-to-run spread.
- The same recorded frames shifted −14…+6 ms against the recorded XR cycles: 13–36 resends at every shift, no phase without them. The game's Presents are themselves 20–24.5 ms apart (p10–p90; the pacer holds the release, the game's own work follows), and GPU completion adds its own spread.
- A bounded CPU wait (≤4 ms, high-resolution timer) on the capture fence when a cycle would otherwise resend: headset ABBA, resends 138/121 → 89/118 but content jumps over 5 ms 126/117 → 163/255 and SteamVR vsync gaps 29/26 → 36/37; the later submit costs more than the frames it saves.

**Kept: one finished frame in hand** (`CyberpunkVR_SteamVrFrameBuffer`, default 1). The XR thread takes the older finished frame not shown yet even when the newest is finished too, so real frames go out in order on an even beat. It was first set aside on a replay whose "content time" was the Present time, which itself varies ±2.5 ms; the world moves by simulation time, which the pacer keeps even. Two headset ABBA runs, counted over the seconds the game held 45 fps: resent cycles 0.36–0.76 a second with it against 1.38–2.34 without (four blocks each), capture-step jumps ~2.5 against ~3.3 a second; capture → submit +2.5 ms at the median, +8 ms at p90.

**Still there: seconds at 30 fps.** When the game's own frame time stays over the two-period budget (≥25% of a second's frames), `CadenceLock` runs real frames three periods apart; every third cycle then repeats. In these runs 1–11 seconds of 40 went that way, with and without the buffer, at 82–91% GPU load. That is the game's frame time, not the submission path.

**Seconds at 30 fps were the cadence lock, not the game.** One heavy second (38% of frames over 21.2 ms) moved `CadenceLock` to three periods and it held them seven seconds; during them the game's own time (release → end of Present) was 15.6 ms at the median and 19.9 ms at p90 of a 22.2 ms budget, so the exit rule (≤5% over 19.2 ms) never came true. Pacing is now always two display periods — the rate the headset refresh was chosen for; `xr_framegen_cadence_lock=1` brings the lock back. Two 30 s runs: all 3,499 real frames two periods apart, no 30 fps second.

**Depth on every frame.** Real frames carried a depth layer, generated midpoints none (`Reject::Synthetic`). `XR_KHR_composition_layer_depth`: the runtime *may* reproject with depth; with depth on every other image only, such a runtime treats alternate images differently. A midpoint now gets B's depth moved half of B's motion vectors back toward A (`StereoDepth.cpp`, two fixed-point steps of `s = p − 0.5·mv(s)`; the same vectors the interpolation follows), checked on the GPU in `framegen_stereo_depth`. Live: 269 real and 268 generated submissions in 6 s, all with depth. Long-run ABBA (`CyberpunkVR_SyntheticDepth` 1/0/0/1): 44.1–45.0 fps either way. SteamVR itself only reprojects with depth when `steamvr/depthReprojection` is set (read with no default in vrcompositor; absent from the shipped settings).

**Latency, release → real frame on the headset ≈ 120 ms** (buffer on, p50): game CPU to capture 14 ms, GPU 16 ms, finished frame waiting for its XR take 16 ms, take → submit 4.7 ms, submit → predicted display 58 ms (SteamVR + Steam Link; SteamVR's XrTime is QPC·K/F, the same clock), midpoint before real 11 ms.

**Tried and rejected in the headset: extrapolating late frames.** A cycle whose real frame was late showed the displayed real frame moved forward along its own motion vectors (up to a whole game frame, pose and depth moved alike, the late frame then without a midpoint). By the numbers it worked -- world freezes 0.36–0.46 a second against 0.92–1.28, resends 0.1 against 0.9 (`judder_extrapolate2.json`) -- but it looked bad in the headset, 3–4 such images a second, and was reverted. The resend stays.

**Kept from it: four capture slots** (`kMonoSlots`, `kVrcamEyeSlots`). With three, a Present after a late frame could find every slot taken and its capture was skipped -- a real frame the headset never got, the world two frames ahead at once. With four, `DebugCapSkipNoSlot` stayed 0 over four 30 s runs; real frames never shown went from 9–11 per 30 s (three slots) to 4–10, the rest being frames overtaken by the next.

For comparison, REFramework (praydog, read at c38d1fe) has no submit thread, own queue or frame generation: the game's render thread blocks in `WaitGetPoses` / `xrWaitFrame` at the start of each stereo pair and submits at Present with the game's own command queue (`D3D12TextureData_t::m_pCommandQueue`, `XrGraphicsBindingD3D12KHR::queue`), so SteamVR orders its copy after the game's GPU work and reprojects whatever is late itself. That is the shape of `CyberpunkVR_SteamVrFinishedCapture` bit 0 cleared (newest capture ordered after the game's queue), which measured worst here with our own generation on top of it.

The SteamVR null driver is not a stand-in for these numbers: its compositor runs on the desktop monitor's vsync (7.19 ms steps at 143 Hz) while it reports an 11.1 ms period.

Evidence: `judder_ab.json`, `judder_prio.json`, `judder_lead.json`, `judder_repeats.json`, `judder_adaptive.json`, `judder_final_null.json`, `long_run_null_final.json`.

## Runtime comparison and framegen OFF

The initial runs were actually OpenXR Simulator even though `xr_runtime=1`: the elevated OpenXR loader ignored `XR_RUNTIME_JSON`. Null Driver tests explicitly switched the system runtime and verified `SteamVR/OpenXR : null` in the log. Null Driver has since been disabled and all 13 modified SteamVR keys restored. The system runtime remains SteamVR for the user's physical headset.

The old `xr_stage_replay=0` is **not** the active single-pass switch. The correct live control is `CyberpunkVR_SinglePass`, with engine stereo kept enabled.

Null Driver same-launch ABBA, background sprint:

| Single-pass | Real FPS | Frame p99 |
|---|---:|---:|
| ON | 44.89 | 27.45 ms |
| OFF | 44.90 | 27.24 ms |

No meaningful causal improvement. `xr_gt_skip_render_queue=1` also failed its earlier ABBA (run p99 47.8 vs 34.3 ms), and was restored to 0.

The original framegen-OFF path still set `fgSelected=true` for ordinary stereo frames, accidentally directing copies onto the game queue. SteamVR then waited behind game work inside its D3D11On12 submission. Without generation the SteamVR path now copies finished captures on its runtime queue; with generation it takes the newest capture and orders the runtime queue after the game's (see the latency follow-up above). Submission stays threaded in auto mode regardless of framegen. Virtual Desktop's auto selection is unchanged.

Physical-headset ON/OFF/ON verification before the period-filter fix:

| State | Real FPS | Generated FPS | XR cycles/s | Empty ends |
|---|---:|---:|---:|---:|
| ON | 44.36 | 43.65 | 89.44 | 0 |
| OFF | 57.51 | 0 | 90.00 | 0 |
| ON again | 44.23 | 43.50 | 89.34 | 0 |

With framegen OFF, ~57.5 cycles received fresh images and ~32.5 reused the last submitted layer. HUD remained present. Real FPS and generated/displayed FPS are not interchangeable. Valve compositor timing was read by a background OpenVR client that never submitted images or claimed scene focus; raw timing is retained rather than extrapolated to Steam Link transport latency.

Evidence: `headset_threaded_fg_aba.json`, `headset_compositor_aba.csv`. Settings returned to framegen=1, pace=1, threaded_submit=-1.

## HUD readiness and dropped endpoints

The discarded inline experiment submitted two XR cycles per game Present, but HUD source selection continually returned a just-written, GPU-incomplete canvas. The live readiness check correctly failed with zero HUD submissions; it was not waived. The experiment was removed.

SteamVR can now request the newest **completed** HUD canvas from the bounded producer slots. The shared reference prevents producer reuse while the consumer copies it. Other runtimes keep their existing source selection. A GPU fence-blocking regression test proves selection of the preceding completed canvas and then the newer one after completion.

On SteamVR, an accepted midpoint retains its pinned real endpoint across a transient submit failure. A newer capture cannot replace it, including when generation is disabled; origin/menu reset still clears it. Other runtimes keep the previous reset-on-failure behaviour (`Presenter::Submitted(..., retainEndpoint)` is true only for SteamVR, and the owed-endpoint reuse, producer-fence waits and the no-reset path are gated the same way). The presenter timeline test covers both: SteamVR failure/retry with framegen toggled OFF, and the unchanged non-SteamVR reset.

## Interaction changes — implementation and validation limits

- Right-grip carry initiation is disabled. Recovery for an already-active carry remains. Offline production-gate test passes.
- Slide detection previously considered only rolled and upright wrist candidates. A wrist halfway through a 90-degree cant could be >50 mm from both despite lying on the allowed arc. `grip_arc.lua` adds its nearest bounded candidate without increasing radius or removing the angle gate. Deterministic cases pass. Stock standalone LuaJIT rejects both HEAD and the edited large reload function at its pre-existing 60-upvalue limit; the actual game host loaded the modified module and its arc function successfully. The tilted grab still needs an actual hand/weapon interaction check.
- C++ generator adds 66 adjacent support points for 29 long-gun families and bat/kanabo/hammer/katana. Firearm axes are measured from matching recorded idle frames; melee uses the recorded two-wrist handle direction. Primary rotations/fingers are preserved. `@z_*` sorts after existing `@mag`, so magazine suppression retains index 1. Capacity increased to 256. Live loaded counter: 136. Extracted production C++ parser/selector: 136 loaded, 49 non-alias primaries, 134 reachable candidates. Data integrity tests pass; arbitrary mesh clearance has not been visually validated.
- Vehicle minimap: car-only transparent luminous style with a perspective warp and a faint frosted combiner (alpha ≤0.13, soft edges), cabin-anchored. Non-car HUD keeps the original style. Entry/exit wait for matching style metadata.
  - First placement (wheel +0.35 m forward, +0.40 m up) measured live at 0.83 m and **+12.1°** above eye level: on the upper glass, overlapping the roof liner and visor (`windshield-focused-left-2-small.jpg`), unlike the reference photo.
  - Now placed like an automotive HUD virtual image: wheel +0.92 m forward, +0.14 m up, 48×29 cm (≈19.5°×12°). Live Porsche, FOCUSED: 1.405 m away, **−3.1°**, 4 mm lateral offset from the driver axis; the map sits over the road above the hood in front of the driver (`windshield-hud-v2-left.jpg`, `windshield-hud-v2-crop.jpg`).
  - GPU shader test checks the frosted combiner as premultiplied in linear space through the sRGB canvas; placement tests check the image stays below eye level and beyond 1 m. Test car deleted, player position restored.

Final background sprint ABBA (`headset_final_sprint_fg_abba.json`, dashboard qualification above): framegen ON 44.83 real FPS / p99 26.98 ms / max 31.60 ms; OFF 57.50 / 23.09 / 25.74 ms; no >33 ms frames in either run sample. Framegen restored to ON, test stick zero, player returned to H10 start.

## Automated checks so far

- Framegen: 9 selected policy/pacing/presenter tests, including `presentation_period` and `cadence_lock`, passed.
- HUD: 41 tests including `hud_windshield` (frosted combiner) and completed-source regression passed.
- HUD submit ring / vehicle placement: passed.
- Carry initiation, support-file integrity, and grip-arc deterministic tests: passed.
- Release build/deploy: passed.

## Startup crash encountered during final visual validation

The first launch after the interaction/minimap build crashed before world load. No blind relaunch loop was used. Report: `Cyberpunk2077-20261113-145647-4352-17688` under REDEngine ReportQueue.

Exception: read null at game RVA `0x2BF14E`, `scnVoicesetComponent`, dereferencing its missing `gameIStatusEffectSystem` pointer at +0x288. Confirmed from exception context and IDA; no plugin frame in the faulting unwind. This establishes the fault site, not the upstream cause or independence from the changes. No audio-system patch was made. One diagnostic repeat after inspection succeeded and reached world-ready with a fresh HUD (PID 25740). The startup fault's upstream cause remains unproven.

IDA exports are under `build/sprint-hitch-20261113/re`; IDA database was not saved.

## Primary references

- Khronos OpenXR frame synchronization: https://registry.khronos.org/OpenXR/specs/1.1/html/xrspec.html#frame-synchronization
- D3D12 image state and queue availability: https://registry.khronos.org/OpenXR/specs/1.1/html/xrspec.html#XR_KHR_D3D12_enable
- Valve compositor timing fields (OpenVR diagnostics, not OpenXR call-order rules): https://github.com/ValveSoftware/openvr/wiki/Compositor_FrameTiming
- Valve compositor overview: https://github.com/ValveSoftware/openvr/wiki/IVRCompositor_Overview
