#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
// CameraPair -- natives lifted out of src/Natives/Natives.cpp, which held every family at once.
//
// The camera-pair publisher: the one place the pose the camera was written from is
// handed to whatever reads it back.
//
// The cut was placed by the seam map and then SNAPPED to the nearest point at brace depth zero.
// Boundaries taken from line numbers alone are how a split lands in the middle of a function; the
// check is cheap and it is the same lesson as every other generator in this restructure.
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/GameEngine.hpp>
#include <sstream>
#include <locale>
#include <clocale>
#include "Utils/SharedSlots.hpp"   // CyberpunkVR_Hands_Shared slot map (single source of truth)
#include <RED4ext/Containers/StaticArray.hpp>
#include <RED4ext/Scripting/Natives/ScriptGameInstance.hpp>
#include <RED4ext/Scripting/Utils.hpp>
#include <RED4ext/Scripting/Functions.hpp>
#include <RED4ext/Scripting/CProperty.hpp>
#include <RED4ext/Scripting/Natives/Generated/WorldPosition.hpp>
#include <RED4ext/Scripting/Natives/Transform.hpp>
#include <RED4ext/Scripting/Natives/animRig.hpp>
#include <RED4ext/Scripting/Natives/Generated/Vector4.hpp>
#include <RED4ext/Scripting/Natives/Generated/Quaternion.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimGraph.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimFeature_IK.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimFeature_MeleeIKData.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimFeature_WeaponUser.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimVariableBool.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimVariableContainer.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimVariableFloat.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimVariableInt.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimVariableQuaternion.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimVariableTransform.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimVariableVector.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/AnimationControlBinding.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/AnimInputSetterAnimFeature.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/AnimInputSetterFloat.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/AnimInputSetterVector.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/IBinding.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/IKTargetAddEvent.hpp>
#include <RED4ext/Scripting/Natives/Generated/red/Event.hpp>
#include <RED4ext/Scripting/Natives/entEntity.hpp>
#include <RED4ext/Scripting/Natives/entAnimationControllerComponent.hpp>
#include <RED4ext/Scripting/Natives/entIPlacedComponent.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/AnimatedComponent.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/StaticOrientationProvider.hpp>
#include <RED4ext/Scripting/Natives/worldAnimationSystem.hpp>
#include <RED4ext/Scripting/Natives/worldAnimationSystemScriptInterface.hpp>
#include <RED4ext/Scripting/Natives/entSkinnedMeshComponent.hpp>
#include <RED4ext/Scripting/Natives/entAnimationControllerComponent.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/GarmentSkinnedMeshComponent.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/MeshComponent.hpp>
#include <windows.h>
#include <algorithm>
#include <cctype>
#include <cstring>
#include <atomic>
#include <cstdint>
#include <fstream>
#include <utility>
#include <iomanip>
#include <string>
#include "Anim/VrikHook.hpp"
#include "Anim/WeaponAim.hpp"
#include "Natives/NativeState.hpp"
#include "Camera/CameraLink.hpp"
#include "Camera/CameraState.hpp"
#include "Core/VrCoreShared.hpp"
#include "Natives/NativeHelpers.hpp"
#include <MinHook.h>
#include "Natives/NativeFunctions.hpp"
#include "Natives/NativeHelpers.hpp"
#include "Natives/NativeState.hpp"




// VR Transform data from Lua (Camera and Player Model Space)




#if CVR_RUST_VRIK
#else
volatile float g_VRPlayerYaw = 0.0f;
#endif

#if CVR_RUST_VRIK
#else
volatile float g_VRCamI = 0.0f;
#endif
#if CVR_RUST_VRIK
#else
volatile float g_VRCamJ = 0.0f;
#endif
#if CVR_RUST_VRIK
#else
volatile float g_VRCamK = 0.0f;
#endif
#if CVR_RUST_VRIK
#else
volatile float g_VRCamR = 1.0f;
#endif

