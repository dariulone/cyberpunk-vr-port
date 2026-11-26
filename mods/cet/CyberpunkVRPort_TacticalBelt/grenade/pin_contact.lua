local profiles = require("grenade/poses/pin_contact")
local M = { duration = 1.27 }
local thumbs = {LeftInHandThumb=1, LeftHandThumb1=2, LeftHandThumb2=3}
M.thumbBones=thumbs
-- The four fingers' phalanges and the bend each takes from profile.fingers: its column (index, middle, ring,
-- little; MCP then PIP) and its share (the DIP follows the PIP at two thirds).
local fingers = {LeftHandIndex1={1,1}, LeftHandIndex2={2,1}, LeftHandIndex3={2,.67},
                 LeftHandMiddle1={3,1}, LeftHandMiddle2={4,1}, LeftHandMiddle3={4,.67},
                 LeftHandRing1={5,1}, LeftHandRing2={6,1}, LeftHandRing3={6,.67},
                 LeftHandPinky1={7,1}, LeftHandPinky2={8,1}, LeftHandPinky3={8,.67}}
for _,hand in pairs(profiles) do
    for _,profile in pairs(hand) do
        local n=profile.normal
        local length=math.sqrt(n[1]^2+n[2]^2+n[3]^2)
        n[1],n[2],n[3]=n[1]/length,n[2]/length,n[3]/length
    end
end

local function lerp(a,b,u)
    return a+(b-a)*u
end

-- Normalized lerp of two quaternions given as numbers (no tables: this runs per bone per frame).
local function nlerp(ax,ay,az,aw,bx,by,bz,bw,u)
    if ax*bx+ay*by+az*bz+aw*bw < 0 then bx,by,bz,bw = -bx,-by,-bz,-bw end
    local x,y,z,w = lerp(ax,bx,u),lerp(ay,by,u),lerp(az,bz,u),lerp(aw,bw,u)
    local n = math.sqrt(x*x+y*y+z*z+w*w)
    return x/n,y/n,z/n,w/n
end

local function rotation(a,b,u)
    return nlerp(a[1],a[2],a[3],a[4],b[1],b[2],b[3],b[4],u)
end

function M.profile(side,kind)
    return profiles[side] and profiles[side][kind]
end

