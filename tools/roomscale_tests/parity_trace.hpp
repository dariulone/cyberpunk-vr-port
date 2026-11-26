#pragma once
// trace: roomscale, the crouch, the native heading, hand reacquisition, the view's anchor and frames, the vehicle
// heading, the eye-centre ledger and the scene/input policies on long deterministic random input, every output hashed
// bit for bit. The C++ build and the Rust build (CVR_RUST_ROOM, rust/crates/cvr-body and cvr-camera) must print the
// same lines (docs/rust-migration-plan.md, stage 3).
//
// EVERY RANDOM INPUT IS DRAWN INTO A NAMED LOCAL FIRST, in statement order. C++ leaves the order in which a call's
// arguments are evaluated unspecified, and MSVC changes it with inlining: draws inside one argument list made the two
// builds consume the generator differently (and one build differ from itself after a rebuild).
#include "Runtimes/NativeBodyHeading.hpp"
#include "Runtimes/PhysicalCrouch.hpp"
#include "Runtimes/TrackingReacquire.hpp"
#include "Runtimes/PipelinePrediction.hpp"
#include "Runtimes/RuntimeFovCorrection.hpp"
#include "Runtimes/AsymmetricProjection.hpp"
#include <bit>
#include <cstdint>
#include <cstdio>
#include <utility>

