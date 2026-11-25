local moduleFile, placementsFile = arg[1], arg[2]
local hooks, spawned, deleted, shown, inVR = {}, 0, 0, false, true
local node = {}
node.__index = node
local function new(parent, text)
  return setmetatable({parent=parent, text=text, opacity=1, visible=true},node)
end
function node:GetOpacity() return self.opacity end
function node:SetOpacity(v) self.opacity=v end
function node:IsVisible() return self.visible end
function node:GetParentWidget() return self.parent end
function node:GetController() return self.controller end
function node:GetText() return self.text end
function node:SetText(value) self.text=value end
function node:IsA(t) return t=="inkTextWidget" and self.text~=nil end
function node:SetVerticalAlignment() end
function node:GetDesiredSize() return {Y=50} end
function node:RemoveAllChildren() end
local texts={}
local function drawText(_,_,_,_,_,value)
  local n=new(nil,value); n.wrappingInfo={}; texts[#texts+1]=n; return n
end
package.loaded["modules/ink"]={rect=function() end,text=drawText,size=function() end}
local heading=1
package.loaded["modules/space"]={
  view=function() return {p={heading,0,1},f={0,1,0},r={1,0,0}} end,
  add=function(p) return {p[1],1,1} end, mul=function() end, dot=function() return 1 end,
  transformFor=function(c) return {Position=c,GetOrientation=function() end} end
}
CName={new=function(v) return v end}
inkWidgetRef={Get=function(w) return w end}
textVerticalAlignment={Top=1}
WorldPosition={ToVector4=function(v) return v end}
function ObserveAfter(c,e,f) hooks[c..e]=f end
ObserveBefore=ObserveAfter
local ready=false
function VRMenuSurfaceReady() return ready end
local world=new()
local mesh={Toggle=function(_,v) shown=v end}
local comp={GetWidget=function() return world end}
local entity={FindComponentByName=function(_,k) return k=="display" and mesh or comp end}
local anchor
Game={
  VRPhysicalOpticsEnabled=function() return inVR end,
  VRSpawnTemplate=function(_,_,p) spawned=spawned+1;anchor=p[1];return {hash=spawned} end,
  FindEntityByID=function() return entity end,
  VRSpawnDelete=function() deleted=deleted+1 end
}
local win=new()
local src=new(win)
local disp={GetRootWidget=function() return src end,m_title=new(src,"New Perk Tree Unlocked"),
  m_message=new(src,"Relic"),m_inputHint=new(src,"Continue")}
local cf=true
Game.GetPlayer=function() return {PlayerLastUsedPad=function() return cf end} end
VRCyberFingerHint=function(action) return cf and action=="proceed" and "Either Trigger" or "" end
disp.m_inputHint.controller={IsA=function(_,kind) return kind=="inkInputDisplayController" end,GetInputAction=function() return "proceed" end}
local cards=dofile(moduleFile)
cards.init()
local data=hooks.TutorialPopupDisplayControllerSetData
local close=hooks.TutorialPopupGameControllerCloseTutorial
data(disp);cards.update(.1)
assert(spawned==1 and src.opacity==1 and not shown,"hid native before world surface was ready")
ready=true;heading=3;cards.update(.1);cards.update(.1)
assert(shown and src.opacity==0 and anchor==1 and spawned==1,"tutorial did not keep its opening position")
assert(texts[3].text=="Continue  [Either Trigger]","tutorial did not use the native action")
cf=false;cards.update(.3)
assert(texts[3].text=="Continue","tutorial kept controller hint after input switched")
close();assert(deleted==1 and src.opacity==1,"dismissal did not restore native card")
disp.m_image=new(src);data(disp);cards.update(.2);assert(spawned==1,"suppressed illustrated tutorial")
close();disp.m_image=nil;ready=false;data(disp);cards.update(.2);cards.update(4)
assert(deleted==2 and src.opacity==1,"failed surface hid the game card")
inVR=false;data(disp);cards.update(.1);assert(spawned==2,"started a panel in flat mode")
local places=dofile(placementsFile)
local sword=places.get("hip_l",false,true,true)
assert(sword.name=="hip_l" and sword.knucklesUp and not places.get("hip_l",false,true,false).knucklesUp)
assert(places.sword("Wea_Katana") and not places.sword("Wea_Axe"))
assert(places.knucklesUp({i=1,j=0}) and not places.knucklesUp({i=0,j=0}))
