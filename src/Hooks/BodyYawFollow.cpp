#include "Runtimes/HeadLedBodyTurn.hpp"
#include "Utils/DebugGate.hpp"
// Estimates physical body heading from HMD/controllers. BodyHeading.cpp feeds
// each request to the native movement adapter and retains only committed
// heading. View tracking has a separate reference and never cancels this output.

#include "Core/VrCoreShared.hpp"
#include "Core/LiveControls.hpp"
#include "Hooks/Hook.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include "Anim/CharacterRig.hpp"   // g_VREntityPos*: the player's world position for the publish
#include "Anim/VrikState.hpp"
#include "Camera/CameraLink.hpp"
#include "Camera/NativeCameraPair.hpp"
#include "Hooks/RoomscaleMove.hpp"
#include "Hooks/BodyHeading.hpp"
#include "Runtimes/BodyYawFollower.hpp"
#include "Camera/CameraState.hpp"
#include "Runtimes/LookDownCone.hpp"
#include "Runtimes/HybridBodyYaw.hpp"
#include "Fbt/Game/Calibration.hpp"
#include <mutex>

#include <windows.h>
#include <atomic>
#include <cstdint>
#include <cmath>
#include <chrono>

extern void Log(const char* fmt, ...);

// ---- controls -----------------------------------------------------------------------------------
//
// The switch itself lives in LiveControls (xr_physical_body_rotation, persisted); this mirror exists
// so the pose path and the camera write have one plain symbol to test on their hot paths.
extern "C" __declspec(dllexport) int   CyberpunkVR_BodyYawFollow        = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugVrikNativePairPublished = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugVrikNativePairRejected = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugVrikNativePairPhaseMiss = 0;
// FIVE DEGREES OF FREE LOOK, per the roomscale movement requirement.
//
// Glances inside the cone leave the body still. Crossing it starts a short
// body-only catch-up toward the centre, restoring the full free-look zone.
// Camera tracking consumes its independent reference immediately.
extern "C" __declspec(dllexport) float CyberpunkVR_BodyYawFollowDeadDeg = 5.0f;
// Hybrid: the hands' share of the body's turn, 0 (head cone) .. 1 (tracked); fades between the two.
extern "C" __declspec(dllexport) float CyberpunkVR_DebugHybridTrackedWeight = 0.0f;
// Hybrid: the hands' heading and the neck limit's pull on top of it (degrees, the follower's offset convention).
extern "C" __declspec(dllexport) float CyberpunkVR_DebugHybridHandsDeg = 0.0f;
extern "C" __declspec(dllexport) float CyberpunkVR_DebugHybridNeckPullDeg = 0.0f;
extern "C" __declspec(dllexport) float CyberpunkVR_DebugHybridAlignDeg = 0.0f;   // hands' alignment to the body
extern "C" __declspec(dllexport) int   CyberpunkVR_DebugHybridEstimate = 0;      // 1: the hands' estimate is valid
// The head-led turn: looking down (0/1), the head's yaw, the body's (degrees, the model's [0, 360)), re-seeds
extern "C" __declspec(dllexport) float CyberpunkVR_DebugHeadLedTurn[4] = {};
// The body's turning speed (deg/s), upright and looking down alike; Runtimes/HeadLedBodyTurn.hpp kDefaultBodyTurnConfig
extern "C" __declspec(dllexport) int CyberpunkVR_BodyTurnSpeedDps = 300;
// The upright cone (deg): a look within it of where the body last re-aimed leaves the body; kDefaultBodyTurnConfig
extern "C" __declspec(dllexport) int CyberpunkVR_BodyUprightConeDeg = 10;
// The hands' estimator's last update (TrackedBodyYaw::Trace): delta, wristMean, headDelta, headLead, accepted, evidence,
// residual, target (radians / seconds / metres), then the gate bits as a float-sized integer
extern "C" __declspec(dllexport) float CyberpunkVR_DebugTrackedTrace[8] = {};
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugTrackedGates = 0;

