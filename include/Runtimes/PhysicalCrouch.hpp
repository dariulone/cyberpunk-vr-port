#pragma once
#include "Anim/BodyBend.hpp"
#include <algorithm>
#include <cmath>
#include <cstdint>
#if CVR_RUST_ROOM
#include "Rust/CvrRust.h"
#endif

namespace cvr::body {
#if CVR_RUST_ROOM
// rust/crates/cvr-body/src/crouch.rs; the class keeps its fields (Rust has its layout).
inline float CrouchThreshold(float value) { return cvr_crouch_threshold(value); }
inline float CrouchDepth(float neckDrop,const BendSample& bend) { return cvr_crouch_depth(neckDrop,&bend); }
class PhysicalCrouch {
    uint64_t m_origin{},m_owner{},m_stamp{},m_candidateSince{};
    bool m_active{},m_candidate{};
public:
    void Reset(){cvr_crouch_reset(this);}
    bool Active() const{return m_active;}
    bool Update(float depth,float threshold,uint64_t now,uint64_t origin,uint64_t owner,bool allowed) {
        return cvr_crouch_update(this,depth,threshold,now,origin,owner,allowed);
    }
};
static_assert(sizeof(PhysicalCrouch) == 40);
#else
inline float CrouchThreshold(float value) {
    return std::isfinite(value) ? std::clamp(value,.15f,.55f):.45f;
}
// Only measured horizontal hinge travel can explain away a height loss.
// Downward gaze / the floor-reach fallback cannot turn a vertical squat into a bend.
inline float CrouchDepth(float neckDrop,const BendSample& bend) {
    const float span=std::min(std::hypot(bend.right,bend.forward),BendReach*std::sin(MaxBend));
    const float hinge=BendReach-std::sqrt(std::max(0.f,BendReach*BendReach-span*span));
    return std::max(0.f,neckDrop-hinge);
}
class PhysicalCrouch {
    uint64_t m_origin{},m_owner{},m_stamp{},m_candidateSince{};
    bool m_active{},m_candidate{};
public:
    void Reset(){*this={};}
    bool Active() const{return m_active;}
    bool Update(float depth,float threshold,uint64_t now,uint64_t origin,uint64_t owner,bool allowed) {
        if(!allowed || !origin || !owner || !now || !std::isfinite(depth) || depth>1.5f) {
            Reset();return false;
        }
        if(origin!=m_origin || owner!=m_owner || (m_stamp && (now<m_stamp || now-m_stamp>150000))) {
            Reset();m_origin=origin;m_owner=owner;
        }
        m_stamp=now;
        const float enter=CrouchThreshold(threshold);
        const bool candidate=m_active ? depth>enter*.60f:depth>=enter;
        if(candidate!=m_candidate){m_candidate=candidate;m_candidateSince=now;}
        if(candidate!=m_active && now-m_candidateSince>=80000)m_active=candidate;
        return m_active;
    }
};
#endif
}
