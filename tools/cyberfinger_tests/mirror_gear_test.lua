local gear = dofile(assert(arg[1]))
local hidden, paused, johnny, searches = true, false, 0, 0
local entities, nextKey = {}, 0
local function component(name, enabled, visual)
    nextKey = nextKey + 1
    return { key=nextKey, enabled=enabled, GetName=function() return name end,
        IsA=function(_,class) return visual~=false and class=="entIVisualComponent" end,
        IsEnabled=function(self) return self.enabled end,
        Toggle=function(self,on) self.enabled=on end }
end
local function entity(id, parts, item)
    local e = { parts=parts, slots={}, GetEntityID=function() return {hash=id} end,
        GetItemID=function() return item end, IsA=function(_,class) return class=="NPCPuppet" end,
        GetRecordID=function() return "Character.q110_tpp_female_v" end,
        GetResolvedGenderName=function() return "Female" end }
    entities[id] = e
    return e
end
local player = entity(1,{component("vrp_belt",true), component("vrp_belt",true),
    component("vrp_belt_strap",false), component("outfit_belt",true), component("ucbeltgrenade",true)})
local mirror = entity(2,{component("ucbeltgrenade",true), component("vrp_belt_strap",true),
    component("outfit_belt",true), component("ucbeltmag",false), component("ucbelt_controller",true,false)})
local gun = entity(3,{component("mesh",true),component("mesh",false)},"Wea_Rifle")
local prop = entity(4,{component("mesh",true)},"frag_prop")
local sleeve = entity(5,{component("mesh",true)},"Items.OutfitBelt")
local arm = entity(6,{component("skin",true)},"Cyb_StrongArms")
mirror.slots = {["AttachmentSlots.WeaponRight"]=gun, ["AttachmentSlots.VRPortBelt6"]=prop,
    ["OutfitSlots.Waist"]=sleeve, ["AttachmentSlots.WeaponLeft"]=arm}
local playerBelt = entity(8,{component("mesh",true),component("mesh",false)},"Items.vrp_belt_cf_lh")
local outfit = entity(9,{component("mesh",true)},"Items.OutfitBelt")
local hand = entity(10,{component("mesh",true)},"Wea_Rifle")
player.slots = {["AttachmentSlots.VRPortWaist"]=playerBelt, ["OutfitSlots.Waist"]=outfit,
    ["AttachmentSlots.WeaponLeft"]=hand}
local found = mirror
local function find(p,radius,onlyMirror)
    searches=searches+1; assert(p==player and radius==15 and onlyMirror)
    return found
end
local grenades = true
GetMod=function() return {wornGearHidden=function() return hidden end, grenadeHolsterEnabled=function() return grenades end} end
NameToString=function(n) return n end
TweakDBID={new=NameToString}; ItemID={GetTDBID=NameToString}; TDBID={ToStringDEBUG=NameToString}
RPGManager={GetItemType=function(item) return {value=item} end}
Game={NameToString=NameToString,
    GetSystemRequestsHandler=function() return {IsPreGame=function() return false end, IsGamePaused=function() return paused end} end,
    GetQuestsSystem=function() return {GetFactStr=function() return johnny end} end,
    FindEntityByID=function(id) return entities[id.hash] end,
    VREntityComponents=function(e) return e.parts end, VRComponentKey=function(c) return c.key end,
    GetTransactionSystem=function() return {GetItemInSlot=function(_,actor,slot)
        assert(actor==player or actor==mirror); return actor.slots[slot] end} end
}
local function tick(bound) gear.update(player,.25,bound,find,15) end
assert(gear.matches("character.q110_tpp_male_v",false,true))
assert(not gear.matches("character.q110_tpp_male_v",true,true))
assert(not gear.matches("character.some_tpp_female_v",true,true))
hidden=false; tick(); assert(searches==0)
paused=true; hidden=true; tick(); assert(searches==0)
paused=false; tick(); assert(searches==1)
assert(not mirror.parts[1].enabled and not mirror.parts[2].enabled and mirror.parts[3].enabled)
assert(not mirror.parts[4].enabled and mirror.parts[5].enabled)
assert(not player.parts[1].enabled and not player.parts[2].enabled and not player.parts[5].enabled)
assert(not playerBelt.parts[1].enabled and player.parts[4].enabled and outfit.parts[1].enabled and hand.parts[1].enabled)
assert(not gun.parts[1].enabled and not gun.parts[2].enabled and not prop.parts[1].enabled)
assert(sleeve.parts[1].enabled and arm.parts[1].enabled)

