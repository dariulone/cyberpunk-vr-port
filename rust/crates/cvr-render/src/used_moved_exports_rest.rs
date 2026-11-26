//! Exported variables the C++ half still uses, defined in Rust: the rest after used_moved_exports (their definitions
//! moved from the sources named in each section; each source keeps an extern declaration in its Rust branch, so its own
//! uses and the other sources' compile unchanged). Generated from the definition lines; the scratch check_moved compares
//! every variable's bytes with the C++ definitions'.
#![allow(non_upper_case_globals)]
use core::sync::atomic::{AtomicI32, AtomicI64, AtomicU32, AtomicU64, AtomicUsize};

// ---- src/Core/VrCore.cpp

/// THE VIEW'S OWN YAW FOR SCENES THAT TURN NOTHING.
///
/// Measured 2026-09-05 in a Tier 3 dialogue on foot, with the stick held and the head still: the
/// camera component's world forward came back BIT-IDENTICAL (0.12880, 0.96511, -0.22798 twice) while
/// its position moved 4.5 cm, and the body quaternion did not move either (0, 0, 0.08655, 0.99625
/// both samples). The base this composition uses is the engine's own pre-write value
/// (HeadingFromPreWrite), so a base that does not move means the engine did not turn the camera at
/// all -- the stick went into a head-look ANIMATION, which is why the position moved and nothing
/// else did. There is nothing to follow, so the yaw has to come from here.
///
/// Deliberately NOT armed everywhere: in ordinary play and in a vehicle the game turns the view
/// itself -- the same kind of scene is reported to work in a car -- and a second source would
/// simply double every turn. So it is gated on the scene tier AND on being on foot, and the
/// accumulator is dropped the moment the gate closes, which puts the view back exactly where the
/// game's own yaw has it.
///
/// Degrees per second at full deflection; 0 disables the whole thing, which is the default until it
/// has been seen on the picture.
#[no_mangle]
pub static mut CyberpunkVR_SceneStickYawDps: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_SceneStickYawTier: i32 = 3;
/// Live kill switch, exported so it can be flipped from the debugger, and a file escape hatch
/// for a bad boot: dropping bin\x64\vrport_nostereo.txt keeps the engine hooks out entirely
/// without a rebuild. Stereo is the default now, so the file is an opt-OUT (the old build had
/// the opposite, vrport_stereo.txt, back when the module was the experiment rather than the
/// shipping path).
#[no_mangle]
pub static mut CyberpunkVR_StereoModuleEnable: i32 = 1;

// ---- src/Framegen/FrameCapture.cpp

#[no_mangle]
pub static CyberpunkVR_FramegenCapture: AtomicI32 = AtomicI32::new(0);

// ---- src/Framegen/Framegen.cpp

#[no_mangle]
pub static mut CyberpunkVR_FramegenReport: [u8; 2048] = [0; 2048];
#[no_mangle]
pub static CyberpunkVR_FramegenReportSeq: AtomicU32 = AtomicU32::new(0);
/// The frame cap (CapToDisplay): [0] frames held, [1] frames let through (the game at or below the refresh).
#[no_mangle]
pub static mut CyberpunkVR_DebugFpsCap: [u64; 2] = [0; 2];
/// Every frame's times for benchmarks (tools/vr_bench), written while the FPS metrics run: entry i of a ring is
/// [i % 8192]; [0] counts real frames (CPU: the interval between real frames at the Present), [1] GPU frames (the
/// game queue's span from the frame's first submission to its Present, GpuTimer.cpp). A reader takes the two counts
/// at the start and the end of its window and reads the entries in between: no polling, no frame missed.
#[no_mangle]
pub static CyberpunkVR_FrameRingCount: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];
#[no_mangle]
pub static mut CyberpunkVR_FrameRingCpu: [f32; 8192] = [0.0; 8192];
#[no_mangle]
pub static mut CyberpunkVR_FrameRingGpu: [f32; 8192] = [0.0; 8192];

// ---- src/Hooks/DepthCapture.cpp

/// Historical cross-queue tracker has no production consumer: depth now copies
/// inline/on its writer queue. Keep it available only for an explicit diagnostic A/B.
#[no_mangle]
pub static CyberpunkVR_LegacyQueueSignalTracking: AtomicU32 = AtomicU32::new(0);
/// Counted so the guard can never become a silent behaviour change.
#[no_mangle]
pub static mut CyberpunkVR_DebugQueueWaitsIssued: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugQueueWaitsSkipped: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugQueueWaitsDropped: u64 = 0;

// ---- src/Hooks/LeftHandedMirror.cpp

/// The controller whose names are mirrored, 0 while none is.
#[no_mangle]
pub static CyberpunkVR_LeftHandedController: AtomicUsize = AtomicUsize::new(0);

// ---- src/Hooks/LodFov.cpp

/// The callback, the MAIN stub and the threshold tails are rust/crates/cvr-rig/src/vrik/lod_fov.rs; the exports and the
/// registrations stay.
#[no_mangle]
pub static mut CyberpunkVR_BdHeadWrite: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_BdFovWrite: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_FixLodEnable: i32 = 1;

// ---- src/Hooks/Ngx.cpp

/// Internal A/B switch. Production framegen uses RecordTags below, not these
/// legacy single-view diagnostic snapshots.
#[no_mangle]
pub static mut CyberpunkVR_NgxLegacyCapture: u32 = 0;

// ---- src/Hooks/RebindCensus.cpp

/// Read by the rate census in src/Runtimes/OpenXRPresent.cpp, and live-readable so the number can be
/// checked without waiting for a log line.
#[no_mangle]
pub static mut CyberpunkVR_DebugRebindLoopCalls: u64 = 0;

// ---- src/Hooks/SwapChain.cpp

/// Where each game Present's time goes, as QPC tick sums (read as deltas over a window by
/// tools/single_pass_tests/control_ab.py): 0 presents, 1 interval since the previous present,
/// 2 GPU-frame finish, 3 inline XR pump (xrWaitFrame/begin/locate/end),
/// 4 the original DXGI Present, 5 the whole hook, 6 QPC frequency, 7 post-publish FG pacing. A change that removes CPU
/// work while the frame stays paced elsewhere shows here as wait time grown, not as FPS.
/// (City roof, 2560x2560 per eye: 12.5 ms intervals with < 0.3 ms of waits inside the hook while
/// the GPU was 96% busy -- the frame was GPU-bound, not paced by the desktop present. A tearing
/// present was tried and measured no different, run 825.)
#[no_mangle]
pub static CyberpunkVR_PresentTiming: [AtomicU64; 8] = [const { AtomicU64::new(0) }; 8];

// ---- src/Hooks/ViewKey.cpp

/// The hook and the thread's view are rust/crates/cvr-rig/src/vrik/view_key.rs; the exports and the registration stay.
#[no_mangle]
pub static mut CyberpunkVR_DebugViewKeyMainNodes: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugViewKeyOtherNodes: u64 = 0;

// ---- src/Hooks/XInput.cpp

/// DASH on the right stick pushed fully UP. 1 = on. A knob rather than a hardcoded gesture like
/// sprint and crouch, because this one claims half of an axis that a player who turns "Disable Mouse Y"
/// off is using for camera pitch -- and they should be able to have it back.
/// DISCRETE MOVEMENT SPEED. 1 = a push past the deadzone moves at ONE speed whatever the deflection,
/// which is what the keyboard does (LeftY_Axis binds IK_W at val="1.0"); 0 = the pad's analogue
/// magnitude, where the speed rides the thumb.
/// GAMEPLAY REMAPS ARE OFF WHILE A MENU OR A BRAINDANCE OWNS THE SCREEN.
///
/// In menus every binding is the default one and none of our override bindings apply, and the same
/// for a braindance. Every remap in this file was written for gameplay -- the
/// grip standing in for a shoulder, the ear gesture on LB, the dash on B, the magazine drop's mask, the
/// walk tiers on the left stick -- and in a menu they are at best redundant and at worst they eat a
/// button the menu itself needs. With this on, a menu or a braindance gets the raw controller: buttons,
/// triggers and sticks merged as they come, and nothing else.
///
/// The Quest Touch controllers have no physical LB/RB. In menus and in a braindance only, their grip
/// buttons stand in for those two shoulders so the UI remains navigable with Virtual Desktop gamepad
/// emulation off, and the braindance has its editor (LB) and its layers (RB). This is a hardware
/// translation, not a gameplay remap; gameplay grips remain reserved for VR actions.
#[no_mangle]
pub static mut CyberpunkVR_InputDefaultInUi: i32 = 1;
/// How long the magazine drop stays blocked after a UI overlay closes. Spamming the close button is what
/// this is for: the popup goes away on the first press and the next one would land in gameplay, where B
/// drops the magazine.
#[no_mangle]
pub static mut CyberpunkVR_PopupMagBlockMs: i32 = 1000;
/// THE HACK LIST ON THE LEFT STICK, up and down, at the threshold every other gesture in this file
/// fires at. It REPLACES the face buttons for paging -- X and Y sat under the thumb of the hand that is
/// already squeezing a grip at the ear, which is the reach the one-handed scanner exists to avoid, so X
/// is now only the apply and Y does nothing. 0.90 to fire and 0.50 to re-arm are the snap turn's numbers
/// for its reason: a resting thumb, or a wrist drifting while walking, must not page a list.
/// 1 = while MOUNTED, A also emits X so a dialogue line can be confirmed. X is the vehicle exit
/// there and is held out of the merge, which left nothing able to confirm at all. The handbrake on
/// A keeps working -- the mirror is added, not substituted. 0 restores the previous behaviour.
#[no_mangle]
pub static mut CyberpunkVR_VehicleDialogConfirmOnA: i32 = 1;

