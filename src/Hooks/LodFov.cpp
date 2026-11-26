#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
#include "Utils/DebugGate.hpp"
// LodFov -- the level-of-detail cone, and the one hook whose value is NOT the render FOV.
//
// The site reads the camera's vertical FOV and the engine squares its tangent to get the
// screen-space-error term it selects detail with. Handed a VR field of view, that term grows by
// several times and every cull and detail decision fires as if the object were that much further
// away. The main builder clamps only the value used by this computation; the
// other descriptor builders clamp the resulting term. The rendered FOV stays intact.

#include "Core/VrCoreShared.hpp"
#include "Hooks/Hook.hpp"
#include "Hooks/Trampoline.hpp"
#include "Hooks/LodFovPatch.hpp"

#include <windows.h>
#include <psapi.h>
#include <cstdint>
#include <iterator>

#include "Camera/CameraState.hpp"      // g_bdActive, g_bdSceneQuat, MulQuat
#include "Runtimes/OpenXRManager.hpp"   // the head pose this composes with
#include "Utils/MemorySafe.hpp"
#include "Utils/LogThrottle.hpp"   // g_verboseLog: the launcher's DEBUG box

#if CVR_RUST_VRIK
// The callback, the MAIN stub and the threshold tails are rust/crates/cvr-rig/src/vrik/lod_fov.rs; the exports and the
// registrations stay.
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/used_moved_exports_rest.rs.
#else
extern "C" __declspec(dllexport) int      CyberpunkVR_BdHeadWrite = 1;
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs.
#else
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugBdHeadWrites = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugBdViewSeen = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugBdViewByFov = 0;
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/used_moved_exports_rest.rs.
#else
extern "C" __declspec(dllexport) int   CyberpunkVR_BdFovWrite = 1;
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs.
#else
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugBdFovWrites = 0;
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/used_moved_exports_rest.rs.
#else
extern "C" __declspec(dllexport) int32_t CyberpunkVR_FixLodEnable = 1;
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs.
#else
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugFixLodCoverage = 0;
#endif
#if !CVR_RUST_VRIK   // registered from rust/crates/cvr-render/src/hook_table.rs
CVR_HOOK("LodFov", ::cvr::hooks::Stage::Boot, 60, cvr_hook_lod_fov_install);
#endif
#if !CVR_RUST_VRIK   // registered from rust/crates/cvr-render/src/hook_table.rs
CVR_HOOK("LodFovThreshold", ::cvr::hooks::Stage::Boot, 61, cvr_hook_lod_fov_threshold_install);
#endif
#else
namespace { uint64_t g_fixLoDHits = 0; uint64_t g_bdFovWrites = 0; uint64_t g_bdFovSeen = 0; }

// ---- THE BRAINDANCE VIEW ------------------------------------------------------------------------
//
// Layout read out of the running process (x64dbg, conditional breakpoint on this hook's own trampoline
// with dword:[rbx+0x20] == the scene fov); see the file header of the patch that introduced this.
static constexpr uint32_t kBdPos   = 0x00;   // int32[3], 1/131072 m
static constexpr uint32_t kBdQuat  = 0x10;   // float[4]  i, j, k, r
static constexpr uint32_t kBdFov   = 0x20;   // float
static constexpr uint32_t kBdBasis = 0x50;   // float[3] rows at +0x50, +0x60, +0x70

extern "C" __declspec(dllexport) int      CyberpunkVR_BdHeadWrite = 1;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugBdHeadWrites = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugBdViewSeen = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugBdViewByFov = 0;

// The scene camera's own FOV, learned on a frame the pose test confirmed and before anything of ours was
// written into it. It is the identity for the frames the published pose is too old to match.
static float g_bdLearnedFov = 0.0f;

// The struct's own orientation, which is what the head is composed onto whichever test recognised it.
static bool BdReadQuat(const uint8_t* p, float* q) {
    if (!ReadFloatArraySafe(reinterpret_cast<float*>(const_cast<uint8_t*>(p) + kBdQuat), q, 4)) return false;
    const float len2 = q[0]*q[0] + q[1]*q[1] + q[2]*q[2] + q[3]*q[3];
    return (len2 > 0.9f && len2 < 1.1f);
}