namespace parity {
struct Hash {
    uint64_t h = 1469598103934665603ull, n = 0;
    void Bytes(uint64_t v, int bytes) { for (int i = 0; i < bytes; ++i) { h ^= (v >> (8 * i)) & 0xFF; h *= 1099511628211ull; } ++n; }
    // A NaN is hashed as one value. x86 passes on the NaN of an instruction's first operand, and which operand comes
    // first is the compiler's choice (MSVC and LLVM choose differently), not the code's: a NaN's sign and payload are
    // no behaviour. NaN against a number is still a difference.
    void F(float v) { Bytes(v != v ? 0x7FC00000u : std::bit_cast<uint32_t>(v), 4); }
    void B(bool v) { Bytes(v ? 1 : 0, 1); }
    void U(uint64_t v) { Bytes(v, 8); }
    void V(cvr::roomscale::Vec2 v) { F(v.x); F(v.y); }
    void A(cvr::camera::AnchorVector v) { F(v.x); F(v.y); F(v.z); }
    void Q(XrQuaternionf q) { F(q.x); F(q.y); F(q.z); F(q.w); }
};
struct Rng {
    uint64_t s;
    explicit Rng(uint64_t seed) : s(seed) {}
    uint64_t Next() { s ^= s << 13; s ^= s >> 7; s ^= s << 17; return s; }
    float Unit() { return float(Next() >> 40) / float(1ull << 24); }
    float Range(float a, float b) { const float u = Unit(); return a + (b - a) * u; }
    bool Chance(uint32_t oneIn) { return Next() % oneIn == 0; }
    float Special(float normal) {
        switch (Next() % 400) { case 0: return std::numeric_limits<float>::quiet_NaN(); case 1: return INFINITY;
                                case 2: return -INFINITY; default: return normal; }
    }
    float SR(float a, float b) { const float v = Range(a, b); return Special(v); }   // a range, sometimes not finite
    cvr::roomscale::Vec2 V(float r) { const float x = SR(-r, r); const float y = SR(-r, r); return {x, y}; }
    cvr::camera::AnchorVector A(float r) { const float x = SR(-r, r); const float y = SR(-r, r); const float z = SR(-r, r); return {x, y, z}; }
    XrQuaternionf Q() {
        const float yaw = Range(-7, 7); const float pitch = Range(-2, 2); const float roll = Range(-2, 2);
        const bool scaled = Chance(30); const float s = scaled ? Range(0, 2) : 1;
        const XrQuaternionf y{0, std::sin(yaw / 2), 0, std::cos(yaw / 2)}, p{std::sin(pitch / 2), 0, 0, std::cos(pitch / 2)},
            r{0, 0, std::sin(roll / 2), std::cos(roll / 2)};
        const auto q = MultiplyQuat(MultiplyQuat(y, p), r);
        const float w = Special(q.w * s);
        return {q.x * s, q.y * s, q.z * s, w};
    }
};

inline void Movement(Hash& out, uint64_t seed) {
    using cvr::roomscale::Vec2;
    Rng r(seed);
    cvr::roomscale::Movement m;
    cvr::roomscale::Sample s;
    s.valid = true; s.sequence = 1; s.origin = 1; s.stampUs = 1000000;
    uint64_t now = 1000000;
    Vec2 head{};
    for (int i = 0; i < 150000; ++i) {
        const bool hitch = r.Chance(200);
        const float dt = hitch ? r.Range(.2f, .4f) : r.Range(.007f, .02f);
        now += uint64_t(dt * 1e6f);
        const float hx = r.Range(-.01f, .01f); const float hy = r.Range(-.01f, .01f); const bool fast = r.Chance(5);
        head = head + Vec2{hx, hy} * (fast ? 3.0f : 1.0f);
        if (r.Chance(800)) { const float jx = r.Range(-1, 1); const float jy = r.Range(-1, 1); head = head + Vec2{jx, jy}; }
        const bool broken = r.Chance(300);
        s.head = broken ? Vec2{NAN, 0} : head;
        const float nx = r.Range(-.002f, .002f); const float ny = r.Range(-.002f, .002f);
        s.runtimeHead = head + Vec2{nx, ny};
        s.runtimeHeadKnown = !r.Chance(10);
        if (r.Chance(4)) { const float bx = r.Range(-.1f, .1f); const float by = r.Range(-.1f, .2f); s.bend = {bx, by}; } else s.bend = {};
        s.upright = !r.Chance(6);
        s.valid = !r.Chance(200);
        s.sequence += r.Chance(20) ? 0 : 1;
        if (r.Chance(3000)) s.origin += 1;
        const bool stale = r.Chance(300);
        const uint64_t age = uint64_t(r.Range(0, 5000));
        s.stampUs = stale ? now - 300000 : now - age;
        const float stepDt = r.Special(dt);
        const bool oddScale = r.Chance(100);
        const float scale = oddScale ? r.Range(-1, 30) : r.Range(.8f, 1.2f);
        const float yaw = r.SR(-7, 7);
        const bool allowed = !r.Chance(50);
        const float radius = r.Range(0, .2f);
        const bool strict = r.Chance(3);
        const auto step = m.Begin(s, now, stepDt, scale, yaw, allowed, radius, strict);
        out.V(step.raw); out.V(step.world); out.F(step.yaw); out.F(step.scale); out.F(step.dt); out.U(step.origin);
        out.U(step.ticket); out.U(uint64_t(step.reason));
        if (r.Chance(2)) m.Complete(step);
        else {
            cvr::roomscale::Movement::LeadLimit limit;
            limit.eyeBase = r.V(.2f);
            const bool unlimited = r.Chance(4);
            const float limitRadius = r.SR(-.1f, .3f);
            limit.radius = unlimited ? std::numeric_limits<float>::infinity() : limitRadius;
            const float share = r.Range(0, 1.1f);
            const Vec2 noise = r.V(.001f);
            m.Complete(step, step.world * share + noise, limit);
        }
        out.V(m.Consumed(s.origin)); out.V(m.Consumed(s.origin + 1)); out.V(m.Pending()); out.B(m.Following());
        Vec2 neck{}; out.B(m.RestNeck(s.origin, &neck)); out.V(neck);
        if (r.Chance(2000)) m.Suspend();
        if (r.Chance(9000)) m.Reset();
    }
    cvr::roomscale::BodyTranslationFollow f;
    cvr::roomscale::PhysicalWalkGate g;
    for (int i = 0; i < 100000; ++i) {
        const Vec2 neck = r.V(.1f); const bool upright = !r.Chance(5); const float motionDt = r.SR(-.01f, .03f);
        f.HeadMotion(neck, upright, motionDt);
        out.B(f.HeadStill());
        if (r.Chance(7)) { const Vec2 refused = r.V(.01f); f.Block(refused); }
        const Vec2 error = r.V(.3f); const float radius = r.SR(-.05f, .2f); const float dt = r.SR(-.01f, .06f);
        out.V(f.Step(error, radius, dt)); out.B(f.Following());
        if (r.Chance(500)) f.Reset();
        const Vec2 velocity = r.V(1.5f); const float gateDt = r.SR(-.01f, .2f);
        g.Update(velocity, gateDt); out.B(g.Walking()); out.V(g.Reported());
        if (r.Chance(3000)) g.Reset();
        const auto q = r.Q();
        float yaw = -1; out.B(cvr::roomscale::HeadYaw(q.x, q.y, q.z, q.w, &yaw)); out.F(yaw);
        out.V(cvr::roomscale::EyeFromNeck(q.x, q.y, q.z, q.w));
        cvr::roomscale::Sample a, b;
        a.head = r.V(1); a.valid = !r.Chance(5); a.origin = r.Next() % 3; a.stampUs = r.Next() % 1000;
        b.head = r.V(1); b.valid = !r.Chance(5); b.origin = r.Next() % 3; b.stampUs = r.Next() % 1000; b.sequence = r.Next();
        const auto ms = cvr::roomscale::MovementFrameSample(a, b);
        out.V(ms.head); out.B(ms.valid); out.U(ms.origin); out.U(ms.stampUs); out.V(ms.runtimeHead); out.B(ms.runtimeHeadKnown);
        const Vec2 n1 = r.V(.05f); const Vec2 p1 = r.V(.05f); const Vec2 r1 = r.V(.05f);
        out.V(cvr::roomscale::AcceptedDisplacement(n1, p1, r1));
        const Vec2 n2 = r.V(.05f); const Vec2 p2 = r.V(.05f); const Vec2 r2 = r.V(3); const float vdt = r.SR(-.01f, .05f);
        out.V(cvr::roomscale::PhysicalVelocity(n2, p2, r2, vdt));
        const Vec2 v = r.V(2); const float a1 = r.SR(-7, 7);
        out.V(cvr::roomscale::Rotate(v, a1));
        const float e = r.SR(-9, 9); const float realign = r.Range(-9, 9);
        out.F(cvr::roomscale::TrackingYaw(e, realign));
    }
}

inline void Crouch(Hash& out, uint64_t seed) {
    using namespace cvr::body;
    Rng r(seed);
    PhysicalCrouch c;
    uint64_t now = 1000, origin = 1, owner = 5;
    for (int i = 0; i < 150000; ++i) {
        now += r.Chance(100) ? 200000 : uint64_t(r.Range(5000, 20000));
        if (r.Chance(2000)) origin += 1;
        if (r.Chance(3000)) owner = r.Chance(5) ? 0 : owner + 1;
        const float right = r.SR(-.5f, .5f); const float forward = r.Range(-.5f, .5f); const float angle = r.Range(0, 1);
        const BendSample b{right, forward, angle};
        const float drop = r.SR(0, 1);
        const float depth = CrouchDepth(drop, b);
        out.F(depth);
        const float threshold = r.SR(0, .8f);
        out.F(CrouchThreshold(threshold));
        const bool useDepth = r.Chance(4); const float other = r.SR(-.1f, 1.7f); const float enter = r.SR(0, .8f);
        const bool zeroNow = r.Chance(400); const bool allowed = !r.Chance(300);
        out.B(c.Update(useDepth ? depth : other, enter, zeroNow ? 0 : now, origin, owner, allowed));
        out.B(c.Active());
        if (r.Chance(5000)) c.Reset();
    }
    NativeBodyHeading h;
    for (int i = 0; i < 100000; ++i) {
        const uintptr_t owner2 = r.Next() % 4;
        const uint64_t origin2 = r.Next() % 3;
        const float yaw = r.SR(-9, 9);
        out.B(h.Observe(owner2, origin2, yaw));
        if (r.Chance(2)) { const float body = r.SR(-9, 9); const float turn = r.SR(-1, 1); h.Commit(body, turn); }
        out.B(h.Matches(owner2, origin2)); out.B(h.OwnedBy(owner2)); out.F(h.View()); out.F(h.Body()); out.F(h.CentreYaw());
        out.F(h.Offset());
        const float w = r.SR(-20, 20);
        out.F(NativeBodyHeading::Wrap(w));
        if (r.Chance(5000)) h.Reset();
    }
    cvr::tracking::Reacquire q;
    cvr::tracking::HandPose pose;
    for (int i = 0; i < 100000; ++i) {
        for (float& p : pose.p) p += r.Range(-.01f, .01f);
        if (r.Chance(100)) for (float& p : pose.p) p += r.Range(-.3f, .3f);
        const auto rq = r.Q();
        if (r.Chance(10)) { pose.q[0] = rq.x; pose.q[1] = rq.y; pose.q[2] = rq.z; pose.q[3] = rq.w; }
        const bool burst = r.Chance(50); const bool lost = r.Chance(burst ? 2 : 40); const float dt = r.SR(-.01f, .05f);
        const auto o = q.Step(pose, !lost, dt);
        for (float v : o.p) out.F(v);
        for (float v : o.q) out.F(v);
        out.F(q.Blend());
        float velocity = r.Range(-3, 3);
        const float current = r.Range(-2, 2); const float target = r.Range(-2, 2); const float time = r.Range(-.1f, .5f);
        const float sdt = r.Range(-.01f, .1f);
        out.F(cvr::tracking::SmoothDamp(current, target, velocity, time, sdt));
        out.F(velocity);
        const float rate = r.SR(-5, 50); const float edt = r.SR(-.01f, .2f);
        out.F(cvr::tracking::ExponentialFollow(rate, edt));
        if (r.Chance(5000)) q.Reset();
    }
}

inline void Camera(Hash& out, uint64_t seed) {
    using namespace cvr::camera;
    Rng r(seed);
    for (int i = 0; i < 120000; ++i) {
        const AnchorVector v0 = r.A(2); const float y0 = r.SR(-7, 7);
        out.A(RotateAnchorYaw(v0, y0));
        const AnchorVector c1 = r.A(1); const AnchorVector c2 = r.A(1); const AnchorVector c3 = r.A(1);
        const float ty = r.Range(-7, 7); const float my = r.Range(-7, 7);
        out.A(ComposeAnchorTranslation(c1, c2, c3, ty, my));
        const AnchorVector manual = r.A(.3f); const AnchorVector vehicle = r.A(.3f); const AnchorVector baked = r.A(.3f);
        const bool inVehicle = r.Chance(3); const float scale = r.SR(-.5f, 2); const float ry = r.Range(-7, 7);
        const float rm = r.Range(-7, 7);
        auto recipe = MakeAnchorRecipe(manual, vehicle, baked, inVehicle, scale, ry, rm);
        recipe.bodyOrbit = r.Chance(2);
        out.A(recipe.trackingOffset); out.A(recipe.modelOffset); out.F(recipe.scale); out.F(recipe.centreYaw);
        const AnchorVector residual = r.A(1); const float vy = r.Range(-7, 7); const float by = r.Range(-7, 7);
        out.A(ComposeViewTranslation(residual, recipe, vy, by));
        const AnchorVector rest = r.A(.3f);
        const bool noRest = r.Chance(3);
        out.A(BodyOrbitNeckLever(recipe, noRest ? nullptr : &rest));
        const float oy = r.SR(-7, 7); const float oty = r.SR(-7, 7); const bool noRest2 = r.Chance(3);
        out.A(BodyOrbitModel(recipe, oy, oty, noRest2 ? nullptr : &rest));
        const AnchorVector b1 = r.A(1); const AnchorVector b2 = r.A(1); const AnchorVector b3 = r.A(1);
        out.A(AbsoluteHorizontalCameraBake(b1, b2, b3));
        const AnchorVector base = r.A(2); const AnchorVector res = r.A(1); const auto headQ = r.Q();
        const float fy = r.Range(-7, 7); const float fty = r.Range(-7, 7); const auto pitch = r.Q();
        const auto frame = BuildHandViewFrame(base, recipe, res, headQ, fy, fty, pitch);
        out.A(frame.position); out.Q(frame.rotation);
        const int32_t cx = int32_t(r.Next()); const int32_t cy = int32_t(r.Next() % 300000); const int32_t cz = int32_t(r.Next());
        FixedPosition c{cx, cy, cz}, b = c, e = c;
        b[0] += int32_t(r.Next() % 200000) - 100000;
        e[1] -= int32_t(r.Next() % 400000);
        const auto qa = r.Q(); const auto qb = r.Q();
        const float fa[4]{qa.x, qa.y, qa.z, qa.w}, fb[4]{qb.x, qb.y, qb.z, qb.w};
        const auto sample = MakeCameraEntitySample(c, b, e, fa, fb);
        for (float v : sample.cameraQuat) out.F(v);
        for (float v : sample.entityQuat) out.F(v);
        for (float v : sample.cameraMinusEntity) out.F(v);
        for (float v : sample.bodyMinusEntity) out.F(v);
        out.B(sample.valid);
        const uintptr_t o1 = r.Next() % 3; const uint64_t g1 = r.Next() % 3; const uint64_t st = r.Next() % 600;
        const uintptr_t o2 = r.Next() % 3; const uint64_t g2 = r.Next() % 3; const uint64_t nw = r.Next() % 600;
        const bool valid = r.Chance(2); const bool unavailable = r.Chance(2);
        out.U(uint64_t(CameraPacketStatus(o1, g1, st, o2, g2, nw, valid, unavailable)));
        const bool mounted = r.Chance(2); const int tier = int(r.Next() % 8) - 1; const bool device = r.Chance(4);
        const bool scene = r.Chance(4);
        out.B(UseNativeMainHeading(mounted, tier, device, scene));
        float yaw = -5; out.B(CompositionBaseYaw(fa, fb, &yaw)); out.F(yaw);
    }
    cvr::camera::VehicleStereoHeading v;
    for (int i = 0; i < 60000; ++i) {
        if (r.Chance(300)) { const uintptr_t p = r.Next() % 3; const uintptr_t m = r.Next() % 3; v.SetSource(p, m); }
        const auto q1 = r.Q(); const auto q2 = r.Q();
        const float a[4]{q1.x, q1.y, q1.z, q1.w}, b[4]{q2.x, q2.y, q2.z, q2.w};
        const uintptr_t camera = r.Next() % 3; const bool noQ = r.Chance(30); const bool noE = r.Chance(30);
        out.B(v.Observe(camera, noQ ? nullptr : a, noE ? nullptr : b));
        const uint64_t epoch = r.Next() % 4; const bool noE2 = r.Chance(20); const float pub = r.Range(-7, 7);
        const float off = r.Range(-1, 1);
        const auto s = v.ForFrame(epoch, noE2 ? nullptr : b, pub, off);
        out.F(s.yaw); out.B(s.changed);
    }
    cvr::camera::EyeCentreLedger ledger;
    std::vector<EyeCentreLedger::Position> eyes;
    for (int i = 0; i < 30000; ++i) {
        const int32_t ex = int32_t(r.Next() % 2000000); const int32_t ey = int32_t(r.Next() % 64); const int32_t ez = int32_t(r.Next());
        EyeCentreLedger::Position eye{ex, ey, ez};
        if (!eyes.empty() && r.Chance(4)) eye = eyes[r.Next() % eyes.size()];
        EyeCentreLedger::Position centre = eye, base = eye;
        centre[0] += int32_t(r.Next() % 3) - 1;
        base[2] += int32_t(r.Next() % 20000);
        ledger.Publish(eye, centre, base);
        eyes.push_back(eye);
        EyeCentreLedger::Position c{}, bb{};
        const auto& query = eyes[r.Next() % eyes.size()];
        const bool wantBase = r.Chance(2);
        out.B(ledger.Read(query, c, wantBase ? &bb : nullptr));
        for (int k = 0; k < 3; ++k) { out.F(float(c[k])); out.F(float(bb[k])); }
        EyeCentreLedger::WorldPosition wc{}, wb{};
        const bool wantWorldBase = r.Chance(2);
        out.B(ledger.ReadWorld(EyeCentreLedger::World(query), wc, wantWorldBase ? &wb : nullptr));
        for (int k = 0; k < 3; ++k) { out.F(wc[k]); out.F(wb[k]); }
    }
}

inline void Policies(Hash& out, uint64_t seed) {
    Rng r(seed);
    int armed = 0;
    cvr::input::VehicleExitHold hold;
    for (int i = 0; i < 100000; ++i) {
        const int a = int(r.Next() % 9) - 2; const int b = int(r.Next() % 9) - 2; const int c = int(r.Next() % 12) - 2;
        const int d = int(r.Next() % 3);
        out.B(cvr::anim::ShouldSuspendVrik(a, b));
        const bool inVehicle = r.Chance(2);
        out.B(cvr::anim::ShouldSuspendVrik(a, inVehicle, b, c));
        out.B(cvr::anim::ShouldSuspendVrikForTakedown(a, d)); out.B(cvr::anim::ShouldSuspendVrikForBodyCarry(a, b, c, d));
        const uint64_t age = r.Next() % 30000;
        out.B(cvr::anim::ShouldSuspendVrikForFinisher(a, age, d));
        out.B(cvr::anim::ShouldHandLeftArmToBodyCarry(a, d)); out.B(cvr::anim::ShouldHandRightArmToBodyCarry(a, b, d));
        const bool explicitTier = r.Chance(2);
        out.U(uint64_t(int64_t(cvr::anim::ResolveVehicleVrikSuspendTier(a * 3, b * 3, explicitTier))));
        const int binding = int(r.Next() % 6); const bool headAim = r.Chance(3); const bool vehicle = r.Chance(3);
        out.B(cvr::roomscale::IsVrikBodyMovementAllowed(binding, headAim, vehicle, a, b));
        const float axis = r.SR(-1.2f, 1.2f); const bool snap = !r.Chance(4); const float angle = r.Range(-10, 60);
        const float fire = r.SR(-.2f, 1.2f); const float rearm = r.SR(-.2f, 1.2f);
        const auto t = cvr::input::RouteTurn(axis, snap, armed, angle, fire, rearm);
        out.F(t.axis); out.F(t.snapDegrees); out.B(t.clearPending); out.U(uint64_t(int64_t(armed)));
        const bool mounted = r.Chance(2); const int state = int(r.Next() % 9) - 1;
        const auto vb = cvr::input::VehicleButtonPolicy(mounted, state);
        out.U(vb.owned); out.B(vb.exitOnX); out.B(vb.confirmOnA);
        const bool allowed = !r.Chance(20); const bool held = !r.Chance(10); const uint64_t now = r.Next() % 5000;
        const uint64_t holdMs = r.Next() % 2000;
        out.B(hold.Update(allowed, held, now, holdMs));
    }
}

// The pipeline prediction on random cadences, fits and strengths; the runtime FOV correction and the panel's covering
// FOV on random frusta (rust/crates/cvr-camera/src/projection.rs).
inline void Projection(Hash& out, uint64_t seed) {
    Rng r(seed);
    cvr::tracking::PipelinePrediction p;
    int64_t display = 1000000000;
    for (int i = 0; i < 100000; ++i) {
        const int64_t period = int64_t(r.Next() % 50000000);
        const bool jump = r.Chance(50);
        const int64_t step = jump ? int64_t(r.Next() % 400000000) - 100000000 : period;
        display += step;
        const int64_t fitted = display + int64_t(r.Next() % 200000000) - 50000000;
        const bool valid = !r.Chance(5);
        const float strength = r.SR(-.5f, 1.5f);
        if (r.Chance(500)) p.Reset();
        out.U(uint64_t(p.Target(display, period, fitted, valid, strength)));
        const XrFovf left{r.SR(-1.2f, .2f), r.SR(-.2f, 1.2f), r.SR(-.2f, 1.2f), r.SR(-1.2f, .2f)};
        const XrFovf right{r.SR(-1.2f, .2f), r.SR(-.2f, 1.2f), r.SR(-.2f, 1.2f), r.SR(-1.2f, .2f)};
        const auto c = ComputeRuntimeFovCorrection(left, right);
        for (const auto& e : c.eye) { out.F(e.angleLeft); out.F(e.angleRight); out.F(e.angleUp); out.F(e.angleDown); }
        out.F(c.yawDeltaRad); out.F(c.pitchDeltaRad); out.B(c.yawEnabled); out.B(c.pitchEnabled);
        out.F(GetCorrectedGameHorizontalFovDeg(c));
        float h = 0, v = 0;
        GetPanelHalfAngles(left, right, &h, &v);
        out.F(h); out.F(v);
        const float aspect = r.SR(0, 3);
        out.F(GetPanelCoveringHorizontalFovDeg(left, right, aspect));
        // the asymmetric projection's windows
        const XrFovf fov[2] = {left, right};
        const XrQuaternionf eyes[2] = {r.Q(), r.Q()};
        const bool oriented = !r.Chance(4);
        const float guard = r.Chance(3) ? 0.0f : r.Range(0, 1);
        const float target = r.Chance(4) ? 0.0f : r.SR(0, 6);
        cvr::projection::EyeWindow w[2];
        const bool ok = cvr::projection::WindowsWith(fov, oriented ? eyes : nullptr, w, guard, target);
        out.B(ok);
        if (ok) {
            for (const auto& e : w) {
                out.F(e.l); out.F(e.r); out.F(e.d); out.F(e.u);
                const auto f = cvr::projection::ToFov(e);
                out.F(f.angleLeft); out.F(f.angleRight); out.F(f.angleUp); out.F(f.angleDown);
                const auto p = cvr::projection::ToEngine(e);
                out.F(p.vfovDeg); out.F(p.aspect); out.F(p.shiftX); out.F(p.shiftY);
            }
            const float n = r.SR(-1.5f, 1.5f);
            out.F(cvr::projection::MapNdcX(n, w[0], w[1])); out.F(cvr::projection::MapNdcY(n, w[1], w[0]));
        }
        float yaw[2];
        cvr::projection::EyeYawsAgainstHead(eyes[0], eyes[1], yaw);
        out.F(yaw[0]); out.F(yaw[1]);
    }
}

inline void Trace() {
    using Section = void (*)(Hash&, uint64_t);
    const std::pair<const char*, Section> sections[] = {{"movement", Movement}, {"crouch", Crouch}, {"camera", Camera},
                                                        {"policies", Policies}, {"projection", Projection}};
    Hash all;
    for (const auto& [name, run] : sections) {
        Hash part;
        for (uint64_t seed : {1ull, 0x9E3779B97F4A7C15ull, 42ull}) run(part, seed);
        // One hash per section, so a difference names the module.
        std::printf("  %s %016llx %llu\n", name, static_cast<unsigned long long>(part.h), static_cast<unsigned long long>(part.n));
        all.U(part.h);
        all.n += part.n;
    }
    std::printf("trace %016llx %llu\n", static_cast<unsigned long long>(all.h), static_cast<unsigned long long>(all.n));
}
}
