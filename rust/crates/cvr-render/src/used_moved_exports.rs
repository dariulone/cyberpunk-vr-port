//! Exported variables the C++ half still uses, defined in Rust (their definitions moved from the sources named in each
//! section; each source keeps an extern declaration in its Rust branch, so its own uses and the other sources' compile
//! unchanged). Generated from the definition lines; tools' scratch check_moved compares every variable's bytes with
//! the C++ definitions'.
#![allow(non_upper_case_globals)]
use core::sync::atomic::{AtomicU32, AtomicU64};

// ---- src/Stereo/NativeDefinitionTracking.cpp

#[no_mangle]
pub static CyberpunkVR_PreparationDefinitionMode: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparationDefinitionRequest: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparationDefinitionCompatible: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparationDefinitionSeq: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_PreparationDefinitionStats: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_PreparationDefinitionTicket: [u64; 8] = [0; 8];
#[no_mangle]
pub static CyberpunkVR_PreparationNativeConsumers: [AtomicU64; 10] = [const { AtomicU64::new(0) }; 10];
#[no_mangle]
pub static mut CyberpunkVR_PreparationDefinitionAggregate: [u64; 8] = [0; 8];
#[no_mangle]
pub static mut CyberpunkVR_PreparationNativeConsumerReject: [u64; 8] = [0; 8];
#[no_mangle]
pub static mut CyberpunkVR_PreparationDefinitionOwners: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_PreparationDefinitionMappings: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_PreparationDefinitionEntries: u64 = 0;
#[no_mangle]
pub static CyberpunkVR_PreparationDefinitionMutationCounters: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];
#[no_mangle]
pub static CyberpunkVR_PreparationGenerationStats: [AtomicU64; 16] = [const { AtomicU64::new(0) }; 16];
#[no_mangle]
pub static CyberpunkVR_PreparationGenerationSeq: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparationGenerationCount: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparationGenerationCoverageLost: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_PreparationGenerationTicket: [u64; 4] = [0; 4];
#[no_mangle]
pub static mut CyberpunkVR_PreparationGenerationReject: [u64; 8] = [0; 8];
#[no_mangle]
pub static CyberpunkVR_PreparationGenerationRejectPhase: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_PreparationGenerationRows: [[u64; 12]; 128] = [[0; 12]; 128];
#[no_mangle]
pub static mut CyberpunkVR_PreparationDefinitionTreeStats: [u64; 12] = [0; 12];
#[no_mangle]
pub static mut CyberpunkVR_PreparationDefinitionTreeRoots: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_PreparationDefinitionTreeNodes: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_PreparationDefinitionTreeHandles: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_PreparationDefinitionTreeArrays: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_PreparationDefinitionTreeAllocators: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_PreparationDefinitionTreeStrings: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_PreparationDefinitionDeclaration: [u64; 8] = [0; 8];
#[no_mangle]
pub static CyberpunkVR_OwnedExecutionMode: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_OwnedExecutionRequest: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_OwnedExecutionClosing: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_OwnedExecutionSeq: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_OwnedExecutionStats: [u64; 24] = [0; 24];
#[no_mangle]
pub static mut CyberpunkVR_OwnedExecutionResources: [u64; 12] = [0; 12];
#[no_mangle]
pub static mut CyberpunkVR_OwnedExecutionCompiler: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_OwnedExecutionSchedule: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_OwnedExecutionScheduleNodes: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_OwnedExecutionScheduleEdges: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_OwnedExecutionScheduleBindings: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_OwnedStageStats: [u64; 8] = [0; 8];
#[no_mangle]
pub static mut CyberpunkVR_OwnedStageDefinitions: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_OwnedStageEntries: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_OwnedSceneStats: [u64; 8] = [0; 8];
#[no_mangle]
pub static mut CyberpunkVR_OwnedSceneRoots: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_OwnedSceneChildren: u64 = 0;
#[no_mangle]
pub static CyberpunkVR_OwnedSceneSplitEnable: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_OwnedSceneSplitStats: [u64; 24] = [0; 24];
#[no_mangle]
pub static mut CyberpunkVR_OwnedSceneSplitSections: [u64; 12] = [0; 12];
#[no_mangle]
pub static mut CyberpunkVR_OwnedSceneSplitSourceNodes: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_OwnedSceneSplitSourceEdges: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_OwnedSceneSplitMappingRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_OwnedExecutionRetirement: [u64; 8] = [0; 8];
#[no_mangle]
pub static mut CyberpunkVR_OwnedExecutionRetirementFailure: [u64; 8] = [0; 8];
#[no_mangle]
pub static mut CyberpunkVR_OwnedExecutionAllocator: [u64; 24] = [0; 24];
#[no_mangle]
pub static mut CyberpunkVR_OwnedExecutionRetirementTotals: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_OwnedExecutionRetirementEventSeq: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_OwnedExecutionRetirementEventCount: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_OwnedExecutionRetirementEvents: [[u64; 24]; 32] = [[0; 24]; 32];
#[no_mangle]
pub static mut CyberpunkVR_OwnedExecutionCache: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_OwnedExecutionCacheRows: [[u64; 8]; 64] = [[0; 8]; 64];
#[no_mangle]
pub static mut CyberpunkVR_OwnedExecutionAdmission: [u64; 6] = [0; 6];
#[no_mangle]
pub static CyberpunkVR_OwnedResourceReplayEnable: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_OwnedResourceReplayStats: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_OwnedResourceReplayRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_OwnedResourceReplayProof: [u64; 8] = [0; 8];
#[no_mangle]
pub static mut CyberpunkVR_OwnedResourceReplayRegistration: [u64; 8] = [0; 8];
#[no_mangle]
pub static mut CyberpunkVR_OwnedResourceReplayReset: [u64; 8] = [0; 8];
#[no_mangle]
pub static mut CyberpunkVR_OwnedResourceReplayAsync: [u64; 8] = [0; 8];
#[no_mangle]
pub static mut CyberpunkVR_OwnedResourceReplayMismatch: [u64; 16] = [0; 16];

// ---- src/Runtimes/OpenXRFrameLoop.cpp

