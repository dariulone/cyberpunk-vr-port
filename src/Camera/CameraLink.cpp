#include "Utils/DebugGate.hpp"
#include "Camera/CameraLink.hpp"
#include "Camera/EyeCentreLedger.hpp"
#include "Camera/RenderedPoseHistory.hpp"
#include "Camera/CameraState.hpp"
#include "Camera/PoseIdentity.hpp"

#include "Core/VrCoreShared.hpp"

#include <atomic>
#include <cstdint>
#include <mutex>
#include <cstring>

// The counters are src/Camera/CameraLinkState.cpp's (both builds); what this file needs from the rest of the plugin
// comes through src/Camera/CameraServices.cpp's C forms, which tools/vrik_tests replaces.
extern "C" {
extern std::atomic<uint64_t> CyberpunkVR_BodyCentreMatch,CyberpunkVR_BodyCentreMiss,CyberpunkVR_BodyLuaCentreMatch,CyberpunkVR_BodyLuaCentreRejected;
extern std::atomic<uint64_t> CyberpunkVR_PlacedPoseMatch,CyberpunkVR_PlacedPoseMiss,CyberpunkVR_PlacedPoseAmbiguous;
uint64_t cvr_cpp_xr_tracking_origin_serial();
uint64_t cvr_cpp_next_pose_identity();
bool cvr_cpp_device_cam_active();
bool cvr_cpp_braindance_active();
}

// Lifted out of the core hub verbatim when the three camera hooks were split into files of their
// own. The arithmetic is untouched; what changed is that the raw objects below are file-local, so
// the seqlock and the ring can only be reached through the four functions the header declares.
//
// See Camera/CameraLink.hpp for the ownership rule, the single-publisher invariant, and why these
// two are functions where LiveControls' forty scalars are not.

namespace {
bool g_camWriteKnown=false;

float g_camWriteQuat[4] = { 0.0f, 0.0f, 0.0f, 1.0f };
OpenXRHeadPose g_camWritePose{};
cvr::roomscale::Vec2 g_camWriteConsumed{};
bool g_camWriteConsumedKnown{};
uint64_t g_camWritePoseId{};
std::mutex s_camWriteMutex;


}  // namespace

namespace {
cvr::camera::EyeCentreLedger s_eyeCentres;
std::mutex s_anchorMutex;
cvr::camera::AnchorRecipe s_anchorRecipe{};
bool s_anchorKnown{};
}
void cvr::camera::AnchorRecipePublish(const AnchorRecipe& recipe) {
    std::lock_guard lock(s_anchorMutex);
    s_anchorRecipe=recipe; s_anchorKnown=true;
}
bool cvr::camera::AnchorRecipeRead(AnchorRecipe* recipe) {
    if (!recipe) return false;
    std::lock_guard lock(s_anchorMutex);
    if (!s_anchorKnown) return false;
    *recipe=s_anchorRecipe;
    return true;
}
void cvr::camera::AnchorRecipeClear() {
    std::lock_guard lock(s_anchorMutex);
    s_anchorKnown=false;
}
void cvr::camera::MainEyeCentrePublish(const int32_t eye[3], const int32_t centre[3], const int32_t bodyBase[3]) {
    s_eyeCentres.Publish({eye[0],eye[1],eye[2]}, {centre[0],centre[1],centre[2]},
                        {bodyBase[0],bodyBase[1],bodyBase[2]});
}
bool cvr::camera::MainEyeCentreRead(const int32_t eye[3], int32_t centre[3], int32_t bodyBase[3]) {
    EyeCentreLedger::Position value{},base{};
    if (!s_eyeCentres.Read({eye[0],eye[1],eye[2]}, value,bodyBase ? &base : nullptr)) {
        CVR_DIAGNOSTIC(CyberpunkVR_BodyCentreMiss.fetch_add(1, std::memory_order_relaxed));
        return false;
    }
    CVR_DIAGNOSTIC(CyberpunkVR_BodyCentreMatch.fetch_add(1, std::memory_order_relaxed));
    for (int i=0;i<3;++i) centre[i]=value[i];
    if (bodyBase) for (int i=0;i<3;++i) bodyBase[i]=base[i];
    return true;
}
bool cvr::camera::MainEyeCentreReadWorld(const float eyeOrCentre[3], float centre[3], float bodyBase[3]) {
    EyeCentreLedger::WorldPosition value{},base{};
    if (!s_eyeCentres.ReadWorld({eyeOrCentre[0],eyeOrCentre[1],eyeOrCentre[2]}, value,bodyBase ? &base : nullptr)) {
        CVR_DIAGNOSTIC(CyberpunkVR_BodyLuaCentreRejected.fetch_add(1, std::memory_order_relaxed));
        return false;
    }
    for (int i=0;i<3;++i) centre[i]=value[i];
    if (bodyBase) for (int i=0;i<3;++i) bodyBase[i]=base[i];
    CVR_DIAGNOSTIC(CyberpunkVR_BodyLuaCentreMatch.fetch_add(1, std::memory_order_relaxed));
    return true;
}
void cvr::camera::CamWriteQuatPublish(float x, float y, float z, float w, const OpenXRHeadPose& pose,
                                    const cvr::roomscale::Vec2* consumed,uint64_t sourceId) {
    std::lock_guard lock(s_camWriteMutex);
    g_camWriteQuat[0] = x; g_camWriteQuat[1] = y;
    g_camWriteQuat[2] = z; g_camWriteQuat[3] = w;
    g_camWritePose=pose;
    g_camWritePoseId=pose.valid ? (sourceId ? sourceId : cvr_cpp_next_pose_identity()) : 0;
    g_camWriteConsumedKnown=consumed!=nullptr;
    if(consumed)g_camWriteConsumed=*consumed;
    g_camWriteKnown=true;
}

