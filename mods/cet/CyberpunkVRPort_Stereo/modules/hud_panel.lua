-- The plugin hands out the live HUD layer (Game.VRInkLayer). All native widget access happens
-- here on the UI/script update thread, never on the OpenXR worker.
local M = { count = 0, error = nil, phoneError = nil, lootVisible = false, refreshHiddenBounds = false }
local phoneRoots = {}
local subtitleRoots = {}
local gazeRoots = {}
local scanSecond = -1
-- CET exposes game types only after onInit, not while require() loads this file.
local boundsType
local topPath, lootPath
-- Names used every frame or every scan, made once (CName.new allocates).
local hudLayerName, virtualWindowType, mainPanelPath

local function vector(x, y)
    -- CET's generated Vector2 constructor ignores positional numeric arguments.
    local value = Vector2.new()
    value.X = x
    value.Y = y
    return value
end

local function exactPath(root, ...)
    local widget = root
    for _, name in ipairs({...}) do
        widget = widget and widget:GetWidgetByPathName(CName.new(name))
        -- A failed lookup can return the root.
        if not widget or NameToString(widget:GetName()) ~= name then return nil end
    end
    return widget
end

local function identityRoot(root)
    return exactPath(root, "SC_01", "CARD_00_info") ~= nil
        or exactPath(root, "Main", "Pane1", "right", "right_panel", "text_up", "imprint_text") ~= nil
end

-- The per-frame upkeep below runs in the plugin (VRHudPanelUpdate) when it says so: this file then finds the
-- roots once a second and hands them over, and about 40 calls a frame through CET are saved. Asked at every scan.
local native = false
local function askNative()
    local ok, on = pcall(function() return Game.VRHudPanelRootsNative() end)
    return ok and on == true and not M.refreshHiddenBounds
end
-- True when the plugin took every root; otherwise it is handed nothing and this file keeps the upkeep.
local function handOver()
    Game.VRHudPanelRootsBegin()
    local all = true
    for _, root in ipairs(gazeRoots) do
        all = Game.VRHudPanelRoot(root, root:GetParentWidget(), nil, 8, 0) and all
    end
    for _, phone in ipairs(phoneRoots) do
        all = Game.VRHudPanelRoot(phone.root, phone.window, phone.info,
            (phone.scanner and 1 or 0) + (phone.windowOrigin and 2 or 0) + (phone.interaction and 4 or 0), phone.padding or 0) and all
    end
    for _, subtitle in ipairs(subtitleRoots) do
        all = Game.VRHudPanelSubtitle(subtitle.root, subtitle.window, subtitle.info, subtitle.panel) and all
    end
    if not all then Game.VRHudPanelRootsBegin() end
    return all
end

