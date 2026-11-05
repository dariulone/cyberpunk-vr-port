-- Ladder geometry/state only. Tracking and grip movement stay in the native
-- coherent head/hand frame; no poses or player transforms are written in Lua.
local owner, description
local topKey, topMatrix, topFound, moduleTop
local topPoll = 0
local reportedError = false
local topReportedError = false
local function onLadder(state) return state >= 10 and state <= 12 end
local function clear()
    owner, description, topKey, topMatrix, topFound, moduleTop = nil, nil, nil, nil, false, nil
    topPoll = 0
end
-- THE LADDER'S MODULES, by their mesh (FNV1a64 of base\environment\architecture\common\generic_ladder\
-- generic_ladder_<name>.mesh) and their height: each has a rung every 0.4 m from its foot up to its top (measured from
-- the meshes 2026-11-05, build/ladder-rungs-20261105). The description's top is not always the ladder's: one of 10 m
-- (4 + 4 + 2 under a finisher rail) says 9.4, and the last two rungs and the rail could not be gripped.
local MODULES = {
    ['13525070408616376406'] = 1.6,  -- a_h160
    ['12058514104176874699'] = 2.0,  -- a_h200
    ['6760872976104212387'] = 2.8,   -- a_h280
    ['14213707566087999640'] = 3.6,  -- a_h360
    ['5486285113709760136'] = 4.0,   -- b_h400
    ['13340627325582521188'] = 3.0,  -- c_h300
}
local function meshKey(component)
    local ok, h = pcall(function() return component.mesh.hash end)
    if not ok or h == nil then return nil end
    return (string.gsub(tostring(h), 'U?LL$', ''))
end
-- The top of the highest module standing on the ladder's line, above its foot; nil when none is known.
local function measureModules(ladder, p, up)
    local best
    for _, component in ipairs(Game.VREntityComponents(ladder)) do
        local height = MODULES[meshKey(component) or '']
        if height then
            local m = component:GetLocalToWorld()
            local d = Vector4.new(m.W.x - p.x, m.W.y - p.y, m.W.z - p.z, 0)
            local along = d.x * up.x + d.y * up.y + d.z * up.z
            local sx, sy, sz = d.x - up.x * along, d.y - up.y * along, d.z - up.z * along
            local stands = m.Z.x * up.x + m.Z.y * up.y + m.Z.z * up.z
            if sx * sx + sy * sy + sz * sz < .09 and stands > .98 then
                local top = along + height * stands
                if best == nil or top > best then best = top end
            end
        end
    end
    return best
end
local function publishTop(player, dt)
    if not player or not description then return end
    local p = description.position
    local key = string.format('%.3f/%.3f/%.3f/%.3f', p.x, p.y, p.z, description.topHeightFromPosition)
    if key ~= topKey then topKey, topMatrix, topFound, moduleTop, topPoll = key, nil, false, nil, 0 end
    topPoll = topPoll - (dt or .016)
    if not topFound and topPoll <= 0 then
        topPoll = .25
        local ladder = Game.GetTargetingSystem():GetLookAtObject(player, false, false)
        if ladder and ladder:GetClassName().value == 'Ladder' then
            local at = ladder:GetWorldPosition()
            local distance = (at.x-p.x)^2 + (at.y-p.y)^2 + (at.z-p.z)^2
            if distance < .0001 then
                topFound = true
                moduleTop = measureModules(ladder, p, description.up)
                for _, component in ipairs(Game.VREntityComponents(ladder)) do
                    if component:GetName().value:find('generic_ladder_f_finisher_rail', 1, true) then
                        topMatrix = component:GetLocalToWorld()
                        break
                    end
                end
            end
        end
    end
    -- the measured top first: the rails are accepted at the ladder's physical top
    -- (one refused -- the plugin's geometry not fresh yet, another ladder -- is looked for again)
    if moduleTop then
        if Game.SetVRLadderMeasuredTop(player, p, moduleTop) ~= 1 then moduleTop, topFound = nil, false end
    end
    if topMatrix then
        if Game.SetVRLadderTopRails(player, p, topMatrix.W, topMatrix.X, topMatrix.Y, topMatrix.Z) ~= 1 then
            topMatrix, topFound = nil, false
        end
    end
end
registerForEvent("onInit", function()
    local function capture(_, _, context, interface)
        local ok = pcall(function()
            owner = interface.owner
            description = context:GetConditionScriptableParameter(CName.new("usingLadder"))
                or context:GetTemporaryScriptableParameter(CName.new("usingLadder"))
        end)
        if not ok then clear() end
    end
    for _, class in ipairs({"LadderEvents", "LadderSprintEvents", "LadderSlideEvents"}) do
        ObserveAfter(class, "OnUpdate", capture)
    end
    ObserveAfter("PlayerPuppet", "OnGameAttached", clear)
    -- The plugin gives a resting MoveY the climbing axis natively (src/Natives/ActionReads.cpp, debug value 21 = 1);
    -- this override is the fallback for a plugin that could not (it runs on every state machine's MoveY read).
    if Game.GetVRLadderDebug(21) ~= 1 then
        Override("gamestateMachineGameScriptInterface", "GetActionValue", function(self, name, wrapped)
            local value = wrapped(name)
            if name.value ~= "MoveY" or math.abs(value) > .15 then return value end
            local player = Game.GetPlayer()
            if not self.owner or not player or self.owner:GetEntityID().hash ~= player:GetEntityID().hash then return value end
            local axis = Game.GetVRLadderDebug(20)
            return axis ~= 0 and axis or value
        end)
    end
end)
-- The field id, read once: blackboard definitions are global and live as long as the game.
local locomotionDetailedId
-- A plain function for pcall: `pcall(function() ... end)` would allocate a closure every frame.
local function bridgeState(dt)
    local player = Game.GetPlayer()
    local state = -1
    if player then
        local board = player:GetPlayerStateMachineBlackboard()
        if board then
            locomotionDetailedId = locomotionDetailedId or GetAllBlackboardDefs().PlayerStateMachine.LocomotionDetailed
            state = board:GetInt(locomotionDetailedId)
        end
    end
    if not onLadder(state) then clear() end
    Game.SetVRLadderState(owner or player, state, Game.GetSystemRequestsHandler():IsGamePaused() and 1 or 0, description)
    -- Read actual finisher placement once, only from the same ladder. A
    -- missing/different look-at target never changes the state bridge.
    local topOk, topErr = pcall(publishTop, player, dt)
    if not topOk and not topReportedError then
        print('[ladder] top rail geometry unavailable: ' .. tostring(topErr))
    end
    topReportedError = not topOk
end
registerForEvent("onUpdate", function(dt)
    local ok, err = pcall(bridgeState, dt)
    if not ok and not reportedError then
        reportedError = true;print("[ladder] state bridge unavailable: " .. tostring(err))
    elseif ok then reportedError = false end
end)
registerForEvent("onShutdown", function()
    pcall(function() Game.SetVRLadderState(nil, -1, 1, nil) end)
end)
