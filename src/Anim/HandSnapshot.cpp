// HandSnapshot -- reading the shared block the CET side publishes, and answering "where is the
// hand" without re-deriving it.
//
// This is the input end of the pose path: a seqlock read of the shared hand block, the latched view
// packet, and the guarded probe every reader uses before dereferencing an engine pointer. Nothing
// here decides a bone; it decides what the solve is allowed to believe.

#include "Anim/VrikHook.hpp"
#include "Anim/CharacterRig.hpp"
#include "Core/Paths.hpp"

// The VRIK pose path. Was include/Anim/VrikHook.hpp -- a header carrying 4,400 lines of
// implementation, included by exactly one translation unit, which is the only reason its file-scope
// objects linked at all.
//
// The shared ABI moved to Anim/VrikState.hpp. Everything below was never anybody's business but
// this file's; it was reachable only because a header cannot keep a secret.

#include "Anim/VrikHook.hpp"

// NOTE ON THE FOURTEEN DEFINITIONS BELOW THAT ARE NOT `inline`.
//
// They are declared `extern` in Anim/VrikState.hpp because the natives reach them. Leaving them
// `inline` here made MSVC treat the line as a DECLARATION matching that extern rather than a
// definition, so nothing was emitted and the link failed with twenty-two unresolved externals
// naming a file that looked correct. An extern declaration plus an inline definition is not a
// definition; plain is.


// ------------------------------------------------------------------------------------------------
// FILE-SCOPE OBJECTS IN THIS HEADER ARE `inline`, NOT `static`, AND THAT IS LOAD-BEARING.
//
// This header carries its implementation and is included by exactly one translation unit today, so
// `static` worked. It stops working the moment a second file includes it -- and splitting VRIK into
// Anim/ units is exactly that. With `static`, every including TU gets its OWN copy: the pose hook
// writes one, the solver reads another, and the failure is silent. Bone measurements come back 0,
// the hand stop never fires, a held object sits at the origin, and nothing is logged because
// nothing went wrong from the compiler's point of view.
//
// Worse for the trampoline originals (OrigWaXhUpd and its kin): a second copy is null, so the
// detour calls through a null pointer the first time that path runs.
//
// `inline` gives one object across all TUs, which is what the code has always assumed.
// ------------------------------------------------------------------------------------------------

#pragma once
#include <windows.h>
#include <cstdint>
#include <cstdio>
#include <cstdarg>
#include <share.h>
#include <iostream>
#include <cmath>
#include <string>
#include <fstream>
#include <iomanip>
#include <atomic>
#include <MinHook.h>
#include "Anim/VrikSolver.hpp"


// Page-readable guard: returns true only if [p, p+n) is committed and readable.
// Used by main.cpp diagnostics that dereference component pointers; __try alone
// is unreliable there, so we pre-validate the page protection.
bool VRIK_IsReadable(const void* p, size_t n) {
    if (!p) return false;
    MEMORY_BASIC_INFORMATION mbi;
    if (VirtualQuery(p, &mbi, sizeof(mbi)) == 0) return false;
    if (mbi.State != MEM_COMMIT) return false;
    DWORD prot = mbi.Protect & 0xFF;
    if (prot == PAGE_NOACCESS || prot == 0) return false;
    if (mbi.Protect & PAGE_GUARD) return false;
    uintptr_t start = reinterpret_cast<uintptr_t>(mbi.BaseAddress);
    uintptr_t end   = start + mbi.RegionSize;
    uintptr_t a     = reinterpret_cast<uintptr_t>(p);
    return (a + n) <= end;
}

// ---------------------------------------------------------------------------
// Pose-apply hook on the player's live track buffer copy function. This function copies the
// evaluated pose into the destination skeleton:
//   a2[7][0] = bone transform buffer (48 bytes/bone == QsTransform:
//              Translation@+0, Rotation(x,y,z,w)@+16, Scale@+32 -- authoritative
//              from RED4ext generated QsTransform.hpp; the old comment had it
//              inverted, which is what made the floating hand misbehave)
//   a2[7][3] = track value buffer (used to identify the player)
// The buffer holds PARENT-LOCAL transforms, so a hand can't be placed by writing
// its translation directly -- VRIK_SolveArm does model-space FK + 2-bone IK and
// writes only LOCAL ROTATIONS (no translation writes => no skin stretch).
// We run AFTER the original, so writing hand bones here survives graph eval.
// THE WEAPON'S RIG. Same identification as the player's, on a different skeleton: a weapon in hand has its own
// animated component, its own pose apply through this hook, and its own track buffer. Armed from script, because
// getting the active weapon's entity is one call there and a walk through the equipment system here.
//
// The parts are read for now (g_WeaponPartHave). Writing them is the same operation the cigarette already does on
// the player's rig -- a full local T + R on a bone that carries no skin -- so a proven one; see patch notes.
// SMALL-RIG WATCH: a rig is identified by HOW MANY BONES its pass remaps -- exact and structural, where the
// census below was statistical and saturated twice. counts[a4] from the pose function gives it directly; the
// Silverhand's rig assets say 16 bones for the frame and 5 for the magazine. Characters copy hundreds, so this
// table stays small by construction.

