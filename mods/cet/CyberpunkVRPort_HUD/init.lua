local hudLive = {
    scale = 0.0,
    layout = nil,
    root = nil,
    controllerRoot = nil,
    capturedRoot = nil,
    capturedWidgetCount = 0,
    originalMetrics = {},
    messengerController = nil,
    messengerLastX = nil,
    messengerLastY = nil,
    messengerLastScale = nil,
    pollTimer = 0.0,
    pollInterval = 0.10,
    iniPath = 'vrport.ini',
    scalePath = nil,
    rootResolvedLogged = false,
    logPath = nil
}

local kLayoutPathCandidates = {
    '.\\hud_layout.ini',
    'plugins\\cyber_engine_tweaks\\mods\\CyberpunkVRPort_HUD\\hud_layout.ini'
}

local kLogPathCandidates = {
    '.\\hud_live_debug.log',
    'plugins\\cyber_engine_tweaks\\mods\\CyberpunkVRPort_HUD\\hud_live_debug.log'
}

local function cloneDefaultLayout()
    return {
        minimapQuest = 0.0,
        minimapQuestY = 0.0,
        minimapQuestScale = 1.00,
        phone = 0.0,
        phoneY = 0.0,
        phoneScale = 1.00,
        holocall = 600.0,
        holocallY = 600.0,
        holocallScale = 2.00,
        incomingCall = 600.0,
        incomingCallY = 1050.0,
        incomingCallScale = 2.00,
        phoneMessage = 600.0,
        phoneMessageY = 800.0,
        phoneMessageScale = 1.80,
        messageReader = 600.0,
        messageReaderY = 500.0,
        messageReaderScale = 1.40,
        messengerMenu = 1150.0,
        messengerMenuY = 500.0,
        messengerMenuScale = 1.50,
        topLeftAlerts = 0.0,
        topLeftAlertsY = 0.0,
        topLeftAlertsScale = 1.00,
        topRight = 0.0,
        topRightY = 0.0,
        topRightScale = 1.00,
        bottomLeft = 0.0,
        bottomLeftY = 0.0,
        bottomLeftScale = 1.00,
        bottomLeftTop = 0.0,
        bottomLeftTopY = 0.0,
        bottomLeftTopScale = 1.00,
        radio = 0.0,
        radioY = 0.0,
        radioScale = 1.00,
        bottomRight = 0.0,
        bottomRightY = 0.0,
        bottomRightScale = 1.00,
        rightCenter = 0.0,
        rightCenterY = 0.0,
        rightCenterScale = 1.00,
        johnnyHint = 0.0,
        activityLog = 0.0,
        warning = 0.0,
        bossHealth = 0.0,
        vehicleScan = 0.0,
        progressBar = 0.0,
        oxygenBar = 0.0
    }
end

local function clampOffset(value)
    if type(value) ~= 'number' then
        return 0.0
    end
    if value < -2400.0 then
        return -2400.0
    end
    if value > 2400.0 then
        return 2400.0
    end
    return value
end

local function clampRegionScale(value)
    if type(value) ~= 'number' then
        return 1.00
    end
    if value < 0.01 then
        return 0.01
    end
    if value > 2.00 then
        return 2.00
    end
    return value
end

local function appliedRegionScale(value)
    return clampRegionScale(value) * 0.5
end

local function logLine(text)
    if not hudLive.logPath then
        return
    end
    local file = io.open(hudLive.logPath, 'a')
    if not file then
        return
    end
    file:write(text .. '\n')
    file:close()
end

local function pickWritablePath(candidates)
    for _, path in ipairs(candidates) do
        local file = io.open(path, 'a')
        if file then
            file:close()
            return path
        end
    end
    return candidates[1]
end

local function getScriptDir()
    local source = ''
    if debug and debug.getinfo then
        local info = debug.getinfo(1, 'S')
        if info and info.source then
            source = info.source
        end
    end
    if source:sub(1, 1) == '@' then
        source = source:sub(2)
    end
    return source:match('^(.*)[/\\][^/\\]+$') or '.'
end