bool cvr::camera::CamWriteQuatRead(float out[4], OpenXRHeadPose* pose,
                                 cvr::roomscale::Vec2* consumed,bool* consumedKnown,uint64_t* poseId) {
    std::lock_guard lock(s_camWriteMutex);
    if (!g_camWriteKnown) return false;
    for (int i=0;i<4;++i) out[i]=g_camWriteQuat[i];
    if (pose) *pose=g_camWritePose;
    if(consumed)*consumed=g_camWriteConsumed;
    if(consumedKnown)*consumedKnown=g_camWriteConsumedKnown;
    if(poseId)*poseId=g_camWritePoseId;
    return true;
}

// ---- WHAT WE WROTE, SO IT CAN BE RECOGNISED LATER -------------------------------------------
//
// Legacy orientation history for camera routes that do not publish a complete
// placed transform. On foot, translation-only frames have identical quaternions
// with different positions; s_placedCameraPoses supplies the full key instead.
namespace {
struct CamWriteRecord {
    float quat[4];
    OpenXRHeadPose pose;
    uint64_t id;         // monotonic write index; ordering is what disambiguates a tie
    uint32_t valid;
};

constexpr uint32_t kCamWriteRing = 16;

CamWriteRecord g_camWriteRing[kCamWriteRing]{};

std::atomic<uint64_t> g_camWriteRingHead{0};
std::mutex s_camWriteRingMutex;
cvr::camera::RenderedPoseHistory<OpenXRHeadPose> s_placedCameraPoses;
}  // namespace

bool cvr::camera::PlacedCameraPoseEnabled() {
    return CyberpunkVR_OneSamplePerFrame && CyberpunkVR_CamWriteInPatch &&
           CyberpunkVR_CamComposeAtWrite && !CyberpunkVR_CamWriteInFinal &&
           CyberpunkVR_HeadTranslationInPatch && CyberpunkVR_VrcamHeadTranslation &&
           CyberpunkVR_DeltaFromFreshSample && CyberpunkVR_VrcamPosFromMain!=3 &&
           !g_isInVehicle && !cvr_cpp_device_cam_active() && !cvr_cpp_braindance_active();
}
void cvr::camera::PlacedCameraPosePush(uint32_t view, const float q[4], const int32_t position[3],
                                      const OpenXRHeadPose& pose) {
    s_placedCameraPoses.Push(view,q,position,pose);
}
bool cvr::camera::PlacedCameraPoseFind(uint32_t view, const float q[4], const int32_t position[3],
                                      OpenXRHeadPose* out, uint32_t* age, uint32_t* ties) {
    const auto match=s_placedCameraPoses.Find(view,q,position,
        cvr_cpp_xr_tracking_origin_serial(),out,age,ties);
    if (match==RenderedPoseMatch::Exact) {
        CVR_DIAGNOSTIC(CyberpunkVR_PlacedPoseMatch.fetch_add(1,std::memory_order_relaxed));
        return true;
    }
    if (match==RenderedPoseMatch::Ambiguous)
        CVR_DIAGNOSTIC(CyberpunkVR_PlacedPoseAmbiguous.fetch_add(1,std::memory_order_relaxed));
    else CVR_DIAGNOSTIC(CyberpunkVR_PlacedPoseMiss.fetch_add(1,std::memory_order_relaxed));
    return false;
}

