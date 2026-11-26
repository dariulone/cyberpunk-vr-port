//! The input tunables, world markers', profiler's and render parity's exported state (formerly definitions in
//! src/Hooks/XInput.cpp, src/Hooks/WorldMarkers.cpp, src/Stereo/Profiler.cpp and src/Stereo/RenderParity.cpp that no C++
//! code of the Rust build reads any more): defined on the Rust side, with the notes that stood over them, and exported
//! from rust/exported-data.txt; the Rust modules and the tools read them by name.
#![allow(non_upper_case_globals)]
use core::sync::atomic::{AtomicI32, AtomicU32, AtomicU64};

// ---- src/Hooks/XInput.cpp

/// TEST INPUT: XInput button bits (0x4000 X, 0x2000 B, 0x0080 R3, ...) OR'ed into the VR controller state as if
/// the controllers pressed them, so a harness can drive the whole input path -- shared slots, the merge, the
/// game's own actions. The OpenXR simulator does not map the port's boolean actions; written by a test process
/// only, and 0 in normal play. Bits 16/17 hold the left/right grip fully: the simulator's pose command has no
/// grip, and the monowire deploys and swings only while one is held.
#[no_mangle]
pub static mut CyberpunkVR_TestPadButtons: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_MoveTiers: i32 = 1;
/// HOW LONG THE STICK MUST STAY AT THE STOP BEFORE IT SPRINTS, milliseconds. It used to be instant, and
/// with discrete speed that is wrong by construction: any push already runs at full speed, so the stick
/// naturally sits AT the stop and an instant detent sprints on every step. 200 ms separates walking
/// forward from leaning on it.
#[no_mangle]
pub static mut CyberpunkVR_SprintHoldMs: i32 = 200;
/// 0 (default, MEASURED) = a crouch blocks the detent. PSMLocomotionStates has a CrouchSprint state, so
/// this was worth asking for -- and the answer, in game, was that the player is STOOD UP and sprints.
/// That is the opposite of what a crouching player wants, so the block is the honest default. 1 asks
/// anyway, for a build or a cyberware where the game answers differently.
#[no_mangle]
pub static mut CyberpunkVR_SprintFromCrouch: i32 = 0;
/// THE RE-ENTRY BLIP. Sprint is a HOLD -- measured: ten press/release pulses never entered the sprint
/// state once, while a held button always has. But a game that has LEFT the sprint (a dash, a jump, ADS)
/// will not re-enter on a button that is already down, so the hold is briefly RELEASED to make the next
/// hold an edge. Held for SprintPulseMs at a minimum between blips, released for SprintGapMs.
#[no_mangle]
pub static mut CyberpunkVR_SprintPulseMs: i32 = 240;
#[no_mangle]
pub static mut CyberpunkVR_SprintGapMs: i32 = 60;
/// After this long without reaching the sprint, stop blipping and just HOLD. Holding is the behaviour
/// that shipped and worked, so it is the right thing to fall back to when the game is refusing us
/// (no stamina, a state that forbids it) -- a blip every 300 ms forever would only flicker it.
#[no_mangle]
pub static mut CyberpunkVR_SprintGiveUpMs: i32 = 1500;
/// The WALK band, opt-in: stick magnitudes up to this walk instead of run. 0 = no band, every push runs.
/// It is off by default on purpose -- a band is a slice of travel that has to be HELD, which is the thing
/// discrete speed exists to remove -- but the game gives a pad no walk button at all (ToggleWalk_Button
/// is IK_G, a key an XInput hook cannot press), so a band is the only way to reach a walk.
#[no_mangle]
pub static mut CyberpunkVR_MoveWalkMax: f32 = 0.0;
/// What the walk band outputs, and how much wider the band gets once you are in it (so a thumb resting on
/// the boundary does not flip between walk and run).
#[no_mangle]
pub static mut CyberpunkVR_MoveWalkMag: f32 = 0.35;
#[no_mangle]
pub static mut CyberpunkVR_MoveWalkMargin: f32 = 0.06;
/// The left trigger held off the game while it works a pump's fore-end -- a GetTickCount64 deadline, see the merge.
#[no_mangle]
pub static mut CyberpunkVR_LeftTriggerBlockUntil: u64 = 0;
/// DUAL WIELDING: each hand's trigger fires its own gun (see the merge). The deadline is held up by the
/// reload mod (SetVRDualWield) for as long as the game reports two guns in hand; the switch turns the
/// whole remap off, and the press delay is how long a new squeeze keeps the fire action released so the
/// script's hand routing reaches the right gun before the press does.
#[no_mangle]
pub static mut CyberpunkVR_DualWieldUntil: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DualWieldInput: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_DualWieldPressDelayMs: i32 = 40;
/// How long Y has to be held before it is the game's Y again while two guns are in hand; shorter is a tap,
/// the left gun's magazine drop (see the end of the merge).
#[no_mangle]
pub static mut CyberpunkVR_DualWieldYHoldMs: i32 = 300;
/// SCANNER GESTURE -- left hand at the left ear + left grip toggles the scanner.
#[no_mangle]
pub static mut CyberpunkVR_ScannerGesture: i32 = 1;
/// The ear, in HMD-LOCAL XR axes (x right, y up, z BACKWARD -- the published hand position is in the
/// headset's own frame, so this rides the head and works whichever way the player faces).
/// This locates the grip centre, below and outside the point where fingers touch the headset.
#[no_mangle]
pub static mut CyberpunkVR_ScannerEarX: f32 = -0.18;
#[no_mangle]
pub static mut CyberpunkVR_ScannerEarY: f32 = -0.08;
#[no_mangle]
pub static mut CyberpunkVR_ScannerEarZ: f32 = 0.08;
/// How near the ear point counts, and how much wider it has to get before it stops counting. The margin
/// is what keeps a hand sitting on the boundary from chattering.
#[no_mangle]
pub static mut CyberpunkVR_ScannerEarRadius: f32 = 0.22;
#[no_mangle]
pub static mut CyberpunkVR_ScannerEarMargin: f32 = 0.04;
/// The two debounce windows, milliseconds. Asymmetric on purpose: arming is a decision, dropping is a
/// dropout, so a bad tick has to be swallowed rather than believed.
#[no_mangle]
pub static mut CyberpunkVR_ScannerEnterMs: i32 = 80;
#[no_mangle]
pub static mut CyberpunkVR_ScannerExitMs: i32 = 250;
/// TOGGLE OR HOLD. The game's own binding is Vision_Hold_Button, i.e. the scanner stays open only
/// while LB is asserted -- so a held gesture was the shape that matched it, and holding a grip at the
/// ear for as long as the scanner is wanted is what that costs. With this on the gesture only FLIPS a
/// latch and the port keeps LB asserted itself: squeeze at the ear to open, squeeze at the ear again
/// to close, hand free in between. 0 restores the original hold.
#[no_mangle]
pub static mut CyberpunkVR_ScannerToggle: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_ScannerStickNav: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_ScannerStickNavFire: f32 = 0.90;
#[no_mangle]
pub static mut CyberpunkVR_ScannerStickNavRearm: f32 = 0.50;
/// SCANNER, WORKED WITH ONE HAND. What each of these has to become is not a preference: the game
/// binds the hack list to the D-PAD, applying to X, the scanner's tab to RB and the tag to the right
/// stick click. The block that emits them, down in the merge, names the file every one of those was
/// read out of. Each is separately switchable because each takes a button away from gameplay.
/// left Y up, left X down / apply
#[no_mangle]
pub static mut CyberpunkVR_ScannerFaceNav: i32 = 1;
/// right trigger tags, and does not fire
#[no_mangle]
pub static mut CyberpunkVR_ScannerTriggerTag: i32 = 1;
/// right stick click changes the tab
#[no_mangle]
pub static mut CyberpunkVR_ScannerStickTab: i32 = 1;
/// THE SCANNER'S ZOOM: hold the LEFT trigger and the right stick zooms, in the game's own steps.
/// The right stick is busy -- crouch, dash, pitch, snap turn -- so the trigger is the modifier that
/// borrows it, and while it is held the axis is consumed before any of those four sees it.
#[no_mangle]
pub static mut CyberpunkVR_ScannerZoom: i32 = 1;
/// How far the stick must go for a step. Lower than the 0.90 the crouch and dash gestures use on
/// purpose: those are deliberate shoves to the stop, this is a readout being nudged.
#[no_mangle]
pub static mut CyberpunkVR_ScannerZoomStick: f32 = 0.50;
/// How often a held stick repeats the step. ZoomIn_Button is a button, so holding it does not keep
/// zooming -- the press has to be made again, and this is the rate.
#[no_mangle]
pub static mut CyberpunkVR_ScannerZoomRepeatMs: i32 = 200;
/// How long X must be HELD to apply the selected hack instead of paging down one. The same split the
/// vehicle exit uses and for the same reason: one button with two meanings, the cheap one on the tap.
#[no_mangle]
pub static mut CyberpunkVR_ScannerApplyHoldMs: i32 = 350;
/// How long each synthesised press stays down. The game's listeners act on BUTTON_PRESSED, so this
/// only has to outlast one XInput poll -- it is not a repeat rate.
#[no_mangle]
pub static mut CyberpunkVR_ScannerPulseMs: i32 = 90;
/// HOW LONG X MUST BE HELD TO LEAVE A CAR, milliseconds. Not zero, and that is the whole point:
/// ExitVehicle_Button has no hold in the game's own mappings, so the vehicle state machine acts on
/// the first frame it sees -- and at speed that means jumping out. Level-mirroring X ejected the
/// player the moment the button was brushed while driving, which reads as a physics bug rather than
/// as a button. 400 ms is the timeout the game uses for its own hold bindings.
#[no_mangle]
pub static mut CyberpunkVR_VehicleExitHoldMs: i32 = 400;
#[no_mangle]
pub static mut CyberpunkVR_DashStickUp: i32 = 1;
/// HOW FAR THE RIGHT STICK MUST GO TO SNAP-TURN, and how far back before it can snap again: RAW horizontal travel,
/// not the deadzoned camera axis (main's 0.80/0.50: > 0.90 after the deadzone meant > 0.918 raw and missed slightly
/// diagonal flicks). A deliberate push with a wide hysteresis band, without requiring the stop.
#[no_mangle]
pub static mut CyberpunkVR_SnapTurnStickFire: f32 = cvr_body::policy::SNAP_TURN_STICK_FIRE;
#[no_mangle]
pub static mut CyberpunkVR_SnapTurnStickRearm: f32 = cvr_body::policy::SNAP_TURN_STICK_REARM;
/// How long the synthetic dodge press is held, milliseconds. A dodge is a button PRESS, so the pulse
/// only has to be long enough for the game to see an edge; held level it would dodge over and over.
#[no_mangle]
pub static mut CyberpunkVR_DashPulseMs: i32 = 100;
/// How long the synthetic R3 of the crouch gesture is held, milliseconds: short enough to stay a tap.
#[no_mangle]
pub static mut CyberpunkVR_CrouchPulseMs: i32 = 100;
/// 1 = a button bound to LB toggles the scanner as well (press opens, press closes); 0 = the game's raw hold.
#[no_mangle]
pub static mut CyberpunkVR_ScannerButtonToggle: i32 = 1;
/// A bounded background test stick: signed X/Y int16 in the low DWORD, a GetTickCount deadline in the high DWORD. No
/// controller replacement in normal play; a lost harness expires within ten seconds.
#[no_mangle]
pub static CyberpunkVR_TestPadLeft: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
/// A bounded background test pair of triggers, as the test stick: the left trigger 0..255 in bits 0-7, the right in bits
/// 8-15, a GetTickCount deadline in the high DWORD (ten seconds at most).
#[no_mangle]
pub static CyberpunkVR_TestPadTriggers: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
/// How many times the wheel-hub horn gesture fired while it had no button to press. Non-zero here
/// with xr_wheel_horn on means the gesture still works and only the binding is missing.
#[no_mangle]
pub static mut CyberpunkVR_DebugWheelHornGestures: u64 = 0;

