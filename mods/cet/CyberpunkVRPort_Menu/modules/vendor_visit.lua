-- Scripted requests route before entering the native menu. A scene-driven visit also owns
-- a native menu instance: initialize its hidden hub, then close it through its own event queue.
-- Switching that instance to Idle before it is initialized strands the engine in an empty menu.
local V={}
local Availability=require("modules/availability")
local function vendorBoard(active)
  local defs=Game.GetAllBlackboardDefs().UI_Vendor
  local data=VendorData.new();data.isActive=active
  Game.GetBlackboardSystem():Get(defs):SetVariant(defs.VendorData,ToVariant(data),true)
end
function V.openSession(visit)
  if visit and not visit.boardOpen then vendorBoard(true);visit.boardOpen=true end
end
function V.closeSession(visit)
  if visit and visit.boardOpen then
    visit.boardOpen=false
    -- Same completion signal as VendorHubMenuGameController.OnUninitialize:
    -- quest-driven dialogue waits for this before offering the shop again.
    vendorBoard(false)
  end
end
-- The native shop's background and cursor belong to the menu layer, outside
-- VendorHubMenuGameController. Hiding only that controller leaves a full-screen
-- flash. Suppress the parent before OpenMenu, preserving native initialization.
function V.hidePresentation(shared)
  local scope=shared.vendorPresentation
  if not scope then
    local layer=Game.VRInkLayer(CName.new("inkMenuLayer"))
    local root=layer and layer:GetVirtualWindow()
    if not root then return end
    scope={root=root,opacity=root:GetOpacity()};shared.vendorPresentation=scope
  end
  scope.root:SetOpacity(.001)
end
function V.restorePresentation(shared)
  shared.vendorPresentationEarly=nil
  local scope=shared.vendorPresentation
  if not scope then return end
  shared.vendorPresentation=nil
  pcall(function() scope.root:SetOpacity(scope.opacity) end)
end
function V.cancelHandoff(shared)
  local visit=shared.vendorNativeHandoff
  if visit and shared.ripperSession==visit then shared.ripperSession=nil end
  shared.vendorNativeHandoff=nil
  V.restorePresentation(shared)
end
local function eligible(panel)
  if not panel then return nil end
  local ok,object=pcall(function() return Game.FindEntityByID(panel.data.entityID) end)
  if not ok or not object then return nil end
  local isRipper,ripper=pcall(function() return object:IsRipperdoc() end)
  local qs=Game.GetQuestsSystem()
  local vendorID=MarketSystem.GetVendorID(object)
  if not TDBID.IsValid(vendorID) then return nil end
  ripper=isRipper and ripper==true
  if ripper and vendorID==TweakDBID.new("Vendors.wat_lch_ripperdoc_01") and qs:GetFactStr("q001_ripperdoc_done")==0 then return nil end
  local tutorial=(qs:GetFactStr("mq048_active")>0 and qs:GetFactStr("mq048_done")==0) or qs:GetFactStr("ep1_standalone")>0
  if ripper and tutorial and qs:GetFactStr("ep1_ripperdoc_tutorial_seen")==0 then return nil end
  return object,ripper
end
function V.begin(shared,panel,scenario)
  if not Availability.enabled() then return false end
  local vendor,ripper=eligible(panel);if not vendor then return false end
  if shared.vendorTradeOpening and shared.vendorTradeOpening.entityHash==tostring(panel.data.entityID.hash) then return false end
  -- Older CET override layers can still call this shared function after a hot update.
  -- Let them reach the original OnEnterScenario instead of creating another visit or swallowing it.
  local existing=shared.vendorNativeHandoff
  if scenario and existing and tostring(existing.vendor:GetEntityID().hash)==tostring(vendor:GetEntityID().hash) then return false end
  if scenario then V.hidePresentation(shared) end
  local manager=VendorDataManager.new();manager:Initialize(Game.GetPlayer(),vendor:GetEntityID())
  local visit={vendor=vendor,manager=manager,panel=panel,scenario=scenario,pending=true,ripperdoc=ripper,
    started=shared.vendorClock or 0,nativeHandoff=scenario~=nil}
  shared.ripperSession=visit
  if scenario then
    shared.vendorNativeHandoff=visit
    local hub=shared.vendorLiveHub
    if hub and hub.entityHash==tostring(vendor:GetEntityID().hash) then V.captureHub(shared,hub.controller,panel) end
  end
  shared.vendorRouteCount=(shared.vendorRouteCount or 0)+1
  return true