void cvr::camera::CamWriteRecordPush(const float q[4], const OpenXRHeadPose& p) {
    std::lock_guard lock(s_camWriteRingMutex);
    const uint64_t id = g_camWriteRingHead.load(std::memory_order_relaxed);
    CamWriteRecord& r = g_camWriteRing[id % kCamWriteRing];
    r.valid = 0;
    std::atomic_thread_fence(std::memory_order_release);
    r.quat[0] = q[0]; r.quat[1] = q[1]; r.quat[2] = q[2]; r.quat[3] = q[3];
    r.pose = p;
    r.id = id;
    std::atomic_thread_fence(std::memory_order_release);
    r.valid = 1;
    g_camWriteRingHead.fetch_add(1, std::memory_order_release);
}

// The identification counters (CyberpunkVR_DebugFinal*, CyberpunkVR_DebugVrcamFinal*) are CameraLinkState.cpp's.

// EXACT FIRST; NEAREST ONLY AS A FALLBACK THAT ANNOUNCES ITSELF.
//
// Measured over a session: the bit-identical match fires on every single frame -- `exact` tracked
// `match` one for one across thousands of frames. So the engine passes our quaternion to the
// render camera verbatim, and identification is unique BY CONSTRUCTION: two independent
// xrLocateSpace results do not come out bit-identical while a tracker is live, so at most one ring
// entry can match. No threshold, no nearest-search, nothing to tune -- which is the formal 100%
// the approximate path could only approach.
//
// The nearest path stays underneath for the day a game patch changes that, and it is counted
// separately so the change shows up as a number instead of as a symptom. Everything below about
// tolerances applies only to that fallback.
//
// TAKE THE NEAREST ENTRY, MEASURED PROPERLY. Not the first inside a tolerance, and not an
// ordinal pick either -- both of those were wrong, in opposite directions.
//
// First attempt: newest entry with `dot > 0.999999`. That tolerance is 0.16 degrees, so while the
// head barely moves several consecutive writes fall inside it and the scan always returned the
// LAST of them -- a pose from after the one in the frame. Random 0..0.16 deg, about six pixels,
// present or absent per frame: shimmer, and only on micro-movements, because an ordinary turn
// moves further than the tolerance between writes and the match is unique again.
//
// Second attempt: among the candidates, the oldest with `id >= lastMatched`. That freezes. With
// the head still, the previously matched entry keeps satisfying the tolerance, so it is chosen
// again and again while the head quietly drifts; the label stops advancing, the compositor
// reprojects by the whole accumulated difference, and the world slides away under you. The
// reported "floating while holding still" is exactly that.
//
// The real fix is to make the comparison precise enough that there is nothing to disambiguate.
// Two things were in the way:
//
//   * `1 - dot` cancels catastrophically. Head-still tracker noise is on the order of 0.01 deg,
//     which is dot = 1 - 4e-9 -- below float32 resolution, so every candidate compared EQUAL to
//     1.0f and the real nearest one was invisible. Comparing the component difference instead is
//     well conditioned at zero, and in double it resolves far below the noise floor.
//   * The gate still has to be loose, because the engine may renormalise the quaternion between
//     the placed component and the render camera, so a bitwise compare would find nothing at all.
//     Loose gate, precise pick: the gate only rejects nonsense, the minimum decides.
//
// Renormalisation perturbs a component by ~1e-7; the difference between two consecutive writes,
// even with the head still, is ~1e-4. Three orders of magnitude apart, so the nearest entry is
// the written one, unambiguously. No ordering state, nothing to freeze.
bool cvr::camera::CamWriteRecordFind(const float q[4], OpenXRHeadPose* out,
                               uint32_t* outAge, uint32_t* outTies) {
    std::lock_guard lock(s_camWriteRingMutex);
    const uint64_t head = g_camWriteRingHead.load(std::memory_order_acquire);
    const uint64_t n = head < kCamWriteRing ? head : kCamWriteRing;

    bool haveBest = false;
    double bestD = 0.0;
    uint64_t bestId = 0;
    OpenXRHeadPose bestPose{};
    uint32_t ties = 0;

    // IS THE MATCH ACTUALLY EXACT? -- the measurement that decides whether the tolerance is
    // needed at all.
    //
    // The whole reason identification is approximate is the assumption that the engine may
    // renormalise the quaternion between the placed component and the render camera. It probably
    // does somewhere -- sub_1401DA684, a callee of the view-matrix writer, visibly divides by
    // the norm before building a basis -- but that is on the path to the MATRIX, and says nothing
    // about the quaternion field we read. If the field is a straight copy, every match is
    // bit-identical, the tolerance is dead weight, and we can switch to exact compare: unique by
    // construction, no threshold, no nearest-search, formally 100%.
    //
    // So count it. exact == match over a session means the assumption was unnecessary.
    uint32_t exactCount = 0;
    bool haveExact = false;
    uint64_t exactId = 0;
    OpenXRHeadPose exactPose{};

    for (uint64_t i = 1; i <= n; ++i) {
        const CamWriteRecord& r = g_camWriteRing[(head - i) % kCamWriteRing];
        if (!r.valid) continue;
        if (r.quat[0] == q[0] && r.quat[1] == q[1] &&
            r.quat[2] == q[2] && r.quat[3] == q[3]) {
            ++exactCount;
            if (!haveExact) { haveExact = true; exactId = r.id; exactPose = r.pose; }
        }
        double dot = static_cast<double>(r.quat[0]) * q[0] + static_cast<double>(r.quat[1]) * q[1] +
                     static_cast<double>(r.quat[2]) * q[2] + static_cast<double>(r.quat[3]) * q[3];
        // q and -q are the same rotation; align before differencing.
        const double s = dot < 0.0 ? -1.0 : 1.0;
        if (dot * s <= 0.999999) continue;          // gate: obviously not this one
        ++ties;
        double d = 0.0;
        for (int k = 0; k < 4; ++k) {
            const double e = s * static_cast<double>(r.quat[k]) - static_cast<double>(q[k]);
            d += e * e;
        }
        if (!haveBest || d < bestD) {
            bestD = d; bestId = r.id; bestPose = r.pose; haveBest = true;
        }
    }
    // The exact hit decides whenever there is one -- it is the identification, not an estimate.
    if (haveExact) {
        CVR_DIAGNOSTIC(++CyberpunkVR_DebugFinalExact);
        if (exactCount > 1) CVR_DIAGNOSTIC(++CyberpunkVR_DebugFinalExactTies);
        if (out)     *out = exactPose;
        if (outAge)  *outAge = static_cast<uint32_t>((head - 1) - exactId);
        if (outTies) *outTies = exactCount;
        return true;
    }
    if (!haveBest) return false;

    // Count and report nearest-neighbour fallback so missing pose entries remain diagnosable.
    CVR_DIAGNOSTIC(++CyberpunkVR_DebugFinalApprox);
    {
        static uint64_t s_lastReport = 0;
        if (CyberpunkVR_DebugFinalApprox - s_lastReport >= 600) {
            s_lastReport = CyberpunkVR_DebugFinalApprox;
            Log("POSEDIAG: WARNING -- the render camera quaternion is no longer a verbatim copy "
                "(approx=%llu exact=%llu). Frame identification has degraded to nearest-neighbour; "
                "expect the pose label to be within %.3f deg rather than exact.\n",
                (unsigned long long)CyberpunkVR_DebugFinalApprox,
                (unsigned long long)CyberpunkVR_DebugFinalExact,
                0.16);
        }
    }
    if (out)     *out = bestPose;
    if (outAge)  *outAge = static_cast<uint32_t>((head - 1) - bestId);
    if (outTies) *outTies = ties;
    return true;
}