// Is this struct the camera the braindance renders through? Answered by the pose the scene reports,
// which is the one identity our own writes cannot feed back into.
// WHY IT DID NOT MATCH, not merely that it did not. The failure line used to print the fov alone,
// which says nothing about a test made of an orientation dot and a distance -- so a playback that
// sat at the scene's own 55 deg had no way to say which half was failing. Both are reported now,
// and it matters: the by-fov fast path cannot start until the pose test has matched ONCE, so a
// pose test that rarely matches shows up as a FOV that arrives late rather than one that is wrong.
static float g_bdWhyDot   = -1.0f;
static float g_bdWhyDist  = -1.0f;
static int   g_bdWhyStage = 0;   // 0 pose invalid, 1 bad quat, 2 dot, 3 dist, 4 matched

// THE SAME TEST, AGAINST THE LENS INSTEAD OF THE SCENE. A takeover has no scene pose -- nothing
// publishes one -- so the braindance test above returns false at its first line and the fov was never
// written: the panel read "fov 60 active". The device camera's pose is
// what the port already has for it, latched by the classifier from the camera the engine renders through,
// so the identification is the same shape with a different reference.
static bool DevViewMatches(const uint8_t* p, float* qOut) {
    if (!g_devCamBaseValid.load(std::memory_order_acquire)) return false;
    if (!g_devCamPosValid.load(std::memory_order_acquire)) return false;
    float q[4] = {};
    if (!ReadFloatArraySafe(reinterpret_cast<const float*>(p + kBdQuat), q, 4)) return false;
    const float len2 = q[0]*q[0] + q[1]*q[1] + q[2]*q[2] + q[3]*q[3];
    if (!(len2 > 0.9f && len2 < 1.1f)) return false;
    // Orientation is NOT part of the test. The lens's own quaternion is what the engine had before our
    // composition went in, and by the time the view is built it carries the head as well -- comparing
    // them would fail exactly on the frames that matter. Position is untouched by the head.
    int32_t pos[3] = {};
    for (int i = 0; i < 3; ++i) {
        uint32_t v = 0;
        if (!ReadU32Safe(reinterpret_cast<uintptr_t>(p) + kBdPos + i * 4, &v)) return false;
        pos[i] = static_cast<int32_t>(v);
    }
    const float k = 1.0f / 131072.0f;
    float d2 = 0.0f;
    for (int i = 0; i < 3; ++i) {
        const float d = (pos[i] - g_devCamPosFP[i].load(std::memory_order_relaxed)) * k;
        d2 += d * d;
    }
    // Half a metre: the lens position is stamped when the device camera is patched, so a moving mount
    // leaves a frame of travel between the two, and the eye separation is in here as well.
    if (d2 > 0.5f * 0.5f) return false;
    qOut[0] = q[0]; qOut[1] = q[1]; qOut[2] = q[2]; qOut[3] = q[3];
    return true;
}

static bool BdViewMatches(const uint8_t* p, float* qOut) {
    g_bdWhyStage = 0; g_bdWhyDot = -1.0f; g_bdWhyDist = -1.0f;
    if (!g_bdScenePoseValid.load(std::memory_order_acquire)) return false;
    float q[4] = {};
    if (!ReadFloatArraySafe(reinterpret_cast<const float*>(p + kBdQuat), q, 4)) return false;
    const float len2 = q[0]*q[0] + q[1]*q[1] + q[2]*q[2] + q[3]*q[3];
    g_bdWhyStage = 1;
    if (!(len2 > 0.9f && len2 < 1.1f)) return false;
    float dot = q[0]*g_bdSceneQuat[0] + q[1]*g_bdSceneQuat[1] +
                q[2]*g_bdSceneQuat[2] + q[3]*g_bdSceneQuat[3];
    if (dot < 0.0f) dot = -dot;
    g_bdWhyStage = 2; g_bdWhyDot = dot;
    if (dot < 0.999f) return false;

    int32_t pos[3] = {};
    for (int i = 0; i < 3; ++i) {
        uint32_t v = 0;
        if (!ReadU32Safe(reinterpret_cast<uintptr_t>(p) + kBdPos + i * 4, &v)) return false;
        pos[i] = static_cast<int32_t>(v);
    }
    const float k = 1.0f / 131072.0f;
    float d2 = 0.0f;
    for (int i = 0; i < 3; ++i) {
        const float d = (pos[i] - g_bdScenePosFP[i].load(std::memory_order_relaxed)) * k;
        d2 += d * d;
    }
    g_bdWhyStage = 3; g_bdWhyDist = sqrtf(d2);
    if (d2 > 0.75f * 0.75f) return false;
    g_bdWhyStage = 4;
    qOut[0] = q[0]; qOut[1] = q[1]; qOut[2] = q[2]; qOut[3] = q[3];
    return true;
}