namespace {
cvr::roomscale::BodyYawFollower s_yawFollower;
cvr::body::TrackedBodyYaw s_trackedBody;
cvr::body::RotationMode s_bodyRotationMode=cvr::body::RotationMode::Off;
cvr::body::HybridBodyYaw s_hybridBody;
cvr::body::HeadLedBodyTurn s_headLedBody;
std::mutex s_yawFollowerMutex;
std::atomic<uint32_t> s_nativePairSeq{0};
std::mutex s_nativePairWriter;
struct AtomicNativePair {
    std::atomic<float> camQuat[4]{},entityQuat[4]{},cameraMinusEntity[3]{};
    std::atomic<uint32_t> valid{0},consumerEpoch{0}; // epoch is diagnostic only
    std::atomic<float> bodyCameraMinusEntity[3]{};
    std::atomic<uint32_t> unavailable{0};
    std::atomic<uintptr_t> owner{0};
    std::atomic<uint64_t> origin{0},stampMs{0};
};
AtomicNativePair s_nativePair{};
struct CurrentBodyFrame {
    std::mutex mutex;
    float position[3]{},rotation[4]{0,0,0,1},trackingYaw{};
    uintptr_t owner{};
    uint64_t stamp{};
} s_currentBodyFrame;
}

bool VRIK_ReadCurrentBodyFrame(float* p,float* q,float* trackingYaw) {
    std::lock_guard lock(s_currentBodyFrame.mutex);
    const auto now=GetTickCount64();
    if(!s_currentBodyFrame.owner || s_currentBodyFrame.owner!=cvr::roomscale::PlayerIdentity() ||
       now<s_currentBodyFrame.stamp || now-s_currentBodyFrame.stamp>250)return false;
    for(int k=0;k<3;++k)p[k]=s_currentBodyFrame.position[k];
    for(int k=0;k<4;++k)q[k]=s_currentBodyFrame.rotation[k];
    *trackingYaw=s_currentBodyFrame.trackingYaw;return true;
}

void VRIK_PublishNativeCameraPair(const int32_t* centre,const int32_t* bodyBase,
        const int32_t* entity,const float* cameraQuat,const float* entityQuat,
        uintptr_t owner,uint64_t origin,bool ownerStable) {
    using namespace cvr::camera;
    const auto pair=MakeCameraEntitySample({centre[0],centre[1],centre[2]},
        {bodyBase[0],bodyBase[1],bodyBase[2]},{entity[0],entity[1],entity[2]},cameraQuat,entityQuat);
    std::lock_guard lock(s_nativePairWriter);
    s_nativePairSeq.fetch_add(1,std::memory_order_acq_rel);
    for(int k=0;k<4;++k) {
        s_nativePair.camQuat[k].store(pair.cameraQuat[k],std::memory_order_relaxed);
        s_nativePair.entityQuat[k].store(pair.entityQuat[k],std::memory_order_relaxed);
    }
    for(int k=0;k<3;++k) {
        s_nativePair.cameraMinusEntity[k].store(pair.cameraMinusEntity[k],std::memory_order_relaxed);
        s_nativePair.bodyCameraMinusEntity[k].store(pair.bodyMinusEntity[k],std::memory_order_relaxed);
    }
    s_nativePair.valid.store(ownerStable && pair.valid,std::memory_order_relaxed);
    s_nativePair.unavailable.store(!ownerStable,std::memory_order_relaxed);
    s_nativePair.consumerEpoch.store(g_VrikFrameEpoch.load(std::memory_order_relaxed),std::memory_order_relaxed);
    s_nativePair.owner.store(owner,std::memory_order_relaxed);
    s_nativePair.origin.store(origin,std::memory_order_relaxed);
    s_nativePair.stampMs.store(GetTickCount64(),std::memory_order_relaxed);
    s_nativePairSeq.fetch_add(1,std::memory_order_release);
    if(ownerStable && pair.valid)CVR_DIAGNOSTIC(++CyberpunkVR_DebugVrikNativePairPublished);
    else CVR_DIAGNOSTIC(++CyberpunkVR_DebugVrikNativePairRejected);
}

