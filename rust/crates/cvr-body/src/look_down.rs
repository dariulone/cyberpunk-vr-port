//! The free-look cone widening as the head looks down (formerly include/Runtimes/LookDownCone.hpp).
use crate::bend::bend_weight;
use crate::cmath::{asin, clamp, max};

/// Pitch from the head's forward vector, independent of yaw and local roll. OpenXR forward is -Z; positive means
/// looking toward the floor.
pub fn head_down_degrees(x: f32, y: f32, z: f32, w: f32) -> f32 {
    let norm = x * x + y * y + z * z + w * w;
    if !norm.is_finite() || norm < 0.5 || norm > 1.5 {
        return 0.0;
    }
    let down = clamp(2.0 * (y * z - w * x) / norm, -1.0, 1.0);
    asin(down) * 57.295779513
}

pub fn look_down_free_look_cone(down_degrees: f32, normal_degrees: f32, lowered_degrees: f32, body_bend_radians: f32) -> f32 {
    let normal = clamp(if normal_degrees.is_finite() { normal_degrees } else { 5.0 }, 0.0, 60.0);
    let lowered = clamp(if lowered_degrees.is_finite() { lowered_degrees } else { 30.0 }, 0.0, 90.0);
    let down = if down_degrees.is_finite() { down_degrees } else { 0.0 };
    // Small natural head tilts alone; the full wider cone by 30 degrees down. Never narrower than the player's cone.
    let t = clamp((down - 10.0) / 20.0, 0.0, 1.0);
    let blend = t * t * (3.0 - 2.0 * t);
    let cone = normal + (max(normal, lowered) - normal) * blend;
    cone + (max(cone, 60.0) - cone) * bend_weight(body_bend_radians)
}

pub fn body_free_look_cone(swimming: bool, swimming_degrees: f32, down_degrees: f32, normal_degrees: f32,
    lowered_degrees: f32, body_bend_radians: f32) -> f32 {
    if swimming {
        return clamp(if swimming_degrees.is_finite() { swimming_degrees } else { 5.0 }, 0.0, 90.0);
    }
    look_down_free_look_cone(down_degrees, normal_degrees, lowered_degrees, body_bend_radians)
}