// EVERY (bones, tracks) PAIR SEEN, so the weapon's rigs are read off a list instead of predicted. Predicting
// (16, 0) and (5, 2) from the rig assets matched nothing at all.

// THE WEAPON'S TWO RIGS, nominated by the (bones, tracks) pair the pose descriptor carries and confirmed by a
// visible write. Index 0 is the magazine rig, 1 is the frame rig. See the patch notes for the two hypotheses
// that died getting here -- a4 is a level of detail, not a rig selector, and a1[8]+0x30 is not a rig path hash.
// The bone's UNMODIFIED local translation, captured the FIRST time the hook sees this bone -- before any write,
// so it is the real rest value. The write SETS base + offset, never adds: adding accumulated without bound
// (0.123 -> 0.711 -> 3.002 m, the pistol flew out of the hands) because these bones are not in the pass's remap
// table, so nothing re-establishes them. Capturing on first sight (not "while the offset is zero") is the fix
// for a slot created with a non-zero offset: the old latch never fired for it, HaveBase stayed 0, and the write
// was skipped forever -- which is exactly why the slide test produced no movement at all.
// Rotation offset per slot, for parts that spin (rotator disc, hammer). angle in degrees about the local axis;
// 0 = no rotation write. Base quaternion is latched on first sight, like the translation base.
// Uniform SCALE write per slot (<= 0 = off). The FRAME rig carries no tracks at all (its trackNames is empty), so
// the round meshes have no showMagazine-style switch to hide them -- but the QsTransform's scale at +32 can shrink a
// bone's mesh to nothing, which is what the eject/chamber logic uses to make the extracted round vanish. The
// magazine rig DOES have tracks, and it uses them instead (see g_RigTrackSet).
// ABSOLUTE writes, for REPLAYING recorded motion. Normally a write is base + offset, which is right for driving a
// part by hand; but a path lifted out of a recording already holds the game's own local values, so it must land
// verbatim. `Abs` makes the translation absolute, `QuatOn` writes the rotation outright (the game rotates a carried
// magazine by up to 180 deg -- a translation-only drive could not follow that).
// 0 = this slot is dormant and the animation owns the bone again (see VRRigWriteOff). Any write re-arms it.
// THE PINNED SLOT, resolved ONLY on an a4==0 pass. The dst numbering is per-LOD: a low-LOD remap resolved bone 6
// to a slot whose rest read (0,40,101)mm -- not back_slider's (0,30,222) -- so writes and the base latch landed on
// a FOREIGN bone whenever the write trusted whichever pass came along ("slide dead" while applied kept counting).
// a4==0 carries the full table (measured: counts[0]=15, counts[3]=2), so its resolve is the authoritative one;
// passes that resolve differently are skipped, and passes whose remap dropped the bone write the pinned slot
// anyway -- safe, because writer 3 (RVA 0x1B4610) restores LOCAL values after each composition, so between passes
// the slot always holds local-stage data.
// READ-BACK of every bone of the weapon rigs, as the game leaves it: parent-local translation (0..2) and rotation
// quaternion (3..6) per bone, refreshed on each pose pass BEFORE our own writes. This is the only way script can
// see what the game does to a weapon's parts -- the vrp_ slots do not ride the bones (the magazine slot is really
// the WELL), and a skinned mesh component's GetLocalToWorld is just the weapon entity's transform, identical for
// every component. Without it the recorder could not capture the native magazine motion at all.
// FLOAT TRACKS -- the GAME'S OWN visibility switch, and the right way to hide a magazine. A rig carries named float
// tracks beside its bones, and a mesh component's `visibilityAnimationParam` names the one that shows or hides it.
// The magazine rig has exactly two: `showMagazine` (index 0, what the visible mag_std mesh listens to) and
// `showMagazineReload` (1), with referenceTracks = [1, 0] -- seated magazine shown, carried one hidden. Read out of
// w_handgun__malorian_silverhand__mag_std.rig, so the index and the polarity are documented facts, not guesses. The
// frame rig's trackNames is EMPTY, which is why an ejected round still needs the scale trick above.
//
// The values live in the very buffer this hook already uses to tell one rig from another -- poseDesc[3]. Read-back
// lands in g_RigTrackVal, so script can see what the game does with them (and prove the buffer is the right one:
// with the magazine in place they must read 1 and 0). A write is g_RigTrackSet gated by g_RigTrackOn, applied every
// pass exactly like a bone write and released by clearing On.
//
// Why this beats the scale route it replaces: hiding the magazine here touches NO BONE. The scale trick had to
// register a bone write, and holding that bone every frame PINNED the game's magazine animation -- a native reload
// then ejected nothing at all (caught immediately in the headset).
// RIG SIGNATURES, so a weapon is recognised from DATA instead of from code. A pass belongs to a rig when the rig's
// bone COUNT matches and the named bones sit at the expected indices -- the same test that was hard-coded for the
// Silverhand (5 bones with mag_std/mag_stdr, 16 with front_slider/back_slider/mag_slot), now a table any weapon's
// config can fill through VRRigSignature. Names arrive as strings and are hashed here with FNV1a64, which IS the
// CName hash: verified against all five of the Silverhand's hard-coded constants, exact.
//
// Slot 0 of a signature is the rig index it claims (0 = the magazine rig, 1 = the frame rig), so a weapon whose
// magazine rig has a different bone count than five simply registers its own numbers.
// 16, not 8. Signatures are per RIG, not per weapon, and two rigs can share a bone count -- so the table grows with
// every distinct rig the set covers (six by the Lexington: three frame rigs of 16/11/10 bones, the Lexington's own
// 11, the per-weapon 5-bone magazine rig and the shared one). A full table returns -1 from VRRigSignature rather
// than failing silently, which is the failure mode fixed-size tables have had four times in this project.

