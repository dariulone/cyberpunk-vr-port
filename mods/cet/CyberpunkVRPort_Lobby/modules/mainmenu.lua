-- The holographic menu's pages (modules/holo.lua draws them): the game's main menu, the saves to load grouped by
-- playthrough, the quit question, and the way into the new game (modules/creation.lua).
--
-- Each row does what the same row of the game's own main menu does, through that menu's controller
-- (SingleplayerMenuGameController, still running behind the lobby, out of view): CONTINUE is the newest save (of the
-- engram's playthrough when one is in the neck), a save is its LoadSaveInGame with the save's index in the menu's own
-- list (init.lua keeps that list, OnSavesForLoadReady) after the game's mod check, asked on a page here (openSave),
-- SETTINGS are the lobby's own pages: the port's settings first (modules/vrsettings.lua), then the game's
-- (modules/gamesettings.lua) -- the VR menu does not open in the main menu.
-- The saves' details come from the plugin (VRLobbySaveLooks: every save folder's metadata, newest first).
local M = {}
local GameSettings = require("modules/gamesettings")
local VRSettings = require("modules/vrsettings")
local Sound = require("modules/sound")
local Ink = require("modules/ink")
-- The ambience's button on the first page: a loudspeaker giving sound while it plays, silent when it does not.
local function ambientGlyph(parent, x, y, size, col)
  return Ink.speaker(parent, x, y, size, Sound.ambientOn() and 3 or 0, col, 1.0)
end

local TEXTS = {
  en = { vr = "VR settings", continue = "Continue",
         new = "New game", load = "Load game", settings = "Settings", quit = "Quit game", back = "Back",
         quitTitle = "Quit", quitSub = "Close the game?", yes = "Quit", no = "Stay", saves = "Saves",
         savesSub = "Playthroughs, newest first", level = "LVL", none = "No saves",
         modTitle = "Mod Verification", mod1 = "This save file was made on a version of the game with mods.",
         mod2 = "Loading it on this system may cause unexpected behavior in the game.",
         mod3 = "Are you sure you want to continue?", confirm = "Confirm", cancel = "Cancel",
         on = "On", off = "Off", apply = "Apply", game = "Game", pending = "Changes waiting to be applied",
         restart = "Some changes take effect after the game restarts", reading = "Reading the settings...",
         vrMissing = "The VR menu's mod is not loaded",
         life = { Nomad = "Nomad", StreetKid = "Street Kid", Corporate = "Corpo", Count = "Phantom Liberty" } },
  ru = { vr = "Настройки VR", continue = "Продолжить",
         new = "Новая игра", load = "Загрузить игру", settings = "Настройки", quit = "Выход", back = "Назад",
         quitTitle = "Выход", quitSub = "Закрыть игру?", yes = "Выйти", no = "Остаться", saves = "Сохранения",
         savesSub = "Прохождения, новые первыми", level = "УР.", none = "Нет сохранений",
         modTitle = "Проверка модификаций", mod1 = "Этот файл сохранения создан в модифицированной версии игры.",
         mod2 = "Его загрузка может привести к ошибкам в этой системе.", mod3 = "Продолжить?",
         confirm = "Подтвердить", cancel = "Отменить",
         on = "Вкл.", off = "Выкл.", apply = "Применить", game = "Игра", pending = "Изменения ждут применения",
         restart = "Часть изменений вступит в силу после перезапуска игры", reading = "Чтение настроек...",
         vrMissing = "Мод VR-меню не загружен",
         life ={ Nomad = "Кочевник", StreetKid = "Дитя улиц", Corporate = "Корпорат", Count = "Призрачная свобода" } },
}

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

function M.lang() return string.sub(language(), 1, 2) == "ru" and "ru" or "en" end
function M.texts() return TEXTS[M.lang()] end

-- The game's own text for a key, ours where the game has none.
local function label(key, fallback)
  local ok, t = pcall(function() return Game.GetLocalizedText(key) end)
  if ok and t ~= nil and t ~= "" and t ~= key then return t end
  return fallback
end
M.label = label

---------------------------------------------------------------- the game's menu

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
M.activate = activate

---------------------------------------------------------------- the saves

