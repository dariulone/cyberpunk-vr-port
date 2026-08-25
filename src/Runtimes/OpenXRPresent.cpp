// 1 (default) = label each captured frame with the head orientation the camera injection
// actually used for it, instead of the pose cache as of Present. See the use site.
extern "C" __declspec(dllexport) int CyberpunkVR_BindPoseToImage = 1;
// 1 = the SECOND EYE is submitted with the label its own image was drawn with, taken from its own
// read-back queue. 0 restores the shared label, which is what it had before, so the two can be compared
// live without a rebuild.
extern "C" __declspec(dllexport) int CyberpunkVR_VrcamOwnLabel = 1;
// How many captures actually used it. If this tracks the present rate the second eye is being labelled
// from its own frames; if it stays at zero the queue is not being filled and the eye is on MAIN's label.
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugVrcamLabelUsed = 0;
// Which eye VRCAM is. Declared here because this file needs it and does not otherwise pull in the camera
// header; defined with the rest of the camera state.
extern "C" extern int CyberpunkVR_MainIsRightEye;
// Render-ahead depth, in PRESENT INTERVALS (not in pushes -- see the ring in the header).
//
// 0, and it is not a preference -- it is the only value CONSISTENT with the exact path.
//
// The exact path matches m_framePoseSerial == S, and that serial is (interval the write
// happened in) + 1, i.e. it returns the write made during interval S-1. The ring lookup with
// lag L returns the newest entry stamped <= S-1-L. Those two agree only at L = 0. With L = 1
// the ring handed back a pose one whole interval OLDER than the exact path -- so the 82% of
// frames that hit exact and the 18% that fell through were labelled a frame apart from each
// other. Every fallback frame therefore submitted an image with a stale orientation, the
// compositor re-warped it by a rotation already baked in, and the result is the judder that
// only shows on head turns. Measured live: PoseExact 29570 vs PoseEstimated 6363.
//
// 1 and 2 stay reachable because a different runtime could genuinely render deeper, but they
// must be compared against the exact path, not chosen on feel.
extern "C" __declspec(dllexport) int CyberpunkVR_PoseFrameLag = 0;
// One [vrik] line every two seconds: present rate vs the skeleton's real update rate.
extern "C" __declspec(dllexport) int CyberpunkVR_VrikRateLog = 1;   // (declared above <cstdint>, so plain int)
// How often the frame's pose is the exact one recorded inside that frame, versus the
// interval estimate. Exact should dominate; a rising estimate count means the depth barrier
// is not firing for those frames and the binding is back to guessing there.
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugPoseExact = 0;
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugPoseEstimated = 0;
// Thread the present/capture runs on; see the write site for why this settles PoseFrameLag.
extern "C" __declspec(dllexport) unsigned int CyberpunkVR_DebugTidPresent = 0;
// How often the frame being submitted had its own published slot (pose + per-eye offset + per-eye
// FOV, all located together for THIS serial) versus had to fall back to the live values. A miss
// is the old mixed-timeline path; it should be rare and it should not grow during gameplay.
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugSlotHit  = 0;
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugSlotMiss = 0;
// Intervals that contained no locate of their own, so the frame legitimately carries the previous
// slot. Expected to be substantial whenever the game presents faster than the XR loop cycles;
// only DebugSlotMiss (no slot at all) is a real fallback to live values.
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugSlotReused = 0;
// Defined in openxr_frameloop.cpp -- which source the submitted centre pose came from.
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugPoseFromWrite;
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugPoseFromSlot;
// Frames labelled with the pose read back out of the engine at frame-open (vr_core.cpp).
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugPoseReadBack = 0;
extern "C" __declspec(dllexport) int CyberpunkVR_PoseReadBack;

// openxr_present.cpp - OnPresent(): per-Present capture/submit trigger + eye schedule.
// Split verbatim from openxr_manager.cpp (OpenXRManager method). Shared module
// state/helpers via openxr_internal.h (inline).
#include "Runtimes/OpenXRManager.hpp"
#include "Runtimes/OpenXRInternal.hpp"
#include "Utils/XrMath.hpp"
#include "Utils/SharedSlots.hpp"
extern "C" __declspec(dllexport) extern float CyberpunkVR_HandLerpSpeed;
extern "C" __declspec(dllexport) extern int CyberpunkVR_XrDeepDiag;
extern "C" __declspec(dllexport) extern float CyberpunkVR_VrikBatchGapMs;
extern "C" __declspec(dllexport) extern unsigned long long CyberpunkVR_DebugHandPerFrameLocates;
#include "Anim/CharacterRig.hpp"   // g_VrikFrameEpoch: the frame counter VRIK keys on
#include "Runtimes/RuntimeFovCorrection.hpp"
#include <cstdarg>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <cmath>
#include <vector>
#include <utility>
#include <chrono>
#include <thread>
#include <memory>
#include <algorithm>
#include <dxgi1_4.h>

extern "C" __declspec(dllexport) extern volatile uint64_t CyberpunkVR_DebugRebindLoopCalls;
extern "C" __declspec(dllexport) extern float CyberpunkVR_HeadingLeadFrames;
extern "C" __declspec(dllexport) extern float CyberpunkVR_DebugHeadingStepDeg;
extern "C" __declspec(dllexport) extern float CyberpunkVR_DebugBodyYawLagDeg;
extern "C" __declspec(dllexport) extern float CyberpunkVR_DebugBodyYawStepDeg;
extern "C" __declspec(dllexport) extern float CyberpunkVR_DebugHipsYawStepDeg;
extern "C" __declspec(dllexport) extern float CyberpunkVR_DebugYawToSolveMaxMs;
extern "C" __declspec(dllexport) extern float CyberpunkVR_DebugYawToSolveMinMs;
extern "C" __declspec(dllexport) extern float CyberpunkVR_DebugYawLagWriteDeg;
extern "C" __declspec(dllexport) extern unsigned long long CyberpunkVR_DebugYawWritesAll;
extern "C" __declspec(dllexport) extern unsigned long long CyberpunkVR_DebugYawWritesPlayer;
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugHeadingLedComps;