// ---- src/Lobby/MenuLobby.cpp

/// xr_lobby (Core/LiveControlsPoll.cpp).
#[no_mangle]
pub static mut CyberpunkVR_Lobby: i32 = 1;
/// 0 = the composition block is left as the game wrote it (the backdrop stays), the world still renders behind it.
#[no_mangle]
pub static mut CyberpunkVR_LobbyCompose: i32 = 1;
/// Diagnostics: composition-sized blocks ComposeWorld was offered while the lobby is up [0], taken [1], turned away
/// for their weights [2], target size [3], repeated grading [4], grading values [5].
#[no_mangle]
pub static CyberpunkVR_LobbyComposeStats: [AtomicU64; 6] = [const { AtomicU64::new(0) }; 6];
/// The last block turned away (432 bytes as floats) and the render node it came under (RVA).
#[no_mangle]
pub static mut CyberpunkVR_LobbyComposeRejected: [f32; 108] = [0.0; 108];
#[no_mangle]
pub static CyberpunkVR_LobbyComposeRejectedNode: AtomicU32 = AtomicU32::new(0);
/// xr_lobby_ambient (Core/LiveControlsPoll.cpp): the lobby's ambience plays (mods/cet/CyberpunkVRPort_Lobby
/// modules/sound.lua asks, its menu's button switches it).
#[no_mangle]
pub static mut CyberpunkVR_LobbyAmbient: i32 = 1;
/// 1 (default) = a loading screen after the first main menu is hung in the dark before the player (LoadingInSpace);
/// 0 = the eyes are shut while it is up.
#[no_mangle]
pub static mut CyberpunkVR_LoadingInSpace: i32 = 1;
/// [0] composition blocks given the world, [1] render-behind-menu switch set, [2] switch handed back,
/// [3] ticks with no runtime UI system to find, [4] composition inputs recorded, [5] UI system lookups,
/// [6] times the world layer was switched on for the lobby, [7] world layer ticks seen,
/// [8] times the player's gameplay input was switched on, [9] times the menu's UI input context was set aside,
/// [10] input handed back, [11] ticks with no player input object to find.
#[no_mangle]
pub static CyberpunkVR_LobbyStats: [AtomicU64; 12] = [const { AtomicU64::new(0) }; 12];
/// The node the composition block is uploaded under (RVA of its work function), and the resources that node resolved
/// in the last frame it ran: the menu panel's UI is one of them.
#[no_mangle]
pub static CyberpunkVR_LobbyCompositeNode: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_LobbyInputCount: AtomicU32 = AtomicU32::new(0);

// ---- src/Lobby/PlayerBody.cpp

/// Spawns seen: [0] the main menu's, given the man's body; [1] a man's save repaired; [2] every other.
#[no_mangle]
pub static mut CyberpunkVR_DebugPlayerSpawns: [u64; 3] = [0; 3];

// ---- src/Lobby/StartScreen.cpp

/// 0 = the start screen shows unless -skipStartScreen is given, as the game has it.
#[no_mangle]
pub static mut CyberpunkVR_SkipStartScreen: i32 = 1;

// ---- src/Natives/HudClearance.cpp

/// 1 = keep the HUD panels in front of the world (xr_hud_clearance). See Runtimes/HudClearance.hpp.
#[no_mangle]
pub static mut CyberpunkVR_HudClearance: i32 = 0;
/// How much a panel grows as it comes in (xr_hud_clearance_grow), 0..1. See GrowFactor.
#[no_mangle]
pub static mut CyberpunkVR_HudClearanceGrow: f32 = 0.5;
/// 1 = a tracked hand in front of a HUD panel pushes it in too (xr_hud_clearance_hands). See HandFront.
#[no_mangle]
pub static mut CyberpunkVR_HudClearanceHands: i32 = 0;

// ---- src/Natives/HudPanel.cpp

/// The HUD root's size in its pixels, as VRHudPanelUpdate last saw it; 0 until then (HudDesignScale).
#[no_mangle]
pub static mut CyberpunkVR_HudRootWidth: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_HudRootHeight: f32 = 0.0;
/// The HUD's ink layer and its virtual window as VRHudPanelUpdate last saw them (diagnostics).
#[no_mangle]
pub static mut CyberpunkVR_HudLayerAddress: [usize; 2] = [0; 2];

// ---- src/Natives/InteractionDepth.cpp

/// 1 = place the interaction prompt at the depth of its object (xr_interaction_anchor). See the header.
#[no_mangle]
pub static mut CyberpunkVR_InteractionAnchor: i32 = 0;

// ---- src/Natives/OrientationProvider.cpp

#[no_mangle]
pub static mut CyberpunkVR_SightDot: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_SightLineOffset: [f32; 3] = [0.0, 0.0, 0.0];
#[no_mangle]
pub static mut CyberpunkVR_SightLineTick: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_HitscanFromSightLine: i32 = 1;

// ---- src/Natives/RemoteCamera.cpp

/// DEFINED HERE, not merely declared. It used to live beside the second eye's HUD capture, which is
/// render code and has gone back to 0.1.5 with the rest of it. This is the only consumer left, and
/// the toggle belongs to the braindance camera rather than to the stereo path.
#[no_mangle]
pub static mut CyberpunkVR_HudInBraindance: i32 = 0;

// ---- src/Overlay/ImGuiOverlay.cpp

#[no_mangle]
pub static mut CyberpunkVR_OverlaySkipEmpty: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_VrShellTab: i32 = -1;

// ---- src/Render/CameraSourceResolve.cpp

#[no_mangle]
pub static CyberpunkVR_OpticsCameraResolve: AtomicI32 = AtomicI32::new(1);
#[no_mangle]
pub static CyberpunkVR_OpticsCameraResolveCount: AtomicU64 = AtomicU64::new(0);
/// Copy nodes that found no target, and resolves refused for an unobserved state.
#[no_mangle]
pub static CyberpunkVR_OpticsCameraResolveMisses: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];

// ---- src/Render/FixedVrs.cpp

#[no_mangle]
pub static mut CyberpunkVR_FixedVrsReportSeq: u32 = 0;

// ---- src/Render/GpuQueueTimeline.cpp

/// Write a number of submissions to capture; 0 when taken.
#[no_mangle]
pub static CyberpunkVR_GpuTimelineRequest: AtomicU32 = AtomicU32::new(0);
/// 0 idle, 1 capturing, 2 waiting for the GPU, 3 done (records valid), 4 failed.
#[no_mangle]
pub static CyberpunkVR_GpuTimelineState: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_GpuTimelineCount: AtomicU32 = AtomicU32::new(0);
/// Per queue type (0 direct, 1 compute): [t*3] timestamp frequency, [t*3+1] GPU timestamp and [t*3+2] QPC
/// of one calibration; [6] QPC frequency.
#[no_mangle]
pub static mut CyberpunkVR_GpuTimelineCalib: [u64; 8] = [0; 8];
/// Per record, 6 words: kind (0 direct, 1 compute, 2 Present) | lists << 8, queue pointer, QPC when the
/// call came in, QPC when the original returned, GPU timestamp before the lists, GPU timestamp after them.
/// Kinds 3 (queue Signal) and 4 (queue Wait): queue pointer, QPC, fence pointer, fence value, 0.
#[no_mangle]
pub static mut CyberpunkVR_GpuTimelineRecords: [u64; 98304] = [0; 98304];

// ---- src/Render/GpuStageProfile.cpp

#[no_mangle]
pub static CyberpunkVR_GpuStageRequest: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_GpuStageReportSeq: AtomicU32 = AtomicU32::new(0);