// 16, up from 8. Eight covered every magazine rig in the set -- they carry three tracks at most -- and it stopped
// covering them at the first REVOLVER: the Malorian Overture's speedloader has NINE, six `bulletUsed01..06` then
// `showMagazineReload`, `showMagazineReloadBullets` and `showMagazineBullets`. The one that shows the rounds in the
// cylinder is index 8, exactly one past the end, so it could be neither read nor written -- and the rounds stayed
// visible with nothing in the log to say why. `VRRigTrack` returned its -1 sentinel for 8, 9 and 10, which is what
// finally named it. Third fixed-size table in this project to run out without a word.
// Per-a4 pass telemetry for the FRAME rig (which==1): pass count, remap entry count, and where that pass's remap
// puts write slot 0's bone (-1 = dropped). Read through VRRigWriteDiag fields 20..31.
// m. Parts of the gun itself never travel a tenth of this, but an EJECTED round flies free of the weapon (the
// eject arc is a real ballistic throw), and clamping that at 0.20 chopped the flight into a stub.

// A CENSUS OF POSE PASSES. Every distinct (trackBuf, a4) the hook sees that is not the player's, with a call
// count. Recording arguments is safe in a way that walking a component's memory was not, and a4 turns out to be
// the rig selector, so the weapon's rigs show up here as entries that appear when the gun is drawn.
// 24 was not enough and it failed the way fixed tables in this project always fail: it filled with NPC rigs
// before a weapon was ever drawn and then silently accepted nothing new. The count is reported now, and
// g_PoseCensusFull says so outright.
// Was a weapon in hand when this entry was FIRST seen? That one bit removes the need for any before/after
// diffing protocol: the weapon's rigs are exactly the entries born while a weapon was out.
// Hits split by whether a weapon was in hand at the time. "Born with a weapon out" turned out to be far too weak
// a test -- seven skeletons carried the same five-mapping signature, because every NPC that spawned during the
// walk got stamped too. A weapon's rigs stop being updated the moment it is holstered, and an NPC's do not, so
// the asymmetry between these two counters is the discriminator that cannot be confused with anything.

// The write test. Added to a part bone's parent-local translation on every pass of the weapon's rig. Adding is
// idempotent per pass because the copy above re-establishes the base value from the source each time -- the same
// reason the finger replay works from this hook.



// VR hand binding: write VR controller pose into the hand bones each frame.

std::string VRDiagPath(const char* name);  // defined in main.cpp

// ===== SEQLOCK READER (torn-read fix) =====
// dxgi writes the pose block [0..93] from its present thread while THIS hook reads
// it on the engine's animation thread every frame to solve full-body IK. With no
// shared lock, a half-written quaternion made the whole body jitter (even at rest).
// dxgi brackets each write with an ODD/EVEN sequence counter in slot [127]. We latch
// a WHOLE consistent frame into g_handsStable (retry while odd/changed) and only swap
// it in atomically when a new complete frame arrives, so every pose value the IK
// pulls in one solve comes from ONE frame. SharedPose(i) returns the latched value.
// Hook WRITES (slots [20..22],[85..88]) stay on raw g_pSharedHands (disjoint).
inline float    g_handsStable[128] = {};
uint32_t g_handsStableSeq   = 0;
bool     g_handsStableValid = false;

