-- B draws the installed arm cyberware from empty hands and holsters it on the next press.
-- The native heartbeat captures the physical B in gameplay, leaving menus and popups to the game.
local previousDown = true
local cooldown = 0
local launcherPending = 0

local function heartbeat(active)
    if type(VRArmCyberwareBHeartbeat) == 'function' then VRArmCyberwareBHeartbeat(active and 1 or 0) end
end

local function update(dt)
    cooldown = math.max(0, cooldown - (dt or 0))
    local player = Game.GetPlayer()
    if not player or type(GetVRSharedSlot) ~= 'function' then
        heartbeat(false); previousDown = true; return
    end
    if launcherPending > 0 then
        launcherPending = math.max(0, launcherPending - (dt or 0))
        if launcherPending == 0 then player.vrArmBLauncherPending = false end
    end
    local equipment = Game.GetScriptableSystemsContainer():Get(CName.new('EquipmentSystem'))
    local data = equipment and equipment:GetPlayerData(player)
    if not data then heartbeat(false); previousDown = true; return end
    local item = data:GetActiveItem(gamedataEquipmentArea.ArmsCW)
    if not item or not ItemID.IsValid(item) then heartbeat(false); previousDown = true; return end
    local launcher = WeaponObject.IsOfType(item, gamedataItemType.Cyb_Launcher)
    local melee = WeaponObject.IsOfType(item, gamedataItemType.Cyb_MantisBlades)
        or WeaponObject.IsOfType(item, gamedataItemType.Cyb_NanoWires)
        or WeaponObject.IsOfType(item, gamedataItemType.Cyb_StrongArms)
    if not launcher and not melee then heartbeat(false); previousDown = true; return end
    heartbeat(true)
    if (GetVRSharedSlot(31) or 0) ~= 0 or (GetVRSharedSlot(164) or 0) ~= 0 then
        previousDown = true; return
    end
    local down = (GetVRSharedSlot(157) or 0) > 0.5
    local pressed = down and not previousDown
    previousDown = down
    if not pressed or cooldown > 0 then return end
    local active = player:GetActiveWeapon()
    if active then
        local activeId = active:GetItemID()
        if melee and tostring(activeId) == tostring(item) then
            player:VRHolsterMelee(); cooldown = 0.75; return
        end
        if not WeaponObject.IsOfType(activeId, gamedataItemType.Wea_Fists)
            and not (launcher and tostring(activeId) == tostring(item)) then return end
    end
    local request = NewObject('EquipmentSystemWeaponManipulationRequest')
    request.owner = player
    if launcher then
        local left = Game.GetTransactionSystem():GetItemInSlot(player, TweakDBID.new('AttachmentSlots.WeaponLeft'))
        if left and tostring(left:GetItemID()) == tostring(item) then
            player.vrLauncherKeepOpen = false
            player.vrLauncherSelectedAsWeapon = false
            player.vrLauncherFromWeaponCycle = true
            request.requestType = EquipmentManipulationAction.UnequipLeftHandCyberware
        else
            if not player:VRArmBLauncherAllowed() then return end
            player.vrArmBLauncherPending = true
            launcherPending = 1.0
            request.requestType = EquipmentManipulationAction.RequestLeftHandCyberware
        end
    else
        request.requestType = EquipmentManipulationAction.RequestActiveMeleeware
    end
    cooldown = 0.75
    equipment:QueueRequest(request)
end

registerForEvent('onInit', function() previousDown = true; cooldown = 0; heartbeat(false) end)
registerForEvent('onUpdate', function(dt)
    local ok, err = pcall(update, dt)
    if not ok then heartbeat(false); previousDown = true; spdlog.error('[Arm Cyberware B] ' .. tostring(err)) end
end)
registerForEvent('onShutdown', function() heartbeat(false) end)