// ---- src/Render/HiddenArea.cpp

/// Effective settings, also writable by live tests (an ini edit overwrites them again).
#[no_mangle]
pub static CyberpunkVR_HiddenAreaEnabled: AtomicU32 = AtomicU32::new(0);
/// Only clears inside this render-graph node (work RVA) are masked; 0 = any node. Default:
/// SetRenderTargetsGBuffer. The hologram, planar-reflection and water depths are square as well.
#[no_mangle]
pub static CyberpunkVR_HiddenAreaNode: AtomicU32 = AtomicU32::new(10487856);
/// 1 = record every distinct native depth clear into CyberpunkVR_HiddenAreaClearLog.
#[no_mangle]
pub static CyberpunkVR_HiddenAreaLog: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_HiddenAreaMasked: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];
/// 1 = put the hidden cells back to the far plane whenever the scene depth leaves DEPTH_WRITE;
/// 2 = also back to the near plane when a G-buffer node takes it into DEPTH_WRITE again.
#[no_mangle]
pub static CyberpunkVR_HiddenAreaSky: AtomicU32 = AtomicU32::new(2);
/// Only barriers inside this node (work RVA) go to the far plane; 0 = every transition out of DEPTH_WRITE.
#[no_mangle]
pub static CyberpunkVR_HiddenAreaSkyNode: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_HiddenAreaSkyResets: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];
#[no_mangle]
pub static CyberpunkVR_HiddenAreaNearResets: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];

// ---- src/Render/LensMask.cpp

#[no_mangle]
pub static CyberpunkVR_OpticsLensClean: AtomicI32 = AtomicI32::new(1);
#[no_mangle]
pub static CyberpunkVR_OpticsLensMaskDraws: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];
#[no_mangle]
pub static CyberpunkVR_OpticsLensMaskTags: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];

// ---- src/Render/LensSourceResolve.cpp

#[no_mangle]
pub static CyberpunkVR_OpticsSourceResolve: AtomicI32 = AtomicI32::new(1);
#[no_mangle]
pub static CyberpunkVR_OpticsResolveCount: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_OpticsResolveSides: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];
#[no_mangle]
pub static CyberpunkVR_OpticsResolveJitterMode: AtomicI32 = AtomicI32::new(0);

// ---- src/Render/MonoCascades.cpp


// ---- src/Render/NativeDepthPreparation.cpp


// ---- src/Render/NativeEarlyDepth.cpp

#[no_mangle]
pub static CyberpunkVR_EarlyDepthMaterialHintCount: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_EarlyDepthMaterialHints: [[u32; 2]; 128] = [[0; 2]; 128];
#[no_mangle]
pub static mut CyberpunkVR_EarlyDepthMaterialDescriptors: u64 = 0;
#[no_mangle]
pub static CyberpunkVR_EarlyDepthAlphaReference: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_EarlyDepthAlphaOptimized: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_EarlyDepthSkinnedAlpha: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_EarlyDepthSkinnedAlphaStats: [u64; 6] = [0; 6];
#[no_mangle]
pub static mut CyberpunkVR_EarlyDepthAlphaStats: [u64; 6] = [0; 6];
#[no_mangle]
pub static mut CyberpunkVR_EarlyDepthAlphaProofs: [u64; 6] = [0; 6];

// ---- src/Render/NativeInstanceObserver.cpp

#[no_mangle]
pub static CyberpunkVR_NativeInstanceGpuProbePhase: AtomicU32 = AtomicU32::new(0);

// ---- src/Render/NativeSourceCbvSnapshot.cpp

#[no_mangle]
pub static mut CyberpunkVR_SourceCbvSnapshotsPoolFailure: [u64; 16] = [0; 16];

// ---- src/Render/NativeStereoProbe.cpp

#[no_mangle]
pub static CyberpunkVR_NativeStereoStaticAssetStatus: AtomicU32 = AtomicU32::new(0);

// ---- src/Render/OpticsProbe.cpp

#[no_mangle]
pub static CyberpunkVR_OpticsProbeState: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_OpticsProbeWidth: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_OpticsProbeHeight: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_OpticsProbePixels: usize = 0;
#[no_mangle]
pub static mut CyberpunkVR_OpticsProbeFormat: u32 = 0;
#[no_mangle]
pub static CyberpunkVR_OpticsProbeStage: AtomicU32 = AtomicU32::new(0);

// ---- src/Render/Osr.cpp

/// Diagnostics: [0] frames reconstructed, [1] failed, [2] frames that used the made eye's history.
#[no_mangle]
pub static CyberpunkVR_OsrStats: [AtomicI64; 3] = [const { AtomicI64::new(0) }; 3];
/// Tuning, read every frame: [0] depth agreement tolerance x1000 (relative distance, default 30 = 3%), [1] the
/// farthest a fill searches along a row or a column for the hole's bounds (pixels; the hands at 30-40 cm open holes
/// of 150-250 px, and a run that reached only the hand's side filled them with the hand's colour), [2] the nearest a
/// surface of the made eye's previous frame may be when this frame's drawn eye cannot confirm it (cm, 0 = no limit;
/// see StillThere), [3] how far a sample may sit from a nearer surface's place in the made eye before it counts as
/// that surface's edge (made-eye pixels x100, 0 = off): see Trace.
#[no_mangle]
pub static mut CyberpunkVR_OsrTune: [i32; 4] = [30, 384, 100, 75];

// ---- src/Render/SceneGpuProfile.cpp

/// Frame mode (tools/vr_bench): profile every command list whose recording starts in the next N whole frames (1..32),
/// counted by the Present hook. A list begun in the frame the request lands in is left out (it is a partial frame),
/// so the report's sums divided by "frames" are per-frame times. Takes precedence over CyberpunkVR_SceneGpuRequest.
#[no_mangle]
pub static CyberpunkVR_SceneGpuFrameRequest: AtomicU32 = AtomicU32::new(0);
/// Diagnostic: 1 = every range on a direct list also gets a pipeline-statistics query (IA / VS / clipper / PS counts,
/// D3D12_QUERY_DATA_PIPELINE_STATISTICS), summed per group as "ps" in the report. Read when a profile starts.
#[no_mangle]
pub static CyberpunkVR_SceneGpuPipelineStats: AtomicU32 = AtomicU32::new(0);
/// Diagnostic: 1 = every draw of a profiled range on a direct list gets an occlusion query; the report's "occ" counts
/// per eye (viewport kind 1 left / whole frame, 2 right half, 3 other) the draws and how many samples passed:
/// [draws, 0, 1-64, 65-4096, more]. Read when a profile starts.
#[no_mangle]
pub static CyberpunkVR_SceneGpuDrawOcclusion: AtomicU32 = AtomicU32::new(0);

// ---- src/Render/SsrWideMerge.cpp


// ---- src/Render/StageReplay.cpp