void RefreshHandsSnapshot() {
    if (!g_pSharedHands) return;
    volatile uint32_t* seqSlot = reinterpret_cast<volatile uint32_t*>(&g_pSharedHands[127]);
    // PERF fast path: the hook calls this once per player pose pass (4-5x per tick);
    // if the writer seq hasn't moved since the last latch there is nothing new to
    // copy (the seq-equal guard below would discard the copy anyway -- including
    // keeping the snap-event packet rotation patched into g_handsStable[104..109]).
    {
        const uint32_t sCur = *seqSlot;
        if (g_handsStableValid && !(sCur & 1u) && sCur == g_handsStableSeq) return;
    }
    for (int tries = 0; tries < 8; ++tries) {
        const uint32_t s0 = *seqSlot;
        if (s0 & 1u) continue;                 // write in progress -> retry
        std::atomic_thread_fence(std::memory_order_acquire);
        // Copy [0..126]: the seqlock brackets [0..93], but dxgi also publishes
        // [104..111] (render-view pose), [116..123] (eye/anchor offsets) and
        // [124..126] (HMD base pos) from the SAME present thread. The old 94-float
        // copy silently returned 0 for all of those (SharedPose(111) never saw the
        // view-pose flag -> hands were stuck on the frozen-baseline fallback and
        // ignored real head translation: stand up -> hands stay at sitting height).
        float tmp[127];
        for (int i = 0; i < 127; ++i) tmp[i] = g_pSharedHands[i];
        std::atomic_thread_fence(std::memory_order_acquire);
        const uint32_t s1 = *seqSlot;
        if (s0 == s1) {                        // consistent (no write straddled the copy)
            if (!g_handsStableValid || s1 != g_handsStableSeq) {
                for (int i = 0; i < 127; ++i) g_handsStable[i] = tmp[i];
                g_handsStableSeq   = s1;
                g_handsStableValid = true;
            }
            return;
        }
    }
    // Contended out: keep the last good snapshot (never expose a torn frame).
}

// An unfinished first publication is no more readable than any later one.
float SharedPose(int i) {
    if (g_handsStableValid) return g_handsStable[i];
    return 0.0f;
}

void VRIK_HeadResidual(float fallbackConsumedRight,float fallbackConsumedForward,float out[3]) {
    const bool paired=SharedPose(115)==2.0f;
    out[0]=SharedPose(124)-(paired ? SharedPose(112) : fallbackConsumedRight);
    out[1]=SharedPose(125);
    out[2]=SharedPose(126)+(paired ? SharedPose(113) : fallbackConsumedForward);
}
// Constant per-hand wrist-orientation correction (hand-local), set live via
// SetVRHandOffset(pitch,yaw,roll,hand). Applied as handRot = mapQuat * wristCorr.
// Defaults are the calibrated values: right = euler(0,-90,0), left = euler(-180,-90,0).
// Per-hand reach scale + position offset (mode 4). Different arm lengths/heights per player.
// Per-hand elbow pole spin (degrees): fine rotation of the bend normal around the
// shoulder->hand axis, to nudge the elbow more outward/inward. 0 = natural.

// SMOKE FINGER-HOLD (fingers-only "hold cigarette" grip). The buffer holds PARENT-LOCAL
// transforms and VRIK never touches the finger bones (it owns arm + wrist only), so a
// captured finger-local rotation, replayed each pass, curls the fingers relative to the
// controller-driven wrist -- a native grip WITHOUT a full-body workspot. The curl is
// captured live from the vanilla player hold-cigarette workspot (played once via AMM):
// VRSmokeCaptureFingers() latches the current finger locals; SetVRSmokeFingers(1) then
// replays them every pass. Right-hand deform finger + metacarpal bones only.
// Cigarette slot = WeaponRight bone: captured/applied with FULL local transform (+ live nudge).
// Model-space distance cig-slot(28) -> mouth (head bone + view offset), recomputed each body solve.
// Same skeletal frame as the grip, so it tracks BOTH the HMD (head) and the controller (cig hand) --
// unlike the redscript FPP camera, whose translation ignores HMD positional/lean tracking. 999 = n/a.
// Mouth anchor (hands-free): pin the cig to the head. See main.cpp for semantics.
// General mouth-pin (arbitrary bone): sel 0=off,1=Neck1,2=Head,3=Neck; resolved idx; alt local.
// Exhale smoke world pose (view pose composed with the smoke's own HMD-local offset).
// The rendered view pose itself, in world space, published raw for scripts. The mouth point above
// is this composed with one fixed offset; anything else that needs to place a world object at a
// tracked point (the VR basketball grip) needs the pose, not somebody else's offset baked in.
// Palm centre (RightHandMiddle1 / LeftHandMiddle1) from the SOLVED avatar, model space, plus the
// model-space camera it is measured against. VRPalmWorldPos composes these with the view pose.
// Skeleton for the ball's own body collision, model space. The avatar has no physics
// representation of any kind, and the player's one authored collider is a single capsule, so a
// ball can only meet a body shape if that shape is built from these.
//   0 hips  1 spine  2 chest  3 neck  4 head
//   5 L thigh  6 L knee  7 L foot   8 R thigh  9 R knee  10 R foot
// Published where the FK is final for the spine and legs -- see the BODY PUBLISH comment. The arms
// are deliberately absent: VRIK_SolveArm runs after this point, so their FK here is the animation
// pose, not the VR pose.
// LEFT-HAND mirror (lighter): left fingers + WeaponLeft slot.
// LEFT-HAND cigarette grip (separate from the lighter); g_VRSmokeLeftUseCig selects it.