// ---- the view frame handed to the solve ---------------------------------------------------------
//
// A seqlock, for the same reason the write quaternion is one: the producer runs on the engine's
// render/job thread and the consumer inside the animation pass, and a reader that catches half a
// publication gets a frame of reference that existed at no instant. Odd = write in progress, and
// the reader retries; even and unchanged across the read = the copy is whole.
namespace {
std::atomic<uint32_t> g_lcfSeq{0};
struct AtomicLocatedCameraFrame {
    std::atomic<float> worldPos[3]{};
    std::atomic<float> worldQuat[4]{};
    std::atomic<uint32_t> sequence{0};
    std::atomic<uint32_t> frameEpoch{0};
    std::atomic<uint32_t> bodyCentreKnown{0};
    std::atomic<float> bodyBaseWorld[3]{};
};
AtomicLocatedCameraFrame g_lcf{};
std::atomic<uint32_t>  g_vfSeq{0};
cvr::camera::ViewFrame g_vf{};
// The barrel packet gets its own sequence: it is published from the final-camera callback, the view
// frame from the locate solve, and sharing one counter would make each writer's readers retry on the
// other's traffic for no reason.
std::atomic<uint32_t>    g_bfSeq{0};
cvr::camera::BarrelFrame g_bf{};
}  // namespace

