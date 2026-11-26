//! The exported state tools/vrik_tests' Rust variants need and do not define themselves, copied verbatim from this
//! crate's export modules (the plugin's own types and initial values) by tools/rust_migration/rig_test_exports.py:
//! regenerate it after changing one of those definitions. Only the test build (feature rig-exports, without
//! rust-build) compiles it; the plugin has the modules themselves.
#![allow(non_upper_case_globals, dead_code, unused_imports)]
use core::sync::atomic::{AtomicI32, AtomicI64, AtomicU32, AtomicU64, AtomicUsize};


// ---- from input_exports.rs
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
/// HOW FAR THE RIGHT STICK MUST GO TO SNAP-TURN, and how far back before it can snap again. 0.90 puts
/// the snap where every other gesture in this file already is (sprint, crouch, dash) -- a deliberate
/// push to the stop, not the half-deflection it used to be, which is where a thumb rests and where a
/// wrist drifts while walking. The re-arm is not back at centre on purpose: 90 degrees is three snaps,
/// and a full return between each makes that slow; 0.90/0.50 is a 0.40 band no jitter crosses.
#[no_mangle]
pub static mut CyberpunkVR_SnapTurnStickFire: f32 = 0.90;
#[no_mangle]
pub static mut CyberpunkVR_SnapTurnStickRearm: f32 = 0.50;
/// How long the synthetic dodge press is held, milliseconds. A dodge is a button PRESS, so the pulse
/// only has to be long enough for the game to see an edge; held level it would dodge over and over.
#[no_mangle]
pub static mut CyberpunkVR_DashPulseMs: i32 = 100;
/// How many times the wheel-hub horn gesture fired while it had no button to press. Non-zero here
/// with xr_wheel_horn on means the gesture still works and only the binding is missing.
#[no_mangle]
pub static mut CyberpunkVR_DebugWheelHornGestures: u64 = 0;
// ---- from render_moved_exports.rs
/// The cyberware chord's sent keys (src/Hooks/CyberwareChord.cpp, or rust/crates/cvr-rig/src/vrik/cyberware_chord.rs).
#[no_mangle]
pub static CyberpunkVR_CyberwareChordSent: AtomicU64 = AtomicU64::new(0);
/// Names mirrored so far (diagnostics).
#[no_mangle]
pub static CyberpunkVR_LeftHandedNamesMirrored: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_LeftHandedNameLogCount: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_MenuTransitionSeq: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_MenuTransitionCount: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_StoryAttentionHookReady: i32 = 0;
#[no_mangle]
pub static CyberpunkVR_StoryAttentionCounters: [AtomicU64; 5] = [const { AtomicU64::new(0) }; 5];
/// gated: player calls / Pacifica / workspot / unavailable pose / explicit clue presses
/// gated: look-at rays turned for a hand
#[no_mangle]
pub static CyberpunkVR_ReachTurns: AtomicU64 = AtomicU64::new(0);
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
// ---- from declared_moved_exports.rs
#[no_mangle]
pub static mut CyberpunkVR_SceneAccessStats: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_SceneAccessRecordings: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_SceneAccessResources: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_SceneAccessEvents: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_SceneAccessChildren: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_TargetAccessStats: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_TargetAccessSources: [u64; 32] = [0; 32];
#[no_mangle]
pub static mut CyberpunkVR_TargetAccessChecks: [u64; 32] = [0; 32];
#[no_mangle]
pub static mut CyberpunkVR_TargetAccessFailureEvent: [u64; 8] = [0; 8];
#[no_mangle]
pub static mut CyberpunkVR_TargetAccessBreadcrumbStats: [u64; 8] = [0; 8];
#[no_mangle]
pub static mut CyberpunkVR_AsymInnerGuard: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_MonoView: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_EngineStereoEyeWindows: i32 = 1;
// ---- from used_moved_exports.rs
#[no_mangle]
pub static mut CyberpunkVR_AsymSquarePixels: i32 = 1;
// ---- from used_moved_exports_rest.rs
/// The controller whose names are mirrored, 0 while none is.
#[no_mangle]
pub static CyberpunkVR_LeftHandedController: AtomicUsize = AtomicUsize::new(0);
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
/// Spawns seen: [0] the main menu's, given the man's body; [1] a man's save repaired; [2] every other.
#[no_mangle]
pub static mut CyberpunkVR_DebugPlayerSpawns: [u64; 3] = [0; 3];
