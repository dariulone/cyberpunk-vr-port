//! Exports the C++ of the Rust build still reads through the declarations its headers carry, now defined on the Rust
//! side (each section names the source they came from, with the notes that stood over them) and exported from
//! rust/exported-data.txt; the C++ readers keep their header declarations, the Rust modules and the tools read them by
//! name.
#![allow(non_upper_case_globals)]
use core::sync::atomic::{AtomicU32, AtomicU64};

// ---- src/Core/VrCore.cpp

/// The fast-path block the PatchCamera trampoline reads. Defined here, beside the latches it
/// mirrors, so there is exactly one owner of both. See CameraState.hpp for why it is armed the way
/// it is.
/// THE SECOND EYE'S CAMERA IS ONLY REBUILT WHEN A DIRTY FLAG SAYS SO, AND NOTHING EVER SET IT.
///
/// Read out of the engine, sub_140AC316C -- the driver that owns the RTT camera each tick:
///
/// 140AC3183  lock cmpxchg [rdi+8E0h], ecx   ; rdi = comp+0x120, so the flag is comp+0xA00
/// 140AC3192  test    eax, eax               ; eax = the value that was there
/// 140AC3194  jz      loc_140AC31A8          ; it was 0 -> SKIP the build
/// 140AC31A3  call    sub_140D29AA8          ; builds the view from comp+0xE0 and comp+0xF0
/// 140AC31BD  jmp     sub_14048FAF4          ; the view is CREATED either way
///
/// So the transform we write into the component is consumed only on ticks where something has marked
/// the camera dirty. The creation runs regardless, which is exactly the symptom seen: animation and
/// the rest are fine, only the camera motion lags -- a fresh image every frame, built from a
/// camera nobody refreshed.
///
/// In ordinary play, and in a braindance that swaps the player for the replacer, the eye's parent moves
/// and the engine marks it dirty on its own. In a braindance that does NOT swap the player, the body
/// stands still for the whole replay and the flag is never raised: measured, sub_140D29AA8 was reached
/// 9.1 times a second in a braindance that looks right and ZERO times in the one that does not, with a
/// hardware breakpoint on the same address in the same session. Neither this port nor upstream has ever
/// set that flag, which is why the defect is in 0.1.5, in this branch and in upstream alike.
///
/// Bit 1 = raise the flag, bit 2 = write a fresh pose in first. Both, because the flag alone rebuilds
/// per frame from a pose that only PatchCamera refreshes -- and in that braindance PatchCamera reaches
/// the component 3.4 times a second (measured: +46 over 13.55 s).
///
/// THE HAZARD, stated because this project has already paid it: an older note by
/// CyberpunkVR_VrcamFovDeg records that "forcing comp+0xA00 or calling sub_140AC316C drags view-create
/// in, which hitched the game and hung the GPU". That was forcing it from the wrong place. Here it is
/// one store per view-create, on the thread that owns the driver, and only while a braindance is
/// running -- and it is a live key so it can be taken away without a build.
/// DEFAULT 0, AND IT REMOVED THE DEVICE. Raising comp+0xA00 by hand once per view-create hung the
/// GPU inside a minute -- DXGI_ERROR_DEVICE_HUNG 0x887A0006, frame rate collapsing first -- because
/// it forces the view rebuild while bypassing every piece of bookkeeping the engine does around it.
/// The flag is a RESULT of the engine's transform notify, not an input. Kept switchable only so the
/// experiment is on record; CyberpunkVR_BdPushTransform is the route that goes through the notify.
#[no_mangle]
pub static mut CyberpunkVR_BdCamDirty: i32 = 0;
/// THE SECOND EYE'S TRANSFORM, PUSHED THROUGH THE ENGINE'S OWN NOTIFY -- AND WHY THE RAW FLAG WAS
/// THE WRONG ANSWER.
///
/// The component's transform setter, sub_1401D8558, is where this port's PatchCamera hook already
/// lives (exe+0x1D8A8F is its `movups [rdx], xmm0`, the quaternion store). Read out around it:
///
/// 1401D8A73  movsd   [rsi+0E0h], xmm0      ; position
/// 1401D8A8F  movups  [rdx], xmm0           ; quaternion, rdx = rsi+0xF0   <- our hook
/// 1401D8A92  cmp     byte [rsi+0B1h], 0    ; a "force the notify" request byte
/// 1401D8AAF  cmp     r9d, [rsi+0E0h]       ; ...otherwise: did the transform actually change?
/// 1401D8AD6  call    sub_1401D9958         ; quaternion compare, with an epsilon
/// 1401D8AF1  call    qword ptr [rax+240h]  ; CHANGED -> the engine's own "transform updated"
/// 1401D8AF7  mov     byte [rsi+0B1h], 0
///
/// That virtual is what eventually raises the RTT camera's rebuild flag at comp+0xA00, and forcing
/// that flag directly instead -- which is what the first attempt did, once per view-create on the
/// render thread -- hung the GPU inside a minute: DXGI_ERROR_DEVICE_HUNG, 0x887A0006, with the frame
/// rate collapsing first. Bypassing an engine notification to poke the state it maintains is how that
/// is earned; the older note by CyberpunkVR_VrcamFovDeg had already said as much.
///
/// So the transform goes in the way the engine puts it in: write the fields, then call the component's
/// own vtable+0x240 with the arguments the setter uses (rcx = component, rdx = component+0x100).
/// From LocateCamera, because that runs once per frame ON THE GAME THREAD -- the thread the setter
/// itself runs on -- and because the pose it composes is the one MAIN is about to render from, so both
/// eyes come from one composition.
///
/// WHY IT IS NEEDED AT ALL: the setter is only called when something moves the component's parent. In
/// ordinary play, and in a braindance that swaps the player for the replacer, that is every frame. In
/// a braindance that does NOT swap the player, the body stands still for the whole replay and the
/// setter was measured at 3.4 calls a second against 70+ rendered frames -- for BOTH cameras, which is
/// why the first eye is affected too.
///
/// BIT 1 = the second eye. Confirmed on the picture: VRCAM stopped lagging the moment it was armed,
/// which also answers the obvious objection -- VRCAM is normally written at PatchCamera, but
/// PatchCamera IS this setter, and in that braindance the setter is not called. This does the
/// setter's work instead, every frame, so the second eye stops depending on it.
/// BIT 2 = MAIN's own camera component, the same way.
///
/// MAIN needs it for the same reason, and it is worth saying which half of MAIN's problem it fixes.
/// The replay is rendered through the located buffer, whose position the engine fills from MAIN's
/// camera component -- the same setter, the same standing body, the same 3.4 calls a second. So
/// MAIN's VIEWPOINT is stale exactly as the second eye's was. Its ROTATION is a separate question:
/// with CyberpunkVR_BdQuatFromWriteSite on, that comes from whatever PatchCamera last composed, which
/// is also 3.4 times a second -- pushing it through the notify makes it arrive reliably, not sooner.
/// Both keys stay live so the two can be told apart in one session.
///
/// Bit 2 is OFF by default: MAIN's component is the FPP camera the body, the arms and the weapon all
/// hang off, so a write there reaches more than the picture.
#[no_mangle]
pub static mut CyberpunkVR_BdPushTransform: i32 = 1;
/// BOTH EYES FROM ONE COMPOSITION AT ONE RATE -- the missing quarter of it being MAIN's viewpoint.
///
/// In a braindance the four quantities that place the stereo pair came from three different cadences,
/// and every knob tried so far only moved which one lagged:
///
/// MAIN   rotation   the frame's own composition        per frame
/// MAIN   position   the engine, from the component     at the setter's rate
/// VRCAM  rotation   the same composition, pushed       per frame
/// VRCAM  position   the scene pose, pushed             per frame
///
/// One of the four at a different rate is not "a bit of lag in one eye": it is the pair coming apart,
/// and it shows in EVERY braindance, including the one that otherwise looks right -- which is what
/// showed in the headset and what the rate-based auto-switch started first would have papered over.
///
/// This closes it: MAIN's viewpoint is written into the located buffer from the same per-frame scene
/// pose the second eye is placed with, so all four move together. The half-IPD is NOT applied here --
/// CyberpunkVR_BdIpdInLocate already adds MAIN's half further down, and the push adds the second eye's
/// with the opposite sign, so the pair stays symmetric about this base.
///
/// No engine notify is needed and none is made: nothing else consumes this buffer, and it is the
/// descriptor the replay is rendered through.
#[no_mangle]
pub static mut CyberpunkVR_BdMainPosFromScene: i32 = 0;
/// WHAT THE SECOND EYE IS PLACED RELATIVE TO.
/// 0  the scene pose the CET script publishes
/// 1  MAIN's own located position for THIS frame
///
/// 1, and the reason 0 was ever needed is gone. Two things named it:
///
/// * rare jerks in both eyes in playback -- both eyes at once means the shared source is
/// stepping, and the only shared source was the script's scene pose. CET's update is not strictly
/// per frame, so it steps occasionally; placing the pair relative to MAIN's own position takes the
/// script out of the pair's geometry entirely.
/// * VRCAM above MAIN in the editor -- in the editor the scene owns nothing, the scene pose
/// is invalid, the push stood down and the eyes fell back to their own bases: MAIN's located
/// camera against the second eye's attachment point on the body, which sits at a different height.
///
/// And "pos from main" was already tried and rejected as lagging -- correctly, because it was consumed
/// at PatchCamera, which runs BEFORE LocateCamera publishes it, so it was always a frame old. The push
/// lives INSIDE LocateCamera, where this frame's own value is in hand, so the same idea carries no lag
/// at all. That is the whole difference between the two attempts.
#[no_mangle]
pub static mut CyberpunkVR_BdPushBase: i32 = 1;
/// ONE COMPOSITION AND ONE WRITER FOR THE PAIR, WHILE THE BRAINDANCE PUSH OWNS THE SECOND EYE.
///
/// Two separate desyncs, both from the same shape -- the same quantity produced twice.
///
/// THE SAMPLE. With compose-at-write on, PatchCamera samples the head AGAIN at the write site, so the
/// second eye is built from this site's sample while MAIN is built from the one LocateCamera composed.
/// Two head samples for one stereo pair is a skew between the eyes however small the gap, and
/// CyberpunkVR_BdQuatFromWriteSite was an attempt at the same problem from the other end: it made MAIN
/// adopt THIS value. That is right only where this site runs every frame -- in the braindance without
/// the replacer it runs 3.4 times a second, so adopting it pinned MAIN to that rate, seen as MAIN
/// lagging. Standing the local composition down instead leaves exactly one composition, taken once per
/// frame in LocateCamera, and both eyes read it.
///
/// THE POSITION. The push writes the second eye's full viewpoint -- MAIN's centre plus half an IPD.
/// PatchCamera then READS that back, adds the head displacement and half an IPD again, and stores it:
/// the same offsets counted twice, alternating with the push's own value. That is the left eye
/// juddering in the editor while MAIN was clean, seen exactly that way.
///
/// Both are suppressed only while the push is armed AND a braindance is running, so nothing outside
/// that window changes, and 0 restores the old behaviour whole.
#[no_mangle]
pub static mut CyberpunkVR_BdOneComposition: i32 = 1;
/// THE BRAINDANCE EDITOR'S HEIGHT MISMATCH.
///
/// While a braindance runs but the scene does NOT own the camera -- the editor, where the player flies
/// the camera themselves -- the two eyes fall back to their own bases: MAIN to the camera the engine
/// locates, the second eye to its attachment on the player's body, which sits at a different height.
/// Seen as VRCAM positioned above MAIN in the editor, and it came back the moment the
/// braindance push was correctly confined to playback.
///
/// WHICH base, and the first answer was wrong. MAIN's COMPONENT position -- CyberpunkVR_EngineCamPosFP
/// -- is the camera on the player's BODY, and in the editor the body stands still while the player
/// flies a free camera: MAIN renders from the located buffer, not from that component. Placing the
/// second eye on the body therefore put it somewhere the world was not prepared for it, and the left
/// eye flashed black every few frames.
///
/// The located centre is the place MAIN actually renders from, so that is the base. It is published by
/// LocateCamera, which runs AFTER this site, so it is one frame old -- and that is the right trade
/// here: a frame of lag on a camera the player flies by hand is invisible, while the wrong PLACE is
/// not. In playback this branch does not run at all; there the base is the scene pose.
#[no_mangle]
pub static mut CyberpunkVR_BdEditorAlign: i32 = 1;
/// THE SCENE'S ROTATION, TAKEN FROM THE ENGINE RATHER THAN FROM LUA.
///
/// The head is composed onto the scene camera's own rotation while a replay plays, and that base came
/// from g_bdSceneQuat -- published by the CET script, whose update is not per frame. So the base
/// stepped while the head moved smoothly, and the pair juddered exactly when the engine was driving
/// the scene: once the engine drives the scene the poses fall out of step and the view jerks.
/// Position had already been taken off the script; this is the other half.
///
/// The engine's own value is in hand and costs nothing: the located buffer is filled from the camera
/// system BEFORE this callback runs, so the quaternion at entry is the scene camera's rotation for this
/// frame. It is captured before a single field is written, which is the only ordering that matters --
/// this hook writes that same quaternion further down, and reading it back afterwards would be reading
/// our own output. The script's value stays as the fallback for the frames before the first locate and
/// for anything the plausibility test rejects.
#[no_mangle]
pub static mut CyberpunkVR_BdBaseFromLocate: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_DebugFinalMatch: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugFinalNoMatch: u64 = 0;
/// measured depth
#[no_mangle]
pub static mut CyberpunkVR_DebugFinalAge: u32 = 0;
/// How many ring entries the frame's quaternion matched. 1 = unambiguous. Above 1 means the head
/// moved less than the tolerance between writes, which is the case the ordered pick exists for.
#[no_mangle]
pub static mut CyberpunkVR_DebugFinalTies: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugFinalTieHits: u64 = 0;
/// 1 = label the submitted frame with the pose read back out of the engine at frame-open.
/// 0 = the previous arrangement, which assumed the frame at present N used the write of N-1.
#[no_mangle]
pub static mut CyberpunkVR_PoseReadBack: i32 = 1;
/// 1 = hold the gamepad LT back on foot with empty hands, so striking the smoking lighter does not
/// also pull the camera into aim-zoom. 0 = vanilla LT everywhere, for anyone not using that mod.
/// Driving is never gated: no weapon is equipped in a car, and that is where LT is the brake.
#[no_mangle]
pub static mut CyberpunkVR_LtLighterGate: i32 = 1;
/// 1 = both cameras get the head translation in the COMPONENT (PatchCamera). 0 = the old
/// split, where MAIN took it in LocateCamera's serialised buffer and was therefore weighted
/// by the blender while its orientation was not. See the use site.
/// 1 = MAIN takes the head translation in the COMPONENT, the same way VRCAM always has.
/// 0 = the old split, where MAIN took it in LocateCamera's serialised buffer.
///
/// The component is the right home: the buffer entry is a blender CameraSetup, and the blender
/// multiplies every field by the camera's weight -- so MAIN's translation was weighted while its
/// orientation, which travels through the component, was not. On any two-camera transition the two
/// disagreed. Through the component both are weighted identically.
///
/// (A first attempt at this looked like it had failed -- the view rode the character's head. That
/// was an unrelated regression in the same session: the VRIK cleanup had replaced HALF of a
/// coherent pair, taking hmdRel from a fresh head sample while the controllers it un-rotates came
/// from an older publication. The argument that settled it is simple: the
/// ORIENTATION reaches the blender through this very field and works, so the field is read after we
/// write it, and position cannot be the exception.)
/// 1 = MAIN takes the head translation in the COMPONENT, the same way VRCAM always has; 0 = the
/// old split, where MAIN took it in LocateCamera's serialised buffer.
///
/// The component is the right home: the buffer entry is a blender CameraSetup and the blender
/// multiplies every field by the camera's weight, so MAIN's translation used to be weighted
/// while its orientation -- which travels through the component -- was not. Through the
/// component both are weighted identically.
///
/// Note for anyone reading a symptom here: head translation, the camera bake and the
/// Tracking/Camera sliders are ONE vector (worldDelta), so if the view ever stops following the
/// head, the offset sliders go dead in the same instant. Flip this live to separate that from
/// anything in the VRIK path: python vrprobe.py translation 0|1.
#[no_mangle]
pub static mut CyberpunkVR_HeadTranslationInPatch: i32 = 1;
/// How far AHEAD of the tick heading to aim the camera, in ticks. 0 = the tick heading, which is
/// what the body's tick is; the body itself is drawn with an INTERPOLATED entity transform, so
/// somewhere between 0 and 1 is the value that matches what is on screen. Not guessable from
/// here (the serialise buffer is a verbatim component copy, so there is no render-rate heading
/// to compare against) -- turn the knob until the trail on a fast mouse turn disappears, and
/// that reading is the answer. Read CyberpunkVR_DebugHeadingStepDeg to see the scale involved.
#[no_mangle]
pub static mut CyberpunkVR_HeadingLeadFrames: f32 = 0.0;
/// Peak |heading change| per composition, in degrees -- the size any phase error can have.
/// Cleared by whoever reads it.
#[no_mangle]
pub static mut CyberpunkVR_DebugHeadingStepDeg: f32 = 0.0;
/// THE BODY-YAW CENSUS. Peak-held per window, cleared by the reader in OpenXRPresent.
/// lag  = |view heading - entity world yaw| in degrees: how far behind the drawn body is.
/// step = |entity yaw change| per fresh solve: the turn rate the lag has to be read against.
/// hips = |hips MODEL yaw change| per fresh solve: zero means no bone carries the turn and the
/// yaw lives entirely in the entity transform. Sampled in AnimPose, where the solve is.
#[no_mangle]
pub static mut CyberpunkVR_DebugBodyYawLagDeg: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_DebugBodyYawStepDeg: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_DebugHipsYawStepDeg: f32 = 0.0;
/// THE PHASE OF THE BODY YAW against the animation batch. Peak-held per window, cleared by the
/// reader. yawToSolve = ms between sub_140336390 storing the yaw and our fresh solve reading it:
/// near zero means the yaw of this frame exists before the pose is baked, near a frame period
/// means it does not. lagWrite = the same disagreement in degrees, our heading against the yaw
/// that site actually stored -- no CET push in that path. See src/Hooks/BodyYawCensus.cpp.
#[no_mangle]
pub static mut CyberpunkVR_DebugYawToSolveMaxMs: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_DebugYawToSolveMinMs: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_DebugYawLagWriteDeg: f32 = 0.0;
/// WHERE THE SECOND EYE TAKES ITS POSITION FROM.
/// 0  its own attachment, plus whatever the branches in PatchCamera hand it (current behaviour)
/// 1  MAIN's base in a braindance, but ONLY where the scene does not own the camera -- that is
/// the editor, where the player flies the camera. In playback the scene pose wins and this
/// mode changes nothing: reported from the picture, playback went wrong the moment MAIN's
/// base was preferred there.
/// 2  MAIN's base wherever there is no scene pose, braindance or not
/// 3  the second view sits on MAIN's HEAD CENTRE as located, and has no head translation of
/// its own at all. Whatever MAIN carries the second eye carries, because it is the same
/// number -- no per-view delta to switch on in one mode and off in another.
///
/// Mode 1 exists because of a measurement rather than a theory. In a braindance the second eye takes
/// the pose the SCRIPT publishes for the scene camera (g_bdScenePosFP, written from RemoteCamera),
/// and the field log says that is not where MAIN renders from:
///
/// mainPos  = (-1720.973, -1235.205, 23.665)
/// vrcamPos = (-1722.116, -1236.144, 23.544)
///
/// 1.48 m apart horizontally and 12 cm vertically, where the residual after the head delta should be
/// one IPD. MAIN's base needs no identification and no pose hunt: it is read from MAIN's own
/// component at +0xE0 before anything of ours moves it, which is by definition where the first eye
/// renders. Default 0 until it has been seen on the picture.
/// DEFAULT 0, AND MODE 3 IS NOT THE ANSWER -- the paragraph above is the older claim and it did not
/// survive the picture. Mode 3 takes the second view's base from the located centre, and that base is a
/// TICK OLD relative to what MAIN renders from, so the second eye trails the first; this was reported
/// twice (key 3 lags behind MAIN), and the braindance path deliberately takes its base
/// from elsewhere for the same reason. Retried once anyway while chasing a 5.5 m divergence in an AV and
/// it changed nothing there, because that divergence had another cause entirely (a turret published as a
/// taken-over surveillance camera). Leave this at 0 unless a measurement says otherwise.
#[no_mangle]
pub static mut CyberpunkVR_VrcamPosFromMain: i32 = 0;
/// THE VIEWPOINT OF A TAKEN-OVER CAMERA, in metres, in the LENS's own frame: x right, y forward, z up.
///
/// A slider, because the point is wrong rather than unknown: manning the AV's
/// turret in the Silverhand mission puts the view at the turret's own camera, which is not where a person
/// sitting there would have their eyes (the view comes from the wrong point). No amount of identification fixes
/// that -- it is a number only the person looking can choose.
///
/// YAW-ONLY FRAME, deliberately. The mount is nose-down (measured at 9.4 degrees elsewhere in this file),
/// so composing the full lens orientation would send "forward" into the ground and "up" through the
/// fuselage. Forward is the aim's horizontal direction, up is world up, which is what a slider labelled
/// up/down has to mean.
///
/// Applied to BOTH eyes, after the lens position is established and before the IPD split, so it can never
/// become a disparity between them -- the same shape the IPD half itself uses, which is why re-applying it
/// every pass is safe here (the engine rewrites +0xE0 between passes; if it did not, the IPD would spiral
/// too and this port would have noticed long ago).
/// A TAKEN-OVER CAMERA GOES THROUGH THE LOCATED BUFFER, the same route a braindance scene uses.
///
/// Measured at the AV turret, and by an A/B flipped in the panel: with the scene path open MAIN's
/// orientation tracked the head again and the second eye stopped jerking; with it closed both stopped. The
/// reason is in this file's own machinery -- the located buffer takes its base from the BUFFER the engine
/// is filling this frame (CyberpunkVR_BdBaseFromLocate), so it does not depend on which object the
/// classifier has latched. And the latch cannot be trusted here: in that mission four objects answer to
/// the component name `camera`, the latch changed 696 times in six seconds, and the census had MAIN 3.8 km
/// from the second eye with a 1.7 m "head delta".
///
/// What it does NOT reuse is the scene POSE: during a takeover the scene camera is somewhere else
/// entirely, and taking MAIN's position from it is what made MAIN teleport for a frame. The position stays
/// the buffer's own -- the lens the engine is rendering through -- and only the eye split is added.
/// DEFAULT 2 SINCE 2026-09-05, CONFIRMED ON THE PICTURE. On a story `SurveillanceCamera` mode 1 left
/// the camera on its own state route, where turning the head yanked the view back to the lens and then
/// span it steadily; on this route it is steady. Mode 2 is a superset of 1 -- an unclaimed camera (the
/// turret) behaves identically -- so the only behaviour it changes is that of CLAIMED cameras, which
/// is exactly the case that was broken. Drop it to 1 to get the old split back if some other device
/// camera ever regresses.
#[no_mangle]
pub static mut CyberpunkVR_DevCamInLocate: i32 = 2;
/// THE NAME IS NOT THE IDENTITY -- the header of VRRemoteCamera says so in as many words, and then the
/// claim below tested the name anyway. It cost this: the AV turret's camera component is NOT called
/// `cameraComponent`, so on first entering the turret nothing was ever claimed -- no lens, no fov write,
/// no push -- and the picture stayed at the mount's own 60 degrees. Toggling the scene path on and off
/// "fixed" it only because the fov-match claim runs without the name test and left the address latched.
/// Seen exactly that way: on entering the turret the fov does not change; switching scene A/B on and
/// off again seems to fix it.
///
/// 1 = trust the published position alone, which is what the script publishes it for. 0 = the old
/// name-and-position test, kept because this is the identification path and a wrong claim here moves the
/// picture.
#[no_mangle]
pub static mut CyberpunkVR_DevCamAnyName: i32 = 1;
/// THE FIRST EYE'S HALF-IPD IN A BRAINDANCE, APPLIED IN LocateCamera INSTEAD OF PatchCamera.
///
/// In ordinary play both eyes take their separation at the component: PatchCamera writes -half into
/// MAIN's and +half into VRCAM's, and the located buffer is filled from the component afterwards, so
/// it arrives already carrying it. A braindance has no camera component for MAIN at all -- the replay
/// drives a virtual camera the port cannot reach, which is why MAIN's ORIENTATION is already written
/// from LocateCamera there. The position half was still going to the FPP component, which renders
/// nothing in that mode, so it reached no picture.
///
/// Measured in braindance playback, current build:
/// scene camera (live, through the CET bridge)   -1722.111 -1236.101 23.551
/// VRCAM as patched                              -1722.112 -1236.133 23.551
/// ipd 0.0640, half 0.0320 -- and the gap between them is 0.032
/// So the second eye carried its half and the first carried none: the eyes were separated by HALF an
/// IPD, which reads as everything being twice as far away. 1 puts MAIN's half into the located buffer,
/// which is the descriptor the engine renders the braindance through (measured 0.9995 against it).
/// Only while the scene owns the camera -- in the editor the component is live again and PatchCamera
/// is the right site, exactly as in ordinary play.
#[no_mangle]
pub static mut CyberpunkVR_BdIpdInLocate: i32 = 1;
/// THE SCENE'S QUATERNION AS THE COMPOSE BASE AT THE WRITE SITE TOO, so both eyes are composed
/// onto the same thing in a braindance. Without it the second eye sat on the player's body
/// heading, which a braindance never turns, while MAIN followed the replay -- two bases, one
/// stereo pair. 0 restores the split.
/// 1 = in a braindance MAIN writes the quaternion the WRITE SITE composed, so the pixels and the
/// submitted pose label come from one head sample. 0 = this site composes its own, which is what
/// left MAIN juddering in the headset while the second eye, written at the write site, did not.
#[no_mangle]
pub static mut CyberpunkVR_BdQuatFromWriteSite: i32 = 0;
/// DISPROVEN AND OFF, kept only so the experiment is not repeated. Basing the braindance head compose
/// on the located buffer's own quaternion looked strictly fresher than the script pose -- and it is
/// the same array this hook WRITES further down, so when the engine has not refilled it the base is
/// our own previous output and the head is composed onto itself. Seen as judder that got worse.
/// The warning was already in this codebase: reading our own output back as a base is what once made
/// the camera spin up without bound.
/// 0  the pose the script publishes (a tick old, but never fed back)
/// 1  DISPROVEN, do not use: the buffer taken unconditionally, which reads our own output back
/// 2  the buffer, but only on a call where it does NOT hold what we last published
/// DEFAULT 0, AND BOTH NON-ZERO MODES ARE DISPROVEN ON THE PICTURE. The judder they were written
/// for was never the age of the base: it was the two eyes being composed onto DIFFERENT bases,
/// and once the write site was given the scene's own quaternion the base here has to stay the
/// same one or the split comes back. Kept only so the experiment is not repeated.
#[no_mangle]
pub static mut CyberpunkVR_BdQuatFromBuffer: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugHeadingLedComps: u64 = 0;
/// 1 = put the eye separation into the component's WORLD POSITION (component+0xE0), above the view
/// producer, so culling / shadows / distant pass / motion vectors all see the eye they are drawn
/// for. 0 = do not separate the cameras at all.
#[no_mangle]
pub static mut CyberpunkVR_IpdInWorldPos: i32 = 1;
/// THE HEAD, WRITTEN INTO THE LENS THE MAIN VIEW IS RENDERED FROM.
///
/// MAIN's own component renders nothing during a takeover -- the engine renders the main view through the
/// taken-over camera, which is why writing THAT component's fov through RTTI widened the picture while the
/// port's own fov force never reached it. So MAIN's orientation cannot be given to MAIN's component: with
/// the second eye driven from the lens and nothing driving the lens, the left eye followed the head and the
/// right eye did not, seen exactly that way.
///
/// The position carries the viewpoint slider (CyberpunkVR_DevCamOffset*, yaw-rotated the same way
/// PatchCamera applies it to the second eye) so the slider moves the PICTURE, not one eye, and the eye
/// separation is MAIN's half -- BdPushTransformOnce computes it from the same numbers the second eye uses,
/// with the engine's own change notification after the write.
/// DEFAULT 0. Writing the composition into the lens COMPONENT does not turn the main view -- tried and
/// it does not work -- because the view is built from the located buffer, not from that component's
/// rotation (its fov IS read from there, which is why the fov fix works at the component). Kept as a live
/// key rather than deleted, since it is one line away from being the right answer if the route ever changes.
#[no_mangle]
pub static mut CyberpunkVR_LensHeadWrite: i32 = 0;
/// How close a cameraComponent has to sit to the published camera to be believed. The published point is
/// the ENTITY's origin and the component sits at the lens, so this is not a few centimetres; cameras in
/// the game stand metres apart, so 1.5 m separates them without being tight enough to miss the mount.
/// ...AND IT IS A KNOB NOW, because 1.5 m was measured against a camera bolted to a wall. On the AV
/// turret the mount flies: the script publishes the lens position every frame, but a frame of travel plus
/// the tick the value spends in flight is metres, and the claim simply never happened -- measured
/// DebugPatchCamDevice = 0 for a whole session in that seat, which took the fov, the lens and the second
/// eye's base with it. The latch is sticky (one camera per takeover), so a generous FIRST claim costs
/// nothing: after it the identity is fixed by address.
/// 1.5 m, the value measured against a wall-mounted camera. It is a FALLBACK only: with an entity id
/// published the identity decides and this is not consulted, so widening it buys nothing.
#[no_mangle]
pub static mut CyberpunkVR_DevCamTolM: f32 = 1.5;
#[no_mangle]
pub static mut CyberpunkVR_DebugPatchCamMain: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPatchCamVrcam: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_StereoModuleLoaded: i32 = 0;

