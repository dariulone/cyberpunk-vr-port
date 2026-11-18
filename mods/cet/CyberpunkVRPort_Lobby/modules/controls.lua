-- The controls guide in the lobby: the README's Controls (General, On Foot, Driving) as the controllers' glyphs beside
-- what they do -- a Controls page in the lobby's menu with all of it, and a board standing in the hall with the major
-- ones, in view while the player walks about and tries them.
--
-- THE GLYPHS ARE THE README'S ICONS (docs/controls/icons: Kenney's Meta Quest glyphs and the VR gestures drawn to
-- match), without their tiles, white on clear in one atlas -- vrport\ui\vr_controls_icons.inkatlas, a part per icon
-- named as the icon (tools/controls_icons/make_ink_icons.py) -- drawn tinted in the panels' own light.
--
-- A binding is a list of tokens: an icon's name, or "+" / "/" between two (drawn as text).
local M = {}
local Ink = require("modules/ink")

---------------------------------------------------------------- the glyphs

local ATLAS = "vrport\\ui\\vr_controls_icons.inkatlas"
local ICONS = {}
for name in ("a b x y menu lt rt lg rg ls ls_up ls_down ls_vertical ls_press rs rs_up rs_down rs_left rs_right " ..
             "rs_horizontal rs_vertical rs_press ear_left hold squat swing holster throw wheel_grab steer horn"):gmatch("%S+") do
  ICONS[name] = true
end

-- One icon in the box (x, y, size square), tinted; its widgets.
function M.glyph(parent, name, x, y, size, col, op)
  if not ICONS[name] then return {} end
  return { Ink.image(parent, x, y, size, size, ATLAS, name, col, op or 1.0) }
end

-- A binding's tokens left to right from (x, y), each glyph `size` square and a "+" or "/" between them narrower;
-- the width they take.
function M.strip(parent, tokens, x, y, size, col, sepCol)
  local cx = x
  for _, t in ipairs(tokens) do
    if ICONS[t] then
      M.glyph(parent, t, cx, y, size, col)
      cx = cx + size + size * 0.08
    else
      Ink.text(parent, cx, y, size * 0.42, size, t, math.floor(size * 0.5 + 0.5), sepCol or col, "centre", true)
      cx = cx + size * 0.5
    end
  end
  return cx - x
end

---------------------------------------------------------------- the guide

-- The README's Controls, in the same order: glyphs, what it does, a word on it; board = shown on the standing board.
M.SECTIONS = {
  { title = "General", entries = {
    { { "ls_press", "+", "rs_press", "hold" }, "VR menu", "hold both sticks in for half a second", board = true },
    { { "ls_press", "+", "rs_vertical" }, "Dialogue choices", "hold L3, right stick up or down to the stop; X picks (A in a car)", board = true },
    { { "menu" }, "Pause menu", "left on Touch; right on Steam Frame" },
    { { "ls_press", "+", "rs_vertical", "rs_horizontal" }, "D-pad", "hold L3, right stick to the stop; let go alone for a plain L3" },
  } },
  { title = "On Foot", entries = {
    { { "ls" }, "Move", "one speed, the direction from the stick" },
    { { "ls_up" }, "Sprint", "to the stop, held 0.2 s", board = true },
    { { "rs_up" }, "Dash", "to the stop, once per push", board = true },
    { { "rs_horizontal" }, "Turn", "snap or smooth", board = true },
    { { "rs_down" }, "Crouch", "to the stop" },
    { { "squat" }, "Crouch in the room", "squat; the stick still works", board = true },
    { { "ear_left", "+", "ls_press" }, "Scanner", "left controller to your left ear, click; again to close", board = true },
    { { "ls_vertical" }, "Quickhack list", "to the stop, scanner open" },
    { { "rt", "/", "lt" }, "Fire / Aim", "the left trigger also blocks", board = true },
    { { "holster", "rg" }, "Draw / holster", "the right grip at a holster", board = true },
    { { "b" }, "Drop the magazine", "weapon in hand; holstered, B is the game's", board = true },
    { { "lg" }, "Grab the magazine", "during a reload", board = true },
    { { "rs_press" }, "Rack the slide", "physical reload", board = true },
    { { "x" }, "Reload / interact", nil, board = true },
    { { "a" }, "Jump", nil, board = true },
    { { "y" }, "Next weapon", "tap" },
    { { "y", "hold" }, "Weapon wheel", "point and let go of Y", board = true },
    { { "ls_press", "+", "lg", "hold" }, "Cyberware", "Sandevistan, Berserk, overclock" },
    { { "swing" }, "Melee", "swing the weapon", board = true },
    { { "lt", "+", "rg", "throw" }, "Throw a melee weapon", "hold LT, grip it, throw and open the hand" },
  } },
  { title = "Driving", entries = {
    { { "wheel_grab", "lg", "/", "rg" }, "Grab the wheel", "a grip with the hand at the wheel or the bars", board = true },
    { { "steer" }, "Steer", "tilt the line through both hands", board = true },
    { { "horn" }, "Horn", "a hand on the middle of the wheel" },
    { { "rt", "/", "lt" }, "Throttle / Brake", "with a weapon drawn RT fires", board = true },
    { { "x", "hold" }, "Get out", "a hold, never B", board = true },
    { { "a" }, "Confirm a dialogue line", "X is the exit here" },
  } },
}

