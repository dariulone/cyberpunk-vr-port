-- Start just after the Crusher's magazine seats. Exercise the production slide
-- frame with its real grip targets and shared input slots for both weapon hands.
local mods = assert(arg[1])
package.path = mods .. '/CyberpunkVRPort_Reload/?.lua;' .. package.path
math.atan2 = math.atan2 or math.atan
local function noop() end
local function up(fn, name, replace)
    for i = 1, 100 do
        local key, value = debug.getupvalue(fn, i)
        if key == name then
            if replace then debug.setupvalue(fn, i, replace) end
            return value
        end
    end
    error('Missing production upvalue: ' .. name)
end

local slots = {}
GetVRSharedSlot = function(n) return slots[n] or 0 end
GetMod = function() return nil end
VRRigWrite, VRRigWriteRot, VRRigWriteClear, VRHandStopModel = noop, noop, noop, noop
Vector4 = {new = function(x,y,z,w) return {x=x,y=y,z=z,w=w} end}
local hand, wrist, cf, ammo
VRHandRawModel = function(side) return side == hand and wrist or nil end
VRCyberFingerEnabled = function() return cf end
local q = {i=0,j=0,k=0,r=1}
local gun = {GetEntityID = function() return {hash=1} end, GetWorldOrientation = function() return q end}
local crusher = require('reload/weapons/militech_crusher')
crusher.mag.enabled = false  -- magazine handling is complete in this fixture
local R = require('reload/reload')
local slide, mag = up(R.frame,'slideS'), up(R.frame,'magS')
up(R.frame,'playerFrame',function() return 0,0,0,0,0,0,1 end)
up(R.frame,'cfgFor',function() return crusher end)
up(R.frame,'slotModel',function(_,name)
    if name == crusher.barrel.front then return 0,1,0 end
    if name == crusher.mag.slot then return 0,0,-.2 end
    return 0,0,0
end)
up(R.frame,'magAmmo',function() return ammo end)
up(R.frame,'setMagAmmo',function(_,n) ammo=n end)
R.reachMark = noop
local function frame() R.frame(gun,{},1-hand,.016) end
for side=0,1 do
    hand, cf, ammo = side, true, 0
    local grip, trigger = side==0 and 155 or 49, side==0 and 154 or 160
    local other = side==0 and 49 or 155
    VRLeftHanded = function() return hand==1 end
    R.holderOk = 1-hand
    slots = {}
    local target = up(R.frame,'handGrips')(crusher.slide.grips.behind, side, true, crusher.slide.handleSide)[1].off
    local function reach() wrist = {x=target[1],y=target[2],z=target[3]} end
    wrist = {x=1,y=1,z=1}
    frame()
    mag.state, mag.pending, mag.gripLock = 'in', 8, true
    slots[grip] = 1
    frame(); reach(); frame()
    assert(not slide.grabbed,'sweeping a closed hand onto the handle latched')
    slots[grip], slots[other] = 0, 1
    frame()
    assert(not slide.grabbed and R.rackReachHand==side,'wrong hand claimed the handle')
    slots[grip] = 1
    frame()
    assert(slide.grabbed and slide.hand==side and slide.input=='grip','nearby free-hand close failed')
    wrist.y = wrist.y-crusher.slide.travel
    frame()
    assert(slide.racked and ammo==0,'held handle must travel before release')
    slots[trigger], slots[grip] = 1, 0
    wrist.x = 1
    for i=1,12 do frame() end
    assert(not slide.grabbed and mag.pending==nil and ammo==8,'opening the hand did not finish the reload')
    slots[trigger]=0
    reach(); frame(); slots[grip]=1; frame()
    assert(not slide.grabbed,'loaded gun stole the support grip')
    mag.pending, cf = 8, false
    slots[grip]=0; frame(); slots[grip]=1; frame()
    assert(not slide.grabbed,'changed stock controller grip')
    slots[trigger]=1; frame()
    assert(slide.grabbed and slide.input=='trigger','original Trigger path failed')
    slots[trigger]=0; frame()
    wrist.x=1
    for i=1,12 do frame() end
    assert(not slide.grabbed,'trigger-started grab switched input mid-pull')
end

