-- Loaded by the port's Lua host. Reads take place only when the runner calls snapshot().
local function readGun(p, w, side, factor, wid, failure)
    if not w then return false end
    local id = w:GetItemID()
    local data = assert(Game.GetTransactionSystem():GetItemData(p, id), 'weapon item unavailable')
    local slot = TweakDBID.new('AttachmentSlots.PowerModule')
    local muzzle, other = false, {}
    for _, used in ipairs(data:GetUsedSlotsOnItem()) do
        local part = data:GetItemPart(used)
        local item = InnerItemData.GetItemID(part)
        if ItemID.IsValid(item) then
            local rec = InnerItemData.GetStaticData(part)
            if rec and not rec:TagsContains('DummyPart') then
                if used == slot then
                    muzzle = {record=TDBID.ToStringDEBUG(ItemID.GetTDBID(item)), instance=tostring(ItemID.GetCombinedHash(item)),
                        quality=InnerItemData.GetStatValueByType(part, gamedataStatType.Quality)}
                else
                    other[#other+1] = tostring(used)..'='..tostring(ItemID.GetCombinedHash(item))
                end
            end
        end
    end
    table.sort(other)
    local hasSlot = data:HasPartInSlot(slot)
    if not hasSlot then
        for _, empty in ipairs(data:GetEmptySlotsOnItem()) do
            if empty == slot then hasSlot = true; break end
        end
    end
    local peak = GetVRLastShotPeak(side)
    return {record=TDBID.ToStringDEBUG(ItemID.GetTDBID(id)), instance=tostring(ItemID.GetCombinedHash(id)),
        entity=tostring(w:GetEntityID().hash), publishedEntity=wid, factor=factor, failure=failure,
        ammo=w:GetMagazineAmmoCount(), muzzle=muzzle, other=table.concat(other,';'),
        slot=hasSlot, seq=peak.y + 65536 * peak.z, peak=peak.x}
end

local function snapshot()
    local p = assert(Game.GetPlayer(), 'no player')
    local r, l = GameObject.GetRightActiveWeapon(p), GameObject.GetLeftActiveWeapon(p)
    local status = GetMod('CyberpunkVRPort_Weapon').AttachmentRecoilStatus()
    return {primary=readGun(p, r or l, 1, status.primary, status.primaryWid, status.primaryError),
        partner=readGun(p, r and l or nil, 0, status.partner, status.partnerWid, status.partnerError),
        leftOnly=not r and l~=nil, supported=GetVRTwoHandActive(), locked=Game.IsSavingLocked(),
        time=Game.VRMeleeRealTime()}
end

return {snapshot=snapshot}
