// Pose predictor for AER V2.
//
// Keeps a 100-sample ring of (time, value) samples and recomputes an online
// least-squares fit so the warp path can predict a display-time pose instead of
// using the older capture-time pose directly.
#pragma once

#include <cstdint>
#include <openxr/openxr.h>

namespace aer_v2 {

// Online least-squares regression over a sliding 100-sample window of
// (time, value) pairs.
class ScalarPredictor {
public:
    static constexpr uint32_t kRingSize = 100;

    // Add a (time, value) sample. `time` is a monotonic clock in seconds.
    void AddSample(double time, double value);

    // True once at least kRingSize samples have been collected and the fit is
    // valid (Σdxdx != 0).
    bool IsValid() const { return m_count >= kRingSize && m_valid; }

    // Predicted value at future time `t`: intercept + slope·t.
    double Predict(double t) const { return m_intercept + m_slope * t; }

    double Slope() const { return m_slope; }       // units/sec
    double Intercept() const { return m_intercept; }

    void Reset();

private:
    // Raw sample rings.
    double m_time[kRingSize]{};     // x
    double m_value[kRingSize]{};    // y
    uint32_t m_writeIdx = 0;        // v6 % 100
    uint32_t m_count = 0;           // *(a1+1700), total samples seen

    // Running accumulators for the current fit window.
    double m_sumX = 0.0;
    double m_sumY = 0.0;
    double m_sumXX = 0.0;
    double m_sumXY = 0.0;
    double m_meanX = 0.0;
    double m_meanY = 0.0;

    double m_slope = 0.0;
    double m_intercept = 0.0;
    bool m_valid = false;
};

// Full head pose predictor: position via 3 scalar regressions, orientation via
// angular-velocity integration of the last two samples (small-angle delta
// quaternion). Predicts the XrPosef at a future time.
class PosePredictor {
public:
    // Record a pose sample. `timeSeconds` = monotonic time (QPC->seconds).
    // `pose` = the head pose in the same space the warp will consume (typically
    // the OpenXR LOCAL space view pose).
    void AddSample(double timeSeconds, const XrPosef& pose);

    // Predict the pose at future time `tSeconds`. Returns the last sample if
    // the predictor is not yet valid (<100 samples). The orientation is advanced
    // by angularVelocity·dt as a delta quaternion; position by the linear fit.
    XrPosef Predict(double tSeconds) const;

    bool IsValid() const {
        return m_posX.IsValid() && m_posY.IsValid() && m_posZ.IsValid();
    }

    void Reset() { m_posX.Reset(); m_posY.Reset(); m_posZ.Reset(); m_lastCount = 0; }

    // Angular velocity (rad/s) and linear velocity (m/s) estimates, for telemetry.
    void GetVelocities(float* angVelX, float* angVelY, float* angVelZ,
                       float* linVelX, float* linVelY, float* linVelZ) const;

private:
    ScalarPredictor m_posX, m_posY, m_posZ;
    // Last two orientation samples for angular-velocity estimation.
    XrQuaternionf m_lastQuat[2]{};
    double m_lastQuatTime[2]{};
    uint32_t m_lastCount = 0;
};

} // namespace aer_v2