// BdWriteRotation IS GONE, and the reason is worth keeping. It wrote the composed orientation into
// this descriptor's quaternion at +0x10 and hand-patched the basis rows at +0x50 -- but the engine
// derives BOTH basis matrices and BOTH translation rows from +0x10 and +0x00 inside
// sub_140296EA0, which the builder calls at exe+0x4E4264: ONE INSTRUCTION BEFORE the site this
// callback is hooked to (read in IDA: `add rcx,10h` -> `call sub_1401DA684` quaternion-to-matrix,
// then the shufps transpose, then cvtdq2ps of the int32 position). So the write always landed after
// the derivation, leaving the second matrix and both translations built from the previous
// orientation -- an internally inconsistent camera descriptor, which is the shake and the shimmer
// that were reported.
//
// The orientation now goes through LocateCamera instead, which is upstream of this descriptor
// entirely, so everything derived from it agrees by construction. VRCAM keeps PatchCamera.


// THE ONE SITE THAT PROVABLY SEES THE BRAINDANCE FOV. The port's NormalFOV hook is never called during a
// braindance -- measured: its last log line predates the scene -- while this callback was handed 55.879
// for the whole of it. So while the braindance gate is up and the value handed in is not the headset's,
// it is written back into the field it was read from. If that field turns out to be a copy nobody else
// reads, the counter below says the write happened and the picture will say it changed nothing.
extern "C" __declspec(dllexport) int   CyberpunkVR_BdFovWrite = 1;
extern "C" float CyberpunkVR_DebugVrcamWantFov;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugBdFovWrites = 0;
// 1 = apply the LOD override to the caller whose incoming value falls in the window below (the
// shipping behaviour); 0 = leave every caller on the engine's own value, both eyes alike.
// Live, through xr_fix_lod.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_FixLodEnable = 1;
// Installed coverage: bit 0 MAIN, bit 1 constructor, bit 2 plane override, bit 3 SetFov.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugFixLodCoverage = 0;

