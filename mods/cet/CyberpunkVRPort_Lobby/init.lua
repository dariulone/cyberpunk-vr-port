-- CyberpunkVRPort_Lobby -- the main menu as a place: the main menu's own world in both eyes and head-tracked, the
-- player walking in it on the game's own input (src/Lobby/MenuLobby.cpp), the game's main menu in the air there.
-- modules/lobby.lua is the lobby; this file only wires CET's events and the game's menu it watches. The game's
-- loading screens are the plugin's to show: hung in the dark before the player (src/Render/LoadingScreen.cpp).
--
-- For development, GetMod("CyberpunkVRPort_Lobby").hot() reads modules/ again off disk without a restart (the
-- Reload mod's loader: CET hides `package`, so each file is compiled with its own `require` in front of it).

local Lobby = require("modules/lobby")
-- Kept across hot reloads: the game's main menu controller (the menu's rows go through it) and its list of saves
-- (newest first: the index a save is loaded by), whether that menu is away from its root on one of the game's own
-- screens (the lobby steps aside then), the new game's screen up and its dispatcher, and whether the new game is
-- being made in the lobby (modules/creation.lua: the lobby stays up).
local shared = { mainMenu = nil, menuAway = false, saveNames = nil, creation = nil, creationEvents = nil,
                 creating = false }

local function hotLoad()
  local cache = {}
  local function req(name)
    local v = cache[name]
    if v ~= nil then return v end
    local f = io.open(name .. ".lua", "r")
    if not f then error("hot reload: cannot open " .. name .. ".lua") end
    local src = f:read("*a")
    f:close()
    local chunk, err = loadstring("local require = ...; " .. src, "@" .. name .. ".lua")
    if not chunk then error(err) end
    v = chunk(req)
    if v == nil then v = true end
    cache[name] = v
    return v
  end
  return req("modules/lobby")
end

registerForEvent("onInit", function()
  pcall(function()
    ObserveAfter("SingleplayerMenuGameController", "OnInitialize", function(self) shared.mainMenu = self end)
    ObserveBefore("SingleplayerMenuGameController", "OnUninitialize", function(self)
      if shared.mainMenu == self then shared.mainMenu = nil end
    end)
    ObserveAfter("SingleplayerMenuGameController", "OnSavesForLoadReady", function(self, saves)
      shared.saveNames = saves
    end)
    -- The main menu's root scenario: entered = the game's menu is at its root; left = one of its own screens.
    ObserveAfter("MenuScenario_SingleplayerMenu", "OnEnterScenario", function(self) shared.menuAway = false end)
    ObserveAfter("MenuScenario_SingleplayerMenu", "OnLeaveScenario", function(self) shared.menuAway = true end)
    -- The new game's screens (life path, body, looks, stats, summary): the one up and the dispatcher they step on.
    ObserveAfter("BaseCharacterCreationController", "OnSetMenuEventDispatcher", function(self, d)
      shared.creation, shared.creationEvents = self, d
    end)
    ObserveBefore("BaseCharacterCreationController", "OnUninitialize", function(self)
      if shared.creation == self then shared.creation = nil end
    end)
  end)
  Lobby.init(shared)
  print("[VRLobby] ready")
end)

registerForEvent("onUpdate", function(dt)
  local ok, err = pcall(Lobby.update, dt)
  if not ok then print("[VRLobby] " .. tostring(err)) end
end)

registerForEvent("onShutdown", function()
  pcall(Lobby.close)
end)

return {
  state = function() return Lobby.state() end,
  debug = function() return Lobby.debug() end,
  editor = function(req) return Lobby.editor(req) end,
  respawn = function() return Lobby.respawn() end,
  construct = function(on) return Lobby.construct(on) end,
  terminal = function() return Lobby.terminal() end,
  items = function() return Lobby.items() end,
  lights = function() return Lobby.lights() end,
  holo = function() return Lobby.holo() end,
  creation = function() return Lobby.creation() end,
  engrams = function() return Lobby.engrams() end,
  shared = function() return shared end,
  hot = function()
    pcall(Lobby.close)
    Lobby = hotLoad()
    Lobby.init(shared)
    return "reloaded"
  end,
}