// RELOAD FINGER POSE. Poses the FREE hand's fingers into a grip while it holds a weapon part (slide/mag). Reuses
// the finger bone indices the smoke resolver already found (g_VRSmokeFingerIdx = right hand, IdxL = left), keyed by
// hand (0 = left, 1 = right). Per finger a target parent-local quaternion + a set flag; the hook writes the set
// ones when the hand is active. Fed from the game's own reload anim, converted GLB->runtime by (x,-z,y,w).

// RELOAD RECORDER FK. While on, the hook computes the whole-skeleton model-space FK from the player pass's
// ANIMATED locals and publishes it to the VRBoneModelPos/Rot snapshot. The normal snapshot fill lives inside the
// VRIK solve (diag-gated), which never runs with VRIK off -- so recording the game's own reload animation (VRIK
// off is the only way it plays on the arms) returned a frozen skeleton: both first recordings measured |v|==0 on
// every wrist frame. Toggled by the Lua recorder around a take; costs one 620-bone FK per player pass, so it must
// never stay on outside a recording.

// FPP camera (HMD) + player entity world position (pushed from Lua) -> used to place the IK
// hand target in MODEL space EXACTLY where the visible gizmo is (gizmo-exact 1:1). When
// g_VRCamPosValid is 0 (legacy 5-param SetVRPlayerYaw) the IK falls back to the head-relative path.
// Player entity world orientation quaternion (i,j,k,r). world->model = conjugate(this).
// THE single stabilized camera-local offset (cam - entity, world axes), filtered once
// in SetVRTransforms from the coherent same-push pair; also fed to the render view via
// shared [124..127] so skeleton and view consume the identical value (see main.cpp).

// Render-view position from dxgi ([108..110]) with FIXED-POINT SCALE AUTO-DETECT.
// The LocateCamera rbx+0 buffer published there landed at EXACTLY 2x the render
// camera on at least one build (17 fractional bits or a prev+cur sum -- unclear),
// which threw hand targets kilometres out (both arms clamped to one side). Sanity:
// accept [108..110] only if it (or its half) lands within 2m of the known-good
// render camera position; otherwise report failure so the caller falls back to
// the legacy composition instead of solving toward garbage.
// VIEW PACKET (audit fix): [104..111] + [141] latched ONCE per solve under the dxgi
// seqlock [143], so BOTH arms and the view-pos resolver consume ONE render frame.
// Mixing a latched vq (previous frame) with a directly-read fresh yaw produced the
// snap-turn arm double; per-arm direct reads produced the left-only head-turn ghost.
float g_viewPkt[17] = { 0,0,0,1, 0,0,0, 0, 0, 0, 0,0,0, 0,0,0,1 }; // +[13..16]=head ori the view was built with
bool  g_viewPktValid = false;

// Shared snap-window trace writer (log\cyberpunkvr_snapwin.log). Used by the
// packet-latch [hk] lines AND the puppet pre-rotation [pr] lines -- one file, one
// session-truncating open, chronological.
inline void VRIK_SnapTraceLog(const char* fmt, ...) {
    static FILE* s_tf = nullptr;
    if (!s_tf) {
        s_tf = _wfsopen(cvr::paths::File(cvr::paths::Folder::Log, L"cyberpunkvr_snapwin.log").c_str(), L"w", _SH_DENYNO);
    }
    if (!s_tf) return;
    va_list args;
    va_start(args, fmt);
    vfprintf(s_tf, fmt, args);
    va_end(args);
    fflush(s_tf);
}

