#pragma once
#include <cmath>

namespace cvr::body {
// THE HEAD-LED BODY TURN. The head alone decides the body's yaw; the controllers take no part. The turn is
// VRExpansionPlugin's UParentRelativeAttachmentComponent::GetCalculatedRotation (open source, ported line for line
// below with FMath::FixedTurn), with a separate tolerance and speed while the head is pitched below a look-down angle:
//   - within the tolerance of where the body last re-aimed, the body stays (or finishes a turn already under way);
//   - past it, the body turns at the speed toward the head of that frame.
// Upright the default is a 10 degree cone; looking down (more than 30 degrees) the body stays until the head has turned
// 70 degrees, then turns to where the head was then. kTightBodyTurnConfig is the same turn with a 1 degree upright
// tolerance at 150 deg/s (the reference the tests hold the port to), kFollowHeadTurnConfig the 1/1 at 500 deg/s one.
struct BodyTurnConfig {
    float upperToleranceDeg, lowerToleranceDeg, upperSpeedDps, lowerSpeedDps;
    // Upright, a turn under way follows the head through to it. GetCalculatedRotation re-aims only on the frame the
    // head leaves the tolerance, at the head of THAT frame, and a head that turns under the tolerance a frame does so
    // every so often: with 1 degree the body ends within a degree, with a 10 degree cone it stopped up to 10 degrees
    // short, wherever the last re-aim fell (in game: 6.6 of a 30 degree turn). Following through, a turn begun by the
    // head leaving the cone goes on after the head until the body is under it (kSettleDeg) and the head has stopped
    // (under kSettleDps) -- only then is the cone centred there again.
    bool followThroughUpright = false;
};
inline constexpr BodyTurnConfig kTightBodyTurnConfig{1.0f, 70.0f, 150.0f, 150.0f};
// The default: a 10 degree upright cone, 70 looking down, 300 deg/s both ways, turns followed through.
inline constexpr BodyTurnConfig kDefaultBodyTurnConfig{10.0f, 70.0f, 300.0f, 300.0f, true};
inline constexpr BodyTurnConfig kFollowHeadTurnConfig{1.0f, 1.0f, 500.0f, 500.0f};
inline constexpr float kSettleDeg = 0.5f, kSettleDps = 10.0f;
inline constexpr float kLookDownAngleDeg = -30.0f;   // camera pitch, degrees, up positive

// FRotator::ClampAxis: [0, 360)
inline float TurnClampAxis(float a) {
    a = std::fmod(a, 360.0f);
    if (a < 0.0f) a += 360.0f;
    return a;
}
// FMath::FindDeltaAngleDegrees: the shortest signed angle from A1 to A2
inline float TurnDeltaAngle(float a1, float a2) {
    float d = a2 - a1;
    if (d > 180.0f) d -= 360.0f;
    else if (d < -180.0f) d += 360.0f;
    return d;
}
// FMath::FixedTurn
inline float TurnFixed(float current, float desired, float deltaRate) {
    if (deltaRate == 0.0f) return TurnClampAxis(current);
    if (deltaRate >= 360.0f) return TurnClampAxis(desired);
    float result = TurnClampAxis(current);
    current = result;
    desired = TurnClampAxis(desired);
    if (current > desired) {
        if (current - desired < 180.0f) result -= std::fmin(current - desired, std::fabs(deltaRate));
        else result += std::fmin(desired + 360.0f - current, std::fabs(deltaRate));
    } else {
        if (desired - current < 180.0f) result += std::fmin(desired - current, std::fabs(deltaRate));
        else result -= std::fmin(current + 360.0f - desired, std::fabs(deltaRate));
    }
    return TurnClampAxis(result);
}

class HeadLedBodyTurn {
public:
    // ResetRotValues: the next update starts from the body as it stands (after a snap or stick turn, a reset).
    void Reset() { m_wasSetOnce = false; m_seeded = false; m_turning = false; m_havePrevHead = false; }
    // Starts from the body's current yaw (degrees) instead of VRExpansion's zero: the port's body already has a heading.
    void Seed(float bodyYawDeg) {
        m_lastRot = m_lastLerpVal = m_lerpTarget = TurnClampAxis(bodyYawDeg);
        m_wasSetOnce = true;
        m_seeded = true;
        m_turning = false;
        m_havePrevHead = false;
    }
    bool Seeded() const { return m_seeded; }
    float BodyYawDeg() const { return m_lastLerpVal; }

