//! THE WRIST'S TURN PER WEAPON (the overlay's "VR weapon grip" window, cvr-ffi mechanics::weapon_wrist): pitch, yaw and
//! roll in the wrist calibration's convention (hand-local degrees, XYZ, as SetVRHandOffset builds the calibration's
//! wrist), applied after the calibration's wrist to the hand that holds the drawn weapon -- the weapon turns with it.
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};

type Q = [f32; 4];
static DELTA: [AtomicU32; 4] = [AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0x3F80_0000)];
static ON: AtomicBool = AtomicBool::new(false);

/// SetVRHandOffset's quaternion of (pitch, yaw, roll) degrees.
pub fn quat_of(pitch: f32, yaw: f32, roll: f32) -> Q {
    let d2r = 0.017_453_292_52f32 * 0.5;
    let (cp, sp) = ((pitch * d2r).cos(), (pitch * d2r).sin());
    let (cy, sy) = ((yaw * d2r).cos(), (yaw * d2r).sin());
    let (cr, sr) = ((roll * d2r).cos(), (roll * d2r).sin());
    [
        sp * cy * cr + cp * sy * sr,
        cp * sy * cr - sp * cy * sr,
        cp * cy * sr + sp * sy * cr,
        cp * cy * cr - sp * sy * sr,
    ]
}
fn qmul(a: &Q, b: &Q) -> Q {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

/// The drawn weapon's turn (degrees; all zero for none).
pub fn set_degrees(pitch: f32, yaw: f32, roll: f32) {
    let ok = pitch.is_finite() && yaw.is_finite() && roll.is_finite();
    let on = ok && (pitch != 0.0 || yaw != 0.0 || roll != 0.0);
    let q = if on { quat_of(pitch, yaw, roll) } else { [0.0, 0.0, 0.0, 1.0] };
    for (slot, v) in DELTA.iter().zip(q) {
        slot.store(v.to_bits(), Relaxed);
    }
    ON.store(on, Relaxed);
}

/// The calibration's wrist of the hand that holds the weapon, with the weapon's turn after it.
pub fn apply(wrist: Q) -> Q {
    if !ON.load(Relaxed) {
        return wrist;
    }
    let d: Q = core::array::from_fn(|i| f32::from_bits(DELTA[i].load(Relaxed)));
    let mut q = qmul(&wrist, &d);
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if n > 1e-6 {
        for v in &mut q {
            *v /= n;
        }
        q
    } else {
        wrist
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn no_turn_leaves_the_wrist() {
        set_degrees(0.0, 0.0, 0.0);
        let w = quat_of(10.0, -90.0, -21.0);
        assert_eq!(apply(w), w);
        // a turn about one axis composes after the wrist
        set_degrees(0.0, 0.0, 10.0);
        let got = apply(quat_of(0.0, 0.0, 5.0));
        let want = quat_of(0.0, 0.0, 15.0);
        for i in 0..4 {
            assert!((got[i] - want[i]).abs() < 1e-5, "{got:?} {want:?}");
        }
        set_degrees(0.0, 0.0, 0.0);
    }
}