extern "C" float __fastcall OnFixLoDCallback(float* rbxPtr, float originalVal) {
    g_fixLoDHits++;
    
    float result = originalVal;

    // rbx+0x20 is where this site read the FOV from -- the hook's own pattern is
    // `movss xmm0,[rbx+0x20]` -- so rbxPtr points at rbx and the field is at +0x20 in bytes.
    // THE BRAINDANCE CAMERA, identified by the POSE the scene publishes rather than by its fov.
    //
    // Keying on the fov is what made the field flicker between ours and the game's -- a changing
    // projection every other frame, which is the jitter that was reported. Measured: 48 "left alone" at
    // 55.879 against 10 no-op patches of our own 110, because the discriminator had been poisoned by our
    // own write. The pose cannot be poisoned: it comes from the scene system through VRSceneCamera.
    const bool takeoverFov = LocateOwnsTakeover();
    if (rbxPtr && CyberpunkVR_BdFovWrite &&
        (g_bdActive.load(std::memory_order_relaxed) || takeoverFov)) {
        uint8_t* p = reinterpret_cast<uint8_t*>(rbxPtr);
        float qBase[4] = {};
        const float want = g_normalFovOverrideValue;
        const bool notOursYet = (want > 1.0f && want < 179.0f) && fabsf(originalVal - want) > 0.01f;

        // SEEDED FROM THE SCRIPT, so the fov path does not have to wait for a pose match to start.
        // Guarded the same way the learner is: a value that equals what we write is our own output
        // coming back, and learning it would turn the test into "patch what was already patched".
        {
            const float sceneFov = g_bdSceneFov.load(std::memory_order_relaxed);
            if (sceneFov > 1.0f && sceneFov < 179.0f &&
                (want <= 1.0f || fabsf(sceneFov - want) > 0.01f) &&
                fabsf(sceneFov - g_bdLearnedFov) > 0.01f) {
                g_bdLearnedFov = sceneFov;
            }
        }

        bool isBdView = g_bdActive.load(std::memory_order_relaxed)
                            ? BdViewMatches(p, qBase)
                            : DevViewMatches(p, qBase);
        if (isBdView) {
            // Learn the scene's own fov, but only from a frame that has not been written yet -- otherwise
            // the value learned is our own and the test degenerates into "patch what we already patched".
            if (notOursYet && originalVal > 1.0f && originalVal < 179.0f)
                g_bdLearnedFov = originalVal;
        } else if (g_bdLearnedFov > 1.0f && fabsf(originalVal - g_bdLearnedFov) < 0.5f) {
            // The published pose is a tick old and this pan outran it. The fov says it is the same view,
            // and the base for the head comes out of the struct itself.
            isBdView = BdReadQuat(p, qBase);
            if (isBdView) CVR_DIAGNOSTIC(++CyberpunkVR_DebugBdViewByFov);
        }

        if (isBdView) {
            CVR_DIAGNOSTIC(++CyberpunkVR_DebugBdViewSeen);

            // NOT ON THE SECOND EYE'S VIEW. The test above identifies the lens's view by POSITION,
            // within half a metre -- which was sound while only MAIN sat there. With
            // xr_dev_cam_in_locate=2 the takeover push puts the second eye at the lens as well, three
            // centimetres away, so its view passes the same test and this write flattened its fov to
            // the headset's. That is exactly the zoom that worked with cam in locate = 1 and not with 2:
            // the camera's 4x zoom reached the second eye as a narrow fov (39.30 measured in its
            // component, with main yy 2.80083 and ADS x4.000 on the panel) and was then overwritten
            // with 110 right here.
            //
            // Position cannot separate them at three centimetres. The FOV can: the second eye's view
            // carries the value the port computed for it this frame, and MAIN's view never does. When
            // there is no zoom the two coincide -- and then `notOursYet` is already false, so this test
            // only ever bites on the frames where they genuinely differ.
            const float vrcamWant = CyberpunkVR_DebugVrcamWantFov;
            const bool isSecondEyeView =
                (vrcamWant > 1.0f && fabsf(originalVal - vrcamWant) < 0.05f);

            // THE FOV, as before -- this is the write that proved the struct reaches the picture.
            if (CyberpunkVR_BdFovWrite && notOursYet && !isSecondEyeView &&
                originalVal > 1.0f && originalVal < 179.0f) {
                WriteFloatSafe(reinterpret_cast<uintptr_t>(p) + kBdFov, want);
                CVR_DIAGNOSTIC(++CyberpunkVR_DebugBdFovWrites);
                result = want;
            }

            // (The head write used to sit here; see the note where BdWriteRotation was removed.
            //  Braindance orientation is composed and written in LocateCamera now.)
            if ((++g_bdFovWrites % 300) == 1)
                if (g_verboseLog) Log("FixLoD: braindance view at %p fov %.3f -> %.3f | head=%llu views=%llu "
                    "byFov=%llu learned=%.3f\n",
                    rbxPtr, originalVal, want,
                    static_cast<unsigned long long>(CyberpunkVR_DebugBdHeadWrites),
                    static_cast<unsigned long long>(CyberpunkVR_DebugBdViewSeen),
                    static_cast<unsigned long long>(CyberpunkVR_DebugBdViewByFov),
                    static_cast<double>(g_bdLearnedFov));
        } else if ((++g_bdFovSeen % 3000) == 1) {
            if (g_verboseLog) Log("FixLoD: braindance, view is not the scene camera: fov %.3f stage=%d "
                "dot=%.4f dist=%.3f | want=%.3f learned=%.3f poseValid=%d\n",
                originalVal, g_bdWhyStage, static_cast<double>(g_bdWhyDot),
                static_cast<double>(g_bdWhyDist), static_cast<double>(want),
                static_cast<double>(g_bdLearnedFov),
                g_bdScenePoseValid.load(std::memory_order_acquire) ? 1 : 0);
        }
    }
    
    // THE LOD OVERRIDE. Read out of the engine, not guessed -- IDA over Cyberpunk2077.exe plus a
    // conditional breakpoint at the site, 2026-08-28.
    //
    // WHAT THE SITE COMPUTES. In 2.5, exe+0x425A44 is the FOV read near the tail of
    // sub_14042579C, the main descriptor builder (2.31: exe+0x4E4269):
    //
    //     movss xmm0,[rbx+0x20]        the fov, IN DEGREES
    //     mulss xmm0, 0.008726646      = pi/360, degrees -> HALF-angle in radians
    //     call  tanf                   named in the database, not inferred
    //     mulss xmm0, xmm0             tan^2(fov/2)
    //     mulss xmm0, 1.698400736
    //     maxss xmm0, 1.0              A FLOOR OF EXACTLY ONE
    //     movss [rbx+0x24], xmm0       the threshold this view will use
    //
    //   threshold = max( tan^2(fov/2) * 1.6984 , 1.0 )
    //
    //     fov  51.000 ->  0.386 -> 1.000 (floored)      fov 100.000 -> 2.412
    //     fov  55.879 ->  0.478 -> 1.000 (floored)      fov 110.000 -> 3.464
    //     fov  75.000 ->  1.000 -> 1.000 exactly        fov   3.046 -> 0.001 -> 1.000 (floored)
    //
    // Any sufficiently small angle lands on the floor. The break-even is approximately 75 degrees;
    // the VR camera's wider field raises this term without changing actual object distance.
    //
    // WHICH VIEW GOT WHICH, measured live in braindance at the builder's entry, where rdx+0x38 is the
    // source fov and r8 points at the camera's position:
    //
    //     fov 100.000000  position -1721.6096  -1236.0948  23.5544
    //     fov 109.999992  position -1721.6055  -1236.1268  23.5544
    //                     delta 32.3 mm = half an IPD, and the pair matches MAIN/VRCAM in the
    //                     viewData diff exactly (MAIN C4D74378, VRCAM C4D74360)
    //
    // So the two eyes entered the same computation with different angles: MAIN at 100 got 2.412 while
    // the old rule pulled VRCAM's 110 down to the floor. MAIN therefore culled distant detail the other
    // eye kept -- seen as billboards missing in the distance and a wall decal drawn only half.
    //
    // Clamp the computation's input for wide FOVs, keeping the historical 3.04639287f substitute.
    // It lands safely below the MAXSS floor; 75.0f itself can round slightly above one.
    // This substitutes the register value only, not the descriptor's rendered FOV.
    constexpr float kFovAtFloor = 75.0f;
    // ON `result`, NOT ON `originalVal`, and that distinction is the whole bug. The braindance block
    // above already replaced the output with our wide fov -- its own log line reads
    // `originalVal=55.879314 result=109.999992` -- so testing the INPUT left the braindance camera
    // feeding 110 into the threshold: tan^2(55)*1.6984 = 3.464, the harshest culling in the frame, and
    // exactly the "did not work for the braindance camera" that was reported. What reaches the engine
    // is `result`, so that is what has to be clamped, after every block that can write it.
    if (CyberpunkVR_FixLodEnable && result > kFovAtFloor) {
        result = 3.04639287f;
    }
    if (g_fixLoDHits % 600 == 1) {
        if (g_verboseLog) Log("FixLoD: hits=%llu rbx=%p originalVal=%.6f result=%.6f isVRCamera=%d\n",
            static_cast<unsigned long long>(g_fixLoDHits),
            rbxPtr,
            originalVal,
            result,
            (result != originalVal) ? 1 : 0);
    }
    return result;
}