/// The hand filter's speed, in UEVR's units: the follow fraction per second, multiplied by delta time
/// at the point of use. 15.0 is their default (slider 0.01..30), which at 52 fps is 15 * 0.019 = 0.29 --
/// about 29% of the remaining error per frame, a ~66 ms time constant. 0 disables it.
///
/// The old knob was xr_hand_smooth, a fraction per FRAME, so its time constant moved with the frame
/// rate; 0.45 was 10% per frame, about 190 ms. It is gone, along with the filter it drove.
#[no_mangle]
pub static mut CyberpunkVR_HandLerpSpeed: f32 = 25.0;
/// Milliseconds. Passes inside one batch arrive microseconds apart and batches a frame apart, so
/// anything in between separates them. Watch the gap census before moving it.
#[no_mangle]
pub static mut CyberpunkVR_VrikBatchGapMs: f32 = 3.0;
#[no_mangle]
pub static mut CyberpunkVR_HandRelToFilteredHead: i32 = 1;
/// THE LEFT STICK'S CLICKS, counted: one for each press-and-release that reached the game as L3 (the deferred press
/// below). The press itself is one frame of this loop, which an input poll running slower than it can miss; the
/// sprint-on-click mode (xr_sprint_mode=1, Hooks/XInput.cpp) reads the count instead.
#[no_mangle]
pub static mut CyberpunkVR_LeftStickClicks: i32 = 0;
/// 1 = the hand pose published for the solve is LOCATED ONCE PER FRAME, for that frame's own instant,
/// instead of copying whatever the 72 Hz XR cycle last produced. 0 = the old copy.
///
/// THIS IS AN ALIASING FIX, not a filter. The publish was already once per frame; the VALUE inside it came
/// off a faster stream, so the phase between "when the pose was measured" and "which frame shows it"
/// walked around. For a hand at CONSTANT speed that phase walk is |v * (dt_n - dt_n-1)| of position error
/// -- equal steps arriving unequal, which is what reads as ghosting or a trail rather than motion.
///
/// Measured before the fix: the same head-relative position scored 3-6 mm of second difference on the XR
/// thread's uniform clock and 6-13 mm as the solve read it -- a consistent factor of two, with nothing in
/// between but the resampling.
///
/// xrLocateSpace is a query and not a wait, and this file already calls it off the XR thread for the
/// camera; the spec places no external synchronisation on XrSpace. One call per hand per frame.
#[no_mangle]
pub static mut CyberpunkVR_HandLocatePerFrame: i32 = 1;
/// Hands actually located by the per-frame path. Its RATE against present/s is the only way to
/// tell "the fix changed nothing" from "the fix never ran" -- and those need different work.
#[no_mangle]
pub static mut CyberpunkVR_DebugHandPerFrameLocates: u64 = 0;
/// Definition for the declaration in openxr_manager.h -- see UseThreadedSubmit there.
///
/// 1. IT IS ON, AND THE QUESTION IS NOW MEASURABLE INSTEAD OF FELT.
///
/// Two measurements, and the newer one retires half of the older:
///
/// THEN (the reason it was 0): 16311 XR cycles against ~18469 presents -- the thread ran SLOWER than
/// the game, never reached display rate, and cost phase. That is no longer true.
///
/// NOW: presents 52/s | xr cycles 72/s | wait 72 begin 72 end 72 (WITH A PICTURE 72, EMPTY 0) |
/// perDisplay 1.00 (inline: 0.71) | [xrcadence] 100% of frames occupy exactly ONE display period |
/// HELD 0, LATE 1-2 per 72 | POSEDIAG render->live gap avg 0.00 deg (inline: 0.01).
///
/// So it reaches display rate, and the pose bookkeeping is right: the layer carries the pose of the
/// CAPTURED frame, not a fresh locate, and the mono pool's three rotating slots keep content and pose
/// together across a resubmit. Checked specifically -- there is no content race.
///
/// AND IT STILL LOOKED WORSE IN THE HEADSET, which no counter above explains, because every one of them
/// counts FRAMES. The quantity none of them expresses is how far the runtime has to warp each submitted
/// image, and whether that distance is steady. That is what [xrwarp] now reports, and it is the number
/// to read next -- see the age counters above it.
///
/// WHAT THE BUCKETS WILL SAY, and what to conclude:
///
/// * one bucket near 100% -> the warp distance is uniform, and whatever is being seen is not this
/// * split across 0 and 1 -> the same capture goes out again a display period later, which is
/// arithmetic (72/52 = 1.385) and not a defect. The spread in ms is then the thing to judge: if
/// mean and max differ by about one display period, that IS the alternation being felt.
/// * anything in 2+ -> the game produced nothing for two display cycles, which is a real stall and
/// worth chasing separately.
///
/// A frame-rate cap would make the ratio integral and the spread vanish, and that has been declined --
/// the game runs at its own rate. So if the spread is the cause, the fix has to come from making the
/// warp uniform at a non-integer ratio, not from pacing the game.
/// WHO OWNS THE XR FRAME LOOP -- and it is a THREE-state, not a flag:
///
/// -1  auto (default): the dedicated submit thread on SteamVR, the INLINE pump on Virtual Desktop
/// 0  force the inline pump, on the game's own Present thread
/// 1  force the dedicated submit thread
///
/// Settable live as xr_threaded_submit in vrport.ini.
///
/// Auto exists because the right answer is per-runtime and was already written down in
/// OpenXRManager.hpp: SteamVR's xrWaitFrame/compositor pacing stalls the Present thread when the
/// loop runs inline, which freezes the game, so SteamVR needs the thread. Virtual Desktop does not
/// -- the inline path is the one that was proven on it -- and the knob had been left at a plain 1,
/// which put VD on the thread as well. Anything that is neither SteamVR nor VD keeps the thread,
/// which is where it is today: this changes VD, and only by default.
#[no_mangle]
pub static mut CyberpunkVR_ThreadedMonoSubmit: i32 = -1;
/// ---- THE FRAME LOOP'S CONTRACT, COUNTED WHERE THE CALLS ARE MADE ----------------------------
///
/// Ported from the vr2 tree, where a half-rate submit was found by printing these three numbers
/// next to each other rather than reasoning about the loop. wait == begin == end is what the
/// specification asks of a frame loop, and two different faults hide behind "it looks like half
/// the frames":
///
/// * fewer ENDS than BEGINS -- display frames the loop opened and never closed, and
/// * ends that are EMPTY (layerCount 0) -- closed with nothing for the runtime to show, so it
/// holds the previous composition.
///
/// Neither is visible from outside the process, and `XR pacing:` above cannot tell them apart
/// because it only knows cycles and successful mono submits. These are monotonic totals, exported
/// so they can be read live in x64dbg without a rebuild; the per-second rates are DIFFERENCED
/// from them in ReportXrFrameRates(), never derived from a modulo on a counter.
///
/// RELAXED ATOMICS, AND THE REASON IS A BUG THESE COUNTERS THEMSELVES CAUSED. They were plain
/// `unsigned long long` with `++`, and FrameThreadMain runs on EITHER of two threads -- the Present
/// thread inline, or the dedicated submit thread -- with ownership handed over through
/// AcquireFrameLoop. A non-atomic 64-bit increment from the second thread can operate on a value the
/// first thread has not published, so an increment is simply lost.
///
/// That is exactly what it looked like: once per session, mid-session, [xrloop] flipped from PAIRED to
/// "wait 10138 | begin 10137 | end 10138" -- one begin missing out of ten thousand, at a loop-owner
/// handover. Read as a frame-loop defect it says the loop skipped a begin, which would be an XR spec
/// violation worth hunting. Read correctly it says the COUNTER dropped one.
///
/// The distinction matters because the PAIRED check is the instrument used to judge whether the
/// threaded submit path is sound at all. An instrument that loses counts cannot answer that. On x86 a
/// relaxed fetch_add is a lock xadd; at 72 Hz across eight counters that is nothing, and the exported
/// layout is unchanged (8 bytes, lock-free) so x64dbg and the overlay still read them as before.
/// ---- HOW OLD IS WHAT WE SUBMIT ---------------------------------------------------------------
///
/// The age, in milliseconds, of the captured frame at the moment it is handed to xrEndFrame. This is
/// the warp distance the runtime has to cover, and it is the one quantity none of the counters below
/// can express: they all count frames, and the frame counts can be perfect while this varies.
///
/// Inline submits each capture once, immediately, so the age is a small constant. Threaded submits at
/// display rate, so a capture whose game frame has not been replaced yet goes out AGAIN one display
/// period later -- 72/52 = 1.385, so roughly every third submit is a repeat, at an age one period
/// higher. If that alternation is what reads as the world shifting, this is where it shows as a number:
/// a bimodal spread rather than a single value.
///
/// Doubles behind a mutex-free relaxed protocol would tear on a 32-bit target; this is x64 only and the
/// stores are 8 bytes, so the same relaxed-atomic rule as the counters applies.
/// The GAME's frame interval in whole display periods, and its worst case. Written from the Present
/// hook, read here, for the reason spelled out at the write site: an average cannot see one long frame.
#[no_mangle]
pub static CyberpunkVR_DebugPresentGapBuckets: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];
#[no_mangle]
pub static CyberpunkVR_DebugPresentGapUsMax: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_DebugVrcamEyePaired: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_DebugVrcamEyeUnpaired: AtomicU64 = AtomicU64::new(0);
/// How often the second eye was handed its OWN most recent image because no slot carried this frame's
/// serial. The alternative, and what used to happen, is MAIN's image in that eye for a cycle: a viewpoint
/// one IPD away, in one eye, which is a geometric jump the other eye cannot produce.
#[no_mangle]
pub static CyberpunkVR_DebugVrcamEyeReused: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_DebugSubmitAgeCount: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_DebugSubmitAgeSumUs: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_DebugSubmitAgeMinUs: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_DebugSubmitAgeMaxUs: AtomicU64 = AtomicU64::new(0);
/// Buckets by whole display periods of age: 0 = submitted in the cycle it was captured in, 1 = one
/// period stale (a resubmit), 2+ = the game did not produce anything for two cycles.
#[no_mangle]
pub static CyberpunkVR_DebugSubmitAgeBuckets: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];
#[no_mangle]
pub static CyberpunkVR_DebugXrWaits: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_DebugXrWaitFailed: AtomicU64 = AtomicU64::new(0);
/// WAITS THAT CAME BACK, as against waits that were STARTED.
///
/// The distinction is not pedantry, it is what made the PAIRED check accuse a healthy loop. Waits is
/// incremented BEFORE xrWaitFrame, which blocks for 12.4 ms of every 13.9 ms period, and [xrrate] is
/// printed from the PRESENT thread while the submit thread is mid-cycle. So a snapshot finds waits one
/// ahead of begins about ninety percent of the time -- not because a begin was skipped, but because one
/// is about to happen.
///
/// Inline mode never showed it: there the loop and the reporter are the same thread, so a sample can
/// never land inside the wait. Turning the threaded path on made a correct loop look broken.
///
/// waits-started minus waits-returned is therefore a real quantity: normally 0 or 1 (one in flight), and
/// a value that stays above 1 means a wait is not coming back, which IS a hang worth seeing.
#[no_mangle]
pub static CyberpunkVR_DebugXrWaitsReturned: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_DebugXrBegins: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_DebugXrBeginDiscarded: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_DebugXrEnds: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_DebugXrEndsWithLayer: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_DebugXrEndsEmpty: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_DebugXrEndFailed: AtomicU64 = AtomicU64::new(0);
/// Always counted (rare events, one relaxed add each): display cycles whose allocator was still busy,
/// cycles that sent the last frame again, and ends that went out with no layer at all.
#[no_mangle]
pub static CyberpunkVR_XrAllocatorBusy: AtomicU64 = AtomicU64::new(0);
/// Display cycles that resent the captured frame already in the eye swapchains instead of copying it again
/// (LastCopy, OpenXRManager.hpp); CyberpunkVR_XrReuseImages 0 turns that off (copy every cycle).
#[no_mangle]
pub static CyberpunkVR_XrReusedCycles: AtomicU64 = AtomicU64::new(0);
/// Reprojection rate (tools/vr_bench): display cycles with a projection layer, and those of them that showed the game
/// frame the previous cycle already showed (the runtime reprojects it), whatever CyberpunkVR_XrReuseImages is.
#[no_mangle]
pub static CyberpunkVR_XrProjectionCycles: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_XrRepeatCycles: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_XrResubmits: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_XrEmptyEnds: AtomicU64 = AtomicU64::new(0);
/// XR reports and deep timing are both controlled by the launcher DEBUG gate.
/// RuntimeDiagnostics gates the independent export-only counters and snapshots.
#[no_mangle]
pub static mut CyberpunkVR_XrRateLog: i32 = 1;
/// Diagnostics: the measured game-frame period in microseconds, and how often the fit was used
/// versus fell back. A slope that does not settle near the real frame time means the fit is being
/// fed garbage and mode 1 is the honest choice.
/// ---- HOW MANY PRESENTS AHEAD THE FRAME BEING BUILT WILL BE SHOWN ----------------------------
///
/// We assume the frame the game is building now appears at present (count + 1). REDengine has its
/// own render thread, so the frame presented at N may have been simulated with the camera written
/// two intervals back rather than one. If that is so, every image is systematically one present
/// behind the pose it is labelled with: the compositor over-warps, the next frame pulls it back,
/// and that is judder that survives a perfectly paired pose ring -- which is exactly what the
/// counters now show (SlotHit 11591, SlotReused 0, SlotMiss 0, and the judder still there).
///
/// This is one integer and it decides the answer, so it is a knob rather than a guess. It is
/// applied in BOTH places that must agree: the serial the slot is published under, and the serial
/// the camera's locate is aimed at. The submit side keeps looking up its own present serial.
#[no_mangle]
pub static mut CyberpunkVR_EnginePipelineDepth: i32 = 0;
/// How the submitted centre pose was obtained. Written = the exact sample the camera was built
/// from (correct). Slot = the frame loop's own locate (a different sample; only right when the
/// written one is missing).
#[no_mangle]
pub static mut CyberpunkVR_DebugPoseFromWrite: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPoseFromSlot: u64 = 0;
/// ---- real stereo: VRCAM as the right eye ---------------------------------------------
/// DEFAULT ON -- it has now run a session without incident.
///
/// Safe as a default because it is self-disabling: the accessor returns null unless the second
/// view produced a frame within the last few hundred ms, so menus, loads and a disabled VRCAM
/// component all fall back to the mono path byte-for-byte.
#[no_mangle]
pub static mut CyberpunkVR_StereoSubmit: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_DebugStereoEyeSubmits: u64 = 0;
/// 1 = MAIN goes to the RIGHT eye and VRCAM to the left. Swaps the IMAGES only; the submitted
/// per-eye poses stay as the runtime gave them, which is what keeps the stereo honest. The
/// camera separation sign and the barrel dot are flipped alongside it in their own files --
/// swapping any one of the three on its own inverts the depth.
#[no_mangle]
pub static mut CyberpunkVR_MainIsRightEye: i32 = 1;