bool VRIK_ReadNativeTransformSnapshot(VrikTransformSnapshot* out) {
    if(!out)return false;
    for(int attempt=0;attempt<4;++attempt) {
        const uint32_t before=s_nativePairSeq.load(std::memory_order_acquire);
        if(!before)return false;
        if(before&1u)continue;
        VrikTransformSnapshot value{};
        for(int k=0;k<4;++k) {
            value.camQuat[k]=s_nativePair.camQuat[k].load(std::memory_order_relaxed);
            value.entityQuat[k]=s_nativePair.entityQuat[k].load(std::memory_order_relaxed);
        }
        for(int k=0;k<3;++k) {
            value.cameraMinusEntity[k]=s_nativePair.cameraMinusEntity[k].load(std::memory_order_relaxed);
            value.bodyCameraMinusEntity[k]=s_nativePair.bodyCameraMinusEntity[k].load(std::memory_order_relaxed);
        }
        value.valid=s_nativePair.valid.load(std::memory_order_relaxed);
        value.unavailable=s_nativePair.unavailable.load(std::memory_order_relaxed);
        const auto owner=s_nativePair.owner.load(std::memory_order_relaxed);
        const auto origin=s_nativePair.origin.load(std::memory_order_relaxed);
        const auto stamp=s_nativePair.stampMs.load(std::memory_order_relaxed);
        if(before!=s_nativePairSeq.load(std::memory_order_acquire))continue;
        using namespace cvr::camera;
        const auto state=CameraPacketStatus(owner,origin,stamp,cvr::roomscale::PlayerIdentity(),
            OpenXRManager::Get().GetTrackingOriginSerial(),GetTickCount64(),value.valid!=0,value.unavailable!=0);
        value.valid=state==CameraPacketState::Valid;
        value.unavailable=state==CameraPacketState::Unavailable;
        *out=value;return true;
    }
    return false;
}

// Legacy export name retained for tooling: committed body-to-tracking angle.
extern "C" __declspec(dllexport) float CyberpunkVR_BodyYawRealignRad = 0.0f;

// Readable live: the same realign in degrees, the head-against-body residual, and the two counters.
extern "C" __declspec(dllexport) float    CyberpunkVR_DebugBodyFollowOffsetDeg = 0.0f;
extern "C" __declspec(dllexport) float    CyberpunkVR_DebugBodyFollowErrDeg = 0.0f;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugBodyFollowCalls = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugBodyFollowApplied = 0;

// THE BODY'S TRUE YAW, radians, game convention -- the engine's own value, read at the store site.
// The pose path converts world->model with this while the follower is on: model space IS the entity
// frame, so the entity's actual angle is the only correct converter, and the camera heading (which we
// expressed in the independent tracking frame) is not it.
extern "C" __declspec(dllexport) float CyberpunkVR_BodyYawFinalRad = 0.0f;
extern "C" __declspec(dllexport) int   CyberpunkVR_BodyYawFinalValid = 0;
// The player's frame, published so nothing in the pose path has to ask CET for it.
extern "C" __declspec(dllexport) float CyberpunkVR_PlayerEntityPos[3]  = { 0.0f, 0.0f, 0.0f };
extern "C" __declspec(dllexport) float CyberpunkVR_PlayerEntityQuat[4] = { 0.0f, 0.0f, 0.0f, 1.0f };
extern "C" __declspec(dllexport) int   CyberpunkVR_PlayerEntityValid = 0;

namespace {

// The HMD's yaw relative to the recenter base, radians, about the XR vertical (+Y).
bool HeadYawRelBase(float* outYaw,float* outDownDegrees,float* outBend,uint64_t* origin,bool* yawDefined) {
    OpenXRHeadPose hp{};
    // This starts the next native move. Read the same input latch as CCT, not
    // the completed previous move that camera/IK consumers still own.
    if (!OpenXRManager::Get().AcquireFrameHeadSample(&hp,nullptr,nullptr,nullptr,false) || !hp.valid) return false;
    *origin = hp.originSerial;
    *outDownDegrees=cvr::roomscale::HeadDownDegrees(hp.oriX,hp.oriY,hp.oriZ,hp.oriW);
    *outBend=hp.bodyBend.angle;
    *yawDefined=cvr::roomscale::HeadYaw(hp.oriX, hp.oriY, hp.oriZ, hp.oriW, outYaw);
    const float norm=hp.oriX*hp.oriX+hp.oriY*hp.oriY+hp.oriZ*hp.oriZ+hp.oriW*hp.oriW;
    return std::isfinite(norm) && norm>.5f && norm<1.5f;
}

}  // namespace