static bool LodImage(uint8_t*& base, size_t& size) {
    HMODULE exe = GetModuleHandleA("Cyberpunk2077.exe");
    MODULEINFO info{};
    if (!exe || !GetModuleInformation(GetCurrentProcess(), exe, &info, sizeof(info))) return false;
    base = static_cast<uint8_t*>(info.lpBaseOfDll);
    size = info.SizeOfImage;
    return true;
}

static bool ValidateLodTails(const uint8_t* base, size_t size) {
    if (!cvr::lod_patch::ConstantsMatch(base,size)) {
        Log("FixLoD: LOD constants do not match the verified game build\n");
        return false;
    }
    for (const auto& site : cvr::lod_patch::kTails) {
        if (!cvr::lod_patch::Matches(base,size,site)) {
            Log("FixLoD: %s guard mismatch at exe+0x%llX\n",site.name,static_cast<unsigned long long>(site.rva));
            return false;
        }
    }
    return true;
}

bool InstallFixLoDHook() {
    uint8_t* base{}; size_t size{};
    if (!LodImage(base,size) || !cvr::lod_patch::Matches(base,size,cvr::lod_patch::kMain) ||
        !ValidateLodTails(base,size)) {
        Log("FixLoD: verified MAIN/descriptor sites unavailable; no pattern fallback\n");
        return false;
    }
    uint8_t* found = base+cvr::lod_patch::kMain.rva;

    constexpr int replaceLen = 5; // movss (5)
    void* tramp = AllocateTrampoline(found, 256);
    if (!tramp) return false;

    uint8_t* code = static_cast<uint8_t*>(tramp);
    int pos = 0;

    // --- CALL C++ CALLBACK ---
    // Save volatile registers
    code[pos++] = 0x9C; // pushfq
    code[pos++] = 0x50; // push rax
    code[pos++] = 0x51; // push rcx
    code[pos++] = 0x52; // push rdx
    code[pos++] = 0x41; code[pos++] = 0x50; // push r8
    code[pos++] = 0x41; code[pos++] = 0x51; // push r9
    code[pos++] = 0x41; code[pos++] = 0x52; // push r10
    code[pos++] = 0x41; code[pos++] = 0x53; // push r11
    code[pos++] = 0x55; // push rbp

    // Save xmm registers
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x40; // sub rsp, 40h
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x04; code[pos++] = 0x24; // movups [rsp], xmm0
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10; // movups [rsp+10h], xmm1
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20; // movups [rsp+20h], xmm2
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30; // movups [rsp+30h], xmm3

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xE5; // mov rbp, rsp
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xE4; code[pos++] = 0xF0; // and rsp, -16
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x20; // sub rsp, 20h

    // Set arg1 (rcx) = rbx
    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xD9; // mov rcx, rbx
    // Set arg2 (xmm1) = [rbx + 20h]
    code[pos++] = 0xF3; code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x4B; code[pos++] = 0x20; // movss xmm1, [rbx+20h]

    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(OnFixLoDCallback));
    code[pos++] = 0xFF; code[pos++] = 0xD0; // call rax

    // Save returned value (xmm0) to stack slot for xmm0
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x45; code[pos++] = 0x00; // movups [rbp], xmm0

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xEC; // mov rsp, rbp

    // Restore xmm registers (xmm0 will be our returned value!)
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x04; code[pos++] = 0x24; // movups xmm0, [rsp]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10; // movups xmm1, [rsp+10h]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20; // movups xmm2, [rsp+20h]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30; // movups xmm3, [rsp+30h]
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xC4; code[pos++] = 0x40; // add rsp, 40h

    // Restore volatile registers
    code[pos++] = 0x5D; // pop rbp
    code[pos++] = 0x41; code[pos++] = 0x5B; // pop r11
    code[pos++] = 0x41; code[pos++] = 0x5A; // pop r10
    code[pos++] = 0x41; code[pos++] = 0x59; // pop r9
    code[pos++] = 0x41; code[pos++] = 0x58; // pop r8
    code[pos++] = 0x5A; // pop rdx
    code[pos++] = 0x59; // pop rcx
    code[pos++] = 0x58; // pop rax
    code[pos++] = 0x9D; // popfq

    // Jump back to found + 5
    code[pos++] = 0xE9;
    *reinterpret_cast<int32_t*>(code + pos) = static_cast<int32_t>((found + replaceLen) - (code + pos + 4));
    pos += 4;

    DWORD oldProtect;
    if (!VirtualProtect(found, replaceLen, PAGE_EXECUTE_READWRITE, &oldProtect)) {
        Log("FixLoD: MAIN protection change failed (%lu)\n",GetLastError());
        return false;
    }
    found[0] = 0xE9;
    *reinterpret_cast<int32_t*>(found + 1) = static_cast<int32_t>(code - (found + 5));
    for (int i = 5; i < replaceLen; ++i) found[i] = 0x90; // NOP
    VirtualProtect(found, replaceLen, oldProtect, &oldProtect);
    FlushInstructionCache(GetCurrentProcess(), found, replaceLen);

    CyberpunkVR_DebugFixLodCoverage |= 1u;
    Log("FixLoD hook: guarded MAIN at exe+0x%llX -> %p (trampoline %p)\n",
        static_cast<unsigned long long>(cvr::lod_patch::kMain.rva), found, tramp);
    return true;
}

