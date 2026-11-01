-- The game's settings as pages of the lobby's menu (modules/holo.lua), built the way the game's own settings screen
-- builds them (SettingsMainGameController.PopulateSettingsData / AddSettingsGroup): the settings system's root groups
-- are the categories, each with its own vars and its non-empty subgroups' vars under their names, as the main menu
-- sees them (GetGroups / GetVars with the pre-game flag). Every var is read from the settings system each time its
-- page is drawn -- a preset changing a dozen others shows them changed.
--
-- A var is a row by its type: a switch, a number on a slider (its own bounds and step), or a list stepped through its
-- values (shown as the game shows them: its display values when it has them, else the value as a text key). Key bindings need a keyboard and are the game's own screen's. A change applies as the game applies it: at once
-- for most, after APPLY for those the game confirms first (the graphics); the settings are saved when they are left,
-- and changes left unapplied are taken back, as the game's screen does when it is closed without confirming.
local M = {}

local SKIP = { ["/key_bindings"] = true }

local function settings() return Game.GetSettingsSystem() end

local function text(key)
  local ok, t = pcall(function() return GetLocalizedTextByKey(key) end)
  return ok and t or ""
end

-- A list value as the game shows it.
local function listValue(v, i)
  local shown = ""
  pcall(function()
    if v:ListHasDisplayValues() then
      shown = GetLocalizedTextByKey(v:GetDisplayValue(i))
    else
      local raw = v:GetValues()[i + 1]
      local s = type(raw) == "userdata" and NameToString(raw) or tostring(raw)
      local loc = GetLocalizedText(s)
      shown = (loc ~= nil and loc ~= "") and loc or s
    end
  end)
  return shown
end

-- The number as its step shows it: as many decimals as the step has.
local function number(v, value)
  local step = 1
  pcall(function() step = v:GetStepValue() end)
  local d = 0
  while d < 3 and math.abs(step * 10 ^ d - math.floor(step * 10 ^ d + 0.5)) > 1e-4 do d = d + 1 end
  return string.format("%." .. d .. "f", value)
end

-- One var as a row, or nil when the game hides it.
local function varRow(v, T, key)
  local visible, disabled = true, false
  pcall(function() visible = v:IsVisible() end)
  pcall(function() disabled = v:IsDisabled() end)
  if not visible then return nil end
  local kind = ""
  pcall(function() kind = tostring(v:GetType()) end)
  local row = { key = key, label = text(v:GetDisplayName()), enabled = not disabled }
  if string.find(kind, "Bool", 1, true) then
    local on = v:GetValue()
    row.value = on and T.on or T.off
    local flip = function() v:SetValue(not v:GetValue()) end
    row.left, row.right = flip, flip
  elseif string.find(kind, "List", 1, true) then
    local n, i = 0, 0
    pcall(function() n = #v:GetValues(); i = v:GetIndex() end)
    row.value = listValue(v, i)
    row.left = function() if v:GetIndex() > 0 then v:SetIndex(v:GetIndex() - 1) end end
    row.right = function() if v:GetIndex() < n - 1 then v:SetIndex(v:GetIndex() + 1) end end
  elseif string.find(kind, "Int", 1, true) or string.find(kind, "Float", 1, true) then
    -- a slider, as the game's own: its bounds and its step
    local isInt = string.find(kind, "Int", 1, true) ~= nil
    row.slider = { min = v:GetMinValue(), max = v:GetMaxValue(), step = v:GetStepValue(), value = v:GetValue(),
                   format = function(x) return number(v, x) end,
                   set = function(x) v:SetValue(isInt and math.floor(x + 0.5) or x) end }
  else
    return nil
  end
  return row
end

-- What waits to be applied, and APPLY: the game's confirmation, and the restart a change needs.
local function pendingRows(T, Holo)
  local rows = {}
  local ss = settings()
  local confirm, restart = false, false
  pcall(function() confirm = ss:NeedsConfirmation() end)
  pcall(function() restart = ss:NeedsRestartToApply() end)
  if confirm then
    local apply = ""
    pcall(function() apply = GetLocalizedText("UI-ResourceExports-Apply") end)
    rows[#rows + 1] = { key = "__apply", label = apply ~= "" and apply or T.apply, detail = T.pending, go = function()
      pcall(function() ss:ConfirmChanges() end)
      pcall(function() Game.GetSystemRequestsHandler():RequestSaveUserSettings() end)
      Holo.refresh()
    end }
  end
  if restart then rows[#rows + 1] = { key = "__restart", label = T.restart, text = true, warn = true } end
  return rows
end

-- Leaving the settings: what was not applied is taken back, the rest saved.
local function leave()
  local ss = settings()
  pcall(function() if ss:NeedsConfirmation() then ss:RejectChanges() end end)
  pcall(function()
    if ss:WasModifiedSinceLastSave() then Game.GetSystemRequestsHandler():RequestSaveUserSettings() end
  end)
end

local function categoryPage(T, Holo, group)
  return {
    title = text(group:GetDisplayName()), code = "SYS//" .. string.upper(string.sub(NameToString(group:GetPath()), 2)),
    rows = function()
      local rows = pendingRows(T, Holo)
      local n = 0
      local function add(v)
        n = n + 1
        local row = varRow(v, T, "v" .. n)
        if row ~= nil then rows[#rows + 1] = row end
      end
      pcall(function() if group:HasVars(true) then for _, v in ipairs(group:GetVars(true)) do add(v) end end end)
      pcall(function()
        for i, sub in ipairs(group:GetGroups(true)) do
          if not sub:IsEmpty(true) and sub:HasVars(true) then
            rows[#rows + 1] = { key = "h" .. i, label = text(sub:GetDisplayName()), header = true }
            for _, v in ipairs(sub:GetVars(true)) do add(v) end
          end
        end
      end)
      return rows
    end,
  }
end

-- The categories, a row each (keys "g1" ..): the root groups with anything to show, for the settings page
-- (modules/mainmenu.lua), which also takes M.pending and, as it is left, M.leave.
function M.rows(Holo, T)
  local rows = {}
  pcall(function()
    for i, g in ipairs(settings():GetRootGroup():GetGroups(true)) do
      local path = NameToString(g:GetPath())
      local shown = false
      if not SKIP[path] then
        pcall(function()
          if g:HasVars(true) then shown = true end
          for _, sub in ipairs(g:GetGroups(true)) do
            if not sub:IsEmpty(true) and sub:HasVars(true) then shown = true end
          end
        end)
      end
      if shown then
        rows[#rows + 1] = { key = "g" .. i, label = text(g:GetDisplayName()),
                            go = function() Holo.push(categoryPage(T, Holo, g)) end }
      end
    end
  end)
  return rows
end

M.pending = pendingRows
M.leave = leave

return M