// The solver runs at the native motion provider. Observed offset is feedback
// from its persistent quaternion, so skipped calls cannot accumulate phantom yaw.
namespace {
void PublishFollowerState() {
    CyberpunkVR_BodyYawFollow=s_yawFollower.Enabled() ? 1 : 0;
}
void RefreshFollowerState(float observedOffset) {
    const auto mode=static_cast<cvr::body::RotationMode>(g_liveControls.xrBodyRotationMode);
    s_yawFollower.SetEnabled(BodyHeadingReady() && mode!=cvr::body::RotationMode::Off &&
                            cvr::roomscale::PhysicalBodyHeadingOwned());
    if(mode!=s_bodyRotationMode || !s_yawFollower.Enabled()){
        s_trackedBody.Reset(observedOffset,mode!=cvr::body::RotationMode::Hybrid);
        s_hybridBody.Reset();s_headLedBody.Reset();s_bodyRotationMode=mode;
    }
    PublishFollowerState();
}
}

bool BodyYawFollowActive() {
    // Animation can ask before the next native move. Only that move owns the
    // committed offset needed to seed a mode transition; this query must not
    // reset the estimator from the previous frame's requested angle.
    return BodyHeadingReady() &&
        static_cast<cvr::body::RotationMode>(g_liveControls.xrBodyRotationMode)!=cvr::body::RotationMode::Off &&
        cvr::roomscale::PhysicalBodyHeadingOwned();
}
float BodyYawFollowOffset() {
    return BodyHeadingOffset();
}
extern "C" void BodyYawFollowRelease() {
    std::lock_guard lock(s_yawFollowerMutex);
    s_yawFollower.Reset();
    s_trackedBody.Reset(0,s_bodyRotationMode!=cvr::body::RotationMode::Hybrid);
    s_hybridBody.Reset();
    s_headLedBody.Reset();
    PublishFollowerState();
}