function M.sample(profile,t)
    local keys=profile.keys
    for i=2,#keys do
        if t<=keys[i].t then
            local a,b=keys[i-1],keys[i]
            local u=math.max(0,(t-a.t)/(b.t-a.t))
            return a,b,u*u*(3-2*u)
        end
    end
    return keys[#keys],keys[#keys],0
end

function M.weight(t)
    local u=math.min(1,math.max(0,t/.18))
    local v=math.min(1,math.max(0,(t-.95)/.32))
    return u*u*(3-2*u)*(1-v*v*(3-2*v))
end

function M.idleFinger(profile,name,amount)
    local index=thumbs[name]
    if not index then return nil end
    return rotation(profile.rest[index],profile.closedRest[index],amount)
end

-- THE TIP STRAIGHTENS ON THE YANK: once the ring lets go, 0..1 over the time since (rise, hold, settle).
-- tools/belt_tests/pin_runtime.hpp replays the same curve when the bake checks the room for it.
function M.snap(since)
    if since < 0 then return 0 end
    if since < .025 then local u=since/.025; return u*u*(3-2*u) end
    if since < .095 then return 1 end
    local u=math.min(1,(since-.095)/.22)
    return 1-u*u*(3-2*u)
end

-- THE FINGERS FOLLOW THE REGRIP: each finger's extra MCP and PIP flexion over the hold, in degrees, solved
-- against the first-person hand's skin for every 20 ms of the timeline and for five places of the lever
-- (tools/belt_tests/bake_pin_contact.cpp). Read linearly in time and between the lever's tracks, once per frame.
local angles, cacheProfile, cacheT, cacheLever = {0,0,0,0,0,0,0,0}, nil, nil, nil
local function fingerAngles(profile, t, lever)
    if cacheProfile == profile and cacheT == t and cacheLever == lever then return angles end
    cacheProfile, cacheT, cacheLever = profile, t, lever
    local tracks = profile.fingers
    local n = #tracks
    local x = math.min(1, math.max(0, lever or 0)) * (n - 1)
    local i = math.min(n - 2, math.floor(x))
    if i < 0 then i = 0 end
    local v = n > 1 and (x - i) or 0
    local a, b = tracks[i + 1], tracks[math.min(n, i + 2)]
    local step = a[2][1] - a[1][1]
    local j = math.min(#a - 1, math.max(1, math.floor(t / step) + 1))
    local u = math.min(1, math.max(0, (t - a[j][1]) / (a[j + 1][1] - a[j][1])))
    local ra, rb, sa, sb = a[j], a[j + 1], b[j], b[j + 1]
    for k = 1, 8 do
        local p = ra[k + 1] + (rb[k + 1] - ra[k + 1]) * u
        local q = sa[k + 1] + (sb[k + 1] - sa[k + 1]) * u
        angles[k] = p + (q - p) * v
    end
    return angles
end
M.fingerAngles = fingerAngles

-- The holding hand's keyed bones over the hold (x,y,z,w) at timeline time t: the thumb's three from the keys,
-- and the four fingers' phalanges bent from the track by `lever` (0 open, 1 shut: the hold the timeline started
-- from). `state` is the contact; after its release the thumb's tip straightens toward profile.snap. Nil for
-- any other bone.
function M.finger(profile,t,name,x,y,z,w,a,b,u,state,lever)
    local index=thumbs[name]
    if not index then
        local f=fingers[name]
        if not f or not profile.fingers then return nil end
        local deg=fingerAngles(profile,t,lever)[f[1]]*f[2]
        if deg==0 then return x,y,z,w end
        -- q * rotation about local Z by -deg (flexion)
        local half=-math.rad(deg)*.5
        local sz,cz=math.sin(half),math.cos(half)
        return x*cz+y*sz, y*cz-x*sz, w*sz+z*cz, w*cz-z*sz
    end
    local weight=M.weight(t)
    local p,q=a.q[index],b.q[index]
    local i,j,k,r=nlerp(p[1],p[2],p[3],p[4],q[1],q[2],q[3],q[4],u)
    q=profile.rest[index]
    local tx,ty,tz,tw=q[4]*i+q[1]*r+q[2]*k-q[3]*j,q[4]*j+q[2]*r+q[3]*i-q[1]*k,
                      q[4]*k+q[3]*r+q[1]*j-q[2]*i,q[4]*r-q[1]*i-q[2]*j-q[3]*k
    i,j,k,r=nlerp(x,y,z,w,tx,ty,tz,tw,weight)
    local snap=profile.snap
    if index>1 and snap and state and state.releasedAt then
        local s=M.snap(t-state.releasedAt)*weight
        if s>0 then
            q=snap[index-1]
            i,j,k,r=nlerp(i,j,k,r,q[1],q[2],q[3],q[4],s)
        end
    end
    return i,j,k,r
end

function M.grip(a,b,u)
    local i,j,k,r=rotation(a.grip[1],b.grip[1],u)
    local p,q=a.grip[2],b.grip[2]
    return i,j,k,r,lerp(p[1],q[1],u),lerp(p[2],q[2],u),lerp(p[3],q[3],u)
end

function M.blendGrip(t,from,to)
    local u=M.weight(t)
    local i,j,k,r=rotation(from[2],to[2],u)
    local a,b=from[1],to[1]
    return lerp(a[1],b[1],u),lerp(a[2],b[2],u),lerp(a[3],b[3],u),i,j,k,r
end

function M.pinRotation(profile,t)
    local u=math.min(1,math.max(0,t/.22))
    return rotation({0,0,0,1},profile.pinRotation,u*u*(3-2*u))
end

local function tipRadius(along)
    if along>=.95 then return .0062-(along-.95)*.034 end
    if along>=.85 then return .0076-(along-.85)*.014 end
    return .0085
end

function M.new()
    return {phase="approach",force=0,pull=0,offset={0,0,0}}
end

-- Kinematic finger drives a constrained pin. The retaining spring reacts to
-- axial travel, with damping against the pulling velocity. These are game
-- tuning values. No elapsed-time condition can detach an untouched pin.
function M.step(s,profile,a,b,dt)
    if s.phase=="released" then return false end
    local c,n=profile.centre,profile.normal
    local dx,dy,dz=b[1]-a[1],b[2]-a[2],b[3]-a[3]
    local denominator=dx*n[1]+dy*n[2]+dz*n[3]
    if not s.along then
        if denominator>-.003 then return false end
        local u=((c[1]-a[1])*n[1]+(c[2]-a[2])*n[2]+(c[3]-a[3])*n[3])/denominator
        if u<.65 or u>.985 then return false end
        local x,y,z=a[1]+dx*u,a[2]+dy*u,a[3]+dz*u
        local length=math.sqrt(dx*dx+dy*dy+dz*dz)
        local radius=profile.radius-tipRadius(u)*length/math.abs(denominator)
        if radius<.0003 then return false end
        if (x-c[1])^2+(y-c[2])^2+(z-c[3])^2>radius*radius then return false end
        s.along,s.origin,s.phase=u,{x,y,z},"hooked"
        return false
    end
    local x,y,z=a[1]+dx*s.along-s.origin[1],a[2]+dy*s.along-s.origin[2],a[3]+dz*s.along-s.origin[3]
    local pull=math.max(0,x*n[1]+y*n[2]+z*n[3])
    local speed=(pull-s.pull)/math.max(.001,dt)
    -- Losing the opening releases the finger, including during loading.
    local radial=(x-pull*n[1])^2+(y-pull*n[2])^2+(z-pull*n[3])^2
    if radial>(profile.radius-tipRadius(s.along))^2 then
        s.along,s.origin,s.phase=nil,nil,"approach"
        s.force,s.pull=0,0
        s.offset,s.feedback={0,0,0},nil
        return false
    end
    s.force=math.max(0,1500*pull+1.5*speed)
    s.pull=pull
    s.offset={n[1]*pull,n[2]*pull,n[3]*pull}
    s.phase=pull>.0005 and "tension" or "hooked"
    if s.force>=22 and pull>=.006 then
        s.phase="released"
        local launch=math.min(3,math.max(1.6,math.sqrt(1500/.04)*pull))
        s.velocity={n[1]*launch,n[2]*launch,n[3]*launch}
        return true
    end
    return false
end

return M
