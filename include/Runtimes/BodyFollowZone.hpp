#pragma once
#include <algorithm>
#include <cmath>

namespace cvr::roomscale {
// THE BODY FOLLOWS THE HEAD CONTINUOUSLY. Head/camera tracking is never filtered here; this turns only the body.
//
// It used to be a zone with a latch: the body stood still until the head left the free-look cone, then caught up to
// the head's heading (a 25 ms exponential, later 150 deg/s) and stood still again. A slow turn of the head therefore
// came out as a staircase -- a catch-up every time the head had gained the cone's width again -- and every catch-up
// started and stopped at full speed: the jerk at each step.
//
// Now the body's turn RATE is a continuous function of how far the head is from it (a standing body tracking the head
// within a degree, never by jumps; BONELAB's chest and pelvis are joint drives, soft springs -- the same idea):
//   - past the cone the body is pulled hard (kLeashGain per radian beyond the cone), so the head never gets far past
//     the cone's edge while it turns, and the body turns WITH it rather than after it;
//   - once the head has been past the cone, the body also turns toward the head at kCentreGain per radian of the
//     whole gap -- a gain that grows in over kCentreRamp, so the pull starts from nothing -- and after the turn it
//     comes round under the head (a time constant of a fifth of a second), the way the hips follow the shoulders;
//     a slow head turn is followed a few degrees behind, at the head's own speed. The turn ends within kCentreDeg
//     of the head, and the head's tremor inside the cone starts nothing new;
//   - a glance that stays inside the cone moves nothing, and looking down (the cone widening over the head) stops
//     the centring, so the body does not turn away from the belt the hand is reaching for;
// and the rate itself eases toward that target over kRateTau, so it never starts or stops in one frame -- but for
// the look down, which stops it at once. A head turning slowly is followed by a body turning slowly, with no
// staircase.
//
// A radius of zero (the tracked body, whose own estimate is already smoothed) takes the whole gap at once.
class BodyFollowZone {
public:
    static constexpr float kCentreDeg = 0.05f;     // where a turn ends: the body under the head
    static constexpr float kCentreGain = 5.0f;     // 1/s: the pull toward the head once a turn has started
    static constexpr float kCentreRamp = 0.35f;    // s: that pull's gain grows in over this
    static constexpr float kLeashGain = 14.0f;     // 1/s per radian past the cone: the hard pull outside it
    static constexpr float kMaxRate = 6.2831853f;  // 360 deg/s
    static constexpr float kRateTau = 0.05f;       // s: how fast the rate itself changes

    void Reset() { m_rate = 0; m_blend = 0; m_turning = false; m_previousRadius = 0; }
    bool Following() const { return m_turning || std::fabs(m_rate) > 1e-3f; }
    float Rate() const { return m_rate; }

    // The body's yaw step this frame toward the head. residual = head - body (radians, wrapped), radius = the
    // free-look cone (radians), dt in seconds. Never past the head.
    float Step(float residual, float radius, float dt) {
        if (!std::isfinite(residual) || !std::isfinite(radius) || !std::isfinite(dt) || dt <= 0) {
            Reset();
            return 0;
        }
        if (radius <= 0) {
            Reset();
            return residual;
        }
        dt = std::min(dt, 0.25f);
        const float gap = std::fabs(residual);
        const float centre = kCentreDeg * 0.01745329252f;
        const bool widened = radius > m_previousRadius + 1e-5f;
        m_previousRadius = radius;
        if (gap > radius) m_turning = true;
        else if (widened) {
            // the cone opened over the head (looking down): the turn in progress stops HERE, not after easing out --
            // a rate eased down over kRateTau would carry the body on by degrees, away from the belt being reached for
            m_turning = false;
            m_rate = 0;
        }
        if (gap <= centre) m_turning = false;
        m_blend = m_turning ? std::min(1.0f, m_blend + dt / kCentreRamp) : 0.0f;
        float target = kLeashGain * std::max(0.0f, gap - radius) +
                       (m_turning ? m_blend * kCentreGain * gap : 0.0f);
        target = std::min(target, kMaxRate) * (residual < 0 ? -1.0f : 1.0f);
        m_rate += (target - m_rate) * -std::expm1(-dt / kRateTau);
        float step = m_rate * dt;
        // never through the head: a rate still easing down after the head stopped would carry the body past it
        if (step * residual <= 0) {
            if (gap <= centre) m_rate = 0;
            return 0;
        }
        if (std::fabs(step) > gap) {
            step = residual;
            m_rate = 0;
        }
        return step;
    }

private:
    float m_rate{};
    float m_blend{};
    float m_previousRadius{};
    bool m_turning{};
};
}
