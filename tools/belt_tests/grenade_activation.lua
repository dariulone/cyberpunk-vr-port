-- Run from the repository root with LuaJIT. Drive the normal belt tick.
local dir = "mods/cet/CyberpunkVRPort_TacticalBelt/"
package.path = dir .. "?.lua;" .. package.path
local grenade = require("grenade/grenade")
local flick = require("grenade/ring_flick")
local contact = require("grenade/pin_contact")
local state = {grab={was={}}, litHand={}}
local grip, slots, haptics = {}, {}, {}
local holder, sequence, owner, blocked, gunBlocked, plane
local sceneAttempts, rejectScene = 0, false
function VRGrenadeStick(side) owner = side; return sequence end
function GetVRSharedSlot(slot) return grip[slot] or 0 end
function SetVRTriggerMode(v) gunBlocked = v end
function SetVRLeftTriggerBlock(v) blocked = v end
function VRHapticEvent(_, hand) haptics[#haptics+1] = hand end
function IsDefined(v) return v ~= nil end
local player = {}
function player:VRPortHoldItem(_, slot) assert(not slots[slot]); slots[slot] = true; return true end
function player:VRPortHoldItemScene(item, slot)
    sceneAttempts = sceneAttempts + 1
    if rejectScene then return false end
    return self:VRPortHoldItem(item, slot)
end
function player:VRPortDropItem(slot) assert(slots[slot]); slots[slot] = nil; return true end
Game = {GetPlayer=function() return player end}
local cfg = {grenade_grab=true, grenade_pin=true, grenade_throw=false, grenade_sound=false}
grenade.setup({
    S=state, CFG=cfg, log=function() end, propAllowSlot=function() end,
    transactions=function() end, playerPlane=function() return plane end,
    eulerQuat=function() return 0,0,0,1 end,
    weaponInHands=function(_, side) return side ~= holder end,
    leftHanded=function() return holder == 1 end,
    propItem=function() return "Items.VRPortGrenade_frag", "vrp_gren_frag" end,
})
local now = 0
local function tick(dt) now = now + dt; grenade.tick(now, dt) end
for side=0,1 do
    holder, sequence, plane = side, 17, 2
    local button = side == 0 and 155 or 49
    local slot = "AttachmentSlots.VRPortGrenade" .. (side == 0 and "Left" or "Right")
    local function take()
        state.litHand[side] = "ucbeltgrenade"
        grip[button] = 1
        tick(.016)
        assert(slots[slot] and owner == side)
    end
    local function release()
        grip[button] = 0
        tick(.016)
        assert(not next(slots) and owner == -1 and blocked == 0)
    end
    take()
    tick(.4)
    assert(#haptics == 0 and blocked == 1 and not gunBlocked)
    sequence = sequence + 1
    tick(.08)
    release()
    assert(#haptics == 0)
    take()
    tick(.4)
    assert(#haptics == 0, "a click from the previous hold carried over")
    sequence = sequence + 1
    tick(.08)
    plane = 0
    tick(.016)
    assert(state.grab.plane == 0 and slots[slot] and owner == side)
    for _=1,32 do tick(.016) end
    assert(#haptics == 0, "a timer armed an untouched pin")
    plane = 2
    tick(.016)
    assert(state.grab.plane == 2 and slots[slot] and owner == side)
    sequence = sequence + 1
    tick(.6)
    assert(#haptics == 0, "a repeated click armed an untouched pin")
    local attempts = sceneAttempts
    plane, rejectScene = 0, true
    tick(.016)
    assert(state.grab.plane == 2 and slots[slot] and owner == side, "failed rebind lost the hold")
    rejectScene = false
    tick(.1)
    assert(sceneAttempts == attempts+1, "retried every frame")
    tick(.11)
    assert(state.grab.plane == 0 and sceneAttempts == attempts+2 and #haptics == 0)
    release()
end

-- Check clip playback at key boundaries and between keys. The base hold
-- can be any rotation, including its equivalent negative quaternion.
local function joint(t, name)
    local a,b,u = flick.fingerKeys(t)
    local x,y,z,w = flick.finger(name, 0,0,0,-1, a,b,u)
    assert(math.abs(x*x+y*y+z*z+w*w-1) < .00001)
    return 2 * math.atan2(-z,-w)
end
assert(math.abs(joint(0, "LeftHandIndex2")) < .00001)
for frame=0,110 do
    assert(joint(frame/100,"LeftHandIndex2")<=1e-6)
    assert(joint(frame/100,"LeftHandIndex3")<=1e-6)
end
assert(joint(.46, "LeftHandPinky2") == 0)
print("PASS bent supporting index throughout the gesture")

-- Exercise the component writes as well as the release curve. The active
-- wrist changes frame on every tick while the slot anchor stays still.
Vector4={new=function(x,y,z,w) return {x=x,y=y,z=z,w=w} end}
Vector3={new=function(x,y,z) return {x=x,y=y,z=z} end}
Quaternion={new=function(i,j,k,r) return {i=i,j=j,k=k,r=r} end}
function VRBoneModelPos(bone)
    if bone==24 then return Vector4.new(now*.04,0,1,1) end
    return Vector4.new(0,0,0,1)
end
function VRBoneModelRot(bone)
    if bone==24 then return Quaternion.new(0,0,math.sin(now),math.cos(now)) end
    return Quaternion.new(0,0,0,1)
end
local function mesh()
    return {visualScale=Vector3.new(1,1,1),Toggle=function() end,
            SetLocalTransform=function(self,p,q) self.pos,self.rot=p,q end}
end
local body,ring=mesh(),mesh()
holder,plane=1,2
state.litHand[1],grip[49]="ucbeltgrenade",1
tick(.016)
state.grab.parts,state.grab.pin={body,ring},{ring}
sequence=sequence+1
local function attachment(component)
    local q,h=component.rot,contact.profile(1,"frag").hinge
    local tx,ty,tz=2*(q.j*h[3]-q.k*h[2]),2*(q.k*h[1]-q.i*h[3]),2*(q.i*h[2]-q.j*h[1])
    return {component.pos.x+h[1]+q.r*tx+q.j*tz-q.k*ty,
            component.pos.y+h[2]+q.r*ty+q.k*tx-q.i*tz,
            component.pos.z+h[3]+q.r*tz+q.i*ty-q.j*tx}
end
for n=1,12 do
    tick(.048)
    assert(body.pos and ring.pos and ring.rot)
    local a,b=attachment(body),attachment(ring)
    for i=1,3 do assert(math.abs(a[i]-b[i])<1e-7,"pin socket moved during the turn") end
    assert(ring.visualScale.x==1 and ring.visualScale.y==1 and ring.visualScale.z==1)
end
tick(.048)
assert(not grenade.contactState().armed,"elapsed time released the pin")
grip[49]=0
tick(.016)
print("PASS pin turns around its seated socket and elapsed time cannot release it")
for side=0,1 do
    for _,kind in ipairs({"frag","flash","incendiary","biohazard","smoke","ozob"}) do
        local profile=contact.profile(side,kind)
        local centre,normal=profile.centre,profile.normal
        local function point(travel)
            return {centre[1]+normal[1]*travel,centre[2]+normal[2]*travel,centre[3]+normal[3]*travel}
        end
        local state=contact.new()
        -- Entire finger remains outside the opening, even after a long tick.
        assert(not contact.step(state,profile,point(.02),point(.04),10))
        assert(state.phase=="approach")
        assert(not contact.step(state,profile,point(-.018),point(.002),.016))
        assert(state.phase=="approach","backwards entry captured the ring")
        assert(not contact.step(state,profile,point(.018),point(-.002),.016))
        assert(state.phase=="hooked" and math.abs(state.along-.9)<.001)
        assert(not contact.step(state,profile,point(.019),point(-.001),.016))
        assert(state.phase=="tension" and state.force>1 and state.pull<.002)
        assert(contact.step(state,profile,point(.036),point(.016),.016))
        assert(state.phase=="released" and state.force>=22 and state.velocity)
        local speed=math.sqrt(state.velocity[1]^2+state.velocity[2]^2+state.velocity[3]^2)
        assert(speed>=1.6-1e-6 and speed<=3+1e-6)
        assert(not contact.step(state,profile,point(.036),point(.016),.016))
        local escaped=contact.new()
        contact.step(escaped,profile,point(.018),point(-.002),.016)
        contact.step(escaped,profile,point(.019),point(-.001),.016)
        local a,b=point(.022),point(.002)
        a[1],b[1]=a[1]+.1,b[1]+.1
        assert(not contact.step(escaped,profile,a,b,.016))
        assert(escaped.phase=="approach" and not escaped.along)
        assert(escaped.offset[1]==0 and escaped.offset[2]==0 and escaped.offset[3]==0)
    end
end
print("PASS six ring constraints, missed contact, spring loading, force release and single transition")

-- The fingers' track: home at both ends, the bend about local Z read between the lever's tracks; the thumb's
-- tip straightening only once the ring has let go.
local function angleAboutZ(x,y,z,w, bx,by,bz,bw)
    -- the rotation from the base to the result, as an angle about local Z (degrees)
    local i,j,k,r = bw*x-bx*w-by*z+bz*y, bw*y-by*w-bz*x+bx*z, bw*z-bz*w-bx*y+by*x, bw*w+bx*x+by*y+bz*z
    assert(math.abs(i)<1e-6 and math.abs(j)<1e-6, "bend off the hinge")
    return math.deg(2*math.atan2(k,r))
end
local base = {0.12,-0.05,-0.42,0.897}
local n = math.sqrt(base[1]^2+base[2]^2+base[3]^2+base[4]^2)
for i=1,4 do base[i]=base[i]/n end
for side=0,1 do
    for _,kind in ipairs({"frag","flash","incendiary","biohazard","smoke","ozob"}) do
        local profile=contact.profile(side,kind)
        assert(#profile.fingers==5, "one finger track per lever place")
        for _,lever in ipairs({0,.3,1}) do
            for _,t in ipairs({0,contact.duration}) do
                local a,b,u=contact.sample(profile,t)
                local x,y,z,w=contact.finger(profile,t,"LeftHandRing2",base[1],base[2],base[3],base[4],a,b,u,nil,lever)
                assert(math.abs(x-base[1])+math.abs(y-base[2])+math.abs(z-base[3])+math.abs(w-base[4])<1e-6,
                       "fingers not home at the timeline's ends")
            end
            local t=.5
            local a,b,u=contact.sample(profile,t)
            local angles=contact.fingerAngles(profile,t,lever)
            for name,col in pairs({LeftHandIndex1=1,LeftHandMiddle2=4,LeftHandPinky3=8}) do
                local x,y,z,w=contact.finger(profile,t,name,base[1],base[2],base[3],base[4],a,b,u,nil,lever)
                local share=name=="LeftHandPinky3" and .67 or 1
                assert(math.abs(-angleAboutZ(x,y,z,w,base[1],base[2],base[3],base[4])-angles[col]*share)<1e-3)
            end
        end
        -- Between two tracks of the lever, the linear blend of them.
        local function copy(v) local c={} for k=1,8 do c[k]=v[k] end return c end
        local lo=copy(contact.fingerAngles(profile,.3,.25))
        local hi=copy(contact.fingerAngles(profile,.3,.5))
        local mid=contact.fingerAngles(profile,.3,.375)
        for k=1,8 do assert(math.abs(mid[k]-(lo[k]+hi[k])*.5)<1e-9) end
        -- The tip: keyed before the release, toward profile.snap after it.
        local t=profile.release+.06
        local a,b,u=contact.sample(profile,t)
        local q=profile.rest[3]
        local kx,ky,kz,kw=contact.finger(profile,t,"LeftHandThumb2",q[1],q[2],q[3],q[4],a,b,u,{phase="hooked"})
        local sx,sy,sz,sw=contact.finger(profile,t,"LeftHandThumb2",q[1],q[2],q[3],q[4],a,b,u,
                                         {phase="released",releasedAt=profile.release})
        local s=profile.snap[2]
        local function dist(x,y,z,w) return 1-math.abs(x*s[1]+y*s[2]+z*s[3]+w*s[4]) end
        assert(dist(sx,sy,sz,sw)<1e-6 and dist(kx,ky,kz,kw)>1e-3, "the tip did not straighten on the release")
        local early={contact.finger(profile,profile.release-.05,"LeftHandThumb2",q[1],q[2],q[3],q[4],
                                    a,b,u,{phase="released",releasedAt=profile.release})}
        local keyed={contact.finger(profile,profile.release-.05,"LeftHandThumb2",q[1],q[2],q[3],q[4],a,b,u,nil)}
        for i=1,4 do assert(early[i]==keyed[i], "the tip moved before the release") end
    end
end
print("PASS fingers follow the regrip and come home; the thumb's tip straightens only after the release")
print("PASS offhand grab, trigger routing, cancelled pulse, plane changes, rebind retry and no timer arming")
