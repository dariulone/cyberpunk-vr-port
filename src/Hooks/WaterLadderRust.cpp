// The ladder and the swim strokes in Rust (CVR_RUST_VRIK: rust/crates/cvr-rig/src/vrik/ladder_input.rs and
// swimming_input.rs) in place of src/Hooks/LadderInput.cpp and SwimmingInput.cpp: cvr::ladder::* and cvr::swimming::*,
// forwarded.
#include "Hooks/LadderInput.hpp"
#include "Hooks/SwimmingInput.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include <cstddef>

extern "C" {
#if !CVR_RUST_VRIK   // unused by the Rust build
bool cvr_ladder_publish_top(uintptr_t player,const cvr::ladder::Vec* ladderPosition,const cvr::ladder::Vec* origin,
                            const cvr::ladder::Vec* right,const cvr::ladder::Vec* normal,const cvr::ladder::Vec* up);
#endif
void cvr_swimming_update_input(const VRControllerState* controllers,bool manualBack,cvr::swimming::Input* out);
}
static_assert(sizeof(cvr::ladder::Vec)==12 && sizeof(cvr::ladder::Geometry::Segment)==24 && sizeof(cvr::ladder::Geometry)==336 &&
              offsetof(cvr::ladder::Geometry,height)==48 && offsetof(cvr::ladder::Geometry,topRails)==72 &&
              offsetof(cvr::ladder::Geometry,topRailCount)==312 && offsetof(cvr::ladder::Geometry,identity)==320 &&
              offsetof(cvr::ladder::Geometry,valid)==328,
              "cvr::ladder::Geometry is mirrored in rust/crates/cvr-rig/src/ladder_climb.rs");

#if !CVR_RUST_VRIK
namespace cvr::ladder {
#endif
#if !CVR_RUST_VRIK
}
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // src/Hooks/XInput.cpp's (the C++ build's); the Rust XInput merge calls the Rust itself
namespace cvr::swimming {
Input UpdateInput(const VRControllerState& controllers,bool manualBack) { Input out{};cvr_swimming_update_input(&controllers,manualBack,&out);return out; }
}
#endif