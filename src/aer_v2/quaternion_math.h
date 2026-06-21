// Quaternion math for AER V2.
//
// Used by the warp path to convert the predicted head pose (quaternion +
// translation) into the per-eye clip-space matrix that drives the CUDA warp
// kernel's UV remap.
//
// Conventions:
//   quaternion layout = (x, y, z, w)
//   matrix layout     = row-major 4x4 (16 floats), compatible with D3D / HLSL
//   All functions take pointer-to-float (no struct dependency) so they slot
//   directly next to XrPosef / XrQuaternionf without conversion.
#pragma once

#include <cstdint>

namespace aer_v2 {

// Quaternion as contiguous (x, y, z, w). Matches XrQuaternionf.
struct Quat { float x, y, z, w; };
struct Vec3 { float x, y, z; };

// Hamilton product out = a ⊗ b.
// Arithmetic:
//   out.x = a.w*b.x + a.x*b.w + a.y*b.z - a.z*b.y
//   out.y = a.w*b.y - a.x*b.z + a.y*b.w + a.z*b.x
//   out.z = a.w*b.z + a.x*b.y - a.y*b.x + a.z*b.w
//   out.w = a.w*b.w - a.x*b.x - a.y*b.y - a.z*b.z
inline void quatMul(const Quat& a, const Quat& b, Quat* out) {
    const float ax = a.x, ay = a.y, az = a.z, aw = a.w;
    const float bx = b.x, by = b.y, bz = b.z, bw = b.w;
    out->x = aw*bx + ax*bw + ay*bz - az*by;
    out->y = aw*by - ax*bz + ay*bw + az*bx;
    out->z = aw*bz + ax*by - ay*bx + az*bw;
    out->w = aw*bw - ax*bx - ay*by - az*bz;
}

// Quaternion -> row-major 4x4 rotation matrix.
// Output is 16 floats; translation column (m[3], m[7], m[11]) is zero, m[15]=1.
// For a unit quaternion the diagonal reduces to the textbook
//   m[0]  = 1 - 2(y² + z²)
//   m[5]  = 1 - 2(x² + z²)
//   m[10] = 1 - 2(x² + y²)
// Using the non-normalized form keeps the unit-quaternion result without an
// extra normalize step.
inline void quatToMatrix(const Quat& q, float* m16) {
    const float xx = q.x*q.x;
    const float yy = q.y*q.y;
    const float zz = q.z*q.z;
    const float ww = q.w*q.w;
    const float xy = q.x*q.y;
    const float xz = q.x*q.z;
    const float yz = q.y*q.z;
    const float wx = q.w*q.x;
    const float wy = q.w*q.y;
    const float wz = q.w*q.z;

    // Row 0
    m16[0]  = (ww + xx) - yy - zz;
    m16[1]  = 2.0f * (xy - wz);
    m16[2]  = 2.0f * (xz + wy);
    m16[3]  = 0.0f;
    // Row 1
    m16[4]  = 2.0f * (xy + wz);
    m16[5]  = (ww - xx + yy) - zz;
    m16[6]  = 2.0f * (yz - wx);
    m16[7]  = 0.0f;
    // Row 2
    m16[8]  = 2.0f * (xz - wy);
    m16[9]  = 2.0f * (yz + wx);
    m16[10] = (ww - xx - yy) + zz;
    m16[11] = 0.0f;
    // Row 3
    m16[12] = 0.0f;
    m16[13] = 0.0f;
    m16[14] = 0.0f;
    m16[15] = 1.0f;
}

// Conjugate (== inverse for unit quaternion). Used by the per-frame warp loop
// to build the negated-translation + inverse-rotation transform that maps a
// rendered eye into the predicted-pose frame (the warp source→target delta).
inline void quatConjugate(const Quat& q, Quat* out) {
    out->x = -q.x;
    out->y = -q.y;
    out->z = -q.z;
    out->w =  q.w;
}

// Rotate vector v by quaternion q:  v' = q ⊗ (0,v) ⊗ q⁻¹.
// Branchless cross-product form: t = 2·(q.xyz × v); v' = v + q.w·t + q.xyz × t.
inline Vec3 quatRotateVec(const Quat& q, const Vec3& v) {
    const float tx = 2.0f * (q.y * v.z - q.z * v.y);
    const float ty = 2.0f * (q.z * v.x - q.x * v.z);
    const float tz = 2.0f * (q.x * v.y - q.y * v.x);
    return Vec3{
        v.x + q.w * tx + (q.y * tz - q.z * ty),
        v.y + q.w * ty + (q.z * tx - q.x * tz),
        v.z + q.w * tz + (q.x * ty - q.y * tx)};
}

// Build the 4x4 rigid transform used as the warp source->target matrix.
// 'renderQ/renderPos' = the pose the eye was actually rendered with (from the
// camera hook); 'targetQ/targetPos' = the latest predicted pose the synthetic
// eye should represent. The warp kernel then samples the rendered color at the
// UV implied by (M * clipPosition).
inline void buildWarpTransform(const Quat& renderQ, const Vec3& renderPos,
                               const Quat& targetQ, const Vec3& targetPos,
                               float* m16) {
    // Delta rotation: q_delta = q_target⁻¹ ⊗ q_render  (rotates render→target).
    Quat targetInv;
    quatConjugate(targetQ, &targetInv);
    Quat delta;
    quatMul(targetInv, renderQ, &delta);
    quatToMatrix(delta, m16);
    // Delta translation expressed IN THE TARGET VIEW FRAME. A point maps as
    //   p_target = R_target⁻¹·R_render·p_render + R_target⁻¹·(t_render - t_target).
    // The rotation above is R_target⁻¹·R_render; the translation must likewise be
    // rotated into the target frame by R_target⁻¹ (= targetInv), NOT left in world
    // axes. (Previously the raw world delta was baked in, which skewed the Z-
    // reprojection during positional head motion / walking.)
    const Vec3 worldDelta{renderPos.x - targetPos.x,
                          renderPos.y - targetPos.y,
                          renderPos.z - targetPos.z};
    const Vec3 viewDelta = quatRotateVec(targetInv, worldDelta);
    m16[12] = viewDelta.x;
    m16[13] = viewDelta.y;
    m16[14] = viewDelta.z;
}

} // namespace aer_v2