-- Garment assembly can replace or re-enable a mesh without replacing either puppet.
player.parts[1] = component("vrp_belt",false)
player.parts[2].enabled = true
player.parts[6] = component("vrp_belt_strap",true)
mirror.parts[7] = component("vrp_belt_strap",true)
-- An outfit rebuild replaces the component with a previously used name.
mirror.parts[2]=component("vrp_belt_strap",false)
mirror.parts[6]=component("ucbeltammo_shell1",true)
local cfBelt=entity(7,{component("mesh",true)},"Items.vrp_belt_cf_lh")
mirror.slots["OutfitSlots.Waist"]=cfBelt
mirror.slots["AttachmentSlots.WeaponRight"]=nil
tick()
assert(not mirror.parts[6].enabled and gun.parts[1].enabled and not gun.parts[2].enabled)
assert(not cfBelt.parts[1].enabled and not player.parts[2].enabled and not player.parts[6].enabled and not mirror.parts[7].enabled)
hidden=false; tick()
assert(cfBelt.parts[1].enabled and mirror.parts[7].enabled)
assert(not player.parts[1].enabled and player.parts[2].enabled and not player.parts[3].enabled and player.parts[6].enabled)
assert(playerBelt.parts[1].enabled and not playerBelt.parts[2].enabled)
assert(mirror.parts[1].enabled and not mirror.parts[2].enabled and mirror.parts[6].enabled and prop.parts[1].enabled)

-- Known interactive doubles do not require a target search. Johnny and V keep their own appearances.
hidden=true; tick(mirror:GetEntityID()); assert(not mirror.parts[1].enabled and searches==1)
johnny=1; tick(mirror:GetEntityID()); assert(mirror.parts[1].enabled and not player.parts[2].enabled and not playerBelt.parts[1].enabled)
johnny=0; tick(player:GetEntityID()); assert(not player.parts[2].enabled and player.parts[4].enabled)
tick(mirror:GetEntityID()); assert(not mirror.parts[1].enabled)
mirror.GetRecordID=function() return "Character.Judy" end
tick(mirror:GetEntityID()); assert(mirror.parts[1].enabled)
mirror.GetRecordID=function() return "Character.q110_tpp_female_v" end
tick(mirror:GetEntityID()); assert(not mirror.parts[1].enabled)
prop.GetItemID=function() return "Items.VRPortGrenade_frag" end
mirror.slots["AttachmentSlots.WeaponRight"]=gun
grenades=false; hidden=false; tick(mirror:GetEntityID())
assert(not mirror.parts[1].enabled and mirror.parts[6].enabled and gun.parts[1].enabled)
assert(not prop.parts[1].enabled and cfBelt.parts[1].enabled and sleeve.parts[1].enabled)
assert(player.parts[2].enabled and not player.parts[5].enabled and playerBelt.parts[1].enabled)
grenades=true; tick(); assert(prop.parts[1].enabled and mirror.parts[1].enabled)
entities[2]=nil; found=nil; gear.clear(); assert(prop.parts[1].enabled)
-- A reflected scene is optional. Looking down still hides the tactical belt on its own.
hidden=true; tick()
assert(not player.parts[2].enabled and not playerBelt.parts[1].enabled and outfit.parts[1].enabled)
hidden=false; tick()
assert(player.parts[2].enabled and player.parts[5].enabled and playerBelt.parts[1].enabled)
print("Tracked-body and mirror equipment visibility and restoration passed")
