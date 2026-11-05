-- Written by tools/vr_lobby_assets/make_lobby_scene.py, all in the lobby's frame: the entities the lobby
-- spawns; the things to pick up (entity, where it rests, its rotation r i j k, its size for the grip, kg, the
-- shape and half sizes the hand closes on); the
-- screens the game draws on and the buttons it presses (centre, right, up, the side facing out, size); the
-- dials it turns (centre, right, up, the side facing out, the radius of the rim); the exposure the place is
-- shown at, in stops over the game's auto exposure, and the clock and weather it is held at; where the
-- player is put when the lobby comes up (Night City coordinates, the heading in degrees; none = where the
-- player stands): the frame (lobby_layout.PLACE).
return {
  exposure = 0.50,
  atmosphere = { hour = 21, minute = 30, weather = "24h_weather_light_clouds" },
  place = { position = { -417.00000, 422.50000, -215.00000 }, yaw = -90.0 },
  holo = { bearing = 0.0, dist = 1.10, height = 1.55, panel = { 0.98000, 0.75000 }, gap = 0.08,
           window = { bearing = -10.0, dist = 1.50, height = 1.10, size = { 2.20000, 2.00000 } } },
  figure = { bearing = 38.0, dist = 2.30 },
  entities = {
    "vrport\\lobby\\vr_lobby_near.ent",
  },
  items = {
    { name = "item can", ent = "vrport\\lobby\\vr_lobby_item_0.ent", position = { -1.97569, -0.77868, 0.86350 }, rotation = { 0.51504, 0.00000, 0.00000, 0.85717 }, radius = 0.0610, mass = 0.350, shape = "cylinder", half = { 0.03300, 0.03300, 0.06100 } },
    { name = "item chibi", ent = "vrport\\lobby\\vr_lobby_item_1.ent", position = { -1.34237, -1.41577, 0.92762 }, rotation = { 0.35793, 0.00000, 0.00000, 0.93375 }, radius = 0.1251, mass = 0.600, shape = "box", half = { 0.06742, 0.07062, 0.12513 }, grasp_half = { 0.05500, 0.02400, 0.06250 }, grasp_at = { -0.00011, -0.00486, -0.06262 } },
    { name = "item cube", ent = "vrport\\lobby\\vr_lobby_item_2.ent", position = { -1.85475, -0.96354, 0.88750 }, rotation = { 0.51504, 0.00000, 0.00000, 0.85717 }, radius = 0.0850, mass = 1.200, shape = "box", half = { 0.08500, 0.08500, 0.08500 } },
    { name = "item shard", ent = "vrport\\lobby\\vr_lobby_item_3.ent", position = { -1.58496, -0.66152, 0.81050 }, rotation = { 0.51504, 0.00000, 0.00000, 0.85717 }, radius = 0.0550, mass = 0.050, shape = "box", half = { 0.05500, 0.03000, 0.00800 } },
    { name = "item shard.001", ent = "vrport\\lobby\\vr_lobby_item_4.ent", position = { -1.59771, -0.50974, 0.81050 }, rotation = { 0.51504, 0.00000, 0.00000, 0.85717 }, radius = 0.0550, mass = 0.050, shape = "box", half = { 0.05500, 0.03000, 0.00800 } },
    { name = "item skippy", ent = "vrport\\lobby\\vr_lobby_item_5.ent", position = { -1.99407, -0.40334, 0.87746 }, rotation = { 0.65641, 0.00000, 0.00000, 0.75440 }, radius = 0.0750, mass = 0.150, shape = "cylinder", half = { 0.06718, 0.03825, 0.07497 }, grasp_half = { 0.03200, 0.03200, 0.06000 }, grasp_at = { -0.01002, -0.00003, -0.01496 } },
    { name = "item sphere", ent = "vrport\\lobby\\vr_lobby_item_6.ent", position = { -1.65981, -1.24496, 0.91250 }, rotation = { 0.51504, 0.00000, 0.00000, 0.85717 }, radius = 0.1100, mass = 2.000, shape = "sphere", half = { 0.11000, 0.11000, 0.11000 } },
  },
  screens = {
  },
  buttons = {
  },
  dials = {
  },
}