namespace cvr::camera {

void LocatedCameraFramePublish(const LocatedCameraFrame& f) {
    g_lcfSeq.fetch_add(1u, std::memory_order_acq_rel);  // odd
    for (int i = 0; i < 3; ++i) {
        g_lcf.worldPos[i].store(f.worldPos[i], std::memory_order_relaxed);
    }
    for (int i = 0; i < 4; ++i) {
        g_lcf.worldQuat[i].store(f.worldQuat[i], std::memory_order_relaxed);
    }
    g_lcf.sequence.store(f.sequence, std::memory_order_relaxed);
    g_lcf.frameEpoch.store(f.frameEpoch, std::memory_order_relaxed);
    g_lcf.bodyCentreKnown.store(f.bodyCentreKnown, std::memory_order_relaxed);
    for (int i=0;i<3;++i) g_lcf.bodyBaseWorld[i].store(f.bodyBaseWorld[i],std::memory_order_relaxed);
    g_lcfSeq.fetch_add(1u, std::memory_order_release);  // even
}

bool LocatedCameraFrameRead(LocatedCameraFrame* out) {
    if (!out) return false;
    for (int tries = 0; tries < 8; ++tries) {
        const uint32_t s0 = g_lcfSeq.load(std::memory_order_acquire);
        if (s0 == 0u) return false;
        if (s0 & 1u) continue;
        LocatedCameraFrame tmp{};
        for (int i = 0; i < 3; ++i) {
            tmp.worldPos[i] = g_lcf.worldPos[i].load(std::memory_order_relaxed);
        }
        for (int i = 0; i < 4; ++i) {
            tmp.worldQuat[i] = g_lcf.worldQuat[i].load(std::memory_order_relaxed);
        }
        tmp.sequence = g_lcf.sequence.load(std::memory_order_relaxed);
        tmp.frameEpoch = g_lcf.frameEpoch.load(std::memory_order_relaxed);
        tmp.bodyCentreKnown = g_lcf.bodyCentreKnown.load(std::memory_order_relaxed);
        for (int i=0;i<3;++i) tmp.bodyBaseWorld[i]=g_lcf.bodyBaseWorld[i].load(std::memory_order_relaxed);
        if (g_lcfSeq.load(std::memory_order_acquire) == s0) {
            *out = tmp;
            return true;
        }
    }
    return false;
}

void ViewFramePublish(const ViewFrame& f) {
    const uint32_t s = g_vfSeq.load(std::memory_order_relaxed) + 1u;   // odd
    g_vfSeq.store(s, std::memory_order_relaxed);
    std::atomic_thread_fence(std::memory_order_release);
    g_vf = f;
    std::atomic_thread_fence(std::memory_order_release);
    g_vfSeq.store(s + 1u, std::memory_order_relaxed);                  // even = complete
}

void BarrelFramePublish(const BarrelFrame& f) {
    const uint32_t s = g_bfSeq.load(std::memory_order_relaxed) + 1u;   // odd
    g_bfSeq.store(s, std::memory_order_relaxed);
    std::atomic_thread_fence(std::memory_order_release);
    g_bf = f;
    std::atomic_thread_fence(std::memory_order_release);
    g_bfSeq.store(s + 1u, std::memory_order_relaxed);                  // even = complete
}

bool BarrelFrameRead(BarrelFrame* out) {
    if (!out) return false;
    for (int tries = 0; tries < 8; ++tries) {
        const uint32_t s0 = g_bfSeq.load(std::memory_order_relaxed);
        if (s0 == 0u) return false;          // nothing published yet
        if (s0 & 1u) continue;               // write in progress
        std::atomic_thread_fence(std::memory_order_acquire);
        const BarrelFrame tmp = g_bf;
        std::atomic_thread_fence(std::memory_order_acquire);
        if (g_bfSeq.load(std::memory_order_relaxed) == s0) {
            *out = tmp;
            return true;
        }
    }
    return false;
}

bool ViewFrameRead(ViewFrame* out) {
    if (!out) return false;
    for (int tries = 0; tries < 8; ++tries) {
        const uint32_t s0 = g_vfSeq.load(std::memory_order_relaxed);
        if (s0 == 0u) return false;          // nothing published yet
        if (s0 & 1u) continue;               // write in progress
        std::atomic_thread_fence(std::memory_order_acquire);
        const ViewFrame tmp = g_vf;
        std::atomic_thread_fence(std::memory_order_acquire);
        if (g_vfSeq.load(std::memory_order_relaxed) == s0) {
            *out = tmp;
            return true;
        }
    }
    return false;
}

}  // namespace cvr::camera
