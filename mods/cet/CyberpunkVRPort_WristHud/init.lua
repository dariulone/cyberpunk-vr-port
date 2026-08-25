-- CyberpunkVRPort_WristHud -- the living tattoo on the left forearm, and the sliders that shape it.
--
-- WHAT THE ARCHIVE CARRIES AND WHAT THIS FILE CARRIES. The asset holds only the surface and an empty
-- screen, both added to the player entity (base and ep1):
--
--   vrp_latd_screen   entSkinnedMeshComponent, built by cloning the player's OWN arm sticker
--                     (a0_008_ma__fpp_right_q001_injection_mark): skinning binding bindName "root",
--                     bone weights on LeftForeArm and the wrist joints. That is what sews the readout
--                     INTO the skin instead of hanging a plate off a bone. Its mesh is that sticker
--                     mirrored onto the left arm -- geometry carrying the correction
--                     T = IBM*B_R*S*B_L^-1*IBM^-1 read out of player_man_skeleton.rig, plus a UV
--                     quarter turn and a u -> 1-u flip -- with the transparent UI material.
--   vrp_latd_ui       WorldWidgetComponent -> vrport\ui\wrist_hud.inkwidget, projection plane
--                     0.10 x 0.05 m.
--
-- Everything drawn on it is created HERE, at runtime: the health number and its label, the bar, the
-- cyberdeck memory ticks, the RAM line and the row of buff icons. That is not a workaround. An
-- .inkwidget package cannot gain elements through the importer, but ink widgets CAN be created from
-- code -- inkTextWidget.new() / inkRectangleWidget.new() / inkImageWidget.new() followed by
-- Reparent(root) -- so the asset holds the base and this file holds the content.
--
-- THE ICONS ARE THE GAME'S OWN, by the same chain the HUD walks. Read out of
-- cyberpunk/UI/Player/bufflist.swift: it takes a status-effect record, asks UiData():IconPath(), and
-- hands "UIIcon." + that name to InkImageUtils.RequestSetImage. So this file walks
--   effect -> UiData():IconPath() -> TweakDB flats UIIcon.<name>.atlasResourcePath / .atlasPartName
--         -> SetAtlasResource + SetTexturePart
-- which is why the wrist row shows exactly the buffs the HUD shows, in their own colours, and why a
-- new effect needs no work here.
--
-- THE SLIDERS. Open the CET overlay -> "VR wrist readout". Every number that was tuned by hand is a
-- slider: where the patch sits on the arm (three axes, metres), the type sizes, the bar, the tick
-- geometry, the icon row, the colour and the opacities. SAVE writes wrist.cfg beside this file and it
-- is read back on load, so the numbers survive a restart with no rebuild of anything.
--
-- MEASURED TRAPS, each of which cost a build or an evening:
--   * glitchValue. IWorldWidgetComponent.ShouldReactToHit() is true when the owner is neither a Device
--     nor a Vehicle -- the player is neither -- so the engine glitches the screen permanently:
--     coloured blocks and rolling bars, which look exactly like a broken material and are not one.
--     Zeroed whenever it drifts, and StartGlitching is overridden to nothing.
--   * a nested widget collapses. The authored price_widget sits inside a 0x0 horizontal panel and
--     therefore draws as nothing wherever it is moved. Ours are parented straight to the root.
--   * localizationString beats text. An authored "999" with an empty localizationString reads as an
--     empty label -- the live widget said TEXT='' while the file said 999.
--   * negative widget scale HIDES a widget, and `tracking` is unsigned so letter spacing cannot go
--     below zero. Both were tried on screen; neither is a way to mirror or tighten anything.
--   * runtime children accumulate: nothing drops them when a mod reloads, and one tuning session left
--     249 of them on the root. RemoveChild does work (measured), and the authored elements are the
--     only ones with real names, so the cleanup below removes every child whose name is empty or
--     "UNINITIALIZED_WIDGET" -- precisely the runtime ones -- before building.
--   * font weight. raj has Medium / Bold / Semi-Bold / Regular and no light face at all: SetFontStyle
--     with "Thin"/"Light"/"Regular" reads back Semi-Bold. The thin face is blender's "Book", set with
--     the two-argument SetFontFamily(path, style), which does take.

local SCREEN = "vrp_latd_screen"     -- the skinned patch: the placement offset goes here
local UI     = "vrp_latd_ui"         -- the world widget: disabled in the asset, kept as a fallback
local HUD    = "vrp_latd_hud"        -- the HUD component that now feeds the surface
local ENTRY  = "vrp_wrist"           -- the one entry in vrport\\ui\\wrist_hud.inkhud
local CFG    = "wrist.cfg"

local FONT_THIN = { "base\\gameplay\\gui\\fonts\\blender\\blender.inkfontfamily", "Book" }
local FONT_BOLD = { "base\\gameplay\\gui\\fonts\\raj\\raj.inkfontfamily", "Medium" }

local MAX_TICKS = 32                 -- a cyberdeck tops out far below this; the pool is a backstop
local MAX_ICONS = 16
local MAX_STARS = 5                  -- what the wanted bar itself shows: five star widgets
local STAR_ATLAS = "base\\gameplay\\gui\\widgets\\wanted\\atlas_wanted_2.inkatlas"
local STAR_PART = "star_active"      -- the lit star from the wanted bar atlas

