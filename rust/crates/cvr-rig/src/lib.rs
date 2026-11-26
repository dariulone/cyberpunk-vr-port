//! VRIK (stage 3 of docs/rust-migration-plan.md): the solve for the player's own bones, ported from
//! src/Anim/CharacterRig.cpp and its neighbours.
//!
//!   math, pose, skeleton, gestures, ladder, printf, data, wire, wheel   pure pieces, tested here
//!   menu_swipe                               the VR menu's summon swipe (formerly include/Natives/MenuSwipe.hpp)
//!   reflex                                   NVIDIA Reflex's options policy and timing ABI (the hook is vrik::reflex_hook)
//!   vrik (feature `engine`)                  the solve itself, on the globals it shares with the C++ half; linked
//!                                            only where those are defined (the DLL, tools/vrik_tests)
pub mod data;
pub mod gestures;
pub mod ladder;
pub mod ladder_climb;
pub mod math;
pub mod menu_swipe;
pub mod pose;
pub mod printf;
pub mod reflex;
pub mod skeleton;
pub mod wheel;
pub mod wire;
#[cfg(feature = "engine")]
pub mod vrik;

/// The unit tests' stand-in for the C++ subsystems' settings parser cvr-settings names (the DLL's is cvr-render's).
#[cfg(test)]
#[no_mangle]
extern "C" fn cvr_cpp_settings_parse_subsystem(_which: i32, _line: *const core::ffi::c_char, _s: *mut core::ffi::c_void) -> bool {
    false
}
