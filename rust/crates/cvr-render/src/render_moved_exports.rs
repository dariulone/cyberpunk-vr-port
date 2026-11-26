//! The exported state of the remaining sources whose exports no C++ code of the Rust build reads any more (view reuse,
//! grading, the frame graph, the OpenXR frame loop and manager, the capture, the node dispatch, DLSS, the overlay and
//! others; each section names its source): defined on the Rust side, with the notes that stood over them, and exported
//! from rust/exported-data.txt; the Rust modules and the tools read them by name.
#![allow(non_upper_case_globals)]
use core::sync::atomic::{AtomicI32, AtomicI64, AtomicU32, AtomicU64, AtomicUsize};

// ---- include/Render/ObservationProfile.hpp (formerly inline header definitions)

/// The observation profile's opt-in switch (descriptor_audit.rs reads it).
#[no_mangle]
pub static CyberpunkVR_ObservationProfileEnable: AtomicU32 = AtomicU32::new(0);
/// Per phase: the count and the inclusive QPC ticks.
#[no_mangle]
pub static CyberpunkVR_ObservationProfileStats: [AtomicU64; 16] = [const { AtomicU64::new(0) }; 16];

// ---- src/Camera/ImagePoseIdentity.cpp

#[no_mangle]
pub static CyberpunkVR_PoseIdImageWrites: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];
#[no_mangle]
pub static CyberpunkVR_PoseIdCaptured: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];
#[no_mangle]
pub static CyberpunkVR_PoseIdCaptureMiss: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];
#[no_mangle]
pub static CyberpunkVR_PoseIdLastCapture: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];
#[no_mangle]
pub static CyberpunkVR_PoseIdLastSubmit: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];
#[no_mangle]
pub static CyberpunkVR_PoseIdImageGeneration: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];

// ---- src/Core/LiveControlsPoll.cpp

/// xr_stage_replay: MAIN replays VRCAM's recorded RenderElements stages with its own camera instead of
/// recording them (src/Render/StageReplay.cpp, docs/single-pass-stage-replay-20261008.md). Applied
/// only when the setting itself changes, so an unrelated ini edit never resets live test controls.
#[no_mangle]
pub static mut CyberpunkVR_StageReplaySetting: i32 = 0;
/// xr_vrcam_gbuffer_partitions: the second eye records its G-buffer in worker partitions like MAIN
/// does (src/Render/StageReplay.cpp, ApplyPartitions). Independent of xr_stage_replay; the engine
/// clamps a render-to-texture view to one partition, a 2 ms single-thread job on the frame's
/// critical path in the city.
#[no_mangle]
pub static mut CyberpunkVR_VrcamGbufferPartitionsSetting: i32 = 1;
/// xr_scope_gbuffer_partitions: the same for the camera scope's view. Off by default: the scope's
/// G-buffer is short (0.3 ms), and a view whose cost is its passes' serial stage latencies gets slower
/// with more passes -- 18.3 against 17.8 ms per frame with a Grad drawn (Pacifica, uncapped).
#[no_mangle]
pub static mut CyberpunkVR_ScopeGbufferPartitionsSetting: i32 = 0;
/// xr_submit_chain: each render-graph submission point gets its own job-stage chain, so a graph's
/// Synchronize nodes no longer wait for the previous batch's ExecuteCommandLists (src/Stereo/SubmitChain.cpp,
/// docs/gpu-not-full-20261016.md). 0 off, 1 view jobs and Present wait for the chain, 2 only Present
/// (MAIN's graph starts while VRCAM's last batches are still being submitted, -0.4 ms
/// against 1 in the city). Default 1: on 2.5 with a camera scope drawn (Megabuilding H10, same launch) the
/// frame went 23.85 -> 21.47 ms with the deferred present, the scope view's Synchronize nodes no longer
/// waiting for their Submit; holstered (game-thread bound) no change. Applied only when the setting
/// changes, so a live CyberpunkVR_SubmitChain test value survives ini edits.
#[no_mangle]
pub static mut CyberpunkVR_SubmitChainSetting: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_DeferPresentSetting: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_GtSkipRenderQueueSetting: i32 = 0;

// ---- src/Framegen/Inputs.cpp

#[no_mangle]
pub static CyberpunkVR_FramegenInputStages: [AtomicU64; 10] = [const { AtomicU64::new(0) }; 10];
/// The view camera against Streamline's for the same eye and frame, when both exist (NVIDIA): largest
/// difference in position (m), axes, near/far, vertical FOV (rad), jitter, motion scale.
#[no_mangle]
pub static mut CyberpunkVR_FramegenCameraDelta: [[f32; 6]; 2] = [[0.0; 6]; 2];

// ---- src/Framegen/NativeInputs.cpp

#[no_mangle]
pub static CyberpunkVR_FramegenNativeStages: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];
#[no_mangle]
pub static CyberpunkVR_FramegenNativeDebug: [[AtomicU64; 8]; 2] = [const { [const { AtomicU64::new(0) }; 8] }; 2];
/// Which engine node the last native capture came from: 1 TAA, 2 FSR 2, 3 FSR 3.0, 4 FSR 3.1+, 5 XeSS.
#[no_mangle]
pub static CyberpunkVR_FramegenNativeSource: AtomicU32 = AtomicU32::new(0);

// ---- src/Hooks/InputServices.cpp

/// The cyberware chord's sent keys (src/Hooks/CyberwareChord.cpp, or rust/crates/cvr-rig/src/vrik/cyberware_chord.rs).
#[no_mangle]
pub static CyberpunkVR_CyberwareChordSent: AtomicU64 = AtomicU64::new(0);

// ---- src/Hooks/LeftHandedAnimation.cpp

/// Single-wield graphs attached for the left hand (diagnostics).
#[no_mangle]
pub static CyberpunkVR_LeftHandedGraphsAttached: AtomicU32 = AtomicU32::new(0);

// ---- src/Hooks/LeftHandedMirror.cpp

/// Names mirrored so far (diagnostics).
#[no_mangle]
pub static CyberpunkVR_LeftHandedNamesMirrored: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_LeftHandedNameLogCount: AtomicU32 = AtomicU32::new(0);

// ---- src/Hooks/LodFov.cpp

#[no_mangle]
pub static mut CyberpunkVR_DebugBdHeadWrites: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugBdViewSeen: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugBdViewByFov: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugBdFovWrites: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugFixLodCoverage: u32 = 0;

// ---- src/Hooks/MenuMode.cpp

#[no_mangle]
pub static CyberpunkVR_MenuSourceSeq: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_MenuMethodsLength: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_MenuMethods: [u8; 32768] = [0; 32768];

// ---- src/Hooks/MenuModeCallback.cpp

#[no_mangle]
pub static CyberpunkVR_MenuTransitionSeq: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_MenuTransitionCount: AtomicU32 = AtomicU32::new(0);
/// The transitions (MenuTransition: tick, state, previous, requested, native before, thread -- 32 bytes each), written
/// by rust/crates/cvr-rig/src/vrik/menu_mode.rs and read by tools/single_pass_tests/read_startup_evidence.py.
#[no_mangle]
pub static mut CyberpunkVR_MenuTransitions: [[u64; 4]; 64] = [[0; 4]; 64];

// ---- src/Hooks/OnFootDeltaHead.cpp

/// Peak |heading delta| seen within 150 ms of a shot, degrees. Non-zero means the game kicks the
/// camera through this channel. Physical body motion never enters this input.
#[no_mangle]
pub static mut CyberpunkVR_DebugShotHeadingKickDeg: f32 = 0.0;

// ---- src/Hooks/PhotoModeCapture.cpp

#[no_mangle]
pub static CyberpunkVR_PhotoCaptureSizeOverrides: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_PhotoCaptureClampOverrides: AtomicU64 = AtomicU64::new(0);

// ---- src/Hooks/SeamlessLoading.cpp

/// 0 = the loading screen waits for its button, as the game has it.
#[no_mangle]
pub static mut CyberpunkVR_SeamlessLoading: i32 = 1;
/// Loadings that went on without the button.
#[no_mangle]
pub static CyberpunkVR_DebugSeamlessLoads: AtomicU64 = AtomicU64::new(0);

// ---- src/Hooks/StoryAttention.cpp

#[no_mangle]
pub static mut CyberpunkVR_StoryAttentionHookReady: i32 = 0;
#[no_mangle]
pub static CyberpunkVR_StoryAttentionCounters: [AtomicU64; 5] = [const { AtomicU64::new(0) }; 5];
/// gated: player calls / Pacifica / workspot / unavailable pose / explicit clue presses
/// gated: look-at rays turned for a hand
#[no_mangle]
pub static CyberpunkVR_ReachTurns: AtomicU64 = AtomicU64::new(0);

// ---- src/Hooks/SurveillanceBody.cpp

#[no_mangle]
pub static mut CyberpunkVR_SurveillanceBodyFollow: i32 = 1;
#[no_mangle]
pub static CyberpunkVR_SurveillanceBodyUpdates: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_SurveillanceBodyInjected: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_SurveillanceBodyDebugSeq: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_SurveillanceBodyDebug: [f32; 9] = [0.0; 9];
#[no_mangle]
pub static CyberpunkVR_SurveillanceAutoFiltered: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static mut CyberpunkVR_SurveillanceAutoDebug: [f32; 6] = [0.0; 6];

// ---- src/Hooks/SwapChain.cpp

/// The rest of the hook, in QPC ticks summed over presents: [0] the per-frame polls before the
/// overlay, [1] OverlayRender, [2] OpenXRManager::OnPresent (HUD + mono capture), [3] rate report and
/// desktop overlay, [4] DesktopMirrorRender, [5] after the DXGI Present. The Present is the last node of
/// the render chain, so every tick here is a tick of frame time in a CPU-bound scene.
#[no_mangle]
pub static CyberpunkVR_PresentSections: [AtomicU64; 8] = [const { AtomicU64::new(0) }; 8];

// ---- src/Natives/EntityAttach.cpp

/// [0] attaches made, [1] detaches made, [2] attaches refused (no root, a root already attached, not a placed
/// component), [3] calls that faulted.
#[no_mangle]
pub static CyberpunkVR_DebugEntityAttach: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];

// ---- src/Natives/HudPanel.cpp

/// 1 (default): a HUD window with nothing visible under its root (HasVisibleContent) is not drawn,
/// and a channel with only such windows builds no canvas and submits no layer. 0 draws everything.
#[no_mangle]
pub static CyberpunkVR_HudSkipEmpty: AtomicU32 = AtomicU32::new(1);
#[no_mangle]
pub static CyberpunkVR_HudEmptySprites: AtomicU64 = AtomicU64::new(0);
/// VRHudPanelUpdate cost, QPC ticks: [0] calls, [1] whole call, [2] Geometry, [3] Texture, [4] HasVisibleContent,
/// [5] separate-window entries walked, [6] masks and publish after the walk, [7] entries that became sprites.
#[no_mangle]
pub static CyberpunkVR_HudPanelTiming: [AtomicU64; 8] = [const { AtomicU64::new(0) }; 8];
/// 1 (default): the per-frame upkeep of the HUD roots modules/hud_panel.lua finds once a second (crop bounds,
/// window size, repaint, the loot test) runs here inside VRHudPanelUpdate; the script only hands the roots
/// over. 0: the script does it, as before (A/B; the script switches at its next scan).
#[no_mangle]
pub static CyberpunkVR_HudRootsNative: AtomicU32 = AtomicU32::new(1);
/// [0] frames with upkeep, [1] roots walked, [2] of them active, [3] crop size writes, [4] crop offset writes,
/// [5] window SetSize calls, [6] SetSize calls that failed, [7] subtitle crop writes.
#[no_mangle]
pub static CyberpunkVR_HudRootsStats: [AtomicU64; 8] = [const { AtomicU64::new(0) }; 8];
/// 1 (default): the upkeep's repaints run at the frame submit (see REPAINTS AT THE FRAME SUBMIT). 0: at once.
#[no_mangle]
pub static CyberpunkVR_HudRepaintAtSubmit: AtomicU32 = AtomicU32::new(1);
/// [0] repaints queued, [1] run at the frame submit, [2] run late in the next update (no frame submit between).
#[no_mangle]
pub static CyberpunkVR_HudRepaintStats: [AtomicU64; 3] = [const { AtomicU64::new(0) }; 3];

// ---- src/Natives/KeypadInput.cpp

#[no_mangle]
pub static mut CyberpunkVR_KeypadRoot: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_KeypadLayer: u64 = 0;
#[no_mangle]
pub static CyberpunkVR_KeypadHeadHits: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_KeypadHeldHits: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static mut CyberpunkVR_KeypadCursor: [f32; 2] = [0.0; 2];

// ---- src/Natives/MenuSurface.cpp

/// Allocations, native updates, readiness polls, armed polls, recorded writes,
/// submitted producer fences, completed surfaces, currently retained surfaces.
#[no_mangle]
pub static CyberpunkVR_MenuSurfaceStats: [AtomicU64; 8] = [const { AtomicU64::new(0) }; 8];

// ---- src/Natives/NativeState.cpp

/// Where the hand frame takes the view quaternion from. 1 = the view PACKET (seqlock [143]),
/// the same latch the time-align's headOri_view comes from, so the re-basing cancels exactly.
/// 0 = the hands snapshot's copy of shared [104..107], which is what it was: a second capture of
/// the same publisher, refreshed on the HANDS' cadence, leaving the rotation between two view
/// publications in the hand frame -- visible only while the head turns.
#[no_mangle]
pub static mut CyberpunkVR_VrikHandFrameOneLatch: i32 = 1;

// ---- src/Natives/OrientationProvider.cpp

/// The values the harness and the rest of the port read by name: defined here in both builds (the Rust build's
/// work is rust/crates/cvr-rig/src/vrik/provider.rs).
#[no_mangle]
pub static mut CyberpunkVR_DebugProvEntFunc: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_DebugAtkCalls: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugAtkNoMatch: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugAtkMask: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPosProvCalls: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPosProvOverrides: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_WeaponName: [u8; 64] = [0; 64];
#[no_mangle]
pub static mut CyberpunkVR_DualWieldRecords: [u32; 2] = [0; 2];
#[no_mangle]
pub static mut CyberpunkVR_SightZoom: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_SightApproach: [f32; 3] = [0.0, 0.0, 0.0];

// ---- src/Natives/Performance.cpp

/// Diagnostic A/B only. Returning true retains the original Lua scan path.
#[no_mangle]
pub static mut CyberpunkVR_BodyCapsulePrefilter: i32 = 1;

// ---- src/Natives/UiPopup.cpp

#[no_mangle]
pub static mut CyberpunkVR_PopupVisualStateFilter: i32 = 1;
/// [0] requests held back, [1] restores swallowed with them, [2] calls passed to the game
#[no_mangle]
pub static mut CyberpunkVR_PopupVisualStateStats: [u64; 3] = [0; 3];

// ---- src/Overlay/ImGuiOverlay.cpp

/// THE OVERLAY IS RUST'S (rust/crates/cvr-render/src/imgui_overlay.rs): the context and its fonts, the frame, the
/// settings mirror on the desktop, the second eye, the pacing, the window procedure. What stays here: the exports the
/// tools read and write, the debug drawing's switches (Overlay/OverlayDebugDraw.cpp and the settings panels use them),
/// what Rust asks of this half -- the game font, the world-space debug drawing, the Lua host -- and the entry points.
#[no_mangle]
pub static mut CyberpunkVR_OverlayPacing: i32 = 3;
#[no_mangle]
pub static mut CyberpunkVR_DebugOverlayWaitMs: f64 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_DebugOverlayDrains: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugOverlayWaitTimeouts: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugOverlayGuardArms: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugOverlayEmptyFrames: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugOverlaySubmissions: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugOverlayWaits: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugOverlayWaitTotalUs: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_ImGuiDump: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_ImGuiDumpTabs: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_OverlaySecondEye: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_OverlaySecondEyeDistM: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_DebugOverlaySecondEyeDraws: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugOverlaySecondEyeSkips: u64 = 0;

// ---- src/Render/Afw.cpp

/// Diagnostics: [0] frames warped, [1] fallbacks, [2] plugin state (0 not tried, 1 loaded, -1 failed); of the warped
/// frames whose renderer frame directly follows the last warped one, [3] drawn for the other eye than that one, [4] for
/// the same eye; [5] warped frames after a gap in the renderer's frames.
#[no_mangle]
pub static CyberpunkVR_AfwStats: [AtomicI64; 6] = [const { AtomicI64::new(0) }; 6];
/// Tuning, read every frame (tools write it into the running game): [0] the warp mode (pd::FrameWarpMode), [1] the
/// motion vectors' type (pd::MVType; 3 = from the frame before: Normal after a frame drawn for the same eye, else
/// FromOtherEye), [2] IgnoreMotionThreshold x100, [3] the plugin's debug view, [4] clear before
/// warping. [5] motion vectors off (InMotionScale 0). [6] the reconstruction: 0 PureDark's plugin, 1 the port's own
/// OSR (Render/Osr.hpp; also whenever the plugin is not there), the default: on still frames 35.7 against the plugin's
/// 33.8 dB at the worst and 37.8 against 36.3 on average, at 0.44 against 1.59 ms of GPU (docs/osr-20261104.md).
/// [7] OSR's depth jitter: 0 subtracted (the measured best), 1 added, 2 none.
#[no_mangle]
pub static mut CyberpunkVR_AfwTune: [i32; 8] = [3, 3, 250, 0, 0, 0, 1, 0];
/// Extra factor on the motion vectors' scale, x and y (-1 flips one).
#[no_mangle]
pub static mut CyberpunkVR_AfwMotionScale: [f32; 2] = [1.0, 1.0];

// ---- src/Render/FoveatedDlss.cpp

#[no_mangle]
pub static mut CyberpunkVR_FoveatedReport: [u8; 32768] = [0; 32768];
#[no_mangle]
pub static CyberpunkVR_FoveatedReportSeq: AtomicU32 = AtomicU32::new(0);

// ---- src/Render/NativeSubmissionFrontier.cpp

#[no_mangle]
pub static mut CyberpunkVR_SubmissionFrontierRangeBytes: u32 = 192;
#[no_mangle]
pub static mut CyberpunkVR_SubmissionFrontierSlotBytes: u32 = 160;
#[no_mangle]
pub static mut CyberpunkVR_SubmissionFrontierSubmitBytes: u32 = 128;

// ---- src/Render/SinglePassPipelines.cpp

#[no_mangle]
pub static CyberpunkVR_SinglePassPipelineCounts: [AtomicU64; 12] = [const { AtomicU64::new(0) }; 12];
#[no_mangle]
pub static mut CyberpunkVR_SinglePassError: [u8; 256] = [0; 256];
/// 0: no material pipeline is registered (and nothing else of single pass runs); read once, at the first root.
#[no_mangle]
pub static CyberpunkVR_SinglePassPipelines: AtomicU32 = AtomicU32::new(1);

// ---- src/Runtimes/HudQuad.cpp

#[no_mangle]
pub static CyberpunkVR_HudPoseDebugSeq: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_HudSubmittedFrames: AtomicU64 = AtomicU64::new(0);
/// Internal A/B: 0 preserves the prior per-panel fence wait; 1 rotates fenced
/// command allocators and reuses the last published HUD when all slots are busy.
#[no_mangle]
pub static CyberpunkVR_HudSubmitRing: AtomicU32 = AtomicU32::new(1);

// ---- src/Runtimes/OpenXRCapture.cpp