    // headYawDeg: the head's yaw relative to the character (degrees); headPitchDeg: up positive; dt seconds.
    // Returns the body's yaw (degrees, [0, 360)): GetCalculatedRotation with the tolerance and speed of the pitch.
    float Step(float headYawDeg, float headPitchDeg, float dt, const BodyTurnConfig& config = kTightBodyTurnConfig,
               float lookDownAngleDeg = kLookDownAngleDeg) {
        const bool lookingDown = headPitchDeg < lookDownAngleDeg;
        const float yawTolerance = lookingDown ? config.lowerToleranceDeg : config.upperToleranceDeg;
        const float lerpSpeed = lookingDown ? config.lowerSpeedDps : config.upperSpeedDps;
        const bool lerpTransition = lerpSpeed != 0.0f;
        const float headRate = m_havePrevHead && dt > 0 ? std::fabs(TurnDeltaAngle(m_prevHead, headYawDeg)) / dt : 0.0f;
        m_prevHead = headYawDeg;
        m_havePrevHead = true;
        const bool followThrough = config.followThroughUpright && !lookingDown;
        if (!followThrough) m_turning = false;
        if (m_turning) {
            // the follow-through (BodyTurnConfig::followThroughUpright)
            m_lerpTarget = m_lastRot = TurnClampAxis(headYawDeg);
            m_lastLerpVal = TurnFixed(m_lastLerpVal, m_lerpTarget, lerpSpeed * dt);
            if (std::fabs(TurnDeltaAngle(m_lastLerpVal, m_lerpTarget)) < kSettleDeg && headRate < kSettleDps) {
                m_lastLerpVal = m_lerpTarget;
                m_turning = false;
            }
            m_output = m_lastLerpVal;
            return m_output;
        }
        float finalRot = 0.0f;
        if (std::fabs(TurnDeltaAngle(headYawDeg, m_lastRot)) < yawTolerance) {
            if (!m_wasSetOnce) {
                m_lastRot = TurnClampAxis(headYawDeg);
                m_lastLerpVal = m_lastRot;
                m_lerpTarget = m_lastRot;
                m_wasSetOnce = true;
            }
            if (lerpTransition && std::fabs(m_lastLerpVal - m_lerpTarget) > 1e-8f) {
                m_lastLerpVal = TurnFixed(m_lastLerpVal, m_lerpTarget, lerpSpeed * dt);
                finalRot = m_lastLerpVal;
            } else {
                finalRot = m_lastRot;
                m_lastLerpVal = m_lastRot;
            }
        } else {
            if (yawTolerance > 1e-8f) {
                if (followThrough) m_turning = true;
                m_lerpTarget = TurnClampAxis(headYawDeg);
                m_lastLerpVal = TurnFixed(m_lastLerpVal, m_lerpTarget, lerpSpeed * dt);
                finalRot = m_lastLerpVal;
            } else {
                finalRot = TurnClampAxis(headYawDeg);
            }
            m_lastRot = TurnClampAxis(headYawDeg);
        }
        // (GetCalculatedRotation returns finalRot; with a zero tolerance it does not keep it in LastLerpVal)
        m_output = finalRot;
        return finalRot;
    }
    float Output() const { return m_output; }

private:
    float m_lastRot{}, m_lastLerpVal{}, m_lerpTarget{}, m_output{};
    bool m_wasSetOnce = false, m_seeded = false, m_turning = false, m_havePrevHead = false;
    float m_prevHead{};
};
}
