// PatchCamera -- one hook, one file.
//
// Where the cameras are written. It consumes the heading, writes the camera state, and
// publishes two things back: the engine's pre-write quaternion (LocateCamera's base next
// frame -- the back edge of the only real cycle here) and the write ring entry that lets
// FinalCamera identify which XR sample the frame carries.
//
// The compare-exchange on the aim epoch is not decoration: the patched instruction is
// reached from several engine job threads, so "compose once per interval" needs it to mean
// once. Two threads composing in the same interval each publish a different pose as the
// frame's pose and the last to land wins at random.
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

extern "C" void __fastcall OnPatchCameraCallback(float* cameraState, void* ownerState) {
    g_patchCameraHits++;

    const int camKind = ClassifyPatchCameraOwner(ownerState);

    if (!cameraState || reinterpret_cast<uintptr_t>(cameraState) < 0x10000) return;

    float quat[4] = {};
    float posA[4] = {};
    float posB[4] = {};
    if (!ReadFloatArraySafe(cameraState + 0, quat, 4) ||
        !ReadFloatArraySafe(cameraState + 4, posA, 4) ||
        !ReadFloatArraySafe(cameraState + 8, posB, 4)) {
        return;
    }

    // A DEVICE CAMERA IS ORIENTATION ONLY. kind 3 is a surveillance camera bolted to a wall: the head
    // has to be able to look around from it, and the orientation write below is not gated on the kind so
    // it already does. What must NOT reach it is the head translation and the IPD split -- leaning would
    // drag the camera off its mount, and it is not one of the two eyes. Both of those are gated to kinds
    // 1 and 2 below, unchanged.
    //
    // NOTHING OF OURS GOES INTO A DEVICE CAMERA by default. It is read, not written: the orientation and
    // position below are the engine's own, which is exactly what makes them safe to hand to the second
    // eye -- there is no way for a composition to feed on its own output. The knob exists so the head
    // steering can be tried again from a build, not so it can be left on by accident.
    //
    // ONLY the two cameras we drive. Measured: this site fires ~16.3M times for ordinary
    // placed components against ~12k for the cameras, so an unfiltered write puts the head
    // pose into animated components and slots a thousand times more often than into a camera.
    // That is the "world slides, weapon drags with the head" failure at its source.
    if (camKind == 0) return;

    {
        const uint32_t tid = GetCurrentThreadId();
        if (tid != CyberpunkVR_DebugTidPatchCam) {
            CyberpunkVR_DebugTidPatchCam = tid;
            ++CyberpunkVR_DebugCamThreadSwitches;
        }
    }

    const uintptr_t owner = reinterpret_cast<uintptr_t>(ownerState);

    // ---- HEAD TRANSLATION into the SECOND view ---------------------------------------------
    //
    // MAIN gets it through the located camera buffer (`posFP += delta` in LocateCamera). VRCAM
    // has no equivalent, so it renders from its attachment point and stays welded to the head
    // while MAIN correctly moves away from it -- and the Tracking/Camera offsets, which live
    // inside the very same delta, never reached the second eye either.
    //
    // ADDED, not assigned. VRCAM's engine position is its own correct base, exactly as MAIN's
    // located position is MAIN's; what the two must share is the head displacement, and that is
    // what is shared here. Same structure as `view = base * head` in the Crysis / Far Cry mods
    // and `origin = setupOrigin + hmdPosRelative` in Portal 2 VR.
    static int32_t s_mainPosFP[3] = {};      // last MAIN world position, for the diagnostic below
    static int32_t s_vrcamPosFP[3] = {};
    static int32_t s_mainHeadFP[3] = {};     // and the head delta each view was patched against
    static int32_t s_vrcamHeadFP[3] = {};

    // Keyed off the CAMERA counter, never off the raw hit counter.
    //
    // This site fires ~196M times a session against ~54k camera writes, so a "% 600" on the raw
    // count is hundreds of formatted file writes per second, issued from engine job threads.
    // That is not a diagnostic, it is a stutter source of its own.
    if ((CyberpunkVR_DebugPatchCamMain % 900) == 1 && camKind == 1) {
        const float k = 1.0f / 131072.0f;
        // THE NUMBER THAT SAYS WHETHER THE TWO EYES ARE ALIGNED is `resid`, not `sep`.
        //
        // sep is the raw difference between the two cameras as each was last patched, and it
        // carries three things at once: the eye separation, the head displacement, and however
        // far the player moved between the two writes. A field log showed it swinging to 45 cm
        // while the player stood still, which says nothing about the stereo -- subtract the head
        // delta each view was actually patched against and what remains is the eye separation
        // alone. That must be ONE IPD, along the head's right vector, with essentially nothing
        // vertical: the eyes cannot fuse a vertical disparity at all, so residY is the number to
        // watch. |resid| should sit within a millimetre or two of ipd.
        // Subtract the head delta VRCAM was patched against, not the difference between the two
        // snapshots. Only VRCAM has it added at this site -- MAIN takes its own through
        // LocateCamera's buffer, and by the time these positions are stored MAIN's is already in
        // there. Differencing the snapshots therefore removes nothing and leaves the head
        // displacement sitting in the answer, which is what made the first field log read as a
        // 30 cm eye separation when the true one was 71 mm.
        const float rx = (s_vrcamPosFP[0] - s_mainPosFP[0] - s_vrcamHeadFP[0]) * k;
        const float ry = (s_vrcamPosFP[1] - s_mainPosFP[1] - s_vrcamHeadFP[1]) * k;
        const float rz = (s_vrcamPosFP[2] - s_mainPosFP[2] - s_vrcamHeadFP[2]) * k;
        Log("PatchCamera: main=%llu vrcam=%llu dev=%llu other=%llu | mainPos=(%.3f,%.3f,%.3f) "
            "vrcamPos=(%.3f,%.3f,%.3f) sep=(%.3f,%.3f,%.3f) headDelta=(%.3f,%.3f,%.3f) "
            "| resid=(%.4f,%.4f,%.4f) horiz=%.4f ipd=%.4f\n",
            static_cast<unsigned long long>(CyberpunkVR_DebugPatchCamMain),
            static_cast<unsigned long long>(CyberpunkVR_DebugPatchCamVrcam),
            static_cast<unsigned long long>(CyberpunkVR_DebugPatchCamDevice),
            static_cast<unsigned long long>(CyberpunkVR_DebugPatchCamOther),
            s_mainPosFP[0] * k, s_mainPosFP[1] * k, s_mainPosFP[2] * k,
            s_vrcamPosFP[0] * k, s_vrcamPosFP[1] * k, s_vrcamPosFP[2] * k,
            (s_vrcamPosFP[0] - s_mainPosFP[0]) * k,
            (s_vrcamPosFP[1] - s_mainPosFP[1]) * k,
            (s_vrcamPosFP[2] - s_mainPosFP[2]) * k,
            g_headDeltaFP[0].load(std::memory_order_relaxed) * k,
            g_headDeltaFP[1].load(std::memory_order_relaxed) * k,
            g_headDeltaFP[2].load(std::memory_order_relaxed) * k,
            // World is Z-up here, so the horizontal magnitude is the eye separation and rz alone
            // is the vertical disparity -- the one the eyes cannot fuse at all.
            rx, ry, rz, sqrtf(rx * rx + ry * ry),
            2.0f * GetDesiredHalfIpd());
    }

    // ---- ORIENTATION: the head pose, into BOTH cameras ------------------------------------
    //
    // This is what makes VRCAM track. LocateCamera composed heading * HMD for this frame and
    // published it; here it goes into the camera's own quaternion store, which is what the
    // rest of the frame reads. Both cameras get the SAME orientation -- the eyes differ by the
    // lateral IPD offset below, not by where they look.
    //
    // No feedback loop: the heading LocateCamera used comes from the game's body forward, not
    // from anything we wrote. Reading our own output back as a base is what once made the
    // camera spin up without bound.
    //
    // g_headQuatValid is 0 on the shot frame (and in native-aim mode), where the game's own
    // aim must drive the camera so the bullet follows the sights -- leave the engine's
    // orientation alone then.
    // Snapshot the engine's own orientation BEFORE overwriting it, from MAIN only -- that is
    // the camera whose heading the game actually drives. See g_engineCamQuat for why the
    // heading must not be read back out of the camera we write.
    // THE DEVICE CAMERA'S OWN AIM, taken before we write over it and kept for the whole takeover. This
    // is what makes the view start along the lens instead of wherever the player's body faced.
    // THE LENS AIM, REFRESHED EVERY FRAME THE ENGINE TOUCHES IT. A surveillance camera pans on its own
    // (security_camera_anim_controller), so a base latched once stops describing where it points -- and
    // re-reading blindly would read back our own write and compose onto it. The bitwise test against what
    // we wrote last is what separates the two: different means the engine refreshed it.
    const bool engineWroteDevCam =
        (camKind == 3) &&
        (quat[0] != g_devCamLastWritten[0] || quat[1] != g_devCamLastWritten[1] ||
         quat[2] != g_devCamLastWritten[2] || quat[3] != g_devCamLastWritten[3]);
    if (camKind == 3 && IsPlausibleUnitQuaternion(quat) && engineWroteDevCam) {
        g_devCamBase[0] = quat[0];
        g_devCamBase[1] = quat[1];
        g_devCamBase[2] = quat[2];
        g_devCamBase[3] = quat[3];
        // AND THE SAME AIM AS YAW + PITCH, which is the shape the composition below actually needs:
        // it builds R_z(yaw) * R_x(pitch) * HMD. Basis as everywhere in this file -- X right, Y forward,
        // Z up -- so the quaternion's SECOND column is the game's forward.
        const float fx = 2.0f * (quat[0] * quat[1] - quat[2] * quat[3]);
        const float fy = 1.0f - 2.0f * (quat[0] * quat[0] + quat[2] * quat[2]);
        const float fz = 2.0f * (quat[1] * quat[2] + quat[0] * quat[3]);
        const float fh = sqrtf(fx * fx + fy * fy);
        g_devCamAimYaw = atan2f(-fx, fy);
        g_devCamAimPitch = atan2f(fz, fh);
        g_devCamAimValid.store(1, std::memory_order_release);
        const bool first = (g_devCamBaseValid.exchange(1, std::memory_order_release) == 0);
        if (first) {
            Log("PatchCamera: device camera aim q=(%.3f,%.3f,%.3f,%.3f) yaw=%.2f pitch=%.2f deg\n",
                quat[0], quat[1], quat[2], quat[3],
                g_devCamAimYaw * 57.29578f, g_devCamAimPitch * 57.29578f);
        }
    }

    // THE FOV, the one thing a device camera never receives. Measured live: the player's own camera
    // reads fov=68.238 with GetFOV()=103.982, because the port forces the vertical the headset needs
    // (`NormalFOV: targetH=103.982 aspect=1.00000 (3072x3072) -> wroteV=103.982`), while a surveillance
    // camera reads 60 for BOTH -- so MAIN rendered the lens at 60 degrees against the second eye's 104.
    // Writing 103.982 into this field through the live bridge matched them at once, and GetFOV() followed
    // the field one for one, which is what says the override never reaches this camera by any route.
    //
    // +0x128 is the camera component's fov, and that is this project's own offset for it:
    // src/Stereo/FrameGraph.cpp reads the vrcam component's authored fov there and its log line prints
    // `fov=68.240`. Guarded by a plausibility test, so a layout change cannot write a nonsense value.
    if (camKind == 3 && owner >= 0x10000 && CyberpunkVR_DeviceCamOrient) {
        const float want = g_normalFovOverrideValue;
        float cur = 0.0f;
        if (ReadFloatSafe(owner + 0x128, &cur) && cur > 1.0f && cur < 179.0f &&
            want > 1.0f && want < 179.0f) {
            if (!g_devCamFovSaved.load(std::memory_order_acquire)) {
                g_devCamFovOrig = cur;
                g_devCamFovSaved.store(1, std::memory_order_release);
                Log("PatchCamera: device camera fov %.3f -> %.3f\n", cur, want);
            }
            if (fabsf(cur - want) > 0.01f) WriteFloatSafe(owner + 0x128, want);
        }
    }

    if (camKind == 1 && IsPlausibleUnitQuaternion(quat)) {
        g_engineCamQuat[0] = quat[0];
        g_engineCamQuat[1] = quat[1];
        g_engineCamQuat[2] = quat[2];
        g_engineCamQuat[3] = quat[3];
        g_engineCamQuatValid = 1;
    }

    float hq[4] = { 0.0f, 0.0f, 0.0f, 1.0f };
    bool haveWriteQuat = false;
    // The head position of THIS frame's sample, kept for the translation block below. See the
    // rebuild there for why the published delta cannot be used from this site.
    float freshHeadPos[3] = { 0.0f, 0.0f, 0.0f };
    bool  haveFreshHead = false;

    if (CyberpunkVR_CamWriteInPatch && CyberpunkVR_CamComposeAtWrite) {
        // g_headingValid is 0 on the shot frame and in native-aim mode: there the game's own
        // aim has to drive the camera so the bullet follows the sights. Leave the engine's
        // orientation standing, and do NOT fall back to the cached product -- reusing it would
        // re-apply the head pose on exactly the frames meant to be free of it.
        if (g_headingValid) {
            // GATE ON THE AIM EPOCH, NOT ON THE PRESENT COUNT.
            //
            // m_presentCount is incremented at the very TOP of OnPresent, but the aim time for
            // that interval is only published later, at the end of the same Present, after the
            // whole capture has run. A camera write landing in that window claimed the new
            // interval while GetFrameAimTime() still held the PREVIOUS one -- so it composed and
            // published a pose aimed a whole frame early, at random, a few times a second. The
            // epoch is bumped by SetFrameAimTime itself, so claiming it and reading the aim can
            // no longer disagree.
            const uint64_t epoch = OpenXRManager::Get().GetFrameAimEpoch();
            // `epoch == 0` means the frame loop has not published an aim yet (the window before
            // the XR path is pacing). Without this the once-per-epoch test would latch on the
            // very first write and never fire again -- a camera frozen at whatever pose the
            // game happened to start with, which looks exactly like the mod doing nothing.
            uint64_t claimed = g_camComposedForPresent.load(std::memory_order_acquire);
            const bool mine =
                (epoch == 0) ||
                (claimed != epoch &&
                 g_camComposedForPresent.compare_exchange_strong(
                     claimed, epoch, std::memory_order_acq_rel));
            if (mine) {
                // READ THE POSE HERE, FOR THIS FRAME -- do not take the cached atomics.
                //
                // GetHeadPose() is refreshed once per XR cycle by the frame-loop thread, so read
                // from the write site it is 0..24 ms old and the age WANDERS, because the XR loop
                // and the game's camera update free-run at nearly the same rate. Steady staleness
                // the compositor can reproject away; wandering staleness it cannot, and that is
                // the judder -- on both eyes at once, since both are written from this one
                // composition.
                //
                // So locate afresh, aimed at the display time predicted for the frame being
                // built. That is what RealVR does at the equivalent point:
                // locate_or_fake_headset_poses(seq) -> predicted = seq*period + base ->
                // xrLocateSpace(predicted). The prediction is the same rolling fit the submit
                // path already uses, measured at ~24.3 ms per frame.
                OpenXRHeadPose p{};
                bool got = false;
                if (CyberpunkVR_OneSamplePerFrame) {
                    // THE FRAME'S SAMPLE -- the very struct LocateCamera placed the eye with.
                    //
                    // Not a second locate of our own: that is how the orientation and the
                    // position ended up describing two different instants. Whoever of the two
                    // hooks runs first in this epoch performs the locate; both then read the
                    // same struct, and it is that struct which is handed to the submit below.
                    got = OpenXRManager::Get().AcquireFrameHeadSample(&p) && p.valid;
                    if (got) ++CyberpunkVR_DebugPoseLocatedAtWrite;
                } else if (CyberpunkVR_PoseLocateAtWrite) {
                    const XrTime aim = OpenXRManager::Get().GetFrameAimTime();
                    if (aim > 0) {
                        got = OpenXRManager::Get().LocateHeadPoseAt(aim, &p) && p.valid;
                        if (got) ++CyberpunkVR_DebugPoseLocatedAtWrite;
                    }
                }
                if (!got) {   // no aim yet, or the locate failed -- the cached value still works
                    got = OpenXRManager::Get().GetHeadPose(&p) && p.valid;
                    if (got) ++CyberpunkVR_DebugPoseFromCache;
                }
                if (got) {
                    // THE HEADING OF THIS FRAME, NOT OF THE PREVIOUS ONE.
                    //
                    // g_headingSy/Cy are published by LocateCamera, and LocateCamera runs INSIDE the
                    // blender -- which is downstream of this write. Measured order for one component:
                    // engine writes +0xE0/+0xF0, we write, THEN SerializeSetup reads. So composing
                    // from those cached halves used the heading of frame N-1 while the body was
                    // already turned to frame N's. Standing still that is invisible; a mouse turn at
                    // 300 deg/s is 4.2 deg per frame, and the camera lags the body by exactly that --
                    // which is the body judder on fast mouse turns, with VRIK and VRCAM both off, and
                    // it is absent in the unmodified game because there both come from one tick.
                    //
                    // The fresh heading is already in our hands: `quat` is the component's pre-write
                    // world rotation, assembled by UpdateWorldTransforms this pass as
                    // slotQuat * localQuat, and the slot comes from the parent the director has
                    // already turned. So take the heading from it.
                    //
                    // Basis note (same as LocateCamera): this camera space is X right, Y forward,
                    // Z up, so the quaternion's "up" column is the game's forward.
                    float hSy = g_headingSy, hCy = g_headingCy;
                    if (CyberpunkVR_HeadingFromPreWrite && IsPlausibleUnitQuaternion(quat)) {
                        const float fwdX = 2.0f * (quat[0] * quat[1] - quat[2] * quat[3]);
                        const float fwdY = 1.0f - 2.0f * (quat[0] * quat[0] + quat[2] * quat[2]);
                        float yaw = atan2f(-fwdX, fwdY);

                        // NO LEAD, NO KNOB -- and that is a measured conclusion, not a preference.
                        //
                        // The worry was that this heading belongs to the game TICK while the body is
                        // drawn from an entity transform interpolated per rendered frame, which would
                        // leave the camera a fraction of a turn behind the body. Measured, that
                        // premise is false: UpdateWorldTransforms runs once per RENDERED FRAME for
                        // every component (rebind census: 10203 calls/s over 179 components at 57
                        // presents/s, and PatchCamMain == presents exactly). So component+0xF0, which
                        // this quaternion comes from, is already assembled from the same render-rate
                        // entity the body is drawn with -- the two share an instant by construction
                        // and there is nothing to extrapolate.
                        //
                        // Kept as a census only: the step is the turn RATE (23.75 deg between two
                        // compositions at 57 fps is a 1350 deg/s flick), which is worth having in the
                        // log when reading any other symptom, and says nothing about phase.
                        static float s_prevYaw = 0.0f;
                        static bool  s_prevYawValid = false;
                        if (s_prevYawValid) {
                            float dYaw = yaw - s_prevYaw;
                            while (dYaw >  3.14159265f) dYaw -= 6.28318531f;
                            while (dYaw < -3.14159265f) dYaw += 6.28318531f;
                            const float degs = fabsf(dYaw) * 57.2957795f;
                            if (degs > CyberpunkVR_DebugHeadingStepDeg) {
                                CyberpunkVR_DebugHeadingStepDeg = degs;
                            }
                        }
                        s_prevYaw = yaw;
                        s_prevYawValid = true;

                        // THE VIEW'S YAW IS THE ENGINE'S, NOT THE BODY'S -- and that one line is what
                        // lets the character turn WITHOUT the camera turning.
                        //
                        // The quaternion above is the component's, and the component inherits its yaw
                        // from the entity through the parent slot (traced live: sub_1401D92A0 hands
                        // over parent+0xE0/+0xF0, and the camera's own local rotation is never
                        // written). So the moment the body follows the headset, that yaw carries our
                        // follow offset and the view swings with it. Nothing about the view requires
                        // that: WE compose the camera, so its yaw can come from whatever we choose.
                        //
                        // It comes from the value the engine itself computed for this frame --
                        // published by the store site sub_140336390 before our offset is added
                        // (src/Hooks/BodyYawCensus.cpp). That is the yaw the MOUSE and the stick
                        // produce, so input still turns the view exactly as before, while the body's
                        // extra rotation is invisible to it.
                        //
                        // Two things fall out for free. The base-yaw counter-rotation is no longer
                        // needed (the view never sees the offset, so there is nothing to cancel, and
                        // the HMD-local hands stay untouched), and this value is 0.22-0.79 ms old
                        // instead of a frame old -- which is the 5-10 deg the [yawphase] census
                        // measured between the packet heading and the yaw actually in use.
                        // ON FOOT ONLY, and that is measured rather than cautious. The census reads
                        // the PLAYER'S state transform; mounted, that transform stops describing where
                        // the view looks -- [vehyaw] caught it flat at 0.0 deg while the car swung
                        // through 260, against a camera heading that tracked the car to 1.3 deg. Left
                        // in, it pins the driving view ~90 deg off the road, in a direction the player
                        // cannot predict and cannot recenter away (recentring zeroes the HMD against
                        // its base; this sits on the other side of that product).
                        if (CyberpunkVR_ViewYawFromEngine && CyberpunkVR_EngineBodyYawValid
                            && !g_isInVehicle) {
                            float wz = CyberpunkVR_EngineBodyYawZ;
                            float ww = CyberpunkVR_EngineBodyYawW;
                            if (ww < 0.0f) { wz = -wz; ww = -ww; }   // shortest arc
                            if (wz != 0.0f || ww != 0.0f) {
                                yaw = 2.0f * atan2f(wz, ww);
                                ++CyberpunkVR_DebugViewYawFromEngine;
                            }
                        }
                        // PHYSICAL BODY ROTATION: TAKE OUR OWN TURN BACK OUT OF THE VIEW.
                        //
                        // The body follower turns the character through the engine's own heading (see
                        // src/Hooks/BodyYawFollow.cpp), and that heading is what this composition is
                        // built from -- so without this line every degree the body gains would swing
                        // the view with it. Subtracting the accumulated realign leaves the view
                        // exactly where it would have been had the body never turned, in the SAME
                        // frame the heading changed.
                        //
                        // This is where the cancellation belongs. Doing it by rotating the recenter
                        // base is algebraically identical but lands a frame late -- the heading moves
                        // in the game tick, the base only on the next XR cycle -- and that one frame
                        // of uncancelled step is the camera drift the old on-foot realign had.
                        // Leaving the base alone also means recentring is untouched by the feature.
                        yaw -= CyberpunkVR_BodyYawRealignRad;
                        // PUBLISHED AT THE INSTANT IT IS USED, so the play-space anchor can be rotated by
                        // the very same number instead of by the body's own forward. Those were two clocks
                        // -- this one is assembled per rendered frame, the body's advances on the entity
                        // tick -- and while a car turns the difference between them is the jitter: the eye
                        // ends up rotated by however much further the car got than the yaw did. Straight
                        // line or parked, there is no difference to accumulate, which is exactly where the
                        // symptom was absent.
                        g_viewYawUsedRad = yaw;
                        g_viewYawUsedValid = 1;

                        hSy = sinf(yaw * 0.5f);
                        hCy = cosf(yaw * 0.5f);
                    }
                    // Same axis mapping LocateCamera uses: XR (x, y, z) -> game (x, -z, y). And the
                    // same yaw * pitch * HMD product it composes -- the pitch half is identity
                    // unless the user turned "Disable Mouse Y" off.
                    float headX, headY, headZ, headW;
                    MulQuat(0.0f, 0.0f, hSy, hCy,
                            g_headingPitchS, 0.0f, 0.0f, g_headingPitchC,
                            headX, headY, headZ, headW);
                    // A DEVICE CAMERA HAS NO BODY HEADING, AND NEITHER HAS THE FRAME IT IS RENDERED IN.
                    //
                    // The product above is built from the player's body: the pre-write heading is right
                    // for a device camera, but ViewYawFromEngine then substitutes the BODY's yaw and
                    // BodyYawRealign takes the body follower's own turn back out. Both are meaningless on
                    // a mount, and substituting where the player's body faces is exactly "a wall".
                    //
                    // APPLIED TO EVERY CAMERA KIND, not just to the device camera, and that is the part
                    // that stops the blinking. Only ONE camera composes per epoch and the others read its
                    // published product, so while the base differed per kind whichever camera claimed the
                    // epoch decided the base for all three -- body-based one frame, lens-based the next.
                    // One base leaves the race nothing to decide.
                    if (CyberpunkVR_DeviceCamOrient && DeviceCamActive() &&
                        g_devCamAimValid.load(std::memory_order_acquire)) {
                        // THE LENS YAW, AND THE PITCH HALF THAT MAIN USES -- not the mount's own pitch.
                        // The mount is 9.4 degrees nose-down, and composing that in started the view
                        // tilted with the head's pitch adding on top of it: "смотрю не ровно по
                        // горизонту". The mount's pitch is still measured and logged, it is simply not
                        // part of the base, so this is structurally MAIN's composition with the body
                        // yaw swapped for the lens yaw and the horizon level.
                        const float ly = g_devCamAimYaw * 0.5f;
                        MulQuat(0.0f, 0.0f, sinf(ly), cosf(ly),
                                g_headingPitchS, 0.0f, 0.0f, g_headingPitchC,
                                headX, headY, headZ, headW);
                    }
                    float rx, ry, rz, rw;
                    MulQuat(headX, headY, headZ, headW,
                            p.oriX, -p.oriZ, p.oriY, p.oriW, rx, ry, rz, rw);
                    NormalizeQuat(rx, ry, rz, rw);
                    cvr::camera::CamWriteQuatPublish(rx, ry, rz, rw);
                    {   // file it so the render side can recognise this exact frame later
                        const float qr[4] = { rx, ry, rz, rw };
                        cvr::camera::CamWriteRecordPush(qr, p);
                    }
                    ++CyberpunkVR_DebugCamComposed;
                    if (camKind == 2) ++CyberpunkVR_DebugCamVrcamFirst;

                    // THE pose that is in the image, published at the instant it goes into the
                    // camera. Not before, not from another hook: the submit path labels the
                    // captured frame with this, and the compositor's reprojection is only
                    // correct when the label is the rotation actually baked into the pixels.
                    OpenXRManager::Get().PushRenderHeadPose(p);
                    freshHeadPos[0] = p.posX;
                    freshHeadPos[1] = p.posY;
                    freshHeadPos[2] = p.posZ;
                    haveFreshHead = true;
                    // Keep the published composition in step for the overlay crosshair and the
                    // legacy readers, so there is only ever one current answer.
                    g_headQuatComposed[0] = rx;
                    g_headQuatComposed[1] = ry;
                    g_headQuatComposed[2] = rz;
                    g_headQuatComposed[3] = rw;
                    g_headQuatValid = 1;
                } else {
                    ++CyberpunkVR_DebugCamNoHmd;
                }
            }
            haveWriteQuat = cvr::camera::CamWriteQuatRead(hq);

            // THIS FRAME'S WORLD YAW, ON A FRAME THAT DID NOT COMPOSE.
            //
            // Measured as an identity, not a guess: over ten windows of driving, the MAIN writes that
            // repeated the previous orientation bit-for-bit numbered exactly the shortfall in distinct aim
            // epochs (15/105, 13/107, 9/111, 8/112, 6/114, 5/115...). A frame sharing an epoch with the
            // one before it reads the published product back unchanged -- correct for the head, which has
            // not moved, and stale for the WORLD, which has: the car turned further. The view holds a
            // frame and then catches up, which is why it only shows while turning.
            //
            // Exact, because the composition is Rz(yaw) * Rx(pitch) * HMD, so replacing the yaw is a left
            // multiplication and nothing else: Rz(yaw + d) * Rx(pitch) * HMD == Rz(d) * hq.
            //
            // The head sample is untouched -- shared between the eyes, which is the invariant this epoch
            // machinery exists for -- and so is the pose label, which describes the head and not the
            // world, so the compositor's reprojection is unaffected.
            //
            // SCOPED TO A VEHICLE, AND THAT IS A CORRECTNESS BOUND, NOT CAUTION. `d` is the difference
            // between THIS camera's own pre-write yaw and the yaw the product was composed with, so it is
            // only meaningful where the composition's base IS this camera's own pre-write yaw:
            //
            //   in a vehicle   ViewYawFromEngine is gated off (!g_isInVehicle), no device camera is
            //                  involved, so the base is exactly this camera's pre-write yaw. Correct.
            //   on foot        the base is SUBSTITUTED with the engine body yaw, which differs from the
            //                  camera's own by the 5-10 deg the [yawphase] census measured. d would be
            //                  that difference, not the missed rotation.
            //   in a device    the base is the LENS yaw, while VRCAM's own component still carries the
            //   camera         PLAYER's heading -- so d is the whole angle between the two and it threw
            //                  one eye off entirely. Reported as "один смотрит в другое, другой в другое".
            if (haveWriteQuat && !mine && g_viewYawUsedValid &&
                g_isInVehicle && !DeviceCamActive() &&
                CyberpunkVR_HeadingFromPreWrite &&
                IsPlausibleUnitQuaternion(quat) && CyberpunkVR_YawCatchUpOnSharedEpoch) {
                const float fx = 2.0f * (quat[0] * quat[1] - quat[2] * quat[3]);
                const float fy = 1.0f - 2.0f * (quat[0] * quat[0] + quat[2] * quat[2]);
                if (fx * fx + fy * fy > 1.0e-6f) {
                    float d = atan2f(-fx, fy) - g_viewYawUsedRad;
                    while (d >  3.14159265f) d -= 6.28318531f;
                    while (d < -3.14159265f) d += 6.28318531f;
                    if (d > 1.0e-6f || d < -1.0e-6f) {
                        const float sh = sinf(d * 0.5f), ch = cosf(d * 0.5f);
                        float rx2, ry2, rz2, rw2;
                        MulQuat(0.0f, 0.0f, sh, ch, hq[0], hq[1], hq[2], hq[3], rx2, ry2, rz2, rw2);
                        NormalizeQuat(rx2, ry2, rz2, rw2);
                        hq[0] = rx2; hq[1] = ry2; hq[2] = rz2; hq[3] = rw2;
                        ++CyberpunkVR_DebugYawCaughtUp;
                    }
                }
            }
        }
    } else if (CyberpunkVR_CamWriteInPatch && g_headQuatValid) {
        hq[0] = g_headQuatComposed[0];
        hq[1] = g_headQuatComposed[1];
        hq[2] = g_headQuatComposed[2];
        hq[3] = g_headQuatComposed[3];
        haveWriteQuat = true;
    }

    // THE SECOND EYE TAKES THE CAMERA'S OWN AIM, not a head-composed one, so both eyes look the same way
    // -- one eye steering while the other does not is worse than neither steering. Replaces the composed
    // quaternion outright, and the IPD right-vector below is then computed from the aim actually written.
    if (camKind == 2 && DeviceCamActive()) {
        // WITH THE HEAD STEERING ON, VRCAM COMPOSES THROUGH THE ORDINARY MACHINERY and this block does
        // nothing. Handing it g_devCamViewQuat -- the quaternion the device camera was written with -- was
        // the VRCAM judder: when VRCAM is patched BEFORE the device camera in a frame that value is a
        // frame old, and a frame-old head rotation is precisely what the compositor cannot reproject
        // away. The epoch machinery already gives both eyes one composition from one head sample, and now
        // that the base is shared it is the right composition for both.
        //
        // With the steering off this still stands in: both eyes then simply look along the lens.
        const float* src = nullptr;
        if (!CyberpunkVR_DeviceCamOrient && g_devCamBaseValid.load(std::memory_order_acquire)) {
            src = g_devCamBase;
        }
        if (src) {
            hq[0] = src[0]; hq[1] = src[1]; hq[2] = src[2]; hq[3] = src[3];
            haveWriteQuat = true;
        }
    }
    // TEST LEVER: LEAVE THE VEHICLE CAMERA'S ORIENTATION ALONE.
    //
    // The measurement could not separate 'the engine re-imposes its bound forward and fights our
    // write' from 'the engine reassembles the transform every pass, as it does on foot too' -- the
    // residual it reported is the size of our own head offset either way. This settles it by removing
    // our write instead of reasoning about it: with 0 the head no longer turns the view in a car, so
    // it is a test and not a setting, but if the jitter goes with it the fight is ours, and if the
    // jitter stays the orientation was never the thing moving.
    if (g_isInVehicle && CyberpunkVR_CamWriteOrientInVehicle == 0) haveWriteQuat = false;

    // A device camera is left exactly as the engine wrote it unless the head steering is on.
    if (camKind == 3 && !CyberpunkVR_DeviceCamOrient) haveWriteQuat = false;

    if (haveWriteQuat && IsPlausibleUnitQuaternion(hq)) {
        const uintptr_t q = reinterpret_cast<uintptr_t>(cameraState);
        WriteFloatSafe(q + 0x00, hq[0]);
        WriteFloatSafe(q + 0x04, hq[1]);
        WriteFloatSafe(q + 0x08, hq[2]);
        WriteFloatSafe(q + 0x0C, hq[3]);
        // The IPD shift below needs the RIGHT vector of the orientation actually being
        // rendered, so recompute it from what we just wrote rather than from the engine's
        // pre-write value.
        // [vehhold] DO WE WRITE THE SAME ORIENTATION TWICE IN A ROW?
        //
        // Everything else is ruled out by measurement: the jitter needs our write (a build without it was
        // clean), it is not the game's heading reset or rubber band (disabled live, no change), not the
        // camera mixer (the three other cameras enabled while mounted were disabled live, all at once, no
        // change), and not the frame rate (it is visible on the flat monitor, which carries no
        // reprojection).
        //
        // What remains: the composition runs once per aim EPOCH and is published, and every camera patched
        // inside that epoch writes the published value. Two rendered frames inside one epoch means the
        // second writes the identical quaternion -- the view holds while the car keeps turning, then
        // catches up. Invisible parked, invisible in a straight line, growing with the turn rate, and in
        // the game's own image.
        //
        // MAIN only, so a bitwise compare of consecutive writes cannot be polluted by the second camera
        // writing the same value in the same frame.
        if (g_isInVehicle && camKind == 1) {
            static float s_prev[4] = {};
            static bool  s_have = false;
            static int   s_same = 0, s_n = 0;
            static uint64_t s_prevEpoch = 0;
            static int   s_epochs = 0;
            if (s_have) {
                if (hq[0] == s_prev[0] && hq[1] == s_prev[1] &&
                    hq[2] == s_prev[2] && hq[3] == s_prev[3]) ++s_same;
                ++s_n;
            }
            for (int i = 0; i < 4; ++i) s_prev[i] = hq[i];
            s_have = true;
            const uint64_t ep = OpenXRManager::Get().GetFrameAimEpoch();
            if (ep != s_prevEpoch) { s_prevEpoch = ep; ++s_epochs; }
            if (s_n >= 120) {
                Log("[vehhold] of 120 MAIN writes: %d repeated the previous orientation bit-for-bit "
                    "(%.0f%%), distinct aim epochs spanned %d\n",
                    s_same, 100.0 * static_cast<double>(s_same) / 120.0, s_epochs);
                s_same = 0; s_n = 0; s_epochs = 0;
            }
        }

        quat[0] = hq[0]; quat[1] = hq[1]; quat[2] = hq[2]; quat[3] = hq[3];
        if (camKind == 3) {
            // Remembered so the next frame can tell the engine's value from ours, and published so the
            // second eye renders the same direction rather than its own composition.
            for (int i = 0; i < 4; ++i) {
                g_devCamLastWritten[i] = hq[i];
                g_devCamViewQuat[i] = hq[i];
            }
            g_devCamViewValid.store(1, std::memory_order_release);
        }
    }

    // ---- WORLD POSITION: head translation, and the EYE SEPARATION -----------------------------
    //
    // Runs AFTER the orientation write on purpose: the lateral offset has to be taken along the
    // RIGHT vector of the orientation this camera is actually going to render with.
    //
    // WHY THE EYE SEPARATION BELONGS HERE AND NOT IN A LATE SHIFT
    //
    // It was not applied anywhere at all. The old code wrote it into `cameraState + 0x10/0x20`
    // (component + 0x100/0x110, the "posA/posB" pair), which is not the world position the view
    // producer reads -- that is +0xE0, twenty lines up, where the head translation already goes.
    // Measured: the census printed `sep` exactly equal to `headDelta` to three decimals, with no
    // trace of the 3.25 cm half-IPD, and a live breakpoint found the two render cameras 23
    // MICROMETRES apart. So both eyes were rendering from one point and the only stereo was the
    // per-eye offset in the submitted label -- two identical images pushed apart, i.e. a window
    // rather than depth.
    //
    // The other candidate was the late shift at FinalCamera. It exists for AER, where ONE camera
    // alternates eyes and the offset therefore flips every frame -- there it must be applied
    // below the producer or every frame's culling would disagree with the previous one. We have
    // two real cameras and a CONSTANT offset per view, so that constraint is gone, and writing
    // before the producer is strictly better: the distant/imposter pass, the shadow cascades, the
    // reflections and the motion vectors are all built from the same point the image is drawn
    // from. Nothing downstream can disagree, because nothing downstream sees a different camera.
    // It is also the shape all three reference mods use -- `view = base +- right*ipd/2` at the
    // camera, not as a fix-up afterwards.
    if (owner >= 0x10000) {
        const uintptr_t posAddr = owner + 0xE0;
        int32_t p[3] = {};
        bool ok = true;
        for (int i = 0; i < 3 && ok; ++i) {
            uint32_t v = 0;
            ok = ReadU32Safe(posAddr + i * 4, &v);
            p[i] = static_cast<int32_t>(v);
        }
        bool dirty = false;

        // THE LENS POSITION, recorded from the camera itself and then given to the second eye. VRCAM's
        // component is parented to the player, so with MAIN moved to a surveillance camera the two eyes
        // were metres apart -- measured as 2.4 m in the field log. +0xE0 is the world position the view
        // producer reads (the note below this block says why the posA/posB pair is not), which is why
        // the copy belongs exactly here, before the head translation and the IPD split.
        if (camKind == 3) {
            for (int i = 0; i < 3; ++i) g_devCamPosFP[i].store(p[i], std::memory_order_relaxed);
            // SET, NEVER CLEARED HERE. A failed read is a reason to keep the last known lens position,
            // not a reason to drop the second eye back onto the player. Cleared on release only.
            if (ok) g_devCamPosValid.store(1, std::memory_order_release);
        } else if (camKind == 2 && ok && DeviceCamActive() &&
                   g_devCamPosValid.load(std::memory_order_acquire)) {
            for (int i = 0; i < 3; ++i) p[i] = g_devCamPosFP[i].load(std::memory_order_relaxed);
            dirty = true;
        }

        // THE MOUNT SWEEP, CANCELLED WITHOUT EVER READING BACK WHAT WE WROTE.
        //
        // The FPP camera is not at the character's origin; it hangs off it, so the heading the body
        // follower turns sweeps the camera along a circle of that radius. The view keeps its aim (the
        // yaw is cancelled where it is composed) but slides sideways, and the avatar's head, placed
        // under the view, goes with it -- "either the body or the head is not in the same place after
        // the turn".
        //
        // WHY THE OBVIOUS VERSION BROKE THE GAME. The first attempt rotated the position it had just
        // read: p' = entity + Rz(-realign) * (p - entity). This site fires several passes per frame for
        // the same camera, and that form is only correct if the engine rewrites +0xE0 between passes --
        // if it does not, pass two rotates an already-rotated value and the camera spirals off the
        // character. That is exactly what was reported: the character appeared to fall through the
        // ground and the view started turning again.
        //
        // So the correction is a pure FUNCTION of angles and a LEARNED mount vector, never of the
        // current position. Applying it twice adds the same vector twice at worst, and it is written to
        // be applied once per pass alongside the head displacement, which the engine's own recompute
        // already absorbs the same way:
        //
        //     mount (body frame)  m = Rz(-heading) * (p - entity)      learned only while realign == 0
        //     correction          Rz(heading - realign) * m - Rz(heading) * m
        //
        // Horizontal only: a yaw sweep has no vertical component, and the camera's own height (crouch,
        // lean, cover) belongs to the engine.
        // The learning half runs even with the compensation off: that is how the radius gets
        // measured, and the radius is what decides whether the correction is worth having.
        // NOT WHILE LOOKING THROUGH A DEVICE CAMERA. Everything in this block is about the PLAYER's
        // camera hanging off the PLAYER's body, and during a takeover neither the second eye nor the
        // device camera has anything to do with that body -- both sit at the lens.
        //
        // The CORRECTION was the remaining jerk: its shift depends on the body follower's accumulated
        // turn, that angle steps when the follower turns the body, and kind 2 and kind 3 are patched at
        // different instants -- so a step between the two writes gives one eye the new shift and the
        // other the old one. A lateral disparity of up to twice the mount radius, in ONE eye.
        //
        // The LEARNING half was a silent corruption: it learns the mount as (p - playerEntity), and while
        // a takeover is live that is the distance from the player to a camera on a wall. Through its 0.05
        // EMA it poisons the radius MAIN uses after the player has left the camera.
        //
        // kind 1 stays in: the player's own camera is still patched here and its position is still its
        // own, so learning from it remains valid throughout.
        const bool bodyMountApplies = !(DeviceCamActive() && (camKind == 2 || camKind == 3));
        if (ok && bodyMountApplies && CyberpunkVR_PlayerEntityValid && CyberpunkVR_BodyYawFinalValid) {
            static float s_mountX = 0.0f, s_mountY = 0.0f;
            static bool  s_mountLearned = false;
            const float kFp = 1.0f / 131072.0f;
            const float H = CyberpunkVR_BodyYawFinalRad;
            const float A = CyberpunkVR_BodyYawRealignRad;
            const float rx = static_cast<float>(p[0]) * kFp - CyberpunkVR_PlayerEntityPos[0];
            const float ry = static_cast<float>(p[1]) * kFp - CyberpunkVR_PlayerEntityPos[1];
            if (A == 0.0f) {
                // Learn the mount in the BODY frame, where it is a slowly-varying property of the
                // character rather than of the heading. Only while the follower has nothing applied --
                // that is the one state in which p is known to be un-corrected.
                const float cH = cosf(-H), sH = sinf(-H);
                const float mx = rx * cH - ry * sH;
                const float my = rx * sH + ry * cH;
                if (!s_mountLearned) { s_mountX = mx; s_mountY = my; s_mountLearned = true; }
                else {
                    s_mountX += (mx - s_mountX) * 0.05f;
                    s_mountY += (my - s_mountY) * 0.05f;
                }
                CyberpunkVR_DebugCamMountM = sqrtf(s_mountX * s_mountX + s_mountY * s_mountY);
            } else if (s_mountLearned && CyberpunkVR_CamMountCompensate) {
                const float c0 = cosf(H - A), s0 = sinf(H - A);
                const float c1 = cosf(H),     s1 = sinf(H);
                const float dx = (s_mountX * c0 - s_mountY * s0) - (s_mountX * c1 - s_mountY * s1);
                const float dy = (s_mountX * s0 + s_mountY * c0) - (s_mountX * s1 + s_mountY * c1);
                p[0] += static_cast<int32_t>(dx * 131072.0f);
                p[1] += static_cast<int32_t>(dy * 131072.0f);
                dirty = true;
            }
        }

        // HEAD TRANSLATION, NOW THE SAME WAY FOR BOTH CAMERAS.
        //
        // VRCAM always got it here, in the component. MAIN got it in LocateCamera, by adding the
        // delta into the serialised CameraSetup buffer -- and that is a different pipeline stage
        // with two consequences the trace made explicit:
        //
        //   * the buffer entry is WEIGHTED. The blender multiplies every field by the camera's
        //     blend weight (entry+0x90) before summing into director+0x4C0, so on any camera
        //     transition -- entering a vehicle, a cinematic, anything with two active cameras --
        //     MAIN's head translation was scaled by that weight while its ORIENTATION, which
        //     travels through the component, was not. Position and aim disagreed exactly when the
        //     weight left 1.0.
        //   * the two eyes reached the blender by different routes, so nothing structural
        //     guaranteed they carried the same displacement.
        //
        // Writing the component for MAIN too puts both cameras and both channels on one path:
        // component+0xE0 is what SerializeSetup reads (proven: it reads [rcx-0x40] = obj+0xE0),
        // so the delta now goes through the weighting exactly like the rest of the transform.
        //
        // No accumulation risk: the engine recomputes +0xE0 from slot+local every pass and
        // OVERWRITES it, and our add lands after that write -- the same reason VRCAM has been
        // stable here all along.
        // Both cameras, one route. MAIN is gated separately from VRCAM only so either half can be
        // switched off live for a comparison -- the mechanism is the same write.
        // NO HEAD TRANSLATION FOR A DEVICE CAMERA OR FOR THE EYE THAT SITS ON IT, and that is a
        // measurement rather than a principle. Switched on for both, it made MAIN judder as well
        // ("сейчас дергает translation и main тоже"), so the premise was wrong: the delta is rebuilt from
        // the play-space anchor RECIPE, which expresses a displacement about the PLAYER's anchor, and
        // about a bracket on a wall that is not the same quantity. Reverted rather than tuned -- guessing
        // at the transform bought a second regression, not a fix.
        const bool wantMainHere  = (camKind == 1) && (CyberpunkVR_HeadTranslationInPatch != 0);
        const bool wantVrcamHere = (camKind == 2) && (CyberpunkVR_VrcamHeadTranslation != 0) &&
                                   !DeviceCamActive();
        if (ok && (wantMainHere || wantVrcamHere) &&
            g_headDeltaValid.load(std::memory_order_acquire)) {
            // THE DELTA IS REBUILT HERE, NOT READ. g_headDeltaFP is computed by LocateCamera, and
            // LocateCamera runs AFTER this write (measured: engine writes +0xE0, we write, then
            // SerializeSetup reads). So the published vector belongs to frame N-1, and using it
            // moves the camera to where the head was a frame ago -- a trail on every head motion,
            // and the same defect the heading had until it was taken from the pre-write quaternion.
            //
            // The fix is the mechanism LocateCamera already provides for the hand publish: it
            // publishes the RECIPE (g_anchorOff, g_anchorCy/Sy, g_anchorScale) precisely so a
            // consumer can rebuild this delta from ITS OWN head sample. The recipe is made of slow
            // values -- bakes, sliders, the level heading -- so a frame of age in them is
            // immaterial; the fast term is the head position, and that we have fresh.
            int32_t d[3] = { g_headDeltaFP[0].load(std::memory_order_relaxed),
                             g_headDeltaFP[1].load(std::memory_order_relaxed),
                             g_headDeltaFP[2].load(std::memory_order_relaxed) };
            if (CyberpunkVR_DeltaFromFreshSample && haveFreshHead && g_anchorRecipeValid) {
                const float sc = g_anchorScale;
                const float localRight   =  freshHeadPos[0] * sc + g_anchorOff[0];
                const float localForward = -freshHeadPos[2] * sc + g_anchorOff[1];
                const float localUp      =  freshHeadPos[1] * sc + g_anchorOff[2];
                const float wx = g_anchorCy * localRight - g_anchorSy * localForward;
                const float wy = g_anchorSy * localRight + g_anchorCy * localForward;
                d[0] = static_cast<int32_t>(wx * 131072.0f);
                d[1] = static_cast<int32_t>(wy * 131072.0f);
                d[2] = static_cast<int32_t>(localUp * 131072.0f);
                ++CyberpunkVR_DebugDeltaRebuilt;
            }
            // THE ENGINE'S OWN CAMERA POSITION, PUBLISHED BEFORE WE MOVE IT.
            //
            // The pose path needs this and must not reconstruct it. Rebuilding it as (view - head
            // delta) puts the player's PHYSICAL head motion back into the value with the opposite
            // sign -- move your head right and the reconstructed camera goes left -- so the avatar's
            // body, which hangs off it, follows the head instead of standing still. Measured that way:
            // camModelPos.z came out 1.21 m where the game's camera sits at ~1.6, because the delta
            // carries the physical height too and the squat was then subtracted a second time.
            //
            // Published for MAIN only: it is the gameplay camera, and VRCAM is the same transform one
            // eye over.
            if (camKind == 1) {
                CyberpunkVR_EngineCamPosFP[0] = p[0];
                CyberpunkVR_EngineCamPosFP[1] = p[1];
                CyberpunkVR_EngineCamPosFP[2] = p[2];
                CyberpunkVR_EngineCamPosValid = 1;
            }
            for (int i = 0; i < 3; ++i) p[i] += d[i];
            dirty = true;
            if (camKind == 2) ++CyberpunkVR_DebugVrcamPosWrites;
            else              ++CyberpunkVR_DebugMainPosWrites;
        }

        // Eye separation, symmetric about the head: MAIN is the left eye, VRCAM the right.
        // Symmetric and not "VRCAM only" because the submitted label places the eyes at
        // +-half about the head centre; putting the whole offset on one camera would slide the
        // entire scene sideways by half an IPD relative to that label.
        // ALONG THE CAMERA'S RIGHT, NEVER ALONG THE WORLD'S X. This used to take the right vector from
        // `hq`, which is initialised to IDENTITY and only filled in when a composition happened -- and
        // IsPlausibleUnitQuaternion(identity) is true, so on any frame where nothing composed
        // (g_headingValid is 0 on the shot frame and in native-aim mode) the separation was laid along
        // world X. The two eyes take opposite signs, so that is up to a whole IPD of disparity in the
        // wrong direction for one frame -- a lateral jerk in one eye and nothing in the other.
        //
        // `quat` is the right source unconditionally: it holds what was just written when there was a
        // write, and the engine's own orientation when there was not.
        if (ok && CyberpunkVR_IpdInWorldPos && IsPlausibleUnitQuaternion(quat)) {
            const float half = GetDesiredHalfIpd();
            if (half != 0.0f) {
                float r[3] = {};
                ComputeRightVectorFromQuaternion(quat, r);
                if (IsPlausibleUnitVector3(r)) {
                    // MAIN is the left eye by default; with the swap it becomes the right one,
                    // so the separation has to change hands too or each eye gets the other's
                    // viewpoint -- pseudo-stereo, which reads as depth turned inside out.
                    const float sgn0 = (camKind == 2) ? +1.0f : -1.0f;
                    const float sign = CyberpunkVR_MainIsRightEye ? -sgn0 : sgn0;
                    for (int i = 0; i < 3; ++i) {
                        p[i] += static_cast<int32_t>(r[i] * half * sign * 131072.0f);
                    }
                    dirty = true;
                    ++CyberpunkVR_DebugIpdWorldWrites;
                }
            }
        }

        if (ok && dirty) {
            for (int i = 0; i < 3; ++i) WriteU32Safe(posAddr + i * 4, static_cast<uint32_t>(p[i]));
        }
        // Diagnostic snapshot AFTER the writes, so `sep` shows what the views really differ by:
        // it must come out as headDelta plus a full IPD along the right vector.
        // Snapshot the head delta AS IT STOOD for this view, not just the position. Without it the
        // sep figure below cannot be read at all: the two views are patched at different instants
        // and MAIN takes its head translation through LocateCamera's own buffer rather than here,
        // so sep mixes the eye separation, the head displacement and the time between the two
        // writes into one number. Recorded per view, the difference can be removed and what is
        // left is the thing that actually has to be right.
        for (int i = 0; i < 3; ++i) {
            const int32_t hd = g_headDeltaFP[i].load(std::memory_order_relaxed);
            if (ok && camKind == 1) { s_mainPosFP[i] = p[i];  s_mainHeadFP[i] = hd; }
            if (ok && camKind == 2) { s_vrcamPosFP[i] = p[i]; s_vrcamHeadFP[i] = hd; }
        }
    }

    float right[3] = {};
    float shift = CyberpunkVR_IpdInPosAB ? GetDesiredHalfIpd() : 0.0f;
    bool shifted = false;

    if (shift != 0.0f &&
        IsPlausibleUnitQuaternion(quat) &&
        IsPlausibleCameraSpan(posA, posB)) {
        ComputeRightVectorFromQuaternion(quat, right);
        if (IsPlausibleUnitVector3(right)) {
            // Eye choice from the camera's IDENTITY, never from a call counter.
            //
            // MAIN is the left eye, VRCAM the right -- the same split the stereo module uses.
            // The old code picked the eye from the parity of a global hit counter, which is
            // only stable while exactly one camera exists: add the second and the two share
            // the counter, so the sign flips at random and the camera jumps a whole IPD
            // MAIN is the left eye, VRCAM the right -- the same split the stereo module
            // uses. The old code picked the eye from the parity of a global hit counter,
            // which is only stable while exactly one camera exists: add the second and the
            // two share the counter, so the sign flips at random and the camera jumps a
            // whole IPD between frames.
            shift = (camKind == 2) ? +shift : -shift;   // 1 = MAIN/left, 2 = VRCAM/right

            const uintptr_t stateAddr = reinterpret_cast<uintptr_t>(cameraState);
            const float dx = right[0] * shift;
            const float dy = right[1] * shift;
            const float dz = right[2] * shift;

            WriteFloatSafe(stateAddr + 0x10, posA[0] + dx);
            WriteFloatSafe(stateAddr + 0x14, posA[1] + dy);
            WriteFloatSafe(stateAddr + 0x18, posA[2] + dz);
            WriteFloatSafe(stateAddr + 0x20, posB[0] + dx);
            WriteFloatSafe(stateAddr + 0x24, posB[1] + dy);
            WriteFloatSafe(stateAddr + 0x28, posB[2] + dz);
            shifted = true;
        }
    }

    if ((g_patchCameraHits % 600) == 0) {
        uint8_t ownerFlag = 0xFF;
        if (ownerState && reinterpret_cast<uintptr_t>(ownerState) >= 0x10000) {
            ReadU8Safe(reinterpret_cast<uintptr_t>(ownerState) + 0xB1, &ownerFlag);
        }

        if (g_verboseLog) Log("PatchCamera state @%p owner=%p flagB1=%u plausQ=%d plausSpan=%d Q=(%.3f, %.3f, %.3f, %.3f) P0=(%.3f, %.3f, %.3f) P1=(%.3f, %.3f, %.3f) R=(%.3f, %.3f, %.3f) shift=%.4f applied=%d\n",
            cameraState,
            ownerState,
            static_cast<unsigned>(ownerFlag),
            IsPlausibleUnitQuaternion(quat) ? 1 : 0,
            IsPlausibleCameraSpan(posA, posB) ? 1 : 0,
            quat[0], quat[1], quat[2], quat[3],
            posA[0], posA[1], posA[2],
            posB[0], posB[1], posB[2],
            right[0], right[1], right[2],
            shift,
            shifted ? 1 : 0);
    }
}

