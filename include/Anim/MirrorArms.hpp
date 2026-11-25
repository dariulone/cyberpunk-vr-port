#pragma once
#include "Anim/MirrorPose.hpp"
#include <DirectXMath.h>
#include <algorithm>
#include <array>
#include <cstdint>

namespace cvr::mirror {
// Indices from wrist through elbow and upper arm to the rig root, resolved on bind.
struct ArmChain {
    std::array<int, 32> bones{};
    int count = 0;
};

inline ArmChain ArmPath(const int16_t* parents, int count, int wrist, int elbow, int upper) {
    ArmChain path;
    for (int i = wrist; i >= 0; i = parents[i]) {
        if (i >= count || parents[i] >= i || parents[i] < -1 || path.count == path.bones.size()) return {};
        path.bones[path.count++] = i;
    }
    if (path.count < 4 || path.bones[1] != elbow || path.bones[2] != upper) return {};
    return path;
}

namespace arm {
using namespace DirectX;

inline XMVECTOR Compose(FXMVECTOR parent, FXMVECTOR local) {
    return XMQuaternionMultiply(local, parent);
}
inline bool Finite(FXMVECTOR v) { return !XMVector4IsNaN(v) && !XMVector4IsInfinite(v); }
struct Model { XMVECTOR position, rotation; };

inline bool Read(const ArmChain& path, const unsigned char* locals, Model (&end)[4]) {
    if (!path.count) return false;
    XMVECTOR position = XMVectorZero(), rotation = XMQuaternionIdentity(), scale = XMVectorSplatOne();
    for (int i = path.count - 1; i >= 0; --i) {
        Local local;
        std::memcpy(&local, locals + path.bones[i] * sizeof(Local), sizeof(Local));
        if (!Normalize(local.rotation)) return false;
        auto t = XMVectorSet(local.position.x, local.position.y, local.position.z, 0);
        auto q = XMVectorSet(local.rotation.x, local.rotation.y, local.rotation.z, local.rotation.w);
        position += XMVector3Rotate(t * scale, rotation);
        rotation = Compose(rotation, q);
        scale *= XMVectorSet(local.scale[0], local.scale[1], local.scale[2], 1);
        if (!Finite(position) || !Finite(scale)) return false;
        if (i < 4) end[i] = {position, rotation};
    }
    return true;
}

inline float Length(FXMVECTOR v) { return XMVectorGetX(XMVector3Length(v)); }
inline XMVECTOR Perpendicular(FXMVECTOR v, FXMVECTOR unitAxis) {
    return v - unitAxis * XMVector3Dot(v, unitAxis);
}
inline XMVECTOR FallbackAxis(FXMVECTOR axis) {
    return std::abs(XMVectorGetX(axis)) < .8f ? XMVectorSet(1, 0, 0, 0) : XMVectorSet(0, 1, 0, 0);
}

inline XMVECTOR Swing(FXMVECTOR a, FXMVECTOR b) {
    const auto from = XMVector3Normalize(a), to = XMVector3Normalize(b);
    const float cosine = XMVectorGetX(XMVector3Dot(from, to));
    if (cosine < -.999999f)
        return XMVectorSetW(XMVector3Normalize(XMVector3Cross(from, FallbackAxis(from))), 0);
    return XMQuaternionNormalize(XMVectorSetW(XMVector3Cross(from, to), 1 + cosine));
}
} // namespace arm

// Fit the mirror's two segments to the tracked endpoint. The source elbow
// selects the bend plane. Matching tracked hands share a model-space palm
// orientation. Other rigs keep the wrist orientation from rest-pose retargeting.
inline bool PlaceWrist(const ArmChain& from, const unsigned char* player,
                       const ArmChain& to, unsigned char* mirror, bool matchPalm = false) {
    using namespace DirectX;
    arm::Model src[4], dst[4];
    if (!arm::Read(from, player, src) || !arm::Read(to, mirror, dst)) return false;
    const auto base = dst[2].position;
    const auto upper = dst[1].position - base, lower = dst[0].position - dst[1].position;
    const float a = arm::Length(upper), b = arm::Length(lower);
    const auto reach = src[0].position - base;
    const float distance = arm::Length(reach);
    constexpr float epsilon = 1.e-5f;
    if (a <= epsilon || b <= epsilon) return false;
    const auto axis = XMVector3Normalize(distance > epsilon ? reach : upper);
    const float d = std::clamp(distance, std::max(std::abs(a - b), epsilon), a + b);
    auto pole = arm::Perpendicular(src[1].position - base, axis);
    if (arm::Length(pole) < epsilon) pole = arm::Perpendicular(upper, axis);
    if (arm::Length(pole) < epsilon) pole = arm::Perpendicular(arm::FallbackAxis(axis), axis);
    const float along = (a*a - b*b + d*d) / (2*d);
    const float height = std::sqrt(std::max(0.f, a*a - along*along));
    const auto toElbow = axis * along + XMVector3Normalize(pole) * height;
    const auto toWrist = axis * d - toElbow;

    const auto swing = arm::Swing(upper, toElbow);
    const auto upperRotation = arm::Compose(swing, dst[2].rotation);
    const auto forearmRotation = arm::Compose(arm::Swing(XMVector3Rotate(lower, swing), toWrist),
                                              arm::Compose(swing, dst[1].rotation));
    const XMVECTOR rotations[] = {
        arm::Compose(XMQuaternionConjugate(forearmRotation), matchPalm ? src[0].rotation : dst[0].rotation),
        arm::Compose(XMQuaternionConjugate(upperRotation), forearmRotation),
        arm::Compose(XMQuaternionConjugate(dst[3].rotation), upperRotation)
    };
    for (auto q : rotations) if (!arm::Finite(q)) return false;
    for (int i = 0; i < 3; ++i) {
        XMFLOAT4 q;
        XMStoreFloat4(&q, XMQuaternionNormalize(rotations[i]));
        std::memcpy(mirror + to.bones[i] * sizeof(Local) + offsetof(Local, rotation), &q, sizeof(q));
    }
    return true;
}
} // namespace cvr::mirror
