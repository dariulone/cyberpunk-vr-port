//! The ladder's hands (formerly include/Anim/LadderGripProfile.hpp, LadderFingerTuning.hpp and LadderRungPose.hpp):
//! the native side grips captured with VRIK off (2026-09-22, tools/vrik_tests/fixtures/ladder/grip-provenance.json),
//! the approved rung pose, and the per-finger adjustment channels.
use cvr_body::cmath::clamp;

pub struct Hand {
    pub rotation: [f32; 4],
    pub contact: [f32; 3],
}
pub static HANDS: [Hand; 2] = [
    Hand { rotation: [0.405598559, 0.634011341, -0.27666015, -0.597476852], contact: [0.0442100611, -0.00943239265, -0.0219573217] },
    Hand { rotation: [-0.581068528, 0.319729064, 0.622600803, -0.415332314], contact: [-0.0424212176, 0.00714786932, 0.0252632811] },
];

pub struct Finger {
    pub suffix: &'static str,
    pub group: usize,
    /// [0] left, [1] right.
    pub rotation: [[f32; 4]; 2],
}
macro_rules! finger {
    ($s:literal, $g:literal, [$($l:literal),*], [$($r:literal),*]) => {
        Finger { suffix: $s, group: $g, rotation: [[$($l),*], [$($r),*]] }
    };
}
/// Slots: 0-4 InHand*, 5-9 Hand*1 (knuckles), 10-14 Hand*2, 15-18 Hand*3 (no thumb 3).
pub static FINGERS: [Finger; 19] = [
    finger!("InHandThumb", 0, [-0.915502485, 0.284142918, 0.119032703, -0.25874547], [-0.899766667, 0.33662688, 0.138569952, -0.240625552]),
    finger!("InHandIndex", 1, [-0.788473166, 0.00834596661, 0.0241673931, -0.614537508], [-0.783752366, -0.0147292088, 0.0407753084, -0.619558435]),
    finger!("InHandMiddle", 2, [0.690334105, 0.0189856254, 0.044433702, 0.721875346], [0.691945742, 0.0210051739, 0.0370948588, 0.720689839]),
    finger!("InHandRing", 3, [0.607563889, 0.0784287427, 0.0795887021, 0.786371853], [0.615164581, 0.0975634601, 0.0628201291, 0.779812504]),
    finger!("InHandPinky", 4, [0.398402505, 0.10842663, 0.174364704, 0.893932917], [0.437877937, 0.12720456, 0.147484957, 0.87768451]),
    finger!("HandThumb1", 0, [0.0452024632, -0.0284958031, -0.00275500962, 0.998567542], [0.0298292436, -0.0289560797, -0.0202801874, 0.998929665]),
    finger!("HandIndex1", 1, [-0.161415766, 0.0894211197, -0.350713706, 0.918122383], [-0.126882754, 0.0861247122, -0.350335409, 0.923985066]),
    finger!("HandMiddle1", 2, [-0.079781116, 0.0866159711, -0.441036164, 0.889730155], [-0.0461887999, 0.0772996991, -0.430528673, 0.898073724]),
    finger!("HandRing1", 3, [0.0122638696, 0.078000615, -0.430901728, 0.898937819], [0.0311908382, 0.058128548, -0.413440681, 0.908138209]),
    finger!("HandPinky1", 4, [0.229374588, 0.0203575363, -0.406138826, 0.884321278], [0.223812305, 0.00653007559, -0.357912246, 0.906512126]),
    finger!("HandThumb2", 0, [-2.15937559e-05, -2.15249145e-05, -0.0322120304, 0.999481057], [-2.15638886e-05, -2.14573954e-05, -0.133782834, 0.991010672]),
    finger!("HandIndex2", 1, [-2.13884328e-05, -2.13717372e-05, -0.268083918, 0.963395564], [-2.67463353e-05, -2.40779853e-05, -0.309843284, 0.950787641]),
    finger!("HandMiddle2", 2, [-2.12416942e-05, -2.12376765e-05, -0.346790915, 0.937942461], [-2.67950921e-05, -2.34305908e-05, -0.390462581, 0.920618798]),
    finger!("HandRing2", 3, [-2.12657401e-05, -2.12708799e-05, -0.330996723, 0.943631903], [-2.12471545e-05, -2.12411601e-05, -0.350124397, 0.936703211]),
    finger!("HandPinky2", 4, [-2.1281707e-05, -2.12769787e-05, -0.326771766, 0.945103281], [-2.67673758e-05, -2.3889594e-05, -0.335903322, 0.941896468]),
    finger!("HandIndex3", 1, [-0.0208894539, -0.0100542315, -0.706112208, 0.707720349], [0.00598625455, 0.00325007723, -0.674811362, 0.737958825]),
    finger!("HandMiddle3", 2, [-0.00729520804, 0.013468587, -0.741022787, 0.671305151], [-0.000855259692, 0.0203047719, -0.66911833, 0.742877948]),
    finger!("HandRing3", 3, [-0.0106537845, -0.0265973799, -0.763560428, 0.645100417], [0.00511727438, -0.0241605184, -0.708614284, 0.705163725]),
    finger!("HandPinky3", 4, [0.0086164176, -0.0302944186, -0.510924369, 0.859048482], [0.0157438334, -0.027814367, -0.46068923, 0.886985866]),
];

