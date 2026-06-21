-- CyberpunkVRPort — pin projection probe v5. The map controller exposes:
--   mappinsPositions : array<Vector3>   (world positions of pins)
--   ProjectWorldToScreen(Vector4) -> Vector2   (the projection used for pins)
-- Project each pin's world pos and log world->screen pairs, so we can see the
-- exact X/Y scaling and compute the correction. Read-only. -> rttprobe.log

local LOG = 'rttprobe.log'
local function L(t) local f=io.open(LOG,'a'); if f then f:write(t..'\n'); f:close() end end

local g_ctrl = nil
local g_done = false
local g_accum = 0.0

local function probe()
  L("[PROJ] ---- probe ----")
  -- read mappinsPositions
  local positions = nil
  pcall(function() positions = g_ctrl.mappinsPositions end)
  if positions == nil then L("[PROJ] mappinsPositions nil"); else
    local n = 0; pcall(function() n = #positions end)
    L("[PROJ] mappinsPositions count="..tostring(n))
    local lim = n; if lim > 8 then lim = 8 end
    for i=1,lim do
      local wp = positions[i]
      local wx,wy,wz = 0,0,0
      pcall(function() wx = wp.x; wy = wp.y; wz = wp.z end)
      local sx, sy = nil, nil
      pcall(function()
        local s = g_ctrl:ProjectWorldToScreen(Vector4.new(wx, wy, wz, 1.0))
        sx = s.X; sy = s.Y
      end)
      L(string.format("[PROJ] pin[%d] world=(%.1f,%.1f,%.1f) -> screen=(%s,%s)",
        i, wx, wy, wz, tostring(sx), tostring(sy)))
    end
  end
  -- also project a few reference world points to see axis scaling
  local refs = { {0,0,0}, {100,0,0}, {0,100,0}, {1000,0,0}, {0,1000,0} }
  for _, r in ipairs(refs) do
    local sx, sy = nil, nil
    pcall(function()
      local s = g_ctrl:ProjectWorldToScreen(Vector4.new(r[1], r[2], r[3], 1.0))
      sx = s.X; sy = s.Y
    end)
    L(string.format("[PROJ] ref world=(%.0f,%.0f,%.0f) -> screen=(%s,%s)", r[1], r[2], r[3], tostring(sx), tostring(sy)))
  end
  return true
end

registerForEvent('onInit', function()
  L("[PROJ] v5 probe init")
  ObserveAfter('WorldMapMenuGameController', 'OnInitialize', function(this)
    g_ctrl = this; g_done = false; g_accum = 0.0
  end)
  ObserveAfter('WorldMapMenuGameController', 'OnUninitialize', function()
    g_ctrl = nil
  end)
end)

registerForEvent('onUpdate', function(dt)
  if g_ctrl == nil or g_done then return end
  g_accum = g_accum + (dt or 0.016)
  if g_accum < 1.0 then return end
  g_accum = 0.0
  if pcall(probe) then g_done = true end
end)