-- One declaration per knob, and the ImGui window, the config file and the defaults all read from it,
-- so a new knob is one line here instead of four places that can disagree.
local SPEC = {
  { g = "RENDERING  (the plane and the tint live in the asset now -- nothing here writes them)" },
  { k = "glitch_hook", l = "suppress StartGlitching (needs a mod reload)", d = 1.0, lo = 0.0, hi = 1.0, bool = true },
  { k = "fix_window", l = "backstop: freeze after N s armed if the transition is missed", d = 1.5, lo = 0.0, hi = 4.0 },
  { g = "THE WHOLE READOUT ON THE BAND  (widget space: these cannot drift)" },
  { k = "spin",  l = "spin (degrees)", d = 0.0, lo = -180.0, hi = 180.0 },
  { k = "scale", l = "size",           d = 1.0, lo = 0.2, hi = 2.5 },

  { g = "THE BAND ITSELF  (entity space: shows a number, pulls away as the arm turns -- for baking)" },
  { k = "plate_x",    l = "raw x (m)",             d = 0.0, lo = -0.08, hi = 0.08 },
  { k = "plate_y",    l = "raw y (m)",             d = 0.0, lo = -0.08, hi = 0.08 },
  { k = "plate_z",    l = "raw z (m)",             d = 0.0, lo = -0.08, hi = 0.08 },
  { k = "arm_along",  l = "toward the hand (mm)",  d = 0.0, lo = -140.0, hi = 140.0 },
  { k = "arm_into",   l = "into the skin (mm)",    d = 0.0, lo = -40.0, hi = 40.0 },
  { k = "arm_around", l = "around the arm (mm)",   d = 0.0, lo = -80.0, hi = 80.0 },
  { k = "yaw",        l = "yaw (deg)",             d = 0.0, lo = -180.0, hi = 180.0 },
  { k = "pitch",      l = "pitch (deg)",           d = 0.0, lo = -90.0, hi = 90.0 },
  { k = "roll",       l = "roll (deg)",            d = 0.0, lo = -180.0, hi = 180.0 },

  { g = "WHERE THE READOUT SITS ON THE PATCH  (widget pixels -- these ride the skin exactly)" },
  { k = "org_x", l = "along the arm", d = -200.0, lo = -400.0, hi = 400.0 },
  { k = "org_y", l = "across the arm", d =    0.0, lo = -300.0, hi = 300.0 },

  { g = "HEALTH NUMBER" },
  { k = "hp_font",  l = "size",       d = 84.0, lo = 10.0, hi = 160.0, int = true },
  { k = "hp_x",     l = "right edge", d = 150.0, lo = -200.0, hi = 500.0 },
  { k = "hp_y",     l = "up / down",  d = -14.0, lo = -200.0, hi = 200.0 },
  { k = "hp_alpha", l = "opacity",    d = 0.85, lo = 0.0, hi = 1.0 },
  { k = "hp_cr", l = "number red", d = 0.62, lo = 0.0, hi = 3.0 },
  { k = "hp_cg", l = "number green", d = 1.30, lo = 0.0, hi = 3.0 },
  { k = "hp_cb", l = "number blue", d = 1.00, lo = 0.0, hi = 3.0 },

  { g = "THE WORD  HP" },
  { k = "lbl_font",  l = "size",      d = 20.0, lo = 6.0, hi = 60.0, int = true },
  { k = "lbl_x",     l = "left edge", d = 158.0, lo = -200.0, hi = 500.0 },
  { k = "lbl_y",     l = "up / down", d = 2.0, lo = -200.0, hi = 200.0 },
  { k = "lbl_alpha", l = "opacity",   d = 0.72, lo = 0.0, hi = 1.0 },
  { k = "lbl_cr", l = "the word HP red", d = 0.62, lo = 0.0, hi = 3.0 },
  { k = "lbl_cg", l = "the word HP green", d = 1.30, lo = 0.0, hi = 3.0 },
  { k = "lbl_cb", l = "the word HP blue", d = 1.00, lo = 0.0, hi = 3.0 },

  { g = "PLAYER LEVEL" },
  { k = "lvl_font",  l = "size",       d = 22.0, lo = 6.0, hi = 60.0, int = true },
  { k = "lvl_x",     l = "left edge",  d = -200.0, lo = -400.0, hi = 400.0 },
  { k = "lvl_y",     l = "up / down",  d = -70.0, lo = -300.0, hi = 300.0 },
  { k = "lvl_alpha", l = "opacity",    d = 0.75, lo = 0.0, hi = 1.0 },
  { k = "lvl_box",   l = "box side (0 = no box)", d = 34.0, lo = 0.0, hi = 90.0 },
  { k = "lvl_edge",  l = "box line",              d = 2.0, lo = 1.0, hi = 8.0 },
  { k = "lvl_box_a", l = "box opacity",           d = 0.55, lo = 0.0, hi = 1.0 },
  { k = "lvl_cr", l = "level red", d = 0.62, lo = 0.0, hi = 3.0 },
  { k = "lvl_cg", l = "level green", d = 1.30, lo = 0.0, hi = 3.0 },
  { k = "lvl_cb", l = "level blue", d = 1.00, lo = 0.0, hi = 3.0 },
  { k = "box_cr", l = "box red", d = 0.62, lo = 0.0, hi = 3.0 },
  { k = "box_cg", l = "box green", d = 1.30, lo = 0.0, hi = 3.0 },
  { k = "box_cb", l = "box blue", d = 1.00, lo = 0.0, hi = 3.0 },

  { g = "WANTED STARS  (the police heat, one star per level, as the HUD counts it)" },
  { k = "star_size",  l = "size",              d = 22.0, lo = 6.0, hi = 48.0 },
  { k = "star_step",  l = "step",              d = 24.0, lo = 6.0, hi = 60.0 },
  { k = "star_x",     l = "left edge",         d = -90.0, lo = -400.0, hi = 400.0 },
  { k = "star_y",     l = "up / down",         d = -70.0, lo = -300.0, hi = 300.0 },
  { k = "star_alpha", l = "opacity",           d = 0.90, lo = 0.0, hi = 1.0 },
  { k = "star_empty", l = "the unlit ones",    d = 0.0, lo = 0.0, hi = 1.0 },
  { k = "star_cr", l = "stars red", d = 1.00, lo = 0.0, hi = 3.0 },
  { k = "star_cg", l = "stars green", d = 1.00, lo = 0.0, hi = 3.0 },
  { k = "star_cb", l = "stars blue", d = 1.00, lo = 0.0, hi = 3.0 },

  { g = "HEALTH BAR  (its length IS the health; the icon row wraps at this width)" },
  { k = "bar_w",       l = "full length",    d = 260.0, lo = 40.0, hi = 600.0 },
  { k = "bar_h",       l = "thickness",      d = 6.0, lo = 1.0, hi = 40.0 },
  { k = "bar_y",       l = "up / down",      d = 28.0, lo = -200.0, hi = 300.0 },
  { k = "bar_alpha",   l = "opacity",        d = 0.85, lo = 0.0, hi = 1.0 },
  { k = "track_alpha", l = "the spent part", d = 0.12, lo = 0.0, hi = 1.0 },
  { k = "bar_cr", l = "bar red", d = 0.62, lo = 0.0, hi = 3.0 },
  { k = "bar_cg", l = "bar green", d = 1.30, lo = 0.0, hi = 3.0 },
  { k = "bar_cb", l = "bar blue", d = 1.00, lo = 0.0, hi = 3.0 },
  { k = "track_cr", l = "spent part red", d = 0.62, lo = 0.0, hi = 3.0 },
  { k = "track_cg", l = "spent part green", d = 1.30, lo = 0.0, hi = 3.0 },
  { k = "track_cb", l = "spent part blue", d = 1.00, lo = 0.0, hi = 3.0 },

  { g = "OVERSHIELD  (the gold overfill: health above the maximum, as the HUD shows it)" },
  { k = "over_alpha", l = "opacity",                    d = 0.95, lo = 0.0, hi = 1.0 },
  { k = "over_h",     l = "thickness (0 = as the bar)", d = 0.0, lo = 0.0, hi = 40.0 },
  { k = "over_cr",    l = "overshield red",             d = 1.60, lo = 0.0, hi = 3.0 },
  { k = "over_cg",    l = "overshield green",           d = 1.25, lo = 0.0, hi = 3.0 },
  { k = "over_cb",    l = "overshield blue",            d = 0.45, lo = 0.0, hi = 3.0 },

  { g = "CYBERDECK MEMORY  (one tick per RAM point)" },
  { k = "tick_w",     l = "tick width",  d = 6.0, lo = 1.0, hi = 30.0 },
  { k = "tick_h",     l = "tick height", d = 40.0, lo = 4.0, hi = 90.0 },
  { k = "tick_gap",   l = "gap",         d = 5.0, lo = 0.0, hi = 24.0 },
  { k = "tick_y",     l = "up / down",   d = 70.0, lo = -200.0, hi = 300.0 },
  { k = "tick_alpha", l = "opacity",     d = 0.80, lo = 0.0, hi = 1.0 },
  { k = "tick_empty", l = "spent ticks", d = 0.12, lo = 0.0, hi = 1.0 },
  { k = "tick_cr", l = "ticks red", d = 0.62, lo = 0.0, hi = 3.0 },
  { k = "tick_cg", l = "ticks green", d = 1.30, lo = 0.0, hi = 3.0 },
  { k = "tick_cb", l = "ticks blue", d = 1.00, lo = 0.0, hi = 3.0 },

  { g = "RAM  x/y  (right-aligned to the end of the tick row)" },
  { k = "ram_font",  l = "size",      d = 18.0, lo = 6.0, hi = 60.0, int = true },
  { k = "ram_x",     l = "nudge",     d = 0.0, lo = -200.0, hi = 200.0 },
  { k = "ram_y",     l = "up / down", d = 120.0, lo = -200.0, hi = 400.0 },
  { k = "ram_alpha", l = "opacity",   d = 0.75, lo = 0.0, hi = 1.0 },
  { k = "ram_cr", l = "RAM red", d = 0.62, lo = 0.0, hi = 3.0 },
  { k = "ram_cg", l = "RAM green", d = 1.30, lo = 0.0, hi = 3.0 },
  { k = "ram_cb", l = "RAM blue", d = 1.00, lo = 0.0, hi = 3.0 },

  { g = "BUFF ICONS  (the HUD's own, from the left, wrapping at the bar's width)" },
  { k = "icon_size", l = "size",            d = 28.0, lo = 8.0, hi = 64.0 },
  { k = "icon_step", l = "step",            d = 30.0, lo = 8.0, hi = 80.0 },
  { k = "icon_y",    l = "up / down",       d = 164.0, lo = -200.0, hi = 400.0 },
  { k = "icon_row",  l = "second row step", d = 32.0, lo = 8.0, hi = 90.0 },
  { k = "icon_cr", l = "icons red", d = 1.00, lo = 0.0, hi = 3.0 },
  { k = "icon_cg", l = "icons green", d = 1.00, lo = 0.0, hi = 3.0 },
  { k = "icon_cb", l = "icons blue", d = 1.00, lo = 0.0, hi = 3.0 },

  { g = "COLOUR  (above 1.0 is emissive -- this is an HDR tint)" },
  { k = "col_r", l = "red",   d = 0.62, lo = 0.0, hi = 3.0 },
  { k = "col_g", l = "green", d = 1.30, lo = 0.0, hi = 3.0 },
  { k = "col_b", l = "blue",  d = 1.00, lo = 0.0, hi = 3.0 },
  { k = "thin",  l = "thin type (blender Book, not raj Medium)", d = 1.0, lo = 0.0, hi = 1.0, bool = true },
}

