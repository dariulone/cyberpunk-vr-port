// The camera link and the camera math in Rust (CVR_RUST_VRIK: rust/crates/cvr-rig/src/vrik/camera_link.rs and
// camera_math.rs) in place of src/Camera/CameraLink.cpp and CameraMath.cpp: the C++ API the hooks call, forwarded.
#include "Camera/CameraLink.hpp"
#include "Camera/CameraState.hpp"
#include "Core/CoreInternal.hpp"
#include <cstddef>

static_assert(sizeof(OpenXRHeadPose)==64 && offsetof(OpenXRHeadPose,valid)==28 && offsetof(OpenXRHeadPose,originSerial)==32 &&
              offsetof(OpenXRHeadPose,bodyBend)==40 && offsetof(OpenXRHeadPose,crouchDepth)==52 &&
              offsetof(OpenXRHeadPose,physicalCrouch)==56 && offsetof(OpenXRHeadPose,afwEye)==57,
              "OpenXRHeadPose is mirrored by HeadPose in rust/crates/cvr-rig/src/vrik/camera_link.rs");
static_assert(sizeof(cvr::camera::LocatedCameraFrame)==52 && sizeof(cvr::camera::ViewFrame)==56 &&
              sizeof(cvr::camera::BarrelFrame)==28 && sizeof(cvr::roomscale::Vec2)==8 &&
              sizeof(cvr::camera::AnchorRecipe)==44,
              "the camera link's packets are mirrored in rust/crates/cvr-rig/src/vrik/camera_link.rs");
