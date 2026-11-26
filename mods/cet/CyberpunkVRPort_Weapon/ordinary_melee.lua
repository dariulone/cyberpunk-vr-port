-- Measure the solved item's swept points and use tracking to confirm player motion.
local tuning=require('melee_tuning')
local M={}
local state
local clock=0
local function sub(a,b) return {x=a.x-b.x,y=a.y-b.y,z=a.z-b.z} end
local function length(a) return math.sqrt(a.x*a.x+a.y*a.y+a.z*a.z) end
local function dot(a,b) return a.x*b.x+a.y*b.y+a.z*b.z end
function M.reset() state=nil end
local function controllerTravel(a,b,reach)
    local travel=length(sub(a.control,b.control))
    if a.line and b.line then
        local turn=math.acos(math.max(-1,math.min(1,dot(a.line,b.line))))
        travel=math.max(travel,turn*math.max(.10,reach*.5))
    end
    if not a.rotation or not b.rotation then return travel end
    local u,r=a.rotation,b.rotation
    local cosine=math.abs(dot(u,r)+u.w*r.w)
    -- Chord length at the end of the item. A stationary grip can still turn
    -- the rendered prop. The independent world-space check rejects pure roll.
    return math.max(travel,2*reach*math.sqrt(math.max(0,1-math.min(1,cosine)^2)))
end
local function solvedMotion(a,b)
    local grip=sub(a.relativeGrip,b.relativeGrip)
    local tip=sub(a.relativeTip,b.relativeTip)
    return dot(tip,tip)>dot(grip,grip) and tip or grip
end
local function contactMotion(a,b,axis,edge,reach)
    if controllerTravel(a,b,reach)<.022 then return false end
    local d=solvedMotion(a,b)
    local magnitude=length(d)
    if magnitude<.020 then return false end
    local along=dot(d,axis)
    local across=edge and dot(d,edge)-along*dot(edge,axis) or 0
    return math.max(along,math.abs(across))>=.65*magnitude
end
local function trackedWorldMotion(a,b)
    local d=sub(a.control,b.control)
    local basis=VRBladeTrackingBasis and VRBladeTrackingBasis()
    if not basis or basis.w<.5 then return Vector4.new(0,0,0,0) end
    return Vector4.new(basis.x*d.x+basis.y*d.z,basis.y*d.x-basis.x*d.z,d.y,0)
end
-- The trigger of the hand holding the weapon: the slots are per physical controller, and the weapon is in the left
-- one while VRWeaponInLeftHand says so (input[3] below is that hand already).
local function weaponTrigger()
    return Game.GetVRSharedSlot(Game.VRWeaponInLeftHand() and 154 or 160)>.5
end

local function sweep(s,player,weapon,sample,forward,reach,active,allowShort,motion,dt,diag)
    local old=s.geometry
    s.geometry=sample
    local travel=length(solvedMotion(sample,old))
    local short=allowShort and weapon:VRBladeKind()>0 and contactMotion(sample,s.history[1],forward,weapon:GetWorldForward(),reach)
    local probe=travel>.001 and (active or short)
    if probe then diag.strokeQueries=(diag.strokeQueries or 0)+1 end
    local hit=player:VRMeleeSweepHit(weapon,sample.position,sample.tip,old.position,old.tip,.08,not probe,
        motion,not (active and s.beganOnHit),probe and ((active and s.strong) or (not active and weaponTrigger())) or false,
        active and s.peak or travel/dt)
    if hit and hit>0 then
        diag.strokeDispatches=(diag.strokeDispatches or 0)+1
        if active then s.beganOnHit=true end
    end
end

