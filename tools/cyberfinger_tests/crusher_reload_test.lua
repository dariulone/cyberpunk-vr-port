-- Exercise the production reload frame with each profile's own targets.
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
local profile
local gun = {GetEntityID = function() return {hash=profile.id} end, GetWorldOrientation = function() return q end}
local R = require('reload/reload')
local slide, mag = up(R.frame,'slideS'), up(R.frame,'magS')
up(R.frame,'playerFrame',function() return 0,0,0,0,0,0,1 end)
up(R.frame,'cfgFor',function() return profile end)
up(R.frame,'slotModel',function(_,name)
    if name == profile.barrel.front then return 0,1,0 end
    if name == profile.mag.slot then return 0,0,-.2 end
    return 0,0,0
end)
up(R.frame,'magAmmo',function() return ammo end)
up(R.frame,'setMagAmmo',function(_,n) ammo=n end)
R.reachMark = noop
local function frame() R.frame(gun,{},1-hand,.016) end
local function checkHandle(side)
    hand, cf, ammo = side, true, 0
    local grip, trigger = side==0 and 155 or 49, side==0 and 154 or 160
    local other = side==0 and 49 or 155
    VRLeftHanded = function() return hand==1 end
    R.holderOk = 1-hand
    slots = {}
    local _, recorded = next(profile.slide.grips)
    local g = up(R.frame,'handGrips')(recorded, side, true, profile.slide.handleSide)[1]
    local off = g.previewOff or {0,0,0}
    local function reach() wrist = {x=g.off[1]+off[1],y=g.off[2]+off[2],z=g.off[3]+off[3]} end
    wrist = {x=1,y=1,z=1}
    frame()
    mag.state, mag.pending, mag.gripLock = 'in', 8, true
    mag.snapDpre = 0 -- overlapping magazine, already seated and awaiting the handle
    slots[grip] = 1
    frame(); reach(); frame()
    assert(not slide.grabbed,'sweeping a closed hand onto the handle latched')
    slots[grip], slots[other] = 0, 1
    frame()
    assert(not slide.grabbed and R.rackReachHand==side,'wrong hand claimed the handle')
    slots[grip] = 1
    frame()
    assert(slide.grabbed and slide.hand==side and slide.input=='grip','nearby free-hand close failed')
    wrist.y = wrist.y-profile.slide.travel
    frame()
    assert(slide.racked and ammo==0,'held handle must travel before release')
    slots[trigger], slots[grip] = 1, 0
    wrist.x = 1
    for i=1,30 do frame() end
    assert(not slide.grabbed and mag.pending==nil and ammo==8,'opening the hand did not finish the reload')
    mag.snapDpre = nil
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
    for i=1,30 do frame() end
    assert(not slide.grabbed,'trigger-started grab switched input mid-pull')
    if profile.slide.manual then
        slots[grip], mag.pending, ammo, cf = 0, nil, 3, true
        frame()
        ammo = 2
        reach(); frame(); slots[grip]=1; frame()
        assert(slide.fired and slide.grabbed and slide.input=='grip','manual cycle after a shot')
        slots[grip]=0; frame()
        wrist.x=1; for i=1,30 do frame() end
    end
end