extern "C" float BodyYawFollowStep(float observedOffset) {
    // Acquire XR state before taking the follower lock: camera consumers may
    // read the offset while publishing that same XR frame.
    float hmdYaw{},headDownDegrees{},bodyBend{};
    uint64_t origin{};
    bool yawDefined=false;
    const bool haveHead=HeadYawRelBase(&hmdYaw,&headDownDegrees,&bodyBend,&origin,&yawDefined);
    cvr::body::TrackingFrame bodyFrame{};
    const bool haveBody=cvr::body::NeedsBodyTracking(static_cast<cvr::body::RotationMode>(g_liveControls.xrBodyRotationMode)) &&
        OpenXRManager::Get().GetBodyTrackingFrame(&bodyFrame);
    std::lock_guard lock(s_yawFollowerMutex);
    CVR_DIAGNOSTIC(++CyberpunkVR_DebugBodyFollowCalls);
    RefreshFollowerState(observedOffset);
    // Held while calibrating full-body tracking: the player turns the head to look at V beside them.
    if(!s_yawFollower.Enabled() || !cvr::roomscale::PhysicalBodyAllowed() || !haveHead || cvr::fbt::game::Calibrating()){
        s_trackedBody.Suspend();s_hybridBody.Suspend();return 0;
    }
    s_yawFollower.Observe(observedOffset,origin);
    const auto now=static_cast<uint64_t>(std::chrono::duration_cast<std::chrono::microseconds>(
        std::chrono::steady_clock::now().time_since_epoch()).count());
    CVR_DIAGNOSTIC(CyberpunkVR_DebugBodyFollowErrDeg=yawDefined?
        std::remainder(hmdYaw-s_yawFollower.Offset(),6.28318530718f)*57.2957795f:0);
    cvr::body::YawEstimate result{};
    if(cvr::body::NeedsBodyTracking(s_bodyRotationMode)){
        if(haveBody && bodyFrame.origin==origin) {
            s_trackedBody.trace.bits=0;
            result=s_trackedBody.Update(bodyFrame);
            {
                const auto& t=s_trackedBody.trace;
                const float v[8]{t.delta,t.wristMean,t.headDelta,t.headLead,t.accepted,t.evidence,t.residual,t.target};
                for(int i=0;i<8;++i)CyberpunkVR_DebugTrackedTrace[i]=v[i];
                CyberpunkVR_DebugTrackedGates=t.bits;
            }
            if(result.valid)result=s_trackedBody.Predict(now);
        }
        else s_trackedBody.Suspend();
    }
    if(s_bodyRotationMode==cvr::body::RotationMode::HeadLed){
        // The head-led turn (Runtimes/HeadLedBodyTurn.hpp): the head alone decides; no follow zone, no controllers.
        CyberpunkVR_BodyYawFollowDeadDeg=0;
        if(!yawDefined)return 0;
        const float bodyDeg=s_yawFollower.Offset()*57.2957795f;
        // started from the body as it stands; started again if the body was turned from outside (a reset, a teleport):
        // a missed apply is not that -- the model's own heading carries on and the next request catches the body up
        if(!s_headLedBody.Seeded() || std::fabs(cvr::body::TurnDeltaAngle(s_headLedBody.Output(),bodyDeg))>20.0f) {
            s_headLedBody.Seed(bodyDeg);
            CyberpunkVR_DebugHeadLedTurn[3]+=1;
        }
        const float dt=std::clamp(s_yawFollower.SinceLast(now),0.0f,.25f);
        const float pitch=-headDownDegrees;   // up positive
        auto config=cvr::body::kDefaultBodyTurnConfig;
        const int speed=CyberpunkVR_BodyTurnSpeedDps;
        if(speed>0 && speed<=3600)config.upperSpeedDps=config.lowerSpeedDps=float(speed);
        const int cone=CyberpunkVR_BodyUprightConeDeg;
        if(cone>0 && cone<=90)config.upperToleranceDeg=float(cone);
        const float out=s_headLedBody.Step(hmdYaw*57.2957795f,pitch,dt,config);
        CyberpunkVR_DebugHeadLedTurn[0]=pitch<cvr::body::kLookDownAngleDeg ? 1.0f:0.0f;
        CyberpunkVR_DebugHeadLedTurn[1]=cvr::body::TurnClampAxis(hmdYaw*57.2957795f);
        CyberpunkVR_DebugHeadLedTurn[2]=out;
        const float step=s_yawFollower.Set(out*.01745329252f,now,origin);
        PublishFollowerState();
        if(step!=0)CVR_DIAGNOSTIC(++CyberpunkVR_DebugBodyFollowApplied);
        return step;
    }
    if(s_bodyRotationMode==cvr::body::RotationMode::Hybrid){
        const auto command=s_hybridBody.Step({hmdYaw,headDownDegrees,bodyBend,s_yawFollower.Offset(),
            g_liveControls.xrBodyFreeLookDeg,result,yawDefined,result.valid && s_trackedBody.Ready(),now,origin,
            g_liveControls.xrBodyFreeLookDownDeg});
        CyberpunkVR_BodyYawFollowDeadDeg=command.coneDegrees;
        CyberpunkVR_DebugHybridTrackedWeight=command.trackedWeight;
        CyberpunkVR_DebugHybridHandsDeg=command.handsYaw*57.2957795f;
        CyberpunkVR_DebugHybridNeckPullDeg=command.neckPull*57.2957795f;
        CyberpunkVR_DebugHybridAlignDeg=command.alignment*57.2957795f;
        CyberpunkVR_DebugHybridEstimate=command.estimate ? 1:0;
        const float step=s_yawFollower.Step(command.targetYaw,0,now,origin);
        PublishFollowerState();
        if(step!=0)CVR_DIAGNOSTIC(++CyberpunkVR_DebugBodyFollowApplied);
        return step;
    }
    if(s_bodyRotationMode==cvr::body::RotationMode::Tracked){
        CyberpunkVR_BodyYawFollowDeadDeg=0; // no standing, look-down or swimming cone in this mode
        const float target=result.valid?result.yaw:s_yawFollower.Offset();
        const float step=s_yawFollower.Step(target,0,now,origin);
        PublishFollowerState();
        if(step!=0)CVR_DIAGNOSTIC(++CyberpunkVR_DebugBodyFollowApplied);
        return step;
    }
    if(!yawDefined)return 0;
    CyberpunkVR_BodyYawFollowDeadDeg=cvr::roomscale::BodyFreeLookCone(
        cvr::swimming::Active(),g_liveControls.xrBodyFreeLookSwimDeg,
        headDownDegrees,g_liveControls.xrBodyFreeLookDeg,g_liveControls.xrBodyFreeLookDownDeg,bodyBend);
    const float step=s_yawFollower.Step(hmdYaw,CyberpunkVR_BodyYawFollowDeadDeg*.01745329252f,now,origin);
    PublishFollowerState();
    if(step!=0)CVR_DIAGNOSTIC(++CyberpunkVR_DebugBodyFollowApplied);
    return step;
}

