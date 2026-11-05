-- One clothing repair/refresh after the belt settles, per loaded save.
-- Keep only value keys between ticks; native systems/items belong to this tick.
local M = {}
local Job = {}
Job.__index = Job

local function key(id)
    return tostring(TDBID.ToStringDEBUG(id))
end

local function isClothing(slot, beltSlot, key)
    local name = key(slot)
    return name ~= key(beltSlot) and name ~= "OutfitSlots.HandPropLeft"
        and name ~= "OutfitSlots.HandPropRight"
end

function Job:Reset()
    self.done, self.planned = false, false
    self.beltKey, self.due = nil, nil
    self.waits, self.errors, self.restored, self.refreshCount = 0, 0, 0, 0
    self.refreshed = {}
    self.restorePlan = nil
end

function Job:Step(now, player, transactions, outfit, beltSlot, wantedRecord)
    if not player or not transactions or not outfit then return nil end
    local key = self.api.key
    local belt = transactions:GetItemInSlot(player, beltSlot)
    if not belt or key(self.api.record(belt)) ~= wantedRecord then
        self.beltKey, self.due = nil, nil
        return nil
    end
    local beltKey = self.api.entityKey(belt)
    if self.beltKey ~= beltKey then
        self.beltKey, self.due = beltKey, now + 1.0
    end
    if now < self.due then return nil end
    self.waits = self.waits + 1
    if self.waits > 120 then
        self.done = true
        return { ok = false, error = "clothing attachments did not become ready", done = true }
    end
    if outfit:IsBlocked() then return nil end

    if not self.planned then
        -- A first belt equip can activate EquipmentEX before its base garment
        -- visuals exist. Its clone then saves only the belt and hides equipment.
        -- Recover from logical equipped items, not those now-empty visual slots.
        -- An existing custom outfit is refreshed as-is, never filled/overwritten.
        if self.restorePlan == nil then
            local plan = {}
            local hasOutfitClothes = false
            for _, slot in ipairs(outfit:GetUsedSlots()) do
                if isClothing(slot, beltSlot, key) then hasOutfitClothes = true end
            end
            if outfit:IsActive() and not hasOutfitClothes then
                for _, part in ipairs(outfit:GetEquipmentParts()) do
                    local item = part:GetItemID()
                    local slot = outfit:GetItemSlot(item)
                    if outfit:IsOutfitSlot(slot) and isClothing(slot, beltSlot, key) then
                        plan[#plan + 1] = { item = item, slot = slot }
                    end
                end
            end
            self.restorePlan = plan
        end
        for _, part in ipairs(self.restorePlan) do
            if not outfit:IsOccupied(part.slot) then
                self.changed = true
                if outfit:EquipItem(part.item, part.slot) ~= true then
                    error("cannot restore equipped clothing in " .. key(part.slot))
                end
                self.restored = self.restored + 1
            end
        end
        self.planned = true
        if self.restored > 0 then
            -- EquipItem queues the attachment; wait for its real item object.
            self.due = now + 0.5
            return nil
        end
    end

    local slots = {}
    if outfit:IsActive() then
        slots = outfit:GetUsedSlots()
    else
        for _, part in ipairs(outfit:GetEquipmentParts()) do
            slots[#slots + 1] = part:GetSlotID()
        end
    end
    -- Check the whole set before touching it. A loading garment is not a
    -- reason to repeatedly refresh the other garments which are already ready.
    for _, slot in ipairs(slots) do
        if isClothing(slot, beltSlot, key) and not transactions:GetItemInSlot(player, slot) then
            return nil
        end
    end
    for _, slot in ipairs(slots) do
        local name = key(slot)
        if isClothing(slot, beltSlot, key) and not self.refreshed[name] then
            self.changed = true
            transactions:RefreshAttachment(player, slot)
            self.refreshed[name] = true
            self.refreshCount = self.refreshCount + 1
        end
    end
    self.done = true
    return { ok = true, done = true, restored = self.restored, refreshed = self.refreshCount }
end

function Job:Tick(now, player, transactions, outfit, beltSlot, wantedRecord)
    if self.done then return nil end
    self.changed = false
    local ok, result = pcall(self.Step, self, now, player, transactions, outfit, beltSlot, wantedRecord)
    if ok then
        if not result and self.changed then
            result = { ok = true, done = false, restored = self.restored, refreshed = self.refreshCount }
        end
        if result then result.changed = self.changed end
        return result
    end
    self.errors = self.errors + 1
    self.due = now + 1.0
    if self.errors >= 3 then self.done = true end
    return { ok = false, done = self.done, changed = self.changed, error = tostring(result) }
end

function M.new(api)
    local job = setmetatable({ api = api or {
        key = key,
        record = function(item) return ItemID.GetTDBID(item:GetItemID()) end,
        entityKey = function(item) return tostring(item:GetEntityID().hash) end,
    } }, Job)
    job:Reset()
    return job
end

return M