void VRIK_LatchViewPacket() {
    g_viewPktValid = false;
    if (!g_pSharedHands) return;
    volatile uint32_t* seq = reinterpret_cast<volatile uint32_t*>(
        const_cast<float*>(&g_pSharedHands[143]));
    for (int tries = 0; tries < 8; ++tries) {
        const uint32_t s0 = *seq;
        if (s0 == 0u) return;                    // writer absent (older dxgi)
        if (s0 & 1u) continue;                   // write in progress
        float tmp[17];
        tmp[0] = g_pSharedHands[104]; tmp[1] = g_pSharedHands[105];
        tmp[2] = g_pSharedHands[106]; tmp[3] = g_pSharedHands[107];
        tmp[4] = g_pSharedHands[108]; tmp[5] = g_pSharedHands[109];
        tmp[6] = g_pSharedHands[110]; tmp[7] = g_pSharedHands[111];
        tmp[8] = g_pSharedHands[141];
        tmp[9]  = g_pSharedHands[68];          // publish stamp, for the age census
        tmp[10] = g_pSharedHands[218];         // the position the frame renders from
        tmp[11] = g_pSharedHands[219];
        tmp[12] = g_pSharedHands[220];
        tmp[13] = g_pSharedHands[227]; tmp[14] = g_pSharedHands[228];
        tmp[15] = g_pSharedHands[229]; tmp[16] = g_pSharedHands[230];
        const float ok142 = g_pSharedHands[142];
        if (*seq == s0) {
            for (int k = 0; k < 17; ++k) g_viewPkt[k] = tmp[k];
            g_viewPktValid = (ok142 == 1.0f);
            // SNAP EVENT SYNC (trace-driven; replaces both entity/camera comparators --
            // snap_trace PROVED the puppet yaw sits up to ~10deg off the heading
            // PERMANENTLY (turn-in-place deadband), so any comparator fires every tick
            // and skews the hand frame constantly). dxgi's deltaHead hook publishes at
            // the INJECTION moment (tick stage): [146] = snap yaw delta (radians),
            // [148] = the PRE-snap heading, [147] = event counter. We rotate the
            // just-latched (one-locate-old, hence pre-snap) packet by the delta ONCE,
            // so the snap-tick solve matches the heading the next locate renders
            // (trace: inject at hits=N -> view turned at N+1). The [148] guard skips
            // the rotation if the packet ALREADY shows the post-snap heading (solve
            // ordering variant) -- no double-apply possible.
            if (g_viewPktValid && g_pSharedHands) {
                static float s_lastSnapCtr = -1.0f;
                static int   s_snapTraceWin = 0;   // [snap-win] diag: solves left to trace
                const float ctr = g_pSharedHands[147];
                if (s_lastSnapCtr < 0.0f) { s_lastSnapCtr = ctr; g_pSharedHands[149] = ctr; } // startup: skip history
                // (One-tick DEFER experiment REVERTED: deferring the packet rotation +
                // holding the view put the ghost on STANDING snaps too and made it more
                // visible -- proving the baseline view/arms pairing was already correct
                // and the sprint-only laggard is the PUPPET world transform alone.)
                if (ctr != s_lastSnapCtr) {
                    s_lastSnapCtr = ctr;
                    s_snapTraceWin = 14;
                    // ACK for dxgi's snap HOLDBACK ([149] = last event this solve consumed).
                    // Written for BOTH the rotate and the guard-skip outcome — either way THIS
                    // tick's solve is heading-consistent with the event, the view may turn.
                    // (Second life of the holdback: on the CLEAN baseline the standing solve
                    // provably sees the event same-tick — ack==ctr by locate time, holdback
                    // never arms. During SPRINT the tick scheduling flips to ordering-B
                    // (solve BEFORE DeltaHead) and the mirror-visible one-frame ghost appears
                    // — the exact case the holdback closes. Its first test was polluted by
                    // the since-reverted re-yaw fixes flashing on their own.)
                    g_pSharedHands[149] = ctr;
                    const float d = g_pSharedHands[146];
                    float pre = g_viewPkt[8] - g_pSharedHands[148];
                    while (pre >  3.14159265f) pre -= 6.28318531f;
                    while (pre < -3.14159265f) pre += 6.28318531f;
                    const bool pktIsPreSnap = (pre > -0.035f && pre < 0.035f); // ~2deg
                    if ((d > 1e-4f || d < -1e-4f) && pktIsPreSnap) {
                        const float s = std::sin(d * 0.5f);
                        const float c = std::cos(d * 0.5f);
                        const float x = g_viewPkt[0], y = g_viewPkt[1],
                                    z = g_viewPkt[2], w = g_viewPkt[3];
                        // Rz(d) * vq, expanded (unit * unit = unit).
                        const float nx = c * x - s * y;
                        const float ny = c * y + s * x;
                        const float nz = c * z + s * w;
                        const float nw = c * w - s * z;
                        g_viewPkt[0] = nx; g_viewPkt[1] = ny;
                        g_viewPkt[2] = nz; g_viewPkt[3] = nw;
                        g_viewPkt[8] += d;
                        // Heading-rotated translation delta (bakes up to ~0.35m) must
                        // turn too, or the anchor swings by bake*sin(snap) for a frame.
                        const float vs = std::sin(d);
                        const float vc = std::cos(d);
                        const float dx = g_viewPkt[4], dy = g_viewPkt[5];
                        const float rdx = vc * dx - vs * dy;
                        const float rdy = vs * dx + vc * dy;
                        g_viewPkt[4] = rdx;
                        g_viewPkt[5] = rdy;
                        if (g_handsStableValid) {
                            g_handsStable[104] = nx;  g_handsStable[105] = ny;
                            g_handsStable[106] = nz;  g_handsStable[107] = nw;
                            g_handsStable[108] = rdx; g_handsStable[109] = rdy;
                        }
                    }
                }
                // SPRINT-DOUBLE TRACE (temporary diag). For ~14 solves after each snap
                // event log what THIS solve actually consumes: packet yaw (post-rotate)
                // vs the entity world yaw g_VREntityQ* -- the world->model base of the
                // full-arm IK. The rendered skeleton = entity world transform x model
                // pose, so if entYaw steps LATE (or ramps) in sprint while pktYaw steps
                // at the event, the whole body+hands render one+ frames at the old
                // world orientation = the mirror ghost. pos(x,y) -> speed tells sprint
                // from standing when reading the log offline.
                if (s_snapTraceWin > 0) {
                    --s_snapTraceWin;
                    const float qi = g_VREntityQI, qj = g_VREntityQJ,
                                qk = g_VREntityQK, qr = g_VREntityQR;
                    const float entYaw = std::atan2(2.0f * (qr * qk + qi * qj),
                                                    1.0f - 2.0f * (qj * qj + qk * qk));
                    VRIK_SnapTraceLog("[hk] ms=%llu ctr=%.0f pktYaw=%.4f entYaw=%.4f plYaw=%.2f pos=(%.2f,%.2f)\n",
                                      (unsigned long long)GetTickCount64(), (double)ctr,
                                      (double)g_viewPkt[8], (double)entYaw, (double)g_VRPlayerYaw,
                                      (double)g_VREntityPosX, (double)g_VREntityPosY);
                }
            }
            return;
        }
    }
}