local function readLayoutText(path)
    local file = io.open(path, 'r')
    if not file then
        return nil
    end

    local text = file:read('*a')
    file:close()
    if not text then
        return nil
    end

    local layout = cloneDefaultLayout()
    local keyMap = {
        xr_hud_scale = 'minimapQuest',
        xr_hud_scale_y = 'minimapQuestY',
        xr_hud_scale_scale = 'minimapQuestScale',
        xr_hud_phone = 'phone',
        xr_hud_phone_y = 'phoneY',
        xr_hud_phone_scale = 'phoneScale',
        xr_hud_holocall = 'holocall',
        xr_hud_holocall_y = 'holocallY',
        xr_hud_holocall_scale = 'holocallScale',
        xr_hud_incoming_call = 'incomingCall',
        xr_hud_incoming_call_y = 'incomingCallY',
        xr_hud_incoming_call_scale = 'incomingCallScale',
        xr_hud_phone_message = 'phoneMessage',
        xr_hud_phone_message_y = 'phoneMessageY',
        xr_hud_phone_message_scale = 'phoneMessageScale',
        xr_hud_message_reader = 'messageReader',
        xr_hud_message_reader_y = 'messageReaderY',
        xr_hud_message_reader_scale = 'messageReaderScale',
        xr_hud_messenger = 'messengerMenu',
        xr_hud_messenger_y = 'messengerMenuY',
        xr_hud_messenger_scale = 'messengerMenuScale',
        xr_hud_top_left_alerts = 'topLeftAlerts',
        xr_hud_top_left_alerts_y = 'topLeftAlertsY',
        xr_hud_top_left_alerts_scale = 'topLeftAlertsScale',
        xr_hud_top_right = 'topRight',
        xr_hud_top_right_y = 'topRightY',
        xr_hud_top_right_scale = 'topRightScale',
        xr_hud_bottom_left = 'bottomLeft',
        xr_hud_bottom_left_y = 'bottomLeftY',
        xr_hud_bottom_left_scale = 'bottomLeftScale',
        xr_hud_bottom_left_top = 'bottomLeftTop',
        xr_hud_bottom_left_top_y = 'bottomLeftTopY',
        xr_hud_bottom_left_top_scale = 'bottomLeftTopScale',
        xr_hud_radio = 'radio',
        xr_hud_radio_y = 'radioY',
        xr_hud_radio_scale = 'radioScale',
        xr_hud_bottom_right = 'bottomRight',
        xr_hud_bottom_right_y = 'bottomRightY',
        xr_hud_bottom_right_scale = 'bottomRightScale',
        xr_hud_right_center = 'rightCenter',
        xr_hud_right_center_y = 'rightCenterY',
        xr_hud_right_center_scale = 'rightCenterScale',
        xr_hud_johnny_hint = 'johnnyHint',
        xr_hud_activity_log = 'activityLog',
        xr_hud_warning = 'warning',
        xr_hud_boss_health = 'bossHealth',
        xr_hud_vehicle_scan = 'vehicleScan',
        xr_hud_progress_bar = 'progressBar',
        xr_hud_oxygen_bar = 'oxygenBar'
    }

    for key, field in pairs(keyMap) do
        local value = text:match(key .. '%s*=%s*([%+%-]?[%d%.]+)')
        if value then
            if field:match('Scale$') then
                layout[field] = clampRegionScale(tonumber(value))
            else
                layout[field] = clampOffset(tonumber(value))
            end
        end
    end

    return layout
end

local function readLayoutFromCandidates()
    for _, path in ipairs(kLayoutPathCandidates) do
        local layout = readLayoutText(path)
        if layout ~= nil then
            return layout, path
        end
    end
    return nil, nil
end

local function getWidgetName(widget)
    local ok, name = pcall(function()
        return widget:GetName()
    end)
    if not ok or name == nil then
        return nil
    end

    local okText, text = pcall(function()
        return Game.NameToString(name)
    end)
    if okText and text ~= nil then
        return text
    end

    return tostring(name)
end

local function getParentWidget(widget)
    local ok, parent = pcall(function()
        return widget:GetParentWidget()
    end)
    if ok and parent ~= nil then
        return parent
    end

    ok, parent = pcall(function()
        return widget.parentWidget
    end)
    if ok then
        return parent
    end

    return nil
end

local function getWidgetMargin(widget)
    local ok, margin = pcall(function()
        return widget:GetMargin()
    end)
    if not ok or margin == nil then
        return nil
    end

    return {
        left = margin.left or margin.Left or 0.0,
        top = margin.top or margin.Top or 0.0,
        right = margin.right or margin.Right or 0.0,
        bottom = margin.bottom or margin.Bottom or 0.0
    }
