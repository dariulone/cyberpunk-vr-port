// FinalCamera -- one hook, one file.
//
// Reads back the quaternion the engine is about to render with and finds it in the write
// ring, which identifies the frame's pose exactly -- no assumption about how far ahead the
// engine renders. It is a LEAF, not part of the cycle: nothing the other two read is
// written here. It sat in the knot only because the ring and the seqlock quaternion are not
// plain loads, and those now live behind Camera/CameraLink.hpp.
//
// INSTALL ORDER IS EXACTLY WHAT IT WAS: Locate 10, Patch 12, Final 14, all in Stage::Boot. An
// adversarial pass over the plan for this split proposed reordering Patch before Locate for a
// "single-meaning" startup signal, and the payoff does not exist: g_engineCamQuatValid is set only
// when camKind == 1, which needs ClassifyPatchCameraOwner to have self-calibrated its name offset,
// which cannot happen until MAIN's placed component passes the site. The fallback stays an ordinary
// startup transient in BOTH orders. The two windows being weighed are the gap between consecutive
// FindPattern calls inside one function -- microseconds. An unobservable change is not a safe
// change; it is an unfalsifiable one, so the order is preserved.

#include "Camera/CameraLink.hpp"
#include "Camera/CameraState.hpp"
#include "Utils/LogThrottle.hpp"
#include "Core/LiveControls.hpp"
#include "Core/Telemetry.hpp"
#include "Core/VrCoreShared.hpp"
#include "Hooks/Hook.hpp"
#include "Hooks/Trampoline.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include "Utils/AobScanner.hpp"
#include "Utils/MemorySafe.hpp"

#include <windows.h>
#include <atomic>
#include <cmath>
#include <cstdint>
#include <cstddef>