local cfg = {}
local S = {
  built  = false,
  childMark = 0,           -- how many children the tree held once ours were in it
  hp = nil, lbl = nil, track = nil, fill = nil, over = nil, ram = nil, lvl = nil,
  ticks = {}, icons = {}, stars = {}, lvlBox = {},
  tickN = 0,
  dirty = true,            -- a layout number changed
  accFast = 0.0, accSlow = 0.0, accCheck = 0.0,
  demo = false, demoFrac = 1.0,
  note = "waiting for the player",
  route = "?",             -- which call actually returned the widget tree
  rearmed = false,         -- the one re-arm has been done for this player object
  armedNow = false,        -- the weapon slot, sampled four times a second
  accArm = 0.0,
  fixFor = 0.0,            -- how long a weapon has been in hand, until the re-arm fires
  guard = nil,             -- is the plugin's watchdog present
  guardCode = 0,           -- what it answered last
  repairs = 0,             -- how many times it repaired this session
}

-- ---------------------------------------------------------------- the config file

local function defaults()
  local t = {}
  for _, e in ipairs(SPEC) do
    if e.k then t[e.k] = e.d end
  end
  return t
end

local function loadCfg()
  cfg = defaults()
  local f = io.open(CFG, "r")
  if not f then return false end
  local body = f:read("*a")
  f:close()
  if not body then return false end
  for line in body:gmatch("[^\r\n]+") do
    local k, v = line:match("^%s*([%w_]+)%s*=%s*(-?[%d%.]+)%s*$")
    if k and v and cfg[k] ~= nil then cfg[k] = tonumber(v) or cfg[k] end
  end
  return true
end

local function saveCfg()
  local f = io.open(CFG, "w")
  if not f then return false end
  f:write("-- CyberpunkVRPort wrist readout. Written by the CET overlay: VR wrist readout -> SAVE.\n")
  for _, e in ipairs(SPEC) do
    if e.g then f:write("\n-- " .. e.g .. "\n") end
    if e.k then f:write(string.format("%s=%.4f\n", e.k, cfg[e.k] or e.d)) end
  end
  f:close()
  return true
end

-- ---------------------------------------------------------------- small helpers

local function tint()
  return HDRColor.new({ Red = cfg.col_r, Green = cfg.col_g, Blue = cfg.col_b, Alpha = 1.0 })
end

-- Each item carries its own triple; a wrist.cfg written before those existed simply has none of them,
-- and then the global COLOUR group answers instead. Above 1.0 is emissive, which is why these run to 3.
local function tintOf(pre)
  local r = cfg[pre .. "_cr"]
  local g = cfg[pre .. "_cg"]
  local b = cfg[pre .. "_cb"]
  if r == nil then r = cfg.col_r end
  if g == nil then g = cfg.col_g end
  if b == nil then b = cfg.col_b end
  return HDRColor.new({ Red = r, Green = g, Blue = b, Alpha = 1.0 })
end

local function font()
  if (cfg.thin or 0) >= 0.5 then return FONT_THIN end
  return FONT_BOLD
end

local function place(w, sx, sy, left, top, ax, ay)
  w:SetSize(Vector2.new({ X = sx, Y = sy }))
  w:SetAnchor(inkEAnchor.Centered)
  w:SetAnchorPoint(Vector2.new({ X = ax or 0.0, Y = ay or 0.5 }))
  w:SetMargin(inkMargin.new({ left = left, top = top, right = 0, bottom = 0 }))
end

local function mkRect(root)
  local q = inkRectangleWidget.new()
  q:Reparent(root)
  q:SetVisible(false)
  return q
end

local function mkText(root, align)
  local q = inkTextWidget.new()
  q:Reparent(root)
  local fam = font()
  pcall(function() q:SetFontFamily(fam[1], fam[2]) end)
  pcall(function() q:SetHorizontalAlignment(align) end)
  pcall(function() q:SetVerticalAlignment(textVerticalAlignment.Center) end)
  pcall(function() q.tracking = 0 end)
  pcall(function() q.scrollTextSpeed = 0.0 end)
  q:SetVisible(false)
  return q
end

local function mkImage(root)
  local q = inkImageWidget.new()
  q:Reparent(root)
  -- WHITE IS "UNTINTED": the buff icons must keep their own colours, and a tint multiplies them.
  pcall(function() q:SetTintColor(HDRColor.new({ Red = 1.0, Green = 1.0, Blue = 1.0, Alpha = 1.0 })) end)
  q:SetVisible(false)
  return q
end

local function comp(name)
  local p = Game.GetPlayer()
  if p == nil then return nil end
  local c = nil
  pcall(function() c = p:FindComponentByName(CName.new(name)) end)
  return c
end

-- ---------------------------------------------------------------- build
-- WHERE THE READOUT LIVES, resolved once and reported. The asset feeds the band from a
-- WidgetHudComponent now, because that is the class the player entity already carries for the whole
-- game HUD (`hud_component`, material base\\materials\\ui_main_hud.mt, entries prototype_hud.inkhud,
-- parentTransform -> camera) and the game HUD is composited every frame in the UI pass: it does not
-- belong to a scene rendering plane, so there is no transition for it to lose a render target to.
-- Ours is that component copied field for field with the mesh target pointed at the band and its own
-- one-entry list.
--
-- The declared call is GetWidget(entryName: CName). The three other shapes are here because CET's
-- marshalling is the one thing a file cannot answer, and a wrong guess costs a whole restart.
local function surface()
  local ch = comp(HUD)
  if ch ~= nil then
    local w = nil
    pcall(function() w = ch:GetWidget(CName.new(ENTRY)) end)
    if w ~= nil then S.route = "hud:GetWidget(CName)" return w end
    pcall(function() w = ch:GetWidget(ENTRY) end)
    if w ~= nil then S.route = "hud:GetWidget(string)" return w end
    pcall(function() w = ch:GetWidget() end)
    if w ~= nil then S.route = "hud:GetWidget()" return w end
    S.route = "the HUD component answers no GetWidget"
  else
    S.route = "no " .. HUD .. " on the player"
  end
  local cu = comp(UI)
  if cu ~= nil then
    local w = nil
    pcall(function() w = cu:GetWidget() end)
    if w ~= nil then S.route = "the world widget (fallback)" return w end
  end
  return nil
