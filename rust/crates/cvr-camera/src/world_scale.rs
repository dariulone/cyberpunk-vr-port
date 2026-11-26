//! The live controls' world and eye scales as the stereo depth reads them (formerly src/Camera/CameraMath.cpp
//! GetWorldScale and the frame loop's depth inputs callback): the uniform world scale, 1 unless set above 0, and
//! whether the IPD or the stereo scale moves the eyes away from 1.

/// GetWorldScale: the live controls' world scale, or 1 when it is not above 0.
pub fn world_scale(live: f32) -> f32 {
    if live > 0.0 {
        live
    } else {
        1.0
    }
}
/// Whether the IPD scale or the stereo scale is away from 1 by more than 1e-4.
pub fn eyes_scaled(ipd_scale: f32, stereo_scale: f32) -> bool {
    (ipd_scale - 1.0).abs() > 1e-4 || (stereo_scale - 1.0).abs() > 1e-4
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scales() {
        assert_eq!((world_scale(0.8), world_scale(0.0), world_scale(-1.0)), (0.8, 1.0, 1.0));
        assert!(!eyes_scaled(1.0, 1.0));
        assert!(!eyes_scaled(1.00005, 0.99995));
        assert!(eyes_scaled(1.001, 1.0));
        assert!(eyes_scaled(1.0, 0.999));
    }
}
