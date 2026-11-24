-- A live choice hub, not merely looking at a generic interaction prompt, owns
-- conversation controls. Non-CyberFinger controllers never enter this path.
local controllers={}
local shardContext = 4
local cameraContext = 5
local publishedShard
local meleeToggle=require('melee_toggle')
local hudHints=require('hud_hints')
local function visible(root)
  for _=1,32 do
    if not root then return true end
    if not root:IsVisible() or root:GetOpacity()<=0 then return false end
    root=root:GetParentWidget()
  end
  return false
end
registerForEvent("onInit",function()
  hudHints.init()
  for _,entry in ipairs({{1,'WorldMapMenuGameController'},{2,'questLogV2GameController'},
    {2,'questLogGameController'},{3,'FrameSwitcherMenuGameController'},{shardContext,'ShardNotificationController'}}) do
    local id,class=entry[1],entry[2]
    pcall(function()
      ObserveAfter(class,'OnInitialize',function(this) controllers[id]=this end)
      local function forget(this)
        if controllers[id]==this then controllers[id]=nil end
      end
      ObserveBefore(class,'OnUninitialize',forget)
      if id==shardContext then ObserveBefore(class,'Close',forget) end
    end)
  end
end)
local function controllingCamera()
  local systems=Game.GetScriptableSystemsContainer()
  local takeover=systems and systems:Get(CName.new('TakeOverControlSystem'))
  local device=takeover and takeover:GetControlledObject()
  return device and device:IsA('SurveillanceCamera')
end
local function publishUi(enabled)
  if type(VRCyberFingerUiState)~='function' then return end
  local context,interaction=0,false
  if enabled then
    -- A live visible controller, refreshed each frame. Closing/reloading or
    -- losing the script expires the native lease rather than sticking in UI.
    for id=1,shardContext do
      pcall(function()
        local c=controllers[id];local root=c and c:GetRootWidget()
        if root and visible(root) then context=id end
      end)
    end
    if context==0 then
      pcall(function()
        if not controllingCamera() then return end
        local ui=GetAllBlackboardDefs().UI_System
        if not Game.GetBlackboardSystem():Get(ui):GetBool(ui.IsInMenu) then context=cameraContext end
      end)
    end
    pcall(function()
      local defs=GetAllBlackboardDefs().UIInteractions
      local bb=Game.GetBlackboardSystem():Get(defs)
      local hub=FromVariant(bb:GetVariant(defs.InteractionChoiceHub))
      -- The blackboard retains the last choices after the prompt disappears.
      -- Its active flag is the game's visibility gate, also used by the weapon
      -- wheel when deciding whether a world interaction owns the button.
      interaction=hub and hub.active and hub.choices and #hub.choices>0 or false
    end)
  end
  VRCyberFingerUiState(context,interaction)
  publishedShard = context==shardContext and controllers[shardContext] or nil
  return interaction
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
    if action==7 then
      -- The reader can pause gameplay or sit over the journal. Only dismiss
      -- the same visible popup that owned Grip when we published its context.
      local reader=controllers[shardContext]
      if reader and reader==publishedShard then
        local root=reader:GetRootWidget()
        if root and visible(root) then reader:Close() end
      end
      return
    end
    local player=Game.GetPlayer();if not player then return end
    if Game.GetSystemRequestsHandler():IsGamePaused() then return end
    local defs=GetAllBlackboardDefs().UI_System
    if Game.GetBlackboardSystem():Get(defs):GetBool(defs.IsInMenu) then return end
    -- The player may have taken control since OpenXR queued the shortcut.
    if (action==1 or action==3) and controllingCamera() then return end
    if action==5 then
      local holster=GetMod('CyberpunkVRPort_Holster')
      if holster and holster.toggleWornGear then holster.toggleWornGear() end
      return
    end
    if action==4 or action==6 then meleeToggle.toggle(player,action==6);return end
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
local function update(dt)
  if type(VRCyberFingerConversation)~="function" or type(VRCyberFingerEnabled)~="function" then return end
  local enabled=VRCyberFingerEnabled();command(enabled)
  local interaction=publishUi(enabled);computer(enabled)
  pcall(function() meleeToggle.observe(enabled and Game.GetPlayer() or nil) end)
  if not enabled then VRCyberFingerConversation(false);hudHints.update(dt,false);return end
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
  hudHints.update(dt,true,active or interaction)
  local names={[1]="ChoiceScrollUp",[2]="ChoiceScrollDown",[3]="ChoiceApply"}
  if active and names[action] and type(VRInteractionAction)=="function" then
    VRInteractionAction(CName.new(names[action]))
  end
end
registerForEvent("onUpdate",update)
registerForEvent("onShutdown",function()
  hudHints.update(0,false)
  controllers={};publishUi(false);computer(false)
  if type(VRCyberFingerConversation)=="function" then VRCyberFingerConversation(false) end
end)
return {update=update}