/// How far away the composited HUD should sit, in metres. 0 = leave it at infinity, which is what
/// pasting it at the same pixel in both eyes amounts to. Live-tunable rather than a vrport.ini
/// setting because the comfortable value is personal and worth sweeping in the headset.
///
/// DEFAULT IS 0: NO SHIFT. It was 1.8 m, and at that distance the second eye's HUD is moved by
/// IPD/1.8 radians -- which is correct as optics and wrong as a picture on this headset: markers and
/// labels sit at visibly different places in the two eyes, which is what the disparity IS. The
/// mechanism stays, gated on this value, because the finding that produced it is real and belongs to a
/// different headset:
///
/// at zero disparity a Pimax tester reported "both images too far to the sides to converge" --
/// optical infinity means every icon splits by the full vergence angle when the eyes are focused an
/// arm's length away.
///
/// So this is a per-headset preference with two defensible settings, not a bug with a fix. Set it to
/// 1.8 (or sweep it) to get the finite-distance HUD back; leave it at 0 for identical placement in both
/// eyes. The shift code below reads it every frame, so it can be moved live from the debugger.
#[no_mangle]
pub static mut CyberpunkVR_HudDistanceM: f32 = 0.0;
/// THE SECOND EYE WITHOUT A COMPOSITE IS A COPY. When no HUD is composited into eye 1, ColorBlit's
/// plain blit samples the _UNORM_SRGB snapshot (decoded to linear) into a _UNORM_SRGB target
/// (encoded back) at the same size -- an identity, apart from forcing alpha to 1, which the
/// projection layer ignores (no SOURCE_ALPHA flag; eye 0 is a CopyResource and keeps its alpha the
/// same way). The GPU stage profile measured that pass at 134 us per frame against 34 us for
/// eye 0's copy (run 834). 1 = CopyResource when size and the 8-bit RGBA family match, 0 = blit.
#[no_mangle]
pub static mut CyberpunkVR_SecondEyeCopy: i32 = 1;

// ---- src/Runtimes/OpenXRFrameLoop.cpp

/// Run the XR cycle on every display frame and let xrWaitFrame pace it, re-submitting the
/// last snapshot with ITS OWN pose when the game has not produced a new one. 0 restores the
/// old behaviour (block until a fresh game frame, skipping XR frames entirely).
///
/// This matches how the established injected-VR mods structure it: fholger's crysis_vrmod
/// runs AwaitFrame (xrWaitFrame -> xrBeginFrame -> xrLocateViews) -> game renders ->
/// FinishFrame (xrEndFrame) once per frame and never skips the cycle.
/// The invariant is that the pose in the projection
/// layer is the one the image was rendered against -- not one located at submit time.
#[no_mangle]
pub static mut CyberpunkVR_XrPaceByRuntime: i32 = 1;
/// 1 = the arm solve is clocked by the engine's ANIMATION BATCH, found by the gap between pose-apply
/// passes. 0 = the old clock, one Present. Measured reason for the change: at 84-86 fps freshSolve
/// matched present exactly, but at 79.5 it was 77.0 and at 67.5 it was 63.0 -- so 3-7% of displayed
/// frames replayed the previous arm while the world was drawn from a new camera. That is phase, not
/// rate: the epoch was bumped in Present, and whenever two animation batches fell between two Presents
/// the second had already been solved for.
#[no_mangle]
pub static mut CyberpunkVR_VrikBatchClock: i32 = 1;
/// 1 = a hand offset is measured from the FILTERED head, the one the solve re-anchors it on. 0 = the
/// raw located head, which is what it used to be. See the long note at the hand loop: the product
/// camModelRot * inverse(headOri) is what rotates the arm, and with two different heads in it that
/// product wobbles by their disagreement, amplified by the arm's half-metre reach.
/// 1 = the hand lerp runs on the controller in WORLD space, then the offset is re-localised on
/// the head. 0 = it runs on the head-local offset, which is what it did: in that frame a head
/// rotation is indistinguishable from hand motion, so the filter smoothed the head turn and
/// left (head rate x arm length x tau) of hand displacement -- about 7 cm at 200 deg/s with the
/// shipped 33 ms constant, and only while the head turns.
#[no_mangle]
pub static mut CyberpunkVR_HandFilterInWorld: i32 = 1;
/// THE OPENXR SIMULATOR'S CONTROLLERS POINT; A REAL CONTROLLER'S GRIP DOES NOT. The simulator builds each
/// controller from a yaw and a pitch and reports that one orientation for grip AND aim, so its grip -Z runs
/// along the pointing direction. On a real controller the grip -Z runs through the closed fist, pinky to thumb,
/// which a natural hold tilts well up from where the controller points -- and the hands' wrist base
/// (Runtimes/WristBase.hpp) was tuned on real controllers. Fed the simulator's pose unchanged, every hand came
/// out flexed about 90 deg down: measured, forearm -> hand 94.9 deg (left) and 88.7 (right) with the
/// controllers at their default 17 deg below level. So, on the simulator only, the grip is turned up about its
/// own X (lateral for both hands in OpenXR's grip convention) by this many degrees. Read every frame, so it can be
/// tuned live; 0 turns the correction off. Real runtimes never see it.
#[no_mangle]
pub static mut CyberpunkVR_SimGripPitchDeg: f32 = 75.0;
/// ...and a ROLL per hand (left, right), which the simulator cannot give a controller at all: it carries a yaw
/// and a pitch and nothing else. Degrees about the controller's own pointing axis, applied before the pitch
/// above. 0 by default; tests set it live to turn a gun on its side -- a pump's loading port to the camera.
#[no_mangle]
pub static mut CyberpunkVR_SimGripRollDeg: [f32; 2] = [0.0, 0.0];
/// Display cycles driven versus frames actually submitted; the difference is the freeze.
#[no_mangle]
pub static mut CyberpunkVR_DebugXrCycles: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMonoSubmits: u64 = 0;
/// Of those, the submits that carried a game frame not shown before; cycles minus this are repeats.
#[no_mangle]
pub static mut CyberpunkVR_DebugFreshSubmits: u64 = 0;
/// How many serials old an own-eye image may be before it is not worth showing. 0 restores the old
/// fall-through to MAIN's picture. Three is the pool depth, so this cannot reach past what the pool holds.
#[no_mangle]
pub static mut CyberpunkVR_VrcamEyeReuseMax: u32 = 3;
#[no_mangle]
pub static CyberpunkVR_XrReuseImages: AtomicU32 = AtomicU32::new(1);
/// ---- WHAT INSTANT THE POSE IS AIMED AT ------------------------------------------------------
///
/// 0 = this cycle's predictedDisplayTime (what it always was).
/// 1 = predictedDisplayTime + period * PosePredictScale -- a constant offset (the UEVR shape).
/// 2 = the measured fit: predict the display time of the frame the game is
/// building right now from the rolling regression, and locate everything at that.
///
/// 2 is the default because there is no constant to pick: the XR loop is paced by xrWaitFrame at
/// the headset rate on its own thread while the game presents at its own, so the offset between
/// "the cycle that located this pose" and "the cycle that shows the frame built from it" drifts
/// continuously. Mode 2 falls back to mode 1's arithmetic until the fit has enough samples.
#[no_mangle]
pub static mut CyberpunkVR_PosePredictMode: i32 = 2;
#[no_mangle]
pub static mut CyberpunkVR_PosePredictScale: f32 = 1.0;
/// How much of the measured frame-ahead offset to actually lead by, 0..1.
///
/// 1 = aim the render camera at its measured real presentation slot, with a bounded/slewed horizon.
/// 0 retains the current XR-cycle target for diagnostic A/B. A correct submitted pose label does not
/// compensate for missing edge pixels when a camera was rendered for an earlier display interval.
#[no_mangle]
pub static mut CyberpunkVR_PoseAimScale: f32 = 1.0;
/// Last target calculation: XR display time, camera target time, fitted time, building serial, valid fit.
#[no_mangle]
pub static mut CyberpunkVR_PoseTargetDebug: [i64; 5] = [0; 5];
#[no_mangle]
pub static mut CyberpunkVR_StereoDepthReport: [u8; 768] = [0; 768];
#[no_mangle]
pub static CyberpunkVR_StereoDepthReportSeq: AtomicU32 = AtomicU32::new(0);
/// 1 = re-enable the adaptive head-pose smoother (xr_hmd_smooth). Off by default; see the use
/// site for why a motion-dependent filter on the VIEW pose is judder rather than smoothing.
/// Live-switchable so the two can be compared inside one session.
/// 3 = the same fixed time-constant lerp the view path uses (CyberpunkVR_PoseLerpSpeed), so the
/// hands' head reference and the view lag identically. See the use site.
#[no_mangle]
pub static mut CyberpunkVR_HeadFilter: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugFitSlopeUs: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugFitUsed: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugFitMissed: u64 = 0;
/// Collapse the two submitted eye poses to one orientation and the midpoint position whenever both
/// eyes are shown a single image. On a canted headset the runtime's per-eye orientations differ, and
/// anchoring one image at both of them pushes the copies apart in the direction the eyes cannot
/// follow. 1 = collapse, 0 = submit the runtime's pair.
#[no_mangle]
pub static mut CyberpunkVR_MonoCyclopeanPose: i32 = 1;
/// HOW FAR THE RIGHT STICK MUST GO IN THE D-PAD CHORD. To the stop, like every other gesture in
/// this port: the snap turn, the sprint detent, the crouch and the dash all fire at 0.90, and for
/// the reason stated there -- a thumb resting on the stick, or a wrist drifting while walking, must
/// not step a list. It was 0.5, which is half a push and reachable by accident.
#[no_mangle]
pub static mut CyberpunkVR_DpadChordStick: f32 = 0.90;
/// How far away flat content should sit when both eyes are shown the same image -- the intro, the
/// menus, and any frame the second view could not fill. 0 leaves it at infinity, which is what
/// submitting one image to two eye poses amounts to and what makes it double. Metres.
///
/// OFF, and it should have been from the start. Twice now it has been the visible regression:
/// first rotating one eye and not the other (the probe caught 1.130 deg on the right eye alone),
/// and then, with that gate fixed to "both eyes or neither", shifting the whole world sideways --
/// "one eye is to the right of the other".
///
/// The second one is the fundamental objection, not a bug in the gate. "Both eyes share one image"
/// is true of the intro logo, which is flat and wants a distance -- and equally true of GAMEPLAY
/// whenever the second view has not produced a frame, where the image is a real 3D scene at every
/// depth at once. Converging that by ipd/distance is a flat 2.26 deg horizontal offset between the
/// eyes, applied to a world that already had its own disparity. There is no signal here that tells
/// the two cases apart, and the mono fallback is common precisely while the second eye is
/// misbehaving -- so the one moment this would fire in gameplay is the worst one for it.
///
/// A doubled intro logo is a far smaller price. Left tunable for anyone who wants to try it on a
/// build where the second eye is solid.
#[no_mangle]
pub static mut CyberpunkVR_FlatDistanceM: f32 = 0.0;

// ---- src/Runtimes/OpenXRManager.cpp

/// The camera uses the runtime's predicted pose without an additional low-pass filter. A filtered
/// pose can be labelled correctly and still require more reprojection than the image covers.
/// At 300 deg/s the former 30/s filter added roughly 10 degrees of uncovered rotation.
///
/// 0 = raw (default). Preserve the runtime's prediction and avoid adding motion latency.
/// 1 = adaptive follow. Kills the rest jitter and buys judder in MOTION instead, because the
/// follow factor -- and therefore the lag -- is a function of how fast the head is moving.
/// 2 = dead band. Removes the rest jitter without a motion-dependent lag, but quantises: measured
/// 92 % held / 45 steps in 8 s on the orientation, i.e. ~5.6 discrete 0.05 deg kicks per
/// second, and the eye reads rare discrete steps far more readily than continuous noise of
/// the same amplitude. Kept for comparison; it was worse to look at than 1.
/// 3 = fixed time-constant lerp, retained for diagnostics. Reprojection can correct its orientation
/// only where the rendered image has coverage; it cannot fill the newly exposed borders.
#[no_mangle]
pub static mut CyberpunkVR_PredictFilter: i32 = 0;
/// 1 = do not locate here at all; take the pose the XR cycle already located right after
/// xrWaitFrame (GetCycleHeadPoseLocal). That is the Crysis/Far Cry arrangement and it makes the
/// prediction horizon a constant instead of a 0..14.5 ms lottery. Off by default so the lerp can be
/// judged on its own first -- two changes at once and a verdict means nothing.
#[no_mangle]
pub static mut CyberpunkVR_PoseFromCycle: i32 = 0;
/// Dead-band thresholds for mode 2. Defaults sit just above the measured extrapolation noise
/// (0.30..0.33 mm per read at rest) and below anything a head does deliberately.
#[no_mangle]
pub static mut CyberpunkVR_DeadbandPosMm: f32 = 0.80;
#[no_mangle]
pub static mut CyberpunkVR_DeadbandAngDeg: f32 = 0.05;
/// How often the band held versus stepped -- the numbers that say whether a threshold is sane.
#[no_mangle]
pub static mut CyberpunkVR_DebugDeadbandHeldPos: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugDeadbandHeldOri: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugDeadbandStepPos: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugDeadbandStepOri: u64 = 0;
/// ---- HOW FAR THE PREDICTION HORIZON WANDERS ---------------------------------------------------
///
/// The reference ports (Crysis VR, Far Cry VR) locate the head ONCE per frame, immediately after
/// xrWaitFrame, and hand that one struct to everything -- render and submit alike. The horizon
/// (aim - now) is therefore the same every frame, so the runtime's extrapolation noise is a
/// CONSTANT offset and reads as steady, not as jitter. Neither mod filters the view pose at all.
///
/// Here the locate happens inside a camera hook on an engine job thread, at whatever sub-frame
/// phase that job lands on, so the horizon breathes -- and the noise term breathes with it. These
/// counters measure exactly that spread, in microseconds since SetFrameAimTime stamped the cycle.
/// A tight distribution refutes the theory; a wide one says the fix belongs at the phase, not in
/// a filter. Read live off the DLL's exports, no rebuild needed.
#[no_mangle]
pub static mut CyberpunkVR_DebugAimLagLastUs: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugAimLagMinUs: u32 = 4294967295;
#[no_mangle]
pub static mut CyberpunkVR_DebugAimLagMaxUs: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugAimLagSumUs: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugAimLagCount: u64 = 0;
/// 2 ms bins; the last bin collects everything at or past 22 ms.
#[no_mangle]
pub static mut CyberpunkVR_DebugAimLagHist: [u32; 12] = [0; 12];
/// Debug: GetRoomscaleSample's neck (frame x, y; runtime x, y), eye lever (x, y), head yaw, eye (x, -z)
#[no_mangle]
pub static mut CyberpunkVR_DebugRoomscaleNeck: [f32; 9] = [0.0; 9];
/// A positive value bypasses hand smoothing while holding the wheel. Previously
/// rate 150 only bypassed it when dt >= 1/150 s, so extra publications restored
/// lag. Keep the export for compatibility; zero opts into the base hand filter.
#[no_mangle]
pub static mut CyberpunkVR_WheelHandLerpSpeed: f32 = 150.0;
#[no_mangle]
pub static CyberpunkVR_NativeHandPublishes: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_PresentHandPublishSkipped: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_FrameHandReferenceRejected: AtomicU64 = AtomicU64::new(0);

// ---- src/Runtimes/OpenXRPresent.cpp

/// How many captures actually used it. If this tracks the present rate the second eye is being labelled
/// from its own frames; if it stays at zero the queue is not being filled and the eye is on MAIN's label.
#[no_mangle]
pub static mut CyberpunkVR_DebugVrcamLabelUsed: u64 = 0;
/// Render-ahead depth, in PRESENT INTERVALS (not in pushes -- see the ring in the header).
///
/// 0, and it is not a preference -- it is the only value CONSISTENT with the exact path.
///
/// The exact path matches m_framePoseSerial == S, and that serial is (interval the write
/// happened in) + 1, i.e. it returns the write made during interval S-1. The ring lookup with
/// lag L returns the newest entry stamped <= S-1-L. Those two agree only at L = 0. With L = 1
/// the ring handed back a pose one whole interval OLDER than the exact path -- so the 82% of
/// frames that hit exact and the 18% that fell through were labelled a frame apart from each
/// other. Every fallback frame therefore submitted an image with a stale orientation, the
/// compositor re-warped it by a rotation already baked in, and the result is the judder that
/// only shows on head turns. Measured live: PoseExact 29570 vs PoseEstimated 6363.
///
/// 1 and 2 stay reachable because a different runtime could genuinely render deeper, but they
/// must be compared against the exact path, not chosen on feel.
#[no_mangle]
pub static mut CyberpunkVR_PoseFrameLag: i32 = 0;
/// How often the frame's pose is the exact one recorded inside that frame, versus the
/// interval estimate. Exact should dominate; a rising estimate count means the depth barrier
/// is not firing for those frames and the binding is back to guessing there.
#[no_mangle]
pub static mut CyberpunkVR_DebugPoseExact: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPoseEstimated: u64 = 0;
/// How often the frame being submitted had its own published slot (pose + per-eye offset + per-eye
/// FOV, all located together for THIS serial) versus had to fall back to the live values. A miss
/// is the old mixed-timeline path; it should be rare and it should not grow during gameplay.
#[no_mangle]
pub static mut CyberpunkVR_DebugSlotHit: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugSlotMiss: u64 = 0;
/// Intervals that contained no locate of their own, so the frame legitimately carries the previous
/// slot. Expected to be substantial whenever the game presents faster than the XR loop cycles;
/// only DebugSlotMiss (no slot at all) is a real fallback to live values.
#[no_mangle]
pub static mut CyberpunkVR_DebugSlotReused: u64 = 0;

// ---- src/Runtimes/PhysicalCrouch.cpp

#[no_mangle]
pub static mut CyberpunkVR_PhysicalCrouchDepth: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_PhysicalCrouchDesired: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_PhysicalCrouchApplied: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_PhysicalCrouchActual: i32 = 0;
/// 1: while the body walks after the head the bend tracker reads a bob as a step (Anim/BodyBend.hpp BendWalkBobDrop)
#[no_mangle]
pub static mut CyberpunkVR_BendWalkMode: i32 = 1;

// ---- src/Stereo/Capture.cpp

