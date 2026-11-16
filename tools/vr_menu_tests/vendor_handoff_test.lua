-- No game state writes: exercise the scene-driven lifecycle and old CET override layers.
local rootPath=... or "."
package.preload["modules/availability"]=function()return dofile(rootPath.."/mods/cet/CyberpunkVRPort_Menu/modules/availability.lua")end
local V=dofile(rootPath.."/mods/cet/CyberpunkVRPort_Menu/modules/vendor_visit.lua")
local object={GetEntityID=function() return {hash="fred"} end,IsRipperdoc=function() return false end}
local inMenu=true
local menuLayer={opacity=.8,GetOpacity=function(s) return s.opacity end,SetOpacity=function(s,v) s.opacity=v end}
CName={new=function(v) return v end};NameToString=function(v)return v end
Game={GetPlayer=function() return {} end,GetUISystem=function() return {} end,FindEntityByID=function() return object end,
  VRInkLayer=function() return {GetVirtualWindow=function() return menuLayer end} end,
  GetQuestsSystem=function() return {GetFactStr=function() return 1 end} end,
  GetAllBlackboardDefs=function() return {UI_System={IsInMenu="menu"}} end,
  GetBlackboardSystem=function() return {Get=function() return {GetBool=function() return inMenu end} end} end}
MarketSystem={GetVendorID=function() return "fredVendor" end}
TDBID={IsValid=function() return true end};TweakDBID={new=function(s) return s end}
VendorDataManager={new=function() return {Initialize=function() end} end}
VendorUserData={new=function() return {} end}
local panel={data={entityID={hash="fred"}},GetClassName=function()return 'questVendorPanelData'end}
local shared={vendorClock=1}
local closed,restored=0,0
local scenario={OnVendorClose=function() closed=closed+1;shared.ripperSession.hubClosed=true;inMenu=false end,
  OnSwitchToVendor=function() restored=restored+1 end}
local original=0
local function previousOverride()
  if V.begin(shared,panel,scenario) then return end
  original=original+1
end
assert(V.begin(shared,panel,scenario))
assert(menuLayer.opacity==.001,"suppress background/cursor before native OpenMenu")
local visit=shared.ripperSession
previousOverride()
assert(original==1 and shared.ripperSession==visit,"a stale override must reach native hub initialization")
local root={SetOpacity=function(self,v) self.opacity=v end,SetVisible=function(self,v) self.visible=v end}
local controller={GetRootWidget=function() return root end}
V.captureHub(shared,controller,panel)
assert(root.visible and root.opacity==.001,"hub layout/lifecycle must remain enabled")
assert(not V.advance(shared,visit,1.04) and closed==0,"do not close before native initialization settles")
assert(not V.advance(shared,visit,1.10) and closed==1)
assert(V.advance(shared,visit,1.20) and closed==1 and not shared.vendorNativeHandoff,"world UI requires native teardown and menu-mode exit")
assert(menuLayer.opacity==.8 and not shared.vendorPresentation,"restore original layer opacity after teardown")
inMenu=true;shared.vendorClock=2
assert(V.begin(shared,panel,scenario));visit=shared.ripperSession
V.captureHub(shared,controller,panel);visit.closeQueued=true
assert(not V.advance(shared,visit,7) and not shared.ripperSession and restored==1)
assert(root.visible and root.opacity==1,"timeout restores a usable native shop")
assert(menuLayer.opacity==.8 and not shared.vendorPresentation,"timeout must also restore the parent layer")
V.hidePresentation(shared);V.hidePresentation(shared);V.restorePresentation(shared)
assert(menuLayer.opacity==.8,"repeated suppression must not replace the original opacity")
shared.vendorClock=8;assert(V.begin(shared,panel,scenario));V.cancelHandoff(shared)
assert(menuLayer.opacity==.8 and not shared.vendorNativeHandoff and not shared.ripperSession,"another native page must cancel and restore the handoff")
Override=function() end;ObserveBefore=function() end;ObserveAfter=function() end
inMenu=true;shared.vendorClock=1000;assert(V.begin(shared,panel,scenario))
visit=shared.ripperSession;V.captureHub(shared,controller,panel)
shared.vendorClock=0;V.bind(shared)
local beforeClose=closed
assert(not V.advance(shared,visit,.1) and closed==beforeClose+1,"hot reload must not wait on the old menu clock")
assert(V.advance(shared,visit,.2) and menuLayer.opacity==.8,"handoff completes and restores after hot reload")
local ack
VRMenuVendorPoll=function(enabled) assert(enabled);return panel end
VRMenuVendorResolve=function(value,accepted) assert(value==panel);ack=accepted;return true end
shared.ripperSession=nil;V.pollNative(shared)
assert(ack and shared.ripperSession.pending and not shared.ripperSession.nativeHandoff,"native queue request must open world visit without a native menu")
shared.ripperSession=nil
local find=Game.FindEntityByID;Game.FindEntityByID=function() return nil end
V.pollNative(shared)
assert(ack==false and not shared.ripperSession,"unrecognized vendors return the original request to the engine")
Game.FindEntityByID=find
VRMenuVendorResolve=function() return false end
V.pollNative(shared)
assert(not shared.ripperSession,"an expired acknowledgement must not also open a world shop")
local morph={GetClassName=function()return 'MorphMenuUserData'end}
local answer=nil;local releases=0;ack=nil
shared.appearanceRouting={route=function(p)assert(p==morph);return answer end,release=function()releases=releases+1 end}
VRMenuVendorPoll=function()return morph end
VRMenuVendorResolve=function(p,a)assert(p==morph);ack=a;return true end
V.pollNative(shared);assert(ack==nil,'preparing mirror request must remain queued')
answer=true;V.pollNative(shared);assert(ack==true,'ready mirror uses world route')
VRMenuVendorResolve=function()return false end;V.pollNative(shared);assert(releases==1,'expired mirror lease must be cleaned up')
VRMenuVendorPoll=nil;VRMenuVendorResolve=nil
local notifications={}
VendorData={new=function() return {} end};ToVariant=function(v) return v end
Game.GetAllBlackboardDefs=function() return {UI_Vendor={VendorData="vendor"}} end
Game.GetBlackboardSystem=function() return {Get=function() return {SetVariant=function(_,key,data,signal)
  assert(key=="vendor" and signal==true);notifications[#notifications+1]=data.isActive
end} end} end
local worldVisit={}
V.openSession(worldVisit);V.openSession(worldVisit);V.closeSession(worldVisit);V.closeSession(worldVisit)
assert(#notifications==2 and notifications[1] and notifications[2]==false,"one native open/close notification per visit restores vendor dialogue")
print("PASS: native hub lifecycle, duplicate override passthrough, menu exit gate and timeout recovery")