/// 0 off, 1 census (capture only), 2 skip target stages, 3 replay source capture into target,
/// 4 replay with the target camera substituted. See stage_replay::Mode.
#[no_mangle]
pub static CyberpunkVR_StageReplayMode: AtomicU32 = AtomicU32::new(0);
/// Bit s selects RenderElements stage s (consumer arguments+0x10).
#[no_mangle]
pub static CyberpunkVR_StageReplayStages: AtomicU64 = AtomicU64::new(0);
/// Side that records natively and is captured (1 = VRCAM, 0 = MAIN).
#[no_mangle]
pub static CyberpunkVR_StageReplaySource: AtomicU32 = AtomicU32::new(1);
/// Census: frames to count (written by the test), state 0 idle / 1 running / 2 done.
#[no_mangle]
pub static CyberpunkVR_StageCensusRequest: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StageCensusState: AtomicU32 = AtomicU32::new(0);
/// Census detail stream: this stage's first call per side in the window is recorded in full.
#[no_mangle]
pub static CyberpunkVR_StageStreamStage: AtomicU32 = AtomicU32::new(4294967295);
/// Rows [side*64+stage][32]; columns documented at Column below. Ops [row][Op].
#[no_mangle]
pub static mut CyberpunkVR_StageCensusRows: [[u64; 32]; 128] = [[0; 32]; 128];
#[no_mangle]
pub static mut CyberpunkVR_StageCensusOps: [[u32; 48]; 128] = [[0; 48]; 128];
/// frames, presents, chainedVtables, hookInstalls, shadowsClaimed, shadowFull, epoch, reserved...
#[no_mangle]
pub static mut CyberpunkVR_StageCensusSummary: [u64; 16] = [0; 16];
/// Per side: commandsAddress, commandCount, payloadAddress, payloadBytes, overflow, list, engineFrame,
/// stage, camVs, camPs, heapCpu, heapGpu, stride, nativeHeap, sliceBaseCapacity, cursor.
#[no_mangle]
pub static mut CyberpunkVR_StageStreamInfo: [[u64; 16]; 2] = [[0; 16]; 2];
/// Census stream diagnostic: the native render context (0x800 bytes) and its binding state (0x200)
/// of each side's streamed call, copied after the call recorded. Address in [side][0] of Addr.
#[no_mangle]
pub static mut CyberpunkVR_StageStreamContextDump: [[u64; 256]; 2] = [[0; 256]; 2];
#[no_mangle]
pub static mut CyberpunkVR_StageStreamStateDump: [[u64; 64]; 2] = [[0; 64]; 2];
#[no_mangle]
pub static mut CyberpunkVR_StageStreamContextAddr: [[u64; 2]; 2] = [[0; 2]; 2];
/// captures, captureOverflow, captureBusy, skipped, replayed, replayCommands, replayTicks, fallbacks,
/// cameraTables, cameraDescriptors, restoreOps, reserved..., [16+reason] fallback by reason.
#[no_mangle]
pub static mut CyberpunkVR_StageReplayStats: [u64; 32] = [0; 32];
/// Census timeline: 6 qwords per event -- frame|side<<32|stage<<40|ordinal<<48|kind<<56, thread,
/// startQpc, endQpc, spanBegin, spanEnd. Kind 0 forward, 1 capture, 2 skip, 3 replay, 4 fallback,
/// 5 present. Written only while a census window runs.
#[no_mangle]
pub static mut CyberpunkVR_StageEvents: [u64; 98304] = [0; 98304];
#[no_mangle]
pub static CyberpunkVR_StageEventCount: AtomicU32 = AtomicU32::new(0);
/// 1: give VRCAM's GBuffer_Solid the same worker partitions as MAIN. The scene graph builder
/// sub_141D43040 clamps the partition count to 1 when view+0xF94 (the AA/upscaler mode) is 1, and
/// the render-to-texture eye has that mode -- but so does every other render-to-texture view (the
/// optics feed, ...). The clamp is redirected (InstallVrcamPartitions) to a check that keeps
/// min(workers, dword_14347F790) only for the graph whose camera name equals
/// CyberpunkVR_VrcamGbufferPartitionsKey; every other view keeps the native 1. State: 0 native,
/// 1 VRCAM partitioned, 2 site not installed (bytes did not match).
/// Its own setting (xr_vrcam_gbuffer_partitions), not part of stage replay: unpatched, VRCAM's
/// G-buffer is ONE job -- 2.15 ms on one thread in the city -- and the second view's whole graph,
/// which the engine runs before MAIN's, waits on it. Node timeline 2026-10-16.
/// The camera scope's view (vr_optics_feed) is the same kind of graph -- render-to-texture, run before
/// MAIN's bulk with MAIN idle -- and has its own setting (xr_scope_gbuffer_partitions, off by default:
/// measured slower, see LiveControlsPoll.cpp).
#[no_mangle]
pub static CyberpunkVR_VrcamGbufferPartitions: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_OpticsGbufferPartitions: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_VrcamGbufferPartitionsState: AtomicU32 = AtomicU32::new(0);
/// Read by the trampoline at install-time address: VRCAM's and the scope's camera-name hashes while
/// partitioned, 0 otherwise. Must stay plain globals (Hooks/Trampoline.hpp).
#[no_mangle]
pub static mut CyberpunkVR_VrcamGbufferPartitionsKey: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_OpticsGbufferPartitionsKey: u64 = 0;
/// Replay: 1 lets a captured stream that contains resource barriers replay without them. Every
/// stage begins with one barrier on the source view's own resource (census 816); the target's own
/// consumer -- with its barrier -- is skipped as a whole, so the engine's state tracking stays
/// consistent. 0 (default) treats any barrier as a reason to fall back.
#[no_mangle]
pub static CyberpunkVR_StageReplayAllowBarriers: AtomicU32 = AtomicU32::new(0);
/// 1 = census streams carry every bound table's descriptors (flag 16 on RootTable), for
/// tools/single_pass_tests/stage_table_diff.py. Diagnostic only; captures never do this.
#[no_mangle]
pub static CyberpunkVR_StageReplayTableDump: AtomicU32 = AtomicU32::new(0);
/// Last view descriptors read for a capture ([0]) and a replay target ([1]), with their stage in [x][7].
#[no_mangle]
pub static mut CyberpunkVR_StageReplayViewDiag: [[u64; 8]; 2] = [[0; 8]; 2];
/// Replay: microseconds a target call may wait for the source's partitions of the same frame.
#[no_mangle]
pub static CyberpunkVR_StageReplayWaitUs: AtomicU32 = AtomicU32::new(0);
/// Camera-replay diagnostics: target camera unknown, no heap, captured table without the source
/// camera, tables built, tables reused; the last target camera, source camera, table size; then
/// ring code mismatch, ring heap mismatch, ring exhausted, descriptors reserved.
#[no_mangle]
pub static mut CyberpunkVR_StageReplayCameraDiag: [u64; 12] = [0; 12];

// ---- src/Runtimes/OpenXRManager.cpp

/// Mode 3 follow rate, in 1/seconds: t = 1 - exp(-speed * dt). 30 gives a ~33 ms time constant,
/// which is a whole-step follow of 0.34 at 72 Hz. The hands run 25 (xr_hand_lerp_speed).
#[no_mangle]
pub static mut CyberpunkVR_PoseLerpSpeed: f32 = 30.0;
/// 1 = recenter automatically when the player puts the headset back on (xr_auto_recenter, see
/// Runtimes/PresenceRecenter.hpp). 0 = only F7 / the controller gesture / the runtime's own recenter.
#[no_mangle]
pub static mut CyberpunkVR_AutoRecenter: i32 = 1;
/// THE FILTER'S SPEED FOLLOWS WHAT IS IN THE HAND, and that is the honest axis for it: the filter trades
/// tracking noise against lag, and which of the two hurts depends on what the hand is doing. A blade is
/// swung and any lag reads as the weapon trailing the arm; a sniper is held still and aimed, where the
/// noise is what shows and the lag costs nothing. Numbers set by feel.
///
/// 0 in any of these means "use the base" (xr_hand_lerp_speed), which is also what empty hands and an
/// unnamed weapon get. Class 5 is melee, 4 sniper, 3 shotgun, 2 rifle, 1 handgun/revolver -- published
/// per draw by the weapon module out of the record's own itemType.
#[no_mangle]
pub static mut CyberpunkVR_HandLerpPistol: f32 = 25.0;
#[no_mangle]
pub static mut CyberpunkVR_HandLerpRifle: f32 = 25.0;
/// the other long gun
#[no_mangle]
pub static mut CyberpunkVR_HandLerpShotgun: f32 = 20.0;
#[no_mangle]
pub static mut CyberpunkVR_HandLerpSniper: f32 = 10.0;
#[no_mangle]
pub static mut CyberpunkVR_HandLerpMelee: f32 = 35.0;
/// xr_hand_lerp_framegen_scale multiplies the selected rate under frame generation (main's 6b2afc5c6: 1.10); zero
/// bypasses smoothing while frame generation is active.
#[no_mangle]
pub static mut CyberpunkVR_HandLerpFramegenScale: f32 = 1.10;

// ---- src/Runtimes/OpenXRPresent.cpp

/// 1 (default) = label each captured frame with the head orientation the camera injection
/// actually used for it, instead of the pose cache as of Present. See the use site.
#[no_mangle]
pub static mut CyberpunkVR_BindPoseToImage: i32 = 1;
/// 1 = the SECOND EYE is submitted with the label its own image was drawn with, taken from its own
/// read-back queue. 0 restores the shared label, which is what it had before, so the two can be compared
/// live without a rebuild.
#[no_mangle]
pub static mut CyberpunkVR_VrcamOwnLabel: i32 = 1;
/// One [vrik] line every two seconds: present rate vs the skeleton's real update rate.
/// (declared above <cstdint>, so plain int)
#[no_mangle]
pub static mut CyberpunkVR_VrikRateLog: i32 = 1;
/// Thread the present/capture runs on; see the write site for why this settles PoseFrameLag.
#[no_mangle]
pub static mut CyberpunkVR_DebugTidPresent: u32 = 0;
/// Frames labelled with the pose read back out of the engine at frame-open (vr_core.cpp).
#[no_mangle]
pub static mut CyberpunkVR_DebugPoseReadBack: u64 = 0;

// ---- src/Stereo/Capture.cpp