/// Scope lighting and postprocessing consume the same exposure as VRCAM.
#[no_mangle]
pub static mut CyberpunkVR_OpticsExpoMirror: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_OpticsExpoHits: u64 = 0;
/// The fixed resting-state GUESS that used to live here is DELETED. It was documented as a
/// "fixed GUESS fallback only" and it is exactly the shape of lie the driver's state machine
/// faults on -- see the note beside t_mirror_src_state_seen. There is a tracked state or
/// there is no copy.
/// The vrcam RenderFinal2D output's resting D3D12 state VARIES frame-to-frame. A fixed
/// guess (CyberpunkVR_MirrorCopyState) makes the copy's transition barrier StateBefore
/// wrong on mismatched frames -> hazard -> the copy reads stale/aliased heap memory
/// (main's content, since vrcam's output shares the transient heap with main) -> the
/// bright/dark alternation. hk_ResourceBarrier already tracks the resource's ACTUAL
/// state (CyberpunkVR_DebugMirrorSrcState); use THAT for the copy barrier. 1=track (fix),
/// 0=old fixed-guess behavior (for A/B).
#[no_mangle]
pub static mut CyberpunkVR_MirrorTrackState: i32 = 1;
/// Redirect the vrcam RenderFinal2D output RTV to our OWN committed target so the engine
/// renders the final into a stable, never-aliased resource we control. View-aware (only
/// inside the ctx-keyed vrcam RenderFinal2D node); no resolution heuristic -> VR-safe.
/// Default 0 (enable live via x64dbg after deploy, so a bad build can't wedge startup).
#[no_mangle]
pub static mut CyberpunkVR_VrcamOwnTarget: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugOwnTargetSubs: u64 = 0;
/// Diagnostic: when 1, the present thread clears the mirror window to RED and does
/// NOT copy mtex. Red window => present/window pipeline works => black is the source
/// copy. Black window => present/window itself is broken. Isolates the two halves.
#[no_mangle]
pub static mut CyberpunkVR_MirrorTestPattern: u32 = 0;
/// 0 idle 1 finding 2 init 3 running 9 failed
#[no_mangle]
pub static mut CyberpunkVR_DebugMirrorState: u32 = 0;
/// found vrcam ID3D12Resource
#[no_mangle]
pub static mut CyberpunkVR_DebugMirrorRes: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMirrorW: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMirrorH: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMirrorFrames: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMirrorLastHr: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMirrorFmt: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMirrorStage: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMirrorRtvRegs: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMirrorRtvEvicts: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPubOk: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugHudSnaps: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugHudSnapSkips: u64 = 0;
/// ---- IS THE SUN-CASCADE ATLAS ONE TEXTURE FOR BOTH EYES? -----------------------------------------
///
/// Asked because two attempts to make one eye reuse the other's cascades both failed, in opposite
/// directions, and because the answer decides whether "hand the shadows from MAIN to the second view" is a
/// fix or a contradiction:
///
/// * one texture  -> both eyes already rasterise into it, from a cascade record the in-frame probe shows
/// identical, so they already sample the SAME shadow map. There is nothing to hand over
/// and the map cannot be the source of the eye difference at all.
/// * two textures -> reuse would mean something, and the two maps' CONTENT becomes worth suspecting even
/// though their inputs are measured identical.
///
/// Everything said about this so far has been inference from the port's own comments ("reference the existing
/// cascade atlas") and from how the failures looked. This measures it: the cascade pass binds a depth target,
/// so the DSV descriptor it binds is recorded per view and per cascade index.
///
/// MEASURED: identical, both cascades, both views -- casc0 M=197E7A25A60 V=197E7A25A60, casc1 ...A68 for
/// both. (One earlier report showed casc1 differing; that was a transition, MAIN's handle coming from another
/// part of the heap, and by the settled report both agreed.)
///
/// AND THE CLAIM I DREW FROM IT WAS TOO STRONG, so it is corrected here rather than left standing: equal CPU
/// descriptor handles prove the same descriptor SLOT, not the same resource. The engine is free to rewrite
/// that slot to point at a different texture for each graph, and this probe cannot tell the two apart --
/// resolving it would need CreateDepthStencilView intercepted the way CreateRenderTargetView already is.
///
/// AND THE CAPTURE SETTLED IT: ONE SLOT, TWO TEXTURES. Nsight names resources directly in barriers, and the
/// two cascade runs in a frame (318 draws each, events 19604..21387 and 46338..48113) clear the very same DSV
/// -- DescriptorHeap_58 @ 22, which is what this probe saw -- while barriering DIFFERENT resources:
///
/// run 0 (first view):  Resource_48841 x3   + Resource_5159 x1
/// run 1 (second run):  Resource_4843  x3   + Resource_5159 x1
///
/// So the engine rewrites that descriptor per graph and each view rasterises into its OWN atlas. There was
/// never anything shared to hand over, which is exactly why all three reuse attempts produced artefacts (see
/// the retired cascade draw-suppression experiment): each one made a view sample a texture that nothing in its
/// own graph had written.
///
/// The CONTENT of the two is still duplicated -- identical record, identical casters -- so the work is
/// redundant even though the texture is not shared. Saving it would mean copying one atlas into the other, or
/// repointing the sampling descriptor, not skipping a pass.
///
/// Default 0: it has answered, and its answer needed the capture to be read correctly.
#[no_mangle]
pub static mut CyberpunkVR_CascRtProbe: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugFinalGrabs: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugFinalArms: u64 = 0;
/// The engine's barriers by kind (vr_bench ab --counters, per window): [0] ResourceBarrier calls, [1] transitions,
/// [2] aliasing barriers (a render graph reusing heap memory between transient resources), [3] UAV barriers,
/// [4] split-barrier begins, [5] split-barrier ends, [6] transitions of all subresources, [7] time inside the
/// runtime and driver's ResourceBarrier (100 ns units). The port's own (internal) barriers are not counted.
#[no_mangle]
pub static CyberpunkVR_BarrierCounts: [AtomicU64; 8] = [const { AtomicU64::new(0) }; 8];

// ---- src/Stereo/CommandListCensus.cpp

#[no_mangle]
pub static mut CyberpunkVR_DebugExpoValMain: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_DebugExpoValVrcam: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_DebugExpoMirrors: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugTileNonzeroMain: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugTileNonzeroVrcam: u64 = 0;
/// Published by the DRAW: the vertex shader the sight actually runs with.
#[no_mangle]
pub static mut CyberpunkVR_DebugSightVsUsed: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DrawProbePs: [u64; 4] = [0; 4];
#[no_mangle]
pub static mut CyberpunkVR_DrawProbeVs: [u64; 4] = [0; 4];
#[no_mangle]
pub static mut CyberpunkVR_DrawProbePsLen: [u32; 4] = [0; 4];
#[no_mangle]
pub static CyberpunkVR_DrawProbeHits: AtomicU64 = AtomicU64::new(0);
/// Re-arm both slots at once, every few seconds. Sampling the two views in DIFFERENT frames
/// would let the weapon's own sway (centimetres) drown the 65 mm we are looking for; re-arming
/// them together keeps each pair one frame apart at worst, and repeating it makes a one-off
/// coincidence visible as noise instead of being mistaken for the answer.
#[no_mangle]
pub static mut CyberpunkVR_SightAxisRepeatMs: i32 = 4000;
#[no_mangle]
pub static mut CyberpunkVR_DebugLutSrcLearn: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugLutSrcLend: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugLutSrcStale: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugCascadeDrawsSide0: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugCascadeDrawsSide1: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugCascadeDrawsSideX: u64 = 0;

// ---- src/Stereo/DeviceHooks.cpp

/// STREAMED RESOURCES INTO THE ALLOCATOR'S HEAPS. The engine allocates through D3D12MA and asks for a dedicated
/// (committed) resource for every texture over 256 x 256 texels (sub_140347F74: w*h*d <= 0x10000) and every buffer
/// over 256 KB (sub_1403445AC: size-1 <= 0x3FFFF). Streaming creates them as the head turns: ~7 a frame in fast yaw,
/// ~3 ms of runtime/driver time EACH (CyberpunkVR_CreationCounts[12]), against ~0.13 ms for a placed one.
/// CyberpunkVR_StreamingPlaced=1 (default; 0 restores the engine's limits) raises both limits (16 M texels, 32 MB) so
/// they are placed into D3D12MA's blocks. Measured (docs/vr-render-optimization-20261104.md, "Resource creation"):
/// committed creations a 6 s fast-yaw window 1,970-3,270 -> 36-45, process CPU -4.6 ms a frame (ETW), video memory
/// ~0.15 GB lower, the GPU and the stereo pictures unchanged;
/// the allocator still makes anything over half a block committed itself, and render-target / depth / UAV
/// textures stay committed (their own flag test, untouched). Only the compare's immediate changes -- a stale or
/// half-written value is still a valid threshold -- and the original bytes are checked first.
/// on by default (measured, above)
#[no_mangle]
pub static mut CyberpunkVR_StreamingPlaced: i32 = 1;
/// 1 applied, 2 restored, 0xFFFFFFFF bytes differ
#[no_mangle]
pub static mut CyberpunkVR_StreamingPlacedState: u32 = 0;
/// CyberpunkVR_CreationStacks=1: every committed / placed creation is keyed by the first exe frames of its call
/// stack (RVAs) plus the resource's dimension, heap type and size class; the 16 busiest keys go to the log every
/// 4096 creations. Off by default: a stack walk per creation is not free.
#[no_mangle]
pub static mut CyberpunkVR_CreationStacks: i32 = 0;
/// last local usage, peak
#[no_mangle]
pub static CyberpunkVR_VideoMemoryMB: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];
#[no_mangle]
pub static mut CyberpunkVR_DebugExecPerFrame: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMirrorReadyFence: u64 = 0;

// ---- src/Stereo/Dlss.cpp

/// 1=clear vrcam build-flag 64 (bit0 view+0x17D8) -> build output-res post like main (crop fix)
#[no_mangle]
pub static mut CyberpunkVR_VrcamClearFlag64: u32 = 1;
/// times flag64 cleared for vrcam
#[no_mangle]
pub static mut CyberpunkVR_DebugClearFlag64Hits: u64 = 0;
/// times vrcam res was scaled
#[no_mangle]
pub static mut CyberpunkVR_DebugVrcamResScaleHits: u64 = 0;
/// last vrcam render width (diag)
#[no_mangle]
pub static mut CyberpunkVR_DebugVrcamRenderW: u32 = 0;
/// last vrcam render height (diag)
#[no_mangle]
pub static mut CyberpunkVR_DebugVrcamRenderH: u32 = 0;
/// COMPUTE-RESOLVE ROUTING (plan B, live-tunable): the vrcam-only crop = raster tonemap
/// sub_140557204, gated by GROUP 20 = bit20 of view+0x17D0 (sub_14033E358(ctx,20)). LIVE-CONFIRMED:
/// main bit20=0 (SKIPS the raster tonemap body -> composites via COMPUTE, res-agnostic, full output),
/// vrcam bit20=1 (runs raster tonemap @ render-res 1418 viewport on the 2444 DLSS output -> CROP).
/// The ONLY two view+0x17D0 bits that differ main-vs-vrcam are bit20 (main0/vrcam1) & bit25 (main1/vrcam0).
/// Mode: 0=off, 1=clear bit20, 2=clear bit20 + set bit25 (== EXACT match to MAIN), 3=set bit25 only.
/// Read at EXECUTE by the tonemap's own gate every frame => NOT the cached build-graph group-69 dead end.
/// SHIPPED DEFAULT=2. LIVE-VERDICT: bit20 (group 20) is necessary+sufficient for the crop (clear=no crop, set=crop; mode 3 proved bit25 alone irrelevant). 2 = EXACT match to MAIN's view+0x17D0 (a config main runs every frame => known-good, no synthetic hybrid). Active under VrcamDlss ALONE (VrcamDlssScale retired); DLAA/no-DLSS: matching main's flags is a no-op there.
#[no_mangle]
pub static mut CyberpunkVR_VrcamComputeResolve: u32 = 2;
#[no_mangle]
pub static mut CyberpunkVR_DebugP17D0Hits: u64 = 0;
/// view+0x17D0 before our edit (diag)
#[no_mangle]
pub static mut CyberpunkVR_DebugP17D0Before: u64 = 0;
/// view+0x17D0 after our edit (diag)
#[no_mangle]
pub static mut CyberpunkVR_DebugP17D0After: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_SkipVrcamReadbackNode: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugReadbackSkips: u64 = 0;
/// DIAGNOSTIC A/B: zero the DLSS jitter the engine feeds vrcam (live float @ a1+0x1E0/+0x1E4,
/// found in x64dbg -- the source the const-setter copies into sl::Constants). If vrcam's
/// GEOMETRY render is NOT jittered but DLSS un-jitters by this offset, that mismatch shimmers;
/// forcing jitter=0 aligns them (stable spatial DLAA) and the flicker should stop. If the
/// render IS jittered, zeroing makes it worse -> tells us jitter is not the cause.
/// 1=force vrcam DLSS jitter to 0
#[no_mangle]
pub static mut CyberpunkVR_VrcamDlssZeroJitter: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugVrcamDlssEvalHits: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugVrcamDlssConstHits: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugVrcamApplyDlssHits: u64 = 0;
/// vrcam post getter override: 0=off, 1=CASE C (renderer+0x148 global output; VR-correct but desktop=window res != 2444), 2=CASE B (vrcam OWN output VP+0x54=2444; matches RTT texture+DLSS out in BOTH desktop and VR -- preferred)
#[no_mangle]
pub static mut CyberpunkVR_VrcamForceCaseC: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugForceCaseCHits: u64 = 0;

// ---- src/Stereo/EngineStereoDlss.cpp

/// 1 = DLSS per eye under engine stereo (default); 0 = the engine's one evaluate over both eyes. Live.
#[no_mangle]
pub static CyberpunkVR_EngineStereoDlssPerEye: AtomicU32 = AtomicU32::new(1);
/// [0] evaluates split per eye, [1] passed through (another feature / mode off), [2] no tags for the viewport,
/// [3] no constants for the viewport and frame, [4] tags without a colour input and output of even width,
/// [5] eye options set, [6] an eye's evaluate failed, [7] the engine's options calls seen.
#[no_mangle]
pub static mut CyberpunkVR_DebugEngineStereoDlss: [u64; 8] = [0; 8];
/// Diagnostics, live: 1 = the right eye evaluated first, 2 = the right eye's output put into the left half,
/// 4 = the left eye only, 8 = the eye's tags also passed with its evaluate (local tags), 16 = the right eye
/// written straight into its half (no target of its own: DLSS leaves the half black), 32 = the right eye's
/// inputs read in their halves at an offset (no copies).
#[no_mangle]
pub static CyberpunkVR_EngineStereoDlssMode: AtomicU32 = AtomicU32::new(0);
/// The last split: per eye [options result, tag result, constants result, evaluate result] (eye 0 at 0..3).
#[no_mangle]
pub static mut CyberpunkVR_DebugEngineStereoDlssResults: [i32; 8] = [0; 8];
/// 1 = each eye's DLSS gets the jitter in its own pixels (default); 0 = the engine's jitter as it is. Live.
#[no_mangle]
pub static CyberpunkVR_EngineStereoDlssEyeJitter: AtomicU32 = AtomicU32::new(1);
/// The last split's constants as the engine gave them: jitter x, y (input pixels), mvecScale x, y; and the scale
/// the eyes' jitter was given, x, y.
#[no_mangle]
pub static mut CyberpunkVR_DebugEngineStereoDlssJitter: [f32; 6] = [0.0; 6];
/// Test only, live: non-zero x / y replace the eyes' jitter scale (sign included); [2] non-zero replaces the
/// motion scale's x factor (2 by default).
#[no_mangle]
pub static mut CyberpunkVR_EngineStereoDlssJitterTest: [f32; 3] = [0.0; 3];
/// Test only, live: bit N set = the eyes' DLSS gets no resource for the engine's tag of type N (29 = the bias
/// current colour mask).
#[no_mangle]
pub static CyberpunkVR_EngineStereoDlssDropTags: AtomicU32 = AtomicU32::new(0);
/// Test only, live: 1 = the right eye's output target is taken as not made (as with the GPU's memory full),
/// 2 = the right eye's evaluate is taken as failed.
#[no_mangle]
pub static CyberpunkVR_EngineStereoDlssFailTest: AtomicU32 = AtomicU32::new(0);
/// Back to the engine's one evaluate: [0] frames without the right eye's output target, [1] 1 once the eyes'
/// evaluates failed for good this session.
#[no_mangle]
pub static mut CyberpunkVR_DebugEngineStereoDlssFallback: [u64; 2] = [0; 2];
/// The same for the types 32-63 (bit N = type 32 + N: 39 ColorBeforeParticles, 42 SpecularHitDistance).
#[no_mangle]
pub static CyberpunkVR_EngineStereoDlssDropTagsHigh: AtomicU32 = AtomicU32::new(0);
/// 1 = Ray Reconstruction's left eye reads copies of its halves at (0, 0) as the right eye does (default); 0 = its
/// halves in the engine's resources at offset 0, as DLSS's left eye. Read in place, RR's left eye ghosted fog and fire
/// stuck on the screen (2026-11-02, gone with the copies and back without them). Live.
#[no_mangle]
pub static CyberpunkVR_EngineStereoRrCopyLeft: AtomicU32 = AtomicU32::new(1);
/// 1 = Ray Reconstruction (DLSS-D) per eye as DLSS is (default); 0 = the engine's one RR evaluate over both eyes. Live.
/// One RR over the side-by-side frame reprojects each eye's specular history with one camera and one projection
/// spanning both halves: reflections stuck to the surface.
#[no_mangle]
pub static CyberpunkVR_EngineStereoRrPerEye: AtomicU32 = AtomicU32::new(1);
/// 1 = each eye's RR gets its own camera position (centre -+ sep/2 along the camera's right) in its constants and
/// view matrices (default); 0 = the engine's centre for both. Live.
#[no_mangle]
pub static CyberpunkVR_EngineStereoRrEyeCamera: AtomicU32 = AtomicU32::new(1);
/// [0] RR evaluates split per eye, [1] RR passed through (no recorded options / mode off), [2] eye options set,
/// [3] RR options calls seen, [4] eye view matrices moved, [5] view matrices left (layout not recognised).
#[no_mangle]
pub static mut CyberpunkVR_DebugEngineStereoRr: [u64; 6] = [0; 6];

// ---- src/Stereo/EngineStereoSdk.cpp

/// Calls into the plugin's SDK: [0] factory, [1] interface, by slot (8 bytes each). A slot outside the contract
/// below is logged once and answered 0.
#[no_mangle]
pub static CyberpunkVR_DebugEngineStereoSdkCalls: [[AtomicU64; 16]; 2] = [const { [const { AtomicU64::new(0) }; 16] }; 2];
/// 1 once the main menu has been shown (Armed).
#[no_mangle]
pub static CyberpunkVR_EngineStereoArmed: AtomicI32 = AtomicI32::new(0);
/// Main menu controllers initialised and not yet uninitialised (an instance can be made before the last one goes).
#[no_mangle]
pub static CyberpunkVR_EngineStereoMainMenus: AtomicI32 = AtomicI32::new(0);

// ---- src/Stereo/FrameGraph.cpp

