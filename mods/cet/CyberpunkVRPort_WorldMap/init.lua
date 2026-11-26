-- CyberpunkVRPort — world map head-lock bridge (CET).
--
-- While the world map is open AND a menu is confirmed, tell dxgi (plugin native
-- SetVRMenuOpen -> dedicated shared slot managed by the native bridge) to (a) stop driving the game camera with the
-- HMD so the map doesn't swim, and (b) [TEST] suspend the SettingsRes resolution
-- patch so the game's native UI projection isn't distorted. Gameplay-safe: gated
-- on UI_System.IsInMenu so the flag clears every frame in gameplay.

local g_ctrl = nil

-- Every frame: a plain pcall, no closure per call.
local function sendMenuOpen(value) Game.SetVRMenuOpen(value) end
local function setMenuOpen(open) pcall(sendMenuOpen, open and 1 or 0) end

local function gameSaysInMenu()
  local inMenu = false
  pcall(function()
    local d = Game.GetAllBlackboardDefs()
    local bb = Game.GetBlackboardSystem():Get(d.UI_System)
    inMenu = bb:GetBool(d.UI_System.IsInMenu)
  end)
  return inMenu
end

-- THE RUST BUILD RUNS THIS BRIDGE (rust/crates/cvr-ffi/src/mechanics.rs world_map, bit 2 of VRRustMechanics): the
-- blackboard read and the flag, every frame on the game's main thread, and its clearing at shutdown. The hook stays
-- here -- these two observers tell the plugin when the controller lives. The reference build has no such native, and
-- the whole bridge keeps running here.
local rustOwned = nil
local function rustOwnsBridge()
  if rustOwned == nil then
    rustOwned = type(VRRustMechanics) == 'function' and math.floor(VRRustMechanics() / 4) % 2 == 1
  end
  return rustOwned
end

-- THE RUST BUILD HOOKS THESE ITSELF (rust/crates/cvr-ffi/src/mechanics/game_hooks.rs, bit 1 of VRRustMechanics2).
local function rustHooks()
  return type(VRRustMechanics2) == 'function' and math.floor(VRRustMechanics2() / 2) % 2 == 1
end
registerForEvent('onInit', function()
  if rustOwnsBridge() then
    if rustHooks() then return end
    ObserveAfter('WorldMapMenuGameController', 'OnInitialize', function() VRWorldMapController(true) end)
    ObserveAfter('WorldMapMenuGameController', 'OnUninitialize', function() VRWorldMapController(false) end)
    return
  end
  ObserveAfter('WorldMapMenuGameController', 'OnInitialize', function(this) g_ctrl = this end)
  ObserveAfter('WorldMapMenuGameController', 'OnUninitialize', function() g_ctrl = nil end)
end)

registerForEvent('onUpdate', function()
  if rustOwnsBridge() then return end
  setMenuOpen(g_ctrl ~= nil and gameSaysInMenu())
end)

registerForEvent('onShutdown', function()
  if rustOwnsBridge() then return end
  setMenuOpen(false)
end)