/// Countdown of RTV binds inside the vrcam node still to be logged (see the [rtvpick]
/// diagnostic in hk_OMSetRenderTargets). Burns down to 0 so it costs nothing after the
/// first frames; raise it live from the debugger to sample again.
#[no_mangle]
pub static mut CyberpunkVR_DebugRtvPickLog: i32 = 48;
/// A MILLISECOND CLOCK THAT CAN SEE A FRAME. GetTickCount64 has ~15.6 ms granularity, and the second
/// eye's staleness is interesting at 14 ms and below -- so measured with it, the age came back as exactly
/// 0, 16 or 31 ms and the reported max sat pinned at 16.0 in almost every window. That is one tick, not a
/// measurement. Every frame-scale conclusion drawn from it would have been an artefact of the clock.
///
/// QPC instead, still in milliseconds so CyberpunkVR_StereoEyeMaxAgeMs keeps its units and its meaning.
/// MICROSECONDS, alongside the milliseconds the overlay and the gate use. The ms value cannot
/// express a frame; this one can.
#[no_mangle]
pub static mut CyberpunkVR_DebugVrcamEyeAgeUs: u32 = 0;
/// Record, INTO THE ENGINE'S OWN command list, a copy of the freshly written vrcam
/// final into our committed g_stable_tex. Called at the end of the vrcam RenderFinal2D
/// node: the composite draw is already recorded, no later pass is recorded yet, so in
/// queue order the copy executes after the final write and BEFORE any ALIASING barrier
/// re-purposes the transient's heap memory (the proven bright/dark root). No SEH here:
/// same risk class as the existing raw-vtable append path (uses C++ locks => C2712).
/// A TYPELESS SNAPSHOT CANNOT BE VIEWED, AND THAT IS THE DEVICE REMOVAL.
///
/// The snapshot below inherits the SOURCE's desc, and the finished frame the second view's composition
/// writes is R8G8B8A8_TYPELESS. ColorBlit then does `srv.Format = srcColor->GetDesc().Format` and calls
/// CreateShaderResourceView with a typeless format, which D3D12 does not allow. Measured, with
/// xr_final_grab armed: the log reads `[stable] committed vrcam snapshot 3072x3072 fmt=27` -- 27 is
/// R8G8B8A8_TYPELESS, where the old mid-chain source read fmt=26 (R11G11B10_FLOAT) and viewed cleanly --
/// and the session ends `presentHr=0x887A0005 reason=0x887A0001` with `PageFaultVA=0`: DEVICE_REMOVED
/// for INVALID_CALL, a badly formed call rather than a bad address. The note beside nd.Flags further
/// down already records this project losing a device to "a typeless RTV elsewhere"; this is the same
/// class of mistake one resource along.
///
/// So the snapshot is typed. CopyResource stays legal because a typed member and its TYPELESS parent
/// are in one copy family, and every view over the snapshot -- ColorBlit's SRV, the desktop mirror's
/// shared surface -- becomes legal at once.
///
/// WHICH member of the 8-bit family only the picture can settle, so it is a live key rather than a
/// rebuild:
/// 1 (ships)  _UNORM_SRGB. The finished frame carries sRGB-ENCODED bytes, so the sampler decodes to
/// linear and ColorBlit's own _UNORM_SRGB render target encodes back -- a round trip. That
/// also cancels the second encode, which is what shows as overexposure.
/// 0          _UNORM. The bytes are sampled raw; correct if that frame turns out to hold linear.
/// A float source is already typed and is handed back untouched, so nothing changes without the grab.
#[no_mangle]
pub static mut CyberpunkVR_StableSrgbView: i32 = 1;

// ---- src/Stereo/CullVisibility.cpp

#[no_mangle]
pub static mut CyberpunkVR_DebugMainContainer: usize = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugVrcamContainer: usize = 0;

// ---- src/Stereo/EngineStereo.cpp


// ---- src/Stereo/FrameGraph.cpp

/// VRCAM composition GROUP: DrawComposition / CompositionPostProcess / FullscreenVideo
/// live in SCENE_FULL under build-bit 82 and in the final builder sub_140982C7C; VRCAM
/// runs SCENE_INCR (sub_14227BD10) which has NEITHER -> missing. RenderFinal2D is added
/// by name. Inject all of them via the same Extraction-adder hook (fires for VRCAM with
/// the builder ctx). NOTE: CopyToTexture copies final-color 0x3D7E6258 (pre-composition);
/// composition writes 0x31CF52F9 -> adding it does NOT change what the mirror copies. It
/// makes VRCAM's graph node-complete. Default OFF -> enable + verify live.
/// default OFF: composition inputs absent in VRCAM RTT graph -> downstream fault
#[no_mangle]
pub static mut CyberpunkVR_VrcamCompositionGroup: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_EnvExtraMask: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_WaterMirror: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_WaterAssign: u32 = 1;

// ---- src/Stereo/GameThreadQueues.cpp

/// 1 = the game thread leaves dispatcher queue 1 (render jobs) to the workers while it waits. Live for A/B.
#[no_mangle]
pub static CyberpunkVR_GameThreadSkipRenderQueue: AtomicU32 = AtomicU32::new(0);

// ---- src/Stereo/Grading.cpp


// ---- src/Stereo/Hud.cpp

/// The composite constants are Rust's (rust/crates/cvr-render/src/hud_composite.rs); the engine's own bend stays here.
/// The engine's own bend as it was before the in-eye composite's was zeroed (Grading.cpp, FlattenHudComposite); 0,0 until then.
#[no_mangle]
pub static mut CyberpunkVR_HudEngineCurvature: [f32; 2] = [0.0, 0.0];

// ---- src/Stereo/NativeSceneUpdate.cpp

#[no_mangle]
pub static CyberpunkVR_SceneUpdateRequest: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_SceneUpdateState: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_SceneUpdateCompatible: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_SceneUpdateSeq: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_SceneUpdatePending: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_SceneUpdateStats: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_SceneUpdateJobs: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_SceneUpdateScenes: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_SceneUpdateCommands: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_SceneUpdateMutations: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_SceneUpdateJobCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_SceneUpdateSceneCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_SceneUpdateCommandCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_SceneUpdateMutationCount: u32 = 0;

// ---- src/Stereo/NodeDispatch.cpp

