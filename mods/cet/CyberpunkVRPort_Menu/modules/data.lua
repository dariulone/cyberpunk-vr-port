-- What the menu lists and what its buttons do -- each one the game's own call, read out of the HUD code
-- that does the same thing on a gamepad:
--
--   contacts   MessengerUtils.GetCallableAndNonEmptyContacts (newHudPhoneGameController, Contacts tab)
--   call       questTriggerCallRequest to the PhoneSystem (NewHudPhoneGameController.CallSelectedContact),
--              gated by PhoneSystem.IsCallingEnabled like the phone's own button
--   messages   StartHubMenuEvent "phone" + MessageMenuAttachmentData (messagePopup.GotoMessengerMenu)
--   threads    MessengerUtils.GetSimpleContactDataArray / GetMessageDataArrayForContact, sorted like the
--              phone's Unread tab (quest first, then unread, then newest); a thread's messages and reply
--              options from JournalManager.GetMessagesAndChoices (one conversation) or
--              GetFlattenedMessagesAndChoices (a contact with one), a reply sent the way
--              MessengerDialogViewController.ActivateReply does (the pick Succeeded, the others Inactive)
--   vehicles   VehiclesManagerDataHelper.GetVehicles + QuickSlotsManager.SetActiveVehicle /
--              SummonActiveVehicle (vehiclesManagerPopup.Activate)
--   radio      the stations GetRadioStations lists, + QuickSlotsManager.SendRadioEvent
--              (vehicleRadioPopup.Activate); the receiver is the car's when mounted, else the radioport
--   volume     /audio/volume CarRadioVolume or RadioportVolume, a ConfigVarInt set within its own range
--              (RadioVolumeSettingsController.ChangeValue steps it the same way)
--   game menu  the pause menu's entries (PauseMenuGameController.PopulateMenuItemList): the pause menu
--              opened through the in-game menu controller, then its page; the exit is
--              SystemRequestsHandler.GotoMainMenu, as the pause menu's own exit
--
-- Every call is wrapped: a nil from any of them means "not now", never an error on the screen.

local D = {}

local function player() return Game.GetPlayer() end

local function loc(s)
  if s == nil then return "" end
  s = tostring(s)
  if string.find(s, "^LocKey#") then
    local ok, t = pcall(function() return Game.GetLocalizedText(s) end)
    if ok and t ~= nil and t ~= "" then return t end
  end
  return s
end

local function locKey(name)
  local ok, t = pcall(function() return Game.GetLocalizedTextByKey(name) end)
  if ok and t ~= nil and t ~= "" then return t end
  return ""
end

local function icon(rec)
  if rec == nil then return nil end
  local atlas, part
  pcall(function()
    atlas = rec:AtlasResourcePath()
    part = rec:AtlasPartName()
  end)
  if atlas == nil or part == nil then return nil end
  local name = nil
  pcall(function() name = NameToString(part) end)
  return { atlas = atlas, part = part, name = name }
end

local function mounted()
  local pl = player()
  if pl == nil then return nil end
  local v = nil
  pcall(function() v = Game.GetMountedVehicle(pl) end)
  return v
end
D.mounted = mounted

local function qsm()
  local pl = player()
  if pl == nil then return nil end
  local m = nil
  pcall(function() m = pl:GetQuickSlotsManager() end)
  return m
end

local function phoneSystem()
  local ps = nil
  pcall(function() ps = Game.GetScriptableSystemsContainer():Get(CName.new("PhoneSystem")) end)
  return ps
end

-- Settings are read only by names the game itself uses, and only after HasGroup / HasVar answer yes:
-- GetVar with an unknown name is a native assert that kills the game, and pcall does not catch it.
local function settingVar(group, name)
  local ss = nil
  pcall(function() ss = Game.GetSettingsSystem() end)
  if ss == nil then return nil end
  local ok, has = pcall(function() return ss:HasGroup(group) and ss:GetGroup(group):HasVar(CName.new(name)) end)
  if not ok or not has then return nil end
  local v = nil
  pcall(function() v = ss:GetVar(group, CName.new(name)) end)
  return v
