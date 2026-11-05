-- The port's settings as data: the pages of the ImGui panel, read out by the plugin, for whatever draws them -- the
-- VR menu's settings section (modules/settings.lua) and the lobby's menu in the main menu (CyberpunkVRPort_Lobby
-- modules/vrsettings.lua, through GetMod("CyberpunkVRPort_Menu").settingsData). VRSettingsPage(page) asks the plugin to
-- record a page at its next present and returns the newest recording; VRSettingsEdit(page, id, value) hands an item a
-- new value, which the panel's own code applies -- and saves to the ini -- the way a click or a drag on the panel would
-- have. The page string is described in include/Overlay/UiRec.hpp; its English is shown in the menu's language
-- (modules/locale.lua L.setting).
local L = require("modules/locale")
local Data = require("modules/data")

local D = {}

D.PAGES = { "GENERAL", "FRAMEGEN", "HUD", "CONTROLS", "STEREO", "AVATAR", "OVERLAY" }
local PAGES_RU = { "ОБЩИЕ", "ГЕН. КАДРОВ", "HUD", "УПРАВЛЕНИЕ", "СТЕРЕО", "АВАТАР", "ОВЕРЛЕЙ" }

-- The translations in the game's on-screen language, for a reader the VR menu has not set them for (the lobby's menu:
-- the VR menu sets them as it opens).
function D.useGameLanguage() L.set(Data.language()) end

-- Page i (1-based, the panel's tab order) by name, in the menu's language.
function D.pageName(i) return L.tr(PAGES_RU[i], D.PAGES[i]) end

-- flags: 1 disabled-style text, 2 wrapped text, 4 coloured text (value = 0xRRGGBB), 8 header open by default, 16 item
-- disabled.
function D.has(flags, bit) return math.floor(flags / bit) % 2 == 1 end

function D.split(s, sep)
  local out, i = {}, 1
  while true do
    local j = string.find(s, sep, i, true)
    if j == nil then
      out[#out + 1] = string.sub(s, i)
      return out
    end
    out[#out + 1] = string.sub(s, i, j - 1)
    i = j + 1
  end
end

---------------------------------------------------------------- the page string

-- The panel's English in the menu's language (L.setting): labels, tooltips, texts, and a combo's or tabs'
-- options one by one.
local function translate(text, options)
  if not options then return L.setting(text) end
  local out = D.split(text, "\29")
  for i, o in ipairs(out) do out[i] = L.setting(o) end
  return table.concat(out, "\29")
end

function D.parse(str)
  if type(str) ~= "string" or str == "" then return nil end
  local recs = D.split(str, "\30")
  local head = D.split(recs[1], "\31")
  if head[1] ~= "#" then return nil end
  local pg = { name = head[3], page = tonumber(head[4]) or -1, seq = tonumber(head[5]) or 0, items = {} }
  for i = 2, #recs do
    local f = D.split(recs[i], "\31")
    local kind = f[1]
    pg.items[#pg.items + 1] = {
      kind = kind, id = f[2] or "", label = translate(f[3] or ""), value = tonumber(f[4]) or 0,
      min = tonumber(f[5]) or 0, max = tonumber(f[6]) or 0, text = translate(f[7] or "", kind == "O" or kind == "P"),
      flags = tonumber(f[8]) or 0, tip = translate(f[9] or ""), indent = tonumber(f[10]) or 0, page = pg.page,
    }
  end
  return pg
end

-- A combo's or tabs' options.
function D.options(f) return D.split(f.text or "", "\29") end

-- The rows a page shows: a folded section hides everything up to the next section of its level. isOpen(f) says
-- whether header f is open.
function D.visible(items, isOpen)
  local out, fold = {}, nil
  for _, f in ipairs(items) do
    if fold ~= nil and f.kind == "H" and f.indent <= fold then fold = nil end
    if fold == nil then
      out[#out + 1] = f
      if f.kind == "H" and not isOpen(f) then fold = f.indent end
    end
  end
  return out
end

---------------------------------------------------------------- numbers and text

function D.decimals(s)
  local frac = string.match(s or "", "%d%.(%d+)")
  return frac and #frac or 0
end

function D.nice(x)
  if x <= 0 then return 1 end
  local p = 10 ^ math.floor(math.log10(x))
  local m = x / p
  if m < 1.5 then return p elseif m < 3.5 then return 2 * p elseif m < 7.5 then return 5 * p end
  return 10 * p
end

-- A slider's grain: `steps` steps over its range (two hundred, a drag's), rounded to 1 / 2 / 5, never finer than the
-- panel shows the value (an int slider steps by whole numbers at least).
function D.stepOf(f, steps)
  local span = math.abs(f.max - f.min)
  steps = steps or 200
  if f.kind == "I" then return math.max(1, math.floor(D.nice(span / steps) + 0.5)) end
  return math.max(10 ^ -D.decimals(f.text), D.nice(span / steps))
end

function D.snap(f, v, lo, step)
  v = lo + math.floor((v - lo) / step + 0.5) * step
  if f.kind == "I" then return math.floor(v + 0.5) end
  return tonumber(string.format("%.6f", v))
end

-- The value as the panel would show it: its number swapped into the panel's own text, so a unit or a
-- suffix the format carries stays.
function D.fmt(f, v)
  local pre, num, post = string.match(f.text or "", "^(.-)(%-?%d+%.?%d*)(.*)$")
  local s = f.kind == "I" and tostring(math.floor(v + 0.5)) or string.format("%." .. D.decimals(num or "0.00") .. "f", v)
  if num == nil then return s end
  return pre .. s .. post
end

function D.ulen(s)
  local n = 0
  for _ in string.gmatch(s, "[%z\1-\127\194-\244][\128-\191]*") do n = n + 1 end
  return n
end

-- Lines of at most n characters, broken at spaces (a word longer than a line runs long). The screens' text widgets
-- do not wrap by themselves.
function D.wrap(s, n)
  local lines = {}
  for para in string.gmatch((s or "") .. "\n", "(.-)\n") do
    local line, len = "", 0
    for word in string.gmatch(para, "%S+") do
      local wl = D.ulen(word)
      if len > 0 and len + 1 + wl > n then
        lines[#lines + 1] = line
        line, len = word, wl
      elseif len > 0 then
        line, len = line .. " " .. word, len + 1 + wl
      else
        line, len = word, wl
      end
    end
    lines[#lines + 1] = line
  end
  while #lines > 1 and lines[#lines] == "" do lines[#lines] = nil end
  return lines
end

---------------------------------------------------------------- talking to the plugin

-- The newest recording of page `page` (0-based), the string D.parse reads; the next present records it again.
function D.fetch(page)
  if type(VRSettingsPage) ~= "function" then return nil end
  local ok, s = pcall(VRSettingsPage, page)
  return ok and s or nil
end

function D.edit(f, value)
  if type(VRSettingsEdit) ~= "function" then return end
  pcall(VRSettingsEdit, f.page, f.id, value)
end

return D
