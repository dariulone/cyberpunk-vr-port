-- The controls guide in the lobby: the README's Controls (General, On Foot, Driving) as the controllers' glyphs beside
-- what they do -- a Controls page in the lobby's menu with all of it, and a board standing in the hall with the major
-- ones, in view while the player walks about and tries them.
--
-- THE GLYPHS ARE DRAWN, not pictures: rings, bars and arcs of ink (modules/ink.lua), in the panels' own light, the way
-- the lobby's footer draws its loudspeaker. They follow the README's icons (docs/controls/icons, Kenney's Meta Quest
-- glyphs and the VR gestures drawn to match): a face button is a ring and its letter, a trigger or a grip a pill and
-- its two letters, a stick a ring with its L or R and the way it is pushed, a click a chevron over it. Every glyph is
-- drawn in a box `size` px square on a grid of 100.
--
-- A binding is a list of tokens: a glyph's name, or "+" / "/" between two (drawn as text).
local M = {}
local Ink = require("modules/ink")

---------------------------------------------------------------- the glyphs

local function bar(out, p, x, y, s, x1, y1, x2, y2, col, op, w)
  out[#out + 1] = Ink.line(p, x + x1 * s, y + y1 * s, x + x2 * s, y + y2 * s, col, op, (w or 6) * s)
end

local function ring(out, p, x, y, s, cx, cy, r, col, op, w)
  for _, b in ipairs(Ink.ring(p, x + cx * s, y + cy * s, r * s, (w or 6) * s, col, op, 28)) do out[#out + 1] = b end
end

local function arc(out, p, x, y, s, cx, cy, r, a, b, col, op, w)
  for _, l in ipairs(Ink.arc(p, x + cx * s, y + cy * s, r * s, a, b, (w or 6) * s, col, op, 10)) do out[#out + 1] = l end
end

local function label(out, p, x, y, s, cx, cy, str, size, col)
  out[#out + 1] = Ink.text(p, x + (cx - 40) * s, y + (cy - 30) * s, 80 * s, 60 * s, str, size * s, col, "centre", true,
                           "Bold")
end

-- A chevron: its point at (px, py), its arms back along (dx, dy) and spread `spread` either side of it.
local function chevron(out, p, x, y, s, px, py, dx, dy, col, op)
  local n = math.sqrt(dx * dx + dy * dy)
  local ux, uy = dx / n, dy / n
  local bx, by = px - ux * 9, py - uy * 9
  bar(out, p, x, y, s, bx - uy * 8, by + ux * 8, px, py, col, op, 5)
  bar(out, p, x, y, s, bx + uy * 8, by - ux * 8, px, py, col, op, 5)
end

local function faceButton(letter)
  return function(out, p, x, y, s, col, op)
    ring(out, p, x, y, s, 50, 50, 40, col, op)
    label(out, p, x, y, s, 50, 50, letter, 52, col)
  end
end

-- A trigger or a grip: a pill and its letters (LT, RT, LG, RG).
local function pill(text)
  return function(out, p, x, y, s, col, op)
    arc(out, p, x, y, s, 34, 50, 24, math.pi * 0.5, math.pi * 1.5, col, op)
    arc(out, p, x, y, s, 66, 50, 24, -math.pi * 0.5, math.pi * 0.5, col, op)
    bar(out, p, x, y, s, 34, 26, 66, 26, col, op)
    bar(out, p, x, y, s, 34, 74, 66, 74, col, op)
    label(out, p, x, y, s, 50, 50, text, 34, col)
  end
end

-- A stick: its ring and letter, chevrons for the ways it is pushed ("u", "d", "l", "r"), and for a click one over it
-- pointing down into it.
local function stick(letter, ways, click)
  return function(out, p, x, y, s, col, op)
    ring(out, p, x, y, s, 50, 50, 34, col, op)
    label(out, p, x, y, s, 50, 50, letter, 44, col)
    for w in (ways or ""):gmatch(".") do
      if w == "u" then chevron(out, p, x, y, s, 50, 3, 0, -1, col, op) end
      if w == "d" then chevron(out, p, x, y, s, 50, 97, 0, 1, col, op) end
      if w == "l" then chevron(out, p, x, y, s, 3, 50, -1, 0, col, op) end
      if w == "r" then chevron(out, p, x, y, s, 97, 50, 1, 0, col, op) end
    end
    if click then chevron(out, p, x, y, s, 50, 15, 0, 1, col, op) end
  end
end

-- An open hand, palm on (cx, cy): the palm and four fingers.
local function hand(out, p, x, y, s, cx, cy, col, op)
  bar(out, p, x, y, s, cx - 9, cy, cx + 9, cy, col, op, 14)
  for i = 0, 3 do
    local fx = cx - 9 + i * 6
    bar(out, p, x, y, s, fx, cy - 3, fx, cy - 15, col, op, 4)
  end
  bar(out, p, x, y, s, cx - 11, cy + 4, cx - 16, cy - 4, col, op, 4)
end

local GLYPHS = {
  a = faceButton("A"), b = faceButton("B"), x = faceButton("X"), y = faceButton("Y"),
  menu = function(out, p, x, y, s, col, op)
    ring(out, p, x, y, s, 50, 50, 40, col, op)
    for _, ly in ipairs({ 38, 50, 62 }) do bar(out, p, x, y, s, 34, ly, 66, ly, col, op, 5) end
  end,
  lt = pill("LT"), rt = pill("RT"), lg = pill("LG"), rg = pill("RG"),
  ls = stick("L"), ls_up = stick("L", "u"), ls_vertical = stick("L", "ud"), ls_press = stick("L", nil, true),
  rs = stick("R"), rs_up = stick("R", "u"), rs_down = stick("R", "d"), rs_horizontal = stick("R", "lr"),
  rs_vertical = stick("R", "ud"), rs_all = stick("R", "udlr"), rs_press = stick("R", nil, true),
  -- held: a stopwatch
  hold = function(out, p, x, y, s, col, op)
    ring(out, p, x, y, s, 50, 56, 32, col, op)
    bar(out, p, x, y, s, 50, 56, 50, 36, col, op)
    bar(out, p, x, y, s, 50, 24, 50, 16, col, op)
    bar(out, p, x, y, s, 42, 13, 58, 13, col, op)
  end,
  -- the left hand brought up to the left ear: a head and shoulders, the hand low at the side, the way it goes
  ear_left = function(out, p, x, y, s, col, op)
    ring(out, p, x, y, s, 66, 34, 15, col, op)
    arc(out, p, x, y, s, 66, 92, 30, math.pi, math.pi * 2, col, op)
    hand(out, p, x, y, s, 20, 84, col, op)
    arc(out, p, x, y, s, 46, 64, 28, math.pi, math.pi * 1.5, col, op, 5)
    chevron(out, p, x, y, s, 48, 36, 1, 0, col, op)
  end,
  -- squat in the room: a crouched figure and the way down
  squat = function(out, p, x, y, s, col, op)
    ring(out, p, x, y, s, 32, 16, 8, col, op)
    bar(out, p, x, y, s, 32, 26, 28, 48, col, op)
    bar(out, p, x, y, s, 28, 48, 48, 54, col, op)
    bar(out, p, x, y, s, 48, 54, 40, 78, col, op)
    bar(out, p, x, y, s, 31, 32, 50, 40, col, op)
    bar(out, p, x, y, s, 76, 18, 76, 76, col, op)
    chevron(out, p, x, y, s, 76, 84, 0, 1, col, op)
  end,
  -- a melee weapon swung: its blade and the arc it takes
  swing = function(out, p, x, y, s, col, op)
    bar(out, p, x, y, s, 30, 76, 66, 34, col, op, 8)
    bar(out, p, x, y, s, 22, 64, 40, 82, col, op)
    arc(out, p, x, y, s, 58, 68, 44, math.pi * 1.1, math.pi * 1.55, col, op, 5)
    chevron(out, p, x, y, s, 66, 24, 1, -0.1, col, op)
  end,
  -- reaching for a holster: a pistol at the hip, the hand coming down to its grip
  holster = function(out, p, x, y, s, col, op)
    bar(out, p, x, y, s, 26, 56, 86, 56, col, op, 12)
    bar(out, p, x, y, s, 34, 62, 28, 88, col, op, 12)
    arc(out, p, x, y, s, 48, 66, 8, 0, math.pi, col, op, 4)
    ring(out, p, x, y, s, 34, 16, 10, col, op)
    bar(out, p, x, y, s, 34, 30, 34, 40, col, op, 5)
    chevron(out, p, x, y, s, 34, 48, 0, 1, col, op)
  end,
  -- a throw: the hand opening and where it goes
  throw = function(out, p, x, y, s, col, op)
    hand(out, p, x, y, s, 24, 66, col, op)
    bar(out, p, x, y, s, 44, 40, 82, 40, col, op)
    chevron(out, p, x, y, s, 90, 40, 1, 0, col, op)
    bar(out, p, x, y, s, 50, 54, 70, 54, col, op, 4)
  end,
  -- a hand on the wheel or the handlebars
  wheel_grab = function(out, p, x, y, s, col, op)
    ring(out, p, x, y, s, 48, 56, 34, col, op)
    ring(out, p, x, y, s, 48, 56, 7, col, op)
    bar(out, p, x, y, s, 14, 58, 41, 58, col, op)
    bar(out, p, x, y, s, 55, 58, 82, 58, col, op)
    bar(out, p, x, y, s, 48, 63, 48, 90, col, op)
    hand(out, p, x, y, s, 74, 26, col, op)
  end,
  -- steering: the wheel and the tilt of the line through both hands
  steer = function(out, p, x, y, s, col, op)
    ring(out, p, x, y, s, 50, 62, 28, col, op)
    ring(out, p, x, y, s, 50, 62, 6, col, op)
    bar(out, p, x, y, s, 22, 64, 44, 64, col, op)
    bar(out, p, x, y, s, 56, 64, 78, 64, col, op)
    arc(out, p, x, y, s, 50, 62, 44, math.pi * 1.2, math.pi * 1.8, col, op, 5)
    chevron(out, p, x, y, s, 80, 32, 0.8, 0.6, col, op)
    chevron(out, p, x, y, s, 20, 32, -0.8, 0.6, col, op)
  end,
  -- the horn: a hand on the middle of the wheel
  horn = function(out, p, x, y, s, col, op)
    ring(out, p, x, y, s, 40, 54, 30, col, op)
    ring(out, p, x, y, s, 40, 54, 10, col, op)
    arc(out, p, x, y, s, 60, 54, 18, -0.6, 0.6, col, op, 5)
    arc(out, p, x, y, s, 60, 54, 30, -0.6, 0.6, col, op, 5)
  end,
}

-- One glyph in the box (x, y, size square); every widget it is made of.
function M.glyph(parent, name, x, y, size, col, op)
  local out = {}
  local draw = GLYPHS[name]
  if draw ~= nil then draw(out, parent, x, y, size / 100, col, op or 1.0) end
  return out
end

-- A binding's tokens left to right from (x, y), each glyph `size` square and a "+" or "/" between them narrower;
-- the width they take.
function M.strip(parent, tokens, x, y, size, col, sepCol)
  local cx = x
  for _, t in ipairs(tokens) do
    if GLYPHS[t] ~= nil then
      M.glyph(parent, t, cx, y, size, col)
      cx = cx + size + size * 0.08
    else
      Ink.text(parent, cx, y, size * 0.42, size, t, size * 0.5, sepCol or col, "centre", true)
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
    { { "ls_press", "+", "rs_all" }, "D-pad", "hold L3, right stick to the stop; let go alone for a plain L3" },
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