// FPP camera (HMD) world position + player entity world position, pushed from Lua each
// frame (init.lua getCameraWorldPose + player:GetWorldPosition). The full-arm IK converts
// the gizmo's WORLD hand target into the bone buffer's MODEL space using these, so the
// hand lands exactly on the gizmo. See VRIK camModel block in vrik_hook.h.
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs (cpp_globals)
volatile float g_VRCamPosX = 0.0f, g_VRCamPosY = 0.0f, g_VRCamPosZ = 0.0f;
#endif
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs (cpp_globals)
volatile float g_VREntityPosX = 0.0f, g_VREntityPosY = 0.0f, g_VREntityPosZ = 0.0f;
#endif
// THE single stabilized camera-local offset (cam - entity, world axes), low-passed in
// SetVRTransforms from the coherent same-push Lua pair. Consumed by the VRIK skeleton
// (these globals) AND by the rendered view (published to shared [124..127], applied by
// dxgi's camera stabilizer) -- one value, two consumers, so body and view are welded
// and bob/sway/dash/recoil kicks exist in neither. Tear-safe: cm-scale bounded quantity
// (all fast world motion lives in the entity and cancels out of the difference).
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs (cpp_globals)
volatile float g_VRCamPairLocalX = 0.0f, g_VRCamPairLocalY = 0.0f, g_VRCamPairLocalZ = 0.0f;
#endif
#if CVR_RUST_VRIK
#else
volatile int   g_VRCamPairValid = 0;
#endif
#if CVR_RUST_VRIK
#else
volatile int   g_VRCamPosValid = 0;   // 0 until Lua has pushed a camera/entity pose
#endif
// Player entity world ORIENTATION quaternion (i,j,k,r). The world->model rotation is its
// conjugate; the full-arm IK uses it to convert the gizmo world target into model space.
// GetWorldOrientation().yaw was nil (silently 0), so we now take the real quaternion.
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/cpp_services.rs (cpp_globals)
volatile float g_VREntityQI = 0.0f, g_VREntityQJ = 0.0f, g_VREntityQK = 0.0f, g_VREntityQR = 1.0f;
#endif

#if CVR_RUST_VRIK
// The pair's work and the snapshot are rust/crates/cvr-rig/src/vrik/calibration.rs; the solve reads the snapshot
// through this name (tools/vrik_tests gives its own).
#else
namespace {
std::atomic<uint32_t> s_vrikTransformSeq{0};   // even = stable, odd = write in progress
// A sequence counter alone does not make concurrent plain struct accesses legal in C++: the reader
// and writer would still data-race before the reader can reject a torn sample.  Keep every payload
// lane atomic; the sequence counter then turns the individually safe loads into one logical snapshot.
struct AtomicVrikTransformSnapshot {
    std::atomic<float> camQuat[4]{};
    std::atomic<float> entityQuat[4]{};
    std::atomic<float> cameraMinusEntity[3]{};
    std::atomic<uint32_t> valid{0};
    std::atomic<uint64_t> publishedMs{0};
    std::atomic<float> bodyCameraMinusEntity[3]{};
};
AtomicVrikTransformSnapshot s_vrikTransform{};

void PublishVrikTransformSnapshot(const float bodyCameraMinusEntity[3]) {
    s_vrikTransformSeq.fetch_add(1, std::memory_order_acq_rel);
    s_vrikTransform.camQuat[0].store(g_VRCamI, std::memory_order_relaxed);
    s_vrikTransform.camQuat[1].store(g_VRCamJ, std::memory_order_relaxed);
    s_vrikTransform.camQuat[2].store(g_VRCamK, std::memory_order_relaxed);
    s_vrikTransform.camQuat[3].store(g_VRCamR, std::memory_order_relaxed);
    s_vrikTransform.entityQuat[0].store(g_VREntityQI, std::memory_order_relaxed);
    s_vrikTransform.entityQuat[1].store(g_VREntityQJ, std::memory_order_relaxed);
    s_vrikTransform.entityQuat[2].store(g_VREntityQK, std::memory_order_relaxed);
    s_vrikTransform.entityQuat[3].store(g_VREntityQR, std::memory_order_relaxed);
    s_vrikTransform.cameraMinusEntity[0].store(g_VRCamPairLocalX, std::memory_order_relaxed);
    s_vrikTransform.cameraMinusEntity[1].store(g_VRCamPairLocalY, std::memory_order_relaxed);
    s_vrikTransform.cameraMinusEntity[2].store(g_VRCamPairLocalZ, std::memory_order_relaxed);
    s_vrikTransform.valid.store(1, std::memory_order_relaxed);
    s_vrikTransform.publishedMs.store(GetTickCount64(), std::memory_order_relaxed);
    for (int i=0;i<3;++i)
        s_vrikTransform.bodyCameraMinusEntity[i].store(bodyCameraMinusEntity[i],std::memory_order_relaxed);
    s_vrikTransformSeq.fetch_add(1, std::memory_order_release);
}

void InvalidateVrikTransformSnapshot() {
    s_vrikTransformSeq.fetch_add(1, std::memory_order_acq_rel);
    s_vrikTransform.valid.store(0, std::memory_order_relaxed);
    s_vrikTransformSeq.fetch_add(1, std::memory_order_release);
}
}  // namespace

