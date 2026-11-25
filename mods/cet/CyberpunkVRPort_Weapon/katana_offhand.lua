-- One port-owned hand attachment. The game's primary equipment slot stays
-- authoritative across wheel changes, load screens and scripted unequips.
local motion=require('ordinary_melee')
local M={diag={ticks=0}}
local owner, mainKey, side, blade, fists, lastActor
local time, retry, audit=0,0,0
local deadline, placement, handSlots
local function key(id) return tostring(ItemID.GetCombinedHash(id)) end
local function isKatana(w)
    return w and WeaponObject.IsOfType(w:GetItemID(),gamedataItemType.Wea_Katana)
end
local function area(pl, which) return EquipmentSystem.GetItemsInArea(pl,which) or {} end
local function equipped(pl,id)
    for _,candidate in ipairs(area(pl,gamedataEquipmentArea.Weapon)) do
        if key(candidate)==key(id) then return true end
    end
    return false
end
-- The engine checks the item's placementSlots even when ignoreRestrictions is set.
-- Add only our slot to the actual item record. Preserve changes made by other mods.
local function releasePlacement()
    if not placement then return end
    local current=TweakDB:GetFlat(placement.field) or {}
    for i,slot in ipairs(current) do
        if slot==placement.slot then
            table.remove(current,i)
            TweakDB:SetFlat(placement.field,current)
            break
        end
    end
    placement=nil
end
local function allowPlacement(id,hand)
    local field=TweakDBID.new(ItemID.GetTDBID(id),'.placementSlots')
    local slot=TweakDBID.new(hand==0 and 'AttachmentSlots.VRPortOffhandLeft' or 'AttachmentSlots.VRPortOffhandRight')
    if placement and placement.field==field and placement.slot==slot then return true end
    releasePlacement()
    local slots=TweakDB:GetFlat(field)
    if not slots then return false end
    for _,existing in ipairs(slots) do if existing==slot then return true end end
    slots[#slots+1]=slot
    if not TweakDB:SetFlat(field,slots) then return false end
    placement={field=field,slot=slot}
    return true
end
local function clear(pl)
    if pl and pl.VRClearOffhandItem then
        pl:VRClearOffhandItem(0)
        pl:VRClearOffhandItem(1)
    end
    if type(VRCyberFingerMeleeHand)=='function' then VRCyberFingerMeleeHand(-1,false) end
    mainKey,side,blade,fists,lastActor=nil,nil,nil,nil,nil
    deadline=nil
    retry=0
    releasePlacement()
    motion.resetOffhand()
end
function M.release(pl)
    clear(pl)
    owner=nil
end
function M.draw(pl,primary,second,hand)
    local main=pl:GetActiveWeapon()
    if not VRCyberFingerEnabled() or not isKatana(main) or key(main:GetItemID())~=key(primary)
        or key(primary)==key(second) or not equipped(pl,second) or hand~=side or not mainKey
        or VRCyberFingerMeleeHand(hand,false)<0 then return false end
    blade=second
    deadline=time+2
    retry=0
    motion.resetOffhand()
    return true
end
function M.stow(pl,id)
    if not blade or key(blade)~=key(id) then return false end
    pl:VRClearOffhandItem(side)
    blade,lastActor,deadline=nil,nil,nil
    retry=0
    releasePlacement()
    VRCyberFingerMeleeHand(-1,false)
    motion.resetOffhand()
    return true
end
function M.blade(pl)
    if not blade or not side then return nil end
    local w=pl:VROffhandItem(side)
    if w and key(w:GetItemID())==key(blade) then return w end
end
function M.update(dt,pl,main,enabled)
    time=time+(dt or 0)
    if not pl then
        if owner then M.release(nil) end
        return
    end
    local newOwner=tostring(pl:GetEntityID().hash)
    if owner~=newOwner then clear(pl);owner=newOwner end
    if not enabled or not isKatana(main) then
        -- Also cleans saved or delayed attachments once the player is available.
        if mainKey or time>=audit then clear(pl);audit=time+1 end
        return
    end
    local id=main:GetItemID()
    local newSide=Game.VRWeaponInLeftHand() and 1 or 0
    if mainKey~=key(id) or side~=newSide then
        clear(pl)
        mainKey,side=key(id),newSide
        audit=0
    end
    if time>=audit then
        if not fists then
            for _,candidate in ipairs(area(pl,gamedataEquipmentArea.BaseFists)) do
                if WeaponObject.IsFists(candidate) then fists=candidate;break end
            end
        end
        if blade and not equipped(pl,blade) then M.stow(pl,blade) end
        audit=time+.25
    end
    local desired=blade or fists
    local held=pl:VROffhandItem(side)
    local ready=held and desired and key(held:GetItemID())==key(desired)
    local grip=VRCyberFingerMeleeHand(side,ready and blade~=nil or false)
    if not ready then
        motion.resetOffhand()
        lastActor=nil
        if deadline and time>=deadline then M.stow(pl,blade);return end
        if time>=retry then
            if held then pl:VRClearOffhandItem(side)
            elseif desired and grip>=0 and allowPlacement(desired,side) then
                pl:VRMountOffhandItem(id,desired,side)
            end
            retry=time+.12
        end
        return
    end
    deadline=nil
    handSlots=handSlots or {[0]=TweakDBID.new('AttachmentSlots.WeaponLeft'),[1]=TweakDBID.new('AttachmentSlots.WeaponRight')}
    local stock=Game.GetTransactionSystem():GetItemInSlot(pl,handSlots[side])
    if stock then
        if blade then M.stow(pl,blade) else pl:VRClearOffhandItem(side) end
        motion.resetOffhand();return
    end
    if grip<0 then motion.resetOffhand();return end
    local actor=tostring(held:GetEntityID().hash)
    if lastActor~=actor then motion.resetOffhand();lastActor=actor end
    local pos,forward
    if blade then
        pos,forward=held:GetWorldPosition(),held:VRMeleeDirection()
    else
        local holster=GetMod('CyberpunkVRPort_Holster')
        if holster and holster.handBusy and holster.handBusy(side) then motion.resetOffhand();return end
        pos=VRHandRawWorld(side)
        if not pos or pos.w<=0 then motion.resetOffhand();return end
        forward={x=0,y=0,z=0}
    end
    if pos and forward then
        motion.update(dt,pl,held,pos,forward,pl:GetWorldPosition(),M.diag,side,not blade,grip==1)
    end
end
return M
