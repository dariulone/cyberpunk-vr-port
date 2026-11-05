#pragma once
#include <cstdint>

// THE INTERACTION PROMPT AT THE DEPTH OF ITS OBJECT.
//
// The prompt ("Open", "Pick up", "Talk") is drawn by the game at a fixed spot on screen, beside the crosshair,
// for the object the interaction system picked from the centre of the view. The port shows it on its own
// panel, a quad at a fixed distance (xr_interaction_distance). Against an object at another distance the
// eyes cannot converge on both, so the prompt floats in front of or behind what it names.
//
// The Stereo Lua mod publishes the distance to that object, measured along the camera's forward axis,
// while a prompt is on screen (SetVRInteractionDepth). The panel is moved to it and keeps its angular size
// (its size is computed from the distance), so every pixel slides along its own ray from the head: the
// prompt stays beside the object and gains the object's depth. One quad seen by both eyes
// (xr_hud_stereo_depth=1) then gives each eye the right parallax by geometry. xr_interaction_anchor=0
// restores the fixed distance.
namespace cvr::interaction {

// Distance in metres, or 0 when nothing fresh was published (no prompt, no target, the Lua mod stopped).
float TargetDepth(uint64_t nowMs);

// One smoothing step of the panel distance. Small changes glide (gaze crossing an edge, walking up to the
// object); a change by more than half is a new target and snaps, so the prompt never sweeps through space.
constexpr float StepDistance(float current, float target, float dt) {
    if (current <= 0.0f || target <= 0.0f) return target;
    const float ratio = target > current ? target / current : current / target;
    if (ratio > 1.5f) return target;
    float k = dt * 10.0f;
    if (k > 1.0f) k = 1.0f;
    if (k < 0.0f) k = 0.0f;
    return current + (target - current) * k;
}

namespace detail {
constexpr bool Near(float a, float b) { return a - b < 1e-4f && b - a < 1e-4f; }
static_assert(Near(StepDistance(0.0f, 1.2f, 0.011f), 1.2f), "first target is taken as is");
static_assert(Near(StepDistance(2.0f, 0.8f, 0.011f), 0.8f), "a new, much nearer target snaps");
static_assert(Near(StepDistance(0.8f, 3.0f, 0.011f), 3.0f), "a new, much farther target snaps");
static_assert(StepDistance(1.0f, 1.2f, 0.011f) > 1.0f && StepDistance(1.0f, 1.2f, 0.011f) < 1.2f, "small changes glide");
static_assert(Near(StepDistance(1.0f, 1.2f, 1.0f), 1.2f), "a long frame arrives, it never overshoots");
}
}
