#pragma once
#include <cstdint>

// THE HUD NEVER SITS INSIDE THE WORLD.
//
// The HUD panels (the main HUD and the interaction prompt) are quads at a fixed distance in front of the
// player. In open space that reads right: the HUD floats in the empty volume between the eyes and the
// scene. Walk up to a door or a wall and the scene comes nearer than the panel; the panel is then drawn
// "inside" the door, at a depth behind the surface the eyes are converged on, which is what makes it look
// wrong. The rule here is the one the player would expect: the panel stays where it is configured, unless
// something is between the eyes and it -- then it comes in, just in front of that.
//
// The Stereo Lua mod casts a small fan of rays across the panels' area and publishes the nearest depth,
// along the camera's forward axis (SetVRHudClearance). Each panel is placed at
//     min(configured distance, clearance - margin), never nearer than kMinDistance,
// keeping its angular size (its size is computed from the distance), so only its depth changes.
// Moving in is fast, so it never lingers inside a surface; moving back out is slow, so it does not pump
// while the gaze sweeps past a door frame. xr_hud_clearance=0 restores the fixed distance.
namespace cvr::hud::clearance {

constexpr float kMargin      = 0.15f;   // in front of the nearest surface
constexpr float kMinDistance = 0.35f;   // nearer than this is uncomfortable to read
constexpr float kFree        = 99.0f;   // published when no ray hit anything: open space

// Nearest depth in metres; kFree in open space; 0 when nothing fresh was published (the Lua mod stopped).
float Published(uint64_t nowMs);

// HOW MUCH A PANEL GROWS WHEN IT COMES IN. Kept at a constant angular size, a panel that comes nearer is
// read as SHRINKING: the eyes see it nearer, the size on the retina does not change, and size constancy
// (Emmert's law) turns that into "smaller". A fixed world size would be the opposite extreme and gets huge
// up close. `grow` is the compromise exponent (xr_hud_clearance_grow): the angular size is multiplied by
// (configured / distance)^grow -- 0 = constant angular size, 1 = fixed world size, 0.5 by default
// (half the distance -> about 1.41 x larger).
float GrowFactor(float configured, float distance, float grow);

// Where a panel configured at `configured` metres may sit, given the published clearance.
constexpr float Limit(float configured, float published) {
    if (published <= 0.0f) return configured;                 // no data: as configured
    float limit = published - kMargin;
    if (limit > configured) limit = configured;
    if (limit < kMinDistance) limit = kMinDistance;
    return limit;
}

// HANDS IN FRONT OF THE HUD (xr_hud_clearance_hands). The HUD is a compositor layer drawn over the game's
// picture, so V's hands can never cover it. Instead, while a tracked hand is inside a panel's field of view and
// nearer than the panel, the panel scales up around its centre (xr_hud_clearance_hands_scale, 1..3): the elements,
// which sit near its edges, move outward, toward or past the edge of the headset's view, and the hands are seen.
// Same depth; it scales back when the hand leaves. At HUD FOV 65 an edge at ~33 deg goes to ~52 deg at 2x (the
// edge of a Vive Pro's view, a small head turn away) and ~58 deg at 3x (out of view). A hand is a sphere of
// kHandRadius around the grip.
constexpr float kHandRadius   = 0.08f;
constexpr float kHandOutScale = 2.0f;   // default

// Nearest point of a hand at head-relative x (right), y (up), z (OpenXR: forward is -z), when it is inside a
// panel that spans +-tanH / +-tanV around the view axis; kFree when it is not.
constexpr float HandFront(float x, float y, float z, float tanH, float tanV) {
    const float depth = -z;
    if (depth <= kHandRadius) return kFree;
    const float ax = x < 0 ? -x : x, ay = y < 0 ? -y : y;
    if (ax - kHandRadius > tanH * depth || ay - kHandRadius > tanV * depth) return kFree;
    return depth - kHandRadius;
}
// One step of the out-of-view scale: out fast (a hand coming up must not wait), back slower (no pumping while a
// hand hovers at the panel's edge).
constexpr float OutStep(float current, bool handIn, float dt, float outScale = kHandOutScale) {
    const float target = handIn ? outScale : 1.0f;
    float k = (handIn ? 12.0f : 4.0f) * dt;
    if (k > 1.0f) k = 1.0f;
    if (k < 0.0f) k = 0.0f;
    return current + (target - current) * k;
}

// One smoothing step: fast in, slow out; a large step in snaps, so a panel is never left inside a wall.
constexpr float Step(float current, float target, float dt) {
    if (current <= 0.0f) return target;
    if (target < current * 0.6f) return target;
    float k = (target < current ? 25.0f : 2.5f) * dt;
    if (k > 1.0f) k = 1.0f;
    if (k < 0.0f) k = 0.0f;
    return current + (target - current) * k;
}

namespace detail {
constexpr bool Near(float a, float b) { return a - b < 1e-4f && b - a < 1e-4f; }
static_assert(Near(Limit(1.5f, kFree), 1.5f), "open space: as configured");
static_assert(HandFront(0.0f, 0.0f, -0.40f, 0.6f, 0.6f) < 0.33f && HandFront(0.0f, 0.0f, -0.40f, 0.6f, 0.6f) > 0.31f,
              "a hand 40 cm straight ahead: its front at 32 cm");
static_assert(HandFront(0.0f, -0.60f, -0.40f, 0.6f, 0.6f) >= kFree, "a hand low at the side of the view: not in the panel");
static_assert(HandFront(0.0f, 0.0f, 0.10f, 0.6f, 0.6f) >= kFree, "a hand behind the eyes: ignored");
static_assert(OutStep(1.0f, true, 1.0f) == kHandOutScale && OutStep(kHandOutScale, false, 1.0f) == 1.0f,
              "a long step reaches the target both ways");
static_assert(OutStep(1.0f, true, 0.016f) > OutStep(kHandOutScale, false, 0.016f) - kHandOutScale + 1.0f,
              "out is faster than back");
static_assert(Near(Limit(1.5f, 0.0f), 1.5f), "no data: as configured");
static_assert(Near(Limit(1.5f, 1.0f), 0.85f), "a door at 1 m: just in front of it");
static_assert(Near(Limit(1.5f, 0.3f), kMinDistance), "never nearer than the reading minimum");
static_assert(Near(Limit(2.02f, 5.0f), 2.02f), "never pushed farther than configured");
static_assert(Near(Step(1.5f, 0.5f, 0.011f), 0.5f), "a big step in snaps");
static_assert(Step(1.5f, 1.2f, 0.011f) < 1.5f && Step(1.5f, 1.2f, 0.011f) > 1.2f, "small steps in are quick but smooth");
static_assert(Step(1.0f, 1.5f, 0.011f) - 1.0f < 0.05f, "moving out is slow");
}
}