end
function V.pollNative(shared)
  if type(VRMenuVendorPoll)~="function" or type(VRMenuVendorResolve)~="function" then return end
  local panel=VRMenuVendorPoll(Availability.enabled() and Game.GetPlayer()~=nil,Game.GetUISystem())
  if not panel then return end
  local isAppearance=NameToString(panel:GetClassName())=="MorphMenuUserData"
  local ok,accepted=pcall(function()
    if isAppearance then
      if shared.appearanceRouting then return shared.appearanceRouting.route(panel) end
      return false
    end
    return V.begin(shared,panel,nil)
  end)
  if ok and accepted==nil then return end
  local resolved=VRMenuVendorResolve(panel,ok and accepted==true)
  if not resolved and isAppearance and shared.appearanceRouting then shared.appearanceRouting.release() end
  if not resolved and shared.ripperSession and shared.ripperSession.panel==panel then shared.ripperSession=nil end
  if not ok then print("[VRMenu] vendor routing failed: "..tostring(accepted)) end
end
function V.bind(shared)
  shared.vendorRouting=V
  if not shared.vendorEarlyPresentationBound then
    shared.vendorEarlyPresentationBound=true
    Override("MenuScenario_Idle","OnLeaveScenario",function(self,nextScenario,wrapped)
      -- Idle.ShowMenus(true) reveals the common background before Vendor.OnEnter.
      if Availability.enabled() and NameToString(nextScenario)=="MenuScenario_Vendor" then
        shared.vendorRouting.hidePresentation(shared)
        shared.vendorPresentationEarly=true
      end
      return wrapped(nextScenario)
    end)
    ObserveAfter("MenuScenario_Vendor","OnEnterScenario",function()
      -- Tutorial and explicitly requested native shops retain normal rendering.
      if shared.vendorPresentationEarly and not shared.vendorNativeHandoff then
        shared.vendorRouting.restorePresentation(shared)
      end
    end)
  end
  local pending=shared.vendorNativeHandoff
  if pending then
    -- A hot reload replaces the menu clock. Keep an in-flight native close
    -- bounded by the new clock instead of waiting for the previous session time.
    pending.started=shared.vendorClock or 0
    if pending.controller then pending.hubReadyAt=pending.started end
  end
  if not shared.vendorPresentationLifecycleBound then
    shared.vendorPresentationLifecycleBound=true
    ObserveBefore("MenuScenario_Vendor","OnLeaveScenario",function(_,nextScenario)
      -- A different native page taking over must never inherit our hidden layer.
      if shared.vendorNativeHandoff and NameToString(nextScenario)~="MenuScenario_Idle" then
        shared.vendorRouting.cancelHandoff(shared)
      end
    end)
  end
  if shared.vendorScenarioRoutingVersion~=4 then
    local hadLifecycle=shared.vendorScenarioRoutingVersion~=nil and shared.vendorScenarioRoutingVersion>=2
    shared.vendorScenarioRoutingVersion=4
    Override("UISystem","RequestVendorMenu",function(self,data,scenario,wrapped)
      if shared.vendorRouting.begin(shared,data,nil) then return end
      if not scenario or NameToString(scenario)=="None" or NameToString(scenario)=="" then scenario=CName.new("MenuScenario_Vendor") end
      return wrapped(data,scenario)
    end)
    Override("MenuScenario_Vendor","OnEnterScenario",function(self,previous,userData,wrapped)
      local panel=userData
      if panel and not panel.data then panel=panel.vendorData end
      shared.vendorRouting.begin(shared,panel,self)
      return wrapped(previous,userData)
    end)
    Override("MenuScenario_Vendor","OnSwitchToVendor",function(self,userData,wrapped)
      if shared.vendorNativeHandoff then return true end
      return wrapped(userData)
    end)
    Override("MenuScenario_Vendor","OnSwitchToRipperDoc",function(self,userData,wrapped)
      if shared.vendorTradeOpening and shared.vendorTradeOpening.selecting then return true end
      if shared.vendorNativeHandoff then return true end
      if userData and shared.vendorRouting.begin(shared,userData.vendorData,self) then return true end
      return wrapped(userData)
    end)
    if not hadLifecycle then
      ObserveBefore("VendorHubMenuGameController","OnInitialize",function(self)
        if shared.vendorNativeHandoff then shared.vendorRouting.hideHub(self) end
      end)
      ObserveAfter("VendorHubMenuGameController","OnSetUserData",function(self,data)
        shared.vendorRouting.captureHub(shared,self,data)
      end)
      ObserveBefore("VendorHubMenuGameController","OnUninitialize",function(self)
        local visit=shared.vendorNativeHandoff
        if visit and visit.closeQueued then visit.hubClosed=true end
      end)
    end
  end
  if not shared.vendorHubCacheBound then
    shared.vendorHubCacheBound=true
    ObserveBefore("VendorHubMenuGameController","OnUninitialize",function()
      shared.vendorLiveHub=nil
    end)
  end
  if shared.vendorRoutingBound then return end
  shared.vendorRoutingBound=true
  Override("VendorHubMenuGameController","SetupMenuTabs",function(self,tutorialFinished,wrapped)
    wrapped(tutorialFinished)
    local trade=shared.vendorTradeOpening
    if trade and self.VendorDataManager and tostring(self.VendorDataManager:GetVendorInstance():GetEntityID().hash)==trade.entityHash then
      self.tabRoot:Toggle(0)
      self:OnValueChanged(self.tabRoot,0)
    end
  end)
  ObserveAfter("MenuScenario_Vendor","OnSwitchToVendor",function()
    shared.vendorTradeOpening=nil
  end)
  ObserveBefore("MenuScenario_Vendor","OnLeaveScenario",function()
    shared.vendorTradeOpening=nil
  end)
