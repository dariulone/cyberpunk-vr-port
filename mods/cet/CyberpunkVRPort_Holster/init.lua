-- CyberpunkVRPort_Holster -- the weapons on the body, and the hand that draws them from there.
--
-- Immersive holsters (xr_immersive_holsters = 1, the default; the plugin publishes it inverted to shared slot [23]
-- so the zeroed block reads immersive): every weapon in the three weapon slots that is not in a hand hangs where it
-- belongs on the body and is drawn by reaching for it there -- modules/holsters.lua, the places in
-- modules/placements.lua. Simple holsters (0): the earlier hand-to-zone equip, modules/simple.lua.
--
-- The logic lives in modules/ so it can be read again without restarting the game:
-- GetMod('CyberpunkVRPort_Holster').hot().

-- HOT RELOAD, for development (the Reload mod's loader): each file compiled with its own `require` bound in front
-- of it, against a cache that is new for every reload.
local function hotLoad(name)
  local cache = {}
  local function req(n)
    local v = cache[n]
    if v ~= nil then return v end
    local f = io.open(n .. '.lua', 'r')
    if not f then error('hot reload: cannot open ' .. n .. '.lua') end
    local src = f:read('*a')
    f:close()
    local chunk, err = loadstring('local require = ...; ' .. src, '@' .. n .. '.lua')
    if not chunk then error(err) end
    v = chunk(req)
    if v == nil then v = true end
    cache[n] = v
    return v
  end
  return req(name)
end

local Holsters = require('modules/holsters')
local Simple = require('modules/simple')
local errorAt = -10
local clock = 0

registerForEvent('onInit', function()
  pcall(Simple.init)
  -- the copies belong to the world the player is in: let them go with it
  pcall(function() ObserveBefore('PlayerPuppet', 'OnDetach', function() pcall(Holsters.release) end) end)
end)

registerForEvent('onUpdate', function(dt)
  dt = dt or 0.016
  clock = clock + dt
  if type(GetVRSharedSlot) ~= 'function' then return end
  local simple = GetVRSharedSlot(23) > 0.5
  local ok, err = pcall(Holsters.update, dt, not simple)
  if not ok and clock - errorAt > 5 then
    errorAt = clock
    print('[Holster] ' .. tostring(err))
  end
  if simple then pcall(Simple.update, dt) end
end)

registerForEvent('onShutdown', function() pcall(Holsters.release) end)

return {
  state = function() return Holsters.state() end,
  -- the VR menu's inventory: a weapon slot's holster (H.slotInfo), its choice, and hiding them all
  slotInfo = function(slot) return Holsters.slotInfo(slot) end,
  setPlace = function(slot, name) return Holsters.setPlace(slot, name) end,
  hidden = function() return Holsters.hidden() end,
  setHidden = function(on) return Holsters.setHidden(on) end,
  holsters = function() return Holsters end,
  -- GetMod('CyberpunkVRPort_Holster').hot() -> 'ok' or the error; the old module stays if the new one fails
  hot = function()
    local ok, new = pcall(hotLoad, 'modules/holsters')
    if not ok then return 'hot reload failed: ' .. tostring(new) end
    pcall(Holsters.release)
    Holsters = new
    return 'ok'
  end,
}
