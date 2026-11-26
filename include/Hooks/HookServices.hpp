#pragma once
// What the Rust hooks (rust/crates/cvr-rig/src/vrik) read from the C++ half, in C form (src/Hooks/HookServices.cpp).
#include <cstdint>

extern "C" {
// The g_liveControls fields the roomscale and body-turn hooks read (roomscale_move.rs BodyControls, field for field).
struct CvrBodyControls {
    int32_t roomscaleMovement, cutsceneSuspendTier, bodyRotationMode;
    float bodyMoveRadius, bodyFreeLookDeg, bodyFreeLookDownDeg, bodyFreeLookSwimDeg;
};
static_assert(sizeof(CvrBodyControls) == 28, "mirrored by BodyControls in rust/crates/cvr-rig/src/vrik/roomscale_move.rs");
void cvr_cpp_body_controls(CvrBodyControls* out);
}