-- User preference belongs to the CET mod directory, independently of saves and equipment.
local P={characterModel=false,attributeUpgradeConfirmed=false}
local file=io.open("menu_preferences.json","r")
if file then
  local text=file:read("*a");file:close()
  P.characterModel=text:match('"characterModel"%s*:%s*(%a+)')=="true"
  P.attributeUpgradeConfirmed=text:match('"attributeUpgradeConfirmed"%s*:%s*(%a+)')=="true"
end
local function save(characterModel)
  local out=io.open("menu_preferences.json","w")
  if not out then return false end
  local ok=out:write('{"characterModel":'..tostring(characterModel)..',"attributeUpgradeConfirmed":'..tostring(P.attributeUpgradeConfirmed)..'}\n')
  local closed=out:close()
  return ok~=nil and closed==true
end
function P.setCharacterModel(enabled)
  if not save(enabled==true) then return false end
  P.characterModel=enabled==true
  return true
end
function P.confirmAttributeUpgrade()
  P.attributeUpgradeConfirmed=true
  if not save(P.characterModel) then print("[VRMenu] Could not save attribute upgrade preference") end
end
return P
