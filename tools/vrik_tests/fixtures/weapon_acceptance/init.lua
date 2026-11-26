-- Test-only CET module. The driver installs this in a fresh directory for one run.
-- Sample the real magazine once per game update, not via a slow external polling loop.
local take, active
local hands, recordingHands, observerReady
local handSlots = {}
local analog = {left=154, right=160}

local function slotGun(p, side)
    local w = Game.GetTransactionSystem():GetItemInSlot(p, handSlots[side])
    if w and WeaponObject.IsRanged(w:GetItemID()) then return w end
end

local function startHands()
    local p = Game.GetPlayer()
    if not observerReady or not p or recordingHands then return false end
    local now = EngineTime.ToDouble(Game.GetEngineTime())
    hands = {begin=now,prevTime=now,player=tostring(p:GetEntityID().hash),frames=0,maxGap=0,weapons={}}
    for side, channel in pairs(analog) do
        local w = slotGun(p, side)
        if w then
            local ammo = w:GetMagazineAmmoCount()
            hands.weapons[side] = {entity=tostring(w:GetEntityID().hash),
                item=TDBID.ToStringDEBUG(ItemID.GetTDBID(w:GetItemID())),
                cycle=Game.GetStatsSystem():GetStatValue(w:GetEntityID(),gamedataStatType.CycleTime),
                original=ammo,remaining=ammo,down=GetVRSharedSlot(channel)>=.85,
                receipts={},ammoChanges={},edges={}}
        end
    end
    recordingHands = next(hands.weapons) ~= nil
    return recordingHands
end

local function invalidateHands(reason)
    hands.invalid = reason
    recordingHands = false
end

local function sampleHands()
    local p = Game.GetPlayer()
    if not p or tostring(p:GetEntityID().hash) ~= hands.player then return invalidateHands('player changed') end
    local now = EngineTime.ToDouble(Game.GetEngineTime())
    local previous = hands.prevTime
    if now < previous or now-hands.begin > 20 or hands.frames >= 3000 then
        return invalidateHands('clock or sample limit')
    end
    for side, channel in pairs(analog) do
        local w, row = slotGun(p, side), hands.weapons[side]
        if (w and tostring(w:GetEntityID().hash)) ~= (row and row.entity) then
            return invalidateHands(side..' weapon changed')
        end
        if row then
            local remaining = w:GetMagazineAmmoCount()
            if remaining ~= row.remaining then
                table.insert(row.ammoChanges,{lo=previous,hi=now,delta=row.remaining-remaining})
                row.remaining = remaining
            end
            local raw = GetVRSharedSlot(channel)
            local down = raw >= .85 or (row.down and raw > .200001)
            if down ~= row.down then
                table.insert(row.edges,{lo=previous,hi=now,down=down})
                row.down = down
            end
        end
    end
    hands.prevTime, hands.frames = now, hands.frames+1
    hands.maxGap = math.max(hands.maxGap,now-previous)
end

registerForEvent('onInit',function()
    handSlots.left = TweakDBID.new('AttachmentSlots.WeaponLeft')
    handSlots.right = TweakDBID.new('AttachmentSlots.WeaponRight')
    ObserveBefore('ShootEvents','OnEnter',function(_,_,si)
        if not recordingHands then return end
        local p, w = si.executionOwner, si.owner
        if not p or not w or tostring(p:GetEntityID().hash) ~= hands.player then return end
        local t = EngineTime.ToDouble(Game.GetEngineTime())
        local id = tostring(w:GetEntityID().hash)
        for side, row in pairs(hands.weapons) do
            if row.entity == id then
                table.insert(row.receipts,{t=t,travel=GetVRSharedSlot(analog[side]),
                    token=tostring(Game.VRCurrentFingerAction()),count=si:GetActionPressCount(CName.new('RangedAttack'))})
                return
            end
        end
        invalidateHands('unmatched player shot')
    end)
    observerReady = true
end)

local function stopHands()
    if recordingHands then sampleHands() end
    recordingHands = false
    return hands
end

local function sample()
    local p=Game.GetPlayer(); local w=p and p:GetActiveWeapon()
    if not w then return end
    local id=tostring(w:GetEntityID().hash)
    if id~=take.entity then take.invalid='weapon changed';active=false;return end
    local t=Game.VRMeleeRealTime();local n=w:GetMagazineAmmoCount()
    if t<take.prevTime then take.invalid='clock reversed';active=false;return end
    local gap=t-take.prevTime
    take.maxGap=math.max(take.maxGap,gap);take.frames=take.frames+1
    if take.input then
        local input=take.input
        if Game.VRWeaponTriggerEnabled()~=input.vr then take.invalid='trigger route changed' end
        local value=GetVRSharedSlot(input.slot)
        local down=value>=.85 or (input.down and value>.200001)
        if down~=input.down then
            input.edges[#input.edges+1]={down=down,lo=take.prevTime,hi=t}
            input.down=down
        end
    end
    if take.psm then
        local current=p:GetPlayerStateMachineBlackboard():GetInt(GetAllBlackboardDefs().PlayerStateMachine.RightWeapon)
        if current~=take.lastState then
            take.psm[#take.psm+1]={lo=take.prevTime,hi=t,state=current};take.lastState=current
        end
    end
    if n~=take.remaining then
        local change=take.remaining-n
        take.events[#take.events+1]={lo=take.prevTime,hi=t,delta=change,left=n}
        if change~=1 then take.invalid='non-unit magazine change';active=false end
    end
    take.prevTime=t;take.remaining=n
    if take.frames>3000 or t-take.begin>20 then active=false;take.invalid='sample limit' end
end
registerForEvent('onUpdate',function()
    if recordingHands then sampleHands() end
    if active then sample() end
end)
return {
    startHands=startHands, stopHands=stopHands,
    start=function(traceState,traceTrigger)
        local p=Game.GetPlayer();local w=p and p:GetActiveWeapon();if not w then return false end
        local t=Game.VRMeleeRealTime();local n=w:GetMagazineAmmoCount()
        take={begin=t,prevTime=t,original=n,remaining=n,entity=tostring(w:GetEntityID().hash),
              item=TDBID.ToStringDEBUG(ItemID.GetTDBID(w:GetItemID())),events={},maxGap=0,frames=0,
              ratio=w.vrTapCycleScale,cycle=Game.GetStatsSystem():GetStatValue(w:GetEntityID(),gamedataStatType.CycleTime)}
        if traceState then take.psm={} end
        if traceTrigger then
            if not startHands() then return false end
            local slot=VRLeftHanded() and 154 or 160
            local mode=w:GetCurrentTriggerMode():Type()
            take.input={slot=slot,down=GetVRSharedSlot(slot)>=.85,edges={},vr=Game.VRWeaponTriggerEnabled(),
                        semi=mode==gamedataTriggerMode.SemiAuto,auto=mode==gamedataTriggerMode.FullAuto}
        end
        active=true;return true
    end,
    stop=function()
        active=false
        if take and take.input then take.hands=stopHands() end
        return take
    end
}
