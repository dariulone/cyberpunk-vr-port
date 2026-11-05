-- The terminal: the lobby's main menu, on the terminal's screen at the player's left.
--
-- WHAT IT DOES. Each row does what the same row of the game's own main menu does, through that menu's controller
-- (SingleplayerMenuGameController, still running behind the lobby, out of view): CONTINUE is its QuickLoad (the
-- newest save, the modded-save path included), NEW GAME / LOAD GAME / SETTINGS are its sub-menu events. Those
-- screens are the game's own: while one is open the lobby steps aside (lobby.lua, VRLobbySuspend) and comes back
-- when the game returns to the main menu. VR SETTINGS opens the port's settings in the VR menu
-- (CyberpunkVRPort_Menu); QUIT asks for a second press, then closes the game as the menu's own Quit does.
--
-- WHAT IT IS. A plane with a game ink canvas on it, the VR menu's kind (vrport\ui\vr_lobby.ent: the menu screen's
-- additive plane and a WorldWidgetComponent "vrp_lobby_ui" over a TERMINAL_CANVAS canvas; the plugin gives it a
-- texture of its own, src/Natives/MenuSurface.cpp). Its frame comes from the scene (scene_list.lua screens.terminal,
-- exported from the Blender terminal): the canvas just in front of the terminal's glass. The display is shown only
-- once the canvas has been drawn (VRMenuSurfaceReady).
--
-- TOUCH IT OR POINT AT IT, as the VR menu: an index fingertip presses what it pushes into, a hand's pointer clicks on
-- a trigger pull (VRMenuPointer gives both against the plane). While a hand is on the screen its trigger, grip and
-- face buttons do not reach the game (VRMenuCapture).
local M = {}
local Spawner = require("modules/spawner")

local ENT = "vrport\\ui\\vr_lobby.ent"
local TAG = "VRLobbyTerminal"
local UI = "vrp_lobby_ui"
local CW, CH = 2580, 1980                -- tools/vr_lobby_assets/lobby_layout.py TERMINAL_CANVAS
local FONT = "base\\gameplay\\gui\\fonts\\raj\\raj.inkfontfamily"
local LOGO = "base\\gameplay\\gui\\fullscreen\\main_menu\\menu_background.inkatlas"
local SOUND_DIR = "CyberpunkVRPort_Menu/sounds/"
local HAPTIC_CLICK, HAPTIC_HOVER = 20, 21

-- The game's main menu, in light on dark glass (the screen is additive: black is the terminal's own glass).
local GOLD = { 1.0, 0.76, 0.28 }
local TEXT = { 0.90, 0.88, 0.82 }
local DIM = { 0.40, 0.39, 0.37 }
local EDGE = { 0.46, 0.41, 0.31 }
local PANEL = { 0.07, 0.07, 0.08 }
local RED = { 0.58, 0.06, 0.05 }

-- The rows fill the screen's width under the logo and the room's name, as on the game's own main menu.
local ROW_X, ROW_W, ROW_H, ROW_GAP, ROW_TOP = 190, 2200, 200, 30, 470
local TOUCH = 0.006          -- metres in front of the plane at which a fingertip presses
local HOVER_TIP = 0.06       -- a fingertip this close is on the screen
local PULL, LET_GO = 0.75, 0.35

local TEXTS = {
  en = { title = "BUFFER", vr = "VR SETTINGS", again = "PRESS AGAIN TO QUIT", continue = "CONTINUE",
         new = "NEW GAME", load = "LOAD GAME", settings = "SETTINGS", quit = "QUIT GAME" },
  ru = { title = "БУФЕР", vr = "НАСТРОЙКИ VR", again = "ЕЩЁ РАЗ — ВЫХОД", continue = "ПРОДОЛЖИТЬ",
         new = "НОВАЯ ИГРА", load = "ЗАГРУЗИТЬ ИГРУ", settings = "НАСТРОЙКИ", quit = "ВЫХОД" },
}

