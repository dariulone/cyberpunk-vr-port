-- The port's settings as pages of the lobby's menu (modules/holo.lua): the ImGui panel's pages as the plugin reads them
-- out -- the VR menu's settings data (CyberpunkVRPort_Menu modules/settings_data.lua, through its GetMod), so the lobby
-- and the VR menu show one list: the same rows, ranges, disabled states and translations, and an edit goes to the
-- panel's own code, which applies it and saves the ini. The VR menu itself does not open in the main menu.
--
-- A page is polled while it is on top (a recording a quarter of a second, VRSettingsPage); a new recording draws it
-- again. Its rows by the item's kind: a section's header folds it (the panel's own default, then the player's), a
-- checkbox switches, a slider is a slider (the VR menu's grain), a combo or a row of tabs steps through its options, a
-- number field by its own step, a button presses; text, progress and binding rows are text.
local M = {}

local POLL = 0.25
local LINE = 64                          -- characters of a text row

local function data()
  local d = nil
  pcall(function() d = GetMod("CyberpunkVRPort_Menu").settingsData end)
  return d
end

local function itemRows(D, page, f, T, n, rows)
  local key = "i" .. n
  local dim = D.has(f.flags, 16)
  local function set(v)
    f.value = v
    D.edit(f, v)
  end
  local k = f.kind
  if k == "H" then
    local open = page.isOpen(f)
    rows[#rows + 1] = { key = key, label = f.label, header = true, detail = open and "-" or "+",
                        go = function() page.open[f.id] = not open end }
  elseif k == "C" then
    rows[#rows + 1] = { key = key, label = f.label, enabled = not dim, value = f.value ~= 0 and T.on or T.off,
                        left = function() set(f.value ~= 0 and 0 or 1) end,
                        right = function() set(f.value ~= 0 and 0 or 1) end }
  elseif k == "S" or k == "I" then
    -- a slider, with the VR menu's grain (two hundred steps over its range)
    local step = D.stepOf(f)
    local lo = math.min(f.min, f.max)
    rows[#rows + 1] = { key = key, label = f.label, enabled = not dim,
                        slider = { min = f.min, max = f.max, step = step, value = f.value,
                                   format = function(v) return D.fmt(f, v) end,
                                   set = function(v) set(D.snap(f, v, lo, step)) end } }
  elseif k == "N" then
    local step = (f.min ~= 0) and f.min or 1
    rows[#rows + 1] = { key = key, label = f.label, enabled = not dim, value = tostring(math.floor(f.value + 0.5)),
                        left = function() set(f.value - step) end, right = function() set(f.value + step) end }
  elseif k == "O" or k == "P" then
    local opts = D.options(f)
    local idx = math.floor(f.value + 0.5)
    local function pick(d) if #opts > 0 then set((idx + d) % #opts) end end
    rows[#rows + 1] = { key = key, label = f.label, enabled = not dim, value = opts[idx + 1] or "?",
                        left = function() pick(-1) end, right = function() pick(1) end }
  elseif k == "B" then
    rows[#rows + 1] = { key = key, label = f.label, enabled = not dim, go = function() D.edit(f, 1) end }
  elseif k == "T" then
    for j, line in ipairs(D.wrap(f.text, LINE)) do
      rows[#rows + 1] = { key = key .. "." .. j, label = line, text = true }
    end
  elseif k == "G" or k == "K" then
    rows[#rows + 1] = { key = key, label = f.label .. "  " .. (f.text or ""), text = true }
  end
end

-- Page i (1-based) of the panel.
local function sectionPage(D, T, i)
  local page = { title = D.pageName(i), code = "VR//" .. D.PAGES[i], open = {}, str = nil, rec = nil, pollAt = 0 }
  function page.isOpen(f)
    local o = page.open[f.id]
    if o == nil then o = D.has(f.flags, 8) end
    return o
  end
  local function poll()
    local str = D.fetch(i - 1)
    if str == nil or str == page.str then return false end
    local pg = D.parse(str)
    if pg == nil or pg.page ~= i - 1 then return false end
    page.str, page.rec = str, pg
    return true
  end
  function page.update(_, now)
    if now < page.pollAt then return false end
    page.pollAt = now + POLL
    return poll()
  end
  function page.rows()
    if page.rec == nil then poll() end
    if page.rec == nil then return { { key = "wait", label = T.reading, text = true } } end
    local rows = {}
    for n, f in ipairs(D.visible(page.rec.items, page.isOpen)) do itemRows(D, page, f, T, n, rows) end
    return rows
  end
  return page
end

-- The panel's pages, a row each (keys "vr1" ..), for the settings page (modules/mainmenu.lua).
function M.rows(Holo, T)
  local D = data()
  if D == nil then return { { key = "vrnone", label = T.vrMissing, text = true, warn = true } } end
  D.useGameLanguage()
  local rows = {}
  for i = 1, #D.PAGES do
    rows[#rows + 1] = { key = "vr" .. i, label = D.pageName(i), go = function() Holo.push(sectionPage(D, T, i)) end }
  end
  return rows
end

return M
