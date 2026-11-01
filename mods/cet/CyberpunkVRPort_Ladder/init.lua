-- Ladder geometry/state only. Tracking and grip movement stay in the native
-- coherent head/hand frame; no poses or player transforms are written in Lua.
local owner, description
local topKey, topMatrix
local topPoll = 0
local reportedError = false
local topReportedError = false
local function onLadder(state) return state >= 10 and state <= 12 end
local function clear()
    owner, description, topKey, topMatrix = nil, nil, nil, nil
    topPoll = 0
end
local function publishTop(player, dt)
    if not player or not description then return end
    local p = description.position
    local key = string.format('%.3f/%.3f/%.3f/%.3f', p.x, p.y, p.z, description.topHeightFromPosition)
    if key ~= topKey then topKey, topMatrix, topPoll = key, nil, 0 end
    topPoll = topPoll - (dt or .016)
    if not topMatrix and topPoll <= 0 then
        topPoll = .25
        local ladder = Game.GetTargetingSystem():GetLookAtObject(player, false, false)
        if ladder and ladder:GetClassName().value == 'Ladder' then
            local at = ladder:GetWorldPosition()
            local distance = (at.x-p.x)^2 + (at.y-p.y)^2 + (at.z-p.z)^2
            if distance < .0001 then
                for _, component in ipairs(Game.VREntityComponents(ladder)) do
                    if component:GetName().value:find('generic_ladder_f_finisher_rail', 1, true) then
                        topMatrix = component:GetLocalToWorld()
                        break
                    end
                end
            end
        end
    end
    if topMatrix then
        if Game.SetVRLadderTopRails(player, p, topMatrix.W, topMatrix.X, topMatrix.Y, topMatrix.Z) ~= 1 then
            topMatrix = nil
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
    Override("gamestateMachineGameScriptInterface", "GetActionValue", function(self, name, wrapped)
        local value = wrapped(name)
        if name.value ~= "MoveY" or math.abs(value) > .15 then return value end
        local player = Game.GetPlayer()
        if not self.owner or not player or self.owner:GetEntityID().hash ~= player:GetEntityID().hash then return value end
        local axis = Game.GetVRLadderDebug(20)
        return axis ~= 0 and axis or value
    end)
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