#[no_mangle]
pub static mut CyberpunkVR_NoStateLies: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugForeignStateRefusals: u64 = 0;
/// Capability-refusal census. sub_14021BE28 is not the HUD's private gate: ClusteredLightsCull
/// runs its whole body only `if (... || sub_14021BE28(a2, &word_143C1CDF0) != 0)`, and other
/// nodes use it with their own descriptors. DrawHUD's refusal was found by hand and cost a
/// session; enumerating every (node, descriptor) the engine denies the second view is the same
/// work done once, for all of them.
/// OFF: [cap] per-view gate refusals -- the first thing to switch on for a new "the second eye is missing X"
#[no_mangle]
pub static mut CyberpunkVR_CapCensus: i32 = 1;
/// Granting = making the test pass HONESTLY, by OR-ing the bits the descriptor asks for into the
/// view's own 32-qword mask at ctx+6304 -- exactly what fixed the HUD. Not the same thing as
/// returning 1 from the gate: that leaves the mask short, so the next node to ask gets refused
/// again and anything downstream that reads the mask still sees a crippled view.
///
/// The measured refusals, by node:
/// 77CED4 ClusteredLightsCull, 77D308 RenderLightBuffers   <- the reported unlit lights
/// 77E610 ReflectionProbes, 786BCC RenderShadowmask, 153844 RenderShadowCascade,
/// 6212EC DecoupledParticleLighting, 775ACC SetRenderTargetsMain, 774CF8 HistogramUpdate,
/// 77120C GameplayPostFX, 77B638/77D214 AutoSpawnOnTerrain
/// 0 = census only, 1 = the two light nodes (default: the smallest change that addresses the
/// symptom), 2 = every refusal.
/// SUPERSEDED by CyberpunkVR_RenderMaskGrant, and left at 0.
/// Two things were wrong with granting here. It read the required words from descriptor+0 when
/// the test compares against descriptor+8, so it never actually satisfied anything -- and it
/// fired on EVERY refusal, which includes a dozen `Rendering/Debug/...` overlays the engine is
/// right to withhold. The named table above grants one specific category from the engine's own
/// descriptor. Kept only so the census can still be run with the grant off.
#[no_mangle]
pub static mut CyberpunkVR_CapGrant: u32 = 0;
/// ---- per-view node census ---------------------------------------------------------------
/// Which passes does the engine actually dispatch for each view? "VRCAM is missing effect X"
/// is otherwise answered by breakpointing candidate nodes one at a time, which is slow and only
/// ever tests the guesses one happened to make. This records the SET of work-fn RVAs each view
/// runs and logs the difference, so the answer arrives in one pass over a live frame.
/// (The two arrays have existed as exports for a long time but nothing ever filled them, which
/// is why they always read back zero.)
#[no_mangle]
pub static mut CyberpunkVR_NodeCensus: i32 = 1;
/// ---- the sky pass, as reversed -----------------------------------------------------------
///
/// CRenderNode_RenderSkyScattering (sub_1407F86CC) only checks feature 35 and forwards to
/// sub_1407F8714, and THAT is where the decision is:
///
/// v3 = *(QWORD*)(*(QWORD*)(a2+32) + 96);          // the sky manager
/// v4 = *(DWORD*)(sub_14033C248(a2) + 3988);       // viewData+0xF94, the AA/upscaler mode
/// v8 = 32 * *(BYTE*)(view + 5856);                // 32-byte record, INDEXED BY THE VIEW
/// if ( *(DWORD*)(v8 + v3 + 72) && !v4 || v7 ) { ...build the sky... }
/// ...
/// *(BYTE*)(v8+v3+80) = slot+1;      // one of six slots per frame
/// if (v7 || *(BYTE*)(v8+v3+80) >= 6) { ...publish, timestamp, reset the counter... }
///
/// Two things follow, and both are measurable rather than arguable.
///
/// The sky LUT is built AMORTISED -- six slots, one per frame, then published and the counter
/// reset with an InterlockedExchange. That is shared mutable state with a work counter, the same
/// shape as the GI clipmaps and the shadow cascades, and the same shape that has produced every
/// "two versions alternating" symptom in this project. Whether the two views share it depends
/// entirely on the byte at view+5856 (0x16E0): different index, private record; same index, they
/// take turns filling one sky.
///
/// And the gate reads viewData+0xF94 -- the very field StreamlineHistoryFix WRITES, mirroring
/// MAIN's AA mode onto VRCAM. So our own fix is an input to the sky decision, which nobody knew.
///
/// Report both, for both views. Reading only; nothing here changes engine state.
/// answered: both views index sky record 0, AA mode 0
#[no_mangle]
pub static mut CyberpunkVR_SkyProbe: i32 = 1;
/// ---- the cloud lighting shader selector -----------------------------------------------------
///
/// sub_1407F36D8 RenderVolumetricCloudsLighting, decompiled:
/// v5  = *(int**)(ctx + 7592);        // ctx+0x1DA8, the per-view cloud state
/// v20 = v5[8] == 1;                  // an int at cloudState+32
/// if (v20) { shader -661749514 } else { shader -298149306 }
/// One int chooses between two different lighting shaders. If the views disagree on it they are
/// lighting their clouds with different permutations, which is exactly a brightness difference --
/// and it fits the audit, where this node costs the second view 2.2x what it costs MAIN, and the
/// clouds node 1.7x. More time means the heavier branch.
///
/// Everything upstream is already equal by measurement: viewData has no atmosphere field left
/// differing, the cloud CB's only mirrorable field (0x40) is mirrored, and the pass's other inputs
/// viewData+0x430 / +0x550 sit inside mirrored ranges. This selector is what is left.
///
/// Probe first, write second. CloudLightMirror defaults OFF: swapping a shader permutation is not
/// the same class of change as copying a float, and the measurement costs nothing.
/// answered: selector 2 on both; the state object is pooled per frame and not diffable across views
#[no_mangle]
pub static mut CyberpunkVR_CloudLightProbe: i32 = 1;

// ---- src/Stereo/Profiler.cpp

#[no_mangle]
pub static mut CyberpunkVR_ProfDispMainMs: f64 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_ProfDispVrcamMs: f64 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_ProfDispVrcamNodes: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_ProfDispMainNodes: u32 = 0;

// ---- src/Stereo/RenderParity.cpp

#[no_mangle]
pub static mut CyberpunkVR_RenderParityDebugCapture: i32 = 0;

// ---- src/Stereo/ScopeGraphDag.cpp

/// 1 = the scope's graph gets MAIN's dependency rules (next build). Read by the stub, live; set from
/// xr_scope_graph_dag (Core/LiveControlsPoll.cpp).
#[no_mangle]
pub static mut CyberpunkVR_ScopeGraphDag: u32 = 1;

// ---- src/Stereo/SubmitChain.cpp

/// 1 = the presenting view's Present node queues the present after the chain (see DEFERRED PRESENT); only
/// with the chain on. Set from xr_defer_present (default 1); the pause crash it was once switched off for is
/// closed (see DEFERRED PRESENT), and on 2.5 it passed 25 pause-menu round trips with a camera scope drawn.
/// Live for A/B tests.
#[no_mangle]
pub static CyberpunkVR_SubmitChainDeferPresent: AtomicU32 = AtomicU32::new(0);

// ---- src/Stereo/ViewReuse.cpp