// ---- src/Hooks/WorldMarkers.cpp

#[no_mangle]
pub static CyberpunkVR_WorldMarkerStereo: AtomicI32 = AtomicI32::new(1);
#[no_mangle]
pub static CyberpunkVR_WorldMarkerRoots: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_WorldMarkerQuads: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_WorldMarkerTexts: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_WorldMarkerCameraUploads: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];
#[no_mangle]
pub static CyberpunkVR_WorldMarkerTextUploads: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];
#[no_mangle]
pub static CyberpunkVR_WorldMarkerMappinStages: [AtomicU64; 7] = [const { AtomicU64::new(0) }; 7];
#[no_mangle]
pub static CyberpunkVR_WorldMarkerProjectionStages: [AtomicU64; 5] = [const { AtomicU64::new(0) }; 5];
/// 1 = under the asymmetric projection, move each view's flat HUD by that view's own projection shift
/// (see InkShift). 0 = the HUD composed at the same pixels in both views, as before.
#[no_mangle]
pub static CyberpunkVR_AsymHudShift: AtomicI32 = AtomicI32::new(1);
#[no_mangle]
pub static CyberpunkVR_WorldMarkerProbe: AtomicI32 = AtomicI32::new(0);
/// Unclamped markers whose point is off the canvas or outside either eye's window: 0 = not drawn, 1 = drawn at the
/// point (then seen by one eye only).
#[no_mangle]
pub static CyberpunkVR_WorldMarkerOffCanvas: AtomicI32 = AtomicI32::new(0);
#[no_mangle]
pub static CyberpunkVR_WorldMarkerProbeSequence: [AtomicU32; 2] = [const { AtomicU32::new(0) }; 2];
#[no_mangle]
pub static mut CyberpunkVR_WorldMarkerProbeWords: [[u64; 4]; 2] = [[0; 4]; 2];
#[no_mangle]
pub static mut CyberpunkVR_WorldMarkerProbeFloats: [[f32; 64]; 2] = [[0.0; 64]; 2];
#[no_mangle]
pub static CyberpunkVR_WorldMarkerReproject: AtomicI32 = AtomicI32::new(1);
#[no_mangle]
pub static CyberpunkVR_WorldMarkerWorldVertices: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_WorldMarkerWorldTexts: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_WorldMarkerWorldCameras: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];
#[no_mangle]
pub static CyberpunkVR_WorldMarkerTraceSequence: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_WorldMarkerTraceWords: [[u64; 8]; 64] = [[0; 8]; 64];
#[no_mangle]
pub static mut CyberpunkVR_WorldMarkerTraceFloats: [[f32; 48]; 64] = [[0.0; 48]; 64];
#[no_mangle]
pub static mut CyberpunkVR_WorldMarkerTextLast: [u64; 8] = [0; 8];
/// Live A/B, all on by default: markers hang level from their point (no head roll); exact, unrounded margins;
/// no pixel snapping of lifted geometry; an effect takes its marker's projection despite a few ownerless records.
#[no_mangle]
pub static CyberpunkVR_WorldMarkerLevel: AtomicI32 = AtomicI32::new(1);
#[no_mangle]
pub static CyberpunkVR_WorldMarkerExactMargin: AtomicI32 = AtomicI32::new(1);
#[no_mangle]
pub static CyberpunkVR_WorldMarkerUnsnap: AtomicI32 = AtomicI32::new(1);
#[no_mangle]
pub static CyberpunkVR_WorldMarkerCompositeLoose: AtomicI32 = AtomicI32::new(1);
/// Effects composed: [0] one marker's only, [1] one marker's with ownerless records, taken, [2] with ownerless
/// records, seen, [3] two markers', [4] no marker's, [5] records the port never saw allocated. The last mixed one:
/// root, owned records, ownerless records, tick.
#[no_mangle]
pub static CyberpunkVR_WorldMarkerCompositeStats: [AtomicU64; 8] = [const { AtomicU64::new(0) }; 8];
#[no_mangle]
pub static mut CyberpunkVR_WorldMarkerCompositeMixed: [u64; 4] = [0; 4];
#[no_mangle]
pub static CyberpunkVR_WorldMarkerExactMargins: AtomicU64 = AtomicU64::new(0);
/// A marker's vertices left on the canvas: [0] already tagged, [1] a z of their own, [2] inside an offscreen effect,
/// [3] no source camera, [4] the lift failed; with the last marker root of each.
#[no_mangle]
pub static CyberpunkVR_WorldMarkerUnlifted: [AtomicU64; 5] = [const { AtomicU64::new(0) }; 5];
#[no_mangle]
pub static mut CyberpunkVR_WorldMarkerUnliftedRoot: [u64; 5] = [0; 5];
/// Top-level paints of a marker root's descendant (Paint): count; last root, widget, levels up, tick.
#[no_mangle]
pub static CyberpunkVR_WorldMarkerDetachedPaints: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static mut CyberpunkVR_WorldMarkerDetachedPaint: [u64; 4] = [0; 4];
/// The layers of the marker root named here (with WorldMarkerProbe), a ring of 64: words record, kind | 256 lifted |
/// 512 offscreen | vertices << 16, tick, first vertex colour (half4); floats canvas bounds (text: the matrix's
/// translation, then the record's exact one), first and last vertex UV (raw 16-bit).
#[no_mangle]
pub static mut CyberpunkVR_WorldMarkerLayerRoot: u64 = 0;
#[no_mangle]
pub static CyberpunkVR_WorldMarkerLayerCount: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_WorldMarkerLayerWords: [[u64; 4]; 64] = [[0; 4]; 64];
#[no_mangle]
pub static mut CyberpunkVR_WorldMarkerLayerFloats: [[f32; 8]; 64] = [[0.0; 8]; 64];
#[no_mangle]
pub static CyberpunkVR_WorldMarkerEdgeClamp: AtomicI32 = AtomicI32::new(0);