// ---- src/Hooks/SwapChain.cpp

/// The vtable slot registry is rust/crates/cvr-engine/src/vtable.rs; the A/B switch stays exported from here.
#[no_mangle]
pub static CyberpunkVR_FastOriginalLookup: AtomicU32 = AtomicU32::new(1);

// ---- src/Render/MonoCascades.cpp

#[no_mangle]
pub static CyberpunkVR_MonoCascades: AtomicU32 = AtomicU32::new(1);
#[no_mangle]
pub static mut CyberpunkVR_MonoCascadeReportSeq: u32 = 0;

// ---- src/Render/NativeDepthPreparation.cpp

#[no_mangle]
pub static CyberpunkVR_EagerDepthMode: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_EagerDepthCompatible: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_EagerDepthVerified: AtomicU32 = AtomicU32::new(0);

// ---- src/Render/NativeEarlyDepth.cpp

#[no_mangle]
pub static CyberpunkVR_EarlyDepthRunBindingChecks: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];

// ---- src/Render/NativeGeometryLeases.cpp

#[no_mangle]
pub static CyberpunkVR_GeometryLeaseSeq: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_GeometryLeaseCompatible: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_GeometryLeaseFailure: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_GeometryLeaseDrawCount: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_GeometryLeaseResourceCount: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_GeometryLeaseDraws: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_GeometryLeaseResources: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_GeometryLeaseReadStates: u64 = 0;

