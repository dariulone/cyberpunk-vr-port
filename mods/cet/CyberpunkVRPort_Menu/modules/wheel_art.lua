-- Written by tools/vr_lobby_assets/make_wheel_art.py: the weapon wheel's shapes (canvas px, degrees) and
-- its atlas parts, each drawn at its own size with the wheel's centre (a disc's own) at ox, oy.
return {
  ATLAS = "vrport\\ui\\vr_wheel.inkatlas",
  weapon = { inner = 170, outer = 480, half = 26, inset = 6 },
  arm = { inner = 200, outer = 450, half = 22, inset = 6 },
  holster = { inner = 215, outer = 425, half = 18, inset = 6 },
  disc = 76,
  parts = {
    arm_band = { w = 350, h = 297, ox = 175.000, oy = 466.000 },
    arm_edge = { w = 350, h = 297, ox = 175.000, oy = 466.000 },
    arm_fill = { w = 350, h = 297, ox = 175.000, oy = 466.000 },
    disc_edge = { w = 184, h = 184, ox = 92.000, oy = 92.000 },
    disc_fill = { w = 184, h = 184, ox = 92.000, oy = 92.000 },
    holster_band = { w = 273, h = 253, ox = 136.500, oy = 441.000 },
    holster_edge = { w = 273, h = 253, ox = 136.500, oy = 441.000 },
    holster_fill = { w = 273, h = 253, ox = 136.500, oy = 441.000 },
    weapon_band = { w = 435, h = 360, ox = 217.500, oy = 496.000 },
    weapon_edge = { w = 435, h = 360, ox = 217.500, oy = 496.000 },
    weapon_fill = { w = 435, h = 360, ox = 217.500, oy = 496.000 },
  },
}
