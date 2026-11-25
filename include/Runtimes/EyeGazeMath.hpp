#pragma once
#include <algorithm>
#include <cmath>
#include <cstdint>

namespace cvr::eyegaze {

struct Vec3 {
    float x = 0.0f;
    float y = 0.0f;
    float z = 0.0f;
};

// Tangents of a rendered eye window in the head/view frame: left < right, down < up.
struct FrustumTangents {
    float left = 0.0f;
    float right = 0.0f;
    float down = 0.0f;
    float up = 0.0f;
};

enum class TrackingState : uint8_t {
    Unavailable = 0,
    Unbound = 1,
    Inactive = 2,
    HoldingBlink = 3,
    Active = 4,
};

inline bool Normalize(Vec3& v) {
    if (!std::isfinite(v.x) || !std::isfinite(v.y) || !std::isfinite(v.z)) return false;
    const float lenSq = v.x * v.x + v.y * v.y + v.z * v.z;
    if (!std::isfinite(lenSq) || !(lenSq > 1e-10f)) return false;
    const float inv = 1.0f / std::sqrt(lenSq);
    v.x *= inv;
    v.y *= inv;
    v.z *= inv;
    return true;
}

// Project a head-space cyclopean gaze ray (origin, unit forward dir with dir.z < 0)
// into physical eye `eye` (0 = left, 1 = right) image-center percentage offsets
// (+X right, +Y down toward hands), matching FoveatedDlssSettings::CenterRegion.
inline bool ProjectGazeToEyePercent(Vec3 origin, Vec3 dir, unsigned eye, float ipdMeters,
                                    float convergenceMeters, const FrustumTangents& w,
                                    float& outX, float& outY) {
    if (!std::isfinite(origin.x) || !std::isfinite(origin.y) || !std::isfinite(origin.z)) return false;
    if (!Normalize(dir) || dir.z >= -0.1f) return false;
    const float spanX = w.right - w.left;
    const float spanY = w.up - w.down;
    if (!std::isfinite(spanX) || !std::isfinite(spanY) || !(spanX > 1e-4f) || !(spanY > 1e-4f)) return false;

    const float ipd = (std::isfinite(ipdMeters) && ipdMeters > 0.0f && ipdMeters < 0.2f) ? ipdMeters : 0.064f;
    const float eyeX = (eye == 0 ? -0.5f : 0.5f) * ipd;

    Vec3 ray = dir;
    if (std::isfinite(convergenceMeters) && convergenceMeters > 0.0f) {
        const Vec3 fix{
            origin.x + convergenceMeters * dir.x,
            origin.y + convergenceMeters * dir.y,
            origin.z + convergenceMeters * dir.z,
        };
        ray = {fix.x - eyeX, fix.y, fix.z};
    }
    if (!(ray.z < -0.05f)) return false;

    const float tx = ray.x / (-ray.z);
    const float ty = ray.y / (-ray.z);
    const float u = (tx - w.left) / spanX;
    const float v = (w.up - ty) / spanY;
    const float x = (u - 0.5f) * 100.0f;
    const float y = (v - 0.5f) * 100.0f;
    if (!std::isfinite(x) || !std::isfinite(y)) return false;

    outX = std::clamp(x, -50.0f, 50.0f);
    outY = std::clamp(y, -50.0f, 50.0f);
    return true;
}

// Map a render view index (0 = left/MAIN, 1 = right/VRCAM) to physical OpenXR eye (0 = left, 1 = right).
constexpr unsigned PhysicalEyeForRenderView(unsigned viewIndex, bool engineStereo,
                                            int mainIsRightEye, int8_t afwEye = -1) {
    if (engineStereo) return viewIndex & 1u;
    if (afwEye == 0 || afwEye == 1) return viewIndex == 0 ? unsigned(afwEye) : unsigned(1 - afwEye);
    return (viewIndex == 0) == (mainIsRightEye != 0) ? 1u : 0u;
}

// Clamp gaze and fallback offsets to [-maxAbsPercent, maxAbsPercent] before blending,
// so post-blink decay from a peripheral gaze moves immediately instead of sticking on the edge.
inline float ResolveClampedOffset(float gazePercent, float fallbackPercent, float maxAbsPercent, float blend) {
    const float limit = (std::isfinite(maxAbsPercent) && maxAbsPercent > 0.0f) ? maxAbsPercent : 0.0f;
    const float fb = std::clamp(std::isfinite(fallbackPercent) ? fallbackPercent : 0.0f, -limit, limit);
    if (!(blend > 0.0f) || !std::isfinite(gazePercent)) return fb;
    const float clampedGaze = std::clamp(gazePercent, -limit, limit);
    const float t = std::clamp(blend, 0.0f, 1.0f);
    return std::clamp(fb + (clampedGaze - fb) * t, -limit, limit);
}

// Velocity-adaptive fixation deadband + saccade pass-through + blink hold-and-decay filter.
// Explicit timestamps keep the state machine deterministic and unit-testable without sleeping.
class GazeFilter {
public:
    static constexpr float kDeadbandDeg = 0.5f;
    static constexpr float kSaccadeDeg = 2.5f;
    static constexpr uint64_t kBlinkHoldMs = 350;
    static constexpr uint64_t kBlinkHoldUs = kBlinkHoldMs * 1000ull;
    static constexpr uint64_t kMaxDuplicateHoldUs = 60'000ull;
    static constexpr float kFixationTauSec = 0.080f;
    static constexpr float kDecayTauSec = 0.150f;