end


-- Removes what an earlier run of this mod left behind. The authored children carry real names (bg,
-- fluff3, Layout); anything created at runtime reads as "UNINITIALIZED_WIDGET" or as an empty name,
-- so that is the exact test. Without this a mod reload doubles the readout every time.
local function sweep(root)
  local dropped = 0
  local guard = 0
  while guard < 400 do
    guard = guard + 1
    local victim = nil
    local n = root:GetNumChildren()
    for i = 0, n - 1 do
      local k = root:GetWidgetByIndex(i)
      if k ~= nil then
        local nm = ""
        pcall(function() nm = tostring(k:GetName().value) end)
        if nm == "" or nm == "UNINITIALIZED_WIDGET" or nm == "None" then
          victim = k
          break
        end
      end
    end
    if victim == nil then break end
    local ok = pcall(function() root:RemoveChild(victim) end)
    if not ok then break end
    dropped = dropped + 1
  end
  return dropped
end

-- The authored placeholder has to go, and it is NOT a root child: price_widget sits three levels down
-- (Layout -> hover_buy_button -> price_widget), inside a 0x0 panel the asset parks off to one side.
-- Scanning only the root's own children missed it, so the moment the localisation string made that
-- "999" draw at all it appeared as a green scrap out at the end of the forearm, far from the readout.
-- `bg` is a root child and hidden by the same pass; hover_buy_button goes with its text.
local function hideAuthored(w, depth)
  if depth > 6 then return end
  local n = -1
  pcall(function() n = w:GetNumChildren() end)
  if n < 0 then return end
  for i = 0, n - 1 do
    local k = w:GetWidgetByIndex(i)
    if k ~= nil then
      local nm = ""
      pcall(function() nm = tostring(k:GetName().value) end)
      if nm == "price_widget" or nm == "bg" or nm == "hover_buy_button" then
        k:SetVisible(false)
        k:SetOpacity(0.0)
      else
        hideAuthored(k, depth + 1)
      end
    end
  end
end

local function build()
  local root = surface()
  if root == nil then
    S.note = S.route or "no readout surface"
    return false
  end

  local dropped = sweep(root)
  hideAuthored(root, 0)

  S.track = mkRect(root)
  S.fill  = mkRect(root)
  -- created after the fill, so it draws over it, and before the texts, so they stay on top
  S.over  = mkRect(root)
  S.hp    = mkText(root, textHorizontalAlignment.Right)
  S.lbl   = mkText(root, textHorizontalAlignment.Left)
  S.ram   = mkText(root, textHorizontalAlignment.Right)
  S.lvl   = mkText(root, textHorizontalAlignment.Center)
  S.lvlBox = {}
  for i = 1, 4 do S.lvlBox[i] = mkRect(root) end
  pcall(function() S.lbl:SetText("HP") end)
  S.ticks, S.icons, S.stars, S.tickN = {}, {}, {}, 0
  for i = 1, MAX_TICKS do S.ticks[i] = mkRect(root) end
  for i = 1, MAX_ICONS do S.icons[i] = mkImage(root) end
  for i = 1, MAX_STARS do
    local q = mkImage(root)
    -- the wanted bar's own atlas, so the star is the game's art rather than something drawn here
    pcall(function() q:SetAtlasResource(ResRef.FromString(STAR_ATLAS)) end)
    pcall(function() q:SetTexturePart(CName.new(STAR_PART)) end)
    S.stars[i] = q
  end

  root:SetVisible(true)
  root:SetOpacity(1.0)
  -- what the tree holds once we are in it: the asset's own children plus 5 + MAX_TICKS + MAX_ICONS
  S.childMark = root:GetNumChildren()
  S.built  = true
  S.dirty  = true
  S.note   = string.format("built, %d stale children removed", dropped)
  return true
end

-- The widget instance can be replaced under us -- a load, a respawn, photo mode -- and then our
-- elements are orphaned in a tree nothing draws. The test is the CHILD COUNT and not the handle:
-- CET hands out a fresh userdata wrapper on every call, so tostring() on it is a different address
-- each time and would report "not ours" twice a second, sweeping and rebuilding forever. A fresh
-- instance carries only the asset's handful of children, while ours carries 53 more, so the count
-- separates them and can only grow while the instance lives.
local function ours()
  if not S.built then return false end
  local root = surface()
  if root == nil then return false end
  local n = -1
  pcall(function() n = root:GetNumChildren() end)
  return n >= (S.childMark or 0)
end

-- ---------------------------------------------------------------- layout and values

local function rowWidth(n)
  if n <= 0 then return 0.0 end
  return n * cfg.tick_w + (n - 1) * cfg.tick_gap
end

local function placeRam(n)
  place(S.ram, cfg.ram_font * 7.0, cfg.ram_font * 1.8,
        cfg.org_x + rowWidth(n) + cfg.ram_x, cfg.org_y + cfg.ram_y, 1.0, 0.5)
end