#[no_mangle]
pub static mut CyberpunkVR_DebugFullEye: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugFullTotal: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugIncrEye: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugIncrTotal: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_OpticsSceneLinear: i32 = 1;
/// 1 = the scope view renders without AO and SSR (default), 0 = with MAIN's, for A/B.
#[no_mangle]
pub static mut CyberpunkVR_OpticsNoScreenSpace: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_DebugFgRttF0: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugFgRttF1: u64 = 0;
/// diag: main's upscaler bits
#[no_mangle]
pub static mut CyberpunkVR_DebugMainUpscalerGroups: u64 = 0;
/// diag: vrcam forced -> DLSS count
#[no_mangle]
pub static mut CyberpunkVR_DebugUpscalerForceHits: u64 = 0;
/// LIVE A/B of the two reuse bits that are the ONLY difference between vrcam and
/// main flags (camera + env handles proven identical). Diagnoses which reuse bit
/// breaks vrcam lighting (wrong light / triangle shadows / light bleed / no refl).
/// 0 = current: cascade-reuse (bit50 SET) + GI-reuse (bit31 CLEAR)
/// 1 = EXACT main flags (bit50 CLEAR, bit31 SET) -> vrcam builds own cascades+GI
/// (expect main shadow flicker, but tells us if reuse is what breaks visuals)
/// 2 = cascade-reuse only (bit50 SET, bit31 SET=GI native/own)
/// 3 = GI-reuse only (bit50 CLEAR=cascade native/own, bit31 CLEAR)
/// DEFAULT WAS 1: with the temporal-history fix (StreamlineHistoryFix) giving vrcam a
/// real per-view temporal view-state, vrcam building its OWN cascades+GI no longer
/// collides with main (main flicker GONE, proven live) and fixes interior shadows/
/// sun light that cascade/GI-reuse (mode 0) broke. mode 0 kept for A/B fallback.
///
/// NOW 2, AS THE SINGLE-FACTOR TEST FOR THE FOLIAGE-SHADOW MISMATCH (2026-08-17). Mode 1 means each eye
/// rasterises the sun cascades itself, into what the shared-state notes say is ONE atlas -- so the second
/// view rebuilds a global that the other view also builds. Everything that FEEDS those two builds is now
/// measured identical (the cascade record: 0 differing dwords in-frame; casters: 94002 draws against 94004;
/// per-index dispatches: casc0/casc1 2410 each with no casc2/3 for either; wind; the whole 480-byte frame
/// block, mirrored). What was never verified is the OUTPUT -- and the only way an identical-input rebuild
/// can still differ is a race between one view's rebuild and the other's sampling, which lands hardest on
/// alpha-tested foliage. Bit 50 removes the second rebuild entirely, so this tests exactly that and costs
/// GPU time rather than spending it.
///
/// Mode 2 rather than mode 0 deliberately: mode 0 is cascade reuse AND GI reuse, and the note above records
/// that the pair broke interior shadows and sun light without saying which half did it. Mode 2 is cascade
/// reuse alone, with GI staying native at the flag level (its reuse is a separate node-level hook,
/// CyberpunkVR_GiReuseMode).
///
/// TWO PREDICTED FAILURE MODES, written down so the result can be read rather than guessed at:
/// * interior shadows / sun light break in the second eye again -> then it was the cascade half of mode 0,
/// and reuse is not available at all;
/// * the second view's shadows SLIDE WITH MOVEMENT -> its graph runs first, so it would sample an atlas
/// rasterised with the PREVIOUS frame's cascade placement while using this frame's matrix. At walking
/// pace that is ~4 cm of world offset per frame.
/// Either of those is a reason to go back to 1, and neither is a reason to call the test wasted.
///
/// AND IT WAS THE SECOND ONE, measured: MAIN fine, shadows swimming in the left eye with many artefacts. The left
/// eye is the second view, so reuse in THIS DIRECTION is structurally wrong -- not because sharing an atlas
/// is wrong, but because the view that reuses runs FIRST and therefore samples content from the previous
/// frame with the current frame's matrix. Back to 1.
///
/// THE DIRECTION IS THE WHOLE POINT, and it is fixable: the second view runs first, so IT should be the
/// builder and MAIN the reuser. Then the atlas is built and sampled inside one frame, against a record the
/// in-frame probe already showed to be identical at both passes -- no staleness at all. That cannot be done
/// from here (this path forces the second view's flags, not MAIN's); it belongs in the cascade node detour,
/// where the node can simply be skipped for MAIN. See CyberpunkVR_CascadeSkipMain in ViewReuse.cpp.
#[no_mangle]
pub static mut CyberpunkVR_VrcamFlagMode: u32 = 1;
/// VRCAM final-color EXTRACTION fix: the full scene builder gates ExtractionSceneColor
/// (sub_1428E5748) and ExtractionFinalColor (sub_1408EAFCC) on build-bit 64
/// (sub_1407305B0(a4,64) == f1 bit 0). CopyToTexture (unconditional) READS the final-
/// color 0x3D7E6258 -> for main it's written by a separate final builder, but VRCAM
/// only runs the scene builder where bit 64 is 0 -> extraction skipped -> VRCAM copies
/// an unwritten (black) final-color. Set bit 64 in VRCAM's computed flags so the scene
/// builder emits the extraction passes for VRCAM too. Default OFF -> toggle live.
/// proven live-safe: adds ExtractionFinalColor
#[no_mangle]
pub static mut CyberpunkVR_VrcamExtractionFix: u32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_DebugVrcamExtractionHits: u64 = 0;
/// VRCAM final-color GROUP: the scene builder only adds ExtractionFinalColor (bit 64),
/// NOT ClearFinalColorTarget / DeclareCommonResourceAllocs_FinalOnly (those live only in
/// the separate blank/final builder that VRCAM never runs). Hook the Extraction ADDER
/// (sub_1408EAFCC) and, when it is called from the SCENE builder (== VRCAM, since only
/// VRCAM has bit 64 set there), inject the Declare + Clear adders FIRST so the graph gets
/// Declare -> Clear -> Extraction, matching the engine's blank-builder order. Return-addr
/// range-gated to the full/incremental scene builders so MAIN's blank builder (which
/// already adds all three) is never double-fed. Default OFF (Declare_FinalOnly may
/// re-declare final-color) -> enable + verify live.
/// OFF: ClearFinalColorTarget literally clears 0x3D7E6258 (the RenderFinal2D output we capture) to the bg color -> wipes the vrcam frame to black. RenderFinal2D already draws the full frame; no clear needed.
#[no_mangle]
pub static mut CyberpunkVR_VrcamFinalGroup: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugVrcamFinalGroupHits: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugVrcamCompositionHits: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugRttFlagForceHits: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugRttEnvBindHits: u64 = 0;
/// DISABLED (breaks RTT view)
#[no_mangle]
pub static mut CyberpunkVR_ForceVrcamRes: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugForceCamHits: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugForceResHits: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMainW: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMainH: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugEnvExtraBinds: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugWaterMirrors: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugWaterPtrSkips: u64 = 0;
/// STEP 1 SCOPE: this file forces the VRCAM view's PROJECTION only -- fov, zoom, near
/// and far, copied from MAIN so the second view frames the world identically. It does
/// NOT touch camera position or orientation. Every eye-offset / tripod / mirror / HMD
/// path that used to live here is gone: writing the camera at this stage happens after
/// culling and after the weapon viewmodel is placed, so it slid the world and dragged
/// the weapon with the head. Camera work belongs in the engine's own camera hooks and
/// is a later step.
#[no_mangle]
pub static mut CyberpunkVR_OverlayVisible: i32 = 1;
/// Explicit vertical FOV in degrees for the vrcam eye. 0 = follow MAIN, which is what the
/// flat-screen testbed wants. This is the hook for the headset: once the HMD drives the eye,
/// put its FOV here and weapon ADS keeps applying on top of it.
#[no_mangle]
pub static mut CyberpunkVR_VrcamFovDeg: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_OpticsViewSetupHits: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_OpticsViewExtractHits: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugRttRectHits: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugWaterAssigns: u64 = 0;

// ---- src/Stereo/GameThreadQueues.cpp

/// [0] dequeues, [1] jobs taken from queue 3, [2] from queue 2, [3] dequeues that found nothing on 3 and 2
/// while queue 1 had jobs (left to the workers).
#[no_mangle]
pub static CyberpunkVR_GameThreadQueueStats: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];

// ---- src/Stereo/Grading.cpp