// Called from the body-yaw store site (src/Hooks/BodyYawCensus.cpp), once per frame for the player.
// Publishes the body's own transform for the pose path; writes nothing into the game.
// THE POSITION COMES FROM THE ENGINE, IN THE SAME INSTANT AS THE YAW.
//
// It used to be copied from g_VREntityPos*, the CET push, while the yaw beside it came straight off
// the store site. That was corrected here, but it did not make g_lastLocate* a matching camera:
// LocateCamera publishes after animation, so animation sees camera(N-1) beside this entity(N).
// VRIK_ComputeCamModel therefore no longer subtracts these two absolutes; it consumes the coherent
// relative pair from one SetVRPlayerYaw push. This current engine position remains the right source
// for the camera-mount and script consumers below.
extern "C" void BodyYawFollowTick(float engineZ, float engineW, const float* enginePos) {
    float wz = engineZ, ww = engineW;
    if (ww < 0.0f) { wz = -wz; ww = -ww; }
    if (wz == 0.0f && ww == 0.0f) return;
    if (!enginePos) return;
    const float yaw = 2.0f * std::atan2(wz, ww);
    const float h = yaw * 0.5f;
    const float currentEntityQuat[4] = { 0.0f, 0.0f, std::sin(h), std::cos(h) };
    {
        std::lock_guard lock(s_currentBodyFrame.mutex);
        for(int k=0;k<3;++k)s_currentBodyFrame.position[k]=enginePos[k];
        for(int k=0;k<4;++k)s_currentBodyFrame.rotation[k]=currentEntityQuat[k];
        s_currentBodyFrame.trackingYaw=BodyTrackingYaw(yaw);
        s_currentBodyFrame.owner=cvr::roomscale::PlayerIdentity();
        s_currentBodyFrame.stamp=GetTickCount64();
    }

    // Camera/owner pairing is published by the completed camera write itself.
    // This callback only publishes the current engine body orientation/position.
    CyberpunkVR_BodyYawFinalRad = yaw;
    CyberpunkVR_BodyYawFinalValid = 1;
    CyberpunkVR_PlayerEntityPos[0] = enginePos[0];
    CyberpunkVR_PlayerEntityPos[1] = enginePos[1];
    CyberpunkVR_PlayerEntityPos[2] = enginePos[2];
    CyberpunkVR_PlayerEntityQuat[0] = 0.0f;
    CyberpunkVR_PlayerEntityQuat[1] = 0.0f;
    CyberpunkVR_PlayerEntityQuat[2] = currentEntityQuat[2];
    CyberpunkVR_PlayerEntityQuat[3] = currentEntityQuat[3];
    CyberpunkVR_PlayerEntityValid = 1;
}

// FOR THE RECORD, a door that is real but not the one in: sub_140336390 calls [vt+0x40] on its state
// provider (vtable 0x142AEDBD8) right before storing the transform, handing it r8 = &position,
// r9 = &quaternion, and on that class the slot is a bare `retn` -- an adjust-my-transform hook the
// engine invokes every frame and nobody implements. Claiming it worked, but it never fired for the
// player (0 calls against 6552 counted player frames), so the player's state uses a provider of a
// different class. Still an extension point for other characters.
