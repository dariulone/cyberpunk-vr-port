-- The VR lobby: the main menu's own world as a place to stand and walk in.
--
-- The plugin keeps that world rendering, gives it to both eyes, head-tracked, switches on the world layer every
-- in-world screen is drawn through, and hands the game's own input to the player standing there -- the pad walks and
-- turns the puppet through the game's locomotion (src/Lobby/MenuLobby.cpp). The main menu's own UI input context is
-- set aside with it, so the game's menu no longer answers the pad.
--
-- This file takes the game's main menu out of view: the menu layer's window -- every screen of the main menu and the
-- mouse cursor with them -- is hidden while the lobby is up and shown again when it ends. Its controllers keep
-- running; nothing of the menu is moved or rebuilt.
--
-- And it builds the place (tools/vr_lobby_assets: lobby_layout.py -> Blender -> make_lobby_scene.py). Its list's
-- place, when it has one, is the game's VR tutorial hall (tutorial_lobby.py: its sectors streamed into this world,
-- twice the size): the player is put on its floor, and the terminal, the table and the pod are spawned round that
-- spot, turned to its heading -- the scene's +Y is the way the player looks -- with a fence along the floor's edge and
-- the hall's white haze (a fog volume in the near entity). Without one it is the procedural city, a plaza of polished
-- concrete at the bottom of an endless city of bare concrete blocks, spawned where the player stands. The main menu's
-- world keeps its own clock and weather; both are held at the list's (noon under heavy clouds for the hall's white
-- haze; the blue hour for the city). Its exposure is held too: the game's auto exposure shows the place darker than
-- the reference, so it is exposed brighter by the list's stops (VRPortLobbyExposure,
-- mods/redscript/CyberpunkVRPort_EngineStereo), and put back when the lobby ends.
--
-- The player standing there is the construct (modules/construct.lua): the body a gold hologram, its clothes off.
-- In the tutorial hall the game's main menu hangs in the air before the player as holographic panels
-- (modules/holo.lua, its pages modules/mainmenu.lua); the new game is made there too, on the figure standing at the
-- player's right (modules/creation.lua, modules/figure.lua) -- the lobby does not step aside while it is, though the
-- game's menu is then away from its root (shared.creating). The engrams on the table (modules/engrams.lua) are the
-- player's playthroughs, put on in the neck. Elsewhere the game's main menu is the terminal's (modules/terminal.lua).
-- The rows that open one of the game's own screens
-- (new game, load game, settings) leave the menu's root; while the game's menu is away from its root the lobby steps
-- aside (VRLobbySuspend: the menu, its input and its picture are the game's again), keeping the place where it is,
-- and comes back when the game returns to the root (init.lua watches the menu's scenarios). The things on the table
-- are physical bodies to pick up and throw (modules/items.lua); they are put there once the place stands, the
-- table's collider with it. The terminal's СВЕТ dial dims and brightens the place's lamps
-- (modules/lights.lua). Both are taken hold of by the hands of modules/hands.lua, read once a frame. Everything is
-- spawned under the lobby's tag (modules/spawner.lua).
--
-- Until the place stands and the player is on it the headset shows nothing: the plugin shuts the eyes from the moment
-- the main menu comes up -- the menu's window, then the hall from wherever the player first stood, would show
-- meanwhile -- and this file opens them (VRLobbyCurtain), or opens them on an error rather than hide it. The first
-- time in a launch the intro opens them instead (modules/intro.lua: the game's legal notice and logo in the dark),
-- and the menu waits for it.
local M = {}
local Spawner = require('modules/spawner')
local Hands = require('modules/hands')
local Lights = require('modules/lights')
local Editor = require('modules/worldbuilder')
local Construct = require('modules/construct')
local Terminal = require('modules/terminal')
local Items = require('modules/items')
local Holo = require('modules/holo')
local Mainmenu = require('modules/mainmenu')
local Creation = require('modules/creation')
local Engrams = require('modules/engrams')
local Intro = require('modules/intro')
local Setup = require('modules/setup')
local Sound = require('modules/sound')

-- The place's entities and screens, all in its frame (make_lobby_scene.py writes the list).
local SCENE = require("modules/scene_list")

local ATMOSPHERE = SCENE.atmosphere or { hour = 21, minute = 30, weather = "24h_weather_light_clouds" }
local S = { window = nil, hidden = false, error = nil, scene = nil, anchor = nil, clock = 0, away = false,
            items = false, hour = ATMOSPHERE.hour, minute = ATMOSPHERE.minute, weather = ATMOSPHERE.weather,
            exposure = 0, placeClock = 0 }
local SCENE_TAG = "VRLobbyScene"
S.editor = Editor.command
-- For tests (init.lua's debug()): the lobby's parts.
S.modules = { holo = Holo, creation = Creation, engrams = Engrams, items = Items, mainmenu = Mainmenu,
              construct = Construct, hands = Hands, intro = Intro, setup = Setup, sound = Sound }

-- Where the place stands: the list's place (the tutorial hall: the spawn on its floor and its heading), or where
-- the player stands when the lobby comes up.
local function anchorHere()
  local place = SCENE.place
  if place ~= nil then
    local p = place.position
    return { pos = Vector4.new(p[1], p[2], p[3], 1.0), yaw = place.yaw }
  end
  local player = Game.GetPlayer()
  if player == nil then return nil end
  local pos, f = player:GetWorldPosition(), player:GetWorldForward()
  return { pos = Vector4.new(pos.x, pos.y, pos.z, 1.0), yaw = math.deg(math.atan2(-f.x, f.y)) }
end

-- The tutorial hall is far from where the main menu's world starts the player: they are put on its floor as soon
-- as the lobby is up (and again whenever the world has them elsewhere, a few seconds apart) -- early, in the first
-- frames of the menu: put there once the menu has settled, that far away, the game shows a loading screen.
local PLACE_REACH = 60.0
-- True while the player stands on the place (always, for a place made where the player stands).
local function putOnPlace(dt)
  local place = SCENE.place
  if place == nil then return true end
  local there = false
  pcall(function()
    local p, at = Game.GetPlayer():GetWorldPosition(), place.position
    local dx, dy, dz = p.x - at[1], p.y - at[2], p.z - at[3]
    there = dx * dx + dy * dy + dz * dz < PLACE_REACH * PLACE_REACH
  end)
  S.placeClock = S.placeClock - (dt or 0)
  if there or S.placeClock > 0 then return there end
  pcall(function()
    local at = place.position
    Game.GetTeleportationFacility():Teleport(Game.GetPlayer(), Vector4.new(at[1], at[2], at[3] + 0.1, 1.0),
                                             EulerAngles.new(0, 0, place.yaw))
    S.placeClock = 3.0
  end)
  return false
end

-- How far the eyes are shut, 0 open .. 1 shut; sent every frame, the plugin shuts them again each time the main
-- menu comes up.
local function curtain(closure)
  if type(VRLobbyCurtain) == "function" then pcall(VRLobbyCurtain, closure) end
end

-- The game's main menu: the holographic panels where the scene has their frame, the terminal elsewhere. Until the
-- first-launch setup has been through (modules/setup.lua), its pages come first and end on the main menu.
local function openMenu()
  if SCENE.holo ~= nil then
    local T = Mainmenu.texts()
    local texts = { back = T.back, more = "" }
    local main = function() Holo.open(S.anchor, SCENE.holo, Mainmenu.root(Holo, Creation, Engrams), texts) end
    if Setup.needed() then Holo.open(S.anchor, SCENE.holo, Setup.root(Holo, main), texts) else main() end
  else
    Terminal.open(S.anchor, SCENE.screens.terminal)
  end
end

local function menuOpen()
  if SCENE.holo ~= nil then return Holo.isOpen() end
  return Terminal.state().open ~= false
end

-- The place stays where it first came up: a respawn (a reloaded archive) puts it back on the same spot.
local function spawnScene()
  local ok, err = pcall(function()
    if S.anchor == nil then
      S.anchor = anchorHere()
      if S.anchor == nil then return end
    end
    local tr = WorldTransform.new()
    tr:SetPosition(S.anchor.pos)
    tr:SetOrientation(EulerAngles.new(0, 0, S.anchor.yaw):ToQuat())
    S.scene = {}
    for _, path in ipairs(SCENE.entities) do table.insert(S.scene, Spawner.spawn(path, tr, SCENE_TAG)) end
    if #S.scene ~= #SCENE.entities then error("spawning the place: " .. tostring(Spawner.lastError)) end
    S.items = false
    Editor.open(S.anchor)
  end)
  if not ok then S.error = tostring(err) end
end

-- The place is still in the world (spawned, or still on its way): the main menu's world is made again each time the
-- game comes back to it, and the place with it has to be.
local function sceneStands()
  return S.scene ~= nil and Spawner.alive(S.scene[1])
end

-- Every part of the place spawned: the table's collider is there to put things on.
local function sceneSpawned()
  if S.scene == nil or #S.scene ~= #SCENE.entities then return false end
  for _, id in ipairs(S.scene) do
    if not Spawner.spawned(id) then return false end
  end
  return true
end

local function despawnScene()
  Editor.close()
  Terminal.close()
  Creation.close()
  Holo.close()
  Engrams.close()
  Items.close()
  Lights.close()
  Spawner.clear(SCENE_TAG)
  S.scene, S.items = nil, false
end

-- The exposure offset (stops) on the environment's exposure settings; the plugin's own value is the one checked: a
-- game session that starts puts it back to 0.
local function setExposure(stops)
  pcall(function()
    if math.abs(Game.VRLobbyExposureGet() - stops) > 0.001 then Game.VRLobbyExposureSet(stops) end
    S.exposure = stops
  end)
end

-- The world's clock is stopped at the held time while the lobby is up (the game clock alone: the simulation, the
-- walking and the moving blocks run on), so that neither the light nor the weather moves on with it; the weather
-- is put back should anything change it. A game that starts gets its clock running again.
local CLOCK_SOURCE = "VRLobby"

local function stopClock(stopped)
  if S.clockStopped == stopped then return end
  pcall(function()
    Game.GetTimeSystem():SetPausedState(stopped, CName.new(CLOCK_SOURCE))
    S.clockStopped = stopped
  end)
end

local function holdAtmosphere(dt)
  S.clock = S.clock - dt
  if S.clock > 0 then return end
  S.clock = 1.0
  pcall(function()
    local ts = Game.GetTimeSystem()
    local t = ts:GetGameTime()
    local now, held = t:Hours() * 60 + t:Minutes(), S.hour * 60 + S.minute
    if now < held or now > held + 15 then ts:SetGameTimeByHMS(S.hour, S.minute, 0) end
    if Game.VRWeatherState().value ~= S.weather then Game.VRWeatherSet(CName.new(S.weather), 0.0, 0) end
  end)
  stopClock(true)
  setExposure(SCENE.exposure or 0)
end

-- The menu layer's window: every screen of the main menu is spawned into it.
local function menuWindow()
  local out = nil
  pcall(function()
    local layer = Game.VRInkLayer(CName.new("inkMenuLayer"))
    if layer then out = layer:GetVirtualWindow() end
  end)
  return out
end

local function show()
  if S.hidden and S.window ~= nil then pcall(function() S.window:SetVisible(true) end) end
  S.window, S.hidden = nil, false
end

function M.close()
  show()
  despawnScene()
  Construct.restore()
  setExposure(0)
  stopClock(false)
  S.anchor = nil
end

-- shared: the mod's state across reloads (init.lua): the game's main menu controller, whether its menu is away
-- from the root.
function M.init(shared)
  M.shared = shared
  -- Nothing of the lobby is in the world yet as far as this instance knows: whatever carries its tag was left there.
  Spawner.clear()
  Terminal.init(shared)
  Mainmenu.init(shared)
  -- The new game's figure stands in the lobby's frame, wherever the place came up.
  Creation.init(Holo, Mainmenu, shared, setmetatable({}, { __index = function(_, k)
    if k == "anchor" then return S.anchor end
    if k == "figure" then return SCENE.figure end
  end }))
  Engrams.init(Creation)
end

function M.update(dt)
  -- The game's menu away from its root: the lobby steps aside until it is back (not for the new game made here).
  local away = M.shared ~= nil and M.shared.menuAway == true and M.shared.creating ~= true
  if away ~= S.away and type(VRLobbySuspend) == "function" then
    S.away = away
    pcall(VRLobbySuspend, away)
    if away then
      Terminal.close()
      Holo.close()
    end
  end
  local active = false
  pcall(function() active = type(VRLobbyActive) == "function" and VRLobbyActive() end)
  pcall(Sound.update, dt)
  if not active then
    Intro.stop()
    -- The ambience goes with the main menu; it stays while the lobby only steps aside for the game's own screens.
    if not S.away then Sound.ambient(false) end
    if S.hidden then show() end
    if not S.away then
      if S.scene ~= nil then despawnScene() end
      Construct.restore()
      if S.exposure ~= 0 then setExposure(0) end
      stopClock(false)
      S.anchor = nil
    end
    return
  end
  local placed = S.away or putOnPlace(dt)
  holdAtmosphere(dt or 0)
  local ok, err = pcall(Construct.update, dt)
  if not ok then S.error = tostring(err) end
  if S.scene ~= nil and not sceneStands() then
    despawnScene()
    S.anchor = nil
  end
  if S.scene == nil then spawnScene() end
  if not S.items and sceneSpawned() then
    Items.open(S.anchor, Engrams.items(SCENE.items))
    Lights.open(S.anchor, SCENE.dials and SCENE.dials.light)
    S.items = true
  end
  local ready = placed and S.items
  if ready and S.error == nil then Intro.begin() end
  ok, err = pcall(Intro.update, dt)
  if not ok then
    S.error = tostring(err)
    Intro.stop()
  end
  -- ...and the new game's, shut over the game's body screen coming up (modules/creation.lua).
  local shut = (ready or S.error ~= nil) and 0.0 or 1.0
  if Intro.running() then shut = Intro.closure() end
  curtain(math.max(shut, Creation.closure()))
  -- The lobby's ambience once the place is open, after the intro (modules/sound.lua).
  Sound.ambient(not Intro.running() and (ready or S.error ~= nil))
  -- The menu once the place stands (or has failed to), never over the intro.
  if S.anchor ~= nil and (ready or S.error ~= nil) and not S.away and not menuOpen() and not Intro.running() then
    openMenu()
  end
  ok, err = pcall(SCENE.holo ~= nil and Holo.update or Terminal.update, dt)
  if not ok then S.error = tostring(err) end
  ok, err = pcall(Setup.update, dt)
  if not ok then S.error = tostring(err) end
  ok, err = pcall(Creation.update, dt)
  if not ok then S.error = tostring(err) end
  -- The hands first, then what they take hold of: the dial before the things on the table.
  ok, err = pcall(Hands.update, dt or 0)
  if not ok then S.error = tostring(err) end
  ok, err = pcall(Lights.update, dt)
  if not ok then S.error = tostring(err) end
  ok, err = pcall(Items.update, dt, SCENE.holo ~= nil and Holo.pointing() or nil)
  if not ok then S.error = tostring(err) end
  ok, err = pcall(Engrams.update, dt)
  if not ok then S.error = tostring(err) end
  if S.window == nil then S.window = menuWindow() end
  if S.window == nil then return end
  -- Held every frame: the menu shows its window again when it opens a screen of its own.
  local ok, err = pcall(function()
    if S.window:IsVisible() then S.window:SetVisible(false) end
  end)
  if ok then S.hidden = true else S.error = tostring(err) end
end

-- Spawns the scene again on the next update: picks up a reloaded archive.
function M.respawn() despawnScene() end

function M.debug() return S end
function M.terminal() return Terminal end
function M.holo() return Holo end
function M.creation() return Creation end
function M.engrams() return Engrams end
function M.items() return Items end
function M.lights() return Lights end
function M.construct(on) Construct.enable(on) end
function M.editor(req) return Editor.command(req) end

function M.state()
  return { hidden = S.hidden, window = S.window ~= nil, scene = S.scene ~= nil, error = S.error, away = S.away,
           spawned = Spawner.count(), construct = Construct.state(), terminal = Terminal.state(),
           items = Items.state(), lights = Lights.state(), holo = Holo.state(), creation = Creation.state(),
           engrams = Engrams.state(), intro = Intro.state() }
end

return M