local function layout()
  if not S.built then return end
  local col = tint()
  local ox, oy = cfg.org_x, cfg.org_y
  local fam = font()

  place(S.hp, cfg.hp_font * 3.2, cfg.hp_font * 1.4, ox + cfg.hp_x, oy + cfg.hp_y, 1.0, 0.5)
  pcall(function() S.hp:SetFontFamily(fam[1], fam[2]) end)
  pcall(function() S.hp:SetFontSize(math.floor(cfg.hp_font + 0.5)) end)
  pcall(function() S.hp:SetTintColor(tintOf("hp")) end)
  S.hp:SetOpacity(cfg.hp_alpha)
  S.hp:SetVisible(true)

  place(S.lbl, cfg.lbl_font * 3.0, cfg.lbl_font * 1.8, ox + cfg.lbl_x, oy + cfg.lbl_y, 0.0, 0.5)
  pcall(function() S.lbl:SetFontFamily(fam[1], fam[2]) end)
  pcall(function() S.lbl:SetFontSize(math.floor(cfg.lbl_font + 0.5)) end)
  pcall(function() S.lbl:SetTintColor(tintOf("lbl")) end)
  S.lbl:SetOpacity(cfg.lbl_alpha)
  S.lbl:SetVisible(true)

  place(S.track, cfg.bar_w, cfg.bar_h, ox, oy + cfg.bar_y, 0.0, 0.5)
  pcall(function() S.track:SetTintColor(tintOf("track")) end)
  S.track:SetOpacity(cfg.track_alpha)
  S.track:SetVisible(cfg.track_alpha > 0.01)

  place(S.fill, cfg.bar_w, cfg.bar_h, ox, oy + cfg.bar_y, 0.0, 0.5)
  pcall(function() S.fill:SetTintColor(tintOf("bar")) end)
  S.fill:SetOpacity(cfg.bar_alpha)
  S.fill:SetVisible(true)

  local oh = (cfg.over_h or 0.0)
  if oh < 0.5 then oh = cfg.bar_h end
  place(S.over, cfg.bar_w, oh, ox, oy + cfg.bar_y, 0.0, 0.5)
  pcall(function() S.over:SetTintColor(tintOf("over")) end)
  S.over:SetOpacity(cfg.over_alpha or 0.95)

  pcall(function() S.ram:SetFontFamily(fam[1], fam[2]) end)
  pcall(function() S.ram:SetFontSize(math.floor(cfg.ram_font + 0.5)) end)
  pcall(function() S.ram:SetTintColor(tintOf("ram")) end)
  S.ram:SetOpacity(cfg.ram_alpha)
  S.ram:SetVisible(true)
  placeRam(S.tickN)

  for i = 1, MAX_TICKS do
    local q = S.ticks[i]
    place(q, cfg.tick_w, cfg.tick_h, ox + (i - 1) * (cfg.tick_w + cfg.tick_gap), oy + cfg.tick_y, 0.0, 0.5)
    pcall(function() q:SetTintColor(tintOf("tick")) end)
  end

  -- the whole readout, turned and sized in the widget's OWN space, which the material samples through
  -- the mesh's UVs -- so unlike a component transform this cannot come off the arm
  local rootw = surface()
  if rootw ~= nil then
    pcall(function() rootw:SetRotation(cfg.spin or 0.0) end)
    local sc = cfg.scale or 1.0
    if sc < 0.05 then sc = 0.05 end
    pcall(function() rootw:SetScale(Vector2.new({ X = sc, Y = sc })) end)
  end

  -- the level: a bare number, centred in a square of four thin lines. lvl_x / lvl_y are the square's
  -- CENTRE, so the number and the frame move together.
  local bx, by = ox + cfg.lvl_x, oy + cfg.lvl_y
  local bs = cfg.lvl_box or 0.0
  local be = cfg.lvl_edge or 2.0
  place(S.lvl, math.max(bs, cfg.lvl_font * 2.5), math.max(bs, cfg.lvl_font * 1.6), bx, by, 0.5, 0.5)
  pcall(function() S.lvl:SetFontFamily(fam[1], fam[2]) end)
  pcall(function() S.lvl:SetFontSize(math.floor(cfg.lvl_font + 0.5)) end)
  pcall(function() S.lvl:SetTintColor(tintOf("lvl")) end)
  pcall(function() S.lvl:SetHorizontalAlignment(textHorizontalAlignment.Center) end)
  S.lvl:SetOpacity(cfg.lvl_alpha)
  S.lvl:SetVisible(true)
  if bs > 1.0 then
    place(S.lvlBox[1], bs, be, bx - bs / 2.0, by - bs / 2.0, 0.0, 0.5)   -- top
    place(S.lvlBox[2], bs, be, bx - bs / 2.0, by + bs / 2.0, 0.0, 0.5)   -- bottom
    place(S.lvlBox[3], be, bs, bx - bs / 2.0, by, 0.0, 0.5)              -- left
    place(S.lvlBox[4], be, bs, bx + bs / 2.0 - be, by, 0.0, 0.5)         -- right
  end
  for i = 1, 4 do
    pcall(function() S.lvlBox[i]:SetTintColor(tintOf("box")) end)
    S.lvlBox[i]:SetOpacity(cfg.lvl_box_a or 0.55)
    S.lvlBox[i]:SetVisible(bs > 1.0 and (cfg.lvl_box_a or 0.0) > 0.01)
  end

  for i = 1, MAX_STARS do
    place(S.stars[i], cfg.star_size, cfg.star_size,
          ox + cfg.star_x + (i - 1) * cfg.star_step, oy + cfg.star_y, 0.0, 0.5)
  end

  local perRow = math.max(1, math.floor(cfg.bar_w / math.max(cfg.icon_step, 1.0)))
  for i = 1, MAX_ICONS do
    local c0 = (i - 1) % perRow
    local r0 = math.floor((i - 1) / perRow)
    place(S.icons[i], cfg.icon_size, cfg.icon_size,
          ox + c0 * cfg.icon_step, oy + cfg.icon_y + r0 * cfg.icon_row, 0.0, 0.5)
  end
  S.dirty = false
end

local function values()
  local p = Game.GetPlayer()
  if p == nil or not S.built then return end
  local id = p:GetEntityID()
  local pools, stats = Game.GetStatPoolsSystem(), Game.GetStatsSystem()
  local hp, hpMax, ram, ramMax = 0.0, 1.0, 0.0, 0.0
  pcall(function() hp = pools:GetStatPoolValue(id, gamedataStatPoolType.Health, false) end)
  pcall(function() hpMax = stats:GetStatValue(id, gamedataStatType.Health) end)
  pcall(function() ram = pools:GetStatPoolValue(id, gamedataStatPoolType.Memory, false) end)
  pcall(function() ramMax = stats:GetStatValue(id, gamedataStatType.Memory) end)
  if hpMax == nil or hpMax <= 0.0 then hpMax = 1.0 end

  local over = 0.0
  pcall(function() over = pools:GetStatPoolValue(id, gamedataStatPoolType.Overshield, false) end)
  if over == nil or over < 0.0 then over = 0.0 end

  local frac = math.max(0.0, math.min(1.0, (hp or 0.0) / hpMax))
  -- the share of the bar is measured against MAX HEALTH, which is what the HUD does
  local ofrac = math.max(0.0, math.min(1.0, over / hpMax))
  if S.demo then
    frac = math.min(1.0, S.demoFrac)
    ofrac = math.max(0.0, S.demoFrac - 1.0) * 2.0
    over = ofrac * hpMax
  end
  -- and the number is health PLUS overshield, so it reads above the maximum while the gold is up
  pcall(function() S.hp:SetText(tostring(math.floor((hp or 0.0) + over + 0.5))) end)
  S.fill:SetSize(Vector2.new({ X = math.max(cfg.bar_w * frac, 0.0), Y = cfg.bar_h }))
  local oh = (cfg.over_h or 0.0)
  if oh < 0.5 then oh = cfg.bar_h end
  S.over:SetSize(Vector2.new({ X = math.max(cfg.bar_w * ofrac, 0.0), Y = oh }))
  S.over:SetVisible(ofrac > 0.001)

  local n = math.floor((ramMax or ram or 0.0) + 0.5)
  if n > MAX_TICKS then n = MAX_TICKS end
  local used = math.floor((ram or 0.0) + 0.5)
  if S.demo then used = math.floor(n * S.demoFrac + 0.5) end
  for i = 1, MAX_TICKS do
    local q = S.ticks[i]
    if i <= n then
      q:SetOpacity(i <= used and cfg.tick_alpha or cfg.tick_empty)
      q:SetVisible(true)
    else
      q:SetVisible(false)
    end
  end
  -- the label rides the row's right edge, so it follows whenever the row's length changes
  if n ~= S.tickN then
    S.tickN = n
    placeRam(n)
  end
  pcall(function() S.ram:SetText(string.format("RAM %d/%d", used, n)) end)

  -- the level, from the stat the whole game reads for it
  local lvl = 0.0
  pcall(function() lvl = stats:GetStatValue(id, gamedataStatType.Level) end)
  pcall(function() S.lvl:SetText(tostring(math.floor((lvl or 0.0) + 0.5))) end)

  -- and the heat, off the same blackboard field the wanted bar listens to
  local heat = 0
  pcall(function()
    local defs = GetAllBlackboardDefs().UI_WantedBar
    local bb = Game.GetBlackboardSystem():Get(defs)
    if bb ~= nil then heat = bb:GetInt(defs.CurrentWantedLevel) end
  end)
  if S.demo then heat = math.floor(S.demoFrac * MAX_STARS + 0.5) end
  if heat == nil or heat < 0 then heat = 0 end
  for i = 1, MAX_STARS do
    local q = S.stars[i]
    pcall(function() q:SetTintColor(tintOf("star")) end)
    if i <= heat then
      q:SetOpacity(cfg.star_alpha)
      q:SetVisible(true)
    elseif (cfg.star_empty or 0.0) > 0.01 then
      q:SetOpacity(cfg.star_empty)
      q:SetVisible(true)
    else
      q:SetVisible(false)
    end
  end
