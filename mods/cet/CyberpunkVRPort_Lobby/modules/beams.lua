-- The hands' pointer beams over the lobby's menu, made as the VR menu makes its own (mods/cet/CyberpunkVRPort_Menu
-- modules/menu.lua, beam): vrport\ui\vr_menu_beam.ent, a strip 4 mm by 1.5 m drawn by a WorldWidgetComponent into a
-- canvas of its own (16 x 2048 px, src/Natives/MenuSurface.cpp), laid along the ray from the hand to where it meets a
-- panel and turned about the ray to face the eyes; a soft glow and a core in the colours of the menu they point at,
-- not there for the first HIDE m from the hand and coming in over the next FADE m (STEPS rects of rising opacity), as
-- the VR menu's beam does (its ink.lua K.BEAM). Shown while the hand points at a panel, hidden otherwise; spawned with
-- the menu and gone with it.
local M = {}
local Spawner = require("modules/spawner")
local Ink = require("modules/ink")

local ENT = "vrport\\ui\\vr_menu_beam.ent"
local TAG = "VRLobbyBeam"
local UI = "vrp_menu_ui"
local LEN, CW, CH = 1.5, 16, 2048
local HIDE, FADE, STEPS = 0.12, 0.12, 6

-- The glow and the core, colour and opacity: the menu's own (M.open).
local S = { beams = { {}, {} }, look = { glow = { 1, 1, 1 }, glowOp = 0.2, core = { 1, 1, 1 }, coreOp = 0.6 } }

local function sub(a, b) return { a[1] - b[1], a[2] - b[2], a[3] - b[3] } end
local function add(a, b) return { a[1] + b[1], a[2] + b[2], a[3] + b[3] } end
local function mul(a, s) return { a[1] * s, a[2] * s, a[3] * s } end
local function dot(a, b) return a[1] * b[1] + a[2] * b[2] + a[3] * b[3] end
local function cross(a, b) return { a[2] * b[3] - a[3] * b[2], a[3] * b[1] - a[1] * b[3], a[1] * b[2] - a[2] * b[1] } end
local function norm(v)
  local l = math.sqrt(dot(v, v))
  return l > 1e-6 and mul(v, 1 / l) or { 0, 0, 0 }
end

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

-- The strip centred on C, its canvas right along R and canvas up along U (the plane's local X = canvas left,
-- Y = canvas down, Z toward the viewer).
local function transformFor(C, R, U)
  local tr = WorldTransform.new()
  tr:SetPosition(Vector4.new(C[1], C[2], C[3], 1.0))
  tr:SetOrientation(quatFromBasis(mul(R, -1), mul(U, -1), cross(R, U)))
  return tr
end

-- The entity, its widget and the canvas drawn, once the spawn has landed; shown once the native target took a write.
local function resolve(h)
  if h.id == nil then return false end
  if h.root == nil then
    pcall(function()
      h.entity = Game.FindEntityByID(h.id)
      if h.entity ~= nil then h.comp = h.entity:FindComponentByName(CName.new(UI)) end
      if h.comp ~= nil then h.root = h.comp:GetWidget() end
    end)
    if h.root == nil then return false end
    pcall(function() h.comp.glitchValue = 0.0 end)
    h.root:RemoveAllChildren()
    local L = S.look
    h.pieces, h.px = {}, -1
    for k = 1, STEPS + 1 do
      local f = k <= STEPS and k / (STEPS + 1) or 1
      h.pieces[#h.pieces + 1] = { k = k, x = 0, w = CW, rect = Ink.rect(h.root, 0, 0, CW, 0, L.glow, L.glowOp * f) }
      h.pieces[#h.pieces + 1] = { k = k, x = (CW - 6) / 2, w = 6,
        rect = Ink.rect(h.root, (CW - 6) / 2, 0, 6, 0, L.core, L.coreOp * f) }
    end
  end
  if not h.ready then
    if type(VRMenuSurfaceReady) ~= "function" then return false end
    local ok, done = pcall(VRMenuSurfaceReady, h.comp, h.root)
    if not ok or not done then return false end
    pcall(function()
      h.entity:FindComponentByName(CName.new("display")):Toggle(true)
      h.ready = true
    end)
  end
  return h.ready == true
end

-- Both hands' strips, once, at `at` and out of sight until placed; look: { glow, glowOp, core, coreOp }.
function M.open(at, look)
  if look ~= nil then S.look = look end
  for i = 1, 2 do
    if S.beams[i].id == nil then
      S.beams[i] = { id = Spawner.spawn(ENT, transformFor(at, { 1, 0, 0 }, { 0, 0, 1 }), TAG) }
    end
  end
end

local function visible(h, on)
  for _, p in ipairs(h.pieces) do p.rect:SetVisible(on) end
end

-- The pieces laid out for a ray `length` m long (nothing to do while it stays within a pixel).
local function lay(h, length)
  local perM = CH / LEN
  local px = math.max(0, math.min(LEN, length)) * perM
  if math.abs(px - h.px) < 1 then return end
  h.px = px
  local hide, step = HIDE * perM, FADE * perM / STEPS
  for _, p in ipairs(h.pieces) do
    local top = hide + (p.k - 1) * step
    local bottom = p.k <= STEPS and top + step or px
    p.rect:SetMargin(inkMargin.new({ left = p.x, top = top, right = 0, bottom = 0 }))
    p.rect:SetSize(Vector2.new({ X = p.w, Y = math.max(0, math.min(bottom, px) - top) }))
  end
end

-- Hand i (1 left, 2 right): shown from `origin` to `hit` (world, { x, y, z }), turned to face `eye`; or hidden.
function M.set(i, show, origin, hit, eye)
  local h = S.beams[i]
  if h == nil or not resolve(h) then return end
  if not show then
    if h.shown ~= false then
      visible(h, false)
      h.shown = false
    end
    return
  end
  local ray = sub(hit, origin)
  local length = math.sqrt(dot(ray, ray))
  local dir = norm(ray)
  local toEye = sub(eye, origin)
  local n = norm(sub(toEye, mul(dir, dot(toEye, dir))))
  if length < 1e-3 or (n[1] == 0 and n[2] == 0 and n[3] == 0) then return end
  pcall(function()
    Game.VREntitySetWorldTransform(h.entity, transformFor(add(origin, mul(dir, LEN / 2)), cross(n, dir), mul(dir, -1)))
  end)
  lay(h, length)
  if h.shown ~= true then
    visible(h, true)
    h.shown = true
  end
end

function M.close()
  Spawner.clear(TAG)
  S.beams = { {}, {} }
end

return M