#[no_mangle]
pub static mut CyberpunkVR_OpticsDataProbe: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_OpticsViewData: [[u8; 4096]; 3] = [[0; 4096]; 3];
#[no_mangle]
pub static mut CyberpunkVR_OpticsGradeData: [[u8; 688]; 3] = [[0; 688]; 3];
#[no_mangle]
pub static mut CyberpunkVR_OpticsDataSeq: [u32; 3] = [0; 3];
#[no_mangle]
pub static mut CyberpunkVR_OpticsGradeSeq: [u32; 3] = [0; 3];
#[no_mangle]
pub static mut CyberpunkVR_OpticsGradeMirror: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_OpticsMaterialData: [[f32; 84]; 3] = [[0.0; 84]; 3];
#[no_mangle]
pub static mut CyberpunkVR_OpticsMaterialSize: [u32; 3] = [0; 3];
#[no_mangle]
pub static mut CyberpunkVR_OpticsMaterialSeq: [u32; 3] = [0; 3];
#[no_mangle]
pub static mut CyberpunkVR_OpticsShadowData: [[f32; 232]; 3] = [[0.0; 232]; 3];
#[no_mangle]
pub static mut CyberpunkVR_OpticsShadowSeq: [u32; 3] = [0; 3];
/// ================================================================================================
/// WHY CLOTH FLAGS STAND STILL IN THE SECOND EYE -- the second view's clock is zero.
///
/// Reported 2026-08-17: the fabric flags on buildings wave in MAIN and hang dead in the second eye.
///
/// THERE IS NO CLOTH SIMULATION IN THEM. The flags' pipeline state was identified from an Nsight
/// capture (PipelineState_55927: one draw per view in GBuffer_Discard, 11847 indices, the SAME vertex
/// and index buffers for both views), and its VERTEX shader does the waving itself:
///
/// phase  = frac(((b0[0].z * b4[2].x) / b4[1].z) + ... + sin(b0[0].z * ...))   // b0[0].z is TIME
/// wind   = sqrt(b0[9].x^2 + b0[9].y^2), normalised, dotted with INSTANCE_TRANSFORM
/// offset = sway texture sampled at (row, phase), scaled by b4[4].x * windResponse
/// pos    = POSITION * b5[4] + b5[5] + offset
///
/// so the motion is a function of two things only: the TIME in the frame-constants block and the wind
/// vector beside it. Nothing is skinned, nothing is simulated, and every search for a lost deformation
/// was looking for data that does not exist -- which is why the instance-skinning buffer, the
/// skinning/tangent node (frozen outright, its dispatch counter flat at 19940), the render mask (the
/// second view has all 27 categories), LOD substitution (identical draw shapes and buffers) and the
/// foliage-wind claim all came back clean.
///
/// THE MEASUREMENT. Both views upload that 480-byte block once per frame from the same node, so the
/// two can be compared field by field. Of its 120 floats, exactly three advance for MAIN and never
/// change for the second view, whose copies are all zero -- and the control direction, fields that move
/// only for the second view, was EMPTY:
///
/// [0].z  M=133150.22  V=0     the clock in seconds; THE field the cloth VS takes its phase from
/// [1].z  M=133150.19  V=0     the same clock as of the previous frame (~31 ms behind)
/// [1].w  M=0.1850     V=0     a slow clock, ~1e-4 per second: the day fraction
///
/// The wind vector is identical in both views, so it was never the problem. Time in the second eye
/// stands at zero for EVERYTHING, not just for cloth; the flags are simply the most visible thing that
/// is a function of it.
///
/// (Two more floats are zero on the same side, [9].z = 172.16 and [9].w = 728.93 for MAIN, but they
/// move too slowly to have earned the name "clock" -- so they are in the mask and off by default.)
///
/// THE FIX lends MAIN's clock to the second view's block, into a COPY and never into the engine's
/// buffer: sub_140334328 allocates, memcpys from the caller's pointer and commits (read in the
/// debugger before writing anything), so a temporary of ours is read once and engine state is not
/// touched at all. Taking MAIN's latest value is at worst one frame of phase -- 31 ms on a flag.
/// Verified live: the fill counter advances once per frame, and the flags were confirmed to wave in the headset.
///
/// TWO INSTRUMENTS PAID FOR THIS and are gone, both too expensive to ship: a per-(node, size, view)
/// census of 400/480-byte uploads, which is what named the producing node instead of guessing it, and a
/// draw-shape census over every DrawIndexedInstanced. Each took a mutex on a hot path. Their two
/// lessons are worth more than the code:
///
/// * a probe keyed only by SIZE is a mixture -- 480 bytes is a common block, and the first run put
/// three different render targets' dimensions in one column and called it a clock;
/// * "frozen" means nothing unless the view is uploading at all. The first run reported the second
/// view's fields as frozen when its bucket had simply stopped growing at 694 samples.
/// MODE 2, added 2026-08-17 for the shadow mismatch, and it supersedes mode 1 rather than extending it.
///
/// The complete diff (above) found the frame counter at [28].y differing by exactly ONE between the
/// views -- MAIN 12009 against VRCAM 12010, then MAIN 12425 against VRCAM 12424 -- and the shaders take
/// their dither slice from `asuint(b0[28].y) & 63`, so 41 against 42 and 9 against 8 are DIFFERENT
/// slices of blue noise. Alpha-tested geometry (fences, foliage, tree crowns) therefore discards a
/// different set of pixels in each eye, in the shadow cascade as much as in the GBuffer. That is the
/// reported defect exactly: a piece of shadow present in one eye and missing in the other, in different
/// places, flickering per pixel.
///
/// NOTE WHICH WAY THE ONE GOES: sometimes the second view is ahead, sometimes MAIN is. The counter
/// advances between the two views' uploads, and which of them uploads first varies frame to frame. That
/// is why mode 1 -- lend MAIN's latest value to the second view -- CANNOT fix this: in the frames where
/// the second view uploads first it would receive the previous frame's value and still differ by one.
/// The same objection applies to the clock mode 1 was written for.
///
/// So mode 2 holds ONE value per frame and gives it to BOTH views, keyed on the renderer's frame id:
/// the first upload of a frame installs the values staged from MAIN's previous authored block, and every
/// upload in that frame -- MAIN's own included -- ships those. Order-independent by construction, and
/// the two eyes are then bit-identical in these fields. The cost is that everything driven by them is
/// one frame late, uniformly: 31 ms of sway phase and one slice of noise, neither of which is visible.
///
/// MODE 3, added after modes 1 and 2 both failed to change the shadows -- and added because BOTH of them
/// depend on WHICH VIEW UPLOADS FIRST, so neither can prove anything about the dither hypothesis. Mode 2
/// keys on the renderer frame id, and the shipped values showed that id advancing between the two views'
/// uploads (MAIN shipped 11787 where it authored 11789; the second view shipped 11789) -- the engine
/// counts each VIEW as a frame, so that key does not group a stereo pair at all. Mode 1 does not use the
/// frame id but hands over "MAIN's latest", which is the same value only if the second view uploads
/// after MAIN.
///
/// So mode 3 removes the ordering question instead of arguing about it: the dither slice is forced to
/// ONE CONSTANT in both views. Equality then cannot fail for a mechanical reason, which is the only way
/// a negative result becomes evidence. If the eyes still differ under mode 3, the dithered alpha test in
/// this block is NOT what makes them differ, and the hypothesis is dead rather than unproven.
///
/// The cost while testing is a static noise pattern instead of a temporally varying one (the shaders
/// index a 64-slice blue-noise array with counter & 63, and a fixed slice means TAA has nothing to
/// average). That is a diagnostic setting, not a shipping one.
///
/// mode 0  off
/// mode 1  lend MAIN's latest to the second view only (the clock fix as first shipped)
/// mode 2  one value per frame for both views, keyed on the renderer frame id -- which does NOT
/// group a stereo pair, so this is mode 1 with extra steps
/// mode 3  mode 1, plus the dither slice forced to CyberpunkVR_SwayDitherConst in both views
/// mode 4  the second view gets MAIN's WHOLE block, verbatim
/// mode 5  mode 4, plus the dither slice forced to a constant in EVERY frame-constants upload,
/// whatever node made it  (default)
///
/// Mode 5 exists because mode 3 could not settle the question it was built for. The cascade pixel shader
/// dithers its alpha test from asuint(b0[28].y) & 63, and modes 1-3 all patch the 480-byte block uploaded
/// under PrepareSceneRendering -- but WHICH INSTANCE of b0 the cascade pass binds is not known, and the
/// cascade node itself uploads no 480-byte block at all (measured: zero rows). So those modes may simply
/// never have reached the shader, which makes their negative results worthless.
///
/// Mode 5 drops the node key for the dither pair alone: every 480-byte upload gets the same forced slice,
/// in both views, so no instance can be missed. The node filter is replaced by a CONTENT filter -- the
/// block must have a large clock at [0].z -- because 480 bytes is a common size and one of the other
/// blocks of that size was measured carrying render-target dimensions, which must not be overwritten.
/// If the eyes still differ under mode 5, the blue-noise dither is not the cause and the hypothesis is
/// finally dead rather than untested.
///
/// Mode 4 is the blunt form of the same doctrine, and it is blunt on purpose: no frame key, no field
/// mask, nothing to pick wrong. It is bounded by the measurement rather than by hope -- of the 120
/// floats only ten differ between the views at all, so copying the block wholesale changes exactly those
/// ten. Two of them are the camera position and direction ([5].xyz and [6].xyz, apart by the IPD), so
/// the second eye shades from MAIN's camera; at 1.3 cm that is not something an eye can find, and if it
/// ever is, the masked modes above are still there.
/// Back to 4: mode 5 forced the dither slice to a constant everywhere, which was a diagnostic and is
/// measured innocent -- and a constant slice means static noise, which is worse than the defect it was
/// testing for. Mode 4 keeps the clocks mirrored, which is what the cloth flags need.
/// MODE 1, narrowed back to what was actually confirmed working on the cloth flags: MAIN's clock lent
/// to the second view. Mode 4 copies the whole block, camera position included, and was never verified
/// as necessary -- shipping the broader change on the strength of an unrelated hunt would be guessing.
/// Mode 4 on request: the second view gets MAIN's whole frame-constants block. Worth
/// knowing which lever is which -- the cable's shadow stopped differing in the same build where this
/// went the other way, from 4 to 1 with the mask narrowed, so if it comes back this is the knob.
#[no_mangle]
pub static mut CyberpunkVR_SwayTimeFix: i32 = 4;
/// Which fields to hold, as a bitmask over kSwayTimeFields. Default: the three clocks (bits 0-2), the
/// dither pair (bits 5-6) and the second clock pair (bits 7-8). The two slow accumulators (3-4) and
/// [0].w (9) are measured to differ as well but are left out until something asks for them.
/// Live-settable, so widening or narrowing it costs no rebuild.
/// 0x07: the three clocks, which is the set the flags were fixed with. The dither pair (0x60) is
/// measured not to matter for the shadows, and the second clock pair (0x180) was added on the same
/// unproven hunch -- both remain available.
#[no_mangle]
pub static mut CyberpunkVR_SwayTimeMask: u32 = 7;
#[no_mangle]
pub static mut CyberpunkVR_DebugSwayTimeFills: u64 = 0;
/// Mode 3 only: the frame counter both views are given. Any value works -- only its low six bits are
/// read, as a slice index into a static noise array -- and being a constant is the whole point.
#[no_mangle]
pub static mut CyberpunkVR_SwayDitherConst: u32 = 12345;
/// ---- THE COMPLETE DIFF, because the first probe had a blind spot -------------------------------
///
/// Shadows cast by fences and foliage differ between the eyes: a piece of shadow is in one and not the
/// other, in different places, and it flickers per pixel. Two things are already measured: each view
/// rasterises its OWN cascade (cutting the node for the second view left that eye with no sun shadows
/// at all), and the two views draw the SAME casters into it -- live, at equal resolutions, 94002 draws
/// against 94004 and 267828 instances against 267831 in one interval, the residue being one frame's
/// worth on the interval boundary. So the same geometry, drawn with shared cascade records, ends up as
/// different pixels; and alpha-tested geometry becomes pixels through a DITHERED discard, whose noise
/// slice the pixel shader picks with asuint(b0[28].y) & 63 -- a frame counter.
///
/// THE FIRST PROBE COULD NOT HAVE SEEN THAT. It printed fields that advance for MAIN and are frozen for
/// the second view, and fields that are static but differ. A field that ADVANCES IN BOTH VIEWS WITH
/// DIFFERENT VALUES falls in neither list -- and a frame counter one step apart is exactly that. So
/// this reports every float whose current value differs at all, with what each side is doing to it,
/// and the dither index as an integer beside it. No categories to fall between.
/// DEFAULT 0: a mutex and a 120-float compare on every frame-constants upload. It found the frozen
/// clock and the dither difference; neither needs watching now.
#[no_mangle]
pub static mut CyberpunkVR_SwayDiff: i32 = 0;
/// ================================================================================================
/// THE CHECKERBOARD PHASE -- why thin shadows differ between the eyes.
///
/// The sun-shadow mask is evaluated at HALF resolution on a checkerboard, and the shader that does it
/// (PipelineState_858, one draw per view at 1280 and 1536 -- half of 2560 and half of 3072) opens with:
///
/// x  = uint(gl_FragCoord.x);  y = uint(gl_FragCoord.y);
/// px = (x << 1) | ((asuint(cb_b6[3]).y ^ y) & 1);      // the full-res pixel this lane stands for
///
/// so cb_b6[3].y decides WHICH half of the pattern this lane belongs to; the other half is reconstructed.
///
/// THE PHASE IS NOT THE PROBLEM -- measured, whole-block, in gameplay: the 56-byte block is IDENTICAL for
/// the two views, zero differing dwords, so both eyes use the same parity function. Forcing it patched 1008
/// times and changed nothing on screen, which is what a no-op looks like. The first "the counter differs by
/// one, so the parity is opposite" reading was inference, not measurement, and it was wrong.
///
/// WHAT REMAINS IS THE PATTERN ITSELF, AND IT IS STRUCTURAL. `parity` is a function of the SCREEN x and y
/// of the eye being drawn. One world point projects to different screen x in each eye, so it falls in the
/// evaluated half in one eye and in the reconstructed half in the other; and because px = (x << 1) | parity,
/// the sample it does get is a full-res pixel to the left in one eye and to the right in the other. That is
/// a one-pixel horizontal disagreement, worst where a shadow is one or two pixels wide and worse with
/// distance -- which is the reported symptom (shadows simply shifted... a little
/// farther out the thin part of a shadow differs between the eyes), and why grass and cables differ while a building's
/// shadow does not.
///
/// NO CONSTANT CAN EQUALISE THAT. Half of the full-res pixels are never evaluated by either eye, and which
/// world points land in the evaluated half is decided by each eye's own projection. Equalising the phase,
/// the seed, the wind, the cascade record or the frame clock cannot change which pixels get sampled -- which
/// is exactly why every one of those was measured identical and none of them helped. The only true fix is
/// full-resolution evaluation, i.e. a full-width mask target and a pixel shader without the `x << 1`, and
/// the engine offers no switch for it: the interleave is baked into this pass (its feature bits answer
/// identically for both views, see CyberpunkVR_FeatBitProbe), "CheckerboardSize" turns out to be an editor
/// selection-highlight setting, and the RTTI class that owns an "interleaved" property carries
/// enabled/radius/offset/angle/events beside it and has nothing to do with shadows.
///
/// So this stays a probe, at 0, and the write stays disarmed. Better a named structural limit than a knob
/// that pretends to fix it.
#[no_mangle]
pub static mut CyberpunkVR_CheckerPhaseFix: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_CheckerProbe: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_CheckerSize: u32 = 56;
#[no_mangle]
pub static mut CyberpunkVR_DebugCheckerFixes: u64 = 0;
/// ---- THE GRID ORIGIN, the one field the height mask is addressed by ------------------------------
///
/// The block diff cannot be used here and the reason is worth writing down, because it is the third time
/// today the same trap has caught me: the cascade node uploads an 848-byte block roughly 24 TIMES PER
/// FRAME PER VIEW (4156 against 4602 in a five-second interval at ~35 fps), and a probe that keeps only
/// the last one compares two arbitrary different blocks. That is what produced "132 of 212 floats differ"
/// with matrix rows reading M=546.40 against V=-0.0221 -- not a finding, an artefact.
///
/// So this watches ONE field instead of the whole block: b1[36].xyz, which the cascade pixel shader turns
/// into the height-mask grid origin as floor(x * 21.3333) * 0.046875. And it CHECKS ITS OWN ASSUMPTION --
/// that the field is one value per view per frame rather than varying across those 24 blocks -- because
/// mirroring it is only meaningful if it is.
/// DEFAULT 0: this one locks on an 848-byte upload that happens two dozen times a frame per view.
#[no_mangle]
pub static mut CyberpunkVR_GridProbe: i32 = 0;
/// ---- KILL THE HEIGHT-MASK DISCARD, in both eyes --------------------------------------------------
///
/// The last of the three discards in the cascade pixel shader, and the last candidate standing: the other
/// two are measured dead (the LOD-fade phase is 0 in both views with one distinct value seen, and forcing
/// the blue-noise slice to a constant in every frame-constants block changed nothing). The elimination is
/// sound in the other direction too -- solid objects' shadows match and only alpha-tested geometry
/// differs, and alpha-tested geometry is what these discards decide.
///
/// Equalising this one's input would need b1[36].xy, which I could not isolate: the cascade node uploads
/// an 848-byte block two dozen times a frame and the field's spread WITHIN one view is 7-10 world units,
/// so those blocks are not all the same block. Rather than guess, neutralise: the mask is a 256-texel map
/// covering 12 world units around floor(b1[36].xy * 21.3333) * 0.046875, and the shader itself skips the
/// test when the lookup lands outside [0,1] --
///
/// if (_146 < 0 || _148 < 0 || _146 > 1 || _148 > 1)  _157 = 0;   // and 0.999 - 0 < 0 is false
///
/// so an origin far from the world puts every pixel out of range and the discard never fires, in either
/// eye. If the shadows agree with the test dead, this discard is the cause and the real fix is to give
/// both views one origin. If they still differ, nothing in this shader's discards explains it and the
/// hunt moves to the cascade matrices.
/// DEFAULT 0 since it was measured innocent: neutralising this discard changed nothing on screen, and
/// leaving it off costs frame rate for nothing (the engine wanted that geometry skipped). Kept because the
/// measurement is worth being able to repeat.
#[no_mangle]
pub static mut CyberpunkVR_MaskKill: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_MaskKillOrigin: f32 = 1.0e9;
#[no_mangle]
pub static mut CyberpunkVR_DebugMaskKills: u64 = 0;
/// ---- THE VOLUMETRIC FOG'S JITTER, put in step between the eyes -----------------------------------
///
/// Found by the wide census plus a field diff, after the cascade fit was equalised and the blocky sun shafts
/// were still displaced. The fog pass uploads a 384-byte block once per view per frame, and ten of its 96
/// floats differ. Most of that is legitimate:
///
/// [7].x  M=520.3243  V=520.2593   -- apart by 0.0650, i.e. exactly the IPD: the camera position
/// [7].y, [2].x, [2].y             -- differences of 1e-4, derived from it
///
/// but four are not:
///
/// [10].x  M=-0.0499  V=-0.1045     the froxel jitter offset, advancing every frame
/// [10].y  M= 0.2578  V= 0.3414
/// [10].z  M= 0.4227  V=-0.1234
/// [11].z  M= 592     V= 78         the index into that jitter sequence
///
/// Volumetric fog is a coarse 3D grid whose blockiness is hidden by jittering it per frame and accumulating
/// over time. The two eyes jitter with different offsets and sit at different points of the sequence -- 592
/// against 78 -- so they accumulate different fog. That is what shows as sun shafts displaced in
/// opposite directions and a whole block of light present in one eye and absent in the other, and why it
/// reads as the same class of artefact as the grass without being the shadow map at all.
///
/// The second view runs first, so it is captured and MAIN is given its values, in-frame: taking MAIN's
/// latest would hand the earlier view a sequence position from the previous frame, which is the same trap
/// the cascade lend had to avoid. Copied as raw dwords, because [11].z is an integer in a float slot.
///
/// AND ALL OF THAT IS RETRACTED. A later reading of the same diff printed 49 of 96 floats differing, with
/// MAIN's column holding an exactly axis-aligned basis ([4] = (-1,0,0), [5] = (0,0,1), [6] = (0,-1,0)), a
/// near-origin position ([7] = (0.04, 3.62, 1.59)) and round defaults ([12] = 250/250/125, [13] = 0.9/0.9/0.9)
/// while the other column held a real world camera at (-7026, -4512, 80). Those are not two views of a scene.
/// MAIN's column was catching a CUBEMAP FACE -- a reflection probe's fog block -- because more than one
/// consumer uploads 384 bytes under this node, and the (node, size) key cannot tell them apart.
///
/// So the "10 of 96 differ" reading that this fix was built on is void as well: it was comparing whichever
/// instance each view happened to upload last, which is the same key-too-coarse mistake that has now cost
/// this investigation five separate rounds. The fix DID fire (its counter advanced once per frame, verified)
/// and it changed nothing visible -- which is the only honest thing left to say about it, because a patch
/// keyed on a mixture may well have been writing scene jitter into a probe face.
///
/// DEFAULT 0. Re-arming it needs a CONTENT filter, not a tighter node: the player's block is the one whose
/// camera position matches the world position the player view carries, and a basis that is not axis-aligned.
/// Until the probe can separate the instances, nothing measured here about per-view fog jitter is established.
#[no_mangle]
pub static mut CyberpunkVR_FogJitterFix: i32 = 0;
/// Gated: the player view's 384-byte volumetric-fog block, per side (MAIN, VRCAM), as last uploaded.
#[no_mangle]
pub static mut CyberpunkVR_FogBlockDump: [[f32; 96]; 2] = [[0.0; 96]; 2];
#[no_mangle]
pub static mut CyberpunkVR_DebugFogJitterFixes: u64 = 0;
/// ---- THE PROJECTION JITTER, put in step between the eyes -----------------------------------------
///
/// The once-per-frame census, after it was finally made to compare two eyes and nothing else, left four scalars
/// of the 848-byte CameraShaderConsts differing while every matrix entry around them agreed to 1e-4:
///
/// [0].w   M= 0.062  V=-0.031        float 3
/// [4].z   M= 2.134  V=-1.066        float 18
/// [16].w  M= 0.062  V=-0.031        float 67
/// [28].w  M= 0.062  V=-0.031        float 115
///
/// The same values appear under RenderShadowmask, VolumetricFog, ApplyTXAA, RenderBackground and
/// RenderScreenSpaceWaterDepth -- i.e. it is one shared field of that block, not a per-pass quantity -- and
/// they advance every frame. That is the shape of a projection JITTER: the sub-pixel offset an engine adds per
/// frame so a temporal filter can accumulate. Two eyes on different jitter sequences accumulate different
/// results, and the passes carrying it are exactly the ones that draw the blocky sun shafts seen
/// displaced in opposite directions.
///
/// WHAT IS NOT CLAIMED. The field is not identified. The 1:-2 ratio between the views is one instant's sample,
/// not a law -- a jitter sequence gives an arbitrary ratio each frame -- and an earlier reading of these same
/// numbers as an eye offset was WRONG: the port's own camera measurement says the eyes sit 0.0640 m apart
/// against an expected 0.0640 with no vertical disparity, so the stereo placement is correct and this is
/// something else. What is established is only that these four advance and disagree while their neighbours do
/// not. Equalising them is therefore a test, and its failure mode is obvious: if they are geometric rather than
/// temporal, MAIN's projection visibly breaks.
///
/// Captured from the second view, which runs first, and applied to MAIN in the same frame -- the same direction
/// the cascade lend needed for the same reason.
///
/// ANSWERED, AND THE ANSWER IS GEOMETRIC. With MAIN forced onto the other eye's values the shadows slid and
/// snapped back about once a second and the MENU came up shifted by half a screen. A menu is drawn straight
/// through the same projection, so a half-screen displacement says these four scalars are part of the transform
/// and not a temporal offset riding on it. The failure mode was written down before the test and it is exactly
/// what happened, which is the one good thing about it.
///
/// So: whatever advances in them every frame, they carry geometry, and forcing them across the eyes is not
/// available. DEFAULT 0, and it stays 0 -- kept only because the four field indices and their measured values
/// are the record of what was ruled out.
#[no_mangle]
pub static mut CyberpunkVR_JitterFix: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugJitterFixes: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugJitterUnstable: u64 = 0;
/// ---- THE CASCADE SAMPLING MATRIX: the half of the shadow setup the fit lend never reached --------
///
/// Measured in RenderDoc on two independent captures, and it resolves a contradiction this file already
/// records a few hundred lines down: the sun-cascade atlas is SHARED by the eyes -- one texture, written
/// exactly once per frame by a single copy before either eye's pass -- and the cascade fit record is identical
/// after the lend in ViewReuse.cpp ([cascrec] "differing dwords (0)", [cascfit] after=0.0000 m). By that
/// reasoning the two eyes read one shadow map identically. They do not, and the dither hunt that followed from
/// the same reasoning came back innocent.
///
/// The missing half is the transform each eye uses to READ that shared atlas. The mask pixel shader declares
/// its own constant layout, so these offsets are not guesses:
///
/// b13 CSConstants 928 B = float4 x6 | cascadeMatrix[4] | cascadeVec[4] | ... | SDistantShadowParams | ...
///
/// cascadeMatrix maps an ABSOLUTE world position into the atlas -- verified by transforming the camera position
/// through it and landing inside the unit cube -- so it cannot legitimately depend on which eye is looking. It
/// does. Twelve of the block's 58 float4 rows differ between the eyes, the same twelve in both captures, and
/// cascadeMatrix[0] and [1] are among them:
///
/// cascade 0 (12 m across, 1 texel = 5.9 mm)    lateral -5.71 / -4.83 texel    depth +58.1 mm
/// cascade 1 (40 m across, 1 texel = 19.5 mm)   lateral -8.34 / +3.39 texel    depth +58.2 mm
///
/// The lateral part changes between captures because the fit is snapped to the texel grid, so what survives
/// there is rounding. The depth part does not change: 58.1 / 58.2 mm in one capture, 58.1 / 58.4 in the other,
/// against a geometric prediction of 58.1 mm -- the measured 0.0640 m eye separation projected onto the sun
/// direction. So the cascade frame follows each eye's own camera, and each eye tests the shared atlas against
/// depths biased by that projection.
///
/// cascadeMatrix[2] and [3] are all zero in both captures, which is the same answer the count probe in
/// ViewReuse.cpp reached from the other side: there are two cascades, and the atlas has two array slices.
///
/// DIRECTION. Captured from the second view, which runs first, and applied to MAIN -- the same direction the fit
/// lend needs and for the same reason. The atlas is rasterised with the second view's fit, because that is what
/// the fit lend hands MAIN, so the second view's sampling matrix is the one that matches the pixels in it.
///
/// Mode 1 replaces the two cascade matrices and nothing else. Mode 2 replaces the whole 928-byte block, kept as
/// a separate measurement rather than a wider default because the other ten differing rows are not identified.
///
/// FAILURE MODE, written down before the test. If these matrices are not what samples the atlas, MAIN's shadows
/// will move wholesale rather than settle -- a visible slide, or a constant offset in one eye. That is the shape
/// CyberpunkVR_JitterFix produced above, and the remedy is the same: set this to 0.
#[no_mangle]
pub static mut CyberpunkVR_CascSampleLend: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_DebugCascSampleLends: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugCascSampleSkips: u64 = 0;
/// ---- ONE CASCADE CAMERA FOR BOTH VIEWS, which is where the evidence ended up pointing -------------
///
/// The reading in the headset is what settles the direction, and it is worth more than the three lends before it:
/// lending only the SAMPLING matrix reproduces the bit-50 artefact exactly. Bit 50 makes a view emit no cascade
/// passes at all, so it samples an atlas somebody else rasterised. Handing MAIN a foreign sampling matrix looks
/// identical to that, so whatever draws the atlas is aligned with MAIN and not with the second view -- and the
/// cascade record this port already equalises ([cascfit] after=0.0000 m) is therefore not what aligns it.
///
/// PRECISELY WHAT DRAWS IT, from a capture taken after the fix (cpvr_frame13113): ONE pass per frame, not one
/// per view. Forty depth-only DrawIndexedInstanced into the cascade target, events 23735..23879, sitting in the
/// gap between the two views' graphs and immediately before MAIN's; the atlas the masks sample is not written in
/// that frame at all. So the pass is single and it runs with the LAST cascade camera uploaded before it, which
/// is MAIN's -- which is why MAIN's own sampling matrix used to match and the second view's did not. Lending the
/// camera moves that single rasterisation onto the second view's frame, and lending the matrix puts both eyes on
/// it. "A view samples the atlas it rasterised itself" was the wrong way to say this: there is only one
/// rasterisation, and what matters is that the camera and the sampling matrix belong to the SAME view.
///
/// What does determine it is the 848-byte CameraShaderConsts the cascade node uploads per cascade, and the
/// census has been saying so all along: 848B@RenderShadowCascade M=8844 V=9782 d=18316 -- uploaded twice per
/// view per frame, once per cascade, contents differing between the views. Give both views one camera there and
/// the two rasterisations become the same picture, which is the only state in which either view's sampling
/// matrix can be correct.
///
/// Keyed by CASCADE INDEX, not by (node, size). Two cascades upload this block under one node, and pairing them
/// by node and size alone would copy cascade 1's camera over cascade 0 -- guaranteed to read as "differs"
/// whatever the truth is, which is the note already written against this exact census row.
///
/// Whole block, per the request: cameras and matrices alike, nothing left per-view to disagree about.
///
/// FAILURE MODE. This block is CameraShaderConsts, and CyberpunkVR_JitterFix above proved that forcing its
/// fields across the eyes at the PLAYER passes breaks geometry -- a menu displaced by half a screen. Here it is
/// forced only at the cascade node, whose consumers are the shadow rasterisation, so the blast radius should be
/// the shadow map alone. If it is not, the recognisable failure is shadows drawn from visibly the wrong place
/// or missing outright, not a shifted image; set this to 0.
#[no_mangle]
pub static mut CyberpunkVR_CascRenderLend: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_DebugCascRenderLends: u64 = 0;
/// ---- THE DITHER LOCK, on BOTH uploaders ----------------------------------------------------------
///
/// Reopening a question I closed too early. The alpha-test dither of the cascade pass was declared innocent
/// on the strength of mode 5, which forces the slice to a constant in every frame-constants block -- but
/// mode 5 lives in the BUFFER uploader only, and the cascade node uploads no 480-byte block of its own, so
/// which instance of b0 its pixel shader reads was never established. A test that may never have reached its
/// target is not a negative result. That is the fourth time today a probe has been narrower than the thing
/// it was measuring, and this time the fix is to cover both paths rather than to reason about which one.
///
/// AND THE NEW MEASUREMENT MAKES IT MATTER. The depth-target probe says both eyes rasterise the sun cascades
/// into the SAME atlas (casc0/casc1: identical DSV descriptors for both views), from a cascade record the
/// in-frame diff shows identical. So the eyes already share one shadow map -- there is nothing to hand over,
/// which answers "pass the shadows from MAIN to the second view" -- and the only way that shared map can
/// still hold different content at the two lighting passes is if the two rasterisations DISAGREE about which
/// alpha-tested pixels to keep. Order in a frame is: second view clears, draws, lights; then MAIN clears,
/// draws, lights. Each eye samples its own rasterisation. Give them different dither slices and grass -- the
/// most finely alpha-tested thing in the scene -- lands differently in the two eyes.
///
/// The slices ARE different today, and by construction: [28].x/[28].y were both measured to advance in the
/// two views with different values, and mode 4 hands the second view MAIN's block from the PREVIOUS frame
/// (the second view runs first), so the mirroring cannot equalise them within a frame.
///
/// Locking both to one constant sidesteps the ordering entirely and is the strongest form of the test. Its
/// visible cost while on: the noise pattern stops animating, so alpha-tested foliage may show a fixed dither
/// texture instead of a shimmer. That is a diagnostic cost, not a shipping one.
///
/// RESULT, AND IT ALSO RETRACTS THE REASON FOR BUILDING IT. The counters say the constant uploader carries
/// NONE of these blocks -- DitherLockBuf = 465242, DitherLockCb = 0 -- so the 480-byte frame constants travel
/// the buffer path only, mode 5 had already covered every instance of them, and my "the test may never have
/// reached its target" was wrong. With the slice locked on both paths the eyes still disagree on grass, so
/// the cascade pass's alpha-test dither is genuinely innocent, this time on a test that provably reached
/// everything.
///
/// DEFAULT 0: a locked slice means static noise, which is a real visual cost for no benefit.
#[no_mangle]
pub static mut CyberpunkVR_DitherLock: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugDitherLockBuf: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugDitherLockCb: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugAsymAoUploads: [u32; 16] = [0; 16];
/// last engine block per view
#[no_mangle]
pub static mut CyberpunkVR_DebugAsymAoBlock: [[f32; 64]; 2] = [[0.0; 64]; 2];
#[no_mangle]
pub static mut CyberpunkVR_DebugExpoUploads: [u32; 16] = [0; 16];
/// The node's last blocks as uploaded by the engine, per view: [side][0] the luminance image's 24 bytes,
/// [1] the 28-byte blocks (last of them), [2] the adaptation's 56 bytes. 16 floats each.
#[no_mangle]
pub static mut CyberpunkVR_DebugExpoBlocks: [[[f32; 16]; 3]; 2] = [[[0.0; 16]; 3]; 2];
/// What the reflection nodes upload, as CyberpunkVR_DebugAsymAoUploads for the cone AO: [i] = size
/// (| 0x80000000 on the constant uploader, | 0x40000000 in the planar node), [8 + i] = count; the last block of
/// the size the merge takes is in CyberpunkVR_DebugSsrBlocks / DebugSsprBlocks once Prepare accepts it.
#[no_mangle]
pub static mut CyberpunkVR_DebugSsrUploads: [u32; 16] = [0; 16];
#[no_mangle]
pub static CyberpunkVR_DebugCamBlockNext: AtomicU32 = AtomicU32::new(0);
/// Live-settable so the field can be A/B-ed without a rebuild. 0x640 = viewData+1600, the
/// grading-settings pointer sub_140C944A4 reads EVERYTHING out of (+224..+248). 0xF88 was
/// the first guess and is measurably not it: lending it fired 6776 times and changed
/// nothing, because sub_140C944A4 does `if (!v9) a4 = -1.0f` and throws that float away.
#[no_mangle]
pub static mut CyberpunkVR_GradingSrcOff: u32 = 1600;
/// Parked at 0 while the composer probe says what actually differs: 0x640 is the same
/// object for both views and 0xF88 is thrown away by the composer.
#[no_mangle]
pub static mut CyberpunkVR_GradingSrcLend: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugGradingLends: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugGradingSrcMain: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugGradingSrcVrcam: u64 = 0;
/// 0 = only fill a hole (never displace), 1 = also replace a pointer the view already has.
/// 0x640 IS populated for the second view -- with its own ungraded settings -- so filling alone
/// would do nothing here; the whole point is to substitute, scoped to the one call.
#[no_mangle]
pub static mut CyberpunkVR_GradingSrcDisplace: u32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_DebugGradeMirrors: u64 = 0;

