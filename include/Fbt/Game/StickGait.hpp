#pragma once
#include "Fbt/FbtMath.hpp"
#include <algorithm>
#include <cmath>

namespace cvr::fbt::game {

// World-space offsets for both ankle goals. Tracker positions are room-space metres.
// Advance once per tracking sample; both eyes/pose passes reuse the resulting offsets.
struct StickGait {
    using Vec3 = wrpuppets::fbt::Vec3;
    Vec3 offset[2]{};

    void Advance(float dt, Vec3 player, const Vec3 feet[2], bool movingStick, bool footRaised) {
        const Vec3 travel = player - lastPlayer;
        if (!ready || !(dt > 0.f && dt < .2f) ||
            std::hypot(travel.x, travel.y) > std::max(.65f, 15.f * dt)) {
            *this = {};
            ready = true;
            lastPlayer = player;
            for (int i = 0; i < 2; ++i) lastFoot[i] = feet[i];
            return;
        }
        lastPlayer = player;
        const float smooth = 1.f - std::exp(-dt / .09f);
        velocity = velocity + (Vec3{travel.x / dt, travel.y / dt, 0} - velocity) * smooth;
        float fastestFoot = 0;
        for (int i = 0; i < 2; ++i) {
            fastestFoot = std::max(fastestFoot, Length(feet[i] - lastFoot[i]) / dt);
            lastFoot[i] = feet[i];
        }
        footSpeed += (fastestFoot - footSpeed) * smooth;
        if (footSpeed > .32f || footRaised) physicalHold = .20f;
        else physicalHold = std::max(0.f, physicalHold - dt);

        const float speed = Length(velocity);
        const bool walking = movingStick && speed > .13f && physicalHold == 0.f;
        const float stride = std::clamp(speed * .14f, .025f, .31f);
        const float lift = std::clamp(speed * .035f, .025f, .14f);
        // Cadence in full left/right cycles per second, from a slow walk to a sprint.
        if (walking) phase = std::fmod(phase + dt * std::clamp(.65f + speed * .21f, .65f, 1.85f) * 6.2831853f, 6.2831853f);
        const Vec3 direction = walking ? velocity * (1.f / speed) : Vec3{};
        // Relax back onto the tracked pose when the stick stops or a physical foot moves.
        const float blend = 1.f - std::exp(-dt / (walking ? .033f : .055f));
        for (int i = 0; i < 2; ++i) {
            Vec3 target{};
            if (walking) {
                const float side = i == 0 ? 1.f : -1.f;
                target = direction * (-side * std::cos(phase) * stride);
                const float swing = std::max(0.f, side * std::sin(phase));
                target.z = swing * swing * lift;
            }
            offset[i] = offset[i] + (target - offset[i]) * blend;
        }
    }

private:
    bool ready{};
    Vec3 lastPlayer{}, lastFoot[2]{}, velocity{};
    float footSpeed{}, physicalHold{}, phase{};
};

} // namespace cvr::fbt::game
