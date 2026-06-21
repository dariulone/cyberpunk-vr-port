#pragma once
#include <windows.h>
#include <cstdint>
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
extern void* g_PlayerAnimComponent;
extern volatile uint64_t g_hookTotalCalls;
extern volatile uint64_t g_hookMatchCalls;
extern volatile uint64_t g_hookBoneWrites;
extern volatile int      g_hookCapture;
extern volatile uint64_t g_hookSkeletalCalls;
extern volatile uint32_t g_capturedRcx[32];
extern volatile uint64_t g_capturedFull[32];
extern volatile int      g_capturedCount;

typedef void* (*ComponentFunc_t)(void* rcx, void* rdx, void* r8, void* r9);
static ComponentFunc_t OriginalFunc21 = nullptr;

// Page-readable guard: returns true only if [p, p+n) is committed and readable.
// Needed because the capture path dereferences arbitrary component pointers; __try
// alone is unreliable here, so we pre-validate the page protection.
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

extern "C" void* Hooked_ComponentFunc21(void* rcx, void* rdx, void* r8, void* r9) {
    // 1. Call original function first (let the AnimGraph calculate the pose)
    void* result = OriginalFunc21(rcx, rdx, r8, r9);

    ++g_hookTotalCalls;

    // Capture mode: find which rcx pointers have a skeleton flowing through this function.
    // Every deref is pre-validated with VirtualQuery (page must be committed+readable)
    // because this runs on EVERY component, including non-animated ones where +0x168 is
    // not a pointer. __try is kept only as a last-resort backstop.
    if (g_hookCapture && VRIK_IsReadable(rcx, 0x170)) {
        __try {
            void* mi = *reinterpret_cast<void**>(reinterpret_cast<uint8_t*>(rcx) + 0x168);
            if (VRIK_IsReadable(mi, 0xE8)) {
                void* ba = *reinterpret_cast<void**>(reinterpret_cast<uint8_t*>(mi) + 0xE0);
                if (VRIK_IsReadable(ba, 0x20)) {
                    ++g_hookSkeletalCalls;
                    uint32_t low = static_cast<uint32_t>(reinterpret_cast<uintptr_t>(rcx) & 0xFFFFFFFF);
                    int n = g_capturedCount;
                    bool seen = false;
                    for (int i = 0; i < n && i < 32; ++i) {
                        if (g_capturedRcx[i] == low) { seen = true; break; }
                    }
                    if (!seen && n < 32) {
                        g_capturedRcx[n]  = low;
                        g_capturedFull[n] = reinterpret_cast<uintptr_t>(rcx);
                        g_capturedCount   = n + 1;
                    }
                }
            }
        } __except(EXCEPTION_EXECUTE_HANDLER) {}
    }

    // 2. Filter for Player's AnimatedComponent
    if (rcx && g_PlayerAnimComponent && rcx == g_PlayerAnimComponent) {
        ++g_hookMatchCalls;
        __try {
            // Read ModelInstance
            void** modelInstancePtr = reinterpret_cast<void**>(reinterpret_cast<uint8_t*>(rcx) + 0x168);
            if (modelInstancePtr && *modelInstancePtr) {
                
                // Read Bone Array at +0xE0
                void** boneArrayPtr = reinterpret_cast<void**>(reinterpret_cast<uint8_t*>(*modelInstancePtr) + 0xE0);
                if (boneArrayPtr && *boneArrayPtr) {
                    uint8_t* bones = reinterpret_cast<uint8_t*>(*boneArrayPtr);
                    ++g_hookBoneWrites;

                    if (g_CalibrationBoneIndex == -2) {
                        // Test: shove bone 0 (root) +2.0m on every axis - large + multi-axis so
                        // it is visible even in first person, on whichever component is correct.
                        reinterpret_cast<float*>(bones + 0)[0] += 2.0f;
                        reinterpret_cast<float*>(bones + 4)[0] += 2.0f;
                        reinterpret_cast<float*>(bones + 8)[0] += 2.0f;
                    } 
                    else if (g_CalibrationBoneIndex >= 0 && g_CalibrationBoneIndex < 150) {
                        // 32 bytes per QsTransform. Offset 8 is Z position (local).
                        float* z = reinterpret_cast<float*>(bones + (g_CalibrationBoneIndex * 32) + 8);
                        *z += 0.5f; // Stretch!
                        
                        float* x = reinterpret_cast<float*>(bones + (g_CalibrationBoneIndex * 32) + 0);
                        *x += 0.5f;
                    }
                }
            }
        } __except(EXCEPTION_EXECUTE_HANDLER) {
            // Safe read failed
        }
    }
    
    return result;
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
// [SIDE DIAG] handX (final), camV, frozen, bodyRightX — catch the 1-frame lateral
// arm jump. 4 floats/sample.
inline float g_sd[400 * 4] = {};
inline int   g_sdN = 0;
inline bool  g_sdWritten = false;
inline float g_sideTargetX = 0.0f;
inline float g_sideShoulderX = 0.0f;
inline void FlushSideDiag() {
    if (g_sdN < 400 || g_sdWritten) return;
    g_sdWritten = true;
    std::ofstream jf(VRDiagPath("vrik_side.txt"), std::ios::trunc);
    if (!jf.is_open()) return;
    jf << std::fixed << std::setprecision(6);
    for (int s = 0; s < 400; ++s)
        jf << "ctrlX=" << g_sd[s*4+0] << " targetX=" << g_sd[s*4+1]
           << " shoulderX=" << g_sd[s*4+2] << " handX=" << g_sd[s*4+3] << "\n";
}

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
    for (int tries = 0; tries < 8; ++tries) {
        const uint32_t s0 = *seqSlot;
        if (s0 & 1u) continue;                 // write in progress -> retry
        std::atomic_thread_fence(std::memory_order_acquire);
        float tmp[94];
        for (int i = 0; i < 94; ++i) tmp[i] = g_pSharedHands[i];
        std::atomic_thread_fence(std::memory_order_acquire);
        const uint32_t s1 = *seqSlot;
        if (s0 == s1) {                        // consistent (no write straddled the copy)
            if (!g_handsStableValid || s1 != g_handsStableSeq) {
                for (int i = 0; i < 94; ++i) g_handsStable[i] = tmp[i];
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
// T-pose measured real arm length per hand (metres). 0 = unset -> arm-bone scaling disabled.
extern volatile float     g_VRUserArmLenR, g_VRUserArmLenL;
// Phase 2 body-under-HMD: place the chest (top of the spine) under the HMD so the upper body
// follows the headset instead of the game's animated pose. g_VRBodyUnderHMD gates it.
extern volatile int       g_VRBodyUnderHMD;   // 1 = reposition upper body under the HMD
extern volatile float     g_VRChestDrop;      // eyes -> chest, down along bodyUp (m)
extern volatile float     g_VRChestFwd;       // eyes -> chest, along bodyFwd (m, -=back)
extern volatile float     g_VRHeadDrop;       // HMD -> head bone, down along bodyUp (m)
extern volatile float     g_VRSquatThreshold; // HMD drop deadzone before the body squats (m)
extern volatile float     g_VRCamSmooth;      // body-anchor camera low-pass (per-frame lerp; 1=off)
// Diagnostics for the body placement (model space), surfaced via LogVRDiag.
extern volatile float     g_VRIKDbgChest[3];
extern volatile float     g_VRIKDbgChestTgt[3];

// Full-arm IK (g_VRBind == 4): hierarchy + chain indices resolved in VRIK_DoArmPlayer.
extern int16_t            g_VRBoneParent[256];     // metaRig parent index per bone
extern volatile int       g_VRBoneCount;           // bone count (0 = not resolved)
extern volatile int       g_VRRightUpperArmIdx;    // RightArm  (shoulder joint / upper-arm start)
extern volatile int       g_VRRightForeArmIdx;     // RightForeArm (elbow)
extern volatile int       g_VRLeftUpperArmIdx;     // LeftArm
extern volatile int       g_VRLeftForeArmIdx;      // LeftForeArm
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

    // Hand -> shoulder (F4VR convention; xDir is along this).
    float handToShoulder[3] = { sh[0]-targetModel[0], sh[1]-targetModel[1], sh[2]-targetModel[2] };
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
    float dPerp[3], bPerp[3], oPerp[3];
    projectPerp(downRef, xDir, dPerp);
    projectPerp(backRef, xDir, bPerp);
    projectPerp(outRef,  xDir, oPerp);

    // Bend = down + back + a constant OUTWARD lean (hvr-ik). The down+back blend handles vertical
    // vs horizontal arms (dPerp shrinks when the arm is vertical, bPerp when horizontal). The
    // outward term (oPerp = own-side outward, projected perp to the arm axis) makes the elbow lean
    // OUT/down even for a relaxed arm, instead of collapsing inward across the body ("elbow bent
    // left"). hvr-ik defaults the bend down/back and swings outward; we keep a small constant
    // outward base + the stronger cross-body swing below.
    const float kElbowOutBase = 0.55f; // constant outward lean (tune)
    float yDir[3] = { dPerp[0] + bPerp[0] + oPerp[0]*kElbowOutBase,
                      dPerp[1] + bPerp[1] + oPerp[1]*kElbowOutBase,
                      dPerp[2] + bPerp[2] + oPerp[2]*kElbowOutBase };

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
        float f = swingGain * crossGate * bendFactor * poleWeight;
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

    // Upper arm: rotate current shoulder->elbow direction to the new one.
    float desUp[3] = { newElbow[0]-sh[0], newElbow[1]-sh[1], newElbow[2]-sh[2] };
    VRIK_Norm3(desUp);
    float delta1[4]; VRIK_QuatFromTo(curUp, desUp, delta1);
    float newUpModel[4]; VRIK_QuatMul(delta1, g_fkRot[upperIdx], newUpModel); VRIK_QuatNorm(newUpModel);

    int upParent = g_VRBoneParent[upperIdx];
    const float* upParentModel = (upParent >= 0 && upParent < VRIK_MAX_BONES) ? g_fkRot[upParent] : nullptr;
    float identity[4] = { 0,0,0,1 };
    VRIK_WriteLocalRot(boneBuf, upperIdx, upParentModel ? upParentModel : identity, newUpModel);

    // Forearm: rotate to point at the (real) target.
    float foreBase[3]; VRIK_QuatRotateVec(delta1, curFore, foreBase);
    float desFore[3] = { targetModel[0]-newElbow[0], targetModel[1]-newElbow[1], targetModel[2]-newElbow[2] };
    VRIK_Norm3(desFore);
    float delta2[4]; VRIK_QuatFromTo(foreBase, desFore, delta2);
    float tmp[4]; VRIK_QuatMul(delta2, delta1, tmp);
    float newForeModel[4]; VRIK_QuatMul(tmp, g_fkRot[foreIdx], newForeModel); VRIK_QuatNorm(newForeModel);
    VRIK_WriteLocalRot(boneBuf, foreIdx, newUpModel, newForeModel);

    // Hand orientation written local to the new forearm.
    VRIK_WriteLocalRot(boneBuf, handIdx, newForeModel, handModelRot);

    if (storeDbg) {
        volatile float* L = isLeft ? g_VRIKDbgLocalL : g_VRIKDbgLocal;
        L[0]=lateral; L[1]=crossAmount; L[2]=reach01; L[3]=0.0f;
        volatile float* T = isLeft ? g_VRIKDbgTargetL   : g_VRIKDbgTarget;
        volatile float* S = isLeft ? g_VRIKDbgShoulderL : g_VRIKDbgShoulder;
        volatile float* E = isLeft ? g_VRIKDbgElbowL    : g_VRIKDbgElbow;
        volatile float* N = isLeft ? g_VRIKDbgLensL     : g_VRIKDbgLens;
        T[0]=targetModel[0]; T[1]=targetModel[1]; T[2]=targetModel[2];
        S[0]=sh[0]; S[1]=sh[1]; S[2]=sh[2];
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
    float d[3] = { g_VRCamPosX - g_VREntityPosX,
                   g_VRCamPosY - g_VREntityPosY,
                   g_VRCamPosZ - g_VREntityPosZ };
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
    float targetUpper = userArmLen * 0.52f;
    float targetFore  = userArmLen * 0.48f;
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
    // Head = camera in XY (camModelPos.xy); height = camera + the small head-above-eyes gap, minus
    // the physical squat. The body hangs from this so head = camera rigidly.
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
            VRIK_ComputeFK(boneBuf, g_VRBoneCount);
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
                VRIK_ComputeFK(boneBuf, g_VRBoneCount);
            }
        }
    }

    // 4. Leg IK: feet back to their captured ground positions, knees bending forward.
    if (haveR) VRIK_SolveLeg(boneBuf, g_VRRightUpLegIdx, g_VRRightLegIdx, g_VRRightFootIdx, footR, bodyFwd);
    if (haveL) VRIK_SolveLeg(boneBuf, g_VRLeftUpLegIdx,  g_VRLeftLegIdx,  g_VRLeftFootIdx,  footL, bodyFwd);
    if (moveBody) VRIK_ComputeFK(boneBuf, g_VRBoneCount);

    // 5. Head follows the real head: orient the head bone to the HMD.
    {
        int hp = g_VRBoneParent[headIdx];
        VRIK_WriteLocalRot(boneBuf, headIdx, (hp>=0&&hp<VRIK_MAX_BONES)?g_fkRot[hp]:id, camModelRot);
        VRIK_ComputeFK(boneBuf, g_VRBoneCount);
    }

    g_VRIKDbgChest[0]=g_fkPos[headIdx][0]; g_VRIKDbgChest[1]=g_fkPos[headIdx][1]; g_VRIKDbgChest[2]=g_fkPos[headIdx][2];
    g_VRIKDbgChestTgt[0]=headAnchor[0]; g_VRIKDbgChestTgt[1]=headAnchor[1]; g_VRIKDbgChestTgt[2]=headAnchor[2];
}


typedef void* (*AnimPoseFunc_t)(void* a1, void* a2, void* a3, unsigned int a4);
static AnimPoseFunc_t OriginalAnimPose = nullptr;

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
                    // Frozen-frame rejection was removed. It could misclassify normal
                    // downward head motion as stale data and replace a live pose with an
                    // older one. Solve the live pose directly.
                    VRIK_ComputeFK(boneBuf, g_VRBoneCount);
                    VRIK_DampenTorsoWeaponPose(boneBuf);
                    VRIK_ComputeFK(boneBuf, g_VRBoneCount);
                    // IK-style arm-length calibration: reset upper-arm/forearm segment lengths
                    // from cached rest local translations, then scale them to the T-pose measured
                    // user arm. Do not derive length from the current weapon/stance FK pose.
                    {
                        VRIK_ScaleArmBonesFromRest(boneBuf, trackBuf, g_VRBoneCount,
                                                   g_VRRightForeArmIdx, g_VRRightBoneIdx, g_VRUserArmLenR);
                        VRIK_ScaleArmBonesFromRest(boneBuf, trackBuf, g_VRBoneCount,
                                                   g_VRLeftForeArmIdx, g_VRLeftBoneIdx, g_VRUserArmLenL);
                        VRIK_ComputeFK(boneBuf, g_VRBoneCount);
                    }
                    // HAND-TO-HOLSTER distances. Outfit props (katana, pistol holster) are SKINNED
                    // meshes — no single CPU-readable world position. We approximate the prop handle
                    // as a point offset OUTWARD from the hip bone in the body's lateral direction.
                    // shared[20] = right hip prop, shared[21] = left hip prop.
                    //
                    // Plus an EXTRA signal for "reach behind back" -> ranged weapon (works WITHOUT
                    // visual holsters): shared[22] = signed depth of right wrist BEHIND the chest
                    // center along the body's backward axis. Positive = wrist is behind body.
                    if (g_VRRightBoneIdx >= 0 && g_VRRightBoneIdx < VRIK_MAX_BONES) {
                        const float* rh = g_fkPos[g_VRRightBoneIdx];

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
                        // bodyFwd = cross(bodyUp, bodyRight). Sign convention may differ per rig;
                        // shared[22] is computed as -dot(wristOffset, bodyFwd) below so positive
                        // always means "behind body" regardless of which way bodyFwd points.
                        float bf[3];
                        bf[0] = bu[1]*br[2] - bu[2]*br[1];
                        bf[1] = bu[2]*br[0] - bu[0]*br[2];
                        bf[2] = bu[0]*br[1] - bu[1]*br[0];
                        { float n = sqrtf(bf[0]*bf[0]+bf[1]*bf[1]+bf[2]*bf[2]);
                          if (n > 1e-4f) { bf[0]/=n; bf[1]/=n; bf[2]/=n; } }

                        const float kRightOff = 0.18f; // pistol pulls outward — handle sits right of hip
                        const float kLeftOff  = 0.05f; // katana handle stays close to body center, so anchor near bone
                                                       // (cross-body reach by right hand is hard to reach far-left)
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

                        // "OVER-RIGHT-SHOULDER" reach for a back-slung ranged weapon (sniper/rifle).
                        // Anchor is a point slightly ABOVE and BEHIND the right shoulder where the
                        // butt of a slung rifle naturally sits when reaching over the shoulder. The
                        // user reaches up and back with the right hand, wrist lands near this anchor
                        // -> distance small -> CET fires the ranged equip.
                        // Two anchor candidates are published so CET picks the smaller one — this
                        // sidesteps the bodyFwd sign convention (it might point forward OR backward
                        // depending on the puppet rig). The correct anchor will be the one the wrist
                        // is actually close to when reaching back, the wrong one will always be far
                        // (it ends up in front of the body).
                        if (haveShoulders) {
                            const float* rs = g_fkPos[g_VRRightUpperArmIdx];
                            const float kUp = 0.05f;   // above shoulder
                            const float kBack = 0.10f; // back from shoulder
                            // Candidate A: shoulder + up*kUp + (-bodyFwd)*kBack
                            float ax = rs[0] + bu[0]*kUp - bf[0]*kBack;
                            float ay = rs[1] + bu[1]*kUp - bf[1]*kBack;
                            float az = rs[2] + bu[2]*kUp - bf[2]*kBack;
                            float dA = d3(rh[0],rh[1],rh[2], ax,ay,az);
                            // Candidate B: same but with bodyFwd flipped.
                            float bx = rs[0] + bu[0]*kUp + bf[0]*kBack;
                            float by = rs[1] + bu[1]*kUp + bf[1]*kBack;
                            float bz = rs[2] + bu[2]*kUp + bf[2]*kBack;
                            float dB = d3(rh[0],rh[1],rh[2], bx,by,bz);
                            g_pSharedHands[22] = (dA < dB) ? dA : dB;
                        } else g_pSharedHands[22] = -1.0f;
                    }
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
                        float rawCamModelPos[3] = { camModelPos[0], camModelPos[1], camModelPos[2] };

                        // Apply the SAME baked camera->head offset (shared [91..93], game-local
                        // right/fwd/up) that dxgi shifts the VIEW by, so the avatar head sits exactly
                        // where the offset-tuned view sits. head = camera, regulated by the offset.
                        if (camModelValid && g_pSharedHands) {
                            camModelPos[0] += SharedPose(91);
                            camModelPos[1] += SharedPose(92);
                            camModelPos[2] += SharedPose(93);
                        }
                        if (camModelValid) {
                            VRIK_BodyAxesFromCamYaw(camModelRot, bodyRight, bodyUp, bodyFwd);
                        }
                        // Low-pass the camera used for the BODY anchor so weapon-draw / recoil camera
                        // jerks don't lurch the body & shoulder. The HANDS use the controllers (sharp)
                        // added on top, so they stay responsive. Snap on big jumps (teleport / area
                        // load) so the body doesn't slide in.
                        if (camModelValid) {
                            static float s_smCam[3] = {0,0,0}; static bool s_smInit = false;
                            float a = g_VRCamSmooth; if (a < 0.05f) a = 0.05f; if (a > 1.0f) a = 1.0f;
                            float dx = camModelPos[0]-s_smCam[0], dy = camModelPos[1]-s_smCam[1], dz = camModelPos[2]-s_smCam[2];
                            if (!s_smInit || (dx*dx+dy*dy+dz*dz) > 0.25f) { // >0.5 m -> snap
                                s_smCam[0]=camModelPos[0]; s_smCam[1]=camModelPos[1]; s_smCam[2]=camModelPos[2]; s_smInit=true;
                            } else {
                                s_smCam[0]+=dx*a; s_smCam[1]+=dy*a; s_smCam[2]+=dz*a;
                            }
                            camModelPos[0]=s_smCam[0]; camModelPos[1]=s_smCam[1]; camModelPos[2]=s_smCam[2];
                        }
                        if (camModelValid && g_VRBodyUnderHMD) {
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
                            float k = adj * armLen * 0.08f / l * downFactor;
                            outSh[0] = sh[0] + st[0] * k;
                            outSh[1] = sh[1] + st[1] * k;
                            outSh[2] = sh[2] + st[2] * k;
                        };
                        auto calibratedShoulderModel = [&](const float* shoulderBody, int upperIdx, float* outSh) {
                            const float* current = g_fkPos[upperIdx];
                            outSh[0] = headModelPos[0] + bodyRight[0]*shoulderBody[0] + bodyUp[0]*shoulderBody[1] - bodyFwd[0]*shoulderBody[2];
                            outSh[1] = headModelPos[1] + bodyRight[1]*shoulderBody[0] + bodyUp[1]*shoulderBody[1] - bodyFwd[1]*shoulderBody[2];
                            outSh[2] = headModelPos[2] + bodyRight[2]*shoulderBody[0] + bodyUp[2]*shoulderBody[1] - bodyFwd[2]*shoulderBody[2];
                            // Bias away from weapon-specific upper-body poses, but keep close enough
                            // to the live rig to avoid visible shoulder detaches.
                            const float w = 0.80f;
                            outSh[0] = current[0]*(1.0f-w) + outSh[0]*w;
                            outSh[1] = current[1]*(1.0f-w) + outSh[1]*w;
                            outSh[2] = current[2]*(1.0f-w) + outSh[2]*w;
                        };
                        auto applyShoulderAnchor = [&](int upperIdx, const float* sh) {
                            int parent = (upperIdx >= 0 && upperIdx < VRIK_MAX_BONES) ? g_VRBoneParent[upperIdx] : -1;
                            if (parent >= 0 && parent < VRIK_MAX_BONES) {
                                VRIK_WriteLocalPos(boneBuf, upperIdx, g_fkPos[parent], g_fkRot[parent], sh);
                            }
                            g_fkPos[upperIdx][0] = sh[0];
                            g_fkPos[upperIdx][1] = sh[1];
                            g_fkPos[upperIdx][2] = sh[2];
                            VRIK_ComputeFK(boneBuf, g_VRBoneCount);
                        };

                        // STABLE shoulder anchor. Anchored at a BODY point (the head bone -> the
                        // shoulder hangs off the body, NOT off the forward camera, so the clavicle is
                        // not stretched forward) and ORIENTED by the FK body frame (bodyRight/Up/Fwd),
                        // NOT by the HMD -> the shoulder does NOT move when the head looks up/down. The
                        // clavicle (parent) is anchored at 45% of the half-width and the shoulder JOINT
                        // at 100%, so the clavicle keeps a natural length. Returns the joint position.
                        auto anchorStableShoulder = [&](int upperIdx, const float* anchor, bool isLeft, float* outJoint) {
                            // Half-width from CALIBRATION (g_VRShoulderRX = the measured shoulder X
                            // half-width): a narrow user -> a narrower torso. Clamped to a sane range.
                            float half = std::fabs(g_VRShoulderRX) * 0.82f;
                            if (half < 0.085f) half = 0.085f;
                            if (half > 0.18f) half = 0.18f;
                            const float drop = 0.22f;  // joint below the anchor (head -> shoulder)
                            float side = isLeft ? -1.0f : 1.0f;
                            auto at = [&](float f, float* o) {
                                o[0] = anchor[0] + bodyRight[0]*(side*half*f) - bodyUp[0]*drop;
                                o[1] = anchor[1] + bodyRight[1]*(side*half*f) - bodyUp[1]*drop;
                                o[2] = anchor[2] + bodyRight[2]*(side*half*f) - bodyUp[2]*drop;
                            };
                            int clavi = (upperIdx >= 0 && upperIdx < VRIK_MAX_BONES) ? g_VRBoneParent[upperIdx] : -1;
                            if (clavi >= 0 && clavi < VRIK_MAX_BONES) { float c[3]; at(0.45f, c); applyShoulderAnchor(clavi, c); }
                            at(1.0f, outJoint);
                            applyShoulderAnchor(upperIdx, outJoint);
                        };

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
                                anchorStableShoulder(g_VRRightUpperArmIdx, headModelPos, /*isLeft*/false, shoulderModelR);
                                VRIK_BuildHandTarget(shoulderModelR, shoulderBodyR, hmdRel, vrPos, vrQuat,
                                                     wristR, g_VRScaleR, offR, target, handRot);
                            } else {
                                calibratedShoulderModel(shoulderBodyR, g_VRRightUpperArmIdx, shoulderModelR);
                                applyShoulderAnchor(g_VRRightUpperArmIdx, shoulderModelR);
                                VRIK_BuildHandTarget(shoulderModelR, shoulderBodyR, hmdRel, vrPos, vrQuat, wristR, g_VRScaleR, offR, target, handRot);
                            }
                            float adjShR[3];
                            adjustShoulder(shoulderModelR, target, armLenR, adjShR);
                            applyShoulderAnchor(g_VRRightUpperArmIdx, adjShR);
                            g_sideTargetX = target[0];      // [SIDE DIAG]
                            g_sideShoulderX = adjShR[0];    // [SIDE DIAG]
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
                                anchorStableShoulder(g_VRLeftUpperArmIdx, headModelPos, /*isLeft*/true, shoulderModelL);
                                VRIK_BuildHandTarget(shoulderModelL, shoulderBodyL, hmdRel, vrPos, vrQuat,
                                                     wristL, g_VRScaleL, offL, target, handRot);
                            } else {
                                calibratedShoulderModel(shoulderBodyL, g_VRLeftUpperArmIdx, shoulderModelL);
                                applyShoulderAnchor(g_VRLeftUpperArmIdx, shoulderModelL);
                                VRIK_BuildHandTarget(shoulderModelL, shoulderBodyL, hmdRel, vrPos, vrQuat, wristL, g_VRScaleL, offL, target, handRot);
                            }
                            float adjShL[3];
                            adjustShoulder(shoulderModelL, target, armLenL, adjShL);
                            applyShoulderAnchor(g_VRLeftUpperArmIdx, adjShL);
                            VRIK_SolveArm(boneBuf, g_VRLeftUpperArmIdx, g_VRLeftForeArmIdx,
                                          g_VRLeftBoneIdx, target, handRot,
                                          bodyRight, bodyUp, bodyFwd,
                                          g_VRElbowPoleL * 0.01745329252f, g_VRElbowSwingL,
                                          /*isLeft*/true, /*storeDbg*/true);
                        }
                        // [SIDE DIAG] chain: controller X -> IK target X -> shoulder X ->
                        // final hand X. The 12cm sideways jump is somewhere in this chain;
                        // this shows WHERE it originates (ctrl/target/shoulder/solver).
                        VRIK_ComputeFK(boneBuf, g_VRBoneCount);
                        if (g_sdN < 400 && g_VRRightBoneIdx >= 0) {
                            int s = g_sdN++;
                            g_sd[s*4+0] = SharedPose(9);                  // controller X (input)
                            g_sd[s*4+1] = g_sideTargetX;                  // IK target X
                            g_sd[s*4+2] = g_sideShoulderX;                // shoulder anchor X
                            g_sd[s*4+3] = g_fkPos[g_VRRightBoneIdx][0];   // final hand X (output)
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

    FlushSideDiag();
    return result;
}

inline bool InstallVRIKMinHook() {
    HMODULE hMod = GetModuleHandleA("Cyberpunk2077.exe");
    if (!hMod) return false;
    
    // Original Hook
    void* target = reinterpret_cast<void*>(reinterpret_cast<uintptr_t>(hMod) + 0x15357A0);
    MH_Initialize();
    if (MH_CreateHook(target, &Hooked_ComponentFunc21, reinterpret_cast<void**>(&OriginalFunc21)) != MH_OK) return false;
    if (MH_EnableHook(target) != MH_OK) return false;
    return true;
}

inline bool InstallAnimPoseHook() {
    HMODULE hMod = GetModuleHandleA("Cyberpunk2077.exe");
    if (!hMod) return false;
    void* target = reinterpret_cast<void*>(reinterpret_cast<uintptr_t>(hMod) + 0x17DDB4);
    MH_Initialize(); // no-op if already initialized by InstallVRIKMinHook
    if (MH_CreateHook(target, &Hooked_AnimPoseApply, reinterpret_cast<void**>(&OriginalAnimPose)) != MH_OK)
        return false;
    if (MH_EnableHook(target) != MH_OK)
        return false;
    return true;
}

