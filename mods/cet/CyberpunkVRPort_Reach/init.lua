-- CyberpunkVRPort_Reach -- what a hand reaches for, marked in the world: a ring on the thing a hand would take or use,
-- and the brackets on the thing its held trigger has locked from afar (modules/marker.lua). In the game this mod
-- chooses that thing for the hand and has the game offer it (modules/world.lua); the lobby marks the things on its
-- table through it (mods/cet/CyberpunkVRPort_Lobby modules/items.lua).
--
-- For development, GetMod("CyberpunkVRPort_Reach").hot() reads modules/ again off disk without a restart.

local Marker = require("modules/marker")
local World = require("modules/world")

local function hotLoad(name)
  local f = io.open(name .. ".lua", "r")
  if not f then error("hot reload: cannot open " .. name .. ".lua") end
  local src = f:read("*a")
  f:close()
  local chunk, err = loadstring(src, "@" .. name .. ".lua")
  if not chunk then error(err) end
  return chunk()
end

registerForEvent("onUpdate", function(dt)
  local ok, err = pcall(World.update, dt, Marker)
  if not ok then print("[VRReach] world: " .. tostring(err)) end
  ok, err = pcall(Marker.update, dt)
  if not ok then print("[VRReach] " .. tostring(err)) end
end)

registerForEvent("onShutdown", function()
  pcall(Marker.clear)
end)

-- Through functions, not the modules themselves: another mod's hold on these survives a hot reload.
return {
  mark = function(hand, t) Marker.mark(hand, t) end,
  state = function() return Marker.state() end,
  world = function() return World.state() end,
  worldDebug = function() return World.debug() end,
  pin = function(hand, t) Marker.pin(hand, t) end,
  hold = function(point, seconds) World.hold(point, seconds) end,
  hot = function()
    pcall(Marker.clear)
    Marker = hotLoad("modules/marker")
    World = hotLoad("modules/world")
    return "reloaded"
  end,
}