// ---- src/Render/NativeGeometryPackets.cpp

#[no_mangle]
pub static CyberpunkVR_GeometryPacketHookCompatible: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_GeometryPacketRequest: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_GeometryPacketState: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_GeometryPacketSeq: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_GeometryPacketCount: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_GeometryPacketDataCount: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_GeometryPacketResolve: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_GeometryPacketInputCount: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_GeometryPacketInputFailure: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_GeometryPacketInputs: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_GeometryPacketInputBlob: u64 = 0;
#[no_mangle]
pub static CyberpunkVR_GeometryPacketInputBlobBytes: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_GeometryOriginMode: AtomicU32 = AtomicU32::new(1);
#[no_mangle]
pub static CyberpunkVR_GeometryOriginCompatible: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_GeometryOriginVerified: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_GeometryOriginStats: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];

// ---- src/Render/NativeGeometrySnapshots.cpp

#[no_mangle]
pub static CyberpunkVR_GeometryFreezeEnable: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_GeometryFreezeSeq: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_GeometryFreezeCount: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_GeometryFreezePending: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_GeometryFreezeFailure: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_GeometryFreezeRecords: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_GeometryFreezeBytes: u64 = 0;

// ---- src/Render/NativeInstanceObserver.cpp

#[no_mangle]
pub static CyberpunkVR_NativeInstanceObserverEnable: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_NativeInstanceObserverInstalled: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_NativeInstanceObserverResetRequest: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_NativeInstanceObserverFirstLoss: AtomicU32 = AtomicU32::new(0);
/// The observer's counters: the first 19 per status, then writer calls, completed writers, cached writers and
/// unjoined publications (instance_observer_native.rs).
#[no_mangle]
pub static CyberpunkVR_NativeInstanceObserverDiagnostics: [AtomicU64; 24] = [const { AtomicU64::new(0) }; 24];

