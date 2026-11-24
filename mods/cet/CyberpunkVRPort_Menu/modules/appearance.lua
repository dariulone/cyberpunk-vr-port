-- The mirror's native request is retained by the same queue route as vendors.
-- Only options and the mirror double are used; no native menu/scenes are opened.
local A={}
local shared
local function session() return shared and shared.appearanceSession end
local function system() return Game.GetCharacterCustomizationSystem() end
local function identity(e) return tostring(e:GetEntityID().hash) end
local function loc(key)
  if not key or key=="" then return "" end
  local text=GetLocalizedTextByKey(key)
  return text and text~="" and text~=key and text or ""
end
local function mirrorDouble()
  local player=Game.GetPlayer()
  if not player or Game.GetQuestsSystem():GetFactStr("johnny_mirror")>0 then return nil end
  local wanted=NameToString(player:GetResolvedGenderName())=="Female" and "Character.q110_tpp_female_v" or "Character.q110_tpp_male_v"
  local query=TargetSearchQuery.new();query.maxDistance=3;query.testedSet=TargetingSet.Complete
  local _,parts=Game.GetTargetingSystem():GetTargetParts(player,query)
  for _,part in ipairs(parts or {}) do
    local e=part:GetComponent():GetEntity()
    if e and e:IsA("NPCPuppet") and TDBID.ToStringDEBUG(e:GetRecordID())==wanted then return e end
  end
