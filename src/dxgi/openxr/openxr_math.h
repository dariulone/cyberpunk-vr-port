#pragma once
// Pure quaternion / vector math extracted verbatim from openxr_manager.cpp.
// Header-only `inline` => a single shared definition across all OpenXR-module TUs
// (no linkage clash), and every existing call site stays unchanged. The function
// bodies are byte-for-byte identical to the originals — this is a pure relocation.
#include <openxr/openxr.h>
#include <cmath>

inline XrQuaternionf MultiplyQuat(const XrQuaternionf& a, const XrQuaternionf& b) {
    XrQuaternionf out{};
    out.x = a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y;
    out.y = a.w * b.y - a.x * b.z + a.y * b.w + a.z * b.x;
    out.z = a.w * b.z + a.x * b.y - a.y * b.x + a.z * b.w;
    out.w = a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z;
    return out;
}

inline XrQuaternionf ConjugateQuat(const XrQuaternionf& q) {
    XrQuaternionf out{ -q.x, -q.y, -q.z, q.w };
    return out;
}

inline XrQuaternionf NlerpQuat(const XrQuaternionf& a, const XrQuaternionf& b, float t) {
    float bx = b.x;
    float by = b.y;
    float bz = b.z;
    float bw = b.w;
    const float dot = a.x * bx + a.y * by + a.z * bz + a.w * bw;
    if (dot < 0.0f) {
        bx = -bx;
        by = -by;
        bz = -bz;
        bw = -bw;
    }

    XrQuaternionf out{
        a.x + (bx - a.x) * t,
        a.y + (by - a.y) * t,
        a.z + (bz - a.z) * t,
        a.w + (bw - a.w) * t};
    const float norm = sqrtf(out.x * out.x + out.y * out.y + out.z * out.z + out.w * out.w);
    if (norm > 1e-8f) {
        const float invNorm = 1.0f / norm;
        out.x *= invNorm;
        out.y *= invNorm;
        out.z *= invNorm;
        out.w *= invNorm;
    } else {
        out = a;
    }
    return out;
}

inline XrPosef ExtrapolatePose(const XrPosef& previous, const XrPosef& current, float t) {
    XrPosef out{};
    out.orientation = NlerpQuat(previous.orientation, current.orientation, t);
    out.position.x = previous.position.x + (current.position.x - previous.position.x) * t;
    out.position.y = previous.position.y + (current.position.y - previous.position.y) * t;
    out.position.z = previous.position.z + (current.position.z - previous.position.z) * t;
    return out;
}

inline XrVector3f RotateVector(const XrQuaternionf& q, const XrVector3f& v) {
    const XrQuaternionf pure{v.x, v.y, v.z, 0.0f};
    const XrQuaternionf rotated = MultiplyQuat(MultiplyQuat(q, pure), ConjugateQuat(q));
    return {rotated.x, rotated.y, rotated.z};
}