bool VRIK_ReadTransformSnapshot(VrikTransformSnapshot* out) {
    if (!out) return false;
    for (int attempt = 0; attempt < 4; ++attempt) {
        const uint32_t s0 = s_vrikTransformSeq.load(std::memory_order_acquire);
        if (s0 == 0 || (s0 & 1u)) continue;
        VrikTransformSnapshot tmp{};
        for (int i = 0; i < 4; ++i) {
            tmp.camQuat[i] = s_vrikTransform.camQuat[i].load(std::memory_order_relaxed);
            tmp.entityQuat[i] = s_vrikTransform.entityQuat[i].load(std::memory_order_relaxed);
        }
        for (int i = 0; i < 3; ++i) {
            tmp.cameraMinusEntity[i] =
                s_vrikTransform.cameraMinusEntity[i].load(std::memory_order_relaxed);
            tmp.bodyCameraMinusEntity[i] = s_vrikTransform.bodyCameraMinusEntity[i].load(std::memory_order_relaxed);
        }
        tmp.valid = s_vrikTransform.valid.load(std::memory_order_relaxed);
        const auto publishedMs = s_vrikTransform.publishedMs.load(std::memory_order_relaxed);
        if (s_vrikTransformSeq.load(std::memory_order_acquire) == s0) {
            if (tmp.valid && (!publishedMs || GetTickCount64() - publishedMs > 250)) {
                tmp.valid = false;
                tmp.unavailable = true;
            }
            *out = tmp;
            return true;
        }
    }
    return false;
}

// CAMERA-KICK TRACE ring buffer (see the trace block in SetVRPlayerYaw and the dump in
// WriteVRDiagCore). Columns: 0..2 raw local (camLua-entLua), 3..4 cam quat i/j (pitch/
// roll kick indicators), 5..7 filtered pair local, 8..10 eyeBake [116..118], 11 camBake y,
// 12..14 LOCATED render-camera entity-local (dxgi latch diag, shared [137..139]),
// 15 hips model yaw deg, 16..17 right IK target model x/y, 18..19 solved right hand FK
// model x/y, 20..21 right shoulder joint model x/y (arm-chain drift localizer).
// VR_CAMTRACE_CAP moved to Natives/NativeHelpers.hpp as a constant: it is the bound of an
// array two families index, and a #define in one .cpp is invisible to the other.
float g_camTrace[VR_CAMTRACE_CAP][22];
int   g_camTraceN = 0;
int   g_camTraceFreeze = -1;   // -1 live, >0 post-stop countdown, 0 FROZEN