-- Replay the simple holster with the distances from the reported incident.
local H = dofile(mods..'/CyberpunkVRPort_Holster/modules/simple.lua')
local stows, draws, armed = 0,0,true
local allowStow = false
VRAllowsGestureHolstering = function() return not cf or allowStow end
local player = {VRUnequipWeapon=function() stows=stows+1 end,VREquipWeaponSlot3=function() draws=draws+1 end}
Game = {GetPlayer=function() return player end,GetTransactionSystem=function() return {
    GetItemInSlot=function(_,_,slot)
        if armed and slot == (hand==1 and 'AttachmentSlots.WeaponLeft' or 'AttachmentSlots.WeaponRight') then
            return {GetItemID=function() return {GetTDBID=noop} end}
        end
    end
} end}
TweakDBID = {new=function(s) return s end}
TDBID = {ToStringDEBUG=function() return 'Crusher' end}
spdlog = {info=noop}
GetMod = function(name) if name=='CyberpunkVRPort_Reload' then return {reload=function() return R end} end end
H.init()
for side=0,1 do
    hand, cf, armed, allowStow = side,true,true,false
    R.ownedHand,R.rackReachHand=nil,nil
    local drawing = hand==1 and 155 or 49
    slots={[20]=.42,[21]=.39,[22]=.27,[23]=1}
    local function grip()
        slots[drawing]=0; H.update(.7)
        slots[drawing]=1; H.update(.016)
    end
    local prev = stows
    grip(); assert(stows==prev,'holstered from 39 cm')
    slots[21]=.08
    grip(); assert(stows==prev,'disabled gesture stowed at the hip')
    allowStow=true
    H.update(.1); assert(stows==prev,'enabling while squeezed replayed a stow')
    R.rackReachHand=side
    grip(); assert(stows==prev,'holstered during handle preview')
    R.rackReachHand=nil; R.ownedHand=side
    grip(); assert(stows==prev,'holstered during active reload')
    R.ownedHand=nil
    grip(); assert(stows==prev+1,'deliberate stow at hip failed')
    cf=false; slots[21]=.39
    grip(); assert(stows==prev+2,'changed stock holster reach')
    local d=draws
    cf,armed,allowStow=true,false,false
    grip(); assert(draws==d+1,'changed empty-hand draw reach')
end
-- Use the immersive holster's actual grip-edge and weapon selection code.
-- Count animation and equipment requests separately: either could hide a gun.
package.path = mods..'/CyberpunkVRPort_Holster/?.lua;'..package.path
local path = mods..'/CyberpunkVRPort_Holster/modules/holsters.lua'
-- The offline interpreter is Lua 5.4. The unexercised appearance picker
-- spells its integer constants with LuaJIT's ULL suffix.
local script = assert(io.open(path,'r'))
local source = script:read('*a')
script:close()
if not jit then source = source:gsub('(%x)ULL%f[%W]', '%1') end
local immersive = assert((loadstring or load)(source,'@'..path))()
local sample = up(immersive.update,'hand')
local grab = up(sample,'grab')
local state = up(sample,'S')
local motions, puts, takes = 0,0,0
up(grab,'stow',function() motions=motions+1 end)
up(grab,'holsterHeld',function() puts=puts+1 end)
up(grab,'draw',function() takes=takes+1 end)
up(grab,'show',noop)
VRHandRawWorld = function() return {x=0,y=0,z=0,w=1} end
local anchor={GetWorldPosition=function() return {x=0,y=0,z=0} end}
for side=0,1 do
    hand,cf,allowStow=1-side,true,false
    local slot=side==0 and 155 or 49
    local gun={key='test',handSide=side,attached=true,entity=anchor,place={reach=.12}}
    state.copies={test=gun}
    local held,real={test=true},{test=gun}
    local function press(down)
        slots={[slot]=down and 1 or 0}
        sample(player,held,real,.016,true)
    end
    local m,p,d=motions,puts,takes
    for _,paired in ipairs({false,true}) do
        gun.pair=paired
        press(false); press(true)
        assert(motions==m and puts==p,'disabled immersive gesture moved a weapon')
    end
    allowStow=true; press(true)
    assert(puts==p,'immersive toggle replayed an existing grip')
    press(false); press(true)
    assert(motions==m+1 and puts==p+1,'enabled immersive gesture did not stow')
    state.cooldown=0
    allowStow=false; gun.pair=false
    held.test,real.test=nil,nil
    press(false); press(true)
    assert(takes==d+1,'disabled immersive stow also blocked drawing')
    state.cooldown=0
end
print('Crusher reload and simple/immersive gesture holstering passed for both hands')