// ---- src/Stereo/Hud.cpp

#[no_mangle]
pub static mut CyberpunkVR_DebugHudNodeNames: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugHudRearms: u64 = 0;
/// OFF. It was a bad trade and the log says so.
///
/// Clearing g_hud_node_named turns descriptor matching back on, and that test cannot tell the HUD
/// from any other mip-chained RGBA8 render target -- the inventory's character portrait among them,
/// as the comment beside the hold has always said. Measured after one inventory close: four
/// different surfaces adopted twenty-odd times each, the composite flipping in and out nineteen
/// times, and the headset dropping to mono about as long after as CyberpunkVR_HudHoldMs runs.
///
/// Before the re-arm the composite died once after a menu and stayed dead: the second eye lost its
/// HUD, which is bad. After it, the picture strobes and then loses the second eye entirely, which
/// is worse. Stable-and-wrong beats unstable-and-wrong until the actual cause is found, so this
/// defaults off and stays switchable for the next attempt at it.
#[no_mangle]
pub static mut CyberpunkVR_HudRearmOnGraphChange: i32 = 0;
/// How long a producing surface is protected from being replaced by a descriptor match. The
/// descriptor test cannot tell the HUD from the inventory's character portrait -- both are
/// mip-chained RGBA8 render targets -- but LIVENESS can: the real HUD is the one that keeps
/// yielding snapshots. A newcomer only takes over once the incumbent has gone quiet, which is
/// exactly what a genuine graph rebuild or resolution change looks like. Self-correcting too: if
/// the wrong surface ever gets in, it produces nothing, the hold lapses and the right one wins.
#[no_mangle]
pub static mut CyberpunkVR_HudHoldMs: u32 = 1500;
#[no_mangle]
pub static mut CyberpunkVR_DebugHudHolds: u64 = 0;
/// A frame that takes longer than this -- a load spike, a heavy menu -- would blink the HUD off,
/// so the window is generous: it only has to be shorter than a menu, not shorter than a hitch.
#[no_mangle]
pub static mut CyberpunkVR_HudMaxAgeMs: u32 = 1000;
/// The scanner's object outline, as the SECOND eye draws it. Not copied from MAIN: the outline
/// traces on-screen silhouettes, so MAIN's would sit at MAIN's parallax and read as double
/// vision. VRCAM renders its own -- Resource_83328 at 2444x2560 in EventList_SCANERELEMENTS,
/// against MAIN's Resource_85164 -- it is simply never composited, because the chain that turns
/// it into a visible outline (PS587 -> PS1047 -> PS1290 -> the PS1216 composite) is MAIN-only,
/// behind the same viewData+0x168 gate as the HUD.
#[no_mangle]
pub static mut CyberpunkVR_VisionSnap: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_DebugVisionSnaps: u64 = 0;
/// HOW LONG THE LAST OUTLINE STAYS USABLE, and it is the HUD's number and the HUD's escape hatch
/// rather than a constant, because the two are the same problem. It was hardcoded at 250 ms, and
/// that is what made the outline leave the second eye a moment after each scan: MAIN composites
/// from the LIVE surface every frame and does not care whether the compute pass ran again, while
/// the second eye works from a snapshot and the pass only runs while something needs redrawing.
/// A quarter of a second after the scan settled, our copy aged out and MAIN's did not.
/// 0 = no limit.
#[no_mangle]
pub static mut CyberpunkVR_VisionMaxAgeMs: u32 = 1000;
/// qword index
#[no_mangle]
pub static mut CyberpunkVR_HudCapWord: u32 = 11;
/// bit 7 (=711)
#[no_mangle]
pub static mut CyberpunkVR_HudCapBits: u64 = 128;
#[no_mangle]
pub static mut CyberpunkVR_DebugHudCapGrants: u64 = 0;

// ---- src/Stereo/Mirror.cpp

#[no_mangle]
pub static mut CyberpunkVR_DebugMirrorSrcState: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMirrorBarrierHits: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMirrorPendingHits: u64 = 0;

// ---- src/Stereo/NativeBoundsTrace.cpp

#[no_mangle]
pub static mut CyberpunkVR_BoundsTraceFormat: u32 = 2;

// ---- src/Stereo/NativeDefinitionTracking.cpp

#[no_mangle]
pub static mut CyberpunkVR_OwnedResourceReplayRowBytes: u32 = 32;

// ---- src/Stereo/NativePairedFrame.cpp

#[no_mangle]
pub static mut CyberpunkVR_PreparationSharedOwnerRowBytes: u32 = 32;

// ---- src/Stereo/NodeDispatch.cpp

#[no_mangle]
pub static mut CyberpunkVR_DebugRenderMaskGrants: u64 = 0;
/// THE SAME RESULT WITHOUT THE LOCK. This runs on every VRCAM node and every scope node, so the
/// mutex above made the render jobs of both views queue on one lock, and the scope path rewrote
/// all 256 bytes of the engine's view mask on every node even when nothing was missing -- racing
/// the engine's own writes to those words. The eye path now compares against the last snapshot
/// this thread saw and locks only when the eye's mask changed; the scope path keeps a per-thread
/// copy of the shared mask (refreshed by generation) and ORs in only the words that are missing.
/// 0 = the original locked path, kept for A/B.
#[no_mangle]
pub static mut CyberpunkVR_MaskShareFast: i32 = 1;
/// Cost instrument: 1 = accumulate QPC ticks and calls of every share below.
#[no_mangle]
pub static mut CyberpunkVR_MaskShareTiming: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_MaskShareTicks: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_MaskShareCalls: u64 = 0;
/// The whole per-node pre-work of a scope node (grants, weapon-plane clear, viewData fill).
#[no_mangle]
pub static mut CyberpunkVR_OpticsPreworkTicks: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_OpticsPreworkCalls: u64 = 0;
/// THE SCOPE CAMERA MUST NOT SEE THE PLAYER'S OWN FIRST-PERSON PLANE. Its RTT view is a camera on
/// the player, and the engine grants it RenderMask/WeaponPlane every frame like any view (native
/// word 11 = 0xcf6e3b, bit 0 set): the gun and arms were drawn into the magnified image with the
/// weapon plane's own projection -- the barrel rising to the image centre at 4.4x, far above where
/// its real geometry would sit. Measured live 2026-10-13: holding the WeaponPlane-only bits clear
/// in the view mask removes the gun from the raw camera image and nothing else. Cleared at every
/// scope node, after the grants above, since the engine rewrites the mask once per frame.
#[no_mangle]
pub static mut CyberpunkVR_OpticsHideWeaponPlane: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_DebugCapDenies: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugCapGrants: u64 = 0;
/// ---- WHICH FEATURE BITS THE SHADOW-MASK PASS ASKS ABOUT, and whether the two views answer alike -----
///
/// The sun-shadow mask is the last part of the chain still unexplained: everything upstream is measured
/// identical between the eyes (same casters, same cascade records, both cascades rendered by both views,
/// same wind, same frame constants), and every remaining asymmetry in the small-block census is one of this
/// port's own reuse knobs. Its pass, CRenderNode_RenderShadowmask, branches on feature bits --
///
/// if (feature(0x2C)) { if (feature(0x2D)) return; }      // 44 then 45: an early-out
/// if (feature(0x2C)) { ... feature(0x0E) ... }           // 44 then 14
/// ... feature(0x14), feature(0x0D) ...
///
/// -- so if a bit answers differently for the two views, that eye takes a different path through the mask
/// and the difference needs no exotic explanation. This measures the answers before anything is forced;
/// forcing a bit whose meaning is unmeasured is what the cascade attempts cost.
///
/// ANSWERED, IN GAMEPLAY, AND THE ANSWER IS NO. Every bit the pass asks about answers the same for both
/// views, and the per-interval deltas are identical (408 each), so the two eyes run the pass at the same
/// rate down the same branch -- the standing gap in the totals is a constant accumulated before gameplay:
///
/// bit9  M=10966/0  V=15809/0        both always YES (asked twice per pass)
/// bit44 M=0/16449  V=0/23712        both always NO  -- the early-out is never taken by either view
/// bit20 M=5483/0   V=7904/0         both always YES
/// bit13 M=5483/0   V=7904/0         both always YES
/// bit14 M=5483/0   V=7904/0         both always YES
///
/// So the shadow-mask pass is not where the eyes diverge either. What is left is structural and is written
/// up at CyberpunkVR_CheckerProbe in Grading.cpp: the mask is evaluated at HALF WIDTH on an interleaved
/// pattern addressed in each eye's own screen space.
///
/// THIS PROBE ALSO COST A RUN BY BEING ATTACHED TO THE WRONG FUNCTION, which is why the two are now named
/// apart in EngineRvas.hpp: sub_14021BE28 answers about a render-mask DESCRIPTOR (that is the HUD/AutoGrass
/// gate), sub_14033E358 answers about a BIT INDEX (that is what the nodes ask). A silent probe is not a
/// negative result, and the yes/no counters are printed so an empty answer can be told from an unreached one.
#[no_mangle]
pub static mut CyberpunkVR_FeatBitProbe: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_FeatBitNode: u32 = 7830796;
#[no_mangle]
pub static CyberpunkVR_AutoGrassReadiness: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];
#[no_mangle]
pub static mut CyberpunkVR_WeaponPassListsFiltered: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugSkyIdxMain: u32 = 65535;
#[no_mangle]
pub static mut CyberpunkVR_DebugSkyIdxVrcam: u32 = 65535;
#[no_mangle]
pub static mut CyberpunkVR_CloudLightMirror: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugCloudSelMain: u32 = 4294967295;
#[no_mangle]
pub static mut CyberpunkVR_DebugCloudSelVrcam: u32 = 4294967295;
#[no_mangle]
pub static mut CyberpunkVR_DebugCloudSelWrites: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_OpticsRenderProbe: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_OpticsFreezeCopy: i32 = 0;

// ---- src/Stereo/RenderGraphShape.cpp

/// Nonzero = record; a new value records every graph again.
#[no_mangle]
pub static CyberpunkVR_GraphShapeGen: AtomicU32 = AtomicU32::new(0);

// ---- src/Stereo/ScopeGraphDag.cpp

/// 0 not installed, 1 installed, 2 the site did not match.
#[no_mangle]
pub static CyberpunkVR_ScopeGraphDagState: AtomicU32 = AtomicU32::new(0);

// ---- src/Stereo/SinglePass.cpp

/// on (measured: docs/vr-render-optimization-20261104.md)
#[no_mangle]
pub static CyberpunkVR_SinglePass: AtomicU32 = AtomicU32::new(1);
/// RenderElements; 0 = every node
#[no_mangle]
pub static CyberpunkVR_SinglePassNode: AtomicU32 = AtomicU32::new(3399076);
#[no_mangle]
pub static CyberpunkVR_SinglePassCounts: [AtomicU64; 10] = [const { AtomicU64::new(0) }; 10];
/// 1: while left draws are kept, the engine's state calls are not sent on: the next draw (or command that reads them)
/// gets the difference to what the list has. The kept left batch's own state calls then never reach the driver.
#[no_mangle]
pub static CyberpunkVR_SinglePassLazy: AtomicU32 = AtomicU32::new(1);
/// What sent kept left draws before their partner: [slot] a command-list method (its vtable slot), [88] Flush(),
/// [89] no originals, [90] a draw outside the node, [91] a draw at another viewport, [92] a left draw whose twin is
/// not ready, [93] a right draw without a kept partner, [94] a right draw whose partner differs, [95] a new left batch.
#[no_mangle]
pub static CyberpunkVR_SinglePassFlushBy: [AtomicU64; 96] = [const { AtomicU64::new(0) }; 96];
/// Why a right draw's kept partner did not match: [0] pipeline, [1] kind/arguments, [2] root/tables set/topology,
/// [3] index/vertex buffers, [4] stencil/blend/VRS, [5] viewport, [6] scissor, [7] camera tables unset.
/// [8] blend factor, [9] VRS rate, [10] VRS image
#[no_mangle]
pub static CyberpunkVR_SinglePassMismatch: [AtomicU64; 12] = [const { AtomicU64::new(0) }; 12];
/// PROTOTYPE of GPU occlusion culling for the G-buffer's view-instanced draws (docs/vr-render-optimization-20261104.md,
/// "GPU occlusion culling prototype"); off by default. 2: a binary occlusion query around every instanced draw, nothing
/// culled (the queries' own cost). 1: the same, and a draw found empty in both eyes (one query covers both views of a
/// view-instanced draw: conservative across the eyes) the last frames it was drawn is predicated away
/// (SetPredication EQUAL_ZERO on its last result). Every CyberpunkVR_OcclusionCullRetest-th frame nothing is
/// predicated, so culled draws are tested again: a draw that comes into view can be late by up to that many frames
/// (pop-in). A draw is known by its pipeline, buffers, arguments and its occurrence in the frame.
#[no_mangle]
pub static CyberpunkVR_OcclusionCull: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_OcclusionCullRetest: AtomicU32 = AtomicU32::new(8);
/// [0] queried draws, [1] predicated draws, [2] slots, [3] resolve runs, [4] lists resolved, [5] failures,
/// [6] lists reset with unresolved queries
#[no_mangle]
pub static CyberpunkVR_OcclusionCullCounts: [AtomicU64; 8] = [const { AtomicU64::new(0) }; 8];

// ---- src/Stereo/StereoPairProbe.cpp

/// Frames to record (1..8); 0 when taken. State: 0 idle, 1 armed, 2 recording, 3 done, 4 not installed.
#[no_mangle]
pub static CyberpunkVR_PairProbeRequest: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PairProbeState: AtomicU32 = AtomicU32::new(0);
/// RenderElements; 0 = every node
#[no_mangle]
pub static CyberpunkVR_PairProbeNode: AtomicU32 = AtomicU32::new(3399076);
#[no_mangle]
pub static CyberpunkVR_PairProbeCount: AtomicU32 = AtomicU32::new(0);
/// Single-pass feasibility (StereoPairProbe.hpp): root parameters (bit n = parameter n) whose tables a right-eye draw
/// takes from its left partner; 0 = off. 1 = also the partner's start instance.
#[no_mangle]
pub static CyberpunkVR_PairSubstituteMask: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PairSubstituteMaskHigh: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PairSubstituteInstance: AtomicU32 = AtomicU32::new(0);
/// [0] right-eye draws substituted, [1] right-eye draws without a matching partner, [2] left-eye draws kept,
/// [3] tables substituted.
#[no_mangle]
pub static CyberpunkVR_PairSubstituteCounts: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];

// ---- src/Stereo/SubmitChain.cpp

/// 0 off, 1 chained (every view job also waits for the chain), 2 chained (only Present waits). Set from
/// xr_submit_chain (src/Core/LiveControlsPoll.cpp) when the setting changes; writable live for A/B tests.
#[no_mangle]
pub static CyberpunkVR_SubmitChain: AtomicU32 = AtomicU32::new(0);
/// [0] submission points chained, [1] of them submitted, [2] Present waits that had to wait, [3] their
/// QPC ticks, [4] view-job waits that had to wait, [5] their ticks, [6] waits that gave up after 500 ms,
/// [7] drains after switching off.
#[no_mangle]
pub static CyberpunkVR_SubmitChainStats: [AtomicU64; 8] = [const { AtomicU64::new(0) }; 8];
/// The dispatcher queue (0..3, 3 served first) the chained Close/Submit/JoinFrameSync jobs go to; -1 keeps
/// the caller's (render nodes run from queue 1, the world tick's jobs from queue 2). Live for A/B tests.
#[no_mangle]
pub static CyberpunkVR_SubmitChainPriority: AtomicI32 = AtomicI32::new(-1);
/// [0] presents deferred, [1] of them done, [2] Present nodes that waited for the previous one, [3] their
/// QPC ticks, [4] view jobs that waited for the deferred present, [5] their ticks, [6] presents run in the
/// node (not yet steady: start-up, menus, loading, a changed output), [7] waits that gave up after 500 ms.
#[no_mangle]
pub static CyberpunkVR_DeferredPresentStats: [AtomicU64; 8] = [const { AtomicU64::new(0) }; 8];
/// The steady-state gate, last presenting node: [0] presents with the same output, [1] presents since a
/// graph last ran the node without presenting, [2] output changes, [3] output handle changes.
#[no_mangle]
pub static CyberpunkVR_DeferredPresentGate: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];
/// Render-packet runs (sub_140334CC0) that waited for a deferred present: [0] count, [1] QPC ticks.
#[no_mangle]
pub static CyberpunkVR_DeferredPresentPacketWaits: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];