end

local function cloneMargin(margin)
    if margin == nil then
        return nil
    end

    return {
        left = margin.left or 0.0,
        top = margin.top or 0.0,
        right = margin.right or 0.0,
        bottom = margin.bottom or 0.0
    }
end

local function getWidgetScale(widget)
    local ok, scale = pcall(function()
        return widget:GetScale()
    end)
    if not ok or scale == nil then
        return { x = 1.0, y = 1.0 }
    end

    local x = scale.X or scale.x or 1.0
    local y = scale.Y or scale.y or 1.0
    return { x = x, y = y }
end

local function getWidgetAnchor(widget)
    local ok, anchor = pcall(function()
        return widget:GetAnchor()
    end)
    if not ok then
        return nil
    end
    return anchor
end

local function captureRegionWidgets(root)
    local okCount, count = pcall(function()
        return root:GetNumChildren()
    end)
    if not okCount or count == nil then
        logLine('[CyberpunkVRPort_HUD] failed to enumerate root children')
        return {}
    end

    local widgets = {}
    local widgetKeyCounts = {}
    for i = 0, count - 1 do
        local okWidget, widget = pcall(function()
            return root:GetWidget(i)
        end)
        if okWidget and widget ~= nil then
            local margin = getWidgetMargin(widget)
            local scale = getWidgetScale(widget)
            local anchor = getWidgetAnchor(widget)
            local name = getWidgetName(widget) or ('child_' .. tostring(i))
            local widgetBaseKey = string.format('%s|%s', tostring(name), tostring(anchor))
            local ordinal = (widgetKeyCounts[widgetBaseKey] or 0) + 1
            widgetKeyCounts[widgetBaseKey] = ordinal
            local widgetKey = string.format('%s|%d', widgetBaseKey, ordinal)
            local original = hudLive.originalMetrics[widgetKey]
            if original == nil then
                original = {
                    margin = cloneMargin(margin),
                    scaleX = scale.x,
                    scaleY = scale.y
                }
                hudLive.originalMetrics[widgetKey] = original
            end
            table.insert(widgets, {
                widget = widget,
                key = widgetKey,
                name = name,
                anchor = anchor,
                margin = cloneMargin(original.margin),
                scaleX = original.scaleX,
                scaleY = original.scaleY
            })
            logLine(string.format('[CyberpunkVRPort_HUD] child[%d] name=%s anchor=%s margin=(%.1f, %.1f, %.1f, %.1f)',
                i,
                tostring(name),
                tostring(anchor),
                margin and margin.left or 0.0,
                margin and margin.top or 0.0,
                margin and margin.right or 0.0,
                margin and margin.bottom or 0.0))
        end
    end

    hudLive.capturedWidgetCount = count
    return widgets
end

local function getRootChildCount(root)
    local okCount, count = pcall(function()
        return root:GetNumChildren()
    end)
    if not okCount or count == nil then
        return nil
    end
    return count
end

local function isTopLeftAlertsItem(item)
    return item.name == 'zone alert notification' or
        item.name == 'militech warning' or
        item.name == 'hud_courier_bar' or
        item.name == 'driver_combat_hud' or
        item.name == 'staminabar' or
        (item.name == 'HUDMiddleWidget' and item.anchor == inkEAnchor.TopLeft)
end

