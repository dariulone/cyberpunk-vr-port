-- The new game, made in the tutorial hall. Its first page has the life paths on cards; the one chosen, the panel grows
-- into a window a little larger than V at full height (modules/holo.lua M.window): the figure (modules/figure.lua)
-- stands behind its right part in the clothes of its life path, seen through it, and its rows choose what the game's
-- own new game screens choose -- the start, the difficulty, the life path, the body, the voice, every option of the
-- looks, the first attribute points -- then CREATE AND CONTINUE starts the game. BACK on the window shrinks it back
-- into the life paths, the figure gone.
--
-- THE GAME'S SCREENS RUN BEHIND IT. The character creation system has its options only while the game's own looks
-- screen is up (MenuScenario_CharacterCustomization), so pressing NEW GAME walks the game's new game screens there
-- out of view, each step what its screen does on its button (ExpansionNewGame: SetIsExpansionStandalone;
-- DifficultySelectionMenu: SetDifficulty; CharacterCreationBackstorySelectionMenu: SetLifePath;
-- CharacterCreationGenderSelectionMenu: the body and the brain's gender), stepped on the screens' dispatcher
-- (init.lua keeps it: OnSetMenuEventDispatcher) and each step waiting for the next screen's controller. The system
-- then dresses the figure instead of the game's preview puppet, which stands far from the hall (VRCreationTarget),
-- and its UI preset of the life path dresses it whole. A body changed later goes back to the body screen and comes
-- through it again with a figure of the other body. Leaving goes back through the screens to the main menu.
--
-- THE LOBBY STAYS UP. Leaving the main menu's root closes its controller, which takes the main menu as no longer up
-- (VREngineStereoMenuClosed) and the lobby with it: the new game holds one of its own while it runs
-- (VREngineStereoMenuShown, given back when it ends), and tells the lobby not to step aside (shared.creating).
local M = {}
local Figure = require("modules/figure")

local TEXTS = {
  en = { title = "New game", sub = "A body for the construct", pickSub = "Where the story begins", code = "CC//NEW", mode = "Start", base = "Night City",
         ep1 = "Phantom Liberty", difficulty = "Difficulty", life = "Life path", body = "Body", voice = "Voice",
         male = "Masculine", female = "Feminine", looks = "Appearance", stats = "Attributes", start = "Create and continue",
         busy = "Working...", presets = "Clothes", random = "Randomize", points = "Points left", reset = "Reset",
         looksSub = "Every option of the game's screen", statsSub = "First attribute points", fixed = "Set by Phantom Liberty",
         diff = { "Story", "Easy", "Hard", "Very hard" }, attrs = { "Body", "Reflexes", "Technical Ability", "Intelligence", "Cool" } },
  ru = { title = "Новая игра", sub = "Тело для конструкта", pickSub = "С чего начнётся история", code = "CC//NEW", mode = "Начало", base = "Найт-Сити",
         ep1 = "Призрачная свобода", difficulty = "Сложность", life = "Предыстория", body = "Тело", voice = "Голос",
         male = "Мужское", female = "Женское", looks = "Внешность", stats = "Характеристики", start = "Создать и продолжить",
         busy = "Подождите...", presets = "Одежда", random = "Случайно", points = "Осталось очков", reset = "Сбросить",
         looksSub = "Все параметры экрана игры", statsSub = "Первые очки характеристик", fixed = "Задано «Призрачной свободой»",
         diff = { "Сюжет", "Лёгкий", "Сложный", "Очень сложный" }, attrs = { "Тело", "Рефлексы", "Техника", "Интеллект", "Хладнокровие" } },
}

-- The life paths, each with its picture on the game's own life path screen (CharacterCreationBackstorySelectionMenu).
local LIFE = {
  { id = "LifePaths.Nomad", key = "LocKey#1799", preset = "nomad", attrs = "UIAttributePresets.Preset1", en = "Nomad", ru = "Кочевник",
    poster = "lifepath_nomad" },
  { id = "LifePaths.StreetKid", key = "LocKey#1801", preset = "street", attrs = "UIAttributePresets.Preset2", en = "Street Kid", ru = "Дитя улиц",
    poster = "lifepath_street" },
  { id = "LifePaths.Corporate", key = "LocKey#1800", preset = "corpo", attrs = "UIAttributePresets.Preset3", en = "Corpo", ru = "Корпорат",
    poster = "lifepath_corpo" },
}
local LIFE_ATLAS = "base\\gameplay\\gui\\fullscreen\\main_menu\\character_creation_step_5.inkatlas"
local LIFE_POSTER = 747 / 1088   -- the pictures' width over their height
-- The figure stands this far behind the window (m): close enough to be seen in it at full height.
local FIGURE_BEHIND = 0.25
-- THE FIGURE STEPS UP TO THE PART. As the game's camera goes to the part of the body an option is about, on the option
-- pointed at (characterCreationBodyMorphMenu GetSlotName: the head's options to the face, the arms' to the nails, the
-- rest the whole body), the figure comes out through the window toward the player's spot: this far from it (m) for
-- the face and for the hands, its near side (FIGURE_HALF, arms hanging) ZOOM_CLEAR deg right of the rows' edge. A row
-- pointed at ZOOM_DWELL s sends it; another page sends it back behind the window.
local ZOOM = { face = 0.95, hands = 1.2 }
local FIGURE_HALF, ZOOM_CLEAR = 0.3, 3.0
local ZOOM_DWELL = 0.3
local DIFFICULTY = { "Story", "Easy", "Hard", "VeryHard" }
local ATTRS = { "Strength", "Reflexes", "TechnicalAbility", "Intelligence", "Cool" }
local STEP_TIMEOUT = 10.0
-- THE EYES SHUT OVER THE BODY SCREEN. Coming up, the game's looks screen opens its preview puppet's menu
-- (MenuScenario_CharacterCustomization: player_puppet), and the frame after it holds the game thread ~0.45 s on the
-- preview's jobs (2026-10-31: every time, framegen on or off). Without that menu the system has no options, so the
-- eyes are shut over it instead: closed before the step that brings the screen up, open again once it is past (s).
local SHUT_TIME, OPEN_TIME = 0.15, 0.3
-- BARE WHEN THE GAME ALLOWS IT. A life path's preset sets the genitals option to Off, and Off is the underwear (the
-- system equips Items.Underwear_Basic_01_Bottom with it). With nudity allowed the figure is shown bare instead: the
-- option is set to the body's own (GENITALS: its name among the option's) once the preset is on -- its underwear in
-- the slot; a change made while the preset still goes on is undone by it -- looked at every BARE_EVERY s for BARE_FOR
-- s after each preset. Off chosen on its row later stays.
local BARE_EVERY, BARE_FOR = 0.25, 8.0
local GENITALS = { [true] = "penis01", [false] = "vagina" }
local UNDERWEAR = "AttachmentSlots.UnderwearBottom"