    void Reset() {
        m_hasValid = false;
        m_dir = {0.0f, 0.0f, -1.0f};
        m_origin = {0.0f, 0.0f, 0.0f};
        m_lastDistinctUs = 0;
        m_lastTickUs = 0;
        m_lastValidUs = 0;
        m_activeBlend = 0.0f;
        m_state = TrackingState::Inactive;
    }

    void Update(bool valid, Vec3 origin, Vec3 dir, uint64_t nowMs) {
        UpdateUs(valid, origin, dir, nowMs * 1000ull, false);
    }

    void UpdateUs(bool valid, Vec3 origin, Vec3 dir, uint64_t nowUs, bool duplicateSample = false) {
        const float tickDtSec = (m_lastTickUs == 0)
            ? 0.0f
            : ((nowUs > m_lastTickUs) ? std::min(0.1f, static_cast<float>(nowUs - m_lastTickUs) * 1e-6f) : 0.0f);
        m_lastTickUs = std::max(m_lastTickUs, nowUs);

        const bool rawUsable = valid &&
            std::isfinite(origin.x) && std::isfinite(origin.y) && std::isfinite(origin.z) &&
            Normalize(dir) && dir.z < -0.1f;

        if (rawUsable && duplicateSample && m_hasValid) {
            if (m_state == TrackingState::Active &&
                nowUs >= m_lastDistinctUs && (nowUs - m_lastDistinctUs) <= kMaxDuplicateHoldUs) {
                m_lastValidUs = std::max(m_lastValidUs, nowUs);
                m_activeBlend = 1.0f;
                return;
            }
            // Stale/frozen sampleTime (> 60 ms without a new hardware sample): fall through as unusable!
        }

        const bool freshUsable = rawUsable && !duplicateSample;
        if (freshUsable) {
            if (!m_hasValid) {
                m_dir = dir;
                m_origin = origin;
                m_hasValid = true;
            } else {
                const float sampleDtSec = (m_lastDistinctUs > 0 && nowUs > m_lastDistinctUs)
                    ? std::min(0.1f, static_cast<float>(nowUs - m_lastDistinctUs) * 1e-6f)
                    : 0.0f;
                const float dot = std::clamp(m_dir.x * dir.x + m_dir.y * dir.y + m_dir.z * dir.z, -1.0f, 1.0f);
                const float deltaDeg = std::acos(dot) * 57.2957795f;
                const float slowAlpha = 1.0f - std::exp(-sampleDtSec / kFixationTauSec);
                const float t = std::clamp((deltaDeg - kDeadbandDeg) / (kSaccadeDeg - kDeadbandDeg), 0.0f, 1.0f);
                const float smooth = t * t * (3.0f - 2.0f * t);
                const float alpha = slowAlpha + (1.0f - slowAlpha) * smooth;

                Vec3 nextDir{
                    m_dir.x + (dir.x - m_dir.x) * alpha,
                    m_dir.y + (dir.y - m_dir.y) * alpha,
                    m_dir.z + (dir.z - m_dir.z) * alpha,
                };
                if (Normalize(nextDir)) m_dir = nextDir;
                else m_dir = dir;

                m_origin.x += (origin.x - m_origin.x) * alpha;
                m_origin.y += (origin.y - m_origin.y) * alpha;
                m_origin.z += (origin.z - m_origin.z) * alpha;
            }
            m_lastDistinctUs = std::max(m_lastDistinctUs, nowUs);
            m_lastValidUs = std::max(m_lastValidUs, nowUs);
            m_activeBlend = 1.0f;
            m_state = TrackingState::Active;
            return;
        }

        if (m_hasValid && nowUs >= m_lastValidUs && (nowUs - m_lastValidUs) <= kBlinkHoldUs) {
            m_activeBlend = 1.0f;
            m_state = TrackingState::HoldingBlink;
            return;
        }

        m_state = TrackingState::Inactive;
        m_activeBlend *= std::exp(-tickDtSec / kDecayTauSec);
        if (m_activeBlend < 0.001f) {
            m_activeBlend = 0.0f;
            m_hasValid = false;
        }
    }

    TrackingState State() const { return m_state; }
    float ActiveBlend() const { return m_activeBlend; }
    bool HasDirection() const { return m_hasValid || m_activeBlend > 0.0f; }
    Vec3 Direction() const { return m_dir; }
    Vec3 Origin() const { return m_origin; }

private:
    bool m_hasValid = false;
    Vec3 m_dir{0.0f, 0.0f, -1.0f};
    Vec3 m_origin{0.0f, 0.0f, 0.0f};
    uint64_t m_lastDistinctUs = 0;
    uint64_t m_lastTickUs = 0;
    uint64_t m_lastValidUs = 0;
    float m_activeBlend = 0.0f;
    TrackingState m_state = TrackingState::Inactive;
};

}  // namespace cvr::eyegaze
