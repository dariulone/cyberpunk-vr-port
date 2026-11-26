//! include/Natives/HandSkin.hpp: the physics hand's shapes fitted to the skin of V's arms -- a0_000_pma_base_hq__l/r (a
//! man's) and a0_000_pwa_base_hq__l/r (a woman's), the meshes the port draws V's arms with (its vrp_hands__ components).
//!
//! Fitted offline from the meshes' vertices and skin weights: each vertex taken into its bone's own frame by the bone's
//! inverse bind matrix, so the numbers are in the frames the game's rig has at run time (the glTF export had turned
//! every bone's frame z-up to y-up; the game's (x, y, z) is the export's (x, -z, y), checked against the knuckles' places
//! in the rig as the game binds it). Metres; the hand bone's frame for the palm, each segment bone's own for the fingers.
//!
//!   box    the palm from the wrist to the knuckles' line: the skin of the hand, its metacarpals and the knuckle ends of
//!          the first phalanges, 2nd to 98th percentile across (y) and through (z). The box it replaces was 13 x 9 x 3.2
//!          cm about the bone's line: 5 cm into the fingers, wider than the hand and 6-7 mm thinner than the palm each side.
//!   radius each segment's capsule: the skin's median distance from the segment's line (index, middle, ring, little,
//!          thumb; the thumb's metacarpal is in the palm). A woman's fingers are 15-20 % slimmer than a man's.
//!   offset the skin's centre off the bone's line in the segment bone's frame (the pad side of a finger has more flesh)
//!   tip    the last segment's end in its bone's frame: where the skin ends, less the radius, so the capsule's round end
//!          is the fingertip (the rig has no tip bones; 3/4 of the middle segment's length stood in for it)
//!   fore   the forearm, which tapers to the wrist: 3-11 cm from the wrist, and from 11 cm to the elbow

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HandSkin {
    pub box_lo: [f32; 3],
    pub box_hi: [f32; 3],
    pub radius: [[f32; 3]; 5],
    pub offset: [[[f32; 3]; 3]; 5],
    pub tip: [[f32; 3]; 5],
    pub fore_lower: f32,
    pub fore_upper: f32,
}
/// [woman][side]: side 0 the left hand, 1 the right.
pub const HAND_SKIN: [[HandSkin; 2]; 2] = [
    [
            // a man's left (a0_000_pma_base_hq__l)
            HandSkin {
                box_lo: [0.0067, -0.0324, -0.0223],
                box_hi: [0.0771, 0.0496, 0.0259],
                radius: [[0.01, 0.0085, 0.0066], [0.0102, 0.0088, 0.0065], [0.0096, 0.0083, 0.0059], [0.0088, 0.0075, 0.0061], [0.0, 0.0101, 0.0082]],
                offset: [
                    [[0.0, 0.0, -0.0011], [0.0, -0.0035, -0.0003], [0.0, -0.0013, -0.0005]],
                    [[0.0, -0.002, -0.0004], [0.0, -0.005, 0.0005], [0.0, -0.0011, 0.0]],
                    [[0.0, 0.0012, 0.0001], [0.0, -0.0021, 0.0007], [0.0, -0.001, 0.0001]],
                    [[0.0, 0.001, 0.0004], [0.0, -0.0009, 0.0001], [0.0, -0.0005, 0.0006]],
                    [[0.0, 0.0, 0.0], [0.0, -0.0004, 0.0003], [0.0, -0.001, -0.0002]],
                ],
                tip: [[0.0206, -0.0016, 0.0011], [0.0214, -0.0018, -0.0001], [0.0194, 0.0009, 0.0002], [0.0174, 0.0009, -0.0002], [0.0207, 0.004, -0.0001]],
                fore_lower: 0.032,
                fore_upper: 0.045,
            },
            // a man's right (a0_000_pma_base_hq__r)
            HandSkin {
                box_lo: [-0.0771, -0.051, -0.0263],
                box_hi: [-0.0036, 0.0356, 0.0195],
                radius: [[0.01, 0.0085, 0.0066], [0.0102, 0.0088, 0.0065], [0.0096, 0.0083, 0.0062], [0.0088, 0.0075, 0.0061], [0.0, 0.0101, 0.0082]],
                offset: [
                    [[0.0, 0.0, 0.0011], [0.0, 0.0035, 0.0003], [0.0, 0.0013, 0.0005]],
                    [[0.0, 0.002, 0.0004], [0.0, 0.005, -0.0005], [0.0, 0.0011, 0.0]],
                    [[0.0, -0.0012, 0.0], [0.0, 0.0021, -0.0007], [0.0, 0.0008, -0.0001]],
                    [[0.0, -0.001, -0.0004], [0.0, 0.0009, -0.0001], [0.0, 0.0005, -0.0006]],
                    [[0.0, 0.0, 0.0], [0.0, 0.0004, -0.0003], [0.0, 0.001, 0.0002]],
                ],
                tip: [[-0.0206, 0.0016, -0.0011], [-0.0214, 0.0018, 0.0001], [-0.0192, -0.0004, -0.0002], [-0.0174, -0.0008, 0.0001], [-0.0207, -0.004, 0.0001]],
                fore_lower: 0.032,
                fore_upper: 0.045,
            },
    ],
    [
            // a woman's left (a0_000_pwa_base_hq__l)
            HandSkin {
                box_lo: [0.0037, -0.024, -0.0224],
                box_hi: [0.0771, 0.0424, 0.0187],
                radius: [[0.0085, 0.007, 0.0057], [0.0087, 0.0074, 0.0059], [0.0081, 0.0071, 0.0055], [0.007, 0.0059, 0.0048], [0.0, 0.0086, 0.0076]],
                offset: [
                    [[0.0, -0.004, -0.0016], [0.0, -0.005, -0.0003], [0.0, -0.0023, 0.0]],
                    [[0.0, -0.0039, -0.0005], [0.0, -0.0055, 0.0], [0.0, -0.0022, -0.0001]],
                    [[0.0, -0.0019, 0.0022], [0.0, -0.0038, 0.0027], [0.0, -0.0013, 0.0008]],
                    [[0.0, -0.0023, 0.0041], [0.0, -0.0036, 0.0034], [0.0, -0.0016, 0.0011]],
                    [[0.0, 0.0, 0.0], [0.0, -0.0019, 0.0001], [0.0, -0.0018, -0.0001]],
                ],
                tip: [[0.0215, -0.0027, 0.0001], [0.0222, -0.0028, 0.0002], [0.0195, -0.001, 0.0035], [0.0168, -0.001, 0.0031], [0.0201, 0.0015, -0.0013]],
                fore_lower: 0.025,
                fore_upper: 0.037,
            },
            // a woman's right (a0_000_pwa_base_hq__r)
            HandSkin {
                box_lo: [-0.0771, -0.0433, -0.0193],
                box_hi: [-0.0092, 0.0326, 0.0222],
                radius: [[0.0085, 0.007, 0.0054], [0.0087, 0.0073, 0.0056], [0.0082, 0.0071, 0.0053], [0.007, 0.0059, 0.0047], [0.0, 0.0086, 0.0075]],
                offset: [
                    [[0.0, 0.004, 0.0017], [0.0, 0.0053, 0.0005], [0.0, 0.0019, -0.0001]],
                    [[0.0, 0.0038, 0.0006], [0.0, 0.0058, 0.0], [0.0, 0.0019, -0.0002]],
                    [[0.0, 0.002, -0.0019], [0.0, 0.0038, -0.0026], [0.0, 0.0012, -0.0006]],
                    [[0.0, 0.0024, -0.004], [0.0, 0.0036, -0.0034], [0.0, 0.0014, -0.0009]],
                    [[0.0, 0.0, 0.0], [0.0, 0.0019, -0.0001], [0.0, 0.0017, 0.0002]],
                ],
                tip: [[-0.0218, 0.0028, 0.0], [-0.0224, 0.0029, 0.0001], [-0.0197, 0.0009, -0.0037], [-0.0169, 0.0011, -0.0032], [-0.0202, -0.0015, 0.0012]],
                fore_lower: 0.025,
                fore_upper: 0.037,
            },
    ],
];
/// SkinFor: the skin for V's body (g_VRBodyFemale: 1 a woman's, 0 a man's, -1 not known yet) and this hand.
pub fn skin_for(female: i32, side: i32) -> Option<&'static HandSkin> {
    if female < 0 || !(0..=1).contains(&side) {
        return None;
    }
    Some(&HAND_SKIN[usize::from(female != 0)][side as usize])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn each_body_has_two_mirrored_hands() {
        assert!(skin_for(-1, 0).is_none() && skin_for(0, 2).is_none());
        for woman in [0, 1] {
            let (l, r) = (skin_for(woman, 0).unwrap(), skin_for(woman, 1).unwrap());
            assert!((l.box_hi[0] + r.box_lo[0]).abs() < 1e-6, "the palm's length mirrored");
            assert_eq!(l.fore_lower, r.fore_lower);
            for f in 0..5 {
                assert!((l.tip[f][0] + r.tip[f][0]).abs() < 0.001, "a fingertip mirrored");
            }
        }
        assert_eq!(skin_for(0, 0).unwrap().radius[0][0], 0.0100);
        assert_eq!(skin_for(1, 1).unwrap().fore_upper, 0.037);
        assert!(skin_for(1, 0).unwrap().radius[1][0] < skin_for(0, 0).unwrap().radius[1][0], "a woman's fingers are slimmer");
    }
}
