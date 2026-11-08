#pragma once

// The accounting for roomscale is independent of OpenXR and of engine pointers.
// Only planar displacement is sent to physics. Camera height, gravity and jump Z
// never enter this accumulator. Units here are tracking-space metres.
#include <algorithm>
#include <cmath>
#include <cstdint>
#include <limits>
#include "Runtimes/BodyFollowZone.hpp"
#include "Anim/BodyBend.hpp"

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
// IT ENDS UNDER THE HEAD, AND THEN IT STANDS STILL (2026-11-06: "the legs twitch a hundred times"). A follow used
// to end only within half a millimetre of the headset, which is inside a headset's own position noise (0.3-0.5 mm and
// a slow wander of about as much): it never ended, and every frame of tracking noise after a step was a capsule move --
// 0.1-0.19 m/s pulses on a body standing still, each one a step of the walking animation. Now a follow ends once the
// body is within kSettle of the head and slower than kSettleSpeed, and stands still until the head leaves the free zone
// again -- or, standing, it recentres: a head held still (within kStillRadius) for kStillTime with the body more than
// kRecentre off is followed again, at kRecentreOmega, so after a small step or a turn the body comes under
// the head in one short shuffle rather than staying off it. Still is a place, not a speed: a speed taken from frame to
// frame is the headset's noise (0.5 mm a frame is 45 mm/s at 90 Hz), and no threshold under it ever saw a head at rest.
// And only standing upright, looking ahead (Sample::upright): a lean held a second is not a step, and recentred under
// it the body walked in under the lean and back out after it, a slow creep the legs stepped to all the way (live: 39
// foot steps for 40 mm). Brisk, too: at the follow's gentler 7/s a recentre crept, and a creeping capsule is the
// walking animation starting and stopping.
// AND A WALK IS ONE FOLLOW. The follow used to end whenever it caught the head -- a step that would reach it, or a
// remainder under half a millimetre -- and a walking head is caught every few frames: the body then stood until the
// head was the free zone ahead again, and a steady walk came out as a stop and a lurch four times a second (measured:
// requests of 8, 0, 9, 17, 6, 0 ... mm a frame at 0.4 m/s), each one the walking animation starting and stopping.
// Reaching the head now takes the gap and goes on following; only the settle ends it.
class BodyTranslationFollow {
public:
    static constexpr float kSettle = 0.004f;        // m: a follow ends this close to the head (above the noise)...
    static constexpr float kSettleSpeed = 0.03f;    // m/s: ...and this slow
    static constexpr float kRecentre = 0.03f;       // m: a body further off than this, standing, recentres
    static constexpr float kStillRadius = 0.015f;   // m: the head counts as still while it stays this close to a place...
    static constexpr float kStillTime = 1.0f;       // s: ...for this long (a glance round a corner is shorter)
    static constexpr float kFollowOmega = 20.0f;    // 1/s: leaving the free zone: 95 % in 0.24 s
    static constexpr float kRecentreOmega = 12.0f;  // 1/s: recentring: 95 % in 0.4 s, one short shuffle
    void Reset() { m_following=false;m_velocity={}; }
    bool Following() const { return m_following; }
    // Where the head is this tick, for the standing recentre: the NECK ITSELF, the hip hinge the bend tracker takes
    // out of the movement sample put back. Still is the neck held still: a slow bend moves it (the sample, its hinge
    // taken out, barely moves -- read as still, it walked the body 14 mm into a slow bend and back out of it).
    void HeadMotion(Vec2 neck, bool upright, float dt) {
        if(!upright || !Finite(neck) || !std::isfinite(dt) || dt<=0) { m_stillFor=0; m_stillKnown=false; return; }
        const Vec2 head=neck;
        const Vec2 moved=head-m_stillAt;
        if(!m_stillKnown || Dot(moved,moved)>kStillRadius*kStillRadius) {
            m_stillAt=head; m_stillKnown=true; m_stillFor=0; return;
        }
        m_stillFor+=dt;
    }
    bool HeadStill() const { return m_stillFor>=kStillTime; }
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
        if(!m_following) {
            if(distance2>radius*radius) m_omega=kFollowOmega;
            else if(HeadStill() && distance2>kRecentre*kRecentre) m_omega=kRecentreOmega;
            else return {};
        } else if(distance2>radius*radius) {
            m_omega=kFollowOmega;       // a recentre the head walks away from becomes a follow
        }
        m_following=true;
        if(distance2<1e-12f) { Reset();return {}; }
        if(distance2<=kSettle*kSettle && Dot(m_velocity,m_velocity)<=kSettleSpeed*kSettleSpeed) { Reset();return {}; }
        // Exact critically damped response to this target, in tracking axes
        // and game metres. Roughly 95% settles in 3/omega s, with no initial jump.
        const float omega=m_omega;
        dt=std::min(dt,.05f);
        const float decay=std::exp(-omega*dt);
        const Vec2 c=m_velocity-error*omega;
        Vec2 step=error-(error-c*dt)*decay;
        m_velocity=(m_velocity-c*(omega*dt))*decay;
        // A reversing target must not carry the body away from the headset or
        // past it. These guards also retire residual velocity at the centre.
        if(Dot(step,error)<0) { m_velocity={};return {}; }
        if(Dot(step,step)>=distance2) return error;     // caught the head: on under it, the follow goes on
        return step;
    }
