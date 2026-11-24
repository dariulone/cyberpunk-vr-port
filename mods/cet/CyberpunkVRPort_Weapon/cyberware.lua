-- Two independent swing/contact histories. Geometry comes from the same final
-- pose that draws each blade/fist/wire; no camera-forward or vanilla lunge range.
local M = {}
local tuning=require('melee_tuning')
local ordinary=require('ordinary_melee')
local strength=require('melee_strength')
M.state={ticks=0}
local cachedId, cachedKind, hands = nil, 0, {}
local cachedOrdinary=false
local previousStamp,realClock=nil,0
local function sub(a,b) return {x=a.x-b.x,y=a.y-b.y,z=a.z-b.z} end
local function length(v) return math.sqrt(v.x*v.x+v.y*v.y+v.z*v.z) end
local function dot(a,b) return a.x*b.x+a.y*b.y+a.z*b.z end
local function angle(a,b)
    local lineA=type(a)=='table' and a.line==true
    local lineB=type(b)=='table' and b.line==true
    if lineA or lineB then
        if lineA~=lineB then return 0 end
        return math.acos(math.max(-1,math.min(1,dot(a,b))))
    end
    return 2*math.acos(math.min(1,math.abs(a.x*b.x+a.y*b.y+a.z*b.z+a.w*b.w)))
end
local function relative(points,origin)
    local out={};for i,p in ipairs(points) do out[i]=sub(p,origin) end;return out
end
local function fresh(now) return {history={},soundHistory={},armed=false,mute=now+tuning.equipMute,lastWhoosh=-10,episode=false} end

function M.kind(weapon,player)
    local id=weapon and tostring(weapon:GetEntityID().hash) or ''
    if id~=cachedId then
        if cachedKind==3 and player and player.VRCyberwareWireEffect then player:VRCyberwareWireEffect(nil,false) end
        cachedId,cachedKind,hands=id,0,{}
        previousStamp=nil;cachedOrdinary=false;ordinary.reset()
        if weapon and player and player.VRCyberwareKind then cachedKind=player:VRCyberwareKind(weapon) end
        if weapon and cachedKind==0 then cachedOrdinary=WeaponObject.IsMelee(weapon:GetItemID()) end
    end
    M.state.kind=cachedKind;M.state.weapon=cachedId
    M.state.ordinary=cachedOrdinary
    return cachedKind
end
function M.inputKind() return cachedOrdinary and 5 or cachedKind end

-- Every frame: the two slot ids are made once, not as a table and two TweakDBIDs per call.
local handSlots
function M.handEquipment(player)
    if not player then Game.VRHandEquipment(3);return end
    local transaction=Game.GetTransactionSystem()
    local mask=0
    handSlots=handSlots or {TweakDBID.new('AttachmentSlots.WeaponLeft'),TweakDBID.new('AttachmentSlots.WeaponRight')}
    for side,slot in ipairs(handSlots) do
        local item=transaction:GetItemInSlot(player,slot)
        if item then
            local id=item:GetItemID()
            local free=WeaponObject.IsOfType(id,gamedataItemType.Wea_Fists)
                or WeaponObject.IsOfType(id,gamedataItemType.Cyb_StrongArms)
                or WeaponObject.IsOfType(id,gamedataItemType.Cyb_MantisBlades)
                or WeaponObject.IsOfType(id,gamedataItemType.Cyb_NanoWires)
            if not free then mask=mask+(side==1 and 1 or 2) end
        end
    end
    Game.VRHandEquipment(mask)
end

function M.ordinary(dt,player,weapon,position,forward,origin,blocking)
    ordinary.update(dt,player,weapon,position,forward,origin,M.state,nil,nil,nil,blocking)
end