end

function D.language()
  local v = settingVar("/language", "OnScreen")
  local lang = nil
  if v ~= nil then pcall(function() lang = NameToString(v:GetValue()) end) end
  return lang or "en-us"
end

---------------------------------------------------------------- the phone's threads

local function journal()
  local j = nil
  pcall(function() j = Game.GetJournalManager() end)
  return j
end

-- A journal hash as GetEntry takes it: the Int32 the ContactData carries, read unsigned.
local function u32(h)
  h = math.floor(tonumber(h) or 0)
  if h < 0 then h = h + 4294967296 end
  return h
end

local function enum(e)
  local ok, n = pcall(function() return tonumber(EnumInt(e)) end)
  return ok and n or -1
end

local function clock(t)
  local ok, h, m = pcall(function() return GameTime.Hours(t), GameTime.Minutes(t) end)
  if not ok or h == nil then return "" end
  return string.format("%02d:%02d", h % 24, m % 60)
end

local function stamp(t)
  local ok, s = pcall(function() return GameTime.GetSeconds(t) end)
  return ok and tonumber(s) or 0
end

function D.locKey(name, fallback)
  local ok, t = pcall(function() return Game.GetLocalizedTextByKey(name) end)
  if ok and t ~= nil and t ~= "" then return t end
  return fallback or ""
end

-- The threads, as the phone lists them: every one (showAll) or only those still unread or waiting for a
-- reply -- or every thread of one contact (contactHash).
function D.threads(showAll, contactHash)
  local out = {}
  local j = journal()
  if j == nil then return out end
  local list = nil
  pcall(function()
    if contactHash ~= nil then
      list = MessengerUtils.GetMessageDataArrayForContact(j, contactHash, true, true)
    else
      list = MessengerUtils.GetSimpleContactDataArray(j, true, true, showAll == true)
    end
  end)
  if list == nil then return out end
  local you = D.locKey("UI-Phone-LabelYou", "You")
  local multi = enum(MessengerContactType.MultiThread)
  for i = 1, #list do
    local c = list[i]
    pcall(function()
      local preview
      if c.hasValidTitle then
        preview = loc(c.localizedPreview)
      else
        preview = loc(c.lastMesssagePreview)
        if c.playerIsLastSender then preview = you .. ": " .. preview end
      end
      local avatar = nil
      pcall(function() avatar = icon(TweakDBInterface.GetUIIconRecord(c.avatarID)) end)
      out[#out + 1] = {
        name = loc(c.localizedName),
        preview = preview,
        unread = #(c.unreadMessages or {}),
        canReply = c.playerCanReply == true,
        quest = c.questRelated == true or c.hasQuestImportantReply == true,
        time = clock(c.timeStamp),
        at = stamp(c.timeStamp),
        multi = enum(c.type) == multi,
        hash = c.hash,
        conversationHash = c.conversationHash,
        callable = c.isCallable == true,
        contactId = c.contactId,
        avatar = avatar,
      }
    end)
  end
  table.sort(out, function(a, b)
    if a.quest ~= b.quest then return a.quest end
    if (a.unread > 0) ~= (b.unread > 0) then return a.unread > 0 end
    return a.at > b.at
  end)
  return out
end