float g_vrikViewScaleUsed = 0.0f;   // diag: 0=rejected/fallback, 2=delta-v2, 1/0.5=legacy abs
bool VRIK_ResolveViewPos(float out[3]) {
    g_vrikViewScaleUsed = 0.0f;
    // Prefer the per-solve latched view PACKET (one seqlocked dxgi frame shared by
    // both arms and this resolver); snapshot fallback for an older dxgi build.
    const float flag = g_viewPktValid ? g_viewPkt[7] : SharedPose(111);
    if (flag == 0.0f) return false;
    if (!g_VRCamPosValid) return false;
    // Prefer the delta dxgi built from the SAME head sample as the hand offsets ([112..114],
    // inside the hands seqlock). The render packet's delta is a different, older sample: measured
    // 33 ms against the hands' 21 ms, and a hand reconstructed from two different instants lands
    // in the wrong world place by the head motion between them. Falls back to the packet when
    // dxgi does not publish it (older build, or the switch is off).
    const bool coherent = (SharedPose(115) == 1.0f);
    const float v[3] = {
        coherent ? SharedPose(112) : (g_viewPktValid ? g_viewPkt[4] : SharedPose(108)),
        coherent ? SharedPose(113) : (g_viewPktValid ? g_viewPkt[5] : SharedPose(109)),
        coherent ? SharedPose(114) : (g_viewPktValid ? g_viewPkt[6] : SharedPose(110)) };    if (flag == 2.0f) {
        // v2: v = float-exact translation DELTA (head + sliders + bakes; slow values)
        // added onto the COHERENT camera = entity + same-push (cam - entity). Both parts
        // of the pair come from ONE Lua push, so entity_N + local_N == cam_N exactly --
        // hands ride the camera 1:1 like the body does, and no two fast absolutes
        // sampled on different ticks are ever mixed (that mixing caused the strafe
        // frame-skipping; the local difference is cm-scale and tear-safe).
        if (v[0]*v[0] + v[1]*v[1] + v[2]*v[2] > 2.25f) return false;   // sanity: |delta| < 1.5m
        float bx = g_VRCamPosX, by = g_VRCamPosY, bz = g_VRCamPosZ;    // fallback: raw cam
        if (g_VRCamPairValid) {
            bx = g_VREntityPosX + g_VRCamPairLocalX;
            by = g_VREntityPosY + g_VRCamPairLocalY;
            bz = g_VREntityPosZ + g_VRCamPairLocalZ;
        }
        out[0] = bx + v[0];
        out[1] = by + v[1];
        out[2] = bz + v[2];
        g_vrikViewScaleUsed = 2.0f;
        return true;
    }
    // Legacy (flag==1): absolute position with fixed-point scale auto-detect (old dxgi).
    for (int s = 0; s < 2; ++s) {
        const float k = (s == 0) ? 1.0f : 0.5f;
        const float dx = v[0]*k - g_VRCamPosX;
        const float dy = v[1]*k - g_VRCamPosY;
        const float dz = v[2]*k - g_VRCamPosZ;
        if (dx*dx + dy*dy + dz*dz < 4.0f) {
            out[0] = v[0]*k; out[1] = v[1]*k; out[2] = v[2]*k;
            g_vrikViewScaleUsed = k;
            return true;
        }
    }
    return false;
}
// T-pose measured real arm length per hand (metres). 0 = unset -> arm-bone scaling disabled.
// Phase 2 body-under-HMD: place the chest (top of the spine) under the HMD so the upper body
// follows the headset instead of the game's animated pose. g_VRBodyUnderHMD gates it.
// The BODY's smoothed squat (deadzone+EMA), published by VRIK_PlaceBodyUnderHMD and reused by the
// ARM shoulder anchor so body + arms squat TOGETHER with no relative twitch on sprint/jump.
// Single-TU header (writer & reader both live here).
float s_vrSharedSquatDrop = 0.0f;