local function getLayoutValueForItem(item)
    local layout = hudLive.layout or cloneDefaultLayout()

    if item.name == 'TopRightMain' then
        return layout.minimapQuest, layout.minimapQuestY, layout.minimapQuestScale
    elseif item.name == 'TopLeftMain' or item.name == 'songbird_phone' then
        return layout.phone, layout.phoneY, layout.phoneScale
    elseif item.name == 'VRPortHolocall' then
        return layout.holocall, layout.holocallY, layout.holocallScale
    elseif item.name == 'VRPortIncomingCall' then
        return layout.incomingCall, layout.incomingCallY, layout.incomingCallScale
    elseif item.name == 'VRPortPhoneMessage' then
        return layout.phoneMessage, layout.phoneMessageY, layout.phoneMessageScale
    elseif item.name == 'VRPortMessageReader' then
        return layout.messageReader, layout.messageReaderY, layout.messageReaderScale
    elseif item.name == 'zone alert notification' or item.name == 'militech warning' or item.name == 'hud_courier_bar' or item.name == 'driver_combat_hud' or item.name == 'staminabar' then
        return layout.topLeftAlerts, layout.topLeftAlertsY, layout.topLeftAlertsScale
    elseif item.name == 'HUDMiddleWidget' and item.anchor == inkEAnchor.TopLeft then
        return layout.topLeftAlerts, layout.topLeftAlertsY, layout.topLeftAlertsScale
    elseif item.name == 'InputHintJohnny' then
        return layout.topRight, layout.topRightY, layout.topRightScale
    elseif item.name == 'BottomLeft' or item.name == 'BottomLeftCar' or item.name == 'BottomLeftSpeedometer' then
        return layout.bottomLeft, layout.bottomLeftY, layout.bottomLeftScale
    elseif item.name == 'BottomLeftTop' then
        return layout.bottomLeftTop, layout.bottomLeftTopY, layout.bottomLeftTopScale
    elseif item.name == 'RadioCustom' then
        return layout.radio, layout.radioY, layout.radioScale
    elseif item.name == 'BottomRightMain' then
        return layout.bottomRight, layout.bottomRightY, layout.bottomRightScale
    elseif item.name == 'RightCenter' then
        return layout.rightCenter, layout.rightCenterY, layout.rightCenterScale
    elseif item.name == 'LeftCenter' or item.name == 'activity_log' then
        return layout.activityLog, 0.0, 1.0
    elseif item.name == 'warning' then
        return 0.0, layout.warning, 1.0
    elseif item.name == 'boss_healthbar' then
        return 0.0, layout.bossHealth, 1.0
    elseif item.name == 'vehicle scan widget' then
        return 0.0, layout.vehicleScan, 1.0
    elseif item.name == 'hud_progress_bar' then
        return 0.0, layout.progressBar, 1.0
    elseif item.name == 'oxygenbar' then
        return 0.0, layout.oxygenBar, 1.0
    elseif item.name == 'TopLeftMain' then
        return layout.topLeftAlerts, layout.topLeftAlertsY, layout.topLeftAlertsScale
    end

    return nil
end

