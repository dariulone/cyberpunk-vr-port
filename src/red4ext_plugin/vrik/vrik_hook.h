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
#include "vrik_solver.h"

extern RED4ext::Vector4 g_CameraWorldPos; 
extern int g_CalibrationBoneIndex;

// Page-readable guard: returns true only if [p, p+n) is committed and readable.
// Used by main.cpp diagnostics that dereference component pointers; __try alone
// is unreliable there, so we pre-validate the page protection.
static inline bool VRIK_IsReadable(const void* p, size_t n) {
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
extern volatile uintptr_t g_PlayerTrackBufA;
extern volatile uintptr_t g_PlayerTrackBufB;
extern volatile int       g_AnimPoseDebug;       // 1 = push bones 0..63; 2 = push single test bone
extern volatile uint64_t  g_AnimPoseTotalCalls;
extern volatile uint64_t  g_AnimPoseMatchCalls;
extern volatile uintptr_t g_AnimPoseLastBoneBuf; // last matched player bone buffer (debug)
extern volatile int       g_AnimPoseTestBone;    // single-bone test index (mode 2)
extern volatile float     g_AnimPoseTestMag;     // single-bone test magnitude (mode 2)

// VR hand binding: write VR controller pose into the hand bones each frame.
extern float* g_pSharedHands;                    // shared-memory VR hand data (16 floats/hand layout)

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
inline uint32_t g_handsStableSeq   = 0;
inline bool     g_handsStableValid = false;

inline void RefreshHandsSnapshot() {
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

// Pose-slot accessor: consistent latched value. Falls back to raw only before the
// first complete frame is latched (and if the writer hasn't started the seqlock yet,
// seq stays 0/even so the very first read still latches a coherent frame).
inline float SharedPose(int i) {
    if (g_handsStableValid) return g_handsStable[i];
    return g_pSharedHands ? g_pSharedHands[i] : 0.0f;
}
extern volatile int       g_VRBind;              // 0 off, 1=right pos, 2=right pos+rot, 3=both pos(+rot)
extern volatile float     g_VRBindScale;         // position scale (VR units -> model units)
extern volatile float     g_VRBindOffX;
extern volatile float     g_VRBindOffY;
extern volatile float     g_VRBindOffZ;
extern volatile int       g_VRBindAxis;          // axis-remap preset 0..5
// Constant per-hand wrist-orientation correction (hand-local), set live via
// SetVRHandOffset(pitch,yaw,roll,hand). Applied as handRot = mapQuat * wristCorr.
// Defaults are the calibrated values: right = euler(0,-90,0), left = euler(-180,-90,0).
extern volatile float     g_VRWristR_I, g_VRWristR_J, g_VRWristR_K, g_VRWristR_R;
extern volatile float     g_VRWristL_I, g_VRWristL_J, g_VRWristL_K, g_VRWristL_R;
// Per-hand reach scale + position offset (mode 4). Different arm lengths/heights per user.
extern volatile float     g_VRScaleR, g_VRScaleL;
extern volatile float     g_VROffRX, g_VROffRY, g_VROffRZ;
extern volatile float     g_VROffLX, g_VROffLY, g_VROffLZ;
extern volatile float     g_VRShoulderRX, g_VRShoulderRY, g_VRShoulderRZ;
extern volatile float     g_VRShoulderLX, g_VRShoulderLY, g_VRShoulderLZ;
// Per-hand elbow pole spin (degrees): fine rotation of the bend normal around the
// shoulder->hand axis, to nudge the elbow more outward/inward. 0 = natural.
extern volatile float     g_VRElbowPoleR, g_VRElbowPoleL;
extern volatile float     g_VRElbowSwingR, g_VRElbowSwingL;
extern volatile int       g_VRRightBoneIdx;      // default 24 (RightHand)
extern volatile int       g_VRFppCamIdx[5];      // Torso_fppCamera_* chain (frozen every pass)
extern volatile int       g_VRCamBoneFreeze;     // live toggle: SetVRCamBoneFreeze(0/1) from CET
extern volatile int       g_VRRightUpLegIdx;     // right hip bone (RightUpLeg)
extern volatile int       g_VRLeftUpLegIdx;      // left  hip bone (LeftUpLeg)
extern volatile int       g_VRHipsIdx;           // Hips (pelvis)
extern volatile int       g_VRRightLegIdx;       // RightLeg (knee)
extern volatile int       g_VRLeftLegIdx;        // LeftLeg  (knee)
extern volatile int       g_VRRightFootIdx;      // RightFoot
extern volatile int       g_VRLeftFootIdx;       // LeftFoot
extern volatile int       g_VRNeckIdx;           // Neck
extern volatile int       g_VRNeck1Idx;          // Neck1
extern volatile int       g_VRLeftBoneIdx;       // default 23 (LeftHand)
extern volatile int       g_VRHeadBoneIdx;       // head bone (resolved by name), -1 = none
extern volatile int       g_VREyeLeftIdx;        // LeftEye bone (resolved by name), -1 = none
extern volatile int       g_VREyeRightIdx;       // RightEye bone (resolved by name), -1 = none
extern volatile int       g_VRUseHeadRelative;   // 1 = compose hand pose relative to the head bone
extern volatile int       g_VRDiagCapture;       // 1 = snapshot bones 0..31 (pre-write) into g_VRDiagBones
extern float              g_VRDiagBones[32 * 7];  // per bone: translation(3) + quaternion(4), in buffer space

extern volatile float     g_VRPlayerYaw;          // player world yaw (degrees), pushed from Lua each frame
extern volatile float     g_VRCamI, g_VRCamJ, g_VRCamK, g_VRCamR; // FPP camera (HMD) world quaternion
// FPP camera (HMD) + player entity world position (pushed from Lua) -> used to place the IK
// hand target in MODEL space EXACTLY where the visible gizmo is (gizmo-exact 1:1). When
// g_VRCamPosValid is 0 (legacy 5-param SetVRPlayerYaw) the IK falls back to the head-relative path.
extern volatile float     g_VRCamPosX, g_VRCamPosY, g_VRCamPosZ;
extern volatile float     g_VREntityPosX, g_VREntityPosY, g_VREntityPosZ;
extern volatile int       g_VRCamPosValid;
// Player entity world orientation quaternion (i,j,k,r). world->model = conjugate(this).
extern volatile float     g_VREntityQI, g_VREntityQJ, g_VREntityQK, g_VREntityQR;
// THE single stabilized camera-local offset (cam - entity, world axes), filtered once
// in SetVRTransforms from the coherent same-push pair; also fed to the render view via
// shared [124..127] so skeleton and view consume the identical value (see main.cpp).
extern volatile float     g_VRCamPairLocalX, g_VRCamPairLocalY, g_VRCamPairLocalZ;
extern volatile int       g_VRCamPairValid;

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
inline float g_viewPkt[9] = { 0,0,0,1, 0,0,0, 0, 0 };   // [0..3]=vq, [4..6]=delta, [7]=flag111, [8]=yaw141
inline bool  g_viewPktValid = false;

// Shared snap-window trace writer (bin\x64\cyberpunkvr_snapwin.log). Used by the
// packet-latch [hk] lines AND the puppet pre-rotation [pr] lines -- one file, one
// session-truncating open, chronological.
inline void VRIK_SnapTraceLog(const char* fmt, ...) {
    static FILE* s_tf = nullptr;
    if (!s_tf) {
        char p[MAX_PATH];
        GetModuleFileNameA(nullptr, p, MAX_PATH);
        char* sl = strrchr(p, '\\');
        if (sl) *(sl + 1) = 0;
        strcat_s(p, "cyberpunkvr_snapwin.log");
        s_tf = _fsopen(p, "w", _SH_DENYNO);
    }
    if (!s_tf) return;
    va_list args;
    va_start(args, fmt);
    vfprintf(s_tf, fmt, args);
    va_end(args);
    fflush(s_tf);
}

inline void VRIK_LatchViewPacket() {
    g_viewPktValid = false;
    if (!g_pSharedHands) return;
    volatile uint32_t* seq = reinterpret_cast<volatile uint32_t*>(
        const_cast<float*>(&g_pSharedHands[143]));
    for (int tries = 0; tries < 8; ++tries) {
        const uint32_t s0 = *seq;
        if (s0 == 0u) return;                    // writer absent (older dxgi)
        if (s0 & 1u) continue;                   // write in progress
        float tmp[9];
        tmp[0] = g_pSharedHands[104]; tmp[1] = g_pSharedHands[105];
        tmp[2] = g_pSharedHands[106]; tmp[3] = g_pSharedHands[107];
        tmp[4] = g_pSharedHands[108]; tmp[5] = g_pSharedHands[109];
        tmp[6] = g_pSharedHands[110]; tmp[7] = g_pSharedHands[111];
        tmp[8] = g_pSharedHands[141];
        const float ok142 = g_pSharedHands[142];
        if (*seq == s0) {
            for (int k = 0; k < 9; ++k) g_viewPkt[k] = tmp[k];
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

inline float g_vrikViewScaleUsed = 0.0f;   // diag: 0=rejected/fallback, 2=delta-v2, 1/0.5=legacy abs
inline bool VRIK_ResolveViewPos(float out[3]) {
    g_vrikViewScaleUsed = 0.0f;
    // Prefer the per-solve latched view PACKET (one seqlocked dxgi frame shared by
    // both arms and this resolver); snapshot fallback for an older dxgi build.
    const float flag = g_viewPktValid ? g_viewPkt[7] : SharedPose(111);
    if (flag == 0.0f) return false;
    if (!g_VRCamPosValid) return false;
    const float v[3] = {
        g_viewPktValid ? g_viewPkt[4] : SharedPose(108),
        g_viewPktValid ? g_viewPkt[5] : SharedPose(109),
        g_viewPktValid ? g_viewPkt[6] : SharedPose(110) };
    if (flag == 2.0f) {
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
extern volatile float     g_VRUserArmLenR, g_VRUserArmLenL;
// Phase 2 body-under-HMD: place the chest (top of the spine) under the HMD so the upper body
// follows the headset instead of the game's animated pose. g_VRBodyUnderHMD gates it.
extern volatile int       g_VRBodyUnderHMD;   // 1 = reposition upper body under the HMD
extern volatile float     g_VRChestDrop;      // eyes -> chest, down along bodyUp (m)
extern volatile float     g_VRChestFwd;       // eyes -> chest, along bodyFwd (m, -=back)
extern volatile float     g_VRHeadDrop;       // HMD -> head bone, down along bodyUp (m)
extern volatile float     g_VRSquatThreshold; // HMD drop deadzone before the body squats (m)
// The BODY's smoothed squat (deadzone+EMA), published by VRIK_PlaceBodyUnderHMD and reused by the
// ARM shoulder anchor so body + arms squat TOGETHER with no relative twitch on sprint/jump.
// Single-TU header (writer & reader both live here).
static float s_vrSharedSquatDrop = 0.0f;
extern volatile float     g_VRCamSmooth;      // body-anchor camera low-pass (per-frame lerp; 1=off)
// Diagnostics for the body placement (model space), surfaced via LogVRDiag.
extern volatile float     g_VRIKDbgChest[3];
// Solve-side trace probes: hips MODEL-space yaw (detects locomotion root rotation
// leaking through the local-space hips lock) + right IK shoulder model position.
extern volatile float     g_VRIKDbgHipsYaw;
extern volatile float     g_VRIKDbgShModel[3];
extern volatile float     g_VRIKDbgHandFK[3];    // solved right hand, model space (post-solve FK)
extern volatile float     g_VRIKDbgTargetTrace[3]; // right-hand IK target of the last solve
// Pose-capture generation: bumped by SetVRBindMode on (re)enable so the girdle/hips
// reference captures rerun (user re-toggles VRIK standing to recalibrate).
extern volatile int       g_VRPoseCapGen;
// POST-WRITE TAMPER DETECTOR. After each solve we remember the right-hand bone's local
// translation; at the NEXT hook entry (pre-anim-rewrite... anim rewrites everything, so
// instead we count solve calls per entity tick and expose them). If the engine runs the
// player graph SEVERAL times per tick, our solve on an early pass can be partially
// overwritten by a later additive pass (strafe lean / turn-assist) that we never see.
extern volatile int       g_VRIKSolvesLastTick;   // matched solves during the PREVIOUS entSeq tick
extern volatile int       g_VRIKSolvesMaxTick;    // max solves per tick since enable
extern volatile uintptr_t g_VRIKLastBufA;         // distinct bone buffers seen within one tick
extern volatile uintptr_t g_VRIKLastBufB;
extern volatile int       g_VRIKReplayTotal;      // same-tick replays served from the solve cache
// Clavicle-aim diag [side][8]: desired joint (0..2), FK joint after aim (3..5),
// aim angle needed (6, deg), applied after cap (7, deg). side 0=R, 1=L.
extern volatile float     g_VRIKDbgClav[2][8];
extern volatile float     g_VRIKDbgChestTgt[3];

// Full-arm IK (g_VRBind == 4): hierarchy + chain indices resolved in VRIK_DoArmPlayer.
extern int16_t            g_VRBoneParent[256];     // metaRig parent index per bone
extern volatile int       g_VRBoneCount;           // bone count (0 = not resolved)
extern volatile int       g_VRFKCount;             // solver-touched bone prefix (0 = full count)
extern volatile int       g_VRRightUpperArmIdx;    // RightArm  (shoulder joint / upper-arm start)
extern volatile int       g_VRRightForeArmIdx;     // RightForeArm (elbow)
extern volatile int       g_VRLeftUpperArmIdx;     // LeftArm
extern volatile int       g_VRLeftForeArmIdx;      // LeftForeArm
extern int                g_VRForeTwistR[3];       // r_forearmTwist01..03_JNT
extern int                g_VRForeTwistL[3];       // l_forearmTwist01..03_JNT
extern int                g_VRSpineIdx[8];         // Spine* torso chain
extern volatile int       g_VRSpineCount;

// IK diagnostics (last solve, model space) -- surfaced via LogVRDiag.
extern volatile float     g_VRIKDbgTarget[3];
extern volatile float     g_VRIKDbgShoulder[3];
extern volatile float     g_VRIKDbgElbow[3];
extern volatile float     g_VRIKDbgLens[2];        // upperArmLen, foreArmLen
extern volatile float     g_VRIKDbgLocal[4];       // hand pos in body frame: lx,ly,lz, crossAmount
extern volatile float     g_VRIKDbgTargetL[3];     // same, LEFT arm
extern volatile float     g_VRIKDbgShoulderL[3];
extern volatile float     g_VRIKDbgElbowL[3];
extern volatile float     g_VRIKDbgLensL[2];
extern volatile float     g_VRIKDbgLocalL[4];

// Maps a VR-space vector to model-space per the selected preset (VR is Y-up).
static inline void VRIK_RemapAxis(int preset, const float* v, float* o) {
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

// Rotate vector v by quaternion q (q = i,j,k,r == x,y,z,w). o = q * v * q^-1.
static inline void VRIK_QuatRotateVec(const float* q, const float* v, float* o) {
    const float tx = 2.0f * (q[1] * v[2] - q[2] * v[1]);
    const float ty = 2.0f * (q[2] * v[0] - q[0] * v[2]);
    const float tz = 2.0f * (q[0] * v[1] - q[1] * v[0]);
    o[0] = v[0] + q[3] * tx + (q[1] * tz - q[2] * ty);
    o[1] = v[1] + q[3] * ty + (q[2] * tx - q[0] * tz);
    o[2] = v[2] + q[3] * tz + (q[0] * ty - q[1] * tx);
}

// Hamilton product o = a * b (both i,j,k,r == x,y,z,w).
static inline void VRIK_QuatMul(const float* a, const float* b, float* o) {
    o[0] = a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1];
    o[1] = a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0];
    o[2] = a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3];
    o[3] = a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2];
}

// Conjugate (== inverse for a unit quaternion).
static inline void VRIK_QuatConj(const float* q, float* o) {
    o[0] = -q[0]; o[1] = -q[1]; o[2] = -q[2]; o[3] = q[3];
}

static inline void VRIK_QuatNorm(float* q) {
    float n = std::sqrt(q[0]*q[0] + q[1]*q[1] + q[2]*q[2] + q[3]*q[3]);
    if (n > 1e-8f) { float inv = 1.0f / n; q[0]*=inv; q[1]*=inv; q[2]*=inv; q[3]*=inv; }
    else { q[0]=0.0f; q[1]=0.0f; q[2]=0.0f; q[3]=1.0f; }
}

static inline float VRIK_Dot3(const float* a, const float* b) {
    return a[0]*b[0] + a[1]*b[1] + a[2]*b[2];
}
static inline void VRIK_Cross3(const float* a, const float* b, float* o) {
    o[0] = a[1]*b[2] - a[2]*b[1];
    o[1] = a[2]*b[0] - a[0]*b[2];
    o[2] = a[0]*b[1] - a[1]*b[0];
}
static inline float VRIK_Norm3(float* v) {
    float n = std::sqrt(v[0]*v[0] + v[1]*v[1] + v[2]*v[2]);
    if (n > 1e-8f) { float inv = 1.0f/n; v[0]*=inv; v[1]*=inv; v[2]*=inv; }
    return n;
}
static inline float VRIK_Dist3(const float* a, const float* b) {
    float dx=a[0]-b[0], dy=a[1]-b[1], dz=a[2]-b[2];
    return std::sqrt(dx*dx + dy*dy + dz*dz);
}

// slerp(identity, q, t): a fraction t of the rotation q (used to distribute a spine bend
// across several bones so the curve is gradual instead of a single sharp joint).
static inline void VRIK_QuatScale(const float* q, float t, float* o) {
    float qq[4] = { q[0], q[1], q[2], q[3] };
    if (qq[3] < 0.0f) { qq[0]=-qq[0]; qq[1]=-qq[1]; qq[2]=-qq[2]; qq[3]=-qq[3]; } // shortest arc
    float s = std::sqrt(qq[0]*qq[0] + qq[1]*qq[1] + qq[2]*qq[2]);
    if (s < 1e-6f) { o[0]=0.0f; o[1]=0.0f; o[2]=0.0f; o[3]=1.0f; return; } // ~no rotation
    float w = qq[3]; if (w > 1.0f) w = 1.0f;
    float half = std::acos(w) * t;     // scaled half-angle
    float sn = std::sin(half), cn = std::cos(half);
    o[0] = qq[0]/s*sn; o[1] = qq[1]/s*sn; o[2] = qq[2]/s*sn; o[3] = cn;
}

// Shortest-arc unit quaternion rotating unit vector a onto unit vector b.
static inline void VRIK_QuatFromTo(const float* a, const float* b, float* o) {
    float d = VRIK_Dot3(a, b);
    if (d >= 1.0f - 1e-6f) { o[0]=0.0f; o[1]=0.0f; o[2]=0.0f; o[3]=1.0f; return; }
    if (d <= -1.0f + 1e-6f) {
        // Antiparallel: rotate 180 deg about any axis orthogonal to a.
        float ax[3] = { 1.0f, 0.0f, 0.0f };
        if (std::fabs(a[0]) > 0.9f) { ax[0]=0.0f; ax[1]=1.0f; ax[2]=0.0f; }
        float axis[3]; VRIK_Cross3(a, ax, axis); VRIK_Norm3(axis);
        o[0]=axis[0]; o[1]=axis[1]; o[2]=axis[2]; o[3]=0.0f;
        return;
    }
    float c[3]; VRIK_Cross3(a, b, c);
    o[0]=c[0]; o[1]=c[1]; o[2]=c[2]; o[3]=1.0f + d;
    VRIK_QuatNorm(o);
}

// Writes one VR hand into the destination bone buffer (48-byte QsTransform:
// rotation/quaternion @ +0, translation @ +16, scale @ +32 -- confirmed via IDA
// current pose-apply implementation). When head-relative is active the controller's
// head-local offset is rotated by the head bone's orientation and added to the
// head bone's position, so the result lands in the same buffer space as the head.
// This mirrors the working CET gizmo (worldPos = camPos + camQuat * localPos) and
// is what stops the hand from swinging when the head turns.
static inline void VRIK_WriteHand(uint8_t* boneBuf, int bIdx,
                                  const float* headPos, const float* headQuat, bool headOk,
                                  const float* vrPos, const float* vrQuat, bool writeRot) {
    if (bIdx < 0) return;

    const float s = g_VRBindScale;
    float local[3];
    VRIK_RemapAxis(g_VRBindAxis, vrPos, local);
    local[0] *= s; local[1] *= s; local[2] *= s;

    float pos[3];
    if (g_VRUseHeadRelative && headOk) {
        float rotated[3];
        VRIK_QuatRotateVec(headQuat, local, rotated);
        pos[0] = headPos[0] + rotated[0];
        pos[1] = headPos[1] + rotated[1];
        pos[2] = headPos[2] + rotated[2];
    } else {
        pos[0] = local[0];
        pos[1] = local[1];
        pos[2] = local[2];
    }
    pos[0] += g_VRBindOffX; pos[1] += g_VRBindOffY; pos[2] += g_VRBindOffZ;

    // Translation @ +0 (QsTransform), Rotation @ +16 -- see file header.
    float* t = reinterpret_cast<float*>(boneBuf + bIdx * 48 + 0);
    t[0] = pos[0]; t[1] = pos[1]; t[2] = pos[2];

    if (writeRot) {
        // VR->game axis swap, same as the gizmo's mapLocalQuat: (i, -k, j, r).
        float localQuat[4] = { vrQuat[0], -vrQuat[2], vrQuat[1], vrQuat[3] };
        float* r = reinterpret_cast<float*>(boneBuf + bIdx * 48 + 16);
        if (g_VRUseHeadRelative && headOk) {
            VRIK_QuatMul(headQuat, localQuat, r);
        } else {
            r[0] = localQuat[0]; r[1] = localQuat[1]; r[2] = localQuat[2]; r[3] = localQuat[3];
        }
    }
}

// QsTransform field offsets inside each 48-byte bone slot.
static constexpr int VRIK_TRANS_OFF = 0;   // Translation (Vector4)
static constexpr int VRIK_ROT_OFF   = 16;  // Rotation (Quaternion x,y,z,w)

// Model-space FK scratch (recomputed each matched frame). Sized generously.
static constexpr int VRIK_MAX_BONES = 256;
static float g_fkPos[VRIK_MAX_BONES][3];
static float g_fkRot[VRIK_MAX_BONES][4];

// FK walk length: only the prefix of the rig the solver actually reads
// (parents precede children, so the prefix is self-contained). Falls back to
// the full bone count until the resolve publishes the prefix.
static inline int VRIK_FKCount() {
    const int n = g_VRFKCount;
    return (n > 0) ? n : g_VRBoneCount;
}

// Forward kinematics: accumulate parent-local transforms into model space.
// Requires parent index < child index (true for these rigs / topological order).
static inline void VRIK_ComputeFK(uint8_t* boneBuf, int count) {
    if (count > VRIK_MAX_BONES) count = VRIK_MAX_BONES;
    for (int i = 0; i < count; ++i) {
        const float* lt = reinterpret_cast<float*>(boneBuf + i * 48 + VRIK_TRANS_OFF);
        const float* lr = reinterpret_cast<float*>(boneBuf + i * 48 + VRIK_ROT_OFF);
        float lpos[3] = { lt[0], lt[1], lt[2] };
        float lrot[4] = { lr[0], lr[1], lr[2], lr[3] };
        int p = g_VRBoneParent[i];
        if (p >= 0 && p < i) {
            VRIK_QuatMul(g_fkRot[p], lrot, g_fkRot[i]);
            VRIK_QuatNorm(g_fkRot[i]);
            float rp[3];
            VRIK_QuatRotateVec(g_fkRot[p], lpos, rp);
            g_fkPos[i][0] = g_fkPos[p][0] + rp[0];
            g_fkPos[i][1] = g_fkPos[p][1] + rp[1];
            g_fkPos[i][2] = g_fkPos[p][2] + rp[2];
        } else {
            g_fkRot[i][0]=lrot[0]; g_fkRot[i][1]=lrot[1]; g_fkRot[i][2]=lrot[2]; g_fkRot[i][3]=lrot[3];
            VRIK_QuatNorm(g_fkRot[i]);
            g_fkPos[i][0]=lpos[0]; g_fkPos[i][1]=lpos[1]; g_fkPos[i][2]=lpos[2];
        }
    }
}

// Writes a model-space rotation back into a bone as a LOCAL rotation, given the
// (already updated) model rotation of its parent. localRot = parentModel^-1 * modelRot.
static inline void VRIK_WriteLocalRot(uint8_t* boneBuf, int idx,
                                      const float* parentModelRot, const float* modelRot) {
    float pInv[4]; VRIK_QuatConj(parentModelRot, pInv);
    float local[4]; VRIK_QuatMul(pInv, modelRot, local);
    VRIK_QuatNorm(local);
    float* r = reinterpret_cast<float*>(boneBuf + idx * 48 + VRIK_ROT_OFF);
    r[0]=local[0]; r[1]=local[1]; r[2]=local[2]; r[3]=local[3];
}

static inline void VRIK_WriteLocalPos(uint8_t* boneBuf, int idx,
                                      const float* parentModelPos, const float* parentModelRot,
                                      const float* modelPos) {
    float delta[3] = {
        modelPos[0] - parentModelPos[0],
        modelPos[1] - parentModelPos[1],
        modelPos[2] - parentModelPos[2]
    };
    float pInv[4]; VRIK_QuatConj(parentModelRot, pInv);
    float local[3]; VRIK_QuatRotateVec(pInv, delta, local);
    float* t = reinterpret_cast<float*>(boneBuf + idx * 48 + VRIK_TRANS_OFF);
    t[0]=local[0]; t[1]=local[1]; t[2]=local[2];
}

static inline void VRIK_DampenTorsoWeaponPose(uint8_t* boneBuf) {
    // Weapon-ready upper-body poses bend the Spine* chain before VRIK runs. Neutralize only spine
    // local rotations here; clavicle/upper-arm identity is not the rig rest pose and corrupts FK.
    auto neutralize = [&](int idx) {
        if (idx < 0 || idx >= VRIK_MAX_BONES) return;
        float* q = reinterpret_cast<float*>(boneBuf + idx * 48 + VRIK_ROT_OFF);
        q[0] = 0.0f; q[1] = 0.0f; q[2] = 0.0f; q[3] = 1.0f;
    };

    int count = static_cast<int>(g_VRSpineCount);
    if (count > 0 && count <= 8) {
        for (int i = 0; i < count; ++i) neutralize(g_VRSpineIdx[i]);
    }

    // HIPS LOCK. Strafe/run locomotion rotates the PELVIS ("поворачивается туловище при
    // стрейфе") -- the spine chain above is neutralized relative to the hips, so a hips
    // twist turns the ENTIRE torso incl. the clavicle pivots and the arms drift with it.
    // The identity quat is NOT the rig rest pose for the hips (root convention differs),
    // so capture the live local rotation over the first ~90 solves (idle stance) and pin
    // it afterwards. Legs are re-solved by the leg IK below the hips either way.
    {
        const int hips = g_VRHipsIdx;
        if (hips >= 0 && hips < VRIK_MAX_BONES) {
            float* hq = reinterpret_cast<float*>(boneBuf + hips * 48 + VRIK_ROT_OFF);
            static float s_hipsRef[4] = { 0, 0, 0, 1 };
            static int   s_hipsN = 0;
            static int   s_hipsGen = -1;
            if (s_hipsGen != g_VRPoseCapGen) { s_hipsGen = g_VRPoseCapGen; s_hipsN = 0; }
            if (s_hipsN < 90) {
                if (s_hipsN == 0) {
                    s_hipsRef[0]=hq[0]; s_hipsRef[1]=hq[1]; s_hipsRef[2]=hq[2]; s_hipsRef[3]=hq[3];
                } else {
                    // Incremental average with hemisphere alignment, renormalized.
                    float d = hq[0]*s_hipsRef[0] + hq[1]*s_hipsRef[1] + hq[2]*s_hipsRef[2] + hq[3]*s_hipsRef[3];
                    const float sgn = (d >= 0.0f) ? 1.0f : -1.0f;
                    const float k = 1.0f / static_cast<float>(s_hipsN + 1);
                    s_hipsRef[0] += (sgn*hq[0] - s_hipsRef[0]) * k;
                    s_hipsRef[1] += (sgn*hq[1] - s_hipsRef[1]) * k;
                    s_hipsRef[2] += (sgn*hq[2] - s_hipsRef[2]) * k;
                    s_hipsRef[3] += (sgn*hq[3] - s_hipsRef[3]) * k;
                    const float n = std::sqrt(s_hipsRef[0]*s_hipsRef[0] + s_hipsRef[1]*s_hipsRef[1]
                                            + s_hipsRef[2]*s_hipsRef[2] + s_hipsRef[3]*s_hipsRef[3]);
                    if (n > 1e-4f) { s_hipsRef[0]/=n; s_hipsRef[1]/=n; s_hipsRef[2]/=n; s_hipsRef[3]/=n; }
                }
                ++s_hipsN;
            } else {
                hq[0]=s_hipsRef[0]; hq[1]=s_hipsRef[1]; hq[2]=s_hipsRef[2]; hq[3]=s_hipsRef[3];
            }
        }
    }
}

// GIRDLE TRANSLATION PIN. Locomotion/turn/weapon animations write local TRANSLATIONS
// into the shoulder-girdle chain (measured while strafing: upper-arm socket displaced
// +-5.7cm laterally, forearm segment +11%, avatar arms 0.5423 vs 0.6268 -- asymmetric!).
// Rotation-only solvers cannot fix moved sockets/lengths: the visible result was "торс
// поворачивается, руки отъезжают" on strafe and the arm double on snap-turn (turn-assist
// anims do the same for a few frames). Capture each bone's local translation over the
// first ~90 solves (idle, unarmed -- re-run via g_VRPoseCapGen on VRIK re-enable), then
// pin them every solve BEFORE the IK: geometry becomes anatomy-constant, animations can
// only rotate.
static inline void VRIK_PinGirdleTranslations(uint8_t* boneBuf) {
    static int   s_gen = -1;
    static int   s_n = 0;
    static float s_ref[8][3];
    if (s_gen != g_VRPoseCapGen) { s_gen = g_VRPoseCapGen; s_n = 0; }
    int idx[8];
    idx[0] = (g_VRRightUpperArmIdx >= 0 && g_VRRightUpperArmIdx < VRIK_MAX_BONES)
             ? g_VRBoneParent[g_VRRightUpperArmIdx] : -1;   // right clavicle
    idx[1] = g_VRRightUpperArmIdx;
    idx[2] = g_VRRightForeArmIdx;
    idx[3] = g_VRRightBoneIdx;                              // right hand
    idx[4] = (g_VRLeftUpperArmIdx >= 0 && g_VRLeftUpperArmIdx < VRIK_MAX_BONES)
             ? g_VRBoneParent[g_VRLeftUpperArmIdx] : -1;    // left clavicle
    idx[5] = g_VRLeftUpperArmIdx;
    idx[6] = g_VRLeftForeArmIdx;
    idx[7] = g_VRLeftBoneIdx;                               // left hand
    for (int k = 0; k < 8; ++k)
        if (idx[k] < 0 || idx[k] >= VRIK_MAX_BONES) return; // chain unresolved: skip
    if (s_n < 90) {
        const float w = 1.0f / static_cast<float>(s_n + 1);
        for (int k = 0; k < 8; ++k) {
            const float* t = reinterpret_cast<const float*>(boneBuf + idx[k] * 48 + VRIK_TRANS_OFF);
            if (s_n == 0) { s_ref[k][0]=t[0]; s_ref[k][1]=t[1]; s_ref[k][2]=t[2]; }
            else {
                s_ref[k][0] += (t[0] - s_ref[k][0]) * w;
                s_ref[k][1] += (t[1] - s_ref[k][1]) * w;
                s_ref[k][2] += (t[2] - s_ref[k][2]) * w;
            }
        }
        ++s_n;
        return;
    }
    for (int k = 0; k < 8; ++k) {
        float* t = reinterpret_cast<float*>(boneBuf + idx[k] * 48 + VRIK_TRANS_OFF);
        t[0] = s_ref[k][0]; t[1] = s_ref[k][1]; t[2] = s_ref[k][2];
    }
}

// Two-bone arm IK in model space. Rotates upper arm + forearm so the wrist reaches the model-space
// target. Algorithm follows a production-grade VR full-body solver (a baseline skeleton solver,
// `setArms`), cross-validated against established arm-IK and VR-IK conventions.
//
// Approach (different from a naive "law of cosines at the shoulder"):
//   * xDir   = unit vector from HAND -> SHOULDER (so anchored at the user's actual hand position).
//   * yDir   = unit vector PERPENDICULAR to xDir, in the plane the elbow should sit; built from
//              anatomical hints (down, back, side) projected onto the plane normal to xDir.
//   * Cosine law gives the WRIST angle (the angle of the upper/forearm/hand triangle measured at
//     the hand), so elbow = handPos + xDir * cos(wrist)*foreLen + yDir * sin(wrist)*foreLen.
//   * If hand is out of reach (hsLen > upLen+foreLen), the upper+forearm lengths are stretched
//     proportionally to cover the gap. This is what allows the arm to go COMPLETELY STRAIGHT when
//     extended — clamping the distance back to armLen-eps locks in a permanent bend (the F4VR
//     code explicitly does this stretch instead, see comments below).
//
// The elbow-direction logic uses a body-frame anatomical hint that's stable everywhere:
//   * Default: elbow points DOWN (project -bodyUp onto plane perpendicular to xDir).
//   * Fallback when arm is vertical (axis nearly parallel to -bodyUp): use -bodyFwd (back).
//   * Cross-body correction: when hand reaches across midline, swing elbow OUT to the own side.
//   * Near-straight fade: as |handToShoulder| approaches armLen, fade the cross-body swing to 0
//     so the elbow doesn't twitch sideways during a fully-extended pose.
// Quaternion from an orthonormal basis built on X (primary) + Y-hint (Gram-Schmidt).
static inline void VRIK_QuatFromAxes(const float* x, const float* yHint, float* outQ) {
    float X[3] = { x[0], x[1], x[2] }; VRIK_Norm3(X);
    float Y[3] = { yHint[0], yHint[1], yHint[2] };
    float d = VRIK_Dot3(Y, X); Y[0]-=X[0]*d; Y[1]-=X[1]*d; Y[2]-=X[2]*d;
    if (VRIK_Norm3(Y) < 1e-5f) {
        Y[0]=X[1]; Y[1]=X[2]; Y[2]=X[0];
        d = VRIK_Dot3(Y, X); Y[0]-=X[0]*d; Y[1]-=X[1]*d; Y[2]-=X[2]*d; VRIK_Norm3(Y);
    }
    float Z[3]; VRIK_Cross3(X, Y, Z);
    const float m00=X[0], m01=Y[0], m02=Z[0];
    const float m10=X[1], m11=Y[1], m12=Z[1];
    const float m20=X[2], m21=Y[2], m22=Z[2];
    float tr = m00 + m11 + m22;
    if (tr > 0.0f) {
        float s = std::sqrt(tr + 1.0f) * 2.0f;
        outQ[3]=0.25f*s; outQ[0]=(m21-m12)/s; outQ[1]=(m02-m20)/s; outQ[2]=(m10-m01)/s;
    } else if (m00 > m11 && m00 > m22) {
        float s = std::sqrt(1.0f + m00 - m11 - m22) * 2.0f;
        outQ[3]=(m21-m12)/s; outQ[0]=0.25f*s; outQ[1]=(m01+m10)/s; outQ[2]=(m02+m20)/s;
    } else if (m11 > m22) {
        float s = std::sqrt(1.0f + m11 - m00 - m22) * 2.0f;
        outQ[3]=(m02-m20)/s; outQ[0]=(m01+m10)/s; outQ[1]=0.25f*s; outQ[2]=(m12+m21)/s;
    } else {
        float s = std::sqrt(1.0f + m22 - m00 - m11) * 2.0f;
        outQ[3]=(m10-m01)/s; outQ[0]=(m02+m20)/s; outQ[1]=(m12+m21)/s; outQ[2]=0.25f*s;
    }
    VRIK_QuatNorm(outQ);
}
// Model rotation R with R*aLoc = aW and R*hLoc = hW (both pairs orthonormalised on their
// primary): R = Basis(aW,hW) * Basis(aLoc,hLoc)^-1. Fully determines a bone's SWING AND TWIST
// from its anatomical axis (aLoc: bone-local segment axis, a rig CONSTANT captured at runtime)
// and its anatomical hinge (hLoc: bone-local elbow-hinge axis, also a rig constant).
static inline void VRIK_QuatAlignTwo(const float* aLoc, const float* hLoc,
                                     const float* aW, const float* hW, float* outQ) {
    float qw[4]; VRIK_QuatFromAxes(aW, hW, qw);
    float ql[4]; VRIK_QuatFromAxes(aLoc, hLoc, ql);
    float qlc[4] = { -ql[0], -ql[1], -ql[2], ql[3] };
    VRIK_QuatMul(qw, qlc, outQ); VRIK_QuatNorm(outQ);
}
// Swing/twist decomposition: the twist component of q around a unit axis.
static inline void VRIK_QuatExtractTwist(const float* q, const float* axis, float* outT) {
    float d = q[0]*axis[0] + q[1]*axis[1] + q[2]*axis[2];
    outT[0]=axis[0]*d; outT[1]=axis[1]*d; outT[2]=axis[2]*d; outT[3]=q[3];
    float n = std::sqrt(outT[0]*outT[0]+outT[1]*outT[1]+outT[2]*outT[2]+outT[3]*outT[3]);
    if (n < 1e-6f) { outT[0]=outT[1]=outT[2]=0.0f; outT[3]=1.0f; return; }
    outT[0]/=n; outT[1]/=n; outT[2]/=n; outT[3]/=n;
    if (outT[3] < 0.0f) { outT[0]=-outT[0]; outT[1]=-outT[1]; outT[2]=-outT[2]; outT[3]=-outT[3]; }
}

// Signed twist angle of childModel relative to parentModel about a LOCAL unit axis
// (axis expressed in the parent/child local frame the relative quat lives in):
// rel = conj(parentModel) * childModel (shortest form), angle = 2*atan2(dot(rel.xyz, axis), rel.w).
static inline float VRIK_TwistAngleAbout(const float* parentModel, const float* childModel,
                                         const float* axisLocal) {
    float pc[4] = { -parentModel[0], -parentModel[1], -parentModel[2], parentModel[3] };
    float rel[4]; VRIK_QuatMul(pc, childModel, rel);
    if (rel[3] < 0.0f) { rel[0]=-rel[0]; rel[1]=-rel[1]; rel[2]=-rel[2]; rel[3]=-rel[3]; }
    const float p = rel[0]*axisLocal[0] + rel[1]*axisLocal[1] + rel[2]*axisLocal[2];
    return 2.0f * std::atan2(p, rel[3]);
}

static inline void VRIK_SolveArm(uint8_t* boneBuf, int upperIdx, int foreIdx, int handIdx,
                                 const float* targetModel, const float* handModelRot,
                                 const float* bodyRight, const float* bodyUp, const float* bodyFwd,
                                 float poleAngleRad, float swingGain, bool isLeft,
                                 bool storeDbg) {
    if (upperIdx < 0 || foreIdx < 0 || handIdx < 0) return;
    if (upperIdx >= VRIK_MAX_BONES || foreIdx >= VRIK_MAX_BONES || handIdx >= VRIK_MAX_BONES) return;

    const float* sh = g_fkPos[upperIdx];
    const float* el = g_fkPos[foreIdx];
    const float* wr = g_fkPos[handIdx];

    float curUp[3]   = { el[0]-sh[0], el[1]-sh[1], el[2]-sh[2] };
    float curFore[3] = { wr[0]-el[0], wr[1]-el[1], wr[2]-el[2] };
    float upLen   = VRIK_Norm3(curUp);
    float foreLen = VRIK_Norm3(curFore);
    if (upLen < 1e-4f || foreLen < 1e-4f) return;

    // STABLE SEGMENT LENGTHS (anti-ratchet). The HAND PIN below writes the hand bone's local
    // translation, and the engine does NOT reset it next frame -- so reading segment lengths from
    // the live FK let the pin cap feed on its own output and ratchet the forearm out to 0.5-0.9m
    // (measured). Use deterministic lengths instead: the native rest lengths cached on the very
    // first solve of the session, or the calibrated per-segment length (userArmLen * 0.5, matching
    // VRIK_ScaleArmBonesFromRest's 50/50 split) when calibration is present.
    static float s_restUpLen[2] = { 0.0f, 0.0f }, s_restForeLen[2] = { 0.0f, 0.0f };
    const int sideJ = isLeft ? 1 : 0;
    if (s_restUpLen[sideJ] <= 0.0f) { s_restUpLen[sideJ] = upLen; s_restForeLen[sideJ] = foreLen; }
    {
        const float ual = isLeft ? g_VRUserArmLenL : g_VRUserArmLenR;
        const float calibHalf = (ual >= 0.45f) ? ual * 0.5f : 0.0f;
        upLen   = (calibHalf > 0.0f) ? calibHalf : s_restUpLen[sideJ];
        foreLen = (calibHalf > 0.0f) ? calibHalf : s_restForeLen[sideJ];
    }

    // SHOULDER PROTRACTION (VRArmIK ShoulderPoser-lite). Reaching far FORWARD a real
    // shoulder slides several cm forward (scapula protracts); the avatar's fixed shoulder
    // made forward reaches land ~5cm short, so the hand pin stretched the forearm
    // rubber-style ("как в One Piece"). Slide the upper-arm root toward the reach (mostly
    // forward, a touch out), written ABSOLUTE against the captured rest local translation
    // (the engine keeps our writes; incremental would creep).
    float shP[3] = { sh[0], sh[1], sh[2] };
    {
        const int par = g_VRBoneParent[upperIdx];
        static float s_upLocRest[2][3];
        static bool  s_upLocCap[2] = { false, false };
        if (par >= 0 && par < upperIdx) {
            float* tl = reinterpret_cast<float*>(boneBuf + upperIdx * 48 + VRIK_TRANS_OFF);
            if (!s_upLocCap[sideJ]) {
                s_upLocRest[sideJ][0]=tl[0]; s_upLocRest[sideJ][1]=tl[1]; s_upLocRest[sideJ][2]=tl[2];
                s_upLocCap[sideJ] = true;
            }
            // Clean base shoulder = parent FK * rest local (undo last frame's write, which
            // is already baked into g_fkPos[upperIdx]).
            float base[3]; VRIK_QuatRotateVec(g_fkRot[par], s_upLocRest[sideJ], base);
            base[0] += g_fkPos[par][0]; base[1] += g_fkPos[par][1]; base[2] += g_fkPos[par][2];
            float toT[3] = { targetModel[0]-base[0], targetModel[1]-base[1], targetModel[2]-base[2] };
            const float dist = std::sqrt(toT[0]*toT[0] + toT[1]*toT[1] + toT[2]*toT[2]);
            const float armL0 = upLen + foreLen;
            if (dist > 1e-4f && armL0 > 1e-4f) {
                float fwdness = (toT[0]*bodyFwd[0] + toT[1]*bodyFwd[1] + toT[2]*bodyFwd[2]) / dist;
                if (fwdness < 0.0f) fwdness = 0.0f;
                float need = (dist / armL0 - 0.90f) * (1.0f / 0.12f);
                if (need < 0.0f) need = 0.0f; if (need > 1.0f) need = 1.0f;
                const float pr = need * fwdness;
                const float outS = isLeft ? 1.0f : -1.0f;   // own-side outward = -sideSign*right
                float delta[3] = {
                    (bodyFwd[0]*0.06f - outS*bodyRight[0]*0.015f) * pr,
                    (bodyFwd[1]*0.06f - outS*bodyRight[1]*0.015f) * pr,
                    (bodyFwd[2]*0.06f - outS*bodyRight[2]*0.015f) * pr,
                };
                shP[0] = base[0] + delta[0]; shP[1] = base[1] + delta[1]; shP[2] = base[2] + delta[2];
                float pc[4] = { -g_fkRot[par][0], -g_fkRot[par][1], -g_fkRot[par][2], g_fkRot[par][3] };
                float ld[3]; VRIK_QuatRotateVec(pc, delta, ld);
                tl[0] = s_upLocRest[sideJ][0] + ld[0];
                tl[1] = s_upLocRest[sideJ][1] + ld[1];
                tl[2] = s_upLocRest[sideJ][2] + ld[2];
            }
        }
    }

    // Hand -> shoulder (F4VR convention; xDir is along this).
    float handToShoulder[3] = { shP[0]-targetModel[0], shP[1]-targetModel[1], shP[2]-targetModel[2] };
    float hsLen = std::sqrt(handToShoulder[0]*handToShoulder[0]
                          + handToShoulder[1]*handToShoulder[1]
                          + handToShoulder[2]*handToShoulder[2]);
    if (hsLen < 1e-4f) return; // hand on top of shoulder; no defined direction
    float xDir[3] = { handToShoulder[0]/hsLen, handToShoulder[1]/hsLen, handToShoulder[2]/hsLen };

    // STRETCH instead of clamp (per F4VR). If the user reaches farther than the rest-pose arm
    // length, we keep the arm STRAIGHT by lengthening upLen+foreLen proportionally to cover hsLen.
    // The cosine law then computes a wrist angle of 0 (cos=1, sin=0) -> elbow lies on the
    // hand-shoulder line -> arm is perfectly straight. NOT clamping with an epsilon is the fix
    // for "не могу выпрямить руку вниз" — that bug came from forcing hsLen < upLen+foreLen.
    float upL = upLen, foreL = foreLen;
    if (hsLen > upL + foreL) {
        float diff = hsLen - upL - foreL;
        float ratio = foreL / (foreL + upL);
        foreL += ratio * diff;
        upL   += (1.0f - ratio) * diff;
    }
    // Hand far closer than |up-fore|: math has no solution; equalise to keep solver stable.
    if (hsLen < std::fabs(upL - foreL) + 1e-3f) {
        float avg = (upL + foreL) * 0.5f;
        upL = foreL = avg;
    }

    // EXTENSION SNAP. With arms hanging at the sides the real shoulder->controller distance
    // comes up a few cm short of the calibrated arm length (shoulder drop/adduction, palm-
    // center grip), so the cosine law held a permanent 10-15deg micro-bend ("по швам не
    // разгибает"). Within the last reach percents, ease the segment lengths toward exactly
    // the distance -> the arm straightens fully; max shrink a few cm, visually invisible.
    {
        const float sum0 = upL + foreL;
        if (sum0 > 1e-4f && hsLen < sum0) {
            const float r = hsLen / sum0;
            float s = (r - 0.90f) * (1.0f / 0.06f);
            if (s > 1.0f) s = 1.0f;
            if (s > 0.0f) {
                const float f = 1.0f + (r - 1.0f) * s;
                upL *= f; foreL *= f;
            }
        }
    }

    // ANATOMICAL TWIST RIG CONSTANTS (captured once per side, pose-invariant; used by the
    // geometric bend below AND the twist basis further down). The bone-local segment axis
    // conj(rot)*worldAxis and the bone-local ELBOW-HINGE axis conj(rot)*worldHinge are rigid
    // rig data. Sampled from the native pose whenever the elbow is visibly bent
    // (|upDir x foreDir| > ~10deg), hinge sign = upDir x foreDir (anatomically positive).
    // s_palmLoc = hand-bone-local PALM/finger axis (forearm continuation at capture): rotating
    // the wrist swings handRot*s_palmLoc, which feeds the FinalIK-style bend vector so
    // pronation/supination pulls the elbow naturally.
    static float s_axLocUp[2][3], s_hgLocUp[2][3], s_axLocFore[2][3], s_hgLocFore[2][3];
    static float s_palmLoc[2][3];
    static float s_armRest[2][3], s_bendRest[2][3];  // native arm axis + elbow bend dir (model)
    static float s_twLoc[2][3][4], s_twAx[2][3][3];  // twist bones: base local rot + local axis
    static float s_thCap[2] = { 0.0f, 0.0f };        // hand-vs-forearm twist at capture
    static bool  s_rigCap[2] = { false, false };
    const int sideI = isLeft ? 1 : 0;
    if (!s_rigCap[sideI]) {
        float cx[3]; VRIK_Cross3(curUp, curFore, cx);
        float cl = std::sqrt(cx[0]*cx[0] + cx[1]*cx[1] + cx[2]*cx[2]);
        if (cl > 0.17f) {
            cx[0]/=cl; cx[1]/=cl; cx[2]/=cl;
            const float* ur = g_fkRot[upperIdx];
            const float* fr = g_fkRot[foreIdx];
            const float* hr = g_fkRot[handIdx];
            float uc[4] = { -ur[0], -ur[1], -ur[2], ur[3] };
            float fc[4] = { -fr[0], -fr[1], -fr[2], fr[3] };
            float hc[4] = { -hr[0], -hr[1], -hr[2], hr[3] };
            VRIK_QuatRotateVec(uc, curUp,   s_axLocUp[sideI]);
            VRIK_QuatRotateVec(uc, cx,      s_hgLocUp[sideI]);
            VRIK_QuatRotateVec(fc, curFore, s_axLocFore[sideI]);
            VRIK_QuatRotateVec(fc, cx,      s_hgLocFore[sideI]);
            VRIK_QuatRotateVec(hc, curFore, s_palmLoc[sideI]);
            // Anatomical NEUTRAL for the VRArmIK swivel model: the native animation's arm
            // axis + the direction the ELBOW POKES. Convention check (triangle S-E-W): the
            // solver's yDir is the direction the elbow is DISPLACED from the shoulder-hand
            // line; the forearm's perpendicular component points the OPPOSITE way (from the
            // elbow back across the line). So store the NEGATED perpendicular -- capturing
            // it un-negated bent every elbow backwards.
            s_armRest[sideI][0] = curUp[0]; s_armRest[sideI][1] = curUp[1]; s_armRest[sideI][2] = curUp[2];
            const float dUF = VRIK_Dot3(curFore, curUp);
            float bnd[3] = { curUp[0]*dUF - curFore[0], curUp[1]*dUF - curFore[1], curUp[2]*dUF - curFore[2] };
            if (VRIK_Norm3(bnd) > 1e-3f) {
                s_bendRest[sideI][0]=bnd[0]; s_bendRest[sideI][1]=bnd[1]; s_bendRest[sideI][2]=bnd[2];
                // Twist chain ABSOLUTE base: local rotation + bone-local forearm axis captured
                // ONCE, plus the native hand twist at capture. Per frame the twist bones are
                // rewritten as base * axisAngle(w * (solvedTwist - captureTwist)) -- writing
                // relative to the LIVE local accumulated (the engine does not re-animate these
                // helpers every frame, so a post-multiply spiralled the forearm over time).
                s_thCap[sideI] = VRIK_TwistAngleAbout(fr, hr, s_axLocFore[sideI]);
                const int* twC = isLeft ? g_VRForeTwistL : g_VRForeTwistR;
                for (int t = 0; t < 3; ++t) {
                    const int bi = twC[t];
                    if (bi < 0 || bi >= VRIK_MAX_BONES) continue;
                    const float* bl = reinterpret_cast<const float*>(boneBuf + bi * 48 + VRIK_ROT_OFF);
                    s_twLoc[sideI][t][0]=bl[0]; s_twLoc[sideI][t][1]=bl[1];
                    s_twLoc[sideI][t][2]=bl[2]; s_twLoc[sideI][t][3]=bl[3];
                    const float* trq = g_fkRot[bi];
                    float tcq[4] = { -trq[0], -trq[1], -trq[2], trq[3] };
                    VRIK_QuatRotateVec(tcq, curFore, s_twAx[sideI][t]);
                }
                s_rigCap[sideI] = true;
            }
        }
    }

    // yDir: anatomical elbow bend direction (perpendicular to xDir = hand->shoulder axis).
    //
    // Anatomical reality: a human elbow NEVER bends forward. The bend always has a back+down
    // component relative to the body, never forward. So we BLEND down + back (NOT either-or),
    // and explicitly clamp out any forward component before normalising.
    //
    // The cross-body swing (when hand reaches across midline) blends yDir toward the own-side
    // OUT-direction; that does NOT introduce forward bend either because we re-project after.
    float sideSign = isLeft ? 1.0f : -1.0f;
    float downRef[3] = { -bodyUp[0],            -bodyUp[1],            -bodyUp[2]            };
    float backRef[3] = { -bodyFwd[0],           -bodyFwd[1],           -bodyFwd[2]           };
    float outRef[3]  = { -sideSign*bodyRight[0],-sideSign*bodyRight[1],-sideSign*bodyRight[2] };

    auto projectPerp = [](const float* v, const float* axis, float* out) {
        float d = v[0]*axis[0] + v[1]*axis[1] + v[2]*axis[2];
        out[0] = v[0] - axis[0]*d;
        out[1] = v[1] - axis[1]*d;
        out[2] = v[2] - axis[2]*d;
    };
    auto stripForwardBend = [&](float* dir) {
        float fc = VRIK_Dot3(dir, bodyFwd);
        if (fc > 0.0f) {
            dir[0] -= bodyFwd[0] * fc;
            dir[1] -= bodyFwd[1] * fc;
            dir[2] -= bodyFwd[2] * fc;
        }
        projectPerp(dir, xDir, dir);
    };
    float oPerp[3];
    projectPerp(outRef,  xDir, oPerp);
    (void)downRef; (void)backRef;

    // FinalIK-STYLE GEOMETRIC BEND (VRIK ArmSolver.GetBendNormal — the formula VRChat-class
    // products ship). Replaces the manual down+back+out blend: the bend vector is DERIVED from
    // (a) the target direction in chest space, (b) the current upper-arm axis, and (c) the hand
    // PALM axis (wrist pronation/supination pulls the elbow, FinalIK's `b -= handRot*palmAxis`).
    // Neutral rest: elbow back+down; forward reach: elbow down; high reach: elbow swings out —
    // all emergent, no per-zone constants. Chest coords (right,fwd,up), X mirrored for LEFT.
    // Every safety net below (anti-forward clamp, degenerate fallback, cross-body swing, pole
    // slider trim, straight-arm fade) still applies on top.
    float yDir[3];
    bool yFromVRArmIK = false;
    // VRArmIK SWIVEL MODEL (Parger et al., zone constants re-tuned below).
    // Zero-reference = a SYNTHETIC SYMMETRIC NEUTRAL swung onto the live target direction.
    // (v1 captured the neutral from the native animation per side -- but the two arms
    // captured in DIFFERENT animation frames, so the neutrals differed and mirrored inputs
    // produced visibly different elbows: "рассинхрон рук". The analytic neutral is mirror-
    // identical by construction.) Relaxed arm: hangs mostly down, slightly forward; elbow
    // pokes back, slightly out, slightly down. Deviation = zone model, a PURE function of
    // the normalized hand position in shoulder space; all safety clamps below still apply.
    {
        float dirN0[3] = { -xDir[0], -xDir[1], -xDir[2] };
        float armRest[3] = { -bodyUp[0] + 0.15f*bodyFwd[0] + 0.10f*outRef[0],
                             -bodyUp[1] + 0.15f*bodyFwd[1] + 0.10f*outRef[1],
                             -bodyUp[2] + 0.15f*bodyFwd[2] + 0.10f*outRef[2] };
        VRIK_Norm3(armRest);
        float bendRest[3] = { -0.90f*bodyFwd[0] + 0.35f*outRef[0] - 0.25f*bodyUp[0],
                              -0.90f*bodyFwd[1] + 0.35f*outRef[1] - 0.25f*bodyUp[1],
                              -0.90f*bodyFwd[2] + 0.35f*outRef[2] - 0.25f*bodyUp[2] };
        VRIK_Norm3(bendRest);
        float qsw[4]; VRIK_QuatFromTo(armRest, dirN0, qsw);
        float ref[3]; VRIK_QuatRotateVec(qsw, bendRest, ref);
        float refP[3]; projectPerp(ref, xDir, refP);
        if (VRIK_Norm3(refP) > 0.2f) {
            const float axLen = upLen + foreLen;
            float v[3] = { -handToShoulder[0], -handToShoulder[1], -handToShoulder[2] };
            const float nx = VRIK_Dot3(v, bodyRight) / axLen;
            const float ny = VRIK_Dot3(v, bodyUp)    / axLen;
            const float nz = VRIK_Dot3(v, bodyFwd)   / axLen;
            const float inward = nx * (isLeft ? 1.0f : -1.0f);   // + when crossing midline
            float dev = -60.0f * ny;
            float zGate = 0.6f - nz; if (zGate < 0.0f) zGate = 0.0f;
            // zWeightTop reduced 260 -> 70: at face height the stock top-zone flared the
            // elbow far outward (high shoulder strain). 70 UNDER-cancels the -60*y term
            // there -> the elbow eases slightly DOWN from neutral, only a touch out.
            dev += (ny > 0.0f) ? (70.0f * zGate * ny) : (-100.0f * zGate * (-ny));
            float xT = inward + 0.1f; if (xT < 0.0f) xT = 0.0f;
            dev += -50.0f * xT;
            if (dev >  25.0f) dev =  25.0f;   // asymmetric cap: outward/up flare is the risky side
            if (dev < -60.0f) dev = -60.0f;
            const float a = dev * 0.01745329252f * (isLeft ? 1.0f : -1.0f);
            float cxv[3]; VRIK_Cross3(xDir, refP, cxv);
            const float ca = std::cos(a), sa = std::sin(a);
            yDir[0] = refP[0]*ca + cxv[0]*sa;
            yDir[1] = refP[1]*ca + cxv[1]*sa;
            yDir[2] = refP[2]*ca + cxv[2]*sa;
            yFromVRArmIK = true;
        }
    }
    if (!yFromVRArmIK) {
        const float mir = isLeft ? -1.0f : 1.0f;
        float dirN[3] = { -xDir[0], -xDir[1], -xDir[2] };        // shoulder -> hand (unit)
        auto toChest = [&](const float* v, float* o) {
            o[0] = VRIK_Dot3(v, bodyRight) * mir;
            o[1] = VRIK_Dot3(v, bodyFwd);
            o[2] = VRIK_Dot3(v, bodyUp);
        };
        auto fromChest = [&](const float* c, float* o) {
            o[0] = bodyRight[0]*c[0]*mir + bodyFwd[0]*c[1] + bodyUp[0]*c[2];
            o[1] = bodyRight[1]*c[0]*mir + bodyFwd[1]*c[1] + bodyUp[1]*c[2];
            o[2] = bodyRight[2]*c[0]*mir + bodyFwd[2]*c[1] + bodyUp[2]*c[2];
        };
        float dC[3]; toChest(dirN, dC);
        // b = FromTo(down, dir+fwd) * back   (chest space)
        float f0[3] = { 0.0f, 0.0f, -1.0f };
        float t0[3] = { dC[0], dC[1] + 1.0f, dC[2] };
        if (VRIK_Norm3(t0) < 1e-3f) { t0[0]=dC[0]; t0[1]=dC[1]; t0[2]=dC[2]; }
        float q0[4]; VRIK_QuatFromTo(f0, t0, q0);
        float back0[3] = { 0.0f, -1.0f, 0.0f };
        float bC[3]; VRIK_QuatRotateVec(q0, back0, bC);
        float b[3]; fromChest(bC, b);
        // + target direction (NOT the live arm axis: feeding the CURRENT pose back in made
        // the bend depend on our own previous solve -> two attractors -> elbow TELEPORT).
        // b is now a PURE function of the target position: continuous, deterministic.
        b[0] += dirN[0]; b[1] += dirN[1]; b[2] += dirN[2];
        // Palm/wrist term DISABLED (user: rotating the wrist must not move the elbow).
        // Kept for future tuning: kPalmW ~0.3 would give FinalIK-style pronation coupling.
        const float kPalmW = 0.0f;
        if (kPalmW > 0.0f && s_rigCap[sideI]) {
            float pw[3]; VRIK_QuatRotateVec(handModelRot, s_palmLoc[sideI], pw);
            b[0] -= pw[0]*kPalmW; b[1] -= pw[1]*kPalmW; b[2] -= pw[2]*kPalmW;
        }
        // Elbow direction = b rejected onto the plane perpendicular to the arm axis
        // (cross(dir, cross(b, dir)) collapses to exactly this).
        projectPerp(b, xDir, yDir);
    }

    // CRITICAL anti-forward-bend clamp: project out any +bodyFwd component. The elbow MUST
    // never point in the forward direction; if the blend ended up with a forward component
    // (e.g. axis is mostly down and dPerp picked up forward), strip it here.
    stripForwardBend(yDir);

    if (VRIK_Norm3(yDir) < 0.3f) {
        // The blend degenerated (axis parallel to both bodyUp and bodyFwd, which is impossible
        // for a real body). Fall back to own-side outward.
        if (VRIK_Norm3(oPerp) > 0.3f) {
            yDir[0]=oPerp[0]; yDir[1]=oPerp[1]; yDir[2]=oPerp[2];
        } else {
            float fb[3] = { 0.0f, 0.0f, 1.0f };
            if (std::fabs(xDir[2]) > 0.9f) { fb[0]=1.0f; fb[2]=0.0f; }
            projectPerp(fb, xDir, yDir);
            VRIK_Norm3(yDir);
        }
    }

    // Cross-body swing: if the hand reaches across the body's midline, blend yDir toward the
    // own-side outward direction so the elbow follows out (anatomically correct). Fade off as
    // the arm approaches full extension (reach01 -> 1) — at straight extension yDir doesn't
    // matter, but if you keep blending you'll see a sideways elbow twitch right at the limit.
    float armLen = upL + foreL;
    float reach01 = (armLen > 1e-4f) ? (hsLen / armLen) : 0.0f;
    if (reach01 > 1.0f) reach01 = 1.0f;
    float straightFade = (reach01 - 0.92f) * (1.0f / 0.08f);
    if (straightFade < 0.0f) straightFade = 0.0f;
    if (straightFade > 1.0f) straightFade = 1.0f;
    float poleWeight = 1.0f - straightFade;

    float shoulderToHand[3] = { -handToShoulder[0], -handToShoulder[1], -handToShoulder[2] };
    float lateral = (shoulderToHand[0]*bodyRight[0]
                   + shoulderToHand[1]*bodyRight[1]
                   + shoulderToHand[2]*bodyRight[2]) * sideSign;
    float crossAmount = (lateral > 0.0f) ? (lateral / armLen) : 0.0f;
    if (VRIK_Norm3(oPerp) > 1e-3f && poleWeight > 0.0f) {
        float bendFactor = (0.90f - reach01) * (1.0f / 0.35f);
        if (bendFactor < 0.0f) bendFactor = 0.0f; if (bendFactor > 1.0f) bendFactor = 1.0f;
        float crossGate = (crossAmount - 0.05f) * (1.0f / 0.10f);
        if (crossGate < 0.0f) crossGate = 0.0f; if (crossGate > 1.0f) crossGate = 1.0f;
        // HEIGHT GATE: the cross-body swing is a CHEST-level aid. Raised to face level it
        // flared the elbow far out (strained pose, humans keep the elbow down there) --
        // fade the swing off as the hand rises above the shoulder line.
        float upAmt = (shoulderToHand[0]*bodyUp[0] + shoulderToHand[1]*bodyUp[1]
                     + shoulderToHand[2]*bodyUp[2]) / armLen;
        float hGate = 1.0f - (upAmt - 0.05f) * (1.0f / 0.30f);
        if (hGate < 0.0f) hGate = 0.0f; if (hGate > 1.0f) hGate = 1.0f;
        float f = swingGain * crossGate * bendFactor * poleWeight * hGate;
        if (f < 0.0f) f = 0.0f; if (f > 1.0f) f = 1.0f;
        yDir[0] = (1.0f-f)*yDir[0] + f*oPerp[0];
        yDir[1] = (1.0f-f)*yDir[1] + f*oPerp[1];
        yDir[2] = (1.0f-f)*yDir[2] + f*oPerp[2];
    }
    // Final anti-forward-bend clamp after blending. Elbow must never point forward.
    stripForwardBend(yDir);
    VRIK_Norm3(yDir);

    // Optional swivel (Elbow pole slider): rotate yDir around xDir by poleAngleRad. Faded by
    // poleWeight so the spin can't yank the elbow near full extension.
    {
        float swivelRad = poleAngleRad * poleWeight;
        if (std::fabs(swivelRad) > 1e-5f) {
            float c = std::cos(swivelRad), s = std::sin(swivelRad);
            float cr[3]; VRIK_Cross3(xDir, yDir, cr);
            float ad = VRIK_Dot3(xDir, yDir);
            float y2[3] = {
                yDir[0]*c + cr[0]*s + xDir[0]*ad*(1.0f-c),
                yDir[1]*c + cr[1]*s + xDir[1]*ad*(1.0f-c),
                yDir[2]*c + cr[2]*s + xDir[2]*ad*(1.0f-c),
            };
            yDir[0]=y2[0]; yDir[1]=y2[1]; yDir[2]=y2[2];
            VRIK_Norm3(yDir);
        }
    }
    stripForwardBend(yDir);
    if (VRIK_Norm3(yDir) < 1e-3f) {
        projectPerp(backRef, xDir, yDir);
        if (VRIK_Norm3(yDir) < 1e-3f) {
            projectPerp(outRef, xDir, yDir);
            VRIK_Norm3(yDir);
        }
    }

    // NEAR-EXTENSION RELAX: as the arm approaches full stretch, ease the residual bend
    // direction to anatomical DOWN(+slightly back). A nearly-straight arm hangs its tiny
    // elbow sag downward (T-pose!); holding the zone-model direction there read as
    // "не разгибается" -- the elbow poked sideways/up at 97% extension.
    {
        float rf = (reach01 - 0.88f) * (1.0f / 0.10f);
        if (rf > 1.0f) rf = 1.0f;
        if (rf > 0.0f) {
            float dn[3] = { -bodyUp[0] - 0.3f*bodyFwd[0],
                            -bodyUp[1] - 0.3f*bodyFwd[1],
                            -bodyUp[2] - 0.3f*bodyFwd[2] };
            float dnP[3]; projectPerp(dn, xDir, dnP);
            if (VRIK_Norm3(dnP) > 1e-3f) {
                yDir[0] = yDir[0]*(1.0f-rf) + dnP[0]*rf;
                yDir[1] = yDir[1]*(1.0f-rf) + dnP[1]*rf;
                yDir[2] = yDir[2]*(1.0f-rf) + dnP[2]*rf;
                if (VRIK_Norm3(yDir) < 1e-3f) { yDir[0]=dnP[0]; yDir[1]=dnP[1]; yDir[2]=dnP[2]; }
            }
        }
    }

    // TEMPORAL SMOOTHING (anti-teleport). The bend direction is now a pure function of the
    // target, but crossing workspace zones (chest reach, overhead) can still swing it fast --
    // the elbow visibly TELEPORTED between poses. A real arm re-poses smoothly: exponentially
    // smooth yDir over time (tau = 80ms) and cap the swing rate at 540 deg/s, then re-project
    // perpendicular to the arm axis (the cosine-law placement assumes yDir ⊥ xDir).
    {
        static float s_ySm[2][3];
        static bool  s_yInit[2] = { false, false };
        static long long s_yT[2] = { 0, 0 };
        LARGE_INTEGER qn, qf;
        QueryPerformanceCounter(&qn); QueryPerformanceFrequency(&qf);
        float dt = 0.016f;
        if (s_yT[sideI] != 0) {
            dt = static_cast<float>(static_cast<double>(qn.QuadPart - s_yT[sideI]) / static_cast<double>(qf.QuadPart));
            if (dt < 0.0f) dt = 0.0f;
            if (dt > 0.05f) dt = 0.05f;
        }
        s_yT[sideI] = qn.QuadPart;
        if (!s_yInit[sideI]) {
            s_ySm[sideI][0]=yDir[0]; s_ySm[sideI][1]=yDir[1]; s_ySm[sideI][2]=yDir[2];
            s_yInit[sideI] = true;
        } else {
            float alpha = 1.0f - std::exp(-dt / 0.08f);
            float dotp = s_ySm[sideI][0]*yDir[0] + s_ySm[sideI][1]*yDir[1] + s_ySm[sideI][2]*yDir[2];
            if (dotp < -1.0f) dotp = -1.0f;
            if (dotp >  1.0f) dotp =  1.0f;
            const float ang = std::acos(dotp);
            const float maxStep = 9.42477f * dt;              // 540 deg/s
            if (ang > 1e-4f && ang * alpha > maxStep) alpha = maxStep / ang;
            float ns[3] = {
                s_ySm[sideI][0] + (yDir[0]-s_ySm[sideI][0])*alpha,
                s_ySm[sideI][1] + (yDir[1]-s_ySm[sideI][1])*alpha,
                s_ySm[sideI][2] + (yDir[2]-s_ySm[sideI][2])*alpha,
            };
            if (VRIK_Norm3(ns) > 1e-3f) {
                s_ySm[sideI][0]=ns[0]; s_ySm[sideI][1]=ns[1]; s_ySm[sideI][2]=ns[2];
            } else {
                // Exactly antiparallel lerp degenerated: snap (next frames smooth from here).
                s_ySm[sideI][0]=yDir[0]; s_ySm[sideI][1]=yDir[1]; s_ySm[sideI][2]=yDir[2];
            }
        }
        float yP[3]; projectPerp(s_ySm[sideI], xDir, yP);
        if (VRIK_Norm3(yP) > 1e-3f) { yDir[0]=yP[0]; yDir[1]=yP[1]; yDir[2]=yP[2]; }
    }

    // ANATOMICAL SEPARATION (hard, applied LAST). Two facts in one clamp: (a) the humerus
    // cannot internally rotate the elbow across the body's midline at all, and (b) a human
    // elbow practically never rests flat ON the torso -- the arm always keeps a few degrees
    // of abduction ("локоть всегда слегка под углом от груди"). Enforce a MINIMUM OUTWARD
    // component (~7 deg) on the bend direction; more outward / back / down remain free.
    {
        float oP[3]; projectPerp(outRef, xDir, oP);
        if (VRIK_Norm3(oP) > 1e-3f) {
            const float minOut = 0.12f;                // ~sin(7 deg)
            const float dOut = VRIK_Dot3(yDir, oP);
            if (dOut < minOut) {
                yDir[0] += oP[0] * (minOut - dOut);
                yDir[1] += oP[1] * (minOut - dOut);
                yDir[2] += oP[2] * (minOut - dOut);
                projectPerp(yDir, xDir, yDir);
                if (VRIK_Norm3(yDir) < 1e-3f) {
                    yDir[0]=oP[0]; yDir[1]=oP[1]; yDir[2]=oP[2];
                }
            }
        }
    }

    // F4VR formula: wristAngle = acos((fore^2 + hs^2 - up^2) / (2*fore*hs)). This is the angle of
    // the triangle measured AT THE HAND, not the shoulder. Elbow position is then offset from the
    // hand by (cos*fore) along xDir + (sin*fore) along yDir.
    float cosWrist = (foreL*foreL + hsLen*hsLen - upL*upL) / (2.0f * foreL * hsLen);
    if (cosWrist < -1.0f) cosWrist = -1.0f; if (cosWrist > 1.0f) cosWrist = 1.0f;
    float sinWrist = std::sqrt(std::fmax(0.0f, 1.0f - cosWrist*cosWrist));

    float xDist = cosWrist * foreL;
    float yDist = sinWrist * foreL;
    float newElbow[3] = {
        targetModel[0] + xDir[0]*xDist + yDir[0]*yDist,
        targetModel[1] + xDir[1]*xDist + yDir[1]*yDist,
        targetModel[2] + xDir[2]*xDist + yDir[2]*yDist,
    };

    float desUp[3] = { newElbow[0]-shP[0], newElbow[1]-shP[1], newElbow[2]-shP[2] };
    VRIK_Norm3(desUp);
    float desFore[3] = { targetModel[0]-newElbow[0], targetModel[1]-newElbow[1], targetModel[2]-newElbow[2] };
    VRIK_Norm3(desFore);

    // (Rig constants captured above, before the geometric bend — see s_rigCap block.)

    // Desired hinge in model space: perpendicular to the solved bend plane (spanned by desUp and
    // desFore), anatomical sign = desUp x desFore. Near full extension the cross degenerates ->
    // fall back to the pole plane (hinge = yDir x desUp... i.e. cross(desUp, -yDir)): the forearm
    // flexes toward -yDir from the shoulder-hand line, matching the capture convention.
    float hDes[3]; VRIK_Cross3(desUp, desFore, hDes);
    {
        float hl = std::sqrt(hDes[0]*hDes[0] + hDes[1]*hDes[1] + hDes[2]*hDes[2]);
        if (hl < 0.05f) {
            float negY[3] = { -yDir[0], -yDir[1], -yDir[2] };
            VRIK_Cross3(desUp, negY, hDes);
            VRIK_Norm3(hDes);
        } else { hDes[0]/=hl; hDes[1]/=hl; hDes[2]/=hl; }
    }

    // Upper arm: base = plain swing (shortest arc from the NATIVE pose -> keeps the animation's
    // natural bicep roll). The anatomical basis is used only as a LIMIT REFERENCE: extract how
    // far the basis solution would twist the segment vs the plain swing, and apply that roll
    // only in EXTREMES -- 25deg dead zone, then eased, capped at 60deg (user: "бицепс только в
    // крайнем случае должен подворачиваться").
    float delta1[4]; VRIK_QuatFromTo(curUp, desUp, delta1);
    float swingUp[4]; VRIK_QuatMul(delta1, g_fkRot[upperIdx], swingUp); VRIK_QuatNorm(swingUp);
    float newUpModel[4] = { swingUp[0], swingUp[1], swingUp[2], swingUp[3] };
    if (s_rigCap[sideI]) {
        float basis[4]; VRIK_QuatAlignTwo(s_axLocUp[sideI], s_hgLocUp[sideI], desUp, hDes, basis);
        float swc[4] = { -swingUp[0], -swingUp[1], -swingUp[2], swingUp[3] };
        float d[4]; VRIK_QuatMul(basis, swc, d); VRIK_QuatNorm(d);
        float tw[4]; VRIK_QuatExtractTwist(d, desUp, tw);
        float proj = tw[0]*desUp[0] + tw[1]*desUp[1] + tw[2]*desUp[2];
        float angS = 2.0f * std::atan2(proj, tw[3]);           // signed twist vs plain swing
        const float kDead = 0.4363f;                           // 25 deg dead zone
        const float kCap  = 1.0472f;                           // 60 deg max applied roll
        float mag = std::fabs(angS);
        float eff = (mag <= kDead) ? 0.0f : std::fmin(mag - kDead, kCap);
        if (eff > 0.0f) {
            float s = (angS >= 0.0f ? 1.0f : -1.0f) * std::sin(eff * 0.5f);
            float aa[4] = { desUp[0]*s, desUp[1]*s, desUp[2]*s, std::cos(eff * 0.5f) };
            VRIK_QuatMul(aa, swingUp, newUpModel); VRIK_QuatNorm(newUpModel);
        }
    }

    int upParent = g_VRBoneParent[upperIdx];
    const float* upParentModel = (upParent >= 0 && upParent < VRIK_MAX_BONES) ? g_fkRot[upParent] : nullptr;
    float identity[4] = { 0,0,0,1 };
    VRIK_WriteLocalRot(boneBuf, upperIdx, upParentModel ? upParentModel : identity, newUpModel);

    // Forearm: hinge-consistent basis + LIGHT pronation: the forearm should only SLIGHTLY
    // follow the wrist roll (user feedback: 0.7 was far too much) -- 25% of the hand's twist
    // about the forearm axis, capped at 45deg; the rest stays in the wrist/hand.
    float newForeModel[4];
    if (s_rigCap[sideI]) {
        float fr0[4]; VRIK_QuatAlignTwo(s_axLocFore[sideI], s_hgLocFore[sideI], desFore, hDes, fr0);
        float frc[4] = { -fr0[0], -fr0[1], -fr0[2], fr0[3] };
        float rel[4]; VRIK_QuatMul(handModelRot, frc, rel); VRIK_QuatNorm(rel);
        float tw[4];  VRIK_QuatExtractTwist(rel, desFore, tw);
        float proj2 = tw[0]*desFore[0] + tw[1]*desFore[1] + tw[2]*desFore[2];
        float angS2 = 2.0f * std::atan2(proj2, tw[3]);
        float eff2 = angS2 * 0.25f;                            // light follow
        const float kProCap = 0.7854f;                         // 45 deg
        if (eff2 >  kProCap) eff2 =  kProCap;
        if (eff2 < -kProCap) eff2 = -kProCap;
        float s2 = std::sin(eff2 * 0.5f);
        float aa2[4] = { desFore[0]*s2, desFore[1]*s2, desFore[2]*s2, std::cos(eff2 * 0.5f) };
        VRIK_QuatMul(aa2, fr0, newForeModel); VRIK_QuatNorm(newForeModel);
    } else {
        float foreBase[3]; VRIK_QuatRotateVec(delta1, curFore, foreBase);
        float delta2[4]; VRIK_QuatFromTo(foreBase, desFore, delta2);
        float tmp[4]; VRIK_QuatMul(delta2, delta1, tmp);
        VRIK_QuatMul(tmp, g_fkRot[foreIdx], newForeModel); VRIK_QuatNorm(newForeModel);
    }
    VRIK_WriteLocalRot(boneBuf, foreIdx, newUpModel, newForeModel);

    // DISTRIBUTED STRETCH (elbow half of the hand pin). Any residual length mismatch used
    // to materialize ONLY in the hand pin -> 100% of the stretch showed on the forearm
    // (rubber-arm look). Write the ELBOW bone's parent-local translation too, so the upper
    // arm carries its share; caps keep it within anatomical looks.
    float elbowW[3] = { newElbow[0], newElbow[1], newElbow[2] };
    {
        float relU[3] = { newElbow[0]-shP[0], newElbow[1]-shP[1], newElbow[2]-shP[2] };
        const float rlU = std::sqrt(relU[0]*relU[0] + relU[1]*relU[1] + relU[2]*relU[2]);
        if (rlU > 1e-4f) {
            float clU = rlU;
            const float loU = upLen * 0.85f, hiU = upLen * 1.15f;
            if (clU < loU) clU = loU; if (clU > hiU) clU = hiU;
            elbowW[0] = shP[0] + relU[0]*(clU/rlU);
            elbowW[1] = shP[1] + relU[1]*(clU/rlU);
            elbowW[2] = shP[2] + relU[2]*(clU/rlU);
            VRIK_WriteLocalPos(boneBuf, foreIdx, shP, newUpModel, elbowW);
        }
    }

    // Hand orientation written local to the new forearm.
    VRIK_WriteLocalRot(boneBuf, handIdx, newForeModel, handModelRot);

    // FOREARM TWIST DISTRIBUTION (VRArmIK rotateHand, extended to this rig's 3-bone chain).
    // Route the DELTA of the wrist's twist about the forearm axis (our solved hand vs the
    // native animation's hand) into r/l_forearmTwist01..03 with growing weights toward the
    // wrist. Pronation/supination then skins the forearm gradually like a real radius/ulna
    // instead of snapping 100% at the wrist joint -- and the elbow stays put (the swivel
    // model above deliberately ignores wrist rotation).
    if (s_rigCap[sideI]) {
        const int* tw = isLeft ? g_VRForeTwistL : g_VRForeTwistR;
        // ABSOLUTE formulation: local = capturedBase * axisAngle(w * (solvedTwist - captureTwist)).
        // The previous incremental version post-multiplied the LIVE local rotation each solve;
        // the engine does not re-animate these helper bones every frame, so the increments
        // ACCUMULATED and the forearm skin wound up ("предплечье сильно вращается").
        const float th1 = VRIK_TwistAngleAbout(newForeModel, handModelRot, s_axLocFore[sideI]);
        float dth = th1 - s_thCap[sideI];
        while (dth >  3.14159265f) dth -= 6.28318531f;
        while (dth < -3.14159265f) dth += 6.28318531f;
        if (dth >  2.0944f) dth =  2.0944f;                       // sanity cap +-120 deg
        if (dth < -2.0944f) dth = -2.0944f;
        static const float twW[3] = { 0.2f, 0.4f, 0.6f };         // elbow -> wrist (softened)
        for (int t = 0; t < 3; ++t) {
            const int bi = tw[t];
            if (bi < 0 || bi >= VRIK_MAX_BONES) continue;
            const float half = 0.5f * dth * twW[t];
            const float sh2 = std::sin(half), ch2 = std::cos(half);
            const float* axB = s_twAx[sideI][t];
            float rq[4] = { axB[0]*sh2, axB[1]*sh2, axB[2]*sh2, ch2 };
            float* bl = reinterpret_cast<float*>(boneBuf + bi * 48 + VRIK_ROT_OFF);
            float nl[4]; VRIK_QuatMul(s_twLoc[sideI][t], rq, nl); VRIK_QuatNorm(nl);
            bl[0]=nl[0]; bl[1]=nl[1]; bl[2]=nl[2]; bl[3]=nl[3];
        }
    }

    // HAND PIN (hand == target EXACTLY, user principle "кисть = gizmo, остальное подстраивается").
    // The two-bone solve lands short whenever the avatar arm and the real reach disagree; instead
    // of letting the wrist float off the gizmo, write the hand bone's parent-local TRANSLATION so
    // the wrist sits ON the target. The visual forearm segment stretches/shrinks within anatomical
    // caps (0.7..1.3 of the native segment) -- the F4VR trade: exact hand presence over a few cm
    // of forearm skin stretch.
    {
        // Anchored at the WRITTEN elbow (elbowW), not the ideal one -- the hand's parent
        // model position is wherever the fore bone actually went.
        float rel[3] = { targetModel[0]-elbowW[0], targetModel[1]-elbowW[1], targetModel[2]-elbowW[2] };
        float rl = std::sqrt(rel[0]*rel[0] + rel[1]*rel[1] + rel[2]*rel[2]);
        if (rl > 1e-4f) {
            float cl = rl;
            const float lo = foreLen * 0.7f, hi = foreLen * 1.15f;   // foreLen is rest/calibrated (stable)
            if (cl < lo) cl = lo; if (cl > hi) cl = hi;
            float pin[3] = { elbowW[0]+rel[0]*(cl/rl), elbowW[1]+rel[1]*(cl/rl), elbowW[2]+rel[2]*(cl/rl) };
            VRIK_WriteLocalPos(boneBuf, handIdx, elbowW, newForeModel, pin);
        }
    }

    if (storeDbg) {
        volatile float* L = isLeft ? g_VRIKDbgLocalL : g_VRIKDbgLocal;
        L[0]=lateral; L[1]=crossAmount; L[2]=reach01; L[3]=0.0f;
        volatile float* T = isLeft ? g_VRIKDbgTargetL   : g_VRIKDbgTarget;
        volatile float* S = isLeft ? g_VRIKDbgShoulderL : g_VRIKDbgShoulder;
        volatile float* E = isLeft ? g_VRIKDbgElbowL    : g_VRIKDbgElbow;
        volatile float* N = isLeft ? g_VRIKDbgLensL     : g_VRIKDbgLens;
        T[0]=targetModel[0]; T[1]=targetModel[1]; T[2]=targetModel[2];
        S[0]=shP[0]; S[1]=shP[1]; S[2]=shP[2];
        E[0]=newElbow[0]; E[1]=newElbow[1]; E[2]=newElbow[2];
        N[0]=upL; N[1]=foreL;
    }
}

// Builds the model-space hand target + orientation from the VR controller.
//
// The controller pose in shared memory is HMD-local, exactly like the visible gizmo hands.
// hmdRel rotates it back into the recenter/body frame so the hand target stays in place when
// the user turns their head. The shoulder pivot comes from auto-calibration in the same body
// frame (OpenXR axes: X right, Y up, Z back) instead of from the animated head bone; deriving
// it from the head bone made the wrist target inherit small head-animation arcs.
//
// Math:
//   controller_body         = hmdRel * vrPos
//   hand_from_shoulder_body = controller_body - calibratedShoulderBody
//   mapLocal (game)         = (hfs.x, -hfs.z, hfs.y) * scale
//   outTarget               = shoulderModel + mapLocal + off
static inline void VRIK_BuildHandTarget(const float* shoulderModelPos,
                                        const float* calibratedShoulderBody,
                                        const float* hmdRel,
                                        const float* vrPos, const float* vrQuat,
                                        const float* wristCorr,
                                        float scale,
                                        const float* off,
                                        float* outTarget, float* outHandRot) {
    const float s = scale;

    // Normalise hmdRel (producer composes it without renormalising; a non-unit quat would scale
    // the rotated vector by |q|^2 -> hand drifts when head moves).
    float hq[4] = { hmdRel ? hmdRel[0] : 0.0f,
                    hmdRel ? hmdRel[1] : 0.0f,
                    hmdRel ? hmdRel[2] : 0.0f,
                    hmdRel ? hmdRel[3] : 1.0f };
    if ((hq[0]*hq[0] + hq[1]*hq[1] + hq[2]*hq[2] + hq[3]*hq[3]) > 1e-4f) VRIK_QuatNorm(hq);
    else { hq[0]=0; hq[1]=0; hq[2]=0; hq[3]=1; }

    // 1. Controller in body-frame OpenXR (cancel head orientation).
    float controllerBody[3];
    VRIK_QuatRotateVec(hq, vrPos, controllerBody);

    // 2. Shoulder offset from HMD/body origin, calibrated from the same gizmo/controller poses.
    const float shoulderBody[3] = {
        calibratedShoulderBody ? calibratedShoulderBody[0] : 0.0f,
        calibratedShoulderBody ? calibratedShoulderBody[1] : 0.0f,
        calibratedShoulderBody ? calibratedShoulderBody[2] : 0.0f
    };

    // 3. Hand position relative to shoulder, body-frame OpenXR.
    float handFromShoulderBody[3] = {
        controllerBody[0] - shoulderBody[0],
        controllerBody[1] - shoulderBody[1],
        controllerBody[2] - shoulderBody[2]
    };

    // 4. Axis-swap OpenXR -> game body, then scale by user's arm-length ratio.
    float mapLocal[3] = {
         handFromShoulderBody[0] * s,
        -handFromShoulderBody[2] * s,
         handFromShoulderBody[1] * s
    };

    // 5. Anchor at the shoulder model position.
    outTarget[0] = shoulderModelPos[0] + mapLocal[0] + off[0];
    outTarget[1] = shoulderModelPos[1] + mapLocal[1] + off[1];
    outTarget[2] = shoulderModelPos[2] + mapLocal[2] + off[2];

    // Hand orientation: cancel head rotation, axis-swap, apply per-hand wrist correction.
    float baseQuat[4];
    VRIK_QuatMul(hq, vrQuat, baseQuat);
    VRIK_QuatNorm(baseQuat);
    float mapQuat[4] = { baseQuat[0], -baseQuat[2], baseQuat[1], baseQuat[3] };
    VRIK_QuatMul(mapQuat, wristCorr, outHandRot);
    VRIK_QuatNorm(outHandRot);
}

// IK-CORRECT hand target: the controller's position in MODEL space, head-independent (via
// hmdRel), anchored at the avatar's EYE (head bone horizontal, HMD eye height). Unlike the old
// VRIK_BuildHandTarget it does NOT subtract a calibrated shoulder offset or apply a reach scale
// -- the hand goes EXACTLY where the controller is relative to the head, and the arm IK just
// pivots the (length-calibrated) arm from the shoulder to it. This matches the body-IK convention (hand target =
// controller position) and the baked view (which sits on the head), so the hand lands on the
// real controller instead of an offset-shoulder approximation.
static inline void VRIK_HandTargetModelSpace(const float* eyeAnchor, const float* hmdRel,
                                       const float* vrPos, const float* vrQuat,
                                       const float* wristCorr, const float* off,
                                       float* outTarget, float* outHandRot) {
    float hq[4] = { hmdRel ? hmdRel[0] : 0.0f, hmdRel ? hmdRel[1] : 0.0f,
                    hmdRel ? hmdRel[2] : 0.0f, hmdRel ? hmdRel[3] : 1.0f };
    if ((hq[0]*hq[0]+hq[1]*hq[1]+hq[2]*hq[2]+hq[3]*hq[3]) > 1e-4f) VRIK_QuatNorm(hq);
    else { hq[0]=0; hq[1]=0; hq[2]=0; hq[3]=1; }

    float ctrlBase[3]; VRIK_QuatRotateVec(hq, vrPos, ctrlBase);   // controller in body/base frame
    float mapLocal[3] = { ctrlBase[0], -ctrlBase[2], ctrlBase[1] }; // OpenXR -> game axes
    outTarget[0] = eyeAnchor[0] + mapLocal[0] + (off ? off[0] : 0.0f);
    outTarget[1] = eyeAnchor[1] + mapLocal[1] + (off ? off[1] : 0.0f);
    outTarget[2] = eyeAnchor[2] + mapLocal[2] + (off ? off[2] : 0.0f);

    float baseQuat[4]; VRIK_QuatMul(hq, vrQuat, baseQuat); VRIK_QuatNorm(baseQuat);
    float mapQuat[4] = { baseQuat[0], -baseQuat[2], baseQuat[1], baseQuat[3] };
    VRIK_QuatMul(mapQuat, wristCorr, outHandRot);
    VRIK_QuatNorm(outHandRot);
}

// ---------------------------------------------------------------------------
// GIZMO-EXACT (1:1) full-body anchoring helpers (Phase 1 of the full-body IK rework).
//
// The visible gizmo hands are drawn (init.lua getHandWorldPose) as
//   worldPos = camPos + camQuat * mapLocalPos(rawPos),   mapLocalPos = (x,-z,y)
// i.e. PURELY camera(HMD)-relative. The old IK built its target a different way
// (hmdRel + an animated-head-derived shoulder + a reach scale), so the hand never
// matched the gizmo and the arm anchor drifted with weapon/recoil animation.
//
// These helpers reproduce the gizmo formula in the bone buffer's MODEL space, so
// the hand lands EXACTLY on the gizmo and the shoulder is anchored to the stable
// HMD frame (not the animated spine). That single change fixes: hand!=gizmo, head
// drifting on weapon draw, and the shoulder kick on fire.

// FPP camera (HMD) pose in model space, from the world camera + entity transforms pushed by
// Lua. The bone buffer's model->world transform is the entity transform (entityPos, entityQuat),
// so world->model rotation = conjugate(entityQuat). Using the FULL entity quaternion (not a
// Rz(-yaw) guess) is what makes this exact and convention-proof -- the hand then renders exactly
// on the gizmo.
static inline bool VRIK_ComputeCamModel(float* outPos, float* outRot) {
    if (!g_VRCamPosValid) return false;
    float entQ[4] = { g_VREntityQI, g_VREntityQJ, g_VREntityQK, g_VREntityQR };
    if ((entQ[0]*entQ[0]+entQ[1]*entQ[1]+entQ[2]*entQ[2]+entQ[3]*entQ[3]) < 1e-6f) {
        entQ[0]=0; entQ[1]=0; entQ[2]=0; entQ[3]=1;
    }
    VRIK_QuatNorm(entQ);
    float invEnt[4]; VRIK_QuatConj(entQ, invEnt);   // world->model rotation
    float camQ[4] = { g_VRCamI, g_VRCamJ, g_VRCamK, g_VRCamR };
    VRIK_QuatMul(invEnt, camQ, outRot); VRIK_QuatNorm(outRot);
    // WELDED BODY ANCHOR (single-filter architecture). g_VRCamPairLocal* is THE one
    // stabilized (cam - entity) offset computed in SetVRTransforms; the rendered view
    // applies the IDENTICAL value (shared [124..127] -> dxgi stabilizer). Same number
    // on both sides => body and view cannot move relative to each other, and bob/kick
    // are filtered out of both simultaneously.
    float d[3];
    if (g_VRCamPairValid) {
        d[0] = g_VRCamPairLocalX;
        d[1] = g_VRCamPairLocalY;
        d[2] = g_VRCamPairLocalZ;
    } else {
        d[0] = g_VRCamPosX - g_VREntityPosX;
        d[1] = g_VRCamPosY - g_VREntityPosY;
        d[2] = g_VRCamPosZ - g_VREntityPosZ;
    }
    VRIK_QuatRotateVec(invEnt, d, outPos);
    return true;
}

static inline void VRIK_BodyAxesFromCamYaw(const float* camModelRot,
                                           float* bodyRight, float* bodyUp, float* bodyFwd) {
    bodyUp[0] = 0.0f; bodyUp[1] = 0.0f; bodyUp[2] = 1.0f;

    float fwdLocal[3] = { 0.0f, 1.0f, 0.0f }; // OpenXR -Z forward maps to game/model +Y.
    VRIK_QuatRotateVec(camModelRot, fwdLocal, bodyFwd);
    bodyFwd[2] = 0.0f;
    if (VRIK_Norm3(bodyFwd) < 1e-4f) { bodyFwd[0]=0.0f; bodyFwd[1]=1.0f; bodyFwd[2]=0.0f; }

    VRIK_Cross3(bodyFwd, bodyUp, bodyRight);
    if (VRIK_Norm3(bodyRight) < 1e-4f) { bodyRight[0]=1.0f; bodyRight[1]=0.0f; bodyRight[2]=0.0f; }
    VRIK_Cross3(bodyUp, bodyRight, bodyFwd);
    if (VRIK_Norm3(bodyFwd) < 1e-4f) { bodyFwd[0]=0.0f; bodyFwd[1]=1.0f; bodyFwd[2]=0.0f; }
}

// Gizmo-exact hand target + HMD-anchored shoulder, both in model space.
//   target   = camModelPos + camModelRot * mapLocalPos(controllerOpenXR)   (== gizmo)
//   shoulder = camModelPos + camModelRot * mapLocalPos(shoulderOpenXR)      (HMD-anchored)
//   handRot  = camModelRot * mapLocalQuat(controllerQuat) * wristCorr
// shoulderOpenXR is the calibrated HMD-local shoulder offset (OpenXR axes: X right,
// Y up, Z back) -> mapped (x,-z,y) the SAME way as the controller, so the whole arm
// frame is consistent with the gizmo.
static inline void VRIK_BuildHandTargetGizmo(const float* camModelPos, const float* camModelRot,
                                             const float* vrPos, const float* vrQuat,
                                             const float* shoulderOpenXR, const float* wristCorr,
                                             const float* off,
                                             float* outTarget, float* outHandRot,
                                             float* outShoulderModel) {
    float mapPos[3] = { vrPos[0], -vrPos[2], vrPos[1] };
    float rotP[3];  VRIK_QuatRotateVec(camModelRot, mapPos, rotP);
    outTarget[0] = camModelPos[0] + rotP[0] + (off ? off[0] : 0.0f);
    outTarget[1] = camModelPos[1] + rotP[1] + (off ? off[1] : 0.0f);
    outTarget[2] = camModelPos[2] + rotP[2] + (off ? off[2] : 0.0f);

    float mapSh[3] = { shoulderOpenXR[0], -shoulderOpenXR[2], shoulderOpenXR[1] };
    float rotS[3];  VRIK_QuatRotateVec(camModelRot, mapSh, rotS);
    outShoulderModel[0] = camModelPos[0] + rotS[0];
    outShoulderModel[1] = camModelPos[1] + rotS[1];
    outShoulderModel[2] = camModelPos[2] + rotS[2];

    float localQuat[4] = { vrQuat[0], -vrQuat[2], vrQuat[1], vrQuat[3] };
    float hm[4]; VRIK_QuatMul(camModelRot, localQuat, hm);
    VRIK_QuatMul(hm, wristCorr, outHandRot);
    VRIK_QuatNorm(outHandRot);
}

// Arm-length calibration. A bone's parent-local translation IS its segment length, so the
// avatar arm length = |foreArm.translation| (upper-arm) + |hand.translation| (forearm).
// We scale those two translations so the avatar arm length matches the user's T-pose-measured
// arm length: a straight real arm then yields a straight avatar arm at the 1:1 gizmo target
// (fixes "arm down but elbow still bent"). Hand + fingers keep their normal size.
static inline float VRIK_ArmScale(uint8_t* boneBuf, int foreIdx, int handIdx, float userArmLen) {
    // Reject implausible T-pose measurements (real shoulder->wrist is ~0.45..0.85 m). A bad
    // calibration frame must NOT shrink the avatar arm -> fall back to the avatar's own length.
    if (userArmLen < 0.45f || userArmLen > 0.85f || foreIdx < 0 || handIdx < 0
        || foreIdx >= VRIK_MAX_BONES || handIdx >= VRIK_MAX_BONES) return 1.0f;
    auto segLen = [&](int idx) -> float {
        const float* t = reinterpret_cast<float*>(boneBuf + idx * 48 + VRIK_TRANS_OFF);
        return std::sqrt(t[0]*t[0] + t[1]*t[1] + t[2]*t[2]);
    };
    float avatar = segLen(foreIdx) + segLen(handIdx);
    if (avatar < 0.05f) return 1.0f;
    float s = userArmLen / avatar;
    if (s < 0.7f) s = 0.7f; if (s > 1.3f) s = 1.3f;
    return s;
}

static inline void VRIK_ScaleArmBones(uint8_t* boneBuf, int foreIdx, int handIdx, float scale) {
    if (scale <= 0.0f || std::fabs(scale - 1.0f) < 1e-3f) return;
    if (foreIdx >= 0 && foreIdx < VRIK_MAX_BONES) {
        float* t = reinterpret_cast<float*>(boneBuf + foreIdx * 48 + VRIK_TRANS_OFF);
        t[0]*=scale; t[1]*=scale; t[2]*=scale;
    }
    if (handIdx >= 0 && handIdx < VRIK_MAX_BONES) {
        float* t = reinterpret_cast<float*>(boneBuf + handIdx * 48 + VRIK_TRANS_OFF);
        t[0]*=scale; t[1]*=scale; t[2]*=scale;
    }
}

static inline bool VRIK_ArmRestTrans(uint8_t* boneBuf, uintptr_t trackBuf, int boneCount, int idx, float* out) {
    static uintptr_t s_trackBuf = 0;
    static int s_boneCount = 0;
    static bool s_valid[VRIK_MAX_BONES] = {};
    static float s_trans[VRIK_MAX_BONES][3] = {};

    if (!boneBuf || idx < 0 || idx >= VRIK_MAX_BONES || idx >= boneCount) return false;
    if (trackBuf != s_trackBuf || boneCount != s_boneCount) {
        for (int i = 0; i < VRIK_MAX_BONES; ++i) s_valid[i] = false;
        s_trackBuf = trackBuf;
        s_boneCount = boneCount;
    }
    if (!s_valid[idx]) {
        const float* t = reinterpret_cast<float*>(boneBuf + idx * 48 + VRIK_TRANS_OFF);
        s_trans[idx][0] = t[0]; s_trans[idx][1] = t[1]; s_trans[idx][2] = t[2];
        s_valid[idx] = true;
    }
    out[0] = s_trans[idx][0]; out[1] = s_trans[idx][1]; out[2] = s_trans[idx][2];
    return true;
}

static inline void VRIK_ScaleArmBonesFromRest(uint8_t* boneBuf, uintptr_t trackBuf, int boneCount,
                                              int foreIdx, int handIdx, float userArmLen) {
    if (userArmLen < 0.45f || userArmLen > 0.85f) return;
    float foreRest[3], handRest[3];
    if (!VRIK_ArmRestTrans(boneBuf, trackBuf, boneCount, foreIdx, foreRest)) return;
    if (!VRIK_ArmRestTrans(boneBuf, trackBuf, boneCount, handIdx, handRest)) return;

    float upperLen = std::sqrt(foreRest[0]*foreRest[0] + foreRest[1]*foreRest[1] + foreRest[2]*foreRest[2]);
    float foreLen  = std::sqrt(handRest[0]*handRest[0] + handRest[1]*handRest[1] + handRest[2]*handRest[2]);
    if (upperLen < 0.02f || foreLen < 0.02f) return;

    // Human shoulder->wrist split. The old uniform scale preserved the avatar's bad bicep/forearm
    // ratio; this gives each segment its own calibrated length while preserving its rest direction.
    // Even 50/50 split. The user's real bicep/forearm are near-equal (~33/~35cm); 0.515 forearm
    // read as "forearm too long" in game (amplified by the reach stretch), and the old 0.52 bicep
    // read as bicep too long. Neutral 50/50 avoids both; revisit only if a clear asymmetry shows.
    float targetUpper = userArmLen * 0.50f;
    float targetFore  = userArmLen * 0.50f;
    float upperScale = targetUpper / upperLen;
    float foreScale  = targetFore  / foreLen;
    if (upperScale < 0.65f) upperScale = 0.65f; if (upperScale > 1.45f) upperScale = 1.45f;
    if (foreScale  < 0.65f) foreScale  = 0.65f; if (foreScale  > 1.45f) foreScale  = 1.45f;

    float* foreT = reinterpret_cast<float*>(boneBuf + foreIdx * 48 + VRIK_TRANS_OFF);
    float* handT = reinterpret_cast<float*>(boneBuf + handIdx * 48 + VRIK_TRANS_OFF);
    foreT[0] = foreRest[0] * upperScale; foreT[1] = foreRest[1] * upperScale; foreT[2] = foreRest[2] * upperScale;
    handT[0] = handRest[0] * foreScale;  handT[1] = handRest[1] * foreScale;  handT[2] = handRest[2] * foreScale;
}

// Generic 2-bone limb IK (hip->knee->foot). Rotation-only writes (no stretch). The knee bends
// toward poleDir (projected perpendicular to the hip->foot axis). Used to keep the feet planted
// on their captured ground targets after the hips move under the HMD.
static inline void VRIK_SolveLeg(uint8_t* boneBuf, int upIdx, int midIdx, int endIdx,
                                 const float* target, const float* poleDir) {
    if (upIdx < 0 || midIdx < 0 || endIdx < 0
        || upIdx >= VRIK_MAX_BONES || midIdx >= VRIK_MAX_BONES || endIdx >= VRIK_MAX_BONES) return;
    const float* hip = g_fkPos[upIdx];
    float curUp[3] = { g_fkPos[midIdx][0]-hip[0], g_fkPos[midIdx][1]-hip[1], g_fkPos[midIdx][2]-hip[2] };
    float curLo[3] = { g_fkPos[endIdx][0]-g_fkPos[midIdx][0], g_fkPos[endIdx][1]-g_fkPos[midIdx][1], g_fkPos[endIdx][2]-g_fkPos[midIdx][2] };
    float upLen = VRIK_Norm3(curUp), loLen = VRIK_Norm3(curLo);
    if (upLen < 1e-4f || loLen < 1e-4f) return;

    float toTarget[3] = { target[0]-hip[0], target[1]-hip[1], target[2]-hip[2] };
    float dist = VRIK_Norm3(toTarget);
    if (dist < 1e-4f) return;
    float maxLen = upLen + loLen;
    if (dist > maxLen * 0.999f) dist = maxLen * 0.999f;

    float pole[3] = { poleDir[0], poleDir[1], poleDir[2] };
    float pd = VRIK_Dot3(pole, toTarget);
    pole[0]-=toTarget[0]*pd; pole[1]-=toTarget[1]*pd; pole[2]-=toTarget[2]*pd;
    if (VRIK_Norm3(pole) < 1e-3f) { pole[0]=0; pole[1]=0; pole[2]=1; }

    float cosHip = (upLen*upLen + dist*dist - loLen*loLen) / (2.0f*upLen*dist);
    if (cosHip < -1.0f) cosHip = -1.0f; if (cosHip > 1.0f) cosHip = 1.0f;
    float hipAng = std::acos(cosHip);
    float kneePos[3] = {
        hip[0] + toTarget[0]*(std::cos(hipAng)*upLen) + pole[0]*(std::sin(hipAng)*upLen),
        hip[1] + toTarget[1]*(std::cos(hipAng)*upLen) + pole[1]*(std::sin(hipAng)*upLen),
        hip[2] + toTarget[2]*(std::cos(hipAng)*upLen) + pole[2]*(std::sin(hipAng)*upLen),
    };
    float desUp[3] = { kneePos[0]-hip[0], kneePos[1]-hip[1], kneePos[2]-hip[2] }; VRIK_Norm3(desUp);
    float d1[4]; VRIK_QuatFromTo(curUp, desUp, d1);
    float newUp[4]; VRIK_QuatMul(d1, g_fkRot[upIdx], newUp); VRIK_QuatNorm(newUp);
    int up_p = g_VRBoneParent[upIdx]; float id[4] = {0,0,0,1};
    VRIK_WriteLocalRot(boneBuf, upIdx, (up_p>=0 && up_p<VRIK_MAX_BONES)?g_fkRot[up_p]:id, newUp);
    float loBase[3]; VRIK_QuatRotateVec(d1, curLo, loBase);
    float desLo[3] = { target[0]-kneePos[0], target[1]-kneePos[1], target[2]-kneePos[2] }; VRIK_Norm3(desLo);
    float d2[4]; VRIK_QuatFromTo(loBase, desLo, d2);
    float tmp[4]; VRIK_QuatMul(d2, d1, tmp);
    float newLo[4]; VRIK_QuatMul(tmp, g_fkRot[midIdx], newLo); VRIK_QuatNorm(newLo);
    VRIK_WriteLocalRot(boneBuf, midIdx, newUp, newLo);
}

// PHASE 2 — FULL BODY under the HMD, anchored from the HEAD ("bone head = hmd").
// The head bone is driven to the HMD position+orientation; the spine chain bends NATURALLY and
// distributed (lower spine rounds, chest leans, neck stretches) to connect the hips up to the
// head; the hips slide under the HMD; and a 2-bone leg IK keeps the feet on their captured
// ground positions. This is the CP2077 post-eval adaptation of the standard VR body solver (setBodyUnderHMD +
// handleSpine + setLegs): we can only rewrite local transforms in the bone buffer, so we move
// the hips translation, distribute the spine rotation (CCD), IK the legs, and orient the head.
//
// Falls back to head-orient-only (no body move) when the leg bones aren't resolved, so the feet
// can never float.
static inline void VRIK_PlaceBodyUnderHMD(uint8_t* boneBuf,
                                          const float* camModelPos, const float* camModelRot,
                                          int headIdx, const float* bodyFwd) {
    int hips = g_VRHipsIdx;
    if (hips < 0 || hips >= VRIK_MAX_BONES || headIdx < 0 || headIdx >= VRIK_MAX_BONES) return;
    float id[4] = { 0,0,0,1 };

    bool haveR = (g_VRRightFootIdx >= 0 && g_VRRightFootIdx < VRIK_MAX_BONES
                  && g_VRRightUpLegIdx >= 0 && g_VRRightUpLegIdx < VRIK_MAX_BONES
                  && g_VRRightLegIdx >= 0 && g_VRRightLegIdx < VRIK_MAX_BONES);
    bool haveL = (g_VRLeftFootIdx >= 0 && g_VRLeftFootIdx < VRIK_MAX_BONES
                  && g_VRLeftUpLegIdx >= 0 && g_VRLeftUpLegIdx < VRIK_MAX_BONES
                  && g_VRLeftLegIdx >= 0 && g_VRLeftLegIdx < VRIK_MAX_BONES);
    bool moveBody = haveR && haveL;   // only relocate the body if we can plant the feet

    // 1. Capture foot model positions to keep them planted.
    float footR[3] = {0,0,0}, footL[3] = {0,0,0};
    if (haveR) { footR[0]=g_fkPos[g_VRRightFootIdx][0]; footR[1]=g_fkPos[g_VRRightFootIdx][1]; footR[2]=g_fkPos[g_VRRightFootIdx][2]; }
    if (haveL) { footL[0]=g_fkPos[g_VRLeftFootIdx][0];  footL[1]=g_fkPos[g_VRLeftFootIdx][1];  footL[2]=g_fkPos[g_VRLeftFootIdx][2]; }

    // HEAD = CAMERA, rigidly (user's hard requirement): the head bone tracks the offset-corrected
    // game camera in XY too, so the whole upper body moves with the view as one block -> no "weapon
    // draw: camera back, body forward" desync and no "squat: head stays, body drops". After the
    // camera offset is baked/tuned, camModelPos.xy sits over the feet so the spine is vertical;
    // before tuning it leans (a cue to bake). Hips stay over the feet (legs vertical); the spine
    // bridges the small XY gap.
    float footCx = 0.5f*(footR[0]+footL[0]);
    float footCy = 0.5f*(footR[1]+footL[1]);
    // Real-life SQUAT: the game FPP camera height (camModelPos.z) is FIXED, so it can't tell when
    // the player physically crouches. shared[89] = the HMD's physical height rel the recenter base
    // (~0 standing, negative squatting); lower the whole body by that so the knees bend.
    // Squat height: use the NECK-PIVOT height (shared[90]), which removes the optical-centre arc
    // so looking DOWN no longer reads as a crouch (the #1 false-squat cause). Fall back to the raw
    // HMD height [89] if the producer hasn't written [90]. Deadzone g_VRSquatThreshold then ignores
    // small head bob; subtract it so the squat ramps from 0 smoothly.
    float squatDrop = 0.0f;
    if (g_pSharedHands) {
        float hy = SharedPose(90);
        if (hy == 0.0f) hy = SharedPose(89);
        float drop = -hy - g_VRSquatThreshold;
        if (drop > 0.0f) squatDrop = drop;
        if (squatDrop > 0.7f) squatDrop = 0.7f;
    }
    // Squat deadzone(2cm) + EMA(alpha=0.25) so sprint/jump head-bob doesn't twitch body+arms.
    // Small changes inside the deadband are frozen (ignore bob); larger real crouches ease in.
    // Shared via s_vrSharedSquatDrop so the ARM anchor uses the SAME smoothed squat as the body.
    {
        static float s_squatEMA = 0.0f; static bool s_squatInit = false;
        const float kSquatDead = 0.02f; const float kSquatA = 0.25f;
        if (!s_squatInit) { s_squatEMA = squatDrop; s_squatInit = true; }
        else if (std::fabs(squatDrop - s_squatEMA) > kSquatDead) { s_squatEMA += (squatDrop - s_squatEMA) * kSquatA; }
        squatDrop = s_squatEMA;
    }
    s_vrSharedSquatDrop = squatDrop;
    // Head anchor = camera + small head-above-eyes gap, minus the physical squat. The BODY is
    // placed naturally and is NOT dragged toward the camera mount. "Bake to eyes" is done the
    // OTHER way around (correct direction, per user): the RENDERED VIEW is moved onto the
    // avatar's eyes -- see the eye-view publish at the END of this function (slots [116..119],
    // applied view-only in dxgi LocateCamera). Body solve stays untouched by it (no feedback).
    float headAnchor[3] = { camModelPos[0], camModelPos[1], camModelPos[2] + g_VRHeadDrop - squatDrop };

    // IK-style standing base: weapon stance must not push the legs forward. Keep the current
    // foot spacing, but recenter the pair directly below the HMD/head in model XY.
    if (haveR && haveL) {
        float fcx = 0.5f * (footR[0] + footL[0]);
        float fcy = 0.5f * (footR[1] + footL[1]);
        float sx = headAnchor[0] - fcx;
        float sy = headAnchor[1] - fcy;
        footR[0] += sx; footR[1] += sy;
        footL[0] += sx; footL[1] += sy;
    }

    // 2. Lower/raise the hips to follow the HMD height (so a real squat bends the knees), keeping
    //    them over the feet. Use the VERTICAL torso height (head.z - hips.z), NOT the 3D chain
    //    distance -- otherwise a leaned-back animation pose makes the chain longer than the
    //    vertical drop and the hips sink, bending the knees even while standing straight.
    if (moveBody) {
        // [3-FRAME ENGINE-BOB FIX] torsoVert (head.z - hips.z) sets the hips height.
        // In AER the engine flexes the upper body ~9cm on a strict 3-frame cycle, so
        // this span jittered -> the hips (hence the whole body) bobbed "1 of 3 frames".
        // A 3-tap MEDIAN per the measured cycle kills the 1-in-3 outlier regardless of
        // whether the head, the hips, or both bob (median([lo,hi,hi])=hi every frame),
        // while a real crouch (a sustained change) passes after ~1 frame. torsoVert is
        // a near-constant anatomical span so filtering it has no downside.
        float torsoVertRaw = g_fkPos[headIdx][2] - g_fkPos[hips][2];
        static float s_tvHist[3] = {0,0,0};
        static int   s_tvN = 0;
        s_tvHist[s_tvN % 3] = torsoVertRaw;
        ++s_tvN;
        float torsoVert = torsoVertRaw;
        if (s_tvN >= 3) {
            const float a = s_tvHist[0], b = s_tvHist[1], c = s_tvHist[2];
            torsoVert = a < b ? (b < c ? b : (a < c ? c : a)) : (a < c ? a : (b < c ? c : b));
        }
        if (torsoVert < 0.2f) torsoVert = 0.2f;
        // Hips follow the camera XY too (rigid body block) so the whole body moves with the view as
        // one piece -- no "weapon draw: camera/head back, hips forward" torso desync. After baking
        // camModelPos.xy = foot centre, so the legs stay vertical; the leg IK keeps the feet planted.
        float hipsTarget[3] = { headAnchor[0], headAnchor[1], headAnchor[2] - torsoVert };
        int hp = g_VRBoneParent[hips];
        if (hp >= 0 && hp < VRIK_MAX_BONES) {
            VRIK_WriteLocalPos(boneBuf, hips, g_fkPos[hp], g_fkRot[hp], hipsTarget);
            VRIK_ComputeFK(boneBuf, VRIK_FKCount());
        }
    }

    // 3. Distributed spine bend (CCD). Chain = spine bones + neck. Each bone, base->tip, rotates
    //    a fraction toward putting the head over the feet at HMD height; repeated passes converge
    //    with a gradual curve (rounded lower spine, leaning chest, stretched neck).
    if (moveBody) {
        int chain[10]; int chainN = 0;
        for (int s = 0; s < g_VRSpineCount && chainN < 9; ++s)
            if (g_VRSpineIdx[s] >= 0 && g_VRSpineIdx[s] < VRIK_MAX_BONES) chain[chainN++] = g_VRSpineIdx[s];
        if (g_VRNeckIdx >= 0 && g_VRNeckIdx < VRIK_MAX_BONES && chainN < 10) chain[chainN++] = g_VRNeckIdx;

        for (int pass = 0; pass < 3; ++pass) {
            for (int c = 0; c < chainN; ++c) {
                int idx = chain[c];
                const float* pivot = g_fkPos[idx];
                // curDir MUST be the live FK head: this CCD is iterative (ComputeFK runs
                // after each bone), so the current direction has to reflect the actual
                // updated head each pass — feeding a fixed/median head here makes the
                // spine over-rotate and spasm. The upper-body bob is instead addressed
                // by stabilizing the ANCHOR target (headAnchor, from the stable camera)
                // and the hips (torsoVert median), not this baseline.
                float curDir[3] = { g_fkPos[headIdx][0]-pivot[0], g_fkPos[headIdx][1]-pivot[1], g_fkPos[headIdx][2]-pivot[2] };
                float desDir[3] = { headAnchor[0]-pivot[0], headAnchor[1]-pivot[1], headAnchor[2]-pivot[2] };
                if (VRIK_Norm3(curDir) < 1e-4f || VRIK_Norm3(desDir) < 1e-4f) continue;
                float d[4]; VRIK_QuatFromTo(curDir, desDir, d);
                float pd[4]; VRIK_QuatScale(d, 0.5f, pd);   // half the remaining error per bone
                float newModel[4]; VRIK_QuatMul(pd, g_fkRot[idx], newModel); VRIK_QuatNorm(newModel);
                int pp = g_VRBoneParent[idx];
                VRIK_WriteLocalRot(boneBuf, idx, (pp>=0&&pp<VRIK_MAX_BONES)?g_fkRot[pp]:id, newModel);
                VRIK_ComputeFK(boneBuf, VRIK_FKCount());
            }
        }
    }

    // 4. Leg IK: feet back to their captured ground positions, knees bending forward.
    if (haveR) VRIK_SolveLeg(boneBuf, g_VRRightUpLegIdx, g_VRRightLegIdx, g_VRRightFootIdx, footR, bodyFwd);
    if (haveL) VRIK_SolveLeg(boneBuf, g_VRLeftUpLegIdx,  g_VRLeftLegIdx,  g_VRLeftFootIdx,  footL, bodyFwd);
    if (moveBody) VRIK_ComputeFK(boneBuf, VRIK_FKCount());

    // 5. Head follows the real head: orient the head bone to the HMD.
    {
        int hp = g_VRBoneParent[headIdx];
        VRIK_WriteLocalRot(boneBuf, headIdx, (hp>=0&&hp<VRIK_MAX_BONES)?g_fkRot[hp]:id, camModelRot);
        VRIK_ComputeFK(boneBuf, VRIK_FKCount());
    }

    // 5b. VIEW-ANCHOR PUBLISH -- HEAD BONE + USER-TUNED CONSTANTS ("bake на head").
    // The eye-midpoint auto-measure is gone: view target = HEAD BONE + fixed offset,
    // model axes (X right, Y fwd, Z up). Values tuned by the user with the live Tracking
    // sliders AFTER the 131072 fixed-point fix (honest 1:1 meters): (-0.02, +0.10, +0.15).
    // The Tracking sliders should sit at ZERO now -- these constants replace them.
    // delta = (headFK + kViewOff) - (baked) camModelPos, published on the same [116..119]
    // channel dxgi's LocateCamera already applies view-only (next to xrHeadOffset+camBake).
    // No feedback: the view offset never feeds camModelPos or the body solve. EMA(0.1)
    // kills FK jitter; sanity clamp +-0.9m.
    if (g_pSharedHands && headIdx >= 0 && headIdx < VRIK_MAX_BONES) {
        const float kViewOffRight = -0.02f, kViewOffFwd = 0.10f, kViewOffUp = 0.15f;
        float tgt[3] = { g_fkPos[headIdx][0] + kViewOffRight,
                         g_fkPos[headIdx][1] + kViewOffFwd,
                         g_fkPos[headIdx][2] + kViewOffUp };
        float d[3] = { tgt[0]-camModelPos[0], tgt[1]-camModelPos[1], tgt[2]-camModelPos[2] };
        bool sane = true;
        for (int k = 0; k < 3; ++k) { if (!(d[k] > -0.9f && d[k] < 0.9f)) sane = false; }
        if (sane) {
            static float s_eyeViewEMA[3] = {0,0,0}; static bool s_evInit = false;
            if (!s_evInit) { s_eyeViewEMA[0]=d[0]; s_eyeViewEMA[1]=d[1]; s_eyeViewEMA[2]=d[2]; s_evInit = true; }
            else { for (int k = 0; k < 3; ++k) s_eyeViewEMA[k] += (d[k]-s_eyeViewEMA[k]) * 0.1f; }
            g_pSharedHands[116] = s_eyeViewEMA[0];
            g_pSharedHands[117] = s_eyeViewEMA[1];
            g_pSharedHands[118] = s_eyeViewEMA[2];
            g_pSharedHands[119] = 1.0f;
        }
    }

    g_VRIKDbgChest[0]=g_fkPos[headIdx][0]; g_VRIKDbgChest[1]=g_fkPos[headIdx][1]; g_VRIKDbgChest[2]=g_fkPos[headIdx][2];
    g_VRIKDbgChestTgt[0]=headAnchor[0]; g_VRIKDbgChestTgt[1]=headAnchor[1]; g_VRIKDbgChestTgt[2]=headAnchor[2];
}


// Swing-twist: extract the TWIST of q about axis a (unit, same frame as q's vector part):
// twist = normalize((v·a)a, w); identity when the projection degenerates (q ⟂ a, 180° swing).
static inline void VRIK_TwistAbout(const float* q, const float* a, float* outT) {
    const float d = q[0]*a[0] + q[1]*a[1] + q[2]*a[2];
    outT[0] = a[0]*d; outT[1] = a[1]*d; outT[2] = a[2]*d; outT[3] = q[3];
    float n = outT[0]*outT[0] + outT[1]*outT[1] + outT[2]*outT[2] + outT[3]*outT[3];
    if (n < 1e-10f) { outT[0]=0.0f; outT[1]=0.0f; outT[2]=0.0f; outT[3]=1.0f; return; }
    n = 1.0f / std::sqrt(n);
    outT[0]*=n; outT[1]*=n; outT[2]*=n; outT[3]*=n;
}

typedef void* (*AnimPoseFunc_t)(void* a1, void* a2, void* a3, unsigned int a4);
static AnimPoseFunc_t OriginalAnimPose = nullptr;

// Solve cache (one solve per tick + replay).
static float g_solveCacheTick = -1.0e9f;
static int   g_solveCacheN = 0;
static int   g_solveCacheIdx[96];
static float g_solveCacheVal[96][7];
static float g_solveCacheYaw = 0.0f;   // heading the cached solve was built with
static float g_solveCacheSnapCtr = -1.0f; // snap event counter [147] the cached solve consumed

extern "C" inline void* Hooked_AnimPoseApply(void* a1, void* a2, void* a3, unsigned int a4) {
    void* result = OriginalAnimPose(a1, a2, a3, a4);
    ++g_AnimPoseTotalCalls;

    // Hot-path early-out: this hook runs on EVERY skeleton's pose apply (all NPCs,
    // every frame). Do nothing unless the player is armed AND we actually have work.
    // No VirtualQuery here -- that syscall per call was the FPS killer. a2 is always
    // a valid pose-apply argument, so a single __try guards the dereferences.
    if (!(g_PlayerTrackBufA || g_PlayerTrackBufB)) return result;
    if (g_VRBind <= 0 && g_AnimPoseDebug == 0 && g_VRDiagCapture == 0) return result;

    __try {
        void* poseDesc = reinterpret_cast<void**>(a2)[7];
        if (poseDesc) {
            uint8_t*  boneBuf  = reinterpret_cast<uint8_t**>(poseDesc)[0];
            uintptr_t trackBuf = reinterpret_cast<uintptr_t*>(poseDesc)[3];
            if (boneBuf && trackBuf && (trackBuf == g_PlayerTrackBufA || trackBuf == g_PlayerTrackBufB)) {
                ++g_AnimPoseMatchCalls;
                g_AnimPoseLastBoneBuf = reinterpret_cast<uintptr_t>(boneBuf);

                // SEQLOCK: latch ONE consistent pose frame for this whole solve, so
                // every SharedPose() read below comes from the same frame -> no torn
                // quaternion -> the whole-body IK stops jittering. Refreshed once per
                // player apply (this runs on the animation thread; dxgi writes on the
                // present thread).
                RefreshHandsSnapshot();

                // Diagnostic snapshot of the ORIGINAL (pre-write) pose of bones 0..31.
                // Correct QsTransform layout: translation@+0, rotation@+16.
                if (g_VRDiagCapture) {
                    for (int b = 0; b < 32; ++b) {
                        const float* t = reinterpret_cast<float*>(boneBuf + b * 48 + VRIK_TRANS_OFF);
                        const float* q = reinterpret_cast<float*>(boneBuf + b * 48 + VRIK_ROT_OFF);
                        g_VRDiagBones[b * 7 + 0] = t[0];
                        g_VRDiagBones[b * 7 + 1] = t[1];
                        g_VRDiagBones[b * 7 + 2] = t[2];
                        g_VRDiagBones[b * 7 + 3] = q[0];
                        g_VRDiagBones[b * 7 + 4] = q[1];
                        g_VRDiagBones[b * 7 + 5] = q[2];
                        g_VRDiagBones[b * 7 + 6] = q[3];
                    }
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

                if (g_AnimPoseDebug == 1) {
                    // Calibration: shove translation (offset +0 in each 48-byte bone)
                    // of bones 0..63 by +1.5m. Whole upper body visibly distorts.
                    for (int b = 0; b < 64; ++b) {
                        float* t = reinterpret_cast<float*>(boneBuf + b * 48 + VRIK_TRANS_OFF);
                        t[0] += 1.5f; t[1] += 1.5f; t[2] += 1.5f;
                    }
                }
                else if (g_AnimPoseDebug == 2) {
                    int idx = g_AnimPoseTestBone;
                    if (idx >= 0) {
                        float m = g_AnimPoseTestMag;
                        // +0 = translation (QsTransform: translation@0, rotation@16, scale@32).
                        float* t = reinterpret_cast<float*>(boneBuf + idx * 48 + VRIK_TRANS_OFF);
                        t[0] += m; t[1] += m; t[2] += m;
                    }
                }

                // VRIK FULL-ARM IK (mode 4): model-space FK + 2-bone IK, rotation-only
                // writes (no stretch). Anchored at the head bone's model position; the
                // controller offset is taken straight from the proven gizmo world math.
                if (g_VRBind == 4 && g_pSharedHands && g_VRBoneCount > 0 && g_VRHeadBoneIdx >= 0) {
                    // Solve-per-tick accounting (post-write tamper diagnosis). Counts how
                    // many times the PLAYER pose-apply runs within one entity tick and
                    // which distinct bone buffers participate.
                    {
                        static float s_tickSeq = -1.0f;
                        static int   s_count = 0;
                        const float seqNow = g_pSharedHands[99];
                        if (seqNow != s_tickSeq) {
                            g_VRIKSolvesLastTick = s_count;
                            if (s_count > g_VRIKSolvesMaxTick) g_VRIKSolvesMaxTick = s_count;
                            s_count = 0;
                            s_tickSeq = seqNow;
                            g_VRIKLastBufA = reinterpret_cast<uintptr_t>(boneBuf);
                            g_VRIKLastBufB = 0;
                        } else if (reinterpret_cast<uintptr_t>(boneBuf) != g_VRIKLastBufA) {
                            g_VRIKLastBufB = reinterpret_cast<uintptr_t>(boneBuf);
                        }
                        ++s_count;
                    }
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
                    const float tickNow = g_pSharedHands[99];
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
                        for (int ci = 0; ci < g_solveCacheN; ++ci) {
                            const int bi = g_solveCacheIdx[ci];
                            float* t = reinterpret_cast<float*>(boneBuf + bi * 48 + VRIK_TRANS_OFF);
                            float* q = reinterpret_cast<float*>(boneBuf + bi * 48 + VRIK_ROT_OFF);
                            t[0]=g_solveCacheVal[ci][0]; t[1]=g_solveCacheVal[ci][1]; t[2]=g_solveCacheVal[ci][2];
                            q[0]=g_solveCacheVal[ci][3]; q[1]=g_solveCacheVal[ci][4]; q[2]=g_solveCacheVal[ci][5]; q[3]=g_solveCacheVal[ci][6];
                        }
                        ++g_VRIKReplayTotal;
                    } else {
                    // Latch the render view packet ONCE for this solve: both arms and
                    // the view-pos resolver consume the SAME frame.
                    VRIK_LatchViewPacket();
                    if (g_viewPktValid) g_solveCacheYaw = g_viewPkt[8];
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
                    if (!vrikInVehicle) {
                        VRIK_DampenTorsoWeaponPose(boneBuf);
                        VRIK_PinGirdleTranslations(boneBuf);
                        VRIK_ComputeFK(boneBuf, VRIK_FKCount());
                    }
                    // IK-style arm-length calibration: reset upper-arm/forearm segment lengths
                    // from cached rest local translations, then scale them to the T-pose measured
                    // user arm. Do not derive length from the current weapon/stance FK pose.
                    {
                        VRIK_ScaleArmBonesFromRest(boneBuf, trackBuf, g_VRBoneCount,
                                                   g_VRRightForeArmIdx, g_VRRightBoneIdx, g_VRUserArmLenR);
                        VRIK_ScaleArmBonesFromRest(boneBuf, trackBuf, g_VRBoneCount,
                                                   g_VRLeftForeArmIdx, g_VRLeftBoneIdx, g_VRUserArmLenL);
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
                    // [3-FRAME ENGINE-BOB FIX] Measured: in AER the engine's FPP pose
                    // flexes the upper body ~9cm on a strict 3-frame cycle (head+both
                    // shoulders bob together; root + our HMD height stay fixed). The
                    // avatar shoulders/arms anchor to this FK head, so the whole body
                    // jittered "1 of 3 frames". A 3-tap MEDIAN per axis is the exact
                    // filter for a 1-in-3 outlier: median([lo,hi,hi]) = hi every frame
                    // -> rock-stable, yet only ~1 frame lag on REAL motion (no smear).
                    static float s_hHist[3][3] = {{0,0,0},{0,0,0},{0,0,0}};
                    static int   s_hN = 0;
                    static float s_headMed[3] = {0,0,0};
                    const float* headModelPos = nullptr;
                    if (headFKraw) {
                        s_hHist[s_hN % 3][0] = headFKraw[0];
                        s_hHist[s_hN % 3][1] = headFKraw[1];
                        s_hHist[s_hN % 3][2] = headFKraw[2];
                        ++s_hN;
                        if (s_hN >= 3) {
                            auto med3 = [](float a, float b, float c) -> float {
                                return a < b ? (b < c ? b : (a < c ? c : a))
                                             : (a < c ? a : (b < c ? c : b));
                            };
                            s_headMed[0] = med3(s_hHist[0][0], s_hHist[1][0], s_hHist[2][0]);
                            s_headMed[1] = med3(s_hHist[0][1], s_hHist[1][1], s_hHist[2][1]);
                            s_headMed[2] = med3(s_hHist[0][2], s_hHist[1][2], s_hHist[2][2]);
                            headModelPos = s_headMed;
                        } else {
                            headModelPos = headFKraw;  // warm-up
                        }
                    }
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
                        bool camModelValid = VRIK_ComputeCamModel(camModelPos, camModelRot);
                        // (REVERTED, snap-double isolation: the "render-heading re-yaw" that lived
                        // here — a companion of the fppCamera bone freeze — is removed together
                        // with the ResolveViewPos pairLocal re-yaw and the dxgi snap holdback.
                        // Baseline = the weeks-tested stock heading path. Restore from git if the
                        // freeze experiment resumes.)
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
                        if (camModelValid) {
                            VRIK_BodyAxesFromCamYaw(camModelRot, bodyRight, bodyUp, bodyFwd);
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
                            VRIK_PlaceBodyUnderHMD(boneBuf, camModelPos, camModelRot, hIdx, fwdSigned);
                            VRIK_BodyAxesFromCamYaw(camModelRot, bodyRight, bodyUp, bodyFwd);

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
                        if (camModelValid) {
                            const float kNeckBehind = 0.0f;
                            stableAnchorBuf[0] = camModelPos[0] - bodyFwd[0]*kNeckBehind;
                            stableAnchorBuf[1] = camModelPos[1] - bodyFwd[1]*kNeckBehind;
                            stableAnchorBuf[2] = camModelPos[2] + g_VRHeadDrop - s_vrSharedSquatDrop;
                            stableAnchor = stableAnchorBuf;
                        }

                        // Right arm.
                        if (SharedPose(8) > 0.0f && g_VRRightUpperArmIdx >= 0) {
                            const float vrPos[3]  = { SharedPose(9),  SharedPose(10), SharedPose(11) };
                            const float vrQuat[4] = { SharedPose(12), SharedPose(13), SharedPose(14), SharedPose(15) };
                            float target[3], handRot[4];
                            const float wristR[4]       = { g_VRWristR_I, g_VRWristR_J, g_VRWristR_K, g_VRWristR_R };
                            const float offR[3]         = { g_VROffRX, g_VROffRY, g_VROffRZ };
                            const float shoulderBodyR[3]= { g_VRShoulderRX, g_VRShoulderRY, g_VRShoulderRZ };
                            float armLenR = restArmLen(g_VRRightUpperArmIdx, g_VRRightForeArmIdx, g_VRRightBoneIdx);
                            float shoulderModelR[3];
                            if (camModelValid && headModelPos) {
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
                                    float vq[4] = { SharedPose(104), SharedPose(105), SharedPose(106), SharedPose(107) };
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
                                    const float hs = std::sin(vyaw * 0.5f);
                                    const float hc = std::cos(vyaw * 0.5f);
                                    float ec[4] = { 0.0f, 0.0f, -hs, hc };
                                    float rvM[4]; VRIK_QuatMul(ec, vq, rvM); VRIK_QuatNorm(rvM);
                                    float vpW[3] = { vpView[0] - g_VREntityPosX,
                                                     vpView[1] - g_VREntityPosY,
                                                     vpView[2] - g_VREntityPosZ };
                                    float vpM[3]; VRIK_QuatRotateVec(ec, vpW, vpM);
                                    float mp[3] = { vrPos[0]*g_VRScaleR, -vrPos[2]*g_VRScaleR, vrPos[1]*g_VRScaleR };
                                    float rp[3]; VRIK_QuatRotateVec(rvM, mp, rp);
                                    target[0] = vpM[0] + rp[0] + offR[0];
                                    target[1] = vpM[1] + rp[1] + offR[1];
                                    target[2] = vpM[2] + rp[2] + offR[2];
                                    float lq[4] = { vrQuat[0], -vrQuat[2], vrQuat[1], vrQuat[3] };
                                    float hm[4]; VRIK_QuatMul(rvM, lq, hm);
                                    VRIK_QuatMul(hm, wristR, handRot); VRIK_QuatNorm(handRot);
                                } else {
                                    // Fallback: base-frame composition (head-turn stable, no view pose).
                                    float hq[4] = { hmdRel[0], hmdRel[1], hmdRel[2], hmdRel[3] };
                                    VRIK_QuatNorm(hq);
                                    float cbx[3]; VRIK_QuatRotateVec(hq, vrPos, cbx);
                                    cbx[0] += hmdPosBase[0]; cbx[1] += hmdPosBase[1]; cbx[2] += hmdPosBase[2];
                                    float mp[3] = { cbx[0]*g_VRScaleR, -cbx[2]*g_VRScaleR, cbx[1]*g_VRScaleR };
                                    float rp[3]; VRIK_QuatRotateVec(camModelRot, mp, rp);
                                    target[0] = handAnchor[0] + rp[0] + offR[0];
                                    target[1] = handAnchor[1] + rp[1] + offR[1];
                                    target[2] = handAnchor[2] + rp[2] + offR[2];
                                    float cq[4]; VRIK_QuatMul(hq, vrQuat, cq); VRIK_QuatNorm(cq);
                                    float lq[4] = { cq[0], -cq[2], cq[1], cq[3] };
                                    float hm[4]; VRIK_QuatMul(camModelRot, lq, hm);
                                    VRIK_QuatMul(hm, wristR, handRot); VRIK_QuatNorm(handRot);
                                }
                            }
                            // Capture the SOLVED wrist position (= controller in model space)
                            // for the holster-distance block below.
                            rhWristModel[0] = target[0];
                            rhWristModel[1] = target[1];
                            rhWristModel[2] = target[2];
                            rhWristValid = true;
                            VRIK_SolveArm(boneBuf, g_VRRightUpperArmIdx, g_VRRightForeArmIdx,
                                          g_VRRightBoneIdx, target, handRot,
                                          bodyRight, bodyUp, bodyFwd,
                                          g_VRElbowPoleR * 0.01745329252f, g_VRElbowSwingR,
                                          /*isLeft*/false, /*storeDbg*/true);
                        }
                        // Left arm.
                        if (SharedPose(0) > 0.0f && g_VRLeftUpperArmIdx >= 0) {
                            const float vrPos[3]  = { SharedPose(1), SharedPose(2), SharedPose(3) };
                            const float vrQuat[4] = { SharedPose(4), SharedPose(5), SharedPose(6), SharedPose(7) };
                            float target[3], handRot[4];
                            const float wristL[4]       = { g_VRWristL_I, g_VRWristL_J, g_VRWristL_K, g_VRWristL_R };
                            const float offL[3]         = { g_VROffLX, g_VROffLY, g_VROffLZ };
                            const float shoulderBodyL[3]= { g_VRShoulderLX, g_VRShoulderLY, g_VRShoulderLZ };
                            float armLenL = restArmLen(g_VRLeftUpperArmIdx, g_VRLeftForeArmIdx, g_VRLeftBoneIdx);
                            float shoulderModelL[3];
                            if (camModelValid && headModelPos) {
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
                                    float vq[4] = { SharedPose(104), SharedPose(105), SharedPose(106), SharedPose(107) };
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
                                    const float hs = std::sin(vyaw * 0.5f);
                                    const float hc = std::cos(vyaw * 0.5f);
                                    float ec[4] = { 0.0f, 0.0f, -hs, hc };
                                    float rvM[4]; VRIK_QuatMul(ec, vq, rvM); VRIK_QuatNorm(rvM);
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
                                } else {
                                    float hq[4] = { hmdRel[0], hmdRel[1], hmdRel[2], hmdRel[3] };
                                    VRIK_QuatNorm(hq);
                                    float cbx[3]; VRIK_QuatRotateVec(hq, vrPos, cbx);
                                    cbx[0] += hmdPosBase[0]; cbx[1] += hmdPosBase[1]; cbx[2] += hmdPosBase[2];
                                    float mp[3] = { cbx[0]*g_VRScaleL, -cbx[2]*g_VRScaleL, cbx[1]*g_VRScaleL };
                                    float rp[3]; VRIK_QuatRotateVec(camModelRot, mp, rp);
                                    target[0] = handAnchor[0] + rp[0] + offL[0];
                                    target[1] = handAnchor[1] + rp[1] + offL[1];
                                    target[2] = handAnchor[2] + rp[2] + offL[2];
                                    float cq[4]; VRIK_QuatMul(hq, vrQuat, cq); VRIK_QuatNorm(cq);
                                    float lq[4] = { cq[0], -cq[2], cq[1], cq[3] };
                                    float hm[4]; VRIK_QuatMul(camModelRot, lq, hm);
                                    VRIK_QuatMul(hm, wristL, handRot); VRIK_QuatNorm(handRot);
                                }
                            }
                            VRIK_SolveArm(boneBuf, g_VRLeftUpperArmIdx, g_VRLeftForeArmIdx,
                                          g_VRLeftBoneIdx, target, handRot,
                                          bodyRight, bodyUp, bodyFwd,
                                          g_VRElbowPoleL * 0.01745329252f, g_VRElbowSwingL,
                                          /*isLeft*/true, /*storeDbg*/true);
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
                        cachePush(g_VRRightUpperArmIdx >= 0 && g_VRRightUpperArmIdx < VRIK_MAX_BONES
                                  ? g_VRBoneParent[g_VRRightUpperArmIdx] : -1);
                        cachePush(g_VRLeftUpperArmIdx >= 0 && g_VRLeftUpperArmIdx < VRIK_MAX_BONES
                                  ? g_VRBoneParent[g_VRLeftUpperArmIdx] : -1);
                        cachePush(g_VRRightUpperArmIdx); cachePush(g_VRRightForeArmIdx); cachePush(g_VRRightBoneIdx);
                        cachePush(g_VRLeftUpperArmIdx);  cachePush(g_VRLeftForeArmIdx);  cachePush(g_VRLeftBoneIdx);
                        for (int k = 0; k < 3; ++k) { cachePush(g_VRForeTwistR[k]); cachePush(g_VRForeTwistL[k]); }
                        g_solveCacheTick = tickNow;
                    }
                    }
                }
                // Legacy direct-write binding (modes 1..3): single-bone hand write.
                else if (g_VRBind > 0 && g_pSharedHands) {
                    // Resolve the head bone pose once (shared by both hands).
                    bool  headOk = false;
                    float headPos[3]  = { 0.0f, 0.0f, 0.0f };
                    float headQuat[4] = { 0.0f, 0.0f, 0.0f, 1.0f };
                    int   hIdx = g_VRHeadBoneIdx;
                    if (g_VRUseHeadRelative && hIdx >= 0) {
                        const float* hp = reinterpret_cast<float*>(boneBuf + hIdx * 48 + VRIK_TRANS_OFF);
                        const float* hq = reinterpret_cast<float*>(boneBuf + hIdx * 48 + VRIK_ROT_OFF);
                        headQuat[0] = hq[0]; headQuat[1] = hq[1]; headQuat[2] = hq[2]; headQuat[3] = hq[3];
                        headPos[0]  = hp[0]; headPos[1]  = hp[1]; headPos[2]  = hp[2];
                        headOk = true;
                    }

                    // Right Hand (VR slot 8 = valid, 9..11 = pos, 12..15 = quat).
                    if (SharedPose(8) > 0.0f) {
                        const float vrPos[3]  = { SharedPose(9),  SharedPose(10), SharedPose(11) };
                        const float vrQuat[4] = { SharedPose(12), SharedPose(13), SharedPose(14), SharedPose(15) };
                        VRIK_WriteHand(boneBuf, g_VRRightBoneIdx, headPos, headQuat, headOk,
                                       vrPos, vrQuat, (g_VRBind == 2 || g_VRBind == 3));
                    }

                    // Left Hand (VR slot 0 = valid, 1..3 = pos, 4..7 = quat).
                    if ((g_VRBind == 3) && SharedPose(0) > 0.0f) {
                        const float vrPos[3]  = { SharedPose(1), SharedPose(2), SharedPose(3) };
                        const float vrQuat[4] = { SharedPose(4), SharedPose(5), SharedPose(6), SharedPose(7) };
                        VRIK_WriteHand(boneBuf, g_VRLeftBoneIdx, headPos, headQuat, headOk,
                                       vrPos, vrQuat, true);
                    }
                }
            }
        }
    } __except (EXCEPTION_EXECUTE_HANDLER) {}

    return result;
}

inline bool InstallAnimPoseHook() {
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