void OpenXRManager::OnPresent(IDXGISwapChain* swapChain) {
    // [HANDS] Shared Memory Output
    static HANDLE s_hMapFile = NULL;
    static float* s_pSharedHands = nullptr;
    if (!s_hMapFile) {
        s_hMapFile = CreateFileMappingA(INVALID_HANDLE_VALUE, NULL, PAGE_READWRITE, 0, 1024, "CyberpunkVR_Hands_Shared");
        if (s_hMapFile) {
            s_pSharedHands = (float*)MapViewOfFile(s_hMapFile, FILE_MAP_ALL_ACCESS, 0, 0, 1024);
            m_sharedHandsPtr = s_pSharedHands;   // expose to GetSharedSlot (overlay barrel crosshair)
        }
    }
    if (s_pSharedHands) {
        std::lock_guard<std::mutex> lock(const_cast<std::mutex&>(m_handMutex));
        // Slot [32]: VR hand-tracking request for the RED4ext plugin (set from the overlay menu).
        s_pSharedHands[32] = static_cast<float>(m_vrHandTrackingMode.load(std::memory_order_relaxed));
        s_pSharedHands[58] = static_cast<float>(m_weaponAimEnable.load(std::memory_order_relaxed)); // weapon-aim enable
        // shared[23]: 0/unset = immersive holsters (default), 1 = simple slot mapping. Inverted so the
        // zero-initialized shared block defaults to the immersive (current) behaviour before the first
        // publish. The CET Holster mod reads this via GetVRSharedSlot(23).
        s_pSharedHands[23] = (m_immersiveHolsters.load(std::memory_order_relaxed) != 0) ? 0.0f : 1.0f;
        s_pSharedHands[59] = 5.0f;  // mode 5 = game muzzle xform (the working solution)
        // [70..75]: anatomical HMD/body->shoulder offsets (auto-calibration result).
        // Right (rx,ry,rz), then left (lx,ly,lz). [76] = valid flag. Kept outside
        // [34..47], which is the regular calibration block.
        if (m_calibExtValid.load(std::memory_order_relaxed)) {
            for (int i = 0; i < 6; ++i) s_pSharedHands[70 + i] = m_calibExt[i].load(std::memory_order_relaxed);
            s_pSharedHands[76] = 1.0f;
        }
        // [77..80]: T-pose measured anatomy (real arm length R/L, HMD eye height) + valid flag.
        // The plugin scales the avatar arm bones to match (gizmo-path), straightening a relaxed arm.
        if (m_measureValid.load(std::memory_order_relaxed)) {
            s_pSharedHands[77] = m_userArmLenR.load(std::memory_order_relaxed);
            s_pSharedHands[78] = m_userArmLenL.load(std::memory_order_relaxed);
            s_pSharedHands[79] = m_userEyeHeight.load(std::memory_order_relaxed);
            s_pSharedHands[80] = 1.0f;
        }
        // [89]: HMD PHYSICAL height relative to the recenter base (~0 standing, negative when the
        // user physically squats). The game FPP camera Lua samples is a FIXED eye height, so the
        // plugin needs this to actually lower the body / bend the knees on a real-life squat.
        // [85..88] are written by the plugin (camera->head offset) -- do not touch them here.
        // PAIR-LOCKED: use the frozen physical head height (snapshot at the pair
        // boundary). [89] head height + [90] neck-pivot are now written from the
        // frozen snapshot inside FlushHandsToShared (published at the pair boundary,
        // BEFORE the next pair's animation) together with the hand slots [0..19], so
        // they are no longer sampled live per present here.
        // [91..93]: the ACTIVE baked camera->head offset (game-local right/fwd/up). dxgi shifts the
        // VIEW by this in LocateCamera; the plugin adds the SAME offset to camModelPos so the avatar
        // head sits exactly where the (offset-tuned) view sits -> head = camera, body follows.
        {
            float cb[3]; GetCameraOffset(cb);
            s_pSharedHands[91] = cb[0]; s_pSharedHands[92] = cb[1]; s_pSharedHands[93] = cb[2];
        }
        // IMPORTANT: hand pose slots [0..19] are flushed in OnLocateCameraCallback
        // BEFORE render (FlushHandsToShared). Do NOT rewrite them here after render,
        // or the next frame may see a mixed temporal state (one wrong frame even
        // on the flat monitor). Keep OnPresent for config/static
        // slots only.

        // [33..47] IK calibration from the overlay; [48] one-shot diag request.
        s_pSharedHands[33] = static_cast<float>(m_calibValid.load(std::memory_order_relaxed));
        for (int i = 0; i < 14; ++i) s_pSharedHands[34 + i] = m_calib[i].load(std::memory_order_relaxed);
        s_pSharedHands[48] = static_cast<float>(m_logDiagReq.load(std::memory_order_relaxed));
    }

    if (!swapChain) return;

    // Compare against CyberpunkVR_DebugTidPatchCam: equal means the camera write and the frame
    // that carries it are serialised on one thread, so PoseFrameLag 0 is right by
    // construction. Different means a simulation thread runs ahead of this one and the lag is
    // whatever that depth is.
    CyberpunkVR_DebugTidPresent = GetCurrentThreadId();

    uint64_t s_presentCount = m_presentCount.fetch_add(1, std::memory_order_relaxed) + 1;

    // ONE RENDERED FRAME FOR VRIK, published here because this is the only place in the process that
    // knows a frame has ended. The arm solve keys its per-frame cache on this; it used to key on a
    // counter CET Lua pushed through the shared block. See Anim/CharacterRig.hpp for why a frame and
    // not the XR sample rate.
    g_VrikFrameEpoch.fetch_add(1, std::memory_order_relaxed);

    // THE GAME'S OWN FRAME INTERVAL, BUCKETED -- because the AVERAGE cannot see the thing being
    // hunted, and I drew a wrong conclusion from it once already.
    //
    // The image jumps happen when one capture is submitted three times, which needs a gap of more than
    // three display periods (41.7 ms at 72 Hz) between publishes. [xrcap] proved the capture path never
    // skips: 48-57 captures per window, zero skips, zero fence waits. So the gap has to be the game's,
    // and presents/s cannot show it -- ONE 40 ms frame among fifty 20 ms frames reads as 49/s instead
    // of 50/s, which is inside the normal spread.
    //
    // So the interval is bucketed instead. Bucket 3+ counts frames that took longer than three display
    // periods; if that lands at roughly one per second, the jumps are the game's frame-time spikes and
    // no change on the XR side will remove them.
    {
        // Behind CyberpunkVR_XrDeepDiag: a clock read on EVERY present, for a number that only means
        // anything next to [xrgap]. See the flag's definition in OpenXRFrameLoop.cpp.
        static double s_prevPresentMs = 0.0;
        if (!CyberpunkVR_XrDeepDiag) {
            // Disarmed: forget the last timestamp. Otherwise arming this live from x64dbg books one
            // enormous interval measured from before it was switched off, which lands in bucket 3+
            // and reads as a stall that never happened.
            s_prevPresentMs = 0.0;
        } else {
            const double nowMs = XrDiagNowMs();
            if (s_prevPresentMs > 0.0) {
                const double dtMs = nowMs - s_prevPresentMs;
                const double periodMs =
                    (double)m_predictedDisplayPeriodNs.load(std::memory_order_relaxed) / 1.0e6;
                const double p = periodMs > 1.0 ? periodMs : 13.89;
                int b = (int)(dtMs / p);
                if (b < 0) b = 0;
                if (b > 3) b = 3;
                CyberpunkVR_DebugPresentGapBuckets[b].fetch_add(1, std::memory_order_relaxed);
                const unsigned long long dtUs = (unsigned long long)(dtMs * 1000.0);
                unsigned long long prevMax =
                    CyberpunkVR_DebugPresentGapUsMax.load(std::memory_order_relaxed);
                while (dtUs > prevMax &&
                       !CyberpunkVR_DebugPresentGapUsMax.compare_exchange_weak(
                           prevMax, dtUs, std::memory_order_relaxed)) {
                }
            }
            s_prevPresentMs = nowMs;
        }
    }
    const bool monoEnabled = m_monoSubmitEnabled.load(std::memory_order_relaxed);

    // Publish the VRIK shared slots HERE, before the next animation pass -- the readers on the
    // script side consume them during anim eval, which precedes render.
    //
    // Every present, not on a pair boundary: publishing per pair updated VRIK at HALF the present
    // rate, and hands "teleported" at 20-45 Hz while the world rendered at 90. The pair-lock
    // snapshot that used to be taken alongside is gone with AER -- see FlushHandsToShared.
    FlushHandsToShared();

    // VRIK RATE CENSUS -- the skeleton's update rate, which is not the present rate.
    //
    // READS THE COUNTERS DIRECTLY, not the shared block, and that is the point: the solve, its cache
    // and this line now live in one DLL, so a float slot in a file mapping is a hop for nothing.
    //
    // IT USED TO READ [208..223] AND I BROKE IT. Those slots were written by the solve purely to be
    // read here, and I deleted the writers one commit ago on the strength of an audit that reported
    // them as having no reader -- because the audit's pattern matched `g_pSharedHands` but not
    // `s_pSharedHands`, the static pointer this file uses. Every number below the rates went to zero
    // and nothing said so. The audit is fixed (tools/audit/slot_audit.py) and the metrics that were
    // computed inside the solve are gone for good; what is left is what has a live source.
    //
    // The clock is no longer the CET mod's onUpdate either: freshSolve is now paced by
    // g_VrikFrameEpoch, one per Present, from inside this process.
    if (CyberpunkVR_VrikRateLog) {
        static uint64_t s_last = 0, s_presPrev = 0;
        static unsigned long long s_prevApply = 0;
        static int      s_prevFresh = 0, s_prevReplay = 0;
        static uint32_t s_prevEpoch = 0;
        static unsigned long long s_prevExact = 0, s_prevEst = 0, s_prevMiss = 0, s_prevReuse = 0;
        const uint64_t now = GetTickCount64();
        const unsigned long long apply = g_AnimPoseMatchCalls;
        const int      fresh  = g_VRIKFreshTotal;
        const int      replay = g_VRIKReplayTotal;
        const uint32_t epoch  = g_VrikFrameEpoch.load(std::memory_order_relaxed);
        if (!s_last) {
            s_last = now; s_presPrev = s_presentCount;
            s_prevApply = apply; s_prevFresh = fresh; s_prevReplay = replay; s_prevEpoch = epoch;
        } else if (now - s_last >= 2000) {
            const double dt = static_cast<double>(now - s_last) / 1000.0;
            const double dApply  = static_cast<double>(apply - s_prevApply);
            const double dFresh  = static_cast<double>(fresh - s_prevFresh);
            const unsigned long long ex = CyberpunkVR_DebugPoseExact;
            const unsigned long long es = CyberpunkVR_DebugPoseEstimated;
            const unsigned long long mi = CyberpunkVR_DebugSlotMiss;
            const unsigned long long ru = CyberpunkVR_DebugSlotReused;
            const double dEx = static_cast<double>(ex - s_prevExact);
            const double dEs = static_cast<double>(es - s_prevEst);
            // freshSolve SHOULD track present now. If it does not, the epoch is not reaching the
            // anim thread and the arms are being replayed -- which is the shape of the lag this
            // whole clean-up is about. passesPerSolve is the engine's own pose-apply count per
            // frame (measured 4-5): it says the replay is doing its job of keeping those passes
            // identical, not that anything is stale.
            Log("[vrik] present=%.1f/s  frameEpoch=%.1f/s  poseApply=%.1f/s  freshSolve=%.1f/s  "
                "replay=%.1f/s  passesPerSolve=%.1f  poseExact=%.0f%%  slotMiss=%.1f/s  "
                "reused=%.1f/s\n",
                static_cast<double>(s_presentCount - s_presPrev) / dt,
                static_cast<double>(epoch - s_prevEpoch) / dt,
                dApply / dt, dFresh / dt,
                static_cast<double>(replay - s_prevReplay) / dt,
                (dFresh > 0.5) ? dApply / dFresh : 0.0,
                (dEx + dEs > 0.5) ? 100.0 * dEx / (dEx + dEs) : 0.0,
                static_cast<double>(mi - s_prevMiss) / dt,
                static_cast<double>(ru - s_prevReuse) / dt);
            // HOW OFTEN THE ENGINE RE-ESTABLISHES WORLD TRANSFORMS FROM THEIR BINDINGS.
            //
            // sub_1401D9528 is that loop, and it is upstream of everything the camera inherits --
            // it feeds SetWorldTransform, which lands in the very function our camera write
            // detours. perPresent is the number that matters: 1 means the skeleton is placed once
            // per rendered frame, and 2 means it is placed TWICE inside one image, from two
            // different world transforms. The second case is what a body double on a fast mouse
            // turn looks like while the world itself stays clean -- and it is indistinguishable
            // from a VRIK problem without this ratio, which is why it is measured rather than
            // argued about. (Same instrument settled the HUD: 150/s against 74 presents, and the
            // code already documented that node as entering twice by design.)
            {
                static uint64_t s_prevRebind = 0;
                const uint64_t rb = CyberpunkVR_DebugRebindLoopCalls;
                const double dPres = static_cast<double>(s_presentCount - s_presPrev);
                const double dRb = static_cast<double>(rb - s_prevRebind);
                s_prevRebind = rb;
                Log("[rebind] world-transform rebind loop %.1f/s | perPresent %.2f\n",
                    dRb / dt, (dPres > 0.5) ? dRb / dPres : 0.0);
            }
            // THE SCALE OF ANY HEADING PHASE ERROR, and whether a lead is being applied.
            //
            // stepPeak is the largest heading change between two compositions in this window. The
            // camera uses the TICK heading while the body is drawn with an interpolated entity
            // transform, so whatever fraction of a frame separates them, the resulting angle error
            // is bounded by this number. Standing still it is ~0 and no lead can matter; on a fast
            // mouse turn it is degrees, and then xr_heading_lead is worth turning.
            {
                static uint64_t s_prevLed = 0;
                const uint64_t led = CyberpunkVR_DebugHeadingLedComps;
                // Turn RATE only. There is no lead and no knob: the write site and the body share an
                // instant by construction (UpdateWorldTransforms runs once per rendered frame for
                // every component), so this number is context for other symptoms, not a phase error.
                Log("[heading] turn rate peak %.2f deg/frame\n", CyberpunkVR_DebugHeadingStepDeg);
                // THE BODY AGAINST THE VIEW. lag is the angle between where the view looks and
                // where the entity (= the drawn body) is turned; step is the turn rate it must be
                // read against, and hips says whether any BONE carries the turn at all. A lag of
                // about one step is one frame of delay -- the double on a flick. A lag much larger
                // than a step is a different mechanism. hips ~0 with lag > 0 means the yaw is
                // entirely in the entity transform, which is not something a bone write can fix.
                Log("[bodyyaw] view-vs-entity peak %.2f deg | entity step %.2f deg/solve | "
                    "hips model step %.2f deg\n",
                    CyberpunkVR_DebugBodyYawLagDeg,
                    CyberpunkVR_DebugBodyYawStepDeg,
                    CyberpunkVR_DebugHipsYawStepDeg);
                CyberpunkVR_DebugBodyYawLagDeg = 0.0f;
                CyberpunkVR_DebugBodyYawStepDeg = 0.0f;
                CyberpunkVR_DebugHipsYawStepDeg = 0.0f;
                // THE PHASE. yaw->solve is the age of the body yaw when the pose is solved: near
                // zero means this frame's yaw already exists and the solve can use it; near a
                // display period means the yaw is computed AFTER the animation batch and the frame
                // being drawn was baked with the previous one -- which decides whether the double is
                // fixable from a bone write at all. lag is the same statement in degrees, taken
                // against the value the write site stored (no CET push in that path). writes says
                // how many characters pass through the site and how often the player does.
                {
                    static uint64_t s_prevYawAll = 0, s_prevYawPlayer = 0;
                    const uint64_t ya = CyberpunkVR_DebugYawWritesAll;
                    const uint64_t yp = CyberpunkVR_DebugYawWritesPlayer;
                    Log("[yawphase] yaw->solve %.2f..%.2f ms | lag vs written yaw %.2f deg | "
                        "writes all %.0f/s player %.0f/s\n",
                        CyberpunkVR_DebugYawToSolveMinMs, CyberpunkVR_DebugYawToSolveMaxMs,
                        CyberpunkVR_DebugYawLagWriteDeg,
                        static_cast<double>(ya - s_prevYawAll) / dt,
                        static_cast<double>(yp - s_prevYawPlayer) / dt);
                    s_prevYawAll = ya;
                    s_prevYawPlayer = yp;
                    CyberpunkVR_DebugYawToSolveMaxMs = 0.0f;
                    CyberpunkVR_DebugYawToSolveMinMs = 0.0f;
                    CyberpunkVR_DebugYawLagWriteDeg = 0.0f;
                }
                (void)led;
                s_prevLed = led;
                CyberpunkVR_DebugHeadingStepDeg = 0.0f;   // peak per window, not per session
            }
            // WHERE THE SHAKE ENTERS: peak second difference in mm at three stages, per hand. The
            // FIRST stage that is large is the one adding it -- see VRIK_NoteShake for the reading.
            // SLOW first, because that is the regime the shake was reported in and the only one where
            // a second difference cannot be manufactured by uneven frame timing. ALL below it: if ALL
            // is much larger, those peaks are motion and timing rather than jitter.
            // xr FIRST, because it is the control: the same head-relative position sampled on the XR
            // thread's uniform 72 Hz clock. ctrl is the solve's own read of it at ~52 Hz, irregular.
            // xr quiet with ctrl loud is ALIASING, not tracking noise, and no filter can fix that.
            if (CyberpunkVR_XrDeepDiag) Log("[vrikshake] slow(<3mm/frame): L xr=%.2f ctrl=%.2f anchor=%.2f target=%.2f | "
                "R xr=%.2f ctrl=%.2f anchor=%.2f target=%.2f  mm  (nXR=%d/%d)\n",
                g_VrikShakeSlowMm[0][3], g_VrikShakeSlowMm[0][0], g_VrikShakeSlowMm[0][1],
                g_VrikShakeSlowMm[0][2],
                g_VrikShakeSlowMm[1][3], g_VrikShakeSlowMm[1][0], g_VrikShakeSlowMm[1][1],
                g_VrikShakeSlowMm[1][2],
                g_VrikShakeSlowN[0][3], g_VrikShakeSlowN[1][3]);
            if (CyberpunkVR_XrDeepDiag) Log("[vrikshake] any motion:      L xr=%.2f ctrl=%.2f anchor=%.2f target=%.2f | "
                "R xr=%.2f ctrl=%.2f anchor=%.2f target=%.2f  mm  (predict=%.2f)\n",
                g_VrikShakePeakMm[0][3], g_VrikShakePeakMm[0][0], g_VrikShakePeakMm[0][1],
                g_VrikShakePeakMm[0][2],
                g_VrikShakePeakMm[1][3], g_VrikShakePeakMm[1][0], g_VrikShakePeakMm[1][1],
                g_VrikShakePeakMm[1][2],
                (double)CyberpunkVR_HandLerpSpeed);
            // Two locates per frame (one per hand) means the per-frame path is running. Near zero with
            // xr_hand_per_frame=1 means it is not, and then the numbers above are about the old path.
            {
                // THE WINDOW IN WHICH THE SKELETON IS NOT OURS: how far the engine had moved a bone we
                // own between its own graph evaluation and our write. If a view samples in there, this
                // is how far off the arm it draws -- and with two eyes composited from two moments,
                // both arms can reach the screen at once.
                // WHICH bone and HOW OFTEN, because the two answers point at different work: a
                // quarter-metre peak in one window out of three is not the same defect as a small one
                // in every pass, and the index says whose limb is drawn in the wrong place.
                {
                    const int b = g_VrikEngineOverwriteBone;
                    const char* who = "?";
                    if (b < 0) who = "none";
                    else if (b == g_VRHipsIdx) who = "hips";
                    else if (b == g_VRNeckIdx) who = "neck";
                    else if (b == g_VRRightUpperArmIdx) who = "R upperarm";
                    else if (b == g_VRRightForeArmIdx) who = "R forearm";
                    else if (b == g_VRRightBoneIdx) who = "R hand";
                    else if (b == g_VRLeftUpperArmIdx) who = "L upperarm";
                    else if (b == g_VRLeftForeArmIdx) who = "L forearm";
                    else if (b == g_VRLeftBoneIdx) who = "L hand";
                    else {
                        for (int si = 0; si < 8; ++si) {
                            if (b == g_VRSpineIdx[si]) { who = "spine"; break; }
                        }
                    }
                    if (CyberpunkVR_XrDeepDiag) Log("[vrikshake] engine moved our bones by up to %.1f mm (bone %d = %s) in %d of %d "
                        "replay passes\n",
                        g_VrikEngineOverwriteMm, b, who,
                        g_VrikEngineOverwriteHits, g_VrikEngineOverwritePasses);
                    // WHICH pass of the batch found it moved. Pass 0 is the fresh solve and can never
                    // appear. A single column means one identifiable engine stage runs there and the
                    // fix is to write after it; a spread means something asynchronous is writing the
                    // skeleton and no ordering of ours can close the window.
                    if (CyberpunkVR_XrDeepDiag) Log("[vrikhips] overwrite by pass in batch: 1=%d 2=%d 3=%d 4=%d 5=%d 6=%d 7+=%d\n",
                        g_VrikOverwritePassHist[1], g_VrikOverwritePassHist[2],
                        g_VrikOverwritePassHist[3], g_VrikOverwritePassHist[4],
                        g_VrikOverwritePassHist[5], g_VrikOverwritePassHist[6],
                        g_VrikOverwritePassHist[7]);
                    for (int pi = 0; pi < 8; ++pi) g_VrikOverwritePassHist[pi] = 0;
                    g_VrikEngineOverwriteMm = 0.0f;
                    g_VrikEngineOverwriteBone = -1;
                    g_VrikEngineOverwriteHits = 0;
                    g_VrikEngineOverwritePasses = 0;
                }
                // THE TWO POPULATIONS THE BATCH CLOCK RESTS ON. Gaps inside one animation batch and
                // gaps between batches must stay far apart; the day sameMax approaches newMin, the
                // clock is guessing and the arms will start repeating or double-solving. Printed so
                // that day is visible in the log before it is visible in the headset.
                if (CyberpunkVR_XrDeepDiag) Log("[vrikbatch] batches %d (min gap %.2f ms) | same-batch passes %d (max gap %.2f ms) "
                    "| threshold %.2f\n",
                    g_VrikBatchGapNew, g_VrikBatchGapNewMin,
                    g_VrikBatchGapSame, g_VrikBatchGapSameMax,
                    (double)CyberpunkVR_VrikBatchGapMs);
                g_VrikBatchGapNew = 0; g_VrikBatchGapSame = 0;
                g_VrikBatchGapNewMin = 0.0f; g_VrikBatchGapSameMax = 0.0f;
                static unsigned long long s_prevLoc = 0;
                const unsigned long long loc = CyberpunkVR_DebugHandPerFrameLocates;
                if (CyberpunkVR_XrDeepDiag) Log("[vrikshake] per-frame hand locates: %.1f/s (expect ~2x present)\n",
                    static_cast<double>(loc - s_prevLoc) / dt);
                s_prevLoc = loc;
            }
            for (int hh = 0; hh < 2; ++hh) {
                for (int st = 0; st < 4; ++st) {
                    g_VrikShakePeakMm[hh][st] = 0.0f;
                    g_VrikShakeSlowMm[hh][st] = 0.0f;
                    g_VrikShakeSlowN[hh][st] = 0;
                }
            }
            s_prevApply = apply; s_prevFresh = fresh; s_prevReplay = replay; s_prevEpoch = epoch;
            s_prevExact = ex; s_prevEst = es; s_prevMiss = mi; s_prevReuse = ru;
            s_last = now; s_presPrev = s_presentCount;
        }
    }

    DXGI_SWAP_CHAIN_DESC desc{};
    if (FAILED(swapChain->GetDesc(&desc))) {
        Log("OpenXRManager: Present hook could not read swapchain desc.\n");
        return;
    }

    IDXGISwapChain3* swapChain3 = nullptr;
    UINT backBufferIndex = 0;
    if (SUCCEEDED(swapChain->QueryInterface(IID_PPV_ARGS(&swapChain3)))) {
        backBufferIndex = swapChain3->GetCurrentBackBufferIndex();
        swapChain3->Release();
    }

    ID3D12Resource* backBuffer = nullptr;
    D3D12_RESOURCE_DESC resourceDesc{};
    if (SUCCEEDED(swapChain->GetBuffer(backBufferIndex, IID_PPV_ARGS(&backBuffer)))) {
        resourceDesc = backBuffer->GetDesc();
    }

    XrPosef monoCapturedPoses[2]{};
    XrFovf monoCapturedFovs[2]{};
    bool monoCapturedViews[2] = {};
    if (monoEnabled) {
        std::lock_guard<std::mutex> viewLock(m_viewMutex);
        if (m_views.size() >= 2) {
            float fovWidth = static_cast<float>(desc.BufferDesc.Width);
            float fovHeight = static_cast<float>(desc.BufferDesc.Height);
            if ((fovWidth <= 1.0f || fovHeight <= 1.0f) && resourceDesc.Width != 0 && resourceDesc.Height != 0) {
                fovWidth = static_cast<float>(resourceDesc.Width);
                fovHeight = static_cast<float>(resourceDesc.Height);
            }

            bool hasRenderHeadPose = false;
            XrPosef renderHeadPose{};
            renderHeadPose.orientation.w = 1.0f;
            {
                std::lock_guard<std::mutex> renderLock(m_renderPoseMutex);
                uint32_t renderedSeq = GetRenderedCameraSeq();
                int idx = renderedSeq % 256;
                
                if (renderedSeq > 0 && m_poseQueueFrame[idx] == renderedSeq) {
                    renderHeadPose = m_poseQueue[idx];
                    hasRenderHeadPose = true;
                } else if (m_renderEyeHeadPoseValid[0]) {
                    renderHeadPose = m_renderEyeHeadPose[0];
                    hasRenderHeadPose = true;
                }
            }

            // THE FRAME'S OWN SLOT, not "the current values".
            //
            // Everything below -- head centre, per-eye offset, per-eye FOV -- comes from the slot
            // published when this frame's pose was located, keyed by this present's serial. That
            // is the whole point: one identity per frame. `m_views` is only the fallback for a
            // frame whose slot is missing, and a miss is counted rather than hidden, exactly like
            // RealVR's "Rendering pose entry is invalid".
            OpenXRManager::XrFrameSlot slot{};
            bool slotExact = false;
            const bool haveSlot = GetFrameSlot(s_presentCount, &slot, &slotExact);
            if (!haveSlot)      ++CyberpunkVR_DebugSlotMiss;   // no slot at all -- live fallback
            else if (slotExact) ++CyberpunkVR_DebugSlotHit;    // this frame's own locate
            else                ++CyberpunkVR_DebugSlotReused; // no locate this interval; correct

            const XrPosef* srcViewPose = haveSlot ? slot.viewPose : nullptr;
            const XrFovf*  srcViewFov  = haveSlot ? slot.viewFov  : nullptr;
            const XrVector3f headCenter =
                srcViewPose
                    ? XrVector3f{ (srcViewPose[0].position.x + srcViewPose[1].position.x) * 0.5f,
                                  (srcViewPose[0].position.y + srcViewPose[1].position.y) * 0.5f,
                                  (srcViewPose[0].position.z + srcViewPose[1].position.z) * 0.5f }
                    : XrVector3f{ (m_views[0].pose.position.x + m_views[1].pose.position.x) * 0.5f,
                                  (m_views[0].pose.position.y + m_views[1].pose.position.y) * 0.5f,
                                  (m_views[0].pose.position.z + m_views[1].pose.position.z) * 0.5f };
            XrPosef monoCenterPose{};
            if (haveSlot) {
                // The located head pose of THIS frame, already in the layer's space -- no
                // recenter round-trip needed, and no chance of the two disagreeing.
                monoCenterPose = slot.headPoseLocal;
            } else {
                monoCenterPose.orientation = m_views[0].pose.orientation;
                monoCenterPose.position = headCenter;
            }
            // BIND THE POSE TO THE IMAGE (default).
            //
            // m_views is the pose cache as of NOW; the snapshot below was rendered from the
            // sample the camera injection took while the engine built it, one frame ago.
            // Labelling the image with the newer value makes the compositor place it ahead
            // of where it was drawn, and the next frame snaps back. The error is one frame
            // period, so it hides above ~80 fps and shows plainly at 50-60 -- and no amount
            // of SpaceWarp helps, because the pose is wrong rather than missing.
            //
            // The eye offsets still come from m_views: only the head ORIENTATION and centre
            // must belong to the image; the interpupillary offsets are static geometry.
            // Exact binding first: the pose recorded inside this very frame, stamped with
            // this present. Only if that is missing (the depth barrier did not fire -- menus,
            // loading, a frame the engine composed differently) does the interval-and-lag
            // estimate below stand in.
            OpenXRHeadPose pending{};
            // ONE SOURCE OF TRUTH, AND THE SLOT IS IT.
            //
            // These older paths (GetFramePoseForSerial, then the interval-and-lag ring) predate
            // the frame slot and are keyed differently: m_framePoseSerial is stamped
            // `presentCount + 1` inside PushRenderHeadPose with no knowledge of the engine
            // pipeline depth, while the slot is published under `presentCount + 1 + depth`. With
            // depth at 0 they agree by accident; the moment it is anything else they name
            // different frames -- and since this path takes precedence, part of the stream gets
            // labelled with another frame's pose.
            //
            // Measured, that is not a subtle error: over 120 frames of head motion the gap
            // between the submitted pose and the live pose ran min 0.00 deg, max 55.63, avg 3.10.
            // A real render-to-photon latency is large but STEADY and reprojects away cleanly; a
            // gap that swings from nothing to fifty degrees is two mechanisms disagreeing, and it
            // is exactly what judder looks like from the inside.
            //
            // THE WRITTEN SAMPLE WINS OVER THE SLOT. This ordering is the point.
            //
            // slot.headPoseLocal is the frame loop's OWN xrLocateSpace. The pixels were built
            // from the sample the camera injection took (AcquireFrameHeadSample), which is a
            // different call -- same target instant, but made at a different wall-clock moment
            // and therefore a different answer, and until now also the only one that carried the
            // head POSITION the view was actually placed at. Submitting the slot meant labelling
            // the image with a pose it was never rendered from, which is exactly the thing the
            // OpenXR guide names as the cause of artifacts: the runtime cannot know which pose a
            // frame used, so what is submitted must BE that pose.
            //
            // The slot is still where the per-eye offsets and the FOV come from -- those are
            // geometry, not a sample -- and it remains the fallback for a frame with no write.
            bool haveExactPose = false;
            if (CyberpunkVR_BindPoseToImage) {
                // FIRST: the pose read back out of the engine at frame-open for THIS frame.
                // It is not derived from any assumption about render-ahead depth -- the render
                // side recognised the quaternion it was about to draw with, so this is the pose
                // in the pixels by identification rather than by arithmetic. The serial-keyed
                // paths below remain for frames where the read-back found no match (a menu, a
                // load, a frame the engine composed without our write).
                haveExactPose =
                    (CyberpunkVR_PoseReadBack &&
                     OpenXRManager::Get().PopRenderedFramePose(&pending) && pending.valid);
                if (haveExactPose) ++CyberpunkVR_DebugPoseReadBack;
                if (!haveExactPose) {
                    haveExactPose =
                        (OpenXRManager::Get().GetFramePoseForSerial(s_presentCount, &pending) &&
                         pending.valid) ||
                        (OpenXRManager::Get().GetRenderHeadPoseForPresent(
                             s_presentCount, CyberpunkVR_PoseFrameLag, &pending) &&
                         pending.valid);
                }
            }
            if (haveExactPose) {
                ++CyberpunkVR_DebugPoseExact;
                ++CyberpunkVR_DebugPoseFromWrite;
            } else if (haveSlot && slotExact) {
                // monoCenterPose already holds the slot's headPoseLocal, in the layer's space.
                ++CyberpunkVR_DebugPoseEstimated;
                ++CyberpunkVR_DebugPoseFromSlot;
            }
            if (haveExactPose) {
                // THE WHOLE POSE, AND IN THE LAYER'S SPACE. Both halves of that mattered.
                //
                // Only the orientation used to be taken from the rendered pose; the position
                // stayed the head centre as of NOW, from m_views. So the image carried the head
                // position it was drawn at while the label said where the head is at submit
                // time, and the compositor duly re-projected away a translation that was
                // already in the pixels. On head motion that is a lag-then-snap of exactly one
                // frame -- which is what "дрожь" is. The OpenXR guide states the invariant
                // plainly: the runtime has no way to know which pose a frame was rendered with,
                // so what is submitted must BE that pose. Crysis VR does the same thing the
                // simple way -- FinishFrame submits m_renderViews[eye].pose whole, the very
                // struct AwaitFrame filled.
                //
                // And the space: GetHeadPose() is recenter-relative, the layer is m_localSpace.
                // Undo the base here rather than shipping a pose from the wrong frame of
                // reference (identity base hides it; a recenter does not).
                XrPosef base{};
                OpenXRManager::Get().GetRecenterBase(&base);
                const XrQuaternionf relOri{ pending.oriX, pending.oriY, pending.oriZ, pending.oriW };
                const XrVector3f relPos{ pending.posX, pending.posY, pending.posZ };
                const XrVector3f rotated = RotateVector(base.orientation, relPos);
                monoCenterPose.orientation = MultiplyQuat(base.orientation, relOri);
                monoCenterPose.position = XrVector3f{
                    base.position.x + rotated.x,
                    base.position.y + rotated.y,
                    base.position.z + rotated.z };
            } else if (GetRenderPoseSubmit() != 0 && hasRenderHeadPose) {
                monoCenterPose = renderHeadPose;
            }

            // AND THE SECOND EYE'S OWN CENTRE, from its own queue, built by the same arithmetic as the
            // centre above -- undo the recenter base, then compose. Empty queue (the view not active, a
            // menu, a load) leaves this invalid and the eye keeps taking MAIN's centre, which is exactly
            // the previous behaviour, so this can only add information.
            XrPosef vrcamCenterPose = monoCenterPose;
            bool haveVrcamCenter = false;
            if (CyberpunkVR_BindPoseToImage && CyberpunkVR_PoseReadBack &&
                CyberpunkVR_VrcamOwnLabel) {
                OpenXRHeadPose vrPending{};
                if (OpenXRManager::Get().PopVrcamRenderedFramePose(&vrPending) && vrPending.valid) {
                    XrPosef vbase{};
                    OpenXRManager::Get().GetRecenterBase(&vbase);
                    const XrQuaternionf relOriV{ vrPending.oriX, vrPending.oriY,
                                                 vrPending.oriZ, vrPending.oriW };
                    const XrVector3f relPosV{ vrPending.posX, vrPending.posY, vrPending.posZ };
                    const XrVector3f rotatedV = RotateVector(vbase.orientation, relPosV);
                    vrcamCenterPose.orientation = MultiplyQuat(vbase.orientation, relOriV);
                    vrcamCenterPose.position = XrVector3f{
                        vbase.position.x + rotatedV.x,
                        vbase.position.y + rotatedV.y,
                        vbase.position.z + rotatedV.z };
                    haveVrcamCenter = true;
                    ++CyberpunkVR_DebugVrcamLabelUsed;
                }
            }
            const uint32_t vrcamEyeIndex = CyberpunkVR_MainIsRightEye ? 0u : 1u;
            // The eye offset is STATIC GEOMETRY IN HEAD SPACE, so it has to be rotated by the
            // orientation the frame was RENDERED with -- not by the head orientation as of now.
            //
            // m_views gives the offsets already rotated into local space by the CURRENT head
            // orientation. Adding those to a centre taken from the rendered pose mixes two
            // orientations again: the error is (current - rendered) applied to a half-IPD lever,
            // so it is zero when still and swings back and forth in step with head rotation --
            // small, but exactly the kind of oscillation that reads as swim. Take the offset
            // back into head space with the current orientation, then out again with the
            // rendered one.
            // Source geometry from the FRAME'S slot when we have it -- the eye offsets and the
            // orientation they were located with belong to the same instant as the image. Only a
            // slot miss falls back to `m_views` as of now, which is the old mixed-timeline case.
            const XrQuaternionf curHeadOri =
                srcViewPose ? srcViewPose[0].orientation : m_views[0].pose.orientation;
            const XrQuaternionf curHeadOriInv = ConjugateQuat(curHeadOri);
            for (int eye = 0; eye < 2; ++eye) {
                const XrPosef& srcPose = srcViewPose ? srcViewPose[eye] : m_views[eye].pose;
                const XrVector3f eyeOffsetLocal{
                    srcPose.position.x - headCenter.x,
                    srcPose.position.y - headCenter.y,
                    srcPose.position.z - headCenter.z};
                const XrVector3f eyeOffsetHead = RotateVector(curHeadOriInv, eyeOffsetLocal);
                // Per-eye centre: the second eye uses its own when it has one. The eye offset is
                // rotated by the centre it is being added to, or the two would be mixed timelines again.
                const bool useVrcamCentre =
                    haveVrcamCenter && (static_cast<uint32_t>(eye) == vrcamEyeIndex);
                const XrPosef& centre = useVrcamCentre ? vrcamCenterPose : monoCenterPose;
                const XrVector3f eyeOffset =
                    RotateVector(centre.orientation, eyeOffsetHead);
                monoCapturedPoses[eye] = centre;
                monoCapturedPoses[eye].position.x += eyeOffset.x;
                monoCapturedPoses[eye].position.y += eyeOffset.y;
                monoCapturedPoses[eye].position.z += eyeOffset.z;
                const XrFovf srcFov0 = srcViewFov ? srcViewFov[0] : m_views[0].fov;
                const XrFovf srcFov1 = srcViewFov ? srcViewFov[1] : m_views[1].fov;
                XrFovf monoPairFovs[2] = { srcFov0, srcFov1 };
                monoCapturedFovs[eye] = ApplyForcedProjectionFov(
                    srcViewFov ? srcViewFov[eye] : m_views[eye].fov,
                    monoPairFovs, eye, fovWidth, fovHeight);
                // No cant pose rotation (removed): in mono both eyes derive from ONE
                // frame, so a per-eye cant delta doubled the whole image.
                monoCapturedViews[eye] = true;
            }
        }
    }
    bool monoCaptureOk = false;
    if (monoEnabled && backBuffer) {
        monoCaptureOk = CaptureMonoPresentedFrame(backBuffer, resourceDesc, s_presentCount,
            monoCapturedPoses, monoCapturedFovs, monoCapturedViews);
        if (!monoCaptureOk && (s_presentCount % 300) == 1) {
            Log("OpenXRManager: Mono capture failed. serial=%llu views=(%d,%d)\n",
                static_cast<unsigned long long>(s_presentCount),
                monoCapturedViews[0] ? 1 : 0,
                monoCapturedViews[1] ? 1 : 0);
        }
    }

    std::unique_lock<std::mutex> presentLock(m_presentMutex);
        if (m_lastPresentedBackBuffer) {
            m_lastPresentedBackBuffer->Release();
            m_lastPresentedBackBuffer = nullptr;
        }

        m_lastPresentedWidth = resourceDesc.Width != 0 ? static_cast<uint32_t>(resourceDesc.Width) : desc.BufferDesc.Width;
        m_lastPresentedHeight = resourceDesc.Height != 0 ? resourceDesc.Height : desc.BufferDesc.Height;
        m_lastPresentedFormat = resourceDesc.Format != DXGI_FORMAT_UNKNOWN ? static_cast<uint32_t>(resourceDesc.Format) : static_cast<uint32_t>(desc.BufferDesc.Format);
        m_lastPresentedBufferIndex = backBufferIndex;
        m_lastPresentSerial = s_presentCount;
    if (backBuffer) {
        backBuffer->Release();
        backBuffer = nullptr;
    }

    // [HMD-Paced Frame Sync] Lock the game engine to the OpenXR compositor rate.
    // By waiting for the compositor to finish xrEndFrame, we ensure the game
    // never runs ahead, and the next GetHeadPose will have the absolutely
    // fresh predicted display time for the subsequent frame.
    //
    // Inline submit runs the XR frame loop directly from the Present hook, so there is
    // no separate frame thread to wait for here.

    if ((s_presentCount % 300) != 1) return;

    Log("OpenXRManager: Present observed. hwnd=%p size=%ux%u format=%u backbufferIndex=%u resourceWidth=%llu resourceHeight=%u sessionRunning=%d\n",
        desc.OutputWindow,
        desc.BufferDesc.Width,
        desc.BufferDesc.Height,
        static_cast<unsigned>(desc.BufferDesc.Format),
        backBufferIndex,
        static_cast<unsigned long long>(resourceDesc.Width),
        resourceDesc.Height,
        IsSessionRunning() ? 1 : 0);
}