// ---- src/Stereo/Grading.cpp

#[no_mangle]
pub static mut CyberpunkVR_OpticsMaterialProbe: i32 = 0;
/// ON, aimed at the SAMPLING side of the shadow cascades.
///
/// Where this stands: the two views were measured to fit the sun cascade 9-12 cm (casc0) and 27 cm (casc1)
/// apart, forcing MAIN onto the other view's fit brings that separation to exactly 0, and the sun shafts
/// improved -- but they are still displaced. So the lend reaches what RASTERISES the atlas and not what
/// SAMPLES it. The rasteriser's matrices come from the cascade record; the sampler's come from somewhere else,
/// and only two functions in the whole exe address that record by its offset (the pass itself and
/// sub_140E9BBC4, which is the bit-44 variant of the shadow-mask pass and is measured NOT to run -- bit 44
/// answers no for both views). So the sampler reads it through a register, and the way to find that is to
/// diff what the lighting node uploads rather than to keep reading disassembly.
///
/// BindLightingGlobalConstants (0xC45C70) is the named node that carries the lighting globals, which is where
/// cascade sampling matrices belong. Pointed there, the size census says what it uploads per view and the
/// field diff says which of it differs.
/// DEFAULT 0 AND LIVE, like every other measurement here: `xr_block_diff`, with
/// `xr_block_diff_node` (HEX work-RVA) and `xr_block_diff_size` aiming it. It shipped at 1.
#[no_mangle]
pub static mut CyberpunkVR_BlockDiff: i32 = 0;
/// 848 = 53 float4 = b1, which the census confirms the cascade node uploads for both views (4156 against
/// 4602 in one interval) and which the shader's height-mask grid origin [36].xyz lives in. The 448-byte
/// rows in the same census are b7, 28 float4, where that shader's dither threshold comes from.
#[no_mangle]
pub static mut CyberpunkVR_BlockDiffSize: u32 = 384;
/// Mode 5's content filter: is this 480-byte block the frame constants? The clock at [0].z runs into the
/// hundreds of thousands of seconds, so "greater than a thousand" separates it from every other block of
/// that size seen in the census -- one of which carried 1.0f there and render-target dimensions further
/// on, and must not be written.
/// ---- THE WIDE CENSUS: every large upload, both uploaders, and whether the views disagree ----------
///
/// Built because the narrow version has now cost five rounds. Each time the shape was the same: a probe on ONE
/// uploader, or keyed to ONE node picked by reasoning, and a silence that read like a negative result. The
/// latest was aiming at BindLightingGlobalConstants -- which turns out to be 84 bytes with a single callee,
/// i.e. a BIND that points the root signature at a buffer somebody else filled, so it uploads nothing and the
/// probe was always going to print nothing.
///
/// This asks the question the other way round and without a candidate: for every upload of 128 bytes or more,
/// on BOTH uploaders, keyed by (node, size), how often each view does it and whether their CONTENTS ever
/// differ. The first 64 bytes are enough to detect a difference and cheap enough to compare on a hot path.
/// Rows where the two views disagree are where a per-view value lives, and the shadow sampling matrices --
/// whatever node owns them -- have to appear among them.
#[no_mangle]
pub static mut CyberpunkVR_WideCensus: i32 = 1;
/// The per-field temporal detector rides on the same census; see the long note at kTFields.
#[no_mangle]
pub static mut CyberpunkVR_TemporalScan: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_CascSampleProbe: i32 = 1;
/// THE ENGINE'S HUD WARP, OFF UNDER THE ASYMMETRIC PROJECTION. The HUD composite bends the HUD about
/// each image's own centre (b6 r3.xy: x moves by cy^2*cx*k.x, y by 2*cx^2*cy*k.y, cx/cy from the centre).
/// The eyes' windows are mirror images, so one world point sits at another column in each eye and the
/// bend moves it by a different amount -- vertically too, which the eyes cannot fuse. Measured on the
/// NPC quest marker (1920x946 capture, k = 0.084242): MAIN at cx -0.21, the second view at cx +0.05,
/// MAIN 6 px higher near the top and 2 px lower at 62 % height, the warp's own numbers to 0.4 px.
/// Zero in both views keeps each in-eye marker on its point. The HUD panel keeps the engine's bend:
/// its composite takes the values kept here (HudPanel.cpp).
///
/// 2.5, UNDER THE ENGINE'S STEREO, the same block is 432 bytes and the composite (the indirect dispatch after the
/// HUD's blur pyramid; markeroutline.rdc event 95215, its b6) runs each eye's half about that eye's own centre:
/// the bend as above, and two more terms on each eye's image rather than on the world:
/// - the halo's taps (r7, r8.xy: LOD 1, 2 and 4 of the HUD at uv + k * (2 uv - 1), k -0.005 .. -0.0126) step out
/// from the eye's centre, so one marker's halo sat on another side of it in each eye -- the blurred double of
/// the breach and ripperdoc markers, left in the left eye and right in the right (2026-10-24). Taken at
/// the HUD's own pixel the halo stays round its marker in both.
/// - the shadow (r3.zw, 0.0008 of the target both ways: 4.2 x 2.3 eye pixels down and right) is the black copy of
/// every icon beside it (2026-10-24: the outline sits right of the icon). CyberpunkVR_HudShadowOffset
/// scales it; 0 puts it under the HUD.
#[no_mangle]
pub static mut CyberpunkVR_HudFlatInEye: i32 = 1;
#[no_mangle]
pub static CyberpunkVR_HudFlatInEyeBlocks: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static mut CyberpunkVR_HudShadowOffset: f32 = 0.0;
/// MAIN, second view, neither
#[no_mangle]
pub static mut CyberpunkVR_DebugAsymAoPatched: [u64; 3] = [0; 3];
/// ENGINE STEREO (render feature 98): the engine runs the cone AO over the side-by-side frame with the node's
/// two-halves mode (row 14.y) off, as one picture with the centre's tangents, and the AO thinned out along both
/// eyes' inner edges. 1 = turn the mode on with each eye's window and the separation in the block, for the twins'
/// stereo recipe (tools/asym_tests/asym_ao_shaders.py STEREO_MAGIC); 0 = the engine's block. Live.
#[no_mangle]
pub static CyberpunkVR_EngineStereoAoHalves: AtomicU32 = AtomicU32::new(1);
/// [0] blocks turned to two halves, [1] left alone (the eyes' tangents in rows 12/13 unusable).
#[no_mangle]
pub static mut CyberpunkVR_DebugEngineStereoAo: [u64; 2] = [0; 2];
/// THE AUTO-EXPOSURE'S VIEW CENTRE, written into its luminance image's block (node HistogramUpdate). The
/// meter samples the scene through a magnifier centred on the middle of the screen; the twins in asym_expo\
/// (tools/asym_tests/asym_expo_shaders.py) centre it on the view instead, which is what makes both eyes meter
/// the same region around the head's view. They read the centre from row 1.zw of the 24-byte block: z = x | y
/// << 16 in signed 1/16384 units (the view's NDC, y down, as for the cone AO), w = kExpoCentreTag. The block
/// ends at row 1.y, so on the constant uploader (256-byte slots) the upload grows to 32 bytes; a block that
/// arrives on the buffer uploader is left alone. CyberpunkVR_DebugExpoUploads says what the node uploads:
/// [i] = size (| 0x80000000 on the constant uploader), [8 + i] = count; CyberpunkVR_DebugExpoCentred counts
/// the blocks written for MAIN, the second view and neither.
#[no_mangle]
pub static mut CyberpunkVR_ExpoCentreBlockSize: u32 = 24;
#[no_mangle]
pub static mut CyberpunkVR_DebugExpoCentred: [u64; 3] = [0; 3];
/// THE AUTO-EXPOSURE'S ENVIRONMENT METER (tools/asym_tests/asym_expo_shaders.py has the reasoning): the light of
/// every direction the meter has seen, kept in cells around the player (u16), and the exposure set for the
/// light reaching the eye from the hemisphere in front of it, not for the picture. Two blocks of the node:
/// - the luminance image's (df89, after the centre write) grown to 96: row 2 MAIN's lens window (l, r, d, u
/// tangents), rows 3-5 the camera's world right / up / forward, row 3.w kExpoAccTag -- its samples add their
/// light to the cells of their directions;
/// - the adaptation's 56-byte block (b059) grown to 112 (row 6.x the level offset's rate, see
/// CyberpunkVR_ExposureLevelTime): row 3.z kExpoEnvTag, row 3.w dt over the surprise
/// fade's time, row 4 the view's world forward and the cells' write rate, row 5 the confidence fade (time and
/// the player's movement), the view's own part (gain, limit in natural log) and the mode (low byte; bits
/// 8-15 the lean towards the view's hemisphere, CyberpunkVR_ExposureHemisphere * 255).
/// MAIN alone meters (the second view takes MAIN's exposure, xr_expo_mirror) and has the cells bound; every other
/// view's grown blocks carry zeros -- written either way, so a constant slot never keeps a tag from an earlier
/// upload.
#[no_mangle]
pub static CyberpunkVR_ExposureMemory: AtomicU32 = AtomicU32::new(1);
/// 0 = the environment; diagnostics: 1 = the meter's own reading (the cells still learn), 2 = without the view's own part.
#[no_mangle]
pub static mut CyberpunkVR_ExposureMemoryMode: u32 = 0;
/// s: a cell in view takes the reading
#[no_mangle]
pub static mut CyberpunkVR_ExposureMemoryWrite: f32 = 0.25;
/// A still world does not change for not being looked at: what is remembered fades with the player's movement and
/// with a light the view finds changed, time alone only as a safety (20 s made the place collapse to wherever the
/// head rested: +24 % over 28 s looking under the overpass, measured 2026-10-16).
/// s: a cell out of view fades...
#[no_mangle]
pub static mut CyberpunkVR_ExposureMemoryAge: f32 = 600.0;
/// m: ...and per this many metres moved
#[no_mangle]
pub static mut CyberpunkVR_ExposureMemoryDistance: f32 = 4.0;
/// s: ...and while the view reads a new light
#[no_mangle]
pub static mut CyberpunkVR_ExposureMemorySurprise: f32 = 0.5;
/// s: the engine's meter against the cells' plain mean on the same samples, smoothed this slowly (the level offset).
#[no_mangle]
pub static mut CyberpunkVR_ExposureLevelTime: f32 = 8.0;
/// 0 = the sphere's mean light (the place); 1 = the view's cosine-weighted hemisphere (the light reaching the eye,
/// which under an overpass moves more with a head turn than the engine's meter does).
#[no_mangle]
pub static mut CyberpunkVR_ExposureHemisphere: f32 = 0.0;
/// IN A CAR THE PLACE IS THE CABIN (PR #282). Seen from the seat, most of the sphere is the dash, the footwell, the
/// seats and the roof, and only the band at eye level looks out through the glass: the sphere's mean light is the
/// interior's, so the place was exposed for a dark room and the street through the windows burned out. The game's own
/// centre-weighted meter (mode 1, still with the slow adaptation) is what its vehicle cameras were tuned with -- but a
/// headset's view is far taller than a monitor's, so even that meter took in mostly dash and roof: its low percentile
/// is raised as well (CyberpunkVR_ExposureVehicleLowPercentile).
/// 0 = the environment everywhere, 1 = the game's meter while mounted in anything with a cabin (not a motorcycle),
/// 2 = in every vehicle. Leaving the vehicle starts the cells afresh, so the cabin is not carried onto the street.
#[no_mangle]
pub static mut CyberpunkVR_ExposureVehicleMeter: u32 = 1;
/// In a cabin the histogram's low percentile is raised to this: the meter averages only the view's brightest 40 %
/// (up to the engine's high percentile), the glass rather than the dash and roof. The engine's own is 0.02.
#[no_mangle]
pub static mut CyberpunkVR_ExposureVehicleLowPercentile: f32 = 0.6;
/// the view's own difference taken...
#[no_mangle]
pub static mut CyberpunkVR_ExposureGazeGain: f32 = 0.15;
/// ...up to this many EV
#[no_mangle]
pub static mut CyberpunkVR_ExposureGazeLimit: f32 = 0.3;
/// [0] MAIN adaptation blocks tagged, [1] other adaptation blocks grown untagged, [2] adaptation blocks on the
/// buffer uploader (left alone), [3] the binder refused the cells, [4] cells not allocated, [5] camera
/// unreadable, [6] resets (start, a pause, a jump), [7] MAIN luminance-image blocks tagged.
#[no_mangle]
pub static mut CyberpunkVR_DebugExpoEnv: [u64; 8] = [0; 8];
/// The last MAIN block: forward xyz, write rate, fade, metres moved, dt (s).
#[no_mangle]
pub static mut CyberpunkVR_DebugExpoEnvParams: [f32; 8] = [0.0; 8];
/// OFF: composed grading block dump (scanner tint, parked)
#[no_mangle]
pub static mut CyberpunkVR_GradingProbe: i32 = 1;
/// So "it did not help" and "it never saw that view" can never be confused again.
#[no_mangle]
pub static mut CyberpunkVR_DebugGradeMainUploads: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugGradeVrcamUploads: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugGradeOtherView: u64 = 0;

