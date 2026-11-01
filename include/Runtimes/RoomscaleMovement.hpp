#pragma once

// The accounting for roomscale is independent of OpenXR and of engine pointers.
// Only planar displacement is sent to physics. Camera height, gravity and jump Z
// never enter this accumulator. Units here are tracking-space metres.
#include <algorithm>
#include <cmath>
#include <cstdint>
#include <limits>
#include "Runtimes/BodyFollowZone.hpp"

namespace cvr::roomscale {

struct Vec2 {
    float x{};
    float y{}; // game-local forward = -OpenXR Z
    Vec2 operator+(Vec2 b) const { return {x + b.x, y + b.y}; }
    Vec2 operator-(Vec2 b) const { return {x - b.x, y - b.y}; }
    Vec2 operator*(float s) const { return {x * s, y * s}; }
};
inline float Dot(Vec2 a, Vec2 b) { return a.x*b.x + a.y*b.y; }
inline bool Finite(Vec2 v) { return std::isfinite(v.x) && std::isfinite(v.y); }
inline Vec2 Rotate(Vec2 v, float yaw) {
    const float c = std::cos(yaw), s = std::sin(yaw);
    return {c*v.x - s*v.y, s*v.x + c*v.y};
}
inline float TrackingYaw(float entityYaw, float bodyRealign) {
    return std::remainder(entityYaw - bodyRealign, 6.28318530718f);
}

// Translation starts from rest at the free-zone boundary. The angular follower
// intentionally keeps its shorter response; sharing its exponential fraction
// made a 5.2cm lean produce a 2.7cm first CCT step in the live trace.
class BodyTranslationFollow {
public:
    void Reset() { m_following=false;m_velocity={}; }
    bool Following() const { return m_following; }
    // The controller refused part of the last step: along that direction the body
    // is at rest against the obstacle, not moving as the response assumed. Without
    // this the response keeps gaining speed into a wall and, at a slant, throws the
    // body along it in one step.
    void Block(Vec2 refused) {
        const float r2=Dot(refused,refused);
        if(r2<1e-14f)return;
        const float along=Dot(m_velocity,refused)/r2;
        if(along>0)m_velocity=m_velocity-refused*along;
    }
    Vec2 Step(Vec2 error,float radius,float dt) {
        if(!Finite(error) || !std::isfinite(radius) || !std::isfinite(dt) || dt<=0) {
            Reset();return {};
        }
        if(radius<=0) { Reset();return error; }
        const float distance2=Dot(error,error);
        if(!m_following && distance2<=radius*radius)return {};
        m_following=true;
        if(distance2<1e-12f) { Reset();return {}; }
        // Exact critically damped response to this target, in tracking axes
        // and game metres. Roughly 95% settles in 0.24s, with no initial jump.
        constexpr float omega=20.0f;
        dt=std::min(dt,.05f);
        const float decay=std::exp(-omega*dt);
        const Vec2 c=m_velocity-error*omega;
        Vec2 step=error-(error-c*dt)*decay;
        m_velocity=(m_velocity-c*(omega*dt))*decay;
        // A reversing target must not carry the body away from the headset or
        // past it. These guards also retire residual velocity at the centre.
        if(Dot(step,error)<0) { m_velocity={};return {}; }
        if(Dot(step,step)>=distance2) { Reset();return error; }
        const float finish=std::min(.0005f,radius*.01f);
        const Vec2 remaining=error-step;
        if(Dot(remaining,remaining)<=finish*finish) { Reset();return error; }
        return step;
    }
private:
    Vec2 m_velocity{};
    bool m_following{};
};

inline bool HeadYaw(float x, float y, float z, float w, float* yaw) {
    const float n2 = x*x + y*y + z*z + w*w;
    if (!yaw || !std::isfinite(n2) || n2 < 0.5f || n2 > 1.5f) return false;
    const float inv = 1.0f / std::sqrt(n2);
    x *= inv; y *= inv; z *= inv; w *= inv;
    // Heading of OpenXR -Z projected onto the floor. The denominator contains
    // X^2+Y^2; using Y^2+Z^2 made pitch change body yaw at diagonal headings.
    // Near vertical/inverted poses have no reliable body heading: leave the
    // body alone while the VIEW keeps the full unmodified head quaternion.
    if (1.0f - 2.0f*(x*x + z*z) < 0.1f) return false;
    const float hx = 2.0f*(w*y + x*z), hz = 1.0f - 2.0f*(x*x + y*y);
    if (hx*hx + hz*hz < 0.0001f) return false;
    *yaw = std::atan2(hx, hz);
    return true;
}

struct Sample {
    Vec2 head{};
    uint64_t sequence{};
    uint64_t origin{};
    uint64_t stampUs{};
    bool valid{}; // position AND orientation valid/tracked; session visible or focused
    Vec2 runtimeHead{}; // raw tracking, used only to detect discontinuities hidden by view filtering
    bool runtimeHeadKnown{};
};

// The runtime packet qualifies tracking availability. The displacement must
// come from the SAME latched position that the body/camera will render.
inline Sample MovementFrameSample(const Sample& tracking, const Sample& frame) {
    Sample result=frame;
    result.valid=tracking.valid && frame.valid && Finite(tracking.head) && tracking.origin==frame.origin;
    // A failed/old frame read must not invent origin0 and erase the movement
    // ledger. Only the runtime's actual origin publication may change it.
    result.origin=tracking.origin;
    result.stampUs=std::min(tracking.stampUs,frame.stampUs);
    result.runtimeHead=tracking.head;
    result.runtimeHeadKnown=true;
    return result;
}

enum class Reason : uint32_t {
    Applied, Baseline, Suspended, TrackingInvalid, Stale, Duplicate, Discontinuity
};

struct Step {
    Vec2 raw{};
    Vec2 world{};
    float yaw{}, scale{}; // world = Rotate(raw*scale, yaw)
    uint64_t origin{};
    uint64_t ticket{};
    Reason reason{Reason::Suspended};
};

class Movement {
public:
    static constexpr uint64_t MaxAgeUs = 250000;
    static constexpr float MaxStepMetres = 0.5f;

