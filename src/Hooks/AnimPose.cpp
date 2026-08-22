// AnimPose -- THE HOOK. The engine's animation pose-apply, detoured.
//
// This is the file the pose path exists for, and it belongs in Hooks/ with the other twenty-three:
// it was in Anim/ only because it started life as a header next to the maths it calls.
//
// IT IS NOT REGISTERED WITH THE HOOK REGISTRY, AND THAT IS DELIBERATE. Every hook in Hooks/ is
// installed by a stage at boot; this one is installed when SCRIPT asks, through the
// InstallVRAnimPoseHook native. Registering it with CVR_HOOK would install it at boot instead --
// a behaviour change wearing the clothes of a file move.
//
// THE WEAPON BONES ARE IN HERE, and they should not be. The reload path's weapon-rig writes sit at
// brace depth three to seven inside this detour's single __try, which spans about 1,980 lines. They
// cannot be lifted into src/Anim/WeaponRig.cpp by any brace-aware cut, because a cut that keeps the
// braces balanced still drops the enclosing conditions -- and a weapon-bone write that runs when it
// must not is a held object in the wrong place, not a compile error. Moving them means giving each
// extracted function its own __try (MSVC will not allow one in a function that also needs C++
// unwinding, so the extraction has to be written for that) and reproducing the conditions as
// parameters. That is its own change, with its own commit.

#include "Anim/VrikHook.hpp"
#include "Anim/CharacterRig.hpp"
#include "Anim/WeaponRig.hpp"
#include "Anim/FkRecorder.hpp"
#include "Anim/SmokingPose.hpp"
#include "Anim/ReloadPose.hpp"
#include "Hooks/Hook.hpp"
// The head displacement and the flag that decides where it is written. The body has to subtract it
// back out (see the call to VRIK_PlaceBodyUnderHMD), so the pose path does need to see these two.
#include "Camera/CameraState.hpp"
#include "Core/LiveControls.hpp"   // xrCutsceneSuspendTier, read straight out of the struct
#include "Anim/TwoHandGrip.hpp"
#include "Anim/AdsEyeAlign.hpp"
#include "Anim/AdsMuzzleStabilizer.hpp"
#include "Anim/HeadAimWeapon.hpp"
#include "Anim/WheelGrab.hpp"
// Raised by VRTwoHandCapture(); read by the early-out above, which must not skip a pass it needs.
extern "C" __declspec(dllexport) extern int CyberpunkVR_TwoHandCaptureReq;
// 1 while the support hand is welded to the weapon: it then rides the weapon's kick and must not be
// given a second one of its own.
extern "C" __declspec(dllexport) extern int CyberpunkVR_TwoHandActive;
// Hand recoil (src/Anim/Recoil.cpp): advanced here, sampled per arm below.
extern "C" void RecoilTick();
extern "C" void RecoilSample(int side, int weaponHand, float* outBackM, float* outRiseRad);
#include <MinHook.h>

// The [vrik*] log channel: Log lives in Core, the flag in OpenXRPresent.cpp. Declared here rather
// than pulled in through a header, because this file is the pose path and has no business seeing
// either module.
extern void Log(const char* fmt, ...);
extern "C" __declspec(dllexport) extern int CyberpunkVR_VrikBatchClock;
// The hunt's instruments ride this: sampling and printing both. The batch CLOCK does not.
extern "C" __declspec(dllexport) extern int CyberpunkVR_XrDeepDiag;
extern "C" __declspec(dllexport) extern float CyberpunkVR_VrikBatchGapMs;

// A millisecond clock for the pose path. The pose-apply detour runs a few hundred times a second and
// must not reach into another translation unit's statics for the time; this is the same QPC everything
// else in the tree measures with.
static double VrikNowMs() {
    static LARGE_INTEGER s_freq = {};
    if (s_freq.QuadPart == 0) QueryPerformanceFrequency(&s_freq);
    if (s_freq.QuadPart == 0) return 0.0;
    LARGE_INTEGER t{};
    QueryPerformanceCounter(&t);
    return static_cast<double>(t.QuadPart) * 1000.0 / static_cast<double>(s_freq.QuadPart);
}

// THE GAP HISTOGRAM, which is what makes the threshold a measurement instead of a guess. Passes inside
// one animation batch arrive microseconds apart; batches are a frame apart. If those two populations
// are ever seen to approach each other, the batch clock is unsafe and this says so first.
void VrikNoteBatchGap(double gapMs, bool countedAsNewBatch) {
    if (gapMs < 0.0 || gapMs > 1000.0) return;
    if (countedAsNewBatch) {
        ++g_VrikBatchGapNew;
        if (gapMs < g_VrikBatchGapNewMin || g_VrikBatchGapNewMin <= 0.0f) {
            g_VrikBatchGapNewMin = static_cast<float>(gapMs);
        }
    } else {
        ++g_VrikBatchGapSame;
        if (gapMs > g_VrikBatchGapSameMax) g_VrikBatchGapSameMax = static_cast<float>(gapMs);
    }
}


extern "C" __declspec(dllexport) extern int CyberpunkVR_VrikRateLog;


// The trampoline this detour calls through. The hook's own state.
typedef void* (*AnimPoseFunc_t)(void* a1, void* a2, void* a3, unsigned int a4);
namespace { AnimPoseFunc_t OriginalAnimPose = nullptr; }