bool VRIK_ResolveViewModelPos(const float* pairedCamModel,const float* pairedEntityQuat,float out[3]) {
    const float flag=g_viewPktValid ? g_viewPkt[7] : SharedPose(111);
    if(flag!=2.0f)return false;
    const bool coherent=SharedPose(115)==1.0f;
    float delta[3];
    for(int k=0;k<3;++k)delta[k]=coherent ? SharedPose(112+k) :
        (g_viewPktValid ? g_viewPkt[4+k] : SharedPose(108+k));
    const float norm=VRIK_Dot3(delta,delta);
    if(!std::isfinite(norm) || norm>2.25f)return false;
    float inv[4],local[3];VRIK_QuatConj(pairedEntityQuat,inv);
    VRIK_QuatRotateVec(inv,delta,local);
    for(int k=0;k<3;++k)out[k]=pairedCamModel[k]+local[k];
    return true;
}
// Diagnostics for the body placement (model space), surfaced via LogVRDiag.
// Solve-side trace probes: hips MODEL-space yaw (detects locomotion root rotation
// leaking through the local-space hips lock) + right IK shoulder model position.
// Pose-capture generation: bumped by SetVRBindMode on (re)enable so the girdle/hips
// reference captures rerun (re-toggling VRIK while standing recalibrates).
// POST-WRITE TAMPER DETECTOR. After each solve we remember the right-hand bone's local
// translation; at the NEXT hook entry (pre-anim-rewrite... anim rewrites everything, so
// instead we count solve calls per entity tick and expose them). If the engine runs the
// player graph SEVERAL times per tick, our solve on an early pass can be partially
// overwritten by a later additive pass (strafe lean / turn-assist) that we never see.
// 1 = rotate the controller offset by the head orientation from ITS OWN sample instead of the
// view packet's. Live switch so the two can be compared by feel, not by argument.
// Clavicle-aim diag [side][8]: desired joint (0..2), FK joint after aim (3..5),
// aim angle needed (6, deg), applied after cap (7, deg). side 0=R, 1=L.

// Full-arm IK (g_VRBind == 4): hierarchy + chain indices resolved in VRIK_DoArmPlayer.
// 800, not 256: the smoking gesture resolves finger bones by name across the WHOLE metaRig, and
// the player rig runs past 256 entries. Grown alongside the definition in main.cpp -- the two must
// agree or the linker reports "redefinition; different subscripts".

// IK diagnostics (last solve, model space) -- surfaced via LogVRDiag.

// Maps a VR-space vector to model-space per the selected preset (VR is Y-up).
void VRIK_RemapAxis(int preset, const float* v, float* o) {
    switch (preset) {
        default:
        case 0: o[0] =  v[0]; o[1] =  v[1]; o[2] =  v[2]; break; // identity
        case 1: o[0] =  v[0]; o[1] = -v[2]; o[2] =  v[1]; break; // Y-up -> Z-up (Standard OpenXR)
        case 2: o[0] =  v[0]; o[1] =  v[2]; o[2] =  v[1]; break;
        case 3: o[0] = -v[0]; o[1] = -v[2]; o[2] =  v[1]; break;
        case 4: o[0] =  v[2]; o[1] =  v[0]; o[2] =  v[1]; break;
        case 5: o[0] = -v[2]; o[1] = -v[0]; o[2] =  v[1]; break;
    }
}
