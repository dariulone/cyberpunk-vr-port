-- The lobby's menu in the tutorial hall: holographic panels in the air before the player -- Half-Life: Alyx's menu,
-- drawn in the game's own colours (its red, its cyan for what is chosen, the angled brackets of its screens).
--
-- PANELS ON AN ARC. The page the player is on hangs in the middle (the scene's holo frame: a bearing from the
-- player's heading, a distance, the height of its centre); the page it was opened from hangs to its left. Opening a
-- page brings a new panel in from the right into the middle while the one there moves to the left and the one that
-- was on the left fades out; going back runs it the other way. Every panel faces the player's eyes on their spot.
--
-- WHAT A PANEL IS. A screen of its own (vrport\ui\vr_lobby_holo.ent, lobby_layout.HOLO_SCREEN: the terminal's screen
-- made a little larger -- an additive plane, black is clear air, and a WorldWidgetComponent "vrp_lobby_ui" over the
-- terminal's canvas; src/Natives/MenuSurface.cpp gives each its own texture), moved with SetWorldTransform while it
-- slides; a panel fades with its canvas's opacity. It is shown once its canvas has been drawn (VRMenuSurfaceReady).
-- Drawn as the game's own screens are: a frame cut at two corners, the game's logo over the first page, rows that
-- light up as the game's lists do.
--
-- A PAGE is a table: title (or logo = true: the game's logo instead), subtitle, rows() -> the rows to show now (a row:
-- key, label, detail, enabled, go; a choice row also has value, left and right; a header row has header = true; a line
-- of text has text = true, warn = true for the game's red; a header with go folds its section), onBack, footer (or a
-- function returning it: buttons drawn as icons in the footer from the right -- key, glyph(parent, x, y, size, col)
-- -> its widgets, drawn as a row is, go), and
-- update(page, now) -> true when the page on top has to be drawn again. Its rows scroll when there are more than the
-- panel shows. A page is drawn again when M.refresh() says its data changed or a press changed it; a hand moving over
-- it only lights up the row it is on.
--
-- TOUCH IT OR POINT AT IT, as the terminal: an index fingertip presses what it pushes into, a hand's pointer clicks on
-- a trigger pull. Pressing the panel on the left goes back to it. While a hand is on a panel its trigger, grip and
-- face buttons do not reach the game (VRMenuCapture). A pointer is drawn as the VR menu draws its own: a beam from the
-- hand to the panel (modules/beams.lua) and the menu's mark where it lands, its rings yellow over what a pull would
-- press, a ripple where it pressed.
local M = {}
local Spawner = require("modules/spawner")
local Ink = require("modules/ink")
local Beams = require("modules/beams")
local Sound = require("modules/sound")

local ENT = "vrport\\ui\\vr_lobby_holo.ent"
local TAG = "VRLobbyHolo"
local UI = "vrp_lobby_ui"
local CW, CH = 2580, 1980                -- tools/vr_lobby_assets/lobby_layout.py TERMINAL_CANVAS
local HAPTIC_CLICK, HAPTIC_HOVER = 20, 21
local EYE = 1.7                          -- the eyes the panels turn to, over the player's spot (m)

-- The game's menu colours, as light on clear air.
local RED = { 1.10, 0.34, 0.30 }
local RED_DIM = { 0.34, 0.08, 0.07 }
local GLASS = { 0.16, 0.035, 0.03 }
local TEXT = { 1.00, 0.52, 0.46 }
local CYAN = { 0.34, 0.92, 1.00 }
local WHITE = { 1.0, 0.96, 0.92 }
local DIM = { 0.38, 0.17, 0.16 }
-- The pointer, drawn as the VR menu draws its own but in the panel's colours: a mark for each hand, red, the light
-- text colour over what a pull would press; 4 cm across (the VR menu's 1.6 cm was lost on these panels); the beam's
-- glow and core, fainter than the VR menu's.
local MARK, MARK_HOT, MARK_DOT = RED, TEXT, TEXT
local MARK_M, RIPPLE = 0.04, 0.24
local BEAM = { glow = RED, glowOp = 0.16, core = TEXT, coreOp = 0.5 }
local PHONE_ATLAS = "base\\gameplay\\gui\\widgets\\phone\\new_phone_assets.inkatlas"
-- The game's logo over the first page: the main menu's own, its atlas part (one per script it is drawn in).
local LOGO_ATLAS = "base\\gameplay\\gui\\fullscreen\\main_menu\\menu_background.inkatlas"
local LOGO_PARTS = { ["kr-kr"] = "cp_logo_kr", ["zh-cn"] = "cp_logo_zhs", ["zh-tw"] = "cp_logo_zht" }
local LOGO_W, LOGO_H = 940, 230          -- 1019 x 249 in its atlas

-- The canvas: a header, the rows (a list that scrolls), a footer with BACK.
local PAD = 70
local HEAD_Y, HEAD_H = 60, 250
local ROW_H, ROW_GAP = 148, 18
local FOOT_H = 120

-- A canvas's layout: its size (px); the list of rows, its left, width, top and size; the footer's top (inside the
-- glass, clear of its bottom right cut); a slider's track on a row, its right end where a choice's value is. A panel's
-- rows are as wide as its canvas; the new game's window has them down its left (WINDOW).
local function layout(cw, ch, rowW)
  local L = { cw = cw, ch = ch, rowX = 130, rowW = rowW, rowTop = 360, footY = ch - 230 }
  L.listW, L.listH = rowW + 40, L.footY - 40 - L.rowTop
  L.sliderW = math.min(820, math.floor((rowW - 360) / 2))
  L.sliderX = rowW - 360 - L.sliderW
  return L
end
local PANEL = layout(CW, CH, CW - 260)
local FOOT_BUTTON_W, FOOT_ICON_H = 190, 100 -- a footer button (page.footer) and the square its glyph is drawn in
local CUT = 64                           -- the frame's cut corners
local TOUCH, HOVER_TIP = 0.006, 0.06
local PULL, LET_GO = 0.75, 0.35
local SLIDE, FADE = 7.0, 6.0             -- 1/s: how fast a panel reaches its place and its opacity

local S = { time = 0, anchor = nil, spec = nil, stack = {}, leaving = {}, hands = { {}, {} }, captured = false,
            texts = { back = "BACK", more = "MORE" } }

---------------------------------------------------------------- the frame of a place on the arc

local function norm(v)
  local l = math.sqrt(v[1] * v[1] + v[2] * v[2] + v[3] * v[3])
  return l > 1e-6 and { v[1] / l, v[2] / l, v[3] / l } or { 0, 0, 1 }
end
local function cross(a, b) return { a[2] * b[3] - a[3] * b[2], a[3] * b[1] - a[1] * b[3], a[1] * b[2] - a[2] * b[1] } end
local function mul(a, s) return { a[1] * s, a[2] * s, a[3] * s } end

-- The rotation whose columns are the world images of the local X, Y and Z axes.
local function quatFromBasis(X, Y, Z)
  local m00, m01, m02 = X[1], Y[1], Z[1]
  local m10, m11, m12 = X[2], Y[2], Z[2]
  local m20, m21, m22 = X[3], Y[3], Z[3]
  local tr = m00 + m11 + m22
  local qx, qy, qz, qw
  if tr > 0 then
    local s = math.sqrt(tr + 1.0) * 2
    qw, qx, qy, qz = 0.25 * s, (m21 - m12) / s, (m02 - m20) / s, (m10 - m01) / s
  elseif m00 > m11 and m00 > m22 then
    local s = math.sqrt(1.0 + m00 - m11 - m22) * 2
    qw, qx, qy, qz = (m21 - m12) / s, 0.25 * s, (m01 + m10) / s, (m02 + m20) / s
  elseif m11 > m22 then
    local s = math.sqrt(1.0 + m11 - m00 - m22) * 2
    qw, qx, qy, qz = (m02 - m20) / s, (m01 + m10) / s, 0.25 * s, (m12 + m21) / s
  else
    local s = math.sqrt(1.0 + m22 - m00 - m11) * 2
    qw, qx, qy, qz = (m10 - m01) / s, (m02 + m20) / s, (m12 + m21) / s, 0.25 * s
  end
  return Quaternion.new(qx, qy, qz, qw)
end

-- A screen's frame given in the lobby's frame (X right, Y forward, Z up), in the world: its centre, right and up, and
-- its size.
local function place(c, r, u, w, h)
  local A = S.anchor
  local yaw = math.rad(A.yaw)
  local cs, sn = math.cos(yaw), math.sin(yaw)
  local function dir(v) return { cs * v[1] - sn * v[2], sn * v[1] + cs * v[2], v[3] } end
  local p = dir(c)
  return { c = { A.pos.x + p[1], A.pos.y + p[2], A.pos.z + p[3] }, r = dir(r), u = dir(u), w = w, h = h }
end

-- The panel's frame at a place on the arc (0 the middle, -1 the left, +1 coming in from the right): its centre, right
-- and up in the world and its size. In the lobby's frame X is right, Y forward, Z up; the panel faces the eyes.
-- A screen w x h m on the arc at bearing b (radians, from the player's heading), its centre at `height`.
local function frameOn(b, height, w, h)
  local H = S.spec
  local c = { H.dist * math.sin(b), H.dist * math.cos(b), height }
  local n = norm({ -c[1], -c[2], EYE - c[3] })
  local r = norm(cross({ 0, 0, 1 }, n))
  return place(c, r, cross(n, r), w, h)
end

local function frameAt(slot)
  local H = S.spec
  local step = math.deg((H.panel[1] + H.gap) / H.dist)
  return frameOn(math.rad(H.bearing + slot * step), H.height, H.panel[1], H.panel[2])
end

-- The preview window's place: right of the middle panel, a gap between, their top edges level.
local PREVIEW_SIZE = { 0.52, 0.40 }      -- lobby_layout.HOLO_PREVIEW
local function framePreview()
  local H = S.spec
  local w, h = PREVIEW_SIZE[1], PREVIEW_SIZE[2]
  return frameOn(math.rad(H.bearing) + (H.panel[1] / 2 + H.gap + w / 2) / H.dist, H.height + (H.panel[2] - h) / 2, w, h)
end

-- The plane entity: its local X = canvas LEFT, Y = canvas DOWN, Z = toward the viewer.
local function transformFor(F)
  local tr = WorldTransform.new()
  tr:SetPosition(Vector4.new(F.c[1], F.c[2], F.c[3], 1.0))
  tr:SetOrientation(quatFromBasis(mul(F.r, -1), mul(F.u, -1), cross(F.r, F.u)))
  return tr
end

---------------------------------------------------------------- feedback

local function play(name) Sound.menu(name) end

-- Hands: 1 left, 2 right (VRMenuPointer's 0 / 1), the same as the haptic mask (1 left, 2 right).
local function haptic(hand, kind)
  if type(VRHapticEvent) == "function" then pcall(VRHapticEvent, kind, hand == 2 and 2 or 1) end
end

local function capture(on)
  if S.captured == on or type(VRMenuCapture) ~= "function" then return end
  if pcall(VRMenuCapture, on) then S.captured = on end
end

---------------------------------------------------------------- a panel's canvas

local function rowsOf(P)
  local ok, rows = pcall(P.page.rows, P.page)
  if not ok then
    M.lastError = tostring(rows)
    return {}
  end
  return rows or {}
end

-- A place a press lands on: (x, y, w, h) in panel pixels, or with `list` in the scrolling list's content (its y from
-- the top of the list, as the rows lie before they are scrolled).
local function hit(P, x, y, w, h, row, part, list)
  P.hits[#P.hits + 1] = { x = x, y = y, w = w, h = h, row = row, part = part, list = list }
end

local BLACK = { 0, 0, 0 }
local IMAGE_ROW_H = 240                  -- a row with a picture (row.image): the picture 377 x 212, 16:9
local ICON_ROW_H = 190                   -- a row with an icon (row.icon): the icon 162 px square
local CARDS_ROW_H = 900                  -- a row of cards to choose from (row.cards): each a picture 680 px square
local POSTER_ROW_H = 1200                -- a row of poster cards (row.poster): the picture as wide as its card
local PICTURE_ROW_H = 560                -- a row with one picture in its middle (row.picture)
local DIMMED = { 0.62, 0.62, 0.62 }      -- a poster not chosen and not pointed at
-- A SAVE'S PICTURE, AS LIGHT. These screens are lit planes in the hall, seen through its exposure and its grading, and
-- the panels' colours are chosen as light for that (a text at 1.0 reads white). A save's screenshot drawn at 1.0 came
-- out burnt white: its dark dusk sky (121, 128, 138) at (249, 244, 231), its shadows 35 at 192. This tint takes the
-- picture back to the screenshot itself, to about 8 levels in 255 across its darks and lights, and takes off the
-- hall's warm cast (blue a little higher): fitted on the headset image against the save's screenshot.png, 2026-11-01.
local PHOTO = { 0.37, 0.39, 0.43 }

local function rowHeight(row)
  if row.image ~= nil then return IMAGE_ROW_H end
  if row.icon ~= nil then return ICON_ROW_H end
  if row.cards ~= nil then return row.poster and POSTER_ROW_H or CARDS_ROW_H end
  if row.picture ~= nil then return PICTURE_ROW_H end
  return ROW_H
end

-- A row's look for what a hand is over (part: nil, "main", "left" or "right"), changed in place: pointing does not
-- draw the page again, so a list being scrolled keeps every row it has.
local function restyle(look, part)
  if look == nil then return end
  local row = look.row
  pcall(function()
    if look.cards then
      for i, cl in ipairs(look.cards) do
        local hot = part == "card" .. i
        cl.field:SetTintColor(Ink.color(hot and RED_DIM or GLASS))
        cl.field:SetOpacity(hot and 1.0 or 0.6)
        for _, w in ipairs(cl.frame) do w:SetTintColor(Ink.color(hot and RED or (cl.card.on and CYAN or RED_DIM))) end
        if cl.poster then
          cl.picture:SetTintColor(Ink.color((hot or cl.card.on) and WHITE or DIMMED))
        else
          cl.picture:SetTintColor(Ink.color(hot and WHITE or (cl.card.on and CYAN or TEXT)))
        end
        cl.label:SetTintColor(Ink.color((hot or cl.card.on) and WHITE or TEXT))
      end
      return
    end
    if look.header then
      local hot = part ~= nil and row.go ~= nil
      look.label:SetTintColor(Ink.color(hot and WHITE or RED))
      if look.detail then look.detail:SetTintColor(Ink.color(hot and WHITE or RED)) end
      look.line:SetTintColor(Ink.color(hot and RED or RED_DIM))
      return
    end
    if look.button then
      local hot = part ~= nil
      look.field:SetTintColor(Ink.color(hot and RED or RED_DIM))
      look.field:SetOpacity(hot and 1.0 or 0.8)
      look.label:SetTintColor(Ink.color(hot and BLACK or WHITE))
      return
    end
    local hot = part ~= nil and look.live
    look.field:SetTintColor(Ink.color(hot and RED_DIM or GLASS))
    look.field:SetOpacity(hot and 1.0 or 0.6)
    for _, w in ipairs(look.frame) do w:SetVisible(hot or look.on) end
    look.line:SetVisible(not (hot or look.on))
    local ink = Ink.color(not look.live and DIM or ((part ~= nil or look.on) and WHITE or TEXT))
    if look.label ~= nil then look.label:SetTintColor(ink) end
    for _, w in ipairs(look.glyph or {}) do w:SetTintColor(ink) end
    if look.sign ~= nil then look.sign:SetTintColor(Ink.color(hot and WHITE or RED)) end
    for side, a in pairs(look.arrows) do
      local on = part == side
      a.box:SetTintColor(Ink.color(on and RED or RED_DIM))
      a.box:SetOpacity(on and 1.0 or 0.7)
      a.sign:SetTintColor(Ink.color(on and BLACK or WHITE))
    end
    if look.knob ~= nil then look.knob:SetTintColor(Ink.color((part == "slider" and look.live) and WHITE or TEXT)) end
  end)
end

-- The value a point along a slider's track stands for: snapped to the slider's step, within its bounds.
local function sliderValue(sl, frac)
  local lo, hi = math.min(sl.min, sl.max), math.max(sl.min, sl.max)
  local v = lo + math.max(0, math.min(1, frac)) * (hi - lo)
  if sl.step ~= nil and sl.step > 0 then v = lo + math.floor((v - lo) / sl.step + 0.5) * sl.step end
  return math.max(lo, math.min(hi, v))
end

-- The track filled up to value v, the knob on it, the value written beside it.
local function showSlider(look, v)
  local SLIDER_X, SLIDER_W = look.L.sliderX, look.L.sliderW
  local sl = look.row.slider
  local lo, hi = math.min(sl.min, sl.max), math.max(sl.min, sl.max)
  local frac = hi > lo and (v - lo) / (hi - lo) or 0
  frac = math.max(0, math.min(1, frac))
  pcall(function()
    look.fill:SetSize(Vector2.new({ X = SLIDER_W * frac, Y = 14 }))
    look.knob:SetMargin(inkMargin.new({ left = SLIDER_X + SLIDER_W * frac - 14, top = ROW_H / 2 - 32, right = 0,
                                        bottom = 0 }))
    look.value:SetText(sl.format ~= nil and sl.format(v) or tostring(v))
  end)
end

-- One row on canvas c at (0, 0): a section's header, a line of text, or the game's list item -- a faint field over a
-- hairline that lights up inside a frame cut at its corners when pointed at (the chosen one framed in cyan); a choice
-- has its label, < value > and the two arrows, each a press of its own; a slider (row.slider: min, max, step, value,
-- set(v), format(v)) its label, the track with its knob, and the value; a picture row (row.image: an image widget, the
-- row taller) the picture, its label and detail. Its presses are kept at hitY in the list.
local function drawRow(P, c, row, hitY)
  local L = P.L
  local ROW_X, ROW_W, SLIDER_X, SLIDER_W = L.rowX, L.rowW, L.sliderX, L.sliderW
  local id = row.key
  local look = { row = row, arrows = {}, L = L }
  if row.header then
    look.header = true
    look.label = Ink.text(c, 20, ROW_H * 0.35, ROW_W - 160, ROW_H * 0.65, row.label, 62, RED, "left")
    if row.detail ~= nil then
      look.detail = Ink.text(c, ROW_W - 140, ROW_H * 0.35, 120, ROW_H * 0.65, row.detail, 72, RED, "right", true)
    end
    look.line = Ink.rect(c, 20, ROW_H - 10, ROW_W - 40, 3, RED_DIM, 1.0)
    if row.go ~= nil then hit(P, ROW_X, hitY, ROW_W, ROW_H, id, "main", true) end
  elseif row.text then
    Ink.text(c, 20, 0, ROW_W - 40, ROW_H, row.label, 60, row.warn and RED or TEXT, "left", true)
    return
  elseif row.picture ~= nil then
    -- one picture (row.picture: { atlas, part }) in the row's middle, as tall as the row; nothing to press
    local s = PICTURE_ROW_H - 20
    Ink.image(c, (ROW_W - s) / 2, 10, s, s, row.picture[1], row.picture[2], TEXT, 1.0)
    return
  elseif row.cards ~= nil then
    -- cards side by side (row.cards: { key, picture = { atlas, part }, label, on, go }), each a press of its own (part
    -- "card1", "card2" ..): a field in its frame, the picture, the label under it; the chosen one in cyan. A square
    -- picture is a glyph, tinted in the panel's colours; row.poster (its width over its height) makes the pictures
    -- the game's own art in full colour, as wide as the card, the ones not chosen dimmed.
    look.cards = {}
    local gap = 60
    local rh = rowHeight(row)
    local cw = math.floor((ROW_W - gap * (#row.cards - 1)) / #row.cards)
    -- (a square picture no wider than its card: three cards to a row are narrower than the row is high)
    local pw = row.poster and math.min(cw - 60, math.floor((rh - 220) * row.poster)) or math.min(rh - 220, cw - 60)
    local ph = row.poster and math.floor(pw / row.poster) or pw
    for i, card in ipairs(row.cards) do
      local x = (i - 1) * (cw + gap)
      local tint = row.poster and (card.on and WHITE or DIMMED) or TEXT
      look.cards[i] = { card = card, poster = row.poster ~= nil, field = Ink.rect(c, x, 0, cw, rh, GLASS, 0.6),
                        frame = Ink.cutFrame(c, x, 0, cw, rh, 40, RED_DIM, 1.0, 3),
                        picture = Ink.image(c, x + (cw - pw) / 2, 30, pw, ph, card.picture[1], card.picture[2], tint, 1.0),
                        label = Ink.text(c, x, rh - 180, cw, 150, card.label,
                                         Ink.fit(card.label, cw - 60, 84, 50), TEXT, "centre") }
      hit(P, ROW_X + x, hitY, cw, rh, id, "card" .. i, true)
    end
  else
    look.live = row.enabled ~= false and (row.go ~= nil or row.left ~= nil or row.right ~= nil or row.slider ~= nil)
    look.on = row.on == true
    local rh = rowHeight(row)
    -- from its indent (row.indent px, a picture row under the one it belongs to) to the right of the list
    local ix = row.indent or 0
    local rw = ROW_W - ix
    look.field = Ink.rect(c, ix, 0, rw, rh, GLASS, 0.6)
    look.frame = Ink.cutFrame(c, ix, 0, rw, rh, 28, look.on and CYAN or RED, 1.0, 3)
    look.line = Ink.rect(c, ix, rh - 3, rw, 3, RED_DIM, 1.0)
    if row.image ~= nil or row.icon ~= nil then
      -- a picture row (a save) or an icon row (row.icon: { atlas, part }, a playthrough's life path): the picture in its
      -- 16:9 box or the icon in its square on the left, the label and the detail beside it, and its sign (row.sign: the
      -- + or - a playthrough opens and closes with) on the right
      local x, y, h = ix + 16, 14, rh - 28
      local w = row.image ~= nil and math.floor(h * 16 / 9) or h
      Ink.rect(c, x, y, w, h, BLACK, 0.8)
      if row.icon ~= nil then Ink.image(c, x + 10, y + 10, w - 20, h - 20, row.icon[1], row.icon[2], RED, 0.95) end
      if row.image ~= nil then pcall(function()
        local img = row.image
        img:SetAnchor(inkEAnchor.TopLeft)
        img:SetAnchorPoint(Vector2.new({ X = 0, Y = 0 }))
        img:SetMargin(inkMargin.new({ left = x, top = y, right = 0, bottom = 0 }))
        img:SetSize(Vector2.new({ X = w, Y = h }))
        img:SetTintColor(Ink.color(PHOTO))
        img:SetOpacity(1.0)
        img:SetVisible(true)
        img:Reparent(c)
      end) end
      local tx = x + w + 40
      local tw = ROW_W - tx - (row.sign ~= nil and 160 or 40)
      look.label = Ink.text(c, tx, 20, tw, rh * 0.45, row.label, Ink.fit(row.label, tw, 76, 50), TEXT, "left")
      if row.detail ~= nil then Ink.text(c, tx, rh * 0.5, tw, rh * 0.4, row.detail, 54, DIM, "left", true) end
      if row.sign ~= nil then look.sign = Ink.text(c, ROW_W - 140, 0, 110, rh, row.sign, 96, RED, "centre", true) end
      if look.live then hit(P, ROW_X + ix, hitY, rw, rh, id, "main", true) end
    elseif row.slider ~= nil then
      look.label = Ink.text(c, 60, 0, SLIDER_X - 100, ROW_H, row.label, Ink.fit(row.label, SLIDER_X - 100, 66, 46), TEXT,
                            "left")
      Ink.rect(c, SLIDER_X, ROW_H / 2 - 7, SLIDER_W, 14, RED_DIM, 1.0)
      look.fill = Ink.rect(c, SLIDER_X, ROW_H / 2 - 7, 0, 14, look.live and RED or DIM, 1.0)
      look.knob = Ink.rect(c, SLIDER_X - 14, ROW_H / 2 - 32, 28, 64, TEXT, look.live and 1.0 or 0.4)
      local vx = SLIDER_X + SLIDER_W + 40
      look.value = Ink.text(c, vx, 0, ROW_W - vx - 30, ROW_H, "", 64, WHITE, "centre")
      showSlider(look, row.slider.value)
      hit(P, ROW_X + SLIDER_X - 40, hitY, SLIDER_W + 80, ROW_H, id, "slider", true)
    elseif row.left ~= nil or row.right ~= nil then
      local aw, vw = 150, 640
      local vx = ROW_W - aw * 2 - vw
      look.label = Ink.text(c, 60, 0, vx - 80, ROW_H, row.label, Ink.fit(row.label, vx - 80, 66, 46), TEXT, "left")
      for i, side in ipairs({ "left", "right" }) do
        local ax = i == 1 and vx or vx + aw + vw
        look.arrows[side] = { box = Ink.rect(c, ax + 20, 22, aw - 40, ROW_H - 44, RED_DIM, 0.7),
                              sign = Ink.text(c, ax, 0, aw, ROW_H, i == 1 and "<" or ">", 96, WHITE, "centre") }
        hit(P, ROW_X + ax, hitY, aw, ROW_H, id, side, true)
      end
      local value = row.value or ""
      Ink.text(c, vx + aw, 0, vw, ROW_H, value, Ink.fit(value, vw - 30, 64, 40), look.on and CYAN or WHITE, "centre")
      hit(P, ROW_X, hitY, vx, ROW_H, id, "right", true)
    else
      look.label = Ink.text(c, 60, 0, rw - 160, ROW_H, row.label, 84, TEXT, "left")
      if row.detail ~= nil then Ink.text(c, 60, 0, rw - 120, ROW_H, row.detail, 54, DIM, "right", true) end
      if look.live then hit(P, ROW_X, hitY, rw, ROW_H, id, "main", true) end
    end
  end
  P.looks[id] = look
  restyle(look, (P.hover ~= nil and P.hover.row == id) and P.hover.part or nil)
end

-- Canvas pixels a metre on a panel.
local function pxPerMetre(P) return P.L.cw / P.frame.w end

local function tint(list, col, op)
  for _, w in ipairs(list) do
    w:SetTintColor(Ink.color(col))
    if op ~= nil then w:SetOpacity(op) end
  end
end

-- The pointers' marks (one a hand: both hands can point at one panel) and a press's ripple, the VR menu's (menu.lua
-- build.all, setCursor, updatePressFX): two rings and a dot, three rings; made once a panel over its page, moved,
-- scaled and tinted, never drawn again.
local function buildMark(P)
  P.marks = {}
  for hand = 1, 2 do
    local host = Ink.canvas(P.top, 0, 0, 64, 64)
    host:SetRenderTransformPivot(Vector2.new({ X = 0, Y = 0 }))
    host:SetOpacity(0.0)
    P.marks[hand] = { host = host, inner = Ink.ring(host, 32, 32, 17, 2, MARK, 0.9, 28),
                      outer = Ink.ring(host, 32, 32, 26, 1.5, MARK, 0.4, 32, 4),
                      dot = Ink.image(host, 29, 29, 6, 6, PHONE_ATLAS, "circle", MARK_DOT, 1.0), color = MARK }
  end
  local fx = Ink.canvas(P.top, 0, 0, 96, 96)
  fx:SetRenderTransformPivot(Vector2.new({ X = 0.5, Y = 0.5 }))
  fx:SetOpacity(0.0)
  Ink.ring(fx, 48, 48, 22, 3, MARK, 0.95, 32)
  Ink.ring(fx, 48, 48, 32, 2, MARK, 0.65, 36)
  Ink.ring(fx, 48, 48, 42, 1, MARK, 0.3, 40)
  P.ripple = { host = fx }
end

-- at: where the hand is on the panel (canvas px) and whether a pull would press something there; nil hides its mark.
local function showMark(P, hand, at)
  local m = P.marks and P.marks[hand]
  if m == nil then return end
  if at == nil then
    if m.shown then m.host:SetOpacity(0.0); m.shown = false end
    return
  end
  local span = MARK_M * pxPerMetre(P)
  m.host:SetMargin(inkMargin.new({ left = at.x - span / 2, top = at.y - span / 2, right = 0, bottom = 0 }))
  m.host:SetScale(Vector2.new({ X = span / 64, Y = span / 64 }))
  local col = at.hot and MARK_HOT or MARK
  if m.color ~= col then
    tint(m.inner, col, 0.95)
    tint(m.outer, col, 0.45)
    m.color = col
  end
  if not m.shown then m.host:SetOpacity(1.0); m.shown = true end
end

local function ripple(P, x, y)
  if P.ripple ~= nil then P.ripple.x, P.ripple.y, P.ripple.at = x, y, S.time end
end

local function updateRipple(P)
  local r = P.ripple
  if r == nil or r.at == nil then return end
  local t = (S.time - r.at) / RIPPLE
  if t >= 1 then
    r.at = nil
    r.host:SetOpacity(0.0)
    return
  end
  local scale = (0.65 + 0.6 * t) * MARK_M * pxPerMetre(P) / 52   -- as the VR menu's: 96 px rings to a 52 px mark
  r.host:SetMargin(inkMargin.new({ left = r.x - 48, top = r.y - 48, right = 0, bottom = 0 }))
  r.host:SetScale(Vector2.new({ X = scale, Y = scale }))
  r.host:SetOpacity((1 - t) ^ 2)
end

-- The glass and its frame, as the game's screens have them: a faint field in a frame cut at its top left and bottom
-- right corners, a hairline inside it, short bright bars on the top and bottom edges. The field reaches the cuts: each
-- cut corner's triangle is filled in steps, under the frame's 4 px line, which covers their edge. Right of the
-- layout's clearX (the new game's window: right of its rows, where the figure is seen) the field is nearly clear.
local CLEAR_GLASS = 0.12
local function buildGlass(layer, L)
  local x0, y0, w0, h0 = PAD, PAD, L.cw - 2 * PAD, L.ch - 2 * PAD
  local step = 2
  local function field(x, y, w, h)
    local cx = L.clearX or (x + w)
    if cx > x then Ink.rect(layer, x, y, math.min(w, cx - x), h, GLASS, 0.55) end
    if cx < x + w then
      local from = math.max(x, cx)
      Ink.rect(layer, from, y, x + w - from, h, GLASS, CLEAR_GLASS)
    end
  end
  field(x0 + CUT, y0, w0 - CUT, CUT)
  field(x0, y0 + CUT, w0, h0 - 2 * CUT)
  field(x0, y0 + h0 - CUT, w0 - CUT, CUT)
  for d = 0, CUT - step, step do
    -- the row d px into the cut: from the diagonal to the strip beside it
    field(x0 + CUT - d - step, y0 + d, d + step, step)
    field(x0 + w0 - CUT, y0 + h0 - CUT + d, CUT - d, step)
  end
  Ink.cutFrame(layer, x0, y0, w0, h0, CUT, RED, 1.0, 4)
  Ink.cutFrame(layer, x0 + 16, y0 + 16, w0 - 32, h0 - 32, CUT - 7, RED_DIM, 1.0, 2)
  Ink.rect(layer, x0 + CUT + 40, y0 - 5, 460, 12, RED, 1.0)
  Ink.rect(layer, x0 + w0 - CUT - 500, y0 + h0 - 7, 460, 12, RED, 1.0)
end

-- The scrolling list: the VR menu's (its modules/scroll.lua, through its mod) -- dragged with a fingertip pushed into
-- the panel or a pointer with its trigger held, momentum when let go, rows fading at its edges, clipped to it -- in
-- the panel's colours. Between the header and the footer, its scroll bar right of the rows.
local SCROLL_STYLE = { track = RED_DIM, thumb = RED, held = WHITE }
local DRAG = 28                          -- px a held press moves before it scrolls instead of pressing

local function scrollKit()
  local V = nil
  pcall(function() V = GetMod("CyberpunkVRPort_Menu").scrollKit end)
  return V
end

-- The page, drawn again when what it shows changes, between the glass and the mark, which are made once.
local function build(P)
  local L = P.L
  local CW, CH, ROW_X, ROW_W, ROW_TOP = L.cw, L.ch, L.rowX, L.rowW, L.rowTop
  local FOOT_Y, LIST_W, LIST_H = L.footY, L.listW, L.listH
  if P.layer == nil then
    P.glass = Ink.canvas(P.root, 0, 0, CW, CH)
    buildGlass(P.glass, L)
    P.layer = Ink.canvas(P.root, 0, 0, CW, CH)
    P.top = Ink.canvas(P.root, 0, 0, CW, CH)
    buildMark(P)
  end
  local layer = P.layer
  P.builds = (P.builds or 0) + 1
  layer:RemoveAllChildren()
  P.hits, P.looks = {}, {}
  local page = P.page
  if page.logo then
    Ink.image(layer, ROW_X - 10, HEAD_Y + 30, LOGO_W, LOGO_H, LOGO_ATLAS, LOGO_PARTS[Ink.language()] or "cp_logo", RED,
              1.0)
  else
    Ink.text(layer, ROW_X, HEAD_Y + 30, ROW_W, 150, page.title or "", 118, WHITE, "left")
    if page.subtitle ~= nil then
      Ink.text(layer, ROW_X, HEAD_Y + 150, ROW_W, 90, page.subtitle, 58, TEXT, "left", true)
    end
  end
  Ink.text(layer, ROW_X, HEAD_Y + 30, ROW_W, 70, page.code or "VR//LOBBY", 40, DIM, "right")
  Ink.rect(layer, ROW_X, HEAD_Y + HEAD_H + 10, ROW_W, 4, RED, 0.8)
  -- The rows, in a list that scrolls when there are more than it shows; drawn again where it was scrolled to.
  local rows = rowsOf(P)
  local V = scrollKit()
  -- where the list was scrolled to: this page's, as it was drawn last or as it was left
  local keep = (P.view ~= nil and P.builtPage == page) and P.view.offset or page.scroll or 0
  P.builtPage = page
  P.view = nil
  if V ~= nil then
    local v = V.new(layer, ROW_X, ROW_TOP, ROW_X, ROW_TOP, LIST_W, LIST_H, SCROLL_STYLE)
    for _, row in ipairs(rows) do
      v:add(rowHeight(row) + ROW_GAP, function(c, it) drawRow(P, c, row, it.y) end)
    end
    v:scrollTo(keep)
    v:update(0, #rows)
    P.view = v
  else
    -- without the VR menu's mod, as many as the list shows
    for i, row in ipairs(rows) do
      local y = (i - 1) * (ROW_H + ROW_GAP)
      if y + ROW_H > LIST_H then break end
      drawRow(P, Ink.canvas(layer, ROW_X, ROW_TOP + y, ROW_W, ROW_H), row, y)
    end
  end
  -- The footer: BACK on the left (not on the first page).
  if P.index ~= nil and P.index > 1 then
    local look = { row = { key = "__back" }, button = true }
    look.field = Ink.rect(layer, ROW_X, FOOT_Y, 620, FOOT_H, RED_DIM, 0.8)
    look.label = Ink.text(layer, ROW_X, FOOT_Y, 620, FOOT_H, "< " .. S.texts.back, 72, WHITE, "centre")
    hit(P, ROW_X, FOOT_Y, 620, FOOT_H, "__back", "main")
    P.looks.__back = look
    restyle(look, (P.hover ~= nil and P.hover.row == "__back") and "main" or nil)
  end
  -- The page's own buttons in the footer from its right end, drawn as its rows are -- a faint field over a hairline,
  -- framed and lit when pointed at -- an icon where a row has its label.
  P.footer = {}
  local footer = page.footer
  if type(footer) == "function" then footer = footer() end
  local fx = ROW_X + ROW_W
  for _, b in ipairs(footer or {}) do
    local key = "__foot:" .. b.key
    fx = fx - FOOT_BUTTON_W
    local look = { row = { key = key }, arrows = {}, live = true }
    look.field = Ink.rect(layer, fx, FOOT_Y, FOOT_BUTTON_W, FOOT_H, GLASS, 0.6)
    look.frame = Ink.cutFrame(layer, fx, FOOT_Y, FOOT_BUTTON_W, FOOT_H, 28, RED, 1.0, 3)
    look.line = Ink.rect(layer, fx, FOOT_Y + FOOT_H - 3, FOOT_BUTTON_W, 3, RED_DIM, 1.0)
    look.glyph = b.glyph(layer, fx + (FOOT_BUTTON_W - FOOT_ICON_H) / 2, FOOT_Y + (FOOT_H - FOOT_ICON_H) / 2,
                         FOOT_ICON_H, TEXT) or {}
    hit(P, fx, FOOT_Y, FOOT_BUTTON_W, FOOT_H, key, "main")
    P.looks[key], P.footer[key] = look, b
    restyle(look, (P.hover ~= nil and P.hover.row == key) and "main" or nil)
    fx = fx - 30
  end
  P.rowsCache = rows
  P.dirty = false
end

local function hitAt(P, x, y)
  -- where (x, y) is in the list's content, when it is on a row the list shows
  local cy = nil
  local v = P.view
  if v ~= nil then
    if v:visibleAt(x, y) then cy = y - v.y + v.offset end
  elseif y >= P.L.rowTop and y <= P.L.rowTop + P.L.listH then
    cy = y - P.L.rowTop
  end
  for i = #P.hits, 1, -1 do
    local h = P.hits[i]
    local hy = y
    if h.list then hy = cy end
    if hy ~= nil and x >= h.x and x <= h.x + h.w and hy >= h.y and hy <= h.y + h.h then return h end
  end
  return nil
end

local function rowByKey(P, key)
  for _, r in ipairs(P.rowsCache or {}) do
    if r.key == key then return r end
  end
  return nil
end

---------------------------------------------------------------- panels in the world

local function spawnPanel(page, slot)
  local P = { page = page, slot = slot, alpha = 0.0, hits = {}, dirty = true, L = PANEL }
  P.frame = frameAt(slot)
  P.id = Spawner.spawn(ENT, transformFor(P.frame), TAG)
  return P
end

local function resolve(P)
  if P.id == nil then return false end
  if P.root ~= nil then return true end
  pcall(function()
    P.entity = Game.FindEntityByID(P.id)
    if P.entity ~= nil then P.comp = P.entity:FindComponentByName(CName.new(UI)) end
    if P.comp ~= nil then P.root = P.comp:GetWidget() end
  end)
  return P.root ~= nil
end

local function ready(P)
  if P.ready then return true end
  if type(VRMenuSurfaceReady) ~= "function" then return false end
  local ok, done = pcall(VRMenuSurfaceReady, P.comp, P.root)
  if not ok or not done then return false end
  pcall(function()
    P.entity:FindComponentByName(CName.new("display")):Toggle(true)
    P.ready = true
  end)
  return P.ready == true
end

local function despawn(P) Spawner.delete(P.id) end

-- Where each panel on the stack goes: the top one to the middle, the one under it to the left, the rest away. The new
-- game's window on top stays where it is, and the panels under it stay too, faded once it is there to grow.
local function retarget()
  local n = #S.stack
  local window = S.stack[n] ~= nil and S.stack[n].window
  local panels = window and n - 1 or n
  local hidden = window and S.stack[n].ready
  for i, P in ipairs(S.stack) do
    P.index = i
    if P.window then
      P.target, P.alphaTarget = 0, 1.0
    else
      P.target = i - panels
      P.alphaTarget = (not hidden and (i == panels or i == panels - 1)) and 1.0 or 0.0
    end
    P.dirty = true
  end
end

local function approach(v, target, rate, dt) return v + (target - v) * (1 - math.exp(-rate * dt)) end

local function animate(P, dt)
  local moved = false
  if math.abs(P.slot - P.target) > 0.001 then
    P.slot = approach(P.slot, P.target, SLIDE, dt)
    if math.abs(P.slot - P.target) < 0.002 then P.slot = P.target end
    moved = true
  end
  P.alpha = approach(P.alpha, P.alphaTarget, FADE, dt)
  if moved or P.placed == nil then
    P.frame = frameAt(P.slot)
    if P.entity ~= nil then
      pcall(function() Game.VREntitySetWorldTransform(P.entity, transformFor(P.frame)) end)
      P.placed = true
    end
  end
  if P.root ~= nil and P.shownAlpha ~= P.alpha then
    pcall(function() P.root:SetOpacity(P.alpha) end)
    P.shownAlpha = P.alpha
  end
end

---------------------------------------------------------------- the preview window

-- A window of its own right of the middle panel: the picture of what the top page's pointed row stands for
-- (row.preview, an image widget; row.previewText and row.previewDetail under it), fading in while that row is pointed
-- at and out when it is not. Its canvas is the panels' CW x CH on a smaller screen (lobby_layout.HOLO_PREVIEW, the
-- same shape), so its texts are drawn larger to read as the panels' do. Nothing on it is pressed.
local PREVIEW_ENT = "vrport\\ui\\vr_lobby_preview.ent"
local PV_X, PV_Y = 130, 130
local PV_W = CW - 2 * PV_X
local PV_H = math.floor(PV_W * 9 / 16)   -- the saves' pictures are 16:9

local function spawnPreview()
  local pv = { alpha = 0.0, alphaTarget = 0.0 }
  pv.frame = framePreview()
  pv.id = Spawner.spawn(PREVIEW_ENT, transformFor(pv.frame), TAG)
  return pv
end

-- Everything on a body canvas, the one that fades.
local function buildPreview(pv)
  local body = Ink.canvas(pv.root, 0, 0, CW, CH)
  buildGlass(Ink.canvas(body, 0, 0, CW, CH), PANEL)
  Ink.rect(body, PV_X, PV_Y, PV_W, PV_H, BLACK, 0.8)
  pv.host = Ink.canvas(body, PV_X + 4, PV_Y + 4, PV_W - 8, PV_H - 8)
  Ink.cutFrame(body, PV_X, PV_Y, PV_W, PV_H, 48, RED, 1.0, 4)
  pv.title = Ink.text(body, PV_X, PV_Y + PV_H + 50, PV_W, 190, "", 140, WHITE, "left")
  pv.detail = Ink.text(body, PV_X, PV_Y + PV_H + 250, PV_W, 140, "", 100, TEXT, "left", true)
  pv.body = body
  pv.built = true
end

-- The window's surface is ready only after a real paint, and a hidden canvas paints nothing: so it is painted once
-- with its glass showing while its display is still off, then hidden, and the display is switched on a moment later,
-- the hidden canvas painted over the glass by then (the lobby's screens are painted every frame).
local PV_SETTLE = 0.25                   -- s
local function previewReady(pv)
  if pv.ready then return true end
  if pv.paintedAt == nil then
    if type(VRMenuSurfaceReady) ~= "function" then return false end
    local ok, done = pcall(VRMenuSurfaceReady, pv.comp, pv.root)
    if not ok or not done then return false end
    pv.paintedAt = S.time
    pcall(function() pv.body:SetOpacity(0.0) end)
    pv.shownAlpha = 0.0
    return false
  end
  if S.time - pv.paintedAt < PV_SETTLE then return false end
  pcall(function()
    pv.entity:FindComponentByName(CName.new("display")):Toggle(true)
    pv.ready = true
  end)
  return pv.ready == true
end

-- The row's picture and texts into the window (the picture moved from wherever it was last drawn).
local function fillPreview(pv, row)
  pcall(function()
    if pv.img ~= row.preview then
      pv.host:RemoveAllChildren()
      local img = row.preview
      img:SetAnchor(inkEAnchor.TopLeft)
      img:SetAnchorPoint(Vector2.new({ X = 0, Y = 0 }))
      img:SetMargin(inkMargin.new({ left = 0, top = 0, right = 0, bottom = 0 }))
      img:SetSize(Vector2.new({ X = PV_W - 8, Y = PV_H - 8 }))
      img:SetTintColor(Ink.color(PHOTO))
      img:SetOpacity(1.0)
      img:SetVisible(true)
      img:Reparent(pv.host)
      pv.img = img
    end
    pv.title:SetText(row.previewText or "")
    pv.detail:SetText(row.previewDetail or "")
  end)
  pv.row = row
end

local function updatePreview(dt)
  local pv = S.preview
  if pv == nil or not resolve(pv) then return end
  if not pv.built then pcall(buildPreview, pv) end
  if not pv.built or not previewReady(pv) then return end
  local P = S.stack[#S.stack]
  local row = P ~= nil and P.hover ~= nil and rowByKey(P, P.hover.row) or nil
  if row ~= nil and row.preview == nil then row = nil end
  if row ~= nil and pv.row ~= row then fillPreview(pv, row) end
  pv.alphaTarget = row ~= nil and 1.0 or 0.0
  pv.alpha = approach(pv.alpha, pv.alphaTarget, FADE, dt)
  if pv.alpha < 0.01 and pv.alphaTarget == 0 then pv.alpha = 0.0 end
  if pv.shownAlpha ~= pv.alpha then
    pcall(function() pv.body:SetOpacity(pv.alpha) end)
    pv.shownAlpha = pv.alpha
  end
end

---------------------------------------------------------------- the new game's window

-- THE NEW GAME'S WINDOW (M.window). The middle panel grows into it: a screen of its own (vrport\ui\vr_lobby_window.ent,
-- lobby_layout.HOLO_WINDOW, a canvas of HOLO_WINDOW_CANVAS px) a little larger than V at full height, upright at the
-- scene's holo.window place, its rows down its left and room on its right for the new game's figure, who stands
-- behind it and is seen through it (M.windowSpot), or steps out in front of it clear of the rows (M.windowRowsEdge)
-- to show the part a row is about. It comes up where the panel is, its glass drawn to the panel's
-- size, and over GROW s moves to its place as its glass grows to the whole window and the panels fade; its rows show
-- as it gets there. The pages opened from it are drawn in it and BACK goes back through them; from its first page it
-- shrinks back into the panel the same way.
local WINDOW_ENT = "vrport\\ui\\vr_lobby_window.ent"
local WINDOW = layout(3190, 2900, 1650)  -- lobby_layout.HOLO_WINDOW_CANVAS; the rows 1.14 m wide
WINDOW.clearX = WINDOW.rowX + WINDOW.listW + 60
local GROW = 0.8                         -- s
local WINDOW_SETTLE = 0.25               -- s: painted hidden before its display is on, as the preview window

local function smooth(x)
  x = math.max(0, math.min(1, x))
  return x * x * (3 - 2 * x)
end

local function lerp3(a, b, k) return { a[1] + (b[1] - a[1]) * k, a[2] + (b[2] - a[2]) * k, a[3] + (b[3] - a[3]) * k } end

-- The window's frame at its place: upright, facing the player's spot.
local function frameWindow()
  local H = S.spec.window
  local b = math.rad(H.bearing)
  return place({ H.dist * math.sin(b), H.dist * math.cos(b), H.height }, { math.cos(b), -math.sin(b), 0 }, { 0, 0, 1 },
               H.size[1], H.size[2])
end

-- Part of the way from one frame to another: the centre along the line between, the axes turned between.
local function between(F0, F1, k)
  local r = norm(lerp3(F0.r, F1.r, k))
  local u = lerp3(F0.u, F1.u, k)
  local d = u[1] * r[1] + u[2] * r[2] + u[3] * r[3]
  u = norm({ u[1] - r[1] * d, u[2] - r[2] * d, u[3] - r[3] * d })
  return { c = lerp3(F0.c, F1.c, k), r = r, u = u, w = F1.w, h = F1.h }
end

-- The glass's scale on the window grown part k of the way: from the panel's glass to its own.
local function glassScale(W, k)
  local L = W.L
  local sx = (W.panelW * (PANEL.cw - 2 * PAD) / PANEL.cw) / (W.frame.w * (L.cw - 2 * PAD) / L.cw)
  local sy = (W.panelH * (PANEL.ch - 2 * PAD) / PANEL.ch) / (W.frame.h * (L.ch - 2 * PAD) / L.ch)
  return sx + (1 - sx) * k, sy + (1 - sy) * k
end

local function spawnWindow(page, from)
  local W = { window = true, L = WINDOW, pages = { page }, page = page, slot = 0, target = 0, alpha = 1.0,
              alphaTarget = 1.0, hits = {}, dirty = true, grow = 0.0, opening = true, panelW = from.w, panelH = from.h }
  W.to = frameWindow()
  W.from = { c = from.c, r = from.r, u = from.u, w = W.to.w, h = W.to.h }
  W.frame = W.from
  W.id = Spawner.spawn(WINDOW_ENT, transformFor(W.frame), TAG)
  return W
end

-- Painted once with its glass showing while its display is still off, then hidden, and the display switched on a
-- moment later: from there it grows (see the preview window's).
local function windowReady(W)
  if W.ready then return true end
  if W.paintedAt == nil then
    if type(VRMenuSurfaceReady) ~= "function" then return false end
    local ok, done = pcall(VRMenuSurfaceReady, W.comp, W.root)
    if not ok or not done then return false end
    W.paintedAt = S.time
    pcall(function() W.root:SetOpacity(0.0) end)
    return false
  end
  if S.time - W.paintedAt < WINDOW_SETTLE then return false end
  pcall(function()
    W.entity:FindComponentByName(CName.new("display")):Toggle(true)
    W.ready = true
  end)
  if W.ready and S.stack[#S.stack] == W then retarget() end
  return W.ready == true
end

local function animateWindow(W, dt)
  if W.ready then W.grow = math.max(0, math.min(1, W.grow + (W.opening and dt or -dt) / GROW)) end
  local k = smooth(W.grow)
  if W.placedAt ~= W.grow and W.entity ~= nil then
    W.frame = between(W.from, W.to, k)
    pcall(function() Game.VREntitySetWorldTransform(W.entity, transformFor(W.frame)) end)
    W.placedAt = W.grow
  end
  if W.glass ~= nil and W.scaledAt ~= W.grow then
    local sx, sy = glassScale(W, k)
    pcall(function()
      W.glass:SetRenderTransformPivot(Vector2.new({ X = 0.5, Y = 0.5 }))
      W.glass:SetScale(Vector2.new({ X = sx, Y = sy }))
    end)
    W.scaledAt = W.grow
  end
  if W.ready and W.root ~= nil and W.shownAt ~= W.grow then
    -- it comes up over the panel as the panel fades, its rows once it has nearly grown
    pcall(function()
      W.root:SetOpacity(smooth(W.grow / 0.25))
      W.layer:SetOpacity(smooth((W.grow - 0.7) / 0.3))
    end)
    W.shownAt = W.grow
  end
end

---------------------------------------------------------------- hands

-- Each hand's touch on a panel this frame: pixels, and whether it is a fingertip or the pointer (with the ray's ends
-- in the world, for its beam).
local function touches(P)
  local F = P.frame
  local CW, CH = P.L.cw, P.L.ch
  local out = {}
  -- Made once per frame table (frameAt builds a new one when the panel moves): a panel at rest makes no new
  -- userdata here every frame.
  F.v = F.v or { Vector4.new(F.c[1], F.c[2], F.c[3], 1.0), Vector4.new(F.r[1], F.r[2], F.r[3], 0),
                 Vector4.new(F.u[1], F.u[2], F.u[3], 0), Vector4.new(F.w, F.h, 0, 0) }
  local ok, hits = pcall(VRMenuPointer, F.v[1], F.v[2], F.v[3], F.v[4])
  if not ok or hits == nil or #hits < 8 then return out end
  for hand = 1, 2 do
    local tip, ray, origin, at = hits[4 * hand - 3], hits[4 * hand - 2], hits[4 * hand - 1], hits[4 * hand]
    local on = function(p) return p.x >= 0 and p.x <= 1 and p.y >= 0 and p.y <= 1 end
    if tip.w > 0.5 and on(tip) and tip.z > -0.03 and tip.z < HOVER_TIP then
      out[hand] = { x = tip.x * CW, y = tip.y * CH, tip = true, depth = tip.z }
    elseif ray.w > 0.5 and on(ray) then
      out[hand] = { x = ray.x * CW, y = ray.y * CH, tip = false, trigger = origin.w, len = ray.z,
                    origin = { origin.x, origin.y, origin.z }, hit = { at.x, at.y, at.z } }
    end
  end
  return out
end

local function press(P, h, hand)
  if h == nil then return end
  haptic(hand, HAPTIC_CLICK)
  play("click")
  if P.index ~= #S.stack then
    M.backTo(P.index)
    return
  end
  if h.row == "__back" then M.back(); return end
  local button = P.footer ~= nil and P.footer[h.row] or nil
  if button ~= nil then
    if button.go ~= nil then
      local ok, err = pcall(button.go, button, hand)
      if not ok then M.lastError = tostring(err) end
    end
    for _, Q in ipairs(S.stack) do Q.dirty = true end
    return
  end
  local row = rowByKey(P, h.row)
  if row == nil or row.enabled == false then return end
  local card = row.cards and row.cards[tonumber(string.match(h.part or "", "^card(%d+)$") or "")]
  local fn = (card and card.go) or (h.part == "left" and row.left) or (h.part == "right" and (row.right or row.go)) or
             row.go
  if fn ~= nil then
    local ok, err = pcall(fn, row, hand)
    if not ok then M.lastError = tostring(err) end
  end
  for _, Q in ipairs(S.stack) do Q.dirty = true end
end

-- A slider held at panel x: its knob there at once, its value sent at most ten times a second while it moves and once
-- more as it is let go (final), the way the VR menu's sliders send theirs.
local function slide(hand, d, x, final)
  local look = d.slider
  local sl = look.row.slider
  local v = sliderValue(sl, (x - look.L.rowX - look.L.sliderX) / look.L.sliderW)
  showSlider(look, v)
  if (v ~= d.sent and S.time - (d.sentAt or -1) >= 0.1) or (final and v ~= d.sent) then
    d.sent, d.sentAt = v, S.time
    local ok, err = pcall(sl.set, v)
    if not ok then M.lastError = tostring(err) end
  end
  if final then
    haptic(hand, HAPTIC_CLICK)
    for _, Q in ipairs(S.stack) do Q.dirty = true end
  end
end

-- A press let go: the press it was waiting to be, the end of a slider's move, or the end of a drag (the list flies
-- on); pressed is false when the hand left the panels with it held.
local function release(hand, pressed)
  local d = S.hands[hand].down
  S.hands[hand].down = nil
  if d == nil or d.done then return end
  if d.dragging then
    if d.P.view ~= nil then d.P.view:endDrag() end
  elseif d.slider ~= nil then
    if d.sliding or pressed then slide(hand, d, d.sliding and d.x or d.x0, true) end
  elseif pressed and d.h ~= nil then
    ripple(d.P, d.x, d.y)
    press(d.P, d.h, hand)
  end
end

-- A hand holds panel P's list or one of its sliders: the page is not drawn again under it.
local function held(P)
  for hand = 1, 2 do
    local d = S.hands[hand].down
    if d ~= nil and d.P == P and (d.dragging or d.sliding) then return true end
  end
  return false
end

-- A hand's fingertip pushed in, or its trigger pulled, on panel P at t (h: what is there). On a list that scrolls a
-- press waits to see what it is, as the VR menu's lists do: let go where it began, it presses what it began on; moved
-- up or down, it drags the list; on a slider, moved sideways, it moves the slider (let go unmoved, the slider takes the
-- value it was pressed at). Anywhere else it presses at once, and a slider follows it from the start.
local function gesture(hand, P, t, h)
  local H = S.hands[hand]
  local d = H.down
  local engaged
  if t.tip then
    engaged = t.depth <= TOUCH
  else
    engaged = (d ~= nil and t.trigger > LET_GO) or t.trigger >= PULL
  end
  if not engaged then
    if d ~= nil then release(hand, true) end
    return
  end
  if d == nil then
    local v = P.view
    local inList = v ~= nil and v:scrollable() and v:contains(t.x, t.y)
    local look = h ~= nil and h.part == "slider" and P.looks[h.row] or nil
    if look ~= nil and not look.live then look = nil end
    if look ~= nil then
      H.down = { P = P, h = h, x = t.x, y = t.y, x0 = t.x, y0 = t.y, slider = look, sliding = not inList }
      if not inList then slide(hand, H.down, t.x, false) end
    elseif inList then
      H.down = { P = P, h = h, x = t.x, y = t.y, x0 = t.x, y0 = t.y }
    else
      if h ~= nil then
        ripple(P, t.x, t.y)
        press(P, h, hand)
      end
      H.down = { P = P, done = true }
    end
    return
  end
  if d.done or d.P ~= P then return end
  d.x, d.y = t.x, t.y
  local dx, dy = math.abs(t.x - d.x0), math.abs(t.y - d.y0)
  if d.sliding then
    slide(hand, d, t.x, false)
  elseif d.dragging then
    if P.view ~= nil then P.view:dragTo(t.y, S.time) end
  elseif d.slider ~= nil and dx > DRAG and dx >= dy then
    d.sliding = true
    slide(hand, d, t.x, false)
  elseif dy > DRAG and P.view ~= nil then
    d.dragging = true
    P.view:beginDrag(d.y0)
    P.view:dragTo(t.y, S.time)
  end
end

local function pointers()
  local seen = {}
  for _, P in ipairs(S.stack) do
    if P.ready and P.alpha > 0.4 and (not P.window or P.grow >= 1) then
      for hand, t in pairs(touches(P)) do
        local best = seen[hand]
        -- A fingertip on a panel wins over a pointer; between pointers, the panel nearer the middle.
        if best == nil or (t.tip and not best.t.tip) or (t.tip == best.t.tip and math.abs(P.slot) < math.abs(best.P.slot)) then
          seen[hand] = { P = P, t = t }
        end
      end
    end
  end
  local hovers = {}
  for hand = 1, 2 do
    local s, H = seen[hand], S.hands[hand]
    if s ~= nil then
      local P, t = s.P, s.t
      local h = hitAt(P, t.x, t.y)
      gesture(hand, P, t, h)
      -- a list being dragged has nothing under the hand to press
      if H.down ~= nil and H.down.dragging then h = nil end
      hovers[P] = hovers[P] or h
      local over = h and (h.row .. "/" .. h.part) or nil
      if over ~= nil and H.over ~= over then
        haptic(hand, HAPTIC_HOVER)
        play("hover")
      end
      H.over = over
      H.at = { P = P, x = t.x, y = t.y, len = t.len or 0, hot = h ~= nil }
    else
      if H.down ~= nil then release(hand, false) end
      H.over, H.at = nil, nil
    end
  end
  capture(seen[1] ~= nil or seen[2] ~= nil)
  -- The beams, from each pointing hand to the panel it points at, turned to the eyes.
  local eye = nil
  pcall(function()
    local v = VRViewWorldPos()
    eye = { v.x, v.y, v.z }
  end)
  for hand = 1, 2 do
    local t = seen[hand] and seen[hand].t
    Beams.set(hand, eye ~= nil and t ~= nil and not t.tip, t and t.origin, t and t.hit, eye)
  end
  for _, P in ipairs(S.stack) do
    local h = hovers[P]
    local was = P.hover
    if (h == nil) ~= (was == nil) or (h ~= nil and (h.row ~= was.row or h.part ~= was.part)) then
      P.hover = h
      if P.looks ~= nil then
        if was ~= nil then restyle(P.looks[was.row], nil) end
        if h ~= nil then restyle(P.looks[h.row], h.part) end
      end
    end
  end
  for _, P in ipairs(S.stack) do
    for hand = 1, 2 do
      local at = S.hands[hand].at
      pcall(showMark, P, hand, (at ~= nil and at.P == P) and at or nil)
    end
    pcall(updateRipple, P)
  end
end

---------------------------------------------------------------- the lobby's calls

-- anchor: the lobby's frame (pos, yaw in degrees); spec: the scene's holo frame; page: the first page.
function M.open(anchor, spec, page, texts)
  M.close()
  if anchor == nil or spec == nil or page == nil then return end
  S.anchor, S.spec = anchor, spec
  if texts ~= nil then S.texts = texts end
  S.stack = { spawnPanel(page, 0) }
  S.preview = spawnPreview()
  Beams.open(S.stack[1].frame.c, BEAM)
  retarget()
  play("appear")
end

function M.isOpen() return S.anchor ~= nil end

-- Per hand (1 left, 2 right), how far along its pointer a panel takes it this frame (0 for a fingertip on one), or
-- nil: nothing behind a panel is the hand's to point at.
function M.pointing()
  return { S.hands[1].at and S.hands[1].at.len, S.hands[2].at and S.hands[2].at.len }
end

-- A new page from the right into the middle; on the new game's window, in it.
function M.push(page)
  if S.anchor == nil then return end
  page.scroll = 0
  local top = S.stack[#S.stack]
  if top ~= nil and top.window then
    top.page.scroll = top.view ~= nil and top.view.offset or 0
    top.pages[#top.pages + 1] = page
    top.page, top.dirty = page, true
    play("open")
    return
  end
  local P = spawnPanel(page, 1)
  table.insert(S.stack, P)
  retarget()
  play("open")
end

-- Back to the page under the top one: on the new game's window, the one opened before in it, or from its first page
-- out of it.
function M.back()
  local top = S.stack[#S.stack]
  if top ~= nil and top.window and #top.pages > 1 then
    local page = table.remove(top.pages)
    if page.onBack ~= nil then pcall(page.onBack, page) end
    top.page, top.dirty = top.pages[#top.pages], true
    play("close")
    return
  end
  if #S.stack <= 1 then return end
  M.backTo(#S.stack - 1)
end

-- Back to the page at `index` on the stack: the ones over it slide out to the right; the new game's window shrinks
-- back into the panel it grew from.
function M.backTo(index)
  while #S.stack > index do
    local P = table.remove(S.stack)
    for hand = 1, 2 do pcall(showMark, P, hand, nil) end
    if P.window then
      for i = #P.pages, 1, -1 do
        if P.pages[i].onBack ~= nil then pcall(P.pages[i].onBack, P.pages[i]) end
      end
      P.opening = false
      if S.closing ~= nil then despawn(S.closing) end
      S.closing = P
    else
      P.target, P.alphaTarget = 1, 0.0
      if P.page.onBack ~= nil then pcall(P.page.onBack, P.page) end
      table.insert(S.leaving, P)
    end
  end
  retarget()
  play("close")
end

-- The new game's window grows out of the middle panel, `page` its first page; false when the scene has no place for it.
function M.window(page)
  local top = S.stack[#S.stack]
  if S.anchor == nil or S.spec.window == nil or top == nil or top.window then return false end
  page.scroll = 0
  table.insert(S.stack, spawnWindow(page, top.frame))
  retarget()
  play("open")
  return true
end

-- The new game's window is up and has grown.
function M.windowOpen()
  local top = S.stack[#S.stack]
  return top ~= nil and top.window == true and top.grow >= 1
end

-- What the player's spot sees through the window's canvas column x, `behind` m behind the window, on the floor -- in
-- the lobby's frame, a bearing (degrees) and a distance (modules/figure.lua's spot); nil when the scene has no window.
local function windowColumn(x, behind)
  local H = S.spec and S.spec.window
  if H == nil then return nil end
  local off = (x - WINDOW.cw / 2) * H.size[1] / WINDOW.cw
  local b = math.rad(H.bearing)
  local k = (H.dist + behind) / H.dist
  local px = (H.dist * math.sin(b) + math.cos(b) * off) * k
  local py = (H.dist * math.cos(b) - math.sin(b) * off) * k
  return { bearing = math.deg(math.atan2(px, py)), dist = math.sqrt(px * px + py * py) }
end

-- Where what is seen through the window's right part stands: the middle of its clear glass, `behind` m behind it.
function M.windowSpot(behind)
  return windowColumn((WINDOW.clearX + WINDOW.cw - PAD) / 2, behind)
end

-- The bearing (degrees, in the lobby's frame) of the right edge of the window's rows, or nil.
function M.windowRowsEdge()
  local c = windowColumn(WINDOW.rowX + WINDOW.listW, 0)
  return c and c.bearing
end

-- The row a hand points at on the top page, or nil.
function M.hovered()
  local P = S.stack[#S.stack]
  if P == nil or P.hover == nil then return nil end
  return rowByKey(P, P.hover.row)
end

function M.home() M.backTo(1) end

-- The page on top, and the pages' data changed: draw them again.
function M.top() local P = S.stack[#S.stack]; return P and P.page end
function M.depth() return #S.stack end
function M.refresh()
  for _, P in ipairs(S.stack) do P.dirty = true end
end

function M.close()
  capture(false)
  Spawner.clear(TAG)
  Beams.close()
  S.stack, S.leaving, S.anchor, S.preview, S.closing = {}, {}, nil, nil, nil
  S.hands = { {}, {} }
end

function M.update(dt)
  S.time = S.time + (dt or 0)
  if S.anchor == nil then return end
  dt = dt or 0
  for i = #S.leaving, 1, -1 do
    local P = S.leaving[i]
    resolve(P)
    animate(P, dt)
    if P.alpha < 0.02 then
      despawn(P)
      table.remove(S.leaving, i)
    end
  end
  local W = S.closing
  if W ~= nil then
    resolve(W)
    animateWindow(W, dt)
    if W.grow <= 0 then
      despawn(W)
      S.closing = nil
    end
  end
  local top = S.stack[#S.stack]
  if top ~= nil and top.page.update ~= nil then
    local ok, changed = pcall(top.page.update, top.page, S.time)
    if ok and changed then top.dirty = true end
  end
  for _, P in ipairs(S.stack) do
    if resolve(P) then
      -- not under a hand holding its list or a slider: the hand would lose what it holds
      if P.dirty and not held(P) then build(P) end
      if P.view ~= nil then pcall(P.view.update, P.view, dt, 4) end
      if P.window then windowReady(P) else ready(P) end
    end
    if P.window then animateWindow(P, dt) else animate(P, dt) end
  end
  pointers()
  updatePreview(dt)
end

function M.state()
  local pages = {}
  for i, P in ipairs(S.stack) do
    pages[i] = { title = P.page.title, slot = P.slot, alpha = P.alpha, ready = P.ready == true, builds = P.builds,
                 scroll = P.view and P.view.offset, scrollMax = P.view and P.view:maxOffset(),
                 window = P.window and { grow = P.grow, pages = #P.pages } or nil }
  end
  local pv = S.preview
  return { open = S.anchor ~= nil, pages = pages, leaving = #S.leaving, captured = S.captured, error = M.lastError,
           preview = pv and { ready = pv.ready == true, alpha = pv.alpha, text = pv.row and pv.row.previewText,
                              found = pv.entity ~= nil, root = pv.root ~= nil, built = pv.built == true } }
end

-- For tests: what a press at canvas pixel (x, y) of the top panel would press.
function M.press(x, y)
  local P = S.stack[#S.stack]
  if P == nil then return false end
  local h = hitAt(P, x, y)
  local look = h ~= nil and h.part == "slider" and P.looks[h.row] or nil
  if look ~= nil then
    slide(2, { P = P, slider = look }, x, true)
  else
    press(P, h, 2)
  end
  return true
end
function M.frame(index)
  local P = S.stack[index or #S.stack]
  return P and P.frame
end
-- For tests: the preview window (id, entity, comp, root, frame, ready ...) and a panel on the stack.
function M.preview() return S.preview end
function M.panel(index) return S.stack[index or #S.stack] end

return M