-- Where the board stands: to the right of the menu and its preview window, turned to the eyes (lobby frame, degrees
-- from the heading the menu is on, metres).
M.BOARD = { bearing = 82.0, dist = 1.25, height = 1.50 }

local GLYPH = 124                         -- a glyph's square on a page's row (px)
local ROW_H = 168

-- The Controls page: every section, a header and its rows -- the glyphs on the left, what it does and a word on it.
function M.page(title)
  local rows = {}
  for _, sec in ipairs(M.SECTIONS) do
    rows[#rows + 1] = { key = "h_" .. sec.title, header = true, label = sec.title }
    for i, e in ipairs(sec.entries) do
      rows[#rows + 1] = { key = sec.title .. i, height = ROW_H, draw = function(c, w, h, C)
        local sw = 640
        M.strip(c, e[1], 20, (h - GLYPH) / 2, GLYPH, C.glyph, C.dim)
        local tw = w - sw - 60
        Ink.text(c, sw + 40, 12, tw, h * 0.5, e[2], Ink.fit(e[2], tw, 72, 48), C.text, "left")
        if e[3] ~= nil then
          Ink.text(c, sw + 40, h * 0.5, tw, h * 0.42, e[3], Ink.fit(e[3], tw, 50, 34), C.dim, "left", true)
        end
        Ink.rect(c, 0, h - 3, w, 3, C.line, 1.0)
      end }
    end
  end
  return { title = title or "Controls", subtitle = "Meta Quest Touch; other controllers alike", code = "SYS//CONTROLS",
           rows = function() return rows end }
end

-- The board's face: the major ones in two columns, under the section they belong to, on a canvas cw x ch.
function M.drawBoard(layer, cw, ch, C)
  local x0, top, bottom, gap = 120, 300, ch - 110, 70
  Ink.text(layer, x0, 70, cw - 2 * x0, 150, "Controls", 118, C.white, "left")
  Ink.text(layer, x0, 70, cw - 2 * x0, 70, "VR//CONTROLS", 40, C.dim, "right")
  Ink.rect(layer, x0, 245, cw - 2 * x0, 4, C.red, 0.8)
  local colW = math.floor((cw - 2 * x0 - gap) / 2)
  local items = {}
  for _, sec in ipairs(M.SECTIONS) do
    local mine = {}
    for _, e in ipairs(sec.entries) do if e.board then mine[#mine + 1] = e end end
    if #mine > 0 then
      items[#items + 1] = { header = sec.title }
      for _, e in ipairs(mine) do items[#items + 1] = { entry = e } end
    end
  end
  local HEAD, ENTRY, G = 76, 124, 104
  local col, y = 0, top
  for _, it in ipairs(items) do
    local h = it.header and HEAD or ENTRY
    if y + h > bottom and col == 0 then col, y = 1, top end
    local x = x0 + col * (colW + gap)
    if it.header then
      Ink.text(layer, x, y + 10, colW, 56, it.header, 50, C.red, "left")
      Ink.rect(layer, x, y + h - 6, colW, 3, C.line, 1.0)
    else
      local sw = M.strip(layer, it.entry[1], x, y + (h - G) / 2, G, C.glyph, C.dim)
      local lx = x + math.max(sw, 4 * G) + 30
      local lw = x + colW - lx
      Ink.text(layer, lx, y, lw, h, it.entry[2], Ink.fit(it.entry[2], lw, 58, 40), C.text, "left")
    end
    y = y + h
  end
end

return M