local function updatePhoneBounds(layer)
    if not layer then
        phoneRoots = {}; subtitleRoots = {}; gazeRoots = {}; scanSecond = -1
        if native then pcall(handOver) end
        return false
    end
    boundsType = boundsType or CName.new("inkHudEntryInfo")
    topPath = topPath or CName.new("topWidgets")
    lootPath = lootPath or CName.new("looting")
    virtualWindowType = virtualWindowType or CName.new("inkVirtualWindow")
    mainPanelPath = mainPanelPath or CName.new("mainPanel")
    local lootVisible = false
    local second = os.time()
    if second ~= scanSecond then
        scanSecond = second
        phoneRoots = {}
        gazeRoots = {}
        local previousSubtitles=subtitleRoots
        subtitleRoots = {}
        for _, controller in ipairs(layer:GetGameControllers()) do
            local class = NameToString(controller:GetClassName())
            -- The braindance HUD is laid out about the screen's middle, as the dot is: both are moved onto the gaze.
            if class == "CrosshairGameControllerPersistentDot" or class == "BraindanceGameController" then
                table.insert(gazeRoots, controller:GetRootWidget())
            elseif class == "SubtitlesGameController" then
                local root=controller:GetRootWidget()
                local window=root and root:GetParentWidget()
                local info=root and root:GetUserData(boundsType)
                local panel=root and root:GetWidgetByPathName(mainPanelPath)
                if info and panel and window and window:IsA(virtualWindowType) then
                    local bounds={x=info.offset.X,y=info.offset.Y,width=info.size.X,height=info.size.Y}
                    for _,old in ipairs(previousSubtitles) do if old.root==root then bounds=old;break end end
                    table.insert(subtitleRoots,{root=root,window=window,info=info,panel=panel,
                        x=bounds.x,y=bounds.y,width=bounds.width,height=bounds.height})
                end
            elseif class == "NewHudPhoneGameController" or class == "HudPhoneGameController"
                or class == "gameuiInteractionsHubGameController" or class == "gameuiPanzerHUDGameController"
                or class == "gameuiBriefingGameController"
                or class == "scannerDetailsGameController" or class == "hudCameraController"
                or class == "hudSniperNestController"
                or class == "gameuiScannerGameController"
                or class == "CustomAnimationsHudGameController"
                or class == "TargetHitIndicatorGameController" then
                local root = controller:GetRootWidget()
                local window = root and root:GetParentWidget()
                -- Sniper HUD is a child of the shared briefing entry's root.
                -- Capture that existing window, including the native controller.
                if class == "hudSniperNestController" and window and NameToString(window:GetName())=="Root" then
                    root=window
                    window=root:GetParentWidget()
                end
                -- Authored subtitle/audio crops are left intact. Phone,
                -- interaction, vehicle/device HUDs, scanner data and story roots need bounds.
                -- Hit and kill markers: a 1x1 root centred with anchorPoint (0,0). The markers are spawned
                -- around it (200x200 holders, animated up to 2x) and weapon sway moves it by its margin,
                -- so the window crop is a padded box around the root's place in its window.
                local hitMarker = class == "TargetHitIndicatorGameController"
                if root and (hitMarker or NameToString(root:GetName()) == "Root") and window
                    and window:IsA(virtualWindowType)
                    and (class ~= "CustomAnimationsHudGameController" or identityRoot(root)) then
                    local info = root:GetUserData(boundsType)
                    if not info then
                        info = inkHudEntryInfo.new()
                        info.offset = vector(0, 0)
                        root:SetUserData(info)
                    end
                    table.insert(phoneRoots, { root = root, window = window, info = info,
                        padding = class == "CustomAnimationsHudGameController" and 32 or hitMarker and 160 or 0,
                        scanner = class == "gameuiScannerGameController",
                        windowOrigin = class == "scannerDetailsGameController" or class == "hudSniperNestController" or hitMarker,
                        interaction = class == "gameuiInteractionsHubGameController" })
                end
            end
        end
        local wasNative = native
        native = askNative()
        if native then
            local okH, all = pcall(handOver)
            native = okH and all == true
        elseif wasNative then pcall(function() Game.VRHudPanelRootsBegin() end) end
    end
    if native then return false end
    for _, phone in ipairs(phoneRoots) do
        -- Source roots retain their native visibility (only the final slot image
        -- is masked). Hidden panels need no crop work or redraw. Check every
        -- tick so the first visible frame still gets its full bounds and paint.
        local active = phone.root:IsVisible() and phone.root:GetOpacity()>0
        if not active and not M.refreshHiddenBounds and phone.wasActive~=false then
            -- Clear the last visible image once on hide, including a new source.
            phone.root:FlagForVisualInvalidation()
            phone.window:FlagForVisualInvalidation()
        end
        phone.wasActive=active
        if M.refreshHiddenBounds or active then
        if phone.interaction then
            -- Keep the game's loot plate and tooltip together in their native
            -- window. Only change the XR follow policy, never their layout.
            -- Resolve the child each update: game controllers may replace it.
            -- CET returns distinct handle wrappers for the same widget; Lua ==
            -- cannot identify ancestors. Resolve the two single-name children
            -- explicitly (a slash path can return the root).
            local top=phone.root:GetWidgetByPathName(topPath)
            if top and NameToString(top:GetName())=="topWidgets" then
                local loot=top:GetWidgetByPathName(lootPath)
                lootVisible=lootVisible or (loot~=nil and NameToString(loot:GetName())=="looting"
                    and loot:IsVisible() and loot:GetOpacity()>0
                    and top:IsVisible() and top:GetOpacity()>0
                    and active)
            end
        end
        local size = phone.root:GetSize()
        if size.X > 0 and size.Y > 0 and size.X <= 16384 and size.Y <= 16384 then
            local bounds = phone.info.size
            -- The dossier input icon extends 10px left of its 920x1000 root.
            -- Pad the capture only; keep the native window/layout size intact.
            local padding = phone.padding or 0
            local x, y = -padding, -padding
            local width, height = size.X + padding*2, size.Y + padding*2
            if phone.windowOrigin then
                -- The details root also uses Centered with anchorPoint=(0,0).
                -- Its right-side panel lies outside a crop starting at (0,0)
                -- unless the root's offset within the source window is included.
                local origin = phone.window:GetChildPosition(phone.root)
                if origin and math.abs(origin.X)<16384 and math.abs(origin.Y)<16384 then
                    x, y = origin.X-padding, origin.Y-padding
                else
                    x, y = phone.info.offset.X, phone.info.offset.Y
                end
            end
            if phone.scanner then
                -- The scanner's 1920x1080 root contains a 3840x2160 overlay
                -- at (-960,-540). Capture its real bounds, without moving or
                -- resizing the native widgets that place the centre reticle.
                local border = exactPath(phone.root, "border")
                local panel = exactPath(phone.root, "border", "scanner_overlay")
                local a = border and phone.root:GetChildPosition(border)
                local b = panel and border:GetChildPosition(panel)
                local s = panel and border:GetChildSize(panel)
                local origin = phone.window:GetChildPosition(phone.root)
                if a and b and s and origin and s.X>0 and s.Y>0 and s.X<16384 and s.Y<16384
                    and math.abs(origin.X+a.X+b.X)<16384 and math.abs(origin.Y+a.Y+b.Y)<16384 then
                    -- inkHudEntryInfo crops WINDOW coordinates. The scanner
                    -- root is centred with anchorPoint=(0,0), so its top-left
                    -- is (960,540), not (0,0), in a1920x1080 source window.
                    local px, py = origin.X+a.X+b.X, origin.Y+a.Y+b.Y
                    x, y = math.min(origin.X,math.floor(px-32)), math.min(origin.Y,math.floor(py-32))
                    width = math.max(origin.X+size.X,math.ceil(px+s.X+32))-x
                    height = math.max(origin.Y+size.Y,math.ceil(py+s.Y+32))-y
                elseif bounds.X>0 and bounds.Y>0 then
                    x, y = phone.info.offset.X, phone.info.offset.Y
                    width, height = bounds.X, bounds.Y
                end
            end
            if bounds.X ~= width or bounds.Y ~= height then
                phone.info.size = vector(width, height)
                phone.root:FlagForVisualInvalidation()
            end
            if (padding>0 or phone.scanner or phone.windowOrigin) and (phone.info.offset.X~=x or phone.info.offset.Y~=y) then
                phone.info.offset = vector(x, y)
                phone.root:FlagForVisualInvalidation()
            end
            local windowSize = phone.window:GetSize()
            if windowSize.X ~= size.X or windowSize.Y ~= size.Y then
                phone.window:SetSize(size.X, size.Y)
            end
            -- Fullscreen controllers cache their draw data differently from
            -- authored HUD tiles. Without this, quest-icon animation periodically
            -- leaves an empty offscreen image even though the window stays visible.
            phone.root:FlagForVisualInvalidation()
            phone.window:FlagForVisualInvalidation()
        end
        end
    end
    for _, subtitle in ipairs(subtitleRoots) do
        local active=subtitle.root:IsVisible() and subtitle.root:GetOpacity()>0
        if not active and not M.refreshHiddenBounds and subtitle.wasActive~=false then
            subtitle.root:FlagForVisualInvalidation()
            subtitle.window:FlagForVisualInvalidation()
        end
        subtitle.wasActive=active
        if M.refreshHiddenBounds or active then
        local p=subtitle.root:GetChildPosition(subtitle.panel)
        local s=subtitle.root:GetChildSize(subtitle.panel)
        if s.X>0 and s.Y>0 and s.X<16384 and s.Y<16384 and math.abs(p.X)<16384 and math.abs(p.Y)<16384 then
            -- The game's subtitle layout intentionally puts mainPanel above the
            -- 100px root (observed Y=-255). The old authored crop starts at Y=0,
            -- clipping the whole line after conversion to a separate window.
            local x=math.min(subtitle.x,math.floor(p.X-32))
            local y=math.min(subtitle.y,math.floor(p.Y-32))
            local width=math.max(subtitle.width,math.ceil(p.X+s.X+32-x))
            local height=math.max(subtitle.height,math.ceil(p.Y+s.Y+32-y))
            local offset,bounds=subtitle.info.offset,subtitle.info.size
            if offset.X~=x or offset.Y~=y or bounds.X~=width or bounds.Y~=height then
                subtitle.info.offset=vector(x,y);subtitle.info.size=vector(width,height)
            end
            subtitle.root:FlagForVisualInvalidation()
            subtitle.window:FlagForVisualInvalidation()
        end
        end
    end
    return lootVisible
end

local function tickHud()
    hudLayerName = hudLayerName or CName.new("inkHUDLayer")
    local layer = Game.VRInkLayer(hudLayerName)
    local phoneOk, phoneError = pcall(updatePhoneBounds, layer)
    M.lootVisible = phoneOk and phoneError == true
    if phoneOk then M.phoneError = nil
    elseif M.phoneError ~= tostring(phoneError) then
        M.phoneError = tostring(phoneError)
        phoneRoots = {}; subtitleRoots = {}; scanSecond = -1
        print("[Stereo.HUD phone] " .. M.phoneError)
    end
    return Game.VRHudPanelUpdate(layer and layer:GetVirtualWindow() or nil, M.lootVisible)
end

function M.tick()
    local ok, result = pcall(tickHud)
    if ok then M.count = result; M.error = nil
    elseif M.error ~= tostring(result) then
        M.error = tostring(result)
        print("[Stereo.HUD] " .. M.error)
    end
end
function M.shutdown()
    M.lootVisible=false
    pcall(function() Game.VRHudPanelUpdate(nil, false) end)
end
return M