private:
    Vec2 m_velocity{};
    float m_omega = kFollowOmega;
    Vec2 m_stillAt{};
    float m_stillFor{};
    bool m_stillKnown{};
    bool m_following{};
};

// THE WALK THE REST OF THE GAME SEES (strict mode, 2026-11-07). With the body strictly under the head every move of the
// neck is a capsule move -- a lean, a sway, the headset's own noise -- and every reader of the player's velocity (the
// locomotion animation first) would see each of them as a start and a
// stop. Those readers get the physical part of the velocity only as a WALK: smoothed over kTau, and only once the
// smoothed speed has passed kWalkOn, until it has stayed under kWalkOff for kWalkOffHold. Standing, leaning, swaying
// and stepping in place, they see nothing of it; walking, they see one steady speed. The capsule itself still moves
// by every step. The solver never sees any of it (Hooks/RoomscaleMove.cpp subtracts the whole physical part there).
class PhysicalWalkGate {
public:
    static constexpr float kTau = 0.15f;          // s: the readers' speed is the physical one smoothed over this
    static constexpr float kWalkOn = 0.30f;       // m/s: a walk starts once the smoothed speed passes this...
    static constexpr float kWalkOff = 0.12f;      // m/s: ...and ends once it has stayed under this...
    static constexpr float kWalkOffHold = 0.25f;  // s: ...for this long (a gait's slow phase is shorter)
    void Reset() { *this = {}; }
    void Update(Vec2 velocity, float dt) {
        if(!Finite(velocity) || !std::isfinite(dt) || dt<=0) { Reset(); return; }
        dt=std::min(dt,0.1f);
        m_smoothed=m_smoothed+(velocity-m_smoothed)*(1.0f-std::exp(-dt/kTau));
        const float speed2=Dot(m_smoothed,m_smoothed);
        if(!m_walking) {
            if(speed2>kWalkOn*kWalkOn) { m_walking=true; m_slowFor=0; }
        } else if(speed2<kWalkOff*kWalkOff) {
            m_slowFor+=dt;
            if(m_slowFor>=kWalkOffHold) { m_walking=false; m_slowFor=0; }
        } else {
            m_slowFor=0;
        }
    }
    bool Walking() const { return m_walking; }
    Vec2 Reported() const { return m_walking ? m_smoothed : Vec2{}; }
private:
    Vec2 m_smoothed{};
    float m_slowFor{};
    bool m_walking{};
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

// THE NECK, NOT THE EYES. The view pose is the eyes, and they sit ahead of and above the point the head turns about,
// so a head turned without a step carries them along an arc round the neck: 13 cm for a quarter turn looking ahead,
// 20 cm and more looking down, past the free zone (xr_body_move_radius) -- and the body walked after a look round
// (2026-11-04: "turning the head down and to the left or right moves the avatar"). Roomscale counts the
// neck: the eyes less this lever, in the head's own axes (OpenXR: +Y up, -Z ahead). The view keeps the eyes.
inline constexpr float kEyeAboveNeck = cvr::body::EyeAboveNeck, kEyeAheadOfNeck = cvr::body::EyeAheadOfNeck;
// The lever's planar part in movement axes (x right, y forward = -OpenXR Z) for a head orientation; zero for an
// orientation that is not one.
inline Vec2 EyeFromNeck(float x, float y, float z, float w) {
    const float n2 = x*x + y*y + z*z + w*w;
    if (!std::isfinite(n2) || n2 < 0.5f || n2 > 1.5f) return {};
    const float inv = 1.0f / std::sqrt(n2);
    x *= inv; y *= inv; z *= inv; w *= inv;
    const float v[3]{0.0f, kEyeAboveNeck, -kEyeAheadOfNeck};
    // v + w*t + q x t, t = 2 (q x v)
    const float t[3]{2*(y*v[2] - z*v[1]), 2*(z*v[0] - x*v[2]), 2*(x*v[1] - y*v[0])};
    const float rx = v[0] + w*t[0] + (y*t[2] - z*t[1]);
    const float rz = v[2] + w*t[2] + (x*t[1] - y*t[0]);
    return {rx, -rz};
}

struct Sample {
    Vec2 head{};
    uint64_t sequence{};
    uint64_t origin{};
    uint64_t stampUs{};
    bool valid{}; // position AND orientation valid/tracked; session visible or focused
    Vec2 runtimeHead{}; // raw tracking, used only to detect discontinuities hidden by view filtering
    bool runtimeHeadKnown{};
    Vec2 eyeFromNeck{}; // what `head` left out of the eyes (EyeFromNeck), for the eye's own limits
    Vec2 bend{};        // what `head` left out of the neck: the hip hinge (BendTracker), metres
    bool upright{true}; // standing and looking ahead: no crouch, no hinge, the head within 20 deg of level
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
    float dt{};           // seconds this move covers (Applied only)
    uint64_t origin{};
    uint64_t ticket{};
    Reason reason{Reason::Suspended};
    bool strict{};        // asked in strict mode: Complete takes the refused part off the view (VRExpansion's rule)
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