// ---- src/Stereo/UpscalerViews.cpp

/// 1 = the second eye uses its own context (default); 0 = both eyes share the engine's, as before.
#[no_mangle]
pub static mut CyberpunkVR_UpscalerPerEye: i32 = 1;
/// Per library (0 FSR 3.0, 1 FFX API, 2 XeSS): second contexts created, first-eye dispatches,
/// second-eye dispatches given their own context, failures (create or install).
#[no_mangle]
pub static CyberpunkVR_UpscalerPerEyeStats: [[AtomicU64; 4]; 3] = [const { [const { AtomicU64::new(0) }; 4] }; 3];

// ---- src/Stereo/ViewConstants.cpp

/// One node's block, both views, in full. Live-settable so the next candidate costs no rebuild.
/// LightChannelVolumes
#[no_mangle]
pub static mut CyberpunkVR_CbvDumpNode: u32 = 4258924;
#[no_mangle]
pub static mut CyberpunkVR_CamPairMaxMs: u32 = 40;
#[no_mangle]
pub static mut CyberpunkVR_DebugCamPairs: u64 = 0;

// ---- src/Stereo/ViewReuse.cpp

#[no_mangle]
pub static mut CyberpunkVR_DebugDistantSkipHits: u64 = 0;
/// 3 = mirror ONLY the three noise-layer offsets -- the measured difference  [default, proven]
/// 0 = off (engine's own per-view constants, i.e. the broken-stereo baseline)
/// 1 = mirror MAIN's buffer, keep VRCAM's own view size + jitter   -- kills VRCAM's clouds
/// 2 = mirror MAIN's buffer verbatim                               -- kills VRCAM's clouds
#[no_mangle]
pub static mut CyberpunkVR_CloudCbMode: u32 = 3;
#[no_mangle]
pub static mut CyberpunkVR_DebugCloudCbMain: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugCloudCbVrcam: u64 = 0;
/// ONLY 0x40. The buffer's other differing fields are per-view by construction and the mode-1
/// path below already says so: +152 is this view's width/height and +168 its jitter, both
/// deliberately preserved there. 0xA8 IS +168, so mirroring it -- which the first version of
/// this did -- hands VRCAM MAIN's jitter. 0x90 (+144) is the resolved cloud-target size, which
/// is why it reads {M 1 | V 512}. +0x8C/+0xA0/+0xA4 are transient descriptor indices and
/// mirroring the buffer wholesale is already known to kill VRCAM's clouds outright.
/// bit 0  0x40..0x4F -- the WHOLE float4, not just the first dword. The diff prints where
/// a run starts, never how long it is, and 4 bytes was the same off-by-one that cost
/// five rounds on viewData. The raymarch reads _40_m0[4].w -- byte 0x4C -- as the
/// light-intensity multiplier: `_412 = _40_m0[4].w * _30_m0[6].x` for each channel,
/// i.e. a flat scale on the cloud colour. That is the shape of "always lighter".
#[no_mangle]
pub static mut CyberpunkVR_CloudCbExtra: u32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_DebugCloudCbExtra: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugViewParamsRuns: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugViewParamsSeen: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugViewParamsEntries: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugViewParamsBusy: u64 = 0;
/// WALKED versus APPLIED, and why the difference. Measured 2026-08-30: across a whole session the
/// ratio of entries applied to runs is exactly 2.00 for minutes at a time and then exactly 3.00 for
/// the rest, with lock busy at 0 and runs never stalling -- and the braindance tint is present in
/// the 3.00 window and absent in the 2.00 one. So the pass runs in both and is one override short
/// in the first, and the only question left is whether that entry is missing from the stack we read
/// or present and dropped by one of the two skips below.
#[no_mangle]
pub static mut CyberpunkVR_DebugViewParamsWalked: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugViewParamsSkipNoObj: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugViewParamsSkipErase: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugViewDataFixes: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugFogMirrors: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugEnvMirrors: u64 = 0;
/// Every eight bytes the guard refused because one side held an address. Not an error: it is how
/// a range is allowed to span a pointer at all. A number that climbs with the frame rate means a
/// range is mostly pointers and was chosen badly.
#[no_mangle]
pub static mut CyberpunkVR_DebugEnvMirrorPtrSkips: u64 = 0;
/// Internal A/B switch: both paths apply the same ranges and pointer filters.
#[no_mangle]
pub static mut CyberpunkVR_ViewDataBatchSnapshot: u32 = 1;
/// THE SCOPE VIEW'S FILL, ONCE PER CHANGE INSTEAD OF ONCE PER NODE. The scope view dispatches about
/// 600 nodes a frame and every one ran the whole fill (read-compare ~30 environment ranges, write the
/// holes and the fog block): ~1.2 us each, ~0.7 ms of render-job CPU per frame, measured 2026-10-14.
/// What the fill writes changes only when MAIN is captured again (g_vd_gen), when the scope's block
/// moves, or when the engine copies parameters into a view block (g_vd_write_epoch), so the fill runs
/// when that triple changes and is skipped otherwise. Every 64th skipped call compares the block with
/// what a fill would write; a difference (CyberpunkVR_OpticsVdStale) re-fills at once, so a writer we
/// have not named costs at most 64 nodes and is counted rather than hidden. 0 = fill on every node.
#[no_mangle]
pub static mut CyberpunkVR_OpticsVdFillGate: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_OpticsVdFills: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_OpticsVdSkips: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_OpticsVdVerifies: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_OpticsVdStale: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugLocalShadowSkipHits: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugViewRectReads: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugProbeSkipHits: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugProbeNodeEntries: u64 = 0;
/// ================================================================================================
/// THE FOLIAGE WIND IMPULSE -- one write, and the measurement behind it.
///
/// Symptom (2026-08-17): standing in a bush made it twitch, and turning the head made all of MAIN's
/// vegetation jitter while the second eye stayed perfect. The decisive datum came from the headset:
/// switch the SECOND view off and MAIN is correct. So MAIN was never short of data -- it was affected
/// by the mere existence of the other view.
///
/// THE PASS. CRenderNode_WindImpulseVolumeUpdate (sub_1407F2BF0) is the only thing that injects the
/// repellers -- actors standing in vegetation, gathered by the game-side tick group the exe names
/// "RepellerComponents/FoliageRepeller" -- into the volume the foliage shaders read. It decides
/// whether to do anything from two gates:
///
/// windState = *(*(wc + 0x20) + 0xB0)                      // ONE object, shared by both views
/// if ((wc[0x30] & 2) == 0)                       -> skip  // not the view that owns shared work
/// if (!latch(windState + 0x1C, renderer+0x4CA4)) -> skip  // already done this frame
///
/// The latch is an InterlockedExchange of the frame id, so EXACTLY ONE view per frame gets through --
/// correct for a game with one view, and the whole of this bug for two. Measured in x64dbg: the body
/// is entered by VRCAM (view key at ctx+0x28 non-zero) in 5433 frames out of 5791, while MAIN arrives
/// with the owner bit set, finds the frame already claimed, and jumps over everything.
///
/// AND THE ANSWER IS NOT WHAT THE VOLUME CONTAINS BUT WHEN IT IS WRITTEN. Each of these was built,
/// measured to reach MAIN, and left the jitter exactly where it was:
///
/// * replaying the pass's 48-byte constant buffer for MAIN, captured in the uploader, same frame
/// (2987 extra runs, 2988 replays, 0 stale) -- MAIN still jittered;
/// * rebuilding those 48 bytes from the shared state instead of copying them -- and a self-check
/// against the engine's own upload showed the built block differing by exactly one scroll step,
/// because the state advances between the two dispatches. Fresh is the wrong thing to be here:
/// the volume was written with the OTHER view's numbers;
/// * replaying the pass's five resource bindings (51656 recorded, 4119 replayed);
/// * reopening the same latch on CRenderNode_AdvanceSpeedTreeWind, the global wind (4121 extra runs);
/// * running the whole pass for both views with the clock restored, the ageing suppressed and the
/// impulse records snapshotted and put back -- which made BOTH eyes twitch and cost frame rate.
///
/// That last one produced the trace worth keeping. A hardware write breakpoint on the impulse count at
/// windState+0x104 caught two writers:
///
/// 0x7A2016  mov  [rdi+0x0C], esi     the game ADDS an impulse (esi=1, capacity 0xFF)
/// 0x7A26E8  dec  [rcx+0x0C]          the pass RETIRES one, and MOVES records to close the gap
///
/// so restoring the count alone re-injects a record that has been shuffled -- and restoring the bytes
/// as well still twitches, because the second dispatch itself is what one shared volume cannot
/// survive.
///
/// WHAT IT ACTUALLY IS. The claim is taken on the CPU by whichever view's node runs first, but the two
/// graphs do not reach the GPU in that order: MAIN's draws are submitted before the second view's
/// dispatch, so MAIN samples the volume as of the PREVIOUS frame. Correct data, one frame old --
/// invisible standing still, and exactly a jitter while the head turns. Switching the second view off
/// makes MAIN claim the latch itself, which puts its dispatch ahead of its own draws, which is why
/// that looked right.
///
/// THE FIX is to hand the claim to MAIN: deny it to the second view by pre-claiming the frame, grant it
/// to MAIN by rewinding one. One update per frame, no second dispatch, no frame-rate cost, and the view
/// that draws later reads the same fresh volume. Confirmed in the headset: no difference between the eyes.
///
/// `CyberpunkVR_FoliageWindMode`: 1 = this, 0 = the engine's own behaviour. Live-flippable, which is how
/// all of the above was walked.
#[no_mangle]
pub static mut CyberpunkVR_FoliageWindMode: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_DebugWindClaimMain: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugWindDenyVrcam: u64 = 0;
/// ================================================================================================
/// THE OTHER WIND -- and the one that actually moves the vegetation.
///
/// Everything above concerns the IMPULSE volume: the local push of an actor standing in a bush. It is
/// now measured not to be the twitch: MAIN received the engine's own same-frame constant block every
/// frame (2987 extra runs, 2988 replays, 0 stale) and still twitched on head turns. So that thread is
/// closed, and the symptom -- ALL of MAIN's grass, not just the bush the player stands in -- points at
/// the global wind instead.
///
/// CRenderNode_AdvanceSpeedTreeWind (sub_140D60084) is three lines and both of this project's usual
/// suspects are in them:
///
/// if ((wc[0x30] & 2) != 0)                          // the owner bit
/// return sub_140200B4C(*(*(wc + 0x20) + 0x80));  // the SpeedTree wind manager
/// and inside:
/// if (latch(mgr + 0x48, renderer + 0x4CA4))          // the same once-per-frame latch
/// for each wind instance: push the current wind, then sub_140334328(idx, 416, inst + 0x4538)
///
/// So one view per frame uploads 416 bytes of wind state and the other view's vegetation is left with
/// whatever was last bound to it -- stale, which is what "the grass freezes when I turn my head" is.
///
/// AND THIS ONE IS SAFE TO RUN TWICE, which is the whole difference from the impulse pass. It
/// accumulates nothing and consumes nothing: the values come from the manager (`v4+2848` scaled, and
/// the three dwords at `v4+2864..2872`), are written into each instance and uploaded. Run it again in
/// the same frame with the same inputs and it writes the same bytes. So the fix is only to reopen the
/// latch for the second view -- no snapshot, no restore, nothing held.
///
/// AND 2026-08-17 GAVE IT A SECOND, SHARPER REASON: the shadows cast by fences, plants and trees differ
/// between the eyes, while solid objects' shadows match -- so whatever it is acts only on geometry that
/// MOVES. The cascade pixel shader's three discards are all eliminated by measurement (the LOD-fade phase
/// reads 0 in both views with one distinct value seen; forcing the blue-noise slice to a constant in
/// every frame-constants block changed nothing; neutralising the height-mask test changed nothing), and
/// the cascade VERTEX shader sways foliage in the shadow pass itself out of b8 -- a wind-texture scroll
/// at b8[4].xy, a phase at b8[5].w. b8 is 26 float4 = 416 bytes, exactly what this pass uploads, and the
/// census names the owner: AdvanceSpeedTreeWind, 29638 uploads for MAIN and ZERO for the second view.
///
/// One buffer, written once per frame, read by two graphs at different times. The second view's graph
/// runs FIRST (measured: its passes at capture event 12909, MAIN's at 40420), so every frame goes
///
/// second view's cascades   read the buffer as it stands  -> wind(N-1)
/// AdvanceSpeedTreeWind     writes it                     -> wind(N)
/// MAIN's cascades          read it                       -> wind(N)
///
/// and the second eye's shadow map holds its foliage one wind step behind its own image. Invisible on
/// anything rigid, unmistakable on leaves. Reopening the latch puts the upload in the second view's graph
/// too, ahead of its cascades.
///
/// Recorded because it nearly cost the finding: this latch was reopened once before, during the bush
/// jitter hunt, and "changed nothing" -- true of THAT symptom, and the shadows were not being looked at.
/// A knob measured innocent of one fault is not innocent of the next.
/// THE WIND IS BACK IN THE FRAME, and my earlier reading of these counters was wrong. From
/// First = 8441, ExtraMain = 5870, ExtraVrcam = 0 I concluded "the second view claims the wind, so both
/// eyes read the same one". That skipped the fact this hunt had already established elsewhere: the
/// renderer's frame id ADVANCES BETWEEN THE TWO VIEWS' PASSES. The latch compares against that id, so each
/// view sees it as a new frame and each claims the wind for itself:
///
/// second view's frame:  latch = its id     -> uploads wind(N)
/// MAIN's frame:         id has moved on    -> claims it too, uploads wind(N+1)
///
/// so the grass in the two shadow maps sways one wind step apart. 8441 claims in an interval is TWO per
/// eye-pair, not one. And it fits what shows now: with everything else settled the cable's shadow
/// (rigid) matches, while grass (wind-animated) still differs.
///
/// mode 0  the engine's own behaviour: each view advances the wind in its own frame
/// mode 1  reopen the latch for the view that finds it claimed (idempotent, but pointless -- it just
/// re-uploads the same values)
/// mode 2  DENY the second view the claim, so one wind state serves the pair
///
/// Default 0 deliberately: mode 2 is the candidate and it is switched on live, one variable at a time,
/// because testing it together with the block change above would be two changes and no measurement.
#[no_mangle]
pub static mut CyberpunkVR_SpeedTreeWindMode: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugStWindFirst: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugStWindExtraMain: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugStWindExtraVrcam: u64 = 0;
/// ================================================================================================
/// THE CASCADE'S OWN MATRIX, keyed by cascade index -- the last unverified assumption.
///
/// The thin-shadow report (a cable on a building differs between the eyes) overturned my own reason for
/// excluding the cascade transform. I had argued that a placement difference would move EVERY shadow;
/// that is wrong. A cascade texel is a few centimetres and the eyes are 6.5 cm apart, so an offset of a
/// texel is invisible on a building's broad shadow and decisive on anything thinner than a few texels:
/// a cable, a fence slat, a branch. Which is exactly the set of objects reported.
///
/// What IS verified, live in the debugger: the two views' cascade passes reach the same scene manager --
/// *(view_ctx + 0x1E10) reads 0x2A926A675C0 for both -- so the cascade records are one shared object, not
/// one per view. But a shared pointer is not a shared VALUE at draw time: this project's own notes record
/// the pattern of two views rebuilding one global in turn, and if each view writes its placement before
/// its own pass, both read "their own" matrix out of the same memory.
///
/// So the matrix has to be compared AT THE MOMENT EACH VIEW DRAWS, and keyed by cascade index -- the
/// mistake that wasted three measurements today was keying too coarsely, and the cascade node uploads its
/// 848-byte block two dozen times a frame. The index is taken from the node argument the pass itself uses
/// (a1 + 24, the same read as `v12 = *(unsigned int *)(a1 + 24)`) and published in a thread-local for the
/// upload probe to key on.
/// AND THE FIELD THAT MAKES THEM DIFFER, found in the pass's own body after the plugin captured the call
/// stack at the upload (engine frames 1E37E3 153A6C 1EC46D ... -- 153A6C is inside this very node, so the
/// constants are built here rather than by a separate setup):
///
/// sub_1402130D0(v51, v13 + 801616);     // v51 = local cascade constants, from the SHARED record
/// v20 = *(float *)(v14 + 2664);         // v14 = viewData + 16 * cascade_idx  -> PER VIEW
/// v52 = v52 - v20;                      // two fields of the constants, adjusted by it
/// v53 = v53 - v20;
/// sub_140297904(a2, (__int64)v51, ...); // uploaded
///
/// So the cascade record itself is shared -- the pointer was verified identical live -- and what splits
/// the two eyes is one float per cascade taken from each view's own viewData (the getter is sub_14033C248,
/// which this port already calls elsewhere). Equalising THAT is the fix, and it has to be done in the
/// viewData rather than in the upload: the lighting pass samples with the matrix its view rendered with,
/// which is why lending MAIN's matrix to the second view's upload made grass slide with head turns.
///
/// Not restored afterwards, deliberately: the value must still read as MAIN's when the sampling side comes
/// to it later in the frame. Each view has its own viewData, so writing the second view's cannot disturb
/// MAIN, and the engine recomputes it next frame.
/// Stays ON: it only publishes the cascade index in a thread-local, which costs one store per cascade
/// node and is what any further work on this will need to key by.
#[no_mangle]
pub static mut CyberpunkVR_CascadeIdxProbe: i32 = 1;
/// The per-index dispatch count. Default 0 now that it has answered; see the note at its call site.
#[no_mangle]
pub static mut CyberpunkVR_CascCountProbe: i32 = 0;
/// DEFAULT 0: measured to read 0.060000 in both views, so it is a shared constant and lending it does
/// nothing. Kept only so the measurement can be repeated.
#[no_mangle]
pub static mut CyberpunkVR_CascadeBiasLend: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugCascBiasLends: u64 = 0;
/// ---- THE CASCADE RECORD, and which of its fields split the eyes ----------------------------------
///
/// The viewData field above was a dead end: it reads 0.060000 in BOTH views, so it is a shared constant
/// (a depth bias) and never lent anything. What actually differs was found by dumping the record itself at
/// each view's pass -- scene_mgr + 801616 + 1104 * cascade_idx, the block the pass copies its constants
/// from, at the shared pointer verified live:
///
/// +0x10  orientation quaternion            IDENTICAL in both views (the sun direction is shared)
/// +0x40  two floats, the cascade extent    VRCAM -18.0587 / 18.0587   MAIN -18.0586 / 18.0586
/// +0x80  three floats, the cascade CENTRE  VRCAM (-1959.021, -363.2735, -1442.0868)
/// MAIN  (-1958.997, -363.2810, -1442.1367)
///
/// THOSE OFFSETS ARE 0x40 AND 0x80, and the first attempt wrote 0x50 and 0x90 -- I had misread the first
/// dump by one sixteen-byte row. At 0x50 and 0x90 sit direction vectors, so the "fix" handed each view the
/// other's directions, and the bushes' shadows swung with the head. Re-read row by row against a fresh
/// dump before believing an offset; the failure was arithmetic, not the approach.
///
/// AND THEN THE IN-FRAME PROBE RETRACTED ALL OF IT. Those numbers above came from TWO DEBUGGER STOPS, in
/// different frames, and in VR the head never holds still -- so they are a measurement of head jitter, not
/// of the two views. Running the same comparison inside one frame, from the detour, reports:
///
/// [cascrec] cascade 1 record, dwords differing between the views (0): (none)
///
/// zero differing dwords across the whole 1104-byte record, repeatedly. The record is SHARED and IDENTICAL:
/// both eyes place the cascade the same way, and the "5 cm centre offset" never existed. The two failed
/// lends (this one and the deleted CascMatFix) were therefore fixing nothing and breaking the render/sample
/// agreement of whichever view received the copy.
///
/// The rule this cost: a cross-frame comparison cannot answer a per-view question. Two breakpoint hits are
/// two different frames. Only a probe that holds both views' copies from ONE frame may be believed -- and
/// that is also how the same trap was caught with the 480-byte block earlier the same day.
///
/// AND THE "RECORD IDENTICAL" READING IS NOW RETRACTED FOR THE SECOND TIME, by a probe that measures instead
/// of asking yes/no. With the head turning, the two views fit cascade centres 9-13 cm apart on cascade 0 and
/// 21-36 cm apart on cascade 1 -- five to seven texels and more, in one frame. Standing still they agree,
/// which is why a boolean taken at rest said "identical" and why "only when I turn my head" was the
/// observation that broke the case open.
///
/// Both eyes write that one shared atlas from boxes up to a third of a metre apart, so their content is
/// quantised on grids several texels offset. A shadow one or two texels wide lands on a different texel in
/// each eye (grass), and along a view ray in the volumetric fog the offset accumulates into a visible
/// horizontal displacement, opposite in the two eyes (the blocky sun shafts in the hospital-bed ending, which
/// is far more visible than the grass -- same defect, amplified by the ray).
///
/// ON, with the fit copied whole. This is the first version aimed at the measured cause rather than at a
/// guess, and CyberpunkVR_CascFitProbe is the check: if the lend works, maxCentre must fall to zero.
///
/// AND THE WARNING TWENTY LINES UP CAME TRUE, WHICH IS WHY THIS LEND NEEDS A PARTNER. "Breaking the
/// render/sample agreement of whichever view received the copy" is exactly what was then reported:
/// shadows blinking in MAIN, clean in the second view -- MAIN being the view that receives the copy. A capture
/// measured the broken half directly: MAIN rasterises with the second view's fit (this lend, [cascfit]
/// after=0.0000 m) and still SAMPLES with a matrix built from its own camera, and the two disagree by 58.1 mm
/// along the light -- the 0.0640 m eye separation projected onto the sun direction, reproduced to 0.3 mm in two
/// independent captures.
///
/// The sampling matrix is not in this record, which is why zeroing the record's difference could never fix it:
/// it lives in the 928-byte CSConstants block the frame graph uploads at PrepareSceneRendering.
/// CyberpunkVR_CascSampleLend in Grading.cpp lends it in the same direction as this one. The two belong
/// together -- lending either alone leaves one view rasterising and sampling on different grids.
#[no_mangle]
pub static mut CyberpunkVR_CascRecLend: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_DebugCascRecLends: u64 = 0;
/// ---- CASCADE REUSE, POINTED THE RIGHT WAY --------------------------------------------------------
///
/// The frame-graph route (CyberpunkVR_VrcamFlagMode = 2, feature bit 50) gave the SECOND view the reuse, and
/// it produced sliding shadows in that eye: its graph runs FIRST, so it sampled an atlas rasterised the
/// previous frame while using this frame's matrix.
///
/// That failure also settled a question the flag comment only implied: the eye that skipped its build still
/// HAD shadows -- wrong ones, not missing ones -- so the cascade atlas is one shared resource, not a texture
/// per graph. Which means reuse works in the other direction with no staleness at all: let the second view
/// (first to run) build the atlas, and skip the build for MAIN, which runs after it in the same frame. The
/// in-frame record probe already showed the cascade record is identical at both passes, so MAIN's sampling
/// matrix matches what the second view rasterised.
///
/// Both nodes have to go for MAIN, not just the render: ClearShadowCascades would otherwise wipe the atlas
/// between the second view's build and MAIN's sampling, and MAIN would lose sun shadows entirely. That is
/// also the recognisable failure if this reasoning is wrong.
///
/// Default 0. It is a single dword, so it can be armed live in the debugger without a rebuild, and its whole
/// purpose is to answer one question: the two views' cascade INPUTS are measured identical, but their
/// OUTPUTS never were. If MAIN's shadows change at all when it samples the other eye's atlas, they were not
/// identical, and that is the foliage mismatch. If nothing changes, the atlases agree and the mismatch is
/// downstream, in the half-resolution interleaved mask -- and this stays on anyway, because it takes a whole
/// cascade rasterisation per frame off the GPU.
#[no_mangle]
pub static mut CyberpunkVR_CascadeSkipMain: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugCascadeMainSkips: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugCmdListOffset: u32 = 4294967295;
#[no_mangle]
pub static mut CyberpunkVR_OpticsCascadeDraws: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_OpticsCascadeClears: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugSkySkipHits: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugSkyMainHits: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugGiSkipHits: u64 = 0;
/// DEFAULT 0 -- this CRASHES: EXCEPTION_ACCESS_VIOLATION reading 0x10, i.e. a null deref one
/// level inside the borrowed list. Lending MAIN's real pointer scoped to a single call is
/// safe at DrawComposition (CullReuseMode 5) but NOT here: RenderLightBuffers walks further
/// into the block list and reaches per-view resources the RTT view does not own. That makes
/// three separate crashes from writing viewData+0x168 -- treat the field as unusable and fix
/// the RTT view's own block collection instead.
#[no_mangle]
pub static mut CyberpunkVR_LightBlockLend: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugLightBlockLends: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugCompScopedSet: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugCompScopedLends: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugCompScopedSkips: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_CompLendFrameHold: u32 = 1;
/// [0] assigns held, [1] lends written from the held object, [2] of those, lends where the set's live
/// object was a different one (what the old read would have lent), [3] holds from the game frame,
/// [4] holds from the view fill, [5] assigns of the set from either place skipped (other set),
/// [6] lends into an empty block (a frame's first), [7] later assigns of the frame the block kept
/// (CyberpunkVR_CompLendFrameLatch), [8] lends that replaced an object no longer held
#[no_mangle]
pub static mut CyberpunkVR_CompLendFrameStats: [u64; 9] = [0; 9];
#[no_mangle]
pub static mut CyberpunkVR_CompLendFrameLatch: u32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_DebugCompLendWrites: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugCompLendSkips: u64 = 0;
/// ---- registered where they are defined -----------------------------------------------------
/// AN ELEMENT WITH NO OWNER IN THIS VIEW CONTRIBUTES NO DRAW KEYS.
///
/// This is the entry to the whole subtree every fault under the composition lend has come from, and it
/// is where the null enters. sub_1401ECBE0 walks MAIN's composition-element list and, for one element
/// kind, does:
///
/// 1401ECE2A  mov  r9, [rbx+28h]     ; the element's own owner field -- null for this view
/// 1401ECE8E  call sub_1402ABA18     ; r9 arrives as the 4th parameter
/// 1401ECE93  jmp  loc_1401ECD03     ; ...and the caller NEVER READS THE RESULT
///
/// From there it reaches the leaves untested, and both faults measured with the lend armed are that:
///
/// exe+0x2AC150   read at 0x70   `v3 = *(a1 + 0x70)`, a1 == 0
/// exe+0x2F098B   read at 0x68   a byte spin-lock acquired on *(null + 0x68)
///
/// sub_1403C6EF8 makes the link explicit -- it calls sub_1402AC150(a3, ...) with a3 being this
/// function's 4th parameter -- so guarding here covers the subtree instead of answering leaf by leaf,
/// which is what the second fault showed to be a losing game.
///
/// Returning early is legal, not merely convenient: the caller discards the return value on the very
/// next instruction, and the effect is that this element emits no draw keys. That is the correct answer
/// for an element the view has no owner for -- it could not have been drawn correctly anyway. It is the
/// list-is-MAIN's problem solved as a FILTER at consumption rather than by rebuilding the list, which is
/// the only other route this branch's own record allowed.
#[no_mangle]
pub static mut CyberpunkVR_DebugCompElemSkips: u64 = 0;
/// THE COMPOSITION LEND'S ONE NULL, ANSWERED THE WAY THE FUNCTION ITSELF ANSWERS IT.
///
/// With CyberpunkVR_CompLendSet armed the second view runs CRenderNode_DrawComposition against an
/// element list assembled for MAIN, and one element in it has no owner for this view. Measured twice,
/// a year apart in this branch's own record and again today with the finished-frame copy working:
///
/// 0xC0000005  read at 0x70,  FAULT exe+0x2AC150,  Rcx = 0
/// stack: exe+0x2AB8CB, exe+0x1ED03C   (the element walk, sub_1401ECFDC -> sub_1401ECBE0)
///
/// The null is loaded at exe+0x1ECE2A -- `mov r9, [rbx+28h]`, the element record's own field -- and
/// handed down three frames untested, arriving as this function's first argument. Decompiled, the
/// function is:
///
/// v3 = *(a1 + 0x70);                       <- the fault, on the FIRST instruction
/// *(a2) = 0; *(a2+8) = 0; *(a2+16) = 0;    <- the output is zeroed unconditionally
/// if (v3) { ...fill a2 from v3... }
/// return a2;
///
/// So it ALREADY has a "nothing to compute" answer and takes it whenever *(a1+0x70) is null: zero the
/// twenty output bytes and return a2. Nothing here is invented -- a1 == 0 is given the same answer the
/// engine gives one dereference later, which is why this is a guard and not a behaviour change.
///
/// Gated on our own switch, per this project's rule for hooks on a shared leaf: seven callers across the
/// image reach this function, and none of them can be affected, because with the lend off the guard
/// never runs at all. The counter increments only when it fires, so it costs nothing when it does not.
#[no_mangle]
pub static mut CyberpunkVR_DebugCompElemNulls: u64 = 0;

