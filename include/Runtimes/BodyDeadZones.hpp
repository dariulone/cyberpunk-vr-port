#pragma once
#include "Runtimes/RoomscaleMovement.hpp"
#include <algorithm>
#include <cmath>

namespace cvr::roomscale {
// THE VISIBLE BODY'S DEAD ZONES (off by default: CyberpunkVR_BodyDeadZones). The capsule stands under the head every
// tick (strict roomscale); with the dead zones on, the body the player sees is placed apart from it:
//   lookingDown          = camera pitch < -30
//   innerDistance        = Distance2D(body, camera)
//   bodyToCamera         = Normal(camera - body)  (2D)
//   distance             = Distance2D(body + (lookingDown ? bodyToCamera : body forward) * xyShift, camera)
//   in the dead zone     = upright:      distance < 9 cm
//                          looking down: (Acos(camera forward 2D . bodyToCamera) <= 60 || pitch <= -75)
//                                        && innerDistance >= 12 cm && distance < 30 cm
//   breached:
//       inner breached (innerDistance < 12 && lookingDown): body += bodyToCamera * (innerDistance - 12)
//       else: body = camera - body forward * xyShift   (the neck under the camera)
//   otherwise the body stays where it stands -- moving with the capsule while the stick moves it, which in the
//   tracking space the head lives in is standing still too.
// xyShift is the camera's horizontal reach ahead of the neck: the port's neck lever (EyeFromNeck). Everything is in the
// tracking space's axes, relative to the NECK, which the capsule stands under: the body's offset from the capsule.
// Units: tracking metres times scale = game metres.
class BodyDeadZones {
public:
    static constexpr float kUpperRadius = 0.09f;      // upright
    static constexpr float kLowerRadius = 0.30f;      // looking down
    static constexpr float kLowerHalfAngleDeg = 60.0f; // looking down: the sector ahead
    static constexpr float kInnerRadius = 0.12f;      // looking down: kept off the camera
    static constexpr float kLookDownDeg = -30.0f;
    static constexpr float kStraightDownDeg = -75.0f; // looking straight down: any direction counts as ahead
    // THE PLACING IS SHOWN EASED, NOT AS A JUMP. Set under the neck in one frame, a 9 cm (or, looking down, 30 cm) move
    // of a body whose legs are in view reads as a teleport. The dead zones decide as above; only what is SHOWN goes to
    // the new place on a critically damped spring (no jump in the first frame, about 95 % in 0.3 s) and stands still
    // with the world while it gets there. omega 0 shows the jump.
    static constexpr float kShowOmega = 16.0f;        // 1/s

    struct State { bool lookingDown{}, breached{}, inner{}; float distance{}, innerDistance{}; };

    void Reset() { m_offset = {}; m_shown = {}; m_shownVelocity = {}; m_valid = false; m_state = {}; }
    bool Valid() const { return m_valid; }
    Vec2 Offset() const { return m_offset; }          // the body less the neck, tracking metres (the zones' decision)
    Vec2 Shown() const { return m_shown; }            // the same, as shown: eased to the offset (kShowOmega)
    const State& Last() const { return m_state; }

    // neckMoved: how far the neck -- the capsule -- really moved this tick (tracking metres, what the controller
    // accepted); the body stays in the world, so its offset from the neck shrinks by that. eyeFromNeck: the camera
    // relative to the neck (tracking metres, horizontal). bodyForward: the body's forward in tracking axes (unit).
    // pitchDeg: the camera's pitch, up positive. scale: game metres per tracking metre.
    // dt (s) and showOmega (1/s) ease what is shown; dt 0 or showOmega 0 shows the offset as it is.
    Vec2 Step(Vec2 neckMoved, Vec2 eyeFromNeck, Vec2 bodyForward, float pitchDeg, float scale, float dt = 0,
              float showOmega = 0) {
        if (!Finite(neckMoved) || !Finite(eyeFromNeck) || !Finite(bodyForward) || !std::isfinite(pitchDeg) ||
            !std::isfinite(scale) || scale <= 0) { Reset(); return {}; }
        if (!m_valid) { m_offset = {}; m_shown = {}; m_shownVelocity = {}; m_valid = true; }   // first tick: under the neck
        m_offset = m_offset - neckMoved;
        m_shown = m_shown - neckMoved;                 // the shown body stands in the world too
        // game metres from here on
        const Vec2 body = m_offset * scale, camera = eyeFromNeck * scale;
        const float xyShift = std::sqrt(Dot(camera, camera));
        const Vec2 cameraForward = xyShift > 1e-6f ? camera * (1.0f / xyShift) : bodyForward;
        State s{};
        s.lookingDown = pitchDeg < kLookDownDeg;
        const Vec2 toCamera = camera - body;
        s.innerDistance = std::sqrt(Dot(toCamera, toCamera));
        const Vec2 bodyToCamera = s.innerDistance > 1e-6f ? toCamera * (1.0f / s.innerDistance) : Vec2{};
        const Vec2 reach = body + (s.lookingDown ? bodyToCamera : bodyForward) * xyShift - camera;
        s.distance = std::sqrt(Dot(reach, reach));
        bool inZone;
        if (s.lookingDown) {
            const float cosine = std::clamp(Dot(cameraForward, bodyToCamera), -1.0f, 1.0f);
            const bool ahead = std::acos(cosine) * 57.2957795f <= kLowerHalfAngleDeg || pitchDeg <= kStraightDownDeg;
            inZone = ahead && s.innerDistance >= kInnerRadius && s.distance < kLowerRadius;
        } else {
            inZone = s.distance < kUpperRadius;
        }
        s.breached = !inZone;
        if (s.breached) {
            s.inner = s.innerDistance < kInnerRadius && s.lookingDown;
            Vec2 placed = s.inner ? body + bodyToCamera * (s.innerDistance - kInnerRadius)
                                  : camera - bodyForward * xyShift;
            m_offset = placed * (1.0f / scale);
        }
        m_state = s;
        Show(dt, showOmega);
        return m_offset;
    }

private:
    // Exact critically damped response toward the offset (as BodyTranslationFollow's): no jump, no overshoot.
    void Show(float dt, float omega) {
        if (!std::isfinite(dt) || !std::isfinite(omega) || dt <= 0 || omega <= 0) {
            m_shown = m_offset; m_shownVelocity = {}; return;
        }
        dt = std::min(dt, 0.05f);
        const Vec2 error = m_offset - m_shown;
        if (Dot(error, error) < 1e-10f && Dot(m_shownVelocity, m_shownVelocity) < 1e-8f) {
            m_shown = m_offset; m_shownVelocity = {}; return;
        }
        const float decay = std::exp(-omega * dt);
        const Vec2 c = m_shownVelocity - error * omega;
        m_shown = m_shown + (error - (error - c * dt) * decay);
        m_shownVelocity = (m_shownVelocity - c * (omega * dt)) * decay;
    }
    Vec2 m_offset{}, m_shown{}, m_shownVelocity{};
    State m_state{};
    bool m_valid{};
};
}