end

-- The HUD's own chain: a status effect record -> UiData():IconPath() -> the UIIcon record that names
-- an atlas and a part. Effects with no UI data are the engine's internal counters (StaminaRatio,
-- DetectorRush, dismember stacks) and the HUD does not show those either.
local function icons()
  local p = Game.GetPlayer()
  if p == nil or not S.built then return end
  local list = nil
  pcall(function() list = Game.GetStatusEffectSystem():GetAppliedEffects(p:GetEntityID()) end)
  local want = {}
  if list ~= nil then
    for i = 1, #list do
      if #want >= MAX_ICONS then break end
      local rec = nil
      pcall(function() rec = list[i]:GetRecord() end)
      if rec ~= nil then
        local path = nil
        pcall(function()
          local ui = rec:UiData()
          if ui ~= nil then path = tostring(ui:IconPath()) end
        end)
        if path ~= nil and path ~= "" and path ~= "nil" then
          local atlas, part = nil, nil
          pcall(function() atlas = TweakDB:GetFlat("UIIcon." .. path .. ".atlasResourcePath") end)
          pcall(function() part = TweakDB:GetFlat("UIIcon." .. path .. ".atlasPartName") end)
          if atlas ~= nil and part ~= nil then
            table.insert(want, { atlas = atlas, part = part })
          end
        end
      end
    end
  end
  for i = 1, MAX_ICONS do
    local q = S.icons[i]
    if i <= #want then
      pcall(function() q:SetAtlasResource(want[i].atlas) end)
      pcall(function() q:SetTexturePart(want[i].part) end)
      pcall(function() q:SetTintColor(tintOf("icon")) end)
      q:SetOpacity(1.0)
      q:SetVisible(true)
    else
      q:SetVisible(false)
    end
  end
end

-- NO COMPONENT TRANSFORM, and that is the whole lesson of an evening of sliders. A skinned
-- component's localTransform (and its orientation) is applied in the ENTITY's space, whose origin is
-- at the player's feet -- not in the bone's. So any offset put there looks right while the arm is
-- still and separates from the skin the moment the arm turns, and a rotation swings the patch about
-- the body instead of spinning it in place. Compensating the pivot only makes it correct in the bind
-- pose. Measured on screen twice, reported as "вращается хуй пойми как и отдаляется при вращении
-- руки", and there is no version of that knob that behaves.
--
-- So WHERE THE PATCH IS is the mesh's business alone: the geometry is mirrored into the left arm's own
-- bind pose and skinned to the left bones, which is exactly how the game's own arm sticker rides the
-- skin, and nothing here touches it. WHERE THE READOUT SITS ON IT is this panel's business, through
-- the widget -- and those pixels cannot drift, because the material samples them through the mesh's
-- UVs. Moving the surface itself along the arm is a one-number rebuild of the mesh (ALONG_MM in
-- scratchpad/left_exact.py), not a slider.

-- THE READOUT GOES BLACK WITH A WEAPON DRAWN, and this file no longer does anything about it. Every
-- lever that was here -- driving renderingPlaneAnimationParam, freezing the surface in the weapon plane,
-- re-attaching the component afterwards, re-enabling it if something switched it off -- has been taken
-- out on purpose, so that what the ASSET does can be read on its own.
--
-- What the asset now says: the world widget's parentTransform binds to `camera` rather than to the
-- surface it draws on, while its meshTargetBinding still names that surface. The component's transform
-- is what the engine uses to decide where the widget IS -- which pass it belongs to, whether it is in
-- view -- and hung off the camera that answer stops depending on where the arm goes.
--
-- If the black comes back, the next thing to try is a different component class (WidgetHudComponent),
-- not another plaster here.

-- THE BAND'S OWN FRAME, in model space, printed by the mesh build (scratchpad/arm_band.py). The
-- forearm runs diagonally through model space, so none of the entity's axes points along it.
local PIVOT  = { -0.4331, 0.1311, 1.1120 }   -- the band's centre, so a turn spins it about itself
local ALONG  = { -0.3859, 0.7383, -0.5532 }  -- toward the hand
local INWARD = { -0.6733, -0.6353, -0.3783 } -- toward the bone
local AROUND = {
  ALONG[2] * INWARD[3] - ALONG[3] * INWARD[2],
  ALONG[3] * INWARD[1] - ALONG[1] * INWARD[3],
  ALONG[1] * INWARD[2] - ALONG[2] * INWARD[1],
}

local function qAxis(ax, deg)
  local a = math.rad(deg or 0.0) * 0.5
  local sn = math.sin(a)
  return { i = ax[1] * sn, j = ax[2] * sn, k = ax[3] * sn, r = math.cos(a) }
end

local function qMul(a, b)
  return {
    i = a.r * b.i + a.i * b.r + a.j * b.k - a.k * b.j,
    j = a.r * b.j - a.i * b.k + a.j * b.r + a.k * b.i,
    k = a.r * b.k + a.i * b.j - a.j * b.i + a.k * b.r,
    r = a.r * b.r - a.i * b.i - a.j * b.j - a.k * b.k,
  }
end

local function qRot(q, v)
  local x, y, z, w = q.i, q.j, q.k, q.r
  local tx = 2.0 * (y * v[3] - z * v[2])
  local ty = 2.0 * (z * v[1] - x * v[3])
  local tz = 2.0 * (x * v[2] - y * v[1])
  return {
    v[1] + w * tx + (y * tz - z * ty),
    v[2] + w * ty + (z * tx - x * tz),
    v[3] + w * tz + (x * ty - y * tx),
  }
end

-- WHAT DRIFTS AND WHAT DOES NOT. The band's place on the arm is the MESH's business: its vertices carry
-- the skin's own weights, so it follows the arm exactly. A component transform is applied in the
-- ENTITY's space -- origin at the player's feet -- so it looks right at rest and pulls away as the arm
-- turns. It is here only because it is the one way to SEE a candidate number on screen; once a number is
-- settled it gets baked into the geometry, where it rides the skin.
local function transform()
  local cs = comp(SCREEN)
  if cs == nil then return end
  local out = { -INWARD[1], -INWARD[2], -INWARD[3] }
  local q = qMul(qMul(qAxis(out, cfg.yaw), qAxis(AROUND, cfg.pitch)), qAxis(ALONG, cfg.roll))
  local pos = { cfg.plate_x or 0.0, cfg.plate_y or 0.0, cfg.plate_z or 0.0 }
  for i = 1, 3 do
    pos[i] = pos[i] + (ALONG[i] * (cfg.arm_along or 0.0)
                       + INWARD[i] * (cfg.arm_into or 0.0)
                       + AROUND[i] * (cfg.arm_around or 0.0)) * 0.001
  end
  local rc = qRot(q, PIVOT)
  for i = 1, 3 do
    pos[i] = pos[i] + PIVOT[i] - rc[i]
  end
  pcall(function() cs:SetLocalPosition(Vector4.new(pos[1], pos[2], pos[3], 1.0)) end)
  local okq = pcall(function() cs:SetLocalOrientation(Quaternion.new(q.i, q.j, q.k, q.r)) end)
  if not okq then
    pcall(function()
      cs:SetLocalOrientation(Quaternion.new({ i = q.i, j = q.j, k = q.k, r = q.r }))
    end)
  end