bool InstallPatchCameraHook() {
    const char* pattern = "\x0F\x11\x02\x80\xBE\xB1\x00\x00\x00\x00\x89\x45\x88";
    const char* mask = "xxxxxxxxxxxxx";
    uint8_t* found = static_cast<uint8_t*>(FindPattern("Cyberpunk2077.exe", pattern, mask));
    if (!found) return false;

    constexpr int replaceLen = 10; // 0F 11 02 80 BE B1 00 00 00 00
    void* tramp = AllocateTrampoline(found, 512);
    if (!tramp) return false;

    uint8_t* code = static_cast<uint8_t*>(tramp);
    int pos = 0;

    // push rax
    code[pos++] = 0x50;
    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(g_telemetry) + kPatchTelemetryOffset);
    code[pos++] = 0xFF; code[pos++] = 0x00; // inc dword ptr [rax+0]
    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0x50; code[pos++] = 0x08; // mov [rax+8], rdx
    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0x70; code[pos++] = 0x10; // mov [rax+10h], rsi
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x40; code[pos++] = 0x18; // movups [rax+18h], xmm0
    code[pos++] = 0x58; // pop rax

    // Original instructions:
    // movups [rdx], xmm0
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x02;

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

    // Save xmm0-xmm3
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x40; // sub rsp, 40h
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x04; code[pos++] = 0x24; // movups [rsp+00h], xmm0
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10; // movups [rsp+10h], xmm1
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20; // movups [rsp+20h], xmm2
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30; // movups [rsp+30h], xmm3

    // Align stack and allocate shadow space
    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xE5; // mov rbp, rsp
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xE4; code[pos++] = 0xF0; // and rsp, -16 (0xFFFFFFFFFFFFFFF0)
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x20; // sub rsp, 20h

    // Set arg1 (rcx) = rdx (camera state), arg2 (rdx) = rsi (owner state)
    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xD1; // mov rcx, rdx
    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xF2; // mov rdx, rsi

    // Call OnPatchCameraCallback
    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(OnPatchCameraCallback));
    code[pos++] = 0xFF; code[pos++] = 0xD0; // call rax

    // Restore unaligned stack pointer
    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xEC; // mov rsp, rbp

    // Restore xmm0-xmm3
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x04; code[pos++] = 0x24; // movups xmm0, [rsp+00h]
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

    // End original instruction block
    code[pos++] = 0x80; code[pos++] = 0xBE; code[pos++] = 0xB1; code[pos++] = 0x00;
    code[pos++] = 0x00; code[pos++] = 0x00; code[pos++] = 0x00; // cmp byte ptr [rsi+0B1h], 0

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

CVR_HOOK("PatchCamera", ::cvr::hooks::Stage::Boot, 12, InstallPatchCameraHook);