// ---- src/Stereo/NativePairedFrame.cpp

#[no_mangle]
pub static mut CyberpunkVR_PreparationRootObserved: [[u64; 8]; 16] = [[0; 8]; 16];
#[no_mangle]
pub static CyberpunkVR_PreparationRootObservedCount: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparationGraphRequest: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparationGraphState: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparationGraphSeq: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparationGraphNodeCount: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparationGraphEdgeCount: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparationGraphFailure: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_PreparationGraphNodes: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_PreparationGraphEdges: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_PreparationGraphMeta: [u64; 4] = [0; 4];
#[no_mangle]
pub static mut CyberpunkVR_PreparationFrontierStats: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_PreparationFrontierRoles: [u64; 22] = [0; 22];
#[no_mangle]
pub static mut CyberpunkVR_PreparationFrontierCuts: [u64; 6] = [0; 6];
#[no_mangle]
pub static mut CyberpunkVR_PreparationFrontierAdds: [u64; 6] = [0; 6];
#[no_mangle]
pub static CyberpunkVR_PreparationOwnedGraphMode: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_PreparationOwnedGraphStats: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_PreparationOwnedGraphNodes: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_PreparationOwnedGraphEdges: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_PreparationWholePassStats: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_PreparationContextOrderStats: [u64; 24] = [0; 24];
#[no_mangle]
pub static mut CyberpunkVR_PreparationContextOrderWitnesses: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_PreparationContextOrderWitnessCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_PreparationContextOrderWitnessBytes: u32 = 24;
#[no_mangle]
pub static mut CyberpunkVR_PreparationSharedOwnerStats: [u64; 8] = [0; 8];
#[no_mangle]
pub static mut CyberpunkVR_PreparationSharedOwnerRows: u64 = 0;
#[no_mangle]
pub static CyberpunkVR_PreparationGraphJobsMode: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparationGraphJobsEpoch: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_PreparationGraphJobsStats: [u64; 8] = [0; 8];
#[no_mangle]
pub static mut CyberpunkVR_PreparationGraphJobsCompatibility: [u64; 8] = [0; 8];
#[no_mangle]
pub static mut CyberpunkVR_PreparationGraphJobsSelection: [u64; 8] = [0; 8];
#[no_mangle]
pub static mut CyberpunkVR_PreparationGraphJobsPending: [[u64; 8]; 4] = [[0; 8]; 4];
#[no_mangle]
pub static mut CyberpunkVR_PreparationGraphJobsSourceReject: [u64; 8] = [0; 8];
#[no_mangle]
pub static CyberpunkVR_PreparationGraphJobsPendingCount: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_PreparationResourceUsesStats: [u64; 12] = [0; 12];
#[no_mangle]
pub static mut CyberpunkVR_PreparationResourceUsesRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_PreparationResourceUseIds: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_PreparationResourceAuxKeys: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_PreparationResourceBoundaryIds: u64 = 0;
#[no_mangle]
pub static CyberpunkVR_PreparationResourceMarkerContract: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparationScheduleMode: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparationScheduleCompatible: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparationScheduleSeq: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_PreparationScheduleStats: [u32; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_PreparationScheduleRanges: [u64; 4] = [0; 4];
#[no_mangle]
pub static mut CyberpunkVR_PreparationScheduleEdges: u64 = 0;
#[no_mangle]
pub static CyberpunkVR_PreparationScheduleCache: AtomicU32 = AtomicU32::new(1);
#[no_mangle]
pub static CyberpunkVR_PreparationScheduleCpu: [AtomicU64; 6] = [const { AtomicU64::new(0) }; 6];
#[no_mangle]
pub static mut CyberpunkVR_PreparationScheduleCpuFrequency: u64 = 0;

// ---- src/Stereo/CommandListCensus.cpp

/// ---- name the node behind each compute dispatch -------------------------------------------
/// The night capture pinned the defect to one constant block: the pass at PSO 926 gets six
/// world-space light entries and a non-zero count for MAIN, and an empty list with count 0 for
/// VRCAM. What the capture cannot say is WHICH frame-graph node issues that dispatch, and
/// without the node there is nothing to read in the reverse dumps and nothing to hook.
///
/// The dispatch is identifiable by shape: it writes the tile grid, so its group count is the
/// render resolution over 16 in both axes. Recording the node RVA for every distinct
/// (groupX, groupY) per view names it in one run -- and gives the same table for every other
/// compute pass for free.
/// OFF: [dispatch] per-node dispatch census
#[no_mangle]
pub static mut CyberpunkVR_DispatchCensus: i32 = 1;
/// OFF -- the light arrays are byte-identical between the views.
#[no_mangle]
pub static mut CyberpunkVR_LightContent: i32 = 0;
/// ---- is the two views' auto-exposure the same? ---------------------------------------------
/// The lighting output has the lamps in BOTH views (checked in the capture), and every node after
/// it -- ApplyBloomAndTonemapping, GenerateTonemappingLUT, HistogramUpdate -- runs symmetrically.
/// What those share as an input but hold PER VIEW is the exposure: each view has its own 28-byte
/// FrameExposureData. A higher exposure for VRCAM would compress the highlights and keep the
/// lamps under the bloom threshold -- light present, glow absent, which is exactly the symptom.
/// Read here rather than inferred: the buffer is copied into a readback in the engine's own list
/// at the barrier that already identifies it, so no state is guessed and nothing is added to any
/// queue of ours.
/// 0 off, 1 a line every 8 s, 2 a line every 200 ms.
///
/// Default is 0, and the note above already said OFF while the value said 1 -- it copies 28
/// bytes out of an engine resource, with two barriers, on EVERY bind of that buffer, and that is
/// not something to leave armed for a difference that was measured and written down long ago.
///
/// Mode 2 exists for a TRANSIENT. Reported 2026-08-29: when the braindance glasses go on, MAIN
/// gets a flash of exposure -- the eye adapting to the cut -- and the second view gets nothing.
/// That is the shape this whole file predicts: adaptation is metered by HistogramUpdate, which
/// the second view is refused, so its exposure is a standing number that cannot flash. At 8 s a
/// line the flash falls between two samples and the log shows only the standing difference; at
/// 200 ms the ramp itself is in the log, field by field, for both views.
#[no_mangle]
pub static mut CyberpunkVR_ExpoProbe: i32 = 0;
/// ---- THE CLOUD BRIGHTNESS, and why it is only the clouds ------------------------------------
///
/// Nsight settles it. The cloud raymarch is PipelineState_1301, an indirect dispatch in
/// Transparents that read-writes the half-res cloud buffer, and its last line is:
///
/// float _471 = WaveReadLaneFirst(asfloat(_8.Load(0u).x));      // _8 = t37
/// _23[pixel] = float4(_471 * r, _471 * g, _471 * b, alpha);
///
/// and t37 is bound to `StructuredBuffer<FrameExposureData>` -- Resource_30946 on the second view,
/// Resource_2803 on MAIN. The clouds are multiplied by THAT VIEW'S EXPOSURE at raymarch time.
/// Alpha is left alone.
///
/// Everything else in the frame takes exposure later, at tonemap, where each view applies its own
/// consistently. The clouds are the only thing that bakes it early, so a per-view exposure
/// difference lands on the clouds and nowhere else -- a flat multiplier, not noise, which is
/// exactly the reported "VRCAM's clouds are always lighter".
///
/// And the difference was already measured by the probe below: 10-35%.
///
/// Two eyes 6.5 cm apart have no business metering differently -- unequal brightness between eyes
/// is binocular rivalry, tiring even when it is not consciously noticed. So this is a correction,
/// not a workaround: hand the second view MAIN's exposure.
///
/// Done with the probe's own proven mechanism. The buffer is copied at the barrier that hands it
/// to the shaders, so its state is known exactly, and the copy goes through a private staging
/// buffer rather than touching MAIN's resource from the second view's list: MAIN's copy is taken
/// where MAIN's state is known, VRCAM's is written where VRCAM's is. Frame order is VRCAM-first,
/// so the value applied is MAIN's from the previous frame -- exposure adapts over many frames, so
/// one frame of lag is nothing.
/// DEFAULT FLIPPED TO 0 ON 2026-08-29, ON THE PICTURE. Everything argued above is still right --
/// two eyes metering differently is binocular rivalry and wants correcting -- but THIS copy is too
/// blunt to be the correction. It takes all 28 bytes, and that carries field [0], where the two sides
/// were measured to differ by 2880x (0.945322 against 0.000328187). Handing the second view a
/// multiplier three thousand times its own whitens it, which is what was reported and what
/// switching this export off in the debugger addressed.
///
/// The replacement already exists and is not lost: the field-masked mirror written on top of this one
/// carries only the field that differs by the size of the actual symptom ([6], MAIN 1.0 against
/// 0.860). It went out with the render rollback and belongs back here -- until it does, the honest
/// default is off, because a correction that overshoots by three orders of magnitude is worse than
/// the difference it corrects.
/// SHIPS ON. It is not a diagnostic and not a workaround: it is the mechanism that gives both eyes
/// one exposure. Measured 2026-08-30, without it the second view's pre-exposure multiplier sits at
/// 0.945388 where MAIN's is 0.000328187 on the same scene -- 2880.6x -- and since every lighting
/// shader multiplies its entire output by that float, every render target of that view from
/// Lighting onward comes out white. Off = that comes back.
///
/// It only started working once the buffer stopped being identified by t_vrcam_node_active; see
/// the note at the classification in Capture.cpp.
#[no_mangle]
pub static mut CyberpunkVR_ExpoMirror: i32 = 1;
/// WHICH OF THE SEVEN FLOATS, and the measurement that chose them.
///
/// Reported 2026-08-29: when the braindance glasses go on, MAIN gets a flash -- the eye adapting
/// to the cut -- and the second view gets nothing. Sampled at 200 ms through the readback probe
/// in this file, both views side by side, the whole event reads:
///
/// at rest        MAIN f0 1.915  f3 0.5223   |  VRCAM f0 1.915  f3 0.5223
/// the cut        MAIN f0 0.5716 f3 1.750    |  VRCAM f0 1.856  f3 0.5389
/// +4 s           MAIN f0 1.885  f3 0.5304   |  VRCAM f0 1.915  f3 0.5223
///
/// So MAIN's multiplier drops 3.35x and climbs back over about four seconds, while the second
/// view moves three per cent and is done inside one. It does not adapt, which is what this file
/// already predicted: adaptation is metered by HistogramUpdate, and that node is refused for
/// this view. f0 is exactly 1/f3, and f1 and f4 move with f3 -- one quantity in four slots.
///
/// AT REST THE TWO VIEWS AGREE TO FOUR FIGURES, which is the whole reason a masked copy is safe
/// where the 28-byte copy was not: on these four fields the write is a no-op except during a
/// transient, so it can only add the flash and cannot shift the standing brightness. The blunt
/// version carried every field, including one where the views were once measured 2880x apart,
/// and whitening the eye by three thousand times is what took it out of the defaults.
///
/// One bit per float. 27 = f0|f1|f3|f4, the adaptation set. 127 is the old whole-buffer copy.
/// DEFAULT 25 = f0|f3|f4, MEASURED 2026-08-30 out of the Nsight capture. The seven floats of
/// each view's FrameExposureData, read at the lighting pass that consumes them:
///
/// VRCAM  f0 0.945388     f1 0.110189  f2 0.0003472  f3 1.05777  f4 1.05793
/// MAIN   f0 0.000328187  f1 0.25      f2 1.0        f3 3047.04  f4 3047.04
///
/// f0 is the pre-exposure the lighting shader multiplies its entire output by, f3 is its
/// reciprocal (f0*f3 == 1 in both views) and f4 tracks f3. Those three are the quantity, and
/// they only mean anything as a set. f1 and f5/f6 are settings that legitimately differ per
/// view, and f2 is the pending correction the view computed for itself -- copying MAIN's 1.0
/// over it would tell the second view there is nothing left to correct.
///
/// DEFAULT 127 -- THE WHOLE BUFFER -- BECAUSE THE TWO EYES MUST SHOW ONE EXPOSURE.
///
/// With 25 (f0|f3|f4) the second view converged to MAIN's f1 and f2 by itself inside a frame, so
/// its chain was never broken; it was only ever starting from the wrong multiplier. That made a
/// narrow mask look attractive -- leave the view its own adaptation. It is the wrong instinct:
/// independent adaptation in the second view is a defect by construction, because the eyes would
/// then disagree with each other. Why should it adapt on its own? The answer
/// is that it must not.
///
/// 25 = f0|f3|f4 (the multiplier, its reciprocal and its previous value), 9 = the bare f0|f3 pair.
#[no_mangle]
pub static mut CyberpunkVR_ExpoFieldMask: u32 = 127;
/// ---- does the light cull actually FILL the tile grid for VRCAM? ----------------------------
/// The lamp's own emissive surface shows in both views; the light it casts does not -- no red
/// wash under the red lamps, no lit roof, no lit road. In a clustered renderer that is exactly
/// "the light list is fine but the per-tile assignment is empty", and the list IS fine: compared
/// byte for byte, same length, same entries. So read the cull's OUTPUT -- the R16_UINT grid at
/// render-res/32 that the lighting pass indexes. Non-zero tiles for MAIN and zeros for VRCAM
/// would settle this; equal grids move the search to the lighting shader's other inputs.
/// OFF -- the per-tile grids read equal once the readback was finally synchronised.
/// DEFAULT 0. The comment above already said OFF while the value said 1, which is how it kept
/// running: a GetDesc and a readback copy on barriers, for a question closed long ago.
#[no_mangle]
pub static mut CyberpunkVR_TileProbe: i32 = 0;
/// ---- how many lights does each view's cull actually output? --------------------------------
/// The capture shows both views clearing and filling the SAME persistent UAV buffers for the
/// clustered light list -- Resource_1360 (20 B counter), 2980/2981 (3 MB each), 2991, 2994, 4943 --
/// while some neighbouring counters are per-view transients. A 20-byte counter is the cheapest
/// thing in that set to read, and it separates the two remaining explanations outright:
/// count == 0 for VRCAM  -> its cull produces nothing, and the fault is in the cull's inputs;
/// count  > 0 for VRCAM  -> the cull works and the result is overwritten before its lighting
/// reads it, i.e. the shared buffers are raced (AsyncComputeDuring-
/// Shadowmaps is a separate list and a likely second writer).
/// Read off a UAV barrier, not a transition: this counter never changes state, it stays in
/// UNORDERED_ACCESS, so the copy brackets it explicitly from a state we know rather than guess.
/// OFF -- the 20-byte counters it found were indirect-draw args, now understood.
#[no_mangle]
pub static mut CyberpunkVR_CullCountProbe: i32 = 1;
/// ---- the work the Dispatch census could never see -------------------------------------------
/// The lighting is issued through ExecuteIndirect, which is vtable slot 59 -- an entirely
/// different call from Dispatch (slot 14). Every census so far was blind to it, which is why they
/// all reported the lighting nodes as symmetric while the light was plainly missing.
///
/// The capture says what to expect: inside the Lighting list MAIN issues one
/// CommandSignature_81 / Resource_1359 indirect DRAW (that signature is used 126 further times in
/// GBuffer_Discard and the shadow cascades, so it is a draw, not a dispatch -- i.e. light volumes)
/// and VRCAM issues none, while every other signature/argument pair matches one for one.
/// ARMABLE ON ITS OWN, and default 0. It used to be reachable only through the launcher DEBUG
/// tick, which arms forty-two probes at once and costs most of the frame rate -- so the one
/// census that names the node behind the recurring 0x88 crash could not be read without making
/// the game unplayable.
///
/// Why it is the one that matters: the crash faults in the driver computing a resource state of
/// 0x200 = INDIRECT_ARGUMENT on a binding whose resource record is null, the debug layer reports
/// 1422 (a placed resource used by ExecuteIndirect before it was initialised) on RenderCascade0
/// and RenderCascade1, and the note in hk_ExecuteIndirect above already recorded the mechanism:
/// the SECOND VIEW replays a frame-graph node whose argument resource has not been rebuilt, and
/// MAIN never does. What is missing is the node's name. This census holds exactly that -- node
/// RVA against per-view hit counts -- so arming it during a session that crashes names it.
/// [indirect] ExecuteIndirect census
#[no_mangle]
pub static mut CyberpunkVR_IndirectCensus: i32 = 0;
/// ---- which PSO draws the holographic sight ---------------------------------------------------
/// The reticle is a 6-index instanced quad (PipelineState_29513 in the capture, Forward_NoTXAA,
/// depth range 0.9..1.0 -- the first-person weapon layer). To give it real collimated behaviour
/// its pixel shader has to be replaced, and a shader can only be swapped where the PSO is CREATED.
/// So creation needs a stable name for it. The PSO pointer is not stable across runs; the PS
/// bytecode hash is, being a hash of the bytes themselves.
///
/// This pass only NAMES it. Nothing is substituted yet and no rendering changes.
/// OFF: both hashes are
#[no_mangle]
pub static mut CyberpunkVR_PsoProbe: i32 = 1;
/// known and hardcoded now, and this runs on every 6-index instanced draw -- of which a menu
/// issues tens of thousands. Set to 1 to re-identify a shader.
/// Identification by REMOVAL. Two pixel shaders draw a 6-index instanced quad once per view per
/// frame inside CRenderNode_RenderElements, and their tallies are equally balanced, so counting
/// cannot separate them. Setting this to one of the two hashes drops that draw: whichever makes
/// the reticle disappear IS the reticle. Instant, reversible, and it also proves the whole
/// identification chain end to end before anything is substituted for real.
#[no_mangle]
pub static mut CyberpunkVR_SightSkipPs: u64 = 0;
/// CONFIRMED by removal, 2026-07-29: dropping this pixel shader's draw removes the reticle, and
/// dropping the other 6-index quad in the same node (FA07D39A5AFDFBA5) changes nothing visible.
/// Game 2.5 recompiled it (same code, wider constant buffer declarations): EE25C03C35C017BD, the
/// material parallaxscreen_transparent_ui's pixel shader, paired with 2.31's 66394C5F4B95AB9A through
/// the shader cache's technique records (build/optics-20261026/sight_pairs.py).
#[no_mangle]
pub static mut CyberpunkVR_SightPsHash: u64 = 17160333319997495229;
/// One-shot: write the original bytecode out, so the replacement can be built against the real
/// container -- shader model, input signature and whether this is DXBC or a DXIL blob decide
/// whether it is fxc or dxc that has to compile it. Guessing that would waste a build.
/// one-shot, long done
#[no_mangle]
pub static mut CyberpunkVR_SightPsDump: i32 = 1;
/// Draw probe: the shaders of the draws with this many indices (one chunk of a known mesh), to find a
/// material's pipelines live -- the hash and the length name the shader in the game's shader cache
/// (tools/asym_tests/extract_shader_cache.py hashes the same bytes). Up to four pixel shaders, first
/// seen first. 0: off.
#[no_mangle]
pub static CyberpunkVR_DrawProbeIndices: AtomicU32 = AtomicU32::new(0);
/// Which dispatch inside the vision node is the outline generator, and where that count stands.
/// WHICH 384x384 DISPATCH IN THE NODE IS THE OUTLINE: the third, ordinal 2. Established on the
/// picture, not by reasoning -- with the shader forced to write RED while keeping the alpha it
/// computed, ordinal 2 turned the outline red and left the rest of the frame intact, while 0 blacked
/// the frame out (it writes the frame's colour) and 1 changed nothing. It agrees with the capture's
/// own event list, where the outline dispatch (event 44898, PipelineState_917) is the third of the
/// five 384x384 dispatches recorded into the PostFX list.
#[no_mangle]
pub static mut CyberpunkVR_VisionCsOrd: i32 = 2;
#[no_mangle]
pub static mut CyberpunkVR_VisionCsDim: i32 = 384;
/// The node whose square dispatch draws the outline, measured by [viscs] rather than remembered: the
/// historical 0x4262CC moved to 0x777514 with a game update. Kept separate from CyberpunkVR_VisionNode,
/// which also picks the surface the second eye copies -- one number, one meaning.
/// 0xFFFFFFFF = do not filter by node at all, which is the shipping value: see the note at the
/// selection itself. A concrete RVA is still accepted, for narrowing a measurement by hand.
#[no_mangle]
pub static mut CyberpunkVR_VisionCsNode: u32 = 4294967295;
/// OFF: a GetGPUVirtualAddress() call and a mutex on every CopyBufferRegion, every descriptor
/// table bind and every resource creation -- thousands a frame. It answered its question; the
/// cost it leaves behind is uneven frame time.
#[no_mangle]
pub static mut CyberpunkVR_SightAxisProbe: i32 = 1;
/// ---- the second view's grading LUT is built from a DIFFERENT SOURCE TEXTURE --------------------
///
/// Inside GenerateTonemappingLUT the three build dispatches read an `8 x Texture3D<float4>` SRV
/// table, and the two views are handed different tables. Measured in an Nsight capture: MAIN's
/// source has mean rgb 0.044/0.200/0.056, the second view's 0.962/0.836/0.635 -- not a shade apart,
/// a different asset. It is a DESCRIPTOR TABLE, not a constant, which is why no buffer diff could
/// ever see it: viewData, the graph context, the 688-byte grading block and the cloud buffer were
/// all compared field by field at a yellow lamp where MAIN reads warm and the second view cold, and
/// every one of them came back either equal or already mirrored.
///
/// So MAIN's table is learned inside that node and handed to the second view inside that same node,
/// per root-parameter index. Everything outside the node passes through untouched, and the scope is
/// a node identity the port itself sets, not a guess about what the data looks like.
///
/// THE KNOWN HAZARD, and why this defaults to 0: descriptor handles address a shader-visible heap
/// the engine recycles. The second view records its frame BEFORE MAIN, so the handle lent to it is
/// MAIN's from the PREVIOUS frame, and a heap that has wrapped in between would hand it something
/// else entirely. If the picture goes wrong rather than merely unchanged, that is the first thing to
/// suspect -- and the fix would be to learn the table at its creation instead of at its bind.
/// CONFIRMED ON THE PICTURE 2026-09-05, which is why this ships at 1: with the lend on, the second
/// eye at the street lamp went from cold to the same warm as MAIN, immediately. Everything else had
/// already been excluded by measurement -- viewData, the graph context, the 688-byte grading block
/// with all 26 fields substituted, the cloud buffer copied whole, the render masks, the reuse modes
/// and the eye's capture source. The difference was never a number in a buffer; it was which texture
/// the build was pointed at.
///
/// THE HAZARD, SEEN 2026-10-18 at the new-game mirror: the second eye's bright surfaces (a lit metal
/// edge, a sticker, a hook) flashed solid yellow, green and magenta from frame to frame, MAIN clean,
/// gone the moment xr_lut_src_lend went to 0 and back the moment it returned. A LUT built from
/// whatever the recycled handles held by then. So a table is lent only while it is ONE build old:
/// MAIN must have built its LUT since the second view's previous build. Otherwise the second view
/// builds from its own table that frame -- its own grade for a frame instead of garbage.
#[no_mangle]
pub static mut CyberpunkVR_LutSrcLend: u32 = 1;
/// ---- draw census: which nodes draw for one view and never for the other ---------------------
/// OFF: [draw] per-node draw census + imbalance
#[no_mangle]
pub static mut CyberpunkVR_DrawCensus: i32 = 1;
/// ---- WHAT EACH VIEW PUTS INTO ITS OWN SHADOW CASCADE -------------------------------------------
///
/// Shadows cast by fences, plants and trees differ between the eyes: a piece of shadow is present in
/// one and missing in the other, in different places. A capture settled the first half of that -- the
/// cascade draws come in TWO separated runs per frame (RenderCascade0: events 19657..20440, then
/// 46391..47167, with the other view's scene passes in between), so each view rasterises its OWN
/// cascade rather than sharing one. Cutting the cascade node for the second view confirmed it from the
/// other side: that eye lost sun shadows entirely, so the render target is per graph.
///
/// The same capture showed the two runs drawing ALMOST the same thing, differing in instance counts --
/// 373 against 376 of one 33-index mesh, 554 against 556, 70 against 71 -- which is a handful of
/// instanced foliage cards in one eye's shadow map and not the other's.
///
/// BUT THAT CAPTURE CANNOT BE TRUSTED FOR THIS, and saying so is the point of this probe: it was taken
/// with the two eyes at different resolutions (3072 against 2560) because Nsight would not record
/// otherwise, and LOD selection and culling margins both scale with resolution. The difference may
/// therefore be an artefact of the capture conditions rather than the reported defect.
///
/// So this counts the same thing live, where the eyes are configured alike: per view, how many draws
/// the cascade node issues, how many instances in total, and an order-independent sum over the draw
/// arguments -- so a difference in WHICH draws are issued shows up even when the totals match. Counters
/// are per interval, reset after each report, because two numbers from the same seconds compare and two
/// running totals from different seconds do not.
/// DEFAULT 0: it counted every cascade draw in the frame and its job is done (the two views draw the
/// same casters, 94002 against 94004). Kept for repeating that measurement.
#[no_mangle]
pub static mut CyberpunkVR_CascadeCensus: i32 = 0;
/// Optional per-side cascade census. It observes native draws without dropping them.
#[no_mangle]
pub static mut CyberpunkVR_CascSideCensus: i32 = 1;
/// ---- VIEW AND NODE MARKERS, so a capture labels itself -------------------------------------------
///
/// Built because a question could not be answered from three captures in a row: with MAIN's cascade draws
/// withheld, a block of cascade draws still appeared in the middle of the frame, and NOTHING in the file said
/// whether that region belonged to MAIN's graph or the tail of the second view's. The two readings could not be
/// separated, because a D3D12 capture is a flat list of five thousand commands with no idea that this port runs
/// the graph twice.
///
/// So the port says it. One PIX marker whenever the (command list, node, view, cascade) tuple changes.
///
/// AND IT DOES NOT REACH A RENDERDOC CAPTURE AS BUILT -- measured: markers found 0, with DEBUG armed and the
/// counter proving they were emitted. Our vtable hook sits BELOW RenderDoc's wrapper, so the marker goes into
/// the real command list and the capture layer above never sees it. The same ordering is why a draw this file
/// withholds still appears in a capture: RenderDoc serialises what the GAME called, then our hook drops it.
///
/// To fix it the marker has to be emitted on the object the GAME holds, i.e. by hooking CreateCommandList on the
/// device vtable the game uses (which DEPTH-DIAG already reaches) and then that list's vtable -- in a capture run
/// that is RenderDoc's wrapper. Left as is for now, because the thing markers were built to answer got answered
/// another way: read whose camera is bound (CameraShaderConsts float 144, the eyes 0.0640 m apart in X) and every
/// region labels itself, on every capture already taken. Kept because they DO show in a PIX or Nsight capture,
/// which hook at the driver level rather than above us.
///
/// Emitted BEFORE the withheld-draw test on purpose. A node whose draws are all dropped otherwise records
/// nothing at all and vanishes from the capture; with the marker it still appears, empty and labelled, which is
/// exactly the evidence the cascade-ownership question needs.
///
/// SetMarker rather than BeginEvent/EndEvent: a marker needs no matching close, so there is no path by which
/// this can leave a command list with an unbalanced scope. Metadata 1 is the PIX convention for a
/// null-terminated ANSI string.
/// DEFAULT 0. This is RenderDoc scaffolding and it shipped armed. Every change of
/// (command list, node, view, cascade) formats a label with sprintf_s and calls SetMarker on one of
/// the game's own command lists -- and a camera feed or a monitor is an EXTRA view (it classifies as
/// neither-eye), so each active one multiplies the marker count. Turn it on for a capture; it has no
/// business costing frame time in play.
#[no_mangle]
pub static mut CyberpunkVR_CaptureMarkers: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugCaptureMarkers: u64 = 0;
/// Single-pass ceiling probe (Render/GeometryCostProbe.hpp). Off unless the live measurement script
/// writes the control; every native draw otherwise pays one relaxed load.
#[no_mangle]
pub static CyberpunkVR_GeometryCostProbe: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_GeometryCostNode: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_GeometryCostHits: AtomicU64 = AtomicU64::new(0);
/// Optional: only draws with this pipeline (0 = any).
#[no_mangle]
pub static CyberpunkVR_GeometryCostPso: AtomicU64 = AtomicU64::new(0);
/// [EI-DIAG] How often this hook drops the game's draw, and how often it declines to issue one that
/// would kill the process. Exported so they can be read without waiting for a log line.
#[no_mangle]
pub static mut CyberpunkVR_DebugEiNoEntry: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugEiNullArgs: u64 = 0;
/// ---- which node builds the colour-grading tables? -------------------------------------------
/// The scanner's green tint is missing in the second eye, and the capture says why: the tonemap
/// pass (PipelineState_777) is bound `3 x Texture3D<float3>` -- three 48^3 R11G11B10 tables --
/// and at VRCAM's draw they hold a neutral colour cube while at MAIN's draw they hold the graded
/// green one. Same shader, same three resources, rebuilt once per view by three 6x6x6 dispatches
/// (48^3 / 8^3) in AsyncComputeDuringShadowmaps; only the per-view constants differ, and VRCAM's
/// come out ungraded.
///
/// The tables are a SHARED resource and the frame order is VRCAM then MAIN, so the fix is simply
/// to let VRCAM skip its own build and sample the ones MAIN left. Colour grading is
/// view-independent -- both eyes MUST have the same grade -- so that is the correct answer, not a
/// workaround, and it costs the second eye one frame of grading latency.
///
/// This probe exists only to name the node, so CyberpunkVR_NodeCutSet can target it without
/// another rebuild. Cubic thread-group shapes are rare enough to be a clean filter.
/// OFF: cubic-dispatch (volume/LUT) probe
#[no_mangle]
pub static mut CyberpunkVR_VolumeNodeProbe: i32 = 1;

// ---- src/Render/SsrWideMerge.cpp

/// 1 = merge the eyes' reflections (needs the asym_ssr twins); 0 = each eye traces alone.
#[no_mangle]
pub static CyberpunkVR_SsrMerge: AtomicU32 = AtomicU32::new(1);
/// The same for the planar reflections.
#[no_mangle]
pub static CyberpunkVR_SsprMerge: AtomicU32 = AtomicU32::new(1);
/// 0 = merge; diagnostics: 1 = paint the pixels that took the other eye's result red, 2 = show the other eye's
/// result wherever it is the same surface (black elsewhere), 3 = show this eye's own result.
#[no_mangle]
pub static mut CyberpunkVR_SsrMergeMode: u32 = 0;
/// The other eye's result is the same surface if its view depth is within this fraction (+1 cm).
#[no_mangle]
pub static mut CyberpunkVR_SsrMergeTolerance: f32 = 0.03;
/// [0] MAIN blocks, [1] second-view blocks, [2] other views (left alone), [3] binder refused a slot,
/// [4] store allocation failed, [5] block/camera unreadable, [6] store capacity (entries), [7] width << 16 | height.
#[no_mangle]
pub static mut CyberpunkVR_DebugSsrMerge: [u64; 8] = [0; 8];
/// the same for the planar reflections
#[no_mangle]
pub static mut CyberpunkVR_DebugSsprMerge: [u64; 8] = [0; 8];
/// The cone AO's merge (channel kConeAo, tools/asym_tests/asym_ao_shaders.py ao_merge): 1 = on (needs the
/// asym_ao twins and the centre Stereo/Grading.cpp writes into the block).
#[no_mangle]
pub static CyberpunkVR_AoMerge: AtomicU32 = AtomicU32::new(1);
/// 0 = merge; diagnostics: 1 = the pixels that moved towards the other eye black, 2 = the other eye's AO
/// wherever it is the same surface (open elsewhere), 3 = this eye's own AO.
#[no_mangle]
pub static mut CyberpunkVR_AoMergeMode: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_AoMergeTolerance: f32 = 0.03;
/// How fast a pixel moves to the other eye's AO: by this times the difference of the fractions of cone
/// samples the two eyes lost off their pictures (4: all the way at a quarter more lost).
#[no_mangle]
pub static mut CyberpunkVR_AoMergeStrength: f32 = 4.0;
#[no_mangle]
pub static mut CyberpunkVR_DebugAoMerge: [u64; 8] = [0; 8];
/// The shadow mask's sun factor merged as one wide display (channel kShadowMask, asym_shadow_shaders.py).
#[no_mangle]
pub static CyberpunkVR_ShadowMerge: AtomicU32 = AtomicU32::new(1);
/// 0 = merge; diagnostics: 1 = the pixels that moved towards the other eye fully shadowed, 2 = the other eye's
/// factor wherever it is the same surface (lit elsewhere), 3 = this eye's own factor.
#[no_mangle]
pub static mut CyberpunkVR_ShadowMergeMode: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugShadowMerge: [u64; 8] = [0; 8];
/// Diagnostic: nonzero snapshots the exposure cells (cvr::ssr_merge::BindExposureCells) into
/// CyberpunkVR_DebugExpoCells (raw entries: the cells' float3, then the frame's sums' uint3); Seq counts snapshots.
#[no_mangle]
pub static mut CyberpunkVR_DebugExpoCellsRequest: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugExpoCellsSeq: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_ProbeLayoutNode: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugLayoutCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugLayoutBytes: [u8; 644] = [0; 644];

// ---- src/Stereo/SyncStereo.cpp

#[no_mangle]
pub static mut CyberpunkVR_DebugSecondaryManager: usize = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugSecondaryActive: usize = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugSecondaryBinding: usize = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugLastBatchManager: usize = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugLastWorkContext: usize = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMainNodeWorks: [usize; 256] = [0; 256];
#[no_mangle]
pub static mut CyberpunkVR_DebugSecondaryNodeWorks: [usize; 256] = [0; 256];
#[no_mangle]
pub static mut CyberpunkVR_DebugEyeItem: usize = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugEyeBuildMgr: usize = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugEyeFa0: usize = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugEyeMgrArray: usize = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugEyeMgrQueued: usize = 0;
/// caller RVA that requested it
#[no_mangle]
pub static mut CyberpunkVR_DebugDescHeapSVRetRva: usize = 0;
/// absolute caller return address
#[no_mangle]
pub static mut CyberpunkVR_DebugDescHeapSVRetAbs: usize = 0;
/// Enable/disable the VRCAM component at runtime. The component's isEnabled lives behind the
/// game's RTTI, which only Lua can reach, so this writes a request into the CET mod's bridge
/// folder and modules/vrcam_select.lua acts on it. 1 = the selected component on (default),
/// 0 = every vrcam component off, i.e. the engine stops rendering the second view entirely.
#[no_mangle]
pub static mut CyberpunkVR_VrcamEnabled: u32 = 1;
/// STAGE PROBE (path-A, 17): the vrcam post-color id 0x3C7E6258 is resolved at MANY
/// pipeline STAGES (the resolver's a4/r9 salt = a stage index 36..71, live-confirmed).
/// This maps every stage's resolved physical index per frame and dumps the whole map at
/// the fin (mirror-blit) resolve, so we can diff DLSS on/off and see which stage's physical
/// the fin consumer should read. Read-only; gated by CyberpunkVR_StageProbe (default 0).
#[no_mangle]
pub static mut CyberpunkVR_StageProbe: i32 = 1;
/// ---- who wants the VRCAM colour snapshot -----------------------------------------------
/// The snapshot used to exist purely for the desktop mirror, so its capture was gated on
/// MirrorOutput. The OpenXR submit now reads the SAME resource for the right eye, and it must
/// be able to do so with the mirror window closed -- that window is a capture convenience,
/// not a prerequisite for stereo.
#[no_mangle]
pub static mut CyberpunkVR_StereoEyeCapture: i32 = 1;

// ---- src/Runtimes/OpenXRCapture.cpp

#[no_mangle]
pub static CyberpunkVR_DebugSecondEyeCopies: AtomicU64 = AtomicU64::new(0);
/// 1 = while a menu is open, both eyes get the backbuffer (the menu) instead of pairing it with
/// VRCAM_s stale view of the world. 0 = the previous behaviour, for A/B.
#[no_mangle]
pub static mut CyberpunkVR_MonoMenu: i32 = 1;
/// ---- WHY A PRESENT DID NOT PRODUCE A CAPTURE -------------------------------------------------
///
/// Measured because the frame accounting proved this path is where the jumps come from, and could not
/// say which of the five early-outs below causes them.
///
/// The evidence: in GAMEPLAY windows (presents 40-70/s, so no menus or loading), a submit carrying an
/// image 30-53 ms old happens once every ~3.9 seconds, and in those windows the PRESENT RATE IS
/// UNCHANGED -- 50.3/s against 51.4/s in clean windows. The game kept delivering frames; the capture
/// did not publish one for two or three display periods. So the cause is here, not in the game.
///
/// The fence wait is the prime suspect and is timed rather than just counted: it runs ON THE GAME'S
/// PRESENT THREAD, so every millisecond it blocks is a millisecond the game is not presenting -- which
/// would explain both the stale submit AND the 1.1/s present-rate difference at once.
#[no_mangle]
pub static CyberpunkVR_DebugCapSkipNoView: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_DebugCapSkipNoRes: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_DebugCapSkipNoSlot: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_DebugCapSkipFence: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_DebugCapSkipReset: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_DebugCapOk: AtomicU64 = AtomicU64::new(0);
/// The second eye's CONTENT age -- see the sampling site for why this is not the same thing as [xreye].
#[no_mangle]
pub static CyberpunkVR_DebugEyeAgeCount: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_DebugEyeAgeSumMs: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_DebugEyeAgeMaxMs: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_DebugEyeAgeNever: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_DebugEyeAgeBuckets: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];
/// The bounded fence wait: how often it is entered at all, and how long it actually blocked.
#[no_mangle]
pub static CyberpunkVR_DebugCapFenceWaits: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_DebugCapFenceUsSum: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_DebugCapFenceUsMax: AtomicU64 = AtomicU64::new(0);