local function applyInsetToCapturedWidgets(root, force)
    if root == nil then
        return
    end

    local childCount = getRootChildCount(root)
    if hudLive.capturedRoot ~= root or hudLive.capturedWidgetCount ~= childCount then
        hudLive.capturedRoot = root
        hudLive.capturedWidgets = captureRegionWidgets(root)
    end

    pcall(function()
        root:SetAnchorPoint(Vector2.new({ X = 0.5, Y = 0.5 }))
        root:SetScale(Vector2.new({ X = 1.0, Y = 1.0 }))
    end)

    for _, item in ipairs(hudLive.capturedWidgets or {}) do
        if item.widget ~= nil and item.margin ~= nil and item.anchor ~= nil then
            local left = item.margin.left
            local top = item.margin.top
            local right = item.margin.right
            local bottom = item.margin.bottom
            local adjusted = false
            local valueX, valueY, scale = getLayoutValueForItem(item)
            if valueX == nil and valueY == nil then
                goto continue
            end
            if valueX == nil then valueX = 0.0 end
            if valueY == nil then valueY = 0.0 end
            if scale == nil then scale = 1.0 end
            -- Skip widgets whose layout value didn't change, so editing one slider
            -- moves only its element instead of re-applying the whole HUD each poll.
            -- force=true (root re-resolve / first capture) always applies everything.
            if not force and item.lastX ~= nil
                and math.abs(item.lastX - valueX) < 0.0005
                and math.abs(item.lastY - valueY) < 0.0005
                and math.abs((item.lastScale or 1.0) - scale) < 0.0005 then
                goto continue
            end
            item.lastX = valueX
            item.lastY = valueY
            item.lastScale = scale
            local appliedScale = appliedRegionScale(scale)

            if item.name == 'TopLeftMain' then
                left = left + valueX
                top = top + valueY
                adjusted = true
            elseif item.name == 'songbird_phone' then
                left = left + valueX
                top = top + valueY
                adjusted = true
            elseif item.name == 'VRPortHolocall' or
                item.name == 'VRPortIncomingCall' or
                item.name == 'VRPortPhoneMessage' or
                item.name == 'VRPortMessageReader' then
                -- These are VR-port-owned top-level slots, so their values are
                -- absolute layout coordinates rather than offsets from a stock
                -- game margin. Scale remains consistent with the F10 convention:
                -- 2.00 = full original size.
                left = valueX
                top = valueY
                adjusted = true
            elseif isTopLeftAlertsItem(item) then
                left = left + valueX
                top = top + valueY
                adjusted = true
            elseif item.name == 'TopRightMain' then
                right = right - valueX
                top = top + valueY
                adjusted = true
            elseif item.name == 'BottomLeft' or item.name == 'BottomLeftCar' or item.name == 'BottomLeftSpeedometer' then
                left = left + valueX
                bottom = bottom - valueY
                adjusted = true
            elseif item.name == 'BottomLeftTop' then
                left = left + valueX
                bottom = bottom - valueY
                adjusted = true
            elseif item.name == 'RadioCustom' then
                left = left + valueX
                bottom = bottom - valueY
                adjusted = true
            elseif item.name == 'BottomRightMain' then
                right = right - valueX
                bottom = bottom - valueY
                adjusted = true
            elseif item.name == 'RightCenter' then
                right = right - valueX
                top = top + valueY
                adjusted = true
            elseif item.name == 'InputHintJohnny' then
                right = right - valueX
                top = top + valueY
                adjusted = true
            elseif item.name == 'LeftCenter' or item.name == 'activity_log' then
                left = left + valueX
                top = top + valueY
                adjusted = true
            elseif item.name == 'warning' or item.name == 'boss_healthbar' then
                left = left + valueX
                top = top + valueY
                adjusted = true
            elseif item.name == 'hud_progress_bar' or item.name == 'oxygenbar' then
                left = left + valueX
                bottom = bottom - valueY
                adjusted = true
            elseif item.name == 'vehicle scan widget' then
                left = left + valueX
                top = top + valueY
                adjusted = true
            end

            if adjusted then
                pcall(function()
                    local scaleX = item.scaleX * appliedScale
                    local scaleY = item.scaleY * appliedScale
                    if item.name == 'VRPortHolocall' or
                        item.name == 'VRPortIncomingCall' or
                        item.name == 'VRPortPhoneMessage' or
                        item.name == 'VRPortMessageReader' then
                        scaleX = appliedScale
                        scaleY = appliedScale
                    end
                    item.widget:SetScale(Vector2.new({ X = scaleX, Y = scaleY }))
                    item.widget:SetMargin(inkMargin.new({ left = left, top = top, right = right, bottom = bottom }))
                end)
            end
        end
        ::continue::
    end
end

local function applyMessengerLayout(force)
    local controller = hudLive.messengerController
    if controller == nil then
        return
    end

    local layout = hudLive.layout or cloneDefaultLayout()
    local x = layout.messengerMenu
    local y = layout.messengerMenuY
    local scale = layout.messengerMenuScale
    if not force and hudLive.messengerLastX ~= nil
        and math.abs(hudLive.messengerLastX - x) < 0.0005
        and math.abs(hudLive.messengerLastY - y) < 0.0005
        and math.abs(hudLive.messengerLastScale - scale) < 0.0005 then
        return
    end

    local ok = pcall(function()
        controller:VrApplyMessengerLayout(x, y, scale)
    end)
    if ok then
        hudLive.messengerLastX = x
        hudLive.messengerLastY = y
        hudLive.messengerLastScale = scale
        logLine(string.format('[CyberpunkVRPort_HUD] applied Messenger layout (%.1f, %.1f, %.2f)', x, y, scale))
    else
        hudLive.messengerController = nil
        hudLive.messengerLastX = nil
        hudLive.messengerLastY = nil
        hudLive.messengerLastScale = nil
        logLine('[CyberpunkVRPort_HUD] Messenger layout bridge failed; clearing cached controller')
    end
end

local function tryResolveHudRootFromWidget(startWidget)
    local widget = startWidget
    local depth = 0
    while widget ~= nil and depth < 32 do
        local name = getWidgetName(widget)
        if name == 'Root' then
            if not hudLive.rootResolvedLogged then
                print('[CyberpunkVRPort_HUD] resolved HUD root via widget parent chain')
                logLine('[CyberpunkVRPort_HUD] resolved HUD root via widget parent chain')
                hudLive.rootResolvedLogged = true
            end
            return widget
        end
        widget = getParentWidget(widget)
        depth = depth + 1
    end
    return nil
