#pragma once
#include <windows.h>
#include <cstdint>
#include <iostream>
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
// Pose-apply hook on sub_14017DDB4 (RVA 0x17DDB4), found via Cheat Engine
// write-BP on the player's live track buffer. This function copies the
// evaluated pose into the destination skeleton:
//   a2[7][0] = bone transform buffer (48 bytes/bone: quat@0, translation@16, scale@32)
//   a2[7][3] = track value buffer (used to identify the player)
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
extern volatile int       g_VRBind;              // 0 off, 1=right pos, 2=right pos+rot, 3=both pos(+rot)
extern volatile float     g_VRBindScale;         // position scale (VR units -> model units)
extern volatile float     g_VRBindOffX;
extern volatile float     g_VRBindOffY;
extern volatile float     g_VRBindOffZ;
extern volatile int       g_VRBindAxis;          // axis-remap preset 0..5
extern volatile int       g_VRRightBoneIdx;      // default 24 (RightHand)
extern volatile int       g_VRLeftBoneIdx;       // default 23 (LeftHand)



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

typedef void* (*AnimPoseFunc_t)(void* a1, void* a2, void* a3, unsigned int a4);
static AnimPoseFunc_t OriginalAnimPose = nullptr;

extern "C" inline void* Hooked_AnimPoseApply(void* a1, void* a2, void* a3, unsigned int a4) {
    void* result = OriginalAnimPose(a1, a2, a3, a4);
    ++g_AnimPoseTotalCalls;

    if ((g_PlayerTrackBufA || g_PlayerTrackBufB) && VRIK_IsReadable(a2, 0x40)) {
        void* poseDesc = reinterpret_cast<void**>(a2)[7];
        if (VRIK_IsReadable(poseDesc, 0x20)) {
            void*     boneBuf  = reinterpret_cast<void**>(poseDesc)[0];
            uintptr_t trackBuf = reinterpret_cast<uintptr_t*>(poseDesc)[3];
            if (trackBuf && (trackBuf == g_PlayerTrackBufA || trackBuf == g_PlayerTrackBufB)) {
                ++g_AnimPoseMatchCalls;
                g_AnimPoseLastBoneBuf = reinterpret_cast<uintptr_t>(boneBuf);
                if (g_AnimPoseDebug == 1 && VRIK_IsReadable(boneBuf, 48 * 64)) {
                    __try {
                        // Calibration: shove translation (offset +16 in each 48-byte bone)
                        // of bones 0..63 by +1.5m. Whole upper body visibly distorts.
                        for (int b = 0; b < 64; ++b) {
                            float* t = reinterpret_cast<float*>(reinterpret_cast<uint8_t*>(boneBuf) + b * 48 + 16);
                            t[0] += 1.5f; t[1] += 1.5f; t[2] += 1.5f;
                        }
                    } __except (EXCEPTION_EXECUTE_HANDLER) {}
                }
                else if (g_AnimPoseDebug == 2) {
                    int idx = g_AnimPoseTestBone;
                    if (idx >= 0 && VRIK_IsReadable(reinterpret_cast<uint8_t*>(boneBuf) + idx * 48, 48)) {
                        __try {
                            float m = g_AnimPoseTestMag;
                            // +0 = translation (QsTransform: translation@0, rotation@16, scale@32)
                            float* t = reinterpret_cast<float*>(reinterpret_cast<uint8_t*>(boneBuf) + idx * 48 + 0);
                            t[0] += m; t[1] += m; t[2] += m;
                        } __except (EXCEPTION_EXECUTE_HANDLER) {}
                    }
                }

                // VRIK LIVE BINDING
                if (g_VRBind > 0 && g_pSharedHands) {
                    float s = g_VRBindScale;
                    float offX = g_VRBindOffX, offY = g_VRBindOffY, offZ = g_VRBindOffZ;
                    int axis = g_VRBindAxis;
                    
                    __try {
                        // Right Hand (VR offset 8, bone index default 24)
                        if (g_pSharedHands[8] > 0.0f) {
                            int bIdx = g_VRRightBoneIdx;
                            if (bIdx >= 0 && VRIK_IsReadable(reinterpret_cast<uint8_t*>(boneBuf) + bIdx * 48, 48)) {
                                float* t = reinterpret_cast<float*>(reinterpret_cast<uint8_t*>(boneBuf) + bIdx * 48 + 0);
                                float vr_pos[3] = { g_pSharedHands[9], g_pSharedHands[10], g_pSharedHands[11] };
                                float md_pos[3];
                                VRIK_RemapAxis(axis, vr_pos, md_pos);
                                
                                t[0] = md_pos[0] * s + offX;
                                t[1] = md_pos[1] * s + offY;
                                t[2] = md_pos[2] * s + offZ;
                                
                                if (g_VRBind == 2 || g_VRBind == 3) {
                                    // ... rotation later
                                }
                            }
                        }

                        // Left Hand
                        if ((g_VRBind == 3) && g_pSharedHands[0] > 0.0f) {
                            int bIdx = g_VRLeftBoneIdx;
                            if (bIdx >= 0 && VRIK_IsReadable(reinterpret_cast<uint8_t*>(boneBuf) + bIdx * 48, 48)) {
                                float* t = reinterpret_cast<float*>(reinterpret_cast<uint8_t*>(boneBuf) + bIdx * 48 + 0);
                                float vr_pos[3] = { g_pSharedHands[1], g_pSharedHands[2], g_pSharedHands[3] };
                                float md_pos[3];
                                VRIK_RemapAxis(axis, vr_pos, md_pos);
                                
                                t[0] = md_pos[0] * s + offX;
                                t[1] = md_pos[1] * s + offY;
                                t[2] = md_pos[2] * s + offZ;
                            }
                        }
                    } __except (EXCEPTION_EXECUTE_HANDLER) {}
                }
            }
        }
    }
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

