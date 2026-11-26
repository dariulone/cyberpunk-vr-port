-- LT still enters native MeleeTargeting. A fresh right-grip press/release
-- authorizes exactly one throw. Samples use the XR clock, not CET frame time.
local M={state={phase='idle',releases=0}}
local current
-- Casual aim. A hand's velocity at release points well below where the throw was meant
-- (an overhand stroke is already coming down when the grip opens), so the pitch is mostly
-- the head's, and the heading leans towards the gaze when the stroke is roughly towards it.
-- A stroke far off the gaze (sideways, behind) keeps its own heading. The gaze is the one the
-- head had the last time the hand was still (below `still` m/s) before the measured stroke began:
-- right before the throw, before the head follows the arm.
local AIM={pitchHand=.2,yawHand=.35,yawWindow=math.rad(30),yawFade=math.rad(60),still=1.0}
-- Launch speed from hand speed: fast enough that the knife flies close to a straight line
-- (the flat game's knife leaves at 110 m/s).
local LAUNCH={base=10,perHand=5,min=10,max=45}
local function length(v) return math.sqrt(v.x*v.x+v.y*v.y+v.z*v.z) end
local function sub(a,b) return {x=a.x-b.x,y=a.y-b.y,z=a.z-b.z} end
local function scaled(a,k) return {x=a.x*k,y=a.y*k,z=a.z*k} end
local function age(a,b) return ((a-b)%16777216)*.001 end
local function world(v,b) return Vector4.new(b.x*v.x+b.y*v.z,b.y*v.x-b.x*v.z,v.y,0) end
-- Tracking space: Y up, -Z forward. Heading 0 looks along -Z, positive turns to +X.
local atan2=math.atan2 or math.atan -- LuaJIT has atan2; Lua 5.3+ (the tests) takes atan(y,x)
local function heading(v) return atan2(v.x,-v.z) end
local function pitch(v) return math.asin(math.max(-1,math.min(1,v.y/math.max(length(v),1e-6)))) end
local function wrap(a) return (a+math.pi)%(2*math.pi)-math.pi end
local function flat(v) return math.sqrt(v.x*v.x+v.z*v.z)/math.max(length(v),1e-6) end
local function along(yaw,p) return {x=math.sin(yaw)*math.cos(p),y=math.sin(p),z=-math.cos(yaw)*math.cos(p)} end
-- The throw, and the stroke's own heading at the same corrected pitch: aim assistance looks for a
-- target around either, so a knife thrown at an enemy the head is not looking at still finds it.
local function aimed(d,gaze)
    if not gaze or length(gaze)<.5 then return d,d end
    local gazeYaw=flat(gaze)>.2 and heading(gaze) or heading(d)
    local own=flat(d)>.35 and heading(d) or gazeYaw
    local off=math.abs(wrap(own-gazeYaw))
    local share=off<=AIM.yawWindow and AIM.yawHand
        or AIM.yawHand+(1-AIM.yawHand)*math.min(1,(off-AIM.yawWindow)/(AIM.yawFade-AIM.yawWindow))
    local p=pitch(gaze)+AIM.pitchHand*(pitch(d)-pitch(gaze))
    return along(gazeYaw+wrap(own-gazeYaw)*share,p),along(own,p)
end

function M.reset(player)
    current=nil;M.state.phase='idle'
    if player then player:VRMeleeThrowCancel() end
end

function M.update(dt,player,weapon)
    local ready=player and weapon and player:VRMeleeThrowReady(weapon)
    if not ready or not dt or dt<=0 or dt>.1 or Game.GetSystemRequestsHandler():IsGamePaused() then
        M.reset(player);return player and player:VRMeleeThrowRecovering(weapon) or false
    end
    local frame=Game.VRMeleeThrowFrame()
    if not frame or #frame<4 then M.reset(player);return true end
    local h,p,b=frame[1],frame[2],frame[3]
    local key=tostring(weapon:GetEntityID().hash)
    local s=current
    if not s or s.key~=key or s.origin~=h.z then
        current={key=key,origin=h.z,seq=h.y,stamp=h.x,p=p,down=b.w>.35,history={}}
        M.state.phase='ready';return false
    end
    -- Duplicate poses may carry a new grip edge, but must not become a zero-dt velocity.
    if h.y~=s.seq then
        local span=age(h.x,s.stamp)
        if span>.1 or length(sub(p,s.p))>math.max(.45,span*16) then M.reset(player);return true end
        local moved=span>0 and length(sub(p,s.p))/span or 0
        s.seq=h.y;s.stamp=h.x;s.p=p
        if s.held then
            if moved<AIM.still then
                s.aims[#s.aims+1]={t=h.x,gaze=frame[5],eye=frame[6]}
                if #s.aims>128 then table.remove(s.aims,1) end
            end
            s.history[#s.history+1]={p=p,t=h.x}
            -- Keep the start of a 65ms segment whose end is still within the
            -- 100ms release grace, including a low-frame-rate boundary sample.
            while #s.history>1 and age(h.x,s.history[1].t)>.20 do table.remove(s.history,1) end
        end
    end
    local down=s.down and b.w>.35 or (not s.down and b.w>=.65)
    local releasing=not down and s.down and s.held
    if down and not s.down then
        s.held=true;s.started=h.x;s.history={{p=p,t=h.x}};M.state.phase='gripped'
        s.aims={{t=h.x,gaze=frame[5],eye=frame[6]}}
    elseif releasing then
        s.held=false
        local best,velocity,began=0,nil,h.x
        for i,finish in ipairs(s.history) do
            local recency=age(h.x,finish.t)
            if recency<=.10 then
                for j=math.max(1,i-6),i-1 do
                    local start=s.history[j];local seconds=age(finish.t,start.t)
                    if seconds>=.02 and seconds<=.065 then
                        local delta=sub(finish.p,start.p);local travel=length(delta)
                        local speed=travel/seconds
                        local score=speed*(1-.25*recency/.10)
                        if travel>=.035 and speed<=16 and score>best then
                            best=score;velocity=scaled(delta,1/seconds);began=start.t
                        end
                    end
                end
            end
        end
        -- A slow release is a gentle toss along the controller. A real stroke
        -- keeps its measured heading, including sideways and underhand throws.
        local speed=velocity and length(velocity) or 0
        -- The last still sample at or before the stroke's start (the clock is modulo 2^24 ms).
        local aim=s.aims[1]
        for _,a in ipairs(s.aims) do if age(began,a.t)<60 then aim=a end end
        local gaze,eye=aim.gaze or frame[5],aim.eye or frame[6]
        local direction,own=aimed(speed>=.7 and scaled(velocity,1/speed) or frame[4],gaze)
        -- Aim assistance looks first around this gaze, from the eye, and only then around the throw.
        local look=gaze and length(gaze)>.5 and world(gaze,b) or world(direction,b)
        local eyeOffset=eye and world(sub(eye,p),b) or Vector4.new(0,0,0,0)
        local launchSpeed=math.max(LAUNCH.min,math.min(LAUNCH.max,LAUNCH.base+speed*LAUNCH.perHand))
        if age(h.x,s.started)>=.04 then
            local accepted=player:VRMeleeThrowRelease(weapon,world(direction,b),launchSpeed,speed,world(own,b),look,eyeOffset)
            M.state.phase=accepted and 'released' or 'rejected'
            if accepted then M.state.releases=M.state.releases+1 end
            M.state.handSpeed=speed;M.state.launchSpeed=launchSpeed
        end
        s.history={}
    end
    s.down=down
    -- Aiming a throwable item is also a valid melee state. Reserve the input
    -- only for an armed throw, its release frame, or a pending game transition.
    return s.held or releasing or (player.VRMeleeThrowPending and player:VRMeleeThrowPending(weapon)) or false
end
return M