// ---- src/Render/NativePairedGeometry.cpp

#[no_mangle]
pub static CyberpunkVR_PairedGeometryRequest: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PairedGeometryState: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PairedGeometryOrdinal: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PairedGeometryFamily: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_PairedGeometryStats: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_PairedGeometryDifferent: [u64; 12] = [0; 12];
#[no_mangle]
pub static mut CyberpunkVR_PairedGeometryBytes: [u64; 2] = [0; 2];
#[no_mangle]
pub static mut CyberpunkVR_PairedGeometryCameraDiagnostics: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_PairedGeometryChanged: [u64; 12] = [0; 12];

// ---- src/Render/NativeSceneAccess.cpp

#[no_mangle]
pub static CyberpunkVR_SceneAccessEnable: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_SceneAccessSeq: AtomicU32 = AtomicU32::new(0);
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

// ---- src/Render/NativeSourceCbvSnapshot.cpp

#[no_mangle]
pub static CyberpunkVR_SourceCbvSnapshotsEnable: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_SourceCbvSnapshotsUploadEnable: AtomicU32 = AtomicU32::new(0);

// ---- src/Render/NativeSourceSrvBinding.cpp

#[no_mangle]
pub static CyberpunkVR_SourceSrvBindingEnable: AtomicU32 = AtomicU32::new(0);

// ---- src/Render/NativeStereoCameraUpload.cpp

#[no_mangle]
pub static CyberpunkVR_SinglePassTwinUpload: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_SinglePassTwinAllocations: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_SinglePassTwinBindings: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_SinglePassTwinFailures: AtomicU32 = AtomicU32::new(0);

// ---- src/Render/NativeStereoProbe.cpp

#[no_mangle]
pub static CyberpunkVR_NativeStereoPipelineCount: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_NativeStereoProbeAssetStatus: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_NativeStereoAllDraws: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_NativeStereoBindingHooks: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_NativeStereoLiveBindings: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_NativeStereoLiveFailure: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_NativeStereoProbeRequest: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_NativeStereoProbeState: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_NativeStereoProbeSeq: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_NativeStereoDrawCount: AtomicU32 = AtomicU32::new(0);

// ---- src/Render/NativeStructuredBufferSnapshot.cpp

#[no_mangle]
pub static CyberpunkVR_StructuredBufferSnapshotsEnable: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_StructuredBufferFinalizeDiagnostic: [u64; 8] = [0; 8];

// ---- src/Render/NativeSubmissionFrontier.cpp

#[no_mangle]
pub static CyberpunkVR_SubmissionFrontierEnable: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_SubmissionFrontierInstalled: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_SubmissionFrontierState: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_SubmissionFrontierFailure: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_SubmissionFrontierSeq: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_SubmissionFrontierBoundaryFailure: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_SubmissionFrontierStats: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_SubmissionFrontierDetails: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_SubmissionFrontierRanges: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_SubmissionFrontierSlots: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_SubmissionFrontierSubmits: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_SubmissionFrontierRangeCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_SubmissionFrontierSlotCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_SubmissionFrontierSubmitCount: u32 = 0;

// ---- src/Render/NativeTargetAccess.cpp

#[no_mangle]
pub static CyberpunkVR_TargetAccessSeq: AtomicU32 = AtomicU32::new(0);
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

// ---- src/Render/NativeUploadArena.cpp

#[no_mangle]
pub static CyberpunkVR_UploadArenaEnable: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_UploadArenaSeq: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_UploadArenaStats: [u64; 16] = [0; 16];
/// One row per arena event (upload_arena::MaxRows), read by the tools.
#[no_mangle]
pub static mut CyberpunkVR_UploadArenaRows: [[u64; 16]; 4096] = [[0; 16]; 4096];

// ---- src/Render/NativeUploadReservation.cpp

#[no_mangle]
pub static CyberpunkVR_UploadReservationEnable: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_UploadReservationCount: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_UploadReservationDropped: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_UploadReservationPrepareRequest: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_UploadReservationAcceptRequest: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_UploadReservationExternalPlanSeq: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_UploadReservationExternalReceiptSeq: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_UploadReservationExternalInstallState: AtomicU32 = AtomicU32::new(0);

// ---- src/Render/SceneGpuProfile.cpp

#[no_mangle]
pub static CyberpunkVR_SceneGpuRequest: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_SceneGpuReportSeq: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_SceneGpuGroupMode: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_SceneGpuNodeFilter: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_SceneGpuViewportDetail: AtomicU32 = AtomicU32::new(0);

// ---- src/Render/SinglePassTrace.cpp

#[no_mangle]
pub static CyberpunkVR_SinglePassTraceRequest: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_SinglePassTraceState: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_SinglePassTraceSeq: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_SinglePassTraceCount: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_SinglePassTraceNodeFilter: AtomicU32 = AtomicU32::new(0);

