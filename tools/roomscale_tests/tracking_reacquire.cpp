// Tests for Runtimes/TrackingReacquire.hpp: a hand coming back into tracking glides instead of jumping.
#include "Runtimes/TrackingReacquire.hpp"
#include <cmath>
#include <cstdio>

using cvr::tracking::HandPose;
using cvr::tracking::Reacquire;

namespace {
int g_failed = 0;
void Check(bool ok, const char* what) {
    if (!ok) {
        std::printf("FAIL %s\n", what);
        ++g_failed;
    }
}
float Distance(const HandPose& a, const HandPose& b) {
    const float dx = a.p[0] - b.p[0], dy = a.p[1] - b.p[1], dz = a.p[2] - b.p[2];
    return std::sqrt(dx * dx + dy * dy + dz * dz);
}
float Angle(const HandPose& a, const HandPose& b) {
    const float d = std::fabs(a.q[0] * b.q[0] + a.q[1] * b.q[1] + a.q[2] * b.q[2] + a.q[3] * b.q[3]);
    return 2.0f * std::acos(std::fmin(1.0f, d));
}
HandPose At(float x, float y, float z, float yaw = 0.0f) {
    HandPose p;
    p.p[0] = x, p.p[1] = y, p.p[2] = z;
    p.q[0] = 0, p.q[1] = std::sin(yaw * 0.5f), p.q[2] = 0, p.q[3] = std::cos(yaw * 0.5f);
    return p;
}
}  // namespace

int main() {
    const float dt = 1.0f / 90.0f;
    {
        Reacquire r;
        for (int i = 0; i < 50; ++i) {
            const HandPose in = At(0.01f * i, 0.0f, -0.3f, 0.01f * i);
            const HandPose out = r.Step(in, true, dt);
            Check(Distance(in, out) < 1e-6f && Angle(in, out) < 1e-3f, "tracked hand passes through");
        }
    }
    {
        Reacquire r;
        const HandPose before = At(0.2f, -0.3f, -0.4f, 0.0f);
        for (int i = 0; i < 10; ++i) r.Step(before, true, dt);
        for (int i = 0; i < 30; ++i) {
            const HandPose held = r.Step(HandPose{}, false, dt);
            Check(Distance(held, before) < 1e-6f, "a lost hand keeps its last pose");
        }
        const HandPose after = At(0.3f, -0.3f, -0.4f, 1.5708f);   // 10 cm and 90 degrees away
        HandPose out = r.Step(after, true, dt);
        Check(Distance(out, before) < 0.015f, "the first pose back starts where the hand was");
        Check(Angle(out, before) < 0.25f, "and turned as it was");
        float last = Distance(out, after);
        bool monotonic = true;
        for (int i = 1; i < 45; ++i) {   // half a second
            out = r.Step(after, true, dt);
            const float d = Distance(out, after);
            monotonic &= d <= last + 1e-6f;
            last = d;
        }
        Check(monotonic, "the gap only closes");
        Check(Distance(out, after) < 0.002f && Angle(out, after) < 0.02f, "within half a second the hand is there");
        Check(r.Blend() == 0.0f, "and the blend has ended");
    }
    {
        Reacquire r;
        const HandPose before = At(0.0f, 0.0f, -0.3f);
        r.Step(before, true, dt);
        r.Step(HandPose{}, false, dt);
        const HandPose after = At(0.001f, 0.0f, -0.3f);
        const HandPose out = r.Step(after, true, dt);
        Check(Distance(out, after) < 1e-6f, "an unnoticeable gap is not blended");
    }
    {
        Reacquire r;   // two frames the runtime could not locate are no loss of tracking
        const HandPose before = At(0.0f, 0.0f, -0.3f);
        r.Step(before, true, dt);
        r.Step(HandPose{}, false, dt);
        r.Step(HandPose{}, false, dt);
        const HandPose after = At(0.03f, 0.0f, -0.3f);
        Check(Distance(r.Step(after, true, dt), after) < 1e-6f, "a short gap is not blended");
    }
    {
        Reacquire r;   // a hand never seen: nothing to blend from
        r.Step(HandPose{}, false, dt);
        const HandPose first = At(0.5f, 0.0f, 0.0f);
        Check(Distance(r.Step(first, true, dt), first) < 1e-6f, "the first pose ever is published as it is");
    }
    std::printf(g_failed ? "%d FAILED\n" : "tracking reacquire: all passed\n", g_failed);
    return g_failed ? 1 : 0;
}