-- The choices made so far; the job walking the game's screens; whether the window is up and its figure dressed; how
-- far the eyes are shut (dim), where they are going (dimTo) and whether a long frame has come since the body step;
-- the part the figure shows (zoom), the one a row pointed at wants and since when, and the page it was sent on; when
-- a preset's underwear is next looked at to come off (bareAt), till when (bareUntil); per option, the value last
-- asked of it and when (chosen).
local S = { open = false, held = false, ep1 = false, standalone = false, difficulty = 3, life = 1, male = true,
            job = nil, error = nil, lastChange = 0, time = 0, windowOpen = false, dressed = false, dim = 0, dimTo = 0,
            stalled = false, zoom = "body", zoomWant = nil, zoomSince = 0, zoomPage = nil, bareAt = nil, bareUntil = 0,
            chosen = {} }

local Holo, Mainmenu, shared, scene = nil, nil, nil, nil

local function lang() return Mainmenu ~= nil and Mainmenu.lang() or "en" end
local function T() return TEXTS[lang()] end

---------------------------------------------------------------- the game's screens

local function system() return Game.GetCharacterCustomizationSystem() end
local function state() return system():GetState() end

-- The class of the game's new game screen that is up, or nil.
local function screen()
  local c = shared and shared.creation
  if c == nil then return nil end
  local ok, name = pcall(function()
    if not IsDefined(c) then return nil end
    return c:GetClassName().value
  end)
  return ok and name or nil
end

local function event(name)
  local d = shared and shared.creationEvents
  if d == nil then return false end
  return pcall(function() d:SpawnEvent(CName.new(name)) end)
end

local function hold(on)
  if S.held == on then return end
  S.held = on
  pcall(on and VREngineStereoMenuShown or VREngineStereoMenuClosed)
  if shared ~= nil then shared.creating = on end
end

