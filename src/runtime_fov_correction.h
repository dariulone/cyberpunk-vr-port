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
        out.pitchEnabled = true;
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
        out.yawEnabled = true;
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
