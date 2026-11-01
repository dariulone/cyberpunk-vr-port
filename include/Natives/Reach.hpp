#pragma once
#include <cmath>

// THE HAND'S REACH IN THE GAME (mods/cet/CyberpunkVRPort_Reach modules/world.lua).
//
// The game picks what the player interacts with by where the player looks: the targeting system's look-at query,
// a ray cast every frame from the camera's transform (TargetingUser_UpdateLookAtRay, Hooks/StoryAttention.cpp). Its
// hits are the look-at object, and through the interaction system's look-at they choose the loot prompt, the choice
// hub and the highlight. While a free hand reaches for a thing the script gives that thing's point every frame
// (VRReachAim), and the query's ray is turned onto it from where it starts -- the eyes; its origin never moves -- so
// what the game offers is what the hand is on. The camera itself, and with it the view and the weapons' targeting,
// is not touched. The grip that takes the thing presses the game's own interaction button for a moment
// (VRReachUse): the game then does what its button does there, its own choice, sound and animation.
//
// VRReachAim(point) -> Bool: point.W 1 = look at (X, Y, Z) of the world this frame; 0 = let go. A point not given
//   again for kStaleMs is let go by itself.
// VRReachUse(seconds) -> Bool: the interaction button (the pad's X) held that long, 0.05 .. 2 s; 0 lets it go.
// VRHandHolds(side) -> Bool: the port has something in that hand (0 left, 1 right, the controllers as they are) --
//   HandHolds below. What the game holds in the hand's weapon slot is the script's to read.
namespace cvr::reach {
inline constexpr unsigned long long kStaleMs = 150;

// The point the look-at ray is turned onto, in the world, while the script gives one.
bool AimPoint(float point[3]);
// The interaction button the hand holds down now.
bool UseHeld();
// A hand that cannot take anything now: one the port has put something in (Anim/HandGestures.hpp PortHolds) or the
// support hand on the gun.
bool HandHolds(int side);

// Turn a transform (position X Y Z W, rotation X Y Z W; forward = +Y) onto a point of the world by the shortest arc:
// the position stays and the roll goes along. False, and the transform untouched, for a point within 5 cm or past
// 50 m, or a rotation that is not one.
inline bool TurnToward(float* transform, const float* point) {
    for (int i = 0; i < 8; ++i)
        if (!std::isfinite(transform[i])) return false;
    for (int i = 0; i < 3; ++i)
        if (!std::isfinite(point[i])) return false;
    const float n = transform[4] * transform[4] + transform[5] * transform[5] + transform[6] * transform[6] +
                    transform[7] * transform[7];
    if (n < 0.5f || n > 1.5f) return false;
    const float inv = 1.0f / std::sqrt(n);
    const float x = transform[4] * inv, y = transform[5] * inv, z = transform[6] * inv, w = transform[7] * inv;
    const float d[]{point[0] - transform[0], point[1] - transform[1], point[2] - transform[2]};
    const float length = std::sqrt(d[0] * d[0] + d[1] * d[1] + d[2] * d[2]);
    if (length < 0.05f || length > 50.0f) return false;
    const float to[]{d[0] / length, d[1] / length, d[2] / length};
    const float from[]{2 * (x * y - z * w), 1 - 2 * (x * x + z * z), 2 * (y * z + x * w)};
    const float dot = from[0] * to[0] + from[1] * to[1] + from[2] * to[2];
    // The arc from -> to; straight behind, half a turn about the rotation's own up.
    float a[4];
    if (dot < -0.9999f) {
        a[0] = 2 * (x * z + y * w);
        a[1] = 2 * (y * z - x * w);
        a[2] = 1 - 2 * (x * x + y * y);
        a[3] = 0.0f;
    } else {
        a[0] = from[1] * to[2] - from[2] * to[1];
        a[1] = from[2] * to[0] - from[0] * to[2];
        a[2] = from[0] * to[1] - from[1] * to[0];
        a[3] = 1.0f + dot;
    }
    const float an = std::sqrt(a[0] * a[0] + a[1] * a[1] + a[2] * a[2] + a[3] * a[3]);
    for (float& v : a) v /= an;
    // The arc after the rotation, in the world: a * q.
    transform[4] = a[3] * x + a[0] * w + a[1] * z - a[2] * y;
    transform[5] = a[3] * y - a[0] * z + a[1] * w + a[2] * x;
    transform[6] = a[3] * z + a[0] * y - a[1] * x + a[2] * w;
    transform[7] = a[3] * w - a[0] * x - a[1] * y - a[2] * z;
    return true;
}
}  // namespace cvr::reach
