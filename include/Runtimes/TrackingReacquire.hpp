#pragma once
#include <cmath>

// A HAND COMING BACK INTO TRACKING GLIDES TO WHERE IT IS. While a controller has no pose (out of the cameras' view,
// occluded, asleep) the body holds the hand where it last was; when the pose comes back it is often somewhere
// else, and publishing it at once teleports the hand -- and the weapon in it -- across the view. BONELAB's answer
// (SLZ.Marrow OpenControllerRig.ProcessTrackedBlend / BlendOffset, read from its IL2CPP build): on the first pose
// after a gap, keep the difference between the pose last published and the new one as an offset, publish the new
// pose plus that offset scaled by a weight, and bring the weight from 1 to 0 with a critically damped spring
// (Unity's SmoothDamp). The hand follows the controller from the first frame and the jump is spread over a few
// tenths of a second. In the frame the caller publishes in (the port's hands are head-local).
namespace cvr::tracking {
struct HandPose {
    float p[3]{};
    float q[4]{0, 0, 0, 1};   // x, y, z, w
};

// Unity's Mathf.SmoothDamp (Game Programming Gems 4, 1.10): a critically damped approach to `target`.
inline float SmoothDamp(float current, float target, float& velocity, float smoothTime, float dt) {
    smoothTime = smoothTime > 1e-4f ? smoothTime : 1e-4f;
    const float omega = 2.0f / smoothTime;
    const float x = omega * dt;
    const float decay = 1.0f / (1.0f + x + 0.48f * x * x + 0.235f * x * x * x);
    const float change = current - target;
    const float temp = (velocity + omega * change) * dt;
    velocity = (velocity - omega * temp) * decay;
    float out = target + (change + temp) * decay;
    if ((target - current > 0.0f) == (out > target)) {
        out = target;
        velocity = dt > 0.0f ? (out - target) / dt : 0.0f;
    }
    return out;
}

class Reacquire {
public:
    // The time the jump is spread over: 95% of it gone in about three of these (s).
    static constexpr float kSmoothTime = 0.12f;
    // A gap that comes back closer than this (m, rad) is published as it is.
    static constexpr float kNoticeable = 0.002f, kNoticeableAngle = 0.01f;
    // Nor a gap shorter than this (s): a frame or two the runtime could not locate is no loss of tracking, and a hand
    // swung fast through it would lag behind its controller.
    static constexpr float kMinGap = 0.1f;

    void Reset() { *this = {}; }

    // This frame's pose (`valid` false: none); what to publish. Without a pose nothing is published and the last
    // published one is kept to blend from.
    HandPose Step(const HandPose& tracked, bool valid, float dt) {
        if (!valid) {
            if (have_ && !lost_) gap_ = 0.0f;
            lost_ = have_;
            gap_ += dt;
            return last_;
        }
        if (lost_ && gap_ < kMinGap) lost_ = false;
        if (lost_) {
            lost_ = false;
            for (int i = 0; i < 3; ++i) offset_.p[i] = last_.p[i] - tracked.p[i];
            // The rotation that takes the new orientation to the last one: last = offset * tracked.
            Multiply(last_.q, Conjugate(tracked.q), offset_.q);
            if (offset_.q[3] < 0.0f)
                for (float& c : offset_.q) c = -c;
            const float distance = std::sqrt(offset_.p[0] * offset_.p[0] + offset_.p[1] * offset_.p[1] +
                                             offset_.p[2] * offset_.p[2]);
            const float angle = 2.0f * std::acos(std::fmin(1.0f, offset_.q[3]));
            blend_ = distance > kNoticeable || angle > kNoticeableAngle ? 1.0f : 0.0f;
            velocity_ = 0.0f;
        }
        HandPose out = tracked;
        if (blend_ > 0.0f) {
            blend_ = dt > 0.0f ? SmoothDamp(blend_, 0.0f, velocity_, kSmoothTime, dt) : blend_;
            if (blend_ <= 0.01f) blend_ = 0.0f;
            for (int i = 0; i < 3; ++i) out.p[i] = tracked.p[i] + offset_.p[i] * blend_;
            float part[4];
            Nlerp(offset_.q, blend_, part);
            Multiply(part, tracked.q, out.q);
        }
        last_ = out;
        have_ = true;
        return out;
    }

    float Blend() const { return blend_; }

private:
    struct Quat { float q[4]; };
    static Quat Conjugate(const float* q) { return {{-q[0], -q[1], -q[2], q[3]}}; }
    static void Multiply(const float* a, const Quat& b, float* out) { Multiply(a, b.q, out); }
    static void Multiply(const float* a, const float* b, float* out) {
        const float r[4] = {a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
                            a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
                            a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
                            a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2]};
        for (int i = 0; i < 4; ++i) out[i] = r[i];
    }
    // From the identity towards `q` by `t`, normalised (the offsets are small: no slerp needed).
    static void Nlerp(const float* q, float t, float* out) {
        const float r[4] = {q[0] * t, q[1] * t, q[2] * t, 1.0f + (q[3] - 1.0f) * t};
        const float n = std::sqrt(r[0] * r[0] + r[1] * r[1] + r[2] * r[2] + r[3] * r[3]);
        for (int i = 0; i < 4; ++i) out[i] = n > 1e-6f ? r[i] / n : (i == 3 ? 1.0f : 0.0f);
    }

    HandPose last_, offset_;
    bool have_ = false, lost_ = false;
    float blend_ = 0.0f, velocity_ = 0.0f, gap_ = 0.0f;
};
}  // namespace cvr::tracking