void SetVRPlayerYaw(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);

    float pYaw = 0.0f;
    float ci = 0.0f, cj = 0.0f, ck = 0.0f, cr = 1.0f;
    float camX = 0.0f, camY = 0.0f, camZ = 0.0f;   // FPP camera (HMD) world position
    float entX = 0.0f, entY = 0.0f, entZ = 0.0f;   // player entity world position
    float eqi = 0.0f, eqj = 0.0f, eqk = 0.0f, eqr = 1.0f; // entity world orientation quaternion

    RED4ext::GetParameter(aFrame, &pYaw);
    RED4ext::GetParameter(aFrame, &ci);
    RED4ext::GetParameter(aFrame, &cj);
    RED4ext::GetParameter(aFrame, &ck);
    RED4ext::GetParameter(aFrame, &cr);
    RED4ext::GetParameter(aFrame, &camX);
    RED4ext::GetParameter(aFrame, &camY);
    RED4ext::GetParameter(aFrame, &camZ);
    RED4ext::GetParameter(aFrame, &entX);
    RED4ext::GetParameter(aFrame, &entY);
    RED4ext::GetParameter(aFrame, &entZ);
    RED4ext::GetParameter(aFrame, &eqi);
    RED4ext::GetParameter(aFrame, &eqj);
    RED4ext::GetParameter(aFrame, &eqk);
    RED4ext::GetParameter(aFrame, &eqr);
    aFrame->code++;
    if (aOut) *aOut = 0;
    float bodyCameraWorld[3] = {camX,camY,camZ};

    // The Lua path samples the SAME FPP component via GetLocalToWorld. It must
    // have the same position meaning as the native camera/entity pair. Otherwise
    // a native phase miss alternates the body between head centre and MAIN eye.
    // Resolve before the legacy pair filter: filtering eye data and subtracting
    // a newer IPD later is a different frame and leaves a rotation-dependent tail.
    if ((CyberpunkVR_VrikNativeFramePair || CyberpunkVR_VrikTransformsFromPlugin) &&
        (CyberpunkVR_IpdInWorldPos || CyberpunkVR_HeadTranslationInPatch) &&
        !DeviceCamActive() && !g_bdActive.load(std::memory_order_relaxed)) {
        const float supplied[3] = {camX,camY,camZ};
        float centre[3]{};
        if (!cvr::camera::MainEyeCentreReadWorld(supplied,centre,bodyCameraWorld)) {
            // Keep the previous complete snapshot briefly; never publish a raw
            // eye as its replacement. Its reader enforces a bounded lifetime.
            return;
        }
        camX=centre[0]; camY=centre[1]; camZ=centre[2];
    }
    g_VRPlayerYaw = pYaw;
    // FPP camera (HMD) world quaternion -- used by the full-arm IK to place the
    // hand target in world space (world->model via -yaw), so head turns don't drag it.
    g_VRCamI = ci; g_VRCamJ = cj; g_VRCamK = ck; g_VRCamR = cr;
    // Camera (HMD) + entity world position -> lets the IK convert the gizmo world target
    // into model space (camModelPos = Rz(-yaw)*(camPos - entityPos)). The legacy quat-only
    // call (5 params) leaves these at 0 and g_VRCamPosValid stays 0 -> IK falls back to the
    // head-relative path.
    if (!(camX == 0.0f && camY == 0.0f && camZ == 0.0f &&
          entX == 0.0f && entY == 0.0f && entZ == 0.0f)) {
        g_VRCamPosX = camX; g_VRCamPosY = camY; g_VRCamPosZ = camZ;
        g_VREntityPosX = entX; g_VREntityPosY = entY; g_VREntityPosZ = entZ;
        g_VREntityQI = eqi; g_VREntityQJ = eqj; g_VREntityQK = eqk; g_VREntityQR = eqr;
        g_VRCamPosValid = 1;
    }   // legacy 5-param call: keep the last GOOD pose instead of zeroing everything
    // Publish the player entity position for dxgi's camera-position STABILIZER
    // ([96..98]). [99] is a TICK COUNTER, not a flag: dxgi steps its filter ONLY when
    // this advances, pairing the entity with the camera of the SAME game tick. Filtering
    // per RENDER frame against a stale-tick entity made `local` oscillate by v*dt during
    // locomotion -> the whole view/body trembled forward while walking/sprinting.
    // SINGLE-FILTER CAMERA ARCHITECTURE. There is exactly ONE stabilized (cam - entity)
    // local offset in the whole pipeline, computed HERE from the coherent same-push pair:
    //   * the skeleton (body anchor + hands view base) consumes it via g_VRCamPairLocal*;
    //   * the RENDERED VIEW consumes the very same value via shared [124..127] (dxgi
    //     replaces its own filter with the published one when tick-matched).
    // History of why: raw-body/smooth-view showed bob+kick on the skeleton (walk micro-
    // jitter, sprint start/stop jerk); smooth-body/smooth-view with TWO independent
    // filters trembled by their transient disagreement. One value, two consumers ==
    // nothing can diverge, and bob/sway/dash/recoil kicks exist NOWHERE on screen.
    // CAMERA-KICK TRACE (diagnostic). Ring buffer of the raw camera-local offset and
    // the camera quat's non-yaw components per push; dumped by LogVRDiag. Answers WITH
    // DATA which channel carries the sprint/shot/dash kick that drags the body:
    // position (lx/ly/lz swing) or rotation (qi/qj swing), and whether the filter
    // passes it (flt vs raw).
    bool pairAccepted = false;
    {
        // SPRINT-TRANSIENT AUTO-FREEZE. The manual Log VR Diag click requires the CET
        // overlay, which blocks movement -- by the time the player stops, opens it and
        // clicks, the sprint has rolled out of the 256-push (~4.3s) ring (a dump full
        // of standing frames proved it). So: when the entity speed EMA [132..133]
        // falls through 3.5 m/s (sprint stop), record 90 more pushes (~1.5s of stop
        // transient + settle) and FREEZE the ring. A short sprint keeps its ENGAGE
        // transient inside the frozen window too. The dump un-freezes for the next run.
        if (g_pSharedHands) {
            static float s_trPrevSp2 = 0.0f;
            const float tvx = g_pSharedHands[132], tvy = g_pSharedHands[133];
            const float sp2 = tvx * tvx + tvy * tvy;
            if (g_camTraceFreeze < 0 && s_trPrevSp2 > 12.25f && sp2 <= 12.25f)
                g_camTraceFreeze = 90;
            s_trPrevSp2 = sp2;
        }
        if (g_camTraceFreeze > 0) --g_camTraceFreeze;
        const float lx = camX - entX, ly = camY - entY, lz = camZ - entZ;
        if (g_camTraceFreeze != 0 && lx*lx + ly*ly + lz*lz < 9.0f) {
            const int w = g_camTraceN % VR_CAMTRACE_CAP;
            g_camTrace[w][0] = lx;
            g_camTrace[w][1] = ly;
            g_camTrace[w][2] = lz;
            g_camTrace[w][3] = ci;   // quat i (pitch/roll kick indicator)
            g_camTrace[w][4] = cj;   // quat j
            g_camTrace[w][5] = g_VRCamPairLocalX;   // filtered (previous push's output is fine)
            g_camTrace[w][6] = g_VRCamPairLocalY;
            g_camTrace[w][7] = g_VRCamPairLocalZ;
            g_camTrace[w][8]  = g_pSharedHands ? g_pSharedHands[116] : 0.0f;  // eyeBake x
            g_camTrace[w][9]  = g_pSharedHands ? g_pSharedHands[117] : 0.0f;  // eyeBake y
            g_camTrace[w][10] = g_pSharedHands ? g_pSharedHands[118] : 0.0f;  // eyeBake z
            g_camTrace[w][11] = g_pSharedHands ? g_pSharedHands[92]  : 0.0f;  // camBake y
            g_camTrace[w][12] = 0.0f;  // (dead: located-local [137..139] writer removed)
            g_camTrace[w][13] = 0.0f;
            g_camTrace[w][14] = 0.0f;
            g_camTrace[w][15] = g_VRIKDbgHipsYaw;                             // hips model yaw
            g_camTrace[w][16] = g_VRIKDbgTargetTrace[0];                      // IK target x
            g_camTrace[w][17] = g_VRIKDbgTargetTrace[1];                      // IK target y
            g_camTrace[w][18] = g_VRIKDbgHandFK[0];                           // solved hand x
            g_camTrace[w][19] = g_VRIKDbgHandFK[1];                           // solved hand y
            g_camTrace[w][20] = g_VRIKDbgShModel[0];                          // shoulder x
            g_camTrace[w][21] = g_VRIKDbgShModel[1];                          // shoulder y
            ++g_camTraceN;
        }
    }
    // DEGENERATE PUSH GUARD. The legacy 5-param SetVRPlayerYaw call leaves cam/ent at
    // zero. Such a push must be ignored COMPLETELY: no pair update, no entity publish,
    // no seq advance -- publishing "loc=(0,0,0), valid" once made dxgi apply
    // held = entity - camera => the view sank to the feet; publishing entity=(0,0,0)
    // wasted dxgi latch ticks. (No published-pair channel anymore either: the view
    // runs purely on dxgi's own filter -- verified in the headset as the stable configuration.)
    const bool degeneratePush =
        (camX == 0.0f && camY == 0.0f && camZ == 0.0f &&
         entX == 0.0f && entY == 0.0f && entZ == 0.0f);
    {
        // XY SLEW LIMITER -- BY MEASUREMENT (sprint-transient dive fix). The original
        // "NO FILTER" verdict came from a STANDING/gunfire trace; the frozen sprint
        // window (auto-freeze dump) showed the truth: during sprint the game holds the
        // FPP camera ~0.20m AHEAD of the entity -- pair = (-0.155,-0.126,1.6) -- and at
        // sprint stop that lead collapses to (0,0) in ~0.13s. Anchor, IK targets, hand
        // and shoulder all rode that 20cm swing 1:1 (tgtY 0.297->0.093, shY -0.155->
        // -0.356): the whole body+hands LURCH around the player's stationary real head =
        // the sprint start/stop body/hands dive (world provably stable meanwhile).
        // Steady states are exact by construction (limiter passes constants); only the
        // swing RATE is capped to ~0.5 m/s, stretching the 20cm transition into a soft
        // ~0.4s settle. Z passes RAW (crouch height must not lag). Teleport guard: a
        // horizontal residual > 0.35m snaps (vault/knockback/cinematic cuts).
        const float lx = camX - entX, ly = camY - entY, lz = camZ - entZ;
        const float l2 = lx*lx + ly*ly + lz*lz;
        if (degeneratePush || l2 < 1.0e-4f) {
            // Ignore entirely: keep the previous pair state untouched.
        } else if (l2 < 9.0f) {
            static float s_slew[2] = { 0.0f, 0.0f };
            static float s_pvel[2] = { 0.0f, 0.0f };   // pair velocity (m/push, EMA)
            static bool  s_slewInit = false;
            if (!s_slewInit) { s_slew[0] = lx; s_slew[1] = ly; s_slewInit = true; }
            const float rx = lx - s_slew[0], ry = ly - s_slew[1];
            if (rx * rx + ry * ry > 0.1225f) {              // >0.35m: teleport, snap
                s_slew[0] = lx; s_slew[1] = ly;
                s_pvel[0] = 0.0f; s_pvel[1] = 0.0f;
            } else {
                // m/push at ~60Hz ticks; rate live-tunable via SetVRPairSlew (CET).
                const float kMaxStep = g_VRPairSlewRate * 0.0166f;
                const float sx = (rx >  kMaxStep) ?  kMaxStep : ((rx < -kMaxStep) ? -kMaxStep : rx);
                const float sy = (ry >  kMaxStep) ?  kMaxStep : ((ry < -kMaxStep) ? -kMaxStep : ry);
                s_slew[0] += sx;
                s_slew[1] += sy;
                // Pair velocity EMA (tau ~2.5 pushes): smooth enough to avoid end-of-
                // ramp overshoot, fast enough to track the 8-push sprint transition.
                s_pvel[0] += (sx - s_pvel[0]) * 0.4f;
                s_pvel[1] += (sy - s_pvel[1]) * 0.4f;
            }
            // Publish with one-tick lead (SetVRPairLead) to cancel solve->render skew.
            const float lead = g_VRPairLeadTicks;
            g_VRCamPairLocalX = s_slew[0] + s_pvel[0] * lead;
            g_VRCamPairLocalY = s_slew[1] + s_pvel[1] * lead;
            g_VRCamPairLocalZ = lz;
            g_VRCamPairValid = 1;
            pairAccepted = true;
        } else {
            // Cinematic / detached camera: bypass (consumers fall back).
            g_VRCamPairValid = 0;
        }
    }
    if (!degeneratePush && g_VRCamPosValid) {
        if (pairAccepted) {
            const float bodySpan[3]={bodyCameraWorld[0]-entX,bodyCameraWorld[1]-entY,bodyCameraWorld[2]-entZ};
            PublishVrikTransformSnapshot(bodySpan);
        }
        else if (!g_VRCamPairValid) InvalidateVrikTransformSnapshot();
    }
    if (g_pSharedHands && !degeneratePush) {
        static float s_entSeq = 0.0f;
        s_entSeq += 1.0f;
        if (s_entSeq > 1.0e6f) s_entSeq = 1.0f;
        // ENTITY VELOCITY + PUSH TIMESTAMP ([132..136]) for the synthetic-view
        // extrapolation: the view is built from the PUSHED entity while the skeleton
        // renders at the engine's (fresher) entity transform -- during strafe/run the
        // WHOLE BODY led the view by v*dt (~2cm, confirmed in the headset). dxgi extrapolates
        // ent + v*(tFinalCam - tPush) with the same-process QPC clock.
        {
            static LARGE_INTEGER s_qpf = { 0 };
            if (!s_qpf.QuadPart) QueryPerformanceFrequency(&s_qpf);
            static LARGE_INTEGER s_prevT = { 0 };
            static float s_prevEnt[3] = { 0, 0, 0 };
            static float s_vel[3] = { 0, 0, 0 };
            static bool  s_velInit = false;
            LARGE_INTEGER nowT; QueryPerformanceCounter(&nowT);
            if (s_velInit && s_qpf.QuadPart) {
                const double dt = double(nowT.QuadPart - s_prevT.QuadPart) / double(s_qpf.QuadPart);
                if (dt > 1.0e-4 && dt < 0.25) {
                    const float ivx = static_cast<float>((entX - s_prevEnt[0]) / dt);
                    const float ivy = static_cast<float>((entY - s_prevEnt[1]) / dt);
                    const float ivz = static_cast<float>((entZ - s_prevEnt[2]) / dt);
                    if (ivx*ivx + ivy*ivy + ivz*ivz < 400.0f) {   // < 20 m/s: locomotion
                        s_vel[0] += (ivx - s_vel[0]) * 0.3f;
                        s_vel[1] += (ivy - s_vel[1]) * 0.3f;
                        s_vel[2] += (ivz - s_vel[2]) * 0.3f;
                    } else {                                       // teleport: reset
                        s_vel[0] = s_vel[1] = s_vel[2] = 0.0f;
                    }
                }
            }
            s_prevEnt[0] = entX; s_prevEnt[1] = entY; s_prevEnt[2] = entZ;
            s_prevT = nowT; s_velInit = true;
            g_pSharedHands[132] = s_vel[0];
            g_pSharedHands[133] = s_vel[1];
            g_pSharedHands[134] = s_vel[2];
            const double ts = static_cast<double>(nowT.QuadPart);
            memcpy(const_cast<float*>(&g_pSharedHands[135]), &ts, sizeof(double));   // [135..136]
        }
        // Publish the clean pair on [128..131]. NOT [124..127]: those belong to
        // openxr_manager (HMD position [124..126] + the hands-snapshot SEQLOCK [127]) --
        // writing there clobbered the pair (seq stayed 0, dxgi never saw it and fell
        // back to its leaky own-filter: the kicks that kept showing) AND corrupted
        // the hands seqlock. Order: velocity/timestamp, then pair [128..131], then
        // entity+seq [96..99].
        if (g_VRCamPairValid) {
            g_pSharedHands[128] = g_VRCamPairLocalX;
            g_pSharedHands[129] = g_VRCamPairLocalY;
            g_pSharedHands[130] = g_VRCamPairLocalZ;
            g_pSharedHands[131] = s_entSeq;
        } else {
            g_pSharedHands[131] = 0.0f;
        }
        g_pSharedHands[96] = entX;
        g_pSharedHands[97] = entY;
        g_pSharedHands[98] = entZ;
        // [CAMWRITE] entity world yaw (deg) for dxgi's mode-1 heading source.
        // On foot the mouse/stick yaw integrator IS the entity yaw; the camera
        // quat can no longer serve as the heading source there because in
        // component mode it carries OUR composed HMD yaw (reading it back
        // re-integrates the head turn every locate = runaway spin).
        g_pSharedHands[153] = pYaw;
        g_pSharedHands[99] = s_entSeq;
    }

    if (aOut) *aOut = 1;
}
#endif
#endif