    void Reset() { *this = {}; }
    // m_pending is the view's lead over the capsule, not motion still to replay:
    // a suspension, a gap or a tracking jump keeps it for the body to follow
    // later. Dropped, it would stay in the view as a permanent offset from the body.
    void Suspend() { m_active = false;m_follow.Reset(); }

    Step Begin(const Sample& sample, uint64_t nowUs, float dt,
               float scale, float trackingYaw, bool allowed,float freeRadiusMetres=0) {
        Step step{};
        if (sample.origin != m_origin) {
            Reset();
            m_origin = sample.origin;
        }
        step.origin = m_origin;
        step.ticket = ++m_ticket;
        const bool gap = m_tickUs && (nowUs < m_tickUs || nowUs - m_tickUs > MaxAgeUs);
        m_tickUs = nowUs;
        if (!sample.valid || !sample.sequence || !Finite(sample.head) ||
            !std::isfinite(scale) || scale < 0.05f || scale > 20.0f ||
            !std::isfinite(trackingYaw)) {
            m_active = false;
            step.reason = Reason::TrackingInvalid;
            return step;
        }
        if (nowUs < sample.stampUs || nowUs - sample.stampUs > MaxAgeUs) {
            m_active = false;
            step.reason = Reason::Stale;
            return step;
        }
        if (!allowed || !std::isfinite(dt) || dt <= 0.000001f || dt > 0.25f) {
            m_previous = sample.head;
            m_sequence = sample.sequence;
            m_active = false;
            step.reason = Reason::Suspended;
            return step;
        }
        if (!m_active || gap) {
            m_follow.Reset();
            m_previous = sample.head;
            m_sequence = sample.sequence;
            m_active = true;
            m_previousRuntime = sample.runtimeHead;
            m_runtimeKnown = sample.runtimeHeadKnown;
            m_rebasingFilteredJump = false;
            step.reason = Reason::Baseline;
            return step;
        }
        if (sample.sequence < m_sequence ||
            (sample.sequence==m_sequence && !m_follow.Following())) {
            step.reason = Reason::Duplicate;
            return step;
        }
        const Vec2 delta = sample.sequence==m_sequence ? Vec2{}:sample.head-m_previous;
        m_previous = sample.head;
        m_sequence = sample.sequence;
        const Vec2 metres = delta * scale;
        const Vec2 runtimeDelta = (sample.runtimeHead - m_previousRuntime) * scale;
        const bool runtimeJump = sample.runtimeHeadKnown && m_runtimeKnown &&
            Dot(runtimeDelta, runtimeDelta) > MaxStepMetres * MaxStepMetres;
        m_previousRuntime = sample.runtimeHead;
        m_runtimeKnown = sample.runtimeHeadKnown;
        if (runtimeJump) m_rebasingFilteredJump = true;
        if (Dot(metres, metres) > MaxStepMetres * MaxStepMetres || m_rebasingFilteredJump) {
            m_follow.Reset();
            // A tracking jump must not teleport either the capsule or the camera.
            // Rebase the camera's tracking span, without saving a movement debt.
            m_consumed = m_consumed + delta;
            // A raw tracking teleport can take several filtered camera frames
            // to settle. Drop that entire discontinuity rather than accepting
            // the smaller tail as ordinary physical walking.
            const Vec2 remaining = (sample.runtimeHead - sample.head) * scale;
            if (!sample.runtimeHeadKnown || Dot(remaining, remaining) <= 0.000001f)
                m_rebasingFilteredJump = false;
            step.reason = Reason::Discontinuity;
            return step;
        }
        m_pending=m_pending+delta;
        step.raw=m_follow.Step(m_pending*scale,freeRadiusMetres,dt)*(1.0f/scale);
        step.world = Rotate(step.raw*scale, trackingYaw);
        step.yaw = trackingYaw;
        step.scale = scale;
        step.reason = Reason::Applied;
        return step;
    }