/// --- what ELSE of the view block does the RTT view not get? --------------------------------
/// The cloud wind offsets turned out to be one instance of a pattern: sub-blocks of viewData
/// that the engine only ever fills for the primary view. ScreenSpaceRain is measurably another
/// -- it runs for VRCAM but early-outs, because its gate is
/// sub_14033C248(wc) + 0xAB0, floats at +52 / +72 / +76, at least one > 0
/// and those are zero for VRCAM (audit: the node is 119x cheaper there). So rather than chase
/// them one symptom at a time, diff the WHOLE view block once and let it name every hole.
/// Runs off the cloud hook because that already has viewData in hand for both views
/// (a4 == viewData + 0x430), so no extra engine call and no new hook.
/// MODE 2 for one run. Mode 1 lists only MAIN-set/VRCAM-zero holes, and the night difference is
/// not a hole -- both views have values, they are simply not the same values. It is reported
/// that VRCAM does not follow the dusk-to-dawn transition at all, which is a TIME failure: the
/// weather/time-of-day blend reaches MAIN's view block and not the second one. Mode 2 prints
/// every differing run, so the extent of that block gets named instead of guessed at.
/// DEFAULT 0, AND THE COMMENT THAT USED TO SIT HERE SAID SO WHILE THE VALUE SAID 2. It shipped at 2
/// -- the mode that prints EVERY differing run, 33 MB and sixteen thousand lines in a session -- on
/// the hottest per-view path there is. It is a measurement, not a mechanism, so it belongs at 0 with
/// a live key: `xr_viewdata_diff`, armed the moment the picture shows whatever is being chased and
/// switched off again straight after.
/// 1 = the holes only (MAIN has a value, the second view has zero)
/// 2 = every differing run, including the ones where both views hold their own value
#[no_mangle]
pub static mut CyberpunkVR_ViewDataDiff: i32 = 0;
/// Applied before every VRCAM node consumer; only the selected ranges below are
/// written into that view. The staging snapshot itself stays in our own storage.
/// ---- the distant-fog switch the two views disagree on --------------------------------------
///
/// Measured, both views, same frame, same spot:
/// 0x8C0 {M 1.5 | V 3}   0x8C8 {M 0.75 | V 0.5}   0x8CC {M 1 | V 4}
/// 0x918 {M 0.5 | V 1}   0x920 {M 0    | V 0.00025}
///
/// 0x920 is not a parameter, it is THE SWITCH. From sub_1407F3C20 (RenderFogOverlay):
/// if ((f0 & 0x7E7EA0) == 0 && !sub_14033AA6C(ctx+20)
/// || (v6 = 1, *(float*)(vd + 0x920) <= 0.0))   v6 = 0;
/// if ((f0 & 0x1082850) || (f0 & 0x47C3E60) || v6) { ... if (v6) <the distant-fog branch> }
/// v6 can only survive as 1 when vd+0x920 is strictly positive. MAIN holds exactly 0 there and so
/// never enters that branch; VRCAM holds 0.00025 and always does. The second eye runs a
/// distant-fog path the first eye does not have, with the parameters beside it 2x to 4x MAIN's.
/// A fog the other eye lacks, at four times the strength, buries the far city and washes the
/// stars out of the sky at once -- both reports, one field.
///
/// Deliberately NOT part of the hole table. Those are safe because VRCAM has nothing in them;
/// here it has real values, so copying MAIN's overwrites live per-view data. Start with the
/// Start with the switch alone if you must, but all three by default: every measured field
/// differed in the same direction, so leaving the parameters at VRCAM's 2x-4x only moves the
/// problem to whichever fog path it does take. The risk is real and stated -- unlike a hole
/// these overwrite live per-view values, and if one is resolution-derived (0x8CC reads
/// M 1 | V 4, which has that shape) the second eye will look wrong in a new way.
/// bit 0   0x920            the switch
/// bit 1   0x918, 0x91C     the scale pair
/// bit 2   0x8C0 .. 0x8D7   the parameter block
/// Live: 7 = all, 1 = switch only, 0 = untouched.
#[no_mangle]
pub static mut CyberpunkVR_FogMirrorMask: u32 = 7;
/// ---- the atmosphere block, mirrored in bisectable pieces -----------------------------------
///
/// Four captures, two at night and two through dawn, printing BOTH views. They split the night
/// difference in two, and only one half is weather.
///
/// Constant across all four -- configuration, not time:
/// 3F0 {M 1.365|V 4.55}   480 {M 1|V 0.75}    4D4 {M 0.03|V 0.06}
/// 570 {M 0.7  |V 0.4 }   5A4 {M 1|V 0.766}   5C0 {M 1.3 |V 0.3 }
/// 610 {M 8    |V 4   }   61C {M 0.05|V 0.03} 628 {M 0.65|V 1   }   6FC {M 1.4|V 1}
/// 8 against 4 and 0.03 against 0.06 have the shape of a march step count and its step size: the
/// engine hands the RTT view a cheaper atmosphere. That is the tint, the over-visible mountain
/// silhouettes, and why daylight looks the same -- at noon those coefficients barely register.
///
/// Moving, but not together -- the weather blend:
/// 430 sky radiance  M 8.17 -> 8.62 -> 8.89 -> 131.0
/// V 2.09 -> 2.09 -> 64.6 -> 68.1
/// 3C0 colour        M 0.157 -> 0.155 -> 0.153 -> 0.051
/// V 0.095 -> 0.095 -> 0     -> 0
/// The second view is not frozen; it takes the transition early and on different values, and 3C0
/// collapses to zero by dawn. MAIN's smooth 8 -> 131 arc against VRCAM's 2 -> 65 step is exactly
/// the reported "there is no gradual lightening".
///
/// So mirror MAIN's atmosphere block. This is a BROAD overwrite of live per-view values and it is
/// not a root fix -- the root is whatever hands the RTT view its own configuration, which is not
/// found yet. Split into bits so a bad range can be bisected out without a rebuild, and with two
/// exclusions that are not negotiable: 0x6E0 is a pointer (21B04B08 | 212ACC20) and 0x120..0x143
/// is the camera position, which MUST differ between eyes.
/// bit 0  0x370..0x3FF   sun direction and colours
/// bit 1  0x400..0x45F   radiance
/// bit 2  0x460..0x4FF   the 0x480 / 0x4C0 / 0x4D4 group
/// bit 3  0x500..0x5CF   the 0x554 / 0x570 / 0x598 / 0x5A4 / 0x5C0 group
/// bit 4  0x610..0x633 and 0x6FC   the march-step group
/// Bits 12-15 are the environment look the second view was never given: the colour grade, the
/// brightness/exposure group, the bloom/flare table and the chromatic aberration. All four were read
/// out of the two views in the SAME braindance with both views identified the only way that holds --
/// the virtual-camera name hash at work_context+0x18 -> +0x28, 0 for MAIN and the port's own key for
/// VRCAM -- taken on the port's cloud-CB detour where viewData is a4 - 0x430. The port was running
/// with mask 0xFFF at the time, so what was left differing is exactly what nothing else mirrors.
///
/// Every field that differed is included, packed words and integer-shaped fields as well, because the
/// loop copies a qword at a time and refuses any qword that looks like an address on either side. A
/// bad guess here can therefore look wrong; it cannot take the process down.
/// DEFAULT IS 0xFFFF, NOT 0x4D0D43, AND THAT IS A MEASURED RETREAT. Shipping all of bits 12-21 on
/// crashed the game before the main menu, twice, at 17 and 19 seconds, in a function family the
/// report queue had never shown before this build (exe+0x275704 and exe+0x2758E1, both with R14 =
/// FFFFFFFC -- a container index that had gone negative). Bits 16-21 are therefore OUT of the
/// default and stay switchable one at a time:
///
/// 16  scene bounds        +0120
/// 17  the zeroed block    +01A0..+01E7   contains +01A0 = 0x32B6, an integer
/// 18  packed words        +0330 +0738
/// 19  cloud wind          +05E8
/// 20  tagged union        +0FB8          MAIN stores ASCII 'game' where the view stores floats
/// 21  small integer       +0FE8          M 0x1461 against V 0x461
///
/// The guard makes each of them incapable of carrying an address, which is what it was for; it cannot
/// make an integer meaningful in a structure that counts with it. That is the distinction this
/// retreat is drawing, and it is the reason these are separate bits rather than one.
#[no_mangle]
pub static mut CyberpunkVR_EnvMirrorMask: u32 = 65535;
/// --- THE FOUR RESOURCE POINTERS THE ATMOSPHERE MIRROR LEAVES ALONE -------
///
/// Where this lands after three disproven guesses. The sky node's gates were measured and all three pass for
/// BOTH views (rect (0,0)..(2560,2560) non-empty, feature bit 35 set, the global set, node entered 6191 times),
/// so the second view does compute sky scattering and "MAIN-only" was an artefact of marker-based pass listing.
/// Reflection-probe reuse and local-shadow reuse were each armed and each changed nothing.
///
/// What no measurement has explained away is the split the capture found directly: the two views run the
/// atmosphere/volumetric-fog chain on DIFFERENT resources.
///
/// VRCAM  reads 26609  ->  writes 707881  ->  samples it 81 times
/// MAIN   reads 45412  ->  writes 26608   ->  samples it 78 times
/// 45412 is never written in the frame; 26609 is written by a copy at event 47295, i.e. during MAIN's
/// turn, AFTER the second view already read it
///
/// And the atmosphere mirror right above says where such a split would live: its own note lists what it refuses
/// to copy -- "0x6E0, 0xF58, 0xF88, 0xFB8 are pointers/resources". Those are per-view RESOURCE slots in
/// viewData, excluded on purpose because copying a pointer is not copying a float. If one of them is the LUT the
/// fog samples, that exclusion is exactly the split.
///
/// READ ONLY. Four pointers per view, printed side by side, so the next step is chosen from what is in them
/// rather than from a fourth guess. Nothing is copied: a resource pointer mirrored blind is how a view ends up
/// sampling a texture that has been freed.
#[no_mangle]
pub static mut CyberpunkVR_EnvPtrProbe: i32 = 1;
/// --- THE EMPTY-RECT GATE, read where the engine reads it -----------------
///
/// IDA named it, headless, on the existing database (tools/ida_sky_probe.py):
///
/// sub_1407F86CC  RenderSkyScattering node work
/// if ( sub_14033E358(a2, 35) )                     // feature bit 35
/// if ( !sub_14033AA6C(*(a2+24) + 20) )         // 24 = 0x18 = the view ctx, then +0x14
/// if ( *(qword_143BBCAE8 + 200) ) sub_1407F8714(...)   // the actual work
///
/// bool sub_14033AA6C(_DWORD *a1) { return *a1 >= a1[2] || a1[1] >= a1[3]; }
///
/// That is an EMPTY-RECTANGLE test on four dwords at ctx+0x14: x0>=x1 || y0>=y1. Sky scattering runs only
/// while that rect is NON-empty. The same predicate gates the distant-fog branch -- see the fog mirror note
/// above, which had already reverse-engineered `!sub_14033AA6C(ctx+20)` inside sub_1407F3C20 -- and it gates
/// RenderRainMap too: two of its 67 call sites, 0x3726F2 and 0x3727EB, are inside node 0x5AA54C.
///
/// So ONE pair of fields decides whether a view gets sky scattering, the rain map and the distant fog. That is
/// the shape of the reported symptom: interior lighting wrong in every way at once -- glasses lit blue like the
/// sky instead of yellow like the room, no haze in the distance, blown highlights, wrong window reflections --
/// rather than five separate bugs. And it agrees with the capture from the other side: RenderSkyScattering and
/// RenderRainMap are MAIN-only there, and the second view samples its own atmosphere LUT chain.
///
/// This is a PROBE and nothing else: it reads sixteen bytes and prints them per view. Fixing the rect before
/// knowing what is in it would be the guess this project keeps paying for.
#[no_mangle]
pub static mut CyberpunkVR_ViewRectProbe: i32 = 1;
/// --- Reflection-probe atlas reuse for VRCAM -----------------------------
/// THE ONE SHARED CACHE THIS PORT NEVER GUARDED. The reuse table had GI, distant shadows, local shadow maps
/// and the temporal history; env probes were not in it, and the capture says they should have been.
///
/// Measured on an interior capture (18506), per view, from the port's own pass markers:
///
/// VRCAM | ReflectionProbes   37 actions: 30+ draws of 3 indices into ResourceId::26530
/// (512x256, THIRTY-TWO array slices, R11G11B10_FLOAT -- the probe atlas)
/// with 1x1x1 dispatches interleaved, i.e. per-probe convolution
/// MAIN  | ReflectionProbes    1 action:  a single 80x80 dispatch and two copies. No probe rendering.
///
/// So the second view rebuilds the SHARED probe atlas every frame and MAIN does not -- the same shape as the
/// local-shadow thrash that this file already documents: a fresh view has nothing cached, so it forces a full
/// re-render into a cache MAIN relies on. Everything else in the specular path is symmetric (DrawConeAO 5/5,
/// PrepareFeedbackSSRBuffer_PreSSR 21/21, ScreenSpaceReflections 5/5), which makes this the only candidate left
/// for the symptom: interior lighting differs between the eyes, "shiny in VRCAM, normal in MAIN". A probe that
/// has just been rasterised and not yet convolved reads as a mirror rather than a rough reflection, which is
/// what "shiny" looks like.
///
/// Same ABI as the distant and local skips: a2 = rdx, view ctx = *(a2+0x18), key @ ctx+0x28, and the skipped
/// node returns 0 exactly as RenderLocalShadowMaps does.
///
/// NOT PROVEN, and the failure mode is written down first: if the second view genuinely needs its own probes,
/// its reflections go flat or stale instead of shiny, and this goes back to 0. Two earlier suspects for this
/// same symptom were eliminated by measurement -- RenderLightsIntegrate (identical bindings, identical output
/// statistics) and local shadow reuse (tested at 0, artefacts unchanged).
/// MEASURED HARMFUL AT 1, AND KEPT AT 0. Turning it back off fixed two things seen as part of
/// the interior bug: the glasses on the bar went from bluish back to the same yellow MAIN shows, and the black
/// patch on the sofa chairs disappeared. Both of those were MINE -- introduced by this knob, not by the port's
/// stereo path -- and they were mixed into the symptom list for an hour before that came out.
///
/// The mechanism is the one the capture had already shown and I armed the knob against anyway: the SECOND VIEW
/// is the one that FILLS the shared reflection-probe atlas (ResourceId::26530, 512x256 x32 slices -- 30 draws
/// in VRCAM's turn against none in MAIN's). Skipping its node does not make it reuse MAIN's work, because MAIN
/// does no probe work; it leaves the atlas to nobody. Unconvolved or stale probe faces read as a mirror and as
/// black, which is exactly what was seen. Reuse of a cache only helps when someone else fills it.
///
/// A separate lesson, worth more than the knob: DO NOT TREAT AN x64dbg READ AS A MEASUREMENT WITHOUT CHECKING
/// debug_get_state FIRST. DebugProbeSkipHits read as 0 with the debugger detached (state "stopped"), so the
/// expression resolved against the image instead of the process, and that zero was reported as evidence. The
/// log is the instrument that does not lie about this; the detour now reports itself there.
///
/// The detour now reports itself into the log, which needs no debugger: entries at zero means this detour is
/// not on the path at all; entries climbing with match=0 means the ABI assumption is wrong and the view
/// context is not at *(a2+0x18). The view KEY itself is known good -- the sky-node probe below reads
/// *(ctx+0x28) the same way and separates the views correctly thousands of times a run.
/// 1=skip vrcam's rebuild, 0=off
#[no_mangle]
pub static mut CyberpunkVR_ProbeReuseMode: u32 = 0;
/// ---- HOW FAR APART THE TWO VIEWS FIT THE CASCADE, in world units ---------------------------------
///
/// The observation in the headset is what this exists for, and it is the sharpest evidence of the day: with the reuse
/// armed, the shadows are CORRECT while the head is still and wrong the moment the head TURNS. Translation
/// does not do it; rotation does. A sun cascade is fitted to the viewer -- centre roughly position plus
/// forward times half the range -- so orientation moves the centre by metres where position moves it by
/// centimetres. Cascade 0's extent is +-18 world units, a 36 m box over maybe 2048 texels, i.e. ~1.8 cm per
/// texel: a couple of degrees of head turn is hundreds of texels.
///
/// So the two views do NOT fit the same cascade whenever the head is moving, and reuse fails for that reason
/// alone -- no barrier, no ordering, no second texture required to explain it.
///
/// AND THE EARLIER "0 differing dwords" HAS TO BE RE-READ IN THAT LIGHT. It was a yes/no over the record
/// bytes, taken while standing still, and it answered "are they ever different" with the head not moving.
/// This reports the MAGNITUDE instead, and its maximum over the interval, so head motion cannot hide inside a
/// boolean. If the centres separate by centimetres or more while turning, the same mechanism is a candidate
/// for the ORIGINAL defect too: two eyes whose shadow maps are fitted to poses sampled a few milliseconds
/// apart would disagree exactly where the shadow is a texel or two wide.
#[no_mangle]
pub static mut CyberpunkVR_CascFitProbe: i32 = 1;
/// ---- FINDING THE COMMAND LIST THE ENGINE HOLDS ---------------------------------------------------
///
/// The port's PIX markers are emitted 198038 times a run and appear in a RenderDoc capture exactly zero times.
/// Both numbers are measured, and together they say where we sit: our command-list vtable hook is on the REAL
/// D3D12Core list (distinctVtables=1, found through our own creation path), while the game records into
/// RenderDoc's WRAPPER. Anything we call on the real list is below the capture layer and is recorded by nobody.
///
/// So the marker has to go to the object the ENGINE holds, and this probe finds it by the one signature that
/// cannot be mistaken: a pointer inside the node's work context whose VTABLE lies inside renderdoc.dll. Nothing
/// else in this process has that shape. With a capture layer absent it reports d3d12core vtables instead, which
/// is the same slot and is how the offset is confirmed without a capture running.
///
/// Every dereference is guarded by VirtualQuery rather than by __try, deliberately: this project has already
/// established that the engine's own VEH swallows SEH first, so a bad read crashes the game even inside a
/// __try. Asking the OS whether a page is committed and readable is the only guard that actually holds.
#[no_mangle]
pub static mut CyberpunkVR_CmdListHunt: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_CompLendSet: u32 = 1;