local handles, magazines, singleRound = 0, 0, false
for _, name in ipairs(require('reload/weapons/index')) do
    profile = require('reload/weapons/' .. name)
    if profile.slide and profile.slide.grabWith == 'trigger' then
        handles = handles + 1
        local magEnabled = profile.mag.enabled
        profile.mag.enabled = false -- start after the magazine has seated
        for side=0,1 do
            local ok, err = pcall(checkHandle, side)
            assert(ok, name .. ' hand ' .. side .. ': ' .. tostring(err))
        end
        profile.mag.enabled = magEnabled
    end
    if profile.mag and profile.mag.pullTrigger then
        magazines = magazines + 1
        for side=0,1 do
            hand, slots, cf = side, {}, true
            local function sample(near, closed)
                mag.wasOpenAtMag, mag.openAtMag = mag.openAtMag, nil
                return R.seatedMagazineInput(hand, near, closed)
            end
            mag.openAtMag, mag.gripLock, mag.pending = nil, false, nil
            slide.grabbed, slide.releasing, slide.ammo = false, false, 0
            assert(not sample(false, false) and not sample(true, true))
            assert(sample(true, false) and sample(true, true), name)
            assert(not sample(true, true), 'a grasp must not repeat')
            for _, state in ipairs({'loaded','pending','stock','wheel','busy','locked'}) do
                cf, slide.ammo = state ~= 'stock', state == 'loaded' and 1 or 0
                mag.pending, mag.gripLock = state == 'pending' and 4 or nil, state == 'locked'
                slide.grabbed = state == 'busy'
                slots[163] = state == 'wheel' and 3 or 0
                sample(true, false)
                assert(not sample(true, true), name .. ' took magazine while ' .. state)
            end
            slots[side==0 and 154 or 160] = 1
            assert(sample(true,true), 'explicit trigger fallback')
        end
    end
    if name == 'tsunami_ashura' then
        singleRound = profile.mag.enabled and not profile.mag.pullTrigger and not profile.slide
    end
end
assert(handles >= 20 and magazines >= 24 and singleRound)
print(('Checked %d handle and %d magazine profiles, both weapon hands'):format(handles, magazines))

-- The right free slot on the left-handed Nekomata sits 287 mm from the
-- tracked wrist. Ordinary right-hand slots and either quaternion sign pass through.
local rightBind = assert(R.slotBind(1, 0, .287, 0, 0, 0, 0, 1))
assert(math.abs(rightBind[1]+.087285)<1e-5 and math.abs(rightBind[2]-.042120)<1e-5)
assert(not R.slotBind(1,-.087285,.04212,.02095,.470714,-.513836,-.507860,-.506439))
assert(not R.slotBind(0,.0873,-.0421,-.0209,.5064,.5079,-.5138,.4707))

