//! Finger gestures from the controllers (formerly include/Anim/HandGestures.hpp's evaluation): how far each digit
//! (thumb .. little) closes toward the native fist and straightens, from grip, trigger and the thumb-up button; the
//! monowire's own fist; tracked gloves.
use cvr_body::cmath::clamp;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Targets {
    pub closed: [f32; 5],
    pub straight: [f32; 5],
}

pub fn evaluate(grip: f32, trigger: f32, thumbs_up: bool, wire: bool, deployment: f32) -> Targets {
    let grip = clamp(grip, 0.0, 1.0);
    let trigger = clamp(trigger, 0.0, 1.0);
    let mut r = Targets::default();
    if wire {
        let mut x = if grip > 0.5 { clamp((deployment - 0.35) / 0.30, 0.0, 1.0) } else { 0.0 };
        x = x * x * (3.0 - 2.0 * x);
        r.closed = [x; 5];
    } else {
        r.closed[0] = grip * trigger;
        r.closed[1] = trigger;
        r.straight[1] = grip * (1.0 - trigger);
        r.closed[2] = grip;
        r.closed[3] = grip;
        r.closed[4] = grip;
        if thumbs_up && grip > 0.5 {
            r.closed[0] = 0.0;
            r.straight[0] = grip;
            r.closed[1] = grip;
            r.straight[1] = 0.0;
        }
    }
    r
}

/// Glove curl reads about 0.1 open and 1 in a fist: TRACKED_OPEN..TRACKED_CLOSED stretched to 0..1.
pub const TRACKED_OPEN: f32 = 0.15;
pub const TRACKED_CLOSED: f32 = 0.90;

pub fn from_tracking(curl: &[f32; 5]) -> Targets {
    let mut r = Targets::default();
    for d in 0..5 {
        let x = (curl[d] - TRACKED_OPEN) / (TRACKED_CLOSED - TRACKED_OPEN);
        r.closed[d] = if x < 0.0 { 0.0 } else if x > 1.0 { 1.0 } else { x };
        r.straight[d] = 1.0;
    }
    r
}

pub type CalibrationWeights = [f32; 4];

pub fn calibration_targets(grip: f32, trigger: f32, thumbs_up: bool, wire: bool) -> CalibrationWeights {
    let grip = clamp(grip, 0.0, 1.0);
    let trigger = clamp(trigger, 0.0, 1.0);
    if wire {
        return [0.0; 4];
    }
    if thumbs_up && grip > 0.5 {
        return [0.0, 0.0, grip, 0.0];
    }
    [grip * (1.0 - trigger), grip * trigger, 0.0, (1.0 - grip) * trigger]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tracked_digits_are_their_own() {
        let open = from_tracking(&[0.10, 0.12, 0.10, 0.14, 0.18]);
        assert!(open.closed[2] == 0.0 && open.straight[2] == 1.0);
        let fist = from_tracking(&[0.95, 1.0, 0.96, 1.0, 1.0]);
        assert!(fist.closed[0] == 1.0 && fist.closed[4] == 1.0);
        let point = from_tracking(&[0.90, 0.15, 1.0, 1.0, 1.0]);
        assert!(point.closed[1] == 0.0 && point.closed[2] == 1.0 && point.closed[0] > 0.98);
        let t = evaluate(1.0, 0.0, true, false, 0.0);
        assert_eq!((t.closed[0], t.straight[0], t.closed[1]), (0.0, 1.0, 1.0));
    }
}
