-- Look-down and mirror views can use separate puppets. Track each worn mesh by
-- instance: garment assembly can put several components with the same name on V.
local M = {}
local owners, doubleId = {}, nil
local pollIn, searchIn = 0, 0
local holster
local beltSlots = {
    "AttachmentSlots.VRPortWaist", "OutfitSlots.Waist",
    "AttachmentSlots.VRPortBeltR", "AttachmentSlots.VRPortBelt1", "AttachmentSlots.VRPortBelt2",
    "AttachmentSlots.VRPortBelt3", "AttachmentSlots.VRPortBelt4", "AttachmentSlots.VRPortBelt5",
    "AttachmentSlots.VRPortBelt6",
}
local handSlots = {
    "AttachmentSlots.VRPortOffhandLeft", "AttachmentSlots.VRPortOffhandRight",
    "AttachmentSlots.GrenadeLeft", "AttachmentSlots.GrenadeRight",
    "AttachmentSlots.WeaponLeft", "AttachmentSlots.WeaponRight",
}

function M.matches(record, female, storyMirror)
    if storyMirror then
        return record == (female and "character.q110_tpp_female_v" or "character.q110_tpp_male_v")
    end
    return record:find("tpp", 1, true) and record:match("_v$") and
        (record:find("female", 1, true) ~= nil) == female
end

local function isGearItem(slot, item)
    if slot == "OutfitSlots.Waist" then
        local record = TDBID.ToStringDEBUG(ItemID.GetTDBID(item:GetItemID()))
        return record == "Items.vrp_belt" or record == "Items.vrp_belt_lh" or record == "Items.vrp_belt_cf_lh" or
            record == "Items.ucbelt_d" or record == "Items.ucbelt_e" or record == "Items.ucbelt_f"
    end
    if slot:find("Weapon", 1, true) or slot:find("Offhand", 1, true) then
        local kind = RPGManager.GetItemType(item:GetItemID())
        local name = kind and kind.value
        return name == "Gad_Grenade" or (type(name) == "string" and name:match("^Wea_") and name ~= "Wea_Fists")
    end
    return true -- slots reserved for the tactical belt and grenades
end

local function hide(entity, scope)
    local id = entity:GetEntityID()
    local entityKey = tostring(id.hash)
    local previous = owners[entityKey]
    local changed = {}
    for _, c in ipairs(Game.VREntityComponents(entity)) do
        local name = NameToString(c:GetName())
        local selected = scope == "all" or name:match("^ucbeltgrenade") or name == "ucbeltsoftpouch" or
            (scope == "gear" and (name:match("^vrp_belt") or name:match("^ucbelt")))
        if selected and (c:IsA("entIVisualComponent") or c:IsA("entLightComponent")) then
            local k = tostring(Game.VRComponentKey(c))
            if c:IsEnabled() then
                c:Toggle(false)
                changed[k] = true
            elseif previous and previous.changed[k] then
                changed[k] = true
            end
        elseif previous and previous.scope ~= scope and previous.changed[tostring(Game.VRComponentKey(c))] then
            if not c:IsEnabled() then c:Toggle(true) end
        end
    end
    owners[entityKey] = next(changed) and {id=id, changed=changed, scope=scope} or nil
    return entityKey
end

local function hideBelt(actor, ts, hidden, seen)
    seen[hide(actor, hidden and "gear" or "grenade")] = true
    for _, slot in ipairs(beltSlots) do
        local item = ts:GetItemInSlot(actor, TweakDBID.new(slot))
        if item and isGearItem(slot, item) then
            if hidden then
                seen[hide(item, "all")] = true
            elseif slot == "AttachmentSlots.VRPortWaist" or slot == "OutfitSlots.Waist" then
                seen[hide(item, "grenade")] = true
            else
                local record = TDBID.ToStringDEBUG(ItemID.GetTDBID(item:GetItemID())) or ""
                if record == "Items.VRPortGrenade" or record:match("^Items%.VRPortGrenade_") then
                    seen[hide(item, "all")] = true
                end
            end
        end
    end
end

local function restoreAbsent(seen)
    for key, owner in pairs(owners) do
        if not seen or not seen[key] then
            local entity = Game.FindEntityByID(owner.id)
            if entity then
                for _, c in ipairs(Game.VREntityComponents(entity)) do
                    if owner.changed[tostring(Game.VRComponentKey(c))] and not c:IsEnabled() then c:Toggle(true) end
                end
            end
            owners[key] = nil
        end
    end
end

function M.clear()
    restoreAbsent(nil)
    doubleId, searchIn, pollIn = nil, 0, 0
end

function M.update(player, dt, boundId, findDouble, radius)
    pollIn, searchIn = pollIn - dt, searchIn - dt
    if pollIn > 0 then return end
    pollIn = 0.2
    holster = holster or GetMod("CyberpunkVRPort_Holster")
    local hidden = holster and holster.wornGearHidden and holster.wornGearHidden()
    local noGrenade = holster and holster.grenadeHolsterEnabled and not holster.grenadeHolsterEnabled()
    if not hidden and not noGrenade and not doubleId and not next(owners) then return end
    local session = Game.GetSystemRequestsHandler()
    if session:IsPreGame() or session:IsGamePaused() then return end
    if not hidden and not noGrenade then
        M.clear()
        return
    end

    local seen = {}
    local ts = Game.GetTransactionSystem()
    hideBelt(player, ts, hidden, seen)
    if Game.GetQuestsSystem():GetFactStr("johnny_mirror") > 0 then
        doubleId = nil
        restoreAbsent(seen)
        return
    end

    local id = boundId or doubleId
    local double = id and Game.FindEntityByID(id)
    if not double and searchIn <= 0 then
        searchIn = 1
        -- Story mirrors may have no interactive hub. Identify V's mirror record
        -- there without binding the double's pose or changing the scripted scene.
        double = findDouble(player, radius, true)
    end
    if double and double:GetEntityID().hash ~= player:GetEntityID().hash and double:IsA("NPCPuppet") and
        M.matches((TDBID.ToStringDEBUG(double:GetRecordID()) or ""):lower(),
            Game.NameToString(player:GetResolvedGenderName()) == "Female", not boundId) then
        doubleId = double:GetEntityID()
        hideBelt(double, ts, hidden, seen)
        if hidden then
            for _, slot in ipairs(handSlots) do
                local item = ts:GetItemInSlot(double, TweakDBID.new(slot))
                if item and isGearItem(slot, item) then seen[hide(item, "all")] = true end
            end
        end
    else
        doubleId = nil
    end
    restoreAbsent(seen)
end

return M