/// The approved rung pose, baked from vrport.ini on 2026-09-22 (tools/vrik_tests/fixtures/ladder/tuned-rung.json).
pub static RUNG_CURL: [f32; 5] = [1.0; 5];
const THUMB_RUNG: [f32; 6] = [10.5, 68.8, 0.0, -31.5, 29.8, 18.1];
const FLAT: [f32; 6] = [0.0; 6];
pub static RUNG_ADJUST: [[[f32; 6]; 5]; 2] = [[THUMB_RUNG, FLAT, FLAT, FLAT, FLAT], [THUMB_RUNG, FLAT, FLAT, FLAT, FLAT]];

pub const BASE_BEND: usize = 0;
pub const MIDDLE_BEND: usize = 1;
pub const TIP_BEND: usize = 2;
pub const SPREAD: usize = 3;
pub const ROLL: usize = 4;
pub const PALM_BEND: usize = 5;

pub fn has_adjustment(finger: usize, channel: usize) -> bool {
    finger != 0 || channel != TIP_BEND
}
pub fn adjustment_limit(finger: usize, channel: usize) -> f32 {
    if channel == SPREAD {
        return 40.0;
    }
    if channel == PALM_BEND {
        return if finger == 0 { 60.0 } else { 35.0 };
    }
    if channel == MIDDLE_BEND {
        75.0
    } else {
        60.0
    }
}
pub fn clamp_adjustment(finger: usize, channel: usize, degrees: f32) -> f32 {
    let limit = adjustment_limit(finger, channel);
    clamp(if degrees.is_finite() { degrees } else { 0.0 }, -limit, limit)
}

/// Measured rig axes: flexion is -Z, spread is Y, finger roll is X. Thumb opposition/spread/roll belong to its
/// metacarpal; other digits spread and roll at the first phalanx. Angles are local offsets in degrees.
pub fn finger_adjustment_angles(suffix: &str, group: usize, settings: Option<&[f32; 6]>) -> [f32; 3] {
    let mut xyz = [0.0f32; 3];
    let Some(settings) = settings else { return xyz };
    let palm = suffix.starts_with("InHand");
    let joint = if palm { 0 } else { (suffix.as_bytes()[suffix.len() - 1] as i32) - ('0' as i32) };
    if palm {
        xyz[2] = -clamp_adjustment(group, PALM_BEND, settings[PALM_BEND]);
    } else if (1..=3).contains(&joint) {
        let channel = (joint - 1) as usize;
        xyz[2] = -clamp_adjustment(group, channel, settings[channel]);
    }
    if (group == 0 && palm) || (group != 0 && joint == 1) {
        xyz[0] = clamp_adjustment(group, ROLL, settings[ROLL]);
        xyz[1] = clamp_adjustment(group, SPREAD, settings[SPREAD]);
    }
    xyz
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn adjustments() {
        let a = finger_adjustment_angles("InHandThumb", 0, Some(&THUMB_RUNG));
        assert_eq!(a, [29.8, -31.5, -18.1]);
        let b = finger_adjustment_angles("HandThumb2", 0, Some(&THUMB_RUNG));
        assert_eq!(b, [0.0, 0.0, -68.8]);
        assert_eq!(finger_adjustment_angles("HandIndex1", 1, None), [0.0; 3]);
        assert!(!has_adjustment(0, TIP_BEND));
    }
}