end

-- glitchValue is a field of IWorldWidgetComponent, so the HUD component has none and there is nothing
-- to zero while it is the one driving the surface -- the engine's "glitch anything whose owner is not a
-- Device" behaviour cannot reach it at all. Kept for the fallback path.
local function unglitch()
  local cu = comp(UI)
  if cu == nil then return end
  pcall(function()
    if cu.glitchValue ~= 0 then cu.glitchValue = 0.0 end
  end)
end

-- Is something in the right hand. Presence only: building a tag out of it:GetItemID().tdbid throws --
-- measured -- and a pcall then swallows it, leaving the answer permanently "no weapon", which is what
-- kept an earlier version of this from ever running.
-- BUILT ONCE, AND LAZILY. TweakDBID.new hashes a string, and this used to be constructed on every call --
-- which was every frame until the readout re-armed, and together with GetItemInSlot that was the 24 fps.
-- It is NOT built at file scope either: this file runs when CET loads the mod, and TweakDB is not
-- guaranteed to be up then, so an id made there can be a dud that never matches anything. Built on the
-- first call that has a player, kept after that.
local slotWeaponRight = nil

local function armed()
  local pl = Game.GetPlayer()
  if pl == nil then return false end
  if slotWeaponRight == nil then
    pcall(function() slotWeaponRight = TweakDBID.new("AttachmentSlots.WeaponRight") end)
    if slotWeaponRight == nil then return false end
  end
  local out = false
  pcall(function()
    local it = Game.GetTransactionSystem():GetItemInSlot(pl, slotWeaponRight)
    if it ~= nil then out = true end
  end)
  return out
end

-- THE REPAIR, and it is exactly one call pair. Both halves were measured separately on a live black
-- surface: RefreshAppearance alone leaves it black, Toggle(false)/Toggle(true) alone brings the picture
-- back. So the re-registration is not what matters -- the component being rebuilt is.
--
-- It costs nothing visible. The ink tree survives (68 children before, 68 after), so S.built stays true
-- and no content is rebuilt; and both calls land in the same frame, so the disabled state is never
-- rendered. That is what makes it safe to repeat every frame inside the window below.
--
-- Of the seven methods the live object answers at all -- RefreshAppearance, Toggle, IsEnabled, GetEntity,
-- GetLocalToWorld, GetName, GetClassName -- there is nothing that addresses the render target directly,
-- so this is the whole toolbox.
local function rebind()
  local cu = comp(UI)
  if cu == nil then return false end
  pcall(function() cu:Toggle(false) end)
  pcall(function() cu:Toggle(true) end)
  return true
end

-- THE WHOLE FIX, and it happens exactly once per session.
--
-- The asset gives the surface renderingPlaneAnimationParam = "renderPlane", so the animation graph
-- carries it into the WEAPON rendering plane when a weapon is drawn -- and that is the only place it is
-- not covered by the arms, which are drawn afterwards in their own plane on top of the frame. Clearing
-- the param then freezes the surface where the draw left it, so no further transition ever happens --
-- and a transition is what destroys the widget's render target and leaves the material sampling its
-- default, which reads as an opaque black rectangle.
--
-- The order is the part that took two wrong turns to find: the param must be "renderPlane" ACROSS a real
-- draw, and only then cleared. Shipping "None" in the asset, or clearing it while unarmed, pins the
-- surface in the SCENE plane instead, where the readout is simply covered -- and both look identical to
-- the recipe that works.
local function freezeAndRearm()
  local cs = comp(SCREEN)
  if cs == nil then return false end
  pcall(function() cs.renderingPlaneAnimationParam = CName.new("None") end)
  return rebind()
end

-- ---------------------------------------------------------------- CET events

local overlay = false

registerForEvent("onInit", function()
  local had = loadCfg()

  -- THE ENGINE GLITCHES any world screen whose owner is neither a Device nor a Vehicle -- the player is
  -- neither -- so without this the screen carries a permanent glitch: coloured blocks and rolling bars.
  --
  -- It is also a SUSPECT in the black screen, which is why it is a knob now. This Override replaces a
  -- native method for every world widget in the game, and the engine calls StartGlitching during its own
  -- setup as well, so a no-op there could leave a widget half-initialised. Measured against it: while
  -- black, glitchValue reads 0, so the black is not the glitch effect itself. Switch this off, reload the
  -- mods, and draw a weapon: if the black stops, the hook was the cause; if the screen instead fills with
  -- glitch, that is what the hook is holding back and the cure has to be gentler than removing it.
  if (cfg.glitch_hook or 1.0) >= 0.5 then
    pcall(function()
      Override("IWorldWidgetComponent", "StartGlitching", function(self, intensity, lifetime, wrapped) end)
    end)
  end
  print("[WristHud] ready; " .. (had and "wrist.cfg loaded" or "built-in defaults"))
end)