extern "C" inline void* Hooked_AnimPoseApply(void* a1, void* a2, void* a3, unsigned int a4) {
    void* result = OriginalAnimPose(a1, a2, a3, a4);
    ++g_AnimPoseTotalCalls;

    // Hot-path early-out: this hook runs on EVERY skeleton's pose apply (all NPCs,
    // every frame). Do nothing unless the player is armed AND we actually have work.
    // No VirtualQuery here -- that syscall per call was the FPS killer. a2 is always
    // a valid pose-apply argument, so a single __try guards the dereferences.
    if (!(g_PlayerTrackBufA || g_PlayerTrackBufB)) return result;
    // ...AND A PENDING TWO-HAND CAPTURE, which is the one job here that exists BECAUSE VRIK is off.
    // Every other condition in this list is a reason the port is doing something; that capture needs the
    // opposite -- the engine's own animation on the arms -- so with VRIK off it fell through this early-out
    // and the request sat unanswered forever (measured: the flag stayed at 1). A gate written as "are we
    // busy" quietly excludes the work that only happens when we are not.
    // ...AND THE NON-VRIK ADS STABILIZER, which is the other job that exists BECAUSE VRIK is off:
    // it corrects the direction the vanilla aim-in animation gives the muzzle, so it has to run on
    // exactly the passes this early-out used to discard (dabinn, TofuExpress 797a2a95).
    // HEAD AIM owns the weapon's rotation and suspends VRIK while it does, so it needs these
    // passes as well -- and the moment it hands the weapon back, any cached VRIK solve belongs to the
    // previous owner and must not be replayed (dabinn, TofuExpress d002d314).
    const bool headAimWork = cvr::anim::IsHeadAimWeaponActive();
    {
        static bool s_prevHeadAim = false;
        if (headAimWork != s_prevHeadAim) {
            g_solveCacheTick = 0xFFFFFFFFu;   // the "nothing cached" value this tree uses
            g_solveCacheN = 0;
            s_prevHeadAim = headAimWork;
        }
    }
    const bool nonVrikAdsWork = g_pSharedHands && CyberpunkVR_NonVrikAdsStabilizer &&
        g_pSharedHands[vrshared::kWeaponFlag] > 0.5f;
    if (g_VRBind <= 0 && !headAimWork && !nonVrikAdsWork &&
        g_VRDiagCapture == 0 && g_WeaponRigActive == 0 &&
        g_PoseCensusOn == 0 && g_VRRecordFK == 0 && CyberpunkVR_TwoHandCaptureReq == 0 &&
        g_VRSmokeFingerActive == 0 && g_VRSmokeFingerCapture == 0 &&
        g_VRSmokeFingerActiveL == 0 && g_VRSmokeFingerCaptureL == 0) return result;

    __try {
        void* poseDesc = reinterpret_cast<void**>(a2)[7];
        if (poseDesc) {
            uint8_t*  boneBuf  = reinterpret_cast<uint8_t**>(poseDesc)[0];
            uintptr_t trackBuf = reinterpret_cast<uintptr_t*>(poseDesc)[3];
            // IDENTIFY BY BONE NAME. a1[8] is the rig object -- established in the debugger -- so the names it
            // carries say which rig this pass belongs to, with nothing inferred from sizes or frequencies.
            //
            // NO VirtualQuery ON THIS PATH. VRIK_IsReadable calls it, and three of those per pass on a function
            // the engine invokes ~10k times a second took the game to 4 fps -- exactly what the note at the top
            // of this hook warns about. The enclosing __try is the guard, as it is for every other read here.
            //
            // Known buffers short-circuit: once a rig has been identified its track buffer is remembered, so the
            // steady state is two pointer comparisons.
            if (boneBuf && trackBuf &&
                trackBuf != g_PlayerTrackBufA && trackBuf != g_PlayerTrackBufB) {
                cvr::anim::WeaponRigIdentifyAndWrite(reinterpret_cast<void**>(a1), reinterpret_cast<void**>(a2), a4,
                                                     poseDesc, boneBuf, trackBuf);
            }

            // CENSUS -- the abandoned statistical route, left gated off. It cost two dead ends and it
            // saturated twice; identification is by bone name above.
            // CENSUS. Bounded, linear, and it only ever stores arguments.
            if (g_PoseCensusOn && boneBuf && trackBuf &&
                trackBuf != g_PlayerTrackBufA && trackBuf != g_PlayerTrackBufB) {
                cvr::anim::WeaponRigCensusNote(a4, trackBuf);
            }

            // THE WEAPON'S PASS. A different skeleton, so a different track buffer; captured before the player
            // branch because the two are mutually exclusive and this one is cheap.
            if (g_WeaponRigActive && boneBuf && trackBuf &&
                (trackBuf == g_WeaponTrackBufA || trackBuf == g_WeaponTrackBufB)) {
                cvr::anim::WeaponRigCaptureParts(boneBuf);
            }
            if (boneBuf && trackBuf && (trackBuf == g_PlayerTrackBufA || trackBuf == g_PlayerTrackBufB)) {
                ++g_AnimPoseMatchCalls;
                g_AnimPoseLastBoneBuf = reinterpret_cast<uintptr_t>(boneBuf);

// The recorder publishes the engine's ANIMATED bones (VRIK off) so reload poses can be
// authored. A tool, in its own file -- see src/Anim/FkRecorder.cpp.
if (g_VRRecordFK) {
    cvr::anim::VrikPublishAnimatedFk(boneBuf);
}

                // SEQLOCK: latch ONE consistent pose frame for this whole solve, so
                // every SharedPose() read below comes from the same frame -> no torn
                // quaternion -> the whole-body IK stops jittering. Refreshed once per
                // player apply (this runs on the animation thread; dxgi writes on the
                // present thread).
                RefreshHandsSnapshot();

                // CUTSCENE FULL-SUSPEND (PR #40, fr05t1k). During scripted scenes the engine plays
                // a fully authored body+arm animation; letting VRIK keep solving fights it and the
                // avatar looks wrong. Suspends the whole solve while the scene tier reaches the
                // threshold the overlay owns, leaving the engine's own cinematic pose untouched.
                //
                // Both inputs are read DIRECTLY: the tier from the global the camera hook refreshes,
                // the threshold from g_liveControls like every other setting in this file. Nothing is
                // published anywhere. The first version routed both through shared slots [157]/[158],
                // which XInput.cpp overwrites every input tick with the B and Y button flags, so this
                // test read 0 for both no matter what was written.
                //
                // Only tiers 1..4 (Tier2..Tier5) arm it, so a zero-initialised setting -- before
                // vrport.ini is read -- and an explicit -1 both mean "never suspend". Bails AFTER the
                // match counter above, so the CET desync detector stays healthy and does not re-arm
                // in a storm during the scene.
                {
                    const int tier = g_sceneTier.load(std::memory_order_relaxed);
                    const int minTier = g_liveControls.xrCutsceneSuspendTier;
                    const bool suspend = (minTier >= 1 && minTier <= 4 && tier >= minTier);
                    if (suspend) {
                        return result;
                    }
                }

                // THE TWO-HAND CAPTURE READS FIRST, BEFORE ANY LAYER OF OURS WRITES. It sat after the
                // resting pose and recorded that instead of the game: the resting layer writes the left
                // fingers whenever a weapon is out, so the 'two-handed grip' came back bit-identical to
                // the resting hand -- the capture had photographed its own output. Anything that reads the
                // animation must run before everything that replaces it.
                cvr::anim::TwoHandCapture(boneBuf);

                // HEAD AIM: the weapon takes the head's orientation. Needs the view packet latched
                // even when VRIK is not solving, because the packet IS the orientation it applies --
                // and so does the eye re-anchoring below, for both aiming models.
                const bool adsEyeAlignmentActive = headAimWork || nonVrikAdsWork;
                if (adsEyeAlignmentActive) VRIK_LatchViewPacket();
                cvr::anim::ApplyHeadAimWeaponOrientation(boneBuf);

                // THE NON-VRIK ADS MUZZLE STABILIZER. Here because it must see the engine's own
                // animated pose: it measures the direction error the aim-in animation introduced and
                // takes it back out, so it runs after the capture reads above and before any layer of
                // ours writes. Internally a no-op while VRIK drives the arms.
                cvr::anim::ApplyNonVrikAdsMuzzleStabilizer(boneBuf);

                // AND THE ARMS FOLLOW THE WEAPON ONTO THE SIGHTING EYE (dabinn, TofuExpress
                // 73bdf668). Prepare records the authored arm pose for this tick and computes
                // eye-anchored targets from it; Solve moves the arms to them, rotation only. Solve
                // runs AFTER the weapon writers above because it reads the right hand's finished
                // rotation back out of the pose they left.
                cvr::anim::PrepareAimArmTargets(boneBuf);
                cvr::anim::SolvePreparedAimArms(boneBuf);


                // THE LEFT HAND AT REST. While a weapon is out the game curls that hand into the support
                // half of a two-handed grip; in VR it is empty and in view, so it wears the pose the game
                // itself plays with empty hands -- captured from this same buffer while unarmed. FIRST,
                // because the layers below nlerp onto whatever is here: a reload preview must fade in FROM
                // the resting fingers, which is the difference between a base layer and an override.
                cvr::anim::VrikRestFingerPose(boneBuf);

                // THE TWO-HAND GRIP'S FINGERS, on top of the resting hand and under the reload layer. The
                // order is the priority: a hand resting does the least specific job, a hand on the weapon's
                // grip a more specific one, and a hand on a magazine the most specific of all -- so each
                // writes over the one before it and the reload keeps the last word.
                cvr::anim::TwoHandFingers(boneBuf);

                // RELOAD FINGER POSE: pose the FREE hand's fingers into a grip while it holds a weapon part
                // (slide/mag). Reuses the smoke resolver's finger indices (hand 1 = right g_VRSmokeFingerIdx,
                // hand 0 = left IdxL). Only the fingers explicitly set (from the reload anim, GLB->runtime) are
                // written, so a partial pose leaves the others under tracking. Same spot as the smoke hold, so no
                // finger flicker across the passes/tick.
                if (g_VRReloadFingerActive[0] || g_VRReloadFingerActive[1]) {
                    cvr::anim::VrikReloadFingerPose(boneBuf);
                }

                // SMOKE FINGER-HOLD (fingers-only grip). Runs on EVERY player pass, BEFORE
                // the solve/replay split, so the curl is bit-identical across the 4-5
                // passes/tick -> no finger flicker. The original ran at the top of this
                // hook, so boneBuf's finger locals here are the game's current anim pose:
                //   * CAPTURE reads them (while the AMM hold-cigarette workspot plays -> the
                //     authored curl) and latches them; it takes priority so it never records
                //     our own replayed pose.
                //   * APPLY writes the latched locals back. Finger bones are parent-local and
                //     untouched by VRIK, so they curl relative to the controller-driven wrist.
                // OPEN HANDS IN A CAR. Before the smoke grip on purpose: if the smoking mod is
                // holding something in that hand it writes after us and wins.
                cvr::anim::WheelFingers(boneBuf);

                if (g_VRSmokeFingerCount > 0 || g_VRSmokeCigIdx >= 0) {
                    cvr::anim::VrikSmokingCigPose(boneBuf);
                }
                // LEFT HAND mirror (lighter grip): left fingers + WeaponLeft slot.
                if (g_VRSmokeFingerCountL > 0 || g_VRSmokeLighterIdx >= 0) {
                    cvr::anim::VrikSmokingLighterPose(boneBuf);
                }


                // (SNAP PUPPET PRE-ROTATION removed after live test: rotating the ROOT
                // bone during the entity-lag tick DID visibly rotate the rendered body
                // (walking: body turned separately under the hands) yet the sprint snap
                // ghost was UNCHANGED -- proving the ghost frame is composed from pose
                // data snapshotted BEFORE the snap-tick writes; no bone write on the
                // snap tick can reach that frame. Also: Aim_JNT full-freeze test showed
                // the sprint-START camera jerk does NOT live in the fppCamera chain.)

                // FPP-CAMERA BONE FREEZE (rig-level camera-motion kill, user order: no bob
                // after shots, no bob after sprint with melee). Every per-shot recoil kick,
                // melee swing sway, sprint settle and idle camera lean is ANIMATION DATA on
                // the Torso_fppCamera_* control chain inside the weapon/locomotion .anims
                // sets. Per-weapon anim-edit mods zero the camera track file by file and
                // can't process the melee/revolver packings (their own "doesn't work on"
                // list: katana, knife, fists, batons, base_revolver...). We sit in the pose
                // pipeline instead, so: capture each camera bone's local on the FIRST player
                // pass (rest; the graph's camera additives are ~identity outside actions)
                // and REWRITE it every pass after that. Runs BEFORE the solve/replay split
                // -> identical in fresh-solve and replay passes (bit-exact across the tick),
                // and BEFORE VRIK_ComputeFK -> camModelPos (hand anchors) stabilizes too,
                // not just the view base dxgi locates. Works for every weapon incl. all
                // base_melee, and kills the residual "input-driven camera lean" noted in
                // dxgi's view-translation comment. ADS/scene camera-bone anims die with it
                // — desired in VR (aim is physical, scenes are HMD-driven).
                // AIM_JNT CAMERA-SHAKE KILL. Mask testing isolated the ENTIRE baked camera
                // shake (shot recoil kick, melee swing sway, sprint settle) to this ONE joint
                // — Torso_fppCamera_Aim_JNT — so the other four fppCamera bones stay untouched.
                // The same joint ALSO carries the camera's live yaw response (freezing it whole
                // = the snap/sprint doubles), hence the component split.
                // Modes (SetVRCamBoneFreeze): 0 = stock (default until validated);
                //   1 = YAW-LIVE freeze: TWIST about the model vertical passes through live,
                //       SWING (pitch/roll kick+sway) and TRANSLATION freeze to the captured
                //       rest. The vertical is expressed in the PARENT frame via a partial FK
                //       up the ancestor chain — rig joint frames are arbitrary (the first cut
                //       assumed parent Z ≈ model up, split about a skewed axis, and BOTH leaked
                //       shake and distorted the yaw). q = swing * twist =>
                //       q_out = swing_rest(axis_now) * twist_live(axis_now), rest re-decomposed
                //       per pass about the CURRENT axis (parents are live and move).
                //   2 = FULL freeze: diagnostic reference (shake provably dead, doubles present).
                //   3 = SWING-ONLY freeze: like 1 but the TRANSLATION stays LIVE — mode-1 testing
                //       (shake dead, double still there) points at the live camera response
                //       living (partly) in the translation channel, not only the rotation twist.
                {   // (unconditional: the rest capture must run from pass one even in mode 0,
                    //  so enabling a mode later never freezes onto a mid-action snapshot)
                    // GENERALIZED TO ALL FIVE fppCamera JOINTS (foreign-frame hunt).
                    // dxgi's [RENDERCAM] proved 3-9 frames/s reach the render with
                    // 5-8 deg of pitch/roll the head never made — episodic, anim-timed.
                    // The mask isolation that pinned the baked shake to Aim_JNT was run
                    // on shots/melee/sprint only; landing/vault/hit-reaction/idle-
                    // transition anims may drive the OTHER four joints. So now:
                    //  * every pass MEASURES each joint's raw (pre-rewrite) rotation
                    //    deviation from its captured rest; the max + argmax go to
                    //    shared [82]/[83], and dxgi appends them to [RENDERCAM] —
                    //    one session shows WHICH joint moves on the foreign frames;
                    //  * mode 4 (test default) = the proven mode-3 swing-only freeze
                    //    applied to ALL five joints: TWIST about the model vertical
                    //    stays live (no snap/sprint doubles), SWING (pitch/roll) is
                    //    pinned to rest. Translations stay live on all joints.
                    //  Modes 0..3 keep their exact old semantics (Aim_JNT only).
                    static float s_camRest[5][7];
                    static bool  s_camCap[5] = {};
                    float maxDevDeg = 0.0f;
                    float maxDevBone = -1.0f;
                    for (int ci = 0; ci < 5; ++ci) {
                        const int bi = g_VRFppCamIdx[ci];
                        if (bi < 0 || bi >= VRIK_MAX_BONES) continue;
                        float* t = reinterpret_cast<float*>(boneBuf + bi * 48 + VRIK_TRANS_OFF);
                        float* q = reinterpret_cast<float*>(boneBuf + bi * 48 + VRIK_ROT_OFF);
                        if (!s_camCap[ci]) {
                            s_camRest[ci][0]=t[0]; s_camRest[ci][1]=t[1]; s_camRest[ci][2]=t[2];
                            s_camRest[ci][3]=q[0]; s_camRest[ci][4]=q[1]; s_camRest[ci][5]=q[2]; s_camRest[ci][6]=q[3];
                            s_camCap[ci] = true;
                            continue;
                        }
                        // Raw anim deviation from rest, BEFORE any rewrite below.
                        {
                            float d = q[0]*s_camRest[ci][3] + q[1]*s_camRest[ci][4]
                                    + q[2]*s_camRest[ci][5] + q[3]*s_camRest[ci][6];
                            if (d < 0.0f) d = -d;
                            if (d > 1.0f) d = 1.0f;
                            const float devDeg = 2.0f * std::acos(d) * 57.2957795f;
                            if (devDeg > maxDevDeg) { maxDevDeg = devDeg; maxDevBone = static_cast<float>(ci); }
                        }
                        const bool aimJnt = (ci == 1);
                        const bool freezeThis = (g_VRCamBoneFreeze == 4) ||
                            (aimJnt && (g_VRCamBoneFreeze == 1 || g_VRCamBoneFreeze == 2 || g_VRCamBoneFreeze == 3));
                        if (!freezeThis) continue;
                        if (aimJnt && g_VRCamBoneFreeze == 2) {   // full-freeze diagnostic mode
                            t[0]=s_camRest[ci][0]; t[1]=s_camRest[ci][1]; t[2]=s_camRest[ci][2];
                            q[0]=s_camRest[ci][3]; q[1]=s_camRest[ci][4]; q[2]=s_camRest[ci][5]; q[3]=s_camRest[ci][6];
                            continue;
                        }
                        // Swing-only freeze (modes 1/3 on Aim_JNT; mode 4 on all five).
                        float Rp[4] = { 0.0f, 0.0f, 0.0f, 1.0f };
                        {
                            int chain[24]; int cn = 0;
                            for (int p = g_VRBoneParent[bi]; p >= 0 && p < VRIK_MAX_BONES && cn < 24; p = g_VRBoneParent[p])
                                chain[cn++] = p;
                            for (int k = cn - 1; k >= 0; --k) {   // root ... immediate parent
                                const float* pq = reinterpret_cast<float*>(boneBuf + chain[k] * 48 + VRIK_ROT_OFF);
                                float tmp[4]; VRIK_QuatMul(Rp, pq, tmp);
                                Rp[0]=tmp[0]; Rp[1]=tmp[1]; Rp[2]=tmp[2]; Rp[3]=tmp[3];
                            }
                            VRIK_QuatNorm(Rp);
                        }
                        float RpInv[4]; VRIK_QuatConj(Rp, RpInv);
                        const float upModel[3] = { 0.0f, 0.0f, 1.0f };
                        float axis[3]; VRIK_QuatRotateVec(RpInv, upModel, axis);
                        const float an = std::sqrt(axis[0]*axis[0] + axis[1]*axis[1] + axis[2]*axis[2]);
                        if (an > 1e-6f) {
                            axis[0]/=an; axis[1]/=an; axis[2]/=an;
                            float twLive[4]; VRIK_TwistAbout(q, axis, twLive);
                            float qRest[4] = { s_camRest[ci][3], s_camRest[ci][4], s_camRest[ci][5], s_camRest[ci][6] };
                            float twRest[4]; VRIK_TwistAbout(qRest, axis, twRest);
                            float twRestInv[4]; VRIK_QuatConj(twRest, twRestInv);
                            float swingRest[4]; VRIK_QuatMul(qRest, twRestInv, swingRest);
                            float qOut[4]; VRIK_QuatMul(swingRest, twLive, qOut);
                            VRIK_QuatNorm(qOut);
                            if (aimJnt && g_VRCamBoneFreeze == 1) {   // modes 3/4 keep translation LIVE
                                t[0]=s_camRest[ci][0]; t[1]=s_camRest[ci][1]; t[2]=s_camRest[ci][2];
                            }
                            q[0]=qOut[0]; q[1]=qOut[1]; q[2]=qOut[2]; q[3]=qOut[3];
                        }
                    }
                    // Publish the camera-chain deviation for dxgi's [RENDERCAM] line.
                    if (g_pSharedHands) {
                        g_pSharedHands[82] = maxDevDeg;
                        g_pSharedHands[83] = maxDevBone;
                    }
                }


                // VRIK FULL-ARM IK (mode 4): model-space FK + 2-bone IK, rotation-only
                // writes (no stretch). Anchored at the head bone's model position; the
                // controller offset is taken straight from the proven gizmo world math.
                // NOT WHILE HEAD AIM OWNS THE WEAPON. The whole body/arm solve below answers
                // "where do the controllers put the hands"; head aim answers "where does the head
                // point the gun". Running both puts the arms in a fight with the weapon they hold.
                if (!headAimWork && g_VRBind == 4 && g_pSharedHands &&
                    g_VRBoneCount > 0 && g_VRHeadBoneIdx >= 0) {
                    // ONE SOLVE PER TICK + BIT-EXACT REPLAY. Measured: the engine applies
                    // the player pose 4-5x per tick (solvesPerTick max=5, same buffer).
                    // Every engine pass re-evaluates the graph from ITS OWN inputs --
                    // overwriting our previous write -- and we used to re-solve after
                    // each. Mid-frame consumers (shadow/reflection/render snapshot) could
                    // catch the buffer BETWEEN the engine write and our solve -> mixed
                    // anim/solved states inside one frame: hands shifted during strafe
                    // (lean passes), arm double on snap-turn (turn-assist passes) -- none
                    // of which existed in 0.0.8 when nothing wrote the skeleton. Now the
                    // full solve runs ONCE per entity tick; every later pass of the same
                    // tick replays the cached solved locals, so the buffer leaves every
                    // pass bit-identical no matter which pass anyone samples.
                    // THE CLOCK IS THE ANIMATION BATCH ITSELF, not the Present that follows it.
                    //
                    // It was the Present epoch, and that was measurably wrong: at 84-86 fps freshSolve
                    // matched present exactly, but at 79.5 it was 77.0 and at 67.5 it was 63.0 -- 3 to
                    // 7 per cent of displayed frames got a REPLAY instead of a new solve. Those frames
                    // show the previous arm while the world is drawn from a new camera, which is the
                    // step felt on a moving hand and the reason it worsens as the frame rate drops.
                    //
                    // The cause is phase, not rate. The epoch was bumped in Present while the engine's
                    // animation runs on its own schedule, so whenever two animation batches fell between
                    // two Presents the second saw an epoch it had already solved for.
                    //
                    // A BATCH IS FOUND BY THE GAP, and the two populations do not overlap: the engine
                    // applies the player pose 4-5 times back to back, microseconds apart, and then not
                    // again for a frame. Anything past a few milliseconds is a new batch. The threshold
                    // is compared against the measured distribution below rather than assumed -- if the
                    // two populations ever meet, the gap histogram says so before the arms do.
                    static double s_lastPassMs = 0.0;
                    static uint32_t s_batchSeq = 0;
                    const double passNowMs = VrikNowMs();
                    const double passGapMs = (s_lastPassMs > 0.0) ? (passNowMs - s_lastPassMs) : 1000.0;
                    s_lastPassMs = passNowMs;
                    static int s_passInBatch = 0;
                    if (passGapMs > CyberpunkVR_VrikBatchGapMs) { ++s_batchSeq; s_passInBatch = 0; }
                    else ++s_passInBatch;
                    // The CLOCK above is load-bearing and always runs; only its census is diagnostic.
                    if (CyberpunkVR_XrDeepDiag) {
                        VrikNoteBatchGap(passGapMs, passGapMs > CyberpunkVR_VrikBatchGapMs);
                    }
                    const uint32_t tickNow = CyberpunkVR_VrikBatchClock
                                                 ? s_batchSeq
                                                 : g_VrikFrameEpoch.load(std::memory_order_relaxed);
                    // SNAP EVENT AWARE REPLAY DECISION. snap_trace proved the raw
                    // packet-yaw comparator was wrong: on the snap tick pass #1 the
                    // event-rotated packet solved CORRECTLY, then pass #2 re-latched the
                    // still-old shared[141] and the raw comparator forced ANOTHER fresh
                    // solve, undoing the correction inside the SAME tick (trace: two
                    // lines at seq99=7509, first pktYaw=new then pktYaw=old). Replay may
                    // only break on the SNAP EVENT counter [147] itself: new counter =>
                    // exactly one fresh solve this tick; later passes replay that solved
                    // pose bit-identically no matter what stale render packet still says.
                    const float snapCtrNow = g_pSharedHands[147];
                    const bool  snapEvent = (g_solveCacheN > 0 && snapCtrNow != g_solveCacheSnapCtr);
                    if (tickNow == g_solveCacheTick && g_solveCacheN > 0 && !snapEvent) {
                        // HOW FAR THE ENGINE HAD MOVED THE BONE BEFORE WE PUT IT BACK.
                        //
                        // Our detour sits ON the pose-apply function, so the order inside every pass is:
                        // the engine re-evaluates the graph from its own inputs, calls pose-apply, and
                        // then we write. Between those two the buffer holds the ENGINE's arm, not ours,
                        // and anything that samples the skeleton in that window renders that arm.
                        //
                        // This is the size of that window's error, in millimetres: the local translation
                        // as we found it against the value we are about to restore. Small means the two
                        // arms nearly coincide and catching the wrong one would not show. Centimetres
                        // means a frame that caught it shows the arm somewhere else -- which is what a
                        // double image on the hand and nothing else looks like.
                        //
                        // Peak per window, cleared by the reader, and only on the replay path: the fresh
                        // solve has no previous value of ours to compare against.
                        if (CyberpunkVR_XrDeepDiag) {
                            float worst = 0.0f;
                            int   worstIdx = -1;
                            for (int ci = 0; ci < g_solveCacheN; ++ci) {
                                const int bi = g_solveCacheIdx[ci];
                                const float* t0 = reinterpret_cast<const float*>(boneBuf + bi * 48 + VRIK_TRANS_OFF);
                                const float dx = t0[0] - g_solveCacheVal[ci][0];
                                const float dy = t0[1] - g_solveCacheVal[ci][1];
                                const float dz = t0[2] - g_solveCacheVal[ci][2];
                                const float mm = std::sqrt(dx*dx + dy*dy + dz*dz) * 1000.0f;
                                if (mm < 2000.0f && mm > worst) { worst = mm; worstIdx = bi; }
                            }
                            if (worst > g_VrikEngineOverwriteMm) {
                                g_VrikEngineOverwriteMm = worst;
                                g_VrikEngineOverwriteBone = worstIdx;
                            }
                            if (worst > 10.0f) {
                                ++g_VrikEngineOverwriteHits;
                                // WHICH PASS OF THE BATCH, because that names the writer. Our own write
                                // lands at the end of every pass, so a bone found moved at the start of
                                // pass N was moved by whatever ran between pass N-1 and pass N. If that
                                // is always the same index, it is one identifiable engine stage; if it
                                // is scattered, it is something asynchronous and the fix is different.
                                const int slot = (s_passInBatch < 7) ? s_passInBatch : 7;
                                ++g_VrikOverwritePassHist[slot];
                            }
                            ++g_VrikEngineOverwritePasses;
                        }
                        for (int ci = 0; ci < g_solveCacheN; ++ci) {
                            const int bi = g_solveCacheIdx[ci];
                            float* t = reinterpret_cast<float*>(boneBuf + bi * 48 + VRIK_TRANS_OFF);
                            float* q = reinterpret_cast<float*>(boneBuf + bi * 48 + VRIK_ROT_OFF);
                            t[0]=g_solveCacheVal[ci][0]; t[1]=g_solveCacheVal[ci][1]; t[2]=g_solveCacheVal[ci][2];
                            q[0]=g_solveCacheVal[ci][3]; q[1]=g_solveCacheVal[ci][4]; q[2]=g_solveCacheVal[ci][5]; q[3]=g_solveCacheVal[ci][6];
                        }
                        ++g_VRIKReplayTotal;
                    } else {
                    ++g_VRIKFreshTotal;
                    // Latch the render view packet ONCE for this solve: both arms and
                    // the view-pos resolver consume the SAME frame.
                    VRIK_LatchViewPacket();
                    if (g_viewPktValid) g_solveCacheYaw = g_viewPkt[8];
                    // Hand recoil advances ONCE per fresh solve, not per arm and not per pass: the
                    // spring is integrated with real elapsed time, so a second call in the same frame
                    // would step it twice and make the kick frame-rate dependent -- which is exactly
                    // the defect the spring replaced.
                    RecoilTick();
                    g_solveCacheSnapCtr = snapCtrNow;
                    // Frozen-frame rejection was removed. It could misclassify normal
                    // downward head motion as stale data and replace a live pose with an
                    // older one. Solve the live pose directly.
                    // IN-VEHICLE = ARMS ONLY (user order). Seated, the vehicle drives the
                    // puppet + camera; every BODY write (torso dampen, girdle pins,
                    // PlaceBodyUnderHMD with hips/spine/legs) fights that and breaks the
                    // character/camera position. dxgi publishes the flag in [31].
                    const bool vrikInVehicle = (SharedPose(31) > 0.5f);
                    VRIK_ComputeFK(boneBuf, VRIK_FKCount());
                    // WHEEL GRAB. This FK is the pure ANIMATED pose -- nothing of ours has been
                    // written into the buffer yet this solve -- so g_fkPos[hand] is literally the hand
                    // the driving animation puts on the wheel. Capture it HERE and nowhere else: a few
                    // lines further down the segment lengths are rescaled to the player's arm and it
                    // stops being the animation's answer.
                    cvr::anim::WheelCaptureAnim(0, g_VRRightBoneIdx);
                    cvr::anim::WheelCaptureAnim(1, g_VRLeftBoneIdx);
                    {
                        // QPC, not GetTickCount64: the tick counter moves in ~15.6 ms steps, which
                        // over a 160 ms blend is ten of them -- the hand would step to the wheel.
                        static int64_t s_wheelLastQpc = 0;
                        LARGE_INTEGER qc{}, qf{};
                        QueryPerformanceCounter(&qc);
                        QueryPerformanceFrequency(&qf);
                        float dt = 0.016f;
                        if (s_wheelLastQpc != 0 && qf.QuadPart > 0) {
                            dt = static_cast<float>(
                                static_cast<double>(qc.QuadPart - s_wheelLastQpc) /
                                static_cast<double>(qf.QuadPart));
                        }
                        s_wheelLastQpc = qc.QuadPart;
                        cvr::anim::WheelUpdate(dt);
                    }
                    const bool wheelOffR = cvr::anim::WheelHandsOff(0);
                    const bool wheelOffL = cvr::anim::WheelHandsOff(1);
                    if (!vrikInVehicle) {
                        VRIK_DampenTorsoWeaponPose(boneBuf);
                        VRIK_PinGirdleTranslations(boneBuf);
                        VRIK_ComputeFK(boneBuf, VRIK_FKCount());
                    }
                    // IK-style arm-length calibration: reset upper-arm/forearm segment lengths
                    // from cached rest local translations, then scale them to the T-pose measured
                    // user arm. Do not derive length from the current weapon/stance FK pose.
                    {
                        // AN ARM HANDED TO THE ANIMATION KEEPS THE RIG'S OWN LENGTHS: scaled to
                        // the player's arm instead, the animation's rotations put the hand BESIDE the
                        // wheel rather than on it. Put the rest translations back -- neither this
                        // scale nor the shoulder protraction is undone by the engine.
                        if (wheelOffR) {
                            VRIK_RestoreArmRestTrans(boneBuf, trackBuf, g_VRBoneCount,
                                                     g_VRRightUpperArmIdx, g_VRRightForeArmIdx,
                                                     g_VRRightBoneIdx, /*isLeft*/false);
                        } else {
                            VRIK_ScaleArmBonesFromRest(boneBuf, trackBuf, g_VRBoneCount,
                                                       g_VRRightForeArmIdx, g_VRRightBoneIdx, g_VRUserArmLenR);
                        }
                        if (wheelOffL) {
                            VRIK_RestoreArmRestTrans(boneBuf, trackBuf, g_VRBoneCount,
                                                     g_VRLeftUpperArmIdx, g_VRLeftForeArmIdx,
                                                     g_VRLeftBoneIdx, /*isLeft*/true);
                        } else {
                            VRIK_ScaleArmBonesFromRest(boneBuf, trackBuf, g_VRBoneCount,
                                                       g_VRLeftForeArmIdx, g_VRLeftBoneIdx, g_VRUserArmLenL);
                        }
                        VRIK_ComputeFK(boneBuf, VRIK_FKCount());
                    }
                    // Right-hand CONTROLLER position in model space, captured from the arm-IK
                    // target right before VRIK_SolveArm. The holster distances [20..22] MUST
                    // use this, not g_fkPos[wrist]: the FK above is recomputed from the
                    // ENGINE'S ANIMATED pose each fresh solve, so the FK wrist tracks the
                    // game animation (idle = hands at thighs -> permanent zone R; weapon
                    // ready = wrist at chest -> permanent zone B), NOT the player's hand --
                    // grip presses fired holster zones with the real hand nowhere near them.
                    float rhWristModel[3] = { 0.0f, 0.0f, 0.0f };
                    bool  rhWristValid = false;
                    int hIdx = g_VRHeadBoneIdx;
                    const float* headFKraw =
                        (hIdx >= 0 && hIdx < VRIK_MAX_BONES) ? g_fkPos[hIdx] : nullptr;
                    // THE ARMS HANG OFF THE ENGINE'S OWN FK HEAD, this frame's, unfiltered.
                    //
                    // A 3-tap median per axis used to sit here. It was the exact filter for the bug it
                    // was built for -- an AER-era FPP pose that flexed the upper body ~9 cm on a strict
                    // 3-frame cycle, where median([lo,hi,hi]) = hi every frame -- and it cost about one
                    // frame of lag on real motion. AER is gone, so its premise is gone with it, and a
                    // frame of lag on the anchor the hands are built from is not something to keep on
                    // the strength of an obsolete measurement.
                    const float* headModelPos = headFKraw;
                    // HMD orientation relative to recenter base (producer slots 16..19).
                    // Used to undo the HMD-local frame of the controller poses. Read
                    // from the seqlock snapshot (local array so callees take a ptr).
                    const float hmdRelBuf[4] = { SharedPose(16), SharedPose(17), SharedPose(18), SharedPose(19) };
                    const float* hmdRel = hmdRelBuf;
                    // Full HMD position relative to the recenter base ([124..126], base axes):
                    // completes the room-fixed controller vector (head translation included).
                    const float hmdPosBase[3] = { SharedPose(124), SharedPose(125), SharedPose(126) };
                    if (headModelPos) {
                        // Fallback body frame. Once camera pose is available below, this is replaced
                        // by HMD/body yaw. Never let animated weapon-stance shoulders define IK axes.
                        float bodyUp[3]    = { 0.0f, 0.0f, 1.0f };
                        float bodyRight[3] = { 1.0f, 0.0f, 0.0f };
                        {
                            const float* rootP = g_fkPos[0];
                            bodyUp[0]=headModelPos[0]-rootP[0]; bodyUp[1]=headModelPos[1]-rootP[1]; bodyUp[2]=headModelPos[2]-rootP[2];
                            if (VRIK_Norm3(bodyUp) < 1e-4f) { bodyUp[0]=0.0f; bodyUp[1]=0.0f; bodyUp[2]=1.0f; }
                            if (g_VRRightUpperArmIdx >= 0 && g_VRLeftUpperArmIdx >= 0) {
                                const float* rs = g_fkPos[g_VRRightUpperArmIdx];
                                const float* ls = g_fkPos[g_VRLeftUpperArmIdx];
                                bodyRight[0]=rs[0]-ls[0]; bodyRight[1]=rs[1]-ls[1]; bodyRight[2]=rs[2]-ls[2];
                                if (VRIK_Norm3(bodyRight) < 1e-4f) { bodyRight[0]=1.0f; bodyRight[1]=0.0f; bodyRight[2]=0.0f; }
                            }
                        }
                        // bodyFwd = up x right. Keep this body-derived, not head-derived: turning
                        // the HMD must not swivel the elbow bend plane or flip forward/back.
                        float bodyFwd[3];
                        VRIK_Cross3(bodyUp, bodyRight, bodyFwd);
                        if (VRIK_Norm3(bodyFwd) < 1e-4f) { bodyFwd[0]=0.0f; bodyFwd[1]=1.0f; bodyFwd[2]=0.0f; }

                        // PHASE 2 — FULL BODY under the HMD, anchored from the head ("bone head = hmd").
                        // Drives the head bone to the HMD pose, bends the spine naturally to connect the
                        // hips up to it, slides the hips under the HMD, and IKs the legs to keep the feet
                        // planted -- BEFORE anchoring the shoulders/arms. Afterwards the head sits at the
                        // HMD and the shoulder girdle hangs under it, so the arm reach matches the gizmo.
                        float camModelPos[3] = {0,0,0}, camModelRot[4] = {0,0,0,1};
                        float camModelEntityQuat[4] = {0,0,0,1};
                        float camModelPairedRot[4] = {0,0,0,1};
                        bool camModelValid = VRIK_ComputeCamModel(
                            camModelPos, camModelRot, camModelEntityQuat, camModelPairedRot);
                        // A fresh XR head is correct for the head bone, but the controller packet's
                        // existing composition expects the stable pushed camera base. Rotating that
                        // packet by the fresh head leaves head motion between the instants on the arm.
                        // Keep an explicit live A/B that changes only this split, never the body anchor.
                        const float* camModelHandRot = CyberpunkVR_VrikSplitHeadHandRot
                            ? camModelPairedRot : camModelRot;
                        // (REVERTED, snap-double isolation: the "render-heading re-yaw" that lived
                        // here — a companion of the fppCamera bone freeze — is removed together
                        // with the ResolveViewPos pairLocal re-yaw and the dxgi snap holdback.
                        // Baseline = the weeks-tested stock heading path. Restore from git if the
                        // freeze experiment resumes.)
                        // ---- THE RIG CONSTANT THE CLEAN PATH NEEDS -------------------------------
                        //
                        // Everything below still takes its anchor from camModelPos/camModelRot, and those
                        // come from WORLD transforms a CET script pushes in: the entity quaternion, the
                        // camera quaternion, the stabilized pair offset. That is the last thing arriving
                        // from outside, and it is a round trip -- WE compose the game camera from the
                        // headset, the script reads it back out of the game, and pushes it to us.
                        //
                        // The clean path anchors on the ENGINE'S OWN HEAD BONE instead, which the pose
                        // buffer already gives us in model space, plus the headset pose. The only thing
                        // missing is the fixed rotation and offset between the head-bone frame and the
                        // camera frame -- a property of the rig, not of the frame. So measure it while the
                        // old path is still here, and cut with a number instead of a guess.
                        //
                        // A CONSTANT IS A CONSTANT ONLY IF IT DOES NOT MOVE. The spread is printed with
                        // the value: if it wanders, the head bone is carrying animation the camera does
                        // not (a look-at, a bob), and then the bone is not a proxy for the camera and this
                        // approach is wrong -- which is exactly what needs to be known before cutting.
                        float rawCamModelPos[3] = { camModelPos[0], camModelPos[1], camModelPos[2] };

                        // Apply the SAME baked camera->head offset (shared [91..93], game-local
                        // right/fwd/up) that dxgi shifts the VIEW by, so the avatar head sits exactly
                        // where the offset-tuned view sits. head = camera, regulated by the offset.
                        // IN VEHICLE the bake is dropped on BOTH sides (dxgi stops shifting the
                        // view, we stop shifting camModelPos): it was measured on the standing
                        // body and just pushes the seated view/anchors off the seat.
                        if (camModelValid && g_pSharedHands && !vrikInVehicle) {
                            camModelPos[0] += SharedPose(91);
                            camModelPos[1] += SharedPose(92);
                            camModelPos[2] += SharedPose(93);
                        }
                        // HAND == GIZMO via the single-origin pattern used by open-source VR-mod
                        // frameworks (UEVR / REFramework: originWorld = cameraWorld * inv(hmdStage);
                        // controllerWorld = origin * controllerStage; viewWorld = origin * hmdStage --
                        // controller and HMD live in ONE space, ONE transform maps both, so the
                        // view<->hands relation is exact under any head rotation, no inversion by
                        // construction). Our signals: the producer stores controllers FULL-HMD-local
                        // (openxr_manager 3309-3315) while the game camera carries NO HMD pitch/roll
                        // (dxgi adds them render-only: renderQuat = gameYaw * xrPitchRoll, 2495-2508)
                        // -- rotating a full-HMD-local vector by that partial camera frame was the
                        // head-turn inversion. The origin in model space therefore is:
                        //   baseModelRot = camModelRot * inverse(yawTwist(mapQuat(hmdRel)))
                        // (the game yaw already CONTAINS the physical head yaw; removing hmdRel's yaw
                        // twist leaves the recenter-base heading -- constant under head motion).
                        // Hand composition = EXACT gizmo formula (camModelRot * map(raw controller)),
                        // see the arm blocks. Every attempt to out-smart the gizmo's frame handling
                        // (baseModelRot yaw-removal, pitchRollOnly(hmdRel)) broke yaw one way or the
                        // other; the gizmo itself is the user-validated ground truth in all head poses.
                        // Anchor = the EXACT render-view point: body camera (baked camModelPos) plus
                        // the view-only offsets dxgi adds on top of the bake (manual Tracking-Camera +
                        // auto eye-view) = [120..122] total minus [91..93] bake. Keeps hand-vs-view ==
                        // gizmo-vs-view (including the user's manual view tuning) with reachable
                        // arm geometry (no 0.37m camera-mount gap).
                        float handAnchor[3] = { camModelPos[0], camModelPos[1], camModelPos[2] };
                        if (g_pSharedHands && SharedPose(123) == 1.0f) {
                            // [120..122] total minus the bake share. IN VEHICLE dxgi already
                            // publishes the total WITHOUT the bake (and camModelPos above has
                            // no bake either), so there is nothing to subtract.
                            const float bkx = vrikInVehicle ? 0.0f : SharedPose(91);
                            const float bky = vrikInVehicle ? 0.0f : SharedPose(92);
                            const float bkz = vrikInVehicle ? 0.0f : SharedPose(93);
                            handAnchor[0] += SharedPose(120) - bkx;
                            handAnchor[1] += SharedPose(121) - bky;
                            handAnchor[2] += SharedPose(122) - bkz;
                        }
                        // stage 1: the anchor both arms hang off, after every offset is in. Recorded
                        // under BOTH hands because it is shared -- the first version wrote it under hand
                        // 0 only, so the right hand's anchor column read 0.00 and looked like a result.
                        if (CyberpunkVR_XrDeepDiag) VRIK_NoteShake(0, 1, handAnchor);
                        if (CyberpunkVR_XrDeepDiag) VRIK_NoteShake(1, 1, handAnchor);
                        // THE ARM FRAME COMES FROM THE SKELETON, NOT FROM THE CAMERA YAW.
                        //
                        // bodyRight/Up/Fwd decide where the elbow points and where the shoulder joints
                        // sit, and taking them from the camera means both follow the HEAD -- turn your
                        // head and the elbows swing. The avatar's own bones already carry the answer:
                        // up along root->head, right across the shoulder line, forward as their cross
                        // product. That frame turns when the BODY turns and stands still when only the
                        // head moves, which is the whole point.
                        //
                        // Kept behind a flag next to the camera-yaw version, because the camera version
                        // is what the dead-cone follow (VRIK_BodyAxesFromCamYaw, gain/cap) was tuned
                        // for and it is the only way back if a rig has no usable shoulder line.
                        auto armFrameFromBody = [&]() -> bool {
                            if (g_VRRightUpperArmIdx < 0 || g_VRLeftUpperArmIdx < 0) return false;
                            if (g_VRRightUpperArmIdx >= VRIK_MAX_BONES ||
                                g_VRLeftUpperArmIdx >= VRIK_MAX_BONES) return false;
                            const int hb = (g_VRHeadBoneIdx >= 0 && g_VRHeadBoneIdx < VRIK_MAX_BONES)
                                               ? g_VRHeadBoneIdx : -1;
                            if (hb < 0) return false;
                            float up[3] = { g_fkPos[hb][0] - g_fkPos[0][0],
                                            g_fkPos[hb][1] - g_fkPos[0][1],
                                            g_fkPos[hb][2] - g_fkPos[0][2] };
                            if (VRIK_Norm3(up) < 1e-4f) { up[0]=0.0f; up[1]=0.0f; up[2]=1.0f; }
                            const float* rs = g_fkPos[g_VRRightUpperArmIdx];
                            const float* ls = g_fkPos[g_VRLeftUpperArmIdx];
                            float right[3] = { rs[0]-ls[0], rs[1]-ls[1], rs[2]-ls[2] };
                            if (VRIK_Norm3(right) < 1e-4f) return false;
                            float fwd[3]; VRIK_Cross3(up, right, fwd);
                            if (VRIK_Norm3(fwd) < 1e-4f) return false;
                            // Re-orthogonalise so the three stay a proper basis after the cross.
                            VRIK_Cross3(fwd, up, right); VRIK_Norm3(right);
                            for (int k = 0; k < 3; ++k) {
                                bodyUp[k] = up[k]; bodyRight[k] = right[k]; bodyFwd[k] = fwd[k];
                            }
                            return true;
                        };
                        if (!(CyberpunkVR_VrikArmAnchorFromBody && armFrameFromBody()) && camModelValid) {
                            VRIK_BodyAxesFromCamYaw(camModelHandRot, bodyRight, bodyUp, bodyFwd);
                        }
                        // BODY ANCHOR: intentionally NO smoothing (user-driven "glued to camera"
                        // design). The 3-tier low-pass + 0.02m/frame slew clamp that used to sit
                        // here made the body LAG the view on dash kicks / strafe sway / recoil
                        // (view moved first, body eased in over ~5+ frames = "тело двигается").
                        // camModelPos comes from a same-push coherent (cam - entity) pair now, so
                        // it is clean per tick and can be consumed raw: body and view move in the
                        // SAME frame, and even a real teleport is invisible because the view cuts
                        // simultaneously.
                        if (camModelValid && g_VRBodyUnderHMD && !vrikInVehicle) {
                            // CAMERA-MOUNT REMOVAL (user's idea): the HMD sits ~0.2 m FORWARD of the
                            // head bone because CP2077 mounts the FPP camera ahead of the head -- that
                            // is NOT the player leaning. So VRIK_PlaceBodyUnderHMD stands the body
                            // VERTICAL over the feet and uses only the HMD's HEIGHT (camModelPos.z) for
                            // squat; the horizontal position is the foot centre, computed inside. Head
                            // orientation still follows the HMD (camModelRot).
                            float fwdSigned[3] = { bodyFwd[0], bodyFwd[1], bodyFwd[2] };
                            // THE BODY STANDS UNDER THE GAMEPLAY HEAD, NOT UNDER THE VIEW -- and this
                            // pairs with CyberpunkVR_HeadTranslationInPatch, so the two must never be
                            // separated again.
                            //
                            // With the head displacement written into the camera COMPONENT, everything
                            // that calls itself "the camera" carries it, including the value Lua pushes
                            // and VRIK_ComputeCamModel turns into camModelPos. Place the body on that
                            // and it walks after the headset instead of standing in the room -- the
                            // "body glued to the HMD" symptom, and it comes back the moment this
                            // subtraction is missing while the flag is on (verified the hard way: the
                            // pose path was reverted to a state that predates the flag, and the body
                            // followed the head again immediately).
                            //
                            // So take it back out HERE, for the body only. The view keeps the
                            // displacement, the hands keep it (their anchor is the view), and the
                            // gameplay head returns to where the engine put it.
                            float bodyCamModelPos[3] = { camModelPos[0], camModelPos[1], camModelPos[2] };
                            if (CyberpunkVR_HeadTranslationInPatch &&
                                g_headDeltaValid.load(std::memory_order_acquire)) {
                                // Convert with the exact entity basis VRIK_ComputeCamModel used for
                                // camModelPos.  BodyYawFinal/current globals can already be one census
                                // newer than the coherent camera/entity push at this point.
                                float entQ[4] = {
                                    camModelEntityQuat[0], camModelEntityQuat[1],
                                    camModelEntityQuat[2], camModelEntityQuat[3] };
                                if ((entQ[0]*entQ[0] + entQ[1]*entQ[1] +
                                     entQ[2]*entQ[2] + entQ[3]*entQ[3]) < 1e-6f) {
                                    entQ[0] = 0.0f; entQ[1] = 0.0f; entQ[2] = 0.0f; entQ[3] = 1.0f;
                                }
                                VRIK_QuatNorm(entQ);
                                float invEnt[4];
                                VRIK_QuatConj(entQ, invEnt);        // world -> model
                                const float k = 1.0f / 131072.0f;
                                const float dw[3] = {
                                    g_headDeltaFP[0].load(std::memory_order_relaxed) * k,
                                    g_headDeltaFP[1].load(std::memory_order_relaxed) * k,
                                    g_headDeltaFP[2].load(std::memory_order_relaxed) * k };
                                float dm[3];
                                VRIK_QuatRotateVec(invEnt, dw, dm);
                                for (int di = 0; di < 3; ++di) bodyCamModelPos[di] -= dm[di];
                            }
                            VRIK_PlaceBodyUnderHMD(boneBuf, bodyCamModelPos, camModelRot, hIdx, fwdSigned);
                            // Re-taken AFTER the placement, so the frame reflects the body as it now
                            // stands rather than as the animation left it.
                            if (!(CyberpunkVR_VrikArmAnchorFromBody && armFrameFromBody())) {
                                VRIK_BodyAxesFromCamYaw(camModelHandRot, bodyRight, bodyUp, bodyFwd);
                            }

                            // Publish the horizontal CAMERA-MOUNT offset = (where the body stands =
                            // foot centre) - (RAW camera), so BakeCameraOffset moves the view + head
                            // back over the body. Using the RAW camera (pre-correction) and the foot
                            // centre makes it STABLE/idempotent: after baking, camModelPos = foot
                            // centre, and a re-bake measures the same mount again. [88] = valid.
                            bool fR = (g_VRRightFootIdx >= 0 && g_VRRightFootIdx < VRIK_MAX_BONES);
                            bool fL = (g_VRLeftFootIdx  >= 0 && g_VRLeftFootIdx  < VRIK_MAX_BONES);
                            if (g_pSharedHands && (fR || fL)) {
                                float fcx = 0.0f, fcy = 0.0f; int fn = 0;
                                if (fR) { fcx += g_fkPos[g_VRRightFootIdx][0]; fcy += g_fkPos[g_VRRightFootIdx][1]; ++fn; }
                                if (fL) { fcx += g_fkPos[g_VRLeftFootIdx][0];  fcy += g_fkPos[g_VRLeftFootIdx][1];  ++fn; }
                                if (fn > 0) { fcx /= fn; fcy /= fn; }
                                g_pSharedHands[85] = fcx - rawCamModelPos[0];
                                g_pSharedHands[86] = fcy - rawCamModelPos[1];
                                g_pSharedHands[87] = 0.0f;
                                g_pSharedHands[88] = 1.0f;
                            }
                        }

                        // Common arm-length estimate (rest-pose upper + forearm) for shoulder
                        // adjustment. F4VR uses a configured constant; we use the actual rig.
                        auto restArmLen = [&](int upper, int fore, int hand) -> float {
                            if (upper < 0 || fore < 0 || hand < 0
                             || upper >= VRIK_MAX_BONES || fore >= VRIK_MAX_BONES || hand >= VRIK_MAX_BONES) return 0.6f;
                            const float* a = g_fkPos[upper], *b = g_fkPos[fore], *c = g_fkPos[hand];
                            float dx1 = b[0]-a[0], dy1 = b[1]-a[1], dz1 = b[2]-a[2];
                            float dx2 = c[0]-b[0], dy2 = c[1]-b[1], dz2 = c[2]-b[2];
                            return std::sqrt(dx1*dx1+dy1*dy1+dz1*dz1) + std::sqrt(dx2*dx2+dy2*dy2+dz2*dz2);
                        };

                        // F4VR-STYLE SHOULDER ADJUSTMENT: when the controller is far from the
                        // resting shoulder, slide the SHOULDER itself a small amount toward the
                        // controller. This is the missing piece that made hardcoded poses look
                        // right in the original code — without it, reaching forward leaves the
                        // shoulder behind and the arm visibly stretches (or the elbow flips).
                        //
                        // Formula (per F4VR Skeleton.cpp:944-953):
                        //   stoH = handTarget - shoulder
                        //   adjust = clamp(|stoH| - armLen*0.5, 0, armLen*0.85) / (armLen*0.85)
                        //   shoulderAdj = shoulder + normalize(stoH) * (adjust * armLen * 0.08)
                        auto adjustShoulder = [&](const float* sh, const float* hand, float armLen, float* outSh) {
                            float st[3] = { hand[0]-sh[0], hand[1]-sh[1], hand[2]-sh[2] };
                            float l = std::sqrt(st[0]*st[0]+st[1]*st[1]+st[2]*st[2]);
                            if (l < 1e-5f || armLen < 1e-4f) {
                                outSh[0]=sh[0]; outSh[1]=sh[1]; outSh[2]=sh[2]; return;
                            }
                            float adj = (l - armLen * 0.5f) / (armLen * 0.85f);
                            if (adj < 0.0f) adj = 0.0f; if (adj > 1.0f) adj = 1.0f;
                            float dotUp = (st[0]*bodyUp[0] + st[1]*bodyUp[1] + st[2]*bodyUp[2]) / l;
                            float downFactor = (dotUp + 0.6f) / 0.4f;
                            if (downFactor < 0.0f) downFactor = 0.0f; if (downFactor > 1.0f) downFactor = 1.0f;
                            // FULL-EXTENSION FADE. adjustShoulder slides the shoulder TOWARD the hand,
                            // which SHORTENS shoulder->target distance right before VRIK_SolveArm reads
                            // the (moved) FK shoulder against the (un-recomputed) target. Lateral T-pose
                            // is calibrated to EXACTLY arm length (openxr_manager: spanArm=(armSpan-2*
                            // shoulderHalf)/2, bones scaled to spanArm), so that stolen ~3cm pushes
                            // hsLen just under armLen and the elbow stays bent -- "T-pose arms bent while
                            // forward arms (which have reach margin) stay straight". Fade the shoulder
                            // slide to 0 as the reach approaches full extension so it can't steal the
                            // last bit of length: full effect below l/armLen=0.90, zero by ~0.97.
                            float extFade = (0.97f - l / armLen) / 0.07f;
                            if (extFade < 0.0f) extFade = 0.0f; if (extFade > 1.0f) extFade = 1.0f;
                            float k = adj * armLen * 0.08f / l * downFactor * extFade;
                            outSh[0] = sh[0] + st[0] * k;
                            outSh[1] = sh[1] + st[1] * k;
                            outSh[2] = sh[2] + st[2] * k;
                        };
                        // WEAPON-POSE-IMMUNE HEAD REFERENCE for the shoulder anchors. They used to
                        // hang off the ANIMATED head bone + a 20% live-FK blend: drawing a weapon
                        // plays an upper-body pose that moves both, so the arm roots migrated toward
                        // the neck ("руки выходят из шеи"). Freeze the (head bone - camera) relation
                        // instead: measured in body axes over the first ~90 valid frames, then kept
                        // CONSTANT -- headRef = smoothedCam + frozenOffset. The camera is gameplay-
                        // driven (and stabilized), so no game animation can move the anchors anymore.
                        float headRef[3] = { headModelPos[0], headModelPos[1], headModelPos[2] };
                        if (camModelValid) {
                            static float s_hOffB[3] = {0, 0, 0};
                            static int   s_hOffN = 0;
                            if (s_hOffN < 90) {
                                const float dw[3] = { headModelPos[0]-camModelPos[0],
                                                      headModelPos[1]-camModelPos[1],
                                                      headModelPos[2]-camModelPos[2] };
                                const float b0 = VRIK_Dot3(dw, bodyRight);
                                const float b1 = VRIK_Dot3(dw, bodyFwd);
                                const float b2 = VRIK_Dot3(dw, bodyUp);
                                const float k = 1.0f / static_cast<float>(s_hOffN + 1);
                                s_hOffB[0] += (b0 - s_hOffB[0]) * k;
                                s_hOffB[1] += (b1 - s_hOffB[1]) * k;
                                s_hOffB[2] += (b2 - s_hOffB[2]) * k;
                                ++s_hOffN;
                            }
                            headRef[0] = camModelPos[0] + bodyRight[0]*s_hOffB[0] + bodyFwd[0]*s_hOffB[1] + bodyUp[0]*s_hOffB[2];
                            headRef[1] = camModelPos[1] + bodyRight[1]*s_hOffB[0] + bodyFwd[1]*s_hOffB[1] + bodyUp[1]*s_hOffB[2];
                            headRef[2] = camModelPos[2] + bodyRight[2]*s_hOffB[0] + bodyFwd[2]*s_hOffB[1] + bodyUp[2]*s_hOffB[2];
                        }
                        auto calibratedShoulderModel = [&](const float* shoulderBody, int upperIdxUnused, float* outSh) {
                            (void)upperIdxUnused;   // live-FK blend removed: 100% calibrated anchor
                            outSh[0] = headRef[0] + bodyRight[0]*shoulderBody[0] + bodyUp[0]*shoulderBody[1] - bodyFwd[0]*shoulderBody[2];
                            outSh[1] = headRef[1] + bodyRight[1]*shoulderBody[0] + bodyUp[1]*shoulderBody[1] - bodyFwd[1]*shoulderBody[2];
                            outSh[2] = headRef[2] + bodyRight[2]*shoulderBody[0] + bodyUp[2]*shoulderBody[1] - bodyFwd[2]*shoulderBody[2];
                        };
                        auto applyShoulderAnchor = [&](int upperIdx, const float* sh) {
                            int parent = (upperIdx >= 0 && upperIdx < VRIK_MAX_BONES) ? g_VRBoneParent[upperIdx] : -1;
                            if (parent >= 0 && parent < VRIK_MAX_BONES) {
                                VRIK_WriteLocalPos(boneBuf, upperIdx, g_fkPos[parent], g_fkRot[parent], sh);
                            }
                            g_fkPos[upperIdx][0] = sh[0];
                            g_fkPos[upperIdx][1] = sh[1];
                            g_fkPos[upperIdx][2] = sh[2];
                            VRIK_ComputeFK(boneBuf, VRIK_FKCount());
                        };

                        // SHOULDER GIRDLE, ROTATION-BASED (redesign step 2). The old anchor WROTE
                        // POSITIONS for the clavicle (45% width) and the shoulder joint (100%),
                        // which teleported/stretched the girdle off its pivots (measured 0.246m
                        // clavicle local translation -> "armpit stretched up" mush). Anatomically
                        // the clavicle ROTATES about its sternum-side pivot and the shoulder joint
                        // rides its end at a FIXED radius. So: rotate the clavicle bone toward the
                        // desired joint point (capped ~35deg from the native pose), never write any
                        // position, and return wherever the joint FK lands as the IK root. Width /
                        // drop now only shape the DESIRED DIRECTION; no stretch is possible.
                        auto anchorStableShoulder = [&](int upperIdx, const float* anchor, bool isLeft, float* outJoint) {
                            float half = std::fabs(g_VRShoulderRX);
                            if (half < 0.13f) half = 0.13f;
                            if (half > 0.19f) half = 0.19f;
                            const float drop = 0.17f;
                            float side = isLeft ? -1.0f : 1.0f;
                            float desired[3] = {
                                anchor[0] + bodyRight[0]*(side*half) - bodyUp[0]*drop,
                                anchor[1] + bodyRight[1]*(side*half) - bodyUp[1]*drop,
                                anchor[2] + bodyRight[2]*(side*half) - bodyUp[2]*drop };
                            int clavi = (upperIdx >= 0 && upperIdx < VRIK_MAX_BONES) ? g_VRBoneParent[upperIdx] : -1;
                            const int dbgSide = isLeft ? 1 : 0;
                            g_VRIKDbgClav[dbgSide][0]=desired[0]; g_VRIKDbgClav[dbgSide][1]=desired[1]; g_VRIKDbgClav[dbgSide][2]=desired[2];
                            g_VRIKDbgClav[dbgSide][6]=0.0f; g_VRIKDbgClav[dbgSide][7]=0.0f;
                            if (clavi >= 0 && clavi < VRIK_MAX_BONES) {
                                // WEAPON-STANCE TRANSLATION RESET. Armed poses write local
                                // TRANSLATIONS on the clavicle/upper-arm bones (measured ~15cm on
                                // bone[15] with a pistol), dragging the shoulder PIVOT to the neck.
                                // The rotation aim below cannot fix a moved pivot: diag showed
                                // need=0.2deg "already aligned" while the joint sat 12cm inboard of
                                // desired. Fix at the source: remember the girdle's local
                                // translations at the WIDEST stance seen (relaxed/unarmed) and
                                // restore them whenever the current stance is narrower (armed hunch).
                                // Self-calibrating per rig -- no hardcoded bind values needed.
                                {
                                    const int pp2 = g_VRBoneParent[clavi];
                                    float* clavT = reinterpret_cast<float*>(boneBuf + clavi * 48 + VRIK_TRANS_OFF);
                                    float* armT  = reinterpret_cast<float*>(boneBuf + upperIdx * 48 + VRIK_TRANS_OFF);
                                    static float s_refClav[2][3];
                                    static float s_refArm[2][3];
                                    static float s_refWidth[2] = { -1.0f, -1.0f };
                                    const float* base = (pp2 >= 0 && pp2 < VRIK_MAX_BONES) ? g_fkPos[pp2] : g_fkPos[clavi];
                                    const float rel[3] = { g_fkPos[upperIdx][0] - base[0],
                                                           g_fkPos[upperIdx][1] - base[1],
                                                           g_fkPos[upperIdx][2] - base[2] };
                                    const float width = std::fabs(VRIK_Dot3(rel, bodyRight));
                                    s_refWidth[dbgSide] *= 0.9995f;   // slow decay: re-adapts in ~1min
                                    if (width >= s_refWidth[dbgSide] - 0.02f) {
                                        // At (or near) the widest stance: (re)capture the reference.
                                        s_refClav[dbgSide][0] = clavT[0]; s_refClav[dbgSide][1] = clavT[1]; s_refClav[dbgSide][2] = clavT[2];
                                        s_refArm[dbgSide][0]  = armT[0];  s_refArm[dbgSide][1]  = armT[1];  s_refArm[dbgSide][2]  = armT[2];
                                        if (width > s_refWidth[dbgSide]) s_refWidth[dbgSide] = width;
                                    } else if (s_refWidth[dbgSide] - width > 0.03f) {
                                        // Narrow (weapon) stance: restore the relaxed girdle geometry.
                                        clavT[0] = s_refClav[dbgSide][0]; clavT[1] = s_refClav[dbgSide][1]; clavT[2] = s_refClav[dbgSide][2];
                                        armT[0]  = s_refArm[dbgSide][0];  armT[1]  = s_refArm[dbgSide][1];  armT[2]  = s_refArm[dbgSide][2];
                                        VRIK_ComputeFK(boneBuf, VRIK_FKCount());
                                    }
                                }
                                const float* pv = g_fkPos[clavi];
                                float cur[3] = { g_fkPos[upperIdx][0]-pv[0], g_fkPos[upperIdx][1]-pv[1], g_fkPos[upperIdx][2]-pv[2] };
                                float des[3] = { desired[0]-pv[0], desired[1]-pv[1], desired[2]-pv[2] };
                                if (VRIK_Norm3(cur) > 1e-4f && VRIK_Norm3(des) > 1e-4f) {
                                    float d4[4]; VRIK_QuatFromTo(cur, des, d4);
                                    if (d4[3] < 0.0f) { d4[0]=-d4[0]; d4[1]=-d4[1]; d4[2]=-d4[2]; d4[3]=-d4[3]; }
                                    float ang = 2.0f * std::acos(std::fmin(1.0f, d4[3]));
                                    // 75 deg cap (was 35). The cap is measured FROM THE LIVE ANIMATED
                                    // pose: weapon-ready stances hunch the clavicles inward by MORE
                                    // than 35 deg, so the correction saturated and the shoulder joint
                                    // stayed collapsed at the neck ("рука строится из шеи") while
                                    // armed. 75 deg reaches the anchor from every stance; the cap now
                                    // only guards against a genuinely broken anchor.
                                    const float kMaxClav = 1.3090f;   // 75 deg from the native pose
                                    float applied = ang;
                                    if (ang > kMaxClav && ang > 1e-4f) { VRIK_QuatScale(d4, kMaxClav/ang, d4); applied = kMaxClav; }
                                    g_VRIKDbgClav[dbgSide][6] = ang * 57.29578f;
                                    g_VRIKDbgClav[dbgSide][7] = applied * 57.29578f;
                                    float nm[4]; VRIK_QuatMul(d4, g_fkRot[clavi], nm); VRIK_QuatNorm(nm);
                                    int pp = g_VRBoneParent[clavi];
                                    float idq[4] = { 0,0,0,1 };
                                    VRIK_WriteLocalRot(boneBuf, clavi, (pp>=0&&pp<VRIK_MAX_BONES)?g_fkRot[pp]:idq, nm);
                                    VRIK_ComputeFK(boneBuf, VRIK_FKCount());
                                }
                            }
                            outJoint[0]=g_fkPos[upperIdx][0]; outJoint[1]=g_fkPos[upperIdx][1]; outJoint[2]=g_fkPos[upperIdx][2];
                            g_VRIKDbgClav[dbgSide][3]=outJoint[0]; g_VRIKDbgClav[dbgSide][4]=outJoint[1]; g_VRIKDbgClav[dbgSide][5]=outJoint[2];
                            if (!isLeft) {
                                // Trace probes: right shoulder joint (model) + hips MODEL yaw.
                                g_VRIKDbgShModel[0] = outJoint[0];
                                g_VRIKDbgShModel[1] = outJoint[1];
                                g_VRIKDbgShModel[2] = outJoint[2];
                                const int hb = g_VRHipsIdx;
                                if (hb >= 0 && hb < VRIK_MAX_BONES) {
                                    // Yaw of the hips bone in model space: heading of its
                                    // local +X axis (rig lateral) projected to the ground.
                                    const float* q = g_fkRot[hb];
                                    const float axX = 1.0f - 2.0f*(q[1]*q[1] + q[2]*q[2]);
                                    const float axY = 2.0f*(q[0]*q[1] + q[2]*q[3]);
                                    g_VRIKDbgHipsYaw = std::atan2(axY, axX) * 57.29578f;
                                }
                            }
                        };

                        // ANTI-SHAKE arm anchor. headModelPos is the median-of-3 of the ANIMATED FK
                        // head bone; the median only kills a 1-in-3 outlier, NOT the continuous
                        // sprint/jog sinusoidal body-bob the game's locomotion camera/anim injects,
                        // so anchoring the shoulders to it made the hands shake while running. The
                        // smoothed camModelPos carries NO bone bob (camera-bob disabled + 3-tier
                        // low-pass above), so anchor the shoulder girdle to it instead, at the same
                        // head height the body-under-HMD placement uses (camModelPos.z + headDrop -
                        // shared smoothed squat). XY stays at the camera (kNeckBehind=0). Only the
                        // ANCHOR POSITION changes here -- the bodyRight/Up/Fwd yaw frame is left
                        // exactly as computed from the live camera yaw, so turning still works.
                        const float* stableAnchor = headModelPos;   // default: legacy FK head
                        float stableAnchorBuf[3];
                        // THE GIRDLE HANGS OFF THE BODY, NOT OFF THE CAMERA.
                        //
                        // Anchoring it at camModelPos ties the shoulders to the HMD: they move with the
                        // head, and after the camera-onto-head bake -- which is a real (0.093, -0.428)
                        // shift, measured -- they went half a metre BACK with it and dragged the chest
                        // and armpit after them.
                        //
                        // The neck bone is the honest anchor: it IS the top of the spine the girdle sits
                        // on, and by this point the body has already been placed and the spine bent, so
                        // its FK carries the squat and the lean without carrying the camera mount or any
                        // head rotation. The bob objection that pushed this to the camera in the first
                        // place applies to the HEAD bone (it rides the locomotion animation); the neck
                        // sits below the head-bone anim and under our own placement.
                        //
                        // Chest (topmost spine) is the fallback when there is no neck bone, and the
                        // camera-based value stays as the last resort so a rig without either still
                        // solves.
                        int anchorBone = -1;
                        if (CyberpunkVR_VrikArmAnchorFromBody) {
                            if (g_VRNeckIdx >= 0 && g_VRNeckIdx < VRIK_MAX_BONES) {
                                anchorBone = g_VRNeckIdx;
                            } else if (g_VRSpineCount > 0) {
                                const int top = g_VRSpineIdx[g_VRSpineCount - 1];
                                if (top >= 0 && top < VRIK_MAX_BONES) anchorBone = top;
                            }
                        }
                        if (anchorBone >= 0) {
                            stableAnchorBuf[0] = g_fkPos[anchorBone][0];
                            stableAnchorBuf[1] = g_fkPos[anchorBone][1];
                            stableAnchorBuf[2] = g_fkPos[anchorBone][2];
                            stableAnchor = stableAnchorBuf;
                        } else if (camModelValid) {
                            const float kNeckBehind = 0.0f;
                            stableAnchorBuf[0] = camModelPos[0] - bodyFwd[0]*kNeckBehind;
                            stableAnchorBuf[1] = camModelPos[1] - bodyFwd[1]*kNeckBehind;
                            stableAnchorBuf[2] = camModelPos[2] + g_VRHeadDrop - s_vrSharedSquatDrop;
                            stableAnchor = stableAnchorBuf;
                        }

                        // Right arm.
                        if (SharedPose(8) > 0.0f && g_VRRightUpperArmIdx >= 0) {
                            const float vrPos[3]  = { SharedPose(9),  SharedPose(10), SharedPose(11) };
                            if (CyberpunkVR_XrDeepDiag) VRIK_NoteShake(1, 0, vrPos);   // stage 0: tracking, as read
                            const float vrQuat[4] = { SharedPose(12), SharedPose(13), SharedPose(14), SharedPose(15) };
                            float target[3], handRot[4];
                            // Scratch for the hand diagnostic further down (the stop zone's shift).
                            float dbgTargetPreStopR[3] = {0.0f, 0.0f, 0.0f};
                            bool  dbgHandDiagR = false;
                            const float wristR[4]       = { g_VRWristR_I, g_VRWristR_J, g_VRWristR_K, g_VRWristR_R };
                            const float offR[3]         = { g_VROffRX, g_VROffRY, g_VROffRZ };
                            const float shoulderBodyR[3]= { g_VRShoulderRX, g_VRShoulderRY, g_VRShoulderRZ };
                            float armLenR = restArmLen(g_VRRightUpperArmIdx, g_VRRightForeArmIdx, g_VRRightBoneIdx);
                            float shoulderModelR[3];
                            if (wheelOffR) {
                                // WHEEL GRAB: this arm belongs to the animation, and so does the
                                // girdle it hangs from. anchorStableShoulder ROTATES the clavicle
                                // toward our body-frame anchor -- leave it running and the arm we
                                // deliberately stopped solving is still swung off the wheel by its
                                // own shoulder. Read the animated joint instead, write nothing.
                                const int ui = g_VRRightUpperArmIdx;
                                shoulderModelR[0] = g_fkPos[ui][0];
                                shoulderModelR[1] = g_fkPos[ui][1];
                                shoulderModelR[2] = g_fkPos[ui][2];
                                VRIK_BuildHandTarget(shoulderModelR, shoulderBodyR, hmdRel, vrPos, vrQuat,
                                                     wristR, g_VRScaleR, offR, target, handRot);
                            } else if (camModelValid && headModelPos) {
                                // CONFIRMED-WORKING path (2026-06-05: "head-coupling GONE, tracking +
                                // weapon great"). hmdRel (HMD orientation rel the recenter base) cancels
                                // head rotation MATHEMATICALLY, so there is NO inversion when you turn --
                                // unlike the gizmo/camQuat formulas, which ride the pitch-locked game
                                // camera and drift/invert. Anchor at the stable body-frame shoulder.
                                anchorStableShoulder(g_VRRightUpperArmIdx, stableAnchor, /*isLeft*/false, shoulderModelR);
                                VRIK_BuildHandTarget(shoulderModelR, shoulderBodyR, hmdRel, vrPos, vrQuat,
                                                     wristR, g_VRScaleR, offR, target, handRot);
                            } else {
                                calibratedShoulderModel(shoulderBodyR, g_VRRightUpperArmIdx, shoulderModelR);
                                applyShoulderAnchor(g_VRRightUpperArmIdx, shoulderModelR);
                                VRIK_BuildHandTarget(shoulderModelR, shoulderBodyR, hmdRel, vrPos, vrQuat, wristR, g_VRScaleR, offR, target, handRot);
                            }

                            // HAND == GIZMO (user principle: the in-game hand must EQUAL the visible
                            // gizmo = the real hand; the rest of the arm adapts). Position AND
                            // orientation from the exact gizmo formula in model space: raw camera +
                            // camModelRot * mapped(controller HMD-local). If a head-pitch/turn
                            // inversion shows, the fix is in the FRAME COMPOSITION (independent
                            // research running), not a return to shoulder-relative targets.
                            // View (HMD) pose in model space, captured from the gizmo path below so the
                            // mouth anchor (after the solve) can ride the real head, not the idle head bone.
                            float vrViewPosM[3] = {0.0f,0.0f,0.0f}; float vrViewRotM[4] = {0.0f,0.0f,0.0f,1.0f}; bool vrViewFrameOk = false;
                            if (camModelValid) {
                                // Anchor at the BODY camera (baked+smoothed camModelPos -- the same
                                // camera the body/shoulders hang from), NOT the raw camera: the raw
                                // camera sits ~0.37m in FRONT of the torso (camera mount), which put
                                // the target ~0.8m from the shoulder = permanently unreachable ->
                                // max-stretch arms. Anchored here, hand-relative-to-VIEW equals
                                // gizmo-relative-to-view (what the player perceives) AND the arm
                                // geometry is reachable.
                                // VIEW-FRAME composition, PROVEN by the two static head-turn diags:
                                // cam.quat/entityQuat DO NOT rotate with physical head yaw (identical
                                // across a 40deg turn) -- head yaw lives ONLY in hmdRel, and dxgi
                                // composes the rendered view as heading*hmdRel. Composing the raw
                                // HMD-local controller with the heading alone (previous build) provably
                                // swung the target 25cm on a head turn (controller motionless). The
                                // matching hand frame is heading*hmdRel:
                                //   target = anchor + camModelRot * map(hmdRel * vrPos)
                                // hmdRel*vrPos = controller in the recenter-base frame: motionless under
                                // head yaw AND pitch (only the real neck-lever eye translation remains).
                                float vpView[3];
                                if (VRIK_ResolveViewPos(vpView)) {
                                    // RENDER-VIEW ORIGIN (praydog single-origin, exact): dxgi publishes
                                    // the FINAL view pose it renders this frame from ([104..107] quat,
                                    // [108..110] pos, game world axes) -- including head translation,
                                    // Tracking-Camera sliders, camBake, eye-view and BOTH rotation
                                    // modes' composition. target = View ∘ map(controller HMD-local).
                                    // The avatar hand projects onto the HMD-space gizmo overlay by
                                    // construction; nothing is re-derived, so nothing can drift.
                                    // Position goes through VRIK_ResolveViewPos (fixed-point scale
                                    // auto-detect + 2m sanity gate vs the known-good camera).
                                    // vq via the hands snapshot -- THE path that was trail-free
                                    // in user testing (reverted to it by user order; the packet
                                    // source trailed the rendered view by one sample).
                                    // ONE LATCH FOR THE WHOLE FRAME OF REFERENCE. vq used to be
                                    // read from the hands snapshot while the heading and the head
                                    // orientation beside it came from the view packet -- two
                                    // seqlocks, two instants. The comment below still claims they
                                    // are the same packet; they were not. With the camera doing
                                    // its idle sway the two drift apart, and the difference lands
                                    // straight on the hand's ROTATION: measured 0.44 deg per frame
                                    // with the controllers lying on the table.
                                    //
                                    // Taking vq from the packet was tried before and left a trail,
                                    // because the packet lags the rendered view by a sample. That
                                    // objection is gone: the alignment below replaces the packet's
                                    // head part with hmdRel from the hand sample, so what is used
                                    // is the packet's heading with the hands' own head rotation.
                                    // REVERTED 2026-07-30, by measurement: sourcing vq from the
                                    // packet instead of the snapshot took VIEWREL rot from 0.44 to
                                    // 0.95 deg per frame with the controllers lying still. The
                                    // packet updates on the locate cadence, the snapshot on the
                                    // publish cadence, and the snapshot is the closer of the two
                                    // to the frame being solved. The cross-latch mixing with the
                                    // heading is real, but this was the wrong half to move.
                                    // ONE LATCH FOR BOTH HALVES OF THE RE-BASING BELOW.
                                    //
                                    // The time-align a few lines down computes
                                    //     vqUse = vq * mapQ(conj(headOri_view) * hmdRel)
                                    // which only cancels the head rotation if vq was BUILT from the
                                    // very sample headOri_view names. It was not. vq came out of the
                                    // hands snapshot (RefreshHandsSnapshot, seqlock [127]) while
                                    // headOri_view comes out of the view packet (VRIK_LatchViewPacket,
                                    // seqlock [143]) -- two captures of the same publisher at two
                                    // moments. Worse, the snapshot has a fast path that returns unless
                                    // [127] moved, so its copy of [104..107] refreshes at the HANDS'
                                    // cadence, not the view's. What was left in the hand frame was
                                    // V_snapshot * conj(V_packet): the head rotation between two view
                                    // publications. Zero standing still, degrees during a head turn,
                                    // centimetres at the end of a 0.6 m arm -- head-turn only, by
                                    // construction, which is exactly how it was reported.
                                    //
                                    // [104..107] also sits OUTSIDE the [127] bracket by that latch's
                                    // own comment, so the snapshot could tear it as well.
                                    //
                                    // Taking vq from the packet puts numerator and denominator in one
                                    // publication. Nothing is lost: the snap-event block rotates
                                    // g_viewPkt[0..3] and mirrors the same values into
                                    // g_handsStable[104..107], so both carried the snap rotation.
                                    float vq[4];
                                    if (CyberpunkVR_VrikHandFrameOneLatch && g_viewPktValid) {
                                        vq[0] = g_viewPkt[0]; vq[1] = g_viewPkt[1];
                                        vq[2] = g_viewPkt[2]; vq[3] = g_viewPkt[3];
                                    } else {
                                        vq[0] = SharedPose(104); vq[1] = SharedPose(105);
                                        vq[2] = SharedPose(106); vq[3] = SharedPose(107);
                                    }
                                    VRIK_QuatNorm(vq);
                                    // Exhale-smoke mouth point in WORLD space = vpView + rotate(vq, mouthOffset),
                                    // the same view pose the cig mouth-anchor rides. redscript spawns the smoke
                                    // fx here so it tracks the real HMD (not the lean-less FPP camera).
                                    // Publish the view pose itself, before any offset is applied.
                                    g_VRViewWorldPos[0] = vpView[0];
                                    g_VRViewWorldPos[1] = vpView[1];
                                    g_VRViewWorldPos[2] = vpView[2];
                                    g_VRViewWorldRot[0] = vq[0];
                                    g_VRViewWorldRot[1] = vq[1];
                                    g_VRViewWorldRot[2] = vq[2];
                                    g_VRViewWorldRot[3] = vq[3];
                                    g_VRViewWorldValid = 1;
                                    {
                                        const float mo[3] = { g_VRSmokeSmokePos[0], g_VRSmokeSmokePos[1], g_VRSmokeSmokePos[2] };
                                        float moW[3]; VRIK_QuatRotateVec(vq, mo, moW);
                                        g_VRSmokeMouthWorldPos[0] = vpView[0] + moW[0];
                                        g_VRSmokeMouthWorldPos[1] = vpView[1] + moW[1];
                                        g_VRSmokeMouthWorldPos[2] = vpView[2] + moW[2];
                                        const float sr[4] = { g_VRSmokeSmokeRot[0], g_VRSmokeSmokeRot[1], g_VRSmokeSmokeRot[2], g_VRSmokeSmokeRot[3] };
                                        float wr[4]; VRIK_QuatMul(vq, sr, wr); VRIK_QuatNorm(wr);
                                        g_VRSmokeMouthWorldRot[0] = wr[0];
                                        g_VRSmokeMouthWorldRot[1] = wr[1];
                                        g_VRSmokeMouthWorldRot[2] = wr[2];
                                        g_VRSmokeMouthWorldRot[3] = wr[3];
                                        g_VRSmokeMouthWorldValid = 1;
                                    }
                                    // WORLD->MODEL yaw = game heading from the SAME view
                                    // packet as vq (one seqlocked frame: no snap-turn
                                    // old-vq/new-yaw mix -> no arm double). No HMD yaw
                                    // in any mode: head turns do not rotate the hand
                                    // frame. Fallback (older dxgi): yaw extracted from vq.
                                    float vyaw;
                                      if (g_viewPktValid) {
                                        vyaw = g_viewPkt[8];
                                    } else {
                                        const float fX = 2.0f*(vq[0]*vq[1] - vq[3]*vq[2]);
                                        const float fY = 1.0f - 2.0f*(vq[0]*vq[0] + vq[2]*vq[2]);
                                        vyaw = std::atan2(-fX, fY);
                                    }
                                    // MODEL SPACE IS THE ENTITY FRAME, so the converter has to be the
                                    // body's TRUE yaw. With physical body rotation on those two part
                                    // company by design: the body carries the engine's heading while
                                    // the view deliberately holds back the realign we injected into it
                                    // (src/Hooks/BodyYawFollow.cpp). Using the view's heading here
                                    // would leave every hand target rotated about the body by exactly
                                    // that angle -- the hands riding with the body. The census value is
                                    // also self-correcting: it is what the engine ACTUALLY ended up at,
                                    // so a heading it clamps or eases still leaves the hands put.
                                    if (CyberpunkVR_BodyYawFollow && CyberpunkVR_BodyYawFinalValid) {
                                        vyaw = CyberpunkVR_BodyYawFinalRad;
                                    }
                                    const float hs = std::sin(vyaw * 0.5f);
                                    const float hc = std::cos(vyaw * 0.5f);
                                    float ec[4] = { 0.0f, 0.0f, -hs, hc };
                                    // TIME-ALIGN THE HAND FRAME. vq is the view orientation
                                    // from [104..107], written at LocateCamera; the offset it is
                                    // about to rotate came from the XR sample the hands were
                                    // published with. Measured: 33.6 ms against 23.5 ms, and at
                                    // 886 deg/s that 10 ms gap is ~9 deg -- on a half-metre arm,
                                    // 8 cm of hand displacement, changing every frame. The whole
                                    // reason the offset is stored head-locally is so head
                                    // rotation cancels; it only cancels if the two halves are
                                    // from one instant.
                                    //
                                    // vq = heading * mapQ(headOri_view), so replacing its head
                                    // part with the sample's is a right-multiply by
                                    // mapQ(conj(headOri_view) * hmdRel). Nothing is assumed about
                                    // how the heading is built -- it divides out.
                                    float vqUse[4] = { vq[0], vq[1], vq[2], vq[3] };
                                    if (g_viewPktValid && CyberpunkVR_VrikHandFrameAlign) {
                                          const float* vo = &g_viewPkt[13];
                                        if (vo[0] != 0.0f || vo[1] != 0.0f || vo[2] != 0.0f ||
                                            vo[3] != 1.0f) {
                                            const float voc[4] = { -vo[0], -vo[1], -vo[2], vo[3] };
                                            float dxr[4]; VRIK_QuatMul(voc, hmdRel, dxr);
                                            const float dg[4] = { dxr[0], -dxr[2], dxr[1], dxr[3] };
                                            float t2[4]; VRIK_QuatMul(vq, dg, t2);
                                            VRIK_QuatNorm(t2);
                                            vqUse[0]=t2[0]; vqUse[1]=t2[1]; vqUse[2]=t2[2]; vqUse[3]=t2[3];
                                        }
                                    }
                                    float rvM[4]; VRIK_QuatMul(ec, vqUse, rvM); VRIK_QuatNorm(rvM);
                                    // THE VIEW-FRAME BRANCH TAKES THE VIEW ORIGIN, NOT THE ANCHOR.
                                    // 64e706fc replaced this with `handAnchor` to get the Lua globals
                                    // out of the IK target, and that part of its reasoning stands. But
                                    // handAnchor is camModelPos PLUS the view-only offsets
                                    // ([120..122] - [91..93]), so assigning it here collapsed this
                                    // branch onto the fallback below and added those offsets to the
                                    // target. Bisected in the headset: bd0f5aca alone is clean,
                                    // bd0f5aca+64e706fc puts both hands visibly ABOVE the gizmo.
                                    // The view origin is what the gizmo is drawn from, so it is what
                                    // the hand must be built on.
                                    float vpW[3] = { vpView[0] - g_VREntityPosX,
                                                     vpView[1] - g_VREntityPosY,
                                                     vpView[2] - g_VREntityPosZ };
                                    float vpM[3]; VRIK_QuatRotateVec(ec, vpW, vpM);
                                    vrViewPosM[0]=vpM[0]; vrViewPosM[1]=vpM[1]; vrViewPosM[2]=vpM[2];
                                    vrViewRotM[0]=rvM[0]; vrViewRotM[1]=rvM[1]; vrViewRotM[2]=rvM[2]; vrViewRotM[3]=rvM[3];
                                    vrViewFrameOk = true;
                                    float mp[3] = { vrPos[0]*g_VRScaleR, -vrPos[2]*g_VRScaleR, vrPos[1]*g_VRScaleR };
                                    float rp[3]; VRIK_QuatRotateVec(rvM, mp, rp);
                                    target[0] = vpM[0] + rp[0] + offR[0];
                                    target[1] = vpM[1] + rp[1] + offR[1];
                                    target[2] = vpM[2] + rp[2] + offR[2];
                                    float lq[4] = { vrQuat[0], -vrQuat[2], vrQuat[1], vrQuat[3] };
                                    float hm[4]; VRIK_QuatMul(rvM, lq, hm);
                                    // TWO-HAND GRIP: the support hand's own controller, brought into model
                                    // space by the SAME transform this hand just used, so the two positions
                                    // are comparable to the millimetre. Then the barrel is turned onto the
                                    // line between them, which is why this sits BEFORE handRot is composed --
                                    // the weapon rides the hand bone, so aiming it means aiming this.
                                    // THE KICK GOES ONTO `hm`, AND IT GOES ON FIRST.
                                    //
                                    // It used to be applied to handRot further down, which is the same
                                    // rotation by the same angle -- kq*(hm*wristR) == (kq*hm)*wristR -- but
                                    // in the wrong ORDER: the two-hand support point is built from `hm` a
                                    // few lines below, so a kick applied afterwards never reached it. The
                                    // weapon rose and the hand welded to it stayed put, which is the one
                                    // thing a hand holding a gun cannot do.
                                    {
                                        float backM = 0.0f, riseRad = 0.0f;
                                        RecoilSample(1, 1, &backM, &riseRad);
                                        if (riseRad != 0.0f) {
                                            const float rightL[3] = { 1.0f, 0.0f, 0.0f };
                                            float ax[3]; VRIK_QuatRotateVec(hm, rightL, ax);
                                            const float h2 = riseRad * 0.5f;
                                            const float s2 = std::sin(h2);
                                            const float kq[4] = { ax[0]*s2, ax[1]*s2, ax[2]*s2, std::cos(h2) };
                                            float kr[4]; VRIK_QuatMul(kq, hm, kr); VRIK_QuatNorm(kr);
                                            hm[0]=kr[0]; hm[1]=kr[1]; hm[2]=kr[2]; hm[3]=kr[3];
                                        }
                                        if (backM != 0.0f) {
                                            const float fwdL[3] = { 0.0f, 1.0f, 0.0f };
                                            float fwd[3]; VRIK_QuatRotateVec(hm, fwdL, fwd);
                                            target[0] -= fwd[0] * backM;
                                            target[1] -= fwd[1] * backM;
                                            target[2] -= fwd[2] * backM;
                                        }
                                    }
                                    {
                                        const float lp[3] = { SharedPose(1), SharedPose(2), SharedPose(3) };
                                        const float lmp[3] = { lp[0]*g_VRScaleL, -lp[2]*g_VRScaleL, lp[1]*g_VRScaleL };
                                        float lrp[3]; VRIK_QuatRotateVec(rvM, lmp, lrp);
                                        const float lCtrl[3] = { vpM[0] + lrp[0] + g_VROffLX,
                                                                 vpM[1] + lrp[1] + g_VROffLY,
                                                                 vpM[2] + lrp[2] + g_VROffLZ };
                                        cvr::anim::TwoHandRight(target, hm, wristR, lCtrl);
                                    }
                                    VRIK_QuatMul(hm, wristR, handRot); VRIK_QuatNorm(handRot);

                                    // RECOIL, ADDED TO THE TARGET -- the only place it can survive.
                                    //
                                    // Everything below this writes the wrist from the controller, so a kick
                                    // applied to the bones would be overwritten in the same solve; applied to
                                    // the target, the arm IK carries it up the chain and the weapon, being
                                    // parented to the hand, goes with it.
                                    //
                                    // `hm` is the CONTROLLER's orientation in model space, before the wrist
                                    // correction -- so its +Y is where the weapon points (the same axis map the
                                    // camera uses: XR -Z forward -> game +Y) and its +X is the hand's right.
                                    // Taking the axes from here rather than from handRot keeps the kick along
                                    // the barrel whatever the per-weapon wrist correction does.
                                    {
                                        float backM = 0.0f, riseRad = 0.0f;
                                        // MOVED UP, onto `hm`, so the weapon is already kicked when the
                                        // two-hand support point is built from it. Same rotation, earlier.
                                        (void)backM; (void)riseRad;
                                    }

                                    // Kept for the two numbers below: where the controller IS in the
                                    // world, and what the target was before anything downstream of
                                    // here could move it.
                                    {
                                        dbgTargetPreStopR[0] = target[0];
                                        dbgTargetPreStopR[1] = target[1];
                                        dbgTargetPreStopR[2] = target[2];
                                        dbgHandDiagR = true;
                                    }

                                } else {
                                    // Fallback: base-frame composition (head-turn stable, no view pose).
                                    float hq[4] = { hmdRel[0], hmdRel[1], hmdRel[2], hmdRel[3] };
                                    VRIK_QuatNorm(hq);
                                    float cbx[3]; VRIK_QuatRotateVec(hq, vrPos, cbx);
                                    cbx[0] += hmdPosBase[0]; cbx[1] += hmdPosBase[1]; cbx[2] += hmdPosBase[2];
                                    float mp[3] = { cbx[0]*g_VRScaleR, -cbx[2]*g_VRScaleR, cbx[1]*g_VRScaleR };
                                    float rp[3]; VRIK_QuatRotateVec(camModelHandRot, mp, rp);
                                    target[0] = handAnchor[0] + rp[0] + offR[0];
                                    target[1] = handAnchor[1] + rp[1] + offR[1];
                                    target[2] = handAnchor[2] + rp[2] + offR[2];
                                    float cq[4]; VRIK_QuatMul(hq, vrQuat, cq); VRIK_QuatNorm(cq);
                                    float lq[4] = { cq[0], -cq[2], cq[1], cq[3] };
                                    float hm[4]; VRIK_QuatMul(camModelHandRot, lq, hm);
                                    VRIK_QuatMul(hm, wristR, handRot); VRIK_QuatNorm(handRot);
                                }
                            }
                            // Capture the SOLVED wrist position (= controller in model space)
                            // for the holster-distance block below.
                            // HAND DIAGNOSTIC. A second number lived here -- the solved wrist's world
                            // position against the controller's -- and it was REMOVED as a lying
                            // instrument: both sides were built from the same vpView and vq, so it
                            // cancelled algebraically and read ~0.0 mm no matter what was wrong. It
                            // was quoted as proof that the hands were on the controllers while they
                            // were visibly behind the headset. A measurement whose two sides share a
                            // source measures nothing.
                            //
                            //   stop      = how far the hand-collision stop zone moved the target.
                            //               With the body turned under stationary hands, the target
                            //               travels through the torso in MODEL space, which is exactly
                            //               where that zone lives.
                            if (dbgHandDiagR) {
                                const float sx = target[0] - dbgTargetPreStopR[0];
                                const float sy = target[1] - dbgTargetPreStopR[1];
                                const float sz = target[2] - dbgTargetPreStopR[2];
                                CyberpunkVR_DebugHandMissMm =
                                    std::sqrt(sx*sx + sy*sy + sz*sz) * 1000.0f;
                            }
                            rhWristModel[0] = target[0];
                            rhWristModel[1] = target[1];
                            rhWristModel[2] = target[2];

                            // SHAKE, MEASURED AT THREE LEVELS. Every rate and coherence check has
                            // come back clean, so the next question is not where the data comes
                            // from but where the wobble is ADDED. Smooth motion has a small second
                            // difference; a shake has a large one. Same metric on the raw
                            // controller offset, on the anchor, and on the final target tells
                            // which stage introduces it -- runtime noise, the anchor, or the
                            // composition in between. Runs once per fresh solve, so consecutive
                            // samples are consecutive frames.
                            {
                                static float hp[5][3][2] = {};   // [level][axis][age]
                                static int   hn = 0;
                                float av[3] = {0.0f, 0.0f, 0.0f};
                                VRIK_ResolveViewPos(av);   // pure; vpView is scoped inside the branch
                                // Two more levels, BELOW the target: the wrist can sit perfectly
                                // still while the arm still flickers, because the arm also hangs
                                // off the shoulder and the shoulder off the body camera. If those
                                // move, the IK bends the limb differently every frame -- visible
                                // as shaking arms with a motionless hand.
                                const float lv[5][3] = {
                                    { vrPos[0], vrPos[1], vrPos[2] },
                                    { av[0], av[1], av[2] },
                                    { target[0], target[1], target[2] },
                                    { shoulderModelR[0], shoulderModelR[1], shoulderModelR[2] },
                                    { camModelPos ? camModelPos[0] : 0.0f,
                                      camModelPos ? camModelPos[1] : 0.0f,
                                      camModelPos ? camModelPos[2] : 0.0f } };
                                if (hn >= 2) {
                                    for (int L = 0; L < 5; ++L) {
                                        float a2 = 0.0f;
                                        for (int k = 0; k < 3; ++k) {
                                            const float d = lv[L][k] - 2.0f * hp[L][k][0] + hp[L][k][1];
                                            a2 += d * d;
                                        }
                                        const float mm = std::sqrt(a2) * 1000.0f;
                                        const int slot = (L < 3) ? (224 + L) : (231 + (L - 3));
                                        if (mm < 500.0f && mm > g_pSharedHands[slot])
                                            g_pSharedHands[slot] = mm;
                                    }
                                }
                                for (int L = 0; L < 5; ++L)
                                    for (int k = 0; k < 3; ++k) {
                                        hp[L][k][1] = hp[L][k][0];
                                        hp[L][k][0] = lv[L][k];
                                    }
                                if (hn < 2) ++hn;
                            }
                            rhWristValid = true;
                            // LAST THING BEFORE THE SOLVE, on purpose. An earlier call site right after
                            // VRIK_BuildHandTarget was measured to do nothing at all: the gizmo path
                            // recomputes target[0..2] from the camera afterwards, so the clamp was overwritten
                            // before the solver ever saw it. A forced 10 cm hold moved the hand not at all.
                            if (CyberpunkVR_XrDeepDiag) VRIK_NoteShake(1, 2, target);   // stage 2: what the IK solves for
                            VRIK_ApplyHandStop(1, target, handRot);
                            // WHEEL GRAB. Store the controller target FIRST: it is what the NEXT
                            // solve measures against the animated hand, and it has to keep being
                            // recorded while the arm is handed over, or letting go of the wheel could
                            // never re-arm. Then blend unconditionally (a no-op at 0, exact at 1) so
                            // everything downstream that treats (target, handRot) as the hand's model
                            // transform -- the palm the basketball reads, the cigarette anchor --
                            // follows the hand that is actually rendered, not the controller it left.
                            cvr::anim::WheelStoreTarget(0, target);
                            cvr::anim::WheelBlendTarget(0, target, handRot);
                            if (!wheelOffR) {
                                VRIK_SolveArm(boneBuf, g_VRRightUpperArmIdx, g_VRRightForeArmIdx,
                                              g_VRRightBoneIdx, target, handRot,
                                              bodyRight, bodyUp, bodyFwd,
                                              g_VRElbowPoleR * 0.01745329252f, g_VRElbowSwingR,
                                              /*isLeft*/false, /*storeDbg*/true);
                            }
                            // Solved RIGHT wrist for the VR basketball (see the left-hand twin).
                            g_VRPalmModelR[0] = target[0];
                            g_VRPalmModelR[1] = target[1];
                            g_VRPalmModelR[2] = target[2];
                            g_VRPalmModelRotR[0] = handRot[0];
                            g_VRPalmModelRotR[1] = handRot[1];
                            g_VRPalmModelRotR[2] = handRot[2];
                            g_VRPalmModelRotR[3] = handRot[3];
                            // SMOKING: head<->cig-hand distance. vrPos is the right controller expressed
                            // RELATIVE TO THE HMD (HMD-local, metres) -- the same input the arm solve
                            // maps into model space. Its magnitude is literally how far the cig hand
                            // (the WeaponRight slot rides it) is from the head: small when the cig is at
                            // the mouth ("weapon slot just below the head"), ~0.6 with the arm down.
                            // No model-space anchor to get wrong (that was the chest-vs-eye bug).
                            {
                                const float dx = vrPos[0], dy = vrPos[1], dz = vrPos[2];
                                g_VRSmokeMouthDist = std::sqrt(dx*dx + dy*dy + dz*dz);
                            }

                            // MOUTH ANCHOR: pin the cig to a head-anchored point so it stays at the
                            // lips when the hand drops. The cig (g_VRSmokeCigIdx / WeaponRight) is a
                            // descendant of the SOLVED hand -- (target, handRot) is the hand model
                            // transform. Walk from the cig slot up to the hand to get the slot's PARENT
                            // model transform (robust whether it is a direct child or deeper), then set
                            // the slot LOCAL = parentModel^-1 . mouthModel so its model pose == the
                            // head-anchored mouth pose. Overrides the grip-pose write from earlier this
                            // pass; runs every (replayed) solve so it never flickers.
                            if (g_VRSmokeMouthAnchor && g_VRSmokeMouthBoneIdx >= 0 && g_VRSmokeMouthBoneIdx < VRIK_MAX_BONES
                                && vrViewFrameOk) {
                                int chain[16]; int cn = 0; bool reachedHand = false;
                                int a = g_VRBoneParent[g_VRSmokeMouthBoneIdx];
                                while (a >= 0 && a < VRIK_MAX_BONES && cn < 16) {
                                    if (a == g_VRRightBoneIdx) { reachedHand = true; break; }
                                    chain[cn++] = a; a = g_VRBoneParent[a];
                                }
                                if (reachedHand) {
                                    // parentModel: start at the hand, compose intermediate locals down to the slot's parent.
                                    float pmPos[3] = { target[0], target[1], target[2] };
                                    float pmRot[4] = { handRot[0], handRot[1], handRot[2], handRot[3] };
                                    for (int k = cn - 1; k >= 0; --k) {
                                        const float* lt = reinterpret_cast<const float*>(boneBuf + chain[k]*48 + VRIK_TRANS_OFF);
                                        const float* lr = reinterpret_cast<const float*>(boneBuf + chain[k]*48 + VRIK_ROT_OFF);
                                        float rp[3]; VRIK_QuatRotateVec(pmRot, lt, rp);
                                        pmPos[0]+=rp[0]; pmPos[1]+=rp[1]; pmPos[2]+=rp[2];
                                        float nr[4]; VRIK_QuatMul(pmRot, lr, nr); VRIK_QuatNorm(nr);
                                        pmRot[0]=nr[0]; pmRot[1]=nr[1]; pmRot[2]=nr[2]; pmRot[3]=nr[3];
                                    }
                                    // Desired cig MODEL pose = VIEW (HMD) transform . (offset, rot). vpM/rvM
                                    // track real head yaw+pitch+position (the avatar head BONE is idle in FPP,
                                    // which is why anchoring to it left the cig hanging in space). Offset is
                                    // HMD-local, same frame as the hand target: x=right, y=forward, z=up (m).
                                    const float mo[3]  = { g_VRSmokeMouthPos[0], g_VRSmokeMouthPos[1], g_VRSmokeMouthPos[2] };
                                    const float mrl[4] = { g_VRSmokeMouthRot[0], g_VRSmokeMouthRot[1], g_VRSmokeMouthRot[2], g_VRSmokeMouthRot[3] };
                                    float moW[3]; VRIK_QuatRotateVec(vrViewRotM, mo, moW);
                                    const float mouthPos[3] = { vrViewPosM[0]+moW[0], vrViewPosM[1]+moW[1], vrViewPosM[2]+moW[2] };
                                    float mouthRot[4]; VRIK_QuatMul(vrViewRotM, mrl, mouthRot); VRIK_QuatNorm(mouthRot);
                                    float pmConj[4]; VRIK_QuatConj(pmRot, pmConj);
                                    float rel[3] = { mouthPos[0]-pmPos[0], mouthPos[1]-pmPos[1], mouthPos[2]-pmPos[2] };
                                    float localPos[3]; VRIK_QuatRotateVec(pmConj, rel, localPos);
                                    float localRot[4]; VRIK_QuatMul(pmConj, mouthRot, localRot); VRIK_QuatNorm(localRot);
                                    // Store for the every-pass grip-apply block to replay (and write now for this pass).
                                    g_VRSmokeAnchorLocalPos[0]=localPos[0]; g_VRSmokeAnchorLocalPos[1]=localPos[1]; g_VRSmokeAnchorLocalPos[2]=localPos[2];
                                    g_VRSmokeAnchorLocalRot[0]=localRot[0]; g_VRSmokeAnchorLocalRot[1]=localRot[1]; g_VRSmokeAnchorLocalRot[2]=localRot[2]; g_VRSmokeAnchorLocalRot[3]=localRot[3];
                                    g_VRSmokeAnchorValid = 1;
                                    float* t = reinterpret_cast<float*>(boneBuf + g_VRSmokeMouthBoneIdx*48 + VRIK_TRANS_OFF);
                                    float* q = reinterpret_cast<float*>(boneBuf + g_VRSmokeMouthBoneIdx*48 + VRIK_ROT_OFF);
                                    t[0]=localPos[0]; t[1]=localPos[1]; t[2]=localPos[2];
                                    q[0]=localRot[0]; q[1]=localRot[1]; q[2]=localRot[2]; q[3]=localRot[3];
                                }
                            }
                            // GENERAL mouth-pin: same lips pose but for an arbitrary NON-hand bone
                            // (Neck1/Head/Neck), so a prop on a non-weapon slot rides the mouth with
                            // BOTH hands + weapon slots free. Local = parentModel^-1 * mouthModel,
                            // using the parent's model FK (g_fkPos/g_fkRot). Stored; applied every pass.
                            {
                                int abSel = g_VRSmokeAnchorBoneSel;
                                int ab = (abSel==1) ? g_VRNeck1Idx : (abSel==2) ? g_VRHeadBoneIdx : (abSel==3) ? g_VRNeckIdx : -1;
                                g_VRSmokeAnchorBoneIdx = ab;
                                if (g_VRSmokeMouthAnchor && vrViewFrameOk && ab >= 0 && ab < VRIK_MAX_BONES) {
                                    int pa = g_VRBoneParent[ab];
                                    if (pa >= 0 && pa < VRIK_MAX_BONES) {
                                        const float amo[3]  = { g_VRSmokeMouthPos[0], g_VRSmokeMouthPos[1], g_VRSmokeMouthPos[2] };
                                        const float amrl[4] = { g_VRSmokeMouthRot[0], g_VRSmokeMouthRot[1], g_VRSmokeMouthRot[2], g_VRSmokeMouthRot[3] };
                                        float amoW[3]; VRIK_QuatRotateVec(vrViewRotM, amo, amoW);
                                        const float amouthPos[3] = { vrViewPosM[0]+amoW[0], vrViewPosM[1]+amoW[1], vrViewPosM[2]+amoW[2] };
                                        float amouthRot[4]; VRIK_QuatMul(vrViewRotM, amrl, amouthRot); VRIK_QuatNorm(amouthRot);
                                        const float* ppos = g_fkPos[pa]; const float* prot = g_fkRot[pa];
                                        float pInv[4]; VRIK_QuatConj(prot, pInv);
                                        float alrot[4]; VRIK_QuatMul(pInv, amouthRot, alrot); VRIK_QuatNorm(alrot);
                                        float ad[3] = { amouthPos[0]-ppos[0], amouthPos[1]-ppos[1], amouthPos[2]-ppos[2] };
                                        float alpos[3]; VRIK_QuatRotateVec(pInv, ad, alpos);
                                        g_VRSmokeAltAnchorLocalPos[0]=alpos[0]; g_VRSmokeAltAnchorLocalPos[1]=alpos[1]; g_VRSmokeAltAnchorLocalPos[2]=alpos[2];
                                        g_VRSmokeAltAnchorLocalRot[0]=alrot[0]; g_VRSmokeAltAnchorLocalRot[1]=alrot[1]; g_VRSmokeAltAnchorLocalRot[2]=alrot[2]; g_VRSmokeAltAnchorLocalRot[3]=alrot[3];
                                        g_VRSmokeAltAnchorValid = 1;
                                        // APPLY here (post body/hip solve) so pinning a structural bone
                                        // doesn't fling the VRIK body anchor. Write the bone's local now.
                                        float* abt = reinterpret_cast<float*>(boneBuf + ab*48 + VRIK_TRANS_OFF);
                                        float* abq = reinterpret_cast<float*>(boneBuf + ab*48 + VRIK_ROT_OFF);
                                        abt[0]=alpos[0]; abt[1]=alpos[1]; abt[2]=alpos[2];
                                        abq[0]=alrot[0]; abq[1]=alrot[1]; abq[2]=alrot[2]; abq[3]=alrot[3];
                                    }
                                } else if (abSel == 0) {
                                    g_VRSmokeAltAnchorValid = 0;
                                }
                            }
                        }
                        // Left arm.
                        if (SharedPose(0) > 0.0f && g_VRLeftUpperArmIdx >= 0) {
                            const float vrPos[3]  = { SharedPose(1), SharedPose(2), SharedPose(3) };
                            if (CyberpunkVR_XrDeepDiag) VRIK_NoteShake(0, 0, vrPos);   // stage 0: tracking, as read
                            const float vrQuat[4] = { SharedPose(4), SharedPose(5), SharedPose(6), SharedPose(7) };
                            // SMOKING: LEFT hand <-> mouth distance (mirror of g_VRSmokeMouthDist for the
                            // left controller). |vrPos| = how far the left hand is from the head (HMD-local
                            // metres) -- small when the left hand is at the lips. Lets the LEFT grip toggle
                            // the cig when the LEFT hand (not the right) is the one raised to the mouth.
                            { const float dxl=vrPos[0], dyl=vrPos[1], dzl=vrPos[2];
                              g_VRSmokeMouthDistL = std::sqrt(dxl*dxl + dyl*dyl + dzl*dzl); }
                            float target[3], handRot[4];
                            const float wristL[4]       = { g_VRWristL_I, g_VRWristL_J, g_VRWristL_K, g_VRWristL_R };
                            const float offL[3]         = { g_VROffLX, g_VROffLY, g_VROffLZ };
                            const float shoulderBodyL[3]= { g_VRShoulderLX, g_VRShoulderLY, g_VRShoulderLZ };
                            float armLenL = restArmLen(g_VRLeftUpperArmIdx, g_VRLeftForeArmIdx, g_VRLeftBoneIdx);
                            float shoulderModelL[3];
                            if (wheelOffL) {
                                // WHEEL GRAB -- animated girdle, no clavicle write (see right arm).
                                const int ui = g_VRLeftUpperArmIdx;
                                shoulderModelL[0] = g_fkPos[ui][0];
                                shoulderModelL[1] = g_fkPos[ui][1];
                                shoulderModelL[2] = g_fkPos[ui][2];
                                VRIK_BuildHandTarget(shoulderModelL, shoulderBodyL, hmdRel, vrPos, vrQuat,
                                                     wristL, g_VRScaleL, offL, target, handRot);
                            } else if (camModelValid && headModelPos) {
                                anchorStableShoulder(g_VRLeftUpperArmIdx, stableAnchor, /*isLeft*/true, shoulderModelL);
                                VRIK_BuildHandTarget(shoulderModelL, shoulderBodyL, hmdRel, vrPos, vrQuat,
                                                     wristL, g_VRScaleL, offL, target, handRot);
                            } else {
                                calibratedShoulderModel(shoulderBodyL, g_VRLeftUpperArmIdx, shoulderModelL);
                                applyShoulderAnchor(g_VRLeftUpperArmIdx, shoulderModelL);
                                VRIK_BuildHandTarget(shoulderModelL, shoulderBodyL, hmdRel, vrPos, vrQuat, wristL, g_VRScaleL, offL, target, handRot);
                            }

                            // HAND == GIZMO, anchored at the BODY camera (see right arm).
                            if (camModelValid) {
                                // RENDER-VIEW ORIGIN / fallback (see right arm).
                                float vpView[3];
                                if (VRIK_ResolveViewPos(vpView)) {
                                    // vq via the hands snapshot -- MADE IDENTICAL TO THE RIGHT
                                    // ARM by user order ("сделай левую руку = правой, когда не
                                    // было трейла"). The packet source lagged the rendered view
                                    // by one sample -> the left-only trail; the snapshot path
                                    // was trail-free on the right in user testing.
                                    // ONE LATCH FOR THE WHOLE FRAME OF REFERENCE. vq used to be
                                    // read from the hands snapshot while the heading and the head
                                    // orientation beside it came from the view packet -- two
                                    // seqlocks, two instants. The comment below still claims they
                                    // are the same packet; they were not. With the camera doing
                                    // its idle sway the two drift apart, and the difference lands
                                    // straight on the hand's ROTATION: measured 0.44 deg per frame
                                    // with the controllers lying on the table.
                                    //
                                    // Taking vq from the packet was tried before and left a trail,
                                    // because the packet lags the rendered view by a sample. That
                                    // objection is gone: the alignment below replaces the packet's
                                    // head part with hmdRel from the hand sample, so what is used
                                    // is the packet's heading with the hands' own head rotation.
                                    // REVERTED 2026-07-30, by measurement: sourcing vq from the
                                    // packet instead of the snapshot took VIEWREL rot from 0.44 to
                                    // 0.95 deg per frame with the controllers lying still. The
                                    // packet updates on the locate cadence, the snapshot on the
                                    // publish cadence, and the snapshot is the closer of the two
                                    // to the frame being solved. The cross-latch mixing with the
                                    // heading is real, but this was the wrong half to move.
                                    // ONE LATCH FOR BOTH HALVES OF THE RE-BASING BELOW.
                                    //
                                    // The time-align a few lines down computes
                                    //     vqUse = vq * mapQ(conj(headOri_view) * hmdRel)
                                    // which only cancels the head rotation if vq was BUILT from the
                                    // very sample headOri_view names. It was not. vq came out of the
                                    // hands snapshot (RefreshHandsSnapshot, seqlock [127]) while
                                    // headOri_view comes out of the view packet (VRIK_LatchViewPacket,
                                    // seqlock [143]) -- two captures of the same publisher at two
                                    // moments. Worse, the snapshot has a fast path that returns unless
                                    // [127] moved, so its copy of [104..107] refreshes at the HANDS'
                                    // cadence, not the view's. What was left in the hand frame was
                                    // V_snapshot * conj(V_packet): the head rotation between two view
                                    // publications. Zero standing still, degrees during a head turn,
                                    // centimetres at the end of a 0.6 m arm -- head-turn only, by
                                    // construction, which is exactly how it was reported.
                                    //
                                    // [104..107] also sits OUTSIDE the [127] bracket by that latch's
                                    // own comment, so the snapshot could tear it as well.
                                    //
                                    // Taking vq from the packet puts numerator and denominator in one
                                    // publication. Nothing is lost: the snap-event block rotates
                                    // g_viewPkt[0..3] and mirrors the same values into
                                    // g_handsStable[104..107], so both carried the snap rotation.
                                    float vq[4];
                                    if (CyberpunkVR_VrikHandFrameOneLatch && g_viewPktValid) {
                                        vq[0] = g_viewPkt[0]; vq[1] = g_viewPkt[1];
                                        vq[2] = g_viewPkt[2]; vq[3] = g_viewPkt[3];
                                    } else {
                                        vq[0] = SharedPose(104); vq[1] = SharedPose(105);
                                        vq[2] = SharedPose(106); vq[3] = SharedPose(107);
                                    }
                                    VRIK_QuatNorm(vq);
                                    // WORLD->MODEL yaw = game heading from the SAME view
                                    // packet as vq (one seqlocked frame: no snap-turn
                                    // old-vq/new-yaw mix -> no arm double). No HMD yaw
                                    // in any mode: head turns do not rotate the hand
                                    // frame. Fallback (older dxgi): yaw extracted from vq.
                                    float vyaw;
                                      if (g_viewPktValid) {
                                        vyaw = g_viewPkt[8];
                                    } else {
                                        const float fX = 2.0f*(vq[0]*vq[1] - vq[3]*vq[2]);
                                        const float fY = 1.0f - 2.0f*(vq[0]*vq[0] + vq[2]*vq[2]);
                                        vyaw = std::atan2(-fX, fY);
                                    }
                                    // MODEL SPACE IS THE ENTITY FRAME, so the converter has to be the
                                    // body's TRUE yaw. With physical body rotation on those two part
                                    // company by design: the body carries the engine's heading while
                                    // the view deliberately holds back the realign we injected into it
                                    // (src/Hooks/BodyYawFollow.cpp). Using the view's heading here
                                    // would leave every hand target rotated about the body by exactly
                                    // that angle -- the hands riding with the body. The census value is
                                    // also self-correcting: it is what the engine ACTUALLY ended up at,
                                    // so a heading it clamps or eases still leaves the hands put.
                                    if (CyberpunkVR_BodyYawFollow && CyberpunkVR_BodyYawFinalValid) {
                                        vyaw = CyberpunkVR_BodyYawFinalRad;
                                    }
                                    const float hs = std::sin(vyaw * 0.5f);
                                    const float hc = std::cos(vyaw * 0.5f);
                                    float ec[4] = { 0.0f, 0.0f, -hs, hc };
                                    // TIME-ALIGN THE HAND FRAME. vq is the view orientation
                                    // from [104..107], written at LocateCamera; the offset it is
                                    // about to rotate came from the XR sample the hands were
                                    // published with. Measured: 33.6 ms against 23.5 ms, and at
                                    // 886 deg/s that 10 ms gap is ~9 deg -- on a half-metre arm,
                                    // 8 cm of hand displacement, changing every frame. The whole
                                    // reason the offset is stored head-locally is so head
                                    // rotation cancels; it only cancels if the two halves are
                                    // from one instant.
                                    //
                                    // vq = heading * mapQ(headOri_view), so replacing its head
                                    // part with the sample's is a right-multiply by
                                    // mapQ(conj(headOri_view) * hmdRel). Nothing is assumed about
                                    // how the heading is built -- it divides out.
                                    float vqUse[4] = { vq[0], vq[1], vq[2], vq[3] };
                                    if (g_viewPktValid && CyberpunkVR_VrikHandFrameAlign) {
                                          const float* vo = &g_viewPkt[13];
                                        if (vo[0] != 0.0f || vo[1] != 0.0f || vo[2] != 0.0f ||
                                            vo[3] != 1.0f) {
                                            const float voc[4] = { -vo[0], -vo[1], -vo[2], vo[3] };
                                            float dxr[4]; VRIK_QuatMul(voc, hmdRel, dxr);
                                            const float dg[4] = { dxr[0], -dxr[2], dxr[1], dxr[3] };
                                            float t2[4]; VRIK_QuatMul(vq, dg, t2);
                                            VRIK_QuatNorm(t2);
                                            vqUse[0]=t2[0]; vqUse[1]=t2[1]; vqUse[2]=t2[2]; vqUse[3]=t2[3];
                                        }
                                    }
                                    float rvM[4]; VRIK_QuatMul(ec, vqUse, rvM); VRIK_QuatNorm(rvM);
                                    // THE VIEW-FRAME BRANCH TAKES THE VIEW ORIGIN, NOT THE ANCHOR.
                                    // 64e706fc replaced this with `handAnchor` to get the Lua globals
                                    // out of the IK target, and that part of its reasoning stands. But
                                    // handAnchor is camModelPos PLUS the view-only offsets
                                    // ([120..122] - [91..93]), so assigning it here collapsed this
                                    // branch onto the fallback below and added those offsets to the
                                    // target. Bisected in the headset: bd0f5aca alone is clean,
                                    // bd0f5aca+64e706fc puts both hands visibly ABOVE the gizmo.
                                    // The view origin is what the gizmo is drawn from, so it is what
                                    // the hand must be built on.
                                    float vpW[3] = { vpView[0] - g_VREntityPosX,
                                                     vpView[1] - g_VREntityPosY,
                                                     vpView[2] - g_VREntityPosZ };
                                    float vpM[3]; VRIK_QuatRotateVec(ec, vpW, vpM);
                                    float mp[3] = { vrPos[0]*g_VRScaleL, -vrPos[2]*g_VRScaleL, vrPos[1]*g_VRScaleL };
                                    float rp[3]; VRIK_QuatRotateVec(rvM, mp, rp);
                                    target[0] = vpM[0] + rp[0] + offL[0];
                                    target[1] = vpM[1] + rp[1] + offL[1];
                                    target[2] = vpM[2] + rp[2] + offL[2];
                                    float lq[4] = { vrQuat[0], -vrQuat[2], vrQuat[1], vrQuat[3] };
                                    float hm[4]; VRIK_QuatMul(rvM, lq, hm);
                                    VRIK_QuatMul(hm, wristL, handRot); VRIK_QuatNorm(handRot);
                                    // ...and if the grip is HELD, this wrist belongs to the weapon instead:
                                    // the support point the right hand computed a moment ago, in the same
                                    // frame. Only a held grip moves a wrist -- a preview never does.
                                    cvr::anim::TwoHandLeft(target, handRot);

                                    // RECOIL, ADDED TO THE TARGET -- the only place it can survive.
                                    //
                                    // Everything below this writes the wrist from the controller, so a kick
                                    // applied to the bones would be overwritten in the same solve; applied to
                                    // the target, the arm IK carries it up the chain and the weapon, being
                                    // parented to the hand, goes with it.
                                    //
                                    // `hm` is the CONTROLLER's orientation in model space, before the wrist
                                    // correction -- so its +Y is where the weapon points (the same axis map the
                                    // camera uses: XR -Z forward -> game +Y) and its +X is the hand's right.
                                    // Taking the axes from here rather than from handRot keeps the kick along
                                    // the barrel whatever the per-weapon wrist correction does.
                                    {
                                        float backM = 0.0f, riseRad = 0.0f;
                                        // NOT WHILE IT IS WELDED TO THE WEAPON. A hand on the gun already
                                        // rides the gun's kick -- the support point is computed from the
                                        // kicked hand -- so sampling the spring again here would kick it
                                        // twice, and about its own axis rather than the weapon's, which
                                        // tears the hand off the grip for the length of the recoil.
                                        if (!CyberpunkVR_TwoHandActive) RecoilSample(0, 0, &backM, &riseRad);
                                        if (backM != 0.0f || riseRad != 0.0f) {
                                            const float fwdL[3] = { 0.0f, 1.0f, 0.0f };
                                            float fwd[3]; VRIK_QuatRotateVec(hm, fwdL, fwd);
                                            target[0] -= fwd[0] * backM;
                                            target[1] -= fwd[1] * backM;
                                            target[2] -= fwd[2] * backM;
                                            const float rightL[3] = { 1.0f, 0.0f, 0.0f };
                                            float ax[3]; VRIK_QuatRotateVec(hm, rightL, ax);
                                            const float h2 = riseRad * 0.5f;
                                            const float s2 = std::sin(h2);
                                            const float kq[4] = { ax[0]*s2, ax[1]*s2, ax[2]*s2, std::cos(h2) };
                                            float kr[4]; VRIK_QuatMul(kq, handRot, kr); VRIK_QuatNorm(kr);
                                            handRot[0]=kr[0]; handRot[1]=kr[1]; handRot[2]=kr[2]; handRot[3]=kr[3];
                                        }
                                    }

                                } else {
                                    float hq[4] = { hmdRel[0], hmdRel[1], hmdRel[2], hmdRel[3] };
                                    VRIK_QuatNorm(hq);
                                    float cbx[3]; VRIK_QuatRotateVec(hq, vrPos, cbx);
                                    cbx[0] += hmdPosBase[0]; cbx[1] += hmdPosBase[1]; cbx[2] += hmdPosBase[2];
                                    float mp[3] = { cbx[0]*g_VRScaleL, -cbx[2]*g_VRScaleL, cbx[1]*g_VRScaleL };
                                    float rp[3]; VRIK_QuatRotateVec(camModelHandRot, mp, rp);
                                    target[0] = handAnchor[0] + rp[0] + offL[0];
                                    target[1] = handAnchor[1] + rp[1] + offL[1];
                                    target[2] = handAnchor[2] + rp[2] + offL[2];
                                    float cq[4]; VRIK_QuatMul(hq, vrQuat, cq); VRIK_QuatNorm(cq);
                                    float lq[4] = { cq[0], -cq[2], cq[1], cq[3] };
                                    float hm[4]; VRIK_QuatMul(camModelHandRot, lq, hm);
                                    VRIK_QuatMul(hm, wristL, handRot); VRIK_QuatNorm(handRot);
                                }
                            }
                            if (CyberpunkVR_XrDeepDiag) VRIK_NoteShake(0, 2, target);   // stage 2: what the IK solves for
                            VRIK_ApplyHandStop(0, target, handRot);
                            cvr::anim::WheelStoreTarget(1, target);
                            cvr::anim::WheelBlendTarget(1, target, handRot);
                            if (!wheelOffL) {
                                VRIK_SolveArm(boneBuf, g_VRLeftUpperArmIdx, g_VRLeftForeArmIdx,
                                              g_VRLeftBoneIdx, target, handRot,
                                              bodyRight, bodyUp, bodyFwd,
                                              g_VRElbowPoleL * 0.01745329252f, g_VRElbowSwingL,
                                              /*isLeft*/true, /*storeDbg*/true);
                            }
                            // STEERING. Here and not inside WheelUpdate: this is the first point
                            // where BOTH controller targets are this solve's, and where the body
                            // right/up axes that define the wheel's plane are in scope.
                            cvr::anim::WheelSteerUpdate(bodyRight, bodyUp);
                            // Solved LEFT wrist, mirroring rhWristModel above. The VR basketball
                            // needs both hands, and the pre-solve g_fkPos is the animated pose
                            // (hands at the thighs) -- exactly the trap the holster code documents.
                            g_VRPalmModelL[0] = target[0];
                            g_VRPalmModelL[1] = target[1];
                            g_VRPalmModelL[2] = target[2];
                            g_VRPalmModelRotL[0] = handRot[0];
                            g_VRPalmModelRotL[1] = handRot[1];
                            g_VRPalmModelRotL[2] = handRot[2];
                            g_VRPalmModelRotL[3] = handRot[3];
                            g_VRCamModelPos[0] = camModelPos[0];
                            g_VRCamModelPos[1] = camModelPos[1];
                            g_VRCamModelPos[2] = camModelPos[2];
                            // Consumers combine this with the published view/controller frame for
                            // model<->world conversion, so it follows the hand rotation, not the
                            // fresh head-only rotation.
                            g_VRCamModelRot[0] = camModelHandRot[0];
                            g_VRCamModelRot[1] = camModelHandRot[1];
                            g_VRCamModelRot[2] = camModelHandRot[2];
                            g_VRCamModelRot[3] = camModelHandRot[3];
                            g_VRPalmModelValid = 1;
                        }
                        // HAND-TO-HOLSTER distances [20..22] -- computed AFTER the arm solve,
                        // from the SOLVED right wrist (rhWristModel = the arm-IK target = the
                        // player's controller in model space). The old pre-solve version read
                        // g_fkPos[wrist] = the ENGINE'S ANIMATED wrist (idle anim: hands at
                        // thighs -> permanent zone R; weapon-ready anim: wrist at chest ->
                        // permanent zone B), so grip presses fired holster zones while the
                        // REAL hand was nowhere near them (log-proven slot 1<->2 swap).
                        // Anchors (hips/shoulders) read g_fkPos, which at this point holds
                        // the post-body-place, post-clavicle-anchoring FK.
                        if (rhWristValid) {
                            const float* rh = rhWristModel;

                            // Body axes: bodyRight from shoulders, bodyUp from root->head, bodyFwd from cross.
                            float br[3] = { 1.0f, 0.0f, 0.0f };
                            float bu[3] = { 0.0f, 0.0f, 1.0f };
                            bool haveShoulders = (g_VRRightUpperArmIdx >= 0 && g_VRLeftUpperArmIdx >= 0
                                && g_VRRightUpperArmIdx < VRIK_MAX_BONES && g_VRLeftUpperArmIdx < VRIK_MAX_BONES);
                            if (haveShoulders) {
                                const float* rs = g_fkPos[g_VRRightUpperArmIdx];
                                const float* ls = g_fkPos[g_VRLeftUpperArmIdx];
                                br[0] = rs[0]-ls[0]; br[1] = rs[1]-ls[1]; br[2] = rs[2]-ls[2];
                                float n = sqrtf(br[0]*br[0]+br[1]*br[1]+br[2]*br[2]);
                                if (n > 1e-4f) { br[0]/=n; br[1]/=n; br[2]/=n; }
                            }
                            if (g_VRHeadBoneIdx >= 0 && g_VRHeadBoneIdx < VRIK_MAX_BONES) {
                                const float* root = g_fkPos[0];
                                const float* head = g_fkPos[g_VRHeadBoneIdx];
                                bu[0] = head[0]-root[0]; bu[1] = head[1]-root[1]; bu[2] = head[2]-root[2];
                                float n = sqrtf(bu[0]*bu[0]+bu[1]*bu[1]+bu[2]*bu[2]);
                                if (n > 1e-4f) { bu[0]/=n; bu[1]/=n; bu[2]/=n; }
                            }
                            // bodyFwd = cross(bodyUp, bodyRight). Sign convention may differ per
                            // rig; the over-shoulder anchor below publishes min of BOTH sign
                            // candidates, so the sign never matters.
                            float bf[3];
                            bf[0] = bu[1]*br[2] - bu[2]*br[1];
                            bf[1] = bu[2]*br[0] - bu[0]*br[2];
                            bf[2] = bu[0]*br[1] - bu[1]*br[0];
                            { float n = sqrtf(bf[0]*bf[0]+bf[1]*bf[1]+bf[2]*bf[2]);
                              if (n > 1e-4f) { bf[0]/=n; bf[1]/=n; bf[2]/=n; } }

                            const float kRightOff = 0.18f; // pistol pulls outward — handle sits right of hip
                            const float kLeftOff  = 0.05f; // katana handle stays close to body center
                            auto d3 = [](float ax, float ay, float az, float bx, float by, float bz) -> float {
                                float dx = ax-bx, dy = ay-by, dz = az-bz;
                                return sqrtf(dx*dx + dy*dy + dz*dz);
                            };

                            // Hip prop distances.
                            if (g_VRRightUpLegIdx >= 0 && g_VRRightUpLegIdx < VRIK_MAX_BONES) {
                                const float* p = g_fkPos[g_VRRightUpLegIdx];
                                float px = p[0] + br[0]*kRightOff, py = p[1] + br[1]*kRightOff, pz = p[2] + br[2]*kRightOff;
                                g_pSharedHands[20] = d3(rh[0],rh[1],rh[2], px,py,pz);
                            } else g_pSharedHands[20] = -1.0f;
                            if (g_VRLeftUpLegIdx >= 0 && g_VRLeftUpLegIdx < VRIK_MAX_BONES) {
                                const float* p = g_fkPos[g_VRLeftUpLegIdx];
                                float px = p[0] - br[0]*kLeftOff, py = p[1] - br[1]*kLeftOff, pz = p[2] - br[2]*kLeftOff;
                                g_pSharedHands[21] = d3(rh[0],rh[1],rh[2], px,py,pz);
                            } else g_pSharedHands[21] = -1.0f;

                            // "OVER-RIGHT-SHOULDER" reach for a back-slung ranged weapon.
                            // Two sign candidates published as min (bodyFwd sign per rig unknown;
                            // the wrong candidate lands in front of the body = always far).
                            if (haveShoulders) {
                                const float* rs = g_fkPos[g_VRRightUpperArmIdx];
                                const float kUp = 0.05f;   // above shoulder
                                const float kBack = 0.10f; // back from shoulder
                                float ax = rs[0] + bu[0]*kUp - bf[0]*kBack;
                                float ay = rs[1] + bu[1]*kUp - bf[1]*kBack;
                                float az = rs[2] + bu[2]*kUp - bf[2]*kBack;
                                float dA = d3(rh[0],rh[1],rh[2], ax,ay,az);
                                float bx = rs[0] + bu[0]*kUp + bf[0]*kBack;
                                float by = rs[1] + bu[1]*kUp + bf[1]*kBack;
                                float bz = rs[2] + bu[2]*kUp + bf[2]*kBack;
                                float dB = d3(rh[0],rh[1],rh[2], bx,by,bz);
                                g_pSharedHands[22] = (dA < dB) ? dA : dB;
                            } else g_pSharedHands[22] = -1.0f;
                        } else {
                            // No right-hand tracking this tick: publish "far" so the Lua side
                            // never fires a zone from stale distances.
                            g_pSharedHands[20] = -1.0f;
                            g_pSharedHands[21] = -1.0f;
                            g_pSharedHands[22] = -1.0f;
                        }
                        // Post-solve FK refresh is DIAG-ONLY (feeds g_VRIKDbgHandFK and the
                        // vrik_diag dump); skip the full-skeleton FK on normal frames.
                        if (g_VRDiagCapture != 0) {
                            VRIK_ComputeFK(boneBuf, VRIK_FKCount());
                            if (g_VRRightBoneIdx >= 0 && g_VRRightBoneIdx < VRIK_MAX_BONES) {
                                g_VRIKDbgHandFK[0] = g_fkPos[g_VRRightBoneIdx][0];
                                g_VRIKDbgHandFK[1] = g_fkPos[g_VRRightBoneIdx][1];
                                g_VRIKDbgHandFK[2] = g_fkPos[g_VRRightBoneIdx][2];
                                g_VRIKDbgTargetTrace[0] = g_VRIKDbgTarget[0];
                                g_VRIKDbgTargetTrace[1] = g_VRIKDbgTarget[1];
                                g_VRIKDbgTargetTrace[2] = g_VRIKDbgTarget[2];
                            }
                            // Whole-rig snapshot for VRBoneModelPos/Rot -- see the arrays' comment. This is
                            // the only point in the pass where the arms are SOLVED and the FK is fresh.
                            {
                                const int n = VRIK_FKCount();
                                const int lim = (n < VRIK_MAX_BONES) ? n : VRIK_MAX_BONES;
                                for (int i = 0; i < lim; ++i) {
                                    g_VRFKSnapPos[i][0] = g_fkPos[i][0];
                                    g_VRFKSnapPos[i][1] = g_fkPos[i][1];
                                    g_VRFKSnapPos[i][2] = g_fkPos[i][2];
                                    g_VRFKSnapRot[i][0] = g_fkRot[i][0];
                                    g_VRFKSnapRot[i][1] = g_fkRot[i][1];
                                    g_VRFKSnapRot[i][2] = g_fkRot[i][2];
                                    g_VRFKSnapRot[i][3] = g_fkRot[i][3];
                                }
                                g_VRFKSnapCount = lim;
                            }
                        }
                    }
                    // Cache the solved locals of every bone this solve owns, for the
                    // same-tick replay above.
                    {
                        g_solveCacheN = 0;
                        auto cachePush = [&](int bi) {
                            if (bi < 0 || bi >= VRIK_MAX_BONES || g_solveCacheN >= 96) return;
                            for (int k = 0; k < g_solveCacheN; ++k) if (g_solveCacheIdx[k] == bi) return;
                            const float* t = reinterpret_cast<const float*>(boneBuf + bi * 48 + VRIK_TRANS_OFF);
                            const float* q = reinterpret_cast<const float*>(boneBuf + bi * 48 + VRIK_ROT_OFF);
                            g_solveCacheIdx[g_solveCacheN] = bi;
                            g_solveCacheVal[g_solveCacheN][0]=t[0]; g_solveCacheVal[g_solveCacheN][1]=t[1]; g_solveCacheVal[g_solveCacheN][2]=t[2];
                            g_solveCacheVal[g_solveCacheN][3]=q[0]; g_solveCacheVal[g_solveCacheN][4]=q[1]; g_solveCacheVal[g_solveCacheN][5]=q[2]; g_solveCacheVal[g_solveCacheN][6]=q[3];
                            ++g_solveCacheN;
                        };
                        // ROOT + ANCESTORS (audit fix, user-approved). The engine
                        // re-evaluates the locomotion ROOT from stick input on every
                        // same-tick pass (even at v=0 against a wall); replaying our
                        // body locals onto that fresh root composed the whole body
                        // shifted in the movement direction -- the strafe/sprint/shot
                        // body-vs-HMD shift, one mechanism. Cache every ancestor of
                        // the hips up to bone 0 so replay leaves the buffer
                        // bit-identical from the root down on all 4-5 passes.
                        // IN-VEHICLE: body bones are NOT ours (body chain skipped) --
                        // caching/replaying them would freeze the engine's per-pass
                        // ride pose within the tick. Cache only the arm chain we wrote.
                        if (!vrikInVehicle) {
                            {
                                int a = g_VRHipsIdx;
                                int guard = 0;
                                while (a >= 0 && a < VRIK_MAX_BONES && ++guard <= 16) {
                                    cachePush(a);
                                    a = g_VRBoneParent[a];
                                }
                            }
                            cachePush(g_VRHipsIdx);
                            for (int si = 0; si < g_VRSpineCount && si < 8; ++si) cachePush(g_VRSpineIdx[si]);
                            cachePush(g_VRNeckIdx);
                            cachePush(g_VRHeadBoneIdx);
                            cachePush(g_VRRightUpLegIdx); cachePush(g_VRRightLegIdx); cachePush(g_VRRightFootIdx);
                            cachePush(g_VRLeftUpLegIdx);  cachePush(g_VRLeftLegIdx);  cachePush(g_VRLeftFootIdx);
                        }
                        // WHEEL GRAB: an arm we did not write is not ours to replay. Caching it
                        // would freeze the engine's own per-pass arm pose inside the tick -- the same
                        // reason the body bones are left out in a vehicle -- and, worse, would keep
                        // re-applying the last solved locals over the animation, so the hand would
                        // never actually reach the wheel.
                        if (!wheelOffR) {
                            cachePush(g_VRRightUpperArmIdx >= 0 && g_VRRightUpperArmIdx < VRIK_MAX_BONES
                                      ? g_VRBoneParent[g_VRRightUpperArmIdx] : -1);
                            cachePush(g_VRRightUpperArmIdx); cachePush(g_VRRightForeArmIdx); cachePush(g_VRRightBoneIdx);
                            for (int k = 0; k < 3; ++k) cachePush(g_VRForeTwistR[k]);
                        }
                        if (!wheelOffL) {
                            cachePush(g_VRLeftUpperArmIdx >= 0 && g_VRLeftUpperArmIdx < VRIK_MAX_BONES
                                      ? g_VRBoneParent[g_VRLeftUpperArmIdx] : -1);
                            cachePush(g_VRLeftUpperArmIdx);  cachePush(g_VRLeftForeArmIdx);  cachePush(g_VRLeftBoneIdx);
                            for (int k = 0; k < 3; ++k) cachePush(g_VRForeTwistL[k]);
                        }
                        g_solveCacheTick = tickNow;
                    }
                    }
                }
            }
        }
    } __except (EXCEPTION_EXECUTE_HANDLER) {}

    return result;
}

bool InstallAnimPoseHook() {
    HMODULE hMod = GetModuleHandleA("Cyberpunk2077.exe");
    if (!hMod) return false;
    void* target = reinterpret_cast<void*>(reinterpret_cast<uintptr_t>(hMod) + 0x17DDB4);
    MH_Initialize(); // no-op if already initialized
    if (MH_CreateHook(target, &Hooked_AnimPoseApply, reinterpret_cast<void**>(&OriginalAnimPose)) != MH_OK)
        return false;
    if (MH_EnableHook(target) != MH_OK)
        return false;
    return true;
}



