-- CyberpunkVRPort — world map head-lock bridge (CET).
--
-- While the world map is open AND a menu is confirmed, tell dxgi (plugin native
-- SetVRMenuOpen -> dedicated shared slot managed by the native bridge) to (a) stop driving the game camera with the
-- HMD so the map doesn't swim, and (b) [TEST] suspend the SettingsRes resolution
-- patch so the game's native UI projection isn't distorted. Gameplay-safe: gated
-- on UI_System.IsInMenu so the flag clears every frame in gameplay.

local g_ctrl = nil

local function setMenuOpen(open) pcall(function() Game.SetVRMenuOpen(open and 1 or 0) end) end

local function gameSaysInMenu()
  local inMenu = false
  pcall(function()
    local d = Game.GetAllBlackboardDefs()
    local bb = Game.GetBlackboardSystem():Get(d.UI_System)
    inMenu = bb:GetBool(d.UI_System.IsInMenu)
  end)
  return inMenu
end

registerForEvent('onInit', function()
  ObserveAfter('WorldMapMenuGameController', 'OnInitialize', function(this) g_ctrl = this end)
  ObserveAfter('WorldMapMenuGameController', 'OnUninitialize', function() g_ctrl = nil end)
end)

registerForEvent('onUpdate', function()
  setMenuOpen(g_ctrl ~= nil and gameSaysInMenu())
end)

registerForEvent('onShutdown', function() setMenuOpen(false) end)