function M.update(dt,player,weapon,position,forward,origin,diag)
    diag.bladeTracking=nil
    diag.ticks=diag.ticks+1;diag.kind=0;diag.left=0;diag.right=2;diag.frameSize=12
    local input=Game.VRMeleeInputFrame()
    if not input or #input~=5 or input[3].w<=0 or not dt or dt<=0 or dt>.25
        or Game.GetSystemRequestsHandler():IsGamePaused() then
        state=nil;diag.phase='no physical melee tracking';return
    end
    local reach=math.min(1.25,weapon:VRMeleeReach())
    local tip=Vector4.new(position.x+forward.x*reach,position.y+forward.y*reach,position.z+forward.z*reach,1)
    local sample={control=input[3],rotation=input[5],position=position,tip=tip,origin=origin,
        relativeGrip=sub(position,origin),relativeTip=sub(tip,origin),stamp=input[1].x,epoch=input[1].y}
    if input[1].z>.5 and input[2].w>0 then
        local line=sub(input[2],input[3]);local n=length(line)
        if n>.03 then sample.line={x=line.x/n,y=line.y/n,z=line.z/n} end
    end
    diag.bladeTracking=input[3] -- same XR sample, before any virtual resistance
    local previous=state and state.sample
    local repeated=previous and previous.epoch==sample.epoch
    local span=previous and ((sample.stamp-previous.stamp)%16777216)*.001 or 0
    if repeated then
        span=0
        state.repeatAge=(state.repeatAge or 0)+dt
        if length(sub(origin,state.geometry.origin))>.6 then state=nil;return end
        if state.repeatAge>.06 then
            -- Still poll passive contacts, but never replay a stale input interval.
            sweep(state,player,weapon,sample,forward,reach,false,false,Vector4.new(0,0,0,0),dt,diag)
            return
        end
    elseif previous and span<.004 then
        return
    end
    clock=clock+math.min(span,.25);sample.time=clock
    if previous and (span>.1 or previous.control.w~=sample.control.w
        or length(sub(sample.control,previous.control))>math.max(tuning.teleport,span*15)
        or length(sub(origin,previous.origin))>.6) then state=nil;previous=nil end
    if not state then
        state={sample=sample,geometry=sample,history={sample},mute=clock+tuning.equipMute,armed=false,soundArmed=false,
            lastBegin=-10,lastSound=-10};diag.phase='settling';return
    end
    local s=state
    if (previous.line~=nil)~=(sample.line~=nil) then
        -- Changing grip mode is not motion. Keep the current stroke/mute state;
        -- a support-hand squeeze must not mute the next second of combat.
        s.history={};previous=sample;s.geometry=sample
    end
    local history=s.history
    if repeated and #history>0 then history[#history]=sample else history[#history+1]=sample end
    while #history>1 and clock-history[1].time>.18 do table.remove(history,1) end
    local speed=0
    local velocity={x=0,y=0,z=0}
    for _,old in ipairs(history) do
        local seconds=clock-old.time
        if seconds>=.004 and (seconds<=.05 or old==history[#history-1]) then
            local delta=solvedMotion(sample,old)
            local tracked=controllerTravel(sample,old,reach)
            local travel=math.min(length(delta),tracked)
            local value=travel/(seconds+(repeated and s.repeatAge or 0))
            -- Reject a one-sample tracking twitch by physical excursion,
            -- without averaging a short real stroke over 90ms of standing still.
            if travel>=.05 and value>speed then
                speed=value;velocity=delta
            end
        end
    end
    diag.rightSwingSpeed=speed;diag.rightSoundSpeed=speed
    if speed<tuning.rearmSpeed then
        s.quiet=s.quiet or clock
        if clock-s.quiet>=.06 then s.armed=true;s.soundArmed=true end
    else
        s.quiet=nil
        if s.direction and dot(velocity,s.direction)<-.2*length(velocity)*length(s.direction)
            and clock-s.lastBegin>=.12 then
            s.armed=true;s.soundArmed=true
        end
    end
    if clock>=s.mute and speed>=tuning.swingSpeed and s.armed and clock-s.lastBegin>=.12 then
        s.strong=weaponTrigger()
        s.episode=true;s.beganOnHit=false;s.peak=speed
        diag.fullSwingStarts=(diag.fullSwingStarts or 0)+1
        s.armed=false;s.lastBegin=clock;s.direction=velocity
    end
    if s.episode and speed>=tuning.swingSpeed then
        s.liveUntil=clock+.06
        s.peak=math.max(s.peak,speed)
    end
    -- Air sound belongs to the physical stroke, not to a collision or the
    -- success of a queued game attack. One whoosh per stroke/reversal.
    if clock>=s.mute and speed>=tuning.whooshSpeed and s.soundArmed and clock-s.lastSound>=.12 then
        player:VRMeleeWhoosh(weapon,speed>=tuning.fastSpeed,weaponTrigger())
        s.soundArmed=false;s.lastSound=clock
        diag.whooshes=(diag.whooshes or 0)+1
    end
    local active=s.episode and clock<=(s.liveUntil or -1)
    if not active then s.episode=false end
    -- History establishes intent. Geometry always covers just the latest solved
    -- interval, including the first fast sample and the frame the solver catches up.
    if not repeated then s.repeatAge=0 end
    local short=not repeated and clock>=s.mute and controllerTravel(sample,previous,reach)>.0015
    sweep(s,player,weapon,sample,forward,reach,active,short,trackedWorldMotion(sample,previous),repeated and dt or span,diag)
    if diag.traceMotion then
        local samples=diag.motionSamples or {};diag.motionSamples=samples
        samples[#samples+1]={side=1,time=clock,speed=speed,sound=speed,active=active or false,
            control={sample.control.x,sample.control.y,sample.control.z},
            position={position.x,position.y,position.z},tip={tip.x,tip.y,tip.z}}
        if #samples>160 then table.remove(samples,1) end
    end
    s.sample=sample;diag.phase='complete'
end
return M
