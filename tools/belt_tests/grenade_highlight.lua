-- Run from the repository root with LuaJIT. Exercise the belt's actual scan,
-- appearance writes, worn-record lookup and exported grip query.
local dir = "mods/cet/CyberpunkVRPort_TacticalBelt/"
package.path = dir .. "?.lua;" .. package.path
local events, palms, components = {}, {}, {}
function registerForEvent(name, fn) events[name] = fn end
local function identity(x) return x end
CName, TweakDBID, ItemID = {new=identity}, {new=identity}, {GetTDBID=identity}
function IsDefined(x) return x ~= nil end
function VRPalmModelPos(side) return palms[side] end
local holsterDistance, armed, hipsValid = nil, false, true
function GetMod() return { holsters = function()
    return { near = function() return holsterDistance end }
end } end
local player = {
    GetEntityID = function() return {hash=23} end,
    FindComponentByName = function(_, name) return components[name] end,
}
local worn = {record="Items.vrp_belt"}
function worn:GetItemID() return self.record end
function worn:GetEntityID() return {hash=self.record} end
local prop
local bothOccupied = false
local ts = {GetItemInSlot=function(_, _, slot)
    if slot == "AttachmentSlots.VRPortBelt6" then return prop end
    if slot == "AttachmentSlots.WeaponLeft" or slot == "AttachmentSlots.WeaponRight" then
        local left = worn.record == "Items.vrp_belt_lh"
        if bothOccupied or armed and ((slot == "AttachmentSlots.WeaponLeft") == left) then return worn end
        return nil
    end
    return worn
end}
local appearanceWrites = 0
Game = {
    GetPlayer=function() return player end,
    GetTransactionSystem=function() return ts end,
    VREntityComponents=function()
        local list = {}
        for _, comp in pairs(components) do list[#list+1] = comp end
        return list
    end,
    VRComponentChangeAppearance=function(c, appearance, wait)
        assert(wait)
        c.meshAppearance.value = appearance
        appearanceWrites = appearanceWrites + 1
        return true
    end,
}
local belt = assert(loadfile(dir .. "init.lua"))()
local function up(fn, name, replacement)
    for i=1,100 do
        local key, value = debug.getupvalue(fn, i)
        if key == name then
            if replacement then debug.setupvalue(fn, i, replacement) end
            return value
        end
        if not key then break end
    end
    error("missing belt fixture boundary " .. name)
end
local scan = up(events.onUpdate, "updateHighlight")
local refresh = up(up(events.onUpdate, "applyElements"), "beltComponents")
local state = up(scan, "S")
up(scan, "playerPlane", function() return armed and 2 or 0 end)
up(scan, "hipsModelFrame", function()
    if hipsValid then return 0, -.0151072554, .96074301, .701699018, -.087263152, .701704204, -.0872637928 end
end)
local function mesh(name, skin)
    return { enabled=true, meshAppearance={value=skin},
             IsEnabled=function(self) return self.enabled end,
             GetName=function() return {value=name} end }
end
local host, pouch = "ucbeltgrenade_smoke", "ucbeltmag_test"
local smoke = mesh(host, "default")
components[host], components[pouch] = smoke, mesh(pouch, "default")
state.state.gtype = "smoke"
local cfg = belt.cfg
cfg.force_appearance, cfg.auto_equip = false, false
local function tick() scan(.051) end
local function hand(side, target, dx)
    palms[side] = {x=target[1]+dx, y=target[2], z=target[3], w=1}
end
local function check(side, skin, grip)
    assert(smoke.meshAppearance.value == skin, smoke.meshAppearance.value .. " expected " .. skin)
    assert(belt.grenadeReach(side) == grip, "unexpected grip eligibility")
end

for _, mirrored in ipairs({false, true}) do
    worn.record = mirrored and "Items.vrp_belt_lh" or "Items.vrp_belt"
    cfg.centres[pouch] = {cfg.centres[host][1] + .40, cfg.centres[host][2], cfg.centres[host][3]}
    refresh()
    local sign = mirrored and -1 or 1
    assert(state.beltMirrored == mirrored)
    local centre = cfg.centres[host]
    local target = {sign*centre[1], centre[2], centre[3]}
    for _, drawn in ipairs({false, true}) do
        armed = drawn
        for side=0,1 do
            palms[1-side] = nil
            hand(side, target, sign*.82); tick(); check(side, "default", false)
            local free = not armed or side == (mirrored and 1 or 0)
            hand(side, target, .004); tick(); check(side, "default_lit", free)
            local writes = appearanceWrites
            hand(side, target, sign*.19); tick(); check(side, "default_lit", free)
            assert(appearanceWrites == writes)
            -- The pouch is now nearest but also outside entry. The old grenade
            -- must release on this scan even if both controllers remain valid.
            hand(side, target, sign*.82); tick(); check(side, "default", false)
        end
    end
    armed = false
    holsterDistance = .001
    hand(0, target, .004); tick(); check(0, "default_lit", false)
    holsterDistance = nil
    hand(1, target, -.004); tick(); check(1, "default_lit", true)
    palms[0] = nil; tick(); check(0, "default_lit", false)
    armed, bothOccupied = true, true; tick(); check(1, "default_lit", false)
    smoke.enabled = false; tick()
    assert(smoke.meshAppearance.value == "default" and not belt.grenadeReach(1))
    smoke.enabled = true; tick(); check(1, "default_lit", false)
    cfg.highlight = false; tick(); check(1, "default", false)
    cfg.highlight = true; armed, bothOccupied = false, false; tick(); check(1, "default_lit", true)
    hipsValid = false; tick(); check(1, "default", false)
    hipsValid = true; palms[0], palms[1] = nil, nil
end

-- The flash component's original CName and the mesh's default skin differ.
local flash = "ucbeltgrenade_flash"
components[flash] = mesh(flash, "militech")
components[host].enabled = false
state.recheck = -10; refresh()
hand(1, {up(scan, "elementCentre")(flash)}, 0); tick()
assert(components[flash].meshAppearance.value == "default_lit")
palms[1] = nil; tick()
assert(components[flash].meshAppearance.value == "militech")

-- Optional followed prop measures from the drawable plain mesh's world matrix.
local fakeBody = {GetLocalToWorld=function() return {W={x=30,y=20,z=1}} end}
prop = {FindComponentByName=function(_, name)
    assert(name == "mag_mesh")
    return fakeBody
end}
function VRPalmWorldPos(side)
    if side==0 then return {x=30.02,y=20,z=1,w=1} end
end
local propScan = up(events.onUpdate, "updatePropDistances")
for i,p in ipairs(cfg.props) do
    if p.follow == "grenade" then
        assert(not p.on)
        p.on = true
        propScan(10)
        assert(math.abs(up(propScan, "propS").dist[i]-.02)<.00001)
    end
end
print("PASS grenade hover, grip arbitration, withdrawal, handedness, flash restore and followed prop anchor")
