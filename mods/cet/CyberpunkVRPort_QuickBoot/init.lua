-- CyberpunkVRPort QuickBoot
-- Development-only helper: when the first main menu appears, resume the most recent save once.
-- It never runs again after returning to the menu later in the same process.

local QuickBoot = {
    enabled = true,
    delay = 0.5,
    armed = false,
    fired = false,
    elapsed = 0.0,
    configPath = "quickboot.json",
    statusPath = "quickboot_state.txt",
    detailedStatusPath = "quickboot_status.json",
    runPath = "quickboot_run.json",
    runId = "manual",
    phase = "starting",
    phaseElapsed = 0.0,
    stableFrames = 0,
    loadingFrames = 0,
    readySeconds = 0.0,
    loadingSeconds = 0.0,
    statusElapsed = 0.0,
    menuInitialized = false,
    savesReady = false,
    menuController = nil,
    continueItem = nil,
    menuItems = {},
    resumeMethod = nil,
    requireNativeReady = false,
    resumeAllowed = false,
    gateElapsed = 0.0,
    saveName = nil,   -- an owned run's save (quickboot_run.json "save"); nil = the latest, as Continue
    saveNames = nil,  -- the menu's save list, newest first (the index LoadModdedSave takes)
}

local function writeStatus(value)
    QuickBoot.phase = value
    local file = io.open(QuickBoot.statusPath, "w")
    if file then file:write(value .. "\n"); file:close() end
end

local function readJson(path)
    local file = io.open(path, "r")
    if not file then return nil end
    local body = file:read("*a")
    file:close()
    local ok, value = pcall(json.decode, body or "")
    return ok and type(value) == "table" and value or nil
end