// ---- src/Stereo/EngineStereo.cpp

/// 1 = the eye cameras turn the TAA jitter into NDC with their own width (half the view's): each eye's samples
/// cover a whole pixel, as the engine's pixel jitter means. 0 = the view's width, as the engine hands it (an eye
/// jitters over half a pixel horizontally). Live: the size is written at every jittered build.
#[no_mangle]
pub static mut CyberpunkVR_EngineStereoEyeJitterWidth: i32 = 0;
/// MAIN frames prepared with the engine's stereo off / on.
#[no_mangle]
pub static mut CyberpunkVR_DebugEngineStereoFrames: [u64; 2] = [0; 2];
/// What the engine was last handed: separation, first-person separation, convergence.
#[no_mangle]
pub static mut CyberpunkVR_DebugEngineStereoValues: [f32; 3] = [0.0; 3];
/// Eye camera builds given their own window: left, right.
#[no_mangle]
pub static mut CyberpunkVR_DebugEngineStereoEyeBuilds: [u64; 2] = [0; 2];
/// The last eye camera built, per eye: aspect, shift x, shift y as given; P00, P11, P20, P21 as built.
#[no_mangle]
pub static mut CyberpunkVR_DebugEngineStereoEyes: [[f32; 7]; 2] = [[0.0; 7]; 2];
/// The last eye camera built, per eye: its jitter x, y in pixels and the width, height that turned them into NDC.
#[no_mangle]
pub static mut CyberpunkVR_DebugEngineStereoEyeJitter: [[f32; 4]; 2] = [[0.0; 4]; 2];
/// First-person plane eyes put back at their own place after the plane's override rebuild: left, right.
#[no_mangle]
pub static mut CyberpunkVR_DebugEngineStereoPlaneEyes: [u64; 2] = [0; 2];
/// With diagnostics on, MAIN's world plane (0) and first-person plane (2) as the view producer left them: centre, left
/// eye, right eye, each jitter x, y, jitter size w, h, P20, P21.
#[no_mangle]
pub static mut CyberpunkVR_DebugEngineStereoViewEyes: [[[f32; 6]; 3]; 2] = [[[0.0; 6]; 3]; 2];
/// 1 (default) = ink lays the UI out in one eye's window (UiOneEye); 0 = in the whole window, as the engine does.
/// Read when a layer is resized: at start-up, and when the window changes size.
#[no_mangle]
pub static mut CyberpunkVR_EngineStereoUiOneEye: i32 = 1;
/// Layer resizes handed one eye's size / passed as they came; the last size asked for and the size given.
#[no_mangle]
pub static mut CyberpunkVR_DebugEngineStereoUiResizes: [u64; 2] = [0; 2];
#[no_mangle]
pub static mut CyberpunkVR_DebugEngineStereoUiSize: [i32; 4] = [0; 4];
/// Layer ticks handed the layer's one-eye size as their screen (Detour_LayerTick).
#[no_mangle]
pub static mut CyberpunkVR_DebugEngineStereoUiTicks: u64 = 0;
/// 1 (default) = the loading screen is shown as one picture on the menu panel while its layer is up (LoadingShown).
#[no_mangle]
pub static mut CyberpunkVR_EngineStereoLoadingPanel: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_DebugEngineStereoLoadingTicks: u64 = 0;