-- A thread's messages, oldest first, and the replies the player can send now. A message is
-- { text, player, quest, image = { atlas, part } or nil }; a reply { text, quest }; both carry their
-- journal entry and hash.
function D.thread(t)
  local out = { messages = {}, replies = {} }
  local j = journal()
  if j == nil or t == nil then return out end
  local msgs, choices = nil, nil
  pcall(function()
    if t.multi then
      msgs, choices = j:GetMessagesAndChoices(j:GetEntry(u32(t.conversationHash)))
    else
      msgs, choices = j:GetFlattenedMessagesAndChoices(j:GetEntry(u32(t.hash)))
    end
  end)
  local player = enum(gameMessageSender.Player)
  for i = 1, #(msgs or {}) do
    local m = msgs[i]
    pcall(function()
      local rec = { entry = m, hash = j:GetEntryHash(m), text = loc(m:GetText()), quest = m:IsQuestImportant() }
      if m:IsA("gameJournalPhoneMessage") then
        rec.player = enum(m:GetSender()) == player
        local img = m:GetImageID()
        if TDBID.IsValid(img) then
          pcall(function() rec.image = icon(TweakDBInterface.GetUIIconRecord(img)) end)
        end
      else
        rec.player = true          -- a reply the player already sent
      end
      out.messages[#out.messages + 1] = rec
    end)
  end
  for i = 1, #(choices or {}) do
    local c = choices[i]
    pcall(function()
      out.replies[#out.replies + 1] = { entry = c, hash = j:GetEntryHash(c), text = loc(c:GetText()), quest = c:IsQuestImportant() }
    end)
  end
  return out
end

-- What opening a thread does in the game: every message in it read.
function D.markRead(messages)
  local j = journal()
  if j == nil then return end
  for _, m in ipairs(messages or {}) do
    pcall(function()
      if not j:IsEntryVisited(m.entry) then j:SetEntryVisited(m.entry, true) end
    end)
  end
end

-- Send `pick`, one of `replies`: the others are dropped, as in the game.
function D.reply(replies, pick)
  local j = journal()
  if j == nil or pick == nil then return false end
  return pcall(function()
    for _, r in ipairs(replies) do
      if r.hash ~= pick.hash then
        j:SetEntryVisited(r.entry, true)
        j:ChangeEntryStateByHash(u32(r.hash), gameJournalEntryState.Inactive, JournalNotifyOption.Notify)
      end
    end
    j:SetEntryVisited(pick.entry, true)
    j:ChangeEntryStateByHash(u32(pick.hash), gameJournalEntryState.Succeeded, JournalNotifyOption.Notify)
  end)
end

-- The phone journal's ear (mods/redscript/CyberpunkVRPort_Menu): counters of thread changes and of
-- messages scheduled to arrive, with the last one's delay and sender. nil without the script.
function D.journalWatch()
  local w = nil
  pcall(function() w = player():VRMenuJournalWatch() end)
  return w
end

---------------------------------------------------------------- contacts

function D.contacts()
  local out = {}
  local list = nil
  pcall(function() list = MessengerUtils.GetCallableAndNonEmptyContacts(Game.GetJournalManager()) end)
  if list == nil then return out end
  for i = 1, #list do
    local c = list[i]
    pcall(function()
      local name = loc(c.localizedName)
      if name ~= "" then
        local avatar = nil
        pcall(function() avatar = icon(TweakDBInterface.GetUIIconRecord(c.avatarID)) end)
        out[#out + 1] = {
          name = name,
          contactId = c.contactId,
          callable = c.isCallable,
          messages = (c.messagesCount or 0) > 0 or (c.repliesCount or 0) > 0 or c.playerCanReply or c.hasMessages,
          unread = c.unreadMessegeCount or 0,
          hash = c.hash,
          conversationHash = c.conversationHash,
          avatar = avatar,
        }
      end
    end)
  end
  table.sort(out, function(a, b) return string.lower(a.name) < string.lower(b.name) end)
  return out
end

function D.callingEnabled()
  local ps = phoneSystem()
  if ps == nil then return false end
  local ok, on = pcall(function() return ps:IsCallingEnabled() end)
  return ok and on == true
end

function D.call(contact)
  if contact == nil or not contact.callable then return false, "unavailable" end
  local ps = phoneSystem()
  if ps == nil or not D.callingEnabled() then return false, "blocked" end
  local ok = pcall(function()
    local r = questTriggerCallRequest.new()
    r.addressee = CName.new(contact.contactId)
    r.caller = CName.new("Player")
    r.callPhase = questPhoneCallPhase.IncomingCall
    r.callMode = questPhoneCallMode.Video
    ps:QueueRequest(r)
  end)
  return ok, ok and "calling" or "failed"
end

-- The hub's messages page, opened on this contact's thread -- the game's notification popup does this.
-- The hub listens through the in-game menu controller (OnRequestHubMenu); init.lua hands that in.
function D.openMessages(contact, menuController)
  if contact == nil then return false end
  local ok = pcall(function()
    local data = MessageMenuAttachmentData.new()
    data.entryHash = contact.hash
    local evt = StartHubMenuEvent.new()
    evt:SetStartMenu(CName.new("phone"), CName.new(""), data)
    if menuController ~= nil then
      menuController:OnRequestHubMenu(evt)
    else
      Game.GetUISystem():QueueEvent(evt)
    end
  end)
  return ok
end

---------------------------------------------------------------- vehicles

-- (CET names a script class's fields without their m_ prefix: d.displayName, not d.m_displayName --
-- the prefixed name reads nil and writes nothing.)
function D.vehicles()
  local out = {}
  local pl = player()
  if pl == nil then return out end
  local list = nil
  pcall(function() list = VehiclesManagerDataHelper.GetVehicles(pl) end)
  if list == nil then return out end
  for i = 1, #list do
    local d = list[i]
    pcall(function()
      local name = locKey(d.displayName)
      if name == "" then name = NameToString(d.displayName) end
      -- the vehicle popup's own type icon: a bike, a car with mounted guns, or a car
      local kind = "car"
      pcall(function()
        if d.data.vehicleType == gamedataVehicleType.Bike then
          kind = "motorcycle"
        elseif TweakDBInterface.GetVehicleRecord(d.data.recordID):VehDataPackageHandle():DriverCombat():Type()
            == gamedataDriverCombatType.MountedWeapons then
          kind = "weaponized"
        end
      end)
      local picture = nil
      pcall(function() picture = icon(d.icon) end)
      out[#out + 1] = {
        name = name,
        picture = picture,
        data = d.data,
        canBeActive = d.canBeActive,
        repair = d.repairTimeRemaining or 0.0,
        kind = kind,
      }
    end)
  end
  table.sort(out, function(a, b) return string.lower(a.name) < string.lower(b.name) end)
  return out
end

function D.summonRestricted()
  -- The host fills the GameInstance parameter itself; passing one fails the call ("requires 0 parameter(s)").
  local ok, r = pcall(function() return VehicleSystem.IsSummoningVehiclesRestricted() end)
  return ok and r == true
end

function D.summon(v)
  if v == nil then return false, "unavailable" end
  if mounted() ~= nil then return false, "mounted" end
  if D.summonRestricted() then return false, "blocked" end
  if v.repair > 0.0 then return false, "repair" end
  local m = qsm()
  if m == nil then return false, "failed" end
  local ok = pcall(function()
    if v.canBeActive then
      m:SetActiveVehicle(v.data)
      m:SummonActiveVehicle(true)
    else
      m:SummonVehicle(true, v.data.vehicleType, v.data.recordID, v.data.spawnOnlyOnValidRoad)
    end
  end)
  return ok, ok and "summoned" or "failed"
end

---------------------------------------------------------------- radio

-- The popup's own list, in its own order (VehiclesManagerDataHelper.GetRadioStations). Read straight from
-- TweakDB: the helper's RadioListItemData holds its record by a WEAK reference, which CET hands back as nil.
local STATIONS = { "Downtempo", "AggroIndie", "GrowlFM", "Jazz", "ElectroIndie", "MinimTech", "Metal", "Pop",
  "Impulse", "HipHop", "AggroTechno", "Latino", "AttRock", "DarkStar" }
local stationCache = nil

function D.stations()
  if stationCache ~= nil then return stationCache end
  local out = {}
  for _, key in ipairs(STATIONS) do
    pcall(function()
      local rec = TweakDBInterface.GetRadioStationRecord(TweakDBID.new("RadioStation." .. key))
      local index = rec:Index()
      if index >= 0 then
        local logo = nil
        pcall(function() logo = icon(rec:Icon()) end)
        out[#out + 1] = { name = loc(rec:DisplayName()), index = index, logo = logo }
      end
    end)
  end
  if #out > 0 then stationCache = out end
  return out
end

-- The receiver that plays: the car's while mounted, otherwise the radioport.
function D.radio()
  local st = { active = false, station = "", track = "", index = -1 }
  local pl = player()
  if pl == nil then return st end
  local v = mounted()
  pcall(function()
    local stationName, trackName
    if v ~= nil then
      st.active = v:IsRadioReceiverActive()
      stationName = v:GetRadioReceiverStationName()
      trackName = v:GetRadioReceiverTrackName()
    else
      local r = pl:GetPocketRadio()
      st.active = r:IsActive()
      stationName = r:GetStationName()
      trackName = r:GetTrackName()
    end
    if st.active then
      st.station = locKey(stationName)
      st.track = locKey(trackName)
    end
  end)
  if st.active and st.station ~= "" then
    for _, s in ipairs(D.stations()) do
      if s.name == st.station then st.index = s.index end
    end
  end
  return st
end

function D.tune(index)
  local m = qsm()
  if m == nil then return false end
  return pcall(function() m:SendRadioEvent(true, true, index) end)
end

function D.radioOff()
  local m = qsm()
  if m == nil then return false end
  return pcall(function() m:SendRadioEvent(false, false, -1) end)
end

local function volumeVar()
  return settingVar("/audio/volume", mounted() ~= nil and "CarRadioVolume" or "RadioportVolume")
end

function D.volume()
  local v = volumeVar()
  if v == nil then return nil end
  local value, lo, hi = nil, 0, 100
  pcall(function() value, lo, hi = v:GetValue(), v:GetMinValue(), v:GetMaxValue() end)
  return value, lo, hi
end

-- How loud the game plays its effects, 0..1: the master slider times the effects slider (both 0..100).
function D.effectsVolume()
  local function get(name)
    local v, n = settingVar("/audio/volume", name), nil
    if v ~= nil then pcall(function() n = v:GetValue() end) end
    return (type(n) == "number") and math.max(0, math.min(100, n)) / 100 or 1.0
  end
  return get("MasterVolume") * get("SfxVolume")
end

-- Whole steps of one, clamped to the var's own range; the value that took, or nil.
function D.setVolume(n)
  local v = volumeVar()
  if v == nil or n == nil then return nil end
  local out = nil
  pcall(function()
    n = math.max(v:GetMinValue(), math.min(v:GetMaxValue(), math.floor(n + 0.5)))
    if n ~= v:GetValue() then v:SetValue(n) end
    out = n
  end)
  return out
end

---------------------------------------------------------------- the game menu

-- A UI string by the game's own key, or the fallback when the key does not resolve.
function D.label(key, fallback)
  local ok, t = pcall(function() return Game.GetLocalizedText(key) end)
  if ok and t ~= nil and t ~= "" and t ~= key then return t end
  return fallback
end

function D.savesCount()
  local n = 0
  pcall(function() n = Game.GetSystemRequestsHandler():RequestSavesCountSync() end)
  return n or 0
end

-- An event for the menu scenario that is current (gameuiBaseMenuGameController.SpawnMenuInstanceEvent):
-- OnOpenPauseMenu from gameplay, then OnSwitchToSaveGame / OnSwitchToLoadGame / OnSwitchToSettings /
-- OnCreditsPicker inside the pause menu.
function D.menuEvent(ctrl, name)
  if ctrl == nil or name == nil then return false end
  return pcall(function() ctrl:SpawnMenuInstanceEvent(CName.new(name)) end)
end

-- The same shortcut as the game's map binding: native hub initialization and restrictions.
function D.openMap(ctrl)
  local player=Game.GetPlayer()
  if not ctrl or not player or HubMenuUtility.IsPlayerHardwareDisabled(player) then return false end
  return pcall(function() ctrl:OpenShortcutMenu(CName.new("OpenMapMenu")) end)
end

function D.exitToMainMenu()
  return pcall(function() Game.GetSystemRequestsHandler():GotoMainMenu() end)
end

-- Straight to the desktop, as the main menu's "Close Game" does (no save).
function D.exitGame()
  return pcall(function() Game.GetSystemRequestsHandler():ExitGame() end)
end

return D