-- A job: steps run one after another, each its act once and then waiting for its condition (or a time). `kind`
-- names it for what may stop it.
local function run(steps, done, kind)
  S.job = { steps = steps, i = 1, t = 0, started = false, done = done, kind = kind }
  S.error = nil
  -- A job that replaced one (a leave, a failure's way back) opens the eyes the other one shut.
  S.dimTo = 0
end

local function stepJob(dt)
  local J = S.job
  if J == nil then return end
  local st = J.steps[J.i]
  if st == nil then
    S.job = nil
    if J.done ~= nil then pcall(J.done) end
    if Holo ~= nil then Holo.refresh() end
    return
  end
  if not J.started then
    J.started, J.t = true, 0
    if st.act ~= nil then
      local ok, err = pcall(st.act)
      if not ok then S.error = st.name .. ": " .. tostring(err) end
    end
  end
  J.t = J.t + dt
  local ok, ready = true, true
  if st.wait ~= nil then ok, ready = pcall(st.wait, J.t) end
  if ok and ready and J.t >= (st.min or 0) then
    J.i, J.started = J.i + 1, false
  elseif J.t > (st.timeout or STEP_TIMEOUT) then
    S.error = (st.name or "step") .. " timed out on " .. tostring(screen())
    S.job = nil
    if J.fail ~= nil then pcall(J.fail) end
    if Holo ~= nil then Holo.refresh() end
  end
end

local function on(class) return function() return screen() == class end end

-- The steps either side of the ones that bring the body screen up (from the life path screen: the body choice
-- screen between holds the thread ~0.1 s too): the eyes shut first, open once it has come through.
local function shutStep()
  return { name = "shut", act = function() S.dimTo = 1 end, wait = function() return S.dim >= 1 end,
           min = SHUT_TIME + 0.05 }
end
-- Open once the screen's long frame has been (S.stalled, set from the body step on), or after a second without one.
local function openStep()
  return { name = "open", wait = function(t)
    if not (S.stalled or t > 1.0) then return false end
    S.dimTo = 0
    return true
  end }
end

-- The attributes of the life path's preset (what the body screen sets as it opens), or Phantom Liberty's build.
local function presetAttributes()
  local st = state()
  pcall(function()
    if S.standalone then
      local build = TweakDB:GetRecord("ProgressionBuilds.EP1_Standalone_StartingBuild")
      for _, a in ipairs(build:StartingAttributes()) do st:SetAttribute(a:Attribute():StatType(), a:Level()) end
    else
      st:SetAttributePointsAvailable(7)
      local preset = TweakDB:GetRecord(LIFE[S.life].attrs)
      for _, a in ipairs(preset:Attributes()) do st:SetAttribute(a:Attribute():StatType(), a:Value()) end
    end
  end)
end

-- The life path's preset on the figure; its underwear off once it is on, if nudity is allowed (M.update: bare).
local function preset()
  S.chosen = {}
  pcall(function() system():ApplyUIPreset(CName.new(LIFE[S.life].preset)) end)
  S.bareAt, S.bareUntil = S.time + BARE_EVERY, S.time + BARE_FOR
end

-- True once there is nothing more to do: nudity not allowed, or the genitals option not Off and no underwear on.
local function bare()
  local done = true
  pcall(function()
    local sys = system()
    if not sys:IsNudityAllowed() then return end
    local figure = Figure.entity()
    local dressed = figure ~= nil and Game.GetTransactionSystem():GetItemInSlot(figure, TweakDBID.new(UNDERWEAR)) ~= nil
    for _, o in ipairs(sys:GetUnitedOptions(true, true, true)) do
      if o.info.name.value == "genitals" then
        done = o.currIndex ~= 0 and not dressed
        if o.currIndex ~= 0 or not dressed then return end
        for i, op in ipairs(o.info.options) do
          if tostring(op.localizedName):find(GENITALS[S.male], 1, true) then
            sys:ApplyChangeToOption(o, i - 1)
            return
          end
        end
      end
    end
  end)
  return done
end

-- Where the figure stands to show a part: behind the window's right part for the whole body; for the face or the
-- hands out in front of it, on the same line or right of it, clear of the rows.
local function figureSpot(zoom)
  local spot = Holo.windowSpot(FIGURE_BEHIND) or scene.figure
  local d = ZOOM[zoom]
  if d == nil then return spot end
  local bearing, edge = spot.bearing, Holo.windowRowsEdge()
  if edge ~= nil then bearing = math.max(bearing, edge + math.deg(math.atan(FIGURE_HALF / d)) + ZOOM_CLEAR) end
  return { bearing = bearing, dist = d }
end

-- The figure dressed by the system, whole, in the life path's clothes: behind the window's right part.
local function dressSteps()
  return {
    { name = "figure", act = function()
        S.zoom, S.zoomWant, S.zoomPage = "body", nil, nil
        Figure.spawn(scene.anchor, figureSpot("body"), S.male)
        Figure.goTo(figureSpot("body"))
      end,
      wait = function() return Figure.entity() ~= nil end, min = 0.5 },
    { name = "target", act = function() VRCreationTarget(Figure.entity()) end, min = 0.3 },
    { name = "dress", act = preset, min = 1.0 },
  }
end

-- From the main menu into the game's looks screen, then the figure (unless the looks are for the player).
local function enterSteps(withFigure)
  local steps = {
    { name = "new game", act = function()
        hold(true)
        shared.mainMenu.menuEventDispatcher:SpawnEvent(CName.new("OnNewGame"))
      end,
      wait = function() local s = screen(); return s == "ExpansionNewGame" or s == "DifficultySelectionMenu" end },
    { name = "start", act = function()
        if screen() ~= "ExpansionNewGame" then return end
        state():SetIsExpansionStandalone(S.standalone)
        event("OnAccept")
      end, wait = on("DifficultySelectionMenu") },
    { name = "difficulty", act = function()
        Game.GetStatsDataSystem():SetDifficulty(gameDifficulty[DIFFICULTY[S.difficulty]])
        event("OnAccept")
      end, wait = on("CharacterCreationBackstorySelectionMenu") },
    shutStep(),
    { name = "life path", act = function()
        state():SetLifePath(TweakDBID.new(LIFE[S.life].id))
        event("OnAccept")
      end, wait = on("CharacterCreationGenderSelectionMenu") },
    { name = "body", act = function()
        S.stalled = false
        state():SetIsBodyGenderMale(S.male)
        state():SetIsBrainGenderMale(S.male)
        event("OnAccept")
      end, wait = function() return screen() == "characterCreationBodyMorphMenu" and #system():GetUnitedOptions(true, true, true) > 0 end,
      min = 0.5 },
    openStep(),
  }
  if withFigure ~= false then
    for _, st in ipairs(dressSteps()) do steps[#steps + 1] = st end
  end
  return steps
end

-- Back through the game's screens to the main menu, the system's own puppet its target again (the figure left
-- standing when it wears an engram's look). The way back holds the game thread twice, the eyes shut over both: the
-- body screen's preview going (~0.2 s) and the life path screen coming up again (~0.4 s); they open on the screens
-- past it.
local PAST_LIFE_PATH = { DifficultySelectionMenu = true, ExpansionNewGame = true }
-- `ready()`: once the system has let go and the eyes are shut, before the way back.
local function leaveSteps(keepFigure, ready)
  local sent = 0
  return {
    { name = "untarget", act = function()
        pcall(VRCreationTarget)
        if not keepFigure then Figure.clear() end
      end },
    shutStep(),
    { name = "ready", act = function() if ready ~= nil then ready() end end },
    { name = "back", act = function() if screen() ~= nil then event("OnBack") end end,
      wait = function(t)
        local s = screen()
        if s == nil or PAST_LIFE_PATH[s] then S.dimTo = 0 end
        if s == nil then return shared.mainMenu ~= nil end
        -- One screen back every half second until the main menu is up again.
        if t - sent > 0.5 then
          sent = t
          event("OnBack")
        end
        return false
      end, timeout = 15 },
  }
end

---------------------------------------------------------------- the looks

local function loc(s)
  if s == nil then return "" end
  s = tostring(s)
  local ok, t = pcall(function() return Game.GetLocalizedText(s) end)
  if ok and t ~= nil and t ~= "" and t ~= s then return t end
  if s:sub(1, 7) ~= "LocKey#" then
    ok, t = pcall(function() return Game.GetLocalizedTextByKey(CName.new(s)) end)
    if ok and t ~= nil and t ~= "" then return t end
  end
  -- A colour the game shows as a swatch has a key and no text: its name from the key ("11_red_apple" -> "red apple").
  local key = s:match("^UI%-CharacterCreation%-(.+)$")
  if key ~= nil then return (key:gsub("^%d+_", ""):gsub("_", " ")) end
  return s
end

-- An option's values: how many, and the name of one.
local function values(o)
  local info = o.info
  local kind = info:GetClassName().value
  if kind == "gameuiAppearanceInfo" then
    return #info.definitions, function(i) return loc(info.definitions[i + 1].localizedName) end
  elseif kind == "gameuiMorphInfo" then
    return #info.morphNames, function(i) return loc(info.morphNames[i + 1].localizedName) end
  elseif kind == "gameuiSwitcherInfo" then
    return #info.options, function(i) return loc(info.options[i + 1].localizedName) end
  end
  return 0, function() return "" end
end

-- The system takes a change and shows it in the option's currIndex only some frames later, while the row is drawn
-- again on the press: until it does (or CHOSEN_FOR s have gone), the row shows the value asked and steps from it.
local CHOSEN_FOR = 2.0                   -- s

local function optionKey(o) return tostring(o.info.name.value) end

local function change(o, index)
  if S.job ~= nil then return end
  local ok, err = pcall(function() system():ApplyChangeToOption(o, index) end)
  if not ok then
    S.error = tostring(err)
    return
  end
  S.chosen[optionKey(o)] = { index = index, at = S.time }
end

local function currentIndex(o)
  local key = optionKey(o)
  local c = S.chosen[key]
  if c ~= nil then
    if o.currIndex ~= c.index and S.time - c.at < CHOSEN_FOR then return c.index end
    S.chosen[key] = nil
  end
  return o.currIndex
end

-- The part of the body an option is about (GetSlotName's camera: the head's slots, the nails', the preview's).
local function zoomOf(o)
  local part = nil
  pcall(function() part = tonumber(EnumInt(o.bodyPart)) end)
  if part == tonumber(EnumInt(gameuiCharacterCustomizationPart.Head)) then return "face" end
  if part == tonumber(EnumInt(gameuiCharacterCustomizationPart.Arms)) then return "hands" end
  return "body"
end

local function looksRows()
  local rows = {}
  local t = T()
  rows[#rows + 1] = { key = "preset", label = t.presets, value = loc(LIFE[S.life].key), zoom = "body",
                      left = preset, right = preset }
  rows[#rows + 1] = { key = "random", label = t.random, go = function()
    S.chosen = {}
    pcall(function() system():RandomizeOptions(system():GetRandomizationParameters()) end)
  end }
  local ok, options = pcall(function() return system():GetUnitedOptions(true, true, true) end)
  if not ok or options == nil then return rows end
  for i, o in ipairs(options) do
    local shown = false
    pcall(function() shown = o.isEditable and o.isActive and not o.isCensored end)
    if shown then
      local n, name = values(o)
      if n > 1 then
        local cur = currentIndex(o)
        rows[#rows + 1] = { key = "o" .. i .. tostring(o.info.name.value), label = loc(o.info.localizedName),
                            value = name(cur) .. "  " .. (cur + 1) .. "/" .. n, zoom = zoomOf(o),
                            left = function() change(o, (cur - 1) % n) end,
                            right = function() change(o, (cur + 1) % n) end }
      end
    end
  end
  return rows
end

---------------------------------------------------------------- a save's look

local function str(x)
  if x == nil then return nil end
  if type(x) == "string" then return x end
  local ok, v = pcall(function() return x.value end)
  if ok and v ~= nil then return v end
  return tostring(x)
end

-- A save's look (VRLobbySaveLooks: the options it had and their appearances, the morphs of its regions) on the
-- options the system has now: an appearance by its definition's name, a morph by its region and name, a switcher
-- by the option whose names the save has most of. `only`: the switchers first, which turn other options on. The
-- screen's own options are not all in a save (the skin is: skin_type_01 = h0_000_pma__basehead__02_ca_limestone,
-- the screen's skin_color has that name among its definitions): an option the save has no line of takes the
-- definition the save has an appearance of.
-- What a save's look leaves as the lobby has it: the underwear stays on.
local function kept(name) return name:find("genital") or name:find("underpants") or name:find("pubic") end

local function applyLook(look, only)
  local apps, morphs, names, values = {}, {}, {}, {}
  for _, a in ipairs(look.apps) do
    if not kept(a.option) then
      apps[a.option], names[a.option] = a.appearance, true
      values[a.appearance] = true
    end
  end
  for _, m in ipairs(look.morphs) do morphs[m.region], names[m.region] = m.morph, true end
  local applied = 0
  for _, o in ipairs(system():GetUnitedOptions(true, true, true)) do
    local info = o.info
    local kind = info:GetClassName().value
    if (only == nil or kind == only) and not kept(str(info.name) or "") then
      local index = nil
      if kind == "gameuiAppearanceInfo" then
        local want = apps[str(info.name)]
        for i, d in ipairs(info.definitions) do
          local n = str(d.name)
          if want ~= nil then
            if n == want then index = i - 1 end
          elseif values[n] and index == nil then
            index = i - 1
          end
        end
      elseif kind == "gameuiMorphInfo" then
        local want = morphs[str(info.name)]
        for i, m in ipairs(want and info.morphNames or {}) do
          if str(m.morphName) == want then index = i - 1 end
        end
      elseif kind == "gameuiSwitcherInfo" then
        local best = 0
        for i, sw in ipairs(info.options) do
          local n = 0
          for _, name in ipairs(sw.names) do if names[str(name)] then n = n + 1 end end
          if n > best then best, index = n, i - 1 end
        end
      end
      if index ~= nil and index ~= o.currIndex then
        system():ApplyChangeToOption(o, index)
        applied = applied + 1
      end
    end
  end
  S.lookApplied = (S.lookApplied or 0) + applied
end

-- The switchers first (they turn other options on), the rest, and the rest again for what the switchers turned on
-- late. Nothing of it is seen as it goes on (modules/construct.lua M.stage): the waits are what the system needs.
local function lookSteps(look)
  return {
    { name = "look: switchers", act = function() S.lookApplied = 0; applyLook(look, "gameuiSwitcherInfo") end,
      min = 0.5 },
    { name = "look", act = function() applyLook(look) end, min = 0.9 },
    { name = "look: again", act = function() applyLook(look) end, min = 0.5 },
  }
end

local function lifeIndex(id)
  for i, l in ipairs(LIFE) do
    if l.id == "LifePaths." .. tostring(id) then return i end
  end
  return 1
end

---------------------------------------------------------------- the attributes

local function limits()
  local lo, hi = 3, 6
  pcall(function()
    lo = TweakDB:GetFlat("UICharacterCreationGeneral.BaseValues.minAttributeValue") or lo
    hi = TweakDB:GetFlat("UICharacterCreationGeneral.BaseValues.maxAttributeValue") or hi
  end)
  return lo, hi
end

local function statsRows()
  local t = T()
  local st = state()
  local lo, hi = limits()
  local points = st:GetAttributePointsAvailable()
  local rows = { { key = "points", label = S.standalone and t.fixed or (t.points .. ": " .. points), header = true } }
  for i, name in ipairs(ATTRS) do
    local stat = gamedataStatType[name]
    local v = st:GetAttribute(stat)
    local row = { key = name, label = t.attrs[i], value = tostring(v) }
    if not S.standalone then
      row.left = function()
        if v > lo then st:SetAttribute(stat, v - 1); st:SetAttributePointsAvailable(points + 1) end
      end
      row.right = function()
        if v < hi and points > 0 then st:SetAttribute(stat, v + 1); st:SetAttributePointsAvailable(points - 1) end
      end
    end
    rows[#rows + 1] = row
  end
  if not S.standalone then
    rows[#rows + 1] = { key = "reset", label = t.reset, go = function()
      for _, name in ipairs(ATTRS) do st:SetAttribute(gamedataStatType[name], lo) end
      st:SetAttributePointsAvailable(7)
    end }
  end
  return rows
end

---------------------------------------------------------------- the pages

local function cycle(i, n, d) return (i - 1 + d) % n + 1 end

local function busyRow(t) return { key = "busy", label = t.busy, detail = S.error, enabled = false } end

-- Starts the game: what the game's summary screen does when it closes.
-- THE FIGURE GOES BEFORE THE NEW GAME IS MADE: the system its own puppet again, the figure out of the main menu's
-- world, and the Lua references to it let go while that world is still there. Made with the figure standing, the new
-- game crashed 8 s after START in the new player's appearance build (EntityBuilder_LoadCustomAppearance, a handle to
-- an appearance resource already freed); the game's own screens, with no figure, started it every time (reported by
-- players, reproduced 2026-11-04).
local function startGame()
  if S.job ~= nil then return end
  run({
    { name = "figure out", act = function()
        pcall(VRCreationTarget)
        Figure.clear()
        S.windowOpen, S.dressed = false, false
        collectgarbage("collect")
      end, min = 0.6 },
    { name = "finalize", act = function()
        system():FinalizeState()
        Game.GetSystemRequestsHandler():StartNewGame(state())
      end, min = 0.5 },
  }, function()
    hold(false)
    S.open = false
    if Holo ~= nil then Holo.close() end
  end)
end

-- Another body: back to the body screen, the other body chosen there, the figure of that body.
local function rebody(male)
  if S.job ~= nil or male == S.male then return end
  S.male = male
  local steps = {
    { name = "untarget", act = function() pcall(VRCreationTarget) end },
    shutStep(),
    { name = "back to body", act = function() event("OnBack") end, wait = on("CharacterCreationGenderSelectionMenu") },
    { name = "body", act = function()
        S.stalled = false
        state():SetIsBodyGenderMale(male)
        state():SetIsBrainGenderMale(male)
        event("OnAccept")
      end, wait = function() return screen() == "characterCreationBodyMorphMenu" and #system():GetUnitedOptions(true, true, true) > 0 end,
      min = 0.5 },
    openStep(),
  }
  for _, st in ipairs(dressSteps()) do steps[#steps + 1] = st end
  run(steps)
end

local function relife(d)
  S.life = cycle(S.life, #LIFE, d)
  pcall(function() state():SetLifePath(TweakDBID.new(LIFE[S.life].id)) end)
  presetAttributes()
  preset()
end

local function lifeName(i)
  local fallback = LIFE[i][lang()] or LIFE[i].en
  return Mainmenu ~= nil and Mainmenu.label(LIFE[i].key, fallback) or fallback
end

-- Out of the window, back to the life paths: the figure goes, the system's own puppet its target again.
local function closeWindow()
  S.windowOpen, S.dressed, S.bareAt = false, false, nil
  if S.job ~= nil and S.job.kind == "dress" then S.job = nil end
  pcall(VRCreationTarget)
  Figure.clear()
end

local function newPage()
  local t = T()
  local page = { title = t.title, subtitle = t.sub, code = t.code }
  page.rows = function()
    if S.job ~= nil or not S.ready then return { busyRow(t) } end
    local rows = {}
    if S.ep1 then
      rows[#rows + 1] = { key = "mode", label = t.mode, value = S.standalone and t.ep1 or t.base,
                          left = function() S.standalone = not S.standalone; pcall(function() state():SetIsExpansionStandalone(S.standalone) end); presetAttributes() end,
                          right = function() S.standalone = not S.standalone; pcall(function() state():SetIsExpansionStandalone(S.standalone) end); presetAttributes() end }
    end
    rows[#rows + 1] = { key = "difficulty", label = t.difficulty, value = t.diff[S.difficulty],
                        left = function() S.difficulty = cycle(S.difficulty, 4, -1); pcall(function() Game.GetStatsDataSystem():SetDifficulty(gameDifficulty[DIFFICULTY[S.difficulty]]) end) end,
                        right = function() S.difficulty = cycle(S.difficulty, 4, 1); pcall(function() Game.GetStatsDataSystem():SetDifficulty(gameDifficulty[DIFFICULTY[S.difficulty]]) end) end }
    rows[#rows + 1] = { key = "life", label = t.life, value = lifeName(S.life),
                        left = function() relife(-1) end, right = function() relife(1) end }
    rows[#rows + 1] = { key = "body", label = t.body, value = S.male and t.male or t.female,
                        left = function() rebody(not S.male) end, right = function() rebody(not S.male) end }
    local brain = true
    pcall(function() brain = state():IsBrainGenderMale() end)
    rows[#rows + 1] = { key = "voice", label = t.voice, value = brain and t.male or t.female,
                        left = function() state():SetIsBrainGenderMale(not brain); system():TriggerVoiceToneSample() end,
                        right = function() state():SetIsBrainGenderMale(not brain); system():TriggerVoiceToneSample() end }
    rows[#rows + 1] = { key = "looks", label = t.looks, go = function()
      Holo.push({ title = t.looks, subtitle = t.looksSub, code = "CC//LOOKS", rows = function()
        if S.job ~= nil then return { busyRow(t) } end
        return looksRows()
      end })
    end }
    rows[#rows + 1] = { key = "stats", label = t.stats, go = function()
      Holo.push({ title = t.stats, subtitle = t.statsSub, code = "CC//ATTR", rows = statsRows })
    end }
    rows[#rows + 1] = { key = "start", label = t.start, on = true, go = startGame }
    return rows
  end
  page.onBack = closeWindow
  return page
end

-- A life path chosen on its card: the game's state takes it (with its attributes' preset), and the panel grows into
-- the window the new game is made in; the figure is dressed behind it once it has (M.update).
local function choose(i)
  S.life = i
  pcall(function() state():SetLifePath(TweakDBID.new(LIFE[S.life].id)) end)
  pcall(presetAttributes)
  if Holo.window(newPage()) then S.windowOpen = true end
end

-- The new game's first page: the life paths on cards, the one chosen last in cyan.
local function lifePage()
  local t = T()
  local page = { title = t.title, subtitle = t.pickSub, code = "CC//PATH" }
  page.rows = function()
    local cards = {}
    for i, l in ipairs(LIFE) do
      cards[i] = { key = l.preset, picture = { LIFE_ATLAS, l.poster }, label = lifeName(i), on = i == S.life,
                   go = function() choose(i) end }
    end
    return { { key = "paths", cards = cards, poster = LIFE_POSTER } }
  end
  page.onBack = function() M.leave() end
  return page
end

---------------------------------------------------------------- the lobby's calls

-- holo: modules/holo.lua; mainmenu: modules/mainmenu.lua; sh: init.lua's shared state; sc: { anchor, figure } (the
-- lobby's frame and the scene's figure spot).
function M.init(holo, mainmenu, sh, sc)
  Holo, Mainmenu, shared, scene = holo, mainmenu, sh, sc
end

-- NEW GAME: the life paths, and the game's screens walked to the looks behind them.
function M.open()
  if S.open or S.job ~= nil or shared == nil or shared.mainMenu == nil then return end
  S.open, S.ready, S.windowOpen, S.dressed = true, false, false, false
  S.ep1 = false
  pcall(function() S.ep1 = Game.GetSystemRequestsHandler():IsAdditionalContentEnabled(CName.new("EP1")) end)
  if not S.ep1 then S.standalone = false end
  Holo.push(lifePage())
  run(enterSteps(false), function() S.ready = true end)
  -- out of the new game's pages, the window with them: their BACKs leave the game's screens
  S.job.fail = function() Holo.home() end
end

-- Back to the main menu (the page's BACK, or a step that failed).
function M.leave()
  if not S.open then return end
  S.open, S.ready = false, false
  S.job = nil
  run(leaveSteps(), function() hold(false) end)
  S.job.fail = function() hold(false) end
end

-- An engram's V on the player (modules/engrams.lua): the game's screens are walked to the looks out of view with the
-- save's body chosen -- a woman's on the man's the main menu spawned too: the system puts that body's meshes on the
-- player (modules/construct.lua M.dressBody) -- the system is pointed at the player, the save's look goes on, and
-- back to the main menu. The calls in `on`: stage() under the eyes shut on the way in, before the system is the
-- player's; ready() once the look is all on and the eyes are shut for the way back (the V is shown then, at once);
-- done(ok) when it is over.
function M.wear(look, on)
  if S.job ~= nil or S.open or shared == nil or shared.mainMenu == nil then return false end
  local call = function(f, ...) if f ~= nil then f(...) end end
  local finish = function(ok) pcall(call, on.done, ok) end
  S.male, S.life, S.standalone = look.body ~= "Female", lifeIndex(look.life), false
  local steps = {}
  for _, st in ipairs(enterSteps(false)) do
    steps[#steps + 1] = st
    if st.name == "shut" then steps[#steps + 1] = { name = "stage", act = function() call(on.stage) end } end
  end
  steps[#steps + 1] = { name = "the player", act = function() VRCreationTarget(Game.GetPlayer()) end, min = 0.4 }
  for _, st in ipairs(lookSteps(look)) do steps[#steps + 1] = st end
  for _, st in ipairs(leaveSteps(true, on.ready)) do steps[#steps + 1] = st end
  run(steps, function() hold(false); finish(true) end)
  S.job.fail = function()
    pcall(VRCreationTarget)
    run(leaveSteps(true), function() hold(false) end)
    finish(false)
  end
  return true
end

function M.busy() return S.job ~= nil end
-- How far the new game has the eyes shut, 0 open .. 1 shut (modules/lobby.lua sends it with its own).
function M.closure() return S.dim end
function M.isOpen() return S.open end

function M.update(dt)
  S.time = S.time + (dt or 0)
  stepJob(dt or 0)
  -- The eyes: toward where the job wants them, and open whenever no job holds them (one that failed included). The
  -- frame time is capped: the long frame itself must not jump them open.
  if S.job == nil then S.dimTo = 0 end
  if (dt or 0) > 0.2 then S.stalled = true end
  local step = math.min(dt or 0, 0.05)
  if S.dim < S.dimTo then S.dim = math.min(S.dimTo, S.dim + step / SHUT_TIME)
  elseif S.dim > S.dimTo then S.dim = math.max(S.dimTo, S.dim - step / OPEN_TIME) end
  -- The figure, once the window has grown and the game's screens are walked to the looks: dressed behind it.
  if S.open and S.windowOpen and S.ready and not S.dressed and S.job == nil and Holo.windowOpen() then
    run(dressSteps(), function() S.dressed = true end, "dress")
    S.job.fail = function() S.dressed = true end
  end
  if S.open and S.dressed and Figure.standing() then
    -- The figure to the part of the row pointed at a moment, back behind the window on another page.
    local page = Holo.top()
    if page ~= S.zoomPage then S.zoomPage, S.zoomWant, S.zoom = page, nil, "body" end
    local row = S.job == nil and Holo.hovered() or nil
    local want = row ~= nil and row.zoom or nil
    if want ~= S.zoomWant then S.zoomWant, S.zoomSince = want, S.time end
    if want ~= nil and S.time - S.zoomSince >= ZOOM_DWELL then S.zoom = want end
    Figure.goTo(figureSpot(S.zoom))
    -- A preset's underwear off, once it is on.
    if S.bareAt ~= nil and S.time >= S.bareAt and S.job == nil then
      S.bareAt = (not bare() and S.time < S.bareUntil) and S.time + BARE_EVERY or nil
    end
  end
  Figure.update(dt)
end

-- The lobby ends: nothing of the new game is left behind.
function M.close()
  if S.held then pcall(VRCreationTarget) end
  Figure.clear()
  S.job, S.open, S.ready, S.windowOpen, S.dressed = nil, false, false, false, false
  hold(false)
end

function M.state()
  return { open = S.open, ready = S.ready == true, job = S.job and (S.job.steps[S.job.i] or {}).name or nil,
           screen = screen(), error = S.error, held = S.held, male = S.male, life = S.life,
           window = S.windowOpen, dressed = S.dressed, figure = Figure.state() }
end

return M