// ---- src/Render/StereoDescriptorAudit.cpp

#[no_mangle]
pub static CyberpunkVR_DescriptorCaptureSeq: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_DescriptorDrawCount: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_DescriptorBindingCount: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_DescriptorIdCount: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_DescriptorValueCount: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_DescriptorCaptureFailure: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_DescriptorDraws: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DescriptorBindings: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DescriptorIds: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DescriptorValues: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DescriptorStoreStats: [u64; 6] = [0; 6];
#[no_mangle]
pub static mut CyberpunkVR_DescriptorHeapSlots: u64 = 0;
#[no_mangle]
pub static CyberpunkVR_DescriptorDenseSlots: AtomicU32 = AtomicU32::new(1);
#[no_mangle]
pub static CyberpunkVR_DescriptorDenseActive: AtomicU32 = AtomicU32::new(1);
#[no_mangle]
pub static CyberpunkVR_DescriptorLiveMetadata: AtomicU32 = AtomicU32::new(1);
#[no_mangle]
pub static CyberpunkVR_DescriptorLiveMetadataActive: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_DescriptorPendingLists: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_DescriptorRootCount: u32 = 0;

// ---- src/Runtimes/OpenXRFrameLoop.cpp

#[no_mangle]
pub static mut CyberpunkVR_XrDeepDiag: i32 = 1;

// ---- src/Stereo/AsymProjection.cpp

/// 0 = the engine's symmetric projection. 1 = each eye's own runtime frustum
/// (Runtimes/AsymmetricProjection.hpp). 2 = the test values below, for measuring the engine.
#[no_mangle]
pub static mut CyberpunkVR_AsymProjection: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_AsymInnerGuard: f32 = 0.0;

// ---- src/Stereo/Capture.cpp

/// ===== VRCAM MIRROR OUTPUT ==================================================
/// Standalone borderless window + own D3D12 swapchain that mirrors the vrcam RTT
/// texture every frame (CopyResource -> our backbuffer -> Present). Capture in
/// OBS via Window/Game Capture. Fully toggle-gated + self-disabling on any error
/// so it can never take down the game. v1: quality/sync refined later.
/// Default OFF: the mirror is a debug/streaming view, not part of the VR path, and it costs a
/// second swapchain + a per-frame copy. Turned on from the overlay ("VRCAM Mirror"). Safe to
/// flip at any time: the present thread is started lazily from the vrcam blit submit, so with
/// this at 0 it simply never starts, and switching to 1 later starts it on the next vrcam frame.
/// D3D11On12 mirror (OBS-style, non-blocking)
#[no_mangle]
pub static mut CyberpunkVR_MirrorOutput: u32 = 0;
/// ---- THE HUD IS ITS OWN TEXTURE, AND IT CAN SIMPLY BE COPIED ---------------------------------
///
/// Settled from two independent Nsight captures rather than from theory. The HUD is NOT drawn into
/// the scene colour; the composition group renders it into a dedicated full-resolution surface:
///
/// DiscardResource(hudTex, NumSubresources = 5)
/// ClearRenderTargetView(mip0, {0,0,0,0})        <- cleared TRANSPARENT
/// OMSetRenderTargets(1, mip0); RSSetViewports(output res)
/// ... ~29 ink quad draws (stride-24 verts, blend ONE / INV_SRC_ALPHA) ...
/// OMSetRenderTargets(0, nullptr)                <- the snapshot below goes exactly here
/// ... mips 1..4 generated for the HUD glow ...
///
/// and RenderFinal2D then composites that surface over the scene in a single fullscreen draw.
///
/// Those draws are recorded into the "PostFX" command list, OUTSIDE the DrawHUD node's dynamic
/// extent -- which is why the earlier node-scoped OMSetRenderTargets probe saw nothing and I
/// concluded, wrongly, that no separable HUD surface existed. It does.
///
/// Identification is exact, not heuristic: across all 23209 resources in the capture, exactly ONE
/// is an RGBA8 render target with MipLevels != 1. That is the whole signature.
///
/// Blend ONE / INV_SRC_ALPHA means the surface holds PREMULTIPLIED alpha, so the second eye wants
/// out.rgb = hud.rgb + eye.rgb * (1 - hud.a)
/// which is what ColorBlit::RecordOverlay does.
///
/// Nothing here touches engine state: the copy is a barrier pair around a CopyResource appended to
/// the engine's own list, the same shape as mirror_stable_inline_copy, which has been stable.
#[no_mangle]
pub static mut CyberpunkVR_HudToSecondEye: i32 = 0;
/// ---- THE SECOND EYE'S FINISHED FRAME -----------------------------------------------------------
///
/// The port used to submit a texture remembered at ApplyTXAA -- mid-chain, R11G11B10_FLOAT, with a THIRD
/// of the frame at exactly zero where the finished image has none (measured over the whole 3072x3072
/// picture). The finished image is a full-size R8G8B8A8_TYPELESS written by a COMPUTE dispatch through a
/// UAV inside the second view's CompositionPostProcess, so no render-target substitution can reach it and
/// no UAV->SRV publish barrier exists for it (nothing downstream reads it in that view).
///
/// What DOES exist is the engine's own ACQUIRE barrier on that texture, which the census read as
/// `DrawHUD 3072/f27 RENDER_TARGET -> UNORDERED_ACCESS` in the second view's window. So the barrier hook
/// learns the pointer AND the state there -- no guessing, no patch to the command stream -- and the
/// epilogue of CompositionPostProcess does the copy, which is the first moment the contents are final and
/// still before anything aliases the transient's heap.
#[no_mangle]
pub static mut CyberpunkVR_FinalGrab: i32 = 1;

// ---- src/Stereo/CullVisibility.cpp

/// 0=off, 1=skip VRCAM cull, 2=skip MAIN cull (diag), 4=force VRCAM query localCtx(+0x348)=0, 5=REUSE MAIN block-list v5, 6=unsafe graph-output experiment, 7=replay VRCAM tagged visibility, 8=mode7 + skip duplicate MAIN candidate gather, 9=mode8 + replay fine drawable IDs
#[no_mangle]
pub static mut CyberpunkVR_CullReuseMode: u32 = 0;
/// OBSERVE-ONLY DEFAULT (2026-07-31): Enable=1 with ApplyMask=0 reads the threshold each view
/// actually gets and writes nothing. Two night symptoms -- no stars, and distance lost to fog
/// while MAIN is fine -- are both "a distant drawable is not there", and the note above says the
/// engine FORCES this threshold to 0.1 for some view kinds instead of 1.0. If VRCAM is one of
/// them it culls far geometry hard, which is exactly both symptoms. Set ApplyMask=2 and
/// LodThreshValue to MAIN's reading to hand VRCAM the same threshold -- live, no rebuild.
/// answered: detail threshold is 1.0 on both views
#[no_mangle]
pub static mut CyberpunkVR_LodThreshOverrideEnable: u32 = 1;

// ---- src/Stereo/DeviceHooks.cpp

#[no_mangle]
pub static mut CyberpunkVR_DebugExecTotal: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMirrorGameQueue: u64 = 0;

// ---- src/Stereo/Dlss.cpp

/// RETIRED/vestigial: core upscale + crop-fix now key off VrcamDlss ALONE (see Detour_RenderRes). FlagCompute drives the native downscale from VrcamDlss. Kept default=1 for overlay/back-compat (only gates dormant STAGE2 / diagnostics now).
#[no_mangle]
pub static mut CyberpunkVR_VrcamDlssScale: u32 = 1;
/// AUTOMATIC, NOT A SETTING. Written by Detour_FlagCompute from MAIN's own upscaler groups and
/// read everywhere else; there is no overlay switch any more.
///
/// It was never a real choice. Every gate that consumes it already refused to act unless MAIN had
/// group 69 set, so the player-facing switch was only ever the redundant half of an AND -- and the
/// half that could be wrong. On it does two things, both of which only make sense while MAIN is
/// upscaling: it stands vrcam up in its own Streamline viewport, and it makes vrcam render below
/// its target and upscale rather than run DLAA.
///
/// Without the separate viewport the two views share viewport 0, which is not merely "vrcam gets
/// no DLSS of its own" -- it actively corrupts MAIN. Both views push camera matrices and jitter
/// into the same viewport and evaluate against the same temporal history, so MAIN's history is
/// interleaved with frames from a camera that is somewhere else. DLSS then resolves against a
/// history that keeps jumping, which reads as distant geometry shimmering under head rotation, and
/// it disappears entirely with the VRCAM component off, because then nothing else touches
/// viewport 0. So when MAIN is on DLSS, this being on is the correct state, not an opt-in.
///
/// 0 while MAIN is not upscaling (DLAA, TAA, no upscaler, or before the first graph build) --
/// which is exactly the old default, so nothing changes for those setups.
/// derived; do not set by hand
#[no_mangle]
pub static mut CyberpunkVR_VrcamDlss: i32 = 0;
/// distinct SL viewport id
#[no_mangle]
pub static mut CyberpunkVR_VrcamDlssViewport: i32 = 1;

// ---- src/Stereo/EngineStereo.cpp

#[no_mangle]
pub static mut CyberpunkVR_EngineStereo: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_MonoMode: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_MonoView: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_Afw: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_EngineStereoEyeWindows: i32 = 1;

// ---- src/Stereo/FrameGraph.cpp