// ---- src/Stereo/WeaponPlane.cpp

/// 0 = the game's own routing, first-person plane intact; 1 (DEFAULT since main's d7239a782) = the first-person draw
/// passes take the scene's view parameters. Live: flipping the key applies or reverts the byte in place.
#[no_mangle]
pub static mut CyberpunkVR_WeaponPlaneAsScene: i32 = 1;
/// THE HIGHLIGHT'S OWN PLANE FILTER, which is a different thing from the routing above and the one
/// that actually decides whether a held object can be outlined.
///
/// Measured, not assumed: with a katana in the hand and one variable changed at a time, the whole
/// gameplay half is plane-blind -- the renderer's proxy predicate accepts 4 of 4 in both planes, and
/// the highlight record is written 4 times in both. The claim is stored either way. What differs is
/// one byte on the render proxy, +0x9D, which holds the raw plane (2 for the weapon plane, 0 for the
/// scene), and the collection at PLANE_FILTER_JNZ_RVA drops every object whose byte does not match
/// the plane being gathered.
///
/// 0 = the game's own filter (DEFAULT). 1 = the jnz becomes a six-byte nop, so no object is dropped
/// for its plane. Live, and validated against the exact bytes before every write.
#[no_mangle]
pub static mut CyberpunkVR_HighlightAnyPlane: i32 = 0;
/// EVERY DRAW ITEM INTO THE SCENE BUCKET. This is the one that actually moves the first-person plane,
/// as opposed to the two knobs above, which were aimed at gates that turned out not to exist.
///
/// The plane is a draw-list index, not a filter (see PLANE_BUCKET_LOAD_RVA). Loading zero instead of
/// the proxy's plane puts every item on the scene pile, which is the pile the highlight gather walks --
/// so a weapon in the hand becomes outlineable without touching a single item.
///
/// What it costs is not known from measurement and should not be guessed at: forcing the scene
/// parameter block (the routing knob above) changed nothing on screen, and moving one katana to the
/// scene plane by event looked fine, so the usual first-person-plane arguments -- wall clipping, near
/// plane, its own field of view -- are not evidenced here. Default 0 all the same, because this moves
/// every draw in the frame and that deserves a deliberate switch.
#[no_mangle]
pub static mut CyberpunkVR_PlaneBucketScene: i32 = 0;