    // Where the eye may be relative to the capsule's axis (world metres): eyeBase is
    // the view's fixed offset from the axis (the camera's calibration), the tracking
    // offset adds to it, and the sum stays within radius -- never tighter than where
    // the eye already is without the lead, so the limit never pulls the view.
    struct LeadLimit {
        Vec2 eyeBase{};
        float radius = std::numeric_limits<float>::infinity();
    };

    // The controller carried out the whole request.
    void Complete(const Step& step) { Complete(step, step.world, LeadLimit{}); }

    // acceptedWorld: what the controller carried out of step.world (world metres).
    // The refused rest is not consumed: the head keeps it as a lead over the capsule,
    // so at a wall the view stays on the headset and the body follows once the way
    // is clear. Past the limit the lead is consumed as before, so a headset walked
    // into a wall builds no debt and the view stops there; stepping back moves it at once.
    void Complete(const Step& step, Vec2 acceptedWorld, LeadLimit limit) {
        if (step.origin != m_origin || step.ticket != m_ticket || m_completed == step.ticket) return;
        m_completed = step.ticket;
        Vec2 accepted = step.raw;
        const Vec2 refusedWorld = step.world - acceptedWorld;
        if (Finite(acceptedWorld) && step.scale > 0 && Dot(refusedWorld, refusedWorld) > 1e-14f)
            accepted = step.raw - Rotate(refusedWorld, -step.yaw) * (1.0f / step.scale);
        m_consumed = m_consumed + accepted;
        if (step.reason != Reason::Applied) return;
        m_pending = m_pending - accepted;
        m_follow.Block(step.raw - accepted);
        if (!std::isfinite(limit.radius) || !Finite(limit.eyeBase) || step.scale <= 0) return;
        const Vec2 base = limit.eyeBase + Rotate((m_previous - m_consumed - m_pending) * step.scale, step.yaw);
        const Vec2 eye = base + Rotate(m_pending * step.scale, step.yaw);
        const float reach = std::sqrt(Dot(eye, eye));
        const float bound = std::max(std::max(limit.radius, 0.0f), std::sqrt(Dot(base, base)));
        if (reach > bound) {
            const Vec2 excess = Rotate(eye * (1.0f - bound / reach), -step.yaw) * (1.0f / step.scale);
            m_consumed = m_consumed + excess;
            m_pending = m_pending - excess;
        }
    }

    Vec2 Consumed(uint64_t origin) const { return origin == m_origin ? m_consumed : Vec2{}; }
    // The head's lead over the capsule that the body still has to follow (tracking units).
    Vec2 Pending() const { return m_pending; }

private:
    Vec2 m_previous{}, m_consumed{};
    Vec2 m_pending{};
    BodyTranslationFollow m_follow;
    Vec2 m_previousRuntime{};
    uint64_t m_origin{}, m_sequence{}, m_tickUs{}, m_ticket{}, m_completed{};
    bool m_active{};
    bool m_runtimeKnown{}, m_rebasingFilteredJump{};
};

// Attribute the planar CCT response to the added physical displacement. In free
// space it is exact. At a wall use the rejected-motion normal as a projection;
// for physical-only movement the measured velocity is the exact contribution.
// Mixed-input/contact attribution needs the in-game wall/step tests as well.
// The part of the physical displacement the controller carried out, for the ledger.
// Only the refused component along the request is withheld: unlike the velocity
// attribution above, motion the request did not ask for (a shove, a slide along
// the wall from the stick) is never credited to the headset.
inline Vec2 AcceptedDisplacement(Vec2 native, Vec2 physical, Vec2 resolved) {
    if (!Finite(native) || !Finite(physical) || !Finite(resolved)) return physical;
    const Vec2 lost = native + physical - resolved;
    const float lost2 = Dot(lost, lost);
    if (lost2 <= 1e-12f) return physical;
    return physical - lost * std::max(0.0f, Dot(physical, lost) / lost2);
}

inline Vec2 PhysicalVelocity(Vec2 native, Vec2 physical, Vec2 resolvedVelocity, float dt) {
    if (!Finite(native) || !Finite(physical) || !Finite(resolvedVelocity) ||
        !std::isfinite(dt) || dt <= 0.000001f || Dot(physical, physical) < 1e-14f) return {};
    if (Dot(native, native) < 1e-12f) return resolvedVelocity;
    const Vec2 lost = native + physical - resolvedVelocity * dt;
    const float lost2 = Dot(lost, lost);
    Vec2 accepted = physical;
    if (lost2 > 1e-8f) {
        accepted = physical - lost * std::max(0.0f, Dot(physical, lost) / lost2);
    }
    return accepted * (1.0f / dt);
}

} // namespace cvr::roomscale
