-- Decorate only the native HUD's hint and choice rows. The game keeps its own
-- localized caption, icon, hold progress, and rules for when a row is visible.
local H = {}
local previous, frame, remaining = {}, 0, 0
local hudRoots = {
    gameuiInputHintManagerGameController = true, InputHintManagerGameController = true,
    HUDButtonHints = true, gameuiHUDButtonHints = true,
}
local choiceRoots = {
    gameuiInteractionsHubGameController = true, InteractionsHubGameController = true,
    dialogWidgetGameController = true, interactionWidgetGameController = true,
}
local function report(err)
    if H.error ~= tostring(err) then
        H.error = tostring(err)
        spdlog.error('[CyberFinger HUD hints] ' .. H.error)
    end
end
local function walk(widget, budget, visit)
    if not widget or budget.left <= 0 then return end
    budget.left = budget.left - 1
    if not widget:IsVisible() or widget:GetOpacity() <= 0 then return end
    if visit(widget:GetController()) or not widget:IsA('inkCompoundWidget') then return end
    for child = 0, math.min(widget:GetNumChildren(), budget.left) - 1 do
        walk(widget:GetWidgetByIndex(child), budget, visit)
    end
end

-- Combination hints can contain several input displays. Leave these to the game.
local function singleAction(container)
    local action, displays = nil, 0
    local budget = {left = 24}
    walk(container, budget, function(c)
        if c and c:IsA('inkInputDisplayController') then
            displays = displays + 1
            action = c:GetInputAction()
            return true
        end
    end)
    if displays == 1 and budget.left > 0 then return action end
end

local function scan(current, choices)
    local player = Game.GetPlayer()
    if not player or not player:PlayerLastUsedPad() then return end
    local layer = Game.VRInkLayer(CName.new('inkHUDLayer'))
    if not layer then return end
    local function show(ref, label)
        if not ref or not label or label == '' then return end
        local text = inkWidgetRef.Get(ref)
        if text and text:IsVisible() then
            text:VRShowControlHint(label, frame)
            current[#current + 1] = text
        end
    end
    local function visit(c)
        if not c then return end
        local class = NameToString(c:GetClassName())
        if class == 'gameuiInputHintController' or class == 'InputHintController' then
            local container = c.inputDisplayContainer and inkWidgetRef.Get(c.inputDisplayContainer)
            local action = singleAction(container)
            if action then show(c.textWidgetRef, VRCyberFingerHint(action, false)) end
        elseif class == 'interactionItemLogicController' then
            show(c.m_label, VRCyberFingerHint(c.vrCFAction, c.vrCFHold))
        elseif class == 'DialogChoiceLogicController' then
            if c.m_isSelected and not c.m_hasDedicatedInput then
                local label = VRCyberFingerHint(CName.new('ChoiceApply'), false)
                if label ~= '' and c.m_InputView and c.m_InputView.m_AllItemsNum > 1 then
                    local scroll = VRCyberFingerHint(CName.new('ChoiceScroll'), false)
                    if scroll ~= '' then label = scroll .. ' / ' .. label end
                end
                show(c.m_ActiveTextRef, label)
            end
        else
            return false
        end
        return true
    end
    for _, controller in ipairs(layer:GetGameControllers()) do
        local class = NameToString(controller:GetClassName())
        if hudRoots[class] or (choices and choiceRoots[class]) then
            -- A controller can disappear between discovering its root and reading
            -- its children. Other HUD panels can still update in that frame.
            local ok, err = pcall(walk, controller:GetRootWidget(), {left = 256}, visit)
            if not ok then report(err) end
        end
    end
end

function H.update(dt, enabled, choices)
    if not enabled and #previous == 0 then remaining = 0; return end
    remaining = remaining - (dt or 0)
    if enabled and remaining > 0 then return end
    remaining = 0.25
    frame = frame % 4294967294 + 1
    local current = {}
    if enabled and type(VRCyberFingerHint) == 'function' then
        local ok, err = pcall(scan, current, choices)
        if not ok then report(err) end
    end
    for _, text in ipairs(previous) do
        -- A row may already have been destroyed when unloading a save.
        pcall(function() text:VRRestoreControlHint(frame) end)
    end
    previous = current
end
return H
