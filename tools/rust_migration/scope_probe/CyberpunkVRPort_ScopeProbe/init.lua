-- Optics and outlines in game for one run (tools/rust_migration/probe_session.ps1 -ScopeTest): installed into the
-- port's Lua mods for that run and taken out after it. When the probe script asks for it (go.txt beside this file), a
-- schedule runs: Panam's sniper rifle (Overwatch: a camera scope of its own) is given and drawn; then each glass scope
-- the scope images name as the "shader" path is installed in turn into an assault rifle (the VR menu's way:
-- InventoryDataManagerV2:InstallPart) and the rifle drawn again; last, the targets around the player get the engine's
-- forced vision appearance (the focus outline the port's belt uses). So the engine creates the scopes' and the
-- outline's pipelines and the port its optics and its outline (stage 8's pipelines in game). The run is closed without
-- saving (the saves it makes are moved aside by the probe script); each step prints one "[scope-probe]" line.
local CAMERA_SCOPED = 'Items.Preset_Grad_Panam'
local RIFLE = 'Items.Preset_Ajax_Default'
local GLASS_SCOPES = { 'Items.w_att_scope_long_01', 'Items.w_att_scope_long_02', 'Items.w_att_scope_long_03',
                       'Items.w_att_scope_long_05', 'Items.w_att_scope_short_02', 'Items.w_att_scope_short_06' }

local pass = 0
local function say(text) print(string.format('[scope-probe] %d %s', pass, text)) end

-- The player's item of a record: its ItemID, or nil.
local function find(player, ts, record)
    local wanted = TweakDBID.new(record)
    local ok, items = ts:GetItemList(player)
    if not ok or not items then return nil end
    for _, data in ipairs(items) do
        local id = data:GetID()
        if ItemID.GetTDBID(id) == wanted then return id end
    end
    return nil
end

local function give(record)
    local player, ts = Game.GetPlayer(), Game.GetTransactionSystem()
    local id = find(player, ts, record)
    if not id then
        ts:GiveItem(player, ItemID.FromTDBID(TweakDBID.new(record)), 1)
        id = find(player, ts, record)
    end
    return id
end

local function draw(id, what)
    local r = DrawItemRequest.new()
    r.owner = Game.GetPlayer()
    r.itemID = id
    r.equipAnimationType = gameEquipAnimationType.Instant
    Game.GetScriptableSystemsContainer():Get(CName.new('EquipmentSystem')):QueueRequest(r)
    say(what .. ' drawn')
end

local function camera_scope()
    local id = give(CAMERA_SCOPED)
    say(CAMERA_SCOPED .. ' found ' .. tostring(id ~= nil))
    if id then draw(id, CAMERA_SCOPED) end
end

local function glass_scope(record)
    local player = Game.GetPlayer()
    local rifle, scope = give(RIFLE), give(record)
    if not (rifle and scope) then
        say(string.format('%s: rifle %s, scope %s', record, tostring(rifle ~= nil), tostring(scope ~= nil)))
        return
    end
    local ok, err = pcall(function()
        local m = InventoryDataManagerV2.new()
        m:Initialize(player)
        m:InstallPart(rifle, scope, TweakDBID.new('AttachmentSlots.Scope'))
    end)
    say(string.format('%s installed: %s %s', record, tostring(ok), tostring(err)))
end

-- The weapon in hand put away (the VRIK mod's request): a weapon already out keeps the next one from being drawn.
local function holster()
    local ok = pcall(function()
        local q = EquipmentSystemWeaponManipulationRequest.new()
        q.owner = Game.GetPlayer()
        q.requestType = EquipmentManipulationAction.UnequipWeapon
        Game.GetScriptableSystemsContainer():Get(CName.new('EquipmentSystem')):QueueRequest(q)
    end)
    say('holstered: ' .. tostring(ok))
end

-- The rifle equipped into the first weapon slot and drawn (a DrawItemRequest alone leaves an unequipped rifle in the
-- backpack: the optics never scanned it); the VR menu's GameplayEquipRequest, with the weapon forced out.
local function equip_rifle()
    local player = Game.GetPlayer()
    local rifle = give(RIFLE)
    if not rifle then return end
    local equipped = pcall(function()
        local q = GameplayEquipRequest.new()
        q.owner = player
        q.itemID = rifle
        q.slotIndex = 0
        q.forceEquipWeapon = true
        Game.GetScriptableSystemsContainer():Get(CName.new('EquipmentSystem')):QueueRequest(q)
    end)
    say('equipped: ' .. tostring(equipped))
    draw(rifle, RIFLE)
end

-- The targets around the player (the targeting system's search), each given the forced vision appearance.
local function outline_targets()
    local player = Game.GetPlayer()
    local targeting = Game.GetTargetingSystem()
    local seen, lit = 0, 0
    local function light(obj)
        if obj == nil then return end
        local ok = pcall(function()
            local d = FocusForcedHighlightData.new()
            d.isSavable = false
            d.hudData = HighlightInstance.new()
            d.hudData.instant = true
            d.hudData.context = 1
            d.highlightType = 0
            d.outlineType = 4
            d.inTransitionTime = 0.0
            d.outTransitionTime = 0.0
            d.priority = EPriority.Absolute
            d.sourceName = CName.new('cvr_scope_probe')
            d.isRevealed = true
            obj:ForceVisionAppearance(d)
        end)
        if ok then lit = lit + 1 end
    end
    pcall(function() light(targeting:GetLookAtObject(player, false, false)) end)
    pcall(function()
        local q = TargetSearchQuery.new()
        q.maxDistance = 60.0
        q.testedSet = TargetingSet.Complete
        local ok, parts = targeting:GetTargetParts(player, q)
        if ok and parts then
            for _, part in ipairs(parts) do
                seen = seen + 1
                pcall(function() light(part:GetComponent():GetEntity()) end)
            end
        end
    end)
    say(string.format('outlines: %d target part(s), %d object(s) lit', seen, lit))
end

-- The schedule: seconds after the ask, and the step. Each glass scope: the weapon put away and the scope installed,
-- 1.5 s later the rifle equipped and drawn.
local steps = { { 0.0, camera_scope } }
for i, record in ipairs(GLASS_SCOPES) do
    local at = 6.0 + 4.0 * (i - 1)
    steps[#steps + 1] = { at, function() holster(); glass_scope(record) end }
    steps[#steps + 1] = { at + 1.5, equip_rifle }
end
steps[#steps + 1] = { 6.0 + 4.0 * #GLASS_SCOPES, outline_targets }
steps[#steps + 1] = { 10.0 + 4.0 * #GLASS_SCOPES, function() print(string.format('[scope-probe] pass %d done', pass)) end }

local poll, clock, started, next_step = 0.0, 0.0, nil, 1
registerForEvent('onUpdate', function(delta)
    clock = clock + delta
    if started then
        while next_step <= #steps and clock - started >= steps[next_step][1] do
            local ok, err = pcall(steps[next_step][2])
            if not ok then say('error ' .. tostring(err)) end
            next_step = next_step + 1
        end
        return
    end
    poll = poll + delta
    if poll < 0.25 then return end
    poll = 0.0
    local f = io.open('go.txt', 'r')
    if not f then return end
    local wanted = tonumber(f:read('*l') or '') or 0
    f:close()
    if wanted <= pass or not Game.GetPlayer() then return end
    pass, started = wanted, clock
end)