-- Every save folder, newest first, and the playthroughs they belong to (VRLobbySaveLooks). Read once -- the plugin
-- reads a metadata file a save for it -- and again only when the game's menu sends its list of saves anew (a save
-- made or deleted since).
local cache = { saves = nil, order = nil, names = nil }
local function readSaves()
  local names = M.shared and M.shared.saveNames or false
  if cache.saves ~= nil and cache.names == names then return cache.saves, cache.order end
  local saves, byId, order = {}, {}, {}
  cache.saves, cache.order, cache.names = saves, order, names
  local ok, text = pcall(function() return type(VRLobbySaveLooks) == "function" and VRLobbySaveLooks() or "" end)
  if not ok or text == nil then return saves, order end
  for line in text:gmatch("[^\n]+") do
    local f = {}
    for field in (line .. "\t"):gmatch("([^\t]*)\t") do f[#f + 1] = field end
    if f[1] == "save" then
      local s = { folder = f[2], playthrough = f[3], life = f[4], gender = f[5], level = tonumber(f[6]) or 0,
                  time = tonumber(f[7]) or 0, location = f[8], quest = f[9], played = tonumber(f[10]) or 0,
                  modded = f[11] ~= "0" }
      saves[#saves + 1] = s
      local p = byId[s.playthrough]
      if p == nil then
        p = { id = s.playthrough, saves = {}, newest = s }
        byId[s.playthrough] = p
        order[#order + 1] = p
      end
      p.saves[#p.saves + 1] = s
    end
  end
  return saves, order
end

local function when(t)
  if t == nil or t <= 0 then return "" end
  return os.date("%d.%m.%Y %H:%M", t)
end

-- The save's index in the menu's own list (newest first), the one LoadModdedSave takes; before the menu has sent
-- its list, the index in the plugin's (newest first too).
local function menuIndex(folder)
  local names = M.shared and M.shared.saveNames
  if names == nil then
    for i, s in ipairs(readSaves()) do
      if s.folder == folder then return i - 1 end
    end
    return nil
  end
  for i, name in ipairs(names) do
    if tostring(name) == folder then return i - 1 end
  end
  return nil
end

-- A save's picture, the one the game's own load screen shows: the engine reads it into an image widget
-- (RequestSavedGameScreenshot with the save's index in the menu's list), one a save and a use (a widget is drawn in
-- one place: a playthrough's row and its newest save's are on two panels at once), kept for the main menu's life.
-- None before the menu has sent its list: the index would be the plugin's, not the engine's.
local thumbs = {}
local function thumb(folder, use)
  local key = folder .. "/" .. (use or "")
  local img = thumbs[key]
  if img ~= nil or M.shared == nil or M.shared.saveNames == nil then return img end
  local index = menuIndex(folder)
  if index == nil then return nil end
  pcall(function()
    img = inkImageWidget.new()
    Game.GetSystemRequestsHandler():RequestSavedGameScreenshot(index, img)
  end)
  thumbs[key] = img
  return img
end

-- The save loaded straight away: the menu's LoadSaveInGame, which in the main menu loads (sub_1409AAB70 -> the load
-- its mod check's CONFIRM runs, sub_14223AB10). Its LoadModdedSave would first check the save's mods and ask on the
-- game's 2D notification layer, out of the lobby's reach -- the lobby asks itself (openSave).
local function loadSave(folder)
  local ctrl = M.shared and M.shared.mainMenu
  local index = menuIndex(folder)
  if ctrl == nil or index == nil then
    M.lastError = "save " .. tostring(folder) .. " is not in the menu's list"
    return false
  end
  return pcall(function() ctrl:LoadSaveInGame(index) end)
end

-- The player's Gameplay setting "mod verification" (/gameplay/misc EnableModVerification): ask before a save made
-- with mods is loaded.
local function verifiesMods()
  local on = true
  pcall(function()
    local group = Game.GetSettingsSystem():GetGroup("/gameplay/misc")
    local name = CName.new("EnableModVerification")
    if group:HasVar(name) then on = group:GetVar(name):GetValue() == true end
  end)
  return on
end

local function placeName(s)
  local name = s.location ~= nil and s.location ~= "" and label(s.location, nil) or nil
  return name or s.folder
end

-- The game's mod check (system_notifications.inkwidget LoadModdedSaveFilePC: its LocKeys), as a page of its own.
local function modCheckPage(T, Holo, s)
  return {
    title = label("LocKey#98280", T.modTitle), subtitle = placeName(s) .. "  " .. when(s.time), code = "SYS//MODS",
    rows = function()
      return {
        { key = "m1", text = true, label = label("LocKey#98259", T.mod1) },
        { key = "m2", text = true, warn = true, label = label("LocKey#98312", T.mod2) },
        { key = "m3", text = true, label = label("LocKey#98261", T.mod3) },
        { key = "cancel", label = label("LocKey#22175", T.cancel), go = function() Holo.back() end },
        { key = "confirm", label = label("LocKey#23123", T.confirm), go = function() loadSave(s.folder) end },
      }
    end,
  }
end

-- A save chosen: loaded, or first the mod check when the player has it on and the save was made with mods.
local function openSave(T, Holo, s)
  if s.modded and verifiesMods() then
    Holo.push(modCheckPage(T, Holo, s))
    return true
  end
  return loadSave(s.folder)
end
M.openSave = openSave

-- A playthrough's life path, as the game's load screen pictures it (its atlas, the parts the VR menu's load screen
-- takes too).
local LIFE_ATLAS = "base\\gameplay\\gui\\fullscreen\\load_game\\save_game.inkatlas"
local LIFE_ICONS = { Nomad = "LifepathNomad1", Corporate = "LifepathCorpo1", StreetKid = "LifepathStreetKid1" }

-- The playthroughs, newest first, as the VR menu's load screen has them: a playthrough's row (its life path's icon)
-- opens and closes its saves under it, in the list (the newest playthrough open to begin with); a save's row (its
-- picture) loads it.
local function loadPage(T, Holo)
  local page = { title = T.saves, subtitle = T.savesSub, code = "SAVE//INDEX", open = nil }
  page.rows = function()
    local _, order = readSaves()
    if page.open == nil and order[1] ~= nil then page.open = { [order[1].id] = true } end
    local rows = {}
    for i, p in ipairs(order) do
      local s = p.newest
      local opened = page.open ~= nil and page.open[p.id] == true
      rows[#rows + 1] = { key = "p" .. i, label = (T.life[s.life] or s.life) .. " · " .. placeName(s),
                          detail = T.level .. " " .. s.level .. "  " .. when(s.time) .. "  ·  " .. #p.saves,
                          sign = opened and "-" or "+", icon = LIFE_ICONS[s.life] and { LIFE_ATLAS, LIFE_ICONS[s.life] },
                          image = not LIFE_ICONS[s.life] and thumb(s.folder, "playthrough") or nil,
                          go = function() page.open[p.id] = not opened end }
      if opened then
        for j, save in ipairs(p.saves) do
          rows[#rows + 1] = { key = "p" .. i .. "s" .. j, label = placeName(save), indent = 90,
                              detail = T.level .. " " .. save.level .. "  " .. when(save.time), image = thumb(save.folder),
                              go = function() openSave(T, Holo, save) end }
        end
      end
    end
    if #rows == 0 then rows[1] = { key = "none", label = T.none, enabled = false } end
    return rows
  end
  return page
end

-- The settings: the port's first (its panel's pages), then the game's categories; what waits for the game's
-- confirmation on top. Left, the game's settings are applied or taken back and saved (modules/gamesettings.lua).
local function settingsPage(T, Holo)
  return {
    title = label("UI-Labels-Settings", T.settings), code = "SYS//SETTINGS", onBack = GameSettings.leave,
    rows = function()
      local rows = GameSettings.pending(T, Holo)
      rows[#rows + 1] = { key = "hvr", label = T.vr, header = true }
      for _, r in ipairs(VRSettings.rows(Holo, T)) do rows[#rows + 1] = r end
      rows[#rows + 1] = { key = "hgame", label = T.game, header = true }
      for _, r in ipairs(GameSettings.rows(Holo, T)) do rows[#rows + 1] = r end
      return rows
    end,
  }
end

local function quitPage(T, Holo)
  return {
    title = T.quitTitle, subtitle = T.quitSub, code = "SYS//EXIT",
    rows = function()
      return {
        { key = "no", label = T.no, go = function() Holo.back() end },
        { key = "yes", label = T.yes, go = function() activate(PauseMenuAction.ExitGame) end },
      }
    end,
  }
end

---------------------------------------------------------------- the first page

-- Holo: modules/holo.lua; Creation: modules/creation.lua (its open() pushes the new game's pages); Engrams:
-- modules/engrams.lua (CONTINUE goes on with the playthrough of the engram in the neck, the newest save without one).
function M.root(Holo, Creation, Engrams)
  local T = M.texts()
  return {
    -- the game's main menu's layout: the rows on the left, the save CONTINUE goes on with pictured on the right while
    -- it is pointed at
    logo = true, code = "VR//LOBBY",
    -- the lobby's ambience on or off (modules/sound.lua), kept with the port's settings
    footer = function()
      return { { key = "ambient", glyph = ambientGlyph,
                 go = function() Sound.setAmbientOn(not Sound.ambientOn()) end } }
    end,
    rows = function()
      local saves = readSaves()
      local newest = saves[1]
      local engram = Engrams ~= nil and Engrams.inserted() or nil
      if engram ~= nil then
        for _, s in ipairs(saves) do
          if s.folder == engram.folder then newest = s end
        end
      end
      return {
        { key = "continue", label = label("UI-ScriptExports-Continue0", T.continue), enabled = newest ~= nil,
          preview = newest and thumb(newest.folder, "continue") or nil,
          previewText = newest and ((T.life[newest.life] or newest.life) .. " · " .. placeName(newest)) or nil,
          previewDetail = newest and (T.level .. " " .. newest.level .. "  " .. when(newest.time)) or nil,
          go = function() if newest ~= nil then openSave(T, Holo, newest) end end },
        { key = "new", label = label("UI-ScriptExports-NewGame0", T.new),
          go = function() if Creation ~= nil then Creation.open() end end },
        { key = "load", label = label("UI-ScriptExports-LoadGame0", T.load), enabled = newest ~= nil,
          go = function() Holo.push(loadPage(T, Holo)) end },
        { key = "settings", label = label("UI-Labels-Settings", T.settings),
          go = function() Holo.push(settingsPage(T, Holo)) end },
        { key = "quit", label = label("UI-Labels-CloseGame", T.quit), go = function() Holo.push(quitPage(T, Holo)) end },
      }
    end,
  }
end

function M.init(shared) M.shared = shared end
M.readSaves = readSaves

return M
