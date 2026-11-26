-- CyberpunkVRPort_Reach -- what a hand reaches for, marked in the world: a ring on the thing a hand would take or use,
-- and the brackets on the thing its held trigger has locked from afar (modules/marker.lua). In the game this mod
-- chooses that thing for the hand and has the game offer it (modules/world.lua); the lobby marks the things on its
-- table through it (mods/cet/CyberpunkVRPort_Lobby modules/items.lua).
--
-- For development, GetMod("CyberpunkVRPort_Reach").hot() reads modules/ again off disk without a restart.

local Marker = require("modules/marker")
local World = require("modules/world")

-- THE RUST BUILD RUNS BOTH (rust/crates/cvr-ffi/src/mechanics/reach.rs, bit 17 of VRRustMechanics): the other mods' marks
-- go to it through VRReachMark. The reference build has no such native, and the modules keep running here.
local rustOwned = nil
local function rust()
  if rustOwned == nil then
    rustOwned = type(VRRustMechanics) == "function" and math.floor(VRRustMechanics() / 131072) % 2 == 1
  end
  return rustOwned
end
local KINDS = { near = 0, aim = 1, lock = 2, pull = 3 }
local function rustMark(native, hand, t)
  local p, f = t and t.pos, t and t.from
  native(hand, p and Vector4.new(p[1], p[2], p[3], 1) or Vector4.new(0, 0, 0, 0), t and t.radius or -1e9, t and t.lift or -1e9,
    t and KINDS[t.kind] or 0, not (t and t.ready == false), f and Vector4.new(f[1], f[2], f[3], 1) or Vector4.new(0, 0, 0, 0))
end

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
  if rust() then return end
  local ok, err = pcall(World.update, dt, Marker)
  if not ok then print("[VRReach] world: " .. tostring(err)) end
  ok, err = pcall(Marker.update, dt)
  if not ok then print("[VRReach] " .. tostring(err)) end
end)

registerForEvent("onShutdown", function()
  if rust() then return end
  pcall(Marker.clear)
end)

-- Through functions, not the modules themselves: another mod's hold on these survives a hot reload.
return {
  mark = function(hand, t)
    if rust() then if t ~= nil and t.pos ~= nil then rustMark(VRReachMark, hand, t) end; return end
    Marker.mark(hand, t)
  end,
  state = function() return Marker.state() end,
  world = function() return World.state() end,
  worldDebug = function() return World.debug() end,
  pin = function(hand, t) if rust() then rustMark(VRReachPin, hand, t) else Marker.pin(hand, t) end end,
  hold = function(point, seconds)
    if rust() then VRReachHold(Vector4.new(point[1], point[2], point[3], 1), seconds or 1) else World.hold(point, seconds) end
  end,
  hot = function()
    pcall(Marker.clear)
    Marker = hotLoad("modules/marker")
    World = hotLoad("modules/world")
    return "reloaded"
  end,
}
