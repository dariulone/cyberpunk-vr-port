local root=assert(arg[1])
package.path=root.."?.lua;"..package.path
local grenade=dofile(root.."grenade/grenade.lua")
local function up(fn,key,value)
    for i=1,100 do
        local name,old=debug.getupvalue(fn,i)
        if name==key then
            if value~=nil then debug.setupvalue(fn,i,value) end
            return old
        end
    end
    error("missing upvalue "..key)
end
local capture=up(grenade.tick,"grenadeHoldPose")
local identity={i=0,j=0,k=0,r=1}
Vector4={new=function(x,y,z,w) return {x=x,y=y,z=z,w=w} end}
Quaternion={new=function(i,j,k,r) return {i=i,j=j,k=k,r=r} end}
function IsDefined(o) return o~=nil end
local active=true
function VRCyberFingerEnabled() return active end
local wrist=Vector4.new(1,2,3,1)
local anchor=Vector4.new(.9,2,3,1)
local wristRot,anchorRot=identity,identity
local legacyReads=0
function VRBoneModelPos(idx)
    legacyReads=legacyReads+1
    return (idx==23 or idx==24) and wrist or anchor
end
function VRBoneModelRot() return identity end
function VRPalmModelPos(side) assert(side==0); return wrist end
function VRPalmModelRot() return wristRot end
function VRWorldToModel(p) return p end
function VRWorldRotationToModel(q) return q end
local grab={side=0, obj={GetWorldPosition=function() return anchor end,
    GetWorldOrientation=function() return anchorRot end}}
grenade.setup({CFG={},S={grab=grab},eulerQuat=function() return 0,0,0,1 end})
local pos, rot
local part={SetLocalTransform=function(_,p,q) pos,rot=p,q end}
up(capture,"grenadeParts",function() return {part} end)
up(capture,"holdNow",function() return {-.08,.05,.03},{0,0,0,1} end)
local function near(a,b) return math.abs(a-b)<1e-6 end
capture()
assert(legacyReads==0 and near(pos.x,.18) and near(pos.y,-.05) and near(pos.z,-.03))
assert(near(math.abs(rot.i),1) and near(rot.r,0))
-- Rotate the wrist 90 degrees and the actual prop root 180 degrees around Z.
local h=math.sqrt(.5)
wristRot=Quaternion.new(0,0,h,h); anchorRot=Quaternion.new(0,0,1,0)
capture()
assert(near(pos.x,-.15) and near(pos.y,-.08) and near(pos.z,-.03),"mesh did not follow the solved hand")
wrist.w=0; pos=nil; capture(); assert(pos==nil,"lost tracking wrote a stale transform")
wrist.w=1; grab.side=1; capture()
assert(legacyReads==2 and near(pos.x,.02) and near(pos.y,.05),"right-hand pickup changed")
active=false; grab.side=0; capture()
assert(legacyReads==4 and near(pos.x,.02),"non-CyberFinger pickup changed")
grab.slot=nil; grab.armSlots={["AttachmentSlots.WeaponLeft"]=true,["AttachmentSlots.GrenadeRight"]=true}
local weaponLeft,grenadeLeft,grenadeRight=grenade.reservedSlots()
assert(not grenade.held() and weaponLeft and not grenadeLeft and grenadeRight,"in-flight effect counted as held")
-- Disable the belt holder during a queued pickup, then while a grenade is already attached.
local state={grab={pending={side=0},was={}},grenadeHolsterDisabled=true,litHand={[0]="ucbeltgrenade"}}
grenade.setup({CFG={grenade_grab=true},S=state})
Game={GetPlayer=function() return {} end}
for _,fn in ipairs({"leverTick","throwSample","throwTick","setTriggerBlock","poseTick","updatePin","grenadeHoldPose"}) do
    up(grenade.tick,fn,function() end)
end
up(grenade.tick,"gripDown",function() return true end)
grenade.tick(0,.01)
assert(not state.grab.pending and not grenade.held() and not grenade.pin(0,"ucbeltgrenade"))
state.grab.slot,state.grab.side,state.grab.pinned="AttachmentSlots.GrenadeLeft",0,true
grenade.tick(.01,.01)
assert(grenade.held(),"disabling the holder took a grenade out of the hand")
print("Grenade attachment checks passed")