function M.update(dt,player,weapon,kind,now,supplied,ordinaryLever)
    local threshold=kind==1 and tuning.gorillaSwingSpeed or tuning.swingSpeed
    local soundThreshold=kind==1 and tuning.gorillaWhooshSpeed or tuning.whooshSpeed
    M.state.ticks=M.state.ticks+1;M.state.phase='input'
    if kind==4 then M.state.phase='launcher';return end
    if not player.VRCyberwareBeginSwing then M.state.phase='missing script helper';return end
    if not dt or dt<=0 or dt>.1 or Game.GetSystemRequestsHandler():IsGamePaused() then hands={};return end
    local data=supplied or Game.VRCyberwareFrame()
    M.state.frameSize=data and #data or -1
    if not data or #data<6 or data[1].x~=kind then hands={};return end
    local header,origin=data[1],data[2]
    local tracking=7+header.y+header.z
    if #data<tracking+3 then hands={};M.state.phase='missing tracking';return end
    if previousStamp then realClock=realClock+((origin.w-previousStamp)%16777216)*.001 end
    previousStamp=origin.w;now=realClock
    if kind==3 and player.VRCyberwareWireEffect then player:VRCyberwareWireEffect(weapon,header.y+header.z>0) end
    M.state.phase='geometry';M.state.left=header.y;M.state.right=header.z
    local offset=7
    for side=0,1 do
        local count=side==0 and header.y or header.z
        local points={}
        for i=1,count do points[i]=data[offset+i-1] end
        offset=offset+count
        local s=hands[side] or fresh(now);hands[side]=s
        if count<2 or data[tracking+side].w<=0 then hands[side]=fresh(now)
        elseif s.epoch~=header.w and (not s.time or now-s.time>=.004) then
            s.epoch=header.w
            if kind==3 and player.VRCyberwareCollideWire then
                player:VRCyberwareCollideWire(side,header.w,points,s.points or {},s.time and now-s.time or dt)
                M.state.collisions=(M.state.collisions or 0)+1
            end
            local rel=relative(points,origin)
            local control,rotation=data[tracking+side],data[tracking+2+side]
            local teleport=s.origin and length(sub(origin,s.origin))>.6
            if s.trackOrigin and s.trackOrigin~=control.w then teleport=true end
            if s.control and length(sub(control,s.control))>tuning.teleport then teleport=true end
            local instantSpeed,fastestDir=0,{x=0,y=0,z=0}
            if s.rel and #s.rel==count then
                for i=1,count do
                    local move=sub(rel[i],s.rel[i]);local distance=length(move)
                    if kind~=3 and distance>.65 then teleport=true end
                    local speed=distance/math.max(now-s.time,.001)
                    if speed>instantSpeed then instantSpeed,fastestDir=speed,move end
                end
            end
            if teleport then s=fresh(now);hands[side]=s end
            local wireMotionSpeed=instantSpeed
            local lever=ordinaryLever or (kind==2 and .65 or (kind==3 and .35 or .10))
            if s.control then
                local span=math.max(now-s.time,.001)
                local tracked=math.max(length(sub(control,s.control)),angle(rotation,s.rotation)*lever)/span
                instantSpeed=math.min(instantSpeed,tracked)
            end
            s.history[#s.history+1]={time=now,points=rel,control=control,rotation=rotation}
            local window=kind==3 and .06 or .035
            while #s.history>2 and now-s.history[1].time>window do table.remove(s.history,1) end
            local speed,velocity=0,{x=0,y=0,z=0}
            local first=s.history[1]
            if first and now-first.time>=.008 and #first.points==count then
                for i=1,count do
                    local move=sub(rel[i],first.points[i]);local value=length(move)/(now-first.time)
                    if value>speed then speed,velocity=value,move end
                end
                do
                    local tracked=math.max(length(sub(control,first.control)),angle(rotation,first.rotation)*lever)/(now-first.time)
                    speed=math.min(speed,tracked)
                end
            end
            -- Short punches can enter and leave a body inside the former
            -- 100ms average. Admit the current tracked sweep immediately;
            -- its geometry includes both ends of that same sample interval.
            if instantSpeed>speed then speed,velocity=instantSpeed,fastestDir end
            -- Sound uses the same 90ms window / 40ms minimum as ordinary
            -- melee, and only physical controller motion. A falling or
            -- oscillating wire tip must never manufacture a whoosh.
            local sound=s.soundHistory
            sound[#sound+1]={time=now,control=control,rotation=rotation}
            while sound[1] and now-sound[1].time>tuning.window do table.remove(sound,1) end
            local soundSpeed=0
            if sound[1] and now-sound[1].time>=tuning.minSpan then
                local span=now-sound[1].time
                soundSpeed=math.max(length(sub(control,sound[1].control)),angle(rotation,sound[1].rotation)*lever)/span
            end
            M.state[side==0 and 'leftSwingSpeed' or 'rightSwingSpeed']=speed
            M.state[side==0 and 'leftSoundSpeed' or 'rightSoundSpeed']=soundSpeed
            if M.state.traceMotion then
                local samples=M.state.motionSamples or {};M.state.motionSamples=samples
                samples[#samples+1]={side=side,time=now,speed=speed,instant=instantSpeed,sound=soundSpeed,
                    control={control.x,control.y,control.z},rotation={rotation.x,rotation.y,rotation.z,rotation.w},
                    tip={points[count].x,points[count].y,points[count].z}}
                if #samples>160 then table.remove(samples,1) end
            end
            if kind==1 then velocity=sub(control,s.control or control) end
            if speed<tuning.rearmSpeed then
                s.quietSince=s.quietSince or now
                if now-s.quietSince>=.08 then s.armed=true;s.episode=false;s.whoosh=false end
            elseif not s.armed and s.direction and speed>=threshold and length(velocity)>.0001 then
                s.quietSince=nil
                if dot(velocity,s.direction)<-.1*length(velocity)*length(s.direction) then s.armed=true end
            else s.quietSince=nil
            end
            if s.armed and speed>=threshold and now>=s.mute and now-(s.lastBegin or -10)>=.12 then
                s.strong=strength.strong(side)
                s.episode=player:VRCyberwareBeginSwing(weapon,side,s.strong,speed)
                s.lastBegin=now
                if kind==3 and s.episode then s.wireLiveUntil=now+.45 end
                s.armed=false;s.direction=velocity;s.whoosh=false
            end
            if s.episode and player.VRCyberwareUpdateSwing then player:VRCyberwareUpdateSwing(weapon,side,speed) end
            if s.episode and soundSpeed>=soundThreshold and not s.whoosh and now-s.lastWhoosh>=tuning.minGap then
                player:VRMeleeWhoosh(weapon,soundSpeed>=tuning.fastSpeed,s.strong)
                s.whoosh=true;s.lastWhoosh=now
                M.state.whooshes=(M.state.whooshes or 0)+1
            end
            if s.points and #s.points==count then
                if kind==3 then
                    if now-(s.lastContactCheck or -10)>=.04 then
                        local active=s.episode or now<(s.wireLiveUntil or -1)
                        player:VRCyberwareContacts(weapon,side,points,s.contactPoints or s.points,.035,not(active and wireMotionSpeed>=threshold))
                        s.lastContactCheck=now
                        s.contactPoints=points
                    end
                elseif kind==0 then
                    local direction=sub(points[2],points[1])
                    player:VRMeleeBladeHit(weapon,points[1],Vector4.new(direction.x,direction.y,direction.z,0),.22,s.strong or false,not(s.episode and instantSpeed>=threshold))
                else
                    player:VRCyberwareContacts(weapon,side,points,s.points,kind==1 and .08 or .10,not(s.episode and instantSpeed>=threshold))
                end
            end
            s.rel,s.points,s.origin,s.time=rel,points,origin,now
            s.control,s.rotation=control,rotation
            s.trackOrigin=control.w
        end
    end
    M.state.phase='complete'
end
return M