extern "C" void __fastcall OnFinalCameraCallback(float* rsiPtr) {
    g_finalCameraHits++;
    if (g_telemetry) {
        g_telemetry->finalHits = static_cast<uint32_t>(g_finalCameraHits);
        g_telemetry->finalRsi = reinterpret_cast<uintptr_t>(rsiPtr);
    }

    const uint32_t locateSeq = g_lastLocateSeq;
    if (locateSeq != 0) {
        g_renderedSeq = locateSeq;
    }

    if (!rsiPtr || reinterpret_cast<uintptr_t>(rsiPtr) < 0x10000) return;

    // ---- READ BACK THE POSE THIS FRAME IS ACTUALLY BEING BUILT WITH ---------------------------
    //
    // Read-only, and it runs whatever the write path is set to -- it is a measurement of the
    // engine, not a modification of it. rsiPtr is the render camera + 0x70, so rsiPtr + 4 floats
    // is the camera quaternion at object+0x80 (verified live: for
    // q = (-0.112416, 0.204406, -0.710105, 0.664354) the basis rows at +0xC0 matched R(q) with
    // the Y/Z columns exchanged, to 1e-5). That quaternion is the one we composed and wrote into
    // the placed component, carried through the view producer, so finding it in the write ring
    // tells us exactly which XR sample is in this frame.
    //
    // MAIN only: both views carry the same orientation, so taking both would push two entries
    // for one presented frame and the queue would run at double rate. For the barrel packet the
    // same restriction is what keeps VRCAM from overwriting MAIN's render camera -- the overlay is
    // drawn into MAIN's backbuffer and the second eye is handed the finished NDC separately.
    //
    // THE GATE MOVED INWARD (dabinn, TofuExpress 821e8a4e): the quaternion read is now shared by two
    // consumers, the pose read-back and the barrel packet, and only the first of them is what
    // CyberpunkVR_PoseReadBack is about.
    if (CyberpunkVR_StereoModuleLoaded) {
        const bool isVrcam = CyberpunkVR_IsVrcamViewActive() != 0;
        const bool isMain  = !isVrcam && CyberpunkVR_IsMainViewActive() != 0;
        if (isMain) {
            float camq[4] = {};
            if (ReadFloatArraySafe(rsiPtr + 4, camq, 4) && IsPlausibleUnitQuaternion(camq)) {
                // LATCHED, and for the reason the overlay already latched it on its own side: with
                // no weapon the muzzle quaternion is identity and the publisher then sends its +Y as
                // exactly (0,1,0) -- not a barrel direction, and it used to drag the aim point
                // behind the camera. Anything that is exactly that default is not an answer, so the
                // last real direction is kept. The latch lives here now so the direction and the
                // quaternion leave as ONE packet.
                static float s_muzzleFwd[3] = {};
                if (float* sh = GetShotShared(); sh && sh[27] >= 0.5f) {
                    const float x = sh[24], y = sh[25], z = sh[26];
                    const bool identityDefault = (x == 0.0f && y == 1.0f && z == 0.0f);
                    if (!identityDefault && x * x + y * y + z * z > 0.25f) {
                        s_muzzleFwd[0] = x; s_muzzleFwd[1] = y; s_muzzleFwd[2] = z;
                    }
                }
                if (s_muzzleFwd[0] * s_muzzleFwd[0] +
                    s_muzzleFwd[1] * s_muzzleFwd[1] +
                    s_muzzleFwd[2] * s_muzzleFwd[2] > 0.25f) {
                    cvr::camera::BarrelFrame bf{};
                    bf.camQuat[0] = camq[0]; bf.camQuat[1] = camq[1];
                    bf.camQuat[2] = camq[2]; bf.camQuat[3] = camq[3];
                    bf.muzzleFwd[0] = s_muzzleFwd[0];
                    bf.muzzleFwd[1] = s_muzzleFwd[1];
                    bf.muzzleFwd[2] = s_muzzleFwd[2];
                    cvr::camera::BarrelFramePublish(bf);
                }

                if (CyberpunkVR_PoseReadBack) {
                    OpenXRHeadPose matched{};
                    uint32_t age = 0, ties = 0;
                    if (cvr::camera::CamWriteRecordFind(camq, &matched, &age, &ties)) {
                        CyberpunkVR_DebugFinalAge  = age;
                        CyberpunkVR_DebugFinalTies = ties;
                        if (ties > 1) ++CyberpunkVR_DebugFinalTieHits;
                        ++CyberpunkVR_DebugFinalMatch;
                        OpenXRManager::Get().PushRenderedFramePose(matched);
                    } else {
                        ++CyberpunkVR_DebugFinalNoMatch;
                    }
                }
            }
        }

        // THE SECOND VIEW, IDENTIFIED THE SAME WAY, INTO ITS OWN QUEUE.
        //
        // The read-back above is MAIN-only, and the reason recorded for that is the QUEUE -- one queue
        // cannot carry two views without running at double rate -- not that the second view is
        // unidentifiable. It is identified by exactly the same means: its render camera's quaternion is
        // one we wrote, so the ring says which XR sample produced it. With a queue of its own the eye can
        // be submitted with the pose ITS pixels were drawn with instead of the other eye's, which is what
        // an OpenXR projection layer's per-view pose is for.
        //
        // The barrel packet above is NOT duplicated: that restriction exists to keep VRCAM from
        // overwriting MAIN's render camera, and it is unrelated to the pose.
        if (isVrcam && CyberpunkVR_PoseReadBack) {
            float camq[4] = {};
            if (ReadFloatArraySafe(rsiPtr + 4, camq, 4) && IsPlausibleUnitQuaternion(camq)) {
                OpenXRHeadPose matched{};
                uint32_t age = 0, ties = 0;
                if (cvr::camera::CamWriteRecordFind(camq, &matched, &age, &ties)) {
                    CyberpunkVR_DebugVrcamFinalAge = age;
                    ++CyberpunkVR_DebugVrcamFinalMatch;
                    OpenXRManager::Get().PushVrcamRenderedFramePose(matched);
                } else {
                    ++CyberpunkVR_DebugVrcamFinalNoMatch;
                }
            }
        }
    }

    // ---- MONO: THE PER-VIEW WRITE SITE -------------------------------------------------------
    //
    // This callback sits inside CRenderNode_PrepareSceneRendering (sub_140784ABC ->
    // sub_1407854C0), the third node of the FIRST pipeline stage, and it runs ONCE PER VIEW.
    // Measured live: consecutive hits alternate between exactly two camera objects, A -> B -> A,
    // sharing one vtable, positions 23 micrometres apart (i.e. the same viewpoint -- there is no
    // eye separation in these objects at all, so writing here owns both eyes outright).
    //
    // And it is the same object the view-matrix bake reads: a breakpoint on
    // `mov rsi,[r14+18h]` inside sub_140788A9C (CRenderNode_SetStreamlineConstants, stage 8)
    // returned one of those two pointers. So a write here is seen by the matrix bake AND by
    // everything between -- which is why frame-open is the right place and stage 8 is not.
    //
    // `rsiPtr` is the component + 0x70: int32 fixed-point position at [0..2], and the rotation
    // rows the bake consumes at component +0xC0 == rsiPtr + 20 floats, which is exactly what
    // ApplyFinalCameraOrientationFromQuat already writes. The machinery was here all along; it
    // was simply gated behind AER.
    //
    // DEFAULT OFF. The pose-binding work in this same build has to be measurable on its own
    // first -- two changes at once and a regression tells you nothing. Flip live to compare.
    {
        if (!CyberpunkVR_CamWriteInFinal) return;

        // ASK THE DISPATCHER, DO NOT HASH A NAME.
        //
        // The first attempt compared the view key against CyberpunkVR_VrcamCamNameHash(), and
        // VRCAM matched exactly zero times out of 8395 MAIN hits. That hash is the COMPONENT name
        // ("vrcam_2560x2560") used to classify owners at PatchCamera; the view key is the hash of
        // the CAMERA name ("vrcam_feed_2560x2560"). Different strings, so the test could never
        // fire -- MAIN moved to this path while VRCAM stayed on the old one, and two views driven
        // by two different mechanisms is what made the right eye judder.
        //
        // These two accessors are the ones the whole VRCAM capture pipeline already runs on, so
        // they are proven against the live dispatcher rather than reconstructed from a name.
        const bool isVrcam = CyberpunkVR_IsVrcamViewActive() != 0;
        const bool isMain  = !isVrcam && CyberpunkVR_IsMainViewActive() != 0;
        const bool isEye   = isMain || isVrcam;

        // EVERY VIEW IN THE IMAGE, not just the two eye views.
        //
        // Restricting the write to MAIN/VRCAM produced a very specific symptom: near geometry
        // stayed world-locked while distant geometry and shadows dragged with the head. That is
        // what it looks like when one part of the frame is rendered from the VR camera and the
        // rest from the engine's own -- the untouched views are composited into an image whose
        // main view has already turned, so their content appears to counter-rotate.
        //
        // These other views (distant/imposter, reflection, shadow) each carry their own key at
        // ctx+0x28 and sail straight through a two-key gate. They belong to the same eye and the
        // same instant, so they need the same orientation. Only the lateral IPD term is withheld
        // from them -- that one is per-eye, and a view whose eye we cannot name must not get it.
        if (isEye) {
            if (isMain) ++CyberpunkVR_DebugViewCamMain; else ++CyberpunkVR_DebugViewCamVrcam;
        } else {
            ++CyberpunkVR_DebugViewCamOther;
            if (CyberpunkVR_CamFinalViewScope == 0) return;
        }

        float hq[4] = { 0.0f, 0.0f, 0.0f, 1.0f };
        if (!g_headQuatValid || !cvr::camera::CamWriteQuatRead(hq) || !IsPlausibleUnitQuaternion(hq)) return;

        // One orientation for every view -- it is one head. The eyes differ by the lateral IPD
        // term and nothing else, the same rule all three reference mods keep.
        WriteRenderCameraBasis(rsiPtr, hq);

        const float half = isEye ? GetDesiredHalfIpd() : 0.0f;
        if (half != 0.0f) {
            float right[3] = {};
            ComputeRightVectorFromQuaternion(hq, right);
            if (IsPlausibleUnitVector3(right)) {
                const float sign = isVrcam ? +1.0f : -1.0f;   // MAIN = left, VRCAM = right
                int32_t* posFP = reinterpret_cast<int32_t*>(rsiPtr);
                for (int i = 0; i < 3; ++i) {
                    posFP[i] += static_cast<int32_t>(right[i] * half * sign * 131072.0f);
                }
            }
        }
        return;
    }

    // THE LATE IPD SHIFT IS GONE, AND IT WAS UNREACHABLE BEFORE IT WAS DELETED.
    //
    // It lived here: finalPos = g_lastLocatePosFP + g_lastIpdShiftFP, with the shift computed in
    // LocateCamera and the eye chosen by the PARITY of the locate counter. That is the AER shape --
    // one camera alternating eyes, so the offset had to flip per frame and had to be applied below
    // the view producer or consecutive frames would cull against different viewpoints.
    //
    // Two separate reasons it is not the arrangement any more:
    //
    //   * REACHABILITY. The block above returns unconditionally: `if (!CamWriteInFinal) return;`
    //     on the way in, and `return;` at the end of the write path. So this code could not
    //     execute at either setting of the flag -- it was dead in both directions, and the eye
    //     parity it used would in any case be wrong now that two cameras exist at once.
    //   * CORRECTNESS. With two real cameras the per-view offset is CONSTANT, so it belongs ABOVE
    //     the producer, in the component world position (CyberpunkVR_IpdInWorldPos, PatchCamera):
    //     there the distant/imposter pass, the shadow cascades, the reflections and the motion
    //     vectors are all built from the eye they are drawn for. Applied here they would not be.
    //
    // What remains of this hook is the read-back above, which is the one thing only this stage can
    // do: identify which written pose the frame is actually being drawn with.

    if (!g_verboseLog || (g_finalCameraHits % 600) != 1) return;

    float values[24] = {};
    float cameraMtx[16] = {};
    float cameraViewA[12] = {};
    float cameraViewB[16] = {};
    if (!ReadFloatArraySafe(rsiPtr, values, 24)) return;
    ReadFloatArraySafe(rsiPtr + 20, cameraMtx, 16);   // +0x50
    ReadFloatArraySafe(rsiPtr + 68, cameraViewA, 12); // +0x110
    ReadFloatArraySafe(rsiPtr + 204, cameraViewB, 16); // +0x330

    const int32_t* finalPosFP = reinterpret_cast<const int32_t*>(rsiPtr);
    const float posScale = 1.0f / 131072.0f;   // WorldPosition = 17 fractional bits

    Log("FinalCamera probe: hit=%llu rsi=%p eye=%d f40=%.6f f44=%.6f pos=(%.3f, %.3f, %.3f) locateSeq=%u locQ=(%.3f, %.3f, %.3f, %.3f)\n",
        static_cast<unsigned long long>(g_finalCameraHits),
        rsiPtr,
        OpenXRManager::Get().GetCurrentRenderEyeIndex(),
        values[16],
        values[17],
        static_cast<float>(finalPosFP[0]) * posScale,
        static_cast<float>(finalPosFP[1]) * posScale,
        static_cast<float>(finalPosFP[2]) * posScale,
        locateSeq,
        g_lastLocateQuat[0], g_lastLocateQuat[1], g_lastLocateQuat[2], g_lastLocateQuat[3]);

    if (LooksProjectionLike(values, 16)) {
        LogMatrix4x4("FinalCamera matrix candidate:", values);
    } else {
        Log("FinalCamera raw[0..15]: %.6f %.6f %.6f %.6f  %.6f %.6f %.6f %.6f  %.6f %.6f %.6f %.6f  %.6f %.6f %.6f %.6f\n",
            values[0], values[1], values[2], values[3],
            values[4], values[5], values[6], values[7],
            values[8], values[9], values[10], values[11],
            values[12], values[13], values[14], values[15]);
    }

    Log("FinalCamera mtx+0x50: %.6f %.6f %.6f %.6f  %.6f %.6f %.6f %.6f  %.6f %.6f %.6f %.6f  %.6f %.6f %.6f %.6f\n",
        cameraMtx[0], cameraMtx[1], cameraMtx[2], cameraMtx[3],
        cameraMtx[4], cameraMtx[5], cameraMtx[6], cameraMtx[7],
        cameraMtx[8], cameraMtx[9], cameraMtx[10], cameraMtx[11],
        cameraMtx[12], cameraMtx[13], cameraMtx[14], cameraMtx[15]);
    Log("FinalCamera view+0x110: %.6f %.6f %.6f %.6f  %.6f %.6f %.6f %.6f  %.6f %.6f %.6f %.6f\n",
        cameraViewA[0], cameraViewA[1], cameraViewA[2], cameraViewA[3],
        cameraViewA[4], cameraViewA[5], cameraViewA[6], cameraViewA[7],
        cameraViewA[8], cameraViewA[9], cameraViewA[10], cameraViewA[11]);
    Log("FinalCamera view+0x330: %.6f %.6f %.6f %.6f  %.6f %.6f %.6f %.6f  %.6f %.6f %.6f %.6f  %.6f %.6f %.6f %.6f\n",
        cameraViewB[0], cameraViewB[1], cameraViewB[2], cameraViewB[3],
        cameraViewB[4], cameraViewB[5], cameraViewB[6], cameraViewB[7],
        cameraViewB[8], cameraViewB[9], cameraViewB[10], cameraViewB[11],
        cameraViewB[12], cameraViewB[13], cameraViewB[14], cameraViewB[15]);
}