registerForEvent("onUpdate", function(dt)
  if Game.GetPlayer() == nil then
    S.built = false     -- a load screen: the components start over and so does the content
    S.rearmed = false   -- and the one re-arm is owed again
    S.armedNow = false
    S.fixFor = 0.0
    return
  end

  -- EVERY TRANSITION, NOT ONCE. Drawing or holstering a weapon carries the arm meshes -- ours with them
  -- -- into the other rendering plane, and every one of those transitions leaves the widget's target
  -- dead: the material then samples the engine's default, which is opaque black. Measured: a single
  -- repair holds only until the next swap.
  --
  -- It cannot be avoided instead of repaired. Pinning the plane does not work (clearing the anim param
  -- returns the surface to the SCENE plane, where the arms cover it), the widget's own plane changes
  -- nothing, the mesh cannot be hidden for the moment of the repair (the visibility param is read at
  -- registration, and a component toggle on the MESH destroys the binding for good), and no material in
  -- the game makes an empty UI target invisible on a skinned mesh -- the one truly additive UI template,
  -- simple_additive_ui.mt, declares MVF_MeshStatic only.
  --
  -- So: watch the armed state every frame, and after every change repair for a window, because the
  -- transition lands somewhere inside the draw animation rather than on the frame the slot changes.
  -- THE PLUGIN WATCHES THE THING THAT BREAKS. VRWristGuard reads the world widget's render-target
  -- handle (component+0x220) and toggles the component on the frame that pointer is swapped, which is
  -- the frame the readout turns black. Located by diffing the live object; the whole record is in
  -- src/Natives/WristGuard.cpp. It returns 1 when it repaired something, 0 when there was nothing to do.
  --
  -- Guessing was the problem this replaces: the transition lands inside the draw animation, not on the
  -- frame the weapon slot changes, so a timer was either early, late, or silent.
  -- NOT A REPAIR LOOP ANY MORE. VRWristGuard(4) only REPORTS whether the render-target pointer changed
  -- since the last call -- and that change IS the frame the surface moved rendering plane. So the freeze
  -- happens on that frame instead of after a fixed delay, which is what turns a visible half-second of
  -- black into one frame.
  --
  -- Two earlier versions of this were wrong and both are worth remembering: repairing on every change
  -- toggled the component forever (60 fps -> 35, because the native also re-walked all 243 components
  -- every frame), and reading a rising counter as a "toggle loop" when it was simply one repair per
  -- weapon swap. It is cheap now: the component is cached and validated by its CName, and once the
  -- freeze is done this stops being called at all.
  if S.guard == nil then
    S.guard = (type(VRWristGuard) == "function")
  end

  -- ONCE PER SESSION, AND ENTIRELY IN THE PLUGIN. VRWristGuard(5) knows when a weapon is out from
  -- g_hasWeaponEquipped, which the camera hook already maintains, and it sees the plane transition as the
  -- render-target pointer changing. On that frame it clears the surface's plane param -- freezing it in
  -- the weapon plane -- and re-arms the widget. It answers 2 once it is done, and then this stops.
  --
  -- What this replaced: asking the transaction system for the weapon slot from Lua, per frame, with a
  -- TweakDBID built on every call. That window ran at 24 fps.
  if S.built and S.guard then
    -- BEFORE the freeze: every frame, because the transition is one frame wide and the black lasts until
    -- it is caught. AFTER the freeze: four times a second, because then it is only insurance against the
    -- target dying for some reason we have not seen -- a quarter second of black on an event that may
    -- never happen is a fair price for doing nothing the rest of the time.
    local due = true
    if S.rearmed then
      S.accWatch = (S.accWatch or 0.0) + (dt or 0.016)
      due = S.accWatch >= 0.25
      if due then S.accWatch = 0.0 end
    end
    if due then
      local r = 0
      local ok = pcall(function() r = VRWristGuard(5) end)
      if not ok then
        S.guard = false
      else
        S.guardCode = r
        if r == 1 then
          S.rearmed = true
          S.note = "caught the transition -> frozen and re-armed"
        elseif r == 2 then
          S.rearmed = true
        elseif r == 3 then
          S.repairs = (S.repairs or 0) + 1
          S.note = "the target died again -> re-armed"
        end
      end
    end
  end

  if not S.rearmed and S.built then

    -- THE BACKSTOP, for an older DLL only: the same recipe from Lua, on a timer, with the weapon slot
    -- sampled four times a second rather than per frame.
    if not S.guard then
      local win = cfg.fix_window or 1.5
      S.accArm = (S.accArm or 0.0) + (dt or 0.016)
      if S.accArm >= 0.25 then
        S.accArm = 0.0
        S.armedNow = armed()
      end
      if S.armedNow then
        S.fixFor = (S.fixFor or 0.0) + (dt or 0.016)
        if win > 0.01 and S.fixFor >= win then
          S.rearmed = freezeAndRearm()
          if S.rearmed then S.note = "frozen and re-armed on the backstop timer" end
        end
      else
        S.fixFor = 0.0
      end
    end
  end

  -- HIDING THE SURFACE IS NOT AVAILABLE, and that was measured the hard way: toggling the mesh
  -- component off takes the widget's target with it -- the widget is bound to that very mesh -- so the
  -- readout came back permanently black, and left switched off it read as nothing at all. Nothing here
  -- toggles either half any more.
  -- EVERY FRAME, and this is what the long re-arm was. The repair keeps the ink tree for the frame it
  -- happens on -- measured, 68 children before and after -- but the engine replaces the widget instance
  -- a moment later, and our content goes with the old one. Checked twice a second, that showed as up to
  -- half a second of an empty readout after every weapon swap on top of the black; checked every frame,
  -- the content is back on the next one. The test itself is one GetNumChildren, so it is cheap enough to
  -- run at frame rate; only the transform and the glitch guard stay on the slow cadence.
  if not ours() then
    S.built = false
    local ok, err = pcall(build)
    if not ok then S.note = "build failed: " .. tostring(err) end
  end

  S.accCheck = S.accCheck + (dt or 0.016)
  if S.accCheck >= 0.5 then
    S.accCheck = 0.0
    unglitch()
    transform()
  end
  if not S.built then return end

  if S.dirty then
    layout()
    values()
    icons()
    transform()
  end

  S.accFast = S.accFast + (dt or 0.016)
  if S.accFast >= 0.12 then
    S.accFast = 0.0
    values()
  end
  S.accSlow = S.accSlow + (dt or 0.016)
  if S.accSlow >= 0.50 then
    S.accSlow = 0.0
    icons()
  end
end)

registerForEvent("onOverlayOpen", function() overlay = true end)
registerForEvent("onOverlayClose", function() overlay = false end)

registerForEvent("onDraw", function()
  if not overlay then return end
  pcall(function()
    ImGui.Begin("VR wrist readout")
    if S.built then
      ImGui.Text("on the arm -- " .. S.note)
    else
      ImGui.Text("NOT BUILT -- " .. S.note)
    end
    ImGui.Text("surface: " .. tostring(S.route))
    ImGui.Text("frozen + re-armed: " .. (S.rearmed and "yes" or "not yet (needs a weapon in hand)"))
    if S.guard then
      local walks = -1
      pcall(function() walks = VRWristGuard(3) end)
      ImGui.Text(string.format("detector: plugin, %d walks, %d later re-arm(s)", walks, S.repairs or 0))
    else
      ImGui.Text("transition detector: not in the plugin -- using the backstop timer")
    end
    ImGui.Text("Everything applies as you drag. SAVE writes wrist.cfg.")
    ImGui.Text("Spin and size are widget space and exact. The band group is entity space: it shows a")
    ImGui.Text("number but pulls away as the arm turns -- SAVE it and ask for a bake into the mesh.")
    ImGui.Separator()

    local v, ch, b
    b, ch = ImGui.Checkbox("test values (fake the bar and the ticks)", S.demo)
    if ch then
      S.demo = b
      S.dirty = true
    end
    if S.demo then
      v, ch = ImGui.SliderFloat("fill fraction (above 1.0 is overshield)", S.demoFrac, 0.0, 1.5)
      if ch then
        S.demoFrac = v
        S.dirty = true
      end
    end

    for _, e in ipairs(SPEC) do
      if e.g then
        ImGui.Separator()
        ImGui.Text(e.g)
      elseif e.bool then
        b, ch = ImGui.Checkbox(e.l, (cfg[e.k] or 0.0) >= 0.5)
        if ch then
          cfg[e.k] = b and 1.0 or 0.0
          S.dirty = true
        end
      else
        v, ch = ImGui.SliderFloat(e.l .. "##" .. e.k, cfg[e.k] or e.d, e.lo, e.hi)
        if ch then
          if e.int then
            cfg[e.k] = math.floor(v + 0.5)
          else
            cfg[e.k] = v
          end
          S.dirty = true
        end
      end
    end

    ImGui.Separator()
    if ImGui.Button("SAVE") then
      if saveCfg() then
        S.note = "saved to wrist.cfg"
      else
        S.note = "could not write wrist.cfg"
      end
    end
    ImGui.SameLine()
    if ImGui.Button("reload the file") then
      loadCfg()
      S.dirty = true
      S.note = "re-read wrist.cfg"
    end
    ImGui.SameLine()
    if ImGui.Button("defaults") then
      cfg = defaults()
      S.dirty = true
      S.note = "back to the built-in numbers"
    end
    ImGui.SameLine()
    if ImGui.Button("repair the target now") then
      rebind()
      S.note = "repaired by hand"
    end
    ImGui.SameLine()
    if ImGui.Button("re-arm again") then
      S.rearmed = false
      S.fixFor = 0.0
      S.guard = nil
      pcall(function() VRWristGuard(6) end)   -- and let the plugin owe the freeze again
      S.note = "will freeze and re-arm on the next weapon drawn"
    end
    ImGui.SameLine()
    if ImGui.Button("rebuild") then
      S.built = false
      local ok, err = pcall(build)
      if not ok then S.note = "build failed: " .. tostring(err) end
    end
    ImGui.End()
  end)
end)