end

local function tryResolveHudRootFromController(controller)
    if controller == nil then
        return nil
    end

    local ok, startWidget = pcall(function()
        return controller:GetRootCompoundWidget()
    end)
    if not ok or startWidget == nil then
        return nil
    end

    return tryResolveHudRootFromWidget(startWidget)
end

local function readHudLayout()
    local layout, path = readLayoutFromCandidates()
    if layout ~= nil then
        hudLive.scalePath = path
        return layout
    end

    return cloneDefaultLayout()
end

local function tryGetLayer(inkSystem)
    local ok, layer = pcall(function()
        return inkSystem:GetLayer(StringToName('inkHUDLayer'))
    end)
    if ok and layer ~= nil then
        return layer
    end

    ok, layer = pcall(function()
        return inkSystem:GetLayer('inkHUDLayer')
    end)
    if ok then
        return layer
    end

    return nil
end

local function tryResolveHudRoot()
    local ok, inkSystem = pcall(Game.GetInkSystem)
    if not ok or inkSystem == nil then
        return nil
    end

    local layer = tryGetLayer(inkSystem)
    if layer == nil then
        return nil
    end

    local okController, controller = pcall(function()
        return layer:GetGameController()
    end)
    if okController and controller ~= nil then
        local okRoot, root = pcall(function()
            return controller:GetRootCompoundWidget()
        end)
        if okRoot and root ~= nil then
            if not hudLive.rootResolvedLogged then
                print('[CyberpunkVRPort_HUD] resolved HUD root via layer game controller')
                logLine('[CyberpunkVRPort_HUD] resolved HUD root via layer game controller')
                hudLive.rootResolvedLogged = true
            end
            return root
        end
    end

    local window = layer:GetVirtualWindow()
    if window == nil then
        return nil
    end

    local okRoot, root = pcall(function()
        return window:GetWidget(StringToName('Root'))
    end)
    if (not okRoot or root == nil) then
        okRoot, root = pcall(function()
            return window:GetWidgetByPathName('Root')
        end)
    end
    if okRoot and root ~= nil then
        if not hudLive.rootResolvedLogged then
            print('[CyberpunkVRPort_HUD] resolved HUD root via virtual window')
            logLine('[CyberpunkVRPort_HUD] resolved HUD root via virtual window')
            hudLive.rootResolvedLogged = true
        end
        return root
    end

    if hudLive.controllerRoot ~= nil then
        if not hudLive.rootResolvedLogged then
            logLine('[CyberpunkVRPort_HUD] falling back to controller-derived root')
        end
        return hudLive.controllerRoot
    end

    return nil
end

local function applyScale(force)
    local hadRoot = hudLive.root ~= nil
    if hudLive.root == nil then
        hudLive.root = tryResolveHudRoot()
    end
    if hudLive.root == nil then
        return
    end

    if not hadRoot then
        force = true
    end

    local ok = pcall(function()
        applyInsetToCapturedWidgets(hudLive.root, force)
    end)
    if ok then
        hudLive.lastAppliedScale = hudLive.scale
        logLine('[CyberpunkVRPort_HUD] applied HUD layout update')
    else
        hudLive.root = nil
        hudLive.lastAppliedScale = nil
        hudLive.rootResolvedLogged = false
        logLine('[CyberpunkVRPort_HUD] apply failed; clearing cached root')
    end
end

