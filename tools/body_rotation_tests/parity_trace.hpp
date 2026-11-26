#pragma once
// trace: every body module on long deterministic random input, every output hashed bit for bit. The C++ build and the
// Rust build (CVR_RUST_BODY, rust/crates/cvr-body) must print the same line (docs/rust-migration-plan.md, stage 3).
#include "Anim/BodyBend.hpp"
#include "Runtimes/HybridBodyYaw.hpp"
#include "Runtimes/LookDownCone.hpp"
#include <bit>
#include <cstdint>
#include <cstdio>

namespace parity {
struct Hash {
    uint64_t h = 1469598103934665603ull, n = 0;
    void Bytes(uint64_t v, int bytes) { for (int i = 0; i < bytes; ++i) { h ^= (v >> (8 * i)) & 0xFF; h *= 1099511628211ull; } ++n; }
    // A NaN is hashed as one value. x86 passes on the NaN of an instruction's first operand, and which operand comes
    // first is the compiler's choice (MSVC and LLVM choose differently), not the code's: a NaN's sign and payload are
    // no behaviour. NaN against a number is still a difference.
    void F(float v) { Bytes(v != v ? 0x7FC00000u : std::bit_cast<uint32_t>(v), 4); }
    void B(bool v) { Bytes(v ? 1 : 0, 1); }
    void U(uint32_t v) { Bytes(v, 4); }
    void E(const cvr::body::YawEstimate& e) { F(e.yaw); F(e.confidence); B(e.valid); }
};
struct Rng {
    uint64_t s;
    explicit Rng(uint64_t seed) : s(seed) {}
    uint64_t Next() { s ^= s << 13; s ^= s >> 7; s ^= s << 17; return s; }
    float Unit() { return float(Next() >> 40) / float(1ull << 24); }               // [0,1)
    float Range(float a, float b) { return a + (b - a) * Unit(); }
    bool Chance(uint32_t oneIn) { return Next() % oneIn == 0; }
    float Special(float normal) {                                                   // occasionally not finite
        switch (Next() % 400) { case 0: return std::numeric_limits<float>::quiet_NaN(); case 1: return INFINITY;
                                case 2: return -INFINITY; default: return normal; }
    }
};

inline XrQuaternionf YawPitchRoll(float yaw, float pitch, float roll) {
    const XrQuaternionf y{0, std::sin(yaw / 2), 0, std::cos(yaw / 2)}, p{std::sin(pitch / 2), 0, 0, std::cos(pitch / 2)},
        r{0, 0, std::sin(roll / 2), std::cos(roll / 2)};
    return MultiplyQuat(MultiplyQuat(y, p), r);
}

inline void Tracked(Hash& out, uint64_t seed) {
    using namespace cvr::body;
    Rng r(seed);
    TrackedBodyYaw t;
    t.Reset(r.Range(-3, 3), r.Chance(2));
    float body = 0, head = 0, pitch = 0;
    uint64_t seq = 1, stamp = 1000000, origin = 1;
    int pose = 0;
    for (int i = 0; i < 60000; ++i) {
        // a person: the body turns in bursts, the head leads, the hands change hold now and then
        if (r.Chance(150)) pose = int(r.Next() % poses.size());
        const float turnRate = r.Chance(4) ? r.Range(-4, 4) : 0;
        const float dt = r.Chance(300) ? r.Range(.2f, .6f) : r.Range(.008f, .016f);
        body += turnRate * dt;
        head += (body + r.Range(-.8f, .8f) - head) * .2f;
        pitch = std::clamp(pitch + r.Range(-.05f, .05f), -1.2f, 1.2f);
        stamp += uint64_t(dt * 1e6f);
        seq = r.Chance(500) ? seq : seq + 1;
        if (r.Chance(5000)) origin += 1;
        if (r.Chance(3000)) seq = 0xFFFFFFF0u;                                     // the 32-bit wrap
        auto frame = Frame(poses[pose], body, head, seq, stamp, pitch, r.Range(-.1f, .1f), r.Range(.8f, 1.2f));
        frame.origin = origin;
        for (int h = 0; h < 2; ++h) {
            frame.hands[h].x += r.Range(-.003f, .003f); frame.hands[h].y += r.Range(-.003f, .003f);
            frame.hands[h].z = r.Special(frame.hands[h].z + r.Range(-.003f, .003f));
            frame.tracked[h] = !r.Chance(800);
        }
        if (r.Chance(700)) frame.valid = false;
        if (r.Chance(900)) { const float j = r.Range(-2, 2); frame.head = MultiplyQuat(Yaw(j), frame.head); }
        out.E(t.Update(frame));
        out.E(t.Predict(stamp + uint64_t(r.Range(0, 200000))));
        out.B(t.Ready());
        const auto& tr = t.trace;
        out.F(tr.delta); out.F(tr.wristMean); out.F(tr.headDelta); out.F(tr.headLead); out.F(tr.accepted);
        out.F(tr.evidence); out.F(tr.residual); out.F(tr.target); out.U(tr.bits);
        out.B(TrackingFrameFresh(frame, origin, stamp + uint64_t(r.Range(0, 200000))));
        if (r.Chance(4000)) t.Suspend();
        if (r.Chance(9000)) t.Reset(r.Special(r.Range(-7, 7)), r.Chance(2));
    }
}

inline void Hybrid(Hash& out, uint64_t seed) {
    using namespace cvr::body;
    Rng r(seed);
    HybridBodyYaw y;
    HybridInput in;
    in.origin = 1;
    in.stampUs = 1000;
    float headYaw = 0, estimate = 0;
    for (int i = 0; i < 120000; ++i) {
        headYaw += r.Range(-.1f, .1f);
        estimate += r.Range(-.05f, .05f);
        in.headYaw = r.Special(headYaw);
        in.headDownDegrees = r.Special(r.Chance(3) ? r.Range(-20, 60) : in.headDownDegrees);
        in.bodyBendRadians = r.Special(r.Chance(5) ? r.Range(0, 1.2f) : in.bodyBendRadians);
        in.bodyOffset = r.Special(r.Chance(2) ? r.Range(-7, 7) : in.bodyOffset);
        in.coneDegrees = r.Special(r.Range(-5, 70));
        in.loweredConeDegrees = r.Special(r.Range(-5, 100));
        in.estimate = {r.Special(estimate), r.Unit(), !r.Chance(20)};
        in.yawDefined = !r.Chance(30);
        in.estimateReady = !r.Chance(15);
        in.stampUs += r.Chance(200) ? 400000 : (r.Chance(300) ? 0 : uint64_t(r.Range(5000, 20000)));
        if (r.Chance(2000)) in.origin += r.Chance(2) ? 1 : uint64_t(-1);
        if (r.Chance(5000)) in.stampUs = 0;
        const auto c = y.Step(in);
        out.F(c.targetYaw); out.F(c.coneDegrees); out.B(c.tracked); out.F(c.trackedWeight); out.F(c.handsYaw);
        out.F(c.neckPull); out.F(c.alignment); out.B(c.estimate); out.B(y.Tracked()); out.F(y.TrackedWeight());
        if (r.Chance(3000)) y.Suspend(r.Chance(2));
        if (r.Chance(9000)) y.Reset();
        if (in.stampUs == 0) in.stampUs = 1000;
    }
    for (int p = 0; p < 2; ++p) for (int t = 0; t < 2; ++t) for (int h = 0; h < 2; ++h) {
        const auto mode = RotationModeFromFlags(p, t, h);
        out.U(uint32_t(mode)); out.B(NeedsBodyTracking(mode));
    }
}

inline void Followers(Hash& out, uint64_t seed) {
    using namespace cvr::body;
    Rng r(seed);
    cvr::roomscale::BodyFollowZone zone;
    cvr::roomscale::BodyYawFollower follower;
    BodyYawPrediction prediction;
    uint64_t stamp = 1000, origin = 7;
    float head = 0, measured = 0;
    for (int i = 0; i < 150000; ++i) {
        out.F(zone.Step(r.Special(r.Range(-3.5f, 3.5f)), r.Special(r.Range(-.1f, .6f)), r.Special(r.Range(-.01f, .4f))));
        out.B(zone.Following()); out.F(zone.Rate());
        if (r.Chance(1000)) zone.Reset();
        head += r.Range(-.08f, .08f);
        stamp += r.Chance(100) ? 300000 : (r.Chance(200) ? 0 : uint64_t(r.Range(5000, 15000)));
        if (r.Chance(3000)) origin += 1;
        if (r.Chance(500)) follower.SetEnabled(!r.Chance(3));
        if (r.Chance(400)) follower.Observe(r.Special(r.Range(-7, 7)), r.Chance(10) ? origin + 1 : origin);
        out.F(follower.Step(r.Special(head), r.Special(r.Range(-.1f, .5f)), stamp, origin));
        out.F(follower.Offset()); out.B(follower.Enabled());
        if (r.Chance(5000)) follower.Reset();
        measured += r.Chance(3) ? r.Range(-.03f, .06f) : 0;
        out.F(prediction.Update(r.Special(measured), r.Special(r.Chance(50) ? r.Range(-.01f, .2f) : r.Range(.008f, .016f)),
                                !r.Chance(10)));
        out.F(prediction.Sample(r.Special(r.Range(-.01f, .05f)))); out.F(prediction.Value()); out.F(prediction.Rate());
        if (r.Chance(800)) prediction.Suspend();
        if (r.Chance(4000)) prediction.Reset(r.Range(-3, 3));
    }
}

inline void Bend(Hash& out, uint64_t seed) {
    using namespace cvr::body;
    Rng r(seed);
    BendTracker t;
    std::array<float, 3> head{0, 1.7f, 0};
    float yaw = 0, pitch = 0;
    uint64_t origin = 1;
    for (int i = 0; i < 150000; ++i) {
        if (r.Chance(400)) t.SetSquatDepth(r.Special(r.Range(0, .8f)));
        yaw += r.Range(-.1f, .1f);
        pitch = std::clamp(pitch + r.Range(-.05f, .05f), -1.4f, 1.4f);
        const auto q = YawPitchRoll(yaw, pitch, r.Range(-.05f, .05f));
        head[0] += r.Range(-.01f, .01f); head[2] += r.Range(-.01f, .01f);
        head[1] = std::clamp(head[1] + r.Range(-.02f, .02f), .9f, 1.9f) - 1.7f + 1.7f;
        std::array<float, 3> h{head[0], r.Special(head[1] - 1.7f), head[2]};
        std::array<float, 4> qa{q.x, q.y, q.z, r.Special(q.w)};
        if (r.Chance(3000)) origin += 1;
        const auto s = t.Update(h, qa, origin, r.Chance(500), r.Chance(3));
        out.F(s.right); out.F(s.forward); out.F(s.angle); out.F(t.NeckDrop()); out.F(t.MeasuredAngle());
    }
    for (int i = 0; i < 20000; ++i) {
        const auto q = YawPitchRoll(r.Range(-7, 7), r.Range(-2, 2), r.Range(-2, 2));
        out.F(cvr::roomscale::HeadDownDegrees(q.x, q.y, q.z, r.Special(q.w * r.Range(.6f, 1.3f))));
        const float down = r.Special(r.Range(-30, 90)), bend = r.Special(r.Range(-.2f, 1.4f));
        out.F(cvr::roomscale::LookDownFreeLookCone(down, r.Special(r.Range(-5, 70)), r.Special(r.Range(-5, 100)), bend));
        out.F(cvr::roomscale::BodyFreeLookCone(r.Chance(5), r.Special(r.Range(-5, 100)), down, r.Range(0, 60),
                                               r.Range(0, 90), bend));
        out.F(SmoothRange(r.Special(r.Range(-1, 2)), r.Range(-1, 0), r.Range(0, 2)));
        out.F(BendWeight(bend)); out.F(HingeDrop(bend));
    }
}

inline void Trace() {
    Hash out;
    for (uint64_t seed : {1ull, 0x9E3779B97F4A7C15ull, 42ull}) { Tracked(out, seed); Hybrid(out, seed); Followers(out, seed); Bend(out, seed); }
    std::printf("trace %016llx %llu\n", static_cast<unsigned long long>(out.h), static_cast<unsigned long long>(out.n));
}
}