    // STRICT: the body strictly under the head (2026-11-07): the whole lead the head has over the capsule is asked of
    // the controller every tick -- no free zone, no follow and no settle. The readers of the velocity see it as
    // PhysicalWalkGate lets them.
    // WHAT THE WORLD REFUSES IS THE VIEW'S, AT ONCE (2026-11-07). As VRExpansionPlugin's retained roomscale does it:
    // every tick UVRBaseCharacterMovementComponent::RewindVRRelativeMovement moves the root back by the headset's delta
    // and the move sweeps it forward again (SafeMoveUpdatedComponent(Velocity * dt + AdditionalVRInputVector), step-up,
    // slide along the surface), so the capsule stays at the headset and whatever the sweep refuses moves the tracking
    // origin -- the view -- back by exactly that, the same tick. Nothing is kept: no lead in the wall that is asked again
    // next tick (the stumble against a prop: the controller met the same refused lead every tick) and no lead eased out
    // of the view afterwards (the view drifting back from the wall at kLeadEaseBack). Complete() does that for a strict
    // step.
    static constexpr float kStrictRest = 0.0001f;   // m: a lead under this is no move (the same sample again)
    Step Begin(const Sample& sample, uint64_t nowUs, float dt,
               float scale, float trackingYaw, bool allowed,float freeRadiusMetres=0,bool strict=false) {
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
            m_haveHead = true;
            m_sequence = sample.sequence;
            m_active = false;
            step.reason = Reason::Suspended;
            return step;
        }
        if (!m_active || gap) {
            m_follow.Reset();
            m_previous = sample.head;
            m_haveHead = true;
            m_sequence = sample.sequence;
            m_active = true;
            m_previousRuntime = sample.runtimeHead;
            m_runtimeKnown = sample.runtimeHeadKnown;
            m_rebasingFilteredJump = false;
            step.reason = Reason::Baseline;
            return step;
        }
        const bool chasing = strict ? Dot(m_pending*scale, m_pending*scale) > kStrictRest*kStrictRest
                                    : m_follow.Following();
        if (sample.sequence < m_sequence || (sample.sequence==m_sequence && !chasing)) {
            step.reason = Reason::Duplicate;
            return step;
        }
        const Vec2 delta = sample.sequence==m_sequence ? Vec2{}:sample.head-m_previous;
        m_previous = sample.head;
        m_haveHead = true;
        m_sequence = sample.sequence;
        if (Finite(sample.eyeFromNeck)) m_eyeFromNeck = sample.eyeFromNeck;
        const Vec2 metres = delta * scale;
        m_follow.HeadMotion((sample.head + sample.bend) * scale, sample.upright, dt);
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
            // The raw head keeps the hip hinge the frame head leaves out (Sample::bend): compared without it, any hinge
            // held -- and the bend tracker holds one of a few mm from tracking noise alone -- kept roomscale rebasing,
            // the capsule frozen, for as long as it was held (measured: minutes, 2026-11-06).
            const Vec2 remaining = (sample.runtimeHead - sample.head - sample.bend) * scale;
            if (!sample.runtimeHeadKnown || Dot(remaining, remaining) <= 0.000001f)
                m_rebasingFilteredJump = false;
            step.reason = Reason::Discontinuity;
            return step;
        }
        m_pending=m_pending+delta;
        step.strict = strict;
        if (strict) { m_follow.Reset(); step.raw=m_pending; }
        else step.raw=m_follow.Step(m_pending*scale,freeRadiusMetres,dt)*(1.0f/scale);
        step.world = Rotate(step.raw*scale, trackingYaw);
        step.yaw = trackingYaw;
        step.scale = scale;
        step.dt = dt;
        step.reason = Reason::Applied;
        return step;
    }

    // Where the eye may be relative to the capsule's axis (world metres): eyeBase is
    // the view's fixed offset from the axis (the camera's calibration), the tracking
    // offset adds to it, and the sum stays within radius -- never tighter than where
    // the eye already is without the lead, so the limit never pulls the view.
    // ONLY AGAINST AN OBSTACLE: the limit holds while the controller refuses part of the
    // step. In the open the lead is the follow's own lag -- up to the free zone and the
    // response behind it -- and capping that too stopped the body outright: with the
    // 0.25 m capsule the eye may lead its axis by 0.20 m, it sits 0.13 m ahead already,
    // and the 0.07 m left never reached the 0.10 m free zone, so a step straight ahead
    // was consumed into the view and never walked (2026-11-04: "roomscale
    // moves in steps again"). Met with more lead than that, the eye is not thrown back
    // either: it stops where it is and eases back in at kLeadEaseBack.
    static constexpr float kLeadEaseBack = 0.10f;    // m/s
    static constexpr float kRefusedMin = 0.0005f;    // m a tick: less is the solver's rounding
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
        if (step.strict && step.reason == Reason::Applied) {
            // the accepted part moved the capsule, the refused part moves the view back: both are taken off the lead
            m_consumed = m_consumed + step.raw;
            m_pending = m_pending - step.raw;
            m_reach = -1;
            return;
        }
        m_consumed = m_consumed + accepted;
        if (step.reason != Reason::Applied) { m_reach = -1; return; }
        m_pending = m_pending - accepted;
        // Only an obstacle stops the follow, not the solver's rounding (under kRefusedMin a tick). Block() takes the
        // follow's whole speed along what was refused, and the controller refuses a few hundredths of a millimetre
        // along the walk on open floor every other tick: each time the follow lost its speed and the next request
        // sagged to half (live, 2026-11-06: every dip of a steady walk, 0.65 -> 0.30 m/s, followed such a tick; the
        // walk surged and sagged at the animation's expense).
        if (Finite(refusedWorld) && Dot(refusedWorld, refusedWorld) > kRefusedMin * kRefusedMin)
            m_follow.Block(step.raw - accepted);
        if (!std::isfinite(limit.radius) || !Finite(limit.eyeBase) || step.scale <= 0) { m_reach = -1; return; }
        const Vec2 base = limit.eyeBase + Rotate((m_previous - m_consumed - m_pending) * step.scale, step.yaw);
        Vec2 eye = base + Rotate(m_pending * step.scale, step.yaw);
        float reach = std::sqrt(Dot(eye, eye));
        const bool blocked = Finite(refusedWorld) && Dot(refusedWorld, refusedWorld) > kRefusedMin * kRefusedMin;
        if (blocked) {
            float bound = std::max(std::max(limit.radius, 0.0f), std::sqrt(Dot(base, base)));
            const float dt = std::isfinite(step.dt) ? std::clamp(step.dt, 0.0f, 0.05f) : 0.0f;
            if (m_reach >= 0) bound = std::max(bound, m_reach - kLeadEaseBack * dt);
            if (reach > bound) {
                const Vec2 excess = Rotate(eye * (1.0f - bound / reach), -step.yaw) * (1.0f / step.scale);
                m_consumed = m_consumed + excess;
                m_pending = m_pending - excess;
                eye = eye * (bound / reach);
                reach = bound;
            }
        }
        m_reach = reach;
    }

    Vec2 Consumed(uint64_t origin) const { return origin == m_origin ? m_consumed : Vec2{}; }
    // THE NECK AT REST: where the player's neck stands, less the lead the body has yet to follow, in the tracked
    // residual's frame (tracking metres; the residual -- the head less Consumed -- is zero at the calibrated eye). What
    // moves the neck and the capsule alike leaves it alone (walking, a wall's refusal, a tracking jump); what moves the
    // head and not the capsule (a suspension, the head where it was when roomscale began) moves it. The body's physical
    // turns pivot about it (Camera/AnchorTranslation.hpp BodyOrbitModel). False until a head has been sampled.
    bool RestNeck(uint64_t origin, Vec2* out) const {
        if (!out || !m_origin || origin != m_origin || !m_haveHead) return false;
        const Vec2 neck = m_previous - m_consumed - m_pending;
        if (!Finite(neck)) return false;
        *out = neck;
        return true;
    }
    // THE EYE AS THE LAST MOVE SAW IT, in the tracked residual's frame (the head less Consumed): where the camera was
    // when the capsule was last placed. The rendered camera has moved on since by the head's motion since that sample;
    // the visible body follows that too (Hooks/AnimPose.cpp, CyberpunkVR_BodyUnderRenderedHead).
    bool TickEye(uint64_t origin, Vec2* out) const {
        if (!out || !m_origin || origin != m_origin || !m_haveHead) return false;
        const Vec2 eye = m_previous - m_consumed - m_pending + m_eyeFromNeck;
        if (!Finite(eye)) return false;
        *out = eye;
        return true;
    }
    // The body is walking after the head (a follow in progress).
    bool Following() const { return m_active && m_follow.Following(); }
    // The head's lead over the capsule that the body still has to follow (tracking units).
    Vec2 Pending() const { return m_pending; }