/// LIVE A/B control for distant-shadow reuse (write via x64dbg to isolate distant
/// in VRCAM):  0 = distant OFF for vrcam (bit 11 cleared -> node no-ops, no distant
/// shadows).  1 = distant REUSE (bit 11 kept SET + vrcam's distant work skipped ->
/// vrcam samples main's distant map).  Flip 1->0->1 and watch vrcam's far-field
/// (out-the-window) sun shadows disappear/reappear = proof distant reuse is live.
#[no_mangle]
pub static mut CyberpunkVR_DistantReuseMode: u32 = 1;
/// Force VRCAM camera to match MAIN instead of relying on the RTT-camera asset.
/// Camera params live at ctx+0x70 (a3-view-params). We copy PROJECTION from main
/// (lens/fov/aspect/near/far at a3+0x10..0x48 = ctx+0x80..0xB8, and the inv/fwd
/// proj matrices at a3+0x150..0x1D0 = ctx+0x1C0..0x240) but KEEP vrcam's own view
/// matrix/position (a3+0x50..0x140 = ctx+0xC0..0x1B0) so steer/IPD still apply.
/// Resolution: force vrcam render dims (ctx+0x44/0x48) + rect to main's W/H.
/// (LOD/culling in this engine is screen-space-error driven -> forcing resolution
/// + fov to main makes VRCAM's LOD selection match main automatically.)
/// NOTE: ForceVrcamRes via ctx dims is INVALID  the RTT view is created with dims
/// == its RTT texture (2444^2); forcing ctx+0x44/0x48 to a different W/H makes the
/// view fail validation at init -> VRCAM never renders (absent from Nsight). To
/// change VRCAM resolution, resize the RTT TEXTURE (dynamicTextureRes asset) so the
/// dims derive correctly. ForceVrcamCam copies MAIN's camera SCALARS (fov/zoom/near/
/// far) into the vrcam view-ctx so both eyes match under gameplay fov + weapon ZOOM.
/// Live-confirmed ctx layout: +0x90 fovV, +0x9C zoom, +0xB0 nearZ, +0xB4 farZ. It does
/// NOT touch orientation (+0x80..0x8C) or aspect (+0x98, vrcam
/// keeps its own for its square RTT dims) and copies NO proj matrix. Default ON.
/// vrcam fov/zoom/near/far = main
#[no_mangle]
pub static mut CyberpunkVR_ForceVrcamCam: u32 = 1;
/// ---- WHICH CAMERA FIELDS THE SECOND VIEW TAKES FROM MAIN -------------------------------------
///
/// ForceVrcamCam above copies MAIN's fov/zoom/near/far onto the second view's camera context, and
/// the note beside it records what that context is for: writing it steers CULLING only. What the
/// second view actually RENDERS with is its component's own fov at comp+0x128, set further down.
/// Two sources, kept in step by the fact that they normally agree.
///
/// They stop agreeing the moment a scripted beat drives MAIN's camera, and then the copy is
/// actively harmful, because one of the things it steers is the CLUSTERED LIGHT GRID:
///
/// ClusteredLightsCull   bins every light into a froxel grid over the frustum -- and the
/// frustum it uses is the one in the view context, i.e. MAIN's
/// RenderLightsIntegrate maps each pixel to a froxel -- using the frustum the second view is
/// actually rendered with, i.e. its own
///
/// The grid is then filled for one frustum and read with another, every pixel lands in a bin that
/// was never filled, and the analytic light contribution disappears while emissive, sky and
/// ambient carry on -- which is the reported defect exactly.
///
/// How this was pinned, in order. Skipping ClusteredLightsCull for the second view reproduces the
/// picture one for one, while skipping RenderLightBuffers gives something else entirely. Every
/// other account was measured away: only two views run that cull (by camera-name hash), the
/// grid-write gate holds for both in the same 2:1 ratio over thousands of calls, the capability
/// gate passes for both, the light list is ONE shared object with the same count for both, and the
/// capture's submission order has MAIN's own cull running after the second view has already
/// sampled the grid, so nothing overwrites it. A mid-frame race was the next guess and is dead
/// too: sampling the four scalars at the cull and again at the integrate gave ZERO drift over
/// 1929 pairs while the defect was on screen. What did fix it, live, was switching the copy off.
///
/// So the copy is the cause and the VALUE is the problem, not its timing. Bisected on the picture
/// field by field: NEAR is the one that does it, which is what the shape of the symptom already
/// suggested -- the grid's depth axis is sliced logarithmically between near and far, so a near that
/// belongs to another camera moves every slice boundary at once and every pixel reads a bin nobody
/// filled. A wrong fov would have put the light in the wrong PLACE; a wrong near removes it.
///
/// Shipped default is fov + zoom, which is what this mechanism exists for -- matching MAIN's field
/// of view and its ADS zoom so culling and LOD agree with the flat view. The depth range is not
/// MAIN's to lend: the second view renders with its own, so it must cull with its own too. Dropping
/// far alongside near for the same reason, rather than keeping a half-borrowed frustum that happens
/// not to break today.
///
/// One bit per field, live, so the bisection stays repeatable instead of becoming a number to trust:
///
/// bit 0 = fov      bit 1 = zoom      bit 2 = near      bit 3 = far
///
/// F is the old behaviour, 3 is what ships.
#[no_mangle]
pub static mut CyberpunkVR_VrcamCamFields: u32 = 3;
#[no_mangle]
pub static mut CyberpunkVR_DebugMainCamFov: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMainProjYY: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_MainAdsZoomFactor: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_DebugVrcamWantFov: f32 = 0.0;

// ---- src/Stereo/Grading.cpp

/// OFF: grading upload-ring capture (scanner tint, parked)
#[no_mangle]
pub static mut CyberpunkVR_GradeUpProbe: i32 = 1;

// ---- src/Stereo/Hud.cpp

/// The descriptor signature above is NOT unique, and a log taken in the inventory proves it:
///
/// [hud] surface res=...  1024x1024 mips=11 fmt=27
///
/// That is the character-portrait render target -- 1024x1024, a full mip chain, RGBA8 -- and it
/// satisfies every clause. It was then adopted as "the HUD" and composited into the second eye,
/// which is the yellow full-screen face the menu showed. The signature was validated against one
/// gameplay capture ("exactly one such resource in the capture"); menus simply have another one.
///
/// So name it the way the outline and the sight were named: by WHO DRAWS INTO IT. rva 0x33E388 is
/// CRenderNode_DrawHUD, the node the RenderMask/HUD descriptor gates, and the render target bound
/// while it is on the stack is the HUD surface by construction. Once the node has spoken, the
/// descriptor path stops adopting entirely, so no menu resource can take the surface over.
///
/// If that node never binds one -- a graph we have not seen -- nothing is named, the flag stays
/// false and today's behaviour continues unchanged. This cannot be worse than what it replaces.
/// MEASURED, after 0x33E388 turned out to bind nothing at all. The [hudnode] table, live:
///
/// 20A264 : 2560x2560 m5 f29  (M675/V0)     CRenderNode_DrawComposition
/// 1F8928 : 2560x2560 m5 f29  (M2696/V0)    CRenderNode_CompositionPostProcess
///
/// Those two, and only those two, ever bind a HUD-shaped target -- the same surface each time,
/// 2560x2560 with 5 mips in sRGB, exactly the one the capture shows the 2D HUD geometry drawn
/// into. 0x33E388 is where the RenderMask/HUD capability is TESTED, which is not the node body
/// that binds; mistaking one for the other is what made the first attempt a silent no-op.
///
/// Note what is ABSENT from that table: the inventory's portrait. It is created as an RTV -- which
/// is why the descriptor test saw it -- but never bound through this path, so keying on the node
/// excludes it by construction rather than by another shape rule.
#[no_mangle]
pub static mut CyberpunkVR_HudNodeRva: u32 = 4070176;
#[no_mangle]
pub static mut CyberpunkVR_HudNodeRva2: u32 = 3395756;
#[no_mangle]
pub static mut CyberpunkVR_HudByNode: i32 = 1;
/// The node named in the render-mask table (0x33E388) never bound a target through this hook --
/// zero [hud] surface named by DrawHUD lines in a whole session -- so that rva is the function
/// that TESTS the HUD capability, not the node body that binds. Rather than try another guess,
/// record which node binds each HUD-shaped target and read the answer off the log.
/// answered; the two
#[no_mangle]
pub static mut CyberpunkVR_HudNodeProbe: i32 = 1;

// ---- src/Stereo/LumaProbe.cpp

/// Write 1 to arm; the file lands in the log folder and the knob resets itself.
#[no_mangle]
pub static mut CyberpunkVR_VisionDump: i32 = 1;

// ---- src/Stereo/NativeBoundsTrace.cpp

#[no_mangle]
pub static CyberpunkVR_BoundsTraceCompatible: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_BoundsTraceCount: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_BoundsTraceLost: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_ParticleTraceCompatible: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_ParticleTraceCount: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_ParticleTraceLost: AtomicU32 = AtomicU32::new(0);

// ---- src/Stereo/NativeDefinitionTracking.cpp

#[no_mangle]
pub static CyberpunkVR_SceneChildEnable: AtomicU32 = AtomicU32::new(0);

// ---- src/Stereo/Profiler.cpp