// ---- src/Stereo/AsymProjection.cpp

#[no_mangle]
pub static mut CyberpunkVR_AsymSquarePixels: i32 = 1;
/// [0..3] MAIN: NDC shift x, y; aspect (0 keeps the engine's); unused. [4..7] the same for VRCAM.
#[no_mangle]
pub static mut CyberpunkVR_AsymTest: [f32; 8] = [0.0; 8];
/// Prepare calls seen: MAIN, VRCAM, other views.
#[no_mangle]
pub static mut CyberpunkVR_DebugAsymPrepare: [u64; 3] = [0; 3];
/// The input block as the engine left it, per eye (MAIN [0..7], VRCAM [8..15]):
/// fov, +0x1E44, aspect, zoom, shift x, shift y, near, far.
#[no_mangle]
pub static mut CyberpunkVR_DebugAsymView: [f32; 16] = [0.0; 16];
/// The plane overrides as the engine left them, per eye and plane (4 x 8 floats each):
/// fov, zoom, shift x, shift y, [4], [5], near, far; zeros where the plane has none.
#[no_mangle]
pub static mut CyberpunkVR_DebugAsymPlanes: [f32; 64] = [0.0; 64];
/// What the engine built, per eye (MAIN [0..15], VRCAM [16..31]): scene descriptor fov, aspect, zoom,
/// shift x, shift y, P00, P11, P20, P21; first-person descriptor fov, shift x, shift y, P00, P11, P20,
/// P21. Read right after the producer returns -- the numbers the frame is drawn with.
#[no_mangle]
pub static mut CyberpunkVR_DebugAsymBuilt: [f32; 32] = [0.0; 32];
/// The terms the input fov goes through on the way to the descriptor, per eye:
/// descriptor fov = [view+0x17F4] * input fov + [view+0x1D40].
#[no_mangle]
pub static mut CyberpunkVR_DebugAsymFovTerms: [f32; 4] = [0.0; 4];
/// The window each eye was given in mode 1, tangents l, r, d, u (MAIN [0..3], VRCAM [4..7]).
#[no_mangle]
pub static mut CyberpunkVR_DebugAsymWindow: [f32; 8] = [0.0; 8];
/// All four descriptors per eye after the producer (eye*4+desc)*8: fov, aspect, zoom, shift x, shift y,
/// jitter x, jitter y, P20. And whether each plane carries an override right now (eye*4+plane).
#[no_mangle]
pub static mut CyberpunkVR_DebugAsymDescs: [f32; 64] = [0.0; 64];
#[no_mangle]
pub static mut CyberpunkVR_DebugAsymPlanePtr: [u32; 8] = [0; 8];
/// The sun cascades fitted to both eyes' frusta (Detour_CascadeFit). 0 = each view fits its own window.
#[no_mangle]
pub static mut CyberpunkVR_AsymCascadeUnion: i32 = 1;
/// Fits given the union: MAIN, VRCAM, and fits left alone (no window of ours on the view, or off).
#[no_mangle]
pub static mut CyberpunkVR_DebugAsymCascadeFits: [u64; 3] = [0; 3];
/// What the last fit of each view was given: vertical fov, aspect, shift x, shift y (MAIN [0..3], VRCAM [4..7]).
#[no_mangle]
pub static mut CyberpunkVR_DebugAsymCascadeWindow: [f32; 8] = [0.0; 8];

