//! The player's body (stage 3 of docs/rust-migration-plan.md): how the body turns under the head and hands, how it
//! bends, and the arithmetic they share. Ported from the C++ headers they are named after; each type with state has
//! the C++ class's field order (`#[repr(C)]`) so the C++ half can hold it and call it through cvr_rust.lib while its
//! callers are still C++, and tools/body_rotation_tests runs every case against both, plus a bit-for-bit trace.
//!
//!   hud         the panels' yaw-follow and the display timeline (formerly include/Runtimes/HudFollow.hpp)
//!   vr_overlay  the settings panel in VR: its placement, the pointer, the L3 + R3 hold (formerly
//!               src/Overlay/VrOverlay.cpp, with include/Overlay/VrInteraction.hpp's pose policy)
pub mod aim_dwell;
pub mod optic_hinge;
pub mod hand_kick;
pub mod recoil_spring;
pub mod bend;
pub mod cmath;
pub mod crouch;
pub mod follow_zone;
pub mod head_led;
/// The visible body's dead zones (main's BodyDeadZones.hpp, off by default).
pub mod dead_zones;
pub mod hud;
pub mod hybrid_yaw;
pub mod look_down;
pub mod native_heading;
pub mod policy;
pub mod prediction;
pub mod reacquire;
pub mod roomscale;
pub mod swim;
pub mod tracked_yaw;
pub mod tracking;
pub mod vr_overlay;
pub mod vec2;
pub mod vehicle_minimap;
pub mod xr;
pub mod yaw_follower;
pub mod fbt_cross;

#[cfg(test)]
mod layout {
    use core::mem::size_of;
    // The C++ sizes, asserted on the C++ side too (include/Rust/CvrRust.h).
    #[test]
    fn sizes_match_the_cpp_classes() {
        assert_eq!(size_of::<crate::follow_zone::BodyFollowZone>(), 16);
        assert_eq!(size_of::<crate::yaw_follower::BodyYawFollower>(), 40);
        assert_eq!(size_of::<crate::prediction::BodyYawPrediction>(), 84);
        assert_eq!(size_of::<crate::tracked_yaw::TrackingFrame>(), 112);
        assert_eq!(size_of::<crate::tracked_yaw::YawEstimate>(), 12);
        assert_eq!(size_of::<crate::tracked_yaw::TrackedBodyYaw>(), 488);
        assert_eq!(size_of::<crate::hybrid_yaw::HybridInput>(), 64);
        assert_eq!(size_of::<crate::hybrid_yaw::HybridCommand>(), 32);
        assert_eq!(size_of::<crate::hybrid_yaw::HybridBodyYaw>(), 64);
        assert_eq!(size_of::<crate::bend::BendTracker>(), 128);
        assert_eq!(size_of::<crate::roomscale::BodyTranslationFollow>(), 28);
        assert_eq!(size_of::<crate::roomscale::PhysicalWalkGate>(), 16);
        assert_eq!(size_of::<crate::roomscale::Sample>(), 72);
        assert_eq!(size_of::<crate::roomscale::Step>(), 56);
        assert_eq!(size_of::<crate::roomscale::LeadLimit>(), 12);
        // 120 with main's 0.2.3 eye lever (tick_eye); no C++ of the Rust build holds one
        assert_eq!(size_of::<crate::roomscale::Movement>(), 120);
        assert_eq!(size_of::<crate::crouch::PhysicalCrouch>(), 40);
        assert_eq!(size_of::<crate::native_heading::NativeBodyHeading>(), 32);
        assert_eq!(size_of::<crate::reacquire::HandPose>(), 28);
        assert_eq!(size_of::<crate::reacquire::Reacquire>(), 72);
    }
}
