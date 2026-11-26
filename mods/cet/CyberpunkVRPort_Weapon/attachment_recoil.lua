-- The hand spring models the peak kick. RecoilKickMin, ADS variants and the
-- tooltip-only RecoilKickReduction can all describe the same installed brake.
local M = {}
local rulesByRecord = {}
local hands = { [0] = {}, [1] = {} }

local function rulesFor(record)
    local key = tostring(record:GetID())
    if rulesByRecord[key] then return rulesByRecord[key] end
    local rules, seen = {}, {}
    local function collect(source)
        for _, mod in ipairs(source:StatModifiers()) do
            if mod:StatType():StatType() == gamedataStatType.RecoilKickMax then
                local id = tostring(mod:GetID())
                if not seen[id] then
                    seen[id] = true
                    local constant = mod:IsA('gamedataConstantStatModifier_Record')
                    local quality = not constant and mod:IsA('gamedataCombinedStatModifier_Record')
                        and mod:RefObject().value == 'Self' and mod:OpSymbol().value == '*'
                        and mod:RefStat():StatType() == gamedataStatType.Quality
                    if constant or quality then
                        rules[#rules + 1] = { op = mod:ModifierType().value, value = mod:Value(), quality = quality }
                    end
                end
            end
        end
    end
    collect(record)
    for _, group in ipairs(record:StatModifierGroups()) do
        -- Randomized/limited groups describe candidates. Only unconditional
        -- rows are known to be active from their records alone.
        if group:StatModsLimit() < 0 and not group:StatModsLimitModifier() then collect(group) end
    end
    rulesByRecord[key] = rules
    return rules
end

local function totals()
    return { add = 0, relative = 0, mul = 1 }
end

local function apply(total, rules, quality)
    for _, rule in ipairs(rules) do
        local value = rule.value
        if rule.quality then value = value * quality end
        if rule.op == 'AdditiveMultiplier' then total.relative = total.relative + value
        elseif rule.op == 'Multiplier' then total.mul = total.mul * value
        elseif rule.op == 'Additive' then total.add = total.add + value end
    end
end

local function readScale(player, gun)
    local weaponID = gun:GetItemID()
    if not WeaponObject.IsRanged(weaponID) then return 1 end
    local data = Game.GetTransactionSystem():GetItemData(player, weaponID)
    if not data then return 1, 'weapon item data unavailable' end
    local total, seen = totals(), {}
    for _, slot in ipairs(data:GetUsedSlotsOnItem()) do
        local key = tostring(slot)
        if not seen[key] then
            seen[key] = true
            local part = data:GetItemPart(slot)
            local id = InnerItemData.GetItemID(part)
            if ItemID.IsValid(id) and (InventoryUtils.IsPart(slot) or RPGManager.IsWeaponMod(id)) then
                local record = InnerItemData.GetStaticData(part)
                if record and not record:TagsContains('DummyPart') then
                    local rules = rulesFor(record)
                    if #rules > 0 then
                        apply(total, rules, InnerItemData.GetStatValueByType(part, gamedataStatType.Quality))
                    end
                end
            end
        end
    end
    local scale = total.mul
    if total.add ~= 0 or total.relative ~= 0 then
        -- The game's additive and additive-multiplier buckets each have an
        -- unmodified baseline. A plain multiplier cancels out of this ratio.
        local base = totals()
        local record = TweakDBInterface.GetItemRecord(ItemID.GetTDBID(weaponID))
        apply(base, rulesFor(record), data:GetStatValueByType(gamedataStatType.Quality))
        if base.add > 0 then scale = scale * math.max(0, 1 + total.add / base.add) end
        local relativeBase = 1 + base.relative
        if relativeBase > 0 then scale = scale * (1 + total.relative / relativeBase) end
    end
    return math.max(0, math.min(1, scale))
end

function M.invalidate()
    hands[0].due, hands[1].due = 0, 0
end

function M.update(side, player, gun, wid, dt)
    if type(SetVRAttachmentRecoil) ~= 'function' then return end
    local hand = hands[side]
    if hand.wid == wid and hand.scale ~= nil then
        if not gun then return end
        hand.due = hand.due - dt
        if hand.due > 0 then return end
    end
    -- Equip events invalidate on the next update. Reconcile changes made
    -- directly through TransactionSystem and retry an unavailable item.
    local scale, failure = 1, nil
    if gun then
        local ok, result, reason = pcall(readScale, player, gun)
        if ok then scale, failure = result, reason else failure = tostring(result) end
    end
    if scale ~= hand.scale then SetVRAttachmentRecoil(side, scale) end
    hand.wid, hand.scale, hand.error, hand.due = wid, scale, failure, 0.5
end

function M.reset()
    M.update(0, nil, nil, nil, 0)
    M.update(1, nil, nil, nil, 0)
end

function M.status()
    return { primary = hands[1].scale, partner = hands[0].scale,
        primaryWid = hands[1].wid, partnerWid = hands[0].wid,
        primaryError = hands[1].error, partnerError = hands[0].error }
end

return M
