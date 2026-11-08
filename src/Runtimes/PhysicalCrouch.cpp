#include "Runtimes/OpenXRManager.hpp"
#include "Core/LiveControls.hpp"
#include "Hooks/RoomscaleMove.hpp"
#include "Anim/VrikState.hpp"
#include "Camera/CameraState.hpp"
#include <chrono>

extern "C" {
__declspec(dllexport) float CyberpunkVR_PhysicalCrouchDepth=0;
__declspec(dllexport) int CyberpunkVR_PhysicalCrouchDesired=0;
__declspec(dllexport) int CyberpunkVR_PhysicalCrouchApplied=0;
__declspec(dllexport) int CyberpunkVR_PhysicalCrouchActual=0;
// 1: while the body walks after the head the bend tracker reads a bob as a step (Anim/BodyBend.hpp BendWalkBobDrop)
__declspec(dllexport) int CyberpunkVR_BendWalkMode=1;
}
namespace {
uint64_t NowUs() {
    return static_cast<uint64_t>(std::chrono::duration_cast<std::chrono::microseconds>(
        std::chrono::steady_clock::now().time_since_epoch()).count());
}
bool CrouchContext() {
    return g_liveControls.xrPhysicalCrouch && cvr::roomscale::PlayerIdentity() &&
        cvr::roomscale::PoseFrameAllowed() && !cvr::swimming::Active() && !cvr::ladder::Active();
}
bool Fresh(uint64_t now,uint64_t stamp,uint64_t maxAge) {
    return stamp && now>=stamp && now-stamp<=maxAge;
}
}
void OpenXRManager::SetPhysicalCrouchBridgeReady(bool ready) {
    m_crouchBridgeStampUs.store(ready ? NowUs():0,std::memory_order_release);
}
void OpenXRManager::UpdateBodyPosture(OpenXRHeadPose& p) {
    m_bodyBendTracker.SetSquatDepth(cvr::body::CrouchThreshold(g_liveControls.xrPhysicalCrouchDepth));
    p.bodyBend=m_bodyBendTracker.Update({p.posX,p.posY,p.posZ},
        {p.oriX,p.oriY,p.oriZ,p.oriW},p.originSerial,cvr::swimming::Active() || cvr::ladder::Active(),
        CyberpunkVR_BendWalkMode!=0 && cvr::roomscale::BodyFollowing());
    p.crouchDepth=cvr::body::CrouchDepth(m_bodyBendTracker.NeckDrop(),p.bodyBend);
    const auto now=NowUs();
    const float norm=p.oriX*p.oriX+p.oriY*p.oriY+p.oriZ*p.oriZ+p.oriW*p.oriW;
    const bool valid=p.valid && std::isfinite(p.posX) && std::isfinite(p.posY) && std::isfinite(p.posZ) &&
        std::isfinite(norm) && norm>.5f && norm<1.5f;
    bool allowed=valid && CrouchContext() && Fresh(now,m_crouchBridgeStampUs.load(std::memory_order_acquire),500000);
    if(allowed) {
        // A cached/fallback view pose can remain valid after positional tracking
        // is lost. It must not refresh the age of a held gameplay crouch.
        XrSpaceLocation tracking{XR_TYPE_SPACE_LOCATION};
        const auto aim=GetFrameAimTimeNow();
        constexpr auto required=XR_SPACE_LOCATION_POSITION_VALID_BIT|XR_SPACE_LOCATION_ORIENTATION_VALID_BIT|
            XR_SPACE_LOCATION_POSITION_TRACKED_BIT|XR_SPACE_LOCATION_ORIENTATION_TRACKED_BIT;
        allowed=m_sessionRunning.load(std::memory_order_relaxed) && aim>0 &&
            m_viewSpace!=XR_NULL_HANDLE && m_localSpace!=XR_NULL_HANDLE &&
            XR_SUCCEEDED(xrLocateSpace(m_viewSpace,m_localSpace,aim,&tracking)) &&
            (tracking.locationFlags&required)==required;
    }
    p.physicalCrouch=m_physicalCrouch.Update(p.crouchDepth,g_liveControls.xrPhysicalCrouchDepth,
        now,p.originSerial,cvr::roomscale::PlayerIdentity(),allowed);
    // (The gaze-only floor-reach inference has handed over to the squat by the crouch's depth -- BendTracker::Update --
    // so the crouch coming on no longer replaces it: that replacement was a one-frame jump of the torso and hips.)
    CyberpunkVR_PhysicalCrouchDepth=p.crouchDepth;
    CyberpunkVR_PhysicalCrouchDesired=p.physicalCrouch ? 1:0;
}
bool OpenXRManager::PhysicalCrouchRequested() {
    const auto now=NowUs();
    if(!CrouchContext() || !Fresh(now,m_crouchBridgeStampUs.load(std::memory_order_acquire),500000))return false;
    std::lock_guard lock(m_frameSampleMutex);
    return m_frameSample.valid && m_frameSample.originSerial==GetTrackingOriginSerial() &&
        Fresh(now,m_frameSampleStampUs,150000) && m_frameSample.physicalCrouch;
}