bool InstallFinalCameraHook() {
    const char* pattern = "\xF3\x44\x0F\x5E\x4E\x40\x49\x81\xC0\x20\x0F\x00\x00";
    const char* mask = "xxxxxxxxxxxxx";
    uint8_t* found = static_cast<uint8_t*>(FindPattern("Cyberpunk2077.exe", pattern, mask));
    if (!found) return false;

    constexpr int replaceLen = 13; 
    void* tramp = AllocateTrampoline(found, 512);
    if (!tramp) return false;

    uint8_t* code = static_cast<uint8_t*>(tramp);
    int pos = 0;

    // --- CALL C++ CALLBACK ---
    code[pos++] = 0x9C; // pushfq
    code[pos++] = 0x50; // push rax
    code[pos++] = 0x51; // push rcx
    code[pos++] = 0x52; // push rdx
    code[pos++] = 0x41; code[pos++] = 0x50; // push r8
    code[pos++] = 0x41; code[pos++] = 0x51; // push r9
    code[pos++] = 0x41; code[pos++] = 0x52; // push r10
    code[pos++] = 0x41; code[pos++] = 0x53; // push r11
    code[pos++] = 0x55; // push rbp

    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x40; // sub rsp, 40h
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x04; code[pos++] = 0x24; // movups [rsp+00h], xmm0
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10; // movups [rsp+10h], xmm1
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20; // movups [rsp+20h], xmm2
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30; // movups [rsp+30h], xmm3

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xE5; // mov rbp, rsp
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xE4; code[pos++] = 0xF0; // and rsp, -16
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x20; // sub rsp, 20h

    // Set arg1 (rcx) = rsi
    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xF1; // mov rcx, rsi

    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(OnFinalCameraCallback));
    code[pos++] = 0xFF; code[pos++] = 0xD0; // call rax

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xEC; // mov rsp, rbp

    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x04; code[pos++] = 0x24; // movups xmm0, [rsp+00h]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10; // movups xmm1, [rsp+10h]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20; // movups xmm2, [rsp+20h]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30; // movups xmm3, [rsp+30h]
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xC4; code[pos++] = 0x40; // add rsp, 40h

    code[pos++] = 0x5D; // pop rbp
    code[pos++] = 0x41; code[pos++] = 0x5B; // pop r11
    code[pos++] = 0x41; code[pos++] = 0x5A; // pop r10
    code[pos++] = 0x41; code[pos++] = 0x59; // pop r9
    code[pos++] = 0x41; code[pos++] = 0x58; // pop r8
    code[pos++] = 0x5A; // pop rdx
    code[pos++] = 0x59; // pop rcx
    code[pos++] = 0x58; // pop rax
    code[pos++] = 0x9D; // popfq

    // Original instructions:
    // divss xmm9, [rsi+40h]
    code[pos++] = 0xF3; code[pos++] = 0x44; code[pos++] = 0x0F; code[pos++] = 0x5E; code[pos++] = 0x4E; code[pos++] = 0x40;
    // add r8, 0F20h
    code[pos++] = 0x49; code[pos++] = 0x81; code[pos++] = 0xC0; code[pos++] = 0x20; code[pos++] = 0x0F; code[pos++] = 0x00; code[pos++] = 0x00;

    // jmp back
    code[pos++] = 0xE9;
    *reinterpret_cast<int32_t*>(code + pos) = static_cast<int32_t>((found + replaceLen) - (code + pos + 4));
    pos += 4;

    DWORD oldProtect;
    VirtualProtect(found, replaceLen, PAGE_EXECUTE_READWRITE, &oldProtect);
    found[0] = 0xE9;
    *reinterpret_cast<int32_t*>(found + 1) = static_cast<int32_t>(code - (found + 5));
    for (int i = 5; i < replaceLen; ++i) found[i] = 0x90;
    VirtualProtect(found, replaceLen, oldProtect, &oldProtect);
    FlushInstructionCache(GetCurrentProcess(), found, replaceLen);
    return true;
}

CVR_HOOK("FinalCamera", ::cvr::hooks::Stage::Boot, 14, InstallFinalCameraHook);
