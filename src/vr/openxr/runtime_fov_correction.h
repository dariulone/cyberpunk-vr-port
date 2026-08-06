#pragma once

#include <cmath>

#include <openxr/openxr.h>

// Runtime frustum correction helpers for the game camera/projection path.
// Some runtimes expose slightly asymmetric per-eye frusta; the game camera code
// behaves better when those frusta are recentred while the visible runtime view
// stays unchanged.

struct RuntimeFovCorrection {
    XrFovf eye[2]{};
    float yawDeltaRad = 0.0f;    // left eye +yawDelta, right eye -yawDelta
    float pitchDeltaRad = 0.0f;  // both eyes pitch upward by +pitchDelta
    bool yawEnabled = false;
    bool pitchEnabled = false;
};

inline constexpr float kRuntimeFovDeltaThresholdRad = 0.017000001f;

inline RuntimeFovCorrection ComputeRuntimeFovCorrection(const XrFovf& left, const XrFovf& right) {
    RuntimeFovCorrection out{};
    out.eye[0] = left;
    out.eye[1] = right;

    const float upL = left.angleUp;
    const float downL = -left.angleDown;
    const float upR = right.angleUp;
    const float downR = -right.angleDown;
    const float deltaV = ((upL + upR) - (downL + downR)) * 0.25f;
    if (std::fabs(deltaV) > kRuntimeFovDeltaThresholdRad) {
        out.pitchEnabled = false;
        out.pitchDeltaRad = deltaV;
        out.eye[0].angleUp -= deltaV;
        out.eye[0].angleDown -= deltaV;
        out.eye[1].angleUp -= deltaV;
        out.eye[1].angleDown -= deltaV;
    }

    const float leftL = -left.angleLeft;
    const float rightL = left.angleRight;
    const float leftR = -right.angleLeft;
    const float rightR = right.angleRight;
    const float deltaH = ((leftL + rightR) - (leftR + rightL)) * 0.25f;
    if (std::fabs(deltaH) > kRuntimeFovDeltaThresholdRad) {
        out.yawEnabled = false;
        out.yawDeltaRad = deltaH;
        out.eye[0].angleLeft += deltaH;
        out.eye[0].angleRight += deltaH;
        out.eye[1].angleLeft -= deltaH;
        out.eye[1].angleRight -= deltaH;
    }

    return out;
}

inline float GetCorrectedGameHorizontalFovDeg(const RuntimeFovCorrection& corr) {
    const float h0 = corr.eye[0].angleRight - corr.eye[0].angleLeft;
    const float h1 = corr.eye[1].angleRight - corr.eye[1].angleLeft;
    return ((h0 + h1) * 0.5f) * (180.0f / 3.1415926535f);
}

// The panel's largest half-angle on each axis, taken across BOTH eyes: the engine renders one FOV
// scalar for both views, so a frustum that covers has to cover the worse eye.
inline void GetPanelHalfAngles(const XrFovf& left, const XrFovf& right, float* halfH, float* halfV) {
    *halfH = std::fmax(
        std::fmax(std::fabs(left.angleLeft), std::fabs(left.angleRight)),
        std::fmax(std::fabs(right.angleLeft), std::fabs(right.angleRight)));
    *halfV = std::fmax(
        std::fmax(std::fabs(left.angleUp), std::fabs(left.angleDown)),
        std::fmax(std::fabs(right.angleUp), std::fabs(right.angleDown)));
}

// COVER THE PANEL, DON'T MATCH ITS SPAN.
//
// The engine renders ONE frustum, symmetric about the camera axis, and derives the vertical from the
// render target's aspect -- there is no off-axis/shear term to give it. A canted headset's panel is
// not symmetric about the eye axis: a Quest 3 reports -54/+40 horizontally and +44/-55 vertically,
// i.e. frusta rotated 7 deg outward and 5.5 deg down. ComputeRuntimeFovCorrection recentres the
// FRUSTUM onto the eye axis, but nothing rotates the POSE to match, so a frustum sized to the
// panel's SPAN (2 x 47 deg) sits 7 deg short of the outer edge and 5 deg short of the bottom. Those
// gaps are the black border, and the matching slivers on the inner edge and top are rendered and
// thrown away.
//
// Sizing to the panel's largest HALF-angle instead covers every edge. The inner slice still falls
// outside the panel and is never shown -- that is the cost, and it is proportional to the cant, so a
// symmetric headset pays nothing. R.E.A.L. VR reaches the same place from the other end: it renders
// wider than the panel and crops the submitted rectangle.
//
// The vertical is not free to choose -- the engine derives V from H through the render aspect -- so H
// has to satisfy the vertical requirement as well, which is the second term below.
//
// Takes the max across BOTH eyes because the engine renders a single FOV scalar for both views. On a
// symmetric headset the largest half-angle IS half the span, so this returns exactly what
// GetCorrectedGameHorizontalFovDeg returns and nothing changes. Returns 0 when the inputs cannot
// produce a sane frustum, so callers keep their existing fallback.
inline float GetPanelCoveringHorizontalFovDeg(const XrFovf& left, const XrFovf& right, float aspect) {
    if (!(aspect > 0.01f && aspect < 100.0f)) {
        return 0.0f;
    }

    float halfH = 0.0f, halfV = 0.0f;
    GetPanelHalfAngles(left, right, &halfH, &halfV);
    if (!(halfH > 0.0f) || !(halfV > 0.0f)) {
        return 0.0f;
    }

    // tan(H/2) = tan(V/2) * aspect, so the H that just reaches halfV is atan(tan(halfV) * aspect).
    const float halfRad = std::fmax(halfH, std::atan(std::tan(halfV) * aspect));
    const float deg = 2.0f * halfRad * (180.0f / 3.1415926535f);
    return (deg > 1.0f && deg < 170.0f) ? deg : 0.0f;
}