#[no_mangle]
pub static mut CyberpunkVR_ProfFrameMs: f64 = 0.0;
/// master toggle (OFF: profiler dormant, no per-node QPC/atomic cost)
#[no_mangle]
pub static mut CyberpunkVR_ProfEnable: i32 = 0;
/// NODE TIMELINE (diagnostic, off by default). The per-node totals above say how much CPU each view
/// costs, not WHEN it runs -- and in a CPU-bound frame the question is the critical path: whether the
/// two views' graphs overlap or follow each other. Every top-level render-graph node, every frame-graph
/// build and every Present lands here as one span; tools/hitch_tests/node_timeline.py reads the ring
/// live. kind: 0 MAIN node, 1 VRCAM node, 2 other view's node, 3 Present, 4 MAIN build, 5 VRCAM build.
#[no_mangle]
pub static mut CyberpunkVR_NodeTimeline: i32 = 0;
/// OFF -- forcing the gate CRASHES, and that is the useful result.
///
/// With the refusal overridden the node ran on and immediately took an access violation reading
/// 0xF0 off a null pointer (report 20260728-105606; our "capability refusal overridden" line is
/// the last thing in the log before it). So the capability test is not an arbitrary veto: it is
/// the engine declining to run a node whose prerequisites this view does not have. Skipping the
/// question does not create the answer -- behind it there is no 2D/HUD state on an RTT view, and
/// past the first missing pointer there would only be the next one.
///
/// The way in is therefore to SATISFY the condition, not remove it: find which capability bits
/// the node requires, find which of them the second view lacks, and set them where the view is
/// built -- early enough that the engine itself allocates everything that follows. The mask dump
/// below is the measurement that makes that possible.
#[no_mangle]
pub static mut CyberpunkVR_HudInVrcam: i32 = 0;

// ---- src/Stereo/SyncStereo.cpp

/// Storage for the switch declared in logger.h. On by default: when stereo fails to come up,
/// the install sequence is the first thing worth reading.
#[no_mangle]
pub static mut CyberpunkVR_StereoLog: i32 = 1;
/// Mirror/stereo chain diagnostics, always on (the comparisons they count already happen):
/// NodeHits    - node dispatches for a view matching our key   (0 => wrong key / not enabled)
/// CopyNodeHits- that view's RenderFinal2D node                (0 => VRCAM has no final blit)
/// RtvHits     - its render target captured                    (0 => OMSetRenderTargets missed)
#[no_mangle]
pub static mut CyberpunkVR_DebugVrcamNodeHits: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMirrorRtvHits: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_StableFromTonemap: i32 = 0;
/// last natural fin idx
#[no_mangle]
pub static mut CyberpunkVR_DebugFinNatural: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_StableCopy: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_DebugStableCopies: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugStableSkips: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_StereoEyeMaxAgeMs: u32 = 250;
#[no_mangle]
pub static mut CyberpunkVR_DebugVrcamEyeAgeMs: u32 = 4294967295;
/// Luma oscilloscope: turn the bright/normal alternation into NUMBERS. Each vrcam frame
/// the deferred hop also copies an 8x8 center block of the stable snapshot into a
/// readback slot (ringed with the existing copy fence); when a slot is reused its
/// completed block is decoded (R11G11B10F) and average luma is accumulated (a) by frame
/// parity and (b) as mean |dL| between CONSECUTIVE frames (parity-proof oscillation
/// amplitude). One log line per ~120 vrcam frames -> fixes get A/B'd by numbers.
/// diagnostics OFF (flicker RE done) -> no readbacks / [luma][cbflap][chain]... log spam
#[no_mangle]
pub static mut CyberpunkVR_LumaProbe: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_DebugCbCaptures: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugFrameCb: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugHudCb: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugRttComp: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugVrcamBaseFov: f32 = 0.0;
/// observed main AA mode  *(view+0xF94)
#[no_mangle]
pub static mut CyberpunkVR_DebugMainAaMode: u32 = 0;
/// vrcam AA mode (pre-fix) *(view+0xF94)
#[no_mangle]
pub static mut CyberpunkVR_DebugVrcamAaMode: u32 = 0;
/// Nsight cannot capture this setup, so the mod has to answer the question a capture would:
/// what does each view actually upload into the light path? Summing the bytes was too blunt --
/// it mixed every buffer the two nodes touch and produced a meaningless 0.86 ratio. A HISTOGRAM
/// of upload sizes separates them: the light array is one distinctive size, and if VRCAM's copy
/// of that size is smaller, or missing, that is the answer.
#[no_mangle]
pub static mut CyberpunkVR_LightCensus: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_DebugLightBytesMain: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugLightBytesVrcam: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugLightUploadsMain: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugLightUploadsVrcam: u64 = 0;

// ---- src/Stereo/ViewConstants.cpp

/// OFF -- the 256-byte b6 blocks turned out to be ring garbage past register 0.
#[no_mangle]
pub static mut CyberpunkVR_CbvProbe: i32 = 1;
/// OFF: a mutex and an 848-byte memcpy on every constant-buffer view over 768 bytes.
#[no_mangle]
pub static mut CyberpunkVR_CamCbProbe: i32 = 1;

// ---- src/Stereo/ViewReuse.cpp

