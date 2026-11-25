local events, calls, actions = {}, {}, {}
package.path=assert(arg[1]:match('^(.*)[/\\]'))..'/?.lua;'..package.path
local enabled, nextAction, activeId = true, 0, 7
local hubs = {choiceHubs={}}
local prompt={active=false,choices={}}
local uiState,uiPrompt
function VRCyberFingerUiState(context,interaction) uiState,uiPrompt=context,interaction end
local initialized,uninitialized,closing={},{},{}
function ObserveAfter(class,method,fn) initialized[class]=fn end
function ObserveBefore(class,method,fn)
  if method=='OnUninitialize' then uninitialized[class]=fn
  elseif method=='Close' then closing[class]=fn end
end
function registerForEvent(name,fn) events[name]=fn end
function VRCyberFingerEnabled() return enabled end
function VRCyberFingerConversation(active) calls[#calls+1]=active;return nextAction end
function VRInteractionAction(name) actions[#actions+1]=name end
CName={new=function(s) return s end}
local defs={ActiveChoiceHubID='id',DialogChoiceHubs='hubs',InteractionChoiceHub='prompt'}
function GetAllBlackboardDefs() return {UIInteractions=defs} end
function FromVariant(v) return v end
local bb={GetInt=function() return activeId end,GetVariant=function(_,key) return key=='prompt' and prompt or hubs end}
Game={GetBlackboardSystem=function() return {Get=function() return bb end} end}
dofile(assert(arg[1]))
events.onInit()
events.onUpdate();assert(calls[#calls]==false,'ambient prompt captured controls')
prompt.choices={1};events.onUpdate()
assert(not uiPrompt and not calls[#calls],'cached inactive choices revealed HUD or captured interaction buttons')
prompt.active=true;events.onUpdate()
assert(uiPrompt and not calls[#calls],'prompt was mistaken for conversation')
prompt.active=false;events.onUpdate();assert(not uiPrompt,'expired prompt retained HUD reveal')
local shown=true
local root={IsVisible=function() return shown end,GetOpacity=function() return 1 end,GetParentWidget=function() return nil end}
local controller={GetRootWidget=function() return root end}
for _,row in ipairs({{'WorldMapMenuGameController',1},{'questLogV2GameController',2},{'FrameSwitcherMenuGameController',3},{'ShardNotificationController',4}}) do
  initialized[row[1]](controller);events.onUpdate();assert(uiState==row[2],'menu context not published')
  shown=false;events.onUpdate();assert(uiState==0,'invisible controller retained context');shown=true
  uninitialized[row[1]](controller);events.onUpdate();assert(uiState==0,'closed context retained')
end
initialized.questLogV2GameController(controller)
initialized.ShardNotificationController(controller)
events.onUpdate();assert(uiState==4,'journal underneath a shard took the close button')
enabled=false;events.onUpdate();assert(uiState==0,'shard captured a non-CF controller')
enabled=true;uninitialized.ShardNotificationController(controller)
events.onUpdate();assert(uiState==2,'closing the shard lost the journal underneath')
uninitialized.questLogV2GameController(controller)
prompt.choices={}
hubs={choiceHubs={{id=7,choices={1,2}}}}
for i,name in ipairs({'ChoiceScrollUp','ChoiceScrollDown','ChoiceApply'}) do
  nextAction=i;events.onUpdate();assert(calls[#calls] and actions[#actions]==name,'choice action missing')
end
local count=#actions
activeId=8;events.onUpdate();assert(not calls[#calls] and #actions==count,'inactive hub received input')
activeId=7;hubs.choiceHubs[1].choices={};events.onUpdate();assert(not calls[#calls],'empty hub captured input')
hubs.choiceHubs[1].choices={1};enabled=false;events.onUpdate()
assert(not calls[#calls] and #actions==count,'another controller was changed')
-- Computer ownership follows the zoomed entity, never gaze or all world UIs.
local computerRoot,computerResult,backCalls,mainCalls=nil,1,0,0
function VRComputerUpdate(r) computerRoot=r;return r and computerResult or 0 end
local owner={cameraZoomActive=true}
local computerController={IsA=function(_,n) return n=='ComputerInkGameController' end,
  IsInteractivityBlocked=function() return false end,GetRootWidget=function() return root end,
  GetMainLayoutController=function() return {HideWindow=function() backCalls=backCalls+1 end} end,
  GoToMenu=function() mainCalls=mainCalls+1 end}
function owner:GetGameController() return computerController end
Game.GetPlayer=function() return {GetEntityID=function() return 123 end} end
Game.FindEntityByID=function(id) assert(id==321);return owner end
EComputerMenuType={MAIN=0}
local zoom=true
GetAllBlackboardDefs=function() return {UIInteractions=defs,UI_System={IsInMenu='menu'},
  PlayerStateMachine={IsUIZoomDevice='zoom',UIZoomDeviceID='owner'}} end
Game.GetBlackboardSystem=function() return {
  Get=function(_,d) if d==defs then return bb end return {GetBool=function() return false end} end,
  GetLocalInstanced=function() return {GetBool=function() return zoom end,GetEntityID=function() return 321 end} end} end
enabled=true;events.onUpdate();assert(computerRoot==root,'zoomed computer not acquired')
computerResult=3;events.onUpdate();assert(backCalls==1 and mainCalls==1,'Grip did not navigate within computer')
zoom=false;events.onUpdate();assert(not computerRoot and backCalls==1,'leaving computer retained ownership')
zoom=true;enabled=false;events.onUpdate();assert(not computerRoot,'computer changed non-CF behavior')
-- Named commands reach the intended game function, independent of Start/Back.
local menuCommand, inventoryCalls, pauseCalls, cyberwareCalls=0,0,0,0
local paused,inMenu=false,false
function VRCyberFingerCommand() local action=menuCommand;menuCommand=0;return action end
local player={VRActivateIconicCyberware=function() cyberwareCalls=cyberwareCalls+1 end}
Game.GetPlayer=function() return player end
Game.GetSystemRequestsHandler=function() return {IsGamePaused=function() return paused end} end
Game.GetBlackboardSystem=function() return {Get=function(_,d)
  if d==defs then return bb end return {GetBool=function() return inMenu end}
end} end
local menuController={IsA=function(_,name) return name=='gameuiInGameMenuGameController' end,
  OpenShortcutMenu=function(_,name) assert(name=='OpenInventoryMenu');inventoryCalls=inventoryCalls+1 end,
  SpawnMenuInstanceEvent=function(_,name) assert(name=='OnOpenPauseMenu');pauseCalls=pauseCalls+1 end}
Game.VRInkLayer=function() return {GetGameControllers=function() return {menuController} end} end
spdlog={error=function(message) error(message) end}
local device
Game.GetScriptableSystemsContainer=function() return {Get=function(_,name)
  assert(name=='TakeOverControlSystem')
  return {GetControlledObject=function() return device end}
end} end
enabled=true
menuCommand=1;events.onUpdate();events.onUpdate();assert(inventoryCalls==1 and pauseCalls==0,'inventory became pause or repeated')
menuCommand=2;events.onUpdate();assert(pauseCalls==1,'black hold did not open native pause')
menuCommand=3;events.onUpdate();assert(cyberwareCalls==1,'right Menu hold did not activate OS cyberware')
local gearToggles=0
GetMod=function(name) if name=='CyberpunkVRPort_Holster' then return {toggleWornGear=function() gearToggles=gearToggles+1 end} end end
menuCommand=5;events.onUpdate();events.onUpdate();assert(gearToggles==1,'worn gear toggle missed or repeated')
inMenu=true;menuCommand=5;events.onUpdate();inMenu=false;assert(gearToggles==1,'menu changed worn gear')
paused=true;menuCommand=3;events.onUpdate();assert(cyberwareCalls==1,'OS action leaked through pause');paused=false
inMenu=true;menuCommand=1;events.onUpdate();assert(inventoryCalls==1,'command replaced an active game menu');inMenu=false
enabled=false;menuCommand=1;events.onUpdate();enabled=true;events.onUpdate();assert(inventoryCalls==1,'command leaked across controller change')

device={IsA=function(_,class) return class=='SurveillanceCamera' end}
menuCommand=1;events.onUpdate()
assert(uiState==5 and inventoryCalls==1,'camera takeover allowed a queued inventory tap')
menuCommand=3;events.onUpdate();assert(cyberwareCalls==1,'camera takeover allowed OS cyberware')
inMenu=true;events.onUpdate();assert(uiState==0,'camera claimed a native menu')
inMenu=false;enabled=false;events.onUpdate();assert(uiState==0,'camera claimed another controller')
enabled=true;initialized.ShardNotificationController(controller);events.onUpdate()
assert(uiState==4,'camera took priority over the shard reader')
uninitialized.ShardNotificationController(controller)
device={IsA=function(_,class) return class=='SecurityTurret' end}
events.onUpdate();assert(uiState==0,'another controlled device claimed the camera shortcut')
device=nil;events.onUpdate();assert(uiState==0,'camera context persisted after exit')

local closeCount=0
local function newShard()
  return {GetRootWidget=function() return root end, Close=function(self)
    closing.ShardNotificationController(self)
    closeCount=closeCount+1
  end}
end
paused,inMenu=true,true
initialized.questLogV2GameController(controller)
local first,second=newShard(),newShard()
initialized.ShardNotificationController(first);events.onUpdate()
initialized.ShardNotificationController(second);menuCommand=7;events.onUpdate()
assert(closeCount==0 and uiState==4,'a queued press dismissed a replacement shard')
shown=false;menuCommand=7;events.onUpdate()
assert(closeCount==0,'a hidden reader received Close')
shown=true;events.onUpdate()
menuCommand=7;events.onUpdate()
assert(closeCount==1 and uiState==2,'Grip failed to dismiss the paused reader over the journal')
menuCommand=7;events.onUpdate()
assert(closeCount==1 and uiState==2,'a stale request dismissed another UI')
initialized.ShardNotificationController(first);events.onUpdate()
enabled=false;menuCommand=7;events.onUpdate();enabled=true
assert(closeCount==1,'a disabled controller closed the reader')

enabled=true;Game.GetBlackboardSystem=function() error('game unloading') end
events.onUpdate();assert(not calls[#calls],'unloading retained dialogue capture')
events.onShutdown();assert(not calls[#calls],'shutdown retained dialogue capture')
print('CyberFinger conversation bridge passed')
