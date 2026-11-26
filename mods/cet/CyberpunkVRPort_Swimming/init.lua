-- Only state is bridged here. Controller/head poses and gesture recognition
-- remain in the plugin's coherent native hand publication.
local fast = -1
local reportedError = false

-- THE RUST BUILD RUNS THE STATE BRIDGE (rust/crates/cvr-ffi/src/mechanics.rs swimming, bit 4 of VRRustMechanics): the
-- blackboard and water reads, the publication every frame and the shutdown. The overrides stay here as they are, and
-- the observers below report the fast-stroke state to the plugin. The reference build has no such native, and the whole
-- bridge keeps running here.
local rustOwned = nil
local function rustOwnsBridge()
    if rustOwned == nil then
        rustOwned = type(VRRustMechanics) == 'function' and math.floor(VRRustMechanics() / 16) % 2 == 1
    end
    return rustOwned
end
local function setFast(value)
    if rustOwnsBridge() then VRSwimmingFast(value) else fast = value end
end

-- THE RUST BUILD HOOKS THESE ITSELF (rust/crates/cvr-ffi/src/mechanics/game_hooks.rs, bit 1 of VRRustMechanics2).
local function rustHooks()
  return type(VRRustMechanics2) == 'function' and math.floor(VRRustMechanics2() / 2) % 2 == 1
end
registerForEvent("onInit", function()
    -- Script decisions and native locomotion must see the same gesture input. The plugin adds the gestures to the
    -- state machines' reads natively (src/Natives/SwimmingScript.cpp, debug value 16 = 1); these overrides are the
    -- fallback for a plugin that could not, and they cost ~0.3 ms a tick on the game thread.
    if Game.GetVRSwimmingDebug(16) ~= 1 then
        local actions = { MoveY = 20, ToggleSprint = 21, Jump = 22, Dive = 23 }
        local function valueFor(owner, name)
            local player = Game.GetPlayer()
            if not owner or not player or owner:GetEntityID().hash ~= player:GetEntityID().hash then return 0 end
            local index = actions[name.value]
            return index and Game.GetVRSwimmingDebug(index) or 0
        end
        Override("gamestateMachineGameScriptInterface", "GetActionValue", function(self, name, wrapped)
            local value = wrapped(name)
            local added = valueFor(self.owner, name)
            return added > 0 and math.max(value, added) or value
        end)
        Override("gamestateMachineGameScriptInterface", "IsActionJustPressed", function(self, name, wrapped)
            local pressed = wrapped(name)
            -- Dive is a surface transition; the command clears upon entering water.
            return pressed or (name.value == "Dive" and valueFor(self.owner, name) > 0)
        end)
    end
    if rustHooks() then return end
    local function watch(class, method, value)
        ObserveAfter(class, method, function() setFast(value) end)
    end
    watch("SwimmingSurfaceEvents", "OnEnter", 0)
    watch("SwimmingDivingEvents", "OnEnter", 0)
    for _, class in ipairs({ "SwimmingSurfaceFastEvents", "SwimmingFastDivingEvents" }) do
        watch(class, "OnEnter", 1)
        watch(class, "OnExit", 0)
    end
    ObserveAfter("PlayerPuppet", "OnGameAttached", function() setFast(-1) end)
end)

-- The two field ids, read once: blackboard definitions are global and live as long as the game.
local swimmingId, highLevelId
-- A plain function for pcall: `pcall(function() ... end)` would allocate a closure every frame.
local function bridgeState()
    local player = Game.GetPlayer()
    local state = -1
    local waterContext = false
    local paused = Game.GetSystemRequestsHandler():IsGamePaused()
    if player then
        local board = player:GetPlayerStateMachineBlackboard()
        if board then
            if not swimmingId then
                local defs = GetAllBlackboardDefs().PlayerStateMachine
                swimmingId, highLevelId = defs.Swimming, defs.HighLevel
            end
            state = board:GetInt(swimmingId)
            -- HighLevel stays Swimming (6) while Surface/Diving transitions
            -- briefly clear the narrower Swimming blackboard value to 0.
            waterContext = board:GetInt(highLevelId) == 6
        end
        waterContext = waterContext or player:IsUnderwater() or player:HasHeadUnderwater()
    end
    if state == 0 or state == 3 then fast = -1 end
    Game.SetVRSwimmingState(player, state, fast, paused and 1 or 0, waterContext and 1 or 0)
end

registerForEvent("onUpdate", function()
    if rustOwnsBridge() then return end
    local ok, err = pcall(bridgeState)
    if not ok and not reportedError then
        reportedError = true
        print("[swimming] state bridge unavailable: " .. tostring(err))
    elseif ok then
        reportedError = false
    end
end)

registerForEvent("onShutdown", function()
    if rustOwnsBridge() then return end
    pcall(function() Game.SetVRSwimmingState(nil, -1, -1, 1, 0) end)
end)
