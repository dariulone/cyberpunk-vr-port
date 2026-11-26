-- B owns the release. During its first 220 ms the pistol controller can direct
-- that same falling prop outward. Inputs are XR samples before IK and recoil.
local M = { releases = 0, flicks = 0 }
local armed
local function finite(n) return type(n) == 'number' and n == n and math.abs(n) < math.huge end
local function xyz(p) return p and finite(p.x) and finite(p.y) and finite(p.z) end

local function readFrame(side)
    if not Game.VRWeaponTriggerEnabled() or Game.GetSystemRequestsHandler():IsGamePaused()
        or (Game.VRWeaponInLeftHand() and 0 or 1) ~= side
        or (GetVRSharedSlot(side == 1 and 160 or 154) or 1) > .2 then return end
    -- This accessor supplies the physical weapon hand, the tracking origin,
    -- sample clock, tracking-to-world yaw, head direction and head position.
    local f = Game.VRMeleeThrowFrame()
    if f and #f >= 6 and xyz(f[1]) and xyz(f[2]) and xyz(f[5]) and xyz(f[6])
        and finite(f[3].x) and finite(f[3].y) then return f end
end

-- Older or partial script installations can still use the normal B drop.
local function sample(side)
    local ok,f=pcall(readFrame,side)
    if ok then return f end
end

function M.cancel() armed = nil end
function M.begin(weapon, side, drop)
    armed = nil
    if not drop then return end
    local ok,pistol=pcall(function() return WeaponObject.IsOfType(weapon:GetItemID(),gamedataItemType.Wea_Handgun) end)
    if not ok or not pistol then return end
    M.releases = M.releases + 1
    M.lastDelta = 0
    local f = sample(side)
    if not f then return end
    local gaze = f[5]
    local horizontal = math.sqrt(gaze.x*gaze.x + gaze.z*gaze.z)
    if horizontal < .4 then return end
    local sign = side == 1 and 1 or -1
    armed = {drop=drop, side=side, origin=f[1].z, seq=f[1].y, start=f[1].x, stamp=f[1].x,
        p=f[2], head=f[6], ax=-sign*gaze.z/horizontal, az=sign*gaze.x/horizontal,
        distance=0, duration=0, steps=0, elapsed=0}
end

function M.update(drop, side, held, dt, velocity, maxSpeed)
    local a = armed
    if not a then return end
    if not held or a.drop ~= drop or a.side ~= side or not finite(dt) or dt <= 0 or dt > .08 then
        armed=nil; return
    end
    a.elapsed = a.elapsed + dt
    local f = sample(side)
    if not f or a.elapsed > .22 or f[1].z ~= a.origin or (f[1].x-a.start)%16777216 > 220 then
        armed=nil; return
    end
    if f[1].y == a.seq then return end
    local seconds = ((f[1].x-a.stamp)%16777216)*.001
    if seconds <= 0 or seconds > .08 then armed=nil; return end
    local p,h = f[2],f[6]
    local dx,dy,dz = p.x-a.p.x,p.y-a.p.y,p.z-a.p.z
    local hx,hz = h.x-a.head.x,h.z-a.head.z
    a.p,a.head,a.stamp,a.seq = p,h,f[1].x,f[1].y
    local length2 = dx*dx + dy*dy + dz*dz
    if length2 > (4*seconds)^2 then armed=nil; return end -- tracking jump
    local outward = dx*a.ax + dz*a.az
    -- Common head/hand translation and a rotating controller at rest contribute
    -- no stroke. Count consecutive samples along the arm-time lateral axis.
    local travel = math.min(outward, outward-hx*a.ax-hz*a.az)
    if travel < .6*seconds or outward*outward < .64*length2 then
        a.distance,a.duration,a.steps=0,0,0; return
    end
    a.distance,a.duration,a.steps=a.distance+travel,a.duration+seconds,a.steps+1
    if a.steps < 2 or a.distance < .065 then return end

    armed=nil
    local yaw = f[3]
    local wx,wy = yaw.x*a.ax + yaw.y*a.az, yaw.y*a.ax - yaw.x*a.az
    local impulse = math.min(2, .7 + a.distance/a.duration)
    local vx,vy,vz = velocity[1]+wx*impulse,velocity[2]+wy*impulse,velocity[3]
    local scale=math.min(1,maxSpeed/math.sqrt(vx*vx+vy*vy+vz*vz+1e-12))
    M.lastDelta=(vx*scale-velocity[1])*wx + (vy*scale-velocity[2])*wy
    velocity[1],velocity[2],velocity[3]=vx*scale,vy*scale,vz*scale
    M.flicks=M.flicks+1
end
return M