// ---- src/Stereo/Profiler.cpp

/// published averages (ms) read by the overlay
#[no_mangle]
pub static mut CyberpunkVR_ProfBuildMainMs: f64 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_ProfBuildVrcamMs: f64 = 0.0;
#[no_mangle]
pub static CyberpunkVR_NodeTimelineCursor: AtomicU64 = AtomicU64::new(0);
/// Script-side trigger for CyberpunkVR_ProfDumpNodes (normally an overlay button): a nonzero write
/// makes the next published frame log the per-node audit and reset its window. The live
/// measurement (tools/single_pass_tests/node_cpu_audit.py) dumps once to start a clean window and
/// once to read it; tools/node_audit_md.py parses the log lines.
#[no_mangle]
pub static CyberpunkVR_ProfDumpRequest: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_ProfDumpCount: AtomicU32 = AtomicU32::new(0);
/// Dispatch census: does the node reach the second view AT ALL? If Vrcam stays 0 while Main
/// climbs, the node is not in that view's graph and the capability test is not the obstacle --
/// a different problem, and this hook cannot fix it.
#[no_mangle]
pub static mut CyberpunkVR_DebugHudNodeMain: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugHudNodeVrcam: u64 = 0;
/// How often the engine refused the second eye, and how often we overrode that refusal.
#[no_mangle]
pub static mut CyberpunkVR_DebugHudGateDenied: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugHudGateForced: u64 = 0;
/// OFF for the same reason as the gate override: lending the list only carried the node further
/// into state the view does not have. Kept because the measurement it produces (null vs ok) is
/// still worth having, and because it becomes correct once the capability bits are set properly.
#[no_mangle]
pub static mut CyberpunkVR_HudBorrowBlocks: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugHudBlockNull: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugHudBlockOk: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugHudBlockLent: u64 = 0;
/// ---- GRANT THE CAPABILITY INSTEAD OF SKIPPING THE TEST ---------------------------------------
///
/// Measured, the second view is short of exactly ONE bit of what DrawHUD asks for:
///
/// MAIN  w11 req=...0080 have=0000000000CFFFBF missing=0
/// VRCAM w11 req=...0080 have=0000000000CF6E3F missing=0000000000000080
///
/// word 11, bit 7 -- absolute feature 711. (The full difference between the two views in that
/// word is 0x9180, bits 7/8/12/15; the HUD needs only the first.) And note this has nothing to do
/// with the f0/f1 pair the [fgflags] path forces: VRCAM's f0/f1 are already a superset of MAIN's
/// (3C00017F vs 3C00017D), which is why FORCED never changed anything.
///
/// Why setting the bit is not the same thing as the override that crashed: the override answered
/// one question at the moment it was asked, deep inside the node, long after the work that
/// question guards was skipped. The bit is what the rest of the engine READS -- including,
/// evidently, whatever collects HUD elements, since VRCAM's draw-block list comes out empty on
/// every single frame (blocks null == vrcam dispatch count, exactly). Set early and left set, the
/// view genuinely declares the capability and the engine populates the state itself.
///
/// Whether that is enough is a measurement, not a claim: if the block list stops being null, the
/// collection ran and this was the right lever. If it stays null, collection is gated somewhere
/// else and this bit is only the last of several conditions.
///
/// RESULT: it was necessary but not sufficient. `denied` went to 0 with no crash -- the view now
/// passes the test honestly -- but the draw-block list stays empty on every frame.
///
/// And the obvious explanation is WRONG, so do not reach for it again: both views are built by the
/// SAME builder. The log says so directly ("VRCAM built via FULL (sub_141D43040)"), and VRCAM's
/// f0/f1 are a superset of MAIN's. Same graph, same nodes; the difference is in the DATA the
/// nodes find, not in which nodes exist. So the empty list has a writer that runs for one view and
/// not the other, and finding that writer is the next step -- not re-litigating the builder.
#[no_mangle]
pub static mut CyberpunkVR_HudGrantCap: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_DebugProfPairOverflow: u64 = 0;
/// ---- NODE CUT (census experiment): skip selected nodes at dispatch ----------
/// Rules match work-fn RVA (+ optional SceneDrv rtId) and view side. Armed live
/// from the overlay; every change is logged. Master switch default OFF. Used to
/// census which nodes are droppable per view (CPU win) without quality loss.
#[no_mangle]
pub static mut CyberpunkVR_NodeCutEnable: i32 = 0;
/// dispatch return when skipping
#[no_mangle]
pub static mut CyberpunkVR_NodeCutRetVal: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_DebugNodeCutSkips: u64 = 0;
/// Script-side node-cut rule (the overlay calls CyberpunkVR_NodeCutSet directly). One packed
/// request per Present: bits 0..31 work RVA, 32..39 rtId (0xFF any), 40..47 mode (0 off,
/// 1 cut-vrcam, 2 cut-both, 3 cut-main), bit 63 set so a zero request means "none".
/// Diagnostic only: CyberpunkVR_NodeCutEnable must be set separately and defaults to 0.
#[no_mangle]
pub static CyberpunkVR_NodeCutRequest: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_NodeCutApplied: AtomicU32 = AtomicU32::new(0);

