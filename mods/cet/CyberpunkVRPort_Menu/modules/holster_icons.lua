-- Written by tools/vr_lobby_assets/make_holster_icons.py: the holster place icons' atlas and its parts --
-- "body" (the figure; "body_w" the woman's), "none" (not on the body) and <place>_<family> (the weapon where
-- it hangs; <place>_<family>_w over the woman's figure where it differs), each a square cell drawn over the
-- figure's.
return {
  ATLAS = "vrport\\ui\\vr_holster_icons.inkatlas",
  parts = {
    ["body"] = true,
    ["body_w"] = true,
    ["chest_l_knife"] = true,
    ["chest_r_knife"] = true,
    ["hip_l_blade"] = true,
    ["hip_l_gun"] = true,
    ["hip_r_blade"] = true,
    ["hip_r_gun"] = true,
    ["lower_back_blade"] = true,
    ["lower_back_blade_w"] = true,
    ["lower_back_knife"] = true,
    ["lower_back_knife_w"] = true,
    ["lower_back_long"] = true,
    ["lower_back_long_w"] = true,
    ["none"] = true,
    ["shoulder_l_blade"] = true,
    ["shoulder_l_blade_w"] = true,
    ["shoulder_l_long"] = true,
    ["shoulder_l_long_w"] = true,
    ["shoulder_r_blade"] = true,
    ["shoulder_r_blade_w"] = true,
    ["shoulder_r_long"] = true,
    ["shoulder_r_long_w"] = true,
    ["thigh_l_gun"] = true,
    ["thigh_l_knife"] = true,
    ["thigh_r_gun"] = true,
    ["thigh_r_knife"] = true,
    ["underarm_l_gun"] = true,
    ["underarm_r_gun"] = true,
  },
}
