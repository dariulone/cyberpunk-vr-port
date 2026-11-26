#pragma once
#include <array>
#include <cmath>
#include <cstdint>
#if CVR_RUST_ROOM
#include "Rust/CvrRust.h"
#endif

namespace cvr::camera {
// Called under the camera composition mutex. Only the selected camera
// contributes a native heading; VRCAM's attachment may face another direction.
// Vehicle MAIN and a takeover lens use separate instances and owner identities.
class VehicleStereoHeading {
public:
    struct Selection { float yaw{}; bool changed{}; };
#if CVR_RUST_ROOM
    // rust/crates/cvr-camera/src/vehicle.rs; the class keeps its fields (Rust has its layout).
    void SetSource(uintptr_t player, uintptr_t mainCamera) { cvr_vehicle_heading_set_source(this, player, mainCamera); }
    bool Observe(uintptr_t camera, const float* q, const float* entityQ) {
        return cvr_vehicle_heading_observe(this, camera, q, entityQ);
    }
    Selection ForFrame(uint64_t epoch, const float* entityQ, float publishedYaw, float nativeOffset) {
        Selection s;
        cvr_vehicle_heading_for_frame(this, epoch, entityQ, publishedYaw, nativeOffset, &s);
        return s;
    }
#else
    void SetSource(uintptr_t player, uintptr_t mainCamera) {
        if (m_player == player && m_mainCamera == mainCamera) return;
        m_player = player;
        m_mainCamera = mainCamera;
        m_nativeValid = false;
        m_frameValid = false;
    }

    bool Observe(uintptr_t camera, const float* q, const float* entityQ) {
        if (!m_player || !m_mainCamera || camera != m_mainCamera) return false;
        Quaternion native{},entity{};
        if (!Normalize(q,native) || !Normalize(entityQ,entity) || !Yaw(native,m_nativeYaw)) return false;
        // Both values come from the same native MAIN update, before VR writes.
        // Preserve the camera's aim relative to its vehicle-driven owner.
        m_local = Multiply({-entity[0],-entity[1],-entity[2],entity[3]},native);
        m_nativeValid = true;
        return true;
    }

    Selection ForFrame(uint64_t epoch, const float* entityQ, float publishedYaw, float nativeOffset) {
        Quaternion entity{};
        const bool haveEntity=Normalize(entityQ,entity);
        // The head sample may be reused while the car keeps turning. The common
        // owner transform is a second part of the frame identity, not an eye's
        // independent native heading. Both camera callbacks read that same owner.
        // A transient owner-read failure is not a new world frame and must not
        // give one eye a fallback base midway through the shared sample.
        if (epoch && m_frameValid && epoch == m_epoch &&
            (!haveEntity || !m_frameEntityValid || SameRotation(entity,m_frameEntity))) return {m_frameYaw,false};
        float yaw=m_nativeYaw;
        if (m_nativeValid && haveEntity) {
            const auto world=Multiply(entity,m_local);
            if (Yaw(world,yaw)) m_nativeYaw=yaw;
        }
        m_frameYaw = m_nativeValid ? yaw + nativeOffset : publishedYaw;
        m_frameEntity=entity;
        m_frameEntityValid=haveEntity;
        m_epoch = epoch;
        m_frameValid = true;
        return {m_frameYaw,true};
    }
#endif

private:
    using Quaternion=std::array<float,4>;
    static bool Normalize(const float* q,Quaternion& out) {
        if (!q) return false;
        const float n=q[0]*q[0]+q[1]*q[1]+q[2]*q[2]+q[3]*q[3];
        if (!std::isfinite(n) || n<=.25f || n>=4.0f) return false;
        const float inv=1.0f/std::sqrt(n);
        for(int i=0;i<4;++i)out[i]=q[i]*inv;
        return true;
    }
    static Quaternion Multiply(const Quaternion& a,const Quaternion& b) {
        return {a[3]*b[0]+a[0]*b[3]+a[1]*b[2]-a[2]*b[1],
                a[3]*b[1]-a[0]*b[2]+a[1]*b[3]+a[2]*b[0],
                a[3]*b[2]+a[0]*b[1]-a[1]*b[0]+a[2]*b[3],
                a[3]*b[3]-a[0]*b[0]-a[1]*b[1]-a[2]*b[2]};
    }
    static bool Yaw(const Quaternion& q,float& yaw) {
        const float fx=2.0f*(q[0]*q[1]-q[2]*q[3]);
        const float fy=1.0f-2.0f*(q[0]*q[0]+q[2]*q[2]);
        if(fx*fx+fy*fy<=1.0e-6f)return false;
        yaw=std::atan2(-fx,fy);
        return true;
    }
    static bool SameRotation(const Quaternion& a,const Quaternion& b) {
        return a==b || a==Quaternion{-b[0],-b[1],-b[2],-b[3]};
    }
    uintptr_t m_player{}, m_mainCamera{};
    uint64_t m_epoch{};
    Quaternion m_local{},m_frameEntity{};
    float m_nativeYaw{}, m_frameYaw{};
    bool m_nativeValid{}, m_frameValid{}, m_frameEntityValid{};
};
#if CVR_RUST_ROOM
static_assert(sizeof(VehicleStereoHeading) == 72 && sizeof(VehicleStereoHeading::Selection) == 8);
#endif
}