local S = { time = 0, screen = nil, frame = nil, rows = {}, hover = nil, hands = { {}, {} }, captured = false,
            armed = nil, dirty = true, cursor = nil }

---------------------------------------------------------------- vectors and the frame

local function add(a, b) return { a[1] + b[1], a[2] + b[2], a[3] + b[3] } end
local function mul(a, s) return { a[1] * s, a[2] * s, a[3] * s } end
local function cross(a, b) return { a[2] * b[3] - a[3] * b[2], a[3] * b[1] - a[1] * b[3], a[1] * b[2] - a[2] * b[1] } end

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

-- The screen's frame in the world, from the lobby's anchor (position, heading in degrees) and the frame in the
-- lobby's own (X right, Y forward, Z up).
local function worldFrame(anchor, s)
  local yaw = math.rad(anchor.yaw)
  local c, sn = math.cos(yaw), math.sin(yaw)
  local function dir(v) return { c * v[1] - sn * v[2], sn * v[1] + c * v[2], v[3] } end
  local p = dir(s.centre)
  return { c = { anchor.pos.x + p[1], anchor.pos.y + p[2], anchor.pos.z + p[3] }, r = dir(s.right), u = dir(s.up),
           w = s.size[1], h = s.size[2] }
end

-- The plane entity: its local X = canvas LEFT, Y = canvas DOWN, Z = toward the viewer (the VR menu's measured
-- orientation for this plane; the component's own rotation is ignored).
local function transformFor(F)
  local tr = WorldTransform.new()
  tr:SetPosition(Vector4.new(F.c[1], F.c[2], F.c[3], 1.0))
  tr:SetOrientation(quatFromBasis(mul(F.r, -1), mul(F.u, -1), cross(F.r, F.u)))
  return tr
end

---------------------------------------------------------------- feedback

local function play(name)
  if type(VRMenuPlayWav) == "function" then pcall(VRMenuPlayWav, SOUND_DIR .. name .. ".wav", 0.8) end
end

-- Hands: 1 left, 2 right (VRMenuPointer's 0 / 1), the same as the haptic mask (1 left, 2 right).
local function haptic(hand, kind)
  if type(VRHapticEvent) == "function" then pcall(VRHapticEvent, kind, hand == 2 and 2 or 1) end
end

local function capture(on)
  if S.captured == on or type(VRMenuCapture) ~= "function" then return end
  if pcall(VRMenuCapture, on) then S.captured = on end
end

---------------------------------------------------------------- the menu's rows

local function language()
  local lang = "en-us"
  pcall(function()
    local ss = Game.GetSettingsSystem()
    if ss:HasGroup("/language") and ss:GetGroup("/language"):HasVar(CName.new("OnScreen")) then
      lang = NameToString(ss:GetVar("/language", CName.new("OnScreen")):GetValue())
    end
  end)
  return lang
end

-- The game's own text for a row of its menu, ours where the game has none.
local function label(key, fallback)
  local ok, t = pcall(function() return Game.GetLocalizedText(key) end)
  if ok and t ~= nil and t ~= "" and t ~= key then return t end
  return fallback
end

local function savesCount()
  local n = 0
  pcall(function() n = Game.GetSystemRequestsHandler():RequestSavesCountSync() end)
  return n or 0
end

-- What the game's menu does for one of its rows (gameuiMenuItemListGameController.HandleMenuItemActivate).
local function activate(action, event)
  local ctrl = M.shared and M.shared.mainMenu
  if ctrl == nil then return false end
  return pcall(function()
    local data = PauseMenuListItemData.new()
    data.action = action
    if event ~= nil then data.eventName = CName.new(event) end
    ctrl:HandleMenuItemActivate(data)
  end)
end

local function rows(T)
  local saves = savesCount() > 0
  return {
    { key = "continue", label = label("UI-ScriptExports-Continue0", T.continue), enabled = saves,
      go = function() activate(PauseMenuAction.QuickLoad) end },
    { key = "new", label = label("UI-ScriptExports-NewGame0", T.new), enabled = true,
      go = function() activate(PauseMenuAction.OpenSubMenu, "OnNewGame") end },
    { key = "load", label = label("UI-ScriptExports-LoadGame0", T.load), enabled = saves,
      go = function() activate(PauseMenuAction.OpenSubMenu, "OnLoadGame") end },
    { key = "settings", label = label("UI-Labels-Settings", T.settings), enabled = true,
      go = function() activate(PauseMenuAction.OpenSubMenu, "OnSwitchToSettings") end },
    { key = "vr", label = T.vr, enabled = true,
      go = function()
        local menu = GetMod("CyberpunkVRPort_Menu")
        if menu ~= nil then pcall(menu.openCentred, "settings") end
      end },
    { key = "quit", label = label("UI-Labels-CloseGame", T.quit), enabled = true, confirm = T.again,
      go = function() activate(PauseMenuAction.ExitGame) end },
  }
end

---------------------------------------------------------------- drawing

local function color(c, a) return HDRColor.new({ Red = c[1], Green = c[2], Blue = c[3], Alpha = a or 1.0 }) end

local function place(w, parent, x, y, sx, sy)
  w:SetAnchor(inkEAnchor.TopLeft)
  w:SetAnchorPoint(Vector2.new({ X = 0.0, Y = 0.0 }))
  w:SetMargin(inkMargin.new({ left = x, top = y, right = 0, bottom = 0 }))
  w:SetSize(Vector2.new({ X = sx, Y = sy }))
  w:Reparent(parent)
  return w
end

local function rect(parent, x, y, sx, sy, col, op)
  local r = place(inkRectangleWidget.new(), parent, x, y, sx, sy)
  r:SetTintColor(color(col))
  r:SetOpacity(op or 1.0)
  return r
end

-- A frame of thin lines round a box.
local function outline(parent, x, y, w, h, col, op, t)
  rect(parent, x, y, w, t, col, op)
  rect(parent, x, y + h - t, w, t, col, op)
  rect(parent, x, y, t, h, col, op)
  rect(parent, x + w - t, y, t, h, col, op)
end

local function text(parent, x, y, w, h, str, size, col, centred)
  local t = inkTextWidget.new()
  t:SetAnchor(inkEAnchor.TopLeft)
  t:SetAnchorPoint(Vector2.new({ X = centred and 0.5 or 0.0, Y = 0.5 }))
  t:SetMargin(inkMargin.new({ left = x + (centred and w / 2 or 0), top = y + h / 2, right = 0, bottom = 0 }))
  t:Reparent(parent)
  pcall(function() t:SetFontFamily(FONT, "Semi-Bold") end)
  t:SetFontSize(size)
  pcall(function() t:SetLetterCase(textLetterCase.UpperCase) end)
  pcall(function() t:SetHorizontalAlignment(centred and textHorizontalAlignment.Center or textHorizontalAlignment.Left) end)
  pcall(function() t:SetVerticalAlignment(textVerticalAlignment.Center) end)
  t:SetText(str or "")
  t:SetTintColor(color(col))
  return t
end

local function image(parent, x, y, sx, sy, atlas, part, col)
  local q = place(inkImageWidget.new(), parent, x, y, sx, sy)
  pcall(function() q:SetAtlasResource(ResRef.FromString(atlas)) end)
  pcall(function() q:SetTexturePart(CName.new(part)) end)
  q:SetTintColor(color(col))
  return q
end

-- The whole canvas, drawn again when what it shows changes: the rows' state, the hover, the armed quit.
local function build()
  local root = S.screen.root
  if S.layer ~= nil then pcall(function() root:RemoveChild(S.layer) end) end
  local layer = place(inkCanvas.new(), root, 0, 0, CW, CH)
  S.layer = layer
  local T = TEXTS[string.sub(language(), 1, 2) == "ru" and "ru" or "en"]
  -- The game's logo, the room's name under it.
  image(layer, (CW - 960) / 2, 70, 960, 235, LOGO, "cp_logo", GOLD)
  text(layer, 0, 320, CW, 100, T.title, 88, GOLD, true)
  rect(layer, (CW - 600) / 2, 428, 600, 4, GOLD, 0.6)
  S.rows = {}
  for i, row in ipairs(rows(T)) do
    local y = ROW_TOP + (i - 1) * (ROW_H + ROW_GAP)
    local hover = row.enabled and S.hover == row.key
    local armed = S.armed ~= nil and S.armed.key == row.key and S.time < S.armed.until_
    if hover or armed then
      rect(layer, ROW_X, y, ROW_W, ROW_H, RED, 0.95)
      rect(layer, ROW_X, y, 14, ROW_H, GOLD, 1.0)
      rect(layer, ROW_X + ROW_W - 14, y, 14, ROW_H, GOLD, 1.0)
    else
      rect(layer, ROW_X, y, ROW_W, ROW_H, PANEL, 1.0)
    end
    outline(layer, ROW_X, y, ROW_W, ROW_H, (hover or armed) and GOLD or EDGE, 1.0, 4)
    text(layer, ROW_X + 90, y, ROW_W - 180, ROW_H, armed and row.confirm or row.label, 100,
         not row.enabled and DIM or ((hover or armed) and { 1, 1, 1 } or TEXT))
    S.rows[#S.rows + 1] = { x = ROW_X, y = y, w = ROW_W, h = ROW_H, row = row }
  end
  -- The pointer's mark, moved without a rebuild.
  S.cursor = rect(layer, 0, 0, 36, 36, GOLD, 0.0)
  pcall(function() S.cursor:SetRenderTransformPivot(Vector2.new({ X = 0.5, Y = 0.5 })); S.cursor:SetRotation(45) end)
  S.dirty = false
end

local function rowAt(x, y)
  for _, r in ipairs(S.rows) do
    if x >= r.x and x <= r.x + r.w and y >= r.y and y <= r.y + r.h then return r.row end
  end
  return nil
end

---------------------------------------------------------------- the screen in the world

local function spawn(F)
  return { id = Spawner.spawn(ENT, transformFor(F), TAG) }
end

local function resolve(h)
  if h == nil or h.id == nil then return false end
  if h.root ~= nil then return true end
  pcall(function()
    h.entity = Game.FindEntityByID(h.id)
    if h.entity ~= nil then h.comp = h.entity:FindComponentByName(CName.new(UI)) end
    if h.comp ~= nil then h.root = h.comp:GetWidget() end
  end)
  return h.root ~= nil
end

-- The display is revealed once the plugin's own texture holds a canvas written after the build.
local function ready(h)
  if h.ready then return true end
  if type(VRMenuSurfaceReady) ~= "function" then return false end
  local ok, done = pcall(VRMenuSurfaceReady, h.comp, h.root)
  if not ok or not done then return false end
  pcall(function()
    h.entity:FindComponentByName(CName.new("display")):Toggle(true)
    h.ready = true
  end)
  return h.ready == true
end

---------------------------------------------------------------- hands

-- Each hand's touch on the canvas this frame: pixels and whether it is a fingertip or the pointer.
local function hands(F)
  local out = {}
  local ok, hits = pcall(VRMenuPointer, Vector4.new(F.c[1], F.c[2], F.c[3], 1.0), Vector4.new(F.r[1], F.r[2], F.r[3], 0),
                         Vector4.new(F.u[1], F.u[2], F.u[3], 0), Vector4.new(F.w, F.h, 0, 0))
  if not ok or hits == nil or #hits < 8 then return out end
  for hand = 1, 2 do
    local tip, ray, origin = hits[4 * hand - 3], hits[4 * hand - 2], hits[4 * hand - 1]
    local on = function(p) return p.x >= 0 and p.x <= 1 and p.y >= 0 and p.y <= 1 end
    if tip.w > 0.5 and on(tip) and tip.z > -0.03 and tip.z < HOVER_TIP then
      out[hand] = { x = tip.x * CW, y = tip.y * CH, tip = true, depth = tip.z }
    elseif ray.w > 0.5 and on(ray) then
      out[hand] = { x = ray.x * CW, y = ray.y * CH, tip = false, trigger = origin.w }
    end
  end
  return out
end

local function click(row, hand)
  if row == nil or not row.enabled then return end
  haptic(hand, HAPTIC_CLICK)
  play("click")
  if row.confirm ~= nil and not (S.armed ~= nil and S.armed.key == row.key and S.time < S.armed.until_) then
    S.armed = { key = row.key, until_ = S.time + 3.0 }
    S.dirty = true
    return
  end
  S.armed = nil
  S.dirty = true
  row.go()
end

local function pointers(F)
  local touch = hands(F)
  local hover, at = nil, nil
  for hand = 1, 2 do
    local t, P = touch[hand], S.hands[hand]
    if t ~= nil then
      local row = rowAt(t.x, t.y)
      if row ~= nil and row.enabled then hover = row.key end
      at = at or t
      if t.tip then
        -- A press is the fingertip crossing into the plane; it has to come back out before the next one.
        local pressed = t.depth <= TOUCH
        if pressed and not P.pressed then click(row, hand) end
        P.pressed = pressed
        P.pulled = false
      else
        local pulled = P.pulled and t.trigger > LET_GO or t.trigger >= PULL
        if pulled and not P.pulled then click(row, hand) end
        P.pulled = pulled
        P.pressed = false
      end
      if row ~= nil and row.enabled and P.over ~= row.key then haptic(hand, HAPTIC_HOVER) end
      P.over = row and row.key or nil
    else
      P.pressed, P.pulled, P.over = false, false, nil
    end
  end
  capture(touch[1] ~= nil or touch[2] ~= nil)
  if hover ~= S.hover then S.hover = hover; S.dirty = true end
  if S.armed ~= nil and S.time >= S.armed.until_ then S.armed = nil; S.dirty = true end
  if S.cursor ~= nil then
    pcall(function()
      if at ~= nil then
        S.cursor:SetMargin(inkMargin.new({ left = at.x - 18, top = at.y - 18, right = 0, bottom = 0 }))
        S.cursor:SetOpacity(1.0)
      else
        S.cursor:SetOpacity(0.0)
      end
    end)
  end
end

---------------------------------------------------------------- the lobby's calls

-- shared: the Lobby mod's own state across reloads (init.lua): the game's main menu controller.
function M.init(shared) M.shared = shared end

-- The screen goes up on the terminal, in the lobby's frame.
function M.open(anchor, screen)
  M.close()
  if screen == nil or anchor == nil then return end
  S.frame = worldFrame(anchor, screen)
  S.screen = spawn(S.frame)
  S.dirty = true
end

function M.close()
  capture(false)
  Spawner.clear(TAG)
  S.screen, S.frame, S.layer, S.cursor, S.rows, S.hover, S.armed = nil, nil, nil, nil, {}, nil, nil
  S.hands = { {}, {} }
end

function M.update(dt)
  S.time = S.time + (dt or 0)
  local h = S.screen
  if h == nil or not resolve(h) then return end
  if S.dirty then build() end
  if not ready(h) then return end
  pointers(S.frame)
end

function M.state()
  return { open = S.screen ~= nil, ready = S.screen ~= nil and S.screen.ready == true, hover = S.hover,
           captured = S.captured, menu = M.shared ~= nil and M.shared.mainMenu ~= nil }
end

-- For tests: what a press at canvas pixel (x, y) would press.
function M.press(x, y) click(rowAt(x, y), 2) end
function M.frame() return S.frame end

return M