end
function V.hideHub(controller)
  local root=controller and controller:GetRootWidget()
  -- Invisible roots are not adopted by the menu instance and cannot deliver its close event.
  -- Keep lifecycle/layout active while suppressing the native shell's pixels.
  if root then root:SetOpacity(.001);root:SetVisible(true) end
end
function V.captureHub(shared,controller,panel)
  local visit=shared.vendorNativeHandoff
  if not panel or not panel.data or not controller then return end
  shared.vendorLiveHub={controller=controller,entityHash=tostring(panel.data.entityID.hash)}
  if not visit then return end
  if tostring(panel.data.entityID.hash)~=tostring(visit.vendor:GetEntityID().hash) then return end
  visit.controller=controller;visit.hubReadyAt=shared.vendorClock or 0
  V.hideHub(controller)
end
function V.advance(shared,visit,now)
  local defs=Game.GetAllBlackboardDefs().UI_System
  local inMenu=Game.GetBlackboardSystem():Get(defs):GetBool(defs.IsInMenu)
  if visit.nativeHandoff then
    V.hidePresentation(shared)
    if visit.controller and not visit.hubClosed then V.hideHub(visit.controller) end
    -- Let the engine finish adopting the new menu instance before asking it to close.
    if visit.controller and not visit.closeQueued and now-visit.hubReadyAt>=.08 then
      visit.closeQueued=true;visit.closeAt=now
      visit.scenario:OnVendorClose()
    end
    if visit.hubClosed and not inMenu then
      shared.vendorNativeHandoff=nil;visit.nativeHandoff=false;visit.scenario=nil
      V.restorePresentation(shared)
    elseif now-visit.started>4 then
      -- Keep a functional native shop if a future game build changes its close protocol.
      -- Never leave a hidden hub or wait forever for a world widget in menu mode.
      shared.vendorNativeHandoff=nil;shared.ripperSession=nil;visit.pending=false
      V.restorePresentation(shared)
      if visit.controller then
        local root=visit.controller:GetRootWidget();if root then root:SetOpacity(1);root:SetVisible(true) end
        local data=VendorUserData.new();data.vendorData=visit.panel
        visit.scenario:OnSwitchToVendor(data)
      else visit.scenario:OpenMenu(CName.new("vendor_hub_menu"),visit.panel,ScreenDisplayContext.Vendor) end
      print("[VRMenu] vendor handoff timed out; restored native shop")
      return false
    else return false end
  end
  return not inMenu
end
function V.current(shared)
  local visit=shared.ripperSession
  if not visit or not visit.active or not visit.vendor or not visit.manager or not Game.GetPlayer() then return nil end
  local p,v=Game.GetPlayer():GetWorldPosition(),visit.vendor:GetWorldPosition()
  if (p.x-v.x)^2+(p.y-v.y)^2+(p.z-v.z)^2>64 then return nil end
  return visit
end
function V.trade(shared,visit)
  if not visit or not visit.vendor then return false end
  local panel=visit.panel
  if not panel then
    panel=VendorPanelData.new();local data=VendorData.new()
    data.entityID=visit.vendor:GetEntityID();data.isActive=true;panel.data=data
  end
  shared.vendorTradeOpening={entityHash=tostring(visit.vendor:GetEntityID().hash),selecting=true}
  Game.GetUISystem():RequestVendorMenu(panel)
  return true
end
return V
