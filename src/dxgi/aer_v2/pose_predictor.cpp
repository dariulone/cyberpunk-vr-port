#include "pose_predictor.h"
#include "quaternion_math.h"

#include <cmath>

namespace aer_v2 {

// ---------------------------------------------------------------------------
// ScalarPredictor.
//
// We implement the fit over the full window using the standard
// least-squares normal-equation form, recomputed on each fill. Numerically
// equivalent in steady state and simpler to reason about:
//   slope     = Σ((x-x̄)(y-ȳ)) / Σ((x-x̄)²)
//   intercept = ȳ - x̄·slope
// Σdxdx == 0 (degenerate, e.g. timestamps identical) -> not valid.
// ---------------------------------------------------------------------------
void ScalarPredictor::AddSample(double time, double value) {
    const uint32_t idx = m_writeIdx % kRingSize;
    m_writeIdx = idx + 1;

    m_time[idx] = time;
    m_value[idx] = value;
    if (m_count < kRingSize) ++m_count;

    // Recompute the fit whenever the ring is full. Cheaper than a per-sample
    // incremental form and identically accurate for a 100-sample
    // window at 90 Hz (recompute cost ~1.3k flops every ~1.1s of fills, then
    // every sample in steady state — negligible vs the GPU warp).
    if (m_count >= kRingSize) {
        double sumX = 0.0, sumY = 0.0;
        for (uint32_t i = 0; i < kRingSize; ++i) {
            sumX += m_time[i];
            sumY += m_value[i];
        }
        const double meanX = sumX / kRingSize;
        const double meanY = sumY / kRingSize;
        double sxx = 0.0, sxy = 0.0;
        for (uint32_t i = 0; i < kRingSize; ++i) {
            const double dx = m_time[i] - meanX;
            sxx += dx * dx;
            sxy += dx * (m_value[i] - meanY);
        }
        if (sxx > 1e-12) {
            m_slope = sxy / sxx;
            m_intercept = meanY - meanX * m_slope;
            m_meanX = meanX;
            m_meanY = meanY;
            m_valid = true;
        }
    }
}

void ScalarPredictor::Reset() {
    m_writeIdx = 0;
    m_count = 0;
    m_sumX = m_sumY = m_sumXX = m_sumXY = 0.0;
    m_meanX = m_meanY = 0.0;
    m_slope = 0.0;
    m_intercept = 0.0;
    m_valid = false;
    for (uint32_t i = 0; i < kRingSize; ++i) { m_time[i] = 0.0; m_value[i] = 0.0; }
}

// ---------------------------------------------------------------------------
// PosePredictor
// ---------------------------------------------------------------------------
void PosePredictor::AddSample(double timeSeconds, const XrPosef& pose) {
    m_posX.AddSample(timeSeconds, pose.position.x);
    m_posY.AddSample(timeSeconds, pose.position.y);
    m_posZ.AddSample(timeSeconds, pose.position.z);

    // Orientation: keep the last two samples for an angular-velocity estimate.
    // Regressing quaternion components directly only holds for infinitesimal
    // rotations and tears during fast head turns. Integrating
    // a delta quaternion (q2 · q1⁻¹ -> axis/angle -> scale by dt) is correct
    // for arbitrary rotations within the sample window.
    m_lastQuat[0] = m_lastQuat[1];
    m_lastQuatTime[0] = m_lastQuatTime[1];
    m_lastQuat[1] = pose.orientation;
    m_lastQuatTime[1] = timeSeconds;
    ++m_lastCount;
}

XrPosef PosePredictor::Predict(double tSeconds) const {
    XrPosef out{};
    if (m_posX.IsValid()) {
        out.position.x = static_cast<float>(m_posX.Predict(tSeconds));
        out.position.y = static_cast<float>(m_posY.Predict(tSeconds));
        out.position.z = static_cast<float>(m_posZ.Predict(tSeconds));
    } else if (m_lastCount > 0) {
        // Warmup: not enough samples for a fit yet — hold the last position.
        out.position.x = static_cast<float>(m_posX.Intercept());
        out.position.y = static_cast<float>(m_posY.Intercept());
        out.position.z = static_cast<float>(m_posZ.Intercept());
    }

    // Orientation: advance the most recent sample by angularVelocity·dt.
    if (m_lastCount >= 2 && m_lastQuatTime[1] > m_lastQuatTime[0]) {
        const Quat q1{m_lastQuat[0].x, m_lastQuat[0].y, m_lastQuat[0].z, m_lastQuat[0].w};
        const Quat q2{m_lastQuat[1].x, m_lastQuat[1].y, m_lastQuat[1].z, m_lastQuat[1].w};
        Quat q1Inv;
        quatConjugate(q1, &q1Inv);
        Quat delta;
        quatMul(q2, q1Inv, &delta);   // rotation from q1 to q2 over dt0
        // Normalize delta (q1,q2 are unit so delta is ~unit; guard anyway).
        const float dn = std::sqrt(delta.x*delta.x + delta.y*delta.y +
                                   delta.z*delta.z + delta.w*delta.w);
        if (dn > 1e-9f) {
            const float inv = 1.0f / dn;
            delta.x *= inv; delta.y *= inv; delta.z *= inv; delta.w *= inv;
        }
        // delta represents rotation over dt0 = t1-t0. Scale to dt = tPredict-t1.
        const double dt0 = m_lastQuatTime[1] - m_lastQuatTime[0];
        const double dt = tSeconds - m_lastQuatTime[1];
        double scale = (dt0 > 1e-9) ? (dt / dt0) : 0.0;
        // Clamp the extrapolation: beyond ~2x the sample interval the axis-angle
        // scaling loses accuracy and overshoots; cap to keep the prediction sane.
        if (scale > 2.0) scale = 2.0;
        if (scale < -1.0) scale = -1.0;
        // Axis-angle: angle = 2·acos(|w|), axis = xyz/|xyz|; scale angle, rebuild.
        float wClamped = delta.w;
        if (wClamped > 1.0f) wClamped = 1.0f;
        if (wClamped < -1.0f) wClamped = -1.0f;
        const float halfAngle = std::acos(wClamped);
        const float s = std::sin(halfAngle);
        Quat scaledDelta;
        if (s > 1e-6f) {
            const float newHalf = halfAngle * static_cast<float>(scale);
            const float ns = std::sin(newHalf) / s;
            scaledDelta.x = delta.x * ns;
            scaledDelta.y = delta.y * ns;
            scaledDelta.z = delta.z * ns;
            scaledDelta.w = std::cos(newHalf);
        } else {
            // Near-identity delta: small-angle linear scale on xyz.
            scaledDelta.x = delta.x * static_cast<float>(scale);
            scaledDelta.y = delta.y * static_cast<float>(scale);
            scaledDelta.z = delta.z * static_cast<float>(scale);
            scaledDelta.w = 1.0f;
        }
        const Quat cur{m_lastQuat[1].x, m_lastQuat[1].y, m_lastQuat[1].z, m_lastQuat[1].w};
        Quat pred;
        quatMul(scaledDelta, cur, &pred);
        // Re-normalize the predicted quaternion.
        const float pn = std::sqrt(pred.x*pred.x + pred.y*pred.y +
                                   pred.z*pred.z + pred.w*pred.w);
        if (pn > 1e-9f) {
            const float inv = 1.0f / pn;
            out.orientation.x = pred.x * inv;
            out.orientation.y = pred.y * inv;
            out.orientation.z = pred.z * inv;
            out.orientation.w = pred.w * inv;
        } else {
            out.orientation = m_lastQuat[1];
        }
    } else if (m_lastCount == 1) {
        out.orientation = m_lastQuat[1];
    } else {
        out.orientation.x = 0.0f;
        out.orientation.y = 0.0f;
        out.orientation.z = 0.0f;
        out.orientation.w = 1.0f;
    }
    return out;
}

void PosePredictor::GetVelocities(float* angVelX, float* angVelY, float* angVelZ,
                                  float* linVelX, float* linVelY, float* linVelZ) const {
    if (linVelX) *linVelX = static_cast<float>(m_posX.Slope());
    if (linVelY) *linVelY = static_cast<float>(m_posY.Slope());
    if (linVelZ) *linVelZ = static_cast<float>(m_posZ.Slope());
    // Angular velocity from the last delta quaternion.
    if (m_lastCount >= 2 && m_lastQuatTime[1] > m_lastQuatTime[0] &&
        angVelX && angVelY && angVelZ) {
        const Quat q1{m_lastQuat[0].x, m_lastQuat[0].y, m_lastQuat[0].z, m_lastQuat[0].w};
        const Quat q2{m_lastQuat[1].x, m_lastQuat[1].y, m_lastQuat[1].z, m_lastQuat[1].w};
        Quat q1Inv; quatConjugate(q1, &q1Inv);
        Quat d; quatMul(q2, q1Inv, &d);
        float w = d.w; if (w > 1.0f) w = 1.0f; if (w < -1.0f) w = -1.0f;
        const float half = std::acos(w);
        const float dt = static_cast<float>(m_lastQuatTime[1] - m_lastQuatTime[0]);
        const float omega = 2.0f * half / dt;   // rad/s
        const float s = std::sin(half);
        if (s > 1e-6f) {
            *angVelX = d.x / s * omega;
            *angVelY = d.y / s * omega;
            *angVelZ = d.z / s * omega;
        } else {
            *angVelX = 2.0f * d.x / dt;
            *angVelY = 2.0f * d.y / dt;
            *angVelZ = 2.0f * d.z / dt;
        }
    } else if (angVelX && angVelY && angVelZ) {
        *angVelX = *angVelY = *angVelZ = 0.0f;
    }
}

} // namespace aer_v2
