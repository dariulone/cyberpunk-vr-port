//! Bone names and the neutral posture (formerly include/Anim/SkeletonPolicy.hpp and UpperBodyPosture.hpp). Names
//! compare ASCII-case-insensitively, as the rig's own resource names vary in case.
use cvr_body::cmath::clamp;

pub fn equal_bone_name(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.to_ascii_lowercase() == y.to_ascii_lowercase())
}

pub fn starts_bone_name(name: &[u8], prefix: &[u8]) -> bool {
    name.len() >= prefix.len() && equal_bone_name(&name[..prefix.len()], prefix)
}

pub fn is_primary_spine(name: &[u8]) -> bool {
    starts_bone_name(name, b"Spine") && name[5..].iter().all(u8::is_ascii_digit)
}

pub fn is_rig_control(name: &[u8]) -> bool {
    if starts_bone_name(name, b"Torso_") || starts_bone_name(name, b"shadow_") {
        return true;
    }
    [b"_setup".as_slice(), b"_GRP".as_slice()]
        .iter()
        .any(|suffix| name.len() >= suffix.len() && equal_bone_name(&name[name.len() - suffix.len()..], suffix))
}

pub fn forearm_twist_slot(name: &[u8], left: bool) -> Option<usize> {
    const WRIST_L: [&[u8]; 3] = [b"l_Wrist_0_JNT", b"l_Wrist_1_JNT", b"l_Wrist_2_JNT"];
    const WRIST_R: [&[u8]; 3] = [b"r_Wrist_0_JNT", b"r_Wrist_1_JNT", b"r_Wrist_2_JNT"];
    const TWIST_L: [&[u8]; 3] = [b"l_forearmTwist01_JNT", b"l_forearmTwist02_JNT", b"l_forearmTwist03_JNT"];
    const TWIST_R: [&[u8]; 3] = [b"r_forearmTwist01_JNT", b"r_forearmTwist02_JNT", b"r_forearmTwist03_JNT"];
    (0..3).find(|&i| {
        equal_bone_name(name, if left { WRIST_L[i] } else { WRIST_R[i] })
            || equal_bone_name(name, if left { TWIST_L[i] } else { TWIST_R[i] })
    })
}

/// The same posture for camera calibration and the live solve: the rounding in the upper thoracic chain, the pelvis
/// upright (12 degrees at the top).
pub fn spine_flex(index: i32, count: i32) -> f32 {
    let t = if count > 1 { clamp(index as f32 / (count - 1) as f32, 0.0, 1.0) } else { 0.0 };
    -0.209439510 * t * t
}
pub const NECK_FLEX: f32 = -0.104719755;
pub const UPPER_NECK_FLEX: f32 = -0.034906585;
/// Model-back, from the anatomical pivot.
pub const CHEST_RETRACTION: f32 = 0.02;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names() {
        assert!(equal_bone_name(b"Hips", b"hips") && !equal_bone_name(b"Hips", b"Hip"));
        assert!(is_primary_spine(b"Spine3") && is_primary_spine(b"spine") && !is_primary_spine(b"Spine1a"));
        assert!(is_rig_control(b"Torso_fppCamera") && is_rig_control(b"x_grp") && !is_rig_control(b"Hips"));
        assert_eq!(forearm_twist_slot(b"L_FOREARMTWIST02_jnt", true), Some(1));
        assert_eq!(forearm_twist_slot(b"r_Wrist_0_JNT", true), None);
    }
}