private:
    Vec2 m_previous{}, m_consumed{};
    Vec2 m_pending{};
    Vec2 m_eyeFromNeck{};
    float m_reach = -1;   // the eye's distance from the axis after the last move; -1 unknown
    BodyTranslationFollow m_follow;
    Vec2 m_previousRuntime{};
    uint64_t m_origin{}, m_sequence{}, m_tickUs{}, m_ticket{}, m_completed{};
    bool m_active{}, m_haveHead{};
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
// AT MOST THE LOSS ITSELF: the share is clamped to 1. Unclamped, a loss of a hundredth of a
// millimetre -- the solver's rounding on open floor -- withheld the request's whole component
// along it: 9.6 mm asked and moved, 5.1 mm accepted. The rest stayed in the lead, the follow
// asked for it again, and the capsule overran the headset (1.23-1.36 m for a 1 m walk,
// measured 2026-11-04).
inline Vec2 AcceptedDisplacement(Vec2 native, Vec2 physical, Vec2 resolved) {
    if (!Finite(native) || !Finite(physical) || !Finite(resolved)) return physical;
    const Vec2 lost = native + physical - resolved;
    const float lost2 = Dot(lost, lost);
    if (lost2 <= 1e-12f) return physical;
    return physical - lost * std::clamp(Dot(physical, lost) / lost2, 0.0f, 1.0f);
}

inline Vec2 PhysicalVelocity(Vec2 native, Vec2 physical, Vec2 resolvedVelocity, float dt) {
    if (!Finite(native) || !Finite(physical) || !Finite(resolvedVelocity) ||
        !std::isfinite(dt) || dt <= 0.000001f || Dot(physical, physical) < 1e-14f) return {};
    if (Dot(native, native) < 1e-12f) return resolvedVelocity;
    const Vec2 lost = native + physical - resolvedVelocity * dt;
    const float lost2 = Dot(lost, lost);
    Vec2 accepted = physical;
    if (lost2 > 1e-8f) {
        // at most the loss itself, as in AcceptedDisplacement
        accepted = physical - lost * std::clamp(Dot(physical, lost) / lost2, 0.0f, 1.0f);
    }
    return accepted * (1.0f / dt);
}

} // namespace cvr::roomscale
