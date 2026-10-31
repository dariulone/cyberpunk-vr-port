-- Ordinary melee speed is the grip's physical movement, as before Enhanced.
-- Wrist roll must not acquire a sword-length lever and become a phantom swing.
local tuning=require('melee_tuning')
local M={}
local state
local clock=0
local function sub(a,b) return {x=a.x-b.x,y=a.y-b.y,z=a.z-b.z} end
local function length(a) return math.sqrt(a.x*a.x+a.y*a.y+a.z*a.z) end
local function dot(a,b) return a.x*b.x+a.y*b.y+a.z*b.z end
local function distance(a,b,lever)
    local d=length(sub(a.control,b.control))
    if a.line and b.line then
        d=math.max(d,math.acos(math.max(-1,math.min(1,dot(a.line,b.line))))*lever)
    end
    return d
end
function M.reset() state=nil end

function M.update(dt,player,weapon,position,forward,origin,diag)
    diag.ticks=diag.ticks+1;diag.kind=0;diag.left=0;diag.right=2;diag.frameSize=12
    local input=Game.VRMeleeInputFrame()
    if not input or #input~=5 or input[3].w<=0 or not dt or dt<=0 or dt>.25
        or Game.GetSystemRequestsHandler():IsGamePaused() then
        state=nil;diag.phase='no physical melee tracking';return
    end
    local reach=math.min(1.25,weapon:VRMeleeReach())
    local tip=Vector4.new(position.x+forward.x*reach,position.y+forward.y*reach,position.z+forward.z*reach,1)
    local sample={control=input[3],position=position,tip=tip,origin=origin,stamp=input[1].x,epoch=input[1].y}
    if input[1].z>.5 and input[2].w>0 then
        local line=sub(input[2],input[3]);local n=length(line)
        if n>.03 then sample.line={x=line.x/n,y=line.y/n,z=line.z/n} end
    end
    local previous=state and state.sample
    if previous and previous.epoch==sample.epoch then return end
    local span=previous and ((sample.stamp-previous.stamp)%16777216)*.001 or 0
    if previous and span<.004 then return end
    clock=clock+math.min(span,.25);sample.time=clock
    if previous and (span>.1 or previous.control.w~=sample.control.w
        or length(sub(sample.control,previous.control))>math.max(tuning.teleport,span*15)
        or length(sub(origin,previous.origin))>.6) then state=nil;previous=nil end
    if not state then
        state={sample=sample,history={sample},mute=clock+tuning.equipMute,armed=false,soundArmed=false,
            lastBegin=-10,lastSound=-10};diag.phase='settling';return
    end
    local s=state
    if (previous.line~=nil)~=(sample.line~=nil) then
        -- Changing grip mode is not motion. Keep the current stroke/mute state;
        -- a support-hand squeeze must not mute the next second of combat.
        s.history={};previous=sample
    end
    local lever=math.max(.10,reach*.5)
    local history=s.history
    history[#history+1]=sample
    while #history>1 and clock-history[1].time>.08 do table.remove(history,1) end
    local speed,soundSpeed=0,0
    local velocity={x=0,y=0,z=0}
    for _,old in ipairs(history) do
        local seconds=clock-old.time
        if seconds>=.012 and (seconds<=.05 or old==previous) then
            local travel=distance(sample,old,lever)
            local value=travel/seconds
            -- Reject a one-sample tracking twitch by physical excursion,
            -- without averaging a short real stroke over 90ms of standing still.
            if travel>=.05 and value>speed then
                speed=value;velocity=sub(sample.control,old.control)
                if sample.line and length(velocity)<.005 then velocity=sub(tip,old.tip) end
            end
            if travel>=.05 then soundSpeed=math.max(soundSpeed,value) end
        end
    end
    diag.rightSwingSpeed=speed;diag.rightSoundSpeed=soundSpeed
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
        s.strong=Game.GetVRSharedSlot(160)>.5
        s.episode=player:VRCyberwareBeginSwing(weapon,1,s.strong,speed)
        s.armed=false;s.lastBegin=clock;s.direction=velocity
    end
    if s.episode and speed>=tuning.swingSpeed then
        s.liveUntil=clock+.06
        player:VRCyberwareUpdateSwing(weapon,1,speed)
    end
    -- Air sound belongs to the physical stroke, not to a collision or the
    -- success of a queued game attack. One whoosh per stroke/reversal.
    if clock>=s.mute and soundSpeed>=tuning.whooshSpeed and s.soundArmed and clock-s.lastSound>=.12 then
        player:VRMeleeWhoosh(weapon,soundSpeed>=tuning.fastSpeed,Game.GetVRSharedSlot(160)>.5)
        -- Recovered swing envelope on 0.1.8's physical tracking path.
        -- Native routes to OpenXR, or exclusively to the active Toolkit bridge.
        pcall(function()
            local amplitude=.45+(.85-.45)*math.min(1,math.max(0,(soundSpeed-tuning.whooshSpeed)/3))
            SetVRHapticPulse(1,amplitude,45)
        end)
        s.soundArmed=false;s.lastSound=clock
        diag.whooshes=(diag.whooshes or 0)+1
    end
    local active=s.episode and clock<=(s.liveUntil or -1)
    if active and speed>=tuning.swingSpeed and clock-(s.lastHaptic or -10)>=.12 then
        s.lastHaptic=clock
        -- This is swing feedback, not confirmed collision telemetry.
        pcall(function() SetVRHapticPulse(1,1.0,90) end)
    end
    if not active then s.episode=false end
    player:VRMeleeSweepHit(weapon,position,tip,previous.position,previous.tip,.22,not active)
    if diag.traceMotion then
        local samples=diag.motionSamples or {};diag.motionSamples=samples
        samples[#samples+1]={side=1,time=clock,speed=speed,sound=soundSpeed,active=active or false,
            control={sample.control.x,sample.control.y,sample.control.z},
            position={position.x,position.y,position.z},tip={tip.x,tip.y,tip.z}}
        if #samples>160 then table.remove(samples,1) end
    end
    s.sample=sample;diag.phase='complete'
end
return M