local function refreshScale(force)
    local nextLayout = readHudLayout()
    local changed = false
    local current = hudLive.layout or cloneDefaultLayout()

    for key, value in pairs(nextLayout) do
        if math.abs((current[key] or 1.0) - value) >= 0.0005 then
            changed = true
            break
        end
    end

    if changed then
        logLine(string.format('[CyberpunkVRPort_HUD] layout changed minimapQuest=(%.1f, %.1f, %.2f) phone=(%.1f, %.1f, %.2f) holocall=(%.1f, %.1f, %.2f) incomingCall=(%.1f, %.1f, %.2f) phoneMessage=(%.1f, %.1f, %.2f) messageReader=(%.1f, %.1f, %.2f) messenger=(%.1f, %.1f, %.2f) topRight=(%.1f, %.1f, %.2f) bottomLeft=(%.1f, %.1f, %.2f) bottomRight=(%.1f, %.1f, %.2f)',
            nextLayout.minimapQuest,
            nextLayout.minimapQuestY,
            nextLayout.minimapQuestScale,
            nextLayout.phone,
            nextLayout.phoneY,
            nextLayout.phoneScale,
            nextLayout.holocall,
            nextLayout.holocallY,
            nextLayout.holocallScale,
            nextLayout.incomingCall,
            nextLayout.incomingCallY,
            nextLayout.incomingCallScale,
            nextLayout.phoneMessage,
            nextLayout.phoneMessageY,
            nextLayout.phoneMessageScale,
            nextLayout.messageReader,
            nextLayout.messageReaderY,
            nextLayout.messageReaderScale,
            nextLayout.messengerMenu,
            nextLayout.messengerMenuY,
            nextLayout.messengerMenuScale,
            nextLayout.topRight,
            nextLayout.topRightY,
            nextLayout.topRightScale,
            nextLayout.bottomLeft,
            nextLayout.bottomLeftY,
            nextLayout.bottomLeftScale,
            nextLayout.bottomRight,
            nextLayout.bottomRightY,
            nextLayout.bottomRightScale))
    end

    hudLive.layout = nextLayout
    hudLive.scale = nextLayout.minimapQuest
    if force or changed then
        -- Pass the real force flag: a live slider edit (changed, force=false) only
        -- re-applies the widget whose value actually changed; a root re-resolve
        -- (force=true) re-applies the whole HUD.
        applyScale(force)
        applyMessengerLayout(force)
    end
end

registerForEvent('onInit', function()
    hudLive.scalePath = nil
    hudLive.logPath = pickWritablePath(kLogPathCandidates)
    local reset = io.open(hudLive.logPath, 'w')
    if reset then
        reset:write('')
        reset:close()
    end
    local initialLayout, initialScalePath = readLayoutFromCandidates()
    hudLive.scalePath = initialScalePath
    hudLive.layout = initialLayout or cloneDefaultLayout()
    hudLive.scale = hudLive.layout.minimapQuest
    print('[CyberpunkVRPort_HUD] init, scale path: ' .. tostring(hudLive.scalePath) .. ' log path: ' .. tostring(hudLive.logPath))
    logLine('[CyberpunkVRPort_HUD] init, scale path: ' .. tostring(hudLive.scalePath) .. ' log path: ' .. tostring(hudLive.logPath))

    ObserveAfter('QuestTrackerGameController', 'OnInitialize', function(this)
        hudLive.controllerRoot = tryResolveHudRootFromController(this)
        hudLive.root = nil
        hudLive.capturedRoot = nil
        hudLive.capturedWidgetCount = 0
        hudLive.rootResolvedLogged = false
        applyScale(true)
    end)

    ObserveAfter('PhoneDialerLogicController', 'OnInitialize', function(this)
        hudLive.messengerController = this
        hudLive.messengerLastX = nil
        hudLive.messengerLastY = nil
        hudLive.messengerLastScale = nil
        applyMessengerLayout(true)
    end)

    ObserveAfter('MinimapContainerController', 'OnInitialize', function(this)
        hudLive.controllerRoot = tryResolveHudRootFromController(this)
        hudLive.root = nil
        hudLive.capturedRoot = nil
        hudLive.capturedWidgetCount = 0
        hudLive.rootResolvedLogged = false
        applyScale(true)
    end)

    ObserveAfter('QuestTrackerGameController', 'OnUninitialize', function()
        hudLive.root = nil
        hudLive.controllerRoot = nil
        hudLive.capturedRoot = nil
        hudLive.capturedWidgetCount = 0
        hudLive.lastAppliedScale = nil
        hudLive.rootResolvedLogged = false
    end)
end)

registerForEvent('onUpdate', function(delta)
    local ok, err = pcall(function()
        hudLive.pollTimer = hudLive.pollTimer + delta
        if hudLive.pollTimer < hudLive.pollInterval then
            return
        end
        hudLive.pollTimer = 0.0
        refreshScale(false)
    end)
    if not ok then
        logLine('[CyberpunkVRPort_HUD] onUpdate error: ' .. tostring(err))
    end
end)