CVR_HOOK("LodFov", ::cvr::hooks::Stage::Boot, 60, InstallFixLoDHook);

// ---- THE SAME THRESHOLD, BUILT AGAIN ------------------------------------------------------------
// Verified 2.5 sites: constructor, plane override and SetFov. MAIN is covered by
// the callback above. Replace only MULSS by the coefficient: the stereo flag
// TEST/MOV before it and the following MAXSS/store/JE remain native.
static bool InstallFixLoDThresholdSites() {
    if (!(CyberpunkVR_DebugFixLodCoverage&1u)) {
        Log("FixLoD threshold: MAIN hook unavailable; descriptor tails not installed\n");
        return false;
    }
    uint8_t* base{}; size_t size{};
    if (!LodImage(base,size) || !ValidateLodTails(base,size)) return false;
    uint32_t patched=0;
    for (size_t i=0;i<std::size(cvr::lod_patch::kTails);++i) {
        const auto& site=cvr::lod_patch::kTails[i];
        uint8_t* const mul=base+site.rva;
        uint8_t* const code=static_cast<uint8_t*>(AllocateTrampoline(mul,64));
        if (!code) {
            Log("FixLoD threshold: no trampoline space for %s\n",site.name);
            continue;
        }
        const auto displacement=int64_t(reinterpret_cast<uintptr_t>(code))-int64_t(reinterpret_cast<uintptr_t>(mul+5));
        if (displacement<std::numeric_limits<int32_t>::min() || displacement>std::numeric_limits<int32_t>::max() ||
            !cvr::lod_patch::EmitThreshold(code,reinterpret_cast<uintptr_t>(code),
                reinterpret_cast<uintptr_t>(&CyberpunkVR_FixLodEnable),
                reinterpret_cast<uintptr_t>(base+cvr::lod_patch::kScaleRva),reinterpret_cast<uintptr_t>(mul+8))) {
            Log("FixLoD threshold: trampoline out of range for %s\n",site.name);
            continue;
        }
        DWORD oldProtect{};
        if (!VirtualProtect(mul,8,PAGE_EXECUTE_READWRITE,&oldProtect)) {
            Log("FixLoD threshold: protection change failed for %s (%lu)\n",site.name,GetLastError());
            continue;
        }
        mul[0]=0xE9;
        const auto jump=int32_t(displacement);
        std::memcpy(mul+1,&jump,sizeof(jump));
        mul[5]=mul[6]=mul[7]=0x90;
        VirtualProtect(mul,8,oldProtect,&oldProtect);
        FlushInstructionCache(GetCurrentProcess(),mul,8);
        CyberpunkVR_DebugFixLodCoverage |= 1u << (i+1);
        ++patched;
        Log("FixLoD threshold: guarded %s at exe+0x%llX -> trampoline %p\n",
            site.name,static_cast<unsigned long long>(site.rva),code);
    }
    Log("FixLoD threshold: %u/3 installed; coverage=0x%X (complete=0xF)\n",
        patched,CyberpunkVR_DebugFixLodCoverage);
    return patched==3;
}

CVR_HOOK("LodFovThreshold", ::cvr::hooks::Stage::Boot, 61, InstallFixLoDThresholdSites);
#endif
#endif
