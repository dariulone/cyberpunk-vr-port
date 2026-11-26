//! The ranges of steering with the hands (main's include/Anim/WheelRange.hpp): the hand travel to one full lock of a
//! car's wheel, a motorcycle's bars, and the body's sideways lean on a bike. One home for the settings poll, the
//! overlay and the steering itself, so the ini path and the overlay never disagree about what a valid value is.
use crate::crt::cpp_clamp;

pub const WHEEL_FULL_LOCK_MIN: f32 = 15.0;
pub const WHEEL_FULL_LOCK_MAX: f32 = 180.0;
pub const WHEEL_FULL_LOCK_DEFAULT: f32 = 180.0;
/// A MOTORCYCLE'S BARS: full lock is reached after this many degrees of yaw of the line between the hands. Small,
/// because arms only push and pull so far: hands 50 cm apart at 40 degrees are about 16 cm forward and 16 cm back. 40
/// is what felt right in game (2026-11-12); the first estimate was 25.
pub const BIKE_BAR_LOCK_MIN: f32 = 15.0;
pub const BIKE_BAR_LOCK_MAX: f32 = 60.0;
pub const BIKE_BAR_LOCK_DEFAULT: f32 = 40.0;
/// LEANING THE BODY ON A BIKE. Share of full lock that a sideways lean of BIKE_LEAN_FULL_M adds to the arms'
/// steering; 0 is off.
pub const BIKE_BODY_LEAN_DEFAULT: f32 = 0.5;
pub const BIKE_LEAN_DEAD_M: f32 = 0.03;
pub const BIKE_LEAN_FULL_M: f32 = 0.15;

pub fn bike_body_lean_share(value: f32) -> f32 {
    if value.is_finite() && value >= 0.0 {
        // std::min(value, 1.f)
        if 1.0 < value { 1.0 } else { value }
    } else {
        BIKE_BODY_LEAN_DEFAULT
    }
}
/// Degrees of steering for a sideways lean in metres, right positive. Never more than one full lock.
pub fn bike_lean_degrees(sideways: f32, share: f32, limit: f32) -> f32 {
    if !sideways.is_finite() {
        return 0.0;
    }
    let over = sideways.abs() - BIKE_LEAN_DEAD_M;
    let amount = (if over < 0.0 { 0.0 } else { over }) / (BIKE_LEAN_FULL_M - BIKE_LEAN_DEAD_M);
    let degrees = amount * bike_body_lean_share(share) * limit;
    (if limit < degrees { limit } else { degrees }).copysign(sideways)
}
pub fn bike_bar_lock_degrees(value: f32) -> f32 {
    if value.is_finite() && value > 0.0 {
        cpp_clamp(value, BIKE_BAR_LOCK_MIN, BIKE_BAR_LOCK_MAX)
    } else {
        BIKE_BAR_LOCK_DEFAULT
    }
}
/// Physical hand travel from the grab reference to one full steering lock. This normalizes input only; cabin wheel
/// travel is authored in vehicle graphs.
pub fn wheel_full_lock_degrees(value: f32) -> f32 {
    if value.is_finite() && value > 0.0 {
        cpp_clamp(value, WHEEL_FULL_LOCK_MIN, WHEEL_FULL_LOCK_MAX)
    } else {
        WHEEL_FULL_LOCK_DEFAULT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        assert_eq!(wheel_full_lock_degrees(10.0), 15.0);
        assert_eq!(wheel_full_lock_degrees(f32::NAN), 180.0);
        assert_eq!(bike_bar_lock_degrees(0.0), 40.0);
        assert_eq!(bike_bar_lock_degrees(90.0), 60.0);
        assert_eq!(bike_body_lean_share(-1.0), 0.5);
        assert_eq!(bike_body_lean_share(3.0), 1.0);
        assert_eq!(bike_lean_degrees(0.02, 0.5, 40.0), 0.0);
        assert!((bike_lean_degrees(-0.15, 1.0, 40.0) + 40.0).abs() < 1e-5);
        assert!((bike_lean_degrees(0.09, 0.5, 40.0) - 10.0).abs() < 1e-4);
    }
}