local function saveConfirmation()
    local layer=Game.VRInkLayer(CName.new("inkSystemNotificationsLayer"))
    local root=layer and layer:GetVirtualWindow()
    if not root then return {ready=false} end
    local stack={{widget=root,depth=0}};local count=0;local popups={}
    while #stack>0 do
        local entry=table.remove(stack);local widget=entry.widget;count=count+1
        if count>512 or entry.depth>24 then return {ready=false} end
        if widget:IsVisible() and widget:GetOpacity()>.01 then
            local name=NameToString(widget:GetName());local proof=entry.proof
            if name=="popup" then proof={};popups[#popups+1]=proof end
            -- The native warning has no exposed logic controller. Its own
            -- ModLoadingTitle widget identifies it independently of language.
            -- Both buttons must belong to that same visible popup subtree.
            if proof then
                if name=="ModLoadingTitle" and widget:IsA(CName.new("inkTextWidget")) then proof.modded=true end
                if name=="buttonConfirm" or name=="buttonCancel" then
                    local controller=widget:GetController()
                    if controller and controller:IsA(CName.new("inkButtonController")) then proof[name]=true end
                end
            end
            if widget:IsA(CName.new("inkCompoundWidget")) then
                local children=widget:GetNumChildren()
                if count+#stack+children>512 then return {ready=false} end
                for i=children-1,0,-1 do stack[#stack+1]={widget=widget:GetWidgetByIndex(i),depth=entry.depth+1,proof=proof} end
            end
        end
    end
    local p=#popups==1 and popups[1]
    return {kind=p and p.modded and "modded-save" or "unknown",ready=p and p.modded and p.buttonConfirm and p.buttonCancel or false}
end

local function writeDetailedStatus(extra)
    local value = {
        runId = QuickBoot.runId,
        phase = QuickBoot.phase,
        enabled = QuickBoot.enabled,
        fired = QuickBoot.fired,
        stableFrames = QuickBoot.stableFrames,
        loadingFrames = QuickBoot.loadingFrames,
        continueReady = false,
        updatedAt = os.time(),
        menuItems = QuickBoot.menuItems,
        resumeMethod = QuickBoot.resumeMethod,
        save = QuickBoot.saveName,
    }
    if QuickBoot.saveName and not QuickBoot.fired then
        value.saves = {}
        for i, save in ipairs(QuickBoot.saveNames or {}) do value.saves[i] = tostring(save) end
    end
    for key, item in pairs(extra or {}) do value[key] = item end
    if QuickBoot.fired and QuickBoot.requireNativeReady and value.preGame then
        local ok, confirmation=pcall(saveConfirmation)
        value.loadConfirmation=ok and confirmation or {ready=false,error=tostring(confirmation)}
    end
    if QuickBoot.fired then
        local ok, evidence = pcall(function()
            local defs = GetAllBlackboardDefs()
            local board = Game.GetBlackboardSystem():Get(defs.UI_System)
            local result = { menuBlackboard = board and board:GetBool(defs.UI_System.IsInMenu) }
            result.menuControllers = {}
            local layer = Game.VRInkLayer(CName.new("inkMenuLayer"))
            if layer then
                for _, controller in ipairs(layer:GetGameControllers()) do
                    local name = controller:GetClassName()
                    local root = controller:GetRootWidget()
                    result.menuControllers[#result.menuControllers + 1] = {
                        class = name and (name.value or tostring(name)) or "",
                        visible = root and root:IsVisible() or false,
                    }
                end
            end
            return result
        end)
        if ok then value.menuEvidence = evidence end
    end
    local ok, encoded = pcall(json.encode, value)
    if not ok then return end
    local file = io.open(QuickBoot.detailedStatusPath, "w")
    if file then file:write(encoded); file:close() end
end

local function loadConfig()
    local config = readJson(QuickBoot.configPath)
    if not config then return end
    if type(config.enabled) == "boolean" then QuickBoot.enabled = config.enabled end
    local delay = tonumber(config.delay)
    if delay and delay >= 0.0 and delay <= 30.0 then QuickBoot.delay = delay end
end

local function saveConfig()
    local file = io.open(QuickBoot.configPath, "w")
    if not file then return end
    file:write(string.format('{\n  "enabled": %s,\n  "delay": %.2f\n}\n',
        tostring(QuickBoot.enabled), QuickBoot.delay))
    file:close()
end

-- On a modded game the menu loads through LoadModdedSave, which first raises a native warning that no script
-- controller answers; a test run has accepted the mods already, so it loads the way an unmodded game does.

-- The run named a save: load that one through the menu's own call. Never falls back to the latest -- the latest may
-- be the save being avoided.
local function resumeNamedSave(controller)
    local name = QuickBoot.saveName
    local index = nil
    for i, save in ipairs(QuickBoot.saveNames or {}) do
        if tostring(save) == name then index = i - 1; break end
    end
    if not controller or not index then
        print("[CyberpunkVR QuickBoot] save " .. name .. " is not in the menu's list")
        writeStatus("failed:named-save-missing:" .. name)
        return false
    end
    local ok, err = pcall(function() controller:LoadSaveInGame(index) end)
    if not ok then
        writeStatus("failed:named-save:" .. tostring(err))
        return false
    end
    QuickBoot.resumeMethod = "named-save:" .. name
    print(string.format("[CyberpunkVR QuickBoot] loading save %s (index %d)", name, index))
    writeStatus("resumed:named-save")
    return true
end

local function resumeLastSave()
    local controller, item = QuickBoot.menuController, QuickBoot.continueItem
    QuickBoot.menuController = nil
    QuickBoot.continueItem = nil
    if QuickBoot.saveName then return resumeNamedSave(controller) end
    if controller and item and not QuickBoot.viaCheckpoint then
        -- Continue itself is LoadLastCheckpoint(false) on an unmodded game, LoadModdedSave(0) on a modded one:
        -- the same newest save, which the menu showing Continue proves is there.
        local ok, err = pcall(function() controller:GetSystemRequestsHandler():LoadLastCheckpoint(false) end)
        if ok then
            QuickBoot.resumeMethod = "menu-continue"
            print("[CyberpunkVR QuickBoot] resumed the newest save, as the menu's Continue does")
            writeStatus("resumed:menu-continue")
            return true
        end
        writeStatus("failed:menu-continue:" .. tostring(err))
        return false
    end
    local handler = Game.GetSystemRequestsHandler()
    if not handler then
        print("[CyberpunkVR QuickBoot] SystemRequestsHandler is unavailable")
        writeStatus("failed:no-system-requests-handler")
        return false
    end

    -- The RTTI name has moved across game releases. Keep the alternatives isolated and report
    -- the exact one that works instead of silently binding the test flow to one guessed method.
    local attempts = {
        { label = "LoadLastCheckpoint(false)", call = function() handler:LoadLastCheckpoint(false) end },
        { label = "LoadLastCheckpoint()", call = function() handler:LoadLastCheckpoint() end },
        { label = "RequestLoadLastCheckpoint()", call = function() handler:RequestLoadLastCheckpoint() end },
    }

    for _, attempt in ipairs(attempts) do
        local ok, err = pcall(attempt.call)
        if ok then
            QuickBoot.resumeMethod = attempt.label
            print("[CyberpunkVR QuickBoot] resumed via " .. attempt.label)
            writeStatus("resumed:" .. attempt.label)
            return true
        end
        print("[CyberpunkVR QuickBoot] " .. attempt.label .. " failed: " .. tostring(err))
    end

    writeStatus("failed:no-compatible-continue-method")
    print("[CyberpunkVR QuickBoot] no compatible Continue method was found")
    return false
end

local function armFromMainMenu(source)
    if QuickBoot.fired or QuickBoot.armed or not QuickBoot.enabled then return end
    QuickBoot.armed = true
    QuickBoot.elapsed = 0.0
    writeStatus("armed:" .. source)
    writeDetailedStatus()
    print(string.format("[CyberpunkVR QuickBoot] main menu observed by %s; resuming in %.2fs",
        source, QuickBoot.delay))
end

local function continuePanelReady(root)
    local nodes, matched = 0, 0
    local ok, ready = pcall(function()
        local stack = { { widget = root, depth = 0 } }
        local compound = CName.new("inkCompoundWidget")
        local found = false
        while #stack > 0 do
            local entry = table.remove(stack)
            nodes = nodes + 1
            if nodes > 256 or entry.depth > 16 or not entry.widget then
                error("loading widget traversal limit or invalid child")
            end
            local widget = entry.widget
            if widget:IsVisible() then
                local controller = widget:GetController()
                local name = controller and controller:GetClassName()
                name = name and (name.value or tostring(name)) or ""
                if name == "inkInitialLoadingScreenLogicController" or name == "InitialLoadingScreenLogicController" then
                    matched = matched + 1
                    -- Native readiness registers the input callback before showing
                    -- this panel; it needs both loading and animation completion.
                    local panel = inkWidgetRef.Get(controller.skipButtonPanel)
                    local visible = panel ~= nil
                    for depth = 1, 24 do
                        if not panel then break end
                        if not panel:IsVisible() then visible = false; break end
                        panel = panel:GetParentWidget()
                        if depth == 24 and panel then error("loading panel ancestry limit") end
                    end
                    found = found or visible
                end
                if widget:IsA(compound) then
                    local count = widget:GetNumChildren()
                    if type(count) ~= "number" or count < 0 or count % 1 ~= 0 or nodes + #stack + count > 256 then
                        error("loading widget child limit")
                    end
                    for i = count - 1, 0, -1 do
                        stack[#stack + 1] = { widget = widget:GetWidgetByIndex(i), depth = entry.depth + 1 }
                    end
                end
            end
        end
        return found
    end)
    return ok and ready == true, nodes, matched, not ok and tostring(ready) or nil
end

local function loadingScreenActive()
    local layer = Game.VRInkLayer(CName.new("inkLoadingLayer"))
    if not layer then return false, {}, false end
    local active = false
    local continueReady, walkFailed = false, false
    local controllers = {}
    for _, controller in ipairs(layer:GetGameControllers()) do
        local name = controller:GetClassName()
        name = name and (name.value or tostring(name)) or ""
        local root = controller:GetRootWidget()
        local visible = root and root:IsVisible() or false
        local row = { class = name, visible = visible }
        controllers[#controllers + 1] = row
        -- The loading layer can host other loading controllers as well as the
        -- initial/fast-travel supervisors. Never report ready through one.
        if visible then
            active = true
            if name == "inkInitialLoadingControllerSupervisor" then
                row.continueReady, row.continueNodes, row.continueControllers, row.continueError = continuePanelReady(root)
                continueReady = continueReady or row.continueReady
                walkFailed = walkFailed or row.continueError ~= nil
            end
        end
    end
    return active, controllers, continueReady and not walkFailed
end

local function sampleWorld(deltaSeconds)
    local ok, sample = pcall(function()
        local requests = Game.GetSystemRequestsHandler()
        local player = Game.GetPlayer()
        local preGame = not requests or requests:IsPreGame()
        local paused = requests and requests:IsGamePaused() or false
        local loading, loadingControllers, continueReady = loadingScreenActive()
        local position = player and player:GetWorldPosition() or nil
        local camera = player and player:GetFPPCameraComponent() or nil
        local valid = position and type(position.x) == "number" and
            type(position.y) == "number" and type(position.z) == "number" and
            position.x == position.x and position.y == position.y and
            position.z == position.z and camera ~= nil
        return { preGame = preGame, paused = paused, loading = loading,
            player = player ~= nil, worldValid = valid == true,
            loadingControllers = loadingControllers, continueReady = continueReady == true }
    end)
    if not ok then
        QuickBoot.stableFrames = 0
        QuickBoot.readySeconds = 0
        if QuickBoot.phase ~= "waiting_for_game" then writeStatus("waiting_for_game") end
        writeDetailedStatus({ continueReady = false })
        return
    end

    local ready = sample.player and sample.worldValid and not sample.preGame and
        not sample.paused and sample.loading == false
    QuickBoot.stableFrames = ready and QuickBoot.stableFrames + 1 or 0
    QuickBoot.loadingFrames = sample.loading == true and QuickBoot.loadingFrames + 1 or 0
    QuickBoot.readySeconds = ready and QuickBoot.readySeconds + deltaSeconds or 0
    QuickBoot.loadingSeconds = sample.loading == true and
        QuickBoot.loadingSeconds + deltaSeconds or 0
    local phase = "loading"
    if ready and QuickBoot.stableFrames >= 30 and QuickBoot.readySeconds >= 1.0 then
        phase = "world_ready"
    elseif sample.player and sample.worldValid and not sample.preGame and not sample.paused and
           sample.loading == true and sample.continueReady and
           QuickBoot.loadingFrames >= 30 and QuickBoot.loadingSeconds >= 1.0 then
        phase = "awaiting_continue"
    end
    local changed = phase ~= QuickBoot.phase
    if changed then
        writeStatus(phase)
        QuickBoot.statusElapsed = 0
    end
    QuickBoot.statusElapsed = QuickBoot.statusElapsed + deltaSeconds
    if QuickBoot.statusElapsed >= 0.5 or changed then
        QuickBoot.statusElapsed = 0
        writeDetailedStatus(sample)
    end
end

registerForEvent("onInit", function()
    loadConfig()
    local run = readJson(QuickBoot.runPath)
    if run and type(run.runId) == "string" and #run.runId <= 80 then
        QuickBoot.runId = run.runId
        QuickBoot.requireNativeReady = run.requireNativeReady == true and
            tonumber(run.expiresAt) ~= nil and run.expiresAt >= os.time()
        QuickBoot.resumeAllowed = run.allowResume == true
        if QuickBoot.requireNativeReady and type(run.save) == "string" and run.save ~= "" then
            QuickBoot.saveName = run.save
        end
    end
    saveConfig()
    writeStatus(QuickBoot.enabled and "ready" or "disabled")
    writeDetailedStatus()

    -- The native base OnInitialize runs before the actual single-player menu
    -- finishes and before its save-list callback. Wait for both real events.
    local function menuReady()
        if QuickBoot.menuInitialized and QuickBoot.savesReady then
            armFromMainMenu("SingleplayerMenuGameController:menu-and-saves-ready")
        end
    end
    local observers = 0
    local function captureMenuItem(controller, label, value, isAction)
        if QuickBoot.fired or not controller then return end
        local name = controller:GetClassName()
        name = name and (name.value or tostring(name)) or ""
        if name ~= "SingleplayerMenuGameController" then return end
        local event = not isAction and (value.value or tostring(value)) or ""
        QuickBoot.menuItems[#QuickBoot.menuItems + 1] = {
            label = tostring(label), value = tostring(value), event = event, isAction = isAction,
        }
        local isContinue = isAction and value == PauseMenuAction.QuickLoad or
            not isAction and event == "OnContinue"
        if not isContinue then return end
        local item = NewObject("PauseMenuListItemData")
        item.label = label
        if isAction then item.action = value else item.eventName = value end
        QuickBoot.continueItem = item
        QuickBoot.menuController = controller
    end
    pcall(function()
        Observe("gameuiMenuItemListGameController", "AddMenuItem;script_ref<String>CName", function(controller, label, event)
            captureMenuItem(controller, label, event, false)
        end)
        Observe("gameuiMenuItemListGameController", "AddMenuItem;script_ref<String>PauseMenuAction", function(controller, label, action)
            captureMenuItem(controller, label, action, true)
        end)
    end)
    local okMenu = pcall(function()
        Observe("SingleplayerMenuGameController", "FinishMenuInitialization", function(controller)
            QuickBoot.menuController = controller
            QuickBoot.menuInitialized = true
            menuReady()
        end)
    end)
    local okSaves = pcall(function()
        Observe("SingleplayerMenuGameController", "OnSavesForLoadReady", function(_, saves)
            QuickBoot.saveNames = saves
            QuickBoot.savesReady = true
            menuReady()
        end)
    end)
    if okMenu then observers = observers + 1 end
    if okSaves then observers = observers + 1 end
    if observers ~= 2 then writeStatus("failed:menu-ready-observers") end
    Observe("SingleplayerMenuGameController", "OnUninitialize", function()
        QuickBoot.menuController = nil
        QuickBoot.continueItem = nil
    end)
    -- 2.5 opens a once-per-patch CD PROJEKT RED Account pitch (and after it the telemetry and marketing consent
    -- screen) before the single-player menu exists. Those choices are the player's: a launch the tools own
    -- leaves both screens untouched and resumes the last save from under them, so the player still meets them
    -- on their own next launch.
    pcall(function()
        Observe("PitchMenuGameController", "OnInitialize", function()
            if not QuickBoot.requireNativeReady then return end
            QuickBoot.viaCheckpoint = true
            armFromMainMenu("PitchMenuGameController:first-launch-screen")
        end)
    end)

    print(string.format("[CyberpunkVR QuickBoot] ready; enabled=%s delay=%.2fs observers=%d",
        tostring(QuickBoot.enabled), QuickBoot.delay, observers))
end)
-- CET takes registerHotkey away once init.lua's root has run; inside onInit it was nil.
registerHotkey("cyberpunkvr_quickboot_toggle", "CyberpunkVR QuickBoot: toggle", function()
    QuickBoot.enabled = not QuickBoot.enabled
    saveConfig()
    writeStatus(QuickBoot.enabled and "ready" or "disabled")
    writeDetailedStatus()
    print("[CyberpunkVR QuickBoot] " .. (QuickBoot.enabled and "enabled" or "disabled") ..
        " for the next launch")
end)
registerForEvent("onUpdate", function(deltaSeconds)
    if not QuickBoot.enabled then
        QuickBoot.armed = false
        return
    end
    -- After the resume the world is sampled for the launch tools' readiness report. A launch the tools
    -- own (requireNativeReady) keeps it all session -- probes check quickboot_status.json later -- but a
    -- player's own launch stops at world_ready: the report walks the loading layer every frame and writes
    -- the status file twice a second, and a CET file open costs 0.3-1.2 ms on the game thread.
    if QuickBoot.fired then
        if QuickBoot.requireNativeReady or QuickBoot.phase ~= "world_ready" then sampleWorld(deltaSeconds) end
        return
    end
    if not QuickBoot.armed then return end
    QuickBoot.elapsed = QuickBoot.elapsed + deltaSeconds
    if QuickBoot.elapsed < QuickBoot.delay then return end
    if QuickBoot.requireNativeReady and not QuickBoot.resumeAllowed then
        QuickBoot.gateElapsed = QuickBoot.gateElapsed + deltaSeconds
        if QuickBoot.phase ~= "awaiting_native_menu" then
            writeStatus("awaiting_native_menu")
            writeDetailedStatus()
        end
        if QuickBoot.gateElapsed < 0.1 then return end
        QuickBoot.gateElapsed = 0
        local run = readJson(QuickBoot.runPath)
        QuickBoot.resumeAllowed = run and run.runId == QuickBoot.runId and run.allowResume == true
        if not QuickBoot.resumeAllowed then return end
    end

    -- Claim the one-shot before calling into the game. A throwing or renamed RTTI method must not
    -- turn into an every-frame load loop.
    QuickBoot.fired = true
    QuickBoot.armed = false
    writeStatus("resuming")
    writeDetailedStatus()
    resumeLastSave()
    writeDetailedStatus()
end)
