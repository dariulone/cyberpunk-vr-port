-- THE DEPTH OF THE OBJECT AN INTERACTION PROMPT IS ABOUT, for the native side to place the prompt panel
-- at (include/Runtimes/InteractionDepth.hpp). Only while a prompt is on screen; otherwise it publishes 0.
--
-- The interaction system picks its object from the centre of the view, so the distance is measured along
-- the camera's forward axis: to the look-at object's position (items, NPCs, devices), or, when there is
-- none or it reports a point behind the camera, to the first static surface the forward ray hits (doors,
-- panels built into walls).
local M = { depth = 0, error = nil }
local elapsed = 0
local staticGroup = nil
local MAX_RAY = 6.0

local function promptVisible(boards, defs)
    local ui = boards:Get(defs.UIInteractions)
    if not ui then return false end
    local hub = FromVariant(ui:GetVariant(defs.UIInteractions.InteractionChoiceHub))
    return hub ~= nil and hub.choices ~= nil and #hub.choices > 0
end

local function measure()
    local player = Game.GetPlayer()
    if not player then return 0 end
    local defs = GetAllBlackboardDefs()
    local boards = Game.GetBlackboardSystem()
    if not boards or not promptVisible(boards, defs) then return 0 end
    local cam = Game.GetCameraSystem()
    if not cam then return 0 end
    local t = Transform.new(); cam:GetActiveCameraWorldTransform(t)
    local o = t.position
    local f = cam:GetActiveCameraForward()
    if not o or not f then return 0 end
    -- 1. The object the interaction system is looking at.
    local targeting = Game.GetTargetingSystem()
    local object = targeting and targeting:GetLookAtObject(player, false, false)
    if object then
        local p = object:GetWorldPosition()
        local d = (p.x - o.x) * f.x + (p.y - o.y) * f.y + (p.z - o.z) * f.z
        if d > 0.2 and d < 20 then return d end
    end
    -- 2. The first static surface straight ahead.
    local sq = Game.GetSpatialQueriesSystem()
    if not sq then return 0 end
    staticGroup = staticGroup or CName.new("Static")
    local a = Vector4.new(o.x, o.y, o.z, 1)
    local b = Vector4.new(o.x + f.x * MAX_RAY, o.y + f.y * MAX_RAY, o.z + f.z * MAX_RAY, 1)
    local ok, res = sq:SyncRaycastByCollisionGroup(a, b, staticGroup, false, false)
    if ok and res and res.position then
        local p = res.position
        local d = (p.x - o.x) * f.x + (p.y - o.y) * f.y + (p.z - o.z) * f.z
        if d > 0.2 then return d end
    end
    return 0
end

function M.tick(dt, overlayOpen)
    elapsed = elapsed + math.max(tonumber(dt) or 0, 0)
    if elapsed < 0.033 then return end
    elapsed = 0
    if type(SetVRInteractionDepth) ~= 'function' then return end
    local depth = 0
    if not overlayOpen then
        local ok, d = pcall(measure)
        M.error = not ok and tostring(d) or nil
        depth = ok and tonumber(d) or 0
    end
    M.depth = depth
    SetVRInteractionDepth(depth)
end

return M