// ---- src/Stereo/DeviceHooks.cpp

/// Creation counts since launch (vr_bench ab --counters, per window): [0] committed resources, [1] their bytes,
/// [2] placed resources, [3] their allocation bytes, [4] descriptor heaps, [5] graphics/stream pipelines,
/// [6] compute pipelines, [7] root signatures; who asked: [8] committed by the port, [9] committed by the game's exe,
/// [10] placed by the port, [11] placed by the exe (others: drivers, Streamline, DLSS); time inside the runtime and
/// driver: [12] committed, [13] placed (100 ns units); [14] mesh-shader pipelines (an MS subobject). No heaps or root signatures are created in play; resources are,
/// by the engine's streaming as the head turns (the port creates none) -- see CyberpunkVR_StreamingPlaced above.
#[no_mangle]
pub static CyberpunkVR_CreationCounts: [AtomicU64; 15] = [const { AtomicU64::new(0) }; 15];
/// Submission cost, QPC ticks summed (the tail of each view's graph is a chain of submissions):
/// [0] calls, [1] lists, [2] HookedExecuteCommandLists before it calls on, [3] this hook before the
/// driver call, [4] the driver call, [5] this hook after it. Hooks/DepthCapture.cpp writes [2].
#[no_mangle]
pub static CyberpunkVR_EclTiming: [AtomicU64; 8] = [const { AtomicU64::new(0) }; 8];
#[no_mangle]
pub static mut CyberpunkVR_DebugPsoGfx: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPsoStream: u64 = 0;
/// The replacement is loaded from a FILE rather than baked into the DLL. Iterating on it then
/// costs a dxc run and a game restart instead of a full rebuild, and the shader stays readable
/// next to the binary it patches.
#[no_mangle]
pub static mut CyberpunkVR_SightPsSwap: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_DebugSightSwaps: u64 = 0;
/// Measured: this pixel shader is used by TWO pipelines and only one of them draws the reticle.
/// Keying the swap on the PAIR (pixel AND vertex hash) picks that one exactly, instead of relying
/// on the other pipeline failing to build. Game 2.5: B99868EB53FB5975 (2.31: 9228439BF72D91DB), the
/// vertex shader of the same two technique records as the pixel shader (sight_reflex_vs.hlsl says
/// what changed in it).
#[no_mangle]
pub static mut CyberpunkVR_SightVsHash: u64 = 13373554453412272501;
/// MAGNIFIED SCOPES KEEP THE RETICLE ON THE GLASS. The collimator build sizes its reticle by angle
/// (SIGHT_RETICLE_RELIEF), which is right for a reflex window and wrong for a scope: a scope's
/// reticle is authored to fill its eyepiece, and at the angular size it shrank to a third at 5 cm.
/// The reticle panels of both kinds are the game's own materials drawn by this one pipeline, so the
/// only thing that tells them apart is the optical path the optics mod chose for the weapon in hand
/// (SetVRPhysicalOpticsPath). Every sight pipeline gets a twin built with the scope variant of the
/// pixel shader (CyberpunkVR_SightPsScope.dxil); SetPipelineState binds the twin while the path is
/// 'shader'. Missing file = no twins, collimator mapping for every sight as before.
/// kSightVariants holds one file today; each original PSO owns its path variants.
/// 0 none 1 collimator 2 shader 3 camera 4 native
#[no_mangle]
pub static mut CyberpunkVR_OpticsPath: i32 = 0;
/// 0 = always the collimator build
#[no_mangle]
pub static mut CyberpunkVR_SightScopeTwin: i32 = 1;
/// diagnostics: -1 by path, 0 collimator, 1 scope
#[no_mangle]
pub static mut CyberpunkVR_SightTwinForce: i32 = -1;
#[no_mangle]
pub static mut CyberpunkVR_SightScopeTwins: u64 = 0;
/// ---- the OUTLINE shader ------------------------------------------------------------------------
///
/// VisionMode_Highlight, the compute pass that draws the scanner/focus outline. Replaced so the line
/// can be made thicker: its width is nine taps at radius one (the shader's own `c_offsets`), and that
/// number exists nowhere else -- not in the scripts, whose whole surface is two colour indices and a
/// few flags, and not in any archive, which holds no outline resource at all.
///
/// IDENTIFIED BY THE CONTAINER DIGEST, not by an FNV of the bytes: every DXBC/DXIL blob carries its
/// own digest at offset 4, and the capture reports it, so the target is known before the game has ever
/// run. That is the whole reason this needs no discovery session the way the sight shader did.
/// ON, with a self-validating guard rather than a promise: the pipeline is built against the root
/// signature the engine has ALREADY bound for this dispatch, so a node that is not the outline pass
/// fails creation (its signature does not fit our shader's bindings) and nothing is swapped.
#[no_mangle]
pub static mut CyberpunkVR_VisionCsSwap: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_DebugVisionCsSwaps: u64 = 0;
