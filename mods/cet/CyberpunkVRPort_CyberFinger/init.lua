-- A live choice hub, not merely looking at a generic interaction prompt, owns
-- conversation controls. Non-CyberFinger controllers never enter this path.
local controllers={}
local function visible(root)
  for _=1,32 do
    if not root then return true end
    if not root:IsVisible() or root:GetOpacity()<=0 then return false end
    root=root:GetParentWidget()
  end
  return false
end
registerForEvent("onInit",function()
  for _,entry in ipairs({{1,'WorldMapMenuGameController'},{2,'questLogV2GameController'},
    {2,'questLogGameController'},{3,'FrameSwitcherMenuGameController'}}) do
    local id,class=entry[1],entry[2]
    pcall(function()
      ObserveAfter(class,'OnInitialize',function(this) controllers[id]=this end)
      ObserveBefore(class,'OnUninitialize',function(this) if controllers[id]==this then controllers[id]=nil end end)
    end)
  end
end)
local function publishUi(enabled)
  if type(VRCyberFingerUiState)~='function' then return end
  local context,interaction=0,false
  if enabled then
    -- A live visible controller, refreshed each frame. Closing/reloading or
    -- losing the script expires the native lease rather than sticking in UI.
    for id=1,3 do
      pcall(function()
        local c=controllers[id];local root=c and c:GetRootWidget()
        if root and visible(root) then context=id end
      end)
    end
    pcall(function()
      local defs=GetAllBlackboardDefs().UIInteractions
      local bb=Game.GetBlackboardSystem():Get(defs)
      local hub=FromVariant(bb:GetVariant(defs.InteractionChoiceHub))
      interaction=hub and hub.choices and #hub.choices>0 or false
    end)
  end
  VRCyberFingerUiState(context,interaction)
end
local function computer(enabled)
  if type(VRComputerUpdate)~='function' then return end
  local root,controller
  if enabled then pcall(function()
    local p=Game.GetPlayer();if not p then return end
    local defs=GetAllBlackboardDefs();local boards=Game.GetBlackboardSystem()
    if boards:Get(defs.UI_System):GetBool(defs.UI_System.IsInMenu) then return end
    local psm=boards:GetLocalInstanced(p:GetEntityID(),defs.PlayerStateMachine)
    if not psm:GetBool(defs.PlayerStateMachine.IsUIZoomDevice) then return end
    local owner=Game.FindEntityByID(psm:GetEntityID(defs.PlayerStateMachine.UIZoomDeviceID))
    if not owner or not owner.cameraZoomActive then return end
    local c=owner:GetGameController()
    if not c or not c:IsA('ComputerInkGameController') or c:IsInteractivityBlocked() then return end
    local r=c:GetRootWidget()
    if r and visible(r) then root,controller=r,c end
  end) end
  local actions=VRComputerUpdate(root)
  if actions==3 and controller then pcall(function()
    -- Back is local to the computer. Exit (Menu) uses the game's separate
    -- StopDeviceControl input, so browsing never accidentally leaves the seat.
    local layout=controller:GetMainLayoutController()
    if layout then layout:HideWindow() end
    controller:GoToMenu(EComputerMenuType.MAIN)
  end) end
end
local commandWarned=false
local function command(enabled)
  if type(VRCyberFingerCommand)~='function' then return end
  local action=VRCyberFingerCommand() -- consume once, including while disabled
  if not enabled or action==0 then return end
  local ok,err=pcall(function()
    local player=Game.GetPlayer();if not player then return end
    if Game.GetSystemRequestsHandler():IsGamePaused() then return end
    local defs=GetAllBlackboardDefs().UI_System
    if Game.GetBlackboardSystem():Get(defs):GetBool(defs.IsInMenu) then return end
    if action==3 then
      -- Uses the same item/cooldown/phone-call checks as the game's OS action.
      player:VRActivateIconicCyberware()
      return
    end
    local layer=Game.VRInkLayer(CName.new('inkMenuLayer'))
    for _,controller in ipairs(layer and layer:GetGameControllers() or {}) do
      if controller:IsA('gameuiInGameMenuGameController') then
        if action==1 then controller:OpenShortcutMenu(CName.new('OpenInventoryMenu'))
        elseif action==2 then controller:SpawnMenuInstanceEvent(CName.new('OnOpenPauseMenu')) end
        return
      end
    end
  end)
  if not ok and not commandWarned then
    commandWarned=true;spdlog.error('[CyberFinger command] '..tostring(err))
  end
end
local function update()
  if type(VRCyberFingerConversation)~="function" or type(VRCyberFingerEnabled)~="function" then return end
  local enabled=VRCyberFingerEnabled();command(enabled);publishUi(enabled);computer(enabled)
  if not enabled then VRCyberFingerConversation(false);return end
  local active=false
  pcall(function()
    local defs=GetAllBlackboardDefs().UIInteractions
    local bb=Game.GetBlackboardSystem():Get(defs)
    local id=bb:GetInt(defs.ActiveChoiceHubID)
    local hubs=FromVariant(bb:GetVariant(defs.DialogChoiceHubs))
    for _,hub in ipairs(hubs and hubs.choiceHubs or {}) do
      if hub.id==id and #(hub.choices or {})>0 then active=true;break end
    end
  end)
  local action=VRCyberFingerConversation(active)
  local names={[1]="ChoiceScrollUp",[2]="ChoiceScrollDown",[3]="ChoiceApply"}
  if active and names[action] and type(VRInteractionAction)=="function" then
    VRInteractionAction(CName.new(names[action]))
  end
end
registerForEvent("onUpdate",update)
registerForEvent("onShutdown",function()
  controllers={};publishUi(false);computer(false)
  if type(VRCyberFingerConversation)=="function" then VRCyberFingerConversation(false) end
end)
return {update=update}