-- Exercise each authored slide's contact generation, including the slot-basis
-- grips. A one-sided knob must never acquire a contact on the opposite side.
local pinkyProfiles = 0
for _, name in ipairs(require('reload/weapons/index')) do
    local cfg = require('reload/weapons/' .. name)
    local sc = cfg.slide
    if sc and sc.grips then
        local contacts = R.handleContacts(sc,cfg.basis=='weapon')
        assert(#contacts > 0, 'missing contact: '..name)
        assert(contacts==R.handleContacts(sc,cfg.basis=='weapon'))
        for _,point in ipairs(contacts) do
            assert(point.radius==.032 and #point.off==3)
        end
        pinkyProfiles = pinkyProfiles+1
    end
end
print(('Checked pinky contacts on %d slide profiles'):format(pinkyProfiles))

local neko = require('reload/weapons/tsunami_nekomata')
local contacts = R.handleContacts(neko.slide,true)
assert(#contacts==2 and contacts[1].off[1]==-.030 and contacts[2].off[1]==.030)
profile = neko
profile.mag.enabled = false
for side=0,1 do
    for _,contact in ipairs(contacts) do
        hand, cf, ammo, slots = side, true, 0, {}
        VRLeftHanded = function() return hand==1 end
        R.holderOk = 1-side
        local grip = side==0 and 155 or 49
        VRHandRawRot = function() return q end
        wrist={x=1,y=1,z=1,w=1}; frame()
        mag.state, mag.pending, mag.gripLock = 'in',4,false
        local sign=side==0 and 1 or -1
        wrist={x=contact.off[1]-sign*.0697,y=-sign*.0387,z=0,w=1}
        frame(); slots[grip]=1; frame()
        assert(slide.grabbed and slide.style=='pinky' and slide.hand==side)
        wrist.y=wrist.y-neko.slide.travel; frame(); assert(slide.racked)
        slots[grip]=0; frame(); wrist.x=1
        for i=1,30 do frame() end
        assert(not slide.grabbed and ammo==4)
    end
end
profile.mag.enabled = true

-- Check the actual belt visibility rule and the pickup host resolved by onUpdate.
do
    local events={}
    registerForEvent=function(name,callback) events[name]=callback end
    package.path=mods..'/CyberpunkVRPort_TacticalBelt/?.lua;'..package.path
    local belt=assert(loadfile(mods..'/CyberpunkVRPort_TacticalBelt/init.lua'))()
    local update=events.onUpdate
    local reach=up(update,'updateMagReach')
    local wanted=up(up(update,'applyElements'),'wanted')
    local st=up(belt.state,'S')
    up(reach,'hipsModelFrame',function() return nil end)
    st.comps={ucbeltmag_copperhead={}}
    st.state.wmag='nekomata'
    cf=true; reach()
    assert(belt.magKey()=='copperhead' and wanted('weaponmag=copperhead',st.state))
    belt.cfg.enabled=false
    assert(belt.magKey()==nil,'disabled belt left a stale pickup host')
    belt.cfg.enabled=true
    st.magTaken=true
    assert(not wanted('weaponmag=copperhead',st.state))
    st.magTaken=false; cf=false; reach()
    assert(belt.magKey()==nil and not wanted('weaponmag=copperhead',st.state))
    st.state.wmag='copperhead'; reach()
    assert(belt.magKey()=='copperhead' and wanted('weaponmag=copperhead',st.state))

    -- A disabled belt can still have components worn from before it was disabled.
    local previousGame, previousMods = Game, GetMod
    local hide, piecePass, elementPass, propPass = true, nil, nil, nil
    GetMod=function() return {wornGearHidden=function() return hide end} end
    Game={GetSystemRequestsHandler=function() return {IsPreGame=function() return false end} end,
          GetPlayer=function() return {} end}
    up(update,'gamePaused',function() return false end)
    up(update,'dropIfPlayerChanged',noop)
    up(update,'entityHash',function() return 1 end)
    up(update,'applyBeltPieces',function() piecePass=st.scene end)
    up(update,'applyBeltChunks',noop)
    up(update,'applyElements',function() elementPass=st.scene end)
    up(update,'updateProps',function(_,hidden) propPass=hidden==true end)
    up(update,'sceneHides',function() return false end)
    st.ready, belt.cfg.enabled = true, false
    st.magD[0], st.shellD[1], st.litHand[0] = .01, .01, 'ucbeltgrenade'
    st.grab.pending={side=0}
    update(.016)
    assert(piecePass and elementPass and propPass, 'worn belt or props survived hide')
    assert(not belt.grenadeReach(0) and not st.grab.pending and not belt.magDist(0) and not belt.shellDist(1))
    hide=false; update(.016)
    assert(piecePass==false and elementPass==false and propPass==false,'belt did not restore on second toggle')
    Game, GetMod = previousGame, previousMods
end

-- The same wrist position can put the pinky on or off the Crusher lever.
-- These wrists lie outside all the recorded grip targets for that hand.
profile = require('reload/weapons/militech_crusher')
profile.mag.enabled = false
for side=0,1 do
    hand, cf, ammo, slots = side, true, 0, {}
    local grip = side==0 and 155 or 49
    VRLeftHanded = function() return hand==1 end
    R.holderOk = 1-side
    -- Cant the gun about +Y for the right-hand trial, with the test wrist
    -- outside the original grip's entire cant-compensation arc on both builds.
    local angle = side==1 and math.rad(10) or 0
    local s, c = math.sin(angle/2), math.cos(angle/2)
    local gunRot = {i=0,j=s,k=0,r=c}
    gun.GetWorldOrientation = function() return gunRot end
    -- Left and right reference hands have opposite local knuckle offsets.
    local toward = side==0 and q or {i=s,j=0,k=c,r=0}
    local away = side==0 and {i=0,j=0,k=1,r=0} or gunRot
    local rot = away
    VRHandRawRot = function(h) if h==side then return rot end end
    local function model(x,y,z)
        return {x=math.cos(angle)*x+math.sin(angle)*z,y=y,z=-math.sin(angle)*x+math.cos(angle)*z,w=1}
    end
    wrist = model(1,1,1); frame()
    mag.state, mag.pending, mag.gripLock = 'in', 8, false
    local contact = profile.slide.cyberFingerPinky.off
    wrist = model(contact[1]-.0697-.018,contact[2]-.0387,contact[3])
    frame(); slots[grip]=1; frame()
    assert(not slide.grabbed,'wrong edge or distant recorded wrist latched')
    rot=toward; frame()
    assert(not slide.grabbed,'already closed when pinky reached lever')
    slots[grip]=0
    wrist.w=0; frame(); wrist.w=1; slots[grip]=1; frame()
    assert(not slide.grabbed,'invalid tracking armed a pinky grasp')
    cf=false; slots[grip]=0; frame(); slots[grip]=1; frame()
    assert(not slide.grabbed and R.rackReachHand==nil,'stock controller activated the pinky shortcut')
    cf=true; mag.pending=nil; slots[grip]=0; frame(); slots[grip]=1; frame()
    assert(not slide.grabbed,'ready weapon took pinky-side support grip')
    mag.pending=8; slots[grip]=0; frame(); slots[grip]=1; frame()
    assert(slide.grabbed and slide.style=='pinky' and slide.hand==side and slide.input=='grip')
    local pinned, turns = nil, 0
    VRHandStopModel = function(h,on,p) if h==side and on then pinned=p end end
    VRHandStopRot = function(h,on) if h==side and on==1 then turns=turns+1 end end
    frame()
    assert(pinned and math.abs(pinned.x-wrist.x)+math.abs(pinned.y-wrist.y)+math.abs(pinned.z-wrist.z)<.0001)
    wrist.y=wrist.y-profile.slide.travel
    frame()
    assert(slide.racked and turns==0,'pinky grasp forced a wrist turn or failed to pull')
    slots[grip]=0; frame(); wrist=model(1,1,1)
    for i=1,30 do frame() end
    assert(not slide.grabbed and mag.pending==nil and ammo==8)
    local thumb = up(R.frame,'handGrips')(profile.slide.grips.behind,side,true,'left')[1]
    wrist=model(thumb.off[1],thumb.off[2],thumb.off[3])
    mag.pending=8; rot=away; frame(); slots[grip]=1; frame()
    assert(slide.grabbed and slide.style=='behind','recorded Crusher grasp failed')
    slots[grip]=0; frame(); wrist=model(1,1,1)
    for i=1,30 do frame() end
    VRHandStopModel, VRHandStopRot = noop, nil
end
profile.mag.enabled = true
gun.GetWorldOrientation = function() return q end

-- Replay both pump profiles with a real wrist target and game ammo transitions.
local sg = require('reload/shotgun')
local pumpState = up(up(sg.frame,'pFrame'),'P')
local helper = setmetatable({
    magAmmo=function() return ammo end, magCapacity=function() return 8 end,
    wornGearHidden=function() return false end,
    beltShellDist=function() return 2 end, beltShellRadius=function() return .1 end,
    reachMark=noop, playSnd=noop, pose=noop, setTrg=noop,
}, {__index=R.sgH})
gun.GetWorldPosition = function() return {x=0,y=0,z=0} end
Game = {GetPlayer=function() return gun end}
VRHandRawRot = function(side) return side==hand and q or nil end
for _, name in ipairs({'budget_carnage','constitutional_tactician'}) do
    profile = require('reload/weapons/'..name)
    local pc = profile.shotgun.pump
    for side=0,1 do
        sg.lost(helper)
        hand, cf, slots, ammo = side, true, {}, 4
        local grip, trigger = side==0 and 155 or 49, side==0 and 154 or 160
        local offset = helper.handGrip(pc.grip,side,1).off
        local function point(distance)
            wrist = {}
            for i,axis in ipairs({'x','y','z'}) do
                wrist[axis]=pc.restW[i]+offset[i]+distance*pc.dir[i]
            end
        end
        local function pump() assert(sg.frame(helper,gun,{},1-side,.016,profile)=='sg:pump') end
        point(0); pump(); slots[grip]=1; pump()
        assert(not pumpState.engaged,'loaded pump took support grip')
        ammo=3; pump(); assert(pumpState.spentIn and not pumpState.engaged)
        slots[grip]=0; point(1); pump(); slots[grip]=1; point(0); pump()
        assert(not pumpState.engaged,'closed hand swept into pump')
        slots[grip]=0; pump(); slots[grip]=1; pump()
        assert(pumpState.pumpGripHand==side,'gesture did not take pump')
        point(pc.travel); pump(); assert(pumpState.back)
        point(0); pump(); assert(pumpState.chambered and not pumpState.engaged)
        point(pc.travel); pump(); assert(ammo==3 and pumpState.s==0,'held grip cycled twice')
        cf, ammo, slots[grip] = false, 2, 0
        point(0); pump(); slots[grip]=1; pump()
        assert(not pumpState.engaged,'stock grip took pump')
        slots[trigger]=1; pump(); assert(pumpState.engaged,'stock chord failed')
        slots[trigger]=0; pump(); assert(not pumpState.engaged)

        -- Start a fresh pump using the pinky contact at the moving part,
        -- with the wrist turned around relative to the normal grasp.
        sg.lost(helper)
        cf, ammo, slots = true, 4, {}
        local sign=side==0 and 1 or -1
        local function pinkyAt(distance)
            wrist={x=pc.restW[1]+distance*pc.dir[1]-sign*.0697,
                   y=pc.restW[2]+distance*pc.dir[2]-sign*.0387,
                   z=pc.restW[3]+distance*pc.dir[3], w=1}
        end
        pinkyAt(0); pump(); ammo=3; pump()
        slots[grip]=1; pump()
        assert(pumpState.engaged and pumpState.pinkyGrip and pumpState.pumpGripHand==side)
        pinkyAt(pc.travel); pump(); assert(pumpState.back)
        pinkyAt(0); pump(); assert(pumpState.chambered and not pumpState.engaged)
    end
end
sg.lost(helper)

-- Replay the simple holster with the distances from the reported incident.
local H = dofile(mods..'/CyberpunkVRPort_Holster/modules/simple.lua')
local stows, draws, armed = 0,0,true
local allowStow, allowMelee, heldMelee, meleeStows = false, true, false, 0
VRAllowsGestureHolstering = function(melee)
    assert(type(melee)=='boolean')
    if not cf then return true end
    if melee then return allowMelee end
    return allowStow
end
local player = {VRUnequipWeapon=function() stows=stows+1 end,VREquipWeaponSlot3=function() draws=draws+1 end,
    VRHolsterMelee=function() meleeStows=meleeStows+1 end}
Game = {GetPlayer=function() return player end,GetTransactionSystem=function() return {
    GetItemInSlot=function(_,_,slot)
        if armed and slot == (hand==1 and 'AttachmentSlots.WeaponLeft' or 'AttachmentSlots.WeaponRight') then
            return {GetItemID=function() return {GetTDBID=noop} end, IsMelee=function() return heldMelee end}
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
    local initialMelee=meleeStows
    cf,heldMelee,allowStow,allowMelee=true,true,false,true
    slots[21]=.08
    grip(); assert(meleeStows==initialMelee+1,'gun setting blocked a simple melee stow')
    allowStow,allowMelee=true,false
    grip(); assert(meleeStows==initialMelee+1 and stows==prev+2,'gun setting enabled a simple melee stow')
    heldMelee=false
    slots[21]=.39
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
    gun.pair,gun.typeName=false,'Wea_Hammer'
    allowStow,allowMelee=false,true
    press(false); press(true)
    assert(motions==m+2 and puts==p+2,'gun setting blocked an immersive melee stow')
    state.cooldown=0
    allowStow,allowMelee=true,false
    press(false); press(true)
    assert(motions==m+2 and puts==p+2,'gun setting enabled an immersive melee stow')
    allowStow=false
    held.test,real.test=nil,nil
    press(false); press(true)
    assert(takes==d+1,'disabled immersive stow also blocked drawing')
    state.cooldown=0
end
print('CyberFinger reload and holster checks passed for both hands')