// ---- src/Stereo/RenderParity.cpp

#[no_mangle]
pub static mut CyberpunkVR_RenderCachePerEye: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_EarlyAaMode: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_FogHistorySync: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_SkyRadianceSync: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_OpticsSkyRadianceSync: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_OpticsSkyRadianceDebug: [f32; 8] = [0.0; 8];
#[no_mangle]
pub static mut CyberpunkVR_OpticsSkyRadianceSeq: u32 = 0;
#[no_mangle]
pub static CyberpunkVR_OpticsSkyRadianceWrites: AtomicU64 = AtomicU64::new(0);
/// gated: cache salts / AA MAIN / AA swaps / AA misses / fog MAIN / fog applied /
/// fog different frame-or-owner / fog invalid pose / fog recycled resource / fog same handle
#[no_mangle]
pub static CyberpunkVR_RenderParityCounters: [AtomicU64; 10] = [const { AtomicU64::new(0) }; 10];
#[no_mangle]
pub static CyberpunkVR_RenderParityDebugSeq: [AtomicU32; 3] = [const { AtomicU32::new(0) }; 3];
#[no_mangle]
pub static mut CyberpunkVR_RenderParityDebug: [[u64; 10]; 3] = [[0; 10]; 3];
/// Gated MAIN observations / VRCAM writes / unavailable-or-invalid samples.
#[no_mangle]
pub static CyberpunkVR_SkyRadianceCounters: [AtomicU64; 3] = [const { AtomicU64::new(0) }; 3];
#[no_mangle]
pub static CyberpunkVR_SkyRadianceDebugSeq: AtomicU32 = AtomicU32::new(0);
/// native VRCAM / MAIN source
#[no_mangle]
pub static mut CyberpunkVR_SkyRadianceDebug: [f32; 8] = [0.0; 8];
/// Publications / borrows / unavailable / incompatible / upload failure /
/// camera observations / grid observations / invalid grids.
#[no_mangle]
pub static CyberpunkVR_FogReprojectionCounters: [AtomicU64; 8] = [const { AtomicU64::new(0) }; 8];
/// Gated, per view (MAIN, VRCAM): the fog camera's P20, P21 and camera[51] (the NDC jitter). P2x minus
/// the jitter is the projection's off-centre shift, which the borrowed fog history must keep.
#[no_mangle]
pub static mut CyberpunkVR_FogCameraDebug: [[f32; 4]; 2] = [[0.0; 4]; 2];
/// History borrows refused because the two views' froxel grids span different frusta (FogHistory.hpp).
#[no_mangle]
pub static CyberpunkVR_FogFrustumMismatch: AtomicU64 = AtomicU64::new(0);
/// Gated, per view: the volumetric-fog node's whole camera constants (212 floats) and the prepare-scene
/// fog constants (396 floats) as last seen -- to tell which fields carry the frustum.
#[no_mangle]
pub static mut CyberpunkVR_FogCameraDump: [[f32; 212]; 2] = [[0.0; 212]; 2];
#[no_mangle]
pub static mut CyberpunkVR_FogConstantsDump: [[f32; 396]; 2] = [[0.0; 396]; 2];
