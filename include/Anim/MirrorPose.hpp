#pragma once
#include "Anim/SkeletonPolicy.hpp"
#include "Utils/XrMath.hpp"
#include <cstddef>
#include <cstring>

namespace cvr::mirror {
enum class Motion { Native, Rotation, RootMotion };

// Joint names on the root rig. Face, shape, muscle, and garment bones continue
// to use the mirror puppet's evaluated local pose underneath these joints.
inline Motion MotionFor(std::string_view name) {
    if (name.starts_with("shadow_")) name.remove_prefix(7);
    if (name == "Hips" || name == "Trajectory" || name == "Root") return Motion::RootMotion;
    if (name == "Head" || name == "Neck" || name == "Neck1" || cvr::vrik::IsPrimarySpine(name) ||
        cvr::vrik::ForearmTwistSlot(name, true) >= 0 || cvr::vrik::ForearmTwistSlot(name, false) >= 0)
        return Motion::Rotation;
    if (name.starts_with("Left")) name.remove_prefix(4);
    else if (name.starts_with("Right")) name.remove_prefix(5);
    else return Motion::Native;
    for (auto joint : {"Shoulder","Arm","ForeArm","Hand","HandEnd","UpLeg","Leg","Foot","Heel","ToeBase"})
        if (name == joint) return Motion::Rotation;
    bool inHand = name.starts_with("InHand");
    if (inHand) name.remove_prefix(6);
    else if (name.starts_with("Hand")) name.remove_prefix(4);
    else return Motion::Native;
    if (!inHand) {
        if (name.empty() || name.back() < '1' || name.back() > '3') return Motion::Native;
        name.remove_suffix(1);
    }
    for (auto digit : {"Thumb","Index","Middle","Ring","Pinky"})
        if (name == digit) return Motion::Rotation;
    return Motion::Native;
}

struct Local {
    XrVector3f position;
    float positionW;
    XrQuaternionf rotation;
    float scale[4];
};
static_assert(sizeof(Local) == 48 && offsetof(Local, rotation) == 16);

inline bool Normalize(XrQuaternionf& q) {
    const float n = q.x*q.x + q.y*q.y + q.z*q.z + q.w*q.w;
    if (!std::isfinite(n) || n < .5f || n > 1.5f) return false;
    const float k = 1.f / std::sqrt(n);
    q = {q.x*k, q.y*k, q.z*k, q.w*k};
    return true;
}

struct Rest {
    XrQuaternionf parent, model;
    XrVector3f position;
    bool valid;
};

inline Rest ReadRest(const unsigned char* bytes, const Rest* parent) {
    Local t;
    std::memcpy(&t, bytes, sizeof(t));
    Rest r{};
    if ((parent && !parent->valid) || !Normalize(t.rotation) ||
        !std::isfinite(t.position.x) || !std::isfinite(t.position.y) || !std::isfinite(t.position.z)) return r;
    r.position = t.position;
    r.parent = parent ? parent->model : XrQuaternionf{0,0,0,1};
    r.model = MultiplyQuat(r.parent, t.rotation);
    r.valid = Normalize(r.model);
    return r;
}

struct Joint {
    int source = -1;
    Motion motion = Motion::Native;
    XrQuaternionf parentBasis{}, jointBasis{};
    XrVector3f sourceOrigin{}, targetOrigin{};
};

inline Joint Bind(int index, Motion motion, const Rest& from, const Rest& to) {
    if (motion == Motion::Native || !from.valid || !to.valid) return {};
    return {index, motion, MultiplyQuat(ConjugateQuat(to.parent), from.parent),
            MultiplyQuat(ConjugateQuat(from.model), to.model), from.position, to.position};
}

// The first-person and mirror rigs can share finger axes but have different
// rest curls. Check each segment's longitudinal axis and the five metacarpal
// frames at the wrist before using the player's finger rotations directly.
inline bool MatchingFingerFrames(const Local& a, const Local& b, bool metacarpal) {
    const auto dot = [](const XrVector3f& p, const XrVector3f& q) {
        return p.x*q.x + p.y*q.y + p.z*q.z;
    };
    const float lengths = std::sqrt(dot(a.position,a.position) * dot(b.position,b.position));
    if (!(lengths > 1e-8f && dot(a.position,b.position) >= .999f * lengths)) return false;
    if (!metacarpal) return true;
    auto p = a.rotation, q = b.rotation;
    return Normalize(p) && Normalize(q) && std::abs(p.x*q.x+p.y*q.y+p.z*q.z+p.w*q.w) >= .9998f;
}

inline void CopyFingerRotation(const unsigned char* source, unsigned char* target) {
    XrQuaternionf q;
    std::memcpy(&q, source + offsetof(Local, rotation), sizeof(q));
    if (Normalize(q)) std::memcpy(target + offsetof(Local, rotation), &q, sizeof(q));
}

inline void Apply(const Joint& joint, const unsigned char* source, unsigned char* target) {
    if (joint.source < 0) return;
    Local pose;
    std::memcpy(&pose, source, sizeof(pose));
    if (!Normalize(pose.rotation)) return;
    const auto q = MultiplyQuat(MultiplyQuat(joint.parentBasis, pose.rotation), joint.jointBasis);
    std::memcpy(target + offsetof(Local, rotation), &q, sizeof(q));
    if (joint.motion == Motion::RootMotion) {
        const auto d = RotateVector(joint.parentBasis, {pose.position.x - joint.sourceOrigin.x,
            pose.position.y - joint.sourceOrigin.y, pose.position.z - joint.sourceOrigin.z});
        if (std::isfinite(d.x) && std::isfinite(d.y) && std::isfinite(d.z)) {
            const XrVector3f p{joint.targetOrigin.x + d.x, joint.targetOrigin.y + d.y, joint.targetOrigin.z + d.z};
            std::memcpy(target, &p, sizeof(p));
        }
    }
}
}
