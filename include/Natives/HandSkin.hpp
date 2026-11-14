#pragma once
// HAND SKIN -- the physics hand's shapes fitted to the skin of V's arms: a0_000_pma_base_hq__l/r (a man's) and
// a0_000_pwa_base_hq__l/r (a woman's), the meshes the port draws V's arms with (its vrp_hands__ components).
//
// Fitted offline from the meshes' vertices and skin weights: each vertex taken into its bone's own frame by the bone's
// inverse bind matrix, so the numbers are in the frames the game's rig has at run time (the glTF export had turned every
// bone's frame z-up to y-up; the game's (x, y, z) is the export's (x, -z, y), checked against the knuckles' places in the
// rig as the game binds it). Metres; the hand bone's frame for the palm, each segment bone's own for the fingers.
//
//   box    the palm from the wrist to the knuckles' line: the skin of the hand, its metacarpals and the knuckle ends of
//          the first phalanges, 2nd to 98th percentile across (y) and through (z). The box it replaces was 13 x 9 x 3.2 cm
//          about the bone's line: 5 cm into the fingers, wider than the hand and 6-7 mm thinner than the palm each side.
//   radius each segment's capsule: the skin's median distance from the segment's line (index, middle, ring, little,
//          thumb; the thumb's metacarpal is in the palm). A woman's fingers are 15-20 % slimmer than a man's.
//   offset the skin's centre off the bone's line in the segment bone's frame (the pad side of a finger has more flesh)
//   tip    the last segment's end in its bone's frame: where the skin ends, less the radius, so the capsule's round end
//          is the fingertip (the rig has no tip bones; 3/4 of the middle segment's length stood in for it)
//   fore   the forearm, which tapers to the wrist: 3-11 cm from the wrist, and from 11 cm to the elbow
namespace cvr::physhands {
struct HandSkin {
    float boxLo[3], boxHi[3];
    float radius[5][3];
    float offset[5][3][3];
    float tip[5][3];
    float foreLower, foreUpper;
};
// [woman][side]: side 0 the left hand, 1 the right
inline constexpr HandSkin kHandSkin[2][2] = {
    {
        // a man's left (a0_000_pma_base_hq__l)
        {{0.0067f, -0.0324f, -0.0223f}, {0.0771f, 0.0496f, 0.0259f},
         {{0.0100f, 0.0085f, 0.0066f}, {0.0102f, 0.0088f, 0.0065f}, {0.0096f, 0.0083f, 0.0059f}, {0.0088f, 0.0075f, 0.0061f}, {0.0000f, 0.0101f, 0.0082f}},
         {{{0.0f, 0.0000f, -0.0011f}, {0.0f, -0.0035f, -0.0003f}, {0.0f, -0.0013f, -0.0005f}},
          {{0.0f, -0.0020f, -0.0004f}, {0.0f, -0.0050f, 0.0005f}, {0.0f, -0.0011f, 0.0000f}},
          {{0.0f, 0.0012f, 0.0001f}, {0.0f, -0.0021f, 0.0007f}, {0.0f, -0.0010f, 0.0001f}},
          {{0.0f, 0.0010f, 0.0004f}, {0.0f, -0.0009f, 0.0001f}, {0.0f, -0.0005f, 0.0006f}},
          {{0.0f, 0.0000f, 0.0000f}, {0.0f, -0.0004f, 0.0003f}, {0.0f, -0.0010f, -0.0002f}}},
         {{0.0206f, -0.0016f, 0.0011f}, {0.0214f, -0.0018f, -0.0001f}, {0.0194f, 0.0009f, 0.0002f}, {0.0174f, 0.0009f, -0.0002f}, {0.0207f, 0.0040f, -0.0001f}},
         0.032f, 0.045f},
        // a man's right (a0_000_pma_base_hq__r)
        {{-0.0771f, -0.0510f, -0.0263f}, {-0.0036f, 0.0356f, 0.0195f},
         {{0.0100f, 0.0085f, 0.0066f}, {0.0102f, 0.0088f, 0.0065f}, {0.0096f, 0.0083f, 0.0062f}, {0.0088f, 0.0075f, 0.0061f}, {0.0000f, 0.0101f, 0.0082f}},
         {{{0.0f, 0.0000f, 0.0011f}, {0.0f, 0.0035f, 0.0003f}, {0.0f, 0.0013f, 0.0005f}},
          {{0.0f, 0.0020f, 0.0004f}, {0.0f, 0.0050f, -0.0005f}, {0.0f, 0.0011f, 0.0000f}},
          {{0.0f, -0.0012f, 0.0000f}, {0.0f, 0.0021f, -0.0007f}, {0.0f, 0.0008f, -0.0001f}},
          {{0.0f, -0.0010f, -0.0004f}, {0.0f, 0.0009f, -0.0001f}, {0.0f, 0.0005f, -0.0006f}},
          {{0.0f, 0.0000f, 0.0000f}, {0.0f, 0.0004f, -0.0003f}, {0.0f, 0.0010f, 0.0002f}}},
         {{-0.0206f, 0.0016f, -0.0011f}, {-0.0214f, 0.0018f, 0.0001f}, {-0.0192f, -0.0004f, -0.0002f}, {-0.0174f, -0.0008f, 0.0001f}, {-0.0207f, -0.0040f, 0.0001f}},
         0.032f, 0.045f},
    },
    {
        // a woman's left (a0_000_pwa_base_hq__l)
        {{0.0037f, -0.0240f, -0.0224f}, {0.0771f, 0.0424f, 0.0187f},
         {{0.0085f, 0.0070f, 0.0057f}, {0.0087f, 0.0074f, 0.0059f}, {0.0081f, 0.0071f, 0.0055f}, {0.0070f, 0.0059f, 0.0048f}, {0.0000f, 0.0086f, 0.0076f}},
         {{{0.0f, -0.0040f, -0.0016f}, {0.0f, -0.0050f, -0.0003f}, {0.0f, -0.0023f, 0.0000f}},
          {{0.0f, -0.0039f, -0.0005f}, {0.0f, -0.0055f, 0.0000f}, {0.0f, -0.0022f, -0.0001f}},
          {{0.0f, -0.0019f, 0.0022f}, {0.0f, -0.0038f, 0.0027f}, {0.0f, -0.0013f, 0.0008f}},
          {{0.0f, -0.0023f, 0.0041f}, {0.0f, -0.0036f, 0.0034f}, {0.0f, -0.0016f, 0.0011f}},
          {{0.0f, 0.0000f, 0.0000f}, {0.0f, -0.0019f, 0.0001f}, {0.0f, -0.0018f, -0.0001f}}},
         {{0.0215f, -0.0027f, 0.0001f}, {0.0222f, -0.0028f, 0.0002f}, {0.0195f, -0.0010f, 0.0035f}, {0.0168f, -0.0010f, 0.0031f}, {0.0201f, 0.0015f, -0.0013f}},
         0.025f, 0.037f},
        // a woman's right (a0_000_pwa_base_hq__r)
        {{-0.0771f, -0.0433f, -0.0193f}, {-0.0092f, 0.0326f, 0.0222f},
         {{0.0085f, 0.0070f, 0.0054f}, {0.0087f, 0.0073f, 0.0056f}, {0.0082f, 0.0071f, 0.0053f}, {0.0070f, 0.0059f, 0.0047f}, {0.0000f, 0.0086f, 0.0075f}},
         {{{0.0f, 0.0040f, 0.0017f}, {0.0f, 0.0053f, 0.0005f}, {0.0f, 0.0019f, -0.0001f}},
          {{0.0f, 0.0038f, 0.0006f}, {0.0f, 0.0058f, 0.0000f}, {0.0f, 0.0019f, -0.0002f}},
          {{0.0f, 0.0020f, -0.0019f}, {0.0f, 0.0038f, -0.0026f}, {0.0f, 0.0012f, -0.0006f}},
          {{0.0f, 0.0024f, -0.0040f}, {0.0f, 0.0036f, -0.0034f}, {0.0f, 0.0014f, -0.0009f}},
          {{0.0f, 0.0000f, 0.0000f}, {0.0f, 0.0019f, -0.0001f}, {0.0f, 0.0017f, 0.0002f}}},
         {{-0.0218f, 0.0028f, 0.0000f}, {-0.0224f, 0.0029f, 0.0001f}, {-0.0197f, 0.0009f, -0.0037f}, {-0.0169f, 0.0011f, -0.0032f}, {-0.0202f, -0.0015f, 0.0012f}},
         0.025f, 0.037f},
    },
};
// The skin for V's body (Anim/VrikState.hpp g_VRBodyFemale: 1 a woman's, 0 a man's, -1 not known yet) and this hand.
inline const HandSkin* SkinFor(int female, int side) {
    return (female < 0 || side < 0 || side > 1) ? nullptr : &kHandSkin[female ? 1 : 0][side];
}
}  // namespace cvr::physhands