/// DEFAULT 1 ON 2026-08-29, ON THE PICTURE. Reported the first time it was switched on:
/// the scanner's green tint is back in the second eye. That isolates a finding this repository had
/// only ever recorded collectively -- the tint was first attributed to lending root parameter 5 of the
/// LUT build, then to "comp_lend plus viewdata_fix_mask plus run_view_params" together. It is this
/// alone: comp_lend is off, LutTableLend does not exist in this build, and the tint is there.
///
/// Which matters beyond the tint, because comp_lend is the mechanism that crashed twice inside
/// DrawComposition today. It is not needed for this, and nothing else has asked for it since.
///
/// Measured alongside: runs=2626 against MAIN's params seen 3327 over the same window, both detours
/// registered, no crash dump in forty minutes of play.
/// DEFAULT 2, NOT 1. Mode 1 is the engine's whole pass and it erases entries from a list MAIN shares;
/// the 0x88 driver fault returned 229 seconds into the one session it ran in. Mode 2 delivers the same
/// thing -- the scanner's tint was measured with mode 1 and mode 2 applies the identical per-entry
/// function -- with the erase gone. Shipping the mode with a named hazard as the default would be
/// choosing the worse of two that do the same job.
#[no_mangle]
pub static mut CyberpunkVR_RunViewParams: u32 = 2;
/// Historical A/B on 2026-07-29. The per-node dispatch census leaves exactly one lighting-
/// relevant node that never dispatches for VRCAM, and it is this one, skipped by us. It fits
/// the symptom better than anything else measured: some lights work for VRCAM (near stalls)
/// and some never do (street lamps, the road) -- which is what a missing shadow slice looks
/// like, since a shadow-casting light with no slice is dropped rather than drawn unshadowed.
/// Tested before only together with the other two reuse modes, where VRCAM re-rendering every
/// local shadow (30x MAIN in the audit) could break it a second way.
/// Measured 2026-07-29: with this at 0 the node does dispatch for VRCAM (AD5770 leaves the
/// census) and the lamps are still unlit. Not the cause; put back for the 30x it costs.
/// AND TESTED AT 0 AGAIN 2026-08-18 AGAINST A SYMPTOM IT HAD NEVER BEEN TRIED ON, WITH THE SAME ANSWER:
/// interior lighting differs between the eyes -- the sofa cushions and the ceiling, "shiny in VRCAM,
/// normal in MAIN". Outdoors the sun dominates and this is invisible; indoors the local lights ARE the
/// lighting, and this is the one lighting-relevant node the second view does not run, so it was the first
/// thing to try. At 0 the artefacts are UNCHANGED (same scene). Local shadow reuse is not the cause
/// of the interior mismatch either, and this knob is back at 1 for the 10.6x it costs.
///
/// That also kills the analogy I reached for. A sun cascade is fitted to the VIEWER, which is what gave
/// the two views something to disagree about; local shadow maps are per-light and world-space with a
/// verified-identical light->slice mapping. This rules out index mismatch, but the later moving-light
/// test above disproves universal reuse safety. The next suspect for that interior symptom was reflection probes and
/// screen-space reflections -- not the shadow atlases.
/// (previous note kept below)
/// AT 0 2026-08-18 for the interior symptom: interior lighting differs
/// between the eyes -- the sofa cushions and the ceiling, "shiny in VRCAM, normal in MAIN". Outdoors the
/// sun dominates and this is invisible; indoors the local lights ARE the lighting, and this is the one
/// lighting-relevant node the second view does not run.
///
/// Read the result with the note above in mind. At 0 the second view re-renders every local shadow into
/// the shared atlas, which is measured at 10.6x MAIN's cycles and thrashes MAIN's temporal cache, so
/// shadow flicker and a frame-time cost at 0 are EXPECTED and are not new bugs. What the test answers is
/// only whether the interior specular mismatch follows this knob.
///
/// And one analogy is withdrawn before it misleads: this is NOT the sun-cascade case. A cascade is fitted
/// to the viewer, which is what gave the two views something to disagree about. Local shadow maps are
/// per-light and world-space, and the light->slice mapping was verified live identical. That only proves
/// index compatibility. If the interior mismatch does follow this knob, the mechanism is something else
/// and has to be found rather than assumed.
/// 0=native updates, 1=experimental VRCAM skip
#[no_mangle]
pub static mut CyberpunkVR_LocalShadowReuseMode: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_SkyReuseMode: u32 = 1;
/// DEFAULT 0 since 2026-07-28. A night capture (Objects/EventList_LATESTVR) shows MAIN running
/// a lighting stage VRCAM does not: PSO 1030 (5424 + 4166 groups), 671 (1943 + 1636), 1316
/// (16^3) and 1336 x16 over 3D grids, writing the shared GI cache Resource_122 (1024x1024x3
/// R11G11B10), the 64^3 clipmap volumes Resource_3164/3166 and a 112 MB probe buffer. That is
/// this hook skipping the GI build for VRCAM, and the audit agrees: GlobalIllumination is
/// MAIN 0.1022 ms vs VRCAM 0.0289 ms. The reuse was measured as harmless earlier -- but in
/// DAYLIGHT, where the stage is inert; at night it is what lights the street lamps.
/// BACK TO 1 (2026-07-31). The 0 above was set on 2026-07-28 for the night street lamps, and
/// that theory is recorded as DISPROVEN in the same hunt: all three reuse knobs off changed
/// nothing for the lamps, which were finally fixed by RenderMask/DistantLights instead. What
/// the 0 did keep doing is let VRCAM rebuild the SHARED GI every frame from its own frustum --
/// cache Resource_122, the 64^3 clipmaps 3164/3166, the 112 MB probe buffer -- and the note at
/// GI_FEATURE_BIT says exactly what that costs: "main GI (ambient light/shadows) flicker".
/// It is also the ONLY reuse knob that differs from the known-good testbed snapshot of 25 Jul.
/// AT 0 FOR AN INTERIOR TEST 2026-08-18, and the standing warning above is exactly why this is a test and
/// not a change. The pipeline sweep says the two views do not merely run this node a different number of
/// times, they run DIFFERENT compute shaders inside it: VRCAM 3 actions (cs 1911, 2574, 2058) against
/// MAIN 28 (cs 2058, 2058, 1174). So reuse here is not "the same apply over reused data", it is another
/// branch -- and indoors the indirect light IS the lighting, which is the reported symptom: ceiling too
/// shiny, glasses lit cold instead of warm like the room, blown highlights, haze wrong. The flicker
/// verdict that pinned this knob at 1 was measured OUTDOORS; indoors it was never tried.
///
/// EXPECTED COST AT 0, from the record above: position-dependent shadow flicker, two shadow sets
/// alternating in one spot and clean in others, because the second view rebuilds a shared structure from
/// its own frustum. If that appears it is the price of the test, not a new defect. Back to 1 either way
/// unless the interior mismatch follows this knob.
/// AND THE INTERIOR A/B CAME BACK NEGATIVE, TWICE OVER (2026-08-18). At 0 the ceiling and the rest of
/// the interior artefacts were UNCHANGED, so GI reuse is not the cause; and the one thing that did change
/// got WORSE -- the glasses went from bluish to outright white, i.e. the second view building its own GI is
/// further from MAIN than reusing MAIN's. The predicted price also arrived on cue: shadows started
/// flickering, which is the position-dependent flicker this file already records against this knob and is
/// what confirms the knob really took effect rather than sitting inert.
///
/// So the different compute shaders in this node (VRCAM cs 1911/2574/2058 against MAIN cs 2058/2058/1174)
/// are a real pipeline difference and still NOT the interior defect. Back at 1, and do not spend another
/// round here.
/// 0=vrcam builds its own GI, 1=reuse main's (A/B)
#[no_mangle]
pub static mut CyberpunkVR_GiReuseMode: u32 = 1;
/// ---- THE COMPOSITION STATE, LENT FOR ONE NODE CALL AND NO MORE --------------------------------
///
/// viewData+0x168 is the gate on four nodes the engine dispatches for MAIN and never for this view:
/// StartRender, DeclareCommonResourceAllocs_FinalOnly, CompositionPostProcess and DrawComposition.
/// Writing the field opens ALL of them, and that is what the previous attempt did -- installed on
/// every dispatch of our view and left in place. It ran the stage, and it crashed twice in one
/// afternoon, both times inside DrawComposition and never inside CompositionPostProcess:
///
/// exe+0x2AC150   *(a1 + 0x70) with a1 == 0 -- the null is *(record + 0x28) of a type-3
/// composition element, loaded at exe+0x1ECE2A and passed down three frames untested
/// exe+0xA75052   mov eax,[rcx+0x18] with rcx = 0x34E0304032662E56 -- garbage read out of a
/// 56-byte-stride array of composition elements
///
/// Both with the view identified in the same frame by the camera-name hash at view+0x28. The reason
/// is not the gate: DrawComposition walks a LIST of composition elements assembled for another view
/// and runs off its end. Giving that node the stage as well needs the second view to own a
/// composition set, which is a separate piece of engineering and not a knob.
///
/// So the lend is scoped two ways instead of none. By TIME: installed before the original call and
/// removed on return, so nothing outside the wrapped dispatch ever sees the field set. By NODE: one
/// bit per node, because which nodes actually need it is NOT settled --
///
/// bit 0   CompositionPostProcess (0x3E1B20)          the target, try this alone first
/// bit 1   DeclareCommonResourceAllocs_FinalOnly      only if bit 0 alone is not enough
///
/// DrawComposition is not offered a bit at all. It is the one that crashes, and no amount of scoping
/// changes that its element list belongs to another view.
///
/// Why bit 1 is a bit and not a given. The record of the first lend attempt says two different things
/// about the same crash -- an access violation reading 0x68 of a null object -- and only one of them
/// can be true. It says composition against undeclared resources caused it, which is what would make
/// the declaration pass necessary; and it also says those four crashes correlated with the feature's
/// key EVEN IN RUNS WHERE IT CAPTURED AND INSTALLED NOTHING, i.e. that touching MAIN's composition
/// dispatch was what broke, which would make the declaration pass irrelevant. The second reading is
/// the better evidence -- a crash in runs that changed nothing cannot be caused by what the change
/// would have done -- so the declaration pass is a hypothesis here, not a requirement, and it gets a
/// bit to be tested with rather than a place in the default.
///
/// The source is the half of the previous attempt that was right, and every clause of it is a
/// measurement. The pair is read FRESH from the set on every use, because the set object is stable
/// while the pair inside it is swapped per frame -- {1B50C817D90,1B50411F9C0} then
/// {1B508752630,1B528DB7240} in consecutive frames. The engine's own assign sub_140545714 is never
/// called: it does InterlockedExchange(set+8, 0), a one-shot per frame, and a second caller pushes
/// MAIN onto the other branch on alternate frames -- that was MAIN's judder and the flickering HUD.
/// viewData+0xF88 is never written: CRenderNode_Present reads the same field and would enlist this
/// view in presenting to the desktop window. And the refcount is written as 0 on purpose, because
/// the engine's release reads `if (!refcount || InterlockedExchangeAdd(refcount,-1) != 1) return 0;`
/// -- so every release of our copy is a no-op, and MAIN's object can neither be freed nor have its
/// count moved.
///
/// WHICH NODES, MEASURED -- and a wrong conclusion of mine withdrawn along the way.
///
/// The first scoped attempt gave CompositionPostProcess the field and nothing else, and the node
/// died in the frame graph's resource lookup, sub_1406E8884, with the resolved index ZERO:
///
/// mov   eax, [rdx]          ; the resolved resource index for the key
/// dec   eax                 ; 0 - 1 == 0xFFFFFFFF, and rax in the dump is exactly that
/// imul  r9, rax, 0B0h       ; r9 = table base + 0xFFFFFFFF*176
/// movzx r8d, [r9+2F22Ch]    ; fault, and 0x29D1519F17C minus r9 is exactly 0x2F22C
///
/// I read that as proof that the whole route is impossible -- that frame-graph resources are fixed
/// when the graph is built, so opening the gate at dispatch time can never work. That is WRONG, and
/// the port's own record refutes it: the unscoped version of this lend made the stage run, with
/// counters, `CompositionPostProcess M=69896 V=2536`, `192x192 M=17474 V=634` -- all zero for the
/// whole session before it. If the resources were fixed at build time, no version could have run.
/// The only difference between the two is HOW MANY NODES see the field, so the resource is created
/// during the frame by one of the other readers, and the scoped version simply starved it.
///
/// So the readers were measured instead of assumed: of the 68 functions that call the viewData
/// accessor sub_14033C248, exactly five touch +0x168.
///
/// 0x3E1B20  CompositionPostProcess    bit 0   the target
/// 0xCEA010  render-target declarations bit 1  names RT_viewDepthTexture[%d] and
/// RT_localShadowTexture[%d] -- the likely creator
/// of what the composition node then looks up
/// 0x33E388  DrawHUD                   bit 2
/// 0x773580  unnamed, vtable-only      bit 3
/// 0x33D0AC  DrawComposition           NO BIT -- this is the one that crashes, twice measured,
/// because it walks an element list built for another view
///
/// And DeclareCommonResourceAllocs_FinalOnly (0x3E0070) is NOT a reader at all, so the bit I gave
/// it in the first attempt could never have done anything. That is what came of taking a note as a
/// measurement.
///
/// Start at 3 -- the composition node and the declarations that feed it. Ships off.
#[no_mangle]
pub static mut CyberpunkVR_CompLendScoped: u32 = 0;

// ---- src/Stereo/VisionLayer.cpp

/// The tally that FOUND that node. Report-only.
#[no_mangle]
pub static mut CyberpunkVR_VisionMap: i32 = 0;
/// Blend the snapshotted layer into the second eye (openxr_capture does the pass).
#[no_mangle]
pub static mut CyberpunkVR_VisionToSecondEye: i32 = 1;
/// 3 = straight alpha (PipelineState_1216, the default); 0 premultiplied, 1 opaque replace, 2 additive.
#[no_mangle]
pub static mut CyberpunkVR_VisionDebug: i32 = 3;
#[no_mangle]
pub static mut CyberpunkVR_VisionFit: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_VisionOffX: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_VisionOffY: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_DebugVisionOverlays: u64 = 0;
/// OFF: [rtmap] node -> render-target map
#[no_mangle]
pub static mut CyberpunkVR_RtMapProbe: i32 = 1;

// ---- src/Utils/DebugGate.cpp

#[no_mangle]
pub static mut CyberpunkVR_RuntimeDiagnostics: i32 = 0;