// ---- src/Stereo/VisionLayer.cpp

/// The outline layer's writer, CRenderNode_RenderVisionElements, selected by node.
#[no_mangle]
pub static mut CyberpunkVR_VisionNode: u32 = 4350668;
/// Ordinal of the full-size match inside that node. 0 -- measured, not assumed.
#[no_mangle]
pub static mut CyberpunkVR_VisionPick: i32 = 0;

// ---- src/Stereo/WeaponPlane.cpp

/// Last result, readable from the debugger and the overlay:
/// 0 untouched   1 patched   -1 bytes did not validate   -2 VirtualProtect refused
#[no_mangle]
pub static mut CyberpunkVR_DebugWeaponPlaneState: i32 = 0;
/// 0 untouched   1 patched   -1 bytes did not validate   -2 VirtualProtect refused
#[no_mangle]
pub static mut CyberpunkVR_DebugHighlightAnyPlane: i32 = 0;
/// 0 untouched   1 patched   -1 bytes did not validate   -2 VirtualProtect refused
#[no_mangle]
pub static mut CyberpunkVR_DebugPlaneBucketScene: i32 = 0;

// ---- B494: the tables and words of several sources (their C++ definitions stood in the Rust build's view though no
// C++ code of it reads them): all zero but the scanner slots, the render-mask grant, the view-data fix mask and the
// pair probe's record size, which keep their C++ initial values.

// src/Core/LiveControlsPoll.cpp: the scanner panels' slots (offset x, offset y, scale per panel; the in-game editor and
// the ini move them).
#[no_mangle]
pub static mut CyberpunkVR_ScannerSlots: [f32; 21] = [
    0.0, 0.0, 1.0, // the scanner frame
    0.0, 0.0, 1.0, // the details panel
    0.0, 0.0, 1.0, // the quickhack panel
    0.0, 0.0, 1.0, // the hint line
    0.0, 0.0, 1.0, // top_panel: the cyberdeck memory, whole
    0.0, 0.0, 1.0, // left_panel: the script list and its heading
    0.0, 0.0, 1.0, // right_panel: the description block
];
// src/Hooks/MenuMode.cpp: the menu's source (MenuSource, 168 bytes; cvr-rig's menu_source.rs writes it).
#[no_mangle]
pub static mut CyberpunkVR_MenuSource: [u64; 21] = [0; 21];
// src/Hooks/OsInput.cpp: the cursor map (cvr-rig's os_input.rs).
#[no_mangle]
pub static CyberpunkVR_DebugCursorMap: [AtomicI32; 4] = [const { AtomicI32::new(0) }; 4];
// src/Render/HiddenArea.cpp: every distinct native depth clear and every distinct barrier on a masked scene depth
// (with CyberpunkVR_HiddenAreaLog; hidden_area_table.rs writes them).
#[no_mangle]
pub static mut CyberpunkVR_HiddenAreaClearLog: [crate::hidden_area_runtime::ClearRecord; 32] = unsafe { core::mem::zeroed() };
#[no_mangle]
pub static mut CyberpunkVR_HiddenAreaBarrierLog: [crate::hidden_area_runtime::BarrierRecord; 32] = unsafe { core::mem::zeroed() };
// src/Runtimes/HudQuad.cpp: the HUD quad's pose (cvr-xr's hud_quad.rs writes it).
#[no_mangle]
pub static mut CyberpunkVR_HudPoseDebug: cvr_xr::hud_quad::PoseDebug = unsafe { core::mem::zeroed() };
// src/Stereo/Grading.cpp: the camera blocks uploaded, with the node and the side, in a ring (grading_uploads.rs).
#[no_mangle]
pub static mut CyberpunkVR_DebugCamBlocks: [crate::grading_uploads::CamBlockNote; 64] = unsafe { core::mem::zeroed() };
// src/Stereo/NodeDispatch.cpp: the render-mask bits the second view is granted (0, 1 and 11).
#[no_mangle]
pub static CyberpunkVR_RenderMaskGrant: AtomicU32 = AtomicU32::new((1 << 0) | (1 << 1) | (1 << 11));
// src/Stereo/Profiler.cpp: the node timeline's ring (stereo_profiler.rs writes it; tools/hitch_tests/node_timeline.py).
#[no_mangle]
pub static mut CyberpunkVR_NodeTimelineRing: [crate::stereo_profiler::ProfSpan; crate::stereo_profiler::SPAN_COUNT as usize] =
    unsafe { core::mem::zeroed() };
// src/Stereo/RenderGraphShape.cpp: the render graphs' shapes (render_graph_shape.rs; tools/hitch_tests/graph_shape.py).
#[no_mangle]
pub static mut CyberpunkVR_GraphShapes: [crate::render_graph_shape::GraphShape; crate::render_graph_shape::SLOTS] =
    unsafe { core::mem::zeroed() };
// src/Stereo/StereoPairProbe.cpp: the pair probe's records and their size (pair_probe.rs).
#[no_mangle]
pub static CyberpunkVR_PairProbeRecordBytes: u32 = core::mem::size_of::<crate::pair_probe::Record>() as u32;
#[no_mangle]
pub static mut CyberpunkVR_PairProbeRecords: [crate::pair_probe::Record; crate::pair_probe::CAPACITY as usize] =
    unsafe { core::mem::zeroed() };
const _: () = assert!(core::mem::size_of::<crate::pair_probe::Record>() == 448);
// src/Stereo/SyncStereo.cpp: the main view's context-create identity (the context capture probe).
#[no_mangle]
pub static CyberpunkVR_DebugMainManager: AtomicUsize = AtomicUsize::new(0);
// src/Stereo/ViewReuse.cpp: the view-data fixes applied (bits 0, 5 and 7).
#[no_mangle]
pub static CyberpunkVR_ViewDataFixMask: AtomicU32 = AtomicU32::new((1 << 0) | (1 << 5) | (1 << 7));

// ---- src/Render/NativeStereoProbe.cpp (the native probe's rows: stereo_probe_state.rs fills them)

#[no_mangle]
pub static CyberpunkVR_NativeStereoPipelineBytes: u32 = core::mem::size_of::<crate::camera_upload::PipelineRecord>() as u32;
#[no_mangle]
pub static CyberpunkVR_NativeStereoDrawBytes: u32 = core::mem::size_of::<crate::geometry_commands::DrawRecord>() as u32;
/// PipelineRecord's defaults: the camera root and table offset unknown (UINT_MAX).
const EMPTY_PIPELINE: crate::camera_upload::PipelineRecord = crate::camera_upload::PipelineRecord {
    original: 0,
    instanced: 0,
    root: 0,
    result: 0,
    targets: 0,
    depth_format: 0,
    sample_count: 0,
    camera_root: u32::MAX,
    camera_table_offset: u32::MAX,
    formats: [0; 8],
};
#[no_mangle]
pub static mut CyberpunkVR_NativeStereoPipelines: [crate::camera_upload::PipelineRecord; 8] = [EMPTY_PIPELINE; 8];
#[no_mangle]
// SAFETY: a draw record's all-zero value is its C++ default.
pub static mut CyberpunkVR_NativeStereoDraws: [crate::geometry_commands::DrawRecord; 4096] = unsafe { core::mem::zeroed() };
#[no_mangle]
pub static CyberpunkVR_NativeStereoTargetRecordBytes: u32 = 464;
/// pass_targets::Record x 4096 (464 bytes each, 8-aligned).
#[no_mangle]
pub static mut CyberpunkVR_NativeStereoTargets: [[u64; 58]; 4096] = [[0; 58]; 4096];

// ---- src/Render/StereoDescriptorAudit.cpp (descriptor_audit.rs fills the roots)

#[no_mangle]
pub static CyberpunkVR_DescriptorBindingBytes: u32 = 64;
#[no_mangle]
pub static CyberpunkVR_DescriptorValueBytes: u32 = 160;
#[no_mangle]
// SAFETY: a root row's all-zero value is its C++ default.
pub static mut CyberpunkVR_DescriptorRoots: [crate::audit_roots::RootInfo; 128] = unsafe { core::mem::zeroed() };

// ---- src/Render/NativeStereoFamilies.cpp (families_native.rs fills the records)

#[no_mangle]
// SAFETY: a record's all-zero value is its C++ default.
pub static mut CyberpunkVR_StereoFamilyRecords: [crate::families_native::Record; 4096] = unsafe { core::mem::zeroed() };

// ---- src/Render/NativeTargetHandoff.cpp (target_handoff_observer.rs publishes into them)

#[no_mangle]
pub static CyberpunkVR_TargetHandoffSeq: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_TargetHandoffStats: [u64; 16] = [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
#[no_mangle]
pub static mut CyberpunkVR_TargetHandoffSources: [u64; 32] = [0; 32];
#[no_mangle]
pub static mut CyberpunkVR_TargetHandoffSourceLocal: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_TargetHandoffOwners: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_TargetHandoffTargets: [u64; 192] = [0; 192];
#[no_mangle]
pub static mut CyberpunkVR_TargetHandoffStates: [u64; 80] = [0; 80];

// ---- src/Stereo/Grading.cpp (grading_uploads.rs and the live controls read it)

/// The fields mirrored MAIN -> the second view (bits 0 and 1; see grading_uploads.rs).
#[no_mangle]
pub static CyberpunkVR_GradeMirrorMask: AtomicU32 = AtomicU32::new((1 << 0) | (1 << 1));

// ---- src/Hooks/LeftHandedMirror.cpp (cvr-rig's left_handed_animation.rs writes it)

/// LeftHandedNameRecord x 64 ({u64 name, u32 info, u32 pad}).
#[no_mangle]
pub static mut CyberpunkVR_LeftHandedNameLog: [[u64; 2]; 64] = [[0; 2]; 64];