end
local function leaseSlot(s,slot,item)
  local ts=Game.GetTransactionSystem();local old=ts:GetItemInSlot(s.puppet,slot)
  local lease={slot=slot,old=old and old:GetItemID(),item=item,had=item and ts:HasItem(s.puppet,item)}
  s.leases[#s.leases+1]=lease
  -- Destroy the detached visual entity, not the inventory item. Keeping its entity alive leaves external
  -- garment meshes attached to the mirror NPC even though GetItemInSlot already reports an empty slot.
  if old then ts:RemoveItemFromSlot(s.puppet,slot,true) end
  if item then
    if not lease.had then ts:GiveItem(s.puppet,item,1) end
    assert(ts:AddItemToSlot(s.puppet,slot,item),"preview attachment refused")
  end
end
-- The double's own headwear, head and arms back on it. Once per session.
local function restoreLeases(s)
  if s.leasesBack then return end
  s.leasesBack=true
  local player=Game.GetPlayer()
  if s.puppet and IsDefined(s.puppet) and (not player or identity(player)~=identity(s.puppet)) then
    local ts=Game.GetTransactionSystem()
    for i=#s.leases,1,-1 do
      local l=s.leases[i];local current=ts:GetItemInSlot(s.puppet,l.slot)
      if l.item and current and current:GetItemID()==l.item then ts:RemoveItemFromSlot(s.puppet,l.slot,true);current=nil end
      if not current and l.old and ts:HasItem(s.puppet,l.old) then ts:AddItemToSlot(s.puppet,l.slot,l.old) end
      if l.item and not l.had then ts:RemoveItem(s.puppet,l.item,1) end
    end
  end
end
local function restore(s)
  restoreLeases(s)
  if s.target then VRCreationTarget(nil);s.target=false end
end
-- Seconds: after the editor head comes off (it strips the double), and after the double is rebuilt.
local STEP=1.0
-- Seconds the double's own customization controllers take to rebuild it after a finalize; the system's reset
-- waits that long (a reset in the finalize's frame cuts them off: a bald reflection).
local SETTLE=2.0
-- The game's reset first takes the session's parts off the system's target -- the mirror double, by resource, its
-- own copies too. VRCustomizationReset resets with no target and gives the game its target back.
local function resetOptions()
  if type(VRCustomizationReset)=="function" and VRCustomizationReset() then return end
  system():ResetCustomizationOptionsResource()
end
local function resetSystem()
  shared.appearanceResetIn=nil
  pcall(resetOptions)
end
-- ApplyChangeToOption ignores a value equal to the option's current one. With the current index cleared the game
-- takes the value as new: loads its resources, puts its parts on the system's target, and removes nothing.
local function reapplyAll(c,final)
  local n=0
  for _,o in ipairs(c:GetUnitedOptions(true,true,true)) do
    if o.isActive then
      local index=o.currIndex
      if final then final[NameToString(o.info.name)]=index end
      o.currIndex=4294967295
      c:ApplyChangeToOption(o,index,true)
      if o.currIndex~=index then o.currIndex=index end
      n=n+1
    end
  end
  return n
end
-- Undo the preview on the mirror double: every value the panel offers goes back to the finalized one through the
-- same change (the preview's parts off, the finalized ones on); equal values are left alone by the game.
local function revert(s)
  if not s.final or not s.rows then return end
  local c=system()
  for _,row in ipairs(s.rows) do
    local index=s.final[row.key]
    if index then pcall(function() c:ApplyChangeToOption(row.option,index,true) end) end
  end
end
-- Taking the editor head off strips the double of its customization parts (hair, beard, piercings; seen on the
-- first edit after a load). Put the finalized state back on it, then finalize those same values: the double's own
-- customization controllers then rebuild it from them, one copy of each part.
local function rebuild()
  local c=system()
  c:InitializeOptionsFromFinalizedState()
  reapplyAll(c)
  c:ReFinalizeState()
end
-- mode: nil = the scene goes on here; "now" = the request goes back to the game's own editor at once;
-- "native" = this session hands it to that editor.
local function finish(reason,mode)
  local s=session();if not s then return end
  local touched=s.target
  pcall(restore,s)
  -- The game's own editor shows an empty screen while the double's option resource is still held: its
  -- SetCustomizationOptionsResource loads nothing and never sends it the options list. Reset at once when the
  -- request goes to that editor; otherwise once the double has settled.
  if touched then
    if mode then resetSystem() else shared.appearanceResetIn=SETTLE end
  end
  -- The mirror scene waits for the menu to return to Idle. Handed to the game's own editor, the request is
  -- that editor's to complete; otherwise (or when it can no longer be handed over) the scene is completed here.
  -- A request that went to the native editor at the start is unknown to these natives: false, nothing done.
  local ok,handed,completed=false,false,false
  if mode=="native" and s.panel and type(VRMirrorAppearanceNative)=="function" then
    ok,handed=pcall(VRMirrorAppearanceNative,s.panel);handed=ok and handed==true
  end
  if not handed and s.panel and type(VRMirrorAppearanceFinish)=="function" then
    ok,completed=pcall(VRMirrorAppearanceFinish,s.panel);completed=ok and completed==true
  end
  shared.appearanceLast={phase="closed",reason=reason,error=s.error,accepted=s.accepted,sceneCompleted=completed,native=handed}
  shared.appearanceSession=nil
end
-- The session ends in steps (A.poll): the double's own head back now, rebuilt a STEP later, then the system let go
-- and the request completed (or handed to the game's own editor) a STEP after that.
local function close(s,reason,mode)
  pcall(restoreLeases,s)
  s.phase="restoring";s.stage=1;s.stageAt=s.age+STEP;s.endReason=reason;s.endMode=mode
end
function A.rows()
  local rows={}
  for _,o in ipairs(system():GetUnitedOptions(true,true,true)) do
    if o.isEditable and o.isActive and not o.isCensored then
      local info=o.info;local kind=NameToString(info:GetClassName())
      local values=kind=="gameuiAppearanceInfo" and info.definitions
        or kind=="gameuiMorphInfo" and info.morphNames or kind=="gameuiSwitcherInfo" and info.options
      if values and #values>1 then rows[#rows+1]={option=o,key=NameToString(info.name),label=loc(info.localizedName),values=values} end
    end
  end
  return rows
end
function A.route(panel)
  local s=session()
  if not s then
    -- Whoever takes this request -- the world editor or the game's own -- starts from a clean system.
    if shared.appearanceResetIn then resetSystem() end
    if not shared.appearanceEnabled() or type(VRMirrorAppearanceOptions)~="function" then return false end
    local puppet=mirrorDouble();local player=Game.GetPlayer()
    if not puppet or identity(puppet)==identity(player) then return false end
    s={phase="preparing",age=0,panel=panel,puppet=puppet,player=identity(player),leases={},chosen={},anchor=player:GetWorldPosition()}
    shared.appearanceSession=s
    local ok,err=pcall(function()
      leaseSlot(s,TweakDBID.new("AttachmentSlots.Head"))
      leaseSlot(s,TweakDBID.new("AttachmentSlots.Eyes"))
      s.male=NameToString(player:GetResolvedGenderName())=="Male"
      for _,path in ipairs({s.male and "Items.CharacterCustomizationMaHead" or "Items.CharacterCustomizationWaHead","Items.CharacterCustomizationArms"}) do
        local item=ItemID.FromTDBID(TweakDBID.new(path));leaseSlot(s,EquipmentSystem.GetPlacementSlot(item),item)
      end
    end)
    if not ok then s.error=tostring(err);finish("prepare-failed");return false end
  end
  if s.phase=="ready" then s.phase="building";return true end
  if s.phase=="failed" or s.age>8 then
    if s.target then pcall(function() system():CancelFinalizedStateUpdate() end) end
    finish("native-fallback","now");return false
  end
  return nil -- keep the native request until the world editor is ready
end
function A.poll(dt)
  dt=math.max(0,dt or 0)
  local s=session()
  if not s then
    if shared.appearanceResetIn then
      shared.appearanceResetIn=shared.appearanceResetIn-dt
      if shared.appearanceResetIn<=0 then resetSystem() end
    end
    return false
  end
  s.age=s.age+dt
  local player=Game.GetPlayer()
  if not player or identity(player)~=s.player then finish("player-changed");return false end
  if s.phase=="restoring" then
    if s.age>=s.stageAt then
      if s.stage==1 and s.target and s.puppet and IsDefined(s.puppet) then
        local ok,err=pcall(rebuild);if not ok then s.error=tostring(err) end
        s.stage=2;s.stageAt=s.age+STEP
      else
        finish(s.endReason,s.endMode)
      end
    end
    return false
  end
  if not s.puppet or not IsDefined(s.puppet) or Vector4.Distance(player:GetWorldPosition(),s.anchor)>4 then A.cancel();return false end
  if s.phase=="preparing" and s.age>=.35 then
    local ok,err=pcall(function()
      local ts=Game.GetTransactionSystem()
      for _,l in ipairs(s.leases) do if l.item and not ts:GetItemInSlot(s.puppet,l.slot) then return end end
      local c=system()
      assert(VRCreationTarget(s.puppet),"mirror target rejected");s.target=true
      resetOptions()
      assert(VRMirrorAppearanceOptions(s.puppet,s.male),"mirror option resources not ready")
      c:InitializeOptionsFromFinalizedState();c:ApplyEditTag(gameuiCharacterCustomizationEditTag.HairDresser)
      -- The editor head and arms are new items every session; unchanged options would put nothing on them
      -- (no face, no hands). The finalized values are kept for the cancel.
      s.final={};s.reapplied=reapplyAll(c,s.final)
      s.rows=A.rows();assert(#s.rows>=10,"incomplete appearance options")
      s.phase="ready"
    end)
    if not ok then s.error=tostring(err);s.phase="failed" end
  end
  return s.phase=="building"
end
function A.value(row)
  local s=session();local chosen=s and s.chosen[row.key];local index=row.option.currIndex
  if chosen and index~=chosen.index and s.age-chosen.at<2 then index=chosen.index
  elseif chosen then s.chosen[row.key]=nil end
  if index<0 or index>=#row.values then index=0 end
  local label=loc(row.values[index+1] and row.values[index+1].localizedName)
  return index,(label~="" and label.."  " or "")..(index+1).."/"..#row.values
end
-- Applied at once: the game's own deferred change (0.3 s) could otherwise land after the session ended.
function A.change(row,step)
  local s=session();if not s or s.phase~="active" then return false end
  local index=(A.value(row)+step)%#row.values
  local ok,err=pcall(function() system():ApplyChangeToOption(row.option,index,true) end)
  if not ok then s.error=tostring(err);return false end
  s.chosen[row.key]={index=index,at=s.age};s.refreshAt=s.age+.25;return true
end
function A.action(accept)
  local s=session();if not s or s.phase=="restoring" or (accept and s.phase~="active") then return false end
  local ok,result=pcall(function()
    if accept then return system():ReFinalizeState() end
    if s.target then revert(s);return system():CancelFinalizedStateUpdate() end
    return true
  end)
  if not ok or result==false then s.error=tostring(result);return false end
  s.accepted=accept
  close(s,accept and "accepted" or "cancelled")
  return true
end
function A.cancel() return A.action(false) end
-- The game's own editor instead: the unsaved preview is undone, the double rebuilt, and then the retained request
-- opens that editor as if it had never been taken. True when the world editor let go of the request.
function A.native()
  local s=session();if not s or s.phase~="active" or type(VRMirrorAppearanceNative)~="function" then return false end
  pcall(function() revert(s);system():CancelFinalizedStateUpdate() end)
  close(s,"native","native")
  return true
end
function A.release()
  local s=session()
  if not s then
    if shared and shared.appearanceResetIn then resetSystem() end
    return
  end
  if s.target and s.phase~="restoring" then pcall(function() revert(s);system():CancelFinalizedStateUpdate() end) end
  finish("released","now")
end
function A.beforeBuild() end
function A.present(kit)
  local s=session();if not s or not kit.S.menu or not kit.S.menu.placed then return end
  if s.phase=="building" then s.phase="active" end
  if s.refreshAt and s.age>=s.refreshAt then s.refreshAt=nil;s.rows=A.rows();kit.rebuild(false,false) end
end
function A.current() return session() end
function A.state()
  local s=session();return s and {phase=s.phase,rows=s.rows and #s.rows or 0,error=s.error,world=true,mirror=identity(s.puppet),reapplied=s.reapplied}
    or shared and shared.appearanceLast or {phase="idle"}
end
function A.bind(state,enabled) shared=state;shared.appearanceRouting=A;shared.appearanceEnabled=enabled end
return A
